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


if __name__=='__main__':unittest.main()
