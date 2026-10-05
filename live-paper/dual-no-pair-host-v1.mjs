import fs from 'node:fs';import path from 'node:path';import {createHash,randomUUID} from 'node:crypto';
import {Journal} from './transport-runtime.mjs';import {atomicJson,payloadHash} from '../execution/ledger.mjs';
import {evaluateCanonicalNoCapacity,CANONICAL_NO_POLICY} from './canonical-no-capacity.mjs';
import {validateDualNoReview} from './dual-no-review-v1.mjs';
import {createDualNoPairState,advanceDualNoPair,dualNoPairReport,assertDualNoPair,DUAL_NO_PAIR_POLICY} from './dual-no-pair-paper-v1.mjs';
const sha=b=>createHash('sha256').update(b).digest('hex'),need=(ok,c)=>{if(!ok)throw Error('DUAL_NO_HOST_'+c);};
function prefixDescriptor(file,end){const fd=fs.openSync(file,'r'),before=fs.fstatSync(fd),hash=createHash('sha256'),chunk=Buffer.alloc(1048576);let offset=0,tail=Buffer.alloc(0);try{need(before.size>=end&&end>0,'SOURCE_PREFIX');while(offset<end){const n=fs.readSync(fd,chunk,0,Math.min(chunk.length,end-offset),offset);need(n>0,'SOURCE_PREFIX');hash.update(chunk.subarray(0,n));tail=Buffer.concat([tail,chunk.subarray(0,n)]).subarray(-1048576);offset+=n;}const after=fs.fstatSync(fd);need(after.ino===before.ino&&after.birthtimeMs===before.birthtimeMs&&after.size>=end&&tail.at(-1)===10,'SOURCE_PREFIX');const start=tail.lastIndexOf(10,tail.length-2)+1;return{end,hash:hash.digest('hex'),tail:JSON.parse(tail.subarray(start,tail.length-1))};}finally{fs.closeSync(fd);}}
function sealedSource(file){const size=fs.statSync(file).size,fd=fs.openSync(file,'r'),tail=Buffer.alloc(Math.min(size,1048576));try{fs.readSync(fd,tail,0,tail.length,size-tail.length);}finally{fs.closeSync(fd);}const index=tail.lastIndexOf(10);need(index>=0,'EMPTY_SOURCE');return prefixDescriptor(file,size-tail.length+index+1);}
export const dualNoPairCodeIdentity=()=>Object.fromEntries(['./dual-no-pair-host-v1.mjs','./dual-no-pair-paper-v1.mjs','./dual-no-review-v1.mjs','./canonical-no-capacity.mjs','./probability-receipt.mjs','./transport-runtime.mjs','./journal-lines.mjs','../execution/ledger.mjs','../paper/fixed.mjs','../feed/contract.mjs'].map(file=>[file,sha(fs.readFileSync(new URL(file,import.meta.url)))]));
export function openDualNoPaperHost({runtimeDir,cohort,initialize=false,bundle,now=Date.now,canOperate=()=>true}){
 need(path.isAbsolute(runtimeDir)&&/^[a-z][a-z0-9-]{1,63}$/.test(cohort),'SCOPE');
 const dir=path.join(runtimeDir,'dual-no-pair-cohorts',cohort);fs.mkdirSync(dir,{recursive:true,mode:0o700});
 const bankFile=path.join(dir,'bank.json'),statsFile=path.join(dir,'stats.json'),source=path.join(runtimeDir,'public-books.jsonl'),lock=path.join(dir,'worker.lock');
 const stopped=()=>fs.existsSync(path.join(runtimeDir,'STOP'))||fs.existsSync(path.join(dir,'STOP'))||fs.existsSync(path.join(dir,'OBSERVER_STOP'));
 need(canOperate()&&!stopped(),'STOPPED');const token=JSON.stringify({pid:process.pid,id:randomUUID()});fs.writeFileSync(lock,token,{flag:'wx',mode:0o600});let closed=false,failed=false;
 try{
  let bank;
  if(initialize){need(bundle&&!fs.existsSync(bankFile)&&!fs.existsSync(path.join(dir,'journal.jsonl')),'EXISTS');
   validateDualNoReview(bundle.review);const sealed=sealedSource(source),end=sealed.end,tail=sealed.tail,cutoff=new Date(now()).toISOString();
   need(Date.parse(tail.at)<=Date.parse(cutoff)&&Date.parse(bundle.review.recordedAt)<=Date.parse(cutoff)&&Date.parse(bundle.review.expiresAt)>Date.parse(cutoff),'CUTOFF');
   bank={schemaVersion:1,mode:'dual_no_sequential_paper_v1',name:cohort,cutoff,review:structuredClone(bundle.review),policy:DUAL_NO_PAIR_POLICY,capacityPolicy:{...CANONICAL_NO_POLICY,maxQuantity:1,maxCost:'1.000000',maxUnmatchedCost:'1.000000'},codeIdentity:dualNoPairCodeIdentity(),sourcePrefixBytes:end,sourcePrefixHash:sealed.hash,sourcePrefixSequence:tail.sequence,sourcePrefixTailHash:tail.hash};
   fs.writeFileSync(bankFile,JSON.stringify(bank,null,2)+'\n',{flag:'wx',mode:0o600});
  }else bank=JSON.parse(fs.readFileSync(bankFile));
  validateDualNoReview(bank.review);need(payloadHash(bank.codeIdentity)===payloadHash(dualNoPairCodeIdentity()),'CODE_IDENTITY');
  const bytes=fs.readFileSync(bankFile),hash=payloadHash(bank),prefix=prefixDescriptor(source,bank.sourcePrefixBytes);need(prefix.hash===bank.sourcePrefixHash,'SOURCE_PREFIX');const tail=prefix.tail;need(tail.sequence===bank.sourcePrefixSequence&&tail.hash===bank.sourcePrefixTailHash,'PREFIX_TAIL');
  const journalFile=path.join(dir,'journal.jsonl'),journal=new Journal(journalFile);let state={paper:createDualNoPairState(bank),sourceSequence:bank.sourcePrefixSequence,sourceHash:bank.sourcePrefixTailHash};
  if(journal.last){const records=fs.readFileSync(journalFile,'utf8').trimEnd().split('\n').map(JSON.parse);for(const [i,row]of records.entries()){
   const event=row.event;need(event&&['genesis','book','boundary'].includes(event.kind),'REPLAY_EVENT');
   if(i===0)need(event.kind==='genesis'&&event.bankHash===hash&&event.cutoff===bank.cutoff,'GENESIS');
   else{need(event.kind!=='genesis','REPLAY_GENESIS');const paper=advanceDualNoPair(state.paper,event,bank);if(event.kind==='book'){need(event.sourceSequence>state.sourceSequence&&/^[a-f0-9]{64}$/.test(event.sourceHash),'REPLAY_FRONTIER');state={paper,sourceSequence:event.sourceSequence,sourceHash:event.sourceHash};}else state={...state,paper};}
   need(payloadHash(state)===payloadHash(row.state),'REPLAY_STATE');assertDualNoPair(state.paper,bank);
  }}
  let journalDigest=fs.existsSync(journalFile)?sha(fs.readFileSync(journalFile)):sha(Buffer.alloc(0));
  const live=(allowStopped=false)=>need(!closed&&!failed&&(allowStopped||canOperate()&&!stopped())&&fs.readFileSync(lock,'utf8')===token&&sha(fs.readFileSync(bankFile))===sha(bytes),'OWNERSHIP');
  const status=()=>({...dualNoPairReport(state.paper,bank,new Date(now()).toISOString()),failed,journalSequence:journal.sequence,journalHash:journal.previous,sourceSequence:state.sourceSequence,sourceHash:state.sourceHash,workerPid:process.pid});
  const append=(next,event,allowStopped=false)=>{live(allowStopped);try{need((fs.existsSync(journalFile)?sha(fs.readFileSync(journalFile)):sha(Buffer.alloc(0)))===journalDigest,'JOURNAL_CHANGED');journal.commit(next,event);journalDigest=sha(fs.readFileSync(journalFile));state=next;atomicJson(statsFile,status());}catch(e){failed=true;throw e;}};
  if(!journal.last)append(state,{kind:'genesis',cutoff:bank.cutoff,bankHash:hash});
  else{need(state.paper.bankHash===hash,'JOURNAL_BANK');const at=new Date(Math.max(now(),Date.parse(state.paper.lastAt))).toISOString();append({...state,paper:advanceDualNoPair(state.paper,{kind:'boundary',availableAt:at},bank)},{kind:'boundary',availableAt:at,reason:'restart'});}
  const host={bank,status,publish:()=>atomicJson(statsFile,status()),
   onBook(publication,marketId,accountEligibility){live();const leg=bank.review.legs.find(x=>x.marketId===marketId);if(!leg)return;const book=publication.books[leg.exchangeId+':YES'];if(!book)return;
    need(publication.sequence>state.sourceSequence&&/^[a-f0-9]{64}$/.test(publication.hash)&&book.publicationSequence<=publication.sequence&&book.publicationSequence>bank.sourcePrefixSequence,'SOURCE_FRONTIER');
    const at=new Date(now()).toISOString(),books=bank.review.legs.map(x=>publication.books[x.exchangeId+':YES']);let admission={positiveConditionalCapacity:false};
    if(!publication.closed&&books.every(Boolean))try{admission=evaluateCanonicalNoCapacity({review:bank.review,policy:bank.capacityPolicy,books,account:accountEligibility,now:at});}catch{admission={positiveConditionalCapacity:false,reason:'invalid_or_unavailable_pair'};}
    const event={kind:'book',book,pairBooks:publication.closed?[]:books.filter(Boolean),availableAt:at,accountEligibility:publication.closed?null:accountEligibility,admission};
    append({paper:advanceDualNoPair(state.paper,event,bank),sourceSequence:publication.sequence,sourceHash:publication.hash},{...event,sourceSequence:publication.sequence,sourceHash:publication.hash});
   },
   focus(){const paper=state.paper,owned=Object.keys(paper.positions).length>0||Boolean(paper.pending)||Boolean(paper.package);return{bankHash:hash,markets:bank.review.legs.filter(()=>owned||now()<Date.parse(bank.review.expiresAt)).map(leg=>({marketId:leg.marketId,phase:paper.positions[leg.exchangeId]?'held':paper.pending?.exchangeId===leg.exchangeId?(paper.pending.action==='sell'?'pending_sell':'pending_buy'):'depth_required'})),ownedRiskReads:true,executionAuthority:false};},
   boundary(reason){live(true);const at=new Date(Math.max(now(),Date.parse(state.paper.lastAt))).toISOString();append({...state,paper:advanceDualNoPair(state.paper,{kind:'boundary',availableAt:at},bank)},{kind:'boundary',availableAt:at,reason},true);},
   close(){if(closed)return;need(fs.readFileSync(lock,'utf8')===token,'LOCK_CHANGED');closed=true;fs.unlinkSync(lock);}
  };return host;
 }catch(e){if(fs.existsSync(lock)&&fs.readFileSync(lock,'utf8')===token)fs.unlinkSync(lock);throw e;}
}
