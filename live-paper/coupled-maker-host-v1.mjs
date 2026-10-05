import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {fileURLToPath} from 'node:url';
import {Journal} from './transport-runtime.mjs';import {atomicJson,payloadHash} from '../execution/ledger.mjs';import {journalLines} from './journal-lines.mjs';
import {collectFrozenPaperFiles} from './frozen-paper-graph.mjs';import {quoteProjectionRecoveryRequests} from './quote-projection-recovery.mjs';
import {createCoupledMakerState,advanceCoupledMaker,coupledMakerReport} from './coupled-maker-paper-v1.mjs';
const need=(v,c)=>{if(!v)throw Error('COUPLED_MAKER_HOST_'+c);},sha=b=>createHash('sha256').update(b).digest('hex'),here=path.dirname(fileURLToPath(import.meta.url));
export function coupledMakerCodeIdentity(){return Object.fromEntries(Object.entries(collectFrozenPaperFiles(path.dirname(here),['live-paper/coupled-maker-host-v1.mjs'])).map(([f,h])=>[path.posix.relative('live-paper',f),h]));}
export function mergeMakerTapeFocus(...focus){const riskMarketIds=[...new Set(focus.filter(Boolean).flatMap(x=>x.riskMarketIds||[]))],requests=new Map();
 for(const x of focus.filter(Boolean))for(const r of x.projectionRecoveryRequests||[])requests.set(r.key,r);
 return{riskMarketIds,projectionRecoveryRequests:[...requests.values()].sort((a,b)=>Date.parse(a.from)-Date.parse(b.from)||Number(a.marketId)-Number(b.marketId)).slice(0,4)};
}
export function openCoupledMakerHost({runtimeDir,now=Date.now,canOperate=()=>true,activeFileName='coupled-maker-active.json'}){
 need(activeFileName==='coupled-maker-active.json','ACTIVE_NAME');
 const activeFile=path.join(runtimeDir,activeFileName);if(!fs.existsSync(activeFile))return null;
 const activeBytes=fs.readFileSync(activeFile),active=JSON.parse(activeBytes);need(active.schemaVersion===1&&/^[a-z][a-z0-9-]{1,70}$/.test(active.cohort),'ACTIVE');
 const dir=path.join(runtimeDir,'coupled-maker-cohorts',active.cohort),bankBytes=fs.readFileSync(path.join(dir,'bank.json')),bank=JSON.parse(bankBytes),stopFile=path.join(dir,'STOP');
 need(payloadHash(bank)===active.bankHash&&payloadHash(bank.codeIdentity)===payloadHash(coupledMakerCodeIdentity()),'FROZEN');
 const sourceFile=path.join(runtimeDir,'anonymous-trades.jsonl'),prefix=fs.readFileSync(sourceFile).subarray(0,bank.tapePrefixBytes);
 need(prefix.length===bank.tapePrefixBytes&&(!prefix.length||prefix.at(-1)===10)&&sha(prefix)===bank.tapePrefixHash,'SOURCE_PREFIX');
 const journal=new Journal(path.join(dir,'journal.jsonl')),statsFile=path.join(dir,'stats.json');let failed=false,state=journal.last?.state||{paper:createCoupledMakerState(bank),tapeSequence:bank.tapePrefixSequence,tapeHash:bank.tapePrefixTailHash};
 need(state.paper.bankHash===active.bankHash&&state.tapeSequence>=bank.tapePrefixSequence,'STATE');
 if(fs.existsSync(statsFile)){const prior=JSON.parse(fs.readFileSync(statsFile));need(prior.bankHash===active.bankHash&&prior.journalSequence<=journal.sequence,'PUBLISHED_ROLLBACK');if(prior.journalSequence===journal.sequence)need(prior.journalHash===journal.previous,'PUBLISHED_TAIL');}
 const status=()=>({...coupledMakerReport(state.paper,bank,new Date(now()).toISOString()),failed,lastEventAt:state.paper.lastAt,journalSequence:journal.sequence,journalHash:journal.previous,tapeSequence:state.tapeSequence,
  riskMarketIds:[...new Set(Object.values(state.paper.wallets).flatMap(w=>[...Object.values(w.positions).map(p=>p.marketId),...Object.values(w.quotes).filter(q=>!q.resolvedAt).map(q=>q.marketId)]))],
  projectionRecoveryRequests:quoteProjectionRecoveryRequests(state.paper,bank,new Date(now()).toISOString())});
 const allowed=()=>!failed&&canOperate()&&!fs.existsSync(stopFile);
 function verify(allowStopped=false){need(!failed&&(allowStopped||allowed()),'STOPPED');need(sha(fs.readFileSync(activeFile))===sha(activeBytes)&&sha(fs.readFileSync(path.join(dir,'bank.json')))===sha(bankBytes),'BANK_CHANGED');}
 function append(next,event,allowStopped=false){verify(allowStopped);try{journal.commit(next,event);state=next;atomicJson(statsFile,status());}catch{failed=true;throw Error('COUPLED_MAKER_HOST_DURABILITY');}}
 function page(row,availableAt){need(row.sequence===state.tapeSequence+1&&row.previous===state.tapeHash,'FRONTIER');const{hash,...body}=row;need(sha(JSON.stringify(body))===hash,'SOURCE_CHAIN');const relevant=bank.legs.some(l=>l.exchangeId===row.event.exchangeId&&l.marketId===row.event.marketId);
  const paper=relevant?advanceCoupledMaker(state.paper,{kind:'trades',availableAt,page:row.event},bank):state.paper;append({...state,paper,tapeSequence:row.sequence,tapeHash:hash},{kind:'source_page',availableAt,tapeSequence:row.sequence,tapeHash:hash,relevant});}
 let sequence=0,previous='';for(const l of journalLines(sourceFile)){const row=JSON.parse(l),{hash,...body}=row;need(row.sequence===++sequence&&row.previous===previous&&sha(JSON.stringify(body))===hash,'SOURCE_CHAIN');previous=hash;
  if(row.sequence===state.tapeSequence)need(row.hash===state.tapeHash,'FRONTIER_CHANGED');if(row.sequence>state.tapeSequence)page(row,new Date(Math.max(now(),Date.parse(state.paper.lastAt))).toISOString());}
 need(sequence>=state.tapeSequence,'SOURCE_TRUNCATED');if(!journal.last)append(state,{kind:'genesis',cutoff:bank.cutoff});
 return{bank,dir,status,focus(){const s=status();return{...s,candidateMarketIds:allowed()&&now()<Date.parse(bank.entryExpiresAt)?bank.legs.map(l=>l.marketId):[]};},
  onPage(row){verify();if(row.sequence<=state.tapeSequence){need(row.sequence===state.tapeSequence&&row.hash===state.tapeHash,'DUPLICATE');return;}page(row,new Date(Math.max(now(),Date.parse(state.paper.lastAt))).toISOString());},
  onBook(publication,marketId,accountEligibility){verify();const leg=bank.legs.find(l=>l.marketId===marketId);if(!leg)return;const b=publication.books[leg.exchangeId+':YES'];if(!b)return;const at=new Date(now()).toISOString();append({...state,paper:advanceCoupledMaker(state.paper,{kind:'book',availableAt:at,book:b,accountEligibility},bank)},{kind:'book',availableAt:at,sourceSequence:publication.sequence,sourceHash:publication.hash,marketId});},
  boundary(reason){verify(true);const at=new Date(Math.max(now(),Date.parse(state.paper.lastAt))).toISOString();append({...state,paper:advanceCoupledMaker(state.paper,{kind:'boundary',availableAt:at},bank)},{kind:'boundary',reason,availableAt:at},true);}
 };
}
