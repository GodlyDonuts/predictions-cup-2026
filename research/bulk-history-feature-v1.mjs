import {price,money} from '../paper/fixed.mjs';
export function bulkHistoryFeatureV1(history,requestAt,policy){
 const prior=history.filter(p=>p.received<=requestAt&&p.received>=requestAt-policy.lookbackMs);
 if(prior.length<policy.minimumPriorBooks||requestAt-prior.at(-1).received>policy.maximumBulkAgeMs||requestAt-prior[0].received<policy.minimumHistorySpanMs)return null;
 for(let i=1;i<prior.length;i++)if(prior[i].received-prior[i-1].received>policy.maximumBulkGapMs)return null;
 const values=prior.map(p=>(price(p.bid)+price(p.ask))/2n).sort((a,b)=>a<b?-1:a>b?1:0),i=Math.floor(values.length/2),mid=values.length%2?values[i]:(values[i-1]+values[i])/2n;
 return{medianYesMid:money(mid),lastBulkAt:new Date(prior.at(-1).received).toISOString(),lastBulkHash:prior.at(-1).hash,firstBulkAt:new Date(prior[0].received).toISOString(),observations:prior.length};
}
