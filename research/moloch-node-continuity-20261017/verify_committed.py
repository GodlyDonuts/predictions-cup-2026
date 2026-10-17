"""Pinned offline sixth-generation proof; never contacts or changes a worker."""
import argparse
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
from decimal import Decimal

# Reuse only this known, reviewed repository implementation, never packet code.
HELPER = Path(__file__).resolve().parents[1] / "moloch-oregon-node-refresh-20261017/verify_committed.py"
assert hashlib.sha256(HELPER.read_bytes().replace(b"\r\n", b"\n")).hexdigest() == "c47cd592dd32c445f51e15aae45bd99f4927749aa042a25ba77ef6dffb9bdb8b"
spec = importlib.util.spec_from_file_location("_retained_or_proof_helpers", HELPER)
helpers = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helpers)
need, sha, load, sealed, chain, clock, packet, receipt_bytes = [getattr(helpers, n) for n in
    ("need", "sha", "load", "sealed", "chain", "clock", "packet", "receipt_bytes")]

CANDIDATE = "b34e2a40aaacd606feb824bcf8a0de9a3dd6a79b45c8d10b1af3e544a7ba8785"
LIVE = "19e964fb2168bee447600a23e14cdb83a1dd5ead24be04a0c43cd0f824ba0774"
PREVIOUS = "8c09fd1aaa589dd0e4e55da09188b5b5aa7f19c9ea3378c779155da534dc02d3"
PREVIOUS_GENERATION = "64b373ce0b63e7e59a694da7175b49788b59320c68e7c12248c3fc38bffedbda"
GENERATION = "813f85a7a6da473c52152d7acbdd0b88102f288380c95199bd5d23360b0ff12a"
BASE = "916e8ca33b837a1846db27458adf8daac29f3910746bc2bf9da773aef094488f"
TID = "bda92870-621e-47b0-bc3c-3602c5c26f55"
REVIEW_HASHES = {"865": "2f3acf006fdba3149342bd031b92459d5552fa048373a18bb389556d22c1227d",
                 "1067": "5de3e94596b6e93f8a39caeb7da529c7694b5a897e117058d5e56aa25b183c2a"}


