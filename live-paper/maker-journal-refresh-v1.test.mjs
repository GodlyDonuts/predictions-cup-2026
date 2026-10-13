import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import { Journal } from './transport-runtime.mjs';
import { acquireMakerJournalWriterV1, assertPaperReplaySupervisorLeaseV1, journalSnapshotV1, paperReplaySupervisorLeaseV1, refreshCachedMakerJournalV1 } from './maker-journal-refresh-v1.mjs';

function fixture() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'maker-journal-refresh-'));
  const file = path.join(dir, 'journal.jsonl');
  const supervisorFile = path.join(dir, 'paper-replay-worker.lock');
  fs.writeFileSync(supervisorFile, JSON.stringify({pid: process.pid, mode: 'local_quote_replay_only', apiClients: 0}));
  const supervisorLease = paperReplaySupervisorLeaseV1(supervisorFile);
  const journal = new Journal(file);
  const state = { bankHash: 'fixture-bank', revision: 1 };
  journal.commit(state, { type: 'seed' });
  return { dir, file, supervisorFile, supervisorLease, cached: { journal, state, ...journalSnapshotV1(file) } };
}

function nextLine(journal, revision) {
  const body = {
    sequence: journal.sequence + 1,
    previous: journal.previous,
    at: new Date().toISOString(),
    event: { type: 'external_append' },
    state: { bankHash: 'fixture-bank', revision },
  };
  return JSON.stringify({ ...body, hash: journal.hash(body) }) + '\n';
}

test('refreshes a valid external append without mutating the prior cache', t => {
  const { dir, file, cached } = fixture();
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const writer = new Journal(file);
  writer.commit({ bankHash: 'fixture-bank', revision: 2 }, { type: 'external_append' });
  const refreshed = refreshCachedMakerJournalV1(file, cached);
  assert.equal(refreshed.refreshed, true);
  assert.equal(refreshed.appendedRecords, 1);
  assert.equal(refreshed.journal.sequence, 2);
  assert.equal(refreshed.state.revision, 2);
  assert.equal(refreshed.integrityMode, 'append_suffix_trusted_writer');
  assert.equal(cached.journal.sequence, 1);
  assert.equal(cached.state.revision, 1);
});

test('keeps the last verified cache through a partial row, then completes refresh', t => {
  const { dir, file, cached } = fixture();
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const row = Buffer.from(nextLine(cached.journal, 2));
  const split = Math.floor(row.length / 2);
  fs.appendFileSync(file, row.subarray(0, split));
  assert.throws(
    () => refreshCachedMakerJournalV1(file, cached),
    error => error.code === 'MAKER_JOURNAL_APPEND_INCOMPLETE',
  );
  assert.equal(cached.journal.sequence, 1);
  fs.appendFileSync(file, row.subarray(split));
  const refreshed = refreshCachedMakerJournalV1(file, cached);
  assert.equal(refreshed.journal.sequence, 2);
  assert.equal(refreshed.state.revision, 2);
});

test('rejects a bad hash chain without advancing cached sequence or state', t => {
  const { dir, file, cached } = fixture();
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const row = JSON.parse(nextLine(cached.journal, 2));
  row.hash = '0'.repeat(64);
  fs.appendFileSync(file, JSON.stringify(row) + '\n');
  assert.throws(
    () => refreshCachedMakerJournalV1(file, cached),
    error => error.code === 'MAKER_JOURNAL_APPEND_CHAIN',
  );
  assert.equal(cached.journal.sequence, 1);
  assert.equal(cached.state.revision, 1);
});

test('fails closed on truncation and same-size mutation with changed metadata', t => {
  const first = fixture();
  t.after(() => fs.rmSync(first.dir, { recursive: true, force: true }));
  fs.truncateSync(first.file, 0);
  assert.throws(
    () => refreshCachedMakerJournalV1(first.file, first.cached),
    error => error.code === 'MAKER_JOURNAL_REPLACED',
  );

  const second = fixture();
  t.after(() => fs.rmSync(second.dir, { recursive: true, force: true }));
  const before = fs.statSync(second.file);
  const bytes = fs.readFileSync(second.file);
  bytes[bytes.length - 2] ^= 1;
  fs.writeFileSync(second.file, bytes);
  fs.utimesSync(second.file, before.atime, new Date(before.mtimeMs + 10000));
  assert.throws(
    () => refreshCachedMakerJournalV1(second.file, second.cached),
    error => error.code === 'MAKER_JOURNAL_MUTATED_WITHOUT_APPEND',
  );
});

