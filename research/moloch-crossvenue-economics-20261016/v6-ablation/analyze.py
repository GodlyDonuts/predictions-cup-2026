"""Predeclared fixed v6 ablations; exploratory after prior results, never approval."""
import argparse
from collections import Counter
from decimal import Decimal
import hashlib
import math
from pathlib import Path
import sys

# Local repository or explicit private read-only reader copy. No installed package.
if '--reader-directory' in sys.argv:
    READER = Path(sys.argv[sys.argv.index('--reader-directory')+1]).resolve()
else:
    READER = Path(__file__).resolve().parents[3]/'moloch'/'external-data'
sys.path.insert(0, str(READER))
import crossvenue_economics as e
import crossvenue_toxicity as t

POLICY = {'comparison': ['unchanged_SIG_BBO', 'SIG_local_imbalance_spread_ridge', 'original_seven_feature_ridge'],
          'ridgePenalty': 1, 'folds': 3, 'labelSlackMs': 3000, 'fee': '0', 'buffer': '0.005',
          'minimumClusters': 100, 'fitOrThresholdTuning': False,
          'status': 'EXPLORATORY_AFTER_PRIOR_V6_RESULTS'}


def ablated_rows(rows):
    # Deliberate exclusion of five columns, not imputation of missing observations.
    # Original fit uses the same seven-column objective; inactive zero columns
    # have zero RHS/cross-products and ridge diagonal1, exactly the2-column fit.
    return [{**r, 'x': [0, 0, 0, 0, r['x'][4], r['x'][5], 0]} for r in rows]


def cohort(rows, result):
    selected = []
    for fold in result['folds']:
        if fold['trained']:
            selected += [r for r in rows if fold['testStartMs'] <= r['at'] and
                         (fold['testEndExclusiveMs'] is None or r['at'] < fold['testEndExclusiveMs'])]
    e.require(e.digest([r['id'] for r in selected]) == result['outOfSampleSampleIdsSha256'], 'ABLATION_COHORT')
    return selected


def compare(rows, decisions):
    full, local = e.walk_forward(rows), e.walk_forward(ablated_rows(rows))
    e.require(full['outOfSampleSampleIdsSha256'] == local['outOfSampleSampleIdsSha256'], 'ABLATION_COHORT')
    scored = cohort(rows, full)
    errors, selected = [], []
    for r in scored:
        b = decisions[r['id']]['sig']['features']
        prediction = -(e.money(b['ask'])-e.money(b['bid']))-Decimal(POLICY['fee'])-Decimal(POLICY['buffer'])
        errors += [(r[s]-float(prediction))**2 for s in ('yes', 'no')]
        if prediction > 0:
            selected.append(r)
    bbo = {'outOfSampleClusters': len(scored), 'selectedClusters': len(selected),
           'selectedNetCrossingReturnSumExact': str(sum((Decimal(r['yesExact']) for r in selected), Decimal(0))),
           'mse': sum(errors)/len(errors) if errors else None}
    def compact(result):
        return {'outOfSampleClusters': result['outOfSampleClusters'], 'selectedClusters': result['selectedClusters'],
                'selectedNetCrossingReturnSumExact': result['selectedNetCrossingReturnSumExact'],
                'mse': result['modelMseNetReturn'], 'interceptMse': result['interceptBaselineMseNetReturn'],
                'selectedSampleIdsSha256': result['selectedSampleIdsSha256'], 'abstain': result['abstain']}
    f, l = compact(full), compact(local)
    def delta(a, b): return a-b if a is not None and b is not None else None
    return {'outOfSampleSampleIdsSha256': full['outOfSampleSampleIdsSha256'],
            'unchangedSigBbo': bbo, 'sigLocalRidge': l, 'fullRidge': f,
            'fullMseMinusSigLocalMse': delta(f['mse'], l['mse']),
            'fullMseMinusUnchangedBboMse': delta(f['mse'], bbo['mse']),
            'folds': [{k: v for k, v in fold.items() if k != 'modelsResearchOnly'} for fold in full['folds']],
            'actualFill': False, 'executionAuthority': False}


