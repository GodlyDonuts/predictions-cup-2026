import hashlib
import json
import unittest
from urllib.parse import urlencode

from .audit_sig_trade_history import audit
from .test_audit_sig_history import HistoryAuditTest


class TradeAuditTest(unittest.TestCase):
    def setUp(self):
        self.fixture = HistoryAuditTest()
        self.fixture.setUp()
        f = self.fixture
        self.entries = []
        for n, t in enumerate([f.split, f.first]):
            cursor = None if n == 0 else 'page_two'
            query = {'tournamentId': 'cup', 'from': f.start, 'to': f.end, 'limit': '200'}
            if cursor:
                query['cursor'] = cursor
            receipt = {'endpoint': '/exchanges/840/trades?' + urlencode(query), 'status': 200,
                'request_started_at': '2026-10-20T00:00:00Z', 'received_at': '2026-10-20T00:00:01Z',
                'data': {'exchangeId': '840', 'marketId': '151', 'from': f.start, 'to': f.end,
                    'data': [{'id': '4', 'createdAt': t, 'price': .0600000000000001,
                              'volume': .6000000000000001, 'size': 10, 'side': 'NO'}],
                    'coverage': {'complete': True, 'projectedThroughSequence': 100},
                    'pagination': {'limit': 200, 'hasMore': n == 0,
                                   'nextCursor': 'page_two' if n == 0 else None}}}
            name = f'page-{n:06}-validated.json'
            raw = json.dumps(receipt).encode()
            (f.root / name).write_bytes(raw)
            self.entries.append({'page': n, 'exchangeId': '840', 'file': name,
                'sha256': hashlib.sha256(raw).hexdigest(), 'fromInclusive': f.start, 'throughExclusive': f.end,
                'requestCursor': cursor, 'nextCursor': receipt['data']['pagination']['nextCursor'],
                'trades': 1, 'retrievedAt': receipt['received_at'], 'trainingAdmitted': False,
                'bookDepthReconstructed': False})
        self.write_index()
        (f.root / 'complete.json').write_text(json.dumps({'requests': 2, 'trades': 2,
            'exchangesComplete': 1, 'fromInclusive': f.start, 'throughExclusive': f.end}))

    def tearDown(self):
        self.fixture.tearDown()

    def write_index(self):
        (self.fixture.root / 'index.jsonl').write_text(''.join(json.dumps(x) + '\n' for x in self.entries))

    def run_audit(self):
        f = self.fixture
        return audit(f.root, f.scope, start=f.start, train_end=f.split, end=f.end)

    def edit_page(self, n, change):
        file = self.fixture.root / self.entries[n]['file']
        value = json.loads(file.read_text())
        change(value)
        raw = json.dumps(value).encode()
        file.write_bytes(raw)
        self.entries[n]['sha256'] = hashlib.sha256(raw).hexdigest()
        self.write_index()

    def test_roles_precision_and_reused_legacy_id_are_preserved(self):
        r = self.run_audit()
        self.assertEqual(r['roleCounts'], {'train': 1, 'evolution': 1})
        self.assertEqual(r['reusedLegacyEngineIdsRetainedAsDistinctFacts'], 1)
        self.assertFalse(r['ownedFillsReconstructed'])

    def test_exclusive_end_prevents_future_trade(self):
        self.edit_page(0, lambda r: r['data']['data'][0].update(createdAt=self.fixture.end))
        with self.assertRaisesRegex(ValueError, 'TRADE_EVENT'):
            self.run_audit()

    def test_cursor_gap_is_rejected(self):
        self.entries[1]['requestCursor'] = 'wrong_page'
        self.write_index()
        with self.assertRaisesRegex(ValueError, 'TRADE_CURSOR_CHAIN'):
            self.run_audit()

    def test_null_price_is_unknown_observation(self):
        self.edit_page(0, lambda r: r['data']['data'][0].update(price=None))
        self.assertEqual(self.run_audit()['nullPriceObservations'], 1)

    def test_original_hash_change_is_rejected(self):
        (self.fixture.root / self.entries[0]['file']).write_text('{}')
        with self.assertRaisesRegex(ValueError, 'TRADE_RECEIPT_HASH'):
            self.run_audit()


if __name__ == '__main__':
    unittest.main()
