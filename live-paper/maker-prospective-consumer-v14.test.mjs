import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { payloadHash, atomicJson } from '../execution/ledger.mjs';
import { MAKER_JOIN_CONTROL_SCHEDULER_V1 } from '../paper/maker-join-control-v1.mjs';
import { makerProspectiveV14CodeIdentity, updateMakerProspectiveV14 } from './maker-prospective-consumer-v14.mjs';
import { Journal } from './transport-runtime.mjs';
import { paperReplaySupervisorLeaseV1 } from './maker-journal-refresh-v1.mjs';

function fixture(t) {
  const runtimeDir = fs.mkdtempSync(path.join(os.tmpdir(), 'maker-v14-recovery-'));
  t.after(() => fs.rmSync(runtimeDir, { recursive: true, force: true }));
  fs.writeFileSync(path.join(runtimeDir, 'paper-replay-worker.lock'), JSON.stringify({pid: process.pid, mode: 'local_quote_replay_only', apiClients: 0}));
  const clock = Date.parse('2026-10-13T02:00:00.000Z');
  const at = new Date(clock).toISOString();
  const policy = {
    tournamentId: 'cup',
    scheduler: MAKER_JOIN_CONTROL_SCHEDULER_V1,
    participationBps: 100,
    wallet: {
      cutoff: at,
      initialCash: '100.000000',
      maximumCommitted: '5.000000',
      maximumMarkets: 3,
      maximumOrderCost: '1.000000',
      quantity: 1,
      latencyMs: 3000,
      lifetimeMs: 120000,
      maximumGapMs: 120000,
      minimumSamples: 1,
      minimumHistoryMs: 0,
      depthFractionBps: 100,
      fillModel: 'strict_tape_through',
    },
  };
  const cursor = (file, hash) => {
    fs.writeFileSync(file, '');
    const stat = fs.statSync(file);
    return { dev: stat.dev, ino: stat.ino, bytes: 0, sequence: 0, hash };
  };
  const bookAnchor = cursor(path.join(runtimeDir, 'public-books.jsonl'), 'a'.repeat(64));
  const tapeAnchor = cursor(path.join(runtimeDir, 'anonymous-trades.jsonl'), '');
  fs.writeFileSync(
    path.join(runtimeDir, 'realtime-test.jsonl'),
    JSON.stringify({ format: 'sig-feed-capture', captureIndex: 0, kind: 'config', observedAt: at }) + '\n',
  );
  atomicJson(path.join(runtimeDir, 'realtime-session.json'), {
    file: 'realtime-test.jsonl',
    scope: { tournamentId: 'cup' },
  });
  const cohort = 'test-v14-cache-refresh';
  const dir = path.join(runtimeDir, 'maker-prospective-cohorts-v14', cohort);
  fs.mkdirSync(dir, { recursive: true });
  const body = {
    schemaVersion: 1,
    mode: 'maker_prospective_paper_v14',
    cohort,
    tournamentId: 'cup',
    executionAuthority: false,
    pushSourcePolicy: { requireContinuous: false },
    policy,
    codeIdentity: makerProspectiveV14CodeIdentity(),
    minimumSamples: 1,
    minimumRecoverySamples: 1,
    markoutReserve: { perShare: '0.000000', sourceHash: 'b'.repeat(64), knownAt: at },
    bookAnchor,
    tapeAnchor,
    publication: {
      scope: { exchanges: [{ marketId: '11', exchangeId: '101' }] },
      books: {},
    },
    history: {},
  };
  const bank = { ...body, bankHash: payloadHash(body) };
  atomicJson(path.join(dir, 'bank.json'), bank);
  atomicJson(path.join(runtimeDir, 'maker-prospective-active-v14.json'), {
    schemaVersion: 1,
    mode: 'maker_prospective_v14',
    executionAuthority: false,
    cohort,
    bankHash: bank.bankHash,
  });
  return { runtimeDir, dir, clock, bank };
}

