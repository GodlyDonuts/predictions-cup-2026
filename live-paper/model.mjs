import {amount, money, price, fee, TICK, SCALE} from '../paper/fixed.mjs';
import {createHash} from 'node:crypto';
import {timestamp,instantNs} from '../feed/contract.mjs';

export const digest = value => createHash('sha256').update(JSON.stringify(value)).digest('hex');
export function validateConfig(c) {
  if(c.schemaVersion!==1 || amount(c.initialCash)<=0n)throw Error('Invalid paper configuration');
  for(const k of ['readIntervalMs','cycleIntervalMs','maxDataAgeMs','latencyMs','feeBps','slippageTicks','depthFractionBps','orderShares','maxPositionShares','warmupObservations','lookback','exitAfterMs','port'])
    if(!Number.isSafeInteger(c[k])||c[k]<0)throw Error('Invalid '+k);
  if(c.readIntervalMs<5000||c.cycleIntervalMs<10000||c.maxDataAgeMs<1000||c.orderShares<1||c.maxPositionShares<c.orderShares||c.warmupObservations<2||c.lookback<c.warmupObservations||c.depthFractionBps<1||c.depthFractionBps>1000||c.feeBps>10000||c.slippageTicks>5||c.latencyMs<1||c.port<1024||c.port>65535||c.exitAfterMs<1000)throw Error('Unsafe paper limits');
  if(!Array.isArray(c.marketIds)||c.marketIds.length<1||c.marketIds.length>8||new Set(c.marketIds).size!==c.marketIds.length||c.marketIds.some(x=>!/^\d+$/.test(x)))throw Error('Invalid market scope');
  if(c.tournamentId!=='bda92870-621e-47b0-bc3c-3602c5c26f55'||c.tournamentSlug!=='midterm-elections')throw Error('Invalid tournament scope');
  if(!Number.isFinite(c.entryZ)||c.entryZ<0||amount(c.minEdge)<=0n||amount(c.stopLoss)<=0n||amount(c.maxExposure)<=0n||amount(c.maxExposure)>amount(c.initialCash)||amount(c.cashReserve)<0n||amount(c.cashReserve)>=amount(c.initialCash))throw Error('Invalid strategy risk parameters');
  return c;
}

// Source numbers represent decimal ticks; immediately convert to exact integers.
export function normalizeBook(response, marketId, tournamentId, receivedAt, requestStartedAt, maxAge, excluded=new Set()) {
  const now=Date.parse(receivedAt),started=Date.parse(requestStartedAt);
  if(!Number.isFinite(now)||!Number.isFinite(started)||now<started||now-started>maxAge)throw Error('Slow or invalid snapshot observation');
  const contexts=response.contexts?.filter(x=>x.tournament?.id===tournamentId);
  if(!contexts?.length||contexts.some(x=>x.type!=='tournament'))throw Error('Missing exact tournament book context');
  if(contexts.some(x=>digest(x.orderbook)!==digest(contexts[0].orderbook)))throw Error('Conflicting tournament contexts');
  const context=contexts[0];
  const exchanges=context.orderbook?.exchanges;
  if(!Array.isArray(exchanges)||exchanges.length!==1||exchanges[0].option!=='YES')throw Error('Only verified single YES exchange books supported');
  const e=exchanges[0], id=String(e.exchangeId),version=e.asOf;
  if(!/^\d+$/.test(id)||!version||!Number.isSafeInteger(version.sequence)||version.sequence<0||!Number.isFinite(Date.parse(version.at))||Date.parse(version.at)>now+2000||now-Date.parse(version.at)>maxAge)throw Error('Missing/stale engine version');
  if(excluded.has(id))throw Error('Exchange has real account holdings or open orders');
  const levels=(rows,descending)=>{
    if(!Array.isArray(rows))throw Error('Missing depth');
    const seen=new Set();
    const out=rows.map(l=>{const p=price(String(l.price));if(!Number.isSafeInteger(l.quantity)||l.quantity<=0||seen.has(String(p)))throw Error('Invalid/duplicate depth');seen.add(String(p));return{price:money(p),quantity:l.quantity};});
    return out.sort((a,b)=>descending?Number(amount(b.price)-amount(a.price)):Number(amount(a.price)-amount(b.price)));
  };
  const bids=levels(e.bids,true),asks=levels(e.asks,false);
  if(!bids.length||!asks.length||amount(bids[0].price)>=amount(asks[0].price))throw Error('Missing, locked or crossed market');
  const sourceAt=timestamp(version.at);
  return {marketId:String(marketId),exchangeId:id,instrument:id+':YES',receivedAt,requestStartedAt,sourceAt,sequence:String(version.sequence),versionKey:String(version.sequence)+':'+sourceAt,bids,asks};
}

