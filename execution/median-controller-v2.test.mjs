import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {createHash} from 'node:crypto';
import {MedianCompetitionController,MEDIAN_EXECUTION_POLICY,MEDIAN_PAPER_POLICY,medianControllerIdentity} from './median-controller.mjs';
import {MedianCompetitionControllerV2,MEDIAN_EXECUTION_POLICY_V2,medianControllerV2Identity,medianEntryCapLimit,validateMedianRevision} from './median-controller-v2.mjs';
import {digest} from '../live-paper/model.mjs';
import {payloadHash} from './core.mjs';
import {validateUniverse} from '../live-paper/breadth.mjs';

const TID='bda92870-621e-47b0-bc3c-3602c5c26f55',iso=n=>new Date(n).toISOString(),sha=x=>createHash('sha256').update(x).digest('hex');
function activationAndHandoff(now,bankHash){
 const thread='01a0f8cc-b9bb-77d2-9f4c-2844396f1b79',dot='01a0f8cb-e876-739f-b149-0d446a5be0c3';
 const prior={schemaVersion:2,enabled:true,ownerId:'Codex',tournamentId:TID,runId:'sig-pilot-prior-fixture',strategyId:'settlement-value-native-v1',authorityKind:'direct_human_assignment',handoffId:'human-to-baal-owner',assignmentCommentId:5984685670,ackCommentId:5984845412,handoffHash:'a'.repeat(64),strategyBankHash:'b'.repeat(64),feeBps:0,depthFractionBps:100,maxManagedMarkets:3,maxOrderCost:'1.000000',maxAggregateCost:'10.000000',maxCumulativeBuyCost:'25.000000',cashReserve:'90.000000',sizingCapital:'100.000000',startedAt:'2026-10-04T22:00:00.000Z',entryExpiresAt:'2026-10-05T03:59:22.131Z'};
 const old={schemaVersion:2,handoffId:prior.handoffId,authorityKind:prior.authorityKind,assignmentCommentId:prior.assignmentCommentId,ackCommentId:prior.ackCommentId,formerWriter:'Daenerys',executionOwner:'Codex',writerStopped:true,inFlightResolved:true,formerReadsPerMinute:0,formerWritesPerMinute:0,humanAuthority:{kind:'direct_human_message',commentId:5984685670,threadId:thread,quote:'Fixture human authority',recordedAt:'2026-10-04T21:58:00.000Z'},writerStopEvidence:{commentId:5984600000,cupWriterReportedStopped:true,cupInFlightReportedClear:true,reportedAt:'2026-10-04T21:00:00.000Z',checkedAt:'2026-10-04T21:59:00.000Z',threadId:dot,threadStatus:'active'},reconciliation:{complete:true,tournamentId:TID,receiptHash:'c'.repeat(64),oldestReceivedAt:'2026-10-04T21:59:00.000Z',newestReceivedAt:'2026-10-04T21:59:10.000Z'},assignedAt:'2026-10-04T21:58:00.000Z',reconciledAt:'2026-10-04T21:59:20.000Z',acknowledgedAt:'2026-10-04T21:59:30.000Z'};prior.handoffHash=payloadHash(old);
 const a={...prior,runId:'sig-pilot-median-fixture',strategyId:'historical-median-receipt-v1',strategyBankHash:bankHash,startedAt:iso(now-1000),entryExpiresAt:iso(now+3600000),activationAuthority:{kind:'direct_human_activation',threadId:thread,runId:'sig-pilot-median-fixture',exploratory:true,profitabilityEstablished:false,quote:'Fixture small median pilot',recordedAt:iso(now-10000)}};
 const h={schemaVersion:3,kind:'same_owner_explicit_human_activation',executionOwner:'Codex',ownershipTransferred:false,handoffId:a.handoffId,authorityKind:a.authorityKind,assignmentCommentId:a.assignmentCommentId,ackCommentId:a.ackCommentId,inheritedOwnershipRecord:old,inheritedActivation:prior,humanActivation:a.activationAuthority,reconciliation:{complete:true,tournamentId:TID,receiptHash:'d'.repeat(64),oldestReceivedAt:iso(now-8000),newestReceivedAt:iso(now-6000)},reconciledAt:iso(now-4000),acknowledgedAt:iso(now-2000)};a.handoffHash=payloadHash(h);return{activation:a,handoff:h};
}
function fixture(t,options={}){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'median-official-'));let now=Date.parse('2026-10-06T06:00:00.000Z'),c,sequence=0;
 const members=options.markets||[{marketId:'377',exchangeId:'700',option:'YES'}],universe=validateUniverse({tournamentId:TID,exchanges:members},TID),defaultMarket=members[0].marketId;
 let previous='',text='';for(let i=0;i<21;i++){const at=now-2100000+i*100000,quotes=members.map(m=>{const bid=options.featureBid?.(i,m)??.35;return{...m,latestPrice:bid,bestBid:bid,bestAsk:(Math.round(bid*200)+1)/200,spread:.005,crossed:false};}),body={sequence:i+1,previous,at:iso(at),event:{type:'market_price_observation',snapshot:{tournamentId:TID,universeHash:universe.hash,requestStartedAt:iso(at-100),receivedAt:iso(at),sourceTimestamp:null,depthAvailable:false,executable:false,quotes,missingIds:[]}},state:null};const row={...body,hash:digest(body)};previous=row.hash;text+=JSON.stringify(row)+'\n';}
 fs.writeFileSync(path.join(dir,'market-prices.jsonl'),text);
 const bank={schemaVersion:1,mode:'baal_historical_median_production_v1',tournamentId:TID,universeHash:universe.hash,cutoff:iso(now-1000),paperBinding:{bankHash:'e'.repeat(64),policyHash:digest(MEDIAN_PAPER_POLICY)},sourcePrefix:{bytes:Buffer.byteLength(text),sha256:sha(text),sequence:21,hash:previous},policy:MEDIAN_EXECUTION_POLICY,codeIdentity:medianControllerIdentity()};
 const {activation,handoff}=activationAndHandoff(now,digest(bank));
 fs.mkdirSync(path.join(dir,'production-banks'));fs.writeFileSync(path.join(dir,'production-banks',digest(bank)+'.json'),JSON.stringify(bank));fs.writeFileSync(path.join(dir,'competition-activation.json'),JSON.stringify(activation));fs.writeFileSync(path.join(dir,'production-handoff.json'),JSON.stringify(handoff));
 const server={quantities:Object.fromEntries(members.map((m,i)=>[m.exchangeId,i===0&&options.external?1:0])),cash:100,posts:[],orders:new Map(),keys:new Map(),requests:[],ambiguous:options.ambiguous===true,fill:options.fill!==false,fillPrice:options.fillPrice,beforeFence:null,afterFence:null,cancelUnknown:false,stickyExpired:false,cancels:0};
 const response=(endpoint,data)=>({status:200,endpoint,requestStartedAt:iso(now-1),receivedAt:iso(now),data}),page=data=>({data,pagination:{limit:200,hasMore:false,nextCursor:null}});
 const order=o=>({id:o.id,exchangeId:o.body.exchangeId,tournamentId:TID,side:'yes',action:o.body.action,quantity:o.open?o.body.quantity-o.filled:o.body.quantity,priceLimit:o.body.price,open:o.open,createdAt:o.createdAt,expirationDate:o.body.expirationDate});
 const open=()=>{if(!server.stickyExpired)for(const o of server.orders.values())if(now>=Date.parse(o.body.expirationDate))o.open=false;return [...server.orders.values()].filter(o=>o.open).map(order);};
 const positions=()=>response('/tournaments/midterm-elections/portfolio/positions',{positions:members.filter(m=>server.quantities[m.exchangeId]).map(m=>({marketId:m.marketId,exchangeId:m.exchangeId,quantity:server.quantities[m.exchangeId],lots:[]})),summary:{totalMarketValue:0,totalCostBasis:0,totalUnrealizedPnl:0}});
 const book=({marketId=defaultMarket,bid=.335,ask=.340,depth=1000,seq,sourceAt,requestStartedAt,exchangeId=members.find(m=>m.marketId===marketId)?.exchangeId,asks}={})=>{now+=20;sequence++;return{...response('/markets/'+marketId+'/orderbook?tournamentId='+TID+'&depth=20',{contexts:[{type:'tournament',tournament:{id:TID},orderbook:{exchanges:[{exchangeId,option:'YES',asOf:{sequence:seq??sequence,at:sourceAt??iso(now-2)},bids:[{price:bid,quantity:depth}],asks:asks||[{price:ask,quantity:depth}]}]}}]}),...(requestStartedAt?{requestStartedAt}:{})};};
 const fill=o=>{if(o.filled)return;o.filled=1;o.open=false;const price=server.fillPrice??o.body.price;o.fills=[{id:2000+o.id,price,quantity:1,side:'yes',filledAt:iso(now)}];server.quantities[o.body.exchangeId]+=o.body.action==='buy'?1:-1;server.cash+=(o.body.action==='buy'?-1:1)*price;};
 const api={async dispatch(spec){now+=20;server.requests.push({method:spec.method,endpoint:spec.endpoint});
  if(spec.method==='POST'){
   server.beforeFence?.();await spec.beforeDispatch();server.afterFence?.();await spec.beforeNetwork();server.posts.push(structuredClone(spec.body));
   const old=server.keys.get(spec.body.idempotencyKey);if(old){assert.equal(old.bodyJson,spec.bodyJson);return response(spec.endpoint,old.result);}
   const o={id:1001+server.orders.size,body:structuredClone(spec.body),createdAt:iso(now),filled:0,open:true,fills:[]};server.orders.set(o.id,o);if(server.fill)fill(o);
   const result={orderId:o.id,exchangeId:o.body.exchangeId,open:o.open,remainingQuantity:1-o.filled,action:o.body.action,side:'yes',price:o.body.price,quantity:1,quantityTraded:o.filled,totalCost:o.filled*(o.fills[0]?.price||0),fillPrice:o.fills[0]?.price??null,all:null};server.keys.set(spec.body.idempotencyKey,{bodyJson:spec.bodyJson,result});
   if(server.ambiguous){server.ambiguous=false;const e=Error('Synthetic lost response');e.status=502;e.apiCode='ORDER_STATUS_UNKNOWN';throw e;}return response(spec.endpoint,result);
  }
  if(spec.method==='DELETE'){await spec.beforeDispatch();await spec.beforeNetwork();server.cancels++;const id=Number(spec.endpoint.split('/').at(-1)),o=server.orders.get(id);if(server.cancelUnknown){const e=Error('Synthetic unknown cancel');e.status=503;throw e;}o.open=false;return response(spec.endpoint,{orderId:id,tournamentId:TID});}
  assert.equal(spec.method,'GET');if(spec.endpoint==='/tournaments/midterm-elections')return response(spec.endpoint,{id:TID,slug:'midterm-elections',status:'active',myBalance:server.cash,isPendingEnrolment:false});
  if(spec.endpoint.endsWith('/portfolio/positions'))return positions();if(spec.endpoint.startsWith('/orders?'))return response(spec.endpoint,page(open()));const market=/^\/markets\/(\d+)/.exec(spec.endpoint);if(market){const item=members.find(m=>m.marketId===market[1]);assert.ok(item);if(spec.endpoint.includes('/orderbook?'))return book({marketId:item.marketId});return response(spec.endpoint,{id:item.marketId,title:'Synthetic median fixture',status:'open',isComposite:false,isMultiOutcome:false,exchanges:[{id:item.exchangeId,option:'YES'}],contexts:[{type:'tournament',tournament:{id:TID},status:'open',settledWith:null}]});}
  const m=/^\/orders\/(\d+)(\/fills)?/.exec(spec.endpoint);if(m){open();const o=server.orders.get(Number(m[1]));assert.ok(o);return response(spec.endpoint,m[2]?{...page(o.fills),orderId:o.id,exchangeId:o.body.exchangeId,tournamentId:TID,totalQuantityFilled:o.filled,avgFillPrice:o.fills[0]?.price??null,coverage:{complete:true,projectedThroughSequence:3000+o.id}}:order(o));}throw Error('Unexpected synthetic endpoint');
 }};
 let revision,newBank;
 const controllerOptions=()=>({runtimeDir:dir,api,universe,config:{},isStopped:()=>false,activation,now:()=>now});
 const start=()=>c=revision?new MedianCompetitionControllerV2(controllerOptions()):new MedianCompetitionController(controllerOptions());start();
 const migrate=(mutate=()=>{})=>{
  const coreCheckpoint={sequence:c.journal.sequence,hash:c.journal.previous,stateHash:payloadHash(c.state)},proposalRaw=fs.existsSync(c.proposalJournal.file)?fs.readFileSync(c.proposalJournal.file):Buffer.alloc(0),proposalCheckpoint={sequence:c.proposalJournal.sequence,hash:c.proposalJournal.previous,stateHash:payloadHash(c.proposals),bytes:proposalRaw.length,sha256:sha(proposalRaw)};
  c.close();
  const archive=path.join(dir,'production-code',activation.strategyBankHash);for(const name of Object.keys(bank.codeIdentity)){const dest=path.join(archive,name);fs.mkdirSync(path.dirname(dest),{recursive:true});fs.copyFileSync(path.resolve(name),dest);}
  newBank={...bank,schemaVersion:2,mode:'baal_historical_median_production_v2',policy:MEDIAN_EXECUTION_POLICY_V2,codeIdentity:medianControllerV2Identity(),legacyCompatibility:{priorBankHash:activation.strategyBankHash,priorPolicyHash:payloadHash(bank.policy),priorCodeIdentityHash:payloadHash(bank.codeIdentity)}};
  revision={schemaVersion:1,kind:'same_run_median_entry_cap_revision',revisionId:'median-entry-cap-fixture',ownerId:'Codex',runId:activation.runId,tournamentId:TID,activationHash:payloadHash(activation),priorBankHash:activation.strategyBankHash,nextBankHash:digest(newBank),recordedAt:iso(now),coreCheckpoint,proposalCheckpoint};
  mutate({revision,bank:newBank,archive});
  fs.writeFileSync(path.join(dir,'production-banks',revision.nextBankHash+'.json'),JSON.stringify(newBank));fs.writeFileSync(path.join(dir,'competition-runs',activation.runId,'median-entry-cap-revision.json'),JSON.stringify(revision));start();
 };
 if(!options.legacy)migrate();
 const before=(ids=[defaultMarket],extra={})=>c.beforeBooks({positions:positions(),orders:open(),ordersAt:iso(now),candidateMarketIds:ids,...extra});
 const cycle=async b=>{const id=b?.marketId||defaultMarket;await before([id]);await c.beforeBook(id);await c.onBook(book(b));};
 t.after(()=>{c?.close();fs.rmSync(dir,{recursive:true,force:true});});
 return{get c(){return c;},get now(){return now;},get revision(){return revision;},get newBank(){return newBank;},dir,bank,activation,universe,server,book,before,cycle,fill,migrate,advance:n=>now+=n,restart(){c.close();start();},proposals:()=>Object.values(c.proposals.proposals)};
}
test('durable causal proposal then later book dispatches one fixed-limit share and official fills alone establish owned cost',async t=>{
 const f=fixture(t,{fillPrice:.340});await f.cycle();assert.equal(f.server.posts.length,0);const p=f.proposals()[0];assert.equal(p.limit,'0.345000');assert.equal(p.proof.reference.sourceProof.retainedSamples,20);assert.equal(f.c.proposalJournal.sequence,3);
 f.advance(3100);await f.cycle();assert.equal(f.server.posts.length,1);assert.equal(f.server.posts[0].quantity,1);assert.equal(f.server.posts[0].price,.345);assert.equal(f.server.posts[0].idempotencyKey,p.idempotencyKey);assert.deepEqual(f.c.state.owned,{});assert.equal(Date.parse(f.server.posts[0].expirationDate)-f.now,29980);
 await f.before();assert.equal(f.c.state.owned['700'].cost,'0.340000');assert.equal(f.c.state.owned['700'].quantity,1);
});
test('same source version, premature request, thin individual levels and unfavorable price cannot execute proposal',async t=>{
 for(const kind of ['same','early','thin','price']){const f=fixture(t);await f.cycle();const p=f.proposals()[0];f.advance(kind==='early'?1000:3100);await f.before();const opts=kind==='same'?{seq:Number(p.originalBook.sequence),sourceAt:p.originalBook.sourceAt}:kind==='thin'?{asks:[{price:.340,quantity:60},{price:.345,quantity:60}]}:kind==='price'?{ask:.350}:{};await f.c.onBook(f.book(opts));assert.equal(f.server.posts.length,0,kind);}
});
test('restart preserves proposal UUID, fixed limit and original three-second latency',async t=>{
 const f=fixture(t);await f.cycle();const p=f.proposals()[0];f.restart();f.advance(3100);await f.cycle();assert.equal(f.server.posts.length,1);assert.equal(f.server.posts[0].idempotencyKey,p.idempotencyKey);assert.equal(f.server.posts[0].price,.345);
});
test('unknown acceptance blocks replacements and recovered identical expired key accounts fill exactly once',async t=>{
 const f=fixture(t,{ambiguous:true});await f.cycle();f.advance(3100);await f.cycle();assert.equal(f.c.active().status,'uncertain');const body=structuredClone(f.server.posts[0]);assert.deepEqual(f.c.state.owned,{});f.advance(31000);f.restart();await f.before();assert.equal(f.server.posts.length,2);assert.deepEqual(f.server.posts[1],body);await f.before();assert.equal(f.c.state.owned['700'].quantity,1);await f.before();assert.equal(f.c.state.owned['700'].quantity,1);
});
test('account holdings never become owned lots, proposals or covered sells',async t=>{const f=fixture(t,{external:true});await f.cycle();f.advance(3100);await f.cycle();assert.equal(f.server.posts.length,0);assert.deepEqual(f.c.state.owned,{});assert.equal(f.proposals().length,0);});
test('wrong exchange and same-sequence changed depth fail closed',async t=>{
 const f=fixture(t);await f.cycle();const p=f.proposals()[0];f.advance(3100);await assert.rejects(()=>f.c.onBook(f.book({exchangeId:'701'})),/IDENTITY/);await assert.rejects(()=>f.c.onBook(f.book({seq:Number(p.originalBook.sequence),sourceAt:p.originalBook.sourceAt,bid:.330})),/BOOK_SEQUENCE_CONFLICT/);assert.equal(f.server.posts.length,0);
});
test('same sequence with later authoritative asOf is a newer version, while exact-version depth conflict remains blocked',async t=>{
 for(const changedDepth of [false,true]){const f=fixture(t);await f.cycle();const p=f.proposals()[0];f.advance(3100);await f.before();await f.c.onBook(f.book({seq:Number(p.originalBook.sequence),...(changedDepth?{bid:.330,depth:900}:{})}));assert.equal(f.server.posts.length,1);assert.equal(f.server.posts[0].idempotencyKey,p.idempotencyKey);assert.equal(f.server.posts[0].price,.345);}
});
test('same sequence with earlier asOf is rollback even on a newer receipt',async t=>{const f=fixture(t);await f.cycle();const p=f.proposals()[0];f.advance(3100);await assert.rejects(()=>f.c.onBook(f.book({seq:Number(p.originalBook.sequence),sourceAt:iso(Date.parse(p.originalBook.sourceAt)-1)})),/BOOK_ROLLBACK/);assert.equal(f.server.posts.length,0);});
test('both fences enforce quote/account freshness and entry window',async t=>{
 for(const phase of ['beforeFence','afterFence'])for(const mode of ['stale','expiry']){const f=fixture(t);await f.cycle();f.advance(3100);f.server[phase]=()=>f.advance(mode==='stale'?21000:3600000);await f.cycle();assert.equal(f.server.posts.length,0,phase+mode);assert.equal(f.c.active()?.status,phase==='afterFence'?'uncertain':undefined);}
});
test('account read after original decision book is rejected, including apparently fresh evidence',async t=>{const f=fixture(t);await f.before();const response=f.book();f.c.account.orders.at=iso(f.now+1);await f.c.onBook(response);assert.equal(f.proposals().length,0);assert.equal(f.c.reason,'MEDIAN_STALE_OR_NONCAUSAL_ACCOUNT');});
test('profit sale uses actual own fill basis and later depth, records exact positive realized result',async t=>{
 const f=fixture(t,{fillPrice:.340});await f.cycle();f.advance(3100);await f.cycle();await f.before();f.server.fillPrice=undefined;await f.c.onBook(f.book({bid:.345,ask:.350}));assert.equal(f.proposals().at(-1).reason,'profit_target');f.advance(3100);await f.cycle({bid:.345,ask:.350});await f.before();assert.deepEqual(f.c.state.owned,{});const report=JSON.parse(fs.readFileSync(path.join(f.dir,'competition-stats.json')));assert.equal(report.realizedPnl,'0.005000');f.advance(3100);await f.cycle();assert.equal(f.server.posts.length,2,'cooldown cannot immediately rebuy');
});
test('stop replaces unsubmitted target, persists through rebound/restart and exits even when entry window closed',async t=>{
 const f=fixture(t,{fillPrice:.340});await f.cycle();f.advance(3100);await f.cycle();await f.before();await f.c.onBook(f.book({bid:.345,ask:.350}));const target=f.proposals().at(-1);f.advance(1000);await f.cycle({bid:.315,ask:.320});assert.equal(f.c.proposals.proposals[target.intentId].status,'revoked');assert.equal(f.c.state.control.exits['700'].reason,'stop_loss');f.restart();f.advance(3600000);await f.cycle({bid:.330,ask:.335});f.server.fillPrice=undefined;f.advance(3100);await f.cycle({bid:.330,ask:.335});await f.before();assert.deepEqual(f.c.state.owned,{});assert.deepEqual(f.c.state.control.exits,{});
});
test('cancel ambiguity never clears owned accounting or permits replacement before official closed reconciliation',async t=>{
 const f=fixture(t,{fill:false});await f.cycle();f.advance(3100);await f.cycle();assert.equal(f.c.active().status,'acknowledged');f.server.stickyExpired=true;f.server.cancelUnknown=true;f.advance(31000);await f.before();assert.equal(f.server.cancels,1);assert.equal(f.c.state.control.cancellations[f.c.active().intentId].phase,'uncertain');await f.c.onBook(f.book());assert.equal(f.server.posts.length,1);const o=[...f.server.orders.values()][0];f.fill(o);await f.before();assert.equal(f.c.state.owned['700'].quantity,1);assert.equal(f.c.active(),undefined);assert.equal(f.server.posts.length,1);
});
test('changed frozen bank prevents new dispatch and leaves proposal journal intact',async t=>{const f=fixture(t);await f.cycle();const before=fs.readFileSync(f.c.proposalJournal.file);fs.writeFileSync(path.join(f.dir,'production-banks',f.activation.strategyBankHash+'.json'),JSON.stringify({...f.bank,policy:{...f.bank.policy,dipTicks:0}}));f.advance(3100);await assert.rejects(()=>f.c.onBook(f.book()),/BANK/);assert.deepEqual(fs.readFileSync(f.c.proposalJournal.file),before);assert.equal(f.server.posts.length,0);});
test('auxiliary candidate path refreshes account before original read using existing scoped transport',async t=>{const f=fixture(t);await f.c.initializeMedianReference();assert.equal(await f.c.beforeCandidateRead('377'),true);await f.c.onBook(f.book());assert.equal(f.proposals().length,1);f.advance(25000);assert.equal(await f.c.beforeCandidateRead('377'),true);assert(f.now-Date.parse(f.c.account.positions.at)<1000);await assert.rejects(()=>f.c.beforeCandidateRead('999'),/CANDIDATE_SCOPE/);});
test('STOP at either dispatch fence prevents network writes and restart preserves conservative core outcome',async t=>{
 for(const phase of ['beforeFence','afterFence']){const f=fixture(t);await f.cycle();f.advance(3100);f.server[phase]=()=>fs.writeFileSync(path.join(f.dir,'EXECUTION_STOP'),'fixture');await f.cycle();assert.equal(f.server.posts.length,0);assert.equal(f.c.active()?.status,phase==='afterFence'?'uncertain':undefined);fs.unlinkSync(path.join(f.dir,'EXECUTION_STOP'));f.server[phase]=null;f.restart();assert.equal(f.c.active()?.status,phase==='afterFence'?'uncertain':undefined);}
});
test('proposal expiration releases reservation without a new request or fictitious order',async t=>{const f=fixture(t);await f.cycle();const p=f.proposals()[0];f.advance(120001);await f.before();assert.equal(f.c.proposals.proposals[p.intentId].status,'expired');assert.equal(f.server.posts.length,0);assert.deepEqual(f.c.state.intents,{});});
test('mutating the captured followup after durable admission is blocked at final network fence',async t=>{const f=fixture(t);await f.cycle();f.advance(3100);f.server.afterFence=()=>{f.c.currentPermit.referenceProof.followupBook.asks[0].quantity=99;};await f.cycle();assert.equal(f.server.posts.length,0);assert.equal(f.c.active().status,'uncertain');});
test('reference-chain failure remains latched instead of silently re-bootstrapping and accepting a pending buy',async t=>{const f=fixture(t);await f.cycle();const file=path.join(f.dir,'market-prices.jsonl'),original=fs.readFileSync(file),changed=Buffer.from(original);changed[2]^=1;fs.writeFileSync(file,changed);f.advance(3100);await f.cycle();assert.equal(f.c.referenceReady,false);assert.match(f.c.referenceFailure,/CONSUMED_PREFIX_CHANGED/);fs.writeFileSync(file,original);await f.cycle();assert.equal(f.server.posts.length,0);assert.equal(f.c.referenceReady,false);await assert.rejects(()=>f.c.initializeMedianReference(),/REFERENCE_FAILED_RESTART_REQUIRED/);});
test('auxiliary decision consumes newly published bulk point before its original request, changing the exact excluded-newest median',async t=>{
 const f=fixture(t,{featureBid:i=>i<10||i===20?.34:.35});await f.before();
 const query={marketId:'377',exchangeId:'700',bookRequestStartedAt:iso(f.now),availableAt:iso(f.now),bestAsk:'0.340000'};
 assert.equal(f.c.reference.reference(query).reference,'0.350000');
 const at=iso(f.now),body={sequence:22,previous:f.bank.sourcePrefix.hash,at,event:{type:'market_price_observation',snapshot:{tournamentId:TID,universeHash:f.bank.universeHash,requestStartedAt:iso(f.now-1),receivedAt:at,sourceTimestamp:null,depthAvailable:false,executable:false,quotes:[{marketId:'377',exchangeId:'700',option:'YES',latestPrice:.35,bestBid:.35,bestAsk:.355,spread:.005,crossed:false}],missingIds:[]}},state:null};
 fs.appendFileSync(path.join(f.dir,'market-prices.jsonl'),JSON.stringify({...body,hash:digest(body)})+'\n');f.advance(10);
 assert.equal(await f.c.beforeCandidateRead('377'),true);await f.c.onBook(f.book());
 assert.equal(f.c.reference.status().sequence,22);assert.equal(f.c.lastReference.sourceProof.excludedNewest.sequence,22);assert.equal(f.c.lastReference.reference,'0.340000');assert.equal(f.c.lastReference.reason,'insufficient_one_tick_gap');assert.equal(f.proposals().length,0);assert.equal(f.server.posts.length,0);
});

