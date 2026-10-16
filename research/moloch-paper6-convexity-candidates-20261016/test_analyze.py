import unittest
from decimal import Decimal
import analyze


class Economics(unittest.TestCase):
    def test_alaska_budget_and_tick_are_not_full_displayed_depth(self):
        r = analyze.economics(".507", ".300", "11929", "2000")
        self.assertEqual(r["entryCeiling"], "0.480")
        self.assertEqual((r["quantity"], Decimal(r["principalAtCapturedAsk"])), (6666, Decimal("1999.800")))
        self.assertEqual(Decimal(r["winningIncrement"]), Decimal("4666.2"))
        self.assertTrue(r["passesCapturedPrice"])

    def test_iowa_depth_is_less_than_budget_and_quantity_cap(self):
        r = analyze.economics(".166", ".130", "8289", "2000")
        self.assertEqual(r["entryCeiling"], "0.150")
        self.assertEqual(r["quantity"], 8289)
        self.assertEqual(Decimal(r["principalAtCapturedAsk"]), Decimal("1077.57"))
        self.assertFalse(analyze.economics(".140", ".130", "8289", "2000")["passesCapturedPrice"])

    def test_probability_is_win_chance_not_vote_share(self):
        raw = b"Candidate chance to win Forecast share Rob Sand (D) 83 in 100 83.4% 51.9 Zach Lahn (R) 17 in 100 16.6% 45.7"
        self.assertEqual(analyze.current_probability(raw, "Zach Lahn"), Decimal(".166"))

    def test_no_fake_longshot_edge_or_capital(self):
        self.assertFalse(analyze.economics(".040", ".105", "3383", "2000")["passesCapturedPrice"])
        self.assertEqual(analyze.economics(".166", ".13", "20000", "2000")["quantity"], 10000)
        self.assertEqual(analyze.economics(".507", ".30", "11929", "0")["quantity"], 0)


if __name__ == "__main__":
    unittest.main()
