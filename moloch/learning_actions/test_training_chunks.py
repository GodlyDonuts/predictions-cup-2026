"""Batch organization invariants; no profitability or speed claims."""
import tempfile
import unittest
from pathlib import Path
import torch
from .training_chunks import steps, validate


class TrainingChunkTests(unittest.TestCase):
    def test_target_retains_samples_when_worlds_finish(self):
        config = {'compute_chunk_decisions': 8, 'training_chunk_transitions': 64}
        self.assertEqual([steps(config, n) for n in (8, 4, 2, 1)], [8, 16, 32, 64])
        self.assertEqual(steps(config, 3), 22)

    def test_legacy_and_configured_minimum_stay_explicit(self):
        self.assertEqual(steps({}, 1), 32)
        self.assertEqual(steps({'compute_chunk_decisions': 8}, 1), 8)
        self.assertEqual(steps({'compute_chunk_decisions': 8, 'training_chunk_transitions': 1}, 8), 8)

    def test_invalid_configuration_rejected_before_any_dataset_or_output(self):
        from .experiment import Experiment
        with tempfile.TemporaryDirectory() as td:
            for value in (0, -1, True, 1.5, '64'):
                with self.subTest(value=value), self.assertRaisesRegex(ValueError, 'CHUNK_TRANSITIONS'):
                    Experiment({'training_chunk_transitions': value}, Path(td) / 'unused')
                self.assertFalse((Path(td) / 'unused').exists())
        with self.assertRaisesRegex(ValueError, 'ONLINE_PPO'):
            validate({'training_chunk_transitions': 64, 'closed_episode_return_replay': True})
        for count in (0, -1, True, 1.5):
            with self.assertRaisesRegex(ValueError, 'WORLD_COUNT'):
                steps({}, count)

    def test_grouping_keeps_every_stochastic_decision_and_original_world(self):
        from .test_actions import environment
        from .policy import ApiPolicy
        from .batched import BatchStream
        from .economics import GrowthLedger
        from .episode_tape import financial_digest, tensor_digest, observation_fields
        torch.set_num_threads(1)
        torch.manual_seed(986)
        model = ApiPolicy(16)
        model.acting_validation = 'consolidated_v1'
        # PLAN is an ordinary unconstrained policy family. Favor it in this
        # synthetic fixture to exercise the short-time tail without ending it.
        with torch.no_grad():
            model.heads['family'].weight.zero_()
            model.heads['family'].bias.zero_()
            model.heads['family'].bias[11] = 100
        results = []
        for compute_steps in (8, 64):
            torch.manual_seed(128)
            env = environment(); env.end = 1000000
            original = id(env)
            stream = BatchStream([env], model, [GrowthLedger(env, development=True)],
                                 calibration={'decision_latency_ms': 2}, rollout_only=True)
            rows = []
            while stream.decisions[0] < 64:
                chunk = stream.collect(compute_steps)
                for f in chunk.frames:
                    rows.append((tensor_digest(observation_fields(f)),
                                 tensor_digest({k: f[k] for k in ('actions', 'old_logprob', 'old_value', 'initial_hidden')}),
                                 f['economic_reward'], f['elapsed_ms']))
            self.assertEqual(id(env), original)
            self.assertEqual(stream.decisions, [64])
            results.append((rows, financial_digest(env), env.now, stream.hidden.clone(), torch.get_rng_state()))
        a, b = results
        self.assertEqual(a[:3], b[:3])
        self.assertTrue(torch.equal(a[3], b[3]))
        self.assertTrue(torch.equal(a[4], b[4]))


if __name__ == '__main__':
    unittest.main()