export function invalidate(s,marketId,at,reason){s.invalidated??={};for(const[id,b]of Object.entries(s.books))if(marketId===null||b.marketId===String(marketId))s.invalidated[id]={at,reason};}

export function roundTripStats(fills){
  const positions={};let closed=0,winners=0;
  for(const f of fills){const p=positions[f.instrument]??={quantity:0,lots:[],pnl:0n};const n=amount(f.price)*BigInt(f.quantity),charge=amount(f.fee);
    if(f.action==='buy'){p.quantity+=f.quantity;p.lots.push({quantity:f.quantity,cost:n+charge});}
    else{let left=f.quantity,basis=0n;while(left){const l=p.lots[0];if(!l)throw Error('Round-trip ledger oversell');const q=Math.min(left,l.quantity),used=q===l.quantity?l.cost:l.cost*BigInt(q)/BigInt(l.quantity);basis+=used;l.cost-=used;l.quantity-=q;left-=q;if(!l.quantity)p.lots.shift();}p.quantity-=f.quantity;p.pnl+=n-charge-basis;if(!p.quantity){closed++;if(p.pnl>0n)winners++;p.pnl=0n;}}
  }
  return {closedTrades:closed,winningTrades:winners,winRate:closed?winners/closed:null};
}

export function initialState(c,runId,at) {
  validateConfig(c);
  return {schemaVersion:1,engineRevision:3,runId,startedAt:at,configHash:digest(c),lastAt:at,observations:0,skips:{},books:{},histories:{},strategies:Object.fromEntries(['mean_reversion','buy_hold_benchmark'].map(name=>[name,{cash:c.initialCash,realized:'0.000000',fees:'0.000000',turnover:'0.000000',positions:{},pending:{},fills:[],intents:0,expiredIntents:0,consumedVersions:{},equityCurve:[],peak:c.initialCash,maxDrawdown:'0.000000',closedTrades:0,winningTrades:0}]))};
}

export function restoreState(input,c){
  const s=structuredClone(input);
  if(s.engineRevision===undefined){
    // First local prototype recorded buys only. Upgrade its exact entry lots without
    // changing cash, fees, realized returns or previously captured observations.
    for(const st of Object.values(s.strategies)){
      if(st.fills.some(f=>f.action==='sell'))throw Error('Legacy sold inventory needs explicit migration');
      for(const [id,p]of Object.entries(st.positions))p.lots=st.fills.filter(f=>f.instrument===id&&f.action==='buy').map(f=>({quantity:f.quantity,cost:money(amount(f.price)*BigInt(f.quantity)+amount(f.fee))}));
    }
    s.engineRevision=2;
  }
  if(s.engineRevision===2){for(const st of Object.values(s.strategies))for(const[id,key]of Object.entries(st.consumedVersions)){const sep=key.indexOf(':');st.consumedVersions[id]=key.slice(0,sep)+':'+timestamp(key.slice(sep+1));}s.engineRevision=3;s.evidenceValidation='legacy_run_requires_corrected_replay_and_independent_review';}
  if(s.engineRevision!==3)throw Error('Unknown engine revision');assertState(s,c);return s;
}

