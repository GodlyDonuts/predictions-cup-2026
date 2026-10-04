import fs from 'node:fs';
import path from 'node:path';
import {decimalMicros,payloadHash} from './core.mjs';

const TID='bda92870-621e-47b0-bc3c-3602c5c26f55';
const need=(ok,code)=>{if(!ok)throw Error('PRODUCTION_'+code);};
const hash=x=>typeof x==='string'&&/^[a-f0-9]{64}$/.test(x);
const stamp=x=>typeof x==='string'&&Number.isFinite(Date.parse(x))&&new Date(x).toISOString()===x;
const comment=x=>Number.isSafeInteger(x)&&x>0;
export function validateProductionActivation(a){
 need(a?.schemaVersion===2&&a.enabled===true&&a.ownerId==='Codex'&&a.tournamentId===TID&&/^sig-pilot-[a-z0-9-]{1,64}$/.test(a.runId)&&a.strategyId==='settlement-value-native-v1','ACTIVATION');
 need(hash(a.handoffHash)&&hash(a.strategyBankHash)&&typeof a.handoffId==='string'&&/^daenerys-to-baal-[a-z0-9-]{1,70}$/.test(a.handoffId)&&comment(a.releaseCommentId)&&comment(a.ackCommentId),'HANDOFF');
 need(a.feeBps===0&&a.depthFractionBps===100&&a.maxManagedMarkets===3,'POLICY');
 const order=decimalMicros(a.maxOrderCost),total=decimalMicros(a.maxAggregateCost),buys=decimalMicros(a.maxCumulativeBuyCost),reserve=decimalMicros(a.cashReserve),capital=decimalMicros(a.sizingCapital);
 need(order>0n&&order<=1000000n&&total>=order&&total<=10000000n&&buys>=order&&buys<=25000000n&&reserve>=0n&&capital>reserve&&capital<=100000000000n,'LIMITS');
 need(stamp(a.startedAt)&&stamp(a.entryExpiresAt)&&Date.parse(a.entryExpiresAt)>Date.parse(a.startedAt)&&Date.parse(a.entryExpiresAt)-Date.parse(a.startedAt)<=21600000,'WINDOW');
 return a;
}
export function validateProductionHandoff(h,a){
 validateProductionActivation(a);
 need(h?.schemaVersion===1&&payloadHash(h)===a.handoffHash&&h.handoffId===a.handoffId&&h.releaseCommentId===a.releaseCommentId&&h.ackCommentId===a.ackCommentId&&h.formerWriter==='Daenerys'&&h.executionOwner==='Codex'&&h.writerStopped===true&&h.inFlightResolved===true&&h.formerReadsPerMinute===0&&h.formerWritesPerMinute===0,'HANDOFF_RECORD');
 need(stamp(h.releasedAt)&&stamp(h.reconciledAt)&&stamp(h.acknowledgedAt)&&Date.parse(h.releasedAt)<=Date.parse(h.reconciledAt)&&Date.parse(h.reconciledAt)<=Date.parse(h.acknowledgedAt)&&Date.parse(h.acknowledgedAt)<=Date.parse(a.startedAt),'HANDOFF_CLOCK');
 return h;
}
export function productionHandoffCurrent(runtimeDir,a){
 try{return !!validateProductionHandoff(JSON.parse(fs.readFileSync(path.join(runtimeDir,'production-handoff.json'),'utf8')),a);}catch{return false;}
}
