import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from .historical_kalshi import build, KalshiHistory, event_clocks
from .historical_samples import Delivery
from .backfill_kalshi_trades import suffix


class KalshiReplayTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root / 'pages').mkdir()
        self.out = self.root / 'compiled'
        self.plan = {'startTs': 1, 'endTs': 4, 'tickers': ['A'], 'role': 'training_history_recovery_only'}
        self.rows = [{'trade_id': str(i), 'ticker': 'A', 'created_time': timestamp,
                      'yes_price_dollars': '0.1234', 'no_price_dollars': '0.8766', 'count_fp': '1.10'}
                     for i, timestamp in enumerate(('1970-01-01T00:00:01.000500Z',
                                                     '1970-01-01T00:00:02.000000Z'))]

    def tearDown(self):
        self.temp.cleanup()

    def prepare(self):
        plan = json.dumps(self.plan).encode()
        (self.root / 'plan.json').write_bytes(plan)
        body = {'trades': self.rows, 'cursor': ''}
        text = json.dumps(body)
        receipt = {'source': 'kalshi_official_v2', 'status': 200,
            'path': '/trade-api/v2' + suffix('A', 1, 4),
            'body': body, 'bodyText': text, 'bodySha256': hashlib.sha256(text.encode()).hexdigest(),
            'requestStartedAt': '2026-10-20T00:00:00Z', 'receivedAt': '2026-10-20T00:00:01Z'}
        raw = json.dumps(receipt).encode()
        name = '0000-0000000.json'
        (self.root / 'pages' / name).write_bytes(raw)
        h = hashlib.sha256()
        h.update(name.encode())
        h.update(hashlib.sha256(raw).digest())
        report = {'planSha256': hashlib.sha256(plan).hexdigest(), 'trainingAdmitted': False,
            'receiptPages': 1, 'receiptBytes': len(raw), 'uniqueTradeRecords': len({r['trade_id'] for r in self.rows}),
            'completedMappedTickers': 1, 'sortedReceiptInventorySha256': h.hexdigest()}
        audit = self.root / 'audit.json'
        audit.write_text(json.dumps(report))
        return audit, hashlib.sha256(audit.read_bytes()).hexdigest()

    def compile(self):
        audit, sha = self.prepare()
        result = build(self.root, audit, sha, self.out)
        self.manifest_sha = hashlib.sha256((self.out / 'complete.json').read_bytes()).hexdigest()
        return result

    def test_fractional_clock_and_exact_original_trade_information(self):
        self.compile()
        with KalshiHistory(self.out, self.manifest_sha, scoring_bounds=(1000, 4000),
                           information_start_ms=1000, delivery=Delivery(0, 1, 0, 1000)) as reader:
            self.assertEqual(reader.page('A', 1000), [])
            row = reader.page('A', 1001)[0]
            self.assertEqual(row['source_at_ms'], 1000)
            self.assertEqual(row['simulated_available_at_ms'], 1001)
            self.assertEqual(row['trade']['count_fp'], '1.10')
            self.assertEqual(row['actual_retrieved_at'], '2026-10-20T00:00:01Z')
            self.assertFalse(row['execution_authority'])
            self.assertEqual(len(reader.page('A', 2000)), 2)
            self.assertEqual(len(reader.page('A', 2000, to_ms=2000)), 1)
            with self.assertRaisesRegex(ValueError, 'OUTSIDE_ROLE'):
                reader.page('A', 4000)
        self.assertEqual(event_clocks('1970-01-01T00:00:01.999999Z'), (1999, 2000))

    def test_delay_and_polling_are_applied_before_delivery(self):
        self.compile()
        with KalshiHistory(self.out, self.manifest_sha, scoring_bounds=(1000, 4000),
                           information_start_ms=1000, delivery=Delivery(200, 1000, 0, 1000)) as reader:
            self.assertEqual(reader.page('A', 1999), [])
            self.assertEqual(len(reader.page('A', 2000)), 1)
            self.assertEqual(len(reader.page('A', 3000)), 2)

    def test_duplicates_cannot_change_original_facts(self):
        self.rows.append(dict(self.rows[0], count_fp='100'))
        with self.assertRaisesRegex(ValueError, 'FACTS_CHANGED'):
            self.compile()
        self.assertFalse((self.out / 'complete.json').exists())

    def test_duplicate_same_trade_is_not_new_flow(self):
        self.rows.append(dict(self.rows[0]))
        result = self.compile()
        self.assertEqual(result['records'], 2)
        self.assertEqual(result['duplicateRows'], 1)


if __name__ == '__main__':
    unittest.main()
