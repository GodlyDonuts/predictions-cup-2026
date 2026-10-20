"""Observable API metadata and complete-domain mechanics, not learned ROI."""
import tempfile,unittest
from pathlib import Path
import numpy as np
import torch
from .test_bulk_quotes import world as bulk_world
from .api_affordances import ApiAffordanceEpisode,ApiAffordancePolicy,migrate_api_affordances,TYPE,CONTRACT,MARKET,SIGNATURE,PACKET_INDEX,PACKET_SIZE,ENDPOINTS
from .bulk_quotes import BulkQuoteEpisode,BulkQuotePolicy
from .observation_protocol import API_TARGETS
from .feature_cache import identity
from .stream import Stream
from .training import collect_chunk,update,save_checkpoint,load_checkpoint
from .economics import GrowthLedger


def world():
    e=bulk_world();e.__class__=ApiAffordanceEpisode;e.observation_protocol=API_TARGETS;e.named_api_targets=True
    return e


class ApiAffordanceTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(617)

    def test_named_endpoints_share_contract_market_and_full_request_identities(self):
        e=world();market=e.mapping['x'];e.keys['Kalshi'].append(('reference','Kalshi',market,'fixture-ticker'))
        o=e.observe();f=o['contracts'];entities=o['entities'];ci=entities.index(('contract','x'))
        for target,category in((('book','x'),'book'),(('quote','x'),'price'),(('trade_flow','x'),'trade_flow')):
            i=entities.index(('read',('SIG',target)))
            np.testing.assert_array_equal(f[i,CONTRACT],f[ci,CONTRACT])
            np.testing.assert_array_equal(f[i,MARKET],f[ci,MARKET])
            self.assertEqual(f[i,TYPE].sum(),1);self.assertEqual(f[i,TYPE.start+ENDPOINTS.index(category)],1)
            np.testing.assert_array_equal(f[i,SIGNATURE],np.asarray(identity('read',('SIG',target)),dtype=np.float32))
        external=entities.index(('read',('Kalshi',('reference','Kalshi',market,'fixture-ticker'))))
        np.testing.assert_array_equal(f[external,MARKET],f[ci,MARKET])
        self.assertEqual(f[external,TYPE.start+ENDPOINTS.index('price')],1)
        for kind in('account','positions','owned_orders','collateral'):
            i=entities.index(('read',('SIG',(kind,))))
            self.assertEqual(f[i,TYPE.start+ENDPOINTS.index(kind)],1)

    def test_existing_features_and_masks_are_preserved_and_observing_is_free(self):
        e=world();named=e.observe();e.__class__=BulkQuoteEpisode;legacy=e.observe()
        self.assertEqual(named['entities'],legacy['entities'])
        np.testing.assert_array_equal(named['contracts'][:,:438],legacy['contracts'])
        for k in legacy['masks']:np.testing.assert_array_equal(named['masks'][k],legacy['masks'][k])
        self.assertFalse(e.budget.reads);self.assertFalse(e.known_quotes);self.assertFalse(e.actor_books)

    def test_new_metadata_does_not_depend_on_hidden_prices_cash_or_positions(self):
        from dataclasses import replace
        e=world();before=e.observe()
        e.books['x'].book=replace(e.books['x'].book,bids=((300000,17),),asks=((700000,21),))
        e.cash+=100000000;e.lots={('x','YES'):[[5,700000]]}
        after=e.observe();self.assertEqual(before['entities'],after['entities'])
        np.testing.assert_array_equal(before['contracts'][:,438:],after['contracts'][:,438:])
        self.assertFalse(e.known_lots);self.assertFalse(e.known_quotes);self.assertFalse(e.budget.reads)

    def test_presets_and_arbitrary_ordered_duplicate_packet_plans_are_linked(self):
        e=world();e.quote_selection=['y','x','y'];o=e.observe();f=o['contracts'];entities=o['entities']
        packet=('bulk_prices','group',('x','y'));i=entities.index(('read',('SIG',packet)))
        for j,ex in enumerate(('x','y')):
            q=entities.index(('quote_add',ex));np.testing.assert_array_equal(f[q,SIGNATURE],f[i,SIGNATURE])
            self.assertAlmostEqual(float(f[q,PACKET_INDEX]),j/100.)
            self.assertAlmostEqual(float(f[q,PACKET_SIZE]),.02)
        i=entities.index(('read',('SIG',('bulk_prices',))))
        for j,ex in enumerate(e.quote_selection):
            q=entities.index(('quote_remove',j));c=entities.index(('contract',ex))
            np.testing.assert_array_equal(f[q,SIGNATURE],f[i,SIGNATURE])
            np.testing.assert_array_equal(f[q,CONTRACT],f[c,CONTRACT])
            self.assertAlmostEqual(float(f[q,PACKET_INDEX]),j/100.)

    def test_declared_catalog_presets_do_not_grow_per_book_arrival(self):
        e=world();e.exchanges.extend(str(i)for i in range(250))
        e.keys['SIG']=[k for k in e.keys['SIG']if k[:2]!=('bulk_prices','group')];e.quote_keys=set(e.keys['SIG'])
        e.observe();initial=[k for k in e.keys['SIG']if k[:2]==('bulk_prices','group')]
        self.assertEqual(len(initial),3)
        for ex in e.exchanges[1:11]:e.keys['SIG'].append(('book',ex));e.observe()
        self.assertEqual(initial,[k for k in e.keys['SIG']if k[:2]==('bulk_prices','group')])
        self.assertTrue(all(1<=len(k[2])<=100 for k in initial))

    def test_migration_preserves_old_values_and_starts_new_columns_at_zero(self):
        p=BulkQuotePolicy(16);c,r=migrate_api_affordances(p)
        self.assertTrue(r['old_scores_must_not_be_reused'])
        for k,v in p.state_dict().items():
            actual=c.state_dict()[k]
            if k=='encoder.0.weight':
                self.assertTrue(torch.equal(actual[:,:438],v));self.assertFalse(actual[:,438:].count_nonzero())
            else:self.assertTrue(torch.equal(actual,v),k)
        with tempfile.TemporaryDirectory()as d:
            path=Path(d)/'named.pt';sha=save_checkpoint(path,c,torch.optim.Adam(c.parameters()),
                dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            loaded,_=load_checkpoint(path,sha);self.assertIs(type(loaded),ApiAffordancePolicy)
            self.assertEqual(loaded.contract_features,469);self.assertEqual(loaded.observation_protocol,API_TARGETS)

    def test_real_ppo_update_accepts_new_features_and_receipt_protocol(self):
        e=world();m=ApiAffordancePolicy(16)
        s=Stream(e,m,record_training=True,rollout_only=True,calibration={'decision_latency_ms':5},behavior_guard='tensor_versions_v1')
        chunk=collect_chunk(s,8,reward_ledger=GrowthLedger(e,development=True))
        result=update(m,torch.optim.Adam(m.parameters(),lr=1e-5),chunk,
            [f['economic_reward']for f in chunk.frames],role='train',epochs=1)
        self.assertTrue(result[0]['accepted'])

    def test_cache_matches_uncached_and_cannot_be_poisoned_by_caller(self):
        e=world();first=e.observe();saved=first['contracts'].copy()
        first['contracts'][:,438:]=-19
        cached=e.observe();np.testing.assert_array_equal(cached['contracts'],saved)
        e._api_metadata_cache=None;fresh=e.observe()
        np.testing.assert_array_equal(cached['contracts'],fresh['contracts'])
        self.assertFalse(e._api_metadata_cache[1].flags.writeable)

    def test_cache_tracks_packet_order_catalog_and_delivered_cursor_changes(self):
        e=world();e.observe()
        for change in(lambda:e.quote_selection.extend(['x','y','y']),
                       lambda:e.quote_selection.reverse(),
                       lambda:e.mapping.__setitem__('x','changed-public-market'),
                       lambda:e.order_page_issued_cursors.__setitem__('delivered',('ALL','x',None))):
            prior=e._api_metadata_cache;change();cached=e.observe()
            self.assertIsNot(e._api_metadata_cache,prior)
            e._api_metadata_cache=None;fresh=e.observe()
            np.testing.assert_array_equal(cached['contracts'],fresh['contracts'])
            for key in fresh['masks']:np.testing.assert_array_equal(cached['masks'][key],fresh['masks'][key])

    def test_cache_does_not_freeze_paid_quote_values_or_elapsed_ages(self):
        from .test_bulk_quotes import read
        e=world();e.observe();read(e,['x']);quoted=e.observe();before=quoted['contracts'].copy()
        e.advance(e.now+7);cached=e.observe();e._api_metadata_cache=None;fresh=e.observe()
        np.testing.assert_array_equal(cached['contracts'],fresh['contracts'])
        self.assertFalse(np.array_equal(before[:,:438],fresh['contracts'][:,:438]))

    def test_sealed_episode_reconstructs_named_features_and_accepts_update(self):
        from .batched import BatchStream
        from .episode_tape import EpisodeWriter,EpisodeReader
        from .episode_training import optimize
        from .test_bulk_quotes import read
        def make(*args):
            e=world();e.end=1801;read(e,['x','y']);return e
        model=ApiAffordancePolicy(16);e=make()
        stream=BatchStream([e],model,[GrowthLedger(e,development=True)],
            calibration={'decision_latency_ms':10},behavior_guard='tensor_versions_v1')
        with tempfile.TemporaryDirectory()as d:
            writer=EpisodeWriter(Path(d)/'diary',stream,dataset_sha256='a'*64,specification_sha256='b'*64)
            while e.now<e.end:writer.record(stream.collect(8))
            reader=EpisodeReader(writer.path,writer.seal())
            try:
                reports=optimize(reader,model,torch.optim.Adam(model.parameters(),lr=1e-5),make,
                    lambda env:GrowthLedger(env,development=True))
                self.assertTrue(any(r['accepted']for r in reports))
            finally:reader.close()


if __name__=='__main__':unittest.main()
