"""Offline, pinned OR generation proof. Never contacts or changes a live worker."""
import argparse
import copy
import datetime as dt
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import re
import zipfile

LIVE = "d652183c9cff9bb9efe5c0593e9155f5586ff9b6d97517d9b8e50ff137ae4b4d"
CANDIDATE = "5c4c6aa952ab6c7d10b2c78cddc5f1377485f051c577fb4a093ad5c3aab1db08"
PREVIOUS = "ddf08bca7c2f15d20875983ae0ffb414cddc06de38a04d94d97b050555bc48ee"
OLD_GENERATION = "b27c469978fbe3149dcbaa74a42a1d8b79867fe57be40ca50413d572eae84e17"
GENERATION = "64b373ce0b63e7e59a694da7175b49788b59320c68e7c12248c3fc38bffedbda"
REVIEW = "73147d66718c43d1747a0fc50b596cfffce5f8a5f08a57c531fdc7857c1732b7"
BASE = "a98ca3a0ac16ad7e7c0efa6af8e8f31da9deee35a59539e50bb519a25eac0fe4"
NODE = "e6d1b884d628881ffda0fdf86a8ac3ec3b8c974ec60ba5635c4641a7deff664f"
TID = "bda92870-621e-47b0-bc3c-3602c5c26f55"
FACTS = "b59d0ec6ede135a5ce550f9028cc3198da6a63b5e7beade1fb98426d43f321ec"
DECODER = json.JSONDecoder(parse_float=Decimal)


def need(ok, reason):
    if not ok:
        raise ValueError(reason)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def load(raw):
    return json.loads(raw, parse_float=Decimal)


def compact_tokens(text):
    """Remove structural whitespace only; preserve every string/number token."""
    out, quoted, escaped = [], False, False
    for char in text:
        if quoted or not char.isspace():
            out.append(char)
        if quoted:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                quoted = False
        elif char == '"':
            quoted = True
    need(not quoted and not escaped, "JSON_TOKEN_STRING")
    return "".join(out)


def receipt_bytes(raw):
    # Decode only to locate complete JSON value tokens. In particular data
    # amounts are NEVER converted to Python floats and re-encoded.
    text = raw.decode(); i = 0; values = {}
    def skip(n):
        while n < len(text) and text[n].isspace():
            n += 1
        return n
    i = skip(i); need(text[i] == "{", "RECEIPT_OBJECT"); i += 1
    while True:
        i = skip(i)
        if text[i] == "}":
            i += 1; break
        key, end = DECODER.raw_decode(text, i); need(isinstance(key, str) and key not in values, "RECEIPT_KEY")
        i = skip(end); need(text[i] == ":", "RECEIPT_COLON"); i = skip(i + 1)
        _, end = DECODER.raw_decode(text, i)
        values[key] = compact_tokens(text[i:end]); i = skip(end)
        if text[i] == ",":
            i += 1
        else:
            need(text[i] == "}", "RECEIPT_DELIMITER"); i += 1; break
    need(skip(i) == len(text), "RECEIPT_TRAILING")
    fields = ("endpoint", "requestStartedAt", "receivedAt", "status", "data")
    need(set(values) == set(fields), "RECEIPT_FIELDS")
    return ("{" + ",".join(json.dumps(k) + ":" + values[k] for k in fields) + "}").encode()


def sealed(raw):
    value = load(raw)
    unsigned, count = re.subn(rb'"hash":"' + value["hash"].encode() + rb'",', b"", raw.strip(), count=1)
    need(count == 1 and sha(unsigned) == value["hash"], "SEALED_RAW_HASH")
    return value


def chain(raw, anchor=None):
    need(raw.endswith(b"\n"), "CHAIN_TORN")
    previous = anchor["previous"] if anchor else "0" * 64
    sequence = anchor["sequence"] if anchor else 0
    rows = []
    for line in raw.splitlines():
        row = sealed(line)
        need(row["previous"] == previous and row["sequence"] == sequence + 1, "CHAIN_LINK")
        previous, sequence = row["hash"], row["sequence"]; rows.append(row)
    return rows


def clock(stamp):
    return int(dt.datetime.fromisoformat(stamp.replace("Z", "+00:00")).timestamp() * 1000)


