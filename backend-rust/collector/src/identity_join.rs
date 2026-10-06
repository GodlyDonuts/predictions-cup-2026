// live-paper/identity-join.mjs — attach cached structured identities to leads.
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Map, Value as V};

/// JSON.stringify drops `undefined` object properties; absent source keys are
/// omitted from the emitted object for byte parity.
fn put_present(o: &mut Map<String, V>, key: &str, src: &V, src_key: &str) {
    if let Some(v) = src.get(src_key) { o.insert(key.into(), v.clone()); }
}
fn digits(s: &V) -> bool {
    // /^\d+$/.test(v): the regex coerces scalars to strings first.
    match s {
        V::String(x) => !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit()),
        V::Number(n) => n.as_f64().is_some_and(|x| x.is_finite() && x.fract() == 0.0 && x >= 0.0),
        _ => false,
    }
}

/// joinCachedIdentities(leads, input)
pub fn join_cached_identities(leads: &V, input: &V) -> Result<V, String> {
    if input["schemaVersion"].as_i64() != Some(1)
        || input["kind"].as_str() != Some("derived_cached_market_identity")
        || input["tournamentId"] != leads["tournamentId"]
        || !input["markets"].is_array()
    {
        return Err("Invalid identity input".into());
    }
    // Map keyed by the raw marketId value; canonical JSON preserves JS `has`/`get`.
    let mut identities: Map<String, V> = Map::new();
    for m in input["markets"].as_array().unwrap() {
        if !digits(&m["marketId"])
            || identities.contains_key(&js_stringify(&m["marketId"]))
            || !m["title"].as_str().is_some_and(|t| !t.is_empty())
            || !m["exchanges"].is_array()
        {
            return Err("Invalid cached identity".into());
        }
        identities.insert(js_stringify(&m["marketId"]), m.clone());
    }
    let mut exchanges = Vec::new();
    for q in leads["exchanges"].as_array().cloned().unwrap_or_default() {
        let m = identities.get(&js_stringify(&q["marketId"]));
        let matched = m.is_some_and(|m| m["exchanges"].as_array().is_some_and(|es| es.iter()
            .any(|e| e["exchangeId"] == q["exchangeId"] && e["option"] == q["option"])));
        let m = match m { Some(m) if matched => m, _ => return Err("Cached market/exchange identity mismatch".into()) };
        let n = &m["node"];
        let has_node = js_truthy_ext(n);
        let mut row = q.as_object().cloned().unwrap_or_default();
        row.insert("title".into(), m["title"].clone());
        let party = has_node && n["resolutionType"].as_str() == Some("Party Winner");
        if party {
            if let Some(w) = n.get("winnerName") { row.insert("party".into(), w.clone()); } else { row.remove("party"); }
        } else {
            row.insert("party".into(), V::Null);
        }
        if has_node {
            let mut race = Map::new();
            put_present(&mut race, "id", n, "raceId");
            put_present(&mut race, "name", n, "raceName");
            put_present(&mut race, "usState", n, "usState");
            put_present(&mut race, "officeLevel", n, "officeLevel");
            row.insert("race".into(), V::Object(race));
            let mut stage = Map::new();
            put_present(&mut stage, "id", n, "stageId");
            put_present(&mut stage, "name", n, "raceStage");
            row.insert("stage".into(), V::Object(stage));
            let mut sp = Map::new();
            sp.insert("kind".into(), json!("cached_structured_contract_description"));
            put_present(&mut sp, "contractType", n, "contractType");
            put_present(&mut sp, "resolutionType", n, "resolutionType");
            put_present(&mut sp, "title", n, "title");
            put_present(&mut sp, "electionDate", n, "electionDate");
            put_present(&mut sp, "settlementDate", n, "settlementDate");
            row.insert("settlementPredicate".into(), V::Object(sp));
        } else {
            row.insert("race".into(), V::Null);
            row.insert("stage".into(), V::Null);
            row.insert("settlementPredicate".into(), V::Null);
        }
        row.insert("settlementPredicateVerified".into(), json!(false));
        row.insert("identityMetadataStatus".into(),
            json!(if has_node { "title_and_cached_structured_node" } else { "cached_title_only" }));
        let mut prov = Map::new();
        put_present(&mut prov, "marketDiscoveryCacheSnapshotAt", input, "discoveryCacheSnapshotAt");
        prov.insert("marketDiscoveryRequestAt".into(), V::Null);
        prov.insert("marketDiscoveryReceivedAt".into(), V::Null);
        put_present(&mut prov, "nodeRequestAt", m, "nodeRequestAt");
        put_present(&mut prov, "nodeReceivedAt", m, "nodeReceivedAt");
        row.insert("identityProvenance".into(), V::Object(prov));
        exchanges.push(V::Object(row));
    }
    let mut out = leads.as_object().cloned().unwrap_or_default();
    out.insert("kind".into(), json!("bulk_price_leads_with_cached_identity"));
    out.insert("identityInputKind".into(), input["kind"].clone());
    out.insert("exchanges".into(), json!(exchanges));
    let mut limitations: Vec<V> = leads["limitations"].as_array().cloned().unwrap_or_default()
        .into_iter().filter(|x| !x.as_str().is_some_and(|s| s.starts_with("Identity titles,"))).collect();
    limitations.push(json!("Titles/exchange mappings are from an older cached discovery view. Exact discovery request/receipt times are unavailable; cache snapshot time is recorded separately."));
    limitations.push(json!("Structured node fields have their own historical request/receipt times. Missing nodes remain null; race/party are never inferred from IDs or titles."));
    limitations.push(json!("Cached structured contract descriptions do not independently establish complete current resolution rules; settlementPredicateVerified is false for every row."));
    out.insert("limitations".into(), json!(limitations));
    Ok(V::Object(out))
}

fn js_truthy_ext(v: &V) -> bool {
    match v { V::Null => false, V::Bool(b) => *b, V::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0, V::String(s) => !s.is_empty(), _ => true }
}
