import gzip
import json
from pathlib import Path
import tempfile
import unittest
import audit


class ExactClockAudit(unittest.TestCase):
    def test_trade_after_export_is_only_receipt_upper_bound_not_original_available_flow(self):
        trade = {'sourceMs': 9067, 'receivedMs': 9063, 'id': 'original'}
        received, exported = audit.eligibility([trade], 9748, 8991, 1000)
        self.assertEqual(received, [trade]); self.assertEqual(exported, [])

    def test_source_window_excludes_exact_left_boundary_future_and_late_receipt(self):
        rows = [{'sourceMs': s, 'receivedMs': r} for s, r in [(9000, 9000), (10001, 9990), (9990, 10001), (9001, 9999)]]
        self.assertEqual(audit.eligibility(rows, 10000, 10000, 1000), ([rows[-1]], [rows[-1]]))

    def test_old_export_cannot_be_retimed_to_create_one_second_flow(self):
        r = {'sourceMs': 8500, 'receivedMs': 8501}
        self.assertEqual(audit.eligibility([r], 10000, 8900, 1000), ([], []))
        self.assertEqual(audit.eligibility([r], 10000, 8900, 5000), ([r], [r]))

    def test_sealed_hash_change_is_rejected_before_observations_are_used(self):
        with tempfile.TemporaryDirectory() as directory:
            p = Path(directory)/'original.gz'; raw = gzip.compress(b'{"kind":"original"}\n'); p.write_bytes(raw)
            self.assertEqual(audit.sealed(p, audit.digest(raw))[0], [{'kind': 'original'}])
            p.write_bytes(gzip.compress(b'{"kind":"changed"}\n'))
            with self.assertRaisesRegex(ValueError, 'SOURCE_HASH'):
                audit.sealed(p, audit.digest(raw))

    def test_duplicate_trade_identity_conflict_and_raw_body_change_are_rejected(self):
        def row(ts):
            body = {'msg': {'trade_id': 'same', 'market_ticker': 'ORIGINAL', 'ts_ms': ts}}
            text = json.dumps(body)
            return {'kind': 'websocket', 'type': 'trade', 'body': body, 'bodyText': text, 'bodySha256': audit.digest(text.encode()), 'receivedAt': '2026-10-16T12:00:00.000Z'}
        self.assertEqual(sum(map(len, audit.collect([row(1000), row(1000)])[0].values())), 1)
        with self.assertRaisesRegex(ValueError, 'CONFLICTING_TRADE_ID'):
            audit.collect([row(1000), row(1001)])
        changed = row(1000); changed['bodyText'] += ' '
        with self.assertRaisesRegex(ValueError, 'TRADE_ORIGINAL_BODY'):
            audit.collect([changed])


if __name__ == '__main__':
    unittest.main()
