// live-paper/reference-join.mjs — Cup leads vs an external reference table.
use baal_paper::fixed::big_amount;
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::js_truthy;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::BTreeSet;

/// String(v) for the scalar coercions JS performs before amount().
fn js_str(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => n.to_string(),
        V::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}
fn party(p: &V) -> Option<&'static str> {
    match p.as_str() {
        Some("Democratic Party") => Some("Democratic"),
        Some("Republican Party") => Some("Republican"),
        Some("Independent Party") => Some("Independent"),
        _ => None,
    }
}
fn truthy(v: &V) -> bool { js_truthy(Some(v)) }
/// /^\d\d$/ style tail matcher for " District (\d+|At Large|AL)".
fn house_district(name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix("U.S. House ")?;
    let (middle, tail) = rest.rsplit_once(" District ")?;
    if middle.is_empty() { return None; }
    let ok = tail == "At Large" || tail == "AL" || (!tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()));
    if !ok { return None; }
    Some((middle.to_string(), tail.to_string()))
}
fn election_date_ok(v: &V) -> bool {
    let s = v.as_str().unwrap_or("");
    let b = s.as_bytes();
    b.len() == 10 && &b[0..5] == b"2026-" && b[7] == b'-'
        && b[5].is_ascii_digit() && b[6].is_ascii_digit() && b[8].is_ascii_digit() && b[9].is_ascii_digit()
}

/// identityCandidate(cup, external)
pub fn identity_candidate(cup: &V, external: &V) -> bool {
    let predicate = &cup["settlementPredicate"];
    let mapped = match party(&cup["party"]) { Some(p) => p, None => return false };
    if cup["option"].as_str() != Some("YES")
        || !truthy(&cup["race"]) || !truthy(&cup["stage"])
        || cup["stage"]["name"].as_str() != Some("General")
        || predicate["resolutionType"].as_str() != Some("Party Winner")
        || predicate["contractType"].as_str() != Some("Election Outcome")
        || !election_date_ok(&predicate["electionDate"])
        || external["election_year"].as_f64() != Some(2026.0)
        || external["party_rule_verified"] != V::Bool(true)
        || external["party"].as_str() != Some(mapped)
        || external["state_code"] != cup["race"]["usState"]
        || !external["state_name"].as_str().is_some_and(|s| !s.is_empty())
    {
        return false;
    }
    let name = cup["race"]["name"].as_str().unwrap_or("");
    let state = external["state_name"].as_str().unwrap_or("");
    let district_null = external.get("district") == Some(&V::Null);
    if cup["race"]["officeLevel"].as_str() == Some("Federal") && external["office"].as_str() == Some("Senate") {
        return name == format!("U.S. Senate {state}") && district_null;
    }
    if cup["race"]["officeLevel"].as_str() == Some("State") && external["office"].as_str() == Some("Governor") {
        return name == format!("Governor of {state}") && district_null;
    }
    if cup["race"]["officeLevel"].as_str() == Some("Federal") && external["office"].as_str() == Some("House") {
        let Some((mid, tail)) = house_district(name) else { return false };
        if mid != state { return false; }
        let district = if tail == "At Large" || tail == "AL" { "AL".to_string() }
            else { tail.parse::<u64>().map(|x| x.to_string()).unwrap_or_else(|_| tail.parse::<f64>().map(|x| x.to_string()).unwrap_or(tail.clone())) };
        return json!(district) == external["district"];
    }
    false
}
fn diff(a: &V, b: &V) -> Result<V, String> {
    if a.is_null() || b.is_null() { return Ok(V::Null); }
    let x = big_amount(&js_str(a))?;
    let y = big_amount(&js_str(b))?;
    if x < BigInt::from(0) || x > BigInt::from(1_000_000) || y < BigInt::from(0) || y > BigInt::from(1_000_000) {
        return Err("Invalid reference price".into());
    }
    Ok(json!((x - y).to_string().parse::<f64>().unwrap_or(f64::NAN) / 1e6))
}
fn finite_num(v: &V) -> bool {
    match v { V::Number(n) => n.as_f64().is_some_and(|x| x.is_finite()),
        V::String(s) => s.trim().parse::<f64>().is_ok_and(|x| x.is_finite()), _ => false }
}
fn f64_of(v: &V) -> Option<f64> {
    match v { V::Number(n) => n.as_f64(), V::String(s) => s.trim().parse::<f64>().ok(), _ => None }
}

