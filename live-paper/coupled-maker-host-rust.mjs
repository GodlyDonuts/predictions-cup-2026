// Transport-only continuation: exact original bank, frozen reducers, journal and source frontiers.
// No economic policy or fill equation changes, and no original archive edits.
import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {openNativeJournal} from './native-journal-restore.mjs';import {projectNativeSource} from './native-source-projection.mjs';import {frozenPaperModule} from './frozen-paper-runtime.mjs';import {atomicJson,payloadHash} from '../execution/ledger.mjs';import {journalLines} from './journal-lines.mjs';
import {quoteProjectionRecoveryRequests} from './quote-projection-recovery.mjs';
import {collectFrozenPaperFiles} from './frozen-paper-graph.mjs';import {fileURLToPath} from 'node:url';
import * as currentReducer from './coupled-maker-paper-v2.mjs';import * as currentOriginalHost from './coupled-maker-host-v2.mjs';
const need=(v,c)=>{if(!v)throw Error('COUPLED_BULK_HOST_'+c);},sha=b=>createHash('sha256').update(b).digest('hex');
export const coupledRustTransportCodeIdentity=()=>collectFrozenPaperFiles(path.dirname(path.dirname(fileURLToPath(import.meta.url))),['live-paper/coupled-maker-host-rust.mjs']);
function prefixHash(file,length){const hash=createHash('sha256'),fd=fs.openSync(file,'r'),chunk=Buffer.alloc(1024*1024);let offset=0,last;
 try{while(offset<length){const read=fs.readSync(fd,chunk,0,Math.min(chunk.length,length-offset),offset);need(read>0,'SOURCE_PREFIX');hash.update(chunk.subarray(0,read));last=chunk[read-1];offset+=read;}}finally{fs.closeSync(fd);}
 need(length===0||last===10,'SOURCE_PREFIX_LINE');return hash.digest('hex');}
