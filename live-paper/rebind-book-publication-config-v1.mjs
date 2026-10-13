import fs from 'node:fs';
import path from 'node:path';
import {createHash, randomUUID} from 'node:crypto';
import {createInterface} from 'node:readline';
import {createReadStream} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {applyBookPublication, createBookPublicationState} from './book-publication-v3.mjs';
import {collectorNativePublicationConfig, collectorPublicationCodeIdentity} from './collector-publication-native.mjs';
import {nativePublicationState} from './native-publication.mjs';
import {collectFrozenPaperFiles} from './frozen-paper-graph.mjs';
import {payloadHash} from '../execution/ledger.mjs';
import {replaceFileSync} from '../execution/atomic-rename.mjs';

const need = (ok, code) => { if (!ok) throw Error(`PUBLICATION_REBIND_${code}`); };
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const expected = Object.freeze({
  priorGraphHash: '24aadd4118e3bf345ac1a16916db0abf752d2b42595647bf0be54e9725d1fdfc',
  currentGraphHash: 'e8541a26d8d6dbfc540e077ed4b34958dd685649b7a6c459e28096bcc8321996',
  changedPath: 'live-paper/book-publication-v2.mjs',
  priorChangedHash: '08090f8eac68dd883017ad7be77140ab18eefd206b76bdbb103c5e1e36fc3c53',
  currentChangedHash: '4414a64652714a555a41a9fdde4a509686ba4f39e3896ae5707731fd17146948',
  sourceSnapshot: '4824a0249e6974af6658b6a025c4d9db69fc63c20d2e62c5fa872412ef1da163',
  scopeHash: 'dad1ca43b7e5f5a1a130944a6034ea6a19a91f0106862fb1b3e2dbd36dd003c1',
  backend: Object.freeze({schemaVersion: 1, mode: 'native_public_book_prefix_v1',
    binaryHash: '9eaf8bc6890ca63240a88af28030cbadfa885913438be69c5c6e3c5627bf8d61',
    engineHash: '56706afa003ca4bf19eb68de209bfe64c24070dcf86a3c26336b6983a2dbb21e',
    runtimeLibraryHash: '13bf4e99b0193634ebdeb0bbdeff9753b39f1d04799183f664111014aabf4c2c'}),
  journal: Object.freeze({file: 'public-books.jsonl', bytes: 314348774,
    sha256: '9f366013075459a33732220ba1aaa4357964555e991248a2d7dcea8efba74d97',
    sequence: 136225, tailHash: '3b6413ba54cc63e910fc3f71a2d1973ef6caa4f3456947e1ea2c1a5ba71d48db'})
});
const here = path.dirname(fileURLToPath(import.meta.url));
const defaultWorkspace = path.dirname(here);
const defaultRuntime = () => process.env.SIG_PAPER_DIR || path.join(process.env.LOCALAPPDATA || defaultWorkspace, 'SigTradingCup', 'paper-runtime');
const defaultLock = () => path.join(process.env.LOCALAPPDATA || defaultWorkspace, 'SigTradingCup', 'bot.lock');

function exactObject(value, keys) {
  return value && typeof value === 'object' && !Array.isArray(value)
    && Object.keys(value).sort().join('\0') === [...keys].sort().join('\0');
}

