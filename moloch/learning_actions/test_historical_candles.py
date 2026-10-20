import hashlib
import json
import unittest
from .audit_sig_history import clock
from .historical_candles import build, CandleHistory
from .historical_samples import Delivery
from .test_audit_sig_history import HistoryAuditTest


class CandleReplayTests(unittest.TestCase):
    def setUp(self):
        self.fixture = HistoryAuditTest()
        self.fixture.setUp()
        self.root = self.fixture.root
        self.audit = self.root / 'audit.json'
        self.audit.write_text(json.dumps(self.fixture.run_audit()))
        self.audit_sha = hashlib.sha256(self.audit.read_bytes()).hexdigest()
        self.out = self.root / 'compiled'

    def tearDown(self):
        self.fixture.tearDown()

    def compile(self):
        result = build(self.root, self.audit, self.audit_sha, self.out)
        self.manifest_sha = hashlib.sha256((self.out / 'complete.json').read_bytes()).hexdigest()
        return result

    def reader(self, delay=61000):
        return CandleHistory(self.out, self.manifest_sha,
            scoring_bounds=(clock(self.fixture.first), clock(self.fixture.end)),
            information_start_ms=clock(self.fixture.start),
            delivery=Delivery(delay, 1000, 0, 120000))

    def test_exact_fields_roundtrip_and_candle_close_visibility(self):
        result = self.compile()
        self.assertEqual(result['records'], 2)
        self.assertEqual(result['storedDecodedSha256'], result['normalizedScopedCandleSha256'])
        first = clock(self.fixture.first)
        with self.reader() as reader:
            self.assertEqual(reader.page('840', first + 60999)['candles'], [])
            row = reader.page('840', first + 61000)['candles'][0]
            self.assertEqual(row['open'], '0.0600000000000001')
            self.assertEqual(row['vwap'], '0.0612345678901234')
            self.assertEqual(row['volume'], '20')
            self.assertEqual(row['tradeCount'], 3)
            self.assertEqual(row['bar_end_ms'], first + 60000)
            self.assertEqual(row['actual_retrieved_at'], '2026-10-20T00:00:01Z')
            self.assertFalse(reader.page('840', first + 61000)['executable_liquidity'])

    def test_roles_and_exclusive_paging_do_not_reveal_future_bar(self):
        self.compile()
        split = clock(self.fixture.split)
        with self.reader() as reader:
            self.assertEqual(len(reader.page('840', split)['candles']), 1)
            self.assertEqual(len(reader.page('840', split + 61000)['candles']), 2)
            self.assertEqual(len(reader.page('840', split + 61000,
                                           through_exclusive_ms=split)['candles']), 1)
            newest = reader.page('840', split + 61000, limit=1)['candles']
            self.assertEqual(newest[0]['source_at_ms'], split)
            with self.assertRaisesRegex(ValueError, 'OUTSIDE_ROLE'):
                reader.page('840', clock(self.fixture.end))
            with self.assertRaisesRegex(ValueError, 'PAGE_LIMIT'):
                reader.page('840', split, limit=1001)

    def test_aggregate_delay_is_mandatory(self):
        self.compile()
        with self.assertRaisesRegex(ValueError, 'CLOSE_BEFORE_PUBLICATION'):
            self.reader(delay=59999)

    def test_tampering_does_not_produce_complete_dataset(self):
        receipt = self.root / self.fixture.entries[0]['file']
        receipt.write_text('{}')
        with self.assertRaisesRegex(ValueError, 'AUDITED_RECEIPT_CHANGED'):
            self.compile()
        self.assertFalse((self.out / 'complete.json').exists())

    def test_read_database_hash_and_audit_pin_are_enforced(self):
        with self.assertRaisesRegex(ValueError, 'PINNED_CANDLE_AUDIT'):
            build(self.root, self.audit, '0' * 64, self.out)
        self.compile()
        with (self.out / 'history.sqlite').open('ab') as stream:
            stream.write(b'changed')
        with self.assertRaisesRegex(ValueError, 'DATABASE_CHANGED'):
            self.reader()


if __name__ == '__main__':
    unittest.main()
