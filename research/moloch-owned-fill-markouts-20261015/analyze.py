#!/usr/bin/env python3
"""Streaming, offline owned-fill markouts. No networking or account operations."""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from datetime import datetime, timezone
from decimal import Decimal, localcontext
import hashlib
import json
from pathlib import Path
import re
import sys

SCALE = 1_000_000
HORIZONS_MS = (1000, 5000, 30000, 60000)
MAX_ROW_BYTES = 16 * 1024 * 1024


class Rejected(ValueError):
    pass


def check(condition, code):
    if not condition:
        raise Rejected(code)


def integer(value, positive=False):
    check(isinstance(value, int) and not isinstance(value, bool), "INTEGER")
    check(value > 0 if positive else value >= 0, "INTEGER_RANGE")
    return value


def identifier(value):
    check(isinstance(value, (str, int)) and not isinstance(value, bool), "IDENTIFIER")
    result = str(value)
    check(bool(re.fullmatch(r"[1-9][0-9]*", result)), "IDENTIFIER")
    return result


def units(value):
    check(isinstance(value, (str, int, Decimal)) and not isinstance(value, bool), "EXACT_DECIMAL")
    value = Decimal(value)
    check(value.is_finite() and len(value.as_tuple().digits) <= 100, "MONEY_RANGE")
    with localcontext() as context:
        context.prec = 128
        result = value * SCALE
        check(result == result.to_integral_value() and abs(result) < 1 << 127, "MONEY_PRECISION")
        return int(result)


def money(value):
    return f"{'-' if value < 0 else ''}{abs(value) // SCALE}.{abs(value) % SCALE:06d}"


def mean_ratio(numerator, denominator):
    if not denominator:
        return None
    with localcontext() as context:
        context.prec = 40
        return format(Decimal(numerator) / Decimal(denominator), ".6f")


def instant(value):
    check(isinstance(value, str), "CLOCK")
    m = re.fullmatch(r"(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)(?:\.(\d{1,9}))?(Z|[+-]\d\d:\d\d)", value)
    check(m is not None, "CLOCK")
    base = datetime.fromisoformat(m[1] + m[3].replace("Z", "+00:00"))
    elapsed = base.astimezone(timezone.utc) - datetime(1970, 1, 1, tzinfo=timezone.utc)
    return (elapsed.days * 86400 + elapsed.seconds) * 1_000_000_000 + int((m[2] or "").ljust(9, "0"))


def unique(pairs):
    result = {}
    for key, value in pairs:
        check(key not in result, "DUPLICATE_JSON_KEY")
        result[key] = value
    return result


DECODER = json.JSONDecoder(parse_float=Decimal, object_pairs_hook=unique)


def native_sealed_object(raw, expected_keys=None):
    """Verify exact native compact top-level body slices without reencoding."""
    text = raw.decode("utf-8")
    check(text.endswith("\n") and not text.endswith("\r\n"), "NATIVE_ROW_NEWLINE")
    text = text[:-1]
    check(text.startswith("{") and text.endswith("}"), "NATIVE_ROW_OBJECT")
    values, tokens, keys = {}, {}, []
    position = 1
    while position < len(text) - 1:
        key, end = DECODER.raw_decode(text, position)
        check(isinstance(key, str) and key not in values and text[end:end + 1] == ":", "NATIVE_ROW_FIELDS")
        start = end + 1
        check(text[start:start + 1] not in (" ", "\t", "\r", "\n"), "NATIVE_ROW_FIELDS")
        value, position = DECODER.raw_decode(text, start)
        keys.append(key)
        values[key], tokens[key] = value, text[start:position]
        if position == len(text) - 1:
            break
        check(text[position:position + 1] == ",", "NATIVE_ROW_FIELDS")
        position += 1
    check(keys == sorted(keys) and "hash" in keys, "NATIVE_ROW_LAYOUT")
    if expected_keys is not None:
        check(keys == expected_keys, "NATIVE_ROW_LAYOUT")
    expected_layout = "{" + ",".join('"' + key + '":' + tokens[key] for key in keys) + "}"
    check(expected_layout == text, "NATIVE_ROW_LAYOUT")
    body = "{" + ",".join('"' + key + '":' + tokens[key] for key in keys if key != "hash") + "}"
    check(hashlib.sha256(body.encode("utf-8")).hexdigest() == values["hash"], "JOURNAL_BODY_HASH")
    return values


