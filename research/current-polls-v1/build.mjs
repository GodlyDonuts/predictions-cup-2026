import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {parseCsv,senateRaces,presidentialTables,raceFeatures,predict} from '../election-baseline-v2/model.mjs';
import {assessPartyPoll} from './assessment.mjs';
const base='research/current-polls-v1',runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
const sha=x=>createHash('sha256').update(x).digest('hex');
const manifests=['sources.json','ballot-sources.json'].map(f=>JSON.parse(fs.readFileSync(base+'/'+f,'utf8')));
const captures=manifests.flatMap(m=>m.captures);
for(const s of captures){const dir=s.file==='kansas-official.html'?'research-sources-current-polls-20261005-ballots':'research-sources-current-polls-20261005';if(sha(fs.readFileSync(path.join(runtime,dir,s.file)))!==s.contentHash)throw Error('PRIMARY_SOURCE_CHANGED');}
const source=file=>captures.find(s=>s.file===file);
const model=JSON.parse(fs.readFileSync('research/election-baseline-v2/model-summary-affiliation-invariant.json','utf8'));
const senate=senateRaces(parseCsv(fs.readFileSync('research/election-baseline-v1/sources/1976-2024-senate-state.csv','utf8')),JSON.parse(fs.readFileSync('research/election-baseline-v2/final-stage-overrides.json','utf8')));
const president=presidentialTables(parseCsv(fs.readFileSync('research/election-baseline-v1/sources/1976-2024-president.csv','utf8')));
const definition={...JSON.parse(fs.readFileSync('research/current-election/definition.json','utf8')),id:'causal-two-party-poll-sensitivity-20261005-v1'};
const at=new Date().toISOString(),electionAt='2026-11-04T06:00:00.000Z';
const races=[
 {state:'IA',entrants:['Ashley Hinson','Josh Turek','Thomas Laehn'],parties:{republican:'Ashley Hinson',democratic:'Josh Turek',libertarian:'Thomas Laehn'},pollFile:'iowa-emerson.html',ballotFile:'iowa-official.html',marketIds:{republican:'261',democratic:'260'},poll:{republican:.50,democratic:.45,other:0,unallocated:.05,unallocatedMeaning:'Unspecified remaining support in rounded publisher headline, conservatively allocated; not an observed zero for Laehn or a measured5% undecided result.',fieldStartDate:'2026-08-31',fieldEndDate:'2026-09-01',fieldEndLatestAt:'2026-09-02T12:00:00.000Z',publicationDate:'2026-09-03'}},
 {state:'KS',entrants:['Roger Marshall','Adam Hamilton','David C Graham'],parties:{republican:'Roger Marshall',democratic:'Adam Hamilton',libertarian:'David C Graham'},pollFile:'kansas-emerson.html',ballotFile:'kansas-official.html',marketIds:{democratic:'356'},poll:{republican:.43,democratic:.45,other:.04,unallocated:.08,unallocatedMeaning:'Publisher reports4% Libertarian and8% undecided.',fieldStartDate:'2026-09-15',fieldEndDate:'2026-09-16',fieldEndLatestAt:'2026-09-17T12:00:00.000Z',publicationDate:'2026-09-18'}}
];
const assessments=[];
for(const race of races){
 const features=raceFeatures({year:2026,state:race.state,entrants:race.entrants},senate,president);
 const prior={structuralMargin:predict(model.model,features.x),residualRms:model.residualRms,trainedThroughYear:model.model.trainedThroughYear,modelHash:model.modelHash,features};
 const poll={...race.poll,population:'likely_voters',sampleSize:750,publisherCredibilityInterval:.036,publisherIntervalUsedAsMarginSd:false,observedAt:source(race.pollFile).observedAt,ballotVerified:true,publicationTimeUtc:null,methodology:'Weighted mixed text-to-web/voter-file and panel sample; respondent weights/effective sample size unavailable.'};
 for(const party of ['republican','democratic']){
  const assessment=assessPartyPoll({poll,prior,definition,party,assessmentAt:at,electionAt});
  assessments.push({id:race.state.toLowerCase()+'-'+party+'-poll-assessment-20261005-v1',state:race.state,party,marketId:race.marketIds[party]??null,candidates:race.parties,poll,prior,sources:[source(race.pollFile),source(race.ballotFile)],assessment});
 }
}
const record={schemaVersion:1,id:'current-poll-breadth-20261005-v1',at,definition,definitionHash:sha(JSON.stringify(definition)),assessmentCodeHash:sha(fs.readFileSync(base+'/assessment.mjs')),assessments,
 excludedEvidence:[{source:source('iowa-marist.html'),state:'IA',population:'registered_voters',sampleSize:1050,republican:.42,democratic:.50,remaining:.08,fieldStartDate:'2026-09-17',fieldEndDate:'2026-09-20',publicationDate:'2026-09-22',reason:'Newer contradictory registered-voter survey is retained but not promoted to a likely-voter model without tested population transport.'}],
 nationalContext:{source:source('national-emerson.html'),population:'likely_voters',sampleSize:1000,republican:.42,democratic:.53,undecided:.05,fieldStartDate:'2026-09-21',fieldEndDate:'2026-09-22',publicationDate:'2026-09-24',usedAsNewNumericalInput:false,meaning:'The existing fixed−15/−10/−5 national-margin sensitivity is unchanged. House/Senate transport remains unvalidated.'},
 limitations:['Subjective valuations are uncalibrated; no observed profit or guaranteed payout.','These polls are not independent replications of each other or of the fundamentals.','The Iowa likely-voter poll is older than a conflicting registered-voter survey. This does not establish current polling consensus.','All source field/publication clocks are retained; retrieval time is not fresh fieldwork.','Kansas Republicans versus the strongest opponent cannot be automatically complemented into the separate Democratic Party contract.','No active bank, activation, window, journal or wallet is changed by this offline artifact.'],executionAuthority:false,competitionOrders:0,fees:'0.000000',profitabilityEstablished:false};
fs.writeFileSync(base+'/assessments-20261005-v1.json',JSON.stringify(record,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({at,assessments:assessments.map(a=>({id:a.id,marketId:a.marketId,prior:a.prior.structuralMargin,incumbent:a.prior.features.x.at(-1),ordinary:[a.assessment.ordinary.lower,a.assessment.ordinary.upper],stress:[a.assessment.stress.lower,a.assessment.stress.upper]})),excludedRegisteredVoterEvidence:1,networkRequests:0,productionBankChanged:false}));
