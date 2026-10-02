import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {initialState,normalizeBook,stepDip,assertState} from './model.mjs';

const c=JSON.parse(fs.readFileSync(new URL('./fee-free-config.json',import.meta.url)));
const id='1066:YES',epoch=Date.parse('2026-10-02T06:00:00Z');
const at=seconds=>new Date(epoch+seconds*1000).toISOString();
const policy={reference:.6,allowEntry:true,minimumEdge:.02,takeProfit:'0.010000',horizonMs:3600000,maxMarketCost:'50.000000',executionPolicy:'responsive_v2'};
function book(seconds,{bid=.45,ask=.46,sequence=seconds,eventSeconds=seconds,requestSeconds=seconds,quantity=10000,bids}={}){
 return normalizeBook({contexts:[{type:'tournament',tournament:{id:c.tournamentId},orderbook:{exchanges:[{exchangeId:'1066',option:'YES',asOf:{at:at(eventSeconds),sequence},bids:bids||[{price:bid,quantity}],asks:[{price:ask,quantity:10000}]}]}}]},'377',c.tournamentId,at(seconds),at(requestSeconds),c.maxDataAgeMs,new Set(),'authoritative_receipt_v1');
}
const advance=(s,seconds,depth={},settings={})=>stepDip(s,book(seconds,depth),c,{...policy,...settings});
const wallet=s=>s.strategies.mean_reversion;
function entered(settings={}){
 let s=initialState(c,'responsive-exit-fixture',at(0));
 s=advance(s,1,{},settings);s=advance(s,3,{},settings);
 assert.equal(wallet(s).positions[id].quantity,10);return s;
}

test('quiet authoritative receipts trigger the horizon without reusing consumed depth',()=>{
 let s=entered();
 s=advance(s,4000,{sequence:3,eventSeconds:3},{allowEntry:false});
 assert.equal(wallet(s).pending[id].reason,'dip_timeout');
 assert.equal(wallet(s).pending[id].createdAt,at(4000));
 assert.equal(wallet(s).fills.length,1);
 s=advance(s,4002,{sequence:3,eventSeconds:3},{allowEntry:false});
 assert.equal(wallet(s).fills.length,1);
 assert.equal(wallet(s).positions[id].quantity,10);
 s=advance(s,4004,{}, {allowEntry:false});
 assert.equal(wallet(s).positions[id],undefined);
 assert.equal(wallet(s).fills.length,2);
});

test('a falling bid reprices risk exits and requires a later request after latency',()=>{
 let s=advance(entered(),5,{bid:.43,ask:.44},{allowEntry:false});
 assert.equal(wallet(s).pending[id].reason,'dip_stop');
 assert.equal(wallet(s).pending[id].limit,'0.425000');
 s=advance(s,7,{bid:.42,ask:.43},{allowEntry:false});
 const replacement=wallet(s).pending[id];
 assert.equal(replacement.limit,'0.415000');assert.equal(replacement.createdAt,at(7));
 assert.equal(wallet(s).fills.length,1);
 s=advance(s,8.2,{bid:.42,ask:.43,sequence:8,requestSeconds:7.5},{allowEntry:false});
 assert.equal(wallet(s).fills.length,1,'receipt after latency cannot make an earlier request causal');
 s=advance(s,9,{bid:.42,ask:.43,sequence:9},{allowEntry:false});
 assert.equal(wallet(s).fills.length,2);assert.equal(wallet(s).fills.at(-1).price,'0.415000');
 assert.equal(wallet(s).positions[id],undefined);
});

test('a large gap never fills below the prior decision limit or on its repricing observation',()=>{
 let s=advance(entered(),5,{bid:.43,ask:.44},{allowEntry:false});
 s=advance(s,7,{bid:.10,ask:.11},{allowEntry:false});
 assert.equal(wallet(s).fills.length,1);assert.equal(wallet(s).pending[id].limit,'0.095000');
 s=advance(s,9,{bid:.05,ask:.06},{allowEntry:false});
 assert.equal(wallet(s).fills.length,1);assert.equal(wallet(s).pending[id].limit,'0.045000');
 s=advance(s,11,{bid:.05,ask:.06},{allowEntry:false});
 assert.equal(wallet(s).fills.at(-1).price,'0.045000');
 assert.equal(wallet(s).realized,'-4.200000');
});

