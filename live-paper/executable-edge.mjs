import {digest,initialState,validateConfig,observe,stepDip,report,invalidate,assertState} from './model.mjs';
import {amount,money,TICK,SCALE,instant} from '../paper/fixed.mjs';
export const EDGE_POLICY=Object.freeze({horizonMs:300000,labelLatenessMs:60000,trialLifetimeMs:30000,minimumSamples:25,maximumSamples:256,minimumMarkets:3,minimumNetMargin:'0.010000',maximumSpread:'0.030000',cumulativeBuyBudget:'100.000000',maxOpenMarkets:3,cooldownMs:360000});
const need=(x,c)=>{if(!x)throw Error(c);},count=(s,k)=>s.counts[k]=(s.counts[k]||0)+1;
export function createExecutableEdgeBank({config,universe,cutoff,sourcePrefixBytes,sourcePrefixHash,sourcePrefixSequence,sourcePrefixJournalHash,codeIdentity}){
 const c={...structuredClone(config),strategyUniverse:'diversified-dip-v1',marketIds:universe.exchanges.map(x=>x.marketId),feeBps:0,slippageTicks:1,depthFractionBps:100,orderShares:1,maxPositionShares:1,maxExposure:'25.000000',cashReserve:money(amount(config.initialCash)-25000000n),exitAfterMs:300000,stopLoss:'0.020000'};
 validateConfig(c);instant(cutoff);return {schemaVersion:1,name:'executable-edge-v1',config:c,universe:structuredClone(universe),cutoff,sourcePrefixBytes,sourcePrefixHash,sourcePrefixSequence,sourcePrefixJournalHash,codeIdentity,policy:structuredClone(EDGE_POLICY)};
}
export function emptyExecutableEdge(bank){need(digest(bank.policy)===digest(EDGE_POLICY),'EDGE_POLICY_CHANGED');const wallet=initialState(bank.config,'executable-edge:'+bank.cutoff,bank.cutoff);wallet.lastAt='1970-01-01T00:00:00.000Z';return {bankHash:digest(bank),wallet,cells:{},trials:{},seen:{},nextTrialAt:{},cooldowns:{},counts:{},decisions:[],labels:[],lastClock:null};}
export function edgeFeatures(b){
 const bid=amount(b.bids[0].price),ask=amount(b.asks[0].price),spread=ask-bid,bq=BigInt(b.bids[0].quantity),aq=BigInt(b.asks[0].quantity);
 const pressure=bq*5n>=(bq+aq)*3n?'bid_heavy':aq*5n>=(bq+aq)*3n?'ask_heavy':'balanced';
 return {cell:(spread<=3n*TICK?'tight':'wide')+':'+pressure,spread:money(spread),topBidQuantity:b.bids[0].quantity,topAskQuantity:b.asks[0].quantity};
}
export function edgeForecast(cell,policy=EDGE_POLICY){
 const samples=cell?.samples||[],values=samples.map(x=>x.netMicros).sort((a,b)=>a-b),markets=new Set(samples.map(x=>x.marketId));
 const lowerQuartile=values.length?values[Math.floor((values.length-1)/4)]:null;
 // A fixed conservative empirical gate, not an independence-adjusted confidence interval.
 return {samples:values.length,markets:markets.size,lowerQuartile:lowerQuartile===null?null:money(BigInt(lowerQuartile)),admitted:values.length>=policy.minimumSamples&&markets.size>=policy.minimumMarkets&&lowerQuartile>=Number(amount(policy.minimumNetMargin))};
}
function revoke(s,id,at,reason){
 const ids=id===null?Object.keys(s.trials):[id];for(const m of ids){if(s.trials[m])count(s,'trial_'+reason);delete s.trials[m];delete s.seen[m];}
 invalidate(s.wallet,id,at,reason);for(const [key,i]of Object.entries(s.wallet.strategies.mean_reversion.pending))if(i.action==='buy'&&(id===null||s.wallet.books[key]?.marketId===id))delete s.wallet.strategies.mean_reversion.pending[key];
}
export function advanceExecutableEdge(input,row,bank,{availableAt,allowDecisions=false}={}){
 need(input.bankHash===digest(bank)&&digest(bank.policy)===digest(EDGE_POLICY),'EDGE_BANK_CHANGED');const at=instant(availableAt);need(instant(row.at)<=at,'EDGE_FUTURE_PUBLICATION');need(!input.lastClock||at>=instant(input.lastClock),'EDGE_CLOCK_ROLLBACK');const s=structuredClone(input);s.lastClock=availableAt;
 if(['session_start','session_stop','invalidation'].includes(row.kind)){revoke(s,row.kind==='invalidation'?row.payload.marketId:null,row.at,row.kind);return s;}
 if(row.kind!=='book')return s;const b=row.payload.book,id=b.marketId,c=bank.config,p=bank.policy;
 need(bank.universe.exchanges.some(x=>x.marketId===id&&x.exchangeId===b.exchangeId)&&b.tournamentId===c.tournamentId,'EDGE_FOREIGN_BOOK');
 if(at-instant(b.receivedAt)>c.maxDataAgeMs||instant(b.receivedAt)>at){count(s,'stale_consumer_book');revoke(s,id,availableAt,'stale');return s;}
 const prior=s.seen[id],independent=!prior||prior!==b.versionKey;s.seen[id]=b.versionKey;
 const f=edgeFeatures(b),ask=amount(b.asks[0].price)+TICK,bid=amount(b.bids[0].price)-TICK,depth=b.asks[0].quantity>=100&&b.bids[0].quantity>=100;
 let t=s.trials[id];
 if(t&&!t.filledAt){
  if(at-instant(t.preparedAt)>p.trialLifetimeMs){count(s,'trial_unfilled');delete s.trials[id];}
  else if(independent&&b.versionKey!==t.version&&instant(b.requestStartedAt)>=instant(t.preparedAt)+c.latencyMs&&depth&&ask<=amount(t.limit)){t.filledAt=b.receivedAt;t.entry=money(ask);t.fillVersion=b.versionKey;count(s,'trial_filled');}
 }else if(t&&instant(b.requestStartedAt)>=instant(t.filledAt)+p.horizonMs){
  const target=instant(t.filledAt)+p.horizonMs;
  if(at-target>p.labelLatenessMs||!independent||b.versionKey===t.fillVersion||!depth||bid<TICK){count(s,'label_missing');delete s.trials[id];}
  else{const label={marketId:id,cell:t.cell,preparedAt:t.preparedAt,filledAt:t.filledAt,entry:t.entry,exitBid:money(bid),at:b.receivedAt,netMicros:Number(bid-amount(t.entry)),sourceSequence:row.sequence,sourceHash:row.hash,entryVersion:t.fillVersion,exitVersion:b.versionKey};const cell=s.cells[t.cell]??={samples:[]};cell.samples.push(label);if(cell.samples.length>p.maximumSamples)cell.samples.shift();s.labels.push(label);if(s.labels.length>1000)s.labels.shift();count(s,'labels_completed');delete s.trials[id];}
 }
 // Forecast is made from already matured labels; this book cannot label its own entry.
 const forecast=edgeForecast(s.cells[f.cell],p),postCutoff=instant(b.requestStartedAt)>instant(bank.cutoff),current=allowDecisions&&postCutoff;
 const st=s.wallet.strategies.mean_reversion,used=st.fills.filter(x=>x.action==='buy').reduce((n,x)=>n+amount(x.price)*BigInt(x.quantity),0n);
 const reserved=Object.values(st.pending).filter(x=>x.action==='buy').reduce((n,x)=>n+amount(x.limit)*BigInt(x.quantity),0n),occupied=new Set([...Object.keys(st.positions),...Object.keys(st.pending)]);
 const permitted=current&&independent&&forecast.admitted&&depth&&amount(f.spread)<=amount(p.maximumSpread)&&ask<=SCALE-TICK&&bid>=TICK&&used+reserved+ask<=amount(p.cumulativeBuyBudget)&&(occupied.has(b.instrument)||occupied.size<p.maxOpenMarkets)&&at>=(s.cooldowns[id]||0);
 if(current&&independent)count(s,permitted?'entry_gate_allowed':!forecast.admitted?'unproven_net_edge':'execution_gate_rejected');
 if(!current){const strategies=structuredClone(s.wallet.strategies);s.wallet=observe(s.wallet,b,c);s.wallet.strategies=strategies;}
 else{
  const before=st.intents,wasHeld=Boolean(st.positions[b.instrument]);
  s.wallet=stepDip(s.wallet,b,c,{reference:permitted?Number(ask+amount(forecast.lowerQuartile))/Number(SCALE):null,allowEntry:permitted,minimumEdge:.01-4*Number.EPSILON,takeProfit:'0.010000',horizonMs:p.horizonMs,maxMarketCost:'5.000000',executionPolicy:'responsive_v2'});
  const next=s.wallet.strategies.mean_reversion,intent=next.pending[b.instrument];
  if(intent&&intent.id>before){intent.createdAt=availableAt;intent.reason=intent.action==='buy'?'matured_executable_edge':intent.reason;s.decisions.push({intentId:intent.id,action:intent.action,marketId:id,at:availableAt,sourceSequence:row.sequence,sourceHash:row.hash,version:b.versionKey,features:f,forecast,modelHash:digest(s.cells),limit:intent.limit});if(s.decisions.length>1000)s.decisions.shift();}
  if(wasHeld&&!next.positions[b.instrument])s.cooldowns[id]=at+p.cooldownMs;
  need(next.fills.filter(x=>x.action==='buy').reduce((n,x)=>n+amount(x.price)*BigInt(x.quantity),0n)<=amount(p.cumulativeBuyBudget),'EDGE_BUDGET_EXCEEDED');assertState(s.wallet,c);
 }
 if(independent&&depth&&ask<=SCALE-TICK&&bid>=TICK&&!s.trials[id]&&at>=(s.nextTrialAt[id]||0)){s.trials[id]={cell:f.cell,preparedAt:availableAt,limit:money(ask),version:b.versionKey};s.nextTrialAt[id]=at+p.horizonMs+p.labelLatenessMs;count(s,'trial_prepared');}
 return s;
}
export function executableEdgeReport(s,bank,at){const wallet=structuredClone(s.wallet);for(const b of Object.values(wallet.books))if(instant(at)-instant(b.receivedAt)>bank.config.maxDataAgeMs)invalidate(wallet,b.marketId,at,'stale_report');return {schemaVersion:1,mode:'execution_aware_online_paper',bankHash:s.bankHash,cutoff:bank.cutoff,updatedAt:at,counts:s.counts,development:s.development??null,prospectiveCounts:Object.fromEntries(Object.entries(s.counts).map(([k,v])=>[k,v-(s.development?.counts[k]??0)])),cells:Object.fromEntries(Object.entries(s.cells).map(([k,v])=>[k,edgeForecast(v,bank.policy)])),wallet:report(wallet,bank.config,at).strategies.mean_reversion,recentDecisions:s.decisions.slice(-20),recentLabels:s.labels.slice(-20),networkRequests:0,competitionOrders:0,feesBps:0,profitabilityEstablished:false,limitations:['Empirical cell quartiles are hypotheses, not independent confidence bounds; market/common-event observations may correlate.','Trial labels condition on hypothetical causal next-book fills; missing/unfilled observations are counted separately.','All wallet fills remain local simulations with one adverse tick per side and one percent public depth.']};}
