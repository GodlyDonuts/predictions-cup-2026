"""Read-only, finite terminal-candidate arithmetic; never an execution/prize model."""
import datetime as dt
from decimal import Decimal, ROUND_FLOOR
import hashlib
import html
import importlib.util
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[2]
TICK = Decimal(".005")


def economics(probability, price, depth, budget, quantity_cap=10000, exception=Decimal(".03"), margin=Decimal(".01")):
    p, c, qmax, capital = map(Decimal, (probability, price, depth, budget))
    if not (0 <= p <= 1 and 0 < c < 1 and qmax >= 0 and capital >= 0 and 0 <= exception < 1 and margin >= 0):
        raise ValueError("bounds")
    value = (1-exception)*p
    ceiling = max(Decimal(0), ((value-margin)/TICK).to_integral_value(rounding=ROUND_FLOOR)*TICK)
    q = min(int(capital/c), int(qmax), quantity_cap)
    cost = q*c
    return {"ordinaryProbability": str(p), "exceptionAllowance": str(exception), "expectedPayoutPerShareAssumption": str(value), "entryCeiling": str(ceiling), "capturedAsk": str(c), "passesCapturedPrice": c <= ceiling,
            "requiredProbabilityIncludingMargin": str((c+margin)/(1-exception)), "budget": str(capital), "quantity": q, "principalAtCapturedAsk": str(cost), "unspentBudget": str(capital-cost), "grossWinningPayout": str(q),
            "winningIncrement": str(q-cost), "maximumLoss": str(cost), "modelExpectedIncrementAssumption": str(q*(value-c)), "winningPayoutMultiple": str(1/c)}


def current_probability(raw, name):
    text = raw.decode("utf-8")
    text = re.sub(r"<script\b[^>]*>.*?</script>", " ", text, flags=re.S|re.I)
    text = re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", " ", text)))
    start = text.index("Candidate chance")
    table = text[start:start+1400]
    match = re.search(re.escape(name)+r".{0,90}?(\d+(?:\.\d+)?)%", table)
    if not match:
        raise ValueError("candidate_probability_missing")
    return Decimal(match.group(1))/100


