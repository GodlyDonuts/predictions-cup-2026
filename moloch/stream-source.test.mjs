import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import test from 'node:test';import assert from 'node:assert/strict';import {StreamSource,predecessorTradeFacts} from './stream-source.mjs';import {payloadHash} from '../execution/ledger.mjs';import {createMoloch,advanceMoloch,assertMoloch} from './engine.mjs';
const tid='bda92870-621e-47b0-bc3c-3602c5c26f55',at=n=>new Date(Date.parse('2026-10-13T22:00:00Z')+n*1000).toISOString();
const rawBook=(seq,n)=>({exchangeId:'2',asOf:{sequence:seq,at:at(n)},nextExpiryAt:null,bids:[{price:.4,quantity:100}],asks:[{price:.42,quantity:100}]});
const restBook=(seq,n)=>({source:'official_sig_api_v1',status:200,kind:'book',marketId:'1',exchangeId:'2',tournamentId:tid,requestStartedAt:at(n),receivedAt:at(n+.1),endpoint:'/markets/1/orderbook?tournamentId='+tid,data:{contexts:[{type:'tournament',tournament:{id:tid},orderbook:{exchanges:[rawBook(seq,n)]}}]}});
const tape=(n,to=n)=>({source:'official_sig_api_v1',status:200,kind:'trades',marketId:'1',exchangeId:'2',tournamentId:tid,requestStartedAt:at(n),receivedAt:at(n+.1),endpoint:'/exchanges/2/trades?tournamentId='+tid+'&from='+at(0)+'&to='+at(to),data:{marketId:'1',exchangeId:'2',from:at(0),to:at(to),data:[],pagination:{hasMore:false,nextCursor:null},coverage:{complete:true,projectedThroughSequence:20}}});
const setup=()=>{const mux=new StreamSource([{marketId:'1',exchangeId:'2'}],tid,at(0));let sequence=0,previous='';return{mux,row(event){const body={sequence:++sequence,previous,event},r={...body,hash:payloadHash(body)};previous=r.hash;return r;}};};
const batch=(revision,previousRevision,seq,n,trades=[])=>({kind:'broadcast',source:'official_sig_realtime',marketId:'1',tournamentId:tid,event:'market_batch',receivedAt:at(n+.2),payload:{delivery:{model:'best-effort-authoritative-resync',revision,previousRevision},resyncRequired:false,books:[rawBook(seq,n)],trades,marketSettled:[]}});
test('initial and gap recovery require both a later REST book and complete tape spanning invalidation',()=>{
  const {mux,row}=setup();assert.equal(mux.row(row({kind:'subscribed',marketId:'1',receivedAt:at(0)}))[0].type,'gap');assert.equal(mux.rest(restBook(1,1))[0].book.invalid,true);assert.equal(mux.rest(tape(2)).at(-1).type,'book');
  mux.row(row(batch(1,0,2,3)));const gap=mux.row(row(batch(4,3,3,4)));assert.equal(gap[0].type,'gap');assert.equal(gap.some(e=>e.type==='book'),false);
  mux.rest(restBook(4,5));mux.rest(tape(6,3));assert.equal(mux.status.get('1').blocked,true);mux.rest(tape(7));assert.equal(mux.status.get('1').blocked,false);
});
test('older REST cannot roll back a pushed book or redate its source clock',()=>{
  const {mux,row}=setup();mux.rest(restBook(1,1));mux.rest(tape(2));mux.row(row(batch(1,0,3,3)));
  const event=mux.rest(restBook(2,4)).at(-1);assert.equal(event.book.sequence,'3');assert.equal(event.book.sourceAt,at(3));assert.equal(event.book.transport,'official_sig_realtime');assert.equal(mux.metrics.olderBooks,1);
  const rememberedStart=mux.books.get('2').requestStartedAt;const repeated=mux.rest(tape(8)).at(-1);assert.equal(repeated.book.requestStartedAt,rememberedStart);
});
test('a duplicate revision can carry a newer book but never replay its trades',()=>{
  const {mux,row}=setup();mux.rest(restBook(1,1));mux.rest(tape(2));mux.row(row(batch(1,0,2,3)));
  const events=mux.row(row(batch(1,0,3,4)));assert.equal(events.filter(e=>e.type==='book').length,1);assert.equal(events.filter(e=>e.type==='trade').length,0);assert.equal(mux.metrics.duplicates,1);
});
test('stream execution is evaluated before its newer book and REST duplicate cannot fill twice',()=>{
  const {mux,row}=setup(),state=createMoloch({quantity:1}),apply=es=>es.forEach(e=>advanceMoloch(state,e));apply(mux.row(row({kind:'subscribed',marketId:'1',receivedAt:at(0)})));apply(mux.rest(restBook(1,1)));apply(mux.rest(tape(2)));
  const trade={id:'7',sequence:7,marketId:'1',exchangeId:'2',tournamentId:tid,price:.39,quantity:100,executedAt:at(6).replace('.000Z','.000000000Z')};const events=mux.row(row(batch(1,0,7,6,[trade])));assert.equal(events[0].type,'trade');apply(events);assert.equal(state.fills.length,1);
  const r=tape(8);r.data.data=[{id:'7',price:.39,size:100,createdAt:at(6)}];apply(mux.rest(r));assert.equal(state.fills.length,1);assertMoloch(state);
});
test('tampered stream chain and changed same-version levels are rejected',()=>{
  const {mux,row}=setup();const r=row(batch(1,0,1,1));assert.throws(()=>mux.row({...r,event:{}}),/CHAIN/);
  const good=setup();good.mux.rest(restBook(1,1));good.mux.rest(tape(2));const changed=batch(1,0,1,1);changed.payload.books[0].bids[0].quantity=999;assert.throws(()=>good.mux.row(good.row(changed)),/VERSION_CONFLICT/);
});
test('historical reservations retire behind an authoritative barrier while new quoting remains blocked',()=>{
  const state=createMoloch({quantity:1,lifetimeMs:20000});advanceMoloch(state,{type:'book',receivedAt:at(0),book:{tournamentId:tid,marketId:'1',exchangeId:'2',sequence:'1',sourceAt:at(0),requestStartedAt:at(0),receivedAt:at(0),bids:[{price:'0.400000',quantity:100}],asks:[{price:'0.420000',quantity:100}]}});
  const mux=new StreamSource([{marketId:'1',exchangeId:'2'}],tid,at(60)),apply=es=>es.forEach(e=>advanceMoloch(state,e));apply(mux.invalidate('1',at(60)));apply(mux.rest(restBook(100,61)));assert.equal(state.orders.length,2);assert.equal(state.orders[0].barrier.sequence,'100');
  const recovery=tape(62,21);recovery.data.coverage.projectedThroughSequence=110;apply(mux.rest(recovery));assert.equal(state.orders.every(q=>q.retired),true);assert.equal(mux.status.get('1').blocked,true);assert.equal(state.cash,'1000.000000');assertMoloch(state);
});
test('official REST millisecond rounding deduplicates while both original wire clocks remain preserved',()=>{
  const {mux}=setup(),wire={type:'trade',marketId:'1',exchangeId:'2',tournamentId:tid,id:'shared',idNamespace:'sig_engine_match',executedAt:'2026-10-13T22:00:04.603535900Z',receivedAt:at(6),yesPrice:'0.655000',quantity:180};
  mux.trade(wire);const rest={...wire,idNamespace:'sig_reporting_tape_only',executedAt:'2026-10-13T22:00:04.603Z',receivedAt:at(8)},duplicate=mux.trade(rest);
  assert.equal(duplicate.executedAt,wire.executedAt);assert.equal(duplicate.originalExecutedAt,rest.executedAt);assert.equal(mux.metrics.precisionReconciliations,1);
  assert.throws(()=>mux.trade({...rest,quantity:181}),/TRADE_FACT_CONFLICT/);
  assert.throws(()=>mux.trade({...rest,executedAt:'2026-10-13T22:00:04.604Z'}),/TRADE_CLOCK_CONFLICT/);
  assert.throws(()=>mux.trade({...rest,executedAt:'2026-10-13T22:00:04.603600Z'}),/TRADE_CLOCK_CONFLICT/);
  assert.throws(()=>mux.trade({...rest,idNamespace:'unknown'}),/TRADE_CLOCK_CONFLICT/);
});

