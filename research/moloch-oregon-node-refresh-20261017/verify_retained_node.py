"""Offline verification of pinned retained OR node evidence; no approval or renewal."""
import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path
import re
import zipfile

TID = "bda92870-621e-47b0-bc3c-3602c5c26f55"
ENDPOINT = f"/markets/288/nodes?tournamentId={TID}"
NEW_RAW = "e6d1b884d628881ffda0fdf86a8ac3ec3b8c974ec60ba5635c4641a7deff664f"
OLD_RAW = "9bca4722d2a06e32e0d86360b43c6aae6e4336876dec40a5a50b4cb70f9568f1"
TYPED_RECEIPT = "906ba379ab9b8d8dd1497e11fdc32bbd3290cf6e77f6a59816411310fc6ed664"
MANIFEST = "9579856dcdfc084b75d6f6a4defcec8dab20ed817c97ae056a9a956aba500e17"
ROOT = "ebe719c4fc2796abfc66e2669db45e84abf9c1c7310e00dc298f05f5c2c60d15"
PACKET = "ddf08bca7c2f15d20875983ae0ffb414cddc06de38a04d94d97b050555bc48ee"
SIDECAR = "f5b1e36977e81dc15d2d434272846359e4b262328a38a2a556f5f36df022529e"
GENERATION = "b27c469978fbe3149dcbaa74a42a1d8b79867fe57be40ca50413d572eae84e17"
DECLARED_REVIEW = "b32de5206f69525b6c63a371b99b31356634a4ff252ce61e96b7529588ca79aa"
NODE_FILE = "nodes/market-288-e6d1b884d628881f.json"


def need(ok, reason):
    if not ok:
        raise ValueError(reason)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=False, allow_nan=False).encode()


def sorted_value(value):
    if isinstance(value, dict):
        return {k: sorted_value(value[k]) for k in sorted(value)}
    if isinstance(value, list):
        return [sorted_value(x) for x in value]
    return value


def receipt_digest(receipt):
    # Receipt is a Rust struct: preserve DECLARED field order at the top level.
    # Its data is serde_json::Value: recursively sort object keys there only.
    fields = ("endpoint", "requestStartedAt", "receivedAt", "status", "data")
    need(set(receipt) == set(fields), "RECEIPT_FIELDS")
    typed = {k: sorted_value(receipt[k]) if k == "data" else receipt[k] for k in fields}
    return sha(json.dumps(typed, separators=(",", ":"), ensure_ascii=False,
                          allow_nan=False).encode())


def bounded(path, maximum, expected):
    path = Path(path)
    need(path.is_file() and not path.is_symlink() and path.stat().st_size <= maximum,
         "INPUT_FILE_OR_BOUND")
    raw = path.read_bytes()
    need(sha(raw) == expected, "ORIGINAL_RAW_SHA256")
    return raw


def millis(stamp):
    value = dt.datetime.fromisoformat(stamp.replace("Z", "+00:00"))
    need(value.tzinfo is not None, "SOURCE_CLOCK_ZONE")
    return int(value.timestamp() * 1000)


