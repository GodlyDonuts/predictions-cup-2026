import tempfile
import unittest
from pathlib import Path
import numpy as np
import torch
from moloch.learning_realism.test_vector import tapes
from moloch.learning_realism.test_replay import fixture
from .historical_neural import HistoricalNeuralTape, HistoricalNeuralPolicy, FEATURES, OLD, encode
from .contract import UNIT
from .stream import Stream
from .training import collect_chunk, update, save_checkpoint, load_checkpoint
from .economics import GrowthLedger


def environment():
    _, base = tapes()
    tape = HistoricalNeuralTape.from_loaded(base, split=base.split, role='train')
    return tape.episode(fixture().mechanics, 1, 100000,
                        initial_cash_micros=1000 * UNIT,
                        target_wealth_micros=11000 * UNIT, burnin_ms=1)


KEY = ('historical_trades', 'A', 'm', 'YES')


def trade(i):
    return {'source_at_ms': 0, 'simulated_available_at_ms': 1,
            'actual_retrieved_at': '2099-01-01T00:00:00Z',
            'trade': {'trade_id': str(i), 'yes_price_dollars': '0.1250',
                      'no_price_dollars': '0.8750', 'count_fp': '1.50'}}


def wait():
    return dict(family=0, market=0, side=0, price=0, patience=0,
                target_fraction=0., child_fraction=0.,
                query_from_fraction=0., query_to_fraction=1.)


