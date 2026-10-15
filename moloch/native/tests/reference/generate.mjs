// Development-only reference generator. Not shipped or invoked by the Rust VPS.
import fs from 'node:fs';
import {createMoloch,advanceMoloch,reportMoloch} from '../../../engine.mjs';
const at=n=>new Date(Date.parse('2026-10-13T00:00:00Z')+n*1000).toISOString();
const book=(n,ex='101',sequence=1,bid='0.450000',ask='0.455000')=>({type:'book',receivedAt:at(n),book:{tournamentId:'cup',marketId:ex,exchangeId:ex,sequence:String(sequence),requestStartedAt:at(n),sourceAt:at(n),receivedAt:at(n),bids:[{price:bid,quantity:1000}],asks:[{price:ask,quantity:1000}]}});
const trade=(n,id,yesPrice='0.440000',quantity=100,executed=n)=>({type:'trade',receivedAt:at(n),executedAt:at(executed),id,tournamentId:'cup',marketId:'101',exchangeId:'101',yesPrice,quantity});
const tick=n=>({type:'tick',receivedAt:at(n)});
const gap=n=>({type:'gap',receivedAt:at(n),exchangeId:'101'});
const coverage=(n,sequence='20')=>({type:'coverage',receivedAt:at(n),marketId:'101',exchangeId:'101',tournamentId:'cup',from:at(3),to:at(n-1),complete:true,projectedThroughSequence:sequence});
const scenarios=[
 ['paired fees',{quantity:1,feePerShare:'0.000100'},[book(0),trade(4,'a'),trade(5,'b','0.465000')]],
 ['delayed cancel and coverage',{quantity:1},[book(0),gap(4),tick(7),trade(8,'late','0.440000',100,6),book(12,'101',20),coverage(13,'19'),coverage(14)]],
 ['duplicate precision',{quantity:1},[book(0),trade(1,'early'),trade(4,'touch','0.450000'),trade(5,'a'),{...trade(6,'a','0.440000',100,5),executedAt:at(5).replace('.000Z','.000000000Z')},tick(70)]],
 ['conditional queue',{quantity:1,fillModel:'conditionalFifo'},[book(0),trade(4,'a','0.450000',999),trade(5,'a','0.450000',999,4),trade(6,'b','0.450000',101),trade(7,'c','0.455000',1100)]],
 ['conditional fraction',{quantity:1,fillModel:'conditionalFifo',touchAttributionBps:5000},[book(0),trade(4,'a','0.450000',999),trade(6,'b','0.450000',101)]],
 ['inside inventory',{quantity:1,insideTicks:1},[book(0,'101',1,'0.450000','0.465000'),trade(4,'a','0.450000'),tick(8),book(9,'101',20,'0.450000','0.460000'),{...coverage(10),to:at(9)},trade(14,'b','0.460000'),trade(15,'c','0.465000')]],
 ['early renewal',{renewBeforeMs:45000,maximumMarketCost:'20.000000'},[book(0),tick(59),book(81,'101',2),tick(84),tick(88)]],
 ['scarce',{maximumCommitted:'0.500000'},[book(0),tick(4)]],
 ['fee rejection',{feePerShare:'0.002000'},[book(0),trade(4,'a')]],
 ['microprice',{quoteMode:'microprice',maximumCommitted:'0.500000',maximumMarketCost:'0.500000'},[book(0),trade(4,'a'),tick(20)]],
 ['backoff',{entryBackoffTicks:2},[book(0),trade(4,'a','0.430000'),trade(5,'b','0.470000')]],
 ['deadline',{entryStopAt:at(15),quantity:1},[book(0),gap(4),tick(7),book(12,'101',20),coverage(13),coverage(14),book(16,'102')]],
 ['causal current flow',{requireCurrentFlow:true,quantity:1},[book(0),{...trade(4,'bid','0.450000'),idNamespace:'sig_engine_match'},{...trade(5,'ask','0.455000'),idNamespace:'sig_engine_match'},book(6,'101',2),trade(9,'entry'),gap(10),book(14,'101',20),coverage(15)]],
 ['flow regime reset',{requireCurrentFlow:true},[book(0),{...trade(4,'bid','0.450000'),idNamespace:'sig_engine_match'},book(5,'101',2,'0.445000','0.455000'),{...trade(6,'ask','0.455000'),idNamespace:'sig_engine_match'},tick(8),{...trade(9,'new-bid','0.445000'),idNamespace:'sig_engine_match'},tick(190)]],
];
let seed=1701;const random=()=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed/4294967296;};
for(const fillModel of ['strictThrough','conditionalFifo'])for(const quoteMode of ['pairedSpread','microprice','cheapSpread']){
 const events=[];let t=0,sequence=0;for(let i=0;i<100;i++){t+=1+Math.floor(random()*5);if(i%3===0){const bid=0.005*(60+Math.floor(random()*40)),ask=bid+0.005*(1+Math.floor(random()*5));events.push(book(t,'101',++sequence,bid.toFixed(6),ask.toFixed(6)));}else events.push(trade(t,`random-${i}`,(0.005*(55+Math.floor(random()*50))).toFixed(6),1+Math.floor(random()*200)));}
 scenarios.push([`seeded ${fillModel} ${quoteMode}`,{fillModel,quoteMode,quantity:3},events]);
}
const fields=['policyHash','events','cash','realized','fees','ownedCost','reservedCash','netLiquidationPnl','positions','fills','orders','activeOrders','unresolvedOrders','fillModel','conditionalQueueFills','profitSignalEligible','counts'];
const output=scenarios.map(([name,policy,events])=>{const s=createMoloch(policy);return {name,policy,steps:events.map(event=>{advanceMoloch(s,event);const r=reportMoloch(s);return{event,expected:structuredClone(Object.fromEntries(fields.map(k=>[k,r[k]])))};})};});
fs.writeFileSync(new URL('./golden.json',import.meta.url),JSON.stringify(output)+'\n');console.log(`${output.length} scenarios, ${output.reduce((n,s)=>n+s.steps.length,0)} event checkpoints`);
