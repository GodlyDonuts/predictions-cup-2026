import fs from 'node:fs';import vm from 'node:vm';import test from 'node:test';import assert from 'node:assert/strict';
const source=fs.readFileSync(new URL('./runner.mjs',import.meta.url),'utf8');
test('actual collector inspection gate denies all writes even when the financial host would authorize them',()=>{
 const start=source.indexOf('authorizeDispatch:spec=>'),end=source.indexOf(',onAdmission:',start),expression=source.slice(start+'authorizeDispatch:'.length,end);assert.ok(start>0&&end>start);
 const authorize=vm.runInNewContext('('+expression+')',{rateReadOnlyInspection:true,pairedLiveHost:{authorizes:()=>true}});assert.equal(authorize({method:'GET'}),true);for(const method of ['POST','DELETE','PATCH'])assert.equal(authorize({method}),false);
});
test('actual collector inspection gate never advances the financial execution host',async()=>{
 const start=source.indexOf('      if(pairedLiveHost){if(!rateReadOnlyInspection)'),end=source.indexOf('\n',start);assert.ok(start>0);let steps=0;const context={rateReadOnlyInspection:true,pairedLiveHost:{step:()=>steps++,status:()=>({phase:'ready'})},pairedCandidate:null,runtime:{}};
 await vm.runInNewContext('(async()=>{'+source.slice(start,end)+'})()',context);assert.equal(steps,0);assert.equal(context.runtime.pairedLive.operatorReadOnlyInspection,true);
});
