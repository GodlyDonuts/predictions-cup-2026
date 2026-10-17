"""Finite buy-only rank-risk research. No API, order authority or fitted model.

Consumes CONTRACTS.md B and the complete canonical account compiler output.
Numerical MILP incumbents undergo independent exact prefix/funding/wealth checks.
Full visible acquisition is a scenario assumption, never a claimed actual fill.
"""
import hashlib
import json
import math
import re
import time
from fractions import Fraction

PPM = UNIT = 1_000_000
LIMIT = 2 ** 52


def need(ok, reason):
    if not ok:
        raise ValueError(reason)


def integer(value, reason, minimum=-LIMIT, maximum=LIMIT):
    need(type(value) is int and minimum <= value <= maximum, reason)
    return value


def identifier(value):
    need(isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}", value) is not None, "IDENTITY_REQUIRED")
    return value


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
                                     ensure_ascii=False, allow_nan=False).encode()).hexdigest()


def sha(value):
    need(isinstance(value, str) and len(value) == 64
         and all(c in "0123456789abcdef" for c in value), "SOURCE_SHA256_REQUIRED")
    return value


def prepare(account, actions, probabilities, rivals, constraints):
    need(isinstance(constraints, dict), "CONSTRAINTS_REQUIRED")
    need(isinstance(account, dict) and type(account.get("schema_version")) is int and account["schema_version"] == 1
         and account.get("execution_authorized") is False
         and account.get("calibrated_probability_claimed") is False, "COMPILED_ACCOUNT_REQUIRED")
    now = integer(constraints.get("decision_ms"), "DECISION_CLOCK", 0)
    start = integer(account.get("as_of_ms"), "ACCOUNT_CLOCK", 0)
    until = integer(account.get("valid_until_ms"), "ACCOUNT_CLOCK", 0)
    need(start <= now < until, "ACCOUNT_EXPIRED_OR_FUTURE")
    sha(account.get("source_sha256"))
    need(isinstance(account.get("evidence_class"), str) and account["evidence_class"], "EVIDENCE_CLASS")
    need(isinstance(account.get("unmodeled_state_statement"), str)
         and account["unmodeled_state_statement"], "UNMODELED_STATES_REQUIRED")
    cash = integer(account.get("cash_micros"), "CASH", 0)
    reserved = integer(account.get("reserved_cash_micros"), "RESERVED_CASH", 0)
    integer(account.get("terminal_debt_micros"), "TERMINAL_DEBT", 0)
    available = integer(account.get("available_cash_micros"), "AVAILABLE_CASH", 0)
    need(available == cash - reserved, "AVAILABLE_CASH_NOT_GROSS_CASH_MINUS_RESERVATIONS")
    ids = account.get("scenario_ids")
    need(isinstance(ids, list) and 1 <= len(ids) <= 4096, "SCENARIO_IDS")
    for value in ids:
        identifier(value)
    need(len(set(ids)) == len(ids), "SCENARIO_IDS")
    baseline = account.get("baseline_wealth_micros")
    need(isinstance(baseline, list) and len(baseline) == len(ids), "BASELINE_DIMENSIONS")
    for value in baseline:
        integer(value, "BASELINE_WEALTH")
    registry = account.get("contracts")
    need(isinstance(registry, list) and 1 <= len(registry) <= 512, "CONTRACT_REGISTRY")
    contracts, exchanges = {}, set()
    for contract in registry:
        cid = identifier(contract.get("contract_id"))
        need(cid not in contracts and contract.get("exchange_id") not in exchanges, "DUPLICATE_CONTRACT")
        identifier(contract.get("exchange_id")); sha(contract.get("semantic_source_sha256"))
        contracts[cid] = contract; exchanges.add(contract["exchange_id"])
    payouts = account.get("scenario_payouts")
    need(isinstance(payouts, list) and len(payouts) == len(ids), "PAYOUT_DIMENSIONS")
    for row in payouts:
        need(isinstance(row, dict) and set(row) == set(contracts)
             and all(x in ("resolved_yes", "resolved_no", "refund_principal") for x in row.values()), "INCOMPLETE_SCENARIO_PAYOUTS")
    # Recheck the complete compiler baseline; neither marked equity nor a caller
    # supplied sale/ACK amount can become acquisition cash here.
    rebuilt = [cash - account["terminal_debt_micros"] for _ in ids]
    holding_keys = set()
    need(isinstance(account.get("holdings"), list), "COMPLETE_HOLDINGS_REQUIRED")
    for h in account["holdings"]:
        key = (h.get("contract_id"), h.get("exchange_id"), h.get("side"))
        need(key not in holding_keys and key[0] in contracts
             and contracts[key[0]]["exchange_id"] == key[1] and key[2] in ("YES", "NO"), "HOLDING_IDENTITY")
        holding_keys.add(key)
        quantity = integer(h.get("quantity"), "HOLDING_QUANTITY", 1)
        lots = h.get("lots")
        need(isinstance(lots, list) and lots, "HOLDING_LOTS")
        principal = sum(integer(l.get("quantity"), "LOT_QUANTITY", 1)
                        * integer(l.get("unit_cost_micros"), "LOT_COST", 0) for l in lots)
        need(sum(l["quantity"] for l in lots) == quantity, "HOLDING_BASIS_MISMATCH")
        for i, row in enumerate(payouts):
            kind = row[key[0]]
            rebuilt[i] += principal if kind == "refund_principal" else quantity * UNIT if (kind == "resolved_yes") == (key[2] == "YES") else 0
    need(rebuilt == baseline, "COMPILED_BASELINE_MISMATCH")
    maximum_cost = min(available, integer(constraints.get("maximum_gross_cost_micros"), "GROSS_COST_BOUND", 0))
    maximum_quantity = integer(constraints.get("maximum_total_quantity"), "TOTAL_QUANTITY_BOUND", 0, 10 ** 9)
    need(isinstance(actions, list) and len(actions) <= 64, "ACTION_BOUND")
    need(len(actions) * len(ids) <= 262144, "MATRIX_BOUND")
    action_ids, identities, columns = set(), set(), []
    for a in actions:
        aid = identifier(a.get("id")); cid = a.get("contract_id"); ex = a.get("exchange_id"); side = a.get("side")
        need(aid not in action_ids and (cid, ex, side) not in identities, "DUPLICATE_ACTION_OR_DEPTH")
        action_ids.add(aid); identities.add((cid, ex, side))
        need(cid in contracts and contracts[cid]["exchange_id"] == ex and side in ("YES", "NO"), "ACTION_CONTRACT_IDENTITY")
        sha(a.get("source_sha256"))
        need(integer(a.get("available_at_ms"), "ACTION_AVAILABLE", 0) <= now
             < integer(a.get("valid_until_ms"), "ACTION_DEADLINE", 0), "ACTION_EXPIRED_OR_FUTURE")
        fee = integer(a.get("fee_per_share_micros"), "ENTRY_FEE", 0, UNIT)
        levels = a.get("levels")
        need(isinstance(levels, list) and 1 <= len(levels) <= 64, "DEPTH_UNAVAILABLE")
        previous = -1
        for j, level in enumerate(levels):
            price = integer(level.get("unit_cost_micros"), "DEPTH_PRICE", 1, UNIT)
            n = integer(level.get("quantity"), "DEPTH_QUANTITY", 1, 10 ** 9)
            need(price > previous, "DEPTH_NOT_STRICT_ASCENDING")
            previous = price
            delta = [(price if p[cid] == "refund_principal" else UNIT if (p[cid] == "resolved_yes") == (side == "YES") else 0) - price - fee for p in payouts]
            columns.append({"action_id": aid, "level": j, "quantity": n, "price": price,
                            "cost": price + fee, "delta": delta})
    need(len(columns) <= 256 and len(columns) * len(ids) <= 262144, "DEPTH_MATRIX_BOUND")
    pmap, rmap = {}, {}
    need(isinstance(probabilities, list) and 1 <= len(probabilities) <= 8, "PROBABILITY_MODELS_UNAVAILABLE")
    for p in probabilities:
        pid = identifier(p.get("id")); w = p.get("weights_ppm")
        need(pid not in pmap and isinstance(w, list) and len(w) == len(ids), "PROBABILITY_DIMENSIONS")
        for x in w:
            integer(x, "PROBABILITY_WEIGHT", 0, PPM)
        need(sum(w) == PPM and isinstance(p.get("assumption_label"), str) and p["assumption_label"], "PROBABILITY_ASSUMPTION")
        pmap[pid] = p
    need(isinstance(rivals, list) and 1 <= len(rivals) <= 8, "RIVAL_MODELS_UNAVAILABLE")
    for r in rivals:
        rid = identifier(r.get("id")); thresholds = r.get("thresholds_micros")
        need(rid not in rmap and isinstance(thresholds, list) and len(thresholds) == len(ids), "RIVAL_DIMENSIONS")
        for x in thresholds:
            integer(x, "RIVAL_THRESHOLD")
        need(r.get("complete_field") is False and isinstance(r.get("assumption_label"), str)
             and r["assumption_label"], "RIVAL_INCOMPLETE_FIELD_ASSUMPTION")
        rmap[rid] = r
    pairs = constraints.get("target_pairs")
    need(isinstance(pairs, list) and 1 <= len(pairs) <= 16, "EXPLICIT_TARGET_PAIRS_REQUIRED")
    keys = []
    for pair in pairs:
        key = (pair.get("probability_model_id"), pair.get("rival_model_id"))
        need(key[0] in pmap and key[1] in rmap and key not in keys, "TARGET_PAIR_IDENTITY")
        keys.append(key)
    need({pid for pid, _ in keys} == set(pmap), "UNPAIRED_PROBABILITY_MODEL")
    minimum_means = constraints.get("minimum_expected_gain_micros_by_model", {})
    need(isinstance(minimum_means, dict) and set(minimum_means) <= set(pmap), "MEAN_CONSTRAINT_MODEL")
    for value in minimum_means.values():
        integer(value, "MEAN_THRESHOLD")
    return dict(account=account, actions=actions, columns=columns, baseline=baseline,
                pmap=pmap, rmap=rmap, pairs=keys, cash=maximum_cost,
                quantity=maximum_quantity, constraints=constraints)


