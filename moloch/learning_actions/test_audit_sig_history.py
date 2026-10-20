import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from urllib.parse import urlencode

from .audit_sig_history import audit, exact
from decimal import Decimal


class HistoryAuditTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='moloch-sig-history-audit-')
        self.root = Path(self.temp.name)
        self.scope = {'tournamentId': 'cup', 'exchanges': {'840': '151'}}
        self.start, self.split, self.end = '2026-09-01T00:00:00Z', '2026-10-02T00:00:00Z', '2026-10-03T00:00:00Z'
        self.first = '2026-10-01T00:00:00Z'
        rows = []
        for t in [self.first, self.split]:
            rows.append({'time': t, 'open': .0600000000000001, 'high': .07, 'low': .05,
                         'close': .06, 'vwap': .0612345678901234, 'volume': 20, 'tradeCount': 3})
        self.entries = []
        for n, (through, candles) in enumerate([(self.end, rows), (self.first, [])]):
            name = f'page-{n:06}-validated.json'
            receipt = {'endpoint': '/exchanges/840/price-history?' + urlencode({
                'tournamentId': 'cup', 'resolution': '1m', 'limit': '500', 'to': through}),
                'status': 200, 'requestStartedAt': '2026-10-20T00:00:00Z',
                'receivedAt': '2026-10-20T00:00:01Z', 'data': {
                    'exchangeId': '840', 'marketId': '151', 'resolution': '1m',
                    'from': self.first, 'to': through, 'candles': candles,
                    'coverage': {'complete': True, 'projectedThroughSequence': 100}}}
            raw = json.dumps(receipt).encode()
            (self.root / name).write_bytes(raw)
            self.entries.append({'page': n, 'exchangeId': '840', 'file': name,
                'sha256': hashlib.sha256(raw).hexdigest(), 'candles': len(candles),
                'requestedThroughExclusive': through, 'nextThroughExclusive': self.first if n == 0 else None,
                'retrievedAt': receipt['receivedAt'], 'trainingAdmitted': False,
                'bookDepthReconstructed': False})
        (self.root / 'complete.json').write_text(json.dumps({'requests': 2, 'candles': 2,
            'exchangesComplete': 1, 'throughExclusive': self.end, 'fromInclusive': self.start}))
        self.write_index()

    def tearDown(self):
        self.temp.cleanup()

    def write_index(self):
        (self.root / 'index.jsonl').write_text(''.join(json.dumps(x) + '\n' for x in self.entries))

    def run_audit(self):
        return audit(self.root, self.scope, start=self.start, train_end=self.split, end=self.end)

    def test_precision_and_chronological_roles(self):
        result = self.run_audit()
        self.assertEqual(result['roleCounts'], {'train': 1, 'evolution': 1})
        self.assertFalse(result['trainingAdmitted'])
        self.assertEqual(exact(Decimal('0.0600000000000001'), 1), '0.0600000000000001')
        with self.assertRaises(ValueError):
            exact(Decimal('1.00000000000000001'), 1)

    def test_modified_receipt_is_rejected(self):
        (self.root / self.entries[0]['file']).write_text('{}')
        with self.assertRaisesRegex(ValueError, 'RECEIPT_HASH_CHANGED'):
            self.run_audit()

    def test_cursor_gap_is_rejected(self):
        self.entries[1]['requestedThroughExclusive'] = self.split
        self.write_index()
        with self.assertRaisesRegex(ValueError, 'EXCLUSIVE_BACKWARD_CURSOR_CHAIN'):
            self.run_audit()

    def test_missing_original_market_is_rejected(self):
        self.scope['exchanges']['841'] = '152'
        with self.assertRaisesRegex(ValueError, 'FULL_ORIGINAL_SCOPE_REQUIRED'):
            self.run_audit()

    def test_boundary_filter_precedes_bad_out_of_range_price(self):
        self.start = '2026-10-01T01:00:00Z'
        file = self.root / self.entries[0]['file']
        v = json.loads(file.read_text())
        v['data']['candles'][0]['open'] = 'QUARANTINED_OUTSIDE_ROLE'
        raw = json.dumps(v).encode()
        file.write_bytes(raw)
        self.entries[0]['sha256'] = hashlib.sha256(raw).hexdigest()
        # The floor now stops after the first page; retain the immutable old
        # fixture files, but the new complete index has only its real page.
        self.entries = self.entries[:1]
        self.entries[0]['nextThroughExclusive'] = None
        self.write_index()
        (self.root / 'complete.json').write_text(json.dumps({'requests': 1, 'candles': 2,
            'exchangesComplete': 1, 'throughExclusive': self.end, 'fromInclusive': self.start}))
        r = self.run_audit()
        self.assertEqual(r['excludedOutsideRangeBeforeValues'], 1)
        self.assertEqual(r['roleCounts'], {'evolution': 1})


if __name__ == '__main__':
    unittest.main()
