// Independently reconcile frozen native paper output before it can request reads.
// This projection has no execution authority and never aggregates wallet P&L.
import {createHash} from 'node:crypto';
import {payloadHash} from '../execution/ledger.mjs';
const need=(ok,code)=>{if(!ok)throw Error('NATIVE_DIP_'+code);};
export const sha=b=>createHash('sha256').update(b).digest('hex');
const hash=x=>typeof x==='string'&&/^[a-f0-9]{64}$/.test(x);
export function micros(x){need(typeof x==='string'&&/^-?(?:0|[1-9][0-9]*)\.[0-9]{6}$/.test(x),'MONEY');const negative=x.startsWith('-'),[a,b]=(negative?x.slice(1):x).split('.');return (BigInt(a)*1000000n+BigInt(b))*(negative?-1n:1n);}
const count=x=>Number.isSafeInteger(x)&&x>=0;
const cardKey=c=>[c.strategy,c.scenario?.id,c.exitPolicy].join('/');
export function verifyNativeDipResult({result,declaration,binding,universe,previous=null}){
 const cap=result?.capture,e=result?.evaluation,cfg=declaration?.config,start=Date.parse(declaration?.declaredAt),through=Date.parse(e?.evaluationThrough);
 need(binding?.schemaVersion===1&&['binaryHash','engineHash','runtimeLibraryHash','declarationHash'].every(k=>hash(binding[k])),'BINDING');
 need(declaration?.schemaVersion===1&&declaration.mode==='causal_dip_subsequent_v2'&&declaration.cohort===binding.cohort&&cfg?.fillClockPolicy==='independent_request_latency_v2'&&cfg.feeBps===0&&Number.isSafeInteger(start),'DECLARATION');
 need(result.declarationFileHash===binding.declarationHash&&cap?.engine_hash===binding.engineHash&&hash(cap.source_hash)&&count(cap.source_records)&&count(cap.source_bytes)&&cap.source_bytes>0&&Number.isSafeInteger(cap.through)&&cap.through===through&&through>start,'CAPTURE');
 need(e?.mode===declaration.mode&&e.cohort===binding.cohort&&e.evaluationStart===declaration.declaredAt&&e.networkRequests===0&&e.competitionOrders===0&&e.profitabilityEstablished===false&&count(e.subsequentRecords)&&count(e.subsequentCycles),'OUTPUT');
 const expected=new Map();for(const s of declaration.strategies)for(const scenario of declaration.scenarios)for(const exitPolicy of ['snapshot_limit_v1','profit_floor_stop_buffer_v1'])expected.set([s.id,scenario.id,exitPolicy].join('/'),{strategy:s,scenario});
 need(Array.isArray(e.scorecards)&&e.scorecards.length===expected.size&&e.candidates===expected.size,'CARD_COUNT');
 const scope=new Map();for(const row of universe.exchanges){need(row.option==='YES'&&!scope.has(row.exchangeId),'UNIVERSE');scope.set(row.exchangeId,row.marketId);}
 const prior=new Map(previous?.evaluation.scorecards.map(c=>[cardKey(c),c])||[]),seen=new Set(),held=new Set(),events=new Set(),cards=[];
 if(previous)need(cap.source_bytes>=previous.capture.source_bytes&&cap.source_records>=previous.capture.source_records&&cap.through>=previous.capture.through,'SOURCE_REGRESSION');
 for(const card of e.scorecards){
  const key=cardKey(card),spec=expected.get(key),p=card.point,old=prior.get(key);
  need(spec&&!seen.has(key)&&payloadHash(card.parameters)===payloadHash(spec.strategy.parameters)&&payloadHash(card.scenario)===payloadHash(spec.scenario),'CARD_IDENTITY');seen.add(key);
  need(p?.at===e.evaluationThrough&&p.fillClockPolicy===cfg.fillClockPolicy&&Array.isArray(card.fills)&&p.fillExecutions===card.fills.length&&count(p.openShares)&&count(p.completedTrades)&&micros(p.fees)===0n,'POINT');
  if(old)need(card.fills.length>=old.fills.length&&old.fills.every((f,i)=>JSON.stringify(f)===JSON.stringify(card.fills[i])),'FILL_HISTORY_CHANGED');
  const lots=new Map();let cash=micros(cfg.initialCash),realized=0n,turnover=0n,sold=0,open=0,last=start;
  for(const f of card.fills){
   const q=f.sourceClock,a=f.economics,match=/^([1-9][0-9]*):YES$/.exec(f.instrument),market=match&&scope.get(match[1]);
   need(market&&cfg.marketIds.includes(market)&&['buy','sell'].includes(f.action)&&Number.isSafeInteger(f.quantity)&&f.quantity>0&&Number.isSafeInteger(f.at)&&f.at>start&&f.at>=last&&f.at<=through&&micros(f.fee)===0n,'FILL');last=f.at;
   need(q?.policy===cfg.fillClockPolicy&&[q.decisionAt,q.minimumLatencyMs,q.requestStartedAt,q.receivedAt].every(Number.isSafeInteger)&&q.minimumLatencyMs===card.scenario.latencyMs&&q.decisionAt>start&&q.requestStartedAt>=q.decisionAt+q.minimumLatencyMs&&q.receivedAt===f.at&&q.receivedAt>=q.requestStartedAt&&q.receivedAt-q.requestStartedAt<=cfg.maxDataAgeMs,'CLOCK');
   for(const k of ['decisionSequence','decisionSourceNs','fillSequence','fillSourceNs'])need(typeof q[k]==='string'&&/^(?:0|[1-9][0-9]*)$/.test(q[k]),'CLOCK_PRECISION');
   need(q.fillSequence!==q.decisionSequence||q.fillSourceNs!==q.decisionSourceNs,'CLOCK_VERSION');
   need(BigInt(q.fillSourceNs)<=BigInt(f.at+2000)*1000000n&&BigInt(q.fillSourceNs)>=BigInt(f.at-cfg.maxDataAgeMs)*1000000n,'SOURCE_CLOCK');
   need(a&&a.decisionAt===q.decisionAt&&typeof a.reason==='string','ECONOMICS');
   const price=micros(f.price),qty=BigInt(f.quantity);need(price>=5000n&&price<=995000n&&price%5000n===0n,'PRICE');turnover+=price*qty;
   const queue=lots.get(f.instrument)||[];lots.set(f.instrument,queue);
   if(f.action==='buy'){cash-=price*qty;queue.push({quantity:f.quantity,price});open+=f.quantity;}
   else{let left=f.quantity;cash+=price*qty;sold+=f.quantity;open-=f.quantity;while(left){const lot=queue[0];need(lot,'OVERSELL');const n=Math.min(left,lot.quantity);realized+=(price-lot.price)*BigInt(n);left-=n;lot.quantity-=n;if(!lot.quantity)queue.shift();}}
   need(cash>=0n&&open>=0,'WALLET');events.add(JSON.stringify([f.instrument,f.action,f.at,f.price,f.quantity]));
  }
  need(cash===micros(p.cash)&&realized===micros(p.realizedPnl)&&turnover===micros(p.turnover)&&open===p.openShares&&realized===micros(card.economics.netPnl)&&sold===card.economics.matchedShares&&micros(card.economics.fees)===0n,'RECONCILIATION');
  for(const [instrument,queue] of lots)if(queue.length)held.add(scope.get(instrument.split(':')[0]));
  cards.push({id:key,fills:card.fills.length,completedTrades:p.completedTrades,openShares:open,cash:p.cash,realizedPnl:p.realizedPnl,liquidationPnl:p.liquidationPnl,valuation:p.valuation,pendingIntents:p.pendingIntents});
 }
 const groups=new Map();for(const {id,...card} of cards){const key=JSON.stringify(card);if(!groups.has(key))groups.set(key,{wallets:0,...card});groups.get(key).wallets++;}
 return{schemaVersion:1,cohort:binding.cohort,declarationHash:binding.declarationHash,binaryHash:binding.binaryHash,engineHash:binding.engineHash,sourceHash:cap.source_hash,sourceBytes:cap.source_bytes,sourceRecords:cap.source_records,through:e.evaluationThrough,start:e.evaluationStart,candidates:cards.length,heldMarketIds:[...held].sort(),uniqueFillEvents:events.size,cardsWithFills:cards.filter(c=>c.fills).length,groups:[...groups.values()],fees:0,walletResultsAreCorrelated:true,executionAuthority:false,networkRequests:0,competitionOrders:0,profitabilityEstablished:false};
}
