import fs from 'node:fs';import {priceBatch,priceEndpoint,normalizePrices,updateCoverage} from './breadth.mjs';
import {validateNetworkBudget} from './transport-network-budget.mjs';
const fields=['schemaVersion','ownerId','enabled','universeHash','cycleIntervalMs'];
export function validateMarketSweep(value,universe,networkBudget){
 if(!value||typeof value!=='object'||Array.isArray(value)||Object.keys(value).length!==fields.length||Object.keys(value).some(k=>!fields.includes(k))
  ||value.schemaVersion!==1||value.ownerId!=='Codex'||value.enabled!==true||value.universeHash!==universe.hash||value.cycleIntervalMs!==10000
  ||!Array.isArray(universe.exchanges)||universe.exchanges.length<1||universe.exchanges.length>300||![2,3].includes(validateNetworkBudget(networkBudget).schemaVersion))throw Error('INVALID_MARKET_SWEEP_SETTING');
 return Object.freeze({...value});
}
export function loadMarketSweep(file,universe,networkBudget){return fs.existsSync(file)?validateMarketSweep(JSON.parse(fs.readFileSync(file,'utf8')),universe,networkBudget):null;}
// Bulk observations screen candidates. They carry no source timestamp or depth
// and must never substitute for a fresh authoritative execution book.
export async function collectMarketSweep({api,universe,coverage,enabled=false,managedMarketIds=[],isStopped=()=>false,onSnapshot=()=>{},onCoverage=()=>{},now=Date.now}){
 if(!Array.isArray(managedMarketIds))throw Error('INVALID_SWEEP_RISK');
 const count=Math.ceil(universe.exchanges.length/100),start=Number.isInteger(coverage?.nextBatch)&&coverage.nextBatch>=0&&coverage.nextBatch<count?coverage.nextBatch:0;
 const requestedBatches=enabled&&managedMarketIds.length===0?count:1;let next=coverage,primarySnapshot=null,error=null;const snapshots=[];
 for(let i=0;i<requestedBatches;i++){
  if(isStopped()||api.cooldown>now())break;
  const index=(start+i)%count,batch=priceBatch(universe,index);let snapshot;
  try{snapshot=normalizePrices(await api.request(priceEndpoint(batch,universe.tournamentId)),batch,universe);}
  catch(e){if(api.manualReview||e.status===401||e.status===403)throw e;error=isStopped()?'stopped':e.status?'http_'+e.status:'bulk_price_unavailable';break;}
  if(isStopped())break;
  // Durable publication/coverage failures propagate instead of pretending the
  // response was successfully collected. No second request may hide them.
  try{await onSnapshot(snapshot);next=updateCoverage(next,snapshot,universe,index);snapshots.push(snapshot);primarySnapshot??=snapshot;
  // Keep the legacy primary batch rotating, even when all batches are read.
  next.nextBatch=(start+1)%count;await onCoverage(next);}catch{const e=Error('Market sweep durable recording failed');e.durableRecordingFailed=true;throw e;}
 }
 return {coverage:next,primarySnapshot,snapshots,error,requestedBatches,completedBatches:snapshots.length,complete:snapshots.length===requestedBatches,riskPriority:enabled&&managedMarketIds.length>0};
}