test('new one-tick cap is exact, frozen at creation and requires later marketable depth',async t=>{
 assert.equal(medianEntryCapLimit({asks:[{price:'0.495000'}]},{outcome:'reference',reference:'0.500000'}),'0.495000');
 assert.equal(medianEntryCapLimit({asks:[{price:'0.490000'}]},{outcome:'reference',reference:'0.500000'}),'0.495000');
 const f=fixture(t,{featureBid:()=>.5});await f.cycle({bid:.490,ask:.495});const p=f.proposals().at(-1);
 assert.equal(p.limit,'0.495000');assert.equal(p.bankHash,f.revision.nextBankHash);assert.equal(p.proof.reference.reference,'0.500000');
 f.advance(3100);await f.cycle({bid:.495,ask:.500});assert.equal(f.server.posts.length,0,'followup at old protective .500 must not fill');
 f.restart();f.advance(3100);await f.cycle({bid:.490,ask:.495,depth:99});assert.equal(f.server.posts.length,0,'at least 100 shares on one level');
 f.advance(3100);await f.cycle({bid:.490,ask:.495});assert.equal(f.server.posts.length,1);assert.equal(f.server.posts[0].price,.495);assert.equal(f.server.posts[0].idempotencyKey,p.idempotencyKey);
 await f.before();assert.equal(f.c.state.owned['700'].cost,'0.495000');assert.equal(f.c.state.intents[p.intentId].attribution.modelHash,f.revision.nextBankHash);
});

