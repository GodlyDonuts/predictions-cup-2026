import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {workerHealth} from './worker-health.mjs';

// Read-only diagnostics. Never imports account/network clients, reads STOP text,
// repairs a lock, restarts a process, or writes a projection/runtime file.
const sha = x => x === undefined ? null : createHash('sha256').update(JSON.stringify(x)).digest('hex');
const hash = x => typeof x === 'string' && /^[a-f0-9]{64}$/.test(x) ? x : null;
const pid = x => Number.isSafeInteger(Number(x)) && Number(x) > 0 ? Number(x) : null;
const status = x => typeof x === 'string' && /^[a-z][a-z0-9_]{0,63}$/.test(x) ? x : 'unknown';
const safeTime = x => typeof x === 'string' && Number.isFinite(Date.parse(x)) ? new Date(x).toISOString() : null;
const count = x => Number.isSafeInteger(x) && x >= 0 ? x : null;
const specs = [
  {name: 'collector', stats: 'stats', pid: 'pid.txt', script: 'live-paper/runner.mjs', stop: 'COLLECTOR_STOP'},
  {name: 'fee-free', script: 'live-paper/fee-free-shadow.mjs', stop: 'FEE_FREE_STOP'},
  {name: 'ml', script: 'live-paper/ml-shadow.mjs', stop: 'ML_STOP'},
  ...['dip', 'dip-scale', 'dip-responsive'].map(name => ({name, script: 'live-paper/dip-shadow.mjs', stop: 'DIP_STOP', cohortArgument: name})),
  {name: 'shock', script: 'live-paper/shock-shadow.mjs', stop: 'SHOCK_STOP'},
  {name: 'passive', script: 'live-paper/passive-watch.mjs', stop: 'PASSIVE_STOP', lock: 'passive-watch.lock'},
  {name: 'basket', script: 'live-paper/basket-watch.mjs', stop: 'BASKET_STOP', lock: 'basket-watch.lock'},
  {name: 'performance', script: 'execution/performance-watch.mjs', stop: 'PERFORMANCE_STOP', lock: 'performance-watch.lock', derivedProjection: true},
];
function json(file) { try { const stat = fs.statSync(file); if (stat.size > 4 * 1024 * 1024) return {error: 'oversized'};
  const value = JSON.parse(fs.readFileSync(file, 'utf8'));
  return value && typeof value === 'object' && !Array.isArray(value) ? {value, modifiedAt: stat.mtime.toISOString()} : {error: 'invalid_schema'};
} catch (e) { return {error: e.code === 'ENOENT' ? 'missing' : 'unreadable'}; } }
function pidFile(file) { try { const stat = fs.statSync(file); const value = stat.size < 100 ? pid(fs.readFileSync(file, 'utf8').trim()) : null;
  return {present: true, pid: value, modifiedAt: stat.mtime.toISOString()}; } catch { return {present: false, pid: null, modifiedAt: null}; } }
function stopFile(file, provenance) { try { return {present: true, provenance, modifiedAt: fs.statSync(file).mtime.toISOString()}; } catch { return {present: false, provenance, modifiedAt: null}; } }
function captureMetadata(runtimeDir, bank) {
  try {
    if (!bank || typeof bank.sourceFile !== 'string' || !/^realtime-[A-Za-z0-9._-]+\.jsonl$/.test(bank.sourceFile)) throw Error();
    const session = json(path.join(runtimeDir, 'realtime-session.json')).value;
    if (session?.file !== bank.sourceFile) return {available: false, reason: 'source_session_changed'};
    const stat = fs.statSync(path.join(runtimeDir, bank.sourceFile));
    return {available: stat.isFile() && stat.size > 0, kind: 'realtime_capture', at: stat.mtime.toISOString(), bytes: stat.size, maxAgeMs: 20000};
  } catch { return {available: false, reason: 'capture_unavailable'}; }
}

