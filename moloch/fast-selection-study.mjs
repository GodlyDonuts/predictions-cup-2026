import fs from 'node:fs';import path from 'node:path';
import {live} from './live.mjs';import {completedTail,reuseCompletedTail,entryPolicyTransition,flatEntryTransition,run} from './run.mjs';
import {pairedClock} from './paired-clock.mjs';import {payloadHash} from '../execution/ledger.mjs';
import {streamActivity,selectActivePairs} from './research/stream-activity.mjs';
const [parentDirectory,sourceParent,universeFile,warmNumber,studyNumber]=process.argv.slice(2);
if(!studyNumber||![warmNumber,studyNumber].every(n=>/^\d+$/.test(n)))throw Error('Usage: terminal-bank source-parent universe warm-number study-number');
const base='runtime/moloch/',warm=path.resolve(base+`live-sig-20261014-${warmNumber}`),study=path.resolve(base+`live-sig-20261014-${studyNumber}`);
const parent=completedTail(parentDirectory),exit={inventoryExitMode:'fixedGain',inventoryMinimumGain:'0.005000',inventoryTakerAfterMs:0,maximumInventoryAgeMs:1800000};
// Verify wealth before observation; do not spend the fresh selection's lifetime
// replaying the parent again. Its exact-byte proof is rechecked at each fork.
for(const insideTicks of [0,1])flatEntryTransition(structuredClone(parent.snapshot.state),true,exit,{insideTicks});
await live({marketId:'233',exchangeId:'922',outputDirectory:warm,cycles:4,resumeDirectory:sourceParent,pairsFile:base+'pairs-20261014-65.json',realtime:true,drain:true,policyFile:base+'policy-20261014-65.json',universeFile,dnsReadRecovery:true});
const stream=path.join(warm,'capture','stream'),stats=JSON.parse(fs.readFileSync(path.join(stream,'stats.json'))),manifest=JSON.parse(fs.readFileSync(path.join(stream,'manifest.json')));
if(stats.status!=='completed'||stats.failure)throw Error('FAST_SELECTION_FAILED_OBSERVATION');
const rows=fs.readFileSync(path.join(stream,'events.jsonl'),'utf8').split('\n').slice(0,stats.sequence).map(JSON.parse);let sequence=0,previous='';
for(const row of rows){const{hash,...body}=row;if(hash!==payloadHash(body)||body.sequence!==++sequence||body.previous!==previous)throw Error('FAST_SELECTION_SOURCE_CHAIN');previous=hash;}
if(sequence!==stats.sequence||previous!==stats.previous)throw Error('FAST_SELECTION_SOURCE_PREFIX');
const selected=selectActivePairs(streamActivity(rows,stats.at,manifest.pairs));if(!selected.length)throw Error('FAST_SELECTION_NO_ELIGIBLE_MARKET');
reuseCompletedTail(parent);
const pairs=selected.map(({marketId,exchangeId})=>({marketId,exchangeId})),ids=pairs.map(p=>p.exchangeId),dirs=['control','candidate'].map(side=>path.resolve(base+`fast-${side}-20261014-${studyNumber}`));
const write=(file,value)=>fs.writeFileSync(file,JSON.stringify(value,null,2)+'\n',{flag:'wx'}),pairsFile=base+`pairs-20261014-${studyNumber}.json`,policyFile=base+`policy-20261014-${studyNumber}.json`,exitFile=base+`exit-20261014-${studyNumber}.json`,entryFile=base+`entry-20261014-${studyNumber}.json`;
write(pairsFile,pairs);write(policyFile,{entryExchangeIds:ids});write(exitFile,exit);write(entryFile,{insideTicks:1});
const states=[0,1].map(()=>structuredClone(parent.snapshot.state));for(let i=0;i<2;i++){entryPolicyTransition(states[i],ids);flatEntryTransition(states[i],true,exit,i?{insideTicks:1}:null);}
write(`data/moloch-fast-registration-20261014-${studyNumber}.json`,{schemaVersion:1,registeredAt:new Date().toISOString(),parent:parent.directory,parentSnapshotHash:parent.snapshot.hash,source:study,pairs,cycles:15,comparisonMode:'entryInside',controlPolicy:states[0].policy,candidatePolicy:states[1].policy,controlPolicyHash:states[0].policyHash,candidatePolicyHash:states[1].policyHash,selectionMethod:'recent native trade count; minimum two; eligible fresh book; deterministic exchange-ID tie',selectionClock:stats.at,selectionSourceSequence:sequence,selectionSourcePrevious:previous,selected,newAccountCapital:'0',contestOrders:0,profitabilityEstablished:false});
let captureFailure=null;
const capture=live({marketId:pairs[0].marketId,exchangeId:pairs[0].exchangeId,outputDirectory:study,cycles:15,resumeDirectory:warm,pairsFile,realtime:true,drain:true,policyFile,universeFile,watchPaper:dirs,dnsReadRecovery:true}).catch(error=>{captureFailure=error;});
while(!fs.existsSync(path.join(study,'events.jsonl'))){if(captureFailure)throw captureFailure;await new Promise(resolve=>setTimeout(resolve,100));}
const barrier=pairedClock(),common=['--input',path.join(study,'events.jsonl'),'--format','journal','--follow','--seconds','1800','--resume',parent.directory,'--entry-exchanges',ids.join(','),'--resume-entries','--execution-age','--exit-policy',exitFile];
const jobs=dirs.map((out,i)=>run([...common,'--output',out,...(i?['--entry-policy',entryFile]:[])],{processingClock:barrier.clock(i),verifiedParent:parent}).catch(error=>{barrier.abort(error);throw error;}));
let paperFailure=null;
const completion=Promise.all(jobs).catch(error=>{paperFailure=error;fs.writeFileSync(path.join(study,'STOP'),'paper consumer failed; halt observation and preserve state\n');});
await capture;
if(paperFailure){await completion;throw paperFailure;}
const bytes=fs.statSync(path.join(study,'events.jsonl')).size;
while(dirs.some(dir=>!fs.existsSync(path.join(dir,'state.json'))||JSON.parse(fs.readFileSync(path.join(dir,'state.json'))).source.consumedBytes!==bytes)){if(paperFailure)throw paperFailure;await new Promise(resolve=>setTimeout(resolve,250));}
for(const dir of dirs)fs.writeFileSync(path.join(dir,'STOP'),'terminal source consumed; preserve all liabilities for explicit drain\n');
await completion;if(paperFailure)throw paperFailure;if(captureFailure)throw captureFailure;
console.log(JSON.stringify({study,terminal:true,requiresFinancialDrain:true,contestOrders:0,profitabilityEstablished:false}));
