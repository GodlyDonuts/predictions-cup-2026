import copy
from decimal import Decimal
import gzip
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('maker_postfill', Path(__file__).with_name('analyze.py'))
A = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(A)
BASE = 1792108800000


def stamp(ms):
    from datetime import datetime, timezone
    return datetime.fromtimestamp((BASE + ms) / 1000, timezone.utc).isoformat(timespec='milliseconds').replace('+00:00', 'Z')


def fixture(side='YES', n=3):
    q = dict(id=1, marketId='1', exchangeId='101', side=side, action='buy', quantity=n,
             filled=n, price='0.400000', createdAt=BASE, retired=False)
    f = dict(orderId=1, actualOrderId='901', sourceId='801', marketId='1', exchangeId='101',
             side=side, action='buy', quantity=n, price='0.400000', fee='0.000000',
             executedAt=stamp(1000), at=stamp(20000), receiptHash='a'*64, counterfactual=False)
    s = dict(report=dict(executionMode='live', fillModel='actualAccountFills', fills=1, at=stamp(200000)), orders=[q], fills=[f])
    d = [dict(event=dict(kind='intent', qid=1, fundamental=None)), dict(event=dict(kind='ack', qid=1,
         receipt=dict(endpoint='/orders', status=200, requestStartedAt=stamp(100), receivedAt=stamp(200), data=dict(orderId='901'))))]
    return s, d


def book(at, bids=None, asks=None, rev=1, received=None, resync=False):
    b = dict(exchangeId='101', asOf=dict(at=stamp(at), sequence=rev),
             bids=bids or [dict(price='0.450000', quantity=1), dict(price='0.440000', quantity=2)],
             asks=asks or [dict(price='0.550000', quantity=3)])
    batch = dict(delivery=dict(model='best-effort-authoritative-resync', revision=rev, previousRevision=rev-1),
                 books=[b], resyncRequired=resync)
    return dict(hash=str(rev % 10)*64, event=dict(source='official_sig_realtime', event='markets_batch',
                 receivedAt=stamp(at if received is None else received), payload=dict(markets=[dict(marketId='1', batch=batch)])))


def sealed(body):
    data = json.dumps(body, sort_keys=True, separators=(',', ':')).encode()
    return json.dumps(dict(body, hash=hashlib.sha256(data).hexdigest()), sort_keys=True, separators=(',', ':')).encode() + b'\n'


