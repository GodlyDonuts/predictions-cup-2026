import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

from .public_history_prefix import configure, enabled, information_bounds, query_bounds
from .trade_history import seed_role_history
from .test_actions import environment
from .test_trade_history import trade
from moloch.learning_realism.protocol import DAY, Split


class PublicHistoryPrefixTests(unittest.TestCase):
    def tape(self):
        rows = [trade(i, t, 0) for i, t in enumerate((0, 2, 4, 6, 8))]
        return SimpleNamespace(flow_rows=rows, flow_times=tuple(r['available_at_ms'] for r in rows),
            counterparty_information_bounds=(2, 8), split=Split(0, 10 * DAY), role='evolution')

    def test_full_history_defaults_to_real_public_past_queries(self):
        self.assertTrue(enabled({'mode': 'full_history'}))
        self.assertFalse(enabled({'mode': 'inspected_development'}))
        self.assertTrue(enabled({'mode': 'inspected_development', 'public_history_prefix': True}))
        with self.assertRaises(ValueError):
            enabled({'public_history_prefix': 'true'})

    def test_prefix_does_not_move_scoring_or_future_boundary(self):
        t = self.tape()
        self.assertEqual(configure(t, (0, 2), (2, 8)), (0, 8))
        self.assertEqual(t.counterparty_information_bounds, (2, 8))
        self.assertEqual(information_bounds(t, (2, 8)), (0, 8))
        t.reporting_history_information_bounds = (0, 9)
        with self.assertRaisesRegex(ValueError, 'FUTURE_BOUNDARY'):
            information_bounds(t, (2, 8))
        with self.assertRaises(ValueError):
            query_bounds((0, 10), (5, 8))

    def test_memory_history_is_paid_and_does_not_replay_past_fills(self):
        t, e = self.tape(), environment()
        configure(t, (0, 2), (2, 8))
        cash, events = e.cash, tuple(e.clock._events)
        self.assertEqual(seed_role_history(t, e, 6), 3)
        self.assertEqual(tuple(e.clock._events), events)
        self.assertFalse(e.flow.seen)
        self.assertFalse(e.known_trade_pages)
        self.assertFalse(e.api_fills)
        self.assertEqual(e.cash, cash)
        e.advance(6)
        result = e.request_observation('SIG', ('trade_flow', 'x'), trade_limit=200)
        self.assertEqual(result['status'], 'IN_FLIGHT')
        self.assertFalse(e.known_trade_pages)
        self.assertEqual(len(e.budget.reads), 1)
        e.advance(e.busy_until)
        page = e.known_trade_pages['x']
        self.assertEqual({r['match_id'] for r in page['data']}, {'0', '1', '2'})
        self.assertIn('causal_public_prefix', page['coverage']['reason'])
        self.assertFalse(e.api_fills)
        self.assertEqual(e.cash, cash)

    def test_episode_cannot_open_in_earlier_role_or_at_future_boundary(self):
        t, e = self.tape(), environment()
        configure(t, (0, 2), (2, 8))
        for opened in (1, 8):
            with self.assertRaisesRegex(ValueError, 'STAY_IN_ROLE'):
                seed_role_history(t, e, opened)

    def test_disk_and_memory_query_the_same_authorized_past(self):
        from .test_disk_tape import dataset
        from .disk_tape import build, DiskFlowTape
        from moloch.learning_flow.episodes import FlowTape
        from .engine import ApiTape
        from moloch.learning_realism.test_replay import fixture
        from .contract import UNIT
        with tempfile.TemporaryDirectory(prefix='moloch-public-prefix-') as folder:
            root = Path(folder)
            source = dataset(root)
            digest = build(source, root / 'cache')
            originals = (FlowTape(source), DiskFlowTape(root / 'cache', digest, source))
            pages = []
            try:
                for original in originals:
                    t = ApiTape.from_loaded(original, split=Split(0, 10 * DAY), role='train')
                    t.counterparty_information_bounds = (1400, 4000)
                    configure(t, (0, 1400), (1400, 4000))
                    e = t.episode(fixture().mechanics, 1601, 3900, initial_cash_micros=1000 * UNIT,
                                  target_wealth_micros=11000 * UNIT, burnin_ms=1)
                    self.assertFalse(e.known_trade_pages)
                    self.assertFalse(e.api_fills)
                    pages.append(e.trade_history.page('x', 1601, limit=200))
                self.assertEqual(pages[0], pages[1])
                self.assertEqual({r['match_id'] for r in pages[0]['data']}, {'1', '2', '3'})
            finally:
                originals[1].disk_store.close()

    def test_actual_experiment_pins_query_prefix_and_registers_its_data_use(self):
        import json
        from dataclasses import asdict
        from unittest.mock import patch
        from .experiment import Experiment
        from .selection import DataUse
        from .test_disk_tape import dataset
        from moloch.learning_realism.test_replay import fixture
        with tempfile.TemporaryDirectory(prefix='moloch-prefix-experiment-') as folder:
            root = Path(folder)
            source = dataset(root)
            m = asdict(fixture().mechanics)
            document = {'production_authority': False, 'classification': 'conditional_development_mechanics',
                        'budget': m.pop('budget'), 'limits': m.pop('limits'), 'hypotheses': m}
            mechanics = root / 'mechanics.json'
            mechanics.write_text(json.dumps(document, default=list))
            config = {'mode': 'inspected_development', 'device': 'cpu', 'dataset': str(source),
                'mechanics': str(mechanics), 'development_train_ms': [1, 1400],
                'development_selection_ms': [1400, 4000], 'initial_world_history_ms': 0,
                'public_history_prefix': True}
            with patch('moloch.learning_actions.experiment.DataUse', lambda: DataUse(root / 'registry')):
                experiment = Experiment(config, root / 'experiment')
            try:
                tape = experiment.tape('evolution')
                self.assertEqual(tape.reporting_history_information_bounds, (1, 4000))
                self.assertEqual(tape.counterparty_information_bounds, (1400, 4000))
                info = experiment.frozen['public_trade_reporting_history']
                self.assertEqual(info['evolution_information_bounds_ms'], [1, 4000])
                self.assertTrue(info['actor_access_requires_paid_read'])
                records = [json.loads(p.read_text()) for p in (root / 'registry').glob('*.json')]
                selection = next(r for r in records if r['kind'] == 'selection')
                self.assertEqual((selection['start_ms'], selection['end_ms']), (1, 4000))
                env = experiment.world('evolution', 1601, 3900)
                self.assertEqual({r['match_id'] for r in env.trade_history.page('x', 1601)['data']}, {'1', '2', '3'})
                self.assertFalse(env.known_trade_pages)
                self.assertFalse(env.api_fills)
            finally:
                experiment.close()


if __name__ == '__main__':
    unittest.main()
