"""Small regularized market-only models; no network, account state or order API."""
import argparse, datetime, hashlib, json, os, time
import numpy as np

def matrix(rows):
    x = np.asarray([r['x'] for r in rows], dtype=float)
    y = np.asarray([[r['futureBidTicks'] - r['mid'], r['mid'] - r['futureAskTicks']] for r in rows], dtype=float)
    return x, y

def fit(rows, alpha):
    x, y = matrix(rows)
    means, scales = x.mean(axis=0), np.maximum(x.std(axis=0), 1e-6)
    z = np.column_stack([np.ones(len(x)), np.clip((x-means)/scales, -12, 12)])
    penalty = np.eye(z.shape[1]) * alpha
    penalty[0, 0] = 0
    coefficients = np.linalg.solve(z.T @ z / len(z) + penalty, z.T @ y / len(z))
    return {'alpha': alpha, 'means': means.tolist(), 'scales': scales.tolist(), 'coefficients': coefficients.tolist(), 'normalizedClip': 12}

def predict(model, rows):
    x, _ = matrix(rows)
    z = np.column_stack([np.ones(len(x)), np.clip((x-np.asarray(model['means']))/np.asarray(model['scales']), -12, 12)])
    return z @ np.asarray(model['coefficients'])

def evaluate(model, rows, error_margin=None):
    predictions = predict(model, rows)
    _, y = matrix(rows)
    residual = y-predictions
    # A fixed one-tick-inside passive limit, only where a later liquidation
    # forecast beats the purchase plus an adverse exit tick. No fill is inferred.
    margin = np.zeros(2) if error_margin is None else np.asarray(error_margin)
    signals = []
    for r, p in zip(rows, predictions):
        mid, bid, ask = r['mid'], r['bid'], r['ask']
        for side in (0, 1):
            limit = bid+1 if side == 0 else 200-ask+1
            is_passive = limit < (ask if side == 0 else 200-bid)
            fair = mid if side == 0 else 200-mid
            lower_mark = fair+p[side]-margin[side]-1
            if is_passive and lower_mark-limit >= 1:
                realized_mark = (r['futureBidTicks'] if side == 0 else 200-r['futureAskTicks'])-1-limit
                signals.append({'at': r['at'], 'marketId': r['marketId'], 'side': 'YES' if side == 0 else 'NO', 'predictedLowerMarkoutTicks': float(lower_mark-limit), 'counterfactualConditionalMarkoutTicks': realized_mark})
    # Overlapping samples are retained for prediction diagnostics. No aggregate
    # economic sum is interpreted as profit or a deployable funded wallet.
    persistence = np.asarray([[r['bid']-r['mid'], r['mid']-r['ask']] for r in rows], dtype=float)
    return {'samples': len(rows), 'markets': len({r['marketId'] for r in rows}), 'rmseTicks': float(np.sqrt(np.mean(residual**2))),
            'persistenceRmseTicks': float(np.sqrt(np.mean((y-persistence)**2))), 'meanResidualTicks': residual.mean(axis=0).tolist(),
            'passiveForecastSignals': len(signals), 'signalMarkets': len({r['marketId'] for r in signals}),
            'conditionalMeanMarkoutTicks': float(np.mean([s['counterfactualConditionalMarkoutTicks'] for s in signals])) if signals else None,
            'signals': signals}, residual

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--dataset', required=True)
    parser.add_argument('--out', required=True)
    args = parser.parse_args()
    if os.path.exists(args.out):
        raise ValueError('Refusing to overwrite frozen model evidence')
    started = time.monotonic()
    raw = open(args.dataset, 'rb').read()
    data = json.loads(raw)
    if data['schemaVersion'] != 2 or data['policy']['feeTicks'] != 0 or data['policy']['latencyMs'] != 3000:
        raise ValueError('Expected causal latency/depth zero-fee dataset')
    validation_at = '2026-10-05T12:51:00.000Z'
    test_at = '2026-10-05T14:37:00.000Z'
    train = [r for r in data['rows'] if r['labelAt'] < validation_at]
    validation = [r for r in data['rows'] if r['at'] >= validation_at and r['labelAt'] < test_at]
    test = [r for r in data['rows'] if r['at'] >= test_at]
    if min(len(train), len(validation), len(test)) < 100:
        raise ValueError('Insufficient chronological labels')
    for r in data['rows']:
        if not (r['at'] < r['entryAt'] < r['labelAt']) or len(r['x']) != len(data['features']):
            raise ValueError('Invalid label chronology or features')
    candidates = []
    for alpha in [0.0001, 0.001, 0.01, 0.1, 1, 10]:
        model = fit(train, alpha)
        metrics, _ = evaluate(model, validation)
        candidates.append({'model': model, 'validation': {k:v for k,v in metrics.items() if k != 'signals'}})
    selected = min(candidates, key=lambda r: r['validation']['rmseTicks'])
    model = selected['model']
    # Only validation residuals set forecast uncertainty; held-out observations
    # never set normalization, regularization, feature selection or thresholds.
    _, residual = evaluate(model, validation)
    margins = np.maximum(0, -np.quantile(residual, 0.05, axis=0))
    validation_score, _ = evaluate(model, validation, margins)
    test_score, _ = evaluate(model, test, margins)
    bank = {'schemaVersion': 1, 'mode': 'realtime_microprice_forecast_research', 'frozenAt': datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'features': data['features'], 'policy': data['policy'], 'model': model, 'lowerForecastErrorTicks': margins.tolist(),
            'datasetSha256': hashlib.sha256(raw).hexdigest(), 'sources': data['sources'],
            'trainingSamples': len(train), 'validationSamples': len(validation), 'testSamples': len(test),
            'validationStartsAt': validation_at, 'testStartsAt': test_at, 'trainingLabelMaxAt': max(r['labelAt'] for r in train),
            'validationLabelMaxAt': max(r['labelAt'] for r in validation), 'validation': validation_score, 'test': test_score,
            'candidates': candidates, 'fitSeconds': time.monotonic()-started, 'executionAuthority': False, 'profitabilityEstablished': False,
            'limitations': ['Prediction quality and conditional markouts do not prove a fill or funded profitability.',
                            'Market samples overlap and election markets are correlated; empirical residual quantiles are not guaranteed risk bounds.',
                            'Only subsequent frozen prospective fills/accounting can support activation.']}
    with open(args.out, 'x', encoding='utf-8') as f:
        json.dump(bank, f, indent=2)
    print(json.dumps({k:bank[k] for k in ['trainingSamples','validationSamples','testSamples','fitSeconds','lowerForecastErrorTicks']}))
    print(json.dumps({'selectedAlpha': model['alpha'], 'validation': {k:v for k,v in validation_score.items() if k != 'signals'},
                      'test': {k:v for k,v in test_score.items() if k != 'signals'}}))

if __name__ == '__main__':
    main()
