import importlib.util
import json
import subprocess

import numpy as np

spec = importlib.util.spec_from_file_location('baal_horizon_comparison', 'live-paper/compare-realtime-horizons.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
report = json.load(open('data/realtime-horizons-models-20261005.json'))
banks = []
for comparison in report['comparisons']:
    if comparison['status'] != 'research_comparison_completed':
        assert comparison['trainingSamples'] == 0 and comparison['validationSamples'] == 0
        continue
    assert comparison['trainingLabelMaxAt'] < comparison['validationStartsAt']
    assert comparison['validationLabelMaxAt'] < comparison['testStartsAt']
    data = json.load(open(comparison['dataset']))
    rows = [r for r in data['rows'] if r['at'] >= comparison['testStartsAt']]
    selected = [rows[i*(len(rows)-1)//31] for i in range(32)]
    expected = module.predict(comparison['model'], selected)
    banks.append({'horizonMs': comparison['policy']['horizonMs'],
                  'bank': {'mode': 'realtime_microprice_forecast_research', 'features': comparison['features'],
                           'model': comparison['model'], 'lowerForecastErrorTicks': comparison['lowerForecastErrorTicks']},
                  'cases': [{'x': r['x'], 'expected': p.tolist()} for r, p in zip(selected, expected)]})
node_script = """
import fs from 'node:fs';import {micropriceForecast} from './live-paper/microprice-forecast.mjs';
const banks=JSON.parse(fs.readFileSync(0,'utf8'));
const results=banks.map(({horizonMs,bank,cases})=>({horizonMs,cases:cases.length,maximumDifferenceTicks:Math.max(...cases.flatMap(({x,expected})=>micropriceForecast(bank,x).bidChangeTicks.map((v,i)=>Math.abs(v-expected[i]))))}));
console.log(JSON.stringify(results));
"""
child = subprocess.run(['node', '--input-type=module', '-e', node_script], input=json.dumps(banks), text=True, capture_output=True, check=True)
results = json.loads(child.stdout)
assert all(r['maximumDifferenceTicks'] < 1e-9 for r in results)
evidence = {'schemaVersion': 1, 'mode': 'actual_label_forecast_parity_and_purge_verification',
            'results': results, 'chronologicalPurgesVerified': True, 'networkRequests': 0,
            'competitionOrders': 0, 'profitabilityEstablished': False}
with open('data/realtime-horizons-parity-20261005.json', 'x') as handle:
    json.dump(evidence, handle, indent=2)
print(json.dumps(evidence))
