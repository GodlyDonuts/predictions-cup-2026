"""Offline declared party-outcome scenarios for the native rank campaign.

Inputs are original official roots and explicit marginal intervals/assumptions.
An external quote is not a calibrated probability or a resolver certificate.
This finite sampled matrix grants no execution authority and makes no API call.
"""
import hashlib
import json
import random

PPM = 1_000_000
PARTIES = ("Republican Party", "Democratic Party")


def need(ok, reason):
    if not ok:
        raise ValueError(reason)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=False, allow_nan=False).encode()


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def integer(value, lo, hi, reason):
    need(type(value) is int and lo <= value <= hi, reason)
    return value


def _marginal_records(records, originals, decision_ms):
    need(type(records) is list and len(records) <= 64, "MARGINAL_BOUND")
    result = {}
    for m in records:
        need(type(m) is dict, "MARGINAL_SHAPE")
        ex = m.get("exchangeId")
        need(type(ex) is str and ex and ex not in result, "MARGINAL_DUPLICATE")
        lo = integer(m.get("lowerPpm"), 0, PPM, "MARGINAL_INTERVAL")
        hi = integer(m.get("upperPpm"), lo, PPM, "MARGINAL_INTERVAL")
        observed = integer(m.get("observedAtMs"), 0, decision_ms, "MARGINAL_FUTURE")
        age = integer(m.get("maximumAgeMs"), 1, 2**63-1, "MARGINAL_AGE")
        need(decision_ms-observed <= age, "MARGINAL_STALE")
        need(type(m.get("assumptionLabel")) is str and m["assumptionLabel"].strip(), "ASSUMPTION_REQUIRED")
        raw = originals.get(m.get("sourceSha256"))
        need(type(raw) is bytes and sha(raw) == m["sourceSha256"], "MARGINAL_ORIGINAL_REQUIRED")
        result[ex] = (lo, hi, m)
    return result


