from dataclasses import replace
from itertools import product
import unittest
from .account import Account,UNIT
from .interval_excess_reward import IntervalHoldReward


def account(cash,quantities):return Account(cash,0,0,0,0,0,quantities,{},{},{},{})


class IntervalRewardTests(unittest.TestCase):
    def test_bounds_match_all_joint_endpoints_with_complementary_sides(self):
        initial=account(50*UNIT,{('a','YES'):10,('a','NO'):4,('b','NO'):9})
        actual=account(53*UNIT,{('a','YES'):3,('a','NO'):8,('b','YES'):5})
        intervals={'a':(300000,700000),'b':(400000,600000)}
        result=IntervalHoldReward(initial,100*UNIT).observe(actual,intervals)
        def wealth(a,prices):return a.cash+sum(q*(prices[e]if s=='YES'else UNIT-prices[e])for(e,s),q in a.quantities.items())
        endpoints=[wealth(actual,dict(zip(intervals,p)))-wealth(initial,dict(zip(intervals,p)))for p in product(*intervals.values())]
        self.assertEqual((result.lower_excess_micros,result.upper_excess_micros),(min(endpoints),max(endpoints)))

    def test_midpoint_gain_is_not_robust_when_spread_can_reverse_it(self):
        initial=account(0,{('a','YES'):10});actual=account(5_500_000,{})
        result=IntervalHoldReward(initial,5*UNIT).observe(actual,{'a':(400000,600000)})
        self.assertEqual(result.lower_excess_micros,-500000);self.assertEqual(result.upper_excess_micros,1500000)

    def test_equal_unmarked_inventory_cancels_without_inventing_absolute_wealth(self):
        initial=account(5*UNIT,{('unknown','YES'):100})
        result=IntervalHoldReward(initial,10*UNIT).observe(initial,{})
        self.assertTrue(result.available);self.assertEqual(result.lower_excess_micros,0)

    def test_complete_pair_has_known_relative_payout_without_external_marks(self):
        initial=account(100*UNIT,{})
        result=IntervalHoldReward(initial,100*UNIT).observe(account(91*UNIT,{('a','YES'):10,('a','NO'):10}),{})
        self.assertEqual((result.lower_excess_micros,result.upper_excess_micros),(UNIT,UNIT))

    def test_missing_changed_exposure_stays_unknown_and_recovers_full_increment(self):
        initial=account(100*UNIT,{});actual=account(95*UNIT,{('a','YES'):10});r=IntervalHoldReward(initial,100*UNIT)
        self.assertFalse(r.observe(actual,{}).available)
        self.assertEqual(r.observe(actual,{'a':(600000,700000)}).normalized_lower_increment,.01)
        self.assertEqual(r.observe(replace(actual,reserved_cash=4*UNIT),{'a':(600000,700000)}).normalized_lower_increment,0)
        self.assertAlmostEqual(r.observe(actual,{'a':(500000,700000)}).normalized_lower_increment,-.01)


if __name__=='__main__':unittest.main()
