#!/usr/bin/env python3
"""REVIEW-ONLY BUY failure recovery; stage/promote only. All new build/failure/prior pins intentionally unset. Binary-only Control delta, no financial event writes or review reload."""
import copy
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
import time
import uuid
ENGINE = '8acc461bc02fb9236100058fe464b203d5da005375632af05c176223c7246754'
SOURCE = None
BASE_MANIFEST = '09308fe3a64695f9d38882aea03ef01803f8b0e2fb6e1f107e9d137f69ee6096'
BASE_MANIFEST_PATH = '/tmp/moloch-sell-timestamp-recovery-20261016/source-manifest.json'
HELPER = '1e0da4b462dd0627dc597b725fc592530d6ae3c91babe09d4ecb641d2ce87133'
DROPIN = 'ff4237fb5e0b5f2a68159e4fa43d737eefc68ebdd422f56eea068c4ffba0e795'
BACKUP_NAME = '10-bounded-restart.conf.before-exec-condition-20261016'
BACKUP_SHA = 'ddbe320f3fcd657d719a2ec28f67282ce2eb0c90ff8bf03eff9308871121936e'
PRIOR_SOURCE = 'ec8b0fef24ec6b43960784c05d40b7862a7643e3'
PRIOR_BINARY = '40fbaa4731be72c735b6bfc6bff86423cd833e57e20ad80640935d585be8a8d6'
PRIOR_RECEIPT_PATH = '/tmp/moloch-early-entry-recovery-production-rollout.json'
PRIOR_CONTROL_PATH = '/home/archon/apps/moloch/shared/moloch-live-early-entry-recovery-20261016.json'
PRIOR_RECEIPT = None
PRIOR_CONTROL = None
PRIOR_UNIT = None
BRIDGE_SHA = '76fab91acccfe26cfb1a80010a7b1fb00df7f92f02dc20d5127192258bcf0723'
CHANGES = None
ALLOWED_PATHS = {'moloch/live-runtime/src/feedback.rs'}
ADDED_PATHS = set()
EXPECTED_CHECKS = None
TEST_LOGS = None
RELEASE_LOG = None
PROBE_LOGS = None
BINARY_SHA = None
SOURCE_MANIFEST_SHA = None
VALIDATION_SHA = None
OPERATOR = 'paper6_buy_fill_recovery_rollout_v1'
OLD_PID = 534982

def require(ok, code):
    if not ok:
        raise RuntimeError(code)

def hash_bytes(data):
    return hashlib.sha256(data).hexdigest()

def checked_sha(value):
    require(isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value), 'SHA256_REQUIRED')
    return value

def controls(before, after, binary_sha):
    require(before.get('ordinarySideEdge') == 'enforce' and before.get('defensiveCancel') is True, 'PRESERVE_ENFORCE_AND_DEFENSE_REQUIRED')
    expected = copy.deepcopy(before)
    expected['activation']['binarySha256'] = checked_sha(binary_sha)
    require(after == expected, 'BINARY_ONLY_CONTROL_CHANGE_REQUIRED')

def freeze_ready():
    require(isinstance(SOURCE, str) and re.fullmatch('[0-9a-f]{40}', SOURCE) and (SOURCE != PRIOR_SOURCE), 'SOURCE_NOT_FROZEN')
    require(isinstance(CHANGES, dict) and set(CHANGES) == ALLOWED_PATHS and (len(CHANGES) == 1), 'EXACT_FEEDBACK_ONLY_SOURCE_DELTA')
    require(isinstance(EXPECTED_CHECKS, dict) and set(EXPECTED_CHECKS) == {'library', 'worker'} and all((type(v) is int and v > 0 for v in EXPECTED_CHECKS.values())), 'ACTUAL_TEST_COUNTS_REQUIRED')
    require(isinstance(TEST_LOGS, dict) and set(TEST_LOGS) == set(EXPECTED_CHECKS) and isinstance(PROBE_LOGS, dict) and (set(PROBE_LOGS) == {'engine-probe-build', 'engine-probe-run'}), 'ACTUAL_CHECK_LOGS_REQUIRED')
    for value in list(CHANGES.values()) + list(TEST_LOGS.values()) + list(PROBE_LOGS.values()) + [BINARY_SHA, SOURCE_MANIFEST_SHA, VALIDATION_SHA, RELEASE_LOG, PRIOR_RECEIPT, PRIOR_CONTROL, PRIOR_UNIT, FAILURE_STOP_SHA, FAILURE_STATUS_SHA, AMBIGUOUS_RECEIPT_SHA, ORIGINAL_ORDER_SHA, ORIGINAL_ACK_SHA, RECOVERY_REVIEW_SHA, HELPER, DROPIN, BACKUP_SHA, BRIDGE_SHA]:
        checked_sha(value)
    require(BINARY_SHA != PRIOR_BINARY and isinstance(BUILD_SOURCE, str) and BUILD_SOURCE.startswith('/tmp/moloch-'), 'BUILD_NOT_FROZEN')

