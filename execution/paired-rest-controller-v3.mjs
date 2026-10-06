import fs from 'node:fs';import path from 'node:path';import {createHash,randomUUID} from 'node:crypto';import {fileURLToPath} from 'node:url';
import {Journal} from '../live-paper/transport-runtime.mjs';
import {normalizeBook} from '../live-paper/model.mjs';
import {acquireOwnerLease} from './ledger.mjs';import {payloadHash,canonicalPayload} from './core.mjs';
import {OrderTransport} from './transport.mjs';
import {planPairedDryRun} from './paired-dry-run.mjs';
import {inspectScopedRestNoAccount,verifyScopedRestNoBuy} from './no-rest-reconciliation.mjs';
import {verifyScopedRestNoBuyV2} from './no-rest-reconciliation-v2.mjs';
import {inspectNoAckAllEconomics} from './no-neutral-all.mjs';
import {parseNumberLexemes} from './exact-money-receipts.mjs';
import {normalizeOfficialFillTick} from './api-tick-price.mjs';
import {amount,money,price,instant,SCALE,TICK} from '../paper/fixed.mjs';
import {instantNs} from '../feed/contract.mjs';

const need=(x,c)=>{if(!x)throw Error('PAIRED_REST_'+c);},clone=x=>structuredClone(x);
const sha=x=>createHash('sha256').update(x).digest('hex'),hex=x=>typeof x==='string'&&/^[a-f0-9]{64}$/.test(x),id=x=>typeof x==='string'&&/^[1-9]\d*$/.test(x)&&Number.isSafeInteger(Number(x));
const iso=n=>new Date(n).toISOString();
const keys=(o,allowed)=>need(o&&typeof o==='object'&&!Array.isArray(o)&&Object.keys(o).every(k=>allowed.includes(k)),'UNSUPPORTED_FIELDS');
function identity(entry=fileURLToPath(import.meta.url)){ const seen=new Map();function visit(file){if(seen.has(file))return;const raw=fs.readFileSync(file);seen.set(file,sha(raw));for(const m of raw.toString().matchAll(/(?:from\s*|import\s*)['"](\.[^'"]+)['"]/g))visit(path.resolve(path.dirname(file),m[1]));}const file=entry;visit(file);return Object.fromEntries([...seen].map(([f,h])=>[path.relative(path.dirname(file),f).replaceAll('\\','/'),h]).sort(([a],[b])=>a.localeCompare(b)));}
// A distinct dormant REST execution revision. The original capture/dry plan is
// retained by hash; its observed protective cost is not relabeled as the reserve.
// Sequential hedge flexibility uses only the declared ordinary payout margin.
export function derivePairedRestPlan(input){const original=planPairedDryRun(input),{planHash:originalDryPlanHash,...body}=original,ceiling=amount(original.conditionalOrdinaryMinimumPayout)-amount(original.policy.minimumOrdinaryMargin);
 need(ceiling===990000n&&ceiling%TICK===0n&&amount(original.pairCostCeiling)<=ceiling,'ORDINARY_MARGIN_CEILING');
 const derived={...body,schemaVersion:2,mode:'paired_rest_no_fixed_margin_v2',originalDryPlanHash,originalProtectiveCost:original.pairCostCeiling,originalConditionalOrdinarySurplus:original.conditionalOrdinarySurplus,sourceMinimumOrdinaryMargin:original.policy.minimumOrdinaryMargin,pairCostCeiling:money(ceiling),conditionalOrdinarySurplus:money(amount(original.conditionalOrdinaryMinimumPayout)-ceiling),exceptionLoss:money(-ceiling)};
 return{...derived,planHash:payloadHash(derived)};
}
const active=s=>s.orders.find(o=>o.phase!=='reconciled')??null;
const cost=s=>s.orders.reduce((n,o)=>n+amount(o.proof?.scopedPrincipalCost??'0.000000'),0n);
const ended=s=>s.orders.length===2&&s.orders.every(o=>o.phase==='reconciled')||s.orders[0]?.phase==='reconciled'&&s.orders[0].proof.filledQuantity===0;
function funding(s){const used=cost(s),body={schemaVersion:1,ownerId:s.plan.scope.ownerId,runId:s.plan.scope.runId,tournamentId:s.plan.scope.tournamentId,planHash:s.plan.planHash,allocatedCapital:'2.000000',reservedCash:ended(s)?'0.000000':money(amount(s.plan.pairCostCeiling)-used),ownedPrincipal:money(used),cumulativeBuy:money(used),maxCumulativeBuy:'2.000000'};return{...body,reservationHash:payloadHash(body)};}
function binding(s,o,orderId=o.orderId){return{ownerId:s.plan.scope.ownerId,runId:s.plan.scope.runId,intentId:o.intentId,idempotencyKey:o.body.idempotencyKey,orderId,marketId:o.marketId,exchangeId:o.body.exchangeId,tournamentId:o.body.tournamentId,tournamentSlug:s.plan.scope.tournamentSlug,side:'no',action:'buy',quantity:1,limit:money(BigInt(Math.round(o.body.price*1e6))),createdAt:o.createdAt,expirationDate:o.body.expirationDate};}
function raw(r,method,endpoint){keys(r,['method','endpoint','status','bodyText','requestStartedAt','receivedAt','sourceBodyHash','requestBodyHash','fetchInvocationId']);need(r.method===method&&r.endpoint===endpoint&&Number.isInteger(r.status)&&typeof r.bodyText==='string'&&Buffer.byteLength(r.bodyText)<=8*1024*1024,'RAW_RECEIPT_REQUIRED');instant(r.requestStartedAt);instant(r.receivedAt);need(instant(r.receivedAt)>=instant(r.requestStartedAt),'RAW_RECEIPT_CLOCK');const bodyHash=sha(r.bodyText);if(r.sourceBodyHash!==undefined)need(r.sourceBodyHash===bodyHash,'RAW_RECEIPT_HASH');return{...clone(r),sourceBodyHash:bodyHash};}
function accountShape(a){keys(a,['cash','positions','openOrdersPages','scopedOrdersPages']);for(const r of [a.cash,a.positions,...(a.openOrdersPages??[]),...(a.scopedOrdersPages??[])])raw(r,'GET',r.endpoint);}
function expectedPositions(s){const out=clone(s.externalPositions??[]);for(const o of s.orders)if(o.proof?.filledQuantity)out.push({exchangeId:o.body.exchangeId,marketId:o.marketId,quantity:-1,settled:false});return out.filter(p=>p.quantity).sort((a,b)=>Number(a.exchangeId)-Number(b.exchangeId));}
function globalGate(g,s,at,own=null){need(g?.tournamentId===s.plan.scope.tournamentId&&g.ownerId===s.plan.scope.ownerId&&g.runId===s.plan.scope.runId&&g.complete===true&&g.soleCollector===true&&g.stopped===false&&hex(g.sourceHash)&&Array.isArray(g.unresolvedIntentIds)&&g.unresolvedIntentIds.length<=1&&g.unresolvedIntentIds.every(v=>v===own)&&instant(g.at)<=at&&at-instant(g.at)<=20000,'GLOBAL_UNCERTAINTY');}
function preflight(s,e,at,o=null){
 keys(e,['tournamentId','account','book','globalRisk','funding','reservedElsewhere']);accountShape(e.account);
 need(!ended(s)&&instant(s.plan.reviewExpiresAt)>at,'PAIR_FINISHED_OR_RULES_EXPIRED');const index=o?o.index:s.orders.length,leg=s.plan.legs[index];need(leg,'PAIR_LIMIT');
 if(index===0)need(s.plan.legs.every(l=>at-instant(l.book.requestStartedAt)<=20000),'INITIAL_PAIR_BOOKS_STALE');
 const cap=index===0?price(leg.originalLimit):amount(s.plan.pairCostCeiling)-cost(s);need(cap>=TICK&&cap<=SCALE-TICK&&cap<=1000000n&&(index!==0||cap<=500000n),'COST_CAP');
 if(index===1)need(s.orders[0]?.phase==='reconciled'&&s.orders[0].proof.filledQuantity===1,'FIRST_LEG_UNRESOLVED');
 globalGate(e.globalRisk,s,at,o?.intentId??null);need(e.tournamentId===s.plan.scope.tournamentId,'BOOK_SCOPE');
 const reserve=funding(s),installed=e.funding;need(installed?.reservationHash===reserve.reservationHash&&installed.amount===reserve.reservedCash&&hex(installed.globalLedgerHash)&&instant(installed.confirmedAt)<=at&&at-instant(installed.confirmedAt)<=20000,'GLOBAL_FUNDING_NOT_INSTALLED');
 const b={...s.plan.scope,exchangeId:leg.exchangeId},a=inspectScopedRestNoAccount({account:e.account,binding:b,readAt:iso(at),requiredFunding:reserve.reservedCash,reservedElsewhere:e.reservedElsewhere??'0.000000'});need(a.funded,'FULL_PAIR_FUNDING');
 const positions=a.positions.filter(p=>p.quantity).sort((x,y)=>Number(x.exchangeId)-Number(y.exchangeId));
 if(s.externalPositions===null)need(!positions.some(p=>s.plan.legs.some(l=>l.exchangeId===p.exchangeId)),'EXISTING_PAIR_HOLDING');else need(payloadHash(positions)===payloadHash(expectedPositions(s)),'ACCOUNT_POSITION_DRIFT');
 const endpoint='/markets/'+leg.marketId+'/orderbook?tournamentId='+s.plan.scope.tournamentId+'&depth=20',receipt=raw(e.book,'GET',endpoint);need(receipt.status===200,'BOOK_HTTP');
 const data=parseNumberLexemes(receipt.bodyText).data,book=normalizeBook(data,leg.marketId,s.plan.scope.tournamentId,receipt.receivedAt,receipt.requestStartedAt,20000,new Set(),'authoritative_receipt_v1');
 need(data.contexts.length===1&&data.contexts[0].type==='tournament'&&book.exchangeId===leg.exchangeId&&book.bids.length<=20&&book.asks.length<=20&&instant(a.completedAt)<=instant(book.requestStartedAt)&&at-instant(book.requestStartedAt)<=20000&&instant(book.receivedAt)<=at,'BOOK_ACCOUNT_CAUSALITY');
 const prior=o?.book??leg.book;need(BigInt(book.sequence)>=BigInt(prior.sequence)&&instantNs(book.sourceAt)>=instantNs(prior.sourceAt)&&instant(book.requestStartedAt)>=instant(prior.requestStartedAt),'BOOK_ROLLBACK');
 if(book.versionKey===prior.versionKey)need(payloadHash([book.bids,book.asks])===payloadHash([prior.bids,prior.asks]),'BOOK_VERSION_CONFLICT');
 need(book.bids.some(l=>Math.floor(l.quantity/100)>=1&&SCALE-price(l.price)<=cap),'DEPTH_OR_PRICE');
 if(o)need(at<instant(o.body.expirationDate)&&price(binding(s,o).limit)===cap,'EXPIRED_OR_CAP_CHANGED');
 return{index,leg,cap,book,positions,account:a};
}
function legacyCanonicalAck(o,r){try{const d=parseNumberLexemes(r.bodyText).data;return Number.isSafeInteger(d.orderId)&&d.orderId>0&&d.exchangeId===o.body.exchangeId&&d.side==='no'&&d.action==='buy'&&d.quantity===1&&amount(normalizeOfficialFillTick(d.price).price)===price(String(o.body.price))&&d.all===null&&typeof d.open==='boolean'&&Number.isSafeInteger(d.quantityTraded)&&Number.isSafeInteger(d.remainingQuantity)&&d.quantityTraded>=0&&d.remainingQuantity>=0&&d.quantityTraded+d.remainingQuantity===1;}catch{return false;}}
function canonicalAck(o,r){try{const d=parseNumberLexemes(r.bodyText).data;return Number.isSafeInteger(d.orderId)&&d.orderId>0&&d.exchangeId===o.body.exchangeId&&d.side==='no'&&d.action==='buy'&&d.quantity===1&&amount(normalizeOfficialFillTick(d.price).price)===price(String(o.body.price))&&!!inspectNoAckAllEconomics(r)&&typeof d.open==='boolean'&&Number.isSafeInteger(d.quantityTraded)&&Number.isSafeInteger(d.remainingQuantity)&&d.quantityTraded>=0&&d.remainingQuantity>=0&&d.quantityTraded+d.remainingQuantity===1;}catch{return false;}}
function reduce(s,e,{legacy=false}={}){
 if(e.kind==='initialize'){need(s===null,'ALREADY_INITIALIZED');return{schemaVersion:2,plan:derivePairedRestPlan({capture:e.capture,scope:e.scope,readAt:e.at}),orders:[],externalPositions:null,installedFunding:null};}
 need(s,'NOT_INITIALIZED');const n=clone(s),o=n.orders[e.index];
 if(e.kind==='neutral_ack_revision'){need(!legacy&&o?.phase==='semantic_mismatch'&&o.semanticMismatch&&o.orderId===e.orderId&&o.ack.sourceBodyHash===e.sourceBodyHash&&canonicalAck(o,o.ack),'NEUTRAL_ACK_REVISION_BINDING');const proof=inspectNoAckAllEconomics(o.ack);need(proof.allState==='neutral'&&payloadHash(proof)===e.economicsProofHash,'NEUTRAL_ACK_REVISION_PROOF');o.semanticMismatch=false;o.phase='acknowledged';return n;}
 if(e.kind==='funding'){need(e.receipt.reservationHash===funding(s).reservationHash&&e.receipt.amount===funding(s).reservedCash&&hex(e.receipt.globalLedgerHash)&&instant(e.receipt.confirmedAt)<=instant(e.at),'FUNDING_RECEIPT');n.installedFunding=clone(e.receipt);return n;}
 if(e.kind==='prepared'){need(!active(s),'ACTIVE_INTENT');const v=preflight(s,e.evidence,instant(e.at));need(e.index===v.index&&typeof e.intentId==='string'&&typeof e.idempotencyKey==='string'&&!s.orders.some(x=>x.intentId===e.intentId||x.body.idempotencyKey===e.idempotencyKey),'INTENT_IDENTITY');
  const body={idempotencyKey:e.idempotencyKey,exchangeId:v.leg.exchangeId,side:'no',action:'buy',quantity:1,price:Number(money(v.cap)),expirationDate:iso(Math.min(instant(e.at)+30000,instant(s.plan.reviewExpiresAt))),tournamentId:s.plan.scope.tournamentId};
  n.externalPositions??=v.positions;n.orders.push({index:v.index,intentId:e.intentId,marketId:v.leg.marketId,createdAt:e.at,body,bodyJson:canonicalPayload(body),bodyHash:payloadHash(body),before:clone(e.evidence.account),reservedElsewhere:e.evidence.reservedElsewhere??'0.000000',book:v.book,phase:'prepared',orderId:null,ack:null,wireAttempts:[],fetchStarts:[],fences:[],semanticMismatch:false,cancelPending:false,proof:null});return n;
 }
 need(o&&o.phase!=='reconciled','NO_ACTIVE_ORDER');
 if(e.kind==='admitted'){
  if(e.operation==='place')need(o.phase==='prepared','PLACEMENT_PHASE');else if(e.operation==='recover')need(['uncertain','dispatching'].includes(o.phase)&&!o.orderId&&!o.semanticMismatch&&instant(e.at)>=instant(o.body.expirationDate)&&o.fetchStarts.some(w=>w.kind==='place'&&instant(w.requestStartedAt)<instant(o.body.expirationDate)),'RECOVERY_PHASE');else need(e.operation==='cancel'&&o.orderId&&instant(e.at)>=instant(o.body.expirationDate)&&!o.cancelPending,'CANCEL_PHASE');
  o.operation=e.operation;if(e.operation==='cancel')o.cancelPending=true;else o.phase='dispatching';return n;
 }
 if(e.kind==='network_fence'){need(o.operation===e.operation&&hex(e.leaseHash)&&payloadHash(e.leaseIdentity)===e.leaseHash,'NETWORK_FENCE');o.fences.push({operation:e.operation,at:e.at,leaseIdentity:e.leaseIdentity,leaseHash:e.leaseHash});return n;}
 if(e.kind==='fetch_start'){
  const w=e.start,l=o.fences.at(-1);need(l&&w.kind===o.operation&&w.ownerId===s.plan.scope.ownerId&&w.runId===s.plan.scope.runId&&w.intentId===o.intentId&&w.idempotencyKey===o.body.idempotencyKey&&w.requestBodyJson===o.bodyJson&&w.requestBodyHash===o.bodyHash&&w.leaseHash===l.leaseHash&&payloadHash(w.leaseIdentity)===l.leaseHash&&typeof w.fetchInvocationId==='string'&&w.fetchInvocationId.length>0&&!n.orders.some(v=>v.fetchStarts.some(x=>x.fetchInvocationId===w.fetchInvocationId))&&instant(w.requestStartedAt)>=instant(l.at)&&instant(w.requestStartedAt)<=instant(e.at)&&w.method===(w.kind==='cancel'?'DELETE':'POST')&&w.endpoint===(w.kind==='cancel'?'/orders/'+o.orderId:'/orders'),'FETCH_START_BINDING');
  if(w.kind==='place')need(instant(w.requestStartedAt)<instant(o.body.expirationDate),'ORIGINAL_WIRE_EXPIRED');o.fetchStarts.push(clone(w));return n;
 }
 if(e.kind==='wire'){
  const w=e.wire,started=o.fetchStarts.at(-1);need(started&&w.fetchInvocationId===started.fetchInvocationId&&w.requestStartedAt===started.requestStartedAt&&w.kind===o.operation&&w.ownerId===s.plan.scope.ownerId&&w.runId===s.plan.scope.runId&&w.intentId===o.intentId&&w.idempotencyKey===o.body.idempotencyKey&&w.requestBodyJson===o.bodyJson&&w.requestBodyHash===o.bodyHash&&w.leaseHash===o.fences.at(-1)?.leaseHash&&instant(w.requestStartedAt)>=instant(o.fences.at(-1).at)&&instant(w.receivedAt)>=instant(w.requestStartedAt)&&instant(w.receivedAt)<=instant(e.at)&&hex(w.sourceBodyHash)&&sha(w.bodyText)===w.sourceBodyHash,'WIRE_PROVENANCE');
  if(w.kind==='place')need(instant(w.requestStartedAt)<instant(o.body.expirationDate),'ORIGINAL_WIRE_EXPIRED');o.wireAttempts.push(clone(w));
  if(w.kind==='cancel'){o.cancelPending=true;return n;}
  if(w.status>=200&&w.status<300){let d;try{d=parseNumberLexemes(w.bodyText).data;}catch{}if(Number.isSafeInteger(d?.orderId)&&d.orderId>0){need(!o.orderId||o.orderId===String(d.orderId),'ORDER_ID_CHANGED');o.orderId=String(d.orderId);}o.ack={method:'POST',endpoint:'/orders',status:w.status,requestStartedAt:w.requestStartedAt,receivedAt:w.receivedAt,bodyText:w.bodyText,sourceBodyHash:w.sourceBodyHash,requestBodyHash:o.bodyHash};o.semanticMismatch=!(legacy?legacyCanonicalAck:canonicalAck)(o,o.ack);o.phase=o.semanticMismatch?'semantic_mismatch':'acknowledged';}
  else o.phase='uncertain';return n;
 }
 if(e.kind==='error'){if(o.phase==='dispatching')o.phase='uncertain';o.lastError=e.code;return n;}
 if(e.kind==='reconciled'){
  need(o.orderId&&o.ack&&!o.semanticMismatch,'RECONCILIATION_BINDING');
  keys(e.evidence,['order','fillPages','after','globalRisk','accountActivity']);accountShape(e.evidence.after);raw(e.evidence.order,'GET','/orders/'+o.orderId);for(const r of e.evidence.fillPages??[])raw(r,'GET',r.endpoint);
  const proof=(legacy?verifyScopedRestNoBuy:verifyScopedRestNoBuyV2)({...e.evidence,binding:binding(s,o),before:o.before,ack:o.ack,recoveryProof:e.recoveryProof,readAt:e.at,fullPairReservation:funding(s).reservedCash,reservedElsewhere:o.reservedElsewhere});
  need(proof.scopedPrincipalCost===money(amount(proof.scopedPrincipalCost))&&cost(s)+amount(proof.scopedPrincipalCost)<=amount(s.plan.pairCostCeiling)&&cost(s)+amount(proof.scopedPrincipalCost)<=2000000n,'PAIR_PRINCIPAL');o.proof=proof;o.phase='reconciled';o.cancelPending=false;return n;
 }
 throw Error('PAIRED_REST_EVENT');
}

