import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {Journal} from './runtime.mjs';
import {replayJournal} from './replay.mjs';
import {initialState,restoreState,assertState} from './model.mjs';
const c=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url)));
test('captured book replay produces independent paper statistics with checked source checksum',t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-paper-replay-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));const file=path.join(dir,'j'),j=new Journal(file);
 for(let n=1;n<=2;n++){const at=new Date(Date.parse('2026-10-01T23:00:00Z')+30000*n).toISOString();j.commit({}, {type:'cycle',accountExclusions:[],sourceSnapshots:[{endpoint:'/markets/377/orderbook?tournamentId='+c.tournamentId+'&depth=20',requestStartedAt:at,receivedAt:at,data:{contexts:[{type:'tournament',tournament:{id:c.tournamentId},orderbook:{exchanges:[{exchangeId:'9',option:'YES',asOf:{at,sequence:n},bids:[{price:.4,quantity:10000}],asks:[{price:.41,quantity:10000}]}]}}]}}]});}
 const raw=fs.readFileSync(file,'utf8'),r=replayJournal(raw,c);assert.equal(r.observations,2);assert.equal(r.strategies.buy_hold_benchmark.cash,'99995.845850');assert.equal(r.strategies.buy_hold_benchmark.fills,1);assert.equal(r.replay.networkRequests,0);assert.match(r.replay.sourceHash,/^[a-f0-9]{64}$/);assert.throws(()=>replayJournal(raw.replace('10000','20000'),c),/integrity/);assert.throws(()=>replayJournal(raw.slice(0,-1),c),/Incomplete/);
});
test('legacy entry-only state upgrade preserves exact accounting; unknown revisions rejected',()=>{
 const s=initialState(c,'upgrade','2026-10-01T23:00:00.000Z');delete s.engineRevision;
 const st=s.strategies.buy_hold_benchmark;st.fills=[{instrument:'9:YES',action:'buy',quantity:2,price:'0.400000',fee:'0.000800'}];st.cash='99999.199200';st.fees='0.000800';st.turnover='0.800000';st.positions={'9:YES':{quantity:2,cost:'0.800800',enteredAt:s.startedAt}};
 const restored=restoreState(s,c);assertState(restored,c);assert.equal(restored.strategies.buy_hold_benchmark.cash,st.cash);assert.deepEqual(restored.strategies.buy_hold_benchmark.positions['9:YES'].lots,[{quantity:2,cost:'0.800800'}]);assert.throws(()=>restoreState({...restored,engineRevision:999},c),/Unknown/);
 const damaged=structuredClone(restored);damaged.strategies.buy_hold_benchmark.positions['9:YES'].cost='0.100000';assert.throws(()=>assertState(damaged,c),/Position/);
});
