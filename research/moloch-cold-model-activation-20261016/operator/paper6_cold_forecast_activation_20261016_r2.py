#!/usr/bin/env python3
"""PREPARED ONLY: execution owner must freeze/review and invoke on Linux.

No import side effects. No HTTP, SDK client, financial migration or binary change.
Stage creates only a new immutable shared model generation/control/unit candidate.
Stop requires actual Rust Settings::load output for the STAGED paths and digest.
Promote additionally requires separately authored exact owner approval/stop witness.

CLI (none invoked by the author):
  stage MANIFEST MANIFEST_RAW_SHA
  stop MANIFEST MANIFEST_RAW_SHA STAGED_TYPED_SETTINGS RAW_SHA
  promote MANIFEST MANIFEST_RAW_SHA ROOT_APPROVAL RAW_SHA
Use manifest_template() offline, freeze its bytes, then supply its raw hash.
Root runs the existing /tmp/validate_recovery_forecasts against staged settings;
retain its original output (settingsLoadPassed/reviewAccepted/sourcesVerified,
settingsCanonicalSha256) as STAGED_TYPED_SETTINGS. Root approval is described by
approval_template(stage_raw_sha, boundary_raw_sha, typed_path, typed_raw_sha,
stop_witness); it deliberately remains unauthored until those originals exist.
A successful start reports restoring; it does not claim fresh trading or profit.
"""
import copy
import hashlib
import json
import os
import pathlib
import re
import stat
import subprocess
import sys
import time
import uuid

OPERATOR = 'paper6_cold_forecast_activation_20261016_v1'
ENGINE = '8acc461bc02fb9236100058fe464b203d5da005375632af05c176223c7246754'
SOURCE = '774f94fe6938f55ed447d078965cc583810cb3e7'
BINARY = '1180882d0a47f1af948258c37b333fb51552c31ef9823a51e1c058cf8558e95f'
OLD_PID = 540259
OLD_RELEASE = '/home/archon/apps/moloch/releases/paper6-buy-recovery-1180882d0a47/moloch-live-runtime'
OLD_CONTROL = '/home/archon/apps/moloch/shared/moloch-live-buy-fill-recovery-20261016.json'
OLD_CONTROL_SHA = 'bbbfa51b98d783b8eba313291ac66cf4e48e2811ac9e321e2bfb0b87b206bcb8'
OLD_UNIT_SHA = '091797ff9ce24ba6633bfc42db111eaf4a1344452fa1ce81505f68614e87146a'
OLD_SUMMARY = '/tmp/moloch-buy-recovery-production-summary-20261016.json'
OLD_SUMMARY_SHA = '9e5d9ff0241f4bfdeabff3b6c12b38c226ee6abeb78d984f3ef0a0efb8739bc2'
HELPER = '1e0da4b462dd0627dc597b725fc592530d6ae3c91babe09d4ecb641d2ce87133'
DROPIN = 'ff4237fb5e0b5f2a68159e4fa43d737eefc68ebdd422f56eea068c4ffba0e795'
BACKUP_NAME = '10-bounded-restart.conf.before-exec-condition-20261016'
BACKUP_SHA = 'ddbe320f3fcd657d719a2ec28f67282ce2eb0c90ff8bf03eff9308871121936e'
BRIDGE_SHA = '76fab91acccfe26cfb1a80010a7b1fb00df7f92f02dc20d5127192258bcf0723'
BANK_MANIFEST = '78acaef952ed95b9bff24a3349ece669ccbe88ef7294b27dd9c2aa7d09cae325'
GENERATION = '/home/archon/apps/moloch/shared/fundamental-paper6-cold-six-forecast-20261016'
NEW_CONTROL = '/home/archon/apps/moloch/shared/moloch-live-paper6-cold-six-forecast-20261016.json'
NEW_SETTINGS_SHA = 'e7714333a9467b6bf69144905f1b74f1143d85e2c682a7a236d723eb936f37f8'
OLD_SETTINGS_SHA = '5b34be069f440eca0dac92d1238a6eeae42313f2019eccc646895374e6150064'
PACKET_PROOF_SHA = '506012dda5ae42a6f7f9eb49e777a32600c40da8b2e5200459554efa3dd0a051'
REVIEW_CANONICAL = ['7c1e2bc9d038363a8597cb36a9fa0241ca35ea97091859eb42c046e5515b190d',
 'a7263405f19eb354c488c99c09ccadda887439ebc8545b5a54abccef1293da37']