test('refreshes append backlogs above 64 MiB with bounded-memory journal scanning', t => {
  const { dir, file, cached } = fixture();
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const writer = new Journal(file);
  const padding = 'x'.repeat(12 * 1024 * 1024);
  for (let revision = 2; revision <= 7; revision++) {
    writer.commit({ bankHash: 'fixture-bank', revision, padding }, { type: 'external_append' });
  }
  assert.ok(fs.statSync(file).size > 64 * 1024 * 1024);
  const refreshed = refreshCachedMakerJournalV1(file, cached);
  assert.equal(refreshed.appendedRecords, 6);
  assert.equal(refreshed.journal.sequence, 7);
  assert.equal(refreshed.state.revision, 7);
  assert.equal(cached.journal.sequence, 1);
});

test('per-cohort writer lease rejects a live writer and releases its own lease', t => {
  const { dir, supervisorLease } = fixture();
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const file = path.join(dir, 'journal.writer.lock');
  const release = acquireMakerJournalWriterV1(file, {supervisorLease});
  assert.throws(() => acquireMakerJournalWriterV1(file, {supervisorLease}), error => error.code === 'MAKER_JOURNAL_WRITER_ACTIVE');
  release();
  acquireMakerJournalWriterV1(file, {supervisorLease})();
});

test('a prior supervisor lease is reclaimed even when its PID has been reused', t => {
  const { dir, supervisorLease } = fixture();
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const file = path.join(dir, 'journal.writer.lock');
  fs.writeFileSync(file, JSON.stringify({schemaVersion: 1, pid: process.pid, startedAt: new Date(0).toISOString(), nonce: 'ab'.repeat(16), supervisorIdentity: 'prior-worker-generation'}));
  const release = acquireMakerJournalWriterV1(file, {supervisorLease});
  const owner = JSON.parse(fs.readFileSync(file, 'utf8'));
  assert.equal(owner.pid, process.pid);
  assert.equal(owner.schemaVersion, 1);
  release();
  assert.equal(fs.existsSync(file), false);
});

test('legacy numeric lock for the current live supervisor is never stolen by age', t => {
  const { dir, supervisorLease } = fixture();
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const file = path.join(dir, 'journal.writer.lock');
  fs.writeFileSync(file, String(process.pid));
  for (const ageMs of [1000, 60000]) {
    assert.throws(
      () => acquireMakerJournalWriterV1(file, {supervisorLease, now: () => Date.now() + ageMs}),
      error => error.code === 'MAKER_JOURNAL_WRITER_ACTIVE',
      `live PID-only lock remains owned at age ${ageMs}ms`,
    );
  }
  assert.equal(fs.readFileSync(file, 'utf8'), String(process.pid));
});

test('legacy numeric lock from a prior supervisor generation is reclaimed under the current exclusive lease', t => {
  const { dir, supervisorLease } = fixture();
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const file = path.join(dir, 'journal.writer.lock');
  fs.writeFileSync(file, '99999999');
  const release = acquireMakerJournalWriterV1(file, {supervisorLease});
  const owner = JSON.parse(fs.readFileSync(file, 'utf8'));
  assert.equal(owner.pid, process.pid);
  assert.equal(owner.supervisorIdentity, supervisorLease.identity);
  release();
  assert.equal(fs.existsSync(file), false);
});

test('a changed outer supervisor lease blocks journal-lock cleanup', t => {
  const { dir, supervisorFile, supervisorLease } = fixture();
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const file = path.join(dir, 'journal.writer.lock');
  fs.writeFileSync(file, JSON.stringify({schemaVersion: 1, pid: process.pid, startedAt: new Date(0).toISOString(), nonce: 'ef'.repeat(16), supervisorIdentity: 'older-generation'}));
  fs.writeFileSync(supervisorFile, JSON.stringify({pid: process.pid, mode: 'local_quote_replay_only', apiClients: 0, generation: 2}));
  assert.throws(
    () => acquireMakerJournalWriterV1(file, {supervisorLease}),
    error => error.code === 'MAKER_SUPERVISOR_LEASE_CHANGED',
  );
  assert.equal(fs.existsSync(file), true);
});

test('outer supervisor lease remains verifiable at the durable write boundary', t => {
  const { dir, supervisorFile, supervisorLease } = fixture();
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  assert.doesNotThrow(() => assertPaperReplaySupervisorLeaseV1(supervisorLease));
  fs.writeFileSync(supervisorFile, JSON.stringify({pid: process.pid, mode: 'local_quote_replay_only', apiClients: 0, generation: 'replaced'}));
  assert.throws(
    () => assertPaperReplaySupervisorLeaseV1(supervisorLease),
    error => error.code === 'MAKER_SUPERVISOR_LEASE_CHANGED',
  );
});

