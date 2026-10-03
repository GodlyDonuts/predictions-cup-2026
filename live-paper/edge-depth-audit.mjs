import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {createBookPublicationState,applyBookPublication} from './book-publication.mjs';
import {amount,money,TICK,SCALE,instant} from '../paper/fixed.mjs';
import {digest} from './model.mjs';
const need=(ok,reason)=>{if(!ok)throw Error('EDGE_DEPTH_'+reason);};
// Diagnostic only: keep the fixed one-percent cap separately at every level.
export function oneShareDepth(levels,action){
 need(['buy','sell'].includes(action)&&Array.isArray(levels),'SIDE');
 let previous=null;
 for(const l of levels){const p=amount(l.price);need(p>=0n&&p<=SCALE&&Number.isSafeInteger(l.quantity)&&l.quantity>0,'LEVEL');need(previous===null||(action==='buy'?p>previous:p<previous),'ORDER');previous=p;}
 const index=levels.findIndex(l=>l.quantity>=100);
 if(index<0)return {status:'unavailable',levelIndex:null,rawPrice:null,modeledPrice:null};
 const raw=amount(levels[index].price),modeled=raw+(action==='buy'?TICK:-TICK);
 if(modeled<TICK||modeled>SCALE-TICK)return {status:'unavailable',levelIndex:null,rawPrice:null,modeledPrice:null};
 return {status:'available',levelIndex:index,rawPrice:money(raw),modeledPrice:money(modeled)};
}
export function auditTrialDepth({bank,trials,sourceBytes}){
 const complete=sourceBytes.lastIndexOf(10)+1;need(complete>0,'EMPTY');
 const bytes=sourceBytes.subarray(0,complete),rows=bytes.toString().split('\n').slice(0,-1).map(JSON.parse);
 let publication=createBookPublicationState(bank.universe);for(const row of rows)publication=applyBookPublication(publication,row);
 need(bank.config.feeBps===0&&bank.config.slippageTicks===1&&bank.config.depthFractionBps===100&&bank.config.orderShares===1,'POLICY');
 const results=trials.map(t=>{
  need(t.phase==='awaiting_outcome'&&instant(t.dueAt)===instant(t.filledAt)+bank.policy.horizonMs&&instant(t.expiresAt)===instant(t.dueAt)+bank.policy.labelLatenessMs,'DEADLINE');
  const entry=rows.find(r=>r.kind==='book'&&r.payload.book.marketId===t.marketId&&r.payload.book.receivedAt===t.filledAt);need(entry,'ENTRY');
  const eb=entry.payload.book;need(instant(eb.requestStartedAt)>=instant(t.preparedAt)+bank.config.latencyMs&&eb.bids[0].quantity>=100&&eb.asks[0].quantity>=100,'ENTRY_ELIGIBILITY');
  const revoke=rows.find(r=>r.sequence>entry.sequence&&(['session_start','session_stop'].includes(r.kind)||r.kind==='invalidation'&&(r.payload.marketId===null||r.payload.marketId===t.marketId)));
  const outcome=rows.find(r=>r.sequence>entry.sequence&&r.kind==='book'&&r.payload.book.marketId===t.marketId&&instant(r.payload.book.requestStartedAt)>=instant(t.dueAt));
  const base={...t,entrySequence:entry.sequence,entryHash:entry.hash};
  if(!outcome||revoke&&revoke.sequence<outcome.sequence)return {...base,status:revoke?'revoked':'not_observed',revocationSequence:revoke?.sequence??null};
  const b=outcome.payload.book,entryPrice=amount(eb.asks[0].price)+TICK,ladder=oneShareDepth(b.bids,'sell'),topDepth=b.bids[0].quantity>=100&&b.asks[0].quantity>=100;
  const timely=instant(b.receivedAt)<=instant(t.expiresAt),independent=b.versionKey!==eb.versionKey;
  return {...base,outcomeSequence:outcome.sequence,outcomeHash:outcome.hash,outcomeReceivedAt:b.receivedAt,outcomeRequestStartedAt:b.requestStartedAt,topBid:b.bids[0],topAsk:b.asks[0],timely,independent,topDepth,status:!timely?'late_quote':!independent?'unchanged_version':!topDepth?'frozen_top_depth_censor':'top_depth_available',fullLadder:ladder,diagnosticModeledNet:timely&&independent&&ladder.status==='available'?money(amount(ladder.modeledPrice)-entryPrice):null};
 });
 return {schemaVersion:1,mode:'source_verified_trial_depth_diagnostic',bankHash:digest(bank),sourceSequence:publication.sequence,sourceHash:publication.hash,sourceBytes:complete,sourceBytesHash:createHash('sha256').update(bytes).digest('hex'),incompleteTailBytes:sourceBytes.length-complete,trials:results,networkRequests:0,ordersSubmitted:0,profitabilityEstablished:false,limitations:['Full-ladder values are alternative quote diagnostics, not original labels or actual fills.','Original frozen top-depth censoring and wallet accounting remain unchanged.','Receipt time checks cannot replace original consumer checkpoint availability and replay.','One-percent capacity is floored separately at each price level; deeper liquidity receives its worse price.']};
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===path.resolve(process.argv[1])){
 const rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),deployment=JSON.parse(fs.readFileSync('data/edge-trial-deployment.json')),manifest=JSON.parse(fs.readFileSync(path.join(rt,'executable-edge-active.json')));
 need(/^[a-z][a-z0-9-]{1,63}$/.test(manifest.cohort),'COHORT');
 const bank=JSON.parse(fs.readFileSync(path.join(rt,'executable-edge-cohorts',manifest.cohort,'bank.json')));need(digest(bank)===manifest.bankHash&&deployment.focus.bankHash===manifest.bankHash,'BANK');
 console.log(JSON.stringify(auditTrialDepth({bank,trials:deployment.focus.trials,sourceBytes:fs.readFileSync(path.join(rt,'public-books.jsonl'))}),null,2));
}
