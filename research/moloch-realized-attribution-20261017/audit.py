#!/usr/bin/env python3
"""Finite, offline FIFO attribution of a sealed native financial report.

This is an independent accounting reconstruction, not native journal replay,
receipt authentication, financial authority, a valuation model or a profit forecast.
"""
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

SCALE = 1_000_000
CLASSES = ("inherited", "ordinary", "reviewed_fundamental", "package_or_unknown")


class AuditError(ValueError):
    """Errors contain codes, never private source rows or venue identifiers."""


def need(ok, code):
    if not ok:
        raise AuditError(code)


def canonical(v):
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def sha(v):
    return hashlib.sha256(v).hexdigest()


def units(v):
    need(isinstance(v, str), "EXACT_DECIMAL_STRING_REQUIRED")
    try:
        d = Decimal(v)
        with localcontext() as c:
            c.prec = 100
            u = d * SCALE
            need(d.is_finite() and u == u.to_integral_value() and abs(u) < 2**127,
                 "DECIMAL_PRECISION_OR_RANGE")
            return int(u)
    except (InvalidOperation, ValueError, OverflowError):
        raise AuditError("DECIMAL_FORMAT") from None


def money(u):
    return f"{'-' if u < 0 else ''}{abs(u)//SCALE}.{abs(u)%SCALE:06d}"


def integer(v, positive=False):
    need(type(v) is int and (v > 0 if positive else v >= 0), "INTEGER_REQUIRED")
    return v


def clock(v):
    need(isinstance(v, str), "CLOCK_REQUIRED")
    m = re.fullmatch(r"(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)(?:\.(\d{1,9}))?(Z|[+-]\d\d:\d\d)", v)
    need(m is not None, "CLOCK_REQUIRED")
    try:
        t = datetime.fromisoformat(m[1] + m[3].replace("Z", "+00:00")).astimezone(timezone.utc)
        delta = t - datetime(1970, 1, 1, tzinfo=timezone.utc)
        return (delta.days*86400 + delta.seconds)*10**9 + int((m[2] or "").ljust(9, "0"))
    except ValueError:
        raise AuditError("CLOCK_REQUIRED") from None


