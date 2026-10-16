#!/usr/bin/env python3
"""REVIEW-ONLY RECOVERY DRAFT (stage/promote only; never controlled-stop): no mode executed. Prior Control and unit pins were supplied by the execution owner from the original freeze file; deployment remains root-only. Import has no operations; same-engine, binary plus exact NC review settings Control change."""
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
SOURCE = 'ec8b0fef24ec6b43960784c05d40b7862a7643e3'
BASE_MANIFEST = '307501836bee4eced8e290edfb0fc0fcc7a913437a01bf4cf783ea6a0ba4797f'
BASE_MANIFEST_PATH = '/tmp/moloch-defensive-cancel-20261016/source-manifest.json'
HELPER = '1e0da4b462dd0627dc597b725fc592530d6ae3c91babe09d4ecb641d2ce87133'
DROPIN = 'ff4237fb5e0b5f2a68159e4fa43d737eefc68ebdd422f56eea068c4ffba0e795'
BACKUP_NAME = '10-bounded-restart.conf.before-exec-condition-20261016'
BACKUP_SHA = 'ddbe320f3fcd657d719a2ec28f67282ce2eb0c90ff8bf03eff9308871121936e'
PRIOR_SOURCE = 'b112c6c1b941bcc3282c754c1883a55b2b9b6cc2'
PRIOR_BINARY = '3ab6ef7c49c089f19ff90220cd3431ff5ce1c84a5858d6d9fdde060f1b9d6883'
PRIOR_RECEIPT_PATH = '/tmp/moloch-paper6-defensive-cancel-production-rollout.json'
PRIOR_CONTROL_PATH = '/home/archon/apps/moloch/shared/moloch-live-paper6-defensive-cancel-20261016.json'
PRIOR_RECEIPT = '993626d8fe902579f31b9e17bb1851889cd7151c4d3784e79405e316850b815c'
PRIOR_CONTROL = 'f8938aa3823b7ad50487d17aa2c224f4ac39c4c635906123acc07d3d9ffb643c'
PRIOR_UNIT = 'eb64ad9a2199e8a7cbcb7088f57020dd736f90fdce674fb920375ef7f28511d8'
BRIDGE_SHA = '76fab91acccfe26cfb1a80010a7b1fb00df7f92f02dc20d5127192258bcf0723'
CHANGES = {'moloch/live-runtime/src/feedback.rs': '7da4ff94ad5a21543213c24a09971271d2d7a4ef91a9104a0aa03a86dc016aef', 'moloch/live-runtime/src/ordinary_side_edge.rs': 'f2c2a496632e6b52c0d9959af1b5310002c2320c7d1de6a27de374863296596e', 'moloch/live-runtime/src/worker.rs': 'ab692458e5c4cfb55c476a304a7f90d5397f2e50a6b4b674568b38a8e8218634'}
ALLOWED_PATHS = {'moloch/live-runtime/src/ordinary_side_edge.rs', 'moloch/live-runtime/src/worker.rs', 'moloch/live-runtime/src/feedback.rs'}
ADDED_PATHS = set()
EXPECTED_CHECKS = {'library': 208, 'worker': 97}
TEST_LOGS = {'library': '83a598e6bd3a679afa01b8aa4ff9a657f6d19a9818fe3ee6ba8db063af603eb4', 'worker': 'a5dc97572704d5e5c16a86aa7a3981accbf0ce9db16637a4b564b8ace5834486'}
RELEASE_LOG = '50e620c1fa9de2a5ca393efc950949817bd0792cea7bad918143e9ead50491e5'
PROBE_LOGS = {'engine-probe-build': 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855', 'engine-probe-run': '3c5f3feed2383794b2ee7cfed5114c0f351eb200e7f149fcb0cdd2a9e14228fc'}
BINARY_SHA = '40fbaa4731be72c735b6bfc6bff86423cd833e57e20ad80640935d585be8a8d6'
SOURCE_MANIFEST_SHA = '09308fe3a64695f9d38882aea03ef01803f8b0e2fb6e1f107e9d137f69ee6096'
VALIDATION_SHA = 'b2e2b78b54413a25c91689f3f37181919d457ecc38766193c3cb088a1f4f7741'
OPERATOR = 'paper6_early_entry_recovery_rollout_v1'
OLD_PID = 525450
OLD_SETTINGS_CANONICAL_SHA = 'da4968178fc3c24933e9a27bd99d0106b97b3a613537eb62a1c791b58e41425c'
NEW_SETTINGS_CANONICAL_SHA = '5b34be069f440eca0dac92d1238a6eeae42313f2019eccc646895374e6150064'
NEW_NC_REVIEW_RAW_SHA = 'b339c77da9e98bdbd54fe898f06afe85f3105bb6baf18ed5e0cee737c38f3e84'
NEW_NC_REVIEW_RUST_SHA = 'c7732708725fee6cab370f41c2b6170559464ef55f60ea354487c389cbf72fd1'
NEW_AUTHORITY_RAW_SHA = 'd39148823ea24dd54dfe95493bf8e7cf1380ceae83837d7346c3575abc0e4ace'
FUNDAMENTAL_VALIDATION_SHA = '0f1e309e56b881822a360f6ecf75aa87c826a33b605950bb2a0a6f1d6ee9396b'
OLD_SETTINGS = {'version': 1, 'authorityPath': '/home/archon/apps/moloch/shared/fundamental-paper5-terminal-20261016/authority.json', 'authoritySha256': '5c0ebe0d07dc35eec979e89f08383d360cc93b500605c63784c7cd1e7cf1bf07', 'reviews': [{'path': '/home/archon/apps/moloch/shared/fundamental-paper3-actions-20261016/review.json', 'fileSha256': '7c590da5833c7637107cc2fbe2b817cea41dfdfe3c8ebb6de6c24bc1e32a9e89', 'sourceFiles': [{'path': '/home/archon/apps/moloch/shared/fundamental-delaware-democratic-20261016/call-the-map-delaware.html', 'sha256': 'f45d33e0c3f55884fbbe32f608d87566cb5b786d57e928f146dbb8dc7dda5a54'}, {'path': '/home/archon/apps/moloch/shared/fundamental-delaware-democratic-20261016/delaware-general-candidate-list.html', 'sha256': '18f8523e0c9317bc88cd09c464164b09ae858c66466a94e4163af65bb1ef82d3'}, {'path': '/home/archon/apps/moloch/shared/fundamental-delaware-democratic-20261016/official-contract-node.json', 'sha256': '1ffb57719716cb23cfe3538cc0eff223c7c9028845c03d9e3bd63211755259f5'}]}, {'path': '/home/archon/apps/moloch/shared/fundamental-massachusetts-inherited-20261016/review.json', 'fileSha256': '1abebbb714e8929740ed4d43ab37317bb6b8fb9542d1b70782a10d15a5568e45', 'sourceFiles': [{'path': '/home/archon/apps/moloch/shared/fundamental-massachusetts-inherited-20261016/call-the-map-ma-senate.html', 'sha256': 'a416e1a3d023756150adef500d8545fbcd68e45a3f4b91f764a78f47cd20c46f'}, {'path': '/home/archon/apps/moloch/shared/fundamental-massachusetts-inherited-20261016/ma-official-senate-primary-detail.html', 'sha256': '696fbe73451f182cb0032eaf29b7f353ae9ef3d7444eaf75e062713c4a6910c4'}, {'path': '/home/archon/apps/moloch/shared/fundamental-massachusetts-inherited-20261016/official-contract-node-379.json', 'sha256': 'd49709f0b2a4f64a7c0e067cb7639a270faf153b7e89e2c66bcc72b75621b230'}]}, {'path': '/home/archon/apps/moloch/shared/fundamental-north-carolina-terminal-20261016/review-v2-terminal-candidate.json', 'fileSha256': '33408bf8ff412d9577d119bd0a760781d713123c2a391ad1448177feeb6e6344', 'sourceFiles': [{'path': '/home/archon/apps/moloch/shared/fundamental-north-carolina-terminal-20261016/callthemap-nc.html', 'sha256': '3a3ea329d19e640a64646a636e7669196911c500faf70621976a5312d80ab8ad'}, {'path': '/home/archon/apps/moloch/shared/fundamental-north-carolina-terminal-20261016/official-general-candidates.pdf', 'sha256': '378ca3dde9e97f65a921c922171aa34942bcc68582ef46e713b371e0b6caf11d'}, {'path': '/home/archon/apps/moloch/shared/fundamental-north-carolina-terminal-20261016/official-contract-node-277.original.json', 'sha256': '58a98acc30aa5c67510de0b505550df49d3dd58892a141c17eb8b51b05b9434e'}]}], 'allowInitialization': True}
OLD_NC_REVIEW = {'version': 2, 'reviewerId': 'Rhaegal-Viserion-paper5-20261016', 'assumptionLabel': 'Execution-owner small terminal-value cohort: CTMM2.2 original Oct15 NorthCarolina Cooper probability0.919, not a calibrated lower bound; official Oct13 general candidate list identifies Cooper DEM and original SIG Party Winner root. Use0.03 operator exceptional/party-mapping allowance with zero exceptional payout and0.01 margin; these are explicit sensitivity assumptions, not guaranteed probabilities. 99chambers94/97 shares polls and is not an independent likelihood multiplier. COYNTOSS98.22 excluded due visible/embedded contradiction. Scheduled Nov4 settlement is not a guarantee of completion; V2 explicitly holds for actual settlement if delayed. Source expiry09UTC blocks new entries; no existing V1 cohort deadline is extended.', 'runId': 'moloch-live-20261015-v1', 'bankManifestSha256': '78acaef952ed95b9bff24a3349ece669ccbe88ef7294b27dd9c2aa7d09cae325', 'tournamentId': 'bda92870-621e-47b0-bc3c-3602c5c26f55', 'marketId': '277', 'exchangeId': '966', 'contractId': '278', 'officialContractRootSha256': '56670f0ec4ec7e9837de07ca07cc7573996a89e94d578f9db675252613936da1', 'side': 'YES', 'ordinaryProbability': '0.919000', 'exceptionalProbabilityAllowance': '0.030000', 'minimumExceptionalPayout': '0.000000', 'marginPerShare': '0.010000', 'maximumPrincipal': '400.000000', 'maximumQuantity': 10000, 'reviewedAtMs': 1792126942423, 'expiresAtMs': 1792141200000, 'holdUntilMs': 1793811600000, 'payoutMappingIsOperatorAssumption': True, 'revokeExisting': False, 'sources': [{'kind': 'forecast', 'url': 'https://www.callthemap.com/senate/north-carolina', 'sha256': '3a3ea329d19e640a64646a636e7669196911c500faf70621976a5312d80ab8ad', 'publishedAtMs': 1792087428000, 'observedAtMs': 1792126544863, 'maximumAgeMs': 86400000}, {'kind': 'official_ballot', 'url': 'https://s3.amazonaws.com/dl.ncsbe.gov/Elections/2026/Candidate%20Filing/2026_general_candidate_list_by_contest_federal_and_state.pdf', 'sha256': '378ca3dde9e97f65a921c922171aa34942bcc68582ef46e713b371e0b6caf11d', 'publishedAtMs': 1791910059000, 'observedAtMs': 1792126516391, 'maximumAgeMs': 86400000}, {'kind': 'official_contract_node', 'url': 'https://sig.thesuper.market/api/v1/markets/277/nodes?tournamentId=bda92870-621e-47b0-bc3c-3602c5c26f55', 'sha256': '58a98acc30aa5c67510de0b505550df49d3dd58892a141c17eb8b51b05b9434e', 'publishedAtMs': 1792107921269, 'observedAtMs': 1792107921269, 'maximumAgeMs': 86400000}], 'executionPolicy': {'version': 1, 'allowedModes': ['boundedLimitMayRest', 'improveOneTick', 'joinBid']}, 'valuationObjective': {'kind': 'terminalSettlement', 'timingSourceSha256': '58a98acc30aa5c67510de0b505550df49d3dd58892a141c17eb8b51b05b9434e', 'timingJsonPointer': '/data/root/settlement_date', 'scheduledSettlementAtMs': 1793811600000, 'delayPolicy': 'holdForActualSettlement'}}
OLD_AUTHORITY = {'version': 1, 'trustedReviewSha256': ['3e3a8c1676dc9248edf3c754d8c1eeb40be99bface73aaea2f9ba5577074f292', '46088147b39c7a6f563661ba0912b4024522e9a59c4d44aca43a7e908dd18c0c', '9523497c40e24eb5c6b2251cb59d1c839b874d15fbdcccee4c8c9ea5b254cef6', '86f8b72e6bd5b640cdf96579a32378a4ceb1367a07a19641a1b3f95be9e032ed']}

def require(ok, code):
    if not ok:
        raise RuntimeError(code)

def hash_bytes(data):
    return hashlib.sha256(data).hexdigest()

def checked_sha(value):
    require(isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value), 'SHA256_REQUIRED')
    return value