export function assertState(s,c) {
  if(s.schemaVersion!==1||s.configHash!==digest(c))throw Error('Resume requires identical config/schema');
  for(const st of Object.values(s.strategies)){
    let signed=0n,fees=0n,turnover=0n,quantities={},lots={},realized=0n;
    for(const f of st.fills){if(!['buy','sell'].includes(f.action)||!Number.isSafeInteger(f.quantity)||f.quantity<1)throw Error('Invalid durable fill');const notional=amount(f.price)*BigInt(f.quantity);signed+=(f.action==='buy'?-notional:notional)-amount(f.fee);fees+=amount(f.fee);turnover+=notional;quantities[f.instrument]=(quantities[f.instrument]||0)+(f.action==='buy'?f.quantity:-f.quantity);
      lots[f.instrument]??=[];if(f.action==='buy')lots[f.instrument].push({quantity:f.quantity,cost:notional+amount(f.fee)});else{let left=f.quantity,basis=0n;while(left){const lot=lots[f.instrument][0];if(!lot)throw Error('Fill ledger oversell');const q=Math.min(left,lot.quantity),used=q===lot.quantity?lot.cost:lot.cost*BigInt(q)/BigInt(lot.quantity);basis+=used;lot.cost-=used;lot.quantity-=q;left-=q;if(!lot.quantity)lots[f.instrument].shift();}realized+=notional-amount(f.fee)-basis;}}
    if(amount(st.cash)!==amount(c.initialCash)+signed||amount(st.fees)!==fees||amount(st.turnover)!==turnover||amount(st.cash)<0n)throw Error('Paper cash/fee conservation failure');
    for(const [id,q]of Object.entries(quantities))if(q<0||q!==(st.positions[id]?.quantity||0))throw Error('Inventory conservation failure');
    if(realized!==amount(st.realized))throw Error('Realized basis conservation failure');
    for(const [id,p]of Object.entries(st.positions))if(!Number.isSafeInteger(p.quantity)||p.quantity<1||p.quantity>c.maxPositionShares||p.quantity!==(quantities[id]||0)||amount(p.cost)!==lots[id].reduce((n,l)=>n+l.cost,0n)||JSON.stringify(p.lots)!==JSON.stringify(lots[id].map(l=>({quantity:l.quantity,cost:money(l.cost)}))))throw Error('Position invariant failure');
  }
}

function costs(c){return{feeBps:c.feeBps,feePerShare:'0.000000'};}
function fillIntent(st,intent,b,c,at) {
  if(Date.parse(at)-Date.parse(intent.createdAt)<c.latencyMs||b.receivedAt===intent.createdAt)return;
  if(st.consumedVersions[b.instrument]===b.versionKey)return;
  const rows=intent.action==='buy'?b.asks:b.bids;
  let remaining=intent.quantity, cash=amount(st.cash),basis=0n,net=0n,totalQty=0;
  let exposure=Object.values(st.positions).reduce((n,p)=>n+amount(p.cost),0n);
  for(const l of rows){
    if(!remaining)break;
    const p=amount(l.price)+(intent.action==='buy'?1n:-1n)*TICK*BigInt(c.slippageTicks);
    if(p<TICK||p>SCALE-TICK||(intent.action==='buy'?p>amount(intent.limit):p<amount(intent.limit)))break;
    let q=Math.min(remaining,Math.floor(l.quantity*c.depthFractionBps/10000));
    if(intent.action==='buy'){
      q=Math.min(q,c.maxPositionShares-(st.positions[b.instrument]?.quantity||0));
      while(q>0){const cost=p*BigInt(q)+fee(p*BigInt(q),q,costs(c));if(cash-cost>=amount(c.cashReserve)&&exposure+cost<=amount(c.maxExposure))break;q--;}
    }else q=Math.min(q,st.positions[b.instrument]?.quantity||0);
    if(q<1)continue;
    const notional=p*BigInt(q),charge=fee(notional,q,costs(c));
    if(intent.action==='buy'){
      cash-=notional+charge;exposure+=notional+charge;
      const old=st.positions[b.instrument];st.positions[b.instrument]={quantity:(old?.quantity||0)+q,cost:money(amount(old?.cost||'0')+notional+charge),enteredAt:old?.enteredAt||at,lots:[...(old?.lots||[]),{quantity:q,cost:money(notional+charge)}]};
    }else{
      const pos=st.positions[b.instrument];let left=q,used=0n;
      while(left){const lot=pos.lots[0],n=Math.min(left,lot.quantity),part=n===lot.quantity?amount(lot.cost):amount(lot.cost)*BigInt(n)/BigInt(lot.quantity);used+=part;lot.cost=money(amount(lot.cost)-part);lot.quantity-=n;left-=n;if(!lot.quantity)pos.lots.shift();}
      basis+=used;net+=notional-charge;pos.cost=money(amount(pos.cost)-used);pos.quantity-=q;if(!pos.quantity)delete st.positions[b.instrument];
      cash+=notional-charge;st.realized=money(amount(st.realized)+notional-charge-used);
    }
    st.fees=money(amount(st.fees)+charge);st.turnover=money(amount(st.turnover)+notional);st.cash=money(cash);
    st.fills.push({intentId:intent.id,instrument:b.instrument,action:intent.action,quantity:q,price:money(p),fee:money(charge),at,sourceAt:b.sourceAt,sourceVersion:b.versionKey,model:'hypothetical_next_snapshot_taker',depthFractionBps:c.depthFractionBps,slippageTicks:c.slippageTicks});
    totalQty+=q;remaining-=q;
  }
  if(totalQty){st.consumedVersions[b.instrument]=b.versionKey;delete st.pending[b.instrument];if(intent.action==='sell'){st.closedTrades++;if(net>basis)st.winningTrades++;}}
}

