import {validatePriceObservation} from './market-price-publication.mjs';
export const RELATIVE_DIP_POLICY=Object.freeze({horizonMs:300000,maxAgeMs:120000,maxLatencyMs:20000,windowMs:21600000,minMinutes:30,minSpanMs:1800000,minPeers:12,maximumSpread:.02,minimumDip:.02,residualThreshold:-.05,episodeSpacingMs:1800000});
const movementCache=new WeakMap();
const median=a=>{const b=[...a].sort((x,y)=>x-y),m=Math.floor(b.length/2);return b.length%2?b[m]:(b[m-1]+b[m])/2;};
const logit=p=>Math.log(Math.max(.02,Math.min(.98,p))/(1-Math.max(.02,Math.min(.98,p))));
// Cached title parsing is a research grouping only, not resolution verification.
export function relativeIdentity(title){const m=/^Will the (Democratic|Republican) Party win the (.+)\?$/.exec(title||'');return m?{race:m[2],sign:m[1]==='Democratic'?1:-1}:null;}
export function createRelativeHistory(universe,identities){return {universeHash:universe.hash,lastAt:null,series:{},identities:Object.fromEntries(universe.exchanges.map(x=>[x.exchangeId,relativeIdentity(identities[x.marketId]?.title)]))};}
export function observeRelativePrices(state,snapshot,universe,policy=RELATIVE_DIP_POLICY){
 validatePriceObservation(snapshot,universe);const now=Date.parse(snapshot.receivedAt);
 if(state.universeHash!==universe.hash||state.lastAt!==null&&now<Date.parse(state.lastAt))throw Error('RELATIVE_SCOPE_CLOCK');
 movementCache.delete(state);
 for(const id of snapshot.missingIds)if(state.series[id])state.series[id].unavailable=true;
 for(const q of snapshot.quotes){const s=state.series[q.exchangeId]??={marketId:q.marketId,points:[],unavailable:false};
  if(now-Date.parse(snapshot.requestStartedAt)>policy.maxLatencyMs||q.bestBid===null||q.bestAsk===null||q.crossed||q.bestBid<=0||q.bestAsk>=1){s.unavailable=true;continue;}
  const point={at:snapshot.receivedAt,bucket:Math.floor(now/60000),bid:q.bestBid,ask:q.bestAsk,mid:(q.bestBid+q.bestAsk)/2};
  if(s.points.at(-1)?.bucket===point.bucket)s.points[s.points.length-1]=point;else s.points.push(point);
  s.points=s.points.filter(p=>Date.parse(p.at)>=now-policy.windowMs);s.unavailable=false;
 }
 state.lastAt=snapshot.receivedAt;return state;
}
function movement(series,now,policy){
 if(!series||series.unavailable)return null;
 const current=series.points.findLast(p=>Date.parse(p.at)<=now),target=now-policy.horizonMs,prior=series.points.findLast(p=>Date.parse(p.at)<=target);
 if(!current||!prior||now-Date.parse(current.at)>policy.maxAgeMs||target-Date.parse(prior.at)>policy.maxAgeMs||current.ask-current.bid>policy.maximumSpread+1e-9||prior.ask-prior.bid>policy.maximumSpread+1e-9)return null;
 return {current,prior,change:logit(current.mid)-logit(prior.mid)};
}
export function relativeDipFeatures(state,exchangeId,clockAt,policy=RELATIVE_DIP_POLICY){
 const now=Date.parse(clockAt);if(!Number.isFinite(now)||state.lastAt!==null&&now<Date.parse(state.lastAt))throw Error('RELATIVE_FEATURE_CLOCK');
 let cached=movementCache.get(state);if(!cached||cached.now!==now||cached.policy!==policy){cached={now,policy,moves:Object.fromEntries(Object.entries(state.series).map(([id,s])=>[id,movement(s,now,policy)]))};movementCache.set(state,cached);}
 const own=cached.moves[exchangeId],identity=state.identities[exchangeId];
 if(!own||!identity)return {ready:false,reason:!identity?'unclassified_identity':'unaligned_quotes'};
 const races=new Map();
 for(const id of Object.keys(state.series)){const meta=state.identities[id];if(!meta||meta.race===identity.race)continue;const move=cached.moves[id];if(!move)continue;
  // Count one independent race; prefer its Democratic contract consistently.
  const old=races.get(meta.race);if(!old||meta.sign>old.sign)races.set(meta.race,{sign:meta.sign,value:meta.sign*move.change});
 }
 const peerChanges=[...races.values()].map(x=>x.value),factor=peerChanges.length?median(peerChanges):null;
 const past=state.series[exchangeId].points.filter(p=>p.bucket<own.current.bucket-1&&Date.parse(p.at)>=now-policy.windowMs),bids=past.map(p=>p.bid).sort((a,b)=>a-b);
 const enough=past.length>=policy.minMinutes&&Date.parse(past.at(-1)?.at)-Date.parse(past[0]?.at)>=policy.minSpanMs;
 const referenceBid=enough?bids[Math.floor((bids.length-1)*.25)]:null,discount=referenceBid===null?null:referenceBid-own.current.ask;
 const ready=enough&&peerChanges.length>=policy.minPeers;
 return {ready,reason:ready?'aligned_prior_reference':!enough?'insufficient_history':'insufficient_independent_peers',marketId:state.series[exchangeId].marketId,exchangeId,clockAt,quoteAt:own.current.at,entryAsk:own.current.ask,currentBid:own.current.bid,referenceBid,discount,ownChange:own.change,peerFactor:factor,peerRaces:peerChanges.length,residual:factor===null?null:own.change-identity.sign*factor,localDip:ready&&discount>=policy.minimumDip-1e-9,residualDip:ready&&discount>=policy.minimumDip-1e-9&&own.change-identity.sign*factor<=policy.residualThreshold,executionAuthority:false};
}