def canonical_settings_sha(value):
    return hash_bytes(json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode('utf-8'))

def settings_delta(before, after):
    require(canonical_settings_sha(before) == OLD_SETTINGS_CANONICAL_SHA and before == OLD_SETTINGS, 'EXACT_ORIGINAL_SETTINGS_REQUIRED')
    require(canonical_settings_sha(after) == checked_sha(NEW_SETTINGS_CANONICAL_SHA), 'APPROVED_NEW_SETTINGS_REQUIRED')
    require(set(after) == set(before) and len(after['reviews']) == 4 and (after['version'] == before['version']) and (after['allowInitialization'] == before['allowInitialization']), 'SETTINGS_STRUCTURE_CHANGED')
    require(after['reviews'][:2] == before['reviews'][:2], 'DE_MA_SETTINGS_MUST_STAY_IDENTICAL')
    require(after['authorityPath'] != before['authorityPath'] and after['authoritySha256'] == checked_sha(NEW_AUTHORITY_RAW_SHA), 'NEW_IMMUTABLE_AUTHORITY_REQUIRED')
    require(after['reviews'][2]['path'] != before['reviews'][2]['path'] and after['reviews'][2]['fileSha256'] == checked_sha(NEW_NC_REVIEW_RAW_SHA), 'NEW_IMMUTABLE_NC_REVIEW_REQUIRED')
    require(after['reviews'][3]['fileSha256'] == checked_sha(NEW_OR_REVIEW_RAW_SHA), 'APPENDED_OR_REVIEW_REQUIRED')

