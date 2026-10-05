import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import * as currentHost from './probability-receipt-host-v3.mjs';
import * as additionalHost from './probability-receipt-host.mjs';
import {frozenPaperModule} from './frozen-paper-runtime.mjs';
import {projectProbabilityReadFocus} from './probability-read-focus.mjs';
import {writeObservedProjections} from './executable-edge-observe.mjs';
export async function runProbabilityReceipt(options){
 const bank=JSON.parse(fs.readFileSync(path.join(options.runtimeDir,'probability-receipt-cohorts',options.cohort,'bank.json')));
 const host=await frozenPaperModule({runtimeDir:options.runtimeDir,bank,modulePath:'live-paper/probability-receipt-host-v3.mjs',fallback:currentHost});
 const w=await host.openProbabilityReceipt({...options,workspaceDir:options.workspaceDir||path.dirname(path.dirname(fileURLToPath(import.meta.url)))});let stopping=false,additional=null;
 const end=()=>stopping=true,marker=path.join(w.dir,'OBSERVER_STOP');process.on('SIGINT',end);process.on('SIGTERM',end);
 try{
  if(fs.existsSync(marker))throw Error('PROBABILITY_HOST_OBSERVER_STOP');
  const companionPointer=path.join(options.runtimeDir,'calibrated-poll-paper-active.json');
  if(fs.existsSync(companionPointer)){
   const pointer=JSON.parse(fs.readFileSync(companionPointer));
   if(!/^[a-z][a-z0-9-]{1,63}$/.test(pointer.cohort)||pointer.cohort===options.cohort)throw Error('INVALID_ADDITIONAL_PAPER_COHORT');
   const companionBank=JSON.parse(fs.readFileSync(path.join(options.runtimeDir,'probability-receipt-cohorts',pointer.cohort,'bank.json')));
   if(companionBank.schemaVersion!==2)throw Error('UNSUPPORTED_ADDITIONAL_PAPER_BANK');
   const companionHost=await frozenPaperModule({runtimeDir:options.runtimeDir,bank:companionBank,modulePath:'live-paper/probability-receipt-host.mjs',fallback:additionalHost});
   additional=await companionHost.openProbabilityReceipt({...options,cohort:pointer.cohort});
   if(additional.status().bankHash!==pointer.bankHash)throw Error('ADDITIONAL_PAPER_BANK_MISMATCH');
  }
  do{
   if(stopping||fs.existsSync(marker))break;
   w.consume();const stats=w.status(),lock=JSON.parse(fs.readFileSync(path.join(w.dir,'worker.lock')));
   await writeObservedProjections([[path.join(w.dir,'stats.json'),stats],[path.join(w.dir,'read-focus.json'),projectProbabilityReadFocus(stats,lock,options.cohort)],[path.join(options.runtimeDir,'probability-receipt-active.json'),{cohort:options.cohort,bankHash:stats.bankHash}]]);
   if(additional){
    if(fs.existsSync(path.join(additional.dir,'OBSERVER_STOP'))){additional.close();additional=null;}
    else{additional.consume();const companionStats=additional.status(),companionLock=JSON.parse(fs.readFileSync(path.join(additional.dir,'worker.lock')));
     await writeObservedProjections([[path.join(additional.dir,'stats.json'),companionStats],[path.join(additional.dir,'read-focus.json'),projectProbabilityReadFocus(companionStats,companionLock,companionStats.cohort)]]);
    }
   }
   if(options.once)break;await new Promise(r=>setTimeout(r,1000));
  }while(true);
 }finally{process.removeListener('SIGINT',end);process.removeListener('SIGTERM',end);if(additional)additional.close();w.close();}
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const args=process.argv.slice(2),i=args.indexOf('--name');runProbabilityReceipt({runtimeDir:path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),cohort:args[i+1],once:args.includes('--once')}).catch(()=>{console.error('Probability continuation worker stopped; inspect restricted evidence');process.exitCode=1;});
}
