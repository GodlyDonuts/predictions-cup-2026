import unittest
from .test_actions import environment
from .realtime import DerivedStream


def env():
    e=environment();e.realtime=DerivedStream(e);e.keys['SIG'].append(('realtime_token',))
    return e


class DerivedStreamTests(unittest.TestCase):
    def connected(self,baseline=True):
        e=env();e.request_observation('SIG',('realtime_token',));e.advance(101)
        if baseline:e.request_observation('SIG',('book','x'));e.advance(201)
        return e

    def test_token_is_one_write_and_subsequent_books_are_not_rest_polls(self):
        e=self.connected()
        self.assertEqual(len(e.budget.writes),1);self.assertEqual(len(e.budget.reads),1)
        e.advance(1000)
        self.assertEqual(e.actor_books['x'].book.available_at_ms,1000)
        self.assertFalse(e.actor_books['x'].invalidated)
        self.assertEqual(len(e.budget.reads),1);self.assertEqual(e.realtime.deliveries,1)

    def test_unbaselined_stream_quotes_are_visible_but_not_trusted(self):
        e=self.connected(False);e.advance(1000)
        self.assertTrue(e.actor_books['x'].invalidated)
        e.request_observation('SIG',('book','x'));e.advance(1100)
        self.assertFalse(e.actor_books['x'].invalidated);self.assertIn('x',e.realtime.synced)

    def test_gap_requires_new_rest_baseline_even_after_more_stream_books(self):
        e=self.connected()
        e.clock.schedule(1200,10,'tape',{'kind':'gap','exchange_id':'x','market_id':e.mapping['x']})
        e.advance(5000)
        self.assertTrue(e.actor_books['x'].invalidated);self.assertNotIn('x',e.realtime.synced)
        e.request_observation('SIG',('book','x'));e.advance(5100)
        self.assertFalse(e.actor_books['x'].invalidated)

    def test_disconnect_and_refresh_fence_old_queued_deliveries(self):
        e=self.connected();e.realtime.source_book(e.now,'x')
        e.realtime.disconnect();e.advance(e.now)
        self.assertFalse(e.realtime.enabled);self.assertEqual(e.realtime.deliveries,0)
        self.assertTrue(e.actor_books['x'].invalidated)
        e.request_observation('SIG',('realtime_token',));e.realtime.disconnect();e.advance(301)
        self.assertFalse(e.realtime.enabled);self.assertEqual(len(e.budget.writes),2)

    def test_token_expiry_and_periodic_reconciliation_are_not_free_refreshes(self):
        e=self.connected();e.end=20_000_000;e.advance(200000)
        self.assertTrue(e.actor_books['x'].invalidated)
        e.advance(101+e.realtime.token_lifetime_ms)
        self.assertFalse(e.realtime.enabled);self.assertEqual(len(e.budget.writes),1)

    def test_token_uses_write_eligibility_not_read_eligibility(self):
        e=env()
        for i in range(30):
            e.dispatch('CANCEL_ALL',None,idempotency_key=str(i));e.advance(e.now+300)
        o=e.observe();i=o['entities'].index(('read',('SIG',('realtime_token',))))
        self.assertFalse(o['masks']['family'][i,10])
        j=o['entities'].index(('read',('SIG',('book','x'))))
        self.assertTrue(o['masks']['family'][j,10])


if __name__=='__main__':unittest.main()
