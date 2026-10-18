"""Read-only original-contract/reference join for wider campaign research.

External bids/asks are price observations, not calibrated outcome probabilities
or SIG executable liquidity. Numeric market/contract/exchange IDs are distinct.
Every accepted join remains conditional on the two venues' different resolvers.
"""
import datetime
import hashlib
import json
import re
from decimal import Decimal, InvalidOperation


def need(ok, reason):
    if not ok:
        raise ValueError(reason)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=False, allow_nan=False).encode()


def clock(value):
    need(isinstance(value, str), "REFERENCE_CLOCK")
    d = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
    need(d.utcoffset() is not None, "REFERENCE_TIMEZONE")
    return int(d.timestamp() * 1000)


def official_identity(row, raw):
    need(type(raw) is bytes and sha(raw) == row["semanticSourceSha256"], "OFFICIAL_RAW_CHANGED")
    receipt = json.loads(raw)
    root = receipt["data"]["root"]
    need(receipt["status"] == 200 and receipt["data"]["market_id"] == row["marketId"]
         and root["node_type"] == "contract" and root["node_id"] == row["contractId"]
         and root["contract_id"] == row["contractId"]
         and sha(canonical(root)) == row["officialContractRootSha256"], "OFFICIAL_IDENTITY_CHANGED")
    d = root["contract_details"]
    need(d["resolutionType"] == "Party Winner" and d["raceStage"] == "General"
         and d["electionDate"] == "2026-11-03"
         and d["winnerName"] in ["Republican Party", "Democratic Party"], "UNSUPPORTED_PARTY_EVENT")
    name, state, district = d["raceName"], d["usState"], None
    if name.startswith("U.S. House "):
        m = re.fullmatch(r"U\.S\. House .+ District ([1-9][0-9]*)", name)
        need(m is not None and d["officeLevel"] == "Federal", "HOUSE_DISTRICT_REQUIRED")
        office, district = "House", m[1]
    elif name.startswith("U.S. Senate "):
        need(d["officeLevel"] == "Federal", "OFFICE_LEVEL")
        office = "Senate"
    elif name.startswith("Governor of "):
        need(d["officeLevel"] == "State", "OFFICE_LEVEL")
        office = "Governor"
    else:
        raise ValueError("UNSUPPORTED_OFFICE")
    need(re.fullmatch(r"[A-Z]{2}", state) is not None, "STATE_IDENTITY")
    return (office, state, district, d["winnerName"].removesuffix(" Party"), 2026)


def source(raw, path):
    need(type(raw) is bytes, "ORIGINAL_REFERENCE_REQUIRED")
    r = json.loads(raw)
    need(r.get("source") == "kalshi_official_v2" and r.get("path") == path
         and r.get("status") == 200, "REFERENCE_ENDPOINT")
    text = r.get("bodyText")
    need(type(text) is str and sha(text.encode()) == r.get("bodySha256"), "REFERENCE_BODY_CHANGED")
    body = json.loads(text)
    need(body == r.get("body"), "REFERENCE_PARSED_BODY_CHANGED")
    return r, body


def best_bid(levels):
    need(type(levels) is list, "BOOK_LEVELS")
    prices = []
    for level in levels:
        need(type(level) is list and len(level) == 2
             and all(type(x) is str for x in level), "BOOK_DECIMAL_STRINGS_REQUIRED")
        try:
            p, q = map(Decimal, level)
        except InvalidOperation as error:
            raise ValueError("BOOK_DECIMAL") from error
        need(p.is_finite() and q.is_finite() and 0 <= p <= 1 and q >= 0, "BOOK_DECIMAL")
        if q > 0:
            prices.append(p)
    need(prices, "EMPTY_BOOK_SIDE")
    return max(prices)


