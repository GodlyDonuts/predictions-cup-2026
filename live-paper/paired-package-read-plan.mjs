import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
const hash=x=>createHash('sha256').update(JSON.stringify(x)).digest('hex');
const name=x=>typeof x==='string'&&/^[a-z][a-z0-9-]{0,63}$/.test(x)&&!/^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/.test(x);
// This is a bounded observation allocation, not position/trade authority.
export function readPairedPackageFocus(runtimeDir,universe,now=Date.now()){
 const file=path.join(runtimeDir,'paired-package-active.json');if(!fs.existsSync(file))return null;
 try{
  const manifest=JSON.parse(fs.readFileSync(file)),dir=path.join(runtimeDir,'paired-package-cohorts',manifest.cohort||'');
  if(manifest.schemaVersion!==1||!name(manifest.cohort)||!/^[a-f0-9]{64}$/.test(manifest.bankHash||'')||fs.existsSync(path.join(dir,'STOP')))throw Error();
  const bank=JSON.parse(fs.readFileSync(path.join(dir,'bank.json'))),stats=JSON.parse(fs.readFileSync(path.join(dir,'stats.json')));
  if(hash(bank)!==manifest.bankHash||bank.tournamentId!==universe.tournamentId||bank.cohort!==manifest.cohort||stats.bankHash!==manifest.bankHash||stats.cohort!==manifest.cohort||stats.mode!=='prospective_sequential_dual_no_paper'||stats.status!=='running'||stats.networkRequests!==0||stats.competitionOrders!==0||!Number.isSafeInteger(stats.pid)||stats.pid<=0||!Number.isFinite(Date.parse(stats.reportedAt))||now-Date.parse(stats.reportedAt)<0||now-Date.parse(stats.reportedAt)>10000)throw Error();
  process.kill(stats.pid,0);
  if(bank.markets.length!==2||!['381','382'].every(id=>bank.markets.some(m=>m.marketId===id&&universe.exchanges.some(x=>x.marketId===id&&x.exchangeId===m.exchangeId))))throw Error();
  const wallet=stats.wallets?.sequential_dual_no;if(!wallet)throw Error();
  const needed=wallet.pending!==null||wallet.activePackage?.stage==='orphan_exit_blocked';
  if(!needed)return {available:true,needed:false,marketIds:[]};
  if(wallet.pending&&!bank.markets.some(m=>m.marketId===wallet.pending.marketId&&m.exchangeId===wallet.pending.exchangeId))throw Error();
  return {available:true,needed:true,marketIds:bank.markets.map(m=>m.marketId),cohort:manifest.cohort,bankHash:manifest.bankHash,reason:'scoped_counterfactual_pending_or_orphan_observation'};
 }catch{return {available:false,needed:false,marketIds:[],reason:'paper_package_focus_unavailable'};}
}
export function focusPairedPackageReads(plan,focus){
 if(!focus?.available||!focus.needed)return plan;
 const wanted=focus.marketIds;if(wanted?.length!==2||new Set(wanted).size!==2||wanted.some(id=>!['381','382'].includes(id)))throw Error('INVALID_PACKAGE_READ_SCOPE');
 const reads=plan.reads.map(x=>({...x})),present=new Set(reads.map(x=>x.marketId)),missing=wanted.filter(id=>!present.has(id));
 const slots=reads.map((x,i)=>({x,i})).filter(({x})=>['comparison','discovery'].includes(x.reason)&&!wanted.includes(x.marketId));
 if(slots.length<missing.length)return {...plan,paperPackageFocus:{reason:'managed_risk_retains_capacity',marketIds:[]}};
 for(let i=0;i<missing.length;i++)reads[slots[i].i]={marketId:missing[i],reason:'paper_package',path:'supplementalDepth'};
 const admitted=new Set(reads.map(x=>x.marketId));return {...plan,reads,entryMarketIds:plan.entryMarketIds.filter(id=>admitted.has(id)),paperPackageFocus:{reason:focus.reason,marketIds:wanted,requestsAdded:0,replacedSlots:missing.length,executionAuthority:false}};
}