function enqueue(st,b,action,quantity,limit,at,reason){st.intents++;st.pending[b.instrument]={id:st.intents,createdAt:at,action,quantity,limit:money(limit),reason};}

export function observe(input,b,c) {
  const s=structuredClone(input),at=b.receivedAt,id=b.instrument;
  if(Date.parse(at)<Date.parse(s.lastAt))throw Error('Observation time moved backward');
  const prev=s.books[id];
  const sameVersion=prev&&BigInt(b.sequence)===BigInt(prev.sequence)&&instantNs(b.sourceAt)===instantNs(prev.sourceAt);
  if(prev&&(BigInt(b.sequence)<BigInt(prev.sequence)||instantNs(b.sourceAt)<instantNs(prev.sourceAt)))throw Error('Book version rollback');
  if(sameVersion&&digest({bids:b.bids,asks:b.asks})!==digest({bids:prev.bids,asks:prev.asks}))throw Error('Conflicting equal book version');
  b={...b,sourceAt:timestamp(b.sourceAt),versionKey:b.sequence+':'+timestamp(b.sourceAt)};
  if(s.invalidated?.[id]&&Date.parse(b.requestStartedAt)<=Date.parse(s.invalidated[id].at))throw Error('Pre-invalidation snapshot cannot recover');
  if(s.invalidated)delete s.invalidated[id];
  s.observations++;s.lastAt=at;s.books[id]=b;
  if(sameVersion){assertState(s,c);return s;}
  const mid=(Number(amount(b.bids[0].price))+Number(amount(b.asks[0].price)))/2;
  const hist=s.histories[id]||[];const recent=hist.slice(-c.lookback);const mean=recent.length?recent.reduce((n,x)=>n+x,0)/recent.length:mid;
  const sd=recent.length?Math.sqrt(recent.reduce((n,x)=>n+(x-mean)**2,0)/recent.length):0;
  s.signals??={};s.signals[id]={marketId:b.marketId,at,priorObservations:recent.length,warmupRemaining:Math.max(0,c.warmupObservations-recent.length),midpoint:money(BigInt(Math.round(mid))),laggedMean:money(BigInt(Math.round(mean))),standardDeviation:money(BigInt(Math.round(sd))),zScore:sd?(mean-mid)/sd:null,estimatedEntryEdge:money(BigInt(Math.round(mean))-amount(b.asks[0].price)-TICK*BigInt(c.slippageTicks)-fee(amount(b.asks[0].price)+TICK*BigInt(c.slippageTicks),1,costs(c)))};
  for(const [name,st]of Object.entries(s.strategies)){
    const pending=st.pending[id];
    if(pending){if(Date.parse(at)-Date.parse(pending.createdAt)>c.maxDataAgeMs+c.cycleIntervalMs*4){delete st.pending[id];st.expiredIntents++;}else fillIntent(st,pending,b,c,at);}
    if(st.pending[id])continue;
    const pos=st.positions[id];
    if(name==='buy_hold_benchmark'){
      if(!pos&&!st.fills.some(f=>f.instrument===id))enqueue(st,b,'buy',c.orderShares,amount(b.asks[0].price)+TICK*BigInt(c.slippageTicks),at,'small_buy_hold_control');
      continue;
    }
    if(recent.length<c.warmupObservations)continue;
    if(pos){
      const avg=amount(pos.cost)/BigInt(pos.quantity),bid=amount(b.bids[0].price);
      if(Number(bid)>=mean||avg-bid>=amount(c.stopLoss)||Date.parse(at)-Date.parse(pos.enteredAt)>=c.exitAfterMs)
        enqueue(st,b,'sell',pos.quantity,bid-TICK*BigInt(c.slippageTicks),at,'mean_reversion_exit_stop_or_time');
    }else{
      const ask=amount(b.asks[0].price)+TICK*BigInt(c.slippageTicks),edge=mean-Number(ask)-Number(fee(ask,1,costs(c)));
      if(sd>0&&(mean-mid)/sd>=c.entryZ&&edge>=Number(amount(c.minEdge)))enqueue(st,b,'buy',c.orderShares,ask,at,'lagged_rolling_mean_zscore');
    }
  }
  s.histories[id]=[...hist,mid].slice(-c.lookback);assertState(s,c);return s;
}

