import fs from 'node:fs';import path from 'node:path';
import test from 'node:test';import assert from 'node:assert/strict';
import {ReadClient} from '../live-paper/transport-runtime.mjs';
import {createScopedRestReceipts} from './scoped-rest-receipts.mjs';
import {createPairedRestEvidenceBridge as currentBridge} from './paired-rest-evidence-bridge.mjs';
// Regression against the documented and observed official response shape.
const createBridge=currentBridge;
const tid='bda92870-621e-47b0-bc3c-3602c5c26f55',markets=[{marketId:'305',exchangeId:'994'},{marketId:'306',exchangeId:'995'}],base=Date.parse('2026-10-06T12:36:49.000Z'),iso=n=>new Date(base+n).toISOString();
const budget={schemaVersion:4,ownerId:'Codex',enabled:true,intervalMs:1000,writeIntervalMs:2500,soleAccountClient:true,executionOwner:'Codex',handoffId:'human-to-baal-offline-test',ackCommentId:1,writesEnabled:true,authorityKind:'direct_human_assignment',assignmentCommentId:2};
const page=()=>({data:[],pagination:{limit:200,hasMore:false,nextCursor:null},coverage:{complete:true,projectedThroughSequence:67828213}});
// Matches the public wire wrapper in original-source cycle 18212/hash
// c6d426faca144c16de054c57b76a271161473a10b6cb20075252de2ccb741c14,
// received 2026-10-06T12:36:50.119Z. This is synthetic clock/depth data using
// that observed schema, not a claim to replay the full captured response.
function officialBook(clock){const book={exchanges:[{exchangeId:'994',option:'YES',asOf:{sequence:67828213,at:iso(clock)},impliedProbability:.875,bids:[{price:.870,quantity:2884}],asks:[{price:.875,quantity:604}]}],overround:.875,hasArbitrageOpportunity:false};return{...structuredClone(book),contexts:[{type:'tournament',tournament:{id:tid,slug:'midterm-elections',name:'Midterm Elections',currencyName:'SUSQies',isOngoingPlay:false},orderbook:structuredClone(book)}]};}
function fixture(t,mutate=()=>{}){let clock=1000,bridge;const calls=[];const scope={tournamentId:tid,tournamentSlug:'midterm-elections',markets};
 const producer=createScopedRestReceipts({scope:{...scope,marketIds:markets.map(m=>m.marketId)},now:()=>base+clock,fetchFn:async(url,spec)=>{const u=new URL(url),endpoint=u.pathname.slice('/api/v1'.length)+u.search;calls.push({endpoint,method:spec.method});assert.equal(spec.method,'GET');let data;
  if(endpoint==='/tournaments/midterm-elections')data={id:tid,isPendingEnrolment:false,myBalance:100};
  else if(endpoint.endsWith('/portfolio/positions'))data={positions:[],summary:{totalMarketValue:0,totalCostBasis:0,totalUnrealizedPnl:0}};
  else if(endpoint.startsWith('/orders?'))data=page();
  else{assert.equal(endpoint,'/markets/305/orderbook?tournamentId='+tid+'&depth=20');data=officialBook(clock);mutate(data);}
  return new Response(new ReadableStream({pull(c){clock+=50;c.enqueue(new TextEncoder().encode(JSON.stringify(data)));c.close();}}),{status:200});}});
 const api=new ReadClient('synthetic-not-a-key',{networkBudget:budget,fetchFn:producer.fetch,now:()=>base+clock,sleepFn:async ms=>clock+=ms,authorizeDispatch:s=>bridge.authorizes(s)});
 bridge=createBridge({api,receiptOf:producer.receiptOf,scope,getGlobalRisk:()=>({tournamentId:tid,at:iso(clock),sourceHash:'a'.repeat(64)}),getFunding:()=>null,getOtherReserved:()=> '0.000000',ownsOrder:()=>false,canRead:()=>true,hasReadHeadroom:()=>true,now:()=>base+clock});
 t.after(()=>{bridge.close();api.close();producer.close();});return{bridge,calls};}
test('cached primary OpenAPI defines contexts plus combined book, without a marketId field',()=>{const spec=JSON.parse(fs.readFileSync(path.join(import.meta.dirname,'../openapi.json'))),response=spec.paths['/markets/{id}/orderbook'].get.responses['200'].content['application/json'].schema;assert(response.allOf.some(x=>x.$ref==='#/components/schemas/CombinedMarketOrderbook'));assert(response.allOf.some(x=>x.required?.includes('contexts')));const book=spec.components.schemas.CombinedMarketOrderbook;assert.equal(Object.hasOwn(book.properties,'marketId'),false);assert.deepEqual(book.required,['exchanges','overround','hasArbitrageOpportunity']);});
test('documented and observed book response without top-level marketId passes exact scoped preparation',async t=>{const f=fixture(t);await f.bridge.refreshPreparation(markets[0]);const e=f.bridge.getEvidence({operation:'prepare',...markets[0]});assert.equal(f.calls.length,5);assert(f.calls.every(x=>x.method==='GET'));assert.equal(Object.hasOwn(JSON.parse(e.book.bodyText),'marketId'),false);assert.equal(e.book.endpoint,'/markets/305/orderbook?tournamentId='+tid+'&depth=20');assert.equal(f.bridge.status().cachedKind,'preparation');});
test('an optional matching top-level marketId remains valid',async t=>{const f=fixture(t,d=>d.marketId='305');await f.bridge.refreshPreparation(markets[0]);assert.equal(f.bridge.status().cachedKind,'preparation');});
test('explicit contradictory or malformed top-level marketId fails closed',async t=>{for(const value of ['306',null,305]){const f=fixture(t,d=>d.marketId=value);await assert.rejects(f.bridge.refreshPreparation(markets[0]));assert.equal(f.bridge.status().cachedKind,null);}});
test('omitting marketId never permits foreign tournament, foreign exchange or multiple scopes',async t=>{for(const change of [d=>d.contexts[0].tournament.id='bda92870-621e-47b0-bc3c-3602c5c26f56',d=>d.contexts[0].orderbook.exchanges[0].exchangeId='995',d=>d.contexts.push(structuredClone(d.contexts[0])),d=>d.contexts[0].orderbook.exchanges.push({...structuredClone(d.contexts[0].orderbook.exchanges[0]),exchangeId:'995'})]){const f=fixture(t,change);await assert.rejects(f.bridge.refreshPreparation(markets[0]));assert.equal(f.bridge.status().cachedKind,null);}});
