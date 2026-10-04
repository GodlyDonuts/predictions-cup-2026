import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
const prior=path.join(runtime,'research-cohorts','baal-net-20261004-e');
const grid=JSON.parse(fs.readFileSync(path.join(prior,'native-dip-grid-v2.json'),'utf8'));
assert.equal(grid.capture.source_hash,'a19efc0c0ff5d52f8ce851ce5665631d3e3c76899e3e46c3dbc7653723c62507');
assert.equal(grid.capture.source_records,9491);assert.equal(grid.capture.source_bytes,4255286972);
const positive=grid.sweep.scorecards.filter(c=>Number(c.points.at(-1).conservativePnl)>0);assert.equal(positive.length,9);
const selected=new Map();const add=(c,id)=>{const key=JSON.stringify(c.parameters);if(!selected.has(key))selected.set(key,{id,parameters:c.parameters});};
positive.forEach((c,i)=>add(c,'historical-positive-'+(i+1)));
for(const gap of [120,900,3600,21600]){
 const cards=grid.sweep.scorecards.filter(c=>c.parameters.gapResetSeconds===gap).sort((a,b)=>Number(b.points.at(-1).conservativePnl)-Number(a.points.at(-1).conservativePnl));
 add(cards[0],'gap-'+gap+'-best');add(cards[Math.floor(cards.length/2)],'gap-'+gap+'-median');add(cards.at(-1),'gap-'+gap+'-worst');
}
const journal=path.join(runtime,'journal.jsonl'),fd=fs.openSync(journal,'r'),chunks=[];let length=0,first;
try{while(!first){const chunk=Buffer.alloc(65536),n=fs.readSync(fd,chunk,0,chunk.length,length);assert(n>0);const index=chunk.subarray(0,n).indexOf(10);chunks.push(chunk.subarray(0,index<0?n:index+1));length+=index<0?n:index+1;assert(length<=8*1024*1024);if(index>=0)first=Buffer.concat(chunks);}}finally{fs.closeSync(fd);}
const firstRecord=JSON.parse(first.toString('utf8'));assert.equal(firstRecord.sequence,1);
const cohort='baal-dip-development-costs-20261004-v1',dir=path.join(runtime,'research-cohorts',cohort);assert(!fs.existsSync(dir));fs.mkdirSync(dir,{mode:0o700});
const config=JSON.parse(fs.readFileSync(path.join(prior,'native-sweep-config.json'),'utf8'));assert.equal(config.feeBps,0);
const declaration={schemaVersion:1,mode:'causal_dip_development_diagnostic_v1',cohort,declaredAt:new Date().toISOString(),config,
 development:{records:1,bytes:first.length,hash:createHash('sha256').update(first).digest('hex'),through:firstRecord.at},
 strategies:[...selected.values()],scenarios:[{id:'baseline',latencyMs:3000,slippageTicks:1,depthFractionBps:100},{id:'fast-read',latencyMs:1000,slippageTicks:1,depthFractionBps:100},{id:'slow-read',latencyMs:15000,slippageTicks:1,depthFractionBps:100},{id:'two-tick-cost',latencyMs:3000,slippageTicks:2,depthFractionBps:100},{id:'half-depth',latencyMs:3000,slippageTicks:1,depthFractionBps:50}],stopBufferTicks:2};
const bytes=JSON.stringify(declaration,null,2)+'\n';fs.writeFileSync(path.join(dir,'declaration.json'),bytes,{flag:'wx',mode:0o600});
const binding={at:new Date().toISOString(),cohort,evaluationType:'reused_development_data_cost_diagnostic_not_holdout_or_profit_proof',selectionPolicy:'all-nine-development-positive-plus-best-median-worst-per-gap-deduplicated',selectionSourceHash:grid.capture.source_hash,selectionSourceRecords:grid.capture.source_records,selectionSourceBytes:grid.capture.source_bytes,endByte:grid.capture.source_bytes,sourceHash:grid.capture.source_hash,declarationHash:createHash('sha256').update(bytes).digest('hex'),strategies:declaration.strategies.length,candidates:declaration.strategies.length*declaration.scenarios.length*2,competitionOrders:0,networkRequests:0};
fs.writeFileSync(path.join(dir,'source-binding.json'),JSON.stringify(binding,null,2)+'\n',{flag:'wx',mode:0o600});console.log(JSON.stringify(binding));
