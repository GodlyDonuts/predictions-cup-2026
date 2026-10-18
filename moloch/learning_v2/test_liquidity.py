from dataclasses import replace
import unittest
from .liquidity import Book, SharedLiquidity


class SharedDepthCounterfactual(unittest.TestCase):
    def setUp(self):
        self.b = Book('x', 1000, None, ((400000, 100), (390000, 200)),
                      ((450000, 100), (460000, 200)), 'a'*64)
        self.q = SharedLiquidity(self.b)

    def cross(self, side='YES', action='BUY', limit=460000, n=300, at=1000):
        return self.q.cross(side, action, limit, n, at, 3000)

    def test_complete_depth_has_real_different_execution_prices(self):
        self.assertEqual(self.cross(), ((450000, 100), (460000, 200)))

    def test_unchanged_snapshot_and_new_receipt_do_not_restore_depth(self):
        self.cross(n=100, limit=450000)
        self.q.update(replace(self.b, available_at_ms=1100, original_sha256='b'*64))
        self.assertEqual(self.cross(n=100, limit=450000, at=1100), ())

    def test_stream_gap_blocks_matching_without_erasing_counterfactual_depletion(self):
        self.cross(n=80,limit=450000);self.q.invalidate()
        self.assertEqual(self.cross(n=100,limit=450000),())
        self.q.update(replace(self.b,available_at_ms=1100))
        self.assertEqual(self.cross(n=100,limit=450000,at=1100),((450000,20),))

    def test_yes_buy_and_no_sell_share_asks(self):
        self.cross(n=80, limit=450000)
        self.assertEqual(self.cross(side='NO', action='SELL', limit=550000, n=100), ((550000, 20),))

    def test_yes_sell_and_no_buy_share_bids(self):
        self.cross(action='SELL', limit=400000, n=80)
        self.assertEqual(self.cross(side='NO', action='BUY', limit=600000, n=100), ((600000, 20),))

    def test_changed_price_at_arrival_can_remove_the_fill(self):
        self.q.update(replace(self.b, available_at_ms=1500, asks=((470000, 100),)))
        self.assertEqual(self.cross(limit=450000, at=1500), ())

    def test_no_future_or_stale_book_fill(self):
        self.assertEqual(self.cross(at=999), ())
        self.assertEqual(self.cross(at=4001), ())

    def test_partial_fill_not_full_top_price_assumption(self):
        self.assertEqual(self.cross(limit=450000), ((450000, 100),))

    def test_adverse_price_stress_preserves_original_depth_and_obeys_limit(self):
        self.assertEqual(self.q.cross('YES','BUY',450000,100,1000,3000,adverse_execution_micros=5000),())
        self.assertEqual(self.q.cross('YES','BUY',460000,100,1000,3000,depth_fraction_ppm=500000,adverse_execution_micros=5000),((455000,50),))
        self.assertEqual(self.b.asks[0],(450000,100))
        self.assertEqual(self.q.cross('NO','SELL',540000,100,1000,3000,depth_fraction_ppm=500000,adverse_execution_micros=5000),())

    def test_advertised_increase_only_exposes_excess_above_prior_depletion(self):
        self.cross(n=100, limit=450000)
        self.q.update(replace(self.b, available_at_ms=1100, asks=((450000, 140),)))
        self.assertEqual(self.cross(at=1100, n=100, limit=450000), ((450000, 40),))

    def test_disappearing_and_returning_price_does_not_reset_depletion(self):
        self.cross(n=100, limit=450000)
        self.q.update(replace(self.b, available_at_ms=1100, asks=()))
        self.q.update(replace(self.b, available_at_ms=1200))
        self.assertEqual(self.cross(at=1200, limit=450000), ())

    def test_old_source_clock_cannot_be_refreshed_by_new_receipt(self):
        self.q.update(replace(self.b, available_at_ms=5000, source_at_ms=1000))
        self.assertEqual(self.cross(at=5000), ())

    def test_scope_or_time_reversal_rejects(self):
        for b in [replace(self.b, exchange='y'), replace(self.b, available_at_ms=999)]:
            with self.assertRaises(ValueError):
                self.q.update(b)

    def test_invalid_or_locked_levels_reject(self):
        for values in [{'asks': ((400000, 1),)}, {'bids': ((400000, 0),)}, {'asks': ((460000, 1), (450000, 1))}]:
            with self.assertRaises(ValueError):
                replace(self.b, **values)


if __name__ == '__main__':
    unittest.main()