def review_delta(old, new, authority):
    require(old == OLD_NC_REVIEW, 'EXACT_OLD_NC_REVIEW')
    mutable = {'reviewerId', 'assumptionLabel', 'maximumPrincipal', 'reviewedAtMs', 'expiresAtMs', 'sources'}
    require(new['reviewerId'] == 'Rhaegal-Viserion-paper6-20261016', 'EXACT_CURRENT_REVIEWER_REQUIRED')
    require(set(new) == set(old) and all((new[k] == v for k, v in old.items() if k not in mutable)), 'NC_SCOPE_VALUE_HORIZON_OR_MODES_CHANGED')
    require(new['maximumPrincipal'] == '2000.000000' and new['reviewedAtMs'] > old['reviewedAtMs'] and (new['expiresAtMs'] > new['reviewedAtMs']), 'EXPLICIT_NC_REVIEW_REFRESH_REQUIRED')
    oldsources = {x['kind']: x for x in old['sources']}
    newsources = {x['kind']: x for x in new['sources']}
    require(set(newsources) == set(oldsources) and len(new['sources']) == len(oldsources), 'SOURCE_KIND_CHANGED')
    require(newsources['official_contract_node'] == oldsources['official_contract_node'], 'ORIGINAL_NODE_CLOCK_AND_BYTES_MUST_STAY')
    node = oldsources['official_contract_node']
    require(new['expiresAtMs'] <= node['observedAtMs'] + node['maximumAgeMs'], 'NODE_LIFETIME_CANNOT_BE_RENEWED')
    for kind in ['forecast', 'official_ballot']:
        row = newsources[kind]
        require(row['url'] == oldsources[kind]['url'] and row['publishedAtMs'] <= row['observedAtMs'] <= new['reviewedAtMs'] and (row['observedAtMs'] >= oldsources[kind]['observedAtMs']) and (row['maximumAgeMs'] == oldsources[kind]['maximumAgeMs']), 'REFRESH_ORIGINAL_SOURCE_CLOCKS')
        checked_sha(row['sha256'])
    require(authority['version'] == OLD_AUTHORITY['version'] and set(authority) == set(OLD_AUTHORITY) and (authority['trustedReviewSha256'] == OLD_AUTHORITY['trustedReviewSha256'] + [checked_sha(NEW_NC_REVIEW_RUST_SHA), checked_sha(NEW_OR_REVIEW_RUST_SHA)]), 'APPEND_ONLY_REVIEW_AUTHORITY_REQUIRED')