def candidate_checks(c, old_reviews, new_reviews):
    need(set(old_reviews) == set(new_reviews) and len(new_reviews) == 11, "ELEVEN_SCOPES")
    need({ex for ex in old_reviews if old_reviews[ex] != new_reviews[ex]} == set(REVIEW_HASHES), "ONLY_TWO_REVIEW_CHANGES")
    base, overlay, settings = [load(c[n]) for n in ("base-control.original.json", "next-control.inactive.json", "settings.json")]
    need(sha(c["base-control.original.json"]) == BASE and overlay["fundamentalPolicy"] == settings, "CANDIDATE_BASE_SETTINGS")
    financial = copy.deepcopy(overlay); financial["fundamentalPolicy"] = base["fundamentalPolicy"]
    need(financial == base and settings["allowInitialization"] is False, "NO_FINANCIAL_CONTROL_DELTA")
    old_auth, auth = [load(c[n]) for n in ("old-authority.original.json", "authority.json")]
    preserved_auth = copy.deepcopy(auth); preserved_auth["trustedReviewSha256"] = old_auth["trustedReviewSha256"]
    need(len(old_auth["trustedReviewSha256"]) == 27 and len(auth["trustedReviewSha256"]) == 29
         and preserved_auth == old_auth
         and auth["trustedReviewSha256"][:27] == old_auth["trustedReviewSha256"]
         and set(auth["trustedReviewSha256"][27:]) == set(REVIEW_HASHES.values())
         and settings["authoritySha256"] == sha(c["authority.json"]), "ALL_AUTHORITY_HISTORY")
    typed, loaded = [load(c[n]) for n in ("typed-review.stdout.json", "typed-settings.stdout.json")]
    need(typed["typedCurrentReviewChecksPassed"] is True and loaded["settingsLoadPassed"] is True
         and loaded["sourcesVerified"] is True and typed["executionAuthority"] is loaded["executionAuthority"] is False,
         "RETAINED_TYPED_VALIDATOR_RESULT")
    changed_refs = []
    for old_ref, new_ref in zip(base["fundamentalPolicy"]["reviews"], settings["reviews"]):
        if old_ref == new_ref:
            continue
        matches = [ex for ex in REVIEW_HASHES if sha(c[ex + "-node-review.json"]) == new_ref["fileSha256"]]
        need(len(matches) == 1, "EXACT_CHANGED_SETTINGS_REFERENCE"); ex = matches[0]; changed_refs.append(ex)
        old = old_reviews[ex]["review"]; new = load(c[ex + "-node-review.json"])
        need(new_reviews[ex] == {"review": new, "sha256": REVIEW_HASHES[ex]}
             and load(c["original-preparation/" + ex + "/active-review.canonical-original.json"]) == old,
             "EXACT_ACTIVE_REVIEW_BINDING")
        restored = copy.deepcopy(new)
        for key in ("reviewedAtMs", "reviewerId", "assumptionLabel", "expiresAtMs"):
            restored[key] = old[key]
        prior = next(s for s in old["sources"] if s["kind"] == "official_contract_node")
        node = next(s for s in restored["sources"] if s["kind"] == "official_contract_node")
        node["sha256"], node["observedAtMs"] = prior["sha256"], prior["observedAtMs"]
        restored["valuationObjective"]["timingSourceSha256"] = old["valuationObjective"]["timingSourceSha256"]
        need(restored == old and new["expiresAtMs"] == min(s["observedAtMs"] + s["maximumAgeMs"] for s in new["sources"]),
             "NODE_METADATA_ONLY_NO_FORECAST_RENEWAL")
        raw = c[ex + "-node.original.json"]; original = c["original-preparation/" + ex + "/old-node.original.json"]
        new_node = next(s for s in new["sources"] if s["kind"] == "official_contract_node")
        need(sha(original) == prior["sha256"] and sha(raw) == new_node["sha256"]
             and load(raw)["data"] == load(original)["data"] and load(raw)["status"] == 200
             and load(raw)["endpoint"] == load(original)["endpoint"]
             and clock(load(raw)["receivedAt"]) == new_node["observedAtMs"] > prior["observedAtMs"], "ORIGINAL_NODE_CONTINUITY")
        need(REVIEW_HASHES[ex] == next(r["canonicalSha256"] for r in typed["reviews"] if r["key"] == ex + "-node-only"), "TYPED_CANONICAL_REVIEW_HASH")
        for ref in new_ref["sourceFiles"]:
            member = ex + "-node.original.json" if ref["sha256"] == new_node["sha256"] else "original-sources/" + ref["sha256"]
            need(sha(c[member]) == ref["sha256"], "EVERY_CHANGED_REVIEW_ORIGINAL_SOURCE_BYTES")
    need(set(changed_refs) == set(REVIEW_HASHES) and len(settings["reviews"]) == len(base["fundamentalPolicy"]["reviews"]) == 11,
         "NINE_SETTINGS_REFERENCES_UNCHANGED")


def original_facts(f, event, at_ms):
    endpoints = ["/tournaments/midterm-elections", "/tournaments/midterm-elections/portfolio/positions",
                 f"/orders?status=open&limit=200&tournamentId={TID}", f"/account?tournamentId={TID}"]
    facts = {load(raw)["endpoint"]: (name, raw, load(raw)) for name, raw in f.items() if name.startswith("facts/")}
    need(len([n for n in f if n.startswith("facts/")]) == 4 and set(facts) == set(endpoints), "EXACT_FOUR_ACCOUNT_FACTS")
    encoded, ages, last = [], [], 0
    for endpoint in endpoints:
        name, raw, receipt = facts[endpoint]; wire = receipt_bytes(raw)
        start, end = clock(receipt["requestStartedAt"]), clock(receipt["receivedAt"])
        need(Path(name).stem == sha(wire) and receipt["status"] == 200 and last <= start <= end <= at_ms
             and 0 <= at_ms - start <= 20000, "ORIGINAL_TYPED_FACT_HASH_CLOCK")
        last = end; ages.append(at_ms - start); encoded.append(wire)
    digest = sha(b"[" + b",".join(encoded) + b"]")
    need(digest == event["factsSha256"], "EXACT_RAW_TOKEN_FACTS_SHA")
    return [facts[e][2]["data"] for e in endpoints], digest, ages


