// live-paper/basket-pair-capture.mjs — separate successor projection that
// consumes a paired depth measurement only when both leg receipts arrive
// together; the original observer and journal stay intact.
use crate::basket_observer::invalidate_basket;
use crate::basket_pair_observer::observe_complete_basket_pair;
use crate::model::{digest, normalize_book};
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Map, Value as V};
use std::collections::BTreeSet;

fn canonical(x: &V) -> bool {
    x.as_str().and_then(parse_iso_ms).is_some_and(|ms| ms_to_iso(ms) == x.as_str().unwrap())
}
/// JS `===` for JSON values: scalars by value, objects/arrays never equal.
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(x), V::Number(y)) => x.as_f64() == y.as_f64(),
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
/// SameValueZero (Set/includes): `===` except NaN equals NaN.
fn same_value_zero(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Number(x), V::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap_or(f64::NAN), y.as_f64().unwrap_or(f64::NAN));
            x == y || (x.is_nan() && y.is_nan())
        }
        _ => strict_eq(a, b),
    }
}
fn js_set_len(xs: &[V]) -> usize {
    let mut seen: Vec<&V> = Vec::new();
    for x in xs {
        if !seen.iter().any(|s| same_value_zero(s, x)) {
            seen.push(x);
        }
    }
    seen.len()
}
/// a === b + 1 under JS number semantics.
fn plus1_eq(a: &V, b: &V) -> bool {
    a.is_number() && b.is_number() && a.as_f64() == b.as_f64().map(|x| x + 1.0)
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
fn record_clock<'a>(r: &'a V) -> &'a V {
    // e.clockAt || record.at — e is already required non-null by callers.
    if js_truthy(r["event"].get("clockAt")) { &r["event"]["clockAt"] } else { &r["at"] }
}
/// /^\/markets\/([1-9]\d*)\/orderbook\?/ on (r.endpoint || '').
fn endpoint_market_id(endpoint: &V) -> Option<String> {
    let ep = endpoint.as_str().unwrap_or("");
    let rest = ep.strip_prefix("/markets/")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.starts_with('0') { return None; }
    if !rest[digits.len()..].starts_with("/orderbook?") { return None; }
    Some(digits)
}
/// r.marketId || endpoint-regex — the raw value (could be undefined).
fn snapshot_market_id(r: &V) -> Option<V> {
    if js_truthy(r.get("marketId")) { return Some(r["marketId"].clone()); }
    endpoint_market_id(&r["endpoint"]).map(V::from)
}
fn spread(v: &V) -> Result<Vec<V>, String> {
    if !js_truthy(Some(v)) { return Ok(vec![]); }
    match v {
        V::Array(a) => Ok(a.clone()),
        V::String(s) => Ok(s.chars().map(|c| V::from(c.to_string())).collect()),
        _ => Err("spread argument is not iterable".into()),
    }
}
/// `p.bank` property read for `.legs` access.
fn p_bank<'a>(p: &'a V, prop: &str) -> Result<&'a V, String> {
    match p.get("bank") {
        None => Err(format!("Cannot read properties of undefined (reading '{}')", prop)),
        Some(V::Null) => Err(format!("Cannot read properties of null (reading '{}')", prop)),
        Some(b) => Ok(b),
    }
}
/// `p.bank.legs` as an iterable (for-of): strings iterate char-wise, other
/// non-arrays throw `p.bank.legs is not iterable`.
fn p_legs_iterable(p: &V) -> Result<Vec<V>, String> {
    match p_bank(p, "legs")?.get("legs") {
        Some(V::Array(a)) => Ok(a.clone()),
        Some(V::String(s)) => Ok(s.chars().map(|c| V::from(c.to_string())).collect()),
        _ => Err("p.bank.legs is not iterable".into()),
    }
}
/// `p.bank.legs` for `.some(...)` — strings have no `.some` method.
fn p_legs_some(p: &V) -> Result<Vec<V>, String> {
    match p_bank(p, "legs")?.get("legs") {
        None => Err("Cannot read properties of undefined (reading 'some')".into()),
        Some(V::Null) => Err("Cannot read properties of null (reading 'some')".into()),
        Some(V::Array(a)) => Ok(a.clone()),
        Some(_) => Err("p.bank.legs.some is not a function".into()),
    }
}
/// `for (const p of bank.pairs)` — strings iterate char-wise.
fn bank_pairs_iterable(bank: &V) -> Result<Vec<V>, String> {
    match bank.get("pairs") {
        None => Err("Cannot read properties of undefined (reading 'pairs')".into()),
        Some(V::Null) => Err("Cannot read properties of null (reading 'pairs')".into()),
        Some(V::Array(a)) => Ok(a.clone()),
        Some(V::String(s)) => Ok(s.chars().map(|c| V::from(c.to_string())).collect()),
        Some(_) => Err("bank.pairs is not iterable".into()),
    }
}
/// state.pairs[p.id] = invalidateBasket(state.pairs[p.id], leg.marketId, at, p.bank)
/// for every leg — a paired failure always retires both legs at the record clock.
fn invalidate_pair(state: &mut V, p: &V, at: &str) -> Result<(), String> {
    let pid = js_str(&p["id"]);
    for leg in p_legs_iterable(p)? {
        // marketId === null invalidates every leg; any other value must match a
        // leg's marketId or throw 'Foreign basket invalidation' (undefined and
        // non-strings can never === a leg's string marketId).
        let mid: Option<String> = match leg.get("marketId") {
            Some(V::Null) => None,
            Some(v) => Some(v.as_str().ok_or("Foreign basket invalidation")?.to_string()),
            None => return Err("Foreign basket invalidation".into()),
        };
        if state.get("pairs").is_none_or(|x| x.is_null()) {
            return Err(format!("Cannot read properties of undefined (reading '{}')", pid));
        }
        let next = invalidate_basket(&state["pairs"][&pid], mid.as_deref(), at, &p["bank"])?;
        state["pairs"][&pid] = next;
    }
    Ok(())
}

