import test from 'node:test';
import assert from 'node:assert/strict';
import {solveBinaryMakerValueSurface} from './binary-market-maker-hjb-v1.mjs';

const calibration = {
  status: 'validated',
  source: 'own_fill_receipts',
  evidenceId: 'test-fixture-only',
  modelHash: 'b'.repeat(64),
  minimumClustersRequired: 32,
  independentClusters: 32,
  cutoffUtc: '2026-10-14T00:00:00.000Z',
};
const transitionModel = {
  status: 'validated',
  source: 'chronological_market_history',
  evidenceId: 'test-transition-fixture-only',
  modelHash: 'c'.repeat(64),
  cutoffUtc: '2026-10-14T00:00:00.000Z',
};

function fixture(overrides = {}) {
  return {
    probabilityGrid: [0, 0.5, 1],
    transitionModel: {
      ...transitionModel,
      matrix: [
        [1, 0, 0],
        [0, 1, 0],
        [0, 0, 1],
      ],
    },
    evaluationStartUtc: '2026-10-14T00:01:00.000Z',
    maxInventory: 1,
    lotSize: 1,
    tick: 0.25,
    steps: 1,
    dtSeconds: 1,
    gammaTerminal: 0,
    gammaRunning: 0,
    varianceRatePerSecond: 0,
    maxIntensityStep: 0.25,
    quoteContext: () => ({
      bestBid: 0.25,
      bestAsk: 0.75,
      coherent: true,
      complete: true,
      ageMs: 0,
      maxAgeMs: 1000,
      sequence: 1,
    }),
    fillModel: {
      calibration,
      ratePerSecond: ({side}) => side === 'buy' ? 0.1 : 0.1,
    },
    ...overrides,
  };
}

test('zero-risk one-step recurrence equals the hand-computed bid and ask utility', () => {
  const model = solveBinaryMakerValueSurface(fixture());
  assert.ok(model);
  assert.equal(model.researchOnly, true);
  assert.equal(model.executionEligible, false);
  assert.equal(model.valueAt({probability:0.5, remainingSeconds:1, inventory:0}), 0.05);
  assert.equal(model.valueAt({probability:0.5, remainingSeconds:0, inventory:0}), 0);
});

test('terminal binary variance penalty is zero at boundaries and greatest at a fair coin', () => {
  const model = solveBinaryMakerValueSurface(fixture({
    steps:0,
    gammaTerminal:2,
    fillModel:{calibration,ratePerSecond:()=>0},
  }));
  assert.ok(model);
  assert.equal(model.valueAt({probability:0,remainingSeconds:0,inventory:1}), 0);
  assert.equal(model.valueAt({probability:0.5,remainingSeconds:0,inventory:1}), -0.5);
  assert.equal(model.valueAt({probability:1,remainingSeconds:0,inventory:-1}), 0);
});

test('terminal inventory risk removes otherwise positive fills near the binary midpoint', () => {
  const model = solveBinaryMakerValueSurface(fixture({gammaTerminal:2}));
  assert.ok(model);
  assert.equal(model.valueAt({probability:0.5,remainingSeconds:1,inventory:0}), 0);
});

test('inventory limits prevent transitions beyond the signed grid', () => {
  const model = solveBinaryMakerValueSurface(fixture());
  assert.ok(model);
  assert.deepEqual(model.inventoryGrid, [-1,0,1]);
  assert.equal(model.valueAt({probability:0.5,remainingSeconds:1,inventory:2}), null);
  assert.equal(model.valueAt({probability:0.5,remainingSeconds:1,inventory:0.5}), null);
});

test('running variance risk changes continuation value and responds to probability state', () => {
  const model = solveBinaryMakerValueSurface(fixture({
    steps:2,
    gammaTerminal:0,
    gammaRunning:1,
    varianceRatePerSecond:({probability})=>probability*(1-probability),
    fillModel:{calibration,ratePerSecond:()=>0},
  }));
  assert.ok(model);
  assert.ok(model.valueAt({probability:0.5,remainingSeconds:2,inventory:1})
    < model.valueAt({probability:0,remainingSeconds:2,inventory:1}));
});

test('non-crossing book limits and non-positive fill values suppress quote utility', () => {
  const model = solveBinaryMakerValueSurface(fixture({
    quoteContext:()=>({...fixture().quoteContext(),bestBid:0.5,bestAsk:0.5}),
  }));
  assert.equal(model, null);
  const noEdge = solveBinaryMakerValueSurface(fixture({
    fillModel:{calibration,ratePerSecond:()=>0},
  }));
  assert.equal(noEdge.valueAt({probability:0.5,remainingSeconds:1,inventory:0}), 0);
});

test('invalid transition rows, probabilities, stale books, and uncalibrated rates fail closed', () => {
  assert.equal(solveBinaryMakerValueSurface(fixture({
    transitionModel:{...fixture().transitionModel,matrix:[[1,0,0],[0.1,0.1,0],[0,0,1]]},
  })), null);
  assert.equal(solveBinaryMakerValueSurface(fixture({probabilityGrid:[0,0.5,0.5]})), null);
  assert.equal(solveBinaryMakerValueSurface(fixture({
    quoteContext:()=>({...fixture().quoteContext(),ageMs:1001}),
  })), null);
  assert.equal(solveBinaryMakerValueSurface(fixture({
    fillModel:{ratePerSecond:()=>1},
  })), null);
  assert.equal(solveBinaryMakerValueSurface(fixture({
    evaluationStartUtc:'2026-10-14T00:00:00.000Z',
  })), null);
  assert.equal(solveBinaryMakerValueSurface(fixture({
    transitionModel:{...fixture().transitionModel,cutoffUtc:'2026-10-14T00:02:00.000Z'},
  })), null);
});

test('invalid values and excessive arrival mass fail closed', () => {
  assert.equal(solveBinaryMakerValueSurface(fixture({
    gammaTerminal:-1,
  })), null);
  assert.equal(solveBinaryMakerValueSurface(fixture({
    varianceRatePerSecond:({probability})=>probability===0.5?NaN:0,
  })), null);
  assert.equal(solveBinaryMakerValueSurface(fixture({
    fillModel:{calibration,ratePerSecond:()=>0.5},
  })), null);
  assert.equal(solveBinaryMakerValueSurface(fixture({steps:10001})), null);
});


