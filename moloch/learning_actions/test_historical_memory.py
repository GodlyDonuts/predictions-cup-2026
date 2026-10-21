"""Paid-only causal memory, not cross-venue equivalence or profit evidence."""
import tempfile,unittest
from pathlib import Path
import numpy as np
import torch
from .historical_memory import LATEST,profile
from .historical_neural import HistoricalNeuralPolicy,OLD,FEATURES
from .test_historical_neural import environment,trade,wait,KEY
from .stream import Stream
from .training import save_checkpoint,load_checkpoint
from .behavior_guard import contract,require_contract
from .economics import GrowthLedger


def env():
    e=environment();e.historical_memory_profile=LATEST;return e


def target(e,venue,key):
    v=e.observe();return v['contracts'][v['entities'].index(('read',(venue,key))),OLD:]


class HistoricalMemoryTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(903)

    def test_paid_three_venue_values_persist_after_consumption_with_growing_ages(self):
        e=env();sig=('historical_trades','840','m','YES');pm=('historical_sample','token','m','NO')
        s={'source_at_ms':0,'simulated_available_at_ms':1,'fact_sha256':'a'*64,
           'trade':{'id':'4','price':'.4','volume':'2','size':2,'side':'YES'}}
        p={'source_at_ms':0,'simulated_available_at_ms':1,'price':'.7'}
        e.register_history('SIG',sig,lambda now:[s]);e.register_history('Kalshi',KEY,lambda now:[trade(1)])
        e.register_history('Polymarket',pm,lambda now:p)
        addresses=[('SIG',sig),('Kalshi',KEY),('Polymarket',pm)]
        for venue,key in addresses:
            self.assertFalse(target(e,venue,key)[21:28].any())
            self.assertEqual(e.request_observation(venue,key)['status'],'IN_FLIGHT')
            self.assertFalse(target(e,venue,key)[21:28].any()) # not a request-time feed
        e.advance(1000);view=e.observe();before=[target(e,*a).copy()for a in addresses]
        e.act_raw(wait(),view);self.assertEqual(e.pending_history_receipts,[])
        e.advance(2000)
        for address,previous in zip(addresses,before):
            h=target(e,*address);self.assertTrue(h[21:28].any());self.assertGreater(h[8],previous[8])
            self.assertGreater(h[9],previous[9]);self.assertEqual(h[31],1)
        self.assertAlmostEqual(target(e,'SIG',sig)[15],.4)
        self.assertAlmostEqual(target(e,'Kalshi',KEY)[17],.125)
        self.assertAlmostEqual(target(e,'Kalshi',KEY)[18],.875)
        self.assertAlmostEqual(target(e,'Polymarket',pm)[15],.7)
        self.assertEqual(len(e.feed_requests),3);self.assertFalse(e.api_fills)
        self.assertEqual(e.observe()['contracts'].shape[1],FEATURES)

    def test_legacy_law_keeps_endpoint_prices_empty_after_consumption(self):
        e=environment();e.register_history('Kalshi',KEY,lambda now:[trade(1)])
        e.request_observation('Kalshi',KEY);e.advance(151);e.act_raw(wait(),e.observe())
        self.assertFalse(target(e,'Kalshi',KEY)[21:28].any());self.assertFalse(e._paid_history_summaries)

    def test_latest_returned_record_is_causal_and_cache_does_not_refresh_itself(self):
        e=env();calls=[];a=trade(1);b=trade(2);b.update(source_at_ms=5,simulated_available_at_ms=6)
        b['trade']['yes_price_dollars']='.3';box={'rows':[b,a]}
        def provider(now):calls.append(now);return box['rows']
        e.register_history('Kalshi',KEY,provider);e.advance(10)
        for _ in range(3):e.observe()
        self.assertEqual(calls,[])
        e.request_observation('Kalshi',KEY);e.advance(200)
        self.assertAlmostEqual(target(e,'Kalshi',KEY)[17],.3)
        # Provider mutation after receipt cannot change the delivered cache.
        b['trade']['yes_price_dollars']='.8';e.advance(300)
        self.assertAlmostEqual(target(e,'Kalshi',KEY)[17],.3);self.assertEqual(len(calls),1)
        # Explicit older-page query replaces this endpoint's latest response.
        box['rows']=[a];e.request_observation('Kalshi',KEY);e.advance(500)
        self.assertAlmostEqual(target(e,'Kalshi',KEY)[17],.125);self.assertEqual(len(calls),2)

    def test_empty_response_and_unknown_price_never_become_known_zero(self):
        e=env();rows=[trade(1)];e.register_history('Kalshi',KEY,lambda now:rows.copy())
        e.request_observation('Kalshi',KEY);e.advance(151);self.assertTrue(target(e,'Kalshi',KEY)[26])
        rows.clear();e.request_observation('Kalshi',KEY);e.advance(400)
        h=target(e,'Kalshi',KEY);self.assertEqual(h[47],1);self.assertFalse(h[21:28].any())
        other=env();sig=('historical_trades','840','m','YES')
        r={'source_at_ms':0,'simulated_available_at_ms':1,'fact_sha256':'a'*64,
           'trade':{'id':'4','price':None,'volume':'2','size':2,'side':'NO'}}
        other.register_history('SIG',sig,lambda now:[r]);other.request_observation('SIG',sig);other.advance(1000)
        h=target(other,'SIG',sig);self.assertFalse(h[21:28].any());self.assertEqual(h[30],1);self.assertEqual(h[44],1)

    def test_future_record_fails_and_fresh_world_has_no_cache(self):
        e=env();r=trade(1);r['simulated_available_at_ms']=2
        e.register_history('Kalshi',KEY,lambda now:[r]);e.request_observation('Kalshi',KEY)
        with self.assertRaisesRegex(ValueError,'MUST_PRECEDE_REQUEST'):e.advance(151)
        self.assertFalse(env()._paid_history_summaries)

    def test_new_receipt_during_inference_does_not_rewrite_previous_observation(self):
        e=env();a=trade(1);box={'rows':[a]}
        e.register_history('Kalshi',KEY,lambda now:box['rows'])
        e.request_observation('Kalshi',KEY);e.advance(151);first=e.observe()
        i=first['entities'].index(('read',('Kalshi',KEY)));original=first['contracts'][i].copy()
        b=trade(2);b['trade']['yes_price_dollars']='.8';box['rows']=[b]
        e.request_observation('Kalshi',KEY);e.advance(301)
        np.testing.assert_array_equal(first['contracts'][i],original)
        e.act_raw(wait(),first)
        self.assertEqual(len(e.pending_history_receipts),1)
        self.assertAlmostEqual(target(e,'Kalshi',KEY)[17],.8)

    def test_profile_is_pinned_and_legacy_model_cannot_consume_new_features(self):
        e=env();model=HistoricalNeuralPolicy(16);legacy=contract(model)
        with self.assertRaisesRegex(ValueError,'HISTORICAL_MEMORY_MISMATCH'):Stream(e,model)
        model.historical_memory_profile=LATEST
        with self.assertRaisesRegex(ValueError,'PROTOCOL_CHANGED'):require_contract(model,legacy)
        with self.assertRaisesRegex(ValueError,'EXPLICIT_BEHAVIOR'):require_contract(model,None)
        with tempfile.TemporaryDirectory()as folder:
            p=Path(folder)/'model.pt';sha=save_checkpoint(p,model,torch.optim.Adam(model.parameters()),
                dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            loaded,bundle=load_checkpoint(p,sha)
            self.assertEqual(loaded.historical_memory_profile,LATEST)
            self.assertEqual(bundle['historical_memory_profile'],LATEST)
            self.assertEqual(contract(loaded),contract(model));Stream(e,loaded,calibration={'decision_latency_ms':1})
        for invalid in(True,'unknown',0):
            with self.subTest(invalid=invalid),self.assertRaises(ValueError):profile(invalid)

    def test_new_memory_reconstructs_sealed_episodes_and_receives_gradients(self):
        from .batched import BatchStream
        from .episode_tape import EpisodeWriter,EpisodeReader
        from .episode_training import optimize
        def make(*_):
            e=env();e.end=501;e.register_history('Kalshi',KEY,lambda now:[trade(1),trade(2)])
            e.request_observation('Kalshi',KEY);e.advance(151);e.act_raw(wait(),e.observe());return e
        e=make();model=HistoricalNeuralPolicy(16);model.historical_memory_profile=LATEST
        before=model.encoder[0].weight.detach().clone()
        stream=BatchStream([e],model,[GrowthLedger(e,development=True)],
            calibration={'decision_latency_ms':25},behavior_guard='tensor_versions_v1')
        with tempfile.TemporaryDirectory()as folder:
            writer=EpisodeWriter(Path(folder)/'episode',stream,dataset_sha256='a'*64,specification_sha256='b'*64)
            try:
                while e.now<e.end:writer.record(stream.collect(4))
                sha=writer.seal()
            finally:writer.close()
            reader=EpisodeReader(writer.path,sha)
            try:
                reports=optimize(reader,model,torch.optim.Adam(model.parameters(),lr=1e-5),
                    make,lambda x:GrowthLedger(x,development=True))
                self.assertTrue(any(r['accepted']for r in reports))
                self.assertFalse(torch.equal(before[:,OLD:],model.encoder[0].weight[:,OLD:]))
            finally:reader.close()
