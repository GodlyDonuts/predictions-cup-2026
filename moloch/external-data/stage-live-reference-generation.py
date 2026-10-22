"""Stage immutable owner-selected valuations for the existing worker's reload.

No trading client, ledger writes or reload request. Requires the installed typed
review inspector; preserves the base control and all original review/source files.
"""
import argparse
import base64
import hashlib
import json
import subprocess
import time
from decimal import Decimal
from pathlib import Path


def sha(b):
    return hashlib.sha256(b).hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('base-control', 'capture', 'output', 'review-helper'):
        p.add_argument('--' + name, type=Path, required=True)
    p.add_argument('--base-sha256', required=True)
    p.add_argument('--execution-modes-policy', type=Path)
    p.add_argument('--execution-modes-sha256')
    a = p.parse_args()
    original = a.base_control.read_bytes()
    if sha(original) != a.base_sha256 or a.output.exists():
        raise ValueError('PINNED_BASE_AND_NEW_DIRECTORY_REQUIRED')
    control = json.loads(original)
    modes = {}
    margins = {}
    mode_raw = b''
    if a.execution_modes_policy or a.execution_modes_sha256:
        if not a.execution_modes_policy or not a.execution_modes_sha256:
            raise ValueError('EXECUTION_MODES_POLICY_PIN')
        mode_raw = a.execution_modes_policy.read_bytes()
        if sha(mode_raw) != a.execution_modes_sha256:
            raise ValueError('EXECUTION_MODES_POLICY_PIN')
        mode_policy = json.loads(mode_raw)
        if mode_policy['version'] != 1 or not isinstance(mode_policy['exchanges'], dict):
            raise ValueError('EXECUTION_MODES_POLICY_FORMAT')
        modes = mode_policy['exchanges']
        margins = mode_policy.get('entryMargins', {})
        if not isinstance(margins, dict) or not set(margins) <= set(modes):
            raise ValueError('EXECUTION_MARGIN_POLICY_SCOPE')
        for value in margins.values():
            if (not isinstance(value, str) or len(value) != 8 or not value.startswith('0.')
                    or any(c not in '0123456789' for c in value[2:]) or Decimal(value) <= 0):
                raise ValueError('EXECUTION_MARGIN_POLICY_VALUE')
        for choices in modes.values():
            if (not isinstance(choices, list) or not choices or len(set(choices)) != len(choices)
                    or not set(choices) <= {'joinBid','improveOneTick','boundedLimitMayRest'}):
                raise ValueError('EXECUTION_MODES_POLICY_CHOICES')
        known = {json.loads(Path(x['path']).read_bytes())['exchangeId'] for x in control['fundamentalPolicy']['reviews']}
        if not set(modes) <= known:
            raise ValueError('EXECUTION_MODES_POLICY_SCOPE')
    capture_raw = a.capture.read_bytes()
    capture = json.loads(capture_raw)
    now = time.time_ns() // 1_000_000
    if capture['model'] != 'conditional_Kalshi_party_midpoint_v1' or not 0 <= now - capture['capturedAtMs'] <= 120_000:
        raise ValueError('FRESH_CAPTURE_REQUIRED')
    by_exchange = {r['exchangeId']: r for r in capture['rows']}
    if len(by_exchange) != len(capture['rows']):
        raise ValueError('UNIQUE_CAPTURE_EXCHANGES')
    a.output.mkdir(parents=True)
    (a.output / 'capture.original.json').write_bytes(capture_raw)
    if modes:
        (a.output / 'execution-modes.original.json').write_bytes(mode_raw)
    reviews, trust, changes = [], [], []
    for old in control['fundamentalPolicy']['reviews']:
        raw = Path(old['path']).read_bytes()
        if sha(raw) != old['fileSha256']:
            raise ValueError('ORIGINAL_REVIEW_CHANGED')
        r = json.loads(raw)
        entry = by_exchange.get(r['exchangeId'])
        if entry is None:
            reviews.append(old)
            trust.append(subprocess.check_output([str(a.review_helper), 'hash-review', old['path']], text=True).strip())
            continue
        if r['version'] != 2 or any(str(entry[k]) != str(r[k]) for k in ('exchangeId', 'marketId', 'contractId', 'officialContractRootSha256')):
            raise ValueError('EXACT_VERSIONED_CONTRACT_MATCH')
        q = json.loads(base64.b64decode(entry['quoteRecordBase64'], validate=True))
        if q['ticker'] != entry['kalshiTicker'] or q['status'] != 200 or sha(q['bodyText'].encode()) != q['bodySha256'] or json.loads(q['bodyText']) != q['body']:
            raise ValueError('ORIGINAL_QUOTE_IDENTITY')
        if not 0 <= now - entry['quoteObservedAtMs'] <= 120_000:
            raise ValueError('FRESH_ORIGINAL_QUOTE_REQUIRED')
        midpoint = Decimal(entry['yesMidpoint'])
        book = q['body']['orderbook_fp']
        bid = max(Decimal(level[0]) for level in book['yes_dollars'])
        ask = 1 - max(Decimal(level[0]) for level in book['no_dollars'])
        if midpoint != (bid + ask) / 2 or not 0 < bid <= ask < 1:
            raise ValueError('MODEL_MIDPOINT_MUST_MATCH_ORIGINAL_BOOK')
        if not 0 < midpoint < 1:
            raise ValueError('BINARY_MODEL_RANGE')
        point = midpoint if r['side'] == 'YES' else 1 - midpoint
        before = {k: r.get(k) for k in ('ordinaryProbability', 'passiveValueExit', 'expiresAtMs', 'executionPolicy', 'marginPerShare')}
        forecast = {'version': 1, 'kind': 'execution_owner_conditional_live_reference_model',
            'captureSha256': sha(capture_raw), 'capturedAtMs': capture['capturedAtMs'],
            'model': capture['model'], 'side': r['side'], 'ordinaryProbability': format(point, '.6f'),
            'calibratedProbabilityClaimed': False, 'settlementEquivalenceClaimed': False,
            'mlFitOrSelectionUse': False, 'sourceEvidence': entry}
        if r['exchangeId'] in modes:
            forecast['executionModesPolicySha256'] = a.execution_modes_sha256
            r['executionPolicy'] = {'version': 1, 'allowedModes': modes[r['exchangeId']]}
        if r['exchangeId'] in margins:
            forecast['entryMarginOwnerPolicySha256'] = a.execution_modes_sha256
            forecast['entryMarginPerShare'] = margins[r['exchangeId']]
            r['marginPerShare'] = margins[r['exchangeId']]
        fraw = json.dumps(forecast, separators=(',', ':')).encode()
        fp = a.output / (r['exchangeId'] + '-forecast.json')
        fp.write_bytes(fraw)
        sources = []
        for s in r['sources']:
            if s['kind'] == 'forecast':
                s = {'kind': 'forecast', 'url': 'https://api.elections.kalshi.com' + q['path'],
                    'sha256': sha(fraw), 'publishedAtMs': 0, 'observedAtMs': entry['quoteObservedAtMs'], 'maximumAgeMs': 900_000}
                sources.append({'path': str(fp), 'sha256': sha(fraw)})
            else:
                # Retained election identity evidence, not a fresh quote. The
                # sole worker still checks the exact current SIG contract root.
                s = dict(s, maximumAgeMs=14 * 86_400_000)
                source = next(x for x in old['sourceFiles'] if x['sha256'] == s['sha256'])
                if sha(Path(source['path']).read_bytes()) != source['sha256']:
                    raise ValueError('ORIGINAL_STATIC_SOURCE_CHANGED')
                sources.append(source)
            r['sources'][len(sources) - 1] = s
        r.update(reviewerId='Viserion-live-reference-20261021', ordinaryProbability=format(point, '.6f'),
            reviewedAtMs=now, expiresAtMs=entry['quoteObservedAtMs'] + 900_000,
            assumptionLabel='Execution-owner conditional Kalshi party midpoint model, observed from retained original public receipts. Prices are uncalibrated forecasts, not settlement equivalence or guaranteed profits. Original exceptional allowance remains; entry margin changes only under a pinned execution-owner policy recorded in the forecast evidence. Polymarket exact-party observations are retained as context only. Original ballot/contract identity clocks and bytes remain; their declared lifetime is 14 days, with exact current SIG-root verification before dispatch. Forecast lifetime is 15 minutes. FIFO profit turnover remains except Nevada governor and Alaska Senate: those two use current modeled value for capital recycling and may realize a loss. Original holdings, basis, cohort objectives and horizons remain unchanged.')
        if r['exchangeId'] in ('872', '1067'):
            r['passiveValueExit'].pop('pricingObjective', None)
        path = a.output / (r['exchangeId'] + '-review.json')
        path.write_text(json.dumps(r, separators=(',', ':')), encoding='utf-8')
        canonical = subprocess.check_output([str(a.review_helper), 'hash-review', str(path)], text=True).strip()
        trust.append(canonical)
        reviews.append({'path': str(path), 'fileSha256': sha(path.read_bytes()), 'sourceFiles': sources})
        changes.append({'exchangeId': r['exchangeId'], 'side': r['side'], 'before': before,
            'after': {k: r.get(k) for k in before}, 'reviewSha256': canonical})
    if {x['exchangeId'] for x in changes} != set(by_exchange):
        raise ValueError('CAPTURE_HAS_UNMANAGED_SCOPE')
    ap = a.output / 'authority.owner-reviewed.json'
    ap.write_text(json.dumps({'version': 1, 'trustedReviewSha256': sorted(trust)}, separators=(',', ':')), encoding='utf-8')
    control['fundamentalPolicy'] = {'version': 1, 'allowInitialization': False,
        'authorityPath': str(ap), 'authoritySha256': sha(ap.read_bytes()), 'reviews': reviews}
    cp = a.output / 'control.owner-reviewed.json'
    cp.write_text(json.dumps(control, separators=(',', ':')), encoding='utf-8')
    validation = json.loads(subprocess.check_output([str(a.review_helper), 'validate-control', str(cp), str(time.time_ns() // 1_000_000)], text=True))
    if not all(x['current'] for x in validation['reviews'] if x['exchangeId'] in by_exchange):
        raise ValueError('CHANGED_REVIEW_NOT_CURRENT')
    if a.base_control.read_bytes() != original:
        raise ValueError('BASE_CHANGED_DURING_STAGE')
    result = {'baseSha256': a.base_sha256, 'controlPath': str(cp), 'controlSha256': sha(cp.read_bytes()),
        'captureSha256': sha(capture_raw), 'validation': validation, 'changes': changes,
        'stagedOnly': True, 'officialSigRequests': 0, 'reloadSent': False}
    (a.output / 'staged.json').write_text(json.dumps(result, indent=2), encoding='utf-8')
    print(json.dumps({'controlPath': str(cp), 'controlSha256': result['controlSha256'], 'changed': len(changes), 'stagedOnly': True}))


if __name__ == '__main__':
    main()
