import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {fileURLToPath} from 'node:url';
import {openExecutableEdge} from './executable-edge-watch.mjs';import {projectEdgeTrials} from './edge-trial-projection.mjs';import {atomicJson} from '../execution/ledger.mjs';
export async function runObservedEdge(options){
 const wrapperHash=createHash('sha256').update(fs.readFileSync(fileURLToPath(import.meta.url))).update(fs.readFileSync(new URL('./edge-trial-projection.mjs',import.meta.url))).digest('hex');
 const w=openExecutableEdge(options);let stopping=false;const end=()=>stopping=true;process.on('SIGINT',end);process.on('SIGTERM',end);
 const stopFile=path.join(w.dir,'OBSERVER_STOP'),projection=path.join(w.dir,'trial-observation.json');
 const publish=workerState=>{const stats=w.status(),lock=JSON.parse(fs.readFileSync(path.join(w.dir,'worker.lock')));atomicJson(path.join(w.dir,'stats.json'),stats);atomicJson(projection,projectEdgeTrials(w.state(),w.bank,stats,{pid:process.pid,lockId:lock.id,wrapperHash,workerState}));};
 try{if(fs.existsSync(stopFile))throw Error('EDGE_OBSERVER_STOP');do{if(stopping||fs.existsSync(stopFile))break;w.consume();publish('running');if(options.once)break;await new Promise(r=>setTimeout(r,1000));}while(true);if(!options.once)publish('stopped');}
 finally{process.removeListener('SIGINT',end);process.removeListener('SIGTERM',end);w.close();}
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){const args=process.argv.slice(2),i=args.indexOf('--name');runObservedEdge({runtimeDir:path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),cohort:args[i+1],once:args.includes('--once')}).catch(()=>{console.error('Observed paper worker stopped; inspect preserved restricted evidence');process.exitCode=1;});}
