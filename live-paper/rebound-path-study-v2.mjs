import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {fileURLToPath} from 'node:url';
import {amount,money,TICK,instant} from '../paper/fixed.mjs';import {digest} from './model.mjs';import {atomicJson} from '../execution/ledger.mjs';
import {studyDipMarkouts,verifyMarkoutProtocol} from './dip-markout-study.mjs';
import {REBOUND_ENTRY_POLICY,createReboundWatch,advanceReboundWatch} from './rebound-entry.mjs';
export const REBOUND_STUDY_POLICY=Object.freeze({schemaVersion:2,terminalReasons:'latched_after_deadline_expiry',dipThreshold:'0.010000',latenciesMs:[1000,3000],horizonsMs:[60000,180000,300000],
 signal:REBOUND_ENTRY_POLICY,confirmationLifetimeMs:60000,entryLifetimeMs:60000,labelLatenessMs:60000,maximumSpread:'0.020000',
 depthFractionBps:100,orderShares:1,feeBps:0,priceScenarios:['observed_price','one_adverse_tick_each_side'],developmentPurgeMs:660000});
const need=(ok,code)=>{if(!ok)throw Error('REBOUND_STUDY_'+code);},sha=bytes=>createHash('sha256').update(bytes).digest('hex'),price=n=>money(BigInt(Math.round(n*1e6)));
const complete=b=>b.subarray(0,b.lastIndexOf(10)+1),rows=b=>complete(b).toString().split('\n').slice(0,-1).map(JSON.parse);
const identity=()=>Object.fromEntries(['./rebound-entry.mjs','./rebound-path-study-v2.mjs','./dip-markout-study.mjs','../execution/ledger.mjs'].map(p=>[p,sha(fs.readFileSync(new URL(p,import.meta.url)))]));
export function createReboundStudyBank(cutoff,baseBank){instant(cutoff);return {name:'confirmed-rebound-path-20261003-v2',cutoff,baseBank,protocol:REBOUND_STUDY_POLICY,codeIdentity:identity()};}
const valid=q=>q.option==='YES'&&!q.crossed&&typeof q.bestBid==='number'&&typeof q.bestAsk==='number'&&q.bestBid>0&&q.bestAsk<1&&q.bestBid<q.bestAsk;
const depth=(levels,action,limit=null)=>{const x=levels.find(x=>Math.floor(x.quantity/100)>=1&&(limit===null||action==='buy'&&amount(x.price)<=amount(limit)||action==='sell'&&amount(x.price)>=amount(limit)));return x?x.price:null;};

