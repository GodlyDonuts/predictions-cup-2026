import {amount,money,TICK,instant}from'../paper/fixed.mjs';
import {digest,initialState,invalidate,stepDip,report,validateConfig,assertState}from'./model.mjs';
import {validateResearch}from'./probability-paper.mjs';
import {validateProbabilityReceiptBank as validatePredecessor} from './probability-receipt.mjs';
export const PROBABILITY_RECEIPT_POLICY=Object.freeze({accountPolicy:'position_entry_block_open_order_fill_block_v2',minimumLowerEdge:'0.020000',maximumSpread:'0.020000',maximumResearchAgeMs:21600000,maximumAccountAgeMs:20000,maxHoldMs:172800000,takeProfit:'0.020000',maxMarketCost:'1.000000',maxOpenMarkets:3,cumulativeBuyBudget:'100.000000',latenciesMs:[1000,3000]});
const need=(v,c)=>{if(!v)throw Error('PROBABILITY_RECEIPT_'+c);},id=x=>typeof x==='string'&&/^[1-9]\d*$/.test(x)&&Number.isSafeInteger(Number(x));
export function validateProbabilityReceiptBank(bank){
 need(bank.schemaVersion===3&&bank.mode==='source_probability_owned_risk_continuation_v2'&&digest(bank.policy)===digest(PROBABILITY_RECEIPT_POLICY),'BANK');validateConfig(bank.config);
 need(bank.config.feeBps===0&&bank.config.depthFractionBps===100&&bank.config.slippageTicks===1&&bank.config.orderShares===1&&bank.config.maxPositionShares===1&&bank.config.initialCash==='100.000000'&&bank.config.cashReserve==='90.000000'&&bank.config.maxExposure==='10.000000'&&bank.config.stopLoss==='0.050000'&&bank.config.strategyUniverse==='diversified-dip-v1'&&bank.config.maxDataAgeMs===20000&&bank.config.marketIds.length===bank.research.length,'FILL_ASSUMPTIONS');
 need(Array.isArray(bank.research)&&bank.research.length>0&&bank.research.length<=3&&Object.keys(bank.identities).length===bank.research.length,'RESEARCH_SET');const seen=new Set();
 for(const r of bank.research){validateResearch(r);const spec=bank.identities[r.marketId];need(!seen.has(r.marketId)&&instant(r.recordedAt)<=instant(bank.cutoff)&&spec?.exchangeId===r.exchangeId&&spec.predicateHash===r.predicateHash&&bank.universe.exchanges.some(x=>x.marketId===r.marketId&&x.exchangeId===r.exchangeId&&x.option==='YES')&&bank.config.marketIds.includes(r.marketId),'RESEARCH_BINDING');seen.add(r.marketId);}
 const continuation=bank.accountingContinuation;
 need(continuation?.accountingCarried===true&&continuation.executionAuthority===false&&['openingFileHash','priorBankHash','priorBankFileHash','priorStateHash','priorJournalHash','priorJournalFileHash'].every(k=>/^[a-f0-9]{64}$/.test(continuation[k]))&&Number.isSafeInteger(continuation.priorJournalBytes)&&continuation.priorJournalBytes>0&&Number.isSafeInteger(bank.sourcePrefixBytes)&&bank.sourcePrefixBytes>0&&Number.isSafeInteger(bank.sourcePrefixSequence)&&bank.sourcePrefixSequence>0&&/^[a-f0-9]{64}$/.test(bank.sourcePrefixHash)&&/^[a-f0-9]{64}$/.test(bank.sourcePrefixJournalHash),'CONTINUATION');
 return bank;
}
// Store identifiers and causal observation times only. Positions block new
// exposure; resting orders also block counterfactual fills against our liquidity.
export function probabilityAccountEligibility(cache){
 try{need(!cache.errors?.positions&&!cache.errors?.orders&&Array.isArray(cache.positions?.rows)&&Array.isArray(cache.orders?.rows),'ACCOUNT_UNAVAILABLE');instant(cache.positions.at);instant(cache.orders.at);
  const positions=new Set(),orders=new Set();
  for(const p of cache.positions.rows){need(id(p.exchangeId)&&Number.isSafeInteger(p.quantity)&&p.quantity>0,'ACCOUNT_POSITION');positions.add(p.exchangeId);}
  for(const o of cache.orders.rows){need(id(o.exchangeId)&&o.open===true&&Number.isFinite(o.quantity)&&o.quantity>0,'ACCOUNT_ORDER');orders.add(o.exchangeId);}
  const sorted=s=>[...s].sort((a,b)=>Number(a)-Number(b));
  return{schemaVersion:2,positionsAt:cache.positions.at,ordersAt:cache.orders.at,positionExchangeIds:sorted(positions),openOrderExchangeIds:sorted(orders)};
 }catch{return null;}
}
export function probabilityAccountGate(account,b,availableAt,policy=PROBABILITY_RECEIPT_POLICY,phase='entry'){
 try{if(!['entry','reduce_only'].includes(phase)||!account||account.schemaVersion!==2||Object.keys(account).sort().join('|')!=='openOrderExchangeIds|ordersAt|positionExchangeIds|positionsAt|schemaVersion'||['positionExchangeIds','openOrderExchangeIds'].some(k=>!Array.isArray(account[k])||account[k].some(x=>!id(x))||new Set(account[k]).size!==account[k].length))return'account_unavailable';
  const now=instant(availableAt),request=instant(b.requestStartedAt);
  if([account.positionsAt,account.ordersAt].some(t=>instant(t)>request||now-instant(t)>policy.maximumAccountAgeMs))return'account_not_causal_or_fresh';
  if(account.openOrderExchangeIds.includes(b.exchangeId))return'own_account_open_order';
  return phase==='entry'&&account.positionExchangeIds.includes(b.exchangeId)?'own_account_exposure':null;
 }catch{return'account_unavailable';}
}
export function validateProbabilityOpening(opening,bank){
 need(opening?.schemaVersion===1&&digest(opening.priorBank)===bank.accountingContinuation.priorBankHash&&opening.priorBankFileHash===bank.accountingContinuation.priorBankFileHash&&opening.priorBank.name===bank.accountingContinuation.priorCohort&&opening.state?.bankHash===bank.accountingContinuation.priorBankHash&&digest(opening.state)===bank.accountingContinuation.priorStateHash,'OPENING_IDENTITY');validatePredecessor(opening.priorBank);
 need(opening.sourceBytes===bank.sourcePrefixBytes&&opening.sourceSequence===bank.sourcePrefixSequence&&opening.sourceHash===bank.sourcePrefixJournalHash&&opening.sourcePrefixHash===bank.sourcePrefixHash&&opening.journalHash===bank.accountingContinuation.priorJournalHash&&opening.journalFileHash===bank.accountingContinuation.priorJournalFileHash&&opening.journalBytes===bank.accountingContinuation.priorJournalBytes,'OPENING_FRONTIER');
 need(digest(opening.priorBank.config)===digest(bank.config)&&digest(opening.priorBank.research)===digest(bank.research)&&digest(opening.priorBank.identities)===digest(bank.identities)&&digest(opening.priorBank.universe)===digest(bank.universe)&&instant(opening.state.lastAvailableAt)<=instant(bank.cutoff),'OPENING_SCOPE');
 need(Object.keys(opening.state.wallets).sort().join('|')==='1000|3000','OPENING_WALLETS');
 for(const[latency,w]of Object.entries(opening.state.wallets))assertState(w.state,{...bank.config,latencyMs:Number(latency)});
 return opening;
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
export function createProbabilityReceiptState(bank,opening){
 validateProbabilityReceiptBank(bank);validateProbabilityOpening(opening,bank);
 const state=structuredClone(opening.state);state.bankHash=digest(bank);state.lastAvailableAt=bank.cutoff;state.barriers={global:bank.cutoff,markets:{}};
 for(const w of Object.values(state.wallets)){invalidate(w.state,null,bank.cutoff,'owned_risk_continuation_boundary');for(const st of Object.values(w.state.strategies)){st.expiredIntents+=Object.keys(st.pending).length;st.pending={};}}
 return state;
}
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
 const accountReason=probabilityAccountGate(account,b,availableAt,bank.policy,'reduce_only');if(accountReason){fence(s,b.marketId,availableAt,accountReason);inc(accountReason);return s;}
 const r=bank.research.find(x=>x.marketId===b.marketId),researchGate=receiptProbabilityGate(r,b,bank,availableAt),ownPosition=account.positionExchangeIds.includes(b.exchangeId),gate=ownPosition?{...researchGate,accepted:false,reason:'own_account_exposure_reduce_only'}:researchGate;inc(gate.reason);
 if(ownPosition)for(const w of Object.values(s.wallets)){const st=w.state.strategies.mean_reversion;if(st.pending[b.instrument]?.action==='buy'){delete st.pending[b.instrument];st.expiredIntents++;}}
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
export function receiptProbabilityReport(s,bank,at){return{mode:bank.mode,updatedAt:at,bankHash:s.bankHash,cutoff:bank.cutoff,accountingContinuation:bank.accountingContinuation,counts:s.counts,decisions:s.decisions,actualConsumer:{lastAvailableAt:s.lastAvailableAt,barriers:s.barriers},wallets:Object.fromEntries(Object.entries(s.wallets).map(([latency,w])=>{const r=report(w.state,{...bank.config,latencyMs:Number(latency)},at).strategies.mean_reversion;return[latency,{...r,positions:r.positions.map(x=>({...x,label:'Market '+bank.universe.exchanges.find(y=>y.exchangeId+':YES'===x.instrument).marketId+' · YES'})),pending:Object.entries(w.state.strategies.mean_reversion.pending).map(([instrument,x])=>({...x,instrument,marketId:bank.universe.exchanges.find(y=>y.exchangeId+':YES'===instrument).marketId}))}];})),networkRequests:0,competitionOrders:0,profitabilityEstablished:false,limitations:['Forecast bands and settlement review are supplied research attestations, not authenticated or calibrated probability guarantees.','Settlement probability minus purchase cost does not forecast a profitable sale within the two-day holding window.','Counterfactual later-request fills use one-percent depth, one adverse tick and zero fees; latencies share correlated opportunities.','Fresh causal positions block new buys; fresh complete open orders block all simulated fills on our own liquidity.', 'Official account positions may already influence public prices. Reduce-only paper exits are counterfactual and omit market impact.', 'The original wallet cash, basis and fills are carried forward; the continuation cutoff does not reset performance.']};}