class Tests(unittest.TestCase):
    def test_yes_full_depth_cost_not_top_bid(self):
        s, d = fixture(); r = A.analyze(s, [book(2000)], d)
        h = r['aggregate']['horizonsMs']['1000']
        self.assertEqual(h['hypotheticalLiquidationMinusEntryAndEntryFees'], '0.130000')
        self.assertEqual(h['fullDepthShares'], 3)
        self.assertEqual(h['labelsBeforeNativeFillDiscovery'], 1)

    def test_no_uses_complement_of_asks(self):
        s, d = fixture('NO'); b = book(2000, asks=[dict(price='0.550000', quantity=1), dict(price='0.560000', quantity=2)])
        self.assertEqual(A.analyze(s, [b], d)['aggregate']['horizonsMs']['1000']['hypotheticalLiquidationMinusEntryAndEntryFees'], '0.130000')

    def test_shallow_first_is_not_replaced_by_favorable_deep_book(self):
        s, d = fixture(); first = book(2000, bids=[dict(price='0.450000', quantity=1)])
        r = A.analyze(s, [first, book(2100, rev=2)], d)['aggregate']['horizonsMs']['1000']
        self.assertEqual(r['coverage'], {'insufficient_depth': 1}); self.assertIsNone(r['hypotheticalLiquidationMinusEntryAndEntryFees'])
        self.assertEqual(r['displayedCoveredSharesIncludingPartial'], 1)

    def test_partial_fills_same_intent_and_exact_duplicate_not_doublecounted(self):
        s, d = fixture(n=3); s['fills'][0]['quantity'] = 1
        f = copy.deepcopy(s['fills'][0]); f.update(sourceId='802', quantity=2, executedAt=stamp(1100))
        s['fills'] += [f, copy.deepcopy(f)]; s['report']['fills'] = 2
        r = A.analyze(s, [book(2100)], d)
        self.assertEqual(r['duplicateActualFillObservations'], 1)
        self.assertEqual(r['aggregate']['actualShares'], 3); self.assertEqual(r['aggregate']['intentCohorts'], 1)
        self.assertEqual(r['aggregate']['horizonsMs']['1000']['nonOverlappingMarketTimeClusters'], 1)

    def test_changed_duplicate_rejected(self):
        s, d = fixture(); f = copy.deepcopy(s['fills'][0]); f['quantity'] = 1; s['fills'].append(f)
        with self.assertRaises(ValueError): A.analyze(s, [], d)

    def test_fundamental_and_package_excluded(self):
        s, d = fixture(); d[0]['event']['fundamental'] = {'review': 'not ordinary'}
        self.assertEqual(A.analyze(s, [], d)['aggregate']['actualShares'], 0)
        s, d = fixture(); s['report']['livePackages'] = {'records': [{'legs': [{'qid': 1}]}]}
        self.assertEqual(A.analyze(s, [], d)['aggregate']['actualShares'], 0)

    def test_ack_and_attribution_missing_not_invented(self):
        s, d = fixture(); self.assertEqual(A.analyze(s, [], None)['attribution'], {'missing_original_intent': 1})
        d[1]['event']['receipt']['receivedAt'] = stamp(1000)
        self.assertEqual(A.analyze(s, [], d)['attribution'], {'not_proved_rested_after_ack': 1})

    def test_stale_receipt_and_missing_windows_remain_unavailable(self):
        s, d = fixture(); r = A.analyze(s, [book(2000, received=13000)], d)
        self.assertEqual(r['aggregate']['horizonsMs']['1000']['coverage'], {'stale_source': 1})
        self.assertEqual(r['aggregate']['horizonsMs']['5000']['coverage'], {'no_original_book_in_horizon_window': 1})

    def test_censored_future_and_resync_do_not_create_marks(self):
        s, d = fixture(); s['report']['at'] = stamp(21000)
        r = A.analyze(s, [book(2000, resync=True)], d)
        self.assertEqual(r['aggregate']['horizonsMs']['30000']['coverage'], {'right_censored': 1})
        self.assertEqual(r['aggregate']['horizonsMs']['1000']['coverage'], {'no_original_book_in_horizon_window': 1})

    def test_actual_fees_expensed_not_guessed_future_exit_fee(self):
        s, d = fixture(); s['fills'][0]['fee'] = '0.010000'
        r = A.analyze(s, [book(2000)], d)['aggregate']['horizonsMs']['1000']
        self.assertEqual(r['hypotheticalLiquidationMinusEntryAndEntryFees'], '0.120000'); self.assertFalse(r['exitFeesKnown'])

    def test_official_float_representation_not_arbitrary_rounding(self):
        self.assertEqual(A.levels([dict(price=Decimal('0.4500000000000001'), quantity=3)], True), [(450000, 3)])
        with self.assertRaises(ValueError): A.levels([dict(price='0.450001', quantity=3)], True)

    def test_original_hash_chain_gzip_and_tamper(self):
        with tempfile.TemporaryDirectory() as td:
            p = Path(td)/'a.gz'; raw = sealed(dict(event={'test': 1}, previous='', sequence=1))
            p.write_bytes(gzip.compress(raw, mtime=0)); digest = hashlib.sha256(p.read_bytes()).hexdigest()
            A.pinned(p, digest, p.stat().st_size); self.assertEqual(len(list(A.rows(p, ''))), 1)
            p.write_bytes(gzip.compress(raw.replace(b'"test":1', b'"test":2'), mtime=0))
            with self.assertRaises(ValueError): list(A.rows(p, ''))

    def test_subsequent_bid_movement_separate_from_entry_spread(self):
        s, d = fixture(); before = book(900, bids=[dict(price='0.350000', quantity=3)])
        r = A.analyze(s, [before, book(2000, rev=2)], d)['aggregate']['horizonsMs']['1000']
        self.assertEqual(r['futureMinusCausalPrefillLiquidationValue'], '0.280000')
        self.assertEqual(r['hypotheticalLiquidationMinusEntryAndEntryFees'], '0.130000')

    def test_prefill_old_source_received_after_execution_is_not_causal(self):
        s, d = fixture(); before = book(900, received=1100)
        r = A.analyze(s, [before, book(2000, rev=2)], d)['aggregate']['horizonsMs']['1000']
        self.assertIsNone(r['futureMinusCausalPrefillLiquidationValue'])

    def test_prefill_gap_and_stale_are_not_backfilled(self):
        s, d = fixture(); before = book(700); gap = book(800, rev=2, resync=True)
        r = A.analyze(s, [before, gap, book(2000, rev=3)], d)['aggregate']
        self.assertEqual(r['causalPrefillCoverage'], {'prefill_invalidated_by_gap': 1})
        self.assertIsNone(r['horizonsMs']['1000']['futureMinusCausalPrefillLiquidationValue'])


if __name__ == '__main__':
    unittest.main()