def innovations(decisions):
    values = {str(h): [] for h in (1000, 5000, 30000)}
    missing, states, sources = Counter(), Counter(), Counter()
    for row in decisions.values():
        if row['external'].get('mappingKind') != 'direct_party_reference':
            continue
        e.feature_vector(row, 'scoped_v2')  # Causality/anchors even on incomplete rows.
        state, reason = t.feature_state(row, 'scoped_v2')
        if reason: states[reason] += 1
        else:
            for side, value in state.items(): states[side+'_'+value] += 1
        sources[row['external'].get('source')] += 1
        for lag in values:
            value = row['externalLogitChanges'].get(lag)
            if value is None: missing[lag] += 1
            else: values[lag].append(Decimal(str(e.number(value))))
    return {'sources': dict(sources), 'fixedJointThresholdStates': dict(states),
            'lags': {h: {'missing': missing[h], 'available': len(v), 'zero': sum(x == 0 for x in v),
                         'nonzero': sum(x != 0 for x in v), 'sumAbsolute': str(sum(map(abs, v), Decimal(0))),
                         'meanAbsolute': str(sum(map(abs, v), Decimal(0))/len(v)) if v else None,
                         'maxAbsolute': str(max(map(abs, v))) if v else None,
                         'minimum': str(min(v)) if v else None, 'maximum': str(max(v)) if v else None}
                     for h, v in values.items()}}


def analyze(paths, as_of, producer, expected_sha):
    e.require(Path(producer).is_file() and hashlib.sha256(Path(producer).read_bytes()).hexdigest() == expected_sha, 'PRODUCER_SOURCE_HASH')
    decisions, labels, files, counts = e.load(paths, as_of)
    results = {}
    for h in e.HORIZONS:
        rows, data = e.examples(decisions, labels, h, as_of, 3000, Decimal(0), Decimal('.005'), 'direct_party_reference', 'scoped_v2')
        results[str(h)] = {**compare(rows, decisions), 'data': data}
    return {'schemaVersion': 1, 'status': 'EXPLORATORY_AFTER_PRIOR_V6_RESULTS', 'asOfMs': as_of,
            'policy': POLICY, 'policySha256': e.digest(POLICY), 'sampleIdFormat': 'scoped_v2',
            'sourceCodeSha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            'readerSha256': {name: hashlib.sha256((READER/name).read_bytes()).hexdigest() for name in ('crossvenue_economics.py', 'crossvenue_toxicity.py')},
            'producerSha256': expected_sha, 'sources': files, 'sourceBundleSha256': e.digest(files),
            'input': dict(counts), 'decisions': len(decisions), 'labels': len(labels),
            'innovationDiagnostics': innovations(decisions), 'horizons': results,
            'modelApproval': False, 'profitEstablished': False, 'executionAuthority': False,
            'caveats': ['Same complete-case cluster cohort and causal purged folds for all comparators.',
                        'SIG spread mechanically predicts negative crossing cost absent any price movement.',
                        'External full-model MSE improvement does not identify causal Kalshi price discovery.',
                        'Missing labels/features may be selective; market/time clusters remain cross-market correlated.',
                        'No maker fills, fill probability or account PnL; no tuning or prospective approval.']}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--reader-directory', type=Path)
    p.add_argument('--input', nargs='+', type=Path, required=True)
    p.add_argument('--as-of', required=True)
    p.add_argument('--producer-source', type=Path, required=True)
    p.add_argument('--producer-sha256', required=True)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    result = analyze(a.input, e.clock(a.as_of), a.producer_source, a.producer_sha256)
    with a.output.open('x', encoding='utf-8') as f: f.write(e.canonical(result)+'\n')


if __name__ == '__main__': main()
