import {createHash} from 'node:crypto';

const id=x=>typeof x==='string'&&/^[1-9]\d*$/.test(x)&&Number.isSafeInteger(Number(x));
export function validateUniverse(u,tournamentId){
  if(u?.tournamentId!==tournamentId||!Array.isArray(u.exchanges)||!u.exchanges.length)throw Error('Invalid price universe');
  const seen=new Set();
  for(const x of u.exchanges){if(!id(x.exchangeId)||!id(x.marketId)||seen.has(x.exchangeId)||(x.option!==null&&typeof x.option!=='string'))throw Error('Invalid price universe exchange');seen.add(x.exchangeId);}
  return {...u,hash:createHash('sha256').update(JSON.stringify(u)).digest('hex')};
}
export function priceBatch(u,index){const batches=Math.ceil(u.exchanges.length/100);return u.exchanges.slice((index%batches)*100,(index%batches+1)*100);}
export function priceEndpoint(batch,tournamentId){return '/exchanges/prices?ids='+batch.map(x=>x.exchangeId).join(',')+'&tournamentId='+tournamentId;}
export function normalizePrices(response,batch,u){
  const {data,missingIds}=response.data||{};
  if(response.endpoint!==priceEndpoint(batch,u.tournamentId)||!Array.isArray(data)||!Array.isArray(missingIds)||!Number.isFinite(Date.parse(response.requestStartedAt))||!Number.isFinite(Date.parse(response.receivedAt))||Date.parse(response.receivedAt)<Date.parse(response.requestStartedAt))throw Error('Invalid bulk price response');
  const expected=new Map(batch.map(x=>[x.exchangeId,x])),seen=new Set(),quotes=[];
  for(const x of data){
    const spec=expected.get(x?.exchangeId);
    if(!spec||seen.has(x.exchangeId)||x.marketId!==spec.marketId||x.option!==spec.option)throw Error('Bulk price identity mismatch');
    for(const k of ['latestPrice','bestBid','bestAsk','spread'])if(x[k]!==null&&(typeof x[k]!=='number'||!Number.isFinite(x[k])||x[k]>1||x[k]<(k==='spread'?-1:0)))throw Error('Invalid bulk price value');
    seen.add(x.exchangeId);quotes.push({exchangeId:x.exchangeId,marketId:x.marketId,option:x.option,latestPrice:x.latestPrice,bestBid:x.bestBid,bestAsk:x.bestAsk,spread:x.spread,crossed:x.bestBid!==null&&x.bestAsk!==null&&x.bestBid>=x.bestAsk});
  }
  for(const x of missingIds){if(!expected.has(x)||seen.has(x))throw Error('Invalid bulk price missing IDs');seen.add(x);}
  if(seen.size!==expected.size)throw Error('Incomplete bulk price coverage');
  // The official bulk schema supplies no quote timestamp or depth. Receipt time
  // measures collection age only; these observations cannot authorize fills.
  return {tournamentId:u.tournamentId,universeHash:u.hash,requestStartedAt:response.requestStartedAt,receivedAt:response.receivedAt,sourceTimestamp:null,depthAvailable:false,executable:false,quotes,missingIds};
}
export function updateCoverage(previous,snapshot,u,batchIndex){
  const quotes=previous?.universeHash===u.hash?{...previous.quotes}:{};
  for(const x of snapshot.missingIds)delete quotes[x];
  for(const x of snapshot.quotes)quotes[x.exchangeId]={...x,receivedAt:snapshot.receivedAt,requestStartedAt:snapshot.requestStartedAt};
  return {universeHash:u.hash,tournamentId:u.tournamentId,universeSize:u.exchanges.length,nextBatch:(batchIndex+1)%Math.ceil(u.exchanges.length/100),updatedAt:snapshot.receivedAt,executable:false,sourceTimestamp:null,depthAvailable:false,quotes,lastMissingIds:snapshot.missingIds};
}
