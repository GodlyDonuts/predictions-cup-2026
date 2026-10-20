"""Recorded public facts, not complete tape continuity or profitable trading."""
import tempfile,unittest
from pathlib import Path
from unittest.mock import patch
import torch
from .test_actions import environment
from .realtime import DerivedStream


def world(public=True):
    e=environment();e.realtime=DerivedStream(e,public_trades=public)
    e.keys['SIG'].append(('realtime_token',));return e


def trade(e,at,match='public-test',*,executed=None,quantity=3):
    return {'kind':'trade_flow','venue':'SIG','available_at_ms':at,'received_at_ms':at-1,
            'executed_at_ms':at-2 if executed is None else executed,'exchange_id':'x','market_id':e.mapping['x'],
            'tournament_id':'public-fixture','match_id':match,'quantity':quantity,'yes_price_micros':500000,
            'original_sha256':'c'*64,'aggressor_side_known':False}


def identity(row):return(row['tournament_id'],row['exchange_id'],row['match_id'])


def connect(e):
    e.request_observation('SIG',('realtime_token',));e.advance(e.busy_until)


class PublicTradeStreamTests(unittest.TestCase):
    def test_original_receipt_clock_no_backfill_no_extra_read_or_book_baseline(self):
        e=world();old=trade(e,200,'before-connect');e.clock.schedule(200,12,'trade_flow',old);e.advance(200)
        self.assertNotIn(identity(old),e.flow.seen);connect(e)
        self.assertNotIn(identity(old),e.flow.seen)
        row=trade(e,400);e.clock.schedule(400,12,'trade_flow',row);e.advance(399)
        self.assertNotIn(identity(row),e.flow.seen);e.advance(400)
        self.assertIn(identity(row),e.flow.seen);self.assertEqual(e.realtime.public_deliveries,1)
        self.assertEqual(e.flow.rows['x'][-1]['available_at_ms'],400)
        observed=e.observe();i=observed['entities'].index(('contract','x'))
        self.assertEqual(float(observed['contracts'][i,21]),1.)  # flow16:24, recent_print_available
        self.assertEqual((len(e.budget.reads),len(e.budget.writes)),(0,1));self.assertFalse(e.actor_books)
        self.assertFalse(e.api_fills)

    def test_delivered_public_print_interrupts_wait_but_hidden_history_does_not(self):
        for public in(False,True):
            e=world(public);connect(e);row=trade(e,700);e.clock.schedule(700,12,'trade_flow',row)
            self.assertEqual(e.advance_until_observable(900),700 if public else 900)

    def test_disabled_profile_preserves_paid_rest_only_behavior(self):
        e=world(False);connect(e);row=trade(e,300);e.clock.schedule(300,12,'trade_flow',row);e.advance(300)
        self.assertIn(identity(row),e.trade_history.seen);self.assertNotIn(identity(row),e.flow.seen)
        self.assertEqual(e.realtime.public_deliveries,0)

    def test_rest_and_stream_duplicate_facts_are_counted_once_in_both_orders(self):
        for first in('stream','rest'):
            with self.subTest(first=first):
                e=world()
                if first=='stream':connect(e)
                row=trade(e,300);e.clock.schedule(300,12,'trade_flow',row);e.advance(350)
                e.request_observation('SIG',('trade_flow','x'));e.advance(e.busy_until)
                if first=='rest':
                    connect(e);repeat=dict(row,available_at_ms=600)
                    e.clock.schedule(600,12,'trade_flow',repeat);e.advance(600)
                    self.assertEqual(e.realtime.public_duplicates,1)
                matches=[r for r in e.flow.rows['x']if r['match_id']==row['match_id']]
                self.assertEqual(len(matches),1);self.assertEqual(matches[0]['quantity'],3)
                self.assertEqual(len(e.budget.reads),1)

    def test_disconnect_fences_queued_receipt_and_does_not_replay_on_reconnect(self):
        e=world();connect(e);row=trade(e,e.now);e.realtime.source_trade(e.now,row)
        e.realtime.disconnect();e.advance(e.now)
        self.assertNotIn(identity(row),e.flow.seen);self.assertEqual(e.realtime.public_suppressed,1)
        connect(e);self.assertNotIn(identity(row),e.flow.seen)

    def test_gap_fences_pending_scope_but_new_prints_are_partial_observations(self):
        for scope in('exchange','market','global','unrelated'):
            with self.subTest(scope=scope):
                e=world();connect(e);row=trade(e,e.now);e.realtime.source_trade(e.now,row)
                gap=({'exchange_id':'x'}if scope=='exchange'else {'market_id':e.mapping['x']}if scope=='market'
                     else {'exchange_id':'unrelated'}if scope=='unrelated'else {})
                e.realtime.source_gap(gap);e.advance(e.now)
                self.assertEqual(identity(row)in e.flow.seen,scope=='unrelated')
                later=trade(e,300,'after-gap');e.clock.schedule(300,12,'trade_flow',later);e.advance(300)
                self.assertIn(identity(later),e.flow.seen)
                self.assertFalse(e.realtime.synced);self.assertEqual(len(e.budget.reads),0)

    def test_exact_expiry_drops_print_and_queued_payload_is_immutable(self):
        e=world();connect(e);row=trade(e,e.now);e.realtime.source_trade(e.now,row);row['quantity']=999
        e.advance(e.now);self.assertEqual(e.flow.rows['x'][-1]['quantity'],3)
        at=e.realtime.expires_at;e.end=at+10;expired=trade(e,at,'expired')
        e.clock.schedule(at,12,'trade_flow',expired);e.advance(at)
        self.assertNotIn(identity(expired),e.flow.seen);self.assertFalse(e.realtime.enabled)

    def test_bad_availability_and_conflicting_match_facts_fail(self):
        e=world();connect(e)
        with self.assertRaisesRegex(ValueError,'ORIGINAL_PUBLIC_TRADE_AVAILABILITY'):
            e.realtime.source_trade(e.now,trade(e,e.now+1))
        row=trade(e,300);e.clock.schedule(300,12,'trade_flow',row);e.advance(300)
        e.clock.schedule(400,12,'trade_flow',dict(row,available_at_ms=400,quantity=4))
        with self.assertRaisesRegex(ValueError,'ORIGINAL_MATCH_FACTS_CHANGED'):e.advance(400)

    def test_recorded_public_receipts_reconstruct_through_full_return_training(self):
        from . import test_episode_tape as fixture
        from .episode_tape import EpisodeReader
        from .episode_training import optimize
        from .economics import GrowthLedger
        original=fixture.worlds
        def worlds():
            envs=original()
            for e in envs:
                e.realtime=DerivedStream(e,public_trades=True);e.keys['SIG'].append(('realtime_token',))
                e.realtime.mint();e.clock.schedule(350,12,'trade_flow',trade(e,350))
            return envs
        torch.set_num_threads(1);torch.manual_seed(118)
        with tempfile.TemporaryDirectory()as folder,patch.object(fixture,'worlds',worlds):
            helper=fixture.EpisodeTapeTests();original_actor=helper.actor
            def connected_actor():
                model=original_actor()
                with torch.no_grad():
                    # Keep this fixture connected; family11 can legally select
                    # the newly available stream-disconnect control.
                    model.heads['family'].bias[11]=0;model.heads['family'].bias[0]=100
                    model.fractions.bias[2]=-20;model.fractions.bias[3]=1000
                return model
            path=Path(folder)/'episode'
            with patch.object(helper,'actor',side_effect=connected_actor):writer,sha=helper.collect(path)
            self.assertTrue(all(e.realtime.public_deliveries>=1 and identity(trade(e,350))in e.flow.seen
                                for e in writer.stream.envs))
            reader=EpisodeReader(path,sha)
            try:
                model=writer.stream.model;opt=torch.optim.Adam(model.parameters(),lr=1e-5)
                rows=optimize(reader,model,opt,lambda a,b:worlds()[[1101,1201,1301].index(b)],
                    lambda e:GrowthLedger(e,development=True),continue_after_kl_rejection=True)
                self.assertEqual(len(rows),reader.metadata['batches']);self.assertTrue(any(r['accepted']for r in rows))
                self.assertFalse(reader.metadata['independent_test_used'])
            finally:reader.close()
