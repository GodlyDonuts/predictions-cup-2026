import test from 'node:test';import assert from 'node:assert/strict';import {flatExpiredPaper} from './expired-flat-paper-retirement.mjs';
test('expired paper retirement rejects any unknown accounting or remaining liability',()=>{
 const s={failed:false,riskMarketIds:[],wallets:{one:{positions:[],unresolvedQuotes:[],reserved:'0.000000'}}};
 assert.equal(flatExpiredPaper(s,'2026-10-04T00:00:00Z',Date.parse('2026-10-05T00:00:00Z')),true);
 for(const w of [{positions:[{}],unresolvedQuotes:[],reserved:'0.000000'},{positions:[],unresolvedQuotes:[{}],reserved:'0.000000'},{positions:[],unresolvedQuotes:[],reserved:'0.005000'},{positions:[],unresolvedQuotes:[]}])assert.equal(flatExpiredPaper({...s,wallets:{one:w}},'2026-10-04T00:00:00Z'),false);
 assert.equal(flatExpiredPaper({...s,failed:true},'2026-10-04T00:00:00Z'),false);assert.equal(flatExpiredPaper(s,'invalid'),false);assert.equal(flatExpiredPaper(s,'2030-01-01T00:00:00Z'),false);
});
