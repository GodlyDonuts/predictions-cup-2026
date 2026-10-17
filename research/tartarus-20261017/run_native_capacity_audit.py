"""Frozen native packet -> dated capacity bounds and explicit missing-input census.

No network, bank mutation, probabilities, resolver invention or physical actions.
Raw positions remain in the private packet. Published outputs contain aggregates.
"""
import argparse
from datetime import datetime
import importlib.util
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from moloch.tartarus import census, native_snapshot


def audit(packet_path, packet_sha256, pid):
    spec = importlib.util.spec_from_file_location(
        "tartarus_original_packet", ROOT / "research/moloch-source-latency-20261017/compare_book_clocks.py")
    reader = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(reader)
    packet = reader.Packet(packet_path, packet_sha256)
    try:
        capture = packet.json("capture.json")
        if capture.get("pid") != pid or capture.get("zeroAccountCalls") is not True:
            raise ValueError("CAPTURE_PROCESS_OR_READ_ONLY_BINDING")
        report = packet.sealed("report.original.json")
        head = packet.sealed("head.original.json")
        observed = datetime.fromisoformat(capture["observedAt"].replace("Z", "+00:00"))
        if observed.tzinfo is None:
            raise ValueError("CAPTURE_OBSERVATION_TIMEZONE")
        epoch = datetime.fromisoformat("1970-01-01T00:00:00+00:00")
        delta = observed - epoch
        decision_ms = (delta.days * 86400 + delta.seconds) * 1000 + delta.microseconds // 1000
        if capture.get("sequence") != report["sequence"] or capture.get("reportAt") != report["report"]["at"]:
            raise ValueError("CAPTURE_REPORT_BOUNDARY")
        result = native_snapshot.adapt_native_snapshot(
            packet.small(packet.name("report.original.json")),
            expected_report_sha256=packet.original_pin("report.original.json")["sha256"],
            expected_manifest_sha256=head["manifestHash"], expected_head=head,
            source_observation_ms=decision_ms, decision_ms=decision_ms,
            valid_until_ms=None, registry=None, liabilities=None)
        model = packet.json("model-status.original.json")
        provenance = {
            "packet_sha256": packet_sha256, "original_member_manifest_sha256": packet.manifest_sha,
            "original_member_pins": packet.pins, "actual_pid_at_capture": pid,
            "captured_at": capture["observedAt"], "report_at": capture["reportAt"],
            "bank_head": head, "deployed_sha": capture["sourceCommit"],
            "generation_sha256": model.get("previousGenerationSha256"),
            "native_adapter_status": result["status"],
            "native_adapter_unavailable_reasons": result["unavailable_reasons"],
            "native_adapter_source_sha256": census.sha(Path(native_snapshot.__file__).read_bytes()),
            "driver_source_sha256": census.sha(Path(__file__).read_bytes()),
            "actual_official_liabilities_independently_verified": False,
            "underlying_receipt_membership_independently_verified": False,
            "execution_authorized": False, "calibrated_probability_claimed": False}
        # Missing inputs stay absent. Neither the packet's hash nor its capture
        # time supplies a current forecast, semantics certificate or debt=0.
        bundle = {"schema_version": 1, "constraints": {"decision_ms": decision_ms},
                  "evidence_manifest": {"schema_version": 1,
                    "evidence_class": "dated_native_packet_incomplete_research_inputs",
                    "main_sha": None, "deployed_sha": capture["sourceCommit"],
                    "generation_sha256": model.get("previousGenerationSha256"),
                    "bank_head": head, "sources": []}}
        documents = census.run_census(bundle)
        bound = result["static_held_bounds"]
        ceiling = bound["cash_plus_all_sides_pay_one_ceiling_micros"]
        documents["capacity_frontier"]["dated_native_static_bound"] = {
            **bound, "fixed_holdings_only": True,
            "terminal_debt_unknown": True, "future_fees_excluded": True,
            "future_acquisition_or_rotation_capacity": None,
            "threshold_stresses": [
                {"declared_target_micros": target * 1000000,
                 "fixed_holdings_ceiling_reaches_target": ceiling >= target * 1000000,
                 "shortfall_from_loose_ceiling_micros": max(0, target * 1000000 - ceiling)}
                for target in census.THRESHOLDS],
            "thresholds_are_current_leaderboard_observations": False}
        documents["audit_manifest"]["native_packet_provenance"] = provenance
        documents["decision_trace"]["events"].append({
            "step": "sealed_native_adapter", "status": result["status"],
            "unavailable_reasons": result["unavailable_reasons"],
            "account_input_created": result["account_input"] is not None,
            "actual_forecast_or_rank_scores_created": False})
        return documents
    finally:
        packet.finish()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--packet", required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--pid", required=True, type=int)
    parser.add_argument("--output-dir", required=True)
    args = parser.parse_args()
    documents = audit(args.packet, args.sha256, args.pid)
    result = census.write_outputs(documents, args.output_dir)
    print(json.dumps({"status": result["status"],
        "reason": result["unavailable_reason"],
        "static_bounds_verified": True, "execution_authorized": False}))


if __name__ == "__main__":
    main()
