import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {fileURLToPath} from 'node:url';
import {Journal} from './transport-runtime.mjs';import {atomicJson,payloadHash} from '../execution/ledger.mjs';import {journalLines} from './journal-lines.mjs';
import {collectFrozenPaperFiles} from './frozen-paper-graph.mjs';import {quoteProjectionRecoveryRequests} from './quote-projection-recovery.mjs';
import {createCoupledBulkState,advanceCoupledBulk,coupledBulkReport,coupledBulkCandidates} from './coupled-maker-paper-v2.mjs';
const need=(v,c)=>{if(!v)throw Error('COUPLED_BULK_HOST_'+c);},sha=b=>createHash('sha256').update(b).digest('hex'),here=path.dirname(fileURLToPath(import.meta.url));
export function coupledBulkCodeIdentity(){return Object.fromEntries(Object.entries(collectFrozenPaperFiles(path.dirname(here),['live-paper/coupled-maker-host-v2.mjs'])).map(([f,h])=>[path.posix.relative('live-paper',f),h]));}
function prefixHash(file,length){const hash=createHash('sha256'),fd=fs.openSync(file,'r'),chunk=Buffer.alloc(1024*1024);let offset=0,last;
 try{while(offset<length){const read=fs.readSync(fd,chunk,0,Math.min(chunk.length,length-offset),offset);need(read>0,'SOURCE_PREFIX');hash.update(chunk.subarray(0,read));last=chunk[read-1];offset+=read;}}finally{fs.closeSync(fd);}
 need(length===0||last===10,'SOURCE_PREFIX_LINE');return hash.digest('hex');}
export function openCoupledBulkHost({runtimeDir,now=Date.now,canOperate=()=>true}){
 const activeFile=path.join(runtimeDir,'coupled-bulk-active.json');if(!fs.existsSync(activeFile))return null;
 const activeBytes=fs.readFileSync(activeFile),active=JSON.parse(activeBytes);need(active.schemaVersion===1&&/^[a-z][a-z0-9-]{1,70}$/.test(active.cohort),'ACTIVE');
 const dir=path.join(runtimeDir,'coupled-bulk-cohorts',active.cohort),bankFile=path.join(dir,'bank.json'),bankBytes=fs.readFileSync(bankFile),bank=JSON.parse(bankBytes),stopFile=path.join(dir,'STOP');
 need(payloadHash(bank)===active.bankHash&&payloadHash(bank.codeIdentity)===payloadHash(coupledBulkCodeIdentity()),'FROZEN');
 const sources={tape:{file:path.join(runtimeDir,'anonymous-trades.jsonl'),prefix:bank.tapePrefix},prices:{file:path.join(runtimeDir,'market-prices.jsonl'),prefix:bank.pricePrefix}};
 for(const source of Object.values(sources)){need(source.prefix&&Number.isSafeInteger(source.prefix.bytes)&&source.prefix.bytes>=0&&prefixHash(source.file,source.prefix.bytes)===source.prefix.hash,'SOURCE_PREFIX');}
 const journal=new Journal(path.join(dir,'journal.jsonl')),statsFile=path.join(dir,'stats.json');let failed=false,state=journal.last?.state||{paper:createCoupledBulkState(bank),frontiers:Object.fromEntries(Object.entries(sources).map(([key,s])=>[key,{sequence:s.prefix.sequence,hash:s.prefix.tailHash}]))};
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
 for(const[key,source]of Object.entries(sources)){let sequence=0,previous='',frontierSeen=state.frontiers[key].sequence===0&&state.frontiers[key].hash==='';
  for(const line of journalLines(source.file)){const row=JSON.parse(line),{hash,...body}=row;need(row.sequence===++sequence&&row.previous===previous&&sha(JSON.stringify(body))===hash,'SOURCE_CHAIN');previous=hash;
   if(row.sequence===state.frontiers[key].sequence){need(hash===state.frontiers[key].hash,'FRONTIER_CHANGED');frontierSeen=true;}
   if(row.sequence>state.frontiers[key].sequence)consume(key,row);
  }need(frontierSeen&&sequence>=state.frontiers[key].sequence,'SOURCE_TRUNCATED');
 }
 if(!journal.last)append(state,{kind:'genesis',cutoff:bank.cutoff});
 function callback(key,row){verify();const frontier=state.frontiers[key];if(row.sequence<=frontier.sequence){need(row.sequence===frontier.sequence&&row.hash===frontier.hash,'DUPLICATE');return;}consume(key,row);}
 return{bank,dir,status,focus(){const s=status();return{...s,candidateMarketIds:allowed()&&now()<Date.parse(bank.entryExpiresAt)?coupledBulkCandidates(state.paper,bank,time()).map(x=>x.marketId):[]};},
  onPage:row=>callback('tape',row),onPrice:row=>callback('prices',row),
  onBook(publication,marketId,accountEligibility){verify();const leg=bank.legs.find(l=>l.marketId===marketId);if(!leg)return;const book=publication.books[leg.exchangeId+':YES'];if(!book)return;const availableAt=time();append({...state,paper:advanceCoupledBulk(state.paper,{kind:'book',availableAt,book,accountEligibility},bank)},{kind:'book',availableAt,sourceSequence:publication.sequence,sourceHash:publication.hash,marketId});},
  boundary(reason){verify(true);const availableAt=time();append({...state,paper:advanceCoupledBulk(state.paper,{kind:'boundary',availableAt},bank)},{kind:'boundary',reason,availableAt},true);}
 };
}
