import {metadataPairs,raceIdentity} from '../strategy.mjs';
import {digest} from './model.mjs';
import {amount,TICK,SCALE} from '../paper/fixed.mjs';

// Metadata identifies research pairs, not settlement guarantees. Preserve the
// exact predicate fingerprints so later evidence cannot silently change a pair.
export function basketDefinitions(markets,nodes,universe){
 if(!Array.isArray(markets)||!nodes||!Array.isArray(universe?.exchanges))throw Error('Invalid basket catalogue');
 const scope=new Map(universe.exchanges.map(m=>[m.marketId,m]));
 return metadataPairs(markets,nodes).map(pair=>{
  const legs=pair.map(m=>{const official=scope.get(m.marketId),node=nodes[m.marketId]?.root;
   if(!official||official.exchangeId!==m.exchangeId||official.option!=='YES')throw Error('Basket catalogue scope mismatch');
   return {marketId:m.marketId,exchangeId:m.exchangeId,predicateHash:digest(node)};
  }).sort((a,b)=>Number(a.marketId)-Number(b.marketId));
  const race=raceIdentity(nodes[legs[0].marketId].root);
  return {id:digest({tournamentId:universe.tournamentId,race,legs}),tournamentId:universe.tournamentId,race,legs,settlementVerified:false};
 });
}

export function basketCandidates(definitions,coverage,universe,now){
 if(!Number.isSafeInteger(now)||!Array.isArray(definitions))throw Error('Invalid basket candidate inputs');
 if(coverage?.universeHash!==universe.hash||coverage.tournamentId!==universe.tournamentId)return [];
 const scope=new Map(universe.exchanges.map(m=>[m.marketId,m])),out=[];
 for(const d of definitions){
  if(d.tournamentId!==universe.tournamentId||!Array.isArray(d.legs)||d.legs.length!==2||new Set(d.legs.map(l=>l.marketId)).size!==2||new Set(d.legs.map(l=>l.exchangeId)).size!==2)throw Error('Basket definition scope mismatch');
  for(const l of d.legs){const m=scope.get(l.marketId);if(!m||m.exchangeId!==l.exchangeId||m.option!=='YES')throw Error('Basket definition scope mismatch');}
  const quotes=d.legs.map(l=>coverage.quotes?.[l.exchangeId]);
  if(quotes.some((q,i)=>!q||q.exchangeId!==d.legs[i].exchangeId||q.marketId!==d.legs[i].marketId||q.option!=='YES'||q.crossed===true||!Number.isFinite(q.bestBid)||!Number.isFinite(q.bestAsk)||q.bestBid<=0||q.bestAsk>=1||q.bestBid>=q.bestAsk))continue;
  const receipt=quotes[0].receivedAt,clock=Date.parse(receipt);
  if(!Number.isFinite(clock)||new Date(clock).toISOString()!==receipt||quotes[1].receivedAt!==receipt||clock>now||now-clock>120000)continue;
  let bids;try{bids=quotes.map(q=>amount(String(q.bestBid)));}catch{continue;}
  if(bids.some(p=>p%TICK!==0n))continue;
  const gap=bids[0]+bids[1]-SCALE-2n*TICK;
  if(gap<=0n)continue;
  out.push({tournamentId:universe.tournamentId,marketIds:d.legs.map(l=>l.marketId),exchangeIds:d.legs.map(l=>l.exchangeId),receivedAt:receipt,priceGapMicros:Number(gap)});
 }
 return out;
}