export function planPublicationConfigRebindV1(config, currentIdentity) {
  need(exactObject(config, ['schemaVersion', 'mode', 'scopeHash', 'codeIdentity', 'nativeBackend', 'sourceSnapshot']), 'CONFIG_SHAPE');
  need(config.schemaVersion === 1 && config.mode === 'native_publication_collector_v1', 'CONFIG_KIND');
  need(config.scopeHash === expected.scopeHash && config.sourceSnapshot === expected.sourceSnapshot, 'CONFIG_PREDECESSOR');
  need(payloadHash(config.nativeBackend) === payloadHash(expected.backend), 'BACKEND_PREDECESSOR');
  need(payloadHash(config.codeIdentity) === expected.priorGraphHash, 'IDENTITY_PREDECESSOR');
  need(payloadHash(currentIdentity) === expected.currentGraphHash, 'IDENTITY_TARGET');
  const priorKeys = Object.keys(config.codeIdentity).sort(), currentKeys = Object.keys(currentIdentity).sort();
  need(priorKeys.length === 11 && priorKeys.join('\0') === currentKeys.join('\0'), 'IDENTITY_SCOPE');
  const changed = priorKeys.filter(name => config.codeIdentity[name] !== currentIdentity[name]);
  need(changed.length === 1 && changed[0] === expected.changedPath
    && config.codeIdentity[expected.changedPath] === expected.priorChangedHash
    && currentIdentity[expected.changedPath] === expected.currentChangedHash, 'IDENTITY_DELTA');
  return {...config, codeIdentity: structuredClone(currentIdentity)};
}

function regularFileBytes(file) {
  try {
    const stat = fs.lstatSync(file);
    need(stat.isFile() && !stat.isSymbolicLink(), 'CONTROL_FILE_TYPE');
    return fs.readFileSync(file);
  } catch (error) {
    if (error.code === 'ENOENT') return null;
    throw error;
  }
}

function fenceFileNames(directory) {
  if (!fs.existsSync(directory)) return [];
  return fs.readdirSync(directory, {withFileTypes: true})
    .filter(row => (row.isFile() || row.isSymbolicLink()) && /^(?:STOP|.*_STOP)$/.test(row.name))
    .map(row => path.join(directory, row.name));
}

function controlSnapshot({workspaceDir, runtimeDir}) {
  const captureDir = path.join(workspaceDir, 'capture-live');
  const files = new Set([
    ...fenceFileNames(workspaceDir), ...fenceFileNames(runtimeDir), ...fenceFileNames(captureDir),
    path.join(workspaceDir, 'STOP'), path.join(captureDir, 'STOP'),
    path.join(runtimeDir, 'STOP'), path.join(runtimeDir, 'COLLECTOR_STOP'),
    path.join(runtimeDir, 'EXECUTION_STOP'), path.join(runtimeDir, 'RATE_READONLY_INSPECTION'),
    path.join(runtimeDir, 'rate.json'), path.join(runtimeDir, 'coordinated-network-budget.json'),
    path.join(runtimeDir, 'pid.txt'), path.join(runtimeDir, 'realtime-capture.json'),
    path.join(runtimeDir, 'private-account-capture-request-v1.json'),
    path.join(runtimeDir, 'private-account-capture-status-v1.json')
  ]);
  if (fs.existsSync(runtimeDir)) for (const row of fs.readdirSync(runtimeDir, {withFileTypes: true})) {
    if (row.isFile() && /capture.*(?:request|status|active|fence|stop).*\.json$/i.test(row.name)) files.add(path.join(runtimeDir, row.name));
  }
  return Object.fromEntries([...files].sort().map(file => {
    const bytes = regularFileBytes(file);
    return [path.relative(path.join(workspaceDir, '..'), file).split(path.sep).join('/'), bytes === null ? null : sha(bytes)];
  }));
}

