import {amount,money,SCALE,TICK,instant} from '../paper/fixed.mjs';
import {instantNs} from '../feed/contract.mjs';
import {normalizeOfficialFillTick} from '../execution/api-tick-price.mjs';
import {probabilityAccountGate} from './probability-receipt.mjs';
import {payloadHash} from '../execution/ledger.mjs';
const check=(ok,code)=>{if(!ok)throw Error('CAPACITY_PATH_'+code);};
export const CAPACITY_PATH_POLICY=Object.freeze({schemaVersion:1,maximumFrameAgeMs:20000,maximumWindowMs:10000,
 depthFractionBps:100,exitAdverseTicks:1,latencyMs:3000,restLifetimeMs:120000,strictThroughTicks:1,
 makerQuantity:1,diagnosticLookaheadMs:3600000,feePerShare:'0.000000'});
export function tapeTick(trade){
 if(trade.yesPrice===null)return null;
 check(typeof trade.yesPrice==='string'&&String(Number(trade.yesPrice))===trade.yesPrice,'RAW_TRADE_PRICE');
 // The tape preserves the original JSON-number representation. Recognize only
 // documented tick/complement encodings, never round arbitrary off-tick trades.
 try{return normalizeOfficialFillTick(Number(trade.yesPrice));}catch{return null;}
}
function usable(f,p){
 const now=instant(f.at),starts=f.books.map(b=>instant(b.requestStartedAt)),receipts=f.books.map(b=>instant(b.receivedAt));
 return f.books.every(b=>now>=instant(b.publishedAt)&&now-instant(b.requestStartedAt)<=p.maximumFrameAgeMs
  &&probabilityAccountGate(f.accountEligibility,b,f.at)===null)
  &&Math.max(...receipts)-Math.min(...starts)<=p.maximumWindowMs;
}
function noExit(book,quantity,p){
 let remaining=BigInt(quantity),proceeds=0n,shares=0n;
 for(const l of book.asks){const n=BigInt(l.quantity)*BigInt(p.depthFractionBps)/10000n,take=n<remaining?n:remaining;
  const px=SCALE-amount(l.price)-BigInt(p.exitAdverseTicks)*TICK-amount(p.feePerShare);if(px<=0n)break;
  proceeds+=take*px;remaining-=take;shares+=take;if(remaining===0n)break;
 }
 return {quantity:shares.toString(),proceeds:money(proceeds),complete:remaining===0n};
}
export function studyCapacityPaths({frames,trades,policy=CAPACITY_PATH_POLICY}){
 check(payloadHash(policy)===payloadHash(CAPACITY_PATH_POLICY),'POLICY');
 let previous=-Infinity;for(const f of frames){check(instant(f.at)>=previous&&f.books.length===2,'FRAME_ORDER');previous=instant(f.at);
  check(f.books.every((b,i)=>b.versionKey===f.versions[i]&&b.publicationSequence<=f.sourceSequence),'FRAME_BINDING');}
 const usableFrames=frames.filter(f=>usable(f,policy)),anchors=usableFrames.filter(f=>f.result.positiveConditionalCapacity),cases=[];
 for(const f of anchors){
  const known=instant(f.at),quantity=f.result.matchedQuantity,cost=amount(f.result.acquisitionCost);
  check(/^[1-9]\d*$/.test(quantity),'QUANTITY');
  const later=usableFrames.filter(x=>instant(x.at)>known&&instant(x.at)-known<=policy.diagnosticLookaheadMs
   &&x.books.every((b,i)=>b.marketId===f.books[i].marketId&&b.exchangeId===f.books[i].exchangeId
    &&b.versionKey!==f.versions[i]&&instant(b.requestStartedAt)>=known+policy.latencyMs));
  const marks=later.map(x=>{const legs=x.books.map(b=>noExit(b,quantity,policy));return {at:x.at,ageMs:instant(x.at)-known,
   complete:legs.every(l=>l.complete),legs,netProceeds:money(legs.reduce((a,b)=>a+amount(b.proceeds),0n)),
   hypotheticalPnl:legs.every(l=>l.complete)?money(legs.reduce((a,b)=>a+amount(b.proceeds),0n)-cost):null,
   versions:x.versions,sourceSequence:x.sourceSequence,sourceHash:x.sourceHash};});
  const complete=marks.filter(m=>m.complete),best=complete.reduce((a,b)=>!a||amount(b.hypotheticalPnl)>amount(a.hypotheticalPnl)?b:a,null);
  const legs=f.books.map(b=>{
   const limit=amount(b.asks[0].price),activationNs=[BigInt(known+policy.latencyMs)*1000000n,instantNs(b.sourceAt)+2000000000n].reduce((a,b)=>a>b?a:b),expiryNs=BigInt(known+policy.restLifetimeMs)*1000000n;
   const eligible=trades.filter(t=>t.tournamentId===b.tournamentId&&t.marketId===b.marketId&&t.exchangeId===b.exchangeId
    &&instant(t.firstAvailableAt)>=known&&instantNs(t.createdAt)>=activationNs&&instantNs(t.createdAt)<expiryNs);
   const through=eligible.filter(t=>{const p=tapeTick(t);return p!==null&&amount(p.price)>=limit+BigInt(policy.strictThroughTicks)*TICK;});
   through.sort((a,b)=>instantNs(a.createdAt)<instantNs(b.createdAt)?-1:instantNs(a.createdAt)>instantNs(b.createdAt)?1:0);
   return {marketId:b.marketId,exchangeId:b.exchangeId,yesSellLimit:money(limit),canonicalNoAcquisitionPrice:money(SCALE-limit),
    activationAt:new Date(Number(activationNs/1000000n)).toISOString(),expiresAt:new Date(known+policy.restLifetimeMs).toISOString(),
    eligibleObservedTrades:eligible.length,strictThroughEvents:through.length,strictThroughShares:through.reduce((n,t)=>n+t.size,0),
    firstThrough:through[0]?{id:through[0].id,key:through[0].key,createdAt:through[0].createdAt,firstAvailableAt:through[0].firstAvailableAt,
     rawYesPrice:through[0].yesPrice,normalizedPrice:tapeTick(through[0]).price}:null,
    aggressorSide:null,queuePositionProven:false,fillProven:false};
  });
  cases.push({anchorAt:f.at,sourceSequence:f.sourceSequence,sourceHash:f.sourceHash,versions:f.versions,quantity,acquisitionCost:money(cost),
   ordinaryConditionalSettlementSurplus:f.result.conditionalOrdinaryMinimumProfit,zeroPayoutStress:f.result.stressedMinimumProfit,
   subsequentCompleteExitObservations:complete.length,partialExitObservations:marks.length-complete.length,
   bestLaterDepthMark:best,lastLaterDepthMark:complete.at(-1)||null,rightCensored:later.length===0,
   maker:{quantity:policy.makerQuantity,canonicalNoQuotedPackageCost:money(legs.reduce((x,l)=>x+amount(l.canonicalNoAcquisitionPrice),0n)*BigInt(policy.makerQuantity)),
    bothLegsHaveStrictThrough:legs.every(l=>l.strictThroughEvents>0),exactlyOneLegHasStrictThrough:legs.filter(l=>l.strictThroughEvents>0).length===1,legs}});
 }
 return {schemaVersion:1,mode:'conditional_capacity_entry_exit_development_diagnostic',policy,policyHash:payloadHash(policy),frames:frames.length,
  usableFrames:usableFrames.length,positiveAnchors:cases.length,anchorsWithLaterCompleteDepth:cases.filter(c=>c.subsequentCompleteExitObservations>0).length,
  anchorsWithAnyPositiveLaterDepthMark:cases.filter(c=>c.bestLaterDepthMark&&amount(c.bestLaterDepthMark.hypotheticalPnl)>0n).length,
  makerBothLegsThrough:cases.filter(c=>c.maker.bothLegsHaveStrictThrough).length,makerOneLegThrough:cases.filter(c=>c.maker.exactlyOneLegHasStrictThrough).length,
  cases,simulatedFills:null,realizedPnl:null,executionAuthority:false,profitabilityEstablished:false,
  limitations:['Overlapping historical anchors are dependent; best-later marks use hindsight and are not a strategy return.',
   'Later co-observed depth is not proof of entry, exit, simultaneous matching or atomic execution.',
   'Strict trade-through prints are potential counterparties, not queue priority, aggressor identification or maker fills.',
   'Missing reporting windows, null/unrecognized legacy prices and unobserved paths cannot prove no trades or no future profit.',
   'Ordinary settlement scenarios can fail; zero-payout stress and partial-leg risk remain.']};
}