test('both fences reject cap-proof mutation without replacing the committed key',async t=>{
 for(const phase of ['beforeFence','afterFence']){const f=fixture(t,{featureBid:()=>.5});await f.cycle({bid:.490,ask:.495});const p=f.proposals().at(-1);f.advance(3100);
  f.server[phase]=()=>{const q=f.c.proposals.proposals[p.intentId];q.proof.reference.reference='0.495000';q.proofHash=payloadHash(q.proof);f.c.currentPermit.referenceProof.proposalHash=q.proofHash;};
  if(phase==='beforeFence')await assert.rejects(()=>f.cycle({bid:.490,ask:.495}),/ENTRY_REFERENCE_GAP/);else await f.cycle({bid:.490,ask:.495});assert.equal(f.server.posts.length,0,phase);assert.equal(f.c.active()?.status,phase==='afterFence'?'uncertain':undefined);
 }
});

test('two actual-shaped legacy fills keep bank attribution, basis, clocks and activation across V2 restart and new-bank sale',async t=>{
 const markets=[{marketId:'388',exchangeId:'1077',option:'YES'},{marketId:'306',exchangeId:'995',option:'YES'},{marketId:'377',exchangeId:'700',option:'YES'}];
 const f=fixture(t,{legacy:true,markets,featureBid:(_i,m)=>m.marketId==='388'?.175:m.marketId==='306'?.165:.5});
 for(const [marketId,bid,ask]of [['388',.160,.165],['306',.150,.155]]){await f.cycle({marketId,bid,ask});f.advance(3100);await f.cycle({marketId,bid,ask});await f.before([marketId]);}
 const originalState=structuredClone(f.c.state),originalProposals=structuredClone(f.c.proposals),oldProposalBytes=fs.readFileSync(f.c.proposalJournal.file),originalActivation=fs.readFileSync(path.join(f.dir,'competition-activation.json'));
 assert.deepEqual(Object.fromEntries(Object.entries(originalState.owned).map(([id,p])=>[id,p.cost])),{'1077':'0.170000','995':'0.160000'});
 f.migrate();f.restart();assert.deepEqual(f.c.state,originalState);assert.deepEqual(f.c.proposals.proposals,originalProposals.proposals);assert.deepEqual(fs.readFileSync(path.join(f.c.dir,'median-proposals.jsonl')),oldProposalBytes);assert.deepEqual(fs.readFileSync(path.join(f.dir,'competition-activation.json')),originalActivation);
 assert.equal(f.c.activation.entryExpiresAt,f.activation.entryExpiresAt);assert.equal(f.c.activation.maxCumulativeBuyCost,'25.000000');assert.equal(f.c.ownedEntry('1077').modelHash,f.activation.strategyBankHash);
 assert.equal(JSON.parse(fs.readFileSync(path.join(f.dir,'competition-stats.json'))).realizedPnl,'0.000000');
 f.c.referenceFailure='FIXTURE_REFERENCE_UNAVAILABLE';f.c.referenceReady=false;
 await f.cycle({marketId:'388',bid:.175,ask:.180});const sale=f.proposals().at(-1);assert.equal(sale.action,'sell');assert.equal(sale.bankHash,f.revision.nextBankHash);assert.equal(sale.proof.ownedEntry.modelHash,f.activation.strategyBankHash);assert.equal(sale.proof.ownedEntry.cost,'0.170000');
 f.advance(3100);await f.cycle({marketId:'388',bid:.175,ask:.180});await f.before();assert.equal(f.c.state.owned['1077'],undefined);assert.equal(f.c.state.owned['995'].cost,'0.160000');
 for(const [id,i]of Object.entries(originalState.intents))assert.deepEqual(f.c.state.intents[id],i,'legacy intent remains byte-equivalent');
 assert.equal(JSON.parse(fs.readFileSync(path.join(f.dir,'competition-stats.json'))).realizedPnl,'0.005000');
 f.restart();assert.equal(f.c.ownedEntry('995').modelHash,f.activation.strategyBankHash);
});