def checked_counts(checks):
    require(isinstance(checks, dict) and set(checks) == {'library', 'worker', 'release', 'engine-probe-build', 'engine-probe-run'}, 'EXACT_CHECK_GROUPS_REQUIRED')
    for group, row in checks.items():
        require(type(row.get('exitCode')) is int and row['exitCode'] == 0, 'CHECK_EXIT_CODE')
        require(type(row.get('logBytes')) is int and row['logBytes'] >= 0, 'CHECK_LOG_BYTES')
        expected = TEST_LOGS[group] if group in TEST_LOGS else RELEASE_LOG if group == 'release' else PROBE_LOGS[group]
        require(row.get('logSha256') == expected, 'CHECK_LOG_BINDING:' + group)
        if group in EXPECTED_CHECKS:
            require(type(row.get('passedTests')) is int and row['passedTests'] == EXPECTED_CHECKS[group], 'CHECK_COUNT:' + group)

def checked_test_log(group, raw):
    require(group in EXPECTED_CHECKS and hash_bytes(raw) == TEST_LOGS[group], 'ORIGINAL_TEST_LOG_PIN')
    found = re.findall('test result: ok\\. (\\d+) passed; (\\d+) failed; (\\d+) ignored; (\\d+) measured; (\\d+) filtered out;', raw.decode('utf-8'))
    require(found == [(str(EXPECTED_CHECKS[group]), '0', '0', '0', '0')], 'EXACT_ORIGINAL_TEST_GROUP:' + group)

def checked_release_log(raw):
    require(hash_bytes(raw) == RELEASE_LOG, 'ORIGINAL_RELEASE_LOG_PIN')
    text = raw.decode('utf-8')
    require(re.search("Finished [`']release[`'] profile \\[optimized\\]", text) is not None and 'error: could not compile' not in text and ('error: failed to run' not in text), 'RELEASE_FINISH_NOT_PROVED')

def checked_probe_logs(build, run):
    require(hash_bytes(build) == PROBE_LOGS['engine-probe-build'] and hash_bytes(run) == PROBE_LOGS['engine-probe-run'], 'ORIGINAL_PROBE_LOG_PIN')
    require(run.decode('utf-8').strip() == ENGINE, 'ACTUAL_COMPILED_ENGINE_IDENTITY')

def checked_sources(old, new):
    require(set(CHANGES) == ALLOWED_PATHS and len(CHANGES) == 1, 'EXACT_FEEDBACK_ONLY_SOURCE_DELTA')
    require(len(old) == 251 and len(new) == 251 and (set(old) == set(new)), 'EXACT_251_SOURCE_SET_REQUIRED')
    for name, value in new.items():
        require(isinstance(name, str) and (not name.startswith('/')) and ('\\' not in name) and (':' not in name) and ('..' not in name.split('/')), 'SOURCE_PATH_INVALID')
        checked_sha(value)
        require(value == (CHANGES[name] if name in CHANGES else old[name]), 'UNEXPECTED_SOURCE_CHANGE:' + name)
    require(all((old[k] != new[k] for k in CHANGES)), 'TWO_ACTUAL_CHANGES_REQUIRED')
    require(any((k.startswith('moloch/native/') for k in old)) and any((k.startswith('backend-rust/transport-service/') for k in old)), 'NATIVE_SDK_INPUTS_REQUIRED')