OLD_SETTINGS = {'allowInitialization': True,
 'authorityPath': '/home/archon/apps/moloch/shared/fundamental-paper6-sell-recovery-20261016/authority.json',
 'authoritySha256': 'd39148823ea24dd54dfe95493bf8e7cf1380ceae83837d7346c3575abc0e4ace',
 'reviews': [{'fileSha256': '7c590da5833c7637107cc2fbe2b817cea41dfdfe3c8ebb6de6c24bc1e32a9e89',
              'path': '/home/archon/apps/moloch/shared/fundamental-paper3-actions-20261016/review.json',
              'sourceFiles': [{'path': '/home/archon/apps/moloch/shared/fundamental-delaware-democratic-20261016/call-the-map-delaware.html',
                               'sha256': 'f45d33e0c3f55884fbbe32f608d87566cb5b786d57e928f146dbb8dc7dda5a54'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-delaware-democratic-20261016/delaware-general-candidate-list.html',
                               'sha256': '18f8523e0c9317bc88cd09c464164b09ae858c66466a94e4163af65bb1ef82d3'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-delaware-democratic-20261016/official-contract-node.json',
                               'sha256': '1ffb57719716cb23cfe3538cc0eff223c7c9028845c03d9e3bd63211755259f5'}]},
             {'fileSha256': '1abebbb714e8929740ed4d43ab37317bb6b8fb9542d1b70782a10d15a5568e45',
              'path': '/home/archon/apps/moloch/shared/fundamental-massachusetts-inherited-20261016/review.json',
              'sourceFiles': [{'path': '/home/archon/apps/moloch/shared/fundamental-massachusetts-inherited-20261016/call-the-map-ma-senate.html',
                               'sha256': 'a416e1a3d023756150adef500d8545fbcd68e45a3f4b91f764a78f47cd20c46f'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-massachusetts-inherited-20261016/ma-official-senate-primary-detail.html',
                               'sha256': '696fbe73451f182cb0032eaf29b7f353ae9ef3d7444eaf75e062713c4a6910c4'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-massachusetts-inherited-20261016/official-contract-node-379.json',
                               'sha256': 'd49709f0b2a4f64a7c0e067cb7639a270faf153b7e89e2c66bcc72b75621b230'}]},
             {'fileSha256': 'b339c77da9e98bdbd54fe898f06afe85f3105bb6baf18ed5e0cee737c38f3e84',
              'path': '/home/archon/apps/moloch/shared/fundamental-paper6-sell-recovery-20261016/nc-review.json',
              'sourceFiles': [{'path': '/home/archon/apps/moloch/shared/fundamental-paper6-sell-recovery-20261016/nc-callthemap.html',
                               'sha256': 'fbe3194c1c938057f36bffb6d6ab27910175affcecdddf4fef6112342a171de2'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-paper6-sell-recovery-20261016/nc-ncsbe-general-candidates.pdf',
                               'sha256': '378ca3dde9e97f65a921c922171aa34942bcc68582ef46e713b371e0b6caf11d'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-paper6-sell-recovery-20261016/nc-official-contract-node-277.original.json',
                               'sha256': '58a98acc30aa5c67510de0b505550df49d3dd58892a141c17eb8b51b05b9434e'}]},
             {'fileSha256': 'f4371abd456ecf0a920f98391d6e014a6f9a641c93986da079b41d17d62b956e',
              'path': '/home/archon/apps/moloch/shared/fundamental-paper6-sell-recovery-20261016/or-review.json',
              'sourceFiles': [{'path': '/home/archon/apps/moloch/shared/fundamental-paper6-sell-recovery-20261016/or-forecast.html',
                               'sha256': '7e6f07c53525903c7ae12ef2fb6e344028539d2f3ee8d8c498f17c07125563d1'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-paper6-sell-recovery-20261016/or-official-candidates.html',
                               'sha256': '82753be8eb22a0741fe402c0cb841b37f187cad0fef50c65e3ddd6c8bb4a9f64'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-paper6-sell-recovery-20261016/or-official-contract-node.json',
                               'sha256': '9bca4722d2a06e32e0d86360b43c6aae6e4336876dec40a5a50b4cb70f9568f1'}]}],
 'version': 1}
NEW_SETTINGS = {'allowInitialization': True,
 'authorityPath': '/home/archon/apps/moloch/shared/fundamental-paper6-cold-six-forecast-20261016/authority.json',
 'authoritySha256': '2956246e3d5ce0300f690978738aea420e00f61f16f76c5537de8debf45d6351',
 'reviews': [{'fileSha256': '7c590da5833c7637107cc2fbe2b817cea41dfdfe3c8ebb6de6c24bc1e32a9e89',
              'path': '/home/archon/apps/moloch/shared/fundamental-paper3-actions-20261016/review.json',
              'sourceFiles': [{'path': '/home/archon/apps/moloch/shared/fundamental-delaware-democratic-20261016/call-the-map-delaware.html',
                               'sha256': 'f45d33e0c3f55884fbbe32f608d87566cb5b786d57e928f146dbb8dc7dda5a54'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-delaware-democratic-20261016/delaware-general-candidate-list.html',
                               'sha256': '18f8523e0c9317bc88cd09c464164b09ae858c66466a94e4163af65bb1ef82d3'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-delaware-democratic-20261016/official-contract-node.json',
                               'sha256': '1ffb57719716cb23cfe3538cc0eff223c7c9028845c03d9e3bd63211755259f5'}]},
             {'fileSha256': '1abebbb714e8929740ed4d43ab37317bb6b8fb9542d1b70782a10d15a5568e45',
              'path': '/home/archon/apps/moloch/shared/fundamental-massachusetts-inherited-20261016/review.json',
              'sourceFiles': [{'path': '/home/archon/apps/moloch/shared/fundamental-massachusetts-inherited-20261016/call-the-map-ma-senate.html',
                               'sha256': 'a416e1a3d023756150adef500d8545fbcd68e45a3f4b91f764a78f47cd20c46f'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-massachusetts-inherited-20261016/ma-official-senate-primary-detail.html',
                               'sha256': '696fbe73451f182cb0032eaf29b7f353ae9ef3d7444eaf75e062713c4a6910c4'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-massachusetts-inherited-20261016/official-contract-node-379.json',
                               'sha256': 'd49709f0b2a4f64a7c0e067cb7639a270faf153b7e89e2c66bcc72b75621b230'}]},
             {'fileSha256': 'b339c77da9e98bdbd54fe898f06afe85f3105bb6baf18ed5e0cee737c38f3e84',
              'path': '/home/archon/apps/moloch/shared/fundamental-paper6-sell-recovery-20261016/nc-review.json',
              'sourceFiles': [{'path': '/home/archon/apps/moloch/shared/fundamental-paper6-sell-recovery-20261016/nc-callthemap.html',
                               'sha256': 'fbe3194c1c938057f36bffb6d6ab27910175affcecdddf4fef6112342a171de2'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-paper6-sell-recovery-20261016/nc-ncsbe-general-candidates.pdf',
                               'sha256': '378ca3dde9e97f65a921c922171aa34942bcc68582ef46e713b371e0b6caf11d'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-paper6-sell-recovery-20261016/nc-official-contract-node-277.original.json',
                               'sha256': '58a98acc30aa5c67510de0b505550df49d3dd58892a141c17eb8b51b05b9434e'}]},
             {'fileSha256': 'f4371abd456ecf0a920f98391d6e014a6f9a641c93986da079b41d17d62b956e',
              'path': '/home/archon/apps/moloch/shared/fundamental-paper6-sell-recovery-20261016/or-review.json',
              'sourceFiles': [{'path': '/home/archon/apps/moloch/shared/fundamental-paper6-sell-recovery-20261016/or-forecast.html',
                               'sha256': '7e6f07c53525903c7ae12ef2fb6e344028539d2f3ee8d8c498f17c07125563d1'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-paper6-sell-recovery-20261016/or-official-candidates.html',
                               'sha256': '82753be8eb22a0741fe402c0cb841b37f187cad0fef50c65e3ddd6c8bb4a9f64'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-paper6-sell-recovery-20261016/or-official-contract-node.json',
                               'sha256': '9bca4722d2a06e32e0d86360b43c6aae6e4336876dec40a5a50b4cb70f9568f1'}]},
             {'fileSha256': 'ae008cd18567b7912c24fe53be1b06fe9bf5d30e994bbf7f6de77e90c6baf661',
              'path': '/home/archon/apps/moloch/shared/fundamental-paper6-cold-six-forecast-20261016/alaska-review.json',
              'sourceFiles': [{'path': '/home/archon/apps/moloch/shared/fundamental-paper6-cold-six-forecast-20261016/sources/alaska/ak-senate.html',
                               'sha256': '955c9d1d819ffc6e1194aa0027d378dfb5d6eeabd9dd04345433ffe0dcd7aaf8'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-paper6-cold-six-forecast-20261016/sources/alaska/ak-official-candidates.html',
                               'sha256': 'a5676ae3baa845a4cb618fe97fa7551d786103df0f7c5a02bc55898450dd26f2'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-paper6-cold-six-forecast-20261016/sources/alaska/official-contract-node-378.original.json',
                               'sha256': '38ad364b1d91ae1825a38e3e8c135ba49d2a79554fd46210e835772ef40b19c0'}]},
             {'fileSha256': '2cb3c4ca88a638bafa95d3756752df92dc72cf6de40228d36f4a34cc0ffdac97',
              'path': '/home/archon/apps/moloch/shared/fundamental-paper6-cold-six-forecast-20261016/maine-review.json',
              'sourceFiles': [{'path': '/home/archon/apps/moloch/shared/fundamental-paper6-cold-six-forecast-20261016/sources/maine/forecast.html',
                               'sha256': '0b6f1861631f02cc66d07587008a9164bdb97a9d4223a283c083f7c6c9602ce5'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-paper6-cold-six-forecast-20261016/sources/maine/official-candidates.xlsx',
                               'sha256': 'c628e48fa6298b6862e709166adf3145f7bc4a3f60929c3e6ef5a3ea324e03ea'},
                              {'path': '/home/archon/apps/moloch/shared/fundamental-paper6-cold-six-forecast-20261016/sources/maine/official-contract-node.json',
                               'sha256': 'fc46cb639d50b0714af5bc8af7e48271ee6e98ae4565f55d830a6171272cf499'}]}],
 'version': 1}
