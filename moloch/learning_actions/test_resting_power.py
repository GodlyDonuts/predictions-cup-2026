"""Synthetic funding/quote lifetime checks, not calibrated SIG priority."""
from copy import deepcopy
import unittest
from .test_component_execution import world as base_world,read
from .test_collateral_ledger import component
from .component_execution import configure
from .contract import OrderSpec,UNIT
from .resting_power import MODES,validate,reconcile,reported_quantity
from .episode_tape import financial_digest


def world(cash=10*UNIT,linked=False,mode=MODES[0]):
    e=base_world();e.cash=cash;e.known_cash=cash
    configure(e,[component()]if linked else(),scope='fixture-cup',other_draw_order=('a',)if linked else None)
    e.resting_buy_funding=mode;return e


def place(e,ex,n,price,side='YES',action='BUY',expiration=None):
    return e._execute('ORDER',[OrderSpec(ex,side,action,n,price,expiration)])[0]


class RestingPowerTests(unittest.TestCase):
    def test_fill_spend_shrinks_unaffordable_order_without_free_actor_knowledge(self):
        e=world();q=place(e,'y',20,400000);read(e,'owned_orders')
        before=deepcopy(e.known_orders);cash=e.known_cash
        place(e,'x',10,450000)
        self.assertEqual(e.cash,5500000);self.assertEqual(q.remaining,13)
        self.assertEqual(e.order_history.records[q.id].order.remaining,13)
        self.assertEqual(e.known_orders,before);self.assertEqual(e.known_cash,cash)
        read(e,'owned_orders');self.assertEqual(e.known_orders[q.id].remaining,13)
        self.assertEqual(e.known_cash,cash)

    def test_zero_power_cancels_and_later_profit_does_not_resurrect(self):
        e=world(450000);q=place(e,'y',90,5000);place(e,'x',1,450000)
        self.assertEqual(e.buying_power(),0);self.assertNotIn(q.id,e.api_orders)
        self.assertEqual(q.remaining,0);r=e.order_history.records[q.id]
        self.assertFalse(r.open);self.assertIsNone(r.terminal_reason)
        place(e,'x',1,400000,action='SELL');self.assertGreater(e.cash,0)
        self.assertNotIn(q.id,e.api_orders);self.assertEqual(reported_quantity(e,q),0)

    def test_cross_exchange_quotes_do_not_escrow_each_others_cash(self):
        e=world(UNIT);a=place(e,'x',200,5000);b=place(e,'y',200,5000)
        self.assertEqual((a.remaining,b.remaining),(200,200));self.assertEqual(e.cash,UNIT)
        self.assertIsNone(place(e,'x',1,5000))
        reconcile(e);self.assertEqual((a.remaining,b.remaining),(200,200))

    def test_expired_buy_does_not_commit_new_admission(self):
        from .engine import Resting
        e=world(UNIT);q=Resting(999,OrderSpec('x','YES','BUY',200,5000,e.now),200,e.now-1)
        e.api_orders[q.id]=q
        self.assertEqual(place(e,'x',200,5000).remaining,200)

    def test_post_fill_remainder_cannot_exceed_current_power_and_closed_size_is_reduced(self):
        e=world(100*UNIT)
        # This initial order can fill all100 displayed shares and rest the rest.
        q=place(e,'x',300,450000)
        self.assertEqual(e.held('x','YES'),100)
        self.assertEqual(q.remaining,e.buying_power()//450000)
        self.assertEqual(q.remaining,122)
        self.assertEqual(reported_quantity(e,q),222)
        e._execute('CANCEL',q.id)
        r=e.order_history.records[q.id];self.assertFalse(r.open)
        self.assertEqual(r.order.remaining+r.filled,222)

    def test_priority_is_explicit_and_no_resize_ever_increases_an_order(self):
        for mode,expected in ((MODES[0],(10,3)),(MODES[1],(10,10))):
            e=world(mode=mode);a=place(e,'y',10,400000);b=place(e,'y',10,400000)
            place(e,'x',10,450000)
            self.assertEqual((a.remaining,b.remaining),expected)
            place(e,'x',1,400000,action='SELL');self.assertEqual((a.remaining,b.remaining),expected)
        for invalid in (True,False,'oldest',[],0):
            with self.assertRaisesRegex(ValueError,'EXPLICIT_RESTING'):validate(invalid)

    def test_component_debt_finalizes_before_quote_reconciliation(self):
        e=world(linked=True);place(e,'x',10,450000);place(e,'y',10,450000)
        self.assertGreater(e.component_ledger.debt['a'],0)
        q=place(e,'x',2000,5000)
        place(e,'y',10,400000,action='SELL')
        self.assertEqual(e.cash,e.component_ledger.cash)
        self.assertLessEqual(q.remaining*5000,e.buying_power())
        self.assertEqual(e.component_ledger.debt['a'],0)

    def test_atomic_rollback_restores_automatically_resized_quotes_and_history(self):
        e=world();place(e,'y',20,400000);before=financial_digest(e)
        with self.assertRaisesRegex(ValueError,'ALL_OR_NONE'):
            e._execute('ALL_OR_NONE',[OrderSpec('x','YES','BUY',10,450000),OrderSpec('x','YES','BUY',10,5000)])
        self.assertEqual(financial_digest(e),before)
        self.assertEqual(e.api_orders[1].remaining,20)

    def test_inactive_profile_preserves_old_physical_quote_behavior(self):
        e=world(mode=None);q=place(e,'y',20,400000);place(e,'x',10,450000)
        self.assertEqual(q.remaining,20)

    def test_fill_preview_cannot_continue_after_its_resting_order_was_resized_to_zero(self):
        from dataclasses import replace
        e=world();q=place(e,'x',10,400000)
        e.books['x'].book=replace(e.books['x'].book,bids=((5000,100),),asks=((100000,1),(150000,9)))
        original=e._apply_fill
        def fill_and_shock(order,price,n,**kwargs):
            original(order,price,n,**kwargs)
            # Explicit synthetic external funding shock between preview levels.
            # It exercises stale-preview safety, not a historical cash event.
            e.cash=0;e.component_ledger.cash=0;e._funding_changed()
        e._apply_fill=fill_and_shock
        e._match_api(q)
        self.assertEqual(sum(r['quantity']for r in e.api_fills),1)
        self.assertEqual(q.remaining,0);self.assertNotIn(q.id,e.api_orders)


if __name__=='__main__':unittest.main()
