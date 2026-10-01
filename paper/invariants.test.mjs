import assert from 'node:assert/strict';
import test from 'node:test';
import {PaperEngine} from './engine.mjs';
import {amount} from './fixed.mjs';
const at=n=>new Date(Date.parse('2026-10-01T00:00:00.000Z')+n).toISOString();
test('15000 seeded events preserve cash, FIFO, inventory, fees and print-volume invariants', () => {
let checked=0;
for(let seed=1;seed<=50;seed++){
 let rng=seed; const r=n=>{rng=(Math.imul(rng,1664525)+1013904223)>>>0;return rng%n};
 const cfg={schemaVersion:1,initialCash:'100',maxDataAgeMs:10000,orderLatencyMs:r(5),cancelLatencyMs:r(5),maxPositionShares:200,costs:{assumption:'unverified_scenario',feeBps:r(10001),feePerShare:`0.${String(r(100001)).padStart(6,'0')}`}};
 const e=new PaperEngine(cfg);let bs='1';
 for(let t=0;t<300;t++){
  const x={schemaVersion:1,eventId:`e${t}`,receivedAt:at(t),occurredAt:at(t)}; const kind=t===0?0:r(10);
  if(kind===0){bs=String(t+1); Object.assign(x,{type:'snapshot',instrument:'X',sequence:bs,authoritative:true,nextExpiryAt:null,bids:[{price:'0.3',quantity:1+r(100),ownQuantity:0}],asks:[{price:'0.7',quantity:1+r(100),ownQuantity:0}]})}
  else if(kind<4){const action=r(2)?'buy':'sell';Object.assign(x,{type:'submit',instrument:'X',orderId:`o${t}`,action,quantity:1+r(20),limitPrice:action==='buy'?'0.3':'0.7',expiresAt:r(3)?null:at(t+1+r(50))})}
  else if(kind<8){const ag=r(2)?'buy':'sell';Object.assign(x,{type:'trade',instrument:'X',sequence:String(t+1),bookSequence:bs,price:ag==='buy'?'0.7':'0.3',quantity:1+r(100),aggressor:ag,externalAggressor:r(5)!==0,evidence:'explicit_trade'})}
  else if(kind===8&&e.orders.size){const os=[...e.orders.keys()];Object.assign(x,{type:'cancel',orderId:os[r(os.length)]})}
  else Object.assign(x,{type:'clock'});
  const oldFills=e.fills.length;e.apply(x);
  assert.equal(e.cash+e.basis('X')-e.realized,e.initialCash);
  assert.ok(e.cash>=e.reservedCash()); assert.ok(e.quantity('X')>=e.reservedShares('X'));
  assert.equal(e.fills.reduce((a,f)=>a+amount(f.fee),0n),e.totalFees);
  if(x.type==='trade')assert.ok(e.fills.slice(oldFills).reduce((a,f)=>a+f.quantity,0)<=x.quantity);
  for(const action of ['buy','sell']){
   let floor=0n;
   for(const o of [...e.orders.values()].filter(o=>['open','pending_cancel'].includes(o.status)&&o.activatedAt!==null&&o.action===action).sort((a,b)=>a.ordinal-b.ordinal)){
    assert.ok(o.queueAhead>=floor,`queue overlap seed${seed} t${t}`); floor=o.queueAhead+BigInt(o.remaining);
   }
  }
  checked++;
 }
}
assert.equal(checked, 15000);
});
