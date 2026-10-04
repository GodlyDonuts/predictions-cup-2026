import test from 'node:test';import assert from 'node:assert/strict';
import {createBaalPairState,advanceBaalPair,baalPairReport,assertBaalPair,BAAL_PAIR_POLICY} from './baal-pair-paper-v3.mjs';
import fs from 'node:fs';
const base=Date.parse('2026-10-04T02:00:00.000Z'),at=n=>new Date(base+n).toISOString();
const capacity=JSON.parse(fs.readFileSync(new URL('../data/canonical-capacity-bank-20261003-v1.json',import.meta.url)));
const bank={mode:'baal_sequential_no_pair_paper',name:'test-baal-pair',cutoff:at(0),policy:BAAL_PAIR_POLICY,
 review:{...capacity.review,recordedAt:at(-1000),expiresAt:at(3600000)}};
function book(id,n,patch={}){const d=id==='1070';return{tournamentId:capacity.tournamentId,marketId:d?'381':'382',exchangeId:id,instrument:id+':YES',
 requestStartedAt:at(n),receivedAt:at(n+100),publishedAt:at(n+100),sourceAt:at(n+100),sequence:String(n),versionKey:n+':'+at(n+100),publicationSequence:n,publicationHash:'a'.repeat(64),
 bids:[{price:d?'0.810000':'0.220000',quantity:1000}],asks:[{price:d?'0.820000':'0.230000',quantity:1000}],...patch};}
function event(b,other,options={}){const pair=[b,other].filter(Boolean),start=Math.min(...pair.map(x=>Date.parse(x.requestStartedAt)))-base;
 return{kind:'book',availableAt:b.receivedAt,book:b,pairBooks:pair,accountEligibility:{schemaVersion:1,positionsAt:at(start-100),ordersAt:at(start-100),excludedExchangeIds:[]},admission:{positiveConditionalCapacity:true},...options};}
const step=(s,b,other,options)=>advanceBaalPair(s,event(b,other,options),bank);
function pendingEntry(){let s=createBaalPairState(bank);s=step(s,book('1070',1000),book('1071',1100),{availableAt:at(1200)});
 s=step(s,book('1070',10000),book('1071',11000),{availableAt:at(11100)});return s;}
function holding(){let s=pendingEntry();s=step(s,book('1070',20000),book('1071',11000));return step(s,book('1071',24000),book('1070',20000));}
const favorable=(id,n)=>book(id,n,{bids:[{price:id==='1070'?'0.790000':'0.190000',quantity:1000}],asks:[{price:id==='1070'?'0.800000':'0.200000',quantity:1000}]});

