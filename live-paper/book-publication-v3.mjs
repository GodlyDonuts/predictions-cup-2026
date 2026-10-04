import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {normalizeBook} from './model.mjs';
import {amount, price, instant} from '../paper/fixed.mjs';
import {instantNs, timestamp} from '../feed/contract.mjs';
import {nativePublicationState,nativePublicationBinding} from './native-publication.mjs';

const writers = new Set();
const reasons = new Set(['BOOK_READ_FAILED', 'INVALID_BOOK', 'ACCOUNT_OUTAGE', 'STOP', 'COLLECTOR_FAILURE', 'MANUAL_INVALIDATION']);
const stopReasons = new Set(['STOP', 'COLLECTOR_FAILURE', 'NORMAL_SHUTDOWN']);
const check = (ok, code = 'PUBLIC_BOOK_RECORD_INVALID') => { if (!ok) throw Error(code); };
const id = value => typeof value === 'string' && /^[1-9]\d*$/.test(value) && Number.isSafeInteger(Number(value));
const plain = value => value !== null && typeof value === 'object' && !Array.isArray(value)
  && Object.getPrototypeOf(value) === Object.prototype && !['__proto__', 'constructor', 'prototype'].some(key => Object.hasOwn(value, key));
const exact = (value, keys) => plain(value) && Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
const canonical = value => JSON.stringify(value && typeof value === 'object' ? Array.isArray(value)
  ? value.map(item => JSON.parse(canonical(item))) : Object.fromEntries(Object.keys(value).sort().map(key => [key, JSON.parse(canonical(value[key]))])) : value);
const hash = value => createHash('sha256').update(canonical(value)).digest('hex');
const frozen = new WeakSet(), trustedScopes = new WeakMap();
const freeze = value => { if (value && typeof value === 'object' && !frozen.has(value)) { Object.values(value).forEach(freeze); Object.freeze(value); frozen.add(value); } return value; };
const session = value => typeof value === 'string' && /^[A-Za-z0-9][A-Za-z0-9:_.-]{0,127}$/.test(value);

function scopeOf(universe) {
  check(plain(universe) && typeof universe.tournamentId === 'string' && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(universe.tournamentId)
    && Array.isArray(universe.exchanges) && universe.exchanges.length > 0 && universe.exchanges.length <= 10000, 'PUBLIC_BOOK_SCOPE_INVALID');
  const markets = new Set(), exchanges = new Set();
  const rows = universe.exchanges.map(row => {
    check(plain(row) && id(row.marketId) && id(row.exchangeId) && row.option === 'YES'
      && !markets.has(row.marketId) && !exchanges.has(row.exchangeId), 'PUBLIC_BOOK_SCOPE_INVALID');
    markets.add(row.marketId); exchanges.add(row.exchangeId);
    return {marketId: row.marketId, exchangeId: row.exchangeId, option: 'YES'};
  }).sort((a, b) => Number(a.marketId) - Number(b.marketId));
  return {tournamentId: universe.tournamentId, exchanges: rows};
}

export function createBookPublicationState(universe) {
  const scope = freeze(scopeOf(universe)); trustedScopes.set(scope, hash(scope));
  return freeze({schemaVersion: 1, tournamentId: scope.tournamentId, scope, scopeHash: hash(scope), sessionId: null,
    sessions: [], closed: true, sequence: 0, hash: '', lastAt: null, books: {}, sourceVersions: {},
    invalidations: {global: null, markets: {}}});
}

