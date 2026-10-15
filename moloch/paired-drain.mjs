import fs from 'node:fs';import path from 'node:path';
import {live} from './live.mjs';import {completedTail,run} from './run.mjs';
import {pairedClock} from './paired-clock.mjs';import {reportMoloch} from './engine.mjs';
import {payloadHash} from '../execution/ledger.mjs';
import {createHash} from 'node:crypto';import {fileURLToPath} from 'node:url';

// One explicitly started, finite continuation. No capital reset, policy fitting,
// auth refresh, competing collector, or automatic retry of a failed capture.
const [sourceParent,controlParent,candidateParent,pairsFile,number,cycleArg='60',depthArg]=process.argv.slice(2);
if(depthArg!==undefined&&(!/^\d+$/.test(depthArg)||Number(depthArg)<1||Number(depthArg)>10000))throw Error('DRAIN_EXIT_DEPTH_INVALID');
if(!/^\d+$/.test(number??'')||!/^\d+$/.test(cycleArg)||Number(cycleArg)<1||Number(cycleArg)>60)throw Error('Usage: source-parent control-parent candidate-parent pairs phase [cycles<=60]');
if(path.resolve(controlParent)===path.resolve(candidateParent))throw Error('DRAIN_DISTINCT_PARENTS');
const pairs=JSON.parse(fs.readFileSync(pairsFile,'utf8'));
if(!Array.isArray(pairs)||pairs.length<1||pairs.length>4||Number(cycleArg)>Math.floor(60/pairs.length))throw Error('DRAIN_CAPTURE_DURATION_LIMIT');
const parents=[controlParent,candidateParent].map(dir=>completedTail(dir));
const source=path.resolve(`runtime/moloch/live-sig-20261014-${number}`),dirs=['control','candidate'].map(side=>path.resolve(`runtime/moloch/fast-${side}-20261014-${number}`));
for(const dir of [source,...dirs])if(fs.existsSync(dir))throw Error('DRAIN_OUTPUT_EXISTS');
const registration={schemaVersion:1,registeredAt:new Date().toISOString(),driverSha256:createHash('sha256').update(fs.readFileSync(fileURLToPath(import.meta.url))).digest('hex'),sourceParent:path.resolve(sourceParent),source,parents:parents.map(p=>({directory:p.directory,snapshotHash:p.snapshot.hash,cash:p.snapshot.state.cash,realized:p.snapshot.state.realized})),pairs,maximumCycles:Number(cycleArg),entriesEnabled:false,...(depthArg?{inventoryExitDepthBps:Number(depthArg)}:{}),newAccountCapital:'0',contestOrders:0,profitabilityEstablished:false};
fs.writeFileSync(`data/moloch-paired-drain-registration-20261014-${number}.json`,JSON.stringify(registration,null,2)+'\n',{flag:'wx'});
let captureFailure=null,paperFailure=null;
const capture=live({marketId:pairs[0].marketId,exchangeId:pairs[0].exchangeId,outputDirectory:source,cycles:Number(cycleArg),resumeDirectory:sourceParent,pairsFile,restRecovery:true,drain:true,recoveryTail:parents.map(p=>p.directory),preparedRecoveryTails:parents,watchPaper:dirs,dnsReadRecovery:true,exitPriorityBooks:true}).catch(e=>{captureFailure=e;});
while(!fs.existsSync(path.join(source,'events.jsonl'))){if(captureFailure)throw captureFailure;await new Promise(r=>setTimeout(r,250));}
const barrier=pairedClock();
const jobs=dirs.map((directory,i)=>run(['--input',path.join(source,'events.jsonl'),'--format','journal','--follow','--seconds','3600','--resume',parents[i].directory,'--drain',...(depthArg?['--drain-exit-depth-bps',depthArg]:[]),'--output',directory],{processingClock:barrier.clock(i),verifiedParent:parents[i]}).catch(e=>{barrier.abort(e);throw e;}));
const completion=Promise.all(jobs).catch(e=>{paperFailure=e;fs.writeFileSync(path.join(source,'STOP'),'paper failure; preserve liabilities\n');});
const closed=directory=>{
 const file=path.join(directory,'state.json');if(!fs.existsSync(file))return false;
 const {hash,...snapshot}=JSON.parse(fs.readFileSync(file));if(hash!==payloadHash(snapshot))throw Error('DRAIN_SNAPSHOT_HASH');
 const r=reportMoloch(snapshot.state);return r.ownedCost==='0.000000'&&r.reservedCash==='0.000000'&&r.unresolvedOrders===0;
};
const watcher=setInterval(()=>{try{if(dirs.every(closed))fs.writeFileSync(path.join(source,'STOP'),'both paper banks flat and reconciled\n');}catch(e){paperFailure=e;fs.writeFileSync(path.join(source,'STOP'),'snapshot verification failed\n');}},1000);
try{
 await capture;if(paperFailure)throw paperFailure;
 const bytes=fs.statSync(path.join(source,'events.jsonl')).size;
 while(dirs.some(dir=>!fs.existsSync(path.join(dir,'state.json'))||JSON.parse(fs.readFileSync(path.join(dir,'state.json'))).source.consumedBytes!==bytes)){if(paperFailure)throw paperFailure;await new Promise(r=>setTimeout(r,250));}
 for(const dir of dirs)fs.writeFileSync(path.join(dir,'STOP'),'terminal source consumed; preserve inventory and liabilities\n');
 await completion;if(paperFailure)throw paperFailure;if(captureFailure)throw captureFailure;
 const banks=dirs.map(dir=>{const proof=completedTail(dir);return{directory:dir,snapshotHash:proof.snapshot.hash,...reportMoloch(proof.snapshot.state)};});
 const result={...registration,finishedAt:new Date().toISOString(),banks,fullyClosed:dirs.every(closed)};
 fs.writeFileSync(`data/moloch-paired-drain-result-20261014-${number}.json`,JSON.stringify(result,null,2)+'\n',{flag:'wx'});
 console.log(JSON.stringify({source,fullyClosed:result.fullyClosed,banks:banks.map(b=>({directory:b.directory,cash:b.cash,realized:b.realized,ownedCost:b.ownedCost,unresolved:b.unresolvedOrders})),contestOrders:0}));
}finally{clearInterval(watcher);for(const dir of dirs)if(fs.existsSync(dir)&&!fs.existsSync(path.join(dir,'STOP')))fs.writeFileSync(path.join(dir,'STOP'),'drain coordinator stopped; preserve state\n');await completion;}
