import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {initialState, restoreState} from './model.mjs';
import {resolvePaperJournalRoute} from './paper-journal-route-v1.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const baseConfig = JSON.parse(fs.readFileSync(path.join(here, 'config.json'), 'utf8'));

test('missing journal name preserves the native legacy journal route', () => {
  const runtimeDir = path.join(os.tmpdir(), 'paper-journal-route-default');
  const route = resolvePaperJournalRoute(runtimeDir);
  assert.equal(route.file, path.join(runtimeDir, 'journal.jsonl'));
  assert.equal(route.fileName, 'journal.jsonl');
  assert.equal(route.nativeJournal, true);
});

test('named fee-free journal is isolated and leaves the legacy journal unchanged', () => {
  const runtimeDir = fs.mkdtempSync(path.join(os.tmpdir(), 'paper-journal-route-'));
  const legacyFile = path.join(runtimeDir, 'journal.jsonl');
  fs.writeFileSync(legacyFile, 'legacy journal sentinel\n');

  const route = resolvePaperJournalRoute(runtimeDir, 'fee-free-baseline-v1.jsonl');
  assert.equal(route.file, path.join(runtimeDir, 'fee-free-baseline-v1.jsonl'));
  assert.equal(route.nativeJournal, false);
  assert.equal(fs.readFileSync(legacyFile, 'utf8'), 'legacy journal sentinel\n');
  assert.equal(fs.existsSync(route.file), false);
});

test('named journal route is stable and its state resumes only with the same config', () => {
  const runtimeDir = path.join(os.tmpdir(), 'paper-journal-route-resume');
  const name = 'fee-free-baseline-v1.jsonl';
  const first = resolvePaperJournalRoute(runtimeDir, name);
  const afterRestart = resolvePaperJournalRoute(runtimeDir, name);
  assert.deepEqual(afterRestart, first);

  const oldState = initialState(baseConfig, 'legacy-run', '2026-10-14T12:00:00.000Z');
  const namedConfig = {...baseConfig, journalFileName: name};
  assert.throws(() => restoreState(oldState, namedConfig), /identical config\/schema/);

  const namedState = initialState(namedConfig, 'fee-free-run', '2026-10-14T12:01:00.000Z');
  assert.equal(restoreState(namedState, namedConfig).runId, 'fee-free-run');
  assert.throws(() => restoreState(namedState, baseConfig), /identical config\/schema/);
});

test('rejects traversal, separators, device names, and the legacy journal as explicit names', () => {
  const runtimeDir = path.join(os.tmpdir(), 'paper-journal-route-invalid');
  for (const name of [
    '../escape.jsonl', '..\\escape.jsonl', '/absolute.jsonl', 'C:\\escape.jsonl',
    'has space.jsonl', 'CON.jsonl', 'journal.jsonl', '', null
  ]) {
    assert.throws(() => resolvePaperJournalRoute(runtimeDir, name), undefined, String(name));
  }
});

test('requires an absolute runtime directory', () => {
  assert.throws(() => resolvePaperJournalRoute('relative/runtime'), /must be absolute/);
});
