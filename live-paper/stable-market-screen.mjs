import {normalizeOfficialFillTick} from '../execution/api-tick-price.mjs';
export const STABLE_SCREEN_POLICY=Object.freeze({historyMs:300000,activityMs:3600000,minimumSpanMs:240000,maximumGapMs:120000,maximumAgeMs:90000,maximumMidpointRangeTicks:2,minimumSpreadTicks:1,maximumSpreadTicks:10,maximumCandidates:16});
export function createStableScreen(){return{lastAt:-Infinity,markets:{},invalidQuotes:0};}
export function observeStableScreen(state,snapshot,availableAt){const next={...state,markets:{...state.markets}},at=Date.parse(availableAt),request=Date.parse(snapshot.requestStartedAt),received=Date.parse(snapshot.receivedAt);if(!Number.isFinite(at)||at<state.lastAt||!Number.isFinite(request)||request>received||received>at)throw Error('STABLE_SCREEN_CLOCK');next.lastAt=at;
 for(const q of snapshot.quotes){let bid,ask;try{bid=normalizeOfficialFillTick(q.bestBid).tick;ask=normalizeOfficialFillTick(q.bestAsk).tick;}catch{delete next.markets[q.exchangeId];next.invalidQuotes++;continue;}
  if(bid<10||ask>190||ask-bid<1||ask-bid>10){delete next.markets[q.exchangeId];continue;}let rows=next.markets[q.exchangeId]?.rows||[];if(rows.length&&received-rows.at(-1).received>STABLE_SCREEN_POLICY.maximumGapMs)rows=[];
  if(!rows.length||request>rows.at(-1).request+2000)rows=rows.concat({at,request,received,bid,ask});
  rows=rows.filter(r=>at-r.at<=STABLE_SCREEN_POLICY.activityMs);next.markets[q.exchangeId]={marketId:q.marketId,exchangeId:q.exchangeId,rows};
 }for(const id of snapshot.missingIds||[])delete next.markets[id];return next;}
export function stableScreenCandidates(state,now,{excludedMarketIds=[],excludedExchangeIds=[]}={}){const p=STABLE_SCREEN_POLICY,excludedM=new Set(excludedMarketIds),excludedE=new Set(excludedExchangeIds),out=[];
 for(const m of Object.values(state.markets)){if(excludedM.has(m.marketId)||excludedE.has(m.exchangeId))continue;const last=m.rows.at(-1),window=m.rows.filter(r=>now-r.at<=p.historyMs);if(!last||now-last.received>p.maximumAgeMs||now<last.at||window.length<4||last.at-window[0].at<p.minimumSpanMs||new Set(window.map(r=>Math.floor(r.request/60000))).size<4)continue;
  const mid=window.map(r=>r.bid+r.ask);if(Math.max(...mid)-Math.min(...mid)>p.maximumMidpointRangeTicks)continue;const activity=m.rows.slice(1).filter((r,i)=>r.bid!==m.rows[i].bid||r.ask!==m.rows[i].ask).length;
  out.push({marketId:m.marketId,exchangeId:m.exchangeId,priceChangesLastHour:activity,latestReceivedAt:new Date(last.received).toISOString(),historySpanMs:last.at-window[0].at,spreadTicks:last.ask-last.bid,indicativeBidTicks:last.bid,indicativeAskTicks:last.ask,executionEligible:false});
 }return out.sort((a,b)=>b.priceChangesLastHour-a.priceChangesLastHour||b.spreadTicks-a.spreadTicks||Number(a.marketId)-Number(b.marketId)).slice(0,p.maximumCandidates);}
