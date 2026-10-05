import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';import {createHash} from 'node:crypto';
const dir=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime','research-profit-sprint-20261005');
const sources=JSON.parse(fs.readFileSync('research/profit-sprint-20261005/sources.json'));
let checked=0;
for(const s of sources.captures){if(s.status!=='captured')continue;const b=fs.readFileSync(path.join(dir,s.file));assert.equal(b.length,s.bytes);assert.equal(createHash('sha256').update(b).digest('hex'),s.contentHash);assert.equal(s.url.startsWith('https://'),true);checked++;}
const comparison=JSON.parse(fs.readFileSync('research/profit-sprint-20261005/texas-governor-comparison.json'));
assert.equal(comparison.primaryPolls.length,6);assert.equal(new Set(comparison.primaryPolls.map(p=>p.pollster)).size,6);
assert.equal(comparison.executionAuthority,false);assert.equal(comparison.activeBanksUnchanged,true);
assert(comparison.allVerifiedPrimaryPolls.ordinaryRange[0]<0.295);
assert(comparison.allVerifiedPrimaryPolls.centralEstimate<comparison.previousSelectionAtCurrentClock.centralEstimate);
assert.equal(comparison.unverifiedClockSensitivityPoll.primaryClockVerified,false);
assert.equal(comparison.leaveOnePollsterOut.length,6);
const bigdata=fs.readFileSync(path.join(dir,'bigdata-texas.html'),'utf8');for(const text of ['793','698','September 24','48.9','44.4'])assert(bigdata.includes(text));
const ids=JSON.parse(fs.readFileSync('research/profit-sprint-20261005/market-identities.json'));
assert.equal(ids.selected.find(x=>x.marketId==='293').exchangeId,'982');assert.equal(ids.selected.find(x=>x.marketId==='195').exchangeId,'884');assert.deepEqual(ids.settlementDates,['2026-11-04T17:00:00.000Z']);
console.log(JSON.stringify({publicSourceHashesVerified:checked,scopeAndSensitivityChecksPassed:true,accountApiRequests:0,executionAuthority:false}));
