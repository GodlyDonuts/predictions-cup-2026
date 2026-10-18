"""Bounded whole-account SELL/BUY search over declared joint target outcomes.

Offline research only. Quotes can be dated future-liquidity hypotheses. This
module cannot retire reservations, approve a campaign or place an order. Every
incumbent is independently checked against original FIFO and shared book depth.
"""
from copy import deepcopy
from math import gcd
from functools import reduce
from time import monotonic

from .sequence_audit import UNIT, amount, audit, need
from .scenario_projection import project


ASSUMPTIONS = {
    "zeroFees": True, "zeroTerminalDebt": True,
    "conditionalReservationTreatment": "Conditional retirement of positively proved untouched drafts; no retirement performed by this search.",
    "datedBooksAreFutureLiquidityHypotheses": True,
}


def _chunks(book, side, action, cap, *, quantity=None, skip=0, budget=None):
    """Conservative physical orders priced at their worst consumed level."""
    key = "asks" if (action == "BUY") == (side == "YES") else "bids"
    levels = sorted(((amount(r["price"]), r["quantity"]) for r in book[key]),
                    reverse=key == "bids")
    pending = 0
    for yes_price, available in levels:
        price = yes_price if side == "YES" else UNIT - yes_price
        need(0 < price < UNIT and type(available) is int and available > 0, "BOOK_LEVEL")
        used = min(skip, available); skip -= used; available -= used
        if quantity is not None:
            available = min(available, quantity)
        while available:
            take = min(available, cap - pending)
            available -= take; pending += take
            if quantity is not None:
                quantity -= take
            if pending == cap:
                n = min(pending, budget // price) if budget is not None else pending
                if n:
                    yield n, price
                if budget is not None:
                    budget -= n * price
                    if n < pending:
                        return
                pending = 0
        if quantity == 0:
            break
    if pending:
        n = min(pending, budget // price) if budget is not None else pending
        if n:
            yield n, price


def _columns(fin, registry, books):
    byex = {r["exchangeId"]: r for r in registry}
    cap = fin["physicalOrderQuantity"]
    cols, sales, buys = [], {}, {}
    # A retained inherited market becomes fresh once its original lot is sold.
    # Free existing fresh slots first so a feasible final portfolio does not
    # fail its intermediate market-count limit solely through iteration order.
    inherited = set(fin["inheritedExchanges"])
    for ex in sorted(fin["positions"], key=lambda ex: ex in inherited):
        sides = fin["positions"][ex]
        for side, lots in sides.items():
            quantity = sum(l["quantity"] for l in lots)
            if not quantity or ex not in books:
                continue
            original = deepcopy(lots)
            for n, price in _chunks(books[ex].get("book", books[ex]), side, "SELL", cap, quantity=quantity):
                left, basis = n, 0
                for lot in original:
                    take = min(left, lot["quantity"])
                    lot["quantity"] -= take; left -= take
                    basis += take * amount(lot["price"])
                    if not left:
                        break
                need(left == 0, "SELL_EXCEEDS_ORIGINAL_FIFO")
                indices = sales.setdefault((ex, side), [])
                cols.append(dict(action="SELL", ex=ex, side=side, n=n, price=price,
                                 basis=basis, previous=indices[-1] if indices else None))
                indices.append(len(cols) - 1)
    for ex in byex:
        if ex not in books:
            continue
        if ex in fin.get("excludedBuyExchanges", []):
            continue
        for side in ("YES", "NO"):
            opposite = "NO" if side == "YES" else "YES"
            opposite_quantity = sum(l["quantity"] for l in fin["positions"].get(ex, {}).get(opposite, []))
            dependencies = sales.get((ex, opposite), [])
            if opposite_quantity and sum(cols[j]["n"] for j in dependencies) != opposite_quantity:
                continue
            # A complement purchase consumes the same physical depth as that
            # full opposite-side sale. Reserve those original levels only once.
            for n, price in _chunks(books[ex].get("book", books[ex]), side, "BUY", cap,
                                     skip=opposite_quantity, budget=amount(fin["maximumMarketCost"])):
                indices = buys.setdefault((ex, side), [])
                cols.append(dict(action="BUY", ex=ex, side=side, n=n, price=price,
                                 previous=indices[-1] if indices else None,
                                 opposite_sale=dependencies[-1] if opposite_quantity else None))
                indices.append(len(cols) - 1)
    return cols, sales, buys


def solve(financial, registry, books, model, fields, *, target_pairs,
          objective="rank", seconds=20, minimum_expected_change=None,
          minimum_terminal_wealth=None, forbidden_new_market_sets=(),
          required_covered_sale_exchanges=(), maximum_native_scenarios=None,
          required_target_hits=(), minimum_model_expected_changes=None,
          ordinary_probability_inputs=None, minimum_interval_expected_change=None):
    """Search both BUY sides and FIFO SELL prefixes; report actual model scores.

    target_pairs is an explicit nonempty list of (modelId, fieldId). ``rank``
    maximizes the equal-weight average of their declared strict-hit masses;
    ``robust_rank`` maximizes their minimum; ``mean`` maximizes the weakest
    declared monetary change. Negative-mean alternatives remain identifiable.
    All supplied fields survive scoring, including unoptimized adverse fields.
    A time-limited incumbent is never a claim of global portfolio optimality.
    Optional model-specific money floors name every supplied model, allowing
    a winning-state-preserving comparison without sacrificing another mean.
    ``interval_mean`` additionally maximizes the exact ordinary worst-case
    expectation over explicitly supplied categorical marginal intervals. It
    does not calibrate those intervals or replace the exceptional-world audit.
    """
    import numpy as np
    from scipy.optimize import Bounds, LinearConstraint, milp
    from scipy.sparse import coo_matrix

    need(objective in {"rank", "robust_rank", "mean", "interval_mean"}, "OBJECTIVE")
    interval_requested = ordinary_probability_inputs is not None
    need(not (objective == "interval_mean" or minimum_interval_expected_change is not None)
         or interval_requested, "ORDINARY_INTERVAL_INPUTS_REQUIRED")
    if interval_requested:
        need(type(ordinary_probability_inputs) is dict
             and set(ordinary_probability_inputs) == {"marginals", "profiles", "diagnostics"}, "ORDINARY_INTERVAL_INPUTS")
    need(type(seconds) in (int, float) and 0 < seconds <= 300, "TIME_BOUND")
    baseline_audit = audit(financial, registry, books, model, fields, [], assumptions=ASSUMPTIONS)
    worlds = model["scenarios"]
    need(len(worlds) <= 4096 and len(registry) <= 512, "MATRIX_BOUND")
    pmap = {m["id"]: m for m in model["probabilityModels"]}
    fmap = {f["id"]: f for f in fields}
    need(len(pmap) == len(model["probabilityModels"]) and len(fmap) == len(fields), "DUPLICATE_MODEL")
    model_floors = {}
    if minimum_model_expected_changes is not None:
        need(isinstance(minimum_model_expected_changes, dict)
             and set(minimum_model_expected_changes) == set(pmap), "COMPLETE_MODEL_MEAN_FLOORS")
        model_floors = {mid: amount(value) for mid, value in minimum_model_expected_changes.items()}
    need(1 <= len(target_pairs) <= 16 and len(set(target_pairs)) == len(target_pairs)
         and all(p in pmap and f in fmap for p, f in target_pairs), "TARGET_PAIRS")
    cols, sales, buys = _columns(financial, registry, books)
    need(len(cols) <= 8192 and len(cols) * len(worlds) <= 8_000_000, "COLUMN_BOUND")
    byex = {r["exchangeId"]: r for r in registry}
    baseline = np.asarray(baseline_audit["terminalWealthMicros"], dtype=np.float64) / UNIT
    N = len(cols)
    upper = [v["n"] if v["action"] == "BUY" else 1 for v in cols]
    lower = [0] * N; integer = [1] * N

    def variable(low=0, high=1, integral=1):
        j = len(upper); upper.append(high); lower.append(low); integer.append(integral)
        return j

    z = {j: variable() for j, v in enumerate(cols) if v["action"] == "BUY"}
    active = {ex: variable() for ex in sorted(set(financial["positions"]) | {v["ex"] for v in cols}
                                            | set(financial.get("retainedMarketExchanges", [])))}
    choices = {ex: variable() for ex in sorted({ex for ex, side in buys})}
    inherited_quantities = financial.get("inheritedRemainingQuantities")
    inherited_survives = {ex: variable() for ex in inherited_quantities} if inherited_quantities is not None else {}
    inherited_fresh = {ex: variable() for ex in inherited_survives}
    hits = {(f, i): variable() for f in sorted({f for _, f in target_pairs}) for i in range(len(worlds))}
    monetary_objective = objective in {"mean", "interval_mean"}
    tcol = variable(-1e9 if monetary_objective else 0,
                    1e9 if monetary_objective else UNIT, 0)
    rows, low, high = [], [], []

    def row(values, lo=-np.inf, hi=np.inf):
        rows.append(values); low.append(lo); high.append(hi)

    # Target-conditioned feasibility retains every world, field and model;
    # it never increases the selected world's probability.
    world_indices = {world["id"]: i for i, world in enumerate(worlds)}
    for fid, sid in required_target_hits:
        need(sid in world_indices and (fid, world_indices[sid]) in hits,
             "REQUIRED_TARGET_HIT")
        row({hits[fid, world_indices[sid]]: 1}, lo=1)

    for forbidden in forbidden_new_market_sets:
        need(type(forbidden) in (list, tuple, set) and forbidden
             and len(set(forbidden)) == len(forbidden)
             and all(ex in active and ex not in financial["positions"] for ex in forbidden),
             "NATIVE_SCOPE_EXCLUSION")
        row({active[ex]: 1 for ex in forbidden}, hi=len(forbidden)-1)
    for ex in required_covered_sale_exchanges:
        starts = [js[0] for (sx, _), js in sales.items() if sx == ex and js]
        need(ex in financial["positions"] and starts, "REQUIRED_COVERED_SALE_UNAVAILABLE")
        row({j: 1 for j in starts}, lo=1)

    if maximum_native_scenarios is not None:
        need(type(maximum_native_scenarios) is int and 1 <= maximum_native_scenarios <= 256,
             "NATIVE_SCENARIO_BUDGET")
        held_ids = sorted(byex[ex]["contractId"] for ex in financial["positions"])
        new_buys = {}
        for j in z:
            if cols[j]["ex"] not in financial["positions"]:
                new_buys.setdefault(cols[j]["ex"], []).append(z[j])
        selected = {}
        for ex, flags in new_buys.items():
            selected[ex] = variable()
            for flag in flags:
                row({selected[ex]: 1, flag: -1}, lo=0)
            row({selected[ex]: 1, **{flag: -1 for flag in flags}}, hi=0)
        row({flag: 1 for flag in selected.values()}, hi=64-len(held_ids))
        groups = {}
        for i, world in enumerate(worlds):
            signature = (world["kind"], tuple(world["payouts"][cid] for cid in held_ids),
                         tuple(field["thresholds"][i] for field in fields))
            groups.setdefault(signature, []).append(i)
        need(len(groups) <= maximum_native_scenarios, "HELD_MODEL_EXCEEDS_NATIVE_SCENARIO_BOUND")
        representatives, equality_flags = [], {}
        for indices in groups.values():
            prior, seen = [], set()
            for i in indices:
                full = tuple(worlds[i]["payouts"][byex[ex]["contractId"]] for ex in sorted(selected))
                if full in seen:
                    continue
                seen.add(full)
                if prior:
                    same_flags = []
                    for earlier in prior:
                        differences = tuple(ex for ex in sorted(selected)
                            if worlds[i]["payouts"][byex[ex]["contractId"]]
                            != worlds[earlier]["payouts"][byex[ex]["contractId"]])
                        need(differences, "NATIVE_PARTITION_DUPLICATE")
                        if differences not in equality_flags:
                            same = variable(); equality_flags[differences] = same
                            for ex in differences:
                                row({same: 1, selected[ex]: 1}, hi=1)
                            row({same: 1, **{selected[ex]: 1 for ex in differences}}, lo=1)
                        same_flags.append(equality_flags[differences])
                    representative = variable(); representatives.append(representative)
                    # Duplicated equality flags still represent distinct earlier rows.
                    coefficients = {representative: 1}
                    for same in same_flags:
                        coefficients[same] = coefficients.get(same, 0) + 1
                        row({representative: 1, same: 1}, hi=1)
                    row(coefficients, lo=1)
                prior.append(i)
        row({flag: 1 for flag in representatives}, hi=maximum_native_scenarios-len(groups))

    cash, owned, writes, market = {}, {}, {}, {}
    deltas = np.zeros((len(worlds), N))
    exact_deltas = [[0] * N for _ in worlds]
    for j, v in enumerate(cols):
        ex, p, n = v["ex"], v["price"], v["n"]
        market.setdefault(ex, {})
        if v["action"] == "SELL":
            cash[j] = -p * n / UNIT; owned[j] = -v["basis"] / UNIT
            market[ex][j] = owned[j]; writes[j] = 1
            if v["previous"] is not None:
                row({j: 1, v["previous"]: -1}, hi=0)
        else:
            cash[j] = p / UNIT; owned[j] = p / UNIT; market[ex][j] = p / UNIT
            writes[z[j]] = 1
            row({j: 1, z[j]: -n}, hi=0); row({j: 1, z[j]: -1}, lo=0)
            if ex in financial.get("buyRequiresPriorCoveredSellExchanges", []):
                first_sales = [js[0] for (sx, _), js in sales.items() if sx == ex and js]
                row({z[j]: 1, **{s: -1 for s in first_sales}}, hi=0)
            if v["previous"] is not None:
                row({v["previous"]: 1, z[j]: -cols[v["previous"]]["n"]}, lo=0)
            if v["opposite_sale"] is not None:
                row({z[j]: 1, v["opposite_sale"]: -1}, hi=0)
            row({z[j]: 1, active[ex]: -1}, hi=0)
            # Both sides are offered to the solver; it may acquire only one.
            row({z[j]: 1, choices[ex]: -1 if v["side"] == "YES" else 1},
                hi=0 if v["side"] == "YES" else 1)
        cid = byex[ex]["contractId"]
        for i, world in enumerate(worlds):
            outcome = world["payouts"][cid]
            payout = UNIT if outcome == ("resolved_yes" if v["side"] == "YES" else "resolved_no") else 0
            if v["action"] == "SELL":
                delta = n * p - (v["basis"] if outcome == "refund_principal" else n * payout)
            else:
                delta = 0 if outcome == "refund_principal" else payout - p
            exact_deltas[i][j] = delta
            deltas[i, j] = delta / UNIT

    for ex, a in active.items():
        if ex in financial.get("retainedMarketExchanges", []):
            row({a: 1}, lo=1)
        original = sum(l["quantity"] for lots in financial["positions"].get(ex, {}).values() for l in lots)
        indices = [j for (sx, side), js in sales.items() if sx == ex for j in js]
        if original:
            if indices and sum(cols[j]["n"] for j in indices) == original:
                row({a: 1, indices[-1]: 1}, lo=1)
            else:
                row({a: 1}, lo=1)
    reserved = amount(financial.get("reservedCash", "0.000000"))
    row(cash, hi=(amount(financial["cash"]) - reserved) / UNIT)
    row(owned, hi=(amount(financial["maximumCommitted"]) - amount(financial["ownedCost"]) - reserved) / UNIT)
    row(writes, hi=financial["conditionalFutureLegs"] + 1)
    row({a: 1 for a in active.values()}, hi=financial["maximumMarkets"])
    inherited = set(financial["inheritedExchanges"])
    fresh_count = {a: 1 for ex, a in active.items() if ex not in inherited}
    for ex, alive in inherited_survives.items():
        original_inherited = inherited_quantities[ex]
        sold = {j: cols[j]["n"] for (sx, _), js in sales.items() if sx == ex for j in js}
        maximum_sale = sum(sold.values())
        row({**sold, alive: original_inherited}, lo=original_inherited)
        row({**sold, alive: maximum_sale}, hi=original_inherited - 1 + maximum_sale)
        fresh, present = inherited_fresh[ex], active[ex]
        row({fresh: 1, present: -1}, hi=0)
        row({fresh: 1, alive: 1}, hi=1)
        row({fresh: 1, present: -1, alive: 1}, lo=0)
        fresh_count[fresh] = 1
    row(fresh_count, hi=financial["freshMarketLimit"])
    for ex, values in market.items():
        old = sum(l["quantity"] * amount(l["price"]) for ls in financial["positions"].get(ex, {}).values() for l in ls)
        row(values, hi=(amount(financial["maximumMarketCost"]) - old) / UNIT)

    means = {mid: np.asarray(m["weightsPpm"], dtype=np.float64) @ deltas / UNIT for mid, m in pmap.items()}
    for mid, delta in means.items():
        if minimum_expected_change is not None:
            row(dict(enumerate(delta)), lo=amount(minimum_expected_change) / UNIT)
        if mid in model_floors:
            row(dict(enumerate(delta)), lo=model_floors[mid] / UNIT)
        if objective == "mean":
            row({**{j: -v for j, v in enumerate(delta)}, tcol: 1}, hi=0)
    if interval_requested:
        from .categorical_money_bound import ordinary_probability_vertices
        event_floors = []
        for vertices in ordinary_probability_vertices(registry, **ordinary_probability_inputs).values():
            indices = [j for j, col in enumerate(cols) if col['ex'] in vertices[0]]
            if not indices: continue
            bound = variable(-1e9, 1e9, 0); event_floors.append(bound)
            for vertex in vertices:
                values = {bound: 1}
                for j in indices:
                    col = cols[j]; payout = vertex[col['ex']]
                    if col['side'] == 'NO': payout = UNIT - payout
                    gain = ((payout - col['price']) if col['action'] == 'BUY'
                            else col['n'] * (col['price'] - payout)) / UNIT
                    values[j] = -gain
                row(values, hi=0)
        if objective == 'interval_mean':
            row({tcol: 1, **{v: -1 for v in event_floors}}, hi=0)
        if minimum_interval_expected_change is not None:
            row({v: 1 for v in event_floors}, lo=amount(minimum_interval_expected_change) / UNIT)
    if minimum_terminal_wealth is not None:
        floor = amount(minimum_terminal_wealth) / UNIT
        for i, delta in enumerate(deltas):
            row(dict(enumerate(delta)), lo=floor - baseline[i])

    # Bounds are conservative independent-column relaxations, not economic
    # claims. Only a hit=>strict-wealth implication is required for maximization.
    least = baseline + np.minimum(deltas, 0) @ np.asarray(upper[:N])
    for (fid, i), h in hits.items():
        # Every decision quantity is integral. The first strictly winning
        # reachable lattice value is exact; avoid a one-micro floating row
        # tolerance accidentally crediting an exact tie as a first-place hit.
        step = reduce(gcd, (abs(v) for v in exact_deltas[i]), 0)
        initial = baseline_audit["terminalWealthMicros"][i]
        threshold = amount(fmap[fid]["thresholds"][i])
        if not step:
            lower[h] = upper[h] = int(initial > threshold)
            continue
        target = (initial + ((threshold + 1 - initial + step - 1) // step) * step) / UNIT
        big_m = max(0.0, target - least[i])
        row({**dict(enumerate(deltas[i])), h: -big_m}, lo=target - baseline[i] - big_m)

    costs = np.zeros(len(upper))
    if monetary_objective:
        costs[tcol] = -1
    else:
        for mid, fid in target_pairs:
            weights = pmap[mid]["weightsPpm"]
            if objective == "robust_rank":
                row({**{hits[fid, i]: -w for i, w in enumerate(weights)}, tcol: 1}, hi=0)
            else:
                for i, w in enumerate(weights):
                    costs[hits[fid, i]] -= w / (UNIT * len(target_pairs))
        if objective == "robust_rank":
            costs[tcol] = -1 / UNIT
    rr, cc, vv = [], [], []
    for i, values in enumerate(rows):
        for j, v in values.items():
            if v:
                rr.append(i); cc.append(j); vv.append(v)
    matrix = coo_matrix((vv, (rr, cc)), shape=(len(rows), len(upper))).tocsc()
    started = monotonic()
    result = milp(costs, integrality=integer, bounds=Bounds(lower, upper),
                  constraints=LinearConstraint(matrix, low, high),
                  options={"time_limit": seconds, "mip_rel_gap": 0.0})
    out = dict(objective=objective, targetPairs=target_pairs, variables=len(upper),
               columns=N, seconds=round(monotonic() - started, 3), solverStatus=int(result.status),
               solverMessage=str(result.message), exactOptimalityClaimed=False,
               calibratedProbabilities=False, executionAuthority=False,
               minimumExpectedChange=minimum_expected_change,
               minimumModelExpectedChanges=minimum_model_expected_changes,
               minimumIntervalExpectedChange=minimum_interval_expected_change,
               requiredTargetHits=list(required_target_hits),
               minimumTerminalWealth=minimum_terminal_wealth, baseline=baseline_audit)
    if result.x is None:
        return dict(out, status="no_incumbent")
    try:
        legs = [dict(v, quantity=v["n"] if v["action"] == "SELL" else int(round(result.x[j])))
                for j, v in enumerate(cols) if int(round(result.x[j]))]
        checked = audit(financial, registry, books, model, fields, legs, assumptions=ASSUMPTIONS)
        if interval_requested:
            from .categorical_money_bound import ordinary_money_bounds
            interval_bound = ordinary_money_bounds(registry, **ordinary_probability_inputs, legs=legs)
            if minimum_interval_expected_change is not None:
                need(interval_bound['ordinaryMinimumExpectedChangeMicros'] >= amount(minimum_interval_expected_change),
                     'EXACT_ORDINARY_INTERVAL_FLOOR')
            out['ordinaryIntervalSensitivity'] = interval_bound
        if minimum_expected_change is not None:
            need(all(m["expectedChangeMicrosNumerator"] >= amount(minimum_expected_change) * UNIT for m in checked["models"]), "EXACT_MEAN_FLOOR")
        if model_floors:
            need(all(m["expectedChangeMicrosNumerator"] >= model_floors[m["id"]] * UNIT
                     for m in checked["models"]), "EXACT_MODEL_MEAN_FLOOR")
        if minimum_terminal_wealth is not None:
            need(min(checked["terminalWealthMicros"]) >= amount(minimum_terminal_wealth), "EXACT_WEALTH_FLOOR")
        for (fid, i), h in hits.items():
            if round(result.x[h]):
                need(checked["terminalWealthMicros"][i] > amount(fmap[fid]["thresholds"][i]), "NUMERICAL_FALSE_STRICT_HIT")
        selected = [p["afterStrictMassPpm"] for p in checked["fieldPairs"] if (p["modelId"], p["fieldId"]) in target_pairs]
        before = [p["beforeStrictMassPpm"] for p in checked["fieldPairs"] if (p["modelId"], p["fieldId"]) in target_pairs]
        improved = (min(selected) > min(before) if objective == "robust_rank"
                    else sum(selected) > sum(before))
        out.update(status="exact_sequence_checked", legs=legs, independentExactAudit=checked,
                   selectedAverageStrictMassPpm=sum(selected) / len(selected),
                   selectedMinimumStrictMassPpm=min(selected),
                   targetObjectiveImproved=improved,
                   researchDisposition="monetary_comparison_only" if monetary_objective
                       else "target_alternative_requires_economic_and_physical_review" if improved
                       else "retain_no_action_for_this_target_objective")
        retained = sorted({byex[ex]["contractId"] for ex in financial["positions"]} | {byex[v["ex"]]["contractId"] for v in legs})
        try:
            out["nativeProjection"] = project(worlds, model["probabilityModels"], fields,
                retained_contract_ids=retained,
                maximum_native_scenarios=maximum_native_scenarios or 128,
                complete_held_contract_ids=[byex[ex]["contractId"] for ex in financial["positions"]])
        except ValueError as error:
            out["nativeProjectionUnavailable"] = str(error)
    except (ValueError, AssertionError) as error:
        out.update(status="incumbent_rejected_by_exact_audit", rejectionReason=str(error))
    return out
