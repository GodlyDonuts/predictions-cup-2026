import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import * as currentHost from './probability-receipt-host-v2.mjs';
import {frozenPaperModule} from './frozen-paper-runtime.mjs';
import {projectProbabilityReadFocus} from './probability-read-focus.mjs';
import {writeObservedProjections} from './executable-edge-observe.mjs';
export async function runProbabilityReceipt(options){
 const bank=JSON.parse(fs.readFileSync(path.join(options.runtimeDir,'probability-receipt-cohorts',options.cohort,'bank.json')));
 const host=await frozenPaperModule({runtimeDir:options.runtimeDir,bank,modulePath:'live-paper/probability-receipt-host-v2.mjs',fallback:currentHost});
 const w=host.openProbabilityReceipt({...options,workspaceDir:options.workspaceDir||path.dirname(path.dirname(fileURLToPath(import.meta.url)))});let stopping=false;
 const end=()=>stopping=true,marker=path.join(w.dir,'OBSERVER_STOP');process.on('SIGINT',end);process.on('SIGTERM',end);
 try{
  if(fs.existsSync(marker))throw Error('PROBABILITY_HOST_OBSERVER_STOP');
  do{
   if(stopping||fs.existsSync(marker))break;
   w.consume();const stats=w.status(),lock=JSON.parse(fs.readFileSync(path.join(w.dir,'worker.lock')));
   await writeObservedProjections([[path.join(w.dir,'stats.json'),stats],[path.join(w.dir,'read-focus.json'),projectProbabilityReadFocus(stats,lock,options.cohort)],[path.join(options.runtimeDir,'probability-receipt-active.json'),{cohort:options.cohort,bankHash:stats.bankHash}]]);
   if(options.once)break;await new Promise(r=>setTimeout(r,1000));
  }while(true);
 }finally{process.removeListener('SIGINT',end);process.removeListener('SIGTERM',end);w.close();}
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const args=process.argv.slice(2),i=args.indexOf('--name');runProbabilityReceipt({runtimeDir:path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),cohort:args[i+1],once:args.includes('--once')}).catch(()=>{console.error('Probability continuation worker stopped; inspect restricted evidence');process.exitCode=1;});
}
