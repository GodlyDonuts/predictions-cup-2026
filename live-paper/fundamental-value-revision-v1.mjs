import fs from 'node:fs';import path from 'node:path';import {createHash,randomUUID} from 'node:crypto';
import {amount,money,TICK,instant} from '../paper/fixed.mjs';
import {digest} from './model.mjs';import {Journal} from './transport-runtime.mjs';
import {atomicJson,payloadHash} from '../execution/ledger.mjs';import {probabilityAccountGate,probabilityAccountEligibility} from './probability-receipt.mjs';
const need=(ok,code)=>{if(!ok)throw Error('FUNDAMENTAL_REVISION_'+code);},sha=b=>createHash('sha256').update(b).digest('hex');
export const FUNDAMENTAL_REVISION_POLICY=Object.freeze({mode:'invalidated_forecast_robust_value_reduction',maximumBookAgeMs:20000,depthFractionBps:100,adverseTicks:1,intentLifetimeMs:180000,fees:'0.000000',newEntries:false});
export function validateFundamentalRevision(r,bank){
 need(r?.schemaVersion===1&&r.owner==='Codex'&&r.bankHash===digest(bank)&&r.cohort===bank.name&&r.marketId===bank.research[0].marketId&&r.exchangeId===bank.identities[r.marketId]?.exchangeId&&r.predicateHash===bank.identities[r.marketId]?.predicateHash,'BINDING');
 need(r.policy===FUNDAMENTAL_REVISION_POLICY.mode&&r.newEntries===false&&r.sourceClockRenewed===false,'AUTHORITY');
 need(instant(r.sourceAt)<=instant(r.recordedAt)&&instant(r.recordedAt)<instant(r.expiresAt),'CLOCK');
 need(amount(r.lower)>=0n&&amount(r.upper)<=1000000n&&amount(r.lower)<=amount(r.central)&&amount(r.central)<=amount(r.upper),'VALUES');
 need(Array.isArray(r.sources)&&r.sources.length>0&&r.sources.every(s=>typeof s.url==='string'&&s.url.startsWith('https://')&&/^[a-f0-9]{64}$/.test(s.contentHash))&&/^[a-f0-9]{64}$/.test(r.sourceComparisonHash),'SOURCES');return r;
}
export function createFundamentalRevisionState({bank,originalStats,revision,cutoff}){
 validateFundamentalRevision(revision,bank);need(originalStats.bankHash===digest(bank)&&originalStats.failed===false&&instant(cutoff)>=instant(revision.recordedAt),'OPENING');
 const id=revision.marketId,wallets={};
 for(const [latency,w]of Object.entries(originalStats.wallets||{})){
  need(/^[1-9]\d*$/.test(latency)&&w.fills===w.recentFills?.length&&w.positions?.length===1&&w.positions[0].marketId===id&&w.positions[0].quantity===1&&Array.isArray(w.pending)&&w.pending.every(p=>p.action==='sell'&&p.marketId===id),'FULL_HELD_HISTORY');
  wallets[latency]={cash:w.cash,realizedPnl:w.realizedPnl,cumulativeBuys:w.cumulativeBuys,positions:{[id]:{quantity:1,cost:w.positions[0].cost,enteredAt:w.positions[0].enteredAt}},fills:structuredClone(w.recentFills),pending:null,consumedVersions:{},intents:0,discardedPriorSellIntents:w.pending.length};
 }
 need(Object.keys(wallets).length===bank.fillPolicy.latenciesMs.length&&bank.fillPolicy.latenciesMs.every(ms=>wallets[ms]),'LATENCY_SET');
 const state={schemaVersion:1,bankHash:digest(bank),revisionHash:digest(revision),cutoff,lastAvailableAt:cutoff,barrier:cutoff,sourceBytes:originalStats.sourceBytes,sourceSequence:originalStats.sourceSequence,sourceHash:originalStats.sourceHash,wallets,counts:{},book:null};assertFundamentalRevisionState(state,bank,revision);return state;
}
export function assertFundamentalRevisionState(s,bank,r){
 need(s.bankHash===digest(bank)&&s.revisionHash===digest(r),'STATE_BINDING');
 for(const w of Object.values(s.wallets)){let cash=amount(bank.fillPolicy.initialCash),realized=0n,buys=0n;const positions={};
  for(const f of w.fills){need(f.marketId===r.marketId&&f.exchangeId===r.exchangeId&&f.quantity===1&&f.fee==='0.000000','FILL_SCOPE');const v=amount(f.price);if(f.action==='buy'){need(!positions[f.marketId],'DUPLICATE');positions[f.marketId]={quantity:1,cost:f.price,enteredAt:f.at};cash-=v;buys+=v;}else{need(f.action==='sell'&&positions[f.marketId],'OVERSELL');cash+=v;realized+=v-amount(positions[f.marketId].cost);delete positions[f.marketId];}}
  need(cash===amount(w.cash)&&realized===amount(w.realizedPnl)&&buys===amount(w.cumulativeBuys)&&digest(positions)===digest(w.positions),'CONSERVATION');
  need(!w.pending||w.pending.action==='sell'&&w.pending.marketId===r.marketId&&w.pending.quantity===1&&Boolean(w.positions[r.marketId]),'COVERED_INTENT');
 }
}
export function advanceFundamentalRevision(input,event,bank,revision){
 assertFundamentalRevisionState(input,bank,revision);const s=structuredClone(input),at=event.availableAt,now=instant(at);need(now>=instant(s.lastAvailableAt)&&now>=instant(s.cutoff),'CAUSAL');s.lastAvailableAt=at;
 const boundary=reason=>{s.barrier=at;s.book=null;for(const w of Object.values(s.wallets))w.pending=null;s.counts[reason]=(s.counts[reason]||0)+1;};
 if(event.sourceSequence!==undefined){need(event.sourceSequence>s.sourceSequence&&/^[a-f0-9]{64}$/.test(event.sourceHash),'SOURCE');s.sourceSequence=event.sourceSequence;s.sourceHash=event.sourceHash;if(event.sourceBytes!==undefined)s.sourceBytes=event.sourceBytes;}
 if(event.kind==='boundary'){boundary(event.reason||'source_boundary');assertFundamentalRevisionState(s,bank,revision);return s;}
 if(event.kind==='cursor'){assertFundamentalRevisionState(s,bank,revision);return s;}
 need(event.kind==='book','SOURCE_KIND');
 const b=event.book;need(b.marketId===revision.marketId&&b.exchangeId===revision.exchangeId&&b.instrument===revision.exchangeId+':YES','BOOK_SCOPE');
 const request=instant(b.requestStartedAt),received=instant(b.receivedAt);
 if(request<=instant(s.barrier)||received<request||received>now||now-request>FUNDAMENTAL_REVISION_POLICY.maximumBookAgeMs||b.freshnessPolicy!=='authoritative_receipt_v1'||probabilityAccountGate(event.accountEligibility,b,at)!==null){boundary('unavailable_or_noncausal');return s;}
 if(request<instant(revision.recordedAt)){boundary('revision_not_known');return s;}const revisionCurrent=now<instant(revision.expiresAt);
 need(Array.isArray(b.bids)&&Array.isArray(b.asks)&&b.bids.length&&b.asks.length&&typeof b.versionKey==='string','BOOK_DEPTH');
 const level=b.bids.find(l=>Number.isSafeInteger(l.quantity)&&Math.floor(l.quantity*FUNDAMENTAL_REVISION_POLICY.depthFractionBps/10000)>=1&&amount(l.price)-TICK>=TICK),sale=level?amount(level.price)-TICK:null;
 s.book=structuredClone(b);
 for(const [latency,w]of Object.entries(s.wallets)){
  if(!w.positions[revision.marketId]){w.pending=null;continue;}
  if(w.pending&&(now-instant(w.pending.createdAt)>FUNDAMENTAL_REVISION_POLICY.intentLifetimeMs||sale===null||sale<amount(revision.lower))){w.pending=null;}
  const pending=w.pending;
  if(pending&&request>=instant(pending.createdAt)+Number(latency)&&request>instant(pending.createdAt)&&event.sourceSequence>pending.sourceSequence&&b.versionKey!==pending.sourceVersion&&w.consumedVersions[revision.marketId]!==b.versionKey&&sale>=amount(pending.limit)){
   const old=w.positions[revision.marketId],fill={intentId:pending.id,marketId:revision.marketId,exchangeId:revision.exchangeId,instrument:revision.exchangeId+':YES',action:'sell',quantity:1,price:money(sale),fee:'0.000000',at,requestStartedAt:b.requestStartedAt,receivedAt:b.receivedAt,sourceVersion:b.versionKey,sourceSequence:event.sourceSequence,sourceHash:event.sourceHash,revisionHash:digest(revision),reason:revision.policy,model:'later_rest_request_counterfactual'};
   w.cash=money(amount(w.cash)+sale);w.realizedPnl=money(amount(w.realizedPnl)+sale-amount(old.cost));w.fills.push(fill);delete w.positions[revision.marketId];w.pending=null;w.consumedVersions[revision.marketId]=b.versionKey;s.counts.held_risk_sale=(s.counts.held_risk_sale||0)+1;
  }else if(!w.pending&&revisionCurrent&&sale!==null&&sale>=amount(revision.lower))w.pending={id:++w.intents,marketId:revision.marketId,action:'sell',quantity:1,limit:money(sale),createdAt:at,sourceSequence:event.sourceSequence,sourceVersion:b.versionKey,sourceHash:event.sourceHash,revisionHash:digest(revision)};
 }
 assertFundamentalRevisionState(s,bank,revision);return s;
}
export function fundamentalRevisionReport(s,bank,r,at){
 assertFundamentalRevisionState(s,bank,r);return{mode:'fundamental_held_value_revision_v1',cohort:bank.name,bankHash:s.bankHash,revisionHash:s.revisionHash,updatedAt:at,cutoff:s.cutoff,sourceSequence:s.sourceSequence,sourceHash:s.sourceHash,counts:s.counts,wallets:Object.fromEntries(Object.entries(s.wallets).map(([latency,w])=>[latency,{cash:w.cash,realizedPnl:w.realizedPnl,cumulativeBuys:w.cumulativeBuys,positions:Object.entries(w.positions).map(([marketId,p])=>({marketId,...p})),pending:w.pending?[w.pending]:[],fills:w.fills.length,closedTrades:w.fills.filter(f=>f.action==='sell').length,recentFills:w.fills.slice(-25),reservedCost:'0.000000',newEntries:false}])),policy:FUNDAMENTAL_REVISION_POLICY,revision:{sourceAt:r.sourceAt,recordedAt:r.recordedAt,expiresAt:r.expiresAt,lower:r.lower,upper:r.upper,central:r.central,sourceComparisonHash:r.sourceComparisonHash},executionAuthority:false,competitionOrders:0,profitabilityEstablished:false,limitations:['An invalidated forecast prompts declared downside-risk reduction, not profit-taking or calibrated probability bounds.','Original purchase costs and earlier cash/FIFO remain attributed; a sale may realize a loss.','The original bank and journal remain immutable; only one reducer may own this inventory.','Expiry neither refreshes sources nor settles inventory. Later receipt fills remain local counterfactuals.']};
}
export function openFundamentalValueRevision({runtimeDir,originalHost,revision,initialize=false,now=Date.now,canOperate=()=>true}){
 const dir=path.join(originalHost.dir,'operator-value-revision'),file=path.join(dir,'opening.json'),journalFile=path.join(dir,'journal.jsonl'),lock=path.join(dir,'worker.lock');fs.mkdirSync(dir,{recursive:true,mode:0o700});
 const token=JSON.stringify({pid:process.pid,id:randomUUID()});fs.writeFileSync(lock,token,{flag:'wx',mode:0o600});let closed=false,failed=false;
 try{const bank=originalHost.bank;validateFundamentalRevision(revision,bank);let opening;
  if(initialize){need(!fs.existsSync(file)&&!fs.existsSync(journalFile),'EXISTS');const originalStats=originalHost.status(),originalJournal=fs.readFileSync(path.join(originalHost.dir,'journal.jsonl')),bankBytes=fs.readFileSync(path.join(originalHost.dir,'bank.json')),raw=fs.readFileSync(path.join(runtimeDir,'public-books.jsonl')),prefix=raw.subarray(0,raw.lastIndexOf(10)+1),tailStart=prefix.lastIndexOf(10,prefix.length-2)+1,tail=JSON.parse(prefix.subarray(tailStart,prefix.length-1)),cutoff=new Date(now()).toISOString();need(tail.sequence>=originalStats.sourceSequence&&instant(tail.at)<=instant(cutoff)&&digest(JSON.parse(bankBytes))===digest(bank),'SOURCE_OPENING');opening={revision:structuredClone(revision),originalStats:structuredClone(originalStats),originalStatsHash:digest(originalStats),originalJournalBytes:originalJournal.length,originalJournalHash:sha(originalJournal),originalBankHash:digest(bank),originalBankFileHash:sha(bankBytes),cutoff,sourcePrefixBytes:prefix.length,sourcePrefixHash:sha(prefix),sourcePrefixSequence:tail.sequence,sourcePrefixTailHash:tail.hash,codeHash:sha(fs.readFileSync(new URL('./fundamental-value-revision-v1.mjs',import.meta.url)))};fs.writeFileSync(file,JSON.stringify(opening,null,2)+'\n',{flag:'wx',mode:0o600});}
  else opening=JSON.parse(fs.readFileSync(file));
  const fileHash=sha(fs.readFileSync(file)),verifyOriginal=()=>{const b=fs.readFileSync(path.join(originalHost.dir,'journal.jsonl')),bankBytes=fs.readFileSync(path.join(originalHost.dir,'bank.json'));need(b.length===opening.originalJournalBytes&&sha(b)===opening.originalJournalHash&&sha(bankBytes)===opening.originalBankFileHash&&digest(JSON.parse(bankBytes))===opening.originalBankHash&&digest(originalHost.bank)===opening.originalBankHash,'ORIGINAL_CHANGED');};verifyOriginal();need(digest(opening.revision)===digest(revision)&&digest(opening.originalStats)===opening.originalStatsHash&&opening.codeHash===sha(fs.readFileSync(new URL('./fundamental-value-revision-v1.mjs',import.meta.url))),'OPENING_CHANGED');
  const source=path.join(runtimeDir,'public-books.jsonl'),verifySource=()=>{const b=fs.readFileSync(source).subarray(0,opening.sourcePrefixBytes);need(b.length===opening.sourcePrefixBytes&&sha(b)===opening.sourcePrefixHash,'SOURCE_PREFIX');};verifySource();
  const journal=new Journal(journalFile);let state=createFundamentalRevisionState({bank,originalStats:opening.originalStats,revision,cutoff:opening.cutoff});state.sourceBytes=opening.sourcePrefixBytes;state.sourceSequence=opening.sourcePrefixSequence;state.sourceHash=opening.sourcePrefixTailHash;
  if(journal.last)for(const [i,row]of fs.readFileSync(journalFile,'utf8').trimEnd().split('\n').map(JSON.parse).entries()){if(i===0)need(row.event.kind==='genesis'&&row.event.openingHash===fileHash,'GENESIS');else state=advanceFundamentalRevision(state,row.event,bank,revision);need(digest(row.state)===digest(state),'ECONOMIC_REPLAY');}
  const live=(allowStopped=false)=>{need(!closed&&!failed&&(allowStopped||canOperate()&&!fs.existsSync(path.join(runtimeDir,'STOP')))&&fs.readFileSync(lock,'utf8')===token&&sha(fs.readFileSync(file))===fileHash&&opening.codeHash===sha(fs.readFileSync(new URL('./fundamental-value-revision-v1.mjs',import.meta.url))),'OWNERSHIP');verifyOriginal();verifySource();};
  let operatorJournalHash=fs.existsSync(journalFile)?sha(fs.readFileSync(journalFile)):sha(Buffer.alloc(0));
  const status=()=>({...fundamentalRevisionReport(state,bank,revision,new Date(now()).toISOString()),originalJournalHash:opening.originalJournalHash,originalJournalBytes:opening.originalJournalBytes,originalBankFileHash:opening.originalBankFileHash,sourcePrefixHash:opening.sourcePrefixHash,sourcePrefixBytes:opening.sourcePrefixBytes,operatorCodeHash:opening.codeHash,journalSequence:journal.sequence,journalHash:journal.previous,failed});
  const append=(next,event,allowStopped=false)=>{live(allowStopped);try{need((fs.existsSync(journalFile)?sha(fs.readFileSync(journalFile)):sha(Buffer.alloc(0)))===operatorJournalHash,'OPERATOR_JOURNAL_CHANGED');journal.commit(next,event);operatorJournalHash=sha(fs.readFileSync(journalFile));state=next;atomicJson(path.join(dir,'stats.json'),status());}catch(e){failed=true;throw e;}};
  if(!journal.last)append(state,{kind:'genesis',openingHash:fileHash});else{const event={kind:'boundary',reason:'operator_restart',availableAt:new Date(Math.max(now(),instant(state.lastAvailableAt))).toISOString()};append(advanceFundamentalRevision(state,event,bank,revision),event);}
  return{dir,bank,status,publish:()=>atomicJson(path.join(dir,'stats.json'),status()),entryDraining:()=>true,
   consume(){live();const raw=fs.readFileSync(source),sealed=raw.subarray(0,raw.lastIndexOf(10)+1);need(sealed.length>=state.sourceBytes,'SOURCE_TRUNCATED');const lines=sealed.subarray(state.sourceBytes).toString().split('\n').slice(0,-1);let account=null;try{account=probabilityAccountEligibility(JSON.parse(fs.readFileSync(path.join(runtimeDir,'official-account.json'))));}catch{}
    for(const line of lines){const row=JSON.parse(line),{hash,...body}=row;need(row.sequence===state.sourceSequence+1&&row.previousHash===state.sourceHash&&payloadHash(body)===hash,'SOURCE_CHAIN');const availableAt=new Date(now()).toISOString();need(instant(row.at)<=instant(availableAt),'FUTURE_SOURCE');const sourceBytes=state.sourceBytes+Buffer.byteLength(line+'\n'),base={availableAt,sourceSequence:row.sequence,sourceHash:hash,sourceBytes};let event;
     if(row.kind==='book'&&row.payload?.book?.marketId===revision.marketId)event={...base,kind:'book',book:row.payload.book,accountEligibility:account};else if(['session_start','session_stop'].includes(row.kind)||row.kind==='invalidation'&&(row.payload?.marketId===null||row.payload?.marketId===revision.marketId))event={...base,kind:'boundary',reason:'source_boundary'};else event={...base,kind:'cursor'};
     append(advanceFundamentalRevision(state,event,bank,revision),event);
    }return lines.length;},
   onBook(publication,marketId,accountEligibility){if(marketId!==revision.marketId)return;live();const book=publication.books[revision.exchangeId+':YES'];if(!book)return;const event=publication.closed?{kind:'boundary',reason:'source_closed',availableAt:new Date(now()).toISOString()}:{kind:'book',book,accountEligibility,availableAt:new Date(now()).toISOString(),sourceSequence:publication.sequence,sourceHash:publication.hash};append(advanceFundamentalRevision(state,event,bank,revision),event);},
   focus(){return{bankHash:opening.originalBankHash,revisionHash:digest(revision),markets:Object.values(state.wallets).some(w=>w.positions[revision.marketId]||w.pending)?[{marketId:revision.marketId,phase:'held'}]:[],ownedRiskReads:true,executionAuthority:false};},
   boundary(reason){const event={kind:'boundary',reason,availableAt:new Date(Math.max(now(),instant(state.lastAvailableAt))).toISOString()};append(advanceFundamentalRevision(state,event,bank,revision),event,true);},
   close(){if(closed)return;need(fs.readFileSync(lock,'utf8')===token,'LOCK_CHANGED');closed=true;fs.unlinkSync(lock);}
  };
 }catch(e){if(fs.existsSync(lock)&&fs.readFileSync(lock,'utf8')===token)fs.unlinkSync(lock);throw e;}
}

export function wrapFundamentalValueRevision(host,{runtimeDir,now=Date.now,filesystem=fs,canOperate=()=>true}={}){
 const pointer=path.join(runtimeDir,'FUNDAMENTAL_VALUE_REVISION'),opening=path.join(host.dir,'operator-value-revision','opening.json');
 if(!filesystem.existsSync(pointer)&&!filesystem.existsSync(opening))return host;
 let revision;if(filesystem.existsSync(pointer)){const v=JSON.parse(filesystem.readFileSync(pointer,'utf8'));need(v.schemaVersion===1&&v.owner==='Codex'&&v.cohort===host.bank.name&&v.bankHash===digest(host.bank),'POINTER');revision=v.revision;}else revision=JSON.parse(filesystem.readFileSync(opening,'utf8')).revision;
 const operator=openFundamentalValueRevision({runtimeDir,originalHost:host,revision,initialize:!filesystem.existsSync(opening),now,canOperate});
 // Sticky ownership: absent or expired pointer never resumes the old forecast.
 return{...host,consume:operator.consume,status:operator.status,publish:operator.publish,focus:operator.focus,entryDraining:()=>true,boundary:operator.boundary,close(){operator.close();host.close();},operatorRevisionActive:true};
}