def date(v):
    return datetime.fromtimestamp(clock(v)//10**9, timezone.utc).date().isoformat()


@dataclass
class Lot:
    n: int
    p: int
    origin: str
    qid: int | None
    born: int
    executed: str | None
    entry_date: str


def audit(v, source_sha):
    need(isinstance(v, dict), "REPORT_REQUIRED")
    need(v.get("hash") == sha(canonical({k: x for k, x in v.items() if k != "hash"})),
         "SEALED_REPORT_HASH")
    r = v["report"]
    need(r.get("continuation") is None, "CONTINUATION_REQUIRES_ORIGINAL_STATE")
    need(not r.get("livePackages"), "PACKAGE_ATTRIBUTION_REQUIRES_ORIGINAL_QID_MAP")
    need(not r.get("collateralAccounting"), "COLLATERAL_TRANSFORMS_REQUIRE_ORIGINAL_EVENTS")
    need(r["executionMode"] == "live" and r["fillModel"] == "actualAccountFills", "LIVE_ACTUAL_REPORT")
    end = clock(r["at"])
    need(integer(v["sequence"]) == integer(r["events"]), "BOUNDARY_SEQUENCE")
    orders = {}
    for q in v["orders"]:
        qid = integer(q["id"], True)
        need(qid not in orders, "DUPLICATE_NATIVE_QID")
        orders[qid] = q
    need(len(orders) == integer(r["orders"]), "ORDER_COUNT")
    need(len(orders) <= 200_000 and len(v["fills"]) <= 10_000, "FINITE_REPORT_BOUND")

    def order_class(q):
        if q.get("reviewedFundamentalEntry"):
            return "reviewed_fundamental"
        if q.get("packageId") or q.get("livePackage"):
            return "package_or_unknown"
        return "ordinary"

    opening = r["inheritedInventory"]
    need(opening["historicalFillsImported"] is False and opening["feesRemainInNewTradingRealized"] is True,
         "OPENING_ATTRIBUTION_SCHEMA")
    adoption = clock(opening["managementStartedAt"])
    need(adoption <= end, "ADOPTION_CLOCK")
    allocation = r["ownerCapitalAllocation"]
    initial = units(allocation["originalInitialCash"])
    contributions = allocation["contributions"]
    need(len(contributions) <= 16, "CONTRIBUTION_BOUND")
    need(sum(units(x["delta"]) for x in contributions) == units(allocation["netContributions"])
         and initial + units(allocation["netContributions"]) == units(allocation["effectiveInitialCash"]),
         "CONTRIBUTION_IDENTITY")
    contribution_times = [clock(x["at"]) for x in contributions]
    need(contribution_times == sorted(set(contribution_times))
         and all(adoption <= t <= end for t in contribution_times), "CONTRIBUTION_ORDER")
    pending_contributions = deque(zip(contribution_times, contributions))
    lots = defaultdict(deque)
    opening_cost = 0
    market = {}
    seen_lots = set()
    for position in opening["sourcePositions"]:
        ex, mid = position["exchangeId"], position["marketId"]
        need(ex not in market, "DUPLICATE_OPENING_EXCHANGE")
        market[ex] = mid
        cost, total = 0, 0
        side = "NO" if position["quantity"] < 0 else "YES"
        previous = None
        for l in position["lots"]:
            need(l["lotId"] not in seen_lots and l["side"] == side, "OPENING_LOT_ID_OR_SIDE")
            seen_lots.add(l["lotId"])
            n, p, born = integer(l["quantity"], True), units(l["entryPrice"]), clock(l["openedAt"])
            need(0 <= p <= SCALE and born <= adoption and (previous is None or born >= previous),
                 "OPENING_LOT_BASIS_OR_CLOCK")
            previous = born
            # Management starts at actual adoption, not historical lot birth.
            lots[ex, side].append(Lot(n, p, "inherited", None, adoption, None, "opening_at_adoption"))
            cost += n*p
            total += n
        need(total == abs(position["quantity"]) and cost == units(position["lotPrincipal"]),
             "OPENING_QUANTITY_OR_PRINCIPAL")
        opening_cost += cost
    need(opening_cost == units(opening["openingCost"]) == units(r["openingInventoryCost"]), "OPENING_COST")
    cash = initial - opening_cost
    need(cash >= 0, "OPENING_CASH")
    fees = 0
    gross = Counter()
    fee_class = Counter()
    by_date = defaultdict(Counter)
    by_exit = defaultdict(Counter)
    by_entry = defaultdict(Counter)
    entry_meta = {}
    fills_by_qid = Counter()
    seen = {}
    duplicates = inversions = 0
    last_observed = adoption
    last_execution = None

    def close(ex, side, quantity, proceeds, executed, reason, mechanism):
        need(sum(l.n for l in lots[ex, side]) >= quantity, "UNCOVERED_CLOSE_OR_UNKNOWN_BASIS")
        while quantity:
            l = lots[ex, side][0]
            take = min(quantity, l.n)
            age = executed - l.born
            gain = take*(proceeds-l.p)
            gross[l.origin] += gain
            bucket = by_date[l.origin, l.entry_date]
            bucket["closedShares"] += take
            bucket["gross"] += gain
            exit_bucket = by_exit[l.origin, reason, mechanism]
            exit_bucket["shares"] += take
            exit_bucket["gross"] += gain
            if age >= 0:
                exit_bucket["age_ns_weighted"] += take*age
                exit_bucket["age_known_shares"] += take
            else:
                # Financial FIFO follows native commit order. A late-arriving
                # earlier execution cannot establish a nonnegative holding age.
                exit_bucket["age_missing_shares"] += take
            exit_bucket["fragments"] += 1
            if l.qid is not None:
                by_entry[l.qid]["closedShares"] += take
                by_entry[l.qid]["gross"] += gain
            quantity -= take
            l.n -= take
            if l.n == 0:
                lots[ex, side].popleft()

    for f in v["fills"]:  # Native append/commit order; NEVER execution-sort.
        need(f.get("counterfactual") is False, "COUNTERFACTUAL_FILL")
        identity = (f["actualOrderId"], str(f["sourceId"]))
        # at/receipt membership is part of the exact retained native fill row.
        encoded = canonical(f)
        if identity in seen:
            need(seen[identity] == encoded, "CHANGED_DUPLICATE_FILL")
            duplicates += 1
            continue
        seen[identity] = encoded
        qid, n, p, fee = integer(f["orderId"], True), integer(f["quantity"], True), units(f["price"]), units(f["fee"])
        ex, mid, side, action = f["exchangeId"], f["marketId"], f["side"], f["action"]
        observed, executed = clock(f["at"]), clock(f["executedAt"])
        need(adoption <= observed <= end and adoption <= executed <= observed and observed >= last_observed,
             "NEED_ORIGINAL_EVENT_CHRONOLOGY")
        need(observed not in contribution_times, "AMBIGUOUS_CONTRIBUTION_FILL_ORDER")
        while pending_contributions and pending_contributions[0][0] < observed:
            _, c = pending_contributions.popleft()
            delta = units(c["delta"])
            need(delta > 0, "CONTRIBUTION_DELTA")
            cash += delta
        last_observed = observed
        inversions += last_execution is not None and executed < last_execution
        last_execution = executed
        need(side in ("YES", "NO") and action in ("buy", "sell") and 0 < p < SCALE and fee >= 0,
             "FILL_TERMS")
        q = orders.get(qid)
        need(q is not None and all(q[k] == f[k] for k in ("exchangeId", "marketId", "side", "action")),
             "FILL_NATIVE_ORDER_SCOPE")
        need(executed >= integer(q["createdAt"])*1_000_000
             and (p <= units(q["price"]) if action == "buy" else p >= units(q["price"])),
             "FILL_LIMIT_OR_CLOCK")
        need(ex not in market or market[ex] == mid, "EXCHANGE_MARKET_CHANGED")
        market[ex] = mid
        fills_by_qid[qid] += n
        need(fills_by_qid[qid] <= integer(q["quantity"], True), "ORDER_OVERFILLED")
        origin = order_class(q)
        fee_class[origin] += fee
        fees += fee
        reason = q.get("inventoryExitReason", "other")
        if action == "buy":
            entry_meta[qid] = {"nativeQid": qid, "exchangeId": ex, "side": side, "entryClass": origin}
            by_entry[qid]["boughtShares"] += n
            by_entry[qid]["entryPrincipal"] += n*p
            lots[ex, side].append(Lot(n, p, origin, qid, executed, f["executedAt"], date(f["executedAt"])))
            cash -= n*p+fee
            other = "NO" if side == "YES" else "YES"
            pairs = min(sum(l.n for l in lots[ex, side]), sum(l.n for l in lots[ex, other]))
            if pairs:
                close(ex, side, pairs, p, executed, reason, "opposite_buy_netting")
                close(ex, other, pairs, SCALE-p, executed, reason, "opposite_buy_netting")
                cash += pairs*SCALE
        else:
            cash += n*p-fee
            close(ex, side, n, p, executed, reason, "sell")
        need(cash >= 0, "NEGATIVE_HISTORICAL_CASH")
    for _, c in pending_contributions:
        cash += units(c["delta"])
    for qid, q in orders.items():
        need(fills_by_qid[qid] == integer(q["filled"]), "ORDER_FILLED_PARITY")
    need(len(seen) == integer(r["fills"]), "UNIQUE_FILL_COUNT")
    actual_positions = {}
    remaining = Counter()
    open_summary = defaultdict(Counter)
    for (ex, side), queue in lots.items():
        if not queue:
            continue
        rows = []
        for l in queue:
            row = {"quantity": l.n, "price": money(l.p)}
            if l.executed is not None:
                row["executedAt"] = l.executed
            rows.append(row)
            remaining[l.origin] += l.n*l.p
            open_summary[l.origin, ex, side]["quantity"] += l.n
            open_summary[l.origin, ex, side]["principal"] += l.n*l.p
            by_date[l.origin, l.entry_date]["heldShares"] += l.n
            by_date[l.origin, l.entry_date]["heldPrincipal"] += l.n*l.p
            if l.qid is not None:
                by_entry[l.qid]["heldShares"] += l.n
                by_entry[l.qid]["heldPrincipal"] += l.n*l.p
        actual_positions[ex, side] = rows
    expected_positions = {(ex, side): rows for ex, sides in r["positions"].items()
                          for side, rows in sides.items() if rows}
    need(actual_positions == expected_positions, "EXACT_ENDPOINT_FIFO_POSITIONS")
    realized = sum(gross.values())-fees
    inherited = gross["inherited"]
    owned = sum(remaining.values())
    totals = {"cash": cash, "ownedCost": owned, "fees": fees, "realized": realized,
              "newTradingRealized": realized-inherited, "inheritedRealizedGross": inherited}
    for k, value in totals.items():
        need(value == units(r[k]), "NATIVE_PARITY_"+k)
    need(cash+owned-units(allocation["effectiveInitialCash"]) == realized, "ACCOUNTING_IDENTITY")
    classes = {k: {"grossRealized": money(gross[k]), "stillHeldPrincipal": money(remaining[k]),
                   "entryOrders": sum(m["entryClass"] == k for m in entry_meta.values())} for k in CLASSES}
    return {"schemaVersion": 1, "sourceSha256": source_sha, "analyzerSha256": sha(Path(__file__).read_bytes()),
            "boundary": {k: v[k] for k in ("sequence", "hash", "previous", "manifestHash")},
            "reportAt": r["at"], "managementStartedAt": opening["managementStartedAt"],
            "accounting": {k: money(x) for k, x in totals.items()},
            "originalInitialCash": money(initial), "netOwnerContributions": allocation["netContributions"],
            "openingInventoryCost": money(opening_cost), "originalFillRows": len(v["fills"]),
            "uniqueActualFillRows": len(seen), "duplicateRows": duplicates,
            "executionClockInversionsPreserved": inversions, "entryClasses": classes,
            "feesByExecutingOrderClass": {k: money(fee_class[k]) for k in CLASSES},
            "entryTimeCohorts": [{"entryClass": k, "executionDateUtc": day, "closedShares": b["closedShares"],
                                  "grossRealized": money(b["gross"]), "stillHeldShares": b["heldShares"],
                                  "stillHeldPrincipal": money(b["heldPrincipal"])} for (k, day), b in sorted(by_date.items())],
            "closedFifoByExit": [{"entryClass": k, "exitReason": reason, "mechanism": mechanism,
                                 "matchedShares": b["shares"], "fragments": b["fragments"],
                                 "grossRealized": money(b["gross"]),
                                 "quantityWeightedAgeNsNumerator": str(b["age_ns_weighted"]),
                                 "ageKnownShares": b["age_known_shares"],
                                 "ageMissingExecutionOrderingShares": b["age_missing_shares"],
                                 "ageClock": "since_actual_adoption" if k == "inherited" else "original_fill_execution"}
                                for (k, reason, mechanism), b in sorted(by_exit.items())],
            "entryNativeQids": [entry_meta[qid] | {"boughtShares": b["boughtShares"], "closedFilledShares": b["closedShares"],
                                                  "grossRealized": money(b["gross"]), "entryPrincipal": money(b["entryPrincipal"]),
                                                  "stillHeldShares": b["heldShares"], "stillHeldPrincipal": money(b["heldPrincipal"]),
                                                  "filledInventoryFullyClosed": b["heldShares"] == 0}
                                for qid, b in sorted(by_entry.items())],
            "openPositions": [{"entryClass": k, "exchangeId": ex, "side": side, "quantity": b["quantity"],
                               "principal": money(b["principal"])} for (k, ex, side), b in sorted(open_summary.items())],
            "parity": "exact_native_cash_positions_fifo_cost_fees_realized_new_inherited",
            "executionAuthority": False, "profitabilityEstablished": False,
            "limitations": ["Sealed report reconstruction, not independent original-bank chain replay or receipt authentication.",
                            "All fills must follow actual adoption; earlier activity requires original financial events.",
                            "Native commit order retained, never sorted by execution timestamps.",
                            "Negative execution-age matches retain financial attribution but are missing age labels, never zero durations.",
                            "Class realized values are gross; all fees remain new-trading expense, including inherited exits.",
                            "Filled inventory closure does not prove original physical order terminal or exclude future unfilled quantity.",
                            "No forward payouts, mark-based gains, hypothetical fills or allocation authority."]}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("source", type=Path)
    p.add_argument("--sha256", required=True)
    p.add_argument("--source-manifest", required=True, type=Path)
    p.add_argument("--out", required=True, type=Path)
    a = p.parse_args()
    raw = a.source.read_bytes()
    need(len(raw) <= 16*1024*1024 and sha(raw) == a.sha256, "RAW_SOURCE_PIN")
    manifest_raw = a.source_manifest.read_bytes()
    need(len(manifest_raw) <= 64*1024, "SOURCE_MANIFEST_BOUND")
    manifest = json.loads(manifest_raw)
    v = json.loads(raw)
    need(manifest["sourceSha256"] == a.sha256 and manifest["financialSequence"] == v["sequence"],
         "SOURCE_MANIFEST_BINDING")
    result = audit(v, a.sha256)
    result["sourceManifestSha256"] = sha(manifest_raw)
    result["sourceObservedAt"] = manifest["observedAt"]
    with a.out.open("x", encoding="utf-8", newline="\n") as f:
        json.dump(result, f, indent=2, sort_keys=True)
        f.write("\n")
    print(json.dumps({"parity": result["parity"], "accounting": result["accounting"],
                      "entryClasses": result["entryClasses"], "outputSha256": sha(a.out.read_bytes())}, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except (AuditError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise SystemExit(str(error) if isinstance(error, AuditError) else "SOURCE_SCHEMA") from None
