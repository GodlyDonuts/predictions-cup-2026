"""Training-only hindsight quote census; not a trading strategy or ROI backtest."""
import argparse
from collections import deque
import hashlib
import json
from pathlib import Path
import sqlite3
import time


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class Window:
    def __init__(self, minimum_ms=3000, maximum_ms=3600000):
        self.minimum_ms, self.maximum_ms = minimum_ms, maximum_ms
        self.pending, self.lowest = deque(), deque()
        self.best = None

    def observe(self, at, bid, ask, bid_quantity, ask_quantity, source):
        cutoff = at - self.maximum_ms
        while self.lowest and self.lowest[0][0] < cutoff:
            self.lowest.popleft()
        while self.pending and self.pending[0][0] <= at - self.minimum_ms:
            entry = self.pending.popleft()
            if entry[0] < cutoff:
                continue
            while self.lowest and self.lowest[-1][1] >= entry[1]:
                self.lowest.pop()
            self.lowest.append(entry)
        if self.lowest:
            entry = self.lowest[0]
            profit = bid - entry[1]
            if profit > 0 and (self.best is None or profit > self.best['oneShareGrossMicros']):
                self.best = dict(entryAtMs=entry[0], exitAtMs=at, entryAskMicros=entry[1],
                    exitBidMicros=bid, oneShareGrossMicros=profit,
                    displayedTopQuantity=min(entry[2], bid_quantity),
                    entrySourceSha256=entry[3], exitSourceSha256=source)
        self.pending.append((at, ask, ask_quantity, source))


def run(experiment_path, cache, output):
    experiment_path, cache, output = map(Path, (experiment_path, cache, output))
    original = json.loads(experiment_path.read_bytes())
    bounds = original['actual_train_bounds_ms']
    if bounds != original['config']['development_train_ms'] or bounds[1] <= bounds[0]:
        raise ValueError('EXACT_INSPECTED_FIT_BOUNDS_REQUIRED')
    output.mkdir(parents=True, exist_ok=False)
    manifest = cache / 'manifest.json'
    before = digest(manifest)
    database = cache / 'replay.sqlite'
    stat = database.stat()
    db = sqlite3.connect(database.resolve().as_uri() + '?mode=ro&immutable=1', uri=True)
    started = time.monotonic()
    windows, counts = {}, dict(rows=0, validBooks=0, staleOrInvalid=0)
    try:
        query = ('SELECT at,ex,body FROM market INDEXED BY market_time '
                 'WHERE at>=? AND at<? AND venue=? AND kind=? ORDER BY at,i')
        for at, exchange, raw in db.execute(query, (*bounds, 'SIG', 'book')):
            counts['rows'] += 1
            row = json.loads(raw)
            bids, asks = row.get('bids', []), row.get('asks', [])
            source_at = row.get('source_at_ms')
            if (not bids or not asks or type(source_at) is not int or
                    not 0 <= at - source_at <= 10000 or row.get('available_at_ms') != at or
                    not 0 < bids[0][0] < asks[0][0] < 1000000 or
                    bids[0][1] < 1 or asks[0][1] < 1):
                counts['staleOrInvalid'] += 1
                continue
            counts['validBooks'] += 1
            bid, bq = bids[0]
            ask, aq = asks[0]
            for side, bb, aa, bqty, aqty in [('YES', bid, ask, bq, aq),
                    ('NO', 1000000 - ask, 1000000 - bid, aq, bq)]:
                window = windows.setdefault((exchange, side), Window())
                window.observe(at, bb, aa, bqty, aqty, row['original_sha256'])
            if counts['rows'] % 100000 == 0:
                (output / 'progress.json').write_text(json.dumps(dict(**counts, atMs=at,
                    elapsedSeconds=time.monotonic()-started)))
    finally:
        db.close()
    after = database.stat()
    if (stat.st_size, stat.st_mtime_ns) != (after.st_size, after.st_mtime_ns) or digest(manifest) != before:
        raise ValueError('IMMUTABLE_CACHE_CHANGED')
    best = sorted([dict(exchangeId=ex, side=side, **w.best)
                   for (ex, side), w in windows.items() if w.best],
                  key=lambda r: (-r['oneShareGrossMicros'], r['exchangeId'], r['side']))
    result = dict(classification='clairvoyant_fit_quote_opportunities_not_executable_portfolio_ROI',
        implementationSha256=digest(__file__),
        experimentSha256=digest(experiment_path), cacheManifestSha256=before,
        boundsMs=bounds, **counts, exchangeSides=len(windows), profitableExchangeSides=len(best),
        profitableExchanges=len({r['exchangeId'] for r in best}), bestPerExchangeSide=best,
        minimumHoldMs=3000, maximumHoldMs=3600000, maximumQuoteAgeMs=10000,
        elapsedSeconds=time.monotonic()-started, independentTestUsed=False,
        developmentSelectionUsed=False, httpRequests=0, optimizerRun=False,
        productionChanged=False, profitabilityEstablished=False,
        limitations=['Perfect future timing; cannot be deployed as a policy.',
            'Displayed top depth is not guaranteed execution after latency.',
            'No account/API scheduling, queue depletion, fees or portfolio simulation.',
            'Opportunities overlap; their displayed quantities and gains cannot be summed.'])
    (output / 'RESULT.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('experiment', 'cache', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    result = run(args.experiment, args.cache, args.output)
    print(json.dumps({k: result[k] for k in ('rows', 'validBooks', 'profitableExchangeSides', 'elapsedSeconds')}))


if __name__ == '__main__':
    main()