test('initial revision blocks unknown, open and live legacy proposals without writing its proposal journal',async t=>{
 for(const kind of ['unknown','open','proposal']){const f=fixture(t,{legacy:true,ambiguous:kind==='unknown',fill:kind!=='open'});await f.cycle();if(kind!=='proposal'){f.advance(3100);await f.cycle();}
  const prior=structuredClone(f.c.state),bytes=fs.readFileSync(f.c.proposalJournal.file);assert.throws(()=>f.migrate(),/REVISION_(UNRESOLVED_INTENT|LIVE_LEGACY_PROPOSAL)/);assert.equal(fs.existsSync(path.join(f.c.dir,'median-proposals-v2.jsonl')),false);assert.deepEqual(f.c.state,prior);assert.deepEqual(fs.readFileSync(path.join(f.c.dir,'median-proposals.jsonl')),bytes);
 }
});

test('expired legacy proposal retains UUID, original uncapped terms and old bank without becoming a V2 order',async t=>{
 const f=fixture(t,{legacy:true,featureBid:()=>.5});await f.cycle({bid:.490,ask:.495});const original=structuredClone(f.proposals()[0]);assert.equal(original.limit,'0.500000');f.advance(120001);f.migrate();
 assert.deepEqual(f.c.proposals.proposals[original.intentId],original);await f.before();const retained=f.c.proposals.proposals[original.intentId];assert.equal(retained.status,'expired');for(const k of ['intentId','idempotencyKey','bankHash','limit','proofHash','createdAt'])assert.equal(retained[k],original[k]);assert.equal(f.server.posts.length,0);
});

