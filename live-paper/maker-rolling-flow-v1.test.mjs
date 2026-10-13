import test from 'node:test';import assert from 'node:assert/strict';import {createMakerRollingFlowV1} from './maker-rolling-flow-v1.mjs';
const start=Date.parse('2026-10-13T00:20:00.000Z'),at=ms=>new Date(start+ms).toISOString();
function fixture(options={}){let clock=start;const scope={tournamentId:'test-scope',markets:[{marketId:'322',exchangeId:'1001'},{marketId:'254',exchangeId:'1002'}]};
  const f=createMakerRollingFlowV1({scope,now:()=>clock,...options});const row=(id='100',extra={})=>({kind:'trade_observed',observedAt:at(0),
    trade:{id,sequence:Number(id),tournamentId:scope.tournamentId,marketId:'322',exchangeId:'1001',quantity:100,price:.5,executedAt:at(0),...extra}});
  return{f,row,advance:ms=>clock=start+ms};}
test('deduplicates engine matches and ranks current scoped shares without interpreting readiness',()=>{
  const x=fixture();assert.equal(x.f.observe(x.row()),true);assert.equal(x.f.observe(x.row()),false);
  x.f.observe(x.row('101',{marketId:'254',exchangeId:'1002',quantity:200}));assert.deepEqual(x.f.recoveryPriority().marketIds,['254','322']);
  assert.equal(x.f.snapshot().duplicates,1);assert.equal(x.f.snapshot().executionAuthority,false);
});
test('stale receipt/history and foreign scope cannot become current activity',()=>{
  const x=fixture();x.advance(10000);assert.equal(x.f.observe(x.row('102',{executedAt:at(0)})),true);
  const late=x.row('103');late.observedAt=at(10000);assert.equal(x.f.observe(late),false);
  assert.throws(()=>x.f.observe(x.row('104',{tournamentId:'foreign'})),/FOREIGN_SCOPE/);
  x.advance(121000);assert.deepEqual(x.f.recoveryPriority().marketIds,[]);
});
test('conflicting same engine identity rejects instead of adding volume twice',()=>{
  const x=fixture();x.f.observe(x.row());assert.throws(()=>x.f.observe(x.row('100',{quantity:101})),/MATCH_ID_CONFLICT/);
  assert.throws(()=>x.f.observe(x.row('100',{sequence:101})),/MATCH_ID_CONFLICT/);
  assert.equal(x.f.snapshot().ranked[0].shares,100);
});
test('bounded storage reports truncation and expiration frees capacity',()=>{
  const x=fixture({maximumRecords:1});x.f.observe(x.row());assert.equal(x.f.observe(x.row('101')),false);assert.equal(x.f.snapshot().sampleTruncated,true);
  x.advance(901000);const fresh=x.row('102',{executedAt:at(901000)});fresh.observedAt=at(901000);assert.equal(x.f.observe(fresh),true);
  assert.equal(x.f.snapshot().records,1);
});
test('reading a priority snapshot does not refresh the underlying observation clock',()=>{
  const x=fixture();x.f.observe(x.row());x.advance(60000);const p=x.f.recoveryPriority();
  assert.equal(p.at,at(0));assert.equal(p.generatedAt,at(60000));assert.deepEqual(p.marketIds,['322']);
});
