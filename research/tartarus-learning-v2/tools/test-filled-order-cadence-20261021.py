"""Physical orders, partial receipts and UTC boundaries remain distinct."""
import importlib.util
from datetime import datetime, timezone
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('audit', Path(__file__).with_name('audit-r71-turnover-20261021.py'))
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class Cadence(unittest.TestCase):
    def fill(self, order, at):
        return dict(counterfactual=False, actualOrderId=order, executedAt=at)

    def test_partial_fill_later_does_not_create_another_trade(self):
        now = datetime(2026, 10, 21, 21, 10, tzinfo=timezone.utc)
        rows = [self.fill('1', '2026-10-21T21:03:00Z'),
                self.fill('1', '2026-10-21T21:09:59Z'),
                self.fill('2', '2026-10-21T21:09:59Z')]
        result = audit.filled_order_cadence(rows, now)
        self.assertEqual(result['windowsMinutes']['1']['distinctFirstFilledOrders'], 1)
        self.assertEqual(result['windowsMinutes']['5']['distinctFirstFilledOrders'], 1)
        self.assertEqual(result['windowsMinutes']['15']['distinctFirstFilledOrders'], 2)

    def test_windows_use_exact_utc_execution_boundaries(self):
        now = datetime(2026, 10, 21, 21, 10, tzinfo=timezone.utc)
        rows = [self.fill('1', '2026-10-21T21:09:00Z'),
                self.fill('2', '2026-10-21T21:09:00.001Z')]
        result = audit.filled_order_cadence(rows, now)
        self.assertEqual(result['windowsMinutes']['1']['distinctFirstFilledOrders'], 1)
        self.assertAlmostEqual(result['secondsSinceNewFilledOrder'], 59.999)

    def test_future_or_simulated_execution_is_rejected(self):
        now = datetime(2026, 10, 21, 21, 10, tzinfo=timezone.utc)
        with self.assertRaises(AssertionError):
            audit.filled_order_cadence([self.fill('1', '2026-10-21T21:10:01Z')], now)
        row = self.fill('1', '2026-10-21T21:09:00Z')
        row['counterfactual'] = True
        with self.assertRaises(AssertionError):
            audit.filled_order_cadence([row], now)

    def test_burst_cannot_satisfy_sustained_minute_cadence(self):
        now = datetime(2026, 10, 21, 21, 10, tzinfo=timezone.utc)
        rows = [self.fill(str(i), '2026-10-21T21:09:59Z') for i in range(5)]
        window = audit.filled_order_cadence(rows, now)['windowsMinutes']['5']
        self.assertTrue(window['averageTargetMet'])
        self.assertFalse(window['rollingMinuteCadenceMet'])
        self.assertEqual(window['completedUtcMinutesWithNoNewFilledOrder'], 4)
        self.assertEqual(window['maximumGapWithinWindowSeconds'], 299)

    def test_even_first_fills_satisfy_cadence_but_empty_history_does_not(self):
        now = datetime(2026, 10, 21, 21, 10, tzinfo=timezone.utc)
        rows = [self.fill(str(i), f'2026-10-21T21:0{i}:30Z') for i in range(5, 10)]
        window = audit.filled_order_cadence(rows, now)['windowsMinutes']['5']
        self.assertTrue(window['rollingMinuteCadenceMet'])
        self.assertEqual(window['completedUtcMinutesWithNoNewFilledOrder'], 0)
        self.assertEqual(window['maximumGapWithinWindowSeconds'], 60)
        empty = audit.filled_order_cadence([], now)['windowsMinutes']['1']
        self.assertFalse(empty['rollingMinuteCadenceMet'])


if __name__ == '__main__':
    unittest.main()
