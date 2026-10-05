import fs from 'node:fs';
import path from 'node:path';
import {createHash,randomUUID} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {performance} from 'node:perf_hooks';
import {nativePublicationState,nativePublicationBinding} from './native-publication.mjs';
import {applyBookPublication} from './book-publication-v2.mjs';
import {digest} from './model.mjs';
import {atomicJson} from '../execution/ledger.mjs';
import {probabilityAccountEligibility} from './probability-receipt.mjs';
import {runSettlementValue} from './settlement-value-native.mjs';
import {SETTLEMENT_PAPER_POLICY,SETTLEMENT_PAPER_FILLS,validateSettlementPaperBank,createSettlementPaperState,advanceSettlementPaper,settlementPaperReport} from './settlement-paper.mjs';

const need=(ok,code)=>{if(!ok)throw Error('SETTLEMENT_HOST_'+code);},sha=b=>createHash('sha256').update(b).digest('hex');
export const settlementPaperCodeIdentity=()=>Object.fromEntries(['./settlement-paper-host.mjs','./settlement-paper.mjs','./settlement-value-native.mjs','./probability-receipt.mjs','./probability-paper.mjs','./model.mjs','./book-publication-v2.mjs','./native-publication.mjs','./rust-backend.mjs','../paper/fixed.mjs','../feed/contract.mjs','../execution/ledger.mjs'].map(p=>[p,sha(fs.readFileSync(new URL(p,import.meta.url)))]));
const complete=file=>{const b=fs.readFileSync(file);return b.subarray(0,b.lastIndexOf(10)+1);};
const rows=bytes=>{need(!bytes.length||bytes.at(-1)===10,'PARTIAL');return bytes.toString().split('\n').slice(0,-1).map(JSON.parse);};
function write(file,bytes,flag){const fd=fs.openSync(file,flag,0o600);try{let offset=0;while(offset<bytes.length){const n=fs.writeSync(fd,bytes,offset,bytes.length-offset);need(n>0,'WRITE');offset+=n;}fs.fsyncSync(fd);}finally{fs.closeSync(fd);}}

