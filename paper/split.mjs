import { createHash } from 'node:crypto';

const UTC_MILLIS = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/;
const MIN_TIME = Date.parse('0000-01-01T00:00:00.000Z');
const MAX_TIME = Date.parse('9999-12-31T23:59:59.999Z');
const OPTION_KEYS = ['embargoMs', 'labelHorizonMs', 'trainEnd', 'validationEnd'];

function timestamp(value, name) {
  if (typeof value !== 'string' || !UTC_MILLIS.test(value)) {
    throw new TypeError(`${name} must be a canonical UTC timestamp with milliseconds`);
  }
  const parsed = Date.parse(value);
  if (!Number.isFinite(parsed) || new Date(parsed).toISOString() !== value) {
    throw new TypeError(`${name} must be a valid canonical UTC timestamp`);
  }
  return parsed;
}

function duration(value, name) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new TypeError(`${name} must be a nonnegative safe integer in milliseconds`);
  }
  return value;
}

function addTime(time, delta, name) {
  const result = time + delta;
  if (!Number.isSafeInteger(result) || result < MIN_TIME || result > MAX_TIME) {
    throw new RangeError(`${name} exceeds the supported timestamp range`);
  }
  return result;
}

function plainObject(value, name) {
  if (value === null || typeof value !== 'object' || Array.isArray(value)
      || ![Object.prototype, null].includes(Object.getPrototypeOf(value))) {
    throw new TypeError(`${name} must be a plain JSON object`);
  }
}

// canonical-json-v1: compact JSON, recursively sorted object keys (UTF-16 order),
// array order preserved, standard JSON scalar encoding, no trailing newline.
// Reject values JSON.stringify would silently drop or transform ambiguously.
function canonicalJSON(value, ancestors = new Set(), path = 'events') {
  if (value === null || typeof value === 'string' || typeof value === 'boolean') {
    return JSON.stringify(value);
  }
  if (typeof value === 'number' && Number.isFinite(value)) return JSON.stringify(value);
  if (typeof value !== 'object' || value === null) {
    throw new TypeError(`${path} must contain only finite JSON values`);
  }
  if (ancestors.has(value)) throw new TypeError(`${path} must not contain cycles`);
  const isArray = Array.isArray(value);
  if (!isArray) plainObject(value, path);
  if (Object.getOwnPropertySymbols(value).length !== 0) {
    throw new TypeError(`${path} must not contain symbol properties`);
  }
  const descriptors = Object.getOwnPropertyDescriptors(value);
  for (const [key, descriptor] of Object.entries(descriptors)) {
    if (isArray && key === 'length') continue;
    if (!descriptor.enumerable || !Object.hasOwn(descriptor, 'value')) {
      throw new TypeError(`${path}.${key} must be an enumerable JSON value, not an accessor`);
    }
  }
  ancestors.add(value);
  let result;
  if (isArray) {
    if (Object.keys(value).length !== value.length) {
      throw new TypeError(`${path} must be a dense JSON array without extra properties`);
    }
    const parts = [];
    for (let index = 0; index < value.length; index += 1) {
      if (!Object.hasOwn(value, index)) {
        throw new TypeError(`${path} must be a dense JSON array`);
      }
      parts.push(canonicalJSON(value[index], ancestors, `${path}[${index}]`));
    }
    result = `[${parts.join(',')}]`;
  } else {
    result = `{${Object.keys(value).sort().map((key) => (
      `${JSON.stringify(key)}:${canonicalJSON(value[key], ancestors, `${path}.${key}`)}`
    )).join(',')}}`;
  }
  ancestors.delete(value);
  return result;
}

/**
 * Produce an offline, JSON-safe availability-time split manifest; never train a model.
 * All four options are required. Windows are train t < trainEnd, validation
 * trainEnd <= t < validationEnd, test t >= validationEnd, using receivedAt only.
 * Exclude symmetric embargo intervals [cut - embargoMs, cut + embargoMs), then
 * purge train/validation rows whose closed label interval [t, t + horizon] reaches
 * their next nominal cut (equality is purged). Equal-time groups stay together.
 * Input must be sorted by receivedAt, unique by eventId, and contain plain JSON
 * schemaVersion:1 rows with occurredAt <= receivedAt. Keys are canonically sorted
 * for the SHA256 of ALL source events; tied-event input order remains significant.
 * This checks temporal assignment, not feature causality, actual label availability,
 * correct horizon choice, independent markets, or predictive performance.
 */
