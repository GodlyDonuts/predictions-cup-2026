import {decimalMicros,formatMicros} from './core.mjs';
export const CANDIDATE_MARKOUT_POLICY=Object.freeze({horizonsMinutes:[1,5,15],maxDelayMs:60000,maxBookLatencyMs:20000,episodeSpacingMs:1800000,slippageMicros:5000,quantity:1,depthFractionBps:100});
const time=x=>typeof x==='string'&&Number.isFinite(Date.parse(x))?Date.parse(x):NaN;
const signed=x=>(x<0n?'-':'')+formatMicros(x<0n?-x:x);
// One-share hypothetical entry and future depth-aware mark. These are diagnostics,
// never orders, fills or portfolio P&L. Repeated candidate polls form one episode.
export function measureCandidateMarkouts({decisions,books,now,policy=CANDIDATE_MARKOUT_POLICY}){
 if(!Array.isArray(decisions)||!Array.isArray(books)||!Number.isSafeInteger(now)||JSON.stringify(policy)!==JSON.stringify(CANDIDATE_MARKOUT_POLICY))throw Error('INVALID_CANDIDATE_MARKOUT_INPUT');
 const last=new Map(),episodes=[],codes={};let repeated=0;
 for(const d of [...decisions].sort((a,b)=>time(a.receivedAt)-time(b.receivedAt))){
  if(!Number.isFinite(time(d.clockAt))||time(d.clockAt)<time(d.receivedAt)||time(d.clockAt)>now||!Number.isFinite(time(d.receivedAt))||time(d.receivedAt)>now||typeof d.marketId!=='string'||typeof d.exchangeId!=='string'||typeof d.code!=='string'||d.eligibleForExecution!==false)throw Error('INVALID_CANDIDATE_DECISION');
  if(last.has(d.marketId)&&time(d.receivedAt)-last.get(d.marketId)<policy.episodeSpacingMs){repeated++;continue;}last.set(d.marketId,time(d.receivedAt));codes[d.code]=(codes[d.code]||0)+1;
  const matching=books.filter(b=>b.marketId===d.marketId&&b.exchangeId===d.exchangeId&&time(b.receivedAt)>time(d.receivedAt)&&time(b.receivedAt)<=now).sort((a,b)=>time(a.receivedAt)-time(b.receivedAt));
  const marks=policy.horizonsMinutes.map(minutes=>{
   if(!Number.isSafeInteger(d.bestAskQuantity)||Math.floor(d.bestAskQuantity*policy.depthFractionBps/10000)<1)return {minutes,status:'entry_depth_unavailable',netPerShare:null};
   const target=time(d.clockAt)+minutes*60000;if(target>now)return {minutes,status:'pending',netPerShare:null};
   // Select the first receipt, including unusable evidence. Never skip a thin,
   // slow or invalid first quote to manufacture a better later result.
   const b=matching.find(x=>time(x.receivedAt)>=target);if(!b)return {minutes,status:'unobserved',netPerShare:null};
   if(time(b.receivedAt)-target>policy.maxDelayMs)return {minutes,status:'late',netPerShare:null};
   if(b.freshnessPolicy!=='authoritative_receipt_v1'||b.invalidated||b.valid===false||!Number.isFinite(time(b.requestStartedAt))||time(b.requestStartedAt)<=time(d.clockAt)||time(b.requestStartedAt)>time(b.receivedAt)||time(b.receivedAt)-time(b.requestStartedAt)>policy.maxBookLatencyMs||b.versionKey===d.versionKey)return {minutes,status:'invalid_or_dependent',netPerShare:null};
   const bid=b.bids?.[0],ask=b.asks?.[0];if(!bid||!ask||decimalMicros(bid.price)>=decimalMicros(ask.price)||!Number.isSafeInteger(bid.quantity)||Math.floor(bid.quantity*policy.depthFractionBps/10000)<1)return {minutes,status:'insufficient_or_crossed_depth',netPerShare:null};
   const net=decimalMicros(bid.price)-decimalMicros(d.ask)-2n*BigInt(policy.slippageMicros);return {minutes,status:'observed',netPerShare:signed(net),receivedAt:b.receivedAt,requestStartedAt:b.requestStartedAt,versionKey:b.versionKey};
  });episodes.push({marketId:d.marketId,exchangeId:d.exchangeId,decisionCode:d.code,receivedAt:d.receivedAt,ask:d.ask,marks});
 }
 return {policy,episodes,counts:{decisions:decisions.length,episodes:episodes.length,repeatedPolls:repeated,decisionCodes:codes},addedFees:'0.000000',networkRequests:0,competitionOrders:0,profitabilityEstablished:false,meaning:'Hypothetical one-share bid recovery with two-sided tick allowance; no assumed trade or execution.'};
}