/// joinReference(cupData, reference)
pub fn join_reference(cup_data: &V, reference: &V) -> Result<V, String> {
    let cup_exchanges = cup_data["exchanges"].as_array().cloned().unwrap_or_default();
    let markets = reference["markets"].as_array().cloned().unwrap_or_default();
    if !cup_data["exchanges"].is_array() || !reference["markets"].is_array() {
        return Err("Invalid reference tables".into());
    }
    let mut tickers: BTreeSet<String> = BTreeSet::new();
    for r in &markets {
        if r["ticker"].as_str().is_none() || !tickers.insert(r["ticker"].as_str().unwrap().to_string()) {
            return Err("Duplicate external contract".into());
        }
    }
    let mut unmatched = Vec::new();
    let mut candidates = Vec::new();
    for cup in &cup_exchanges {
        let matches: Vec<&V> = markets.iter().filter(|r| identity_candidate(cup, r)).collect();
        if matches.is_empty() {
            unmatched.push(json!({"marketId": cup["marketId"], "exchangeId": cup["exchangeId"],
                "title": cup["title"],
                "reason": if truthy(&cup["race"]) { "no_eligible_structured_identity_match" } else { "structured_metadata_unavailable" }}));
            continue;
        }
        for r in matches.iter() {
            let buy_gap = diff(&r["yes_bid_dollars"], &cup["bestAsk"])?;
            let sell_gap = diff(&cup["bestBid"], &r["yes_ask_dollars"])?;
            let sizes = [&r["yes_bid_size_fp"], &r["yes_ask_size_fp"]];
            let mut flags = vec![json!("full_settlement_equivalence_unverified"),
                json!("cup_special_status_not_explicit"), json!("source_quote_age_unknown"),
                json!("mixed_time_observations"), json!("cup_depth_unavailable")];
            if r["special_election"] == V::Bool(true) { flags.push(json!("external_special_election_requires_review")); }
            if !r["special_election"].is_boolean() { flags.push(json!("external_special_status_unknown")); }
            if matches.len() > 1 { flags.push(json!("multiple_external_contracts_retained")); }
            if sizes.iter().any(|x| x.is_null() || !finite_num(x) || f64_of(x).unwrap_or(f64::NAN) <= 0.0) {
                flags.push(json!("external_zero_or_unknown_top_size"));
            }
            if truthy(&cup["crossed"]) { flags.push(json!("cup_crossed_bulk_quote")); }
            let receipt_skew = if truthy(&cup["receivedAt"]) && truthy(&r["quote_receipt_utc"]) {
                json!(parse_iso_ms(r["quote_receipt_utc"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
                    - parse_iso_ms(cup["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN))
            } else { V::Null };
            let largest = match (buy_gap.is_null(), sell_gap.is_null()) {
                (true, _) => sell_gap.clone(),
                (false, true) => buy_gap.clone(),
                (false, false) => json!(buy_gap.as_f64().unwrap_or(f64::NAN).max(sell_gap.as_f64().unwrap_or(f64::NAN))),
            };
            candidates.push(json!({
                "marketId": cup["marketId"], "exchangeId": cup["exchangeId"], "cupTitle": cup["title"],
                "cupParty": cup["party"], "cupRace": cup["race"], "cupStage": cup["stage"],
                "cupPredicate": cup["settlementPredicate"], "cupPredicateVerified": false,
                "cupSpecialElectionStatus": "not_provided",
                "ticker": r["ticker"], "eventTicker": r["event_ticker"], "externalRaceKey": r["race_key"],
                "externalOffice": r["office"], "externalParty": r["party"],
                "externalSpecialElection": r["special_election"],
                "cupBestBid": cup["bestBid"], "cupBestAsk": cup["bestAsk"],
                "externalYesBid": r["yes_bid_dollars"], "externalYesAsk": r["yes_ask_dollars"],
                "externalYesBidSize": r["yes_bid_size_fp"], "externalYesAskSize": r["yes_ask_size_fp"],
                "cupReceivedAt": cup["receivedAt"], "cupRequestStartedAt": cup["requestStartedAt"],
                "externalReceivedAt": r["quote_receipt_utc"], "externalRequestStartedAt": r["request_sent_utc"],
                "receiptSkewMs": receipt_skew,
                "externalBidMinusCupAsk": buy_gap, "cupBidMinusExternalAsk": sell_gap,
                "largestPriceGap": largest,
                "classification": if r["special_election"] == V::Bool(true) { "special_election_ambiguity" } else { "identity_candidate_rules_unverified" },
                "externalRulesPrimary": r["rules_primary"], "externalRulesSecondary": r["rules_secondary"],
                "externalTermsUrl": r["contract_terms_url"], "externalSourceUrl": r["source_url"],
                "flags": flags, "executable": false, "probabilityInputReady": false,
            }));
        }
    }
    candidates.sort_by(|a: &V, b: &V| {
        let ga = a["largestPriceGap"].as_f64().unwrap_or(f64::NEG_INFINITY);
        let gb = b["largestPriceGap"].as_f64().unwrap_or(f64::NEG_INFINITY);
        gb.partial_cmp(&ga).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| js_str(&a["ticker"]).cmp(&js_str(&b["ticker"])))
            .then_with(|| js_str(&a["marketId"]).cmp(&js_str(&b["marketId"])))
    });
    let matched: BTreeSet<String> = candidates.iter()
        .map(|x| baal_service_runtime::json_text::js_stringify(&x["exchangeId"])).collect();
    let mut interpretation = Map::new();
    interpretation.insert("networkRequests".into(), json!(0));
    interpretation.insert("competitionOrders".into(), json!(0));
    interpretation.insert("existingWalletsChanged".into(), json!(false));
    interpretation.insert("profitabilityEstablished".into(), json!(false));
    interpretation.insert("ruleEquivalenceEstablished".into(), json!(false));
    interpretation.insert("ranking".into(), json!("Descending largest cross-venue observed price difference before costs. Does not imply profit, probability, or executable arbitrage."));
    interpretation.insert("identity".into(), json!("Only provided structured Cup state/race/office/General stage/2026 election date/party versus verified external generic-party record. House district parsed only from explicit structured raceName, never opaque IDs or Cup title."));
    interpretation.insert("limitations".into(), json!([
        "All predicates and special-election equivalence need independent review",
        "Cup quotes and identities are historical; neither venue source quote age is known",
        "Receipt clocks are retained separately; observations are not synchronized",
        "Distinct external contracts remain distinct; no duplicate collapse or party complement assumption",
        "Price differences exclude fees, quantity/depth, cross-venue execution, settlement timing and correlated risk",
        "No record is suitable for a paper probability cohort until source-backed estimates and exact reviewed predicates are supplied",
    ]));
    Ok(json!({
        "schemaVersion": 1, "kind": "cross_venue_stale_identity_leads_only",
        "cupAsOf": cup_data["asOf"],
        "externalReceiptStart": reference["metadata"].get("receipt_start_utc").cloned().unwrap_or(V::Null),
        "externalReceiptEnd": reference["metadata"].get("receipt_end_utc").cloned().unwrap_or(V::Null),
        "cupExchanges": cup_exchanges.len(), "matchedCupExchanges": matched.len(),
        "candidateContracts": candidates.len(), "unmatched": unmatched,
        "candidates": candidates, "interpretation": V::Object(interpretation),
    }))
}
