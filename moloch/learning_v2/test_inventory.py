import unittest
from moloch.learning_v2.account import UNIT,Limits
from moloch.learning_v2.clock import BudgetConfig
from moloch.learning_v2.reference_env import ReferenceEnv,Mechanics
from moloch.learning_v2.liquidity import Book
from .inventory_curriculum import seed_inventory


class CausalStartingInventory(unittest.TestCase):
    def test_depth_capacity_selection_does_not_spend_market_slots_on_tiny_books_first(self):
        m=Mechanics(BudgetConfig(100,600,24,2500,'a'*64),Limits(1000,2000*UNIT,600*UNIT,maximum_fresh_markets=2),
                    600,600,300,10000,20000,16,'Kalshi')
        e=ReferenceEnv(m,0,1000*UNIT)
        for i in range(16):
            ex=str(i);qty=5000 if i<2 else 1
            e.add_book(Book(ex,0,None,((400000,qty),),((600000,qty),),'b'*64))
            e.add_reference(ex,500000,0,'c'*64,source_kind='Kalshi')
        e.advance(0);r=seed_inventory(e,900000,33,selection='current_depth_capacity')
        self.assertEqual(r['owned_cost_micros'],900*UNIT)
        self.assertEqual({x['exchange_id']for x in r['initial_lots']},{'0','1'})
        self.assertEqual(e.cash,100*UNIT)

    def test_initial_inventory_is_funded_with_original_basis_and_does_not_fake_fills(self):
        m=Mechanics(BudgetConfig(100,600,24,2500,'a'*64),Limits(1000,2000*UNIT,600*UNIT,maximum_fresh_markets=2),
                    600,600,300,10000,20000,16,'Kalshi')
        e=ReferenceEnv(m,0,1000*UNIT)
        for ex in ['x','y']:
            e.add_book(Book(ex,0,None,((200000,5000),),((400000,5000),),'b'*64))
            e.add_reference(ex,500000,0,'c'*64,source_kind='Kalshi')
        e.advance(0);r=seed_inventory(e,900000,33);a=e.account()
        self.assertEqual(e.cash+a.owned_cost,1000*UNIT)
        self.assertLessEqual(a.owned_cost,900*UNIT)
        self.assertLessEqual(max(a.market_cost.values()),600*UNIT)
        self.assertGreater(sum(a.quantities.values()),m.limits.maximum_order_quantity)
        self.assertEqual(e.fill_events,[])
        self.assertTrue(all(not q.depleted for q in e.books.values()))
        self.assertEqual(r['initial_marked_wealth_micros'],e.marked_wealth())
        self.assertFalse(r['initial_mark_change_is_profit'])
        with self.assertRaisesRegex(ValueError,'INITIAL_STATE'):seed_inventory(e,900000,33)

    def test_entry_basis_mark_difference_is_not_credited_as_episode_return(self):
        from .episodes import Episode
        from .policy import RecurrentPolicy
        from .run_experiment import rollout
        m=Mechanics(BudgetConfig(100,600,24,2500,'a'*64),Limits(1000,2000*UNIT,600*UNIT),
                    600,600,300,1000000,1000000,16,'Kalshi')
        class StaticTape:
            def episode(self,mechanics,start,end,**kw):
                e=Episode(mechanics,0,kw['initial_cash_micros'],('__GLOBAL_WAIT__','x'),kw['target_wealth_micros'],start,end)
                e.add_book(Book('x',0,None,((400000,5000),),((600000,5000),),'b'*64))
                e.add_reference('x',500000,0,'c'*64,source_kind='Kalshi');return e
        protocol={'run_start_ms':0,'fit_end_ms':1000000,'episode_length_ms':2000,'maximum_receipt_drain_ms':1000,
                  'initial_cash_micros':1000*UNIT,'planning_target_wealth_micros':11000*UNIT,
                  'recurrent_burnin_ms':1000,'decision_interval_ms':1000,'_sha256':'d'*64,
                  'initial_inventory_allocation_ppm':900000,'initial_inventory_seed':33}
        _,r=rollout(StaticTape(),m,protocol,RecurrentPolicy(24,14),1000,baseline=True)
        self.assertTrue(r['qualified_accounting']);self.assertEqual(r['reference_return'],0.)
        self.assertLess(r['initial_reference_wealth_micros'],1000*UNIT)
        self.assertEqual(r['fill_receipts'],0)
