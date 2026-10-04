import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {digest} from './model.mjs';
import {nativePublicationState} from './native-publication.mjs';
import {applyBookPublication} from './book-publication-v2.mjs';
import {probabilityCodeIdentityV2} from './probability-receipt-host-v2.mjs';
import {validateProbabilityReceiptBank,createProbabilityReceiptState,advanceReceiptProbability} from './probability-receipt-v2.mjs';
const sha=b=>createHash('sha256').update(b).digest('hex'),need=(ok,code)=>{if(!ok)throw Error('NATIVE_CONTINUATION_'+code);};
const rows=b=>{need(b.length>0&&b.at(-1)===10,'PARTIAL_JOURNAL');return b.toString().split('\n').slice(0,-1).map(JSON.parse);};
export async function exportNativeProbabilityOpening({runtimeDir,cohort,nativeBackend}){
 need(path.isAbsolute(runtimeDir)&&/^[a-z][a-z0-9-]{1,63}$/.test(cohort),'DIRECTORY');
 const dir=path.join(runtimeDir,'probability-receipt-cohorts',cohort);need(!fs.existsSync(path.join(dir,'worker.lock')),'PREDECESSOR_RUNNING');
 const bankBytes=fs.readFileSync(path.join(dir,'bank.json')),priorBank=JSON.parse(bankBytes);validateProbabilityReceiptBank(priorBank);need(priorBank.name===cohort&&digest(priorBank.codeIdentity)===digest(probabilityCodeIdentityV2()),'FROZEN_CODE');
 const previousOpeningBytes=fs.readFileSync(path.join(dir,'opening-state.json'));need(sha(previousOpeningBytes)===priorBank.accountingContinuation.openingFileHash,'OPENING_CHANGED');
 const source=path.join(runtimeDir,'public-books.jsonl'),bytes=fs.readFileSync(source),records=rows(bytes),journal=fs.readFileSync(path.join(dir,'journal.jsonl'));
 const native=await nativePublicationState({runtimeDir,binding:nativeBackend,universe:priorBank.universe,journal:source,endByte:priorBank.sourcePrefixBytes,expectedSha:priorBank.sourcePrefixHash,receiptDir:path.join(dir,'native-export')});
 let publication=native.state,state=createProbabilityReceiptState(priorBank,JSON.parse(previousOpeningBytes)),offset=priorBank.sourcePrefixBytes,index=priorBank.sourcePrefixSequence;
 need(publication.sequence===index&&publication.hash===priorBank.sourcePrefixJournalHash&&sha(bytes.subarray(0,offset))===priorBank.sourcePrefixHash,'ORIGINAL_PREFIX');
 let sequence=0,previous=digest(priorBank);
 for(const record of rows(journal)){
  const {hash,...body}=record;need(body.sequence===++sequence&&body.previous===previous&&body.bankHash===digest(priorBank)&&digest(body)===hash,'JOURNAL');
  if(body.kind==='genesis')need(sequence===1&&body.stateHash===digest(state),'GENESIS');
  else{
   need(body.kind==='batch'&&body.sourceSequence>publication.sequence,'CHECKPOINT');
   while(publication.sequence<body.sourceSequence){const row=records[index++];need(row,'SOURCE_TRUNCATED');publication=applyBookPublication(publication,row);advanceReceiptProbability(state,row,priorBank,body.availableAt,body.accountEligibility);offset=bytes.indexOf(10,offset)+1;}
   need(body.sourceBytes===offset&&body.sourceSequence===publication.sequence&&body.sourceHash===publication.hash&&body.stateHash===digest(state),'REPLAY');
  }
  previous=hash;
 }
 need(sequence>0&&offset===bytes.length,'UNCONSUMED_SOURCE');need(sha(fs.readFileSync(source))===sha(bytes),'SOURCE_CHANGED');
 return{schemaVersion:1,priorBank,priorBankFileHash:sha(bankBytes),state,sourceBytes:offset,sourceSequence:publication.sequence,sourceHash:publication.hash,sourcePrefixHash:sha(bytes),journalSequence:sequence,journalHash:previous,journalFileHash:sha(journal),journalBytes:journal.length};
}
export function nativeProbabilityContinuationBundle(opening,nativeBackend){
 const openingBytes=Buffer.from(JSON.stringify(opening,null,2)+'\n');
 return{identities:opening.priorBank.identities,research:opening.priorBank.research,researchEvidence:opening.priorBank.researchEvidence,opening,nativeBackend,
  accountingContinuation:{accountingCarried:true,priorCohort:opening.priorBank.name,priorBankHash:digest(opening.priorBank),priorBankFileHash:opening.priorBankFileHash,priorStateHash:digest(opening.state),priorJournalHash:opening.journalHash,priorJournalFileHash:opening.journalFileHash,priorJournalBytes:opening.journalBytes,openingFileHash:sha(openingBytes),reason:'native_prefix_transport_only_original_wallet_accounting_carried',executionAuthority:false}};
}