function refresh(s,id,n,other,accountN=n-1000,options={}){
 const b=book(id,n);return advanceBaalPair(s,event(b,other,{accountEligibility:{schemaVersion:1,positionsAt:at(accountN),ordersAt:at(accountN),excludedExchangeIds:[]},...options}),bank);
}
function firstConfirmation(){let s=createBaalPairState(bank),a=book('1070',1000),b=book('1071',4000);s=refresh(s,'1070',1000,null,500);return refresh(s,'1071',4000,a,500);}
test('serialized cycle refresh retains a non-executable marker, then confirms two later cofresh requests',()=>{
 let s=firstConfirmation();assert.ok(s.anchor);assert.equal(s.pending,null);const anchor=s.anchor.at;
 s=refresh(s,'1070',41000,book('1071',4000),40000);assert.equal(s.anchor.at,anchor);assert.equal(s.books['1071'],undefined);assert.equal(s.pending,null);assert.equal(s.fills.length,0);
 s=refresh(s,'1071',45000,book('1070',41000),40000);assert.equal(s.pending.stage,'entry_first');assert.equal(s.fills.length,0);assert.equal(s.pending.createdAt,at(45100));
 s=refresh(s,'1070',47000,book('1071',45000),40000);assert.equal(s.fills.length,0,'early request cannot fill');
 s=refresh(s,'1070',81000,book('1071',45000),80000);assert.equal(s.fills.length,1);assert.equal(s.pending.stage,'entry_second');
 s=refresh(s,'1071',85000,book('1070',81000),80000);assert.equal(s.fills.length,2);assert.equal(s.package.stage,'holding');assertBaalPair(s,bank);
});
test('anchor lifetime, boundary, explicit exclusions and unavailable account prevent a stale confirmation',()=>{
 let s=firstConfirmation();s=refresh(s,'1070',131000,book('1071',4000),130000);assert.equal(s.anchor,null);assert.equal(s.pending,null);assert.equal(s.counts.confirmation_anchor_expired,1);
 s=refresh(s,'1071',135000,book('1070',131000),130000);assert.ok(s.anchor);assert.equal(s.pending,null);
 for(const options of [{accountEligibility:null},{accountEligibility:{schemaVersion:1,positionsAt:at(40000),ordersAt:at(40000),excludedExchangeIds:['1071']}}]){
  const next=refresh(firstConfirmation(),'1070',41000,book('1071',4000),40000,options);assert.equal(next.anchor,null);assert.equal(next.pending,null);
 }
 s=advanceBaalPair(firstConfirmation(),{kind:'boundary',availableAt:at(10000)},bank);assert.equal(s.anchor,null);assert.equal(s.pending,null);
});
test('complete but unprofitable or unadmitted pair clears the marker and cannot recycle an old opportunity',()=>{
 const before=firstConfirmation(),bad=book('1071',12000,{bids:[{price:'0.190000',quantity:1000}]});
 let s=advanceBaalPair(before,event(bad,book('1070',10000)),bank);assert.equal(s.anchor,null);assert.equal(s.pending,null);
 s=refresh(before,'1071',12000,book('1070',10000),9000,{admission:{positiveConditionalCapacity:false}});assert.equal(s.anchor,null);assert.equal(s.pending,null);
});

