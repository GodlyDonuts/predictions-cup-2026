"""Audited public Kalshi trade information; execution remains SIG-only."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import sqlite3
import time
from .backfill_kalshi_trades import validate, suffix
from .audit_sig_history import require
from .historical_samples import Delivery, digest, integer

FORMAT = 'audited_kalshi_trade_information_v1'


def event_clocks(value):
    dt = datetime.fromisoformat(value.replace('Z', '+00:00'))
    require(dt.utcoffset() is not None, 'AWARE_KALSHI_EVENT_CLOCK')
    delta = dt - datetime(1970, 1, 1, tzinfo=timezone.utc)
    micros = (delta.days * 86400 + delta.seconds) * 1000000 + delta.microseconds
    return micros // 1000, -(-micros // 1000)


def build(root, audit_path, audit_sha256, output):
    began = time.perf_counter()
    root, audit_path, output = map(Path, (root, audit_path, output))
    raw_audit = audit_path.read_bytes()
    require(hashlib.sha256(raw_audit).hexdigest() == audit_sha256, 'PINNED_KALSHI_AUDIT')
    audit = json.loads(raw_audit)
    raw_plan = (root / 'plan.json').read_bytes()
    require(hashlib.sha256(raw_plan).hexdigest() == audit['planSha256']
            and audit['trainingAdmitted'] is False, 'AUDITED_KALSHI_PLAN')
    plan = json.loads(raw_plan)
    require(plan['role'] in ('training_history_recovery_only', 'evolution_history_recovery_only'),
            'EXPLICIT_RECOVERED_KALSHI_ROLE')
    output.mkdir(parents=True, exist_ok=False)
    db = sqlite3.connect(output / 'history.sqlite')
    inventory = hashlib.sha256()
    receipt_bytes = pages = excluded = duplicates = 0
    try:
        db.executescript('''
CREATE TABLE receipts(i INTEGER PRIMARY KEY,file TEXT,sha TEXT,body_sha TEXT,received TEXT);
CREATE TABLE trades(ticker TEXT,id TEXT,at INTEGER,ready INTEGER,body TEXT,receipt INTEGER,PRIMARY KEY(ticker,id));
CREATE INDEX trade_clock ON trades(ticker,at DESC,id);
''')
        for number, ticker in enumerate(plan['tickers']):
            cursor, seen, page = '', set(), 0
            while True:
                name = f'{number:04d}-{page:07d}.json'
                raw = (root / 'pages' / name).read_bytes()
                sha = hashlib.sha256(raw).hexdigest()
                inventory.update(name.encode())
                inventory.update(bytes.fromhex(sha))
                receipt_bytes += len(raw)
                receipt = json.loads(raw)
                proof = validate(receipt, suffix(ticker, plan['startTs'], plan['endTs'], cursor),
                                 ticker, plan['startTs'], plan['endTs'])
                db.execute('INSERT INTO receipts VALUES(?,?,?,?,?)',
                           (pages, name, sha, receipt['bodySha256'], receipt['receivedAt']))
                for row in receipt['body']['trades']:
                    at, ready = event_clocks(row['created_time'])
                    if not plan['startTs'] * 1000 <= at < plan['endTs'] * 1000:
                        excluded += 1
                        continue
                    body = json.dumps(row, sort_keys=True, separators=(',', ':'), allow_nan=False)
                    prior = db.execute('SELECT body FROM trades WHERE ticker=? AND id=?',
                                       (ticker, row['trade_id'])).fetchone()
                    require(prior is None or prior[0] == body, 'ORIGINAL_KALSHI_TRADE_FACTS_CHANGED')
                    if prior:
                        duplicates += 1
                    else:
                        db.execute('INSERT INTO trades VALUES(?,?,?,?,?,?)',
                                   (ticker, row['trade_id'], at, ready, body, pages))
                pages += 1
                following = proof['nextCursor']
                if not following:
                    break
                require(following not in seen, 'KALSHI_CURSOR_REPEATED')
                seen.add(following)
                cursor, page = following, page + 1
            db.commit()
        count = db.execute('SELECT COUNT(*) FROM trades').fetchone()[0]
        require(pages == audit['receiptPages'] and receipt_bytes == audit['receiptBytes']
                and count == audit['uniqueTradeRecords']
                and len(plan['tickers']) == audit['completedMappedTickers']
                and inventory.hexdigest() == audit['sortedReceiptInventorySha256'],
                'AUDITED_KALSHI_INVENTORY_CHANGED')
        normal = hashlib.sha256()
        for ticker, trade_id, at, ready, body in db.execute('SELECT ticker,id,at,ready,body FROM trades ORDER BY ticker,at,id'):
            normal.update(json.dumps([ticker, trade_id, at, ready, json.loads(body)],
                                    sort_keys=True, separators=(',', ':')).encode() + b'\n')
        db.commit()
    finally:
        db.close()
    require((root / 'plan.json').read_bytes() == raw_plan and audit_path.read_bytes() == raw_audit,
            'KALSHI_SOURCE_CHANGED_DURING_BUILD')
    manifest = {'format': FORMAT, 'sourceAuditSha256': audit_sha256,
        'sourcePlanSha256': audit['planSha256'], 'receiptInventorySha256': inventory.hexdigest(),
        'databaseSha256': digest(output / 'history.sqlite'),
        'databaseBytes': (output / 'history.sqlite').stat().st_size,
        'sourceBoundsMs': [plan['startTs'] * 1000, plan['endTs'] * 1000],
        'role': plan['role'], 'tickers': plan['tickers'], 'records': count, 'receiptPages': pages,
        'excludedUpperBoundaryRows': excluded, 'duplicateRows': duplicates,
        'normalizedTradeSha256': normal.hexdigest(), 'originalHistoricalDeliveryVerified': False,
        'exactDecimalStringsRetained': True, 'executionAuthority': False,
        'subMillisecondPublicationRoundedUp': True,
        'trainingAdmitted': False, 'independentTestUsed': False,
        'buildWallSeconds': time.perf_counter() - began}
    with (output / 'complete.json').open('x') as stream:
        json.dump(manifest, stream, indent=2)
        stream.write('\n')
    return manifest


class KalshiHistory:
    def __init__(self, directory, manifest_sha256, *, scoring_bounds,
                 information_start_ms, delivery):
        directory = Path(directory)
        raw = (directory / 'complete.json').read_bytes()
        require(hashlib.sha256(raw).hexdigest() == manifest_sha256, 'KALSHI_MANIFEST_CHANGED')
        self.manifest = m = json.loads(raw)
        require(m['format'] == FORMAT and m['trainingAdmitted'] is False
                and m['executionAuthority'] is False
                and m['originalHistoricalDeliveryVerified'] is False, 'KALSHI_INFORMATION_FORMAT')
        lo, hi = scoring_bounds
        require(all(integer(x) for x in (lo, hi, information_start_ms))
                and information_start_ms <= lo < hi and isinstance(delivery, Delivery),
                'FIXED_KALSHI_ROLE_AND_DELIVERY')
        database = directory / 'history.sqlite'
        require(digest(database) == m['databaseSha256'], 'KALSHI_DATABASE_CHANGED')
        self.db = sqlite3.connect(database.resolve().as_uri() + '?mode=ro&immutable=1', uri=True)
        self.bounds, self.start, self.delivery = (lo, hi), information_start_ms, delivery
        self.tickers = set(m['tickers'])

    def page(self, ticker, now, *, from_ms=None, to_ms=None, limit=1000):
        require(integer(now) and self.bounds[0] <= now < self.bounds[1], 'KALSHI_QUERY_OUTSIDE_ROLE')
        lo = self.start if from_ms is None else from_ms
        hi = self.manifest['sourceBoundsMs'][1] if to_ms is None else to_ms
        require(integer(lo) and integer(hi) and lo <= hi and type(limit) is int and 1 <= limit <= 1000,
                'VALID_KALSHI_HISTORY_QUERY')
        if ticker not in self.tickers:
            raise KeyError(ticker)
        frontier = self.delivery.source_frontier(now)
        rows = self.db.execute('''SELECT t.at,t.ready,t.body,r.received,r.sha,r.body_sha FROM trades t
 JOIN receipts r ON r.i=t.receipt WHERE t.ticker=? AND t.at>=? AND t.at<? AND t.ready<=?
 ORDER BY t.at DESC,t.id LIMIT ?''',
                              (ticker, max(lo, self.start), min(hi, frontier + 1), frontier, limit))
        return [{'trade': json.loads(body), 'source_at_ms': at,
                 'simulated_available_at_ms': self.delivery.available(ready),
                 'actual_retrieved_at': received, 'original_receipt_sha256': sha,
                 'original_body_sha256': body_sha, 'execution_authority': False,
                 'original_historical_delivery_verified': False}
                for at, ready, body, received, sha, body_sha in rows]

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
    print(json.dumps({k: result[k] for k in ('records', 'receiptPages', 'databaseBytes', 'buildWallSeconds')}))
