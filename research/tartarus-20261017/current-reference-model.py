"""Build declared terminal sensitivities from retained cache/review originals.

No network, fitting, nomination, owner approval or order. A fresh cache receipt
is not a publication update, calibrated probability or resolver equivalence.
"""
import argparse
from copy import deepcopy
from decimal import Decimal
from hashlib import sha256
import json
from pathlib import Path
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from moloch.tartarus.party_scenarios import compile_scenarios, canonical, PPM
from moloch.tartarus.native_snapshot import _ns


def build(inputs, original_directory, field_observation, *, allow_dated=False, directional=False):
    raw = (inputs / "kalshi-cache-quotes.original.json").read_bytes()
    cache = json.loads(raw)
    pins = json.loads((inputs / "external-input-capture.json").read_text(encoding="utf-8"))
    assert sha256(raw).hexdigest() == pins["quoteSha256"]
    review_raw = (inputs / "active-fourteen-reviews.private.json").read_bytes()
    assert sha256(review_raw).hexdigest() == pins["reviewProjectionSha256"]
    cutoff = _ns(cache["at"]) // 1_000_000
    assert cutoff <= int(time.time() * 1000)
    if not allow_dated:
        assert int(time.time() * 1000) - cutoff <= 30000, "CACHE_EXPIRED_BEFORE_ISSUE"
    registry = json.loads((original_directory / "full-universe-expanded/census.private.json").read_text(encoding="utf-8"))["rows"]
    originals = {pins["quoteSha256"]: raw}
    groups = {}
    byex = {r["exchangeId"]: r for r in registry}
    for row in registry:
        body = (original_directory / "roots" / row["originalFile"]).read_bytes()
        assert sha256(body).hexdigest() == row["semanticSourceSha256"]
        originals[row["semanticSourceSha256"]] = body
        details = json.loads(body)["data"]["root"]["contract_details"]
        event = canonical({k: details[k] for k in ("raceId", "stageId", "electionDate", "raceName", "officeLevel", "usState")}).decode()
        groups.setdefault(event, {}).setdefault(details["winnerName"], []).append(row["exchangeId"])
    quotes = {m["ticker"]: m for m in cache["markets"]}
    points, ignorance = {}, []
    for row in registry:
        ex = row["exchangeId"]
        q = quotes.get(row["kalshiTicker"])
        valid = q is not None and row["marketId"] in q["sigMarketIds"]
        valid = valid and 0 <= cutoff - _ns(q["receivedAt"]) // 1_000_000 <= 30000
        valid = valid and q["normalized"]["yesBids"] and q["normalized"]["yesAsks"]
        if valid:
            bid = max(int(Decimal(x["price"]) * PPM) for x in q["normalized"]["yesBids"])
            ask = min(int(Decimal(x["price"]) * PPM) for x in q["normalized"]["yesAsks"])
            valid = 0 <= bid <= ask <= PPM
        if valid:
            lo, hi = max(0, bid - 1000), min(PPM, ask + 1000)
            observed = _ns(q["receivedAt"]) // 1_000_000
            label = "Conditional receipt-timed Kalshi price proxy with explicit unfitted1000ppm widening; publication time/resolver equivalence/calibration not established."
        else:
            lo, hi, observed = 0, PPM, cutoff
            label = "Explicit ignorance at the cache snapshot: no valid matching receipt-timed quote. This does not refresh an older quote."
            ignorance.append(ex)
        points[ex] = dict(exchangeId=ex, lowerPpm=lo, upperPpm=hi,
            sourceSha256=pins["quoteSha256"], observedAtMs=observed,
            maximumAgeMs=30000, assumptionLabel=label)
    conflicts = []
    for event, parties in groups.items():
        bounds = [(max(points[x]["lowerPpm"] for x in members),
                   min(points[x]["upperPpm"] for x in members)) for members in parties.values()]
        if any(lo > hi for lo, hi in bounds) or sum(lo for lo, hi in bounds) > PPM:
            conflicts.append(json.loads(event))
            for members in parties.values():
                for ex in members:
                    points[ex].update(lowerPpm=0, upperPpm=PPM, observedAtMs=cutoff,
                        assumptionLabel="Explicit categorical ignorance: cached conditional quote intervals conflict. Original quotes retained, never silently normalized outside bounds.")
    reviews = []
    for item in json.loads(review_raw)["reviews"]:
        body = item["bodyText"].encode()
        assert sha256(body).hexdigest() == item["sha256"]
        review = json.loads(body)
        assert review["reviewedAtMs"] <= cutoff < review["expiresAtMs"], "ORIGINAL_REVIEW_EXPIRED"
        if review["exchangeId"] in byex:
            reviews.append((review, item["sha256"]))
            originals[item["sha256"]] = body
    overrides = {}
    for event, parties in groups.items():
        claimed = {}
        for party, members in parties.items():
            selected = [(r, h) for r, h in reviews if r["exchangeId"] in members]
            if selected:
                values = [int(Decimal(r["ordinaryProbability"]) * PPM) if r["side"] == "YES"
                          else PPM - int(Decimal(r["ordinaryProbability"]) * PPM) for r, h in selected]
                claimed[party] = (min(values), max(values), selected[0])
        if not claimed:
            continue
        assert sum(c[0] for c in claimed.values()) <= PPM, "ACTIVE_REVIEW_CATEGORICAL_CONFLICT"
        for party, members in parties.items():
            if party in claimed:
                lo, hi, (origin, source) = claimed[party]
                label = "Original active owner conditional ordinary numerical input; same-party predicates share a declared point/envelope, not calibrated probability. Original exceptional mixture is not reconstructed."
            else:
                lo, hi = 0, PPM - sum(c[0] for c in claimed.values())
                origin, source = next(iter(claimed.values()))[2]
                label = "CTM-priority sensitivity: opposing-party ignorance bounded by reviewed ordinary exclusivity. Price-only hypotheses remain separate; not resolver equivalence or calibration."
            for ex in members:
                overrides[ex] = dict(exchangeId=ex, lowerPpm=lo, upperPpm=hi,
                    sourceSha256=source, observedAtMs=origin["reviewedAtMs"],
                    maximumAgeMs=origin["expiresAtMs"] - origin["reviewedAtMs"], assumptionLabel=label)
    profiles = []
    for name, point, dependence in [("price_lower_common", "lower", "common_party_swing"),
                                  ("price_upper_common", "upper", "common_party_swing"),
                                  ("price_midpoint_independent", "midpoint", "independent"),
                                  ("active_review_lower_common", "lower", "common_party_swing"),
                                  ("active_review_upper_common", "upper", "common_party_swing")]:
        active = name.startswith("active_review")
        selected_overrides = list(overrides.values()) if active else []
        if directional and dependence == "common_party_swing":
            selected = deepcopy(points)
            if active:
                selected.update(deepcopy(overrides))
            selected_overrides = []
            for parties in groups.values():
                bounds = {party: (max(selected[ex]["lowerPpm"] for ex in members),
                                  min(selected[ex]["upperPpm"] for ex in members))
                          for party, members in parties.items()}
                rlo, rhi = bounds.get("Republican Party", (0, 0))
                dlo, dhi = bounds.get("Democratic Party", (0, 0))
                if point == "lower":
                    rmass, dmass = rlo, min(dhi, PPM - rlo)
                else:
                    rmass, dmass = min(rhi, PPM - dlo), dlo
                masses = {"Republican Party": rmass, "Democratic Party": dmass}
                for party, members in parties.items():
                    assert bounds[party][0] <= masses[party] <= bounds[party][1]
                    for ex in members:
                        rec = deepcopy(selected[ex])
                        rec.update(lowerPpm=masses[party], upperPpm=masses[party])
                        rec["assumptionLabel"] += " Explicit coherent directional party-envelope sensitivity within original declared intervals; neither a fitted probability nor a silent quote normalization."
                        selected_overrides.append(rec)
        profiles.append(dict(id=name, marginalPoint=point, dependence=dependence,
            hypotheticalRefundWeightPpm=0 if active else 1000,
            marginalOverrides=selected_overrides,
            assumptionLabel="Uncalibrated terminal sensitivity from original receipt-timed price proxies and declared dependence; active-review ordinary points omit their full exceptional mixture."))
    model = compile_scenarios(registry, originals, list(points.values()), profiles, cutoff,
        draws_per_profile=64, seed=17023, research_scenario_limit=4096, research_contract_limit=512)
    pub = json.loads(field_observation.read_text(encoding="utf-8"))
    anchor = pub["derivedFlatMarkedWealthAnchor"]
    benchmark_at = _ns(pub["receivedAt"]) // 1_000_000
    assert benchmark_at <= cutoff, "PUBLIC_BENCHMARK_NOT_YET_AVAILABLE"
    n = len(model["scenarios"])
    alaska = [r for r in registry if r["raceName"] == "U.S. Senate Alaska" and r["winnerName"] == "Republican Party"]
    assert len(alaska) == 1
    cid = alaska[0]["contractId"]
    fields = [dict(id=f"public_flat_anchor_{benchmark_at}", thresholds=[anchor] * n,
        completeField=False, assumptionLabel="Incomplete flat terminal-threshold hypothesis from initial100000 plus dated official public marked PnL; not rival cash or terminal wealth."),
        dict(id="adverse_growing_field", thresholds=["1500000.000000"] * n,
            completeField=False, assumptionLabel="Hypothetical constant1.5m adverse field; not an observation."),
        dict(id="held_alaska_correlated_adverse", thresholds=["1500000.000000" if s["payouts"][cid] == "resolved_yes" else anchor for s in model["scenarios"]],
            completeField=False, assumptionLabel="Hypothetical field1.5m when held Alaska Republican Senate wins, otherwise dated public anchor; no opposing positions are known.")]
    issued = int(time.time() * 1000)
    current = issued - cutoff <= 30000 and all(
        0 <= issued - m["observedAtMs"] <= m["maximumAgeMs"] for m in points.values())
    current = current and all(issued < r["expiresAtMs"] for r, _ in reviews)
    if not allow_dated:
        assert current, "CACHE_EXPIRED_DURING_ISSUE"
    return dict(model=model, fields=fields, profiles=profiles, marginals=list(points.values()),
        publicBenchmarkObservation=pub, inputCutoffMs=cutoff, issuedAtMs=issued,
        currentInputsAtIssue=current, datedResearchOverride=allow_dated,
        directionalPartyEnvelopeVariant=directional,
        activeReviewCount=len(reviews), cacheMarketCount=len(cache["markets"]),
        quoteIgnoranceExchanges=ignorance, categoricalPriceConflicts=conflicts,
        quoteCacheRawSha256=pins["quoteSha256"], originalControlSha256=json.loads(review_raw)["controlSha256"],
        sourceCodeSha256=sha256(Path(__file__).read_bytes()).hexdigest(),
        originalClocksRenewed=False, executionAuthority=False,
        limitations="Numerical terminal hypotheses, not a fitted forecast or positive alpha certificate. Missing source publication times stay unknown. Independent rare events may be absent; exceptional legal outcomes and rival future actions are not exhaustive.")


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--input-directory", type=Path, required=True)
    p.add_argument("--original-directory", type=Path, required=True)
    p.add_argument("--field-observation", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--allow-dated-research", action="store_true")
    p.add_argument("--directional-party-envelope", action="store_true")
    a = p.parse_args()
    out = build(a.input_directory, a.original_directory, a.field_observation,
                allow_dated=a.allow_dated_research, directional=a.directional_party_envelope)
    a.output.write_text(json.dumps(out, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({k: out[k] for k in ("inputCutoffMs", "issuedAtMs", "currentInputsAtIssue", "activeReviewCount", "cacheMarketCount")}))
    print(json.dumps({"scenarios": len(out["model"]["scenarios"]), "ignorance": len(out["quoteIgnoranceExchanges"]), "categoricalConflicts": len(out["categoricalPriceConflicts"]), "executionAuthority": False}))
