"""Verify the actual frozen selector splits and produce Python/JS parity cases."""
import hashlib
import importlib.util
import json
import numpy as np
spec = importlib.util.spec_from_file_location('quote_training', 'live-paper/train-quote-outcomes.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
cases = []
splits = []
for suffix in ['', '-exact-exit']:
    model_file = f'data/realtime-quote{suffix}-selectors-20261005.json'
    dataset_file = f'data/realtime-quote{suffix}-dataset-20261005.json'
    report = json.load(open(model_file))
    raw = open(dataset_file, 'rb').read()
    assert hashlib.sha256(raw).hexdigest() == report['datasetSha256']
    data = json.loads(raw)
    for bank_index, bank in enumerate(report['banks']):
        supported = [r for r in data['rows'] if r['policyId'] == bank['policy']['id'] and r['kind'] in ('paired', 'marked', 'unfilled') and not r['gapExposed']]
        train = [r for r in supported if r['resolvedAt'] < module.VALIDATION_AT]
        validation = [r for r in supported if r['createdAt'] >= module.VALIDATION_AT and r['resolvedAt'] < module.TEST_AT]
        test = [r for r in supported if r['createdAt'] >= module.TEST_AT]
        assert [len(train), len(validation), len(test)] == [bank['trainingSamples'], bank['validationSamples'], bank['testSamples']]
        assert max(r['resolvedAt'] for r in train) < min(r['createdAt'] for r in validation)
        assert max(r['resolvedAt'] for r in validation) < min(r['createdAt'] for r in test)
        assert all(r['decisionSource']['observedAt'] == r['createdAt'] < r['resolvedAt'] for r in supported)
        x = np.asarray([r['x'] for r in train], dtype=float)
        assert np.isfinite(x).all()
        filled = [r for r in train if r['tradeSources']]
        np.testing.assert_allclose(x.mean(axis=0), bank['model']['arrival']['means'], atol=1e-12)
        np.testing.assert_allclose(np.asarray([r['x'] for r in filled]).mean(axis=0), bank['model']['conditionalValue']['means'], atol=1e-12)
        rows = [supported[i*(len(supported)-1)//15] for i in range(16)]
        forecast = module.forecast(bank['model'], rows)
        for i, row in enumerate(rows):
            cases.append({'modelFile': model_file, 'bankIndex': bank_index, 'x': row['x'], 'expected': [float(v[i]) for v in forecast]})
        splits.append({'modelFile': model_file, 'policyId': bank['policy']['id'], 'counts': [len(train), len(validation), len(test)], 'purgedBoundariesVerified': True, 'separateTrainingNormalizationVerified': True})
with open('data/quote-outcome-parity-cases-20261005.json', 'x') as handle:
    json.dump(cases, handle)
with open('data/quote-outcome-splits-20261005.json', 'x') as handle:
    json.dump({'cases': len(cases), 'splits': splits, 'networkRequests': 0, 'competitionOrders': 0}, handle, indent=2)
print(json.dumps({'cases': len(cases), 'splitsVerified': len(splits)}))