def native_row(raw):
    """Native Journal body integrity, not serde reserialization/financial replay."""
    values = native_sealed_object(raw, ["event", "hash", "previous", "sequence"])
    check(isinstance(values["event"], dict), "JOURNAL_EVENT")
    return values


@dataclass
class Book:
    exchange: str
    market: str
    source: int
    request: int
    received: int
    processed: int
    bids: list
    asks: list
    status: str

    def fresh(self, at, bound):
        if self.status != "valid":
            return self.status
        if max(self.source, self.request, self.received, self.processed) > at:
            return "future_clock"
        if max(at - self.source, at - self.request, at - self.received) > bound:
            return "stale_clock"
        return "valid"

    def levels(self, side, action):
        if side == "YES":
            return self.bids if action == "sell" else self.asks
        return [(SCALE - p, n) for p, n in (self.asks if action == "sell" else self.bids)]

    def value(self, side, action, quantity):
        value, needed = 0, quantity
        for p, n in self.levels(side, action):
            taken = min(needed, n)
            value += p * taken
            needed -= taken
            if not needed:
                return value
        return None


def read_book(event, processed, bound):
    raw = event["book"]
    ex, mid = identifier(raw["exchangeId"]), identifier(raw["marketId"])
    source, request, received = (instant(raw[key]) for key in ("sourceAt", "requestStartedAt", "receivedAt"))
    check(received == instant(event["receivedAt"]), "BOOK_RECEIPT_MISMATCH")
    status = "valid"
    ref = event.get("sourceRef", {})
    if not (isinstance(ref, dict) and isinstance(ref.get("id"), str) and ref["id"]
            and isinstance(ref.get("sequence"), int) and ref["sequence"] > 0
            and isinstance(ref.get("hash"), str) and re.fullmatch(r"[0-9a-fA-F]{64}", ref["hash"])):
        status = "missing_recorded_source_ref"
    transport = raw.get("transport")
    if transport not in (None, "official_sig_api_v1", "official_sig_realtime"):
        status = "unrecognized_transport"
    # Native normalized REST books have no transport field. They retain the
    # official sourceRef and clocks; this is provenance from the native bank,
    # not independent retrieval/authentication of the archived raw receipt.
    if raw.get("invalid") is True:
        status = "invalidated_book"
    if source > received or request > received or received > processed:
        status = "invalid_clock"
    if max(processed - source, processed - request, processed - received) > bound:
        status = "stale_clock"
    bids, asks = [], []
    for key, target, descending in (("bids", bids, True), ("asks", asks, False)):
        for level in raw[key]:
            p, n = units(level["price"]), integer(level["quantity"], True)
            if not 0 < p < SCALE or p % 5000 or (target and (target[-1][0] <= p if descending else target[-1][0] >= p)):
                status = "invalid_depth"
            target.append((p, n))
    if not bids or not asks:
        status = "empty_depth"
    elif bids[0][0] >= asks[0][0]:
        status = "crossed_depth"
    return Book(ex, mid, source, request, received, processed, bids, asks, status)


@dataclass
class Fill:
    raw: dict
    order: dict
    exchange: str
    market: str
    side: str
    action: str
    quantity: int
    price: int
    executed: int
    observed: int
    decision: int
    journal_sequence: int | None = None
    pre_decision: Book | None = None
    pre_execution: Book | None = None
    pre_decision_gap: bool = False
    pre_execution_gap: bool = False
    posts: dict = field(default_factory=dict)
    late_historical_books: int = 0


