"""Exact audited OHLCV history on a declared causal replay clock.

This is a public-information provider. It supplies no executable liquidity,
settlement prices or original historical delivery timestamps. The caller must
admit API requests through the simulated account's shared limiter.
"""
import argparse
from collections import Counter
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import sqlite3
import time
from .audit_sig_history import clock, exact, require
from .historical_samples import Delivery, digest, integer

FORMAT = 'audited_sig_candle_information_v1'
FIELDS = ('open', 'high', 'low', 'close', 'vwap')


def build(root, audit_path, audit_sha256, output):
    began = time.perf_counter()
    root, audit_path, output = map(Path, (root, audit_path, output))
    audit_raw = audit_path.read_bytes()
    require(hashlib.sha256(audit_raw).hexdigest() == audit_sha256, 'PINNED_CANDLE_AUDIT')
    audit = json.loads(audit_raw)
    require(audit['classification'] == 'audited_sparse_trade_candles_not_books_or_original_delivery'
            and audit['trainingAdmitted'] is False
            and audit['minimumAggregateDelayMs'] == 60000, 'AUDITED_CANDLE_SOURCE_REQUIRED')
    index_raw = (root / 'index.jsonl').read_bytes()
    require(hashlib.sha256(index_raw).hexdigest() == audit['indexSha256'], 'AUDITED_INDEX_CHANGED')
    entries = [json.loads(line) for line in index_raw.splitlines()]
    floor, split, end = map(clock, (audit['fromInclusive'], audit['trainEndExclusive'], audit['endExclusive']))
    output.mkdir(parents=True, exist_ok=False)
    db = sqlite3.connect(output / 'history.sqlite')
    count = raw_count = excluded = receipt_bytes = 0
    roles = Counter()
    normal, inventory = hashlib.sha256(), hashlib.sha256()
    try:
        db.executescript('''
CREATE TABLE receipts(i INTEGER PRIMARY KEY,file TEXT,sha TEXT,started TEXT,received TEXT);
CREATE TABLE exchanges(exchange TEXT PRIMARY KEY,market TEXT NOT NULL);
CREATE TABLE candles(exchange TEXT,at INTEGER,open TEXT,high TEXT,low TEXT,close TEXT,
 vwap TEXT,volume TEXT,trades INTEGER,receipt INTEGER,ordinal INTEGER UNIQUE,
 PRIMARY KEY(exchange,at));
''')
        for i, entry in enumerate(entries):
            name = entry['file']
            path = (root / name).resolve()
            require(path.parent == root.resolve() and not path.is_symlink(), 'ORIGINAL_RECEIPT_PATH')
            raw = path.read_bytes()
            require(hashlib.sha256(raw).hexdigest() == entry['sha256'], 'AUDITED_RECEIPT_CHANGED')
            receipt_bytes += len(raw)
            inventory.update(json.dumps([name, entry['sha256']], separators=(',', ':')).encode() + b'\n')
            receipt = json.loads(raw, parse_float=Decimal)
            data, exchange = receipt['data'], entry['exchangeId']
            require(exchange == data['exchangeId'], 'ORIGINAL_EXCHANGE_IDENTITY')
            prior = db.execute('SELECT market FROM exchanges WHERE exchange=?', (exchange,)).fetchone()
            require(prior is None or prior[0] == data['marketId'], 'IMMUTABLE_EXCHANGE_MARKET')
            db.execute('INSERT OR IGNORE INTO exchanges VALUES(?,?)', (exchange, data['marketId']))
            db.execute('INSERT INTO receipts VALUES(?,?,?,?,?)',
                       (i, name, entry['sha256'], receipt['requestStartedAt'], receipt['receivedAt']))
            for row in data['candles']:
                at = clock(row['time'])
                raw_count += 1
                if not floor <= at < end:
                    excluded += 1
                    continue
                prices = [None if row[k] is None else exact(row[k], 1) for k in FIELDS]
                volume = exact(row['volume'])
                require(type(row['tradeCount']) is int and row['tradeCount'] >= 0, 'EXACT_TRADE_COUNT')
                role = 'train' if at < split else 'evolution'
                normal.update(json.dumps([exchange, at, prices, volume, row['tradeCount'], role],
                                         separators=(',', ':')).encode() + b'\n')
                db.execute('INSERT INTO candles VALUES(?,?,?,?,?,?,?,?,?,?,?)',
                           (exchange, at, *prices, volume, row['tradeCount'], i, count))
                count += 1
                roles[role] += 1
            if i % 50 == 0:
                db.commit()
        require(len(entries) == audit['receiptPages'] and receipt_bytes == audit['receiptBytes']
                and raw_count == audit['rawCandles']
                and excluded == audit['excludedOutsideRangeBeforeValues']
                and dict(roles) == audit['roleCounts'], 'AUDITED_COUNTS_CHANGED')
        require(normal.hexdigest() == audit['normalizedScopedCandleSha256']
                and inventory.hexdigest() == audit['receiptInventorySha256'], 'AUDITED_FACTS_CHANGED')
        require(db.execute('SELECT COUNT(*) FROM exchanges').fetchone()[0] == audit['exchanges'],
                'AUDITED_EXCHANGE_SCOPE_CHANGED')
        db.commit()
        # Decode stored exact fields independently in original acquisition order.
        decoded = hashlib.sha256()
        for ex, at, op, high, low, close, vwap, volume, trades in db.execute(
                'SELECT exchange,at,open,high,low,close,vwap,volume,trades FROM candles ORDER BY ordinal'):
            decoded.update(json.dumps([ex, at, [op, high, low, close, vwap], volume, trades,
                                       'train' if at < split else 'evolution'],
                                      separators=(',', ':')).encode() + b'\n')
        require(decoded.hexdigest() == normal.hexdigest(), 'STORED_CANDLE_FACTS_CHANGED')
    finally:
        db.close()
    require(hashlib.sha256((root / 'index.jsonl').read_bytes()).hexdigest() == audit['indexSha256']
            and audit_path.read_bytes() == audit_raw, 'CANDLE_SOURCE_CHANGED_DURING_BUILD')
    manifest = {'format': FORMAT, 'sourceAuditSha256': audit_sha256,
        'sourceIndexSha256': audit['indexSha256'], 'databaseSha256': digest(output / 'history.sqlite'),
        'databaseBytes': (output / 'history.sqlite').stat().st_size, 'records': count,
        'exchanges': audit['exchanges'], 'receiptPages': len(entries), 'roleCountsBySourceStart': dict(roles),
        'normalizedScopedCandleSha256': normal.hexdigest(), 'storedDecodedSha256': decoded.hexdigest(),
        'sourceBoundsMs': [floor, end], 'trainEndExclusiveMs': split, 'minimumAggregateDelayMs': 60000,
        'exactObservationDecimalsRetained': True, 'originalHistoricalDeliveryVerified': False,
        'bookDepthReconstructed': False, 'trainingAdmitted': False,
        'independentTestUsed': False, 'buildWallSeconds': time.perf_counter() - began}
    with (output / 'complete.json').open('x') as stream:
        json.dump(manifest, stream, indent=2)
        stream.write('\n')
    return manifest


