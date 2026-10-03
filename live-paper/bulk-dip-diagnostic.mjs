import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {amount,money,TICK} from '../paper/fixed.mjs';
import {digest} from './model.mjs';
import {BID_ANCHOR_POLICY} from './bid-anchor.mjs';
import {quotedDipCandidate} from './bulk-dip.mjs';
import {validateUniverse} from './breadth.mjs';
import {validatePriceObservation} from './market-price-publication.mjs';
import {atomicJson} from '../execution/ledger.mjs';

const price=n=>money(BigInt(Math.round(n*1e6))),inc=(o,k)=>o[k]=(o[k]||0)+1;
export function diagnoseQuotedDip(points,q,at,policy=BID_ANCHOR_POLICY){
 const now=Date.parse(at),a=points.filter(p=>Date.parse(p.at)<now&&Date.parse(p.at)>=now-policy.historyMs);
 const result={reason:null,samples:a.length,features:null};
 const fail=reason=>({...result,reason});
 if(a.length<policy.minimumMinutes)return fail('history_minutes');
 if(Date.parse(a.at(-1).at)-Date.parse(a[0].at)<policy.minimumSpanMs)return fail('history_span');
 if(now-Date.parse(a.at(-1).at)>policy.maximumGapMs||a.some((p,i)=>i&&Date.parse(p.at)-Date.parse(a[i-1].at)>policy.maximumGapMs))return fail('history_gap');
 if(q.crossed||typeof q.bestBid!=='number'||typeof q.bestAsk!=='number'||q.bestBid<=0||q.bestAsk>=1||q.bestBid>=q.bestAsk)return fail('invalid_quote');
 const values=a.map(p=>amount(p.bid)).sort((x,y)=>x<y?-1:x>y?1:0),anchor=values[Math.floor((values.length-1)/2)],
  bid=amount(price(q.bestBid)),ask=amount(price(q.bestAsk)),range=values.at(-1)-values[0],spread=ask-bid,bidDrop=anchor-bid,concession=anchor-2n*TICK-ask;
 result.features={anchor:money(anchor),range:money(range),spread:money(spread),bidDrop:money(bidDrop),netConcession:money(concession)};
 const failures=[];
 if(range>amount(policy.maximumAnchorRange))failures.push('unstable_anchor');
 if(spread>amount(policy.maximumSpread))failures.push('spread');
 if(bidDrop>amount(policy.maximumBidDrop))failures.push('bid_collapse');
 if(concession<amount(policy.minimumNetConcession))failures.push('insufficient_concession');
 result.reason=failures[0]||'depth_candidate';result.failures=failures;
 if(Boolean(quotedDipCandidate(points,q,at,policy))!==(result.reason==='depth_candidate'))throw Error('DIAGNOSTIC_GATE_DIVERGENCE');
 return result;
}
function summary(){return {quotes:0,markets:new Set(),receipts:new Set(),observedFirstAt:null,observedLastAt:null,primaryGates:{},allReadyGates:{},priceChanges:{unchanged:0,askDown:0,askUp:0,askDownAtLeastTwoCents:0,unobservedGapComparisons:0},gaps:[],nearest:[],candidates:[]};}
const quantile=(a,p)=>a.length?a.slice().sort((x,y)=>x-y)[Math.floor((a.length-1)*p)]:null;
function finish(s){return {quotes:s.quotes,markets:s.markets.size,observedFirstAt:s.observedFirstAt,observedLastAt:s.observedLastAt,priceBatchReceipts:s.receipts.size,primaryGates:s.primaryGates,allReadyGates:s.allReadyGates,priceChanges:s.priceChanges,
 quoteGapMs:{samples:s.gaps.length,p50:quantile(s.gaps,.5),p90:quantile(s.gaps,.9),maximum:s.gaps.length?s.gaps.reduce((a,b)=>Math.max(a,b)):null},
 nearestConcessions:s.nearest,quoteOnlyCandidates:s.candidates};}
