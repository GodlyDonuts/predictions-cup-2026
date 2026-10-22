"""Stage a Rhode Island review; no account request or reload publication.

Existing inventory requires its original adoption identity. The owner captures
and verifies the actual bank, account and lots independently during reload.
"""
from pathlib import Path
from decimal import Decimal, ROUND_DOWN
from datetime import datetime
import copy
import argparse
import hashlib
import json
import os
import subprocess
import time

REFERENCE = Path('/home/archon/apps/moloch/shared/fundamental-live-reference-20261021')
SOURCE = Path('/tmp/moloch-rank-current-r99-20261022')


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def put(path, value):
    raw = (json.dumps(value, separators=(',', ':'), ensure_ascii=False)+'\n').encode()
    with path.open('xb') as f:
        f.write(raw)
    return sha(raw)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--capture', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--existing-inventory', action='store_true')
    args = parser.parse_args()
    root_output = args.output
    os.umask(0o077)
    native_path = Path('/home/archon/apps/moloch/shared/live-production-v1/tartarus-current-native-state.json')
    native_raw = native_path.read_bytes()
    native = json.loads(native_raw)
    actual_lots = native['account']['positions'].get('1076', {'YES': [], 'NO': []})
    original_position = None
    approval = None
    if args.existing_inventory:
        report_raw = (SOURCE/'inputs/report.original.json').read_bytes()
        inherited = json.loads(report_raw)['report']['inheritedInventory']
        matches = [x for x in inherited['sourcePositions'] if x['exchangeId'] == '1076']
        assert len(matches) == 1
        original_position = matches[0]
        lots = [{'price': format(Decimal(x['entryPrice']), '.6f'), 'quantity': x['quantity']}
                for x in original_position['lots']]
        assert actual_lots == {'YES': lots, 'NO': []}, 'EXISTING_ORIGINAL_FIFO_CHANGED'
        assert original_position['quantity'] == sum(x['quantity'] for x in lots) > 0
        approval = {'adoptionReceiptSha256': inherited['receiptHash'],
                    'adoptedPositionSha256': sha(json.dumps(original_position, sort_keys=True,
                        separators=(',', ':'), ensure_ascii=False).encode())}
    else:
        assert not actual_lots['YES'] and not actual_lots['NO'], 'NEW_SCOPE_NOT_FLAT'
    root_output.mkdir(mode=0o700)
    state_raw = (REFERENCE/'state.json').read_bytes()
    state = json.loads(state_raw)
    original = Path(state['controlPath']).read_bytes()
    assert sha(original) == state['controlSha256']
    control = json.loads(original)
    config = json.loads((REFERENCE/'config-agent-maker-r101.json').read_bytes())
    assert config['workerPid'] == 1131806
    helper = Path(config['reviewHelper'])
    assert sha(helper.read_bytes()) == config['reviewHelperSha256']
    rows = json.loads((SOURCE/'originals/full-universe-expanded/census.private.json').read_bytes())['rows']
    target = next(r for r in rows if r['exchangeId'] == '1076')
    reviews = [(x, json.loads(Path(x['path']).read_bytes())) for x in control['fundamentalPolicy']['reviews']]
    assert all(r['exchangeId'] != '1076' for _, r in reviews)
    old, parent = next((x, r) for x, r in reviews if r['exchangeId'] == '1077')
    raw = args.capture.read_bytes()
    assert len(raw) <= 4_000_000
    capture = json.loads(raw)
    assert {r['exchangeId'] for r in capture['rows']} == {'1076', '1077', '983'}
    entry = next(r for r in capture['rows'] if r['exchangeId'] == '1076')
    point = Decimal(entry['yesMidpoint'])
    exceptional = Decimal(parent['exceptionalProbabilityAllowance'])
    floor = Decimal(parent['minimumExceptionalPayout'])
    value = (point*(1-exceptional)+exceptional*floor).quantize(Decimal('.000001'), rounding=ROUND_DOWN)
    margin = value-Decimal('.890000')
    assert Decimal('0') < margin < Decimal('1')
    review = copy.deepcopy(parent)
    assert review.get('existingOrigin') is None
    review.update(exchangeId='1076', marketId=target['marketId'], contractId=target['contractId'],
                  officialContractRootSha256=target['officialContractRootSha256'], side='YES',
                  ordinaryProbability=format(point, '.6f'), marginPerShare=format(margin, '.6f'),
                  maximumPrincipal='1750.000000', maximumQuantity=2000,
                  executionPolicy={'version': 1, 'allowedModes': ['boundedLimitMayRest']},
                  reviewerId='Viserion-ri-senate-maker-1076-20261022')
    review['passiveValueExit']['maximumQuantity'] = 2000
    if args.existing_inventory:
        review.update(existingOrigin=approval, maximumPrincipal='10000.000000',
                      maximumQuantity=10000, sameSideTopUps={'version': 2, 'maximumEntries': 16})
        # The inherited lots do not establish historical acquisition fees.
        # Use model-value exits, never label inherited markup as new FIFO profit.
        review['passiveValueExit'] = {'version': 1, 'maximumQuantity': 10000}
        put(root_output/'existing-original-position.json', original_position)
        put(root_output/'existing-native-observation.json',
            {'writtenAt': native['writtenAt'], 'lots': actual_lots,
             'nativeViewSha256': sha(native_raw),
             'originalReportSha256': sha(report_raw), 'approval': approval})
    root_path = SOURCE/'originals/roots'/target['originalFile']
    root_raw = root_path.read_bytes()
    root = json.loads(root_raw)
    assert root['status'] == 200
    node = root['data']['root']
    assert sha(json.dumps(node, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()) == target['officialContractRootSha256']
    settlement = int(datetime.fromisoformat(node['settlement_date'].replace('Z', '+00:00')).timestamp()*1000)
    assert settlement == review['holdUntilMs']
    review['valuationObjective'].update(timingSourceSha256=sha(root_raw), scheduledSettlementAtMs=settlement)
    files = []
    for i, source in enumerate(review['sources']):
        if source['kind'] == 'official_contract_node':
            review['sources'][i] = dict(source, url='https://sig.thesuper.market/api/v1'+root['endpoint'],
                sha256=sha(root_raw), observedAtMs=int(datetime.fromisoformat(root['receivedAt'].replace('Z', '+00:00')).timestamp()*1000))
            files.append({'path': str(root_path), 'sha256': sha(root_raw)})
        else:
            # The staged refresh below replaces the forecast with this exact
            # target's fresh captured receipt. The RI official ballot includes
            # both JOHN FRANCIS REED/Democrat and RAYMOND T MCKAY/Republican.
            files.append(next(x for x in old['sourceFiles'] if x['sha256'] == source['sha256']))
    seed_review = root_output/'1076-template.unapproved.json'
    review_hash = put(seed_review, review)
    control['fundamentalPolicy']['reviews'].append({'path': str(seed_review), 'fileSha256': review_hash, 'sourceFiles': files})
    seed = root_output/'control-template.unapproved.json'
    seed_hash = put(seed, control)
    capture_path = root_output/'capture.original.json'
    with capture_path.open('xb') as f:
        f.write(raw)
    policy = {'version': 1, 'exchanges': {'1077': ['joinBid'], '983': ['joinBid'], '1076': ['boundedLimitMayRest']},
              'entryMargins': {'1076': format(margin, '.6f')}}
    policy_path = root_output/'entry-policy.owner-selected.json'
    policy_hash = put(policy_path, policy)
    assert (REFERENCE/'state.json').read_bytes() == state_raw, 'GENERATION_CHANGED_DURING_STAGE'
    assert sha(Path(config['stageScript']).read_bytes()) == config['stageScriptSha256']
    subprocess.run(['python3', config['stageScript'], '--base-control', str(seed), '--base-sha256', seed_hash,
                    '--capture', str(capture_path), '--output', str(root_output/'generation'), '--review-helper', str(helper),
                    '--execution-modes-policy', str(policy_path), '--execution-modes-sha256', policy_hash], check=True)
    put(root_output/'decision.public.json', dict(observedAtMs=time.time_ns()//1_000_000,
        parentGeneration=state['generationSha256'], sourceControlSha256=state['controlSha256'],
        exchangeId='1076', race=target['raceName'], party=target['winnerName'], side='YES',
        maximumPrincipal=review['maximumPrincipal'], maximumQuantity=review['maximumQuantity'],
        existingOrigin=approval, originalPosition=original_position, initialPriceCeiling='0.890000',
        ordinaryProbability=format(point, '.6f'), exceptionalAllowance=review['exceptionalProbabilityAllowance'],
        makerExitObjective=review['passiveValueExit'], modelCalibrated=False, sourceClocksRenewed=False,
        newAccountClients=0, reloadPublished=False, productionChanged=False))


if __name__ == '__main__':
    main()
