import {amount,money,TICK,instant}from'../paper/fixed.mjs';
import {digest,initialState,invalidate,stepDip,report,validateConfig}from'./model.mjs';
import {validateResearch}from'./probability-paper.mjs';
export const PROBABILITY_RECEIPT_POLICY=Object.freeze({minimumLowerEdge:'0.020000',maximumSpread:'0.020000',maximumResearchAgeMs:21600000,maximumAccountAgeMs:20000,maxHoldMs:172800000,takeProfit:'0.020000',maxMarketCost:'1.000000',maxOpenMarkets:3,cumulativeBuyBudget:'100.000000',latenciesMs:[1000,3000]});
const need=(v,c)=>{if(!v)throw Error('PROBABILITY_RECEIPT_'+c);},id=x=>typeof x==='string'&&/^[1-9]\d*$/.test(x)&&Number.isSafeInteger(Number(x));
export function validateProbabilityReceiptBank(bank){
 need(bank.schemaVersion===2&&bank.mode==='source_probability_actual_receipt_paper'&&digest(bank.policy)===digest(PROBABILITY_RECEIPT_POLICY),'BANK');validateConfig(bank.config);
 need(bank.config.feeBps===0&&bank.config.depthFractionBps===100&&bank.config.slippageTicks===1&&bank.config.orderShares===1&&bank.config.maxPositionShares===1&&bank.config.initialCash==='100.000000'&&bank.config.cashReserve==='90.000000'&&bank.config.maxExposure==='10.000000'&&bank.config.stopLoss==='0.050000'&&bank.config.strategyUniverse==='diversified-dip-v1'&&bank.config.maxDataAgeMs===20000&&bank.config.marketIds.length===bank.research.length,'FILL_ASSUMPTIONS');
 need(Array.isArray(bank.research)&&bank.research.length>0&&bank.research.length<=3&&Object.keys(bank.identities).length===bank.research.length,'RESEARCH_SET');const seen=new Set();
 for(const r of bank.research){validateResearch(r);const spec=bank.identities[r.marketId];need(!seen.has(r.marketId)&&instant(r.recordedAt)<=instant(bank.cutoff)&&spec?.exchangeId===r.exchangeId&&spec.predicateHash===r.predicateHash&&bank.universe.exchanges.some(x=>x.marketId===r.marketId&&x.exchangeId===r.exchangeId&&x.option==='YES')&&bank.config.marketIds.includes(r.marketId),'RESEARCH_BINDING');seen.add(r.marketId);}
 return bank;
}
// Derive only the fields needed to reject our own account's liquidity. Raw
// account financial values and order details never enter a paper checkpoint.
export function probabilityAccountEligibility(cache){
 try{need(!cache.errors?.positions&&!cache.errors?.orders&&Array.isArray(cache.positions?.rows)&&Array.isArray(cache.orders?.rows),'ACCOUNT_UNAVAILABLE');instant(cache.positions.at);instant(cache.orders.at);const excluded=new Set();
  for(const p of cache.positions.rows){need(id(p.exchangeId)&&Number.isSafeInteger(p.quantity)&&p.quantity>0,'ACCOUNT_POSITION');excluded.add(p.exchangeId);}
  for(const o of cache.orders.rows){need(id(o.exchangeId)&&o.open===true&&Number.isFinite(o.quantity)&&o.quantity>0,'ACCOUNT_ORDER');excluded.add(o.exchangeId);}
  return{schemaVersion:1,positionsAt:cache.positions.at,ordersAt:cache.orders.at,excludedExchangeIds:[...excluded].sort((a,b)=>Number(a)-Number(b))};
 }catch{return null;}
}
export function probabilityAccountGate(account,b,availableAt,policy=PROBABILITY_RECEIPT_POLICY){
 try{if(!account||account.schemaVersion!==1||Object.keys(account).sort().join('|')!=='excludedExchangeIds|ordersAt|positionsAt|schemaVersion'||!Array.isArray(account.excludedExchangeIds)||account.excludedExchangeIds.some(x=>!id(x)))return'account_unavailable';
  const now=instant(availableAt),request=instant(b.requestStartedAt);
  if([account.positionsAt,account.ordersAt].some(t=>instant(t)>request||now-instant(t)>policy.maximumAccountAgeMs))return'account_not_causal_or_fresh';
  return account.excludedExchangeIds.includes(b.exchangeId)?'own_account_exposure':null;
 }catch{return'account_unavailable';}
}
export function receiptProbabilityGate(r,b,bank,availableAt){
 const identity=bank.identities[b.marketId],p=bank.policy,now=instant(availableAt);
 if(!r)return{accepted:false,reason:'missing_research'};
 validateResearch(r);
 if(r.marketId!==b.marketId||r.exchangeId!==b.exchangeId||identity?.exchangeId!==b.exchangeId||identity.predicateHash!==r.predicateHash)return{accepted:false,reason:'contract_mismatch'};
 if(!r.settlementReviewed||r.flags.length)return{accepted:false,reason:'unresolved_evidence'};
 if(instant(r.recordedAt)>instant(b.requestStartedAt)||instant(r.sourceAt)>instant(b.requestStartedAt))return{accepted:false,reason:'research_not_yet_known'};
 if(now>=instant(r.expiresAt)||now-instant(r.sourceAt)>p.maximumResearchAgeMs)return{accepted:false,reason:'expired_research'};
 if(b.freshnessPolicy!=='authoritative_receipt_v1'||now-instant(b.receivedAt)>bank.config.maxDataAgeMs||instant(b.receivedAt)-instant(b.requestStartedAt)>bank.config.maxDataAgeMs)return{accepted:false,reason:'stale_receipt'};
 if(amount(b.asks[0].price)-amount(b.bids[0].price)>amount(p.maximumSpread))return{accepted:false,reason:'spread'};
 const entry=amount(b.asks[0].price)+TICK,edge=amount(r.lower)-entry;
 return{accepted:edge>=amount(p.minimumLowerEdge),reason:edge>=amount(p.minimumLowerEdge)?'lower_bound_edge':'insufficient_lower_edge',entry:money(entry),edge:money(edge),researchHash:digest(r)};
}
export function createProbabilityReceiptState(bank){validateProbabilityReceiptBank(bank);return{bankHash:digest(bank),lastAvailableAt:bank.cutoff,barriers:{global:bank.cutoff,markets:{}},counts:{},decisions:[],wallets:Object.fromEntries(bank.policy.latenciesMs.map(latency=>[latency,{state:initialState({...bank.config,latencyMs:latency},bank.name+':'+latency,bank.cutoff),cooldowns:{}}]))};}
function fence(s,id,at,reason){if(id===null)s.barriers.global=at;else s.barriers.markets[id]=at;
 for(const w of Object.values(s.wallets)){invalidate(w.state,id,at,reason);const st=w.state.strategies.mean_reversion;for(const[key,intent]of Object.entries(st.pending))if(intent.action==='buy'&&(id===null||w.state.books[key]?.marketId===id)){delete st.pending[key];st.expiredIntents++;}}
}
export function advanceReceiptProbability(s,row,bank,availableAt,account){
 need(s.bankHash===digest(bank)&&instant(row.at)<=instant(availableAt)&&instant(availableAt)>=instant(s.lastAvailableAt),'CAUSAL_STATE');s.lastAvailableAt=availableAt;const inc=k=>s.counts[k]=(s.counts[k]||0)+1;
 if(['session_start','session_stop','invalidation'].includes(row.kind)){fence(s,row.kind==='invalidation'?row.payload.marketId:null,availableAt,'probability_source_boundary');inc('source_boundary');return s;}
 need(row.kind==='book','ROW_KIND');const b=row.payload.book;
 if(!bank.identities[b.marketId])return s;
 if(instant(availableAt)-instant(b.receivedAt)>bank.config.maxDataAgeMs){fence(s,b.marketId,availableAt,'stale_consumer_book');inc('stale_consumer_book');return s;}
 if([s.barriers.global,s.barriers.markets[b.marketId],bank.cutoff].some(at=>at&&instant(b.requestStartedAt)<=instant(at))){inc('request_before_consumer_barrier');return s;}
 const accountReason=probabilityAccountGate(account,b,availableAt);if(accountReason){fence(s,b.marketId,availableAt,accountReason);inc(accountReason);return s;}
 const r=bank.research.find(x=>x.marketId===b.marketId),gate=receiptProbabilityGate(r,b,bank,availableAt);inc(gate.reason);
 s.decisions.push({at:availableAt,marketId:b.marketId,sourceSequence:row.sequence,sourceHash:row.hash,requestStartedAt:b.requestStartedAt,receivedAt:b.receivedAt,...gate});if(s.decisions.length>1000)s.decisions.shift();
 for(const[latency,w]of Object.entries(s.wallets)){const c={...bank.config,latencyMs:Number(latency)},st=w.state.strategies.mean_reversion,
  spent=st.fills.filter(x=>x.action==='buy').reduce((n,f)=>n+amount(f.price)*BigInt(f.quantity),0n),reserved=Object.values(st.pending).filter(x=>x.action==='buy').reduce((n,i)=>n+amount(i.limit)*BigInt(i.quantity),0n),occupied=new Set([...Object.keys(st.positions),...Object.keys(st.pending)]),old=st.pending[b.instrument];
  const allow=gate.accepted&&spent+reserved+(old?.action==='buy'?0n:amount(b.asks[0].price)+TICK)<=amount(bank.policy.cumulativeBuyBudget)&&(occupied.has(b.instrument)||occupied.size<bank.policy.maxOpenMarkets)&&instant(availableAt)>=(w.cooldowns[b.marketId]||0);
  w.state=stepDip(w.state,b,c,{reference:gate.accepted?Number(amount(r.lower))/1e6:null,allowEntry:allow,minimumEdge:Number(amount(bank.policy.minimumLowerEdge))/1e6-4*Number.EPSILON,takeProfit:bank.policy.takeProfit,horizonMs:bank.policy.maxHoldMs,maxMarketCost:bank.policy.maxMarketCost,executionPolicy:'responsive_v2'});
  const next=w.state.strategies.mean_reversion,pending=next.pending[b.instrument];if(pending&&(!old||old.id!==pending.id||old.createdAt!==pending.createdAt)){pending.createdAt=availableAt;pending.actualAvailableAt=availableAt;pending.probabilityResearchHash=digest(r);if(pending.action==='buy')pending.reason='source_probability_lower_bound';}
  if(st.positions[b.instrument]&&!next.positions[b.instrument])w.cooldowns[b.marketId]=instant(availableAt)+900000;
  need(next.fills.filter(x=>x.action==='buy').reduce((n,f)=>n+amount(f.price)*BigInt(f.quantity),0n)<=amount(bank.policy.cumulativeBuyBudget),'CUMULATIVE_BUDGET');
 }
 return s;
}
export function receiptProbabilityReport(s,bank,at){return{mode:bank.mode,updatedAt:at,bankHash:s.bankHash,cutoff:bank.cutoff,counts:s.counts,decisions:s.decisions,actualConsumer:{lastAvailableAt:s.lastAvailableAt,barriers:s.barriers},wallets:Object.fromEntries(Object.entries(s.wallets).map(([latency,w])=>{const r=report(w.state,{...bank.config,latencyMs:Number(latency)},at).strategies.mean_reversion;return[latency,{...r,positions:r.positions.map(x=>({...x,label:'Market '+bank.universe.exchanges.find(y=>y.exchangeId+':YES'===x.instrument).marketId+' · YES'})),pending:Object.entries(w.state.strategies.mean_reversion.pending).map(([instrument,x])=>({...x,instrument,marketId:bank.universe.exchanges.find(y=>y.exchangeId+':YES'===instrument).marketId}))}];})),networkRequests:0,competitionOrders:0,profitabilityEstablished:false,limitations:['Forecast bands and settlement review are supplied research attestations, not authenticated or calibrated probability guarantees.','Settlement probability minus purchase cost does not forecast a profitable sale within the two-day holding window.','Counterfactual later-request fills use one-percent depth, one adverse tick and zero fees; latencies share correlated opportunities.','Entry and exit depth require causal, fresh account exclusions; missing or own-account exposure prevents simulated fills.']};}