def packet(path, expected, all_files=True):
    need(path.is_file() and not path.is_symlink() and path.stat().st_size <= 16 * 1024 * 1024, "PACKET_BOUND")
    need(sha(path.read_bytes()) == expected, "PACKET_HASH")
    with zipfile.ZipFile(path) as archive:
        need(len(archive.namelist()) == len(set(archive.namelist())), "DUPLICATE_MEMBER")
        pins = load(archive.read("packet-manifest.json"))
        pins = pins.get("files", pins)
        need(set(archive.namelist()) == set(pins) | {"packet-manifest.json"}, "PACKET_MEMBERS")
        names = list(pins) if all_files else ["fundamental.original.jsonl"]
        need(sum(archive.getinfo(n).file_size for n in names) <= 64 * 1024 * 1024, "PACKET_TOTAL_BOUND")
        result = {}
        for name in names:
            need(archive.getinfo(name).file_size <= 32 * 1024 * 1024, "MEMBER_BOUND")
            raw = archive.read(name)
            need(len(raw) == pins[name]["bytes"] and sha(raw) == pins[name]["sha256"], "MEMBER_PIN")
            result[name] = raw
        return result


def verify(live_path, candidate_path, previous_path):
    f = packet(live_path, LIVE); c = packet(candidate_path, CANDIDATE)
    old_sidecar = packet(previous_path, PREVIOUS, False)["fundamental.original.jsonl"]
    need(len(f) == 36 and f["fundamental.original.jsonl"].startswith(old_sidecar), "ORIGINAL_SIDECAR_PREFIX")
    for name in c.keys() & f.keys():
        need(c[name] == f[name], "CANDIDATE_MEMBER_CHANGED_" + name)
    old_rows = chain(old_sidecar); rows = chain(f["fundamental.original.jsonl"])
    old_gens = [r for r in old_rows if r["event"]["kind"] == "review_generation"]
    gens = [r for r in rows if r["event"]["kind"] == "review_generation"]
    need(len(old_gens) == 4 and len(gens) == 5 and gens[:4] == old_gens, "ORIGINAL_GENERATIONS_CHANGED")
    g = gens[-1]; event = g["event"]; meta = event["generation"]; request = load(f["request-001.json"])
    need(meta["requestBodyText"].encode() == f["request-001.json"] and sha(f["request-001.json"]) == GENERATION
         and meta["generationSha256"] == GENERATION and meta["request"] == request
         and request["previousGenerationSha256"] == OLD_GENERATION, "GENERATION_REQUEST")
    need(request["issuedAtMs"] <= g["atMs"] < request["expiresAtMs"]
         and request["expiresAtMs"] - request["issuedAtMs"] <= 60000
         and request["workerPid"] == 651324 and request["baseControlSha256"] == BASE
         and sha(c["base-control.original.json"]) == BASE
         and request["controlSha256"] == sha(f["next-control.inactive.json"]), "REQUEST_OWNER_SCOPE_CLOCK")
    approval = load(f["owner-approval.json"]); observed = load(f["reload-001-observed-result.json"])
    need(approval["role"] == "human_authorized_execution_owner" and approval["task"] == 168
         and approval["candidatePrivatePacketSha256"] == CANDIDATE
         and approval["nextControlSha256"] == request["controlSha256"]
         and observed["exitCode"] == 0 and observed["requestSha256"] == GENERATION
         and observed["ownerApprovalSha256"] == sha(f["owner-approval.json"])
         and f["reload-001.stderr.txt"] == b"", "OWNER_CLI_RESULT")
    old_reviews = {r["review"]["exchangeId"]: r for r in old_gens[-1]["event"]["reviews"]}
    new_reviews = {r["review"]["exchangeId"]: r for r in event["reviews"]}
    need(set(old_reviews) == set(new_reviews) and len(new_reviews) == 11, "REVIEW_SCOPE_COUNT")
    need([ex for ex in old_reviews if old_reviews[ex] != new_reviews[ex]] == ["977"], "OTHER_REVIEWS_CHANGED")
    old, new = load(f["old-review.original.json"]), load(f["or-node-review.json"])
    need(new_reviews["977"] == {"review": new, "sha256": REVIEW}
         and old_reviews["977"]["review"] == old, "OR_REVIEW_IDENTITY")
    restored = copy.deepcopy(new)
    for key in ("reviewedAtMs", "reviewerId", "assumptionLabel", "expiresAtMs"):
        restored[key] = old[key]
    prior_node = next(s for s in old["sources"] if s["kind"] == "official_contract_node")
    new_node = next(s for s in restored["sources"] if s["kind"] == "official_contract_node")
    new_node["sha256"], new_node["observedAtMs"] = prior_node["sha256"], prior_node["observedAtMs"]
    restored["valuationObjective"]["timingSourceSha256"] = old["valuationObjective"]["timingSourceSha256"]
    need(restored == old and sha(f["original-node.receipt.json"]) == NODE
         and new["expiresAtMs"] == 1792262095251, "ONLY_NODE_REVIEW_CHANGE")
    old_auth, auth = load(f["old-authority.original.json"]), load(f["authority.json"])
    need(auth["trustedReviewSha256"] == old_auth["trustedReviewSha256"] + [REVIEW]
         and len(old_auth["trustedReviewSha256"]) == 26, "AUTHORITY_HISTORY")
    base, overlay = load(c["base-control.original.json"]), load(f["next-control.inactive.json"])
    before, after = copy.deepcopy(base), copy.deepcopy(overlay)
    before.pop("fundamentalPolicy"); after.pop("fundamentalPolicy")
    need(before == after and overlay["fundamentalPolicy"] == load(f["settings.json"]), "FINANCIAL_CONTROL_CHANGED")
    for name in ("typed-final-review.stdout.json", "typed-final-settings.stdout.json"):
        typed = load(f[name]); need(typed["executionAuthority"] is False, "TYPED_VALIDATOR_AUTHORITY")
        if "reviews" in typed:
            need(typed["typedCurrentReviewChecksPassed"] is True and typed["reviews"][0]["canonicalSha256"] == REVIEW, "TYPED_REVIEW_RESULT")
        else:
            need(typed["settingsLoadPassed"] is True and typed["sourcesVerified"] is True, "TYPED_SETTINGS_RESULT")
    anchor, head, report = (sealed(f[n]) for n in ("stopped-head.original.json", "head.original.json", "report.original.json"))
    native = chain(f["native.suffix.ndjson"], anchor)
    need(anchor["sequence"] == 481281 and native[-1]["sequence"] == head["sequence"] == report["sequence"]
         and native[-1]["hash"] == head["previous"] == report["previous"], "NATIVE_SUFFIX_BOUNDARY")
    captured = event["nativeHead"]; at_capture = next(r for r in native if r["sequence"] == captured["sequence"])
    need(captured["sequence"] == 482297 and captured["previous"] == at_capture["hash"], "GENERATION_NATIVE_CAPTURE")
    mode = next(r for r in native if r["sequence"] == 482298)
    need(mode["event"]["type"] == "managed_inventory_planning_v1"
         and mode["event"]["generationSha256"] == GENERATION
         and mode["event"]["bankHead"] == captured
         and mode["event"]["controlSha256"] == BASE, "GENERATION_NATIVE_MODE_BINDING")
    response = load(f["reload-001.stdout.json"])
    need(response["state"] == "committed" and response["generationSha256"] == GENERATION
         and response["baseFinancialControlUnchanged"] is True
         and response["nativeHead"]["sequence"] == mode["sequence"]
         and response["nativeHead"]["previous"] == mode["hash"], "COMMITTED_CLI_NATIVE_BOUNDARY")
    # No financial transition follows the generation in this pinned suffix;
    # therefore the later report's cash/OR lots can check the original facts.
    safe = {"book", "trade", "coverage", "gap", "tick", "unsubmitted_intent_retired", "managed_inventory_planning_v1"}
    need(all(r["event"]["type"] in safe for r in native if r["sequence"] > captured["sequence"]), "POST_GENERATION_FINANCIAL_TRANSITION")
    endpoints = ["/tournaments/midterm-elections", "/tournaments/midterm-elections/portfolio/positions",
                 f"/orders?status=open&limit=200&tournamentId={TID}", f"/account?tournamentId={TID}"]
    facts = {load(raw)["endpoint"]: (name, raw, load(raw)) for name, raw in f.items() if name.startswith("facts/")}
    need(set(facts) == set(endpoints), "FOUR_FACT_ENDPOINTS")
    encoded = []; last = 0; ages = []
    for endpoint in endpoints:
        name, raw, receipt = facts[endpoint]; wire = receipt_bytes(raw)
        need(Path(name).stem == sha(wire) and receipt["status"] == 200, "FACT_TYPED_ORIGINAL_HASH")
        start, end = clock(receipt["requestStartedAt"]), clock(receipt["receivedAt"])
        need(last <= start <= end <= g["atMs"] and g["atMs"] - start <= 20000, "FACT_ORIGINAL_FRESHNESS")
        last = end; ages.append(g["atMs"] - start); encoded.append(wire)
    facts_sha = sha(b"[" + b",".join(encoded) + b"]")
    need(facts_sha == event["factsSha256"] == FACTS, "GENERATION_FACTS_HASH")
    tournament, positions, orders, account = [facts[e][2]["data"] for e in endpoints]
    need(tournament["id"] == TID and tournament["slug"] == "midterm-elections" and tournament["status"] == "active", "FACT_TOURNAMENT")
    need("pagination" not in positions and orders["coverage"]["complete"] is True
         and orders["pagination"]["hasMore"] is False and orders["pagination"]["nextCursor"] is None
         and not any(r["exchangeId"] == "977" for r in orders["data"]), "ORIGINAL_NO_OR_OPEN_ORDER")
    lots = report["report"]["positions"]["977"]; quantity = sum(l["quantity"] for l in lots["NO"])
    basis = sum(Decimal(l["price"]) * l["quantity"] for l in lots["NO"])
    actual = [p for p in positions["positions"] if p["exchangeId"] == "977"]
    need(len(actual) == 1 and actual[0]["quantity"] == -quantity and not lots["YES"]
         and quantity <= new["maximumQuantity"] and basis <= Decimal(new["maximumPrincipal"])
         and account["balance"] == Decimal(report["report"]["cash"]), "OR_ACCOUNT_CAP_AND_CASH")
    dispatch = chain(f["dispatch.original.ndjson"])
    d = event["dispatch"]; prefix = f["dispatch.original.ndjson"][:d["bytes"]]
    drows = chain(prefix)
    need(drows[-1]["sequence"] == d["sequence"] and drows[-1]["hash"] == d["previous"], "GENERATION_DISPATCH_PREFIX")
    attempts, acks, closed, aborted = set(), set(), set(), set()
    for r in drows:
        e = r["event"]; q = e.get("qid")
        if e["kind"] == "attempt": attempts.add(q)
        if e["kind"] in ("ack", "recovered_ack"): acks.add(q)
        if e["kind"] == "closed": closed.add(q)
        if e["kind"] == "aborted_before_admission": aborted.add(q)
    need(not (attempts - acks - aborted), "UNKNOWN_ATTEMPT_AT_GENERATION")
    or_ids = {q["id"] for q in report["orders"] if q["exchangeId"] == "977"}
    need(not ((attempts | acks) & or_ids - closed - aborted), "OR_PENDING_PHYSICAL_AT_GENERATION")
    status, progress, summary = [load(f[n]) for n in ("model-status.original.json", "progress.original.json", "activation-summary.json")]
    need(status["workerPid"] == progress["pid"] == request["workerPid"] == 651324
         and status["workerStartTicks"] == request["workerStartTicks"]
         and status["ownerLease"] == request["ownerLease"] and status["baseControlSha256"] == BASE
         and status["previousGenerationSha256"] == GENERATION
         and progress["phase"] == "trading" and progress["realtimeHealthy"] is True
         and 0 <= clock(summary["at"]) - clock(progress["at"]) <= 120000, "CAPTURED_FRESH_WORKER_CHANNEL")
    need(len(load(f["bridge.original.json"])["upgrades"]) == 15
         and sha(f["bridge.original.json"]) == "b54830b219fba35238792927fc0c123c582a91b4311cb86a3b100f305f4c32e8",
         "ORIGINAL_BRIDGE_BYTES_AND_COUNT")
    return {"result": "PASS_PINNED_COMMITTED_GENERATION_PROOF", "privatePacketSha256": LIVE,
            "originalFilesVerified": len(f), "candidatePacketSha256": CANDIDATE,
            "originalFourGenerationPrefixByteIdentical": True, "newGenerationSha256": GENERATION,
            "newGenerationCount": len(gens), "newReviewCanonicalSha256FromRetainedTypedValidator": REVIEW,
            "unchangedOtherReviews": 10, "old26ApprovalHashesRetained": True,
            "onlyNodeObservationAndReviewMetadataChanged": True, "forecastBallotClocksAndEconomicTermsUnchanged": True,
            "nativeSuffixFirstSequence": native[0]["sequence"], "nativeSuffixLastSequence": native[-1]["sequence"],
            "nativeSuffixRows": len(native), "generationCapturedNativeSequence": 482297,
            "committedCliNativeSequence": 482298, "baseControlSha256": BASE,
            "fourOriginalReceiptTokenFactsSha256": facts_sha, "factRequestAgesAtCommitMs": ages,
            "originalOrAccountQuantityMatches": True, "orQuantity": quantity,
            "orRemainingFifoPrincipal": str(basis), "orMaximumPrincipal": new["maximumPrincipal"],
            "noOrOpenOrUnresolvedRecordedPhysicalAtGeneration": True,
            "capturedPid": 651324, "capturedFreshTrading": True,
            "capturedProgressAt": progress["at"], "captureObservedAt": summary["at"],
            "notCurrentHealthClaim": True, "noWorkerRestartOrFinancialControlRenewal": True,
            "financialSessionReplayPerformed": False, "newAccountRequests": 0,
            "rawProviderAuthenticationIndependentlyReperformed": False,
            "sdkNoNetworkArchivesIndependentlyReverified": False,
            "executionAuthority": False, "profitabilityEstablished": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("live-packet", "candidate-packet", "previous-packet"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = verify(args.live_packet, args.candidate_packet, args.previous_packet)
    raw = (json.dumps(result, indent=2) + "\n").encode()
    if args.output:
        with args.output.open("xb") as output: output.write(raw)
    print(raw.decode(), end="")


if __name__ == "__main__":
    main()
