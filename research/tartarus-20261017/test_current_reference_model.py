"""Historical opinions can widen fresh-price stress, never renew authority."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("current_reference_model", Path(__file__).with_name("current-reference-model.py"))
model = importlib.util.module_from_spec(spec)
spec.loader.exec_module(model)


class HistoricalReviewEnvelopes(unittest.TestCase):
    def review(self, side="YES"):
        return {"reviewedAtMs": 10, "expiresAtMs": 20,
                "ordinaryProbability": "0.800000", "side": side}

    def test_expired_opinion_widens_current_quotes_without_mutating_its_clocks(self):
        review = self.review(); original = copy.deepcopy(review)
        points = {"1": {"lowerPpm": 300000, "upperPpm": 400000}}
        lo, hi, sha, raw = model.historical_envelope([(review, "a" * 64)], ["1"], points, 100, "b" * 64)
        self.assertEqual((lo, hi), (300000, 800000))
        self.assertEqual(review, original)
        source = json.loads(raw)
        self.assertEqual(source["originalReviews"][0]["expiresAtMs"], 20)
        self.assertEqual(source["originalReviews"][0]["reviewedAtMs"], 10)
        self.assertFalse(source["originalReviewRenewed"])
        self.assertFalse(source["executionAuthority"])
        self.assertEqual(sha, model.sha256(raw).hexdigest())

    def test_no_side_and_same_party_members_keep_both_price_and_opinion_extremes(self):
        points = {"1": {"lowerPpm": 700000, "upperPpm": 800000},
                  "2": {"lowerPpm": 650000, "upperPpm": 850000}}
        lo, hi, _, _ = model.historical_envelope([(self.review("NO"), "a" * 64)],
                                                 ["1", "2"], points, 100, "b" * 64)
        self.assertEqual((lo, hi), (200000, 850000))

    def test_quote_ignorance_cannot_be_replaced_by_an_expired_confident_opinion(self):
        lo, hi, _, _ = model.historical_envelope([(self.review(), "a" * 64)], ["1"],
            {"1": {"lowerPpm": 0, "upperPpm": 1000000}}, 100, "b" * 64)
        self.assertEqual((lo, hi), (0, 1000000))


if __name__ == "__main__":
    unittest.main()
