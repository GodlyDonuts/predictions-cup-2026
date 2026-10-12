import test from 'node:test';import assert from 'node:assert/strict';
import {pairScenarioReturn as expected,pairScenarioPriceCap as cap} from './price-cap.mjs';
const input={quantity:1,firstLegLimit:'0.575000',unmatchedUnwindLoss:'0.015000',completionProbability:'0.900000',exceptionProbability:'0.005000',maximumPackageReservation:'0.995000'};
test('worst permitted hedge cost can negate the positive observed-price expectation',()=>{
 const old=expected({...input,principal:'0.980000'}),worst=expected({...input,principal:'0.995000'});
 assert.equal(old.expectedReturnFloor,'0.011517');assert.equal(old.strictlyPositive,true);
 assert.equal(worst.expectedReturnFloor,'-0.001990');assert.equal(worst.strictlyPositive,false);
});
test('exact inequality tightens the hedge price without altering first-leg custody',()=>{
 const r=cap(input);assert.equal(r.maximumPackageUnitLimit,'0.990000');assert.equal(r.maximumHedgeLimit,'0.415000');assert.equal(r.returnAtCap.expectedReturnFloor,'0.002512');assert.equal(r.executionAuthority,false);
});
test('exception and completion sensitivities change the protected tick cap monotonically',()=>{
 assert.equal(cap({...input,exceptionProbability:'0.010000'}).maximumPackageUnitLimit,'0.985000');
 assert.equal(cap({...input,completionProbability:'0.500000',exceptionProbability:'0.000000'}).maximumPackageUnitLimit,'0.980000');
 assert.equal(cap({...input,completionProbability:'1.000000',exceptionProbability:'0.000000'}).maximumPackageUnitLimit,'0.995000');
});
test('gross reservation and a positive expected-return target bind independently',()=>{
 assert.equal(cap({...input,maximumPackageReservation:'0.980000'}).maximumPackageUnitLimit,'0.980000');
 assert.equal(cap({...input,minimumExpectedReturn:'0.005000'}).maximumPackageUnitLimit,'0.985000');
 assert.equal(cap({...input,maximumPackageReservation:'0.500000'}).feasible,false);
});
test('larger quantity scales exact scenario economics and uses a uniform limit for the full package',()=>{
 const r=cap({...input,quantity:100,unmatchedUnwindLoss:'1.500000',maximumPackageReservation:'99.500000',minimumExpectedReturn:'0.000100'});
 assert.equal(r.maximumPackagePrincipal,'99.000000');assert.equal(r.returnAtCap.expectedReturnFloor,'0.251250');assert.equal(r.maximumHedgeLimit,'0.415000');
});
test('certain exception, zero completion, invalid odds and non-tick first prices cannot create a cap',()=>{
 assert.equal(cap({...input,exceptionProbability:'1.000000'}).feasible,false);
 assert.equal(cap({...input,completionProbability:'0.000000',exceptionProbability:'0.000000'}).feasible,false);
 assert.throws(()=>cap({...input,completionProbability:'1.000001'}),/SCENARIO_PROBABILITY/);
 assert.throws(()=>cap({...input,firstLegLimit:'0.576000'}),/tick/);
});
test('fractional negative expected returns round down instead of disappearing at zero',()=>{
 const r=expected({quantity:1,principal:'0.500000',unmatchedUnwindLoss:'0.000001',completionProbability:'0.000000',exceptionProbability:'0.500000'});
 assert.equal(r.expectedReturnFloor,'-0.250001');
});
