import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {collectorPublicationCodeIdentity} from './collector-publication-native.mjs';
import {payloadHash} from '../execution/ledger.mjs';
import {planPublicationConfigRebindV1, rebindCollectorPublicationConfigV1} from './rebind-book-publication-config-v1.mjs';

const scopeHash = 'dad1ca43b7e5f5a1a130944a6034ea6a19a91f0106862fb1b3e2dbd36dd003c1';
const sourceSnapshot = '4824a0249e6974af6658b6a025c4d9db69fc63c20d2e62c5fa872412ef1da163';
const nativeBackend = {schemaVersion: 1, mode: 'native_public_book_prefix_v1',
  binaryHash: '9eaf8bc6890ca63240a88af28030cbadfa885913438be69c5c6e3c5627bf8d61',
  engineHash: '56706afa003ca4bf19eb68de209bfe64c24070dcf86a3c26336b6983a2dbb21e',
  runtimeLibraryHash: '13bf4e99b0193634ebdeb0bbdeff9753b39f1d04799183f664111014aabf4c2c'};
const journalProof = {completeStateParity: true,
  sourceHash: '9f366013075459a33732220ba1aaa4357964555e991248a2d7dcea8efba74d97',
  sourceBytes: 314348774, sourceRecords: 136225, scopeHash,
  tailHash: '3b6413ba54cc63e910fc3f71a2d1973ef6caa4f3456947e1ea2c1a5ba71d48db',
  engineHash: nativeBackend.engineHash, networkRequests: 0, competitionOrders: 0};

function configFixture() {
  const current = collectorPublicationCodeIdentity();
  const old = {...current, 'live-paper/book-publication-v2.mjs': '08090f8eac68dd883017ad7be77140ab18eefd206b76bdbb103c5e1e36fc3c53'};
  return {current, config: {schemaVersion: 1, mode: 'native_publication_collector_v1', scopeHash,
    codeIdentity: old, nativeBackend: structuredClone(nativeBackend), sourceSnapshot}};
}

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'publication-rebind-v1-'));
  t.after(() => fs.rmSync(root, {recursive: true, force: true}));
  const workspaceDir = path.join(root, 'workspace'), runtimeDir = path.join(root, 'paper-runtime');
  const lockPath = path.join(root, 'SigTradingCup', 'bot.lock');
  fs.mkdirSync(path.join(workspaceDir, 'live-paper'), {recursive: true});
  fs.mkdirSync(path.join(workspaceDir, 'capture-live'), {recursive: true});
  fs.mkdirSync(runtimeDir, {recursive: true});
  fs.mkdirSync(path.dirname(lockPath), {recursive: true});
  fs.writeFileSync(path.join(runtimeDir, 'EXECUTION_STOP'), 'preserve');
  fs.writeFileSync(path.join(runtimeDir, 'rate.json'), JSON.stringify({next: 0, manualReview: false}));
  const {current, config} = configFixture();
  const configFile = path.join(runtimeDir, 'collector-native-publication.json');
  const originalBytes = Buffer.from(`${JSON.stringify(config, null, 2)}\n`);
  fs.writeFileSync(configFile, originalBytes);
  const verifyJournal = async () => structuredClone(journalProof);
  const postWriteCheck = async ({config: actual, identity}) => {
    assert.equal(payloadHash(actual.codeIdentity), payloadHash(identity));
  };
  return {root, workspaceDir, runtimeDir, lockPath, configFile, originalBytes, current,
    verifyJournal, postWriteCheck};
}

test('plans only the exact one-file predecessor-to-target identity transition', () => {
  const {config, current} = configFixture(), target = planPublicationConfigRebindV1(config, current);
  assert.deepEqual(Object.keys(target), Object.keys(config));
  assert.deepEqual(target.codeIdentity, current);
  assert.equal(target.scopeHash, config.scopeHash);
  assert.deepEqual(target.nativeBackend, config.nativeBackend);
  assert.equal(target.sourceSnapshot, config.sourceSnapshot);
  assert.equal(Object.keys(config.codeIdentity).length, 11);
});

test('rejects any changed source, unknown predecessor, or native backend', () => {
  const {config, current} = configFixture();
  for (const mutate of [
    c => { c.codeIdentity['live-paper/model.mjs'] = '0'.repeat(64); },
    c => { c.codeIdentity.extra = '0'.repeat(64); },
    c => { c.sourceSnapshot = '0'.repeat(64); },
    c => { c.nativeBackend.binaryHash = '0'.repeat(64); },
    c => { c.scopeHash = '0'.repeat(64); }
  ]) {
    const changed = structuredClone(config); mutate(changed);
    assert.throws(() => planPublicationConfigRebindV1(changed, current), /^Error: PUBLICATION_REBIND_/);
  }
  const changedTarget = structuredClone(current); changedTarget['live-paper/book-publication-v2.mjs'] = '1'.repeat(64);
  assert.throws(() => planPublicationConfigRebindV1(config, changedTarget), /PUBLICATION_REBIND_IDENTITY_TARGET/);
});

test('check fully verifies injected parity evidence and leaves config bytes unchanged', async t => {
  const f = fixture(t), before = fs.readFileSync(f.configFile);
  const result = await rebindCollectorPublicationConfigV1({...f, mode: 'check'});
  assert.equal(result.status, 'verified_no_change');
  assert.equal(result.stateParity, true);
  assert.equal(result.networkRequests, 0);
  assert.equal(result.competitionOrders, 0);
  assert.deepEqual(fs.readFileSync(f.configFile), before);
  assert.equal(fs.existsSync(f.lockPath), false);
});

