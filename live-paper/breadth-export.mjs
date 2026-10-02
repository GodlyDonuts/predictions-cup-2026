import fs from 'node:fs';
import {fileURLToPath} from 'node:url';
import {verifySource} from './ml-shadow.mjs';
import {validateUniverse,priceBatch,priceEndpoint,normalizePrices} from './breadth.mjs';
function checked(snapshot,u){
 if(snapshot.tournamentId!==u.tournamentId||snapshot.universeHash!==u.hash||snapshot.sourceTimestamp!==null||snapshot.depthAvailable!==false||snapshot.executable!==false||!Array.isArray(snapshot.quotes)||!Array.isArray(snapshot.missingIds))throw Error('Invalid captured bulk snapshot');
 const ids=[...snapshot.quotes.map(q=>q.exchangeId),...snapshot.missingIds].sort().join(',');
 let index=-1;for(let i=0;i<Math.ceil(u.exchanges.length/100);i++)if(priceBatch(u,i).map(q=>q.exchangeId).sort().join(',')===ids)index=i;
 if(index<0)throw Error('Bulk snapshot does not match a catalogue batch');
 const batch=priceBatch(u,index),normalized=normalizePrices({endpoint:priceEndpoint(batch,u.tournamentId),requestStartedAt:snapshot.requestStartedAt,receivedAt:snapshot.receivedAt,data:{data:snapshot.quotes,missingIds:snapshot.missingIds}},batch,u);
 return {index,snapshot:normalized};
}
export function extractLatestBulk(raw,universe){
 const records=verifySource(raw);if(!records?.length)throw Error('Complete source required');const u=validateUniverse(universe,universe.tournamentId),latest=new Map();
 for(const r of records){const b=r.event.bulkPrices;if(!b)continue;const {index,snapshot}=checked(b,u);if(Date.parse(snapshot.receivedAt)>Date.parse(r.event.clockAt||r.at))throw Error('Bulk receipt after event clock');if(latest.has(index)&&Date.parse(snapshot.receivedAt)<Date.parse(latest.get(index).receivedAt))throw Error('Bulk receipt rollback');latest.set(index,snapshot);}
 return {schemaVersion:1,kind:'derived_latest_market_bulk_batches',asOf:records.at(-1).event.clockAt||records.at(-1).at,snapshots:[...latest.entries()].sort((a,b)=>a[0]-b[0]).map(x=>x[1])};
}
export function exportBreadth(input,universe){
 if(input.schemaVersion!==1||input.kind!=='derived_latest_market_bulk_batches'||!Number.isFinite(Date.parse(input.asOf))||!Array.isArray(input.snapshots))throw Error('Invalid bulk export input');
 const u=validateUniverse(universe,universe.tournamentId),quotes=new Map(),seen=new Set();
 for(const raw of input.snapshots){const {index,snapshot}=checked(raw,u);if(seen.has(index)||Date.parse(snapshot.receivedAt)>Date.parse(input.asOf))throw Error('Duplicate/future bulk batch');seen.add(index);
  for(const q of snapshot.quotes)quotes.set(q.exchangeId,{quote:q,requestStartedAt:snapshot.requestStartedAt,receivedAt:snapshot.receivedAt,status:'observed'});
  for(const id of snapshot.missingIds)quotes.set(id,{quote:null,requestStartedAt:snapshot.requestStartedAt,receivedAt:snapshot.receivedAt,status:'missing_in_latest_batch'});
 }
 const exchanges=u.exchanges.map(spec=>{const o=quotes.get(spec.exchangeId),q=o?.quote;return {marketId:spec.marketId,exchangeId:spec.exchangeId,option:spec.option,title:null,party:null,race:null,stage:null,settlementPredicate:null,identityMetadataStatus:'not_present_in_local_id_catalogue',status:o?.status||'not_observed',latestPrice:q?.latestPrice??null,bestBid:q?.bestBid??null,bestAsk:q?.bestAsk??null,spread:q?.spread??null,crossed:q?.crossed??null,requestStartedAt:o?.requestStartedAt??null,receivedAt:o?.receivedAt??null,collectionAgeMs:o?Date.parse(input.asOf)-Date.parse(o.receivedAt):null,sourceTimestamp:null,sourceQuoteAgeMs:null,depthAvailable:false,executable:false};});
 return {schemaVersion:1,kind:'bulk_price_lead_generation_only',asOf:input.asOf,tournamentId:u.tournamentId,universeHash:u.hash,universeSize:exchanges.length,batchesObserved:seen.size,exchanges,limitations:['Mixed collection ages; not a simultaneous snapshot','Official bulk schema supplies neither engine quote time nor quantities; receipt age is not quote freshness','Identity titles, party, race, stage and exact settlement predicates are unavailable in the local catalogue and remain null','External comparisons require an independently verified exact contract join, fresh depth, rules and cost/risk checks','Derived normalized snapshots cannot independently verify the unshared original journal or omitted account context','No source prices are asserted as fair probabilities or executable fills'],networkRequests:0,competitionOrders:0};
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===process.argv[1]){const arg=n=>{const i=process.argv.indexOf(n);if(i<0||!process.argv[i+1])throw Error('Required '+n);return process.argv[i+1];};const u=JSON.parse(fs.readFileSync(arg('--universe'),'utf8')),input=process.argv.includes('--journal')?extractLatestBulk(fs.readFileSync(arg('--journal'),'utf8'),u):JSON.parse(fs.readFileSync(arg('--snapshots'),'utf8'));console.log(JSON.stringify(exportBreadth(input,u)));}
