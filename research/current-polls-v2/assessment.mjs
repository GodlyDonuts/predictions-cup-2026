import assert from 'node:assert/strict';import {probability} from '../poll-calibration-v1/model.mjs';
export function assessPrimaryPolls({polls,models,definition,at}){
 assert(polls.length&&models.length&&Number.isFinite(Date.parse(at)));
 const seen=new Set();for(const p of polls){assert(!seen.has(p.pollster));seen.add(p.pollster);assert(['registered_voters','likely_voters'].includes(p.population)&&[p.democratic,p.republican,p.unallocated].every(v=>Number.isFinite(v)&&v>=0&&v<=100)&&p.democratic+p.republican<=100&&Date.parse(p.fieldEndAt)<=Date.parse(at));}
 const weights=polls.map(p=>2**(-((Date.parse(at)-Date.parse(p.fieldEndAt))/86400000)/definition.recencyHalfLifeDays)),sum=weights.reduce((a,b)=>a+b,0),w=weights.map(v=>v/sum);
 const margin=polls.reduce((v,p,i)=>v+(p.democratic-p.republican)*w[i],0),rvWeight=polls.reduce((v,p,i)=>v+(p.population==='registered_voters'?w[i]:0),0),unallocated=polls.reduce((v,p,i)=>v+p.unallocated*w[i],0);
 const scenarios=[];for(const model of models)for(const rvShift of definition.rvTurnoutMarginShiftPoints)for(const roundingShift of definition.roundingMarginShiftPoints){const m=margin+rvWeight*rvShift+roundingShift;scenarios.push({omittedCycle:model.omittedCycle??null,rvShift,roundingShift,marginPoints:m,valuation:Math.max(0,probability(model,m)-definition.administrativeValuationHaircut)});}
 const estimates=scenarios.map(s=>s.valuation),lower=Math.min(...estimates),upper=Math.max(...estimates);
 return{marginPoints:margin,pollWeights:polls.map((p,i)=>({pollster:p.pollster,population:p.population,fieldEndAt:p.fieldEndAt,weight:w[i]})),rvWeight,unallocatedPoints:unallocated,ordinaryRange:[lower,upper],scenarios,
  centralEstimate:Math.max(0,probability(models[0],margin)-definition.administrativeValuationHaircut),severeOpponentAllocationStress:Math.max(0,probability(models[0],margin-unallocated-3*rvWeight-1)-definition.administrativeValuationHaircut),
  estimateMeaning:'Settlement-value research under historical error and declared sensitivity; not calibrated population transport or a short-term return forecast.',executionAuthority:false,profitabilityEstablished:false};
}
