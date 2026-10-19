"""Paid quote replay mechanics; synthetic fixtures are not exchange calibration."""
from copy import deepcopy
import tempfile,unittest
from pathlib import Path
import torch
from .test_component_execution import world as component_world
from .bulk_quotes import BulkQuoteEpisode,BulkQuotePolicy,initialize,publish,snapshot,migrate_bulk_quotes
from .component_execution import ComponentPolicy
from .observation_protocol import BULK_QUOTES
from .contract import OrderSpec,UNIT
from .training import save_checkpoint,load_checkpoint


def world():
    e=component_world();e.__class__=BulkQuoteEpisode;e.observation_protocol=BULK_QUOTES
    initialize(e);return e


def read(e,ids,key=('bulk_prices',)):
    e.advance(e.budget.ready_at(e.now,False))
    result=e.request_observation('SIG',key,quote_ids=ids)
    if result['status']!='IN_FLIGHT':raise AssertionError(result)
    e.advance(e.busy_until);return e.last_quote_response


class BulkQuoteTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(812)

    def test_one_hundred_quotes_cost_one_read_and_publish_only_after_receipt(self):
        e=world();before=(e.known_cash,deepcopy(e.known_lots),e.known_advance,deepcopy(e.actor_books))
        self.assertEqual(e.request_observation('SIG',('bulk_prices',),quote_ids=['x']*100)['status'],'IN_FLIGHT')
        self.assertFalse(e.known_quotes);self.assertIsNone(e.last_quote_response)
        self.assertEqual(e.clock.next_observable(),e.busy_until)
        self.assertEqual(len(e.budget.reads),1)
        self.assertEqual(e.request_observation('SIG',('quote','y'))['status'],'BUSY')
        e.advance(e.busy_until)
        self.assertEqual(len(e.last_quote_response['data']),100)
        self.assertEqual(e.last_quote_response['http_status'],200)
        self.assertEqual((e.known_cash,e.known_lots,e.known_advance,e.actor_books),before)
        q=e.known_quotes['x'];self.assertIsNone(q['latestPrice']);self.assertIsNone(q['option'])
        self.assertFalse(q['_option_source_available'])
        obs=e.observe();i=obs['entities'].index(('contract','x'))
        self.assertEqual(obs['contracts'][i,32],0)  # No invented last trade.
        self.assertAlmostEqual(float(obs['contracts'][i,25]),q['bestBid']/UNIT,places=6)

    def test_invalid_packet_is_paid_and_missing_ids_preserve_requested_order(self):
        e=world();r=read(e,['x']*101)
        self.assertEqual(r['http_status'],400);self.assertEqual(len(e.budget.reads),1)
        r=read(e,['y','999999','x','999998','y'])
        self.assertEqual([q['exchangeId']for q in r['data']],['y','x','y'])
        self.assertEqual(r['missingIds'],['999999','999998'])
        self.assertEqual(r['requestedIds'],('y','999999','x','999998','y'))
        r=read(e,['bad-id']);self.assertEqual(r['http_status'],400)
        r=read(e,['y'],key=('quote','x'))
        self.assertEqual([q['exchangeId']for q in r['data']],['x'])

    def test_missing_historical_depth_is_explicit_unknown_and_does_not_erase_previous_quote(self):
        e=world();read(e,['x']);old=deepcopy(e.known_quotes)
        e.books['x'].invalidated=True
        r=read(e,['x']);self.assertEqual(r['http_status'],503)
        self.assertEqual(r['error'],'REPLAY_QUOTE_SOURCE_UNAVAILABLE')
        self.assertEqual(r['data'],[]);self.assertEqual(r['missingIds'],[])
        self.assertEqual(e.known_quotes,old)

    def test_older_receipts_cannot_replace_newer_quotes_or_clear_them(self):
        e=world();read(e,['x']);old=deepcopy(e.last_quote_response)
        new=deepcopy(old);new['_request_sequence']+=1;new['data'][0]['bestBid']=123456
        publish(e,new);publish(e,old)
        self.assertEqual(e.known_quotes['x']['bestBid'],123456)
        old['data']=[];old['missingIds']=['x'];publish(e,old)
        self.assertIn('x',e.known_quotes)
        new['data']=[];new['missingIds']=['x'];new['_request_sequence']+=1;publish(e,new)
        self.assertNotIn('x',e.known_quotes)

    def test_custom_ordered_duplicate_selection_and_clear_are_local_not_paid_reads(self):
        e=world()
        def act(entity,family):
            o=e.observe();i=o['entities'].index(entity)
            a={'market':i,'family':family,'target_fraction':.2,'child_fraction':.7,'price':0,'side':0,'patience':0}
            return e.act_raw(a,o)
        for ex in('y','x','y'):act(('quote_add',ex),5)
        self.assertEqual(e.quote_selection,['y','x','y']);self.assertFalse(e.budget.reads)
        act(('quote_remove',1),6);self.assertEqual(e.quote_selection,['y','y'])
        r=read(e,None);self.assertEqual(r['requestedIds'],('y','y'))
        act(('quote_clear',None),6);self.assertFalse(e.quote_selection)
        r=read(e,None);self.assertEqual(r['http_status'],400)

    def test_snapshot_counts_resting_quotes_and_depletion_without_changing_depth(self):
        e=world();before=deepcopy(e.books['x']);q=snapshot(e,'x')
        e._execute('ORDER',[OrderSpec('x','NO','BUY',1,5000)])
        q2=snapshot(e,'x');self.assertEqual(q2['bestAsk'],995000 if q['bestAsk']is None else min(q['bestAsk'],995000))
        self.assertEqual(vars(e.books['x']),vars(before))
        e._execute('ORDER',[OrderSpec('x','YES','BUY',1,450000)])
        self.assertEqual(snapshot(e,'x')['latestPrice'],450000)
        self.assertFalse(e.actor_books)

    def test_latest_print_is_causal_and_does_not_issue_reporting_cursor(self):
        from .trade_history import TradeHistory
        h=TradeHistory()
        def row(at,price,match):
            return dict(tournament_id='cup',exchange_id='x',market_id='m',match_id=match,
                executed_at_ms=at-1,received_at_ms=at,available_at_ms=at,quantity=1,yes_price_micros=price)
        h.accept(10,row(10,100000,'a'));h.accept(20,row(20,200000,'b'))
        before=(h.sequence,deepcopy(h.cursors),deepcopy(h.rows))
        self.assertIsNone(h.latest('x',9));self.assertEqual(h.latest('x',10)['yes_price_micros'],100000)
        q=h.latest('x',20);q['yes_price_micros']=1
        self.assertEqual(h.latest('x',20)['yes_price_micros'],200000)
        self.assertEqual((h.sequence,h.cursors,h.rows),before)

    def test_policy_read_action_dispatches_bulk_and_selection_cap_is_real_packet_limit(self):
        e=world();e.quote_selection=['x']*100;o=e.observe()
        i=o['entities'].index(('quote_add','x'));self.assertFalse(o['masks']['family'][i,5])
        i=o['entities'].index(('quote_remove',99));self.assertTrue(o['masks']['family'][i,6])
        i=o['entities'].index(('read',('SIG',('bulk_prices',))))
        a=dict(market=i,family=10,price=0,side=0,patience=0,target_fraction=.5,child_fraction=.5,
            query_filter=0,query_from_fraction=0.,query_to_fraction=1.)
        self.assertEqual(e.act_raw(a,o)['status'],'IN_FLIGHT');e.advance(e.busy_until)
        self.assertEqual(len(e.last_quote_response['data']),100);self.assertEqual(len(e.budget.reads),1)

    def test_existing_bulk_presets_remain_valid_after_new_read_targets_appear(self):
        from .bulk_quotes import add_targets
        e=world();first=next(k for k in e.keys['SIG']if k[:2]==('bulk_prices','group'))
        e.keys['SIG'].append(('book','y'));add_targets(e)
        self.assertIn(first,e.keys['SIG'])
        count=len(e.keys['SIG']);add_targets(e);self.assertEqual(len(e.keys['SIG']),count)
        r=read(e,None,key=first);self.assertEqual(r['requestedIds'],first[2])

    def test_expired_resting_orders_and_depleted_external_levels_are_not_quotes(self):
        from .engine import Resting
        e=world();b=e.books['x'];p=b.book.bids[0][0]
        b.depleted[('bids',p)]=b.book.bids[0][1]
        e.api_orders[999]=Resting(999,OrderSpec('x','YES','BUY',1,990000,e.now),1,e.now-1)
        q=snapshot(e,'x');self.assertNotEqual(q['bestBid'],990000)
        self.assertNotEqual(q['bestBid'],p)

    def test_migration_preserves_weights_and_checkpoint_protocol(self):
        p=ComponentPolicy(32);p.eval();c,record=migrate_bulk_quotes(p)
        self.assertFalse(c.training);self.assertTrue(record['optimizer_reset_required'])
        for k,v in p.state_dict().items():self.assertTrue(torch.equal(v,c.state_dict()[k]))
        with tempfile.TemporaryDirectory()as d:
            path=Path(d)/'quote.pt'
            sha=save_checkpoint(path,c,torch.optim.Adam(c.parameters()),dataset_manifest_sha256='a'*64,
                split_specification_sha256='b'*64)
            loaded,_=load_checkpoint(path,sha)
            self.assertIsInstance(loaded,BulkQuotePolicy);self.assertEqual(loaded.observation_protocol,BULK_QUOTES)

    def test_paid_quote_profile_supports_real_ppo_updates(self):
        from .stream import Stream
        from .training import collect_chunk,update
        from .economics import GrowthLedger
        e=world();read(e,['x','y']);model=BulkQuotePolicy(16)
        s=Stream(e,model,record_training=True,calibration={'decision_latency_ms':2},behavior_guard='tensor_versions_v1')
        c=collect_chunk(s,8,reward_ledger=GrowthLedger(e,development=True))
        result=update(model,torch.optim.Adam(model.parameters(),lr=1e-5),c,
            [f['economic_reward']for f in c.frames],role='train',epochs=1)
        self.assertTrue(result[0]['accepted'])

    def test_sealed_episode_with_paid_quotes_reconstructs_and_updates(self):
        from .batched import BatchStream
        from .episode_tape import EpisodeWriter,EpisodeReader
        from .episode_training import optimize
        from .economics import GrowthLedger
        def make(*args):
            e=world();e.end=1801;read(e,['x','y']);return e
        model=BulkQuotePolicy(16);e=make()
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


if __name__=='__main__':unittest.main()
