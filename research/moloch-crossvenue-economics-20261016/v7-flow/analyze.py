"""Prospectively frozen optional-flow research policy. No execution authority."""
import argparse
from collections import Counter
from decimal import Decimal
import hashlib
import math
from pathlib import Path
import sys

if '--reader-directory' in sys.argv:
    READER = Path(sys.argv[sys.argv.index('--reader-directory')+1]).resolve()
else:
    READER = Path(__file__).resolve().parents[3]/'moloch'/'external-data'
sys.path.insert(0, str(READER))
import crossvenue_economics as e
import crossvenue_observer as o

PRODUCER = '674264745ced43b053881ad7408a5031a6e389c8652ff950b23bca0e1448b34d'
MODELS = {'sigLocal': (4, 5), 'sigLocalPlusFlow': (4, 5, 7, 8, 9),
          'originalSeven': tuple(range(7)), 'originalSevenPlusFlow': tuple(range(10))}
POLICY = {'version': 1, 'producerSha256': PRODUCER, 'sampleIdFormat': 'scoped_v2',
          'features': list(e.FEATURES)+['signedFlowOverOnePlusObservedQuantity1s', 'signedFlowOverOnePlusObservedQuantity5s', 'signedFlowOverOnePlusObservedQuantity30s'],
          'comparators': {'unchangedSigBbo': 'negative original spread minus costs', **{k: list(v) for k, v in MODELS.items()}},
          'ridgePenalty': 1, 'trainingMinimumClusters': 16, 'minimumOosClusters': 100,
          'minimumSelectedClusters': 100, 'folds': 3, 'labelSlackMs': 3000,
          'feePerShareRoundTrip': '0', 'bufferPerShare': '0.005', 'predictionThresholdNet': 0,
          'maximumFlowSnapshotAgeMs': 2000, 'requireAllThreeIdentifiedFlowWindows': True,
          'clusterOrder': 'original price/label valid nonoverlap clusters then causal flow eligibility; identical for all models',
          'missingImputation': False, 'makerFillInference': False, 'fitOrThresholdSearch': False}


def flow_vector(row):
    cached, at = row.get('externalTradeFlow'), row['decisionMs']
    if cached is None:
        return [None]*3, ['missingSnapshot']*3
    e.require(isinstance(cached, dict), 'FLOW_CACHE')
    s = cached['sourceSnapshot']
    e.require(cached.get('pricingAuthority') is False and cached.get('executionAuthority') is False and
              cached.get('decisionMs') == at and type(cached.get('availableMs')) is int and
              s['asOfMs'] <= cached['availableMs'] <= at and
              cached.get('snapshotAgeMs') == at-s['asOfMs'] and 0 <= at-s['asOfMs'] <= 2000, 'FLOW_CLOCK')
    e.require(cached.get('sourceSnapshotSha256') == hashlib.sha256(o.compact(s).encode()).hexdigest(), 'FLOW_SOURCE_HASH')
    e.require(s['ticker'] == row['external']['ticker'], 'FLOW_SCOPE')
    o.validate_trade_flow(s, s['ticker'], s['asOfMs'])
    values, reasons = [], []
    for lag in (1000, 5000, 30000):
        w, supplied = s['windows'][str(lag)], cached['inputs'][str(lag)]
        e.require(supplied.get('completeTradeTape') is False and supplied.get('validatedEvidenceComplete') is w['evidenceComplete'], 'FLOW_COVERAGE')
        rows = [r for r in w['evidence'] if at-lag < r['sourceTsMs'] <= at]
        reason = ('partialEvidence' if not w['evidenceComplete'] else 'missingExecutionClock' if w['missingExecutionClockCount'] else
                  'noObservedExecution' if not rows else 'unknownDirection' if any(r['direction'] is None for r in rows) else None)
        if reason:
            e.require(supplied.get('observedSignedQuantity') is None, 'FLOW_UNIDENTIFIED_SIGN')
            values.append(None); reasons.append(reason)
            continue
        signed = sum((o.decimal(r['quantity'])*r['direction'] for r in rows), Decimal(0))
        gross = sum((o.decimal(r['quantity']) for r in rows), Decimal(0))
        e.require(e.signed_money(supplied['observedSignedQuantity']) == signed and supplied.get('observedTradeCount') == len(rows), 'FLOW_AGGREGATE')
        values.append(float(signed/(1+gross))); reasons.append(None)
    return values, reasons


def fit(train, side, columns):
    x = [[r['x'][j] for j in columns] for r in train]
    means = [sum(row[j] for row in x)/len(x) for j in range(len(columns))]
    scales = [max(1e-9, math.sqrt(sum((row[j]-means[j])**2 for row in x)/len(x))) for j in range(len(columns))]
    design = [[1.0]+[(v-m)/s for v, m, s in zip(row, means, scales)] for row in x]
    dim = len(columns)+1
    a = [[sum(row[i]*row[j] for row in design)+(1.0 if i == j and i else 0.0) for j in range(dim)] for i in range(dim)]
    b = [sum(row[i]*r[side] for row, r in zip(design, train)) for i in range(dim)]
    return {'means': means, 'scales': scales, 'coefficients': e.solve(a, b), 'columns': columns}


def prediction(model, row):
    x = [row['x'][j] for j in model['columns']]
    return e.predict(model, x)


