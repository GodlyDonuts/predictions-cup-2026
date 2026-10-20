// Inert source-time price index. No quotes, fills, or backdated delivery clock.
import fs from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';
import {fileURLToPath,pathToFileURL} from 'node:url';
import assert from 'node:assert/strict';
const sha=x=>crypto.createHash('sha256').update(x).digest('hex');
export const RECORD_BYTES=24;
export function encode(points){
 const out=Buffer.alloc(points.length*RECORD_BYTES);let prior=-1;
 for(let i=0;i<points.length;i++){
  const {timeMs,price,receipt}=points[i];
  if(!Number.isSafeInteger(timeMs)||timeMs<0||timeMs<=prior||typeof price!=='number'||!Number.isFinite(price)||price<0||price>1||!Number.isInteger(receipt)||receipt<0||receipt>0xffffffff)throw Error('SCOPED_SORTED_SAMPLE_REQUIRED');
  prior=timeMs;const n=i*RECORD_BYTES;out.writeBigInt64LE(BigInt(timeMs),n);out.writeDoubleLE(price,n+8);out.writeUInt32LE(receipt,n+16);out.writeUInt32LE(1,n+20);
 }return out;
}
export function sampleAtSourceTime(bytes,timeMs){
 if(bytes.length%RECORD_BYTES||!Number.isSafeInteger(timeMs))throw Error('SOURCE_INDEX_SHAPE');
 let lo=0,hi=bytes.length/RECORD_BYTES;
 while(lo<hi){const mid=Math.floor((lo+hi)/2);if(bytes.readBigInt64LE(mid*RECORD_BYTES)<=BigInt(timeMs))lo=mid+1;else hi=mid;}
 if(!lo)return null;const n=(lo-1)*RECORD_BYTES;
 return {sourceTimeMs:Number(bytes.readBigInt64LE(n)),price:bytes.readDoubleLE(n+8),receipt:bytes.readUInt32LE(n+16),sampleOnly:bytes.readUInt32LE(n+20)===1,originalDeliveryVerified:false};
}
export async function build(root,output,auditFile,helperFile){
 const started=performance.now(),planRaw=await fs.readFile(path.join(root,'plan.json')),plan=JSON.parse(planRaw),auditRaw=await fs.readFile(auditFile),audit=JSON.parse(auditRaw);
 if(!['training_history_recovery_only','evolution_history_recovery_only'].includes(plan.role)||plan.role!==audit.role||sha(planRaw)!==audit.planSha256||audit.trainingAdmitted!==false)throw Error('AUDITED_ROLE_PLAN_REQUIRED');
 assert.equal(sha(await fs.readFile(helperFile)),plan.runnerSha256);
 const {scopedPoints}=await import(pathToFileURL(path.resolve(helperFile)));
 await fs.mkdir(output);const assets=[],receipts=[],inventory=crypto.createHash('sha256'),normal=crypto.createHash('sha256');let total=0;
 for(let i=0;i<plan.assets.length;i++){
  const asset=plan.assets[i],seen=new Map();let chunk=0;
  for(let start=plan.startTs;start<plan.endTs;start+=7*86400,chunk++){
   const end=Math.min(plan.endTs,start+7*86400),name=`${String(i).padStart(4,'0')}-${chunk}.json`,raw=await fs.readFile(path.join(root,'pages',name)),r=JSON.parse(raw),digest=sha(raw),receipt=receipts.length;
   inventory.update(name);inventory.update(Buffer.from(digest,'hex'));
   receipts.push({file:name,sha256:digest,bodySha256:r.bodySha256,requestStartedAt:r.requestStartedAt,actualReceivedAt:r.receivedAt,assetId:asset,startTs:start,endExclusiveTs:end});
   for(const point of scopedPoints(r,asset,start,end)){
    if(seen.has(point.t)){assert.equal(seen.get(point.t).price,point.p);continue;}
    seen.set(point.t,{timeMs:point.t*1000,price:point.p,receipt});
   }
  }
  const points=[...seen.values()].sort((a,b)=>a.timeMs-b.timeMs),bytes=encode(points),name=`asset-${String(i).padStart(4,'0')}.bin`;
  for(const p of points)normal.update(JSON.stringify([asset,p.timeMs/1000,p.price])+'\n');
  await fs.writeFile(path.join(output,name),bytes,{flag:'wx'});assert.equal(sha(await fs.readFile(path.join(output,name))),sha(bytes));
  assets.push({assetId:asset,file:name,sha256:sha(bytes),records:points.length});total+=points.length;
 }
 const normalized=normal.digest('hex'),receiptInventory=inventory.digest('hex');
 assert.equal(normalized,audit.normalizedScopedSampleSha256);assert.equal(receiptInventory,audit.sortedReceiptInventorySha256);assert.equal(total,audit.uniqueInRangeSamples);
 const receiptRaw=JSON.stringify(receipts);await fs.writeFile(path.join(output,'receipts.json'),receiptRaw,{flag:'wx'});
 const manifest={classification:'inert_source_time_price_samples_not_replay_dataset',role:plan.role,startTs:plan.startTs,endExclusiveTs:plan.endTs,recordBytes:RECORD_BYTES,layout:'int64_le_source_ms,float64_le_original_JS_price,uint32_le_receipt,uint32_le_sample_only_flag',assets,records:total,receiptsSha256:sha(receiptRaw),sourceAuditSha256:sha(auditRaw),sourcePlanSha256:sha(planRaw),normalizedScopedSampleSha256:normalized,receiptInventorySha256:receiptInventory,buildWallMs:performance.now()-started,trainingAdmitted:false,bookDepthReconstructed:false,originalHistoricalDeliveryVerified:false,requiresExplicitCausalDeliveryProfile:true,independentTestUsed:false};
 await fs.writeFile(path.join(output,'complete.json'),JSON.stringify(manifest,null,2)+'\n',{flag:'wx'});
 return manifest;
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 build(...process.argv.slice(2)).then(m=>console.log(JSON.stringify({records:m.records,assets:m.assets.length,buildWallMs:m.buildWallMs,trainingAdmitted:false}))).catch(e=>{console.error(e.message);process.exitCode=1;});
}
