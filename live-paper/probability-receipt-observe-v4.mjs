import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import * as currentHost from './probability-receipt-host-v3.mjs';
import * as additionalHost from './probability-receipt-host.mjs';
import {frozenPaperModule} from './frozen-paper-runtime.mjs';
import {projectProbabilityReadFocus} from './probability-read-focus.mjs';
import {writeObservedProjections} from './executable-edge-observe.mjs';
import {Journal} from './runtime.mjs';
import {payloadHash} from '../execution/ledger.mjs';
import {digest} from './model.mjs';
const need=(ok,code)=>{if(!ok)throw Error('PROBABILITY_WRAPPER_V4_'+code);};
const sha=b=>createHash('sha256').update(b).digest('hex');
const ownDir=path.dirname(fileURLToPath(import.meta.url));
const legacyPointers=['calibrated-poll-paper-active.json','primary-breadth-paper-active.json'];
const safeCohort=s=>typeof s==='string'&&/^[a-z][a-z0-9-]{1,63}$/.test(s);
const safeHash=s=>typeof s==='string'&&/^[a-f0-9]{64}$/.test(s);
const read=file=>JSON.parse(fs.readFileSync(file));
const sourceIdentity=()=>Object.fromEntries(['probability-receipt-observe-v4.mjs','frozen-paper-runtime.mjs','probability-read-focus.mjs','executable-edge-observe.mjs','runtime.mjs','model.mjs','../execution/ledger.mjs'].map(p=>[p,sha(fs.readFileSync(path.join(ownDir,p)))]));
function pointer(value){need(value&&safeCohort(value.cohort)&&safeHash(value.bankHash),'POINTER');return {cohort:value.cohort,bankHash:value.bankHash};}
export function probabilityRegistryV4(runtimeDir,cohort){
 need(path.isAbsolute(runtimeDir)&&safeCohort(cohort),'DIRECTORY');
 const rootFile=path.join(runtimeDir,'probability-receipt-active.json'),root=pointer(read(rootFile));need(root.cohort===cohort,'ROOT_POINTER');
 const registryFile=path.join(runtimeDir,'probability-receipt-companions.json');
 const registry=fs.existsSync(registryFile)?read(registryFile):{schemaVersion:1,entries:[]};
 need(registry.schemaVersion===1&&Object.keys(registry).sort().join(',')==='entries,schemaVersion'&&Array.isArray(registry.entries)&&registry.entries.length<=4,'REGISTRY');
 const entries=[{...root,root:true,pointerFile:rootFile}];
 for(const name of legacyPointers){const file=path.join(runtimeDir,name);if(fs.existsSync(file))entries.push({...pointer(read(file)),pointerFile:file});}
 for(const item of registry.entries){need(item&&Object.keys(item).sort().join(',')==='bankHash,cohort','REGISTRY_ENTRY');entries.push({...pointer(item),registry:true});}
 const seen=new Set();const index=read(path.join(runtimeDir,'frozen-code','index.json'));
 for(const item of entries){need(!seen.has(item.cohort),'DUPLICATE_COHORT');seen.add(item.cohort);item.dir=path.join(runtimeDir,'probability-receipt-cohorts',item.cohort);item.bank=read(path.join(item.dir,'bank.json'));need(item.bank.name===item.cohort&&digest(item.bank)===item.bankHash,'BANK_POINTER_BINDING');need(item.root?item.bank.schemaVersion===4:item.bank.schemaVersion===2,'BANK_SCHEMA');need(safeHash(index.banks?.[payloadHash(item.bank)]?.snapshot),'FROZEN_BINDING_REQUIRED');}
 const context=entries[0].bank.universe;
 need(entries.every(e=>e.bank.universe?.hash===context?.hash&&e.bank.universe?.tournamentId===context?.tournamentId),'UNIVERSE_SCOPE');
 return {entries,registryFile,registry,identity:payloadHash({registry,pointers:entries.map(({cohort,bankHash,pointerFile,root})=>({cohort,bankHash,pointerFile:pointerFile||null,root:root===true}))})};
}
export async function openProbabilityWorkersV4(options,resolveHost=frozenPaperModule){
 const registry=probabilityRegistryV4(options.runtimeDir,options.cohort),workers=[];
 try{for(const entry of registry.entries){if(!entry.root&&fs.existsSync(path.join(entry.dir,'OBSERVER_STOP')))continue;
  const modulePath=entry.root?'live-paper/probability-receipt-host-v3.mjs':'live-paper/probability-receipt-host.mjs';
  const host=await resolveHost({runtimeDir:options.runtimeDir,bank:entry.bank,modulePath,fallback:entry.root?currentHost:additionalHost});
  const worker=await host.openProbabilityReceipt({...options,cohort:entry.cohort,initialize:false});workers.push(worker);
  need(worker.dir===entry.dir&&worker.status().bankHash===entry.bankHash,'OPENED_BANK_BINDING');
 }return {workers,registry};}catch(error){for(const worker of workers.reverse())worker.close();throw error;}
}
export async function runProbabilityReceiptV4(options,{resolveHost=frozenPaperModule,project=writeObservedProjections}={}){
 const workspaceDir=options.workspaceDir||path.dirname(ownDir);options={...options,workspaceDir};
 const {workers,registry}=await openProbabilityWorkersV4(options,resolveHost),root=workers[0],retired=new Set();let stopping=false;
 const end=()=>stopping=true;process.on('SIGINT',end);process.on('SIGTERM',end);
 const identity=sourceIdentity(),revisionFile=path.join(options.runtimeDir,'probability-receipt-wrapper-revisions.jsonl');
 const stopped=()=>['STOP','EXECUTION_STOP'].some(p=>fs.existsSync(path.join(options.runtimeDir,p)))||fs.existsSync(path.join(workspaceDir,'STOP'))||fs.existsSync(path.join(root.dir,'OBSERVER_STOP'));
 const guard=()=>{need(!stopped(),'STOPPED');need(payloadHash(sourceIdentity())===payloadHash(identity),'WRAPPER_CODE_CHANGED');need(probabilityRegistryV4(options.runtimeDir,options.cohort).identity===registry.identity,'REGISTRY_OR_POINTER_CHANGED');};
 try{
  guard();const revisions=new Journal(revisionFile);
  revisions.commit(null,{kind:'probability_observer_wrapper_code_revision_v4',wrapperIdentity:identity,registryIdentity:registry.identity,cohorts:workers.map(w=>{const s=w.status();return {cohort:s.cohort,bankHash:s.bankHash,stateHash:s.stateHash,journalSequence:s.journalSequence,journalHash:s.journalHash,bankFileHash:sha(fs.readFileSync(path.join(w.dir,'bank.json'))),journalFileHash:sha(fs.readFileSync(path.join(w.dir,'journal.jsonl')))};}),accountingReset:false,oldFillsImported:false,networkRequests:0,competitionOrders:0,executionAuthority:false});
  do{guard();if(stopping)break;
   for(const worker of workers){guard();if(stopping)break;if(retired.has(worker))continue;if(worker!==root&&fs.existsSync(path.join(worker.dir,'OBSERVER_STOP'))){worker.close();retired.add(worker);continue;}
    worker.consume();const stats=worker.status(),lock=read(path.join(worker.dir,'worker.lock'));
    const projections=[[path.join(worker.dir,'stats.json'),stats],[path.join(worker.dir,'read-focus.json'),projectProbabilityReadFocus(stats,lock,stats.cohort)]];
    if(worker===root)projections.push([path.join(options.runtimeDir,'probability-receipt-active.json'),{cohort:options.cohort,bankHash:stats.bankHash}]);
    await project(projections);
   }
   if(options.once)break;await new Promise(r=>setTimeout(r,1000));
  }while(!stopping);
 }finally{process.removeListener('SIGINT',end);process.removeListener('SIGTERM',end);for(const worker of workers.reverse())if(!retired.has(worker))worker.close();}
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const args=process.argv.slice(2),i=args.indexOf('--name');runProbabilityReceiptV4({runtimeDir:path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),cohort:args[i+1],once:args.includes('--once')}).catch(()=>{console.error('Probability companion worker stopped; inspect restricted evidence');process.exitCode=1;});
}
