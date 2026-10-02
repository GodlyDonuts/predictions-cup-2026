import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {ageExecutionPerformance} from './app/performance-freshness.mjs';
import {executionPerformanceView} from './app/execution-view.mjs';

const now=Date.parse('2026-10-02T12:14:21.000Z'),at=ms=>new Date(now+ms).toISOString();
const execution={runId:'fixture',positions:{},fills:[]};
const hash=createHash('sha256').update(JSON.stringify(execution)).digest('hex');
const report=changes=>({mode:'official_execution_performance',status:'running',runId:'fixture',asOf:at(0),updatedAt:at(0),addedFees:'0.000000',positions:[{book:{receivedAt:at(-11000)}}],executionLedgerHash:hash,realizedPnl:'0.000000',ownedCost:'3.920000',observedLiquidationValue:'3.760000',observedLiquidationUnrealizedPnl:'-0.160000',observedLiquidationTotalPnl:'-0.160000',midpointValue:'3.800000',midpointUnrealizedPnl:'-0.120000',midpointTotalPnl:'-0.120000',...changes});
const marks=['observedLiquidationValue','observedLiquidationUnrealizedPnl','observedLiquidationTotalPnl','midpointValue','midpointUnrealizedPnl','midpointTotalPnl'];

test('a formerly fresh response ages out without another fetch or rewritten evidence clocks',()=>{
 const input=executionPerformanceView(report(),execution,now),copy=structuredClone(input);
 const fresh=ageExecutionPerformance(input,now);assert.equal(fresh.available,true);assert.equal(fresh.observedLiquidationUnrealizedPnl,'-0.160000');
 const aged=ageExecutionPerformance(input,now+65000);assert.equal(aged.available,false);assert.equal(aged.quotesFresh,false);
 for(const key of marks)assert.equal(aged[key],null,key);
 assert.equal(aged.ownedCost,'3.920000');assert.equal(aged.asOf,at(0));assert.equal(aged.updatedAt,at(0));assert.equal(aged.quoteAt,at(-11000));assert.deepEqual(input,copy);
});

test('quote expires independently while the report is still within its ten-second boundary',()=>{
 const input=executionPerformanceView(report(),execution,now);
 assert.equal(ageExecutionPerformance(input,now+9000).observedLiquidationUnrealizedPnl,'-0.160000');
 const result=ageExecutionPerformance(input,now+9001);assert.equal(result.available,true);assert.equal(result.quotesFresh,false);
 for(const key of marks)assert.equal(result[key],null,key);assert.equal(result.ownedCost,'3.920000');
});

test('report and worker timestamp bounds independently match the server view at exact boundaries',()=>{
 const input=executionPerformanceView(report({positions:[{book:{receivedAt:at(0)}}]}),execution,now);
 assert.equal(ageExecutionPerformance(input,now+10000).available,true);assert.equal(ageExecutionPerformance(input,now+10001).available,false);
 for(const key of ['asOf','updatedAt'])for(const value of [at(-10001),at(1),'bad',null]){
  const result=ageExecutionPerformance({...input,[key]:value},now);assert.equal(result.available,false);assert.equal(result.midpointUnrealizedPnl,null);
 }
});

test('missing, future, or malformed open-position quote clocks never display a mark',()=>{
 const input=executionPerformanceView(report(),execution,now);
 for(const quoteAt of [undefined,null,'bad',at(1)]){
  const result=ageExecutionPerformance({...input,quoteAt},now);assert.equal(result.available,true);assert.equal(result.quotesFresh,false);assert.equal(result.observedLiquidationUnrealizedPnl,null);assert.equal(result.midpointUnrealizedPnl,null);
 }
});

test('server-rejected or depth-unavailable projections cannot be rehabilitated by a client repaint',()=>{
 const rejected=ageExecutionPerformance({available:false,asOf:at(0),updatedAt:at(0),quoteAt:at(0),midpointUnrealizedPnl:'0.500000'},now);
 assert.equal(rejected.available,false);assert.equal(rejected.midpointUnrealizedPnl,null);
 const input=executionPerformanceView(report({observedLiquidationValue:null,observedLiquidationUnrealizedPnl:null,observedLiquidationTotalPnl:null}),execution,now);
 const result=ageExecutionPerformance(input,now);assert.equal(result.available,true);assert.equal(result.observedLiquidationUnrealizedPnl,null);assert.equal(result.midpointUnrealizedPnl,'-0.120000');
 assert.equal(ageExecutionPerformance(undefined,now).available,false);assert.equal(ageExecutionPerformance(input,NaN).available,false);
});

test('fresh verified flat holdings need no quote but their report still ages out',()=>{
 const values=Object.fromEntries(['ownedCost',...marks].map(key=>[key,'0.000000']));
 const input=executionPerformanceView(report({...values,positions:[]}),execution,now);
 const result=ageExecutionPerformance(input,now);assert.equal(result.available,true);assert.equal(result.quoteAt,null);assert.equal(result.observedLiquidationUnrealizedPnl,'0.000000');
 assert.equal(ageExecutionPerformance(input,now+10001).observedLiquidationUnrealizedPnl,null);
});
