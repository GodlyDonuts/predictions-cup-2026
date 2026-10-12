import fs from 'node:fs';import crypto from 'node:crypto';
import {payloadHash} from '../../execution/core.mjs';
import {amount,money,price,SCALE,TICK,instant} from '../../paper/fixed.mjs';
const base=new URL('../../',import.meta.url),read=p=>{const bytes=fs.readFileSync(new URL(p,base));return{file:p,sha256:crypto.createHash('sha256').update(bytes).digest('hex'),data:JSON.parse(bytes)};};
const archive=read('data/paired-role-public-receipts-20261012.json'),report=read('data/paired-role-comparison-20261012.json'),a=archive.data,r=report.data,m=r.manifest;
const need=(value,reason)=>{if(!value)throw Error(reason);};
need(a.captures.length===3&&r.finiteCaptures===3&&r.actualOrders===0&&a.actualOrders===0,'ARCHIVE_SCOPE');
need(m.quantityPerLeg===1&&m.depthFractionBps===100&&m.fees==='0.000000'&&m.latencyMs===3000,'DECLARED_MODEL_SCOPE');
need(m.roles.length===1&&m.roles[0]==='marketable'&&r.cases.length===2&&r.cases.every(c=>[0,1].includes(c.adverseTicks)),'ROLE_SCENARIOS');
const [entry,fill,mark]=a.captures,decision=instant(r.decisionAt);
need(instant(m.startedAt)<instant(entry.at)&&instant(entry.at)<=decision&&decision<instant(fill.at)&&instant(fill.at)<instant(mark.at),'CAPTURE_CHRONOLOGY');
const clockChecks=[],roots=new Map(entry.roots.map(x=>[x.marketId,x.rootHash]));
for(const[index,capture]of a.captures.entries()){
 need(capture.phase==='complete'&&capture.books.length===2&&capture.roots.length===2&&new Set(capture.books.map(x=>x.book.marketId)).size===2&&new Set(capture.roots.map(x=>x.marketId)).size===2,'COMPLETE_DISTINCT_CAPTURE');
 need(capture.originalCaptureHash===r.sourceHashes[index]&&capture.at===r.captureClocks[index],'SOURCE_HASH_BINDING');
 for(const wrapper of capture.books){const b=wrapper.book,root=capture.roots.find(x=>x.marketId===b.marketId);
  need(root&&payloadHash(root.root)===root.rootHash&&root.rootHash===roots.get(b.marketId),'RULE_ROOT_IDENTITY');
  const start=instant(b.requestStartedAt),end=instant(b.receivedAt),at=instant(capture.at),rootStart=instant(root.requestStartedAt),rootEnd=instant(root.receivedAt);
  need(rootStart<=rootEnd&&rootEnd<=start&&start<=end&&end<=at&&at-start<=20000,'RECEIPT_CLOCK');
  need(b.marketId===wrapper.marketId&&m.marketIds.includes(b.marketId)&&typeof b.versionKey==='string','BOOK_SCOPE');
  if(index===0)need(decision-start<=20000&&end<=decision,'DECISION_RECEIPT_FRESHNESS');
  if(index===1)need(start>=decision+m.latencyMs&&end<decision+m.lifetimeMs,'INDEPENDENT_UNEXPIRED_FILL_CLOCK');
  if(index===2)need(start>=instant(fill.at)+m.latencyMs,'INDEPENDENT_LIQUIDATION_CLOCK');
  for(const[side,descending]of [['bids',true],['asks',false]]){let previous=descending?SCALE:0n;for(const level of b[side]){const p=price(level.price);need((descending?p<previous:p>previous)&&Number.isSafeInteger(level.quantity)&&level.quantity>0,'DEPTH_ORDER');previous=p;}}
  clockChecks.push({capture:index+1,marketId:b.marketId,rootHash:root.rootHash,requestStartedAt:b.requestStartedAt,receivedAt:b.receivedAt,rootKnownBeforeBook:true,observationAgeAtCaptureMs:at-start});
 }
}
function oneSharePrice(book,side){let cumulative=0n;for(const level of book[side]){cumulative+=BigInt(level.quantity);if(cumulative/100n>=1n)return SCALE-price(level.price);}throw Error('INSUFFICIENT_ONE_PERCENT_DEPTH');}
const checks=r.cases.map(c=>{
 let principal=0n,liquidation=0n,reservation=0n;
 const legs=m.marketIds.map(marketId=>{const entryBook=entry.books.find(x=>x.book.marketId===marketId).book,fillBook=fill.books.find(x=>x.book.marketId===marketId).book,markBook=mark.books.find(x=>x.book.marketId===marketId).book;
  need(entryBook.exchangeId===fillBook.exchangeId&&fillBook.exchangeId===markBook.exchangeId,'EXCHANGE_SCOPE');
  const limit=SCALE-price(entryBook.bids[0].price)+TICK,raw=oneSharePrice(fillBook,'bids'),fillPrice=raw+BigInt(c.adverseTicks)*TICK;
  need(fillPrice<=limit,'PROTECTED_FILL_PRICE');principal+=fillPrice;reservation+=limit;const exit=oneSharePrice(markBook,'asks')-TICK;need(exit>=TICK,'LIQUIDATION_PRICE');liquidation+=exit;
  const published=c.legs.find(x=>x.marketId===marketId);need(published?.modeledQuantity===1&&amount(published.modeledNotional)===fillPrice&&amount(published.reservedCash)===limit,'REPORTED_LEG_ARITHMETIC');
  return{marketId,exchangeId:fillBook.exchangeId,postedLimit:money(limit),independentLaterPrice:money(fillPrice),independentLaterLiquidation:money(exit)};
 });
 const cash=amount(m.initialCash)-principal,pnl=liquidation-principal;
 need(reservation<=amount(m.initialCash)&&principal<=reservation&&cash>=0n,'SHARED_FUNDING');
 need(money(principal)===c.modeledPrincipal&&money(cash)===c.cash&&money(pnl)===c.datedStressedLiquidationPnl,'REPORTED_WEALTH_REPRODUCTION');
 return{adverseTicks:c.adverseTicks,reservedPrincipal:money(reservation),modeledPrincipal:money(principal),cash:money(cash),liquidationValue:money(liquidation),datedCounterfactualLiquidationPnl:money(pnl),ordinaryConditionalSettlementSurplus:money(SCALE-principal),legs};
});
const codeChecks=Object.entries(m.codeIdentity).map(([file,expected])=>{const raw=fs.readFileSync(new URL(file,base)),hash=x=>crypto.createHash('sha256').update(x).digest('hex');return{file,expectedSHA256:expected,rawMatches:hash(raw)===expected,exactRecordedLfBytesMatch:hash(Buffer.from(raw.toString().replace(/\r\n/g,'\n')))===expected};});
const output={schemaVersion:1,at:new Date().toISOString(),mode:'independent_public_receipt_arithmetic_review',sources:[archive,report].map(({file,sha256})=>({file,sha256})),sourceBindingsMatch:true,originalFullCaptureHashesReconstructed:false,accountEvidenceOmitted:true,clockChecks,costChecks:checks,codeChecks,
 frozenProducerCodeIdentityReproduced:codeChecks.every(c=>c.rawMatches||c.exactRecordedLfBytesMatch),actualFillProven:false,actualOrders:0,accountRequests:0,profitabilityEstablished:false,currentReceiptsFresh:false,
 findings:['Recorded public roots/scopes/decision/fill/mark clocks and uniform-price funding reproduce the report.','Both independent later mark scenarios reproduce negative counterfactual immediate liquidation results.','Public projection binds original capture IDs but cannot reconstruct the full omitted-account capture hashes.','Two recorded producer source identities remain unresolved; requested exact versions from Rhaegal.','One percent cumulative-depth participation, zero fees and marketable fill price are assumptions, not verified actual queue fills or fees.','The ordinary settlement payout floor and administrative exceptions remain independently unverified.']};
fs.writeFileSync(new URL('results.json',import.meta.url),JSON.stringify(output,null,2)+'\n');
console.log(JSON.stringify({rootsVerified:clockChecks.length,cases:checks.map(c=>({adverseTicks:c.adverseTicks,principal:c.modeledPrincipal,liquidationPnl:c.datedCounterfactualLiquidationPnl})),frozenCodeVerified:output.frozenProducerCodeIdentityReproduced},null,2));
