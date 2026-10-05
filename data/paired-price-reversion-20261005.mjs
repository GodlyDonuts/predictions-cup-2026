import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {performance} from 'node:perf_hooks';
import {journalLines} from '../live-paper/journal-lines.mjs';import {sealedEnd} from './replay-dual-no-timelapse-20261005.mjs';
import {amount,money,SCALE,TICK} from '../paper/fixed.mjs';
import {fileURLToPath} from 'node:url';
const need=(v,c)=>{if(!v)throw Error('PAIR_PRICE_'+c);};
// Fixed diagnostic policy, declared before scanning. Bulk quotes have no
// executable depth: this is opportunity selection, never paper fills or P&L.
export const POLICY=Object.freeze({maximumQuoteAgeMs:20000,maximumSkewMs:10000,latencyMs:2000,maximumHoldMs:1800000,entryCost:'0.980000',targetQuoteMargin:'0.010000',perLegAdverseTicks:1,feePerShare:'0.000000'});
export function partyTitlePairs(markets,universe){
 const group=new Map(),scope=new Map(universe.exchanges.map(x=>[x.marketId,x]));
 for(const m of markets){const match=/^Will the (Democratic|Republican) Party win the (.+)\?$/.exec(m.title||'');if(!match)continue;const e=scope.get(m.id);if(!e||e.option!=='YES')continue;
  const rows=group.get(match[2])||[];rows.push({marketId:m.id,exchangeId:e.exchangeId,party:match[1]});group.set(match[2],rows);
 }
 return[...group].filter(([,x])=>x.length===2&&new Set(x.map(y=>y.party)).size===2).map(([title,legs])=>({title,legs:legs.sort((a,b)=>a.party.localeCompare(b.party)),contractPredicatesVerified:false}));
}
export function pairedQuotePacket(pair,cache,at){
 const quotes=pair.legs.map(l=>cache.get(l.exchangeId));if(!quotes.every(Boolean))return null;
 if(quotes.some(q=>q.requestStartedAt>at||q.receivedAt>at||at-q.requestStartedAt>POLICY.maximumQuoteAgeMs||q.bestBid===null||q.bestAsk===null||q.bestBid>q.bestAsk))return null;
 if(Math.max(...quotes.map(q=>q.receivedAt))-Math.min(...quotes.map(q=>q.requestStartedAt))>POLICY.maximumSkewMs)return null;
 const fixed=n=>{need(typeof n==='number'&&Number.isFinite(n)&&n>=0&&n<=1,'QUOTE');return amount(n.toFixed(6));};
 const buy=quotes.reduce((v,q)=>v+SCALE-fixed(q.bestBid)+TICK,0n),sell=quotes.reduce((v,q)=>v+SCALE-fixed(q.bestAsk)-TICK,0n);
 if(quotes.some(q=>SCALE-fixed(q.bestBid)+TICK>=SCALE||SCALE-fixed(q.bestAsk)-TICK<=0n))return null;
 return{buy,sell,requests:quotes.map(q=>q.requestStartedAt),received:quotes.map(q=>q.receivedAt)};
}
export function createQuoteReversionScan(pairs){
 const cache=new Map(),states=pairs.map(pair=>({pair,usablePackets:0,quotedEntries:0,quotedProfitableExits:0,quotedTimeouts:0,entry:null,quoteMargins:[],holdingMs:[],minBuy:null,last:null}));
 return{observe(snapshot){const at=Date.parse(snapshot.receivedAt),start=Date.parse(snapshot.requestStartedAt);need(Number.isSafeInteger(at)&&Number.isSafeInteger(start)&&at>=start,'CLOCK');
  const changed=new Set();for(const q of snapshot.quotes){cache.set(q.exchangeId,{...q,requestStartedAt:start,receivedAt:at});changed.add(q.exchangeId);}
  for(const s of states){if(!s.pair.legs.some(l=>changed.has(l.exchangeId)))continue;const p=pairedQuotePacket(s.pair,cache,at);if(!p)continue;s.usablePackets++;s.last={at:new Date(at).toISOString(),buy:money(p.buy),sell:money(p.sell)};s.minBuy=s.minBuy===null?p.buy:p.buy<s.minBuy?p.buy:s.minBuy;
   if(s.entry){const e=s.entry;if(!p.requests.every(t=>t>=e.at+POLICY.latencyMs))continue;
    if(p.sell-e.cost>=amount(POLICY.targetQuoteMargin)||at-e.at>=POLICY.maximumHoldMs){const margin=p.sell-e.cost;s.quoteMargins.push(money(margin));s.holdingMs.push(at-e.at);margin>=amount(POLICY.targetQuoteMargin)?s.quotedProfitableExits++:s.quotedTimeouts++;s.entry=null;}continue;
   }
   if(p.buy<=amount(POLICY.entryCost)){s.entry={at,cost:p.buy};s.quotedEntries++;}
  }
 },report(){return states.map(s=>({title:s.pair.title,legs:s.pair.legs,contractPredicatesVerified:false,usablePackets:s.usablePackets,minimumQuotedAcquisition:s.minBuy===null?null:money(s.minBuy),quotedEntrySignals:s.quotedEntries,quotedProfitableExitSignals:s.quotedProfitableExits,quotedTimeoutExitSignals:s.quotedTimeouts,quotedRoundtripMargins:s.quoteMargins,totalClosedQuoteMargin:money(s.quoteMargins.reduce((n,v)=>n+amount(v),0n)),quoteHoldingMs:s.holdingMs,openQuotedSignal:s.entry?{at:new Date(s.entry.at).toISOString(),cost:money(s.entry.cost)}:null,last:s.last,depthAvailable:false,executionEvidence:false,simulatedFills:0,realizedPnl:null}));}};
}
async function main(){
 const rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),file=path.join(rt,'market-prices.jsonl'),universe=JSON.parse(fs.readFileSync('live-paper/price-universe.json')),markets=JSON.parse(fs.readFileSync('data/markets.json'));
 const pairs=partyTitlePairs(markets,universe),scan=createQuoteReversionScan(pairs),end=sealedEnd(file),raw=createHash('sha256');let sequence=0,previous='',first=null,last=null;const started=performance.now();
 for(const line of journalLines(file,{endByte:end})){raw.update(line+'\n');const row=JSON.parse(line),{hash,...body}=row;need(row.sequence===++sequence&&row.previous===previous&&createHash('sha256').update(JSON.stringify(body)).digest('hex')===hash,'CHAIN');previous=hash;need(row.event.type==='market_price_observation','SOURCE');first??=row.event.snapshot.receivedAt;last=row.event.snapshot.receivedAt;scan.observe(row.event.snapshot);}
 const results=scan.report().sort((a,b)=>Number(b.totalClosedQuoteMargin)-Number(a.totalClosedQuoteMargin)||b.quotedRoundtripMargins.length-a.quotedRoundtripMargins.length),summary={hours:(Date.parse(last)-Date.parse(first))/3600000,quotedEntrySignals:results.reduce((n,r)=>n+r.quotedEntrySignals,0),quotedClosedSignals:results.reduce((n,r)=>n+r.quotedRoundtripMargins.length,0),quotedProfitableExits:results.reduce((n,r)=>n+r.quotedProfitableExitSignals,0),quotedTimeouts:results.reduce((n,r)=>n+r.quotedTimeoutExitSignals,0),totalClosedQuoteMargin:money(results.reduce((n,r)=>n+amount(r.totalClosedQuoteMargin),0n)),positiveNetQuotePairs:results.filter(r=>amount(r.totalClosedQuoteMargin)>0n).length};
 const out={schemaVersion:1,at:new Date().toISOString(),policy:POLICY,source:{prefixBytes:end,sha256:raw.digest('hex'),records:sequence,tailHash:previous,first,last},wallMs:performance.now()-started,pairsScreened:pairs.length,summary,results,competitionOrders:0,networkRequests:0,simulatedFills:0,profitabilityEstablished:false,selectionIsExploratory:true,limitations:['Historical bulk bid/ask quotes contain no executable depth and do not establish fills, order acceptance, queue priority or capital returns.','Title-paired catalogue groups require exact contract-root review; administrative resolution or differing predicates can invalidate payout assumptions.','Quote exit signals omit sequential partial-fill and failed-hedge risk. They rank future exact depth capture only.','Summed quote margins across overlapping opportunities are not a capital-constrained wallet, account return or realized P&L.','No current account eligibility or later contract review was backfilled into historical sources.','Fixed policy is declared before this scan; results are exploratory and require untouched later depth and prospective policy validation.']};fs.writeFileSync('data/paired-price-reversion-results-20261005.json',JSON.stringify(out,null,2)+'\n');console.log(JSON.stringify({records:sequence,pairs:pairs.length,wallMs:out.wallMs,summary,profitabilityEstablished:false}));
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){await main();}
