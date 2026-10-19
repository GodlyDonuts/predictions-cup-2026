"""Tiny synthetic paired-driver checks, not method-performance evidence."""
import tempfile,unittest
from pathlib import Path
from unittest.mock import patch
import torch
from .test_population_experiment import FixtureExperiment
from .paired_training_comparison import compare
from .selection import frozen
from .population import ResearchArchive


class ComparisonFixture(FixtureExperiment):
    def __init__(self,config,output):
        Path(output).mkdir(parents=True)
        super().__init__(Path(output))
        self.config.update(config);self.frozen={'source_sha256':{'software_fixture':'a'*64}}
        self.spec_sha=frozen(self.output/'experiment.json',{'config':self.config,'production_authority':False,
            'full_history_experiment':False,'independent_test_used':False,'dataset_manifest_sha256':self.dataset_sha})
        self.archive=ResearchArchive(self.dataset_sha,self.spec_sha,8)


class PairedComparisonTests(unittest.TestCase):
    def test_initial_parameters_and_calibration_match_and_both_paths_complete(self):
        torch.set_num_threads(1)
        with tempfile.TemporaryDirectory()as folder:
            config={'mode':'inspected_development','device':'cpu','members':2,'fresh_founders':2,'cycles':1,
                'seed':81,'development_episode_ms':200,'training_worlds':2,'hidden_size':16}
            with patch('moloch.learning_actions.paired_training_comparison.Experiment',ComparisonFixture),patch(
                'moloch.learning_actions.paired_training_comparison.calibrate',return_value={'decision_latency_ms':2}):
                result=compare(config,Path(folder)/'comparison')
            self.assertTrue(result['identicalFounderParameters']);self.assertEqual(len(result['results']),2)
            self.assertTrue(all(r['acceptedUpdates']>0 for r in result['results']))
            self.assertFalse(result['independentTestUsed']);self.assertFalse(result['fullSizeGpuQualified'])

    def test_production_test_and_unpaired_initialization_are_refused(self):
        with tempfile.TemporaryDirectory()as folder:
            base={'mode':'inspected_development','device':'cpu','members':2,'fresh_founders':2,'cycles':1}
            for change in({'mode':'full_history'},{'device':'cuda'},{'test_finalist_once':True},
                          {'fresh_founders':1},{'cycles':2},{'development_initial_checkpoint':{'path':'fixture'}}):
                with self.assertRaises(ValueError):compare({**base,**change},Path(folder)/'forbidden')
