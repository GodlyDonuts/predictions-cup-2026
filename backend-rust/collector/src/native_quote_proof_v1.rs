// live-paper/native-quote-proof-v1.mjs — verify frozen native causal-quote dip
// wallet projections against declaration, binding and multi-source capture.
use crate::native_dip_proof_v1::micros;
use baal_execution::core::payload_hash;
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::{HashMap, HashSet, VecDeque};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("NATIVE_QUOTE_{}", code)); }
    Ok(())
}
fn hash_str(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn count(v: &V) -> bool { js_int(v).is_some_and(|x| x >= 0) }
fn key(c: &V) -> String { format!("{}/{}", jstr(&c["strategy"]), jstr(&c["scenario"]["id"])) }
// JS === between JSON values (numbers compare as f64, everything else strictly).
fn js_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Number(_), V::Number(_)) => a.as_f64().unwrap_or(f64::NAN) == b.as_f64().unwrap_or(f64::NAN),
        _ => a == b,
    }
}
fn jstr(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => {
            let mut b = ryu_js::Buffer::new();
            b.format_finite(n.as_f64().unwrap_or(f64::NAN)).to_string()
        }
        V::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        V::Null => "null".to_string(),
        _ => js_stringify(v),
    }
}
fn jnum(v: &V) -> f64 {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok())).unwrap_or(f64::NAN)
}
fn push_unique(v: &mut Vec<V>, x: V) {
    if !v.iter().any(|e| js_eq(e, &x)) { v.push(x); }
}
// /^(0|[1-9][0-9]*)$/ on strings.
fn version_str(v: &V) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty() && (s == "0" || !s.starts_with('0')) && s.bytes().all(|b| b.is_ascii_digit()))
}
// Date.parse + Number.isSafeInteger (no toISOString round-trip in this module).
fn ts(v: &V) -> Option<f64> {
    let n = v.as_str().and_then(parse_iso_ms)?;
    if n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0 { Some(n) } else { None }
}
// BigInt(x): integral numbers and integer strings coerce; anything else throws.
fn bigint_num(v: &V) -> Result<BigInt, String> {
    match v {
        V::Number(n) => {
            let f = n.as_f64().unwrap_or(f64::NAN);
            if f.is_finite() && f.fract() == 0.0 && f.abs() <= 9_007_199_254_740_991.0 {
                Ok(BigInt::from(f as i64))
            } else {
                Err(format!("RangeError: The number {} cannot be converted to a BigInt because it is not an integer", jstr(v)))
            }
        }
        V::String(s) => s.parse::<BigInt>()
            .map_err(|_| format!("SyntaxError: Cannot convert {} to a BigInt", s)),
        V::Bool(b) => Ok(BigInt::from(*b as i64)),
        _ => Err("TypeError: Cannot convert value to a BigInt".into()),
    }
}
fn js_max(a: f64, b: f64) -> f64 { if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) } }

