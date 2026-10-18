"""File-only current-account input for the sequential portfolio search.

The byte pins authenticate supplied artifacts, not current API facts. Original
FIFO, source clocks, pending operations and absent ownership remain explicit.
This module cannot publish a nomination, approve a campaign or send an order.
"""
from copy import deepcopy
from hashlib import sha256
import json

from .native_snapshot import _sealed, _head
from .sequence_audit import amount, need


def digest(value):
    return sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
                             ensure_ascii=False).encode()).hexdigest()


def _quantity(n):
    need(type(n) is int and n >= 0, "ORIGINAL_LOT_QUANTITY")
    return n


def _remaining_inherited(source, fills, positions):
    """Mirror native FIFO consumption, including opposite-side actual BUYs."""
    remaining = {}
    for row in source.get("sourcePositions", []):
        ex = row["exchangeId"]
        for lot in row["lots"]:
            side = lot["side"]
            need(side in ("YES", "NO"), "ORIGINAL_INHERITED_SIDE")
            remaining.setdefault((ex, side), []).append(
                {"quantity": _quantity(lot["quantity"]), "price": lot["entryPrice"]})
    ids = set()
    for fill in fills:
        need(fill["counterfactual"] is False and fill["sourceId"] not in ids,
             "ACTUAL_UNIQUE_FILL_REQUIRED")
        ids.add(fill["sourceId"])
        side = fill["side"]
        need(side in ("YES", "NO") and fill["action"] in ("buy", "sell"), "ACTUAL_FILL_SCOPE")
        close_side = ("NO" if side == "YES" else "YES") if fill["action"] == "buy" else side
        left = _quantity(fill["quantity"])
        need(left > 0, "ACTUAL_FILL_QUANTITY")
        lots = remaining.get((fill["exchangeId"], close_side), [])
        while left and lots:
            take = min(left, lots[0]["quantity"])
            left -= take
            lots[0]["quantity"] -= take
            if not lots[0]["quantity"]:
                lots.pop(0)
    # Any surviving inherited principal must still be an exact FIFO prefix.
    for (ex, side), lots in remaining.items():
        held = deepcopy(positions.get(ex, {}).get(side, []))
        for lot in lots:
            left = lot["quantity"]
            while left:
                need(held and amount(held[0]["price"]) == amount(lot["price"]),
                     "INHERITED_FIFO_PREFIX_CHANGED")
                take = min(left, held[0]["quantity"])
                need(take > 0, "INHERITED_FIFO_PREFIX_CHANGED")
                left -= take
                held[0]["quantity"] -= take
                if not held[0]["quantity"]:
                    held.pop(0)
    return sorted({ex for (ex, _), lots in remaining.items() if lots})


