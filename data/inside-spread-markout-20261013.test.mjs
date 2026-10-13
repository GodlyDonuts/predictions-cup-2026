import test from 'node:test';
import assert from 'node:assert/strict';
import {
  createSpreadPolicyReplay,
  combineStudyCells,
  insideLimitTicks,
  strictThrough,
  summarizeCell,
} from './run-inside-spread-markout-20261013.mjs';

const base = Date.parse('2026-10-13T10:00:00.000Z');
const at = ms => new Date(base + ms).toISOString();
const book = (ms, bidTick = 100, askTick = 102, marketId = 'M1', exchangeId = `E-${marketId}`) => ({
  marketId,
  exchangeId,
  sequence: String(ms + 1),
  sourceAt: at(ms),
  receivedAt: at(ms),
  bids: [{ price: (bidTick * 0.005).toFixed(3), quantity: 100 }],
  asks: [{ price: (askTick * 0.005).toFixed(3), quantity: 100 }],
});
const trade = (ms, tick, id = ms, marketId = 'M1', exchangeId = `E-${marketId}`) => ({
  marketId,
  exchangeId,
  id: String(id),
  sequence: id,
  price: (tick * 0.005).toFixed(3),
  quantity: 1,
  executedAt: at(ms),
});

test('fixed inside policy moves exactly one tick only when spread is at least two ticks', () => {
  assert.deepEqual(insideLimitTicks({ bidTick: 100, askTick: 102, policy: 'atBest' }), { bidTick: 100, askTick: 102, spreadTicks: 2 });
  assert.deepEqual(insideLimitTicks({ bidTick: 100, askTick: 102, policy: 'insideOneTick' }), { bidTick: 101, askTick: 101, spreadTicks: 2 });
  assert.deepEqual(insideLimitTicks({ bidTick: 100, askTick: 103, policy: 'insideOneTick' }), { bidTick: 101, askTick: 102, spreadTicks: 3 });
  assert.equal(insideLimitTicks({ bidTick: 100, askTick: 101, policy: 'insideOneTick' }), null);
  assert.throws(() => insideLimitTicks({ bidTick: -1, askTick: 2, policy: 'atBest' }), /INSIDE_SPREAD_BOOK/);
  assert.throws(() => insideLimitTicks({ bidTick: 199, askTick: 201, policy: 'atBest' }), /INSIDE_SPREAD_BOOK/);
  assert.throws(() => insideLimitTicks({ bidTick: 100, askTick: 102, policy: 'adaptive' }), /INSIDE_SPREAD_POLICY/);
});

test('a proxy requires a full tick through the limit, not merely a trade at the quote', () => {
  assert.equal(strictThrough({ side: 'buy', tradeTick: 101, limitTick: 101 }), false);
  assert.equal(strictThrough({ side: 'buy', tradeTick: 100, limitTick: 101 }), true);
  assert.equal(strictThrough({ side: 'buy', tradeTick: 102, limitTick: 101 }), false);
  assert.equal(strictThrough({ side: 'sell', tradeTick: 101, limitTick: 101 }), false);
  assert.equal(strictThrough({ side: 'sell', tradeTick: 102, limitTick: 101 }), true);
});

test('matched two-tick opportunity separates baseline and one-tick-inside proxy fills', () => {
  const baseline = createSpreadPolicyReplay({ policy: 'atBest', fold: 'heldOut' });
  const inside = createSpreadPolicyReplay({ policy: 'insideOneTick', fold: 'heldOut' });
  const decision = book(0);
  baseline.observeBook(decision, at(0));
  inside.observeBook(decision, at(0));

  // At 4s the print is one tick through the inside bid but only at the old best bid.
  const print = trade(4000, 100, 1);
  baseline.observeTrade(print, at(5000));
  inside.observeTrade(print, at(5000));
  const baselineReport = baseline.report({ throughAt: at(6000) });
  const insideReport = inside.report({ throughAt: at(6000) });
  assert.equal(baselineReport.counts.strictThroughProxies, 0);
  assert.equal(insideReport.counts.strictThroughProxies, 1);
  const insideBuy = insideReport.cells.find(x => x.fold === 'heldOut' && x.side === 'buy' && x.horizonMs === 1000);
  assert.equal(insideBuy.eligibleCandidateQuotes, 1);
  assert.equal(insideBuy.strictThroughProxyFills, 1);
  assert.equal(insideBuy.markedProxyFills, 0);
});

