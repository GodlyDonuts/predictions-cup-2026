import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, readdirSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { hash, parseDataset } from '../core.mjs';
import { fixture } from '../generate-fixture.mjs';
import { args, main, codeFingerprint, RUNTIME_FILES } from '../cli.mjs';
import { prepare, develop, finalEvaluation } from '../experiment.mjs';
import { config, runtime, values, encode } from './helpers.mjs';
const root = fileURLToPath(new URL('../..', import.meta.url));
const input = join(root, 'ml/fixtures/synthetic-books.jsonl'), configPath = join(root, 'ml/fixtures/config.json');
const dir = () => mkdtempSync(join(tmpdir(), 'ml-test-'));
const command = (out, extra = []) => ['develop', '--input', input, '--config', configPath, '--out', out, ...extra];
const read = path => JSON.parse(readFileSync(path, 'utf8'));
test('fixture bytes reproduce from fixed seed and golden key results match', () => {
  assert.equal(readFileSync(input, 'utf8'), fixture());
  const p = prepare(parseDataset(fixture()), config), d = develop(p, runtime), f = finalEvaluation(p, runtime, d.selection);
  const actual = { warning: f.report.warning, sourceSha256: p.dataset.sourceHash, counts: f.report.counts, coverage: f.report.coverage, selectedAlpha: f.model.alpha, validation: f.report.validation, test: f.report.test };
  assert.deepEqual(actual, read(join(root, 'ml/fixtures/expected-synthetic.json')));
});
test('CLI writes exclusive deterministic, hash-verified run manifests and withholds test', () => {
  const base = dir(), out = join(base, 'dev'), argv = command(out), summary = main(argv);
  assert.equal(summary.test.status, 'withheld'); assert.match(summary.warning, /SYNTHETIC-ONLY/);
  const report = read(join(out, 'report.json')); assert.deepEqual(report.runtime.argv, ['node', 'ml/cli.mjs', ...argv]); assert.equal(report.runtime.seed, 20261001);
  assert.equal(report.fingerprint.implementationSha256, codeFingerprint().implementationSha256);
  for (const artifact of read(join(out, 'complete.json')).artifacts) assert.equal(hash(readFileSync(join(out, artifact.name))), artifact.sha256);
  assert.ok(!existsSync(join(out, 'test-predictions.json')));
  const original = readFileSync(join(out, 'report.json')); assert.throws(() => main(argv)); assert.deepEqual(readFileSync(join(out, 'report.json')), original);
  const final = join(base, 'final'); main(['final', '--input', input, '--config', configPath, '--out', final, '--selection', join(out, 'selection.json')]);
  assert.equal(read(join(final, 'report.json')).test.count, 102); assert.equal(read(join(final, 'test-predictions.json')).length, 102);
});
test('unknown/live/URL/duplicate flags are rejected before creating output', () => {
  for (const argv of [['--live'], ['collect'], ['develop', '--input', 'https://example.com/book', '--config', configPath, '--out', 'unused'], ['develop', '--input', input], command('unused', ['--live']), command('unused', ['--out', 'other']), command('unused', ['--selection', 'forbidden'])]) assert.throws(() => args(argv));
  assert.equal(args([]), null); assert.equal(args(['--help']), null); assert.match(main([]).help, /Offline/);
});
test('invalid/truncated/feed-format inputs and selection mismatches leave no output directory', () => {
  const base = dir(), path = join(base, 'bad.jsonl');
  for (const content of [fixture().trimEnd(), 'malformed\n', '{"format":"sig-feed-capture"}\n']) { writeFileSync(path, content); assert.throws(() => main(['develop', '--input', path, '--config', configPath, '--out', join(base, 'never')])); assert.ok(!existsSync(join(base, 'never'))); }
  const selection = join(base, 'selection.json'); writeFileSync(selection, '{}');
  assert.throws(() => main(['final', '--input', input, '--config', configPath, '--out', join(base, 'never'), '--selection', selection]), /SELECTION/); assert.ok(!existsSync(join(base, 'never')));
});
test('actual subprocess emits structured real-data rejection without success files', () => {
  const base = dir(), rows = values(); rows[0].dataKind = 'real'; const path = join(base, 'real.jsonl'); writeFileSync(path, encode(rows));
  const run = spawnSync(process.execPath, ['ml/cli.mjs', 'develop', '--input', path, '--config', configPath, '--out', join(base, 'out')], { cwd: root, encoding: 'utf8' });
  assert.equal(run.status, 1); assert.equal(run.stdout, ''); assert.equal(JSON.parse(run.stderr).status, 'rejected-insufficient-data'); assert.ok(!existsSync(join(base, 'out')));
});
test('actual process help and invalid arguments return safe statuses', () => {
  assert.equal(spawnSync(process.execPath, ['ml/cli.mjs', '--help'], { cwd: root }).status, 0);
  const run = spawnSync(process.execPath, ['ml/cli.mjs', '--live'], { cwd: root, encoding: 'utf8' }); assert.equal(run.status, 1); assert.equal(JSON.parse(run.stderr).code, 'OFFLINE_COMMAND_ONLY');
  const missing = spawnSync(process.execPath, ['ml/cli.mjs', ...command('/tmp/unused').map(x => x === input ? '/nonexistent/private-path' : x)], { cwd: root, encoding: 'utf8' }); assert.equal(missing.status, 1); assert.ok(!missing.stderr.includes('/nonexistent'));
});
test('fixture generator only creates new local files, and never overwrites', () => {
  const base = dir(), path = join(base, 'fixture.jsonl');
  let run = spawnSync(process.execPath, ['ml/generate-fixture.mjs', path], { cwd: root }); assert.equal(run.status, 0); assert.equal(readFileSync(path, 'utf8'), fixture());
  run = spawnSync(process.execPath, ['ml/generate-fixture.mjs', path], { cwd: root }); assert.equal(run.status, 1);
  run = spawnSync(process.execPath, ['ml/generate-fixture.mjs', '--live'], { cwd: root }); assert.equal(run.status, 1);
});
test('runtime has no network, credential, scanner, execution or dynamic-code capability imports', () => {
  const texts = [...RUNTIME_FILES, 'generate-fixture.mjs'].map(name => readFileSync(join(root, 'ml', name), 'utf8')).join('\n');
  for (const forbidden of [/\bfetch\s*\(/, /\bWebSocket\b/, /node:(?:http|https|net|tls|child_process|worker_threads)/, /process\.env/, /import\s*\(/, /eval\s*\(/, /new Function/, /\.\.\/(?:bot|feed|paper)\//]) assert.doesNotMatch(texts, forbidden);
  assert.equal(readdirSync(join(root, 'ml')).filter(x => x === 'node_modules').length, 0);
});
test('directory inputs and oversized config fail before output creation', () => {
  const base = dir(), huge = join(base, 'huge-config.json'); writeFileSync(huge, ' '.repeat(1024 * 1024 + 1));
  assert.throws(() => main(['develop', '--input', base, '--config', configPath, '--out', join(base, 'no')]), /REGULAR_FILE_REQUIRED/);
  assert.throws(() => main(['develop', '--input', input, '--config', huge, '--out', join(base, 'no')]), /INPUT_TOO_LARGE/);
  assert.ok(!existsSync(join(base, 'no')));
});
test('final predictions preserve exact past/current/label provenance and manifest hashes', () => {
  const rows = values(), p = prepare(parseDataset(encode(rows)), config), d = develop(p, runtime), f = finalEvaluation(p, runtime, d.selection);
  const original = new Map(rows.slice(1).map(r => [r.rowId, r]));
  for (const prediction of f.predictions) {
    for (const [prefix, id] of [['', prediction.rowId], ['past', prediction.pastRowId], ['label', prediction.labelRowId]]) {
      for (const field of ['sourceAt', 'receivedAt', 'availableAt', 'sequence', 'quality']) {
        const key = prefix ? prefix + field[0].toUpperCase() + field.slice(1) : field;
        assert.deepEqual(prediction[key], original.get(id)[field]);
      }
    }
  }
});
test('checked-in run manifest pins current fixture, generator, config and runtime bytes', () => {
  const m = read(join(root, 'ml/fixtures/run-manifest.json'));
  assert.equal(m.inputSha256, hash(readFileSync(input))); assert.equal(m.configFileSha256, hash(readFileSync(configPath)));
  assert.deepEqual(m.runtime, codeFingerprint()); assert.equal(m.fixtureGeneratorSha256, hash(readFileSync(join(root, m.fixtureGenerator))));
  assert.equal(m.goldenSha256, hash(readFileSync(join(root, m.golden)))); assert.equal(m.dataKind, 'synthetic');
});
