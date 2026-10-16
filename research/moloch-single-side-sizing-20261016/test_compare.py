import unittest
from compare import compare, fee


class Sizing(unittest.TestCase):
    def test_supplied_8086(self):
        x=compare(40000,45000,33171,487)
        self.assertEqual(x['micropriceMicros'],44927)
        self.assertEqual(x['pairedSpread']['drafts']['YES']['quantity'],487)
        self.assertEqual(x['microprice']['drafts']['YES']['quantity'],10000)
        self.assertEqual(x['pairedSpread']['reservedMicros'],484565000)
        self.assertEqual(list(x['microprice']['drafts']),['YES'])

    def test_supplied_8126(self):
        x=compare(695000,700000,4451,38)
        self.assertEqual(x['micropriceMicros'],699957)
        self.assertEqual(x['pairedSpread']['drafts']['YES']['quantity'],38)
        self.assertEqual(x['microprice']['drafts']['YES']['quantity'],4451)
        self.assertEqual(x['microprice']['reservedMicros'],3093445000)

    def test_complement_no_uses_yes_floor(self):
        x=compare(300000,305000,1,999)
        self.assertEqual(x['sideEdgeMicros']['NO'],4995)
        self.assertEqual(list(x['microprice']['drafts']),['NO'])

    def test_both_pass_not_same_sizes(self):
        x=compare(400000,405000,1000,2000)
        self.assertTrue(all(x['sidePasses'].values()))
        self.assertEqual(x['microprice']['drafts']['NO']['quantity'],2000)
        self.assertEqual(x['pairedSpread']['drafts']['NO']['quantity'],1000)

    def test_fee_can_break_whole_pair_equivalence(self):
        x=compare(40000,45000,33171,487,fee_share=2000)
        self.assertFalse(x['pairSpreadPasses'])
        self.assertEqual(x['pairedSpread']['drafts'],{})
        self.assertIn('YES',x['microprice']['drafts'])

    def test_cash_total_market_all_preserved(self):
        for field in ['cash','total_room','market_room']:
            x=compare(695000,700000,4451,38,**{field:2000000})
            for mode in ['pairedSpread','microprice']:
                self.assertLessEqual(x[mode]['reservedMicros'],2000000)

    def test_depth_and_quantity_caps(self):
        x=compare(40000,45000,33171,487,quantity=123,depth_bps=5000)
        self.assertEqual(x['microprice']['drafts']['YES']['quantity'],123)
        self.assertLessEqual(x['pairedSpread']['drafts']['NO']['quantity'],487//2)

    def test_one_share_fee_rounding_conservative(self):
        self.assertEqual(fee(40000,1,3),7)
        x=compare(40000,45000,33171,487,fee_bps=1)
        for mode in ['pairedSpread','microprice']:
            for q in x[mode]['drafts'].values():
                self.assertEqual(q['reservedMicros'],q['quantity']*(q['limitMicros']+fee(q['limitMicros'],1)))


if __name__=='__main__':unittest.main()
