"""Sampling/finance equivalence for consolidated inference checks."""
from copy import deepcopy
import tempfile,unittest
from pathlib import Path
from unittest.mock import patch
import torch
from .acting_validation import CONSOLIDATED,STRICT,categorical,beta,validate_samples,actions_to_python
from .order_policy import OrderPagePolicy
from .joint_memory_policy import JointMemoryPolicy
from .component_execution import ComponentPolicy
from .test_order_pages import world
from .stream import Stream,tensors
from .training import collect_chunk,update,save_checkpoint,load_checkpoint
from .economics import GrowthLedger
from .episode_tape import financial_digest
from .behavior_guard import identity,contract


class ActingValidationTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(902)

    def compare_sampling(self,device):
        x=tensors(world().observe(),device)
        for kind in(OrderPagePolicy,JointMemoryPolicy,ComponentPolicy):
            a=kind(16).to(device);b=deepcopy(a);b.acting_validation=CONSOLIDATED
            h=a.initial_state(1);reset=torch.ones(1,dtype=torch.bool,device=device)
            for deterministic in(False,True):
                for seed in range(6):
                    outputs=[];states=[]
                    for m in(a,b):
                        torch.manual_seed(seed)
                        with torch.no_grad():outputs.append(m.step(**x,hidden=h,reset=reset,deterministic=deterministic))
                        states.append((torch.get_rng_state(),torch.cuda.get_rng_state()if device=='cuda'else None))
                    for key in outputs[0]:
                        left,right=outputs[0][key],outputs[1][key]
                        if isinstance(left,dict):
                            for name in left:self.assertTrue(torch.equal(left[name],right[name]),(key,name,seed,device))
                        else:self.assertTrue(torch.equal(left,right),(key,seed,device))
                    self.assertTrue(torch.equal(states[0][0],states[1][0]))
                    if device=='cuda':self.assertTrue(torch.equal(states[0][1],states[1][1]))
                    expected={k:v.item()for k,v in outputs[0]['actions'].items()}
                    self.assertEqual(actions_to_python(outputs[1]['actions'])[0],expected)

    def test_cpu_actions_density_hidden_and_rng_match_bitwise(self):self.compare_sampling('cpu')

    @unittest.skipUnless(torch.cuda.is_available(),'CUDA hardware required')
    def test_cuda_actions_density_hidden_and_rng_match_bitwise(self):self.compare_sampling('cuda')

    def test_invalid_masks_parameters_and_samples_still_fail(self):
        for fast in(False,True):
            with self.assertRaisesRegex(ValueError,'NO_FEASIBLE'):
                categorical(torch.zeros(1,3),torch.zeros(1,3,dtype=torch.bool),fast)
            with self.assertRaises(ValueError):categorical(torch.full((1,3),float('nan')),torch.ones(1,3,dtype=torch.bool),fast)
            with self.assertRaises(ValueError):beta(torch.tensor([-1.]),torch.tensor([2.]),fast)
        with self.assertRaisesRegex(ValueError,'BETA_SAMPLES'):
            validate_samples({'target_fraction':torch.tensor([float('nan')])})
        d=categorical(torch.tensor([[-float('inf'),0.]]),torch.ones(1,2,dtype=torch.bool),True)
        self.assertEqual(d.probs.tolist(),[[0.,1.]])

    def test_bad_forward_cannot_advance_clock_or_dispatch_an_order(self):
        e=world();m=OrderPagePolicy(16);m.acting_validation=CONSOLIDATED
        with torch.no_grad():m.heads['family'].bias.fill_(float('nan'))
        s=Stream(e,m,calibration={'decision_latency_ms':2});before=financial_digest(e)
        with self.assertRaises(ValueError):s.step()
        self.assertEqual(financial_digest(e),before);self.assertFalse(e.api_orders);self.assertFalse(e.budget.writes)

    def test_teacher_forcing_keeps_original_checks_and_gradient_density(self):
        e=world();a=OrderPagePolicy(16);b=deepcopy(a);b.acting_validation=CONSOLIDATED
        x=tensors(e.observe(),'cpu');h=a.initial_state(1);reset=torch.ones(1,dtype=torch.bool)
        with torch.no_grad():actions=a.step(**x,hidden=h,reset=reset)['actions']
        outputs=[]
        for m in(a,b):
            out=m.step(**x,hidden=h,reset=reset,actions=actions);out['logprob'].sum().backward();outputs.append(out)
        self.assertTrue(torch.equal(outputs[0]['logprob'],outputs[1]['logprob']))
        for p,q in zip(a.parameters(),b.parameters()):
            if p.grad is not None:self.assertTrue(torch.equal(p.grad,q.grad))
        actions['target_fraction'].fill_(2.)
        with self.assertRaises(ValueError):b.step(**x,hidden=h,reset=reset,actions=actions)

    def test_complete_streams_have_exact_finance_reads_clocks_and_actions(self):
        a=OrderPagePolicy(16);b=deepcopy(a);b.acting_validation=CONSOLIDATED
        environments=[];traces=[]
        for m in(a,b):
            torch.manual_seed(77);e=world();e.end=2001;s=Stream(e,m,calibration={'decision_latency_ms':10})
            trace=[]
            while e.now<e.end:trace.append(s.step())
            environments.append(e);traces.append(trace)
        self.assertEqual(traces[0],traces[1]);self.assertEqual(financial_digest(environments[0]),financial_digest(environments[1]))
        self.assertEqual(vars(environments[0].budget),vars(environments[1].budget))

    def test_guard_checkpoint_and_real_ppo_retain_mode(self):
        m=OrderPagePolicy(16);before=identity(m,'tensor_versions_v1');m.acting_validation=CONSOLIDATED
        self.assertNotEqual(before,identity(m,'tensor_versions_v1'));self.assertEqual(contract(m)[-1],CONSOLIDATED)
        e=world();s=Stream(e,m,calibration={'decision_latency_ms':2},record_training=True,behavior_guard='tensor_versions_v1')
        c=collect_chunk(s,8,reward_ledger=GrowthLedger(e,development=True));opt=torch.optim.Adam(m.parameters(),lr=1e-5)
        self.assertTrue(update(m,opt,c,[f['economic_reward']for f in c.frames],role='train',epochs=1)[0]['accepted'])
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'cp.pt';sha=save_checkpoint(p,m,opt,dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            restored,_=load_checkpoint(p,sha);self.assertEqual(restored.acting_validation,CONSOLIDATED)

    def test_finite_benchmark_requires_exact_finance_and_records_priming(self):
        from .acting_validation_benchmark import compare
        from .test_population_experiment import FixtureExperiment
        from .policy import ApiPolicy
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);m=ApiPolicy(16);p=root/'model.pt'
            sha=save_checkpoint(p,m,torch.optim.Adam(m.parameters()),dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            def factory(config,output):
                output.mkdir();e=FixtureExperiment(output);e.evolution_bounds=(501,10001)
                e.frozen={'source_sha256':{}};e.close=lambda:None;return e
            with patch('moloch.learning_actions.acting_validation_benchmark.Experiment',factory):
                r=compare({'mode':'inspected_development','device':'cpu'},{'path':str(p),'sha256':sha},root/'proof',steps=4)
            self.assertTrue(r['exactActionsRngFinanceKnowledgeHiddenClocksAndBudgets'])
            self.assertEqual(len(r['measurements']),4);self.assertFalse(r['roiEstablished'])
            self.assertTrue(all(all(n>0 for n in row['primingReads'])for row in r['measurements']))
            calls=[0]
            def changed(env):calls[0]+=1;return('a'if calls[0]<=4 else'b')*64
            with patch('moloch.learning_actions.acting_validation_benchmark.Experiment',factory),\
                 patch('moloch.learning_actions.acting_validation_benchmark.financial_digest',changed):
                with self.assertRaisesRegex(ValueError,'EXACT_SERIAL_VALIDATION'):
                    compare({'mode':'inspected_development','device':'cpu'},{'path':str(p),'sha256':sha},root/'failed',steps=2)
            self.assertTrue((root/'failed/FAILED_COMPARISON.json').is_file())

    def test_benchmark_never_opens_independent_test(self):
        from .acting_validation_benchmark import compare
        with self.assertRaisesRegex(ValueError,'NO_INDEPENDENT_TEST'):
            compare({'mode':'inspected_development','test_finalist_once':True},{},'unused')
