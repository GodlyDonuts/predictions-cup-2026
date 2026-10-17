"""Finite FIFO arithmetic census. No network, execution authority or fill model."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import zipfile

SCALE = 1_000_000
TICK = 5_000
ZIP_SHA256 = "224e5c55c55bf473a5641283fa61b97072ab618cd7e67bc4313a1bf3086efa67"
REPORT_SHA256 = "9001d57bed3eff63a2cdc0d5da3b4ead2a64139f942f7ca460ecd99bb5454337"
REVIEWS_SHA256 = "e294520aff0a0b2571e0b7f57a98aff0f7c7a4026b70bdcf15d7f6176a4650f2"
CANDIDATE_SOURCE = "111abff39d9391d7e1840fc2ae6d7e785828c06c"
EXCHANGES = {"1042", "1051", "1067", "1068", "851", "865", "872", "885", "966", "977", "993"}


def need(ok, reason):
    if not ok:
        raise ValueError(reason)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def units(text):
    need(isinstance(text, str) and re.fullmatch(r"\d+\.\d{6}", text), "exact_six_decimal_amount_required")
    whole, fraction = text.split(".")
    return int(whole) * SCALE + int(fraction)


def money(value):
    return ("-" if value < 0 else "") + f"{abs(value) // SCALE}.{abs(value) % SCALE:06}"


def fee(price, quantity, bps, per_share):
    # moloch/native/src/lib.rs::fee: ceil(gross*bps/10000), then quantity fee.
    return (price * quantity * bps + 9999) // 10000 + quantity * per_share


def limit_for(worst_cost, minimum_gain, bps, per_share):
    # One-share rounding is conservative for every positive partial quantity.
    return next((p for p in range(TICK, SCALE, TICK)
                 if p - fee(p, 1, bps, per_share) >= worst_cost + minimum_gain), None)


def prefix(lots, quantity):
    remaining, basis, worst = quantity, 0, 0
    for lot in lots:
        take = min(remaining, lot["quantity"])
        if take:
            basis += take * units(lot["price"])
            worst = max(worst, units(lot["price"]))
            remaining -= take
        if not remaining:
            break
    need(remaining == 0, "fifo_quantity_exceeds_owned")
    return basis, worst


def profitable_prefix(lots, price, gain, bps, per_share):
    net_unit = price - fee(price, 1, bps, per_share)
    quantity = 0
    for lot in lots:  # Original native FIFO order; never sort by price/time.
        if net_unit < units(lot["price"]) + gain:
            break
        quantity += lot["quantity"]
    return quantity


def scenario_value(review):
    objective = review.get("valuationObjective", {}).get("kind")
    need((review["version"] == 1 and objective is None)
         or (review["version"] == 2 and objective == "terminalSettlement"), "unsupported_value_objective")
    p = units(review["ordinaryProbability"])
    e = units(review["exceptionalProbabilityAllowance"])
    exceptional = units(review["minimumExceptionalPayout"])
    need(max(p, e, exceptional) <= SCALE, "probability_or_payout_range")
    # structure() subtracts margin; active SELL adds it back exactly once.
    return ((SCALE - e) * p + e * exceptional) // SCALE


def analyze(private_zip, books_path):
    need(private_zip.stat().st_size == 4_851_416, "wrong_original_zip_size")
    need(sha(private_zip.read_bytes()) == ZIP_SHA256, "wrong_original_zip_sha256")
    with zipfile.ZipFile(private_zip) as z:
        manifest_raw = z.read("packet-manifest.json")
        pins = json.loads(manifest_raw)["files"]
        need(len(z.namelist()) == len(set(z.namelist())), "duplicate_zip_member")
        need(set(z.namelist()) == {*pins, "packet-manifest.json"}, "unlisted_zip_member")
        need(sum(i.file_size for i in z.infolist()) < 64 * 1024 * 1024, "original_input_bound")
        for name, pin in pins.items():
            raw = z.read(name)
            need(len(raw) == pin["bytes"] and sha(raw) == pin["sha256"], "original_member_pin:" + name)
        need(pins["report.original.json"]["sha256"] == REPORT_SHA256, "wrong_report_pin")
        need(pins["fundamental.original.jsonl"]["sha256"] == REVIEWS_SHA256, "wrong_reviews_pin")
        envelope = json.loads(z.read("report.original.json"))
        manifest = json.loads(z.read("bank-manifest.original.json"))
        capture = json.loads(z.read("capture.json"))
        control = json.loads(z.read("control.original.json"))
        reviews, review_hashes = {}, {}
        for raw in z.read("fundamental.original.jsonl").splitlines():
            event = json.loads(raw)["event"]
            if "review" in event:
                review = event["review"]
                reviews[review["exchangeId"]] = review
                review_hashes[review["exchangeId"]] = event["sha256"]
            if event["kind"] == "review_generation":
                for retained in event["reviews"]:
                    review = retained["review"]
                    reviews[review["exchangeId"]] = review
                    review_hashes[review["exchangeId"]] = retained["sha256"]
    report = envelope["report"]
    need(envelope["sequence"] == capture["sequence"] == 492737
         and report["at"] == capture["reportAt"] == "2026-10-17T15:23:08.558Z", "original_boundary_mismatch")
    policy = manifest["policy"]
    for field in ("feeBps", "feePerShare", "quantity", "inventoryMinimumGain"):
        need(policy[field] == control["policy"][field], "original_policy_disagreement:" + field)
    bps, per_share, order_cap = policy["feeBps"], units(policy["feePerShare"]), policy["quantity"]
    need((bps, per_share, order_cap, units(policy["inventoryMinimumGain"])) == (0, 0, 10000, TICK), "unexpected_original_fee_or_order_policy")
    books_raw = books_path.read_bytes()
    books = json.loads(books_raw)
    need(books["authenticatedOriginalBook"] is False and books["currentExecutableQuote"] is False, "book_provenance_changed")
    need(len(books["books"]) == 11 and {b["exchangeId"] for b in books["books"]} == EXCHANGES, "historical_book_scope")
    need(set(reviews) == EXCHANGES, "original_review_scope")
    rows = []
    for book in books["books"]:
        ex = book["exchangeId"]
        review = reviews[ex]
        side = review["side"]
        need(side in ("YES", "NO"), "original_side")
        positions = report["positions"].get(ex, {})
        need(not positions.get("NO" if side == "YES" else "YES", []), "mixed_opposite_inventory")
        lots = positions.get(side, [])
        for lot in lots:
            need(type(lot["quantity"]) is int and lot["quantity"] > 0 and units(lot["price"]) < SCALE, "original_lot")
        yb, ya = units(book["yesBid"]), units(book["yesAsk"])
        need(TICK <= yb < ya <= SCALE - TICK and yb % TICK == ya % TICK == 0, "historical_price")
        need(all(type(book[k]) is int and book[k] > 0 for k in ("yesBidQuantity", "yesAskQuantity")), "historical_depth")
        bid, ask, bid_depth, ask_depth = ((yb, ya, book["yesBidQuantity"], book["yesAskQuantity"])
                                        if side == "YES" else (SCALE - ya, SCALE - yb, book["yesAskQuantity"], book["yesBidQuantity"]))
        held = sum(lot["quantity"] for lot in lots)
        max_quantity = min(held, order_cap)
        basis, worst = prefix(lots, max_quantity)
        capped_floor = limit_for(worst, TICK, bps, per_share) if max_quantity else None
        bid_quantity = min(profitable_prefix(lots, bid, TICK, bps, per_share), bid_depth, order_cap)
        bid_basis, _ = prefix(lots, bid_quantity)
        competitive_quantity = min(profitable_prefix(lots, ask, TICK, bps, per_share), order_cap)
        competitive_basis, competitive_worst = prefix(lots, competitive_quantity)
        competitive_limit = limit_for(competitive_worst, TICK, bps, per_share) if competitive_quantity else None
        value = scenario_value(review)
        active_native = [q for q in envelope["orders"] if q["exchangeId"] == ex and q["retired"] is False]
        row = {"exchangeId": ex, "marketId": review["marketId"], "side": side,
               "heldQuantity": held, "activeNativeQuoteCountAtReport": len(active_native),
               "historicalBid": money(bid), "historicalBidQuantity": bid_depth,
               "historicalAsk": money(ask), "historicalAskQuantity": ask_depth,
               "firstFifoLotQuantity": lots[0]["quantity"] if lots else 0,
               "firstFifoLotUnitPrincipal": lots[0]["price"] if lots else None,
               "profitableBidDepthCappedQuantity": bid_quantity,
               "bidDepthConditionalNetGain": money(bid * bid_quantity - fee(bid, bid_quantity, bps, per_share) - bid_basis),
               "nativeOrderCappedQuantity": max_quantity, "orderCappedPrefixWorstUnitPrincipal": money(worst) if held else None,
               "orderCappedPrefixOneTickNetFloor": money(capped_floor) if capped_floor is not None else None,
               "competitiveProfitableFifoPrefixQuantity": competitive_quantity,
               "competitiveOneTickNetLimit": money(competitive_limit) if competitive_limit is not None else None,
               "competitiveFifoBasis": money(competitive_basis),
               "competitiveConditionalProceeds": money(competitive_limit * competitive_quantity) if competitive_limit is not None else None,
               "competitiveConditionalNetGain": money(competitive_limit * competitive_quantity - fee(competitive_limit, competitive_quantity, bps, per_share) - competitive_basis) if competitive_limit is not None else None,
               "reviewedScenarioValue": money(value), "reviewedValueTickFloor": money(limit_for(value, 0, bps, per_share)),
               "originalTypedReviewSha256": review_hashes[ex], "inheritedOriginReview": "existingOrigin" in review,
               "reviewVersion": review["version"], "reviewedAtMs": review["reviewedAtMs"], "reviewExpiresAtMs": review["expiresAtMs"],
               "earliestOriginalSourceExpiryMs": min(s["observedAtMs"] + s["maximumAgeMs"] for s in review["sources"]),
               "originalReviewSources": [{k: s[k] for k in ("kind", "sha256", "publishedAtMs", "observedAtMs", "maximumAgeMs")} for s in review["sources"]]}
        rows.append(row)
    need(sum(r["profitableBidDepthCappedQuantity"] for r in rows) == 0, "historical_result_changed")
    return {"schemaVersion": 1, "analysis": "original_fifo_one_tick_turnover_census", "executionAuthority": False,
            "actualOrGuaranteedFillClaim": False, "counterfactualExecutionOccurred": False,
            "candidateSourceReviewed": CANDIDATE_SOURCE,
            "source": {"privateZipSha256": ZIP_SHA256, "packetManifestSha256": sha(manifest_raw),
                       "reportSha256": REPORT_SHA256, "fundamentalSidecarSha256": REVIEWS_SHA256,
                       "bankManifestRawSha256": pins["bank-manifest.original.json"]["sha256"],
                       "bankManifestHash": envelope["manifestHash"], "nativeSequence": envelope["sequence"], "nativePrevious": envelope["previous"],
                       "reportAtUtc": report["at"], "capturedAtUtc": capture["observedAt"], "actualFillCount": len(envelope["fills"]),
                       "analyzerSha256": sha(Path(__file__).read_bytes()), "historicalBookFixtureSha256": sha(books_raw)},
            "historicalBookProvenance": {k: v for k, v in books.items() if k != "books"},
            "economics": {"minimumNetGainPerShare": money(TICK), "officialTick": money(TICK),
                          "feeBpsFromOriginalPolicy": bps, "feePerShareFromOriginalPolicy": money(per_share),
                          "nativeRecordedFees": report["fees"], "nativePerOrderQuantityCap": order_cap,
                          "inheritedPreAdoptionFeesKnown": False, "gainExcludesUnknownPreAdoptionFees": True,
                          "averageCostQualificationUsed": False, "pricesAndLotsReordered": False},
            "summary": {"reviewScopes": len(rows), "heldScopes": sum(r["heldQuantity"] > 0 for r in rows),
                        "flatScopes": [r["exchangeId"] for r in rows if r["heldQuantity"] == 0],
                        "oneTickProfitableHistoricalBidQuantity": sum(r["profitableBidDepthCappedQuantity"] for r in rows),
                        "competitivePassiveScopes": [r["exchangeId"] for r in rows if r["competitiveProfitableFifoPrefixQuantity"] > 0]},
            "markets": rows,
            "limitations": ["Book fixture is a root-supplied historical quotation without original receipt pins, not a current execution witness.",
                            "Original report holdings are earlier than the supplied quotation; intervening holdings/liability changes are not observed here.",
                            "Competitive means a limit no higher than the supplied own-side ask, not a predicted fill or demonstrated queue/depth allocation.",
                            "Conditional proceeds and gains require actual fills at the limit; displayed ask quantity is competing supply, never execution capacity.",
                            "Original FIFO unit principal excludes independently unknown inherited pre-adoption fees.",
                            "Model scenario values are explicit uncalibrated reviewed assumptions, not measured fair values or early-exit cash values.",
                            "No allocation/ranking override or current review, account, ownership, source, reservation or physical dispatch approval is granted.",
                            "ZIP and all member raw hashes are checked; Rust financial replay and venue receipt authentication are not performed by this analyzer."]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("private_zip", type=Path)
    parser.add_argument("--books", type=Path, default=Path(__file__).with_name("historical-top-books.json"))
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    result = analyze(args.private_zip, args.books)
    args.out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
