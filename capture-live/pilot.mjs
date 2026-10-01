import {MarketCollector} from '../feed/collector.mjs';
import {checkStop,interruptibleWait} from '../feed/runtime.mjs';
import {snapshotPath,requireThat} from '../feed/contract.mjs';

// Bounded pilot orchestration; caller supplies reviewed credentials, SDK,
// shared budget authority and an exclusive sanitized Capture sink.
export async function capturePilot({scope,transport,capture,signal,shouldStop=()=>false,durationMs=60000,now=Date.now,sleep,staleMs=10000,maxRecoveries=8,maxConnections=2}) {
  checkStop(signal,shouldStop);
  requireThat(durationMs>0&&durationMs<=60000&&Number.isInteger(maxRecoveries)&&maxRecoveries>0&&maxRecoveries<=10,'INVALID_PILOT_BOUND');
  requireThat(Number.isInteger(maxConnections)&&maxConnections>0&&maxConnections<=2,'INVALID_PILOT_BOUND');
  const local=new AbortController();const combined=signal?AbortSignal.any([signal,local.signal]):local.signal;
  const hardDeadline=setTimeout(()=>local.abort(),durationMs);
  const stopPoll=setInterval(()=>{if(shouldStop())local.abort();},25);
  const collector=new MarketCollector(scope,{capture,now,staleMs});
  let connections=0,recoveries=0,outcome='duration_complete';const started=now();
  // Abort subscription/snapshot caller waits too, even for a broken injected adapter.
  async function call(fn){
    checkStop(combined,shouldStop);let listener;
    const aborted=new Promise((_,reject)=>{listener=()=>reject(new Error('PILOT_STOPPED'));combined.addEventListener('abort',listener,{once:true});if(combined.aborted)listener();});
    try{return await Promise.race([Promise.resolve().then(()=>{checkStop(combined,shouldStop);return fn();}),aborted]);}finally{combined.removeEventListener('abort',listener);}
  }
  transport.onDisconnect=()=>collector.disconnected('SOCKET_ERROR');
  async function connect(){
    if(connections>=maxConnections)throw new Error('CONNECTION_BOUND');connections++;
    let active=false;const pending=[];const epoch=collector.epoch+1;
    await call(()=>transport.subscribe({topic:collector.scope.topic,signal:combined,onEvent:(event,payload)=>{
      const receivedAt=new Date(now()).toISOString();
      if(active)collector.ingest(event,payload,{epoch,receivedAt});
      else if(pending.length<1000)pending.push([event,payload,receivedAt]);
      else local.abort();
    }}));
    checkStop(combined,shouldStop);collector.subscribed(connections===1?'INITIAL':'RECONNECT');active=true;
    for(const [event,payload,receivedAt] of pending)collector.ingest(event,payload,{epoch,receivedAt});
  }
  async function recover(){
    if(recoveries>=maxRecoveries)throw new Error('RECOVERY_BOUND');recoveries++;
    const ticket=collector.beginRecovery();
    const raw=await call(()=>transport.readSnapshot({method:'GET',path:snapshotPath(collector.scope),signal:combined}));
    collector.completeRecovery(ticket,raw);
  }
  try {
    await connect();await recover();
    while(now()-started<durationMs){
      checkStop(combined,shouldStop);collector.tick();
      if(collector.needsMarketState)throw new Error('SETTLEMENT_RECONCILIATION_REQUIRED');
      if(transport.refreshDue()){await transport.disconnect();collector.disconnected();await connect();await recover();}
      else if(collector.state==='disconnected'){await interruptibleWait(1000,{signal:combined,shouldStop,sleep});await connect();await recover();}
      else if(collector.state==='needs_resync'){
        // At most one recovery per 2 seconds; shared budget may delay further.
        await interruptibleWait(2000,{signal:combined,shouldStop,sleep});await recover();
      }
      await interruptibleWait(250,{signal:combined,shouldStop,sleep});
    }
  }catch{outcome=combined.aborted||shouldStop()?'stopped':'blocked';}
  finally {
    clearTimeout(hardDeadline);clearInterval(stopPoll);let cleanupTimer;
    try {await Promise.race([transport.close(),new Promise(r=>{cleanupTimer=setTimeout(r,1000);})]);}
    catch {outcome='blocked';}
    finally {clearTimeout(cleanupTimer);collector.stop();}
  }
  const report={format:'sig-capture-pilot-summary',schemaVersion:1,startedAt:new Date(started).toISOString(),finishedAt:new Date(now()).toISOString(),outcome,connections,recoveries,
    restCounts:{...transport.counts},collector:collector.status(),orderWrites:0,tradeTapeComplete:false,eligibleForExecution:false};
  capture?.append({kind:'summary',observedAt:new Date(now()).toISOString(),report});
  return report;
}
