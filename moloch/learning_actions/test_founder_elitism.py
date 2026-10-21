"""Measured negative founders must survive a worse first trained generation."""
import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from .experiment import Experiment
from .training import load_checkpoint


class FounderElitismTests(unittest.TestCase):
    def test_invalid_profile_fails_before_source_or_output(self):
        with tempfile.TemporaryDirectory() as td:
            for config in ({'evaluate_founders_before_training':1},
                           {'evaluate_founders_before_training':True},
                           {'evaluate_founders_before_training':True,'population_search':{'retain_island_champions':False}}):
                with self.assertRaises(ValueError):Experiment(config,Path(td)/'unused')
                self.assertFalse((Path(td)/'unused').exists())

    def test_complete_run_retains_measured_founder_with_negative_score(self):
        self.run_retention_case(-.06)

    def test_positive_screened_founder_gets_full_window_confirmation(self):
        self.run_retention_case(.06,screened=True)

    def run_retention_case(self,founder_reward,*,screened=False):
        from .test_disk_tape import dataset
        from .disk_tape import build
        from .snapshot_clock import MODE
        with tempfile.TemporaryDirectory() as td:
            root=Path(td);source=dataset(root);cache=root/'cache';digest=build(source,cache)
            config={'mode':'inspected_development','device':'cpu','cpu_threads':1,
                    'dataset':str(source),'mechanics':str(Path(__file__).resolve().parents[2]/
                        'research/tartarus-learning-v2/development-mechanics-v2.json'),
                    'replay_cache':{'path':str(cache),'manifest_sha256':digest},
                    'development_train_ms':[1,1001],'development_selection_ms':[1001,5000],
                    'development_episode_ms':200,'initial_world_history_ms':0,
                    'members':2,'cycles':1,'hidden_size':16,'compute_chunk_decisions':2,
                    'snapshot_world_clock':MODE,'evaluate_founders_before_training':True,
                    'population_search':{'islands':1,'retain_island_champions':True}}
            if screened:config['development_selection_episode_ms']=1000
            # Real initialization/checkpoints/training/selection lifecycle with
            # controlled evaluation returns isolates the retention regression.
            experiment=Experiment(config,root/'run');experiment.calibration={'decision_latency_ms':20}
            scored=[]
            def evaluate(model,checkpoint,*args,**kwargs):
                bundle=load_checkpoint(checkpoint['path'],checkpoint['sha256'],'cpu')[1]
                self.assertEqual(bundle['split_specification_sha256'],experiment.spec_sha)
                founder=Path(checkpoint['path']).name.startswith('founder-')
                reward=(founder_reward if checkpoint['path'].endswith('000.pt')else-.1)if founder else-.24
                scored.append((checkpoint['path'],founder,reward,kwargs.get('confirmation',False)))
                return {'role':'development_selection','checkpoint_sha256':checkpoint['sha256'],
                        'reward':reward,'marked_roi_min':reward,'cases':[{'reward':reward,
                        'start_ms':1001,'end_ms':5000,'stress':False,'selection_seed':20,
                        'classification':'fixture','valuation':'fixture','evaluation_compute_mode':'fixture'}]}
            try:
                with patch.object(experiment,'evaluate',side_effect=evaluate),contextlib.redirect_stdout(io.StringIO()):
                    result=experiment.run()
                self.assertEqual([x[1]for x in scored],[True,True,True,False,False]if screened else[True,True,False,False])
                self.assertEqual(sum(x[3]for x in scored),int(screened))
                self.assertEqual((root/'run/development-confirmation-founder.json').exists(),screened)
                self.assertEqual(result['best']['selection']['reward'],founder_reward)
                self.assertEqual(result['best']['cycle'],-1)
                self.assertFalse(result['best']['training']['performed'])
                self.assertGreater(experiment.total_updates,0)
                self.assertTrue(any(Path(p['path']).name=='founder-000.pt'for p in experiment.population))
                self.assertTrue((root/'run/initial-island-champions.json').is_file())
                for n in ('founder-000.pt','cycle-000-member-000.pt','cycle-000-member-001.pt'):
                    self.assertTrue((root/'run'/n).is_file())
                self.assertFalse(result['independent_test_used'])
            finally:experiment.close()


if __name__=='__main__':unittest.main()
