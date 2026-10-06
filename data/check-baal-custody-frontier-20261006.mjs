// Local integration checks on the actual private frontier; no API and no writes
// to originals. CI without the private runtime must run pure component tests.
import path from 'node:path';import assert from 'node:assert/strict';
import {readBaalCustodyFrontierV1,assertBaalCustodyFrontierCanContinue as check} from '../execution/baal-custody-frontier-v1.mjs';
const f=readBaalCustodyFrontierV1({runtimeDir:path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),workspace:process.cwd()});
assert.equal(check(f).freshAccountStillRequired,true);
let x=structuredClone(f);x.summary.ownedQuantity=0;assert.throws(()=>check(x),/SUMMARY_ECONOMICS/);
x=structuredClone(f);x.currentState.orders=[];assert.throws(()=>check(x),/EXACT_FRONTIER/);
x=structuredClone(f);x.risk.cashReserved='0.000000';assert.throws(()=>check(x),/CURRENT_FULL_RESERVATION/);
x=structuredClone(f);x.executionAuthority=true;assert.throws(()=>check(x),/EXACT_FRONTIER/);
console.log(JSON.stringify({checks:5,summary:f.summary,currentStateHash:f.currentStateHash,networkRequests:0,originalJournalsChanged:false,executionAuthority:false}));
