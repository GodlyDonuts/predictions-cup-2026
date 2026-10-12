import test from 'node:test';import assert from 'node:assert/strict';import {bulkHistoryFeatureV1 as feature} from './bulk-history-feature-v1.mjs';
const policy={lookbackMs:3600000,minimumPriorBooks:2,minimumHistorySpanMs:60000,maximumBulkAgeMs:120000,maximumBulkGapMs:120000};
const p=(received,bid='0.495000',ask='0.500000')=>({received,bid,ask,hash:String(received)});
test('causal rolling median may be between tradable ticks; future observations cannot affect it',()=>{const h=[p(0),p(60000),p(70000,'0.100000','0.105000')],r=feature(h,60000,policy);assert.equal(r.medianYesMid,'0.497500');assert.equal(r.observations,2);assert.equal(r.lastBulkHash,'60000');});
test('stale bulk observations and capture gaps cannot supply a fresh historical baseline',()=>{assert.equal(feature([p(0),p(60000)],200001,policy),null);assert.equal(feature([p(0),p(200000)],200000,policy),null);});
