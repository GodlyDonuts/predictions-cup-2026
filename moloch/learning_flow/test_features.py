import unittest
from .features import Flow,trade


class FlowTests(unittest.TestCase):
    def row(self,identity='a',executed=1000,received=1000,available=1000,price='0.400000',quantity=2):
        from datetime import datetime,timezone
        stamp=lambda n:datetime.fromtimestamp(n/1000,timezone.utc).isoformat()
        return trade({'type':'trade','idNamespace':'sig_engine_match','id':identity,'marketId':'m','exchangeId':'e','tournamentId':'t',
                      'executedAt':stamp(executed),'receivedAt':stamp(received),'yesPrice':price,'quantity':quantity},'a'*64,available)

    def test_original_clocks_and_exact_units(self):
        with self.assertRaisesRegex(ValueError,'INFORMATION_CLOCK'):self.row(executed=2000)
        with self.assertRaisesRegex(ValueError,'EXACT_TRADE_QUANTITY'):self.row(quantity=True)
        with self.assertRaisesRegex(ValueError,'YES_PRICE'):self.row(price='0.4000001')
        self.assertEqual(self.row()['yes_price_micros'],400000)

    def test_same_print_is_not_counted_twice_and_changed_facts_refuse(self):
        f=Flow();self.assertTrue(f.accept(1000,self.row()));self.assertFalse(f.accept(1001,self.row(received=1001,available=1001)))
        self.assertEqual(f.observe('e',1001)[4],.4)
        with self.assertRaisesRegex(ValueError,'FACTS_CHANGED'):f.accept(1002,self.row(received=1002,available=1002,price='0.500000'))

    def test_delayed_historical_print_does_not_create_current_volume(self):
        f=Flow();f.accept(100000,self.row(received=100000,available=100000))
        self.assertEqual(f.observe('e',100000),(0.,)*8)

    def test_out_of_order_execution_and_stale_windows_keep_real_time(self):
        f=Flow();f.accept(2000,self.row('a',2000,2000,2000,price='0.500000'))
        f.accept(3000,self.row('b',1000,3000,3000,price='0.400000'))
        self.assertEqual(f.observe('e',3000)[4],.5);self.assertAlmostEqual(f.observe('e',3000)[7],.1)
        self.assertEqual(f.observe('e',63001),(0.,)*8)
        with self.assertRaisesRegex(ValueError,'REGRESSION'):f.observe('e',1000)


if __name__=='__main__':unittest.main()
