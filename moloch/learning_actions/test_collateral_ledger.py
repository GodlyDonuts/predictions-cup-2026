"""Synthetic ALL cash/debt invariants, never calibrated exchange evidence."""
import unittest
from .collateral_ledger import Component,Fill,CollateralLedger
from .contract import UNIT


def component(name='a',exchanges=('x','y'),eligible=True,scope='fixture-cup'):
    return Component(name,exchanges,((1,0),(0,1)),('fixture-all-'+name,),scope,'a'*64,eligible)


class CollateralLedgerTests(unittest.TestCase):
    def ledger(self,**overrides):
        args=dict(cash=10*UNIT,positions={'x':10,'y':9},components=[component()],scope='fixture-cup')
        args.update(overrides);return CollateralLedger(**args)

    def test_floor_uses_allowed_joint_outcomes_and_both_signed_sides_not_marks(self):
        c=component();self.assertEqual(c.floor({'x':10,'y':9}),9*UNIT)
        self.assertEqual(c.floor({'x':-8,'y':-11}),8*UNIT)
        self.assertEqual(c.floor({'x':10,'y':-9}),0)
        independent=Component('b',('x','y'),((0,0),(0,1),(1,0),(1,1)),('independent',),'fixture-cup','b'*64)
        self.assertEqual(independent.floor({'x':10,'y':9}),0)

    def test_own_group_draws_before_cash_without_creating_equity_or_profit(self):
        l=self.ledger();marks={'x':500000,'y':400000};before=l.equity(marks)
        r=l.apply('fill-own',Fill('y','YES','BUY',1,400000))
        self.assertEqual(l.cash,10*UNIT);self.assertEqual(l.debt,{'a':400000})
        self.assertEqual(r['effectiveEntryCost'],0);self.assertEqual(r['collateralSavings'],400000)
        self.assertEqual(r['fullNotionalCost'],400000);self.assertEqual(l.equity(marks),before)
        self.assertEqual(r['guaranteedPayoutFloorAfter'],10*UNIT)

    def test_cash_precedes_other_groups_and_negative_cash_is_not_spendable(self):
        l=self.ledger(cash=UNIT)
        r=l.apply('fill-outside',Fill('z','YES','BUY',6,500000),other_draw_order=('a',))
        self.assertEqual(l.cash,0);self.assertEqual(r['componentDraws'],(('a',2*UNIT),))
        self.assertIsNone(r['componentId']);self.assertEqual(r['effectiveEntryCost'],UNIT)
        negative=self.ledger(cash=-5*UNIT)
        self.assertEqual(negative.buying_power(),9*UNIT)
        r=negative.apply('negative-cash-buy',Fill('z','YES','BUY',2,500000),other_draw_order=('a',))
        self.assertEqual(negative.cash,-5*UNIT);self.assertEqual(r['collateralSavings'],UNIT)

    def test_floor_reduction_repays_own_debt_and_may_leave_negative_cash(self):
        l=self.ledger(cash=0);marks={'x':500000,'y':500000,'z':500000};before=l.equity(marks)
        l.apply('borrow-other',Fill('z','YES','BUY',18,500000),other_draw_order=('a',))
        self.assertEqual(l.debt['a'],9*UNIT);self.assertEqual(l.equity(marks),before)
        r=l.apply('reduce-floor',Fill('y','YES','SELL',9,500000))
        self.assertEqual(r['collateralRepayment'],9*UNIT);self.assertEqual(l.debt['a'],0)
        self.assertEqual(l.cash,-4500000);self.assertEqual(l.equity(marks),before)
        with self.assertRaisesRegex(ValueError,'CANNOT_BE_FUNDED'):l.apply('unfunded',Fill('z','YES','BUY',1,500000))

    def test_complement_netting_redemption_and_debt_repayment_preserve_equity(self):
        l=self.ledger(outstanding={'a':9*UNIT});marks={'x':500000,'y':500000};before=l.equity(marks)
        r=l.apply('buy-complement',Fill('x','NO','BUY',10,500000))
        self.assertEqual(r['redemptionCredit'],10*UNIT);self.assertEqual(r['collateralRepayment'],9*UNIT)
        self.assertEqual(l.cash,6*UNIT);self.assertEqual(l.positions['x'],0);self.assertEqual(l.equity(marks),before)

    def test_uncovered_sell_is_full_complement_buy_and_still_accounts_redemption(self):
        l=self.ledger(positions={'u':2},components=[]);before=l.equity({'u':600000})
        r=l.apply('oversell',Fill('u','YES','SELL',5,600000))
        self.assertEqual(r['canonical'],Fill('u','NO','BUY',5,400000))
        self.assertEqual(r['redemptionCredit'],2*UNIT);self.assertEqual(l.positions['u'],-3)
        self.assertEqual(l.equity({'u':600000}),before)

    def test_unknown_other_group_priority_and_insufficient_funding_are_atomic(self):
        l=self.ledger(cash=0);original=(l.cash,dict(l.positions),dict(l.debt),dict(l.events))
        with self.assertRaisesRegex(ValueError,'DRAW_ORDER_UNCALIBRATED'):l.apply('unknown',Fill('z','YES','BUY',1,500000))
        self.assertEqual((l.cash,l.positions,l.debt,l.events),original)
        with self.assertRaisesRegex(ValueError,'CANNOT_BE_FUNDED'):l.apply('too-large',Fill('z','YES','BUY',100,500000),other_draw_order=('a',))
        self.assertEqual((l.cash,l.positions,l.debt,l.events),original)

    def test_idempotency_returns_original_without_redrawing_or_mutable_result_alias(self):
        l=self.ledger();f=Fill('y','YES','BUY',1,400000);r=l.apply('same',f)
        state=(l.cash,dict(l.positions),dict(l.debt));r['collateralSavings']=999
        again=l.apply('same',f);self.assertEqual(again['collateralSavings'],400000)
        self.assertEqual((l.cash,l.positions,l.debt),state)
        with self.assertRaisesRegex(ValueError,'IDENTITY_CONFLICT'):l.apply('same',Fill('y','YES','BUY',2,400000))

    def test_scope_overlap_bad_money_and_ineligibility_do_not_create_advance(self):
        with self.assertRaisesRegex(ValueError,'DOUBLE_COUNT'):self.ledger(components=[component(),component('b')])
        with self.assertRaisesRegex(ValueError,'ONE_VERIFIED'):self.ledger(components=[component(scope='other')])
        with self.assertRaisesRegex(ValueError,'INTEGER_CASH'):self.ledger(cash=1.)
        with self.assertRaisesRegex(ValueError,'INTEGER_NET'):self.ledger(positions={'x':True})
        l=self.ledger(cash=0,components=[component(eligible=False)])
        self.assertEqual(l.buying_power(),0)
        with self.assertRaisesRegex(ValueError,'CANNOT_BE_FUNDED'):l.apply('ineligible',Fill('y','YES','BUY',1,400000))

    def test_debt_stays_a_liability_when_component_has_no_lending_headroom(self):
        l=self.ledger(cash=-UNIT,outstanding={'a':10*UNIT})
        self.assertEqual(l.available(),{'a':0});self.assertEqual(l.buying_power(),0)
        self.assertEqual(l.equity({'x':500000,'y':500000}),-1500000)

    def test_zero_component_accepted_fill_accounting_matches_existing_fifo_api_episode(self):
        import random
        from .test_actions import environment
        from .contract import OrderSpec
        env=environment(20*UNIT);ledger=CollateralLedger(cash=env.cash,positions={},components=[],scope='fixture-cup')
        rng=random.Random(991882);seen=0
        for i in range(100):
            spec=OrderSpec('x',rng.choice(('YES','NO')),rng.choice(('BUY','SELL')),rng.randint(1,15),
                rng.choice((300000,450000,550000,700000)))
            env.dispatch('ORDER',[spec],idempotency_key='fixture-%d'%i);env.advance(env.now+3000)
            for f in env.api_fills[seen:]:
                ledger.apply('existing-fill-%d'%seen,Fill(f['exchange'],f['side'],f['action'],f['quantity'],f['price']))
                seen+=1
            self.assertEqual(ledger.cash,env.cash)
            self.assertEqual({k:v for k,v in ledger.positions.items()if v},
                {ex:env.held(ex,'YES')-env.held(ex,'NO')for ex in env.exchanges[1:]if env.held(ex,'YES')or env.held(ex,'NO')})
            self.assertEqual(sum(ledger.debt.values()),0)
        self.assertGreater(seen,0)

    def test_random_fill_paths_conserve_equity_at_execution_marks_and_failed_fills_do_not_mutate(self):
        import random
        rng=random.Random(491103);l=self.ledger(cash=20*UNIT,components=[component(),component('b',('v','w'))],
            positions={'x':10,'y':9,'v':12,'w':8})
        accepted=0;rejected=0
        for i in range(500):
            ex=rng.choice(('x','y','v','w','unlinked'));side=rng.choice(('YES','NO'));action=rng.choice(('BUY','SELL'))
            price=rng.randint(1,199)*5000;f=Fill(ex,side,action,rng.randint(1,25),price)
            marks={key:500000 for key in set(l.positions)|{ex}};marks[ex]=price if side=='YES'else UNIT-price
            equity=l.equity(marks);before=(l.cash,dict(l.positions),dict(l.debt),dict(l.events))
            order=tuple(sorted(set(l.components)-{l.owner.get(ex)}))
            try:r=l.apply('random-%d'%i,f,other_draw_order=order)
            except ValueError as error:
                self.assertEqual(str(error),'ACCEPTED_FILL_CANNOT_BE_FUNDED');rejected+=1
                self.assertEqual((l.cash,l.positions,l.debt,l.events),before);continue
            accepted+=1;self.assertEqual(l.equity(marks),equity)
            self.assertEqual(l.cash-before[0],r['cashDelta'])
            self.assertEqual(sum(l.debt.values())-sum(before[2].values()),r['collateralSavings']-r['collateralRepayment'])
        self.assertGreater(accepted,0);self.assertGreater(rejected,0)