test('V14 reads only its versioned pointer and exposes append-only cache integrity mode', t => {
  const f = fixture(t);
  const initial = updateMakerProspectiveV14({ runtimeDir: f.runtimeDir, now: () => f.clock });
  assert.equal(initial.updates, 1);
  assert.equal(initial.journalIntegrityMode, 'initial_full_journal_load');
  assert.equal(initial.journalPrefixRehashed, true);

  const journalFile = path.join(f.dir, 'journal.jsonl');
  const writer = new Journal(journalFile);
  const nextState = { ...writer.last.state, externalAppendMarker: 'preserved' };
  writer.commit(nextState, { type: 'external_append' });

  const result = updateMakerProspectiveV14({
    runtimeDir: f.runtimeDir,
    now: () => f.clock + 1000,
  });
  const recovered = new Journal(journalFile);
  assert.equal(result.updates, 2);
  assert.equal(result.journalIntegrityMode, 'append_suffix_trusted_writer');
  assert.equal(result.journalPrefixRehashed, false);
  assert.equal(recovered.sequence, 3);
  assert.equal(recovered.last.state.externalAppendMarker, 'preserved');
  assert.equal(fs.existsSync(path.join(f.dir, 'journal.writer.lock')), false);
  assert.equal(fs.existsSync(path.join(f.runtimeDir, 'maker-prospective-active-v13.json')), false);
  assert.equal(fs.existsSync(path.join(f.runtimeDir, 'maker-prospective-cohorts', path.basename(f.dir))), false);
});

test('V14 refuses a concurrent writer without advancing its journal', t => {
  const f = fixture(t);
  const lockFile = path.join(f.dir, 'journal.writer.lock');
  const supervisorLease = paperReplaySupervisorLeaseV1(path.join(f.runtimeDir, 'paper-replay-worker.lock'));
  fs.writeFileSync(lockFile, JSON.stringify({schemaVersion: 1, pid: process.pid, startedAt: new Date(f.clock).toISOString(), nonce: 'cd'.repeat(16), supervisorIdentity: supervisorLease.identity}), { flag: 'wx' });
  assert.throws(
    () => updateMakerProspectiveV14({ runtimeDir: f.runtimeDir, now: () => f.clock }),
    /MAKER_JOURNAL_WRITER_ACTIVE/,
  );
  assert.equal(fs.existsSync(path.join(f.dir, 'journal.jsonl')), false);
});

test('V14 cannot read a V13 cohort from the legacy root', t => {
  const f = fixture(t);
  fs.rmSync(path.join(f.dir, 'bank.json'));
  const legacy = path.join(f.runtimeDir, 'maker-prospective-cohorts', path.basename(f.dir));
  fs.mkdirSync(legacy, { recursive: true });
  atomicJson(path.join(legacy, 'bank.json'), f.bank);
  atomicJson(path.join(f.runtimeDir, 'maker-prospective-active-v13.json'), {
    cohort: path.basename(f.dir),
    bankHash: f.bank.bankHash,
  });
  assert.throws(
    () => updateMakerProspectiveV14({ runtimeDir: f.runtimeDir, now: () => f.clock }),
    /MAKER_V14_BANK_MISSING/,
  );
  assert.equal(fs.existsSync(path.join(legacy, 'journal.jsonl')), false);
});

test('V14 rejects a bank that does not pin its complete implementation closure', t => {
  const f = fixture(t);
  const body = { ...f.bank, codeIdentity: {} };
  delete body.bankHash;
  const bank = { ...body, bankHash: payloadHash(body) };
  atomicJson(path.join(f.dir, 'bank.json'), bank);
  atomicJson(path.join(f.runtimeDir, 'maker-prospective-active-v14.json'), {
    schemaVersion: 1,
    mode: 'maker_prospective_v14',
    executionAuthority: false,
    cohort: bank.cohort,
    bankHash: bank.bankHash,
  });
  assert.throws(
    () => updateMakerProspectiveV14({ runtimeDir: f.runtimeDir, now: () => f.clock }),
    /MAKER_V14_FROZEN_CODE/,
  );
});