def input_fills(snapshot):
    report = snapshot["report"]
    check(report["executionMode"] == "live" and report["fillModel"] == "actualAccountFills", "ACTUAL_LIVE_REPORT")
    check(units(report["fees"]) == 0, "NONZERO_FEES_UNSUPPORTED")
    check(integer(report["fills"]) == len(snapshot["fills"]), "REPORT_FILL_COUNT")
    orders = {}
    for order in snapshot["orders"]:
        qid = identifier(order["id"])
        check(qid not in orders, "DUPLICATE_ORDER")
        orders[qid] = order
    result, quantities, bindings, reverse = {}, Counter(), {}, {}
    for raw in snapshot["fills"]:
        fid, qid, actual = identifier(raw["sourceId"]), identifier(raw["orderId"]), identifier(raw["actualOrderId"])
        check(fid not in result and qid in orders, "FILL_ID_OR_ORDER")
        check(qid not in bindings or bindings[qid] == actual, "ORDER_BINDING_CHANGED")
        check(actual not in reverse or reverse[actual] == qid, "ORDER_REBOUND")
        bindings[qid], reverse[actual] = actual, qid
        order = orders[qid]
        ex, mid, s, action = identifier(raw["exchangeId"]), identifier(raw["marketId"]), raw["side"], raw["action"]
        check(s in ("YES", "NO") and action in ("buy", "sell") and raw.get("counterfactual") is False, "NONACTUAL_FILL")
        check(bool(re.fullmatch(r"[0-9a-fA-F]{64}", raw.get("receiptHash", ""))), "FILL_RECEIPT_HASH")
        check(identifier(order["exchangeId"]) == ex and identifier(order["marketId"]) == mid
              and order["side"] == s and order["action"] == action, "FILL_ORDER_SCOPE")
        n, p = integer(raw["quantity"], True), units(raw["price"])
        check(0 < p < SCALE and p % 5000 == 0 and units(raw["fee"]) == 0, "FILL_PRICE_OR_FEE")
        limit = units(order["price"])
        check(p <= limit if action == "buy" else p >= limit, "FILL_LIMIT")
        executed, observed = instant(raw["executedAt"]), instant(raw["at"])
        decision = integer(order["createdAt"]) * 1_000_000
        check(decision <= executed <= observed <= instant(report["at"]), "FILL_CLOCK")
        quantities[qid] += n
        result[fid] = Fill(raw, order, ex, mid, s, action, n, p, executed, observed, decision)
    for qid, order in orders.items():
        check(quantities[qid] == integer(order["filled"]) <= integer(order["quantity"], True), "ORDER_FILL_TOTAL")
    return result


def match_actual(event, fill, processed):
    raw = fill.raw
    check(event.get("source") == "official_sig_api_v1", "FILL_SOURCE")
    for key in ("orderId", "actualOrderId", "exchangeId", "marketId"):
        check(identifier(event[key]) == identifier(raw[key]), "JOURNAL_FILL_SCOPE")
    check(event["side"] == fill.side and event["action"] == fill.action
          and integer(event["quantity"], True) == fill.quantity and units(event["price"]) == fill.price
          and units(event["fee"]) == 0 and instant(event["executedAt"]) == fill.executed, "JOURNAL_FILL_FACTS")
    check(fill.executed <= instant(event["receivedAt"]) <= processed, "JOURNAL_FILL_RECEIPT")
    if fill.journal_sequence is None:
        check(processed == fill.observed and event["receiptHash"] == raw["receiptHash"], "JOURNAL_FILL_OBSERVATION")


