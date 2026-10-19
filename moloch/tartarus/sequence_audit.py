"""Exact audit of an offline, sequential, zero-fee rotation hypothesis.

No network, orders, owner approval or source freshness certification. Selling
before buying is a conditional execution branch, never credited live cash.
The complete no-fill account and every FIFO breakpoint are scored separately.
"""
from copy import deepcopy
from decimal import Decimal

UNIT = 1_000_000


def amount(value):
    x = Decimal(value) * UNIT
    if not x.is_finite() or x != x.to_integral_value():
        raise ValueError("EXACT_MICROS_REQUIRED")
    return int(x)


def need(ok, reason):
    if not ok:
        raise ValueError(reason)


def audit(financial, registry, books, model, fields, legs, *, assumptions):
    """Inputs are supplied research evidence, not authenticated account state.

    All existing reservations must be explicitly addressed by the hypothesis;
    this function cannot retire them. Exact declared expectation numerators
    have denominator 1e6 and units of micros. Book quantities are shared across
    SELL YES/BUY NO and SELL NO/BUY YES, rather than spent twice.
    """
    need(assumptions.get("zeroFees") is True
         and assumptions.get("zeroTerminalDebt") is True
         and assumptions.get("conditionalReservationTreatment")
         and assumptions.get("datedBooksAreFutureLiquidityHypotheses") is True,
         "EXPLICIT_HYPOTHESES_REQUIRED")
    byex = {r["exchangeId"]: r["contractId"] for r in registry}
    need(len(byex) == len(registry) and len(set(byex.values())) == len(byex), "REGISTRY")
    worlds = model["scenarios"]
    ids = [s["id"] for s in worlds]
    need(worlds and len(set(ids)) == len(ids), "SCENARIOS")
    for s in worlds:
        need(set(s["payouts"]) == set(byex.values()) and all(
            p in ["resolved_yes", "resolved_no", "refund_principal"]
            for p in s["payouts"].values()), "INCOMPLETE_PAYOUTS")
    for m in model["probabilityModels"]:
        w = m["weightsPpm"]
        need(len(w) == len(worlds) and all(type(v) is int and v >= 0 for v in w)
             and sum(w) == UNIT, "MODEL_MASS")
    positions = deepcopy(financial["positions"])
    need(set(positions) <= set(byex), "UNMODELED_HOLDING")
    retained_markets = financial.get("retainedMarketExchanges", [])
    need(type(retained_markets) is list and all(type(ex) is str and ex in byex for ex in retained_markets)
         and len(set(retained_markets)) == len(retained_markets), "RETAINED_MARKET_SCOPE")
    retained_markets = set(retained_markets)
    excluded_buys = financial.get("excludedBuyExchanges", [])
    need(type(excluded_buys) is list and all(type(ex) is str and ex in byex for ex in excluded_buys)
         and len(set(excluded_buys)) == len(excluded_buys), "EXCLUDED_BUY_SCOPE")
    handoff_required = financial.get("buyRequiresPriorCoveredSellExchanges", [])
    need(type(handoff_required) is list and all(type(ex) is str and ex in byex for ex in handoff_required)
         and len(set(handoff_required)) == len(handoff_required), "COVERED_SELL_HANDOFF_SCOPE")
    covered_sales = set()
    for sides in positions.values():
        need(set(sides) <= {"YES", "NO"}, "HOLDING_SIDE")
        for lots in sides.values():
            for lot in lots:
                need(type(lot["quantity"]) is int and lot["quantity"] >= 0
                     and 0 <= amount(lot["price"]) <= UNIT, "HOLDING_LOT")
    inherited = set(financial["inheritedExchanges"])
    inherited_remaining = deepcopy(financial.get("inheritedRemainingQuantities"))
    if inherited_remaining is not None:
        need(type(inherited_remaining) is dict and set(inherited_remaining) == inherited
             and all(type(n) is int and 0 < n <= sum(l["quantity"]
                 for lots in positions.get(ex, {}).values() for l in lots)
                 for ex, n in inherited_remaining.items()), "ORIGINAL_INHERITED_QUANTITIES")
    fresh_limit = financial["freshMarketLimit"]
    need(type(fresh_limit) is int and fresh_limit > 0, "FRESH_MARKET_LIMIT")
    cash = amount(financial["cash"])
    reserved = amount(financial.get("reservedCash", "0.000000"))
    owned = amount(financial["ownedCost"])
    market = {ex: sum(l["quantity"] * amount(l["price"])
                     for lots in sides.values() for l in lots)
              for ex, sides in positions.items()}
    need(cash >= 0 and owned == sum(market.values()), "ORIGINAL_FINANCIALS")
    need(0 <= reserved <= cash and owned + reserved <= amount(financial["maximumCommitted"]),
         "ORIGINAL_RESERVED_FUNDING")
    available = {}
    for ex, entry in books.items():
        if ex not in byex:
            continue
        book = entry.get("book", entry)
        for side in ["bids", "asks"]:
            levels = [[amount(r["price"]), r["quantity"]] for r in book[side]]
            need(all(0 < p < UNIT and type(n) is int and n > 0 for p, n in levels), "BOOK_LEVEL")
            need(len({p for p, n in levels}) == len(levels), "DUPLICATE_PRICE")
            available[ex, side] = sorted(levels, reverse=side == "bids")

    def wealth():
        out = [cash] * len(worlds)
        for ex, sides in positions.items():
            for side, lots in sides.items():
                # All worlds share these exact lots. Refund payout depends on
                # their original basis; YES/NO resolution depends on quantity.
                quantity = sum(l["quantity"] for l in lots)
                basis = sum(l["quantity"] * amount(l["price"]) for l in lots)
                for i, s in enumerate(worlds):
                    p = s["payouts"][byex[ex]]
                    out[i] += basis if p == "refund_principal" else quantity * UNIT if p == (
                        "resolved_yes" if side == "YES" else "resolved_no") else 0
        return out

    baseline = wealth()
    minimum = min(baseline)
    maximum_loss = 0
    breakpoints = 0
    need(len(legs) <= financial["conditionalFutureLegs"] + 1, "WRITE_COUNT")
    for leg in legs:
        ex, side, action = leg["ex"], leg["side"], leg["action"]
        n, price = leg["quantity"], leg["price"]
        need(ex in byex and side in ["YES", "NO"] and action in ["BUY", "SELL"], "LEG_IDENTITY")
        need(type(n) is int and 0 < n <= financial["physicalOrderQuantity"]
             and type(price) is int and 0 < price < UNIT, "LEG_SIZE_PRICE")
        book_side = "asks" if (action == "BUY") == (side == "YES") else "bids"
        yes_price = price if side == "YES" else UNIT - price
        remaining = n
        for level in available.get((ex, book_side), []):
            p, q = level
            if (book_side == "asks" and p > yes_price) or (book_side == "bids" and p < yes_price):
                break
            take = min(q, remaining)
            level[1] -= take
            remaining -= take
            if not remaining:
                break
        need(remaining == 0, "SHARED_DEPTH_EXHAUSTED")
        if action == "BUY":
            need(ex not in excluded_buys, "BUY_EXCLUDED_BY_ORIGINAL_OWNERSHIP_SCOPE")
            need(ex not in handoff_required or ex in covered_sales,
                 "BUY_REQUIRES_PRIOR_COVERED_SELL_HANDOFF")
            need(not any(l["quantity"] for l in positions.get(ex, {}).get(
                "NO" if side == "YES" else "YES", [])), "OPPOSITE_INVENTORY")
            cash -= n * price
            owned += n * price
            market[ex] = market.get(ex, 0) + n * price
            positions.setdefault(ex, {}).setdefault(side, []).append(
                {"quantity": n, "price": str(Decimal(price) / UNIT)})
            states = [wealth()]
        else:
            left, states = n, []
            for lot in positions.get(ex, {}).get(side, []):
                take = min(left, lot["quantity"])
                if not take:
                    continue
                left -= take
                lot["quantity"] -= take
                cash += take * price
                basis = take * amount(lot["price"])
                owned -= basis
                market[ex] -= basis
                states.append(wealth())
                if not left:
                    break
            need(not left, "SELL_EXCEEDS_ORIGINAL_FIFO")
            covered_sales.add(ex)
            if inherited_remaining is not None:
                inherited_remaining[ex] = max(0, inherited_remaining.get(ex, 0) - n)
                if not inherited_remaining[ex]:
                    inherited.discard(ex)
        need(cash >= reserved and owned + reserved <= amount(financial["maximumCommitted"])
             and market[ex] <= amount(financial["maximumMarketCost"]), "PREFIX_FUNDING_OR_BANK_LIMIT")
        active = {ex for ex, sides in positions.items()
                  if any(l["quantity"] for lots in sides.values() for l in lots)}
        active |= retained_markets
        need(len(active - inherited) <= fresh_limit
             and len(active) <= financial["maximumMarkets"], "MARKET_COUNT")
        for values in states:
            breakpoints += 1
            minimum = min(minimum, min(values))
            maximum_loss = max(maximum_loss, max(a - b for a, b in zip(baseline, values)))
    terminal = wealth()
    means, pairs = [], []
    for m in model["probabilityModels"]:
        delta = sum((a - b) * w for a, b, w in zip(terminal, baseline, m["weightsPpm"]))
        means.append({"id": m["id"], "expectedChangeMicrosNumerator": delta, "denominator": UNIT})
    for f in fields:
        need(f["completeField"] is False and len(f["thresholds"]) == len(worlds), "FIELD_SCOPE")
        targets = [amount(v) for v in f["thresholds"]]
        for m in model["probabilityModels"]:
            pairs.append({"modelId": m["id"], "fieldId": f["id"],
                "beforeStrictMassPpm": sum(w for v, t, w in zip(baseline, targets, m["weightsPpm"]) if v > t),
                "afterStrictMassPpm": sum(w for v, t, w in zip(terminal, targets, m["weightsPpm"]) if v > t)})
    return {"trades": len(legs), "cashMicros": cash, "ownedCostMicros": owned,
        "fifoBreakpointsScored": breakpoints, "minimumPrefixWealthMicros": minimum,
        "maximumPrefixLossVersusHoldMicros": maximum_loss, "models": means, "fieldPairs": pairs,
        "terminalWealthMicros": terminal, "scenarioIds": ids,
        "classification": "negative_declared_money" if any(m["expectedChangeMicrosNumerator"] < 0 for m in means)
            else "nonnegative_declared_money", "executionAuthority": False,
        "calibratedProbabilities": False, "actualProceedsCredited": False}
