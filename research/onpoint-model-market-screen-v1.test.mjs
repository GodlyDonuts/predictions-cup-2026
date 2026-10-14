import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {parseOnPointGovernorForecast, parseCallTheMapMichiganForecast, compareIndependentModelToScreens, parseGovernorContracts, analyzeOnPointBooks, readFixedHashChainedBookTail} from './onpoint-model-market-screen-v1.mjs';

const hash = value => createHash('sha256').update(value).digest('hex');
const canonical = value => JSON.stringify(value && typeof value === 'object'
  ? Array.isArray(value) ? value.map(x => JSON.parse(canonical(x)))
    : Object.fromEntries(Object.keys(value).sort().map(k => [k, JSON.parse(canonical(value[k]))])) : value);
const iso = ms => new Date(ms).toISOString();
const now = Date.parse('2026-10-14T16:55:00.000Z');
const html = ({stateName = 'Florida', party = 'R', margin = '+1.7', pollParty = 'R', pollMargin = '+3.5', pct = 62, date = 'October 13, 2026'} = {}) =>
  `<html><head><title>${stateName} governor forecast | OnPoint Politics</title></head><body><p class="lede">David Jolly<!-- --> vs <!-- -->Byron Donalds<!-- -->, an open seat. The model has Byron Donalds ahead by 1.7 points, winning ${pct} percent of 10,000 simulated elections.</p><div class="pmeta"><span>Model <b class="mono">${party} ${margin}</b></span><span>Polls <b class="mono">${pollParty} ${pollMargin}</b></span><span>Updated <b>${date}</b></span></div></body></html>`;
const forecast = (overrides = {}) => parseOnPointGovernorForecast({state:'FL',url:'https://www.tpsielections.com/forecast/fl/governor',html:html(overrides),fetchedAt:iso(now-1000),contentHash:'a'.repeat(64)});
const market = (id, party, name = 'Florida') => ({id:String(id),title:`Will the ${party} Party win the ${name} Governor?`,status:'open',settlementDate:'2026-11-04T17:00:00Z',exchanges:[{id:String(900+id),option:'YES'}],contexts:[{tournament:{id:'tournament'}}]});
const quote = ({received = now-2000, bid='0.760000', ask='0.765000', askQty=100, source = received-20, publication = received+20} = {}) => ({record:{kind:'book',at:iso(publication)},book:{tournamentId:'tournament',marketId:'299',exchangeId:'1199',instrument:'1199:YES',receivedAt:iso(received),requestStartedAt:iso(received-300),sourceAt:iso(source),sourceEventAgeMs:received-source,freshnessPolicy:'authoritative_receipt_v1',bids:[{price:bid,quantity:90}],asks:[{price:ask,quantity:askQty}]}});

test('parses displayed favorite, margin, poll gap, and rounds only the displayed probability', () => {
  const parsed = forecast();
  assert.equal(parsed.modelFavoriteParty,'R'); assert.equal(parsed.displayedFavoriteWinPct,62);
  assert.deepEqual(parsed.displayRoundingOnly,{low:61.5,high:62.5});
  assert.equal(parsed.modelMarginForDPoints,-1.7); assert.equal(parsed.pollingMarginForDPoints,-3.5);
  assert.equal(parsed.modelMinusPollPoints,1.8); assert.equal(parsed.probabilityCalibratedForSIG,false);
});

test('parses exact state-party contract titles and exposes partial metadata coverage', () => {
  const result = parseGovernorContracts({data:[market(299,'Democratic'),market(300,'Republican'),{...market(301,'Democratic'),'title':'Will the Democratic Party win the Florida Senate?'}],pagination:{total:237,hasMore:true}});
  assert.deepEqual(result.contracts.map(x=>[x.marketId,x.state,x.party]),[['299','FL','D'],['300','FL','R']]);
  assert.equal(result.metadataPageSize,3); assert.equal(result.totalMarkets,237); assert.equal(result.metadataCoverageComplete,false);
});

test('compares both executable sides at zero fees and applies one adverse tick', () => {
  const parsed = forecast();
  const contracts = parseGovernorContracts({data:[market(299,'Democratic')],pagination:{total:1,hasMore:false}}).contracts;
  const result = analyzeOnPointBooks({contracts,forecasts:[parsed],books:[quote()],asOf:iso(now),tick:0.005});
  assert.equal(result.screenCount,2,JSON.stringify(result));
  assert.equal(result.fees,0); assert.equal(result.executionAuthority,false); assert.equal(result.orderCandidates,0);
  const yes=result.screens.find(x=>x.side==='YES'), no=result.screens.find(x=>x.side==='NO');
  assert.equal(yes.topAsk,0.765); assert.equal(yes.modelProbability,0.38); assert.equal(yes.oneTickAdverseAsk,0.77);
  assert.equal(no.topAsk,0.24); assert.equal(no.topAskQuantity,90); assert.equal(no.modelProbability,0.62);
  assert.equal(no.rawProbabilityMinusAsk,0.38); assert.equal(no.roundingFloorMinusOneTickAsk,0.37);
});