def compile_current(report_bytes, view_bytes, *, expected_report_sha256,
                    expected_view_sha256, registry, conditional_future_legs):
    """Compile a matched sealed report/view; never infer a physical permission.

    All active native quotes retain their market slots. Their local retirement
    needs a separate actual worker action. Pending real acknowledgments and
    unresolved attempts make this research planning input unavailable.
    """
    need(sha256(report_bytes).hexdigest() == expected_report_sha256, "CURRENT_REPORT_PIN")
    need(sha256(view_bytes).hexdigest() == expected_view_sha256, "CURRENT_VIEW_PIN")
    sealed = _sealed(report_bytes)
    view = json.loads(view_bytes)
    need(view["schema"] == "tartarus_native_research_view_v1"
         and view["researchOnly"] is True and view["executionAuthority"] is False,
         "CURRENT_VIEW_CLASSIFICATION")
    head = view["nativeHead"]
    _head(head, sealed["manifestHash"])
    need(head["sequence"] == sealed["sequence"] and head["previous"] == sealed["previous"]
         and view["manifestHash"] == sealed["manifestHash"], "MATCHED_NATIVE_BOUNDARY_REQUIRED")
    report = sealed["report"]
    account = view["account"]
    need(account["positions"] == report["positions"] and account["cash"] == report["cash"]
         and account["reservedCash"] == report["reservedCash"]
         and account["fillCount"] == len(sealed["fills"])
         and account["actualFillsSha256"] == digest(sealed["fills"])
         and account["positionsSha256"] == digest(report["positions"]), "CURRENT_FINANCIAL_BINDING")
    need(type(conditional_future_legs) is int and 0 <= conditional_future_legs <= 128,
         "EXPLICIT_CONDITIONAL_LEG_BOUND")
    scope = view["scope"]["exchanges"]
    byex = {r["exchangeId"]: r for r in registry}
    need(len(byex) == len(registry), "DUPLICATE_REGISTRY_EXCHANGE")
    positions = {}
    for ex, sides in report["positions"].items():
        need(set(sides) <= {"YES", "NO"}, "CURRENT_POSITION_SIDE")
        for side, lots in sides.items():
            for lot in lots:
                if _quantity(lot["quantity"]):
                    need(ex in byex and ex in scope, "CURRENT_UNMODELED_HOLDING")
                    need(byex[ex]["marketId"] == scope[ex], "CURRENT_CONTRACT_MAPPING_CHANGED")
                    positions.setdefault(ex, {}).setdefault(side, []).append(deepcopy(lot))
    cost = sum(l["quantity"] * amount(l["price"]) for sides in positions.values()
               for lots in sides.values() for l in lots)
    need(cost == amount(report["ownedCost"]), "CURRENT_ORIGINAL_FIFO_COST")
    ownership = {}
    for record in view["managedOwnership"]:
        ex = record["exchangeId"]
        need(ex not in ownership, "DUPLICATE_CURRENT_OWNERSHIP")
        body = record["sourceBodyText"]
        need(sha256(body.encode()).hexdigest() == record["sourceSha256"], "ORIGINAL_OWNERSHIP_BODY")
        original = json.loads(body)
        need(all(record[k] == value for k, value in original.items())
             and original["producer"] == "moloch_original_cohort_owner"
             and original["generationSha256"] == view["generationSha256"]
             and original["bankManifestHash"] == sealed["manifestHash"]
             and type(record["rankPaused"]) is bool
             and record["rankPaused"] == original["handoffEstablished"]
             and original["positionsSha256"] == digest(report["positions"].get(ex))
             and record["positions"] == report["positions"].get(ex), "CURRENT_OWNERSHIP_BINDING")
        ownership[ex] = deepcopy(record)
    active = [q for q in sealed["orders"] if q["retired"] is False]
    need(view["activeNativeOrders"] == active, "CURRENT_ACTIVE_QUOTES_CHANGED")
    retained = sorted({q["exchangeId"] for q in active})
    need(all(ex in byex for ex in retained), "CURRENT_UNMODELED_RESERVED_MARKET")
    inherited = report.get("inheritedInventory")
    need(type(inherited) is dict, "ORIGINAL_INHERITED_HISTORY_REQUIRED")
    inherited_exchanges = _remaining_inherited(inherited, sealed["fills"], report["positions"])
    policy = view["policy"]
    need(digest(policy) == report["policyHash"], "CURRENT_POLICY_BINDING")
    financial = {"nativeSequence": sealed["sequence"],
        "nativeReportRawSha256": expected_report_sha256, "cash": report["cash"],
        "reservedCash": report["reservedCash"],
        "ownedCost": report["ownedCost"], "positions": positions,
        "maximumCommitted": policy["maximumCommitted"],
        "maximumMarketCost": policy["maximumMarketCost"],
        "maximumMarkets": policy["maximumMarkets"], "physicalOrderQuantity": policy["quantity"],
        "conditionalFutureLegs": conditional_future_legs,
        "inheritedExchanges": inherited_exchanges, "freshMarketLimit": inherited["freshMarketLimit"],
        "sourceModelGeneration": view["generationSha256"], "retainedMarketExchanges": retained,
        "excludedBuyExchanges": sorted(set(view["managedTargetAdmissionProjections"]) - set(ownership)),
        "buyRequiresPriorCoveredSellExchanges": sorted(ex for ex, record in ownership.items()
            if ex in positions and not record["rankPaused"])}
    need(amount(policy["feePerShare"]) == 0 and policy["feeBps"] == 0,
         "ZERO_FEE_SOLVER_REQUIRES_ORIGINAL_ZERO_FEE_POLICY")
    reasons = []
    if view["unresolvedOrdinaryAttempt"]:
        reasons.append("ORIGINAL_UNRESOLVED_ATTEMPT")
    if view["knownUnclosedAcknowledgments"]:
        reasons.append("ORIGINAL_PHYSICAL_ORDERS_REQUIRE_RECONCILIATION")
    return {"schema": "tartarus_current_portfolio_research_v1",
        "status": "unavailable" if reasons else "available_research_input",
        "unavailableReasons": reasons, "financial": None if reasons else financial,
        "nativeSequence": sealed["sequence"], "reportSha256": expected_report_sha256,
        "viewSha256": expected_view_sha256, "originalReportAt": report["at"],
        "originalViewWrittenAt": view["writtenAt"], "managedOwnership": ownership,
        "originalBooks": deepcopy(view["books"]),
        "originalObservationReceiptClocks": deepcopy(view["originalObservationReceiptClocks"]),
        "inheritedAttribution": {"receiptHash": inherited.get("receiptHash"),
            "managementStartedAt": inherited.get("managementStartedAt"),
            "basisClassifications": {row["exchangeId"]: row.get("basisClassification")
                                     for row in inherited["sourcePositions"]}},
        "knownUnclosedAcknowledgments": deepcopy(view["knownUnclosedAcknowledgments"]),
        "actualAccountCashCredited": "0.000000", "retirementPerformed": False,
        "terminalDebtEstablished": False,
        "freshAccountOrBookAuthority": False, "executionAuthority": False}


def main():
    import argparse
    from pathlib import Path
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--report", type=Path, required=True)
    p.add_argument("--view", type=Path, required=True)
    p.add_argument("--report-sha256", required=True)
    p.add_argument("--view-sha256", required=True)
    p.add_argument("--registry", type=Path, required=True)
    p.add_argument("--conditional-future-legs", type=int, required=True)
    p.add_argument("--output", type=Path, required=True)
    args = p.parse_args()
    registry = json.loads(args.registry.read_text(encoding="utf-8"))
    if type(registry) is dict:
        registry = registry["rows"]
    result = compile_current(args.report.read_bytes(), args.view.read_bytes(),
        expected_report_sha256=args.report_sha256, expected_view_sha256=args.view_sha256,
        registry=registry, conditional_future_legs=args.conditional_future_legs)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"status": result["status"], "nativeSequence": result["nativeSequence"],
                      "reasons": result["unavailableReasons"], "executionAuthority": False}))


if __name__ == "__main__":
    main()
