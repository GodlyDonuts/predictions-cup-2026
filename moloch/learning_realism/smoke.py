"""Real-data neural-policy replay of already inspected development data only.

This does not open an independent test, train, promote a policy or contact APIs.
"""
import argparse
from collections import Counter
from dataclasses import asdict
import hashlib
import json
from pathlib import Path
import torch
from moloch.learning_flow.episodes import FlowTape
from moloch.learning_flow.policy import FlowPolicy
from moloch.learning_v2.run_experiment import load_mechanics
from .protocol import Split, DAY
from .replay import ReplayTape


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--dataset', type=Path, required=True)
    p.add_argument('--mechanics', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--seconds', type=int, default=60)
    args = p.parse_args()
    if not 1 <= args.seconds <= 300: raise ValueError('FINITE_ENGINEERING_SMOKE')
    torch.manual_seed(1901); torch.set_num_threads(2)
    base = FlowTape(args.dataset)
    # Deliberately synthetic role boundaries, not the human's historical split.
    # Existing inspected history is never advertised as untouched evidence.
    split = Split(base.times[0], base.times[-1]+9*DAY)
    tape = ReplayTape(args.dataset, split=split, role='train')
    mechanics = load_mechanics(args.mechanics)
    start = base.times[0]+1000; end = start+args.seconds*1000
    env = tape.episode(mechanics, start, end, initial_cash_micros=100000000000,
                       target_wealth_micros=1100000000000, burnin_ms=1000)
    model = FlowPolicy(len(tape.exchanges), hidden_size=32, market_hidden_size=8)
    hidden = model.initial_state(1); decisions = Counter()
    for at in range(start, end, 1000):
        env.advance(at); observation = env.observe()
        inputs = {k: torch.as_tensor(observation[k]).unsqueeze(0)
                  for k in ('contracts', 'global_state', 'present')}
        masks = {k: torch.as_tensor(v).unsqueeze(0) for k, v in observation['masks'].items()}
        with torch.no_grad():
            result = model.step(**inputs, masks=masks, hidden=hidden, reset=torch.tensor([at == start]))
        hidden = result['hidden']
        action = {k: v.item() for k, v in result['actions'].items()}
        decisions[str(action['family'])] += 1
        env.act(action)
    env.advance(end)
    out = {'schema_version': 1, 'classification': 'inspected_real_data_neural_replay_smoke_only',
        'dataset_manifest_sha256': tape.manifest_sha256, 'synthetic_smoke_split': asdict(split),
        'seconds': args.seconds, 'decisions': sum(decisions.values()), 'action_families': dict(decisions),
        'feed_requests': dict(env.feed_request_counts),
        'feed_receipts': dict(env.feed_receipt_counts),
        'sig_reads': sum(r.startswith('feed:SIG:') or ':read:' in r for r in env.budget.request_ids),
        'sig_writes': sum(r.endswith(':submit') or r.endswith(':cancel') for r in env.budget.request_ids),
        'simulated_fill_receipts': len(env.fill_events),
        'only_execution_venue': 'SIG', 'http_requests': 0, 'training_performed': False,
        'independent_test_used': False, 'profitability_established': False, 'production_changed': False}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open('x', encoding='utf-8') as f: json.dump(out, f, indent=2)
    print(json.dumps(out))


if __name__ == '__main__': main()
