import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {parseCsv} from '../election-baseline-v1/model.mjs';import {senateRaces} from '../election-baseline-v2/model.mjs';
import {pollObservations,aggregateRacePolls,fitPollError,probability,score} from '../poll-calibration-v1/model.mjs';
const sha=b=>createHash('sha256').update(b).digest('hex');
const raw=fs.readFileSync(path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime/poll-calibration-sources-20261005/raw_polls.csv'));
if(sha(raw)!==JSON.parse(fs.readFileSync('research/poll-calibration-v1/sources.json')).captures.find(x=>x.file==='raw_polls.csv').sha256)throw Error('POLL_SOURCE_HASH');
const outcome=fs.readFileSync('research/election-baseline-v1/sources/1976-2024-senate-state.csv');
if(sha(outcome)!==JSON.parse(fs.readFileSync('research/election-baseline-v1/sources/PEJ5QU-manifest.json')).files.find(x=>x.filename==='1976-2024-senate-state.csv').sha256)throw Error('OUTCOME_SOURCE_HASH');
const races=senateRaces(parseCsv(outcome.toString()),JSON.parse(fs.readFileSync('research/election-baseline-v2/final-stage-overrides.json')));
const observations=pollObservations(parseCsv(raw.toString()),races).rows;
const source=JSON.parse(fs.readFileSync('research/maine-primary-screen-20261012/sources.json'));
const asOf=new Date().toISOString(),rows=[];
for(const halfLife of [7,14,28]){
 const definition={forecastHorizonDays:22,maximumPollAgeDays:30,recencyHalfLifeDays:halfLife};
 const historical=aggregateRacePolls(observations,definition),predictions=[];
 for(const year of [2016,2018,2020,2022]){
  const model=fitPollError(historical.filter(r=>r.year<year),{family:'historical_bias_corrected'});
  for(const r of historical.filter(r=>r.year===year))predictions.push({...r,democraticWinProbability:probability(model,r.pollMargin),trainedThroughYear:model.trainedThroughYear});
 }
 const model=fitPollError(historical,{family:'historical_bias_corrected'});
 const ps=source.captures.map(p=>({...p,weight:2**(-((Date.parse(asOf)-Date.parse(p.fieldEnd+'T23:59:59.999Z'))/86400000)/halfLife)}));
 const total=ps.reduce((n,p)=>n+p.weight,0),margin=ps.reduce((n,p)=>n+p.weight*p.democraticMargin,0)/total;
 rows.push({halfLifeDays:halfLife,definition,model,diagnostic:score(predictions),chronologicalEarlierYearTrainingVerified:predictions.every(p=>p.trainedThroughYear<p.year),currentWeightedDemocraticMargin:margin,currentRepublicanNoCentralValue:probability(model,margin)-.02,currentThreePointAdverseValue:probability(model,margin-3)-.02});
}
const result={at:asOf,pollSourceHash:sha(raw),outcomeSourceHash:sha(outcome),rows,recencyFamilySelected:false,revisedHistoricalArchive:true,historicalPublicationTimesUnknown:true,rvLvTransportUnvalidated:true,currentSourceCensusIncomplete:true,winProbabilitiesAreNotShortTermPriceForecasts:true,actualOrders:0,historicalMarketPnlAvailable:false,profitabilityEstablished:false};
fs.writeFileSync('research/maine-primary-screen-20261012/average-calibration.json',JSON.stringify(result,null,2));
console.log(JSON.stringify(rows.map(r=>({halfLife:r.halfLifeDays,trainingRaces:r.model.races,diagnosticRaces:r.diagnostic.races,brier:r.diagnostic.brier,centralValue:r.currentRepublicanNoCentralValue,adverseValue:r.currentThreePointAdverseValue}))));
