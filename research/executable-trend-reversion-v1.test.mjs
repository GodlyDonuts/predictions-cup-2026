import test from 'node:test';
import assert from 'node:assert/strict';
import {analyzeExecutableTrendReversionV1} from './executable-trend-reversion-v1.mjs';

const epoch = Date.parse('2026-10-13T00:00:00.000Z');
const at = ms => new Date(epoch + ms).toISOString();
function book(ms, bid, ask, {marketId='101', sequence=String(ms + 1), segment='1:0', sourceMs=ms,
  bidQty=100, askQty=100, sessionId='session-a'}={}) {
  return {marketId, exchangeId:'1001', receivedAt:at(ms), sourceAt:at(sourceMs), sequence, sessionId, segment,
    bids:[{price:bid.toFixed(6),quantity:bidQty}], asks:[{price:ask.toFixed(6),quantity:askQty}]};
}
const options = overrides => ({decisionLatencyMs:0, lookbackMs:[1_000], holdingMs:[1_000], decisionSpacingMs:3_000,
  maxLookbackDriftMs:0, maxBookAgeMs:5_000, maxEntryDelayMs:1_000, maxExitDelayMs:0, ...overrides});
const result = (report, rule, period='heldout') => report.results.find(row => row.rule===rule && row.period===period);

test('fixed-sign trend and reversion use only the direction preregistered for YES',()=>{
  const rising=[book(0,.38,.40),book(1_000,.39,.41),book(2_000,.40,.42),book(3_000,.42,.44),
    book(4_000,.43,.45),book(5_000,.46,.48)];
  const trend=analyzeExecutableTrendReversionV1({observations:rising,splitAt:at(3_000),options:options()});
  assert.equal(result(trend,'trend').signals,1);
  assert.equal(result(trend,'trend').completed,1);
  assert.ok(Math.abs(result(trend,'trend').totalNetPnlOneShare-.01)<1e-9);
  assert.ok(Math.abs(result(trend,'trend').totalEntryCost-.45)<1e-9);
  assert.equal(result(trend,'reversion').signals,0);

  const falling=[book(0,.42,.44),book(1_000,.42,.44),book(2_000,.41,.43),book(3_000,.39,.41),
    book(4_000,.40,.42),book(5_000,.44,.46)];
  const reversion=analyzeExecutableTrendReversionV1({observations:falling,splitAt:at(3_000),options:options()});
  assert.equal(result(reversion,'reversion').signals,1);
  assert.equal(result(reversion,'reversion').completed,1);
  assert.ok(Math.abs(result(reversion,'reversion').totalNetPnlOneShare-.02)<1e-9);
  assert.equal(result(reversion,'trend').signals,0);
});

test('entry waits for the declared decision latency and exit waits for the full hold',()=>{
  const rows=[book(0,.38,.40),book(1_000,.39,.41),book(2_000,.40,.42),book(3_000,.42,.44),
    book(3_500,.44,.46),book(4_000,.58,.60),book(5_000,.63,.65),book(6_000,.65,.67)];
  const report=analyzeExecutableTrendReversionV1({observations:rows,splitAt:at(3_000),options:options({decisionLatencyMs:1_000})});
  const trend=result(report,'trend');
  assert.equal(trend.completed,1);
  assert.ok(Math.abs(trend.totalNetPnlOneShare-.03)<1e-9);
  assert.ok(Math.abs(trend.totalEntryCost-.60)<1e-9);
});

