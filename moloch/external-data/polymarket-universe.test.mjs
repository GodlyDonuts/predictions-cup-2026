import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {gzipSync} from 'node:zlib';
import {buildUniverse,loadUniverse,discoveryPaths} from './polymarket-universe.mjs';
const hash=b=>createHash('sha256').update(b).digest('hex');
const receipt=(events,extra={})=>{const bodyText=typeof events==='string'?events:JSON.stringify(events);return {endpoint:'/events?slug=oregon-senate-election-winner',requestStartedAt:'2026-10-17T18:30:00.000Z',receivedAt:'2026-10-17T18:30:00.050Z',status:200,bodyText,bodySha256:hash(bodyText),...extra};};
const market=(extra={})=>({id:'10',question:'Will the Democrats win the Oregon Senate race in 2026?',description:'Winner of the 2026 election including run-offs. This is not a SIG rule certificate.',conditionId:'0x'+'a'.repeat(64),outcomes:'["Yes","No"]',clobTokenIds:'["11","12"]',version:'v1',active:true,closed:false,enableOrderBook:true,acceptingOrders:true,...extra});
const event=(markets=[market()],extra={})=>({id:'20',slug:'oregon-senate-election-winner',title:'Oregon Senate Election Winner',active:true,closed:false,markets,...extra});
const row=(id,party='Democratic',extra={})=>({cup_market_id:id,cup_party:party,cup_office:'Senate',cup_state_code:'OR',cup_district:null,cup_election_year:2026,cup_special_election:null,...extra});
const inventory={markets:[{market_id:1},{market_id:2},{market_id:3}]};
const joins=[row('1'),row('2','Republican'),row('3','Independent',{cup_state_code:'AK'})];
const build=receipts=>buildUniverse({receipts,inventory,joins});

