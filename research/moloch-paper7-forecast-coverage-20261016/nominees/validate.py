"""Offline extraction of limited public nominee facts; emits no personal contact fields."""
import hashlib
import html
import importlib.util
import json
from pathlib import Path
import re
import sys
import xml.etree.ElementTree as E
import zipfile

NS = {"s": "http://schemas.openxmlformats.org/spreadsheetml/2006/main"}


def plain(raw):
    raw = re.sub(r"<(script|style)\b[^>]*>.*?</\1>", " ", raw, flags=re.S|re.I)
    return re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", " ", raw))).strip()


def nevada_rows(raw):
    page = raw.decode("utf-8")
    if "2026 General Election" not in plain(page):
        raise ValueError("general_election_context_missing")
    matches = []
    for table in re.findall(r"<table\b[^>]*>(.*?)</table>", page, re.S | re.I):
        cells = [plain(x) for x in re.findall(r"<t[dh]\b[^>]*>(.*?)</t[dh]>", table, re.S | re.I)]
        if not cells or not re.match(r"^GOVERNOR(?:\s|$)", cells[0]):
            continue
        rows = []
        for row in re.findall(r"<tr\b[^>]*>(.*?)</tr>", table, re.S | re.I):
            pair = [plain(x) for x in re.findall(r"<td\b[^>]*>(.*?)</td>", row, re.S | re.I)]
            if len(pair) == 2 and pair[1].upper() in {"REP", "DEM", "NPP", "--", ""}:
                rows.append({"nameOnBallot": pair[0].upper(), "partyOnBallot": pair[1].upper()})
        if any(x["nameOnBallot"] == "LOMBARDO, JOSEPH" for x in rows):
            matches.append(rows)
    if len(matches) != 1 or [x for x in matches[0] if x["nameOnBallot"] == "LOMBARDO, JOSEPH"] != [{"nameOnBallot":"LOMBARDO, JOSEPH","partyOnBallot":"REP"}]:
        raise ValueError("unique_republican_governor_not_verified")
    return matches[0]


def workbook_rows(path):
    with zipfile.ZipFile(path) as z:
        if sum(x.file_size for x in z.infolist()) > 32_000_000:
            raise ValueError("workbook_bound")
        strings = ["".join(x.itertext()) for x in E.fromstring(z.read("xl/sharedStrings.xml"))]

        def rows(name):
            result = []
            for row in E.fromstring(z.read(name)).findall("s:sheetData/s:row", NS):
                vals = {}
                for c in row:
                    value = c.find("s:v", NS)
                    value = value.text if value is not None else ""
                    if c.get("t") == "s":
                        value = strings[int(value)]
                    elif c.get("t") == "inlineStr":
                        value = "".join(c.itertext())
                    vals[re.sub(r"[0-9]", "", c.get("r"))] = value
                result.append((int(row.get("r")),vals))
            return result

        candidate_rows = rows("xl/worksheets/sheet1.xml")
        header = candidate_rows[0][1]
        if [header.get(k) for k in ["A","C","E"]] != ["Contest","Name On Ballot","Party"]:
            raise ValueError("workbook_original_header")
        selection = rows("xl/worksheets/sheet2.xml")[0][1].get("A")
        governors = [{"row":n,"office":v["A"],"nameOnBallot":v["C"],"partyOnBallot":v["E"]}
                     for n,v in candidate_rows if v.get("A") == "GOVERNOR"]
        scott = [x for x in governors if x["nameOnBallot"] == "PHIL SCOTT"]
        if len(scott) != 1 or scott[0]["partyOnBallot"] != "REPUBLICAN":
            raise ValueError("scott_party_office")
        core = E.fromstring(z.read("docProps/core.xml"))
        modified = core.find("{http://purl.org/dc/terms/}modified")
        return {"governorRows":governors,"selectionCriteria":selection,
                "originalDocumentModified":modified.text if modified is not None else None}


def original_ballot_row_present(raw):
    # A SPA shell and an indexed representation never constitute the rendered original.
    text = plain(raw.decode("utf-8"))
    return bool(re.search(r"11/03/2026\s+STATE GENERAL ELECTION",text,re.I)
                and re.search(r"U\.S\. Senator\s+Michele Tafoya\s+REPUBLICAN",text,re.I))


def run(private_dir, official_dir, public_dir):
    private_dir, official_dir, public_dir = map(Path,[private_dir,official_dir,public_dir])
    source_manifest = (private_dir/"source-pins.json").read_bytes()
    pins = json.loads(source_manifest)["sources"]
    for pin in pins:
        if "sha256" in pin:
            raw = (private_dir/pin["file"]).read_bytes()
            if len(raw) != pin["bytes"] or hashlib.sha256(raw).hexdigest() != pin["sha256"]:
                raise ValueError("original_source_pin")
    nv = [nevada_rows((private_dir/f).read_bytes()) for f in ["nv-douglas-official.html","nv-washoe-official.html"]]
    if nv[0] != [{**x,"partyOnBallot":x["partyOnBallot"] or "--"} for x in nv[1]]:
        raise ValueError("official_county_candidate_conflict")
    vt = workbook_rows(private_dir/"vt-general-candidates.xlsx")
    mn = original_ballot_row_present((private_dir/"mn-official-general-ballot.html").read_bytes())
    if mn:
        raise ValueError("frozen_shell_unexpectedly_changed_requires_new_review")
    base = Path(__file__).resolve().parents[1]
    spec=importlib.util.spec_from_file_location("fixed_coverage",base/"analyze.py")
    coverage_module=importlib.util.module_from_spec(spec);spec.loader.exec_module(coverage_module)
    # Recompute the exact 71-race source/contract linkage, not caller-provided party assertions.
    coverage = coverage_module.run(public_dir,official_dir)
    exact = {}
    for office,state,market,name,p in [("governors","NV","183","Joe Lombardo","0.783"),("senate","MN","361","Michele Tafoya","0.332"),("governors","VT","313","Phil Scott","0.918")]:
        race=next(r for r in coverage["races"] if r["office"]==office and r["state"]==state)
        node=next(n for n in race["contracts"] if n["marketId"]==market)
        candidate=next(c for c in race["forecast"]["candidates"] if c["name"]==name)
        if node["party"]!="Republican Party" or candidate["reportedParty"]!="R" or candidate["candidateWinModelPoint"]!=p:
            raise ValueError("exact_candidate_party_contract")
        exact[state]={"marketId":market,"exchangeId":node["referenceBook"]["exchangeId"],"contractId":node["contractId"],"side":"YES",
                      "predicate":node["predicate"],"rootSha256":node["semanticRootSha256"],"nodeReceipt":node["receipt"],
                      "scheduledSettlementAt":node["scheduledSettlementAt"],"forecastCandidate":candidate,"forecastSource":race["forecast"]["source"],
                      "forecastPublishedDateModified":race["forecast"]["publishedDateModified"]}
    return {"sourceManifestSha256":hashlib.sha256(source_manifest).hexdigest(),"originalSourcePins":pins,
            "nevadaOfficialGovernorRows":nv[0],"vermontOfficialWorkbook":vt,"minnesotaOriginalGeneralBallotRowVerified":mn,
            "exactForecastAndContracts":exact,"executionAuthority":False,"approval":False}


if __name__ == "__main__":
    print(json.dumps(run(*sys.argv[1:4]),indent=2,sort_keys=True))
