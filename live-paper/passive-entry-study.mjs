import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {amount,money,TICK,instant} from '../paper/fixed.mjs';
import {digest} from './model.mjs';
import {atomicJson} from '../execution/ledger.mjs';
import {studyDipMarkouts,verifyMarkoutProtocol} from './dip-markout-study.mjs';

export const PASSIVE_ENTRY_POLICY=Object.freeze({schemaVersion:1,dipThreshold:'0.010000',
 strategies:['cross_ask','rest_bid','improve_bid'],latenciesMs:[1000,3000],horizonsMs:[60000,180000,300000],
 preparationLifetimeMs:60000,admissionLifetimeMs:60000,restLifetimeMs:120000,labelLatenessMs:60000,
 maximumRequestDurationMs:20000,maximumPublicationLagMs:20000,maximumSpread:'0.020000',
 strictCrossingTicks:1,crossingConfirmations:2,maximumCrossingGapMs:30000,
 depthFractionBps:100,orderShares:1,entryCapPadTicks:1,takerAdverseTicks:1,exitAdverseTicks:1,
 feeBps:0,developmentPurgeMs:660000,queuePolicy:'two_independent_price_through_observations_not_cancellations'});
const need=(ok,code)=>{if(!ok)throw Error('PASSIVE_STUDY_'+code);};
const sha=b=>createHash('sha256').update(b).digest('hex'),decimal=n=>money(BigInt(Math.round(n*1e6)));
const rows=b=>b.subarray(0,b.lastIndexOf(10)+1).toString().split('\n').slice(0,-1).map(JSON.parse);
const identity=()=>Object.fromEntries(['./passive-entry-study.mjs','./dip-markout-study.mjs','./model.mjs','../paper/fixed.mjs','../execution/ledger.mjs'].map(p=>[p,sha(fs.readFileSync(new URL(p,import.meta.url)))]));
export function createPassiveStudyBank(cutoff,baseBank){instant(cutoff);return {name:'passive-entry-path-20261003-v1',cutoff,baseBank,protocol:PASSIVE_ENTRY_POLICY,codeIdentity:identity()};}

