import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {createStreamBookAdapter,advanceStreamBookAdapter,streamDigest} from './stream-book-adapter-v2.mjs';
import {replayStreamDip} from './stream-dip-replay-v2.mjs';
const sha=x=>createHash('sha256').update(x).digest('hex'),base=Date.parse('2026-10-02T15:00:00.000Z'),at=n=>new Date(base+n).toISOString(),tid='bda92870-621e-47b0-bc3c-3602c5c26f55';
function fixture(){
 const topic='tournament:'+tid+':market:377',raw=(seq,n)=>({exchangeId:'1066',asOf:{sequence:seq,at:at(n)},nextExpiryAt:null,expiryKnown:true,bids:[{price:.40,quantity:1000}],asks:[{price:.42,quantity:1000}]}),
  row=(i,kind,n,extra)=>({format:'sig-feed-capture',schemaVersion:1,captureIndex:i,topic,epoch:1,kind,observedAt:at(n),...extra});
 const rows=[row(0,'recovery_started',100,{generation:1}),row(1,'snapshot',200,{receivedAt:at(190),requestStartedAt:at(100),books:[raw(10,180)]}),row(2,'recovery_finished',210,{booksReady:true,generation:1}),
  row(3,'event',600,{receivedAt:at(599),event:'market_batch',payload:{delivery:{model:'best-effort-authoritative-resync',revision:1,previousRevision:0,sourceSequenceFrom:null,sourceSequenceThrough:null},books:[raw(11,598)],trades:[],bookDirty:[],marketSettled:[],resyncRequired:false}})];
 const bytes=rows.map(x=>Buffer.from(JSON.stringify(x)+'\n')),prefix=Buffer.concat(bytes.slice(0,3)),source=Buffer.concat(bytes),inputBank={schemaVersion:1,mode:'verified_stream_book_input_v2',scope:{tournamentId:tid,markets:[{marketId:'377',exchangeId:'1066'}]},cutoff:at(500),sourceHash:sha(prefix),sessionId:'fixture',sourcePrefixRecords:3,sourcePrefixBytes:prefix.length};
 let state=createStreamBookAdapter(inputBank);for(const r of rows.slice(0,3))state=advanceStreamBookAdapter(state,r,inputBank,{availableAt:at(500)}).state;
 const first={sequence:1,previous:streamDigest(inputBank),bankHash:streamDigest(inputBank),kind:'prefix',stateHash:streamDigest(state)},checkpoints=[{...first,hash:streamDigest(first)}];
 const out=advanceStreamBookAdapter(state,rows[3],inputBank,{availableAt:at(700)}),second={sequence:2,previous:checkpoints[0].hash,bankHash:streamDigest(inputBank),kind:'capture',availableAt:at(700),sourceCaptureIndex:3,sourceLineHash:sha(bytes[3]),events:out.events,stateHash:streamDigest(out.state)};
 checkpoints.push({...second,hash:streamDigest(second)});return {inputBank,source,checkpoints};
}
test('full source and gateway state replay precede development wallet evidence',()=>{
 const f=fixture(),a=replayStreamDip(f),b=replayStreamDip(f);assert.equal(a.report.finalStateHash,b.report.finalStateHash);
 assert.equal(a.report.warmBooks,1);assert.equal(a.report.inputCheckpointCount,2);assert.equal(a.report.observations,1);
 assert.equal(a.report.wallets.latency_1000.orders,0);assert.equal(a.report.networkRequests,0);
});
test('changed historical prefix and rehashed derived events fail against authoritative capture',()=>{
 const f=fixture(),source=Buffer.from(f.source);source[20]^=1;assert.throws(()=>replayStreamDip({...f,source}),/REPLAY_PREFIX/);
 const altered=structuredClone(f.checkpoints),last=altered.at(-1);last.events=[];const {hash:unused,...body}=last;last.hash=streamDigest(body);
 assert.throws(()=>replayStreamDip({...f,checkpoints:altered}),/REPLAY_EVENTS/);
});
