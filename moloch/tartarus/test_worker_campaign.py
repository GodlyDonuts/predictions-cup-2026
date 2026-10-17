"""Complete synthetic packet pipeline; no live book, probability or trade."""
from copy import deepcopy
import importlib.util
import unittest

import worker_campaign as C
from test_worker_packet import Fixture, encoded
from test_depth_actions import book
from test_native_snapshot import common


class WorkerCampaignTests(unittest.TestCase):
    def setUp(self):
        self.f = Fixture()
        self.addCleanup(self.f.temp.cleanup)
        registry = deepcopy(self.f.registry)
        scope = self.f.m["scope"]
        scope["exchanges"] = {x["exchange_id"]: x["market_id"] for x in registry["contracts"]}
        self.f.members["control"] = encoded({"scope": scope, "accountScope": "acct",
            "activation": {"runId": "run"}, "bankName": "bank"})
        self.f.m["heldBoundary"]["scope"] = deepcopy(scope)
        self.f.m["nativeBooks"] = [book(market="market-1")]
        self.f.refresh()
        self.config = {"evidence_class": "synthetic_full_pipeline", "decision_ms": 2000,
            "valid_until_ms": 3000, "registry": registry, "liabilities": self.f.liability,
            "maximum_book_age_ms": 5000, "buy_sides": {"1": "YES"},
            "fee_evidence": {**common(), "bank_manifest_hash": self.f.d["manifestHash"],
                "fees": [{"exchange_id": "1", "side": s, "action": a, "fee_per_share_micros": 1000}
                         for s in ("YES", "NO") for a in ("BUY", "SELL")]},
            "scenarios": {**common(), "contracts": [{k: r[k] for k in ("contract_id", "exchange_id", "semantic_source_sha256")}
                                                    for r in registry["contracts"]], "scenarios": [
                {"id": sid, "kind": "ordinary", "assumption_label": "Synthetic joint outcome, no forecast",
                 "payouts": {r["contract_id"]: ("resolved_yes" if i == 0 and r["exchange_id"] == "1" else "resolved_no")
                             for r in registry["contracts"]}} for i, sid in enumerate(("win", "lose"))]},
            "probability_models": [{"id": "tail", "weights_ppm": [100000, 900000], "assumption_label": "Declared synthetic tail"}],
            "rival_models": [{"id": "target", "thresholds_micros": [13000000, 13000000],
                "complete_field": False, "assumption_label": "Synthetic fixed rival target"}],
            "constraints": {"decision_ms": 2000, "maximum_gross_cost_micros": 8000000,
                "maximum_total_quantity": 6, "solver_time_limit_seconds": 3,
                "target_pairs": [{"probability_model_id": "tail", "rival_model_id": "target"}]}}

    def run_campaign(self):
        return C.run(self.f.root, self.f.pins, self.config)

    @unittest.skipUnless(importlib.util.find_spec("scipy"), "Private solver unavailable")
    def test_packet_to_23_holding_negative_mean_frontier_without_sale_credit(self):
        before = {p.name: p.read_bytes() for p in self.f.root.iterdir()}
        out = self.run_campaign()
        self.assertEqual(out["audit_manifest"]["status"], "completed_research_campaign", out["decision_trace"])
        self.assertEqual(out["capacity_frontier"]["dated_static_bound"]["reserved_cash_micros"], 2000000)
        self.assertEqual(out["decision_trace"]["events"][3]["held_sides"], 23)
        candidates = out["rank_candidates"]["candidates"]
        target = [c for c in candidates if c["targets"]["tail|target"]["strict_hit_ppm"] > 0]
        self.assertTrue(target)
        self.assertTrue(all(c["monetary_expectations"]["tail"]["gain_numerator"] < 0 for c in target))
        self.assertTrue(all(c["gross_cost_micros"] <= 8000000 for c in candidates))
        self.assertTrue(all(c["positive_alpha_qualified"] is False for c in candidates))
        self.assertTrue(all(d["execution_authorized"] is False for d in out.values()))
        self.assertEqual(before, {p.name: p.read_bytes() for p in self.f.root.iterdir()})

    def test_missing_debt_preserves_static_bound_and_exact_rejection(self):
        self.config.pop("liabilities")
        out = self.run_campaign()
        self.assertIn("LIABILITY_EVIDENCE_UNAVAILABLE", out["audit_manifest"]["unavailable_reason"])
        self.assertIsNone(out["rank_candidates"]["candidates"])
        self.assertEqual(out["capacity_frontier"]["dated_static_bound"]["cash_micros"], 10000000)

    def test_old_receipts_never_reclocked_to_newer_native_snapshot(self):
        self.f.d["report"]["at"] = "1970-01-01T00:00:01.500Z"
        self.f.refresh()
        out = self.run_campaign()
        self.assertIn("SOURCE_OBSERVATION_PRECEDES_SNAPSHOT", out["audit_manifest"]["unavailable_reason"])
        step = out["decision_trace"]["events"][1]
        self.assertEqual(step["source_observation_ms"], 1001)
        self.assertEqual(step["native_at_original"], "1970-01-01T00:00:01.500Z")

    def test_wrong_pin_fails_before_account_or_solver(self):
        self.f.pins["expected_report_sha256"] = "f" * 64
        out = self.run_campaign()
        self.assertEqual(out["audit_manifest"]["status"], "unavailable")
        self.assertEqual(out["decision_trace"]["events"][-1]["step"], "verify_original_worker_packet")
        self.assertIsNone(out["capacity_frontier"]["dated_static_bound"])

    def test_absent_fees_or_rival_models_stay_explicitly_missing(self):
        for field, code in [("fee_evidence", "EXPLICIT_FEES"), ("rival_models", "ECONOMIC_MODEL_INPUT_UNAVAILABLE")]:
            cfg = deepcopy(self.config)
            cfg.pop(field)
            out = C.run(self.f.root, self.f.pins, cfg)
            self.assertIn(code, out["audit_manifest"]["unavailable_reason"])
            self.assertIsNone(out["rank_candidates"]["candidates"])


if __name__ == "__main__":
    unittest.main()