def run(manifest_path, source_folder):
    mp, folder = Path(manifest_path), Path(source_folder)
    manifest_raw = mp.read_bytes()
    manifest = json.loads(manifest_raw)
    for item in manifest["inputs"]:
        if item["bytes"] > 32_000_000:
            raise ValueError("input_bound")
        raw = Path(item["path"]).read_bytes()
        if len(raw) != item["bytes"] or hashlib.sha256(raw).hexdigest() != item["sha256"]:
            raise ValueError("input_pin")
    spec = importlib.util.spec_from_file_location("frozen_prior_parser", ROOT/"research/moloch-shadow-prior-opportunities-20261016/analyze.py")
    parser = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(parser)
    books = {}
    for item in manifest["inputs"]:
        if not item["name"].startswith("sig-breadth-suffix-"):
            continue
        values, _, _ = parser.original_books(item["path"], item["startOffset"])
        for key, value in values.items():
            old = books.get(key)
            if old is None or (int(value["book"]["asOf"]["sequence"]), value["book"]["asOf"]["at"]) > (int(old["book"]["asOf"]["sequence"]), old["book"]["asOf"]["at"]):
                value["artifact"] = item["artifact"]
                books[key] = value
    pins_raw = (folder/"source-pins.json").read_bytes()
    pins = json.loads(pins_raw)
    successful = {}
    for item in pins["sources"]:
        if item.get("status") != 200:
            continue
        raw = (folder/item["file"]).read_bytes()
        if len(raw) != item["bytes"] or hashlib.sha256(raw).hexdigest() != item["sha256"]:
            raise ValueError("primary_source_pin")
        successful[item["file"]] = item
    cases = [("AK Republican Senate YES", "378", "1067", "YES", "ak-senate.html", "Dan S. Sullivan", ".507"),
             ("IA Democratic Governor NO", "168", "857", "NO", "ia-governor.html", "Zach Lahn", ".166"),
             ("SC Democratic Governor YES", "192", "881", "YES", "sc-governor.html", "Jermaine Johnson", ".040"),
             ("FL Democratic Governor YES", "299", "988", "YES", "fl-governor.html", "David Jolly", ".155"),
             ("TX Democratic Governor YES", "195", "884", "YES", "tx-governor.html", "Gina Hinojosa", ".136")]
    results = []
    for label, market, exchange, side, file, candidate, expected in cases:
        p = current_probability((folder/file).read_bytes(), candidate)
        if p != Decimal(expected):
            raise ValueError("primary_probability_changed")
        source = books[(market, exchange)]
        book = source["book"]
        levels = parser.sig_levels(book["asks"] if side == "YES" else book["bids"], side == "NO")
        if not levels:
            raise ValueError("missing_depth")
        price, quantity = levels[0]
        if side == "NO":
            price = 1_000_000-price
        c, depth = Decimal(price)/1_000_000, Decimal(quantity)/1_000_000
        results.append({"label": label, "marketId": market, "exchangeId": exchange, "side": side, "originalSourceAt": book["asOf"]["at"], "originalReceivedAt": source["receivedAt"], "capturedTopDepth": str(depth),
                        "originalRowSha256": source["originalRowSha256"], "journalHashReference": source["journalHash"], "originalSequence": book["asOf"]["sequence"], "sourceArtifact": source["artifact"], "sourceLine": source["line"],
                        "forecastOriginal": successful[file], "pointCase": economics(p, c, depth, "2000"), "capitalSensitivity": [economics(p, c, depth, amount) for amount in ["100", "500", "2000"]],
                        "qualification": "conditional_research_lead_missing_exact_retained_SIG_node_and_fresh_physical_facts" if market in ["378", "168"] else "rejected_even_on_selected_model_point"})
    node_raw = (folder/"official-contract-node-378.original.json").read_bytes()
    node = json.loads(node_raw)
    root = node["data"]["root"]
    if hashlib.sha256(node_raw).hexdigest() != "38ad364b1d91ae1825a38e3e8c135ba49d2a79554fd46210e835772ef40b19c0" or node["data"]["market_id"] != "378" or root["node_id"] != root["contract_id"] or root["contract_id"] != "379" or root["contract_details"]["winnerName"] != "Republican Party" or root["contract_details"]["raceStage"] != "General" or root["contract_details"]["usState"] != "AK":
        raise ValueError("actual_alaska_node_binding")
    results[0]["qualification"] = "inactive_typed_review_with_original_node_proposed_48h_reference_TTL_requires_root_approval_and_fresh_physical_facts"
    results[0]["originalNodeSha256"] = hashlib.sha256(node_raw).hexdigest()
    results[0]["canonicalRootSha256"] = hashlib.sha256(json.dumps(root, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()
    results[0]["nodeOriginalReceivedAt"] = node["receivedAt"]
    results[0]["probabilitySensitivity"] = [economics(p, ".300", "11929", "2000") for p in [".507", ".44", ".35", ".32", ".30"]]
    results[1]["probabilitySensitivity"] = [economics(p, ".130", "8289", "2000") for p in [".166", ".15", ".14"]]
    return {"schemaVersion": 1, "observedAt": dt.datetime.now(dt.timezone.utc).isoformat(), "executionAuthority": False, "currentExecutablePricesClaimed": False, "prizeEVAvailable": False, "calibratedProbabilityBound": False,
            "capturedPriceCutoff": manifest["asOfUtc"], "originalInputManifestSha256": hashlib.sha256(manifest_raw).hexdigest(), "primarySourcePinsSha256": hashlib.sha256(pins_raw).hexdigest(),
            "originalSources": pins["sources"], "cases": results, "limits": ["Later forecasts versus older prices are asynchronous nominations, not causal backtests or currently tradable EV.", "Three-percent exception mass and zero exceptional payout are declared operator stress, not empirical probability bounds.", "Alaska has original structured node evidence and an inactive proposed typed review; Iowa exact SIG node is missing after owner cache search, and no Iowa typed review is minted.", "Partial original suffix hashes do not verify a complete journal chain or provider continuity.", "Prize-rank improvement needs original full held-wealth/joint/opponent evidence; no independence or beneficial whole-account convexity is assumed."]}


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("usage: analyze.py PINNED_EXISTING_SIG_INPUT_MANIFEST NEW_PRIMARY_SOURCE_DIRECTORY")
    print(json.dumps(run(sys.argv[1], sys.argv[2]), indent=2, sort_keys=True))
