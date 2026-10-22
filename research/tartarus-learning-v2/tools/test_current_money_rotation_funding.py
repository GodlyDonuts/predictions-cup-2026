"""Independent cash-versus-cost-basis cases for partial funding sales."""
from decimal import Decimal
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('rotations', Path(__file__).with_name('scan-current-money-rotations.py'))
rotations = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rotations)
units = lambda x: int(Decimal(x)*1000000)


class FundingTests(unittest.TestCase):
    lots = [dict(quantity=3, price='0.2'), dict(quantity=4, price='0.4')]

    def choose(self, cash='0', reserved='0', owned='2.2', cap='2.2', cost='1'):
        return rotations.minimum_funding_sale(self.lots, 7, units('0.5'), units(cash),
            units(reserved), units(owned), units(cap), units(cost), units)

    def test_original_cost_limit_can_require_more_than_cash_funding(self):
        # Two shares raise1 cash, but four release the required1 original cost.
        self.assertEqual(self.choose(), 4)
        self.assertEqual(rotations.fifo_basis(self.lots, 4, units), units('1'))

    def test_reserved_cash_is_not_available_to_the_replacement(self):
        self.assertEqual(self.choose(cash='1', reserved='0.5', cap='3', cost='1.5'), 5)

    def test_cash_funding_can_bind_before_released_basis(self):
        self.assertEqual(self.choose(cap='100', cost='1.1'), 3)

    def test_no_sale_needed_is_distinct_from_insufficient_funding(self):
        self.assertEqual(self.choose(cash='5', cap='100'), 0)
        self.assertIsNone(self.choose(cap='100', cost='4'))

    def test_sale_cannot_create_nonexistent_fifo_basis(self):
        with self.assertRaisesRegex(AssertionError, 'SALE_EXCEEDS_FIFO'):
            rotations.fifo_basis(self.lots, 8, units)


if __name__ == '__main__':
    unittest.main()
