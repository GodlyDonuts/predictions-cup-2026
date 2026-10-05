import test from 'node:test';import assert from 'node:assert/strict';
import {aggregateRacePolls,chronologicalPredictions,fitSinglePollError,probability} from './model.mjs';
const definition={forecastHorizonDays:45,maximumPollAgeDays:30,recencyHalfLifeDays:21,minimumTrainingRaces:3,minimumCalibrationCycles:3,minimumMarginSdPoints:3,families:['poll_centered','historical_bias_corrected'],diagnosticYears:[2020]};
const obs=(year,id,margin,error=0)=>({id,key:year+':KS',year,state:'KS',pollsterId:id,fieldAt:Date.parse(year+'-09-15T12:00:00Z'),electionAt:Date.parse(year+'-11-01T12:00:00Z'),horizonDays:47,pollMargin:margin,actualMargin:margin+error,democraticWon:margin+error>0,remaining:10});
test('One latest observation per pollster; duplicate versions cannot gain weight',()=>{
 const a=obs(2018,'1',5),b={...a,id:'2',fieldAt:a.fieldAt-86400000},c={...a,id:'3',pollsterId:'9',pollMargin:-5};
 const expected=aggregateRacePolls([a,c],definition);assert.deepEqual(aggregateRacePolls([a,b,c],definition),expected);assert.equal(expected[0].pollCount,2);assert.equal(expected[0].pollMargin,0);
});
test('Forecast cutoff rejects a poll fielded after the historical cutoff',()=>{const old=obs(2018,'1',5),future={...old,id:'2',pollsterId:'2',horizonDays:44};assert.equal(aggregateRacePolls([old,future],definition)[0].pollCount,1);});
test('Changing later labels cannot change an earlier calibration',()=>{
 const rows=[2014,2016,2018,2020,2022].map((year,i)=>({key:year+':KS',year,state:'KS',pollMargin:2,error:i,democraticWon:true}));
 const original=chronologicalPredictions(rows,definition);const perturbed=chronologicalPredictions(rows.map(r=>r.year===2022?{...r,error:1e9}:r),definition);
 assert.deepEqual(original,perturbed);assert(original.every(r=>r.trainedThroughYear<r.year));
});
test('Single-poll dispersion survives cancellation in the race average',()=>{
 const rows=[2014,2016,2018].flatMap(year=>[obs(year,'1',-10,10),obs(year,'2',10,-10)]);
 const model=fitSinglePollError(rows,definition);assert.equal(model.biasPoints,0);assert.equal(model.standardDeviationPoints,10);assert.equal(model.races,3);assert.equal(model.pollsterRaceObservations,6);
 assert.equal(probability(model,0),.5);assert(probability(model,5)>.5);
});
