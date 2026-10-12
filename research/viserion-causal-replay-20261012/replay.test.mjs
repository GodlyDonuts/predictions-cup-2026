import fs from 'node:fs';
import crypto from 'node:crypto';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import test from 'node:test';
import assert from 'node:assert/strict';
import {payloadHash} from '../../execution/core.mjs';
import {frozenSourceBytes,checkReceipt,compareReceipts} from './replay.mjs';
const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
const cutoff='2026-10-12T08:00:00.000Z',root={id:'fixture'},rootHash=payloadHash(root);
function bank(){const old=JSON.parse(fs.readFileSync(new URL('../../data/maine-passive-harvest-paper-20261012-bank.json',import.meta.url))),{bankHash,...body}=old;
 Object.assign(body,{cutoff,initialCash:'1000.000000',capitalLimit:'50.000000',instruments:[{marketId:'1',exchangeId:'1',eventId:'1',side:'YES',rootHash,sourceAt:cutoff,entryExpiresAt:'2026-10-12T09:00:00.000Z',lowerValue:'0.700000',minimumGap:'0.005000',maximumPrincipal:'50.000000'}]});return{...body,bankHash:payloadHash(body)};}
function sample(second,bid,ask){const at=new Date(Date.parse(cutoff)+second*1000).toISOString();return{book:{marketId:'1',exchangeId:'1',versionKey:'v'+second,sourceAt:at,requestStartedAt:at,receivedAt:at,bids:[{price:bid,quantity:10000}],asks:[{price:ask,quantity:10000}]},rules:{marketId:'1',root,rootHash,requestStartedAt:at,receivedAt:at}};}
const risk={maxHoldingMs:30000,maximumAdverseTicks:4};
test('recorded byte identity may be restored exactly from Windows line endings, never approximate code',()=>{
 const original=Buffer.from('export const x=1;\n'),windows=Buffer.from('export const x=1;\r\n');
 assert.equal(frozenSourceBytes(original,sha(original)).restoredLineEndings,false);
 const restored=frozenSourceBytes(windows,sha(original));assert.deepEqual(restored.bytes,original);assert.equal(restored.restoredLineEndings,true);
 assert.throws(()=>frozenSourceBytes(Buffer.from('export const x=2;\r\n'),sha(original)),/IDENTITY_MISMATCH/);
});
test('original clocks and chronology are required; hindsight and missing receipts are rejected',()=>{
 const b=bank(),s=sample(1,'0.555000','0.560000');
 assert.throws(()=>checkReceipt({...s,book:{...s.book,requestStartedAt:undefined}},b,0,new Map()),/CLOCKS_REQUIRED/);
 assert.throws(()=>checkReceipt(s,b,Date.parse(cutoff)+2000,new Map()),/NONCAUSAL/);
 assert.throws(()=>checkReceipt(sample(-1,'0.555000','0.560000'),b,0,new Map()),/PRE_CUTOFF/);
});
test('changed price depth under an identical source version cannot fund invented liquidity',()=>{
 const b=bank(),s=sample(1,'0.555000','0.560000'),v=new Map();checkReceipt(s,b,0,v);
 assert.throws(()=>checkReceipt({...s,book:{...s.book,asks:[{price:'0.550000',quantity:10000}]}},b,0,v),/CONFLICTING_BOOK_VERSION/);
});
test('streamed common receipts replay exact funded losses and exposure integrals',async()=>{
 const rows=[sample(1,'0.555000','0.560000'),sample(8,'0.545000','0.550000'),sample(12,'0.500000','0.505000'),sample(15,'0.500000','0.505000')];
 async function* stream(){for(const s of rows)yield s;}
 const report=await compareReceipts({bank:bank(),samples:stream(),inventoryRisk:risk,source:{kind:'synthetic_fixture',provenanceIndependentlyVerified:false}});
 const c=report.results.find(r=>r.id==='candidate');assert.equal(c.economics.realized,'-3.480000');assert.equal(c.economics.ownedPrincipal,'0.000000');assert.equal(c.observations,4);
 assert.equal(c.ownedCapitalMicroSUSQMilliseconds,'225330000000');assert.equal(c.reservedCapitalMicroSUSQMilliseconds,'225330000000');assert.equal(c.ownedCapitalHours,'0.062591');
 assert.equal(c.shareMilliseconds,'406000');assert.equal(report.marketProfitabilityEstablished,false);assert.equal(report.actualOrders,0);
 assert.ok(report.frozenSources.every(x=>x.sha256.length===64));
});
test('an empty or wallet-checkpoint-only file cannot be represented as receipt performance',async()=>{
 await assert.rejects(compareReceipts({bank:bank(),samples:[],inventoryRisk:risk}),/ORIGINAL_RECEIPTS_REQUIRED/);
 await assert.rejects(compareReceipts({bank:bank(),samples:[{state:{books:{}}}],inventoryRisk:risk}),/CLOCKS_REQUIRED/);
});
test('CLI freezes the input byte prefix and refuses to overwrite prior evidence',t=>{
 const directory=fs.mkdtempSync(path.join(os.tmpdir(),'viserion-replay-cli-test-'));
 t.after(()=>{if(path.dirname(path.resolve(directory))!==path.resolve(os.tmpdir())||!path.basename(directory).startsWith('viserion-replay-cli-test-'))throw Error('UNSAFE_TEST_CLEANUP');fs.rmSync(directory,{recursive:true,force:true});});
 const bankFile=path.join(directory,'bank.json'),receipts=path.join(directory,'receipts.jsonl'),output=path.join(directory,'output.json');
 fs.writeFileSync(bankFile,JSON.stringify(bank()));const rows=[sample(1,'0.555000','0.560000'),sample(8,'0.545000','0.550000'),sample(12,'0.500000','0.505000'),sample(15,'0.500000','0.505000')];
 fs.writeFileSync(receipts,rows.map(x=>JSON.stringify({sample:x})).join('\n')+'\n');
 const args=[fileURLToPath(new URL('replay.mjs',import.meta.url)),'--bank',bankFile,'--receipts',receipts,'--holding-ms','30000','--adverse-ticks','4','--output',output];
 const run=spawnSync(process.execPath,args,{encoding:'utf8',timeout:20000});assert.equal(run.status,0,run.stderr);
 const report=JSON.parse(fs.readFileSync(output));assert.equal(report.source.sha256,sha(fs.readFileSync(receipts)));assert.equal(report.source.frozenBytePrefix,true);
 const original=fs.readFileSync(output);const duplicate=spawnSync(process.execPath,args,{encoding:'utf8',timeout:20000});assert.notEqual(duplicate.status,0);assert.match(duplicate.stderr,/OUTPUT_MUST_BE_NEW_FILE/);assert.deepEqual(fs.readFileSync(output),original);
});