// The caller supplies original causal dip opportunities, never filtered by
// their future result. Entries and outcomes below are quote diagnostics.
export function compareReboundPaths({events,prices,books,cutoff,policy=REBOUND_STUDY_POLICY}){
 instant(cutoff);need(digest(policy)===digest(REBOUND_STUDY_POLICY),'POLICY');
 const cases=[],active={},barriers={global:null,markets:{}},bySequence=new Map();
 for(const e of events.filter(e=>e.thresholds.includes(policy.dipThreshold))){const list=bySequence.get(e.priceSequence)||[];list.push(e);bySequence.set(e.priceSequence,list);}
 const makeBranch=signal=>({signal,trials:Object.fromEntries(policy.latenciesMs.map(latency=>[latency,{latencyMs:latency,
  quote:{stage:'awaiting_entry',expiresAt:new Date(instant(signal.availableAt)+policy.entryLifetimeMs).toISOString()},
  depth:{stage:'awaiting_confirmation',expiresAt:new Date(instant(signal.availableAt)+policy.confirmationLifetimeMs).toISOString()}}]))});
 const enter=(trial,entryPrice,at,received,version=null)=>{trial.stage='entered';trial.entryPrice=entryPrice;trial.entryAvailableAt=at;trial.entryReceivedAt=received;trial.entryVersion=version;
  trial.exits=Object.fromEntries(policy.horizonsMs.map(h=>{const due=instant(at)+h+trial.latencyMs;return [h,{status:'awaiting_outcome',dueAt:new Date(due).toISOString(),expiresAt:new Date(due+policy.labelLatenessMs).toISOString(),lastReceivedAt:received}];}));};
 // Expiry is a clock event. A later interruption must not replace the reason
 // an already missed entry or outcome ended, even without an intervening quote.
 const revoke=(c,reason,at)=>{
  const now=instant(at);
  if(c.watch.status==='watching')c.watch.status=now>instant(c.watch.expiresAt)?'confirmation_not_observed':reason;
  for(const branch of Object.values(c.branches))if(branch)for(const t of Object.values(branch.trials))for(const kind of ['quote','depth']){
   const cell=t[kind];
   if(cell.stage.startsWith('awaiting_'))cell.stage=now>instant(cell.expiresAt)?cell.stage.replace('awaiting_','')+'_not_observed':reason;
   else if(cell.stage==='entered')for(const exit of Object.values(cell.exits))if(exit.status==='awaiting_outcome')exit.status=now>instant(exit.expiresAt)?'outcome_not_observed':reason;
  }
 };
 const boundary=(id,at,reason)=>{if(id===null)barriers.global=at;else barriers.markets[id]=at;for(const market of id===null?Object.keys(active):[id])for(const c of active[market]||[])revoke(c,reason,at);};
 function label(cell,exit,value,row,received,request){exit.status='observed';exit.exitPrice=value;exit.rawMarkout=money(amount(value)-amount(cell.entryPrice));exit.adverseMarkout=money(amount(exit.rawMarkout)-2n*TICK);exit.sourceSequence=row.sequence;exit.sourceHash=row.hash;exit.availableAt=row.at;exit.receivedAt=received;exit.requestStartedAt=request;}
 function quoteTrial(branch,t,q,snapshot,row){const cell=t.quote,now=instant(row.at),request=instant(snapshot.requestStartedAt);
  if(cell.stage==='awaiting_entry'){
   if(now>instant(cell.expiresAt)){cell.stage='entry_not_observed';return;}
   if(request<instant(branch.signal.availableAt)+t.latencyMs||amount(price(q.bestAsk))>amount(branch.signal.limit)
    ||amount(price(q.bestAsk))-amount(price(q.bestBid))>amount(policy.maximumSpread)
    ||branch.signal.troughBid&&amount(price(q.bestBid))<amount(branch.signal.troughBid)+TICK)return;
   cell.latencyMs=t.latencyMs;enter(cell,price(q.bestAsk),row.at,snapshot.receivedAt,row.sequence);return;
  }
  if(cell.stage!=='entered')return;
  for(const exit of Object.values(cell.exits)){if(exit.status!=='awaiting_outcome')continue;
   if(instant(snapshot.receivedAt)-instant(exit.lastReceivedAt)>policy.signal.maximumQuoteGapMs){exit.status='coverage_gap';continue;}
   exit.lastReceivedAt=snapshot.receivedAt;
   if(now>instant(exit.expiresAt)){exit.status='outcome_not_observed';continue;}
   if(request<instant(exit.dueAt))continue;
   label(cell,exit,price(q.bestBid),row,snapshot.receivedAt,snapshot.requestStartedAt);
  }
 }
 function bookTrial(branch,t,b,row){const cell=t.depth,now=instant(row.at),request=instant(b.requestStartedAt);
  if(cell.stage==='awaiting_confirmation'){
   if(now>instant(cell.expiresAt)){cell.stage='confirmation_not_observed';return;}
   if(request<=instant(branch.signal.availableAt))return;
   const ask=depth(b.asks,'buy',branch.signal.limit),bid=depth(b.bids,'sell');
   if(ask===null||bid===null||amount(b.asks[0].price)-amount(b.bids[0].price)>amount(policy.maximumSpread)
    ||branch.signal.troughBid&&amount(bid)<amount(branch.signal.troughBid)+TICK)return;
   cell.stage='awaiting_entry';cell.preparedAt=row.at;cell.preparedVersion=b.versionKey;cell.expiresAt=new Date(now+policy.entryLifetimeMs).toISOString();return;
  }
  if(cell.stage==='awaiting_entry'){
   if(now>instant(cell.expiresAt)){cell.stage='entry_not_observed';return;}
   if(request<instant(cell.preparedAt)+t.latencyMs||b.versionKey===cell.preparedVersion)return;
   const ask=depth(b.asks,'buy',branch.signal.limit),bid=depth(b.bids,'sell');
   if(ask===null||bid===null||amount(b.asks[0].price)-amount(b.bids[0].price)>amount(policy.maximumSpread)
    ||branch.signal.troughBid&&amount(bid)<amount(branch.signal.troughBid)+TICK)return;
   cell.latencyMs=t.latencyMs;enter(cell,ask,row.at,b.receivedAt,b.versionKey);return;
  }
  if(cell.stage!=='entered')return;
  for(const exit of Object.values(cell.exits)){if(exit.status!=='awaiting_outcome')continue;
   if(now>instant(exit.expiresAt)){exit.status='outcome_not_observed';continue;}
   if(request<instant(exit.dueAt)||b.versionKey===cell.entryVersion)continue;
   const bid=depth(b.bids,'sell');if(bid!==null)label(cell,exit,bid,row,b.receivedAt,b.requestStartedAt);
  }
 }
 const merged=[...prices.map(row=>({kind:'price',row})),...books.map(row=>({kind:'book',row}))].sort((a,b)=>instant(a.row.at)-instant(b.row.at)||Number(a.kind==='book')-Number(b.kind==='book')||a.row.sequence-b.row.sequence);
 need(merged.length,'EMPTY');
 for(const {kind,row}of merged){
  if(kind==='book'){
   if(['session_start','session_stop','invalidation'].includes(row.kind)){boundary(row.kind==='invalidation'?row.payload.marketId:null,row.at,'source_boundary');continue;}
   if(row.kind!=='book')continue;const b=row.payload.book;
   need(instant(b.requestStartedAt)<=instant(b.receivedAt)&&instant(b.receivedAt)<=instant(row.at),'BOOK_CLOCK');
   if(instant(row.at)-instant(b.receivedAt)>policy.signal.maximumReceiptLagMs){boundary(b.marketId,row.at,'stale_publication');continue;}
   for(const c of active[b.marketId]||[]){need(b.exchangeId===c.exchangeId,'BOOK_SCOPE');for(const branch of Object.values(c.branches))if(branch)for(const t of Object.values(branch.trials))bookTrial(branch,t,b,row);}continue;
  }
  const snapshot=row.event.snapshot;
  need(instant(snapshot.requestStartedAt)<=instant(snapshot.receivedAt)&&instant(snapshot.receivedAt)<=instant(row.at),'PRICE_CLOCK');
  const stale=instant(row.at)-instant(snapshot.receivedAt)>policy.signal.maximumReceiptLagMs;
  if(stale)for(const q of snapshot.quotes)boundary(q.marketId,row.at,'stale_publication');
  for(const id of snapshot.missingIds){for(const c of cases.filter(c=>c.exchangeId===id))revoke(c,'missing_quote',row.at);}
  for(const q of snapshot.quotes){
   if(stale)continue;
   if(!valid(q)){boundary(q.marketId,row.at,'invalid_quote');continue;}
   const permitted=[barriers.global,barriers.markets[q.marketId]].every(at=>at==null||instant(snapshot.requestStartedAt)>instant(at));
   if(!permitted)continue;
   for(const c of active[q.marketId]||[]){
    for(const branch of Object.values(c.branches))if(branch)for(const t of Object.values(branch.trials))quoteTrial(branch,t,q,snapshot,row);
    advanceReboundWatch(c.watch,q,snapshot,row.at);
    if(c.watch.signal&&!c.branches.rebound)c.branches.rebound=makeBranch(c.watch.signal);
   }
  }
  for(const e of bySequence.get(row.sequence)||[]){const q=snapshot.quotes.find(q=>q.marketId===e.marketId&&q.exchangeId===e.exchangeId);
   need(q&&e.priceHash===row.hash&&e.availableAt===row.at&&e.receivedAt===snapshot.receivedAt&&e.quote.entryAsk===price(q.bestAsk),'OPPORTUNITY_SOURCE');
   const dip={marketId:e.marketId,exchangeId:e.exchangeId,availableAt:e.availableAt,receivedAt:e.receivedAt,
    reference:e.reference,bid:money(amount(e.reference.anchor)-amount(e.reference.bidDrop)),ask:e.quote.entryAsk};
   const phase=instant(e.availableAt)>=instant(cutoff)?'prospective':instant(e.availableAt)+policy.developmentPurgeMs<=instant(cutoff)?'development':'purged_boundary';
   const c={id:e.id,marketId:e.marketId,exchangeId:e.exchangeId,phase,watch:createReboundWatch(dip),branches:{immediate:makeBranch({availableAt:e.availableAt,limit:e.quote.entryAsk}),rebound:null}};
   cases.push(c);(active[c.marketId]??=[]).push(c);
   if(stale)revoke(c,'stale_publication',row.at);
  }
 }
 const through=merged.at(-1).row.at;
 for(const c of cases){if(c.watch.status==='watching')c.watch.status=instant(through)>instant(c.watch.expiresAt)?'confirmation_not_observed':'right_censored';
  for(const branch of Object.values(c.branches))if(branch)for(const t of Object.values(branch.trials))for(const kind of ['quote','depth']){const cell=t[kind];
   if(cell.stage!=='entered')cell.stage=instant(through)>instant(cell.expiresAt)&&cell.stage.startsWith('awaiting_')?cell.stage.replace('awaiting_','')+'_not_observed':cell.stage.startsWith('awaiting_')?'right_censored':cell.stage;
   else for(const exit of Object.values(cell.exits))if(exit.status==='awaiting_outcome')exit.status=instant(through)>instant(exit.expiresAt)?'outcome_not_observed':'right_censored';
  }
 }
 const metrics=items=>{const statuses={},done=items.filter(x=>x.status==='observed');for(const x of items)statuses[x.status]=(statuses[x.status]||0)+1;
  const measure=key=>{const values=done.map(x=>amount(x[key])),total=values.reduce((a,b)=>a+b,0n);return {count:values.length,positive:values.filter(x=>x>0n).length,negative:values.filter(x=>x<0n).length,zero:values.filter(x=>x===0n).length,average:values.length?money(total/BigInt(values.length)):null};};
  return {opportunities:items.length,statuses,observedPrices:measure('rawMarkout'),adverseTicks:measure('adverseMarkout')};};
 const groups=Object.fromEntries(['development','purged_boundary','prospective'].map(phase=>{const chosen=cases.filter(c=>c.phase===phase);
  return [phase,{opportunities:chosen.length,markets:new Set(chosen.map(c=>c.marketId)).size,strategies:Object.fromEntries(['immediate','rebound'].map(strategy=>[strategy,{signals:chosen.filter(c=>c.branches[strategy]).length,
   outcomes:Object.fromEntries(policy.latenciesMs.map(latency=>[latency,Object.fromEntries(policy.horizonsMs.map(horizon=>[horizon,Object.fromEntries(['quote','depth'].map(kind=>[kind,metrics(chosen.map(c=>{const branch=c.branches[strategy];if(!branch)return {status:c.watch.status};const cell=branch.trials[latency][kind];return cell.stage==='entered'?cell.exits[horizon]:{status:cell.stage};}))]))]))]))}]))}];}));
 return {through,groups,cases,networkRequests:0,competitionOrders:0,paperFills:0,profitabilityEstablished:false};
}
export function runReboundStudy({priceBytes,bookBytes,universe,bank}){
 need(digest(bank.protocol)===digest(REBOUND_STUDY_POLICY)&&digest(bank.codeIdentity)===digest(identity()),'FROZEN_PROTOCOL');verifyMarkoutProtocol(bank.baseBank,universe);
 const original=studyDipMarkouts({priceBytes,bookBytes,universe,bank:bank.baseBank});
 const result=compareReboundPaths({events:original.events,prices:rows(priceBytes),books:rows(bookBytes),cutoff:bank.cutoff});
 return {...result,createdAt:new Date().toISOString(),mode:'causal_confirmed_rebound_path_comparison_v2',bankHash:digest(bank),cutoff:bank.cutoff,protocol:bank.protocol,sources:original.sources,
  limitations:['Quote outcomes have no verified size/source age and are not paper fills, orders, wallet returns or proof of profitability.',
   'Depth paths require post-signal confirmation, a later independent request/version, original price cap, one-percent depth and a fixed later exit quote. Actual fills are unobserved.',
   'Unconfirmed, missing and revoked opportunities remain in unconditional denominators. Complete-path averages may be selected.',
   'Latencies, horizons and strategy variants share opportunities; market/common-event samples correlate.',
   'Publication times model availability for capture replay, not live consumer receipt. All added fees are zero.']};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const args=process.argv.slice(2),option=n=>{const i=args.indexOf(n);return i<0?null:args[i+1];},file=option('--protocol');need(file,'PROTOCOL_FILE');
 const universe=JSON.parse(fs.readFileSync(new URL('./price-universe.json',import.meta.url)));
 if(args.includes('--initialize')){need(option('--base-protocol'),'BASE_PROTOCOL');const baseBank=JSON.parse(fs.readFileSync(option('--base-protocol')));verifyMarkoutProtocol(baseBank,universe);
  const bank=createReboundStudyBank(new Date().toISOString(),baseBank);fs.writeFileSync(file,JSON.stringify(bank,null,2)+'\n',{flag:'wx',mode:0o600});console.log(JSON.stringify({cutoff:bank.cutoff,bankHash:digest(bank)}));
 }else{const output=option('--output');need(output,'OUTPUT');const dir=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),bank=JSON.parse(fs.readFileSync(file));
  const result=runReboundStudy({priceBytes:fs.readFileSync(path.join(dir,'market-prices.jsonl')),bookBytes:fs.readFileSync(path.join(dir,'public-books.jsonl')),universe,bank});atomicJson(path.resolve(output),result);
  console.log(JSON.stringify({output,cutoff:result.cutoff,through:result.through,groups:result.groups,sources:result.sources},null,2));
 }
}
