import test from 'node:test';import assert from 'node:assert/strict';
import {evaluateEntryGates as gates} from './entry-gates.mjs';
import {assessEntryGates} from './decision-audit.mjs';
const policy={minimumPrice:.05,maximumPrice:.95,maximumSpread:.02,minimumDip:.02,minimumRelativeDip:.03,maximumDip:.1,minimumZ:1.5};
const ref=(center=.275,deviation=.005)=>({ready:true,center,robustDeviation:deviation,samples:8});
test('observed exact spread and minimum discount boundaries admit; actual smaller discount still fails',()=>{
 assert(.26-.24>policy.maximumSpread);assert(.275-.255>policy.minimumDip);
 assert.equal(gates(ref(),'0.260000','0.240000',policy).tests.spread,true);
 assert.equal(gates(ref(),'0.260000','0.240000',policy).tests.minimumDiscount,false);
 assert.equal(gates(ref(),'0.255000','0.240000',policy).admitted,true);
 assert.equal(gates(ref(.28),'0.260000','0.240000',policy).admitted,true);
});
test('one micro beyond spread limit is rejected and locked/crossed books are rejected',()=>{
 for(const bid of ['0.239999','0.260000','0.265000'])assert.equal(gates(ref(.28),'0.260000',bid,policy).tests.spread,false);
});
test('reference estimates retain sub-micro precision without tolerance or tick rounding',()=>{
 assert.equal(gates(ref(.274999999999999),'0.255000','0.240000',policy).tests.minimumDiscount,false);
 assert.equal(gates(ref(.275000000000001),'0.255000','0.240000',policy).tests.minimumDiscount,true);
 assert.equal(gates(ref(.355000000000001),'0.255000','0.240000',policy).tests.maximumDiscount,false);
 assert.equal(gates(ref(.355),'0.255000','0.240000',policy).tests.maximumDiscount,true);
});
test('price, relative-discount and robust deviation gates compare products at exact boundaries',()=>{
 assert.equal(gates(ref(.075),'0.050000','0.045000',policy).tests.price,true);
 assert.equal(gates(ref(.075),'0.049999','0.045000',policy).tests.price,false);
 assert.equal(gates(ref(.97),'0.950000','0.945000',policy).tests.price,true);
 assert.equal(gates(ref(.97),'0.950001','0.945000',policy).tests.price,false);
 assert.equal(gates(ref(.5),'0.480000','0.475000',{...policy,minimumRelativeDip:.04}).tests.relativeDiscount,true);
 assert.equal(gates(ref(.5),'0.480001','0.475000',{...policy,minimumRelativeDip:.04}).tests.relativeDiscount,false);
 assert.equal(gates(ref(.275,.01),'0.260000','0.245000',policy).tests.robustZ,true);
 assert.equal(gates(ref(.275,.010000000000001),'0.260000','0.245000',policy).tests.robustZ,false);
 for(const d of [0,-.01])assert.equal(gates(ref(.275,d),'0.260000','0.245000',policy).tests.robustZ,false);
});
test('audit and executable decisions use identical gates; invalid operands cannot admit',()=>{
 for(let tick=10;tick<=190;tick++){const ask=(tick*5000/1e6).toFixed(6),bid=((tick-2)*5000/1e6).toFixed(6),r=ref(Number(ask)+.025);assert.deepEqual(assessEntryGates(r,ask,bid,policy).tests,gates(r,ask,bid,policy).tests);}
 assert.equal(gates({ready:false},null,null,null).admitted,false);
 for(const bad of [NaN,Infinity,null,{},'1e9999','secret'])assert.throws(()=>gates(ref(bad),'0.260000','0.240000',policy),/INVALID_GATE_DECIMAL/);
 assert.equal(gates(ref(1e-7,1e-8),'0.000000','-0.005000',policy).admitted,false);
});
