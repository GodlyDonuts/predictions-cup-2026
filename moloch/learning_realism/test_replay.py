import math
import unittest
from moloch.learning_v2.account import UNIT, Limits, Target
from moloch.learning_v2.clock import BudgetConfig
from moloch.learning_v2.reference_env import Mechanics
from .protocol import Split, DAY, growth_score
from .replay import PollingEpisode


def book(at, ask=450000):
    return {'kind': 'book', 'market_id': 'm', 'exchange_id': 'x',
            'available_at_ms': at, 'source_at_ms': at, 'original_sha256': 'b'*64,
            'bids': [[400000, 100]], 'asks': [[ask, 100]]}


def reference(at, venue='Kalshi', market='m'):
    return {'kind': 'reference', 'venue': venue, 'market_id': market,
        'source': venue, 'mapping_kind': 'direct_party_reference',
        'mapping_semantics': 'same_party_conditional_not_contract_equivalence',
        'available_at_ms': at, 'received_at_ms': at, 'source_at_ms': at,
        'price_yes_micros': 500000, 'bid_yes_micros': 480000,
        'ask_yes_micros': 520000, 'original_sha256': 'c'*64}


def fixture(end=10000):
    m = Mechanics(BudgetConfig(100, 600, 24, 2500, 'a'*64),
        Limits(10000, 2000*UNIT, 100*UNIT), 100, 200, 100, 10000, 20000, 1, 'Kalshi')
    e = PollingEpisode(m, 0, 1000*UNIT, ('__GLOBAL_WAIT__', 'x'), 11000*UNIT, 0, end)
    e.clock.schedule(0, 10, 'tape', book(0))
    e.start_delivery()
    return e


