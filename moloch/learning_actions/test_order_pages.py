"""Owned-order API/replay fixtures, not historical trading performance."""
import tempfile,unittest
from pathlib import Path
import torch
from .test_actions import environment
from .contract import OrderSpec,UNIT
from .order_pages import OrderPageEpisode,enable
from .order_history import ARCHIVE_MS
from .observation_protocol import ORDER_PAGES
from .engine import Resting
from .order_policy import OrderPagePolicy,migrate_owned_orders
from .joint_memory_policy import JointMemoryPolicy
from .stream import Stream
from .training import collect_chunk,update,save_checkpoint,load_checkpoint,sequence_forward
from .economics import GrowthLedger


def world():
    e=environment();e.__class__=OrderPageEpisode;e.observation_protocol=ORDER_PAGES;enable(e)
    # A second explicitly eligible, noncrossing fixture exchange tests filters.
    e.exchanges=[*e.exchanges,'y'];e.mapping['y']='fixture-y-market';e.scope['fixture-y-market']={'y'}
    e.observe();return e


def read(e,key=('owned_orders',),**kwargs):
    r=e.request_observation('SIG',key,**kwargs)
    if r['status']=='RATE_WAIT':e.advance(r['ready_ms']);r=e.request_observation('SIG',key,**kwargs)
    if r['status']!='IN_FLIGHT':raise ValueError(r)
    e.advance(e.busy_until);return e.known_order_pages


class OrderPageTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(881)

    def place(self,e,n=1,exchange='x',price=5000,expiration=None):
        for _ in range(n):e._execute('ORDER',[OrderSpec(exchange,'YES','BUY',1,price,expiration)])

    def test_paid_default_max_pages_and_no_cursor_before_receipt_or_partial_absence_clear(self):
        e=world();self.place(e,401);cash=e.cash
        e.known_orders[9999]=Resting(9999,OrderSpec('x','YES','BUY',1,5000),1,e.now)
        e.known_order_revisions[9999]=0
        self.assertEqual(e.request_observation('SIG',('owned_orders',),order_limit=200)['status'],'IN_FLIGHT')
        self.assertFalse(e.order_page_issued_cursors);self.assertEqual(len(e.known_orders),1)
        e.advance(e.busy_until);p=e.known_order_pages[('open',None,None)]
        self.assertEqual(len(p['data']),200);self.assertIn(9999,e.known_orders)
        for expected in(200,1):
            token=p['pagination']['nextCursor'];self.assertIn(('owned_orders','cursor',token),e.keys['SIG'])
            read(e,('owned_orders','cursor',token),order_limit=200);p=e.known_order_pages[('open',None,None)]
            self.assertEqual(len(p['data']),expected)
        self.assertFalse(p['pagination']['hasMore']);self.assertNotIn(9999,e.known_orders)
        self.assertEqual(len(e.known_orders),401);self.assertEqual(len(e.budget.reads),3)
        self.assertEqual(e.cash,cash);self.assertEqual(e.known_resource_revisions['orders'],-1)
        self.assertEqual(len(e.order_page_scans),2)

    def test_old_cursor_can_branch_after_completed_scan_without_losing_prefix_proof(self):
        e=world();self.place(e,5);read(e,order_limit=2)
        first=e.known_order_pages[('open',None,None)]['pagination']['nextCursor']
        read(e,('owned_orders','cursor',first),order_limit=1)
        second=e.known_order_pages[('open',None,None)]['pagination']['nextCursor']
        read(e,('owned_orders','cursor',second),order_limit=200)
        self.assertEqual(len(e.known_orders),5)
        # An earlier cursor remains independently usable after another branch
        # completed. A missing old local order can now be pruned by that proof.
        e.known_orders[9999]=Resting(9999,OrderSpec('x','YES','BUY',1,5000),1,0)
        e.known_order_revisions[9999]=0
        self.place(e);new=e.api_orders[6]
        e.known_orders[6]=new;e.known_order_revisions[6]=e.finance_revision
        read(e,('owned_orders','cursor',first),order_limit=200)
        self.assertNotIn(9999,e.known_orders);self.assertIn(6,e.known_orders)
        self.assertEqual(len(e.known_orders),6)
        self.assertEqual(e.known_resource_revisions['orders'],-1)
        self.assertIn(first,e.order_page_scans);self.assertIn(second,e.order_page_scans)

    def test_default_limit_scope_filter_and_closed_scan_do_not_clear_other_open_orders(self):
        e=world();self.place(e,60);self.place(e,1,'y');e.observe()
        read(e);self.assertEqual(len(e.known_order_pages[('open',None,None)]['data']),50)
        outside=e.api_orders[61];e.known_orders[61]=outside;e.known_order_revisions[61]=0
        read(e,('owned_orders','exchange','x'),order_limit=200)
        self.assertEqual(len(e.known_order_pages[('open','x',None)]['data']),60);self.assertIn(61,e.known_orders)
        read(e,order_status='closed');self.assertTrue(e.known_orders)
        market=e.mapping['x'];read(e,('owned_orders','market',market),order_limit=200)
        self.assertTrue(all(r.order.spec.exchange=='x'for r in e.known_order_pages[('open',None,market)]['data']))
        read(e,order_exchange='x',order_market='fixture-y-market')
        self.assertFalse(e.known_order_pages[('open','x','fixture-y-market')]['data'])

    def test_optional_actor_filter_latents_cover_combined_market_and_exchange(self):
        e=world();self.place(e);o=e.observe();i=o['entities'].index(('read',('SIG',('owned_orders',))))
        a=dict(family=10,market=i,price=0,side=0,patience=0,target_fraction=.5,child_fraction=.2,
            query_filter=1,query_from_fraction=.99,query_to_fraction=.01)
        e.act_raw(a,o);e.advance(e.busy_until)
        self.assertEqual(len(e.known_order_pages[('open','x',e.mapping['x'])]['data']),1)

    def test_invalid_cursor_scope_and_limit_are_charged_without_clearing_state(self):
        e=world();self.place(e,3);read(e,order_limit=1);token=e.known_order_pages[('open',None,None)]['pagination']['nextCursor']
        before=len(e.known_orders);count=len(e.budget.reads);cash=e.cash
        read(e,('owned_orders','exchange','y'),order_cursor=token)
        self.assertIn('error',e.known_order_pages[('open','y',None)]);self.assertEqual(len(e.known_orders),before)
        read(e,order_limit=201);self.assertEqual(len(e.budget.reads),count+2);self.assertEqual(e.cash,cash)
        self.assertTrue(any(kind=='order_page'for kind,key in e.observe()['entities']))

    def test_live_cursor_reads_later_fills_not_frozen_rows_and_keeps_all_issued_cursors_selectable(self):
        e=world();self.place(e,4);read(e,order_limit=1);p=e.known_order_pages[('open',None,None)];old=p['pagination']['nextCursor']
        e._execute('CANCEL',3);self.place(e,1)
        read(e,('owned_orders','cursor',old),order_limit=10)
        page=e.known_order_pages[('open',None,None)]
        self.assertEqual([r.order.id for r in page['data']],[2,1])
        self.assertIn(('owned_orders','cursor',old),e.keys['SIG']);self.assertTrue(e.order_history.records[5].open)

    def test_cancelled_order_is_not_relabelled_expired_by_old_scheduled_event(self):
        e=world();e.dispatch('ORDER',[OrderSpec('x','YES','BUY',1,5000,2000)],idempotency_key='expire-later');e.advance(301)
        e.dispatch('CANCEL',1,idempotency_key='cancel-first');e.advance(3000)
        r=e.order_history.records[1];self.assertFalse(r.open);self.assertIsNone(r.terminal_reason)
        read(e,order_status='expired');self.assertFalse(e.known_order_pages[('expired',None,None)]['data'])

    def test_expired_filled_and_archived_unfilled_orders_report_separate_states(self):
        e=world();e.dispatch('ORDER',[OrderSpec('x','YES','BUY',1,5000,1000)],idempotency_key='expires');e.advance(1200)
        e._execute('ORDER',[OrderSpec('x','YES','BUY',5,450000)])
        read(e,order_status='expired');self.assertEqual(e.known_order_pages[('expired',None,None)]['data'][0].terminal_reason,'Expired')
        read(e,order_status='closed');rows=e.known_order_pages[('closed',None,None)]['data']
        self.assertEqual({r.terminal_reason for r in rows},{'Expired','Filled'})
        e.end=ARCHIVE_MS+10000;e.advance(ARCHIVE_MS+2000)
        read(e,order_status='all');p=e.known_order_pages[('all',None,None)]
        self.assertFalse(p['coverage']['complete']);self.assertEqual([r.order.id for r in p['data']],[2])
        self.assertEqual(p['data'][0].filled,5);self.assertIsNone(p['coverage']['projectedThroughSequence'])

    def test_atomic_basket_failure_restores_order_history_ids_cash_and_liquidity(self):
        e=world();before=(e.cash,e.api_id,e.order_history.digest)
        with self.assertRaisesRegex(ValueError,'ALL_OR_NONE'):
            e._execute('ALL_OR_NONE',[OrderSpec('x','YES','BUY',3,450000),OrderSpec('x','YES','BUY',9999,5000)])
        self.assertEqual((e.cash,e.api_id,e.order_history.digest),before)
        self.assertFalse(e.api_fills);self.assertFalse(e.order_history.records);self.assertFalse(e.api_orders)

    def test_old_read_page_cannot_regress_a_newer_private_order_or_reveal_cash_positions(self):
        from copy import deepcopy
        from .execution_receipts import orders
        e=world();e._execute('ORDER',[OrderSpec('x','YES','BUY',5,5000)]);cash=e.cash
        e.request_observation('SIG',('owned_orders',));receipt=e.busy_until;e.advance(50)
        e._apply_fill(e.api_orders[1],5000,2)
        orders(e,{1:deepcopy(e.api_orders[1])},e.finance_revision)
        e.advance(receipt)
        self.assertEqual(e.known_orders[1].remaining,3);self.assertEqual(e.known_cash,cash);self.assertFalse(e.known_lots)
        self.assertNotEqual(e.cash,cash)

    def test_actor_controls_all_statuses_limit_and_scope_and_rows_are_read_only(self):
        e=world();self.place(e);o=e.observe();i=o['entities'].index(('read',('SIG',('owned_orders',))))
        for price,status in enumerate(('open','closed','expired','all')):
            action=dict(family=10,market=i,price=price,side=0,patience=0,target_fraction=1.,child_fraction=.2,
                query_filter=0,query_from_fraction=.1,query_to_fraction=.9)
            e.act_raw(action,o);e.advance(e.busy_until);o=e.observe();i=o['entities'].index(('read',('SIG',('owned_orders',))))
            page=e.known_order_pages[(status,None,None)];self.assertEqual(page['pagination']['limit'],200)
        for n,(kind,key)in enumerate(o['entities']):
            if kind in('order_page','order_page_row'):self.assertFalse(o['masks']['family'][n].any())

    def test_policy_migration_teacher_probability_ppo_and_checkpoint_identity(self):
        parent=JointMemoryPolicy(16);model,report=migrate_owned_orders(parent)
        self.assertTrue(report['all_weights_preserved_exactly'])
        e=world();stream=Stream(e,model,record_training=True,calibration={'decision_latency_ms':2},behavior_guard='tensor_versions_v1')
        c=collect_chunk(stream,8,reward_ledger=GrowthLedger(e,development=True))
        out,_=sequence_forward(model,c.frames,c.initial_hidden)
        torch.testing.assert_close(out['logprob'],torch.stack([f['old_logprob']for f in c.frames]),rtol=2e-5,atol=2e-5)
        opt=torch.optim.Adam(model.parameters(),lr=1e-5)
        self.assertTrue(update(model,opt,c,[f['economic_reward']for f in c.frames],role='train',epochs=1)[0]['accepted'])
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'pages.pt';sha=save_checkpoint(p,model,opt,dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            restored,_=load_checkpoint(p,sha);self.assertIs(type(restored),OrderPagePolicy)

    def test_adding_history_cannot_adopt_an_existing_order_account(self):
        e=environment();self.place(e)
        with self.assertRaisesRegex(ValueError,'NEW_EMPTY_PAPER'):enable(e)

    def test_population_and_sealed_episode_replay_include_order_read_history(self):
        from .test_population_experiment import FixtureExperiment
        from .episode_tape import EpisodeWriter,EpisodeReader
        from .episode_training import optimize
        from .batched import BatchStream
        class OrderExperiment(FixtureExperiment):
            def world(self,role,start,end,finalist=None,stress=False):
                e=world();e.end=end;e.start=start;e.advance(start);return e
        with tempfile.TemporaryDirectory()as d:
            exp=OrderExperiment(Path(d));exp.config.update(owned_order_pages=True,joint_local_memory_density=True,
                scoped_execution_receipts=True,trade_query_windows=True,full_depth_observations=True,
                observation_protocol=ORDER_PAGES,members=2,fresh_founders=2)
            exp.search_config['islands']=1;r=exp.run();self.assertGreater(r['total_accepted_updates'],0)
            for cp in exp.population:
                model,_=load_checkpoint(cp['path'],cp['sha256']);self.assertIs(type(model),OrderPagePolicy)
            self.assertFalse(r['independent_test_used'])
        class ReadFixture(OrderPageEpisode):
            def observe(self):
                o=super().observe();i=o['entities'].index(('read',('SIG',('owned_orders',))))
                # Fixture-only forcing guarantees the diary exercises paid
                # list responses; real profiles keep every legal action.
                o['masks']['family'][:]=False;o['masks']['family'][i,10]=True
                o['masks']['market'][:]=False;o['masks']['market'][i]=True;return o
        def make(a=1,b=401):
            e=world();e.__class__=ReadFixture;e.end=b
            self.place(e,3);return e
        with tempfile.TemporaryDirectory()as d:
            envs=[make(),make(b=451)];model=OrderPagePolicy(16)
            s=BatchStream(envs,model,[GrowthLedger(e,development=True)for e in envs],
                calibration={'decision_latency_ms':5},behavior_guard='tensor_versions_v1')
            w=EpisodeWriter(Path(d)/'diary',s,dataset_sha256='a'*64,specification_sha256='b'*64)
            while any(e.now<e.end for e in envs):w.record(s.collect(8))
            sha=w.seal();reader=EpisodeReader(w.path,sha)
            try:
                self.assertTrue(all(e.known_order_pages for e in envs))
                reports=optimize(reader,model,torch.optim.Adam(model.parameters(),lr=1e-5),make,
                    lambda e:GrowthLedger(e,development=True))
                self.assertTrue(any(r['accepted']for r in reports))
            finally:reader.close()