def controls(before, after, binary_sha):
    require(before.get('ordinarySideEdge') == 'enforce' and before.get('defensiveCancel') is True, 'PRESERVE_ENFORCE_AND_DEFENSE_REQUIRED')
    settings_delta(before['fundamentalPolicy'], after['fundamentalPolicy'])
    expected = copy.deepcopy(before)
    expected['activation']['binarySha256'] = checked_sha(binary_sha)
    expected['fundamentalPolicy'] = copy.deepcopy(after['fundamentalPolicy'])
    require(after == expected, 'ONLY_BINARY_AND_NC_SETTINGS_CHANGE_ALLOWED')

def freeze_ready():
    require(isinstance(SOURCE, str) and re.fullmatch('[0-9a-f]{40}', SOURCE) and (SOURCE != PRIOR_SOURCE), 'SOURCE_NOT_FROZEN')
    require(isinstance(CHANGES, dict) and set(CHANGES) == ALLOWED_PATHS and (len(CHANGES) == 3), 'EXACT_THREE_SOURCE_CHANGES_REQUIRED')
    require(isinstance(EXPECTED_CHECKS, dict) and set(EXPECTED_CHECKS) == {'library', 'worker'} and all((type(v) is int and v > 0 for v in EXPECTED_CHECKS.values())), 'ACTUAL_TEST_COUNTS_REQUIRED')
    require(isinstance(TEST_LOGS, dict) and set(TEST_LOGS) == set(EXPECTED_CHECKS) and isinstance(PROBE_LOGS, dict) and (set(PROBE_LOGS) == {'engine-probe-build', 'engine-probe-run'}), 'ACTUAL_CHECK_LOGS_REQUIRED')
    for value in list(CHANGES.values()) + list(TEST_LOGS.values()) + list(PROBE_LOGS.values()) + [BINARY_SHA, SOURCE_MANIFEST_SHA, VALIDATION_SHA, RELEASE_LOG, PRIOR_CONTROL, PRIOR_UNIT, FAILURE_STOP_SHA, FAILURE_STATUS_SHA, AMBIGUOUS_RECEIPT_SHA, RECOVERY_REVIEW_SHA, NEW_SETTINGS_CANONICAL_SHA, NEW_NC_REVIEW_RAW_SHA, NEW_NC_REVIEW_RUST_SHA, NEW_OR_REVIEW_RAW_SHA, NEW_OR_REVIEW_RUST_SHA, NEW_AUTHORITY_RAW_SHA, FUNDAMENTAL_VALIDATION_SHA]:
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
    require(set(CHANGES) == ALLOWED_PATHS and len(CHANGES) == 3, 'EXACT_THREE_SOURCE_PINS')
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
    return {'at': at, 'oldPid': OLD_PID, 'newPid': new_pid, 'sourceCommit': SOURCE, 'binarySha256': s['binarySha256'], 'engineIdentity': ENGINE, 'control': s['control'], 'release': s['release'], 'approvedManifestSha256': expected, 'priorRolloutSha256': m['priorRollout']['sha256'], 'boundary': b, 'testLogSha256s': {k: m['checkLogs'][k]['sha256'] for k in ['library', 'worker']}, 'releaseLogSha256': m['checkLogs']['release']['sha256'], 'checks': m['checks'], 'unchangedBridgeSha256': b['snapshot']['bridgeSha256'], 'bridgeEntries': b['snapshot']['bridgeEntries'], 'restartDropinSha256': restart_dropin_sha, 'restartBackupSha256': restart_backup_sha, 'controlChanges': ['activation.binarySha256', 'fundamentalPolicy'], 'bridgeWritten': False, 'fundamentalSettingsCanonicalSha256': NEW_SETTINGS_CANONICAL_SHA, 'ncReviewCanonicalSha256': NEW_NC_REVIEW_RUST_SHA, 'fundamentalValidationSha256': FUNDAMENTAL_VALIDATION_SHA, 'freshTradingVerified': False, 'profitabilityEstablished': False}
