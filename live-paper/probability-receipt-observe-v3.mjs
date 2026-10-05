import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import * as currentHost from './probability-receipt-host-v3.mjs';
import * as additionalHost from './probability-receipt-host.mjs';
import {frozenPaperModule} from './frozen-paper-runtime.mjs';
import {projectProbabilityReadFocus} from './probability-read-focus.mjs';
import {writeObservedProjections} from './executable-edge-observe.mjs';
export const COMPANION_POINTERS=['calibrated-poll-paper-active.json','primary-breadth-paper-active.json'];
export async function openProbabilityCompanions(options,resolveHost=frozenPaperModule){
 const workers=[],seen=new Set([options.cohort]);
 try{
  for(const name of COMPANION_POINTERS){
   const file=path.join(options.runtimeDir,name);if(!fs.existsSync(file))continue;
   const pointer=JSON.parse(fs.readFileSync(file)),dir=path.join(options.runtimeDir,'probability-receipt-cohorts',pointer.cohort);
   if(!/^[a-z][a-z0-9-]{1,63}$/.test(pointer.cohort)||seen.has(pointer.cohort))throw Error('INVALID_ADDITIONAL_PAPER_COHORT');
   seen.add(pointer.cohort);if(fs.existsSync(path.join(dir,'OBSERVER_STOP')))continue;
   const bank=JSON.parse(fs.readFileSync(path.join(dir,'bank.json')));if(bank.schemaVersion!==2)throw Error('UNSUPPORTED_ADDITIONAL_PAPER_BANK');
   const host=await resolveHost({runtimeDir:options.runtimeDir,bank,modulePath:'live-paper/probability-receipt-host.mjs',fallback:additionalHost});
   const worker=await host.openProbabilityReceipt({...options,cohort:pointer.cohort});workers.push(worker);
   if(worker.status().bankHash!==pointer.bankHash)throw Error('ADDITIONAL_PAPER_BANK_MISMATCH');
  }
  return workers;
 }catch(error){for(const worker of workers.reverse())worker.close();throw error;}
}
export async function runProbabilityReceipt(options){
 const bank=JSON.parse(fs.readFileSync(path.join(options.runtimeDir,'probability-receipt-cohorts',options.cohort,'bank.json')));
 const host=await frozenPaperModule({runtimeDir:options.runtimeDir,bank,modulePath:'live-paper/probability-receipt-host-v3.mjs',fallback:currentHost});
 const w=await host.openProbabilityReceipt({...options,workspaceDir:options.workspaceDir||path.dirname(path.dirname(fileURLToPath(import.meta.url)))});let stopping=false,additional=[];
 const end=()=>stopping=true,marker=path.join(w.dir,'OBSERVER_STOP');process.on('SIGINT',end);process.on('SIGTERM',end);
 try{
  if(fs.existsSync(marker))throw Error('PROBABILITY_HOST_OBSERVER_STOP');
  additional=await openProbabilityCompanions(options);
  do{
   if(stopping||fs.existsSync(marker))break;
   w.consume();const stats=w.status(),lock=JSON.parse(fs.readFileSync(path.join(w.dir,'worker.lock')));
   await writeObservedProjections([[path.join(w.dir,'stats.json'),stats],[path.join(w.dir,'read-focus.json'),projectProbabilityReadFocus(stats,lock,options.cohort)],[path.join(options.runtimeDir,'probability-receipt-active.json'),{cohort:options.cohort,bankHash:stats.bankHash}]]);
   for(const companion of [...additional]){
    if(fs.existsSync(path.join(companion.dir,'OBSERVER_STOP'))){companion.close();additional=additional.filter(x=>x!==companion);}
    else{companion.consume();const companionStats=companion.status(),companionLock=JSON.parse(fs.readFileSync(path.join(companion.dir,'worker.lock')));
     await writeObservedProjections([[path.join(companion.dir,'stats.json'),companionStats],[path.join(companion.dir,'read-focus.json'),projectProbabilityReadFocus(companionStats,companionLock,companionStats.cohort)]]);
    }
   }
   if(options.once)break;await new Promise(r=>setTimeout(r,1000));
  }while(true);
 }finally{process.removeListener('SIGINT',end);process.removeListener('SIGTERM',end);for(const companion of additional)companion.close();w.close();}
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const args=process.argv.slice(2),i=args.indexOf('--name');runProbabilityReceipt({runtimeDir:path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),cohort:args[i+1],once:args.includes('--once')}).catch(()=>{console.error('Probability continuation worker stopped; inspect restricted evidence');process.exitCode=1;});
}
