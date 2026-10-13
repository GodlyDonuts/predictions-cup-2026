import fs from 'node:fs';import path from 'node:path';import {spawn} from 'node:child_process';import {fileURLToPath,pathToFileURL} from 'node:url';
import {createMoloch,advanceMoloch,reportMoloch,assertMoloch} from './engine.mjs';import {receiptEvents} from './sig-source.mjs';import {payloadHash} from '../execution/ledger.mjs';import {collectFrozenPaperFiles} from '../live-paper/frozen-paper-graph.mjs';import {createHash} from 'node:crypto';
const root=fileURLToPath(new URL('../',import.meta.url));
export async function live({marketId,exchangeId,outputDirectory,cycles=10,resumeDirectory=null}){
  if(!/^[1-9]\d*$/.test(marketId)||!/^[1-9]\d*$/.test(exchangeId)||!Number.isSafeInteger(cycles)||cycles<1||cycles>100)throw Error('MOLOCH_LIVE_SCOPE');
  const out=path.resolve(outputDirectory);if(fs.existsSync(out))throw Error('MOLOCH_LIVE_OUTPUT_EXISTS');fs.mkdirSync(out,{recursive:true});
  const cutoff=new Date().toISOString();let state=createMoloch(),continuation=null;
  if(resumeDirectory){const prior=path.resolve(resumeDirectory),oldStats=JSON.parse(fs.readFileSync(path.join(prior,'stats.json'))),snapshot=JSON.parse(fs.readFileSync(path.join(prior,'state.json'))),{hash,...body}=snapshot;
    if(!['completed','failed'].includes(oldStats.status)||oldStats.captureExit===null||hash!==payloadHash(body))throw Error('MOLOCH_CONTINUATION_REQUIRES_VERIFIED_STOPPED_PARENT');
    if(oldStats.marketId!==marketId||oldStats.exchangeId!==exchangeId)throw Error('MOLOCH_CONTINUATION_SCOPE');
    const silence=state.policy.maximumSourceSilenceMs;state=body.state;assertMoloch(state);const nextPolicy=createMoloch({...state.policy,maximumSourceSilenceMs:silence}).policy;const {policy,policyHash,...financial}=state;
    continuation={parentDirectory:prior,parentStatus:oldStats.status,parentCaptureExit:oldStats.captureExit,parentSnapshotHash:hash,parentFinancialHash:payloadHash(financial),newPaperCapital:'0.000000',priorPolicyHash:policyHash};
    state.policy=nextPolicy;state.policyHash=payloadHash(nextPolicy);const {policy:np,policyHash:nh,...carried}=state;if(payloadHash(carried)!==continuation.parentFinancialHash)throw Error('MOLOCH_FINANCIAL_CONTINUITY');
  }
  const capture=path.join(out,'capture'),receiptFile=path.join(capture,'receipts.jsonl');
  const captureProcess=spawn('pwsh',['-NoProfile','-File',path.join(root,'moloch','capture-sig.ps1'),'-MarketId',marketId,'-ExchangeId',exchangeId,'-OutputDirectory',capture,'-Cycles',String(cycles),'-HumanAuthorizedSharedReadCapture'],{windowsHide:true,stdio:['ignore','ignore','pipe']});
  const codeIdentity={...collectFrozenPaperFiles(root,['moloch/live.mjs']),'moloch/capture-sig.ps1':createHash('sha256').update(fs.readFileSync(path.join(root,'moloch','capture-sig.ps1'))).digest('hex')};
  fs.writeFileSync(path.join(out,'manifest.json'),JSON.stringify({bot:'Moloch',cutoff,initialPaperCash:state.policy.initialCash,policy:state.policy,codeIdentity,continuation,accountSharedWithBaal:true,humanAuthorizedRecreatedReadCapture:true,contestOrders:0},null,2));
  let ended=false,code=null,offset=0,pending=Buffer.alloc(0),sequence=0,previous='',lastTick=0,officialReceipts=0;
  let captureFailed=false,captureHttpStatus=null;captureProcess.stderr.on('data',b=>{captureFailed=true;const match=/SIG_GET_FAILED_(\d+)/.exec(b.toString());if(match)captureHttpStatus=Number(match[1]);});captureProcess.on('error',()=>{ended=true;code=-1;captureFailed=true;});captureProcess.on('exit',n=>{ended=true;code=n;});
  const stopping=()=>{fs.writeFileSync(path.join(out,'STOP'),'human/runtime stop\n');if(fs.existsSync(capture))fs.writeFileSync(path.join(capture,'STOP'),'stop\n');};
  process.once('SIGINT',stopping);process.once('SIGTERM',stopping);
  function apply(event){const candidate=structuredClone(state);advanceMoloch(candidate,event);const body={sequence:sequence+1,previous,event,stateHash:payloadHash(candidate)},row={...body,hash:payloadHash(body)};
    fs.appendFileSync(path.join(out,'events.jsonl'),JSON.stringify(row)+'\n');Object.assign(state,candidate);sequence++;previous=row.hash;}
  function persist(status){const report={...reportMoloch(state),coreNetworkRequests:0,networkRequests:officialReceipts,networkRequestCountScope:'successful capture receipts only; excludes failed/in-flight requests',mode:'prospective_official_sig_paper',cutoff,status,capturePid:captureProcess.pid,captureExit:code,captureHttpStatus,marketId,exchangeId,officialReceipts,transport:'official_rest_polling',liveConnectionVerified:officialReceipts>0,financialFillsCounterfactual:true,maximumLocalReadsPerMinute:5,accountSharedWithBaal:true};
    const temp=path.join(out,'state.tmp'),body={state,sequence,previous};fs.writeFileSync(temp,JSON.stringify({...body,hash:payloadHash(body)}));fs.renameSync(temp,path.join(out,'state.json'));
    fs.writeFileSync(path.join(out,'stats.tmp'),JSON.stringify(report,null,2));fs.renameSync(path.join(out,'stats.tmp'),path.join(out,'stats.json'));return report;}
  try{while(true){
    if(fs.existsSync(path.join(out,'STOP'))){stopping();if(ended)break;}
    if(fs.existsSync(receiptFile)){
      const st=fs.statSync(receiptFile);if(st.size<offset)throw Error('MOLOCH_CAPTURE_TRUNCATED');
      if(st.size>offset){const fd=fs.openSync(receiptFile,'r');try{const b=Buffer.alloc(Math.min(st.size-offset,1048576));const n=fs.readSync(fd,b,0,b.length,offset);offset+=n;pending=Buffer.concat([pending,b.subarray(0,n)]);}finally{fs.closeSync(fd);}
        let n;while((n=pending.indexOf(10))>=0){const line=pending.subarray(0,n).toString('utf8').replace(/^\uFEFF/,'');pending=Buffer.from(pending.subarray(n+1));if(!line.trim())continue;
          const r=JSON.parse(line);const events=receiptEvents(r);officialReceipts++;for(const e of events)apply({...e,processedAt:new Date().toISOString()});
        }
        if(pending.length>1048576)throw Error('MOLOCH_CAPTURE_EVENT_TOO_LARGE');
      }
    }
    if(Date.now()-lastTick>=1000){const now=new Date().toISOString();apply({type:'tick',receivedAt:now,processedAt:now});lastTick=Date.now();persist('running');}
    if(ended){if(pending.length)throw Error('MOLOCH_PARTIAL_CAPTURE');if(code!==0||captureFailed)throw Error('MOLOCH_CAPTURE_FAILED: inspect sanitized capture status; no restart');break;}
    await new Promise(r=>setTimeout(r,100));
  }return persist('completed');
  }catch(error){stopping();captureProcess.kill();persist('failed');throw error;}finally{process.removeListener('SIGINT',stopping);process.removeListener('SIGTERM',stopping);}
}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){live({marketId:process.argv[2],exchangeId:process.argv[3],outputDirectory:process.argv[4],cycles:Number(process.argv[5]??10),resumeDirectory:process.argv[6]??null}).then(r=>console.log(JSON.stringify(r,null,2))).catch(e=>{console.error(e.message);process.exitCode=1;});}
