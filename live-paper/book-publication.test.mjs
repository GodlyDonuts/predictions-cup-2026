import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {createBookPublication, createBookPublicationState, applyBookPublication} from './book-publication.mjs';

const TID = 'bda92870-621e-47b0-bc3c-3602c5c26f55', NOW = Date.parse('2026-10-02T12:00:00.000Z');
const universe = {tournamentId: TID, exchanges: [{marketId: '369', exchangeId: '1058', option: 'YES'}, {marketId: '377', exchangeId: '1066', option: 'YES'}]};
const at = n => new Date(NOW + n).toISOString();
function response({marketId = '369', exchangeId = '1058', started = 100, received = 200, source = 130, sequence = 1, bid = 0.235, ask = 0.240} = {}) {
  return {endpoint: '/markets/' + marketId + '/orderbook?tournamentId=' + TID + '&depth=20', requestStartedAt: at(started), receivedAt: at(received),
    headers: {Authorization: 'SHOULD_NEVER_BE_PUBLISHED'}, data: {unknown: 'RAW_RESPONSE_SHOULD_NOT_APPEAR', contexts: [{type: 'tournament', tournament: {id: TID},
      orderbook: {exchanges: [{exchangeId, option: 'YES', asOf: {sequence, at: at(source)}, bids: [{price: bid, quantity: 200}], asks: [{price: ask, quantity: 200}]}]}}]}};
}
function fixture(t, options = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'sig-public-books-')), file = path.join(dir, 'public-books.jsonl');
  let clock = NOW, allowed = true, writer;
  writer = createBookPublication({file, universe, sessionId: 'session-1', now: () => clock, canOperate: () => allowed, ...options});
  t.after(() => { try { writer.close(); } finally { fs.rmSync(dir, {recursive: true, force: true}); } });
  return {file, dir, writer, setTime: n => { clock = NOW + n; }, stop: () => { allowed = false; }, rows: () => fs.readFileSync(file, 'utf8').trim().split('\n').map(JSON.parse)};
}
const replay = rows => rows.reduce(applyBookPublication, createBookPublicationState(universe));
const canonical = value => JSON.stringify(value && typeof value === 'object' ? Array.isArray(value) ? value.map(x => JSON.parse(canonical(x)))
  : Object.fromEntries(Object.keys(value).sort().map(k => [k, JSON.parse(canonical(value[k]))])) : value);
const rehash = record => { const {hash, ...body} = record; return {...body, hash: createHash('sha256').update(canonical(body)).digest('hex')}; };

test('successful publication is immediately durable, allowlisted, immutable and incrementally replayable', t => {
  const f = fixture(t); f.setTime(300);
  assert.equal(f.writer.publishBook({marketId: '369', response: response()}), true);
  const rows = f.rows(), state = f.writer.status(); assert.equal(rows.length, 2); assert.equal(rows[0].kind, 'session_start');
  assert.equal(rows[1].kind, 'book'); assert.equal(state.sequence, 2); assert.equal(state.books['1058:YES'].tournamentId, TID);
  assert.equal(state.books['1058:YES'].receivedAt, at(200)); assert.equal(state.books['1058:YES'].requestStartedAt, at(100));
  assert.equal(state.books['1058:YES'].sourceAt, '2026-10-02T12:00:00.130000000Z');
  assert.equal(state.books['1058:YES'].publicationHash, rows[1].hash); assert.equal(state.books['1058:YES'].publishedAt, at(300));
  assert.deepEqual(replay(rows), state); assert.ok(Object.isFrozen(state.books['1058:YES'].bids));
  const raw = fs.readFileSync(f.file, 'utf8'); assert.ok(!raw.includes('SHOULD')); assert.ok(!raw.includes('headers')); assert.ok(raw.endsWith('\n'));
});

test('invalid ordinary responses clear prior mark and return false, without aborting future valid collection', t => {
  const f = fixture(t); f.setTime(300); f.writer.publishBook({marketId: '369', response: response()});
  const bad = response({started: 400, received: 500, source: 450, sequence: 2}); bad.data.contexts[0].orderbook.exchanges[0].bids[0].price = 0.236;
  f.setTime(600); assert.equal(f.writer.publishBook({marketId: '369', response: bad}), false);
  assert.deepEqual(f.writer.status().books, {}); assert.equal(f.rows().at(-1).payload.reason, 'INVALID_BOOK');
  f.setTime(900); assert.equal(f.writer.publishBook({marketId: '369', response: response({started: 700, received: 800, source: 750, sequence: 3})}), true);
  assert.deepEqual(replay(f.rows()), f.writer.status());
});