function assertSafeControls({workspaceDir, runtimeDir, lockPath, leaseBytes}) {
  need(!fs.existsSync(path.join(workspaceDir, 'STOP')), 'WORKSPACE_STOP');
  need(!fs.existsSync(path.join(runtimeDir, 'STOP')), 'RUNTIME_STOP');
  need(!fs.existsSync(path.join(runtimeDir, 'COLLECTOR_STOP')), 'COLLECTOR_STOP');
  need(!fs.existsSync(path.join(runtimeDir, 'REALTIME_STOP')), 'REALTIME_STOP');
  need(!fs.existsSync(path.join(workspaceDir, 'capture-live', 'STOP')), 'CAPTURE_STOP');
  need(fs.existsSync(path.join(runtimeDir, 'EXECUTION_STOP')), 'EXECUTION_STOP_REQUIRED');
  const rateBytes = regularFileBytes(path.join(runtimeDir, 'rate.json'));
  need(rateBytes !== null, 'RATE_MISSING');
  const rate = JSON.parse(rateBytes.toString('utf8'));
  need(rate.manualReview === false, 'RATE_MANUAL_REVIEW');
  const lock = regularFileBytes(lockPath);
  if (leaseBytes === null) need(lock === null, 'COLLECTOR_LOCK_PRESENT');
  else need(lock !== null && lock.equals(leaseBytes), 'LEASE_CHANGED');
  const pidBytes = regularFileBytes(path.join(runtimeDir, 'pid.txt'));
  if (pidBytes !== null) {
    const value = pidBytes.toString('utf8').trim();
    need(/^[1-9]\d{0,8}$/.test(value), 'PID_RECORD_INVALID');
    try { process.kill(Number(value), 0); throw Error('PUBLICATION_REBIND_COLLECTOR_RUNNING'); }
    catch (error) { if (error.message === 'PUBLICATION_REBIND_COLLECTOR_RUNNING' || error.code !== 'ESRCH') throw error; }
  }
}

async function fileHash(file) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest('hex');
}

async function verifyPublicJournal({workspaceDir, runtimeDir, config, identity}) {
  const universeFile = path.join(workspaceDir, 'live-paper', 'price-universe.json');
  const universeBytes = regularFileBytes(universeFile);
  need(universeBytes !== null, 'UNIVERSE_MISSING');
  const universe = JSON.parse(universeBytes.toString('utf8'));
  const initialState = createBookPublicationState(universe);
  need(initialState.scopeHash === expected.scopeHash, 'UNIVERSE_SCOPE');
  need(payloadHash(identity) === expected.currentGraphHash, 'IDENTITY_CHANGED');
  need(payloadHash(config.nativeBackend) === payloadHash(expected.backend), 'BACKEND_CHANGED');

  const journal = path.join(runtimeDir, expected.journal.file);
  const before = fs.statSync(journal);
  need(before.isFile() && before.size === expected.journal.bytes, 'JOURNAL_SIZE');
  const firstHash = await fileHash(journal);
  need(firstHash === expected.journal.sha256, 'JOURNAL_HASH');
  const fd = fs.openSync(journal, 'r');
  try {
    const last = Buffer.alloc(1);
    need(fs.readSync(fd, last, 0, 1, before.size - 1) === 1 && last[0] === 10, 'JOURNAL_TORN');
  } finally { fs.closeSync(fd); }

  const receiptDir = path.join(runtimeDir, `publication-rebind-v1-${randomUUID()}`);
  fs.mkdirSync(receiptDir, {mode: 0o700});
  let native;
  try {
    native = await nativePublicationState({runtimeDir, binding: config.nativeBackend, universe, journal,
      endByte: before.size, expectedSha: firstHash, receiptDir});
    let state = initialState, records = 0;
    for await (const line of createInterface({input: createReadStream(journal, {encoding: 'utf8'}), crlfDelay: Infinity})) {
      if (!line) continue;
      state = applyBookPublication(state, JSON.parse(line));
      records++;
    }
    const after = fs.statSync(journal), finalHash = await fileHash(journal);
    need(after.size === before.size && after.mtimeMs === before.mtimeMs && finalHash === firstHash, 'JOURNAL_RACED');
    need(records === expected.journal.sequence && native.sourceRecords === records
      && state.sequence === records && native.state.sequence === records, 'JOURNAL_SEQUENCE');
    need(state.scopeHash === expected.scopeHash && native.scopeHash === expected.scopeHash
      && state.hash === expected.journal.tailHash && native.state.hash === expected.journal.tailHash, 'JOURNAL_TAIL');
    need(native.sourceHash === firstHash && native.engineHash === expected.backend.engineHash, 'NATIVE_RECEIPT');
    need(payloadHash(state) === payloadHash(native.state), 'STATE_PARITY');
    need(sha(universeBytes) === sha(regularFileBytes(universeFile)), 'UNIVERSE_RACED');
    return {sourceHash: firstHash, sourceBytes: before.size, sourceRecords: records, scopeHash: state.scopeHash,
      tailHash: state.hash, engineHash: native.engineHash, completeStateParity: true, networkRequests: 0, competitionOrders: 0};
  } finally {
    fs.rmSync(receiptDir, {recursive: true, force: true});
  }
}