// Only the listed runtime PIDs are inspected. Raw process command lines remain
// in memory and never enter the returned health report or error output.
export function probeLocalProcesses(pids) {
  const ids = [...new Set(pids.filter(p => pid(p)))], result = Object.fromEntries(ids.map(id => {
    try { process.kill(id, 0); return [id, {alive: true, commandLine: null, startedAt: null}]; }
    catch (e) { return [id, {alive: e.code === 'ESRCH' ? false : null, commandLine: null, startedAt: null}]; }
  }));
  if (process.platform !== 'win32' || !ids.length) return result;
  try {
    const filter = ids.map(id => 'ProcessId = ' + id).join(' OR ');
    const command = `Get-CimInstance Win32_Process -Filter '${filter}' | ForEach-Object { [pscustomobject]@{pid=$_.ProcessId; commandLine=$_.CommandLine; startedAt=$_.CreationDate.ToUniversalTime().ToString('o')} } | ConvertTo-Json -Compress`;
    const query = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', command], {windowsHide: true, encoding: 'utf8', timeout: 5000, maxBuffer: 1024 * 1024});
    if (query.status !== 0 || !query.stdout.trim()) return result;
    const parsed = JSON.parse(query.stdout);
    for (const row of Array.isArray(parsed) ? parsed : [parsed]) if (result[row.pid]) result[row.pid] = {alive: true, commandLine: row.commandLine, startedAt: safeTime(row.startedAt)};
  } catch { /* Liveness remains useful; identity will be explicitly unverified. */ }
  return result;
}

// Metadata scan only: do not deserialize market/account payloads. No claim of
// full hash-chain verification is made; the actual workers own that validation.
export async function sourceMetadata(file, wanted = []) {
  try {
    const stat = fs.statSync(file), target = new Set(wanted.filter(Number.isSafeInteger)), clocks = {}, hashes = {};
    if (!stat.size) return {available: false, reason: 'empty_source', clocks};
    const fd = fs.openSync(file, 'r'), lastByte = Buffer.alloc(1);
    try { fs.readSync(fd, lastByte, 0, 1, stat.size - 1); } finally { fs.closeSync(fd); }
    let latest = null, pending = null, malformed = 0;
    const inspect = line => {
      const head = /^\{"sequence":(\d+),"previous":"[a-f0-9]*","at":"([^"]+)"/.exec(line);
      const tail = /,"hash":"([a-f0-9]{64})"\}$/.exec(line);
      if (!head || !tail || !safeTime(head[2])) { malformed++; return; }
      const row = {sequence: Number(head[1]), at: safeTime(head[2]), hash: tail[1]};
      latest = row; if (target.has(row.sequence)) { clocks[row.sequence] = row.at; hashes[row.sequence] = row.hash; }
    };
    const lines = readline.createInterface({input: fs.createReadStream(file, {end: stat.size - 1}), crlfDelay: Infinity});
    for await (const line of lines) { if (pending !== null) inspect(pending); pending = line; }
    if (lastByte[0] === 10 && pending !== null) inspect(pending);
    return {available: latest !== null, ...latest, modifiedAt: stat.mtime.toISOString(), clocks, hashes, malformedMetadataRows: malformed,
      partialTailExcluded: lastByte[0] !== 10, integrityVerified: false};
  } catch { return {available: false, reason: 'source_unavailable', clocks: {}, integrityVerified: false}; }
}

