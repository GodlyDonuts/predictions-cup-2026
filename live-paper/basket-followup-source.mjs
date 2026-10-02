import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {digest, normalizeBook} from './model.mjs';
import {advanceBasketRecord, basketCodeIdentity} from './basket-watch.mjs';
import {createBasketObserver} from './basket-observer.mjs';
import {instantNs} from '../feed/contract.mjs';

const MAX_PENDING = 128, MAX_PROOFS = 64, MAX_AGE_MS = 600000;
const CHUNK_BYTES = 1024 * 1024, MAX_LINE_BYTES = 8 * 1024 * 1024;
const hex = x => typeof x === 'string' && /^[a-f0-9]{64}$/.test(x);
const clock = x => typeof x === 'string' && Number.isFinite(Date.parse(x)) && new Date(x).toISOString() === x;
const need = (ok, code) => { if (!ok) throw Error(code); };
const sha = bytes => createHash('sha256').update(bytes).digest('hex');

// Append-only local files are the trust boundary, not authenticated messages.
// Every row is checked once; exact candidate rows are reread before emission.
// Identity, size, same-size edits, head and prior-tail guards detect replacement
// and ordinary rewrites. Arbitrary edits elsewhere in an old prefix combined
// with an append cannot be disproved without rescanning that entire prefix.
// Restart performs that full verification. No account/source bodies are exposed.
function reader(file) {
  let identity, prior, head, tail, buffer = Buffer.alloc(0), offset = 0, start = 0, cached;
  let streamedBytes = 0;
  const read = (at, length) => {
    const fd = fs.openSync(file, 'r'), bytes = Buffer.alloc(length);
    try {
      let n = 0;
      while (n < length) { const got = fs.readSync(fd, bytes, n, length - n, at + n); need(got > 0, 'JOURNAL_CHANGED'); n += got; }
    } finally { fs.closeSync(fd); }
    return bytes;
  };
  const verify = span => need(sha(read(span.start, span.length)) === span.hash, 'JOURNAL_CHANGED');
  const snapshot = () => {
    const stat = fs.statSync(file);
    need(stat.isFile(), 'JOURNAL_NOT_FILE');
    if (!identity) identity = stat;
    need(stat.dev === identity.dev && stat.ino === identity.ino && stat.birthtimeMs === identity.birthtimeMs, 'JOURNAL_REPLACED');
    if (prior) {
      need(stat.size >= prior.size, 'JOURNAL_TRUNCATED');
      need(stat.size !== prior.size || (stat.mtimeMs === prior.mtimeMs && stat.ctimeMs === prior.ctimeMs), 'JOURNAL_CHANGED');
      if (head) verify(head);
      if (tail) verify(tail);
    }
    const complete = stat.size === 0 || read(stat.size - 1, 1)[0] === 10;
    return {...stat, complete};
  };
  const remember = stat => {
    if (!head && stat.size) { const length = Math.min(4096, stat.size); head = {start: 0, length, hash: sha(read(0, length))}; }
    const length = Math.min(65536, stat.size);
    tail = length ? {start: stat.size - length, length, hash: sha(read(stat.size - length, length))} : null;
    prior = stat;
  };
  const peek = stat => {
    if (cached) return cached;
    while (true) {
      const end = buffer.indexOf(10);
      if (end >= 0) {
        need(end > 0 && end + 1 <= MAX_LINE_BYTES, 'INVALID_JOURNAL_LINE');
        const bytes = buffer.subarray(0, end + 1), text = bytes.subarray(0, end).toString('utf8');
        need(Buffer.from(text, 'utf8').equals(bytes.subarray(0, end)), 'INVALID_JOURNAL_UTF8');
        cached = {row: JSON.parse(text), span: {start, length: end + 1, hash: sha(bytes)}, bytes};
        return cached;
      }
      need(buffer.length <= MAX_LINE_BYTES, 'OVERSIZED_JOURNAL_LINE');
      if (offset >= stat.size) return null;
      const bytes = read(offset, Math.min(CHUNK_BYTES, stat.size - offset));
      offset += bytes.length; streamedBytes += bytes.length; buffer = Buffer.concat([buffer, bytes]);
    }
  };
  const consume = () => { start += cached.span.length; buffer = buffer.subarray(cached.span.length); cached = undefined; };
  return {snapshot, remember, peek, consume, verify, progress: () => ({bytes: streamedBytes, consumedBytes: start})};
}

