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
