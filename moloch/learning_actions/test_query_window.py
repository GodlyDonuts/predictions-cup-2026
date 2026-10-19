import math,tempfile,unittest
from pathlib import Path
import numpy as np
import torch
from .query_window import timestamp,parameters,MAX_TIMESTAMP_MS
from .query_policy import QueryWindowPolicy,migrate_full_depth
from .full_depth import FullDepthPolicy,FullDepthEpisode,extend_observation
from .observation_protocol import QUERY_WINDOW,CURRENT,migrate_development
from .test_actions import environment
from .test_trade_history import trade
from .stream import Stream,tensors
from .training import save_checkpoint,load_checkpoint,collect_chunk,update,sequence_forward
from .economics import GrowthLedger
from .test_population_experiment import FixtureExperiment
from .population import freeze_probe,behavior,mutate


def query_environment():
    e=environment();e.__class__=FullDepthEpisode;e.observation_protocol=QUERY_WINDOW
    return e


class QueryWindowTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(991)

    def test_all_millisecond_bounds_and_default_cursor_choices(self):
        for at in(0,1,123,1792416000000,MAX_TIMESTAMP_MS-1,MAX_TIMESTAMP_MS):
            f=(at+.5)/(MAX_TIMESTAMP_MS+1)
            self.assertEqual(timestamp(f),at)
        self.assertEqual(timestamp(1.),MAX_TIMESTAMP_MS)
        for f in(-.1,1.1,float('nan'),float('inf')):
            with self.assertRaises(ValueError):timestamp(f)
        a={'side':0,'query_filter':1,'query_from_fraction':.8,'query_to_fraction':.2}
        self.assertEqual(parameters(a),{'trade_from_ms':timestamp(.2),'trade_to_ms':timestamp(.8)})
        self.assertFalse(parameters({**a,'query_filter':0}));self.assertFalse(parameters({**a,'side':1}))

    def test_policy_action_charges_query_and_delivers_only_requested_available_prints(self):
        e=query_environment();e.advance(50)
        for n in range(10):e.trade_history.accept(40,trade(n,40,n))
        # This already-stored future receipt must stay hidden even with a future to-bound.
        e.trade_history.accept(80,trade(11,80,3))
        obs=e.observe();i=obs['entities'].index(('read',('SIG',('trade_flow','x'))))
        action=dict(family=10,market=i,side=0,price=0,patience=0,target_fraction=.01,child_fraction=.1,
            query_filter=1,query_from_fraction=2.5/(MAX_TIMESTAMP_MS+1),query_to_fraction=5.5/(MAX_TIMESTAMP_MS+1))
        self.assertEqual(e.act_raw(action,obs)['status'],'IN_FLIGHT');self.assertFalse(e.known_trade_pages)
        e.advance(e.busy_until);page=e.known_trade_pages['x']
        self.assertEqual((page['from_ms'],page['to_ms']),(2,5))
        self.assertEqual([r['executed_at_ms']for r in page['data']],[4,3,2])
        self.assertNotIn('11',[r['match_id']for r in page['data']]);self.assertEqual(len(e.budget.reads),1)
        self.assertFalse(e.budget.writes)
        local=e.observe();j=local['entities'].index(('local_action',0))
        np.testing.assert_array_equal(local['contracts'][j,15:18],np.array(
            [1,action['query_from_fraction'],action['query_to_fraction']],np.float32))

    def test_teacher_forcing_query_density_and_irrelevant_latents(self):
        e=query_environment();o=e.observe();model=QueryWindowPolicy(16);view=tensors(o,'cpu')
        sample=model.step(**view,hidden=model.initial_state(1),reset=torch.tensor([True]))
        a={k:v.clone()for k,v in sample['actions'].items()}
        a['family'].fill_(10);a['market'].fill_(o['entities'].index(('read',('SIG',('trade_flow','x')))))
        a['side'].zero_();a['query_filter'].fill_(1)
        args=dict(**view,hidden=model.initial_state(1),reset=torch.tensor([True]),actions=a)
        left=model.step(**args);right=model.step(**args)
        self.assertTrue(torch.equal(left['logprob'],right['logprob']))
        left['logprob'].sum().backward();self.assertGreater(float(model.query_times.weight.grad.abs().sum()),0)
        for side,enabled in((1,1),(0,0)):
            a['side'].fill_(side);a['query_filter'].fill_(enabled)
            l=model.step(**args);a['query_from_fraction'].fill_(.123);a['query_to_fraction'].fill_(.876)
            r=model.step(**args);torch.testing.assert_close(l['logprob'],r['logprob'],rtol=0,atol=0)

    def test_explicit_migration_roundtrip_and_recurrent_ppo(self):
        parent=FullDepthPolicy(16);old={k:v.clone()for k,v in parent.state_dict().items()}
        config={'mode':'inspected_development','observation_protocol':QUERY_WINDOW,'migrate_observation_protocol':True}
        report=migrate_development(parent,config);self.assertTrue(report['old_scores_discarded'])
        model,migration=migrate_full_depth(parent)
        self.assertTrue(migration['optimizer_reset_required'])
        for k,v in old.items():self.assertTrue(torch.equal(v,model.state_dict()[k]))
        with self.assertRaisesRegex(ValueError,'PROTOCOL_MISMATCH'):
            Stream(environment(),model,calibration={'decision_latency_ms':2})
        e=query_environment();stream=Stream(e,model,calibration={'decision_latency_ms':2},record_training=True)
        ledger=GrowthLedger(e,development=True);chunk=collect_chunk(stream,8,reward_ledger=ledger)
        out,_=sequence_forward(model,chunk.frames,chunk.initial_hidden)
        torch.testing.assert_close(out['logprob'],torch.stack([f['old_logprob']for f in chunk.frames]),atol=2e-5,rtol=2e-5)
        opt=torch.optim.Adam(model.parameters(),lr=1e-5)
        result=update(model,opt,chunk,[f['economic_reward']for f in chunk.frames],role='train',epochs=1)
        self.assertTrue(result[0]['accepted'])
        with tempfile.TemporaryDirectory()as d:
            path=Path(d)/'query.pt';sha=save_checkpoint(path,model,opt,dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            loaded,bundle=load_checkpoint(path,sha);self.assertIs(type(loaded),QueryWindowPolicy)
            self.assertEqual(loaded.observation_protocol,QUERY_WINDOW)

    def test_population_trains_evolves_and_rechecks_new_architecture(self):
        class QueryExperiment(FixtureExperiment):
            def world(self,role,start,end,finalist=None,stress=False):
                e=query_environment();e.end=end;e.start=start;e.advance(start);return e
        with tempfile.TemporaryDirectory()as d:
            exp=QueryExperiment(Path(d));exp.config.update(trade_query_windows=True,full_depth_observations=True,
                observation_protocol=QUERY_WINDOW,members=2,fresh_founders=2)
            exp.search_config['islands']=1
            result=exp.run();self.assertGreater(result['total_accepted_updates'],0)
            replacements=[e for e in exp.events if e['event']=='population_replacement']
            self.assertEqual(len(replacements),1)
            for cp in exp.population:
                model,_=load_checkpoint(cp['path'],cp['sha256']);self.assertIs(type(model),QueryWindowPolicy)
            self.assertFalse(result['independent_test_used']);self.assertFalse(result['production_changed'])

    def test_behavior_probe_retains_query_mask_and_mutates_new_heads(self):
        e=query_environment();frame={**tensors(e.observe(),'cpu'),'at_ms':e.now,'reset':torch.tensor([True])}
        probe=freeze_probe([frame],role='train',dataset_sha256='a'*64,bounds=(1,100000))
        self.assertIn('trade_query',probe.frames[0]['masks'])
        model=QueryWindowPolicy(16);before=model.query_times.weight.detach().clone()
        report=mutate(model,probe,'local',111)
        self.assertFalse(torch.equal(before,model.query_times.weight));self.assertTrue(report['training_only_probe'])
        probe.frames[0]['masks']['trade_query'].logical_not_()
        with self.assertRaisesRegex(ValueError,'PROBE_CHANGED'):behavior(model,probe)
