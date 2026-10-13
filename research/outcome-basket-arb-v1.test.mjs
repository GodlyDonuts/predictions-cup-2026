import test from 'node:test';
import assert from 'node:assert/strict';
import {analyzeOutcomeBasketV1 as analyze} from './outcome-basket-arb-v1.mjs';

// These tiny constructed books verify arithmetic, coverage, and censoring
// rules only; they are not market observations or profitability evidence.
const hash = 'a'.repeat(64);
const time = ms => new Date(Date.parse('2026-10-13T12:00:00.000Z') + ms).toISOString();
function basket(ids = ['A', 'B']) {
  return {basketId: 'fixture-only', payoutPerWinningYes: '1.000000',
    settlementProof: {status: 'settlement_verified', mutuallyExclusive: true, exhaustive: true,
      payoutPerWinningYes: '1.000000', sourceUrl: 'https://example.invalid/rules', sourceSha256: hash,
      rulesSha256: hash, coveredOutcomeIds: ids},
    outcomes: ids.map((outcomeId, i) => ({outcomeId, marketId: String(i + 1), exchangeId: String(101 + i), contractRootSha256: hash}))};
}
function book(outcome, at, asks, sequence = Math.floor(at / 250) + 1) {
  const i = outcome.charCodeAt(0) - 65;
  return {marketId: String(i + 1), exchangeId: String(101 + i), sequence: String(sequence),
    availableAt: time(at), sourceAt: time(at), asks: asks.map(([price, quantity]) => ({price, quantity}))};
}

test('complete YES set sweeps all visible depth at zero fees and applies one tick per outcome', () => {
  const b = basket(['A', 'B', 'C']);
  const observations = [
    book('A', 0, [['0.300', 2], ['0.305', 3]]),
    book('B', 0, [['0.300', 1], ['0.305', 4]]),
    book('C', 0, [['0.385', 5]]),
  ];
  const r = analyze({basket: b, observations, options:{latenciesMs:[0], decisionSpacingMs:0}});
  const s = r.scenarios[0];
  assert.equal(r.fees, '0.000000');
  assert.equal(s.visibleDepthCandidates, 1);
  assert.equal(s.quantityAcrossCandidates, '5');
  assert.equal(s.netSurplusAcrossCandidates, '0.040000');
  assert.equal(s.stressedNetSurplusAcrossCandidates, '-0.035000');
  assert.equal(s.positiveAtObservedAsks, 1);
  assert.equal(s.positiveAfterOneTickStress, 0);
  assert.equal(r.actualFills, 0);
  assert.equal(r.profitabilityEstablished, false);
});

test('all four latency scenarios use later chronological books rather than reusing the decision quote', () => {
  const observations = [];
  for (const ms of [0, 250, 1000, 3000]) {
    observations.push(book('A', ms, [['0.490', 100]], ms / 250 + 1));
    const secondAsk = ms === 0 ? '0.500' : ms === 250 ? '0.510' : ms === 1000 ? '0.525' : '0.550';
    observations.push(book('B', ms, [[secondAsk, 100]], ms / 250 + 1));
  }
  const r = analyze({basket: basket(), observations, options:{decisionSpacingMs:10000}});
  assert.deepEqual(r.latenciesMs, [0, 250, 1000, 3000]);
  assert.equal(r.scenarios[0].positiveAtObservedAsks, 1);
  assert.equal(r.scenarios[0].positiveAfterOneTickStress, 0, 'one tick on each leg consumes the 1-cent edge');
  assert.equal(r.scenarios[1].positiveAtObservedAsks, 0, '250ms sees the later 1.000 package');
  assert.equal(r.scenarios[2].positiveAtObservedAsks, 0);
  assert.equal(r.scenarios[3].positiveAtObservedAsks, 0);
  assert.equal(r.scenarios[3].visibleDepthCandidates, 1);
});

test('proof must cover every unique outcome and explicitly assert both settlement properties', () => {
  const b = basket(['A', 'B', 'C']);
  b.settlementProof.coveredOutcomeIds = ['A', 'B'];
  assert.throws(() => analyze({basket: b, observations: []}), /OUTCOME_BASKET_INCOMPLETE_PROOF/);
  const c = basket(); c.settlementProof.exhaustive = false;
  assert.throws(() => analyze({basket: c, observations: []}), /OUTCOME_BASKET_SETTLEMENT_PROOF/);
  const d = basket(['A', 'A']);
  assert.throws(() => analyze({basket: d, observations: []}), /OUTCOME_BASKET_DUPLICATE/);
});

test('a missing outcome book, excessive asynchronous skew, or intervening break censors the package', () => {
  const b = basket(['A', 'B', 'C']);
  const partial = analyze({basket: b, observations:[book('A',0,[['0.20',10]]),book('B',0,[['0.20',10]])], options:{latenciesMs:[0],decisionSpacingMs:0}});
  assert.equal(partial.scenarios[0].visibleDepthCandidates, 0);
  assert.ok(partial.scenarios[0].censoredMissingDecisionBook > 0);
  const skew = analyze({basket: basket(), observations:[book('A',0,[['0.40',10]]),book('B',2000,[['0.40',10]])], options:{latenciesMs:[0],decisionSpacingMs:0,maxOutcomeBookSkewMs:100}});
  assert.equal(skew.scenarios[0].visibleDepthCandidates, 0);
  assert.ok(skew.scenarios[0].censoredDecisionSkew > 0);
  const interrupted = analyze({basket:basket(), observations:[book('A',0,[['0.40',10]]),book('B',0,[['0.40',10]]),book('A',250,[['0.40',10]],2),book('B',250,[['0.40',10]],2)], breaks:[{marketId:'2',at:time(100)}], options:{latenciesMs:[250],decisionSpacingMs:10000}});
  assert.equal(interrupted.scenarios[0].visibleDepthCandidates, 0);
  assert.equal(interrupted.scenarios[0].censoredBreak, 1);
});

test('asks with duplicate levels, stale source clocks, and unsorted replay points are rejected', () => {
  const b = basket();
  assert.throws(() => analyze({basket:b, observations:[book('A',0,[['0.400',1],['0.400',2]])]}), /OUTCOME_BOOK_ASK_LEVEL/);
  const stale = book('A',0,[['0.400',1]]); stale.sourceAt = time(-6000);
  assert.throws(() => analyze({basket:b, observations:[stale]}), /OUTCOME_BOOK_STALE/);
  assert.throws(() => analyze({basket:b, observations:[book('A',250,[['0.4' + '00',1]],2),book('A',0,[['0.400',1]],1)]}), /OUTCOME_INPUT_ORDER/);
});

