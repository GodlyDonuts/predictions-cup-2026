import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {MICROPRICE_FEATURES} from './realtime-microprice-dataset.mjs';
import {quoteOutcomeForecast} from './quote-outcome-forecast.mjs';

const legacyReport=JSON.parse(fs.readFileSync('data/realtime-quote-selectors-20261005.json','utf8'));
const model=(outputs)=>({normalizedClip:12,means:Array(MICROPRICE_FEATURES.length).fill(0),scales:Array(MICROPRICE_FEATURES.length).fill(1),coefficients:Array.from({length:MICROPRICE_FEATURES.length+1},()=>Array(outputs).fill(0))});
function currentFixture(){return{report:{mode:'quote_fill_conditional_selector_research',features:[...MICROPRICE_FEATURES]},bank:{status:'research_selector_frozen',policy:{feeTicks:0,latencyMs:3000},selector:{minimumExpectedNetTicks:0,minimumAnyFillProbability:0},model:{arrival:model(2),conditionalValue:model(1)}},x:Array(MICROPRICE_FEATURES.length).fill(0)};}

test('legacy quote-selector banks fail closed after trade-flow feature semantics change',()=>{
 assert(legacyReport.features.includes('signedFlow30s'));
 assert(MICROPRICE_FEATURES.includes('aggressorFlow30sUnavailable'));
 assert.throws(()=>quoteOutcomeForecast(legacyReport,legacyReport.banks[0],Array(MICROPRICE_FEATURES.length).fill(0)),/QUOTE_FORECAST_BINDING/);
});

test('schema-compatible selector still validates thresholds, model shape and feature binding',()=>{
 const {report,bank,x}=currentFixture();
 const prediction=quoteOutcomeForecast(report,bank,x);
 assert.equal(prediction.expectedNetTicks,0);
 assert.equal(prediction.executionAuthority,false);
 assert.equal(prediction.profitabilityEstablished,false);
 for(const bad of [NaN,Infinity,-1,1.1]){const b=structuredClone(bank);b.selector.minimumAnyFillProbability=bad;assert.throws(()=>quoteOutcomeForecast(report,b,x),/QUOTE_FORECAST_SELECTOR/);}
 for(const bad of [NaN,Infinity,-.1]){const b=structuredClone(bank);b.selector.minimumExpectedNetTicks=bad;assert.throws(()=>quoteOutcomeForecast(report,b,x),/QUOTE_FORECAST_SELECTOR/);}
 const b=structuredClone(bank);b.model.arrival.scales[0]=0;assert.throws(()=>quoteOutcomeForecast(report,b,x),/QUOTE_FORECAST_MODEL/);
 assert.throws(()=>quoteOutcomeForecast({...report,features:[]},bank,x),/QUOTE_FORECAST_BINDING/);
});
