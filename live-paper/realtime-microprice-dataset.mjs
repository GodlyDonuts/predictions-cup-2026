import {instantNs} from '../feed/contract.mjs';
const need=(v,c)=>{if(!v)throw Error('MICROPRICE_DATA_'+c);},ticks=p=>{const n=p*200;need(Number.isFinite(n)&&Math.abs(n-Math.round(n))<1e-7,'TICK');return Math.round(n);};
export const MICROPRICE_FEATURES=Object.freeze(['midProbability','spreadTicks','imbalance1','imbalance3','imbalance10','imbalance20','logBidDepth','logAskDepth','micropriceDeviationTicks','return60sTicks','return300sTicks','range60sTicks','range300sTicks','signedFlow30s','signedFlow120s','logVolume30s','logVolume120s','tradeCount30s','lastTradeAgeSeconds','imbalanceProbabilityInteraction']);
export const MICROPRICE_DATA_POLICY=Object.freeze({sampleMs:30000,latencyMs:3000,horizonMs:300000,labelToleranceMs:20000,historyMs:300000,minimumWarmupMs:60000,depthFractionBps:100,quantity:1,entrySlippageTicks:1,exitSlippageTicks:1,feeTicks:0});
function parsedBook(b,at){if(!b?.asOf||!b.bids?.length||!b.asks?.length)return null;need(instantNs(b.asOf.at)<=BigInt(at)*1000000n+1000000000n,'FUTURE_BOOK');const bid=ticks(b.bids[0].price),ask=ticks(b.asks[0].price);need(bid<ask,'CROSSED');return{at,bid,ask,mid:(bid+ask)/2,bids:b.bids,asks:b.asks,sequence:b.asOf.sequence,expiry:b.nextExpiryAt?Date.parse(b.nextExpiryAt):null};}
export function micropriceFeatures(m,b,at){
 const depth=(rows,n)=>rows.slice(0,n).reduce((v,l)=>v+l.quantity,0),imb=n=>{const x=depth(b.bids,n),y=depth(b.asks,n);return(x-y)/(x+y);},dBid=depth(b.bids,20),dAsk=depth(b.asks,20),prob=b.mid/200;
 const window=ms=>m.history.filter(h=>at-h.at<=ms),old=ms=>{const rows=m.history.filter(h=>h.at<=at-ms);return rows.at(-1)?.mid??m.history[0].mid;},range=ms=>{const rows=window(ms).map(h=>h.mid);return Math.max(...rows,b.mid)-Math.min(...rows,b.mid);};
 const flow=ms=>{const rows=m.trades.filter(t=>at-t.at<=ms),volume=rows.reduce((n,t)=>n+t.quantity,0);return{imbalance:volume?rows.reduce((n,t)=>n+t.sign*t.quantity,0)/volume:0,volume,count:rows.length};},f30=flow(30000),f120=flow(120000),micro=(b.ask*b.bids[0].quantity+b.bid*b.asks[0].quantity)/(b.bids[0].quantity+b.asks[0].quantity)-b.mid;
 const x=[prob,b.ask-b.bid,imb(1),imb(3),imb(10),imb(20),Math.log1p(dBid),Math.log1p(dAsk),micro,b.mid-old(60000),b.mid-old(300000),range(60000),range(300000),f30.imbalance,f120.imbalance,Math.log1p(f30.volume),Math.log1p(f120.volume),f30.count,Math.min(120,(at-(m.trades.at(-1)?.at??at-120000))/1000),imb(1)*prob*(1-prob)];need(x.every(Number.isFinite),'FEATURES');return x;
}
export function createMicropriceDataset({onSample=()=>{},policy=MICROPRICE_DATA_POLICY}={}){
 const markets=new Map(),seen=new Set();let clock=0,index=-1,source=null;const counts={books:0,trades:0,labels:0,samples:0,censored:0,gaps:0};
 const market=id=>{let m=markets.get(id);if(!m){m={book:null,history:[],trades:[],pending:[],revision:null,lastSample:-Infinity};markets.set(id,m);}return m;};
 function clear(m){counts.censored+=m.pending.length;m.book=null;m.history=[];m.trades=[];m.pending=[];m.revision=null;}
 function applyBook(id,raw,at,provenance){const m=market(id),b=parsedBook(raw,at);if(!b||b.expiry&&b.expiry<=at)return;if(m.book&&b.sequence<m.book.sequence)return;
  if(m.book&&b.sequence===m.book.sequence)return;counts.books++;m.book=b;
  for(const sample of m.pending){
   if(!sample.entryAt&&at>=Date.parse(sample.at)+policy.latencyMs){
    if(at>Date.parse(sample.at)+policy.latencyMs+policy.labelToleranceMs||Math.floor(Math.min(b.bids[0].quantity,b.asks[0].quantity)*policy.depthFractionBps/10000)<policy.quantity){counts.censored++;sample.done=true;continue;}
    if(instantNs(raw.asOf.at)<BigInt(Date.parse(sample.at)+policy.latencyMs)*1000000n)continue;
    sample.entryAt=new Date(at).toISOString();sample.entryBidTicks=b.bid;sample.entryAskTicks=b.ask;sample.entrySource=provenance;
   }
   if(at<sample.targetAt)continue;if(!sample.entryAt||at>sample.targetAt+policy.labelToleranceMs){counts.censored++;sample.done=true;continue;}
   const depth=Math.min(b.bids[0].quantity,b.asks[0].quantity);if(Math.floor(depth*policy.depthFractionBps/10000)<policy.quantity){counts.censored++;sample.done=true;continue;}
   onSample({...sample,labelAt:new Date(at).toISOString(),labelSource:provenance,y:b.mid-sample.mid,futureBidTicks:b.bid,futureAskTicks:b.ask,yesTakerNetTicks:b.bid-policy.exitSlippageTicks-sample.entryAskTicks-policy.entrySlippageTicks,noTakerNetTicks:sample.entryBidTicks-b.ask-policy.exitSlippageTicks-policy.entrySlippageTicks});sample.done=true;counts.labels++;
  }m.pending=m.pending.filter(s=>!s.done);
  const prior=m.history.at(-1);if(!prior||at-prior.at>=1000)m.history.push({at,mid:b.mid});m.history=m.history.filter(h=>at-h.at<=policy.historyMs+1000);m.trades=m.trades.filter(t=>at-t.at<=120000);
  if(!m.history.length||at-m.history[0].at<policy.minimumWarmupMs||at-m.lastSample<policy.sampleMs||Math.floor(Math.min(b.bids[0].quantity,b.asks[0].quantity)*policy.depthFractionBps/10000)<policy.quantity)return;
  const x=micropriceFeatures(m,b,at);m.pending.push({marketId:id,at:new Date(at).toISOString(),targetAt:at+policy.horizonMs,mid:b.mid,bid:b.bid,ask:b.ask,x,source:provenance});m.lastSample=at;counts.samples++;
 }
 function consume(row,{sourceId='capture'}={}){need(row?.format==='sig-feed-capture'&&row.schemaVersion===1,'ENVELOPE');const at=Date.parse(row.observedAt);need(Number.isSafeInteger(at)&&at>=clock,'CLOCK');clock=at;if(sourceId!==source){source=sourceId;index=-1;for(const m of markets.values())clear(m);}need(row.captureIndex===index+1,'INDEX');index=row.captureIndex;const id=/:market:(\d+)$/.exec(row.topic)?.[1];need(id,'SCOPE');const m=market(id),provenance={file:sourceId,captureIndex:index};
  if(['invalidation','subscribed','disconnected','rejected','stopped'].includes(row.kind)){clear(m);return;}
  if(row.kind==='snapshot'){for(const b of row.books||[])applyBook(id,b,at,provenance);return;}
  if(row.kind!=='event')return;if(row.event!=='market_batch'){clear(m);return;}const p=row.payload;
  if(p.resyncRequired||m.revision!==null&&p.delivery.previousRevision>m.revision){counts.gaps++;clear(m);}m.revision=Math.max(m.revision??0,p.delivery.revision);
  if(p.marketSettled?.length||p.trades?.some(t=>t.sequence===null)||(p.bookDirty||[]).some(d=>!(p.books||[]).some(b=>b.exchangeId===d.exchangeId))){clear(m);return;}
  for(const t of p.trades||[]){const key=id+':'+t.id;if(seen.has(key))continue;seen.add(key);counts.trades++;const price=ticks(t.price);m.trades.push({at,quantity:t.quantity,sign:m.book?price>=m.book.ask?1:price<=m.book.bid?-1:0:0});}
  for(const b of p.books||[])applyBook(id,b,at,provenance);
 }
 return{consume,report:()=>({policy,features:MICROPRICE_FEATURES,counts,unresolved:[...markets.values()].reduce((n,m)=>n+m.pending.length,0),through:new Date(clock).toISOString()})};
}