test('legacy original outcome order binds the returned token, never assumes index0 YES',()=>{
 const u=build([receipt([event([market({outcomes:'["No","Yes"]'})])])]);
 assert.equal(u.outcomes.find(r=>r.outcome==='No').assetId,'11');assert.equal(u.outcomes.find(r=>r.outcome==='Yes').assetId,'12');
 assert.equal(u.outcomes[0].rules,market().description);assert.equal(u.metadata[0].receivedAt,'2026-10-17T18:30:00.050Z');
});
test('V2 uses actual positionIds array; legacy token IDs cannot substitute',()=>{
 const u=build([receipt([event([market({version:'v2',positionIds:['91','92']})])])]);assert.deepEqual(u.outcomes.map(r=>r.assetId).sort(),['91','92']);
 assert.equal(build([receipt([event([market({version:'v2',positionIds:'["91","92"]'})])])]).outcomes.length,0);
});
test('overflow, noncanonical, duplicate or missing asset identifiers never mint outcomes',()=>{
 for(const ids of [['01','2'],['1','1'],['1',(1n<<256n).toString()],['1'],[1,'2']])assert.equal(build([receipt([event([market({clobTokenIds:JSON.stringify(ids)})])])]).outcomes.length,0);
});
test('inactive event/market and explicit unavailable orderbook flags exclude actual rows',()=>{
 for(const change of [{active:false},{closed:true},{enableOrderBook:false},{acceptingOrders:false},{acceptingOrders:null}])assert.equal(build([receipt([event([market(change)])])]).outcomes.length,0);
 assert.equal(build([receipt([event(undefined,{closed:true})])]).outcomes.length,0);
 const m=market();delete m.enableOrderBook;delete m.acceptingOrders;assert.equal(build([receipt([event([m])])]).outcomes.length,2);
});
test('2028, margin and primary questions are not general-winner mappings',()=>{
 for(const question of ['Will the Democrats win the Oregon Senate race in 2028?','Will the Democratic candidate win the 2026 Oregon Senate election by 10% or more?','Will Smith win the Oregon Senate primary in 2026?'])assert.equal(build([receipt([event([market({question})])])]).outcomes.length,0);
});
test('negative of one party is never silently the opposite-party outcome',()=>{
 const u=build([receipt([event()])]);const no=u.outcomes.find(r=>r.outcome==='No');
 assert.equal(no.sigReferences.find(r=>r.sigMarketId==='1').sigSide,'NO');assert.equal(no.sigReferences.find(r=>r.sigMarketId==='2').sigSide,null);
 assert.deepEqual(u.unmatchedSigMarketIds,['3']);assert.equal(u.summary.samePartyConditionalSigMarkets,1);
});
test('candidate outcomes remain race context even when a familiar candidate resembles a party',()=>{
 const u=build([receipt([event([market({question:'Will Dan Sullivan win the Alaska Senate race in 2026?',groupItemTitle:'Dan Sullivan (R)'})],{title:'Alaska Senate Election Winner',slug:'alaska-senate-election-winner'})])]);
 assert.equal(u.outcomes.length,2);assert.equal(u.outcomes[0].sigReferences[0].relationship,'candidate_race_context');assert.equal(u.outcomes[0].sigReferences[0].sigSide,null);
 assert.equal(u.summary.samePartyConditionalSigMarkets,0);
 for(const o of u.outcomes){assert.equal(o.pricingAuthority,false);assert.equal(o.resolutionEquivalenceVerified,false);}
});
test('house district references come from returned identity, not discovery query',()=>{
 const j=[row('1','Democratic',{cup_office:'House',cup_state_code:'PA',cup_district:'7'}),row('2','Republican',{cup_office:'House',cup_state_code:'PA',cup_district:'8'})];
 const u=buildUniverse({inventory,joins:j,receipts:[receipt([event([market({question:'Will the Democrats win the PA-07 House election in 2026?'})],{title:'PA-07 House Election Winner',slug:'pa-07-house-election-winner'})])]});
 assert.deepEqual(u.outcomes[0].sigReferences.map(r=>r.sigMarketId),['1']);assert.equal(u.outcomes[0].scope.district,'7');
});
test('duplicates retain original receipts; conflicting asset metadata fails closed',()=>{
 const first=receipt([event()]),second=receipt([event([market({volume:'9999'})])],{receivedAt:'2026-10-17T18:31:00.000Z'});
 const u=build([first,second]);assert.equal(u.outcomes.length,2);assert.equal(u.outcomes[0].sourceReceipts.length,2);assert.equal(u.outcomes[0].sourceReceipts[0].receivedAt,first.receivedAt);
 assert.throws(()=>build([first,receipt([event([market({description:'Changed resolution rule 2026'})])])]),/PM_ASSET_IDENTITY_CONFLICT/);
});
test('raw body tamper, wrong host and receipt-clock inversion fail source validation',()=>{
 assert.throws(()=>build([receipt([event()],{bodySha256:'0'.repeat(64)})]),/PM_RECEIPT_BODY_HASH/);
 assert.throws(()=>build([receipt([event()],{endpoint:'https://other.invalid/events'})]),/PM_RECEIPT_ENDPOINT/);
 assert.throws(()=>build([receipt([event()],{receivedAt:'2026-10-17T18:29:00.000Z'})]),/PM_RECEIPT_CLOCK/);
});
test('failed original responses and malformed JSON are retained, never fabricated coverage',()=>{
 const u=build([receipt('<html>Unavailable</html>',{status:503}),receipt('invalid-json')]);assert.equal(u.outcomes.length,0);assert.equal(u.metadata.length,2);assert.equal(u.summary.httpErrors,1);assert.equal(u.rejected[1].reason,'INVALID_JSON_RESPONSE');
});
test('query seeds are bounded discovery strings, not invented market or token IDs',()=>{
 const paths=discoveryPaths([row('1'),row('2'),row('3','Independent',{cup_state_code:'AK'}),row('4','Democratic',{cup_office:'House',cup_state_code:'PA',cup_district:'7'})]);
 assert.equal(paths.filter(p=>p.includes('oregon-senate')).length,1);assert(paths.some(p=>p.includes('alaska-senate')));assert(paths.some(p=>p.includes('public-search')));
});
function fixture(){const root=mkdtempSync(join(tmpdir(),'pm-universe-'));const base=join(root,'research/crossvenue_screen_2026-10-02');mkdirSync(base,{recursive:true});writeFileSync(join(base,'public_market_inventory_2026-10-01_184851.json.gz'),gzipSync(JSON.stringify(inventory)));writeFileSync(join(base,'join_records.json.gz'),gzipSync(JSON.stringify(joins)));return root;}
test('offline replay preserves original clocks and cache tampering cannot renew evidence',async()=>{
 const root=fixture(),cache=join(root,'cache');try{
  const u=await loadUniverse({repositoryRoot:root,cacheDirectory:cache,maximumRequests:1,fetchReceipt:async()=>receipt([event()])});
  const offline=await loadUniverse({repositoryRoot:root,cacheDirectory:cache,offline:true,fetchReceipt:async()=>{throw Error('must not fetch')}});
  assert.deepEqual(offline.outcomes,u.outcomes);assert.equal(offline.discovery.offlineReplay,true);assert.equal(u.discovery.discoveryComplete,false);
  const s=JSON.parse(readFileSync(join(cache,'snapshot.json'),'utf8'));writeFileSync(join(cache,s.receipts[0].file),'{}');await assert.rejects(loadUniverse({repositoryRoot:root,cacheDirectory:cache,offline:true}),/PM_CACHE_RECEIPT_SHA/);
 }finally{rmSync(root,{recursive:true,force:true});}
});
test('429 stops discovery without a new retry/admission and preserves original body',async()=>{
 const root=fixture(),cache=join(root,'cache');let calls=0;try{const u=await loadUniverse({repositoryRoot:root,cacheDirectory:cache,fetchReceipt:async()=>{calls++;return receipt('rate limited',{status:429,responseHeaders:{retryAfter:'60'}});}});assert.equal(calls,1);assert.equal(u.metadata[0].status,429);assert.equal(u.discovery.discoveryComplete,false);assert.equal(u.discovery.failures[0].retryAfter,'60');}finally{rmSync(root,{recursive:true,force:true});}
});
test('truncated discovery explicitly exposes remaining pages without invented outcomes',()=>{
 const u=build([receipt({events:[event()],pagination:{hasMore:true,totalResults:50}})]);assert.equal(u.summary.truncatedSearchQueries,1);assert.equal(u.outcomes.length,2);assert.equal(u.metadata[0].pagination.totalResults,50);
});
test('cached mapping replay refuses a changed original inventory identity source',async()=>{
 const root=fixture(),cache=join(root,'cache');try{await loadUniverse({repositoryRoot:root,cacheDirectory:cache,maximumRequests:1,fetchReceipt:async()=>receipt([event()])});
 writeFileSync(join(root,'research/crossvenue_screen_2026-10-02/join_records.json.gz'),gzipSync(JSON.stringify([row('1','Republican')])));
 await assert.rejects(loadUniverse({repositoryRoot:root,cacheDirectory:cache,offline:true}),/PM_CACHE_INVENTORY_CHANGED/);
 }finally{rmSync(root,{recursive:true,force:true});}
});
