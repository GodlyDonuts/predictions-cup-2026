import test from 'node:test';
import assert from 'node:assert/strict';
import {stressCapitalScreen as screen} from './screen.mjs';
const sizing={equityLower:'1000.000000',cashAvailable:'1000.000000',protectedPrice:'0.500000',currentQuantity:0,reservedBuyQuantity:0,depthQuantity:10000,kellyFractionBps:1000,eventFractionBps:1000,eventPrincipal:'0.000000',grossRemaining:'1000.000000'};
const evidence={auditAt:'2026-10-12T07:00:00.000Z',bookAt:'2026-10-12T06:59:59.000Z',maxBookAgeMs:20000,sourceCensusComplete:true,populationTransportValidated:true,feeScheduleVerified:true,independentPerformanceValidated:true};
const input={sizing,evidence,auditQuantity:100,scenarios:[{id:'reference',value:'0.600000'},{id:'stress',value:'0.550000'}]};
test('a less favorable positive-edge scenario halves the funded exposure',()=>{
 const r=screen(input);assert.equal(r.scenarios[0].fundedQuantity,40);assert.equal(r.scenarios[1].fundedQuantity,20);assert.equal(r.stressFundedQuantity,20);
 assert.equal(r.auditedPrincipal,'50.000000');assert.equal(r.scenarios[1].conditionalExpectedSettlementSurplus,'5.000000');
 assert.equal(r.maximumSettlementLoss,'50.000000');assert.equal(r.allWinSettlementSurplus,'50.000000');assert.equal(r.executionAuthority,false);
});
test('a plausible losing settlement scenario blocks extra exposure despite a positive point edge',()=>{
 const r=screen({...input,scenarios:[input.scenarios[0],{id:'adverse',value:'0.400000'}]});
 assert.equal(r.stressFundedQuantity,0);assert.equal(r.scenarios[1].conditionalExpectedSettlementSurplus,'-10.000000');assert.equal(r.harvestExitProfitInferred,false);
});
test('existing positions and pending purchases are counted once across every scenario',()=>{
 const r=screen({...input,sizing:{...sizing,currentQuantity:5,reservedBuyQuantity:7}});
 assert.equal(r.scenarios[0].fundedQuantity,28);assert.equal(r.scenarios[1].fundedQuantity,8);
});
test('cash, event principal and observed executable depth constrain capital',()=>{
 assert.equal(screen({...input,sizing:{...sizing,cashAvailable:'1.000000'}}).stressFundedQuantity,2);
 assert.equal(screen({...input,sizing:{...sizing,eventPrincipal:'100.000000'}}).stressFundedQuantity,0);
 assert.equal(screen({...input,sizing:{...sizing,depthQuantity:3}}).stressFundedQuantity,3);
});
test('stale or future books preserve diagnostic economics but cannot qualify fresh sizing',()=>{
 for(const bookAt of ['2026-10-12T06:00:00.000Z','2026-10-12T07:00:01.000Z']){
 const r=screen({...input,evidence:{...evidence,bookAt}});assert.equal(r.stressFundedQuantity,20);assert.equal(r.evidenceQualifiedShadowQuantity,0);assert.ok(r.evidenceBlockers.includes('NO_FRESH_EXECUTABLE_BOOK'));}
});
test('missing source, transport, fee and economic validation remain explicit blockers',()=>{
 const r=screen({...input,evidence:{...evidence,sourceCensusComplete:false,populationTransportValidated:false,feeScheduleVerified:false,independentPerformanceValidated:false}});
 assert.equal(r.evidenceQualifiedShadowQuantity,0);assert.equal(r.evidenceBlockers.length,4);
});
test('scenario identity/value errors and nondecimal money fail rather than invent probabilities',()=>{
 assert.throws(()=>screen({...input,scenarios:[]}),/SCENARIOS_REQUIRED/);
 assert.throws(()=>screen({...input,scenarios:[input.scenarios[0],input.scenarios[0]]}),/SCENARIO_ID/);
 assert.throws(()=>screen({...input,scenarios:[{id:'invalid',value:'1.000001'}]}),/SCENARIO_VALUE/);
 assert.throws(()=>screen({...input,scenarios:[{id:'float',value:0.6}]}),/canonical decimal/);
});
