import fs from 'node:fs/promises';
import path from 'node:path';
import http from 'node:http';
import {pathToFileURL} from 'node:url';
import {run as runRest, atomicJson} from './polymarket-collector.mjs';
import {startPolymarketMarketStream} from './polymarket-websocket.mjs';

// Public observations only. REST and WS keep distinct identities and clocks.
export async function run(options, dependencies={}) {
  const cache=path.resolve(options.cache??'polymarket-cache');
  const wsPort=Number(options['ws-port']??17843);
  if(!Number.isInteger(wsPort)||wsPort<0||wsPort>65535)throw Error('POLY_LIVE_WS_PORT');
  let stream,server,timer,cacheTimer,flushInFlight,restStop,fatal,closing=false,selected=[],total=0;
  const now=dependencies.now??Date.now,latest=new Map();
  const pending=new Map(),identities=new Map(),cacheStats={updates:0,coalesced:0,writes:0};
  const writeCache=dependencies.writeCache??atomicJson;
  const flushCache=()=>{if(flushInFlight)return flushInFlight;if(!pending.size)return Promise.resolve();const rows=[...pending.values()];pending.clear();flushInFlight=(async()=>{for(const row of rows){await writeCache(path.join(cache,'ws-latest',row.assetId+'.json'),{...row,identity:identities.get(row.assetId)});cacheStats.writes++;}})().finally(()=>{flushInFlight=null;});return flushInFlight;};
  const fail=e=>{fatal??=e;restStop?.();};
  const status=()=>({source:'polymarket_public_live',at:new Date(now()).toISOString(),pricingAuthority:false,resolutionEquivalenceVerified:false,wsOutcomes:selected.length,restOutcomes:total,restOnlyOutcomes:total-selected.length,selection:'assetId_lexicographic_first_1024',cacheFlushMs:200,pendingCoalesced:pending.size,cacheFlushInFlight:!!flushInFlight,cacheUpdates:cacheStats.updates,cacheCoalesced:cacheStats.coalesced,cacheWrites:cacheStats.writes,stream:stream?.status()??null,failed:!!fatal});
  const quote=id=>{const identity=selected.find(x=>x.assetId===id);if(!identity)return null;const event=latest.get(id);return {identity,streamEpoch:event?.streamEpoch??null,book:stream?.view(id,now())??{valid:false,pricingAuthority:false,sequenceContinuityVerified:false},trigger:event?{eventType:event.eventType,requestStartedAt:event.requestStartedAt??null,receivedAt:event.receivedAt,sourceTimestamp:event.sourceTimestamp??null,bodySha256:event.bodySha256,eventIndex:event.eventIndex??null}:null,pricingAuthority:false,resolutionEquivalenceVerified:false};};
  const publish=()=>atomicJson(path.join(cache,'ws-status.json'),status());
  const shutdown=async()=>{if(closing)return;closing=true;clearInterval(timer);clearInterval(cacheTimer);try{await stream?.stop();await flushCache();await flushCache();}catch(e){fatal??=e;}try{await publish();}catch(e){fatal??=e;}if(server){server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}};
  const {['ws-port']:ignored,...restOptions}=options;
  let result;
  try {
    result=await (dependencies.runRest??runRest)(restOptions,{
      ...dependencies.restDependencies,
      onStopping:shutdown,
      onReady:async ready=>{
        restStop=ready.stop;total=ready.outcomes.length;
        if(typeof ready.recordEvent!=='function')throw Error('POLY_LIVE_JOURNAL_INTERFACE');
        selected=[...ready.outcomes].sort((a,b)=>a.assetId.localeCompare(b.assetId,'en')).slice(0,1024);
        for(const identity of selected)identities.set(identity.assetId,identity);
        await fs.mkdir(path.join(cache,'ws-latest'),{recursive:true,mode:0o700});
        // No prior-process WS book can be promoted into this connection epoch.
        for(const identity of selected)await atomicJson(path.join(cache,'ws-latest',identity.assetId+'.json'),{source:'polymarket_clob_ws_public',identity,normalized:{valid:false},eventType:'process_start_invalidated',pricingAuthority:false});
        const onEvent=async event=>{try{
          await ready.recordEvent(event);
          if(event.assetId&&event.normalized){if(!identities.has(event.assetId))throw Error('POLY_LIVE_WS_SCOPE');latest.set(event.assetId,event);cacheStats.updates++;if(pending.has(event.assetId))cacheStats.coalesced++;pending.set(event.assetId,event);}
        }catch(e){fail(e);throw e;}};
        stream=(dependencies.startStream??startPolymarketMarketStream)({...dependencies.streamOptions,outcomes:selected,onEvent,onStatus:s=>{if(s.stopped&&s.lastReason==='SINK_ERROR')fail(Error('POLY_LIVE_WS_SINK'));}});
        cacheTimer=setInterval(()=>flushCache().catch(fail),200);cacheTimer.unref?.();
        server=http.createServer((req,res)=>{try{
          if(req.method!=='GET'){res.writeHead(405);res.end();return;}
          const url=new URL(req.url,'http://127.0.0.1');let value;
          if(url.pathname==='/status'||url.pathname==='/health')value=status();
          else if(url.pathname==='/quotes')value={at:new Date(now()).toISOString(),pricingAuthority:false,resolutionEquivalenceVerified:false,quotes:selected.map(x=>quote(x.assetId))};
          else if(url.pathname==='/book')value=quote(url.searchParams.get('assetId'));
          if(value==null){res.writeHead(404);res.end();return;}
          res.writeHead(fatal?503:200,{'Content-Type':'application/json','Cache-Control':'no-store'});res.end(JSON.stringify(value));
        }catch(e){fail(e);res.destroy();}});
        server.maxConnections=64;server.requestTimeout=5000;
        await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(wsPort,'127.0.0.1',resolve);});
        server.on('error',fail);await publish();
        timer=setInterval(()=>publish().catch(fail),1000);timer.unref?.();
        await dependencies.onReady?.({port:server.address().port,status,stop:ready.stop,stream});
      }
    });
  } finally {await shutdown();}
  if(fatal)throw fatal;return {rest:result,ws:status()};
}
export function args(argv){const out={};for(let i=0;i<argv.length;i+=2){if(!['--cache','--universe','--rps','--concurrency','--batch-size','--port','--ws-port','--duration-seconds'].includes(argv[i])||argv[i+1]===undefined)throw Error('POLY_LIVE_ARGUMENT');out[argv[i].slice(2)]=argv[i+1];}return out;}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href)run(args(process.argv.slice(2))).catch(e=>{process.stderr.write(e.message+'\n');process.exitCode=1;});