function replaceBytes(file, bytes) {
  const temp = `${file}.rebind-v1-${randomUUID()}.tmp`;
  let fd;
  try {
    fd = fs.openSync(temp, 'wx', 0o600);
    let offset = 0;
    while (offset < bytes.length) offset += fs.writeSync(fd, bytes, offset, bytes.length - offset);
    fs.fsyncSync(fd); fs.closeSync(fd); fd = undefined;
    replaceFileSync(temp, file);
  } finally {
    if (fd !== undefined) fs.closeSync(fd);
    if (fs.existsSync(temp)) fs.unlinkSync(temp);
  }
}

export async function rebindCollectorPublicationConfigV1({workspaceDir = defaultWorkspace, runtimeDir = defaultRuntime(),
  lockPath = defaultLock(), mode = 'check', currentIdentity = collectorPublicationCodeIdentity(),
  verifyJournal = verifyPublicJournal, postWriteCheck = null} = {}) {
  need(mode === 'check' || mode === 'apply', 'MODE');
  workspaceDir = path.resolve(workspaceDir); runtimeDir = path.resolve(runtimeDir); lockPath = path.resolve(lockPath);
  need(fs.existsSync(runtimeDir) && fs.statSync(runtimeDir).isDirectory(), 'RUNTIME_MISSING');
  const lockToken = Buffer.from(JSON.stringify({kind: 'publication-rebind-v1', token: randomUUID(), pid: process.pid}));
  let lockFd, lockCreated = false;
  try {
    lockFd = fs.openSync(lockPath, 'wx', 0o600); lockCreated = true;
    let offset = 0;
    while (offset < lockToken.length) {
      const written = fs.writeSync(lockFd, lockToken, offset, lockToken.length - offset);
      need(Number.isInteger(written) && written > 0, 'LOCK_WRITE');
      offset += written;
    }
    fs.fsyncSync(lockFd); fs.closeSync(lockFd); lockFd = undefined;
  } catch (error) {
    if (lockFd !== undefined) try { fs.closeSync(lockFd); } catch {}
    if (lockCreated) try {
      const stat = fs.lstatSync(lockPath), bytes = fs.readFileSync(lockPath);
      const ours = bytes.length <= lockToken.length && lockToken.subarray(0, bytes.length).equals(bytes);
      if (stat.isFile() && !stat.isSymbolicLink() && ours) fs.unlinkSync(lockPath);
    } catch {}
    if (error.code === 'EEXIST') throw Error('PUBLICATION_REBIND_COLLECTOR_LOCK_PRESENT');
    throw error;
  }
  let originalBytes, wrote = false, intendedBytes;
  try {
    assertSafeControls({workspaceDir, runtimeDir, lockPath, leaseBytes: lockToken});
    const controls = controlSnapshot({workspaceDir, runtimeDir});
    const configFile = path.join(runtimeDir, 'collector-native-publication.json');
    originalBytes = regularFileBytes(configFile);
    need(originalBytes !== null, 'CONFIG_MISSING');
    const originalHash = sha(originalBytes), original = JSON.parse(originalBytes.toString('utf8'));
    const target = planPublicationConfigRebindV1(original, currentIdentity);
    const proof = await verifyJournal({workspaceDir, runtimeDir, config: original, identity: currentIdentity});
    need(proof?.completeStateParity === true && proof.sourceHash === expected.journal.sha256
      && proof.sourceBytes === expected.journal.bytes && proof.sourceRecords === expected.journal.sequence
      && proof.scopeHash === expected.scopeHash && proof.tailHash === expected.journal.tailHash
      && proof.engineHash === expected.backend.engineHash && proof.networkRequests === 0 && proof.competitionOrders === 0, 'PROOF_INVALID');
    assertSafeControls({workspaceDir, runtimeDir, lockPath, leaseBytes: lockToken});
    need(payloadHash(controlSnapshot({workspaceDir, runtimeDir})) === payloadHash(controls), 'CONTROL_STATE_CHANGED');
    need(sha(regularFileBytes(configFile)) === originalHash, 'CONFIG_RACED');
    const summary = {status: mode === 'check' ? 'verified_no_change' : 'rebound', priorGraphHash: expected.priorGraphHash,
      currentGraphHash: expected.currentGraphHash, sourceHash: proof.sourceHash, sourceBytes: proof.sourceBytes,
      sourceRecords: proof.sourceRecords, stateParity: true, networkRequests: 0, competitionOrders: 0,
      executionStopPreserved: true};
    if (mode === 'check') return summary;

    intendedBytes = Buffer.from(`${JSON.stringify(target, null, 2)}\n`, 'utf8');
    assertSafeControls({workspaceDir, runtimeDir, lockPath, leaseBytes: lockToken});
    need(payloadHash(controlSnapshot({workspaceDir, runtimeDir})) === payloadHash(controls), 'CONTROL_STATE_CHANGED');
    need(sha(regularFileBytes(configFile)) === originalHash, 'CONFIG_RACED');
    replaceBytes(configFile, intendedBytes); wrote = true;
    assertSafeControls({workspaceDir, runtimeDir, lockPath, leaseBytes: lockToken});
    need(payloadHash(controlSnapshot({workspaceDir, runtimeDir})) === payloadHash(controls), 'CONTROL_STATE_CHANGED');
    need(sha(regularFileBytes(configFile)) === sha(intendedBytes), 'CONFIG_POSTWRITE_RACED');
    const updated = JSON.parse(regularFileBytes(configFile).toString('utf8'));
    need(payloadHash(updated.codeIdentity) === expected.currentGraphHash, 'CONFIG_POSTWRITE_IDENTITY');
    need(payloadHash({...updated, codeIdentity: original.codeIdentity}) === payloadHash(original), 'CONFIG_OTHER_FIELDS_CHANGED');
    if (postWriteCheck) await postWriteCheck({workspaceDir, runtimeDir, config: updated, identity: currentIdentity});
    else {
      const universe = JSON.parse(regularFileBytes(path.join(workspaceDir, 'live-paper', 'price-universe.json')).toString('utf8'));
      const bound = collectorNativePublicationConfig(runtimeDir, universe);
      need(payloadHash(bound?.codeIdentity) === expected.currentGraphHash, 'CONFIG_BINDING_REJECTED');
    }
    summary.configChangedFields = ['codeIdentity'];
    return summary;
  } catch (error) {
    if (wrote && originalBytes && intendedBytes) {
      try {
        const configFile = path.join(runtimeDir, 'collector-native-publication.json');
        if (sha(regularFileBytes(configFile) ?? Buffer.alloc(0)) === sha(intendedBytes)) {
          replaceBytes(configFile, originalBytes);
          need(sha(regularFileBytes(configFile)) === sha(originalBytes), 'ROLLBACK_VERIFY');
        }
      } catch { throw Error('PUBLICATION_REBIND_ROLLBACK_FAILED'); }
    }
    throw error;
  } finally {
    if (lockFd !== undefined) fs.closeSync(lockFd);
    const existingLock = regularFileBytes(lockPath);
    if (existingLock?.equals(lockToken)) fs.unlinkSync(lockPath);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2), mode = args.length === 1 && args[0] === '--check' ? 'check'
    : args.length === 1 && args[0] === '--apply' ? 'apply' : null;
  need(mode !== null, 'USAGE');
  try { console.log(JSON.stringify(await rebindCollectorPublicationConfigV1({mode}))); }
  catch (error) { console.error(error?.message?.startsWith('PUBLICATION_REBIND_') ? error.message : 'PUBLICATION_REBIND_FAILED'); process.exitCode = 1; }
}
