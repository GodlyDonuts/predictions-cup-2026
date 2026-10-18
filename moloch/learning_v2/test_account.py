import unittest
from dataclasses import replace
from .account import Account, Limits, Target, project, wealth, observation, UNIT as U


def cash_start(amount=100_000):
    return Account(amount*U, 0, 0, 0, 0, 0, {}, {}, {}, {}, {})


class ExactTargetSizing(unittest.TestCase):
    def setUp(self):
        self.a = cash_start()
        self.m = {('x', 'YES'): 400_000, ('x', 'NO'): 600_000}
        self.l = Limits(10_000, 100_000*U, 10_000*U)
        self.t = Target('x', 'YES', 20_000, 1_000_000, 400_000, 'BUY')

    def test_exact_original_example(self):
        p = project(self.a, self.m, self.t, self.l)
        self.assertEqual((p.requested_quantity, p.admitted_quantity, p.maximum_cost), (5000, 5000, 2000*U))

    def test_fixed_depth_does_not_scale_with_bankroll(self):
        p = project(cash_start(1_000_000), self.m, self.t, replace(self.l, maximum_committed=2_000_000*U, maximum_market_cost=2_000_000*U), 6000)
        self.assertEqual((p.requested_quantity, p.admitted_quantity), (50_000, 6000))

    def test_same_target_pending_is_not_a_second_purchase(self):
        a = replace(self.a, reserved_cash=2000*U, reserved_commitment=2000*U,
                    pending_buys={('x', 'YES'): 5000}, pending_market_cost={'x': 2000*U})
        self.assertEqual(project(a, self.m, self.t, self.l).admitted_quantity, 0)
        self.assertEqual(wealth(a, self.m), wealth(self.a, self.m))

    def test_same_target_after_fill_is_not_a_second_purchase(self):
        a = replace(self.a, cash=98_000*U, quantities={('x', 'YES'): 5000}, owned_cost=2000*U, market_cost={'x': 2000*U})
        self.assertEqual(project(a, self.m, self.t, self.l).admitted_quantity, 0)

    def test_price_limit_and_fee_determine_funding_not_mark(self):
        a = replace(self.a, cash=1000*U)
        p = project(a, self.m, replace(self.t, target_fraction_ppm=U, limit_price=500_000), replace(self.l, fee_per_share=1))
        self.assertEqual(p.admitted_quantity, 1999)
        self.assertLessEqual(p.maximum_cost, a.cash)

    def test_reserved_and_encumbered_cash_not_spendable(self):
        a = replace(self.a, reserved_cash=99_000*U, other_cash_encumbrances=500*U)
        self.assertEqual(project(a, self.m, self.t, self.l).admitted_quantity, 1250)

    def test_pending_sales_not_spendable(self):
        a = replace(cash_start(0), quantities={('x', 'YES'): 1000}, pending_sells={('x', 'YES'): 1000})
        self.assertEqual(project(a, self.m, replace(self.t, target_fraction_ppm=U), self.l).admitted_quantity, 0)

    def test_total_and_market_commitments_include_outstanding_orders(self):
        a = replace(self.a, owned_cost=99_000*U, reserved_commitment=900*U,
                    market_cost={'x': 9000*U}, pending_market_cost={'x': 900*U})
        self.assertEqual(project(a, self.m, self.t, self.l).admitted_quantity, 250)

    def test_no_uncovered_sell_after_other_pending_sell(self):
        a = replace(self.a, quantities={('x', 'YES'): 1000}, pending_sells={('x', 'YES'): 700})
        sell = replace(self.t, target_fraction_ppm=0, action='SELL')
        self.assertEqual(project(a, self.m, sell, self.l).admitted_quantity, 300)

    def test_buy_opposite_targets_net_exposure_and_still_requires_gross_cash(self):
        a = replace(self.a,quantities={('x','YES'):100})
        close = Target('x','NO',0,U,600000,'BUY')
        p = project(a,self.m,close,self.l)
        self.assertEqual((p.requested_quantity,p.admitted_quantity,p.maximum_cost),(100,100,60*U))
        unfunded = replace(a,cash=0)
        self.assertEqual(project(unfunded,self.m,close,self.l).admitted_quantity,0)

    def test_shares_cannot_back_pending_sell_and_new_opposite_buy(self):
        a = replace(self.a,quantities={('x','YES'):100},pending_sells={('x','YES'):70})
        p = project(a,self.m,Target('x','NO',U,U,600000,'BUY'),self.l)
        self.assertEqual(p.admitted_quantity,30)
        with self.assertRaisesRegex(ValueError,'SAME_SHARES_PLEDGED'):
            replace(a,pending_buys={('x','NO'):31})

    def test_pending_opposite_buy_already_encumbers_sale_shares(self):
        a = replace(self.a,quantities={('x','YES'):100},pending_buys={('x','NO'):40})
        p = project(a,self.m,Target('x','YES',0,U,400000,'SELL'),self.l)
        self.assertEqual(p.admitted_quantity,60)

    def test_optional_position_cap_is_distinct_from_per_order_cap(self):
        a = replace(self.a, quantities={('x', 'YES'): 1000}, pending_buys={('x', 'YES'): 500})
        p = project(a, self.m, self.t, replace(self.l, maximum_position_quantity=1700))
        self.assertEqual(p.admitted_quantity, 200)

    def test_fresh_slot_budget_counts_pending_buys_and_preserves_inherited_classification(self):
        a = replace(self.a, quantities={('old','YES'):5}, pending_buys={('new','YES'):1})
        limits = replace(self.l, maximum_fresh_markets=1, inherited_exchanges=frozenset(['old']))
        marks = dict(self.m)
        marks[('old','YES')]=400000;marks[('new','YES')]=400000
        p = project(a,marks,self.t,limits)
        self.assertEqual(p.admitted_quantity,0)
        self.assertIn('MARKET_SLOTS',p.constraints)
        same = replace(self.t,exchange='new')
        self.assertGreater(project(a,marks,same,limits).admitted_quantity,0)

    def test_small_child_budget_rounds_down(self):
        p = project(self.a, self.m, replace(self.t, child_budget_fraction_ppm=1), self.l)
        self.assertEqual(p.admitted_quantity, 0)

    def test_unknown_held_mark_propagates_unavailability(self):
        a = replace(self.a, quantities={('unknown', 'YES'): 1})
        self.assertFalse(observation(a, self.m, 1_100_000*U)['available'])
        self.assertEqual(project(a, self.m, self.t, self.l).admitted_quantity, 0)

    def test_unknown_flat_contract_does_not_destroy_wealth(self):
        self.assertEqual(wealth(replace(self.a, quantities={('unknown', 'YES'): 0}), self.m), self.a.cash)

    def test_nonpositive_wealth_never_uses_arbitrary_epsilon(self):
        self.assertIsNone(wealth(replace(self.a, liabilities=self.a.cash), self.m))

    def test_requested_and_admitted_both_remain_visible(self):
        p = project(self.a, self.m, self.t, self.l, available_depth=17)
        self.assertEqual((p.requested_quantity, p.admitted_quantity), (5000, 17))
        self.assertIn('DISPLAYED_DEPTH', p.constraints)

    def test_invalid_or_overreserved_account_rejects(self):
        for changes in [{'cash': True}, {'reserved_cash': self.a.cash+1}, {'pending_sells': {('x', 'YES'): 1}}]:
            with self.assertRaises(ValueError):
                replace(self.a, **changes)

    def test_policies_cannot_request_bad_fraction_or_price(self):
        for changes in [{'target_fraction_ppm': U+1}, {'limit_price': 0}, {'side': 'UNKNOWN'}, {'child_budget_fraction_ppm': -1}]:
            with self.assertRaises(ValueError):
                replace(self.t, **changes)


if __name__ == '__main__':
    unittest.main()
