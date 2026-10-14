import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {POLL_MARKET_SCREEN_V1, candidateContractIdentity, compareBoundToQuote} from './primary-poll-market-screen-v1.mjs';

const market = (overrides = {}) => ({
  marketId: '42', rootHash: 'root-hash', receivedAt: '2026-10-13T19:52:33.000Z',
  root: {title: 'Will the Democratic Party win the Example Senate?', contract_details: {
    contractType: 'Election Outcome', resolutionType: 'Party Winner', raceStage: 'General',
    officeLevel: 'Federal', usState: 'EX', raceName: 'U.S. Senate Example', raceId: '10',
    stageId: '20', electionDate: '2026-11-03', winnerName: 'Democratic Party',
  }}, ...overrides,
});

test('candidate mapping requires exact general-election party-winner contract metadata', () => {
  const value = candidateContractIdentity(market());
  assert.equal(value.state, 'EX');
  assert.equal(value.office, 'U.S. Senate Example');
  assert.equal(value.stage, 'General');
  assert.equal(value.party, 'democratic');
  assert.equal(candidateContractIdentity(market({root: {contract_details: {
    ...market().root.contract_details, raceStage: 'Primary',
  }}})), null);
  assert.equal(candidateContractIdentity(market({root: {contract_details: {
    ...market().root.contract_details, winnerName: 'Libertarian Party',
  }}})), null);
  assert.equal(candidateContractIdentity(market({root: {contract_type: 'Freeform'}})), null);
});

test('poll shares or an unverified source cannot be promoted into a contract bound', () => {
  const noBound = compareBoundToQuote({ask: 0.40, lowerBound: null,
    exactMapping: true, sourceHashVerified: true, calibrationScopeMatched: true});
  assert.equal(noBound.eligible, false);
  assert.equal(noBound.lowerBound, null);
  assert.equal(noBound.edge, null);
  assert.equal(noBound.oneTickEntry, 0.405);
  const unhashed = compareBoundToQuote({ask: 0.40, lowerBound: 0.9,
    exactMapping: true, sourceHashVerified: false, calibrationScopeMatched: true});
  assert.equal(unhashed.eligible, false);
  assert.equal(unhashed.lowerBound, null);
});

test('a fully verified hypothetical lower bound must clear the ask plus one tick', () => {
  const miss = compareBoundToQuote({ask: 0.50, lowerBound: 0.504,
    exactMapping: true, sourceHashVerified: true, calibrationScopeMatched: true});
  assert.equal(miss.oneTickEntry, 0.505);
  assert.equal(miss.edge, -0.001);
  assert.equal(miss.eligible, false);
  const pass = compareBoundToQuote({ask: 0.50, lowerBound: 0.505,
    exactMapping: true, sourceHashVerified: true, calibrationScopeMatched: true});
  assert.equal(pass.edge, 0);
  assert.equal(pass.eligible, true);
});

test('generated screen accounts for every sealed market and never fills an unavailable bound', () => {
  const file = new URL('../data/primary-poll-market-screen-20261013.json', import.meta.url);
  assert(fs.existsSync(file), 'run the offline screen before this report audit');
  const report = JSON.parse(fs.readFileSync(file, 'utf8'));
  assert.equal(report.source.marketCount, 237);
  assert.equal(report.candidates.length, 237);
  assert.equal(new Set(report.candidates.map(x => x.marketId)).size, 237);
  assert.equal(Object.values(report.universe.rootCategories).reduce((a, b) => a + b, 0), 237);
  assert.equal(report.exclusions.eligibleCandidates, 0);
  assert.equal(report.eligibleTrades, 0);
  assert.equal(report.priceVsBound.numericComparisons, 0);
  assert(report.candidates.every(x => x.eligible === false && x.probabilityBound === null));
  assert(report.candidates.every(x => x.quote === null || Number.isFinite(x.quote.oneTickEntry)));
  assert(report.candidates.every(x => !x.quote || Date.parse(x.quote.receivedAt) <= Date.parse(report.source.through)));
  assert(report.candidates.every(x => !x.contract?.rootReceivedAt || Date.parse(x.contract.rootReceivedAt) <= Date.parse(x.quote.receivedAt)));
  assert(report.candidates.filter(x => x.contract?.state === 'IA' && x.contract.office.includes('Senate'))
    .every(x => x.sourceEvidence.some(s => s.contentHash === null && s.publishedAt === '2026-10-09')));
  assert.equal(report.networkRequests, 0);
  assert.equal(report.runtimeChanges, 0);
});

test('report pins the sealed capture digest and records every retained and unretained source digest state', () => {
  const file = new URL('../data/primary-poll-market-screen-20261013.json', import.meta.url);
  const report = JSON.parse(fs.readFileSync(file, 'utf8'));
  const pinnedCaptureSha256 = 'fd6a9552a5f70365ee78706fa1efff70e2a36e7f3dd55c1a932cc3795e1979e7';
  assert.equal(POLL_MARKET_SCREEN_V1.capture.sha256, pinnedCaptureSha256);
  assert.equal(report.source.sha256, pinnedCaptureSha256);
  assert.equal(report.sourceRecords.digestAlgorithm, 'SHA-256');

  const digests = report.sourceRecords.retainedPrimaryPageDigests;
  assert.equal(digests.length, 16);
  assert.equal(report.sourceRecords.locallyHashVerifiedPrimaryPages, 16);
  assert.equal(new Set(digests.map(x => `${x.sourceDirectory}/${x.file}`)).size, digests.length);
  const digestByFile = new Map(digests.map(x => [x.file, x]));
  for (const source of digests) {
    assert.match(source.expectedHash, /^[a-f0-9]{64}$/);
    assert.equal(source.actualHash, source.expectedHash, `${source.sourceDirectory}/${source.file}`);
    assert.equal(source.digestState, 'verified_match', `${source.sourceDirectory}/${source.file}`);
  }

  const unretained = Object.values(report.sourceRecords.knownNewerPrimarySources);
  assert.equal(unretained.length, 3);
  for (const source of unretained) {
    assert.equal(source.contentHash, null);
    assert.equal(source.expectedHash, null);
    assert.equal(source.actualHash, null);
    assert.equal(source.digestState, 'unretained_null');
  }
  for (const candidate of report.candidates) for (const source of candidate.sourceEvidence) {
    if (source.file) {
      assert.equal(source.actualHash, source.expectedHash);
      assert.equal(source.digestState, 'verified_match');
      const retained = digestByFile.get(source.file);
      assert(retained, `missing retained source digest: ${source.file}`);
      assert.equal(source.actualHash, retained.actualHash);
      assert.equal(source.digestState, retained.digestState);
    } else {
      assert.equal(source.contentHash, null);
      assert.equal(source.expectedHash, null);
      assert.equal(source.actualHash, null);
      assert.equal(source.digestState, 'unretained_null');
    }
  }
  assert.equal(report.eligibleTrades, 0);
  assert.equal(report.profitabilityEstablished, false);
  assert(report.candidates.every(x => x.eligible === false && x.probabilityBound === null));
});