// These are conditional depth paths, not a trade-tape reconstruction. A quote
// touch or shrinking displayed queue alone can never simulate a maker fill.
export function comparePassivePaths({events,prices,books,cutoff,policy=PASSIVE_ENTRY_POLICY}){
 instant(cutoff);need(digest(policy)===digest(PASSIVE_ENTRY_POLICY),'POLICY');
 const cases=[],active={},barriers={global:null,markets:{}},bySequence=new Map();
 for(const e of events.filter(e=>e.thresholds.includes(policy.dipThreshold))){const list=bySequence.get(e.priceSequence)||[];list.push(e);bySequence.set(e.priceSequence,list);}
 const trial=availableAt=>({stage:'awaiting_preparation',expiresAt:new Date(instant(availableAt)+policy.preparationLifetimeMs).toISOString()});
 const endExpired=(cell,now)=>{
  if(cell.stage.startsWith('awaiting_')&&now>instant(cell.expiresAt))cell.stage=cell.stage.replace('awaiting_','')+'_not_observed';
  else if(cell.stage==='resting'&&now>instant(cell.expiresAt))cell.stage='price_through_not_observed';
  else if(cell.stage==='entered')for(const x of Object.values(cell.exits))if(x.status==='awaiting_outcome'&&now>instant(x.expiresAt))x.status='outcome_not_observed';
 };
 const revoke=(id,at,reason)=>{
  if(id===null)barriers.global=at;else barriers.markets[id]=at;
  for(const c of id===null?cases:active[id]||[])for(const trials of Object.values(c.strategies))for(const cell of Object.values(trials)){
   endExpired(cell,instant(at));
   if(cell.stage.startsWith('awaiting_')||cell.stage==='resting')cell.stage=reason;
   else if(cell.stage==='entered')for(const x of Object.values(cell.exits))if(x.status==='awaiting_outcome')x.status=reason;
  }
 };
 const enough=level=>Math.floor(level.quantity*policy.depthFractionBps/10000)>=policy.orderShares;
 const buyLevel=(levels,limit,adverse=0)=>levels.find(x=>enough(x)&&amount(x.price)+BigInt(adverse)*TICK<=amount(limit));
 const sellLevel=levels=>levels.find(enough);
 const provenance=(b,row)=>({availableAt:row.at,requestStartedAt:b.requestStartedAt,receivedAt:b.receivedAt,versionKey:b.versionKey,sourceSequence:row.sequence,sourceHash:row.hash});
 const enter=(cell,value,b,row,latency)=>{
  cell.stage='entered';cell.entryPrice=value;cell.entry=provenance(b,row);
  cell.exits=Object.fromEntries(policy.horizonsMs.map(h=>{const due=instant(row.at)+h+latency;return[h,{status:'awaiting_outcome',dueAt:new Date(due).toISOString(),expiresAt:new Date(due+policy.labelLatenessMs).toISOString()}];}));
 };
 function bookTrial(c,strategy,latency,cell,b,row){
  const now=instant(row.at),request=instant(b.requestStartedAt),topAsk=amount(b.asks[0].price),topBid=amount(b.bids[0].price);
  const spread=topAsk-topBid;
  if(cell.stage==='awaiting_preparation'){
   if(request<=instant(c.availableAt)||spread>amount(policy.maximumSpread))return;
   const ask=buyLevel(b.asks,c.maximumCost,policy.takerAdverseTicks),bid=sellLevel(b.bids);
   if(!ask||!bid)return;
   if(strategy==='improve_bid'&&topBid+TICK>=topAsk){cell.stage='spread_no_improvement';return;}
   cell.limit=strategy==='cross_ask'?c.maximumCost:money(topBid+(strategy==='improve_bid'?TICK:0n));
   cell.preparation=provenance(b,row);cell.stage='awaiting_admission';
   cell.expiresAt=new Date(now+policy.admissionLifetimeMs).toISOString();return;
  }
  if(cell.stage==='awaiting_admission'){
   if(request<instant(cell.preparation.availableAt)+latency||b.versionKey===cell.preparation.versionKey)return;
   if(spread>amount(policy.maximumSpread))return;
   if(strategy==='cross_ask'){
    const ask=buyLevel(b.asks,cell.limit,policy.takerAdverseTicks);
    if(ask)enter(cell,money(amount(ask.price)+BigInt(policy.takerAdverseTicks)*TICK),b,row,latency);
    return;
   }
   if(topAsk<=amount(cell.limit)){cell.stage='post_only_would_cross';return;}
   cell.admission=provenance(b,row);
   cell.displayedAhead=b.bids.filter(x=>amount(x.price)>=amount(cell.limit)).reduce((n,x)=>n+x.quantity,0);
   cell.stage='resting';cell.expiresAt=new Date(now+policy.restLifetimeMs).toISOString();cell.crossing=null;return;
  }
  if(cell.stage==='resting'){
   if(request<=instant(cell.admission.availableAt)||b.versionKey===cell.admission.versionKey)return;
   const crossed=buyLevel(b.asks,money(amount(cell.limit)-BigInt(policy.strictCrossingTicks)*TICK));
   if(!crossed){cell.crossing=null;return;}
   if(!cell.crossing||now-instant(cell.crossing.availableAt)>policy.maximumCrossingGapMs){cell.crossing=provenance(b,row);return;}
   if(request<=instant(cell.crossing.availableAt)||b.versionKey===cell.crossing.versionKey)return;
   // Use the original resting price, never the cheaper hindsight ask.
   cell.crossingConfirmation=provenance(b,row);enter(cell,cell.limit,b,row,latency);return;
  }
  if(cell.stage!=='entered')return;
  for(const exit of Object.values(cell.exits)){
   if(exit.status!=='awaiting_outcome'||request<instant(exit.dueAt)||b.versionKey===cell.entry.versionKey)continue;
   const bid=sellLevel(b.bids);if(!bid)continue;
   const value=amount(bid.price)-BigInt(policy.exitAdverseTicks)*TICK;
   if(value<=0n){exit.status='invalid_exit_price';continue;}
   exit.status='observed';exit.exitPrice=money(value);exit.netMarkout=money(value-amount(cell.entryPrice));exit.source=provenance(b,row);
  }
 }
 const merged=[...prices.map(row=>({kind:'price',row})),...books.map(row=>({kind:'book',row}))].sort((a,b)=>instant(a.row.at)-instant(b.row.at)||Number(a.kind==='book')-Number(b.kind==='book')||a.row.sequence-b.row.sequence);
 need(merged.length,'EMPTY');
 for(const {kind,row}of merged){
  for(const c of cases)for(const trials of Object.values(c.strategies))for(const cell of Object.values(trials))endExpired(cell,instant(row.at));
  if(kind==='book'){
   if(['session_start','session_stop','invalidation'].includes(row.kind)){revoke(row.kind==='invalidation'?row.payload.marketId:null,row.at,'source_boundary');continue;}
   if(row.kind!=='book')continue;const b=row.payload.book;
   need(instant(b.requestStartedAt)<=instant(b.receivedAt)&&instant(b.receivedAt)<=instant(row.at),'BOOK_CLOCK');
   if(instant(b.receivedAt)-instant(b.requestStartedAt)>policy.maximumRequestDurationMs||instant(row.at)-instant(b.receivedAt)>policy.maximumPublicationLagMs){revoke(b.marketId,row.at,'stale_book');continue;}
   if(![barriers.global,barriers.markets[b.marketId]].every(at=>at==null||instant(b.requestStartedAt)>instant(at)))continue;
   if(!b.asks.length||!b.bids.length||amount(b.asks[0].price)<=amount(b.bids[0].price)){revoke(b.marketId,row.at,'invalid_book');continue;}
   for(const c of active[b.marketId]||[]){need(b.exchangeId===c.exchangeId,'BOOK_SCOPE');for(const [strategy,trials]of Object.entries(c.strategies))for(const [latency,cell]of Object.entries(trials))bookTrial(c,strategy,Number(latency),cell,b,row);}continue;
  }
  const snapshot=row.event.snapshot;
  need(instant(snapshot.requestStartedAt)<=instant(snapshot.receivedAt)&&instant(snapshot.receivedAt)<=instant(row.at),'PRICE_CLOCK');
  const stale=instant(row.at)-instant(snapshot.receivedAt)>policy.maximumPublicationLagMs;
  for(const id of snapshot.missingIds)for(const c of cases.filter(c=>c.exchangeId===id))revoke(c.marketId,row.at,'missing_quote');
  for(const q of snapshot.quotes){
   if(stale)revoke(q.marketId,row.at,'stale_price');
   else if(q.option!=='YES'||q.crossed||typeof q.bestBid!=='number'||typeof q.bestAsk!=='number'||q.bestBid<=0||q.bestAsk>=1||q.bestAsk<=q.bestBid)revoke(q.marketId,row.at,'invalid_quote');
  }
  for(const e of bySequence.get(row.sequence)||[]){
   const q=snapshot.quotes.find(q=>q.marketId===e.marketId&&q.exchangeId===e.exchangeId);
   need(q&&e.priceHash===row.hash&&e.availableAt===row.at&&e.receivedAt===snapshot.receivedAt&&e.quote.entryAsk===decimal(q.bestAsk),'OPPORTUNITY_SOURCE');
   const phase=instant(e.availableAt)>=instant(cutoff)?'prospective':instant(e.availableAt)+policy.developmentPurgeMs<=instant(cutoff)?'development':'purged_boundary';
   const c={id:e.id,marketId:e.marketId,exchangeId:e.exchangeId,availableAt:e.availableAt,phase,maximumCost:money(amount(e.quote.entryAsk)+BigInt(policy.entryCapPadTicks)*TICK),
    strategies:Object.fromEntries(policy.strategies.map(name=>[name,Object.fromEntries(policy.latenciesMs.map(latency=>[latency,trial(e.availableAt)]))]))};
   cases.push(c);(active[c.marketId]??=[]).push(c);
   if(stale)revoke(c.marketId,row.at,'stale_price');
   else if(![barriers.global,barriers.markets[c.marketId]].every(at=>at==null||instant(snapshot.requestStartedAt)>instant(at)))revoke(c.marketId,row.at,'source_boundary');
  }
 }
 const through=merged.at(-1).row.at;
 for(const c of cases)for(const trials of Object.values(c.strategies))for(const cell of Object.values(trials)){
  endExpired(cell,instant(through));if(cell.stage.startsWith('awaiting_')||cell.stage==='resting')cell.stage='right_censored';
  else if(cell.stage==='entered')for(const x of Object.values(cell.exits))if(x.status==='awaiting_outcome')x.status='right_censored';
 }
 const metrics=items=>{const statuses={},done=items.filter(x=>x.status==='observed'),values=done.map(x=>amount(x.netMarkout)),total=values.reduce((a,b)=>a+b,0n);
  for(const x of items)statuses[x.status]=(statuses[x.status]||0)+1;
  return {opportunities:items.length,statuses,completedPaths:values.length,positive:values.filter(x=>x>0n).length,negative:values.filter(x=>x<0n).length,zero:values.filter(x=>x===0n).length,meanNetMarkout:values.length?money(total/BigInt(values.length)):null};};
 const groups=Object.fromEntries(['development','purged_boundary','prospective'].map(phase=>{const chosen=cases.filter(c=>c.phase===phase);return[phase,{opportunities:chosen.length,markets:new Set(chosen.map(c=>c.marketId)).size,
  strategies:Object.fromEntries(policy.strategies.map(strategy=>[strategy,Object.fromEntries(policy.latenciesMs.map(latency=>[latency,{entries:chosen.filter(c=>c.strategies[strategy][latency].stage==='entered').length,
   outcomes:Object.fromEntries(policy.horizonsMs.map(h=>[h,metrics(chosen.map(c=>{const t=c.strategies[strategy][latency];return t.stage==='entered'?t.exits[h]:{status:t.stage};}))]))}]))]))}];}));
 return {through,cases,groups,networkRequests:0,competitionOrders:0,paperFills:0,profitabilityEstablished:false};
}