test('unconditional control pays the complete observed spread even with zero fees',()=>{
  const rows=[book(0,.40,.42),book(1_000,.40,.42),book(2_000,.40,.42),book(3_000,.40,.42),
    book(4_000,.40,.42),book(5_000,.40,.42)];
  const report=analyzeExecutableTrendReversionV1({observations:rows,splitAt:at(3_000),options:options()});
  assert.equal(result(report,'unconditional').completed,1);
  assert.ok(Math.abs(result(report,'unconditional').totalNetPnlOneShare+.02)<1e-9);
  assert.equal(report.methodology.fees,'0.000000');
  for(const row of report.results){
    const byMarketPnl=row.byMarket.reduce((sum,item)=>sum+item.netPnlOneShare,0);
    const byDayPnl=row.byUtcDay.reduce((sum,item)=>sum+item.netPnlOneShare,0);
    const byMarketCost=row.byMarket.reduce((sum,item)=>sum+item.entryCost,0);
    const byDayCost=row.byUtcDay.reduce((sum,item)=>sum+item.entryCost,0);
    assert.ok(Math.abs(byMarketPnl-row.totalNetPnlOneShare)<1e-9);
    assert.ok(Math.abs(byDayPnl-row.totalNetPnlOneShare)<1e-9);
    assert.ok(Math.abs(byMarketCost-row.totalEntryCost)<1e-9);
    assert.ok(Math.abs(byDayCost-row.totalEntryCost)<1e-9);
  }
});

test('new session or invalidation segment censors a position instead of bridging the gap',()=>{
  const rows=[book(0,.38,.40),book(1_000,.39,.41),book(2_000,.40,.42),book(3_000,.42,.44),
    book(4_000,.43,.45,{segment:'2:0'}),book(5_000,.46,.48,{segment:'2:0'})];
  const report=analyzeExecutableTrendReversionV1({observations:rows,splitAt:at(3_000),options:options()});
  const trend=result(report,'trend');
  assert.equal(trend.completed,0);
  assert.equal(trend.censoring.invalidation,1);
});

test('development labels are purged for the full feature and maximum holding window',()=>{
  const rows=Array.from({length:12},(_,i)=>book(i*1_000,.40+i*.001,.42+i*.001));
  const report=analyzeExecutableTrendReversionV1({observations:rows,splitAt:at(6_000),options:options({holdingMs:[1_000,2_000]})});
  assert.ok(report.observations.decisionsByPeriod.purged>0);
  assert.ok(report.results.filter(x=>x.period==='purged').every(x=>x.signals===0&&x.completed===0));
  assert.equal(report.methodology.purgeMs,4_000);
});

test('stale and duplicate sequence observations are counted and excluded',()=>{
  const rows=[book(0,.38,.40),book(1_000,.39,.41),book(2_000,.40,.42),book(3_000,.42,.44),
    book(3_500,.01,.99,{sequence:'3501'}),book(4_000,.43,.45,{sequence:'3501'}),book(5_000,.46,.48,{sequence:'5001',sourceMs:-1_000})];
  const report=analyzeExecutableTrendReversionV1({observations:rows,splitAt:at(3_000),options:options()});
  assert.equal(report.observations.rejected.duplicateSequence,1);
  assert.equal(report.observations.rejected.stale,1);
  assert.equal(report.observations.rejected.sourceTooOld,1);
  assert.equal(report.observations.rejected.sourceTooFarFuture,0);
  assert.equal(report.observations.acceptedSourceAgeMs.count,report.observations.acceptedUniqueBooks);
  assert.equal(report.observations.acceptedSourceAgeMs.p50,0);
  assert.equal(report.observations.topOfBookQuantity.bid.atLeast1,report.observations.acceptedUniqueBooks);
  assert.equal(report.observations.topOfBookQuantity.ask.atLeast1,report.observations.acceptedUniqueBooks);
  assert.ok(result(report,'trend').totalNetPnlOneShare<.1);
});

test('out-of-order sources, invalid durations and missing split fail closed',()=>{
  assert.throws(()=>analyzeExecutableTrendReversionV1({observations:[book(1_000,.4,.42),book(0,.4,.42)],splitAt:at(2_000)}));
  assert.throws(()=>analyzeExecutableTrendReversionV1({observations:[],splitAt:at(0),options:{holdingMs:[0]}}));
  assert.throws(()=>analyzeExecutableTrendReversionV1({observations:[]}));
});
