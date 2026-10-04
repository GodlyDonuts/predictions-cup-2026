import {payloadHash} from '../execution/ledger.mjs';
import {micros} from './native-dip-proof-v1.mjs';
const need=(x,c)=>{if(!x)throw Error('NATIVE_QUOTE_'+c);},hash=x=>typeof x==='string'&&/^[a-f0-9]{64}$/.test(x),count=x=>Number.isSafeInteger(x)&&x>=0;
const key=c=>c.strategy+'/'+c.scenario.id;
export function verifyNativeQuoteResult({result,declaration:d,binding,universe,previous=null}){
 const cap=result?.capture,e=result?.evaluation,c=d?.config,start=Date.parse(d?.declaredAt),through=Date.parse(e?.evaluationThrough);
 need(binding?.schemaVersion===1&&['engineHash','binaryHash','runtimeLibraryHash','declarationHash'].every(k=>hash(binding[k])),'BINDING');
 need(d.schemaVersion===1&&d.mode==='causal_quote_dip_subsequent_v1'&&binding.cohort===d.cohort&&c.feeBps===0&&c.fillClockPolicy==='independent_request_latency_v2'&&d.maxManagedMarkets===1&&payloadHash(d.universe)===payloadHash(universe)&&Number.isSafeInteger(start),'DECLARATION');
 need(result.declarationFileHash===binding.declarationHash&&cap?.engineHash===binding.engineHash&&cap.mode==='native_compact_market_sources_v1'&&count(cap.events)&&Number.isSafeInteger(through)&&through>=start&&through===Math.max(start,cap.through??0)&&e.evaluationStart===d.declaredAt&&e.cohort===d.cohort&&e.mode===d.mode&&[cap,e].every(v=>v.networkRequests===0&&v.competitionOrders===0)&&e.profitabilityEstablished===false,'CAPTURE');
 for(const name of ['books','quotes','eligibility']){const s=cap.sources[name],dev=d.development[name];need(s&&hash(s.hash)&&count(s.bytes)&&count(s.records)&&Array.isArray(s.verifiedCheckpoints)&&s.verifiedCheckpoints.some(x=>x.bytes===dev.bytes&&x.hash===dev.hash&&(x.through===null||x.through<start)),'SOURCE_BINDING');
  if(previous){const old=previous.capture.sources[name];need(s.bytes>=old.bytes&&s.records>=old.records&&s.verifiedCheckpoints.some(x=>x.bytes===old.bytes&&x.hash===old.hash),'SOURCE_HISTORY');}}
 const expected=new Map();for(const s of d.strategies)for(const scenario of d.scenarios)expected.set(s.id+'/'+scenario.id,{s,scenario});
 need(Array.isArray(e.scorecards)&&e.scorecards.length===expected.size&&e.candidates===expected.size,'CARDS');
 const members=new Map(universe.exchanges.map(x=>[x.exchangeId+':YES',x.marketId])),prior=new Map(previous?.evaluation.scorecards.map(x=>[key(x),x])||[]),seen=new Set(),held=new Set(),pending=[],events=new Set(),cards=[];
 for(const card of e.scorecards){const id=key(card),spec=expected.get(id),p=card.point,old=prior.get(id);need(spec&&!seen.has(id)&&payloadHash(spec.s.parameters)===payloadHash(card.parameters)&&payloadHash(spec.scenario)===payloadHash(card.scenario),'CARD');seen.add(id);
  need(p.at===e.evaluationThrough&&p.fillClockPolicy===c.fillClockPolicy&&Array.isArray(card.fills)&&p.fillExecutions===card.fills.length&&micros(p.fees)===0n&&count(p.intents)&&count(p.openShares)&&count(p.completedTrades),'POINT');
  if(old)need(card.fills.length>=old.fills.length&&old.fills.every((f,i)=>JSON.stringify(f)===JSON.stringify(card.fills[i])),'FILL_HISTORY');
  const lots=new Map(),used=new Set();let cash=micros(c.initialCash),basis=0n,realized=0n,turnover=0n,open=0,sold=0,closed=0,last=start;
  for(const f of card.fills){const q=f.sourceClock,a=f.economics,market=members.get(f.instrument);need(market&&c.marketIds.includes(market)&&['buy','sell'].includes(f.action)&&count(f.quantity)&&f.quantity>0&&count(f.intentId)&&f.intentId>0&&f.intentId<=p.intents&&count(f.at)&&f.at>start&&f.at>=last&&f.at<=through&&micros(f.fee)===0n,'FILL');last=f.at;used.add(String(f.intentId));
   need(q?.policy===c.fillClockPolicy&&[q.decisionAt,q.minimumLatencyMs,q.requestStartedAt,q.receivedAt].every(Number.isSafeInteger)&&q.minimumLatencyMs===card.scenario.latencyMs&&q.decisionAt>start&&q.requestStartedAt>=q.decisionAt+q.minimumLatencyMs&&q.receivedAt===f.at&&f.at>=q.requestStartedAt&&f.at-q.requestStartedAt<=c.maxDataAgeMs,'CLOCK');
   for(const k of ['decisionSequence','decisionSourceNs','fillSequence','fillSourceNs'])need(typeof q[k]==='string'&&/^(0|[1-9][0-9]*)$/.test(q[k]),'VERSION');
   need(BigInt(q.fillSequence)>=BigInt(q.decisionSequence)&&BigInt(q.fillSourceNs)>=BigInt(q.decisionSourceNs)&&(q.fillSequence!==q.decisionSequence||q.fillSourceNs!==q.decisionSourceNs)&&BigInt(q.fillSourceNs)<=BigInt(f.at+2000)*1000000n,'VERSION');
   need(a&&a.decisionAt===q.decisionAt&&['ENTRY','TAKE_PROFIT','STOP_LOSS','HORIZON'].includes(a.reason),'ECONOMICS');
   const price=micros(f.price),qty=BigInt(f.quantity),quote=micros(a.quotePrice),slip=BigInt(card.scenario.slippageTicks)*5000n;
   need(price>=5000n&&price<=995000n&&price%5000n===0n&&price===quote+(f.action==='buy'?slip:-slip)&&micros(a.slippage)===slip*qty,'PRICE');turnover+=price*qty;
   const queue=lots.get(f.instrument)||[];lots.set(f.instrument,queue);
   if(f.action==='buy'){need(a.reason==='ENTRY'&&f.quantity===c.orderShares,'ENTRY');cash-=price*qty;basis+=price*qty;open+=f.quantity;queue.push({quantity:f.quantity,price});}
   else{need(a.reason!=='ENTRY','EXIT');cash+=price*qty;sold+=f.quantity;open-=f.quantity;let left=f.quantity;while(left){const lot=queue[0];need(lot,'OVERSELL');const n=Math.min(left,lot.quantity);realized+=(price-lot.price)*BigInt(n);basis-=lot.price*BigInt(n);left-=n;lot.quantity-=n;if(!lot.quantity)queue.shift();}if(!queue.length)closed++;}
   need(cash>=micros(c.cashReserve)&&basis<=micros(c.maxExposure)&&open>=0&&[...lots.values()].filter(x=>x.length).length<=1&&open<=c.maxPositionShares,'RISK');events.add(JSON.stringify([f.instrument,f.action,f.at,f.price,f.quantity]));
  }
  need(cash===micros(p.cash)&&realized===micros(p.realizedPnl)&&turnover===micros(p.turnover)&&open===p.openShares&&closed===p.completedTrades&&realized===micros(card.economics.netPnl)&&sold===card.economics.matchedShares&&micros(card.economics.fees)===0n,'RECONCILIATION');
  const openIds=new Set();for(const [instrument,queue]of lots)if(queue.length){held.add(members.get(instrument));openIds.add(instrument);}
  need(Array.isArray(card.pendingReadIntents)&&card.pendingReadIntents.length===p.pendingIntents,'INTENTS');const pendingIds=new Set();
  for(const row of card.pendingReadIntents){const at=Date.parse(row.decisionAt),earliest=Date.parse(row.minimumRequestStartedAt),expiry=Date.parse(row.expiresAt),limit=micros(row.limit);need(members.get(row.instrument)===row.marketId&&typeof row.intentId==='string'&&/^[1-9][0-9]*$/.test(row.intentId)&&BigInt(row.intentId)<=BigInt(p.intents)&&!used.has(row.intentId)&&!pendingIds.has(row.instrument),'INTENT');pendingIds.add(row.instrument);
   need(Number.isSafeInteger(at)&&at>start&&at<=through&&earliest===at+card.scenario.latencyMs&&expiry===at+c.maxDataAgeMs+c.cycleIntervalMs*4&&count(row.quantity)&&row.quantity>0&&limit>=5000n&&limit<=995000n&&limit%5000n===0n,'INTENT_CLOCK');
   need(row.action==='buy'&&!openIds.has(row.instrument)&&row.quantity===c.orderShares&&row.reason==='ENTRY'||row.action==='sell'&&openIds.has(row.instrument)&&row.quantity===(lots.get(row.instrument)||[]).reduce((n,l)=>n+l.quantity,0)&&['TAKE_PROFIT','STOP_LOSS','HORIZON'].includes(row.reason),'INTENT_QUANTITY');
   for(const k of ['decisionSequence','decisionSourceNs'])need(typeof row[k]==='string'&&/^(0|[1-9][0-9]*)$/.test(row[k]),'INTENT_VERSION');pending.push({...row,wallet:id});
  }
  cards.push({id,cash:p.cash,realizedPnl:p.realizedPnl,liquidationPnl:p.liquidationPnl,valuation:p.valuation,fills:card.fills.length,completedTrades:p.completedTrades,openShares:p.openShares,pendingIntents:p.pendingIntents,signalDiagnostics:card.signalDiagnostics});
 }
 return{schemaVersion:1,cohort:d.cohort,start:d.declaredAt,through:e.evaluationThrough,sources:cap.sources,candidates:cards.length,cards,heldMarketIds:[...held].sort(),pendingReadIntents:pending,uniqueFillEvents:events.size,fees:0,walletResultsAreCorrelated:true,executionAuthority:false,competitionOrders:0,networkRequests:0,profitabilityEstablished:false};
}
export function nativeQuoteReadFocus(p,now){return{heldMarketIds:p.heldMarketIds,pendingMarketIds:[...new Set(p.pendingReadIntents.filter(x=>Date.parse(x.minimumRequestStartedAt)<=now&&Date.parse(x.expiresAt)>=now).map(x=>x.marketId))].sort()};}
