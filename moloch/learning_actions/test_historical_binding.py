import copy
import unittest
from unittest.mock import patch
from .historical_binding import HistoryBindings, validate


def config():
    sources = {venue: {'path': 'private-index', 'manifest_sha256': 'a' * 64,
                      'information_start_ms': 0,
                      'delivery': {'delay_ms': 60000 if venue == 'SIG' else 1000,
                                   'poll_ms': 50, 'phase_ms': 0, 'maximum_age_ms': 100000},
                      **({} if venue == 'SIG' else {'mapping': {'asset': ['m', 'YES']}})}
               for venue in ('SIG', 'Kalshi', 'Polymarket')}
    return {'format': 'three_venue_history_information_v1',
            'historical_mapping_availability_verified': False,
            'roles': {'train': sources, 'evolution': copy.deepcopy(sources)}}


class HistoricalBindingTests(unittest.TestCase):
    def test_historical_target_order_is_independent_of_json_object_key_order(self):
        from .historical_delivery import register_kalshi,register_polymarket
        class Env:
            def __init__(self):self.targets=[]
            def register_history(self,venue,key,provider,**kwargs):self.targets.append((venue,key))
        class Reader:
            tickers={'a','z'}
            def page(self,*_):return []
            def at(self,*_):return None
        a={'z':['m2','NO'],'a':['m1','YES']};b=dict(reversed(list(a.items())))
        for register in(register_kalshi,register_polymarket):
            left=Env();right=Env();register(left,Reader(),a);register(right,Reader(),b)
            self.assertEqual(left.targets,right.targets)
            self.assertEqual([x[1][1]for x in left.targets],['a','z'])

    def test_sig_trade_source_requires_explicit_pins(self):
        spec=config();spec['roles']['train']['SIG']['trade_information']={}
        with self.assertRaisesRegex(ValueError,'PINNED_SIG_TRADE'):validate(spec)
        extra={k:v for k,v in spec['roles']['train']['SIG'].items() if k!='trade_information'}
        spec['roles']['train']['SIG']['trade_information']=extra
        validate(spec)

    def test_missing_venue_and_unpinned_sources_fail(self):
        for change in ('venue', 'sha', 'mapping', 'provenance'):
            spec = config()
            if change == 'venue': del spec['roles']['train']['Kalshi']
            if change == 'sha': spec['roles']['train']['SIG']['manifest_sha256'] = 'bad'
            if change == 'mapping': spec['roles']['train']['Polymarket']['mapping'] = {}
            if change == 'provenance': spec['historical_mapping_availability_verified'] = 1
            with self.assertRaises(ValueError): validate(spec)

    def test_role_bounds_and_test_source_checked_before_opening_files(self):
        class Env:
            now, end = 100, 200
        bindings = HistoryBindings(config())
        with self.assertRaisesRegex(ValueError, 'NO_HISTORY_SOURCE'):
            bindings.attach(Env(), 'test', (100, 300))
        with self.assertRaisesRegex(ValueError, 'OUTSIDE_REGISTERED_ROLE'):
            bindings.attach(Env(), 'train', (101, 300))

    def test_partial_reader_failure_closes_previously_opened_source(self):
        class Env:
            now, end = 100, 200
        class Reader:
            closed = False
            def close(self): self.closed = True
        reader = Reader()
        bindings = HistoryBindings(config())
        with patch('moloch.learning_actions.historical_binding.CandleHistory', return_value=reader), \
             patch('moloch.learning_actions.historical_binding.KalshiHistory',
                   side_effect=ValueError('CHANGED_RECEIPT')):
            with self.assertRaisesRegex(ValueError, 'CHANGED_RECEIPT'):
                bindings.attach(Env(), 'train', (100, 300))
        self.assertTrue(reader.closed)
        self.assertEqual(bindings.readers, {})
