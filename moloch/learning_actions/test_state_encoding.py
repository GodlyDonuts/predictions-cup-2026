import hashlib,json,tempfile,unittest
from pathlib import Path
import numpy as np
import torch
from .state_encoding import encode,INDICES,RAW,SIGNED_LOG
from .test_actions import environment
from .policy import ApiPolicy
from .stream import Stream
from .economics import GrowthLedger
from .training import collect_chunk,sequence_forward,update,save_checkpoint,load_checkpoint
from .test_population_experiment import FixtureExperiment
from .selection import frozen


class StateEncodingTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(72)

    def test_signed_log_has_no_clip_and_leaves_money_prices_and_flags_unchanged(self):
        raw=np.arange(16,dtype=np.float32);raw[list(INDICES)]=[-1,0,1,100,10000,1e6,1e12]
        transformed=encode(raw,SIGNED_LOG)
        np.testing.assert_array_equal(encode(raw,RAW),raw)
        untouched=[i for i in range(16)if i not in INDICES]
        np.testing.assert_array_equal(transformed[untouched],raw[untouched])
        recovered=np.sign(transformed)*np.expm1(np.abs(transformed.astype(np.float64)))
        np.testing.assert_allclose(recovered[list(INDICES)],raw[list(INDICES)],rtol=2e-6,atol=1e-6)
        self.assertEqual(raw[15],1e12);self.assertGreater(transformed[15],transformed[12])

    def test_observation_changes_no_physical_state_contract_input_or_action_support(self):
        e=environment();e.advance(500);before=e.observe();financial=(e.cash,e.now,dict(e.lots),len(e.api_fills))
        e.global_state_encoding=SIGNED_LOG;after=e.observe()
        self.assertEqual(financial,(e.cash,e.now,dict(e.lots),len(e.api_fills)))
        np.testing.assert_array_equal(before['contracts'],after['contracts'])
        for key in before['masks']:np.testing.assert_array_equal(before['masks'][key],after['masks'][key])
        np.testing.assert_array_equal(after['global_state'],encode(before['global_state'],SIGNED_LOG))
        with self.assertRaisesRegex(ValueError,'ENCODING_MISMATCH'):
            Stream(e,ApiPolicy(16),calibration={'decision_latency_ms':1})

    def test_fused_training_and_checkpoint_use_exact_same_encoded_inputs(self):
        e=environment();e.global_state_encoding=SIGNED_LOG;model=ApiPolicy(16);model.global_state_encoding=SIGNED_LOG
        with torch.no_grad():model.fractions.bias[2]=-10;model.fractions.bias[3]=100
        s=Stream(e,model,calibration={'decision_latency_ms':1},record_training=True,behavior_guard='tensor_versions_v1')
        chunk=collect_chunk(s,16,reward_ledger=GrowthLedger(e,development=True))
        out,_=sequence_forward(model,chunk.frames,chunk.initial_hidden)
        torch.testing.assert_close(out['logprob'],torch.stack([f['old_logprob']for f in chunk.frames]),atol=1e-6,rtol=1e-5)
        optimizer=torch.optim.Adam(model.parameters(),lr=1e-6)
        self.assertTrue(update(model,optimizer,chunk,[f['economic_reward']for f in chunk.frames],role='train',epochs=1)[0]['accepted'])
        with tempfile.TemporaryDirectory()as directory:
            p=Path(directory)/'model.pt';h=save_checkpoint(p,model,optimizer,dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            restored,_=load_checkpoint(p,h);self.assertEqual(restored.global_state_encoding,SIGNED_LOG)
            original=torch.load(p,weights_only=True);original['global_state_encoding']='unknown'
            bad=Path(directory)/'bad.pt';torch.save(original,bad)
            with self.assertRaisesRegex(ValueError,'EXPLICIT_GLOBAL_STATE'):load_checkpoint(bad,hashlib.sha256(bad.read_bytes()).hexdigest())

    def test_explicit_migration_keeps_weights_but_requires_new_training_and_scores(self):
        with tempfile.TemporaryDirectory()as directory:
            root=Path(directory);parent=ApiPolicy(16);optimizer=torch.optim.Adam(parent.parameters(),lr=1e-5)
            optimizer.param_groups[0]['tartarus_entropy_coefficient']=.01
            source={'config':{'mode':'inspected_development'},'dataset_manifest_sha256':'a'*64,
                'full_history_experiment':False,'independent_test_used':False,'production_authority':False,
                'actual_train_bounds_ms':[1,501],'actual_evolution_bounds_ms':[501,1001]}
            spec=frozen(root/'experiment.json',source);path=root/'parent.pt'
            h=save_checkpoint(path,parent,optimizer,dataset_manifest_sha256='a'*64,split_specification_sha256=spec)
            x=FixtureExperiment(root);x.config.update(global_state_encoding=SIGNED_LOG,fresh_founders=0,
                development_initial_checkpoints=[{'path':str(path),'sha256':h}]*4,cycles=1)
            with self.assertRaisesRegex(ValueError,'EXPLICIT_GLOBAL_STATE_ENCODING_MIGRATION'):x.run()
            x.config['migrate_global_state_encoding']=True
            old_world=x.world
            def world(*args,**kwargs):
                e=old_world(*args,**kwargs);e.global_state_encoding=SIGNED_LOG;return e
            x.world=world;x.run()
            child,_=load_checkpoint(x.output/'founder-000.pt',hashlib.sha256((x.output/'founder-000.pt').read_bytes()).hexdigest())
            self.assertEqual(child.global_state_encoding,SIGNED_LOG)
            for name,value in parent.state_dict().items():torch.testing.assert_close(value,child.state_dict()[name],atol=0,rtol=0)
            migration=json.loads((x.output/'founder-000-state-encoding-migration.json').read_text())
            self.assertTrue(migration['new_training_and_scores_required'])
