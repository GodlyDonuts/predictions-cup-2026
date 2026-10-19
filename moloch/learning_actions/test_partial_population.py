"""Software-only preservation fixtures, never historical learning evidence."""
import json,tempfile,unittest
from pathlib import Path
import torch
from .policy import ApiPolicy
from .training import save_checkpoint
from .selection import frozen
from .partial_population import preserve,verify
from .population_handoff import next_configuration


class PartialPopulationTests(unittest.TestCase):
    def fixture(self,root):
        run=root/'old';run.mkdir();recovery=run/'training-recovery';recovery.mkdir()
        spec=frozen(run/'experiment.json',{'config':{'mode':'inspected_development','members':2},
            'dataset_manifest_sha256':'a'*64,'full_history_experiment':False,
            'independent_test_used':False,'production_authority':False})
        model=ApiPolicy(16);opt=torch.optim.Adam(model.parameters(),lr=1e-5)
        opt.param_groups[0]['tartarus_entropy_coefficient']=.001
        def checkpoint(p):return {'path':str(p.resolve()),'sha256':save_checkpoint(p,model,opt,
            dataset_manifest_sha256='a'*64,split_specification_sha256=spec)}
        cp=checkpoint(run/'cycle-000-member-000.pt');active=checkpoint(recovery/'slot-1.pt')
        result={'member':0,'cycle':0,'checkpoint':cp,'training':{},'selection':{'reward':-.1}}
        frozen(run/'result-000-000.json',result)
        frozen(recovery/'slot-1.json',{'checkpoint':active,'total_updates':7,'total_decisions':80,
            'financial_world_resume_supported':False,'selection_or_profitability_result':False,'rotating_recovery_copy':True})
        rows=[{'event':'member_evaluated',**result},{'event':'training_weight_snapshot','checkpoint':active,'total_updates':7}]
        (run/'progress.jsonl').write_text(''.join(json.dumps(r)+'\n'for r in rows));return run

    def test_every_member_survives_without_fictitious_completed_cycle_or_ranking(self):
        torch.set_num_threads(1)
        with tempfile.TemporaryDirectory()as directory:
            root=Path(directory);run=self.fixture(root)
            plan=preserve(run,root/'preserved',members=2,hidden_size=16,reason='software fixture protocol correction')
            self.assertFalse(plan['completedGeneration']);self.assertEqual(plan['unfinishedMember'],1)
            self.assertEqual(plan['activeSnapshotUpdates'],7);self.assertEqual(len(plan['parents']),2)
            config=next_configuration({'mode':'inspected_development','members':2,'transferred_completed_cycle':100},plan,compute_steps=64)
            self.assertNotIn('transferred_completed_cycle',config);self.assertEqual(config['transferred_partial_cycle'],0)
            self.assertEqual(config['transferred_partial_snapshot_updates'],7)
            self.assertEqual(config['initial_entropy_coefficients'],[.001,.001]);self.assertEqual(config['fresh_founders'],0)
            # Rotating the source slot cannot alter the preserved immutable copy.
            (run/'training-recovery/slot-1.pt').write_bytes(b'rotated fixture')
            verify(plan,run,members=2,hidden_size=16)
            Path(plan['parents'][1]['path']).write_bytes(b'changed fixture')
            with self.assertRaisesRegex(ValueError,'CHECKPOINT_CHANGED'):verify(plan,run,members=2,hidden_size=16)

    def test_completed_generation_and_unproven_active_member_are_refused(self):
        with tempfile.TemporaryDirectory()as directory:
            root=Path(directory);run=self.fixture(root)
            progress=run/'progress.jsonl';original=progress.read_text()
            progress.write_text(original+json.dumps({'event':'cycle_complete','cycle':0})+'\n')
            with self.assertRaisesRegex(ValueError,'COMPLETED_GENERATION'):preserve(run,root/'no1',members=2,hidden_size=16,reason='fixture')
            progress.write_text(original.splitlines()[0]+'\n')
            with self.assertRaisesRegex(ValueError,'SNAPSHOT_REQUIRED'):preserve(run,root/'no2',members=2,hidden_size=16,reason='fixture')
