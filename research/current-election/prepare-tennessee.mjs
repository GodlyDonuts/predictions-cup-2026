import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {assessElection} from './assessment.mjs';
import {parseCsv,senateRaces,presidentialTables,raceFeatures,predict} from '../election-baseline-v2/model.mjs';
const base='research/current-election',runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
const evidence=JSON.parse(fs.readFileSync('data/probability-research-bundle-20261003-v1.json'));
const model=JSON.parse(fs.readFileSync('research/election-baseline-v2/model-summary-affiliation-invariant.json'));
const definition=JSON.parse(fs.readFileSync(`${base}/definition.json`));
const entrants=['Bill Hagerty','Marquita Bradshaw','Tharon Chandler','Andrew Gerena','Jeremy Dean Hearn','Robert Jones','James William Macon III','Yoshi D. Matthews','David Sutman Jr','Catherine Barcel Whitson'];
const sourceNames=['TN-poll-marginals.pdf','TN-poll-methodology.html','TN-ballot.pdf'];
const sources=sourceNames.map(name=>{const s=evidence.captures.find(c=>c.file===name);const bytes=fs.readFileSync(path.join(runtime,'research-sources-20261003',name));const hash=createHash('sha256').update(bytes).digest('hex');if(hash!==s.contentHash)throw Error('PRIMARY_SOURCE_HASH_CHANGED');return{url:s.url,contentHash:hash,observedAt:s.observedAt};});
const facts={schemaVersion:1,state:'TN',marketId:'363',exchangeId:'1052',entrants,
 poll:{population:'likely_voters',sampleSize:607,overallSurveySampleSize:1200,senateRegisteredVoterResponses:1157,republican:.58,democratic:.33,independent:.05,undecided:.04,
  observedAt:sources[0].observedAt,fieldStartDate:'2026-08-15',fieldEndDate:'2026-08-26',fieldEndLatestAt:'2026-08-27T12:00:00.000Z',
  fieldEndClockMeaning:'Conservative latest UTC bound for the unspecified local time on the final Tennessee field day; not a new poll date.',
  publicationDate:'2026-09-03',publicationTimeUtc:null,pdfGeneratedDate:'2026-08-28',pdfPage:46,
  publisherOverallEstimatedMoe:.0277,publisherMoeUsedAsLvMarginSd:false,ballotVerified:true,largestNamedOpponent:'democratic',
  methodology:'Stratified, weighted online registered-voter panel;607 Senate likely-voter responses. Effective sample size and respondent-level weights not supplied.'},
 sources,ballotCrossCheckUrl:'https://montgomerytn.gov/storage/departments/election/QualifiedCandidatesNOV26_State.pdf',
 ballotCrossCheckMeaning:'Official November3 state general-election candidates, U.S. Senate page1, independently read October4. Primary Wilson County PDF hash is preserved.',
 excludesMarketDerivedForecasts:true,ddhqUsedAsInput:false};
const senate=senateRaces(parseCsv(fs.readFileSync('research/election-baseline-v1/sources/1976-2024-senate-state.csv','utf8')),JSON.parse(fs.readFileSync('research/election-baseline-v2/final-stage-overrides.json')));
const president=presidentialTables(parseCsv(fs.readFileSync('research/election-baseline-v1/sources/1976-2024-president.csv','utf8')));
const features=raceFeatures({year:2026,state:'TN',entrants},senate,president);
const prior={structuralMargin:predict(model.model,features.x),residualRms:model.residualRms,trainedThroughYear:model.model.trainedThroughYear,modelHash:model.modelHash,features};
const assessment=assessElection({poll:facts.poll,prior,definition,assessmentAt:new Date().toISOString(),electionAt:'2026-11-04T06:00:00.000Z'});
const record={schemaVersion:1,id:'tennessee-causal-assessment-20261004-v1',facts,prior,definitionHash:createHash('sha256').update(fs.readFileSync(`${base}/definition.json`)).digest('hex'),assessment,
 originalResearchNotOverwritten:true,sourceAtMeaning:'Time this new V2/poll/ballot assessment became available. Original component clocks, source hashes and age remain explicit.',
 invalidation:['New independent poll','Candidate/affiliation/ballot change','Material campaign shock','Contract change','Six-hour assessment expiry'],
 liveOrdersSubmitted:0,profitabilityEstablished:false};
fs.writeFileSync(`${base}/tennessee-20261004-v1.json`,JSON.stringify(record,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({assessedAt:assessment.assessedAt,ordinary:[assessment.ordinary.lower,assessment.ordinary.upper],stress:[assessment.stress.lower,assessment.stress.upper],pollSample:assessment.sampleSize,fieldAgeDays:assessment.componentClocks.pollFieldAgeDays,modelHash:prior.modelHash,apiRequests:0}));
