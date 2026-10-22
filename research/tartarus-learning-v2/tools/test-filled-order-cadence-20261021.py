"""Physical orders, partial receipts and UTC boundaries remain distinct."""
import importlib.util
import contextlib
import hashlib
import io
import json
import tempfile
from datetime import datetime, timezone
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('audit', Path(__file__).with_name('audit-r71-turnover-20261021.py'))
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class Cadence(unittest.TestCase):
    def test_completed_minute_report_does_not_recount_an_older_orders_partial(self):
        spec = importlib.util.spec_from_file_location('minute_report', Path(__file__).with_name('report-trade-cadence.py'))
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        rows = [dict(self.fill('1', '2026-10-21T21:03:00Z'), sourceId='a'),
                dict(self.fill('1', '2026-10-21T21:09:59Z'), sourceId='b'),
                dict(self.fill('2', '2026-10-21T21:09:59Z'), sourceId='c')]
        now = int(datetime(2026, 10, 21, 21, 10, tzinfo=timezone.utc).timestamp()*1000)
        result = module.report(json.dumps(dict(fills=rows)).encode(), now, 5)
        self.assertEqual(result['actualFillReceiptsInWindow'], 2)
        self.assertEqual(result['distinctFilledOrdersInWindow'], 1)
        self.assertEqual(result['emptyMinutes'], 4)
        self.assertFalse(result['oneFilledTradeEveryCompletedMinute'])

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

    def test_actual_buy_preserves_both_old_and_new_fifo_execution_clocks(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root/'bank-all-markets-v1').mkdir()
            (root/'receipts').mkdir()
            executed = '2026-10-20T10:00:00Z'
            lot = dict(quantity=4, price='0.500000', executedAt='2026-10-18T10:00:00Z')
            before = dict(fills=[], report=dict(positions={'1': dict(YES=[], NO=[lot])},
                cash='100.000000', ownedCost='2.000000', realized='0.000000'))
            baseline = root/'before.json'
            baseline.write_text(json.dumps(before))
            receipt = dict(status=200, endpoint='/orders/99/fills?limit=200', data=dict(data=[
                dict(id=7, filledAt=executed, quantity=-3, side='no', price=0.8)]))
            raw = json.dumps(receipt).encode()
            digest = hashlib.sha256(raw).hexdigest()
            (root/'receipts'/f'{digest}.json').write_bytes(raw)
            fill = dict(counterfactual=False, orderId=1, actualOrderId='99', sourceId='7',
                exchangeId='1', side='NO', action='buy', executedAt=executed, quantity=3,
                price='0.800000', fee='0.000000', receiptHash=digest)
            after = dict(fills=[fill], report=dict(positions={'1': dict(YES=[], NO=[lot,
                dict(quantity=3, price='0.800000', executedAt=executed)])},
                cash='97.600000', ownedCost='4.400000', realized='0.000000',
                newTradingRealized='0.000000', inheritedRealizedGross='0.000000', at=executed))
            (root/'bank-all-markets-v1/report.json').write_text(json.dumps(after))
            ack = dict(event=dict(kind='ack', qid=1, receipt=dict(status=200, endpoint='/orders',
                data=dict(orderId=99, exchangeId='1', side='no', action='buy'))))
            (root/'dispatch.ndjson').write_text(json.dumps(ack)+'\n')
            output = root/'audit.json'
            with patch.object(audit, 'ROOT', root), contextlib.redirect_stdout(io.StringIO()):
                audit.audit(baseline, output)
            proof = json.loads(output.read_text())
            self.assertTrue(proof['remainingFifoExactlyMatches'])
            self.assertEqual(proof['orders']['1']['cashDelta'], '-2.400000')


if __name__ == '__main__':
    unittest.main()
