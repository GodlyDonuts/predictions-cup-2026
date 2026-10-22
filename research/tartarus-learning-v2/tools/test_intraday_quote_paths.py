import unittest
from intraday_quote_paths import sweep, best_cycle


class QuotePathTests(unittest.TestCase):
    def test_full_depth_and_complement_have_exact_equal_cost(self):
        book = dict(asks=[dict(price='0.3', quantity=2), dict(price='0.35', quantity=3)],
                    bids=[dict(price='0.7', quantity=2), dict(price='0.65', quantity=3)])
        self.assertEqual(sweep(book, 'YES', 4, True), 1300000)
        self.assertEqual(sweep(book, 'NO', 4, True), 1300000)
        self.assertIsNone(sweep(book, 'YES', 6, True))

    def test_later_bid_can_exceed_earlier_full_depth_ask(self):
        points = [dict(receivedAtMs=0, segment=0, askCostMicros=1300000, bidProceedsMicros=1200000),
                  dict(receivedAtMs=10000, segment=0, askCostMicros=1500000, bidProceedsMicros=1400000)]
        self.assertEqual(best_cycle(points, 10000)['maximumGrossSurplusMicros'], 100000)
        self.assertEqual(best_cycle(points, 11000)['maximumGrossSurplusMicros'], 0)

    def test_a_source_gap_prevents_cross_gap_cycle_credit(self):
        points = [dict(receivedAtMs=0, segment=0, askCostMicros=1300000, bidProceedsMicros=1200000),
                  dict(receivedAtMs=10000, segment=1, askCostMicros=1500000, bidProceedsMicros=1400000)]
        self.assertEqual(best_cycle(points, 1000)['maximumGrossSurplusMicros'], 0)

    def test_future_cheaper_ask_cannot_finance_an_earlier_bid(self):
        points = [dict(receivedAtMs=0, segment=0, askCostMicros=1500000, bidProceedsMicros=1400000),
                  dict(receivedAtMs=10000, segment=0, askCostMicros=1200000, bidProceedsMicros=1100000)]
        self.assertEqual(best_cycle(points, 1000)['maximumGrossSurplusMicros'], 0)


if __name__ == '__main__':
    unittest.main()