test('revision cannot change legacy identity, scope, original source proof, risk policy, activation window or source archive',async t=>{
 for(const kind of ['priorHash','scope','policy','prefix','window','archive']){const f=fixture(t,{legacy:true});assert.throws(()=>f.migrate(({revision,bank,archive})=>{
   if(kind==='priorHash')bank.legacyCompatibility.priorBankHash='f'.repeat(64);
   if(kind==='scope')bank.universeHash='f'.repeat(64);
   if(kind==='policy')bank.policy={...bank.policy,stopLossTicks:5};
   if(kind==='prefix')bank.sourcePrefix={...bank.sourcePrefix,hash:'f'.repeat(64)};
   if(kind==='window')revision.activationHash=payloadHash({...f.activation,entryExpiresAt:iso(f.now+7200000)});
   if(kind==='archive')fs.appendFileSync(path.join(archive,'execution/median-controller.mjs'),'\n// changed\n');
   revision.nextBankHash=digest(bank);
  }),/MEDIAN_EXECUTION_(LEGACY_COMPATIBILITY|BANK_SCOPE_OR_SOURCE|POLICY|REVISION_BINDING|CODE_IDENTITY)/,kind);assert.equal(f.server.posts.length,0);}
});

test('V2 scheduling preserves caller order while exposing next-bank provenance without changing activation',async t=>{
 const f=fixture(t,{markets:[{marketId:'377',exchangeId:'700',option:'YES'},{marketId:'354',exchangeId:'701',option:'YES'}]});await f.before(['354','377']);assert.deepEqual(f.c.selected,['354']);assert.equal(f.c.policyBankHash,f.revision.nextBankHash);assert.equal(f.c.schedulingBankHash,f.revision.nextBankHash);assert.equal(f.c.activation.strategyBankHash,f.revision.priorBankHash);
});

