"""Bounded offline original-byte and arithmetic checks; grants no authority."""
import hashlib
import html
import json
import re
import sys
from decimal import Decimal, ROUND_FLOOR
from pathlib import Path


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def visible(raw):
    value = raw.decode("utf-8")
    value = re.sub(r"<script\b[^>]*>.*?</script>", " ", value, flags=re.S | re.I)
    return re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", " ", value)))


def main(directory):
    here = Path(__file__).resolve().parent
    review = json.loads((here / "review-input.inactive.json").read_bytes())
    evidence = json.loads((here / "evidence.json").read_bytes())
    originals = Path(directory)
    for item in evidence["sourcePins"]:
        raw = (originals / item["file"]).read_bytes()
        assert len(raw) == item["bytes"]
        assert hashlib.sha256(raw).hexdigest() == item["sha256"]
    node_raw = (originals / "official-contract-node.json").read_bytes()
    node = json.loads(node_raw)
    assert hashlib.sha256(node_raw).hexdigest() == review["sources"][2]["sha256"]
    root = node["data"]["root"]
    assert node["data"]["market_id"] == review["marketId"] == "288"
    assert root["node_id"] == root["contract_id"] == review["contractId"] == "289"
    assert root["contract_details"]["winnerName"] == "Republican Party"
    assert root["contract_details"]["resolutionType"] == "Party Winner"
    assert hashlib.sha256(canonical(root)).hexdigest() == review["officialContractRootSha256"]
    assert hashlib.sha256(canonical(review)).hexdigest() == evidence["reviewCanonicalSha256"]
    forecast = visible((originals / "forecast.html").read_bytes())
    assert re.search(r"Jeff Merkley.{0,120}99\.5%", forecast)
    ballot = visible((originals / "official-candidates.html").read_bytes())
    assert re.search(r"Jeff Merkley.{0,40}Democrat.{0,20}Working Families", ballot)
    assert re.search(r"David Brock Smith.{0,40}Republican", ballot)
    assert review["sources"][1]["publishedAtMs"] == 0
    assert review["expiresAtMs"] == min(s["observedAtMs"] + s["maximumAgeMs"] for s in review["sources"])
    assert all(s["publishedAtMs"] <= s["observedAtMs"] <= review["reviewedAtMs"] for s in review["sources"])
    assert review["side"] == "NO" and review["maximumPrincipal"] == "2000.000000"
    p, e, margin = map(Decimal, (review["ordinaryProbability"], review["exceptionalProbabilityAllowance"], review["marginPerShare"]))
    value = (1 - e) * p
    assert value == Decimal(".965150")
    assert ((value - margin) * 1000).to_integral_value(rounding=ROUND_FLOOR) / 1000 == Decimal(".955")
    assert "null" not in json.dumps(review)
    assert review["valuationObjective"]["timingSourceSha256"] == review["sources"][2]["sha256"]
    print(json.dumps({"status": "offline_checks_passed_not_runtime_authority", "reviewCanonicalSha256": evidence["reviewCanonicalSha256"], "expectedPayout": str(value), "entryTickCeiling": ".955"}))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: validate_candidate.py PRIVATE_ORIGINALS_DIRECTORY")
    main(sys.argv[1])
