"""Census orchestration tests. All accounts/prices/probabilities are synthetic."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from moloch.tartarus.census import run_census, synthetic_demo, write_outputs, canonical, THRESHOLDS

HAS_SCIPY = importlib.util.find_spec("scipy") is not None


class CensusRefusals(unittest.TestCase):
    def test_no_inputs_yields_missing_not_zero_ev_or_winning_probability(self):
        result = run_census({"schema_version": 1})
        self.assertEqual(result["audit_manifest"]["status"], "unavailable")
        self.assertIn("MISSING_INPUTS:account,scenarios", result["audit_manifest"]["unavailable_reason"])
        self.assertIsNone(result["rank_candidates"]["candidates"])
        self.assertIsNone(result["rank_candidates"]["winning_probability"])
        self.assertIsNone(result["capacity_frontier"]["static_bound"])

    def test_demo_requires_explicit_opt_in_and_never_pretends_actual_account(self):
        result = run_census(synthetic_demo())
        self.assertIn("SYNTHETIC_INPUT_REQUIRES_EXPLICIT_OPT_IN", result["audit_manifest"]["unavailable_reason"])
        self.assertIsNone(result["rank_candidates"]["candidates"])

    def test_missing_probability_keeps_only_known_compiled_static_bound(self):
        bundle = synthetic_demo(); del bundle["probability_models"]
        result = run_census(bundle, synthetic=True)
        self.assertIn("MISSING_INPUTS:probability_models", result["audit_manifest"]["unavailable_reason"])
        bound = result["capacity_frontier"]["static_bound"]
        self.assertEqual(bound["available_acquisition_cash_micros"], 94_737_000000)
        self.assertFalse(bound["scenario_matrix_exhaustive"])
        self.assertIsNone(result["rank_candidates"]["candidates"])

    def test_unmodeled_liability_and_stale_account_are_distinct(self):
        bundle = synthetic_demo(); bundle["account"]["pending_obligations"] = [{"unknown_order": True}]
        result = run_census(bundle, synthetic=True)
        self.assertEqual(result["decision_trace"]["events"][-1]["category"], "unmodeled_obligations")
        self.assertIsNone(result["capacity_frontier"]["static_bound"])
        bundle = synthetic_demo(); bundle["account"]["valid_until_ms"] = 1999
        result = run_census(bundle, synthetic=True)
        self.assertEqual(result["decision_trace"]["events"][-1]["category"], "stale_or_noncausal_source")

    def test_raw_input_mutation_and_unsupported_authority_refuse_before_scoring(self):
        bundle = synthetic_demo(); raw = canonical(bundle)
        result = run_census(bundle, synthetic=True, input_raw=raw, expected_input_sha256="0" * 64)
        self.assertEqual(result["audit_manifest"]["unavailable_reason"], "INPUT_SHA_MISMATCH")
        self.assertEqual(result["audit_manifest"]["input_file_sha256"], hashlib.sha256(raw).hexdigest())
        bundle["probability_models"][0]["calibrated_probability_claimed"] = True
        result = run_census(bundle, synthetic=True)
        self.assertEqual(result["decision_trace"]["events"][-1]["category"], "unsupported_claim")
        for doc in result.values():
            self.assertFalse(doc["execution_authorized"])
            self.assertFalse(doc["calibrated_probability_claimed"])

    def test_non_synthetic_source_requires_original_bytes_no_api_or_clock_renewal(self):
        bundle = synthetic_demo(); ref = bundle["evidence_manifest"]["sources"][0]
        ref["evidence_class"] = "original_byte_fixture"
        result = run_census(bundle, synthetic=True)
        self.assertEqual(result["audit_manifest"]["unavailable_reason"], "ORIGINAL_SOURCE_BODY_UNAVAILABLE")
        ref["available_at_ms"] = 2001
        result = run_census(bundle, synthetic=True)
        self.assertEqual(result["audit_manifest"]["unavailable_reason"], "SOURCE_EXPIRED_OR_FUTURE")

    def test_declared_source_path_cannot_escape_evidence_root(self):
        bundle = synthetic_demo(); ref = bundle["evidence_manifest"]["sources"][0]
        ref.update(evidence_class="original_byte_fixture", path="../private.json")
        with tempfile.TemporaryDirectory(prefix="tartarus-census-path-") as directory:
            result = run_census(bundle, synthetic=True, evidence_root=directory)
        self.assertEqual(result["audit_manifest"]["unavailable_reason"], "SOURCE_PATH_OUTSIDE_EVIDENCE_ROOT")


@unittest.skipUnless(HAS_SCIPY, "Optional private SciPy environment unavailable")
class CensusEndToEnd(unittest.TestCase):
    def test_declared_23_holding_eight_state_demo_and_five_fixed_target_cases(self):
        bundle = synthetic_demo(); before = copy.deepcopy(bundle)
        result = run_census(bundle, synthetic=True)
        self.assertEqual(result["audit_manifest"]["status"], "completed_research_census", result["decision_trace"])
        self.assertEqual(bundle, before)
        first = result["decision_trace"]["events"][0]
        self.assertEqual(first["holdings"], 23)
        self.assertEqual(first["contracts"], 24)
        self.assertEqual(len(first["scenario_ids"]), 8)
        self.assertGreater(first["contracts"], 6)  # Exercises new compiler, not a claimed Rust replay.
        self.assertEqual(len(result["capacity_frontier"]["target_outcomes"]), 10)
        self.assertEqual({x["id"] for x in bundle["rival_models"]}, {"fixed_" + str(t) for t in THRESHOLDS})
        for target in (1086100, 1500000, 2000000):
            case = result["capacity_frontier"]["target_outcomes"]["six_percent|fixed_" + str(target)]
            self.assertIn("unattainable", case["status"])
            self.assertEqual(case["analytical_capacity_hit_upper_ppm"], 0)
        self.assertTrue(any(c["classification"] == "mixed_monetary_expectation" for c in result["rank_candidates"]["candidates"]))
        self.assertIsNone(result["rank_candidates"]["winning_probability"])
        self.assertFalse(result["audit_manifest"]["execution_authorized"])

    def test_source_byte_pins_and_source_generation_deployed_pins_survive(self):
        bundle = synthetic_demo(); old = bundle["account"]["source_sha256"]
        raw = b"Retained byte-verification fixture; NOT an actual account source.\n"
        pin = hashlib.sha256(raw).hexdigest()
        def replace(value):
            if isinstance(value, dict):
                return {k: replace(v) for k, v in value.items()}
            if isinstance(value, list):
                return [replace(v) for v in value]
            return pin if value == old else value
        bundle = replace(bundle)
        m = bundle["evidence_manifest"]; m.update(main_sha="1" * 40, deployed_sha="2" * 40, generation_sha256="3" * 64, bank_head={"sequence": 123, "hash": "4" * 64})
        m["sources"][0].update(evidence_class="original_byte_fixture", path="source.original.txt")
        with tempfile.TemporaryDirectory(prefix="tartarus-census-pins-") as directory:
            source = Path(directory) / "source.original.txt"; source.write_bytes(raw)
            result = run_census(bundle, synthetic=True, evidence_root=directory)
            self.assertEqual(result["audit_manifest"]["status"], "completed_research_census", result["decision_trace"])
            witness = result["audit_manifest"]["source_witnesses"][0]
            self.assertEqual(witness["body_verification"], "original_bytes_match_declared_sha256")
            self.assertEqual(witness["available_at_ms"], 1000)
            self.assertEqual(result["audit_manifest"]["generation_sha256"], "3" * 64)
            self.assertEqual(result["audit_manifest"]["deployed_sha"], "2" * 40)
            source.write_bytes(raw + b"changed")
            bad = run_census(bundle, synthetic=True, evidence_root=directory)
            self.assertEqual(bad["audit_manifest"]["unavailable_reason"], "ORIGINAL_SOURCE_HASH_MISMATCH")
            source.unlink()
            bad = run_census(bundle, synthetic=True, evidence_root=directory)
            self.assertEqual(bad["audit_manifest"]["unavailable_reason"], "ORIGINAL_SOURCE_BODY_UNAVAILABLE")

    def test_stale_depth_and_missing_models_do_not_become_zero_ev(self):
        bundle = synthetic_demo(); bundle["depth_actions"][0]["valid_until_ms"] = 2000
        result = run_census(bundle, synthetic=True)
        self.assertEqual(result["audit_manifest"]["status"], "unavailable")
        self.assertEqual(result["decision_trace"]["events"][-1]["category"], "stale_or_noncausal_source")
        self.assertIsNone(result["rank_candidates"]["candidates"])
        bundle = synthetic_demo(); bundle["probability_models"] = []
        result = run_census(bundle, synthetic=True)
        self.assertIn("PROBABILITY_MODELS_UNAVAILABLE", result["audit_manifest"]["unavailable_reason"])
        self.assertIsNone(result["rank_candidates"]["winning_probability"])

    def test_result_four_file_hashes_and_refusal_to_overwrite(self):
        result = run_census(synthetic_demo(), synthetic=True)
        with tempfile.TemporaryDirectory(prefix="tartarus-census-output-") as directory:
            target = Path(directory) / "run"
            audit = write_outputs(result, target)
            self.assertEqual(set(x.name for x in target.iterdir()), {"audit_manifest.json", "capacity_frontier.json", "rank_candidates.json", "decision_trace.json"})
            for name, pin in audit["outputs"].items():
                self.assertEqual(hashlib.sha256((target / name).read_bytes()).hexdigest(), pin["sha256"])
            with self.assertRaises(FileExistsError):
                write_outputs(result, target)

    def test_cli_is_explicit_and_reproducible_with_pinned_input(self):
        with tempfile.TemporaryDirectory(prefix="tartarus-census-cli-") as directory:
            root = Path(directory); bundle = synthetic_demo(); raw = canonical(bundle)
            source = root / "bundle.json"; source.write_bytes(raw)
            common = [sys.executable, "-B", "-m", "moloch.tartarus.census"]
            missing = subprocess.run(common + ["--output-dir", str(root / "missing")], capture_output=True, text=True)
            self.assertEqual(missing.returncode, 2, missing.stderr)
            self.assertEqual(json.loads(missing.stdout)["status"], "unavailable")
            command = common + ["--input", str(source), "--input-sha256", hashlib.sha256(raw).hexdigest(), "--synthetic"]
            first = subprocess.run(command + ["--output-dir", str(root / "one")], capture_output=True, text=True)
            second = subprocess.run(command + ["--output-dir", str(root / "two")], capture_output=True, text=True)
            self.assertEqual(first.returncode, 0, first.stderr + first.stdout)
            self.assertEqual(second.returncode, 0, second.stderr + second.stdout)
            self.assertEqual(first.stdout, second.stdout)
            self.assertEqual((root / "one" / "audit_manifest.json").read_bytes(), (root / "two" / "audit_manifest.json").read_bytes())


if __name__ == "__main__":
    unittest.main()
