import {MarketCollector} from '../feed/collector.mjs';
import {checkStop,interruptibleWait} from '../feed/runtime.mjs';
import {snapshotPath,requireThat} from '../feed/contract.mjs';

// Bounded pilot only. Host supplies reviewed credentials, SDK, account budget
// and an exclusive sanitized sink. No order methods or fake fills exist.
export async function capturePilot({scope,transport,capture,signal,shouldStop=()=>false,durationMs=60000,now=Date.now,sleep,staleMs=10000,maxRecoveries=8,maxConnections=2}) {
  checkStop(signal,shouldStop);
  requireThat(durationMs>0&&durationMs<=60000&&Number.isInteger(maxRecoveries)&&maxRecoveries>0&&maxRecoveries<=10,'INVALID_PILOT_BOUND');
  requireThat(Number.isInteger(maxConnections)&&maxConnections>0&&maxConnections<=2,'INVALID_PILOT_BOUND');
  const local=new AbortController();const combined=signal?AbortSignal.any([signal,local.signal]):local.signal;
  let cause=null,phase='construction',collector,connections=0,recoveries=0,outcome='duration_complete',sinkFailed=false,everReady=false;
  const started=now();
  const abort=reason=>{if(cause===null)cause=reason;local.abort();};
  const safeSink=capture?{append:row=>{if(sinkFailed)return;try{capture.append(row);}catch{sinkFailed=true;abort('sink_failure');}}}:null;
  const hardDeadline=setTimeout(()=>abort('deadline'),durationMs);
  const stopPoll=setInterval(()=>{if(shouldStop())abort('user_stop');},25);
  async function call(fn){
    checkStop(combined,shouldStop);let listener;
    const aborted=new Promise((_,reject)=>{listener=()=>reject(new Error('PILOT_ABORTED'));combined.addEventListener('abort',listener,{once:true});if(combined.aborted)listener();});
    try{return await Promise.race([Promise.resolve().then(()=>{checkStop(combined,shouldStop);return fn();}),aborted]);}
    finally{combined.removeEventListener('abort',listener);}
  }
  async function connect(){
    if(connections>=maxConnections)throw new Error('CONNECTION_BOUND');connections++;
    phase='connect';let active=false,failed=false;const pending=[],epoch=collector.epoch+1;
    transport.onDisconnect=()=>{failed=true;if(active)collector.disconnected('SOCKET_ERROR');};
    await call(()=>transport.subscribe({topic:collector.scope.topic,signal:combined,onEvent:(event,payload)=>{
      if(combined.aborted||failed)return;
      try {
        const receivedAt=new Date(now()).toISOString();
        if(active)collector.ingest(event,payload,{epoch,receivedAt});
        else if(pending.length<1000)pending.push([event,payload,receivedAt]);
        else abort('buffer_failure');
      }catch{abort('callback_failure');}
    }}));
    checkStop(combined,shouldStop);if(failed)throw new Error('SUBSCRIPTION_FAILED');
    collector.subscribed(connections===1?'INITIAL':'RECONNECT');active=true;
    for(const [event,payload,receivedAt] of pending){checkStop(combined,shouldStop);collector.ingest(event,payload,{epoch,receivedAt});}
  }
  async function recover(){
    if(recoveries>=maxRecoveries)throw new Error('RECOVERY_BOUND');recoveries++;phase='recover';
    const ticket=collector.beginRecovery();
    const raw=await call(()=>transport.readSnapshot({method:'GET',path:snapshotPath(collector.scope),signal:combined}));
    if(collector.completeRecovery(ticket,raw))everReady=true;
  }
  try {
    collector=new MarketCollector(scope,{capture:safeSink,now,staleMs});
    await connect();await recover();
    while(now()-started<durationMs){
      checkStop(combined,shouldStop);collector.tick();
      if(collector.needsMarketState)throw new Error('SETTLEMENT_RECONCILIATION_REQUIRED');
      if(transport.refreshDue()){await transport.disconnect();collector.disconnected();await connect();await recover();}
      else if(collector.state==='disconnected'){phase='reconnect_wait';await interruptibleWait(1000,{signal:combined,shouldStop,sleep});await connect();await recover();}
      else if(collector.state==='needs_resync'){phase='recovery_wait';await interruptibleWait(2000,{signal:combined,shouldStop,sleep});await recover();}
      phase='idle';await interruptibleWait(250,{signal:combined,shouldStop,sleep});
    }
    if(!everReady||collector.state!=='ready')outcome='blocked';
  }catch{
    if(sinkFailed||['callback_failure','buffer_failure'].includes(cause))outcome='blocked';
    else if(signal?.aborted||cause==='user_stop'||shouldStop())outcome='stopped';
    else if(cause==='deadline'&&phase==='idle'&&everReady&&collector?.state==='ready')outcome='duration_complete';
    else outcome='blocked';
  }finally{
    clearTimeout(hardDeadline);clearInterval(stopPoll);local.abort();let cleanupTimer;
    try{await Promise.race([Promise.resolve().then(()=>transport.close()),new Promise(resolve=>{cleanupTimer=setTimeout(()=>{outcome='blocked';resolve();},1000);})]);}
    catch{outcome='blocked';}
    finally{clearTimeout(cleanupTimer);try{collector?.stop();}catch{sinkFailed=true;outcome='blocked';}}
  }
  if(sinkFailed)outcome='blocked';
  const report={format:'sig-capture-pilot-summary',schemaVersion:1,startedAt:new Date(started).toISOString(),finishedAt:new Date(now()).toISOString(),outcome,connections,recoveries,
    restCounts:{...transport.counts},collector:collector?.status()??{state:'unavailable'},orderWrites:0,tradeTapeComplete:false,eligibleForExecution:false,captureCommitted:false};
  if(capture&&!sinkFailed){
    try{capture.append({kind:'summary',observedAt:new Date(now()).toISOString(),report:{...report,captureCommitted:true}});report.captureCommitted=true;}
    catch{sinkFailed=true;report.outcome='blocked';}
  }
  return report;
}