def observe(row, originals, inventory, quotes, metadata, *, decision_ms,
            maximum_receipt_age_ms, maximum_metadata_age_ms):
    """Return a conditional original-backed price interval or an exact failure.

    Inventory identity is a supplied mapping claim. Original metadata/rules and
    its raw pin remain visible; this function does not certify settlement parity.
    It ignores legacy sigMarketIds and never complements another party's price.
    """
    need(type(decision_ms) is int and type(maximum_receipt_age_ms) is int
         and type(maximum_metadata_age_ms) is int
         and min(maximum_receipt_age_ms, maximum_metadata_age_ms) > 0, "REFERENCE_TIME_LIMITS")
    identity = official_identity(row, originals.get(row["semanticSourceSha256"]))
    matches = [x for x in inventory if
               (x.get("office"), x.get("state"), x.get("district"),
                x.get("party"), x.get("electionYear")) == identity]
    need(len(matches) == 1, "REFERENCE_IDENTITY_MISSING_OR_AMBIGUOUS")
    mapping = matches[0]
    ticker = mapping["ticker"]
    need(re.fullmatch(r"[A-Z0-9-]{1,100}", ticker) is not None, "REFERENCE_TICKER")
    qraw, mraw = quotes.get(ticker), metadata.get(ticker)
    quote, body = source(qraw, f"/trade-api/v2/markets/{ticker}/orderbook?depth=20")
    meta, document = source(mraw, f"/trade-api/v2/markets/{ticker}")
    need(quote.get("ticker") == ticker and document.get("market", {}).get("ticker") == ticker,
         "REFERENCE_TICKER_CHANGED")
    qstart, qend = clock(quote["requestStartedAt"]), clock(quote["receivedAt"])
    mstart, mend = clock(meta["requestStartedAt"]), clock(meta["receivedAt"])
    need(qstart <= qend <= decision_ms and mstart <= mend <= decision_ms,
         "REFERENCE_FUTURE_OR_REGRESSED")
    need(decision_ms-qstart <= maximum_receipt_age_ms, "REFERENCE_QUOTE_STALE")
    need(decision_ms-mstart <= maximum_metadata_age_ms, "REFERENCE_METADATA_STALE")
    market = document["market"]
    need(market.get("status") == "active" and market.get("result") in [None, ""], "REFERENCE_NOT_ACTIVE")
    rules = market.get("rules_primary")
    need(type(rules) is str and rules.strip(), "REFERENCE_RULES_REQUIRED")
    need(re.search(r"\b" + re.escape(identity[3].lower()) + r" party\b", rules.lower())
         is not None, "REFERENCE_RULE_PARTY_CONFLICT")
    # Do not infer an election year from the ticker suffix or settlement year.
    # KS '-27' explicitly refers to the 2026 election in its current rules.
    explicit_years = re.findall(r"(?:pursuant to the |in the )(20\d\d) election", rules)
    need(not explicit_years or set(explicit_years) == {"2026"}, "REFERENCE_RULE_ELECTION_YEAR_CONFLICT")
    book = body.get("orderbook_fp", {})
    bid = best_bid(book.get("yes_dollars"))
    ask = Decimal(1)-best_bid(book.get("no_dollars"))
    need(bid <= ask, "REFERENCE_CROSSED")
    return dict(row, kalshiTicker=ticker, identity=list(identity),
        referenceRecordSha256=sha(qraw), metadataRecordSha256=sha(mraw),
        mappingClaimSha256=sha(canonical(mapping)),
        requestStartedAt=quote["requestStartedAt"], receivedAt=quote["receivedAt"],
        metadataRequestStartedAt=meta["requestStartedAt"],
        metadataReceivedAt=meta["receivedAt"], yesBid=format(bid, "f"), yesAsk=format(ask, "f"),
        rulesPrimary=rules, explicitRuleElectionYears=explicit_years,
        sourceTimestampAvailable=False, sourceTimestamp=None,
        resolverRelationship="conditional party-event reference; SIG/venue exceptions unverified",
        calibratedProbabilityClaimed=False, pricingAuthority=False, executionAuthority=False)


def census(registry, originals, inventory, quotes, metadata, **clocks):
    need(type(registry) is list and 1 <= len(registry) <= 512, "REGISTRY_BOUND")
    for key in ["exchangeId", "marketId", "contractId"]:
        values = [r[key] for r in registry]
        need(len(values) == len(set(values)), "REGISTRY_DUPLICATE")
    rows, unavailable = [], []
    for r in registry:
        try:
            rows.append(observe(r, originals, inventory, quotes, metadata, **clocks))
        except (ValueError, KeyError, TypeError) as error:
            unavailable.append({"exchangeId":r["exchangeId"], "reason":str(error)})
    return {"rows":rows, "unavailable":unavailable, "executionAuthority":False,
            "calibratedProbabilityClaimed":False, "sigLiquidityClaimed":False,
            "decisionMs":clocks["decision_ms"], "originalRegistrySha256":sha(canonical(registry))}
