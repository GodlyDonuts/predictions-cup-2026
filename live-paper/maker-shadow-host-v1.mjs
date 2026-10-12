import {Journal} from './transport-runtime.mjs';
import {allocateFundedMakerQuotesV1} from '../execution/maker-funded-allocation-v1.mjs';
import {createMakerSchedulerV1,planMakerQuoteV1} from '../execution/maker-quote-scheduler-v1.mjs';
import {payloadHash} from '../execution/ledger.mjs';

// A causal, durable observer of the proposed operating policy. It does not
// turn placement proposals into fills or clear the contest execution stop.
export function openMakerShadowHostV1({file,policy}) {
 const journal=new Journal(file),policyHash=payloadHash(policy);
 if(journal.last&&journal.last.state.policyHash!==policyHash)throw Error('MAKER_SHADOW_POLICY_CHANGED');
 return {observe({at,account,funding,budget,entriesEnabled,exitsEnabled,cancelEnabled,schedulerState=createMakerSchedulerV1()}) {
  if(journal.last&&Date.parse(at)<Date.parse(journal.last.state.at))throw Error('MAKER_SHADOW_CLOCK_REGRESSION');
  if(account?.complete!==true||funding.openOrders.length!==account.orders.filter(q=>['open','partial'].includes(q.status)).length)
   throw Error('MAKER_SHADOW_ACCOUNT_SCOPE');
  const actual=account.orders.filter(q=>['open','partial'].includes(q.status));
  if(payloadHash(actual)!==payloadHash(funding.openOrders))throw Error('MAKER_SHADOW_ORDER_BINDING');
  const allocation=allocateFundedMakerQuotesV1({...funding,at,quotePolicy:policy});
  const decision=planMakerQuoteV1(schedulerState,{at,account,desired:allocation.quotes,budget,entriesEnabled,exitsEnabled,cancelEnabled},policy);
  const state={schemaVersion:1,mode:'maker_shadow_planning_only',at,policyHash,allocation,decision,
   inputHash:payloadHash({at,account,funding,budget,entriesEnabled,exitsEnabled,cancelEnabled,schedulerState}),
   networkRequests:0,contestOrders:0,fills:0,profitabilityEstablished:false};
  journal.commit(state,{type:'maker_shadow_decision',at,decisionKey:decision.dispatchKey??null});
  return structuredClone(state);
 },get last(){return journal.last?structuredClone(journal.last.state):null;}};
}
