import fs from 'node:fs';
import {fileURLToPath} from 'node:url';
import {verifySource} from './ml-shadow.mjs';
import {validateUniverse,normalizePrices,priceEndpoint} from './breadth.mjs';

export function breadthFeatures(records,universe){
 const history=new Map(),rows=[];
 for(const r of records){
  const s=r.event?.bulkPrices;if(!s)continue;
  if(s.universeHash!==universe.hash||s.tournamentId!==universe.tournamentId)throw Error('Breadth universe mismatch');
  if(Date.parse(s.receivedAt)>Date.parse(r.event.clockAt||r.at))throw Error('Future breadth observation');
  const batch=[...s.quotes.map(q=>universe.exchanges.find(x=>x.exchangeId===q.exchangeId)),...s.missingIds.map(id=>universe.exchanges.find(x=>x.exchangeId===id))];
  if(batch.some(x=>!x)||batch.length>100)throw Error('Invalid breadth batch');
  normalizePrices({endpoint:priceEndpoint(batch,universe.tournamentId),requestStartedAt:s.requestStartedAt,receivedAt:s.receivedAt,data:{data:s.quotes,missingIds:s.missingIds}},batch,universe);
  for(const id of s.missingIds)history.delete(id);
  for(const q of s.quotes){
   const h=history.get(q.exchangeId),dt=h?Date.parse(s.receivedAt)-Date.parse(h.at):null;
   if(dt!==null&&dt<=0)throw Error('Breadth collection time rollback');
   const paired=q.bestBid!==null&&q.bestAsk!==null&&q.bestBid<q.bestAsk;
   const mid=paired?(q.bestBid+q.bestAsk)/2:null;
   const previous=h&&dt<=600000?h:null;
   rows.push({exchangeId:q.exchangeId,marketId:q.marketId,at:s.receivedAt,mid,spread:paired?q.bestAsk-q.bestBid:null,collectionIntervalMs:dt,midChange:mid!==null&&previous?.mid!==null&&previous?.mid!==undefined?mid-previous.mid:null,quoteChanged:previous?previous.bestBid!==q.bestBid||previous.bestAsk!==q.bestAsk:null,executable:false});
   history.set(q.exchangeId,{at:s.receivedAt,mid,bestBid:q.bestBid,bestAsk:q.bestAsk});
  }
 }
 return rows;
}
export function activityReport(rows){
 const groups=new Map();for(const r of rows){const g=groups.get(r.exchangeId)||{exchangeId:r.exchangeId,marketId:r.marketId,observations:0,quoteChanges:0,pairedObservations:0,spreads:[],absoluteMidChanges:[]};g.observations++;if(r.quoteChanged===true)g.quoteChanges++;if(r.spread!==null){g.pairedObservations++;g.spreads.push(r.spread);}if(r.midChange!==null)g.absoluteMidChanges.push(Math.abs(r.midChange));g.lastCollectionAt=r.at;groups.set(r.exchangeId,g);}
 const median=x=>{if(!x.length)return null;const v=[...x].sort((a,b)=>a-b);return(v[Math.floor((v.length-1)/2)]+v[Math.floor(v.length/2)])/2;};
 return {executable:false,observationMeaning:'Collection polls; quote-source age and depth are unavailable. Quote changes are descriptive, not independent trades or profitability.',exchanges:[...groups.values()].map(({spreads,absoluteMidChanges,...g})=>({...g,medianSpread:median(spreads),meanAbsoluteMidChange:absoluteMidChanges.length?absoluteMidChanges.reduce((a,b)=>a+b,0)/absoluteMidChanges.length:null})).sort((a,b)=>(b.quoteChanges-a.quoteChanges)||((a.medianSpread??Infinity)-(b.medianSpread??Infinity)))};
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===process.argv[1]){
 const arg=n=>process.argv[process.argv.indexOf(n)+1];if(!process.argv.includes('--journal')||!process.argv.includes('--universe'))throw Error('Required --journal and --universe');
 const records=verifySource(fs.readFileSync(arg('--journal'),'utf8'));if(!records)throw Error('Incomplete capture; retry after append');
 const raw=JSON.parse(fs.readFileSync(arg('--universe'),'utf8')),u=validateUniverse(raw,raw.tournamentId);
 console.log(JSON.stringify({...activityReport(breadthFeatures(records,u)),sourceHash:records.at(-1).hash,sourceRecords:records.length,universeHash:u.hash},null,2));
}