test('apply replaces only codeIdentity and preserves the execution stop and rate bytes', async t => {
  const f = fixture(t), rate = fs.readFileSync(path.join(f.runtimeDir, 'rate.json'));
  const stop = fs.readFileSync(path.join(f.runtimeDir, 'EXECUTION_STOP'));
  const result = await rebindCollectorPublicationConfigV1({...f, mode: 'apply'});
  const updated = JSON.parse(fs.readFileSync(f.configFile, 'utf8'));
  const original = JSON.parse(f.originalBytes.toString('utf8'));
  assert.equal(result.status, 'rebound');
  assert.deepEqual(result.configChangedFields, ['codeIdentity']);
  assert.equal(payloadHash(updated.codeIdentity), payloadHash(f.current));
  assert.equal(payloadHash({...updated, codeIdentity: original.codeIdentity}), payloadHash(original));
  assert.deepEqual(fs.readFileSync(path.join(f.runtimeDir, 'rate.json')), rate);
  assert.deepEqual(fs.readFileSync(path.join(f.runtimeDir, 'EXECUTION_STOP')), stop);
  assert.equal(fs.existsSync(f.lockPath), false);
});

test('rejects absent execution stop and leaves config untouched', async t => {
  const f = fixture(t); fs.unlinkSync(path.join(f.runtimeDir, 'EXECUTION_STOP'));
  await assert.rejects(rebindCollectorPublicationConfigV1({...f, mode: 'apply'}), /PUBLICATION_REBIND_EXECUTION_STOP_REQUIRED/);
  assert.deepEqual(fs.readFileSync(f.configFile), f.originalBytes);
});

test('rejects workspace, collector, and realtime stops without altering any stop bytes', async t => {
  for (const relative of ['workspace/STOP', 'paper-runtime/COLLECTOR_STOP', 'paper-runtime/REALTIME_STOP']) {
    const f = fixture(t), file = path.join(f.root, relative); fs.writeFileSync(file, 'retain');
    await assert.rejects(rebindCollectorPublicationConfigV1({...f, mode: 'apply'}), /PUBLICATION_REBIND_(WORKSPACE_STOP|COLLECTOR_STOP|REALTIME_STOP)/);
    assert.equal(fs.readFileSync(file, 'utf8'), 'retain');
    assert.deepEqual(fs.readFileSync(f.configFile), f.originalBytes);
  }
});

test('rejects rate manual-review and existing collector lease', async t => {
  const a = fixture(t); fs.writeFileSync(path.join(a.runtimeDir, 'rate.json'), JSON.stringify({next: 0, manualReview: true}));
  await assert.rejects(rebindCollectorPublicationConfigV1({...a, mode: 'apply'}), /PUBLICATION_REBIND_RATE_MANUAL_REVIEW/);
  assert.deepEqual(fs.readFileSync(a.configFile), a.originalBytes);
  const b = fixture(t); fs.mkdirSync(path.dirname(b.lockPath), {recursive: true}); fs.writeFileSync(b.lockPath, 'foreign owner');
  await assert.rejects(rebindCollectorPublicationConfigV1({...b, mode: 'apply'}), /PUBLICATION_REBIND_COLLECTOR_LOCK_PRESENT/);
  assert.equal(fs.readFileSync(b.lockPath, 'utf8'), 'foreign owner');
});

test('rejects a live PID instead of treating a stale lock record as dead', async t => {
  const f = fixture(t); fs.writeFileSync(path.join(f.runtimeDir, 'pid.txt'), String(process.pid));
  await assert.rejects(rebindCollectorPublicationConfigV1({...f, mode: 'apply'}), /PUBLICATION_REBIND_COLLECTOR_RUNNING/);
  assert.deepEqual(fs.readFileSync(f.configFile), f.originalBytes);
});

test('aborts if control fences change during parity verification', async t => {
  const f = fixture(t), rateFile = path.join(f.runtimeDir, 'rate.json');
  const verifyJournal = async () => { fs.writeFileSync(rateFile, JSON.stringify({next: 1, manualReview: false})); return structuredClone(journalProof); };
  await assert.rejects(rebindCollectorPublicationConfigV1({...f, verifyJournal, mode: 'apply'}), /PUBLICATION_REBIND_CONTROL_STATE_CHANGED/);
  assert.deepEqual(fs.readFileSync(f.configFile), f.originalBytes);
});

test('rolls back exact original config bytes when post-write validation fails', async t => {
  const f = fixture(t), postWriteCheck = async () => { throw Error('injected validation failure'); };
  await assert.rejects(rebindCollectorPublicationConfigV1({...f, postWriteCheck, mode: 'apply'}), /injected validation failure/);
  assert.deepEqual(fs.readFileSync(f.configFile), f.originalBytes);
  assert.equal(fs.existsSync(f.lockPath), false);
});

test('rejects missing or altered native parity receipt fields', async t => {
  const f = fixture(t), verifyJournal = async () => ({...journalProof, competitionOrders: 1});
  await assert.rejects(rebindCollectorPublicationConfigV1({...f, verifyJournal, mode: 'apply'}), /PUBLICATION_REBIND_PROOF_INVALID/);
  assert.deepEqual(fs.readFileSync(f.configFile), f.originalBytes);
});
