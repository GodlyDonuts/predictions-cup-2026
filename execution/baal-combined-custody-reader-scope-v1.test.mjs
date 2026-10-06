import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {createHash} from 'node:crypto';import test from 'node:test';import assert from 'node:assert/strict';
import {readBaalCombinedCustodyReaderScopeV1} from './baal-combined-custody-reader-scope-v1.mjs';
import {replayBaalScaledControllerV1,baalScaledNoControllerIdentity} from './baal-scaled-controller-legacy-replay-v1.mjs';
import {openBaalScaledLiveHost,baalScaledLiveIdentity} from './baal-scaled-live-host-v1.mjs';
import {payloadHash,createExecutionState} from './core.mjs';
import {createScopedRestReceiptsV2} from './scoped-rest-receipts-v2.mjs';
import {ReadClient} from '../live-paper/transport-runtime.mjs';import {amount,money} from '../paper/fixed.mjs';
const text=fs.readFileSync(new URL('./baal-scaled-live-host-v1.test.mjs',import.meta.url),'utf8');
const fragment=text.slice(text.indexOf('const T='),text.indexOf("test('whole actual"));
const fixture=Function('fs','os','path','createHash','assert','openBaalScaledLiveHost','baalScaledLiveIdentity','payloadHash','createExecutionState','createScopedRestReceiptsV2','ReadClient','amount','money',fragment+';return fixture;')(fs,os,path,createHash,assert,openBaalScaledLiveHost,baalScaledLiveIdentity,payloadHash,createExecutionState,createScopedRestReceiptsV2,ReadClient,amount,money);
test('reader captures only economically replayed owned pair scope, refusing changed seal or universe',async t=>{
 const f=fixture(t);await f.h.step();await f.h.step();f.h.close();
 const source=path.join(f.runtimeDir,'baal-scaled-live-runs',f.descriptor.runId,'controller','baal-scaled-no-journal.jsonl');
 const seed=replayBaalScaledControllerV1({bodyText:fs.readFileSync(source,'utf8'),expectedCodeIdentity:baalScaledNoControllerIdentity()});
 const descriptor={kind:'baal_combined_custody_host_v6',ownerId:'Codex',runId:f.descriptor.runId,
  tournamentId:f.opts.universe.tournamentId,universeHash:f.opts.universe.hash,seedHash:payloadHash(seed),
  legacyAnchor:seed.anchor,legacyCodeIdentityHash:seed.legacyCodeIdentityHash};
 const file=path.join(f.runtimeDir,'baal-combined-custody-request-v6.json');
 fs.writeFileSync(file,JSON.stringify(descriptor));
 const args={runtimeDir:f.runtimeDir,universe:f.opts.universe};
 assert.deepEqual(readBaalCombinedCustodyReaderScopeV1(args).allowedOwnedGraphMarketIds,['11','12']);
 assert.equal(readBaalCombinedCustodyReaderScopeV1(args).networkRequests,0);
 assert.throws(()=>readBaalCombinedCustodyReaderScopeV1({...args,universe:{...args.universe,exchanges:[args.universe.exchanges[0]]}}),/UNIVERSE_LEG/);
 fs.writeFileSync(file,JSON.stringify({...descriptor,seedHash:'a'.repeat(64)}));
 assert.throws(()=>readBaalCombinedCustodyReaderScopeV1(args),/EXACT_SEAL/);
});
