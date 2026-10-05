import fs from 'node:fs';
import path from 'node:path';
import {performance} from 'node:perf_hooks';
import {createHash} from 'node:crypto';
import {journalLines} from '../live-paper/journal-lines.mjs';
import {projectNativeSource} from '../live-paper/native-source-projection.mjs';
const sha=b=>createHash('sha256').update(b).digest('hex');
const runtimeDir=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),binding=JSON.parse(fs.readFileSync('data/coupled-rust-benchmark-20261005.json')).binding,results=[];
for(const name of ['anonymous-trades.jsonl','market-prices.jsonl']){
 const file=path.join(runtimeDir,name),endByte=fs.statSync(file).size,start=performance.now(),prefix=createHash('sha256');let sequence=0,previous='';
 for(const line of journalLines(file,{endByte})){prefix.update(line+'\n');if(!line)continue;const row=JSON.parse(line),{hash,...body}=row;if(row.sequence!==++sequence||row.previous!==previous||sha(JSON.stringify(body))!==hash)throw Error('JS_CHAIN');previous=hash;}
 const sourceHash=prefix.digest('hex'),jsMs=performance.now()-start,nativeStart=performance.now();const projected=await projectNativeSource({runtimeDir,binding,file,frontier:{sequence,hash:previous},endByte,receiptDir:path.join(runtimeDir,'rust-source-benchmark-20261005',name)});const rustMs=performance.now()-nativeStart;
 if(projected.sourceHash!==sourceHash||projected.projection.records!==0||projected.records!==sequence||projected.tailHash!==previous)throw Error('PARITY');results.push({source:name,sourceBytes:endByte,sourceHash,records:sequence,tailHash:previous,jsMs,rustMs,rustInternalVerifyMs:projected.verifyMs,speedup:jsMs/rustMs,suffixBytes:0,sourceParity:true});
}
const evidence={schemaVersion:1,observedAt:new Date().toISOString(),mode:'actual_coupled_source_frontier_same_work_v1',results,binding,networkRequests:0,competitionOrders:0,profitabilityEstablished:false};fs.writeFileSync('data/source-projection-benchmark-20261005.json',JSON.stringify(evidence,null,2)+'\n');console.log(JSON.stringify(evidence));
