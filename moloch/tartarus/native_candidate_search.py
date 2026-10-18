"""Search the full universe subject to exact native scenario capacity.

No held contract or scenario is removed. Lossless coalescing cardinality is
monotone as candidate contracts are added, so an over-budget combination gives
a valid exclusion for every later incumbent containing that combination.
"""
from time import monotonic
from .sequence_audit import need
from .sequential_frontier import solve


def scenario_keys(model, fields, contract_ids):
    ids = sorted(contract_ids)
    return {(row["kind"], tuple(row["payouts"][cid] for cid in ids),
             tuple(field["thresholds"][i] for field in fields))
            for i, row in enumerate(model["scenarios"])}


def minimal_over_budget(held, extra, byex, model, fields, limit=128):
    """Return an irreducible forbidden combination; held scope is immutable."""
    count = lambda xs: len(scenario_keys(model, fields, held | {byex[ex] for ex in xs}))
    need(count(extra) > limit and count([]) <= limit, "EXCLUSION_PRECONDITION")
    kept = sorted(extra)
    for ex in list(kept):
        smaller = [x for x in kept if x != ex]
        if count(smaller) > limit:
            kept = smaller
    return kept


def solve_routable(financial, registry, books, model, fields, *, target_pairs,
                   seconds=120, per_trial_seconds=20, required_covered_sale_exchanges=()):
    """Retain all trials and exact cuts; a time limit is not an impossibility proof."""
    need(0 < seconds <= 600 and 0 < per_trial_seconds <= 300, "NATIVE_SEARCH_BUDGET")
    byex = {r["exchangeId"]: r["contractId"] for r in registry}
    held = {byex[ex] for ex in financial["positions"]}
    base = len(scenario_keys(model, fields, held))
    out = {"schema": "tartarus_native_capacity_search_v1", "heldContractCount": len(held),
        "heldOnlyScenarioCount": base, "nativeScenarioLimit": 128, "trials": [],
        "forbiddenNewMarketSets": [], "candidate": None, "executionAuthority": False,
        "heldContractsRemoved": False, "scenariosRemoved": False,
        "allInputModelsAndFieldsPreserved": True}
    if base > 128:
        out.update(status="native_capacity_insufficient_for_current_holdings")
        return out
    started = monotonic()
    while monotonic() - started < seconds:
        remaining = seconds - (monotonic() - started)
        candidate = solve(financial, registry, books, model, fields, target_pairs=target_pairs,
            objective="rank", seconds=min(per_trial_seconds, remaining),
            forbidden_new_market_sets=out["forbiddenNewMarketSets"],
            required_covered_sale_exchanges=required_covered_sale_exchanges)
        trial = {"candidate": candidate}
        out["trials"].append(trial)
        if candidate["status"] != "exact_sequence_checked":
            out.update(status="solver_or_exact_audit_unresolved")
            return out
        extra = sorted({leg["ex"] for leg in candidate["legs"] if leg["ex"] not in financial["positions"]})
        count = len(scenario_keys(model, fields, held | {byex[ex] for ex in extra}))
        trial["losslessScenarioCount"] = count
        if "nativeProjection" in candidate:
            out.update(status="native_routable_candidate" if candidate["targetObjectiveImproved"]
                       else "native_routable_no_target_improvement", candidate=candidate)
            return out
        if count <= 128:
            out.update(status="other_native_projection_constraint", reason=candidate.get("nativeProjectionUnavailable"))
            return out
        forbidden = minimal_over_budget(held, extra, byex, model, fields)
        need(forbidden not in out["forbiddenNewMarketSets"], "REPEATED_NATIVE_SCOPE_EXCLUSION")
        out["forbiddenNewMarketSets"].append(forbidden)
        trial["exactExclusion"] = forbidden
    out.update(status="bounded_search_inconclusive")
    return out
