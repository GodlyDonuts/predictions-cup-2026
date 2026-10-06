import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {verifyScopedRestNoMultishare as verify} from './no-rest-multishare-reconciliation-v1.mjs';
import {canonicalPayload,payloadHash} from './core.mjs';
import {amount,money} from '../paper/fixed.mjs';

const T='bda92870-621e-47b0-bc3c-3602c5c26f55',N=Date.parse('2026-10-06T16:00:00.000Z');
const scope={ownerId:'Codex',runId:'multishare-offline-test',tournamentId:T,tournamentSlug:'midterm-elections',marketId:'377',exchangeId:'700'};
const iso=x=>new Date(x).toISOString(),sha=x=>createHash('sha256').update(x).digest('hex');
function raw(method,endpoint,data,start,end=start+50){const bodyText=JSON.stringify(data);return {method,endpoint,status:200,bodyText,sourceBodyHash:sha(bodyText),requestStartedAt:iso(start),receivedAt:iso(end),fetchInvocationId:'fetch-'+start};}
function edit(r,fn){const d=JSON.parse(r.bodyText);fn(d);r.bodyText=JSON.stringify(d);r.sourceBodyHash=sha(r.bodyText);return r;}
function lex(r,from,to){assert.ok(r.bodyText.includes(from));r.bodyText=r.bodyText.replace(from,to);r.sourceBodyHash=sha(r.bodyText);}
const page=data=>({data,pagination:{limit:200,hasMore:false,nextCursor:null},coverage:{complete:true,projectedThroughSequence:100}});
function account(start,cash,qty,history=[]){
 return {cash:raw('GET','/tournaments/midterm-elections',{id:T,isPendingEnrolment:false,myBalance:cash},start),
  positions:raw('GET','/tournaments/midterm-elections/portfolio/positions',{positions:[{marketId:'399',exchangeId:'799',quantity:4,settled:false},...(qty?[{marketId:'377',exchangeId:'700',quantity:qty,settled:false}]:[])],summary:{totalMarketValue:1,totalCostBasis:1,totalUnrealizedPnl:0}},start+100),
  openOrdersPages:[raw('GET','/orders?status=open&limit=200&tournamentId='+T,page([]),start+200)],
  scopedOrdersPages:[raw('GET','/orders?status=all&exchangeId=700&limit=200&tournamentId='+T,page(history),start+300)]};
}
function fixture({action='buy',quantity=5,fills=[{quantity:2,price:'0.100000'},{quantity:3,price:'0.105000'}],limit=action==='buy'?'0.200000':'0.105000',base=N,orderId=900,prior=null,ackFilled=null,ackOpen=false}={}){
 const created=base+1000,expiration=created+30000,total=fills.reduce((n,f)=>n+f.quantity,0),cost=fills.reduce((n,f)=>n+amount(f.price)*BigInt(f.quantity),0n);
 const history=prior?JSON.parse(prior.after.scopedOrdersPages[0].bodyText).data:[],oldQty=prior?.proof.remainingOwnedQuantity??0,priorCash=prior?JSON.parse(prior.after.cash.bodyText).myBalance:1000;
 const b={...scope,orderId:String(orderId),intentId:'intent-'+orderId,idempotencyKey:'key-'+orderId,action,side:'no',quantity,limit,createdAt:iso(created),expirationDate:iso(expiration)};
 const before=account(base,priorCash,-oldQty,history),postQuantity=oldQty+(action==='buy'?total:-total),postCash=Math.round((priorCash+(action==='buy'?-1:1)*Number(cost)/1e6)*100)/100;
 const od={id:orderId,exchangeId:'700',tournamentId:T,side:'no',action,quantity,quantityFilled:total,priceLimit:Number(limit),open:false,createdAt:iso(base+1100),expirationDate:iso(expiration)};
 const rows=fills.map((f,i)=>({id:orderId*100+i,side:'no',quantity:-f.quantity,price:Number(f.price),filledAt:iso(base+1500+i*10)}));
 const ackQty=ackFilled??total;let left=ackQty,ackCost=0n,last=null;
 for(const f of fills){const q=Math.min(left,f.quantity);if(q){ackCost+=amount(f.price)*BigInt(q);last=Number(f.price);left-=q;}}
 const all={fullNotionalCost:Number(ackCost)/1e6,effectiveEntryCost:Number(ackCost)/1e6,collateralSavings:0,relationshipIds:[],componentId:null,guaranteedPayoutFloorAfter:0,outstandingAdvanceAfter:0,collateralRepayment:0,netBuyingPowerImpact:(action==='buy'?1:-1)*Number(ackCost)/1e6,redemptionCredit:0};
 const request={idempotencyKey:b.idempotencyKey,exchangeId:'700',side:'no',action,quantity,price:Number(limit),expirationDate:b.expirationDate,tournamentId:T};
 const ack=raw('POST','/orders',{orderId,exchangeId:'700',side:'no',action,quantity,quantityTraded:ackQty,remainingQuantity:quantity-ackQty,open:ackOpen,price:Number(limit),fillPrice:last,totalCost:Number(ackCost)/1e6,all},base+1100,base+2000);
 ack.requestBodyHash=payloadHash(request);
 const leaseIdentity={ownerId:scope.ownerId,tournamentId:T,leaseId:'original-local-lease'};
 const dispatchReceipt={...scope,intentId:b.intentId,idempotencyKey:b.idempotencyKey,method:'POST',endpoint:'/orders',requestBodyJson:canonicalPayload(request),requestBodyHash:payloadHash(request),requestStartedAt:ack.requestStartedAt,fetchInvocationId:ack.fetchInvocationId,leaseIdentity,leaseHash:payloadHash(leaseIdentity)};
 const fillPage=raw('GET','/orders/'+orderId+'/fills?limit=200',{...page(rows),orderId,exchangeId:'700',tournamentId:T,totalQuantityFilled:-total,avgFillPrice:total?Number(cost)/1e6/total:null},base+3200);
 const after=account(base+4000,postCash,-postQuantity,[...history,od]);
 return {binding:b,before,ack,order:raw('GET','/orders/'+orderId,od,base+3000),fillPages:[fillPage],after,dispatchReceipt,readAt:iso(base+5000),reservedFunding:action==='buy'?money(amount(limit)*BigInt(quantity)):'0.000000',reservedElsewhere:'0.000000',priorProof:prior?.proof??null,priorAfter:prior?.after??null,globalRisk:{...scope,complete:true,soleCollector:true,stopped:false,sourceHash:'a'.repeat(64),at:iso(base+4900),unresolvedIntentIds:[b.intentId]}};
}
function verifiedBuy(options){const args=fixture(options);return {proof:verify(args),after:args.after,args};}

