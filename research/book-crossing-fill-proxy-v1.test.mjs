import test from 'node:test';
import assert from 'node:assert/strict';
import { createBookCrossingFillProxyV1 } from './book-crossing-fill-proxy-v1.mjs';

const origin = Date.parse('2026-10-13T21:30:00.000Z');
const at = ms => new Date(origin + ms).toISOString();
const scope = { tournamentId: 'T', exchanges: [{ marketId: 'M', exchangeId: 'E' }] };
const book = (ms, sequence, bid = '0.500', ask = '0.510', overrides = {}) => ({
  tournamentId: 'T', marketId: 'M', exchangeId: 'E', sequence: String(sequence),
  sourceAt: at(ms), requestStartedAt: at(ms), receivedAt: at(ms),
  bids: [{ price: bid, quantity: 10 }], asks: [{ price: ask, quantity: 12 }],
  origin: 'scoped_sequenced_engine_book_point_slice',
  ...overrides,
});
const trade = (ms, id, price) => ({
  tournamentId: 'T', marketId: 'M', exchangeId: 'E', id: String(id), sequence: id,
  executedAt: at(ms), price, quantity: 1,
});
const study = options => createBookCrossingFillProxyV1({ scope, ...options });

test('fresh scoped sequenced depletion is only an unconfirmed proxy; tape corroboration and report lag stay separate', () => {
  const s = study();
  s.observeBatch({ observedAt: at(0), books: [book(0, 1)] });
  s.observeBatch({ observedAt: at(4000), books: [book(4000, 2, '0.495', '0.515')] });
  let r = s.report({ splitAt: at(100000), purgeMs: 0 });
  assert.equal(r.proxyEvidence.bookCrossingProxies, 2);
  assert.equal(r.proxyEvidence.unconfirmedBookCrossings, 2);
  assert.equal(r.proxyEvidence.tapeCorroboratedProxies, 0);
  assert.equal(r.actualFillsProven, false);
  assert.equal(r.profitabilityEstablished, false);

  s.observeBatch({ observedAt: at(5500), receivedAt: at(5500), trades: [trade(4500, 7, '0.495')] });
  r = s.report({ splitAt: at(100000), purgeMs: 0 });
  assert.equal(r.proxyEvidence.tapeCorroboratedProxies, 1);
  assert.equal(r.proxyEvidence.unconfirmedBookCrossings, 1);
  assert.equal(r.proxyEvidence.tapeReportLagMs.samples, 1);
  assert.equal(r.proxyEvidence.tapeReportLagMs.p50, 1000);
  assert.equal(r.proxyEvidence.localBookObservationLeadMs.p50, 1500);
  assert.equal(r.counts.invalidTradeReports, 0);
});

test('pre-activation and post-expiry movements cannot proxy a fill', () => {
  const s = study();
  s.observeBatch({ observedAt: at(0), books: [book(0, 1)] });
  s.observeBatch({ observedAt: at(2000), books: [book(2000, 2, '0.495', '0.515')] });
  s.observeBatch({ observedAt: at(17000), books: [book(17000, 3, '0.490', '0.520')] });
  const r = s.report({ splitAt: at(100000), purgeMs: 0 });
  assert.equal(r.proxyEvidence.bookCrossingProxies, 0);
  assert.equal(r.proxyEvidence.expiredWithoutCrossing, 2);
});

test('wrong scope, stale sequence and invalidation cannot create a crossing', () => {
  const s = study();
  s.observeBatch({ observedAt: at(0), books: [book(0, 1)] });
  s.observeBatch({ observedAt: at(4000), books: [book(4000, 1, '0.495', '0.515')] });
  s.observeBatch({ observedAt: at(5000), books: [book(5000, 2, '0.495', '0.515', { marketId: 'OTHER' })] });
  s.boundary('E', 'DISCONNECTED', at(6000));
  const r = s.report({ splitAt: at(100000), purgeMs: 0 });
  assert.equal(r.proxyEvidence.bookCrossingProxies, 0);
  assert.equal(r.proxyEvidence.invalidatedBeforeCrossing, 2);
  assert.equal(r.counts.rejectedSequence, 1);
  assert.equal(r.counts.rejectedScope, 1);
});

test('executable markouts require event horizon and local receipt plus response latency', () => {
  const s = study();
  s.observeBatch({ observedAt: at(0), books: [book(0, 1)] });
  s.observeBatch({ observedAt: at(4000), books: [book(4000, 2, '0.495', '0.515')] });
  // Event-time horizon is already available, but receipt is one millisecond early.
  s.observeBatch({ observedAt: at(6999), books: [book(6999, 3, '0.495', '0.505')] });
  let r = s.report({ splitAt: at(100000), purgeMs: 0 });
  const buy = r.folds[0].actions.find(row => row.action === 'buy');
  assert.equal(buy.horizons[0].executable.samples, 0);

  // Fresh local receipt at crossing receipt + 3s qualifies the 1s markout.
  s.observeBatch({ observedAt: at(7000), books: [book(7000, 4, '0.495', '0.505')] });
  // The 5s mark requires source time >= 9s, even though response time has passed.
  s.observeBatch({ observedAt: at(8999), books: [book(8999, 5, '0.495', '0.505')] });
  r = s.report({ splitAt: at(100000), purgeMs: 0 });
  let horizons = r.folds[0].actions.find(row => row.action === 'buy').horizons;
  assert.equal(horizons[0].executable.samples, 1);
  assert.equal(horizons[0].executable.meanExecutableNet, '-0.005000');
  assert.equal(horizons[0].executable.oneTickEachWayStressMean, '-0.015000');
  assert.equal(horizons[1].executable.samples, 0);

  s.observeBatch({ observedAt: at(9000), books: [book(9000, 6, '0.495', '0.505')] });
  r = s.report({ splitAt: at(100000), purgeMs: 0 });
  horizons = r.folds[0].actions.find(row => row.action === 'buy').horizons;
  assert.equal(horizons[1].executable.samples, 1);
  assert.equal(horizons[2].executable.samples, 0);
});

test('development labels arriving at or after the purged cutoff are excluded', () => {
  const s = study();
  s.observeBatch({ observedAt: at(0), books: [book(0, 1)] });
  s.observeBatch({ observedAt: at(4000), books: [book(4000, 2, '0.495', '0.515')] });
  s.observeBatch({ observedAt: at(7000), books: [book(7000, 3, '0.495', '0.505')] });
  const r = s.report({ splitAt: at(8000), purgeMs: 1000 });
  const buy = r.folds[0].actions.find(row => row.action === 'buy');
  assert.equal(buy.horizons[0].executable.samples, 0);
});
