import {Worker} from 'node:worker_threads';
import {isVerifiedPortfolioSource} from './portfolio-bank.mjs';
// Histories are shared across candidate wallets. Every shard must advance the
// same supported instruments; otherwise use one worker to preserve causality.
export function partitionReplayModels(models,workers){
 if(!Number.isSafeInteger(workers)||workers<1||workers>8)throw Error('Invalid offline replay worker count');
 const parts=Array.from({length:Math.min(workers,models.length)},()=>[]);
 for(let i=0;i<models.length;i++)parts[i%parts.length].push(models[i]);
 const scope=items=>JSON.stringify([...new Set(items.flatMap(m=>(m.coverage||[]).filter(c=>c.eligible).flatMap(c=>(c.trainingInstruments||[]).map(id=>c.marketId+':'+id))))].sort());
 return parts.every(part=>scope(part)===scope(models))?parts:[models];
}
export async function parallelPortfolioReplay(job,{workers=4}={}){
 if(!Number.isSafeInteger(workers)||workers<1||workers>8)throw Error('Invalid offline replay worker count');
 if(!isVerifiedPortfolioSource(job.s))throw Error('Unverified offline portfolio source');
 const models=job.modelBank.selected,parts=partitionReplayModels(models,workers);
 if(!parts.length)return new Map();
 job.onProgress?.({phase:'replay_worker_allocation',requestedWorkers:workers,activeWorkers:parts.length});
 const active=new Set();
 try{
  const results=await Promise.all(parts.map((selected,index)=>new Promise((resolve,reject)=>{
   const worker=new Worker(new URL('./portfolio-replay-worker.mjs',import.meta.url),{
    workerData:{records:job.s.records,prefixHashes:job.s.records.prefixHashes,
     config:job.config,modelBank:{...job.modelBank,selected},startCount:job.startCount,endCounts:job.endCounts},
    resourceLimits:{maxOldGenerationSizeMb:768}});
   active.add(worker);let received=false;
   worker.on('message',message=>{
    try{
     if(message.kind==='progress')job.onProgress?.({...message.progress,worker:index});
     else if(message.kind==='result'){received=true;resolve(message.entries);}
     else reject(Error('Invalid offline replay worker response'));
    }catch(error){reject(error);}
   });
   worker.on('error',reject);worker.on('exit',code=>{active.delete(worker);if(code!==0||!received)reject(Error('Offline replay worker failed'));});
  })));
  const paths=new Map();
  for(const entries of results)for(const [id,points]of entries){if(paths.has(id))throw Error('Duplicate portfolio replay result');paths.set(id,points);}
  return paths;
 // Let native worker runtimes exit normally. Forced termination during a
 // rejected progress callback can crash this Windows Node/V8 build.
 }finally{await Promise.all([...active].map(worker=>new Promise(resolve=>worker.once('exit',resolve))));}
}
