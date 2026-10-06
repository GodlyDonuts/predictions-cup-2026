// live-paper/basket-inputs.mjs — fingerprinted research pairs + bulk-lead candidates.
use baal_paper::fixed::{big_amount, SCALE, TICK};
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_stringify, js_truthy};
use crate::model::digest;
use indexmap::IndexMap;
use num_bigint::BigInt;
use serde_json::{json, Value as V};

static NULL: V = V::Null;

/// JS `===` for JSON values: scalars by value, objects/arrays never equal
/// (they compare by identity in JS). Missing and null collapse to Null.
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(x), V::Number(y)) => x.as_f64() == y.as_f64(),
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
fn js_str(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => {
            let mut b = ryu_js::Buffer::new();
            b.format_finite(n.as_f64().unwrap_or(f64::NAN)).to_string()
        }
        V::Bool(b) => if *b { "true".into() } else { "false".into() },
        V::Null => "null".into(),
        _ => "[object Object]".into(),
    }
}
/// JS Number(v) — used by the leg sort comparator.
fn js_num(v: &V) -> f64 {
    match v {
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::String(s) => s.trim().parse::<f64>().unwrap_or(f64::NAN),
        V::Null => 0.0,
        V::Bool(b) => if *b { 1.0 } else { 0.0 },
        _ => f64::NAN,
    }
}
/// Map/Set key with JS SameValueZero-ish typing: scalars by tagged value,
/// objects/arrays by pointer identity (never equal to a different value).
fn map_key(v: &V) -> String {
    match v {
        V::String(s) => format!("s:{s}"),
        V::Number(_) => format!("n:{}", js_str(v)),
        V::Bool(b) => format!("b:{b}"),
        V::Null => "u:".into(),
        _ => format!("o:{:p}", v as *const V),
    }
}
fn prop<'a>(v: &'a V, key: &V) -> &'a V {
    match v {
        V::Object(m) => m.get(&js_str(key)).unwrap_or(&NULL),
        _ => &NULL,
    }
}

// bot/strategy.mjs raceIdentity — JSON.stringify'd identity of an eligible
// Election Outcome contract node, or null (verbatim port; no Rust caller for
// the rest of strategy.mjs yet).
fn race_identity(node: &V) -> Option<String> {
    let d = &node["contract_details"];
    if node["node_type"].as_str() != Some("contract")
        || node["contract_type"].as_str() != Some("Election Outcome")
        || d["resolutionType"].as_str() != Some("Party Winner")
        || d["raceStage"].as_str() != Some("General")
        || !js_truthy(d.get("raceId"))
        || !js_truthy(d.get("stageId"))
        || !js_truthy(d.get("electionDate"))
        || !js_truthy(d.get("winnerName"))
        || !js_truthy(node.get("settlement_date"))
        || node["settlement_date"].as_str().and_then(parse_iso_ms).is_none()
    {
        return None;
    }
    Some(js_stringify(&json!([
        d["raceId"], d["stageId"], d["electionDate"], node["settlement_date"], d["resolutionType"]
    ])))
}
// bot/strategy.mjs metadataPairs — recognized opposing parties only.
fn metadata_pairs(markets: &V, nodes: &V) -> Vec<V> {
    let mut groups: IndexMap<String, Vec<V>> = IndexMap::new();
    for m in markets.as_array().cloned().unwrap_or_default() {
        if m["status"].as_str() != Some("open") || js_truthy(m.get("isComposite"))
            || m["exchanges"].as_array().map(|a| a.len()) != Some(1) {
            continue;
        }
        let node = prop(nodes, &m["id"])["root"].clone();
        let Some(key) = race_identity(&node) else { continue; };
        if !node["settled_with"].is_null() { continue; }
        let party = &node["contract_details"]["winnerName"];
        if !matches!(party.as_str(), Some("Democratic Party") | Some("Republican Party")) { continue; }
        groups.entry(key).or_default().push(json!({
            "marketId": m["id"], "exchangeId": m["exchanges"][0]["id"],
            "title": m["title"], "party": party,
        }));
    }
    groups
        .into_values()
        .filter(|g| g.len() == 2 && !strict_eq(&g[0]["party"], &g[1]["party"]))
        .map(V::Array)
        .collect()
}

