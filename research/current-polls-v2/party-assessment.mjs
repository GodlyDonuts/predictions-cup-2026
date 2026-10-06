import assert from 'node:assert/strict';
import {probability} from '../poll-calibration-v1/model.mjs';

// The historical model is conditional on a comparable Democratic/Republican race.
// These sensitivity scenarios are judgments, not calibrated confidence intervals.
export function assessPartyPolls({party,polls,models,at,haircut=0.02,halfLifeDays=21,transportShifts=[-3,0,3],roundingShifts=[-1,0,1],houseShifts=[0,3]}){
 assert(['DEM','REP'].includes(party)&&polls.length&&models.length&&Number.isFinite(Date.parse(at)));
 const seen=new Set();for(const p of polls){assert(!seen.has(p.pollster));seen.add(p.pollster);assert([p.democratic,p.republican,p.undecided].every(x=>Number.isFinite(x)&&x>=0&&x<=100));assert(p.democratic+p.republican+p.undecided<=100.00001);assert(Date.parse(p.fieldEndAt)<=Date.parse(at));}
 const raw=polls.map(p=>2**(-(Date.parse(at)-Date.parse(p.fieldEndAt))/86400000/halfLifeDays)),total=raw.reduce((a,b)=>a+b,0),weights=raw.map(x=>x/total);
 const margin=polls.reduce((s,p,i)=>s+(p.democratic-p.republican)*weights[i],0),undecided=polls.reduce((s,p,i)=>s+p.undecided*weights[i],0),houseWeight=polls.reduce((s,p,i)=>s+(p.possibleRepublicanHouseBias?weights[i]:0),0);
 const value=(model,m)=>Math.max(0,(party==='DEM'?probability(model,m):1-probability(model,m))-haircut);
 const scenarios=[];for(const model of models)for(const transport of transportShifts)for(const rounding of roundingShifts)for(const house of houseShifts){const adjusted=margin+transport+rounding+house*houseWeight;scenarios.push({omittedCycle:model.omittedCycle??null,transportShift:transport,roundingShift:rounding,possibleRepublicanHouseShift:house,margin:adjusted,value:value(model,adjusted)});}
 return {party,at,marginPoints:margin,pollWeights:polls.map((p,i)=>({pollster:p.pollster,weight:weights[i],fieldEndAt:p.fieldEndAt})),centralEstimate:value(models[0],margin),ordinaryRange:[Math.min(...scenarios.map(s=>s.value)),Math.max(...scenarios.map(s=>s.value))],severeOpponentUndecidedAllocation:value(models[0],margin+(party==='REP'?undecided+3+1+3*houseWeight:-undecided-3-1)),scenarios,distribution:'fixed_t4',estimateMeaning:'Conditional settlement-value research, not a forecast of near-term bid returns or a confidence interval',executionAuthority:false,profitabilityEstablished:false};
}
