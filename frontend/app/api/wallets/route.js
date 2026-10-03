export const dynamic='force-dynamic';
import {paperCohorts,paperWalletView} from '../../wallet-view.mjs';
import {readFile} from 'node:fs/promises';
import path from 'node:path';
import {executionPerformanceView} from '../../execution-view.mjs';
import {comparisonNameValid,comparisonWalletViews} from '../../comparison-view.mjs';
import {limitEntryCohorts,limitEntryWalletViews} from '../../limit-entry-view.mjs';
import {createHash} from 'node:crypto';
import {recoveryManifestValid,recoveryWalletViews} from '../../recovery-view.mjs';
import {executableEdgeWalletView} from '../../executable-edge-view.mjs';
import {bulkDipWalletViews} from '../../bulk-dip-view.mjs';
export async function GET(){
 let execution;try{const r=await fetch('http://127.0.0.1:8787/api/execution',{cache:'no-store',signal:AbortSignal.timeout(4000)});if(!r.ok)throw Error();const x=await r.json();let performance={available:false};try{const dir=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');performance=executionPerformanceView(JSON.parse(await readFile(path.join(dir,'performance-stats.json'),'utf8')),x);}catch{}execution={status:x.status,reason:x.reason??null,updatedAt:x.updatedAt??null,ordersSubmitted:x.ordersSubmitted??0,dispatchAttempts:x.dispatchAttempts??0,unconfirmedIntents:x.unconfirmedIntents??0,attributedFills:x.fills?.length??0,realizedPnl:x.realizedPnl??null,limits:x.limits??null,performance};}catch{execution={status:'unavailable'};}
 let official;try{const r=await fetch('http://127.0.0.1:8787/api/account',{cache:'no-store',signal:AbortSignal.timeout(4000)});if(!r.ok)throw Error();const d=await r.json();official={...d,id:'official',name:'Official account',official:true};}catch{official={id:'official',name:'Official account',official:true,available:false};}
 const wallets=await Promise.all(paperCohorts.map(async([id,name])=>{
  try{
   const response=await fetch(`http://127.0.0.1:8787/api/${id}`,{cache:'no-store',signal:AbortSignal.timeout(4000)});
   if(!response.ok)throw Error('Unavailable');const d=await response.json();if(!d.strategy)throw Error('Unavailable');
   return paperWalletView(d,id,name);
  }catch{return {id,name,available:false,status:'unavailable'};}
 }));
 let comparisons=[];
 try{
  const dir=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
  const manifest=JSON.parse(await readFile(path.join(dir,'entry-comparison-active.json'),'utf8'));
  if(!comparisonNameValid(manifest.cohort))throw Error('Invalid comparison');
  comparisons=comparisonWalletViews(JSON.parse(await readFile(path.join(dir,'entry-comparisons',manifest.cohort,'stats.json'),'utf8')),manifest);
 }catch{}
 let limits=[];
 try{
  const dir=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
  const manifest=JSON.parse(await readFile(path.join(dir,'limit-entry-active.json'),'utf8'));
  if(manifest.schemaVersion!==1||!comparisonNameValid(manifest.cohort)||!/^[a-f0-9]{64}$/.test(manifest.bankHash||''))throw Error('Invalid limit cohort');
  limits=limitEntryCohorts.map(([id,name])=>({id,name,available:false,status:'unavailable'}));
  limits=limitEntryWalletViews(JSON.parse(await readFile(path.join(dir,'limit-entry-cohorts',manifest.cohort,'stats.json'),'utf8')),manifest);
 }catch{}
 let recoveries=[];
 try{
  const dir=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
  const manifest=JSON.parse(await readFile(path.join(dir,'public-recovery-active.json'),'utf8'));
  if(!recoveryManifestValid(manifest))throw Error('Invalid recovery cohort');
  const cohortDir=path.join(dir,'public-recovery-cohorts',manifest.cohort);
  const bank=JSON.parse(await readFile(path.join(cohortDir,'bank.json'),'utf8'));
  if(createHash('sha256').update(JSON.stringify(bank)).digest('hex')!==manifest.bankHash)throw Error('Recovery bank mismatch');
  recoveries=recoveryWalletViews(JSON.parse(await readFile(path.join(cohortDir,'stats.json'),'utf8')),manifest,bank);
 }catch{}
 let edges=[];try{const dir=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');const manifest=JSON.parse(await readFile(path.join(dir,'executable-edge-active.json'),'utf8'));if(!comparisonNameValid(manifest.cohort))throw Error('Invalid edge cohort');const cohortDir=path.join(dir,'executable-edge-cohorts',manifest.cohort),bank=JSON.parse(await readFile(path.join(cohortDir,'bank.json'),'utf8'));if(createHash('sha256').update(JSON.stringify(bank)).digest('hex')!==manifest.bankHash)throw Error('Edge bank mismatch');edges=[executableEdgeWalletView(JSON.parse(await readFile(path.join(cohortDir,'stats.json'),'utf8')),manifest)];}catch{}
 let bulk=[];try{
  const dir=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
  const manifest=JSON.parse(await readFile(path.join(dir,'bulk-dip-active.json'),'utf8'));
  if(!comparisonNameValid(manifest.cohort))throw Error('Invalid bulk cohort');
  const cohortDir=path.join(dir,'bulk-dip-cohorts',manifest.cohort);
  const bank=JSON.parse(await readFile(path.join(cohortDir,'bank.json'),'utf8'));
  const report=JSON.parse(await readFile(path.join(cohortDir,'stats.json'),'utf8'));
  let workerAlive=false;try{const lock=JSON.parse(await readFile(path.join(cohortDir,'worker.lock'),'utf8'));if(Number.isSafeInteger(lock.pid)&&lock.pid>0&&typeof lock.id==='string'){process.kill(lock.pid,0);workerAlive=true;}}catch{}
  bulk=bulkDipWalletViews(report,manifest,bank,{workerAlive});
 }catch{}
 return Response.json({updatedAt:new Date().toISOString(),execution,wallets:[official,...bulk,...wallets,...comparisons,...limits,...recoveries,...edges]},{headers:{'Cache-Control':'no-store'}});
}