class CandleHistory:
    def __init__(self, directory, manifest_sha256, *, scoring_bounds,
                 information_start_ms, delivery):
        directory = Path(directory)
        raw = (directory / 'complete.json').read_bytes()
        require(hashlib.sha256(raw).hexdigest() == manifest_sha256, 'CANDLE_MANIFEST_CHANGED')
        self.manifest = m = json.loads(raw)
        require(m['format'] == FORMAT and m['trainingAdmitted'] is False
                and m['originalHistoricalDeliveryVerified'] is False
                and m['minimumAggregateDelayMs'] == 60000, 'EXPLICIT_CANDLE_INFORMATION_FORMAT')
        lo, hi = scoring_bounds
        require(all(integer(x) for x in (lo, hi, information_start_ms))
                and information_start_ms <= lo < hi, 'FIXED_CANDLE_ROLE_BOUNDS')
        require(isinstance(delivery, Delivery) and delivery.delay_ms >= 60000,
                'CANDLE_MUST_CLOSE_BEFORE_PUBLICATION')
        database = directory / 'history.sqlite'
        require(digest(database) == m['databaseSha256'], 'CANDLE_DATABASE_CHANGED')
        self.db = sqlite3.connect(database.resolve().as_uri() + '?mode=ro&immutable=1', uri=True)
        self.bounds, self.start, self.delivery = (lo, hi), information_start_ms, delivery
        self.exchanges = dict(self.db.execute('SELECT exchange,market FROM exchanges'))

    def page(self, exchange, now, *, through_exclusive_ms=None, limit=500):
        require(integer(now) and self.bounds[0] <= now < self.bounds[1], 'CANDLE_QUERY_OUTSIDE_ROLE')
        require(type(limit) is int and 1 <= limit <= 1000, 'DOCUMENTED_CANDLE_PAGE_LIMIT')
        through = self.manifest['sourceBoundsMs'][1] if through_exclusive_ms is None else through_exclusive_ms
        require(integer(through), 'EXACT_CANDLE_QUERY_CLOCK')
        if exchange not in self.exchanges:
            raise KeyError(exchange)
        rows = self.db.execute('''SELECT c.at,c.open,c.high,c.low,c.close,c.vwap,c.volume,c.trades,
 r.received,r.sha,r.file FROM candles c JOIN receipts r ON r.i=c.receipt
 WHERE c.exchange=? AND c.at>=? AND c.at<? AND c.at<=?
 ORDER BY c.at DESC LIMIT ?''',
                              (exchange, self.start, through, self.delivery.source_frontier(now), limit))
        candles = []
        for at, op, high, low, close, vwap, volume, trades, received, sha, file in rows:
            candles.append({'source_at_ms': at, 'bar_end_ms': at + 60000,
                'simulated_available_at_ms': self.delivery.available(at),
                **dict(zip(FIELDS, (op, high, low, close, vwap))),
                'volume': volume, 'tradeCount': trades, 'actual_retrieved_at': received,
                'original_receipt_sha256': sha, 'original_receipt_file': file})
        return {'exchange_id': exchange, 'market_id': self.exchanges[exchange],
                'candles': list(reversed(candles)), 'original_historical_delivery_verified': False,
                'executable_liquidity': False}

    def close(self):
        self.db.close()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('root', 'audit', 'audit-sha256', 'output'):
        parser.add_argument('--' + name, required=True)
    args = parser.parse_args()
    result = build(args.root, args.audit, args.audit_sha256, args.output)
    print(json.dumps(result, indent=2))
