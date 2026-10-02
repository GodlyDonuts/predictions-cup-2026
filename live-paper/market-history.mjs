import {validatePriceObservation} from './market-price-publication.mjs';
export const MARKET_HISTORY_POLICY=Object.freeze({windowMs:21600000,minMinutes:30,minSpanMs:1800000,maxAgeMs:120000,maxSpread:.02,minNetRecovery:.02,lowerQuantile:.25});
export function createMarketHistory(universe){return {universeHash:universe.hash,lastAt:null,observations:0,minutes:{}};}
export function observeMarketHistory(state,snapshot,universe,policy=MARKET_HISTORY_POLICY){
 validatePriceObservation(snapshot,universe);const now=Date.parse(snapshot.receivedAt);
 if(state.universeHash!==universe.hash||state.lastAt!==null&&now<Date.parse(state.lastAt))throw Error('MARKET_HISTORY_SCOPE_OR_CLOCK');
 for(const id of snapshot.missingIds)if(state.minutes[id])state.minutes[id].unavailableAt=snapshot.receivedAt;
 for(const q of snapshot.quotes){const valid=q.bestBid!==null&&q.bestAsk!==null&&!q.crossed&&q.bestBid>0&&q.bestAsk<1;
 const series=state.minutes[q.exchangeId]??={marketId:q.marketId,buckets:[],latest:null,unavailableAt:null};
 if(!valid){series.unavailableAt=snapshot.receivedAt;continue;}
 const bucket=Math.floor(now/60000),point={bucket,at:snapshot.receivedAt,bid:q.bestBid,ask:q.bestAsk};
 if(series.buckets.at(-1)?.bucket===bucket)series.buckets[series.buckets.length-1]=point;else series.buckets.push(point);
 series.buckets=series.buckets.filter(x=>Date.parse(x.at)>=now-policy.windowMs);series.latest=point;series.unavailableAt=null;
 }
 state.lastAt=snapshot.receivedAt;state.observations++;return state;
}
export function rankMarketHistory(state,universe,clockAt,policy=MARKET_HISTORY_POLICY){
 const now=Date.parse(clockAt);if(!Number.isFinite(now)||state.universeHash!==universe.hash||state.lastAt!==null&&now<Date.parse(state.lastAt))throw Error('MARKET_HISTORY_RANK_CLOCK');
 const leads=[],counts={observed:0,unavailable:0,stale:0,insufficientHistory:0,wideSpread:0,noNetDiscount:0,eligible:0};
 for(const spec of universe.exchanges){const s=state.minutes[spec.exchangeId];if(!s)continue;counts.observed++;
 if(s.unavailableAt!==null||!s.latest){counts.unavailable++;continue;}
 const age=now-Date.parse(s.latest.at);if(age<0||age>policy.maxAgeMs){counts.stale++;continue;}
 // Current minute and the immediately prior minute cannot set the reference.
 // Minute weighting prevents faster polling from manufacturing more evidence.
 const past=s.buckets.filter(x=>x.bucket<s.latest.bucket-1&&Date.parse(x.at)>=now-policy.windowMs);
 if(past.length<policy.minMinutes||Date.parse(past.at(-1).at)-Date.parse(past[0].at)<policy.minSpanMs){counts.insufficientHistory++;continue;}
 if(s.latest.ask-s.latest.bid>policy.maxSpread+1e-9){counts.wideSpread++;continue;}
 const bids=past.map(x=>x.bid).sort((a,b)=>a-b),referenceBid=bids[Math.floor((bids.length-1)*policy.lowerQuantile)],netRecovery=referenceBid-s.latest.ask;
 if(netRecovery<policy.minNetRecovery-1e-9){counts.noNetDiscount++;continue;}
 counts.eligible++;leads.push({marketId:spec.marketId,exchangeId:spec.exchangeId,receivedAt:s.latest.at,referenceBid,entryAsk:s.latest.ask,currentBid:s.latest.bid,netRecovery,minutes:past.length,historyThrough:past.at(-1).at,executable:false,reason:'prior_bid_discount_requires_fresh_depth'});
 }
 leads.sort((a,b)=>b.netRecovery-a.netRecovery||a.marketId.localeCompare(b.marketId));return {clockAt,policy,counts,leads,executable:false,networkRequests:0};
}
