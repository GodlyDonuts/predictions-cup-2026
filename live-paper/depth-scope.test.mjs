import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {validateDepthPlan,scopedSnapshots} from './depth-scope.mjs';
import {replayJournal} from './replay.mjs';
import {captureRows} from './ml-bank.mjs';
import {digest} from './model.mjs';
const c=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url)));
test('supplemental scope is bounded and requires a unique catalogued YES exchange',()=>{
 const u={exchanges:[{marketId:'342',exchangeId:'1031',option:'YES'},{marketId:'376',exchangeId:'1065',option:'YES'}]},p={tournamentId:c.tournamentId,marketIds:['342','376']};
 assert.equal(validateDepthPlan(p,c,u),p);
 for(const marketIds of [['342','342'],['342','376','99'],['377'],['999']])assert.throws(()=>validateDepthPlan({...p,marketIds},c,u));
 assert.throws(()=>validateDepthPlan(p,c,{exchanges:u.exchanges.map(x=>({...x,option:'NO'}))}));
});
test('extra failures cannot invalidate a frozen scope; future cohorts select additional books',()=>{
 const core={endpoint:'/markets/377/orderbook?tournamentId=T'},extra={endpoint:'/markets/342/orderbook?tournamentId=T'},failure={marketId:'342',skipped:'http_503'};
 const e={sourceSnapshots:[core],supplementalDepth:[extra,failure]};assert.deepEqual(scopedSnapshots(e,c),[core]);assert.deepEqual(scopedSnapshots(e,{marketIds:['342']}),[extra,failure]);assert.throws(()=>scopedSnapshots({...e,supplementalDepth:{}},c));
});
test('replay and ML rows consume supplemental captures only under selected configuration',()=>{
 const at='2026-10-02T02:00:00Z';
 const snap=id=>({endpoint:'/markets/'+id+'/orderbook?tournamentId='+c.tournamentId+'&depth=20',requestStartedAt:at,receivedAt:at,data:{contexts:[{type:'tournament',tournament:{id:c.tournamentId},orderbook:{exchanges:[{exchangeId:id==='342'?'1031':'1066',option:'YES',asOf:{sequence:1,at},bids:[{price:.4,quantity:10000}],asks:[{price:.41,quantity:10000}]}]}}]}});
 const body={sequence:1,previous:'',at,event:{type:'cycle',clockAt:at,sourceSnapshots:[snap('377')],supplementalDepth:[snap('342')],accountExclusions:[]}};
 const raw=JSON.stringify({...body,hash:digest(body)})+'\n';
 assert.equal(captureRows(raw,c).rows.length,1);assert.equal(captureRows(raw,{...c,marketIds:['377','342']}).rows.length,2);
 assert.equal(replayJournal(raw,c).observations,1);assert.equal(replayJournal(raw,{...c,marketIds:['377','342']}).observations,2);
});
