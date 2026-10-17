"""Attribute only native reviewed_fifo_profit_turnover activity, without trading."""
import argparse
import datetime as dt
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import re
import zipfile

REASON = "reviewed_fifo_profit_turnover"
SCALE = 1_000_000
HEX = re.compile(r"[0-9a-f]{64}")
DECODER = json.JSONDecoder(parse_float=Decimal)


def need(ok, why):
    if not ok:
        raise ValueError(why)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def load(raw):
    return json.loads(raw, parse_float=Decimal)


def sealed(raw):
    """Hash original compact bytes, never Python-reserialized decimal numbers."""
    raw = raw.strip()
    value = load(raw)
    claimed = value.get("hash")
    need(isinstance(claimed, str) and HEX.fullmatch(claimed), "sealed_hash_missing")
    middle = b',"hash":"' + claimed.encode() + b'"'
    first = b'{"hash":"' + claimed.encode() + b'",'
    if raw.count(middle) == 1:
        unsigned = raw.replace(middle, b"", 1)
    elif raw.startswith(first):
        unsigned = b"{" + raw[len(first):]
    else:
        raise ValueError("original_compact_outer_hash_fragment_required")
    need(sha(unsigned) == claimed, "original_raw_seal_mismatch")
    return value


def fields(raw):
    """Return original JSON value spans to reconstruct the typed Receipt order."""
    text = raw.decode("utf-8").strip()
    need(text.startswith("{"), "original_object_required")
    result, pos = {}, 1
    while text[pos] != "}":
        key, end = DECODER.raw_decode(text, pos)
        need(isinstance(key, str) and key not in result and text[end] == ":", "original_object_field")
        start = end + 1
        _, end = DECODER.raw_decode(text, start)
        result[key] = text[start:end].encode("utf-8")
        need(text[end] in ",}", "original_object_separator")
        pos = end + 1 if text[end] == "," else end
    need(pos == len(text) - 1, "original_object_trailing_bytes")
    return result


def receipt_sha(raw):
    parts = fields(raw)
    order = ("endpoint", "requestStartedAt", "receivedAt", "status", "data")
    need(set(parts) == set(order), "typed_original_receipt_fields")
    # baal_transport_service::Receipt derives Serialize in this field order.
    return sha(b"{" + b",".join(b'"' + k.encode() + b'":' + parts[k] for k in order) + b"}")


def units(value):
    number = Decimal(value)
    need(number.is_finite() and number >= 0 and number * SCALE == (number * SCALE).to_integral_value(), "nonnegative_micro_amount_required")
    return int(number * SCALE)


def money(number):
    need(type(number) is int, "integer_micro_amount_required")
    return ("-" if number < 0 else "") + f"{abs(number) // SCALE}.{abs(number) % SCALE:06}"


def positive(number):
    need(type(number) is int and number > 0, "positive_integer_required")
    return number


def clock(text):
    match = re.fullmatch(r"(.+T\d\d:\d\d:\d\d)(?:\.(\d{1,9}))?(Z|[+-]\d\d:\d\d)", text)
    need(match is not None, "original_utc_clock_required")
    base = dt.datetime.fromisoformat(match[1] + match[3].replace("Z", "+00:00"))
    elapsed = base - dt.datetime(1970, 1, 1, tzinfo=dt.timezone.utc)
    return (elapsed.days * 86400 + elapsed.seconds) * 1_000_000_000 + int((match[2] or "").ljust(9, "0"))


def chain(raw, sequence, previous):
    rows = []
    need(not raw or raw.endswith(b"\n"), "torn_original_journal")
    for original in raw.splitlines():
        row = sealed(original)
        need(row["sequence"] == sequence + 1 and row["previous"] == previous, "original_journal_chain")
        sequence, previous = row["sequence"], row["hash"]
        rows.append((row, original))
    return rows, sequence, previous


def scope_equal(value, q):
    return all(value[k] == q[k] for k in ("marketId", "exchangeId", "side", "action"))


