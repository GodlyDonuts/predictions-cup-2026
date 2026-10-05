import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
import {parseCsv,hash} from '../election-baseline-v1/model.mjs';import {senateRaces} from '../election-baseline-v2/model.mjs';
import {pollObservations,fitSinglePollError,probability} from './model.mjs';import {digest} from '../../live-paper/model.mjs';import {payloadHash} from '../../execution/ledger.mjs';
import {validateResearch} from '../../live-paper/probability-paper.mjs';
const base='research/poll-calibration-v1',runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),dir=path.join(runtime,'poll-calibration-sources-20261005');
const definition=JSON.parse(fs.readFileSync(base+'/definition.json')),sources=JSON.parse(fs.readFileSync(base+'/sources.json'));
for(const s of sources.captures)assert.equal(hash(fs.readFileSync(path.join(dir,s.file))),s.sha256);
const senators=fs.readFileSync('research/election-baseline-v1/sources/1976-2024-senate-state.csv'),overrides=fs.readFileSync('research/election-baseline-v2/final-stage-overrides.json');
const known=JSON.parse(fs.readFileSync('research/election-baseline-v2/model-summary-affiliation-invariant.json'));
assert.equal(hash(senators),known.sourceManifests[0].files.find(f=>f.filename==='1976-2024-senate-state.csv').sha256);assert.equal(hash(overrides),known.overridesHash);
const observations=pollObservations(parseCsv(fs.readFileSync(path.join(dir,'raw_polls.csv'),'utf8')),senateRaces(parseCsv(senators.toString('utf8')),JSON.parse(overrides))).rows;
const model=fitSinglePollError(observations,definition),jackknife=model.cycles.map(cycle=>fitSinglePollError(observations,definition,cycle));
const sourceRecord=JSON.parse(fs.readFileSync('research/current-polls-v1/assessments-20261005-v1.json')),candidate=sourceRecord.assessments.find(a=>a.state==='KS'&&a.party==='democratic');
const poll=candidate.poll,fieldAge=(Date.now()-Date.parse(poll.fieldEndLatestAt))/86400000;assert(poll.population==='likely_voters'&&poll.ballotVerified&&fieldAge<=30,'current_poll_scope_or_age');
const source=candidate.sources.find(s=>s.file==='kansas-emerson.html');assert.equal(hash(fs.readFileSync(path.join(runtime,'research-sources-current-polls-20261005',source.file))),source.contentHash);
const root=JSON.parse(fs.readFileSync(path.join(runtime,'production-metadata','current-poll-post-assessment-20261005-v1','356.json')));
assert(root.rootHash===payloadHash(root.root)&&root.root.contract_details.usState==='KS'&&root.root.contract_details.winnerName==='Democratic Party'&&root.root.contract_details.resolutionType==='Party Winner','contract_scope');
const margin=100*(poll.democratic-poll.republican),estimates=[model,...jackknife].map(m=>({model:m,probability:probability(m,margin)}));
const now=new Date().toISOString(),haircut=.02,lower=Math.max(0,Math.min(...estimates.map(e=>e.probability))-haircut),upper=Math.max(lower,Math.max(...estimates.map(e=>e.probability))-haircut);
const research={schemaVersion:1,id:'kansas-democratic-empirical-poll-20261005-v1',marketId:'356',exchangeId:'1045',side:'YES',predicateHash:root.rootHash,recordedAt:now,sourceAt:now,expiresAt:new Date(Date.parse(now)+21600000).toISOString(),
 lower:lower.toFixed(6),upper:upper.toFixed(6),method:'subjective_range',settlementReviewed:true,flags:[],sources:[...candidate.sources.map(s=>({url:s.url,contentHash:s.contentHash})),{url:sources.captures[0].url,contentHash:sources.captures[0].sha256}]};
validateResearch(research);
const assessment={schemaVersion:1,id:'kansas-single-poll-error-assessment-20261005-v1',at:now,modelCodeHash:hash(fs.readFileSync(base+'/model.mjs')),definitionHash:hash(fs.readFileSync(base+'/definition.json')),model,estimates,
 poll,marginPoints:margin,componentClocks:{fieldEndDate:poll.fieldEndDate,originalPollObservedAt:poll.observedAt,modelTrainedThroughYear:model.trainedThroughYear,rawArchiveObservedAt:sources.captures[0].observedAt,assessmentAt:now},
 research,rootHash:root.rootHash,ordinaryRange:[lower,upper],turnoutOrPollingShiftStress:[-3,0,3].map(shift=>({shiftPoints:shift,valuation:Math.max(0,probability(model,margin+shift)-haircut)})),
 limitations:['Cycle-deletion parameter sensitivity is not a confidence bound. A fixed2-point haircut is a judgment reserve.','Historical individual-poll errors give one race equal weight and include within-race poll dispersion.','The archive lacks population labels and original release times; specific likely-voter transport is not empirically established.','Current Kansas evidence is one September15–16 poll, not a fresh polling consensus.','Expected settlement value does not predict a profitable short-term exit or guarantee a positive paper return.','Third-party/undecided dynamics and turnout shocks remain possible; the stress family may contradict the ordinary estimate.'],
 executionAuthority:false,competitionOrders:0,profitabilityEstablished:false};
const bundle={identities:{356:{exchangeId:'1045',predicateHash:root.rootHash}},research:[research],researchEvidence:[assessment]};
fs.writeFileSync(base+'/kansas-paper-assessment-20261005-v1.json',JSON.stringify(assessment,null,2)+'\n',{flag:'wx'});
fs.writeFileSync(base+'/kansas-paper-bundle-20261005-v1.json',JSON.stringify(bundle,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({at:now,modelRaces:model.races,individualPolls:model.pollsterRaceObservations,errorSdPoints:model.standardDeviationPoints,errorBiasPoints:model.biasPoints,ordinary:[lower,upper],stresses:assessment.turnoutOrPollingShiftStress,marketId:'356',productionBankChanged:false,competitionOrders:0}));