export function runPassiveStudy({priceBytes,bookBytes,universe,bank}){
 need(digest(bank.protocol)===digest(PASSIVE_ENTRY_POLICY)&&digest(bank.codeIdentity)===digest(identity()),'FROZEN_PROTOCOL');verifyMarkoutProtocol(bank.baseBank,universe);
 const original=studyDipMarkouts({priceBytes,bookBytes,universe,bank:bank.baseBank});
 return {...comparePassivePaths({events:original.events,prices:rows(priceBytes),books:rows(bookBytes),cutoff:bank.cutoff}),createdAt:new Date().toISOString(),mode:'conservative_passive_depth_path_comparison',bankHash:digest(bank),cutoff:bank.cutoff,protocol:bank.protocol,sources:original.sources,
  limitations:['Maker entries are conditional local depth scenarios. Snapshots cannot prove transaction flow, queue clearance, fill probability or counterfactual competitor response.',
   'Resting entry requires two later independent price-through observations with one-percent size. Quote touches and displayed queue cancellations never create entries.',
   'Taker and exits include one adverse tick; maker cost is its immutable limit. All added fees are zero.',
   'Missing and censored opportunities stay in denominators. Completed averages are selected, markets/latencies/horizons correlate, and paths are not portfolio returns.',
   'Source publication models availability, not live receipt. No platform orders or paper-wallet fills are asserted.']};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const args=process.argv.slice(2),option=n=>{const i=args.indexOf(n);return i<0?null:args[i+1];},file=option('--protocol');need(file,'PROTOCOL_FILE');
 const universe=JSON.parse(fs.readFileSync(new URL('./price-universe.json',import.meta.url)));
 if(args.includes('--initialize')){need(option('--base-protocol'),'BASE_PROTOCOL');const baseBank=JSON.parse(fs.readFileSync(option('--base-protocol')));verifyMarkoutProtocol(baseBank,universe);
  const bank=createPassiveStudyBank(new Date().toISOString(),baseBank);fs.writeFileSync(file,JSON.stringify(bank,null,2)+'\n',{flag:'wx',mode:0o600});console.log(JSON.stringify({cutoff:bank.cutoff,bankHash:digest(bank)}));
 }else{const output=option('--output');need(output,'OUTPUT');const dir=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),bank=JSON.parse(fs.readFileSync(file));
  const result=runPassiveStudy({priceBytes:fs.readFileSync(path.join(dir,'market-prices.jsonl')),bookBytes:fs.readFileSync(path.join(dir,'public-books.jsonl')),universe,bank});atomicJson(path.resolve(output),result);
  console.log(JSON.stringify({output,cutoff:result.cutoff,through:result.through,groups:result.groups,sources:result.sources}));
 }
}