def verify(directory, packet, old_node):
    manifest_raw = bounded(directory / "manifest.json", 1024 * 1024, MANIFEST)
    manifest = json.loads(manifest_raw)
    need(manifest["readOnlyExistingOriginalReceipts"] is True
         and manifest["apiRequests"] == 0, "EXTRACTION_PROVENANCE")
    pins = [x for x in manifest["files"] if x["kind"] == "nodes" and x["marketId"] == "288"]
    need(len(pins) == 1, "UNIQUE_NODE_PIN")
    pin = pins[0]
    need(pin["path"] == NODE_FILE and pin["sha256"] == NEW_RAW
         and pin["bytes"] == 907, "EXACT_MANIFEST_NODE_PIN")
    new_raw = bounded(directory / NODE_FILE, 907, NEW_RAW)
    need(len(new_raw) == 907, "NODE_BYTE_COUNT")
    old_raw = bounded(old_node, 64 * 1024, OLD_RAW)
    old, new = json.loads(old_raw), json.loads(new_raw)
    for receipt in (old, new):
        need(receipt["endpoint"] == ENDPOINT and receipt["status"] == 200,
             "OFFICIAL_ENDPOINT_STATUS")
        data = receipt["data"]
        need(data["market_id"] == "288"
             and data["contexts"][0]["type"] == "tournament"
             and len(data["contexts"]) == 1
             and data["contexts"][0]["tournament"]["id"] == TID, "EXACT_MARKET_TOURNAMENT")
        root = data["root"]
        need(root["node_type"] == "contract" and root["node_id"] == root["contract_id"] == "289"
             and sha(canonical(root)) == ROOT, "EXACT_ORIGINAL_ROOT")
        need(millis(receipt["requestStartedAt"]) <= millis(receipt["receivedAt"]), "RECEIPT_CLOCK")
    need(old["data"]["root"] == new["data"]["root"], "ROOT_CHANGED")
    need(new["requestStartedAt"] == pin["requestStartedAt"] == "2026-10-16T19:17:11.545Z"
         and new["receivedAt"] == pin["receivedAt"] == "2026-10-16T19:17:11.799Z"
         and pin["endpoint"] == ENDPOINT and pin["status"] == 200, "NEW_ORIGINAL_CLOCKS")
    need(millis(new["requestStartedAt"]) > millis(old["receivedAt"]), "NOT_A_LATER_OBSERVATION")
    need(receipt_digest(new) == TYPED_RECEIPT
         and Path(pin["originalSource"]).name == TYPED_RECEIPT + ".json", "ORIGINAL_RECEIPT_STORE_BINDING")
    # The packet is pinned; inspect only its bounded original policy member.
    bounded(packet, 16 * 1024 * 1024, PACKET)
    with zipfile.ZipFile(packet) as archive:
        info = archive.getinfo("fundamental.original.jsonl")
        need(info.file_size <= 1024 * 1024, "SIDECAR_MEMBER_BOUND")
        sidecar = archive.read(info)
    need(sha(sidecar) == SIDECAR and sidecar.endswith(b"\n"), "ORIGINAL_SIDECAR_SHA256")
    previous, sequence, generations = "0" * 64, 0, []
    for line in sidecar.splitlines():
        row = json.loads(line)
        unsigned, count = re.subn(rb'"hash":"' + row["hash"].encode() + rb'",', b"", line, count=1)
        need(count == 1 and sha(unsigned) == row["hash"] and row["previous"] == previous
             and row["sequence"] == sequence + 1, "SIDECAR_CHAIN")
        previous, sequence = row["hash"], row["sequence"]
        if row["event"]["kind"] == "review_generation":
            generations.append(row["event"])
    generation = generations[-1]
    need(len(generations) == 4 and generation["generation"]["generationSha256"] == GENERATION,
         "ACTIVE_ORIGINAL_GENERATION")
    reviews = generation["reviews"]
    need(len(reviews) == 11, "ELEVEN_REVIEW_PACKET")
    selected = [x for x in reviews if x["review"]["exchangeId"] == "977"]
    need(len(selected) == 1 and selected[0]["sha256"] == DECLARED_REVIEW, "ORIGINAL_REVIEW_BINDING")
    review = selected[0]["review"]
    need(review["marketId"] == "288" and review["contractId"] == "289"
         and review["side"] == "NO" and review["officialContractRootSha256"] == ROOT, "REVIEW_SCOPE")
    sources = {s["kind"]: s for s in review["sources"]}
    need(sources["official_contract_node"]["sha256"] == OLD_RAW
         and sources["official_contract_node"]["observedAtMs"] == millis(old["receivedAt"])
         and sources["official_contract_node"]["maximumAgeMs"] == 172800000, "OLD_NODE_REVIEW_PIN")
    objective = review["valuationObjective"]
    need(objective["kind"] == "terminalSettlement"
         and objective["timingSourceSha256"] == OLD_RAW
         and objective["timingJsonPointer"] == "/data/root/settlement_date"
         and objective["delayPolicy"] == "holdForActualSettlement"
         and objective["scheduledSettlementAtMs"] == millis(new["data"]["root"]["settlement_date"]),
         "UNCHANGED_TERMINAL_OBJECTIVE")
    ends = {k: s["observedAtMs"] + s["maximumAgeMs"] for k, s in sources.items()}
    need(ends["forecast"] == millis("2026-10-17T18:34:55.251Z")
         and ends["official_ballot"] == millis("2026-10-17T18:34:59.736Z"), "UNCHANGED_OTHER_SOURCE_CLOCKS")
    return {"schemaVersion": 1, "verification": "pinned_retained_node_provenance_only",
            "oldNodeRawSha256": OLD_RAW, "newNodeRawSha256": NEW_RAW, "newNodeBytes": 907,
            "extractionManifestSha256": MANIFEST, "activePacketSha256": PACKET,
            "originalSidecarSha256": SIDECAR, "originalGenerationSha256": GENERATION,
            "originalDeclaredReviewSha256": DECLARED_REVIEW,
            "originalReviewTypedHashIndependentlyRecomputed": False,
            "newNodeTypedReceiptSha256": TYPED_RECEIPT,
            "typedDigestConvention": "Receipt field order endpoint/requestStartedAt/receivedAt/status/data; nested Value keys sorted",
            "marketId": "288", "exchangeId": "977", "contractId": "289", "side": "NO",
            "endpoint": ENDPOINT, "status": 200, "semanticRootSha256": ROOT,
            "rootExactEqual": True, "newRequestStartedAt": new["requestStartedAt"],
            "newReceivedAt": new["receivedAt"], "publicationTimeRenewed": False,
            "settlementDate": new["data"]["root"]["settlement_date"],
            "delayPolicy": objective["delayPolicy"],
            "unchangedSourceRefs": {k: sources[k] for k in ("forecast", "official_ballot")},
            "unchangedSourceExpiryMs": {k: ends[k] for k in ("forecast", "official_ballot")},
            "newNode48hObservationEndMs": millis(new["receivedAt"]) + 172800000,
            "otherSourcesWouldStillLimitReviewUntilMs": ends["forecast"],
            "newForecast": False, "candidateGenerated": False, "authorityChanged": False,
            "activation": False, "accountRequests": 0, "profitabilityEstablished": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--retained-directory", type=Path, required=True)
    parser.add_argument("--active-packet", type=Path, required=True)
    parser.add_argument("--old-node", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = verify(args.retained_directory, args.active_packet, args.old_node)
    raw = (json.dumps(result, indent=2, ensure_ascii=False, allow_nan=False) + "\n").encode()
    if args.output:
        with args.output.open("xb") as output:
            output.write(raw)
    print(raw.decode(), end="")


if __name__ == "__main__":
    main()
