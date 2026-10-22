"""Explicit decision model selection; dated opinions remain retained stress evidence."""
from copy import deepcopy
from hashlib import sha256
import json
CURRENT = ('price_lower_common', 'price_upper_common', 'price_midpoint_independent')
HISTORICAL = ('historical_review_envelope_lower_common', 'historical_review_envelope_upper_common')
POLICIES = ('all-declared', 'current-prices')
def expected_ids(policy):
    if policy not in POLICIES:
        raise ValueError('EXPLICIT_DECISION_PROFILE_POLICY')
    return list(CURRENT + HISTORICAL if policy == 'all-declared' else CURRENT)
def select(reference, parent_raw, policy):
    if json.loads(parent_raw) != reference:
        raise ValueError('DECISION_PROFILE_PARENT_CHANGED')
    models = reference['model']['probabilityModels']
    profiles = reference['profiles']
    ids = [m['id'] for m in models]
    if ids != list(CURRENT + HISTORICAL) or [p['id'] for p in profiles] != ids:
        raise ValueError('COMPLETE_ORIGINAL_DECISION_PROFILES_REQUIRED')
    keep = expected_ids(policy)
    result = deepcopy(reference)
    result['model']['probabilityModels'] = [m for m in result['model']['probabilityModels'] if m['id'] in keep]
    result['profiles'] = [p for p in result['profiles'] if p['id'] in keep]
    result['decisionProfileSelection'] = dict(policy=policy, parentModelSha256=sha256(parent_raw).hexdigest(),
        selectedIds=keep, retainedStressOnlyIds=[i for i in ids if i not in keep],
        sourceClocksRenewed=False, calibratedProbabilityClaimed=False,
        assumption='Current receipt-timed price bounds and dependence sensitivities govern this owner-selected policy. Historical opinions remain retained stress evidence; selection is not statistical calibration.')
    return result
def verify_summary(summary, campaign, policy):
    ids = expected_ids(policy)
    if (summary.get('decisionProfilePolicy') != policy or summary.get('decisionProfileIds') != ids
        or summary.get('probabilityModels') != len(ids)
        or [m['id'] for m in campaign['probabilityModels']] != ids):
        raise ValueError('REVIEWED_DECISION_PROFILE_POLICY_CHANGED')
