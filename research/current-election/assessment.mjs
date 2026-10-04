import assert from 'node:assert/strict';
import {probabilityFromMargin} from '../election-baseline-v2/model.mjs';
import {decimalMicros,formatMicros} from '../../execution/core.mjs';
const day=86400000;
const finite=x=>typeof x==='number'&&Number.isFinite(x);
const clock=x=>typeof x==='string'&&new Date(Date.parse(x)).toISOString()===x;
const share=x=>finite(x)&&x>=0&&x<=1;
const signedMicros=x=>x<0n?'-'+formatMicros(-x):formatMicros(x);

// A current assessment clock never substitutes for the underlying field dates.
export function assessElection({poll,prior,definition,assessmentAt,electionAt}){
  assert(clock(assessmentAt)&&clock(electionAt)&&clock(poll.observedAt)&&clock(poll.fieldEndLatestAt),'invalid_clock');
  const now=Date.parse(assessmentAt),end=Date.parse(poll.fieldEndLatestAt),election=Date.parse(electionAt);
  assert(end<=Date.parse(poll.observedAt)&&Date.parse(poll.observedAt)<=now&&now<election,'noncausal_clock');
  assert(now-end<=definition.maximumPollFieldAgeDays*day,'poll_field_too_old');
  assert(poll.population==='likely_voters'&&Number.isSafeInteger(poll.sampleSize)&&poll.sampleSize>=30,'wrong_population_or_sample');
  const shares=[poll.republican,poll.democratic,poll.independent,poll.undecided];
  assert(shares.every(share)&&Math.abs(shares.reduce((a,b)=>a+b,0)-1)<=0.02,'invalid_shares');
  assert(poll.ballotVerified===true&&poll.largestNamedOpponent==='democratic'&&poll.republican>poll.independent+poll.undecided,'unverified_plurality_scope');
  assert(finite(prior.structuralMargin)&&finite(prior.residualRms)&&prior.residualRms>0&&prior.trainedThroughYear<Number(electionAt.slice(0,4)),'invalid_prior');
  const p=definition;
  assert(p.schemaVersion===1&&p.priorWeight>0&&p.priorWeight<1&&p.nationalTransportSd>0&&p.maximumAssessmentAgeMs>0&&p.maximumAssessmentAgeMs<=21600000,'invalid_definition');
  assert(p.nationalMarginScenarios.every(finite)&&p.designEffects.every(x=>finite(x)&&x>=1)&&p.undecidedRepublicanFractions.every(share)&&p.pollMarginBiasScenarios.every(finite),'invalid_scenarios');
  assert(p.systematicMarginSd>0&&p.dailyMarginInnovationSd>0&&share(p.administrativeProbabilityHaircut)&&share(p.reportedShareRoundingHalfWidth),'invalid_error_assumptions');
  // For Z=1(R), -1(D), 0(other), Var(mean Z)=(R+D-(R-D)^2)/n.
  // Delta-method variance for (R-D)/(R+D), retaining negative multinomial covariance.
  const r=poll.republican,d=poll.democratic,s=r+d;
  const gradR=2*d/(s*s),gradD=-2*r/(s*s);
  const samplingVariance=(gradR*gradR*r*(1-r)+gradD*gradD*d*(1-d)-2*gradR*gradD*r*d)/poll.sampleSize;
  const priorSd=Math.hypot(prior.residualRms,p.nationalTransportSd),daysToElection=(election-end)/day;
  function family(systematicSd,dailySd){
    assert(finite(systematicSd)&&systematicSd>0&&finite(dailySd)&&dailySd>0,'invalid_stress');
    const variants=[];
    for(const nationalMargin of p.nationalMarginScenarios)for(const designEffect of p.designEffects)for(const undecidedR of p.undecidedRepublicanFractions)for(const bias of p.pollMarginBiasScenarios){
      // Rounded shares may be adverse by half a displayed percentage point each.
      const finalR=r-p.reportedShareRoundingHalfWidth+poll.undecided*undecidedR;
      const finalD=d+p.reportedShareRoundingHalfWidth+poll.undecided*(1-undecidedR);
      const opposing=Math.max(finalD,poll.independent+poll.undecided*(1-undecidedR));
      const pollMargin=(finalR-opposing)/(finalR+opposing)+bias;
      const mean=p.priorWeight*(prior.structuralMargin+nationalMargin)+(1-p.priorWeight)*pollMargin;
      const pollSd=Math.hypot(Math.sqrt(samplingVariance*designEffect),systematicSd);
      const correlatedPoolSd=p.priorWeight*priorSd+(1-p.priorWeight)*pollSd;
      const standardDeviation=Math.hypot(correlatedPoolSd,dailySd*Math.sqrt(daysToElection));
      const ordinaryProbability=probabilityFromMargin(mean,standardDeviation);
      variants.push({nationalMargin,designEffect,undecidedR,bias,mean,standardDeviation,ordinaryProbability,contractProbability:Math.max(0,ordinaryProbability-p.administrativeProbabilityHaircut)});
    }
    assert(variants.length>0,'empty_scenarios');
    return {lower:Math.min(...variants.map(v=>v.contractProbability)),upper:Math.max(...variants.map(v=>v.contractProbability)),variants};
  }
  const ordinary=family(p.systematicMarginSd,p.dailyMarginInnovationSd),stress=family(p.stress.systematicMarginSd,p.stress.dailyMarginInnovationSd);
  return {schemaVersion:1,modelId:p.id,method:'subjective_range',assessedAt:assessmentAt,expiresAt:new Date(now+p.maximumAssessmentAgeMs).toISOString(),
    componentClocks:{pollObservedAt:poll.observedAt,pollFieldEndLatestAt:poll.fieldEndLatestAt,pollFieldAgeDays:(now-end)/day,daysFromFieldEndToElection:daysToElection,electionAt,priorTrainedThroughYear:prior.trainedThroughYear},
    sampleSize:poll.sampleSize,samplingVariance,priorSd,ordinary,stress,profitabilityEstablished:false,limitations:p.limitations};
}

