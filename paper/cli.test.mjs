import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync, readdirSync, mkdtempSync, writeFileSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {main} from './cli.mjs';

const root = fileURLToPath(new URL('.', import.meta.url));
const args = ['--events', join(root, 'fixtures/synthetic.jsonl'), '--config', join(root, 'fixtures/config.json')];
test('golden synthetic fixture is exact and repeatable; baseline is no-trade', () => {
  const actual = main(args); const expected = readFileSync(join(root, 'fixtures/expected-report.json'), 'utf8').trim();
  assert.equal(actual, expected); assert.equal(main(args), actual);
  const report = JSON.parse(actual);
  assert.equal(report.noTrade.cash, '100.000000'); assert.equal(report.noTrade.metrics.simulatedWriteRequests, 0);
  assert.equal(report.scriptedPassive.assumptions.noProfitabilityClaim, true);
  assert.equal(report.scriptedPassive.fills.length, 4); assert.equal(report.scriptedPassive.orders[1].status, 'cancelled');
  assert.equal(report.scriptedPassive.positions[0].executable.unsoldQuantity, 2);
});
test('CLI fails on --live, URLs, duplicates, unknown and missing arguments', () => {
  for (const input of [[], ['--live'], ['--events', 'https://example.com/feed', '--config', 'x'],
    [...args, '--live', 'true'], [...args, '--config', 'other'], ['--events', '--config'], ['--key', 'abc']]) assert.throws(() => main(input));
  assert.match(main(['--help']), /Offline paper/);
});
test('executable CLI has nonzero error exit and no partial success JSON', () => {
  const result = spawnSync(process.execPath, [join(root, 'cli.mjs'), '--live'], {encoding: 'utf8'});
  assert.equal(result.status, 1); assert.equal(result.stdout, ''); assert.match(result.stderr, /Offline replay failed/);
});
test('CLI rejects malformed inputs instead of skipping broken events', () => {
  const dir = mkdtempSync(join(tmpdir(), 'paper-cli-test-'));
  try {
    const path = join(dir, 'bad.jsonl'); writeFileSync(path, '{broken}\n');
    assert.throws(() => main(['--events', path, '--config', args[3]]), /Invalid event JSON/);
    writeFileSync(path, '{}\n'); assert.throws(() => main(['--events', path, '--config', args[3]]), /schema/);
  } finally { rmSync(dir, {recursive: true, force: true}); }
});
test('runtime has no networking, credentials, process spawning, scanner or dynamic-code hooks', () => {
  for (const name of ['fixed.mjs', 'schema.mjs', 'engine.mjs', 'split.mjs', 'cli.mjs']) {
    const source = readFileSync(join(root, name), 'utf8');
    assert.doesNotMatch(source, /\bfetch\s*\(|\bWebSocket\b|node:(?:https?|net|tls|dgram|child_process)|process\.env|eval\s*\(|new Function|import\s*\(|\.\.\/bot\//, name);
  }
  assert.equal(JSON.parse(readFileSync(join(root, 'package.json'))).dependencies, undefined);
});