export function assessWorker(input, now = Date.now()) {
  const {spec, report = {}, bank, reportPresent, bankPresent, claims = [], lock = {}, processes = {}, source = {}, stops = []} = input;
  const reportedStatus = status(report.status ?? report.runtime?.status), timestamp = report.updatedAt ?? report.reportedAt;
  const heartbeat = workerHealth({updatedAt: timestamp, status: reportedStatus}, now);
  const reportedPids = [...new Set(claims.map(c => c.pid).filter(Boolean))], processId = reportedPids.length === 1 ? reportedPids[0] : null;
  const probe = processId === null ? {} : processes[processId] ?? {};
  const command = typeof probe.commandLine === 'string' ? probe.commandLine.replaceAll('\\', '/').toLowerCase() : null;
  const tokens = command?.match(/"[^"]*"|'[^']*'|[^\s]+/g)?.map(token => token.replace(/^["']|["']$/g, '')) ?? [];
  const expectedScript = spec.script.toLowerCase();
  const scriptMatches = tokens.some(token => token === expectedScript || token.endsWith('/' + expectedScript));
  let identity = command === null ? 'unverified' : scriptMatches
    && (!spec.cohortArgument || new RegExp('--name[ =]+["\']?' + spec.cohortArgument + '(?:["\']|\\s|$)').test(command)) ? 'matches_expected_script' : 'different_process';
  const oldestClaim = claims.filter(c => c.pid === processId && c.modifiedAt).map(c => Date.parse(c.modifiedAt));
  if (probe.startedAt && oldestClaim.length && Date.parse(probe.startedAt) > Math.min(...oldestClaim) + 5000) identity = 'pid_reused_after_claim';
  const processState = reportedPids.length > 1 ? 'conflicting_pid_claims' : processId === null ? 'unknown_pid' : probe.alive === false ? 'exited'
    : probe.alive !== true ? 'unverified' : ['different_process', 'pid_reused_after_claim'].includes(identity) ? 'wrong_process' : 'alive';
  let lineage = {status: spec.name === 'collector' || spec.derivedProjection ? 'not_applicable' : 'unavailable', bankHash: bank ? sha(bank) : null, reportedBankHash: hash(report.bankHash)};
  if (bank && spec.name !== 'collector') {
    if (spec.name === 'passive') lineage.status = report.sessionId === bank.sessionId && report.cutoff === bank.cutoff
      && report.sourceFile === bank.sourceFile && report.codeIdentity && bank.codeIdentity && sha(report.codeIdentity) === sha(bank.codeIdentity)
      && Array.isArray(bank.scope?.markets) && bank.scope.markets.length > 0 && Array.isArray(report.markets)
      && report.markets.length === bank.scope.markets.length && report.markets.every(m => m && typeof m.ready === 'boolean' && m.sourceHash === bank.sourceHash) ? 'reported_session_matches' : 'mismatch';
    else lineage.status = hash(report.bankHash) === sha(bank) ? 'reported_bank_matches' : 'mismatch';
    lineage.cutoff = safeTime(bank.cutoff); lineage.frozenSourceHash = hash(bank.sourceHash);
  }
  const sequence = Number.isSafeInteger(report.sourceSequence) ? report.sourceSequence : null;
  const lag = sequence !== null && source.available ? source.sequence - sequence : null;
  const processedAt = sequence === null ? null : source.clocks?.[sequence] ?? null;
  const reportedSourceHash = hash(report.sourceHash ?? report.lastRecordHash), capturedSourceHash = hash(source.hashes?.[sequence]);
  const sourceIdentity = reportedSourceHash && capturedSourceHash ? reportedSourceHash === capturedSourceHash ? 'matches_source_record' : 'mismatch' : 'unverified';
  const sourceAgeMs = source.at && now >= Date.parse(source.at) ? now - Date.parse(source.at) : null;
  const sourceDelayMs = processedAt && source.at ? Math.max(0, Date.parse(source.at) - Date.parse(processedAt)) : null;
  const activeStops = stops.filter(s => s.present), configured = !!(reportPresent || bankPresent || claims.length || lock.present), reasons = [];
  if (reportedStatus === 'failed') reasons.push('worker_reported_failure');
  if (processState !== 'alive') reasons.push('process_' + processState);
  if (identity !== 'matches_expected_script' && probe.alive === true) reasons.push('process_identity_' + identity);
  if (['stale_worker', 'invalid_heartbeat'].includes(heartbeat.status)) reasons.push(heartbeat.status);
  if (!source.available) reasons.push('source_unavailable');
  if (source.malformedMetadataRows > 0) reasons.push('source_metadata_invalid');
  if (lag !== null && lag < 0) reasons.push('worker_ahead_of_source_snapshot');
  if (lag !== null && lag > 3 && sourceDelayMs !== null && sourceDelayMs > 60000) reasons.push('source_replay_lag');
  if (sourceAgeMs !== null && sourceAgeMs > (source.maxAgeMs ?? 60000)) reasons.push('source_stale');
  if (source.bytes !== undefined && (!Number.isSafeInteger(report.sourceBytes) || report.sourceBytes > source.bytes)) reasons.push('source_metadata_invalid');
  if (lineage.status === 'mismatch') reasons.push('cohort_lineage_mismatch');
  if (sourceIdentity === 'mismatch') reasons.push('source_record_hash_mismatch');
  if (!lock.present && processState === 'alive') reasons.push('worker_lock_missing');
  if (activeStops.length) reasons.push('stop_marker_present');
  let overall = !configured ? 'not_initialized' : reportedStatus === 'failed' ? 'failed' : lineage.status === 'mismatch' || sourceIdentity === 'mismatch' ? 'lineage_mismatch'
    : ['wrong_process', 'conflicting_pid_claims'].includes(processState) ? 'pid_conflict' : processState === 'exited' ? 'exited'
      : activeStops.length ? processState === 'alive' ? 'stop_pending' : 'stopped' : heartbeat.status === 'invalid_heartbeat' ? 'invalid_clock'
        : heartbeat.status === 'stale_worker' ? 'stale_heartbeat' : reasons.includes('source_replay_lag') ? 'source_lag'
          : reasons.includes('worker_ahead_of_source_snapshot') ? 'source_snapshot_unverified'
          : reasons.includes('source_stale') ? 'source_stale' : reasons.includes('source_unavailable') ? 'source_unavailable'
            : reasons.includes('source_metadata_invalid') || lineage.status === 'unavailable' || processState !== 'alive' || identity !== 'matches_expected_script' || !lock.present ? 'unverified'
            : reportedStatus === 'running' ? 'running' : reportedStatus;
  return {name: spec.name, status: overall, configured, reportedStatus, pid: processId,
    process: {status: processState, identity, startedAt: safeTime(probe.startedAt), claims: claims.map(c => ({source: c.source, pid: c.pid}))},
    heartbeat: {updatedAt: safeTime(timestamp), ageMs: heartbeat.heartbeatAgeMs},
    source: {kind: source.kind ?? 'collector_journal', available: source.available === true, processedSequence: sequence, latestSequence: source.sequence ?? null, lagRecords: lag, processedAt,
      lagBytes: Number.isSafeInteger(source.bytes) && Number.isSafeInteger(report.sourceBytes) ? source.bytes - report.sourceBytes : null,
      delayBehindSourceMs: sourceDelayMs, sourceLatestAt: source.at ?? null, sourceAgeMs, identity: sourceIdentity}, lockPresent: !!lock.present, lineage,
    stops: activeStops.map(({provenance, modifiedAt}) => ({provenance, modifiedAt})), reasons};
}

