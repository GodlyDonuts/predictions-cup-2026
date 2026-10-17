#!/usr/bin/env python3
"""Offline ordinary entry cohorts; release-era labels are not admission witnesses."""
import argparse
from collections import Counter, defaultdict, deque
import importlib.util
import json
from pathlib import Path
import sys

AUDITOR_LF_SHA = "130582056d6ef922db3d033583ec8153560982e7c1ada776d7dc2452c47489c5"


def load_auditor():
    path = Path(__file__).resolve().parent.parent / "moloch-realized-attribution-20261017/audit.py"
    raw = path.read_bytes().replace(b"\r\n", b"\n")
    import hashlib
    if hashlib.sha256(raw).hexdigest() != AUDITOR_LF_SHA:
        raise ValueError("PINNED_AUDITOR_SOURCE_CHANGED")
    name = "ordinary_cohort_pinned_auditor"
    spec = importlib.util.spec_from_loader(name, loader=None)
    module = importlib.util.module_from_spec(spec)
    module.__file__ = str(path)
    sys.modules[name] = module
    exec(compile(raw, str(path), "exec"), module.__dict__)
    return module


A = load_auditor()


def regime(created, evidence):
    """Use original quote creation, NEVER the potentially much later fill."""
    side, defensive = evidence["milestones"]
    if created < A.clock(side["deployedAt"]):
        return "pre_first_enforce_quote"
    if created < A.clock(side["firstFreshAt"]):
        return "enforce_startup_transition_unknown"
    if created < A.clock(defensive["deployedAt"]):
        return "enforce_era_before_first_defensive_quote"
    if created < A.clock(defensive["firstFreshAt"]):
        return "defensive_startup_transition_unknown"
    return "after_first_defensive_fresh_quote"


def evidence_verified(path):
    raw = path.read_bytes()
    e = json.loads(raw)
    root = Path(__file__).resolve().parents[2]
    for d in e["sources"]:
        p = root / d["path"]
        A.need(p.resolve().is_relative_to(root), "EVIDENCE_REPOSITORY_PATH")
        body = p.read_bytes().replace(b"\r\n", b"\n")
        A.need(A.sha(body) == d["gitLfSha256"], "DEPLOYMENT_SOURCE_PIN")
        v = json.loads(body)
        for key, expected in d["fields"].items():
            A.need(v[key] == expected, "DEPLOYMENT_SOURCE_FIELD")
    s = e["sources"]
    A.need(len(s) == 5 and s[1]["fields"]["ordinarySideEdge"] == "enforce"
           and s[4]["fields"]["defensiveCancel"] is True, "ACTIVATION_MODES")
    for milestone, rollout, fresh in [(e["milestones"][0], s[0], s[2]), (e["milestones"][1], s[3], s[4])]:
        A.need(milestone["deployedAt"] == rollout["fields"]["at"]
               and milestone["firstFreshAt"] == fresh["fields"]["checkedAt"]
               and fresh["fields"]["freshTradingVerified"] is True
               and fresh["fields"]["pid"] == rollout["fields"]["newPid"], "MILESTONE_ORIGINAL_MEMBERSHIP")
    return e, A.sha(raw)