test('thin and partial exit depth stays bounded and risk mode survives a recovery',()=>{
 let s=advance(entered(),5,{bid:.43,ask:.44,quantity:99},{allowEntry:false});
 s=advance(s,7,{bid:.43,ask:.44,quantity:99},{allowEntry:false});
 assert.equal(wallet(s).fills.length,1,'one percent of 99 displayed shares cannot fill one share');
 s=advance(s,9,{bid:.46,ask:.47,quantity:400},{allowEntry:false});
 assert.equal(wallet(s).positions[id].quantity,6);assert.equal(wallet(s).fills.at(-1).quantity,4);
 assert.equal(wallet(s).pending[id].exitPolicy,'risk_reprice_v2');
 assert.equal(wallet(s).pending[id].reason,'dip_stop');
 s=advance(s,11,{bid:.46,ask:.47,quantity:400,sequence:9,eventSeconds:9},{allowEntry:false});
 assert.equal(wallet(s).positions[id].quantity,6,'consumed depth cannot fill twice');
 s=advance(s,13,{bid:.46,ask:.47,quantity:600},{allowEntry:false});
 assert.equal(wallet(s).positions[id],undefined);assert.equal(wallet(s).fills.length,3);
 assertState(s,c);
});

test('take-profit orders retain their limit until risk or timeout overrides them',()=>{
 let s=advance(entered(),5,{bid:.48,ask:.49},{allowEntry:false});
 assert.equal(wallet(s).pending[id].reason,'dip_take_profit');
 s=advance(s,7,{bid:.46,ask:.47},{allowEntry:false});
 assert.equal(wallet(s).pending[id].limit,'0.475000');assert.equal(wallet(s).pending[id].createdAt,at(5));
 s=advance(s,9,{bid:.43,ask:.44},{allowEntry:false});
 assert.equal(wallet(s).pending[id].reason,'dip_stop');assert.equal(wallet(s).pending[id].limit,'0.425000');
 assert.equal(wallet(s).fills.length,1);
 s=advance(s,11,{bid:.43,ask:.44},{allowEntry:false});
 assert.equal(wallet(s).positions[id],undefined);
});

test('timeout overrides a recently queued unfilled take-profit limit',()=>{
 let s=advance(entered(),3600,{bid:.48,ask:.49},{allowEntry:false});
 assert.equal(wallet(s).pending[id].reason,'dip_take_profit');
 s=advance(s,3604,{bid:.46,ask:.47},{allowEntry:false});
 assert.equal(wallet(s).pending[id].reason,'dip_timeout');assert.equal(wallet(s).pending[id].createdAt,at(3604));
 assert.equal(wallet(s).fills.length,1);
});

test('repeated observations do not duplicate completed exits or immediately reenter',()=>{
 let s=advance(entered(),5,{bid:.43,ask:.44});
 s=advance(s,7,{bid:.43,ask:.44});
 assert.equal(wallet(s).positions[id],undefined);assert.equal(wallet(s).pending[id],undefined);
 const after=structuredClone(wallet(s));
 s=advance(s,9,{bid:.43,ask:.44,sequence:7,eventSeconds:7});
 assert.deepEqual(wallet(s),after);
});

test('omitting the opt-in preserves legacy quiet-book and obsolete-limit behavior',()=>{
 const legacy={executionPolicy:undefined,allowEntry:false};
 let quiet=entered({executionPolicy:undefined});
 quiet=advance(quiet,4000,{sequence:3,eventSeconds:3},legacy);
 assert.equal(wallet(quiet).pending[id],undefined);assert.equal(wallet(quiet).positions[id].quantity,10);
 let falling=advance(entered({executionPolicy:undefined}),5,{bid:.43,ask:.44},legacy);
 falling=advance(falling,7,{bid:.42,ask:.43},legacy);
 assert.equal(wallet(falling).pending[id].limit,'0.425000');assert.equal(wallet(falling).pending[id].createdAt,at(5));
 assert.equal(wallet(falling).fills.length,1);
 assert.throws(()=>advance(initialState(c,'bad-policy',at(0)),1,{}, {executionPolicy:'unknown'}),/execution policy/);
});
