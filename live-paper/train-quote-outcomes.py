"""Arrival probability and conditional inventory value from causal quote outcomes."""
import argparse
import datetime
import hashlib
import json
import os
import time

import numpy as np

VALIDATION_AT = '2026-10-05T12:51:00.000Z'
TEST_AT = '2026-10-05T14:37:00.000Z'
ALPHAS = [0.0001, 0.001, 0.01, 0.1, 1, 10]


def fit(x, y, alpha):
    means, scales = x.mean(axis=0), np.maximum(x.std(axis=0), 1e-6)
    z = np.column_stack([np.ones(len(x)), np.clip((x-means)/scales, -12, 12)])
    penalty = np.eye(z.shape[1])*alpha
    penalty[0, 0] = 0
    coefficients = np.linalg.solve(z.T@z/len(z)+penalty, z.T@y/len(z))
    return {'means': means.tolist(), 'scales': scales.tolist(), 'coefficients': coefficients.tolist(), 'normalizedClip': 12}


def predict(model, rows):
    x = np.asarray([r['x'] for r in rows], dtype=float)
    z = np.column_stack([np.ones(len(x)), np.clip((x-np.asarray(model['means']))/np.asarray(model['scales']), -12, 12)])
    return z@np.asarray(model['coefficients'])


def target(rows):
    return np.asarray([[len(r['tradeSources']) > 0, r['kind'] == 'paired', r['counterfactualNetTicks'] if r['kind'] != 'unfilled' else 0] for r in rows], dtype=float)


def forecast(model, rows):
    arrival = predict(model['arrival'], rows)
    any_fill = np.clip(arrival[:, 0], 0, 1)
    paired = np.clip(arrival[:, 1], 0, any_fill)
    conditional = np.clip(predict(model['conditionalValue'], rows)[:, 0], -200, 199)
    return any_fill, paired, conditional, any_fill*conditional


def evaluate(model, rows):
    any_fill, paired, conditional, value = forecast(model, rows)
    actual = target(rows)
    filled = actual[:, 0] > 0
    selected = (value >= 0.1) & (any_fill >= 0.01)
    return {'samples': len(rows), 'markets': len({r['marketId'] for r in rows}), 'filledOutcomes': int(filled.sum()),
            'pairedOutcomes': int(actual[:, 1].sum()), 'positiveOutcomes': int((actual[:, 2] > 0).sum()),
            'fillBrier': float(np.mean((any_fill-actual[:, 0])**2)), 'pairBrier': float(np.mean((paired-actual[:, 1])**2)),
            'expectedValueRmseTicks': float(np.sqrt(np.mean((value-actual[:, 2])**2))),
            'conditionalValueRmseTicks': float(np.sqrt(np.mean((conditional[filled]-actual[filled, 2])**2))) if filled.any() else None,
            'selectedSupportedOutcomes': int(selected.sum()), 'selectedFilledOutcomes': int((selected & filled).sum()),
            'selectedConditionalTickSum': float(actual[selected, 2].sum()),
            'selectedConditionalMeanTicks': float(actual[selected, 2].mean()) if selected.any() else None,
            'allConditionalTickSum': float(actual[:, 2].sum())}


