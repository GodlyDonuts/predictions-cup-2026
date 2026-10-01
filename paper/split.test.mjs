import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { chronologicalSplit } from './split.mjs';

const BASE = Date.parse('2026-10-01T00:00:00.000Z');
const at = (offset) => new Date(BASE + offset).toISOString();
const event = (eventId, receivedOffset, occurredOffset = receivedOffset, payload = {}) => ({
  schemaVersion: 1,
  eventId,
  receivedAt: at(receivedOffset),
  occurredAt: at(occurredOffset),
  ...payload,
});
const config = (overrides = {}) => ({
  trainEnd: at(100), validationEnd: at(200), labelHorizonMs: 0, embargoMs: 0, ...overrides,
});
const ids = (manifest) => Object.fromEntries(Object.entries(manifest.partitions)
  .map(([name, partition]) => [name, partition.eventIds]));

test('half-open cuts use availability, retain input order, and keep timestamp groups intact', () => {
  const events = [
    event('early', 0), event('before', 99), event('cut-a', 100, 1),
    event('cut-b', 100, 0), event('middle', 199, 10), event('test-a', 200, 1),
    event('test-b', 200, 0), event('later', 201, 11),
  ];
  const result = chronologicalSplit(events, config());
  assert.deepEqual(ids(result), {
    train: ['early', 'before'], validation: ['cut-a', 'cut-b', 'middle'],
    test: ['test-a', 'test-b', 'later'],
  });
  assert.deepEqual(result.counts, { source: 8, train: 2, validation: 3, test: 3, excluded: 0 });
  assert.equal(result.basis, 'receivedAt');
  assert.equal(result.partitions.train.startInclusive, null);
  assert.equal(result.partitions.train.endExclusive, at(100));
  assert.equal(result.partitions.validation.startInclusive, at(100));
  assert.equal(result.partitions.test.endExclusive, null);
});

test('label horizon equality is purged at each next nominal cut', () => {
  const events = [event('safe-train', 79), event('equal-train-a', 80), event('equal-train-b', 80),
    event('cross-train', 99), event('safe-validation', 179), event('equal-validation', 180),
    event('cross-validation', 199), event('test-label-unchecked', 200)];
  const result = chronologicalSplit(events, config({ labelHorizonMs: 20 }));
  assert.deepEqual(ids(result), {
    train: ['safe-train'], validation: ['safe-validation'], test: ['test-label-unchecked'],
  });
  assert.deepEqual(result.excluded.map(({ eventId, reasons }) => [eventId, reasons]), [
    ['equal-train-a', ['label-horizon:trainEnd']], ['equal-train-b', ['label-horizon:trainEnd']],
    ['cross-train', ['label-horizon:trainEnd']], ['equal-validation', ['label-horizon:validationEnd']],
    ['cross-validation', ['label-horizon:validationEnd']],
  ]);
  assert.match(result.semantics.claims, /no model training, label-maturity or performance claim/);
});

test('symmetric embargo includes left edge and excludes right edge around both cuts', () => {
  const events = [89, 90, 99, 100, 109, 110, 189, 190, 199, 200, 209, 210]
    .map((time) => event(`e${time}`, time));
  const result = chronologicalSplit(events, config({ embargoMs: 10 }));
  assert.deepEqual(ids(result), { train: ['e89'], validation: ['e110', 'e189'], test: ['e210'] });
  assert.deepEqual(result.embargoWindows, [
    { cut: 'trainEnd', startInclusive: at(90), endExclusive: at(110) },
    { cut: 'validationEnd', startInclusive: at(190), endExclusive: at(210) },
  ]);
  assert.deepEqual(result.excluded.map(({ eventId }) => eventId),
    ['e90', 'e99', 'e100', 'e109', 'e190', 'e199', 'e200', 'e209']);
});

