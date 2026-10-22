"""Check an old large-order hypothesis as a sequence of legal-sized tranches.

This preserves dated book depth/FIFO and never makes those books current,
credits proceeds, approves a plan, or submits orders.
"""
import argparse
from copy import deepcopy
from hashlib import sha256
import json
import os
from pathlib import Path
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--study-root', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    os.nice(15)
    Path('/proc/self/oom_score_adj').write_text('1000')
    sys.path.insert(0, str(args.source))
    sys.path.insert(0, '/tmp/moloch-upgrade-bound-r100-20261022/money-tools')
    from moloch.tartarus.sequence_audit import audit
    from moloch.tartarus.sequential_frontier import ASSUMPTIONS
    from research_book_observations import load as research_books
    load = lambda path: json.loads(path.read_bytes())
    command = load(args.study_root/'current-research-command.json')
    inputs = Path(command[command.index('--output')+1])/'inputs'
    compiled = load(inputs/'compiled-current.private.json')
    previous = load(args.study_root/'larger-order-win-search-001.private.json')
    assert previous['originalSourcePortfolioSha256'] == compiled['reportSha256']
    original = previous['result']['legs']
    financial = deepcopy(compiled['financial'])
    cap = financial['physicalOrderQuantity']
    assert cap == previous['actualRuntimePhysicalOrderQuantity'] == 10000
    serial, groups = [], []
    for leg in original:
        left = leg['quantity']
        first = len(serial)
        while left:
            take = min(left, cap)
            serial.append({k:leg[k] for k in ('ex','side','action','price')} | {'quantity':take})
            left -= take
        assert sum(x['quantity'] for x in serial[first:]) == leg['quantity']
        groups.append(dict(originalLeg=leg, serialStart=first, serialEnd=len(serial)))
    # This is a finite expanded research horizon, not a native campaign approval.
    source_horizon = financial['conditionalFutureLegs']
    financial['conditionalFutureLegs'] = max(source_horizon, len(serial)-1)
    larger = deepcopy(financial)
    larger['physicalOrderQuantity'] = previous['researchOnlyPhysicalOrderQuantity']
    registry = load(args.source/'originals/full-universe-expanded/census.private.json')['rows']
    reference = load(inputs/'issued-reference-model.private.json')
    books, _ = research_books(inputs, compiled, registry)
    field = previous['publicField']
    selected_field = dict(reference['fields'][0], id='public_flat_anchor_20261022T183802',
        thresholds=[field['derivedFlatMarkedWealthAnchor']]*len(reference['model']['scenarios']))
    before = audit(larger, registry, books, reference['model'], [selected_field], original, assumptions=ASSUMPTIONS)
    after = audit(financial, registry, books, reference['model'], [selected_field], serial, assumptions=ASSUMPTIONS)
    for key in ('cashMicros','ownedCostMicros','terminalWealthMicros','models','fieldPairs'):
        assert before[key] == after[key], key
    result = dict(originalSourcePortfolioSha256=compiled['reportSha256'],
        originalInputFiles={p.name:sha256(p.read_bytes()).hexdigest() for p in (
            inputs/'compiled-current.private.json', inputs/'issued-reference-model.private.json',
            args.study_root/'larger-order-win-search-001.private.json')},
        physicalOrderQuantity=cap, originalLargeLegs=len(original), serialLegs=len(serial),
        originalConditionalFutureLegs=source_horizon,
        researchConditionalFutureLegs=financial['conditionalFutureLegs'],
        everySliceFitsPhysicalQuantity=True, exactFinalWealthAndCashAndModelScoresUnchanged=True,
        sharedDepthConsumedOnce=True, independentAudit=after, serialGroups=groups,
        livePortfolioOrLiquidityClaimed=False, nativeCampaignApproved=False,
        executionAuthority=False, newAccountClients=0, actualProceedsCredited=False,
        calibratedProbabilities=False, profitOrWinningEstablished=False,
        note='Order size alone does not rule out a dated multi-tranche hypothesis. Every live leg would require new owner approval/funding/account/depth reconciliation; old model probabilities and opposing marked wealth are uncalibrated assumptions.')
    with args.output.open('x') as file:
        json.dump(result, file, indent=2)
    print(json.dumps({k:result[k] for k in ('originalLargeLegs','serialLegs',
        'exactFinalWealthAndCashAndModelScoresUnchanged','nativeCampaignApproved','executionAuthority')}))


if __name__ == '__main__':
    main()
