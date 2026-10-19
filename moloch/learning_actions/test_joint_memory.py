"""Likelihood regressions; synthetic fixtures do not establish trading ROI."""
import tempfile,unittest
from pathlib import Path
import torch
from torch.distributions import Beta,Categorical
from .query_policy import QueryWindowPolicy
from .joint_memory_policy import JointMemoryPolicy,migrate_query_memory
from .full_depth import FullDepthEpisode
from .test_actions import environment
from .stream import tensors,Stream
from .training import collect_chunk,sequence_forward,update,save_checkpoint,load_checkpoint
from .economics import GrowthLedger
from .behavior_guard import contract
from .observation_protocol import MEMORY_JOINT,SCOPED_EXECUTION


def world():
    e=environment();e.__class__=FullDepthEpisode;e.observation_protocol=MEMORY_JOINT;return e


class JointMemoryTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(422)

    def pair(self):
        old=QueryWindowPolicy(16);old.observation_protocol=SCOPED_EXECUTION
        new,report=migrate_query_memory(old)
        self.assertTrue(report['all_existing_weights_preserved_exactly'])
        return old,new

    def args(self,model):
        e=world();return e,dict(**tensors(e.observe(),'cpu'),hidden=model.initial_state(1),reset=torch.tensor([True]))

    def test_equal_weights_and_rng_keep_all_actions_hidden_and_next_rng_exact(self):
        old,new=self.pair();e,args=self.args(old);rng=torch.get_rng_state()
        a=old.step(**args);after=torch.get_rng_state();torch.set_rng_state(rng);b=new.step(**args)
        for k in a['actions']:self.assertTrue(torch.equal(a['actions'][k],b['actions'][k]),k)
        self.assertTrue(torch.equal(a['hidden'],b['hidden']));self.assertTrue(torch.equal(after,torch.get_rng_state()))
        self.assertFalse(torch.equal(a['logprob'],b['logprob']))

    def test_clear_has_same_physical_effect_but_distinct_self_memory_and_joint_score(self):
        old,new=self.pair();e,args=self.args(old);a=old.step(**args)['actions']
        # CLEAR is a local builder operation; every raw field is still memory.
        a['family'].fill_(11);o=e.observe();a['market'].fill_(int(o['masks']['family'][:,11].nonzero()[0][0]))
        e.act_raw({k:v[0].item()for k,v in a.items()},o);x=e.observe();i=x['entities'].index(('local_action',0));row=x['contracts'][i].copy()
        cash=e.cash;lots=dict(e.lots);a['price'].fill_((a['price'].item()+1)%200)
        e.act_raw({k:v[0].item()for k,v in a.items()},e.observe());x=e.observe();i=x['entities'].index(('local_action',0))
        self.assertNotEqual(row[1],x['contracts'][i,1]);self.assertEqual(cash,e.cash);self.assertEqual(lots,e.lots)
        with torch.no_grad():
            old.heads['price'].bias[a['price'].item()]+=2;new.heads['price'].bias[a['price'].item()]+=2
        # Compare parameter sensitivity using the same fixed action.
        before=QueryWindowPolicy(16);before.load_state_dict(new.state_dict())
        with torch.no_grad():before.heads['price'].bias[a['price'].item()]-=2
        l=before.step(**args,actions=a)['logprob'];r=old.step(**args,actions=a)['logprob']
        torch.testing.assert_close(l,r,rtol=0,atol=0)
        new.zero_grad();new.step(**args,actions=a)['logprob'].sum().backward()
        for head in (new.heads['price'],new.heads['side'],new.heads['patience'],new.fractions,new.query_filter,new.query_times):
            self.assertGreater(float(head.weight.grad.abs().sum()),0)

    def test_population_migrates_legacy_weights_and_trains_corrected_lineages(self):
        from .test_population_experiment import FixtureExperiment
        class JointExperiment(FixtureExperiment):
            def world(self,role,start,end,finalist=None,stress=False):
                e=world();e.end=end;e.start=start;e.advance(start);return e
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);old,_=self.pair();p=root/'legacy.pt';opt=torch.optim.Adam(old.parameters(),lr=1e-5)
            from .selection import frozen
            spec=frozen(root/'experiment.json',{'config':{'mode':'inspected_development'},
                'dataset_manifest_sha256':'a'*64,'full_history_experiment':False,
                'independent_test_used':False,'production_authority':False,
                'actual_train_bounds_ms':[1,501],'actual_evolution_bounds_ms':[501,1001]})
            sha=save_checkpoint(p,old,opt,dataset_manifest_sha256='a'*64,split_specification_sha256=spec)
            exp=JointExperiment(root);exp.config.update(joint_local_memory_density=True,trade_query_windows=True,
                scoped_execution_receipts=True,full_depth_observations=True,observation_protocol=MEMORY_JOINT,
                migrate_observation_protocol=True,migrate_joint_memory_density=True,mode='inspected_development',
                development_initial_checkpoints=[{'path':str(p),'sha256':sha}],members=2,fresh_founders=1)
            exp.search_config['islands']=1;result=exp.run()
            self.assertGreater(result['total_accepted_updates'],0)
            for cp in exp.population:
                m,_=load_checkpoint(cp['path'],cp['sha256']);self.assertIs(type(m),JointMemoryPolicy)
            self.assertFalse(result['independent_test_used']);self.assertFalse(result['production_changed'])

    def test_joint_logprob_matches_explicit_all_head_reference(self):
        _,m=self.pair();e,args=self.args(m);out=m.step(**args);a=out['actions']
        encoded=m.encoder(args['contracts']);h=out['hidden'];present=args['present'];masks=args['masks'];batch=torch.arange(1)
        pooled=(encoded*present.unsqueeze(-1)).sum(1)/present.sum(1,keepdim=True)
        logits=m.heads['family'](torch.cat((h,pooled),-1));mask=(masks['family']&present.unsqueeze(-1)).any(1)
        from moloch.learning_v2.policy import masked_categorical
        expected=masked_categorical(logits,mask).log_prob(a['family']).double()
        logits=(encoded*m.market_query(h).unsqueeze(1)).sum(-1)/16
        eligible=masks['family'].gather(2,a['family'][:,None,None].expand(-1,present.shape[1],1)).squeeze(-1)
        expected+=masked_categorical(logits,masks['market']&present&eligible).log_prob(a['market']).double()
        selected=torch.cat((h,encoded[batch,a['market']]),-1)+m.conditioners['family'](a['family'])
        order=(a['family']==1)|(a['family']==2)|(a['family']==5)|(a['family']==6)
        query=(a['family']==10)&masks['trade_query'][batch,a['market'],0]
        for name in ('side','price','patience'):
            expected+=Categorical(logits=m.heads[name](selected)).log_prob(a[name]).double()
            active=order|query if name=='side'else order if name=='price'else order&(a['price']!=0)
            selected=selected+m.conditioners[name](a[name])*active.unsqueeze(-1)
        params=(torch.nn.functional.softplus(m.fractions(selected))+1).double()
        for name,pair in [('target_fraction',(0,1)),('child_fraction',(2,3))]:expected+=Beta(params[:,pair[0]],params[:,pair[1]]).log_prob(a[name])
        selected=torch.cat((h,encoded[batch,a['market']]),-1)
        expected+=Categorical(logits=m.query_filter(selected)).log_prob(a['query_filter']).double()
        params=(torch.nn.functional.softplus(m.query_times(selected))+1).double()
        for name,pair in [('query_from_fraction',(0,1)),('query_to_fraction',(2,3))]:expected+=Beta(params[:,pair[0]],params[:,pair[1]]).log_prob(a[name])
        torch.testing.assert_close(expected,out['logprob'],atol=2e-6,rtol=2e-6)

    def test_streamed_teacher_ppo_checkpoint_and_equal_weight_contract_rejection(self):
        old,m=self.pair();e=world();s=Stream(e,m,record_training=True,calibration={'decision_latency_ms':2})
        chunk=collect_chunk(s,8,reward_ledger=GrowthLedger(e,development=True));out,_=sequence_forward(m,chunk.frames,chunk.initial_hidden)
        torch.testing.assert_close(out['logprob'],torch.stack([f['old_logprob']for f in chunk.frames]),atol=2e-5,rtol=2e-5)
        opt=torch.optim.Adam(m.parameters(),lr=1e-5)
        saved=chunk.behavior_contract;chunk.behavior_contract=contract(old)
        with self.assertRaisesRegex(ValueError,'ARCHITECTURE_OR_PROTOCOL'):update(m,opt,chunk,[f['economic_reward']for f in chunk.frames],role='train')
        chunk.behavior_contract=None
        with self.assertRaisesRegex(ValueError,'EXPLICIT_BEHAVIOR'):update(m,opt,chunk,[f['economic_reward']for f in chunk.frames],role='train')
        chunk.behavior_contract=saved;self.assertTrue(update(m,opt,chunk,[f['economic_reward']for f in chunk.frames],role='train',epochs=1)[0]['accepted'])
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'joint.pt';sha=save_checkpoint(p,m,opt,dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            loaded,_=load_checkpoint(p,sha);self.assertIs(type(loaded),JointMemoryPolicy);self.assertEqual(loaded.observation_protocol,MEMORY_JOINT)

    def test_sealed_episode_rejects_equal_weights_with_other_architecture(self):
        from .episode_tape import EpisodeWriter,EpisodeReader
        from .episode_training import EpisodeSession,optimize
        from .batched import BatchStream
        old,m=self.pair()
        def make(a=0,b=20):
            e=world();e.end=b;return e
        with tempfile.TemporaryDirectory()as d:
            envs=[make(),make()];s=BatchStream(envs,m,[GrowthLedger(e,development=True)for e in envs],calibration={'decision_latency_ms':2})
            w=EpisodeWriter(Path(d)/'episode',s,dataset_sha256='a'*64,specification_sha256='b'*64)
            while any(e.now<e.end for e in envs):w.record(s.collect(4))
            sha=w.seal();r=EpisodeReader(w.path,sha)
            try:
                with self.assertRaisesRegex(ValueError,'ARCHITECTURE_OR_PROTOCOL'):EpisodeSession(r,old)
                opt=torch.optim.Adam(m.parameters(),lr=1e-5)
                reports=optimize(r,m,opt,make,lambda e:GrowthLedger(e,development=True))
                self.assertTrue(any(x['accepted']for x in reports))
            finally:r.close()
