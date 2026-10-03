import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {fileURLToPath} from 'node:url';
import {openExecutableEdge} from './executable-edge-watch.mjs';import {projectEdgeTrials} from './edge-trial-projection.mjs';import {atomicJson} from '../execution/ledger.mjs';
import {openLiveBidAnchor} from './bid-anchor-live-host.mjs';
export async function runObservedEdge(options){
 const wrapperHash=createHash('sha256').update(fs.readFileSync(fileURLToPath(import.meta.url))).update(fs.readFileSync(new URL('./edge-trial-projection.mjs',import.meta.url))).digest('hex');
 const w=openExecutableEdge(options);let stopping=false;const end=()=>stopping=true;process.on('SIGINT',end);process.on('SIGTERM',end);
 const stopFile=path.join(w.dir,'OBSERVER_STOP'),projection=path.join(w.dir,'trial-observation.json');
 let companion=null,companionFailed=false;
 const companionFailure=()=>{companionFailed=true;try{companion?.close();}catch{}finally{companion=null;}atomicJson(path.join(w.dir,'companion-status.json'),{mode:'bid_anchor_companion_failed_closed',at:new Date((options.now||Date.now)()).toISOString(),cohort:options.bidAnchor,competitionOrders:0,networkRequests:0});};
 const consumeCompanion=()=>{if(!options.bidAnchor||companionFailed)return;try{companion??=openLiveBidAnchor({...options,cohort:options.bidAnchor,initialize:false});companion.consume();atomicJson(path.join(companion.dir,'stats.json'),companion.status());}catch{companionFailure();}};
 const publish=workerState=>{const stats=w.status(),lock=JSON.parse(fs.readFileSync(path.join(w.dir,'worker.lock')));atomicJson(path.join(w.dir,'stats.json'),stats);atomicJson(projection,projectEdgeTrials(w.state(),w.bank,stats,{pid:process.pid,lockId:lock.id,wrapperHash,workerState}));};
 try{if(fs.existsSync(stopFile))throw Error('EDGE_OBSERVER_STOP');do{if(stopping||fs.existsSync(stopFile))break;w.consume();consumeCompanion();publish('running');if(options.once)break;await new Promise(r=>setTimeout(r,1000));}while(true);if(!options.once)publish('stopped');}
 finally{process.removeListener('SIGINT',end);process.removeListener('SIGTERM',end);try{companion?.close();}finally{w.close();}}
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){const args=process.argv.slice(2),i=args.indexOf('--name'),a=args.indexOf('--bid-anchor');runObservedEdge({runtimeDir:path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),cohort:args[i+1],bidAnchor:a>=0?args[a+1]:undefined,once:args.includes('--once')}).catch(()=>{console.error('Observed paper worker stopped; inspect preserved restricted evidence');process.exitCode=1;});}
