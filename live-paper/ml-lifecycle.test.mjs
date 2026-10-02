import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';
import {initialState,digest} from './model.mjs';import {advanceShadow} from './ml-shadow.mjs';
const config=JSON.parse(fs.readFileSync(new URL('./fee-free-config.json',import.meta.url)));
test('new lifecycle revokes entry forecast after account outage without rewriting frozen legacy semantics',()=>{
 const cutoff='2026-10-02T06:00:00Z',at='2026-10-02T06:00:10Z';
 const wallet=initialState(config,'pending-test',cutoff);wallet.books['1066:YES']={marketId:'377',valid:true};wallet.strategies.mean_reversion.pending['1066:YES']={action:'buy',quantity:1,limit:'0.500000',createdAt:cutoff};
 const records=[{sequence:1,hash:'fixture',at,event:{type:'account_read_outage',clockAt:at}}];
 for(const lifecycle of [undefined,'revoke-on-invalidation-v2']){
  const bank={config,selected:[],trainingCaptureThrough:cutoff,...(lifecycle?{forecastLifecycle:lifecycle}:{})},input={bankHash:digest(bank),sourceSequence:0,sourceHash:null,wallets:{a:wallet},histories:{},predictions:{}};
  const out=advanceShadow(input,records,bank,{allowUnboundFixture:true});assert.equal(Boolean(out.wallets.a.strategies.mean_reversion.pending['1066:YES']),!lifecycle);assert.equal(Boolean(input.wallets.a.strategies.mean_reversion.pending['1066:YES']),true);
 }
});