export function diagnosePriceCapture(bytes,universe,{lookbackMs=21600000,cutoff=null}={}){
 if(!Number.isSafeInteger(lookbackMs)||lookbackMs<600000||cutoff!==null&&!Number.isFinite(Date.parse(cutoff)))throw Error('DIAGNOSTIC_OPTIONS');
 const canonical=validateUniverse(universe,universe.tournamentId),end=bytes.lastIndexOf(10)+1,prefix=bytes.subarray(0,end),records=prefix.toString().split('\n').slice(0,-1).map(JSON.parse);
 if(!records.length)throw Error('DIAGNOSTIC_EMPTY');
 const lastAt=records.at(-1).event?.snapshot?.receivedAt,start=Date.parse(lastAt)-lookbackMs;
 let sequence=0,previous='',lastReceived=-Infinity,lastPublished=-Infinity;const history={},latest={},all=summary(),segments={beforeCutoff:summary(),afterCutoff:summary()};
 for(const record of records){
  const {hash,...body}=record,snapshot=body.event?.snapshot;
  if(body.sequence!==++sequence||body.previous!==previous||digest(body)!==hash||body.state!==null||body.event?.type!=='market_price_observation')throw Error('DIAGNOSTIC_CHAIN');
  validatePriceObservation(snapshot,canonical);const now=Date.parse(snapshot.receivedAt),published=Date.parse(body.at);
  if(!Number.isFinite(published)||published<now||published<lastPublished||now<lastReceived)throw Error('DIAGNOSTIC_CLOCK');
  previous=hash;lastReceived=now;lastPublished=published;
  for(const id of snapshot.missingIds){const market=canonical.exchanges.find(x=>x.exchangeId===id)?.marketId;delete history[market];delete latest[market];}
  for(const q of snapshot.quotes){
   if(q.option!=='YES'||q.crossed||q.bestBid===null||q.bestAsk===null||q.bestBid<=0||q.bestAsk>=1||q.bestBid>=q.bestAsk){delete history[q.marketId];delete latest[q.marketId];if(now>=start)inc(all.primaryGates,'invalid_reference');continue;}
   const points=history[q.marketId]||[],prior=latest[q.marketId],gate=diagnoseQuotedDip(points,q,snapshot.receivedAt);
   if(now>=start)for(const s of [all,...(cutoff?[segments[now<Date.parse(cutoff)?'beforeCutoff':'afterCutoff']]:[])]){
    s.quotes++;s.markets.add(q.marketId);s.receipts.add(snapshot.receivedAt);s.observedFirstAt??=snapshot.receivedAt;s.observedLastAt=snapshot.receivedAt;
    inc(s.primaryGates,gate.reason);for(const reason of gate.failures||[])inc(s.allReadyGates,reason);
    if(prior){const gap=now-Date.parse(prior.at);if(Date.parse(prior.at)>=start)s.gaps.push(gap);
     if(Date.parse(prior.at)<start||gap>BID_ANCHOR_POLICY.maximumGapMs)inc(s.priceChanges,'unobservedGapComparisons');
     else{const delta=amount(price(q.bestAsk))-amount(price(prior.ask));inc(s.priceChanges,delta===0n?'unchanged':delta<0n?'askDown':'askUp');if(delta<=-20000n)inc(s.priceChanges,'askDownAtLeastTwoCents');}
    }
    if(gate.features&&(!gate.failures.length||gate.failures.every(r=>r==='insufficient_concession'))){
     s.nearest.push({marketId:q.marketId,exchangeId:q.exchangeId,receivedAt:snapshot.receivedAt,...gate.features});
     s.nearest.sort((a,b)=>Number(amount(b.netConcession)-amount(a.netConcession))||Date.parse(a.receivedAt)-Date.parse(b.receivedAt));s.nearest=s.nearest.slice(0,20);
    }
    if(gate.reason==='depth_candidate')s.candidates.push({marketId:q.marketId,exchangeId:q.exchangeId,receivedAt:snapshot.receivedAt,...gate.features});
   }
   const minute=Math.floor(now/60000);if(!points.some(p=>p.minute===minute))points.push({minute,at:snapshot.receivedAt,bid:price(q.bestBid)});
   history[q.marketId]=points.filter(p=>Date.parse(p.at)>=now-BID_ANCHOR_POLICY.historyMs);latest[q.marketId]={at:snapshot.receivedAt,ask:q.bestAsk};
  }
 }
 return {mode:'quote_only_dip_diagnostic',createdAt:new Date().toISOString(),codeIdentity:Object.fromEntries(['./bulk-dip-diagnostic.mjs','./bulk-dip.mjs','./bid-anchor.mjs'].map(p=>[p,createHash('sha256').update(fs.readFileSync(new URL(p,import.meta.url))).digest('hex')])),window:{start:new Date(start).toISOString(),end:lastAt,lookbackMs,cutoff},
 source:{bytes:prefix.length,sha256:createHash('sha256').update(prefix).digest('hex'),sequence,hash:previous,excludedPartialTailBytes:bytes.length-prefix.length,universeHash:canonical.hash},
 policy:BID_ANCHOR_POLICY,all:finish(all),segments:cutoff?Object.fromEntries(Object.entries(segments).map(([k,v])=>[k,finish(v)])):null,
 networkRequests:0,competitionOrders:0,profitabilityEstablished:false,
 limitations:['Quotes have unknown source age and no depth. Candidates are historical quote-only diagnostics, never fills or P&L.',
 'This price-only replay does not apply book invalidations or actual consumer receipt gates and can overstate live eligibility.',
 'Each repeated quote is correlated; gate counts and nearest concessions are not independent trades or strategy selection evidence.']};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const args=process.argv.slice(2),option=n=>{const i=args.indexOf(n);return i<0?null:args[i+1];},runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),
  output=option('--output');if(!output)throw Error('DIAGNOSTIC_OUTPUT_REQUIRED');
 const universe=JSON.parse(fs.readFileSync(new URL('./price-universe.json',import.meta.url))),report=diagnosePriceCapture(fs.readFileSync(path.join(runtime,'market-prices.jsonl')),universe,{cutoff:option('--cutoff')});
 atomicJson(path.resolve(output),report);console.log(JSON.stringify({output,source:report.source,window:report.window,all:report.all,limitations:report.limitations},null,2));
}
