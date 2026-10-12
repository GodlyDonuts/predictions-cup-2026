import {createInventoryQuoteWalletV1,advanceInventoryQuoteWalletV1,assertInventoryQuoteWalletV1,inventoryQuoteWalletReportV1} from './inventory-quote-wallet-v1.mjs';
import {createMakerSchedulerV1,planMakerQuoteV1,beginMakerDispatchV1,finishMakerDispatchV1} from '../execution/maker-quote-scheduler-v1.mjs';
import {allocateFundedMakerQuotesV1} from '../execution/maker-funded-allocation-v1.mjs';
import {amount,money,instant} from './fixed.mjs';import {payloadHash} from '../execution/ledger.mjs';
const open=s=>s.wallet.quotes.filter(q=>!q.closedAt);
export function createMakerSelectedWalletV1(policy){
 if(policy.wallet.fillModel!=='strict_tape_through'||!Number.isSafeInteger(policy.participationBps)||policy.participationBps<1||policy.participationBps>10000)throw Error('MAKER_SELECTED_POLICY');
 return {schemaVersion:1,policyHash:payloadHash(policy),wallet:createInventoryQuoteWalletV1(policy.wallet),scheduler:createMakerSchedulerV1(),lastAt:policy.wallet.cutoff,decisions:[],writeTimes:[]};
}
export function advanceMakerSelectedWalletV1(s,event,policy){
 if(s.policyHash!==payloadHash(policy))throw Error('MAKER_SELECTED_POLICY_CHANGED');const now=instant(event.at);if(now<instant(s.lastAt))throw Error('MAKER_SELECTED_CLOCK');s.lastAt=event.at;
 if(!/^[a-f0-9]{64}$/.test(event.sourceHash??''))throw Error('MAKER_SELECTED_SOURCE');
 if(event.kind==='trades'){
  const page={...event.page,trades:event.page.trades.map(t=>({...t,size:Math.floor(t.size*policy.participationBps/10000)})).filter(t=>t.size>0)};
  advanceInventoryQuoteWalletV1(s.wallet,{...event,page},policy.wallet);
  const pending=s.scheduler.pending;
  if(pending?.kind==='cancel'&&!open(s).some(q=>String(q.id)===pending.orderId))s.scheduler=finishMakerDispatchV1(s.scheduler,{dispatchKey:pending.dispatchKey,outcome:'confirmed',reconciledAt:event.at});
 }else if(event.kind==='quotes'){
  s.wallet.lastAt=event.at;
  const markets=new Set([...Object.keys(s.wallet.positions),...open(s).map(q=>q.exchangeId)]),proposals=[];
  for(const q of event.proposals){if(!/^[1-9]\d*$/.test(q.marketId??''))throw Error('MAKER_SELECTED_MARKET');if(!markets.has(q.exchangeId)&&markets.size>=policy.wallet.maximumMarkets)continue;markets.add(q.exchangeId);proposals.push(q);}
  const orders=open(s).map(q=>({orderId:String(q.id),exchangeId:q.exchangeId,side:q.side.toLowerCase(),action:q.action,quantity:q.quantity,remainingQuantity:q.remaining,price:q.limit,status:q.remaining===q.quantity?'open':'partial',createdAt:q.createdAt}));
  const reserved=orders.filter(q=>q.action==='buy').reduce((n,q)=>n+amount(q.price)*BigInt(q.remainingQuantity),0n);
  const allocation=allocateFundedMakerQuotesV1({at:event.at,cashAvailable:money(amount(s.wallet.cash)-reserved),maximumCommitted:policy.wallet.maximumCommitted,maximumOrderCost:policy.wallet.maximumOrderCost,maximumShares:policy.wallet.quantity,
   ownedPositions:Object.entries(s.wallet.positions).map(([exchangeId,p])=>({exchangeId,side:p.side.toLowerCase(),quantity:p.quantity,cost:p.cost})),openOrders:orders,proposals,quotePolicy:policy.scheduler});
  s.writeTimes=s.writeTimes.filter(t=>now-t<60000);const last=s.writeTimes.at(-1);
  const decision=planMakerQuoteV1(s.scheduler,{at:event.at,account:{complete:true,observedAt:event.at,orders},desired:allocation.quotes,budget:{availableWrites:Math.max(0,24-s.writeTimes.length),nextWriteAt:new Date(last===undefined?now:last+2500).toISOString()},entriesEnabled:event.entriesEnabled===true,exitsEnabled:true,cancelEnabled:true},policy.scheduler);
  s.decisions.push({at:event.at,decision,allocationHash:allocation.allocationHash,sourceHash:event.sourceHash});
  if(decision.kind!=='hold'){
   s.scheduler=beginMakerDispatchV1(s.scheduler,decision);s.writeTimes.push(now);
   if(decision.kind==='cancel'){
    const q=open(s).find(q=>String(q.id)===decision.orderId);if(!q)throw Error('MAKER_SELECTED_CANCEL_ID');q.cancelEffectiveAt=now+policy.wallet.latencyMs;
   }else{
    for(const q of decision.kind==='submit_batch'?decision.quotes:[decision.quote])s.wallet.quotes.push({id:s.wallet.nextId++,marketId:q.marketId,exchangeId:q.exchangeId,side:q.side.toUpperCase(),action:q.action,quantity:q.quantity,remaining:q.quantity,limit:q.price,createdAt:event.at,activationAt:now+policy.wallet.latencyMs,expiresAt:Math.min(instant(q.validUntil),now+policy.wallet.lifetimeMs),decisionVersion:q.sourceBinding?.bookVersion??'scheduler',decisionHash:decision.dispatchKey,censored:false,closedAt:null});
    s.scheduler=finishMakerDispatchV1(s.scheduler,{dispatchKey:decision.dispatchKey,outcome:'confirmed',reconciledAt:event.at});
   }
  }
 }else throw Error('MAKER_SELECTED_EVENT');
 assertInventoryQuoteWalletV1(s.wallet,policy.wallet);return s;
}
export function makerSelectedWalletReportV1(s,p){return {...inventoryQuoteWalletReportV1(s.wallet,p.wallet),mode:'scheduler_selected_counterfactual_only',decisions:s.decisions.length,writeActions:s.writeTimes.length,pending:s.scheduler.pending,participationBps:p.participationBps,
 fillAssumptions:['One-tick trade-through after simulated activation.','Tape volume participation cap; aggressor and exact queue position are unknown.','This is local counterfactual accounting, not evidence of contest fills.'],profitabilityEstablished:false};}
