import fs from 'node:fs';
import path from 'node:path';
import {createHash, randomUUID} from 'node:crypto';
import {journalLines} from './journal-lines.mjs';

const fail = code => Object.assign(new Error(code), { code });

export function journalSnapshotV1(file) {
  const stat = fs.statSync(file, { bigint: true });
  const bytes = Number(stat.size);
  if (!Number.isSafeInteger(bytes)) throw fail('MAKER_JOURNAL_SIZE_UNSAFE');
  return {
    dev: String(stat.dev),
    ino: String(stat.ino),
    bytes,
    stamp: [stat.dev, stat.ino, stat.size, stat.mtimeNs, stat.ctimeNs]
      .map(String)
      .join(':'),
  };
}

const lockIdentity = stat => [stat.dev, stat.ino, stat.birthtimeMs, stat.size].join(':');

export function paperReplaySupervisorLeaseV1(file, pid = process.pid) {
  let content, record, stat;
  try {
    content = fs.readFileSync(file);
    record = JSON.parse(content.toString('utf8'));
    stat = fs.statSync(file, {bigint: true});
  } catch {
    throw fail('MAKER_SUPERVISOR_LEASE_UNAVAILABLE');
  }
  if (record.pid !== pid || record.mode !== 'local_quote_replay_only' || record.apiClients !== 0) {
    throw fail('MAKER_SUPERVISOR_LEASE_REQUIRED');
  }
  const identity = createHash('sha256')
    .update([stat.dev, stat.ino, stat.birthtimeNs, stat.mtimeNs, stat.size].map(String).join(':'))
    .update('\0')
    .update(content)
    .digest('hex');
  return {file: path.resolve(file), pid, identity};
}

function assertSupervisorLease(lease) {
  if (!lease || typeof lease.file !== 'string' || lease.pid !== process.pid || typeof lease.identity !== 'string') {
    throw fail('MAKER_SUPERVISOR_LEASE_REQUIRED');
  }
  const current = paperReplaySupervisorLeaseV1(lease.file, lease.pid);
  if (current.identity !== lease.identity) throw fail('MAKER_SUPERVISOR_LEASE_CHANGED');
  return current;
}

export function assertPaperReplaySupervisorLeaseV1(lease) {
  return assertSupervisorLease(lease);
}

function reclaimPreviousGenerationLock(file, {supervisorLease, now, malformedGraceMs}) {
  let stat, text;
  try {
    stat = fs.statSync(file);
    text = fs.readFileSync(file, 'utf8');
  } catch (error) {
    if (error.code === 'ENOENT') return true;
    throw error;
  }

  let owner;
  try { owner = JSON.parse(text); } catch {}
  // Older journal locks used the runtime lock format: a JSON number containing
  // only the PID. JSON.parse returns that number directly, so handle it before
  // treating the lock as malformed. If it names this supervisor, its age is
  // not enough to prove it abandoned; fail closed instead of stealing it.
  if (typeof owner === 'number' && Number.isSafeInteger(owner) && owner > 0) {
    owner = {pid: owner, legacy: true};
  }
  const validPriorLease =
    (owner?.legacy === true && Number.isSafeInteger(owner.pid) && owner.pid > 0) ||
    (owner?.schemaVersion === 1 && Number.isSafeInteger(owner.pid) && owner.pid > 0 &&
      typeof owner.nonce === 'string' && owner.nonce.length >= 16);
  if (owner?.supervisorIdentity === supervisorLease.identity) {
    throw fail('MAKER_JOURNAL_WRITER_ACTIVE');
  }
  if (owner?.legacy === true && owner.pid === supervisorLease.pid) {
    throw fail('MAKER_JOURNAL_WRITER_ACTIVE');
  }
  if (!validPriorLease && typeof owner?.supervisorIdentity !== 'string' && now() - stat.mtimeMs < malformedGraceMs) {
    throw fail('MAKER_JOURNAL_LOCK_INCOMPLETE');
  }

  // V14 can run only inside the worker's exclusive supervisor lock. Therefore
  // a lease from any previous supervisor generation is stale even if its PID
  // has been reused. Rechecking the supervisor plus journal inode avoids the
  // check/unlink race between ordinary active workers: at most one process can
  // hold this unchanged outer lease.
  assertSupervisorLease(supervisorLease);
  try {
    const current = fs.statSync(file);
    if (lockIdentity(current) !== lockIdentity(stat) || fs.readFileSync(file, 'utf8') !== text) {
      throw fail('MAKER_JOURNAL_LOCK_CHANGED');
    }
    fs.unlinkSync(file);
    return true;
  } catch (error) {
    if (error.code === 'ENOENT') return true;
    throw error;
  }
}

