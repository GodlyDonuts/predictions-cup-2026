// Offline market-only diagnostic. Samples overlap; their sum is NOT portfolio P&L.
// Full pushed books are research inputs, not proof of REST/account readiness.
import {instantNs} from '../feed/contract.mjs';
import {micropriceFeatures} from './realtime-microprice-dataset.mjs';
const need=(ok,code)=>{if(!ok)throw Error('REALTIME_ECONOMICS_'+code);};
export const REALTIME_ECONOMICS_POLICIES=Object.freeze([0,1].flatMap(improvementTicks=>[3000,6000,10000].map(latencyMs=>Object.freeze({id:'improve'+improvementTicks+'-latency'+latencyMs,improvementTicks,latencyMs,quoteMs:120000,markoutMs:300000,sampleEveryMs:30000,historyMs:300000,minimumSpanMs:240000,maximumMidpointRangeTicks:1,maximumBookAgeMs:20000,depthFractionBps:100,quantity:1,feeTicks:0,exitSlippageTicks:1}))));
const tick=n=>{const x=n*200;need(Number.isFinite(x)&&Math.abs(x-Math.round(x))<1e-7&&x>=0&&x<=200,'TICK');return Math.round(x);};
function normalizedBook(raw,at){if(!raw?.asOf||!raw.bids?.length||!raw.asks?.length)return null;const sourceNs=instantNs(raw.asOf.at),nowNs=BigInt(at)*1000000n;need(sourceNs<=nowNs+1000000000n,'FUTURE_BOOK');const bid=tick(raw.bids[0].price),ask=tick(raw.asks[0].price);need(bid<ask&&Number.isSafeInteger(raw.asOf.sequence),'BOOK');return{bid,ask,mid:(bid+ask)/2,bids:raw.bids,asks:raw.asks,bidSize:raw.bids[0].quantity,askSize:raw.asks[0].quantity,sequence:raw.asOf.sequence,sourceNs,at,expiry:raw.nextExpiryAt?Date.parse(raw.nextExpiryAt):null};}
const stateFor=p=>({policy:p,markets:new Map(),counts:{quotes:0,paired:0,unfilled:0,oneSided:0,marked:0,censored:0,gapExposed:0,expiryInvalidations:0},pairedGainTicks:0,oneSidedMarkoutTicks:0,outcomes:[]});
export function createRealtimeSpreadEconomics({policies=REALTIME_ECONOMICS_POLICIES,decideQuote=()=>true,onQuote=()=>{},onFill=()=>{},onOutcome=()=>{}}={}){
 const banks=policies.map(stateFor),seenTrades=new Set(),featureMarkets=new Map();let lastAt=0,lastIndex=-1,source=null,sourceRecord=null;const metrics={events:0,books:0,trades:0,duplicateTrades:0,gaps:0,invalidations:0,sessions:0,rejectedQuotes:0};
 function featureMarket(id){let m=featureMarkets.get(id);if(!m){m={book:null,history:[],trades:[]};featureMarkets.set(id,m);}return m;}
 function market(s,id){let m=s.markets.get(id);if(!m){m={book:null,history:[],sampleAt:-Infinity,revision:null,quote:null};s.markets.set(id,m);}return m;}
 function finish(s,m,q,at,kind,mark=null){const fills=q.fills.filter(Boolean);s.counts[kind]++;let value=null;
  if(kind==='paired'){value=200-q.limits[0]-q.limits[1];s.pairedGainTicks+=value;}
  if(kind==='marked'){const side=q.fills[0]?0:1;value=(side===0?mark.bid:200-mark.ask)-s.policy.exitSlippageTicks-q.limits[side];s.oneSidedMarkoutTicks+=value;}
  if(fills.length===1&&kind!=='paired')s.counts.oneSided++;
  if(q.gapExposed)s.counts.gapExposed++;
  const outcome={policyId:s.policy.id,marketId:q.marketId,createdAt:new Date(q.at).toISOString(),resolvedAt:new Date(at).toISOString(),limitsTicks:q.limits,kind,counterfactualNetTicks:value,gapExposed:q.gapExposed,tradeSources:q.fills.filter(Boolean),x:q.x,decisionSource:q.decisionSource};s.outcomes.push(outcome);onOutcome(structuredClone(outcome));m.quote=null;
 }
 function checkpoint(s,m,at){const q=m.quote;if(!q)return;const fills=q.fills.filter(Boolean);
  if(fills.length===2){finish(s,m,q,at,'paired');return;}
  if(at<q.expiry)return;
  if(!fills.length){finish(s,m,q,at,q.gapExposed?'censored':'unfilled');return;}
  const learned=q.fills.find(Boolean).learnedAt;
  if(at<learned+s.policy.markoutMs+s.policy.latencyMs)return;
  const b=m.book;
  if(b&&b.at>=learned+s.policy.markoutMs+s.policy.latencyMs&&at-b.at<=s.policy.maximumBookAgeMs&&(!b.expiry||b.expiry>at)){
   const side=q.fills[0]?0:1,quantity=side===0?b.bidSize:b.askSize;
   if(Math.floor(quantity*s.policy.depthFractionBps/10000)>=1){finish(s,m,q,at,q.gapExposed?'censored':'marked',b);return;}
  }
  if(at>learned+s.policy.markoutMs+s.policy.latencyMs+s.policy.maximumBookAgeMs)finish(s,m,q,at,'censored');
 }
 function invalidate(id,at){metrics.invalidations++;featureMarkets.delete(id);for(const s of banks){const m=market(s,id);m.book=null;m.history=[];m.revision=null;if(m.quote)m.quote.gapExposed=true;checkpoint(s,m,at);}}
 function trades(id,rows,at){for(const t of rows){need(t.marketId===id&&typeof t.id==='string'&&Number.isSafeInteger(t.sequence)&&Number.isSafeInteger(t.quantity)&&t.quantity>0,'TRADE');const key=id+':'+t.id;if(seenTrades.has(key)){metrics.duplicateTrades++;continue;}seenTrades.add(key);metrics.trades++;const sourceNs=instantNs(t.executedAt);need(sourceNs<=BigInt(at)*1000000n+1000000000n,'FUTURE_TRADE');const price=tick(t.price);const fm=featureMarket(id);fm.trades.push({at,quantity:t.quantity,sign:fm.book?price>=fm.book.ask?1:price<=fm.book.bid?-1:0:0});
   for(const s of banks){const m=market(s,id),q=m.quote;if(!q||q.gapExposed||at>=q.expiry||sourceNs<BigInt(q.activation)*1000000n||sourceNs>=BigInt(q.expiry)*1000000n)continue;
    // Process trades before their resulting pushed book. A later quote cannot
    // claim a trade which caused the book on which that quote is based.
    let remaining=t.quantity;for(let side=0;side<2&&remaining;side++)if(!q.fills[side]&&(side===0?price<q.limits[0]:200-price<q.limits[1])){q.fills[side]={id:t.id,sequence:t.sequence,executedAt:t.executedAt,learnedAt:at,priceTicks:price};onFill({policyId:s.policy.id,marketId:id,createdAt:new Date(q.at).toISOString(),learnedAt:new Date(at).toISOString(),side:side===0?'YES':'NO',limitTicks:q.limits[side],source:structuredClone(q.fills[side])});remaining--;}
    checkpoint(s,m,at);
   }
  }}
 function book(id,raw,at){const b=normalizedBook(raw,at);if(!b)return;metrics.books++;
  const fm=featureMarket(id);if(!fm.book||b.sequence>fm.book.sequence){fm.book=b;const previous=fm.history.at(-1);if(!previous||at-previous.at>=1000)fm.history.push({at,mid:b.mid});fm.history=fm.history.filter(h=>at-h.at<=301000);fm.trades=fm.trades.filter(t=>at-t.at<=120000);}
  for(const s of banks){const p=s.policy,m=market(s,id);if(m.book&&b.sequence<m.book.sequence)continue;
   if(m.book&&b.sequence===m.book.sequence){need(b.bid===m.book.bid&&b.ask===m.book.ask,'VERSION_CONFLICT');continue;}
   if(b.expiry&&b.expiry<=at){s.counts.expiryInvalidations++;m.book=null;continue;}m.book=b;
   const sample=m.history.at(-1);if(!sample||at-sample.at>=1000)m.history.push({at,mid:b.bid+b.ask});m.history=m.history.filter(x=>at-x.at<=p.historyMs);checkpoint(s,m,at);
   if(m.quote||at-m.sampleAt<p.sampleEveryMs||!m.history.length||at-m.history[0].at<p.minimumSpanMs)continue;
   const mids=m.history.map(x=>x.mid);if(Math.max(...mids)-Math.min(...mids)>2*p.maximumMidpointRangeTicks)continue;
   const spread=b.ask-b.bid,limits=[b.bid+p.improvementTicks,200-b.ask+p.improvementTicks];
   if(spread>10||limits.some(x=>x<10||x>190)||limits[0]+limits[1]>199||limits[0]>=b.ask||limits[1]>=200-b.bid||Math.floor(Math.min(b.bidSize,b.askSize)*p.depthFractionBps/10000)<p.quantity)continue;
   const activation=Math.max(at+p.latencyMs,Number(b.sourceNs/1000000n)+2000);m.sampleAt=at;
   const candidate={policyId:p.id,marketId:id,createdAt:new Date(at).toISOString(),activationAt:new Date(activation).toISOString(),expiresAt:new Date(at+p.quoteMs).toISOString(),limitsTicks:limits,x:micropriceFeatures(fm,b,at),decisionSource:sourceRecord};
   if(decideQuote(structuredClone(candidate))!==true){metrics.rejectedQuotes++;continue;}
   m.quote={marketId:id,at,activation,expiry:at+p.quoteMs,limits,fills:[null,null],gapExposed:false,x:candidate.x,decisionSource:sourceRecord};s.counts.quotes++;onQuote(structuredClone(candidate));
  }}
 function consume(row,{sourceId='capture'}={}){need(row?.format==='sig-feed-capture'&&row.schemaVersion===1&&Number.isSafeInteger(row.captureIndex),'ENVELOPE');const at=Date.parse(row.observedAt);need(Number.isSafeInteger(at)&&at>=lastAt,'CLOCK');if(source!==sourceId){source=sourceId;lastIndex=-1;metrics.sessions++;featureMarkets.clear();for(const s of banks)for(const m of s.markets.values()){m.book=null;m.history=[];m.revision=null;if(m.quote)m.quote.gapExposed=true;}}need(row.captureIndex===lastIndex+1,'SOURCE_INDEX');lastIndex=row.captureIndex;lastAt=at;sourceRecord={file:sourceId,captureIndex:row.captureIndex,observedAt:row.observedAt};
  const id=/:market:(\d+)$/.exec(row.topic)?.[1];need(id,'TOPIC');
  if(['subscribed','disconnected','stopped','invalidation','rejected'].includes(row.kind)){invalidate(id,at);return;}
  if(row.kind==='snapshot'){for(const b of row.books||[])book(id,b,at);return;}
  if(row.kind!=='event')return;metrics.events++;
  if(row.event!=='market_batch'){invalidate(id,at);return;}const p=row.payload;need(p&&p.delivery,'DELIVERY');let gap=p.resyncRequired===true;
  for(const s of banks){const m=market(s,id);if(m.revision!==null&&p.delivery.previousRevision>m.revision)gap=true;m.revision=Math.max(m.revision??0,p.delivery.revision);}
  if(gap){metrics.gaps++;invalidate(id,at);}
  if(p.marketSettled?.length||p.trades?.some(t=>t.sequence===null)||(p.bookDirty||[]).some(d=>!(p.books||[]).some(b=>b.exchangeId===d.exchangeId))){invalidate(id,at);return;}
  trades(id,p.trades||[],at);for(const b of p.books||[])book(id,b,at);
 }
 function report(){return{schemaVersion:1,at:new Date(lastAt||0).toISOString(),mode:'offline_overlapping_quote_opportunity_diagnostic',networkRequests:0,competitionOrders:0,profitabilityEstablished:false,fees:0,metrics,policies:banks.map(s=>({policy:s.policy,counts:s.counts,unresolved:[...s.markets.values()].filter(m=>m.quote).length,pairedGainTicks:s.pairedGainTicks,oneSidedMarkoutTicks:s.oneSidedMarkoutTicks,conditionalDiagnosticTicks:s.pairedGainTicks+s.oneSidedMarkoutTicks,outcomes:s.outcomes})),limitations:['Overlapping opportunities are not a funded portfolio and cannot be added as returns.','Strict-through anonymous prints do not establish actual queue position or execution.','Research uses full pushed snapshots; production additionally requires fresh REST/account eligibility and validated continuity.','Any quote exposed to a known capture gap is censored; unknown transport losses may remain.','One-sided markouts require a later fresh book and depth, with one-tick exit slippage.']};}
 return{consume,report};
}