def analyze(journal, snapshot, *, max_book_age_ms, post_window_ms, snapshot_sha256=None, snapshot_seal_verified=False):
    bound = integer(max_book_age_ms, True) * 1_000_000
    window = integer(post_window_ms, True) * 1_000_000
    fills = input_fills(snapshot)
    by_exchange = defaultdict(list)
    for fill in fills.values():
        by_exchange[fill.exchange].append(fill)
    end_sequence = integer(snapshot["sequence"], True)
    cutoff = instant(snapshot["report"]["at"])
    prefix, prefix_bytes, previous, sequence, last_clock = hashlib.sha256(), 0, "", 0, None
    counts, book_statuses = Counter(), Counter()
    for _ in range(end_sequence):
        raw = journal.readline(MAX_ROW_BYTES + 1)
        check(raw and len(raw) <= MAX_ROW_BYTES, "JOURNAL_MISSING_OR_OVERSIZE_ROW")
        row = native_row(raw)
        check(integer(row["sequence"], True) == sequence + 1 and row["previous"] == previous, "JOURNAL_SEQUENCE_OR_PREVIOUS")
        previous, sequence = row["hash"], row["sequence"]
        prefix.update(raw)
        prefix_bytes += len(raw)
        event = row["event"]
        received = instant(event["receivedAt"])
        clock = instant(event.get("processedAt", event["receivedAt"]))
        check(received <= clock <= cutoff and (last_clock is None or last_clock <= clock), "JOURNAL_CLOCK")
        last_clock = clock
        kind = event["type"]
        counts[kind if kind in ("book", "gap", "actual_fill") else "otherEvents"] += 1
        if kind == "actual_fill":
            fid = identifier(event["fillId"])
            check(fid in fills, "JOURNAL_FILL_NOT_IN_SNAPSHOT")
            fill = fills[fid]
            match_actual(event, fill, clock)
            if fill.journal_sequence is None:
                fill.journal_sequence = sequence
            else:
                counts["duplicateActualFillEvents"] += 1
        if kind not in ("book", "gap"):
            continue
        if kind == "book":
            book = read_book(event, clock, bound)
            ex = book.exchange
            book_statuses[book.status] += 1
        else:
            ex, book = identifier(event["exchangeId"]), None
        for fill in by_exchange.get(ex, ()):
            if book:
                check(book.market == fill.market, "BOOK_FILL_MARKET_SCOPE")
            before_recorded_fill = fill.journal_sequence is None or sequence < fill.journal_sequence
            if before_recorded_fill and clock <= fill.decision:
                fill.pre_decision, fill.pre_decision_gap = book, book is None
            if before_recorded_fill and clock <= fill.executed:
                fill.pre_execution, fill.pre_execution_gap = book, book is None
            if book and book.source <= fill.executed < book.processed:
                fill.late_historical_books += 1
            if book:
                for horizon in HORIZONS_MS:
                    target = fill.executed + horizon * 1_000_000
                    if horizon not in fill.posts and target <= book.source <= target + window:
                        # First book by observation order, regardless of price,
                        # freshness or depth: no favorable replacement search.
                        fill.posts[horizon] = book
    check(sequence == end_sequence and previous == snapshot["previous"], "SNAPSHOT_HEAD_MISMATCH")
    check(last_clock == cutoff, "SNAPSHOT_CLOCK_MISMATCH")
    check(all(fill.journal_sequence is not None for fill in fills.values()), "SNAPSHOT_FILL_MISSING_FROM_JOURNAL")

    def selected(book, gap, at, fill, need_both=False):
        if book is None:
            return ("invalidated_by_gap" if gap else "no_causally_earlier_book"), None
        status = book.fresh(at, bound)
        if status != "valid":
            return status, None
        action = "sell" if fill.action == "buy" else "buy"
        value = book.value(fill.side, action, fill.quantity)
        if value is None or (need_both and book.value(fill.side, fill.action, fill.quantity) is None):
            return "insufficient_full_quantity_depth", None
        return "priced", value

    def aggregate(items):
        result = {"fills": len(items), "quantity": sum(f.quantity for f in items),
                  "preDecisionCoverage": {}, "preExecutionCoverage": {}, "quotedGap": {},
                  "relativeToPriorBook": {}, "horizons": {}}
        decision_status, execution_status, relation = Counter(), Counter(), Counter()
        decision_by_fill, pre_by_fill = {}, {}
        quoted, quoted_n = 0, 0
        pre_mark, pre_n = 0, 0
        for f in items:
            dstatus, dvalue = selected(f.pre_decision, f.pre_decision_gap, f.decision, f, True)
            estatus, evalue = selected(f.pre_execution, f.pre_execution_gap, f.executed, f)
            decision_status[dstatus] += 1
            execution_status[estatus] += 1
            if dstatus == "priced":
                b = f.pre_decision
                # Opposite-side displayed gap is only a conditional quote
                # opportunity; it is not an earned round-trip spread.
                opposite_value = b.value(f.side, f.action, f.quantity)
                gap = opposite_value - units(f.order["price"]) * f.quantity if f.action == "buy" else units(f.order["price"]) * f.quantity - opposite_value
                decision_by_fill[id(f)] = gap
                quoted += gap
                quoted_n += f.quantity
            if estatus == "priced":
                pre_by_fill[id(f)] = evalue
                signed = evalue - f.price * f.quantity if f.action == "buy" else f.price * f.quantity - evalue
                pre_mark += signed
                pre_n += f.quantity
                bid, ask = f.pre_execution.levels(f.side, "sell")[0][0], f.pre_execution.levels(f.side, "buy")[0][0]
                label = ("at_or_above_prior_ask" if f.price >= ask else ("at_or_below_prior_bid" if f.price <= bid else "inside_prior_spread"))
                relation[f.action + ":" + label] += 1
        result["preDecisionCoverage"] = dict(decision_status)
        result["preExecutionCoverage"] = dict(execution_status)
        result["relativeToPriorBook"] = dict(relation)
        result["quotedGap"] = {"fullQuantityCoveredShares": quoted_n, "oppositeSideGapValue": money(quoted),
                               "quantityWeightedGapPerShare": mean_ratio(quoted, quoted_n * SCALE),
                               "earnedSpread": False}
        result["preExecutionImmediateClose"] = {"fullQuantityCoveredShares": pre_n, "hypotheticalMarkout": money(pre_mark)}
        lag = [f.observed - f.executed for f in items]
        result["fillObservationDelayMs"] = {"min": money(min(lag)) if lag else None, "max": money(max(lag)) if lag else None,
                                            "quantityWeightedMean": mean_ratio(sum((f.observed - f.executed) * f.quantity for f in items), sum(f.quantity for f in items) * 1_000_000)}
        decision_lag = [f.executed - f.decision for f in items]
        result["orderDecisionToFillExecutionDelayMs"] = {"min": money(min(decision_lag)) if decision_lag else None,
            "max": money(max(decision_lag)) if decision_lag else None,
            "quantityWeightedMean": mean_ratio(sum((f.executed - f.decision) * f.quantity for f in items), sum(f.quantity for f in items) * 1_000_000)}
        result["historicalBookUpdatesExcludedFromPreExecution"] = sum(f.late_historical_books for f in items)
        for horizon in HORIZONS_MS:
            statuses, quantity_statuses = Counter(), Counter()
            marked_n, mark, wins, losses, flat = 0, 0, 0, 0, 0
            observed_before_fill, delay_total, source_delay_total = 0, 0, 0
            change, changed_n, gap_diff, gap_diff_n = 0, 0, 0, 0
            for f in items:
                target = f.executed + horizon * 1_000_000
                b = f.posts.get(horizon)
                if b is None:
                    status = "right_censored_before_horizon" if cutoff < target else ("right_censored_inside_window" if cutoff < target + window else "no_book_in_completed_window")
                    value = None
                else:
                    status, value = selected(b, False, b.processed, f)
                statuses[status] += 1
                quantity_statuses[status] += f.quantity
                if status != "priced":
                    continue
                gross = value - f.price * f.quantity if f.action == "buy" else f.price * f.quantity - value
                mark += gross
                marked_n += f.quantity
                wins += gross > 0
                losses += gross < 0
                flat += gross == 0
                observed_before_fill += b.processed < f.observed
                delay_total += (b.processed - target) * f.quantity
                source_delay_total += (b.source - target) * f.quantity
                if id(f) in pre_by_fill:
                    change += value - pre_by_fill[id(f)] if f.action == "buy" else pre_by_fill[id(f)] - value
                    changed_n += f.quantity
                if id(f) in decision_by_fill:
                    gap_diff += gross - decision_by_fill[id(f)]
                    gap_diff_n += f.quantity
            result["horizons"][str(horizon)] = {"coverageFills": dict(statuses), "coverageShares": dict(quantity_statuses),
                "fullQuantityCoveredShares": marked_n, "hypotheticalMarkout": money(mark),
                "quantityWeightedMarkoutPerShare": mean_ratio(mark, marked_n * SCALE),
                "positiveFillMarks": wins, "negativeFillMarks": losses, "zeroFillMarks": flat,
                "postBookObservedBeforeFillObservation": observed_before_fill,
                "quantityWeightedBookObservationLatenessMs": mean_ratio(delay_total, marked_n * 1_000_000),
                "quantityWeightedBookSourceLatenessMs": mean_ratio(source_delay_total, marked_n * 1_000_000),
                "changeFromPreExecutionCloseValue": money(change), "changeComparisonShares": changed_n,
                "markMinusPreDecisionQuotedGap": money(gap_diff), "quotedGapComparisonShares": gap_diff_n}
        return result

    all_fills = list(fills.values())
    by_side = {}
    per_market = []
    for key in sorted({(f.action, f.side) for f in all_fills}):
        by_side[key[0] + "_" + key[1]] = aggregate([f for f in all_fills if (f.action, f.side) == key])
    for ex in sorted(by_exchange, key=int):
        subset = by_exchange[ex]
        per_market.append({"exchangeId": ex, "marketId": subset[0].market, **aggregate(subset)})
    return {"schemaVersion": 1, "analysis": "owned_fill_conditional_depth_markouts",
            "source": {"journalPrefixSequence": sequence, "journalPrefixBytes": prefix_bytes,
                       "journalPrefixSha256": prefix.hexdigest(), "journalHeadHash": previous,
                       "reportSnapshotSha256": snapshot_sha256,
                       "reportRawBodySealVerified": snapshot_seal_verified,
                       "rawNativeBodyHashChainVerified": True, "snapshotHeadMatched": True,
                       "nativeHelperRun": False, "serdeCanonicalReserializationVerified": False,
                       "financialReplayVerified": False},
            "parameters": {"maxBookAgeMs": max_book_age_ms, "postWindowMs": post_window_ms,
                           "horizonsAfterExecutionMs": list(HORIZONS_MS), "fees": "0.000000",
                           "boundsProvenance": "explicit_diagnostic_bounds_not_inferred_runtime_policy"},
            "clocks": {"fillExecutionClock": "actual official executedAt",
                       "observerDecisionClock": "order.createdAt millisecond lower bound",
                       "fillObservationClock": "first actual-fill processedAt matching report fill.at",
                       "preBookAvailabilityClock": "book processedAt with source/request/receipt clocks bounded",
                       "postHorizonClock": "book sourceAt relative to actual fill executedAt"},
            "journalEventCounts": dict(counts), "observedBookQualityCounts": dict(book_statuses),
            "all": aggregate(all_fills), "byActionSide": by_side, "perMarket": per_market,
            "profitabilityEstablished": False,
            "limitations": ["Owned fills alone are a selected sample; these are conditional diagnostics, not an unconditional strategy estimate.",
                            "Hypothetical full-depth closes do not establish future execution or actual realized P&L; horizons are separate, never additive trades.",
                            "Resting own liquidity and intervening traders are not removed; displayed depth can change before any actual order arrives.",
                            "Prior-book price relation does not identify maker/taker status or aggressor; public trades never create owned fills.",
                            "Order decision clock uses the conservative millisecond lower bound in order.createdAt; submillisecond same-event books may be excluded.",
                            "REST provenance comes from recorded native normalized sourceRef/clocks; raw receipt files and fresh account state are not independently authenticated.",
                            "Raw native row-body SHA256 links and the supplied report head are verified without number reencoding; this is not native engine replay or manifest authentication."]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--journal", required=True)
    parser.add_argument("--report", required=True)
    parser.add_argument("--max-book-age-ms", required=True, type=int)
    parser.add_argument("--post-window-ms", required=True, type=int)
    parser.add_argument("--output")
    args = parser.parse_args()
    try:
        raw = Path(args.report).read_bytes()
        snapshot = native_sealed_object(raw)
        with open(args.journal, "rb") as journal:
            result = analyze(journal, snapshot, max_book_age_ms=args.max_book_age_ms,
                             post_window_ms=args.post_window_ms, snapshot_sha256=hashlib.sha256(raw).hexdigest(),
                             snapshot_seal_verified=True)
        rendered = json.dumps(result, sort_keys=True, indent=2) + "\n"
        if args.output:
            check(Path(args.output).resolve() not in (Path(args.report).resolve(), Path(args.journal).resolve()), "OUTPUT_OVERWRITES_INPUT")
            Path(args.output).write_text(rendered, encoding="utf-8")
        else:
            print(rendered, end="")
    except Rejected as error:
        print("analysis rejected: " + str(error), file=sys.stderr)
        return 2
    except Exception:
        print("analysis rejected: INPUT_SCHEMA_OR_IO", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
