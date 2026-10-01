import fs from 'node:fs';
import path from 'node:path';
import http from 'node:http';
import {randomUUID} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {loadKey} from '../bot/api.mjs';
import {initialState,normalizeBook,observe,report,assertState,sampleEquity,validateConfig,restoreState} from './model.mjs';
import {Journal,lock,ReadClient,atomicJson} from './runtime.mjs';

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
let runtime={status:'starting',pid:process.pid,runtimeDir,readRequests:0,errors:[],cycles:0,lastCycleAt:null,dashboard:'http://127.0.0.1:'+c.port};
function persistStatus(){const r=report(state,c);atomicJson(path.join(runtimeDir,'stats.json'),{...r,runtime:{...runtime,readRequests:api?.requests||0}});return r;}
try{
  journal=new Journal(path.join(runtimeDir,'journal.jsonl'));state=journal.last?restoreState(journal.last.state,c):initialState(c,randomUUID(),new Date().toISOString());assertState(state,c);
  if(journal.last&&journal.last.state.engineRevision!==state.engineRevision)journal.commit(state,{type:'entry_lot_schema_upgrade',fromRevision:1,toRevision:2});
  api=new ReadClient(loadKey(),{interval:c.readIntervalMs,signal:control.signal,onAdmission:next=>atomicJson(path.join(runtimeDir,'rate.json'),{next})});
  if(fs.existsSync(path.join(runtimeDir,'rate.json')))api.next=JSON.parse(fs.readFileSync(path.join(runtimeDir,'rate.json'))).next;
  runtime.status='connecting';persistStatus();
  const html=fs.readFileSync(path.join(root,'dashboard.html'));
  server=http.createServer((req,res)=>{
    res.setHeader('Cache-Control','no-store');res.setHeader('X-Content-Type-Options','nosniff');
    if(req.method!=='GET'){res.writeHead(405);return res.end();}
    if(req.url==='/api/stats'){res.setHeader('Content-Type','application/json');return res.end(JSON.stringify({...report(state,c),runtime:{...runtime,readRequests:api.requests}}));}
    if(req.url==='/'){res.setHeader('Content-Type','text/html; charset=utf-8');return res.end(html);}res.writeHead(404);res.end();
  });
  await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(c.port,'127.0.0.1',resolve);});
  heartbeat=setInterval(()=>{try{persistStatus();}catch{runtime.status='failed';stop();}},3000);
  const tournament=(await api.request('/tournaments/'+c.tournamentSlug)).data;
  if(tournament.id!==c.tournamentId||tournament.currencyName!=='SUSQies'||tournament.status!=='active'||tournament.isPendingEnrolment)throw Error('Tournament identity/status mismatch');
  if(!journal.last)journal.commit(state,{type:'started',config:c});
  runtime.status='running';persistStatus();
  const cyclesArg=process.argv.indexOf('--cycles');const maxCycles=cyclesArg===-1?Infinity:Number(process.argv[cyclesArg+1]);
  if(!Number.isInteger(maxCycles)&&maxCycles!==Infinity||maxCycles<1)throw Error('Invalid cycle bound');
  while(!stopping&&runtime.cycles<maxCycles){
    const cycleStart=Date.now();const events=[];let successful=0;
    try{
      const positions=await api.request('/tournaments/'+c.tournamentSlug+'/portfolio/positions');
      if(!Array.isArray(positions.data.positions))throw Error('Invalid real holdings response');
      const orders=await api.orders();
      const excluded=new Set([...positions.data.positions.filter(p=>p.quantity!==0),...orders].map(p=>String(p.exchangeId)));
      for(const marketId of c.marketIds){
        if(stopping)break;
        try{
          const r=await api.request('/markets/'+marketId+'/orderbook?tournamentId='+c.tournamentId+'&depth=20');
          events.push(r);
          const b=normalizeBook(r.data,marketId,c.tournamentId,r.receivedAt,r.requestStartedAt,c.maxDataAgeMs,excluded);
          try{state=observe(state,b,c);}catch(e){e.fatal=true;throw e;}successful++;
        }catch(e){if(stopping)break;if(e.fatal||e.status===401||e.status===403)throw e;const reason=e.status?'http_'+e.status:e.message;state.skips[marketId]={at:new Date().toISOString(),reason};events.push({marketId,skipped:reason});if(e.status===429||e.status===503)break;}
      }
      const at=new Date().toISOString();sampleEquity(state,c,at);assertState(state,c);journal.commit(state,{type:'cycle',sourceSnapshots:events,accountExclusions:[...excluded],exclusionObservedAt:positions.receivedAt});
      runtime.cycles++;runtime.lastCycleAt=at;runtime.status=stopping?'stopping':successful?'running':'degraded';persistStatus();
      console.log(JSON.stringify({at,event:'paper_cycle',cycle:runtime.cycles,observations:state.observations,fills:Object.fromEntries(Object.entries(state.strategies).map(([k,s])=>[k,s.fills.length])),competitionOrders:0}));
    }catch(e){if(stopping)break;runtime.errors.push({at:new Date().toISOString(),reason:e.status?'http_'+e.status:'Account reconciliation or durable recording failed'});runtime.errors=runtime.errors.slice(-20);runtime.status=e.recoverable?'degraded':'failed';persistStatus();if(e.recoverable){journal.commit(state,{type:'account_read_outage',reason:e.status?'http_'+e.status:'connection_unavailable'});await api.pause(Math.max(10000,api.cooldown-Date.now()));continue;}throw Error('Paper runner stopped: '+runtime.errors.at(-1).reason);}
    if(!stopping&&runtime.cycles<maxCycles)await api.pause(Math.max(1000,c.cycleIntervalMs-(Date.now()-cycleStart)));
  }
  runtime.status='stopped';journal.commit(state,{type:'stopped'});persistStatus();
}catch(e){if(state){runtime.status=stopping?'stopped':'failed';runtime.errors.push({at:new Date().toISOString(),reason:stopping?'stop_requested':e.message});try{persistStatus();}catch{}}console.error(stopping?'Paper runner stopped':'Paper runner failed; see local stats');process.exitCode=stopping?0:1;
}finally{clearInterval(stopWatch);clearInterval(heartbeat);api?.close();server?.close();cleanup();}
