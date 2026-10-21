import copy,tempfile,unittest
from pathlib import Path
from .test_actions import environment
from .realtime import DerivedStream,CONTINUOUS_REFRESH,LEGACY_REFRESH,refresh_profile


class TokenRefreshTests(unittest.TestCase):
    def connected(self,mode=None):
        e=environment();e.realtime=DerivedStream(e,refresh_mode=mode);e.realtime.token_lifetime_ms=1000
        e.keys['SIG'].append(('realtime_token',));e.request_observation('SIG',('realtime_token',));e.advance(101)
        e.request_observation('SIG',('book','x'));e.advance(201);return e

    def test_timely_paid_refresh_preserves_channel_baseline_and_queued_data(self):
        e=self.connected();s=e.realtime;connection=s.connection;baseline=dict(s.synced);old_expiry=s.expires_at
        actor=e.actor_books['x'];s.private_pending=True;s.private_fills=[{'known':'fill'}];s.private_orders=[{'known':'order'}]
        e.advance(700);revision=e.observation_revision
        self.assertEqual(e.request_observation('SIG',('realtime_token',))['status'],'IN_FLIGHT');e.advance(801)
        self.assertEqual(s.connection,connection);self.assertEqual(s.synced,baseline)
        self.assertIs(e.actor_books['x'],actor);self.assertFalse(actor.invalidated)
        self.assertTrue(s.private_pending);self.assertEqual(s.private_fills,[{'known':'fill'}]);self.assertEqual(s.private_orders,[{'known':'order'}])
        self.assertGreater(e.observation_revision,revision);self.assertEqual(len(e.budget.writes),2);self.assertEqual(len(e.budget.reads),1)
        # A book queued on the same live connection remains valid after auth
        # changes. The prior token timer must not close the renewed channel.
        e.clock.schedule(old_expiry+1,30,'derived_book_receipt',(connection,'x',copy.deepcopy(actor)))
        e.advance(old_expiry+2);self.assertTrue(s.enabled);self.assertGreater(s.deliveries,0)
        e.advance(s.expires_at);self.assertFalse(s.enabled);self.assertTrue(e.actor_books['x'].invalidated)

    def test_expired_or_disconnected_channel_still_needs_paid_baseline(self):
        for disconnect in (False,True):
            e=self.connected();s=e.realtime;connection=s.connection
            if disconnect:s.disconnect()
            else:e.advance(s.expires_at)
            self.assertFalse(s.enabled);self.assertEqual(e.request_observation('SIG',('realtime_token',))['status'],'IN_FLIGHT')
            e.advance(e.now+100);self.assertTrue(s.enabled);self.assertGreater(s.connection,connection)
            self.assertFalse(s.synced);self.assertTrue(e.actor_books['x'].invalidated)
            self.assertEqual(e.request_observation('SIG',('book','x'))['status'],'IN_FLIGHT');e.advance(e.now+100)
            self.assertFalse(e.actor_books['x'].invalidated);self.assertEqual(len(e.budget.reads),2)

    def test_real_passive_fill_survives_refresh_before_publication_or_receipt_once(self):
        from .test_execution_receipts import scoped
        from .test_passive import print_row
        from .passive import StrictPriorityFlow
        from .contract import OrderSpec,UNIT
        for refresh_at,lifetime in ((1100,1800),(1950,5000)):
            with self.subTest(refresh_at=refresh_at):
                e=scoped();e.passive=StrictPriorityFlow(e,'cup')
                e.realtime=DerivedStream(e);s=e.realtime;s.token_lifetime_ms=lifetime
                e.keys['SIG'].append(('realtime_token',))
                e.request_observation('SIG',('realtime_token',));e.advance(e.busy_until)
                connection=s.connection;old_expiry=s.expires_at
                e.dispatch('ORDER',[OrderSpec('x','YES','BUY',10,425000)],idempotency_key='rest')
                e.advance(e.busy_until);order=next(iter(e.known_orders))
                e.clock.schedule(1000,10,'api_counterparty_trade',print_row(quantity=2))
                e.advance(refresh_at);self.assertEqual(e.known_orders[order].remaining,10)
                self.assertFalse(e.known_private_fills)
                self.assertEqual(s.mint()['status'],'IN_FLIGHT');e.advance(e.busy_until)
                self.assertEqual(s.connection,connection)
                e.advance(2099);self.assertFalse(e.known_private_fills)
                e.advance(2100)
                self.assertEqual(e.known_orders[order].remaining,8)
                self.assertEqual(len(e.known_private_fills),1)
                self.assertEqual(e.known_private_fills[0]['quantity'],2)
                self.assertEqual(len(e.api_fills),1);self.assertEqual(e.cash,1000*UNIT-2*425000)
                # A scoped fill receipt reveals the fill, not unrelated cash or lots.
                self.assertEqual(e.known_cash,1000*UNIT);self.assertFalse(e.known_lots)
                e.advance(max(2200,old_expiry+1))
                self.assertEqual(len(e.known_private_fills),1);self.assertEqual(len(e.api_fills),1)
                self.assertEqual(len(e.budget.writes),3);self.assertFalse(e.budget.reads)

    def test_exact_expiry_boundary_is_reconnect_even_if_timer_has_not_run(self):
        e=self.connected();s=e.realtime;old=s.expires_at;connection=s.connection
        e.advance(old-100);self.assertEqual(s.mint()['status'],'IN_FLIGHT');e.advance(old)
        self.assertTrue(s.enabled);self.assertGreater(s.connection,connection);self.assertFalse(s.synced)

    def test_genuine_gap_survives_successful_token_refresh(self):
        e=self.connected();s=e.realtime;e.advance(700)
        e.clock.schedule(701,10,'tape',{'kind':'gap','exchange_id':'x','market_id':e.mapping['x']});e.advance(701)
        self.assertFalse(s.synced);s.mint();e.advance(801)
        self.assertTrue(s.enabled);self.assertFalse(s.synced);self.assertTrue(e.actor_books['x'].invalidated)

    def test_disconnect_fences_pending_token_receipt_and_unknown_blocks_mint(self):
        e=self.connected();s=e.realtime;s.mint();s.disconnect();e.advance(e.now+100)
        self.assertFalse(s.enabled);self.assertEqual(len(e.budget.writes),2)
        e.budget.unknown.add('unresolved');self.assertEqual(s.mint()['status'],'UNRESOLVED_EXECUTION')
        self.assertEqual(s.mints,2)

    def test_legacy_reconnect_behavior_is_explicitly_reproducible(self):
        e=self.connected(LEGACY_REFRESH);s=e.realtime;connection=s.connection
        s.mint();e.advance(e.now+100)
        self.assertGreater(s.connection,connection);self.assertFalse(s.synced);self.assertTrue(e.actor_books['x'].invalidated)

    def test_default_and_invalid_profile_configuration(self):
        self.assertEqual(refresh_profile(None),CONTINUOUS_REFRESH)
        for value in ('unknown',0,False,{},[]):
            with self.assertRaises(ValueError):refresh_profile(value)
        from .experiment import Experiment
        with tempfile.TemporaryDirectory()as td:
            with self.assertRaisesRegex(ValueError,'REQUIRES_REALTIME_ADAPTER'):
                Experiment({'sig_realtime_refresh_mode':CONTINUOUS_REFRESH},Path(td)/'unused')


if __name__=='__main__':unittest.main()
