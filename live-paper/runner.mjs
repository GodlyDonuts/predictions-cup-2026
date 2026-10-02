import fs from 'node:fs';
import path from 'node:path';
import http from 'node:http';
import {randomUUID} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {loadKey} from '../bot/api.mjs';
import {initialState,normalizeBook,observe,report,assertState,sampleEquity,validateConfig,restoreState,invalidate} from './model.mjs';
import {Journal,lock,ReadClient,atomicJson,exclusionSet} from './runtime.mjs';
import {validateUniverse,priceBatch,priceEndpoint,normalizePrices,updateCoverage} from './breadth.mjs';
import {validateDepthPlan} from './depth-scope.mjs';
import {connectTournament} from './connection.mjs';
import {validateTiming,timingSummary} from './read-timing.mjs';

const root=path.dirname(fileURLToPath(import.meta.url)),workspace=path.dirname(root);
if(process.argv.includes('--live'))throw Error('Competition order execution is disabled');
const configFile=process.argv.includes('--config')?path.resolve(process.argv[process.argv.indexOf('--config')+1]):path.join(root,'config.json');
const c=validateConfig(JSON.parse(fs.readFileSync(configFile,'utf8')));
const runtimeDir=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA||workspace,'SigTradingCup','paper-runtime');
const stopFile=path.join(workspace,'STOP'),paperStop=path.join(runtimeDir,'STOP');
if(fs.existsSync(stopFile)||fs.existsSync(paperStop)){console.log(JSON.stringify({status:'stopped',reason:'STOP_present'}));process.exit(0);}
fs.mkdirSync(runtimeDir,{recursive:true,mode:0o700});
const release=lock(path.join(process.env.LOCALAPPDATA||workspace,'SigTradingCup','bot.lock'));
let cleaned=false;function cleanup(){if(!cleaned){cleaned=true;release();}}
process.on('exit',cleanup);
const control=new AbortController();let stopping=false;
const stop=()=>{stopping=true;control.abort();};process.on('SIGINT',stop);process.on('SIGTERM',stop);
const stopWatch=setInterval(()=>{if(fs.existsSync(stopFile)||fs.existsSync(paperStop))stop();},200);
let journal,state,api,server,heartbeat;
let universe,coverage,depthPlan;const recentReadTimings=[];
let runtime={status:'starting',pid:process.pid,runtimeDir,readRequests:0,errors:[],cycles:0,lastCycleAt:null,dashboard:'http://127.0.0.1:'+c.port};
function persistStatus(){const r=report(state,c);atomicJson(path.join(runtimeDir,'stats.json'),{...r,runtime:{...runtime,readRequests:api?.requests||0}});return r;}
function commit(event){try{journal.commit(state,event);}catch{const e=Error('Paper journal recording failed');e.durableRecordingFailed=true;throw e;}}
async function pauseUntilNextRead(ms){try{await api.pause(ms);}catch(e){if(!stopping)throw e;}}
try{
  journal=new Journal(path.join(runtimeDir,'journal.jsonl'));state=journal.last?restoreState(journal.last.state,c):initialState(c,randomUUID(),new Date().toISOString());assertState(state,c);
  universe=validateUniverse(JSON.parse(fs.readFileSync(path.join(root,'price-universe.json'),'utf8')),c.tournamentId);
  depthPlan=validateDepthPlan(JSON.parse(fs.readFileSync(path.join(root,'depth-plan.json'),'utf8')),c,universe);
  const coverageFile=path.join(runtimeDir,'price-coverage.json');
  if(fs.existsSync(coverageFile)){coverage=JSON.parse(fs.readFileSync(coverageFile,'utf8'));if(coverage.universeHash!==universe.hash)coverage=null;}
  if(journal.last&&journal.last.state.engineRevision!==state.engineRevision)commit({type:'paper_engine_upgrade',fromRevision:journal.last.state.engineRevision||1,toRevision:state.engineRevision});
  api=new ReadClient(loadKey(),{interval:c.readIntervalMs,signal:control.signal,onAdmission:next=>atomicJson(path.join(runtimeDir,'rate.json'),{next,manualReview:next===null}),onTiming:event=>{validateTiming(event);fs.appendFileSync(path.join(runtimeDir,'read-timings.jsonl'),JSON.stringify(event)+'\n',{mode:0o600});recentReadTimings.push(event);if(recentReadTimings.length>1000)recentReadTimings.shift();runtime.readTiming=timingSummary(recentReadTimings);}});
  if(fs.existsSync(path.join(runtimeDir,'rate.json'))){const rate=JSON.parse(fs.readFileSync(path.join(runtimeDir,'rate.json')));if(rate.manualReview||rate.next===null)throw Error('Rate cooldown requires manual review');if(!Number.isFinite(rate.next)||rate.next<0||rate.next>8640000000000000)throw Error('Invalid persisted rate state');api.next=rate.next;}
  runtime.status='connecting';persistStatus();
  const html=fs.readFileSync(path.join(root,'dashboard.html'));
  server=http.createServer((req,res)=>{
    res.setHeader('Cache-Control','no-store');res.setHeader('X-Content-Type-Options','nosniff');
    if(req.method!=='GET'){res.writeHead(405);return res.end();}
    if(req.url==='/api/stats'){res.setHeader('Content-Type','application/json');const legacy=report(state,c);let primary=legacy;try{const baseline=JSON.parse(fs.readFileSync(path.join(runtimeDir,'fee-free-stats.json'),'utf8'));if(baseline.config.feeBps!==0)throw Error('Invalid baseline fee');primary={...baseline,legacyFeeStress:{feeBps:c.feeBps,runId:legacy.runId,strategies:legacy.strategies},baselineWorkerStatus:baseline.status};}catch{primary={...legacy,feeScenario:'historical_fee_stress',baselineWorkerStatus:'unavailable'};}return res.end(JSON.stringify({...primary,runtime:{...runtime,readRequests:api.requests}}));}
    if(req.url==='/api/ml'){res.setHeader('Content-Type','application/json');try{return res.end(fs.readFileSync(path.join(runtimeDir,'ml-stats.json'),'utf8'));}catch{res.writeHead(503);return res.end(JSON.stringify({status:'unavailable'}));}}
    if(req.url==='/api/breadth'){res.setHeader('Content-Type','application/json');return res.end(JSON.stringify({...(coverage||{universeSize:universe.exchanges.length,quotes:{}}),collectionAgeMs:coverage?Date.now()-Date.parse(coverage.updatedAt):null}));}
    if(req.url==='/'){res.setHeader('Content-Type','text/html; charset=utf-8');return res.end(html);}res.writeHead(404);res.end();
  });
  await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(c.port,'127.0.0.1',resolve);});
  heartbeat=setInterval(()=>{try{persistStatus();}catch{runtime.status='failed';stop();}},3000);
  await connectTournament(api,c,{stopped:()=>stopping,now:Date.now,pause:pauseUntilNextRead,onRetry:({attempts,delay,reason})=>{
    const at=new Date().toISOString();invalidate(state,null,at,'startup_connection_unavailable');
    if(journal.last)commit({type:'account_read_outage',clockAt:at,reason,phase:'startup'});
    api.next=Math.max(api.next,Date.now()+delay);api.onAdmission(Math.max(api.next,api.cooldown));
    runtime.status='connecting_retry';runtime.startupRetry={attempts,delay,reason,at};persistStatus();
  }});
  if(!journal.last&&!stopping)commit({type:'started',config:c});
  runtime.status=stopping?'stopping':'running';persistStatus();
  const cyclesArg=process.argv.indexOf('--cycles');const maxCycles=cyclesArg===-1?Infinity:Number(process.argv[cyclesArg+1]);
  if(!Number.isInteger(maxCycles)&&maxCycles!==Infinity||maxCycles<1)throw Error('Invalid cycle bound');
  while(!stopping&&runtime.cycles<maxCycles){
    const cycleStart=Date.now(),checkpoint=structuredClone(state);const events=[];let successful=0,cycleCommitted=false;
    try{
      const positions=await api.request('/tournaments/'+c.tournamentSlug+'/portfolio/positions');
      if(!Array.isArray(positions.data.positions))throw Error('Invalid real holdings response');
      const orders=await api.orders();
      const excluded=exclusionSet(positions.data.positions,orders);
      for(const marketId of c.marketIds){
        if(stopping)break;
        try{
          const r=await api.request('/markets/'+marketId+'/orderbook?tournamentId='+c.tournamentId+'&depth=20');
          events.push(r);
          const b=normalizeBook(r.data,marketId,c.tournamentId,r.receivedAt,r.requestStartedAt,c.maxDataAgeMs,excluded);
          try{state=observe(state,b,c);}catch(e){e.fatal=true;throw e;}successful++;
        }catch(e){if(stopping)break;if(e.fatal||api.manualReview||e.status===401||e.status===403)throw e;const reason=e.status?'http_'+e.status:e.message,at=new Date().toISOString();state.skips[marketId]={at,reason};invalidate(state,marketId,at,reason);events.push({marketId,skipped:reason,at});if(e.status===429||e.status===503)break;}
      }
      const supplementalDepth=[];
      for(const marketId of depthPlan.marketIds){
        if(stopping||api.cooldown>Date.now())break;
        try{
          const r=await api.request('/markets/'+marketId+'/orderbook?tournamentId='+c.tournamentId+'&depth=20');
          const b=normalizeBook(r.data,marketId,c.tournamentId,r.receivedAt,r.requestStartedAt,c.maxDataAgeMs,excluded);
          if(b.exchangeId!==universe.exchanges.find(x=>x.marketId===marketId).exchangeId)throw Error('Supplemental exchange identity mismatch');
          supplementalDepth.push(r);
        }catch(e){if(stopping)break;if(api.manualReview||e.status===401||e.status===403)throw e;supplementalDepth.push({marketId,skipped:e.status?'http_'+e.status:e.message,at:new Date().toISOString()});if(e.status===429||e.status===503)break;}
      }
      runtime.supplementalDepth={marketIds:depthPlan.marketIds,observations:supplementalDepth.filter(x=>x.endpoint).length,skips:supplementalDepth.filter(x=>x.skipped).map(x=>({marketId:x.marketId,reason:x.skipped}))};
      let bulkPrices=null,bulkPriceError=null;
      if(!stopping&&api.cooldown<=Date.now()){
        try{
          const batchIndex=Number.isInteger(coverage?.nextBatch)&&coverage.nextBatch>=0?coverage.nextBatch:0;
          const batch=priceBatch(universe,batchIndex);
          bulkPrices=normalizePrices(await api.request(priceEndpoint(batch,c.tournamentId)),batch,universe);
          coverage=updateCoverage(coverage,bulkPrices,universe,batchIndex);
          atomicJson(coverageFile,coverage);
          runtime.breadth={universeSize:universe.exchanges.length,observedExchanges:Object.keys(coverage.quotes).length,lastCollectionAt:coverage.updatedAt,error:null};
        }catch(e){if(!stopping&&(api.manualReview||e.status===401||e.status===403))throw e;bulkPriceError=stopping?'stopped':e.status?'http_'+e.status:e.message;runtime.breadth={...runtime.breadth,error:bulkPriceError};}
      }
      const at=new Date().toISOString();sampleEquity(state,c,at);assertState(state,c);commit({type:'cycle',clockAt:at,sourceSnapshots:events,supplementalDepth,bulkPrices,bulkPriceError,accountExclusions:[...excluded],exclusionObservedAt:positions.receivedAt});cycleCommitted=true;
      runtime.cycles++;runtime.lastCycleAt=at;runtime.status=stopping?'stopping':successful?'running':'degraded';persistStatus();
      console.log(JSON.stringify({at,event:'paper_cycle',cycle:runtime.cycles,observations:state.observations,fills:Object.fromEntries(Object.entries(state.strategies).map(([k,s])=>[k,s.fills.length])),competitionOrders:0}));
    }catch(e){if(e.durableRecordingFailed)throw e;if(stopping){if(!cycleCommitted)state=checkpoint;break;}const at=new Date().toISOString();invalidate(state,null,at,'account_reconciliation_unavailable');runtime.errors.push({at,reason:e.status?'http_'+e.status:e.message});runtime.errors=runtime.errors.slice(-20);runtime.status=e.recoverable?'degraded':'failed';persistStatus();if(e.recoverable){commit({type:'account_read_outage',clockAt:at,reason:e.status?'http_'+e.status:'connection_unavailable'});await pauseUntilNextRead(Math.max(10000,api.cooldown-Date.now()));continue;}throw Error('Paper runner stopped: '+runtime.errors.at(-1).reason);}
    if(!stopping&&runtime.cycles<maxCycles)await pauseUntilNextRead(Math.max(1000,c.cycleIntervalMs-(Date.now()-cycleStart)));
  }
  runtime.status='stopped';commit({type:'stopped'});persistStatus();
}catch(e){const stopped=stopping&&!e.durableRecordingFailed;if(state){runtime.status=stopped?'stopped':'failed';runtime.errors.push({at:new Date().toISOString(),reason:stopped?'stop_requested':e.message});try{persistStatus();}catch{}}console.error(stopped?'Paper runner stopped':'Paper runner failed; see local stats');process.exitCode=stopped?0:1;
}finally{clearInterval(stopWatch);clearInterval(heartbeat);api?.close();server?.close();cleanup();}
