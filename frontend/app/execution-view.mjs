import {createHash} from 'node:crypto';
const hash=x=>createHash('sha256').update(JSON.stringify(x)).digest('hex');
const amount=x=>x===null||typeof x==='string'&&/^-?\d+\.\d{6}$/.test(x);
export function executionPerformanceView(report,execution,now=Date.now()){
 const keys=['realizedPnl','ownedCost','observedLiquidationValue','observedLiquidationUnrealizedPnl','observedLiquidationTotalPnl','midpointValue','midpointUnrealizedPnl','midpointTotalPnl'];
 const age=now-Date.parse(report?.updatedAt),asOfAge=now-Date.parse(report?.asOf);
 if(report?.mode!=='official_execution_performance'||report.status!=='running'||report.runId!==execution?.runId||!Number.isFinite(age)||age<0||age>10000||!Number.isFinite(asOfAge)||asOfAge<0||asOfAge>10000||report.addedFees!=='0.000000'||!Array.isArray(report.positions)||keys.some(k=>!amount(report[k]))||report.executionLedgerHash!==hash({runId:execution.runId,positions:execution.positions,fills:execution.fills}))return {available:false};
 const quotes=report.positions.map(p=>Date.parse(p.book?.receivedAt)),quotesFresh=quotes.every(t=>Number.isFinite(t)&&now>=t&&now-t<=20000);
 const values=Object.fromEntries(keys.map(k=>[k,report[k]]));
 if(!quotesFresh)for(const k of keys.filter(k=>k.startsWith('observed')||k.startsWith('midpoint')))values[k]=null;
 return {available:true,asOf:report.asOf,updatedAt:report.updatedAt,...values,quoteAt:quotesFresh&&quotes.length?new Date(Math.min(...quotes)).toISOString():null,addedFees:'0.000000'};
}
