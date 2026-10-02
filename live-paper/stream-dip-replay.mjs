import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {createStreamBookAdapter,advanceStreamBookAdapter,streamDigest} from './stream-book-adapter.mjs';
import {streamBookCodeIdentity} from './stream-book-watch.mjs';
import {STREAM_DIP_POLICY,createStreamDip,warmStreamDip,advanceStreamDip,streamDipReport,streamDipHash} from './stream-dip.mjs';
const hash=x=>createHash('sha256').update(x).digest('hex'),need=(x,m)=>{if(!x)throw Error(m);};

// A bounded, read-only development replay. It is not a prospective wallet and
// never substitutes recorded gateway availability for an actual live decision.
export function replayStreamDip({inputBank,source,checkpoints,markets=inputBank.scope.markets}){
 need(hash(source.subarray(0,inputBank.sourcePrefixBytes))===inputBank.sourceHash,'STREAM_DIP_REPLAY_PREFIX');
 let adapter=createStreamBookAdapter(inputBank),offset=0,index=0,previous=streamDigest(inputBank),checkpointCount=0,warm=[],state;
 const bank={schemaVersion:1,mode:'stream_native_dip_paper',tournamentId:inputBank.scope.tournamentId,inputBankHash:streamDigest(inputBank),
  cutoff:inputBank.cutoff,startEventId:0,markets:structuredClone(markets),policy:structuredClone(STREAM_DIP_POLICY)};
 const next=()=>{const end=source.indexOf(10,offset);need(end>=0,'STREAM_DIP_REPLAY_SOURCE_TRUNCATED');const raw=source.subarray(offset,end+1);offset=end+1;const row=JSON.parse(raw);
  need(row.captureIndex===index++,'STREAM_DIP_REPLAY_CAPTURE_INDEX');return {row,raw};};
 while(index<inputBank.sourcePrefixRecords){const {row}=next(),before=adapter.latest;adapter=advanceStreamBookAdapter(adapter,row,inputBank,{availableAt:inputBank.cutoff}).state;
  for(const [id,e]of Object.entries(adapter.latest))if(e!==before[id]&&(!before[id]||streamDigest(e)!==streamDigest(before[id])))warm.push(e);}
 state=warmStreamDip(createStreamDip(bank),warm,bank);
 let lastAt=bank.cutoff,firstHash=streamDipHash(state),events=0;
 for(const saved of checkpoints){const {hash:actual,...body}=saved;
  need(actual===streamDigest(body)&&body.sequence===++checkpointCount&&body.previous===previous&&body.bankHash===streamDigest(inputBank),'STREAM_DIP_REPLAY_CHAIN');previous=actual;
  if(body.kind==='prefix')need(checkpointCount===1&&body.stateHash===streamDigest(adapter),'STREAM_DIP_REPLAY_PREFIX_STATE');
  else{need(body.kind==='capture','STREAM_DIP_REPLAY_KIND');const {row,raw}=next();need(hash(raw)===body.sourceLineHash&&row.captureIndex===body.sourceCaptureIndex,'STREAM_DIP_REPLAY_SOURCE_HASH');
   const out=advanceStreamBookAdapter(adapter,row,inputBank,{availableAt:body.availableAt});need(streamDigest(out.events)===streamDigest(body.events),'STREAM_DIP_REPLAY_EVENTS');adapter=out.state;
   state=advanceStreamDip(state,out.events,bank,{availableAt:body.availableAt});events+=out.events.length;lastAt=body.availableAt;}
  need(body.stateHash===streamDigest(adapter),'STREAM_DIP_REPLAY_INPUT_STATE');
 }
 return {bank,state,warm,verifiedInputState:adapter,report:{...streamDipReport(state,bank,lastAt),mode:'retrospective_stream_dip_development_replay',
  sourcePrefixHash:inputBank.sourceHash,inputCheckpointCount:checkpointCount,inputJournalHash:previous,inputCaptureIndex:adapter.capture.captureIndex,
  sourceBytes:offset,eventsConsumed:events,initialStateHash:firstHash,finalStateHash:streamDipHash(state),
  evidence:'actual_capture_replayed_with_recorded_gateway_availability_not_a_live_strategy_run',
  limitations:['Replay used for correctness and diagnosis only; not held-out or prospective performance evidence.',
   'Gateway availability stands in for a consumer during this replay. Live wallets must persist their own actual consumption timestamps.']}};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 try{const args=process.argv.slice(2),value=k=>{const i=args.indexOf(k);return i<0?undefined:args[i+1];},runtime=value('--runtime')||path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),
  cohort=value('--input')||'stream-books-20261002',dir=path.join(runtime,'stream-book-cohorts',cohort);
  need(/^[a-z][a-z0-9-]{0,63}$/.test(cohort),'STREAM_DIP_REPLAY_NAME');
  const inputBank=JSON.parse(fs.readFileSync(path.join(dir,'bank.json')));need(streamDigest(inputBank.codeIdentity)===streamDigest(streamBookCodeIdentity()),'STREAM_DIP_REPLAY_INPUT_CODE');
  const raw=fs.readFileSync(path.join(dir,'journal.jsonl')),end=raw.lastIndexOf(10)+1;
  const checkpoints=raw.subarray(0,end).toString('utf8').trim().split('\n').map(JSON.parse),source=fs.readFileSync(path.join(runtime,inputBank.sourceFile));
  let identities={};try{const old=JSON.parse(fs.readFileSync(path.join(runtime,'limit-entry-cohorts/limit-pricecap-20261002/bank.json')));identities=old.identities||{};}catch{}
  const markets=inputBank.scope.markets.map(m=>({...m,...(identities[m.marketId]||{}),marketId:m.marketId,exchangeId:m.exchangeId}));
  const result=replayStreamDip({inputBank,source,checkpoints,markets}),report={...result.report,checkedAt:new Date().toISOString(),networkRequests:0,runtimeWrites:0};
  const output=value('--output');if(output)fs.writeFileSync(output,JSON.stringify(report,null,2)+'\n');
  process.stdout.write(JSON.stringify({status:'verified',at:report.checkedAt,checkpoints:report.inputCheckpointCount,captureIndex:report.inputCaptureIndex,
   warmBooks:report.warmBooks,minutes:Object.fromEntries(Object.entries(result.state.histories).map(([k,v])=>[k,v.length])),observations:report.observations,
   wallets:Object.fromEntries(Object.entries(report.wallets).map(([k,w])=>[k,{orders:w.orders,fills:w.fills,realizedPnl:w.realizedPnl,gateCounts:w.gateCounts}])),
   finalStateHash:report.finalStateHash,evidence:report.evidence})+'\n');
 }catch(e){const code=/^STREAM_[A-Z_]+$/.test(e.message)?e.message:'REPLAY_VALIDATION_FAILED';process.stderr.write('Stream dip development replay failed ('+code+'); no runtime files changed.\n');
  if(argsDebug())process.stderr.write((e.stack||'').split('\n').slice(1,5).filter(x=>x.includes('file:///')).join('\n')+'\n');process.exitCode=1;}
}
function argsDebug(){return process.argv.includes('--debug-location');}
