import test from 'node:test';import assert from 'node:assert/strict';
import {oneShareDepth,auditTrialDepth} from './edge-depth-audit.mjs';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {createBookPublication} from './book-publication.mjs';
test('thin best level skips to worse price with unchanged one-percent per-level cap',()=>{
 assert.deepEqual(oneShareDepth([{price:'0.625000',quantity:74},{price:'0.620000',quantity:1000}],'sell'),{status:'available',levelIndex:1,rawPrice:'0.620000',modeledPrice:'0.615000'});
 assert.equal(oneShareDepth([{price:'0.100000',quantity:100}],'buy').modeledPrice,'0.105000');
});
test('sub-share capacities cannot be pooled across different prices; exact threshold matters',()=>{
 assert.equal(oneShareDepth([{price:'0.625000',quantity:99},{price:'0.620000',quantity:99}],'sell').status,'unavailable');
 assert.equal(oneShareDepth([{price:'0.005000',quantity:100}],'sell').status,'unavailable');
 assert.equal(oneShareDepth([{price:'0.625000',quantity:100}],'sell').levelIndex,0);
});
test('unordered, duplicate, malformed or wrong-side depth is rejected',()=>{
 for(const rows of [[{price:'0.620000',quantity:100},{price:'0.625000',quantity:100}],[{price:'0.625000',quantity:100},{price:'0.625000',quantity:100}],[{price:'0.625000',quantity:1.5}]])assert.throws(()=>oneShareDepth(rows,'sell'),/EDGE_DEPTH_/);
 assert.throws(()=>oneShareDepth([],'hold'),/EDGE_DEPTH_SIDE/);
});
test('diagnostic refuses a forged public chain before attributing any outcome',()=>{
 const bank={universe:{tournamentId:'bda92870-621e-47b0-bc3c-3602c5c26f55',exchanges:[{marketId:'1',exchangeId:'101',option:'YES'}]}};
 assert.throws(()=>auditTrialDepth({bank,trials:[],sourceBytes:Buffer.from(JSON.stringify({sequence:1,kind:'session_start',hash:'forged'})+'\n')}),/PUBLIC_BOOK/);
});
test('source-bound horizon uses first due request, retains censoring and honors intervening revocation',t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-edge-depth-')),file=path.join(dir,'books.jsonl'),tid='bda92870-621e-47b0-bc3c-3602c5c26f55';
 const at=n=>new Date(Date.parse('2026-10-03T00:00:00Z')+n).toISOString(),universe={tournamentId:tid,exchanges:[{marketId:'1',exchangeId:'101',option:'YES'}]};let clock=Date.parse(at(0));
 const writer=createBookPublication({file,universe,sessionId:'depth-test',now:()=>clock,canOperate:()=>true});
 t.after(()=>{writer.close();assert.equal(path.dirname(path.resolve(dir)),path.resolve(os.tmpdir()));assert.ok(path.basename(dir).startsWith('sig-edge-depth-'));fs.rmSync(dir,{recursive:true,force:true});});
 const publish=(started,received,sequence,quantity)=>{clock=Date.parse(at(received));assert.equal(writer.publishBook({marketId:'1',response:{endpoint:'/markets/1/orderbook?tournamentId='+tid+'&depth=20',requestStartedAt:at(started),receivedAt:at(received),data:{contexts:[{type:'tournament',tournament:{id:tid},orderbook:{exchanges:[{exchangeId:'101',option:'YES',asOf:{sequence,at:at(started+50)},bids:[{price:.4,quantity},{price:.395,quantity:1000}],asks:[{price:.41,quantity:1000}]}]}}]}}}),true);};
 publish(2000,2100,1,1000);publish(302000,302150,2,1000);publish(302200,302300,3,74);publish(302400,302500,4,1000);
 const bank={universe,config:{feeBps:0,slippageTicks:1,depthFractionBps:100,orderShares:1,latencyMs:1000},policy:{horizonMs:300000,labelLatenessMs:60000}},trial={marketId:'1',phase:'awaiting_outcome',preparedAt:at(1000),filledAt:at(2100),dueAt:at(302100),expiresAt:at(362100)};
 let result=auditTrialDepth({bank,trials:[trial],sourceBytes:Buffer.concat([fs.readFileSync(file),Buffer.from('{')])});const r=result.trials[0];
 assert.equal(result.incompleteTailBytes,1);assert.equal(r.outcomeSequence,4);assert.equal(r.status,'frozen_top_depth_censor');assert.equal(r.fullLadder.levelIndex,1);assert.equal(r.diagnosticModeledNet,'-0.025000');
 assert.throws(()=>auditTrialDepth({bank,trials:[{...trial,dueAt:at(302099)}],sourceBytes:fs.readFileSync(file)}),/DEADLINE/);
 writer.close();const revoked={...trial,filledAt:at(302500),preparedAt:at(301000),dueAt:at(602500),expiresAt:at(662500)};
 result=auditTrialDepth({bank,trials:[revoked],sourceBytes:fs.readFileSync(file)});assert.equal(result.trials[0].status,'revoked');
});