def quantities_to_levels(p, quantities):
    need(isinstance(quantities, dict) and set(quantities) <= {a["id"] for a in p["actions"]}, "CANDIDATE_ACTION_IDENTITY")
    remaining = {a["id"]: integer(quantities.get(a["id"], 0), "CANDIDATE_INTEGER_QUANTITY", 0, 10 ** 9) for a in p["actions"]}
    result = []
    for col in p["columns"]:
        n = min(remaining[col["action_id"]], col["quantity"])
        result.append(n); remaining[col["action_id"]] -= n
    need(not any(remaining.values()), "CANDIDATE_EXCEEDS_ORIGINAL_DEPTH")
    return result


def exact_candidate(p, quantities, label="candidate", required_mean=None, required_floor=None):
    xs = quantities_to_levels(p, quantities)
    gross = sum(n * col["cost"] for n, col in zip(xs, p["columns"]))
    need(gross <= p["cash"] and sum(xs) <= p["quantity"], "CANDIDATE_GROSS_FUNDING_OR_QUANTITY")
    after = [base + sum(n * c["delta"][i] for n, c in zip(xs, p["columns"])) for i, base in enumerate(p["baseline"])]
    need(all(-LIMIT <= x <= LIMIT for x in after), "CANDIDATE_WEALTH_BOUND")
    c = p["constraints"]
    floor = c.get("minimum_wealth_micros")
    if required_floor is not None:
        floor = required_floor if floor is None else max(floor, required_floor)
    if floor is not None:
        need(min(after) >= integer(floor, "MINIMUM_WEALTH"), "CANDIDATE_WEALTH_FLOOR")
    if "maximum_loss_micros" in c:
        loss = integer(c["maximum_loss_micros"], "MAXIMUM_LOSS", 0)
        need(all(a - b >= -loss for a, b in zip(after, p["baseline"])), "CANDIDATE_MAXIMUM_LOSS")
    means = {}
    for pid, model in p["pmap"].items():
        before_num = sum(w * x for w, x in zip(model["weights_ppm"], p["baseline"]))
        after_num = sum(w * x for w, x in zip(model["weights_ppm"], after))
        gain = after_num - before_num
        threshold = c.get("minimum_expected_gain_micros_by_model", {}).get(pid)
        if required_mean is not None:
            threshold = required_mean if threshold is None else max(threshold, required_mean)
        if threshold is not None:
            need(gain >= integer(threshold, "MEAN_THRESHOLD") * PPM, "CANDIDATE_MEAN_FLOOR")
        means[pid] = {"before_numerator": before_num, "after_numerator": after_num,
                      "gain_numerator": gain, "denominator": PPM,
                      "monetary_sign": "positive" if gain > 0 else "negative" if gain < 0 else "zero"}
    targets = {}
    for pid, rid in p["pairs"]:
        w = p["pmap"][pid]["weights_ppm"]; t = p["rmap"][rid]["thresholds_micros"]
        targets[pid + "|" + rid] = {"strict_hit_ppm": sum(weight for weight, wealth, threshold in zip(w, after, t) if wealth > threshold),
                                    "tie_ppm": sum(weight for weight, wealth, threshold in zip(w, after, t) if wealth == threshold),
                                    "first_place_with_ties": None, "expected_prize": None,
                                    "tie_and_prize_rules": "unavailable"}
    signs = {x["monetary_sign"] for x in means.values()}
    classification = "mixed_monetary_expectation" if "negative" in signs and "positive" in signs else "negative_monetary_expectation" if "negative" in signs else "positive_declared_monetary_expectation" if "positive" in signs else "zero_monetary_change"
    quantities = {a["id"]: quantities.get(a["id"], 0) for a in p["actions"]}
    return {"candidate_id": digest({"account": p["account"]["source_sha256"], "actions": p["actions"], "quantities": quantities}),
            "family": "buy_only_full_visible_acquisition", "classification": classification,
            "quantities": quantities, "depth_prefixes": [{"action_id": col["action_id"], "level": col["level"], "quantity": n, "unit_cost_micros": col["price"], "fee_per_share_micros": col["cost"] - col["price"]} for n, col in zip(xs, p["columns"]) if n],
            "gross_cost_micros": gross, "available_cash_before_micros": p["account"]["available_cash_micros"],
            "cash_after_micros": p["account"]["cash_micros"] - gross,
            "whole_account_wealth_before_micros": p["baseline"], "whole_account_wealth_after_micros": after,
            "scenario_ids": p["account"]["scenario_ids"], "contract_ids": [x["contract_id"] for x in p["account"]["contracts"]],
            "semantic_source_sha256": {x["contract_id"]: x["semantic_source_sha256"] for x in p["account"]["contracts"]},
            "monetary_expectations": means, "targets": targets, "minimum_wealth_micros": min(after),
            "worst_delta_micros": min(a - b for a, b in zip(after, p["baseline"])),
            "source_sha256": p["account"]["source_sha256"], "action_source_sha256": {a["id"]: a["source_sha256"] for a in p["actions"]},
            "decision_ms": p["constraints"]["decision_ms"],
            "source_available_at_ms": {"account": p["account"]["as_of_ms"], "actions": {a["id"]: a["available_at_ms"] for a in p["actions"]}},
            "valid_until_ms": min([p["account"]["valid_until_ms"]] + [a["valid_until_ms"] for a in p["actions"]]),
            "evidence_class": p["account"]["evidence_class"], "unmodeled_state_statement": p["account"]["unmodeled_state_statement"],
            "fill_scenarios": {"full_visible_acquisition": "assumed_for_candidate", "zero_or_partial_execution": "not_modeled"},
            "model_assumptions": {k: v["assumption_label"] for k, v in p["pmap"].items()},
            "rival_assumptions": {k: v["assumption_label"] for k, v in p["rmap"].items()},
            "capital_time": None, "sale_confirmation_dependencies": [],
            "positive_alpha_qualified": False, "execution_authorized": False, "declared_constraints_feasible": True,
            "calibrated_probability_claimed": False, "generated_by": [label]}


