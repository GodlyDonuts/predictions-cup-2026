import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from .historical_samples import Delivery, Samples, RECORD


class HistoricalSamplesTests(unittest.TestCase):
    def fixture(self, root, *, rows=None):
        rows = rows if rows is not None else [(1000, .2, 0, 1), (2000, .9, 0, 1), (3000, .4, 0, 1)]
        data = b''.join(RECORD.pack(*r) for r in rows)
        (root / 'asset.bin').write_bytes(data)
        receipts = json.dumps([{'assetId': 'A', 'startTs': 1, 'endExclusiveTs': 4,
            'actualReceivedAt': '2026-10-20T06:00:00Z', 'sha256': 'a' * 64,
            'bodySha256': 'b' * 64}]).encode()
        (root / 'receipts.json').write_bytes(receipts)
        m = {'classification': 'inert_source_time_price_samples_not_replay_dataset',
            'trainingAdmitted': False, 'originalHistoricalDeliveryVerified': False,
            'recordBytes': 24, 'startTs': 1, 'endExclusiveTs': 4,
            'receiptsSha256': hashlib.sha256(receipts).hexdigest(), 'records': len(rows),
            'assets': [{'assetId': 'A', 'file': 'asset.bin', 'records': len(rows),
                        'sha256': hashlib.sha256(data).hexdigest()}]}
        raw = json.dumps(m).encode()
        (root / 'complete.json').write_bytes(raw)
        return hashlib.sha256(raw).hexdigest()

    def open(self, root, sha, **kwargs):
        return Samples(root, sha, scoring_bounds=(2000, 5000), information_start_ms=1000,
                       delivery=Delivery(250, 1000, 0, 3000), **kwargs)

    def test_delivery_does_not_reveal_future_or_relabel_retrieval(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            with self.open(root, self.fixture(root)) as samples:
                before = samples.at('A', 2999)
                self.assertEqual((before['source_at_ms'], before['price']), (1000, .2))
                after = samples.at('A', 3000)
                self.assertEqual((after['source_at_ms'], after['price']), (2000, .9))
                self.assertEqual(after['simulated_available_at_ms'], 3000)
                self.assertEqual(after['actual_retrieved_at'], '2026-10-20T06:00:00Z')
                self.assertFalse(after['executable_quote'])
                self.assertNotIn('bids', after)
                self.assertNotIn('asks', after)
                with self.assertRaisesRegex(ValueError, 'SCORING_ROLE'):
                    samples.at('A', 5000)
            with self.assertRaisesRegex(ValueError, 'CLOSED'):
                samples.at('A', 3000)

    def test_staleness_information_floor_and_empty_frontier(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder); sha = self.fixture(root)
            with Samples(root, sha, scoring_bounds=(2000, 8000), information_start_ms=2000,
                         delivery=Delivery(250, 1000, 0, 1000)) as samples:
                self.assertIsNone(samples.at('A', 2000))
                self.assertEqual(samples.at('A', 3000)['price'], .9)
                self.assertIsNone(samples.at('A', 5000))

    def test_original_hashes_and_sorted_receipt_scopes(self):
        for bad in [[(2000, .2, 0, 1), (1000, .9, 0, 1)],
                    [(2000, .2, 1, 1)], [(2000, .2, 0, 0)],
                    [(4000, .2, 0, 1)], [(2000, float('nan'), 0, 1)]]:
            with self.subTest(bad=bad), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                with self.open(root, self.fixture(root, rows=bad)) as samples:
                    with self.assertRaises(ValueError):
                        samples.at('A', 3000)
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder); sha = self.fixture(root)
            (root / 'asset.bin').write_bytes(b'x' * 72)
            with self.assertRaisesRegex(ValueError, 'ASSET_CHANGED'):
                self.open(root, sha)

    def test_delivery_boundary_arithmetic(self):
        for phase in (0, 27, 999):
            delivery = Delivery(333, 1000, phase, 2000)
            for source in (0, 1, 1000, 2674):
                ready = delivery.available(source)
                self.assertGreaterEqual(delivery.source_frontier(ready), source)
                self.assertLess(delivery.source_frontier(ready - 1), source)
        for args in [(False, 1, 0, 1), (1, 0, 0, 1), (1, 2, 2, 1)]:
            with self.assertRaises(ValueError):
                Delivery(*args)


if __name__ == '__main__':
    unittest.main()
