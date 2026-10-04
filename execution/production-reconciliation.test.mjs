import test from 'node:test';import assert from 'node:assert/strict';
import {reconcileProductionAccount,fillsReadAuthorized} from './production-reconciliation.mjs';
const tid='bda92870-621e-47b0-bc3c-3602c5c26f55',at='2026-10-04T22:00:00.000Z';
function fixture(){
 const r=data=>({status:200,receivedAt:at,requestStartedAt:at,data});
 const fill=id=>({id,orderId:100,exchangeId:'700',marketId:'377',quantity:1,side:'yes',price:.4,filledAt:at});
 const page=(ids,more,cursor)=>r({data:ids.map(fill),pagination:{limit:100,hasMore:more,nextCursor:cursor},coverage:{complete:true,projectedThroughSequence:10}});
 const f={universe:{exchanges:[{exchangeId:'700',marketId:'377'}]},responses:[page([1],true,'next'),page([2],false,null)],transport:{async orders(kind){return r({data:kind==='open'?[]:[{id:100}],pagination:{hasMore:false}});},async positions(){return r({positions:[{marketId:'377',exchangeId:'700',quantity:5}]});},async tournament(){return r({id:tid,status:'active',isPendingEnrolment:false,myBalance:101.005});}},now:()=>Date.parse(at),async dispatch(s){assert(fillsReadAuthorized(s));return f.responses.shift();}};return f;
}
test('complete paginated fills and existing inventory are reconciled without attribution or writes',async()=>{
 const f=fixture(),x=await reconcileProductionAccount(f);assert.equal(x.summary.fills,2);assert.equal(x.summary.positions,1);assert.equal(x.summary.openOrders,0);assert.equal(x.receipt.cashFloor,'101.004999');assert(Number(x.receipt.cashFloor)<=101.005);assert.equal(x.receipt.receiptHash,undefined);assert.equal(x.receiptHash.length,64);assert.equal(x.receipt.positions.data.positions[0].quantity,5);
});
test('incomplete, changing, duplicated or foreign fills never complete the reconciliation',async()=>{
 for(const mode of ['missing_cursor','partial_coverage','different_sequence','duplicate','foreign']){
  const f=fixture();if(mode==='missing_cursor')f.responses[0].data.pagination.nextCursor=null;
  if(mode==='partial_coverage')f.responses[0].data.coverage.complete=false;
  if(mode==='different_sequence')f.responses[1].data.coverage.projectedThroughSequence++;
  if(mode==='duplicate')f.responses[1].data.data[0].id=1;
  if(mode==='foreign')f.responses[1].data.data[0].marketId='378';
  await assert.rejects(reconcileProductionAccount(f));
 }
});
test('stale or inactive account and unbounded pages cannot be ACKed',async()=>{
 const f=fixture();f.now=()=>Date.parse(at)+90001;await assert.rejects(reconcileProductionAccount(f),/FRESHNESS/);
 const g=fixture();g.transport.tournament=async()=>({status:200,receivedAt:at,data:{myBalance:100,status:'closed',isPendingEnrolment:false}});await assert.rejects(reconcileProductionAccount(g),/TOURNAMENT/);
 const h=fixture();await assert.rejects(reconcileProductionAccount({...h,maxPages:1}),/PAGE_LIMIT/);
 assert.equal(fillsReadAuthorized({method:'POST',endpoint:'/orders'}),false);
});
