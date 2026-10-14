import test from 'node:test';
import assert from 'node:assert/strict';
import {
  binaryTerminalInventoryPenalty,
  complementaryBinaryOrder,
  selectBinaryInventoryMakerQuotes,
} from './binary-inventory-maker-v1.mjs';

const certificate = {
  status: 'validated',
  source: 'own_fill_receipts',
  evidenceId: 'frozen-own-fill-calibration',
  modelHash: 'a'.repeat(64),
  minimumClustersRequired: 32,
  independentClusters: 32,
  cutoffUtc: '2026-10-14T00:00:00.000Z',
};

function fixture(overrides = {}) {
  return {
    probability: 0.5,
    remainingSeconds: 3600,
    inventory: 0,
    maxInventory: 3,
    lotSize: 1,
    tick: 0.1,
    book: {bestBid: 0.4, bestAsk: 0.6, coherent: true, complete: true, ageMs: 0, sequence: 1},
    maxBookAgeMs: 1000,
    continuationValue: () => 0,
    fillModel: {
      calibration: certificate,
      ratePerSecond: ({side, quotePrice}) => side === 'buy'
        ? Math.exp(10 * (quotePrice - 0.4))
        : Math.exp(10 * (0.6 - quotePrice)),
    },
    ...overrides,
  };
}

test('terminal penalty is the binary settlement variance and peaks at a fair coin', () => {
  assert.equal(binaryTerminalInventoryPenalty(0, 5, 2), 0);
  assert.equal(binaryTerminalInventoryPenalty(1, -5, 2), 0);
  assert.equal(binaryTerminalInventoryPenalty(0.5, 2, 2), -2);
  assert.equal(binaryTerminalInventoryPenalty(0.25, 2, 2), -1.5);
  assert.throws(() => binaryTerminalInventoryPenalty(1.1, 1, 1), /BINARY_MAKER_TERMINAL_INPUT/);
});

test('zero-inventory symmetric model selects symmetric non-crossing quotes', () => {
  const result = selectBinaryInventoryMakerQuotes(fixture());
  assert.equal(result.researchOnly, true);
  assert.equal(result.executionEligible, false);
  assert.equal(result.quotes.buy.price, 0.4);
  assert.equal(result.quotes.sell.price, 0.6);
  assert.ok(result.quotes.buy.price < 0.6);
  assert.ok(result.quotes.sell.price > 0.4);
});

test('selector equals brute-force maximization of the finite HJB objective', () => {
  const input = fixture();
  const result = selectBinaryInventoryMakerQuotes(input);
  const scores = [];
  for (const price of [0.1, 0.2, 0.3, 0.4, 0.5]) {
    if (price >= input.book.bestAsk) continue;
    const rate = input.fillModel.ratePerSecond({side:'buy', probability:0.5, quotePrice:price,
      remainingSeconds:3600, inventory:0, nextInventory:1, lotSize:1, book:input.book});
    scores.push({price, score:rate * (0.5 - price)});
  }
  const expected = scores.reduce((best, item) => item.score > best.score ? item : best);
  assert.equal(result.quotes.buy.price, expected.price);
  assert.equal(result.quotes.buy.expectedValuePerSecond, expected.score);
});

test('binary terminal value skews quotes to reduce existing signed inventory', () => {
  const continuationValue = ({probability, inventory}) =>
    binaryTerminalInventoryPenalty(probability, inventory, 0.2);
  const flat = selectBinaryInventoryMakerQuotes(fixture({continuationValue}));
  const long = selectBinaryInventoryMakerQuotes(fixture({inventory:1, continuationValue}));
  const short = selectBinaryInventoryMakerQuotes(fixture({inventory:-1, continuationValue}));
  assert.ok(long.quotes.buy.price < flat.quotes.buy.price);
  assert.ok(long.quotes.sell.price < flat.quotes.sell.price);
  assert.ok(short.quotes.buy.price > flat.quotes.buy.price);
  assert.ok(short.quotes.sell.price > flat.quotes.sell.price);
});

test('inventory bounds suppress only the direction that would add exposure', () => {
  const longLimit = selectBinaryInventoryMakerQuotes(fixture({inventory:3}));
  const shortLimit = selectBinaryInventoryMakerQuotes(fixture({inventory:-3}));
  assert.equal(longLimit.quotes.buy, null);
  assert.ok(longLimit.quotes.sell);
  assert.equal(shortLimit.quotes.sell, null);
  assert.ok(shortLimit.quotes.buy);
});

test('complement mapping preserves binary payoff and reverses the action', () => {
  assert.deepEqual(complementaryBinaryOrder('buy', 0.4, 0.1), {outcome:'no',action:'sell',price:0.6});
  assert.deepEqual(complementaryBinaryOrder('sell', 0.6, 0.1), {outcome:'no',action:'buy',price:0.4});
});

test('stale, incoherent, uncalibrated, invalid-rate or non-crossable books fail closed', () => {
  assert.equal(selectBinaryInventoryMakerQuotes(fixture({book:{...fixture().book,ageMs:1001}})), null);
  assert.equal(selectBinaryInventoryMakerQuotes(fixture({book:{...fixture().book,complete:false}})), null);
  assert.equal(selectBinaryInventoryMakerQuotes(fixture({fillModel:{ratePerSecond:()=>1}})), null);
  assert.equal(selectBinaryInventoryMakerQuotes(fixture({fillModel:{calibration:certificate,ratePerSecond:()=>NaN}})), null);
  assert.equal(selectBinaryInventoryMakerQuotes(fixture({fillModel:{calibration:{...certificate,independentClusters:31},ratePerSecond:()=>1}})), null);
  assert.equal(selectBinaryInventoryMakerQuotes(fixture({fillModel:{calibration:{...certificate,minimumClustersRequired:64},ratePerSecond:()=>1}})), null);
  const crossed = fixture({book:{bestBid:0.6,bestAsk:0.5,coherent:true,complete:true,ageMs:0,sequence:1}});
  assert.equal(selectBinaryInventoryMakerQuotes(crossed), null);
});

test('bad tick, malformed inventory, and off-tick complement prices are rejected', () => {
  assert.equal(selectBinaryInventoryMakerQuotes(fixture({tick:0.03})), null);
  assert.equal(selectBinaryInventoryMakerQuotes(fixture({inventory:0.5})), null);
  assert.throws(() => complementaryBinaryOrder('buy', 0.45, 0.1), /BINARY_MAKER_COMPLEMENT_TICK/);
});