test('markout waits for both event horizon and three seconds after local print receipt', () => {
  const replay = createSpreadPolicyReplay({ policy: 'insideOneTick', fold: 'heldOut' });
  replay.observeBook(book(0), at(0));
  replay.observeTrade(trade(4000, 100, 2), at(5000));

  replay.observeBook(book(7000, 101, 103), at(7000));
  replay.observeBook({ ...book(8000, 101, 103), sourceAt: at(7800), receivedAt: at(7900) }, at(8000));
  let report = replay.report({ throughAt: at(8000) });
  let buy = report.cells.find(x => x.fold === 'heldOut' && x.policy === 'insideOneTick' && x.side === 'buy' && x.horizonMs === 1000);
  assert.equal(buy.markedProxyFills, 0, 'too-early receipt and then pre-response receipt are ineligible');

  // A fresh point at/after event time with receipt after report + response becomes executable.
  const fresh = { ...book(8100, 102, 104), receivedAt: at(8100) };
  replay.observeBook(fresh, at(8200));
  report = replay.report({ throughAt: at(8200) });
  buy = report.cells.find(x => x.fold === 'heldOut' && x.policy === 'insideOneTick' && x.side === 'buy' && x.horizonMs === 1000);
  assert.equal(buy.markedProxyFills, 1);
  assert.equal(buy.meanExecutableNetPerShareZeroFee, '0.005000');
  assert.equal(buy.meanOneTickEachWayStressPerShare, '-0.005000');
});

test('source boundary cancels an unfilled quote before later prints can count', () => {
  const replay = createSpreadPolicyReplay({ policy: 'insideOneTick', fold: 'heldOut' });
  replay.observeBook(book(0), at(0));
  replay.boundary('E-M1', at(2000));
  replay.observeTrade(trade(4000, 100, 3), at(5000));
  const report = replay.report({ throughAt: at(6000) });
  assert.equal(report.counts.boundaryCancelledQuotes, 2);
  assert.equal(report.counts.strictThroughProxies, 0);
});

test('development cutoff excludes late fill receipts and post-cutoff labels', () => {
  const replay = createSpreadPolicyReplay({ policy: 'insideOneTick', fold: 'development', decisionCutoffAt: at(15000) });
  replay.observeBook(book(0), at(0));
  replay.observeTrade(trade(10000, 100, 4), at(15000));
  const report = replay.report({ throughAt: at(20000) });
  assert.equal(report.counts.strictThroughProxies, 0);
  assert.equal(report.cells.find(x => x.fold === 'development' && x.side === 'buy' && x.horizonMs === 1000).eligibleCandidateQuotes, 1);
});

