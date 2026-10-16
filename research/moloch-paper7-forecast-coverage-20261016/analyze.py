"""Offline, finite coverage screen. Published model points are not calibrated bounds."""
import datetime as dt
from decimal import Decimal, ROUND_FLOOR
import hashlib
import html
import json
from pathlib import Path
import re
import sys

TID = "bda92870-621e-47b0-bc3c-3602c5c26f55"
ACTIVE = {("senate", x) for x in ["alaska", "delaware", "massachusetts", "north-carolina", "oregon"]} | {("governors", "maine")}
STATES = dict(x.split(":") for x in "alabama:AL alaska:AK arizona:AZ arkansas:AR california:CA colorado:CO connecticut:CT delaware:DE florida:FL georgia:GA hawaii:HI idaho:ID illinois:IL indiana:IN iowa:IA kansas:KS kentucky:KY louisiana:LA maine:ME maryland:MD massachusetts:MA michigan:MI minnesota:MN mississippi:MS missouri:MO montana:MT nebraska:NE nevada:NV new-hampshire:NH new-jersey:NJ new-mexico:NM new-york:NY north-carolina:NC north-dakota:ND ohio:OH oklahoma:OK oregon:OR pennsylvania:PA rhode-island:RI south-carolina:SC south-dakota:SD tennessee:TN texas:TX utah:UT vermont:VT virginia:VA washington:WA west-virginia:WV wisconsin:WI wyoming:WY".split())
D = Decimal


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def stamp(value):
    result = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if result.tzinfo is None:
        raise ValueError("clock_timezone")
    return result


def text(raw):
    return re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]*>", " ", raw))).strip()


def number(value):
    return format(value, "f")


def pinned(folder, entry):
    rel = Path(entry.get("file", entry.get("path", "")))
    if rel.is_absolute() or ".." in rel.parts:
        raise ValueError("source_path")
    raw = (folder / rel).read_bytes()
    if len(raw) > 4_000_000 or len(raw) != entry["bytes"] or sha(raw) != entry["sha256"]:
        raise ValueError("source_hash_or_bytes")
    return raw


def parse_forecast(raw, receipt):
    b = raw.decode("utf-8")
    articles = []
    for match in re.finditer(r'<script\b([^>]*)>(.*?)</script>', b, re.S):
        if re.search(r'type="application/ld\+json"', match[1]):
            obj = json.loads(match[2])
            if obj.get("@type") == "AnalysisNewsArticle":
                articles.append(obj)
    if len(articles) != 1:
        raise ValueError("published_article_identity")
    article = articles[0]
    if article["url"] != receipt["url"] or stamp(article["dateModified"]) > stamp(receipt["receivedAt"]):
        raise ValueError("forecast_scope_or_clock")
    clean = re.sub(r"<script\b[^>]*>.*?</script>", "", b, flags=re.S)
    cards = []
    for side, row in re.findall(r'<div class="mn-card-row" data-side="([a-z])">(.*?)</div>', clean, re.S):
        if 'class="mn-card-cand"' not in row:
            continue
        name = text(row.split('<span class="mn-card-chance', 1)[0])
        name = re.sub(r"\s*\(\s*[A-Z]\s*\)\s*$", "", name).replace(" incumbent", "").replace("*", "")
        name = re.sub(r"\s+", " ", name).strip()
        chance = re.search(r'<span class="mn-card-chance[^>]*>(.*?)</span>', row, re.S)
        if not chance:
            raise ValueError("candidate_chance_missing")
        small = re.search(r'<small[^>]*>(.*?)</small>', chance[1], re.S)
        pct = re.fullmatch(r"([<>])?(\d+(?:\.\d+)?)%", text(small[1])) if small else None
        point, upper, lower = None, None, None
        if pct:
            value = D(pct[2]) / 100
            if not 0 <= value <= 1:
                raise ValueError("candidate_probability_bounds")
            if pct[1] == "<":
                upper = number(value)
            elif pct[1] == ">":
                lower = number(value)
            else:
                point = number(value)
        cards.append({"name": name, "reportedParty": side.upper(), "candidateWinModelPoint": point,
                      "strictUpperIfCensored": upper, "strictLowerIfCensored": lower, "unpublishedProbability": pct is None})
    if not 1 <= len(cards) <= 12 or len({x["name"] for x in cards}) != len(cards):
        raise ValueError("candidate_card_count")
    # This is the forecast's displayed candidate win table, never vote shares or market comparisons.
    if sum(D(x["candidateWinModelPoint"] or "0") for x in cards) > D("1.003"):
        raise ValueError("candidate_mass")
    return {"publisher": "Call the Map", "url": receipt["url"], "publishedDateModified": article["dateModified"],
            "firstAvailabilityAtThisCaptureOnly": receipt["receivedAt"], "candidates": cards,
            "source": receipt, "terminalElectionDate": "2026-11-03", "calibratedProbabilityBound": False}


