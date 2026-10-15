#!/usr/bin/env python3
"""Offline, exact actual-fill diagnosis. No account access or trading actions."""
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
MATURITY_NS = 600_000 * 1_000_000


class AnalysisError(ValueError):
    """Messages deliberately contain no raw account identifiers or values."""


def require(condition, code):
    if not condition:
        raise AnalysisError(code)


def units(value):
    require(not isinstance(value, (float, bool)), "EXACT_DECIMAL_REQUIRED")
    try:
        decimal = Decimal(value)
        require(decimal.is_finite() and len(decimal.as_tuple().digits) <= 100,
                "MONEY_RANGE")
        with localcontext() as context:
            context.prec = 128
            scaled = decimal * SCALE
            require(scaled == scaled.to_integral_value(), "MONEY_PRECISION")
            require(abs(scaled) <= (1 << 127) - 1, "MONEY_RANGE")
            return int(scaled)
    except (InvalidOperation, TypeError, ValueError, OverflowError) as exc:
        if isinstance(exc, AnalysisError):
            raise
        raise AnalysisError("MONEY_FORMAT") from None


def money(value):
    return f"{'-' if value < 0 else ''}{abs(value) // SCALE}.{abs(value) % SCALE:06d}"


def integer(value, *, positive=False):
    require(isinstance(value, int) and not isinstance(value, bool), "INTEGER_REQUIRED")
    require(value > 0 if positive else value >= 0, "INTEGER_RANGE")
    return value


def identifier(value):
    require(isinstance(value, (str, int)) and not isinstance(value, bool), "IDENTIFIER")
    value = str(value)
    require(bool(re.fullmatch(r"[1-9][0-9]*", value)), "IDENTIFIER")
    return value


def instant(value):
    """RFC3339 nanoseconds, including official seven-digit fractional clocks."""
    require(isinstance(value, str), "CLOCK_FORMAT")
    match = re.fullmatch(r"(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)(?:\.(\d{1,9}))?(Z|[+-]\d\d:\d\d)", value)
    require(match is not None, "CLOCK_FORMAT")
    try:
        base = datetime.fromisoformat(match[1] + match[3].replace("Z", "+00:00"))
        epoch = base.astimezone(timezone.utc) - datetime(1970, 1, 1, tzinfo=timezone.utc)
        return (epoch.days * 86400 + epoch.seconds) * 1_000_000_000 + int((match[2] or "").ljust(9, "0"))
    except ValueError:
        raise AnalysisError("CLOCK_FORMAT") from None


def duration_ms(ns):
    return money(ns)  # one millisecond = one million nanoseconds


def side(value):
    require(value in ("YES", "NO"), "SIDE")
    return value


def opposite(value):
    return "NO" if value == "YES" else "YES"


def price(value, *, opening=False):
    result = units(value)
    require(0 <= result <= SCALE if opening else 5_000 <= result <= SCALE - 5_000,
            "PRICE_RANGE")
    require(opening or result % 5_000 == 0, "PRICE_TICK")
    return result


@dataclass
class Lot:
    quantity: int
    price: int
    inherited: bool
    executed: int | None
    observed: int
    cohort: int | None = None


