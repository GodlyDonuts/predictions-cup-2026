import {probability} from './poll-calibration-v1/model.mjs';

// A sensitivity diagnostic, not a calibrated multi-poll posterior. Correlated
// polling errors retain the original model variance instead of shrinking by n.
export function pollDisagreementMixtureV1({model,polls,asOf,halfLifeDays=14,marginShifts=[-3,0,3]}) {
  const now=Date.parse(asOf);
  if(!Number.isFinite(now)||!Array.isArray(polls)||!polls.length||!Number.isFinite(halfLifeDays)||halfLifeDays<=0||!Array.isArray(marginShifts)||!marginShifts.length||marginShifts.some(x=>!Number.isFinite(x)))throw Error('POLL_MIXTURE_INPUT');
  const seen=new Set();
  const rows=polls.map(p=>{
    const end=Date.parse(p.fieldEnd+'T23:59:59.999Z');
    if(typeof p.id!=='string'||seen.has(p.id)||!Number.isFinite(end)||end>now||!Number.isFinite(p.democraticMargin)||!['registered voters','likely voters'].includes(p.population))throw Error('POLL_MIXTURE_SOURCE');
    seen.add(p.id);
    const ageDays=(now-end)/86400000,weight=Math.pow(2,-ageDays/halfLifeDays);
    return {...p,ageDays,weight,democraticProbability:probability(model,p.democraticMargin)};
  });
  const total=rows.reduce((n,p)=>n+p.weight,0);
  if(!(total>0))throw Error('POLL_MIXTURE_WEIGHT');
  const scenarios=marginShifts.map(shift=>({marginShift:shift,democraticProbability:rows.reduce((n,p)=>n+p.weight*probability(model,p.democraticMargin+shift),0)/total}));
  const weightedDemocraticProbability=rows.reduce((n,p)=>n+p.weight*p.democraticProbability,0)/total;
  return {polls:rows.map(p=>({...p,normalizedWeight:p.weight/total})),weightedDemocraticProbability,weightedRepublicanProbability:1-weightedDemocraticProbability,scenarios,betweenPollDemocraticProbabilityRange:{lower:Math.min(...rows.map(p=>p.democraticProbability)),upper:Math.max(...rows.map(p=>p.democraticProbability))},differentPopulations: new Set(rows.map(p=>p.population)).size>1,varianceShrunk:false,recencyWeightsAreJudgment:true,calibratedPosterior:false,executionAuthority:false,actualOrders:0};
}