def metadata_matches(event, quote):
    meta = quote["reviewedInventoryExit"]
    keys = ("originSha256", "reviewSha256", "bookSha256", "executionMode", "maximumPassiveQuantity", "modelSha256", "sourceSha256", "pricingObjective", "pricingBasis", "minimumNetSalePrice", "forecastExpectedPayout")
    return (event.get("type") == "reviewed_cohort_exit_reserved_v1" and event.get("reason") == REASON
            and event.get("marketId") == quote["marketId"] and event.get("exchangeId") == quote["exchangeId"]
            and event.get("side") == quote["side"] and event.get("limitPrice") == quote["price"]
            and all(event.get(k) == meta[k] for k in keys)
            and clock(event.get("processedAt", event["receivedAt"])) // 1_000_000 == quote["createdAt"])


def analyze(path, expected_sha):
    need(HEX.fullmatch(expected_sha) and path.stat().st_size <= 512 * 1024 * 1024, "bounded_pinned_private_zip_required")
    zip_hash = sha(path.read_bytes())
    need(zip_hash == expected_sha, "private_zip_sha256_mismatch")
    with zipfile.ZipFile(path) as z:
        need(len(z.namelist()) <= 128 and len(z.namelist()) == len(set(z.namelist())), "zip_member_bound_or_duplicate")
        need(sum(i.file_size for i in z.infolist()) <= 512 * 1024 * 1024, "uncompressed_input_bound")
        manifest_raw = z.read("packet-manifest.json")
        pins = load(manifest_raw)["files"]
        need(set(z.namelist()) == {*pins, "packet-manifest.json"}, "unlisted_private_member")
        for name, pin in pins.items():
            raw = z.read(name)
            need(len(raw) == pin["bytes"] and sha(raw) == pin["sha256"], "private_member_pin:" + name)
        report = sealed(z.read("report.original.json"))
        head = sealed(z.read("head.original.json"))
        anchor = sealed(z.read("anchor-head.original.json"))
        need(all(report[k] == head[k] for k in ("manifestHash", "sequence", "previous")) and anchor["manifestHash"] == head["manifestHash"], "original_report_boundary")
        native, native_sequence, native_previous = chain(z.read("native.suffix.jsonl"), anchor["sequence"], anchor["previous"])
        need((native_sequence, native_previous) == (head["sequence"], head["previous"]), "suffix_does_not_end_at_report")
        dispatch, dispatch_sequence, dispatch_previous = chain(z.read("dispatch.original.ndjson"), 0, "0" * 64)
    cutoff = clock(report["report"]["at"])
    for row, _ in native:
        event = row["event"]
        need(clock(event.get("processedAt", event["receivedAt"])) <= cutoff, "native_event_after_report")
    quotes = {q["id"]: q for q in report["orders"]}
    need(len(quotes) == len(report["orders"]), "duplicate_native_qid")
    selected = {qid: q for qid, q in quotes.items() if q.get("reviewedInventoryExit", {}).get("reason") == REASON}
    reservations = report["report"].get("reviewedInventoryExitReservations", {})
    need({int(qid) for qid, meta in reservations.items() if meta.get("reason") == REASON} == set(selected), "report_metadata_membership")
    activities, actual_owners, all_fill_ids = [], {}, set()
    for qid, q in sorted(selected.items()):
        meta = q["reviewedInventoryExit"]
        need(q["action"] == "sell" and q["side"] in ("YES", "NO") and meta == reservations[str(qid)], "selected_owned_sell_metadata")
        need(meta["executionMode"] == "passiveValueLimit" and meta["originKind"] == "original_entry_cohort"
             and meta["pricingObjective"]["kind"] == "fifoProfitTurnover", "mixed_fifo_objective")
        basis = meta["pricingBasis"]
        need(basis["pricingPolicy"] == "improveAskOneTickFifoFloor" and basis["pricingObjective"] == meta["pricingObjective"], "fifo_pricing_basis")
        for key in ("originSha256", "reviewSha256", "bookSha256", "modelSha256", "sourceSha256", "cohortOriginSha256"):
            need(isinstance(meta[key], str) and HEX.fullmatch(meta[key]), "fifo_metadata_hash:" + key)
        need(HEX.fullmatch(basis["fifoCostSha256"]) and HEX.fullmatch(basis["positionsSha256"]), "fifo_cost_source_hash")
        quantity, planned = positive(q["quantity"]), positive(basis["quantity"])
        need(quantity <= planned and q["filled"] <= quantity, "native_ack_quantity_bounds")
        limit, cost_upper = units(q["price"]), units(basis["maximumFifoUnitCost"])
        gain = units(meta["pricingObjective"]["minimumNetGain"])
        need(5000 <= gain <= SCALE and units(meta["minimumNetSalePrice"]) == units(basis["minimumNetSalePrice"]) == cost_upper + gain, "fifo_minimum_net_floor")
        need(0 < limit < SCALE and limit % 5000 == 0 and limit >= cost_upper + gain, "native_fifo_limit")
        origins = [row for row, _ in native if metadata_matches(row["event"], q)]
        need(len(origins) == 1 and origins[0]["event"]["quantity"] == planned, "original_reservation_not_uniquely_in_suffix")
        origin = origins[0]["event"]
        original_basis_raw = fields(fields(next(raw for row, raw in native if row["hash"] == origins[0]["hash"]))["event"])["pricingBasis"]
        events = [(row, raw) for row, raw in dispatch if row["event"]["qid"] == qid]
        intents = [(row, raw) for row, raw in events if row["event"]["kind"] == "intent"]
        attempts = [row for row, _ in events if row["event"]["kind"] == "attempt"]
        acks = [(row, raw) for row, raw in events if row["event"]["kind"] in ("ack", "recovered_ack")]
        need(len(intents) <= 1 and len(attempts) <= 1 and len(acks) <= 1, "repeat_original_operation")
        if intents:
            intent = intents[0][0]["event"]
            original = intent["quote"]
            need(scope_equal(original, q) and all(original[k] == q[k] for k in ("id", "price", "createdAt", "activationAt", "expiresAt", "reviewedInventoryExit"))
                 and original["quantity"] == planned and intent.get("fundamental") is not None, "original_intent_quote_changed")
            spec = intent["spec"]
            body = spec["body"]
            tag = intent["fundamental"]
            need(tag["qid"] == qid and tag["runId"] == origin["bankScope"]["runId"]
                 and tag["bankManifestHash"] == report["manifestHash"] and HEX.fullmatch(tag["bindingSha256"])
                 and tag["entryQid"] in [entry["qid"] for entry in origin["cohortOrigin"]["entries"]], "original_fundamental_cohort_tag")
            need(spec["method"] == "POST" and spec["endpoint"] == "/orders" and body["action"] == "sell"
                 and body["exchangeId"] == q["exchangeId"] and body["side"] == q["side"].lower()
                 and body["quantity"] == planned and units(body["price"]) == limit
                 and body["tournamentId"] == origin["bankScope"]["tournamentId"]
                 and body["idempotencyKey"] == f"{tag['runId']}:{qid}"
                 and clock(body["expirationDate"]) == q["expiresAt"] * 1_000_000, "original_post_terms")
            need(clock(intent["at"]) <= cutoff, "target_intent_after_report")
        if attempts:
            need(bool(intents) and intents[0][0]["sequence"] < attempts[0]["sequence"] and clock(attempts[0]["event"]["at"]) <= cutoff, "target_attempt_order_or_cutoff")
        actual, binding_rows = None, []
        if acks:
            row, raw = acks[0]
            need(bool(attempts) and attempts[0]["sequence"] < row["sequence"], "ack_without_original_attempt")
            event = row["event"]
            key = "receipt" if event["kind"] == "ack" else "order"
            receipt = event[key]
            raw_receipt = fields(fields(raw)["event"])[key]
            receipt_hash = receipt_sha(raw_receipt)
            data = receipt["data"]
            actual = str(data["orderId"] if key == "receipt" else data["id"])
            need(actual.isdigit() and not actual.startswith("0") and receipt["status"] == 200
                 and data["exchangeId"] == q["exchangeId"] and data["side"] == q["side"].lower()
                 and data["action"] == "sell" and data["quantity"] == quantity
                 and units(data["price"] if key == "receipt" else data["priceLimit"]) == limit
                 and clock(receipt["requestStartedAt"]) <= clock(receipt["receivedAt"]) <= cutoff, "ack_exact_terms_or_cutoff")
            need(actual not in actual_owners or actual_owners[actual] == qid, "actual_order_bound_to_multiple_qids")
            actual_owners[actual] = qid
            binding_rows = [nr for nr, _ in native if nr["event"].get("type") == "actual_order_ack" and nr["event"].get("orderId") == qid
                            and nr["event"].get("actualOrderId") == actual and nr["event"].get("receiptHash") == receipt_hash]
            need(bool(binding_rows) and all(scope_equal(nr["event"], q) and nr["event"]["quantity"] == quantity and units(nr["event"]["price"]) == limit for nr in binding_rows), "original_native_ack_binding_missing")
        all_native_acks = [nr for nr, _ in native if nr["event"].get("type") == "actual_order_ack" and nr["event"].get("orderId") == qid]
        need(not all_native_acks or actual is not None, "native_ack_without_original_dispatch_ack")
        need(all(nr["event"]["actualOrderId"] == actual and scope_equal(nr["event"], q)
                 and nr["event"]["quantity"] == quantity and units(nr["event"]["price"]) == limit for nr in all_native_acks), "conflicting_native_ack")
        fills = [f for f in report["fills"] if f.get("orderId") == qid]
        seen, filled, gross, fees, fill_row_hashes = set(), 0, 0, 0, []
        for f in fills:
            n, price, charge = positive(f["quantity"]), units(f["price"]), units(f["fee"])
            need(actual is not None and f["actualOrderId"] == actual and f["counterfactual"] is False and scope_equal(f, q)
                 and price >= limit and HEX.fullmatch(f["receiptHash"])
                 and q["createdAt"] * 1_000_000 <= clock(f["executedAt"]) <= clock(f["at"]) <= cutoff, "actual_fill_owned_terms")
            token = (actual, f["sourceId"])
            need(token not in seen and token not in all_fill_ids and str(f["sourceId"]).isdigit(), "duplicate_or_missing_actual_fill_identity")
            seen.add(token)
            all_fill_ids.add(token)
            matches = [nr for nr, _ in native if nr["event"].get("type") == "actual_fill" and nr["event"].get("orderId") == qid
                       and nr["event"].get("actualOrderId") == actual and nr["event"].get("fillId") == f["sourceId"]
                       and nr["event"].get("receiptHash") == f["receiptHash"] and nr["event"].get("executedAt") == f["executedAt"]
                       and nr["event"].get("processedAt", nr["event"].get("receivedAt")) == f["at"]]
            need(bool(matches) and all(scope_equal(nr["event"], q) and nr["event"]["source"] == "official_sig_api_v1"
                                      and nr["event"]["quantity"] == n and units(nr["event"]["price"]) == price and units(nr["event"]["fee"]) == charge
                                      and clock(nr["event"]["executedAt"]) <= clock(nr["event"]["receivedAt"]) <= clock(f["at"]) for nr in matches), "original_native_actual_fill_missing_or_conflicting")
            filled += n
            gross += n * price
            fees += charge
            fill_row_hashes.extend(nr["hash"] for nr in matches)
        need(filled == q["filled"] and filled <= quantity, "native_report_fill_total")
        for nr, _ in native:
            e = nr["event"]
            if e.get("type") == "actual_fill" and e.get("orderId") == qid:
                need(any(e["actualOrderId"] == f["actualOrderId"] and e["fillId"] == f["sourceId"]
                         and scope_equal(e, q) and e["quantity"] == f["quantity"] and units(e["price"]) == units(f["price"])
                         and units(e["fee"]) == units(f["fee"]) and e["executedAt"] == f["executedAt"] for f in fills), "native_fill_missing_or_conflicting_with_report")
        terminal_rows = [nr for nr, _ in native if nr["event"].get("type") == "actual_order_terminal" and nr["event"].get("orderId") == qid]
        for nr in terminal_rows:
            event = nr["event"]
            need(actual is not None and event["actualOrderId"] == actual and scope_equal(event, q)
                 and event["fillsComplete"] is True and event["filledQuantity"] == filled, "original_terminal_incomplete_or_mixed")
        activities.append({"nativeQid": qid, "marketId": q["marketId"], "exchangeId": q["exchangeId"], "side": q["side"], "action": "sell",
                           "preparedQuantity": planned, "nativeQuantity": quantity, "limit": q["price"], "nativeRetired": q["retired"],
                           "originalIntentCount": len(intents), "originalAttemptCount": len(attempts), "originalAckCount": len(acks),
                           "nativeBindingVerified": bool(binding_rows), "unacknowledgedAttemptUnknown": bool(attempts and not acks),
                           "nativeCompleteTerminalVerified": bool(terminal_rows),
                           "dispatchClosedRowObserved": any(row["event"]["kind"] == "closed" for row, _ in events),
                           "actualFillCount": len(fills), "actualFilledQuantity": filled, "actualGrossProceeds": money(gross),
                           "actualSellFees": money(fees), "actualNetProceeds": money(gross - fees),
                           "maximumFifoUnitCostUpper": money(cost_upper), "acquisitionCostUpperForFilledQuantity": money(filled * cost_upper),
                           "conservativeGainLowerBound": money(gross - fees - filled * cost_upper), "exactRealizedProfitClaim": False,
                           "minimumNetGainPerShareObjective": money(gain), "pricingBasisSha256": sha(original_basis_raw),
                           "pricingSourceHashes": {"fifoCostSha256": basis["fifoCostSha256"], "positionsSha256": basis["positionsSha256"],
                                                   "bookSha256": meta["bookSha256"], "reviewSha256": meta["reviewSha256"],
                                                   "originSha256": meta["originSha256"], "modelSha256": meta["modelSha256"], "sourceSha256": meta["sourceSha256"]},
                           "nativeCreatedAtMs": q["createdAt"], "originalReservationProcessedAt": origin.get("processedAt", origin["receivedAt"]),
                           "reviewCurrentUntilMsAtReservation": origin["reviewCurrentUntilMs"],
                           "originalReservationSourceRef": origin["sourceRef"],
                           "nativeReservationRowSha256": origins[0]["hash"], "nativeAckRowSha256": [r["hash"] for r in binding_rows],
                           "nativeFillRowSha256": sorted(set(fill_row_hashes)), "nativeTerminalRowSha256": [r["hash"] for r in terminal_rows],
                           "firstNativeRecordedFillExecutionAt": fills[0]["executedAt"] if fills else None,
                           "lastNativeFillObservedAt": fills[-1]["at"] if fills else None})
    for row, _ in native:
        e = row["event"]
        if e.get("reason") == REASON:
            need(any(metadata_matches(e, q) for q in selected.values()), "new_reason_reservation_missing_from_report")
    return {"schemaVersion": 1, "reasonFilter": REASON, "executionAuthority": False, "counterfactualExecutionOccurred": False,
            "source": {"privateZipSha256": zip_hash, "packetManifestSha256": sha(manifest_raw),
                       "originalFilePins": {k: pins[k] for k in ("report.original.json", "dispatch.original.ndjson", "native.suffix.jsonl", "anchor-head.original.json", "head.original.json")},
                       "nativeAnchorSequence": anchor["sequence"], "nativeSequence": head["sequence"], "nativePrevious": head["previous"],
                       "nativeSuffixRowsVerified": native_sequence - anchor["sequence"],
                       "reportAtUtc": report["report"]["at"], "dispatchRowsVerified": dispatch_sequence, "dispatchFinalSha256": dispatch_previous,
                       "analyzerSha256": sha(Path(__file__).read_bytes())},
            "summary": {"nativeQuotes": len(activities), "originalAttempts": sum(r["originalAttemptCount"] for r in activities),
                        "originalAcks": sum(r["originalAckCount"] for r in activities), "actualFillCount": sum(r["actualFillCount"] for r in activities),
                        "actualFilledQuantity": sum(r["actualFilledQuantity"] for r in activities),
                        "actualNetProceeds": money(sum(int(Decimal(r["actualNetProceeds"]) * SCALE) for r in activities)),
                        "conservativeGainLowerBound": money(sum(int(Decimal(r["conservativeGainLowerBound"]) * SCALE) for r in activities))},
            "activity": activities,
            "limitations": ["Only the explicit new FIFO-profit reason is attributed; other strategies and whole-account PnL are excluded.",
                            "A quote or ACK is not a fill. Filled native retirement is not complete terminal-tape proof.",
                            "A zero-fill open/unknown order is not a confirmed nonfill. No fill probability is estimated.",
                            "The cost bound is frozen native pricing metadata, including native acquisition-fee upper rounding, not independently replayed exact FIFO realized cost.",
                            "Native accepted fill rows, original dispatch and report membership are checked; underlying fill-tape receipt bodies and full Rust financial replay are not authenticated here.",
                            "The suffix must include original reservation, ACK and accepted fill evidence for selected quotes; missing or after-report proof is refused.",
                            "Public output contains native qids, aggregates and source hashes; actual venue order/fill identifiers and raw receipts remain private."]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("private_zip", type=Path)
    parser.add_argument("--zip-sha256", required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    result = analyze(args.private_zip, args.zip_sha256)
    args.out.write_text(json.dumps(result, sort_keys=True, indent=2) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
