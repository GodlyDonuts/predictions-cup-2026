import fs from 'node:fs';import path from 'node:path';import {createHash,randomUUID} from 'node:crypto';
import {sealedNativePrefix} from './native-dip-host-v1.mjs';
import {readNativePublicationPrefix} from './rust-backend.mjs';
import {createStableMakerState,advanceStableMaker,stableMakerReport} from './stable-maker-paper-v2.mjs';
const need=(v,c)=>{if(!v)throw Error('STABLE_MAKER_REPLAY_'+c);},sha=b=>createHash('sha256').update(b).digest('hex');
export function verifyChainedRows(bytes){need(bytes.length===0||bytes.at(-1)===10,'PARTIAL_RECORD');let previous='',sequence=0,at=null;const rows=[];
 for(const l of bytes.toString().trimEnd().split('\n').filter(Boolean)){const row=JSON.parse(l),{hash,...body}=row;need(row.sequence===++sequence&&row.previous===previous&&sha(JSON.stringify(body))===hash,'CHAIN');need(Number.isFinite(Date.parse(row.at))&&(at===null||Date.parse(row.at)>=at),'CLOCK');at=Date.parse(row.at);previous=hash;rows.push(row);}return rows;
}
export async function loadStableMakerSources({runtimeDir,workingDir,executable,binaryHash}){
 fs.mkdirSync(workingDir,{recursive:true,mode:0o700});const names={books:'public-books.jsonl',eligibility:'account-eligibility.jsonl',trades:'anonymous-trades.jsonl'},buffers={},bindings={};
 for(const[name,file]of Object.entries(names)){const target=path.join(runtimeDir,file),prefix=sealedNativePrefix(target),bytes=fs.readFileSync(target).subarray(0,prefix.bytes);need(bytes.length===prefix.bytes,'TRUNCATED');buffers[name]=bytes;bindings[name]={bytes:prefix.bytes,hash:sha(bytes),dev:prefix.dev,ino:prefix.ino};}
 const books=buffers.books.toString().trimEnd().split('\n').map(JSON.parse),scope=books[0]?.payload?.scope;need(books[0]?.kind==='session_start'&&scope?.exchanges?.length,'SCOPE');
 const scopeFile=path.join(workingDir,'scope.json');fs.writeFileSync(scopeFile,JSON.stringify(scope)+'\n',{flag:'wx',mode:0o600});
 const verified=await readNativePublicationPrefix({journal:path.join(runtimeDir,names.books),scope:scopeFile,endByte:bindings.books.bytes,expectedSha:bindings.books.hash,out:path.join(workingDir,'publication.json'),executable,pinnedBinaryHash:binaryHash});
 need(verified.sourceRecords===books.length,'BOOK_RECORDS');const eligibility=verifyChainedRows(buffers.eligibility),trades=verifyChainedRows(buffers.trades);
 for(const[name,file]of Object.entries(names)){const stat=fs.statSync(path.join(runtimeDir,file)),b=bindings[name];need(stat.dev===b.dev&&stat.ino===b.ino&&stat.size>=b.bytes,'SOURCE_REPLACED');}
 return{scope,books,eligibility,trades,bindings,nativeVerification:{engineHash:verified.engineHash,binaryHash,bookRecords:verified.sourceRecords}};
}
export function stableMakerEvents(sources,bank){const mapping=new Map(sources.scope.exchanges.map(x=>[x.marketId,x.exchangeId])),scope=new Set(bank.legs.map(x=>x.exchangeId)),events=[];
 for(const row of sources.eligibility){const a=row.event?.snapshot;need(row.event?.type==='market_account_eligibility'&&a.complete===true&&a.tournamentId===bank.tournamentId&&a.excludedMarketIds.every(id=>mapping.has(id)),'ELIGIBILITY');events.push({kind:'eligibility',availableAt:row.at,snapshot:{schemaVersion:1,positionsAt:a.positionsAt,ordersAt:a.ordersAt,excludedExchangeIds:a.excludedMarketIds.map(id=>mapping.get(id))}});}
 for(const row of sources.books){if(row.kind==='book'&&scope.has(row.payload.book.exchangeId))events.push({kind:'book',availableAt:row.at,book:{...row.payload.book,publicationHash:row.hash,publicationSequence:row.sequence}});
  else if(row.kind==='session_start'||row.kind==='session_stop'||row.kind==='invalidation'&&(!row.payload.marketId||bank.legs.some(l=>l.marketId===row.payload.marketId)))events.push({kind:'boundary',availableAt:row.at,reason:row.kind});}
 for(const row of sources.trades){const page=row.event;if(scope.has(page.exchangeId)){need(page.type==='anonymous_trade_page'&&page.tournamentId===bank.tournamentId,'TRADE_SCOPE');events.push({kind:'trades',availableAt:row.at,page});}}
 events.sort((a,b)=>Date.parse(a.availableAt)-Date.parse(b.availableAt)||(a.kind==='eligibility'?-1:b.kind==='eligibility'?1:0));return events;
}
export function replayStableMakerEvents(events,bank){let state=createStableMakerState(bank),accounts=[],processed=0;const cutoff=Date.parse(bank.cutoff);
 for(const e of events){if(e.kind==='eligibility'){accounts.push(e);continue;}if(Date.parse(e.availableAt)<cutoff)continue;
  if(e.kind==='book'){const request=Date.parse(e.book.requestStartedAt),causal=accounts.filter(x=>Date.parse(x.snapshot.positionsAt)<=request&&Date.parse(x.snapshot.ordersAt)<=request&&Date.parse(x.availableAt)<=Date.parse(e.availableAt)).at(-1);state=advanceStableMaker(state,{...e,accountEligibility:causal?.snapshot||null},bank);}
  else state=advanceStableMaker(state,e,bank);processed++;
 }
 return{state,processed,report:stableMakerReport(state,bank,state.lastAt)};
}
export async function runStableMakerReplay({runtimeDir,bank,executable,binaryHash,out,workingDir=path.join(runtimeDir,'stable-maker-diagnostics',randomUUID())}){
 const started=performance.now(),sources=await loadStableMakerSources({runtimeDir,workingDir,executable,binaryHash}),events=stableMakerEvents(sources,bank),loaded=performance.now(),replay=replayStableMakerEvents(events,bank),finished=performance.now();
 const record={schemaVersion:1,at:new Date().toISOString(),mode:'stable_maker_actual_capture_replay_v2',bank,bindings:sources.bindings,nativeVerification:sources.nativeVerification,
  firstAt:events[0]?.availableAt,through:events.at(-1)?.availableAt,processed:replay.processed,timing:{sourceMs:loaded-started,replayMs:finished-loaded,totalMs:finished-started},report:replay.report,
  fills:Object.fromEntries(Object.entries(replay.state.wallets).map(([side,w])=>[side,w.fills])),networkRequests:0,competitionOrders:0,fees:0,profitabilityEstablished:false};
 fs.writeFileSync(out,JSON.stringify(record,null,2)+'\n',{flag:'wx',mode:0o600});return record;
}