NEW_AUTHORITY = {'trustedReviewSha256': ['3e3a8c1676dc9248edf3c754d8c1eeb40be99bface73aaea2f9ba5577074f292',
                         '46088147b39c7a6f563661ba0912b4024522e9a59c4d44aca43a7e908dd18c0c',
                         '9523497c40e24eb5c6b2251cb59d1c839b874d15fbdcccee4c8c9ea5b254cef6',
                         '86f8b72e6bd5b640cdf96579a32378a4ceb1367a07a19641a1b3f95be9e032ed',
                         'c7732708725fee6cab370f41c2b6170559464ef55f60ea354487c389cbf72fd1',
                         'fa933826010a22a23b338725c5e016fc224f2bd8d27890eb6463b202015951ec',
                         '7c1e2bc9d038363a8597cb36a9fa0241ca35ea97091859eb42c046e5515b190d',
                         'a7263405f19eb354c488c99c09ccadda887439ebc8545b5a54abccef1293da37'],
 'version': 1}
NEW_REVIEWS = [{'assumptionLabel': 'Execution-owner reviewed Alaska model candidate, INACTIVE until separate activation: '
                     'primary Call the Map published incumbent Dan S. Sullivan win chance0.507 includes '
                     'ranked-choice transfers, not first-choice vote share. Only that registered Republican '
                     'incumbent victory is valued as an ordinary subset of exact SIG Republican Party Winner '
                     'YES. Other Republican candidates are not added: official ballot lists Daniel J. '
                     'Sullivan Jr Republican while CTM marks him O, an explicit '
                     'classification/transfer-model uncertainty. Scrutinel70% and99chambers44%-46% disagree '
                     'and are not averaged or independent calibration. The0.507 point is an operator-model '
                     'assumption, not a lower confidence bound. Exceptional allowance0.03 is unchanged '
                     'proposed operator stress, not empirical exception frequency; minimum exceptional '
                     'payout0 and margin0.01. Party affiliation/replacement/refund/resolver/finality and '
                     'national-correlation/model risks remain. The execution owner explicitly approves48h '
                     'historical-node reference TTL, original Oct15 receipt clock unchanged, current '
                     'physical exactroot verification mandatory. Official ballot publication unknown uses '
                     'documented publishedAtMs0 sentinel. Hold for actual settlement if delayed, source '
                     'expiry blocks entry not forced sale. Principal2000 is an inactive candidate comparison '
                     'budget; no prize-EV or whole-account-convexity claim. Root source/risk review accepts '
                     'these stated model assumptions and2,000 principal ceiling only; no current executable '
                     'price, calibration, profit guarantee or physical activation is established. Original '
                     'source clocks are unchanged.',
  'bankManifestSha256': '78acaef952ed95b9bff24a3349ece669ccbe88ef7294b27dd9c2aa7d09cae325',
  'contractId': '379',
  'exceptionalProbabilityAllowance': '0.030000',
  'exchangeId': '1067',
  'executionPolicy': {'allowedModes': ['boundedLimitMayRest', 'improveOneTick', 'joinBid'], 'version': 1},
  'expiresAtMs': 1792263776239,
  'holdUntilMs': 1793811600000,
  'marginPerShare': '0.010000',
  'marketId': '378',
  'maximumPrincipal': '2000.000000',
  'maximumQuantity': 10000,
  'minimumExceptionalPayout': '0.000000',
  'officialContractRootSha256': '8fe67c16b16544d7f71c3c955e40adaaa0baaa6b19e7ac29d6aafe931ae84c8a',
  'ordinaryProbability': '0.507000',
  'payoutMappingIsOperatorAssumption': True,
  'reviewedAtMs': 1792180126962,
  'reviewerId': 'Rhaegal-Viserion-paper6-20261016',
  'revokeExisting': False,
  'runId': 'moloch-live-20261015-v1',
  'side': 'YES',
  'sources': [{'kind': 'forecast',
               'maximumAgeMs': 86400000,
               'observedAtMs': 1792178407889,
               'publishedAtMs': 1792173446000,
               'sha256': '955c9d1d819ffc6e1194aa0027d378dfb5d6eeabd9dd04345433ffe0dcd7aaf8',
               'url': 'https://www.callthemap.com/senate/alaska'},
              {'kind': 'official_ballot',
               'maximumAgeMs': 86400000,
               'observedAtMs': 1792178515989,
               'publishedAtMs': 0,
               'sha256': 'a5676ae3baa845a4cb618fe97fa7551d786103df0f7c5a02bc55898450dd26f2',
               'url': 'https://www.elections.alaska.gov/candidates/?election=26genr'},
              {'kind': 'official_contract_node',
               'maximumAgeMs': 172800000,
               'observedAtMs': 1792090976239,
               'publishedAtMs': 1792090976239,
               'sha256': '38ad364b1d91ae1825a38e3e8c135ba49d2a79554fd46210e835772ef40b19c0',
               'url': 'https://sig.thesuper.market/api/v1/markets/378/nodes?tournamentId=bda92870-621e-47b0-bc3c-3602c5c26f55'}],
  'tournamentId': 'bda92870-621e-47b0-bc3c-3602c5c26f55',
  'valuationObjective': {'delayPolicy': 'holdForActualSettlement',
                         'kind': 'terminalSettlement',
                         'scheduledSettlementAtMs': 1793811600000,
                         'timingJsonPointer': '/data/root/settlement_date',
                         'timingSourceSha256': '38ad364b1d91ae1825a38e3e8c135ba49d2a79554fd46210e835772ef40b19c0'},
  'version': 2},
 {'assumptionLabel': 'Inactive Maine candidate proposed for execution-owner review, not activation: Call the '
                     'Map current published Pingree win probability0.954 is a model point assumption, not a '
                     'calibrated lower bound. Its candidate-field forecast explicitly includes independent '
                     'Rick Bennett; a two-party-only probability is not substituted. Official current '
                     'SOS-linked General Election FINAL candidate file lists GOV Hannah M. Pingree '
                     'Democratic, Robert B. Charles Republican, Rick Bennett Independent. Maine general '
                     'gubernatorial election uses plurality, not RCV. Ordinary Pingree victory is mapped to '
                     'Democratic Party Winner YES under exact structured SIG contract; '
                     'affiliation/replacement/refund/resolver exceptions remain operator assumptions. '
                     'Exceptional allowance0.03 is unchanged operator stress, not estimated frequency; '
                     'minimum exceptional payout0, margin0.01. Models share inputs and are not '
                     'averaged/multiplied, Kalshi is not fair value. Hold for actual settlement if delayed; '
                     'source expiry blocks entry not forced liquidation. Official file publication clock '
                     'unknown uses publishedAtMs0 sentinel, while original HTTP/file-modification clocks are '
                     'preserved separately. Principal2000 is a research proposal pending root '
                     'financial/typed-source review and fresh physical facts. Root source/risk review '
                     'accepts these stated model assumptions and2,000 principal ceiling only; no current '
                     'executable price, calibration, profit guarantee or physical activation is established. '
                     'Original source clocks are unchanged.',
  'bankManifestSha256': '78acaef952ed95b9bff24a3349ece669ccbe88ef7294b27dd9c2aa7d09cae325',
  'contractId': '177',
  'exceptionalProbabilityAllowance': '0.030000',
  'exchangeId': '865',
  'executionPolicy': {'allowedModes': ['boundedLimitMayRest', 'improveOneTick', 'joinBid'], 'version': 1},
  'expiresAtMs': 1792249441431,
  'holdUntilMs': 1793811600000,
  'marginPerShare': '0.010000',
  'marketId': '176',
  'maximumPrincipal': '2000.000000',
  'maximumQuantity': 10000,
  'minimumExceptionalPayout': '0.000000',
  'officialContractRootSha256': '5e768c0f5224b76620be03b0d08a9eb433c300d329491b1cd058ea47b42801e5',
  'ordinaryProbability': '0.954000',
  'payoutMappingIsOperatorAssumption': True,
  'reviewedAtMs': 1792180126962,
  'reviewerId': 'Rhaegal-Viserion-paper6-20261016',
  'revokeExisting': False,
  'runId': 'moloch-live-20261015-v1',
  'side': 'YES',
  'sources': [{'kind': 'forecast',
               'maximumAgeMs': 86400000,
               'observedAtMs': 1792177317654,
               'publishedAtMs': 1792173446000,
               'sha256': '0b6f1861631f02cc66d07587008a9164bdb97a9d4223a283c083f7c6c9602ce5',
               'url': 'https://www.callthemap.com/governors/maine'},
              {'kind': 'official_ballot',
               'maximumAgeMs': 86400000,
               'observedAtMs': 1792177380555,
               'publishedAtMs': 0,
               'sha256': 'c628e48fa6298b6862e709166adf3145f7bc4a3f60929c3e6ef5a3ea324e03ea',
               'url': 'https://www1.maine.gov/sos/sites/maine.gov.sos/files/inline-files/2026%20General%20Candidate%20List%20-%20FINAL.xlsx'},
              {'kind': 'official_contract_node',
               'maximumAgeMs': 86400000,
               'observedAtMs': 1792163041431,
               'publishedAtMs': 1792163041431,
               'sha256': 'fc46cb639d50b0714af5bc8af7e48271ee6e98ae4565f55d830a6171272cf499',
               'url': 'https://sig.thesuper.market/api/v1/markets/176/nodes?tournamentId=bda92870-621e-47b0-bc3c-3602c5c26f55'}],
  'tournamentId': 'bda92870-621e-47b0-bc3c-3602c5c26f55',
  'valuationObjective': {'delayPolicy': 'holdForActualSettlement',
                         'kind': 'terminalSettlement',
                         'scheduledSettlementAtMs': 1793811600000,
                         'timingJsonPointer': '/data/root/settlement_date',
                         'timingSourceSha256': 'fc46cb639d50b0714af5bc8af7e48271ee6e98ae4565f55d830a6171272cf499'},
  'version': 2}]