export function acquireMakerJournalWriterV1(
  file,
  {supervisorLease, now = Date.now, malformedGraceMs = 30000} = {},
) {
  if (!Number.isSafeInteger(malformedGraceMs) || malformedGraceMs < 0) {
    throw fail('MAKER_JOURNAL_LOCK_OPTIONS');
  }
  assertSupervisorLease(supervisorLease);
  fs.mkdirSync(path.dirname(file), {recursive: true});
  for (let attempt = 0; attempt < 3; attempt++) {
    let fd;
    try {
      fd = fs.openSync(file, 'wx', 0o600);
    } catch (error) {
      if (error.code !== 'EEXIST') throw error;
      reclaimPreviousGenerationLock(file, {supervisorLease, now, malformedGraceMs});
      continue;
    }
    let token;
    try {
      token = JSON.stringify({schemaVersion: 1, pid: process.pid, startedAt: new Date(now()).toISOString(), nonce: randomUUID(), supervisorIdentity: supervisorLease.identity});
      fs.writeFileSync(fd, token);
      fs.fsyncSync(fd);
      const claim = fs.fstatSync(fd);
      assertSupervisorLease(supervisorLease);
      let released = false;
      return () => {
        if (released) return;
        released = true;
        let owns = false;
        try {
          assertSupervisorLease(supervisorLease);
          const current = fs.statSync(file);
          owns = lockIdentity(current) === lockIdentity(claim) && fs.readFileSync(file, 'utf8') === token;
        } catch (error) {
          if (error.code !== 'ENOENT') throw error;
        } finally {
          fs.closeSync(fd);
        }
        if (!owns) throw fail('MAKER_JOURNAL_LOCK_CHANGED');
        fs.unlinkSync(file);
      };
    } catch (error) {
      try { fs.closeSync(fd); } catch {}
      try {
        if (fs.readFileSync(file, 'utf8') === token) fs.unlinkSync(file);
      } catch {}
      throw error;
    }
  }
  throw fail('MAKER_JOURNAL_LOCK_BUSY');
}

// Apply only a bounded append whose rows extend the exact verified hash head.
// The caller retains its old cache if validation fails.
export function refreshCachedMakerJournalV1(file, cached) {
  if (
    !cached?.journal ||
    !Number.isSafeInteger(cached.bytes) ||
    cached.bytes < 0 ||
    typeof cached.stamp !== 'string'
  ) throw fail('MAKER_JOURNAL_CACHE_INVALID');

  const before = journalSnapshotV1(file);
  // Integrity contract: this verifies only appended rows against the cached
  // hash head. It does not re-hash or prove the on-disk prefix is unchanged;
  // an out-of-protocol writer can mutate a prior row (with or without a later
  // append) and leave this cache stale. The metadata fast path therefore
  // depends on the exclusive supervisor/cohort leases and a trusted
  // append-only writer. General tamper detection needs a reliable filesystem
  // change token or a full prefix verification, neither of which is provided
  // by this paper-only candidate.
  if (before.stamp === cached.stamp) {
    return { ...cached, ...before, integrityMode: 'append_suffix_trusted_writer', refreshed: false, appendedRecords: 0 };
  }
  if (
    before.dev !== cached.dev ||
    before.ino !== cached.ino ||
    before.bytes < cached.bytes
  ) throw fail('MAKER_JOURNAL_REPLACED');
  if (before.bytes === cached.bytes) throw fail('MAKER_JOURNAL_MUTATED_WITHOUT_APPEND');
  const journal = Object.assign(Object.create(Object.getPrototypeOf(cached.journal)), cached.journal);
  let state = cached.state;
  let bytes = cached.bytes;
  let appendedRecords = 0;

  try {
    for (const line of journalLines(file, {
      startByte: cached.bytes,
      endByte: before.bytes,
    })) {
      let record;
      try { record = JSON.parse(line); }
      catch { throw fail('MAKER_JOURNAL_APPEND_JSON'); }
      const { hash, ...body } = record ?? {};
      if (
        !Number.isSafeInteger(body.sequence) ||
        body.sequence !== journal.sequence + 1 ||
        body.previous !== journal.previous ||
        typeof hash !== 'string' ||
        hash !== journal.hash(body)
      ) throw fail('MAKER_JOURNAL_APPEND_CHAIN');
      if (!body.state || typeof body.state !== 'object' || Array.isArray(body.state)) {
        throw fail('MAKER_JOURNAL_APPEND_STATE');
      }
      journal.sequence = body.sequence;
      journal.previous = hash;
      journal.last = body;
      state = body.state;
      bytes += Buffer.byteLength(line, 'utf8') + 1;
      appendedRecords++;
    }
  } catch (error) {
    if (error.code === 'JOURNAL_INCOMPLETE_TAIL') throw fail('MAKER_JOURNAL_APPEND_INCOMPLETE');
    if (['INVALID_JOURNAL_READ_RANGE', 'JOURNAL_READ_TRUNCATED'].includes(error.code)) {
      throw fail('MAKER_JOURNAL_CHANGED_DURING_REFRESH');
    }
    throw error;
  }

  if (bytes !== before.bytes) throw fail('MAKER_JOURNAL_APPEND_BYTES');
  const after = journalSnapshotV1(file);
  if (after.stamp !== before.stamp) throw fail('MAKER_JOURNAL_CHANGED_DURING_REFRESH');
  return { ...cached, journal, state, ...after, integrityMode: 'append_suffix_trusted_writer', refreshed: true, appendedRecords };
}

