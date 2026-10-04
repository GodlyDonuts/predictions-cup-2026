import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {readNativePublicationPrefix} from '../live-paper/rust-backend.mjs';
import {probabilityAccountEligibility} from '../live-paper/probability-receipt-v2.mjs';
import {sha} from '../live-paper/native-dip-proof-v1.mjs';
import {settlementInputFromProbability,runSettlementValue} from '../live-paper/settlement-value-native.mjs';

const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),read=file=>JSON.parse(fs.readFileSync(file));
const activeFile=path.join(runtime,'probability-receipt-active.json'),activeBytes=fs.readFileSync(activeFile),active=JSON.parse(activeBytes),cohortDir=path.join(runtime,'probability-receipt-cohorts',active.cohort);
const bankBytes=fs.readFileSync(path.join(cohortDir,'bank.json')),bank=JSON.parse(bankBytes),statsBytes=fs.readFileSync(path.join(cohortDir,'stats.json')),stats=JSON.parse(statsBytes);
assert.equal(stats.bankHash,active.bankHash);assert.equal(stats.failed,false);
const stamp=Date.now(),dir=path.join(runtime,'research-cohorts','settlement-value-diagnostic-20261004',String(stamp));fs.mkdirSync(dir,{recursive:true,mode:0o700});
const journal=path.join(runtime,'public-books.jsonl'),prefix=Buffer.alloc(stats.sourceBytes),fd=fs.openSync(journal,'r');
try{assert.equal(fs.readSync(fd,prefix,0,prefix.length,0),prefix.length);}finally{fs.closeSync(fd);}
const scope=path.join(dir,'scope.json'),out=path.join(dir,'publication.json');fs.writeFileSync(scope,JSON.stringify(bank.universe)+'\n',{flag:'wx',mode:0o600});
const start=performance.now();const publication=await readNativePublicationPrefix({journal,scope,endByte:prefix.length,expectedSha:sha(prefix),out});
assert.equal(publication.state.sequence,stats.sourceSequence);assert.equal(publication.state.hash,stats.sourceHash);
let account=null;try{account=probabilityAccountEligibility(read(path.join(runtime,'official-account.json')));}catch{}
const input=settlementInputFromProbability({bank,stats,publication:publication.state,account,asOfMs:Date.parse(stats.updatedAt)});
const native=runSettlementValue({input,dir:path.join(dir,'proposal')});
assert.equal(sha(fs.readFileSync(activeFile)),sha(activeBytes));assert.equal(sha(fs.readFileSync(path.join(cohortDir,'bank.json'))),sha(bankBytes));
const evidence={schemaVersion:1,at:new Date().toISOString(),claimComment:5983667980,status:'native_settlement_policy_verified_current_snapshot',
 source:{cohort:active.cohort,bankHash:active.bankHash,bankFileHash:sha(bankBytes),statsHash:sha(statsBytes),asOf:stats.updatedAt,publicBookPrefixBytes:prefix.length,publicBookPrefixHash:sha(prefix),publicBookSequence:publication.state.sequence,publicBookTailHash:publication.state.hash},
 native:{engineHash:native.result.engineHash,binaryHash:native.binaryHash,inputHash:native.inputHash,outputHash:native.outputHash,decisionWallMs:native.wallMs,totalVerificationAndDecisionMs:performance.now()-start},
 decisions:native.result.decisions,heldAcquisitionRisk:native.result.heldAcquisitionRisk,proposedMaximumCost:native.result.proposedMaximumCost,
 existingPaper:{realizedPnl:stats.wallets['3000'].realizedPnl,liquidationPnl:stats.wallets['3000'].liquidationPnl,fills:stats.wallets['3000'].fills,positions:stats.wallets['3000'].positions.length},
 unchanged:{activeBinding:true,frozenBank:true,walletWritten:false,additionalProcesses:0,competitionOrders:0,networkRequests:0},
 limitations:native.result.limitations,profitabilityEstablished:false};
const file='data/settlement-value-snapshot-'+stamp+'.json';fs.writeFileSync(file,JSON.stringify(evidence)+'\n',{flag:'wx'});
console.log(JSON.stringify({file,status:evidence.status,timing:evidence.native,decisions:evidence.decisions.map(x=>({candidate:x.candidateId,action:x.action,reason:x.reason,worstCaseHeldLoss:x.worstCaseHeldLoss})),proposedMaximumCost:evidence.proposedMaximumCost,existingPaper:evidence.existingPaper,profitabilityEstablished:false}));
