import {atomicPaperJson,PaperPersistenceError} from './paper-persistence.mjs';
// Curated read-only market coverage. Caller awaits this before another API read.
// Reuse reviewed fsynced unique-temp bounded rename retries; never unlink target.
export async function checkpointCoverage(file,value,{canCommit=()=>true,onFault=()=>{},persist=atomicPaperJson}={}){
 try{return await persist(file,value,{canCommit});}
 catch(error){const details=error instanceof PaperPersistenceError?error.details:{};
  onFault({phase:details.phase||'unknown',osCode:details.osCode||'UNKNOWN',attempts:details.attempts??0,target:'price-coverage.json',payloadHash:details.payloadHash??null});throw error;
 }
}
