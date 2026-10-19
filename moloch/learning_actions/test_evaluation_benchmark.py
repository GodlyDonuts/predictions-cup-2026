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

    def test_book_priming_waits_for_original_source_event_and_charges_available_targets(self):
        torch.set_num_threads(1);torch.manual_seed(711)
        with tempfile.TemporaryDirectory()as directory:
            root=Path(directory);model=ApiPolicy(16);p=root/'fixed.pt'
            sha=save_checkpoint(p,model,torch.optim.Adam(model.parameters()),
                dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            def factory(config,output):
                output.mkdir();x=FixtureExperiment(output);x.evolution_bounds=(501,10001)
                x.frozen={'source_sha256':{}};x.close=lambda:None;base_world=x.world
                def delayed(*args,**kwargs):
                    e=base_world(*args,**kwargs);base=type(e)
                    class Delayed(base):
                        def _event(self,at,kind,value):
                            if kind=='fixture_book_target_arrival':self.keys['SIG'].extend(value);return
                            return super()._event(at,kind,value)
                    e.__class__=Delayed;targets=[k for k in e.keys['SIG']if k[0]=='book']
                    self.assertTrue(targets);e.keys['SIG']=[k for k in e.keys['SIG']if k[0]!='book']
                    e.clock.schedule(e.now+23,0,'fixture_book_target_arrival',targets);return e
                x.world=delayed;return x
            with patch('moloch.learning_actions.evaluation_heads_benchmark.Experiment',factory):
                r=compare({'mode':'inspected_development','device':'cpu'},
                    {'path':str(p),'sha256':sha},root/'proof',steps=4,prime_books=-1)
            for row in r['measurements']:
                self.assertEqual(row['sourceWarmupVirtualMs'],[23]*4)
                self.assertTrue(all(n>0 for n in row['simulatedPrimingReads']))
                self.assertTrue(all(n>0 for n in row['deliveredBooks']))

    def test_independent_test_config_is_refused_before_any_data_open(self):
        with self.assertRaisesRegex(ValueError,'NO_INDEPENDENT_TEST'):
            compare({'mode':'inspected_development','test_finalist_once':True},{},'unused')

    def test_reporting_sized_requests_cannot_hide_unexplained_financial_digest_change(self):
        torch.set_num_threads(1);torch.manual_seed(712)
        with tempfile.TemporaryDirectory()as directory:
            root=Path(directory);model=ApiPolicy(16);p=root/'fixed.pt'
            sha=save_checkpoint(p,model,torch.optim.Adam(model.parameters()),
                dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            def factory(config,output):
                output.mkdir();x=FixtureExperiment(output);x.frozen={'source_sha256':{}};x.close=lambda:None;return x
            calls=[0]
            def changed(env):
                calls[0]+=1;return ('a'if calls[0]<=4 else'b')*64
            with patch('moloch.learning_actions.evaluation_heads_benchmark.Experiment',factory),\
                 patch('moloch.learning_actions.evaluation_heads_benchmark.financial_digest',changed):
                with self.assertRaisesRegex(ValueError,'UNEXPLAINED_FULL_FINANCIAL'):
                    compare({'mode':'inspected_development','device':'cpu'},
                        {'path':str(p),'sha256':sha},root/'proof',steps=2,allow_sized_request_differences=True)
