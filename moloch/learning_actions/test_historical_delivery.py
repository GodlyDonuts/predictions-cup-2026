import unittest
from copy import deepcopy
from unittest.mock import patch
from moloch.learning_realism.test_vector import tapes
from moloch.learning_realism.test_replay import fixture
from .historical_delivery import HistoricalInformationTape, owns_external_payload, register_kalshi
from .contract import UNIT


def environment():
    _, base = tapes()
    tape = HistoricalInformationTape.from_loaded(base, split=base.split, role='train')
    return tape.episode(fixture().mechanics, 1, 100000,
                        initial_cash_micros=1000 * UNIT,
                        target_wealth_micros=11000 * UNIT, burnin_ms=1)


class HistoricalDeliveryTests(unittest.TestCase):
    def test_owned_response_transfers_without_changing_paid_clocks(self):
        key=('historical_trades','A','m','YES')
        captured=[]
        def fresh(now):
            value=[{'trade':{'id':'original'},'at':now}];captured.append(value);return value
        snapshots=[]
        for owned in (False,True):
            env=environment();env.register_history('Kalshi',key,fresh,payload_is_owned=owned)
            with patch('moloch.learning_actions.historical_delivery.deepcopy',side_effect=deepcopy) as copy:
                self.assertEqual(env.request_observation('Kalshi',key),{'status':'IN_FLIGHT'})
                self.assertEqual(copy.call_count,0 if owned else 1)
            env.advance(151)
            payload=env.known_historical_information['Kalshi',key]['payload']
            self.assertEqual(payload,captured[-1])
            self.assertEqual(payload is captured[-1],owned)
            snapshots.append((env.observe(),list(env.feed_requests),list(env.feed_receipts),
                              list(env.external_admissions['Kalshi']),env.cash))
        # Finance arrays are covered by the full real-source trace comparison;
        # exact receipt, limits and copied public payloads are compared here.
        self.assertEqual(snapshots[0][1:],snapshots[1][1:])
        self.assertEqual(snapshots[0][0]['historical_information'],snapshots[1][0]['historical_information'])

    def test_builtin_ownership_does_not_cover_subclasses_or_mixed_unions(self):
        from .historical_kalshi import KalshiHistory
        from .historical_samples import Samples
        from .historical_union import KalshiUnion,SampleUnion
        class Extension(KalshiHistory):pass
        native=object.__new__(KalshiHistory);samples=object.__new__(Samples)
        native.tickers={'A'}
        self.assertTrue(owns_external_payload(native,'Kalshi'))
        self.assertTrue(owns_external_payload(samples,'Polymarket'))
        self.assertTrue(owns_external_payload(KalshiUnion([native]),'Kalshi'))
        self.assertTrue(owns_external_payload(SampleUnion([samples]),'Polymarket'))
        self.assertFalse(owns_external_payload(object.__new__(Extension),'Kalshi'))
        self.assertFalse(owns_external_payload(SampleUnion([]),'Polymarket'))
        self.assertFalse(owns_external_payload(SampleUnion([samples,object()]),'Polymarket'))

    def test_custom_registered_reader_still_snapshots_reused_values(self):
        class Reader:
            tickers={'A'}
            payload=[{'trade':{'id':'original'}}]
            def page(self,*args,**kwargs):return self.payload
        reader=Reader();env=environment();register_kalshi(env,reader,{'A':('m','YES')})
        key=('historical_trades','A','m','YES')
        self.assertFalse(env.owned_history_payloads)
        env.request_observation('Kalshi',key);reader.payload[0]['trade']['id']='changed'
        env.advance(151)
        self.assertEqual(env.known_historical_information['Kalshi',key]['payload'][0]['trade']['id'],'original')

    def test_ownership_declaration_must_be_boolean(self):
        env=environment();key=('historical_sample','P','m','YES')
        with self.assertRaisesRegex(ValueError,'EXPLICIT_HISTORY_PAYLOAD_OWNERSHIP'):
            env.register_history('Polymarket',key,lambda now:None,payload_is_owned=1)
        self.assertFalse(env.historical_providers)

    def test_sig_shares_existing_queue_and_delays_immutable_snapshot(self):
        env = environment()
        key = ('historical_candles', 'x', 'm', 'YES')
        payload = {'close': '0.40000001'}
        calls = []
        def read(now):
            calls.append(now)
            return payload
        env.register_history('SIG', key, read)
        before = deepcopy(env.books)
        self.assertEqual(env.request_observation('SIG', key)['status'], 'IN_FLIGHT')
        self.assertEqual(env.request_observation('SIG', ('account',))['status'], 'BUSY')
        self.assertEqual(env.observe()['historical_information'], {})
        payload['close'] = '0.9'
        arrival = env.now + env.mechanics.read_latency_ms
        self.assertEqual(env.clock.next_observable(), arrival)
        env.advance(arrival - 1)
        self.assertFalse(env.known_historical_information)
        env.advance(arrival)
        observed = env.observe()['historical_information']
        self.assertEqual(observed[('SIG', key)]['payload']['close'], '0.40000001')
        observed[('SIG', key)]['payload']['close'] = '1'
        self.assertEqual(env.known_historical_information[('SIG', key)]['payload']['close'], '0.40000001')
        self.assertEqual(calls, [1])
        self.assertEqual(len(env.budget.reads), 1)
        self.assertFalse(env.budget.writes)
        self.assertFalse(env.actor_books)
        self.assertFalse(env.api_fills)
        self.assertEqual(env.books['x'].depleted, before['x'].depleted)

    def test_external_targets_share_venue_budget_and_leave_sig_free(self):
        env = environment()
        one = ('historical_trades', 'A', 'm', 'YES')
        two = ('historical_trades', 'B', 'm', 'NO')
        env.register_history('Kalshi', one, lambda now: {'at': now})
        env.register_history('Kalshi', two, lambda now: {'at': now})
        self.assertEqual(env.request_observation('Kalshi', one)['status'], 'IN_FLIGHT')
        self.assertEqual(env.request_observation('Kalshi', two)['status'], 'RATE_WAIT')
        self.assertFalse(env.budget.reads)
        self.assertEqual(env.request_observation('SIG', ('account',))['status'], 'IN_FLIGHT')
        env.advance(51)
        self.assertEqual(env.request_observation('Kalshi', two)['status'], 'IN_FLIGHT')
        self.assertEqual(list(env.external_admissions['Kalshi']), [1, 51])
        env.advance(201)
        self.assertEqual(env.feed_receipt_counts['Kalshi'], 2)
        self.assertEqual(env.known_historical_information[('Kalshi', two)]['payload']['at'], 51)

    def test_ordinary_external_request_and_history_use_same_admissions(self):
        env = environment()
        key = ('historical_sample', 'asset', 'm', 'YES')
        env.register_history('Polymarket', key, lambda now: None)
        env.external_admissions['Polymarket'].extend([1] * 30)
        result = env.request_observation('Polymarket', key)
        self.assertEqual(result, {'status': 'RATE_WAIT', 'ready_ms': 1001})
        self.assertFalse(env.feed_requests)
        env.advance(1001)
        self.assertEqual(env.request_observation('Polymarket', key)['status'], 'IN_FLIGHT')
        env.advance(1151)
        self.assertIsNone(env.known_historical_information[('Polymarket', key)]['payload'])

    def test_provider_failure_does_not_leave_account_inflight(self):
        env = environment()
        def broken(now):
            raise ValueError('SOURCE_CHANGED')
        key = ('historical_candles', 'x', 'm', 'YES')
        env.register_history('SIG', key, broken)
        with self.assertRaisesRegex(ValueError, 'SOURCE_CHANGED'):
            env.request_observation('SIG', key)
        self.assertIsNone(env.budget.active)
        self.assertFalse(env.budget.reads)
        self.assertFalse(env.feed_requests)