def analyze(envelope, adoption, *, source_hashes=None):
    """Return only redacted aggregates; inputs and FIFO matching remain private."""
    if adoption.get("type") != "actual_position_adoption" and "event" in adoption:
        adoption = adoption["event"]
    require(adoption.get("type") == "actual_position_adoption"
            and adoption.get("source") == "official_sig_api_v1"
            and adoption.get("complete") is True
            and adoption.get("authority") == "human_moloch_inherited_management_20261015",
            "ADOPTION_PROVENANCE")
    require(bool(re.fullmatch(r"[0-9a-fA-F]{64}", adoption.get("receiptHash", ""))), "ADOPTION_RECEIPT_HASH")
    report, fills, orders = envelope["report"], envelope["fills"], envelope["orders"]
    require(isinstance(fills, list) and isinstance(orders, list), "ENVELOPE_ARRAYS")
    sequence = integer(envelope["sequence"])
    require(report.get("executionMode") == "live" and report.get("fillModel") == "actualAccountFills",
            "ACTUAL_LIVE_REPORT_REQUIRED")
    require(integer(report["fills"]) == len(fills), "FILL_COUNT_MISMATCH")
    adopted_at = instant(adoption["processedAt"])
    require(instant(adoption["requestStartedAt"]) <= instant(adoption["receivedAt"]) <= adopted_at,
            "ADOPTION_CLOCK")
    require(not fills or instant(fills[0]["at"]) >= adopted_at, "FILLS_PRECEDE_ADOPTION")
    cutoff = instant(report["at"])
    require(cutoff >= adopted_at, "REPORT_CLOCK")
    seed = units(report["cash"]) + units(report["ownedCost"]) - units(report["realized"])
    require(seed > 0, "DERIVED_SEED")
    lots = defaultdict(deque)
    markets, stats = {}, {}
    seen_positions, seen_lots = set(), set()
    opening = 0

    def market(ex, market_id):
        require(ex not in markets or markets[ex] == market_id, "MARKET_SCOPE_CHANGED")
        markets[ex] = market_id
        if ex not in stats:
            stats[ex] = {"actualFills": 0, "buyShares": 0, "sellShares": 0,
                         "pairRedeemedShares": 0, "realizedGross": 0, "fees": 0,
                         "inheritedRealizedGross": 0, "reentriesAfterFlat": 0,
                         "basisClassifications": Counter(), "exitMix": {}}
        return stats[ex]

    for row in adoption["positions"]:
        ex, mid = identifier(row["exchangeId"]), identifier(row["marketId"])
        require(ex not in seen_positions, "DUPLICATE_ADOPTION_POSITION")
        seen_positions.add(ex)
        signed = row["quantity"]
        require(isinstance(signed, int) and not isinstance(signed, bool), "SIGNED_QUANTITY")
        s = "NO" if signed < 0 else "YES"
        total, principal, last_opened = 0, 0, None
        for lot in row["lots"]:
            lot_id = identifier(lot["lotId"])
            require(lot_id not in seen_lots, "DUPLICATE_ADOPTION_LOT")
            seen_lots.add(lot_id)
            require(side(lot["side"]) == s, "ADOPTION_SIDE_MISMATCH")
            n, p = integer(lot["quantity"], positive=True), price(lot["entryPrice"], opening=True)
            opened = instant(lot["openedAt"])
            require(opened <= instant(adoption["receivedAt"])
                    and (last_opened is None or opened >= last_opened), "ADOPTION_LOT_ORDER")
            last_opened = opened
            total += n
            principal += n * p
            lots[ex, s].append(Lot(n, p, True, None, adopted_at))
        require(total == abs(signed) and principal == units(row["lotPrincipal"]), "ADOPTION_PRINCIPAL_MISMATCH")
        opening += principal
        m = market(ex, mid)
        reported = units(row["reportedCostBasis"])
        difference = reported - principal
        classification = "exact" if difference == 0 else ("cent_rounding" if abs(difference) <= 5000 else "reported_discrepancy")
        require(reported >= 0 and units(row["reportedMinusLotPrincipal"]) == difference
                and row["basisClassification"] == classification, "ADOPTION_BASIS_METADATA")
        m["basisClassifications"][classification] += 1
    require(opening == units(adoption["openingCost"]) == units(report["openingInventoryCost"]), "OPENING_COST_MISMATCH")
    require(opening <= seed, "ADOPTION_FUNDING")
    cash, realized, inherited, fees = seed - opening, 0, 0, 0
    index = {}
    for order in orders:
        qid = identifier(order["id"])
        require(qid not in index, "DUPLICATE_ORDER")
        index[qid] = order
    counts = Counter()
    cohorts, closures, observed_fill_ids = [], [], set()
    order_filled, order_actual, actual_order = Counter(), {}, {}
    execution_times, delays = [], []
    previous_observed, maximum_execution = adopted_at, None
    traded, flattened = set(), set()

    def held(ex):
        return sum(l.quantity for s in ("YES", "NO") for l in lots[ex, s])

    def close(ex, s, n, proceeds, executed, observed, reason, style, mechanism):
        nonlocal inherited
        require(sum(l.quantity for l in lots[ex, s]) >= n, "UNCOVERED_CLOSE")
        cost, gross = 0, 0
        while n:
            lot = lots[ex, s][0]
            k = min(n, lot.quantity)
            gain = (proceeds - lot.price) * k
            cost += lot.price * k
            gross += gain
            age = None if lot.inherited else executed - lot.executed
            age_class = "inherited" if lot.inherited else ("executionClockReversal" if age < 0 else ("matured600000ms" if age >= MATURITY_NS else "freshUnder600000ms"))
            key = ("inherited" if lot.inherited else "new", reason, style, mechanism, age_class)
            mix = stats[ex]["exitMix"].setdefault(key, {"fragments": 0, "shares": 0, "gross": 0})
            mix["fragments"] += 1
            mix["shares"] += k
            mix["gross"] += gain
            if lot.inherited:
                inherited += gain
                stats[ex]["inheritedRealizedGross"] += gain
            else:
                cohort = cohorts[lot.cohort]
                cohort["closedShares"] += k
                cohort["gross"] += gain
                cohort["remaining"] -= k
                cohort["ages"].append(age)
                closures.append({"age": age, "quantity": k, "gross": gain, "forced": reason == "maximumAge",
                                 "lateEntryObservation": executed < lot.observed})
            lot.quantity -= k
            n -= k
            if lot.quantity == 0:
                lots[ex, s].popleft()
        return cost, gross

    for f in fills:  # Intentionally NEVER sorted by executedAt.
        require(f.get("counterfactual") is False, "NONACTUAL_FILL")
        source_id = identifier(f["sourceId"])
        require(source_id not in observed_fill_ids, "DUPLICATE_ACTUAL_FILL")
        observed_fill_ids.add(source_id)
        qid = identifier(f["orderId"])
        require(qid in index, "FILL_ORDER_MISSING")
        actual = identifier(f["actualOrderId"])  # validated privately, never returned
        require(qid not in order_actual or order_actual[qid] == actual, "ORDER_BINDING_CHANGED")
        require(actual not in actual_order or actual_order[actual] == qid, "ACTUAL_ORDER_REBOUND")
        order_actual[qid], actual_order[actual] = actual, qid
        require(bool(re.fullmatch(r"[0-9a-fA-F]{64}", f.get("receiptHash", ""))), "FILL_RECEIPT_HASH")
        q = index[qid]
        ex, mid, s = identifier(f["exchangeId"]), identifier(f["marketId"]), side(f["side"])
        action = f["action"]
        require(action in ("buy", "sell"), "FILL_ACTION")
        require(identifier(q["exchangeId"]) == ex and identifier(q["marketId"]) == mid
                and q["side"] == s and q["action"] == action, "FILL_ORDER_SCOPE")
        n, p, charge = integer(f["quantity"], positive=True), price(f["price"]), units(f["fee"])
        limit = price(q["price"])
        require(charge >= 0 and (p <= limit if action == "buy" else p >= limit), "FILL_ECONOMICS_BOUNDS")
        order_filled[qid] += n
        require(order_filled[qid] <= integer(q["quantity"], positive=True), "ORDER_OVERFILLED")
        executed, observed = instant(f["executedAt"]), instant(f["at"])
        require(previous_observed <= observed <= cutoff and executed <= observed, "FILL_OBSERVATION_CLOCK")
        require(executed >= integer(q["createdAt"]) * 1_000_000, "FILL_BEFORE_ORDER")
        previous_observed = observed
        if execution_times:
            counts["adjacentExecutionClockReversals"] += executed < execution_times[-1]
            counts["adjacentExecutionClockTies"] += executed == execution_times[-1]
        counts["executedBeforePreviouslyRecordedFill"] += maximum_execution is not None and executed < maximum_execution
        maximum_execution = executed if maximum_execution is None else max(executed, maximum_execution)
        execution_times.append(executed)
        delays.append(observed - executed)
        counts["observedAtLeast600000msAfterExecution"] += observed - executed >= MATURITY_NS
        reason = q.get("inventoryExitReason", "unspecified")
        style = q.get("executionStyle", "unspecified")
        # Only allow fixed public labels, never arbitrary account payload strings.
        require(reason in ("unspecified", "maximumAge"), "UNKNOWN_EXIT_REASON")
        require(style in ("unspecified", "inventoryAON"), "UNKNOWN_EXECUTION_STYLE")
        m = market(ex, mid)
        m["actualFills"] += 1
        m[action + "Shares"] += n
        m["fees"] += charge
        fees += charge
        realized -= charge
        gross = 0
        if action == "buy":
            if held(ex) == 0 and ex in flattened:
                m["reentriesAfterFlat"] += 1
            traded.add(ex)
            cash -= n * p + charge
            cohort = len(cohorts)
            cohorts.append({"exchange": ex, "side": s, "shares": n, "closedShares": 0,
                            "remaining": n, "gross": 0, "ages": []})
            lots[ex, s].append(Lot(n, p, False, executed, observed, cohort))
            pairs = min(sum(l.quantity for l in lots[ex, s]), sum(l.quantity for l in lots[ex, opposite(s)]))
            if pairs:
                # Native buy_fill_exact appends first and immediately redeems FIFO
                # YES/NO pairs. Attribute inherited proceeds at 1-currentBuyPrice.
                _, incoming_gain = close(ex, s, pairs, p, executed, observed, reason, style, "oppositeBuyPair")
                _, outgoing_gain = close(ex, opposite(s), pairs, SCALE - p, executed, observed, reason, style, "oppositeBuyPair")
                gross = incoming_gain + outgoing_gain
                cash += pairs * SCALE
                m["pairRedeemedShares"] += pairs
        else:
            cash += n * p - charge
            _, gross = close(ex, s, n, p, executed, observed, reason, style, "sell")
        realized += gross
        m["realizedGross"] += gross
        if held(ex) == 0 and ex in traded:
            flattened.add(ex)
        require(cash >= 0, "REPLAY_NEGATIVE_CASH")

    for qid, q in index.items():
        require(order_filled[qid] == integer(q["filled"]), "ORDER_FILLED_MISMATCH")
    owned = sum(l.price * l.quantity for queue in lots.values() for l in queue)
    require(cash + owned - seed == realized, "REPLAY_ACCOUNTING_IDENTITY")
    expected_positions = {}
    for ex, pos in report["positions"].items():
        ex = identifier(ex)
        for s in ("YES", "NO"):
            expected = [(integer(l["quantity"], positive=True), price(l["price"], opening=True)) for l in pos[s]]
            if expected:
                expected_positions[ex, s] = expected
    actual_positions = {key: [(l.quantity, l.price) for l in queue] for key, queue in lots.items() if queue}
    require(actual_positions == expected_positions, "ENDING_FIFO_POSITIONS_MISMATCH")
    for field, value in (("realized", realized), ("cash", cash), ("ownedCost", owned),
                         ("fees", fees), ("inheritedRealizedGross", inherited),
                         ("newTradingRealized", realized - inherited)):
        require(units(report[field]) == value, "REPORT_" + field.upper() + "_MISMATCH")
    require(cash + owned - realized == seed, "DERIVED_SEED_MISMATCH")

    per_market = []
    for ex in sorted(stats, key=int):
        m = stats[ex]
        mixes = [{"cohort": k[0], "inventoryExitReason": k[1], "executionStyle": k[2],
                  "mechanism": k[3], "actualHoldingAge": k[4], "closeFragments": v["fragments"],
                  "closedShares": v["shares"], "grossRealized": money(v["gross"])}
                 for k, v in sorted(m["exitMix"].items())]
        remaining_inherited = sum(l.quantity * l.price for s in ("YES", "NO") for l in lots[ex, s] if l.inherited)
        remaining_new = sum(l.quantity * l.price for s in ("YES", "NO") for l in lots[ex, s] if not l.inherited)
        net = m["realizedGross"] - m["fees"]
        market_closed = [c for c in cohorts if c["exchange"] == ex and c["remaining"] == 0]
        per_market.append({"exchangeId": ex, "marketId": markets[ex],
                           **{k: m[k] for k in ("actualFills", "buyShares", "sellShares", "pairRedeemedShares", "reentriesAfterFlat")},
                           "realized": money(net), "inheritedRealizedGross": money(m["inheritedRealizedGross"]),
                           "newTradingRealized": money(net - m["inheritedRealizedGross"]), "fees": money(m["fees"]),
                           "realizedSign": "win" if net > 0 else ("loss" if net < 0 else "flat"),
                           "basisClassifications": dict(m["basisClassifications"]), "exitMix": mixes,
                           "fullyClosedNewBuyLotsGross": {"wins": sum(c["gross"] > 0 for c in market_closed),
                                                          "losses": sum(c["gross"] < 0 for c in market_closed),
                                                          "flat": sum(c["gross"] == 0 for c in market_closed)},
                           "remainingUnmarkedCost": {"inherited": money(remaining_inherited), "new": money(remaining_new)}})
    closed = [c for c in cohorts if c["remaining"] == 0]
    forced = [c for c in closures if c["forced"]]

    def closure_summary(rows):
        valid = [c["age"] for c in rows if c["age"] >= 0]
        return {"fragments": len(rows), "shares": sum(c["quantity"] for c in rows),
                "grossRealized": money(sum(c["gross"] for c in rows)),
                "freshUnder600000msShares": sum(c["quantity"] for c in rows if 0 <= c["age"] < MATURITY_NS),
                "matured600000msShares": sum(c["quantity"] for c in rows if c["age"] >= MATURITY_NS),
                "executionClockReversalShares": sum(c["quantity"] for c in rows if c["age"] < 0),
                "entryObservedAfterExitExecutedShares": sum(c["quantity"] for c in rows if c["lateEntryObservation"]),
                "nonnegativeHoldingAgeMsMin": duration_ms(min(valid)) if valid else None,
                "nonnegativeHoldingAgeMsMax": duration_ms(max(valid)) if valid else None}

    return {"schemaVersion": 1, "analysis": "actual_fill_fifo_economics", "bankSequence": sequence,
            "sourceSha256": source_hashes or {}, "parity": "exact_fifo_quantities_cost_cash_realized_inherited_fees",
            "accounting": {"initialCashDerivedFromBankIdentity": money(seed), "cash": money(cash),
                           "openingInventoryCost": money(opening), "cashImmediatelyAfterAdoption": money(seed - opening),
                           "remainingUnmarkedCost": money(owned), "realized": money(realized),
                           "inheritedRealizedGross": money(inherited), "newTradingRealized": money(realized - inherited),
                           "fees": money(fees), "cashProvenance": "derived_bank_wallet_not_fresh_official_account_cash",
                           "inheritedBasis": "opening_lot_principal_not_official_account_realized_pnl"},
            "fills": {"recordedActualFills": len(fills), "ordering": "recorded_bank_order_never_resorted",
                      **{k: counts[k] for k in ("adjacentExecutionClockReversals", "adjacentExecutionClockTies", "executedBeforePreviouslyRecordedFill", "observedAtLeast600000msAfterExecution")},
                      "observationLagMsMin": duration_ms(min(delays)) if delays else None,
                      "observationLagMsMax": duration_ms(max(delays)) if delays else None},
            "newBuyLotCohorts": {"total": len(cohorts), "fullyClosed": len(closed),
                                 "partiallyClosed": sum(0 < c["closedShares"] < c["shares"] for c in cohorts),
                                 "unclosed": sum(c["closedShares"] == 0 for c in cohorts),
                                 "fullyClosedGrossWins": sum(c["gross"] > 0 for c in closed),
                                 "fullyClosedGrossLosses": sum(c["gross"] < 0 for c in closed),
                                 "fullyClosedGrossFlat": sum(c["gross"] == 0 for c in closed),
                                 "feesAllocatedToCohorts": False, "closedFragments": closure_summary(closures),
                                 "maximumAgeExitFragments": closure_summary(forced)},
            "marketRealizedSigns": dict(Counter(m["realizedSign"] for m in per_market)),
            "perMarket": per_market, "profitabilityEstablished": False,
            "limitations": ["No mark or liquidation value is assigned to remaining inventory.",
                            "Every fee is expensed to new trading, including fees on still-open lots.",
                            "Cohort wins/losses and exit mix are gross; opposite-buy incoming paired lots have zero gross attribution.",
                            "Ending cash parity is conditional on the seed derived from the same source bank identity; it is not an independent cash audit.",
                            "Exact source hashes and internal parity do not verify the full bank hash chain or fresh account state."]}


def load(path):
    raw = Path(path).read_bytes()
    return json.loads(raw, parse_float=Decimal), hashlib.sha256(raw).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", required=True, help="Private report.json envelope")
    parser.add_argument("--adoption", required=True, help="Private adoption event (or journal row)")
    parser.add_argument("--output", help="Public aggregate JSON; stdout when omitted")
    args = parser.parse_args()
    try:
        envelope, report_hash = load(args.report)
        adoption, adoption_hash = load(args.adoption)
        result = analyze(envelope, adoption, source_hashes={"report": report_hash, "adoption": adoption_hash})
        rendered = json.dumps(result, indent=2, sort_keys=True) + "\n"
        if args.output:
            require(Path(args.output).resolve() not in (Path(args.report).resolve(), Path(args.adoption).resolve()), "OUTPUT_OVERWRITES_INPUT")
            Path(args.output).write_text(rendered, encoding="utf-8")
        else:
            print(rendered, end="")
    except AnalysisError as exc:
        print("analysis rejected: " + str(exc), file=sys.stderr)
        return 2
    except (OSError, KeyError, TypeError, ValueError, AttributeError):
        print("analysis rejected: INPUT_SCHEMA_OR_IO", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
