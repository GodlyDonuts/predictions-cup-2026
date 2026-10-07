import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import test from 'node:test';import assert from 'node:assert/strict';
import {ReadClient} from '../live-paper/transport-runtime.mjs';import {provenNetworkGuardV1} from './proven-network-guard-v1.mjs';
test('real serialized client records guarded rejection before zero physical fetches',async t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-network-proof-')),proofFile=path.join(dir,'proof.jsonl');let fetches=0,clock=Date.parse('2026-10-07T00:00:00Z');
 const spec={method:'POST',endpoint:'/orders',body:{quantity:1},bodyJson:'{"quantity":1}',beforeDispatch:()=>true};
 spec.beforeNetwork=provenNetworkGuardV1({spec,getGuard:()=>()=>{throw Object.assign(Error('EXECUTION_AUTHORITY_UNAVAILABLE'),{code:'EXECUTION_AUTHORITY_UNAVAILABLE'});},proofFile,now:()=>clock});
 const api=new ReadClient('synthetic-offline-only',{authorizeDispatch:s=>s.beforeNetwork===spec.beforeNetwork,now:()=>clock,sleepFn:async n=>clock+=n,fetchFn:async()=>{fetches++;return new Response('{}');}});t.after(()=>api.close());
 await assert.rejects(api.dispatch(spec),/EXECUTION_AUTHORITY_UNAVAILABLE/);assert.equal(fetches,0);const proof=JSON.parse(fs.readFileSync(proofFile));assert.equal(proof.physicalFetchReached,false);assert.equal(proof.method,'POST');
});

