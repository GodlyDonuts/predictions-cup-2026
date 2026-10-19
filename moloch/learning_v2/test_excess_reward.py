import unittest
from .account import Account
from .excess_reward import MatchedHoldReward


def account():return Account(50_000_000,0,0,0,50_000_000,0,{('a','YES'):100},{'a':50_000_000},{},{},{})


class ExcessRewardTests(unittest.TestCase):
    def test_passive_market_gain_is_zero_skill_reward(self):
        r=MatchedHoldReward(account(),100_000_000)
        self.assertEqual(r.observe(110_000_000,{('a','YES'):600_000}).normalized_increment,0)

    def test_positive_wealth_return_can_underperform_hold(self):
        r=MatchedHoldReward(account(),100_000_000)
        self.assertAlmostEqual(r.observe(105_000_000,{('a','YES'):600_000}).normalized_increment,-.05)

    def test_absolute_loss_can_outperform_hold_without_becoming_actual_profit(self):
        r=MatchedHoldReward(account(),100_000_000)
        x=r.observe(95_000_000,{('a','YES'):400_000})
        self.assertAlmostEqual(x.normalized_increment,.05);self.assertEqual(x.excess_wealth_micros,5_000_000)

    def test_unknown_baseline_or_actual_mark_propagates_and_no_interval_is_lost(self):
        r=MatchedHoldReward(account(),100_000_000)
        self.assertFalse(r.observe(100_000_000,{}).available)
        self.assertFalse(r.observe(None,{('a','YES'):500_000}).available)
        self.assertAlmostEqual(r.observe(102_000_000,{('a','YES'):500_000}).normalized_increment,.02)
        self.assertAlmostEqual(r.observe(101_000_000,{('a','YES'):500_000}).normalized_increment,-.01)

    def test_initial_portfolio_is_frozen_against_caller_mutation(self):
        a=account();r=MatchedHoldReward(a,100_000_000);a.quantities[('a','YES')]=1
        self.assertEqual(r.observe(110_000_000,{('a','YES'):600_000}).normalized_increment,0)


if __name__=='__main__':unittest.main()