function checkedBank(input, universe) {
  const bank = structuredClone(input);
  need(bank?.schemaVersion === 1 && bank.mode === 'dual_no_basket_measurement' && clock(bank.cutoff)
    && Number.isSafeInteger(bank.sourceRecords) && bank.sourceRecords > 0 && hex(bank.sourceHash)
    && Array.isArray(bank.pairs) && bank.pairs.length > 0 && bank.pairs.length <= 1000
    && Array.isArray(bank.definitions) && bank.definitions.length === bank.pairs.length, 'INVALID_BASKET_BANK');
  need(digest(bank.codeIdentity) === digest(basketCodeIdentity()), 'FROZEN_BASKET_CODE_CHANGED');
  need(universe && typeof universe.tournamentId === 'string' && Array.isArray(universe.exchanges), 'INVALID_UNIVERSE');
  const markets = new Map(), exchanges = new Set(), pairs = new Set();
  for (const x of universe.exchanges) {
    need(typeof x.marketId === 'string' && typeof x.exchangeId === 'string' && !markets.has(x.marketId) && !exchanges.has(x.exchangeId), 'INVALID_UNIVERSE');
    markets.set(x.marketId, x.exchangeId); exchanges.add(x.exchangeId);
  }
  for (const [i, p] of bank.pairs.entries()) {
    const definition = bank.definitions[i];
    need(hex(p.id) && !pairs.has(p.id) && definition?.id === p.id
      && p.bank?.tournamentId === universe.tournamentId && definition.tournamentId === universe.tournamentId
      && digest(definition.legs) === digest(p.bank.legs) && p.bank.sourceHash === bank.sourceHash && p.bank.cutoff === bank.cutoff, 'FOREIGN_BASKET_SCOPE');
    pairs.add(p.id);
    for (const l of p.bank.legs) need(markets.get(l.marketId) === l.exchangeId, 'FOREIGN_BASKET_SCOPE');
    const expected = createBasketObserver({tournamentId: p.bank.tournamentId, legs: p.bank.legs, sourceHash: bank.sourceHash,
      cutoff: bank.cutoff, policy: p.bank.policy, settlementProof: p.bank.settlementProof});
    need(digest(expected.bank) === digest(p.bank) && digest(expected.state) === digest(p.initialState), 'INVALID_BASKET_INITIAL_STATE');
  }
  return bank;
}

