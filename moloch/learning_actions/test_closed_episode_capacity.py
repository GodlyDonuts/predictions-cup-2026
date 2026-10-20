import tempfile,unittest
from pathlib import Path
from unittest.mock import patch
import torch
from .closed_episode_capacity import run
from .test_population_experiment import FixtureExperiment


class ClosedCapacityTests(unittest.TestCase):
    def test_cpu_collect_seal_reconstruct_and_update_are_all_exercised(self):
        torch.set_num_threads(1)
        def factory(config,output):
            x=FixtureExperiment(Path(output).parent);x.config.update(config);x.frozen={'source_sha256':{}}
            return x
        with tempfile.TemporaryDirectory()as d,patch('moloch.learning_actions.experiment.Experiment',factory):
            result=run({'mode':'inspected_development','device':'cpu','closed_episode_return_replay':True,
                        'training_worlds':2},Path(d)/'run',episode_ms=50)
            self.assertTrue(result['fullReturnTargets']);self.assertTrue(result['optimizerChangedWeights'])
            self.assertGreater(result['acceptedUpdates'],0);self.assertEqual(result['worlds'],2)
            self.assertFalse(result['independentTestUsed']);self.assertFalse(result['profitabilityEstablished'])

    def test_ordinary_chunks_or_nonpositive_horizons_cannot_claim_closed_capacity(self):
        with tempfile.TemporaryDirectory()as d:
            for config,horizon in(({'device':'cpu','mode':'inspected_development'},50),
                ({'device':'cpu','mode':'full_history','closed_episode_return_replay':True},50),
                ({'device':'cpu','mode':'inspected_development','closed_episode_return_replay':True},0)):
                with self.assertRaises(ValueError):run(config,Path(d)/'unused',episode_ms=horizon)