test('depth, exact endpoint, tournament and market/exchange mismatches invalidate only the requested scope', t => {
  const changes = [r => { r.endpoint = r.endpoint.replace('depth=20', 'depth=200'); }, r => { r.data.contexts[0].tournament.id = 'foreign'; },
    r => { r.data.contexts[0].orderbook.exchanges[0].exchangeId = '1066'; }, r => { r.data.contexts[0].orderbook.exchanges[0].option = 'NO'; },
    r => { r.data.marketId = '377'; }, r => { r.data.contexts[0].orderbook.exchanges[0].bids[0].quantity = 1.5; },
    r => { r.data.contexts[0].orderbook.exchanges[0].asks[0].price = 0.235; }, r => { r.status = 500; }];
  for (const change of changes) {
    const f = fixture(t); f.setTime(300); const r = response(); change(r);
    assert.equal(f.writer.publishBook({marketId: '369', response: r}), false); assert.deepEqual(f.writer.status().books, {});
    assert.equal(f.writer.status().invalidations.markets['369'].reason, 'INVALID_BOOK');
  }
});

test('global and market outage barriers reject in-flight pre-invalidation requests even when receipt is later', t => {
  const f = fixture(t); f.setTime(300); f.writer.publishBook({marketId: '369', response: response()});
  f.setTime(400); f.writer.invalidate({reason: 'ACCOUNT_OUTAGE'}); assert.deepEqual(f.writer.status().books, {});
  f.setTime(600); assert.equal(f.writer.publishBook({marketId: '369', response: response({started: 350, received: 500, source: 450, sequence: 2})}), false);
  f.setTime(900); assert.equal(f.writer.publishBook({marketId: '369', response: response({started: 700, received: 800, source: 750, sequence: 3})}), true);
  f.setTime(1000); f.writer.invalidate({marketId: '369', reason: 'BOOK_READ_FAILED'}); assert.deepEqual(f.writer.status().books, {});
});

test('STOP before or during validation durably closes the session, clears books and returns false', t => {
  const f = fixture(t); f.setTime(300); f.writer.publishBook({marketId: '369', response: response()}); f.stop(); f.setTime(400);
  assert.equal(f.writer.publishBook({marketId: '369', response: response()}), false);
  assert.equal(f.writer.status().closed, true); assert.deepEqual(f.writer.status().books, {}); assert.equal(f.rows().at(-1).payload.reason, 'STOP');
  let checks = 0; const mid = fixture(t, {canOperate: () => ++checks === 1}); mid.setTime(300);
  assert.equal(mid.writer.publishBook({marketId: '369', response: response()}), false); assert.equal(mid.rows().length, 2);
  assert.equal(mid.rows()[1].kind, 'session_stop'); assert.equal(mid.writer.close(), false);
});

test('restart preserves chain while clearing older session books and refusing replayed old requests', t => {
  const f = fixture(t); f.setTime(300); f.writer.publishBook({marketId: '369', response: response()}); f.setTime(400); f.writer.close('NORMAL_SHUTDOWN');
  let clock = NOW + 500;
  const second = createBookPublication({file: f.file, universe, sessionId: 'session-2', now: () => clock});
  t.after(() => { if (!second.status().closed) second.close(); });
  assert.equal(second.status().sequence, 4); assert.deepEqual(second.status().books, {}); assert.equal(second.status().sessionId, 'session-2');
  clock = NOW + 700; assert.equal(second.publishBook({marketId: '369', response: response({started: 450, received: 600, source: 550, sequence: 2})}), false);
  clock = NOW + 1000; assert.equal(second.publishBook({marketId: '369', response: response({started: 800, received: 900, source: 850, sequence: 3})}), true);
  assert.deepEqual(replay(f.rows()), second.status()); second.close();
});

