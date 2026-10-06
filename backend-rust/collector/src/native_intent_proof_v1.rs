// live-paper/native-intent-proof-v1.mjs — read-only intent observation proof on
// top of the frozen native dip projection.
use crate::native_dip_proof_v1::{micros, verify_native_dip_result};
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_int, js_stringify};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::{HashMap, HashSet};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("NATIVE_INTENT_{}", code)); }
    Ok(())
}
fn count(v: &V) -> bool { js_int(v).is_some_and(|x| x >= 0) }
// JS integer: typeof s==='string' && /^(?:0|[1-9][0-9]*)$/.test(s)
fn integer(v: &V) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty() && (s == "0" || !s.starts_with('0')) && s.bytes().all(|b| b.is_ascii_digit()))
}
// JS instant: Date.parse + safe-integer + toISOString round-trip, else NATIVE_INTENT_CLOCK.
fn instant(v: &V) -> Result<i64, String> {
    let s = v.as_str().unwrap_or("");
    let n = parse_iso_ms(s).filter(|x| x.fract() == 0.0 && x.abs() <= 9_007_199_254_740_991.0);
    need(n.is_some() && ms_to_iso(n.unwrap()) == s, "CLOCK")?;
    Ok(n.unwrap() as i64)
}
// JS === on possibly-numeric JSON values.
fn js_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Number(_), V::Number(_)) => a.as_f64().unwrap_or(f64::NAN) == b.as_f64().unwrap_or(f64::NAN),
        _ => a == b,
    }
}
// JS String(x) for key concatenation (jstr) and Number(x) for numeric sorts.
fn jstr(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => {
            let mut b = ryu_js::Buffer::new();
            b.format_finite(n.as_f64().unwrap_or(f64::NAN)).to_string()
        }
        V::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        V::Null => "null".to_string(),
        _ => serde_json::to_string(v).unwrap_or_default(),
    }
}
fn jnum(v: &V) -> f64 {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok())).unwrap_or(f64::NAN)
}
fn push_unique(v: &mut Vec<V>, x: V) {
    if !v.iter().any(|e| js_eq(e, &x)) { v.push(x); }
}

const FIELDS: [&str; 12] = [
    "instrument", "marketId", "intentId", "action", "quantity", "limit",
    "decisionAt", "minimumRequestStartedAt", "expiresAt", "decisionSequence", "decisionSourceNs", "reason",
];

