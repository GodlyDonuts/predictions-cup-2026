import unittest
from .account import UNIT,Limits,Target
from .clock import BudgetConfig
from .episodes import Episode,CONTRACT_FEATURES,GLOBAL_FEATURES
from .reference_env import Mechanics
from .liquidity import Book


class EconomicCapacityObservation(unittest.TestCase):
    def fixture(self):
        m=Mechanics(BudgetConfig(100,600,24,2500,'a'*64),Limits(10000,2000*UNIT,100*UNIT),100,200,100,10000,20000,1,'Kalshi')
        e=Episode(m,0,1000*UNIT,('__GLOBAL_WAIT__','x'),11000*UNIT,0,100000)
        e.add_book(Book('x',0,None,((400000,100),),((450000,100),),'b'*64))
        e.add_reference('x',500000,0,'c'*64,source_kind='Kalshi');e.advance(0)
        return e

    def test_one_wait_identity_preserves_stale_market_information(self):
        e=self.fixture();a=e.observe()
        self.assertTrue(a['masks']['market'][0]);self.assertTrue(a['masks']['market'][1])
        self.assertFalse(a['masks']['family'][1,0]);self.assertTrue(a['masks']['family'][0,0])
        e.books['x'].invalidate();b=e.observe()
        self.assertTrue(b['present'][1]);self.assertAlmostEqual(float(b['contracts'][1,0]),.4)
        self.assertFalse(b['masks']['market'][1]);self.assertTrue(b['masks']['market'][0])

    def test_stale_public_book_does_not_hide_a_real_resting_cancel(self):
        e=self.fixture();e.submit(Target('x','YES',25000,UNIT,300000,'BUY'),60000);e.advance(10000)
        self.assertTrue(any(o.phase=='resting'for o in e.orders.values()))
        e.books['x'].invalidate();a=e.observe()
        self.assertTrue(a['masks']['market'][1]);self.assertTrue(a['masks']['family'][1,3])
        self.assertFalse(a['masks']['family'][1,1]);self.assertFalse(a['masks']['family'][1,0])
        cash=e.cash;self.assertTrue(e.act({'family':3,'market':1}));self.assertEqual(e.cash,cash)

    def test_depth_cache_revalues_with_actual_cash_and_replaces_on_new_book(self):
        e=self.fixture();a=e.observe();self.assertAlmostEqual(float(a['contracts'][1,4]),.04)
        e.cash=2000*UNIT;b=e.observe();self.assertAlmostEqual(float(b['contracts'][1,4]),.02)
        e.add_book(Book('x',1,None,((400000,50),),((450000,100),),'b'*64));e.advance(1)
        c=e.observe();self.assertAlmostEqual(float(c['contracts'][1,4]),.01)

    def test_mutable_depth_input_does_not_use_immutable_book_cache(self):
        e=self.fixture();book=Book('x',1,None,[(400000,100)],[(450000,100)],'b'*64)
        e.add_book(book);e.advance(1);a=e.observe();book.bids[0]=(400000,50)
        b=e.observe();self.assertAlmostEqual(float(a['contracts'][1,4]),.04);self.assertAlmostEqual(float(b['contracts'][1,4]),.02)

    def test_explicit_headroom_and_pending_side_exposure_change_with_real_encumbrance(self):
        m=Mechanics(BudgetConfig(100,600,24,2500,'a'*64),Limits(10000,2000*UNIT,100*UNIT),100,200,100,10000,20000,1,'Kalshi')
        e=Episode(m,0,1000*UNIT,('__GLOBAL_WAIT__','x'),11000*UNIT,0,10000)
        e.add_book(Book('x',0,None,((400000,100),),((450000,100),),'b'*64))
        e.add_reference('x',500000,0,'c'*64,source_kind='Kalshi');e.advance(0)
        a=e.observe();self.assertEqual(a['contracts'].shape,(2,CONTRACT_FEATURES));self.assertEqual(a['global_state'].shape,(GLOBAL_FEATURES,))
        self.assertAlmostEqual(float(a['contracts'][1,20]),.1)
        self.assertAlmostEqual(float(a['global_state'][12]),2.)
        e.submit(Target('x','YES',25000,UNIT,450000,'BUY'),5000)
        b=e.observe();self.assertAlmostEqual(float(b['contracts'][1,20]),.0775,places=6)
        self.assertAlmostEqual(float(b['contracts'][1,22]),.05,places=6)
        self.assertAlmostEqual(float(b['global_state'][12]),1.9775,places=6)


if __name__=='__main__':unittest.main()
