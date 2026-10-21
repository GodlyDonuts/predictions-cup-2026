import unittest
from decimal import Decimal as D
from turnover_exit_census import bid_prefix, reconstruct


class ExitCensus(unittest.TestCase):
    def test_expensive_leading_lot_cannot_be_skipped(self):
        lots=[dict(price='.8',quantity=3),dict(price='.9',quantity=2),dict(price='.7',quantity=8)]
        r=bid_prefix(lots,[(D('.85'),100)],100,D('.005'))
        self.assertEqual(r['quantity'],3)
        self.assertEqual(r['displayedProceeds']-r['cost'],D('.15'))

    def test_depth_and_marginal_limit_cover_each_partial_fill(self):
        r=bid_prefix([dict(price='.8',quantity=20)],[(D('.82'),2),(D('.81'),3),(D('.8'),100)],20,D('.005'))
        self.assertEqual(r['quantity'],5); self.assertEqual(r['limit'],D('.81'))
        self.assertEqual(r['limit']*5-r['cost'],D('.05'))

    def test_lower_bid_cannot_lose_on_previous_costlier_units(self):
        r=bid_prefix([dict(price='.9',quantity=2),dict(price='.7',quantity=10)],
                     [(D('.91'),2),(D('.8'),10)],12,D('.005'))
        self.assertEqual(r['quantity'],2)

    def test_cap_does_not_invent_depth(self):
        self.assertEqual(bid_prefix([dict(price='.2',quantity=10)],[(D('.3'),8)],3,D('.005'))['quantity'],3)
        self.assertEqual(bid_prefix([dict(price='.2',quantity=10)],[],3,D('.005'))['quantity'],0)

    def test_original_partial_sales_preserve_execution_clock(self):
        buy=dict(exchangeId='1',side='NO',counterfactual=False,fee='0',action='buy',quantity=3,price='.8',executedAt='original')
        sell=dict(buy,action='sell',quantity=1,price='.9')
        self.assertEqual(reconstruct([buy,sell],'1','NO',2),[dict(quantity=2,price='.8',executedAt='original')])
        with self.assertRaises(ValueError): reconstruct([dict(buy,fee='.01')],'1','NO',2)
        with self.assertRaises(ValueError): reconstruct([sell],'1','NO',2)

    def test_disposed_opposite_side_is_not_remaining_cost_basis(self):
        buy=dict(exchangeId='1',side='NO',counterfactual=False,fee='0',action='buy',quantity=3,price='.8',executedAt='original')
        old=dict(buy,side='YES',price='.2',fee='1')
        self.assertEqual(reconstruct([old,buy],'1','NO',2),[dict(quantity=2,price='.8',executedAt='original')])


if __name__=='__main__': unittest.main()