test('whole-share mixed prices preserve exact principal and a non-tick rational average',()=>{
 const p=verify(fixture());assert.equal(p.filledQuantity,5);assert.equal(p.fillNotional,'0.515000');assert.equal(p.remainingOwnedCost,'0.515000');
 assert.deepEqual(p.averageFillPrice,{numeratorMicros:'515000',denominatorShares:5,reportedDecimal:'0.10300000000000001',basis:'exact_fill_notional_divided_by_filled_shares',reportedAverageUsedForPrincipal:false});
 assert.equal(p.remainingLots.length,2);assert.equal(p.executionAuthority,false);assert.equal(p.globalActivity.status,'unknown');assert.equal(p.globalCashReconciledExactly,false);assert.ok(Object.isFrozen(p.remainingLots[0]));
});
test('100 share boundary supports exact cost and rejects quantity101 without rounding',()=>{
 const a=fixture({quantity:100,fills:[{quantity:100,price:'0.250000'}],limit:'0.250000'});assert.equal(verify(a).fillNotional,'25.000000');
 a.binding.quantity=101;assert.throws(()=>verify(a),/BINDING/);
});
test('terminal partial BUY retains only actual filled inventory and releases unfilled quantity in proof',()=>{
 const p=verify(fixture({quantity:10,fills:[{quantity:3,price:'0.100000'},{quantity:2,price:'0.105000'}]}));
 assert.equal(p.terminalUnfilledQuantity,5);assert.equal(p.remainingOwnedQuantity,5);assert.equal(p.fillNotional,'0.510000');assert.equal(p.signedPositionDelta,-5);
});
test('documented admission resize and discarded terminal remainder preserve original request funding and actual filled basis',()=>{
 const a=fixture({quantity:10,fills:[{quantity:2,price:'0.100000'},{quantity:1,price:'0.105000'}]});
 edit(a.ack,d=>{d.quantity=5;d.remainingQuantity=2;});edit(a.order,d=>d.quantity=3);edit(a.after.scopedOrdersPages[0],d=>d.data[0].quantity=3);
 const p=verify(a);assert.equal(p.requestedQuantity,10);assert.equal(p.canonicalAcceptedQuantity,5);assert.equal(p.admissionDiscardedQuantity,5);assert.equal(p.terminalReportedQuantity,3);assert.equal(p.terminalReportedUnfilledQuantity,0);assert.equal(p.filledQuantity,3);assert.equal(p.terminalUnfilledQuantity,7);assert.equal(p.remainingOwnedCost,'0.305000');assert.equal(p.funding.reservedFunding,'2.000000');assert.equal(JSON.parse(p.sourceReceipts.dispatch.requestBodyJson).quantity,10);
 a.reservedFunding='1.000000';assert.throws(()=>verify(a),/FUNDING_RESERVATION/);
});
test('canonical quantities cannot grow, vanish, hide fills, or disagree with scoped history',()=>{
 for(const kind of ['ack_growth','ack_zero','terminal_growth','terminal_zero','less_than_filled','history']){
  const a=fixture({quantity:10,fills:[{quantity:3,price:'0.100000'}]});edit(a.ack,d=>{d.quantity=5;d.remainingQuantity=2;});edit(a.order,d=>d.quantity=3);edit(a.after.scopedOrdersPages[0],d=>d.data[0].quantity=3);
  if(kind==='ack_growth')edit(a.ack,d=>{d.quantity=11;d.remainingQuantity=8;});if(kind==='ack_zero')edit(a.ack,d=>d.quantity=0);if(kind==='terminal_growth')edit(a.order,d=>d.quantity=6);if(kind==='terminal_zero')edit(a.order,d=>d.quantity=0);if(kind==='less_than_filled')edit(a.order,d=>d.quantity=2);if(kind==='history')edit(a.after.scopedOrdersPages[0],d=>d.data[0].quantity=4);
  assert.throws(()=>verify(a),kind);
 }
});
test('initial partial open ACK is matched to exact chronological fill prefix, final fills to terminal',()=>{
 const a=fixture({ackFilled:3,ackOpen:true});const p=verify(a);assert.equal(p.ackNotional,'0.305000');assert.equal(p.fillNotional,'0.515000');assert.equal(p.ackCoversFinalFill,false);
 edit(a.ack,d=>{d.totalCost=.300;d.all.fullNotionalCost=.300;d.all.effectiveEntryCost=.300;d.all.netBuyingPowerImpact=.300;});assert.throws(()=>verify(a),/ACK_FILL_NOTIONAL/);
});
test('zero-fill terminal buy has zero basis, no invented average or inventory',()=>{
 const p=verify(fixture({fills:[]}));assert.equal(p.fillNotional,'0.000000');assert.equal(p.remainingLots.length,0);assert.equal(p.averageFillPrice,null);assert.equal(p.terminalUnfilledQuantity,5);
});
test('partial terminal SELL consumes exact FIFO across original differently priced fills',()=>{
 const buy=verifiedBuy();const a=fixture({action:'sell',quantity:4,fills:[{quantity:3,price:'0.110000'}],prior:buy,base:N+10000,orderId:901});const p=verify(a);
 assert.equal(p.filledQuantity,3);assert.equal(p.terminalUnfilledQuantity,1);assert.equal(p.scopedProceeds,'0.330000');assert.equal(p.removedPrincipalCost,'0.305000');assert.equal(p.realizedPnl,'0.025000');
 assert.deepEqual(p.removedLots.map(l=>[l.fillId,l.quantity,l.principalCost]),[[90000,2,'0.200000'],[90001,1,'0.105000']]);assert.equal(p.remainingOwnedQuantity,2);assert.equal(p.remainingOwnedCost,'0.210000');
 assert.equal(p.remainingLots[0].originalQuantity,3);assert.equal(p.remainingLots[0].quantity,2);assert.equal(p.priorProofHash,buy.proof.verificationHash);
 const finish=verify(fixture({action:'sell',quantity:2,fills:[{quantity:2,price:'0.100000'}],limit:'0.100000',prior:{proof:p,after:a.after},base:N+20000,orderId:902}));
 assert.equal(finish.realizedPnl,'-0.010000');assert.equal(finish.cumulativeRealizedPnl,'0.015000');assert.equal(finish.remainingOwnedQuantity,0);assert.equal(finish.remainingOwnedCost,'0.000000');assert.equal(buy.proof.remainingOwnedCost,'0.515000');
});
test('unfilled SELL preserves all old cost, lot identities and zero realized amount',()=>{
 const buy=verifiedBuy(),p=verify(fixture({action:'sell',quantity:5,fills:[],prior:buy,base:N+10000,orderId:901}));assert.deepEqual(p.remainingLots,buy.proof.remainingLots);assert.equal(p.realizedPnl,'0.000000');
});
test('additional verified BUY retains FIFO before new lots and forbids netting or foreign adoption',()=>{
 const buy=verifiedBuy(),a=fixture({quantity:2,fills:[{quantity:2,price:'0.115000'}],prior:buy,base:N+10000,orderId:901}),p=verify(a);assert.equal(p.remainingOwnedQuantity,7);assert.equal(p.remainingOwnedCost,'0.745000');
 edit(a.before.positions,d=>{d.positions.find(x=>x.exchangeId==='700').quantity=-8;});assert.throws(()=>verify(a),/PRIOR_POSITION/);
 const foreign=fixture();edit(foreign.before.positions,d=>{d.positions.push({marketId:'377',exchangeId:'700',quantity:1,settled:false});});assert.throws(()=>verify(foreign),/FLAT_INITIAL/);
});
test('oversell and unknown global intent fail closed',()=>{
 const buy=verifiedBuy();assert.throws(()=>verify(fixture({action:'sell',quantity:6,fills:[],prior:buy,base:N+10000,orderId:901})),/OWNED_QUANTITY/);
 const a=fixture();a.globalRisk.unresolvedIntentIds.push('other-intent');assert.throws(()=>verify(a),/GLOBAL_UNKNOWN/);
});
test('old proof costs, owner and raw after history cannot be substituted',()=>{
 for(const change of [p=>{p.remainingLots[0].entryPrice='0.095000';},p=>{p.ownerId='foreign';},p=>{p.remainingLots.reverse();}]){
  const buy=verifiedBuy(),p=structuredClone(buy.proof);change(p);const {verificationHash,...body}=p;p.verificationHash=payloadHash(body);
  assert.throws(()=>verify(fixture({action:'sell',quantity:1,fills:[{quantity:1,price:'0.110000'}],prior:{proof:p,after:buy.after},base:N+10000,orderId:901})),/PRIOR_|FIFO_/);
 }
 const buy=verifiedBuy(),a=fixture({action:'sell',quantity:1,fills:[],prior:buy,base:N+10000,orderId:901});edit(a.before.scopedOrdersPages[0],d=>{d.data[0].priceLimit=.195;});assert.throws(()=>verify(a),/PRIOR_SOURCE_OR_HISTORY/);
});
test('raw fraction quantities which Number rounds to integers are rejected',()=>{
 for(const which of ['fill','ack','terminal','history','position']){const a=fixture();
  if(which==='fill')lex(a.fillPages[0],'"quantity":-2','"quantity":-2.00000000000000001');
  if(which==='ack')lex(a.ack,'"quantity":5','"quantity":5.00000000000000001');
  if(which==='terminal')lex(a.order,'"quantityFilled":5','"quantityFilled":5.00000000000000001');
  if(which==='history')lex(a.after.scopedOrdersPages[0],'"quantityFilled":5','"quantityFilled":5.00000000000000001');
  if(which==='position')lex(a.after.positions,'"quantity":-5','"quantity":-5.00000000000000001');
  assert.throws(()=>verify(a),/EXACT_INTEGER/);
 }
});
test('weighted average may repeat, but cannot replace actual notional or drift by a micro',()=>{
 const a=fixture({quantity:3,fills:[{quantity:1,price:'0.100000'},{quantity:2,price:'0.105000'}]});assert.equal(verify(a).fillNotional,'0.310000');
 edit(a.fillPages[0],d=>{d.avgFillPrice+=.000001;});assert.throws(()=>verify(a),/AVERAGE_NOTIONAL/);
});
test('only enumerated IEEE tick forms accepted, arbitrary precision tail/offtick rejected',()=>{
 const a=fixture();edit(a.fillPages[0],d=>{d.data[0].price=1-(1-.1);});assert.equal(verify(a).fillNotional,'0.515000');
 const b=fixture();lex(b.fillPages[0],'"price":0.1','"price":0.10000000000000000001');assert.throws(()=>verify(b),/TICK_LEXEME/);
 const c=fixture();edit(c.fillPages[0],d=>{d.data[0].price=.101;});assert.throws(()=>verify(c),/TICK/);
});
test('neutral ALL requires exact cash sign and no collateral effects for each side',()=>{
 const buy=verifiedBuy();for(const action of ['buy','sell'])for(const change of [d=>{d.all.netBuyingPowerImpact*=-1;},d=>{d.all.collateralSavings=.005;},d=>{d.all.relationshipIds=['x'];},d=>{d.all.redemptionCredit=.005;},d=>{delete d.all.netBuyingPowerImpact;}]){
  const a=action==='buy'?fixture():fixture({action,quantity:2,fills:[{quantity:2,price:'0.110000'}],prior:buy,base:N+10000,orderId:901});edit(a.ack,change);assert.throws(()=>verify(a),/ALL/);
 }
 const a=fixture();edit(a.ack,d=>{d.all=null;});assert.equal(verify(a).ackEconomics.kind,'null_all');edit(a.ack,d=>{delete d.all;});assert.throws(()=>verify(a),/ALL_REQUIRED/);
});
test('full original quantity reservation and conservative rounded cash funding are required',()=>{
 const a=fixture({quantity:100,fills:[{quantity:1,price:'0.250000'}],limit:'0.250000'});a.reservedFunding='24.995000';assert.throws(()=>verify(a),/FULL_FUNDING/);
 const b=fixture();edit(b.before.cash,d=>{d.myBalance=1;});assert.throws(()=>verify(b),/FUNDING/);
 const c=fixture();edit(c.after.cash,d=>{d.myBalance=900;});assert.throws(()=>verify(c),/CASH_INTERVAL/);
});
test('actual invocation, exact body/key and owner lease bind original ACK without replay relabeling',()=>{
 for(const change of [a=>{a.dispatchReceipt.fetchInvocationId='other';},a=>{a.dispatchReceipt.requestBodyJson+=' ';},a=>{a.dispatchReceipt.idempotencyKey='other';},a=>{a.dispatchReceipt.leaseHash='0'.repeat(64);},a=>{a.ack.requestStartedAt=iso(N+31001);a.ack.receivedAt=iso(N+31002);a.readAt=iso(N+32000);}]){const a=fixture();change(a);assert.throws(()=>verify(a));}
});
test('canonical mismatch and closed ACK cannot hide later additional fills',()=>{
 for(const change of [d=>{d.quantity=4;},d=>{d.remainingQuantity=1;},d=>{d.side='yes';},d=>{d.exchangeId='701';},d=>{d.fillPrice=.1;}]){const a=fixture();edit(a.ack,change);assert.throws(()=>verify(a),/ACK_|EXACT_INTEGER/);}
 assert.throws(()=>verify(fixture({ackFilled:2,ackOpen:false})),/COMPLETE_FILL_TOTAL/);
});
test('complete two-page fills preserve exact totals and reject duplicates, missing tail and cursor drift',()=>{
 const a=fixture(),d=JSON.parse(a.fillPages[0].bodyText),first={...d,data:[d.data[0]],pagination:{limit:200,hasMore:true,nextCursor:'page 2'}},last={...d,data:[d.data[1]]};
 a.fillPages=[raw('GET','/orders/900/fills?limit=200',first,N+3200),raw('GET','/orders/900/fills?limit=200&cursor=page%202',last,N+3400)];assert.equal(verify(a).filledQuantity,5);
 const dup=structuredClone(a);edit(dup.fillPages[1],v=>{v.data[0].id=d.data[0].id;});assert.throws(()=>verify(dup),/FILL_IDENTITY/);
 const missing=structuredClone(a);missing.fillPages.pop();assert.throws(()=>verify(missing),/FILL_INCOMPLETE/);
 const cursor=structuredClone(a);cursor.fillPages[1].endpoint+='x';assert.throws(()=>verify(cursor),/RECEIPT_SCOPE/);
});
test('timestamps reject fills before authoritative server creation, future fills and stale/future accounts',()=>{
 for(const change of [a=>edit(a.fillPages[0],d=>{d.data[0].filledAt=iso(N+1000);}),a=>edit(a.fillPages[0],d=>{d.data[0].filledAt=iso(N+3400);}),a=>{a.after.positions.receivedAt=iso(N+6000);},a=>{a.before.cash.requestStartedAt=iso(N-20000);},a=>{a.globalRisk.at=iso(N+4100);}]){const a=fixture();change(a);assert.throws(()=>verify(a));}
});
test('incomplete account, unexplained target history and changed external holdings do not reconcile',()=>{
 for(const change of [a=>edit(a.after.positions,d=>{d.pagination={hasMore:true};}),a=>edit(a.after.positions,d=>{delete d.summary;}),a=>edit(a.after.positions,d=>{d.positions[0].quantity=5;}),a=>edit(a.after.scopedOrdersPages[0],d=>{d.data.push({...d.data[0],id:901});}),a=>edit(a.after.scopedOrdersPages[0],d=>{delete d.data[0].quantityFilled;})]){const a=fixture();change(a);assert.throws(()=>verify(a));}
});
test('proof does not turn a caller activity flag into exact global accounting or authority',()=>{
 const a=fixture();a.accountActivity={complete:true,events:[]};assert.throws(()=>verify(a),/GLOBAL_ACTIVITY_UNSUPPORTED/);
 const p=verify(fixture());assert.equal(p.newSubmissionAuthorized,false);assert.equal(p.authoritativeReconciliation,false);assert.equal(p.globalAccountActivityVerified,false);
});
test('cross-account projection rollback and reused/missing receipt invocation fail closed',()=>{
 const rollback=fixture();edit(rollback.after.scopedOrdersPages[0],d=>{d.coverage.projectedThroughSequence=99;});assert.throws(()=>verify(rollback),/PROJECTION_ROLLBACK/);
 const reused=fixture();reused.after.cash.fetchInvocationId=reused.before.cash.fetchInvocationId;assert.throws(()=>verify(reused),/REUSED_RECEIPT_INVOCATION/);
 const missing=fixture();delete missing.order.fetchInvocationId;assert.throws(()=>verify(missing),/RECEIPT_INVOCATION/);
});
test('dispatch metadata does not accept or echo arbitrary header/account fields',()=>{
 const a=fixture();a.dispatchReceipt.headers={authorization:'synthetic-test-value'};assert.throws(()=>verify(a),/UNEXPECTED_METADATA/);
 const b=fixture();b.dispatchReceipt.leaseIdentity.arbitrary='untrusted';b.dispatchReceipt.leaseHash=payloadHash(b.dispatchReceipt.leaseIdentity);assert.throws(()=>verify(b),/UNEXPECTED_METADATA/);
});
test('covered SELL requires inventory rather than spare cash or a new BUY reservation',()=>{
 const buy=verifiedBuy(),a=fixture({action:'sell',quantity:2,fills:[{quantity:2,price:'0.110000'}],prior:buy,base:N+10000,orderId:901});
 edit(a.before.cash,d=>{d.myBalance=0;});edit(a.after.cash,d=>{d.myBalance=.22;});a.reservedElsewhere='2.000000';
 const p=verify(a);assert.equal(p.scopedProceeds,'0.220000');assert.equal(p.funding.fullyFundedAtSubmission,null);assert.equal(p.funding.inventoryCoveredAtSubmission,true);
});
test('100 shares is an order bound, not a hidden aggregate inventory policy',()=>{
 const buy=verifiedBuy({quantity:100,fills:[{quantity:100,price:'0.250000'}],limit:'0.250000'});
 const second=verify(fixture({quantity:100,fills:[{quantity:100,price:'0.245000'}],limit:'0.250000',prior:buy,base:N+10000,orderId:901}));
 assert.equal(second.requestedQuantity,100);assert.equal(second.remainingOwnedQuantity,200);assert.equal(second.remainingOwnedCost,'49.500000');assert.equal(second.executionAuthority,false);
});
test('unspecified POST fillPrice is classified as exact-prefix last or weighted mean, never cost basis',()=>{
 const a=fixture();edit(a.ack,d=>{d.fillPrice=.103;});const p=verify(a);assert.equal(p.fillNotional,'0.515000');assert.deepEqual(p.ackFillPriceSummary.matches,['ack_prefix_weighted_average']);assert.equal(p.ackFillPriceSummary.principalDerivedFromSummary,false);
 const b=fixture({ackFilled:3,ackOpen:true});edit(b.ack,d=>{d.fillPrice=.305/3;});assert.deepEqual(verify(b).ackFillPriceSummary.matches,['ack_prefix_weighted_average']);
 edit(a.ack,d=>{d.fillPrice=.104;});assert.throws(()=>verify(a),/ACK_FILL_PRICE_SUMMARY/);
});
test('previous proof must already be known before continuation account starts',()=>{
 const buy=verifiedBuy(),a=fixture({quantity:1,fills:[{quantity:1,price:'0.200000'}],prior:buy,base:N+4500,orderId:901});
 assert.throws(()=>verify(a),/PRIOR_SOURCE_OR_HISTORY/);
});
test('server clock allowance cannot put a later BUY before carried FIFO source order',()=>{
 const first=fixture({quantity:1,fills:[{quantity:1,price:'0.100000'}],ackFilled:0,ackOpen:true});
 const od=JSON.parse(first.order.bodyText),fd=JSON.parse(first.fillPages[0].bodyText);fd.data[0].filledAt=iso(N+5000);
 first.order=raw('GET','/orders/900',od,N+5050);first.fillPages=[raw('GET','/orders/900/fills?limit=200',fd,N+5150)];
 first.after=account(N+5250,999.9,-1,[od]);first.readAt=iso(N+5700);first.globalRisk.at=iso(N+5650);
 const buy={proof:verify(first),after:first.after};
 const next=fixture({quantity:1,fills:[{quantity:1,price:'0.200000'}],prior:buy,base:N+5700,orderId:901});
 edit(next.order,d=>{d.createdAt=iso(N+4700);});edit(next.after.scopedOrdersPages[0],d=>{d.data.at(-1).createdAt=iso(N+4700);});
 edit(next.fillPages[0],d=>{d.data[0].filledAt=iso(N+4900);});
 assert.throws(()=>verify(next),/CONTINUATION_FILL_CLOCK/);assert.equal(buy.proof.remainingLots[0].fillId,90000);
});
