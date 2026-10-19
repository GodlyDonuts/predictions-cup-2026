"""Exact evaluation sampling without unused learner/critic statistics."""
from copy import deepcopy
import unittest
from unittest.mock import patch
import torch
from .test_order_pages import world
from .order_policy import OrderPagePolicy
from .component_execution import ComponentPolicy
from .bulk_quotes import BulkQuotePolicy
from .stream import Stream,tensors
from .episode_tape import financial_digest


class ActionsOnlyTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(17)

    def compare(self,device):
        x=tensors(world().observe(),device)
        for cls in(OrderPagePolicy,ComponentPolicy,BulkQuotePolicy):
            m=cls(16).to(device);h=m.initial_state(1);reset=torch.ones(1,dtype=torch.bool,device=device)
            for validation in('strict','consolidated_v1'):
                m.acting_validation=validation
                for deterministic in(False,True):
                    for seed in(11,22,33):
                        outputs=[];rng=[]
                        for method in(m.step,m.step_actions_only):
                            torch.manual_seed(seed)
                            with torch.no_grad():outputs.append(method(**x,hidden=h,reset=reset,deterministic=deterministic))
                            rng.append((torch.cuda.get_rng_state()if device=='cuda'else torch.get_rng_state()).clone())
                        self.assertEqual(set(outputs[1]),{'actions','hidden'})
                        self.assertTrue(torch.equal(outputs[0]['hidden'],outputs[1]['hidden']))
                        self.assertTrue(torch.equal(rng[0],rng[1]))
                        for k,v in outputs[0]['actions'].items():self.assertTrue(torch.equal(v,outputs[1]['actions'][k]),k)

    def test_cpu_sampling_and_hidden_are_exact(self):self.compare('cpu')

    @unittest.skipUnless(torch.cuda.is_available(),'CUDA required')
    def test_cuda_sampling_and_hidden_are_exact(self):self.compare('cuda')

    def test_full_stream_finance_knowledge_clocks_and_rng_are_exact(self):
        m=OrderPagePolicy(16);m.acting_validation='consolidated_v1';traces=[];states=[]
        for fast in(False,True):
            torch.manual_seed(41);e=world();e.end=3001;s=Stream(e,m,calibration={'decision_latency_ms':5},actions_only=fast)
            rows=[]
            while e.now<e.end:rows.append(s.step())
            traces.append(rows);states.append((financial_digest(e),repr(vars(e.budget)),torch.get_rng_state()))
        self.assertEqual(traces[0],traces[1]);self.assertEqual(states[0][:2],states[1][:2])
        self.assertTrue(torch.equal(states[0][2],states[1][2]))

    def test_no_training_or_teacher_path_can_skip_required_statistics(self):
        e=world();m=OrderPagePolicy(16)
        with self.assertRaisesRegex(ValueError,'EVALUATION_ONLY'):Stream(e,m,record_training=True,actions_only=True)
        x=tensors(e.observe(),'cpu');h=m.initial_state(1);reset=torch.ones(1,dtype=torch.bool)
        with self.assertRaisesRegex(ValueError,'EVALUATION_ONLY'):m.step_actions_only(**x,hidden=h,reset=reset)
        with torch.no_grad():
            encoded,state=m.encode(x['contracts'],x['global_state'],x['present'],h,reset)
            with self.assertRaisesRegex(ValueError,'EVALUATION_ONLY'):
                m._act_from_encoding(encoded,state,x['present'],x['masks'],actions={},statistics=False)

    def test_unused_density_entropy_and_value_heads_are_not_executed(self):
        e=world();m=OrderPagePolicy(16);x=tensors(e.observe(),'cpu')
        with torch.no_grad(),patch('torch.distributions.Beta.entropy',side_effect=AssertionError('unused entropy')),\
             patch.object(m.value,'forward',side_effect=AssertionError('unused value')):
            out=m.step_actions_only(**x,hidden=m.initial_state(1),reset=torch.ones(1,dtype=torch.bool))
        self.assertEqual(set(out),{'actions','hidden'})

    def test_invalid_forward_cannot_reach_financial_dispatch(self):
        for mode in('strict','consolidated_v1'):
            for invalid in(float('nan'),float('inf')):
                e=world();m=OrderPagePolicy(16);m.acting_validation=mode
                with torch.no_grad():m.query_times.bias.fill_(invalid)
                s=Stream(e,m,calibration={'decision_latency_ms':5},actions_only=True);before=financial_digest(e)
                with self.assertRaises(ValueError):s.step()
                self.assertEqual(financial_digest(e),before);self.assertFalse(e.budget.writes)
