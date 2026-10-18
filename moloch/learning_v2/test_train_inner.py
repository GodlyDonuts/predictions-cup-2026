import tempfile
import unittest
from pathlib import Path
from dataclasses import replace
import torch
from .policy import RecurrentPolicy
from .train_inner import PPOConfig, PPOTrainer, Trajectory, fingerprint, targets
from .test_policy import masks


class OnPolicyUpdates(unittest.TestCase):
    def setUp(self):
        torch.manual_seed(91);torch.set_num_threads(2)
        self.p = RecurrentPolicy();self.cfg = PPOConfig(epochs=2)
        self.trainer = PPOTrainer(self.p,self.cfg,'a'*64)
        T,B,N = 4,2,3
        x=torch.randn(T,B,N,16);g=torch.randn(T,B,12);present=torch.ones(T,B,N,dtype=torch.bool)
        reset=torch.zeros(T,B,dtype=torch.bool);reset[0]=True
        ms={k:torch.stack([v]*T)for k,v in masks(B,N).items()}
        with torch.no_grad(): rows=self.p.sequence(x,g,present,ms,reset)
        actions={k:torch.stack([r['actions'][k]for r in rows])for k in rows[0]['actions']}
        done=torch.zeros(T,B,dtype=torch.bool);done[-1]=True
        self.t=Trajectory(x,g,present,ms,reset,actions,torch.stack([r['logprob']for r in rows]),
                          torch.stack([r['value']for r in rows]),torch.randn(T,B)*0.001,
                          torch.full((T,B),1000.),done,torch.ones(T,B,dtype=torch.bool),
                          torch.full((T,B),float('nan')),fingerprint(self.p),'a'*64,'fit_training')

    def test_genuine_gradient_update_and_missing_success_labels_remain_missing(self):
        old=fingerprint(self.p);results=self.trainer.update(self.t)
        self.assertNotEqual(old,fingerprint(self.p));self.assertEqual(len(results),2)
        self.assertTrue(all(r['successLabels']==0 for r in results))
        self.assertEqual(self.trainer.training_decisions,8)

    def test_accepted_updates_respect_declared_sampled_trust_region(self):
        out=self.trainer.update(self.t)
        accepted=[r for r in out if'updateSkipped'not in r]
        self.assertTrue(accepted)
        self.assertTrue(all(0<=r['postStepNonnegativeKl']<=self.cfg.maximum_sampled_kl for r in accepted))

    def test_rejected_candidate_restores_weights_and_optimizer(self):
        self.trainer.config=replace(self.cfg,maximum_sampled_kl=1e-15,maximum_step_backtracks=0)
        old=fingerprint(self.p);out=self.trainer.update(self.t)
        self.assertEqual(out[0]['updateSkipped'],'TRUST_REGION_BACKTRACK_FAILED')
        self.assertEqual(fingerprint(self.p),old);self.assertEqual(len(self.trainer.optimizer.state),0)

    def test_parent_trajectories_cannot_train_mutated_child(self):
        with torch.no_grad(): next(self.p.parameters()).add_(0.01)
        with self.assertRaisesRegex(ValueError,'FRESH_TRAJECTORIES'):
            self.trainer.update(self.t)

    def test_trajectory_cannot_be_reused_after_update(self):
        self.trainer.update(self.t)
        with self.assertRaisesRegex(ValueError,'FRESH_TRAJECTORIES'):
            self.trainer.update(self.t)

    def test_search_and_locked_data_never_used_as_gradient_fit(self):
        for role in ['search_development','locked_chronological_test','new_forward_cohort']:
            with self.assertRaisesRegex(ValueError,'ROLE_OR_SPLIT'):
                self.trainer.update(replace(self.t,split_role=role))

    def test_hidden_state_cannot_start_from_unknown_parent_fragment(self):
        reset=self.t.reset.clone();reset[0]=False
        with self.assertRaisesRegex(ValueError,'RECURRENT_BOUNDARIES'):
            self.trainer.update(replace(self.t,reset=reset))

    def test_clock_aware_discount_is_not_replay_row_discount(self):
        rewards=torch.ones_like(self.t.rewards);values=torch.zeros_like(rewards)
        a=replace(self.t,rewards=rewards,old_values=values)
        fast=targets(a,self.cfg)[0]
        slow=targets(replace(a,elapsed_ms=self.t.elapsed_ms*10),self.cfg)[0]
        self.assertTrue(bool((slow[:-1]<fast[:-1]).all()))
        self.assertTrue(torch.equal(slow[-1],fast[-1]))

    def test_checkpoint_requires_complete_source_bundle(self):
        with tempfile.TemporaryDirectory()as d:
            with self.assertRaisesRegex(ValueError,'PROVENANCE'):
                self.trainer.save(Path(d)/'model.pt',{})

    def test_checkpoint_keeps_optimizer_architecture_sources_and_no_live_authority(self):
        provenance={k:'b'*64 for k in ['dataset_manifest_sha256','mechanics_manifest_sha256',
                                     'evaluation_protocol_sha256','action_adapter_sha256']}
        provenance.update(lineage='synthetic-software-test-only',classification='synthetic_only')
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'model.pt';h=self.trainer.save(p,provenance)
            b=torch.load(p,weights_only=True)
            self.assertEqual(len(h),64);self.assertFalse(b['production_authority'])
            self.assertEqual(b['parameter_sha256'],fingerprint(self.p))
            self.assertEqual(b['split_manifest_sha256'],'a'*64)
            self.assertIn('optimizer',b)
            with self.assertRaisesRegex(ValueError,'IMMUTABLE'):
                self.trainer.save(p,provenance)


if __name__=='__main__':unittest.main()