def evaluate_candidate(compiled_account, actions, probability_models, rival_models, constraints, quantities):
    """Exact research recheck; useful even when SciPy is unavailable."""
    return exact_candidate(prepare(compiled_account, actions, probability_models, rival_models, constraints), quantities)


def capacity_upper(p):
    result = []
    for i, base in enumerate(p["baseline"]):
        positive = [col for col in p["columns"] if col["delta"][i] > 0]
        cash = p["cash"]; gain = Fraction(0)
        for col in sorted(positive, key=lambda c: Fraction(c["delta"][i], c["cost"]), reverse=True):
            take = min(Fraction(col["quantity"]), Fraction(cash, col["cost"]))
            gain += take * col["delta"][i]; cash -= take * col["cost"]
        qleft = p["quantity"]; qgain = 0
        for col in sorted(positive, key=lambda c: c["delta"][i], reverse=True):
            take = min(col["quantity"], qleft); qgain += take * col["delta"][i]; qleft -= take
        result.append(base + min(gain.numerator // gain.denominator, qgain))
    return result


def nondominated(candidates):
    def axes(c):
        return ([x["gain_numerator"] for x in c["monetary_expectations"].values()]
                + [x["strict_hit_ppm"] for x in c["targets"].values()]
                + [c["minimum_wealth_micros"], -c["gross_cost_micros"]])
    values = [axes(c) for c in candidates]
    return [c for i, c in enumerate(candidates) if not any(j != i and all(a >= b for a, b in zip(values[j], values[i]))
             and any(a > b for a, b in zip(values[j], values[i])) for j in range(len(candidates)))]


def solve_frontier(compiled_account, actions, probability_models, rival_models, constraints):
    p = prepare(compiled_account, actions, probability_models, rival_models, constraints)
    # No-action remains visible even when a caller's proposed sweep excludes it.
    no_action_p = {**p, "constraints": {k: v for k, v in constraints.items() if k not in ("minimum_wealth_micros", "minimum_expected_gain_micros_by_model")}}
    baseline = exact_candidate(no_action_p, {}, "no_action")
    try:
        exact_candidate(p, {})
    except ValueError as error:
        baseline.update(declared_constraints_feasible=False, declared_constraint_rejection=str(error))
    upper = capacity_upper(p)
    out = {"schema_version": 1, "baseline": baseline, "candidates": [baseline], "frontier": [baseline],
           "capacity_upper_wealth_micros": upper, "solver_runs": [], "sweeps_not_run": 0,
           "probability_models": probability_models, "rival_models": rival_models,
           "constraints": constraints, "execution_authorized": False, "calibrated_probability_claimed": False,
           "positive_alpha_production_lane_modified": False, "sale_funded_rotation": "requires_confirmed_proceeds_then_new_depth_and_account_compile"}
    try:
        import numpy as np
        from scipy.optimize import Bounds, LinearConstraint, milp
        from scipy.sparse import coo_matrix
    except ImportError:
        out.update(status="unavailable", unavailable_reason="MILP_SOLVER_UNAVAILABLE")
        return out
    seconds = integer(constraints.get("solver_time_limit_seconds", 3), "SOLVER_TIME_BOUND", 1, 30)
    total_seconds = integer(constraints.get("total_solver_time_limit_seconds", 30), "TOTAL_SOLVER_TIME_BOUND", 1, 300)
    started = time.monotonic()
    maximum_calls = integer(constraints.get("maximum_solver_calls", 48), "SOLVER_CALL_BOUND", 1, 128)
    mean_sweeps = constraints.get("mean_gain_thresholds_micros", [])
    risk_sweeps = constraints.get("minimum_wealth_thresholds_micros", [])
    need(isinstance(mean_sweeps, list) and len(mean_sweeps) <= 8 and isinstance(risk_sweeps, list) and len(risk_sweeps) <= 8, "FRONTIER_SWEEP_BOUND")
    for value in mean_sweeps + risk_sweeps:
        integer(value, "FRONTIER_SWEEP_THRESHOLD")
    cols = p["columns"]; n = len(cols); scenarios = len(p["baseline"])
    prefixes = [(j - 1, j) for j in range(n) if cols[j]["level"] > 0]
    hit_start = n + len(prefixes); variables = hit_start + scenarios
    lower = [0] * variables; high = [c["quantity"] for c in cols] + [1] * (variables - n)
    targets = []
    for pair in p["pairs"]:
        targets.extend([("rank", pair, None, None), ("minimum_cost_target", pair, None, None)])
    targets.extend(("mean", next(pair for pair in p["pairs"] if pair[0] == pid), None, None) for pid in p["pmap"])
    targets.extend(("rank", pair, mean, None) for mean in mean_sweeps for pair in p["pairs"])
    targets.extend(("rank", pair, None, floor) for floor in risk_sweeps for pair in p["pairs"])
    retained = {baseline["candidate_id"]: baseline}
    for run_index, (goal, pair, mean_floor, risk_floor) in enumerate(targets):
        if run_index >= maximum_calls or time.monotonic() - started >= total_seconds:
            out["sweeps_not_run"] += 1
            continue
        pid, rid = pair; weights = p["pmap"][pid]["weights_ppm"]; thresholds = p["rmap"][rid]["thresholds_micros"]
        rows, lows, highs = [], [], []
        def row(values, lo=-np.inf, hi=np.inf):
            rows.append(values); lows.append(lo); highs.append(hi)
        row({j: c["cost"] / UNIT for j, c in enumerate(cols)}, hi=p["cash"] / UNIT)
        row({j: 1 for j in range(n)}, hi=p["quantity"])
        for k, (previous, current) in enumerate(prefixes):
            z = n + k
            row({current: 1, z: -cols[current]["quantity"]}, hi=0)
            row({previous: 1, z: -cols[previous]["quantity"]}, lo=0)
        for i, (base, threshold) in enumerate(zip(p["baseline"], thresholds)):
            minimum = base + sum(min(c["delta"][i], 0) * c["quantity"] for c in cols)
            maximum = base + sum(max(c["delta"][i], 0) * c["quantity"] for c in cols)
            big_low = max(0, threshold + 1 - minimum); big_high = max(0, maximum - threshold)
            values = {j: c["delta"][i] / UNIT for j, c in enumerate(cols)}
            row({**values, hit_start + i: -big_low / UNIT}, lo=(threshold + 1 - base - big_low) / UNIT)
            row({**values, hit_start + i: -big_high / UNIT}, hi=(threshold - base) / UNIT)
            floor = constraints.get("minimum_wealth_micros")
            if risk_floor is not None:
                floor = risk_floor if floor is None else max(floor, risk_floor)
            if floor is not None:
                row(values, lo=(integer(floor, "MINIMUM_WEALTH") - base) / UNIT)
            if "maximum_loss_micros" in constraints:
                row(values, lo=-integer(constraints["maximum_loss_micros"], "MAXIMUM_LOSS", 0) / UNIT)
        for model_id, model in p["pmap"].items():
            bound = constraints.get("minimum_expected_gain_micros_by_model", {}).get(model_id)
            if mean_floor is not None:
                bound = mean_floor if bound is None else max(bound, mean_floor)
            if bound is not None:
                row({j: float(Fraction(sum(w * d for w, d in zip(model["weights_ppm"], c["delta"])), PPM * UNIT)) for j, c in enumerate(cols)}, lo=integer(bound, "MEAN_THRESHOLD") / UNIT)
        objective = [0.] * variables
        if goal == "rank":
            for i, w in enumerate(weights):
                objective[hit_start + i] = -w
        elif goal == "minimum_cost_target":
            minimum_hit = integer(constraints.get("minimum_target_probability_ppm", 1), "MINIMUM_TARGET_PPM", 1, PPM)
            # Keep this in integer ppm: dividing a one-ppm bound to 1e-6
            # lets a numerical feasibility tolerance swallow the whole target.
            row({hit_start + i: w for i, w in enumerate(weights)}, lo=minimum_hit)
            objective[:n] = [c["cost"] / UNIT for c in cols]
        else:
            objective[:n] = [-float(Fraction(sum(w * d for w, d in zip(weights, c["delta"])), PPM * UNIT)) for c in cols]
        rr, cc, data = [], [], []
        for i, values in enumerate(rows):
            for j, value in values.items():
                if value:
                    rr.append(i); cc.append(j); data.append(value)
        matrix = coo_matrix((data, (rr, cc)), shape=(len(rows), variables)).tocsc()
        report = {"goal": goal, "probability_model_id": pid, "rival_model_id": rid,
                  "minimum_mean_gain_micros": mean_floor, "minimum_wealth_micros": risk_floor}
        try:
            result = milp(np.array(objective), integrality=np.ones(variables), bounds=Bounds(lower, high),
                          constraints=LinearConstraint(matrix, lows, highs),
                          options={"time_limit": max(.001, min(seconds, total_seconds - (time.monotonic() - started))), "mip_rel_gap": 0.0, "presolve": True})
            report.update(solver_status=int(result.status), solver_message=str(result.message),
                          numerical_optimum_reported=result.status == 0, exact_optimality_certified=False,
                          inconclusive=result.status not in (0, 2))
            if result.x is not None:
                need(all(math.isfinite(float(x)) and abs(float(x) - round(float(x))) <= 1e-5 for x in result.x), "SOLVER_NONINTEGER_INCUMBENT")
                xs = [int(round(float(x))) for x in result.x[:n]]
                q = {a["id"]: sum(xs[j] for j, col in enumerate(cols) if col["action_id"] == a["id"]) for a in actions}
                need(xs == quantities_to_levels(p, q), "SOLVER_PREFIX_RECHECK")
                candidate = exact_candidate(p, q, goal + ":" + pid + "|" + rid, mean_floor, risk_floor)
                if goal == "minimum_cost_target":
                    need(candidate["targets"][pid + "|" + rid]["strict_hit_ppm"] >= constraints.get("minimum_target_probability_ppm", 1), "SOLVER_TARGET_RECHECK")
                for i, h in enumerate(result.x[hit_start:]):
                    need(round(float(h)) == int(candidate["whole_account_wealth_after_micros"][i] > thresholds[i]), "SOLVER_TARGET_RECHECK")
                existing = retained.get(candidate["candidate_id"])
                if existing:
                    existing["generated_by"] = sorted(set(existing["generated_by"] + candidate["generated_by"]))
                else:
                    retained[candidate["candidate_id"]] = candidate
                report.update(candidate_id=candidate["candidate_id"], exact_integer_recheck="passed")
            elif result.status not in (0, 2):
                report["inconclusive"] = True
        except (ValueError, RuntimeError) as error:
            report.update(exact_integer_recheck="rejected", rejection_reason=str(error)[:512], inconclusive=True)
        out["solver_runs"].append(report)
    out["candidates"] = list(retained.values()); out["frontier"] = nondominated(out["candidates"])
    out["target_outcomes"] = {}
    for pid, rid in p["pairs"]:
        key = pid + "|" + rid; model = p["pmap"][pid]; thresholds = p["rmap"][rid]["thresholds_micros"]
        upper_hit = sum(w for w, x, t in zip(model["weights_ppm"], upper, thresholds) if x > t)
        found = max(c["targets"][key]["strict_hit_ppm"] for c in out["candidates"])
        out["target_outcomes"][key] = {"best_found_hit_ppm": found, "analytical_capacity_hit_upper_ppm": upper_hit,
            "status": "attainable_under_full_acquisition_assumption" if found else "unattainable_under_declared_cash_depth_quantity" if upper_hit == 0 else "no_attainment_found_inconclusive",
            "unattainability_proof": "exact_fractional_relaxation_upper_bound" if upper_hit == 0 else None}
    out["status"] = "research_candidates_with_exact_feasibility_checks"
    out["frontier_exhaustive"] = False
    return out
