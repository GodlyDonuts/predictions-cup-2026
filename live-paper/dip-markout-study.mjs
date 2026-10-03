import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {fileURLToPath} from 'node:url';
import {amount,money,TICK,instant} from '../paper/fixed.mjs';import {digest} from './model.mjs';
import {diagnoseQuotedDip} from './bulk-dip-diagnostic.mjs';import {BID_ANCHOR_POLICY} from './bid-anchor.mjs';
import {validateUniverse} from './breadth.mjs';import {validatePriceObservation} from './market-price-publication.mjs';
import {createBookPublicationState,applyBookPublication} from './book-publication.mjs';import {atomicJson} from '../execution/ledger.mjs';

export const DIP_MARKOUT_PROTOCOL=Object.freeze({schemaVersion:1,mode:'causal_quote_and_depth_markout_study',
 grossConcessions:['0.005000','0.010000','0.015000','0.020000'],latenciesMs:[1000,3000],
 cooldownMs:900000,horizonMs:300000,labelLatenessMs:60000,candidateLifetimeMs:60000,entryLifetimeMs:60000,
 referencePolicy:BID_ANCHOR_POLICY,feeBps:0,entryLimitPadTicks:1,depthFractionBps:100,orderShares:1,
 priceScenarios:['observed_depth_price','one_adverse_tick_each_side'],developmentPurgeMs:480000});
const need=(ok,reason)=>{if(!ok)throw Error('DIP_MARKOUT_'+reason);},sha=b=>createHash('sha256').update(b).digest('hex'),decimal=n=>money(BigInt(Math.round(n*1e6)));
const complete=b=>b.subarray(0,b.lastIndexOf(10)+1),parse=b=>b.toString().split('\n').slice(0,-1).map(JSON.parse);
const validQuote=q=>q.option==='YES'&&!q.crossed&&typeof q.bestBid==='number'&&typeof q.bestAsk==='number'&&q.bestBid>0&&q.bestAsk<1&&q.bestBid<q.bestAsk;
const identity=()=>Object.fromEntries(['./dip-markout-study.mjs','./bulk-dip-diagnostic.mjs','./bid-anchor.mjs','./model.mjs','./book-publication.mjs','./market-price-publication.mjs','./breadth.mjs','../paper/fixed.mjs'].map(p=>[p,sha(fs.readFileSync(new URL(p,import.meta.url)))]));
export function createMarkoutProtocol(cutoff,universe){instant(cutoff);return {name:'dip-markout-20261003-v1',cutoff,universeHash:validateUniverse(universe,universe.tournamentId).hash,
 protocol:DIP_MARKOUT_PROTOCOL,codeIdentity:identity()};}
export function verifyMarkoutProtocol(bank,universe){need(digest(bank.protocol)===digest(DIP_MARKOUT_PROTOCOL)&&bank.universeHash===validateUniverse(universe,universe.tournamentId).hash,'PROTOCOL');instant(bank.cutoff);
 need(digest(bank.codeIdentity)===digest(identity()),'FROZEN_CODE');return bank;}
