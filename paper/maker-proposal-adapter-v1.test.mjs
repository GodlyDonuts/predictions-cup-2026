import test from 'node:test';import assert from 'node:assert/strict';
import {payloadHash} from '../execution/ledger.mjs';import {adaptMakerProposalV1} from './maker-proposal-adapter-v1.mjs';
import {allocateFundedMakerQuotesV1} from '../execution/maker-funded-allocation-v1.mjs';
import {createMakerSchedulerV1,planMakerQuoteV1} from '../execution/maker-quote-scheduler-v1.mjs';
const at='2026-10-12T22:40:00.000Z';
function fixture(){const b={at,marketId:'11',exchangeId:'101',buys:[{side:'YES',action:'buy',quantity:2,limit:'0.450000'}],
  sells:[{side:'YES',action:'sell',quantity:1,limit:'0.470000'}]};return{at,lifetimeMs:10000,proposal:{...b,proposalHash:payloadHash(b)},
  book:{marketId:'11',exchangeId:'101',requestStartedAt:at,receivedAt:at,versionKey:'source-v1'},valuations:[
    {side:'YES',action:'buy',valuePerShare:'0.460000',feePerShare:'0.001000',valueBasis:'microprice_heuristic',knownAt:at},
    {side:'YES',action:'sell',valuePerShare:'0.460000',feePerShare:'0.001000',valueBasis:'microprice_heuristic',knownAt:at}]};}
test('maps side-relative values, fees and original source clocks without claiming calibrated alpha',()=>{
  const r=adaptMakerProposalV1(fixture());assert.deepEqual(r.quotes.map(q=>q.netEdgePerShare),['0.009000','0.009000']);
  assert.equal(r.quotes[0].reduceOnly,true);assert.equal(r.quotes[1].side,'yes');assert.equal(r.quotes[0].valueBasis,'microprice_heuristic');
  assert.equal(r.quotes[0].sourceAt,at);assert.equal(r.profitabilityEstablished,false);
});
test('missing/future valuations, altered proposal and stale source reject',()=>{
  const f=fixture();f.valuations.pop();assert.throws(()=>adaptMakerProposalV1(f),/MISSING_VALUATION/);
  const g=fixture();g.valuations[0].knownAt='2026-10-12T22:40:01.000Z';assert.throws(()=>adaptMakerProposalV1(g),/VALUATION/);
  const h=fixture();h.proposal.buys[0].quantity=9;assert.throws(()=>adaptMakerProposalV1(h),/PROPOSAL_BINDING/);
  const j=fixture();j.book.requestStartedAt='2026-10-12T22:39:00.000Z';assert.throws(()=>adaptMakerProposalV1(j),/BOOK_CLOCK/);
});
test('mapped candidates reach funded batch planner with one reserved cash charge',()=>{
  const r=adaptMakerProposalV1(fixture());const f=allocateFundedMakerQuotesV1({at,cashAvailable:'1.000000',maximumCommitted:'5.000000',
    maximumOrderCost:'1.000000',maximumShares:2,ownedPositions:[{exchangeId:'101',side:'yes',quantity:1,cost:'0.450000'}],openOrders:[],proposals:r.quotes});
  const decision=planMakerQuoteV1(createMakerSchedulerV1(),{at,account:{observedAt:at,complete:true,orders:[]},desired:f.quotes,
    budget:{availableWrites:1,nextWriteAt:at},entriesEnabled:true,exitsEnabled:true,cancelEnabled:true});
  assert.equal(decision.kind,'submit_batch');assert.equal(decision.quotes[0].action,'sell');assert.equal(f.additionalBuyReservation,'0.900000');
});
test('caller-bound markout reserve reduces heuristic edge and keeps freshness separate from receipt availability',()=>{
  const f=fixture();const {proposalHash,...body}=f.proposal;body.microprice=.46;f.proposal={...body,proposalHash:payloadHash(body)};
  delete f.valuations;f.markoutReserve={perShare:'0.003000',sourceHash:'a'.repeat(64),knownAt:at};
  f.book.requestStartedAt='2026-10-12T22:39:59.000Z';const r=adaptMakerProposalV1(f);
  assert.deepEqual(r.quotes.map(q=>q.netEdgePerShare),['0.007000','0.007000']);
  assert.equal(r.quotes[0].sourceAt,at);assert.equal(r.quotes[0].bookRequestStartedAt,f.book.requestStartedAt);
  assert.equal(r.quotes[0].markoutReservePerShare,'0.003000');assert.equal(r.quotes[0].valueBasis,'microprice_heuristic');
  f.markoutReserve.perShare='0.020000';assert.ok(adaptMakerProposalV1(f).quotes.every(q=>q.netEdgePerShare.startsWith('-')));
  f.markoutReserve.knownAt='2026-10-12T22:40:01.000Z';assert.throws(()=>adaptMakerProposalV1(f),/MARKOUT_RESERVE/);
});
