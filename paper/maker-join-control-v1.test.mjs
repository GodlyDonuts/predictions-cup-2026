import test from 'node:test';import assert from 'node:assert/strict';
import {inventoryAwareQuotesV1} from './inventory-aware-quote-v1.mjs';import {makerJoinControlV1,MAKER_JOIN_CONTROL_SCHEDULER_V1} from './maker-join-control-v1.mjs';
import {adaptMakerProposalV1} from './maker-proposal-adapter-v1.mjs';import {allocateFundedMakerQuotesV1} from '../execution/maker-funded-allocation-v1.mjs';
import {createMakerSchedulerV1,planMakerQuoteV1,MAKER_SCHEDULER_POLICY_V1} from '../execution/maker-quote-scheduler-v1.mjs';
const at='2026-10-12T22:50:00.000Z',book={marketId:'306',exchangeId:'995',requestStartedAt:at,receivedAt:at,versionKey:'original-test',
  bids:[{price:'0.110000',quantity:6516}],asks:[{price:'0.115000',quantity:780}]};
function fixture(cashAvailable='5.000000'){const parent=inventoryAwareQuotesV1({book,inventory:{yes:0,no:0},cashAvailable,variancePerSecond:0,maximumShares:5,maximumOrderCost:'5.000000',now:at});
  return{parent,proposal:makerJoinControlV1({inventoryProposal:parent,book})};}
test('joins original touch prices without changing old source policy or claiming funded execution',()=>{
  const{parent,proposal}=fixture();assert.deepEqual(parent.buys.map(q=>q.limit),['0.105000','0.880000']);
  assert.deepEqual(proposal.buys.map(q=>q.limit),['0.110000','0.885000']);assert.equal(proposal.maximumQuoteReservation,'4.975000');
  assert.equal(proposal.requiresSharedFundingAllocation,true);assert.equal(MAKER_SCHEDULER_POLICY_V1.minimumNetEntryEdge,'0.005000');
});
test('separate positive-micro control admits a one-tick bracket after shared funding; reserve stress can remove it',()=>{
  const{proposal}=fixture();const request={proposal,book,markoutReserve:{perShare:'0.000000',sourceHash:'a'.repeat(64),knownAt:at},at,lifetimeMs:10000};
  const mapped=adaptMakerProposalV1(request),funding={at,cashAvailable:'5.000000',maximumCommitted:'5.000000',maximumOrderCost:'5.000000',
    maximumShares:5,ownedPositions:[],openOrders:[],proposals:mapped.quotes,quotePolicy:MAKER_JOIN_CONTROL_SCHEDULER_V1};
  const a=allocateFundedMakerQuotesV1(funding);const input={at,account:{observedAt:at,complete:true,orders:[]},desired:a.quotes,
    budget:{availableWrites:1,nextWriteAt:at},entriesEnabled:true,exitsEnabled:true,cancelEnabled:true};
  assert.equal(planMakerQuoteV1(createMakerSchedulerV1(),input,MAKER_JOIN_CONTROL_SCHEDULER_V1).kind,'submit_batch');
  assert.equal(allocateFundedMakerQuotesV1({...funding,quotePolicy:MAKER_SCHEDULER_POLICY_V1}).quotes.length,0);
  request.markoutReserve.perShare='0.005000';assert.equal(allocateFundedMakerQuotesV1({...funding,proposals:adaptMakerProposalV1(request).quotes}).quotes.length,0);
});
test('repriced quantities cannot reuse old cheaper-price funding claim',()=>{
  const{proposal}=fixture('4.925000');const mapped=adaptMakerProposalV1({proposal,book,markoutReserve:{perShare:'0.000000',sourceHash:'a'.repeat(64),knownAt:at},at,lifetimeMs:10000});
  const a=allocateFundedMakerQuotesV1({at,cashAvailable:'4.925000',maximumCommitted:'4.925000',maximumOrderCost:'5.000000',maximumShares:5,
    ownedPositions:[],openOrders:[],proposals:mapped.quotes,quotePolicy:MAKER_JOIN_CONTROL_SCHEDULER_V1});
  assert.equal(a.cashAvailableAfter,'0.835000');assert.equal(a.additionalBuyReservation,'4.090000');
  assert.equal(a.quotes.find(q=>q.side==='no').quantity,4);
});
