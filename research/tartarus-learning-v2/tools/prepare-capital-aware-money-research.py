"""Compare fresh matched standalone/rotation research and prepare one nomination.

Run against inputs from the existing sole-worker capture. This never publishes
an approval or submits an order. All original clocks and preparer audits apply.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
from types import SimpleNamespace
import time
from capital_aware_money_selection import select


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def run(a):
    os.umask(0o077)
    a.output.mkdir(parents=True, exist_ok=False, mode=0o700)
    compiled = json.loads((a.inputs/'compiled-current.private.json').read_bytes())
    assert compiled['financial'] is not None and not compiled['unavailableReasons']
    common = dict(source=a.source, inputs=a.inputs, minimum_expected_money=a.minimum_expected_money)
    direct_path = a.output/'direct.public.json'
    direct = module('capital_direct', a.tools/'scan-current-money-opportunities.py').run(
        SimpleNamespace(**common, output=direct_path))
    rotations = module('capital_rotations', a.tools/'scan-current-money-rotations.py').run(
        SimpleNamespace(**common, output=a.output/'rotations.public.json',
                        audit_limit=a.audit_limit, maximum_candidates=a.audit_limit))
    for result in (direct, rotations):
        assert result['sourcePortfolioSha256'] == compiled['reportSha256']
    chosen, deferred = select(direct['candidates'], rotations['candidates'],
                              compiled['managedOwnership'], a.minimum_expected_money)
    result = dict(selection=chosen, deferred=deferred, accountClientsCreated=0,
                  approvalPublished=False, submitted=False, sourceClocksRenewed=False,
                  currentExecutableLiquidityClaimed=False, calibratedProbabilityClaimed=False)
    # Persist the comparison before preparation, including reasons for no trade.
    (a.output/'comparison.public.json').write_text(json.dumps(result, indent=2)+'\n')
    if chosen is not None:
        shared = dict(**common, output=a.output/'prepared',
                      decision_ms=time.time_ns()//1000000, horizon_ms=a.horizon_ms)
        if chosen['kind'] == 'direct':
            leg = chosen['candidate']['leg']
            selected_path = a.output/'selected-direct.public.json'
            selected_path.write_text(json.dumps(dict(direct, candidates=[chosen['candidate']]))+'\n')
            module('capital_prepare_direct', a.tools/'prepare-current-money-nomination.py').run(
                SimpleNamespace(**shared, candidates=selected_path, exchange=leg['ex'],
                                side=leg['side'], action=leg['action'], price_tolerance=a.price_tolerance))
        else:
            plan = a.output/'rotation-plan.json'
            plan.write_text(json.dumps(chosen['candidate'], indent=2)+'\n')
            module('capital_prepare_rotation', a.tools/'prepare-current-money-rotation.py').run(
                SimpleNamespace(**shared, plan=plan))
        result['prepared'] = json.loads((a.output/'prepared/prepared.public.json').read_bytes())
        result['state'] = 'fresh_unapproved_capital_aware_nomination_prepared'
    else:
        result['state'] = 'no_qualifying_managed_capital_allocation'
    (a.output/'result.public.json').write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps(result), flush=True)
    return result


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('source', 'tools', 'inputs', 'output'):
        p.add_argument('--'+name, type=Path, required=True)
    p.add_argument('--minimum-expected-money', default='0.010000')
    p.add_argument('--price-tolerance', default='0.000000')
    p.add_argument('--horizon-ms', type=int, default=300000)
    p.add_argument('--audit-limit', type=int, default=256)
    args = p.parse_args()
    assert 1 <= args.audit_limit <= 10000
    run(args)
