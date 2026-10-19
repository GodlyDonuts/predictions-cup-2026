import tempfile,unittest
from pathlib import Path
import numpy as np
import torch
from moloch.learning_realism.test_vector import tapes
from moloch.learning_realism.test_replay import fixture
from moloch.learning_v2.liquidity import Book,SharedLiquidity
from .full_depth import FullDepthPolicy,FullDepthTape,FEATURES,LEVELS,extend_observation,migrate_legacy_policy
from .policy import ApiPolicy
from .test_actions import environment
from .stream import Stream,tensors
from .training import collect_chunk,sequence_forward,update,save_checkpoint,load_checkpoint
from .economics import GrowthLedger
from .behavior_guard import identity
from .population import freeze_probe,mutate
from .test_population_experiment import FixtureExperiment


def depth_environment():
    _,base=tapes();t=FullDepthTape.from_loaded(base,split=base.split,role='train')
    return t.episode(fixture().mechanics,1,100000,initial_cash_micros=1000000000,
        target_wealth_micros=10000000000,burnin_ms=1)


class FullDepthTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(301)

    def test_same_old_features_can_hide_different_deep_execution_prices(self):
        e=environment();old=[];new=[]
        for tail in (600000,900000):
            e.actor_books['x']=SharedLiquidity(Book('x',0,0,((400000,100),),((450000,100),(tail,100)),'a'*64))
            observed=e.observe();old.append(observed['contracts']);new.append(extend_observation(e,observed)['contracts'])
        np.testing.assert_array_equal(old[0],old[1]);self.assertFalse(np.array_equal(new[0],new[1]))
        np.testing.assert_array_equal(new[0][:,:40],old[0])

    def test_no_hidden_world_depth_until_charged_delivery(self):
        e=depth_environment();self.assertFalse(e.actor_books)
        self.assertFalse(e.observe()['contracts'][:,40:].any())
        e.request_observation('SIG',('book','x'));e.advance(101)
        self.assertTrue(e.observe()['contracts'][:,40:].any());self.assertEqual(len(e.budget.reads),1)
        frozen=e.observe()['contracts'].copy()
        e.books['x'].update(Book('x',101,101,((300000,1000),),((800000,1000),),'b'*64))
        np.testing.assert_array_equal(e.observe()['contracts'],frozen)

    def test_every_real_price_tick_and_large_displayed_quantities_are_retained(self):
        e=environment();base=e.observe();i=base['entities'].index(('contract','x'))
        for side in ('bids','asks'):
            prices=list(range(5000,1000000,5000));prices.sort(reverse=side=='bids')
            levels=tuple((p,2147483648+p)for p in prices)
            e.actor_books['x']=SharedLiquidity(Book('x',0,0,levels if side=='bids'else(),levels if side=='asks'else(),'a'*64))
            full=extend_observation(e,e.observe());start=40+(0 if side=='bids'else LEVELS)
            self.assertEqual(np.count_nonzero(full['contracts'][i,start:start+LEVELS]),199)
            self.assertEqual(full['contracts'].shape[-1],FEATURES)
            np.testing.assert_array_equal(full['masks']['family'],base['masks']['family'])

    def test_off_tick_depth_is_rejected_without_rounding_or_truncation(self):
        e=environment();e.actor_books['x']=SharedLiquidity(Book('x',0,0,((400001,100),),((500000,100),),'a'*64))
        with self.assertRaisesRegex(ValueError,'PRICE_LATTICE'):extend_observation(e,e.observe())

    def test_own_draft_contents_and_sampled_action_are_visible_without_becoming_targets(self):
        e=depth_environment();before=e.now;observation=e.observe()
        action=dict(family=5,market=1,side=0,price=90,patience=0,target_fraction=.1,child_fraction=.01)
        self.assertEqual(e.act_raw(action,observation)['status'],'BUILDER_UPDATED')
        view=e.observe();i=view['entities'].index(('builder_order',0));j=view['entities'].index(('local_action',0))
        self.assertEqual(float(view['contracts'][i,0]),np.float32(.45));self.assertEqual(view['contracts'][i,2],1.)
        self.assertEqual(view['contracts'][j,0],np.float32(5/11))
        self.assertFalse(view['masks']['family'][i].any());self.assertFalse(view['masks']['market'][j])
        self.assertEqual(e.now,before);self.assertFalse(e.budget.writes)
        e.act_raw({**action,'family':11,'market':0},view)
        self.assertFalse(any(kind=='builder_order'for kind,key in e.observe()['entities']))

    def test_last_local_status_does_not_reveal_an_unreceived_server_result(self):
        e=depth_environment();action=dict(family=1,market=1,side=0,price=90,patience=0,target_fraction=.1,child_fraction=.01)
        result=e.act_raw(action,e.observe());self.assertEqual(result['status'],'IN_FLIGHT')
        e.advance(e.now+e.mechanics.arrival_latency_ms)
        self.assertEqual(result['status'],'EXECUTED');self.assertFalse(e.responses)
        self.assertEqual(e._last_local_action['result_status'],'IN_FLIGHT')

    def test_explicit_zero_extended_migration_preserves_existing_weights_and_distribution(self):
        e=environment();old=ApiPolicy(16);new,report=migrate_legacy_policy(old)
        self.assertTrue(report['all_existing_weights_preserved_exactly']);self.assertFalse(report['legal_action_support_changed'])
        a=tensors(e.observe(),'cpu');b=tensors(extend_observation(e,e.observe()),'cpu');reset=torch.tensor([True])
        with torch.no_grad():
            first=old.step(**a,hidden=old.initial_state(1),reset=reset,deterministic=True)
            second=new.step(**b,hidden=new.initial_state(1),reset=reset,actions=first['actions'])
        for key in ('logprob','value','hidden','entropy'):torch.testing.assert_close(first[key],second[key],atol=1e-6,rtol=1e-5)

    def test_full_depth_checkpoint_roundtrip_and_fast_guard_cover_new_columns(self):
        model=FullDepthPolicy(16);optimizer=torch.optim.Adam(model.parameters())
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'full.pt';digest=save_checkpoint(p,model,optimizer,dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            restored,bundle=load_checkpoint(p,digest)
            self.assertIs(type(restored),FullDepthPolicy);self.assertEqual(restored.contract_features,438)
            self.assertEqual(bundle['architecture'],FullDepthPolicy.architecture)
            before=identity(restored,'tensor_versions_v1')
            with torch.no_grad():restored.encoder[0].weight[0,437]+=1
            self.assertNotEqual(before,identity(restored,'tensor_versions_v1'))

    def test_real_growth_labels_fused_update_and_fresh_mutation_keep_full_depth(self):
        e=depth_environment();e.request_observation('SIG',('book','x'));e.advance(101)
        model=FullDepthPolicy(16)
        with torch.no_grad():model.fractions.bias[2]=-10.;model.fractions.bias[3]=100.
        stream=Stream(e,model,calibration={'decision_latency_ms':1},record_training=True,behavior_guard='tensor_versions_v1')
        chunk=collect_chunk(stream,8,reward_ledger=GrowthLedger(e,development=True))
        self.assertEqual(chunk.frames[0]['contracts'].shape[-1],438)
        optimizer=torch.optim.Adam(model.parameters(),lr=1e-6)
        reports=update(model,optimizer,chunk,[f['economic_reward']for f in chunk.frames],role='train',epochs=1,checkpoint_encoder=True)
        self.assertTrue(reports[0]['accepted']);self.assertGreater(float(model.encoder[0].weight.grad[:,40:].abs().sum()),0)
        frames=[{**f,'at_ms':i,'reset':torch.tensor([i==0])}for i,f in enumerate(chunk.frames)]
        probe=freeze_probe(frames,role='train',dataset_sha256='a'*64,bounds=(0,100))
        mutate(model,probe,'fresh',491)
        self.assertEqual(model.contract_features,438);self.assertIs(type(model),FullDepthPolicy)

    def test_two_population_cycles_use_full_depth_without_changing_action_support(self):
        class FullFixture(FixtureExperiment):
            def __init__(self,root):
                super().__init__(root);self.config['full_depth_observations']=True
            def world(self,role,start,end,finalist=None,stress=False):
                e=depth_environment();e.end=end;e.start=start;e.advance(start);return e
        with tempfile.TemporaryDirectory()as d:
            x=FullFixture(Path(d));summary=x.run()
            self.assertGreater(summary['total_accepted_updates'],0);self.assertFalse(summary['independent_test_used'])
            for cp in x.population:
                model,_=load_checkpoint(cp['path'],cp['sha256']);self.assertIs(type(model),FullDepthPolicy)
