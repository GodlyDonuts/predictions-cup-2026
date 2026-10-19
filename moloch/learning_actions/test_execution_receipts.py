import unittest
import numpy as np
import torch
from .test_actions import environment
from .test_passive import print_row
from .contract import OrderSpec,UNIT
from .observation_protocol import SCOPED_EXECUTION
from .execution_receipts import orders,publish_ack,publish_private
from .account_reads import capture,publish
from .realtime import DerivedStream
from .passive import StrictPriorityFlow
from .full_depth import FullDepthEpisode
from .query_policy import QueryWindowPolicy,migrate_full_depth
from .full_depth import FullDepthPolicy
from .observation_protocol import migrate_development
from .stream import Stream
from .training import collect_chunk,update
from .economics import GrowthLedger
from .test_population_experiment import FixtureExperiment
from pathlib import Path
import tempfile


def scoped():
    e=environment();e.observation_protocol=SCOPED_EXECUTION;return e


class ExecutionReceiptTests(unittest.TestCase):
    def test_ack_only_reveals_target_orders_and_documented_execution_results(self):
        e=scoped();cash=e.known_cash
        e._execute('ORDER',[OrderSpec('x','YES','BUY',2,450000)])
        e._execute('ORDER',[OrderSpec('x','YES','BUY',3,200000)])
        e.dispatch('ORDER',[OrderSpec('x','YES','BUY',4,250000)],idempotency_key='one')
        e.advance(e.busy_until)
        self.assertEqual(set(e.known_orders),{3});self.assertEqual(e.known_cash,cash);self.assertFalse(e.known_lots)
        self.assertEqual(e.finance_observation_count,0)
        row=e.known_acknowledgments[0];self.assertEqual(row['order'],3);self.assertTrue(row['open'])
        self.assertEqual(row['at_ms'],e.now);self.assertNotIn('cash',row);self.assertNotIn('revision',row)
        obs=e.observe();i=obs['entities'].index(('execution_ack',0));self.assertFalse(obs['masks']['market'][i])
        self.assertEqual(obs['contracts'][i,7],0)
        e.request_observation('SIG',('positions',));e.advance(e.busy_until)
        self.assertTrue(e.known_lots);self.assertEqual(e.known_cash,cash)
        e.request_observation('SIG',('account',));e.advance(e.busy_until);self.assertEqual(e.known_cash,e.cash)

    def test_rejected_write_does_not_reconcile_hidden_finance(self):
        e=scoped();e._execute('ORDER',[OrderSpec('x','YES','BUY',2,450000)])
        e.dispatch('ORDER',[OrderSpec('missing','YES','BUY',1,200000)],idempotency_key='bad')
        e.advance(e.busy_until)
        self.assertEqual(e.idempotency['bad']['status'],'REJECTED');self.assertFalse(e.known_lots)
        self.assertEqual(e.known_cash,1000*UNIT);self.assertFalse(e.known_acknowledgments)

    def test_target_revision_tombstones_merge_with_full_baselines_without_regression(self):
        e=scoped();e._execute('ORDER',[OrderSpec('x','YES','BUY',2,200000)])
        e._execute('ORDER',[OrderSpec('x','YES','BUY',3,250000)])
        a,b=e.api_orders[1],e.api_orders[2]
        orders(e,{1:a},5);publish(e,{'revision':3,'fields':{'orders':{2:b}}})
        self.assertEqual(set(e.known_orders),{1,2})
        orders(e,{1:None},7);publish(e,{'revision':6,'fields':{'orders':{1:a,2:b}}})
        self.assertEqual(set(e.known_orders),{2})
        publish_ack(e,{'revision':4,'orders':{1:a},'acknowledgments':[]});self.assertNotIn(1,e.known_orders)
        publish(e,{'revision':9,'fields':{'orders':{}}});orders(e,{2:b},8)
        self.assertFalse(e.known_orders)

    def connected(self):
        e=scoped();e.passive=StrictPriorityFlow(e,'cup');e.realtime=DerivedStream(e)
        e.keys['SIG'].append(('realtime_token',));e.request_observation('SIG',('realtime_token',));e.advance(e.busy_until)
        e.dispatch('ORDER',[OrderSpec('x','YES','BUY',10,425000)],idempotency_key='rest');e.advance(e.busy_until)
        return e

    def test_private_fill_batch_changes_only_affected_order_and_exposes_raw_atoms(self):
        e=self.connected();order=next(iter(e.known_orders));self.assertEqual(e.known_orders[order].remaining,10)
        e.clock.schedule(1000,10,'api_counterparty_trade',print_row(quantity=2))
        e.clock.schedule(1200,10,'api_counterparty_trade',print_row(at=1200,quantity=2,identity='two'))
        e.advance(2099);self.assertEqual(e.known_orders[order].remaining,10)
        e.advance(2100);self.assertEqual(e.known_orders[order].remaining,6)
        self.assertEqual(e.known_cash,1000*UNIT);self.assertFalse(e.known_lots)
        self.assertEqual([r['quantity']for r in e.known_private_fills],[2,2])
        self.assertEqual(e.known_private_fills[0]['yes_price_micros'],425000)
        obs=e.observe();i=obs['entities'].index(('private_fill',0));self.assertEqual(obs['contracts'][i,24],9)
        self.assertFalse(obs['masks']['market'][i]);self.assertEqual(len(e.budget.reads),0)

    def test_disconnected_private_batch_is_dropped_and_expiry_is_not_free_observation(self):
        e=self.connected();order=next(iter(e.known_orders))
        e.clock.schedule(1000,10,'api_counterparty_trade',print_row(quantity=2));e.advance(1100)
        e.realtime.disconnect();e.advance(3000)
        self.assertEqual(e.known_orders[order].remaining,10);self.assertFalse(e.known_private_fills)
        e.clock.schedule(3100,20,'api_expiry',order);e.advance(3200)
        self.assertNotIn(order,e.api_orders);self.assertIn(order,e.known_orders)
        e.request_observation('SIG',('owned_orders',));e.advance(e.busy_until);self.assertFalse(e.known_orders)

    def test_execution_accounting_matches_legacy_with_identical_forced_commands(self):
        left,right=environment(),scoped()
        for j,(operation,payload)in enumerate([
            ('ORDER',[OrderSpec('x','YES','BUY',5,450000)]),
            ('ORDER',[OrderSpec('x','YES','SELL',7,400000)]),
            ('ORDER',[OrderSpec('x','YES','BUY',10,200000)]),('CANCEL_ALL',None)]):
            for e in(left,right):e.dispatch(operation,payload,idempotency_key=str(j));e.advance(e.busy_until)
            self.assertEqual((left.cash,left.lots,left.api_fills,left.realized,left.fees,left.api_orders,left.now),
                             (right.cash,right.lots,right.api_fills,right.realized,right.fees,right.api_orders,right.now))

    def test_scoped_profile_collects_recurrent_on_policy_frames_and_updates(self):
        torch.set_num_threads(1);torch.manual_seed(9913)
        e=scoped();e.__class__=FullDepthEpisode;model=QueryWindowPolicy(16);model.observation_protocol=SCOPED_EXECUTION
        stream=Stream(e,model,calibration={'decision_latency_ms':2},record_training=True)
        ledger=GrowthLedger(e,development=True);chunk=collect_chunk(stream,16,reward_ledger=ledger)
        opt=torch.optim.Adam(model.parameters(),lr=1e-5)
        report=update(model,opt,chunk,[f['economic_reward']for f in chunk.frames],role='train',epochs=1)
        self.assertTrue(report[0]['accepted'])

    def test_private_fill_before_late_ack_requires_reconciliation_instead_of_resurrection(self):
        e=scoped();e._execute('ORDER',[OrderSpec('x','YES','BUY',10,200000)]);o=e.api_orders[1]
        publish_private(e,{'fills':[{'order':1,'exchange':'x','quantity':2,'yes_price_micros':200000,
                                   'executed_at_ms':1,'revision':5}], 'orders':[]})
        publish_ack(e,{'revision':3,'orders':{1:o},'acknowledgments':[]});self.assertFalse(e.known_orders)
        o.remaining=8;publish(e,{'revision':5,'fields':{'orders':{1:o}}})
        self.assertEqual(e.known_orders[1].remaining,8)

    def test_hidden_finance_revisions_never_enter_ack_observations(self):
        observed=[]
        for revision in(3,900000):
            e=scoped();e.finance_revision=revision
            e.dispatch('ORDER',[OrderSpec('x','YES','BUY',3,200000)],idempotency_key='r');e.advance(e.busy_until)
            observed.append(e.observe())
        self.assertEqual(observed[0]['entities'],observed[1]['entities'])
        for k in('contracts','global_state','present'):np.testing.assert_array_equal(observed[0][k],observed[1][k])

    def test_query_architecture_extension_preserves_explicit_scoped_protocol_migration(self):
        parent=FullDepthPolicy(16)
        migrate_development(parent,{'mode':'inspected_development','migrate_observation_protocol':True,
                                    'observation_protocol':SCOPED_EXECUTION})
        child,_=migrate_full_depth(parent);self.assertEqual(child.observation_protocol,SCOPED_EXECUTION)
        e=scoped();e.__class__=FullDepthEpisode
        Stream(e,child,calibration={'decision_latency_ms':2})

    def test_scoped_population_completes_training_selection_and_evolution(self):
        class ScopedExperiment(FixtureExperiment):
            def world(self,role,start,end,finalist=None,stress=False):
                e=scoped();e.__class__=FullDepthEpisode;e.end=end;e.start=start;e.advance(start);return e
        torch.set_num_threads(1)
        with tempfile.TemporaryDirectory()as folder:
            exp=ScopedExperiment(Path(folder));exp.config.update(trade_query_windows=True,
                full_depth_observations=True,scoped_execution_receipts=True,
                observation_protocol=SCOPED_EXECUTION,members=2,fresh_founders=2)
            exp.search_config['islands']=1;result=exp.run()
            self.assertGreater(result['total_accepted_updates'],0)
            self.assertEqual(len([r for r in exp.events if r['event']=='population_replacement']),1)
            self.assertFalse(result['independent_test_used']);self.assertFalse(result['production_changed'])
