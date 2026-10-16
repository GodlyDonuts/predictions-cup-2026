#!/usr/bin/env python3
"""Offline original-event FIFO attribution. No account client or trading actions."""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict, deque
from dataclasses import dataclass
from datetime import datetime, timezone
from decimal import Decimal, InvalidOperation, localcontext
import hashlib
import json
from pathlib import Path
import re
import sys

SCALE = 1_000_000


class AnalysisError(ValueError):
    """Public errors deliberately exclude private payloads and identifiers."""


def need(ok, code):
    if not ok:
        raise AnalysisError(code)


def units(value):
    need(not isinstance(value, (bool, float)), "EXACT_DECIMAL_REQUIRED")
    try:
        n = Decimal(value)
        need(n.is_finite() and len(n.as_tuple().digits) <= 100, "MONEY_RANGE")
        with localcontext() as ctx:
            ctx.prec = 128
            scaled = n * SCALE
            need(scaled == scaled.to_integral_value() and abs(scaled) < 2 ** 127,
                 "MONEY_PRECISION_OR_RANGE")
            return int(scaled)
    except (InvalidOperation, TypeError, ValueError, OverflowError):
        raise AnalysisError("MONEY_FORMAT") from None


def money(n):
    return f"{'-' if n < 0 else ''}{abs(n) // SCALE}.{abs(n) % SCALE:06d}"


def integer(n, positive=False):
    need(isinstance(n, int) and not isinstance(n, bool) and (n > 0 if positive else n >= 0),
         "INTEGER_REQUIRED")
    return n


def ident(n):
    need(isinstance(n, (str, int)) and not isinstance(n, bool), "IDENTIFIER_REQUIRED")
    n = str(n)
    need(re.fullmatch(r"[1-9][0-9]*", n) is not None, "IDENTIFIER_REQUIRED")
    return n


def digest(n):
    need(isinstance(n, str) and re.fullmatch(r"[0-9a-fA-F]{64}", n) is not None,
         "SOURCE_HASH_REQUIRED")
    return n


def clock(s):
    need(isinstance(s, str), "CLOCK_REQUIRED")
    m = re.fullmatch(r"(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)(?:\.(\d{1,9}))?(Z|[+-]\d\d:\d\d)", s)
    need(m is not None, "CLOCK_REQUIRED")
    try:
        delta = datetime.fromisoformat(m[1] + m[3].replace("Z", "+00:00")).astimezone(timezone.utc) - datetime(1970, 1, 1, tzinfo=timezone.utc)
        return (delta.days * 86400 + delta.seconds) * 1_000_000_000 + int((m[2] or "").ljust(9, "0"))
    except ValueError:
        raise AnalysisError("CLOCK_REQUIRED") from None


def side(s):
    need(s in ("YES", "NO"), "SIDE_REQUIRED")
    return s


def price(v):
    n = units(v)
    need(0 <= n <= SCALE, "PRICE_RANGE")
    return n


def fill_fact(f, native=False):
    need((native and f.get("counterfactual") is False) or
         (not native and f.get("type") == "actual_fill" and f.get("source") == "official_sig_api_v1"),
         "ACTUAL_FILL_REQUIRED")
    action = f["action"]
    need(action in ("buy", "sell"), "ACTION_REQUIRED")
    charge = units(f["fee"])
    need(charge >= 0, "NEGATIVE_FEE")
    fill_price = price(f["price"])
    need(5_000 <= fill_price <= SCALE - 5_000 and fill_price % 5_000 == 0, "FILL_PRICE_TICK")
    digest(f["receiptHash"])
    return (ident(f["actualOrderId"]), ident(f["sourceId" if native else "fillId"]),
            ident(f["orderId"]), ident(f["exchangeId"]), ident(f["marketId"]), side(f["side"]),
            action, integer(f["quantity"], True), fill_price, charge, clock(f["executedAt"]))


@dataclass
class Lot:
    n: int
    p: int
    origin: str
    opened: int
    executed: int | None
    cohort: int


