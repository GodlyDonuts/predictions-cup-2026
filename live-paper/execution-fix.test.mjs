import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {execFileSync, spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {trainingOptions} from './training-options.mjs';
import {netEdgeDiagnostics} from './execution-metrics.mjs';
import {ROUND_TRIP, LEGACY_TARGET, targetPolicy} from './roundtrip-target.mjs';
import {ISOLATED, LEGACY} from './history-policy.mjs';
import {trainBank} from './ml-bank.mjs';
import {digest} from './model.mjs';
import {advanceShadow, verifySource, requireSourceMetadata} from './ml-shadow-v2.mjs';
import {DEFAULT_HORIZONS,trainingHorizons,parseTrainingHorizons} from './training-horizons.mjs';

const required = ['--journal', 'capture.jsonl', '--config', 'config.json', '--out', 'new-bank.json'];
const c = {...JSON.parse(fs.readFileSync(new URL('./config.json', import.meta.url))), marketIds: ['377']};
const start = Date.parse('2026-10-01T23:00:00Z');

test('explicit slower horizons retain purged labels and immutable model-specific holding times',()=>{
 assert.deepEqual(trainingHorizons(),DEFAULT_HORIZONS);assert.deepEqual(parseTrainingHorizons('900,1800'),[900,1800]);
 assert.deepEqual(trainingOptions([...required,'--horizons','900,1800']).options.horizons,[900,1800]);
 const bank=trainBank(capture(),c,{horizons:[900,1800]});
 assert.equal(bank.candidatesAttempted,64);assert.deepEqual(bank.trainingHorizonsSeconds,[900,1800]);
 assert.ok(bank.models.length>0);for(const model of bank.models){assert.ok([900,1800].includes(model.horizon));assert.ok(Date.parse(model.maxTrainingLabelAt)<Date.parse(bank.developmentSplitAt));}
 for(const horizons of [[],[60,60],[0],[61.1],[Infinity],[86401]])assert.throws(()=>trainBank('',c,{horizons}),/Invalid training horizons/);
 for(const value of ['900,','-60','60.5','60,60'])assert.throws(()=>parseTrainingHorizons(value),/Invalid training horizons/);
});
function capture(count = 180) {
  let previous = '';
  return Array.from({length: count}, (_, n) => {
    const at = new Date(start + n * 30000).toISOString(), bid = .4 + (n % 3) * .005;
    const data = {contexts: [{type: 'tournament', tournament: {id: c.tournamentId}, orderbook: {exchanges: [{exchangeId: '9', option: 'YES', asOf: {at, sequence: n}, bids: [{price: Number(bid.toFixed(3)), quantity: 10000}], asks: [{price: Number((bid + .01).toFixed(3)), quantity: 10000}]}]}}]};
    const body = {sequence: n + 1, previous, at, state: {}, event: {type: 'cycle', clockAt: at, accountExclusions: [], sourceSnapshots: [{endpoint: '/markets/377/orderbook?tournamentId=' + c.tournamentId, receivedAt: at, requestStartedAt: at, data}]}};
    const hash = digest(body); previous = hash;
    return JSON.stringify({...body, hash});
  }).join('\n') + '\n';
}

test('new CLI defaults to execution-aligned, isolated, supported-instrument training', () => {
  const args = trainingOptions(required);
  assert.equal(args.options.targetPolicy, ROUND_TRIP);
  assert.equal(args.options.historyPolicy, ISOLATED);
  assert.equal(args.options.requireInstrumentCoverage, true);
  assert.equal(args.options.trainingScope, 'supported-instruments-v1');
  assert.equal(targetPolicy(undefined), LEGACY_TARGET, 'old frozen banks retain their original interpretation');
});

test('historical target and undercovered exploration remain explicit, reproducible options', () => {
  const {options} = trainingOptions([...required, '--target-policy', LEGACY_TARGET, '--history-policy', LEGACY, '--allow-undercovered-exploration']);
  assert.equal(options.targetPolicy, LEGACY_TARGET);
  assert.equal(options.historyPolicy, LEGACY);
  assert.equal(options.requireInstrumentCoverage, false);
  assert.equal(options.trainingScope, 'all-markets-v1');
  assert.equal(trainingOptions([...required, '--training-scope', 'all-markets-v1']).options.trainingScope, 'all-markets-v1');
});

test('invalid, contradictory or accidentally repeated CLI flags fail before fitting', () => {
  for (const extra of [
    ['--unknown'], ['--target-policy'], ['--out', 'other.json'], ['--family', 'unknown'],
    ['--since', 'bad-date'], ['--history-policy', 'bad'], ['--target-policy', 'bad'],
    ['--require-instrument-coverage', '--allow-undercovered-exploration'],
    ['--allow-undercovered-exploration', '--training-scope', 'supported-instruments-v1'],
    ['--target-policy', LEGACY_TARGET, '--training-scope', 'supported-instruments-v1'],
  ]) assert.throws(() => trainingOptions([...required, ...extra]));
  assert.throws(() => trainingOptions([]), /Required/);
});

test('net-edge diagnostics use fixed actual paper margins and do not double-charge costs', () => {
  const d = netEdgeDiagnostics([{y: .02}, {y: -.04}, {y: .01}, {y: -.10}], [.005, .01, .02, -.1]);
  assert.deepEqual(d.margins.map(m => m.decisions), [3, 2, 1]);
  assert.ok(Math.abs(d.margins[0].meanObservedNetPerShare + .01 / 3) < 1e-12);
  assert.equal(d.margins[2].meanObservedNetPerShare, .01);
  assert.equal(d.portfolioPnl, null);
  assert.equal(d.profitabilityEstablished, false);
  const empty = netEdgeDiagnostics([{y: 0}], [0]);
  assert.ok(empty.margins.every(x => x.decisions === 0 && x.meanObservedNetPerShare === null));
  assert.throws(() => netEdgeDiagnostics([{y: 0}], [NaN]), /Invalid/);
  assert.throws(() => netEdgeDiagnostics([], [0]), /Invalid/);
});

test('new bank CLI output is frozen, cost-aligned and consumed as a net return by the real shadow reducer', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'sig-training-defaults-'));
  try {
    const journal = path.join(dir, 'capture.jsonl'), config = path.join(dir, 'config.json'), out = path.join(dir, 'bank.json');
    fs.writeFileSync(journal, capture()); fs.writeFileSync(config, JSON.stringify(c));
    const stdout = execFileSync(process.execPath, [fileURLToPath(new URL('./ml-bank.mjs', import.meta.url)), '--journal', journal, '--config', config, '--out', out], {encoding: 'utf8'});
    const bank = JSON.parse(fs.readFileSync(out)), result = JSON.parse(stdout);
    assert.equal(result.targetPolicy, ROUND_TRIP);
    assert.equal(bank.targetPolicy, ROUND_TRIP);
    assert.equal(bank.trainingDefaultsVersion, 'execution-aligned-cli-v1');
    assert.equal(bank.historyPolicy, ISOLATED);
    assert.equal(bank.requireInstrumentCoverage, true);
    assert.equal(bank.trainingScope, 'supported-instruments-v1');
    assert.equal(bank.modelsTrained, 128);
    assert.equal(bank.selected.length, 12);
    assert.equal(bank.labelDiagnostics[0].rejected.decision_warmup, 6);
    assert.ok(bank.models.every(m => m.executionDiagnostics && m.executionDiagnostics.portfolioPnl === null));
    assert.ok(bank.models.every(m => Date.parse(m.maxTrainingLabelAt) < Date.parse(bank.developmentSplitAt)));
    assert.deepEqual(bank.config, c, 'no latency, fees or risk limits silently changed');
    requireSourceMetadata(bank);
    const input = {bankHash: digest(bank), sourceSequence: 0, sourceHash: null, wallets: {}, histories: {}, predictions: {}};
    const state = advanceShadow(input, verifySource(capture(200)), bank);
    assert.equal(Object.keys(state.predictions).length, 36);
    assert.ok(Object.values(state.predictions).every(p => p.exitValue === null && Number.isFinite(p.netEdge)));
    const bytes = fs.readFileSync(out);
    const overwrite = spawnSync(process.execPath, [fileURLToPath(new URL('./ml-bank.mjs', import.meta.url)), '--journal', 'DOES_NOT_EXIST', '--config', 'DOES_NOT_EXIST', '--out', out], {encoding: 'utf8'});
    assert.notEqual(overwrite.status, 0);
    assert.match(overwrite.stderr, /Refusing to overwrite an existing frozen model bank/);
    assert.deepEqual(fs.readFileSync(out), bytes);
    assert.ok(!fs.readdirSync(dir).some(name => name.endsWith('.tmp')));
  } finally { fs.rmSync(dir, {recursive: true, force: true}); }
});