function initialFollowups(record, output, bank, bankHash) {
  const e = record.event, origin = e.basketMeasurement;
  if (e.type !== 'cycle' || origin?.reason !== 'paired_depth_measurement' || origin.followup !== undefined
    || !clock(origin.receivedAt) || !Number.isSafeInteger(origin.cycle) || origin.cycle < 0
    || !Number.isSafeInteger(origin.every) || origin.every < 1 || origin.cycle % origin.every !== 0) return [];
  const sameLegs = p => Array.isArray(origin.marketIds) && Array.isArray(origin.exchangeIds)
    && origin.marketIds.length === 2 && origin.exchangeIds.length === 2
    && new Set(origin.marketIds).size === 2 && p.bank.legs.every(l => origin.marketIds.some((id, i) => id === l.marketId && origin.exchangeIds[i] === l.exchangeId));
  const snapshots = [...(e.sourceSnapshots || []), ...(e.supplementalDepth || [])], found = [];
  for (const o of output.observations) {
    const p = bank.pairs.find(x => x.id === o.pairId);
    if (!p || !sameLegs(p) || o.status !== 'apparent_positive_basket' || o.apparentPositive !== true || o.newPairEvidence !== true
      || o.independentPair !== false || o.positivePersistenceCount !== 1 || Date.parse(origin.receivedAt) > Date.parse(o.at)) continue;
    // Both legs must have been fetched in this exact initiating cycle. A cached
    // leg, incidental pair, or an observation invalidated later in the cycle is
    // insufficient, even if it appears positive in the genuine observer output.
    const exact = p.bank.legs.every((l, i) => {
      const current = output.state.pairs[p.id].books[l.exchangeId];
      if (!current || current.receivedAt !== o.pairReceipts[i] || current.versionKey !== o.pairVersions[i]) return false;
      return snapshots.some(r => {
        if (r.skipped || r.receivedAt !== o.pairReceipts[i] || r.endpoint !== '/markets/' + l.marketId + '/orderbook?tournamentId=' + bank.pairs[0].bank.tournamentId + '&depth=20') return false;
        try {
          const b = normalizeBook(r.data, l.marketId, p.bank.tournamentId, r.receivedAt, r.requestStartedAt, 20000, new Set(e.accountExclusions), 'authoritative_receipt_v1');
          return b.versionKey === o.pairVersions[i] && Date.parse(b.requestStartedAt) >= Date.parse(origin.receivedAt);
        } catch { return false; }
      });
    });
    if (exact) found.push({tournamentId: p.bank.tournamentId, observerBankHash: bankHash, pairId: p.id,
      sourceSequence: record.sequence, sourceHash: record.hash, cutoff: bank.cutoff,
      marketIds: p.bank.legs.map(l => l.marketId), exchangeIds: p.bank.legs.map(l => l.exchangeId),
      observation: structuredClone(o), sourceMeasurement: structuredClone(origin)});
  }
  return found;
}

/** Read-only, bounded incremental evidence reader. No claims are consumed here.
 * Caller must pass a trusted scoped universe and clock; persist/install the
 * planner's returned scheduler state before either follow-up request. A helper
 * failure is optional research unavailability, not permission to skip risk reads.
 * `poll()` returns planner followups; `status()` contains only compact identities
 * and counters. No retained proof, row, bank or observer state is caller-mutable.
 */