PAYLOAD_HASHES = {'alaska-review.json': 'ae008cd18567b7912c24fe53be1b06fe9bf5d30e994bbf7f6de77e90c6baf661',
 'authority.json': '2956246e3d5ce0300f690978738aea420e00f61f16f76c5537de8debf45d6351',
 'maine-review.json': '2cb3c4ca88a638bafa95d3756752df92dc72cf6de40228d36f4a34cc0ffdac97',
 'sources/alaska/ak-official-candidates.html': 'a5676ae3baa845a4cb618fe97fa7551d786103df0f7c5a02bc55898450dd26f2',
 'sources/alaska/ak-senate.html': '955c9d1d819ffc6e1194aa0027d378dfb5d6eeabd9dd04345433ffe0dcd7aaf8',
 'sources/alaska/official-contract-node-378.original.json': '38ad364b1d91ae1825a38e3e8c135ba49d2a79554fd46210e835772ef40b19c0',
 'sources/maine/forecast.html': '0b6f1861631f02cc66d07587008a9164bdb97a9d4223a283c083f7c6c9602ce5',
 'sources/maine/official-candidates.xlsx': 'c628e48fa6298b6862e709166adf3145f7bc4a3f60929c3e6ef5a3ea324e03ea',
 'sources/maine/official-contract-node.json': 'fc46cb639d50b0714af5bc8af7e48271ee6e98ae4565f55d830a6171272cf499'}
