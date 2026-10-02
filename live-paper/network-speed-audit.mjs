import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {fileURLToPath} from 'node:url';
import {validateTiming,timingSummary} from './read-timing.mjs';
export function auditNetworkSpeed(raw,installedAt,{beforeWindowMs=600000}={}){
 const cutoff=Date.parse(installedAt);if(!Number.isFinite(cutoff)||!Number.isSafeInteger(beforeWindowMs)||beforeWindowMs<=0)throw Error('Invalid measurement window');
 const end=raw.lastIndexOf('\n')+1,complete=raw.slice(0,end),events=complete.trim()?complete.trimEnd().split('\n').map(x=>validateTiming(JSON.parse(x))):[];
 const before=events.filter(e=>Date.parse(e.queuedAt)>=cutoff-beforeWindowMs&&Date.parse(e.finishedAt)<cutoff),after=events.filter(e=>Date.parse(e.queuedAt)>=cutoff);
 const admitted=after.filter(e=>e.admittedAt!==null).sort((a,b)=>Date.parse(a.admittedAt)-Date.parse(b.admittedAt));
 let maximumRequestsInInclusiveMinute=0,minimumCompletionGapMs=null;for(let i=0,j=0;i<admitted.length;i++){
  const at=Date.parse(admitted[i].admittedAt);while(at-Date.parse(admitted[j].admittedAt)>60000)j++;maximumRequestsInInclusiveMinute=Math.max(maximumRequestsInInclusiveMinute,i-j+1);
  if(i){const gap=at-Date.parse(admitted[i-1].finishedAt);minimumCompletionGapMs=minimumCompletionGapMs===null?gap:Math.min(gap,minimumCompletionGapMs);}
 }
 return {schemaVersion:1,sourceHash:createHash('sha256').update(complete).digest('hex'),sourceBytes:Buffer.byteLength(complete),partialTailExcluded:end!==raw.length,installedAt,beforeWindowMs,before:timingSummary(before),after:timingSummary(after),firstNewAdmissionAt:admitted[0]?.admittedAt??null,lastFinishedAt:after.at(-1)?.finishedAt??null,maximumRequestsInInclusiveMinute,minimumCompletionGapMs,networkRequests:0,
  limitations:['Descriptive windows differ in traffic mix and duration; this is not a controlled strategy-performance comparison.','Timing records do not identify HTTP method; observed request totals cannot prove the write-specific budget.','Write pacing is established by configuration and separate rolling-limit/restart tests.','API admission delay is not signal-to-fill latency or evidence of profit.']};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),proof=JSON.parse(fs.readFileSync('data/network-speed-deployment.json'));
 const report=auditNetworkSpeed(fs.readFileSync(path.join(rt,'read-timings.jsonl'),'utf8'),proof.installedAt);fs.writeFileSync('data/network-speed-live-timing.json',JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report));
}
