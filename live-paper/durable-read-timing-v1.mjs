import fs from 'node:fs';
import path from 'node:path';
import {validateTiming} from './read-timing.mjs';
import {atomicJson} from '../execution/ledger.mjs';

const schemaCauses = new Map([
  ['Invalid timing schema', 'TIMING_SCHEMA'],
  ['Invalid timing clock', 'TIMING_CLOCK'],
  ['Invalid timing admission', 'TIMING_ADMISSION'],
  ['Invalid timing duration', 'TIMING_DURATION'],
  ['Invalid timing status', 'TIMING_STATUS'],
  ['Invalid timing instrumentation', 'TIMING_INSTRUMENTATION'],
  ['Invalid event-loop duration', 'TIMING_EVENT_LOOP'],
  ['Invalid event-loop quantiles', 'TIMING_QUANTILES'],
]);
const transientOpenCodes = new Set(['EBUSY', 'EACCES', 'EPERM']);
const safeIoCodes = new Set(['EBUSY', 'EACCES', 'EPERM', 'EIO', 'ENOSPC']);
const sinks = new Map();
let spillSequence = 0;

function safeCode(error) {
  return schemaCauses.get(error?.message)
    ?? (safeIoCodes.has(error?.code) ? error.code
      : error?.message === 'TIMING_SHORT_WRITE' ? 'TIMING_SHORT_WRITE'
        : 'TIMING_PERSISTENCE_FAILED');
}

function sidecars(filesystem, file) {
  const directory = path.dirname(file);
  const stem = path.basename(file).replace(/\.jsonl$/i, '');
  const pattern = new RegExp(`^${stem.replace(/[.*+?^${}()|[\\]\\]/g, '\\$&')}\\.spill-\\d{17}-\\d+-[a-z0-9]+\\.jsonl$`);
  return filesystem.readdirSync(directory).filter(name => pattern.test(name)).sort();
}

function safeEventMetadata(event, bytesWritten) {
  return {
    code: null,
    bytesWritten,
    schemaVersion: Number.isSafeInteger(event?.schemaVersion) ? event.schemaVersion : null,
    outcome: ['success', 'blocked', 'connection_error', 'http_error', 'body_error', 'aborted'].includes(event?.outcome) ? event.outcome : null,
    statusCode: Number.isInteger(event?.statusCode) ? event.statusCode : null,
    eventLoopSamples: Number.isSafeInteger(event?.eventLoopSamples) ? event.eventLoopSamples : null,
    eventLoopP99Ms: Number.isFinite(event?.eventLoopP99Ms) ? event.eventLoopP99Ms : null,
    eventLoopMaxMs: Number.isFinite(event?.eventLoopMaxMs) ? event.eventLoopMaxMs : null,
  };
}

function writeDiagnostic(diagnosticFile, error, event, bytesWritten, primaryCode) {
  const safe = safeEventMetadata(event, bytesWritten);
  safe.code = safeCode(error);
  if (primaryCode) safe.primaryCode = safeIoCodes.has(primaryCode) ? primaryCode : 'TIMING_PERSISTENCE_FAILED';
  try { atomicJson(diagnosticFile, {schemaVersion: 1, at: new Date().toISOString(), ...safe}); } catch {}
}

function spillPath(filesystem, file, reason) {
  const directory = path.dirname(file), base = path.basename(file).replace(/\.jsonl$/i, '');
  for (let attempt = 0; attempt < 32; attempt++) {
    const stamp = new Date().toISOString().replace(/\D/g, '');
    const sequence = ++spillSequence;
    const target = path.join(directory, `${base}.spill-${stamp}-${process.pid}-${sequence}.jsonl`);
    try {
      const fd = filesystem.openSync(target, 'wx', 0o600);
      return {sink: 'spill', destination: target, fd, reason};
    } catch (error) {
      if (error?.code !== 'EEXIST') throw error;
    }
  }
  throw Object.assign(Error('TIMING_SPILL_NAME_EXHAUSTED'), {code: 'TIMING_SPILL_NAME_EXHAUSTED'});
}

function openAppend(filesystem, destination, pause) {
  for (let attempt = 0; attempt < 7; attempt++) {
    try { return filesystem.openSync(destination, 'a', 0o600); }
    catch (error) {
      if (!transientOpenCodes.has(error?.code) || attempt === 6) throw error;
      pause(Math.min(20 * 2 ** attempt, 500));
    }
  }
}

function selectSink({filesystem, file, pause}) {
  // Once a side segment exists, keep this process on a new segment. This
  // preserves append chronology across collector restarts without rewriting
  // either the primary journal or any prior spill.
  const prior = sidecars(filesystem, file);
  if (prior.length) return spillPath(filesystem, file, 'ROLLOVER');

  try { return {sink: 'primary', destination: file, fd: openAppend(filesystem, file, pause)}; }
  catch (error) {
    if (!transientOpenCodes.has(error?.code)) throw error;
    try { return {...spillPath(filesystem, file, error.code), primaryCode: error.code}; }
    catch (spillError) { spillError.primaryCode = error.code; throw spillError; }
  }
}

export function appendReadTimingV1({file, diagnosticFile, event, filesystem = fs, pause = ms => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms)}) {
  const key = path.resolve(file);
  let state = sinks.get(key);
  let fd = null, written = 0, failure = null, primaryCode = null;

  try {
    validateTiming(event);
    const bytes = Buffer.from(JSON.stringify(event) + '\n');
    if (state?.poisoned) throw state.poisoned;
    if (!state) {
      state = selectSink({filesystem, file, pause});
      sinks.set(key, state);
      fd = state.fd;
      state.fd = null;
      primaryCode = state.primaryCode ?? null;
    } else {
      primaryCode = state.primaryCode ?? null;
      try { fd = openAppend(filesystem, state.destination, pause); }
      catch (error) {
        if (!transientOpenCodes.has(error?.code)) throw error;
        const previousSink = state.sink;
        try {
          const next = spillPath(filesystem, file, previousSink === 'primary' ? error.code : 'ROLLOVER');
          state.sink = next.sink;
          state.destination = next.destination;
          state.primaryCode = previousSink === 'primary' ? error.code : state.primaryCode;
          fd = next.fd;
          primaryCode = state.primaryCode ?? null;
        } catch (spillError) {
          spillError.primaryCode = previousSink === 'primary' ? error.code : state.primaryCode;
          throw spillError;
        }
      }
    }

    while (written < bytes.length) {
      const count = filesystem.writeSync(fd, bytes, written, bytes.length - written);
      if (!Number.isInteger(count) || count < 1 || count > bytes.length - written) throw Error('TIMING_SHORT_WRITE');
      written += count;
    }
    filesystem.fsyncSync(fd);
  } catch (error) {
    failure = error;
  }

  if (fd !== null) {
    try { filesystem.closeSync(fd); }
    catch (error) { if (!failure) failure = error; }
  }

  if (failure) {
    const code = safeCode(failure);
    const fatal = Object.assign(Error(code), {code, fatal: true, durableRecordingFailed: true});
    if (state) state.poisoned = fatal;
    else {
      state = {sink: 'failed', destination: file, poisoned: fatal, primaryCode: failure.primaryCode ?? primaryCode};
      sinks.set(key, state);
    }
    if (!state.diagnosticWritten) {
      writeDiagnostic(diagnosticFile, failure, event, written, state.primaryCode ?? failure.primaryCode ?? primaryCode);
      state.diagnosticWritten = true;
    }
    throw fatal;
  }
  return {durable: true, sink: state.sink};
}
