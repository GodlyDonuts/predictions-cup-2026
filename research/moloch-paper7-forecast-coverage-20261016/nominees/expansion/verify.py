"""Finite five-race source reproduction. No network, account, orders or approval.

Raw candidate/contact documents remain private. pypdf is needed only to read the
original Texas certification and Wisconsin sample ballot. This is not a native
typed review factory or a calibration test.
"""
from decimal import Decimal as D, ROUND_FLOOR
from hashlib import sha256
from html.parser import HTMLParser
from pathlib import Path
import json
import importlib.util
import re
import sys

SPECS = [
    ("governors", "WI", "315", "Tom Tiffany", "0.377"),
    ("governors", "TX", "196", "Greg Abbott", "0.864"),
    ("governors", "KS", "304", "Ty Masterson", "0.84"),
    ("senate", "FL", "355", "Ashley Moody", "0.933"),
    ("governors", "AK", "368", "Jonathan Kreiss-Tomkins", "0.856"),
]


def need(ok, why):
    if not ok:
        raise ValueError(why)


def check(raw, pin, key="sha256"):
    need(sha256(raw).hexdigest() == pin[key], "raw_source_sha")
    if "bytes" in pin:
        need(len(raw) == pin["bytes"], "raw_source_length")


class Tables(HTMLParser):
    def __init__(self):
        super().__init__()
        self.rows, self.row, self.cell = [], [], None

    def handle_starttag(self, tag, attrs):
        if tag == "tr":
            self.row = []
        if tag in ("td", "th"):
            self.cell = []

    def handle_endtag(self, tag):
        if tag in ("td", "th") and self.cell is not None:
            self.row.append(" ".join("".join(self.cell).split()))
            self.cell = None
        if tag == "tr" and self.row:
            self.rows.append(self.row)

    def handle_data(self, data):
        if self.cell is not None:
            self.cell.append(data)


def rows(raw):
    p = Tables()
    p.feed(raw.decode("utf-8") if isinstance(raw, bytes) else raw)
    return p.rows


def kansas(raw):
    found = [r[:4] for r in rows(raw) if len(r) >= 4
             and r[2] == "Governor" and r[3] == "2026 General"]
    need(found == [
        ["Republican", "Ty Masterson / Jeffrey Klemp", "Governor", "2026 General"],
        ["Democratic", "Cindy Holscher / KC Ohaebosim", "Governor", "2026 General"],
    ], "kansas_current_general_field")
    return found


def alaska(raw):
    text = raw.decode("utf-8")
    marker = "GOVERNOR / LIEUTENANT GOVERNOR"
    need("2026 General Election" in text and marker in text, "alaska_general_context")
    section = text[text.index(marker):]
    need("</table>" in section, "alaska_table_boundary")
    found = [r[0] for r in rows(section[:section.index("</table>") + 8])
             if r and not r[0].startswith("Candidate")]
    need(len(found) == 6, "alaska_full_original_field")
    expected = ["Bronson, Dave", "DeWitt, Meda", "Kreiss-Tomkins, Jonathan S.",
                "Parkin, James W.", "Taylor, Treg", "Wilson, Bernadette M."]
    need(all(n in r and "(Certified)" in r for n, r in zip(expected, found)),
         "alaska_certified_names")
    need("Registered Democrat / Nonpartisan" in found[2], "alaska_target_party_ticket")
    return found


def texas(page0, page1):
    text = " ".join(page1.split())
    need("2026 NOVEMBER GENERAL ELECTION" in text and "November 03, 2026" in text,
         "texas_general_context")
    need("final ballot certification" in " ".join(page0.split()), "texas_certification")
    section = re.search(r" GOVERNOR (.*?) LIEUTENANT GOVERNOR ", text)
    need(section is not None, "texas_office_boundary")
    need(section[1] == "GREG ABBOTT REP GINA HINOJOSA DEM PAT DIXON LIB", "texas_field")
    return [{"name": "GREG ABBOTT", "party": "REP"},
            {"name": "GINA HINOJOSA", "party": "DEM"},
            {"name": "PAT DIXON", "party": "LIB"}]


def wisconsin(text):
    text = " ".join(text.replace("\ufffd", " ").split())
    need("November 03, 2026" in text and "Official Ballot" in text, "wisconsin_context")
    need(re.search(r"Governor\s*/\s*Lieutenant Governor Vote for 1 Tom Tiffany\s*/\s*"
                   r"David Varnam\s*\(Republican\) David Crowley\s*/\s*"
                   r"Sarah Godlewski\s*\(Democratic\)", text) is not None,
         "wisconsin_ticket_field")
    need("write-in (Governor)" in text and "write-in (Lieutenant Governor)" in text,
         "wisconsin_write_in_not_omitted")
    return [{"name": "Tom Tiffany / David Varnam", "party": "Republican"},
            {"name": "David Crowley / Sarah Godlewski", "party": "Democratic"}]


