from dataclasses import replace
import unittest
from .account import Limits, Target, UNIT as U
from .clock import BudgetConfig
from .liquidity import Book
from .reference_env import Mechanics, ReferenceEnv


def experiment(**changes):
    m = Mechanics(BudgetConfig(100, 600, 24, 2500, 'a'*64), Limits(10_000, 100_000*U, 10_000*U),
                  100, 200, 100, 10000, 20000, 1)
    return replace(m, **changes)


class EventAccount(unittest.TestCase):
    def setUp(self):
        self.b = Book('x', 0, None, ((400000, 100),), ((450000, 100),), 'a'*64)
        self.e = ReferenceEnv(experiment(), 0, 1000*U)
        self.e.add_book(self.b)
        self.e.add_reference('x', 400000, 0, 'b'*64)
        self.e.advance(0)
        self.t = Target('x', 'YES', 20000, U, 450000, 'BUY')

    def test_no_observed_cash_or_inventory_change_until_fill_receipt(self):
        p = self.e.submit(self.t, 5000)
        self.assertEqual(p.admitted_quantity, 50)
        self.e.advance(999)
        self.assertEqual(self.e.cash, 1000*U)
        self.assertEqual(self.e.account().quantities, {})
        self.assertEqual(self.e.account().reserved_cash, 22_500_000)
        self.e.advance(1000)
        self.assertEqual(self.e.cash, 977_500_000)
        self.assertEqual(self.e.account().quantities, {('x', 'YES'):50})
        self.assertEqual(self.e.account().reserved_cash, 0)
        self.assertEqual(self.e.fill_events[0]['filled_at_ms'], 900)
        self.assertEqual(self.e.fill_events[0]['known_at_ms'], 1000)

    def test_price_at_arrival_not_decision_determines_fill(self):
        self.e.submit(self.t, 1500)
        self.e.add_book(replace(self.b, available_at_ms=850, asks=((460000, 100),)))
        self.e.advance(1000)
        self.assertEqual(self.e.fill_events, [])
        self.e.advance(3799)
        self.assertEqual(self.e.account().reserved_cash, 22_500_000)
        self.e.advance(3800)
        self.assertEqual(self.e.account().reserved_cash, 0)
        self.assertEqual(list(self.e.budget.writes), [700,3500])

    def test_partial_fill_leaves_real_reservation_until_cancel_receipt(self):
        self.e.books['x'].update(replace(self.b, asks=((450000, 20),)))
        self.e.submit(self.t, 1500)
        self.e.advance(1000)
        self.assertEqual(self.e.account().quantities[('x', 'YES')], 20)
        self.assertEqual(self.e.account().reserved_cash, 13_500_000)
        self.e.advance(3800)
        self.assertEqual(self.e.account().reserved_cash, 0)
        self.assertEqual(self.e.cash, 991*U)

    def test_fill_before_cancel_effective_cannot_be_erased(self):
        self.e.books['x'].update(replace(self.b, asks=((450000, 20),)))
        self.e.submit(self.t, 5000)
        self.e.advance(1100)
        self.assertTrue(self.e.cancel(1))
        self.e.add_book(replace(self.b, available_at_ms=3600, asks=((450000, 80),)))
        self.e.advance(3800)
        self.assertEqual(sum(x['quantity'] for x in self.e.fill_events), 50)
        self.assertEqual(self.e.cash, 977_500_000)
        self.assertEqual(self.e.account().reserved_cash, 0)

    def test_pending_sale_proceeds_and_fifo_are_not_invented(self):
        e = ReferenceEnv(experiment(), 0, 0, {('x', 'YES'):[[40,300000],[60,350000]]})
        e.add_book(self.b); e.add_reference('x',400000,0,'b'*64);e.advance(0)
        e.submit(Target('x','YES',0,U,400000,'SELL'),5000)
        e.advance(999)
        self.assertEqual(e.cash, 0)
        self.assertEqual(e.account().pending_sells[('x','YES')], 100)
        e.advance(1000)
        self.assertEqual(e.cash, 40*U)
        self.assertEqual(e.realized, 7*U)
        self.assertEqual(e.account().owned_cost, 0)

    def test_opposite_buy_nets_pairs_at_receipt_with_original_fifo(self):
        e = ReferenceEnv(experiment(),0,1000*U,{('x','YES'):[[40,300000],[60,350000]]})
        e.add_book(self.b);e.add_reference('x',400000,0,'b'*64);e.advance(0)
        p = e.submit(Target('x','NO',0,U,600000,'BUY'),5000)
        self.assertEqual(p.admitted_quantity,100)
        e.advance(999)
        self.assertEqual(e.cash,1000*U)
        self.assertEqual(e.account().quantities,{('x','YES'):100})
        e.advance(1000)
        self.assertEqual(e.cash,1040*U)
        self.assertEqual(e.realized,7*U)
        self.assertEqual(e.account().quantities,{})
        self.assertEqual(e.account().owned_cost,0)

    def test_buy_fees_are_expensed_not_hidden_in_owned_basis(self):
        e = ReferenceEnv(experiment(limits=Limits(10000,100000*U,10000*U,fee_per_share=1000)),0,1000*U)
        e.add_book(self.b);e.add_reference('x',400000,0,'b'*64);e.advance(0)
        e.submit(self.t,5000);e.advance(1000)
        self.assertEqual(e.fees,50000);self.assertEqual(e.realized,-50000)
        self.assertEqual(e.account().owned_cost,22500000)

    def test_same_target_after_fill_does_not_repeat_purchase(self):
        self.e.submit(self.t,5000);self.e.advance(1000)
        self.assertEqual(self.e.submit(self.t,5000).admitted_quantity, 0)

    def test_unknown_reference_is_unavailable_not_acquisition_cost_fallback(self):
        self.e.submit(self.t,5000);self.e.advance(1000)
        self.e.advance(20001)
        self.assertIsNone(self.e.marked_wealth())

    def test_book_gap_after_preflight_is_not_sent(self):
        e = ReferenceEnv(experiment(read_latency_ms=10_001),0,1000*U)
        e.add_book(self.b);e.add_reference('x',400000,0,'b'*64);e.advance(0)
        e.submit(self.t,5000);e.advance(10601)
        self.assertEqual(e.execution_events[0]['kind'],'not_sent')
        self.assertEqual(e.account().reserved_cash,0)
        self.assertEqual(list(e.budget.writes),[])

    def test_preflight_cannot_submit_a_draft_after_its_local_patience_deadline(self):
        e=ReferenceEnv(experiment(preflight_reads=16),0,1000*U)
        e.add_book(self.b);e.add_reference('x',400000,0,'b'*64);e.advance(0)
        e.submit(self.t,1000);e.advance(10000)
        self.assertEqual(e.fill_events,[]);self.assertEqual(list(e.budget.writes),[])
        self.assertEqual(e.account().reserved_cash,0)
        self.assertEqual(e.execution_events[-1]['reason'],'PATIENCE_EXPIRED_BEFORE_SEND')

    def test_previously_admitted_request_may_fill_after_local_patience_in_real_cancel_race(self):
        self.e.submit(self.t,800);self.e.advance(1000)
        self.assertEqual(sum(f['quantity']for f in self.e.fill_events),50)
        self.assertEqual(list(self.e.budget.writes),[700])

    def test_two_environments_never_share_consumed_liquidity(self):
        other = ReferenceEnv(experiment(),0,1000*U)
        other.add_book(self.b);other.add_reference('x',400000,0,'b'*64);other.advance(0)
        self.e.submit(self.t,5000);self.e.advance(1000)
        self.assertEqual(other.books['x'].depleted,{})

    def test_cannot_double_cancel_or_cancel_unsubmitted_order(self):
        self.e.submit(self.t,5000)
        self.assertFalse(self.e.cancel(1))
        self.e.advance(1000)
        self.assertFalse(self.e.cancel(1))  # known fully filled; no replacement


if __name__ == '__main__':
    unittest.main()
