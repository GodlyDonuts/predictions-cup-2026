import fs from 'node:fs';import path from 'node:path';import {journalLines} from '../live-paper/journal-lines.mjs';import {sealedNativePrefix} from '../live-paper/native-dip-host-v1.mjs';import {atomicJson} from '../execution/ledger.mjs';
export function rankFlowRecoveryV1(trades,{at,members,windowMs=900000}){
 const now=Date.parse(at);if(!Number.isFinite(now)||!(members instanceof Map))throw Error('FLOW_PRIORITY_POLICY');const groups=new Map(),seen=new Set();
 for(const t of trades){const executed=Date.parse(t.executedAt),observed=Date.parse(t.observedAt);if(members.get(t.marketId)!==t.exchangeId||!Number.isSafeInteger(t.sequence)||t.sequence<0||typeof t.id!=='string'||!Number.isSafeInteger(t.quantity)||t.quantity<1||!Number.isFinite(executed)||!Number.isFinite(observed)||executed>now||observed>now||now-executed>windowMs||observed-executed>5000||observed<executed-1000)continue;
  const key=[t.marketId,t.exchangeId,t.id,t.sequence].join(':');if(seen.has(key))continue;seen.add(key);let g=groups.get(t.marketId);if(!g){g={marketId:t.marketId,trades:0,shares:0};groups.set(t.marketId,g);}g.trades++;g.shares+=t.quantity;
 }
 return [...groups.values()].filter(g=>g.trades>=3).map(g=>({...g,score:Math.log1p(g.trades)*Math.sqrt(g.shares)})).sort((a,b)=>b.score-a.score||a.marketId.localeCompare(b.marketId)).slice(0,4);
}
let cache=null;
export function publishFlowRecoveryPriorityV1({runtimeDir,now=Date.now}){
 const session=JSON.parse(fs.readFileSync(path.join(runtimeDir,'realtime-session.json')));if(!/^realtime-[A-Za-z0-9.-]+\.jsonl$/.test(session.file))throw Error('FLOW_SOURCE_PATH');const file=path.join(runtimeDir,session.file),seal=sealedNativePrefix(file),members=new Map(session.scope.markets.map(m=>[m.marketId,m.exchangeId]));
 if(!cache||cache.file!==file)cache={file,dev:seal.dev,ino:seal.ino,bytes:0,index:0,trades:[],lastObservedAt:null};if(cache.dev!==seal.dev||cache.ino!==seal.ino||seal.bytes<cache.bytes)throw Error('FLOW_SOURCE_REPLACED');
 const next={...cache,trades:[...cache.trades]};for(const line of journalLines(file,{startByte:cache.bytes,endByte:seal.bytes})){const r=JSON.parse(line);if(r.format!=='sig-feed-capture'||r.captureIndex!==next.index++)throw Error('FLOW_CAPTURE_CHAIN');next.bytes+=Buffer.byteLength(line)+1;if(Number.isFinite(Date.parse(r.observedAt)))next.lastObservedAt=r.observedAt;if(r.kind==='trade_observed'&&r.trade?.tournamentId===session.scope.tournamentId)next.trades.push({...r.trade,observedAt:r.observedAt});}
 const at=new Date(now()).toISOString();next.trades=next.trades.filter(t=>Date.parse(t.executedAt)>=Date.parse(at)-900000);const ranked=rankFlowRecoveryV1(next.trades,{at,members});const report={schemaVersion:1,at:next.lastObservedAt??at,computedAt:at,marketIds:ranked.map(g=>g.marketId),ranked,source:{file:session.file,bytes:next.bytes,captureRecords:next.index},readOnlyRecoveryPriority:true,networkRequests:0,executionAuthority:false};atomicJson(path.join(runtimeDir,'flow-recovery-priority-v1.json'),report);cache=next;return report;
}
export function readFlowRecoveryPriorityV1({runtimeDir,scope,now=Date.now}){
 try{const r=JSON.parse(fs.readFileSync(path.join(runtimeDir,'flow-recovery-priority-v1.json'))),age=now()-Date.parse(r.at),members=new Set(scope.markets.map(m=>m.marketId));if(r.schemaVersion!==1||r.executionAuthority!==false||!Number.isFinite(age)||age<0||age>120000||!Array.isArray(r.marketIds)||r.marketIds.length>4||new Set(r.marketIds).size!==r.marketIds.length||r.marketIds.some(id=>!members.has(id)))return null;return{at:r.at,marketIds:r.marketIds};}catch{return null;}
}
