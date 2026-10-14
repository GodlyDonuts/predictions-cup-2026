import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {createMoloch,advanceMoloch} from './engine.mjs';import {payloadHash} from '../execution/ledger.mjs';import {watchedPaperOrders} from './paper-watch.mjs';import {captureScopes} from './live.mjs';
test('active wallet recovery retains a cancelled quote older than the normal fifteen-minute tape',t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'moloch-watch-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));const source=path.join(dir,'source.jsonl'),at='2026-10-14T00:00:00.000Z',base=Date.parse(at),s=createMoloch({lifetimeMs:1800000});
 advanceMoloch(s,{type:'book',receivedAt:at,book:{tournamentId:'cup',marketId:'1',exchangeId:'2',sequence:'1',requestStartedAt:at,receivedAt:at,sourceAt:at,bids:[{price:'0.450000',quantity:1000}],asks:[{price:'0.455000',quantity:1000}]}});advanceMoloch(s,{type:'tick',receivedAt:new Date(base+1200000).toISOString()});
 const save=body=>fs.writeFileSync(path.join(dir,'state.json'),JSON.stringify({...body,hash:payloadHash(body)}));const body={state:s,source:{path:source}};save(body);const scopes=[{marketId:'1',exchangeId:'2'}],orders=watchedPaperOrders(dir,source,scopes),r=captureScopes(scopes,{orders:[]},base+1300000,orders);assert.equal(Date.parse(r[0].recoveryFrom),base+3000);assert.ok(Date.parse(r[0].recoveryFrom)<base+1300000-900000);
 assert.throws(()=>watchedPaperOrders(dir,source+'wrong',scopes),/WATCH_PROOF/);assert.throws(()=>watchedPaperOrders(dir,source,[{marketId:'3',exchangeId:'2'}]),/WATCH_SCOPE/);body.state.cash='1.000000';save(body);assert.throws(()=>watchedPaperOrders(dir,source,scopes),/LEDGER/);
});
test('not-yet-created watched wallet is harmless; a changed snapshot hash is rejected',t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'moloch-watch-deny-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));assert.deepEqual(watchedPaperOrders(dir,'source',[]),[]);fs.writeFileSync(path.join(dir,'state.json'),JSON.stringify({hash:'bad',state:{bot:'Moloch'}}));assert.throws(()=>watchedPaperOrders(dir,'source',[]),/WATCH_PROOF/);
});
