import test from 'node:test';import assert from 'node:assert/strict';
import {verifyNoBuyEconomics} from './no-order-economics.mjs';
const t=n=>`2026-10-02T22:00:${String(n).padStart(2,'0')}.000Z`;
function fixture(){return {binding:{orderId:'1001',exchangeId:'1070',marketId:'381',tournamentId:'test',side:'no',action:'buy',quantity:6,sideRelativeLimit:'0.165000',preparedAt:t(2)},
 ack:{orderId:1001,exchangeId:'1070',tournamentId:'test',side:'no',action:'buy',quantity:6,price:.165,open:false,remainingQuantity:0,quantityTraded:6,totalCost:'0.990000'},
 fills:{orderId:'1001',exchangeId:'1070',tournamentId:'test',complete:true,rows:[{id:5001,orderId:'1001',exchangeId:'1070',marketId:'381',tournamentId:'test',side:'no',quantity:-6,price:.165,filledAt:t(3)}]},
 before:{tournamentId:'test',complete:true,cashBasis:'gross_cash',cash:'100.000000',startedAt:t(0),completedAt:t(1),positions:[],openOrders:[]},
 after:{tournamentId:'test',complete:true,cashBasis:'gross_cash',cash:'99.010000',startedAt:t(4),completedAt:t(5),positions:[{exchangeId:'1070',marketId:'381',signedQuantity:-6}],openOrders:[]},
 activity:{tournamentId:'test',complete:true,from:t(0),through:t(5),rows:[{kind:'fill',orderId:'1001',fillId:5001,at:t(3)}]},readAt:t(6)};}
test('side-relative NO evidence agrees with exact isolated cash and position changes',()=>{const s=verifyNoBuyEconomics(fixture());assert.equal(s.priceBasis,'side_relative');assert.equal(s.principalCost,'0.990000');assert.equal(s.signedPositionDelta,-6);assert.equal(s.dispatchAuthority,false);assert.equal(s.globalConventionEstablished,false);});
test('YES-denominated candidate is inferred only for its own reconciled order',()=>{const x=fixture();x.fills.rows[0].price=.835;assert.equal(verifyNoBuyEconomics(x).priceBasis,'yes_denominated');});
test('known binary complementary tick representations retain exact prices',()=>{const x=fixture();x.fills.rows[0].price=1-.835;assert.equal(verifyNoBuyEconomics(x).principalCost,'0.990000');});
test('economically identical half-price hypotheses remain ambiguous',()=>{const x=fixture();x.binding.sideRelativeLimit='0.500000';x.ack.price=.5;x.ack.totalCost='3.000000';x.after.cash='97.000000';x.fills.rows[0].price=.5;assert.throws(()=>verifyNoBuyEconomics(x),/AMBIGUOUS/);});
test('duplicate fill delivery deduplicates but changed repeated IDs reject',()=>{const x=fixture();x.fills.rows.push(structuredClone(x.fills.rows[0]));assert.equal(verifyNoBuyEconomics(x).filledQuantity,6);x.fills.rows[1].price=.16;assert.throws(()=>verifyNoBuyEconomics(x),/CONFLICTING/);});
test('pre-existing NO lots remain external; verifier checks only new delta',()=>{const x=fixture();x.before.positions=[{exchangeId:'1070',marketId:'381',signedQuantity:-10}];x.after.positions[0].signedQuantity=-16;assert.equal(verifyNoBuyEconomics(x).signedPositionDelta,-6);});
test('two price-improved fills verify quantity weighted principal without averaging ticks',()=>{const x=fixture();x.fills.rows[0].quantity=-3;x.fills.rows.push({...x.fills.rows[0],id:5002,price:.16});x.ack.totalCost='0.975000';x.after.cash='99.025000';x.activity.rows.push({...x.activity.rows[0],fillId:5002});assert.equal(verifyNoBuyEconomics(x).principalCost,'0.975000');});
for(const [label,change]of [
 ['wrong cash',x=>x.after.cash='99.000000'],['wrong position',x=>x.after.positions[0].signedQuantity=-5],
 ['external activity',x=>x.activity.rows.push({kind:'settlement',at:t(4)})],
 ['incomplete activity',x=>x.activity.complete=false],['available cash',x=>x.before.cashBasis='available_collateral'],
 ['partial acknowledgement',x=>x.ack.open=true],['partial fills',x=>x.fills.rows[0].quantity=-3],
 ['complement SELL response',x=>x.ack.action='sell'],['YES netting',x=>x.before.positions=[{exchangeId:'1070',marketId:'381',signedQuantity:3}]],
 ['unrelated position change',x=>x.after.positions.push({exchangeId:'1071',marketId:'382',signedQuantity:-1})],
 ['future fill',x=>x.fills.rows[0].filledAt=t(7)],['late baseline',x=>x.before.completedAt=t(3)],
 ['ack binary money cannot be rounded',x=>x.ack.totalCost=.99],['stale account',x=>x.readAt='2026-10-02T22:04:00.000Z'],
 ['open external order',x=>x.before.openOrders=[{id:999}]],['wrong sign',x=>x.fills.rows[0].quantity=6],
 ['fill scope mismatch',x=>x.fills.rows[0].tournamentId='other'],['duplicate activity',x=>x.activity.rows.push({...x.activity.rows[0]})],
 ['activity omitted first second',x=>x.activity.from=t(1)],['off-tick fill',x=>x.fills.rows[0].price=.1651]
])test(`reject ${label}`,()=>{const x=fixture();change(x);assert.throws(()=>verifyNoBuyEconomics(x));});
