import test from 'node:test';import assert from 'node:assert/strict';
import {tradeTapeFocus} from './trade-tape-focus.mjs';
const universe={exchanges:[['363','1052'],['297','986'],['376','1065'],['364','1053']].map(([marketId,exchangeId])=>({marketId,exchangeId}))};
const broad={legs:universe.exchanges.slice(2)};
test('broad membership includes original-window recovery and risk while narrow host is absent',()=>{
 const recoveryWindows=[{marketId:'376',exchangeId:'1065',key:'original'}];
 const p=tradeTapeFocus(universe,{activeBanks:[broad],riskMarketIds:['376'],recoveryWindows});
 assert.deepEqual(p.marketIds,['376','364']);assert.deepEqual(p.priorityMarketIds,['376']);assert.strictEqual(p.recoveryWindows,recoveryWindows);
 assert.deepEqual(tradeTapeFocus(universe,{activeBanks:[broad]}).priorityMarketIds,['376','364']);
});
test('expired-bank risk and recovery remain candidates even with no active entries',()=>{
 const p=tradeTapeFocus(universe,{riskMarketIds:['364'],recoveryWindows:[{marketId:'376',exchangeId:'1065'}]});
 assert.deepEqual(p.marketIds,['364','376']);assert.deepEqual(p.priorityMarketIds,['364']);
 assert.deepEqual(tradeTapeFocus(universe).marketIds,['363','297']);
});
test('foreign bank, risk and exact recovery exchange identities fail before selection',()=>{
 for(const p of [{activeBanks:[{legs:[{marketId:'376',exchangeId:'1053'}]}]},{riskMarketIds:['999']},{recoveryWindows:[{marketId:'376',exchangeId:'1053'}]}])assert.throws(()=>tradeTapeFocus(universe,p),/FOCUS_SCOPE/);
});