/// verifyNativeQuoteResult({result, declaration, binding, universe, previous})
pub fn verify_native_quote_result(result: &V, d: &V, binding: &V, universe: &V, previous: Option<&V>) -> Result<V, String> {
    let cap = &result["capture"];
    let e = &result["evaluation"];
    let c = &d["config"];
    let start = d["declaredAt"].as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN);
    let through = e["evaluationThrough"].as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN);
    need(js_int(&binding["schemaVersion"]) == Some(1)
        && ["engineHash", "binaryHash", "runtimeLibraryHash", "declarationHash"].iter().all(|k| hash_str(&binding[*k])), "BINDING")?;
    need(js_int(&d["schemaVersion"]) == Some(1) && d["mode"].as_str() == Some("causal_quote_dip_subsequent_v1")
        && js_eq(&binding["cohort"], &d["cohort"]) && js_int(&c["feeBps"]) == Some(0)
        && c["fillClockPolicy"].as_str() == Some("independent_request_latency_v2")
        && js_int(&d["maxManagedMarkets"]) == Some(1)
        && payload_hash(&d["universe"]).ok() == payload_hash(universe).ok()
        && !start.is_nan() && start.fract() == 0.0 && start.abs() <= 9_007_199_254_740_991.0, "DECLARATION")?;
    let cap_through = if cap["through"].is_null() { 0.0 } else { jnum(&cap["through"]) };
    need(result["declarationFileHash"] == binding["declarationHash"]
        && cap["engineHash"] == binding["engineHash"]
        && cap["mode"].as_str() == Some("native_compact_market_sources_v1")
        && count(&cap["events"])
        && !through.is_nan() && through.fract() == 0.0 && through.abs() <= 9_007_199_254_740_991.0
        && through >= start && through == js_max(start, cap_through)
        && e["evaluationStart"] == d["declaredAt"] && e["cohort"] == d["cohort"] && e["mode"] == d["mode"]
        && cap["networkRequests"].as_i64() == Some(0) && cap["competitionOrders"].as_i64() == Some(0)
        && e["networkRequests"].as_i64() == Some(0) && e["competitionOrders"].as_i64() == Some(0)
        && e["profitabilityEstablished"] == V::Bool(false), "CAPTURE")?;
    for name in ["books", "quotes", "eligibility"] {
        let s = &cap["sources"][name];
        let dev = &d["development"][name];
        need(js_truthy(Some(s)) && hash_str(&s["hash"]) && count(&s["bytes"]) && count(&s["records"])
            && s["verifiedCheckpoints"].as_array().is_some_and(|a| a.iter().any(|x| {
                js_eq(&x["bytes"], &dev["bytes"]) && js_eq(&x["hash"], &dev["hash"])
                    && (x["through"].is_null() || jnum(&x["through"]) < start)
            })), "SOURCE_BINDING")?;
        if let Some(prev) = previous {
            let old = &prev["capture"]["sources"][name];
            need(jnum(&s["bytes"]) >= jnum(&old["bytes"]) && jnum(&s["records"]) >= jnum(&old["records"])
                && s["verifiedCheckpoints"].as_array().is_some_and(|a| a.iter().any(|x| {
                    js_eq(&x["bytes"], &old["bytes"]) && js_eq(&x["hash"], &old["hash"])
                })), "SOURCE_HISTORY")?;
        }
    }
    let mut expected: indexmap::IndexMap<String, (V, V)> = indexmap::IndexMap::new();
    for s in d["strategies"].as_array().cloned().unwrap_or_default() {
        for scenario in d["scenarios"].as_array().cloned().unwrap_or_default() {
            expected.insert(format!("{}/{}", jstr(&s["id"]), jstr(&scenario["id"])), (s.clone(), scenario));
        }
    }
    need(e["scorecards"].as_array().is_some_and(|a| a.len() == expected.len())
        && js_int(&e["candidates"]) == Some(expected.len() as i64), "CARDS")?;
    let mut members: HashMap<String, V> = HashMap::new();
    for x in universe["exchanges"].as_array().cloned().unwrap_or_default() {
        members.insert(format!("{}:YES", jstr(&x["exchangeId"])), x["marketId"].clone());
    }
    let prior: HashMap<String, V> = previous.map(|p| p["evaluation"]["scorecards"].as_array().cloned().unwrap_or_default()
        .into_iter().map(|c| (key(&c), c)).collect()).unwrap_or_default();
    let mut seen: HashSet<String> = HashSet::new();
    let mut held: Vec<V> = Vec::new();
    let mut pending: Vec<V> = Vec::new();
    let mut events: HashSet<String> = HashSet::new();
    let mut cards: Vec<V> = Vec::new();
    for card in e["scorecards"].as_array().cloned().unwrap_or_default() {
        let id = key(&card);
        let spec = expected.get(&id).cloned();
        let p = &card["point"];
        let old = prior.get(&id);
        need(spec.is_some() && !seen.contains(&id)
            && payload_hash(&spec.as_ref().unwrap().0["parameters"]).ok() == payload_hash(&card["parameters"]).ok()
            && payload_hash(&spec.as_ref().unwrap().1).ok() == payload_hash(&card["scenario"]).ok(), "CARD")?;
        seen.insert(id.clone());
        need(js_eq(&p["at"], &e["evaluationThrough"]) && js_eq(&p["fillClockPolicy"], &c["fillClockPolicy"])
            && card["fills"].is_array()
            && js_int(&p["fillExecutions"]) == Some(card["fills"].as_array().unwrap().len() as i64)
            && micros(&p["fees"])? == BigInt::from(0)
            && count(&p["intents"]) && count(&p["openShares"]) && count(&p["completedTrades"]), "POINT")?;
        if let Some(old) = old {
            let old_fills = old["fills"].as_array().cloned().unwrap_or_default();
            let fills = card["fills"].as_array().cloned().unwrap_or_default();
            need(fills.len() >= old_fills.len()
                && old_fills.iter().enumerate().all(|(i, f)| js_stringify(f) == js_stringify(&fills[i])), "FILL_HISTORY")?;
        }
        let mut lots: HashMap<String, VecDeque<(i64, BigInt)>> = HashMap::new();
        let mut used: HashSet<String> = HashSet::new();
        let mut cash = micros(&c["initialCash"])?;
        let mut basis = BigInt::from(0);
        let mut realized = BigInt::from(0);
        let mut turnover = BigInt::from(0);
        let (mut open, mut sold, mut closed) = (0i64, 0i64, 0i64);
        let mut last = start;
        for f in card["fills"].as_array().unwrap() {
            let q = &f["sourceClock"];
            let a = &f["economics"];
            let market = members.get(f["instrument"].as_str().unwrap_or("")).cloned();
            let intents = js_int(&p["intents"]).unwrap_or(i64::MAX);
            need(market.is_some()
                && c["marketIds"].as_array().is_some_and(|m| m.iter().any(|x| js_eq(x, market.as_ref().unwrap())))
                && ["buy", "sell"].contains(&f["action"].as_str().unwrap_or(""))
                && js_int(&f["quantity"]).is_some_and(|x| x > 0)
                && js_int(&f["intentId"]).is_some_and(|x| x > 0 && x <= intents)
                && js_int(&f["at"]).is_some_and(|t| t as f64 > start && t as f64 >= last && t as f64 <= through)
                && micros(&f["fee"])? == BigInt::from(0), "FILL")?;
            let fat = js_int(&f["at"]).unwrap();
            last = fat as f64;
            used.insert(jstr(&f["intentId"]));
            need(js_eq(&q["policy"], &c["fillClockPolicy"])
                && [&q["decisionAt"], &q["minimumLatencyMs"], &q["requestStartedAt"], &q["receivedAt"]].iter().all(|x| js_int(x).is_some())
                && js_int(&q["minimumLatencyMs"]) == js_int(&card["scenario"]["latencyMs"])
                && js_int(&q["decisionAt"]).unwrap() as f64 > start
                && js_int(&q["requestStartedAt"]).unwrap() >= js_int(&q["decisionAt"]).unwrap() + js_int(&q["minimumLatencyMs"]).unwrap()
                && js_int(&q["receivedAt"]).unwrap() == fat
                && fat >= js_int(&q["requestStartedAt"]).unwrap()
                && (fat - js_int(&q["requestStartedAt"]).unwrap()) as f64 <= c["maxDataAgeMs"].as_f64().unwrap_or(f64::NAN), "CLOCK")?;
            for k in ["decisionSequence", "decisionSourceNs", "fillSequence", "fillSourceNs"] {
                need(version_str(&q[k]), "VERSION")?;
            }
            let fill_seq = bigint_num(&q["fillSequence"])?;
            let dec_seq = bigint_num(&q["decisionSequence"])?;
            let fill_ns = bigint_num(&q["fillSourceNs"])?;
            let dec_ns = bigint_num(&q["decisionSourceNs"])?;
            need(fill_seq >= dec_seq && fill_ns >= dec_ns
                && (q["fillSequence"] != q["decisionSequence"] || q["fillSourceNs"] != q["decisionSourceNs"])
                && fill_ns <= BigInt::from(fat + 2000) * BigInt::from(1_000_000), "VERSION")?;
            need(js_truthy(Some(a))
                && js_int(&a["decisionAt"]) == js_int(&q["decisionAt"])
                && ["ENTRY", "TAKE_PROFIT", "STOP_LOSS", "HORIZON"].contains(&a["reason"].as_str().unwrap_or("")), "ECONOMICS")?;
            let price = micros(&f["price"])?;
            let qty_i = js_int(&f["quantity"]).unwrap();
            let qty = BigInt::from(qty_i);
            let quote = micros(&a["quotePrice"])?;
            let slip = bigint_num(&card["scenario"]["slippageTicks"])? * BigInt::from(5000);
            need(price >= BigInt::from(5000) && price <= BigInt::from(995_000) && &price % BigInt::from(5000) == BigInt::from(0)
                && price == &quote + if f["action"].as_str() == Some("buy") { slip.clone() } else { -&slip }
                && micros(&a["slippage"])? == &slip * &qty, "PRICE")?;
            turnover += &price * &qty;
            let instrument = jstr(&f["instrument"]);
            let queue = lots.entry(instrument.clone()).or_default();
            if f["action"].as_str() == Some("buy") {
                need(a["reason"].as_str() == Some("ENTRY") && js_eq(&f["quantity"], &c["orderShares"]), "ENTRY")?;
                cash -= &price * &qty;
                basis += &price * &qty;
                open += qty_i;
                queue.push_back((qty_i, price.clone()));
            } else {
                need(a["reason"].as_str() != Some("ENTRY"), "EXIT")?;
                cash += &price * &qty;
                sold += qty_i;
                open -= qty_i;
                let mut left = qty_i;
                while left > 0 {
                    need(!queue.is_empty(), "OVERSELL")?;
                    let lot = queue.front_mut().unwrap();
                    let n = left.min(lot.0);
                    realized += (&price - &lot.1) * BigInt::from(n);
                    basis -= &lot.1 * BigInt::from(n);
                    left -= n;
                    lot.0 -= n;
                    if lot.0 == 0 { queue.pop_front(); }
                }
                if queue.is_empty() { closed += 1; }
            }
            need(cash >= micros(&c["cashReserve"])?
                && basis <= micros(&c["maxExposure"])?
                && open >= 0
                && lots.values().filter(|x| !x.is_empty()).count() <= 1
                && (open as f64) <= c["maxPositionShares"].as_f64().unwrap_or(f64::NAN), "RISK")?;
            events.insert(js_stringify(&json!([f["instrument"], f["action"], f["at"], f["price"], f["quantity"]])));
        }
        need(cash == micros(&p["cash"])?
            && realized == micros(&p["realizedPnl"])?
            && turnover == micros(&p["turnover"])?
            && open == js_int(&p["openShares"]).unwrap_or(i64::MIN)
            && closed == js_int(&p["completedTrades"]).unwrap_or(i64::MIN)
            && realized == micros(&card["economics"]["netPnl"])?
            && sold == js_int(&card["economics"]["matchedShares"]).unwrap_or(i64::MIN)
            && micros(&card["economics"]["fees"])? == BigInt::from(0), "RECONCILIATION")?;
        let mut open_ids: HashSet<String> = HashSet::new();
        for (instrument, queue) in &lots {
            if !queue.is_empty() {
                if let Some(m) = members.get(instrument) { push_unique(&mut held, m.clone()); }
                open_ids.insert(instrument.clone());
            }
        }
        need(card["pendingReadIntents"].as_array()
            .is_some_and(|a| a.len() as i64 == js_int(&p["pendingIntents"]).unwrap_or(i64::MIN)), "INTENTS")?;
        let mut pending_ids: HashSet<String> = HashSet::new();
        for row in card["pendingReadIntents"].as_array().cloned().unwrap_or_default() {
            let at = ts(&row["decisionAt"]);
            let earliest = ts(&row["minimumRequestStartedAt"]);
            let expiry = ts(&row["expiresAt"]);
            let limit = micros(&row["limit"])?;
            need(members.get(row["instrument"].as_str().unwrap_or("")).is_some_and(|m| js_eq(m, &row["marketId"]))
                && row["intentId"].as_str().is_some_and(|s| !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit()))
                && bigint_num(&row["intentId"])? <= BigInt::from(js_int(&p["intents"]).unwrap_or(i64::MAX))
                && !used.contains(row["intentId"].as_str().unwrap())
                && !pending_ids.contains(&jstr(&row["instrument"])), "INTENT")?;
            pending_ids.insert(jstr(&row["instrument"]));
            let lat = card["scenario"]["latencyMs"].as_f64();
            let ma = c["maxDataAgeMs"].as_f64();
            let cy = c["cycleIntervalMs"].as_f64();
            need(at.is_some() && at.unwrap() > start && at.unwrap() <= through
                && lat.is_some_and(|l| earliest == Some(at.unwrap() + l))
                && ma.is_some() && cy.is_some() && expiry == Some(at.unwrap() + ma.unwrap() + cy.unwrap() * 4.0)
                && js_int(&row["quantity"]).is_some_and(|x| x > 0)
                && limit >= BigInt::from(5000) && limit <= BigInt::from(995_000) && &limit % BigInt::from(5000) == BigInt::from(0), "INTENT_CLOCK")?;
            let open_qty: i64 = lots.get(&jstr(&row["instrument"])).map(|q| q.iter().map(|l| l.0).sum()).unwrap_or(0);
            need((row["action"].as_str() == Some("buy")
                    && !open_ids.contains(&jstr(&row["instrument"]))
                    && js_eq(&row["quantity"], &c["orderShares"])
                    && row["reason"].as_str() == Some("ENTRY"))
                || (row["action"].as_str() == Some("sell")
                    && open_ids.contains(&jstr(&row["instrument"]))
                    && js_int(&row["quantity"]) == Some(open_qty)
                    && ["TAKE_PROFIT", "STOP_LOSS", "HORIZON"].contains(&row["reason"].as_str().unwrap_or(""))), "INTENT_QUANTITY")?;
            for k in ["decisionSequence", "decisionSourceNs"] {
                need(version_str(&row[k]), "INTENT_VERSION")?;
            }
            let mut o = row.as_object().cloned().unwrap_or_default();
            o.insert("wallet".into(), json!(id));
            pending.push(V::Object(o));
        }
        let mut cv = serde_json::Map::new();
        cv.insert("id".into(), json!(id));
        cv.insert("cash".into(), p["cash"].clone());
        cv.insert("realizedPnl".into(), p["realizedPnl"].clone());
        cv.insert("liquidationPnl".into(), p["liquidationPnl"].clone());
        cv.insert("valuation".into(), p["valuation"].clone());
        cv.insert("fills".into(), json!(card["fills"].as_array().unwrap().len()));
        cv.insert("completedTrades".into(), p["completedTrades"].clone());
        cv.insert("openShares".into(), p["openShares"].clone());
        cv.insert("pendingIntents".into(), p["pendingIntents"].clone());
        if let Some(sd) = card.as_object().and_then(|m| m.get("signalDiagnostics")) {
            cv.insert("signalDiagnostics".into(), sd.clone());
        }
        cards.push(V::Object(cv));
    }
    held.sort_by(|a, b| jstr(a).encode_utf16().cmp(jstr(b).encode_utf16()));
    Ok(json!({"schemaVersion": 1, "cohort": d["cohort"], "start": d["declaredAt"],
        "through": e["evaluationThrough"], "sources": cap["sources"], "candidates": cards.len(),
        "cards": cards, "heldMarketIds": held, "pendingReadIntents": pending,
        "uniqueFillEvents": events.len(), "fees": 0, "walletResultsAreCorrelated": true,
        "executionAuthority": false, "competitionOrders": 0, "networkRequests": 0,
        "profitabilityEstablished": false}))
}
/// nativeQuoteReadFocus(p, now)
pub fn native_quote_read_focus(p: &V, now: i64) -> V {
    let mut pending: Vec<V> = Vec::new();
    for x in p["pendingReadIntents"].as_array().cloned().unwrap_or_default() {
        let st = parse_iso_ms(x["minimumRequestStartedAt"].as_str().unwrap_or(""));
        let exp = parse_iso_ms(x["expiresAt"].as_str().unwrap_or(""));
        if st.is_some_and(|s| s <= now as f64) && exp.is_some_and(|e| e >= now as f64) {
            push_unique(&mut pending, x["marketId"].clone());
        }
    }
    pending.sort_by(|a, b| jstr(a).encode_utf16().cmp(jstr(b).encode_utf16()));
    json!({"heldMarketIds": p["heldMarketIds"], "pendingMarketIds": pending})
}
