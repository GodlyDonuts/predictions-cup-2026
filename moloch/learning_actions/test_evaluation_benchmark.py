"""Synthetic orchestration proof, distinct from finite real-data measurement."""
import tempfile,unittest
from pathlib import Path
from unittest.mock import patch
import torch
from .evaluation_heads_benchmark import compare
from .test_population_experiment import FixtureExperiment
from .policy import ApiPolicy
from .training import save_checkpoint


class EvaluationBenchmarkTests(unittest.TestCase):
    def test_finite_paired_selection_preserves_funding_clocks_rng_and_recurrent_state(self):
        torch.set_num_threads(1);torch.manual_seed(710)
        with tempfile.TemporaryDirectory()as directory:
            root=Path(directory);model=ApiPolicy(16);p=root/'fixed.pt'
            sha=save_checkpoint(p,model,torch.optim.Adam(model.parameters()),
                dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            def factory(config,output):
                output.mkdir()
                x=FixtureExperiment(output);x.frozen={'source_sha256':{}};x.close=lambda:None;return x
            with patch('moloch.learning_actions.evaluation_heads_benchmark.Experiment',factory):
                r=compare({'mode':'inspected_development','device':'cpu'},
                    {'path':str(p),'sha256':sha},root/'proof',steps=8)
            self.assertTrue(r['identicalFinanceLiquidityClocksAndCaseRng']);self.assertTrue(r['recurrentStatesMatchTolerance'])
            self.assertTrue(r['policyUnchanged']);self.assertEqual(len(r['measurements']),6)
            self.assertEqual(r['accountRequests'],0);self.assertFalse(r['independentTestUsed']);self.assertFalse(r['roiEstablished'])
            self.assertTrue((root/'proof/RESULT.json').is_file())

    def test_independent_test_config_is_refused_before_any_data_open(self):
        with self.assertRaisesRegex(ValueError,'NO_INDEPENDENT_TEST'):
            compare({'mode':'inspected_development','test_finalist_once':True},{},'unused')