def checked_prior(prior, manifest):
    require(prior['newPid'] == OLD_PID and prior['engineIdentity'] == ENGINE == manifest['engineIdentity'], 'PRIOR_PID_OR_ENGINE')
    for prior_key, manifest_key in [('sourceCommit', 'priorSourceCommit'), ('binarySha256', 'priorBinarySha256'), ('release', 'priorReleasePath'), ('control', 'priorControlPath')]:
        require(prior[prior_key] == manifest[manifest_key], 'PRIOR_ROLLOUT_BINDING:' + prior_key)
    checked_sha(prior['binarySha256'])
    require(prior['control'] == manifest['priorControlPath'] == PRIOR_CONTROL_PATH, 'EXACT_PRIOR_CONTROL_PATH')
    require(prior['sourceCommit'] == PRIOR_SOURCE and prior['binarySha256'] == PRIOR_BINARY, 'PRIOR_SOURCE_BINARY_REQUIRED')

def rollout_receipt(s, m, b, expected, new_pid, at, restart_dropin_sha, restart_backup_sha):
    require(type(new_pid) is int and new_pid > 0 and (new_pid != OLD_PID), 'RECEIPT_NEW_PID')
    for value in [expected, restart_dropin_sha, restart_backup_sha]:
        checked_sha(value)
    return {'at': at, 'oldPid': OLD_PID, 'newPid': new_pid, 'sourceCommit': SOURCE, 'binarySha256': s['binarySha256'], 'engineIdentity': ENGINE, 'control': s['control'], 'release': s['release'], 'approvedManifestSha256': expected, 'priorRolloutSha256': m['priorRollout']['sha256'], 'boundary': b, 'testLogSha256s': {k: m['checkLogs'][k]['sha256'] for k in ['library', 'worker']}, 'releaseLogSha256': m['checkLogs']['release']['sha256'], 'checks': m['checks'], 'unchangedBridgeSha256': b['snapshot']['bridgeSha256'], 'bridgeEntries': b['snapshot']['bridgeEntries'], 'restartDropinSha256': restart_dropin_sha, 'restartBackupSha256': restart_backup_sha, 'controlChanges': ['activation.binarySha256'], 'bridgeWritten': False, 'freshTradingVerified': False, 'profitabilityEstablished': False}
ORIGINAL_ORDER_SHA = None
ORIGINAL_ACK_SHA = None
FAILURE_STOP_SHA = None
FAILURE_STATUS_SHA = None
AMBIGUOUS_RECEIPT_SHA = None
RECOVERY_REVIEW_SHA = None
BUILD_SOURCE = None

def failure_witness(w, b):
    require(w == {'operator': OPERATOR, 'executionOwnerReviewed': True, 'oldPid': OLD_PID, 'manifestSha256': b['manifestSha256'], 'failureStopSha256': FAILURE_STOP_SHA, 'failureStatusSha256': FAILURE_STATUS_SHA, 'ambiguousReceiptSha256': AMBIGUOUS_RECEIPT_SHA, 'originalOrderSha256': ORIGINAL_ORDER_SHA, 'originalAckSha256': ORIGINAL_ACK_SHA, 'recoveryReviewSha256': RECOVERY_REVIEW_SHA, 'reason': 'validated-same-timestamp-buy-fill-recovery'}, 'INDEPENDENT_BUY_RECOVERY_WITNESS_REQUIRED')

def checked_failure(stop, status, tape, order, ack, review):
    for data, pin in [(stop, FAILURE_STOP_SHA), (status, FAILURE_STATUS_SHA), (tape, AMBIGUOUS_RECEIPT_SHA), (order, ORIGINAL_ORDER_SHA), (ack, ORIGINAL_ACK_SHA), (review, RECOVERY_REVIEW_SHA)]:
        require(hash_bytes(data) == pin, 'EXACT_ORIGINAL_FAILURE_PINS')
    r = json.loads(review)
    require(r['reason'] == 'validated-same-timestamp-buy-fill-recovery' and r['oldPid'] == OLD_PID and (r['oldSourceCommit'] == PRIOR_SOURCE) and (r['oldBinarySha256'] == PRIOR_BINARY) and (r['failureStopSha256'] == FAILURE_STOP_SHA) and (r['failureStatusSha256'] == FAILURE_STATUS_SHA) and (r['ambiguousReceiptSha256'] == AMBIGUOUS_RECEIPT_SHA) and (r['originalOrderSha256'] == ORIGINAL_ORDER_SHA) and (r['originalAckSha256'] == ORIGINAL_ACK_SHA) and (r['completeTapeSemanticSha256'] == '5a84d4d6c62bf64f3f92cd44a845e844a83d6b4b1f7982a1bd462f9f220bda3c') and (r['knownOriginalBuyFillsVerified'] is True and r['qid'] == 8803 and (str(r['actualOrderId']) == '43530180') and (r['exchangeId'] == '977') and (r['marketId'] == '288') and (r['side'] == 'NO') and (r['filledQuantity'] == 2094) and (r['originalFillRowCount'] == 5) and (r['grossCost'] == '1949.320000')) and (r['fillPricesAndQuantitiesPreserved'] is True) and (r['noManualFinancialRows'] is True) and (r['buyFifoConventionReviewed'] is True) and isinstance(r['buyFifoConvention'], str) and (1 <= len(r['buyFifoConvention']) <= 4096) and (r['nativeSourcesChanged'] is False) and (r['sdkSourcesChanged'] is False), 'EXPLICIT_REVIEWED_BUY_FIFO_RECOVERY_REQUIRED')

