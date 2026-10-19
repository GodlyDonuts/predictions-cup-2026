import unittest
import torch
from .policy import RecurrentPolicy
from .fused_policy import from_cell_policy,FusedRecurrentPolicy
from .batched_rollout import rollout_batch
from .train_inner import fingerprint
from .episodes import Episode
from .account import UNIT,Limits
from .clock import BudgetConfig
from .reference_env import Mechanics
from .liquidity import Book


class FusedSemantics(unittest.TestCase):
    def test_packed_transfer_preserves_exact_values_dtypes_and_shapes(self):
        from .packed_io import transfer
        tree={'x':torch.randn(2,3,24),'mask':torch.tensor([True,False]),'actions':{'id':torch.tensor([2**54,7],dtype=torch.int64)}}
        out=transfer(tree,'cpu')
        self.assertTrue(torch.equal(tree['x'],out['x']));self.assertTrue(torch.equal(tree['mask'],out['mask']))
        self.assertTrue(torch.equal(tree['actions']['id'],out['actions']['id']))
        self.assertEqual(out['actions']['id'].dtype,torch.int64)

    def test_fused_sequence_matches_cell_with_asynchronous_resets_and_no_future_leak(self):
        torch.manual_seed(15001);p=RecurrentPolicy(24,14,64);f=from_cell_policy(p)
        T,B,N=4,2,3;x=torch.randn(T,B,N,24);g=torch.randn(T,B,14);present=torch.ones(T,B,N,dtype=torch.bool)
        masks={'market':torch.ones(T,B,N,dtype=torch.bool),'family':torch.ones(T,B,N,5,dtype=torch.bool),
               'side':torch.ones(T,B,N,5,2,dtype=torch.bool),'price':torch.ones(T,B,N,5,2,5,dtype=torch.bool),
               'patience':torch.ones(T,B,N,5,2,3,dtype=torch.bool)}
        reset=torch.zeros(T,B,dtype=torch.bool);reset[0]=True;reset[2,0]=True
        old=p.sequence(x,g,present,masks,reset);actions={k:torch.stack([r['actions'][k]for r in old])for k in old[0]['actions']}
        new=f.sequence(x,g,present,masks,reset,actions)
        for a,b in zip(old,new):
            self.assertTrue(torch.allclose(a['logprob'],b['logprob'],atol=2e-5))
            self.assertTrue(torch.allclose(a['hidden'],b['hidden'],atol=2e-6))
        changed=x.clone();changed[-1]+=20;future=f.sequence(changed,g,present,masks,reset,actions)
        self.assertTrue(torch.equal(new[0]['value'],future[0]['value']))
        sum(r['value'].sum()+r['logprob'].sum()for r in new).backward()
        self.assertTrue(torch.isfinite(f.recurrent.weight_ih_l0.grad).all())


class StaticTape:
    def episode(self,mechanics,start,end,**kw):
        e=Episode(mechanics,0,kw['initial_cash_micros'],('__GLOBAL_WAIT__','x'),kw['target_wealth_micros'],start,end)
        e.add_book(Book('x',0,None,((400000,5000),),((600000,5000),),'b'*64))
        e.add_reference('x',500000,0,'c'*64,source_kind='Kalshi');return e


class BatchedWorlds(unittest.TestCase):
    def setUp(self):
        self.m=Mechanics(BudgetConfig(100,600,24,2500,'a'*64),Limits(1000,2000*UNIT,600*UNIT),
                    600,600,300,1000000,1000000,16,'Kalshi')
        self.p={'run_start_ms':0,'fit_end_ms':1000000,'episode_length_ms':2000,'maximum_receipt_drain_ms':1000,
                'initial_cash_micros':1000*UNIT,'planning_target_wealth_micros':11000*UNIT,'recurrent_burnin_ms':1000,
                'decision_interval_ms':1000,'_sha256':'d'*64,'initial_inventory_allocation_ppm':900000,'initial_inventory_seed':33}

    def test_worlds_have_independent_funded_accounts_and_one_behavior_policy(self):
        model=FusedRecurrentPolicy(hidden_size=64)
        t,r,profile=rollout_batch(StaticTape(),self.m,self.p,model,[1000,2000],baseline=True)
        self.assertEqual(t.contracts.shape[:2],(3,2));self.assertEqual(t.behavior_sha256,fingerprint(model))
        self.assertTrue(t.reset[0].all());self.assertTrue(t.done[-1].all())
        self.assertTrue(all(e['qualified_accounting']and e['reference_return']==0 and e['fill_receipts']==0 for e in r))
        self.assertEqual(profile['batch_worlds'],2)

    def test_failed_world_aborts_waiting_worlds(self):
        class Broken(StaticTape):
            def episode(self,mechanics,start,end,**kw):
                if start==2000:raise ValueError('BROKEN_WORLD')
                return super().episode(mechanics,start,end,**kw)
        with self.assertRaisesRegex(ValueError,'BROKEN_WORLD'):
            rollout_batch(Broken(),self.m,self.p,FusedRecurrentPolicy(hidden_size=64),[1000,2000])

    def test_spawned_process_worlds_preserve_the_same_financial_boundary(self):
        import tempfile,gzip,json,hashlib
        from pathlib import Path
        from .process_rollout import ProcessRollouts
        rows=[{'kind':'book','venue':'SIG','available_at_ms':0,'source_at_ms':0,'market_id':'201','exchange_id':'101',
               'original_sha256':'b'*64,'bids':[[400000,5000]],'asks':[[600000,5000]]},
              {'kind':'reference','venue':'Kalshi','available_at_ms':0,'received_at_ms':0,'market_id':'201',
               'mapping_kind':'direct_party_reference','price_yes_micros':500000,'original_sha256':'c'*64},
              {'kind':'gap','venue':'SIG','available_at_ms':10000,'market_id':None,'exchange_id':None}]
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);path=root/'observations.jsonl.gz'
            with gzip.open(path,'wt',encoding='utf-8')as f:
                for r in rows:f.write(json.dumps(r)+'\n')
            manifest=json.dumps({'observationsSha256':hashlib.sha256(path.read_bytes()).hexdigest()}).encode()
            h=hashlib.sha256(manifest).hexdigest();(root/'dataset-manifest.json').write_bytes(manifest)
            (root/'COMPLETE.json').write_text(json.dumps({'manifestSha256':h}),encoding='utf-8')
            pool=ProcessRollouts(root,h,4,actor_processes=2)
            try:
                model=FusedRecurrentPolicy(hidden_size=64)
                t,r,profile=pool.rollout(self.m,self.p,model,[1000]*4,baseline=True)
                self.assertEqual(t.behavior_sha256,fingerprint(model));self.assertEqual(t.contracts.shape[:2],(3,4))
                self.assertTrue(all(e['qualified_accounting']and e['reference_return']==0 for e in r))
                self.assertEqual(profile['cpu_actor_processes'],2)
            finally:pool.close()
