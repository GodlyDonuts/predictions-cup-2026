import hashlib,json,random,tempfile,unittest
from pathlib import Path
import torch
from .experiment import Experiment
from .population import ResearchArchive
from .selection import DataUse
from .test_actions import environment


class FixtureExperiment(Experiment):
    def __init__(self,root):
        self.output=root/'run';self.output.mkdir();self.device='cpu';self.seed=81;self.rng=random.Random(81)
        self.development=True;self.model_size=16;self.dataset_sha='a'*64;self.spec_sha='b'*64
        self.config={'members':4,'cycles':2,'fresh_founders':4,'development_episode_ms':200,
                     'compute_chunk_decisions':4,'learning_rate':1e-4,'maximum_mark_age_ms':120000,
                     'training_snapshot_every_updates':4}
        self.search_config={'islands':2,'probe_decisions':4,'archive_size':8,'development_selection_blocks':2}
        self.archive=ResearchArchive(self.dataset_sha,self.spec_sha,8)
        self.train_bounds=(1,501);self.evolution_bounds=(501,1001)
        self.total_decisions=0;self.total_updates=0;self.population=[];self.last_snapshot_bucket=0
        self.calibration={'decision_latency_ms':2};self.data_use=DataUse(root/'registry');self.events=[]

    def world(self,role,start,end,finalist=None,stress=False):
        env=environment();env.end=end;env.start=start;env.advance(start);return env

    def log(self,event):self.events.append(event)


class PopulationExperimentTests(unittest.TestCase):
    def test_two_real_training_cycles_keep_island_elites_and_use_fresh_child_rollouts(self):
        torch.set_num_threads(1);torch.manual_seed(81)
        with tempfile.TemporaryDirectory()as d:
            experiment=FixtureExperiment(Path(d));summary=experiment.run()
            replacements=[e for e in experiment.events if e['event']=='population_replacement']
            self.assertEqual(len(replacements),2)
            self.assertGreater(summary['total_accepted_updates'],0)
            self.assertFalse(summary['independent_test_used']);self.assertFalse(summary['production_changed'])
            for r in replacements:
                self.assertEqual(r['donor']%2,r['loser']%2)
                self.assertNotEqual(r['donor'],r['loser'])
                self.assertTrue(r['mutation']['training_only_probe'])
                child=torch.load(r['checkpoint']['path'],map_location='cpu',weights_only=True)
                self.assertEqual(child['optimizer']['state'],{})
                later=json.loads((experiment.output/('result-001-%03d.json'%r['loser'])).read_bytes())
                self.assertGreater(sum(later['training']['family_counts'].values()),0)
                cases=later['selection']['cases'];self.assertEqual(len(cases),4)
                self.assertEqual([(c['start_ms'],c['end_ms'])for c in cases],[(501,751),(501,751),(751,1001),(751,1001)])
            archive=json.loads((experiment.output/'research-archive-001.json').read_bytes())
            self.assertTrue(archive['protected_from_weak_slot_replacement'])
            self.assertFalse(archive['production_authority'])
            snapshots=[e for e in experiment.events if e['event']=='training_weight_snapshot']
            self.assertTrue(snapshots)
            self.assertTrue(all(not e['financial_world_resume_supported']for e in snapshots))
            self.assertTrue(all(not e['selection_or_profitability_result']for e in snapshots))
            recovery=list((experiment.output/'training-recovery').glob('slot-*.json'))
            self.assertLessEqual(len(recovery),2)
            for path in recovery:
                slot=json.loads(path.read_bytes())['checkpoint']
                self.assertEqual(hashlib.sha256(Path(slot['path']).read_bytes()).hexdigest(),slot['sha256'])
