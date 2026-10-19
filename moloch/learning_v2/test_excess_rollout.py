import unittest
from dataclasses import replace
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

    def test_realized_only_reward_cannot_credit_passive_marks(self):
        t,r=rollout(MovingTape(),self.m,{**self.p,'training_reward_objective':'realized_pnl'},self.model,1000,baseline=True)
        self.assertGreater(r['reference_return'],0)
        self.assertEqual(r['realized_micros'],0);self.assertEqual(float(t.rewards.sum()),0)

    def test_realized_only_reward_including_drain_equals_net_fifo_result(self):
        class ExitFixture(RecurrentPolicy):
            def step(self,*args,**kwargs):
                row=super().step(*args,**kwargs)
                if bool(kwargs['masks']['family'][0,1,4]):
                    for key,value in {'market':1,'family':4,'side':0,'price':2,'patience':2,'target_fraction':0.,'child_fraction':1.}.items():
                        row['actions'][key]=torch.full_like(row['actions'][key],value)
                else:
                    row['actions']['family'].fill_(0);row['actions']['market'].fill_(0)
                return row
        m=replace(self.m,limits=replace(self.m.limits,fee_per_share=1000))
        protocol={**self.p,'episode_length_ms':20000,'maximum_receipt_drain_ms':20000,'training_reward_objective':'realized_pnl'}
        t,r=rollout(MovingTape(),m,protocol,ExitFixture(24,14,32),1000)
        self.assertTrue(r['qualified_accounting']);self.assertGreater(r['fill_receipts'],0)
        self.assertGreater(r['fees_micros'],0);self.assertLess(r['realized_micros'],0)
        self.assertAlmostEqual(float(t.rewards.sum()),r['realized_micros']/r['initial_reference_wealth_micros'],places=6)
        self.assertEqual(r['modeled_realized_return'],r['realized_micros']/r['initial_reference_wealth_micros'])


if __name__=='__main__':unittest.main()
