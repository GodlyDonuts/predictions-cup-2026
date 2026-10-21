import unittest
from unittest.mock import patch
import torch
from .stream import calibrate
from .test_actions import environment
from .policy import ApiPolicy
from moloch.learning_v2.train_inner import fingerprint
from .episode_tape import financial_digest


class ActorCalibrationTests(unittest.TestCase):
    def test_measured_region_includes_preparation_transfer_and_host_result(self):
        clock=[0.];calls=[]
        class Env:
            def observe(self):clock[0]+=.004;calls.append('observe');return {}
        class Model(torch.nn.Module):
            acting_validation='consolidated_v1'
            def __init__(self):super().__init__();self.p=torch.nn.Parameter(torch.zeros(1))
            def initial_state(self,n):return torch.zeros(n,1)
            def step(self,**kw):
                self.assert_stochastic=kw['deterministic'] is False
                clock[0]+=.005;calls.append('forward');torch.rand(1)
                return {'actions':{'family':torch.zeros(1,dtype=torch.long)}}
        def prepare(*args):clock[0]+=.003;calls.append('transfer');return {'contracts':torch.zeros(1,2,40)}
        def convert(actions):clock[0]+=.002;calls.append('convert');return [{'family':0}]
        model=Model();rng=torch.get_rng_state().clone()
        with patch('moloch.learning_actions.stream.tensors',side_effect=prepare),\
             patch('moloch.learning_actions.stream.time.perf_counter',side_effect=lambda:clock[0]),\
             patch('moloch.learning_actions.acting_validation.actions_to_python',side_effect=convert):
            result=calibrate(Env(),model,rounds=2)
        self.assertTrue(model.assert_stochastic);self.assertTrue(torch.equal(rng,torch.get_rng_state()))
        self.assertEqual(calls,['observe','transfer','forward','convert']*5)
        for value in result['measured_ms']:self.assertAlmostEqual(value,14.)
        self.assertEqual(result['observation_shapes'],[[1,2,40]]*2)

    def test_real_actor_preserves_weights_wallet_clock_and_next_random_choice(self):
        torch.set_num_threads(1);torch.manual_seed(9951);e=environment();model=ApiPolicy(16)
        initial=(fingerprint(model),financial_digest(e),e.now);rng=torch.get_rng_state().clone()
        result=calibrate(e,model,rounds=3)
        self.assertEqual(initial,(fingerprint(model),financial_digest(e),e.now))
        self.assertTrue(torch.equal(rng,torch.get_rng_state()));self.assertEqual(len(result['measured_ms']),3)
        self.assertGreaterEqual(result['decision_latency_ms'],1);self.assertTrue(result['policy_rng_preserved'])
        self.assertIn('action_host_conversion',result['timing_scope'])

    def test_invalid_rounds_reject_before_observing_environment(self):
        for value in(0,-1,True,1.5,None):
            with self.assertRaisesRegex(ValueError,'CALIBRATION_ROUNDS'):calibrate(None,None,value)


if __name__=='__main__':unittest.main()
