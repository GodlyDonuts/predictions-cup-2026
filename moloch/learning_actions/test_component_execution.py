"""Synthetic debt/FIFO integration, not topology or exchange calibration."""
from copy import deepcopy
import hashlib,json,tempfile,unittest
from pathlib import Path
import torch
from .test_order_pages import world as base_world
from .test_collateral_ledger import component
from .component_execution import ComponentEpisode,ComponentPolicy,configure,configure_from_evidence,migrate_component_debt
from .contract import OrderSpec,UNIT
from .economics import snapshot,GrowthLedger
from .episode_tape import financial_digest
from .observation_protocol import COMPONENT_DEBT
from .order_policy import OrderPagePolicy
from .training import save_checkpoint,load_checkpoint,collect_chunk,update
from .stream import Stream


def world(linked=False):
    e=base_world();e.__class__=ComponentEpisode;e.observation_protocol=COMPONENT_DEBT
    e.books['y']=deepcopy(e.books['x'])
    configure(e,[component()]if linked else(),scope='fixture-cup',other_draw_order=('a',)if linked else None)
    return e


def buy(e,ex,n=2,side='YES',price=450000):
    return e._execute('ORDER',[OrderSpec(ex,side,'BUY',n,price)])


def read(e,kind):
    if e.budget.active is not None:e.advance(e.busy_until)
    e.advance(e.budget.ready_at(e.now,False))
    assert e.request_observation('SIG',(kind,))['status']=='IN_FLIGHT'
    e.advance(e.busy_until)


class ComponentExecutionTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(714)

    def test_no_components_match_original_fifo_through_real_accepted_orders(self):
        a=base_world();a.books['y']=deepcopy(a.books['x']);b=world()
        for ex,side,action,n,price in [('x','YES','BUY',2,450000),('y','YES','BUY',2,450000),
            ('x','YES','SELL',1,400000),('x','NO','BUY',1,600000),('y','YES','SELL',2,400000)]:
            for e in(a,b):e._execute('ORDER',[OrderSpec(ex,side,action,n,price)])
            self.assertEqual((a.cash,a.lots,a.realized,a.fees,a.api_orders),(b.cash,b.lots,b.realized,b.fees,b.api_orders))
            self.assertEqual(snapshot(a),snapshot(b))
            self.assertTrue(all(r['collateral_debt_delta_micros']==0 for r in b.api_fills))

    def test_linked_fill_draws_principal_but_does_not_create_wealth(self):
        e=world(True);plain=world()
        for ex in('x','y'):
            buy(e,ex);buy(plain,ex)
        self.assertEqual(e.component_ledger.debt,{'a':900000})
        self.assertEqual(e.cash,plain.cash+900000)
        self.assertEqual(snapshot(e),snapshot(plain))
        row=e.api_fills[-1]
        self.assertEqual(row['cash_delta_micros'],0);self.assertEqual(row['collateral_debt_delta_micros'],900000)
        self.assertEqual(row['all_execution_economics']['collateralSavings'],900000)
        self.assertEqual(e.buying_power(),max(e.cash,0)+1100000)

    def test_paid_collateral_components_and_advance_do_not_reveal_cash_or_positions(self):
        e=world(True);buy(e,'x');buy(e,'y')
        before=(e.known_cash,deepcopy(e.known_lots),e.known_advance)
        self.assertFalse(e.known_components)
        e.request_observation('SIG',('collateral',));self.assertFalse(e.known_components)
        e.advance(e.busy_until)
        self.assertEqual((e.known_cash,e.known_lots),before[:2]);self.assertEqual(e.known_advance,1100000)
        self.assertEqual(e.known_components[0]['outstandingAdvance'],900000)
        self.assertEqual(e.known_components[0]['guaranteedPayoutFloor'],2*UNIT)
        o=e.observe();i=o['entities'].index(('collateral_component','a'))
        self.assertFalse(o['masks']['family'][i].any())
        read(e,'account');self.assertEqual(e.known_cash,e.cash);self.assertEqual(e.known_advance,1100000)

    def test_older_collateral_receipt_cannot_replace_a_newer_paid_baseline(self):
        from .account_reads import capture,publish
        e=world(True);buy(e,'x');old=capture(e,'collateral');buy(e,'y')
        publish(e,capture(e,'collateral'));publish(e,old)
        self.assertEqual(e.known_components[0]['outstandingAdvance'],900000)
        self.assertEqual(e.known_advance,1100000)

    def test_sale_repaying_floor_drop_preserves_original_fifo_profit(self):
        e=world(True);plain=world()
        for env in(e,plain):
            buy(env,'x');buy(env,'y');env._execute('ORDER',[OrderSpec('x','YES','SELL',2,400000)])
        self.assertEqual(e.component_ledger.debt,{'a':0})
        self.assertEqual((e.cash,e.lots,e.realized),(plain.cash,plain.lots,plain.realized))
        self.assertEqual(e.api_fills[-1]['all_execution_economics']['collateralRepayment'],900000)
        self.assertEqual(e.api_fills[-1]['collateral_debt_delta_micros'],-900000)

    def test_complementary_redemption_repaying_debt_matches_cash_only_economics(self):
        e=world(True);plain=world()
        for env in(e,plain):buy(env,'x');buy(env,'y');buy(env,'x',side='NO',price=600000)
        self.assertEqual((e.cash,e.lots,e.realized),(plain.cash,plain.lots,plain.realized))
        self.assertEqual(e.component_ledger.positions['x'],0)
        self.assertEqual(e.api_fills[-1]['all_execution_economics']['redemptionCredit'],2*UNIT)
        self.assertEqual(e.component_ledger.debt,{'a':0})

    def test_failed_atomic_basket_restores_debt_history_fifo_and_liquidity(self):
        e=world(True);buy(e,'x');buy(e,'y');before=financial_digest(e);events=deepcopy(e.component_ledger.events)
        with self.assertRaisesRegex(ValueError,'ALL_OR_NONE'):
            e._execute('ALL_OR_NONE',[OrderSpec('x','YES','BUY',1,450000),OrderSpec('y','YES','BUY',9999,5000)])
        self.assertEqual(financial_digest(e),before);self.assertEqual(e.component_ledger.events,events)
        self.assertEqual(e.component_ledger.cash,e.cash)

    def test_reward_and_event_opportunities_do_not_count_borrowing_as_profit(self):
        e=world(True);plain=world();ledgers=[GrowthLedger(env,development=True)for env in(e,plain)]
        for env in(e,plain):buy(env,'x');buy(env,'y');env.advance(env.end)
        results=[ledger.finish()for ledger in ledgers]
        self.assertEqual(results[0]['reward'],results[1]['reward'])
        self.assertEqual(results[0]['marked_roi'],results[1]['marked_roi'])
        self.assertEqual(ledgers[0]._opportunities(snapshot(e)),ledgers[1]._opportunities(snapshot(plain)))

    def test_negative_cash_is_not_spendable_against_component_headroom(self):
        e=world();e.cash=-UNIT;e.lots={('x','YES'):[[2,450000]],('y','YES'):[[2,450000]]}
        configure(e,[component()],scope='fixture-cup',other_draw_order=('a',))
        self.assertEqual(e.buying_power(),2*UNIT)
        with self.assertRaisesRegex(ValueError,'LENDING_PRIORITY'):
            configure(e,[component()],scope='fixture-cup')

    def test_causal_pinned_topology_and_reconfiguration_constraints(self):
        e=world()
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'components.json';value={'available_at_ms':e.now+1,'scope':'fixture-cup','components':[], 'other_draw_order':[]}
            p.write_text(json.dumps(value));sha=hashlib.sha256(p.read_bytes()).hexdigest()
            with self.assertRaisesRegex(ValueError,'NOT_YET_AVAILABLE'):configure_from_evidence(e,{'path':str(p),'sha256':sha})
            with self.assertRaisesRegex(ValueError,'EVIDENCE_CHANGED'):configure_from_evidence(e,{'path':str(p),'sha256':'0'*64})
        buy(e,'x')
        with self.assertRaisesRegex(ValueError,'FRESH_PAPER_ORDERS'):configure(e)

    def test_explicit_migration_ppo_and_checkpoint_contract(self):
        model,report=migrate_component_debt(OrderPagePolicy(16));self.assertTrue(report['all_weights_preserved_exactly'])
        e=world(True);s=Stream(e,model,record_training=True,calibration={'decision_latency_ms':2},behavior_guard='tensor_versions_v1')
        c=collect_chunk(s,8,reward_ledger=GrowthLedger(e,development=True));opt=torch.optim.Adam(model.parameters(),lr=1e-5)
        self.assertTrue(update(model,opt,c,[f['economic_reward']for f in c.frames],role='train',epochs=1)[0]['accepted'])
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'model.pt';sha=save_checkpoint(p,model,opt,dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            loaded,_=load_checkpoint(p,sha);self.assertIs(type(loaded),ComponentPolicy)

    def test_http_ack_exposes_only_aggregate_own_component_economics(self):
        e=world(True);e.dispatch('ORDER',[OrderSpec('x','YES','BUY',2,450000)],idempotency_key='x');e.advance(e.busy_until)
        e.dispatch('ORDER',[OrderSpec('y','YES','BUY',2,450000)],idempotency_key='y');e.advance(e.busy_until)
        row=e.known_acknowledgments[0]['all']
        self.assertEqual(row['collateralSavings'],900000);self.assertNotIn('componentDraws',row)
        self.assertEqual(row['relationshipIds'],('fixture-all-a',))
        self.assertTrue(any(kind=='all_execution'for kind,key in e.observe()['entities']))

    def test_zero_power_cannot_bootstrap_a_buy_from_its_hypothetical_new_floor(self):
        e=world(True);e.cash=0;configure(e,[component()],scope='fixture-cup',other_draw_order=('a',))
        before=financial_digest(e)
        with self.assertRaisesRegex(ValueError,'INSUFFICIENT_BUYING_POWER'):buy(e,'y')
        self.assertEqual(financial_digest(e),before);self.assertFalse(e.component_ledger.events)

    def test_sealed_episode_replays_existing_debt_and_preserves_on_policy_updates(self):
        from .batched import BatchStream
        from .episode_tape import EpisodeWriter,EpisodeReader
        from .episode_training import optimize
        def make(*args):
            e=world(True);e.end=2401;buy(e,'x');buy(e,'y');read(e,'collateral');return e
        model=ComponentPolicy(16);e=make()
        stream=BatchStream([e],model,[GrowthLedger(e,development=True)],
            calibration={'decision_latency_ms':10},behavior_guard='tensor_versions_v1')
        with tempfile.TemporaryDirectory()as d:
            writer=EpisodeWriter(Path(d)/'diary',stream,dataset_sha256='a'*64,specification_sha256='b'*64)
            while e.now<e.end:writer.record(stream.collect(8))
            sha=writer.seal();reader=EpisodeReader(writer.path,sha)
            try:
                reports=optimize(reader,model,torch.optim.Adam(model.parameters(),lr=1e-5),make,
                    lambda env:GrowthLedger(env,development=True))
                self.assertTrue(any(r['accepted']for r in reports))
            finally:reader.close()