def analyze(data, *, input_sha256=None, inventory_age_ms=None):
    need(data["schemaVersion"] == 1, "INPUT_SCHEMA")
    if inventory_age_ms is not None:
        integer(inventory_age_ms, True)
    source = data["source"]
    integer(source["bankSequence"], True)
    integer(source["journalPrefixBytes"], True)
    for key in ("bankPrevious", "reportEnvelopeHash", "journalPrefixSha256"):
        digest(source[key])
    need(isinstance(source["everyOriginalRowChainVerified"], bool), "SOURCE_VERIFICATION_LABEL")
    if input_sha256 is not None:
        digest(input_sha256)
    observed = clock(data["observedAt"])
    initial = units(data["startingCash"])
    need(initial > 0, "STARTING_CASH")
    expected = data["expectedReport"]
    need(expected.get("continuation") is None, "CONTINUATION_REQUIRES_ORIGINAL_OPENING_STATE")
    forecast = {ident(q) for q in data.get("forecastQids", [])}
    orders = {}
    for q in data["orders"]:
        qid = ident(q["id"])
        need(qid not in orders, "DUPLICATE_ORDER")
        orders[qid] = q
    lots = defaultdict(deque)
    cohorts, fragments = [], []
    stats = defaultdict(lambda: Counter())
    origin_gross, origin_fees = Counter(), Counter()
    markets, seen_lots, seen_positions = {}, set(), set()
    seen, global_ids, accepted, order_actual, actual_order = {}, {}, [], {}, {}
    filled = Counter()
    cash, realized, fees, opening, inherited = initial, 0, 0, 0, 0
    adoption_at = None
    last_seq, last_observed = 0, None
    duplicate_count, execution_reversals = 0, 0
    previous_execution = None

    def market(ex, mid):
        need(ex not in markets or markets[ex] == mid, "MARKET_CHANGED")
        markets[ex] = mid

    def cohort(ex, s, n, p, origin, opened, executed):
        index = len(cohorts)
        cohorts.append({"exchangeId": ex, "side": s, "origin": origin, "quantity": n,
                        "remaining": n, "closed": 0, "basis": n * p, "gross": 0, "fees": 0,
                        "opened": opened, "executed": executed, "turnover": 0})
        lots[ex, s].append(Lot(n, p, origin, opened, executed, index))
        return index

    def close(ex, s, n, proceeds, executed, reason, mechanism):
        nonlocal inherited
        need(sum(l.n for l in lots[ex, s]) >= n, "UNCOVERED_CLOSE_OR_UNKNOWN_BASIS")
        gross, closed = 0, []
        while n:
            l = lots[ex, s][0]
            take = min(n, l.n)
            age = executed - (l.executed if l.executed is not None else l.opened)
            need(age >= 0, "NEGATIVE_HOLDING_AGE")
            gain = take * (proceeds - l.p)
            gross += gain
            stats[ex, s]["fifoGross"] += gain
            origin_gross[l.origin] += gain
            if l.origin == "inherited":
                inherited += gain
            c = cohorts[l.cohort]
            c["gross"] += gain
            c["closed"] += take
            c["remaining"] -= take
            c["turnover"] += take * proceeds
            fragments.append({"exchangeId": ex, "side": s, "origin": l.origin, "quantity": take,
                              "age": age, "gross": gain, "reason": reason, "mechanism": mechanism})
            closed.append((l.cohort, take))
            l.n -= take
            n -= take
            if l.n == 0:
                lots[ex, s].popleft()
        return gross, closed

    for wrapper in data["events"]:  # Original durable BANK order. Never sort execution clocks.
        seq = integer(wrapper["sequence"], True)
        need(last_seq < seq <= source["bankSequence"], "EVENT_SEQUENCE")
        last_seq = seq
        digest(wrapper["hash"])
        e = wrapper["event"]
        at = clock(e.get("processedAt", e["receivedAt"]))
        need((last_observed is None or at >= last_observed) and at <= observed, "OBSERVATION_ORDER")
        need(clock(e["receivedAt"]) <= at, "RECEIPT_AFTER_OBSERVATION")
        last_observed = at
        if e["type"] == "actual_position_adoption":
            need(adoption_at is None and not accepted and e.get("source") == "official_sig_api_v1"
                 and e.get("complete") is True and e.get("authority") == "human_moloch_inherited_management_20261015",
                 "ADOPTION_PROVENANCE_OR_ORDER")
            digest(e["receiptHash"])
            need(clock(e["requestStartedAt"]) <= clock(e["receivedAt"]), "ADOPTION_CLOCK")
            adoption_at = at
            for row in e["positions"]:
                ex, mid = ident(row["exchangeId"]), ident(row["marketId"])
                market(ex, mid)
                need(ex not in seen_positions, "DUPLICATE_ADOPTION_POSITION")
                seen_positions.add(ex)
                signed = row["quantity"]
                need(isinstance(signed, int) and not isinstance(signed, bool), "SIGNED_QUANTITY")
                s = "NO" if signed < 0 else "YES"
                total, basis, prior = 0, 0, None
                for l in row["lots"]:
                    key = ident(l["lotId"])
                    need(key not in seen_lots and side(l["side"]) == s, "ADOPTION_LOT_DUPLICATE_OR_SIDE")
                    seen_lots.add(key)
                    n, p, opened = integer(l["quantity"], True), price(l["entryPrice"]), clock(l["openedAt"])
                    need(opened <= clock(e["receivedAt"]) and (prior is None or opened >= prior), "ADOPTION_LOT_CLOCK")
                    prior = opened
                    cohort(ex, s, n, p, "inherited", opened, None)
                    total += n
                    basis += n * p
                need(total == abs(signed) and basis == units(row["lotPrincipal"]), "ADOPTION_BASIS_MISMATCH")
                opening += basis
            need(opening == units(e["openingCost"]) and opening <= initial, "OPENING_COST_MISMATCH")
            cash -= opening
            continue
        need(adoption_at is not None, "FILL_BEFORE_ADOPTION")
        fact = fill_fact(e)
        actual, fid, qid, ex, mid, s, action, n, p, fee, executed = fact
        need(executed <= clock(e["receivedAt"]), "FILL_EXECUTION_CLOCK")
        key = (actual, fid)
        need(qid not in order_actual or order_actual[qid] == actual, "ORDER_ACTUAL_BINDING_CHANGED")
        need(actual not in actual_order or actual_order[actual] == qid, "ACTUAL_ORDER_REBOUND")
        order_actual[qid], actual_order[actual] = actual, qid
        need(fid not in global_ids or global_ids[fid] == actual, "FILL_ID_REBOUND")
        global_ids[fid] = actual
        if key in seen:
            need(seen[key] == fact, "CONFLICTING_DUPLICATE_FILL")
            duplicate_count += 1
            continue
        seen[key] = fact
        accepted.append((fact, at, e["receiptHash"]))
        q = orders.get(qid)
        need(q is not None, "FILL_ORDER_MISSING")
        need(ident(q["marketId"]) == mid and ident(q["exchangeId"]) == ex and q["side"] == s and q["action"] == action,
             "FILL_ORDER_SCOPE")
        limit = price(q["price"])
        need((p <= limit if action == "buy" else p >= limit) and executed >= integer(q["createdAt"]) * 1_000_000,
             "FILL_ORDER_LIMIT_OR_CLOCK")
        filled[qid] += n
        need(filled[qid] <= integer(q["quantity"], True), "ORDER_OVERFILLED")
        market(ex, mid)
        reason = "maximumAge" if q.get("inventoryExitReason") == "maximumAge" else "other"
        origin = "forecast" if qid in forecast else "new"
        execution_reversals += previous_execution is not None and executed < previous_execution
        previous_execution = executed
        st = stats[ex, s]
        st["fills"] += 1
        st[action + "Shares"] += n
        st["fees"] += fee
        st[action + "Notional"] += n * p
        fees += fee
        realized -= fee
        if action == "buy":
            cash -= n * p + fee
            ci = cohort(ex, s, n, p, origin, executed, executed)
            cohorts[ci]["fees"] += fee
            origin_fees[origin] += fee
            other = "NO" if s == "YES" else "YES"
            pairs = min(sum(l.n for l in lots[ex, s]), sum(l.n for l in lots[ex, other]))
            gross = 0
            if pairs:
                a, _ = close(ex, s, pairs, p, executed, reason, "oppositeBuyPair")
                b, _ = close(ex, other, pairs, SCALE - p, executed, reason, "oppositeBuyPair")
                gross = a + b
                cash += pairs * SCALE
                st["pairedShares"] += pairs
        else:
            cash += n * p - fee
            gross, closed = close(ex, s, n, p, executed, reason, "sell")
            # Exact integer-millionth fee allocation in FIFO fragment order.
            # Fees closing inherited lots remain new-trading expense, matching native.
            left = fee
            for i, (ci, take) in enumerate(closed):
                part = left if i == len(closed) - 1 else fee * take // n
                left -= part
                c = cohorts[ci]
                fee_origin = c["origin"] if c["origin"] != "inherited" else origin
                origin_fees[fee_origin] += part
                if c["origin"] != "inherited":
                    c["fees"] += part
        realized += gross
        st["executionGross"] += gross
        need(cash >= 0, "NEGATIVE_CASH")

    need(adoption_at is not None, "ADOPTION_REQUIRED")
    for qid, q in orders.items():
        need(filled[qid] == integer(q["filled"]), "ORDER_FILLED_PARITY")
    native = [(fill_fact(f, True), clock(f["at"]), f["receiptHash"]) for f in data["fills"]]
    need(native == accepted, "NATIVE_UNIQUE_FILL_PARITY")
    actual_positions = {k: [(l.n, l.p, l.executed) for l in ls] for k, ls in lots.items() if ls}
    end_positions = {}
    for ex, position in expected["positions"].items():
        for s in ("YES", "NO"):
            values = [(integer(l["quantity"], True), price(l["price"]), clock(l["executedAt"]) if "executedAt" in l else None) for l in position[s]]
            if values:
                end_positions[ident(ex), s] = values
    need(actual_positions == end_positions, "ENDING_FIFO_POSITION_PARITY")
    owned = sum(l.n * l.p for ls in lots.values() for l in ls)
    need(cash + owned - initial == realized, "ACCOUNTING_IDENTITY")
    for key, value in (("cash", cash), ("ownedCost", owned), ("realized", realized), ("fees", fees),
                       ("openingInventoryCost", opening), ("inheritedRealizedGross", inherited),
                       ("newTradingRealized", realized - inherited)):
        need(units(expected[key]) == value, "REPORT_PARITY_" + key.upper())
    need(sum(origin_gross.values()) - sum(origin_fees.values()) == realized and sum(origin_fees.values()) == fees,
         "ATTRIBUTION_IDENTITY")

    def closes(rows):
        shares = sum(r["quantity"] for r in rows)
        return {"fragments": len(rows), "shares": shares, "grossRealized": money(sum(r["gross"] for r in rows)),
                "quantityWeightedHoldingMs": (str(Decimal(sum(r["age"] * r["quantity"] for r in rows)) / shares / SCALE) if shares else None),
                "minimumHoldingMs": money(min(r["age"] for r in rows)) if rows else None,
                "maximumHoldingMs": money(max(r["age"] for r in rows)) if rows else None,
                "belowExplicitAgeThresholdShares": (sum(r["quantity"] for r in rows if r["age"] < inventory_age_ms * SCALE)
                                                    if inventory_age_ms is not None else None)}

    origins = {}
    for origin in ("inherited", "new", "forecast"):
        cs = [c for c in cohorts if c["origin"] == origin]
        fs = [r for r in fragments if r["origin"] == origin]
        closed = [c for c in cs if c["remaining"] == 0]
        origins[origin] = {"grossRealized": money(origin_gross[origin]), "feesExpensed": money(origin_fees[origin]),
                           "netRealizedAttribution": money(origin_gross[origin] - origin_fees[origin]),
                           "remainingUnmarkedCost": money(sum(l.n * l.p for ls in lots.values() for l in ls if l.origin == origin)),
                           "lotCohorts": len(cs), "fullyClosedCohorts": len(closed),
                           "fullyClosedNetWins": sum(c["gross"] - c["fees"] > 0 for c in closed),
                           "fullyClosedNetLosses": sum(c["gross"] - c["fees"] < 0 for c in closed),
                           "fullyClosedNetFlat": sum(c["gross"] - c["fees"] == 0 for c in closed),
                           "closed": closes(fs), "maximumAgeExits": closes([r for r in fs if r["reason"] == "maximumAge"]),
                           "maximumAgeGrossLoss": money(sum(min(0, r["gross"]) for r in fs if r["reason"] == "maximumAge")),
                           "fullyClosedAcquisitionPrincipal": money(sum(c["basis"] for c in closed)),
                           "fullyClosedExitTurnover": money(sum(c["turnover"] for c in closed))}
    per_market = []
    for ex, s in sorted(stats, key=lambda k: (int(k[0]), k[1])):
        st = stats[ex, s]
        per_market.append({"exchangeId": ex, "marketId": markets[ex], "side": s,
                           **{k: st[k] for k in ("fills", "buyShares", "sellShares", "pairedShares")},
                           "buyNotional": money(st["buyNotional"]), "sellNotional": money(st["sellNotional"]),
                           "fees": money(st["fees"]), "executionRealized": money(st["executionGross"] - st["fees"]),
                           "fifoOriginRealized": money(st["fifoGross"] - st["fees"]),
                           "closingOriginGross": {o: money(sum(r["gross"] for r in fragments if r["exchangeId"] == ex and r["side"] == s and r["origin"] == o)) for o in origins},
                           "remainingUnmarkedCost": money(sum(l.n * l.p for l in lots[ex, s]))})
    open_positions = [{"exchangeId": ex, "marketId": markets[ex], "side": s, "origin": o,
                       "quantity": sum(l.n for l in ls if l.origin == o), "unmarkedCost": money(sum(l.n * l.p for l in ls if l.origin == o))}
                      for (ex, s), ls in sorted(lots.items(), key=lambda k: (int(k[0][0]), k[0][1]))
                      for o in origins if any(l.origin == o for l in ls)]
    return {"schemaVersion": 1, "analysis": "original_actual_event_fifo_attribution", "inputSha256": input_sha256,
            "sourceBoundary": {k: source[k] for k in ("bankSequence", "bankPrevious", "reportEnvelopeHash", "journalPrefixSha256", "journalPrefixBytes", "everyOriginalRowChainVerified")},
            "chainVerification": "extractor_attestation_only_full_original_chain_not_recomputed_from_filtered_events",
            "observedAt": data["observedAt"], "parity": "exact_native_fills_fifo_positions_cash_cost_fees_realized_attribution",
            "uniqueActualFills": len(accepted), "duplicateOriginalFillEvents": duplicate_count,
            "adjacentExecutionClockReversals": execution_reversals, "inventoryAgeThresholdMs": inventory_age_ms,
            "accounting": {"initialCash": money(initial), "openingInventoryCost": money(opening), "cash": money(cash),
                           "ownedCost": money(owned), "fees": money(fees), "realized": money(realized),
                           "feesNotAssignedToAcquisitionCohort": money(fees - sum(c["fees"] for c in cohorts)),
                           "inheritedRealizedGross": money(inherited), "newTradingRealized": money(realized - inherited)},
            "origins": origins, "perExchangeSide": per_market, "openPositions": open_positions,
            "cohorts": [{"cohortIndex": i, "exchangeId": c["exchangeId"], "side": c["side"], "origin": c["origin"],
                         "acquiredShares": c["quantity"], "closedShares": c["closed"], "remainingShares": c["remaining"],
                         "grossRealized": money(c["gross"]), "feesExpensed": money(c["fees"]),
                         "netRealizedAttribution": money(c["gross"] - c["fees"])} for i, c in enumerate(cohorts)],
            "limitations": ["No remaining position receives a market mark, liquidation value, or future fill.",
                            "Inherited realized is opening-lot gross attribution, not official account P&L; all fees remain new/forecast expense.",
                            "Forecast origin uses only the extraction's explicit original forecast qids; it is not inferred from market title.",
                            "FIFO fee fragments use integer-millionth rounding with the remainder on the last fragment.",
                            "Cohort fees are expensed immediately, including entry fees on still-open inventory.",
                            "Fees closing inherited lots remain new/forecast execution expense, outside acquisition-cohort fees.",
                            "Open quantities are attributed native inventory; no fresh official account audit is claimed."],
            "profitabilityEstablished": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", required=True, help="Private extraction envelope")
    parser.add_argument("--output", help="Aggregate JSON; default stdout")
    parser.add_argument("--inventory-age-ms", type=int, help="Optional explicit actual policy threshold")
    args = parser.parse_args()
    try:
        raw = Path(args.input).read_bytes()
        data = json.loads(raw, parse_float=Decimal)
        result = analyze(data, input_sha256=hashlib.sha256(raw).hexdigest(), inventory_age_ms=args.inventory_age_ms)
        rendered = json.dumps(result, sort_keys=True, indent=2) + "\n"
        if args.output:
            need(Path(args.output).resolve() != Path(args.input).resolve(), "OUTPUT_OVERWRITES_INPUT")
            Path(args.output).write_text(rendered, encoding="utf-8")
        else:
            print(rendered, end="")
        return 0
    except (AnalysisError, KeyError, TypeError, ValueError, OSError):
        print("analysis rejected: INPUT_OR_EXACT_PARITY", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
