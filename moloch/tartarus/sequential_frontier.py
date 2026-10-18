"""Bounded whole-account SELL/BUY search over declared joint target outcomes.

Offline research only. Quotes can be dated future-liquidity hypotheses. This
module cannot retire reservations, approve a campaign or place an order. Every
incumbent is independently checked against original FIFO and shared book depth.
"""
from copy import deepcopy
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
    for ex, sides in fin["positions"].items():
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
          minimum_terminal_wealth=None):
    """Search both BUY sides and FIFO SELL prefixes; report actual model scores.

    target_pairs is an explicit nonempty list of (modelId, fieldId). ``rank``
    maximizes the equal-weight average of their declared strict-hit masses;
    ``robust_rank`` maximizes their minimum; ``mean`` maximizes the weakest
    declared monetary change. Negative-mean alternatives remain identifiable.
    All supplied fields survive scoring, including unoptimized adverse fields.
    A time-limited incumbent is never a claim of global portfolio optimality.
    """
    import numpy as np
    from scipy.optimize import Bounds, LinearConstraint, milp
    from scipy.sparse import coo_matrix

    need(objective in {"rank", "robust_rank", "mean"}, "OBJECTIVE")
    need(type(seconds) in (int, float) and 0 < seconds <= 300, "TIME_BOUND")
    baseline_audit = audit(financial, registry, books, model, fields, [], assumptions=ASSUMPTIONS)
    worlds = model["scenarios"]
    need(len(worlds) <= 4096 and len(registry) <= 512, "MATRIX_BOUND")
    pmap = {m["id"]: m for m in model["probabilityModels"]}
    fmap = {f["id"]: f for f in fields}
    need(len(pmap) == len(model["probabilityModels"]) and len(fmap) == len(fields), "DUPLICATE_MODEL")
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
    active = {ex: variable() for ex in sorted(set(financial["positions"]) | {v["ex"] for v in cols})}
    choices = {ex: variable() for ex in sorted({ex for ex, side in buys})}
    hits = {(f, i): variable() for f in sorted({f for _, f in target_pairs}) for i in range(len(worlds))}
    tcol = variable(-1e9 if objective == "mean" else 0,
                    1e9 if objective == "mean" else UNIT, 0)
    rows, low, high = [], [], []

    def row(values, lo=-np.inf, hi=np.inf):
        rows.append(values); low.append(lo); high.append(hi)

    cash, owned, writes, market = {}, {}, {}, {}
    deltas = np.zeros((len(worlds), N))
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
            payout = (v["basis"] / n if v["action"] == "SELL" else p) if outcome == "refund_principal" else UNIT if outcome == ("resolved_yes" if v["side"] == "YES" else "resolved_no") else 0
            deltas[i, j] = n * (p - payout) / UNIT if v["action"] == "SELL" else (payout - p) / UNIT

    for ex, a in active.items():
        original = sum(l["quantity"] for lots in financial["positions"].get(ex, {}).values() for l in lots)
        indices = [j for (sx, side), js in sales.items() if sx == ex for j in js]
        if original:
            if indices and sum(cols[j]["n"] for j in indices) == original:
                row({a: 1, indices[-1]: 1}, lo=1)
            else:
                row({a: 1}, lo=1)
    row(cash, hi=amount(financial["cash"]) / UNIT)
    row(owned, hi=(amount(financial["maximumCommitted"]) - amount(financial["ownedCost"])) / UNIT)
    row(writes, hi=financial["conditionalFutureLegs"] + 1)
    row({a: 1 for a in active.values()}, hi=financial["maximumMarkets"])
    inherited = set(financial["inheritedExchanges"])
    row({a: 1 for ex, a in active.items() if ex not in inherited}, hi=financial["freshMarketLimit"])
    for ex, values in market.items():
        old = sum(l["quantity"] * amount(l["price"]) for ls in financial["positions"].get(ex, {}).values() for l in ls)
        row(values, hi=(amount(financial["maximumMarketCost"]) - old) / UNIT)

    means = {mid: np.asarray(m["weightsPpm"], dtype=np.float64) @ deltas / UNIT for mid, m in pmap.items()}
    for mid, delta in means.items():
        if minimum_expected_change is not None:
            row(dict(enumerate(delta)), lo=amount(minimum_expected_change) / UNIT)
        if objective == "mean":
            row({**{j: -v for j, v in enumerate(delta)}, tcol: 1}, hi=0)
    if minimum_terminal_wealth is not None:
        floor = amount(minimum_terminal_wealth) / UNIT
        for i, delta in enumerate(deltas):
            row(dict(enumerate(delta)), lo=floor - baseline[i])

    # Bounds are conservative independent-column relaxations, not economic
    # claims. Only a hit=>strict-wealth implication is required for maximization.
    least = baseline + np.minimum(deltas, 0) @ np.asarray(upper[:N])
    for (fid, i), h in hits.items():
        target = amount(fmap[fid]["thresholds"][i]) / UNIT + 1 / UNIT
        big_m = max(0.0, target - least[i])
        row({**dict(enumerate(deltas[i])), h: -big_m}, lo=target - baseline[i] - big_m)

    costs = np.zeros(len(upper))
    if objective == "mean":
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
               minimumTerminalWealth=minimum_terminal_wealth, baseline=baseline_audit)
    if result.x is None:
        return dict(out, status="no_incumbent")
    try:
        legs = [dict(v, quantity=v["n"] if v["action"] == "SELL" else int(round(result.x[j])))
                for j, v in enumerate(cols) if int(round(result.x[j]))]
        checked = audit(financial, registry, books, model, fields, legs, assumptions=ASSUMPTIONS)
        if minimum_expected_change is not None:
            need(all(m["expectedChangeMicrosNumerator"] >= amount(minimum_expected_change) * UNIT for m in checked["models"]), "EXACT_MEAN_FLOOR")
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
                   researchDisposition="monetary_comparison_only" if objective == "mean"
                       else "target_alternative_requires_economic_and_physical_review" if improved
                       else "retain_no_action_for_this_target_objective")
        retained = sorted({byex[ex]["contractId"] for ex in financial["positions"]} | {byex[v["ex"]]["contractId"] for v in legs})
        try:
            out["nativeProjection"] = project(worlds, model["probabilityModels"], fields,
                retained_contract_ids=retained,
                complete_held_contract_ids=[byex[ex]["contractId"] for ex in financial["positions"]])
        except ValueError as error:
            out["nativeProjectionUnavailable"] = str(error)
    except (ValueError, AssertionError) as error:
        out.update(status="incumbent_rejected_by_exact_audit", rejectionReason=str(error))
    return out