def verify(live_path, live_sha, candidate_path, previous_path):
    need(live_sha == LIVE, "EXPLICIT_OWNER_PINNED_LIVE_SHA")
    f = packet(live_path, live_sha); c = packet(candidate_path, CANDIDATE)
    previous = packet(previous_path, PREVIOUS, False)["fundamental.original.jsonl"]
    need(f["fundamental.original.jsonl"].startswith(previous), "ORIGINAL_FIVE_GENERATION_BYTES")
    for name in c.keys() & f.keys():
        need(c[name] == f[name], "CANDIDATE_MEMBER_CHANGED_" + name)
    old_rows, rows = chain(previous), chain(f["fundamental.original.jsonl"])
    old_generations = [r for r in old_rows if r["event"]["kind"] == "review_generation"]
    generations = [r for r in rows if r["event"]["kind"] == "review_generation"]
    need(len(old_generations) == 5 and len(generations) == 6 and generations[:5] == old_generations,
         "SIX_GENERATIONS_ORIGINAL_PREFIX")
    g = generations[-1]; event = g["event"]; meta = event["generation"]
    request = load(f["request-001.json"])
    need(sha(f["request-001.json"]) == GENERATION == meta["generationSha256"]
         and meta["requestBodyText"].encode() == f["request-001.json"] and meta["request"] == request
         and request["previousGenerationSha256"] == PREVIOUS_GENERATION
         and request["issuedAtMs"] <= g["atMs"] < request["expiresAtMs"]
         and 0 < request["expiresAtMs"] - request["issuedAtMs"] <= 60000
         and request["workerPid"] == 654706 and request["baseControlSha256"] == BASE
         and request["controlSha256"] == sha(c["next-control.inactive.json"]), "ORIGINAL_REQUEST_SCOPE_CLOCK")
    old_reviews = {r["review"]["exchangeId"]: r for r in old_generations[-1]["event"]["reviews"]}
    new_reviews = {r["review"]["exchangeId"]: r for r in event["reviews"]}
    candidate_checks(c, old_reviews, new_reviews)
    response = load(f["reload-001.stdout.json"])
    approval = load(f["owner-approval.json"])
    need(approval["role"] == "human_authorized_execution_owner"
         and approval["candidatePacketSha256"] == CANDIDATE
         and approval["nextControlSha256"] == request["controlSha256"]
         and set(approval["newCanonicalReviewHashes"]) == set(REVIEW_HASHES.values())
         and approval["decision"] == "APPROVE_GENUINELY_LATER_UNCHANGED_NODE_REFERENCES_ONLY"
         and clock(approval["reviewedAt"]) <= g["atMs"]
         and response["state"] == "committed" and response["generationSha256"] == GENERATION
         and response["baseFinancialControlUnchanged"] is True and f["reload-001.stderr.txt"] == b"", "OWNER_APPROVAL_AND_CLI")
    anchor, head, report = [sealed(f[n]) for n in ("stopped-head.original.json", "head.original.json", "report.original.json")]
    native = chain(f["native.suffix.ndjson"], anchor)
    need(anchor["sequence"] == 489338 and native[-1]["sequence"] == head["sequence"] == report["sequence"]
         and native[-1]["hash"] == head["previous"] == report["previous"], "NATIVE_SUFFIX_BOUNDARY")
    captured = event["nativeHead"]
    capture_row = next(r for r in native if r["sequence"] == captured["sequence"])
    mode = next(r for r in native if r["sequence"] == 490342)
    need(captured["previous"] == capture_row["hash"] and mode["event"]["type"] == "managed_inventory_planning_v1"
         and mode["event"]["generationSha256"] == GENERATION and mode["event"]["bankHead"] == captured
         and mode["event"]["controlSha256"] == BASE and response["nativeHead"]["sequence"] == mode["sequence"]
         and response["nativeHead"]["previous"] == mode["hash"], "ACTUAL_NATIVE_GENERATION_COMMIT")
    safe = {"book", "trade", "coverage", "gap", "tick", "unsubmitted_intent_retired", "managed_inventory_planning_v1"}
    need(all(r["event"]["type"] in safe for r in native if r["sequence"] > captured["sequence"]),
         "LATER_REPORT_HAS_NO_INTERVENING_FINANCIAL_TRANSITION")
    (tournament, positions, orders, account), facts_sha, ages = original_facts(f, event, g["atMs"])
    need(tournament["id"] == TID and tournament["slug"] == "midterm-elections" and tournament["status"] == "active"
         and "pagination" not in positions and orders["coverage"]["complete"] is True
         and orders["pagination"]["hasMore"] is False and orders["pagination"]["nextCursor"] is None,
         "COMPLETE_ACCOUNT_FACT_COVERAGE")
    need(account["balance"] == Decimal(report["report"]["cash"]), "ORIGINAL_ACCOUNT_CASH_RECONCILIATION")
    scopes = {}
    for ex in REVIEW_HASHES:
        review = new_reviews[ex]["review"]; side = review["side"]; other = "NO" if side == "YES" else "YES"
        lots = report["report"]["positions"].get(ex, {"YES": [], "NO": []})
        quantity = sum(l["quantity"] for l in lots[side]); basis = sum((Decimal(l["price"]) * l["quantity"] for l in lots[side]), Decimal(0))
        actual = [p for p in positions["positions"] if p["exchangeId"] == ex]
        signed = quantity if side == "YES" else -quantity
        need(not lots[other] and ((quantity == 0 and not actual) or (len(actual) == 1 and actual[0]["quantity"] == signed))
             and quantity <= review["maximumQuantity"] and basis <= Decimal(review["maximumPrincipal"])
             and not any(o["exchangeId"] == ex for o in orders["data"]), "SCOPE_FIFO_CAP_ORIGINAL_FACTS_" + ex)
        # Match genuine remaining lot price/quantity/time membership. Do not use
        # rounded account costBasis or pretend the provider's tied-lot order
        # supersedes the native journal's original conservative FIFO convention.
        official_groups, native_groups = {}, {}
        for lot in actual[0]["lots"] if actual else []:
            need(lot["side"] == side and lot["quantity"] > 0, "OFFICIAL_SCOPE_LOT_SIDE_QUANTITY")
            key = (clock(lot["openedAt"]), Decimal(lot["entryPrice"]))
            official_groups[key] = official_groups.get(key, 0) + lot["quantity"]
        for lot in lots[side]:
            key = (clock(lot["executedAt"]), Decimal(lot["price"]))
            native_groups[key] = native_groups.get(key, 0) + lot["quantity"]
        need(official_groups == native_groups, "ORIGINAL_REMAINING_LOT_MEMBERSHIP_" + ex)
        scopes[ex] = {"side": side, "quantity": quantity, "remainingFifoPrincipal": str(basis),
                      "maximumPrincipal": review["maximumPrincipal"], "reviewExpiresAtMs": review["expiresAtMs"],
                      "originalLotPriceQuantityMillisecondGroupsMatch": True}
    chain(f["dispatch.original.ndjson"])
    dispatch = event["dispatch"]; drows = chain(f["dispatch.original.ndjson"][:dispatch["bytes"]])
    need(drows[-1]["sequence"] == dispatch["sequence"] and drows[-1]["hash"] == dispatch["previous"], "ORIGINAL_DISPATCH_CAPTURE")
    attempts, acks, closed, aborted, cancel_attempts, cancel_acks = [set() for _ in range(6)]
    sets = {"attempt": attempts, "ack": acks, "recovered_ack": acks, "closed": closed,
            "aborted_before_admission": aborted, "cancel_attempt": cancel_attempts, "cancel_ack": cancel_acks}
    for row in drows:
        e = row["event"]
        if e["kind"] in sets:
            sets[e["kind"]].add(e["qid"])
    need(not (attempts - acks - aborted) and not (cancel_attempts - cancel_acks - closed), "NO_UNRESOLVED_RECORDED_PHYSICAL")
    target_qids = {q["id"] for q in report["orders"] if q["exchangeId"] in REVIEW_HASHES}
    need(not ((attempts | acks) & target_qids - closed - aborted), "BOTH_TARGETS_NO_PENDING_PHYSICAL")
    status, progress, summary = [load(f[n]) for n in ("model-status.original.json", "progress.original.json", "activation-summary.json")]
    need(status["workerPid"] == progress["pid"] == request["workerPid"] == 654706
         and status["workerStartTicks"] == request["workerStartTicks"] and status["ownerLease"] == request["ownerLease"]
         and status["baseControlSha256"] == BASE and status["previousGenerationSha256"] == GENERATION
         and progress["phase"] == "trading" and progress["realtimeHealthy"] is True
         and summary["controlIdentityObserved"] == meta["controlIdentity"]
         and summary["first300sAlreadyClosedSha256"] == approval["clockWindowAlreadyClosedSha256"]
         and summary["cliExitCodeIndependentlyCaptured"] is False and summary["accountCallsByCapture"] == 0
         and 0 <= clock(summary["at"]) - clock(progress["at"]) <= 120000, "CAPTURED_CURRENT_WORKER_CHANNEL")
    need(sha(f["bridge.original.json"]) == "b54830b219fba35238792927fc0c123c582a91b4311cb86a3b100f305f4c32e8"
         and len(load(f["bridge.original.json"])["upgrades"]) == 15, "ORIGINAL_FIFTEEN_BRIDGES")
    return {"result": "PASS_PINNED_SIXTH_GENERATION_COMMITTED_PROOF", "privateLivePacketSha256": live_sha,
            "candidatePacketSha256": CANDIDATE, "previousIncidentPacketSha256": PREVIOUS,
            "originalLiveMembersVerified": len(f), "originalFiveGenerationPrefixByteIdentical": True,
            "generationCount": 6, "generationSha256": GENERATION, "baseControlSha256": BASE,
            "changedReviewCanonicalHashesFromRetainedTypedValidator": REVIEW_HASHES, "unchangedOtherReviews": 9,
            "all27PreviousTrustedHashesRetained": True, "trustedHashCount": 29,
            "forecastBallotClocksEconomicsUnchanged": True, "nativeSuffixFirstSequence": native[0]["sequence"],
            "nativeSuffixLastSequence": native[-1]["sequence"], "generationCapturedNativeSequence": captured["sequence"],
            "committedCliNativeSequence": mode["sequence"], "fourOriginalReceiptTokenFactsSha256": facts_sha,
            "factRequestAgesAtCommitMs": ages, "scopeReconciliation": scopes,
            "noTargetOpenOrUnresolvedRecordedPhysicalAtGeneration": True, "capturedPid": 654706,
            "capturedProgressAt": progress["at"], "captureObservedAt": summary["at"], "notCurrentHealthClaim": True,
            "committedCliResponseAndNativeBoundaryVerified": True, "cliProcessExitIndependentlyCaptured": False,
            "financialSessionReplayPerformed": False, "sdkNoNetworkArchivesIndependentlyReverified": False,
            "rawProviderAuthenticationIndependentlyReperformed": False, "newAccountRequests": 0,
            "workerRestartRequiredForThisGeneration": False, "executionAuthority": False, "profitabilityEstablished": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("live-packet", "candidate-packet", "previous-packet"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--live-sha256", required=True, help="Owner's independently frozen exact private ZIP SHA256")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = verify(args.live_packet, args.live_sha256, args.candidate_packet, args.previous_packet)
    raw = (json.dumps(result, indent=2) + "\n").encode()
    if args.output:
        with args.output.open("xb") as target:
            target.write(raw)
    print(raw.decode(), end="")


if __name__ == "__main__":
    main()
