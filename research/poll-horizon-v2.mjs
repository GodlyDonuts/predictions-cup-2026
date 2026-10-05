import assert from 'node:assert/strict';
import {aggregateRacePolls} from './poll-calibration-v1/model.mjs';
import {probabilityFromMargin} from './election-baseline-v1/model.mjs';
export const DEFINITION=Object.freeze({forecastHorizonDays:29,maximumPollAgeDays:30,recencyHalfLifeDays:21,minimumRaces:30,minimumCycles:3,minimumSd:3});
export function fitHorizonError(observations,throughYear){
 const races=aggregateRacePolls(observations.filter(p=>p.year<=throughYear),DEFINITION);assert(races.length>=DEFINITION.minimumRaces);
 const cycles=new Map();for(const r of races){const a=cycles.get(r.year)||[];a.push(r);cycles.set(r.year,a);}assert(cycles.size>=DEFINITION.minimumCycles);
 const bias=races.reduce((s,r)=>s+r.error,0)/races.length,cycleRows=[...cycles].map(([year,rows])=>({year,races:rows.length,mean:rows.reduce((s,r)=>s+r.error,0)/rows.length}));
 const rawVariance=races.reduce((s,r)=>s+(r.error-bias)**2,0)/races.length;
 let within=0;for(const c of cycleRows)for(const r of cycles.get(c.year))within+=(r.error-c.mean)**2/races.length;
 const cycleCenter=cycleRows.reduce((s,c)=>s+c.mean,0)/cycleRows.length,meanCycleVariance=cycleRows.reduce((s,c)=>s+(c.mean-cycleCenter)**2,0)/cycleRows.length;
 const finiteRaceMeanNoise=within*cycleRows.reduce((s,c)=>s+1/c.races,0)/cycleRows.length,common=Math.max(0,meanCycleVariance-finiteRaceMeanNoise);
 // Keep at least the full race-level historical variance; repeated polls and
 // larger source counts cannot divide away a common election shock.
 const total=Math.max(DEFINITION.minimumSd**2,rawVariance,within+common);
 return{schemaVersion:1,definition:DEFINITION,trainedThroughYear:throughYear,maximumTrainingElectionYear:Math.max(...races.map(r=>r.year)),races:races.length,cycles:cycleRows,meanBiasPoints:bias,withinCycleVariance:within,estimatedCommonCycleVariance:common,rawRaceVariance:rawVariance,totalVariance:total,standardDeviationPoints:Math.sqrt(total),commonCorrelationProxy:common/total,historicalReleaseClocksAvailable:false,historicalPopulationLabelsAvailable:false,distribution:'fixed_t4',profitabilityEstablished:false};
}
export function valuePollMargin(model,margin,{shift=0}={}){assert(Number.isFinite(margin)&&Number.isFinite(shift));return probabilityFromMargin(margin+model.meanBiasPoints+shift,model.standardDeviationPoints);}
export function currentKnownMargin(polls,{electionDate,asOf}){
 const election=Date.parse(electionDate+'T12:00:00Z'),clock=Date.parse(asOf),selected=new Map(),excluded=[];assert(Number.isFinite(election)&&Number.isFinite(clock)&&election>clock);
 for(const p of polls){
  if(!p.methodologyVerified||!p.fieldStart||!p.fieldEnd){excluded.push({source:p.source,reason:'fieldwork_or_methodology_unverified'});continue;}
  const from=Date.parse(p.fieldStart+'T12:00:00Z'),to=Date.parse(p.fieldEnd+'T12:00:00Z'),middle=(from+to)/2,horizon=(election-middle)/86400000;
  if(!Number.isFinite(horizon)||from>to||to>clock||horizon<DEFINITION.forecastHorizonDays||horizon>DEFINITION.forecastHorizonDays+DEFINITION.maximumPollAgeDays||!Number.isFinite(p.margin)){excluded.push({source:p.source,reason:'field_clock_or_horizon'});continue;}
  const old=selected.get(p.source);if(!old||middle>old.middle)selected.set(p.source,{...p,middle,horizon,weight:2**(-(horizon-DEFINITION.forecastHorizonDays)/DEFINITION.recencyHalfLifeDays)});
 }
 assert(selected.size>0);const rows=[...selected.values()],weight=rows.reduce((s,r)=>s+r.weight,0);return{margin:rows.reduce((s,r)=>s+r.margin*r.weight/weight,0),sources:rows.map(r=>({source:r.source,horizonDays:r.horizon,weight:r.weight/weight})),excluded,precisionGainAssumed:false};
}
export function chronologicalHorizonDiagnostic(observations,years=[2020,2022]){
 const rows=[];for(const year of years){const model=fitHorizonError(observations,year-1);assert(model.maximumTrainingElectionYear<year);for(const race of aggregateRacePolls(observations.filter(r=>r.year===year),DEFINITION)){rows.push({key:race.key,year,pollMargin:race.pollMargin,democraticWon:race.democraticWon,probability:valuePollMargin(model,race.pollMargin),trainingThroughYear:model.maximumTrainingElectionYear});}}
 return{races:rows.length,years,brier:rows.reduce((s,r)=>s+(r.probability-Number(r.democraticWon))**2,0)/rows.length,byYear:years.map(year=>{const rs=rows.filter(r=>r.year===year);return{year,races:rs.length,brier:rs.reduce((s,r)=>s+(r.probability-Number(r.democraticWon))**2,0)/rs.length};}),rows,untouchedOriginalForecastVintage:false,marketPnlAvailable:false,profitabilityEstablished:false};
}
