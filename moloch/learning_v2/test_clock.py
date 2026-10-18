import unittest
from .clock import BudgetConfig, RateBudget, EventClock


class VirtualMechanics(unittest.TestCase):
    def setUp(self):
        self.c = BudgetConfig(100, 600, 24, 2500, 'a'*64)
        self.q = RateBudget(self.c)

    def test_slow_response_delays_both_next_read_and_write(self):
        self.assertTrue(self.q.admit('r1', 0, False))
        self.q.complete('r1', 10_000)
        self.assertEqual(self.q.ready_at(10_000, True), 10_600)
        self.assertFalse(self.q.admit('w1', 10_000, True))

    def test_write_completion_blocks_subsequent_reads(self):
        self.q.admit('w1', 0, True)
        self.q.complete('w1', 2000)
        self.assertEqual(self.q.ready_at(2000, False), 4500)

    def test_inclusive_read_boundary_is_60001_not_60000(self):
        for i in range(100):
            self.assertTrue(self.q.admit(str(i), i*600, False))
            self.q.complete(str(i), i*600)
        self.assertEqual(self.q.ready_at(60000, False), 60001)
        self.assertTrue(self.q.admit('next', 60001, False))

    def test_inclusive_write_cap_does_not_reset_for_new_policy(self):
        for i in range(24):
            self.assertTrue(self.q.admit(str(i), i*2500, True))
            self.q.complete(str(i), i*2500)
        self.assertEqual(self.q.ready_at(60000, True), 60001)

    def test_unknown_write_blocks_writes_but_allows_reconciliation_reads(self):
        self.q.admit('w', 0, True)
        self.q.complete('w', 1000, uncertain=True)
        with self.assertRaisesRegex(ValueError, 'UNKNOWN_ACCEPTANCE'):
            self.q.ready_at(3500, True)
        self.assertTrue(self.q.admit('read', 3500, False))

    def test_active_request_cannot_be_overlapped(self):
        self.q.admit('r', 0, False)
        with self.assertRaisesRegex(ValueError, 'SERIAL_QUEUE_BUSY'):
            self.q.admit('other', 1000, False)

    def test_backoff_uses_retry_after_and_is_shared(self):
        self.q.admit('r', 0, False)
        self.q.complete('r', 1000, status=429, retry_after_ms=90_000)
        self.assertEqual(self.q.ready_at(1000, True), 91000)

    def test_clock_regression_is_durable_for_episode(self):
        self.q.ready_at(1000, False)
        with self.assertRaisesRegex(ValueError, 'CLOCK_REGRESSION'):
            self.q.ready_at(999, False)
        with self.assertRaisesRegex(ValueError, 'MANUAL_REVIEW'):
            self.q.ready_at(1001, False)

    def test_unknown_request_or_replacement_refused(self):
        self.q.admit('r', 0, False)
        with self.assertRaisesRegex(ValueError, 'REQUEST_MISMATCH'):
            self.q.complete('other', 0)
        self.q.complete('r', 0)
        with self.assertRaisesRegex(ValueError, 'KNOWN_REQUEST'):
            self.q.admit('r', 600, False)

    def test_virtual_time_runs_without_wall_sleep_and_source_precedes_arrival(self):
        c = EventClock(0)
        c.schedule(5000, 20, 'arrival', 'order')
        c.schedule(5000, 10, 'source', 'new-book')
        seen = []
        c.advance(4999, lambda *event: seen.append(event))
        self.assertEqual(seen, [])
        c.advance(5000, lambda *event: seen.append(event))
        self.assertEqual([e[1] for e in seen], ['source', 'arrival'])
        self.assertEqual(c.now, 5000)

    def test_cancel_ack_does_not_erase_preceding_fill(self):
        c = EventClock(0)
        for at, kind in [(100, 'cancel-request'), (150, 'fill'), (200, 'cancel-ack')]:
            c.schedule(at, 0, kind, None)
        seen = []
        c.advance(200, lambda at, kind, _: seen.append(kind))
        self.assertEqual(seen, ['cancel-request', 'fill', 'cancel-ack'])


if __name__ == '__main__':
    unittest.main()
