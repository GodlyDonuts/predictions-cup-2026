import unittest
from collections import Counter
from .test_actions import environment
from .contract import OrderSpec,UNIT
from .experiment import Experiment
from .economics import GrowthLedger
from .passive import StrictPriorityFlow
from moloch.learning_v2.liquidity import Book


def world(terminal_book=True):
    e=environment();e.end=5000;e.exchanges=(*e.exchanges,'860')
    e.passive=StrictPriorityFlow(e,'cup')
    e._execute('ORDER',[OrderSpec('860','YES','BUY',5,500000)])
    if terminal_book:e.clock.schedule(2000,10,'book',Book('860',2000,2000,((400000,100),),((500000,100),),'a'*64))
    return e


def print_before_book(e):
    e.advance(1000)
    e.passive.trade(1000,dict(tournament_id='cup',exchange_id='860',market_id='m',match_id='fixture',
        executed_at_ms=1000,yes_price_micros=450000,quantity=5))


class FixtureStream:
    def __init__(self,e):
        self.env=e;self.decisions=0;self.action_counts=Counter();self.status_counts=Counter();self.entity_counts=Counter()

    def run_chunk(self,*args,**kwargs):
        if self.decisions==0:print_before_book(self.env)
        elif self.decisions==1:self.env.advance(2000)
        else:self.env.advance(self.env.end)
        self.decisions+=1


class EvaluationValuationTests(unittest.TestCase):
    def test_explicit_resume_order_preserves_every_member_and_later_cycles(self):
        x=Experiment.__new__(Experiment);x.config={'first_cycle_member_order':[2,0,1]}
        self.assertEqual(x.member_order(0,3),[2,0,1]);self.assertEqual(x.member_order(1,3),[0,1,2])
        for invalid in [[0,0,1],[0,1],[0,1,True]]:
            x.config['first_cycle_member_order']=invalid
            with self.assertRaisesRegex(ValueError,'EVERY_MEMBER_ONCE'):x.member_order(0,3)

    def experiment(self,terminal_book=True):
        x=Experiment.__new__(Experiment);x.seed=14;x.evolution_bounds=(1,5000);x.search_config={}
        x.development=True;x.config={'development_last_observed_marks':False};x.stream=lambda model,e:FixtureStream(e)
        x.world=lambda *args:world(terminal_book)
        return x

    def test_legal_fill_can_precede_first_midpoint_but_terminal_score_is_available(self):
        e=world();ledger=GrowthLedger(e,development=True);print_before_book(e)
        self.assertEqual(e.held('860','YES'),5);self.assertNotIn('860',e.books)
        with self.assertRaisesRegex(ValueError,'INCOMPLETE_SIG_WEALTH_MARK:860'):ledger.transition()
        # The actual selection runner can finish, without an interim mark or
        # any substitution of the fill limit/print/cost for the absent midpoint.
        result=self.experiment().evaluate(None,{'sha256':'a'*64})
        self.assertEqual(len(result['cases']),2)
        for case in result['cases']:
            self.assertEqual(case['fill_count'],1)
            self.assertAlmostEqual(case['marked_roi'],-.00025)
            self.assertEqual(case['valuation_observation_schedule'],'start_daily_boundaries_and_terminal')

    def test_missing_required_terminal_mark_still_fails(self):
        with self.assertRaisesRegex(ValueError,'INCOMPLETE_SIG_WEALTH_MARK:860'):
            self.experiment(False).evaluate(None,{'sha256':'a'*64})
