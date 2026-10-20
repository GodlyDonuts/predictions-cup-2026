"""Offline sparse-candle provenance audit; never creates execution quotes."""
import argparse
import hashlib
import json
import re
from collections import Counter
from datetime import datetime, timezone
from decimal import Decimal
from pathlib import Path
from urllib.parse import parse_qs, urlsplit


def require(ok, code):
    if not ok:
        raise ValueError(code)


def clock(value):
    dt = datetime.fromisoformat(value.replace('Z', '+00:00'))
    require(dt.tzinfo is not None, 'AWARE_CLOCK_REQUIRED')
    delta = dt - datetime(1970, 1, 1, tzinfo=timezone.utc)
    return delta.days * 86400000 + delta.seconds * 1000 + delta.microseconds // 1000


def exact(value, maximum=None):
    require(type(value) in (int, Decimal), 'OBSERVATION_NUMBER')
    require(Decimal(value).is_finite() and value >= 0, 'OBSERVATION_NONNEGATIVE')
    require(maximum is None or value <= maximum, 'OBSERVATION_RANGE')
    return str(value)


def audit(root, scope, *, start, train_end, end):
    root = Path(root).resolve()
    floor, split, limit = map(clock, (start, train_end, end))
    require(0 <= floor < split < limit, 'FIXED_ROLE_BOUNDARIES')
    raw_index = (root / 'index.jsonl').read_bytes()
    require(raw_index.endswith(b'\n'), 'SEALED_INDEX_LINES')
    index = [json.loads(line) for line in raw_index.splitlines()]
    complete = json.loads((root / 'complete.json').read_text())
    require(complete['throughExclusive'] == end and complete['fromInclusive'] == start,
            'COMPLETION_RANGE')
    expected = sorted(scope['exchanges'])
    order, completed, requested = [], set(), end
    current, seen = None, set()
    daily, roles = Counter(), Counter()
    normalized, inventory = hashlib.sha256(), hashlib.sha256()
    total, excluded, retained_bytes = 0, 0, 0
    first = last = None
    retrieval_first = retrieval_last = None
    for number, entry in enumerate(index):
        require(entry['page'] == number, 'CONTIGUOUS_PAGE_SEQUENCE')
        require(entry.get('trainingAdmitted') is False and
                entry.get('bookDepthReconstructed') is False, 'OBSERVATION_ONLY_INDEX')
        exchange = entry['exchangeId']
        require(exchange in scope['exchanges'], 'ORIGINAL_EXCHANGE_SCOPE')
        if exchange != current:
            require(current is None or current in completed, 'PREVIOUS_EXCHANGE_INCOMPLETE')
            require(exchange not in order, 'EXCHANGE_REAPPEARED')
            order.append(exchange)
            current, requested, seen = exchange, end, set()
        require(exchange not in completed and entry['requestedThroughExclusive'] == requested,
                'EXCLUSIVE_BACKWARD_CURSOR_CHAIN')
        name = entry['file']
        require(re.fullmatch(r'page-\d{6}-validated\.json', name) is not None, 'RECEIPT_NAME')
        file = root / name
        require(file.resolve().parent == root and not file.is_symlink(), 'LOCAL_ORIGINAL_RECEIPT')
        raw = file.read_bytes()
        require(hashlib.sha256(raw).hexdigest() == entry['sha256'], 'RECEIPT_HASH_CHANGED')
        retained_bytes += len(raw)
        inventory.update(json.dumps([name, entry['sha256']], separators=(',', ':')).encode() + b'\n')
        receipt = json.loads(raw, parse_float=Decimal)
        require(receipt['status'] == 200, 'SUCCESSFUL_HISTORY_RECEIPT')
        began, received = clock(receipt['requestStartedAt']), clock(receipt['receivedAt'])
        require(began <= received and received >= clock(requested), 'ACTUAL_RETRIEVAL_CLOCK')
        require(entry['retrievedAt'] == receipt['receivedAt'], 'INDEX_RETRIEVAL_CLOCK')
        retrieval_first = min(retrieval_first or received, received)
        retrieval_last = max(retrieval_last or received, received)
        url = urlsplit(receipt['endpoint'])
        require(not url.scheme and not url.netloc and not url.fragment and
                url.path == f'/exchanges/{exchange}/price-history', 'EXACT_TYPED_ENDPOINT')
        require(parse_qs(url.query) == {'tournamentId': [scope['tournamentId']], 'resolution': ['1m'],
                'limit': ['500'], 'to': [requested]}, 'EXACT_HISTORY_QUERY')
        data = receipt['data']
        require(data['exchangeId'] == exchange and data['marketId'] == scope['exchanges'][exchange]
                and data['resolution'] == '1m', 'RECEIPT_CONTRACT_SCOPE')
        lower, upper = clock(data['from']), clock(data['to'])
        rows = data['candles']
        require(isinstance(rows, list) and len(rows) <= 500 and len(rows) == entry['candles'],
                'BOUNDED_CANDLE_ARRAY')
        require(type(data['coverage']['complete']) is bool and
                type(data['coverage']['projectedThroughSequence']) is int, 'COVERAGE_SCHEMA')
        terminal = lower == upper and not rows and data['coverage']['complete'] is True
        require((lower < upper or terminal) and upper <= clock(requested) and upper % 60000 == 0,
                'HISTORICAL_WINDOW')
        earliest = None
        for row in rows:
            t = clock(row['time'])
            require(lower <= t and t + 60000 <= upper and t % 60000 == 0 and t not in seen,
                    'UNIQUE_CAUSAL_CANDLE_CLOCK')
            seen.add(t)
            earliest = min(earliest if earliest is not None else t, t)
            total += 1
            # Time-fence before inspecting any out-of-role price or volume.
            if t < floor or t >= limit:
                excluded += 1
                continue
            prices = [None if row[k] is None else exact(row[k], 1)
                      for k in ('open', 'high', 'low', 'close', 'vwap')]
            volume = exact(row['volume'])
            require(type(row['tradeCount']) is int and row['tradeCount'] >= 0, 'TRADE_COUNT')
            role = 'train' if t < split else 'evolution'
            roles[role] += 1
            day = datetime.fromtimestamp(t // 1000, timezone.utc).date().isoformat()
            daily[day] += 1
            first, last = min(first or t, t), max(last or t, t)
            normalized.update(json.dumps([exchange, t, prices, volume, row['tradeCount'], role],
                                         separators=(',', ':')).encode() + b'\n')
        next_end = entry['nextThroughExclusive']
        if earliest is None or earliest <= floor:
            require(next_end is None, 'TERMINAL_CURSOR_REQUIRED')
            completed.add(exchange)
        else:
            require(next_end is not None and clock(next_end) == earliest, 'EXACT_NEXT_CANDLE_MINUTE')
            requested = next_end
    require(order == expected and completed == set(expected), 'FULL_ORIGINAL_SCOPE_REQUIRED')
    require(complete['requests'] == len(index) and complete['candles'] == total and
            complete['exchangesComplete'] == len(expected), 'COMPLETION_COUNTS')
    return {'classification': 'audited_sparse_trade_candles_not_books_or_original_delivery',
            'observedUtc': datetime.now(timezone.utc).isoformat(), 'exchanges': len(expected),
            'receiptPages': len(index), 'receiptBytes': retained_bytes, 'rawCandles': total,
            'excludedOutsideRangeBeforeValues': excluded, 'roleCounts': dict(roles),
            'dailySampleCounts': dict(sorted(daily.items())), 'firstSourceMs': first, 'lastSourceMs': last,
            'firstActualRetrievalMs': retrieval_first, 'lastActualRetrievalMs': retrieval_last,
            'indexSha256': hashlib.sha256(raw_index).hexdigest(),
            'receiptInventorySha256': inventory.hexdigest(),
            'normalizedScopedCandleSha256': normalized.hexdigest(),
            'fromInclusive': start, 'trainEndExclusive': train_end, 'endExclusive': end,
            'competitionStartVerified': False, 'historicalCatalogAvailabilityVerified': False,
            'historicalBookDepthReconstructed': False, 'trainingAdmitted': False,
            'candleStartIsNotAvailabilityTime': True, 'minimumAggregateDelayMs': 60000,
            'originalHistoricalDeliveryVerified': False,
            'normalizedDigestOrder': 'acquisition_pages_not_chronological_replay',
            'independentTestUsed': False, 'profitabilityEstablished': False}


if __name__ == '__main__':
    p = argparse.ArgumentParser()
    for name in ('root', 'scope', 'start', 'train-end', 'end', 'output'):
        p.add_argument('--' + name, required=True)
    a = p.parse_args()
    report = audit(a.root, json.loads(Path(a.scope).read_text()), start=a.start,
                   train_end=a.train_end, end=a.end)
    with Path(a.output).open('x') as f:
        json.dump(report, f, indent=2)
        f.write('\n')
    print(json.dumps({k: report[k] for k in ('exchanges', 'receiptPages', 'roleCounts', 'trainingAdmitted')}))
