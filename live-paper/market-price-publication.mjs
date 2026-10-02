import {Journal} from './runtime.mjs';
const keys=['tournamentId','universeHash','requestStartedAt','receivedAt','sourceTimestamp','depthAvailable','executable','quotes','missingIds'];
const quoteKeys=['exchangeId','marketId','option','latestPrice','bestBid','bestAsk','spread','crossed'];
const exact=(x,fields)=>x&&typeof x==='object'&&!Array.isArray(x)&&Object.keys(x).length===fields.length&&Object.keys(x).every(k=>fields.includes(k));
export function validatePriceObservation(snapshot,universe){
 if(!exact(snapshot,keys)||snapshot.universeHash!==universe.hash||snapshot.tournamentId!==universe.tournamentId||snapshot.executable!==false||snapshot.depthAvailable!==false||snapshot.sourceTimestamp!==null||!Array.isArray(snapshot.quotes)||!Array.isArray(snapshot.missingIds))throw Error('MARKET_PRICE_PUBLICATION_NOT_NORMALIZED');
 const start=Date.parse(snapshot.requestStartedAt),end=Date.parse(snapshot.receivedAt);
 if(!Number.isFinite(start)||!Number.isFinite(end)||end<start||new Date(start).toISOString()!==snapshot.requestStartedAt||new Date(end).toISOString()!==snapshot.receivedAt)throw Error('MARKET_PRICE_PUBLICATION_CLOCK');
 const members=new Map(universe.exchanges.map(x=>[x.exchangeId,x])),seen=new Set();
 for(const q of snapshot.quotes){
  const spec=members.get(q?.exchangeId);
  if(!exact(q,quoteKeys)||!spec||spec.marketId!==q.marketId||spec.option!==q.option||seen.has(q.exchangeId))throw Error('MARKET_PRICE_PUBLICATION_IDENTITY');
  for(const k of ['latestPrice','bestBid','bestAsk','spread'])if(q[k]!==null&&(typeof q[k]!=='number'||!Number.isFinite(q[k])||q[k]>1||q[k]<(k==='spread'?-1:0)))throw Error('MARKET_PRICE_PUBLICATION_PRICE');
  if(q.crossed!==(q.bestBid!==null&&q.bestAsk!==null&&q.bestBid>=q.bestAsk))throw Error('MARKET_PRICE_PUBLICATION_CROSSED');seen.add(q.exchangeId);
 }
 for(const id of snapshot.missingIds){if(!members.has(id)||seen.has(id))throw Error('MARKET_PRICE_PUBLICATION_MISSING');seen.add(id);}
 if(seen.size<1||seen.size>100)throw Error('MARKET_PRICE_PUBLICATION_SIZE');return snapshot;
}
export function createMarketPricePublication({file,universe,canOperate=()=>true}){
 const journal=new Journal(file);if(journal.last&&journal.last.event?.snapshot?.universeHash!==universe.hash)throw Error('MARKET_PRICE_PUBLICATION_SCOPE_CHANGED');
 return {publish(snapshot){
  if(!canOperate())throw Error('MARKET_PRICE_PUBLICATION_STOPPED');validatePriceObservation(snapshot,universe);
  const last=journal.last?.event?.snapshot;if(last&&Date.parse(snapshot.receivedAt)<Date.parse(last.receivedAt))throw Error('MARKET_PRICE_PUBLICATION_CLOCK_REVERSED');
  journal.commit(null,{type:'market_price_observation',snapshot});
 },status:()=>({sequence:journal.sequence,hash:journal.previous,universeHash:universe.hash,networkRequests:0})};
}
