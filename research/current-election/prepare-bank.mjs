import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {digest} from '../../live-paper/model.mjs';import {validateUniverse} from '../../live-paper/breadth.mjs';
import {validateSettlementTransition,applyStoppedSettlementTransition} from '../../execution/settlement-bank-transition.mjs';
import {assessElection} from './assessment.mjs';
const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),base='research/current-election';
const a=JSON.parse(fs.readFileSync(path.join(runtime,'competition-activation.json'))),previous=JSON.parse(fs.readFileSync(path.join(runtime,'production-banks',a.strategyBankHash+'.json')));
const file=`${base}/tennessee-20261004-v1.json`,record=JSON.parse(fs.readFileSync(file)),d=JSON.parse(fs.readFileSync(`${base}/definition.json`));
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
if(sha(fs.readFileSync(`${base}/definition.json`))!==record.definitionHash)throw Error('ASSESSMENT_DEFINITION_CHANGED');
const recomputed=assessElection({poll:record.facts.poll,prior:record.prior,definition:d,assessmentAt:record.assessment.assessedAt,electionAt:record.assessment.componentClocks.electionAt});
if(digest(recomputed)!==digest(record.assessment))throw Error('ASSESSMENT_NOT_REPRODUCIBLE');
const metadata=JSON.parse(fs.readFileSync(path.join(runtime,'production-metadata','tennessee-current-assessment-20261004-v1','363.json')));
if(metadata.marketId!=='363'||metadata.rootHash!==previous.contracts['363'].rootHash||metadata.root.contract_details?.winnerName!=='Republican Party'||metadata.root.contract_details?.electionDate!=='2026-11-03')throw Error('CONTRACT_BINDING_CHANGED');
const universe=validateUniverse(JSON.parse(fs.readFileSync('live-paper/price-universe.json')),a.tournamentId),bank=structuredClone(previous),assessment=record.assessment;
bank.cutoff=assessment.assessedAt;bank.parentBankHash=a.strategyBankHash;
bank.contracts['363']={...bank.contracts['363'],receivedAt:metadata.receivedAt,sourceHash:metadata.responseHash};
bank.candidates.find(c=>c.marketId==='363').research={id:record.id,predicateHash:previous.contracts['363'].predicateHash,side:'YES',
 lower:(Math.floor(assessment.ordinary.lower*1e6)/1e6).toFixed(6),upper:(Math.ceil(assessment.ordinary.upper*1e6)/1e6).toFixed(6),
 recordedAtMs:Date.parse(assessment.assessedAt),sourceAtMs:Date.parse(assessment.assessedAt),expiresAtMs:Date.parse(assessment.expiresAt),method:'subjective_range',settlementReviewed:true,flags:[],sources:record.facts.sources.map(({url,contentHash})=>({url,contentHash}))};
bank.assessment={id:record.id,manifestSha256:sha(fs.readFileSync(file)),definitionHash:record.definitionHash,modelHash:record.prior.modelHash,
 componentClocks:assessment.componentClocks,ordinary:[assessment.ordinary.lower,assessment.ordinary.upper],broaderStress:[assessment.stress.lower,assessment.stress.upper],
 sourceAtMeaning:record.sourceAtMeaning,assessmentCodeHash:sha(fs.readFileSync(`${base}/assessment.mjs`))};
bank.limitations=['New TN assessment has its own six-hour clock, preserving the original August field dates and source hashes; WV stays expired and KS remains unresolved.',
 'Subjective scenario probabilities are uncalibrated; broader error stresses contradict a small positive ordinary edge. No guaranteed win or near-term profitable exit.',
 'The same run, window, engine, groups, limits, original journals, cumulative purchases and inventory are retained. Legacy account inventory is excluded.'];
validateSettlementTransition({activation:a,previousBank:previous,nextBank:bank,universe,nowMs:Date.now()});
if(process.argv.includes('--apply')){
 const applied=applyStoppedSettlementTransition({runtimeDir:runtime,collectorLockFile:path.join(process.env.LOCALAPPDATA,'SigTradingCup','bot.lock'),nextBank:bank,universe});
 const transition=JSON.parse(fs.readFileSync(applied.recordFile));
 fs.writeFileSync('data/tennessee-production-transition-20261004.json',JSON.stringify({at:transition.at,recordHash:applied.recordHash,runId:transition.runId,previousBankHash:transition.previousBankHash,nextBankHash:transition.nextBankHash,previousActivationHash:transition.previousActivationHash,nextActivationHash:transition.nextActivationHash,
  coreSequence:transition.coreSequence,coreHash:transition.coreHash,ledgerSequence:transition.ledgerSequence,ledgerHash:transition.ledgerHash,accountingReset:false,runChanged:false,windowChanged:false,policyChanged:false,ordersSubmittedByTransition:0,profitabilityEstablished:false},null,2)+'\n',{flag:'wx'});
 console.log(JSON.stringify({nextBankHash:applied.nextActivation.strategyBankHash,runId:applied.nextActivation.runId,accountingReset:false,ordersSubmitted:0}));
}else{
 const out=`${base}/production-bank-20261004-v2.json`;
 fs.writeFileSync(out,JSON.stringify(bank,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({stagedBankHash:digest(bank),assessmentId:record.id,lower:bank.candidates.find(c=>c.marketId==='363').research.lower,upper:bank.candidates.find(c=>c.marketId==='363').research.upper,ordersSubmitted:0}));
}
