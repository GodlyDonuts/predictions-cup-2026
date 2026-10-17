"""Offline retained-worker packet to complete campaign decision or exact rejection.

This connects the existing research components; it collects no data and grants
no execution authority. Registry, terminal liabilities, scenario probabilities,
field models and fees are explicit supplied research evidence/assumptions. Their
meaning and calibration are not authenticated by this adapter or a byte hash.
Original account/book clocks are retained, never replaced by invocation time.
"""
import argparse
import hashlib
import json
from pathlib import Path

try:
    from . import worker_packet as W, native_snapshot as N, depth_actions as D
    from . import account_scenarios as A, target_frontier as B, census
except ImportError:
    import worker_packet as W, native_snapshot as N, depth_actions as D
    import account_scenarios as A, target_frontier as B, census

PIN_NAMES = {"expected_metadata_sha256", "expected_control_sha256",
             "expected_generation_sha256", "expected_report_sha256", "expected_head_sha256"}
MAX_CONFIG_BYTES = 16 * 1024 * 1024


def run(packet_directory, pins, config):
    """Four private research artifacts; no fallback model, debt or sale proceeds.

    Required config: decision_ms, evidence_class. Optional economic inputs:
    valid_until_ms, registry, liabilities, fee_evidence, maximum_book_age_ms,
    buy_sides, scenarios, probability_models, rival_models, constraints. Missing
    inputs produce specific unavailable stages. buy_sides explicitly selects at
    most one BUY side per exchange; separate SELL curves never finance the solver.
    """
    trace = []
    docs = {
        "audit_manifest": {"schema_version": 1, "external_packet_pins": pins,
            "source_code_sha256": {Path(m.__file__).name: census.sha(Path(m.__file__).read_bytes())
                                  for m in (W, N, D, A, B)},
            "config_sha256": census.sha(census.canonical(config)),
            "input_authentication": "Packet bytes/chains/receipts checked; SDK/lease assertions conditional on producer. Economic input semantics and calibration remain supplied, not independently authenticated."},
        "capacity_frontier": {"schema_version": 1, "dated_static_bound": None, "depth": None, "frontier": None},
        "rank_candidates": {"schema_version": 1, "candidates": None, "winning_probability": None},
        "decision_trace": {"schema_version": 1, "events": trace}}
    docs["audit_manifest"]["source_code_sha256"][Path(__file__).name] = census.sha(Path(__file__).read_bytes())
    status, reason, stage = "unavailable", None, "input"
    try:
        if type(pins) is not dict or set(pins) != PIN_NAMES:
            raise ValueError("EXACT_FIVE_EXTERNAL_PINS_REQUIRED")
        if type(config) is not dict or type(config.get("evidence_class")) is not str or not config["evidence_class"]:
            raise ValueError("EXPLICIT_EVIDENCE_CLASS_REQUIRED")
        if config.get("execution_authorized") is True or config.get("calibrated_probability_claimed") is True:
            raise ValueError("UNSUPPORTED_AUTHORITY_OR_CALIBRATION_CLAIM")
        decision = N._int(config.get("decision_ms"), "DECISION_CLOCK")
        stage = "verify_original_worker_packet"
        packet = W.verify_packet(packet_directory, **pins)
        docs["audit_manifest"].update(evidence_class=config["evidence_class"], packet_provenance=dict(packet.provenance),
            bank_head=packet.head, generation_sha256=pins["expected_generation_sha256"])
        trace.append({"step": stage, "status": "passed", "execution_authorized": False})
        stage = "adapt_complete_native_account"
        native = N.adapt_native_snapshot(packet.report_bytes,
            expected_report_sha256=pins["expected_report_sha256"],
            expected_manifest_sha256=packet.head["manifestHash"], expected_head=packet.head,
            source_observation_ms=packet.provenance["source_observation_ms"],
            decision_ms=decision, valid_until_ms=config.get("valid_until_ms"),
            registry=config.get("registry"), liabilities=config.get("liabilities"),
            draft_classification=packet.draft_classification)
        docs["capacity_frontier"]["dated_static_bound"] = native["static_held_bounds"]
        trace.append({"step": stage, "status": native["status"], "reasons": native["unavailable_reasons"],
                      "source_observation_ms": packet.provenance["source_observation_ms"],
                      "native_at_original": native["snapshot"]["at_original"]})
        if native["account_input"] is None:
            raise ValueError("NATIVE_ACCOUNT_UNAVAILABLE:" + ",".join(native["unavailable_reasons"]))
        stage = "adapt_captured_depth"
        if any(config.get(k) is None for k in ("fee_evidence", "maximum_book_age_ms", "buy_sides")):
            raise ValueError("EXPLICIT_FEES_BOOK_AGE_AND_BUY_SELECTION_REQUIRED")
        depth = D.from_packet(packet, expected_metadata_sha256=pins["expected_metadata_sha256"],
            registry=config["registry"], fee_evidence=config["fee_evidence"], decision_ms=decision,
            maximum_book_age_ms=config["maximum_book_age_ms"])
        docs["capacity_frontier"]["depth"] = depth
        actions = D.select_buy_actions(depth, config["buy_sides"])
        trace.append({"step": stage, "status": depth["status"], "selected_buy_actions": len(actions),
                      "market_rejections": depth["rejections"], "sale_proceeds_credited": False})
        stage = "compile_whole_account_scenarios"
        if config.get("scenarios") is None:
            raise ValueError("JOINT_SCENARIO_INPUT_UNAVAILABLE")
        compiled = A.compile_account(native["account_input"], config["scenarios"], decision)
        trace.append({"step": stage, "status": "passed", "held_sides": len(compiled["holdings"]),
                      "scenario_count": len(compiled["scenario_ids"]), "available_cash_micros": compiled["available_cash_micros"]})
        stage = "search_explicit_rank_risk_frontier"
        missing = [k for k in ("probability_models", "rival_models", "constraints") if config.get(k) is None]
        if missing:
            raise ValueError("ECONOMIC_MODEL_INPUT_UNAVAILABLE:" + ",".join(missing))
        if config["constraints"].get("decision_ms") != decision:
            raise ValueError("CONSTRAINT_DECISION_CLOCK_MISMATCH")
        frontier = B.solve_frontier(compiled, actions, config["probability_models"],
                                    config["rival_models"], config["constraints"])
        docs["capacity_frontier"]["frontier"] = frontier
        docs["rank_candidates"]["candidates"] = frontier["candidates"]
        trace.append({"step": stage, "status": frontier["status"], "candidate_count": len(frontier["candidates"]),
                      "calibrated_probability_claimed": False})
        status = "completed_research_campaign" if frontier["status"] != "unavailable" else "unavailable"
        if status == "unavailable":
            reason = frontier.get("unavailable_reason", "FRONTIER_UNAVAILABLE")
    except (ValueError, KeyError, TypeError) as error:
        reason = str(error)
        trace.append({"step": stage, "status": "unavailable", "reason": reason})
    for doc in docs.values():
        doc.update(status=status, unavailable_reason=reason, execution_authorized=False,
                   calibrated_probability_claimed=False)
    return docs


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--packet-directory", required=True)
    parser.add_argument("--input", required=True, help="Private JSON: {pins: five external pins, config: explicit inputs}")
    parser.add_argument("--input-sha256", required=True)
    parser.add_argument("--output-dir", required=True)
    args = parser.parse_args()
    path = Path(args.input)
    if not path.is_file() or path.is_symlink() or path.stat().st_size > MAX_CONFIG_BYTES:
        raise ValueError("PRIVATE_INPUT_FILE_BOUND_OR_TYPE")
    raw = path.read_bytes()
    if len(raw) > MAX_CONFIG_BYTES or hashlib.sha256(raw).hexdigest() != args.input_sha256:
        raise ValueError("PRIVATE_INPUT_RAW_SHA256")
    bundle = json.loads(raw)
    docs = run(args.packet_directory, bundle["pins"], bundle["config"])
    docs["audit_manifest"]["input_file_sha256"] = args.input_sha256
    audit = census.write_outputs(docs, args.output_dir)
    print(json.dumps({"status": audit["status"], "reason": audit["unavailable_reason"],
                      "execution_authorized": False}))
    return 0 if audit["status"] == "completed_research_campaign" else 2


if __name__ == "__main__":
    raise SystemExit(main())
