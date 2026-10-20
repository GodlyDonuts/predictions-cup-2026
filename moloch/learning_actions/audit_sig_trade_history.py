"""Offline audit of observed public trades, without treating them as our fills."""
import argparse
import hashlib
import json
import re
from collections import Counter
from datetime import datetime, timezone
from decimal import Decimal
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

try:
    from .audit_sig_history import clock, exact, require
except ImportError:  # Standalone frozen VPS audit alongside the checked helper.
    from audit_sig_history import clock, exact, require


def audit(root, scope, *, start, train_end, end):
    root = Path(root).resolve()
    lo, split, hi = map(clock, (start, train_end, end))
    require(lo < split < hi, 'FIXED_TRADE_ROLES')
    raw_index = (root / 'index.jsonl').read_bytes()
    require(raw_index.endswith(b'\n'), 'SEALED_TRADE_INDEX')
    index = [json.loads(x) for x in raw_index.splitlines()]
    complete = json.loads((root / 'complete.json').read_text())
    require(complete['fromInclusive'] == start and complete['throughExclusive'] == end, 'TRADE_COMPLETE_RANGE')
    current, cursor, order, completed = None, None, [], set()
    cursors, facts, ids = set(), set(), {}
    roles, daily = Counter(), Counter()
    normal, inventory = hashlib.sha256(), hashlib.sha256()
    count = nulls = reused_ids = receipt_bytes = 0
    first = last = retrieval_first = retrieval_last = None
    for number, entry in enumerate(index):
        require(entry['page'] == number and entry['trainingAdmitted'] is False and
                entry['bookDepthReconstructed'] is False, 'TRADE_INDEX_SEQUENCE')
        exchange = entry['exchangeId']
        require(exchange in scope['exchanges'], 'TRADE_ORIGINAL_SCOPE')
        if exchange != current:
            require(current is None or current in completed, 'TRADE_UNFINISHED_EXCHANGE')
            require(exchange not in order, 'TRADE_EXCHANGE_REAPPEARED')
            current, cursor, cursors, facts, ids = exchange, None, set(), set(), {}
            order.append(exchange)
        require(exchange not in completed and entry['requestCursor'] == cursor and
                entry['fromInclusive'] == start and entry['throughExclusive'] == end, 'TRADE_CURSOR_CHAIN')
        name = entry['file']
        require(re.fullmatch(r'page-\d{6}-validated\.json', name) is not None, 'TRADE_RECEIPT_NAME')
        file = root / name
        require(file.resolve().parent == root and not file.is_symlink(), 'TRADE_LOCAL_RECEIPT')
        raw = file.read_bytes()
        require(hashlib.sha256(raw).hexdigest() == entry['sha256'], 'TRADE_RECEIPT_HASH')
        receipt_bytes += len(raw)
        inventory.update(json.dumps([name, entry['sha256']], separators=(',', ':')).encode() + b'\n')
        r = json.loads(raw, parse_float=Decimal)
        require(r['status'] == 200 and clock(r['request_started_at']) <= clock(r['received_at'])
                and clock(r['received_at']) >= hi and entry['retrievedAt'] == r['received_at'], 'TRADE_RECEIPT_CLOCK')
        received = clock(r['received_at'])
        retrieval_first, retrieval_last = min(retrieval_first or received, received), max(retrieval_last or received, received)
        u = urlsplit(r['endpoint'])
        query = {'tournamentId': [scope['tournamentId']], 'from': [start], 'to': [end], 'limit': ['200']}
        if cursor is not None:
            query['cursor'] = [cursor]
        require(not u.scheme and not u.netloc and not u.fragment and
                u.path == f'/exchanges/{exchange}/trades' and parse_qs(u.query) == query, 'EXACT_TRADE_QUERY')
        d = r['data']
        require(d['exchangeId'] == exchange and d['marketId'] == scope['exchanges'][exchange]
                and clock(d['from']) == lo and clock(d['to']) == hi, 'TRADE_SCOPE_WINDOW')
        coverage, pg, rows = d['coverage'], d['pagination'], d['data']
        require(coverage['complete'] is True and (coverage['projectedThroughSequence'] is None or
                type(coverage['projectedThroughSequence']) is int), 'TRADE_COVERAGE')
        require(type(pg['limit']) is int and 1 <= pg['limit'] <= 200 and type(pg['hasMore']) is bool
                and isinstance(rows, list) and len(rows) <= 200 and len(rows) == entry['trades'], 'TRADE_PAGE_BOUND')
        for row in rows:
            t = clock(row['createdAt'])
            require(lo <= t < hi and isinstance(row['id'], str) and row['id'].isdigit() and
                    type(row['size']) is int and row['size'] > 0 and row['side'] in ('YES', 'NO'), 'TRADE_EVENT')
            price = None if row['price'] is None else exact(row['price'], 1)
            nulls += price is None
            role = 'train' if t < split else 'evolution'
            fact = [exchange, row['id'], t, price, row['size'], row['side'], exact(row['volume']), role]
            encoded = json.dumps(fact, separators=(',', ':')).encode() + b'\n'
            key = hashlib.sha256(encoded).digest()
            require(key not in facts, 'TRADE_DUPLICATE_FULL_FACT')
            facts.add(key)
            if row['id'] in ids and ids[row['id']] != key:
                reused_ids += 1  # Documented separate legacy/engine ID spaces.
            ids[row['id']] = key
            normal.update(encoded)
            roles[role] += 1
            daily[datetime.fromtimestamp(t // 1000, timezone.utc).date().isoformat()] += 1
            first, last = min(first or t, t), max(last or t, t)
            count += 1
        next_cursor = pg['nextCursor']
        require(entry['nextCursor'] == next_cursor, 'TRADE_INDEX_CURSOR')
        if pg['hasMore']:
            require(bool(rows) and isinstance(next_cursor, str) and bool(next_cursor) and
                    next_cursor not in cursors, 'TRADE_REPEATED_CURSOR')
            cursors.add(next_cursor)
            cursor = next_cursor
        else:
            require(next_cursor is None, 'TRADE_FINAL_CURSOR')
            completed.add(exchange)
    require(order == sorted(scope['exchanges']) and completed == set(order), 'TRADE_FULL_SCOPE')
    require(complete['requests'] == len(index) and complete['trades'] == count and
            complete['exchangesComplete'] == len(order), 'TRADE_COMPLETE_COUNTS')
    return {'classification': 'audited_public_trade_facts_not_owned_fills_or_book_depth',
            'observedUtc': datetime.now(timezone.utc).isoformat(), 'exchanges': len(order),
            'receiptPages': len(index), 'validatedReceiptBytes': receipt_bytes, 'trades': count,
            'nullPriceObservations': nulls, 'reusedLegacyEngineIdsRetainedAsDistinctFacts': reused_ids,
            'roleCounts': dict(roles), 'dailyTradeCounts': dict(sorted(daily.items())),
            'firstSourceMs': first, 'lastSourceMs': last, 'firstActualRetrievalMs': retrieval_first,
            'lastActualRetrievalMs': retrieval_last, 'fromInclusive': start,
            'trainEndExclusive': train_end, 'endExclusive': end,
            'indexSha256': hashlib.sha256(raw_index).hexdigest(),
            'receiptInventorySha256': inventory.hexdigest(), 'normalizedFactSha256': normal.hexdigest(),
            'trainingAdmitted': False, 'historicalBookDepthReconstructed': False,
            'originalHistoricalDeliveryVerified': False, 'ownedFillsReconstructed': False,
            'historicalCatalogAvailabilityVerified': False, 'independentTestUsed': False,
            'profitabilityEstablished': False}


if __name__ == '__main__':
    p = argparse.ArgumentParser()
    for name in ('root', 'scope', 'start', 'train-end', 'end', 'output'):
        p.add_argument('--' + name, required=True)
    a = p.parse_args()
    r = audit(a.root, json.loads(Path(a.scope).read_text()), start=a.start, train_end=a.train_end, end=a.end)
    with Path(a.output).open('x') as f:
        json.dump(r, f, indent=2)
        f.write('\n')
    print(json.dumps({k: r[k] for k in ('exchanges', 'trades', 'roleCounts', 'trainingAdmitted')}))
