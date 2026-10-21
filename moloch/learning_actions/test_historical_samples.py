import hashlib
import json
import math
import random
import tempfile
import unittest
from pathlib import Path
from .historical_samples import Delivery, Samples, RECORD
from . import historical_samples
from unittest.mock import patch


class HistoricalSamplesTests(unittest.TestCase):
    def test_vectorized_validation_matches_scalar_across_chunk_edges(self):
        receipts=[{'assetId':'A','startTs':0,'endExclusiveTs':100} for _ in range(3)]
        manifest={'startTs':0,'endExclusiveTs':100}
        def scalar(rows, receipts, manifest):
            previous=-1
            for at,price,index,flag in rows:
                if at<=previous or not math.isfinite(price) or not 0<=price<=1 or flag!=1 or index>=len(receipts):
                    raise ValueError('invalid record')
                r=receipts[index]
                if r['assetId']!='A' or not r['startTs']*1000<=at<r['endExclusiveTs']*1000 or not manifest['startTs']*1000<=at<manifest['endExclusiveTs']*1000:
                    raise ValueError('invalid scope')
                previous=at
        rng=random.Random(731)
        for case in range(300):
            rows=[(i*1000,rng.random(),rng.randrange(3),1) for i in range(7)]
            if case%10:
                i=rng.randrange(len(rows));r=list(rows[i]);field=(case%10-1)%4
                r[field]=([-1,0,100000][case%3] if field==0 else
                          [float('nan'),float('inf'),-0.1,1.1][case%4] if field==1 else
                          3 if field==2 else 0)
                rows[i]=tuple(r)
            try:scalar(rows,receipts,manifest);valid=True
            except ValueError:valid=False
            with self.subTest(case=case),patch.object(historical_samples,'VALIDATION_CHUNK_RECORDS',2):
                raw=b''.join(RECORD.pack(*r) for r in rows)
                if valid:historical_samples.validate_records(raw,'A',receipts,manifest)
                else:
                    with self.assertRaises(ValueError):historical_samples.validate_records(raw,'A',receipts,manifest)

    def test_vectorized_receipt_scope_and_unbounded_integer_endpoints(self):
        raw=b''.join(RECORD.pack(i*1000,.5,i%2,1)for i in range(7))
        receipts=[{'assetId':'A','startTs':-10**30,'endExclusiveTs':10**30}for _ in range(2)]
        manifest={'startTs':-10**30,'endExclusiveTs':10**30}
        with patch.object(historical_samples,'VALIDATION_CHUNK_RECORDS',2):
            historical_samples.validate_records(raw,'A',receipts,manifest)
            for field,value in [('assetId','B'),('startTs',2),('endExclusiveTs',3)]:
                changed=[dict(r)for r in receipts];changed[1][field]=value
                with self.subTest(field=field),self.assertRaisesRegex(ValueError,'RECEIPT_SCOPE'):
                    historical_samples.validate_records(raw,'A',changed,manifest)

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
