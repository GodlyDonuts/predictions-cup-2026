import unittest
from .compile_snapshot_metadata import enrich


class SnapshotMetadataTests(unittest.TestCase):
    def test_original_values_cannot_be_changed_under_a_clock_extension(self):
        original={'original_sha256':'a'*64,'available_at_ms':100,'bids':[[400000,10]]}
        verified={**original,'native_event_received_at_ms':20,'native_processed_at_ms':90}
        result=enrich(original,verified)
        self.assertEqual(result['available_at_ms'],100);self.assertEqual(result['bids'],original['bids'])
        self.assertEqual(result['native_processed_at_ms'],90)
        with self.assertRaisesRegex(ValueError,'SOURCE_VALUE_CHANGED'):
            enrich({**original,'bids':[[450000,10]]},verified)
        with self.assertRaisesRegex(ValueError,'UNDECLARED'):
            enrich(original,{**verified,'unverified_price':.9})
