import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import readline from 'node:readline';
import {pathToFileURL,fileURLToPath} from 'node:url';
const repository=fileURLToPath(new URL('../../',import.meta.url));
const sha=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
const baselineModule='paper/shared-reference-wallet-v6.mjs', candidateModule='paper/shared-reference-wallet-v7.mjs';

export function frozenSourceBytes(bytes,expected) {
 if(sha(bytes)===expected)return{bytes,restoredLineEndings:false};
 const restored=Buffer.from(bytes.toString('utf8').replace(/\r\n/g,'\n'));
 if(sha(restored)!==expected)throw Error('FROZEN_SOURCE_IDENTITY_MISMATCH');
 return{bytes:restored,restoredLineEndings:true};
}
export function checkReceipt(sample,bank,previousAt,versions) {
 const {book,rules}=sample??{};
 const canonical=x=>typeof x==='string'&&Number.isFinite(Date.parse(x))&&new Date(x).toISOString()===x;
 if(!book||!rules||![book.requestStartedAt,book.receivedAt,rules.requestStartedAt,rules.receivedAt].every(canonical))throw Error('ORIGINAL_RECEIPT_CLOCKS_REQUIRED');
 const received=Date.parse(book.receivedAt),start=Date.parse(book.requestStartedAt);
 if(received<previousAt||start>received)throw Error('NONCAUSAL_RECEIPT_ORDER');
 if(start<Date.parse(bank.cutoff))throw Error('PRE_CUTOFF_RECEIPT');
 const instrument=bank.instruments.find(i=>i.marketId===book.marketId&&i.exchangeId===book.exchangeId);
 if(!instrument||typeof book.versionKey!=='string'||!book.versionKey||!Array.isArray(book.bids)||!Array.isArray(book.asks))throw Error('RECEIPT_SCOPE');
 const key=book.marketId+':'+book.exchangeId+':'+book.versionKey;
 const content=sha(JSON.stringify({bids:book.bids,asks:book.asks,sourceAt:book.sourceAt??null}));
 if(versions.has(key)&&versions.get(key)!==content)throw Error('CONFLICTING_BOOK_VERSION');
 versions.set(key,content);return received;
}
async function materialize(bank,workspace) {
 if(!bank.codeIdentity?.[baselineModule]||!bank.codeIdentity?.['paper/fixed.mjs'])throw Error('FROZEN_CODE_MANIFEST_REQUIRED');
 const temporary=fs.mkdtempSync(path.join(os.tmpdir(),'viserion-frozen-replay-'));
 const records=[];
 try {
  for(const[file,hash]of Object.entries(bank.codeIdentity)){
   if(!/^[a-zA-Z0-9_/-]+\.mjs$/.test(file)||file.split('/').includes('..'))throw Error('UNSAFE_SOURCE_PATH');
   const result=frozenSourceBytes(fs.readFileSync(path.join(workspace,file)),hash);
   const destination=path.join(temporary,file);fs.mkdirSync(path.dirname(destination),{recursive:true});fs.writeFileSync(destination,result.bytes);
   records.push({file,sha256:hash,restoredLineEndings:result.restoredLineEndings});
  }
  const candidate=fs.readFileSync(path.join(workspace,candidateModule));
  fs.writeFileSync(path.join(temporary,candidateModule),candidate);
  records.push({file:candidateModule,sha256:sha(candidate),candidateNotHistorical:true});
  const original=await import(pathToFileURL(path.join(temporary,baselineModule)).href);
  const risk=await import(pathToFileURL(path.join(temporary,candidateModule)).href);
  const economics=await import(pathToFileURL(path.join(temporary,'research/shared-wallet-economics-v6.mjs')).href);
  return{temporary,records,original,risk,economics};
 }catch(error){cleanup(temporary);throw error;}
}
function cleanup(temporary){
 const resolved=path.resolve(temporary);
 if(path.dirname(resolved)!==path.resolve(os.tmpdir())||!path.basename(resolved).startsWith('viserion-frozen-replay-'))throw Error('UNSAFE_REPLAY_CLEANUP');
 fs.rmSync(resolved,{recursive:true,force:true});
}
function exposure(state,amount,price){
 const owned=Object.values(state.positions).reduce((n,p)=>n+amount(p.cost),0n);
 const reserved=Object.values(state.pending).filter(p=>p.action==='buy').reduce((n,p)=>n+price(p.limit)*BigInt(p.quantity),0n);
 const shares=Object.values(state.positions).reduce((n,p)=>n+BigInt(p.quantity),0n);
 return{owned,reserved,shares};
}
export async function compareReceipts({bank,samples,inventoryRisk,workspace=repository,source}) {
 const frozen=await materialize(bank,workspace);
 try {
  const fixed=await import(pathToFileURL(path.join(frozen.temporary,'paper/fixed.mjs')).href);
  const {payloadHash}=await import(pathToFileURL(path.join(frozen.temporary,'execution/core.mjs')).href);
  const {bankHash,...body}=bank;
  if(payloadHash(body)!==bankHash)throw Error('BANK_HASH_MISMATCH');
  const riskBody={...body,inventoryRisk,codeIdentity:{...body.codeIdentity,[candidateModule]:sha(fs.readFileSync(path.join(workspace,candidateModule)))}};
  const candidate={...riskBody,bankHash:payloadHash(riskBody)};
  const cohorts=[{id:'original',bank,state:frozen.original.createSharedReferenceWalletV6(bank),advance:frozen.original.advanceSharedReferenceWalletV6},
   {id:'candidate',bank:candidate,state:frozen.risk.createSharedReferenceWalletV7(candidate),advance:frozen.risk.advanceSharedReferenceWalletV7}];
  const meter=()=>({ownedMicroMs:0n,reservedMicroMs:0n,shareMs:0n,decisions:{}});
  const meters=cohorts.map(meter),versions=new Map();let previousAt=Date.parse(bank.cutoff),count=0;
  for await(const sample of samples){
   const at=checkReceipt(sample,bank,previousAt,versions),delta=BigInt(at-previousAt);
   for(let index=0;index<cohorts.length;index++){
    const c=cohorts[index],m=meters[index],e=exposure(c.state,fixed.amount,fixed.price);
    m.ownedMicroMs+=e.owned*delta;m.reservedMicroMs+=e.reserved*delta;m.shareMs+=e.shares*delta;
    const decision=c.advance(c.state,c.bank,structuredClone(sample));m.decisions[decision.reason]=(m.decisions[decision.reason]??0)+1;
   }
   previousAt=at;count++;
  }
  if(!count)throw Error('ORIGINAL_RECEIPTS_REQUIRED');
  const markAt=new Date(previousAt).toISOString();
  const results=cohorts.map((c,index)=>{const m=meters[index];return{id:c.id,bankHash:c.bank.bankHash,observations:count,decisions:m.decisions,
   economics:frozen.economics.sharedWalletEconomicsV6({state:c.state,bank:c.bank,at:markAt}),hypotheticalFills:c.state.fills.length,
   riskStops:c.state.marketRiskStop??{},ownedCapitalHours:fixed.money(m.ownedMicroMs/3600000n),reservedCapitalHours:fixed.money(m.reservedMicroMs/3600000n),
   ownedCapitalMicroSUSQMilliseconds:m.ownedMicroMs.toString(),reservedCapitalMicroSUSQMilliseconds:m.reservedMicroMs.toString(),shareMilliseconds:m.shareMs.toString(),
   exposureIntegralEndsAtLastOriginalReceipt:true,holdings:structuredClone(c.state.positions),pending:structuredClone(c.state.pending)};});
  return{schemaVersion:1,mode:'same_receipt_causal_funded_policy_comparison',source,through:markAt,cutoff:bank.cutoff,inventoryRisk,frozenSources:frozen.records,results,
   modelParametersSelectedByScore:false,actualOrders:0,networkRequests:0,marketProfitabilityEstablished:false,
   limitations:['Identical original receipt chronology; hypothetical later-depth fills, not actual queue execution.','Source provenance is caller-supplied and not independently verified by this offline runner.','No source reference is updated from future observations.','Incomplete/stale liquidation marks stay null; capital-hours stop at final receipt, so unresolved future risk remains.','No model selection or activation is authorized by this comparison.']};
 }finally{cleanup(frozen.temporary);}
}
async function hashPrefix(file,bytes){const digest=crypto.createHash('sha256');for await(const block of fs.createReadStream(file,{end:bytes-1}))digest.update(block);return digest.digest('hex');}
async function* receiptFile(file,bytes){
 const lines=readline.createInterface({input:fs.createReadStream(file,{end:bytes-1}),crlfDelay:Infinity});
 for await(const line of lines){if(line.trim()){const row=JSON.parse(line);yield row.sample??row;}}
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const options=Object.fromEntries(process.argv.slice(2).reduce((a,v,i,all)=>{if(i%2===0)a.push([v.replace(/^--/,''),all[i+1]]);return a;},[]));
 if(!options.bank||!options.receipts||!options.output||!options['holding-ms']||!options['adverse-ticks'])throw Error('Usage: --bank FILE --receipts ORIGINAL.jsonl --holding-ms N --adverse-ticks N --output FILE');
 if(fs.existsSync(options.output)||[options.bank,options.receipts].some(p=>path.resolve(p)===path.resolve(options.output)))throw Error('OUTPUT_MUST_BE_NEW_FILE');
 const bytes=fs.statSync(options.receipts).size;if(!bytes)throw Error('ORIGINAL_RECEIPTS_REQUIRED');
 const prefixHash=await hashPrefix(options.receipts,bytes);
 const report=await compareReceipts({bank:JSON.parse(fs.readFileSync(options.bank)),samples:receiptFile(options.receipts,bytes),inventoryRisk:{maxHoldingMs:Number(options['holding-ms']),maximumAdverseTicks:Number(options['adverse-ticks'])},source:{file:path.basename(options.receipts),sha256:prefixHash,bytes,frozenBytePrefix:true,provenanceIndependentlyVerified:false}});
 if(await hashPrefix(options.receipts,bytes)!==prefixHash)throw Error('SOURCE_PREFIX_CHANGED_DURING_REPLAY');
 fs.writeFileSync(options.output,JSON.stringify(report,null,2)+'\n',{flag:'wx'});
 console.log(JSON.stringify({output:options.output,through:report.through,results:report.results.map(r=>({id:r.id,economics:r.economics,ownedCapitalHours:r.ownedCapitalHours,reservedCapitalHours:r.reservedCapitalHours}))},null,2));
}