FAILURE_STOP_SHA = '99890e11cf6a6632fb247f1dba5c481c981e9db985dd4aa6838206c896f5d163'
FAILURE_STATUS_SHA = 'c06132ec53eb4ae04d2930c6a4d5c1cae124d93fa65d139c7c4feb11b3e49384'
AMBIGUOUS_RECEIPT_SHA = '72b88e2f6e0c9c7f0efcfbc09e5366a9f7436d4bfe86873e2a86386c2782f44e'
RECOVERY_REVIEW_SHA = 'f112e5167622854077fba6230befdbf59fdb9a593ab1a7162fdd93916364aa2c'
NEW_OR_REVIEW_RAW_SHA = 'f4371abd456ecf0a920f98391d6e014a6f9a641c93986da079b41d17d62b956e'
NEW_OR_REVIEW_RUST_SHA = 'fa933826010a22a23b338725c5e016fc224f2bd8d27890eb6463b202015951ec'
BUILD_SOURCE = '/tmp/moloch-sell-timestamp-recovery-20261016'

def failure_witness(w, b):
    require(w == {'operator': OPERATOR, 'executionOwnerReviewed': True, 'oldPid': OLD_PID, 'manifestSha256': b['manifestSha256'], 'failureStopSha256': FAILURE_STOP_SHA, 'failureStatusSha256': FAILURE_STATUS_SHA, 'ambiguousReceiptSha256': AMBIGUOUS_RECEIPT_SHA, 'recoveryReviewSha256': RECOVERY_REVIEW_SHA, 'reason': 'validated-same-timestamp-sell-fill-recovery'}, 'INDEPENDENT_FAILURE_RECOVERY_WITNESS_REQUIRED')

