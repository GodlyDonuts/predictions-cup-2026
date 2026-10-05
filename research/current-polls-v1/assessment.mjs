import assert from 'node:assert/strict';
import {probabilityFromMargin} from '../election-baseline-v2/model.mjs';
const day=86400000;
// Separate party valuations. A discounted Republican range is not simply
// complemented into a Democratic forecast, especially with a third candidate.
export function assessPartyPoll({poll,prior,definition,party,assessmentAt,electionAt}){
 assert(['republican','democratic'].includes(party),'unknown_party');
 const now=Date.parse(assessmentAt),end=Date.parse(poll.fieldEndLatestAt),observed=Date.parse(poll.observedAt),election=Date.parse(electionAt);
 assert([now,end,observed,election].every(Number.isFinite)&&end<=observed&&observed<=now&&now<election,'noncausal_clock');
 assert(now-end<=definition.maximumPollFieldAgeDays*day,'poll_field_too_old');
 assert(poll.population==='likely_voters'&&Number.isSafeInteger(poll.sampleSize)&&poll.sampleSize>=30,'population_or_sample_unverified');
 const r=poll.republican,d=poll.democratic,o=poll.other,remaining=poll.unallocated;
 assert([r,d,o,remaining].every(x=>Number.isFinite(x)&&x>=0&&x<=1)&&Math.abs(r+d+o+remaining-1)<1e-9,'invalid_shares');
 assert(poll.ballotVerified===true&&o+remaining+definition.reportedShareRoundingHalfWidth<Math.min(r,d)-definition.reportedShareRoundingHalfWidth,'third_party_comparator_unverified');
 assert(Number.isSafeInteger(prior.trainedThroughYear)&&prior.trainedThroughYear<Number(electionAt.slice(0,4))&&Number.isFinite(prior.structuralMargin)&&Number.isFinite(prior.residualRms)&&prior.residualRms>0,'invalid_prior');
 assert(definition.priorWeight>0&&definition.priorWeight<1&&Number.isFinite(definition.nationalTransportSd)&&definition.nationalTransportSd>0&&definition.maximumAssessmentAgeMs>0&&definition.maximumAssessmentAgeMs<=21600000,'invalid_definition');
 assert([definition.nationalMarginScenarios,definition.designEffects,definition.undecidedRepublicanFractions,definition.pollMarginBiasScenarios].every(a=>Array.isArray(a)&&a.length>0&&a.every(Number.isFinite))&&definition.designEffects.every(x=>x>=1)&&definition.undecidedRepublicanFractions.every(x=>x>=0&&x<=1),'invalid_scenarios');
 assert([definition.systematicMarginSd,definition.dailyMarginInnovationSd,definition.stress?.systematicMarginSd,definition.stress?.dailyMarginInnovationSd].every(x=>Number.isFinite(x)&&x>0)&&definition.reportedShareRoundingHalfWidth>=0&&definition.reportedShareRoundingHalfWidth<.05&&definition.administrativeProbabilityHaircut>=0&&definition.administrativeProbabilityHaircut<1,'invalid_error_assumptions');
 const sign=party==='republican'?1:-1,target=party==='republican'?r:d,opponent=party==='republican'?d:r;
 const sum=target+opponent,gr=2*opponent/(sum*sum),gd=-2*target/(sum*sum);
 const samplingVariance=(gr*gr*target*(1-target)+gd*gd*opponent*(1-opponent)-2*gr*gd*target*opponent)/poll.sampleSize;
 const priorSd=Math.hypot(prior.residualRms,definition.nationalTransportSd),days=(election-end)/day;
 function family(systematic,daily){
  const variants=[];
  for(const national of definition.nationalMarginScenarios)for(const deff of definition.designEffects)for(const allocation of definition.undecidedRepublicanFractions)for(const bias of definition.pollMarginBiasScenarios)for(const rounding of [-1,1]){
   const targetAllocation=party==='republican'?allocation:1-allocation;
   const t=target+rounding*definition.reportedShareRoundingHalfWidth+remaining*targetAllocation;
   const q=opponent-rounding*definition.reportedShareRoundingHalfWidth+remaining*(1-targetAllocation);
   const strongest=Math.max(q,o+remaining*(1-targetAllocation));
   const pollMargin=(t-strongest)/(t+strongest)+sign*bias;
   const mean=definition.priorWeight*sign*(prior.structuralMargin+national)+(1-definition.priorWeight)*pollMargin;
   const pollSd=Math.hypot(Math.sqrt(samplingVariance*deff),systematic);
   const sd=Math.hypot(definition.priorWeight*priorSd+(1-definition.priorWeight)*pollSd,daily*Math.sqrt(days));
   const raw=probabilityFromMargin(mean,sd),valuation=Math.max(0,raw-definition.administrativeProbabilityHaircut);
   variants.push({national,designEffect:deff,undecidedRepublicanFraction:allocation,bias,rounding,mean,standardDeviation:sd,modelWinProbability:raw,discountedValuation:valuation});
  }
  return {lower:Math.min(...variants.map(v=>v.discountedValuation)),upper:Math.max(...variants.map(v=>v.discountedValuation)),modelWinRange:[Math.min(...variants.map(v=>v.modelWinProbability)),Math.max(...variants.map(v=>v.modelWinProbability))],variants};
 }
 return {schemaVersion:1,party,method:'subjective_range',assessedAt:assessmentAt,expiresAt:new Date(now+definition.maximumAssessmentAgeMs).toISOString(),componentClocks:{pollObservedAt:poll.observedAt,pollFieldEndLatestAt:poll.fieldEndLatestAt,pollFieldAgeDays:(now-end)/day,daysFromFieldEndToElection:days,electionAt,priorTrainedThroughYear:prior.trainedThroughYear},sampleSize:poll.sampleSize,samplingVariance,ordinary:family(definition.systematicMarginSd,definition.dailyMarginInnovationSd),stress:family(definition.stress.systematicMarginSd,definition.stress.dailyMarginInnovationSd),fees:'0.000000',executionAuthority:false,profitabilityEstablished:false};
}