test('purge and embargo exclusions can overlap and retain all reasons', () => {
  const result = chronologicalSplit([event('both', 95), event('purge-only', 175), event('safe', 210)],
    config({ labelHorizonMs: 25, embargoMs: 10 }));
  assert.deepEqual(result.excluded, [
    { eventId: 'both', receivedAt: at(95), nominalPartition: 'train', reasons: ['embargo:trainEnd', 'label-horizon:trainEnd'] },
    { eventId: 'purge-only', receivedAt: at(175), nominalPartition: 'validation', reasons: ['label-horizon:validationEnd'] },
  ]);
  assert.equal(result.counts.excluded, 2);
});

test('late-arriving old events cannot leak into earlier partitions', () => {
  const result = chronologicalSplit([event('train', 10), event('late', 210, 1)], config());
  assert.deepEqual(ids(result), { train: ['train'], validation: [], test: ['late'] });
});

test('one millisecond of nonempty validation time is permitted', () => {
  const result = chronologicalSplit([event('valid', 101)],
    config({ validationEnd: at(104), labelHorizonMs: 2, embargoMs: 1 }));
  assert.deepEqual(result.partitions.validation.eventIds, ['valid']);
});

test('empty input and sparse partitions remain explicit and JSON-safe', () => {
  const result = chronologicalSplit([], config());
  assert.deepEqual(result.counts, { source: 0, train: 0, validation: 0, test: 0, excluded: 0 });
  assert.equal(result.source.firstReceivedAt, null);
  assert.equal(result.source.lastReceivedAt, null);
  assert.equal(result.source.checksum, createHash('sha256').update('[]').digest('hex'));
  assert.deepEqual(JSON.parse(JSON.stringify(result)), result);
});

test('manifest is deterministic, JSON-safe and never mutates frozen inputs', () => {
  const row = Object.freeze(event('a', 1, 0, { extra: Object.freeze({ nested: Object.freeze([1, null, true]) }) }));
  const events = Object.freeze([row]);
  const options = Object.freeze(config());
  const result = chronologicalSplit(events, options);
  assert.deepEqual(result, chronologicalSplit(events, options));
  assert.deepEqual(JSON.parse(JSON.stringify(result)), result);
  result.partitions.train.eventIds.push('new');
  assert.deepEqual(chronologicalSplit(events, options).partitions.train.eventIds, ['a']);
});

test('canonical checksum ignores object insertion order recursively, including numeric keys', () => {
  const first = event('a', 1, 0, { payload: { z: [{ '2': 2, '10': 10, a: null }], a: 'µ' } });
  const second = { payload: { a: 'µ', z: [{ a: null, '10': 10, '2': 2 }] },
    occurredAt: at(0), receivedAt: at(1), eventId: 'a', schemaVersion: 1 };
  const result = chronologicalSplit([first], config());
  assert.equal(result.source.checksum, chronologicalSplit([second], config()).source.checksum);
  const canonical = `[{"eventId":"a","occurredAt":"${at(0)}","payload":{"a":"µ","z":[{"10":10,"2":2,"a":null}]},"receivedAt":"${at(1)}","schemaVersion":1}]`;
  assert.equal(result.source.checksum, createHash('sha256').update(canonical, 'utf8').digest('hex'));
  assert.equal(result.source.checksumEncoding, 'canonical-json-v1');
  assert.equal(result.source.checksumAlgorithm, 'sha256');
});

test('source checksum covers excluded events, complete payloads, and original tied-row order', () => {
  const first = event('a', 95, 1, { payload: { price: '0.10' } });
  const second = event('b', 95, 1);
  const settings = config({ embargoMs: 10 });
  const original = chronologicalSplit([first, second], settings);
  assert.equal(original.counts.excluded, 2);
  assert.notEqual(original.source.checksum, chronologicalSplit([second, first], settings).source.checksum);
  assert.notEqual(original.source.checksum,
    chronologicalSplit([{ ...first, payload: { price: '0.11' } }, second], settings).source.checksum);
  assert.equal(original.source.checksum, chronologicalSplit([first, second], config()).source.checksum);
});

test('shared nested JSON references are accepted while cycles are rejected', () => {
  const shared = { x: 1 };
  assert.doesNotThrow(() => chronologicalSplit([event('a', 1, 0, { payload: [shared, shared] })], config()));
  const cyclic = {};
  cyclic.self = cyclic;
  assert.throws(() => chronologicalSplit([event('a', 1, 0, { payload: cyclic })], config()), /cycles/);
});