export async function fleetHealth({runtimeDir, workspaceDir = path.dirname(path.dirname(fileURLToPath(import.meta.url))),
  collectorLockPath = path.join(process.env.LOCALAPPDATA || path.dirname(runtimeDir), 'SigTradingCup', 'bot.lock'),
  now, probeProcesses = probeLocalProcesses} = {}) {
  if (!runtimeDir || (now !== undefined && !Number.isFinite(now))) throw Error('Runtime directory and valid clock required');
  runtimeDir = path.resolve(runtimeDir); workspaceDir = path.resolve(workspaceDir);
  const globalStops = [stopFile(path.join(workspaceDir, 'STOP'), 'workspace_stop'), stopFile(path.join(runtimeDir, 'STOP'), 'runtime_stop')];
  const inputs = specs.map(spec => {
    const r = json(path.join(runtimeDir, (spec.stats ?? spec.name + '-stats') + '.json')),
      b = spec.name === 'collector' || spec.derivedProjection ? {} : json(path.join(runtimeDir, spec.name + '-bank.json')),
      p = pidFile(path.join(runtimeDir, spec.pid ?? spec.name + '-pid.txt')),
      l = pidFile(spec.name === 'collector' ? collectorLockPath : path.join(runtimeDir, spec.lock ?? spec.name + '-shadow.lock'));
    const claims = [{source: 'pid_file', ...p}, {source: 'lock', ...l}, {source: 'report', pid: pid(r.value?.runtime?.pid ?? r.value?.pid), modifiedAt: r.modifiedAt}].filter(c => c.pid);
    return {spec, report: r.value, bank: b.value, reportPresent: r.error !== 'missing', bankPresent: b.value !== undefined, claims, lock: l,
      stops: [...globalStops, stopFile(path.join(runtimeDir, spec.stop), 'worker_stop')]};
  });
  const source = await sourceMetadata(path.join(runtimeDir, 'journal.jsonl'), inputs.map(x => x.report?.sourceSequence));
  const processes = await probeProcesses(inputs.flatMap(x => x.claims.map(c => c.pid)));
  const official = json(path.join(runtimeDir, 'competition-stats.json')).value, realtime = json(path.join(runtimeDir, 'realtime-stats.json')).value;
  const passiveSource = captureMetadata(runtimeDir, inputs.find(x => x.spec.name === 'passive')?.bank);
  const executionStop = stopFile(path.join(runtimeDir, 'EXECUTION_STOP'), 'execution_stop');
  now ??= Date.now(); // Assess after local reads so their receipt clocks cannot appear future-dated merely because probing took time.
  const workers = inputs.map(x => assessWorker({...x, processes, source: x.spec.name === 'passive' ? passiveSource : source}, now));
  const executionTime = safeTime(official?.updatedAt), realtimeTime = safeTime(realtime?.at);
  const realtimeHealth = workerHealth({updatedAt: realtimeTime, status: 'fresh'}, now);
  const realtimeMarketsValid = Array.isArray(realtime?.markets) && realtime.markets.length > 0 && realtime.markets.every(m => m && typeof m.ready === 'boolean');
  return {schemaVersion: 1, mode: 'read_only_local_fleet_diagnostic', checkedAt: new Date(now).toISOString(),
    status: !workers.some(w => w.configured) ? 'not_initialized' : workers.some(w => w.configured && ['failed', 'exited', 'pid_conflict', 'lineage_mismatch'].includes(w.status)) ? 'attention_required'
      : workers.some(w => w.configured && w.status !== 'running') ? 'degraded' : 'running',
    workers, source: {...source, clocks: undefined, hashes: undefined},
    officialContext: {execution: {status: status(official?.status), updatedAt: executionTime,
      ageMs: executionTime && now >= Date.parse(executionTime) ? now - Date.parse(executionTime) : null,
      stopPresent: executionStop.present, collectorProcessStatus: workers.find(w => w.name === 'collector')?.process.status},
      realtime: {updatedAt: realtimeTime, status: realtimeMarketsValid ? realtimeHealth.status : 'invalid_schema', ageMs: realtimeHealth.heartbeatAgeMs,
        configuredMarkets: realtimeMarketsValid ? realtime.markets.length : null,
        readyMarkets: realtimeMarketsValid && realtimeHealth.status === 'fresh' ? realtime.markets.filter(m => m.ready).length : null,
        marketStreamObserved: typeof realtime?.marketStreamObserved === 'boolean' ? realtime.marketStreamObserved : null,
        eventsReceived: count(realtime?.transport?.eventsReceived), knownBroadcasts: Object.fromEntries(['market_batch', 'book_dirty', 'market_settled']
          .map(name => [name, count(realtime?.transport?.diagnostics?.knownBroadcasts?.[name])]))}},
    limitations: ['PID liveness alone does not prove process identity; missing metadata remains unverified.',
      'Reported bank identity is compared, but this diagnostic does not replay worker journals or establish current market readiness.',
      'Source delay compares journal append clocks, not market latency.', 'No account bodies, balances, keys, STOP text or raw errors are exported.'],
    networkRequests: 0, runtimeWrites: 0, automaticRestarts: 0};
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2), value = name => { const i = args.indexOf(name); return i < 0 ? undefined : args[i + 1]; };
  try {
    const runtimeDir = value('--runtime') || process.env.SIG_PAPER_DIR || path.join(process.env.LOCALAPPDATA, 'SigTradingCup', 'paper-runtime');
    const report = await fleetHealth({runtimeDir, workspaceDir: value('--workspace')});
    process.stdout.write(JSON.stringify(report, null, args.includes('--pretty') ? 2 : 0) + '\n');
  } catch { process.stderr.write('Local fleet diagnostic unavailable; no files were changed.\n'); process.exitCode = 1; }
}