test('development cutoff censors a pre-cutoff fill when exit source, receipt, or observation reaches cutoff', () => {
  const cutoffAt = at(15000);
  const exitCases = [
    { sourceAt: at(10000), receivedAt: cutoffAt, observedAt: at(16000) },
    { sourceAt: cutoffAt, receivedAt: at(14000), observedAt: at(16000) },
    { sourceAt: at(10000), receivedAt: at(14000), observedAt: cutoffAt },
  ];

  for (const [index, exit] of exitCases.entries()) {
    const replay = createSpreadPolicyReplay({ policy: 'insideOneTick', fold: 'development', decisionCutoffAt: cutoffAt });
    replay.observeBook(book(0), at(0));
    replay.observeTrade(trade(4000, 100, 20 + index), at(5000));
    replay.observeBook({ ...book(10000, 102, 104), sourceAt: exit.sourceAt, receivedAt: exit.receivedAt }, exit.observedAt);

    const report = replay.report({ throughAt: at(17000) });
    const buy = report.cells.find(x => x.fold === 'development' && x.policy === 'insideOneTick' && x.side === 'buy' && x.horizonMs === 1000);
    assert.equal(report.fillProxies[0].reportReceivedAt, at(5000), 'the proxy fill was reported before the cutoff');
    assert.equal(report.counts.strictThroughProxies, 1);
    assert.equal(buy.markedProxyFills, 0, `exit case ${index} must not provide a post-cutoff development label`);
    assert.equal(buy.unresolvedOrCensoredMarkouts, 1);
  }
});

test('all 24 prespecified strategy, fold, side, and horizon cells are present even when empty', () => {
  const build = (policy, fold) => createSpreadPolicyReplay({ policy, fold }).report({ throughAt: at(0) });
  const cells = combineStudyCells({
    development: { atBest: build('atBest', 'development'), insideOneTick: build('insideOneTick', 'development') },
    heldOut: { atBest: build('atBest', 'heldOut'), insideOneTick: build('insideOneTick', 'heldOut') },
  });
  assert.equal(cells.length, 24);
  assert.equal(new Set(cells.map(x => `${x.fold}|${x.policy}|${x.side}|${x.horizonMs}`)).size, 24);
  assert.deepEqual([...new Set(cells.map(x => x.fold))].sort(), ['development', 'heldOut']);
  assert.deepEqual([...new Set(cells.map(x => x.policy))].sort(), ['atBest', 'insideOneTick']);
  assert.deepEqual([...new Set(cells.map(x => x.side))].sort(), ['buy', 'sell']);
  assert.deepEqual([...new Set(cells.map(x => x.horizonMs))].sort((a, b) => a - b), [1000, 5000, 30000]);
  assert.equal(cells.find(x => x.fold === 'development').marketClusterUncertainty.effectiveIndependentMarketCount, 0);
  assert.throws(() => combineStudyCells({ development: { atBest: build('atBest', 'development') }, heldOut: {} }), /INSIDE_SPREAD_CELL_COVERAGE/);
});

test('clustered uncertainty uses independent market-level means, not quote count', () => {
  const candidates = [
    { fold: 'heldOut', policy: 'insideOneTick', side: 'buy', marketId: 'M1', activeDurationNs: 1n },
    { fold: 'heldOut', policy: 'insideOneTick', side: 'buy', marketId: 'M1', activeDurationNs: 1n },
    { fold: 'heldOut', policy: 'insideOneTick', side: 'buy', marketId: 'M2', activeDurationNs: 1n },
  ];
  const fills = [
    { fold: 'heldOut', policy: 'insideOneTick', side: 'buy', marketId: 'M1', marks: { 1000: { netMicros: 5000n } } },
    { fold: 'heldOut', policy: 'insideOneTick', side: 'buy', marketId: 'M1', marks: { 1000: { netMicros: 5000n } } },
    { fold: 'heldOut', policy: 'insideOneTick', side: 'buy', marketId: 'M2', marks: { 1000: { netMicros: -5000n } } },
  ];
  const cell = summarizeCell({ candidates, fills, horizonMs: 1000, fold: 'heldOut', policy: 'insideOneTick', side: 'buy' });
  assert.equal(cell.markedProxyFills, 3);
  assert.equal(cell.marketClusterUncertainty.effectiveIndependentMarketCount, 2);
  assert.equal(cell.marketClusterUncertainty.equalWeightedMarketMeanNet, '0.000000');
  assert.deepEqual(cell.marketClusterUncertainty.marketClusterCi95StudentT, ['-0.063530', '0.063530']);
  assert.match(cell.marketClusterUncertainty.uncertaintyNote, /few markets/);
});
