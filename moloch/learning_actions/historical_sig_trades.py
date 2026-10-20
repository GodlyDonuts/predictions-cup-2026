"""Audited SIG public prints as causal information, never executable depth."""
import argparse
import hashlib
import json
from decimal import Decimal
from pathlib import Path
import sqlite3
from .audit_sig_trade_history import audit
from .audit_sig_history import clock, require
from .historical_kalshi import event_clocks
from .historical_samples import Delivery, digest, integer

FORMAT = 'audited_sig_public_trade_information_v1'


def build(root, audit_path, audit_sha256, scope, output):
    root, audit_path, output = map(Path, (root, audit_path, output))
    raw_audit = audit_path.read_bytes()
    require(hashlib.sha256(raw_audit).hexdigest() == audit_sha256, 'PINNED_SIG_TRADE_AUDIT')
    proof = json.loads(raw_audit)
    fresh = audit(root, scope, start=proof['fromInclusive'],
                  train_end=proof['trainEndExclusive'], end=proof['endExclusive'])
    require({k:v for k,v in fresh.items() if k != 'observedUtc'} ==
            {k:v for k,v in proof.items() if k != 'observedUtc'}, 'SIG_TRADE_AUDIT_CHANGED')
    raw_index = (root / 'index.jsonl').read_bytes()
    output.mkdir(parents=True, exist_ok=False)
    db = sqlite3.connect(output / 'history.sqlite')
    count = 0
    try:
        db.executescript('''
CREATE TABLE receipts(i INTEGER PRIMARY KEY,file TEXT,sha TEXT,received TEXT);
CREATE TABLE trades(exchange TEXT,fact TEXT,at INTEGER,ready INTEGER,body TEXT,receipt INTEGER,PRIMARY KEY(exchange,fact));
CREATE INDEX trade_clock ON trades(exchange,at DESC,fact);
''')
        for entry in map(json.loads, raw_index.splitlines()):
            raw = (root / entry['file']).read_bytes()
            require(hashlib.sha256(raw).hexdigest() == entry['sha256'], 'SIG_TRADE_RECEIPT_CHANGED')
            receipt = json.loads(raw, parse_float=Decimal)
            db.execute('INSERT INTO receipts VALUES(?,?,?,?)',
                       (entry['page'], entry['file'], entry['sha256'], receipt['receivedAt']))
            for original in receipt['data']['data']:
                # ID spaces overlap across legacy and engine records. Preserve
                # the complete fact, including null price and exact decimals.
                row = {k: str(v) if isinstance(v, Decimal) else v for k,v in original.items()}
                body = json.dumps(row, sort_keys=True, separators=(',', ':'), allow_nan=False)
                fact = hashlib.sha256(body.encode()).hexdigest()
                at, ready = event_clocks(row['createdAt'])
                db.execute('INSERT INTO trades VALUES(?,?,?,?,?,?)',
                           (entry['exchangeId'], fact, at, ready, body, entry['page']))
                count += 1
            db.commit()
        require(count == proof['trades'], 'ALL_SIG_TRADE_FACTS_RETAINED')
    finally:
        db.close()
    require(audit_path.read_bytes() == raw_audit and (root / 'index.jsonl').read_bytes() == raw_index,
            'SIG_TRADE_SOURCE_CHANGED_DURING_BUILD')
    manifest = {'format': FORMAT, 'sourceAuditSha256': audit_sha256,
        'indexSha256': proof['indexSha256'], 'receiptInventorySha256': proof['receiptInventorySha256'],
        'normalizedFactSha256': proof['normalizedFactSha256'],
        'databaseSha256': digest(output / 'history.sqlite'),
        'databaseBytes': (output / 'history.sqlite').stat().st_size,
        'sourceBoundsMs': [clock(proof['fromInclusive']), clock(proof['endExclusive'])],
        'exchanges': scope['exchanges'], 'records': count, 'receiptPages': proof['receiptPages'],
        'nullPriceObservations': proof['nullPriceObservations'],
        'reusedLegacyEngineIdsRetainedAsDistinctFacts': proof['reusedLegacyEngineIdsRetainedAsDistinctFacts'],
        'exactDecimalStringsRetained': True, 'subMillisecondPublicationRoundedUp': True,
        'originalHistoricalDeliveryVerified': False, 'executionAuthority': False,
        'trainingAdmitted': False, 'independentTestUsed': False}
    with (output / 'complete.json').open('x') as out:
        json.dump(manifest, out, indent=2)
        out.write('\n')
    return manifest


class SigTradeHistory:
    def __init__(self, directory, manifest_sha256, *, scoring_bounds, information_start_ms, delivery):
        directory = Path(directory)
        raw = (directory / 'complete.json').read_bytes()
        require(hashlib.sha256(raw).hexdigest() == manifest_sha256, 'SIG_TRADE_MANIFEST_CHANGED')
        self.manifest = m = json.loads(raw)
        require(m['format'] == FORMAT and m['trainingAdmitted'] is False
                and m['executionAuthority'] is False and m['originalHistoricalDeliveryVerified'] is False,
                'SIG_TRADE_INFORMATION_FORMAT')
        lo, hi = scoring_bounds
        require(all(integer(x) for x in (lo,hi,information_start_ms))
                and information_start_ms <= lo < hi and isinstance(delivery,Delivery),
                'FIXED_SIG_TRADE_ROLE_AND_DELIVERY')
        database = directory / 'history.sqlite'
        require(digest(database) == m['databaseSha256'], 'SIG_TRADE_DATABASE_CHANGED')
        self.db = sqlite3.connect(database.resolve().as_uri() + '?mode=ro&immutable=1',uri=True)
        self.bounds,self.start,self.delivery = (lo,hi),information_start_ms,delivery
        self.exchanges = m['exchanges']

    def page(self, exchange, now, *, from_ms=None, to_ms=None, limit=200):
        require(integer(now) and self.bounds[0] <= now < self.bounds[1], 'SIG_TRADE_QUERY_OUTSIDE_ROLE')
        lo = self.start if from_ms is None else from_ms
        hi = self.manifest['sourceBoundsMs'][1] if to_ms is None else to_ms
        require(integer(lo) and integer(hi) and lo <= hi and type(limit) is int and 1 <= limit <= 200,
                'VALID_SIG_TRADE_HISTORY_QUERY')
        if exchange not in self.exchanges: raise KeyError(exchange)
        frontier = self.delivery.source_frontier(now)
        rows = self.db.execute('''SELECT t.at,t.ready,t.body,t.fact,r.received,r.sha FROM trades t
 JOIN receipts r ON r.i=t.receipt WHERE t.exchange=? AND t.at>=? AND t.at<? AND t.ready<=?
 ORDER BY t.at DESC,t.fact LIMIT ?''',
            (exchange,max(lo,self.start),min(hi,frontier+1),frontier,limit))
        return [{'trade':json.loads(body),'fact_sha256':fact,'source_at_ms':at,
                 'simulated_available_at_ms':self.delivery.available(ready),'actual_retrieved_at':received,
                 'original_receipt_sha256':sha,'execution_authority':False,
                 'original_historical_delivery_verified':False}
                for at,ready,body,fact,received,sha in rows]

    def close(self):self.db.close()
    def __enter__(self):return self
    def __exit__(self,*_):self.close()


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ('root','audit','audit-sha256','scope','output'):parser.add_argument('--'+name,required=True)
    args=parser.parse_args()
    result=build(args.root,args.audit,args.audit_sha256,json.loads(Path(args.scope).read_text()),args.output)
    print(json.dumps({k:result[k] for k in ('records','receiptPages','databaseBytes')}))
