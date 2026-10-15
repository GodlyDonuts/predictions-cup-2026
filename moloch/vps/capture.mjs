import fs from 'node:fs';import path from 'node:path';import os from 'node:os';import {spawn} from 'node:child_process';import {fileURLToPath,pathToFileURL} from 'node:url';
import {readGet} from '../realtime/read-get.mjs';import {authToken} from '../realtime/auth-token.mjs';import {atomicPaperJson} from '../../live-paper/paper-persistence.mjs';
const tournament='bda92870-621e-47b0-bc3c-3602c5c26f55';
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
export function admissionAt(budget,now,limit=100){
 if(!Number.isInteger(limit)||limit<1||limit>100||!Number.isSafeInteger(now))throw Error('CAPTURE_LIMIT_INVALID');
 const times=budget.times??[];if(times.some(t=>!Number.isSafeInteger(t))||times.some((t,i)=>i&&t<times[i-1])||times.at(-1)>now)throw Error('CAPTURE_CLOCK_INVALID');
 const recent=times.filter(t=>t>now-60000);return Math.max(now,budget.nextAt??0,(times.at(-1)??-Infinity)+Math.ceil(60000/limit),recent.length>=limit?recent[recent.length-limit]+60001:0);
}
export function assertDisabledBoundary(control,environment,markerPresent){
 if(control?.networkEnabled!==true||control?.tradingEnabled!==false||environment.MOLOCH_NETWORK_ENABLED!=='1'||markerPresent)throw Error('MOLOCH_NETWORK_DISABLED');
 if(control.readsPerMinute!==100)throw Error('CAPTURE_ACCOUNT_LIMIT_REQUIRED');
}
export function validatePairs(pairs){
 if(!Array.isArray(pairs)||!pairs.length||pairs.length>4||new Set(pairs.map(p=>p.marketId)).size!==pairs.length||new Set(pairs.map(p=>p.exchangeId)).size!==pairs.length||pairs.some(p=>!/^[1-9]\d*$/.test(p.marketId)||!/^[1-9]\d*$/.test(p.exchangeId)))throw Error('CAPTURE_SCOPE_INVALID');return pairs;
}
export function loadCredential(file){
 const stat=fs.lstatSync(file);if(!stat.isFile()||stat.isSymbolicLink()||(stat.mode&0o077)!==0||(process.getuid&&stat.uid!==process.getuid()))throw Error('CREDENTIAL_PERMISSIONS');
 const key=fs.readFileSync(file,'utf8').trim();if(!/^[A-Za-z0-9_-]{16,256}$/.test(key))throw Error('CREDENTIAL_FORMAT');return key;
}
export async function capture(options,environment=process.env){
 const stateDir=path.resolve(environment.MOLOCH_STATE_DIR??path.join(os.homedir(),'.local/state/moloch'));
 const control=JSON.parse(fs.readFileSync(environment.MOLOCH_CONTROL_FILE??path.join(stateDir,'deployment.json')));
 assertDisabledBoundary(control,environment,fs.existsSync(path.join(stateDir,'NETWORK_DISABLED')));
 const pairs=validatePairs(JSON.parse(fs.readFileSync(options.pairs))),cycles=Number(options.cycles);
 if(!Number.isInteger(cycles)||cycles<1||cycles>Math.floor(60/pairs.length))throw Error('CAPTURE_DURATION_INVALID');
 const out=path.resolve(options.output);if(fs.existsSync(out))throw Error('CAPTURE_OUTPUT_EXISTS');
 fs.mkdirSync(stateDir,{recursive:true,mode:0o700});
 const lock=path.join(stateDir,'capture.lock'),fd=fs.openSync(lock,'wx',0o600),lockStat=fs.fstatSync(fd);fs.writeFileSync(fd,String(process.pid));
 let stream=null,streamExit=null,streamDone=null,headers=null;
 const budgetFile=path.join(stateDir,'sig-read-budget.json'),budget=fs.existsSync(budgetFile)?JSON.parse(fs.readFileSync(budgetFile)):{times:[],nextAt:0,nextAuthAt:0,blocked:false};
 const stopped=()=>fs.existsSync(path.join(out,'STOP'));
 const persistBudget=()=>atomicPaperJson(budgetFile,budget);
 const saveFailure=async(packet,request)=>{
  const delay=Math.max(60,packet.retryAfterSeconds??60);budget.blocked=true;budget.nextAt=Date.now()+delay*1000;await persistBudget();
  const failure={at:new Date().toISOString(),status:packet.status,endpoint:request.endpoint,method:request.kind==='auth'?'Post':'Get',marketId:request.pair?.marketId??'',exchangeId:request.pair?.exchangeId??'',minimumBackoffSeconds:delay,orderWrites:0,transport:packet.transport};
  await atomicPaperJson(path.join(out,'failure.json'),failure);fs.appendFileSync(path.join(out,'read-failures.jsonl'),JSON.stringify(failure)+'\n');return failure;
 };
 const admit=async (auth,endpoint=null)=>{
  for(;;){if(stopped())return false;const now=Date.now(),at=Math.max(admissionAt(budget,now),auth?budget.nextAuthAt??0:0,endpoint?budget.endpointNextAt?.[endpoint.split("?")[0]]??0:0);if(at<=now)break;await sleep(Math.min(250,at-now));}
  const now=Date.now();budget.times=(budget.times??[]).filter(t=>t>now-60000);if(auth)budget.nextAuthAt=now+60000;else budget.times.push(now);budget.nextAt=now+600;if(endpoint){budget.endpointNextAt??={};budget.endpointNextAt[endpoint.split("?")[0]]=now+(endpoint.includes("/orderbook?")?1000:2000);}await persistBudget();return true;
 };
 try{
  if(budget.blocked)throw Error('CAPTURE_BUDGET_BLOCKED');
  headers={Authorization:'Bearer '+loadCredential(environment.MOLOCH_SIG_CREDENTIAL_FILE??path.join(stateDir,'credentials/sig-api.key'))};fs.mkdirSync(out,{mode:0o700});
  const universe=options.universe?JSON.parse(fs.readFileSync(options.universe)):pairs;
  if(options.realtime){
   if(!await admit(true))return;
   const packet=await authToken({headers});if(packet.status!==200){await saveFailure(packet,{kind:'auth',endpoint:'/realtime/token'});throw Error('CAPTURE_AUTH_FAILED');}
   let auth;try{auth=JSON.parse(packet.content);}catch{throw Error('CAPTURE_AUTH_JSON_INVALID');}
   stream=spawn(process.execPath,[fileURLToPath(new URL('../realtime/capture.mjs',import.meta.url))],{stdio:['pipe','ignore','pipe']});stream.stderr.on('data',()=>{});streamDone=new Promise(resolve=>{stream.once('exit',code=>{streamExit=code;resolve(code);});stream.once('error',()=>{streamExit=-1;resolve(-1);});});
   stream.stdin.write(JSON.stringify({auth,outputDirectory:path.join(out,'stream'),tournamentId:tournament,pairs:universe,...(options.universe?{aggregate:true}:{})})+'\n');auth=null;packet.content=null;
  }
  const deadline=Date.now()+cycles*pairs.length*24000,queue=[];
  // Preserve the old observation duration while increasing the polling cadence.
  for(let cycle=0;cycle<cycles*20;cycle++)for(const pair of pairs)queue.push({kind:'book',pair},{kind:'trades',pair});
  const initial=pairs.filter(p=>p.recoveryFrom).flatMap(pair=>[{kind:'book',pair},{kind:'trades',pair,from:pair.recoveryFrom,to:pair.recoveryTo}]);queue.unshift(...initial);
  let recoveryUsed=false;
  for(let index=0;index<queue.length&&Date.now()<deadline&&!stopped();index++){
   const request=queue[index];if(streamExit!==null)throw Error('CAPTURE_STREAM_STOPPED');
   let windows=null;
   if(options.recovery){windows=JSON.parse(fs.readFileSync(options.recovery));validatePairs(windows.scopes);if(windows.scopes.length!==pairs.length||windows.scopes.some(p=>!pairs.some(q=>p.marketId===q.marketId&&p.exchangeId===q.exchangeId)))throw Error('CAPTURE_RECOVERY_SCOPE');
    if(options.priority){const next=Math.max(Date.now(),admissionAt(budget,Date.now()));for(const intent of windows.exitBooks??[]){if(intent.activationAt>next||intent.expiresAt<=next)continue;const target=queue.findIndex((q,i)=>i>=index&&q.kind==='book'&&q.pair.marketId===intent.marketId&&q.pair.exchangeId===intent.exchangeId);if(target>index){const [item]=queue.splice(target,1);queue.splice(index,0,item);break;}}}
   }
   // Reordering moves existing admissions only; use the selected queue element.
   const selected=queue[index];if(windows)selected.pair={...selected.pair,...windows.scopes.find(p=>p.exchangeId===selected.pair.exchangeId)};const pair=selected.pair;
   if(!selected.endpoint){const query=new URLSearchParams({tournamentId:tournament});
    if(selected.kind==='book'){query.set('depth','200');selected.endpoint=`/markets/${pair.marketId}/orderbook?${query}`;}
    else{const to=selected.to??new Date().toISOString(),from=selected.from??new Date(Math.min(Date.now()-900000,pair.recoveryFrom?Date.parse(pair.recoveryFrom):Infinity)).toISOString();if(!Number.isFinite(Date.parse(from))||Date.parse(from)>=Date.parse(to)||Date.parse(to)>Date.now())throw Error('CAPTURE_TAPE_WINDOW');query.set('from',from);query.set('to',to);query.set('limit','200');selected.endpoint=`/exchanges/${pair.exchangeId}/trades?${query}`;}
   }
   if(!await admit(false,selected.endpoint))break;const started=new Date().toISOString(),packet=await readGet({endpoint:selected.endpoint,headers});
   if(packet.status!==200){const failure=await saveFailure(packet,selected);
    if(options.dnsRecovery&&!recoveryUsed&&packet.status===0&&packet.transport?.category==='transport_dns'){recoveryUsed=true;await atomicPaperJson(path.join(out,'read-recovery.json'),{phase:'waiting_backoff',failure,retryCount:1,sameEndpoint:true});queue.splice(index+1,0,{...selected,dnsRetry:true});continue;}
    throw Error('CAPTURE_GET_FAILED');
   }
   let data;try{data=JSON.parse(packet.content);}catch{throw Error('CAPTURE_JSON_INVALID');}
   const receivedAt=new Date().toISOString();if(selected.dnsRetry){budget.blocked=false;await persistBudget();await atomicPaperJson(path.join(out,'read-recovery.json'),{phase:'recovered',endpoint:selected.endpoint,receivedAt,status:200,retryCount:1,orderWrites:0});}
   const receipt={schemaVersion:1,source:'official_sig_api_v1',kind:selected.kind,endpoint:selected.endpoint,tournamentId:tournament,marketId:pair.marketId,exchangeId:pair.exchangeId,requestStartedAt:started,receivedAt,status:200,data};fs.appendFileSync(path.join(out,'receipts.jsonl'),JSON.stringify(receipt)+'\n');
   if(selected.kind==='trades'&&data.pagination?.hasMore&&(selected.pageCount??1)<5){if(!data.pagination.nextCursor)throw Error('CAPTURE_CURSOR_MISSING');const endpoint=selected.endpoint.split('&cursor=')[0]+'&cursor='+encodeURIComponent(data.pagination.nextCursor);if(queue.slice(0,index+1).some(q=>q.endpoint===endpoint))throw Error('CAPTURE_CURSOR_LOOP');queue.splice(index+1,0,{...selected,endpoint,pageCount:(selected.pageCount??1)+1});}
  }
 }finally{
  if(headers)headers.Authorization='';
  if(stream&&streamExit===null){stream.stdin.end('{"stop":true}\n');let timer;try{await Promise.race([streamDone,new Promise((_,reject)=>{timer=setTimeout(()=>reject(Error('CAPTURE_STREAM_STOP_TIMEOUT')),5000);})]);}catch{stream.kill('SIGTERM');}finally{clearTimeout(timer);}}
  fs.closeSync(fd);try{if(fs.statSync(lock).ino===lockStat.ino)fs.unlinkSync(lock);}catch{}
 }
}
function args(argv){const o={};for(let i=0;i<argv.length;i++){const k=argv[i];if(['--realtime','--priority','--dns-recovery'].includes(k))o[k.slice(2).replace('-','')]=true;else if(['--pairs','--output','--cycles','--recovery','--universe'].includes(k)&&argv[i+1])o[k.slice(2)]=argv[++i];else throw Error('CAPTURE_ARGUMENT_INVALID');}o.dnsRecovery=o.dnsrecovery;return o;}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href)capture(args(process.argv.slice(2))).catch(error=>{process.stderr.write((/^[A-Z_]+$/.test(error.message)?error.message:'MOLOCH_CAPTURE_ERROR')+'\n');process.exitCode=1;});