def scope_ok(data):
    return any(c.get("type") == "tournament" and c.get("tournament", {}).get("id") == TID for c in data.get("contexts", []))


def parse_node(receipt, item):
    if receipt["status"] != 200 or any(receipt[k] != item[k] for k in ["endpoint", "requestStartedAt", "receivedAt", "status"]):
        raise ValueError("receipt_manifest_identity")
    data = receipt["data"]
    root = data.get("root", {})
    details = root.get("contract_details", {})
    if not details.get("raceName", "").startswith(("U.S. Senate ", "Governor of ")):
        return None
    if not scope_ok(data) or data.get("market_id") != item["marketId"]:
        raise ValueError("node_scope")
    if receipt["endpoint"] != f"/markets/{item['marketId']}/nodes?tournamentId={TID}" or stamp(receipt["requestStartedAt"]) > stamp(receipt["receivedAt"]):
        raise ValueError("node_endpoint_or_clock")
    if root.get("node_type") != "contract" or not root.get("contract_id") or root.get("contract_id") != root.get("node_id"):
        raise ValueError("node_identity")
    if details.get("resolutionType") != "Party Winner" or details.get("raceStage") != "General" or details.get("electionDate") != "2026-11-03":
        raise ValueError("node_semantics")
    office = "senate" if details["raceName"].startswith("U.S. Senate ") else "governors"
    state_name = next((s.replace("-", " ").title() for s, code in STATES.items() if code == details.get("usState")), None)
    if details["raceName"] != ("U.S. Senate " if office == "senate" else "Governor of ") + str(state_name):
        raise ValueError("node_state_race_identity")
    expected_level = "Federal" if office == "senate" else "State"
    if details.get("officeLevel") != expected_level or details.get("contractType") != "Election Outcome":
        raise ValueError("node_office")
    return {"marketId": data["market_id"], "contractId": root["contract_id"], "office": office,
            "state": details["usState"], "party": details["winnerName"], "predicate": details,
            "semanticRootSha256": sha(json.dumps(root, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()),
            "scheduledSettlementAt": root.get("settlement_date"), "settledWith": root.get("settled_with"),
            "receipt": {k: item[k] for k in ["path", "sha256", "bytes", "endpoint", "requestStartedAt", "receivedAt"]}}


def reference_book(receipt, item, market):
    if receipt["status"] != 200 or not scope_ok(receipt["data"]):
        raise ValueError("book_scope")
    if any(receipt[k] != item[k] for k in ["endpoint", "requestStartedAt", "receivedAt", "status"]):
        raise ValueError("book_receipt_identity")
    expected = f"/markets/{market}/orderbook?tournamentId={TID}&depth=20"
    if receipt["endpoint"] != expected or stamp(receipt["requestStartedAt"]) > stamp(receipt["receivedAt"]):
        raise ValueError("book_endpoint_or_clock")
    scoped = [c for c in receipt["data"]["contexts"] if c.get("type") == "tournament" and c.get("tournament", {}).get("id") == TID]
    if len(scoped) != 1:
        raise ValueError("book_context_count")
    exchanges = scoped[0]["orderbook"]["exchanges"]
    if exchanges != receipt["data"].get("exchanges"):
        raise ValueError("book_context_mismatch")
    if len(exchanges) != 1 or exchanges[0].get("option") != "YES":
        raise ValueError("ambiguous_exchange")
    b = exchanges[0]
    if stamp(b["asOf"]["at"]) > stamp(receipt["receivedAt"]) or int(b["asOf"]["sequence"]) <= 0:
        raise ValueError("book_original_clock")
    levels = {}
    for side in ["bids", "asks"]:
        entries = [(D(str(x["price"])), D(str(x["quantity"]))) for x in b[side]]
        if not entries or any(not (0 < p < 1 and p % D(".005") == 0 and q > 0) for p, q in entries):
            raise ValueError("book_levels")
        if entries != sorted(entries, reverse=side == "bids") or len(set(p for p, _ in entries)) != len(entries):
            raise ValueError("book_order")
        levels[side] = entries[0]
    bid, ask = levels["bids"][0], levels["asks"][0]
    if bid >= ask:
        raise ValueError("book_crossed")
    return {"exchangeId": str(b["exchangeId"]), "originalAsOf": b["asOf"],
            "requestStartedAt": receipt["requestStartedAt"], "receivedAt": receipt["receivedAt"],
            "sourceSha256": item["sha256"], "artifact": item["path"],
            "yesAsk": number(ask), "yesAskDepth": number(levels["asks"][1]),
            "noAskComplement": number(1-bid), "noAskDepth": number(levels["bids"][1]),
            "currentExecutable": False, "fullProviderContinuityVerified": False}


def proposals(forecast, node, book):
    party = {"Democratic Party": "D", "Republican Party": "R"}.get(node["party"])
    if not party:
        return []
    result = []
    for side in ["YES", "NO"]:
        selected = [x for x in forecast["candidates"] if x["candidateWinModelPoint"] is not None
                    and ((x["reportedParty"] == party) == (side == "YES")) and x["name"] != "Other"]
        if not selected:
            continue
        p = sum(D(x["candidateWinModelPoint"]) for x in selected)
        ceiling = max(D(0), ((D(".97")*p-D(".01"))/D(".005")).to_integral_value(rounding=ROUND_FLOOR)*D(".005"))
        price = D(book["yesAsk"] if side == "YES" else book["noAskComplement"]) if book else None
        result.append({"side": side, "supportingCandidateNames": [x["name"] for x in selected],
                       "ordinaryCandidateSubsetModelPoint": number(p),
                       "ordinaryRelation": "named candidate(s) win under their reported party implies this Party Winner side only under the ordinary-party correspondence assumption",
                       "exceptionAllowanceAssumption": "0.03", "exceptionPayoutFloorAssumption": "0", "marginAssumption": "0.01",
                       "tickFlooredResearchCeiling": number(ceiling), "referenceAsk": number(price) if price is not None else None,
                       "referenceTopDepth": book["yesAskDepth" if side == "YES" else "noAskDepth"] if book else None,
                       "ceilingMinusReferenceAsk": number(ceiling-price) if price is not None else None,
                       "passesHistoricalPriceArithmeticOnly": price is not None and price <= ceiling})
    return result


def run(public_dir, official_dir):
    public_dir, official_dir = Path(public_dir), Path(official_dir)
    source_raw = (public_dir/"source-pins.json").read_bytes()
    sources = json.loads(source_raw)["sources"]
    sig_raw = (official_dir/"manifest.json").read_bytes()
    sig = json.loads(sig_raw)
    nodes, books, rejected = [], {}, []
    for item in sig["files"]:
        raw = pinned(official_dir, item)
        if stamp(item["receivedAt"]) > stamp(sig["extractedAt"]):
            raise ValueError("receipt_after_frozen_export")
        receipt = json.loads(raw)
        if item["kind"] == "nodes":
            node = parse_node(receipt, item)
            if node:
                nodes.append(node)
        elif item["kind"] == "orderbook":
            try:
                books[item["marketId"]] = reference_book(receipt, item, item["marketId"])
            except (ValueError, KeyError) as err:
                rejected.append({"marketId": item["marketId"], "sourceSha256": item["sha256"], "reason": str(err)})
    races, leads = [], []
    for pin in sources:
        raw = pinned(public_dir, pin) if pin.get("status") == 200 else None
        match = re.fullmatch(r"https://www.callthemap.com/(senate|governors)/([a-z-]+)", pin["url"])
        if not match:
            continue
        office, slug = match.groups()
        state = STATES[slug]
        forecast = parse_forecast(raw, pin) if raw else None
        matches = [n for n in nodes if n["state"] == state and n["office"] == office]
        race = {"office": office, "state": state, "slug": slug, "alreadyActiveRaceAtAssignment": (office,slug) in ACTIVE,
                "forecast": forecast, "contracts": [],
                "gaps": ["official_candidate_party_ballot_independently_verify_before_review", "resolver_exception_probabilities_not_estimated"]}
        if not matches:
            race["gaps"].append("original_structured_SIG_node_not_in_retained_bundle")
        for node in matches:
            book = books.get(node["marketId"])
            entry = {**node, "referenceBook": book, "researchSides": proposals(forecast,node,book) if forecast else [],
                     "mappingStatus": "exact_structured_party_predicate_candidate_correspondence_still_assumed"}
            if book:
                entry["referenceBook"]["ageAtOriginalBundleExtractionMs"] = int((stamp(sig["extractedAt"])-stamp(book["originalAsOf"]["at"])).total_seconds()*1000)
            else:
                entry["gaps"] = ["original_exchange_id_and_book_not_in_retained_book_bundle"]
            race["contracts"].append(entry)
            for proposal in entry["researchSides"]:
                if proposal["passesHistoricalPriceArithmeticOnly"] and not race["alreadyActiveRaceAtAssignment"]:
                    leads.append({"office": office, "state": state, "marketId": node["marketId"], "contractId": node["contractId"],
                                  "exchangeId": book["exchangeId"], "party": node["party"], **proposal,
                                  "forecastUrl": forecast["url"], "forecastPublishedAt": forecast["publishedDateModified"],
                                  "forecastSourceSha256": pin["sha256"], "nodeSourceSha256": node["receipt"]["sha256"],
                                  "referenceBookSourceSha256": book["sourceSha256"], "referenceBookAt": book["originalAsOf"]["at"],
                                  "referenceBookReceivedAt": book["receivedAt"], "authority": False})
        races.append(race)
    if len(races) != 71 or sum(r["office"]=="senate" for r in races) != 35:
        raise ValueError("finite_screen_coverage")
    leads.sort(key=lambda x: (-D(x["ceilingMinusReferenceAsk"]),x["marketId"],x["side"]))
    return {"schemaVersion": 1, "kind": "forecast_contract_coverage_not_execution_or_calibration", "executionAuthority": False,
            "accountRequests": 0, "allocationAuthority": False, "asOfSourceCaptureEnd": max(x["receivedAt"] for x in sources),
            "publicSourceManifestSha256": sha(source_raw), "officialReceiptManifestSha256": sha(sig_raw),
            "officialBundleExtractedAt": sig["extractedAt"], "officialReceiptCount": len(sig["files"]),
            "summary": {"publishedRacePages": len(races), "senate": 35, "governors": 36,
                        "racesWithExactNodes": sum(bool(r["contracts"]) for r in races), "structuredPartyContracts": len(nodes),
                        "additionalRacesWithNodes": sum(bool(r["contracts"]) and not r["alreadyActiveRaceAtAssignment"] for r in races),
                        "historicalPriceLeadsBeyondSixActiveRaces": len(leads),
                        "distinctLeadRacesBeyondSix": len({(x["office"],x["state"]) for x in leads})},
            "publishedSources": sources, "races": races, "historicalReferenceLeads": leads, "unusableReferenceBooks": rejected,
            "limits": ["No future fills, realized PnL, calibrated probability bound, independent model average or deployment approval.",
                       "The 3% exception mass, zero exceptional floor and 1% margin are explicitly illustrative owner-style stress assumptions, not measured risks.",
                       "Forecast candidates/party labels require independent official current ballot correspondence before a review. Structured Party Winner nodes do not supply full fusion/refund/replacement prose.",
                       "Original source clocks are unchanged. Historical reference books require full current sole-worker physical checks; no current executable quote or causal backtest is claimed.",
                       "Other candidate vote share is not win probability; unpublished and <0.1% values are not imputed. Candidate subset points are not 1-minus-opponent party identities.",
                       "CTM race probabilities share a model and correlated national/state errors. More qualifying rows do not imply independent diversification.",
                       "No raw candidate personal files, account holdings/orders or auth material are published."]}


if __name__ == "__main__":
    result = run(*sys.argv[1:3])
    print(json.dumps(result, indent=2, sort_keys=True))
