import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
import {loadKey} from '../bot/api.mjs';import {ReadClient,lock,atomicJson} from '../live-paper/transport-runtime.mjs';
import {OrderTransport} from '../execution/transport.mjs';import {exactMoneyFetch} from '../execution/exact-money-receipts.mjs';
import {fillsReadAuthorized,reconcileProductionAccount} from '../execution/production-reconciliation.mjs';
const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),read=f=>JSON.parse(fs.readFileSync(f)),marker=read(path.join(runtime,'COLLECTOR_STOP'));
assert.equal(marker.source,'human-production-20261004');assert.equal(read(path.join(runtime,'competition-activation.json')).enabled,false);
const release=lock(path.join(process.env.LOCALAPPDATA,'SigTradingCup/bot.lock'));let client,transport;const receipts=[];
try{
 const budget=read(path.join(runtime,'coordinated-network-budget.json'));assert.equal(budget.schemaVersion,3);assert.equal(budget.writesEnabled,false);
 const rate=read(path.join(runtime,'rate.json'));assert.equal(rate.manualReview,false);assert(Number.isFinite(rate.next));
 const universe=read('live-paper/price-universe.json');
 client=new ReadClient(loadKey(),{networkBudget:budget,fetchFn:exactMoneyFetch(fetch,{tournamentId:universe.tournamentId,tournamentSlug:'midterm-elections',onReceipt:r=>receipts.push(r)}),authorizeDispatch:s=>s.method==='GET'&&(fillsReadAuthorized(s)||transport.authorizeRequest(s)),onAdmission:next=>atomicJson(path.join(runtime,'rate.json'),{next,manualReview:next===null})});client.next=rate.next;
 transport=new OrderTransport({scope:{tournamentId:universe.tournamentId,tournamentSlug:'midterm-elections',exchanges:universe.exchanges},dispatch:s=>client.dispatch(s),canWrite:()=>false,ownsOrder:()=>false,validateLease:()=>false,validatePermit:()=>false,validateNetworkPermit:()=>false});
 const result=await reconcileProductionAccount({transport,dispatch:s=>client.dispatch(s),universe});
 assert.equal(receipts.length,1);assert.equal(receipts[0].sourceBodyHash.length,64);
 const file=path.join(runtime,'production-human-reconciliation-'+Date.now()+'.json');fs.writeFileSync(file,JSON.stringify({...result,exactCashReceipts:receipts},null,2)+'\n',{flag:'wx',mode:0o600});
 atomicJson(path.join(runtime,'production-human-reconciliation-latest.json'),{file:path.basename(file),receiptHash:result.receiptHash,at:result.receipt.at});
 console.log(JSON.stringify({...result.summary,at:result.receipt.at,requests:client.requests,competitionWrites:0}));
}finally{client?.close();release();}
