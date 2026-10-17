"""Original-shaped synthetic packets only; no live liquidity or financial claim."""
from copy import deepcopy
import json
import unittest

try:
    from . import depth_actions as D
    from .test_worker_packet import Fixture, encoded
    from .test_target_frontier import fixture as frontier_fixture
    from .target_frontier import evaluate_candidate
except ImportError:
    import depth_actions as D
    from test_worker_packet import Fixture, encoded
    from test_target_frontier import fixture as frontier_fixture
    from target_frontier import evaluate_candidate

SHA = "a" * 64


def book(ex="1", market="m1"):
    return {"tournamentId": "cup", "marketId": market, "exchangeId": ex,
            "sequence": "7", "sourceAt": "1970-01-01T00:00:01.000000001Z",
            "requestStartedAt": "1970-01-01T00:00:01.010Z",
            "receivedAt": "1970-01-01T00:00:01.100Z",
            "bids": [{"price": "0.400000", "quantity": 3}, {"price": "0.350000", "quantity": 5}],
            "asks": [{"price": "0.450000", "quantity": 2}, {"price": "0.500000", "quantity": 4}]}


class DepthTests(unittest.TestCase):
    def setUp(self):
        self.f = Fixture()
        self.addCleanup(self.f.temp.cleanup)
        self.f.m["nativeBooks"] = [book()]
        common = {"schema_version": 1, "evidence_class": "synthetic_declared",
                  "as_of_ms": 1000, "valid_until_ms": 6000, "source_sha256": SHA,
                  "unmodeled_state_statement": "Synthetic identity/fees, not official semantics.",
                  "bank_manifest_hash": self.f.d["manifestHash"]}
        self.registry = {**common, "contracts": [{"contract_id": "c", "exchange_id": "1",
                          "market_id": "m1", "semantic_source_sha256": SHA}]}
        self.fees = {**common, "fees": [{"exchange_id": "1", "side": side, "action": action,
                                          "fee_per_share_micros": 1000}
                                         for side in ("YES", "NO") for action in ("BUY", "SELL")]}

    def run_adapter(self, **kwargs):
        self.f.meta()
        packet = self.f.verify()
        return D.from_packet(packet, expected_metadata_sha256=self.f.pins["expected_metadata_sha256"],
                             registry=self.registry, fee_evidence=self.fees, decision_ms=2000,
                             maximum_book_age_ms=5000, **kwargs)

    def test_both_complements_original_depth_prefixes_and_fees(self):
        out = self.run_adapter()
        self.assertEqual(out["status"], "available")
        views = {(x["action"], x["side"]): x for x in out["buy_action_views"] + out["rotation_sell_curves"]}
        for key, expected in {("BUY", "YES"): [450000, 500000], ("BUY", "NO"): [600000, 650000],
                              ("SELL", "YES"): [400000, 350000], ("SELL", "NO"): [550000, 500000]}.items():
            field = "unit_cost_micros" if key[0] == "BUY" else "unit_proceeds_micros"
            self.assertEqual([x[field] for x in views[key]["levels"]], expected)
        self.assertEqual(views[("BUY", "YES")]["levels"][-1]["prefix_quantity"], 6)
        self.assertEqual(views[("BUY", "YES")]["levels"][-1]["prefix_gross_cost_micros"], 2_906000)
        self.assertEqual(views[("SELL", "YES")]["levels"][-1]["prefix_net_proceeds_micros"], 2_942000)
        for x in views.values():
            self.assertEqual(x["original_book"]["sourceAt"], book()["sourceAt"])
            self.assertEqual(x["capture_at_ms"], 2000)
            self.assertEqual(x["available_at_ms"], 2000)
            self.assertEqual(x["valid_until_ms"], 6000)
            self.assertIsNone(x["official_receipt_sha256"])
            self.assertFalse(x["execution_authorized"])

    def test_shared_resource_not_two_independent_liquidities(self):
        out = self.run_adapter()
        views = {(x["action"], x["side"]): x for x in out["buy_action_views"] + out["rotation_sell_curves"]}
        ids = lambda k: [x["resource_id"] for x in views[k]["levels"]]
        self.assertEqual(ids(("BUY", "YES")), ids(("SELL", "NO")))
        self.assertEqual(ids(("BUY", "NO")), ids(("SELL", "YES")))
        self.assertNotEqual(ids(("BUY", "YES")), ids(("BUY", "NO")))
        self.assertFalse(out["simultaneous_buy_sell_supported"])
        self.assertFalse(out["opposite_pair_simultaneous_use_supported"])
        self.assertFalse(out["sale_proceeds_are_available_cash"])
        self.assertEqual(len(out["liquidity_resources"]), 4)

    def test_selected_real_format_frontier_exact_cost_without_sale_credit(self):
        out = self.run_adapter()
        selected = D.select_buy_actions(out, {"1": "YES"})
        a, _, p, r, c = frontier_fixture(cash=10_000000)
        a["contracts"][0]["exchange_id"] = "1"
        # The unrelated earlier synthetic account's clock window must itself be valid.
        a["valid_until_ms"] = 6000
        c["decision_ms"] = 2000
        candidate = evaluate_candidate(a, selected, p, r, c, {"buy:1:YES": 3})
        self.assertEqual(candidate["gross_cost_micros"], 1_403000)
        self.assertEqual([x["quantity"] for x in candidate["depth_prefixes"]], [2, 1])
        self.assertEqual(candidate["cash_after_micros"], 8_597000)
        with self.assertRaisesRegex(D.DepthError, "SIDE_ALIAS"):
            D.select_buy_actions(out, {"1": "yes"})
        selected[0]["levels"][0]["quantity"] = 999
        self.assertEqual(out["buy_action_views"][0]["levels"][0]["quantity"], 2)

    def test_stale_original_source_even_with_recent_request_and_capture(self):
        b = self.f.m["nativeBooks"][0]
        b["sourceAt"] = "1970-01-01T00:00:00.000Z"
        self.f.meta()
        p = self.f.verify()
        out = D.from_packet(p, expected_metadata_sha256=self.f.pins["expected_metadata_sha256"],
                            registry=self.registry, fee_evidence=self.fees,
                            decision_ms=2000, maximum_book_age_ms=1000)
        self.assertEqual(out["rejections"][0]["reason"], "BOOK_STALE")
        self.assertEqual(out["buy_action_views"], [])

    def test_future_source_request_and_receipt_refused_without_clock_repair(self):
        for field, value in (("sourceAt", "1970-01-01T00:00:02.001Z"),
                             ("receivedAt", "1970-01-01T00:00:02.001Z"),
                             ("requestStartedAt", "1970-01-01T00:00:01.101Z")):
            self.f.m["nativeBooks"] = [book()]
            self.f.m["nativeBooks"][0][field] = value
            out = self.run_adapter()
            self.assertEqual(out["rejections"][0]["reason"], "BOOK_FUTURE_OR_NONCAUSAL")

    def test_duplicate_conflicting_and_missing_books_are_not_merged(self):
        for books, reason in (([book(), book()], "BOOK_DUPLICATE_OR_CONFLICT"),
                              ([book(), {**book(), "sequence": "8"}], "BOOK_DUPLICATE_OR_CONFLICT"),
                              ([], "BOOK_UNAVAILABLE")):
            self.f.m["nativeBooks"] = books
            out = self.run_adapter()
            self.assertEqual(out["rejections"][0]["reason"], reason)
            self.assertEqual(out["rotation_sell_curves"], [])

    def test_unbound_scope_market_and_partial_market_results(self):
        self.f.m["scope"]["exchanges"]["2"] = "m2"
        control = json.loads(self.f.members["control"])
        control["scope"] = deepcopy(self.f.m["scope"])
        self.f.members["control"] = encoded(control)
        self.f.refresh()
        self.registry["contracts"].append({"contract_id": "c2", "exchange_id": "2", "market_id": "m2", "semantic_source_sha256": SHA})
        self.fees["fees"] += [{**x, "exchange_id": "2"} for x in deepcopy(self.fees["fees"])]
        self.f.m["nativeBooks"].append({**book("2", "m2"), "tournamentId": "other"})
        out = self.run_adapter()
        self.assertEqual(out["status"], "partial")
        self.assertEqual([x["exchange_id"] for x in out["accepted_markets"]], ["1"])
        self.assertEqual(out["rejections"], [{"exchange_id": "2", "market_id": "m2", "reason": "BOOK_SCOPE"}])

    def test_malformed_crossed_empty_duplicate_depth_and_no_float_prices(self):
        changes = [("bids", [], "BOOK_DEPTH_BOUND_OR_EMPTY"),
                   ("bids", [{"price": "0.460000", "quantity": 1}], "BOOK_CROSSED"),
                   ("asks", [{"price": "0.45", "quantity": 1}, {"price": "0.45", "quantity": 2}], "BOOK_DUPLICATE_OR_UNSORTED_LEVEL"),
                   ("asks", [{"price": 0.45, "quantity": 1}], "BOOK_PRICE_DECIMAL"),
                   ("asks", [{"price": "0.4500001", "quantity": 1}], "BOOK_PRICE_DECIMAL"),
                   ("asks", [{"price": "0.45", "quantity": True}], "BOOK_QUANTITY"),
                   ("asks", [{"price": "0.45", "quantity": 10**9+1}], "BOOK_QUANTITY")]
        for field, value, reason in changes:
            self.f.m["nativeBooks"] = [{**book(), field: value}]
            out = self.run_adapter()
            self.assertEqual(out["rejections"][0]["reason"], reason)

    def test_fee_evidence_missing_zero_not_imputed_and_side_alias_denied(self):
        removed = self.fees["fees"].pop()
        out = self.run_adapter()
        self.assertEqual(out["rejections"][0]["reason"], "EXPLICIT_SIDE_FEES_UNAVAILABLE")
        self.fees["fees"].append({**removed, "side": "no"})
        with self.assertRaisesRegex(D.DepthError, "FEE_IDENTITY"):
            self.run_adapter()

    def test_registry_and_external_metadata_pin_are_required(self):
        self.f.meta()
        p = self.f.verify()
        with self.assertRaisesRegex(D.DepthError, "EXPECTED_METADATA"):
            D.from_packet(p, expected_metadata_sha256="b"*64, registry=self.registry,
                          fee_evidence=self.fees, decision_ms=2000, maximum_book_age_ms=5000)
        self.registry["contracts"][0]["market_id"] = "different"
        with self.assertRaisesRegex(D.DepthError, "REGISTRY_SCOPE"):
            self.run_adapter()
        with self.assertRaisesRegex(D.DepthError, "VERIFIED_PACKET"):
            D.from_packet({}, expected_metadata_sha256="a"*64, registry=self.registry,
                          fee_evidence=self.fees, decision_ms=2000, maximum_book_age_ms=5000)

    def test_original_version_or_depth_changes_derived_witness_and_no_authority(self):
        first = self.run_adapter()
        self.f.m["nativeBooks"][0]["asks"][0]["quantity"] += 1
        second = self.run_adapter()
        self.assertNotEqual(first["accepted_markets"][0]["normalized_book_json_sha256"],
                            second["accepted_markets"][0]["normalized_book_json_sha256"])
        self.assertFalse(second["execution_authorized"])
        self.assertIsNone(second["forecast_or_profit_estimate"])


if __name__ == "__main__":
    unittest.main()
