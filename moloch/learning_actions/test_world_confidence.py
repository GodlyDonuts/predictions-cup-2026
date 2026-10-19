import unittest
import tempfile
from pathlib import Path
from unittest.mock import patch
from .test_snapshot_clock import tape,book,episode
from .snapshot_clock import prepare
from .contract import OrderSpec,UNIT
from .passive import DisplayedQueueFlow
from .economics import snapshot
from .world_confidence import validate_lifetime
from moloch.learning_realism.test_replay import fixture


class WorldConfidenceTests(unittest.TestCase):
    def world(self,rows=None,lifetime=500):
        t=tape(rows if rows is not None else[book(0,100)])
        t.world_book_lifetime_ms=lifetime
        return episode(t)

    def test_expiry_does_not_invalidate_actor_or_emit_free_disconnect_or_restrict_actions(self):
        e=self.world();e.advance(200);e.request_observation('SIG',('book','x'));e.advance(499)
        self.assertIn('x',e.actor_books);self.assertFalse(e.actor_books['x'].invalidated)
        old=e.observe();revision=e.observation_revision;read_count=len(e.budget.reads)
        e.advance(501)
        self.assertTrue(e.books['x'].invalidated);self.assertFalse(e.actor_books['x'].invalidated)
        self.assertEqual(e.world_confidence_expiries,1);self.assertEqual(e.observation_revision,revision)
        self.assertEqual(len(e.budget.reads),read_count);self.assertFalse(e.budget.writes)
        new=e.observe()
        self.assertTrue((old['masks']['family']==new['masks']['family']).all())
        self.assertTrue((old['contracts'][:,:3]==new['contracts'][:,:3]).all())
        # Hidden expiry alone is not an actor wake-up.
        other=self.world();other.advance(200);before=other.observation_revision
        self.assertEqual(other.advance_until_observable(900),900)
        self.assertEqual(other.observation_revision,before)

    def test_orders_remain_available_rest_while_unknown_then_fill_only_on_actual_book(self):
        e=self.world([book(0,100),book(1000,1100)]);e.advance(600)
        result=e.dispatch('ORDER',[OrderSpec('x','YES','BUY',1,600000)],idempotency_key='allowed-unknown')
        self.assertEqual(result['status'],'IN_FLIGHT');e.advance(900)
        self.assertFalse(e.api_fills);self.assertEqual(len(e.api_orders),1)
        e.advance(1000);self.assertEqual(len(e.api_fills),1);self.assertEqual(e.held('x','YES'),1)
        self.assertEqual(e.api_fills[0]['filled_at_ms'],1000);self.assertEqual(e.cash,1000*UNIT-450000)
        self.assertFalse(e.books['x'].invalidated)
        e.advance(1500);self.assertTrue(e.books['x'].invalidated)

    def test_new_snapshot_at_expiry_wins_and_old_timer_does_not_expire_it(self):
        e=self.world([book(0,100),book(500,500,ask=550000)])
        e.advance(500);self.assertFalse(e.books['x'].invalidated)
        self.assertEqual(e.books['x'].book.asks[0][0],550000)
        self.assertEqual(e.world_confidence_expiries,0)
        e.advance(1000);self.assertTrue(e.books['x'].invalidated)
        self.assertEqual(e.world_confidence_expiries,1)

    def test_role_prefix_stale_books_are_unknown_without_past_events_or_free_actor_state(self):
        t=tape([book(0,100)]);t.world_book_lifetime_ms=500;prepare(t,(0,5000),role_prefix_initial_state=True)
        e=t.episode(fixture().mechanics,3000,5000,initial_cash_micros=1000*UNIT,
            target_wealth_micros=10000*UNIT,burnin_ms=100)
        self.assertTrue(e.books['x'].invalidated);self.assertFalse(e.actor_books)
        self.assertFalse(e.budget.reads);self.assertFalse(e.budget.writes)
        self.assertEqual(e.cash,1000*UNIT)

    def test_unknown_liquidity_prints_do_not_create_passive_queue_or_strict_fills(self):
        e=self.world();e.passive=DisplayedQueueFlow(e,'cup');e.advance(600)
        e._execute('ORDER',[OrderSpec('x','YES','BUY',10,500000)])
        e.advance(900);e.passive.trade(900,dict(tournament_id='cup',exchange_id='x',market_id='m',
            executed_at_ms=900,match_id='a',yes_price_micros=450000,quantity=100))
        self.assertFalse(e.api_fills);self.assertEqual(e.passive.source_volume,100)
        self.assertEqual(e.passive.unknown_liquidity_print_volume,100)
        self.assertEqual(e.cash,1000*UNIT)

    def test_expired_mark_is_never_relabelled_current_and_legacy_profile_stays_unchanged(self):
        for enabled in(False,True):
            e=self.world(lifetime=500 if enabled else None);e.advance(200)
            e._execute('ORDER',[OrderSpec('x','YES','BUY',1,450000)]);e.advance(600)
            if enabled:
                with self.assertRaisesRegex(ValueError,'INCOMPLETE_SIG_WEALTH_MARK'):snapshot(e)
                value=snapshot(e,historical_last_mark=True)
                self.assertEqual(value.stale_exchanges,('x',));self.assertIn('last_observed',value.convention)
            else:self.assertFalse(e.books['x'].invalidated);self.assertFalse(snapshot(e).stale_exchanges)
        for value in(0,-1,True,'500'):
            with self.assertRaisesRegex(ValueError,'EXPLICIT_POSITIVE'):validate_lifetime(value)

    def test_dataset_required_lifetime_cannot_be_silently_omitted_or_changed(self):
        from .experiment import Experiment
        from .selection import DataUse
        from .snapshot_clock import MODE
        original=tape([book(0,0)]);original.manifest={'requiredWorldBookLifetimeMs':500}
        config={'mode':'inspected_development','device':'cpu','dataset':'fixture-only',
            'mechanics':'research/tartarus-learning-v2/development-mechanics-v2.json',
            'development_train_ms':[1,500],'development_selection_ms':[600,1000],
            'snapshot_world_clock':MODE}
        with tempfile.TemporaryDirectory()as directory:
            root=Path(directory)
            with patch('moloch.learning_actions.experiment.FlowTape',return_value=original),\
                 patch('moloch.learning_actions.experiment.DataUse',side_effect=lambda:DataUse(root/'registry')):
                for i,lifetime in enumerate((None,250)):
                    with self.assertRaisesRegex(ValueError,'DATASET_REQUIRES_EXACT'):
                        Experiment({**config,'world_book_lifetime_ms':lifetime},root/str(i))
                x=Experiment({**config,'world_book_lifetime_ms':500},root/'exact')
                self.assertEqual(x.tape('train').world_book_lifetime_ms,500)