ORIGINAL_PROOF_HASHES = {'owner-model-review.json': 'c9d72ec8a68548436246308d2bcdcf29d7960b3661ea4db6db74708f71cc83a8',
 'typed-reviews.json': 'a8c50baa7ab907b59acb3c0b7075a3cc90fb16e28ee6e9e08f0b423f5ff1d937',
 'typed-settings.json': 'e976a43ff769817bef04829861a41e55069c46e120a9177072756ad7e24c251c'}


def require(ok, code):
    if not ok:
        raise RuntimeError(code)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode('utf-8')


def sha_value(value):
    return digest(encoded(value))


def checked_sha(value):
    require(isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value), 'SHA256_REQUIRED')
    return value


def no_symlink_path(path):
    p = pathlib.Path(path)
    require(p.is_absolute(), 'ABSOLUTE_PATH_REQUIRED')
    require(not any(x.is_symlink() for x in (p, *p.parents)), 'SYMLINK_PATH_REFUSED')
    return p


def read_pinned(path, expected, maximum=4*1024*1024):
    p = no_symlink_path(path)
    require(p.is_file(), 'REGULAR_BOUNDED_ARTIFACT_REQUIRED')
    with p.open('rb') as f:
        before = os.fstat(f.fileno())
        require(stat.S_ISREG(before.st_mode) and before.st_size <= maximum, 'REGULAR_BOUNDED_ARTIFACT_REQUIRED')
        data = f.read(maximum+1)
        after = os.fstat(f.fileno())
        current = p.stat()
    require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) ==
            (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns) ==
            (current.st_dev, current.st_ino, current.st_size, current.st_mtime_ns), 'ARTIFACT_CHANGED_DURING_READ')
    require(digest(data) == checked_sha(expected), 'ARTIFACT_HASH_MISMATCH')
    return data


def manifest_template():
    return {'schemaVersion': 1, 'operator': OPERATOR,
            'operatorSha256': digest(pathlib.Path(__file__).read_bytes()),
            'sourceCommit': SOURCE, 'binarySha256': BINARY, 'engineIdentity': ENGINE,
            'oldPid': OLD_PID, 'oldControlSha256': OLD_CONTROL_SHA, 'oldUnitSha256': OLD_UNIT_SHA,
            'priorRolloutSummary': {'path': OLD_SUMMARY, 'sha256': OLD_SUMMARY_SHA},
            'bankManifestHash': BANK_MANIFEST,
            'packetDirectory': '/tmp/moloch-cold-approved-candidates-20261016',
            'packetProof': {'path': '/tmp/moloch-cold-candidate-typed-proof-20261016.zip', 'sha256': PACKET_PROOF_SHA},
            'generation': GENERATION, 'plannedSettingsCanonicalSha256': NEW_SETTINGS_SHA}


def validate_manifest(value):
    for key in ('operatorSha256', 'binarySha256', 'engineIdentity', 'oldControlSha256', 'oldUnitSha256', 'bankManifestHash', 'plannedSettingsCanonicalSha256'):
        checked_sha(value.get(key))
    checked_sha(value.get('priorRolloutSummary', {}).get('sha256'))
    checked_sha(value.get('packetProof', {}).get('sha256'))
    require(value == manifest_template(), 'EXACT_ROOT_FROZEN_MANIFEST_REQUIRED')


def settings_delta(before, after):
    require(before == OLD_SETTINGS and sha_value(before) == OLD_SETTINGS_SHA, 'EXACT_CURRENT_FOUR_SETTINGS_REQUIRED')
    require(after == NEW_SETTINGS and sha_value(after) == NEW_SETTINGS_SHA, 'EXACT_STAGED_SIX_SETTINGS_REQUIRED')
    require(after['reviews'][:4] == before['reviews'], 'EXISTING_FOUR_ENTRIES_CHANGED')
    require(after['version'] == before['version'] and after['allowInitialization'] == before['allowInitialization'], 'SETTINGS_MODE_CHANGED')


def controls(before, after):
    settings_delta(before['fundamentalPolicy'], after['fundamentalPolicy'])
    expected = copy.deepcopy(before)
    expected['fundamentalPolicy'] = copy.deepcopy(NEW_SETTINGS)
    require(after == expected, 'ONLY_FUNDAMENTAL_SETTINGS_MAY_CHANGE')
    require(before['activation']['binarySha256'] == BINARY, 'ORIGINAL_BINARY_REQUIRED')


def source_lifetimes(reviews, at_ms):
    require(reviews == NEW_REVIEWS and type(at_ms) is int, 'EXACT_ORIGINAL_REVIEWS_REQUIRED')
    for r in reviews:
        require(r['reviewedAtMs'] <= at_ms < r['expiresAtMs'], 'NEW_REVIEW_EXPIRED_OR_FUTURE')
        require(r['valuationObjective']['delayPolicy'] == 'holdForActualSettlement', 'SETTLEMENT_HOLD_REQUIRED')
        for s in r['sources']:
            require(type(s['maximumAgeMs']) is int and s['maximumAgeMs'] > 0 and
                    0 <= s['publishedAtMs'] <= s['observedAtMs'] <= r['reviewedAtMs'] and
                    at_ms < s['observedAtMs'] + s['maximumAgeMs'], 'ORIGINAL_SOURCE_EXPIRED_OR_FUTURE')


def typed_settings_proof(value):
    require(value.get('settingsLoadPassed') is True and value.get('reviewAccepted') is True and
            value.get('sourcesVerified') is True and value.get('settingsCanonicalSha256') == NEW_SETTINGS_SHA,
            'ACTUAL_STAGED_RUST_SETTINGS_PROOF_REQUIRED')
    require(value.get('executionAuthority') is False and value.get('forecastCalibrationVerified') is False,
            'NO_MODEL_QUALIFICATION_OR_EXECUTION_AUTHORITY_CLAIM')


def stop_witness(boundary):
    return {'operator': OPERATOR, 'executionOwnerReviewed': True, 'oldPid': OLD_PID,
            'manifestSha256': boundary['manifestSha256'], 'requestSha256': boundary['requestSha256'],
            'stopSha256': boundary['stopSha256'], 'workerStatusSha256': boundary['workerStatusSha256'],
            'reason': 'controlled-cold-forecast-stop-only-not-human-stop'}