test('source rollback and same-version changed depth invalidate; unchanged fresh authoritative receipts remain valid', t => {
  const f = fixture(t); f.setTime(300); f.writer.publishBook({marketId: '369', response: response()});
  f.setTime(600); assert.equal(f.writer.publishBook({marketId: '369', response: response({started: 400, received: 500})}), true);
  f.setTime(900); assert.equal(f.writer.publishBook({marketId: '369', response: response({started: 700, received: 800, bid: 0.230})}), false);
  f.setTime(1200); assert.equal(f.writer.publishBook({marketId: '369', response: response({started: 1000, received: 1100, source: 129, sequence: 2})}), false);
  f.setTime(1500); assert.equal(f.writer.publishBook({marketId: '369', response: response({started: 1300, received: 1400, source: 1350, sequence: 0})}), false);
  f.setTime(1800); assert.equal(f.writer.publishBook({marketId: '369', response: response({started: 1600, received: 1700, source: 1650, sequence: 3})}), true);
});

test('same engine sequence cannot acquire a different source clock or changed depth', t => {
  for (const bid of [0.235, 0.230]) {
    const f = fixture(t); f.setTime(300); assert.equal(f.writer.publishBook({marketId: '369', response: response()}), true);
    f.setTime(600);
    assert.equal(f.writer.publishBook({marketId: '369', response: response({started: 400, received: 500, source: 450, sequence: 1, bid})}), false);
    assert.deepEqual(f.writer.status().books, {}); assert.equal(f.rows().at(-1).payload.reason, 'INVALID_BOOK');
  }
});

test('freshness and source skew bounds preserve raw timestamps without accepting future receipts', t => {
  for (const [r, accepted, now] of [[response({source: 330}), true, 300], [response({source: 2200}), true, 300],
    [response({source: 2201}), false, 300], [response({received: 301}), false, 300],
    [response({started: 100, received: 20101}), false, 20200], [response(), false, 20201]]) {
    const f = fixture(t); f.setTime(now); assert.equal(f.writer.publishBook({marketId: '369', response: r}), accepted);
  }
});

test('tamper, foreign scope, added fields, sequence gaps and malformed complete lines fail closed', t => {
  const f = fixture(t); f.setTime(300); f.writer.publishBook({marketId: '369', response: response()}); const rows = f.rows();
  for (const mutate of [r => { r.sequence++; }, r => { r.hash = '0'.repeat(64); }, r => { r.payload.book.bids[0].quantity++; }, r => { r.headers = {}; }]) {
    const changed = structuredClone(rows[1]); mutate(changed); assert.throws(() => replay([rows[0], changed]), /PUBLIC_BOOK/);
  }
  const foreign = structuredClone(rows[1]); foreign.payload.book.tournamentId = 'foreign'; assert.throws(() => replay([rows[0], rehash(foreign)]));
  const early = structuredClone(rows[1]); early.payload.book.requestStartedAt = at(-1); assert.throws(() => replay([rows[0], rehash(early)]));
  f.writer.close(); fs.appendFileSync(f.file, '{"incomplete":');
  assert.throws(() => createBookPublication({file: f.file, universe, sessionId: 'session-2', now: () => NOW + 1000}), /CHAIN_INVALID/);
});

test('short writes are completed and fsynced; failed durability cannot advance accepted state', t => {
  let syncs = 0, fail = false;
  const io = {...fs, writeSync(fd, bytes, offset, length) { return fs.writeSync(fd, bytes, offset, Math.min(length, 17)); },
    fsyncSync(fd) { if (fail) throw Error('sensitive OS detail'); syncs++; fs.fsyncSync(fd); }};
  const f = fixture(t, {io}); f.setTime(300); assert.equal(f.writer.publishBook({marketId: '369', response: response()}), true);
  assert.equal(syncs, 2); assert.deepEqual(replay(f.rows()), f.writer.status());
  const accepted = f.writer.status().sequence; fail = true; f.setTime(400);
  assert.throws(() => f.writer.invalidate({reason: 'ACCOUNT_OUTAGE'}), /^Error: PUBLIC_BOOK_DURABILITY_FAILURE$/);
  assert.equal(f.writer.status().sequence, accepted); assert.equal(f.writer.status().failed, true); assert.deepEqual(f.writer.status().books, {});
});

test('existing collector lease is supplemented by local writer and append-drift conflict guards', t => {
  const f = fixture(t);
  assert.throws(() => createBookPublication({file: f.file, universe, sessionId: 'another', now: () => NOW}), /WRITER_EXISTS/);
  fs.appendFileSync(f.file, '\n'); f.setTime(300);
  assert.throws(() => f.writer.publishBook({marketId: '369', response: response()}), /DURABILITY_FAILURE/);
  assert.equal(f.writer.status().failed, true);
});