/// basketDefinitions(markets, nodes, universe)
/// Metadata identifies research pairs, not settlement guarantees. Preserve the
/// exact predicate fingerprints so later evidence cannot silently change a pair.
pub fn basket_definitions(markets: &V, nodes: &V, universe: &V) -> Result<V, String> {
    if !markets.is_array() || !js_truthy(Some(nodes)) || !universe["exchanges"].is_array() {
        return Err("Invalid basket catalogue".into());
    }
    let mut scope: IndexMap<String, &V> = IndexMap::new();
    for m in universe["exchanges"].as_array().unwrap() {
        scope.insert(map_key(&m["marketId"]), m);
    }
    let mut out = Vec::new();
    for pair in metadata_pairs(markets, nodes) {
        let mut legs = Vec::new();
        for m in pair.as_array().unwrap() {
            let official = scope.get(&map_key(&m["marketId"]));
            let node = prop(nodes, &m["marketId"])["root"].clone();
            if official.is_none()
                || !strict_eq(&official.unwrap()["exchangeId"], &m["exchangeId"])
                || official.unwrap()["option"].as_str() != Some("YES")
            {
                return Err("Basket catalogue scope mismatch".into());
            }
            legs.push(json!({
                "marketId": m["marketId"], "exchangeId": m["exchangeId"],
                "predicateHash": digest(&node),
            }));
        }
        legs.sort_by(|a, b| {
            js_num(&a["marketId"]).partial_cmp(&js_num(&b["marketId"])).unwrap_or(std::cmp::Ordering::Equal)
        });
        let race = race_identity(&prop(nodes, &legs[0]["marketId"])["root"])
            .map(V::from)
            .unwrap_or(V::Null);
        out.push(json!({
            "id": digest(&json!({"tournamentId": universe["tournamentId"], "race": race, "legs": legs})),
            "tournamentId": universe["tournamentId"],
            "race": race,
            "legs": legs,
            "settlementVerified": false,
        }));
    }
    Ok(V::Array(out))
}

/// basketCandidates(definitions, coverage, universe, now)
/// `now` is the caller's safe-integer millisecond clock.
pub fn basket_candidates(definitions: &V, coverage: &V, universe: &V, now: i64) -> Result<V, String> {
    if !definitions.is_array() { return Err("Invalid basket candidate inputs".into()); }
    if !strict_eq(&coverage["universeHash"], &universe["hash"])
        || !strict_eq(&coverage["tournamentId"], &universe["tournamentId"]) {
        return Ok(json!([]));
    }
    let exchanges = universe["exchanges"].as_array().cloned().unwrap_or_default();
    let mut scope: IndexMap<String, &V> = IndexMap::new();
    for m in &exchanges {
        scope.insert(map_key(&m["marketId"]), m);
    }
    let mut out = Vec::new();
    'next: for d in definitions.as_array().unwrap() {
        let legs = &d["legs"];
        let distinct = |key: &str| -> bool {
            legs.as_array().is_some_and(|l| l.len() == 2 && !strict_eq(&l[0][key], &l[1][key]))
        };
        if !strict_eq(&d["tournamentId"], &universe["tournamentId"])
            || legs.as_array().map(|l| l.len()) != Some(2)
            || !distinct("marketId")
            || !distinct("exchangeId")
        {
            return Err("Basket definition scope mismatch".into());
        }
        for l in legs.as_array().unwrap() {
            let m = scope.get(&map_key(&l["marketId"]));
            if m.is_none()
                || !strict_eq(&m.unwrap()["exchangeId"], &l["exchangeId"])
                || m.unwrap()["option"].as_str() != Some("YES")
            {
                return Err("Basket definition scope mismatch".into());
            }
        }
        let quotes: Vec<&V> = legs
            .as_array()
            .unwrap()
            .iter()
            .map(|l| prop(&coverage["quotes"], &l["exchangeId"]))
            .collect();
        let bad = quotes.iter().enumerate().any(|(i, q)| {
            let bid = q["bestBid"].as_f64();
            let ask = q["bestAsk"].as_f64();
            !js_truthy(Some(q))
                || !strict_eq(&q["exchangeId"], &legs[i]["exchangeId"])
                || !strict_eq(&q["marketId"], &legs[i]["marketId"])
                || q["option"].as_str() != Some("YES")
                || q["crossed"] == V::Bool(true)
                || !bid.is_some_and(|x| x.is_finite())
                || !ask.is_some_and(|x| x.is_finite())
                || bid.unwrap() <= 0.0
                || ask.unwrap() >= 1.0
                || bid.unwrap() >= ask.unwrap()
        });
        if bad { continue; }
        let receipt = &quotes[0]["receivedAt"];
        let clock = receipt.as_str().and_then(parse_iso_ms);
        if clock.is_none()
            || ms_to_iso(clock.unwrap()) != receipt.as_str().unwrap_or("")
            || !strict_eq(&quotes[1]["receivedAt"], receipt)
            || clock.unwrap() > now as f64
            || now as f64 - clock.unwrap() > 120_000.0
        {
            continue;
        }
        let mut bids = Vec::new();
        for q in &quotes {
            let text = js_str(&q["bestBid"]);
            match big_amount(&text) {
                Ok(v) => bids.push(v),
                Err(_) => continue 'next,
            }
        }
        let tick = BigInt::from(TICK);
        if bids.iter().any(|p| p % &tick != BigInt::from(0)) { continue; }
        let gap = &bids[0] + &bids[1] - BigInt::from(SCALE) - BigInt::from(2) * &tick;
        if gap <= BigInt::from(0) { continue; }
        out.push(json!({
            "tournamentId": universe["tournamentId"],
            "marketIds": legs.as_array().unwrap().iter().map(|l| l["marketId"].clone()).collect::<Vec<_>>(),
            "exchangeIds": legs.as_array().unwrap().iter().map(|l| l["exchangeId"].clone()).collect::<Vec<_>>(),
            "receivedAt": receipt,
            "priceGapMicros": gap.to_string().parse::<i64>().unwrap_or(0),
        }));
    }
    Ok(V::Array(out))
}