// Staged lifecycle only. The caller supplies its EXISTING serialized dispatcher,
// actual account lease, external STOP/activation gate and durable global reserve.
// No credential/client/limiter, activation file, polling loop or implicit retry.
export function openPairedRestControllerV3({dir,capture,scope,dispatch,canOperate,assertAccountLease,installFundingReservation,getEvidence,rawReceiptOf,now=Date.now,neutralAckRevision=null}){
 need(path.isAbsolute(dir??'')&&[dispatch,canOperate,assertAccountLease,installFundingReservation,getEvidence,rawReceiptOf,now].every(f=>typeof f==='function'),'DEPENDENCIES');
 scope=clone(scope);
 const local=acquireOwnerLease({dir,ownerId:scope.ownerId,runId:scope.runId,scope:'paired-rest-lifecycle',now}),file=path.join(dir,'paired-rest-journal.jsonl'),code=identity();let state=null,closed=false,failed=false,busy=false,size=0,bytesHash,stat,rows=[],inFlight=null;const issued=new WeakMap();
 const basic=()=>{need(!closed&&!failed&&canOperate()===true,'STOPPED_OR_FAILED');local.assertOwnership();const l=assertAccountLease();need(l?.ownerId===scope.ownerId&&l.tournamentId===scope.tournamentId&&(l.runId===undefined||l.runId===scope.runId)&&typeof l.leaseId==='string'&&l.leaseId.length>0,'ACCOUNT_LEASE');keys(l,['ownerId','runId','tournamentId','leaseId']);return clone(l);};
 try{
  if(!fs.existsSync(file))fs.closeSync(fs.openSync(file,'wx',0o600));const bytes=fs.readFileSync(file);need(!bytes.length||bytes.at(-1)===10,'PARTIAL_JOURNAL');const journal=new Journal(file);
  const recoveryMember=input=>{if(input.recoveryProof){const r=input.recoveryProof.journalRecord;need(r&&rows.some(v=>v.sequence===r.sequence&&v.hash===r.hash&&payloadHash(v)===payloadHash(r)),'RECOVERY_JOURNAL_MEMBERSHIP');}};
  const legacyCode=identity(path.join(path.dirname(fileURLToPath(import.meta.url)),'paired-rest-controller.mjs'));let prefixSeen=0;
  if(neutralAckRevision){need(neutralAckRevision.kind==='same_run_neutral_all_ack_reconciliation'&&neutralAckRevision.ownerId===scope.ownerId&&neutralAckRevision.runId===scope.runId&&neutralAckRevision.scopeHash===payloadHash(scope)&&Number.isSafeInteger(neutralAckRevision.originalPrefixBytes)&&neutralAckRevision.originalPrefixBytes>0&&neutralAckRevision.originalPrefixBytes<=bytes.length&&sha(bytes.subarray(0,neutralAckRevision.originalPrefixBytes))===neutralAckRevision.originalPrefixHash,'REVISION_PREFIX_BINDING');}
  for(const line of bytes.toString().split('\n').filter(Boolean)){const row=JSON.parse(line),rowBytes=Buffer.byteLength(line)+1,isLegacy=neutralAckRevision!==null&&prefixSeen<neutralAckRevision.originalPrefixBytes;need(!isLegacy||prefixSeen+rowBytes<=neutralAckRevision.originalPrefixBytes,'REVISION_PARTIAL_ROW');need(payloadHash(row.event.codeIdentity)===payloadHash(isLegacy?legacyCode:code),'CODE_CHANGED');if(row.event.input.kind==='neutral_ack_revision')need(!isLegacy&&neutralAckRevision&&row.event.input.revisionHash===payloadHash(neutralAckRevision)&&row.event.input.orderId===neutralAckRevision.orderId&&row.event.input.sourceBodyHash===neutralAckRevision.ackSourceBodyHash&&row.event.input.originalPrefixHash===neutralAckRevision.originalPrefixHash&&row.event.input.originalPrefixBytes===neutralAckRevision.originalPrefixBytes,'REVISION_DESCRIPTOR_CHANGED');recoveryMember(row.event.input);state=reduce(state,row.event.input,{legacy:isLegacy});need(payloadHash(state)===payloadHash(row.state),'REPLAY_STATE');rows.push(row);prefixSeen+=rowBytes;}size=bytes.length;bytesHash=sha(bytes);stat=fs.statSync(file);
  const append=input=>{basic();const before=fs.readFileSync(file),current=fs.statSync(file);need(current.ino===stat.ino&&current.birthtimeMs===stat.birthtimeMs&&before.length===size&&sha(before)===bytesHash&&payloadHash(identity())===payloadHash(code),'JOURNAL_CHANGED');recoveryMember(input);const next=reduce(state,input);need(size+Buffer.byteLength(JSON.stringify(next))+Buffer.byteLength(JSON.stringify(input))<64*1024*1024,'JOURNAL_BOUND');
   try{journal.commit(next,{codeIdentity:code,input});basic();const all=fs.readFileSync(file);need(all.subarray(0,size).equals(before)&&all.length>size,'JOURNAL_CHANGED');rows.push(JSON.parse(all.subarray(size).toString()));state=next;size=all.length;bytesHash=sha(all);}catch(e){failed=true;throw e;}
  };
  if(!state)append({kind:'initialize',capture,scope,at:iso(now())});else need(payloadHash(state.plan.scope)===payloadHash(scope)&&(!capture||state.plan.captureHash===payloadHash(capture)),'SCOPE_OR_CAPTURE_CHANGED');
  if(neutralAckRevision&&!rows.some(r=>r.event.input.kind==='neutral_ack_revision')){const o=active(state);need(o?.orderId===neutralAckRevision.orderId&&o.ack?.sourceBodyHash===neutralAckRevision.ackSourceBodyHash,'REVISION_ORDER_BINDING');const proof=inspectNoAckAllEconomics(o.ack);append({kind:'neutral_ack_revision',index:o.index,orderId:o.orderId,sourceBodyHash:o.ack.sourceBodyHash,economicsProofHash:payloadHash(proof),revisionHash:payloadHash(neutralAckRevision),originalPrefixHash:neutralAckRevision.originalPrefixHash,originalPrefixBytes:neutralAckRevision.originalPrefixBytes,at:iso(now())});}
  const reserve=async()=>{basic();const wanted=funding(state),r=await installFundingReservation(clone(wanted));basic();append({kind:'funding',receipt:r,at:iso(now())});return r;};
  // This callback is a synchronous LOCAL cache read. Issuing another request
  // from the queue's own admission callback would deadlock the shared client.
  const evidence=(operation,o)=>{const e=getEvidence({operation,marketId:o?.marketId??state.plan.legs[state.orders.length]?.marketId,exchangeId:o?.body.exchangeId??state.plan.legs[state.orders.length]?.exchangeId,orderId:o?.orderId??null,reservation:funding(state)});need(e&&typeof e.then!=='function','EVIDENCE_MUST_BE_LOCAL');basic();return clone(e);};
  const own=id=>state.orders.some(o=>o.orderId===id);
  const recoverRow=o=>{const row=rows.find(r=>r.event.input.kind==='fetch_start'&&r.event.input.index===o.index&&r.event.input.start.kind==='place'&&instant(r.event.input.start.requestStartedAt)<instant(o.body.expirationDate));need(row&&row.event.input.start.requestBodyHash===o.bodyHash&&row.event.input.start.requestBodyJson===o.bodyJson,'ORIGINAL_WIRE_PROOF_MISSING');return clone(row);};
  const cancelGate=(o,e,at)=>{need(o.orderId&&at>=instant(o.body.expirationDate),'CANCEL_NOT_EXPIRED_OWN');const r=raw(e.order,'GET','/orders/'+o.orderId),d=parseNumberLexemes(r.bodyText).data;need(r.status===200&&instant(r.receivedAt)<=at&&at-instant(r.requestStartedAt)<=20000&&String(d.id)===o.orderId&&d.tournamentId===scope.tournamentId&&d.exchangeId===o.body.exchangeId&&d.side==='no'&&d.action==='buy'&&d.quantity===1&&d.open===true&&price(String(d.priceLimit))===price(binding(state,o).limit)&&d.expirationDate===o.body.expirationDate,'CANCEL_ORDER_PROOF');};
  const validate=async(p,spec,network)=>{basic();const o=active(state);need(o&&p.intentId===o.intentId&&p.runId===scope.runId&&p.bodyHash===o.bodyHash,'PERMIT_IDENTITY');
   if(p.operation==='cancel'){need(spec.method==='DELETE'&&spec.endpoint==='/orders/'+o.orderId,'CANCEL_SCOPE');cancelGate(o,evidence('cancel',o),now());}
   else{need(spec.method==='POST'&&spec.endpoint==='/orders'&&spec.bodyJson===o.bodyJson,'PLACEMENT_PAYLOAD');if(p.operation==='recover'){need(!o.orderId&&!o.semanticMismatch&&now()>=instant(o.body.expirationDate),'RECOVERY_NOT_ALLOWED');recoverRow(o);}else preflight(state,evidence('place',o),now(),o);}
   const l=basic();if(network){append({kind:'network_fence',index:o.index,operation:p.operation,leaseIdentity:l,leaseHash:payloadHash(l),at:iso(now())});if(p.operation==='place')preflight(state,evidence('place',active(state)),now(),active(state));else if(p.operation==='cancel')cancelGate(active(state),evidence('cancel',active(state)),now());basic();}
   else append({kind:'admitted',index:o.index,operation:p.operation,at:iso(now())});return true;
  };
  const onFetchStart=meta=>{
   if(meta?.method==='GET')return false;
   if(!inFlight)return false;
   try{basic();keys(meta,['fetchInvocationId','method','endpoint','requestBodyHash','requestStartedAt']);const o=state.orders[inFlight.index],l=o.fences.at(-1);need(l&&meta.method===inFlight.spec.method&&meta.endpoint===inFlight.spec.endpoint&&meta.requestBodyHash===(meta.method==='POST'?o.bodyHash:null),'FETCH_START_UNBOUND');
    const start={...clone(meta),kind:o.operation,ownerId:scope.ownerId,runId:scope.runId,intentId:o.intentId,idempotencyKey:o.body.idempotencyKey,requestBodyJson:o.bodyJson,requestBodyHash:o.bodyHash,leaseIdentity:l.leaseIdentity,leaseHash:l.leaseHash};append({kind:'fetch_start',index:o.index,start,at:iso(now())});return true;
   }catch(error){failed=true;throw error;}
  };
  const wrap=async spec=>{
   const o=active(state),index=o.index;issued.set(spec.beforeDispatch,{endpoint:spec.endpoint,method:spec.method,bodyJson:spec.bodyJson,beforeNetwork:spec.beforeNetwork});inFlight={index,spec};
   const captureWire=async r=>{const record=raw(await rawReceiptOf(r,{method:spec.method,requestBodyHash:spec.method==='POST'?o.bodyHash:null}),spec.method,spec.endpoint);need(instant(record.requestStartedAt)>=instant(r.requestStartedAt)&&instant(record.receivedAt)<=instant(r.receivedAt)&&record.status===r.status,'WIRE_CLOCK_RELABELED');if(r.data!==undefined)need(payloadHash(parseNumberLexemes(record.bodyText).data)===payloadHash(r.data),'WIRE_BODY_CHANGED');const current=state.orders[index],l=current.fences.at(-1);need(l,'NETWORK_FENCE_MISSING');
    const wire={kind:current.operation,method:spec.method,endpoint:spec.endpoint,ownerId:scope.ownerId,runId:scope.runId,intentId:current.intentId,idempotencyKey:current.body.idempotencyKey,requestBodyJson:current.bodyJson,requestBodyHash:current.bodyHash,leaseIdentity:l.leaseIdentity,leaseHash:l.leaseHash,fetchInvocationId:record.fetchInvocationId,requestStartedAt:record.requestStartedAt,receivedAt:record.receivedAt,status:record.status,sourceBodyHash:record.sourceBodyHash,bodyText:record.bodyText};append({kind:'wire',index,wire,at:iso(now())});};
   try{const r=await dispatch(spec);await captureWire(r);return r;}catch(error){if(error.wireReceipt){await captureWire(error.wireReceipt);}throw error;}finally{issued.delete(spec.beforeDispatch);inFlight=null;}
  };
  const transport=new OrderTransport({scope:{tournamentId:scope.tournamentId,tournamentSlug:scope.tournamentSlug,exchanges:scope.markets},dispatch:wrap,canWrite:()=>{basic();return true;},ownsOrder:own,validatePermit:(p,s)=>validate(p,s,false),validateNetworkPermit:(p,s)=>validate(p,s,true),validateLease:l=>payloadHash(l)===payloadHash(basic()),now});
  const authorizes=spec=>{try{basic();const p=issued.get(spec?.beforeDispatch);return !!p&&p.method===spec.method&&p.endpoint===spec.endpoint&&p.bodyJson===spec.bodyJson&&p.beforeNetwork===spec.beforeNetwork&&transport.authorizeRequest(spec)===true;}catch{return false;}};
  const run=async fn=>{need(!busy,'BUSY');busy=true;try{return await fn();}finally{busy=false;}};
  const status=()=>{const o=active(state),owned=state.orders.flatMap(x=>x.proof?.lot?[{marketId:x.marketId,exchangeId:x.body.exchangeId,...x.proof.lot}]:[]),hedgeBlocked=!o&&!ended(state)&&owned.length===1&&now()>=instant(state.plan.reviewExpiresAt);return{mode:'paired_rest_lifecycle_v2',planHash:state.plan.planHash,originalDryPlanHash:state.plan.originalDryPlanHash,originalProtectiveCost:state.plan.originalProtectiveCost,pairCostCeiling:state.plan.pairCostCeiling,minimumOrdinaryMargin:state.plan.sourceMinimumOrdinaryMargin,planLegs:state.plan.legs.map(({marketId,exchangeId,originalLimit})=>({marketId,exchangeId,originalLimit})),reviewExpiresAt:state.plan.reviewExpiresAt,sequence:journal.sequence,hash:journal.previous,closed,failed,busy,phase:o?.phase??(ended(state)?owned.length===2?'paired':owned.length?'orphaned':'unfilled':hedgeBlocked?'hedge_blocked':'ready'),blockedReason:hedgeBlocked?'rules_expired':null,funding:funding(state),owned,orders:state.orders.map(x=>({intentId:x.intentId,orderId:x.orderId,marketId:x.marketId,exchangeId:x.body.exchangeId,phase:x.phase,body:clone(x.body),bodyHash:x.bodyHash,cancelPending:x.cancelPending,actualWireStarts:x.wireAttempts.map(w=>w.requestStartedAt),lastActualFetchStartAt:x.fetchStarts.at(-1)?.requestStartedAt??null,actualFetchCount:x.fetchStarts.length,recoveryAttemptCount:x.fetchStarts.filter(w=>w.kind==='recover').length,semanticMismatch:x.semanticMismatch,proofHash:x.proof?.verificationHash??null})),globalActivity:state.orders.map(x=>x.proof?.globalActivity.status??'unknown'),additionalApiClients:0};};
  const dispatchOrder=operation=>run(async()=>{basic();const o=active(state);need(o,'NO_ACTIVE_ORDER');if(operation==='place')need(o.phase==='prepared','UNKNOWN_BLOCKS_REPLACEMENT');if(operation==='recover')recoverRow(o);await reserve();const lease=basic(),permit={intentId:o.intentId,runId:scope.runId,bodyHash:o.bodyHash,operation,...(operation==='recover'?{retry:true}:{})};
   try{if(operation==='cancel')await transport.cancel(o.orderId,{permit,lease});else await transport.place(o.body,{permit,lease});}
   catch(error){if(failed)throw Object.assign(Error('PAIRED_REST_DURABLE_RECORDING_FAILED'),{code:'PAIRED_REST_DURABLE_RECORDING_FAILED',fatal:true,durableRecordingFailed:true,mayHaveExecuted:true});if(canOperate()===true)append({kind:'error',index:o.index,code:/^[A-Z_]+$/.test(error.code??'')?error.code:'REQUEST_UNCONFIRMED',at:iso(now())});throw error;}
   return status();
  });
  return{authorizes,onFetchStart,status,fundingReservation:()=>clone(funding(state)),
   prepareNext:()=>run(async()=>{basic();need(!active(state)&&!ended(state),'ACTIVE_OR_FINISHED');await reserve();const e=await evidence('prepare',null),index=state.orders.length;append({kind:'prepared',index,intentId:'paired-'+randomUUID(),idempotencyKey:randomUUID(),evidence:e,at:iso(now())});return status();}),
   dispatchPrepared:()=>dispatchOrder('place'),recoverExact:()=>dispatchOrder('recover'),cancelExpired:()=>dispatchOrder('cancel'),
   reconcile:input=>run(async()=>{basic();const o=active(state);need(o?.orderId&&o.ack,'ORDER_ACK_REQUIRED');const recoveryProof=instant(o.ack.requestStartedAt)>=instant(o.body.expirationDate)?{journalRecord:recoverRow(o)}:null;append({kind:'reconciled',index:o.index,evidence:input,recoveryProof,at:iso(now())});await reserve();return status();}),
   close(){need(!busy,'BUSY');if(closed)return;closed=true;local.release();}
  };
 }catch(e){local.release();throw e;}
}
