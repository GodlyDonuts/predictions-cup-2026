import fs from 'node:fs';import path from 'node:path';import {spawn} from 'node:child_process';import {fileURLToPath,pathToFileURL} from 'node:url';
import {createMoloch,advanceMoloch,reportMoloch,assertMoloch} from './engine.mjs';import {receiptEvents} from './sig-source.mjs';import {payloadHash} from '../execution/ledger.mjs';import {collectFrozenPaperFiles} from '../live-paper/frozen-paper-graph.mjs';import {createHash} from 'node:crypto';
import {StreamSource,predecessorTradeFacts} from './stream-source.mjs';
const root=fileURLToPath(new URL('../',import.meta.url));
export function liveScopes(marketId,exchangeId,pairs=null,maximum=2){
  const scope=pairs??[{marketId,exchangeId}];
  if(!Array.isArray(scope)||scope.length<1||scope.length>maximum||!scope.some(p=>p.marketId===marketId&&p.exchangeId===exchangeId)||new Set(scope.map(p=>p.exchangeId)).size!==scope.length||new Set(scope.map(p=>p.marketId)).size!==scope.length||scope.some(p=>!(/^[1-9]\d*$/.test(p.marketId)&&/^[1-9]\d*$/.test(p.exchangeId))))throw Error('MOLOCH_LIVE_MULTI_SCOPE');
  return scope.map(({marketId,exchangeId})=>({marketId,exchangeId}));
}
export function captureScopes(scopes,state,at){
  return scopes.map(p=>{const expired=state.orders.filter(q=>!q.retired&&q.exchangeId===p.exchangeId&&Math.min(q.expiresAt,q.cancelEffectiveAt??Infinity)<at);
    if(!expired.length)return p;
    return{...p,recoveryFrom:new Date(Math.min(...expired.map(q=>q.activationAt))).toISOString(),recoveryTo:new Date(Math.max(...expired.map(q=>Math.min(q.expiresAt,q.cancelEffectiveAt??Infinity)))+1).toISOString()};
  });
}
export function retiredPaperScopes(oldScopes,newScopes,state){
  const retired=oldScopes.filter(old=>!newScopes.some(p=>p.marketId===old.marketId&&p.exchangeId===old.exchangeId));
  for(const p of retired){const lots=state.positions[p.exchangeId];if(state.orders.some(q=>q.exchangeId===p.exchangeId&&!q.retired)||lots&&[...lots.YES,...lots.NO].some(l=>l.quantity>0))throw Error('MOLOCH_SCOPE_RETIREMENT_HAS_LIABILITIES');}
  return retired;
}
export function liveUniverse(scopes,value){
  if(!Array.isArray(value)||value.length<1||value.length>1000||value.some(p=>!p||!/^[1-9]\d*$/.test(p.marketId)||!/^[1-9]\d*$/.test(p.exchangeId))||new Set(value.map(p=>p.exchangeId)).size!==value.length||scopes.some(p=>!value.some(q=>q.marketId===p.marketId&&q.exchangeId===p.exchangeId)))throw Error('MOLOCH_UNIVERSE_SCOPE');
  return value.map(({marketId,exchangeId})=>({marketId,exchangeId}));
}
function validateEntryScopes(policy,scopes){if(policy.entryExchangeIds?.some(id=>!scopes.some(p=>p.exchangeId===id)))throw Error('MOLOCH_ENTRY_SCOPE');}
export function paperPolicyOverrides(value,scopes=null){
  if(!value||typeof value!=='object'||Array.isArray(value)||Object.keys(value).some(k=>!['participationBps','lifetimeMs','quantity','retainProfitableHedge','maximumHedgeOvershoot','entryExchangeIds'].includes(k)))throw Error('MOLOCH_POLICY_OVERRIDE_SCOPE');
  createMoloch(value);if(scopes)validateEntryScopes(value,scopes);return{...value};
}
export async function live({marketId,exchangeId,outputDirectory,cycles=10,resumeDirectory=null,pairsFile=null,realtime=false,insideTicks=null,drain=false,resumeEntries=false,policyFile=null,universeFile=null}){
  if(!/^[1-9]\d*$/.test(marketId)||!/^[1-9]\d*$/.test(exchangeId)||!Number.isSafeInteger(cycles)||cycles<1||cycles>100)throw Error('MOLOCH_LIVE_SCOPE');
  if(realtime&&cycles>30)throw Error('MOLOCH_STREAM_BOUNDED_CYCLES');
  const scopes=liveScopes(marketId,exchangeId,pairsFile?JSON.parse(fs.readFileSync(pairsFile,'utf8').replace(/^\uFEFF/,'')):null,realtime?4:2);
  if(realtime&&cycles>Math.floor(60/scopes.length))throw Error('MOLOCH_STREAM_DURATION_LIMIT');
  let universe=null;if(universeFile!==null){if(!realtime||!universeFile||universeFile.startsWith('--'))throw Error('MOLOCH_UNIVERSE_REQUIRES_REALTIME_FILE');let parsed;try{parsed=JSON.parse(fs.readFileSync(universeFile,'utf8').replace(/^\uFEFF/,''));}catch{throw Error('MOLOCH_UNIVERSE_FILE_INVALID');}universe=liveUniverse(scopes,parsed);}
  if(policyFile!==null&&(!policyFile||policyFile.startsWith('--')))throw Error('MOLOCH_POLICY_FILE_REQUIRED');
  let overrides={};if(policyFile){let parsed;try{parsed=JSON.parse(fs.readFileSync(policyFile,'utf8').replace(/^\uFEFF/,''));}catch{throw Error('MOLOCH_POLICY_FILE_INVALID');}overrides=paperPolicyOverrides(parsed,scopes);}
  const out=path.resolve(outputDirectory);if(fs.existsSync(out))throw Error('MOLOCH_LIVE_OUTPUT_EXISTS');fs.mkdirSync(out,{recursive:true});
  const cutoff=new Date().toISOString();let state=createMoloch({...overrides,...((insideTicks===null)?{}:{insideTicks}),...(drain?{entriesEnabled:false}:{})}),continuation=null;
  if(resumeDirectory){const prior=path.resolve(resumeDirectory),oldStats=JSON.parse(fs.readFileSync(path.join(prior,'stats.json'))),snapshot=JSON.parse(fs.readFileSync(path.join(prior,'state.json'))),{hash,...body}=snapshot;
    if(!['completed','failed'].includes(oldStats.status)||oldStats.captureExit===null||hash!==payloadHash(body))throw Error('MOLOCH_CONTINUATION_REQUIRES_VERIFIED_STOPPED_PARENT');
    const retiredScopes=retiredPaperScopes(oldStats.scopes??[{marketId:oldStats.marketId,exchangeId:oldStats.exchangeId}],scopes,body.state);
    const silence=state.policy.maximumSourceSilenceMs;state=body.state;assertMoloch(state);const nextPolicy=createMoloch({...state.policy,...overrides,maximumSourceSilenceMs:silence,...(insideTicks===null?{}:{insideTicks}),...(drain?{entriesEnabled:false}:resumeEntries?{entriesEnabled:true}:{})}).policy;const {policy,policyHash,...financial}=state;
    continuation={parentDirectory:prior,parentStatus:oldStats.status,parentCaptureExit:oldStats.captureExit,parentSnapshotHash:hash,parentFinancialHash:payloadHash(financial),newPaperCapital:'0.000000',priorPolicyHash:policyHash,retiredScopes};
    state.policy=nextPolicy;state.policyHash=payloadHash(nextPolicy);const {policy:np,policyHash:nh,...carried}=state;if(payloadHash(carried)!==continuation.parentFinancialHash)throw Error('MOLOCH_FINANCIAL_CONTINUITY');
  }
  validateEntryScopes(state.policy,scopes);
  const predecessorFacts=realtime?predecessorTradeFacts(resumeDirectory,state,path.join(root,'runtime','moloch')):null;
  const capture=path.join(out,'capture'),receiptFile=path.join(capture,'receipts.jsonl');
  const scopedFile=path.join(out,'scopes.json');const requestedScopes=captureScopes(scopes,state,Date.parse(cutoff));fs.writeFileSync(scopedFile,JSON.stringify(requestedScopes));
  const universePath=universe?path.join(out,'universe.json'):null;if(universePath)fs.writeFileSync(universePath,JSON.stringify(universe));
  const captureProcess=spawn('pwsh',['-NoProfile','-File',path.join(root,'moloch','capture-sig.ps1'),'-ScreenPairsFile',scopedFile,'-OutputDirectory',capture,'-Cycles',String(cycles),'-HumanAuthorizedSharedReadCapture',...(realtime?['-RealtimeCapture']:[]),...(universePath?['-UniversePairsFile',universePath]:[])],{windowsHide:true,stdio:['ignore','ignore','pipe']});
  const codeIdentity={...collectFrozenPaperFiles(root,['moloch/live.mjs']),'moloch/capture-sig.ps1':createHash('sha256').update(fs.readFileSync(path.join(root,'moloch','capture-sig.ps1'))).digest('hex')};
  if(realtime){Object.assign(codeIdentity,collectFrozenPaperFiles(root,['moloch/realtime/capture.mjs']));for(const f of ['moloch/realtime/package.json','moloch/realtime/pnpm-lock.yaml'])codeIdentity[f]=createHash('sha256').update(fs.readFileSync(path.join(root,f))).digest('hex');}
  for(const file of Object.keys(codeIdentity)){const destination=path.join(out,'source',file);fs.mkdirSync(path.dirname(destination),{recursive:true});fs.copyFileSync(path.join(root,file),destination);}
  fs.writeFileSync(path.join(out,'manifest.json'),JSON.stringify({bot:'Moloch',cutoff,scopes,requestedScopes,realtime,observationUniverse:universe?{markets:new Set(universe.map(p=>p.marketId)).size,exchanges:universe.length,hash:payloadHash(universe),financialScopesOnly:scopes}:null,initialPaperCash:state.policy.initialCash,policy:state.policy,codeIdentity,continuation,accountSharedWithBaal:true,humanAuthorizedRecreatedReadCapture:true,contestOrders:0},null,2));
  let ended=false,code=null,offset=0,pending=Buffer.alloc(0),sequence=0,previous='',lastTick=0,officialReceipts=0;
  let captureFailed=false,captureHttpStatus=null;captureProcess.stderr.on('data',b=>{captureFailed=true;const match=/SIG_GET_FAILED_(\d+)/.exec(b.toString());if(match)captureHttpStatus=Number(match[1]);});captureProcess.on('error',()=>{ended=true;code=-1;captureFailed=true;});captureProcess.on('exit',n=>{ended=true;code=n;});
  const mux=realtime?new StreamSource(scopes,'bda92870-621e-47b0-bc3c-3602c5c26f55',cutoff,predecessorFacts,universe??[]):null,streamFile=path.join(capture,'stream','events.jsonl');let streamOffset=0,streamPending=Buffer.alloc(0);
  const stopping=()=>{fs.writeFileSync(path.join(out,'STOP'),'human/runtime stop\n');if(fs.existsSync(capture))fs.writeFileSync(path.join(capture,'STOP'),'stop\n');};
  process.once('SIGINT',stopping);process.once('SIGTERM',stopping);
  function apply(event){const candidate=structuredClone(state);advanceMoloch(candidate,event);const body={sequence:sequence+1,previous,event,stateHash:payloadHash(candidate)},row={...body,hash:payloadHash(body)};
    fs.appendFileSync(path.join(out,'events.jsonl'),JSON.stringify(row)+'\n');Object.assign(state,candidate);sequence++;previous=row.hash;}
  function persist(status){const report={...reportMoloch(state),coreNetworkRequests:0,networkRequests:officialReceipts,networkRequestCountScope:'successful capture receipts only; excludes failed/in-flight requests',mode:'prospective_official_sig_paper',cutoff,status,capturePid:captureProcess.pid,captureExit:code,captureHttpStatus,marketId,exchangeId,officialReceipts,transport:'official_rest_polling',liveConnectionVerified:officialReceipts>0,financialFillsCounterfactual:true,maximumLocalReadsPerMinute:5,accountSharedWithBaal:true};
    const temp=path.join(out,'state.tmp'),body={state,sequence,previous};fs.writeFileSync(temp,JSON.stringify({...body,hash:payloadHash(body)}));fs.renameSync(temp,path.join(out,'state.json'));
    report.scopes=scopes;if(realtime){report.transport='official_realtime_with_serialized_rest_resync';report.financialStreamIntegration=true;report.streamConnectionVerified=mux.metrics.initialResyncs===scopes.length;report.streamMetrics=mux.metrics;report.streamResync=[...mux.status].map(([marketId,s])=>({marketId,...s}));report.authenticationWrites=fs.existsSync(path.join(capture,'stream','stats.json'))?JSON.parse(fs.readFileSync(path.join(capture,'stream','stats.json'))).tokenWrites:0;report.networkRequests+=report.authenticationWrites;report.networkRequestCountScope='successful REST receipts and authentication only; streaming broadcasts are not counted as API requests';}
    fs.writeFileSync(path.join(out,'stats.tmp'),JSON.stringify(report,null,2));fs.renameSync(path.join(out,'stats.tmp'),path.join(out,'stats.json'));return report;}
  try{while(true){
    if(sequence===0&&continuation?.retiredScopes?.length)apply({type:'gap',exchangeIds:continuation.retiredScopes.map(p=>p.exchangeId),receivedAt:cutoff,processedAt:new Date().toISOString()});
    if(fs.existsSync(path.join(out,'STOP'))){stopping();if(ended)break;}
    if(realtime&&fs.existsSync(path.join(capture,'stream','stats.json'))&&JSON.parse(fs.readFileSync(path.join(capture,'stream','stats.json'))).status==='failed')throw Error('MOLOCH_STREAM_FAILED: stop paper admission; no token refresh or silent fallback');
    if(realtime&&fs.existsSync(streamFile)){
      const size=fs.statSync(streamFile).size;if(size<streamOffset)throw Error('MOLOCH_STREAM_TRUNCATED');
      if(size>streamOffset){const fd=fs.openSync(streamFile,'r');try{const b=Buffer.alloc(Math.min(size-streamOffset,1048576));const n=fs.readSync(fd,b,0,b.length,streamOffset);streamOffset+=n;streamPending=Buffer.concat([streamPending,b.subarray(0,n)]);}finally{fs.closeSync(fd);}
        let n;while((n=streamPending.indexOf(10))>=0){const line=streamPending.subarray(0,n).toString('utf8');streamPending=Buffer.from(streamPending.subarray(n+1));if(line.trim())for(const e of mux.row(JSON.parse(line)))apply({...e,processedAt:new Date().toISOString()});}
        if(streamPending.length>1048576)throw Error('MOLOCH_STREAM_EVENT_TOO_LARGE');
      }
    }
    if(fs.existsSync(receiptFile)){
      const st=fs.statSync(receiptFile);if(st.size<offset)throw Error('MOLOCH_CAPTURE_TRUNCATED');
      if(st.size>offset){const fd=fs.openSync(receiptFile,'r');try{const b=Buffer.alloc(Math.min(st.size-offset,1048576));const n=fs.readSync(fd,b,0,b.length,offset);offset+=n;pending=Buffer.concat([pending,b.subarray(0,n)]);}finally{fs.closeSync(fd);}
        let n;while((n=pending.indexOf(10))>=0){const line=pending.subarray(0,n).toString('utf8').replace(/^\uFEFF/,'');pending=Buffer.from(pending.subarray(n+1));if(!line.trim())continue;
          const r=JSON.parse(line);if(!scopes.some(p=>p.marketId===r.marketId&&p.exchangeId===r.exchangeId))throw Error('MOLOCH_UNDECLARED_RECEIPT_SCOPE');const events=mux?mux.rest(r):receiptEvents(r);officialReceipts++;for(const e of events)apply({...e,processedAt:new Date().toISOString()});
        }
        if(pending.length>1048576)throw Error('MOLOCH_CAPTURE_EVENT_TOO_LARGE');
      }
    }
    if(Date.now()-lastTick>=1000){const now=new Date().toISOString();apply({type:'tick',receivedAt:now,processedAt:now});lastTick=Date.now();persist('running');}
    if(ended){if(pending.length||streamPending.length)throw Error('MOLOCH_PARTIAL_CAPTURE');if(fs.existsSync(streamFile)&&fs.statSync(streamFile).size>streamOffset||fs.existsSync(receiptFile)&&fs.statSync(receiptFile).size>offset)continue;if(code!==0||captureFailed)throw Error('MOLOCH_CAPTURE_FAILED: inspect sanitized capture status; no restart');break;}
    await new Promise(r=>setTimeout(r,100));
  }return persist('completed');
  }catch(error){stopping();if(!ended)await new Promise(resolve=>{const timer=setTimeout(()=>{captureProcess.kill();resolve();},40000);captureProcess.once('exit',()=>{clearTimeout(timer);resolve();});});persist('failed');throw error;}finally{process.removeListener('SIGINT',stopping);process.removeListener('SIGTERM',stopping);}
}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){const policyIndex=process.argv.indexOf('--policy'),universeIndex=process.argv.indexOf('--universe');live({marketId:process.argv[2],exchangeId:process.argv[3],outputDirectory:process.argv[4],cycles:Number(process.argv[5]??10),resumeDirectory:process.argv[6]??null,pairsFile:process.argv[7]??null,realtime:process.argv[8]==='--realtime',insideTicks:process.argv[9]===undefined?null:Number(process.argv[9]),drain:process.argv.includes('--drain'),resumeEntries:process.argv.includes('--resume-entries'),policyFile:policyIndex<0?null:process.argv[policyIndex+1]??'',universeFile:universeIndex<0?null:process.argv[universeIndex+1]??''}).then(r=>console.log(JSON.stringify(r,null,2))).catch(e=>{console.error(e.message);process.exitCode=1;});}