def compile_scenarios(registry, originals, marginals, profiles, decision_ms,
                      draws_per_profile=16, seed=17023):
    """Return native scenarios/probabilityModels, never a nomination/approval.

    Each marginal names exchangeId, lowerPpm, upperPpm, sourceSha256,
    observedAtMs, maximumAgeMs and assumptionLabel. [0,1000000] is allowed
    only as an explicitly supplied ignorance interval, never an inferred zero.
    A profile may supply marginalOverrides using the same complete source/clock
    records for a subset of existing exchanges. Overrides affect that profile
    only: conflicting publishers remain separate models, never an average.
    Profiles explicitly select lower|midpoint|upper, independent|common_party_swing,
    and a hypothetical all-refund stress weight. Same-race intervals must admit
    a coherent categorical distribution. Inconsistent quotes do not get silently
    normalized beyond their supplied intervals.
    """
    integer(decision_ms, 0, 2**63-1, "DECISION_CLOCK")
    integer(seed, 0, 2**63-1, "SEED")
    integer(draws_per_profile, 2, 64, "DRAW_BOUND")
    need(type(registry) is list and 1 <= len(registry) <= 64, "REGISTRY_BOUND")
    need(type(profiles) is list and 1 <= len(profiles) <= 8, "NATIVE_MODEL_BOUND")
    need(type(originals) is dict and type(marginals) is list, "INPUT_SHAPE")
    by_exchange = _marginal_records(marginals, originals, decision_ms)
    groups, contracts, exchanges = {}, set(), set()
    for c in registry:
        cid, ex = c.get("contractId"), c.get("exchangeId")
        need(type(cid) is str and cid and cid not in contracts and type(ex) is str
             and ex and ex not in exchanges, "REGISTRY_DUPLICATE")
        contracts.add(cid); exchanges.add(ex)
        raw = originals.get(c.get("semanticSourceSha256"))
        need(type(raw) is bytes and sha(raw) == c["semanticSourceSha256"], "SEMANTIC_ORIGINAL_REQUIRED")
        doc = json.loads(raw)
        data = doc.get("data", doc)
        root = data.get("root", {})
        need(data.get("market_id") == c.get("marketId")
             and root.get("node_type") == "contract"
             and root.get("node_id") == cid and root.get("contract_id") == cid
             and sha(canonical(root)) == c.get("officialContractRootSha256"), "OFFICIAL_ROOT_CHANGED")
        d = root.get("contract_details", {})
        need(d.get("resolutionType") == "Party Winner" and d.get("raceStage") == "General"
             and d.get("winnerName") in PARTIES, "PARTY_PREDICATE_UNSUPPORTED")
        fields = ("raceId", "stageId", "electionDate", "raceName", "officeLevel", "usState")
        need(all(d.get(k) is not None and str(d[k]) for k in fields), "EVENT_IDENTITY_REQUIRED")
        event = canonical({k:d[k] for k in fields}).decode()
        need(ex in by_exchange, "MARGINAL_MISSING")
        groups.setdefault(event, {}).setdefault(d["winnerName"], []).append((cid, ex))
    need(set(by_exchange) == exchanges, "MARGINAL_REGISTRY_SCOPE")
    scenarios, models, diagnostics, profile_ids = [], [], [], set()
    for profile in profiles:
        ident = profile.get("id")
        need(type(ident) is str and ident and ident not in profile_ids, "PROFILE_ID")
        profile_ids.add(ident)
        point, dependence = profile.get("marginalPoint"), profile.get("dependence")
        need(point in ("lower", "midpoint", "upper")
             and dependence in ("independent", "common_party_swing"), "PROFILE_ASSUMPTION")
        refund = integer(profile.get("hypotheticalRefundWeightPpm"), 0, PPM, "REFUND_ASSUMPTION_REQUIRED")
        need(type(profile.get("assumptionLabel")) is str and profile["assumptionLabel"].strip(), "ASSUMPTION_REQUIRED")
        overrides = _marginal_records(profile.get("marginalOverrides", []), originals, decision_ms)
        need(set(overrides) <= exchanges, "PROFILE_OVERRIDE_SCOPE")
        selected_marginals = dict(by_exchange)
        selected_marginals.update(overrides)
        event_specs, cuts = [], {0, PPM}
        for event, parties in sorted(groups.items()):
            intervals, mass = {}, {}
            for party, members in parties.items():
                lo = max(selected_marginals[ex][0] for _, ex in members)
                hi = min(selected_marginals[ex][1] for _, ex in members)
                need(lo <= hi, "SAME_PARTY_INTERVAL_CONFLICT")
                intervals[party] = (lo, hi)
                mass[party] = lo if point == "lower" else hi if point == "upper" else (lo+hi)//2
            need(sum(lo for lo, _ in intervals.values()) <= PPM, "INCOHERENT_PARTY_MARGINALS")
            # Move an overfull declared point only within the supplied bounds.
            excess = max(0, sum(mass.values())-PPM)
            for party in sorted(mass):
                take = min(excess, mass[party]-intervals[party][0])
                mass[party] -= take; excess -= take
            need(excess == 0, "INCOHERENT_PARTY_MARGINALS")
            event_specs.append((event, parties, mass))
            cuts.update([mass.get(PARTIES[0], 0), PPM-mass.get(PARTIES[1], 0)])
            diagnostics.append({"profileId":ident, "eventIdentity":json.loads(event),
                "declaredMassPpm":mass, "otherWinnerMassPpm":PPM-sum(mass.values()),
                "jointDistributionIsAssumption":True})
        if dependence == "common_party_swing":
            boundaries = sorted(cuts)
            cells = [(a+b)//2 for a, b in zip(boundaries, boundaries[1:])]
            masses = [b-a for a, b in zip(boundaries, boundaries[1:])]
        else:
            cells = [((2*i+1)*PPM)//(2*draws_per_profile) for i in range(draws_per_profile)]
            share, remainder = divmod(PPM, draws_per_profile)
            masses = [share+(i < remainder) for i in range(draws_per_profile)]
        need(len(scenarios)+len(cells)+1 <= 128, "NATIVE_SCENARIO_BOUND")
        payouts = [{} for _ in cells]
        for event, parties, mass in event_specs:
            permutation = list(range(len(cells)))
            if dependence == "independent":
                rng = random.Random(int(sha(canonical([seed, ident, event])), 16))
                rng.shuffle(permutation)
            for index, cell in enumerate(permutation):
                u = cells[cell]
                winner = (PARTIES[0] if u < mass.get(PARTIES[0], 0) else
                          PARTIES[1] if u >= PPM-mass.get(PARTIES[1], 0) else None)
                for party, members in parties.items():
                    for cid, _ in members:
                        payouts[index][cid] = "resolved_yes" if party == winner else "resolved_no"
        offset = len(scenarios)
        for index, row in enumerate(payouts):
            need(set(row) == contracts, "INCOMPLETE_WHOLE_ACCOUNT_SCENARIO")
            scenarios.append({"id":f"{ident}:{index}", "kind":"ordinary",
                "assumptionLabel":profile["assumptionLabel"], "payouts":row})
        ordinary = PPM-refund
        weights = [m*ordinary//PPM for m in masses]
        left = ordinary-sum(weights)
        order = sorted(range(len(masses)), key=lambda i:(-(masses[i]*ordinary%PPM), i))
        for i in order[:left]: weights[i] += 1
        models.append({"id":ident, "assumptionLabel":profile["assumptionLabel"],
                       "offset":offset, "ordinaryWeights":weights, "refund":refund})
    scenarios.append({"id":"hypothetical_all_refund", "kind":"exceptional",
        "assumptionLabel":"Hypothetical native principal-refund stress; not an official resolver assertion or exhaustive exception model",
        "payouts":{cid:"refund_principal" for cid in sorted(contracts)}})
    for model in models:
        weights = [0]*len(scenarios)
        for i, weight in enumerate(model.pop("ordinaryWeights")):
            weights[model["offset"]+i] = weight
        weights[-1] = model.pop("refund")
        model.pop("offset")
        model["weightsPpm"] = weights
    return {"schemaVersion":1, "scenarios":scenarios, "probabilityModels":models,
        "diagnostics":diagnostics, "drawsPerProfile":draws_per_profile, "seed":seed,
        "registrySha256":sha(canonical(registry)), "marginalInputSha256":sha(canonical(marginals)),
        "profileInputSha256":sha(canonical(profiles)), "decisionAtMs":decision_ms,
        "resolutionEquivalenceVerified":False, "calibratedProbabilitiesClaimed":False,
        "finiteSamplesExhaustive":False, "independentRareEventsMayBeUnrepresented":True,
        "commonSwingIntervalsExactWithinDeclaredCopula":True, "physicalPreflightRequired":True,
        "executionAuthority":False, "networkRequests":0}
