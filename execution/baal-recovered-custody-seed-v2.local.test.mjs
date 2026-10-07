import fs from 'node:fs';import path from 'node:path';import test from 'node:test';import assert from 'node:assert/strict';
import {readBaalCustodyFrontierV1} from './baal-custody-frontier-v1.mjs';import {buildBaalRecoveredCustodySeedV2,validateBaalRecoveredCustodySeedV2} from './baal-recovered-custody-seed-v2.mjs';import {payloadHash} from './core.mjs';
const rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
test('actual live recovery imports every original order and the expired blocked body without changing holdings',{skip:!fs.existsSync(path.join(rt,'blocked-admission-recovery-result.json'))},()=>{
 const frontier=readBaalCustodyFrontierV1({runtimeDir:rt,workspace:process.cwd()}),request=JSON.parse(fs.readFileSync(path.join(rt,'baal-generic-live-request-v2.json'))),seed=buildBaalRecoveredCustodySeedV2({frontier,scope:request.scope,allocation:request.allocation,runtimeDir:rt});
 assert.equal(seed.inheritedClosedOrders.length,18);assert.equal(seed.inheritedClosedOrders.at(-1).phase,'abandoned');assert.equal(seed.summary.ownedQuantity,68);assert.equal(seed.summary.realizedPnl,'0.975000');assert.equal(seed.summary.abandonedOrders,4);assert.equal(seed.executionAuthority,false);assert.equal(validateBaalRecoveredCustodySeedV2(seed,request.scope,request.allocation),seed.seedHash);
 const bad=structuredClone(seed);bad.inheritedClosedOrders.at(-1).body.quantity++;assert.throws(()=>validateBaalRecoveredCustodySeedV2(bad,request.scope,request.allocation),/HASH/);assert.equal(payloadHash(seed.inventory),payloadHash(frontier.currentState.currentInventory));
});
