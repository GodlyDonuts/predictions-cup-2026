import {readIncrementalSource,incrementalSourceMetrics} from './incremental-journal.mjs';
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {advanceDip,dipReport} from './dip-shadow.mjs';
import {verifySource} from './ml-shadow.mjs';
import {digest} from './model.mjs';
import {lock} from './runtime.mjs';
import {atomicPaperJson,paperFault,persistPaperFault} from './paper-persistence.mjs';

const workspace=path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const cohorts=['dip','dip-scale','dip-responsive'];
// Host-only upgrade: the frozen pure strategy, bank, cutoff and source chain
// remain authoritative. In-memory state advances only after state replacement.
export function openDipWorker({runtimeDir,cohort='dip',workspaceDir=workspace,now=Date.now,writeJson=atomicPaperJson,
 onFault=fault=>process.stderr.write(JSON.stringify(fault)+'\n'),saveFault=persistPaperFault}={}){
 if(!path.isAbsolute(runtimeDir)||!cohorts.includes(cohort))throw Error('Invalid dip host scope');
 const lockFile=path.join(runtimeDir,cohort+'-shadow.lock'),release=lock(lockFile);
 let bank,state,closed=false,failed=false,partials=0,lastStatus='starting',busy=false,phase='read_bank';
 const owned=()=>{try{return !closed&&fs.readFileSync(lockFile,'utf8')===String(process.pid);}catch{return false;}};
 const stopped=()=>['STOP','DIP_STOP'].some(f=>fs.existsSync(path.join(runtimeDir,f)))||fs.existsSync(path.join(workspaceDir,'STOP'));
 const close=()=>{if(closed)return;const releaseOwned=owned();closed=true;if(releaseOwned)release();};
 try{bank=JSON.parse(fs.readFileSync(path.join(runtimeDir,cohort+'-bank.json')));phase='read_state';state=JSON.parse(fs.readFileSync(path.join(runtimeDir,cohort+'-state.json')));}
 catch(error){close();throw error;}
 const context=()=>({cohort,phase,at:new Date(now()).toISOString(),bankHash:state?.bankHash??digest(bank),durableSourceSequence:state?.sourceSequence});
 async function diagnose(error,attemptedSourceSequence){
  const fault=paperFault(error,{...context(),attemptedSourceSequence});
  try{onFault(fault);}catch{} // A broken stderr must not mask the original fault.
  try{await saveFault(runtimeDir,fault);}catch{} // stderr remains the fallback.
  return fault;
 }
 async function publish(status,fault){
  let coverage;try{coverage=JSON.parse(fs.readFileSync(path.join(runtimeDir,'price-coverage.json')));}catch{}
  phase='report';const report={...dipReport(state,bank,new Date(now()).toISOString(),coverage),status,...(fault?{fault}:{}),hostVersion:'dip-worker-v2-incremental-source',sourceReadMetrics:incrementalSourceMetrics(path.join(runtimeDir,'journal.jsonl'))};
  phase='stats';await writeJson(path.join(runtimeDir,cohort+'-stats.json'),report,{canCommit:()=>owned()&&(status!=='running'||!stopped())});lastStatus=status;
 }
 async function poll(){
  if(closed||failed||busy)throw Error('Dip host is not available');busy=true;let attemptedSourceSequence=null;
  try{
   if(stopped()){await publish('stopped');return {status:'stopped',sourceSequence:state.sourceSequence};}
   phase='verify_source';const records=readIncrementalSource(path.join(runtimeDir,'journal.jsonl'));
   if(!records){if(++partials>3)throw Error('Incomplete source');await publish('awaiting_capture');}
   else{
    partials=0;phase='advance';const candidate=advanceDip(state,records,bank);attemptedSourceSequence=candidate.sourceSequence;
    phase='state';await writeJson(path.join(runtimeDir,cohort+'-state.json'),candidate,{canCommit:()=>owned()&&!stopped()});
    state=candidate;await publish(stopped()?'stopped':'running');
   }
   return {status:lastStatus,sourceSequence:state.sourceSequence};
  }catch(error){
   failed=true;lastStatus='failed';const fault=await diagnose(error,attemptedSourceSequence);
   try{await publish('failed',fault);}catch(publishError){await diagnose(publishError,attemptedSourceSequence);}
   return {status:'failed',sourceSequence:state.sourceSequence,fault};
  }finally{busy=false;}
 }
 return {poll,close,stopped};
}

if(process.argv[1]&&fileURLToPath(import.meta.url)===path.resolve(process.argv[1])){
 let worker;const index=process.argv.indexOf('--name'),cohort=index<0?'dip':process.argv[index+1];
 try{
  const runtimeDir=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
  worker=openDipWorker({runtimeDir,cohort});process.on('exit',()=>{try{worker.close();}catch(error){process.stderr.write(JSON.stringify(paperFault(error,{cohort,phase:'release',at:new Date().toISOString()}))+'\n');}});
  do{
   const result=await worker.poll();if(result.status==='failed'){process.exitCode=1;break;}
   if(result.status==='stopped'||process.argv.includes('--once'))break;
   for(let i=0;i<75&&!worker.stopped();i++)await new Promise(r=>setTimeout(r,200));
  }while(true);
 }catch(error){process.stderr.write(JSON.stringify(paperFault(error,{cohort,phase:'host',at:new Date().toISOString()}))+'\n');process.exitCode=1;}
 finally{if(worker)try{worker.close();}catch(error){process.stderr.write(JSON.stringify(paperFault(error,{cohort,phase:'release',at:new Date().toISOString()}))+'\n');process.exitCode=1;}}
}