def compare(rows, decisions):
    times = sorted({r['at'] for r in rows}); scored = []; folds = []
    if len(times) >= 4:
        boundaries = [times[len(times)*i//4] for i in (1, 2, 3)]
        for k, start in enumerate(boundaries):
            stop = boundaries[k+1] if k < 2 else math.inf
            train = [r for r in rows if r['end'] < start and r['available'] <= start]
            test = [r for r in rows if start <= r['at'] < stop]
            folds.append({'testStartMs': start, 'testEndExclusiveMs': None if math.isinf(stop) else stop,
                          'trainingClusters': len(train), 'testClusters': len(test), 'trained': len(train) >= 16,
                          'trainingSampleIdsSha256': e.digest([r['id'] for r in train])})
            if len(train) < 16: continue
            models = {name: {side: fit(train, side, cols) for side in ('yes', 'no')} for name, cols in MODELS.items()}
            for r in test:
                b = decisions[r['id']]['sig']['features']
                bbo = -float(e.money(b['ask'])-e.money(b['bid'])+Decimal('.005'))
                forecasts = {'unchangedSigBbo': {'yes': bbo, 'no': bbo},
                             **{name: {s: prediction(models[name][s], r) for s in ('yes', 'no')} for name in MODELS}}
                scored.append((r, forecasts))
    results = {}
    for name in ('unchangedSigBbo', *MODELS):
        errors, chosen, net = [], [], Decimal(0)
        for r, forecasts in scored:
            estimates = forecasts[name]
            errors.extend((r[s]-estimates[s])**2 for s in ('yes', 'no'))
            side = max(('yes', 'no'), key=lambda s: estimates[s])
            if estimates[side] > 0:
                chosen.append(r['id']); net += Decimal(r[side+'Exact'])
        results[name] = {'mse': sum(errors)/len(errors) if errors else None, 'outOfSampleClusters': len(scored),
                         'selectedClusters': len(chosen), 'selectedNetCrossingReturnSumExact': str(net),
                         'selectedSampleIdsSha256': e.digest(chosen), 'researchCoverageSufficient': len(scored) >= 100 and len(chosen) >= 100,
                         'abstain': True, 'executionAuthority': False}
    return {'models': results, 'folds': folds, 'outOfSampleSampleIdsSha256': e.digest([r['id'] for r, _ in scored])}


def analyze(paths, as_of, producer):
    e.require(hashlib.sha256(Path(producer).read_bytes()).hexdigest() == PRODUCER, 'PRODUCER_SOURCE_HASH')
    d, labels, files, counts = e.load(paths, as_of)
    complete, reasons = {}, Counter(); nonzero = {str(h): Counter() for h in (1000, 5000, 30000)}
    for sid, row in d.items():
        if row['external'].get('mappingKind') != 'direct_party_reference': continue
        vector, why = flow_vector(row)
        for lag, reason in zip((1000,5000,30000), why):
            if reason: reasons[str(lag)+':'+reason] += 1
        for lag, value in zip((1000,5000,30000), vector):
            if value is not None: nonzero[str(lag)]['nonzero' if value != 0 else 'zero'] += 1
        if all(v is not None for v in vector):
            complete[sid] = vector
    horizons = {}
    for h in e.HORIZONS:
        rows, diagnostic = e.examples(d, labels, h, as_of, 3000, Decimal(0), Decimal('.005'), 'direct_party_reference', 'scoped_v2')
        qualified = [{**r, 'x': r['x']+complete[r['id']]} for r in rows if r['id'] in complete]
        horizons[str(h)] = {**compare(qualified, d), 'originalPriceCompleteClusterDiagnostics': diagnostic,
                            'flowCompleteClusters': len(qualified), 'excludedForMissingFlowClusters': len(rows)-len(qualified)}
    return {'schemaVersion': 1, 'status': 'UNQUALIFIED_PREDECLARED_FLOW_RESEARCH', 'asOfMs': as_of,
            'policy': POLICY, 'policySha256': e.digest(POLICY), 'producerSha256': PRODUCER,
            'sourceCodeSha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            'readerSha256': {name: hashlib.sha256((READER/name).read_bytes()).hexdigest() for name in ('crossvenue_economics.py','crossvenue_observer.py')},
            'sources': files, 'sourceBundleSha256': e.digest(files), 'input': dict(counts),
            'completeFlowDecisions': len(complete), 'missingFlowReasons': dict(reasons),
            'identifiedFlowWindowVariation': {k: dict(v) for k, v in nonzero.items()}, 'horizons': horizons,
            'modelApproval': False, 'profitEstablished': False, 'executionAuthority': False,
            'limitations': ['Same full price+identified flow complete-case cohort for every comparator; no imputation.',
                           'Missing flow/labels may be selective and market/time clusters cross-correlated.',
                           'Observed public trades are not maker fills or a complete trade tape.',
                           'Fixed comparisons, no parameter/threshold search; no prospective financial approval.']}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--reader-directory', type=Path)
    p.add_argument('--input', nargs='+', type=Path, required=True)
    p.add_argument('--as-of', required=True)
    p.add_argument('--producer-source', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    result = analyze(a.input, e.clock(a.as_of), a.producer_source)
    with a.output.open('x', encoding='utf-8') as f: f.write(e.canonical(result)+'\n')


if __name__ == '__main__': main()
