"""Hindsight opportunity diagnostic on already exposed development books.

Not a trading policy or fill replay. Ignores observation admission, queue, gaps,
latency uncertainty and adverse selection; positive bounds do not prove profit.
"""
import argparse
from collections import defaultdict, deque
from datetime import datetime, timezone
import gzip
import hashlib
import json
from pathlib import Path
import statistics
import time


def sha(path):
    with path.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


class Window:
    def __init__(self, start, end):
        self.start, self.end = start, end
        self.pending = defaultdict(deque)
        self.cheapest = {}
        self.best = {}
        self.spreads = []
        self.books = 0

    def accept(self, r):
        at = r['available_at_ms']
        if not self.start <= at < self.end or not r['bids'] or not r['asks']:
            return
        bid = max(p for p, q in r['bids'] if q >= 1)
        ask = min(p for p, q in r['asks'] if q >= 1)
        self.books += 1
        self.spreads.append(ask - bid)
        for side, buy, sell in [('YES', ask, bid), ('NO', 1000000 - bid, 1000000 - ask)]:
            key = (r['exchange_id'], side)
            queue = self.pending[key]
            while queue and queue[0][0] + 2500 <= at:
                stamp, price = queue.popleft()
                if key not in self.cheapest or price < self.cheapest[key][1]:
                    self.cheapest[key] = (stamp, price)
            if key in self.cheapest:
                stamp, price = self.cheapest[key]
                gain = sell - price
                if key not in self.best or gain > self.best[key]['gross_micros_per_share']:
                    self.best[key] = {'exchange': key[0], 'side': side,
                        'buy_at_ms': stamp, 'sell_at_ms': at, 'buy_price_micros': price,
                        'sell_price_micros': sell, 'gross_micros_per_share': gain}
            queue.append((at, buy))

    def result(self):
        positive = [r for r in self.best.values() if r['gross_micros_per_share'] > 0]
        return {'bounds_ms': [self.start, self.end], 'book_observations': self.books,
            'exchange_sides_with_two_eligible_observations': len(self.best),
            'exchange_sides_with_positive_hindsight_one_share_roundtrip': len(positive),
            'median_observed_spread_micros': statistics.median(self.spreads) if self.spreads else None,
            'largest_hindsight_opportunities': sorted(positive,
                key=lambda r: r['gross_micros_per_share'], reverse=True)[:10]}


def run(dataset, run_directory):
    began = time.perf_counter()
    dataset, run_directory = Path(dataset), Path(run_directory)
    raw = (dataset / 'dataset-manifest.json').read_bytes()
    manifest = json.loads(raw)
    assert hashlib.sha256(raw).hexdigest() == json.loads((dataset / 'COMPLETE.json').read_bytes())['manifestSha256']
    base = Path(manifest['baseDataset'])
    base_raw = (base / 'dataset-manifest.json').read_bytes()
    assert hashlib.sha256(base_raw).hexdigest() == manifest['baseManifestSha256']
    assert json.loads((base / 'COMPLETE.json').read_bytes())['manifestSha256'] == manifest['baseManifestSha256']
    observations = base / 'observations.jsonl.gz'
    assert sha(observations) == json.loads(base_raw)['observationsSha256']
    member = json.loads((run_directory / 'result-000-000.json').read_bytes())
    bounds = sorted(set((c['start_ms'], c['end_ms']) for c in member['selection']['cases']))
    full = tuple(manifest['calendarRolesMs']['search'])
    assert all(full[0] <= a < b <= full[1] for a, b in bounds)
    windows = [Window(*pair) for pair in bounds + [full]]
    with gzip.open(observations, 'rt', encoding='utf-8') as f:
        for line in f:
            r = json.loads(line)
            if r['kind'] == 'book' and r['venue'] == 'SIG' and full[0] <= r['available_at_ms'] < full[1]:
                for w in windows:
                    w.accept(r)
    assert sha(observations) == json.loads(base_raw)['observationsSha256']
    return {'classification': 'optimistic_hindsight_development_diagnostic_not_strategy_or_fill_replay',
        'observedUtc': datetime.now(timezone.utc).isoformat(),
        'datasetManifestSha256': hashlib.sha256(raw).hexdigest(),
        'observationsSha256': json.loads(base_raw)['observationsSha256'],
        'short_screen_windows': [w.result() for w in windows[:-1]],
        'full_exposed_evolution_window': windows[-1].result(),
        'minimum_buy_sell_separation_ms': 2500, 'wallSeconds': time.perf_counter() - began,
        'ignores_queue_impact_gaps_and_observation_admission': True,
        'independentTestUsed': False, 'gpuUsed': False, 'productionChanged': False}


if __name__ == '__main__':
    p = argparse.ArgumentParser()
    p.add_argument('--dataset', required=True)
    p.add_argument('--run', required=True)
    p.add_argument('--output', required=True)
    a = p.parse_args()
    result = run(a.dataset, a.run)
    with Path(a.output).open('x') as f:
        json.dump(result, f, indent=2)
        f.write('\n')
    print(json.dumps(result, indent=2))
