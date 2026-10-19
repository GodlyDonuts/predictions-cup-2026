import hashlib
from pathlib import Path
import tempfile
import unittest
import torch
from .policy import ApiPolicy
from .training import save_checkpoint,load_checkpoint
from .stream import Stream
from .test_actions import environment
from .observation_protocol import CURRENT,LEGACY,migrate_development
from .behavior_guard import identity


class ObservationProtocolTests(unittest.TestCase):
    def test_checkpoint_roundtrip_and_legacy_identity_cannot_silently_enter_new_replay(self):
        with tempfile.TemporaryDirectory()as folder:
            model=ApiPolicy(16);p=Path(folder)/'new.pt'
            digest=save_checkpoint(p,model,torch.optim.Adam(model.parameters()),
                dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            loaded,bundle=load_checkpoint(p,digest)
            self.assertEqual(loaded.observation_protocol,CURRENT)
            self.assertEqual(bundle['observation_protocol'],CURRENT)
            bundle.pop('observation_protocol');old=Path(folder)/'old.pt';torch.save(bundle,old)
            legacy,_=load_checkpoint(old,hashlib.sha256(old.read_bytes()).hexdigest())
            self.assertEqual(legacy.observation_protocol,LEGACY)
            with self.assertRaisesRegex(ValueError,'PROTOCOL_MISMATCH'):
                Stream(environment(),legacy,calibration={'decision_latency_ms':1})
            with self.assertRaisesRegex(ValueError,'EXPLICIT_DEVELOPMENT'):
                migrate_development(legacy,{'mode':'inspected_development'})
            before={k:v.clone()for k,v in legacy.state_dict().items()}
            migration=migrate_development(legacy,{'mode':'inspected_development','migrate_observation_protocol':True})
            self.assertTrue(migration['old_scores_discarded']);self.assertTrue(migration['old_trajectories_discarded'])
            self.assertTrue(all(torch.equal(v,before[k])for k,v in legacy.state_dict().items()))
            self.assertEqual(legacy.observation_protocol,CURRENT)
            Stream(environment(),legacy,calibration={'decision_latency_ms':1})

    def test_unknown_protocol_and_full_history_relabelling_rejected(self):
        model=ApiPolicy(16);model.observation_protocol=LEGACY
        with self.assertRaisesRegex(ValueError,'EXPLICIT_DEVELOPMENT'):
            migrate_development(model,{'mode':'full_history','migrate_observation_protocol':True})
        model.observation_protocol='unregistered'
        with self.assertRaisesRegex(ValueError,'EXPLICIT_OBSERVATION_PROTOCOL'):
            migrate_development(model,{'mode':'inspected_development','migrate_observation_protocol':True})

    def test_fast_on_policy_guard_tracks_protocol_and_depth_migration_preserves_it(self):
        from .full_depth import migrate_legacy_policy
        model=ApiPolicy(16);before=identity(model,'tensor_versions_v1')
        model.observation_protocol=LEGACY
        self.assertNotEqual(identity(model,'tensor_versions_v1'),before)
        child,_=migrate_legacy_policy(model)
        self.assertEqual(child.observation_protocol,LEGACY)
        self.assertEqual(environment().observe()['observation_protocol'],CURRENT)


if __name__=='__main__':unittest.main()
