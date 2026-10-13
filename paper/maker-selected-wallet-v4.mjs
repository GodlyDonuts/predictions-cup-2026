import {makerVolumeBeforePageV1,makerVolumeAfterPageV1} from './maker-volume-frontier-v1.mjs';
import {createMakerProjectionBarrierV1,observeMakerDeadlineBookV1,observeMakerProjectionPageV1,makerProjectionRetirementV1} from './maker-projection-barrier-v1.mjs';
import {createInventoryQuoteWalletV1,advanceInventoryQuoteWalletV1,assertInventoryQuoteWalletV1,inventoryQuoteWalletReportV1} from './inventory-quote-wallet-v1.mjs';
import {createMakerSchedulerV1,planMakerQuoteV1,beginMakerDispatchV1,finishMakerDispatchV1} from '../execution/maker-quote-scheduler-v1.mjs';
import {allocateFundedMakerQuotesV2} from '../execution/maker-funded-allocation-v2.mjs';
import {amount,money,instant} from './fixed.mjs';import {payloadHash} from '../execution/ledger.mjs';
import {SCALE,TICK} from './fixed.mjs';import {instantNs} from '../feed/contract.mjs';import {tapeTick} from '../live-paper/canonical-entry-exit-study.mjs';
const open=s=>s.wallet.quotes.filter(q=>!q.closedAt);
function tracker(q,p){const end=Math.min(q.expiresAt,q.cancelEffectiveAt??Infinity);if(q.projection&&Date.parse(q.projection.endAt)!==end)q.projection={...q.projection,endAt:new Date(end).toISOString(),barrier:null};if(!q.projection)q.projection=createMakerProjectionBarrierV1({scope:{tournamentId:p.tournamentId,marketId:q.marketId,exchangeId:q.exchangeId},activationAt:new Date(q.activationAt).toISOString(),endAt:new Date(end).toISOString()});return q.projection;}
function retire(s,at){for(const q of open(s)){if(!q.projection)continue;const evidence=makerProjectionRetirementV1(q.projection);if(evidence.canRetire){q.closedAt=at;q.retirementEvidence=evidence;q.participationCredit=0;}}const pending=s.scheduler.pending;if(pending?.kind==='cancel'&&!open(s).some(q=>String(q.id)===pending.orderId))s.scheduler=finishMakerDispatchV1(s.scheduler,{dispatchKey:pending.dispatchKey,outcome:'confirmed',reconciledAt:at});}
export function createMakerSelectedWalletV4(policy){
 if(typeof policy.tournamentId!=='string'||!policy.tournamentId)throw Error('MAKER_SELECTED_TOURNAMENT');
 if(policy.wallet.fillModel!=='strict_tape_through'||!Number.isSafeInteger(policy.participationBps)||policy.participationBps<1||policy.participationBps>10000)throw Error('MAKER_SELECTED_POLICY');
 return {schemaVersion:1,policyHash:payloadHash(policy),wallet:createInventoryQuoteWalletV1(policy.wallet),scheduler:createMakerSchedulerV1(),lastAt:policy.wallet.cutoff,decisions:[],writeTimes:[]};
}
export function advanceMakerSelectedWalletV4(s,event,policy){
 if(s.policyHash!==payloadHash(policy))throw Error('MAKER_SELECTED_POLICY_CHANGED');const now=instant(event.at);if(now<instant(s.lastAt))throw Error('MAKER_SELECTED_CLOCK');s.lastAt=event.at;
 if(!/^[a-f0-9]{64}$/.test(event.sourceHash??''))throw Error('MAKER_SELECTED_SOURCE');
 // Local simulation only: modeled cancel acknowledgement never releases financial reserves.
 const cancellation=s.scheduler.pending;if(cancellation?.kind==='cancel'){const q=open(s).find(q=>String(q.id)===cancellation.orderId);if(q&&Number.isSafeInteger(q.cancelEffectiveAt)&&now>=q.cancelEffectiveAt){q.modeledCancelAcknowledgedAt=event.at;s.scheduler=finishMakerDispatchV1(s.scheduler,{dispatchKey:cancellation.dispatchKey,outcome:'confirmed',reconciledAt:event.at});}}

 if(event.kind==='trades'){
  const page=event.page;s.participationSeen??={};s.participationCounts??={prints:0,smallPrints:0,eligiblePrints:0,grantedShares:0};
  if(instant(page.receivedAt)>now||instant(page.from)>=instant(page.to))throw Error('MAKER_SELECTED_PAGE_CLOCK');
  const volumeQuotes=open(s).filter(q=>q.exchangeId===page.exchangeId&&q.marketId===page.marketId);for(const q of volumeQuotes){const before=makerVolumeBeforePageV1(q,page);q.observedVolume=before.state;if(before.resetCredit)q.participationCredit=0;}
  for(const t of [...page.trades].sort((a,b)=>instantNs(a.createdAt)<instantNs(b.createdAt)?-1:instantNs(a.createdAt)>instantNs(b.createdAt)?1:a.key.localeCompare(b.key))){
   if(s.participationSeen[t.key])continue;
   const created=instantNs(t.createdAt);if(!Number.isSafeInteger(t.size)||t.size<1||t.size>2147483647||typeof t.key!=='string'||created<BigInt(instant(page.from))*1000000n||created>=BigInt(instant(page.to))*1000000n)throw Error('MAKER_SELECTED_TRADE');
   s.participationSeen[t.key]=true;s.participationCounts.prints++;if(t.size<100)s.participationCounts.smallPrints++;
   const tick=tapeTick(t);if(!tick||tick.tick===0||tick.tick===200)continue;
   const q=open(s).find(q=>{if(q.exchangeId!==page.exchangeId||q.marketId!==page.marketId||q.censored||created<BigInt(q.activationAt)*1000000n||created>=BigInt(Math.min(q.expiresAt,q.cancelEffectiveAt??Infinity))*1000000n)return false;const limit=q.side==='YES'?amount(q.limit):SCALE-amount(q.limit),asks=(q.side==='YES')===(q.action==='buy');return asks?amount(tick.price)<=limit-TICK:amount(tick.price)>=limit+TICK;});
   if(!q)continue;s.participationCounts.eligiblePrints++;q.participationCredit=(q.participationCredit??0)+t.size*policy.participationBps;
   const grant=Math.min(t.size,q.remaining,Math.floor(q.participationCredit/10000));if(!grant)continue;q.participationCredit-=grant*10000;s.participationCounts.grantedShares+=grant;
   advanceInventoryQuoteWalletV1(s.wallet,{...event,page:{...page,coverage:{...page.coverage,complete:false},trades:[{...t,size:grant}]}},policy.wallet);
  }
  advanceInventoryQuoteWalletV1(s.wallet,{...event,page:{...page,coverage:{...page.coverage,complete:false},trades:[]}},policy.wallet);
  s.wallet.tradeCoverage[page.exchangeId]={knownAt:event.at,to:page.to,complete:page.coverage.complete===true&&page.pagination.hasMore===false};
  for(const q of open(s).filter(q=>q.exchangeId===page.exchangeId&&q.marketId===page.marketId))q.projection=observeMakerProjectionPageV1(tracker(q,policy),page);
  for(const q of volumeQuotes)q.observedVolume=makerVolumeAfterPageV1(q.observedVolume,page);
  retire(s,event.at);
  for(const q of s.wallet.quotes)if(q.closedAt)q.participationCredit=0;
  const pending=s.scheduler.pending;
  if(pending?.kind==='cancel'&&!open(s).some(q=>String(q.id)===pending.orderId))s.scheduler=finishMakerDispatchV1(s.scheduler,{dispatchKey:pending.dispatchKey,outcome:'confirmed',reconciledAt:event.at});
 }else if(event.kind==='book'){
  s.wallet.lastAt=event.at;const b=event.book;for(const q of open(s).filter(q=>q.exchangeId===b.exchangeId&&q.marketId===b.marketId))q.projection=observeMakerDeadlineBookV1(tracker(q,policy),b);retire(s,event.at);
 }else if(event.kind==='quotes'){
  s.wallet.lastAt=event.at;
  const markets=new Set([...Object.keys(s.wallet.positions),...open(s).map(q=>q.exchangeId)]),proposals=[];
  for(const q of event.proposals){if(instant(q.orderExpiresAt)<now+policy.wallet.latencyMs+policy.scheduler.minimumQuoteAgeMs)continue;if(!/^[1-9]\d*$/.test(q.marketId??''))throw Error('MAKER_SELECTED_MARKET');if(!markets.has(q.exchangeId)&&markets.size>=policy.wallet.maximumMarkets)continue;markets.add(q.exchangeId);proposals.push(q);}
  const orders=open(s).map(q=>({orderId:String(q.id),exchangeId:q.exchangeId,side:q.side.toLowerCase(),action:q.action,quantity:q.quantity,remainingQuantity:q.remaining,price:q.limit,status:q.remaining===q.quantity?'open':'partial',createdAt:q.createdAt,financialOnly:now>=Math.min(q.expiresAt,q.cancelEffectiveAt??Infinity)}));
  const reserved=orders.filter(q=>q.action==='buy').reduce((n,q)=>n+amount(q.price)*BigInt(q.remainingQuantity),0n);
  const allocation=allocateFundedMakerQuotesV2({at:event.at,cashAvailable:money(amount(s.wallet.cash)-reserved),maximumCommitted:policy.wallet.maximumCommitted,maximumOrderCost:policy.wallet.maximumOrderCost,maximumShares:policy.wallet.quantity,
   ownedPositions:Object.entries(s.wallet.positions).map(([exchangeId,p])=>({exchangeId,side:p.side.toLowerCase(),quantity:p.quantity,cost:p.cost})),openOrders:orders,proposals,quotePolicy:policy.scheduler});
  s.writeTimes=s.writeTimes.filter(t=>now-t<60000);const last=s.writeTimes.at(-1);
  const decision=planMakerQuoteV1(s.scheduler,{at:event.at,account:{complete:true,observedAt:event.at,orders:orders.filter(q=>!q.financialOnly)},desired:allocation.quotes,budget:{availableWrites:Math.max(0,24-s.writeTimes.length),nextWriteAt:new Date(last===undefined?now:last+2500).toISOString()},entriesEnabled:event.entriesEnabled===true,exitsEnabled:true,cancelEnabled:true},policy.scheduler);
  s.decisions.push({at:event.at,decision,allocationHash:allocation.allocationHash,sourceHash:event.sourceHash});
  if(decision.kind!=='hold'){
   s.scheduler=beginMakerDispatchV1(s.scheduler,decision);s.writeTimes.push(now);
   if(decision.kind==='cancel'){
    const q=open(s).find(q=>String(q.id)===decision.orderId);if(!q)throw Error('MAKER_SELECTED_CANCEL_ID');q.cancelEffectiveAt=now+policy.wallet.latencyMs;
   }else{
    for(const q of decision.kind==='submit_batch'?decision.quotes:[decision.quote])s.wallet.quotes.push({id:s.wallet.nextId++,marketId:q.marketId,exchangeId:q.exchangeId,side:q.side.toUpperCase(),action:q.action,quantity:q.quantity,remaining:q.quantity,limit:q.price,createdAt:event.at,activationAt:now+policy.wallet.latencyMs,expiresAt:instant(q.orderExpiresAt),decisionVersion:q.sourceBinding?.bookVersion??'scheduler',decisionHash:decision.dispatchKey,censored:false,closedAt:null});
    s.scheduler=finishMakerDispatchV1(s.scheduler,{dispatchKey:decision.dispatchKey,outcome:'confirmed',reconciledAt:event.at});
   }
  }
 }else throw Error('MAKER_SELECTED_EVENT');
 assertInventoryQuoteWalletV1(s.wallet,policy.wallet);return s;
}
export function makerSelectedWalletReportV4(s,p){return {...inventoryQuoteWalletReportV1(s.wallet,p.wallet),mode:'scheduler_selected_counterfactual_only',decisions:s.decisions.length,decisionReasons:s.decisions.reduce((a,d)=>({...a,[d.decision.reason]:(a[d.decision.reason]??0)+1}),{}),submittedQuotes:s.wallet.quotes.length,totalWriteActions:s.decisions.filter(d=>d.decision.kind!=='hold').length,writeActions:s.writeTimes.length,pending:s.scheduler.pending,participationBps:p.participationBps,participationCounts:s.participationCounts??{},
 fillAssumptions:['One-tick trade-through after simulated activation.','Tape volume participation cap; aggressor and exact queue position are unknown.','This is local counterfactual accounting, not evidence of contest fills.'],profitabilityEstablished:false};}


