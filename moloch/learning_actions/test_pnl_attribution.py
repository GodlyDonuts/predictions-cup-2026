"""Exact diagnostic conservation; no source/queue/ROI calibration claims."""
from dataclasses import replace
from copy import deepcopy
import unittest
from .test_component_execution import world
from .test_order_pages import world as plain_world
from .pnl_attribution import Attribution
from .contract import OrderSpec,UNIT
from .episode_tape import financial_digest,tensor_digest
from .stream import tensors


def place(e,ex='x',n=2,price=450000,side='YES',action='BUY'):
    return e._execute('ORDER',[OrderSpec(ex,side,action,n,price)])


class AttributionTests(unittest.TestCase):
    def test_crossing_impact_and_later_mark_drift_are_separate(self):
        e=world()
        with Attribution(e)as a:
            place(e);r=a.summary()
            self.assertEqual(r['fillMarkImpactTwiceMicros'],-100000)
            self.assertEqual(r['nonFillWealthResidualTwiceMicros'],0)
            e.books['x'].book=replace(e.books['x'].book,bids=((500000,100),),asks=((600000,100),))
            r=a.summary();self.assertEqual(r['markedPnlTwiceMicros'],400000)
            self.assertEqual(r['nonFillWealthResidualTwiceMicros'],500000)
            self.assertEqual(r['byExchange']['x']['nonFillMarkResidualTwiceMicros'],500000)

    def test_roundtrip_and_complement_redemption_do_not_hide_trading_cost(self):
        for complement in(False,True):
            e=world()
            with Attribution(e)as a:
                place(e)
                place(e,price=600000,side='NO')if complement else place(e,price=400000,action='SELL')
                r=a.summary();self.assertEqual(r['fillMarkImpactTwiceMicros'],-200000)
                self.assertEqual(r['markedPnlTwiceMicros'],-200000)
                self.assertEqual(r['nonFillWealthResidualTwiceMicros'],0)

    def test_component_borrowing_is_a_liability_not_marked_profit(self):
        e=world(True)
        with Attribution(e)as a:
            place(e);place(e,'y');r=a.summary()
            self.assertEqual(e.component_ledger.debt['a'],900000)
            self.assertEqual(r['markedPnlTwiceMicros'],-200000)
            self.assertEqual(r['fillMarkImpactTwiceMicros'],-200000)
            self.assertEqual(r['nonFillNetCashChangeTwiceMicros'],0)

    def test_fees_adverse_execution_and_spread_depth_impact_reconcile_exactly(self):
        e=plain_world();e.mechanics=replace(e.mechanics,adverse_execution_micros=5000,
            limits=replace(e.mechanics.limits,fee_per_share=1000))
        with Attribution(e)as a:
            place(e,price=455000);r=a.summary()
            self.assertEqual(r['fillMarkImpactTwiceMicros'],-124000)
            self.assertEqual(r['feesTwiceMicros'],4000)
            self.assertEqual(r['modeledAdverseCostTwiceMicros'],20000)
            self.assertEqual(r['grossFillMarkImpactBeforeFeesAndAdverseTwiceMicros'],-100000)

    def test_atomic_rollback_removes_only_rolled_back_fill_attribution(self):
        e=world(True)
        with Attribution(e)as a:
            place(e,n=1);before=financial_digest(e);summary=a.summary()
            with self.assertRaisesRegex(ValueError,'ALL_OR_NONE'):
                e._execute('ALL_OR_NONE',[OrderSpec('y','YES','BUY',2,450000),OrderSpec('x','YES','BUY',999,5000)])
            self.assertEqual(financial_digest(e),before);self.assertEqual(a.summary(),summary)
            self.assertEqual(len(a.records),1)

    def test_unavailable_marks_remain_unattributed_not_zero_cost(self):
        e=world();e.books['x'].book=replace(e.books['x'].book,asks=())
        with Attribution(e)as a:
            place(e,side='NO',price=600000);r=a.summary()
            self.assertFalse(r['completeAttribution']);self.assertIsNone(r['markedPnlTwiceMicros'])
            self.assertIsNone(r['fillMarkImpactTwiceMicros']);self.assertIsNone(r['nonFillWealthResidualTwiceMicros'])
            self.assertTrue(r['unavailableBoundaryMarks'])

    def test_non_fill_cash_changes_are_identified_without_calling_them_alpha(self):
        e=world()
        with Attribution(e)as a:
            e.cash+=UNIT;e.component_ledger.cash+=UNIT;r=a.summary()
            self.assertEqual(r['nonFillNetCashChangeTwiceMicros'],2*UNIT)
            self.assertEqual(r['nonFillWealthResidualTwiceMicros'],2*UNIT)

    def test_instrumentation_preserves_physical_state_and_actor_observation(self):
        def run(e):
            place(e);place(e,'y',n=3);place(e,n=1,price=400000,action='SELL')
            with self.assertRaises(ValueError):
                e._execute('ALL_OR_NONE',[OrderSpec('x','YES','BUY',1,450000),OrderSpec('y','YES','BUY',999,5000)])
        a=world(True);b=world(True);run(a)
        original=b._apply_fill
        with Attribution(b)as ledger:run(b);self.assertTrue(ledger.summary()['completeAttribution'])
        self.assertEqual(financial_digest(a),financial_digest(b))
        self.assertEqual(tensor_digest(tensors(a.observe(),'cpu')),tensor_digest(tensors(b.observe(),'cpu')))
        self.assertEqual(b._apply_fill,original)

    def test_independent_test_role_cannot_be_opened_for_diagnostics(self):
        e=world();e.role='test'
        with self.assertRaisesRegex(ValueError,'NO_INDEPENDENT_TEST'):Attribution(e)

    def test_cached_book_state_is_labeled_at_fill_not_as_model_information_use(self):
        for state in('absent','invalid','valid'):
            e=world()
            if state!='absent':e.actor_books['x']=deepcopy(e.books['x']);e.actor_books['x'].invalidated=state=='invalid'
            with Attribution(e)as a:
                place(e);r=a.summary()
                self.assertEqual(r['deliveredBookStateAtFill'][state]['fills'],1)
                self.assertEqual(r['deliveredBookStateAtFill'][state]['attributedImpactTwiceMicros'],-100000)
                self.assertIn('not_proof_of_decision_time',r['informationTiming'])
