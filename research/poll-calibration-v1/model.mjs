import assert from 'node:assert/strict';
import {identity,probabilityFromMargin} from '../election-baseline-v1/model.mjs';
const day=86400000;
const name=n=>identity(n).split(' ').filter(t=>t.length>1).join(' ');
const date=s=>/^\d{4}-\d{2}-\d{2}$/.test(s)?Date.parse(s+'T12:00:00Z'):NaN;
export function pollObservations(raw,senate){
 const rows=[],excluded={},seen=new Map();const reject=reason=>excluded[reason]=(excluded[reason]||0)+1;
 for(const p of raw){
  if(p.type_simple!=='Sen-G')continue;
  const year=Number(p.cycle),key=year+':'+p.location,race=senate.races.get(key);
  if(!Number.isSafeInteger(year)||year%2||!race||race.selectedStage!=='gen'||!race.opponent.parties.includes('DEMOCRAT')){reject('not_regular_comparable_major_party_final');continue;}
  if(p.cand1_party!=='DEM'||p.cand2_party!=='REP'||name(p.cand1_name)!==name(race.opponent.name)||name(p.cand2_name)!==name(race.republican.name)){reject('final_candidate_identity_unmatched');continue;}
  if(p.partisan&&p.partisan!=='NA'){reject('partisan_or_internal');continue;}
  const fieldAt=date(p.polldate),electionAt=date(p.electiondate),horizon=(electionAt-fieldAt)/day;
  if(!Number.isFinite(horizon)||horizon<=0||!p.electiondate.startsWith(year+'-11-')||Math.abs(horizon-Number(p.time_to_election))>1){reject('election_or_field_clock_invalid');continue;}
  const d=Number(p.cand1_pct),r=Number(p.cand2_pct),actualD=Number(p.cand1_actual),actualR=Number(p.cand2_actual);
  if(![d,r,actualD,actualR].every(x=>Number.isFinite(x)&&x>0&&x<100)||d+r>100.001){reject('invalid_vote_shares');continue;}
  const verifiedD=100*race.opponent.votes/race.total,verifiedR=100*race.republican.votes/race.total;
  if(Math.abs(actualD-verifiedD)>.25||Math.abs(actualR-verifiedR)>.25||((actualD>actualR)!==!race.winner.republican)){reject('independent_final_outcome_disagrees');continue;}
  if(!/^\d+$/.test(p.question_id)||!/^\d+$/.test(p.poll_id)||!/^\d+$/.test(p.pollster_rating_id)){reject('source_identity_missing');continue;}
  const id=p.race_id+':'+p.question_id,record={id,key,year,state:p.location,pollId:p.poll_id,questionId:p.question_id,pollsterId:p.pollster_rating_id,pollster:p.pollster,
   fieldDate:p.polldate,fieldAt,electionDate:p.electiondate,electionAt,horizonDays:horizon,democratic:d,republican:r,remaining:100-d-r,pollMargin:d-r,
   actualMargin:verifiedD-verifiedR,democraticWon:!race.winner.republican,population:'not_in_archive',sourceFinalMargin:actualD-actualR};
  if(seen.has(id)){assert.deepEqual(record,seen.get(id),'conflicting_source_question');reject('identical_duplicate');continue;}
  seen.set(id,record);rows.push(record);
 }
 return {rows:rows.sort((a,b)=>a.year-b.year||a.key.localeCompare(b.key)||a.id.localeCompare(b.id)),excluded};
}
export function aggregateRacePolls(observations,definition){
 assert(Number.isFinite(definition.forecastHorizonDays)&&definition.forecastHorizonDays>0&&Number.isFinite(definition.maximumPollAgeDays)&&definition.maximumPollAgeDays>0&&Number.isFinite(definition.recencyHalfLifeDays)&&definition.recencyHalfLifeDays>0);
 const grouped=new Map();
 for(const p of observations){
  if(p.horizonDays<definition.forecastHorizonDays||p.horizonDays>definition.forecastHorizonDays+definition.maximumPollAgeDays)continue;
  if(!grouped.has(p.key))grouped.set(p.key,new Map());
  const pollsters=grouped.get(p.key),old=pollsters.get(p.pollsterId);
  if(!old||p.fieldAt>old.fieldAt||p.fieldAt===old.fieldAt&&p.id.localeCompare(old.id)<0)pollsters.set(p.pollsterId,p);
 }
 return [...grouped].map(([key,map])=>{
  const polls=[...map.values()],first=polls[0];
  assert(polls.every(p=>p.year===first.year&&p.electionAt===first.electionAt&&Math.abs(p.actualMargin-first.actualMargin)<1e-9&&p.democraticWon===first.democraticWon),'race_outcome_disagreement');
  const weights=polls.map(p=>2**(-(p.horizonDays-definition.forecastHorizonDays)/definition.recencyHalfLifeDays)),sum=weights.reduce((a,b)=>a+b,0);
  const pollMargin=polls.reduce((s,p,i)=>s+p.pollMargin*weights[i]/sum,0);
  return {key,year:first.year,state:first.state,forecastHorizonDays:definition.forecastHorizonDays,pollMargin,actualMargin:first.actualMargin,democraticWon:first.democraticWon,
   error:first.actualMargin-pollMargin,pollCount:polls.length,pollIds:polls.map(p=>p.id),latestFieldDate:new Date(Math.max(...polls.map(p=>p.fieldAt))).toISOString(),
   actualHistoricalReleaseTimesAvailable:false,populationsAvailable:false,remaining:polls.reduce((s,p,i)=>s+p.remaining*weights[i]/sum,0)};
 }).sort((a,b)=>a.year-b.year||a.key.localeCompare(b.key));
}
export function fitPollError(races,{family,minimumRaces=30,minimumCycles=3,minimumSd=3}){
 assert(['poll_centered','historical_bias_corrected'].includes(family),'unknown_family');
 assert(races.length>=minimumRaces&&new Set(races.map(r=>r.year)).size>=minimumCycles,'insufficient_independent_history');
 assert(races.every(r=>Number.isFinite(r.error)),'invalid_error');
 const bias=family==='historical_bias_corrected'?races.reduce((sum,r)=>sum+r.error,0)/races.length:0;
 const sd=Math.max(minimumSd,Math.sqrt(races.reduce((sum,r)=>sum+(r.error-bias)**2,0)/races.length));
 return {family,races:races.length,cycles:[...new Set(races.map(r=>r.year))],trainedThroughYear:Math.max(...races.map(r=>r.year)),biasPoints:bias,standardDeviationPoints:sd,distribution:'fixed_t4',effectiveUnit:'one_state_race',historicalPopulationTransportMeasured:false};
}
export function probability(model,pollMargin){
 assert(Number.isFinite(pollMargin),'invalid_poll_margin');
 return probabilityFromMargin(pollMargin+model.biasPoints,model.standardDeviationPoints);
}
// A current single poll must use individual-poll uncertainty, not the smaller
// error of a historical average. Each race retains total weight one.
export function fitSinglePollError(observations,definition,omitCycle=null){
 const groups=new Map();
 for(const p of observations){if(p.year===omitCycle||p.horizonDays<definition.forecastHorizonDays||p.horizonDays>definition.forecastHorizonDays+definition.maximumPollAgeDays)continue;
  if(!groups.has(p.key))groups.set(p.key,new Map());const map=groups.get(p.key),old=map.get(p.pollsterId);
  if(!old||p.fieldAt>old.fieldAt||p.fieldAt===old.fieldAt&&p.id.localeCompare(old.id)<0)map.set(p.pollsterId,p);
 }
 assert(groups.size>=definition.minimumTrainingRaces,'insufficient_single_poll_history');
 const races=[...groups.values()].map(m=>[...m.values()]),cycles=[...new Set(races.map(ps=>ps[0].year))];assert(cycles.length>=definition.minimumCalibrationCycles,'insufficient_single_poll_cycles');
 const average=f=>races.reduce((sum,ps)=>sum+ps.reduce((s,p)=>s+f(p),0)/ps.length,0)/races.length;
 const bias=average(p=>p.actualMargin-p.pollMargin),sd=Math.max(definition.minimumMarginSdPoints,Math.sqrt(average(p=>(p.actualMargin-p.pollMargin-bias)**2)));
 return {family:'single_poll_race_weighted',races:races.length,pollsterRaceObservations:races.reduce((s,ps)=>s+ps.length,0),cycles,trainedThroughYear:Math.max(...cycles),biasPoints:bias,standardDeviationPoints:sd,distribution:'fixed_t4',effectiveUnit:'equal_race_weight_with_individual_poll_dispersion',omittedCycle:omitCycle,historicalPopulationTransportMeasured:false};
}
export function chronologicalPredictions(races,definition){
 const out=[];
 for(const year of definition.diagnosticYears){
  const training=races.filter(r=>r.year<year),testing=races.filter(r=>r.year===year);
  for(const family of definition.families){
   const model=fitPollError(training,{family,minimumRaces:definition.minimumTrainingRaces,minimumCycles:definition.minimumCalibrationCycles,minimumSd:definition.minimumMarginSdPoints});
   assert(model.trainedThroughYear<year,'future_calibration_leak');
   for(const r of testing)out.push({...r,family,trainedThroughYear:model.trainedThroughYear,model,democraticWinProbability:probability(model,r.pollMargin)});
  }
 }
 return out;
}
export function score(rows){
 assert(rows.length>0,'no_predictions');
 const n=rows.length,prob=r=>r.democraticWinProbability,y=r=>Number(r.democraticWon);
 const brier=rows.reduce((s,r)=>s+(prob(r)-y(r))**2,0)/n,logLoss=rows.reduce((s,r)=>s-y(r)*Math.log(prob(r))-(1-y(r))*Math.log(1-prob(r)),0)/n;
 return {races:n,cycles:[...new Set(rows.map(r=>r.year))],brier,logLoss,correctDirections:rows.filter(r=>(prob(r)>.5)===r.democraticWon).length,
  bins:[[0,.2],[.2,.4],[.4,.6],[.6,.8],[.8,1.000001]].map(([low,high])=>{const group=rows.filter(r=>prob(r)>=low&&prob(r)<high);return{low,high,n:group.length,meanForecast:group.length?group.reduce((s,r)=>s+prob(r),0)/group.length:null,observedFrequency:group.length?group.reduce((s,r)=>s+y(r),0)/group.length:null};}),
  independentElectionCycles:new Set(rows.map(r=>r.year)).size,untouchedOriginalForecastVintage:false,historicalMarketPnlAvailable:false,profitabilityEstablished:false};
}
