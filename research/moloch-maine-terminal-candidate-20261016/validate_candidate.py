"""Bounded offline source checks. No network, orders or financial authority."""
import hashlib
import html
import json
import re
import sys
import xml.etree.ElementTree as ET
import zipfile
from datetime import datetime
from decimal import Decimal, ROUND_FLOOR
from pathlib import Path


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def visible(raw):
    value = raw.decode("utf-8")
    value = re.sub(r"<script\b[^>]*>.*?</script>", " ", value, flags=re.S | re.I)
    return re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", " ", value)))


def governor_candidates(path):
    ns = {"m": "http://schemas.openxmlformats.org/spreadsheetml/2006/main"}
    with zipfile.ZipFile(path) as archive:
        assert sum(item.file_size for item in archive.infolist()) < 5_000_000
        strings = ["".join(item.itertext()) for item in ET.fromstring(archive.read("xl/sharedStrings.xml")).findall("m:si", ns)]
        found = []
        for row in ET.fromstring(archive.read("xl/worksheets/sheet1.xml")).findall(".//m:row", ns):
            cells = {}
            for cell in row.findall("m:c", ns):
                value = cell.find("m:v", ns)
                text = value.text if value is not None else "".join(cell.itertext())
                cells[re.sub(r"[0-9]", "", cell.get("r"))] = strings[int(text)] if cell.get("t") == "s" else text
            if cells.get("A") == "GOV":
                found.append((cells.get("D"), cells.get("F"), cells.get("G"), cells.get("H", "")))
        return found


def main(directory):
    here = Path(__file__).resolve().parent
    review = json.loads((here / "review-input.inactive.json").read_bytes())
    evidence = json.loads((here / "evidence.json").read_bytes())
    originals = Path(directory)
    successful = {}
    for item in evidence["sourcePins"]:
        if item.get("status") != 200:
            continue
        assert item["bytes"] < 3_000_000
        raw = (originals / item["file"]).read_bytes()
        assert len(raw) == item["bytes"]
        assert hashlib.sha256(raw).hexdigest() == item["sha256"]
        successful[item["file"]] = item
    expected = [("Independent", "Bennett", "Rick", ""), ("R", "Charles", "Robert", "B."), ("D", "Pingree", "Hannah", "M.")]
    assert governor_candidates(originals / "official-candidates.xlsx") == expected
    node_raw = (originals / "official-contract-node.json").read_bytes()
    node = json.loads(node_raw)
    root = node["data"]["root"]
    assert hashlib.sha256(node_raw).hexdigest() == review["sources"][2]["sha256"]
    assert node["data"]["market_id"] == review["marketId"] == "176"
    assert root["node_id"] == root["contract_id"] == review["contractId"] == "177"
    assert root["contract_details"]["raceName"] == "Governor of Maine"
    assert root["contract_details"]["winnerName"] == "Democratic Party"
    assert root["contract_details"]["resolutionType"] == "Party Winner"
    assert node["data"]["contexts"][0]["tournament"]["id"] == review["tournamentId"]
    assert hashlib.sha256(canonical(root)).hexdigest() == review["officialContractRootSha256"]
    assert hashlib.sha256(canonical(review)).hexdigest() == evidence["reviewCanonicalSha256"]
    forecast = visible((originals / "forecast.html").read_bytes())
    assert re.search(r"Hannah Pingree.{0,90}95\.4%", forecast)
    assert re.search(r"Rick Bennett.{0,90}<0\.1%", forecast)
    assert review["sources"][0]["sha256"] == successful["forecast.html"]["sha256"]
    assert review["sources"][1]["sha256"] == successful["official-candidates.xlsx"]["sha256"]
    assert review["sources"][1]["publishedAtMs"] == 0
    assert review["expiresAtMs"] == min(s["observedAtMs"] + s["maximumAgeMs"] for s in review["sources"])
    assert all(s["publishedAtMs"] <= s["observedAtMs"] <= review["reviewedAtMs"] for s in review["sources"])
    assert review["side"] == "YES" and review["exchangeId"] == "865"
    assert review["maximumPrincipal"] == "2000.000000"
    p, e, margin = map(Decimal, (review["ordinaryProbability"], review["exceptionalProbabilityAllowance"], review["marginPerShare"]))
    assert (1-e)*p == Decimal(".925380")
    assert (((1-e)*p-margin)*1000).to_integral_value(rounding=ROUND_FLOOR)/1000 == Decimal(".915")
    assert Decimal(".91") < (Decimal(".875")+margin)/(1-e) < p
    assert "null" not in json.dumps(review)
    objective = review["valuationObjective"]
    assert objective["timingSourceSha256"] == review["sources"][2]["sha256"]
    assert objective["delayPolicy"] == "holdForActualSettlement"
    settlement = int(datetime.fromisoformat(root["settlement_date"].replace("Z", "+00:00")).timestamp()*1000)
    assert objective["scheduledSettlementAtMs"] == review["holdUntilMs"] == settlement
    print(json.dumps({"status": "offline_original_source_checks_passed_not_runtime_authority", "reviewCanonicalSha256": evidence["reviewCanonicalSha256"], "expectedPayout": ".925380", "entryTickCeiling": ".915"}))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: validate_candidate.py PRIVATE_ORIGINALS_DIRECTORY")
    main(sys.argv[1])