export async function openSettlementPaper({runtimeDir,cohort,initialize=false,bundle,now=Date.now,workspaceDir=path.dirname(path.dirname(fileURLToPath(import.meta.url)))}={}){
 need(path.isAbsolute(runtimeDir)&&/^[a-z][a-z0-9-]{1,63}$/.test(cohort),'DIRECTORY');
 const dir=path.join(runtimeDir,'settlement-paper-cohorts',cohort),bankFile=path.join(dir,'bank.json'),journal=path.join(dir,'journal.jsonl'),source=path.join(runtimeDir,'public-books.jsonl'),lockFile=path.join(dir,'worker.lock');
 const stopped=()=>[path.join(workspaceDir,'STOP'),path.join(runtimeDir,'STOP'),path.join(dir,'STOP'),path.join(dir,'OBSERVER_STOP')].some(f=>fs.existsSync(f));need(!stopped(),'STOPPED');
 fs.mkdirSync(dir,{recursive:true,mode:0o700});const token=JSON.stringify({pid:process.pid,id:randomUUID()});write(lockFile,Buffer.from(token),'wx');let closed=false,failed=false;
 const live=()=>need(!closed&&!failed&&!stopped()&&fs.readFileSync(lockFile,'utf8')===token,'OWNERSHIP');
 try{
  const start=performance.now(),bytes=complete(source),records=rows(bytes),sourceStat=fs.statSync(source),codeIdentity=settlementPaperCodeIdentity();need(records.length>0,'EMPTY');let bank;
  if(initialize){need(bundle&&!fs.existsSync(bankFile)&&!fs.existsSync(journal),'EXISTS');const last=records.at(-1),cutoff=new Date(now()).toISOString();need(Date.parse(last.at)<=Date.parse(cutoff),'FUTURE_SOURCE');
   bank={schemaVersion:1,mode:'funded_settlement_value_paper_v1',name:cohort,cutoff,universe:structuredClone(bundle.universe),identities:structuredClone(bundle.identities),research:structuredClone(bundle.research),researchEvidence:structuredClone(bundle.researchEvidence),riskGroups:structuredClone(bundle.riskGroups),valuationDeadlineMs:bundle.valuationDeadlineMs,policy:SETTLEMENT_PAPER_POLICY,fillPolicy:SETTLEMENT_PAPER_FILLS,sourcePrefixBytes:bytes.length,sourcePrefixHash:sha(bytes),sourcePrefixSequence:last.sequence,sourcePrefixJournalHash:last.hash,nativeBackend:structuredClone(bundle.nativeBackend),codeIdentity};
  }else{bank=JSON.parse(fs.readFileSync(bankFile));need(digest(bank.codeIdentity)===digest(codeIdentity),'FROZEN_CODE');}
  validateSettlementPaperBank(bank);const executable=nativePublicationBinding(runtimeDir,bank.nativeBackend),native=await nativePublicationState({runtimeDir,binding:bank.nativeBackend,universe:bank.universe,journal:source,endByte:bank.sourcePrefixBytes,expectedSha:bank.sourcePrefixHash,receiptDir:path.join(dir,'native-startup')});live();
  let publication=native.state,state=createSettlementPaperState(bank),offset=bank.sourcePrefixBytes,index=bank.sourcePrefixSequence,decisionSerial=0;
  need(native.sourceRecords===index&&records[index-1]?.hash===publication.hash&&sha(bytes.subarray(0,offset))===bank.sourcePrefixHash&&publication.hash===bank.sourcePrefixJournalHash,'PREFIX');
  const evaluate=input=>{const r=runSettlementValue({input,executable,dir:path.join(dir,'native-decisions',randomUUID()+'-'+(++decisionSerial))});need(r.binaryHash===bank.nativeBackend.binaryHash&&r.result.engineHash===bank.nativeBackend.engineHash,'NATIVE_IDENTITY');return r.result;};
  const reduce=(batch,at,account,sourceState=publication,walletState=state)=>{let pub=sourceState,next=structuredClone(walletState);for(const row of batch){pub=applyBookPublication(pub,row);advanceSettlementPaper(next,row,bank,at,account,evaluate);}return{publication:pub,state:next};};
  let sequence=0,previous=digest(bank),journalBytes=0,journalHash=sha(Buffer.alloc(0));
  const expected=()=>({sourceBytes:offset,sourceSequence:publication.sequence,sourceHash:publication.hash,stateHash:digest(state)});
  if(!initialize){const raw=fs.readFileSync(journal);for(const record of rows(raw)){const {hash,...body}=record;need(body.sequence===++sequence&&body.previous===previous&&body.bankHash===digest(bank)&&digest(body)===hash,'JOURNAL');
    if(body.kind==='genesis')need(sequence===1&&body.stateHash===digest(state),'GENESIS');else{need(body.kind==='batch'&&body.sourceSequence>publication.sequence,'CHECKPOINT');const batch=[];while(publication.sequence+batch.length<body.sourceSequence){need(records[index],'TRUNCATED');batch.push(records[index++]);}({publication,state}=reduce(batch,body.availableAt,body.accountEligibility));for(let n=0;n<batch.length;n++)offset=bytes.indexOf(10,offset)+1;need(Object.entries(expected()).every(([k,v])=>body[k]===v),'REPLAY');}previous=hash;
   }need(sequence>0,'MISSING_GENESIS');journalBytes=raw.length;journalHash=sha(raw);
  }
  const verifyJournal=()=>{const b=fs.readFileSync(journal);need(b.length===journalBytes&&sha(b)===journalHash,'JOURNAL_CHANGED');};
  const append=body=>{live();verifyJournal();const item={sequence:sequence+1,previous,bankHash:digest(bank),...body},hash=digest(item);write(journal,Buffer.from(JSON.stringify({...item,hash})+'\n'),'a');sequence++;previous=hash;journalBytes=fs.statSync(journal).size;journalHash=sha(fs.readFileSync(journal));};
  if(initialize){write(bankFile,Buffer.from(JSON.stringify(bank,null,2)+'\n'),'wx');write(journal,Buffer.alloc(0),'wx');append({kind:'genesis',stateHash:digest(state)});}
  const startup={coldBootMs:performance.now()-start,nativeVerifyMs:native.verifyMs,sourcePrefixBytes:bank.sourcePrefixBytes},bankBytesHash=sha(fs.readFileSync(bankFile));let prefixHash=sha(bytes.subarray(0,offset));
  const consume=()=>{live();try{
   nativePublicationBinding(runtimeDir,bank.nativeBackend);need(sha(fs.readFileSync(bankFile))===bankBytesHash&&digest(settlementPaperCodeIdentity())===digest(codeIdentity),'IDENTITY_CHANGED');verifyJournal();
   const stat=fs.statSync(source);need(stat.ino===sourceStat.ino&&stat.birthtimeMs===sourceStat.birthtimeMs&&stat.size>=offset,'SOURCE_REPLACED');const b=complete(source);need(sha(b.subarray(0,offset))===prefixHash,'SOURCE_CHANGED');if(b.length===offset)return 0;
   const batch=rows(b.subarray(offset)),at=new Date(now()).toISOString();let account=null;try{account=probabilityAccountEligibility(JSON.parse(fs.readFileSync(path.join(runtimeDir,'official-account.json'))));}catch{}
   const next=reduce(batch,at,account);append({kind:'batch',availableAt:at,accountEligibility:account,sourceBytes:b.length,sourceSequence:next.publication.sequence,sourceHash:next.publication.hash,stateHash:digest(next.state)});publication=next.publication;state=next.state;offset=b.length;prefixHash=sha(b);return batch.length;
  }catch(error){failed=true;throw error;}};
  const status=()=>({...settlementPaperReport(state,bank,new Date(now()).toISOString()),startup,...expected(),journalSequence:sequence,journalHash:previous,sourceClosed:publication.closed,failed});
  const publish=()=>atomicJson(path.join(dir,'stats.json'),status());
  const close=()=>{if(closed)return;closed=true;need(fs.readFileSync(lockFile,'utf8')===token,'LOCK_CHANGED');fs.unlinkSync(lockFile);};
  return{dir,bank,consume,status,publish,close};
 }catch(error){if(fs.existsSync(lockFile)&&fs.readFileSync(lockFile,'utf8')===token)fs.unlinkSync(lockFile);throw error;}
}