export async function openCoupledBulkRustHost({runtimeDir,nativeBackend,now=Date.now,canOperate=()=>true,signal}){
 const activeFile=path.join(runtimeDir,'coupled-bulk-active.json');if(!fs.existsSync(activeFile))return null;
 const activeBytes=fs.readFileSync(activeFile),active=JSON.parse(activeBytes);need(active.schemaVersion===1&&/^[a-z][a-z0-9-]{1,70}$/.test(active.cohort),'ACTIVE');
 const dir=path.join(runtimeDir,'coupled-bulk-cohorts',active.cohort),bankFile=path.join(dir,'bank.json'),bankBytes=fs.readFileSync(bankFile),bank=JSON.parse(bankBytes),stopFile=path.join(dir,'STOP');
 const original=await frozenPaperModule({runtimeDir,bank,modulePath:'live-paper/coupled-maker-host-v2.mjs',fallback:currentOriginalHost});need(payloadHash(bank)===active.bankHash&&payloadHash(bank.codeIdentity)===payloadHash(original.coupledBulkCodeIdentity()),'FROZEN');
 const reducer=await frozenPaperModule({runtimeDir,bank,modulePath:'live-paper/coupled-maker-paper-v2.mjs',fallback:currentReducer});const {createCoupledBulkState,advanceCoupledBulk,coupledBulkReport,coupledBulkCandidates}=reducer;
 const guard=()=>canOperate()===true&&!signal?.aborted&&!fs.existsSync(stopFile)&&!fs.existsSync(path.join(runtimeDir,'STOP'))&&sha(fs.readFileSync(activeFile))===sha(activeBytes)&&sha(fs.readFileSync(bankFile))===sha(bankBytes);
 need(nativeBackend&&guard(),'STOPPED');
 const sources={tape:{file:path.join(runtimeDir,'anonymous-trades.jsonl'),prefix:bank.tapePrefix},prices:{file:path.join(runtimeDir,'market-prices.jsonl'),prefix:bank.pricePrefix}};
 for(const source of Object.values(sources)){need(source.prefix&&Number.isSafeInteger(source.prefix.bytes)&&source.prefix.bytes>=0&&prefixHash(source.file,source.prefix.bytes)===source.prefix.hash,'SOURCE_PREFIX');}
 const journal=await openNativeJournal({runtimeDir,binding:nativeBackend,file:path.join(dir,'journal.jsonl'),receiptDir:path.join(dir,'rust-transport-receipts','wallet'),signal,canOperate:guard}),statsFile=path.join(dir,'stats.json');let failed=false,state=journal.last?.state||{paper:createCoupledBulkState(bank),frontiers:Object.fromEntries(Object.entries(sources).map(([key,s])=>[key,{sequence:s.prefix.sequence,hash:s.prefix.tailHash}]))};
 need(state.paper.bankHash===active.bankHash,'STATE');for(const[key,source]of Object.entries(sources))need(state.frontiers[key]?.sequence>=source.prefix.sequence,'FRONTIER');
 if(fs.existsSync(statsFile)){const prior=JSON.parse(fs.readFileSync(statsFile));need(prior.bankHash===active.bankHash&&prior.journalSequence<=journal.sequence,'PUBLISHED_ROLLBACK');if(prior.journalSequence===journal.sequence)need(prior.journalHash===journal.previous,'PUBLISHED_TAIL');}
 const time=()=>new Date(Math.max(now(),Date.parse(state.paper.lastAt))).toISOString();
 const status=()=>({...coupledBulkReport(state.paper,bank,time()),failed,lastEventAt:state.paper.lastAt,journalSequence:journal.sequence,journalHash:journal.previous,sourceFrontiers:state.frontiers,
  riskMarketIds:[...new Set(Object.values(state.paper.wallets).flatMap(w=>[...Object.values(w.positions).map(p=>p.marketId),...Object.values(w.quotes).filter(q=>!q.resolvedAt).map(q=>q.marketId)]))],
  projectionRecoveryRequests:quoteProjectionRecoveryRequests(state.paper,bank,time())});
 const allowed=()=>!failed&&canOperate()&&!fs.existsSync(stopFile);
 function verify(allowStopped=false){need(!failed&&(allowStopped||allowed()),'STOPPED');need(sha(fs.readFileSync(activeFile))===sha(activeBytes)&&sha(fs.readFileSync(bankFile))===sha(bankBytes),'BANK_CHANGED');}
 function append(next,event,allowStopped=false){verify(allowStopped);try{journal.commit(next,event);state=next;atomicJson(statsFile,status());}catch{failed=true;throw Error('COUPLED_BULK_HOST_DURABILITY');}}
 function consume(key,row){const frontier=state.frontiers[key];need(row.sequence===frontier.sequence+1&&row.previous===frontier.hash,'FRONTIER');const{hash,...body}=row;need(sha(JSON.stringify(body))===hash&&(key!=='prices'||row.state===null),'SOURCE_CHAIN');
  const availableAt=time();let paper=state.paper,relevant=true;
  if(key==='prices'){need(row.event?.type==='market_price_observation','PRICE_SOURCE');paper=advanceCoupledBulk(paper,{kind:'prices',availableAt,snapshot:row.event.snapshot},bank);}
  else{relevant=bank.legs.some(l=>l.exchangeId===row.event.exchangeId&&l.marketId===row.event.marketId);if(relevant)paper=advanceCoupledBulk(paper,{kind:'trades',availableAt,page:row.event},bank);}
  append({...state,paper,frontiers:{...state.frontiers,[key]:{sequence:row.sequence,hash}}},{kind:'source_'+key,availableAt,sourceSequence:row.sequence,sourceHash:hash,relevant});
 }
 const restoreMetrics={wallet:journal.nativeRestore,sources:{}};
 for(const[key,source]of Object.entries(sources)){
  const projected=await projectNativeSource({runtimeDir,binding:nativeBackend,file:source.file,frontier:state.frontiers[key],receiptDir:path.join(dir,'rust-transport-receipts',key),signal,canOperate:guard});
  for(const line of journalLines(projected.suffixFile)){need(canOperate()===true&&!signal?.aborted&&!fs.existsSync(stopFile),'STOPPED');consume(key,JSON.parse(line));}
  need(state.frontiers[key].sequence===projected.records&&state.frontiers[key].hash===projected.tailHash,'SOURCE_TRUNCATED');
  restoreMetrics.sources[key]={sourceBytes:projected.sourceBytes,sourceRecords:projected.records,suffixBytes:projected.projection.bytes,suffixRecords:projected.projection.records,verifyMs:projected.verifyMs};
 }
 need(guard(),'STOPPED');
 if(!journal.last)append(state,{kind:'genesis',cutoff:bank.cutoff});
 function callback(key,row){verify();const frontier=state.frontiers[key];if(row.sequence<=frontier.sequence){need(row.sequence===frontier.sequence&&row.hash===frontier.hash,'DUPLICATE');return;}consume(key,row);}
 return{bank,dir,status,rustTransport:Object.freeze(restoreMetrics),focus(){const s=status();return{...s,candidateMarketIds:allowed()&&now()<Date.parse(bank.entryExpiresAt)?coupledBulkCandidates(state.paper,bank,time()).map(x=>x.marketId):[]};},
  onPage:row=>callback('tape',row),onPrice:row=>callback('prices',row),
  onBook(publication,marketId,accountEligibility){verify();const leg=bank.legs.find(l=>l.marketId===marketId);if(!leg)return;const book=publication.books[leg.exchangeId+':YES'];if(!book)return;const availableAt=time();append({...state,paper:advanceCoupledBulk(state.paper,{kind:'book',availableAt,book,accountEligibility},bank)},{kind:'book',availableAt,sourceSequence:publication.sequence,sourceHash:publication.hash,marketId});},
  boundary(reason){verify(true);const availableAt=time();append({...state,paper:advanceCoupledBulk(state.paper,{kind:'boundary',availableAt},bank)},{kind:'boundary',reason,availableAt},true);}
 };
}
