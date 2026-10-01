#!/usr/bin/env node
import { readFileSync, writeFileSync, mkdirSync, statSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
import { hash, canonical, parseDataset } from './core.mjs';
import { prepare, develop, finalEvaluation } from './experiment.mjs';

export const RUNTIME_FILES = ['core.mjs', 'model.mjs', 'experiment.mjs', 'cli.mjs'];
export function codeFingerprint() {
  const files = RUNTIME_FILES.map(name => ({ path: `ml/${name}`, sha256: hash(readFileSync(new URL(name, import.meta.url))) }));
  return { implementationSha256: hash(canonical(files)), files };
}
function localPath(value) {
  if (typeof value !== 'string' || value.length === 0 || value.includes('\0') || /[a-z][a-z0-9+.-]*:\/\//i.test(value) || value.startsWith('-')) throw new Error('LOCAL_PATH_REQUIRED');
  return value;
}
function readLocal(path, maxBytes) {
  const stat = statSync(path);
  if (!stat.isFile()) throw new Error('REGULAR_FILE_REQUIRED');
  if (stat.size > maxBytes) throw new Error('INPUT_TOO_LARGE');
  const text = readFileSync(path, 'utf8');
  if (Buffer.byteLength(text) > maxBytes) throw new Error('INPUT_TOO_LARGE');
  return text;
}
export function args(argv) {
  if (argv.length === 0 || (argv.length === 1 && argv[0] === '--help')) return null;
  const command = argv[0];
  if (!['develop', 'final'].includes(command)) throw new Error('OFFLINE_COMMAND_ONLY');
  const allowed = new Set(['--input', '--config', '--out', ...(command === 'final' ? ['--selection'] : [])]);
  const options = {};
  for (let i = 1; i < argv.length; i += 2) {
    if (!allowed.has(argv[i]) || Object.hasOwn(options, argv[i]) || !argv[i + 1]) throw new Error('ARGUMENTS');
    options[argv[i]] = localPath(argv[i + 1]);
  }
  if (Object.keys(options).length !== allowed.size) throw new Error('ARGUMENTS');
  return { command, input: options['--input'], config: options['--config'], out: options['--out'], selection: options['--selection'] };
}
export function main(argv) {
  const options = args(argv);
  if (!options) return { help: 'Offline book baseline. node ml/cli.mjs develop --input books.jsonl --config config.json --out NEW_DIRECTORY. Final evaluation additionally requires command final and --selection development/selection.json. No network, credentials, orders or deployment.' };
  const input = readLocal(options.input, 100 * 1024 * 1024);
  const configText = readLocal(options.config, 1024 * 1024);
  const config = JSON.parse(configText);
  const runtime = { ...codeFingerprint(), node: process.version, argv: ['node', 'ml/cli.mjs', ...argv], configFileSha256: hash(configText), seed: config.seed, platformArithmetic: 'IEEE-754 binary64; exact nanosecond timestamp comparisons; no monetary accounting' };
  const prepared = prepare(parseDataset(input), config);
  const result = options.command === 'final' ? finalEvaluation(prepared, runtime, JSON.parse(readLocal(options.selection, 1024 * 1024))) : develop(prepared, runtime);
  // Exclusive directory plus exclusive files. Report and completion manifest are last.
  // On an I/O failure a partial directory is not a successful run; never overwrite it.
  mkdirSync(options.out, { mode: 0o700 });
  const artifacts = {
    'selection.json': result.selection,
    'split-manifest.json': result.splitManifest,
    ...(result.predictions ? { 'test-predictions.json': result.predictions } : {}),
    'report.json': result.report
  };
  const hashes = [];
  for (const [name, value] of Object.entries(artifacts)) {
    const content = JSON.stringify(value, null, 2) + '\n';
    writeFileSync(resolve(options.out, name), content, { flag: 'wx', mode: 0o600 });
    hashes.push({ name, sha256: hash(content) });
  }
  writeFileSync(resolve(options.out, 'complete.json'), JSON.stringify({ schemaVersion: 1, complete: true, fingerprint: result.report.fingerprint, artifacts: hashes }, null, 2) + '\n', { flag: 'wx', mode: 0o600 });
  return { status: result.report.status, dataKind: result.report.dataKind, warning: result.report.warning, counts: result.report.counts, coverage: result.report.coverage, selectedAlpha: result.model.alpha, validation: result.report.validation, test: result.report.test, output: options.out };
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { process.stdout.write(JSON.stringify(main(process.argv.slice(2)), null, 2) + '\n'); }
  catch (error) {
    const summary = error.summary || { status: 'failed', code: /^[A-Z_]+(?:\b|:)/.test(error.message) ? error.message.split(':')[0] : 'INVALID_INPUT_OR_IO' };
    process.stderr.write(JSON.stringify(summary, null, 2) + '\n'); process.exitCode = 1;
  }
}