def stress(p, ask):
    p, ask = D(p), D(ask)
    need(D(0) <= p <= 1 and D(0) < ask < 1, "stress_domain")
    result = []
    for e in map(D, (".03", ".10", ".20")):
        value = (1 - e) * p
        before_tick = value - D(".01")
        ceiling = (before_tick / D(".005")).to_integral_value(rounding=ROUND_FLOOR) * D(".005")
        result.append({"exceptionAllowanceAssumption": str(e), "exceptionPayoutAssumption": "0",
                       "assumedPayoutPerShare": str(value), "margin": "0.01", "tick": "0.005",
                       "tickFlooredResearchCeiling": str(ceiling),
                       "ceilingMinusHistoricalAsk": str(ceiling - ask),
                       "passesHistoricalPriceArithmeticOnly": ceiling >= ask,
                       "calibratedProbabilityBound": False})
    return result


def run(private, official, public):
    private, official, public = map(Path, (private, official, public))
    base = Path(__file__).resolve().parents[2]
    dossier = json.loads((Path(__file__).parent / "dossier.json").read_bytes())
    pin_bytes = (private / "source-pins.json").read_bytes()
    need(sha256(pin_bytes).hexdigest() == dossier["privateOriginalManifestSha256"], "capture_manifest")
    pins = json.loads(pin_bytes)["sources"]
    for pin in pins:
        if "sha256" in pin:
            check((private / pin["file"]).read_bytes(), pin)
        check((private / (pin["file"] + ".headers")).read_bytes(),
              {"sha256": pin["headersSha256"]})
    from pypdf import PdfReader
    tx = PdfReader(private / "tx-official-ballot-certification.pdf")
    wi = PdfReader(private / "wi-official-monroe-general-ballot.pdf")
    extracted = {"WI": wisconsin(wi.pages[0].extract_text()),
                 "TX": texas(tx.pages[0].extract_text(), tx.pages[1].extract_text()),
                 "KS": kansas((private / "ks-official-current-candidates.html").read_bytes()),
                 "AK": alaska((private / "ak-official-general-candidates.html").read_bytes())}
    need(b"Ranked Choice Voting" in (private / "ak-official-election-information.html").read_bytes(),
         "alaska_ranked_choice_original_rule")
    coverage = json.loads((base / "coverage.json").read_bytes())
    module_spec = importlib.util.spec_from_file_location("fixed_original_coverage", base / "analyze.py")
    original = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(original)
    recomputed = original.run(public, official)
    need(len(dossier["candidates"]) == len(SPECS), "finite_five_candidates")
    for record, (office, state, market, name, p) in zip(dossier["candidates"], SPECS):
        race = next(x for x in coverage["races"] if x["office"] == office and x["state"] == state)
        original_race = next(x for x in recomputed["races"] if x["office"] == office and x["state"] == state)
        need(race == original_race, "recomputed_original_forecast_and_contracts")
        contract = next(x for x in race["contracts"] if x["marketId"] == market)
        candidate = next(x for x in race["forecast"]["candidates"] if x["name"] == name)
        need(candidate["candidateWinModelPoint"] == p, "original_model_point")
        need(record["state"] == state and record["office"] == office
             and record["marketId"] == market and record["side"] == "YES"
             and record["exchangeId"] == contract["referenceBook"]["exchangeId"]
             and record["contractId"] == contract["contractId"]
             and record["forecastCandidate"] == candidate, "finite_exact_target")
        check((public / race["forecast"]["source"]["file"]).read_bytes(), race["forecast"]["source"])
        node_raw = (official / contract["receipt"]["path"]).read_bytes()
        check(node_raw, contract["receipt"])
        node = json.loads(node_raw)["data"]
        need(node["market_id"] == market and node["root"]["node_id"] == contract["contractId"]
             and node["root"]["contract_id"] == contract["contractId"], "original_exact_ids")
        semantic = sha256(json.dumps(node["root"], sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        need(semantic == contract["semanticRootSha256"] == record["originalContract"]["semanticRootSha256"],
             "original_root_sha")
        check((official / contract["referenceBook"]["artifact"]).read_bytes(),
              contract["referenceBook"], "sourceSha256")
        need(record["stressScenarios"] == stress(p, contract["referenceBook"]["yesAsk"]), "scenario_math")
        if state != "FL":
            need(record["officialNominee"]["safeExtractedField"] == extracted[state], "safe_original_rows")
        else:
            need(record["officialNominee"]["originalGeneralElectionNomineeSourceBodyVerified"] is False
                 and record["reviewStatus"] == "partial_original_nominee_source_missing", "florida_partial")
    return {"candidateCount": 5, "originalNomineeBodiesVerified": 4, "partialNomineeBodies": 1,
            "stressScenariosChecked": 15, "executionAuthority": False, "approval": False}


if __name__ == "__main__":
    print(json.dumps(run(*sys.argv[1:4]), indent=2, sort_keys=True))
