"""Exact old-policy densities/values and PPO updates with less acting work."""
from copy import deepcopy
import unittest
import tempfile
from pathlib import Path
from unittest.mock import patch
import torch
from .test_order_pages import world
from .order_policy import OrderPagePolicy
from .stream import Stream,tensors
from .batched import BatchStream
from .economics import GrowthLedger
from .training import collect_chunk,update
from .episode_tape import financial_digest


class RolloutStatisticsTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(219)

    def compare(self,device):
        m=OrderPagePolicy(16).to(device);x=tensors(world().observe(),device)
        h=m.initial_state(1);reset=torch.ones(1,dtype=torch.bool,device=device)
        for mode in('strict','consolidated_v1'):
            m.acting_validation=mode
            for deterministic in(False,True):
                for seed in(12,23,34):
                    outputs=[];rng=[]
                    for method in(m.step,m.step_rollout):
                        torch.manual_seed(seed)
                        with torch.no_grad():outputs.append(method(**x,hidden=h,reset=reset,deterministic=deterministic))
                        rng.append((torch.cuda.get_rng_state()if device=='cuda'else torch.get_rng_state()).clone())
                    self.assertEqual(set(outputs[1]),{'actions','hidden','logprob','value'})
                    self.assertTrue(torch.equal(rng[0],rng[1]))
                    for k in('hidden','logprob','value'):self.assertTrue(torch.equal(outputs[0][k],outputs[1][k]),k)
                    for k,v in outputs[0]['actions'].items():self.assertTrue(torch.equal(v,outputs[1]['actions'][k]),k)

    def test_cpu_actions_density_value_hidden_and_rng_are_exact(self):self.compare('cpu')

    @unittest.skipUnless(torch.cuda.is_available(),'CUDA required')
    def test_cuda_actions_density_value_hidden_and_rng_are_exact(self):self.compare('cuda')

    def test_bootstrap_is_exact_critic_and_does_not_mutate_hidden_or_rng(self):
        m=OrderPagePolicy(16);x=tensors(world().observe(),'cpu');h=m.initial_state(1);before=h.clone()
        reset=torch.zeros(1,dtype=torch.bool);rng=torch.get_rng_state().clone()
        with torch.no_grad():
            expected=m.step(**x,hidden=h,reset=reset,deterministic=True)['value']
            actual=m.bootstrap_value(**x,hidden=h,reset=reset)
        self.assertTrue(torch.equal(expected,actual));self.assertTrue(torch.equal(h,before))
        self.assertTrue(torch.equal(rng,torch.get_rng_state()))

    def test_compact_chunk_and_accepted_ppo_parameter_update_are_exact(self):
        a=OrderPagePolicy(16);a.acting_validation='consolidated_v1';b=deepcopy(a);chunks=[];streams=[]
        for m,compact in((a,False),(b,True)):
            torch.manual_seed(91);e=world()
            s=Stream(e,m,record_training=True,rollout_only=compact,calibration={'decision_latency_ms':5},behavior_guard='tensor_versions_v1')
            chunks.append(collect_chunk(s,8,reward_ledger=GrowthLedger(e,development=True)));streams.append(s)
        for left,right in zip(chunks[0].frames,chunks[1].frames):
            for key in('old_logprob','old_value','initial_hidden'):self.assertTrue(torch.equal(left[key],right[key]),key)
            for key,value in left['actions'].items():self.assertTrue(torch.equal(value,right['actions'][key]))
            self.assertEqual(left['economic_reward'],right['economic_reward'])
        self.assertTrue(torch.equal(chunks[0].bootstrap_value,chunks[1].bootstrap_value))
        self.assertEqual(financial_digest(streams[0].env),financial_digest(streams[1].env))
        for m,c in zip((a,b),chunks):
            result=update(m,torch.optim.Adam(m.parameters(),lr=1e-5),c,[f['economic_reward']for f in c.frames],role='train',epochs=1)
            self.assertTrue(result[0]['accepted'])
        for key,v in a.state_dict().items():self.assertTrue(torch.equal(v,b.state_dict()[key]),key)

    def test_batch_compaction_preserves_profile_and_bootstrap(self):
        model=OrderPagePolicy(16);records=[]
        for compact in(False,True):
            torch.manual_seed(81);envs=[world(),world()];envs[0].end=envs[0].now+1
            s=BatchStream(envs,model,[GrowthLedger(e,development=True)for e in envs],
                calibration={'decision_latency_ms':5},compact_finished=True,rollout_only=compact)
            s.collect(4);c=s.collect(4)
            records.append((c.bootstrap_value.clone(),[f['old_logprob'].clone()for f in c.frames],
                [financial_digest(e)for e in envs],s.hidden.clone()))
        self.assertTrue(torch.equal(records[0][0],records[1][0]))
        for a,b in zip(records[0][1],records[1][1]):self.assertTrue(torch.equal(a,b))
        self.assertEqual(records[0][2],records[1][2]);self.assertTrue(torch.equal(records[0][3],records[1][3]))

    def test_entropy_auxiliary_heads_and_action_heads_on_bootstrap_are_not_executed(self):
        m=OrderPagePolicy(16);x=tensors(world().observe(),'cpu');h=m.initial_state(1);reset=torch.ones(1,dtype=torch.bool)
        with torch.no_grad(),patch('torch.distributions.Beta.entropy',side_effect=AssertionError('unused entropy')),\
             patch.object(m.monetary_quantiles,'forward',side_effect=AssertionError('unused auxiliary')):
            self.assertIn('logprob',m.step_rollout(**x,hidden=h,reset=reset))
        with torch.no_grad(),patch.object(m.heads['family'],'forward',side_effect=AssertionError('unused action head')):
            m.bootstrap_value(**x,hidden=h,reset=reset)

    def test_rollout_profile_cannot_disable_teacher_loss_or_allow_bad_forward(self):
        m=OrderPagePolicy(16);e=world();x=tensors(e.observe(),'cpu');h=m.initial_state(1);reset=torch.ones(1,dtype=torch.bool)
        with self.assertRaisesRegex(ValueError,'NO_GRAD_ONLY'):m.step_rollout(**x,hidden=h,reset=reset)
        with self.assertRaisesRegex(ValueError,'NO_GRAD_ONLY'):m.bootstrap_value(**x,hidden=h,reset=reset)
        with self.assertRaisesRegex(ValueError,'TRAINING_CAPTURE'):Stream(e,m,rollout_only=True)
        with torch.no_grad():m.fractions.bias.fill_(float('inf'))
        s=Stream(e,m,record_training=True,rollout_only=True,calibration={'decision_latency_ms':5});before=financial_digest(e)
        with self.assertRaises(ValueError):s.step()
        self.assertEqual(financial_digest(e),before)

    def test_sealed_compact_rollouts_replay_through_full_behavior_and_update(self):
        from .episode_tape import EpisodeWriter,EpisodeReader
        from .episode_training import optimize
        def make(*args):
            e=world();e.end=e.now+501;return e
        model=OrderPagePolicy(16);env=make()
        s=BatchStream([env],model,[GrowthLedger(env,development=True)],calibration={'decision_latency_ms':5},
            rollout_only=True,behavior_guard='tensor_versions_v1')
        with tempfile.TemporaryDirectory()as d:
            w=EpisodeWriter(Path(d)/'diary',s,dataset_sha256='a'*64,specification_sha256='b'*64)
            while env.now<env.end:w.record(s.collect(8))
            reader=EpisodeReader(w.path,w.seal())
            try:
                reports=optimize(reader,model,torch.optim.Adam(model.parameters(),lr=1e-5),make,
                    lambda e:GrowthLedger(e,development=True))
                self.assertTrue(any(r['accepted']for r in reports))
            finally:reader.close()

    def test_finite_benchmark_rejects_changed_finance_and_independent_test(self):
        from .rollout_statistics_benchmark import compare
        from .test_population_experiment import FixtureExperiment
        from .policy import ApiPolicy
        from .training import save_checkpoint
        def factory(config,output):
            output.mkdir();e=FixtureExperiment(output);e.frozen={'source_sha256':{}};e.close=lambda:None;return e
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);m=ApiPolicy(16);p=root/'model.pt'
            sha=save_checkpoint(p,m,torch.optim.Adam(m.parameters()),dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            c={'mode':'inspected_development','device':'cpu','development_episode_ms':200}
            cp={'path':str(p),'sha256':sha}
            with patch('moloch.learning_actions.rollout_statistics_benchmark.Experiment',factory):
                r=compare(c,cp,root/'proof',worlds=2,steps=4)
            self.assertEqual(len(r['measurements']),4);self.assertFalse(r['optimizerRun'])
            calls=[0]
            def changed(env):calls[0]+=1;return('a'if calls[0]<=2 else'b')*64
            with patch('moloch.learning_actions.rollout_statistics_benchmark.Experiment',factory),\
                 patch('moloch.learning_actions.rollout_statistics_benchmark.financial_digest',changed):
                with self.assertRaisesRegex(ValueError,'EXACT_ROLLOUT_STATISTICS'):
                    compare(c,cp,root/'failure',worlds=2,steps=4)
            self.assertTrue((root/'failure/FAILED_COMPARISON.json').is_file())
        with self.assertRaisesRegex(ValueError,'NO_INDEPENDENT_TEST'):
            compare({'mode':'inspected_development','test_finalist_once':True},{},'unused')