def approval_template(stage_sha, boundary_sha, typed_path, typed_sha, witness):
    for s in (stage_sha, boundary_sha, typed_sha): checked_sha(s)
    return {'schemaVersion': 1, 'operator': OPERATOR, 'executionOwnerReviewed': None,
            'executionOwner': 'Rhaegal-Viserion-paper6-20261016',
            'sourceCommit': SOURCE, 'binarySha256': BINARY, 'engineIdentity': ENGINE,
            'stagingSha256': stage_sha, 'boundarySha256': boundary_sha,
            'settingsCanonicalSha256': NEW_SETTINGS_SHA, 'newReviewCanonicalSha256': REVIEW_CANONICAL,
            'typedSettings': {'path': str(typed_path), 'sha256': typed_sha}, 'ownedStopWitness': witness,
            'reason': 'activate-only-reviewed-AK-ME-with-exact-existing-four-no-financial-or-binary-change'}


def validate_approval(value, stage_sha, boundary_sha, boundary):
    require(value.get('executionOwnerReviewed') is True, 'INDEPENDENT_OWNER_APPROVAL_REQUIRED')
    item = value.get('typedSettings', {})
    require(item == boundary['typedSettings'], 'APPROVED_TYPED_PROOF_CHANGED')
    expected = approval_template(stage_sha, boundary_sha, item['path'], item['sha256'], stop_witness(boundary))
    expected['executionOwnerReviewed'] = True
    require(value == expected, 'EXACT_OWNER_APPROVAL_AND_STOP_WITNESS_REQUIRED')


def unit_candidate(raw):
    old = ('ExecStart=' + OLD_RELEASE + ' run ' + OLD_CONTROL).encode()
    new = ('ExecStart=' + OLD_RELEASE + ' run ' + NEW_CONTROL).encode()
    lines = raw.splitlines(keepends=True)
    matches = [i for i, line in enumerate(lines) if line.rstrip(b'\r\n') == old]
    require(len(matches) == 1, 'EXACT_SINGLE_ORIGINAL_EXECSTART_REQUIRED')
    i = matches[0]
    lines[i] = new + lines[i][len(old):]
    return b''.join(lines)


def capital_unchanged(report):
    a = report['ownerCapitalAllocation']
    require(a['originalInitialCash'] == '40000.000000' and a['netContributions'] == '55579.915000' and
            a['effectiveInitialCash'] == a['effectiveMaximumCommitted'] == '95579.915000', 'EXACT_EXISTING_CAPITAL_REQUIRED')