test('all options must be explicit, with no misspelled or ambiguous extra fields', () => {
  for (const key of Object.keys(config())) {
    const options = config();
    delete options[key];
    assert.throws(() => chronologicalSplit([], options), /options must contain exactly/);
  }
  for (const options of [null, undefined, [], 1, new Date(), Object.create({ ...config() })]) {
    assert.throws(() => chronologicalSplit([], options), /plain JSON object/);
  }
  assert.throws(() => chronologicalSplit([], { ...config(), embargo: 0 }), /options must contain exactly/);
});

test('durations reject negatives, fractional, unsafe, absent and nonnumeric values', () => {
  for (const name of ['labelHorizonMs', 'embargoMs']) {
    for (const value of [-1, 0.1, Number.MAX_SAFE_INTEGER + 1, NaN, Infinity, '1', null, undefined, 1n]) {
      assert.throws(() => chronologicalSplit([], config({ [name]: value })), TypeError);
    }
  }
});

test('cuts must be strictly ordered and leave a nonempty purged, embargoed validation interval', () => {
  assert.throws(() => chronologicalSplit([], config({ validationEnd: at(100) })), /must precede/);
  assert.throws(() => chronologicalSplit([], config({ validationEnd: at(99) })), /must precede/);
  for (const embargoMs of [50, 51]) {
    assert.throws(() => chronologicalSplit([], config({ embargoMs })), /overlap or leave no validation/);
  }
  for (const settings of [{ labelHorizonMs: 100 }, { labelHorizonMs: 90, embargoMs: 10 }]) {
    assert.throws(() => chronologicalSplit([], config(settings)), /no leakage-free validation/);
  }
});

test('time arithmetic overflow and unsupported date boundaries fail closed', () => {
  assert.throws(() => chronologicalSplit([], config({ embargoMs: Number.MAX_SAFE_INTEGER })), /timestamp range/);
  assert.throws(() => chronologicalSplit([], config({ labelHorizonMs: Number.MAX_SAFE_INTEGER })), /timestamp range/);
  assert.throws(() => chronologicalSplit([], config({
    trainEnd: '0000-01-01T00:00:00.000Z', validationEnd: '0000-01-01T00:00:00.100Z', embargoMs: 1,
  })), /timestamp range/);
  assert.throws(() => chronologicalSplit([], config({ validationEnd: '9999-12-31T23:59:59.999Z', embargoMs: 1 })), /timestamp range/);
  assert.throws(() => chronologicalSplit([{
    schemaVersion: 1, eventId: 'end', receivedAt: '9999-12-31T23:59:59.999Z', occurredAt: '9999-12-31T23:59:59.999Z',
  }], config({ labelHorizonMs: 1 })), /label horizon.*timestamp range/);
});

test('noncanonical or impossible timestamps are rejected in cuts and events', () => {
  for (const value of ['2026-10-01', '2026-10-01T00:00:00Z', '2026-10-01T00:00:00.000+00:00',
    '2026-02-30T00:00:00.000Z', '2026-13-01T00:00:00.000Z', '2026-10-01T24:00:00.000Z',
    '2026-10-01T00:00:60.000Z', '2026-10-01T00:00:00.0000Z', '+010000-01-01T00:00:00.000Z', 0, null]) {
    for (const key of ['trainEnd', 'validationEnd']) {
      assert.throws(() => chronologicalSplit([], config({ [key]: value })), TypeError);
    }
    for (const key of ['receivedAt', 'occurredAt']) {
      assert.throws(() => chronologicalSplit([{ ...event('a', 1), [key]: value }], config()), TypeError);
    }
  }
});