function depthPrice(levels){const level=levels.find(x=>Math.floor(x.quantity/100)>=1);return level?amount(level.price):null;}
export function studyDipMarkouts({priceBytes,bookBytes,universe,bank}){
 verifyMarkoutProtocol(bank,universe);const u=validateUniverse(universe,universe.tournamentId),p=bank.protocol,
  pb=complete(priceBytes),bb=complete(bookBytes),prices=parse(pb),books=parse(bb);need(prices.length&&books.length,'EMPTY');
 let publication=createBookPublicationState(universe);for(const row of books)publication=applyBookPublication(publication,row);
 let sequence=0,previous='',lastPublished=-Infinity,lastReceipt=-Infinity;
 for(const row of prices){const {hash,...body}=row,snapshot=body.event?.snapshot;
  need(body.sequence===++sequence&&body.previous===previous&&hash===digest(body)&&body.state===null&&body.event?.type==='market_price_observation','PRICE_CHAIN');
  validatePriceObservation(snapshot,u);need(instant(body.at)>=instant(snapshot.receivedAt)&&instant(body.at)>=lastPublished&&instant(snapshot.receivedAt)>=lastReceipt,'PRICE_CLOCK');previous=hash;lastPublished=instant(body.at);lastReceipt=instant(snapshot.receivedAt);
 }
 const merged=[...prices.map(row=>({kind:'price',row})),...books.map(row=>({kind:'book',row}))]
  .sort((a,b)=>instant(a.row.at)-instant(b.row.at)||Number(a.kind==='book')-Number(b.kind==='book')||a.row.sequence-b.row.sequence);
 const history={},boundaries={global:null,markets:{}},cooldowns={},events=[],active={},marketByExchange=new Map(u.exchanges.map(x=>[x.exchangeId,x.marketId]));
 const counts={},inc=k=>counts[k]=(counts[k]||0)+1;
 const stopEvent=(e,status)=>{if(e.quote.status==='pending')e.quote.status=status;for(const trial of Object.values(e.depth))if(['awaiting_confirmation','awaiting_entry','awaiting_exit'].includes(trial.status))trial.status=status;};
 const invalidate=(id,at,status)=>{const ids=id===null?new Set([...Object.keys(history),...Object.keys(active)]):new Set([id]);for(const market of ids){delete history[market];for(const e of active[market]||[])stopEvent(e,status);active[market]=[];}if(id===null)boundaries.global=at;else boundaries.markets[id]=at;};
 function visitPrice(q,snapshot,row){
  const id=q.marketId,now=instant(snapshot.receivedAt),available=instant(row.at),permitted=[boundaries.global,boundaries.markets[id]].every(at=>at===null||at===undefined||instant(snapshot.requestStartedAt)>instant(at));
  if(!permitted){inc('price_request_before_boundary');return;}
  if(!validQuote(q)){invalidate(id,row.at,'invalid_quote');inc('invalid_quote');return;}
  for(const e of active[id]||[]){
   if(e.quote.status!=='pending')continue;
   const target=instant(e.availableAt)+p.horizonMs,request=instant(snapshot.requestStartedAt);
   if(now-instant(e.quote.lastReceiptAt)>p.referencePolicy.maximumGapMs){e.quote.status='coverage_gap';continue;}
   e.quote.lastReceiptAt=snapshot.receivedAt;
   if(request<target)continue;
   if(now>target+p.labelLatenessMs){e.quote.status='late_quote';continue;}
   e.quote={...e.quote,status:'observed',outcomeReceivedAt:snapshot.receivedAt,outcomeRequestStartedAt:snapshot.requestStartedAt,
    exitBid:decimal(q.bestBid),rawMarkout:money(amount(decimal(q.bestBid))-amount(e.quote.entryAsk)),
    adverseMarkout:money(amount(decimal(q.bestBid))-amount(e.quote.entryAsk)-2n*TICK),outcomeSequence:row.sequence,outcomeHash:row.hash};
  }
  const points=history[id]||[],gate=diagnoseQuotedDip(points,q,snapshot.receivedAt,p.referencePolicy);
  if(gate.features&&gate.failures.every(reason=>reason==='insufficient_concession')){
   const gross=amount(gate.features.netConcession)+2n*TICK;
   const thresholds=p.grossConcessions.filter(threshold=>gross>=amount(threshold)&&available>=(cooldowns[threshold+':'+id]||0));
   if(thresholds.length){
    const phase=available>=instant(bank.cutoff)?'prospective':available+p.developmentPurgeMs<=instant(bank.cutoff)?'development':'purged_boundary';
    const e={id:id+':'+row.sequence,marketId:id,exchangeId:q.exchangeId,availableAt:row.at,receivedAt:snapshot.receivedAt,
     priceSequence:row.sequence,priceHash:row.hash,phase,thresholds,reference:gate.features,rawConcession:money(gross),
     quote:{status:'pending',entryAsk:decimal(q.bestAsk),lastReceiptAt:snapshot.receivedAt},
     depth:Object.fromEntries(p.latenciesMs.map(latency=>[latency,{status:'awaiting_confirmation',latencyMs:latency,maximumAsk:money(amount(decimal(q.bestAsk))+TICK),expiresAt:new Date(available+p.candidateLifetimeMs).toISOString()}]))};
    events.push(e);(active[id]??=[]).push(e);for(const threshold of thresholds)cooldowns[threshold+':'+id]=available+p.cooldownMs;inc('dip_events');
   }
  }
  const minute=Math.floor(now/60000);if(!points.some(x=>x.minute===minute))points.push({minute,at:snapshot.receivedAt,bid:decimal(q.bestBid)});
  history[id]=points.filter(x=>instant(x.at)>=now-p.referencePolicy.historyMs);
 }
 function visitBook(row){
  if(['session_start','session_stop'].includes(row.kind)||row.kind==='invalidation'){invalidate(row.kind==='invalidation'?row.payload.marketId:null,row.at,'source_boundary');inc('book_boundaries');return;}
  if(row.kind!=='book')return;const b=row.payload.book,at=instant(row.at),request=instant(b.requestStartedAt),received=instant(b.receivedAt);
  for(const e of active[b.marketId]||[])for(const trial of Object.values(e.depth)){
   if(trial.status==='awaiting_confirmation'){
    if(at>instant(trial.expiresAt)){trial.status='confirmation_not_observed';continue;}
    if(request<=instant(e.availableAt))continue;
    const ask=depthPrice(b.asks),bid=depthPrice(b.bids);
    if(ask===null||bid===null){inc('confirmation_depth_unavailable');continue;}
    if(ask>amount(trial.maximumAsk)||amount(b.asks[0].price)-amount(b.bids[0].price)>amount(p.referencePolicy.maximumSpread)){inc('confirmation_price_rejected');continue;}
    trial.status='awaiting_entry';trial.preparedAt=row.at;trial.preparedVersion=b.versionKey;trial.preparationSequence=row.sequence;trial.preparationHash=row.hash;
    trial.limit=money(ask+TICK);trial.expiresAt=new Date(at+p.entryLifetimeMs).toISOString();continue;
   }
   if(trial.status==='awaiting_entry'){
    if(at>instant(trial.expiresAt)){trial.status='entry_not_observed';continue;}
    if(request<instant(trial.preparedAt)+trial.latencyMs||b.versionKey===trial.preparedVersion)continue;
    const ask=depthPrice(b.asks);if(ask===null){inc('entry_depth_unavailable');continue;}
    if(ask+TICK>amount(trial.limit)){inc('entry_price_rejected');continue;}
    trial.status='awaiting_exit';trial.entryRawPrice=money(ask);trial.entryAdversePrice=money(ask+TICK);trial.entryReceivedAt=b.receivedAt;trial.entryRequestStartedAt=b.requestStartedAt;
    trial.entryAvailableAt=row.at;trial.entryVersion=b.versionKey;trial.entrySequence=row.sequence;trial.entryHash=row.hash;
    trial.dueAt=new Date(at+p.horizonMs).toISOString();trial.expiresAt=new Date(at+p.horizonMs+p.labelLatenessMs).toISOString();continue;
   }
   if(trial.status==='awaiting_exit'){
    if(at>instant(trial.expiresAt)){trial.status='exit_not_observed';continue;}
    if(request<instant(trial.dueAt)||b.versionKey===trial.entryVersion)continue;
    const bid=depthPrice(b.bids);if(bid===null){inc('exit_depth_unavailable');continue;}
    trial.status='observed';trial.exitRawPrice=money(bid);trial.exitAdversePrice=money(bid-TICK);trial.exitReceivedAt=b.receivedAt;trial.exitRequestStartedAt=b.requestStartedAt;
    trial.exitSequence=row.sequence;trial.exitHash=row.hash;trial.rawMarkout=money(bid-amount(trial.entryRawPrice));trial.adverseMarkout=money(bid-TICK-amount(trial.entryAdversePrice));
   }
  }
 }
 for(const {kind,row}of merged){if(kind==='book')visitBook(row);else{for(const id of row.event.snapshot.missingIds)invalidate(marketByExchange.get(id),row.at,'missing_quote');for(const q of row.event.snapshot.quotes)visitPrice(q,row.event.snapshot,row);}}
 const through=merged.at(-1).row.at;
 for(const e of events){
  if(e.quote.status==='pending')e.quote.status=instant(through)>instant(e.availableAt)+p.horizonMs+p.labelLatenessMs?'outcome_not_observed':'right_censored';
  for(const t of Object.values(e.depth))if(t.status.startsWith('awaiting_'))t.status=instant(through)>instant(t.expiresAt)?t.status.replace('awaiting_','')+'_not_observed':'right_censored';
 }
 const metrics=series=>{const statuses={},done=series.filter(x=>x.status==='observed');for(const s of series)statuses[s.status]=(statuses[s.status]||0)+1;
  const measure=key=>{const values=done.map(x=>amount(x[key])).sort((a,b)=>a<b?-1:a>b?1:0),total=values.reduce((a,b)=>a+b,0n);return {count:values.length,positive:values.filter(x=>x>0n).length,negative:values.filter(x=>x<0n).length,zero:values.filter(x=>x===0n).length,
   average:values.length?money(total/BigInt(values.length)):null,median:values.length?money(values[Math.floor((values.length-1)/2)]):null,minimum:values.length?money(values[0]):null,maximum:values.length?money(values.at(-1)):null};};
  return {events:series.length,statuses,observedPrices:measure('rawMarkout'),adverseTicks:measure('adverseMarkout')};};
 const groups={};for(const phase of ['development','purged_boundary','prospective'])groups[phase]=Object.fromEntries(p.grossConcessions.map(threshold=>{
  const chosen=events.filter(e=>e.phase===phase&&e.thresholds.includes(threshold));return [threshold,{events:chosen.length,markets:new Set(chosen.map(x=>x.marketId)).size,quoteMarkouts:metrics(chosen.map(x=>x.quote)),depthMarkouts:Object.fromEntries(p.latenciesMs.map(latency=>[latency,metrics(chosen.map(x=>x.depth[latency]))]))}];}));
 return {mode:'causal_quote_and_depth_markout_study',createdAt:new Date().toISOString(),bankHash:digest(bank),cutoff:bank.cutoff,protocol:p,
  sources:{price:{bytes:pb.length,sha256:sha(pb),sequence,hash:previous,incompleteTailBytes:priceBytes.length-pb.length},book:{bytes:bb.length,sha256:sha(bb),sequence:publication.sequence,hash:publication.hash,incompleteTailBytes:bookBytes.length-bb.length}},through,counts,groups,events,
  networkRequests:0,competitionOrders:0,paperFills:0,profitabilityEstablished:false,
  limitations:['Depth pairs are historical causal quote diagnostics, not actual orders, fills, wallet P&L or guaranteed execution.',
   'Bulk quotes have unknown source age/size; source publication is hypothetical availability, not a live consumer receipt.',
   'Missing, revoked and right-censored paths remain in denominators. Complete-path averages may be selected and are not profitability estimates.',
   'Thresholds and latency paths overlap and market/common-event observations correlate; no independent sample or winner claim.']};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const args=process.argv.slice(2),option=n=>{const i=args.indexOf(n);return i<0?null:args[i+1];},universe=JSON.parse(fs.readFileSync(new URL('./price-universe.json',import.meta.url))),protocolFile=option('--protocol'),output=option('--output');need(protocolFile,'PROTOCOL_FILE');
 if(args.includes('--initialize')){const bank=createMarkoutProtocol(new Date().toISOString(),universe);fs.writeFileSync(protocolFile,JSON.stringify(bank,null,2)+'\n',{flag:'wx',mode:0o600});console.log(JSON.stringify({cutoff:bank.cutoff,bankHash:digest(bank),protocolFile}));}
 else{need(output,'OUTPUT');const bank=JSON.parse(fs.readFileSync(protocolFile)),runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime');
  const result=studyDipMarkouts({priceBytes:fs.readFileSync(path.join(runtime,'market-prices.jsonl')),bookBytes:fs.readFileSync(path.join(runtime,'public-books.jsonl')),universe,bank});
  atomicJson(path.resolve(output),result);console.log(JSON.stringify({output,cutoff:result.cutoff,through:result.through,counts:result.counts,groups:result.groups,sources:result.sources},null,2));
 }
}
