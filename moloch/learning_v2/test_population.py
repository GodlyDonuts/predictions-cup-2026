from dataclasses import replace
import unittest
import torch
from .population import probe,distance,mutate,BehaviorArchive
from .train_inner import PPOTrainer,PPOConfig,fingerprint
from . import test_train_inner as inner


class PopulationMechanics(unittest.TestCase):
    def setUp(self):
        f=inner.OnPolicyUpdates();f.setUp();self.t=f.t;self.p=f.p

    def test_identical_behavior_has_zero_distance(self):
        a=probe(self.p,self.t);self.assertEqual(distance(a,a)['market_family_kl'],0.)

    def test_search_data_cannot_tune_mutation_size(self):
        with self.assertRaisesRegex(ValueError,'TRAINING_ONLY'):probe(self.p,replace(self.t,split_role='search_development'))

    def test_mutation_resets_moments_and_requires_fresh_child_trajectories(self):
        trainer=PPOTrainer(self.p,PPOConfig(epochs=1),'a'*64);before=fingerprint(self.p)
        out=mutate(trainer,self.t,'local',871)
        self.assertNotEqual(before,fingerprint(self.p));self.assertEqual(out['parent_parameter_sha256'],before)
        self.assertTrue(out['optimizer_moments_reset']);self.assertEqual(len(trainer.optimizer.state),0)
        self.assertGreaterEqual(out['market_family_kl'],0.)
        with self.assertRaisesRegex(ValueError,'FRESH_TRAJECTORIES'):trainer.update(self.t)

    def episodes(self):
        return [{'split_role':'search_development','start_ms':n*1000,'terminal_ms':n*1000+900,
                 'parameter_sha256':'b'*64,'qualified_accounting':True,'reference_return':.01,'tradable_decision_frames':10}for n in range(2)]

    def test_archive_keeps_quality_and_behavior_cells_separate(self):
        a=BehaviorArchive();e=self.episodes()
        self.assertTrue(a.consider('a','b'*64,e,(0,1),'immutable-a.pt'))
        self.assertTrue(a.consider('b','b'*64,e,(1,1),'immutable-b.pt'))
        self.assertFalse(a.consider('c','b'*64,e,(0,1),'immutable-c.pt'));self.assertEqual(len(a.entries),2)

    def test_archive_does_not_reward_missing_or_losing_blocks(self):
        for invalid in [None,-.01]:
            e=self.episodes();e[1]['reference_return']=invalid;e[1]['qualified_accounting']=invalid is not None
            self.assertFalse(BehaviorArchive().consider('x','b'*64,e,(0,),'x.pt'))

    def test_empty_search_block_cannot_qualify_a_protected_alternative(self):
        e=self.episodes();e[1]['tradable_decision_frames']=0
        self.assertFalse(BehaviorArchive().consider('x','b'*64,e,(0,),'x.pt'))

    def test_selection_cannot_rank_an_empty_evaluation_block_as_zero_profit(self):
        from .run_population import score
        e=self.episodes()
        for row in e:row['observable_market_frames']=100
        self.assertEqual(score(e),.01);e[1]['observable_market_frames']=0
        self.assertEqual(score(e),-1e99)

    def test_archive_rejects_changed_policy_and_overlapping_search_blocks(self):
        e=self.episodes();e[1]['parameter_sha256']='c'*64
        with self.assertRaisesRegex(ValueError,'POLICY_CHANGED'):BehaviorArchive().consider('x','b'*64,e,(0,),'x')
        e=self.episodes();e[1]['start_ms']=100
        with self.assertRaisesRegex(ValueError,'OVERLAP'):BehaviorArchive().consider('x','b'*64,e,(0,),'x')

    def test_same_calendars_in_distinct_execution_worlds_are_not_duplicate_evidence(self):
        e=self.episodes();stress=[dict(row,execution_profile='adverse')for row in e]
        self.assertTrue(BehaviorArchive().consider('x','b'*64,e+stress,(0,),'x'))
        with self.assertRaisesRegex(ValueError,'REPEATED_CALENDARS'):
            BehaviorArchive().consider('x','b'*64,[e[0],stress[0]],(0,),'x')
        stress[0]['reference_return']=-.01
        self.assertFalse(BehaviorArchive().consider('x','b'*64,e+stress,(0,),'x'))

    def market_trajectory(self):
        import torch
        from .interval_market_policy import IntervalMarketPolicy
        model=IntervalMarketPolicy(3,hidden_size=32,market_hidden_size=8)
        x=torch.cat([self.t.contracts,torch.zeros(*self.t.contracts.shape[:-1],16)],-1)
        g=torch.cat([self.t.global_state,torch.zeros(*self.t.global_state.shape[:-1],4)],-1)
        with torch.no_grad():rows=model.sequence(x,g,self.t.present,self.t.masks,self.t.reset)
        actions={k:torch.stack([r['actions'][k]for r in rows])for k in rows[0]['actions']}
        return model,replace(self.t,contracts=x,global_state=g,actions=actions,
            old_logprob=torch.stack([r['logprob']for r in rows]),old_values=torch.stack([r['value']for r in rows]),behavior_sha256=fingerprint(model))

    def test_packed_market_memory_probe_and_mutation_use_the_real_actor_path(self):
        model,t=self.market_trajectory();before=fingerprint(model)
        a=probe(model,t);self.assertEqual(distance(a,a)['market_family_kl'],0.)
        trainer=PPOTrainer(model,PPOConfig(epochs=1),'a'*64);out=mutate(trainer,t,'local',47271)
        self.assertNotEqual(before,fingerprint(model));self.assertTrue(out['optimizer_moments_reset'])
        with self.assertRaisesRegex(ValueError,'FRESH_TRAJECTORIES'):trainer.update(t)

    def test_behavior_probe_does_not_invent_feasible_actions_for_unavailable_markets(self):
        for model,t in [(self.p,self.t),self.market_trajectory()]:
            masks={k:v.clone()for k,v in t.masks.items()};masks['market'][:,:,1]=False;masks['family'][:,:,1,:]=False
            t=replace(t,masks=masks);out=probe(model,t)
            self.assertEqual(distance(out,out)['market_family_kl'],0.)
            self.assertEqual(float(out['market'][...,1].abs().sum()),0.)

    def test_market_checkpoint_resumes_exact_packed_architecture_and_optimizer(self):
        import tempfile
        from pathlib import Path
        from .run_population import load_trainer
        model,t=self.market_trajectory();trainer=PPOTrainer(model,PPOConfig(epochs=1),'a'*64);trainer.update(t)
        with tempfile.TemporaryDirectory()as d:
            provenance={k:'b'*64 for k in ['dataset_manifest_sha256','mechanics_manifest_sha256','evaluation_protocol_sha256','action_adapter_sha256']}
            provenance.update(lineage='synthetic-software-test-only',classification='synthetic_only',architecture_shape={'market_slots':3,'global_hidden_size':32,'market_hidden_size':8,'actor_state_size':model.hidden_size})
            p=Path(d)/'model.pt';h=trainer.save(p,provenance)
            other=load_trainer(p,h,'a'*64,'cpu')
            self.assertEqual(fingerprint(other.model),fingerprint(model));self.assertEqual(other.model.hidden_size,59)
            self.assertEqual(other.training_decisions,trainer.training_decisions)
            self.assertEqual(len(other.optimizer.state),len(trainer.optimizer.state))

    def test_holding_relative_archive_cannot_promote_passive_reference_appreciation(self):
        from .run_population import score
        e=self.episodes()
        for r in e:r.update(observable_market_frames=10,matched_hold_excess_return=-.01)
        self.assertEqual(score(e,'matched_hold_excess'),-.01)
        self.assertFalse(BehaviorArchive(quality_field='matched_hold_excess_return').consider('x','b'*64,e,(0,),'x'))
        for r in e:r['matched_hold_excess_return']=.001
        archive=BehaviorArchive(quality_field='matched_hold_excess_return')
        self.assertTrue(archive.consider('x','b'*64,e,(0,),'x'))
        self.assertEqual(archive.json()['quality_field'],'matched_hold_excess_return')


if __name__=='__main__':unittest.main()