test('new-bank proposal audit cannot claim another revision or precede its activation boundary',async t=>{
 const f=fixture(t);await f.cycle();for(const kind of ['revision','clock']){const state=structuredClone(f.c.proposals),p=Object.values(state.proposals)[0];if(kind==='revision')p.proof.revisionHash='f'.repeat(64);else{p.createdAt=iso(Date.parse(f.revision.recordedAt)-1);p.expiresAt=iso(Date.parse(p.createdAt)+120000);p.proof.decisionAvailableAt=p.createdAt;}p.proofHash=payloadHash(p.proof);assert.throws(()=>f.c.validateProposalState(state),/NEW_PROPOSAL_REVISION/);}
});

test('all four protected identities receive fair bounded metadata selection without an auxiliary eligibility prerequisite',async t=>{
 const markets=[{marketId:'195',exchangeId:'800',option:'YES'},{marketId:'356',exchangeId:'801',option:'YES'},{marketId:'377',exchangeId:'802',option:'YES'},{marketId:'354',exchangeId:'803',option:'YES'}],f=fixture(t,{markets}),ids=markets.map(m=>m.marketId),seen=[];
 for(let cycle=1;cycle<=4;cycle++){await f.before(ids,{metadataCycle:cycle});assert.equal(f.c.selected.length,1);seen.push(f.c.selected[0]);assert.equal(f.c.lastProtectedMetadataPlan.metadataCandidateBudget,1);assert.equal(f.c.lastProtectedMetadataPlan.executionAuthority,false);}
 assert.deepEqual(seen,ids);assert.equal(f.server.posts.length,0);assert.equal(f.proposals().length,0);
});

