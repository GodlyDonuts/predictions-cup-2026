import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {digest} from './model.mjs';
import {payloadHash} from '../execution/ledger.mjs';

// These are the current reducers, not every archived experiment. Archives are
// retained separately and must not be discarded by a runtime migration.
export const CURRENT_PAPER_POINTERS=Object.freeze([
 ['probability-receipt-active.json','probability-receipt-cohorts'],
 ['calibrated-poll-paper-active.json','probability-receipt-cohorts'],
 ['primary-breadth-paper-active.json','probability-receipt-cohorts'],
 ['settlement-paper-active.json','settlement-paper-cohorts'],
 ['settlement-portfolio-v2-active.json','settlement-portfolio-v2-cohorts'],
 ['dual-no-pair-active.json','dual-no-pair-cohorts'],
 ['dual-no-pair-v2-active.json','dual-no-pair-v2-cohorts'],
 ['coupled-bulk-active.json','coupled-bulk-cohorts'],
]);
const need=(v,c)=>{if(!v)throw Error('PAPER_INVENTORY_'+c);};
const sha=b=>createHash('sha256').update(b).digest('hex');
const hashFile=(file,end)=>{const h=createHash('sha256'),fd=fs.openSync(file,'r'),chunk=Buffer.alloc(1024*1024);try{for(let p=0;p<end;){const n=fs.readSync(fd,chunk,0,Math.min(chunk.length,end-p),p);need(n>0,'TRUNCATED_PREFIX');h.update(chunk.subarray(0,n));p+=n;}}finally{fs.closeSync(fd);}return h.digest('hex');};
function tail(file){
 const size=fs.statSync(file).size;need(size>0,'EMPTY_JOURNAL');const n=Math.min(size,8*1024*1024),b=Buffer.alloc(n),fd=fs.openSync(file,'r');let read=0;
 try{while(read<n){const got=fs.readSync(fd,b,read,n-read,size-n+read);need(got>0,'TRUNCATED_TAIL');read+=got;}}finally{fs.closeSync(fd);}
 const end=b.lastIndexOf(10);need(end>=0,'INCOMPLETE_JOURNAL');const start=b.lastIndexOf(10,end-1)+1;need(start>0||n===size,'OVERSIZE_ROW');return{row:JSON.parse(b.subarray(start,end).toString()),bytes:size-n+end+1};
}
const optionalHash=file=>fs.existsSync(file)?sha(fs.readFileSync(file)):null;
export function capturePaperMigrationInventory({runtimeDir,now=Date.now,maximumAgeMs=30000,pointers=CURRENT_PAPER_POINTERS}){
 const clock=typeof now==='function'?now:()=>now;need(Number.isSafeInteger(clock())&&Number.isSafeInteger(maximumAgeMs)&&maximumAgeMs>0,'CLOCK');const current=[];
 for(const[pointer,family]of pointers){
  need(CURRENT_PAPER_POINTERS.some(([p,f])=>p===pointer&&f===family),'POINTER_SCOPE');
  const pointerFile=path.join(runtimeDir,pointer),pointerBytes=fs.readFileSync(pointerFile),p=JSON.parse(pointerBytes);need(/^[-a-z0-9]{1,63}$/.test(p.cohort),'COHORT_SCOPE');
  const base=path.join(runtimeDir,family,p.cohort),bankBytes=fs.readFileSync(path.join(base,'bank.json')),bank=JSON.parse(bankBytes),bankHashMethod=['dual-no-pair-cohorts','dual-no-pair-v2-cohorts'].includes(family)?'canonical_payload':'model_digest';need((bankHashMethod==='canonical_payload'?payloadHash(bank):digest(bank))===p.bankHash,'BANK_IDENTITY');
  const overlay=path.join(base,'operator-value-revision'),hasOverlay=fs.existsSync(path.join(overlay,'opening.json'))||fs.existsSync(path.join(overlay,'journal.jsonl')),dir=hasOverlay?overlay:base;
  if(hasOverlay)need(fs.existsSync(path.join(overlay,'opening.json'))&&fs.existsSync(path.join(overlay,'journal.jsonl')),'INCOMPLETE_OVERLAY');
  let stats,receipt,statsBytes;
  for(let attempt=0;attempt<8;attempt++){
   statsBytes=fs.readFileSync(path.join(dir,'stats.json'));stats=JSON.parse(statsBytes);receipt=tail(path.join(dir,'journal.jsonl'));
   if(stats.journalHash===receipt.row.hash&&stats.journalSequence===receipt.row.sequence&&sha(fs.readFileSync(path.join(dir,'stats.json')))===sha(statsBytes))break;
   stats=null;
  }
  need(stats,'PUBLICATION_RACE');need(stats.bankHash===p.bankHash&&stats.cohort===p.cohort&&stats.failed===false,'PUBLICATION_IDENTITY');
  const observed=Date.parse(stats.updatedAt),readAt=clock();need(Number.isFinite(observed)&&observed<=readAt&&readAt-observed<=maximumAgeMs,'STALE_PUBLICATION');
  need(sha(fs.readFileSync(pointerFile))===sha(pointerBytes)&&sha(fs.readFileSync(path.join(base,'bank.json')))===sha(bankBytes),'IDENTITY_CHANGED');
  const singleFields=['cash','realizedPnl','cumulativeBuys','positions','pending','reserved','fills','recentFills','package','closedPackages','fees'];
  const wallets=stats.wallets||{single:Object.fromEntries(singleFields.filter(k=>k in stats).map(k=>[k,stats[k]]))};need(Object.keys(wallets).length>0,'NO_ACCOUNTING');
  const drain=family==='coupled-bulk-cohorts'?'COUPLED_BULK_ENTRY_DRAIN':family==='settlement-paper-cohorts'?'FUNDAMENTAL_ENTRY_DRAIN':null;
  const status=hasOverlay||drain&&fs.existsSync(path.join(runtimeDir,drain))?'entry_draining':'current_reducer';
  const anchors={bankHashMethod,bankFileHash:sha(bankBytes),journalPrefixBytes:receipt.bytes,journalPrefixHash:hashFile(path.join(dir,'journal.jsonl'),receipt.bytes),journalSequence:receipt.row.sequence,journalHash:receipt.row.hash,openingHash:optionalHash(path.join(dir,'opening.json')),sourceHash:stats.sourceHash??null,sourceSequence:stats.sourceSequence??null,sourcePrefixHash:stats.sourcePrefixHash??bank.sourcePrefixHash??null,sourcePrefixBytes:bank.sourcePrefixBytes??null};
  const overlayHash=hasOverlay?stats.revisionHash:null;need(!hasOverlay||/^[a-f0-9]{64}$/.test(overlayHash),'OVERLAY_BINDING');
  current.push({cohort:p.cohort,bankHash:p.bankHash,bankMode:bank.mode,authoritativeOwner:hasOverlay?'operator_overlay':'original_reducer',overlayHash,status,accounting:{wallets},anchors,scope:{pointer,family,authoritativeDirectory:path.relative(runtimeDir,dir).split(path.sep).join('/'),publishedAt:stats.updatedAt,baseActorMustRemainFenced:hasOverlay}});
 }
 const finished=clock();need(current.every(r=>finished-Date.parse(r.scope.publishedAt)<=maximumAgeMs),'SNAPSHOT_TOO_SLOW');
 need(new Set(current.map(r=>r.cohort)).size===current.length,'DUPLICATE_COHORT');
 return{schemaVersion:1,observedAt:new Date(finished).toISOString(),current,independentEconomicReplay:false,cutoverReady:false,activationAuthority:false,competitionOrders:0,accountRequests:0,limitations:['A fresh derived publication matched to its sealed journal tail is an inventory checkpoint, not independent economic/source replay.','All archived banks and journals remain preserved outside this current-reducer manifest.','Final cutover requires stopped original writers, fresh reconciled native replay and the account queue migration boundary.']};
}