def checked_failure(stop, status, receipt, review):
    require(hash_bytes(stop) == FAILURE_STOP_SHA and hash_bytes(status) == FAILURE_STATUS_SHA and (hash_bytes(receipt) == AMBIGUOUS_RECEIPT_SHA) and (hash_bytes(review) == RECOVERY_REVIEW_SHA), 'EXACT_ORIGINAL_FAILURE_PINS')
    r = json.loads(review)
    require(r['reason'] == 'validated-same-timestamp-sell-fill-recovery' and r['oldPid'] == OLD_PID and (r['oldSourceCommit'] == PRIOR_SOURCE) and (r['oldBinarySha256'] == PRIOR_BINARY) and (r['failureStopSha256'] == FAILURE_STOP_SHA) and (r['failureStatusSha256'] == FAILURE_STATUS_SHA) and (r['ambiguousReceiptSha256'] == AMBIGUOUS_RECEIPT_SHA) and (r['knownOriginalSellFillsVerified'] is True) and (r['buyAmbiguityStillRejected'] is True) and (r['nativeSourcesChanged'] is False) and (r['sdkSourcesChanged'] is False), 'RECOVERY_REVIEW_FACTS_REQUIRED')

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
    stagefile = pathlib.Path('/tmp/moloch-early-entry-recovery-staging.json')
    boundaryfile = pathlib.Path('/tmp/moloch-early-entry-recovery-boundary.json')
    receiptfile = pathlib.Path('/tmp/moloch-early-entry-recovery-production-rollout.json')
    witnessfile = pathlib.Path('/tmp/moloch-early-entry-recovery-approval.json')
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
        checked_failure(stop_path.read_bytes(), (root / 'status.json').read_bytes(), artifact(m['ambiguousReceipt']).read_bytes(), artifact(m['recoveryReview']).read_bytes())
        require(m['ambiguousReceipt']['sha256'] == AMBIGUOUS_RECEIPT_SHA and m['recoveryReview']['sha256'] == RECOVERY_REVIEW_SHA, 'FAILURE_MANIFEST_BINDING')

    def snapshot():
        h, r, mf = (seal(bank / 'head.json'), seal(bank / 'report.json'), seal(bank / 'manifest.json'))
        require(h['sequence'] == r['sequence'] and h['previous'] == r['previous'] and (h['manifestHash'] == r['manifestHash'] == mf['hash'] == m['bankManifestHash']), 'BANK_SEAL_MISMATCH')
        bridge = seal(bank / 'engine-upgrade-v1.json')
        require(len(bridge['upgrades']) == 12 and bridge['upgrades'][-1]['toEngineIdentity'] == ENGINE and (sha(bank / 'engine-upgrade-v1.json') == BRIDGE_SHA == prior['unchangedBridgeSha256']) and (bridge['upgrades'] == prior['bridgeEntries']), 'EXACT_12_BRIDGES_REQUIRED')
        return {'head': h, 'reportSha256': sha(bank / 'report.json'), 'manifestSha256': sha(bank / 'manifest.json'), 'prefixBytes': (bank / 'events.jsonl').stat().st_size, 'prefixSha256': sha(bank / 'events.jsonl'), 'bridgeSha256': sha(bank / 'engine-upgrade-v1.json'), 'bridgeEntries': bridge['upgrades'], 'dispatch': verified_dispatch(r), 'dispatchSha256': sha(root / 'dispatch.ndjson'), 'fundamentalSha256': sha(root / 'fundamental.ndjson'), 'fundamentalMarkerSha256': sha(root / 'fundamental.initialized.json'), 'packageDispatchSha256': sha(root / 'package-dispatch/package-dispatch.ndjson')}

    def refresh_artifacts():
        f = m['fundamentalRefresh']
        settings = load(artifact(f['settings']))
        settings_delta(OLD_SETTINGS, settings)
        ncp = artifact(f['review'])
        orp = artifact(f['orReview'])
        ap = artifact(f['authority'])
        vp = artifact(f['validation'])
        require(f['review']['sha256'] == NEW_NC_REVIEW_RAW_SHA and f['orReview']['sha256'] == NEW_OR_REVIEW_RAW_SHA and (f['authority']['sha256'] == NEW_AUTHORITY_RAW_SHA) and (f['validation']['sha256'] == FUNDAMENTAL_VALIDATION_SHA), 'FROZEN_FUNDAMENTAL_ARTIFACTS')
        require(str(ncp) == settings['reviews'][2]['path'] and str(orp) == settings['reviews'][3]['path'] and (str(ap) == settings['authorityPath']), 'EXACT_REFRESH_PATHS')
        nc, oregon, auth = (load(ncp), load(orp), load(ap))
        review_delta(OLD_NC_REVIEW, nc, auth)
        require(oregon['version'] == 2 and oregon['marketId'] == '288' and (oregon['exchangeId'] == '977') and (oregon['contractId'] == '289') and (oregon['side'] == 'NO') and (oregon['ordinaryProbability'] == '0.995000') and (oregon['exceptionalProbabilityAllowance'] == '0.030000') and (oregon['minimumExceptionalPayout'] == '0.000000') and (oregon['marginPerShare'] == '0.010000') and (oregon['maximumPrincipal'] == '2000.000000') and (oregon['valuationObjective']['kind'] == 'terminalSettlement') and (oregon['valuationObjective']['delayPolicy'] == 'holdForActualSettlement'), 'EXACT_APPROVED_OR_ECONOMICS')
        validation = load(vp)
        require(validation['sourcesVerified'] is True and validation['reviewAccepted'] is True and (validation['reviewCanonicalSha256'] == NEW_NC_REVIEW_RUST_SHA) and (validation['orReviewCanonicalSha256'] == NEW_OR_REVIEW_RUST_SHA) and (validation['settingsCanonicalSha256'] == NEW_SETTINGS_CANONICAL_SHA), 'GENUINE_RUST_REVIEW_VALIDATION_REQUIRED')
        for item, r in [(settings['reviews'][2], nc), (settings['reviews'][3], oregon)]:
            require(len(item['sourceFiles']) == len(r['sources']) and {x['sha256'] for x in item['sourceFiles']} == {x['sha256'] for x in r['sources']}, 'SOURCE_FILE_COVERAGE')
            for source in item['sourceFiles']:
                artifact(source)
        return settings
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
            settings = refresh_artifacts()
            control = copy.deepcopy(load(prior['control']))
            control['activation']['binarySha256'] = BINARY_SHA
            control['fundamentalPolicy'] = settings
            controls(load(prior['control']), control, BINARY_SHA)
            release = pathlib.Path(m['releasePath'])
            require(release.is_absolute() and base / 'releases' in release.parents and (not release.parent.is_symlink()), 'RELEASE_PATH')
            release.parent.mkdir(mode=448, parents=True, exist_ok=True)
            immutable(release, binary.read_bytes(), True)
            cp = shared / 'moloch-live-early-entry-recovery-20261016.json'
            require(not cp.exists(), 'CONTROL_EXISTS')
            save(cp, control)
            oldexec = 'ExecStart=' + prior['release'] + ' run ' + prior['control']
            text = unit.read_text()
            require(text.count(oldexec) == 1, 'EXACT_UNIT_EXEC')
            candidate = immutable(release.parent / 'moloch-live.service', text.replace(oldexec, 'ExecStart=' + str(release) + ' run ' + str(cp)).encode())
            subprocess.run(['systemd-analyze', '--user', 'verify', str(candidate)], check=True)
            for name, path in [('recovery-old-unit.service', unit), ('recovery-old-control.json', pathlib.Path(prior['control'])), ('recovery-original-failure-stop.json', stop_path), ('recovery-original-failure-status.json', root / 'status.json'), ('recovery-original-ambiguous-receipt.json', artifact(m['ambiguousReceipt'])), ('recovery-owner-review.json', artifact(m['recoveryReview']))]:
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
        require(load(s['control'])['fundamentalPolicy'] == refresh_artifacts(), 'REVIEW_ARTIFACT_CHANGED')
        require(snapshot() == b['snapshot'], 'BOUNDARY_CHANGED_AFTER_REVIEW')
        backup = unit.with_name(unit.name + '.before-early-entry-recovery-20261016')
        require(not backup.exists(), 'UNIT_BACKUP_EXISTS')
        immutable(backup, unit.read_bytes())
        atomic(unit, pathlib.Path(s['unitCandidate']).read_bytes())
        subprocess.run(['systemctl', '--user', 'daemon-reload'], check=True)
        require(sha(unit) == s['unitCandidateSha256'] and sha(dropin) == DROPIN, 'INSTALLED_UNIT_CHANGED')
        failure()
        failure_witness(load(witnessfile), b)
        queue_quiet()
        require(snapshot() == b['snapshot'] and (not (shared / 'EXECUTION_STOP').exists()), 'FINAL_BOUNDARY_OR_STOP_CHANGED')
        archive = root / ('EXECUTION_STOP.validated-sell-recovery-' + FAILURE_STOP_SHA + '.json')
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
    receipt['recoveryReason'] = 'validated-same-timestamp-sell-fill-recovery'
    receipt['failureStopSha256'] = FAILURE_STOP_SHA
    receipt['failureStatusSha256'] = FAILURE_STATUS_SHA
    receipt['ambiguousReceiptSha256'] = AMBIGUOUS_RECEIPT_SHA
    receipt['recoveryReviewSha256'] = RECOVERY_REVIEW_SHA
    receipt['orReviewCanonicalSha256'] = NEW_OR_REVIEW_RUST_SHA
    save(receiptfile, receipt)
    print('RECOVERY_BINARY_STARTED; FRESH_TRADING_PENDING', newpid)
if __name__ == '__main__':
    main(sys.argv)