test('explicit programmatic legacy training stays reproducible and is not mislabeled as executable PnL', () => {
  const bank = trainBank(capture(), c, {targetPolicy: LEGACY_TARGET, historyPolicy: LEGACY});
  assert.equal(bank.targetPolicy, LEGACY_TARGET);
  assert.equal(bank.historyPolicy, LEGACY);
  assert.ok(bank.models.every(m => m.executionDiagnostics === undefined));
});

test('production static relative imports resolve in a clean checkout', () => {
  const root = fileURLToPath(new URL('../', import.meta.url));
  for (const folder of ['live-paper', 'execution', 'feed', 'paper', 'bot']) {
    const dir = path.join(root, folder);
    for (const name of fs.readdirSync(dir).filter(n => n.endsWith('.mjs') && !n.endsWith('.test.mjs'))) {
      const file = path.join(dir, name), source = fs.readFileSync(file, 'utf8');
      for (const match of source.matchAll(/(?:from\s+|import\s*)['"](\.\.?\/[^'"]+)['"]/g)) {
        assert.ok(fs.existsSync(path.resolve(dir, match[1])), `${folder}/${name}: missing ${match[1]}`);
      }
    }
  }
});

test('programmatic training also defaults to the executable target and isolated history', () => {
  const bank = trainBank(capture(), c);
  assert.equal(bank.targetPolicy, ROUND_TRIP);
  assert.equal(bank.historyPolicy, ISOLATED);
  assert.ok(bank.models.every(m => m.executionDiagnostics !== undefined));
});