function validateBook(book, state, publishedAt) {
  const fields = ['tournamentId', 'marketId', 'exchangeId', 'instrument', 'receivedAt', 'requestStartedAt', 'sourceAt',
    'sequence', 'versionKey', 'bids', 'asks', 'freshnessPolicy', 'sourceEventAgeMs'];
  check(exact(book, fields));
  const spec = state.scope.exchanges.find(row => row.marketId === book.marketId);
  check(spec && spec.exchangeId === book.exchangeId && book.tournamentId === state.tournamentId
    && book.instrument === book.exchangeId + ':YES' && book.freshnessPolicy === 'authoritative_receipt_v1');
  const started = instant(book.requestStartedAt), received = instant(book.receivedAt), published = instant(publishedAt), source = instantNs(book.sourceAt);
  check(started <= received && received <= published && published - received <= 20000 && received - started <= 20000);
  check(book.sourceAt === timestamp(book.sourceAt) && source <= BigInt(received + 2000) * 1000000n
    && typeof book.sequence === 'string' && /^(0|[1-9]\d*)$/.test(book.sequence) && Number.isSafeInteger(Number(book.sequence))
    && book.versionKey === book.sequence + ':' + book.sourceAt
    && book.sourceEventAgeMs === received - Date.parse(book.sourceAt));
  for (const barrier of [state.invalidations.global, state.invalidations.markets[book.marketId]]) {
    if (barrier) check(started >= instant(barrier.at), 'PUBLIC_BOOK_REQUEST_PRECEDES_INVALIDATION');
  }
  const levels = (rows, descending) => {
    check(Array.isArray(rows) && rows.length > 0 && rows.length <= 20);
    let previous = null;
    for (const row of rows) {
      check(exact(row, ['price', 'quantity']) && Number.isSafeInteger(row.quantity) && row.quantity > 0);
      const p = price(row.price); check(previous === null || (descending ? p < previous : p > previous)); previous = p;
    }
  };
  levels(book.bids, true); levels(book.asks, false); check(amount(book.bids[0].price) < amount(book.asks[0].price));
  const depthHash = hash({bids: book.bids, asks: book.asks}), prior = state.sourceVersions[book.instrument];
  if (prior) {
    check(BigInt(book.sequence) >= BigInt(prior.sequence) && source >= instantNs(prior.sourceAt)
      && received >= instant(prior.receivedAt), 'PUBLIC_BOOK_SOURCE_ROLLBACK');
    check(book.sequence !== prior.sequence || (book.versionKey === prior.versionKey && depthHash === prior.depthHash), 'PUBLIC_BOOK_VERSION_CONFLICT');
  }
  return {sequence: book.sequence, sourceAt: book.sourceAt, versionKey: book.versionKey, receivedAt: book.receivedAt, depthHash};
}

