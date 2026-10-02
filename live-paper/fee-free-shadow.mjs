import fs from 'node:fs';import path from 'node:path';import {fileURLToPath} from 'node:url';
import {verifySource} from './ml-shadow.mjs';import {scopedSnapshots} from './depth-scope.mjs';
import {digest,initialState,normalizeBook,observe,invalidate,sampleEquity,report,assertState} from './model.mjs';
import {atomicJson,lock} from './runtime.mjs';
export function createFeeFree(raw,c){
 if(c.feeBps!==0)throw Error('Fee-free baseline requires zero fees');const records=verifySource(raw);if(!records?.length)throw Error('Complete source required');const last=records.at(-1),cutoff=last.event.clockAt||last.at;
 const bank={schemaVersion:1,name:'fee-free-baseline',config:structuredClone(c),sourceRecords:records.length,sourceHash:records.prefixHashes.at(-1),cutoff};
 return {bank,state:{schemaVersion:1,bankHash:digest(bank),sourceSequence:records.length,lastRecordHash:last.hash,wallet:initialState(c,bank.name,cutoff)}};
}
export function advanceFeeFree(input,records,bank){
 const s=structuredClone(input),c=bank.config;if(c.feeBps!==0||s.bankHash!==digest(bank)||records?.prefixHashes?.[bank.sourceRecords-1]!==bank.sourceHash||records[s.sourceSequence-1]?.hash!==s.lastRecordHash)throw Error('Fee-free source/bank changed');assertState(s.wallet,c);
 const revoke=market=>{for(const st of Object.values(s.wallet.strategies))for(const [id,p]of Object.entries(st.pending))if(p.action==='buy'&&(market===null||s.wallet.books[id]?.marketId===market)){delete st.pending[id];st.expiredIntents++;}};
 for(const r of records){if(r.sequence<=s.sourceSequence)continue;s.sourceSequence=r.sequence;s.lastRecordHash=r.hash;const e=r.event,at=e.clockAt||r.at;
  if(e.type==='account_read_outage'){invalidate(s.wallet,null,at,'account_outage');revoke(null);continue;}if(e.type!=='cycle')continue;
  for(const snap of scopedSnapshots(e,c)){
   if(snap.skipped){invalidate(s.wallet,snap.marketId,snap.at||at,snap.skipped);revoke(snap.marketId);continue;}
   const m=/^\/markets\/(\d+)\/orderbook\?/.exec(snap.endpoint||'');if(!m||!c.marketIds.includes(m[1])||Date.parse(snap.requestStartedAt)<=Date.parse(bank.cutoff))continue;
   let b;try{b=normalizeBook(snap.data,m[1],c.tournamentId,snap.receivedAt,snap.requestStartedAt,c.maxDataAgeMs,new Set(e.accountExclusions));}catch{invalidate(s.wallet,m[1],snap.receivedAt,'invalid_book');revoke(m[1]);continue;}
   s.wallet=observe(s.wallet,b,c);
  }sampleEquity(s.wallet,c,at);
 }assertState(s.wallet,c);return s;
}
export function feeFreeReport(s,bank,at){const r=report(s.wallet,bank.config,at);return {...r,assumptions:{...r.assumptions,feeStatus:'human_requested_zero_additional_explicit_fee_baseline'},mode:'prospective_fee_free_paper_baseline',feeScenario:'zero_additional_explicit_fee_baseline',cutoff:bank.cutoff,bankHash:s.bankHash,sourceSequence:s.sourceSequence,lastRecordHash:s.lastRecordHash,networkRequests:0,competitionOrders:0};}
if(process.argv[1]&&fileURLToPath(import.meta.url)===process.argv[1]){
 const dir=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),workspace=path.dirname(path.dirname(fileURLToPath(import.meta.url))),release=lock(path.join(dir,'fee-free-shadow.lock'));process.on('exit',release);
 const bank=JSON.parse(fs.readFileSync(path.join(dir,'fee-free-bank.json'))),statePath=path.join(dir,'fee-free-state.json');let s=JSON.parse(fs.readFileSync(statePath)),partial=0;
 const stopped=()=>['STOP','FEE_FREE_STOP'].some(f=>fs.existsSync(path.join(dir,f)))||fs.existsSync(path.join(workspace,'STOP'));
 const publish=(status)=>atomicJson(path.join(dir,'fee-free-stats.json'),{...feeFreeReport(s,bank,new Date().toISOString()),status});
 try{do{if(stopped())break;const records=verifySource(fs.readFileSync(path.join(dir,'journal.jsonl'),'utf8'));if(!records){if(++partial>=4)throw Error('Incomplete source');publish('awaiting_capture');}else{partial=0;s=advanceFeeFree(s,records,bank);atomicJson(statePath,s);publish('running');}if(process.argv.includes('--once'))break;for(let i=0;i<75;i++){if(stopped())break;await new Promise(r=>setTimeout(r,200));}}while(!stopped());if(stopped())publish('stopped');}catch{publish('failed');process.exitCode=1;}
}