test('a verified bulk hint ranks metadata only; a fresh nonqualifying original book still creates no proposal',async t=>{
 const markets=[{marketId:'195',exchangeId:'800',option:'YES'},{marketId:'356',exchangeId:'801',option:'YES'}],f=fixture(t,{markets});
 const at=iso(f.now),quotes=markets.map(m=>({...m,latestPrice:.335,bestBid:.335,bestAsk:.340,spread:.005,crossed:false})),body={sequence:22,previous:f.bank.sourcePrefix.hash,at,event:{type:'market_price_observation',snapshot:{tournamentId:TID,universeHash:f.universe.hash,requestStartedAt:iso(f.now-2),receivedAt:iso(f.now-1),sourceTimestamp:null,depthAvailable:false,executable:false,quotes,missingIds:[]}},state:null},row={...body,hash:digest(body)};fs.appendFileSync(path.join(f.dir,'market-prices.jsonl'),JSON.stringify(row)+'\n');f.advance(1);
 await f.before(markets.map(m=>m.marketId),{metadataCycle:1,protectedBulkHints:[{...quotes[1],knownAt:at,sourceSequence:22,sourceHash:row.hash,depthAvailable:false,executable:false}]});
 assert.deepEqual(f.c.selected,['356']);assert.equal(f.c.lastProtectedMetadataPlan.nextMetadataPhase,'causal_reference_hint');await f.c.onBook(f.book({marketId:'356',bid:.350,ask:.355}));assert.equal(f.proposals().length,0);assert.equal(f.server.posts.length,0);assert.equal(f.c.lastReference.reason,'insufficient_one_tick_gap');
});