export function chronologicalSplit(events, options) {
  if (!Array.isArray(events)) throw new TypeError('events must be an array');
  plainObject(options, 'options');
  // Validation here also prevents silent getters, missing values and ignored keys.
  canonicalJSON(options, new Set(), 'options');
  if (Object.keys(options).sort().join(',') !== OPTION_KEYS.join(',')) {
    throw new TypeError('options must contain exactly trainEnd, validationEnd, labelHorizonMs, embargoMs');
  }
  const { trainEnd, validationEnd, labelHorizonMs, embargoMs } = options;
  const trainCut = timestamp(trainEnd, 'trainEnd');
  const validationCut = timestamp(validationEnd, 'validationEnd');
  duration(labelHorizonMs, 'labelHorizonMs');
  duration(embargoMs, 'embargoMs');
  if (trainCut >= validationCut) throw new RangeError('trainEnd must precede validationEnd');

  const cuts = [
    { name: 'trainEnd', time: trainCut },
    { name: 'validationEnd', time: validationCut },
  ].map(({ name, time }) => ({
    name,
    time,
    start: addTime(time, -embargoMs, `${name} minus embargoMs`),
    end: addTime(time, embargoMs, `${name} plus embargoMs`),
    purgeBefore: addTime(time, -labelHorizonMs, `${name} minus labelHorizonMs`),
  }));
  if (cuts[0].end >= cuts[1].start) {
    throw new RangeError('embargo windows overlap or leave no validation interval');
  }
  if (cuts[0].end >= cuts[1].purgeBefore) {
    throw new RangeError('labelHorizonMs and embargoMs leave no leakage-free validation interval');
  }

  const sourceJSON = canonicalJSON(events);
  const seenIds = new Set();
  let previous = -Infinity;
  const parsedEvents = events.map((event, index) => {
    plainObject(event, `events[${index}]`);
    for (const key of ['schemaVersion', 'eventId', 'receivedAt', 'occurredAt']) {
      if (!Object.hasOwn(event, key)) throw new TypeError(`events[${index}].${key} is required`);
    }
    if (event.schemaVersion !== 1) throw new TypeError(`events[${index}].schemaVersion must be 1`);
    if (typeof event.eventId !== 'string' || event.eventId.trim() === '') {
      throw new TypeError(`events[${index}].eventId must be a nonempty string`);
    }
    if (seenIds.has(event.eventId)) throw new TypeError(`duplicate eventId: ${event.eventId}`);
    seenIds.add(event.eventId);
    const received = timestamp(event.receivedAt, `events[${index}].receivedAt`);
    const occurred = timestamp(event.occurredAt, `events[${index}].occurredAt`);
    if (occurred > received) throw new RangeError(`events[${index}].occurredAt cannot follow receivedAt`);
    if (received < previous) throw new RangeError('events must be sorted by receivedAt ascending');
    previous = received;
    const labelEnd = addTime(received, labelHorizonMs, `events[${index}] label horizon`);
    return { event, received, labelEnd };
  });

  const partitions = {
    train: { startInclusive: null, endExclusive: trainEnd, eventIds: [] },
    validation: { startInclusive: trainEnd, endExclusive: validationEnd, eventIds: [] },
    test: { startInclusive: validationEnd, endExclusive: null, eventIds: [] },
  };
  const excluded = [];
  for (const { event, received, labelEnd } of parsedEvents) {
    const nominalPartition = received < trainCut ? 'train' : received < validationCut ? 'validation' : 'test';
    const reasons = cuts.filter((cut) => received >= cut.start && received < cut.end)
      .map((cut) => `embargo:${cut.name}`);
    if (nominalPartition === 'train' && labelEnd >= trainCut) reasons.push('label-horizon:trainEnd');
    if (nominalPartition === 'validation' && labelEnd >= validationCut) reasons.push('label-horizon:validationEnd');
    if (reasons.length > 0) {
      excluded.push({ eventId: event.eventId, receivedAt: event.receivedAt, nominalPartition, reasons });
    } else {
      partitions[nominalPartition].eventIds.push(event.eventId);
    }
  }

  return {
    schemaVersion: 1,
    kind: 'chronological-split',
    basis: 'receivedAt',
    parameters: { trainEnd, validationEnd, labelHorizonMs, embargoMs },
    source: {
      checksumAlgorithm: 'sha256',
      checksumEncoding: 'canonical-json-v1',
      checksum: createHash('sha256').update(sourceJSON, 'utf8').digest('hex'),
      eventCount: events.length,
      firstReceivedAt: events[0]?.receivedAt ?? null,
      lastReceivedAt: events.at(-1)?.receivedAt ?? null,
    },
    semantics: {
      partitionIntervals: '[startInclusive, endExclusive); null means unbounded',
      embargoIntervals: '[startInclusive, endExclusive); zero width excludes nothing',
      labelInterval: '[receivedAt, receivedAt + labelHorizonMs]',
      purge: 'Exclude train/validation rows when label end >= the next nominal cut',
      timestampGroups: 'All rows with the same receivedAt share one assignment or exclusion',
      claims: 'Split assignment only; no model training, label-maturity or performance claim',
    },
    embargoWindows: cuts.map((cut) => ({
      cut: cut.name,
      startInclusive: new Date(cut.start).toISOString(),
      endExclusive: new Date(cut.end).toISOString(),
    })),
    partitions,
    excluded,
    counts: {
      source: events.length,
      train: partitions.train.eventIds.length,
      validation: partitions.validation.eventIds.length,
      test: partitions.test.eventIds.length,
      excluded: excluded.length,
    },
  };
}
