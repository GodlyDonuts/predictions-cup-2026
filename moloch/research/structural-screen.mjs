import fs from 'node:fs';import {createHash} from 'node:crypto';
import {ActivityStudy} from '../activity.mjs';import {selectPairs} from '../screen.mjs';
const [catalogPath,input,output]=process.argv.slice(2);if(!output)throw Error('catalog, frozen stream and new output required');
const catalogRaw=fs.readFileSync(catalogPath,'utf8'),catalog=catalogRaw.trim().split(/\r?\n/).map(JSON.parse);selectPairs(catalog,8,{all:true});
const groups=new Map();for(const m of catalog.flatMap(r=>r.data.data)){
 const match=/^Will the (Democratic|Republican) Party win the (.+)\?$/.exec(m.title);if(!match||m.exchanges.length!==1)continue;
 const g=groups.get(match[2])??{};if(g[match[1]])throw Error('AMBIGUOUS_RACE');g[match[1]]={marketId:m.id,exchangeId:m.exchanges[0].id};groups.set(match[2],g);
}
const pairs=[...groups].filter(([,g])=>g.Democratic&&g.Republican),study=new ActivityStudy(),opportunities=new Map();
const raw=fs.readFileSync(input,'utf8');if(!raw.endsWith('\n'))throw Error('INCOMPLETE_PREFIX');
for(const line of raw.split(/\r?\n/).filter(Boolean)){
 const row=JSON.parse(line);study.row(row);const at=Date.parse(row.event.receivedAt);if(!Number.isFinite(at))continue;
 for(const [race,g]of pairs){
  const a=study.markets.get(g.Democratic.marketId),b=study.markets.get(g.Republican.marketId),da=a?.books.get(g.Democratic.exchangeId),rb=b?.books.get(g.Republican.exchangeId);
  if(!da||!rb||a.gapped||b.gapped)continue;
  const clocks=[da,rb].map(x=>Date.parse(x.book.asOf.at));if(clocks.some(t=>at-t<0||at-t>10000)||Math.abs(clocks[0]-clocks[1])>1000)continue;
  const bids=[da,rb].map(x=>x.book.bids[0]),asks=[da,rb].map(x=>x.book.asks[0]),ticks=bids.map(x=>Math.round(x.price*200));
  const liquidationTicks=400-asks.reduce((n,x)=>n+Math.round(x.price*200),0);
  const packageCostTicks=400-ticks[0]-ticks[1];if(packageCostTicks>=200)continue;
  const key=[race,da.book.asOf.sequence,rb.book.asOf.sequence].join(':');if(opportunities.has(key))continue;
  opportunities.set(key,{race,receivedAt:row.event.receivedAt,legs:[g.Democratic,g.Republican],sourceTimes:[da.book.asOf.at,rb.book.asOf.at],sequences:[da.book.asOf.sequence,rb.book.asOf.sequence],noPurchaseLimits:bids.map(x=>(1-x.price).toFixed(6)),packageCost:(packageCostTicks/200).toFixed(6),immediateOnePackageLiquidation:(liquidationTicks/200).toFixed(6),immediateRoundTripPnl:((liquidationTicks-packageCostTicks)/200).toFixed(6),ordinaryMinimumPayoutAssumption:'1.000000',conditionalSurplus:((200-packageCostTicks)/200).toFixed(6),maximumDisplayedPackages:Math.min(...bids.map(x=>x.quantity)),settlementRulesVerified:false,atomicExecution:false});
 }
}
const observations=[...opportunities.values()],races=[...new Set(observations.map(x=>x.race))].map(race=>{const rows=observations.filter(x=>x.race===race);return{race,observations:rows.length,first:rows[0],last:rows.at(-1),largestConditionalSurplus:rows.reduce((a,b)=>Number(a.conditionalSurplus)>Number(b.conditionalSurplus)?a:b)};});
const result={schemaVersion:1,catalogSha256:createHash('sha256').update(catalogRaw).digest('hex'),catalogObservedAt:catalog.at(-1).receivedAt,sourceSha256:createHash('sha256').update(raw).digest('hex'),sourceFinalHash:study.previous,sourceEventCount:study.sequence,window:{from:study.firstAt,to:study.lastAt},matchedPairs:pairs.length,maximumBookSourceSkewMs:1000,maximumBookAgeMs:10000,races,contestOrders:0,profitabilityEstablished:false,limitations:['Exact title matching is a discovery heuristic, not verified mutually exclusive settlement rules.','Two separate NO contracts pay at least one only under the ordinary mutually exclusive party-winner assumption; fusion, inconsistent resolution and refunds require explicit scenarios.','Packages are two non-atomic orders; displayed depth does not guarantee both fills or persistent availability.','These are conditional settlement surpluses, not immediate engine netting or market-making cash profits.','No catalog refresh or additional account request was made.']};
fs.writeFileSync(output,JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({matchedPairs:pairs.length,races:races.length,observations:observations.length,profitableImmediateRoundTrips:observations.filter(x=>Number(x.immediateRoundTripPnl)>0).length,window:result.window},null,2));
