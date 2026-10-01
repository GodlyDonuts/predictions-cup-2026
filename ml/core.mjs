import { createHash } from 'node:crypto';

export const FORMAT = 'sig-offline-books';
export const FEATURE_NAMES = ['spread', 'depthImbalance', 'micropriceMinusMid', 'pastMidChange', 'pastImbalanceChange'];
export const WARNING = 'SYNTHETIC-ONLY SOFTWARE CHECK. Not market evidence, calibrated fills, strategy P&L or a trained market edge.';
const NS = 1000000n;
export const hash = text => createHash('sha256').update(text).digest('hex');
export const canonical = value => JSON.stringify(value, function (key, item) {
  return item && typeof item === 'object' && !Array.isArray(item)
    ? Object.fromEntries(Object.keys(item).sort().map(k => [k, item[k]])) : item;
});
function fail(message) { throw new Error(message); }
function object(x) { return x !== null && typeof x === 'object' && !Array.isArray(x); }
function exact(x, keys, name) {
  if (!object(x) || Object.keys(x).sort().join('|') !== [...keys].sort().join('|')) fail(`SCHEMA: ${name}`);
}
function id(x, name) { if (typeof x !== 'string' || !/^[a-zA-Z0-9_.:-]{1,128}$/.test(x)) fail(`ID: ${name}`); }
function integer(x, name, min = 0, max = 1000000000) {
  if (!Number.isSafeInteger(x) || x < min || x > max) fail(`INTEGER: ${name}`);
}
export function instant(x) {
  if (typeof x !== 'string') fail('TIMESTAMP');
  const m = /^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(?:\.(\d{1,9}))?Z$/.exec(x);
  if (!m) fail('TIMESTAMP');
  const ms = Date.parse(`${m[1]}.000Z`);
  if (!Number.isFinite(ms) || new Date(ms).toISOString() !== `${m[1]}.000Z`) fail('TIMESTAMP');
  return BigInt(ms) * NS + BigInt((m[2] || '').padEnd(9, '0'));
}
export function validateConfig(c) {
  exact(c, ['schemaVersion', 'horizonMs', 'toleranceMs', 'maxSourceAgeMs', 'maxGapMs', 'trainEnd', 'validationEnd', 'embargoMs', 'alphas', 'seed', 'minSamples', 'minSessions', 'bootstrapReplicates'], 'config');
  if (c.schemaVersion !== 1) fail('CONFIG_VERSION');
  for (const k of ['horizonMs', 'maxSourceAgeMs', 'maxGapMs']) integer(c[k], k, 1, 86400000);
  integer(c.toleranceMs, 'toleranceMs', 0, c.horizonMs);
  integer(c.embargoMs, 'embargoMs', c.horizonMs + c.toleranceMs, 86400000);
  integer(c.seed, 'seed', 0, 4294967295);
  integer(c.bootstrapReplicates, 'bootstrapReplicates', 100, 10000);
  exact(c.minSamples, ['train', 'validation', 'test'], 'minSamples');
  exact(c.minSessions, ['train', 'validation', 'test'], 'minSessions');
  for (const p of ['train', 'validation', 'test']) { integer(c.minSamples[p], `minSamples.${p}`, 2); integer(c.minSessions[p], `minSessions.${p}`, 1); }
  if (!Array.isArray(c.alphas) || !c.alphas.length || c.alphas.length > 20 || c.alphas.some((a, i) => !Number.isFinite(a) || a <= 0 || a > 1000000 || (i && a <= c.alphas[i - 1]))) fail('ALPHAS');
  const t = instant(c.trainEnd), v = instant(c.validationEnd);
  if (v - t <= BigInt(c.embargoMs * 2 + c.horizonMs + c.toleranceMs) * NS) fail('SPLIT_WINDOW');
  return structuredClone(c);
}
function levels(xs, side) {
  if (!Array.isArray(xs) || !xs.length || xs.length > 200) fail('LEVELS');
  let prior = side === 'bids' ? Infinity : -Infinity;
  for (const x of xs) {
    exact(x, ['price', 'quantity'], 'level');
    if (!Number.isFinite(x.price) || x.price < 0 || x.price > 1 || !Number.isFinite(x.quantity) || x.quantity <= 0 || x.quantity > 1e12) fail('LEVEL');
    if (side === 'bids' ? x.price >= prior : x.price <= prior) fail('LEVEL_ORDER');
    prior = x.price;
  }
}
const scope = r => [r.tournamentId, r.marketId, r.exchangeId].join('/');
const localScope = r => [r.sessionId, scope(r)].join('/');
const qualityKeys = ['continuous', 'recovering', 'stale', 'expiryKnown'];
const rowKeys = ['kind', 'rowId', 'sessionId', 'tournamentId', 'marketId', 'exchangeId', 'segmentId', 'receivedAt', 'availableAt'];
export function parseDataset(text) {
  if (typeof text !== 'string' || !text.endsWith('\n')) fail('DATASET: complete newline-terminated JSONL required');
  const lines = text.slice(0, -1).split('\n');
  if (lines.length > 1000001 || lines.some(l => l.length === 0 || l.length > 1000000)) fail('DATASET_LIMIT');
  let values;
  try { values = lines.map(l => JSON.parse(l)); } catch { fail('DATASET_JSON'); }
  const header = values.shift();
  exact(header, ['format', 'schemaVersion', 'datasetId', 'dataKind', 'normalization', 'provenance'], 'header');
  if (header.format !== FORMAT || header.schemaVersion !== 1 || !['synthetic', 'real'].includes(header.dataKind) || header.normalization !== 'quality-segmented-books-v1') fail('DATASET_FORMAT');
  id(header.datasetId, 'datasetId');
  exact(header.provenance, ['sourceSha256', 'normalizerVersion'], 'provenance');
  if (typeof header.provenance.sourceSha256 !== 'string' || !/^[a-f0-9]{64}$/.test(header.provenance.sourceSha256)) fail('SOURCE_HASH');
  id(header.provenance.normalizerVersion, 'normalizerVersion');
  const seen = new Set(); let prior = null;
  const rows = values.map((r, index) => {
    if (!object(r) || !['book', 'barrier'].includes(r.kind)) fail('ROW_KIND');
    exact(r, r.kind === 'book' ? [...rowKeys, 'sourceAt', 'sequence', 'quality', 'nextExpiryAt', 'bids', 'asks'] : [...rowKeys, 'reason'], 'row');
    for (const k of ['rowId', 'sessionId', 'tournamentId', 'marketId', 'exchangeId', 'segmentId']) id(r[k], k);
    if (seen.has(r.rowId)) fail('DUPLICATE_ROW_ID');
    seen.add(r.rowId);
    const availableNs = instant(r.availableAt), receivedNs = instant(r.receivedAt);
    if (receivedNs > availableNs || (prior !== null && availableNs < prior)) fail('AVAILABILITY_ORDER');
    prior = availableNs;
    if (r.kind === 'barrier') {
      if (!['gap', 'recovery', 'disconnect', 'stale', 'expiry', 'settlement'].includes(r.reason)) fail('BARRIER_REASON');
      return { ...r, index, availableNs, receivedNs };
    }
    const sourceNs = instant(r.sourceAt);
    integer(r.sequence, 'sequence', 0, Number.MAX_SAFE_INTEGER);
    exact(r.quality, qualityKeys, 'quality');
    if (qualityKeys.some(k => typeof r.quality[k] !== 'boolean')) fail('QUALITY');
    levels(r.bids, 'bids'); levels(r.asks, 'asks');
    if (r.bids[0].price > r.asks[0].price) fail('CROSSED_BOOK');
    const expiryNs = r.nextExpiryAt === null ? null : instant(r.nextExpiryAt);
    return { ...r, index, availableNs, receivedNs, sourceNs, expiryNs };
  });
  return { header, rows, sourceHash: hash(text) };
}
function basic(r) {
  const bid = r.bids[0], ask = r.asks[0];
  const bidDepth = r.bids.reduce((s, x) => s + x.quantity, 0), askDepth = r.asks.reduce((s, x) => s + x.quantity, 0);
  const mid = (bid.price + ask.price) / 2;
  return { mid, spread: ask.price - bid.price, imbalance: (bidDepth - askDepth) / (bidDepth + askDepth), micro: (ask.price * bid.quantity + bid.price * ask.quantity) / (bid.quantity + ask.quantity) - mid };
}
/** Causal row validation and segmentation. Never sort, interpolate or renew a duplicate. */
export function buildSamples(dataset, c) {
  validateConfig(c);
  const states = new Map(), instruments = new Map(), versionSeen = new Map(), segments = [], excluded = [];
  const maxAge = BigInt(c.maxSourceAgeMs) * NS, maxGap = BigInt(c.maxGapMs) * NS;
  const reject = (r, reason) => excluded.push({ rowId: r.rowId, reason });
  for (const r of dataset.rows) {
    const instrumentKey = scope(r);
    let instrument = instruments.get(instrumentKey);
    if (!instrument) { instrument = { sessionId: null, closedSessions: new Set(), sequence: -1, sourceNs: null, availableNs: null }; instruments.set(instrumentKey, instrument); }
    if (instrument.availableNs !== null && r.availableNs <= instrument.availableNs) fail('AMBIGUOUS_INSTRUMENT_TIME');
    instrument.availableNs = r.availableNs;
    if (instrument.sessionId !== r.sessionId) {
      if (instrument.closedSessions.has(r.sessionId)) fail('SESSION_RESUMED');
      if (instrument.sessionId !== null) instrument.closedSessions.add(instrument.sessionId);
      instrument.sessionId = r.sessionId;
    }
    const key = localScope(r);
    let st = states.get(key);
    if (!st) { st = { segmentId: null, closed: new Set(), blocked: false, latest: null, rows: null }; states.set(key, st); }
    if (r.segmentId !== st.segmentId) {
      if (st.closed.has(r.segmentId)) fail('SEGMENT_REUSED');
      if (st.segmentId !== null) st.closed.add(st.segmentId);
      st.segmentId = r.segmentId; st.blocked = false; st.latest = null;
      st.rows = []; segments.push(st.rows);
    }
    if (r.kind === 'barrier') { st.blocked = true; reject(r, `barrier:${r.reason}`); continue; }
    const q = r.quality;
    const version = [instrumentKey, r.sequence, r.sourceNs].join('/');
    const content = canonical({ bids: r.bids, asks: r.asks, nextExpiryAt: r.nextExpiryAt, expiryKnown: q.expiryKnown });
    const duplicate = versionSeen.has(version);
    if (duplicate && versionSeen.get(version) !== content) fail('CONFLICTING_BOOK_VERSION');
    if (!duplicate && (r.sequence < instrument.sequence || (instrument.sourceNs !== null && r.sourceNs < instrument.sourceNs))) fail('VERSION_ROLLBACK');
    if (!duplicate) {
      versionSeen.set(version, content);
      instrument.sequence = r.sequence; instrument.sourceNs = r.sourceNs;
    }
    let reason = !q.continuous || q.recovering || q.stale || !q.expiryKnown ? 'quality' : null;
    if (r.sourceNs > r.receivedNs) reason = 'source-clock-future';
    else if (r.availableNs - r.sourceNs > maxAge) reason = 'source-stale';
    else if (r.expiryNs !== null && r.expiryNs <= r.availableNs) reason = 'expired';
    if (reason) { st.blocked = true; reject(r, reason); continue; }
    if (st.blocked) { reject(r, 'segment-blocked'); continue; }
    if (duplicate) { reject(r, 'duplicate-version'); continue; }
    const prior = st.latest;
    if (prior) {
      if (r.sequence < prior.sequence || r.sourceNs < prior.sourceNs) fail('VERSION_ROLLBACK');
      if (r.availableNs <= prior.availableNs) fail('AMBIGUOUS_INSTRUMENT_TIME');
      if (r.availableNs - prior.availableNs > maxGap || (prior.expiryNs !== null && prior.expiryNs <= r.availableNs) || r.availableNs - prior.sourceNs > maxAge) {
        st.blocked = true; reject(r, 'unmarked-regime-gap'); continue;
      }
    }
    const b = { ...r, ...basic(r), feature: null, featureStart: null };
    if (prior) { b.feature = [b.spread, b.imbalance, b.micro, b.mid - prior.mid, b.imbalance - prior.imbalance]; b.featureStart = prior.availableNs; }
    st.rows.push(b); st.latest = b;
  }
  const samples = [], missingLabels = [];
  const horizon = BigInt(c.horizonMs) * NS, tolerance = BigInt(c.toleranceMs) * NS;
  let acceptedBooks = 0, warmup = 0;
  for (const rows of segments) {
    acceptedBooks += rows.length;
    let end = 0;
    for (let i = 0; i < rows.length; i++) {
      const r = rows[i];
      if (!r.feature) { warmup++; continue; }
      const target = r.availableNs + horizon;
      end = Math.max(end, i + 1);
      while (end < rows.length && rows[end].availableNs < target) end++;
      const label = rows[end];
      if (!label || label.availableNs > target + tolerance) { missingLabels.push(r.rowId); continue; }
      const past = rows[i - 1];
      samples.push({
        rowId: r.rowId, labelRowId: label.rowId, sessionId: r.sessionId,
        tournamentId: r.tournamentId, marketId: r.marketId, exchangeId: r.exchangeId,
        segmentId: r.segmentId, availableAt: r.availableAt, sourceAt: r.sourceAt,
        receivedAt: r.receivedAt, sequence: r.sequence, quality: r.quality,
        pastRowId: past.rowId, pastAvailableAt: past.availableAt,
        pastReceivedAt: past.receivedAt, pastSourceAt: past.sourceAt,
        pastSequence: past.sequence, pastQuality: past.quality,
        labelAvailableAt: label.availableAt, labelSourceAt: label.sourceAt,
        labelReceivedAt: label.receivedAt, labelSequence: label.sequence,
        labelQuality: label.quality, start: r.availableNs,
        featureStart: r.featureStart, end: label.availableNs,
        x: r.feature, y: label.mid - r.mid
      });
    }
  }
  samples.sort((a, b) => a.start < b.start ? -1 : a.start > b.start ? 1 : a.rowId < b.rowId ? -1 : a.rowId > b.rowId ? 1 : 0);
  return { samples, coverage: { sourceRows: dataset.rows.length, sourceBooks: dataset.rows.filter(r => r.kind === 'book').length, acceptedBooks, warmup, labeled: samples.length, missingLabels: missingLabels.length, excluded: excluded.length, labelCoverageOfSourceBooks: samples.length / Math.max(1, dataset.rows.filter(r => r.kind === 'book').length), reasons: countBy(excluded.map(x => x.reason)) }, excluded, missingLabels };
}
export function countBy(values) { const counts = {}; for (const v of values) counts[v] = (counts[v] || 0) + 1; return counts; }
/** Strict information intervals: past feature start through actual future label time. */
export function splitSamples(samples, c) {
  validateConfig(c);
  const trainCut = instant(c.trainEnd), valCut = instant(c.validationEnd), e = BigInt(c.embargoMs) * NS;
  const partitions = { train: [], validation: [], test: [] }, excluded = [];
  for (const s of samples) {
    const p = s.start < trainCut ? 'train' : s.start < valCut ? 'validation' : 'test';
    let reason = null;
    if ([trainCut, valCut].some(cut => s.start >= cut - e && s.start < cut + e)) reason = 'embargo';
    else if ((p === 'train' && s.end >= trainCut - e) || (p === 'validation' && s.end >= valCut - e)) reason = 'label-purge';
    else if ((p === 'validation' && s.featureStart < trainCut + e) || (p === 'test' && s.featureStart < valCut + e)) reason = 'feature-purge';
    if (reason) excluded.push({ rowId: s.rowId, nominal: p, reason }); else partitions[p].push(s);
  }
  return { partitions, excluded };
}
export function publicSample(s, includeLabel = false) {
  const { start, end, featureStart, x, y, ...metadata } = s;
  return includeLabel ? { ...metadata, features: x, labelMidChange: y } : { rowId: s.rowId, sessionId: s.sessionId, availableAt: s.availableAt };
}
