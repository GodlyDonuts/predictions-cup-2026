import fs from 'node:fs';import path from 'node:path';import readline from 'node:readline';import {fileURLToPath} from 'node:url';
import {createClient} from '@supabase/supabase-js';
import {config,parseEvent,parseSnapshot,compareVersion} from '../../feed/contract.mjs';
import {payloadHash} from '../../execution/ledger.mjs';
import {receiptEvents} from '../sig-source.mjs';
import {createHash} from 'node:crypto';import {collectFrozenPaperFiles} from '../../live-paper/frozen-paper-graph.mjs';
export function captureFailure(phase,error){
  const allowed=new Set(['command','rest','final_rest','channel','aggregate','broadcast','publication']);const code=/^STREAM_[A-Z0-9_]+$/.test(error?.message??'')?error.message:['EPERM','EACCES','EBUSY','ENOENT','ENOSPC','EIO'].includes(error?.code)?error.code:'UNCLASSIFIED';return{phase:allowed.has(phase)?phase:'unknown',code,...(['rename','open','read','write','stat'].includes(error?.syscall)?{operation:error.syscall}:{}),...(['stats.tmp','stats.json','receipts.jsonl','events.jsonl'].includes(path.basename(error?.path??''))?{file:path.basename(error.path)}:{})};
}
export function statsPublisher(out,io=fs){
  let retries=0;return report=>{const temp=path.join(out,'stats.tmp');io.writeFileSync(temp,JSON.stringify(report,null,2));try{io.renameSync(temp,path.join(out,'stats.json'));retries=0;return true;}catch(error){if(['EPERM','EACCES','EBUSY'].includes(error.code)&&++retries<=20)return false;throw error;}};
}
export class StreamEvidence {
  constructor(pairs,tournamentId,append){const grouped=new Map();for(const p of pairs){const marketId=String(p.marketId),ids=grouped.get(marketId)??[];ids.push(String(p.exchangeId));grouped.set(marketId,ids);}this.scopes=new Map([...grouped].map(([marketId,exchangeIds])=>[marketId,config({marketId,exchangeIds,tournamentId})]));this.append=append;this.revisions=new Map();this.books=new Map();this.seen=new Set();this.metrics={batches:0,trades:0,duplicateTrades:0,duplicateBatches:0,gaps:0,truncated:0,schemaErrors:0,restBooks:0,restTapes:0,pushedBooks:0,olderBooks:0,unknownMarkets:0,latencySamples:0,latencySumMs:0,latencyMaxMs:0};}
  aggregate(payload,receivedAt=new Date().toISOString()){
    if(!Array.isArray(payload?.markets)||payload.markets.length>1000)throw Error('STREAM_AGGREGATE_SCHEMA');
    for(const entry of payload.markets){const id=String(entry.marketId);if(!/^[1-9]\d*$/.test(id))throw Error('STREAM_AGGREGATE_MARKET');if(!this.scopes.has(id)){this.metrics.unknownMarkets++;this.append({source:'official_sig_realtime',kind:'unknown_market',marketId:id,receivedAt,requiresCatalogRefresh:true});continue;}this.batch(id,'market_batch',entry.batch,receivedAt);}
  }
  batch(marketId,event,raw,receivedAt=new Date().toISOString()){
    const scope=this.scopes.get(marketId);if(!scope)throw Error('STREAM_SCOPE');
    let normalized;try{normalized=parseEvent(event,raw,scope);}catch{this.metrics.schemaErrors++;this.append({source:'official_sig_realtime',kind:'rejected',marketId,receivedAt,code:'INVALID_SCHEMA'});return;}
    const p=normalized.payload,last=this.revisions.get(marketId);let duplicate=false,gap=false;
    if(event==='market_batch'){
      this.metrics.batches++;duplicate=last!==undefined&&p.delivery.revision<=last;gap=last!==undefined&&p.delivery.previousRevision>last;
      if(duplicate)this.metrics.duplicateBatches++;if(gap)this.metrics.gaps++;if(p.resyncRequired)this.metrics.truncated++;
      if(!duplicate)this.revisions.set(marketId,p.delivery.revision);
      for(const b of p.books){const old=this.books.get(b.exchangeId);if(old&&compareVersion(b.asOf,old.asOf)<=0){this.metrics.olderBooks++;continue;}this.books.set(b.exchangeId,b);this.metrics.pushedBooks++;}
      if(!duplicate&&!gap&&!p.resyncRequired)for(const t of p.trades){if(t.id===null)continue;const key=scope.tournamentId+':'+t.exchangeId+':'+t.id;if(this.seen.has(key)){this.metrics.duplicateTrades++;continue;}this.seen.add(key);this.metrics.trades++;const lag=Date.parse(receivedAt)-Date.parse(t.executedAt);if(lag>=0){this.metrics.latencySamples++;this.metrics.latencySumMs+=lag;this.metrics.latencyMaxMs=Math.max(lag,this.metrics.latencyMaxMs);}}
    }
    this.append({source:'official_sig_realtime',kind:'broadcast',marketId,tournamentId:scope.tournamentId,receivedAt,...normalized,duplicate,gap,requiresRestResync:gap||p.resyncRequired===true||event!=='market_batch'||p.trades?.some(t=>t.sequence===null)===true});
  }
  rest(r){
    const scope=this.scopes.get(r.marketId);if(!scope||r.tournamentId!==scope.tournamentId||r.status!==200||r.source!=='official_sig_api_v1')throw Error('STREAM_REST_SCOPE');
    if(r.kind==='book'){for(const b of parseSnapshot(r.data,scope)){const old=this.books.get(b.exchangeId);if(!old||compareVersion(b.asOf,old.asOf)>=0)this.books.set(b.exchangeId,b);}this.metrics.restBooks++;}
    else if(r.kind==='trades'){for(const e of receiptEvents(r).filter(e=>e.type==='trade'))this.seen.add(scope.tournamentId+':'+e.exchangeId+':'+e.id);this.metrics.restTapes++;}
    this.append({source:'official_sig_api_v1',kind:'rest_reference',marketId:r.marketId,receivedAt:r.receivedAt,endpoint:r.endpoint});
  }
}
export async function capture(input,clientFactory=createClient){
  const init=await new Promise((resolve,reject)=>{input.once('line',line=>{try{resolve(JSON.parse(line));}catch{reject(Error('STREAM_INIT'));}});input.once('close',()=>reject(Error('STREAM_INIT_EOF')));});
  const {auth,outputDirectory,tournamentId,pairs,aggregate=false}=init;
  if(!auth?.token||!auth.anonKey||new URL(auth.supabaseUrl).protocol!=='https:'||Date.parse(auth.expiresAt)<=Date.now()+60000||!Array.isArray(pairs)||pairs.length<1||pairs.length>(aggregate?1000:4))throw Error('STREAM_AUTH_OR_SCOPE');
  const out=path.resolve(outputDirectory);fs.mkdirSync(out);let sequence=0,previous='',closing=false,status='starting',fatal=false,failure=null;const subscriptions={};
  const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..'),codeIdentity=collectFrozenPaperFiles(root,['moloch/realtime/capture.mjs']);
  for(const f of ['moloch/capture-sig.ps1','moloch/read-failure.psm1','moloch/realtime/package.json','moloch/realtime/pnpm-lock.yaml'])codeIdentity[f]=createHash('sha256').update(fs.readFileSync(path.join(root,f))).digest('hex');
  fs.writeFileSync(path.join(out,'manifest.json'),JSON.stringify({mode:'official_sig_realtime_capture',startedAt:new Date().toISOString(),codeIdentity,pairs,tournamentId,aggregate,tokenWrites:1,contestOrders:0,paperOrders:0,tokenPersistence:false},null,2));
  const append=event=>{const body={sequence:++sequence,previous,event},row={...body,hash:payloadHash(body)};fs.appendFileSync(path.join(out,'events.jsonl'),JSON.stringify(row)+'\n');previous=row.hash;};
  const evidence=new StreamEvidence(pairs,tournamentId,append);
  const publishStats=statsPublisher(out);
  const stats=()=>{const m=evidence.metrics;publishStats({mode:'official_sig_realtime_capture',at:new Date().toISOString(),status,failure,subscriptions,metrics:{...m,meanTradeLatencyMs:m.latencySamples?m.latencySumMs/m.latencySamples:null},sequence,previous,tokenWrites:1,contestOrders:0,paperOrders:0,financialIntegration:false,continuousTapeEstablished:false,automaticTokenRefresh:false});};
  const client=clientFactory(auth.supabaseUrl,auth.anonKey,{auth:{persistSession:false,autoRefreshToken:false,detectSessionInUrl:false},realtime:{logger:()=>{}}});
  await client.realtime.setAuth(auth.token);auth.token=null;auth.anonKey=null;init.auth=null;
  let finish;const done=new Promise(resolve=>{finish=resolve;});
  const close=async(failed=false,reason=null)=>{if(closing)return;closing=true;fatal=failed;failure=reason;status=failed?'failed':'completed';clearInterval(timer);clearTimeout(deadline);try{drainRest();}catch(e){fatal=true;failure??=captureFailure('final_rest',e);status='failed';}try{await client.removeAllChannels();}catch{}try{client.realtime.disconnect();}catch{}stats();finish();};
  input.on('line',line=>{try{const command=JSON.parse(line);if(command.stop)void close();}catch(e){void close(true,captureFailure('command',e));}});input.on('close',()=>void close());
  const receiptFile=path.join(path.dirname(out),'receipts.jsonl');let offset=0,pending=Buffer.alloc(0);
  function drainRest(){
    if(fs.existsSync(receiptFile)){const length=fs.statSync(receiptFile).size;if(length<offset)throw Error('STREAM_REST_TRUNCATED');if(length>offset){const fd=fs.openSync(receiptFile,'r');try{const b=Buffer.alloc(Math.min(length-offset,1048576));const n=fs.readSync(fd,b,0,b.length,offset);offset+=n;pending=Buffer.concat([pending,b.subarray(0,n)]);}finally{fs.closeSync(fd);}let n;while((n=pending.indexOf(10))>=0){const line=pending.subarray(0,n).toString('utf8').replace(/^\uFEFF/,'');pending=Buffer.from(pending.subarray(n+1));if(line.trim())evidence.rest(JSON.parse(line));}if(pending.length>1048576)throw Error('STREAM_REST_TOO_LARGE');}}
  }
  const timer=setInterval(()=>{try{drainRest();}catch(e){void close(true,captureFailure('rest',e));return;}try{stats();}catch(e){void close(true,captureFailure('publication',e));}},100);
  const deadline=setTimeout(()=>void close(),Math.min(30*60000,Date.parse(auth.expiresAt)-Date.now()-60000));
  const onStatus=(id,state)=>{if(closing)return;subscriptions[id]=state;if(state==='SUBSCRIBED'){status='subscribed';for(const marketId of aggregate?evidence.scopes.keys():[id])append({kind:'subscribed',source:'official_sig_realtime',marketId,receivedAt:new Date().toISOString(),requiresRestResync:true});}else if(['CHANNEL_ERROR','TIMED_OUT','CLOSED'].includes(state))void close(true,{phase:'channel',code:state});stats();};
  if(aggregate){
    const channel=client.channel(`tournament:${tournamentId}:markets`,{config:{private:true}});
    channel.on('broadcast',{event:'markets_batch'},({payload})=>{if(closing)return;try{evidence.aggregate(payload);stats();}catch(e){void close(true,captureFailure('aggregate',e));}});
    for(const event of ['book_dirty','market_settled'])channel.on('broadcast',{event},({payload})=>{if(closing)return;try{const id=String(payload.marketId);if(evidence.scopes.has(id))evidence.batch(id,event,payload);else evidence.metrics.unknownMarkets++;stats();}catch(e){void close(true,captureFailure('broadcast',e));}});
    channel.subscribe(state=>onStatus('tournament',state));
  }else for(const scope of evidence.scopes.values()){
    const channel=client.channel(scope.topic,{config:{private:true}});
    for(const event of ['market_batch','book_dirty','market_settled'])channel.on('broadcast',{event},({payload})=>{if(closing)return;try{evidence.batch(scope.marketId,event,payload);stats();}catch(e){void close(true,captureFailure('broadcast',e));}});
    channel.subscribe(state=>onStatus(scope.marketId,state));
  }
  await done;input.close();if(fatal)throw Error('STREAM_CAPTURE_FAILED');
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){capture(readline.createInterface({input:process.stdin,crlfDelay:Infinity})).then(()=>process.exit(0)).catch(()=>{process.stderr.write('MOLOCH_STREAM_CAPTURE_FAILED\n');process.exit(1);});}