test('resume restores original trade provenance rather than normalized reporting duplicates',()=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'moloch-provenance-')),dir=path.join(root,'episode');fs.mkdirSync(dir);
  try{const wire={type:'trade',marketId:'1',exchangeId:'2',tournamentId:tid,id:'shared',idNamespace:'sig_engine_match',executedAt:'2026-10-13T22:00:04.603535900Z',yesPrice:'0.655000',quantity:22},normalized={...wire,idNamespace:'sig_reporting_tape_only',originalExecutedAt:'2026-10-13T22:00:04.603Z'},key=[tid,'2','shared'].join(':'),state={seen:{[key]:payloadHash({marketId:wire.marketId,executedAt:wire.executedAt,yesPrice:wire.yesPrice,quantity:wire.quantity})}};let previous='';const rows=[wire,normalized].map((event,i)=>{const body={sequence:i+1,previous,event},row={...body,hash:payloadHash(body)};previous=row.hash;return row;});const snapshot={state,sequence:2,previous};fs.writeFileSync(path.join(dir,'events.jsonl'),rows.map(JSON.stringify).join('\n')+'\n');fs.writeFileSync(path.join(dir,'state.json'),JSON.stringify({...snapshot,hash:payloadHash(snapshot)}));fs.writeFileSync(path.join(dir,'manifest.json'),'{}');
  const facts=predecessorTradeFacts(dir,state,root);assert.equal(facts.get(key).idNamespace,'sig_engine_match');const mux=new StreamSource([],tid,at(0),facts);assert.equal(mux.trade({...wire,idNamespace:'sig_reporting_tape_only',executedAt:normalized.originalExecutedAt}).executedAt,wire.executedAt);assert.throws(()=>mux.trade({...wire,executedAt:'2026-10-13T22:00:04.604Z'}),/CLOCK_CONFLICT/);
  }finally{if(!path.resolve(root).startsWith(path.resolve(os.tmpdir())+path.sep+'moloch-provenance-'))throw Error('TEMP_SCOPE');fs.rmSync(root,{recursive:true});}
});