/// verifyNativeIntentResult({result, declaration, binding, universe, previous})
pub fn verify_native_intent_result(result: &V, declaration: &V, binding: &V, universe: &V, previous: Option<&V>) -> Result<V, String> {
    let mut projection = verify_native_dip_result(result, declaration, binding, universe, previous)?;
    let e = &result["evaluation"];
    let cfg = &declaration["config"];
    let start = instant(&declaration["declaredAt"])?;
    let through = instant(&e["evaluationThrough"])?;
    let mut scope: HashMap<String, V> = HashMap::new();
    for x in universe["exchanges"].as_array().cloned().unwrap_or_default() {
        scope.insert(format!("{}:YES", jstr(&x["exchangeId"])), x["marketId"].clone());
    }
    let mut all: Vec<V> = Vec::new();
    let mut pending_markets: Vec<V> = Vec::new();
    need(js_int(&e["intentObservationSchemaVersion"]) == Some(1), "SCHEMA")?;
    for card in e["scorecards"].as_array().cloned().unwrap_or_default() {
        let point = &card["point"];
        let rows = &card["pendingReadIntents"];
        need(rows.is_array()
            && count(&point["pendingIntents"])
            && rows.as_array().unwrap().len() as i64 == js_int(&point["pendingIntents"]).unwrap_or(-1)
            && count(&point["intents"]), "COUNT")?;
        let intents = js_int(&point["intents"]).unwrap_or(0);
        let mut held: HashMap<String, i64> = HashMap::new();
        for f in card["fills"].as_array().cloned().unwrap_or_default() {
            let q = js_int(&f["quantity"]).unwrap_or(0);
            *held.entry(jstr(&f["instrument"])).or_insert(0) +=
                if f["action"].as_str() == Some("buy") { q } else { -q };
        }
        let mut instruments: HashSet<String> = HashSet::new();
        let mut ids: HashSet<String> = HashSet::new();
        let filled: HashSet<String> = card["fills"].as_array().cloned().unwrap_or_default()
            .iter().map(|f| jstr(&f["intentId"])).collect();
        for row in rows.as_array().unwrap() {
            need(row.as_object().is_some_and(|m| m.len() == FIELDS.len() && FIELDS.iter().all(|k| m.contains_key(*k))), "FIELDS")?;
            let market = scope.get(row["instrument"].as_str().unwrap_or("")).cloned();
            need(market.as_ref().is_some_and(|m| js_eq(m, &row["marketId"]))
                && market.as_ref().is_some_and(|m| cfg["marketIds"].as_array().is_some_and(|a| a.iter().any(|x| js_eq(x, m))))
                && !instruments.contains(&jstr(&row["instrument"])), "SCOPE")?;
            instruments.insert(jstr(&row["instrument"]));
            need(integer(&row["intentId"])
                && row["intentId"].as_str().unwrap().parse::<BigInt>().is_ok_and(|i| i > BigInt::from(0) && i <= BigInt::from(intents))
                && !ids.contains(row["intentId"].as_str().unwrap())
                && !filled.contains(row["intentId"].as_str().unwrap()), "ID")?;
            ids.insert(row["intentId"].as_str().unwrap().to_string());
            need(["buy", "sell"].contains(&row["action"].as_str().unwrap_or(""))
                && js_int(&row["quantity"]).is_some_and(|q| q > 0 && q <= js_int(&cfg["maxPositionShares"]).unwrap_or(i64::MIN)), "QUANTITY")?;
            if row["action"].as_str() == Some("buy") {
                need(row["reason"].as_str() == Some("ENTRY")
                    && js_int(&row["quantity"]) == js_int(&cfg["orderShares"])
                    && held.get(&jstr(&row["instrument"])).copied().unwrap_or(0) == 0, "ENTRY")?;
            } else {
                need(["TAKE_PROFIT", "STOP_LOSS", "HORIZON"].contains(&row["reason"].as_str().unwrap_or(""))
                    && js_int(&row["quantity"]).is_some_and(|q| q == held.get(&jstr(&row["instrument"])).copied().unwrap_or(0)), "EXIT")?;
            }
            let limit = micros(&row["limit"])?;
            let upper = if row["action"].as_str() == Some("buy") {
                BigInt::from(1_000_000)
                    + BigInt::from(js_int(&card["scenario"]["slippageTicks"]).ok_or("RangeError: slippageTicks is not an integer")?) * BigInt::from(5000)
            } else {
                BigInt::from(1_000_000) + micros(&card["parameters"]["takeProfit"])?
            };
            need(limit > BigInt::from(0) && limit <= upper && &limit % BigInt::from(5000) == BigInt::from(0), "LIMIT")?;
            let decision = instant(&row["decisionAt"])?;
            let earliest = instant(&row["minimumRequestStartedAt"])?;
            let expiry = instant(&row["expiresAt"])?;
            let latency = js_int(&card["scenario"]["latencyMs"]);
            let max_age = js_int(&cfg["maxDataAgeMs"]);
            let cycle = js_int(&cfg["cycleIntervalMs"]);
            need(decision > start && decision <= through
                && latency.is_some_and(|l| earliest == decision + l)
                && max_age.is_some() && cycle.is_some()
                && expiry == decision + max_age.unwrap() + cycle.unwrap() * 4, "CLOCK")?;
            need(integer(&row["decisionSequence"]) && integer(&row["decisionSourceNs"]), "VERSION")?;
            let source: BigInt = row["decisionSourceNs"].as_str().unwrap_or("").parse().map_err(|_| "RangeError: not an integer".to_string())?;
            need(source <= BigInt::from(decision + 2000) * BigInt::from(1_000_000)
                && source >= BigInt::from(decision - max_age.unwrap_or(0)) * BigInt::from(1_000_000), "SOURCE_CLOCK")?;
            // An old unrefreshed intent remains in frozen accounting. It cannot request
            // an entry confirmation after expiry; inventory remains protected separately.
            let active = expiry >= through;
            if active { push_unique(&mut pending_markets, market.clone().unwrap()); }
            let mut o = row.as_object().unwrap().clone();
            // JS [a,b,c].join('/'): null/undefined elements join as "".
            let join = |v: &V| if v.is_null() { String::new() } else { jstr(v) };
            o.insert("card".into(), json!(format!("{}/{}/{}",
                join(&card["strategy"]), join(&card["scenario"]["id"]), join(&card["exitPolicy"]))));
            o.insert("active".into(), json!(active));
            o.insert("executionAuthority".into(), json!(false));
            all.push(V::Object(o));
        }
    }
    let mut unique: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    for row in &all {
        let mut identity = row.as_object().unwrap().clone();
        let card = identity.shift_remove("card").unwrap_or(V::Null);
        let key = js_stringify(&V::Object(identity.clone()));
        let entry = unique.entry(key).or_insert_with(|| {
            let mut i = identity.clone();
            i.insert("cards".into(), json!([]));
            V::Object(i)
        });
        entry["cards"].as_array_mut().unwrap().push(card);
    }
    pending_markets.sort_by(|a, b| jnum(a).partial_cmp(&jnum(b)).unwrap_or(std::cmp::Ordering::Equal));
    if let Some(obj) = projection.as_object_mut() {
        obj.insert("intentObservationSchemaVersion".into(), json!(1));
        obj.insert("pendingMarketIds".into(), json!(pending_markets));
        obj.insert("pendingReadIntents".into(), json!(unique.into_values().collect::<Vec<_>>()));
        obj.insert("readOnly".into(), json!(true));
    }
    Ok(projection)
}
/// nativeIntentReadFocus(projection, now)
pub fn native_intent_read_focus(projection: &V, now: i64) -> V {
    let mut held: Vec<V> = Vec::new();
    for m in projection["heldMarketIds"].as_array().cloned().unwrap_or_default() {
        push_unique(&mut held, m);
    }
    let mut pending: Vec<V> = Vec::new();
    for row in projection["pendingReadIntents"].as_array().cloned().unwrap_or_default() {
        let exp = parse_iso_ms(row["expiresAt"].as_str().unwrap_or(""));
        let st = parse_iso_ms(row["minimumRequestStartedAt"].as_str().unwrap_or(""));
        if exp.is_some_and(|e| e >= now as f64) && st.is_some_and(|s| s <= now as f64) && row["active"] == V::Bool(true) {
            push_unique(&mut pending, row["marketId"].clone());
        }
    }
    json!({"heldMarketIds": held, "pendingMarketIds": pending, "executionAuthority": false})
}
