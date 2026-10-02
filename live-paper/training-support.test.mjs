import test from 'node:test';
import assert from 'node:assert/strict';
import {trainingSupport,validateTrainingScope,trainBank} from './ml-bank.mjs';
import {ROUND_TRIP} from './roundtrip-target.mjs';
const rows=(n,marketId,instrument)=>Array.from({length:n},()=>({marketId,instrument,x:[1,0],y:0}));
test('supported scope excludes unsupported labels without lowering per-market coverage thresholds',()=>{
 const train=[...rows(20,'1','A'),...rows(7,'2','B'),...rows(30,'3','C')],test=[...rows(10,'1','A'),...rows(5,'2','B'),...rows(10,'3','replacement')];
 const all=trainingSupport(train,test,['1','2','3']);assert.equal(all.eligible,false);assert.equal(all.train.length,57);
 const subset=trainingSupport(train,test,['1','2','3'],'supported-instruments-v1');assert.equal(subset.eligible,true);assert.equal(subset.train.length,20);assert.equal(subset.test.length,10);assert.ok(subset.train.every(r=>r.instrument==='A'));assert.deepEqual(subset.coverage.map(r=>r.eligible),[true,false,false]);
 const empty=trainingSupport(rows(7,'1','A'),rows(3,'1','A'),['1'],'supported-instruments-v1');assert.equal(empty.eligible,false);assert.equal(empty.train.length,0);assert.equal(empty.test.length,0);
});
test('supported scope cannot bypass runtime coverage or use incompatible legacy targets',()=>{
 assert.equal(validateTrainingScope('supported-instruments-v1',true,ROUND_TRIP),'supported-instruments-v1');
 assert.throws(()=>validateTrainingScope('supported-instruments-v1',false,ROUND_TRIP),/coverage-gated/);assert.throws(()=>validateTrainingScope('supported-instruments-v1',true),/coverage-gated/);assert.throws(()=>validateTrainingScope('unknown'),/coverage-gated/);
 assert.throws(()=>trainBank('',{}, {trainingScope:'supported-instruments-v1',targetPolicy:ROUND_TRIP}),/coverage-gated/);
});