test('invalid rows, IDs, schema, timing and unsorted availability are rejected', () => {
  for (const events of [null, {}, 'events']) assert.throws(() => chronologicalSplit(events, config()), /must be an array/);
  for (const row of [null, [], 1, new Date()]) assert.throws(() => chronologicalSplit([row], config()), TypeError);
  for (const key of ['schemaVersion', 'eventId', 'receivedAt', 'occurredAt']) {
    const row = event('a', 1);
    delete row[key];
    assert.throws(() => chronologicalSplit([row], config()), /is required/);
  }
  for (const schemaVersion of [0, 2, '1', null]) {
    assert.throws(() => chronologicalSplit([{ ...event('a', 1), schemaVersion }], config()), /schemaVersion must be 1/);
  }
  for (const eventId of ['', '  ', 1, null]) {
    assert.throws(() => chronologicalSplit([{ ...event('a', 1), eventId }], config()), /eventId must be a nonempty/);
  }
  assert.throws(() => chronologicalSplit([event('a', 1), event('a', 2)], config()), /duplicate eventId/);
  assert.throws(() => chronologicalSplit([event('a', 2), event('b', 1)], config()), /sorted by receivedAt/);
  assert.throws(() => chronologicalSplit([event('a', 1, 2)], config()), /occurredAt cannot follow receivedAt/);
});

test('non-JSON payloads are rejected instead of silently changing checksum input', () => {
  for (const payload of [undefined, NaN, Infinity, 1n, () => {}, Symbol('x'), new Date(), new Map(), /x/]) {
    assert.throws(() => chronologicalSplit([event('a', 1, 0, { payload })], config()), TypeError);
  }
  const sparse = new Array(1);
  const extra = [1];
  extra.extra = 2;
  const missing = new Array(1);
  missing.extra = 2;
  for (const payload of [sparse, extra, missing]) {
    assert.throws(() => chronologicalSplit([event('a', 1, 0, { payload })], config()), /dense JSON array/);
  }
  const withSymbol = { [Symbol('hidden')]: 1 };
  const nonenumerable = Object.defineProperty({}, 'hidden', { value: 1 });
  let getterCalls = 0;
  const getter = Object.defineProperty({}, 'value', { enumerable: true, get() { getterCalls += 1; return 1; } });
  for (const payload of [withSymbol, nonenumerable, getter]) {
    assert.throws(() => chronologicalSplit([event('a', 1, 0, { payload })], config()), TypeError);
  }
  assert.equal(getterCalls, 0);
});

test('null-prototype objects and prototype-named event IDs remain safe', () => {
  const row = Object.assign(Object.create(null), event('__proto__', 1));
  row.payload = JSON.parse('{"__proto__":{"ok":true},"constructor":"value"}');
  const result = chronologicalSplit([row, event('constructor', 100)], config());
  assert.deepEqual(result.partitions.train.eventIds, ['__proto__']);
  assert.deepEqual(result.partitions.validation.eventIds, ['constructor']);
});

test('deterministic coverage, disjointness, grouping, and label separation over many configurations', () => {
  const events = Array.from({ length: 601 }, (_, index) => event(`e${index}`, Math.floor(index / 2), 0));
  for (const labelHorizonMs of [0, 1, 10, 35]) {
    for (const embargoMs of [0, 1, 10, 25]) {
      const result = chronologicalSplit(events, config({ labelHorizonMs, embargoMs }));
      const assignments = new Map();
      for (const [name, partition] of Object.entries(result.partitions)) {
        for (const id of partition.eventIds) {
          assert.equal(assignments.has(id), false);
          assignments.set(id, name);
          const received = Math.floor(Number(id.slice(1)) / 2);
          for (const cut of [100, 200]) assert.equal(received >= cut - embargoMs && received < cut + embargoMs, false);
          if (name === 'train') assert.ok(received + labelHorizonMs < 100);
          if (name === 'validation') {
            assert.ok(received >= 100);
            assert.ok(received + labelHorizonMs < 200);
          }
          if (name === 'test') assert.ok(received >= 200);
        }
      }
      for (const { eventId } of result.excluded) {
        assert.equal(assignments.has(eventId), false);
        assignments.set(eventId, 'excluded');
      }
      assert.equal(assignments.size, events.length);
      assert.equal(result.counts.train + result.counts.validation + result.counts.test + result.counts.excluded, events.length);
      for (let index = 0; index < events.length - 1; index += 2) {
        assert.equal(assignments.get(`e${index}`), assignments.get(`e${index + 1}`));
      }
    }
  }
});