def main(argv):
    require(len(argv) == 4 and argv[1] in ('stage', 'promote'), 'USAGE_STAGE_PROMOTE_ONLY')
    freeze_ready()
    require(sys.platform.startswith('linux'), 'LINUX_EXECUTION_ONLY')
    import fcntl
    mode, manifest_path = (argv[1], pathlib.Path(argv[2]))
    expected = checked_sha(argv[3])
    require(manifest_path.is_absolute() and manifest_path.is_file() and (not manifest_path.is_symlink()) and (hash_bytes(manifest_path.read_bytes()) == expected), 'APPROVED_MANIFEST_REQUIRED')
    helper = pathlib.Path('/tmp/moloch-throughput-rollout.py')
    require(hash_bytes(helper.read_bytes()) == HELPER, 'HELPER_HASH_MISMATCH')
    prefix = helper.read_text().split('command=sys.argv[1]')[0]
    require('def verified_dispatch(report):' in prefix and 'elif command' not in prefix, 'HELPER_PREFIX_BOUNDARY')
    env = {}
    exec(compile(prefix, 'pinned-helper-prefix', 'exec'), env)
    root, shared, bank, base = (env[k] for k in ('root', 'shared', 'bank', 'base'))
    load, save, sha, seal, atomic, now, pid, quiet, verified_dispatch = (env[k] for k in ('load', 'save', 'sha', 'seal', 'atomic', 'now', 'pid', 'quiet', 'verified_dispatch'))
    unit = pathlib.Path.home() / '.config/systemd/user/moloch-live.service'
    dropin = unit.parent / 'moloch-live.service.d/10-bounded-restart.conf'
    stagefile = pathlib.Path('/tmp/moloch-buy-fill-recovery-staging.json')
    boundaryfile = pathlib.Path('/tmp/moloch-buy-fill-recovery-boundary.json')
    receiptfile = pathlib.Path('/tmp/moloch-buy-fill-recovery-production-rollout.json')
    witnessfile = pathlib.Path('/tmp/moloch-buy-fill-recovery-approval.json')
    stop_path = root / 'EXECUTION_STOP'
    m = load(manifest_path)

    def artifact(item):
        p = pathlib.Path(item['path'])
        require(p.is_absolute() and p.is_file() and (not p.is_symlink()) and (sha(p) == checked_sha(item['sha256'])), 'ARTIFACT_PIN')
        return p

    def immutable(p, data, executable=False):
        require(not p.is_symlink(), 'IMMUTABLE_SYMLINK')
        if p.exists():
            require(p.read_bytes() == data, 'IMMUTABLE_CONFLICT')
        else:
            atomic(p, data)
        p.chmod(320 if executable else 256)
        return p

    def service_pins():
        require(sha(unit) == PRIOR_UNIT and sha(dropin) == DROPIN and (sha(dropin.parent / BACKUP_NAME) == BACKUP_SHA) and (sorted((p.name for p in dropin.parent.iterdir())) == sorted([dropin.name, BACKUP_NAME])), 'UNIT_OR_DROPIN_CHANGED')

    def queue_quiet():
        q = quiet()
        require(type(q['globalUnknownCount']) is int and q['globalUnknownCount'] == 0 and (q['state']['active'] is None) and (q['state']['manual_review'] is False), 'QUEUE_NOT_QUIET')
        return q

    def held():
        require(pid() == 0 and (not pathlib.Path('/proc', str(OLD_PID)).exists()) and (not (shared / 'EXECUTION_STOP').exists()), 'FAILED_OWNER_NOT_ABSENT_OR_GLOBAL_STOP')
        f = open(shared / 'capture.lock', 'r+')
        try:
            fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BaseException:
            f.close()
            raise
        return f

    def failure():
        require(stop_path.is_file() and (not stop_path.is_symlink()), 'ORIGINAL_FAILURE_STOP_REQUIRED')
        checked_failure(stop_path.read_bytes(), (root / 'status.json').read_bytes(), artifact(m['ambiguousReceipt']).read_bytes(), artifact(m['originalOrder']).read_bytes(), artifact(m['originalAck']).read_bytes(), artifact(m['recoveryReview']).read_bytes())
        require(m['ambiguousReceipt']['sha256'] == AMBIGUOUS_RECEIPT_SHA and m['recoveryReview']['sha256'] == RECOVERY_REVIEW_SHA and (m['originalOrder']['sha256'] == ORIGINAL_ORDER_SHA) and (m['originalAck']['sha256'] == ORIGINAL_ACK_SHA), 'FAILURE_MANIFEST_BINDING')

    def snapshot():
        h, r, mf = (seal(bank / 'head.json'), seal(bank / 'report.json'), seal(bank / 'manifest.json'))
        require(h['sequence'] == r['sequence'] and h['previous'] == r['previous'] and (h['manifestHash'] == r['manifestHash'] == mf['hash'] == m['bankManifestHash']), 'BANK_SEAL_MISMATCH')
        bridge = seal(bank / 'engine-upgrade-v1.json')
        require(len(bridge['upgrades']) == 12 and bridge['upgrades'][-1]['toEngineIdentity'] == ENGINE and (sha(bank / 'engine-upgrade-v1.json') == BRIDGE_SHA == prior['unchangedBridgeSha256']) and (bridge['upgrades'] == prior['bridgeEntries']), 'EXACT_12_BRIDGES_REQUIRED')
        return {'head': h, 'reportSha256': sha(bank / 'report.json'), 'manifestSha256': sha(bank / 'manifest.json'), 'prefixBytes': (bank / 'events.jsonl').stat().st_size, 'prefixSha256': sha(bank / 'events.jsonl'), 'bridgeSha256': sha(bank / 'engine-upgrade-v1.json'), 'bridgeEntries': bridge['upgrades'], 'dispatch': verified_dispatch(r), 'dispatchSha256': sha(root / 'dispatch.ndjson'), 'fundamentalSha256': sha(root / 'fundamental.ndjson'), 'fundamentalMarkerSha256': sha(root / 'fundamental.initialized.json'), 'packageDispatchSha256': sha(root / 'package-dispatch/package-dispatch.ndjson')}
    prior = load(artifact(m['priorRollout']))
    require(m['priorRollout']['path'] == PRIOR_RECEIPT_PATH and m['priorRollout']['sha256'] == PRIOR_RECEIPT and (m['priorControlSha256'] == PRIOR_CONTROL) and (m['priorUnitSha256'] == PRIOR_UNIT), 'ORIGINAL_PRIOR_ARTIFACT_PINS')
    checked_prior(prior, m)
    require(m['sourceCommit'] == SOURCE and m['engineIdentity'] == ENGINE, 'CANDIDATE_SOURCE_ENGINE')
    if mode == 'stage':
        require(not stagefile.exists() and (not boundaryfile.exists()) and (not receiptfile.exists()), 'ALREADY_STAGED')
        require(m['baseSourceManifest']['path'] == BASE_MANIFEST_PATH and m['baseSourceManifest']['sha256'] == BASE_MANIFEST and (m['sourceManifest']['sha256'] == SOURCE_MANIFEST_SHA) and (m['validationEvidence']['sha256'] == VALIDATION_SHA) and (m['binary']['sha256'] == BINARY_SHA), 'FROZEN_BUILD_ARTIFACTS')
        old = load(artifact(m['baseSourceManifest']))
        new = load(artifact(m['sourceManifest']))
        v = load(artifact(m['validationEvidence']))
        require(new['sourceHashes'] == m['sourceHashes'] and new['sourceCommit'] == v['sourceCommit'] == SOURCE, 'SOURCE_MANIFEST_BINDING')
        checked_sources(old['sourceHashes'], m['sourceHashes'])
        checked_counts(m['checks'])
        require(v['sourceManifestSha256'] == SOURCE_MANIFEST_SHA and v['checks'] == m['checks'] and (v['binarySha256'] == BINARY_SHA) and (v['engineIdentity'] == ENGINE) and (v['sourceFiles'] == 251) and (v['nativeSourcesChanged'] is False) and (v['sdkSourcesChanged'] is False) and (v['bankMigration'] is False) and (set(v['changedPaths']) == ALLOWED_PATHS) and (new['nativeSourcesChanged'] is False) and (new['sdkSourcesChanged'] is False) and (new['bankMigration'] is False) and (set(new['allowedChanged']) == ALLOWED_PATHS), 'BUILD_VALIDATION_BINDING')
        logs = m['checkLogs']
        require(set(logs) == set(m['checks']), 'CHECK_LOG_SET')
        for group in ['library', 'worker']:
            checked_test_log(group, artifact(logs[group]).read_bytes())
        checked_release_log(artifact(logs['release']).read_bytes())
        checked_probe_logs(artifact(logs['engine-probe-build']).read_bytes(), artifact(logs['engine-probe-run']).read_bytes())
        require(m['buildSource'] == BUILD_SOURCE, 'BUILD_PATH')
        for name, value in m['sourceHashes'].items():
            require(sha(pathlib.Path(BUILD_SOURCE) / name) == value and (not (pathlib.Path(BUILD_SOURCE) / name).is_symlink()), 'SOURCE_BYTES_CHANGED')
        binary = artifact(m['binary'])
        require(binary.stat().st_size == v['binaryBytes'], 'BINARY_SIZE')
        with held():
            service_pins()
            failure()
            snap = snapshot()
            queue_quiet()
            require(snapshot() == snap, 'STOPPED_BOUNDARY_CHANGED')
            require(sha(prior['release']) == PRIOR_BINARY and sha(prior['control']) == PRIOR_CONTROL, 'PRIOR_IMMUTABLE_ARTIFACTS')
            control = copy.deepcopy(load(prior['control']))
            control['activation']['binarySha256'] = BINARY_SHA
            controls(load(prior['control']), control, BINARY_SHA)
            release = pathlib.Path(m['releasePath'])
            require(release.is_absolute() and base / 'releases' in release.parents and (not release.parent.is_symlink()), 'RELEASE_PATH')
            release.parent.mkdir(mode=448, parents=True, exist_ok=True)
            immutable(release, binary.read_bytes(), True)
            cp = shared / 'moloch-live-buy-fill-recovery-20261016.json'
            require(not cp.exists(), 'CONTROL_EXISTS')
            save(cp, control)
            oldexec = 'ExecStart=' + prior['release'] + ' run ' + prior['control']
            text = unit.read_text()
            require(text.count(oldexec) == 1, 'EXACT_UNIT_EXEC')
            candidate = immutable(release.parent / 'moloch-live.service', text.replace(oldexec, 'ExecStart=' + str(release) + ' run ' + str(cp)).encode())
            subprocess.run(['systemd-analyze', '--user', 'verify', str(candidate)], check=True)
            for name, path in [('recovery-old-unit.service', unit), ('recovery-old-control.json', pathlib.Path(prior['control'])), ('recovery-original-failure-stop.json', stop_path), ('recovery-original-failure-status.json', root / 'status.json'), ('recovery-original-ambiguous-receipt.json', artifact(m['ambiguousReceipt'])), ('recovery-original-order.json', artifact(m['originalOrder'])), ('recovery-original-ack.json', artifact(m['originalAck'])), ('recovery-owner-review.json', artifact(m['recoveryReview']))]:
                immutable(release.parent / name, path.read_bytes())
            require(snapshot() == snap, 'BOUNDARY_CHANGED_DURING_STAGE')
            failure()
            queue_quiet()
            require(snapshot() == snap, 'BOUNDARY_CHANGED_AFTER_QUIET')
            save(stagefile, {'manifestSha256': expected, 'binarySha256': BINARY_SHA, 'engineIdentity': ENGINE, 'release': str(release), 'control': str(cp), 'controlSha256': sha(cp), 'unitCandidate': str(candidate), 'unitCandidateSha256': sha(candidate), 'priorControl': prior['control'], 'priorRelease': prior['release']})
            save(boundaryfile, {'manifestSha256': expected, 'snapshot': snap, 'failureStopSha256': FAILURE_STOP_SHA, 'failureStatusSha256': FAILURE_STATUS_SHA})
        print('RECOVERY_STAGED_ONLY; ORIGINAL_FAILURE_STOP_PRESERVED')
        return
    s, b = (load(stagefile), load(boundaryfile))
    require(s['manifestSha256'] == b['manifestSha256'] == expected and (not receiptfile.exists()), 'STAGING_OR_RECEIPT_CHANGED')
    with held():
        service_pins()
        failure()
        failure_witness(load(witnessfile), b)
        queue_quiet()
        require(snapshot() == b['snapshot'], 'STOPPED_BOUNDARY_CHANGED')
        require(sha(s['release']) == BINARY_SHA == s['binarySha256'] and sha(s['control']) == s['controlSha256'] and (sha(s['unitCandidate']) == s['unitCandidateSha256']) and (sha(s['priorControl']) == PRIOR_CONTROL) and (sha(s['priorRelease']) == PRIOR_BINARY), 'STAGED_OR_PRIOR_ARTIFACT_CHANGED')
        controls(load(s['priorControl']), load(s['control']), BINARY_SHA)
        require(snapshot() == b['snapshot'], 'BOUNDARY_CHANGED_AFTER_REVIEW')
        backup = unit.with_name(unit.name + '.before-buy-fill-recovery-20261016')
        require(not backup.exists(), 'UNIT_BACKUP_EXISTS')
        immutable(backup, unit.read_bytes())
        atomic(unit, pathlib.Path(s['unitCandidate']).read_bytes())
        subprocess.run(['systemctl', '--user', 'daemon-reload'], check=True)
        require(sha(unit) == s['unitCandidateSha256'] and sha(dropin) == DROPIN, 'INSTALLED_UNIT_CHANGED')
        failure()
        failure_witness(load(witnessfile), b)
        queue_quiet()
        require(snapshot() == b['snapshot'] and (not (shared / 'EXECUTION_STOP').exists()), 'FINAL_BOUNDARY_OR_STOP_CHANGED')
        archive = root / ('EXECUTION_STOP.validated-buy-recovery-' + FAILURE_STOP_SHA + '.json')
        require(not archive.exists(), 'FAILURE_ARCHIVE_EXISTS')
        try:
            os.rename(stop_path, archive)
            require(sha(archive) == FAILURE_STOP_SHA, 'ORIGINAL_FAILURE_ARCHIVE_CHANGED')
        except BaseException:
            if not stop_path.exists():
                save(stop_path, {'operator': OPERATOR, 'reason': 'RECOVERY_FAILURE_ARCHIVE_FAILED_NO_AUTORETRY', 'at': now()})
            raise
    try:
        require(pid() == 0 and (not pathlib.Path('/proc', str(OLD_PID)).exists()) and (not stop_path.exists()) and (not (shared / 'EXECUTION_STOP').exists()), 'START_OWNER_OR_STOP_CHANGED')
        subprocess.run(['systemctl', '--user', 'reset-failed', 'moloch-live.service'], check=True)
        subprocess.run(['systemctl', '--user', 'start', 'moloch-live.service'], check=True)
        newpid = pid()
        require(newpid > 0 and newpid != OLD_PID and (sha(pathlib.Path('/proc', str(newpid), 'exe')) == BINARY_SHA), 'NEW_BINARY_NOT_VERIFIED')
    except BaseException:
        if not stop_path.exists():
            save(stop_path, {'operator': OPERATOR, 'reason': 'RECOVERY_START_FAILED_NO_AUTORETRY', 'at': now()})
        raise
    receipt = rollout_receipt(s, m, b, expected, newpid, now(), sha(dropin), sha(dropin.parent / BACKUP_NAME))
    receipt['recoveryReason'] = 'validated-same-timestamp-buy-fill-recovery'
    receipt['failureStopSha256'] = FAILURE_STOP_SHA
    receipt['failureStatusSha256'] = FAILURE_STATUS_SHA
    receipt['ambiguousReceiptSha256'] = AMBIGUOUS_RECEIPT_SHA
    receipt['originalOrderSha256'] = ORIGINAL_ORDER_SHA
    receipt['originalAckSha256'] = ORIGINAL_ACK_SHA
    receipt['recoveryReviewSha256'] = RECOVERY_REVIEW_SHA
    save(receiptfile, receipt)
    print('RECOVERY_BINARY_STARTED; FRESH_TRADING_PENDING', newpid)
if __name__ == '__main__':
    main(sys.argv)
