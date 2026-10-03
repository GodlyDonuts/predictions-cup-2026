import {amount,money,TICK,instant} from '../paper/fixed.mjs';
import {digest,initialState,invalidate,stepDip,report,validateConfig} from './model.mjs';
import {oneShareDepth} from './edge-depth-audit.mjs';
export const BID_ANCHOR_POLICY=Object.freeze({historyMs:600000,minimumMinutes:5,minimumSpanMs:240000,maximumGapMs:90000,maximumAnchorRange:'0.015000',minimumNetConcession:'0.010000',maximumBidDrop:'0.040000',maximumSpread:'0.020000',horizonMs:300000,cooldownMs:900000,cumulativeBuyBudget:'100.000000',maxOpenMarkets:3,latenciesMs:[1000,3000]});
const need=(ok,msg)=>{if(!ok)throw Error('BID_ANCHOR_'+msg);};
export function bidAnchorSignal(points,b,policy=BID_ANCHOR_POLICY){
 const now=instant(b.receivedAt),a=points.filter(p=>instant(p.at)<now&&instant(p.at)>=now-policy.historyMs);
 const reject=reason=>({admitted:false,reason,samples:a.length,anchor:null});
 if(a.length<policy.minimumMinutes)return reject('history_minutes');
 if(instant(a.at(-1).at)-instant(a[0].at)<policy.minimumSpanMs)return reject('history_span');
 if(now-instant(a.at(-1).at)>policy.maximumGapMs||a.some((p,i)=>i&&instant(p.at)-instant(a[i-1].at)>policy.maximumGapMs))return reject('history_gap');
 const values=a.map(p=>amount(p.bid)).sort((x,y)=>x<y?-1:x>y?1:0),anchor=values[Math.floor((values.length-1)/2)],low=values[0],high=values.at(-1),bid=amount(b.bids[0].price),ask=amount(b.asks[0].price),entry=ask+TICK;
 if(high-low>amount(policy.maximumAnchorRange))return reject('unstable_anchor');
 if(ask-bid>amount(policy.maximumSpread))return reject('spread');
 if(anchor-bid>amount(policy.maximumBidDrop))return reject('bid_collapse');
 if(b.asks[0].quantity<100)return reject('entry_depth');
 // Prior bid less adverse exit tick must exceed the padded entry by the margin.
 const margin=anchor-TICK-entry;
 return {admitted:margin>=amount(policy.minimumNetConcession),reason:margin>=amount(policy.minimumNetConcession)?'ask_concession':'insufficient_concession',samples:a.length,anchor:money(anchor),modeledReference:money(anchor-TICK),concession:money(margin),historyFirstAt:a[0].at,historyLastAt:a.at(-1).at};
}
export function createBidAnchorState(bank){
 need(digest(bank.policy)===digest(BID_ANCHOR_POLICY),'POLICY');validateConfig(bank.config);
 return {bankHash:digest(bank),histories:{},seen:{},counts:{},signals:[],wallets:Object.fromEntries(bank.policy.latenciesMs.map(latency=>{const state=initialState({...bank.config,latencyMs:latency},'bid-anchor:'+latency,bank.cutoff);state.lastAt='1970-01-01T00:00:00.000Z';return [latency,{state,cooldowns:{}}];}))};
}
export function advanceBidAnchor(s,row,bank){
 need(s.bankHash===digest(bank),'BANK');const p=bank.policy,inc=key=>s.counts[key]=(s.counts[key]||0)+1;
 if(['session_start','session_stop','invalidation'].includes(row.kind)){
  const id=row.kind==='invalidation'?row.payload.marketId:null;
  if(id===null){s.histories={};s.seen={};}else{delete s.histories[id];delete s.seen[id];}
  for(const w of Object.values(s.wallets)){invalidate(w.state,id,row.at,'bid_anchor_source_boundary');for(const [key,i] of Object.entries(w.state.strategies.mean_reversion.pending))if(i.action==='buy'&&(id===null||w.state.books[key]?.marketId===id))delete w.state.strategies.mean_reversion.pending[key];}
  inc('source_boundaries');return s;
 }
 if(row.kind!=='book')return s;const b=row.payload.book,id=b.marketId;
 need(bank.universe.exchanges.some(x=>x.marketId===id&&x.exchangeId===b.exchangeId)&&b.tournamentId===bank.config.tournamentId,'SCOPE');
 if(s.seen[id]===b.versionKey){inc('duplicate_versions');return s;}s.seen[id]=b.versionKey;
 const points=s.histories[id]||[],signal=bidAnchorSignal(points,b,p),post=instant(b.requestStartedAt)>instant(bank.cutoff);
 if(post){inc(signal.reason);if(signal.admitted){s.signals.push({marketId:id,at:row.at,sourceSequence:row.sequence,sourceHash:row.hash,version:b.versionKey,...signal});}}
 for(const [latency,w] of Object.entries(s.wallets)){
  const c={...bank.config,latencyMs:Number(latency)};
  if(!post)continue;
  const st=w.state.strategies.mean_reversion,wasHeld=Boolean(st.positions[b.instrument]),spent=st.fills.filter(f=>f.action==='buy').reduce((n,f)=>n+amount(f.price)*BigInt(f.quantity),0n),reserved=Object.values(st.pending).filter(i=>i.action==='buy').reduce((n,i)=>n+amount(i.limit)*BigInt(i.quantity),0n),occupied=new Set([...Object.keys(st.positions),...Object.keys(st.pending)]);
  // Keep a prepared order's original causal anchor during its short lifetime.
  // Its own trough must not retroactively change the reference used to prepare it.
  const pending=st.pending[b.instrument],reference=pending?.action==='buy'?pending.anchorReference:signal.modeledReference;
  const anchoredPending=pending?.action==='buy'&&reference&&amount(reference)+TICK-amount(b.bids[0].price)<=amount(p.maximumBidDrop)&&amount(b.asks[0].price)-amount(b.bids[0].price)<=amount(p.maximumSpread);
  const allow=(signal.admitted||Boolean(anchoredPending))&&instant(row.at)>=(w.cooldowns[id]||0)&&spent+reserved+(pending?.action==='buy'?0n:amount(b.asks[0].price)+TICK)<=amount(p.cumulativeBuyBudget)&&(occupied.has(b.instrument)||occupied.size<p.maxOpenMarkets);
  w.state=stepDip(w.state,b,c,{reference:reference?Number(amount(reference))/1e6:null,allowEntry:allow,minimumEdge:.01-4*Number.EPSILON,takeProfit:'0.010000',horizonMs:p.horizonMs,maxMarketCost:'1.000000',executionPolicy:'responsive_v2'});
  const next=w.state.strategies.mean_reversion;if(next.intents>st.intents&&next.pending[b.instrument]){next.pending[b.instrument].createdAt=row.at;if(next.pending[b.instrument].action==='buy')next.pending[b.instrument].anchorReference=reference;}
  const exit=next.pending[b.instrument];
  if(exit?.action==='sell'&&exit.exitPolicy==='risk_reprice_v2'){
   const depth=oneShareDepth(b.bids,'sell');
   if(depth.status==='available'&&amount(depth.modeledPrice)<amount(exit.limit)){exit.limit=depth.modeledPrice;exit.createdAt=row.at;exit.depthRepricedAt=row.at;}
  }
  if(wasHeld&&!next.positions[b.instrument])w.cooldowns[id]=instant(row.at)+p.cooldownMs;
  need(next.fills.filter(f=>f.action==='buy').reduce((n,f)=>n+amount(f.price)*BigInt(f.quantity),0n)<=amount(p.cumulativeBuyBudget),'BUDGET');
 }
 const minute=Math.floor(instant(b.receivedAt)/60000);if(!points.some(x=>x.minute===minute))points.push({minute,at:b.receivedAt,bid:b.bids[0].price});s.histories[id]=points.filter(x=>instant(x.at)>=instant(b.receivedAt)-p.historyMs);return s;
}
export function bidAnchorReport(s,bank,at){return {mode:'bid_anchor_causal_capture_replay',bankHash:s.bankHash,cutoff:bank.cutoff,counts:s.counts,signals:s.signals,wallets:Object.fromEntries(Object.entries(s.wallets).map(([latency,w])=>[latency,report(w.state,{...bank.config,latencyMs:Number(latency)},at).strategies.mean_reversion])),networkRequests:0,competitionOrders:0,profitabilityEstablished:false,limitations:['Assumes consumer observes publications promptly; later causal request and fixed1/3second latency are required for fills.','Frozen prospective capture replay is local counterfactual paper accounting, not actual orders.','Prior executable bids are a signal hypothesis, not guaranteed fair value or future bid recovery.','Correlated markets and overlapping latency wallets are not independent evidence.']};}