def main(argv):
    require(__debug__, 'PYTHON_OPTIMIZATION_REFUSED')
    require(len(argv) in (4, 6) and argv[1] in ('stage', 'stop', 'promote'), 'USAGE_STAGE_STOP_PROMOTE')
    mode = argv[1]
    require(len(argv) == (4 if mode == 'stage' else 6), 'EXPLICIT_PROOF_OR_APPROVAL_REQUIRED')
    require(sys.platform.startswith('linux'), 'LINUX_EXECUTION_OWNER_ONLY')
    import fcntl
    import zipfile
    import io
    manifest_sha = checked_sha(argv[3])
    m = json.loads(read_pinned(argv[2], manifest_sha))
    validate_manifest(m)
    helper = pathlib.Path('/tmp/moloch-throughput-rollout.py')
    raw = read_pinned(helper, HELPER)
    prefix = raw.decode().split('command=sys.argv[1]')[0]
    require('def verified_dispatch(report):' in prefix and 'elif command' not in prefix, 'PINNED_HELPER_PREFIX_BOUNDARY')
    env = {}
    exec(compile(prefix, 'pinned-throughput-helper-prefix', 'exec'), env)
    root, shared, bank, base = (env[k] for k in ('root', 'shared', 'bank', 'base'))
    load, save, sha, seal, atomic, now, pid, quiet, verified_dispatch = (
        env[k] for k in ('load', 'save', 'sha', 'seal', 'atomic', 'now', 'pid', 'quiet', 'verified_dispatch'))
    require(root == shared/'live-production-v1' and str(shared) == '/home/archon/apps/moloch/shared', 'ORIGINAL_HELPER_ROOTS_REQUIRED')
    generation = no_symlink_path(GENERATION)
    stagefile = pathlib.Path('/tmp/moloch-cold-six-forecast-20261016-staging.json')
    boundaryfile = pathlib.Path('/tmp/moloch-cold-six-forecast-20261016-boundary.json')
    receiptfile = pathlib.Path('/tmp/moloch-cold-six-forecast-20261016-production-rollout.json')
    requestfile = root/'cold-six-forecast-20261016-stop-request.json'
    stopfile = root/'EXECUTION_STOP'
    globalstop = shared/'EXECUTION_STOP'
    unit = pathlib.Path.home()/'.config/systemd/user/moloch-live.service'
    dropin = unit.parent/'moloch-live.service.d/10-bounded-restart.conf'

    def immutable(path, data):
        path = no_symlink_path(path)
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        no_symlink_path(path.parent)
        try:
            fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o400)
        except FileExistsError:
            require(read_pinned(path, digest(data), len(data)) == data, 'IMMUTABLE_CONFLICT')
            return path
        with os.fdopen(fd, 'wb') as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        d = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try: os.fsync(d)
        finally: os.close(d)
        return path

    def service_pins():
        read_pinned(unit, OLD_UNIT_SHA)
        read_pinned(dropin, DROPIN)
        read_pinned(dropin.parent/BACKUP_NAME, BACKUP_SHA)
        require(sorted(p.name for p in dropin.parent.iterdir()) == sorted([dropin.name, BACKUP_NAME]), 'RESTART_FILES_CHANGED')

    def prior_process():
        p = pathlib.Path('/proc', str(OLD_PID))
        require(pid() == OLD_PID and sha(p/'exe') == BINARY and os.readlink(p/'exe') == OLD_RELEASE and
                (p/'cmdline').read_bytes().split(b'\0') == [OLD_RELEASE.encode(), b'run', OLD_CONTROL.encode(), b''], 'ACTUAL_ORIGINAL_OWNER_CHANGED')

    def queue_quiet():
        q = quiet()
        require(type(q['globalUnknownCount']) is int and q['globalUnknownCount'] == 0 and
                q['state']['active'] is None and q['state']['manual_review'] is False, 'QUEUE_NOT_QUIET')

    def held():
        require(pid() == 0 and not pathlib.Path('/proc', str(OLD_PID)).exists() and not globalstop.exists(), 'ORIGINAL_OWNER_NOT_STOPPED')
        lock = no_symlink_path(shared/'capture.lock')
        f = lock.open('r+')
        try: fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BaseException:
            f.close()
            raise
        return f

    def snapshot():
        h, r, mf = (seal(bank/x) for x in ('head.json', 'report.json', 'manifest.json'))
        require(h['sequence'] == r['sequence'] and h['previous'] == r['previous'] and
                h['manifestHash'] == r['manifestHash'] == mf['hash'] == BANK_MANIFEST, 'BANK_SEALS_CHANGED')
        capital_unchanged(r['report'])
        bridge = seal(bank/'engine-upgrade-v1.json')
        require(sha(bank/'engine-upgrade-v1.json') == BRIDGE_SHA and len(bridge['upgrades']) == 12 and
                bridge['upgrades'][-1]['toEngineIdentity'] == ENGINE, 'ORIGINAL_TWELVE_BRIDGES_REQUIRED')
        return {'head': h, 'reportSha256': sha(bank/'report.json'), 'manifestSha256': sha(bank/'manifest.json'),
                'prefixBytes': (bank/'events.jsonl').stat().st_size, 'prefixSha256': sha(bank/'events.jsonl'),
                'bridgeSha256': BRIDGE_SHA, 'dispatch': verified_dispatch(r),
                'dispatchSha256': sha(root/'dispatch.ndjson'), 'fundamentalSha256': sha(root/'fundamental.ndjson'),
                'fundamentalMarkerSha256': sha(root/'fundamental.initialized.json'),
                'packageDispatchSha256': sha(root/'package-dispatch/package-dispatch.ndjson')}

    def models():
        read_pinned(pathlib.Path(OLD_SETTINGS['authorityPath']), OLD_SETTINGS['authoritySha256'])
        authority = json.loads(read_pinned(generation/'authority.json', PAYLOAD_HASHES['authority.json']))
        oldauthority = load(OLD_SETTINGS['authorityPath'])
        require(authority == NEW_AUTHORITY and authority['trustedReviewSha256'] ==
                oldauthority['trustedReviewSha256']+REVIEW_CANONICAL, 'APPEND_ONLY_NEW_REVIEW_AUTHORITY_REQUIRED')
        for r in NEW_SETTINGS['reviews']:
            read_pinned(r['path'], r['fileSha256'])
            for s in r['sourceFiles']: read_pinned(s['path'], s['sha256'])
        reviews = [load(r['path']) for r in NEW_SETTINGS['reviews'][4:]]
        source_lifetimes(reviews, time.time_ns()//1_000_000)
        settings_delta(OLD_SETTINGS, load(generation/'settings.json'))

    def stage_unchanged(s):
        require(s['manifestSha256'] == manifest_sha and s['sourceCommit'] == SOURCE and
                s['binarySha256'] == BINARY and s['engineIdentity'] == ENGINE and
                s['settingsCanonicalSha256'] == NEW_SETTINGS_SHA, 'STAGING_BINDING_CHANGED')
        read_pinned(OLD_CONTROL, OLD_CONTROL_SHA)
        require(sha(OLD_RELEASE) == BINARY, 'ORIGINAL_BINARY_CHANGED')
        read_pinned(NEW_CONTROL, s['controlSha256'])
        read_pinned(generation/'moloch-live.service', s['unitCandidateSha256'])
        read_pinned(generation/'settings.json', s['settingsRawSha256'])
        controls(load(OLD_CONTROL), load(NEW_CONTROL))
        models()
        service_pins()

    def proof(item):
        value = json.loads(read_pinned(item['path'], item['sha256']))
        typed_settings_proof(value)

    if mode == 'stage':
        require(not stagefile.exists() and not boundaryfile.exists() and not receiptfile.exists(), 'STAGE_ALREADY_RECORDED')
        prior_process()
        service_pins()
        read_pinned(OLD_SUMMARY, OLD_SUMMARY_SHA)
        before = json.loads(read_pinned(OLD_CONTROL, OLD_CONTROL_SHA))
        settings_delta(before['fundamentalPolicy'], NEW_SETTINGS)
        require(not stopfile.exists() and not globalstop.exists(), 'PREEXISTING_STOP')
        archive = read_pinned(m['packetProof']['path'], PACKET_PROOF_SHA)
        with zipfile.ZipFile(io.BytesIO(archive)) as z:
            for name, expected in ORIGINAL_PROOF_HASHES.items():
                require(digest(z.read(name)) == expected, 'ORIGINAL_TYPED_PROOF_CHANGED')
            originals = {name: z.read(name) for name in ('authority.json', 'alaska-review.json', 'maine-review.json')}
        packet = no_symlink_path(m['packetDirectory'])
        for relative, expected in PAYLOAD_HASHES.items():
            data = originals[relative] if relative in originals else read_pinned(packet/relative, expected)
            require(digest(data) == expected, 'EXACT_REVIEWED_PAYLOAD_REQUIRED')
            immutable(generation/relative, data)
        immutable(generation/'settings.json', encoded(NEW_SETTINGS)+b'\n')
        models()
        control = copy.deepcopy(before)
        control['fundamentalPolicy'] = copy.deepcopy(NEW_SETTINGS)
        controls(before, control)
        immutable(NEW_CONTROL, encoded(control)+b'\n')
        candidate = immutable(generation/'moloch-live.service', unit_candidate(unit.read_bytes()))
        subprocess.run(['systemd-analyze', '--user', 'verify', str(candidate)], check=True)
        immutable(generation/'previous-unit.service', unit.read_bytes())
        immutable(generation/'previous-control.json', read_pinned(OLD_CONTROL, OLD_CONTROL_SHA))
        s = {'manifestSha256': manifest_sha, 'sourceCommit': SOURCE, 'binarySha256': BINARY, 'engineIdentity': ENGINE,
             'control': NEW_CONTROL, 'controlSha256': sha(NEW_CONTROL), 'unitCandidateSha256': sha(candidate),
             'settingsCanonicalSha256': NEW_SETTINGS_SHA, 'settingsRawSha256': sha(generation/'settings.json'),
             'generation': GENERATION, 'oldPid': OLD_PID, 'oldFourEntriesUnchanged': True,
             'freshPhysicalFactsStillRequired': True, 'binaryChanged': False, 'financialHistoryChanged': False}
        save(stagefile, s)
        print(json.dumps({'stagedOnly': True, 'stagingFile': str(stagefile), 'stagingSha256': sha(stagefile),
                          'settingsCanonicalSha256': NEW_SETTINGS_SHA, 'requiresStagedRustValidationBeforeStop': True}))
        return

    s = load(stagefile)
    stage_unchanged(s)
    if mode == 'stop':
        require(not boundaryfile.exists() and not requestfile.exists() and not receiptfile.exists(), 'STOP_ALREADY_REQUESTED')
        item = {'path': str(no_symlink_path(argv[4])), 'sha256': checked_sha(argv[5])}
        proof(item)
        prior_process()
        require(not stopfile.exists() and not globalstop.exists(), 'PREEXISTING_STOP')
        request = {'operator': OPERATOR, 'token': uuid.uuid4().hex, 'at': now(), 'oldPid': OLD_PID,
                   'reason': 'CONTROLLED_COLD_FORECAST_SETTINGS_ONLY', 'action': 'finish operation and stop; no new requests or writes'}
        immutable(requestfile, encoded(request)+b'\n')
        immutable(stopfile, encoded(request)+b'\n')
        for _ in range(240):
            if pid() == 0: break
            time.sleep(0.5)
        with held():
            snap = snapshot()
            queue_quiet()
            require(snapshot() == snap, 'STOPPED_BOUNDARY_MOVED')
            save(boundaryfile, {'manifestSha256': manifest_sha, 'snapshot': snap,
                               'requestSha256': sha(requestfile), 'stopSha256': sha(stopfile),
                               'workerStatusSha256': sha(root/'status.json'), 'typedSettings': item})
        print('STOPPED_BOUNDARY_CAPTURED; ROOT_APPROVAL_AND_OWNED_STOP_WITNESS_REQUIRED', sha(boundaryfile))
        return

    require(not receiptfile.exists(), 'PROMOTION_ALREADY_RECORDED')
    approval = json.loads(read_pinned(argv[4], checked_sha(argv[5])))
    b = load(boundaryfile)
    validate_approval(approval, sha(stagefile), sha(boundaryfile), b)
    proof(b['typedSettings'])
    with held():
        stage_unchanged(s)
        require(b['manifestSha256'] == manifest_sha and snapshot() == b['snapshot'], 'STOPPED_BOUNDARY_CHANGED')
        queue_quiet()
        require(snapshot() == b['snapshot'], 'BOUNDARY_CHANGED_DURING_QUEUE_CHECK')
        require(sha(stopfile) == b['stopSha256'] and sha(requestfile) == b['requestSha256'] and
                sha(root/'status.json') == b['workerStatusSha256'], 'OWNED_STOP_OR_STATUS_CHANGED')
        require(load(requestfile)['operator'] == OPERATOR and load(requestfile)['oldPid'] == OLD_PID, 'ORIGINAL_STOP_REQUEST_REQUIRED')
        # All financial state is still frozen. No model or source clock is rewritten.
        models()
        backup = unit.with_name(unit.name+'.before-cold-six-forecast-20261016')
        require(not backup.exists(), 'UNIT_BACKUP_ALREADY_EXISTS')
        immutable(backup, read_pinned(unit, OLD_UNIT_SHA))
        atomic(unit, read_pinned(generation/'moloch-live.service', s['unitCandidateSha256']))
        subprocess.run(['systemctl', '--user', 'daemon-reload'], check=True)
        require(sha(unit) == s['unitCandidateSha256'] and sha(dropin) == DROPIN and
                sha(dropin.parent/BACKUP_NAME) == BACKUP_SHA and not globalstop.exists(), 'INSTALLED_UNIT_OR_GLOBAL_STOP_CHANGED')
        require(sha(stopfile) == b['stopSha256'], 'STOP_CHANGED_BEFORE_ARCHIVE')
        archive = root/('EXECUTION_STOP.cold-six-forecast-'+load(requestfile)['token']+'.json')
        require(not archive.exists(), 'STOP_ARCHIVE_ALREADY_EXISTS')
        try:
            os.rename(stopfile, archive)
            require(sha(archive) == b['stopSha256'], 'ARCHIVED_STOP_CHANGED')
        except BaseException:
            if not stopfile.exists(): save(stopfile, {'operator': OPERATOR, 'reason': 'COLD_FORECAST_STOP_ARCHIVE_FAILED_NO_RETRY', 'at': now()})
            raise
    try:
        require(pid() == 0 and not pathlib.Path('/proc', str(OLD_PID)).exists() and
                not stopfile.exists() and not globalstop.exists(), 'OWNER_OR_STOP_CHANGED_BEFORE_START')
        # No automatic rollback/retry: any failed start remains blocked for the execution owner.
        subprocess.run(['systemctl', '--user', 'reset-failed', 'moloch-live.service'], check=True)
        subprocess.run(['systemctl', '--user', 'start', 'moloch-live.service'], check=True)
        newpid = pid()
        p = pathlib.Path('/proc', str(newpid))
        require(newpid > 0 and newpid != OLD_PID and sha(p/'exe') == BINARY and
                os.readlink(p/'exe') == OLD_RELEASE and (p/'cmdline').read_bytes().split(b'\0') ==
                [OLD_RELEASE.encode(), b'run', NEW_CONTROL.encode(), b''], 'STARTED_PROCESS_BINDING_FAILED')
    except BaseException:
        if not stopfile.exists(): save(stopfile, {'operator': OPERATOR, 'reason': 'COLD_FORECAST_START_FAILED_NO_RETRY', 'at': now()})
        raise
    save(receiptfile, {'at': now(), 'operator': OPERATOR, 'oldPid': OLD_PID, 'newPid': newpid,
                       'sourceCommit': SOURCE, 'binarySha256': BINARY, 'engineIdentity': ENGINE,
                       'control': NEW_CONTROL, 'controlSha256': s['controlSha256'], 'release': OLD_RELEASE,
                       'approvedManifestSha256': manifest_sha, 'approvalSha256': argv[5], 'boundary': b,
                       'settingsCanonicalSha256': NEW_SETTINGS_SHA, 'newReviewCanonicalSha256': REVIEW_CANONICAL,
                       'controlChanges': ['fundamentalPolicy'], 'bridgeWritten': False, 'binaryChanged': False,
                       'financialHistoryChangedByOperator': False, 'freshTradingVerified': False,
                       'profitabilityEstablished': False})
    print('STARTED_SAME_BINARY; RESTORATION_AND_FRESH_HEALTH_PENDING', newpid)


if __name__ == '__main__':
    main(sys.argv)
