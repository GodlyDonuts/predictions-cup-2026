import unittest
import torch
from .test_fused_batch import StaticTape, BatchedWorlds
from .policy import RecurrentPolicy
from .run_experiment import rollout


class MovingTape(StaticTape):
    def episode(self,*args,**kwargs):
        env=super().episode(*args,**kwargs)
        env.add_reference('x',700000,2000,'e'*64,source_kind='Kalshi')
        return env


class ExcessRolloutTests(unittest.TestCase):
    def setUp(self):
        setup=BatchedWorlds();setup.setUp();self.m=setup.m;self.p=setup.p
        torch.manual_seed(47001);self.model=RecurrentPolicy(24,14,32)

    def test_passive_price_gain_does_not_become_skill_or_change_wallet(self):
        raw,actual=rollout(MovingTape(),self.m,self.p,self.model,1000,baseline=True)
        t,relative=rollout(MovingTape(),self.m,{**self.p,'training_reward_objective':'matched_hold_excess'},self.model,1000,baseline=True)
        self.assertGreater(actual['reference_return'],0)
        self.assertEqual(relative['reference_return'],actual['reference_return'])
        self.assertEqual(float(t.rewards.sum()),0.)
        self.assertEqual(relative['terminal_excess_wealth_micros'],0)
        for key in ['terminal_cash_micros','terminal_owned_cost_micros','terminal_owned_shares','fill_receipts','fees_micros']:
            self.assertEqual(actual[key],relative[key])

    def test_reward_telescopes_to_exact_terminal_excess(self):
        t,r=rollout(MovingTape(),self.m,{**self.p,'training_reward_objective':'matched_hold_excess'},self.model,1000)
        self.assertTrue(r['qualified_accounting'])
        self.assertAlmostEqual(float(t.rewards.sum()),r['terminal_excess_wealth_micros']/r['initial_reference_wealth_micros'],places=6)

    def test_unknown_objective_is_rejected(self):
        with self.assertRaisesRegex(ValueError,'UNKNOWN_TRAINING_REWARD_OBJECTIVE'):
            rollout(MovingTape(),self.m,{**self.p,'training_reward_objective':'unknown'},self.model,1000)


if __name__=='__main__':unittest.main()
