"""Independent small integer enumerations and funding/opportunity-cost examples."""
import copy
from fractions import Fraction
import unittest

from moloch.tartarus.rotation_capacity import static_upper_bound

U = 1_000_000


def level(price, quantity):
    return {"price_micros": price, "quantity": quantity}


def upper(result):
    return Fraction(result["upper_wealth_micros_numerator"],
                    result["upper_wealth_micros_denominator"])


class RotationCapacityTests(unittest.TestCase):
    def test_sale_opportunity_cost_and_no_actual_cash_credit(self):
        buys, sells = [level(100000, 100)], [level(500000, 10)]
        original = copy.deepcopy((buys, sells))
        result = static_upper_bound(2*U, 10, buys, sells, allow_sales=True)
        self.assertEqual(upper(result), 70*U)  # 10 old sold, 70 new, no cash.
        self.assertEqual(result["fractional_sold_quantity"], "10")
        self.assertEqual((buys, sells), original)
        self.assertFalse(result["actual_sale_proceeds_credited"])
        self.assertFalse(result["execution_authority"])

    def test_no_cash_cannot_fund_an_unmodeled_sale(self):
        result = static_upper_bound(0, 10, [level(100000, 100)],
                                    [level(500000, 10)], allow_sales=False)
        self.assertEqual(upper(result), 10*U)
        self.assertEqual(result["fractional_bought_quantity"], "0")

    def test_finite_depth_and_unprofitable_exchange_preserve_hold(self):
        result = static_upper_bound(2*U, 10, [level(100000, 5)], [], allow_sales=True)
        self.assertEqual(upper(result), Fraction(33*U, 2))
        result = static_upper_bound(0, 10, [level(600000, 100)],
                                    [level(500000, 10)], allow_sales=True)
        self.assertEqual(upper(result), 10*U)

    def test_fractional_relaxation_dominates_enumerated_integer_allocations(self):
        # Enumerate all feasible integer purchase/sale tuples independently.
        for cash in (0, 100000, 900000):
            for ask in (100000, 350000, 750000):
                for bid in (200000, 550000, 800000):
                    result = static_upper_bound(cash, 3, [level(ask, 7)],
                                                [level(bid, 3)], allow_sales=True)
                    best = max(cash + s*bid - b*ask + (3-s+b)*U
                               for s in range(4) for b in range(8)
                               if b*ask <= cash+s*bid)
                    self.assertGreaterEqual(upper(result), best)

    def test_multiple_depth_breakpoints_dominates_integer_enumeration(self):
        buys = [level(600000, 2), level(200000, 3)]
        sells = [level(700000, 1), level(400000, 2)]
        r = static_upper_bound(100000, 3, buys, sells, allow_sales=True)
        best = max(100000+s1*700000+s2*400000-b1*200000-b2*600000
                   +(3-s1-s2+b1+b2)*U
                   for s1 in range(2) for s2 in range(3)
                   for b1 in range(4) for b2 in range(3)
                   if b1*200000+b2*600000 <= 100000+s1*700000+s2*400000)
        self.assertGreaterEqual(upper(r), best)

    def test_invalid_uncovered_or_float_inputs_reject(self):
        for cash, held, buys, sells in [(0.0, 1, [], []), (0, 1, [], [level(500000, 2)]),
                                        (0, 1, [level(0, 1)], []),
                                        (0, 1, [level(500000, True)], [])]:
            with self.assertRaises(ValueError):
                static_upper_bound(cash, held, buys, sells, allow_sales=True)


if __name__ == "__main__":
    unittest.main()
