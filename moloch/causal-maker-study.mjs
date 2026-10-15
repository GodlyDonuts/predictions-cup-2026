import fs from 'node:fs';import path from 'node:path';
import {completedTail,entryPolicyTransition,flatEntryTransition,run} from './run.mjs';import {live} from './live.mjs';import {pairedClock} from './paired-clock.mjs';
const [parentDirectory,sourceParent,universeFile,number]=process.argv.slice(2);if(!/^\d+$/.test(number??''))throw Error('Usage: terminal-bank source-parent universe phase');
const parent=completedTail(parentDirectory),universe=JSON.parse(fs.readFileSync(universeFile)),ids=['998','1044','1048','846'];
const pairs=ids.map(id=>{const p=universe.find(p=>p.exchangeId===id);if(!p)throw Error('CAUSAL_STUDY_SCOPE');return{marketId:p.marketId,exchangeId:id};});
const source=path.resolve(`runtime/moloch/live-sig-20261014-${number}`),dirs=['control','candidate'].map(side=>path.resolve(`runtime/moloch/fast-${side}-20261014-${number}`));
for(const d of [source,...dirs])if(fs.existsSync(d))throw Error('CAUSAL_OUTPUT_EXISTS');
const write=(file,value)=>fs.writeFileSync(file,JSON.stringify(value,null,2)+'\n',{flag:'wx'}),base=`runtime/moloch/causal-${number}`,pairsFile=base+'-pairs.json',policyFile=base+'-root-policy.json',exitFile=base+'-exit.json',entryFiles=[base+'-control.json',base+'-candidate.json'];
const registeredAt=new Date().toISOString(),entryStopAt=new Date(Date.now()+360000).toISOString(),exitPolicy={inventoryExitMode:'fixedGain',inventoryMinimumGain:'0.005000',inventoryTakerAfterMs:0,maximumInventoryAgeMs:60000,inventoryExitDepthBps:5000};
const entryPolicies=[0,4].map(entryBackoffTicks=>({insideTicks:0,entryBackoffTicks,requireCurrentFlow:true,entryStopAt})),states=[0,1].map(()=>structuredClone(parent.snapshot.state));
for(let i=0;i<2;i++){entryPolicyTransition(states[i],ids);flatEntryTransition(states[i],true,exitPolicy,entryPolicies[i]);}
write(pairsFile,pairs);write(policyFile,{entryExchangeIds:ids});write(exitFile,exitPolicy);for(let i=0;i<2;i++)write(entryFiles[i],entryPolicies[i]);
const registration={schemaVersion:1,registeredAt,parent:parent.directory,parentSnapshotHash:parent.snapshot.hash,source,pairs,cycles:6,comparisonMode:'entryBackoff',entryStopAt,controlEntryPolicyFile:path.resolve(entryFiles[0]),controlPolicy:states[0].policy,candidatePolicy:states[1].policy,controlPolicyHash:states[0].policyHash,candidatePolicyHash:states[1].policyHash,selectionMethod:'fixed historical opportunity markets; live engine current-flow gate',scopeSelectionEvidence:'Previously inspected opportunity windows in captures65,66,71,75; not an untouched selection holdout.',inheritedCash:parent.snapshot.state.cash,inheritedRealized:parent.snapshot.state.realized,newAccountCapital:'0',contestOrders:0,profitabilityEstablished:false};
write(`data/moloch-fast-registration-20261014-${number}.json`,registration);
let captureFailure=null,paperFailure=null;
const capture=live({marketId:pairs[0].marketId,exchangeId:pairs[0].exchangeId,outputDirectory:source,cycles:6,resumeDirectory:sourceParent,pairsFile,policyFile,realtime:true,drain:true,universeFile,watchPaper:dirs,dnsReadRecovery:true}).catch(e=>{captureFailure=e;});
while(!fs.existsSync(path.join(source,'events.jsonl'))){if(captureFailure)throw captureFailure;await new Promise(r=>setTimeout(r,250));}
const barrier=pairedClock(),jobs=dirs.map((dir,i)=>run(['--input',path.join(source,'events.jsonl'),'--format','journal','--follow','--seconds','3600','--resume',parent.directory,'--entry-exchanges',ids.join(','),'--resume-entries','--execution-age','--exit-policy',exitFile,'--entry-policy',entryFiles[i],'--output',dir],{processingClock:barrier.clock(i),verifiedParent:parent}).catch(e=>{barrier.abort(e);throw e;}));
const completion=Promise.all(jobs).catch(e=>{paperFailure=e;fs.writeFileSync(path.join(source,'STOP'),'paper consumer failed; preserve state\n');});
try{
 await capture;if(paperFailure)throw paperFailure;
 const bytes=fs.statSync(path.join(source,'events.jsonl')).size;
 while(dirs.some(d=>!fs.existsSync(path.join(d,'state.json'))||JSON.parse(fs.readFileSync(path.join(d,'state.json'))).source.consumedBytes!==bytes)){if(paperFailure)throw paperFailure;await new Promise(r=>setTimeout(r,250));}
 for(const dir of dirs)fs.writeFileSync(path.join(dir,'STOP'),'terminal source consumed; retain all liabilities\n');await completion;if(paperFailure)throw paperFailure;if(captureFailure)throw captureFailure;
 console.log(JSON.stringify({source,terminal:true,requiresFinancialAudit:true,contestOrders:0,profitabilityEstablished:false}));
}finally{for(const dir of dirs)if(fs.existsSync(dir)&&!fs.existsSync(path.join(dir,'STOP')))fs.writeFileSync(path.join(dir,'STOP'),'coordinator stopped; preserve wallet\n');await completion;}