export function report(s,c,now=new Date().toISOString()) {
  const age=id=>Date.parse(now)-Date.parse(s.books[id]?.receivedAt||'');
  const strategies={};
  for(const [name,st]of Object.entries(s.strategies)){
    let reference=amount(st.cash),liquidation=amount(st.cash),referenceComplete=true,liquidationComplete=true;const positions=[];
    for(const [id,pos]of Object.entries(st.positions)){
      const b=s.books[id];let proceeds=0n,left=pos.quantity;const fresh=b&&!s.invalidated?.[id]&&age(id)>=0&&age(id)<=c.maxDataAgeMs+c.cycleIntervalMs*4;
      if(fresh){reference+=(amount(b.bids[0].price)+amount(b.asks[0].price))*BigInt(pos.quantity)/2n;for(const l of b.bids){const q=Math.min(left,Math.floor(l.quantity*c.depthFractionBps/10000));if(q){const p=amount(l.price)-TICK*BigInt(c.slippageTicks);if(p<TICK)break;const n=p*BigInt(q);proceeds+=n-fee(n,q,costs(c));left-=q;}if(!left)break;}}
      if(!fresh)referenceComplete=false;if(!fresh||left)liquidationComplete=false;liquidation+=proceeds;
      const labels={'377':'Alaska Senate','354':'Florida Senate','277':'North Carolina Senate','305':'Michigan Governor'};
      positions.push({instrument:id,label:(labels[b?.marketId]||'Market '+b?.marketId)+' Democratic YES',...pos,observationAgeMs:fresh?age(id):null,estimatedProceeds:money(proceeds),unsoldShares:left});
    }
    const equity=referenceComplete?reference:null,liq=liquidationComplete?liquidation:null;
    const curve=st.equityCurve;let peak=amount(st.peak),dd=amount(st.maxDrawdown);
    if(equity!==null){if(equity>peak)peak=equity;if(peak-equity>dd)dd=peak-equity;}
    strategies[name]={cash:st.cash,realizedPnl:st.realized,referenceEquity:equity===null?null:money(equity),referencePnl:equity===null?null:money(equity-amount(c.initialCash)),estimatedLiquidationEquity:liq===null?null:money(liq),liquidationPnl:liq===null?null:money(liq-amount(c.initialCash)),fees:st.fees,turnover:st.turnover,filledShares:st.fills.reduce((n,f)=>n+f.quantity,0),fills:st.fills.length,intents:st.intents,pending:Object.values(st.pending),expiredIntents:st.expiredIntents,...roundTripStats(st.fills),sellExecutions:st.closedTrades,maxObservedDrawdown:money(dd),positions,equityCurve:curve.slice(-500),recentFills:st.fills.slice(-25)};
  }
  return {schemaVersion:1,evidenceValidation:s.evidenceValidation||'pending_independent_review',engineRevision:s.engineRevision,mode:'live_data_paper_only',runId:s.runId,startedAt:s.startedAt,lastObservationAt:s.lastAt,reportedAt:now,initialCash:c.initialCash,configHash:s.configHash,observations:s.observations,coveredExchanges:Object.keys(s.books).length,scope:c.marketIds,signals:s.signals||{},skips:s.skips,strategies,noTradeBaseline:{cash:c.initialCash,pnl:'0.000000'},ordersSentToCompetition:0,assumptions:{fillModel:'Hypothetical taker at subsequent REST snapshot; NOT a verified actual fill',feeBps:c.feeBps,feeStatus:'unverified_scenario',slippageTicks:c.slippageTicks,depthFractionBps:c.depthFractionBps,source:'official API v1, fixed tournament',passiveFillEvidence:false,completeTradeTape:false,strategyProfitabilityEstablished:false,mark:'midpoint of fresh bid/ask; not tournament valuation'},config:c};
}

export function sampleEquity(s,c,now){
  const r=report(s,c,now);
  for(const [name,v]of Object.entries(r.strategies)){const st=s.strategies[name];if(v.referenceEquity!==null){const eq=amount(v.referenceEquity);if(eq>amount(st.peak))st.peak=v.referenceEquity;const dd=amount(st.peak)-eq;if(dd>amount(st.maxDrawdown))st.maxDrawdown=money(dd);st.equityCurve.push({at:now,referenceEquity:v.referenceEquity,liquidationEquity:v.estimatedLiquidationEquity});if(st.equityCurve.length>5000)st.equityCurve.shift();}}
}
