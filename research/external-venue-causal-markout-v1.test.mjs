import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {consumeCupRow, consumeExternalRow, createCupAudit, createExternalAudit, summarizeCoverage} from './external-venue-causal-markout-v1.mjs';

const hash = value => createHash('sha256').update(value).digest('hex');
const canonical = value => value && typeof value === 'object'
  ? Array.isArray(value) ? `[${value.map(canonical).join(',')}]`
    : `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`
  : JSON.stringify(value);
const iso = ms => new Date(ms).toISOString();
const condition = 'condition-public';
const tokenNo = 'token-no';
const tournament = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
const target = {tournamentId: tournament, marketId: '386', exchangeId: '1075'};

function externalRow(sequence, receiptAtMs, sourceAtMs, previous = '') {
  const gamma = {question: 'Will the Republican Party candidate win the election?', slug: 'winner', conditionId: condition,
    description: 'This market will resolve according to the margin of victory.', outcomes: '["Yes","No"]', clobTokenIds: '["token-yes","token-no"]'};
  const book = {market: condition, asset_id: tokenNo, timestamp: String(sourceAtMs), bids: [{price: '0.4', size: '10'}], asks: [{price: '0.5', size: '10'}]};
  const gammaText = JSON.stringify(gamma), bookText = JSON.stringify(book);
  const event = {type: 'external_public_no_book_observed', accountCredentialsIncluded: false,
    gammaReceipt: {requestStartedAt: iso(receiptAtMs - 10), receivedAt: iso(receiptAtMs), bodyText: gammaText, sourceBodyHash: hash(gammaText)},
    bookReceipt: {requestStartedAt: iso(receiptAtMs - 5), receivedAt: iso(receiptAtMs), bodyText: bookText, sourceBodyHash: hash(bookText)}};
  const body = {sequence, previous, at: iso(receiptAtMs), event, state: {observations: sequence, lastAt: iso(receiptAtMs), lastVenueAt: iso(sourceAtMs)}};
  return {...body, hash: hash(JSON.stringify(body))};
}

function cupRecord(sequence, previousHash, atMs, kind, payload, sessionId = 'session-1') {
  const body = {schemaVersion: 1, kind, sequence, previousHash, sessionId, tournamentId: tournament,
    scopeHash: 'scope-hash', at: iso(atMs), payload};
  return {...body, hash: hash(canonical(body))};
}

function targetBook(receivedAtMs, sourceAgeMs = 1000) {
  return {tournamentId: tournament, marketId: '386', exchangeId: '1075', instrument: '1075:YES',
    requestStartedAt: iso(receivedAtMs - 100), receivedAt: iso(receivedAtMs), sourceAt: iso(receivedAtMs - sourceAgeMs),
    sequence: String(receivedAtMs), versionKey: `${receivedAtMs}:${iso(receivedAtMs - sourceAgeMs)}`,
    freshnessPolicy: 'authoritative_receipt_v1', sourceEventAgeMs: sourceAgeMs,
    bids: [{price: '0.400000', quantity: 25}], asks: [{price: '0.500000', quantity: 30}]};
}

test('external journal checks chain, receipt hashes, stable question/condition/NO token, and description conflict', () => {
  const state = createExternalAudit();
  const first = externalRow(1, 10_000, 9_000);
  const second = externalRow(2, 20_000, 19_000, first.hash);
  consumeExternalRow(state, first);
  consumeExternalRow(state, second);
  assert.equal(state.rows, 2);
  assert.equal(state.questionChanges, 0);
  assert.equal(state.conditionChanges, 0);
  assert.equal(state.noTokenChanges, 0);
  assert.equal(state.venueMarketMismatches, 0);
  assert.equal(state.noTokenMismatches, 0);
  assert.equal(state.marginDescriptionRows, 2);
  assert.equal(summarizeCoverage({external: state, cup: createCupAudit(), contractReview: {contractEquivalenceEstablished: false,
    tiePolicyEquivalenceVerified: false}}).external.descriptionSaysMarginWhileQuestionAsksWinner, true);
  const tampered = {...externalRow(1, 10_000, 9_000), event: {...first.event, bookReceipt: {...first.event.bookReceipt, bodyText: '{}'}}};
  assert.throws(() => consumeExternalRow(createExternalAudit(), tampered), /journal hash mismatch|response hash mismatch/);
});

test('Cup book journal validates canonical hash chain and retains exact target receipts only', () => {
  const state = createCupAudit();
  const start = cupRecord(1, '', 1000, 'session_start', {scope: {exchanges: []}});
  const row = cupRecord(2, start.hash, 5000, 'book', {book: targetBook(4900)});
  consumeCupRow(state, start, target);
  consumeCupRow(state, row, target);
  assert.equal(state.records, 2);
  assert.equal(state.targetBooks.length, 1);
  assert.equal(state.targetBooks[0].sourceAgeAtReceiptMs, 1000);
  assert.throws(() => consumeCupRow(createCupAudit(), {...row, previousHash: 'wrong'}, target), /sequence\/chain/);
});

test('3-second action latency skips earlier Cup quotes and preserves later-book censoring', () => {
  const t = 1_800_000_000_000;
  const external = createExternalAudit();
  const ext = externalRow(1, t, t - 15_000);
  consumeExternalRow(external, ext);
  const cup = createCupAudit();
  const start = cupRecord(1, '', t, 'session_start', {scope: {exchanges: []}});
  const before = cupRecord(2, start.hash, t + 2999, 'book', {book: targetBook(t + 2998)});
  const after = cupRecord(3, before.hash, t + 10_000, 'book', {book: targetBook(t + 9999, 120_000)});
  consumeCupRow(cup, start, target);
  consumeCupRow(cup, before, target);
  consumeCupRow(cup, after, target);
  const result = summarizeCoverage({external, cup, contractReview: {contractEquivalenceEstablished: false, tiePolicyEquivalenceVerified: false}});
  assert.equal(result.joins.laterBookAfterLatency, 1);
  assert.equal(result.joins.delayAfterActionMs.p50, 6999);
  assert.equal(result.joins.sourceAgeAtActionMs.p50, 18_000);
  assert.equal(result.joins.cupBookSourceAgeAtReceiptMs.p50, 120_000);
  assert.equal(result.joins.censoringByDelayAfterLatency.find(row => row.horizonMs === 5000).matched, 0);
  assert.equal(result.joins.censoringByDelayAfterLatency.find(row => row.horizonMs === 30_000).matched, 1);
});

test('clock-ahead external timestamps are not counted as fresh and unresolved payouts suppress markout', () => {
  const t = 1_800_000_000_000;
  const external = createExternalAudit();
  consumeExternalRow(external, externalRow(1, t, t + 5000));
  const cup = createCupAudit();
  const result = summarizeCoverage({external, cup, contractReview: {contractEquivalenceEstablished: false, tiePolicyEquivalenceVerified: false}});
  assert.equal(result.external.sourceAgeAtReceiptMs.clockAhead, 1);
  assert.equal(result.external.sourceAgeAtReceiptMs.within30s, 0);
  assert.equal(result.joins.sourceAgeAtActionMs.clockAhead, 1);
  assert.equal(result.sourceToCupContractFit, 'unresolved_blocker');
  assert.equal(result.markout.status, 'not_run');
  assert.equal(result.markout.observations, 0);
  assert.match(result.markout.reason, /not established as equivalent/);
});