def train_policy(data, policy):
    rows = [r for r in data['rows'] if r['policyId'] == policy['id']]
    supported = [r for r in rows if r['kind'] in ('paired', 'marked', 'unfilled') and not r['gapExposed']]
    for row in supported:
        if not row['createdAt'] < row['resolvedAt'] or row['decisionSource']['observedAt'] != row['createdAt'] or len(row['x']) != len(data['features']):
            raise ValueError('Noncausal quote feature or label')
    train = [r for r in supported if r['resolvedAt'] < VALIDATION_AT]
    validation = [r for r in supported if r['createdAt'] >= VALIDATION_AT and r['resolvedAt'] < TEST_AT]
    test = [r for r in supported if r['createdAt'] >= TEST_AT]
    result = {'policy': policy, 'allOutcomes': len(rows), 'supportedOutcomes': len(supported), 'censoredOutcomes': len(rows)-len(supported),
              'trainingSamples': len(train), 'validationSamples': len(validation), 'testSamples': len(test),
              'validationStartsAt': VALIDATION_AT, 'testStartsAt': TEST_AT, 'executionAuthority': False}
    training_filled = [r for r in train if r['tradeSources']]
    if min(len(train), len(validation), len(test)) < 100 or len(training_filled) < 20:
        return {**result, 'status': 'insufficient_supported_training_or_chronology', 'trainingFilledOutcomes': len(training_filled)}
    x = np.asarray([r['x'] for r in train], dtype=float)
    y = target(train)
    filled = y[:, 0] > 0
    candidates = []
    for alpha in ALPHAS:
        model = {'alpha': alpha, 'arrival': fit(x, y[:, :2], alpha), 'conditionalValue': fit(x[filled], y[filled, 2:3], alpha)}
        candidates.append({'model': model, 'validation': evaluate(model, validation)})
    selected = min(candidates, key=lambda c: c['validation']['expectedValueRmseTicks'])
    baseline_value = float(y[:, 2].mean())
    actual_test = target(test)
    return {**result, 'status': 'research_selector_frozen', 'model': selected['model'], 'candidates': candidates,
            'trainingFilledOutcomes': len(training_filled), 'trainingResolvedMaxAt': max(r['resolvedAt'] for r in train),
            'validationResolvedMaxAt': max(r['resolvedAt'] for r in validation),
            'validation': selected['validation'], 'test': evaluate(selected['model'], test),
            'trainingMeanValueTicks': baseline_value,
            'testConstantValueRmseTicks': float(np.sqrt(np.mean((actual_test[:, 2]-baseline_value)**2))),
            'selector': {'minimumExpectedNetTicks': 0.1, 'minimumAnyFillProbability': 0.01},
            'profitabilityEstablished': False}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--dataset', required=True)
    parser.add_argument('--out', required=True)
    args = parser.parse_args()
    if os.path.exists(args.out):
        raise ValueError('Frozen selector already exists')
    started = time.monotonic()
    raw = open(args.dataset, 'rb').read()
    data = json.loads(raw)
    if data['mode'] != 'causal_quote_outcome_training_dataset' or any(p['feeTicks'] != 0 or p['latencyMs'] != 3000 for p in data['policies']):
        raise ValueError('Expected fixed zero-fee latency quote dataset')
    report = {'schemaVersion': 1, 'mode': 'quote_fill_conditional_selector_research', 'frozenAt': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'datasetSha256': hashlib.sha256(raw).hexdigest(), 'features': data['features'], 'sources': data['sources'],
              'banks': [train_policy(data, p) for p in data['policies']], 'fitSeconds': time.monotonic()-started,
              'networkRequests': 0, 'competitionOrders': 0, 'executionAuthority': False, 'profitabilityEstablished': False,
              'limitations': ['Arrival and conditional-value forecasts are regularized research estimates, not calibrated risk bounds.',
                              'Supported/correlated overlapping quote outcomes cannot be added as funded profit; censored decisions are excluded from model metrics but remain liabilities in funded replay.',
                              'This development source has been previously examined; selectors require subsequent source and prospective accounting evidence.',
                              'Hypothetical anonymous-trade fills do not prove actual queue execution.']}
    with open(args.out, 'x') as handle:
        json.dump(report, handle, indent=2)
    print(json.dumps({'fitSeconds': report['fitSeconds'], 'banks': [
        {k: b.get(k) for k in ['policy', 'status', 'trainingSamples', 'validationSamples', 'testSamples', 'trainingFilledOutcomes', 'validation', 'test']}
        for b in report['banks']]}, indent=2))


if __name__ == '__main__':
    main()
