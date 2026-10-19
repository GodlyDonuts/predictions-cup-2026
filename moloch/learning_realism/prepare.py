"""Freeze a replay specification and audit actual coverage; never fetch history.

Usage: python -m moloch.learning_realism.prepare --dataset DIR --mechanics FILE
 --competition-start ISO_UTC --cutoff ISO_UTC --output NEW_JSON
"""
import argparse
from collections import defaultdict
from dataclasses import asdict
from datetime import datetime
import hashlib
import json
from pathlib import Path
from moloch.learning_flow.episodes import FlowTape
from moloch.learning_v2.run_experiment import load_mechanics
from .protocol import Split, Delivery,independent_test_claim


def timestamp(value):
    t = datetime.fromisoformat(value.replace('Z', '+00:00'))
    if t.tzinfo is None: raise ValueError('EXPLICIT_TIMEZONE_REQUIRED')
    return int(t.timestamp()*1000)


def coverage(tape, split, maximum_gap_ms=120_000):
    if hasattr(tape,'disk_store'):return tape.disk_store.coverage(split,maximum_gap_ms)
    result = {}
    for role in ('train', 'evolution', 'test'):
        lo, hi = split.bounds(role)
        venues = defaultdict(list); markets = defaultdict(set)
        for row in tape.rows:
            if not lo <= row['available_at_ms'] < hi: continue
            if row['kind'] == 'book': venue = 'SIG'
            elif row['kind'] == 'reference': venue = row['venue']
            else: continue
            venues[venue].append(row['available_at_ms'])
            markets[venue].add(row.get('market_id'))
        by_venue = {}
        for venue in ('SIG', 'Kalshi', 'Polymarket'):
            times = sorted(set(venues[venue]))
            gaps = [b-a for a, b in zip([lo]+times, times+[hi])]
            by_venue[venue] = {'observations': len(venues[venue]), 'markets': len(markets[venue]),
                'first_ms': times[0] if times else None, 'last_ms': times[-1] if times else None,
                'maximum_gap_ms': max(gaps), 'coverage_pass': bool(times) and max(gaps) <= maximum_gap_ms}
        result[role] = {'bounds_ms': [lo, hi], 'venues': by_venue,
                        'coverage_pass': all(r['coverage_pass'] for r in by_venue.values())}
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--dataset', type=Path, required=True)
    p.add_argument('--mechanics', type=Path, required=True)
    p.add_argument('--competition-start', required=True); p.add_argument('--cutoff', required=True)
    p.add_argument('--maximum-gap-ms', type=int, default=120000)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--replay-cache',type=Path)
    p.add_argument('--replay-cache-manifest-sha256')
    args = p.parse_args()
    if args.maximum_gap_ms <= 0: raise ValueError('POSITIVE_COVERAGE_GAP')
    split = Split(timestamp(args.competition_start), timestamp(args.cutoff))
    if (args.replay_cache is None)!=(args.replay_cache_manifest_sha256 is None):
        p.error('replay cache path and manifest SHA256 must be supplied together')
    if args.replay_cache is not None:
        from moloch.learning_actions.disk_tape import DiskFlowTape
        tape=DiskFlowTape(args.replay_cache,args.replay_cache_manifest_sha256,args.dataset)
    else:tape=FlowTape(args.dataset)
    mechanics = load_mechanics(args.mechanics)
    b = mechanics.budget
    if (b.reads_per_minute, b.read_completion_gap_ms, b.writes_per_minute, b.write_completion_gap_ms) != (100, 600, 24, 2500):
        raise ValueError('PRODUCTION_SIG_BUDGET_MISMATCH')
    roles = coverage(tape, split, args.maximum_gap_ms)
    out = {'schema_version': 1, 'classification': 'offline_replay_specification_not_training_or_profit',
        'split': asdict(split), 'delivery': asdict(Delivery()), 'sig_budget': asdict(b),
        'mechanics_sha256': hashlib.sha256(args.mechanics.read_bytes()).hexdigest(),
        'dataset_manifest_sha256': tape.manifest_sha256,
        'maximum_venue_gap_ms': args.maximum_gap_ms, 'roles': roles,
        'full_history_ready': all(r['coverage_pass'] for r in roles.values()),
        'independent_test_eligible':independent_test_claim(tape.manifest,split),
        'coverage_is_not_contract_equivalence_or_execution_calibration': True,
        'latency_calibrated': False, 'only_execution_venue': 'SIG',
        'source_sha256': {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                          for p in Path(__file__).parent.glob('*.py') if not p.name.startswith('test_')},
        'production_changed': False}
    if hasattr(tape,'disk_store'):out['replay_cache_manifest_sha256']=tape.disk_store.cache_manifest_sha256
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open('x', encoding='utf-8') as f: json.dump(out, f, indent=2, allow_nan=False)
    print(json.dumps({'specification_written': True, 'full_history_ready': out['full_history_ready']}))
    if hasattr(tape,'disk_store'):tape.disk_store.close()


if __name__ == '__main__': main()
