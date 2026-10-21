import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import torch

from .family_target_query import FAMILY,LEGACY,install,mode,query
from .policy import ApiPolicy
from .behavior_guard import contract,require_contract
from .training import save_checkpoint,load_checkpoint,collect_chunk,sequence_forward,update


class FamilyTargetQueryTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(81)

    def test_zero_extension_preserves_parameters_actions_statistics_and_rng(self):
        from .stream import tensors
        from .test_actions import environment
        model=ApiPolicy(16);before=copy.deepcopy(model.state_dict())
        x=tensors(environment().observe(),'cpu');h=model.initial_state(1);reset=torch.tensor([True])
        def act():
            encoded,hidden=model.encode(x['contracts'],x['global_state'],x['present'],h,reset)
            return model._act_from_encoding(encoded,hidden,x['present'],x['masks'])
        state=torch.get_rng_state().clone()
        a=act();after=torch.get_rng_state().clone();torch.set_rng_state(state)
        old=contract(model);report=install(model,FAMILY)
        self.assertTrue(torch.equal(state,torch.get_rng_state()));self.assertEqual(report['newParameters'],3072)
        for name,value in before.items():self.assertTrue(torch.equal(value,model.state_dict()[name]),name)
        b=act();self.assertTrue(torch.equal(after,torch.get_rng_state()))
        for name in a['actions']:torch.testing.assert_close(a['actions'][name],b['actions'][name],rtol=0,atol=0)
        for name in ('logprob','entropy','hidden','value'):torch.testing.assert_close(a[name],b[name],rtol=0,atol=0)
        with self.assertRaisesRegex(ValueError,'PROTOCOL_CHANGED'):require_contract(model,old)
        with self.assertRaisesRegex(ValueError,'EXPLICIT_BEHAVIOR'):require_contract(model,None)
        with self.assertRaisesRegex(ValueError,'CANNOT_DISCARD'):install(model,LEGACY)

    def test_buy_and_sell_can_learn_opposite_targets_in_identical_state(self):
        model=ApiPolicy(16)
        with torch.no_grad():model.market_query.weight.zero_();model.market_query.bias.zero_()
        encoded=torch.zeros(2,2,256);encoded[:,0,0]=1;encoded[:,1,0]=-1
        hidden=torch.zeros(2,16);families=torch.tensor([1,2])
        present=torch.ones(2,2,dtype=torch.bool);fm=torch.zeros(2,2,12,dtype=torch.bool)
        fm[0,:,1]=True;fm[1,:,2]=True;masks={'family':fm,'market':present}
        def logits():return (encoded*query(model,hidden,families).unsqueeze(1)).sum(-1)/16
        self.assertTrue(torch.equal(logits()[0],logits()[1]));install(model,FAMILY)
        optimizer=torch.optim.SGD([model.target_family_embedding.weight],lr=32)
        torch.nn.functional.cross_entropy(logits(),torch.tensor([0,1])).backward();optimizer.step()
        out=model._act_from_encoding(encoded,hidden,present,masks,deterministic=True)
        self.assertEqual(out['actions']['market'].tolist(),[0,1])
        self.assertEqual(out['actions']['family'].tolist(),[1,2])
        self.assertTrue(bool((logits().softmax(-1)>0).all()))

    def test_legacy_and_historical_checkpoint_roundtrip(self):
        from .historical_neural import HistoricalNeuralPolicy
        with tempfile.TemporaryDirectory()as td:
            for cls in (ApiPolicy,HistoricalNeuralPolicy):
                for extended in (False,True):
                    model=cls(16)
                    if extended:
                        install(model,FAMILY)
                        with torch.no_grad():model.target_family_embedding.weight.normal_()
                    path=Path(td)/(cls.__name__+str(extended)+'.pt')
                    sha=save_checkpoint(path,model,torch.optim.Adam(model.parameters()),
                        dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
                    # Actual old bundle format omitted the profile entirely.
                    if not extended:
                        bundle=torch.load(path,weights_only=True);del bundle['target_query_profile']
                        torch.save(bundle,path);sha=hashlib.sha256(path.read_bytes()).hexdigest()
                    loaded,_=load_checkpoint(path,sha)
                    self.assertEqual(mode(loaded),FAMILY if extended else LEGACY)
                    self.assertEqual(contract(model),contract(loaded))
                    for name,value in model.state_dict().items():torch.testing.assert_close(value,loaded.state_dict()[name],rtol=0,atol=0)

    def test_real_ppo_replay_reaches_and_updates_new_parameters(self):
        from .stream import Stream
        from .test_actions import environment
        model=ApiPolicy(16);install(model,FAMILY)
        with torch.no_grad():model.fractions.bias[2]=-10.;model.fractions.bias[3]=100.
        stream=Stream(environment(),model,calibration={'decision_latency_ms':1},record_training=True,
                      behavior_guard='tensor_versions_v1')
        chunk=collect_chunk(stream,16)
        out,_=sequence_forward(model,chunk.frames,chunk.initial_hidden)
        out['logprob'].sum().backward()
        self.assertGreater(float(model.target_family_embedding.weight.grad.abs().sum()),0)
        model.zero_grad(set_to_none=True)
        reports=update(model,torch.optim.Adam(model.parameters(),lr=1e-6),chunk,
                       [0.]*len(chunk.frames),role='train',epochs=1)
        self.assertTrue(reports[0]['accepted'])
        self.assertGreater(float(model.target_family_embedding.weight.detach().abs().sum()),0)

    def test_population_probe_and_every_mutation_include_extension(self):
        from .population import behavior,distance,mutate
        from .test_population import probe
        model=ApiPolicy(16);install(model,FAMILY);p=probe();before=behavior(model,p)
        # This tape has one tradable contract but several distinct read targets.
        with torch.no_grad():model.target_family_embedding.weight[10].normal_(0,4)
        self.assertGreater(distance(before,behavior(model,p))['family_target_kl'],0)
        state=copy.deepcopy(model.state_dict())
        for operator in ('local','medium','large','head_reset','fresh'):
            model.load_state_dict(state);mutate(model,p,operator,42,maximum_trials=2)
            self.assertEqual(mode(model),FAMILY)
            self.assertFalse(torch.equal(state['target_family_embedding.weight'],model.target_family_embedding.weight))
            if operator=='fresh':self.assertEqual(int(torch.count_nonzero(model.target_family_embedding.weight)),0)

    def test_invalid_configuration_fails_before_source_access(self):
        from .experiment import Experiment
        with tempfile.TemporaryDirectory()as td:
            with self.assertRaisesRegex(ValueError,'EXPLICIT_FAMILY_TARGET'):
                Experiment({'target_query_profile':'typo'},Path(td)/'run')
            self.assertFalse((Path(td)/'run').exists())

    def test_inherited_real_experiment_extends_founder_and_records_fresh_fitness(self):
        from .test_disk_tape import dataset
        from .disk_tape import build
        from .experiment import Experiment
        from .snapshot_clock import MODE
        with tempfile.TemporaryDirectory()as td:
            root=Path(td);source=dataset(root);cache=root/'cache';digest=build(source,cache)
            config={'mode':'inspected_development','device':'cpu','cpu_threads':1,'dataset':str(source),
                'mechanics':str(Path(__file__).resolve().parents[2]/'research/tartarus-learning-v2/development-mechanics-v2.json'),
                'replay_cache':{'path':str(cache),'manifest_sha256':digest},
                'development_train_ms':[1,1001],'development_selection_ms':[1001,5000],
                'development_episode_ms':100,'initial_world_history_ms':0,'members':2,'cycles':1,
                'hidden_size':16,'compute_chunk_decisions':2,'snapshot_world_clock':MODE}
            old=Experiment(config,root/'old')
            try:
                parent=old.make_policy();cp=old.checkpoint('parent',parent,old.make_optimizer(parent))
            finally:old.close()
            new_config={**config,'target_query_profile':FAMILY,'development_initial_checkpoints':[cp,cp]}
            new=Experiment(new_config,root/'new');new.calibration={'decision_latency_ms':20}
            try:
                import contextlib,io
                with contextlib.redirect_stdout(io.StringIO()):result=new.run()
                for member in range(2):
                    migration=json.loads((new.output/f'founder-{member:03d}-target-query-migration.json').read_text())
                    self.assertTrue(migration['freshTrajectoriesAndScoresRequired'])
                    founder,_=load_checkpoint(new.population[member]['path'],new.population[member]['sha256'])
                    self.assertEqual(mode(founder),FAMILY)
                self.assertGreater(result['total_accepted_updates'],0)
                self.assertFalse(result['independent_test_used'])
            finally:new.close()


if __name__=='__main__':unittest.main()