test('global observation rows advance verified chain without reaching financial state',()=>{
 const mux=new StreamSource([{marketId:'1',exchangeId:'2'}],tid,at(0),new Map(),[{marketId:'1',exchangeId:'2'},{marketId:'3',exchangeId:'4'}]);let sequence=0,previous='';const row=event=>{const body={sequence:++sequence,previous,event},r={...body,hash:payloadHash(body)};previous=r.hash;return r;};
 const other=batch(1,0,1,1,[{id:'other',quantity:100,price:.39}]);other.marketId='3';assert.deepEqual(mux.row(row(other)),[]);assert.equal(mux.facts.size,0);assert.equal(mux.books.size,0);assert.equal(mux.status.has('3'),false);assert.equal(mux.metrics.observationRows,1);
 assert.equal(mux.row(row({kind:'subscribed',source:'official_sig_realtime',marketId:'1',receivedAt:at(2)}))[0].type,'gap');assert.equal(mux.sequence,2);assert.deepEqual(mux.row(row({kind:'rest_reference',source:'official_sig_api_v1',marketId:'3',receivedAt:at(3)})),[]);assert.equal(mux.facts.size,0);
 const bad=row({...other,tournamentId:'wrong'});assert.throws(()=>mux.row(bad),/OBSERVATION_TOURNAMENT/);
});
