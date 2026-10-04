import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {digest} from './model.mjs';
import {createBookPublicationState,applyBookPublication} from './book-publication.mjs';
import {validateProbabilityReceiptBank,createProbabilityReceiptState,advanceReceiptProbability} from './probability-receipt.mjs';
const sha=b=>createHash('sha256').update(b).digest('hex');
const need=(ok,reason)=>{if(!ok)throw Error('PROBABILITY_CONTINUATION_'+reason);};
const originalFiles=['./probability-receipt-host.mjs','./probability-receipt.mjs','./probability-paper.mjs','./model.mjs','./book-publication.mjs','../paper/fixed.mjs','../feed/contract.mjs','../execution/ledger.mjs'];
const rows=b=>{need(b.length>0&&b.at(-1)===10,'PARTIAL_JOURNAL');return b.toString().split('\n').slice(0,-1).map(JSON.parse);};

// Replay the predecessor with its recorded receipt clock and account exclusions.
// Today's account cache must never change yesterday's accounting.
export function exportProbabilityOpening({runtimeDir,cohort}){
 need(path.isAbsolute(runtimeDir)&&/^[a-z][a-z0-9-]{1,63}$/.test(cohort),'DIRECTORY');
 const dir=path.join(runtimeDir,'probability-receipt-cohorts',cohort);
 need(!fs.existsSync(path.join(dir,'worker.lock')),'PREDECESSOR_RUNNING');
 const bankBytes=fs.readFileSync(path.join(dir,'bank.json')),priorBank=JSON.parse(bankBytes);validateProbabilityReceiptBank(priorBank);need(priorBank.name===cohort,'COHORT');
 const identity=Object.fromEntries(originalFiles.map(p=>[p,sha(fs.readFileSync(new URL(p,import.meta.url)))]));need(digest(priorBank.codeIdentity)===digest(identity),'FROZEN_CODE');
 const bytes=fs.readFileSync(path.join(runtimeDir,'public-books.jsonl')),records=rows(bytes),journal=fs.readFileSync(path.join(dir,'journal.jsonl'));
 let publication=createBookPublicationState(priorBank.universe),state=createProbabilityReceiptState(priorBank),offset=0,index=0;
 while(publication.sequence<priorBank.sourcePrefixSequence){need(records[index],'SOURCE_TRUNCATED');publication=applyBookPublication(publication,records[index++]);offset=bytes.indexOf(10,offset)+1;}
 need(offset===priorBank.sourcePrefixBytes&&sha(bytes.subarray(0,offset))===priorBank.sourcePrefixHash&&publication.hash===priorBank.sourcePrefixJournalHash,'ORIGINAL_PREFIX');
 let sequence=0,previous=digest(priorBank);
 for(const record of rows(journal)){
  const{hash,...body}=record;need(body.sequence===++sequence&&body.previous===previous&&body.bankHash===digest(priorBank)&&digest(body)===hash,'JOURNAL');
  if(body.kind==='genesis')need(sequence===1&&body.stateHash===digest(state),'GENESIS');
  else{
   need(body.kind==='batch'&&body.sourceSequence>publication.sequence,'CHECKPOINT');
   while(publication.sequence<body.sourceSequence){const row=records[index++];need(row,'SOURCE_TRUNCATED');publication=applyBookPublication(publication,row);advanceReceiptProbability(state,row,priorBank,body.availableAt,body.accountEligibility);offset=bytes.indexOf(10,offset)+1;}
   need(body.sourceBytes===offset&&body.sourceSequence===publication.sequence&&body.sourceHash===publication.hash&&body.stateHash===digest(state),'REPLAY');
  }
  previous=hash;
 }
 need(sequence>0&&offset===bytes.length,'UNCONSUMED_SOURCE');
 return{schemaVersion:1,priorBank,priorBankFileHash:sha(bankBytes),state,sourceBytes:offset,sourceSequence:publication.sequence,sourceHash:publication.hash,sourcePrefixHash:sha(bytes),journalSequence:sequence,journalHash:previous,journalFileHash:sha(journal),journalBytes:journal.length};
}
export function probabilityContinuationBundle(opening){
 const openingBytes=Buffer.from(JSON.stringify(opening,null,2)+'\n');
 return{identities:opening.priorBank.identities,research:opening.priorBank.research,researchEvidence:opening.priorBank.researchEvidence,opening,
  accountingContinuation:{accountingCarried:true,priorCohort:opening.priorBank.name,priorBankHash:digest(opening.priorBank),priorBankFileHash:opening.priorBankFileHash,priorStateHash:digest(opening.state),priorJournalHash:opening.journalHash,priorJournalFileHash:opening.journalFileHash,priorJournalBytes:opening.journalBytes,openingFileHash:sha(openingBytes),reason:'position_exposure_blocks_entry_but_not_reduce_only_paper_exit',executionAuthority:false}};
}
