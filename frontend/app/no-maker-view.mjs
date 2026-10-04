import {createHash} from 'node:crypto';
import {comparisonNameValid,ageComparisonWallet} from './comparison-view.mjs';
const hash=value=>createHash('sha256').update(JSON.stringify(sort(value))).digest('hex');
function sort(value){if(Array.isArray(value))return value.map(sort);if(value&&typeof value==='object')return Object.fromEntries(Object.keys(value).sort().map(k=>[k,sort(value[k])]));return value;}
export function noMakerWalletViews(report,manifest,bank,{collectorAlive=false,now=Date.now()}={}){
 const bankHash=hash(bank);
 if(!comparisonNameValid(manifest?.cohort)||bankHash!==manifest.bankHash||bank.name!==manifest.cohort||bank.schemaVersion!==1
  ||bank.mode!=='no_maker_trade_tape_paper'||report?.mode!==bank.mode||report.bankHash!==bankHash||report.cohort!==bank.name||report.cutoff!==bank.cutoff
  ||report.networkRequests!==0||report.competitionOrders!==0||report.executionAuthority!==false||report.profitabilityEstablished!==false
  ||typeof report.failed!=='boolean'||bank.policy?.feePerShare!=='0.000000'||bank.policy.families?.join(',')!=='at_quote,improve_one_tick')throw Error('Maker paper identity unavailable');
 const labels=new Map(bank.legs.map(l=>[l.exchangeId,l.marketId]));
 const instrument=x=>{if(labels.get(x.exchangeId)!==x.marketId||!Number.isSafeInteger(x.quantity)||x.quantity<1)throw Error('Maker paper scope unavailable');return {instrument:x.exchangeId+':NO',label:`Market ${x.marketId} · NO`,side:'no'};};
 return bank.policy.families.map(family=>{
  const w=report.wallets?.[family];if(!w||w.fees!=='0.000000'||!Array.isArray(w.positions)||!Array.isArray(w.unresolvedQuotes)||!Array.isArray(w.recentFills))throw Error('Maker paper accounting unavailable');
  const status=report.failed?'failed':collectorAlive?'running':'stopped';
  const view={id:'no-maker-'+family.replaceAll('_','-'),name:family==='at_quote'?'NO maker · join quote':'NO maker · improve tick',available:true,
   updatedAt:report.updatedAt,status,feeBps:0,markets:bank.legs.length,observations:report.journalSequence,
   metrics:{cash:w.cash,realizedPnl:w.realizedPnl,liquidationPnl:w.estimatedLiquidationPnl,fills:w.fills,fees:w.fees},
   positions:w.positions.map(x=>{const valid=Number.isFinite(x.quoteAgeMs)&&x.quoteAgeMs>=0&&x.quoteAgeMs<=20000&&x.estimatedLiquidationProceeds!==null&&Number(x.estimatedLiquidationProceeds)>0;
    return {...instrument(x),quantity:x.quantity,cost:x.cost,enteredAt:x.enteredAt,observationAgeMs:x.quoteAgeMs,
     quoteReceivedAt:Number.isFinite(x.quoteAgeMs)?new Date(Date.parse(report.updatedAt)-x.quoteAgeMs).toISOString():null,quoteAgeMs:x.quoteAgeMs,
     quoteInvalidated:!valid,unsoldShares:valid?0:x.quantity,estimatedProceeds:valid?x.estimatedLiquidationProceeds:null,liquidationValue:valid?x.estimatedLiquidationProceeds:null};}),
   trades:w.recentFills.map(x=>({...instrument(x),action:x.action,quantity:x.quantity,price:x.price,at:x.at,fee:'0.000000',hypothetical:true})),
   pending:w.unresolvedQuotes.map(x=>({...instrument(x),action:x.action,quantity:x.quantity,limit:x.limit,createdAt:x.createdAt,expiresAt:x.expiresAt,hypothetical:true})),
   performance:{closedRoundTrips:w.closedTrades},comparison:{cohort:bank.name,cutoff:bank.cutoff,maxMarkAgeMs:20000},
   noMaker:{bankHash,family,reserved:w.reserved,fillModel:bank.policy.fillModel,counts:report.counts,accountingPredecessor:bank.accountingContinuation?.cohort||null,profitabilityEstablished:false}};
  if(view.positions.some(p=>p.quoteInvalidated))view.metrics.liquidationPnl=null;
  return ageComparisonWallet(view,now);
 });
}