class HistoricalNeuralTests(unittest.TestCase):
    def test_actor_history_query_parameters_reach_paid_provider(self):
        e=environment();key=('historical_trades','840','m','YES');calls=[]
        def provider(now,**kwargs):calls.append((now,kwargs));return []
        e.register_history('SIG',key,lambda now:[],query_provider=provider)
        view=e.observe();i=view['entities'].index(('read',('SIG',key)))
        self.assertTrue(view['masks']['trade_query'][i,0])
        action={**wait(),'family':10,'market':i,'query_filter':1}
        result=e.act_raw(action,view)
        self.assertEqual(result['status'],'IN_FLIGHT')
        self.assertEqual(calls[0][1],{'trade_limit':1,'continue_trade_page':False,
            'trade_from_ms':0,'trade_to_ms':253402300799999})
        self.assertEqual(e.request_observation('SIG',key,continue_trade_page=True)['status'],'BUSY')
        self.assertEqual(len(calls),1)
        self.assertFalse(e.api_fills)

    def test_sig_public_prints_use_paid_budget_and_keep_null_price_unknown(self):
        e=environment();key=('historical_trades','840','m','YES')
        row={'source_at_ms':0,'simulated_available_at_ms':1,'fact_sha256':'a'*64,
             'trade':{'id':'4','price':None,'volume':'12.345','size':10,'side':'NO'}}
        e.register_history('SIG',key,lambda now:[row])
        self.assertEqual(e.request_observation('SIG',key)['status'],'IN_FLIGHT')
        self.assertEqual(e.request_observation('SIG',key)['status'],'BUSY')
        e.advance(1000);view=e.observe()
        ids=[i for i,(kind,_) in enumerate(view['entities']) if kind=='historical_record']
        self.assertEqual(len(ids),1)
        h=view['contracts'][ids[0],OLD:]
        self.assertFalse(h[21:28].any())
        self.assertEqual(h[44],1)
        self.assertFalse(h[39:43].any())
        self.assertFalse(e.api_fills)

    def setUp(self):
        torch.set_num_threads(1)
        torch.manual_seed(902)

    def test_all_paid_records_reach_tensor_without_executable_actions(self):
        e = environment()
        e.register_history('Kalshi', KEY, lambda now: [trade(i) for i in range(1000)])
        before = e.observe()
        self.assertEqual(before['contracts'].shape[1], FEATURES)
        self.assertFalse(any(k == 'historical_record' for k, _ in before['entities']))
        e.request_observation('Kalshi', KEY)
        e.advance(151)
        first = e.observe()
        ids = [i for i, (k, _) in enumerate(first['entities']) if k == 'historical_record']
        self.assertEqual(len(ids), 1000)
        self.assertTrue(first['present'][ids].all())
        self.assertFalse(first['masks']['family'][ids].any())
        self.assertFalse(first['masks']['market'][ids].any())
        self.assertTrue(np.all(first['contracts'][ids, OLD + 17] == .125))
        np.testing.assert_array_equal(first['contracts'], e.observe()['contracts'])
        e.act_raw(wait(), first)
        self.assertFalse(any(k == 'historical_record' for k, _ in e.observe()['entities']))
        self.assertEqual(len(e.known_historical_information[('Kalshi', KEY)]['payload']), 1000)
        self.assertFalse(e.api_fills)

    def test_receipt_arriving_during_inference_survives_consumption(self):
        e = environment()
        e.register_history('Kalshi', KEY, lambda now: [trade(now)])
        e.request_observation('Kalshi', KEY)
        e.advance(151)
        observed = e.observe()
        e.request_observation('Kalshi', KEY)
        e.advance(301)
        e.act_raw(wait(), observed)
        next_view = e.observe()
        self.assertEqual(next_view['history_receipt_sequence'], 2)
        self.assertEqual(len(e.pending_history_receipts), 1)
        self.assertEqual(e.pending_history_receipts[0][0], 2)
        self.assertEqual(e.pending_history_receipts[0][3]['payload'][0]['trade']['trade_id'], '151')

    def test_future_fields_rejected_and_actual_retrieval_is_not_a_feature(self):
        receipt = {'requested_at_ms': 10, 'received_at_ms': 20}
        row = trade(1)
        a = encode('Kalshi', KEY, receipt, row, 0, 20)
        row['actual_retrieved_at'] = '3000-01-01T00:00:00Z'
        np.testing.assert_array_equal(a, encode('Kalshi', KEY, receipt, row, 0, 20))
        row['simulated_available_at_ms'] = 11
        with self.assertRaisesRegex(ValueError, 'PRECEDE_REQUEST'):
            encode('Kalshi', KEY, receipt, row, 0, 20)
        with self.assertRaisesRegex(ValueError, 'PRECEDE_REQUEST'):
            encode('Kalshi', KEY, receipt, trade(1), 0, 19)

    def test_candles_and_samples_preserve_missingness_and_source_type(self):
        receipt = {'requested_at_ms': 60001, 'received_at_ms': 60101}
        key = ('historical_candles', 'x', 'm', 'YES')
        row = {'source_at_ms': 0, 'simulated_available_at_ms': 60000,
               'bar_end_ms': 60000, 'open': None, 'high': '0.8', 'low': '0.2',
               'close': '0.6', 'vwap': '0.5', 'volume': '1.25', 'tradeCount': 3}
        encoded = encode('SIG', key, receipt, row, 0, 60101)
        self.assertEqual(encoded[OLD + 21], 0)
        self.assertEqual(encoded[OLD + 22], 1)
        self.assertAlmostEqual(encoded[OLD + 19], np.log1p(1.25))
        row['bar_end_ms'] = 60001
        with self.assertRaisesRegex(ValueError, 'CANDLE_CLOSE'):
            encode('SIG', key, receipt, row, 0, 60101)
        sample = {'source_at_ms': 0, 'simulated_available_at_ms': 1, 'price': .7}
        encoded = encode('Polymarket', ('historical_sample', 'asset', 'm', 'NO'),
                         receipt, sample, 0, 60101)
        self.assertEqual(encoded[OLD + 2], 1)
        self.assertEqual(encoded[OLD + 7], 1)
        self.assertAlmostEqual(encoded[OLD + 15], .7)
        self.assertEqual(encoded[OLD + 26], 0)  # Not a YES execution quote.

    def test_sealed_replay_reconstructs_consumed_receipt_tokens(self):
        from .batched import BatchStream
        from .episode_tape import EpisodeWriter, EpisodeReader
        from .episode_training import optimize
        def make(*_):
            env = environment()
            env.end = 501
            env.register_history('Kalshi', KEY, lambda now: [trade(1), trade(2)])
            env.request_observation('Kalshi', KEY)
            env.advance(151)
            return env
        env = make()
        model = HistoricalNeuralPolicy(16)
        stream = BatchStream([env], model, [GrowthLedger(env, development=True)],
                             calibration={'decision_latency_ms': 25},
                             behavior_guard='tensor_versions_v1')
        with tempfile.TemporaryDirectory() as d:
            writer = EpisodeWriter(Path(d) / 'episode', stream,
                                   dataset_sha256='a' * 64, specification_sha256='b' * 64)
            while env.now < env.end:
                writer.record(stream.collect(4))
            reader = EpisodeReader(writer.path, writer.seal())
            try:
                results = optimize(reader, model, torch.optim.Adam(model.parameters(), lr=1e-5),
                                   make, lambda e: GrowthLedger(e, development=True))
                self.assertTrue(any(r['accepted'] for r in results))
            finally:
                reader.close()

    def test_receipt_input_learns_and_checkpoint_roundtrip_retains_protocol(self):
        e = environment()
        e.register_history('Kalshi', KEY, lambda now: [trade(1), trade(2)])
        e.request_observation('Kalshi', KEY)
        e.advance(151)
        model = HistoricalNeuralPolicy(16)
        stream = Stream(e, model, record_training=True, rollout_only=True,
                        calibration={'decision_latency_ms': 5}, behavior_guard='tensor_versions_v1')
        chunk = collect_chunk(stream, 4, reward_ledger=GrowthLedger(e, development=True))
        before = model.encoder[0].weight.detach().clone()
        optimizer = torch.optim.Adam(model.parameters(), lr=1e-5)
        result = update(model, optimizer, chunk, [f['economic_reward'] for f in chunk.frames],
                        role='train', epochs=1)
        self.assertTrue(result[0]['accepted'])
        self.assertFalse(torch.equal(before[:, OLD:], model.encoder[0].weight[:, OLD:]))
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / 'checkpoint.pt'
            sha = save_checkpoint(path, model, optimizer, dataset_manifest_sha256='a' * 64,
                                  split_specification_sha256='b' * 64)
            loaded, _ = load_checkpoint(path, sha)
            self.assertIs(type(loaded), HistoricalNeuralPolicy)
            self.assertEqual(loaded.observation_protocol, model.observation_protocol)
            self.assertTrue(torch.equal(loaded.encoder[0].weight, model.encoder[0].weight))