// Incremental consumer reducer. An incomplete JSONL tail is handled by the host;
// every complete record must pass this chain and scope check before use.
export function applyBookPublication(state, record) {
  check(state?.schemaVersion === 1 && state.scopeHash === (trustedScopes.get(state.scope) || hash(scopeOf(state.scope))), 'PUBLIC_BOOK_STATE_INVALID');
  check(exact(record, ['schemaVersion', 'kind', 'sequence', 'previousHash', 'sessionId', 'tournamentId', 'scopeHash', 'at', 'payload', 'hash']));
  const {hash: receivedHash, ...body} = record;
  check(body.schemaVersion === 1 && Number.isSafeInteger(body.sequence) && body.sequence === state.sequence + 1
    && body.previousHash === state.hash && receivedHash === hash(body) && body.tournamentId === state.tournamentId
    && body.scopeHash === state.scopeHash && session(body.sessionId), 'PUBLIC_BOOK_CHAIN_INVALID');
  const at = instant(body.at); check(state.lastAt === null || at >= instant(state.lastAt), 'PUBLIC_BOOK_CLOCK_ROLLBACK');
  const next = {...state, books: {...state.books}, sourceVersions: {...state.sourceVersions},
    invalidations: {global: state.invalidations.global, markets: {...state.invalidations.markets}}};
  if (body.kind === 'session_start') {
    check(exact(body.payload, ['scope']) && canonical(body.payload.scope) === canonical(state.scope)
      && !state.sessions.includes(body.sessionId), 'PUBLIC_BOOK_SESSION_INVALID');
    next.sessionId = body.sessionId; next.sessions = [...state.sessions, body.sessionId]; next.closed = false; next.books = {};
    next.invalidations = {global: {at: body.at, reason: 'SESSION_START', sequence: body.sequence}, markets: {}};
  } else {
    check(!state.closed && body.sessionId === state.sessionId, 'PUBLIC_BOOK_SESSION_CLOSED');
    if (body.kind === 'book') {
      check(exact(body.payload, ['book']));
      const book = body.payload.book, version = validateBook(book, state, body.at);
      next.sourceVersions[book.instrument] = version;
      next.books[book.instrument] = {...structuredClone(book), publicationSequence: body.sequence, publicationHash: receivedHash,
        publicationSessionId: body.sessionId, publishedAt: body.at};
    } else if (body.kind === 'invalidation') {
      check(exact(body.payload, ['marketId', 'reason']) && reasons.has(body.payload.reason)
        && (body.payload.marketId === null || state.scope.exchanges.some(row => row.marketId === body.payload.marketId)));
      const barrier = {at: body.at, reason: body.payload.reason, sequence: body.sequence};
      if (body.payload.marketId === null) { next.books = {}; next.invalidations.global = barrier; }
      else {
        next.invalidations.markets[body.payload.marketId] = barrier;
        for (const [key, book] of Object.entries(next.books)) if (book.marketId === body.payload.marketId) delete next.books[key];
      }
    } else if (body.kind === 'session_stop') {
      check(exact(body.payload, ['reason']) && stopReasons.has(body.payload.reason));
      next.closed = true; next.books = {}; next.invalidations.global = {at: body.at, reason: body.payload.reason, sequence: body.sequence};
    } else check(false);
  }
  next.sequence = body.sequence; next.hash = receivedHash; next.lastAt = body.at;
  return freeze(next);
}

