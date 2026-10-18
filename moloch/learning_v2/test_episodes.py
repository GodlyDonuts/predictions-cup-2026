import unittest
from .account import UNIT,Limits,Target
from .clock import BudgetConfig
from .episodes import Episode,CONTRACT_FEATURES,GLOBAL_FEATURES
from .reference_env import Mechanics
from .liquidity import Book


class EconomicCapacityObservation(unittest.TestCase):
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
