import test from 'node:test';
import assert from 'node:assert/strict';
import {
  createRealtimeOfiDiagnosticV1,
  topOfBookSignalsV1,
} from './realtime-ofi-diagnostic-v1.mjs';

const origin = Date.parse('2026-01-01T00:00:00.000Z');
const iso = ms => new Date(origin + ms).toISOString();

function bookRecord(index, ms, { bid = 0.5, ask = 0.51, bidQuantity = 10, askQuantity = 10 } = {}) {
  return {
    format: 'sig-feed-capture',
    schemaVersion: 1,
    captureIndex: index,
    observedAt: iso(ms),
    receivedAt: iso(ms),
    kind: 'event',
    event: 'market_batch',
    topic: 'tournament:bda92870-621e-47b0-bc3c-3602c5c26f55:market:7',
    epoch: 1,
    payload: {
      resyncRequired: false,
      marketSettled: [],
      delivery: { model: 'best-effort-authoritative-resync', revision: index + 1, previousRevision: index },
      books: [{
        exchangeId: 'exchange-7',
        asOf: { sequence: index + 1, at: iso(ms) },
        nextExpiryAt: null,
        expiryKnown: true,
        bids: [{ price: bid, quantity: bidQuantity }],
        asks: [{ price: ask, quantity: askQuantity }],
      }],
      bookDirty: [],
    },
  };
}

test('top-of-book imbalance is causal, and a future book labels only after the target time', () => {
  const previous = {
    bidTick: 100,
    askTick: 102,
    bids: [{ quantity: 10 }],
    asks: [{ quantity: 10 }],
  };
  const current = {
    bidTick: 100,
    askTick: 102,
    bids: [{ quantity: 20 }],
    asks: [{ quantity: 10 }],
  };
  const signal = topOfBookSignalsV1(current, previous);
  assert.equal(signal.queueImbalanceTop, 1 / 3);
  assert.equal(signal.normalizedBboOfi, 0.2);
  assert.equal(topOfBookSignalsV1(previous).normalizedBboOfi, null);

  const study = createRealtimeOfiDiagnosticV1({
    splitIndex: 2,
    totalRecords: 5,
    throughAt: iso(3_100),
    labelGraceMs: 100,
  });
  study.consume(bookRecord(0, 0, { bidQuantity: 10, askQuantity: 10 }));
  study.consume(bookRecord(1, 1_000, { bidQuantity: 20, askQuantity: 10 }));
  study.consume(bookRecord(2, 2_000, { bid: 0.5, ask: 0.51, bidQuantity: 30, askQuantity: 10 }));
  study.consume(bookRecord(3, 2_500, { bid: 0.5, ask: 0.51, bidQuantity: 35, askQuantity: 10 }));
  assert.equal(study.snapshot().queue1s.labels, 0, 'a pre-target quote cannot label the signal');
  study.consume(bookRecord(4, 3_100, { bid: 0.51, ask: 0.52, bidQuantity: 40, askQuantity: 10 }));
  const report = study.report();
  const oneSecond = report.results.queueImbalanceTop[1_000];
  assert.equal(oneSecond.allHoldoutSignals.opportunities, 1);
  assert.equal(oneSecond.allHoldoutSignals.labeled, 1);
  assert.equal(oneSecond.thresholdHoldoutSignals.labeled, 1);
  assert.equal(oneSecond.thresholdLabeledOutcomes.meanDirectionalMarkoutTicks, 2);
  assert.equal(oneSecond.thresholdLabeledOutcomes.adverseMarkoutTicks, -2);
  assert.equal(report.protocol.split.noLabelMayCrossSplit, true);
});

test('invalidation censors queued labels instead of carrying them across a gap', () => {
  const study = createRealtimeOfiDiagnosticV1({ splitIndex: 1, totalRecords: 4, throughAt: iso(1_500), labelGraceMs: 500 });
  study.consume(bookRecord(0, 0, { bidQuantity: 10, askQuantity: 10 }));
  study.consume(bookRecord(1, 0, { bidQuantity: 20, askQuantity: 10 }));
  study.consume({
    format: 'sig-feed-capture', schemaVersion: 1, captureIndex: 2, observedAt: iso(500),
    kind: 'invalidation', topic: 'tournament:bda92870-621e-47b0-bc3c-3602c5c26f55:market:7',
  });
  study.consume(bookRecord(3, 1_500, { bid: 0.51, ask: 0.52, bidQuantity: 30, askQuantity: 10 }));
  const report = study.report();
  const oneSecond = report.results.queueImbalanceTop[1_000];
  assert.equal(oneSecond.allHoldoutSignals.opportunities, 1);
  assert.equal(oneSecond.allHoldoutSignals.labeled, 0);
  assert.equal(oneSecond.allHoldoutSignals.boundaryCensored, 1);
});