def cohorts(v, e, source_sha):
    # Reproduce all cash, FIFO lots, cost, fees and origin splits first.
    base = A.audit(v, source_sha)
    A.need(base["accounting"]["fees"] == "0.000000", "NONZERO_FEES_NEED_EXPLICIT_COHORT_ALLOCATION")
    q = {o["id"]: o for o in v["orders"]}
    ordinary = {o["nativeQid"]: o for o in base["entryNativeQids"] if o["entryClass"] == "ordinary"}
    lots = defaultdict(deque)
    for p in v["report"]["inheritedInventory"]["sourcePositions"]:
        for l in p["lots"]:
            lots[p["exchangeId"], l["side"]].append([l["quantity"], A.units(l["entryPrice"]), None, None])
    rows, seen, fragments = {}, set(), []

    def close(ex, side, n, proceeds, f, mechanism):
        while n:
            l = lots[ex, side][0]
            take = min(n, l[0])
            if l[2] in ordinary:
                age = A.clock(f["executedAt"]) - l[3]
                fragments.append({"entryNativeQid": l[2], "exitNativeQid": f["orderId"],
                                  "quantity": take, "grossUnits": take*(proceeds-l[1]),
                                  "reason": q[f["orderId"]].get("inventoryExitReason", "other"),
                                  "mechanism": mechanism, "executionHoldingAgeNs": age if age >= 0 else None,
                                  "closeObservedAt": f["at"]})
            n -= take
            l[0] -= take
            if l[0] == 0:
                lots[ex, side].popleft()

    for f in v["fills"]:  # Original native commit order, already validated above.
        identity = (f["actualOrderId"], str(f["sourceId"]))
        if identity in seen:
            continue
        seen.add(identity)
        ex, side, n, p = f["exchangeId"], f["side"], f["quantity"], A.units(f["price"])
        if f["action"] == "buy":
            lots[ex, side].append([n, p, f["orderId"], A.clock(f["executedAt"])])
            other = "NO" if side == "YES" else "YES"
            pairs = min(sum(l[0] for l in lots[ex, side]), sum(l[0] for l in lots[ex, other]))
            if pairs:
                close(ex, side, pairs, p, f, "opposite_buy_netting")
                close(ex, other, pairs, A.SCALE-p, f, "opposite_buy_netting")
        else:
            close(ex, side, n, p, f, "sell")
    grouped = defaultdict(Counter)
    for qid, original in ordinary.items():
        matched = [f for f in fragments if f["entryNativeQid"] == qid]
        A.need(sum(f["quantity"] for f in matched) == original["closedFilledShares"]
               and sum(f["grossUnits"] for f in matched) == A.units(original["grossRealized"]), "COHORT_FIFO_PARITY")
        incoming = [f for f in v["fills"] if f["orderId"] == qid]
        born = q[qid]["createdAt"]*1_000_000
        tag = regime(born, e)
        b = grouped[tag]
        gain = A.units(original["grossRealized"])
        b["entries"] += 1
        b["shares"] += original["boughtShares"]
        b["principal"] += A.units(original["entryPrincipal"])
        b["gross"] += gain
        b["losses" if gain < 0 else "wins" if gain > 0 else "flat"] += 1
        for f in matched:
            kind = "maximumAgeSell" if f["reason"] == "maximumAge" and f["mechanism"] == "sell" else f["mechanism"]
            b[kind+"Gross"] += f["grossUnits"]
            b[kind+"Shares"] += f["quantity"]
            if f["executionHoldingAgeNs"] is None:
                b["missingAgeShares"] += f["quantity"]
            else:
                b["knownAgeShares"] += f["quantity"]
                b["ageNumerator"] += f["executionHoldingAgeNs"]*f["quantity"]
                b[kind+"AgeKnownShares"] += f["quantity"]
                b[kind+"AgeNumerator"] += f["executionHoldingAgeNs"]*f["quantity"]
        rows[qid] = original | {"quoteCreatedAtMs": q[qid]["createdAt"], "releaseEra": tag,
                               "firstOriginalExecutionAt": min(f["executedAt"] for f in incoming),
                               "lastOriginalExecutionAt": max(f["executedAt"] for f in incoming),
                               "lastMatchedCloseObservedAt": max((f["closeObservedAt"] for f in matched), default=None),
                               "maximumAgeSellShares": sum(f["quantity"] for f in matched if f["reason"] == "maximumAge" and f["mechanism"] == "sell"),
                               "maximumAgeSellGross": A.money(sum(f["grossUnits"] for f in matched if f["reason"] == "maximumAge" and f["mechanism"] == "sell")),
                               "oppositeBuyNettingGross": A.money(sum(f["grossUnits"] for f in matched if f["mechanism"] == "opposite_buy_netting")),
                               "ageKnownShares": sum(f["quantity"] for f in matched if f["executionHoldingAgeNs"] is not None),
                               "ageMissingExecutionOrderingShares": sum(f["quantity"] for f in matched if f["executionHoldingAgeNs"] is None),
                               "quantityWeightedAgeNsNumerator": str(sum(f["quantity"]*f["executionHoldingAgeNs"] for f in matched if f["executionHoldingAgeNs"] is not None)),
                               "entryAdmissionGuardWitness": "unknown_not_in_financial_report"}
    A.need(sum(b["gross"] for b in grouped.values()) == A.units(base["entryClasses"]["ordinary"]["grossRealized"]), "ALL_ORDINARY_COHORTS")
    return {"schemaVersion": 1, "sourceSha256": source_sha, "analyzerSha256": A.sha(Path(__file__).read_bytes()),
            "pinnedAuditorGitLfSha256": AUDITOR_LF_SHA, "boundary": base["boundary"], "reportAt": base["reportAt"],
            "ordinaryRealized": base["entryClasses"]["ordinary"]["grossRealized"],
            "eraDefinition": "original native quote creation relative to evidenced first deployments/freshness; NOT per-order admission or causal guard effect",
            "eras": {k: {"entries": b["entries"], "shares": b["shares"], "entryPrincipal": A.money(b["principal"]),
                         "grossRealized": A.money(b["gross"]), "losingEntries": b["losses"], "winningEntries": b["wins"], "flatEntries": b["flat"],
                         "maximumAgeSellGross": A.money(b["maximumAgeSellGross"]), "maximumAgeSellShares": b["maximumAgeSellShares"],
                         "oppositeBuyNettingGross": A.money(b["opposite_buy_nettingGross"]), "oppositeBuyNettingLegShares": b["opposite_buy_nettingShares"],
                         "ageKnownShares": b["knownAgeShares"], "ageMissingExecutionOrderingShares": b["missingAgeShares"],
                         "maximumAgeSellKnownAgeShares": b["maximumAgeSellAgeKnownShares"],
                         "maximumAgeSellQuantityWeightedAgeNsNumerator": str(b["maximumAgeSellAgeNumerator"]),
                         "quantityWeightedAgeNsNumerator": str(b["ageNumerator"])} for k, b in sorted(grouped.items())},
            "entries": [rows[k] for k in sorted(rows)],
            "executionAuthority": False, "causalGuardEffectClaimed": False, "profitabilityEstablished": False,
            "unknowns": ["No original per-entry attempt/control hash or side-edge/defensive assessment in this financial report.",
                         "Subsequent deployment continuity and whether defensive cancellation triggered per order are unverified.",
                         "Old native quotes can fill after new releases; execution date alone is not the entry regime.",
                         "Held filled-inventory closure is not original order terminal proof.",
                         "No invented book, microprice, queue, counterfactual fill or future return labels."]}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("source", type=Path)
    p.add_argument("--sha256", required=True)
    p.add_argument("--out", required=True, type=Path)
    a = p.parse_args()
    raw = a.source.read_bytes()
    A.need(len(raw) <= 16*1024*1024 and A.sha(raw) == a.sha256, "RAW_SOURCE_PIN")
    e, evidence_sha = evidence_verified(Path(__file__).with_name("activation-evidence.json"))
    result = cohorts(json.loads(raw), e, a.sha256)
    result["activationEvidenceSha256"] = evidence_sha
    with a.out.open("x", encoding="utf-8", newline="\n") as f:
        json.dump(result, f, indent=2, sort_keys=True)
        f.write("\n")
    print(json.dumps({"ordinaryRealized": result["ordinaryRealized"], "eras": result["eras"],
                      "outputSha256": A.sha(a.out.read_bytes())}, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except (A.AuditError, KeyError, ValueError, TypeError) as error:
        raise SystemExit(str(error) if isinstance(error, A.AuditError) else "SOURCE_SCHEMA_OR_CODE_PIN") from None
