import fs from 'node:fs/promises';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { performance } from 'node:perf_hooks';
import { fileURLToPath } from 'node:url';
import { setTimeout as sleep } from 'node:timers/promises';
import { acquire, atomicJson } from './kalshi-collector.mjs';

const CHILD = fileURLToPath(new URL('./kalshi-collector.mjs', import.meta.url));
const FLAGS = new Set(['cache','repository-root','env-file','port','max-in-flight','duration-seconds','maximum-markets','websocket']);
export function configuration(argv) {
  if (!Array.isArray(argv) || argv.length % 2) throw Error('KALSHI_SUPERVISOR_ARGUMENT');
  const seen = new Set(); let cache;
  for (let i=0;i<argv.length;i+=2) {
    const flag=argv[i], value=argv[i+1];
    if (typeof flag!=='string'||!flag.startsWith('--')||!FLAGS.has(flag.slice(2))||seen.has(flag)||typeof value!=='string'||!value||value.includes('\0')) throw Error('KALSHI_SUPERVISOR_ARGUMENT');
    seen.add(flag); if(flag==='--cache') cache=path.resolve(value);
  }
  if (!cache) throw Error('KALSHI_SUPERVISOR_CACHE_REQUIRED');
  return {cache,argv:[...argv]};
}
export function restartDelay(exits) {
  if (!Number.isSafeInteger(exits)||exits<1) throw Error('KALSHI_SUPERVISOR_RESTART_COUNT');
  return Math.min(60_000,1000*2**Math.min(6,exits-1));
}
async function exists(file) {try{await fs.lstat(file);return true;}catch(e){if(e.code==='ENOENT')return false;throw e;}}
export async function runSupervisor(argv,{spawnChild=spawn,wait=sleep,clock=()=>performance.now(),signals=process,pollMs=250,closeGraceMs=30_000}={}) {
  const config=configuration(argv);
  if (!Number.isSafeInteger(pollMs)||pollMs<1||pollMs>1000||!Number.isSafeInteger(closeGraceMs)||closeGraceMs<0||closeGraceMs>30_000) throw Error('KALSHI_SUPERVISOR_CONFIGURATION');
  const root=path.join(config.cache,'supervisor'),stopFile=path.join(config.cache,'STOP');
  const release=await acquire(root); let out,err,child,completion,finished=true,closing=false,stopping=false,signalFailure,exits=0,starts=0,lastExit=null;
  const startedAt=new Date().toISOString();
  const status=async(state,extra={})=>atomicJson(path.join(root,'status.json'),{schemaVersion:1,state,pid:process.pid,childPid:finished?null:child?.pid??null,startedAt,at:new Date().toISOString(),starts,restarts:Math.max(0,starts-1),unexpectedExits:exits,lastExit,...extra});
  let stopWrite=Promise.resolve();
  const onSignal=()=>{stopping=true;stopWrite=stopWrite.then(()=>fs.writeFile(stopFile,'Kalshi supervisor requested stop\n',{flag:'a',mode:0o600})).catch(()=>{signalFailure=Error('KALSHI_SUPERVISOR_STOP_IO');});};
  const isStopped=async()=>stopping||await exists(stopFile);
  async function closeChild(){
    if(!child||finished||closing)return;closing=true;
    const deadline=clock()+closeGraceMs;
    while(!finished&&clock()<deadline)await Promise.race([completion,wait(Math.min(pollMs,Math.max(1,deadline-clock())))]);
    if(!finished){child.kill('SIGKILL');await Promise.race([completion,wait(1000)]);if(!finished)throw Error('KALSHI_SUPERVISOR_CHILD_NOT_CLOSED');}
  }
  signals.on('SIGINT',onSignal);signals.on('SIGTERM',onSignal);
  try{
    out=await fs.open(path.join(root,'collector.stdout.log'),'a',0o600);err=await fs.open(path.join(root,'collector.stderr.log'),'a',0o600);
    await status('starting');
    while(!await isStopped()){
      child=null;finished=true;closing=false;lastExit=null;
      // No shell, arbitrary executable, credential loading or network in parent.
      child=spawnChild(process.execPath,[CHILD,...config.argv],{windowsHide:true,shell:false,stdio:['ignore',out.fd,err.fd]});finished=false;starts++;
      completion=new Promise(resolve=>{
        let error=false;child.once('error',()=>{error=true;lastExit={code:null,signal:null,spawnError:true};if(!child.pid){finished=true;resolve();}else{onSignal();}});
        child.once('close',(code,signal)=>{if(!error){lastExit={code:Number.isInteger(code)?code:null,signal:['SIGINT','SIGTERM','SIGKILL'].includes(signal)?signal:null,spawnError:false};}finished=true;resolve();});
      });
      await status('running');
      while(!finished&&!await isStopped())await Promise.race([completion,wait(pollMs)]);
      if(await isStopped())break;
      exits++;const delay=restartDelay(exits),until=clock()+delay;await status('backoff',{restartDelayMs:delay,restartAt:new Date(Date.now()+delay).toISOString()});
      while(clock()<until&&!await isStopped())await wait(Math.min(pollMs,Math.max(1,until-clock())));
    }
    stopping=true;await stopWrite;await status('stopping');await closeChild();if(signalFailure)throw signalFailure;await status('stopped');
  }finally{
    // Any parent error also prevents an orphan collector from running forever.
    if(child&&!finished){await fs.writeFile(stopFile,'Kalshi supervisor failed closed\n',{flag:'a',mode:0o600}).catch(()=>{});await closeChild();}
    signals.off('SIGINT',onSignal);signals.off('SIGTERM',onSignal);await out?.close();await err?.close();await release();
  }
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url))runSupervisor(process.argv.slice(2)).catch(()=>{console.error(JSON.stringify({state:'failed',code:'KALSHI_SUPERVISOR_FAILED',at:new Date().toISOString()}));process.exitCode=1;});
