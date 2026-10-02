import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {createHash} from 'node:crypto';
import {digest} from './model.mjs';
import {verifySource,advanceShadow,bindTrainingSource} from './ml-shadow.mjs';
const c=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url))),at='2026-10-02T03:00:00Z';
function source(marker){const b={sequence:1,previous:'',at,event:{type:'started'},state:{marker}};return JSON.stringify({...b,hash:digest(b)})+'\n';}
test('frozen bank binds exact training prefix before wallet mutation, including first startup',()=>{
 const raw=source('original'),records=verifySource(raw),bank={config:c,selected:[],trainingCaptureThrough:at,sourceRecords:1,sourceHash:createHash('sha256').update(raw).digest('hex')};
 const input={bankHash:digest(bank),sourceSequence:0,sourceHash:null,wallets:{},histories:{},predictions:{}};
 const s=advanceShadow(input,records,bank);assert.equal(s.sourceBinding.sourceHash,bank.sourceHash);assert.deepEqual(input.wallets,{});
 assert.throws(()=>advanceShadow(input,verifySource(source('replacement')),bank),/training source mismatch/);
 assert.throws(()=>bindTrainingSource(records,{...bank,sourceRecords:2}),/training source/);
 assert.throws(()=>advanceShadow({...s,sourceBinding:{...s.sourceBinding,lastRecordHash:'changed'}},records,bank),/binding changed/);
 const spaced=' '+raw.slice(0,-1)+' \n',spacedBank={...bank,sourceHash:createHash('sha256').update(spaced).digest('hex')};assert.equal(bindTrainingSource(verifySource(spaced),spacedBank).sourceHash,spacedBank.sourceHash);
});
test('coverage-gated inference rejects replacement exchange before intent or fill admission',()=>{
 const later='2026-10-02T03:00:30Z',snap={endpoint:'/markets/377/orderbook?tournamentId='+c.tournamentId,requestStartedAt:later,receivedAt:later,data:{contexts:[{type:'tournament',tournament:{id:c.tournamentId},orderbook:{exchanges:[{exchangeId:'999',option:'YES',asOf:{at:later,sequence:1},bids:[{price:.4,quantity:10000}],asks:[{price:.41,quantity:10000}]}]}}]}};
 const bank={config:c,requireInstrumentCoverage:true,trainingCaptureThrough:at,selected:[{id:'supported',horizon:60,weights:[.1],means:[0],scales:[1],coverage:[{marketId:'377',eligible:true,trainingInstruments:['1066:YES']}]}]};
 const initial={bankHash:digest(bank),sourceSequence:0,sourceHash:null,wallets:{},histories:{},predictions:{}};
 const r={sequence:1,hash:'r',at:later,event:{type:'cycle',clockAt:later,accountExclusions:[],sourceSnapshots:[snap]}},s=advanceShadow(initial,[r],bank);
 assert.deepEqual(s.wallets,{});assert.deepEqual(s.predictions,{});assert.equal(Object.values(s.unsupportedInstruments).length,3);assert.ok(Object.values(s.unsupportedInstruments).every(x=>x.reason==='not_in_frozen_training_support'));
 const good=structuredClone(r);good.event.sourceSnapshots[0].data.contexts[0].orderbook.exchanges[0].exchangeId='1066';assert.equal(Object.keys(advanceShadow(initial,[good],bank).wallets).length,3);
});
