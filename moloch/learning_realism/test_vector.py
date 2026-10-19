"""Differential checks use explicit synthetic fixtures, never held-out history."""
import unittest
import torch
from moloch.learning_flow.episodes import FlowTape
from moloch.learning_flow.policy import FlowPolicy
from moloch.learning_v2.train_inner import PPOTrainer,PPOConfig
from .test_replay import book,reference,fixture
from .protocol import Split,DAY
from .replay import ReplayTape
from .fast_replay import PreparedTape
from .vector_rollout import collect


def tapes():
    rows=tuple([book(0),reference(0),reference(0,'Polymarket'),book(1000),
                book(5000),book(10000),book(15000),book(25000),book(200000)])
    original=FlowTape.__new__(FlowTape)  # declared synthetic fixture
    original.rows=rows;original.times=tuple(r['available_at_ms']for r in rows)
    original.exchanges=('__GLOBAL_WAIT__','x');original.index={e:i for i,e in enumerate(original.exchanges)}
    original.flow_rows=();original.flow_times=();original.manifest={};original.manifest_sha256='e'*64
    split=Split(0,10*DAY)
    slow=ReplayTape.__new__(ReplayTape)
    slow.__dict__.update(original.__dict__)
    from .protocol import Delivery
    slow.split=split;slow.role='train';slow.delivery=Delivery();slow.finalist=None
    fast=PreparedTape.from_loaded(original,split=split,role='train')
    return slow,fast


class VectorTests(unittest.TestCase):
    def test_predecoded_books_preserve_observations_and_wallet_independence(self):
        import numpy as np
        slow,fast=tapes();mechanics=fixture().mechanics
        args=dict(start_ms=5000,end_ms=30000,initial_cash_micros=1000_000000,
                  target_wealth_micros=11000_000000,burnin_ms=5000)
        a=slow.episode(mechanics,**args);b=fast.episode(mechanics,**args)
        c=fast.episode(mechanics,**args)
        for at in range(5000,12000,1000):
            a.advance(at);b.advance(at)
            x,y=a.observe(),b.observe()
            for key in ('contracts','global_state','present'):np.testing.assert_array_equal(x[key],y[key])
            for key in x['masks']:np.testing.assert_array_equal(x['masks'][key],y['masks'][key])
        self.assertEqual(a.account(),b.account())
        self.assertIs(b.books['x'].book,fast.prepared_books[id(fast.rows[5])])
        b.cash-=1;self.assertEqual(c.cash,1000_000000)
        b.books['x'].depleted[('BUY',450000)]=12
        self.assertEqual(c.books['x'].depleted,{})

    def test_batch_scalar_complete_trajectory_and_gpu_ready_ppo_contract(self):
        torch.set_num_threads(1);torch.manual_seed(19)
        _,tape=tapes();model=FlowPolicy(2,hidden_size=32,market_hidden_size=8)
        args=dict(steps=14,burnin_steps=2,drain_ms=1000,deterministic=True,
                  capture_trajectory=True,split_manifest_sha256='a'*64)
        a,ar=collect(tape,fixture().mechanics,model,[4000,5000],inference_mode='scalar',**args)
        b,br=collect(tape,fixture().mechanics,model,[4000,5000],inference_mode='batched',**args)
        self.assertEqual(ar['worlds_summary'],br['worlds_summary'])
        for key in ('contracts','global_state','present','reset','rewards','elapsed_ms','done','learn'):
            torch.testing.assert_close(getattr(a,key),getattr(b,key))
        torch.testing.assert_close(a.old_logprob,b.old_logprob,atol=1e-5,rtol=1e-5)
        self.assertEqual(a.contracts.shape,(14,2,2,40))
        self.assertTrue(a.reset[0].all());self.assertTrue(a.done[-1].all())
        self.assertEqual(a.elapsed_ms[-1].tolist(),[2000.,2000.])
        trainer=PPOTrainer(model,PPOConfig(epochs=1),'a'*64)
        trainer.update(b)
        self.assertGreater(trainer.accepted_optimizer_updates,0)

    def test_complete_rewards_and_receipt_drain_match_original_rollout(self):
        from moloch.learning_flow.rollout import rollout
        import types
        torch.set_num_threads(1);torch.manual_seed(21)
        _,tape=tapes();model=FlowPolicy(2,hidden_size=32,market_hidden_size=8)
        original_step=model.step
        def deterministic(self,*args,**kwargs):
            kwargs['deterministic']=True
            return original_step(*args,**kwargs)
        model.step=types.MethodType(deterministic,model)
        protocol={'initial_cash_micros':100000000000,'planning_target_wealth_micros':1100000000000,
            'episode_length_ms':12000,'maximum_receipt_drain_ms':1000,'recurrent_burnin_ms':2000,
            'decision_interval_ms':1000,'run_start_ms':0,'fit_end_ms':100000,
            'training_reward_objective':'interval_hold_lower_excess','_sha256':'a'*64}
        a,_=rollout(tape,fixture().mechanics,protocol,model,4000)
        b,_=collect(tape,fixture().mechanics,model,[4000],steps=14,burnin_steps=2,drain_ms=1000,
                    deterministic=True,split_manifest_sha256='a'*64)
        for key in ('contracts','global_state','present','reset','rewards','elapsed_ms','done','learn','old_logprob','old_values'):
            torch.testing.assert_close(getattr(a,key),getattr(b,key),atol=1e-5,rtol=1e-5)
        for key in a.actions:torch.testing.assert_close(a.actions[key],b.actions[key],atol=1e-5,rtol=1e-5)


if __name__=='__main__':unittest.main()
