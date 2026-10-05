"""Fixed horizon comparison on causal, purged endpoint labels; no order client."""
import argparse
import datetime
import hashlib
import importlib.util
import json
import os
import time
from pathlib import Path

import numpy as np
_spec = importlib.util.spec_from_file_location('baal_microprice_training', Path(__file__).with_name('train-realtime-microprice.py'))
_training = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_training)
fit, predict, evaluate = _training.fit, _training.predict, _training.evaluate

VALIDATION_AT = '2026-10-05T12:51:00.000Z'
TEST_AT = '2026-10-05T14:37:00.000Z'
ALPHAS = [0.0001, 0.001, 0.01, 0.1, 1, 10]


def score(model, rows, margins):
    metrics, _ = evaluate(model, rows, margins)
    metrics.pop('signals')
    predictions = predict(model, rows)
    taker_signals = []
    for row, prediction in zip(rows, predictions):
        for side in (0, 1):
            # Freeze the direction and cost gate at the decision. The separately
            # observed entry price supplies the endpoint diagnostic after latency.
            decision_ask = row['ask'] if side == 0 else 200-row['bid']
            fair = row['mid'] if side == 0 else 200-row['mid']
            expected_net = fair+prediction[side]-margins[side]-1-(decision_ask+1)
            if expected_net >= 1:
                realized = row['yesTakerNetTicks'] if side == 0 else row['noTakerNetTicks']
                taker_signals.append({'at': row['at'], 'marketId': row['marketId'],
                                     'side': 'YES' if side == 0 else 'NO',
                                     'endpointNetTicks': realized,
                                     'pathGapCount': row['pathGapCount']})
    metrics['takerForecastSignals'] = len(taker_signals)
    metrics['takerSignalMarkets'] = len({r['marketId'] for r in taker_signals})
    metrics['conditionalTakerMeanTicks'] = float(np.mean([r['endpointNetTicks'] for r in taker_signals])) if taker_signals else None
    metrics['conditionalTakerPositiveFraction'] = float(np.mean([r['endpointNetTicks'] > 0 for r in taker_signals])) if taker_signals else None
    metrics['labelsAcrossGaps'] = sum(r['pathGapCount'] > 0 for r in rows)
    # Report coverage strata rather than silently hiding the outage subset.
    metrics['pathStrata'] = []
    for missing in (False, True):
        subset = [r for r in rows if (r['pathGapCount'] > 0) == missing]
        if subset:
            submetrics, _ = evaluate(model, subset, margins)
            submetrics.pop('signals')
            metrics['pathStrata'].append({'pathObserved': not missing, **submetrics})
    return metrics


def compare(filename):
    raw = open(filename, 'rb').read()
    data = json.loads(raw)
    policy = data['policy']
    if data['schemaVersion'] != 2 or policy['feeTicks'] != 0 or policy['latencyMs'] != 3000 or policy.get('endpointResearchAcrossGaps') is not True:
        raise ValueError('Expected explicitly marked causal endpoint research')
    for row in data['rows']:
        if not row['at'] < row['entryAt'] < row['labelAt'] or len(row['x']) != len(data['features']):
            raise ValueError('Noncausal entry or label')
        if row['pathGapCount'] < 0 or row['endpointOnlyResearch'] is not True:
            raise ValueError('Missing endpoint uncertainty')
    train = [r for r in data['rows'] if r['labelAt'] < VALIDATION_AT]
    validation = [r for r in data['rows'] if r['at'] >= VALIDATION_AT and r['labelAt'] < TEST_AT]
    test = [r for r in data['rows'] if r['at'] >= TEST_AT]
    result = {'dataset': filename, 'datasetSha256': hashlib.sha256(raw).hexdigest(),
              'policy': policy, 'features': data['features'], 'counts': data['counts'],
              'through': data['through'], 'unresolved': data['unresolved'],
              'trainingSamples': len(train), 'validationSamples': len(validation), 'testSamples': len(test),
              'validationStartsAt': VALIDATION_AT, 'testStartsAt': TEST_AT,
              'executionAuthority': False, 'profitabilityEstablished': False}
    if min(len(train), len(validation), len(test)) < 100:
        return {**result, 'status': 'insufficient_purged_chronological_labels',
                'reason': 'Fixed chronology has too few matured labels; no boundary or cost gate was relaxed.'}
    candidates = []
    for alpha in ALPHAS:
        model = fit(train, alpha)
        metrics, _ = evaluate(model, validation)
        metrics.pop('signals')
        candidates.append({'model': model, 'validation': metrics})
    selected = min(candidates, key=lambda c: c['validation']['rmseTicks'])
    model = selected['model']
    _, residual = evaluate(model, validation)
    margins = np.maximum(0, -np.quantile(residual, 0.05, axis=0))
    return {**result, 'status': 'research_comparison_completed', 'model': model, 'candidates': candidates,
            'trainingLabelMaxAt': max(r['labelAt'] for r in train),
            'validationLabelMaxAt': max(r['labelAt'] for r in validation),
            'lowerForecastErrorTicks': margins.tolist(), 'validation': score(model, validation, margins),
            'test': score(model, test, margins)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--datasets', nargs='+', required=True)
    parser.add_argument('--out', required=True)
    args = parser.parse_args()
    if os.path.exists(args.out):
        raise ValueError('Frozen comparison exists')
    started = time.monotonic()
    comparisons = [compare(f) for f in args.datasets]
    report = {'schemaVersion': 1, 'frozenAt': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'mode': 'fixed_horizon_endpoint_reanalysis', 'comparisons': comparisons,
              'fitSeconds': time.monotonic()-started, 'networkRequests': 0, 'competitionOrders': 0,
              'executionAuthority': False, 'profitabilityEstablished': False,
              'limitations': ['These sealed captures were already used for the earlier five-minute investigation; this is development reanalysis, not new prospective performance.',
                              'Endpoint returns and overlapping forecast samples are not funded P&L or evidence of passive fills. Unlabelled decisions may cause selection bias.',
                              'Outage paths, intrahorizon stops and settlement transitions cannot be inferred from endpoint labels.',
                              'Validation residual quantiles are empirical error gates, not guaranteed loss bounds; samples and election exposures are correlated.',
                              'Promoting a model requires independent subsequent source data and exact attributed fills/accounting.']}
    with open(args.out, 'x', encoding='utf-8') as handle:
        json.dump(report, handle, indent=2)
    print(json.dumps({'fitSeconds': report['fitSeconds'], 'comparisons': [
        {k: c.get(k) for k in ['policy', 'status', 'trainingSamples', 'validationSamples', 'testSamples', 'lowerForecastErrorTicks', 'validation', 'test']}
        for c in comparisons]}, indent=2))


if __name__ == '__main__':
    main()
