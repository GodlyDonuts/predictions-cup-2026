import unittest
from copy import deepcopy
from moloch.learning_realism.test_vector import tapes
from moloch.learning_realism.test_replay import fixture
from .historical_delivery import HistoricalInformationTape
from .contract import UNIT


def environment():
    _, base = tapes()
    tape = HistoricalInformationTape.from_loaded(base, split=base.split, role='train')
    return tape.episode(fixture().mechanics, 1, 100000,
                        initial_cash_micros=1000 * UNIT,
                        target_wealth_micros=11000 * UNIT, burnin_ms=1)


class HistoricalDeliveryTests(unittest.TestCase):
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
