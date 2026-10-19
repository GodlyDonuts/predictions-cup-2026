import hashlib
from pathlib import Path
import tempfile
import unittest
import torch
from .checkpoint_inference import SEMANTIC_SOURCES,load_frozen_policy
from .market_memory_policy import MarketMemoryPolicy
from .train_inner import fingerprint


class FrozenInferenceTests(unittest.TestCase):
    def bundle(self):
        model=MarketMemoryPolicy(3,hidden_size=32,market_hidden_size=8)
        shape={'market_slots':3,'global_hidden_size':32,'market_hidden_size':8,'actor_state_size':model.hidden_size}
        return model,{'architecture':model.architecture,'production_authority':False,'contract_features':24,'global_features':14,
                     'hidden_size':model.hidden_size,'model':model.state_dict(),'parameter_sha256':fingerprint(model),
                     'split_manifest_sha256':'a'*64,'provenance':{'architecture_shape':shape,
                         'source_sha256':{n:hashlib.sha256(Path(__file__).with_name(n).read_bytes()).hexdigest()for n in SEMANTIC_SOURCES}}}

    def test_original_weights_survive_new_universe_and_memory_starts_empty(self):
        model,bundle=self.bundle()
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'checkpoint.pt';torch.save(bundle,p);raw=p.read_bytes()
            loaded,proof=load_frozen_policy(p,hashlib.sha256(raw).hexdigest(),market_slots=5)
            self.assertEqual(fingerprint(loaded),fingerprint(model));self.assertEqual(p.read_bytes(),raw)
            self.assertFalse(loaded.training);self.assertTrue(all(not p.requires_grad for p in loaded.parameters()))
            self.assertEqual(loaded.initial_state(1).shape,(1,32+5*9));self.assertEqual(float(loaded.initial_state(1).abs().sum()),0)
            self.assertFalse(proof['optimizer_restored']);self.assertFalse(proof['production_authority'])

    def test_changed_bytes_or_weights_are_rejected(self):
        _,bundle=self.bundle()
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'checkpoint.pt';torch.save(bundle,p)
            with self.assertRaisesRegex(ValueError,'CHECKPOINT_BYTES_CHANGED'):load_frozen_policy(p,'0'*64)
            bundle['parameter_sha256']='0'*64;torch.save(bundle,p)
            with self.assertRaisesRegex(ValueError,'CHECKPOINT_PARAMETER_FINGERPRINT'):
                load_frozen_policy(p,hashlib.sha256(p.read_bytes()).hexdigest())

    def test_changed_feature_source_or_original_state_shape_is_rejected(self):
        for mode in ['source','shape']:
            _,bundle=self.bundle()
            if mode=='source':bundle['provenance']['source_sha256']['episodes.py']='0'*64
            else:bundle['provenance']['architecture_shape']['actor_state_size']+=1
            with tempfile.TemporaryDirectory()as d:
                p=Path(d)/'checkpoint.pt';torch.save(bundle,p)
                with self.assertRaises(ValueError):load_frozen_policy(p,hashlib.sha256(p.read_bytes()).hexdigest())


    def test_interval_checkpoint_pins_distinct_features_and_interval_sources(self):
        from .interval_market_policy import IntervalMarketPolicy
        model=IntervalMarketPolicy(3,hidden_size=32,market_hidden_size=8)
        _,bundle=self.bundle();bundle.update(architecture=model.architecture,contract_features=32,global_features=16,
            hidden_size=model.hidden_size,model=model.state_dict(),parameter_sha256=fingerprint(model))
        for name in ['interval_market_policy.py','interval_episodes.py','interval_excess_reward.py']:
            bundle['provenance']['source_sha256'][name]=hashlib.sha256(Path(__file__).with_name(name).read_bytes()).hexdigest()
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'interval.pt';torch.save(bundle,p)
            loaded,_=load_frozen_policy(p,hashlib.sha256(p.read_bytes()).hexdigest())
            self.assertEqual((loaded.contract_features,loaded.global_features),(32,16))
            self.assertEqual(fingerprint(loaded),fingerprint(model))
            bundle['provenance']['source_sha256']['interval_episodes.py']='0'*64;torch.save(bundle,p)
            with self.assertRaisesRegex(ValueError,'CHECKPOINT_SEMANTIC_SOURCE_CHANGED:interval_episodes.py'):
                load_frozen_policy(p,hashlib.sha256(p.read_bytes()).hexdigest())


if __name__=='__main__':unittest.main()