class ReplayTests(unittest.TestCase):
    def test_no_information_before_receipt_and_original_age_preserved(self):
        e = fixture(); e.clock.schedule(0, 10, 'tape', reference(0))
        e.advance(0)
        self.assertIn('x', e.books); self.assertNotIn('x', e.actor_books)
        self.assertFalse(e.observe()['present'][1]); self.assertEqual(len(e.reference_cache), 0)
        e.advance(99); self.assertNotIn('x', e.actor_books)
        e.advance(100); self.assertIn('x', e.actor_books)
        self.assertEqual(len(e.reference_cache), 0)
        e.advance(150); self.assertEqual(len(e.reference_cache), 1)
        e.advance(5000)
        self.assertEqual(e.actor_books['x'].book.available_at_ms, 0)
        self.assertEqual(e.reference_cache[('m', 'Kalshi')]['available_at_ms'], 0)
        e.advance(20001); self.assertIsNone(e.venue_interval('x', 'Kalshi'))

    def test_external_throughput_and_no_external_orders(self):
        e = fixture(end=2000)
        for venue in ('Kalshi', 'Polymarket'):
            for i in range(200): e.clock.schedule(0, 10, 'tape', reference(0, venue, str(i)))
        e.advance(999)
        counts = {v: sum(x['venue'] == v for x in e.feed_requests) for v in ('Kalshi', 'Polymarket')}
        self.assertEqual(counts, {'Kalshi': 20, 'Polymarket': 30})
        self.assertEqual(e.cash, 1000*UNIT); self.assertFalse(e.orders)
        self.assertFalse(e.fill_events)
        with self.assertRaises(ValueError): e._event(999, 'tape', reference(999, 'Unknown'))

    def test_sig_polling_shares_execution_budget_and_completion_gap(self):
        e = fixture(); e.clock.schedule(0, 10, 'tape', reference(0)); e.advance(150)
        self.assertEqual(len(e.budget.reads), 1)
        p = e.submit(Target('x', 'YES', 25000, UNIT, 450000, 'BUY'), 60000)
        self.assertGreater(p.admitted_quantity, 0)
        e.advance(700)
        self.assertEqual(e.budget.active, ('1:read:1', False))
        self.assertEqual(len(e.budget.reads), 2)
        e.advance(800); self.assertEqual(e.budget.next_at, 1400)
        e.advance(4000)
        sig = [r['at_ms'] for r in e.feed_requests if r['venue'] == 'SIG']
        self.assertEqual(sig[0], 0)
        self.assertGreaterEqual(len(e.budget.writes), 1)

    def test_actor_prices_do_not_leak_hidden_world_and_matching_uses_world(self):
        e = fixture(); e.clock.schedule(0, 10, 'tape', reference(0)); e.advance(150)
        e.clock.schedule(200, 10, 'tape', book(200, 600000)); e.advance(200)
        self.assertEqual(e.actor_books['x'].book.asks[0][0], 450000)
        self.assertEqual(e.books['x'].book.asks[0][0], 600000)
        e.act({'family': 1, 'market': 1, 'side': 0, 'price': 2,
               'target_fraction': .025, 'child_fraction': 1., 'patience': 2})
        self.assertEqual(e.action_projections[-1][1].limit_price, 450000)
        self.assertGreater(e.action_projections[-1][2].admitted_quantity, 0)
        e.advance(2500)
        self.assertEqual(e.fill_events, [])  # cannot buy at hidden .60 using old .45

    def test_split_includes_burnin_and_drain_and_test_requires_frozen_model(self):
        s = Split(0, 20*DAY)
        self.assertEqual(s.bounds('train'), (0, 12*DAY))
        self.assertEqual(s.bounds('evolution'), (12*DAY, 16*DAY))
        self.assertEqual(s.bounds('test'), (16*DAY, 20*DAY))
        s.require('train', 1, 12*DAY-1)
        with self.assertRaises(ValueError): s.require('train', 1, 12*DAY)
        with self.assertRaises(ValueError): s.require('evolution', 12*DAY-1, 13*DAY)
        with self.assertRaises(ValueError): s.require('test', 16*DAY, 17*DAY)
        s.require('test', 16*DAY, 17*DAY, frozen_finalist_sha256='d'*64)

    def test_prints_require_separate_sig_read_and_retain_execution_time(self):
        from moloch.learning_flow.test_features import FlowTests
        e = fixture(); row = FlowTests().row()
        for key in ('available_at_ms', 'received_at_ms', 'executed_at_ms'): row[key] = 0
        row['exchange_id'] = 'x'
        e.clock.schedule(0, 10, 'trade_flow', row)
        e.advance(100)
        self.assertFalse(e.flow.rows['x'])
        e.advance(800)
        self.assertEqual(len(e.flow.rows['x']), 1)
        delivered = e.flow.rows['x'][0]
        self.assertEqual(delivered['available_at_ms'], 800)
        self.assertEqual(delivered['original_available_at_ms'], 0)
        self.assertEqual(delivered['executed_at_ms'], row['executed_at_ms'])
        self.assertEqual(len(e.budget.reads), 2)

    def test_polling_never_clears_retry_after_or_unknown_acceptance(self):
        e = fixture(); e.advance(0)
        e.budget.unknown.add('original-uncertain-write')
        e.budget.cooldown = 5000
        e.advance(4999)
        self.assertEqual(len(e.budget.reads), 1)
        self.assertEqual(e.budget.unknown, {'original-uncertain-write'})
        e.advance(5000)
        self.assertEqual(len(e.budget.reads), 2)
        with self.assertRaises(ValueError): e.budget.ready_at(5000, True)

    def test_reward_exact_formula_and_event_split_cannot_inflate_n(self):
        daily = [(100, 110), (110, 121), (121, 133.1), (133.1, 146.41)]
        a = growth_score(daily, [('event-a', 1), ('event-a', 2)])
        b = growth_score(daily, [('event-a', 3)])
        self.assertEqual(a, b); self.assertEqual(a['N'], 1)
        self.assertAlmostEqual(a['reward'], math.log(1.1)*math.sqrt(1/11))
        negative = growth_score([(100, 90), (90, 81)], [])
        self.assertAlmostEqual(negative['reward'], math.log(.9))
        self.assertFalse(growth_score([(100, 0), (1, 1)], [])['eligible'])
        with self.assertRaises(ValueError): growth_score([(100, 110)], [])

    def test_history_coverage_cannot_be_manufactured_from_sparse_endpoints(self):
        from types import SimpleNamespace
        from .prepare import coverage
        s = Split(0, 20*DAY)
        rows = []
        for at in (0, 20*DAY-1):
            rows.append(book(at))
            rows.extend(reference(at, venue) for venue in ('Kalshi', 'Polymarket'))
        result = coverage(SimpleNamespace(rows=rows), s)
        self.assertFalse(any(r['coverage_pass'] for r in result.values()))
        self.assertEqual(result['evolution']['venues']['SIG']['observations'], 0)


if __name__ == '__main__': unittest.main()
