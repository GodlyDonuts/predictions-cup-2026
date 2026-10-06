import fs from 'node:fs';import path from 'node:path';import{createHash}from'node:crypto';
import{atomicJson,payloadHash}from'../execution/ledger.mjs';
import{openProbabilityReceipt}from'../live-paper/probability-receipt-host.mjs';
import{collectFrozenPaperFiles}from'../live-paper/frozen-paper-graph.mjs';
import{frozenPaperModule}from'../live-paper/frozen-paper-runtime.mjs';
const rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),cohort='mn-republican-source-value-20261006-v1',sha=b=>createHash('sha256').update(b).digest('hex'),pointerFile=path.join(rt,'source-value-paper-active.json');
for(const f of ['STOP',path.join(rt,'STOP'),pointerFile,path.join(rt,'probability-receipt-cohorts',cohort,'bank.json')])if(fs.existsSync(f))throw Error('STOP or existing source experiment');
const bundleFile='data/mn-source-value-paper-bundle-20261006-v1.json',bundle=JSON.parse(fs.readFileSync(bundleFile)),r=bundle.research[0];
if(bundle.research.length!==1||r.marketId!=='361'||r.exchangeId!=='1050'||Date.now()>=Date.parse(r.expiresAt))throw Error('Wrong or expired source scope');
const preserved=[];for(const p of ['probability-receipt-active.json','calibrated-poll-paper-active.json','primary-breadth-paper-active.json']){const a=JSON.parse(fs.readFileSync(path.join(rt,p))),dir=path.join(rt,'probability-receipt-cohorts',a.cohort);preserved.push({pointer:p,cohort:a.cohort,bankHash:a.bankHash,bankFileHash:sha(fs.readFileSync(path.join(dir,'bank.json')))});}
const files=collectFrozenPaperFiles(process.cwd(),['live-paper/probability-receipt-host.mjs']),snapshot=payloadHash(files),archive=path.join(rt,'frozen-code',snapshot),indexFile=path.join(rt,'frozen-code/index.json'),index=JSON.parse(fs.readFileSync(indexFile));
if(!fs.existsSync(archive)){fs.mkdirSync(archive,{recursive:true});for(const file of Object.keys(files)){const target=path.join(archive,file);fs.mkdirSync(path.dirname(target),{recursive:true});fs.writeFileSync(target,fs.readFileSync(file),{flag:'wx'});}atomicJson(path.join(archive,'manifest.json'),{schemaVersion:1,files});}
for(const[file,hash]of Object.entries(files))if(sha(fs.readFileSync(path.join(archive,file)))!==hash)throw Error('Frozen source changed');
const w=openProbabilityReceipt({runtimeDir:rt,cohort,initialize:true,bundle});let bank,status;try{bank=w.bank;status=w.status();}finally{w.close();}
if(payloadHash(collectFrozenPaperFiles(process.cwd(),['live-paper/probability-receipt-host.mjs']))!==snapshot)throw Error('Reducer changed during freeze');
index.banks[payloadHash(bank)]={snapshot};atomicJson(indexFile,index);await frozenPaperModule({runtimeDir:rt,bank,modulePath:'live-paper/probability-receipt-host.mjs'});
atomicJson(path.join(w.dir,'stats.json'),status);atomicJson(pointerFile,{cohort,bankHash:status.bankHash});
const proof={schemaVersion:1,at:new Date().toISOString(),cohort,bankHash:status.bankHash,canonicalIndexKey:payloadHash(bank),snapshot,frozenFiles:Object.keys(files).length,cutoff:bank.cutoff,researchId:r.id,researchBundleHash:sha(fs.readFileSync(bundleFile)),originalSourceAt:r.sourceAt,researchExpiresAt:r.expiresAt,entryValuation:r.lower,estimateMeaning:'Point estimate minus10pp judgment reserve, not a confidence lower bound. Known sensitivitymin0.115 and severe0 retained.',policy:{fees:0,shares:1,latenciesMs:bank.policy.latenciesMs,maxHoldMs:bank.policy.maxHoldMs,oneAdverseTick:true},preserved,workerStarted:false,accountRequests:0,competitionOrders:0,profitabilityEstablished:false};
fs.writeFileSync('data/mn-source-value-paper-prepared-20261006.json',JSON.stringify(proof,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(proof));
