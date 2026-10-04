import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {instrumentLabelPaths} from './label-paths.mjs';
import {roundTripLabel} from './roundtrip-target.mjs';
const c={...JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url))),feeBps:0};
const epoch=Date.parse('2026-10-04T00:00:00Z'),at=n=>new Date(epoch+n*1000).toISOString();
test('indexed same-instrument paths reproduce full chronological scan labels, censorship and boundaries exactly',()=>{
 const rows=Array.from({length:240},(_,i)=>{
  const k=i%4,n=Math.floor(i/4),marketId=String(377+k),instrument=(9+k)+':YES',time=at(i*10),bid=n%12<3?.36:.41;
  return {at:time,instrument,marketId,historyLength:n,segment:n<35?0:1,x:[1],book:{marketId,exchangeId:String(9+k),instrument,sequence:String(n),
   sourceAt:time,receivedAt:time,requestStartedAt:time,versionKey:n+':'+time,bids:[{price:bid.toFixed(6),quantity:n%13?10000:1}],asks:[{price:(bid+.01).toFixed(6),quantity:n%17?10000:1}]}};
 });
 const paths=instrumentLabelPaths(rows);
 for(const horizon of [60000,180000,300000,600000])for(let i=0;i<rows.length;i++){
  const p=paths[i];assert.equal(p.rows[p.index],rows[i]);
  assert.deepEqual(roundTripLabel(p.rows,p.index,c,horizon),roundTripLabel(rows,i,c,horizon));
 }
 assert.equal(paths[0].rows.length,60);assert.equal(paths[0].rows[1],rows[4]);
});
