import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {atomicJson} from '../execution/ledger.mjs';
import {digest} from './model.mjs';import {instant} from '../paper/fixed.mjs';
import {CANONICAL_NO_POLICY,validateCapacityReview,evaluateCanonicalNoCapacity} from './canonical-no-capacity.mjs';
import {probabilityAccountEligibility} from './probability-receipt.mjs';
const need=(ok,c)=>{if(!ok)throw Error('CAPACITY_MONITOR_'+c);};
const sha=b=>createHash('sha256').update(b).digest('hex');
export const capacityCodeIdentity=()=>Object.fromEntries(['./canonical-no-capacity.mjs','./canonical-capacity-monitor.mjs','./probability-receipt.mjs','./model.mjs','../paper/fixed.mjs','../feed/contract.mjs','../execution/ledger.mjs'].map(p=>[p,sha(fs.readFileSync(new URL(p,import.meta.url)))]));
export function planCapacityPair(plan,review,{cycle,now,excludedExchangeIds=[]}={}){
 validateCapacityReview(review);need(Number.isSafeInteger(cycle)&&cycle>=0&&Number.isSafeInteger(now),'PLAN_CLOCK');
 if(cycle%4!==0||now<instant(review.recordedAt)||now>=instant(review.expiresAt)
  ||review.legs.some(l=>excludedExchangeIds.includes(l.exchangeId))||plan.comparisonSampling
  ||plan.basketMeasurement?.marketIds?.length||plan.paperPackageFocus?.marketIds?.length)return plan;
 need(Array.isArray(plan.reads)&&plan.reads.length<=4,'READ_BUDGET');
 const eligible=new Set(['discovery','comparison','edge_observation']);
 const protectedReads=plan.reads.filter(r=>!eligible.has(r.reason));
 const ids=new Set(protectedReads.map(r=>r.marketId));
 if(new Set([...ids,...review.legs.map(l=>l.marketId)]).size>plan.reads.length)return plan;
 // Retain all risk reads and their order. Keep the selected pair contiguous
 // after protected reads; replace slots instead of appending network traffic.
 const pair=review.legs.filter(l=>!ids.has(l.marketId)).map(l=>({marketId:l.marketId,exchangeId:l.exchangeId,
  option:'YES',path:'supplementalDepth',reason:'conditional_capacity_pair'}));
 const reads=[...protectedReads,...pair];
 for(const r of plan.reads)if(reads.length<plan.reads.length&&!reads.some(x=>x.marketId===r.marketId))reads.push(r);
 const selected=new Set(review.legs.map(l=>l.marketId));
 return{...plan,reads,entryMarketIds:(plan.entryMarketIds||[]).filter(id=>reads.some(r=>r.marketId===id)&&!selected.has(id)),
  canonicalCapacityFocus:{marketIds:[...selected],replacedSlots:pair.length,requestsAdded:0,executionAuthority:false}};
}
const startState=()=>({sequence:0,previous:null,counts:{},lastPair:null,last:null});
export function openCanonicalCapacityMonitor({runtimeDir,universe,now=Date.now}={}){
 const activeFile=path.join(runtimeDir,'canonical-capacity-active.json');if(!fs.existsSync(activeFile))return null;
 const active=JSON.parse(fs.readFileSync(activeFile));need(/^[a-z][a-z0-9-]{1,63}$/.test(active.cohort),'DIRECTORY');
 const dir=path.join(runtimeDir,'canonical-capacity-cohorts',active.cohort),file=path.join(dir,'bank.json'),journal=path.join(dir,'journal.jsonl');
 const bankBytes=fs.readFileSync(file),bank=JSON.parse(bankBytes),bankHash=digest(bank),codeIdentity=capacityCodeIdentity();
 need(bankHash===active.bankHash&&digest(codeIdentity)===digest(bank.codeIdentity)&&digest(bank.policy)===digest(CANONICAL_NO_POLICY),'IDENTITY');
 validateCapacityReview(bank.review);instant(bank.cutoff);
 const source=fs.readFileSync(path.join(runtimeDir,'public-books.jsonl')),prefix=source.subarray(0,bank.sourcePrefixBytes);
 need(Number.isSafeInteger(bank.sourcePrefixBytes)&&bank.sourcePrefixBytes>0&&prefix.length===bank.sourcePrefixBytes&&prefix.at(-1)===10
  &&sha(prefix)===bank.sourcePrefixHash,'SOURCE_PREFIX');
 const lastPrefix=JSON.parse(prefix.toString().split('\n').at(-2));
 need(lastPrefix.sequence===bank.sourcePrefixSequence&&lastPrefix.hash===bank.sourcePrefixJournalHash,'SOURCE_PREFIX');
 need(bank.tournamentId===universe.tournamentId&&bank.review.tournamentId===universe.tournamentId
  &&bank.review.legs.every(l=>universe.exchanges.some(x=>x.marketId===l.marketId&&x.exchangeId===l.exchangeId&&x.option==='YES')),'SCOPE');
 let state=startState();state.previous=bankHash;
 const reduce=row=>{const {hash,...body}=row;need(body.sequence===state.sequence+1&&body.previous===state.previous&&body.bankHash===bankHash&&digest(body)===hash,'JOURNAL');
  need(body.kind==='capacity'&&body.result?.reviewHash===digest(bank.review)&&body.result?.policyHash===digest(bank.policy),'RESULT');
  state.sequence=body.sequence;state.previous=hash;state.lastPair=body.pair;state.last=body.result;
  state.counts[body.result.reason]=(state.counts[body.result.reason]||0)+1;
 };
 const raw=fs.readFileSync(journal);need(!raw.length||raw.at(-1)===10,'PARTIAL_JOURNAL');
 for(const line of raw.toString().split('\n').filter(Boolean))reduce(JSON.parse(line));
 let journalBytes=raw.length,journalHash=sha(raw),failed=false;
 const stopped=()=>['STOP','COLLECTOR_STOP'].some(n=>fs.existsSync(path.join(runtimeDir,n)))||fs.existsSync(path.join(dir,'STOP'));
 const verify=()=>{need(!failed&&!stopped(),'STOPPED');need(sha(fs.readFileSync(file))===sha(bankBytes)&&digest(capacityCodeIdentity())===digest(codeIdentity),'IDENTITY_CHANGED');
  const bytes=fs.readFileSync(journal);need(bytes.length===journalBytes&&sha(bytes)===journalHash,'JOURNAL_CHANGED');};
 const status=()=>({schemaVersion:1,mode:'canonical_no_capacity_observation',cohort:active.cohort,bankHash,updatedAt:new Date(now()).toISOString(),
  failed,sourceSequence:state.lastPair?.sourceSequence??bank.sourcePrefixSequence,journalSequence:state.sequence,journalHash:state.previous,
  counts:state.counts,last:state.last,executionAuthority:false,competitionOrders:0,networkRequests:0,profitabilityEstablished:false});
 const inspect=publication=>{try{verify();if(publication.closed)return false;
  const books=bank.review.legs.map(l=>publication.books[l.exchangeId+':YES']);if(books.some(b=>!b||b.publicationSequence<=bank.sourcePrefixSequence||instant(b.requestStartedAt)<instant(bank.cutoff)))return false;
  const prior=state.lastPair;if(prior&&books.some((b,i)=>b.versionKey===prior.versions[i]||instant(b.requestStartedAt)<=instant(prior.lastReceiptAt)))return false;
  const at=new Date(now()).toISOString();let account=null;try{account=probabilityAccountEligibility(JSON.parse(fs.readFileSync(path.join(runtimeDir,'official-account.json'))));}catch{}
  const result=evaluateCanonicalNoCapacity({review:bank.review,policy:bank.policy,books,account,now:at});
  const pair={sourceSequence:publication.sequence,sourceHash:publication.hash,versions:books.map(b=>b.versionKey),lastReceiptAt:books.map(b=>b.receivedAt).sort().at(-1)};
  const body={sequence:state.sequence+1,previous:state.previous,bankHash,kind:'capacity',at,pair,accountEligibility:account,result},row={...body,hash:digest(body)},bytes=Buffer.from(JSON.stringify(row)+'\n');
  const fd=fs.openSync(journal,'a',0o600);try{let n=0;while(n<bytes.length){const wrote=fs.writeSync(fd,bytes,n,bytes.length-n);need(wrote>0,'SHORT_WRITE');n+=wrote;}fs.fsyncSync(fd);}finally{fs.closeSync(fd);}
  journalBytes+=bytes.length;journalHash=sha(fs.readFileSync(journal));reduce(row);atomicJson(path.join(dir,'stats.json'),status());return true;
 }catch(e){failed=true;throw e;}};
 const plan=(base,context)=>{verify();return planCapacityPair(base,bank.review,context);};
 return{bank,bankHash,dir,inspect,plan,status};
}
