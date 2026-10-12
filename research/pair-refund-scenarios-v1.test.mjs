import test from 'node:test';import assert from 'node:assert/strict';import {pairRefundScenariosV1 as scenarios} from './pair-refund-scenarios-v1.mjs';
test('ordinary floor, both refunds and asymmetric refund losses differ',()=>{
 const r=scenarios(['0.575000','0.400000']);assert.equal(r.rows.length,9);assert.equal(r.ordinaryMinimumNet,'0.025000');
 const row=(a,b)=>r.rows.find(x=>x.outcomes[0]===a&&x.outcomes[1]===b);
 assert.equal(row('REFUND','REFUND').net,'0.000000');assert.equal(row('REFUND','YES').net,'-0.400000');assert.equal(row('YES','REFUND').net,'-0.575000');
 assert.equal(row('YES','YES').net,'-0.975000');assert.equal(row('YES','YES').category,'fusion_or_inconsistent_both_parties_win');
});
test('side swap preserves the scenario economics and principal',()=>{
 const a=scenarios(['0.575000','0.410000']),b=scenarios(['0.410000','0.575000']);
 for(const row of a.rows)assert.equal(row.net,b.rows.find(x=>x.outcomes[0]===row.outcomes[1]&&x.outcomes[1]===row.outcomes[0]).net);
 assert.equal(a.principal,b.principal);
});
test('invalid principal cannot manufacture refund or payout value',()=>{
 for(const c of [[],['0.000000','0.400000'],['1.000000','0.400000']])assert.throws(()=>scenarios(c));
});

