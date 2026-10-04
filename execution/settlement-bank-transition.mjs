import fs from 'node:fs';import path from 'node:path';
import {digest} from '../live-paper/model.mjs';
import {Journal} from '../live-paper/runtime.mjs';
import {payloadHash,atomicJson,replayJournal} from './ledger.mjs';
import {validateSettlementBank} from './settlement-controller.mjs';
import {productionHandoffCurrent} from './production-activation.mjs';
import {validateExecutionState} from './core.mjs';
const need=(v,c)=>{if(!v)throw Error('SETTLEMENT_TRANSITION_'+c);};
const immutableCandidate=c=>({id:c.id,marketId:c.marketId,exchangeId:c.exchangeId,side:c.side,predicateHash:c.predicateHash,riskGroups:c.riskGroups,settlesAtMs:c.settlesAtMs});

export function validateSettlementTransition({activation,previousBank,nextBank,universe,intents={},owned={},nowMs}){
 need(Number.isSafeInteger(nowMs)&&nowMs>=Date.parse(activation.startedAt)&&nowMs<Date.parse(activation.entryExpiresAt),'WINDOW');
 validateSettlementBank(previousBank,activation,universe);
 need(digest(nextBank)!==activation.strategyBankHash,'UNCHANGED_BANK');
 const nextActivation={...activation,strategyBankHash:digest(nextBank)};
 validateSettlementBank(nextBank,nextActivation,universe);
 need(payloadHash(previousBank.nativeBackend)===payloadHash(nextBank.nativeBackend)&&payloadHash(previousBank.policy)===payloadHash(nextBank.policy),'POLICY_OR_ENGINE_CHANGED');
 need(Object.values(intents).every(i=>i.status==='closed'),'UNRESOLVED_INTENT');
 for(const previous of previousBank.candidates){
  const next=nextBank.candidates.find(c=>c.id===previous.id);
  need(next&&payloadHash(immutableCandidate(next))===payloadHash(immutableCandidate(previous)),'EXISTING_CANDIDATE_CHANGED');
  need(nextBank.contracts[previous.marketId].rootHash===previousBank.contracts[previous.marketId].rootHash,'EXISTING_CONTRACT_CHANGED');
 }
 for(const [exchangeId,p]of Object.entries(owned))need(nextBank.candidates.some(c=>c.exchangeId===exchangeId&&c.marketId===p.marketId),'OWNED_SCOPE_MISSING');
 return nextActivation;
}

// An engineering transition changes one frozen-bank pointer. The same run,
// activation window, execution journal, inventory and cumulative purchases stay.
// The collector must be absent and have released both runtime leases first.
export function applyStoppedSettlementTransition({runtimeDir,collectorLockFile,nextBank,universe,now=Date.now}){
 const guard=path.join(runtimeDir,'settlement-bank-transition.lock'),fd=fs.openSync(guard,'wx',0o600);
 try{
  const file=path.join(runtimeDir,'competition-activation.json'),beforeBytes=fs.readFileSync(file),activation=JSON.parse(beforeBytes);
  const dir=path.join(runtimeDir,'competition-runs',activation.runId);
  const stopFile=path.join(runtimeDir,'COLLECTOR_STOP'),stopBytes=fs.readFileSync(stopFile),stop=JSON.parse(stopBytes);
  need(stop.source==='settlement_bank_transition'&&stop.nextBankHash===digest(nextBank),'ENGINEERING_STOP_REQUIRED');
  need(!fs.existsSync(collectorLockFile)&&!fs.existsSync(path.join(dir,'execution-owner.lock')),'WRITER_PRESENT');
  need(!fs.existsSync(path.join(runtimeDir,'EXECUTION_STOP'))&&!fs.existsSync(path.join(runtimeDir,'STOP')),'STOP');
  need(productionHandoffCurrent(runtimeDir,activation),'HANDOFF');
  const previousBank=JSON.parse(fs.readFileSync(path.join(runtimeDir,'production-banks',activation.strategyBankHash+'.json')));
  const coreFile=path.join(dir,'core-journal.jsonl'),intentFile=path.join(dir,'execution-journal.jsonl');
  const coreJournal=fs.existsSync(coreFile)?new Journal(coreFile):null,core=coreJournal?.last?.state;
  if(core)validateExecutionState(core);
  const ledger=replayJournal(fs.existsSync(intentFile)?fs.readFileSync(intentFile,'utf8'):'',{scope:activation.tournamentId});
  need(Object.values(ledger.intents).every(i=>i.status==='closed'),'LEDGER_UNRESOLVED');
  need(Object.keys(ledger.intents).length===Object.keys(core?.intents||{}).length&&Object.keys(ledger.intents).every(id=>core?.intents[id]?.status==='closed'),'CORE_LEDGER_MISMATCH');
  const nextActivation=validateSettlementTransition({activation,previousBank,nextBank,universe,intents:core?.intents||{},owned:core?.owned||{},nowMs:now()});
  const nextFile=path.join(runtimeDir,'production-banks',nextActivation.strategyBankHash+'.json');
  if(fs.existsSync(nextFile))need(digest(JSON.parse(fs.readFileSync(nextFile)))===nextActivation.strategyBankHash,'NEXT_BANK_FILE');
  else{const out=fs.openSync(nextFile,'wx',0o600);try{fs.writeFileSync(out,JSON.stringify(nextBank)+'\n');fs.fsyncSync(out);}finally{fs.closeSync(out);}}
  const transitionDir=path.join(runtimeDir,'production-bank-transitions');fs.mkdirSync(transitionDir,{recursive:true,mode:0o700});
  const record={schemaVersion:1,at:new Date(now()).toISOString(),runId:activation.runId,previousBankHash:activation.strategyBankHash,nextBankHash:nextActivation.strategyBankHash,
   previousActivationHash:payloadHash(activation),nextActivationHash:payloadHash(nextActivation),previousActivation:activation,nextActivation,
   coreSequence:coreJournal?.sequence||0,coreHash:coreJournal?.previous||'',coreIntents:Object.keys(core?.intents||{}).length,ledgerSequence:ledger.sequence,ledgerHash:ledger.lastHash,
   runChanged:false,windowChanged:false,policyChanged:false,accountingReset:false,networkRequests:0,ordersSubmitted:0};
  const recordFile=path.join(transitionDir,payloadHash(record)+'.json');
  fs.writeFileSync(recordFile,JSON.stringify(record)+'\n',{flag:'wx',mode:0o600});const recordFd=fs.openSync(recordFile,'r+');try{fs.fsyncSync(recordFd);}finally{fs.closeSync(recordFd);}
  // An interrupted pointer swap leaves the old activation valid and a prepared
  // immutable record. Never reset a run to recover from a partial transition.
  need(fs.readFileSync(file).equals(beforeBytes)&&fs.readFileSync(stopFile).equals(stopBytes)&&!fs.existsSync(collectorLockFile)&&!fs.existsSync(path.join(dir,'execution-owner.lock'))&&!fs.existsSync(path.join(runtimeDir,'EXECUTION_STOP'))&&!fs.existsSync(path.join(runtimeDir,'STOP')),'CONCURRENT_CHANGE');
  atomicJson(file,nextActivation);
  need(payloadHash(JSON.parse(fs.readFileSync(file)))===record.nextActivationHash,'READBACK');
  return {recordFile,recordHash:payloadHash(record),nextActivation};
 }finally{fs.closeSync(fd);fs.unlinkSync(guard);}
}