/// advanceCompleteBasketRecord(input, record, bank) — {state, observations}
pub fn advance_complete_basket_record(input: &V, record: &V, bank: &V) -> Result<V, String> {
    if !strict_eq(&input["bankHash"], &V::from(digest(bank)))
        || !plus1_eq(&record["sequence"], &input["sourceSequence"])
        || !strict_eq(&record["previous"], &input["lastRecordHash"])
    {
        return Err("BASKET_PAIR_SOURCE_CONTINUITY".into());
    }
    match record.get("event") {
        None => return Err("Cannot read properties of undefined (reading 'clockAt')".into()),
        Some(V::Null) => return Err("Cannot read properties of null (reading 'clockAt')".into()),
        _ => {}
    }
    let mut state = input.clone();
    let mut observations: Vec<V> = Vec::new();
    let e = &record["event"];
    let at_v = record_clock(record);
    if !canonical(at_v) { return Err("BASKET_PAIR_SOURCE_CLOCK".into()); }
    let at = at_v.as_str().unwrap();
    let at_ms = parse_iso_ms(at).unwrap();
    let cutoff_ms = parse_iso_ms(bank["cutoff"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
    if at_ms > cutoff_ms {
        if e["type"].as_str() == Some("account_read_outage") {
            for p in &bank_pairs_iterable(bank)? { invalidate_pair(&mut state, p, at)?; }
        }
        if e["type"].as_str() == Some("cycle") {
            let excl_at_ms = parse_iso_ms(e["exclusionObservedAt"].as_str().unwrap_or(""));
            let exclusions_ok = e["accountExclusions"].as_array().is_some_and(|a| a.iter().all(|x| x.is_string()))
                && canonical(&e["exclusionObservedAt"])
                && excl_at_ms.unwrap_or(f64::NAN) <= at_ms;
            // Object.values(state.pairs) is the last || operand — only evaluated
            // once exclusions are fresh.
            let stale_legs = if exclusions_ok {
                match &state["pairs"] {
                    V::Object(m) => m.values().any(|s| {
                        js_truthy(s.get("lastAt"))
                            && s["lastAt"].as_str().and_then(parse_iso_ms)
                                .zip(excl_at_ms)
                                .is_some_and(|(l, x)| l > x)
                    }),
                    V::Null => return Err("Cannot convert undefined or null to object".into()),
                    _ => false, // Object.values(primitive) is []
                }
            } else {
                false
            };
            let fresh_exclusions = exclusions_ok && !stale_legs;
            if !fresh_exclusions {
                for p in &bank_pairs_iterable(bank)? { invalidate_pair(&mut state, p, at)?; }
            } else {
                let excluded: BTreeSet<String> = e["accountExclusions"].as_array().unwrap().iter()
                    .filter_map(|x| x.as_str().map(String::from)).collect();
                let mut snapshots = spread(&e["sourceSnapshots"])?;
                snapshots.extend(spread(&e["supplementalDepth"])?);
                let measurement = &e["basketMeasurement"];
                for p in &bank_pairs_iterable(bank)? {
                    let legs = p_legs_some(p)?;
                    if legs.iter().any(|l| l["exchangeId"].as_str().is_some_and(|s| excluded.contains(s))) {
                        invalidate_pair(&mut state, p, at)?;
                        continue;
                    }
                    let matches: Vec<V> = snapshots.iter().filter(|r| {
                        let mid = snapshot_market_id(r);
                        legs.iter().any(|l| match &mid {
                            // l.marketId === undefined only when the leg lacks the key.
                            Some(m) => strict_eq(&l["marketId"], m),
                            None => l.get("marketId").is_none(),
                        })
                    }).cloned().collect();
                    if matches.is_empty() { continue; }
                    let ids = &measurement["marketIds"];
                    // Arbitrary single-leg/foreign-cycle quotes cannot seed a paired claim.
                    let qualified = ["paired_depth_measurement", "paired_depth_followup"]
                        .contains(&measurement["reason"].as_str().unwrap_or(""))
                        && ids.as_array().is_some_and(|a| {
                            a.len() == 2
                                && js_set_len(a) == 2
                                && legs.iter().all(|l| {
                                    l.get("marketId")
                                        .is_some_and(|lv| a.iter().any(|x| same_value_zero(x, lv)))
                                })
                        })
                        && matches.len() == 2;
                    if !qualified {
                        invalidate_pair(&mut state, p, at)?;
                        continue;
                    }
                    let attempt = (|| -> Result<Vec<V>, String> {
                        matches.iter().map(|r| {
                            let id = endpoint_market_id(&r["endpoint"]);
                            let expected = format!("/markets/{}/orderbook?tournamentId={}&depth=20",
                                id.clone().unwrap_or_else(|| "undefined".into()),
                                js_str(&p["bank"]["tournamentId"]));
                            let recv = r["receivedAt"].as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN);
                            if js_truthy(r.get("skipped"))
                                || !canonical(&r["receivedAt"])
                                || recv > at_ms
                                || recv < excl_at_ms.unwrap_or(f64::NAN)
                                || r["endpoint"].as_str() != Some(expected.as_str())
                            {
                                return Err("INVALID_PAIRED_SOURCE".into());
                            }
                            let mut b = normalize_book(&r["data"],
                                id.as_deref().unwrap_or("undefined"),
                                p["bank"]["tournamentId"].as_str().unwrap_or(""),
                                r["receivedAt"].as_str().unwrap_or(""),
                                r["requestStartedAt"].as_str().unwrap_or(""),
                                20000, &excluded, "authoritative_receipt_v1")?;
                            b["tournamentId"] = p["bank"]["tournamentId"].clone();
                            Ok(b)
                        }).collect()
                    })();
                    match attempt.and_then(|books| {
                        observe_complete_basket_pair(&state["pairs"][&js_str(&p["id"])], &V::Array(books), &p["bank"])
                    }) {
                        Err(_) => { invalidate_pair(&mut state, p, at)?; }
                        Ok(result) => {
                            let pid = js_str(&p["id"]);
                            state["pairs"][&pid] = result["state"].clone();
                            let mut o = Map::new();
                            o.insert("pairId".into(), p["id"].clone());
                            if let Some(obj) = result["observation"].as_object() {
                                for (k, v) in obj { o.insert(k.clone(), v.clone()); }
                            }
                            o.insert("sourceSequence".into(), record["sequence"].clone());
                            o.insert("sourceHash".into(), record["hash"].clone());
                            o.insert("measurementReason".into(), measurement["reason"].clone());
                            observations.push(V::Object(o));
                        }
                    }
                }
            }
        }
    }
    state["sourceSequence"] = record["sequence"].clone();
    state["lastRecordHash"] = record["hash"].clone();
    Ok(json!({"state": state, "observations": observations}))
}
