import {amount,money,price,instant,TICK,SCALE} from '../paper/fixed.mjs';
import {digest} from './model.mjs';
import {instantNs,timestamp} from '../feed/contract.mjs';

export const CANONICAL_NO_POLICY=Object.freeze({schemaVersion:1,maxAgeMs:20000,maxSkewMs:10000,
 maximumAccountAgeMs:20000,maximumReviewAgeMs:21600000,minimumMarginalEdge:'0.010000',
 minimumTotalProfit:'0.005000',adverseTicksPerLeg:1,feePerShare:'0.000000',fixedFeePerLeg:'0.000000',
 depthFractionBps:100,maxQuantity:100000,maxCost:'25.000000',maxUnmatchedCost:'5.000000',
 representation:'documented_same_exchange_yes_bid_complement',feeEvidence:'direct_human_zero_fee_instruction'});
const need=(ok,c)=>{if(!ok)throw Error('CANONICAL_CAPACITY_'+c);};
const id=v=>typeof v==='string'&&/^[1-9]\d*$/.test(v)&&Number.isSafeInteger(Number(v));
const hex=v=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v);
const minimum=(...xs)=>xs.reduce((a,b)=>a<b?a:b);
export function validateCapacityReview(r){
 need(r?.schemaVersion===1&&r.kind==='conditional_no_pair_review'&&typeof r.id==='string'
  &&typeof r.tournamentId==='string'&&hex(r.evidenceHash)&&Array.isArray(r.legs)&&r.legs.length===2,'REVIEW');
 instant(r.recordedAt);instant(r.expiresAt);need(instant(r.expiresAt)>instant(r.recordedAt),'REVIEW_CLOCK');
 need(r.legs.every(l=>id(l.marketId)&&id(l.exchangeId)&&l.outcome==='NO'&&hex(l.rulesHash))
  &&new Set(r.legs.map(l=>l.exchangeId)).size===2&&new Set(r.legs.map(l=>l.marketId)).size===2,'REVIEW_SCOPE');
 need(Array.isArray(r.assumptions)&&r.assumptions.length&&r.assumptions.every(v=>typeof v==='string'&&v.length>0),'ASSUMPTIONS');
 need(Array.isArray(r.scenarios)&&r.scenarios.length>1&&r.scenarios.every(s=>typeof s.id==='string'
  &&['ordinary','exception'].includes(s.category)&&Array.isArray(s.payouts)&&s.payouts.length===2
  &&s.payouts.every(v=>v===0||v===1))&&new Set(r.scenarios.map(s=>s.id)).size===r.scenarios.length
  &&r.scenarios.some(s=>s.category==='ordinary')&&r.scenarios.some(s=>s.category==='exception'&&s.payouts.every(v=>v===0)),'SCENARIOS');
 return r;
}
function policyCheck(p){
 for(const k of ['maxAgeMs','maxSkewMs','maximumAccountAgeMs','maximumReviewAgeMs','adverseTicksPerLeg','depthFractionBps','maxQuantity'])need(Number.isSafeInteger(p[k])&&p[k]>=0,'POLICY');
 need(p.depthFractionBps>0&&p.depthFractionBps<=10000&&p.adverseTicksPerLeg<=5&&p.maxSkewMs<=p.maxAgeMs,'POLICY');
 for(const k of ['minimumMarginalEdge','minimumTotalProfit','feePerShare','fixedFeePerLeg','maxCost','maxUnmatchedCost'])need(amount(p[k])>=0n,'POLICY_COST');
 need(p.representation===CANONICAL_NO_POLICY.representation&&typeof p.feeEvidence==='string'&&p.feeEvidence.length>0,'REPRESENTATION');
}

