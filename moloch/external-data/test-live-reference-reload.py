"""Economic refresh boundaries; no network, worker or GPU."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import types
import unittest

if sys.platform == 'win32':
    sys.modules.setdefault('fcntl', types.SimpleNamespace())
spec = importlib.util.spec_from_file_location('refresh', Path(__file__).with_name('run-live-reference-reload.py'))
refresh = importlib.util.module_from_spec(spec)
spec.loader.exec_module(refresh)
D = refresh.Decimal


class RefreshTests(unittest.TestCase):
    def review(self):
        return {'exchangeId': '1067', 'side': 'YES', 'ordinaryProbability': '0.5000',
            'expiresAtMs': 900_000, 'exceptionalProbabilityAllowance': '0.01',
            'minimumExceptionalPayout': '0', 'marginPerShare': '0.01',
            'passiveValueExit': {'version': 1, 'maximumQuantity': 10_000,
                'pricingObjective': {'kind': 'fifoProfitTurnover', 'minimumNetGain': '.005'}}}

    def test_unchanged_executable_ticks_do_not_force_new_review(self):
        r = self.review()
        self.assertEqual(refresh.executable_ticks(r, D('.5000')), refresh.executable_ticks(r, D('.5001')))
        self.assertNotEqual(refresh.executable_ticks(r, D('.5000')), refresh.executable_ticks(r, D('.506')))

    def test_no_complement_and_value_exit(self):
        r = self.review(); r['side'] = 'NO'
        self.assertEqual(refresh.executable_ticks(r, D('.5000')), refresh.executable_ticks(r, D('.4999')))
        r['passiveValueExit'].pop('pricingObjective')
        self.assertEqual(refresh.executable_ticks(r, D('.5'))[1], 99)

    def test_expiry_and_open_order_filter(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / 'review.json'; p.write_text(json.dumps(self.review()))
            control = {'fundamentalPolicy': {'reviews': [{'path': str(p)}]}}
            capture = {'rows': [{'exchangeId': '1067', 'yesMidpoint': '.5001'}]}
            self.assertEqual(refresh.changed_scopes(capture, control, set(), 1000), [])
            self.assertEqual(refresh.changed_scopes(capture, control, set(), 780_000), ['1067'])
            self.assertEqual(refresh.changed_scopes(capture, control, {'1067'}, 780_000), [])


if __name__ == '__main__':
    unittest.main()
