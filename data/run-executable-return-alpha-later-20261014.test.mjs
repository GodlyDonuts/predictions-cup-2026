import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {EXECUTABLE_RETURN_POLICY_V1} from '../research/executable-return-alpha-v1.mjs';
import {
  assertStableSourceHashesV1,
  assertStableSourceIdentityV1,
  FORWARD_OUTPUT_RELATIVE_PATH_V1,
  frozenExecutableReturnModelV1,
  hashRangeV1,
  selectPostBoundaryRowsV1,
  verifyPinnedPriorReportBytesV1,
} from './run-executable-return-alpha-later-20261014.mjs';

const cutoff = '2026-10-14T14:31:30.476Z';
function frozenReport() {
  return {source: {bytes: 328_568_080,
    sha256: 'a6628a435a71f99fc702d887614059ea5da48be279b940b20450407108ea5b70'},
    cutoff, policy: {...EXECUTABLE_RETURN_POLICY_V1},
    model: {trainingRows: 55_609, trainingMean: -0.0061, trainingRmse: 0.0016,
      coefficients: Array(7).fill(0.1), featureMean: Array(6).fill(0.2), featureScale: Array(6).fill(1)}};
}

test('loads only the fixed report model and rejects policy, source, or training changes', () => {
  const model = frozenExecutableReturnModelV1(frozenReport());
  assert.equal(model.trainingRows, 55_609);
  assert.equal(model.cutoff, Date.parse(cutoff));
  assert.deepEqual(model.coefficients, Array(7).fill(0.1));
  for (const field of ['latencyMs', 'horizonMs', 'feesPerShare']) {
    const changed = frozenReport(); changed.policy[field]++;
    assert.throws(() => frozenExecutableReturnModelV1(changed), /POLICY_CHANGED/);
  }
  const wrongSource = frozenReport(); wrongSource.source.bytes++;
  assert.throws(() => frozenExecutableReturnModelV1(wrongSource), /PRIOR_SOURCE_BINDING/);
  const wrongTraining = frozenReport(); wrongTraining.model.trainingRows++;
  assert.throws(() => frozenExecutableReturnModelV1(wrongTraining), /MODEL_SHAPE/);
});

test('pins the actual baseline bytes and frozen coefficient vector/cutoff', () => {
  const bytes = fs.readFileSync(new URL('./executable-return-alpha-20261014.json', import.meta.url));
  const report = verifyPinnedPriorReportBytesV1(bytes);
  const model = frozenExecutableReturnModelV1(report);
  assert.equal(report.cutoff, cutoff);
  assert.equal(model.cutoff, Date.parse(cutoff));
  assert.deepEqual(model.coefficients, report.model.coefficients);
  assert.deepEqual(model.mean, report.model.featureMean);
  assert.deepEqual(model.scale, report.model.featureScale);
  const changed = Buffer.from(bytes);
  changed[100] ^= 1;
  assert.throws(() => verifyPinnedPriorReportBytesV1(changed), /PRIOR_REPORT_HASH/);
});

test('uses an exact byte boundary even when observations share the same market timestamp', () => {
  const decisions = [
    {marketId: 'A', sequence: 1, byteEnd: 100, at: 1000},
    {marketId: 'A', sequence: 2, byteEnd: 101, at: 1000},
    {marketId: 'B', sequence: 3, byteEnd: 200, at: 1001},
  ];
  const labels = decisions.flatMap(row => ['YES', 'NO'].map(side => ({...row, side, net: 0.01})));
  const later = selectPostBoundaryRowsV1(decisions, labels, 100);
  assert.deepEqual(later.decisions.map(row => row.sequence), [2, 3]);
  assert.deepEqual(later.labels.map(row => [row.sequence, row.side]), [[2, 'YES'], [2, 'NO'], [3, 'YES'], [3, 'NO']]);
});

test('rejects duplicated source event identity and non-integer boundary', () => {
  assert.throws(() => selectPostBoundaryRowsV1([
    {marketId: 'A', sequence: 1, byteEnd: 101}, {marketId: 'A', sequence: 1, byteEnd: 102},
  ], [], 100), /DUPLICATE_DECISION_KEY/);
  assert.throws(() => selectPostBoundaryRowsV1([], [], 1.5), /BOUNDARY_INPUT/);
});

test('source hashes detect a changed byte even when Windows file identity is unchanged', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'baal-forward-source-'));
  const source = path.join(directory, 'books.jsonl');
  try {
    fs.writeFileSync(source, Buffer.from('abc\nxyz\n'));
    const beforeStat = fs.statSync(source);
    const beforeHash = await hashRangeV1(source, 8, 4);
    const fd = fs.openSync(source, 'r+');
    try { fs.writeSync(fd, Buffer.from('Abc\n'), 0, 4, 0); } finally { fs.closeSync(fd); }
    const afterStat = fs.statSync(source);
    const afterHash = await hashRangeV1(source, 8, 4);
    assert.notEqual(afterHash.sha256, beforeHash.sha256);
    const identityUnchanged = ['dev', 'ino', 'size', 'mtimeMs', 'ctimeMs']
      .every((key) => beforeStat[key] === afterStat[key]);
    if (identityUnchanged) assert.equal(assertStableSourceIdentityV1(beforeStat, afterStat), true);
    else assert.throws(() => assertStableSourceIdentityV1(beforeStat, afterStat), /SOURCE_CHANGED_DURING_READ/);
    assert.throws(() => assertStableSourceHashesV1(beforeHash, afterHash), /SOURCE_CHANGED_DURING_REPLAY/);
  } finally {
    fs.rmSync(directory, {recursive: true, force: true});
  }
});

test('runner has no refit path, network client, or competition dispatcher', () => {
  const source = fs.readFileSync(new URL('./run-executable-return-alpha-later-20261014.mjs', import.meta.url), 'utf8');
  assert.doesNotMatch(source, /fitExecutableReturnModelV1|\bfetch\s*\(|createClient|dispatch\s*\(|submitOrder/i);
  assert.match(source, /flag:\s*'wx'/);
  assert.equal(FORWARD_OUTPUT_RELATIVE_PATH_V1, 'data/executable-return-alpha-later-20261014-v2.json');
  assert.notEqual(FORWARD_OUTPUT_RELATIVE_PATH_V1, 'data/executable-return-alpha-20261014.json');
  assert.equal(fileURLToPath(import.meta.url).endsWith('.test.mjs'), true);
});