// Diagnostics use exact ticks and the same economic hurdle as production.
// They convey no account permission and do not bypass native admission.
export function priceDecision({assessment,book,asOf,policy}){
  const now=Date.parse(asOf),at=Date.parse(assessment.assessedAt),received=Date.parse(book.receivedAt),started=Date.parse(book.requestStartedAt);
  if(![now,at,received,started].every(Number.isFinite)||now<received||received<started||started<at||now-received>policy.maximumBookAgeMs||received-started>policy.maximumBookAgeMs)return {eligible:false,reason:'book_not_causal_or_fresh'};
  if(now>=Date.parse(assessment.expiresAt))return {eligible:false,reason:'assessment_expired'};
  if(!book.asks?.length||!book.bids?.length)return {eligible:false,reason:'missing_depth'};
  const ask=decimalMicros(book.asks[0].price),bid=decimalMicros(book.bids[0].price),slip=BigInt(policy.slippageTicks)*5000n;
  assert(ask>bid&&ask<1000000n&&bid>0n,'invalid_book');
  const lower=BigInt(Math.floor(assessment.ordinary.lower*1000000)),stressLower=BigInt(Math.floor(assessment.stress.lower*1000000));
  const maximumAsk=lower-decimalMicros(policy.minimumEdge)-slip;
  const stressedEdge=stressLower-ask-slip,edge=lower-ask-slip;
  const available=Math.floor(book.asks[0].quantity*policy.depthFractionBps/10000);
  const quantity=Math.min(available,Number(decimalMicros(policy.maxOrderCost)/(ask+slip)));
  const spread=ask-bid;
  const reason=spread>decimalMicros(policy.maximumSpread)?'spread':edge<decimalMicros(policy.minimumEdge)?'insufficient_edge':quantity<1?'insufficient_depth_or_order_budget':'conditional_candidate';
  return {eligible:reason==='conditional_candidate',reason,lower:formatMicros(lower),stressLower:formatMicros(stressLower),ask:formatMicros(ask),bid:formatMicros(bid),slippage:formatMicros(slip),edge:signedMicros(edge),stressedEdge:signedMicros(stressedEdge),maximumAsk:signedMicros(maximumAsk),quantity,maximumPrincipal:formatMicros((ask+slip)*BigInt(quantity)),fees:'0.000000',executionAuthority:false,profitabilityEstablished:false};
}
