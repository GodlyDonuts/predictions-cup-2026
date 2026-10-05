import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import {quoteOutcomeForecast} from './quote-outcome-forecast.mjs';
const modelFile='data/realtime-quote-selectors-20261005.json',report=JSON.parse(fs.readFileSync(modelFile)),bank=report.banks[0],x=bank.model.arrival.means;
test('actual Python predictions match the JS replay selector across both frozen cost cases',()=>{
 const cases=JSON.parse(fs.readFileSync('data/quote-outcome-parity-cases-20261005.json'));let maximumError=0;
 for(const c of cases){const r=JSON.parse(fs.readFileSync(c.modelFile)),p=quoteOutcomeForecast(r,r.banks[c.bankIndex],c.x),actual=[p.anyFillProbability,p.pairedProbability,p.conditionalNetTicks,p.expectedNetTicks];for(let i=0;i<4;i++){maximumError=Math.max(maximumError,Math.abs(actual[i]-c.expected[i]));assert(Math.abs(actual[i]-c.expected[i])<1e-10);}assert.equal(p.executionAuthority,false);}
 assert.equal(cases.length,64);fs.writeFileSync('data/quote-outcome-parity-20261005.json',JSON.stringify({cases:cases.length,maximumError,executionAuthority:false,profitabilityEstablished:false},null,2)+'\n');
});
test('malformed selector thresholds, normalization and causal feature bindings fail closed',()=>{
 for(const bad of[NaN,Infinity,-1,1.1]){const b=structuredClone(bank);b.selector.minimumAnyFillProbability=bad;assert.throws(()=>quoteOutcomeForecast(report,b,x),/SELECTOR/);}
 for(const bad of[NaN,Infinity,-.1]){const b=structuredClone(bank);b.selector.minimumExpectedNetTicks=bad;assert.throws(()=>quoteOutcomeForecast(report,b,x),/SELECTOR/);}
 const b=structuredClone(bank);b.model.arrival.scales[0]=0;assert.throws(()=>quoteOutcomeForecast(report,b,x),/MODEL/);assert.throws(()=>quoteOutcomeForecast({...report,features:[]},bank,x),/BINDING/);
});
