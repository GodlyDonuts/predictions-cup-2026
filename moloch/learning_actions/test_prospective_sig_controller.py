from datetime import datetime,timedelta,timezone
import unittest
from .prospective_sig_controller import segment_end


class CaptureBoundaryTests(unittest.TestCase):
    def test_bounded_segments_never_cross_final_reserved_end(self):
        now=datetime(2026,10,20,tzinfo=timezone.utc)
        self.assertEqual(segment_end(now,now+timedelta(days=4)),'2026-10-20T00:10:00Z')
        self.assertEqual(segment_end(now,now+timedelta(seconds=5)),'2026-10-20T00:00:05Z')
        with self.assertRaises(ValueError):segment_end(now,now)
        with self.assertRaises(ValueError):segment_end(now.replace(tzinfo=None),now)
