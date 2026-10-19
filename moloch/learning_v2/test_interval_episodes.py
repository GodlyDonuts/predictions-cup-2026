import unittest
from .account import UNIT,Limits,Target,project,wealth
from .clock import BudgetConfig
from .reference_env import Mechanics
from .episodes import Episode
from .interval_episodes import IntervalEpisode
from .liquidity import Book
from .interval_excess_reward import IntervalHoldReward


class PartialValuationEpisodes(unittest.TestCase):
    def fixture(self,kind=IntervalEpisode):
        m=Mechanics(BudgetConfig(100,600,24,2500,'a'*64),Limits(100,2000*UNIT,100*UNIT),
                    100,200,100,10000,20000,1,'Kalshi')
        e=kind(m,0,1000*UNIT,('__GLOBAL_WAIT__','x','unmarked'),11000*UNIT,0,100000)
        e.lots[('unmarked','YES')]=[[10,400000]]
        e.add_book(Book('x',0,None,((400000,100),),((450000,100),),'b'*64))
        e.add_book(Book('unmarked',0,None,((300000,100),),((350000,100),),'d'*64))
        e.advance(0);e.scope['m']={'x'};e.mapping['x']='m'
        self.reference(e,'Kalshi',480000,520000)
        return e

    def reference(self,e,venue,bid,ask,at=0):
        r={'venue':venue,'market_id':'m','source':venue,'mapping_kind':'direct_party_reference',
           'mapping_semantics':'same_party_conditional_not_contract_equivalence',
           'available_at_ms':at,'received_at_ms':at,'source_at_ms':at,
           'bid_yes_micros':bid,'ask_yes_micros':ask,'price_yes_micros':(bid+ask)//2,'original_sha256':'c'*64}
        e.reference_cache[('m',venue)]=r;e._refresh_reference('m');e.advance(e.now)

    def test_unknown_wealth_does_not_disable_funded_buy_or_covered_exit(self):
        e=self.fixture();o=e.observe()
        self.assertIsNone(o['wealth']);self.assertIsNone(e.marked_wealth())
        self.assertEqual(o['contracts'].shape,(3,32));self.assertEqual(o['global_state'].shape,(16,))
        self.assertTrue(o['masks']['family'][1,1]);self.assertTrue(o['masks']['family'][2,4])
        self.assertFalse(o['masks']['family'][2,1]);self.assertFalse(o['masks']['family'][2,2])
        self.assertEqual(o['sizing_capital_micros'],1000*UNIT);self.assertEqual(o['global_state'][15],1.)
        p=e.submit(Target('unmarked','YES',0,UNIT,300000,'SELL'),60000)
        self.assertEqual(p.admitted_quantity,10);self.assertEqual(e.cash,1000*UNIT)
        self.assertEqual(e.account().pending_sells[('unmarked','YES')],10)

    def test_original_default_still_masks_missing_portfolio_valuation(self):
        e=self.fixture(Episode);o=e.observe()
        self.assertIsNone(o['wealth']);self.assertFalse(o['masks']['family'][:,[1,2,4]].any())
        self.assertEqual(e.submit(Target('unmarked','YES',0,UNIT,300000,'SELL'),60000).admitted_quantity,0)

    def test_anchor_never_creates_cash_and_reservations_remain_binding(self):
        e=self.fixture();e.cash=2*UNIT
        t=Target('x','YES',UNIT,UNIT,450000,'BUY')
        p=e.submit(t,60000);self.assertEqual(p.admitted_quantity,4)
        self.assertEqual(e.cash,2*UNIT);self.assertEqual(e.account().spendable,200000)
        self.assertEqual(project(e.account(),e.marks(),t,e.mechanics.limits,sizing_capital_micros=1000*UNIT).admitted_quantity,0)

    def test_observed_intervals_keep_disagreement_and_expire(self):
        e=self.fixture();self.reference(e,'Polymarket',600000,620000)
        self.assertEqual(e.yes_intervals(),{'x':(480000,620000)})
        a=e.observe()['contracts'][1];self.assertAlmostEqual(float(a[26]),.04,places=6)
        self.assertEqual(a[27],1.);self.assertAlmostEqual(float(a[28]),.6,places=6)
        r=IntervalHoldReward(e.account(),1000*UNIT)
        self.assertEqual(r.observe(e.account(),e.yes_intervals()).lower_excess_micros,0)
        self.assertIsNone(wealth(e.account(),e.marks()))
        e.advance(20001);self.assertEqual(e.yes_intervals(),{})
        self.assertFalse(e.observe()['contracts'][:,24:].any())

    def test_midpoint_only_and_unverified_mapping_are_not_observed_intervals(self):
        e=self.fixture();r=e.reference_cache[('m','Kalshi')]
        del r['bid_yes_micros'];self.assertIsNone(e.venue_interval('x','Kalshi'))
        self.reference(e,'Kalshi',480000,520000)
        e.reference_cache[('m','Kalshi')]['mapping_semantics']='race_context_only'
        self.assertIsNone(e.venue_interval('x','Kalshi'));self.assertNotIn(('x','YES'),e.marks())

    def test_patience_must_allow_the_actual_preflight_not_just_exchange_arrival(self):
        from dataclasses import replace
        e=self.fixture();e.mechanics=replace(e.mechanics,read_latency_ms=600,preflight_reads=16)
        a=e.observe();self.assertEqual(a['minimum_modeled_admission_delay_ms'],18600)
        self.assertEqual(a['masks']['patience'][1,1,0].tolist(),[False,False,True])
        self.assertTrue(a['masks']['side'][1,1,0])
        e.mechanics=replace(e.mechanics,read_latency_ms=4000)
        b=e.observe();self.assertFalse(b['masks']['family'][:,[1,2,4]].any())
        self.assertTrue(b['masks']['market'][0])

    def test_observing_during_an_actual_preflight_read_preserves_busy_state(self):
        e=self.fixture();p=e.submit(Target('x','YES',UNIT,UNIT,450000,'BUY'),60000)
        self.assertGreater(p.admitted_quantity,0);e.advance(0)
        self.assertIsNotNone(e.budget.active)
        o=e.observe();self.assertFalse(o['masks']['family'][:,[1,2,4]].any())
        self.assertTrue(o['masks']['market'][0]);self.assertIsNotNone(e.budget.active)


    def test_interval_rollout_credits_only_relative_result_when_absolute_roi_unknown(self):
        import torch
        from .policy import RecurrentPolicy
        from .run_experiment import rollout
        outer=self
        class FixtureTape:
            def episode(self,mechanics,start,end,**kwargs):
                e=outer.fixture();e.start=start;e.end=end
                e.lots[('x','YES')]=[[20,600000]];e.cash-=16*UNIT
                return e
        class ExitPolicy(RecurrentPolicy):
            def step(self,*args,**kwargs):
                row=super().step(*args,**kwargs)
                if bool(kwargs['masks']['family'][0,1,4]):
                    for key,value in {'market':1,'family':4,'side':0,'price':2,'patience':2,'target_fraction':0.,'child_fraction':1.}.items():
                        row['actions'][key]=torch.full_like(row['actions'][key],value)
                else:row['actions']['family'].fill_(0);row['actions']['market'].fill_(0)
                return row
        e=self.fixture();protocol={'run_start_ms':0,'fit_end_ms':100000,'episode_length_ms':5000,
            'maximum_receipt_drain_ms':5000,'initial_cash_micros':1000*UNIT,'planning_target_wealth_micros':11000*UNIT,
            'recurrent_burnin_ms':1000,'decision_interval_ms':1000,'_sha256':'d'*64,
            'training_reward_objective':'interval_hold_lower_excess'}
        trajectory,result=rollout(FixtureTape(),e.mechanics,protocol,ExitPolicy(32,16,32),1000)
        self.assertGreater(result['fill_receipts'],0);self.assertLess(result['realized_micros'],0)
        self.assertTrue(result['qualified_relative_interval_accounting'])
        self.assertFalse(result['qualified_accounting']);self.assertIsNone(result['reference_return'])
        self.assertIsNone(result['final_reference_wealth_micros'])
        self.assertGreater(int(trajectory.learn.sum()),0)
        self.assertAlmostEqual(float(trajectory.rewards.sum()),result['interval_lower_excess_micros']/(1000*UNIT),places=6)


if __name__=='__main__':unittest.main()