test('excludes stale, future-source, crossed and forecast-after-quote books', () => {
  const parsed=forecast(), contracts=parseGovernorContracts({data:[market(299,'Democratic')],pagination:{total:1,hasMore:false}}).contracts;
  const variants=[
    quote({received:now-90000}), quote({source:now+1}), quote({bid:'0.770000'}),
    quote({received:now-2000})
  ];
  variants[3].book.receivedAt=iso(now-2000); const late={...parsed,fetchedAt:iso(now-1000)};
  const r1=analyzeOnPointBooks({contracts,forecasts:[parsed],books:[variants[0]],asOf:iso(now)});
  const r2=analyzeOnPointBooks({contracts,forecasts:[parsed],books:[variants[1]],asOf:iso(now)});
  const r3=analyzeOnPointBooks({contracts,forecasts:[parsed],books:[variants[2]],asOf:iso(now)});
  const r4=analyzeOnPointBooks({contracts,forecasts:[{...late,modelUpdateDate:'October 15, 2026'}],books:[variants[3]],asOf:iso(now)});
  assert.equal(r1.screenCount,0); assert.equal(r2.screenCount,0); assert.equal(r3.screenCount,0); assert.equal(r4.screenCount,0);
});

test('accepts only the repository-authorized bounded source-clock skew', () => {
  const parsed=forecast(), contracts=parseGovernorContracts({data:[market(299,'Democratic')],pagination:{total:1,hasMore:false}}).contracts;
  const within=quote({source:now-1880});
  const beyond=quote({source:now+1});
  assert.ok(within.book.sourceEventAgeMs < 0 && within.book.sourceEventAgeMs >= -2000);
  assert.equal(analyzeOnPointBooks({contracts,forecasts:[parsed],books:[within],asOf:iso(now)}).screenCount,2);
  assert.equal(analyzeOnPointBooks({contracts,forecasts:[parsed],books:[beyond],asOf:iso(now)}).screenCount,0);
});

test('independent model cross-check rejects a one-source-only Michigan edge claim', () => {
  const source='<html><head><meta name="description" content="Forecast as of October 14, 2026: Call the Map currently has Jocelyn Benson ahead of John James by D+6.4, with an 84 in 100 chance to win. This is an open seat. The current poll average is D+7.5; 41 public polls are recorded."></head><script>{"dateModified":"2026-10-14T13:18:16.000Z"}</script></html>';
  const ctm=parseCallTheMapMichiganForecast({url:'https://www.callthemap.com/governors/michigan',html:source,fetchedAt:iso(now),contentHash:'c'.repeat(64)});
  const rows=compareIndependentModelToScreens([
    {marketId:'305',state:'MI',party:'D',side:'YES',topAsk:0.9,oneTickAdverseAsk:0.905,modelProbability:0.98},
    {marketId:'305',state:'MI',party:'D',side:'NO',topAsk:0.105,oneTickAdverseAsk:0.11,modelProbability:0.02}
  ],ctm);
  assert.equal(ctm.sourceModifiedAt,'2026-10-14T13:18:16.000Z'); assert.equal(ctm.pollCount,41);
  assert.equal(rows[0].publishedProbabilityDifferenceVsOnPoint,-0.14);
  assert.ok(rows[0].externalModelRoundingFloorMinusOneTickAsk<0);
  assert.ok(rows[1].externalModelRoundingFloorMinusOneTickAsk>0);
  assert.equal(rows.every(x=>x.label.includes('not_trade')),true);
});

test('reads a fixed byte window and verifies canonical record hashes and adjacent chain links', () => {
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-public-books-'));
  try {
    const file=path.join(dir,'books.jsonl');
    let previousHash='', sequence=0;
    const record=(kind,payload,at)=>{const body={schemaVersion:1,kind,sequence:++sequence,previousHash,sessionId:'session-1',tournamentId:'tournament',scopeHash:'b'.repeat(64),at,payload};const h=hash(canonical(body));previousHash=h;return JSON.stringify({...body,hash:h})+'\n';};
    const rows=[record('session_start',{scope:{}},iso(now-1000)),record('book',{book:quote().book},iso(now-900)),record('invalidation',{marketId:'999',reason:'INVALID_BOOK'},iso(now-800))];
    fs.writeFileSync(file,rows.join(''));
    const result=readFixedHashChainedBookTail(file,{maxBytes:4096});
    assert.equal(result.lineCount,3);assert.equal(result.counts.session_start,1);assert.equal(result.books.length,1);
    assert.equal(result.first.sequence,1);assert.equal(result.last.sequence,3);assert.equal(result.rawWindowHash,hash(Buffer.from(rows.join(''))));
    const bad=fs.readFileSync(file,'utf8').replace('INVALID_BOOK','STOP');fs.writeFileSync(file,bad);
    assert.throws(()=>readFixedHashChainedBookTail(file,{maxBytes:4096}),/CAPTURE_RECORD_HASH/);
    fs.writeFileSync(file,'not-json\n');
    assert.throws(()=>readFixedHashChainedBookTail(file,{maxBytes:4096}),/CAPTURE_JSONL_INVALID/);
  } finally { fs.rmSync(dir,{recursive:true,force:true}); }
});
