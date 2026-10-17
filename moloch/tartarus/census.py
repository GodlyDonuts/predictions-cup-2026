"""Thin, offline Tartarus census: explicit immutable inputs, no trading authority.

Bundle: {schema_version:1, account, scenarios, depth_actions,
 probability_models, rival_models, constraints, evidence_manifest}.
Manifest: {schema_version:1,evidence_class,main_sha,deployed_sha,
 generation_sha256,bank_head,sources:[{source_sha256,reference,
 available_at_ms,valid_until_ms,evidence_class,path?}]}.
Actual source paths are relative regular files under an explicit evidence_root;
no URL is fetched. Reference-only synthetic sources require synthetic=True.
Clocks and source authenticity remain declared, not authenticated by a hash.

CLI (new output directory; no overwrite):
 python -m moloch.tartarus.census --input bundle.json --input-sha256 SHA
   --output-dir census-run [--evidence-root DIR] [--synthetic]
 python -m moloch.tartarus.census --synthetic --demo --output-dir demo-run
Without explicit --demo, missing inputs produce unavailable reports, not examples.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re

try:
    from . import account_scenarios as A, target_frontier as B
except ImportError:
    import account_scenarios as A
    import target_frontier as B

REQUIRED = ("account", "scenarios", "depth_actions", "probability_models",
            "rival_models", "constraints", "evidence_manifest")
THRESHOLDS = (925300, 1000000, 1086100, 1500000, 2000000)
MAX_INPUT_BYTES = 64 * 1024 ** 2
SHA = re.compile(r"[0-9a-f]{64}\Z")
COMMIT = re.compile(r"[0-9a-f]{40}\Z")


class Unavailable(ValueError):
    pass


def need(ok, code):
    if not ok:
        raise Unavailable(code)


def canonical(value):
    return (json.dumps(value, sort_keys=True, separators=(",", ":"),
                       ensure_ascii=False, allow_nan=False) + "\n").encode()


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def integer(value, code):
    need(type(value) is int and 0 <= value < 2 ** 63, code)
    return value


def source_hashes(value):
    result = set()
    if isinstance(value, dict):
        for key, item in value.items():
            if key in ("source_sha256", "semantic_source_sha256"):
                need(isinstance(item, str) and SHA.fullmatch(item), "SOURCE_SHA256")
                result.add(item)
            else:
                result.update(source_hashes(item))
    elif isinstance(value, list):
        for item in value:
            result.update(source_hashes(item))
    return result


def evidence(bundle, root, synthetic, decision_ms):
    manifest = bundle["evidence_manifest"]
    need(isinstance(manifest, dict) and type(manifest.get("schema_version")) is int
         and manifest["schema_version"] == 1, "EVIDENCE_MANIFEST_SCHEMA")
    need(isinstance(manifest.get("evidence_class"), str) and manifest["evidence_class"], "EVIDENCE_CLASS")
    for name in ("main_sha", "deployed_sha"):
        need(manifest.get(name) is None or isinstance(manifest[name], str)
             and COMMIT.fullmatch(manifest[name]), "INVALID_" + name.upper())
    need(manifest.get("generation_sha256") is None or isinstance(manifest["generation_sha256"], str)
         and SHA.fullmatch(manifest["generation_sha256"]), "INVALID_GENERATION_SHA256")
    refs = manifest.get("sources")
    need(isinstance(refs, list) and len(refs) <= 256, "EVIDENCE_SOURCES_UNAVAILABLE")
    wanted = source_hashes({k: bundle.get(k) for k in REQUIRED if k != "evidence_manifest"})
    found, checked, total_bytes = set(), [], 0
    for item in refs:
        need(isinstance(item, dict), "EVIDENCE_SOURCE_SHAPE")
        pin = item.get("source_sha256")
        need(isinstance(pin, str) and SHA.fullmatch(pin) and pin not in found, "EVIDENCE_SOURCE_HASH_DUPLICATE_OR_INVALID")
        found.add(pin)
        reference = item.get("reference")
        need(isinstance(reference, str) and 0 < len(reference) <= 4096, "EVIDENCE_REFERENCE_REQUIRED")
        start = integer(item.get("available_at_ms"), "SOURCE_AVAILABLE_CLOCK")
        end = integer(item.get("valid_until_ms"), "SOURCE_DEADLINE_CLOCK")
        need(start <= decision_ms < end, "SOURCE_EXPIRED_OR_FUTURE")
        label = item.get("evidence_class")
        need(isinstance(label, str) and label, "SOURCE_EVIDENCE_CLASS")
        row = {k: item[k] for k in ("source_sha256", "reference", "available_at_ms", "valid_until_ms", "evidence_class")}
        if "synthetic" in label.lower():
            need(synthetic, "SYNTHETIC_INPUT_REQUIRES_EXPLICIT_OPT_IN")
            row["body_verification"] = "not_applicable_declared_synthetic"
        else:
            need(root is not None and isinstance(item.get("path"), str), "ORIGINAL_SOURCE_BODY_UNAVAILABLE")
            relative = Path(item["path"])
            need(not relative.is_absolute() and ".." not in relative.parts, "SOURCE_PATH_OUTSIDE_EVIDENCE_ROOT")
            base = Path(root).resolve(); file = base / relative
            need(file.resolve().is_relative_to(base), "SOURCE_PATH_OUTSIDE_EVIDENCE_ROOT")
            for parent in [file, *file.parents]:
                if parent == base:
                    break
                need(not parent.is_symlink(), "SOURCE_SYMLINK")
            try:
                before = file.stat()
                need(file.is_file() and before.st_size <= MAX_INPUT_BYTES, "SOURCE_FILE_BOUND_OR_TYPE")
                raw = file.read_bytes(); after = file.stat()
            except OSError as error:
                raise Unavailable("ORIGINAL_SOURCE_BODY_UNAVAILABLE") from error
            need((before.st_ino, before.st_size, before.st_mtime_ns) == (after.st_ino, after.st_size, after.st_mtime_ns), "SOURCE_CHANGED_DURING_READ")
            need(sha(raw) == pin, "ORIGINAL_SOURCE_HASH_MISMATCH")
            total_bytes += len(raw); need(total_bytes <= 512 * 1024 ** 2, "EVIDENCE_TOTAL_BYTES_BOUND")
            row.update(body_verification="original_bytes_match_declared_sha256", bytes=len(raw))
        checked.append(row)
    need(wanted <= found, "SOURCE_WITNESS_MISSING:" + ",".join(sorted(wanted - found)))
    return checked


def category(code):
    if "EXPIRED" in code or "FUTURE" in code:
        return "stale_or_noncausal_source"
    if "PENDING_OBLIGATION" in code:
        return "unmodeled_obligations"
    if "SYNTHETIC" in code or "UNSUPPORTED" in code or "COMPLETE_FIELD" in code:
        return "unsupported_claim"
    if "SOLVER" in code:
        return "solver_unavailable_or_inconclusive"
    return "missing_or_invalid_evidence"


def run_census(bundle, *, evidence_root=None, synthetic=False, input_raw=None,
               expected_input_sha256=None):
    """Return four JSON-compatible documents, including explicit refusals."""
    need(type(synthetic) is bool, "SYNTHETIC_FLAG")
    actual_sha = sha(input_raw) if input_raw is not None else None
    trace = []
    manifest = bundle.get("evidence_manifest", {}) if isinstance(bundle, dict) else {}
    manifest = manifest if isinstance(manifest, dict) else {}
    audit = {"schema_version": 1, "input_file_sha256": actual_sha,
             "input_canonical_sha256": sha(canonical(bundle)),
             "expected_input_file_sha256": expected_input_sha256,
             "synthetic_opt_in": synthetic, "main_sha": manifest.get("main_sha"),
             "deployed_sha": manifest.get("deployed_sha"), "generation_sha256": manifest.get("generation_sha256"),
             "bank_head": manifest.get("bank_head"), "source_witnesses": [],
             "source_code_sha256": {Path(m.__file__).name: sha(Path(m.__file__).read_bytes()) for m in (A, B)},
             "source_authentication": "not_established_by_hash_or_owner_declared_clocks",
             "execution_authorized": False, "calibrated_probability_claimed": False}
    audit["source_code_sha256"][Path(__file__).name] = sha(Path(__file__).read_bytes())
    documents = {"audit_manifest": audit,
                 "capacity_frontier": {"schema_version": 1, "static_bound": None, "frontier": None},
                 "rank_candidates": {"schema_version": 1, "candidates": None, "winning_probability": None},
                 "decision_trace": {"schema_version": 1, "events": trace}}
    compiled = None
    try:
        need(isinstance(bundle, dict) and type(bundle.get("schema_version")) is int
             and bundle["schema_version"] == 1, "BUNDLE_SCHEMA")
        if input_raw is not None:
            need(len(input_raw) <= MAX_INPUT_BYTES and json.loads(input_raw) == bundle, "RAW_INPUT_BUNDLE_MISMATCH")
        if expected_input_sha256 is not None:
            need(isinstance(expected_input_sha256, str) and SHA.fullmatch(expected_input_sha256)
                 and actual_sha == expected_input_sha256, "INPUT_SHA_MISMATCH")
        for node in [bundle, manifest, *bundle.get("probability_models", []), *bundle.get("rival_models", [])]:
            if isinstance(node, dict):
                need(node.get("execution_authorized") is not True
                     and node.get("calibrated_probability_claimed") is not True, "UNSUPPORTED_AUTHORITY_OR_CALIBRATION_CLAIM")
        evidence_labels = [manifest.get("evidence_class"), bundle.get("account", {}).get("evidence_class"), bundle.get("scenarios", {}).get("evidence_class")]
        need(synthetic or not any(isinstance(x, str) and "synthetic" in x.lower() for x in evidence_labels), "SYNTHETIC_INPUT_REQUIRES_EXPLICIT_OPT_IN")
        decision = bundle.get("constraints", {}).get("decision_ms")
        audit["decision_ms"] = decision
        if all(k in bundle for k in ("account", "scenarios", "constraints")):
            compiled = A.compile_account(bundle["account"], bundle["scenarios"], decision)
            audit.update(compiled_account_sha256=compiled["source_sha256"],
                         as_of_ms=compiled["as_of_ms"], valid_until_ms=compiled["valid_until_ms"])
            documents["capacity_frontier"]["static_bound"] = {
                "kind": "complete_declared_matrix_no_further_trade_only",
                "minimum_modeled_wealth_micros": min(compiled["baseline_wealth_micros"]),
                "maximum_modeled_wealth_micros": max(compiled["baseline_wealth_micros"]),
                "available_acquisition_cash_micros": compiled["available_cash_micros"],
                "scenario_matrix_exhaustive": False, "original_source_authentication": "see_audit_manifest"}
            trace.append({"step": "compile_complete_account", "status": "passed",
                          "contracts": len(compiled["contracts"]), "holdings": len(compiled["holdings"]),
                          "scenario_ids": compiled["scenario_ids"]})
        missing = [key for key in REQUIRED if key not in bundle or bundle[key] is None]
        need(not missing, "MISSING_INPUTS:" + ",".join(missing))
        audit["source_witnesses"] = evidence(bundle, evidence_root, synthetic, decision)
        trace.append({"step": "source_byte_pins_and_declared_availability", "status": "passed",
                      "source_count": len(audit["source_witnesses"]), "independent_clock_authentication": False})
        result = B.solve_frontier(compiled, bundle["depth_actions"], bundle["probability_models"],
                                  bundle["rival_models"], bundle["constraints"])
        documents["capacity_frontier"].update({k: result.get(k) for k in
            ("baseline", "frontier", "capacity_upper_wealth_micros", "target_outcomes", "frontier_exhaustive", "sale_funded_rotation")})
        documents["rank_candidates"].update(candidates=result["candidates"],
            scenario_ids=compiled["scenario_ids"], probability_models=bundle["probability_models"],
            rival_models=bundle["rival_models"], actual_fill_claimed=False,
            economic_qualification="declared_scenario_research_only")
        trace.extend({"step": "numerical_candidate_search", **row} for row in result["solver_runs"])
        trace.append({"step": "opposing_field", "status": "incomplete_declared_models",
                      "complete_field_claimed": False, "private_opponent_portfolios_known": False})
        for pair, case in result.get("target_outcomes", {}).items():
            trace.append({"step": "target_capacity", "target_pair": pair, **case,
                          "actual_available_cash_micros": compiled["available_cash_micros"],
                          "cash_and_depth_are_joint_bounds": True})
        for candidate in result["candidates"]:
            trace.append({"step": "candidate_classification", "candidate_id": candidate["candidate_id"],
                          "classification": candidate["classification"], "gross_cost_micros": candidate["gross_cost_micros"],
                          "positive_alpha_qualified": False})
        if result["status"] == "unavailable":
            raise Unavailable(result["unavailable_reason"])
        status = "completed_research_census"
        reason = None
    except (ValueError, TypeError, KeyError, AttributeError) as error:
        status = "unavailable"
        reason = str(error)[:512]
        trace.append({"step": "census_refusal", "category": category(reason), "reason": reason})
    for doc in documents.values():
        doc.update(status=status, unavailable_reason=reason, execution_authorized=False,
                   calibrated_probability_claimed=False)
    return documents


def synthetic_demo():
    """Explicit 23-holding, 8-state synthetic replay; no actual market evidence."""
    pin = sha(b"Tartarus explicitly synthetic census example v1")
    common = {"schema_version": 1, "evidence_class": "synthetic",
              "as_of_ms": 1000, "valid_until_ms": 3000, "source_sha256": pin,
              "unmodeled_state_statement": "Declared synthetic matrix; no calibrated election or fill claim."}
    registry = [{"contract_id": "c" + str(i), "exchange_id": "e" + str(i), "semantic_source_sha256": pin} for i in range(24)]
    holdings = [{"contract_id": x["contract_id"], "exchange_id": x["exchange_id"], "side": "YES", "quantity": 1,
                 "lots": [{"quantity": 1, "unit_cost_micros": 400000}], "source_sha256": pin} for x in registry[:23]]
    rows = [{"id": "state" + str(s), "kind": "ordinary",
             "payouts": {x["contract_id"]: "resolved_yes" if s & (1 << (i % 3)) else "resolved_no" for i, x in enumerate(registry)},
             "assumption_label": "Explicit synthetic joint binary pattern, not independent observations."} for s in range(8)]
    models = [{"id": name, "weights_ppm": [1_000_000 - p, 0, 0, 0, p, 0, 0, 0],
               "assumption_label": "Synthetic " + name + " probability assumption, not estimated."} for name, p in [("six_percent", 60000), ("twelve_percent", 120000)]]
    rivals = [{"id": "fixed_" + str(target), "thresholds_micros": [target * 1_000_000 - 1] * 8,
               "complete_field": False, "assumption_label": "Declared >= " + str(target) + " target; NOT a current leaderboard or terminal forecast."} for target in THRESHOLDS]
    return {"schema_version": 1,
            "account": {**common, "cash_micros": 94737 * 1_000_000, "reserved_cash_micros": 0, "terminal_debt_micros": 0, "holdings": holdings, "pending_obligations": []},
            "scenarios": {**common, "contracts": registry, "scenarios": rows},
            "depth_actions": [{"id": "synthetic_buy", "contract_id": "c23", "exchange_id": "e23", "side": "YES", "levels": [{"quantity": 1000000, "unit_cost_micros": 80000}], "fee_per_share_micros": 0, "source_sha256": pin, "available_at_ms": 1000, "valid_until_ms": 3000}],
            "probability_models": models, "rival_models": rivals,
            "constraints": {"decision_ms": 2000, "maximum_gross_cost_micros": 94737 * 1_000_000, "maximum_total_quantity": 1000000,
                            "maximum_solver_calls": 32, "target_pairs": [{"probability_model_id": p["id"], "rival_model_id": r["id"]} for p in models for r in rivals]},
            "evidence_manifest": {"schema_version": 1, "evidence_class": "synthetic", "main_sha": None, "deployed_sha": None,
                                  "generation_sha256": None, "bank_head": None,
                                  "sources": [{"source_sha256": pin, "reference": "synthetic:explicit_demo_v1", "available_at_ms": 1000, "valid_until_ms": 3000, "evidence_class": "synthetic"}]}}


def write_outputs(documents, output_dir):
    """Write a new finite result directory. Never overwrite prior evidence."""
    target = Path(output_dir); target.mkdir(parents=True, exist_ok=False)
    names = ("capacity_frontier", "rank_candidates", "decision_trace")
    pins = {}
    for name in names:
        raw = canonical(documents[name]); file = target / (name + ".json")
        with file.open("xb") as fd:
            fd.write(raw)
        pins[file.name] = {"sha256": sha(raw), "bytes": len(raw)}
    audit = {**documents["audit_manifest"], "outputs": pins}
    with (target / "audit_manifest.json").open("xb") as fd:
        fd.write(canonical(audit))
    return audit


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input"); parser.add_argument("--input-sha256")
    parser.add_argument("--evidence-root"); parser.add_argument("--output-dir", required=True)
    parser.add_argument("--synthetic", action="store_true"); parser.add_argument("--demo", action="store_true")
    args = parser.parse_args(argv)
    if args.demo:
        if not args.synthetic or args.input:
            parser.error("--demo requires --synthetic and cannot use --input")
        bundle = synthetic_demo(); raw = None
    elif args.input:
        file = Path(args.input); need(file.is_file() and file.stat().st_size <= MAX_INPUT_BYTES, "INPUT_FILE_BOUND_OR_TYPE")
        raw = file.read_bytes(); bundle = json.loads(raw)
        need(args.input_sha256 is not None, "IMMUTABLE_INPUT_SHA256_REQUIRED")
    else:
        bundle = {"schema_version": 1}; raw = None
    result = run_census(bundle, evidence_root=args.evidence_root or (str(Path(args.input).resolve().parent) if args.input else None),
                        synthetic=args.synthetic, input_raw=raw, expected_input_sha256=args.input_sha256)
    audit = write_outputs(result, args.output_dir)
    print(json.dumps({"status": audit["status"], "unavailable_reason": audit["unavailable_reason"],
                      "audit_manifest_sha256": sha(canonical(audit)), "execution_authorized": False}))
    return 0 if audit["status"] == "completed_research_census" else 2


if __name__ == "__main__":
    raise SystemExit(main())
