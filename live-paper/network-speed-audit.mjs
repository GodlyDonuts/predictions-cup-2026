import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {fileURLToPath} from 'node:url';
import {validateTiming,timingSummary} from './read-timing.mjs';
export function auditNetworkSpeed(raw,installedAt,{beforeWindowMs=600000}={}){
 const cutoff=Date.parse(installedAt);if(!Number.isFinite(cutoff)||!Number.isSafeInteger(beforeWindowMs)||beforeWindowMs<=0)throw Error('Invalid measurement window');
 const sources=Array.isArray(raw)?raw:[raw],hash=createHash('sha256');let sourceBytes=0,partialTailExcluded=false;const events=[];
 for(const source of sources){
  if(typeof source!=='string')throw Error('Invalid timing source');
  const end=source.lastIndexOf('\n')+1,complete=source.slice(0,end);hash.update(complete);sourceBytes+=Buffer.byteLength(complete);partialTailExcluded||=end!==source.length;
  if(complete.trim())for(const line of complete.trimEnd().split('\n'))events.push(validateTiming(JSON.parse(line)));
 }
 // Primary rows and spill rows can overlap in file order after a restart. For
 // multi-file audits, use the event timestamp as the chronology; preserve the
 // legacy row order and hash for a single-file audit.
 if(sources.length>1)events.sort((a,b)=>Date.parse(a.finishedAt)-Date.parse(b.finishedAt));
 const before=events.filter(e=>Date.parse(e.queuedAt)>=cutoff-beforeWindowMs&&Date.parse(e.finishedAt)<cutoff),after=events.filter(e=>Date.parse(e.queuedAt)>=cutoff);
 const admitted=after.filter(e=>e.admittedAt!==null).sort((a,b)=>Date.parse(a.admittedAt)-Date.parse(b.admittedAt));
 let maximumRequestsInInclusiveMinute=0,minimumCompletionGapMs=null;for(let i=0,j=0;i<admitted.length;i++){
  const at=Date.parse(admitted[i].admittedAt);while(at-Date.parse(admitted[j].admittedAt)>60000)j++;maximumRequestsInInclusiveMinute=Math.max(maximumRequestsInInclusiveMinute,i-j+1);
  if(i){const gap=at-Date.parse(admitted[i-1].finishedAt);minimumCompletionGapMs=minimumCompletionGapMs===null?gap:Math.min(gap,minimumCompletionGapMs);}
 }
 return {schemaVersion:1,sourceHash:hash.digest('hex'),sourceBytes,partialTailExcluded,installedAt,beforeWindowMs,before:timingSummary(before),after:timingSummary(after),firstNewAdmissionAt:admitted[0]?.admittedAt??null,lastFinishedAt:after.at(-1)?.finishedAt??null,maximumRequestsInInclusiveMinute,minimumCompletionGapMs,networkRequests:0,
  limitations:['Descriptive windows differ in traffic mix and duration; this is not a controlled strategy-performance comparison.','Timing records do not identify HTTP method; observed request totals cannot prove the write-specific budget.','Write pacing is established by configuration and separate rolling-limit/restart tests.','API admission delay is not signal-to-fill latency or evidence of profit.']};
}
export function readNetworkTimingSourcesV1(runtimeDir,{filesystem=fs}={}){
 const primary=path.join(runtimeDir,'read-timings.jsonl');
 const spillPattern=/^read-timings\.spill-\d{17}-\d+-[a-z0-9]+\.jsonl$/;
 const spills=filesystem.readdirSync(runtimeDir).filter(name=>spillPattern.test(name)).sort();
 return [filesystem.readFileSync(primary,'utf8'),...spills.map(name=>filesystem.readFileSync(path.join(runtimeDir,name),'utf8'))];
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),proof=JSON.parse(fs.readFileSync('data/network-speed-deployment.json'));
 const report=auditNetworkSpeed(readNetworkTimingSourcesV1(rt),proof.installedAt);fs.writeFileSync('data/network-speed-live-timing.json',JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report));
}
