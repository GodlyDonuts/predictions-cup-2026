import unittest
from analyze import gaps,sig_levels


class Arithmetic(unittest.TestCase):
    def test_yes_and_no_use_ask_not_sig_midpoint(self):
        self.assertEqual(gaps(400000,410000,600000,620000,'YES')['rawApparentGapMicros'],190000)
        self.assertEqual(gaps(400000,410000,600000,620000,'NO')['rawApparentGapMicros'],-220000)

    def test_basis_discounts_are_absolute_not_relative(self):
        self.assertEqual(gaps(400000,410000,600000,620000,'YES')['basisSensitivityNetMicros'],{'5000':185000,'20000':170000,'50000':140000,'100000':90000})

    def test_official_tick_representation_only(self):
        self.assertEqual(sig_levels([{'price':.6950000000000001,'quantity':38}],True),[(695000,38000000)])
        with self.assertRaises(ValueError):sig_levels([{'price':.696,'quantity':38}],True)

    def test_depth_not_imputed_and_sort_required(self):
        self.assertEqual(sig_levels([],True),[])
        for rows in [[{'price':.4,'quantity':0}],[{'price':.4,'quantity':2},{'price':.41,'quantity':3}]]:
            with self.assertRaises(ValueError):sig_levels(rows,True)


if __name__=='__main__':unittest.main()