// BUY NO consumes canonical YES bids. This is the documented same-exchange
// conversion, not an independently observed native NO book or a second source.
// Ordinary-election capacity is always paired with zero-payout exception stress.
export function evaluateCanonicalNoCapacity({review,books,account,now,policy=CANONICAL_NO_POLICY}){
 validateCapacityReview(review);policyCheck(policy);const at=instant(now);
 need(Array.isArray(books)&&books.length===2,'BOOKS');
 const base={schemaVersion:1,mode:'canonical_no_conditional_capacity',evaluatedAt:now,reviewId:review.id,
  reviewHash:digest(review),policyHash:digest(policy),representation:policy.representation,
  nativeNoObserved:false,scenarioCompletenessVerified:false,executionAuthority:false,ordersSubmitted:0,
  ruleBindingKind:'manual_review_attestation_not_live_api_metadata',
  networkRequests:0,fills:null,realizedPnl:null,profitabilityEstablished:false};
 const reject=reason=>({...base,reason,positiveConditionalCapacity:false,matchedQuantity:'0',acquisitionCost:'0.000000',segments:[]});
 if(at<instant(review.recordedAt)||at>=instant(review.expiresAt)||at-instant(review.recordedAt)>policy.maximumReviewAgeMs)return reject('review_not_current');
 const starts=[],receipts=[];
 const ladders=books.map((b,i)=>{
  const l=review.legs[i];need(b?.tournamentId===review.tournamentId&&b.marketId===l.marketId&&b.exchangeId===l.exchangeId
   &&b.instrument===l.exchangeId+':YES'&&b.freshnessPolicy==='authoritative_receipt_v1'
   &&hex(b.publicationHash)&&Number.isSafeInteger(b.publicationSequence)&&b.publicationSequence>0,'BOOK_SCOPE');
  const start=instant(b.requestStartedAt),receipt=instant(b.receivedAt);starts.push(start);receipts.push(receipt);
  need(start<=receipt&&receipt<=at&&instant(b.publishedAt)>=receipt&&instant(b.publishedAt)<=at,'BOOK_CLOCK');
  need(typeof b.sequence==='string'&&/^(0|[1-9]\d*)$/.test(b.sequence)&&b.sourceAt===timestamp(b.sourceAt)
   &&instantNs(b.sourceAt)<=BigInt(receipt+2000)*1000000n&&typeof b.versionKey==='string'&&b.versionKey===b.sequence+':'+b.sourceAt,'BOOK_VERSION');
  need(Array.isArray(b.bids)&&b.bids.length>0&&b.bids.length<=200&&Array.isArray(b.asks)&&b.asks.length>0,'DEPTH');
  need(price(b.bids[0].price)<price(b.asks[0].price),'CROSSED_BOOK');
  let previous=SCALE;
  return b.bids.map(v=>{const bid=price(v.price);need(bid<previous&&Number.isSafeInteger(v.quantity)&&v.quantity>0,'DEPTH');previous=bid;
   const raw=SCALE-bid,effective=raw+BigInt(policy.adverseTicksPerLeg)*TICK+amount(policy.feePerShare);
   need(raw+BigInt(policy.adverseTicksPerLeg)*TICK<=SCALE,'ADVERSE_PRICE');
   return{raw,effective,quantity:BigInt(v.quantity)*BigInt(policy.depthFractionBps)/10000n};
  }).filter(v=>v.quantity>0n);
 });
 if(starts.some(s=>s<instant(review.recordedAt)))return reject('review_not_known_at_request');
 if(starts.some(s=>at-s>policy.maxAgeMs))return reject('stale_request');
 if(Math.max(...receipts)-Math.min(...starts)>policy.maxSkewMs)return reject('whole_window_skew');
 if(!account||account.schemaVersion!==1||!Array.isArray(account.excludedExchangeIds)||account.excludedExchangeIds.some(x=>!id(x)))return reject('account_unavailable');
 const accountClocks=[instant(account.positionsAt),instant(account.ordersAt)];
 if(accountClocks.some(t=>t>Math.min(...starts)||at-t>policy.maximumAccountAgeMs))return reject('account_not_causal_or_fresh');
 if(review.legs.some(l=>account.excludedExchangeIds.includes(l.exchangeId)))return reject('own_account_exposure');
 if(ladders.some(l=>!l.length))return reject('fractional_depth');
 const ordinary=review.scenarios.filter(s=>s.category==='ordinary'),floor=BigInt(Math.min(...ordinary.map(s=>s.payouts[0]+s.payouts[1])))*SCALE;
 const costs=[amount(policy.fixedFeePerLeg),amount(policy.fixedFeePerLeg)],indexes=[0,0],left=ladders.map(l=>l[0].quantity);
 const budget=amount(policy.maxCost),unmatched=amount(policy.maxUnmatchedCost),edge=amount(policy.minimumMarginalEdge);
 let quantity=0n,stopReason='depth_exhausted';const segments=[];
 while(indexes.every((j,i)=>j<ladders[i].length)&&quantity<BigInt(policy.maxQuantity)){
  const levels=indexes.map((j,i)=>ladders[i][j]),unitCost=levels[0].effective+levels[1].effective;
  if(floor-unitCost<edge||floor<=unitCost){stopReason='marginal_edge';break;}
  if(costs[0]+costs[1]>budget||costs.some(c=>c>unmatched)){stopReason='capital_limit';break;}
  const take=minimum(...left,BigInt(policy.maxQuantity)-quantity,(budget-costs[0]-costs[1])/unitCost,
   ...costs.map((c,i)=>(unmatched-c)/levels[i].effective));
  if(take<=0n){stopReason='capital_limit';break;}
  levels.forEach((l,i)=>{costs[i]+=l.effective*take;left[i]-=take;});quantity+=take;
  segments.push({quantity:take.toString(),noPrices:levels.map(l=>money(l.raw)),adverseUnitCost:money(unitCost)});
  left.forEach((n,i)=>{if(n===0n){indexes[i]++;left[i]=ladders[i][indexes[i]]?.quantity??0n;}});
 }
 if(quantity===BigInt(policy.maxQuantity))stopReason='quantity_limit';
 const cost=quantity?costs[0]+costs[1]:0n,ordinaryProfit=quantity*floor-cost;
 return{...base,reason:quantity&&ordinaryProfit>=amount(policy.minimumTotalProfit)?'conditional_positive_capacity':stopReason,
  positiveConditionalCapacity:quantity>0n&&ordinaryProfit>0n&&ordinaryProfit>=amount(policy.minimumTotalProfit),stopReason,
  matchedQuantity:quantity.toString(),acquisitionCost:money(cost),conditionalOrdinaryMinimumProfit:money(ordinaryProfit),
  stressedMinimumProfit:money(BigInt(Math.min(...review.scenarios.map(s=>s.payouts[0]+s.payouts[1])))*SCALE*quantity-cost),
  maximumSingleLegCostAtRisk:money(quantity?minimum(-costs[0],-costs[1])*-1n:0n),segments,
  scenarios:review.scenarios.map(s=>({...s,profit:money(BigInt(s.payouts[0]+s.payouts[1])*SCALE*quantity-cost)})),
  sources:books.map(b=>({marketId:b.marketId,exchangeId:b.exchangeId,requestStartedAt:b.requestStartedAt,receivedAt:b.receivedAt,
   publicationSequence:b.publicationSequence,publicationHash:b.publicationHash,versionKey:b.versionKey})),
  limitations:['Canonical YES-bid complement, not independently captured NO depth.',
   'Current snapshot capacity is not atomic liquidity, a fill, a queue model or a profitable near-term exit.',
   'Ordinary election assumptions can fail through fusion, affiliation, rule changes or administrative settlement.',
   'Zero-payout exception stress may lose the full package cost; an unmatched-leg cap does not cover that loss.']};
}