// The caller must already own the collector lease. No independent network,
// credentials, account data, or recovery of damaged publication files lives here.
// Ordinary invalid books and STOP return false after a durable invalidation.
// Persistence/chain failures throw a stable code and must stop the collector.
export async function createNativeBookPublication({file, universe, sessionId, runtimeDir, nativeBackend, receiptDir, now = Date.now, canOperate = () => true, io = fs} = {}) {
  check(typeof file === 'string' && path.isAbsolute(file) && session(sessionId) && typeof now === 'function' && typeof canOperate === 'function', 'PUBLIC_BOOK_DEPENDENCIES');
  const resolved = path.resolve(file).toLowerCase(); check(!writers.has(resolved), 'PUBLIC_BOOK_WRITER_EXISTS');
  let state = createBookPublicationState(universe), fd = null, size = 0, failed = false, startup = null;
  const stamp = () => { const n = now(); check(Number.isSafeInteger(n) && n >= 0 && n <= 8640000000000000, 'PUBLIC_BOOK_CLOCK_INVALID'); return new Date(n).toISOString(); };
  const allowed = () => { try { return canOperate() === true; } catch { return false; } };
  const prepare = (kind, payload) => {
    const body = {schemaVersion: 1, kind, sequence: state.sequence + 1, previousHash: state.hash, sessionId,
      tournamentId: state.tournamentId, scopeHash: state.scopeHash, at: stamp(), payload};
    const record = {...body, hash: hash(body)};
    return {record, next: applyBookPublication(state, record)};
  };
  const release = () => { if (fd !== null) { try { io.closeSync(fd); } finally { fd = null; writers.delete(resolved); } } };
  const persist = prepared => {
    check(fd !== null && !failed, 'PUBLIC_BOOK_WRITER_CLOSED');
    try {
      check(io.fstatSync(fd).size === size, 'PUBLIC_BOOK_APPEND_CONFLICT');
      const bytes = Buffer.from(JSON.stringify(prepared.record) + '\n'); let offset = 0;
      while (offset < bytes.length) { const written = io.writeSync(fd, bytes, offset, bytes.length - offset); check(Number.isInteger(written) && written > 0 && written <= bytes.length - offset, 'PUBLIC_BOOK_SHORT_WRITE'); offset += written; }
      io.fsyncSync(fd); size += bytes.length; state = prepared.next; return prepared.record;
    } catch { failed = true; try { release(); } catch {} throw Error('PUBLIC_BOOK_DURABILITY_FAILURE'); }
  };
  try {
    writers.add(resolved);
    check(allowed(), 'PUBLIC_BOOK_STOPPED');
    nativePublicationBinding(runtimeDir,nativeBackend);
    if(!io.existsSync(file))io.writeFileSync(file,Buffer.alloc(0),{flag:'wx',mode:0o600});
    const raw=io.readFileSync(file),sourceHash=createHash('sha256').update(raw).digest('hex'),started=performance.now();
    const restored=await nativePublicationState({runtimeDir,binding:nativeBackend,universe,journal:file,endByte:raw.length,expectedSha:sourceHash,receiptDir});
    check(allowed(), 'PUBLIC_BOOK_STOPPED');
    check(createHash('sha256').update(io.readFileSync(file)).digest('hex')===sourceHash,'PUBLIC_BOOK_PREFIX_CHANGED');
    check(restored.state.scopeHash===state.scopeHash,'PUBLIC_BOOK_SCOPE_INVALID');
    trustedScopes.set(restored.state.scope,restored.state.scopeHash);state=freeze(restored.state);
    startup=freeze({mode:'native_public_book_prefix_v1',sourceBytes:raw.length,sourceHash,sourceRecords:state.sequence,
      sourceTailHash:state.hash,verifyMs:restored.verifyMs,adapterMs:performance.now()-started,binaryHash:nativeBackend.binaryHash,engineHash:restored.engineHash});
    size=raw.length;fd=io.openSync(file, 'a', 0o600);
    persist(prepare('session_start', {scope: state.scope}));
  } catch (error) { try { release(); } catch {} writers.delete(resolved); throw Error(error?.message?.startsWith('PUBLIC_BOOK_') ? error.message : 'PUBLIC_BOOK_CHAIN_INVALID'); }
  const invalidate = ({marketId = null, reason} = {}) => {
    check(reasons.has(reason) && (marketId === null || state.scope.exchanges.some(row => row.marketId === marketId)));
    return persist(prepare('invalidation', {marketId, reason}));
  };
  const close = (reason = 'STOP') => {
    if (fd === null) return false;
    check(stopReasons.has(reason));
    try { persist(prepare('session_stop', {reason})); return true; } finally { release(); }
  };
  const publishBook = ({marketId, response} = {}) => {
    check(fd !== null && !failed, 'PUBLIC_BOOK_WRITER_CLOSED');
    if (!allowed()) { close('STOP'); return false; }
    const spec = state.scope.exchanges.find(row => row.marketId === marketId);
    check(spec, 'PUBLIC_BOOK_SCOPE_INVALID');
    let prepared;
    try {
      const endpoint = '/markets/' + marketId + '/orderbook?tournamentId=' + state.tournamentId + '&depth=20';
      check(response?.endpoint === endpoint && (response.status === undefined || response.status === 200)
        && (response.data?.marketId === undefined || String(response.data.marketId) === marketId));
      const normalized = normalizeBook(response.data, marketId, state.tournamentId, response.receivedAt, response.requestStartedAt, 20000, new Set(), 'authoritative_receipt_v1');
      check(normalized.exchangeId === spec.exchangeId);
      prepared = prepare('book', {book: {tournamentId: state.tournamentId, ...normalized}});
    } catch { invalidate({marketId, reason: 'INVALID_BOOK'}); return false; }
    if (!allowed()) { close('STOP'); return false; }
    persist(prepared); return true;
  };
  return Object.freeze({publishBook, invalidate, close, startup: () => startup, status: () => failed ? freeze({...state, failed: true, closed: true, books: {}}) : state});
}