export function createBasketFollowupSource({runtimeDir, bank: input, universe, now}) {
  need(typeof runtimeDir === 'string' && typeof now === 'function', 'INVALID_FOLLOWUP_SOURCE_OPTIONS');
  const bank = checkedBank(input, universe), bankHash = digest(bank);
  const source = reader(path.join(runtimeDir, 'journal.jsonl')), observer = reader(path.join(runtimeDir, 'basket-journal.jsonl'));
  const prefix = createHash('sha256'), pending = [], proofs = [];
  let sourceSequence = 0, sourceHash = '', sourceAt = -Infinity, state;
  let derivedSequence = 0, derivedHash = bankHash, journalSequence = 0, journalHash = bankHash;
  let code = 'not_polled', failed = false, closed = false, lastNow = -Infinity, rejectedOrigins = 0;
  const status = () => ({schemaVersion: 1, status: code, bankHash, cutoff: bank.cutoff, sourceSequence, sourceHash,
    journalSequence, journalHash, pendingRows: pending.length, retainedProofs: proofs.length,
    sourceBytesRead: source.progress().bytes, observerBytesRead: observer.progress().bytes,
    rejectedOrigins, maxPendingRows: MAX_PENDING, maxRetainedProofs: MAX_PROOFS,
    eligibleForExecution: false, networkRequests: 0});
  const poll = () => {
    if (closed || failed) return [];
    try {
      const at = now();
      need(Number.isSafeInteger(at) && at >= 0 && at >= lastNow, 'INVALID_FOLLOWUP_CLOCK'); lastNow = at;
      need(digest(bank.codeIdentity) === digest(basketCodeIdentity()), 'FROZEN_BASKET_CODE_CHANGED');
      const ss = source.snapshot(), os = observer.snapshot();
      if (!ss.complete || !os.complete) { code = 'awaiting_complete_append'; return []; }
      let sourceDone = false;
      while (true) {
        if (pending.length) {
          const item = observer.peek(os);
          if (item) {
            const wanted = pending[0], {hash, ...body} = item.row;
            need(hash === digest(body) && digest(item.row) === digest(wanted.row), 'BASKET_REPLAY_MISMATCH');
            journalSequence = item.row.sequence; journalHash = hash;
            for (const followup of wanted.followups) proofs.push({followup, sourceSpan: wanted.sourceSpan, observerSpan: item.span});
            while (proofs.length > MAX_PROOFS) proofs.shift();
            pending.shift(); observer.consume(); continue;
          }
        }
        if (pending.length >= MAX_PENDING) { code = 'awaiting_observer_backlog'; break; }
        const item = source.peek(ss);
        if (!item) { sourceDone = true; break; }
        const r = item.row, {hash, ...body} = r, stamp = r.event?.clockAt || r.at;
        need(body.sequence === sourceSequence + 1 && body.previous === sourceHash && hash === digest(body), 'SOURCE_INTEGRITY_FAILURE');
        need(clock(stamp) && Date.parse(stamp) >= sourceAt, 'SOURCE_CLOCK_ROLLBACK');
        sourceSequence = r.sequence; sourceHash = hash; sourceAt = Date.parse(stamp); prefix.update(item.bytes);
        if (sourceSequence === bank.sourceRecords) {
          need(prefix.copy().digest('hex') === bank.sourceHash && sourceAt <= Date.parse(bank.cutoff), 'BASKET_SOURCE_PREFIX_CHANGED');
          state = {bankHash, sourceSequence, lastRecordHash: sourceHash, pairs: Object.fromEntries(bank.pairs.map(p => [p.id, structuredClone(p.initialState)]))};
        } else if (sourceSequence > bank.sourceRecords) {
          const output = advanceBasketRecord(state, r, bank);
          const next = {sequence: ++derivedSequence, previous: derivedHash, sourceSequence, sourceHash,
            stateHash: digest(output.state), observationsHash: digest(output.observations), observations: output.observations};
          derivedHash = digest(next);
          const followups = initialFollowups(r, output, bank, bankHash);
          rejectedOrigins += output.observations.filter(o => o.status === 'apparent_positive_basket').length - followups.length;
          pending.push({row: {...next, hash: derivedHash}, followups, sourceSpan: item.span});
          state = output.state;
        }
        source.consume();
      }
      // Freeze exactly the byte boundaries processed this poll. Concurrent appends
      // are picked up next time; edits of already verified anchors are fatal.
      const se = source.snapshot(), oe = observer.snapshot();
      need(se.size >= ss.size && oe.size >= os.size, 'JOURNAL_TRUNCATED');
      source.remember(ss); observer.remember(os);
      while (proofs.length && at - Date.parse(proofs[0].followup.observation.at) > MAX_AGE_MS) proofs.shift();
      if (!sourceDone) return [];
      if (!state) { code = 'awaiting_source_prefix'; return []; }
      if (sourceAt > at) { code = 'awaiting_source_clock'; return []; }
      if (pending.length || observer.peek(os)) { code = pending.length ? 'awaiting_observer' : 'awaiting_source'; return []; }
      if (se.size !== ss.size || oe.size !== os.size || !se.complete || !oe.complete) { code = 'awaiting_complete_append'; return []; }
      const results = [];
      for (const proof of proofs) {
        const f = proof.followup, observedAt = Date.parse(f.observation.at);
        if (at <= observedAt + 1000 || at - observedAt > MAX_AGE_MS || f.observation.pairVersions.some(v => instantNs(v.slice(v.indexOf(':') + 1)) > BigInt(at) * 1000000n)) continue;
        source.verify(proof.sourceSpan); observer.verify(proof.observerSpan);
        results.push(structuredClone(f));
      }
      code = 'verified'; return results;
    } catch (error) {
      failed = true; proofs.length = 0; pending.length = 0;
      // Never publish parser errors, paths or raw journal contents.
      code = /^[A-Z][A-Z_]+$/.test(error.message) ? error.message : 'FOLLOWUP_SOURCE_INVALID';
      return [];
    }
  };
  return {poll, status, close() { closed = true; proofs.length = 0; pending.length = 0; state = undefined; code = 'closed'; }};
}