test('paired inventory uses fixed caps and two later sequential requests; conditional floor is not P&L',()=>{
 let s=pendingEntry();assert.equal(s.pending.exchangeId,'1070','cheaper leg bounds unmatched principal');assert.equal(s.fills.length,0);
 s=step(s,book('1070',13000),book('1071',11000));assert.equal(s.fills.length,0,'request before latency cannot fill');
 s=step(s,book('1070',20000),book('1071',11000));assert.equal(s.fills.length,1);assert.equal(s.pending.stage,'entry_second');
 s=step(s,book('1071',22000),book('1070',20000));assert.equal(s.fills.length,1,'hedge has its own latency');
 s=step(s,book('1071',24000),book('1070',20000));assert.equal(s.fills.length,2);assert.equal(s.package.stage,'holding');
 const r=baalPairReport(s,bank,at(24100));assert.equal(r.ordinaryConditionalSettlementSurplus,'0.020000');assert.equal(r.realizedPnl,'0.000000');
 assert.equal(r.estimatedLiquidationPnl,'-0.040000');assert.equal(r.zeroPayoutExceptionLoss,'-0.980000');assert.equal(r.profitabilityEstablished,false);assertBaalPair(s,bank);
});
test('pair closes with covered sequential sales and exact zero-fee cash/FIFO',()=>{
 let s=holding();s=step(s,favorable('1070',40000),favorable('1071',41000),{availableAt:at(41100)});
 assert.equal(s.pending.stage,'exit_first');assert.equal(s.pending.exchangeId,'1071');
 s=step(s,favorable('1071',46000),favorable('1070',40000));assert.equal(s.pending.stage,'exit_second');
 s=step(s,favorable('1070',50000),favorable('1071',46000));assert.equal(s.cash,'100.010000');assert.equal(s.realizedPnl,'0.010000');
 assert.equal(s.roundtrips[0].realizedPnl,'0.010000');assert.equal(Object.keys(s.positions).length,0);assert.equal(s.fills.length,4);assertBaalPair(s,bank);
});
test('missed fixed hedge becomes an orphan and realizes its actual loss, never an assumed paired gain',()=>{
 let s=pendingEntry();s=step(s,book('1070',20000),book('1071',11000));
 s=step(s,book('1071',30000,{bids:[{price:'0.205000',quantity:1000}]}),book('1070',20000));assert.equal(s.fills.length,1);
 s=step(s,book('1071',145000),null);assert.equal(s.package.stage,'orphan');assert.equal(s.pending,null);
 const exit=n=>book('1070',n,{asks:[{price:'0.820000',quantity:1000}]});s=step(s,exit(150000));assert.equal(s.pending.stage,'orphan');
 s=step(s,exit(154000));assert.equal(s.cash,'99.980000');assert.equal(s.realizedPnl,'-0.020000');assert.equal(s.roundtrips[0].completedPair,false);assertBaalPair(s,bank);
});
test('second sale failure remains covered; its later orphan sale can erase the initially quoted profit',()=>{
 let s=holding();s=step(s,favorable('1070',40000),favorable('1071',41000),{availableAt:at(41100)});s=step(s,favorable('1071',46000),favorable('1070',40000));
 const bad=n=>book('1070',n,{bids:[{price:'0.830000',quantity:1000}],asks:[{price:'0.840000',quantity:1000}]});
 s=step(s,bad(50000),favorable('1071',46000));assert.equal(s.fills.length,3);
 s=step(s,bad(170000));assert.equal(s.pending.stage,'orphan');s=step(s,bad(174000));
 assert.equal(s.realizedPnl,'-0.030000');assert.equal(s.cash,'99.970000');assert.equal(s.roundtrips[0].realizedPnl,'-0.030000');assertBaalPair(s,bank);
});
test('restart/source boundary preserves inventory and losses, invalidates marks and cannot infer missed execution',()=>{
 let s=pendingEntry();s=step(s,book('1070',20000),book('1071',11000));
 s=advanceBaalPair(s,{kind:'boundary',availableAt:at(25000)},bank);
 assert.equal(s.fills.length,1);assert.equal(s.cash,'99.805000');assert.equal(s.package.stage,'orphan');assert.equal(s.pending,null);
 assert.equal(baalPairReport(s,bank,at(25000)).estimatedLiquidationPnl,null);assertBaalPair(s,bank);
});
test('account exclusions remove liquidation marks and block fills; expired review blocks new buys and conditional valuation',()=>{
 let s=pendingEntry();const x=event(book('1070',20000),book('1071',11000));x.accountEligibility.excludedExchangeIds=['1070'];s=advanceBaalPair(s,x,bank);assert.equal(s.fills.length,0);
 s=holding();const b=book('1070',30000),e=event(b);e.accountEligibility.excludedExchangeIds=['1070'];s=advanceBaalPair(s,e,bank);
 assert.equal(baalPairReport(s,bank,at(30100)).estimatedLiquidationPnl,null);
 assert.equal(baalPairReport(s,bank,at(3600000)).ordinaryConditionalSettlementSurplus,null);
 s=pendingEntry();s=step(s,book('1070',3601000));assert.equal(s.fills.length,0);assert.equal(s.package,null);
});
test('actual 39-share best bid is unusable at1%; entry, fill and liquidation walk to the next eligible rung',()=>{
 const d=n=>book('1070',n,{bids:[{price:'0.800000',quantity:39},{price:'0.795000',quantity:439}],asks:[{price:'0.805000',quantity:39},{price:'0.810000',quantity:7219}]}),
  r=n=>book('1071',n,{bids:[{price:'0.240000',quantity:1761}],asks:[{price:'0.245000',quantity:7700}]});
 let s=createBaalPairState(bank);s=step(s,d(1000),r(1100),{availableAt:at(1200)});s=step(s,d(10000),r(11000),{availableAt:at(11100)});
 assert.equal(s.pending.limit,'0.210000');s=step(s,d(20000),r(11000));assert.equal(s.fills[0].price,'0.210000');assert.equal(s.fills[0].source.levelIndex,1);assert.equal(s.fills[0].source.eligibleDepth,4);
 s=step(s,r(24000),d(20000));assert.equal(s.package.stage,'holding');assert.equal(s.cash,'99.025000');
 const report=baalPairReport(s,bank,at(24100));assert.equal(report.ordinaryConditionalSettlementSurplus,'0.025000');assert.equal(report.estimatedLiquidationPnl,'-0.040000');
 assert.equal(report.positions.find(p=>p.exchangeId==='1070').estimatedLiquidationProceeds,'0.185000');assertBaalPair(s,bank);
});
