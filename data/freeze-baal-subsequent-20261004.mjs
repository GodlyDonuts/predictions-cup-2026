import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
const prior=path.join(runtime,'research-cohorts','baal-net-20261004-e');
const grid=JSON.parse(fs.readFileSync(path.join(prior,'native-dip-grid-v2.json'),'utf8'));
assert.equal(grid.capture.source_hash,'a19efc0c0ff5d52f8ce851ce5665631d3e3c76899e3e46c3dbc7653723c62507');
assert.equal(grid.capture.source_bytes,4255286972);assert.equal(grid.capture.source_records,9491);
const positives=grid.sweep.scorecards.filter(c=>Number(c.points.at(-1).conservativePnl)>0);assert.equal(positives.length,9);
const baseline=grid.sweep.scorecards.filter(c=>c.parameters.gapResetSeconds===120).sort((a,b)=>Number(b.points.at(-1).conservativePnl)-Number(a.points.at(-1).conservativePnl))[0];
const cohort='baal-dip-subsequent-20261004-v1',dir=path.join(runtime,'research-cohorts',cohort);assert(!fs.existsSync(dir));fs.mkdirSync(dir,{mode:0o700});
const config=JSON.parse(fs.readFileSync(path.join(prior,'native-sweep-config.json'),'utf8'));assert.equal(config.feeBps,0);
const declaration={schemaVersion:1,mode:'causal_dip_subsequent_v1',cohort,declaredAt:new Date().toISOString(),config,
 development:{records:9491,bytes:4255286972,hash:grid.capture.source_hash,through:new Date(grid.capture.through).toISOString()},
 strategies:[...positives.map((c,i)=>({id:'development-positive-'+(i+1),parameters:c.parameters})),{id:'short-gap-control',parameters:baseline.parameters}],
 scenarios:[{id:'baseline',latencyMs:3000,slippageTicks:1,depthFractionBps:100},{id:'fast-read',latencyMs:1000,slippageTicks:1,depthFractionBps:100},{id:'slow-read',latencyMs:15000,slippageTicks:1,depthFractionBps:100},{id:'two-tick-cost',latencyMs:3000,slippageTicks:2,depthFractionBps:100},{id:'half-depth',latencyMs:3000,slippageTicks:1,depthFractionBps:50}],stopBufferTicks:2};
const declarationBytes=JSON.stringify(declaration,null,2)+'\n';fs.writeFileSync(path.join(dir,'declaration.json'),declarationBytes,{flag:'wx',mode:0o600});
const journal=path.join(runtime,'journal.jsonl'),fd=fs.openSync(journal,'r');let end,sha;
try{
 const size=fs.fstatSync(fd).size,tailLength=Math.min(size,8*1024*1024),tail=Buffer.alloc(tailLength);assert.equal(fs.readSync(fd,tail,0,tailLength,size-tailLength),tailLength);
 const line=tail.lastIndexOf(10);assert(line>=0);end=size-tailLength+line+1;
 const digest=createHash('sha256'),buffer=Buffer.alloc(4*1024*1024);for(let at=0;at<end;){const n=fs.readSync(fd,buffer,0,Math.min(buffer.length,end-at),at);assert(n>0);digest.update(buffer.subarray(0,n));at+=n;}sha=digest.digest('hex');
}finally{fs.closeSync(fd);}
const binding={at:new Date().toISOString(),cohort,endByte:end,sourceHash:sha,declarationHash:createHash('sha256').update(declarationBytes).digest('hex'),selectionSourceHash:grid.capture.source_hash,selectionSourceBytes:grid.capture.source_bytes,selectionSourceRecords:grid.capture.source_records,candidates:declaration.strategies.length*declaration.scenarios.length*2,marketDataWasCollectedEarlier:true,subsequentPerformanceNotInspectedBeforeDeclaration:true,evaluationType:'retrospective_subsequent_data_not_live_prospective_trading',competitionOrders:0,networkRequests:0};
fs.writeFileSync(path.join(dir,'source-binding.json'),JSON.stringify(binding,null,2)+'\n',{flag:'wx',mode:0o600});
fs.writeFileSync('data/baal-subsequent-binding-20261004.json',JSON.stringify(binding,null,2)+'\n');console.log(JSON.stringify(binding));
