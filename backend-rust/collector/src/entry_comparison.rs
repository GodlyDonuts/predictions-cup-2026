// live-paper/entry-comparison.mjs — common-feed entry-quality comparison cohort:
// separate hypothetical wallets reuse the same public books with shared costs.

use baal_paper::fixed::{big_amount, money, SCALE, TICK};
use baal_service_runtime::contract::{instant_ns, ms_to_iso, parse_iso_ms};
use baal_service_runtime::history::historical_reference;
use baal_service_runtime::json_text::{js_int, js_truthy};
use crate::dip_shadow::{dip_signal, validate_dip_policy};
use crate::model::{assert_state, digest, initial_state, invalidate, normalize_book, report,
    sample_equity, validate_config};
use crate::shock_rebound::{advance_shock_rebound, create_shock_rebound, invalidate_shock_rebound};
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::BTreeSet;

pub const ENTRY_COMPARISON_MARKETS: [&str; 4] = ["377", "354", "277", "305"];

fn entry_comparison_execution() -> V {
    json!({"name": "common_next_independent_book_v1", "feeBps": 0,
        "slippageTicks": 1, "depthFractionBps": 100, "orderShares": 10,
        "maxMarketCost": "25.000000", "maxExposure": "100.000000",
        "cumulativeBuyBudget": "100.000000", "maxOpenMarkets": 3, "latencyMs": 1000,
        "entryLifetimeMs": 60000, "accountContextMaxAgeMs": 180000,
        "duplicateEdgeCheckRoundoffTolerance": 4.0 * f64::EPSILON,
        "takeProfit": "0.010000", "stopLoss": "0.030000", "maximumHoldMs": 3600000,
        "cooldownMs": 60000,
        "dipReferenceConvention": "historical_center_minus_padded_ask_once",
        "shockReferenceConvention": "historical_bid_minus_exit_tick_minus_padded_ask",
        "liquidityAssumption": "aggregate_public_depth_may_include_own_orders; separate_hypothetical_wallets"})
}
const VARIANTS: [&str; 2] = ["historical_dip", "shock_rebound"];

fn hex64(s: &V) -> bool {
    s.as_str().is_some_and(|x| x.len() == 64 && x.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn canonical(x: &V) -> bool {
    x.as_str().is_some_and(|s| parse_iso_ms(s).is_some_and(|ms| ms_to_iso(ms) == s))
}
fn parse(x: &V) -> f64 {
    x.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(code.to_string()); }
    Ok(())
}
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(x), V::Number(y)) => x.as_f64() == y.as_f64(),
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
fn js_num(v: &V) -> f64 {
    match v {
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::Bool(b) => if *b { 1.0 } else { 0.0 },
        V::Null => 0.0,
        V::String(s) => {
            let t = s.trim();
            if t.is_empty() { 0.0 } else { t.parse::<f64>().unwrap_or(f64::NAN) }
        }
        _ => f64::NAN,
    }
}
fn js_str(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Null => "null".into(),
        V::Bool(b) => b.to_string(),
        V::Number(n) => {
            let f = n.as_f64().unwrap_or(f64::NAN);
            if f.is_finite() && f.fract() == 0.0 { (f as i64).to_string() } else { n.to_string() }
        }
        V::Array(a) => a.iter().map(|x| match x { V::Null => String::new(), o => js_str(o) })
            .collect::<Vec<_>>().join(","),
        V::Object(_) => "[object Object]".into(),
    }
}
fn js_number(x: f64) -> V {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 9.007_199_254_740_992e15 { json!(x as i64) } else { json!(x) }
}
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}
fn count(o: &mut V, key: &str) {
    let n = js_num(&o[key]);
    o[key] = js_number(n + 1.0);
}
fn amount(v: &V) -> Result<BigInt, String> {
    match v {
        V::String(s) => big_amount(s),
        _ => Err("Money must be a canonical decimal string with at most six places".into()),
    }
}
/// JS BigInt(x): numbers must be integral.
fn bigint(v: &V) -> Result<BigInt, String> {
    match v {
        V::Number(_) => js_int(v).map(BigInt::from).ok_or("Cannot convert value to a BigInt".into()),
        V::String(s) => s.parse::<i128>().map(BigInt::from)
            .map_err(|_| "Cannot convert string to a BigInt".to_string()),
        V::Bool(b) => Ok(BigInt::from(*b as i64)),
        _ => Err("Cannot convert value to a BigInt".into()),
    }
}
fn big_to_f64(v: &BigInt) -> f64 {
    v.to_string().parse::<f64>().unwrap_or(f64::NAN)
}
fn market_listed(v: &V) -> bool {
    ENTRY_COMPARISON_MARKETS.iter().any(|m| strict_eq(v, &json!(m)))
}
/// /^\/markets\/([1-9]\d*)\/orderbook\?/ on snap.endpoint || ''.
fn endpoint_market(ep: &str) -> Option<String> {
    let rest = ep.strip_prefix("/markets/")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.starts_with('0') { return None; }
    rest[digits.len()..].starts_with("/orderbook?").then_some(digits)
}

fn check_bank(bank: &V) -> Result<(), String> {
    need(bank["schemaVersion"].as_i64() == Some(1)
        && bank["name"].as_str() == Some("entry-quality-comparison-v1")
        && js_int(&bank["sourceRecords"]).is_some_and(|n| n > 0)
        && hex64(&bank["sourceHash"]) && canonical(&bank["cutoff"]), "Invalid comparison bank")?;
    validate_config(&bank["config"])?;
    validate_dip_policy(&bank["policy"])?;
    let c = &bank["config"];
    need(digest(&bank["sharedExecution"]) == digest(&entry_comparison_execution())
        && digest(&c["marketIds"]) == digest(&json!(ENTRY_COMPARISON_MARKETS))
        && strict_eq(&c["feeBps"], &json!(0)) && strict_eq(&c["slippageTicks"], &json!(1))
        && strict_eq(&c["depthFractionBps"], &json!(100)) && strict_eq(&c["orderShares"], &json!(10))
        && strict_eq(&c["latencyMs"], &json!(1000))
        && c["stopLoss"].as_str() == Some("0.030000")
        && c["maxExposure"].as_str() == Some("100.000000")
        && js_num(&c["maxDataAgeMs"]) <= 20000.0
        && amount(&c["cashReserve"])? == amount(&c["initialCash"])? - BigInt::from(100_000_000i64),
        "Changed comparison execution contract")?;
    need(strict_eq(&bank["universe"]["tournamentId"], &bank["config"]["tournamentId"])
        && bank["universe"]["exchanges"].is_array(), "Invalid comparison universe")?;
    let exchanges = bank["universe"]["exchanges"].as_array().cloned().unwrap_or_default();
    for market_id in ENTRY_COMPARISON_MARKETS {
        let matches: Vec<&V> = exchanges.iter()
            .filter(|x| strict_eq(&x["marketId"], &json!(market_id)) && x["option"].as_str() == Some("YES"))
            .collect();
        need(matches.len() == 1
            && matches[0]["exchangeId"].as_str()
                .is_some_and(|s| s.bytes().all(|b| b.is_ascii_digit()) && !s.is_empty() && s.as_bytes()[0] != b'0'),
            "Missing common scoped market")?;
    }
    let ids: BTreeSet<String> = exchanges.iter().map(|x| js_str(&x["exchangeId"])).collect();
    need(ids.len() == exchanges.len(), "Duplicate comparison exchange")?;
    need(bank["codeIdentity"].as_object().is_some_and(|o| !o.is_empty()
        && o.values().all(hex64)), "Missing frozen comparison code identity")?;
    Ok(())
}

/// createEntryComparison({sourceRecords, sourceHash, cutoff, config, universe,
/// identities, policy, codeIdentity}) — returns {bank, state}.
pub fn create_entry_comparison(input: &V) -> Result<V, String> {
    let mut c = input["config"].clone();
    c["marketIds"] = json!(ENTRY_COMPARISON_MARKETS);
    c["feeBps"] = json!(0);
    c["slippageTicks"] = json!(1);
    c["depthFractionBps"] = json!(100);
    c["orderShares"] = json!(10);
    c["maxPositionShares"] = json!(50);
    c["latencyMs"] = json!(1000);
    c["maxExposure"] = json!("100.000000");
    c["cashReserve"] = json!(money(&(amount(&input["config"]["initialCash"])? - BigInt::from(100_000_000i64))));
    c["stopLoss"] = json!("0.030000");
    c["exitAfterMs"] = json!(3600000);
    need(amount(&c["initialCash"])? > BigInt::from(100_000_000i64),
        "Insufficient comparison initial cash")?;
    let bank = json!({"schemaVersion": 1, "name": "entry-quality-comparison-v1",
        "sourceRecords": input["sourceRecords"], "sourceHash": input["sourceHash"],
        "cutoff": input["cutoff"], "config": c,
        "universe": input["universe"], "identities": input.get("identities").cloned()
            .unwrap_or_else(|| json!({})),
        "policy": input["policy"], "codeIdentity": input["codeIdentity"],
        "sharedExecution": entry_comparison_execution()});
    check_bank(&bank)?;
    Ok(json!({"bank": bank, "state": empty_entry_comparison_state(&bank)?}))
}

pub fn empty_entry_comparison_state(bank: &V) -> Result<V, String> {
    check_bank(bank)?;
    let exchanges = bank["universe"]["exchanges"].as_array().cloned().unwrap_or_default();
    let mut shocks = Map::new();
    for market_id in ENTRY_COMPARISON_MARKETS {
        let spec = exchanges.iter()
            .find(|x| strict_eq(&x["marketId"], &json!(market_id)) && x["option"].as_str() == Some("YES"));
        let sr = create_shock_rebound(market_id, &js_str(&spec.unwrap_or(&V::Null)["exchangeId"]),
            &js_str(&bank["sourceHash"]), &js_str(&bank["cutoff"]),
            &json!({"maxDataAgeMs": bank["config"]["maxDataAgeMs"]}))?;
        shocks.insert(market_id.to_string(), sr);
    }
    let mut strategies = Map::new();
    for name in VARIANTS {
        strategies.insert(name.to_string(), json!({
            "wallet": initial_state(&bank["config"], &format!("{}:{}", js_str(&bank["name"]), name),
                &js_str(&bank["cutoff"]))?,
            "uniqueObservations": 0, "acceptedSignals": 0, "gateCounts": {}, "executionGateCounts": {},
            "contaminationCounts": {}, "lastSignals": {}, "cooldowns": {}, "decisions": {},
            "fillAttribution": []}));
    }
    Ok(json!({"schemaVersion": 1, "bankHash": digest(bank), "sourceSequence": 0,
        "lastRecordHash": "", "lastSourceAt": null,
        "histories": {}, "bookHistory": {}, "seen": {}, "invalidatedAt": {},
        "shocks": V::Object(shocks), "proposals": {},
        "counters": {"developmentBooks": 0, "prospectiveBooks": 0, "independentBooks": 0,
            "contaminatedBooks": 0, "unknownContaminationBooks": 0, "skippedBooks": 0,
            "invalidBooks": 0, "accountOutages": 0},
        "strategies": V::Object(strategies)}))
}

fn revoke(s: &mut V, market_id: Option<&str>, at: &str, reason: &str) -> Result<(), String> {
    for id in ENTRY_COMPARISON_MARKETS.iter().filter(|x| market_id.is_none() || market_id == Some(*x)) {
        let last_at = if js_truthy(Some(&s["shocks"][*id]["state"]["last"]["at"])) {
            js_num(&s["shocks"][*id]["state"]["last"]["at"]) } else { 0.0 };
        let a_ms = parse(&json!(at));
        let mx = if a_ms.is_nan() || last_at.is_nan() { f64::NAN } else { a_ms.max(last_at) };
        if mx.is_nan() { return Err("RangeError: Invalid time value".into()); }
        let clock = ms_to_iso(mx);
        s["shocks"][*id]["state"] = invalidate_shock_rebound(
            &s["shocks"][*id]["state"], &s["shocks"][*id]["bank"], &clock)?;
        s["proposals"].as_object_mut().map(|o| o.shift_remove(&id.to_string()));
        s["invalidatedAt"][*id] = json!(clock);
        let names: Vec<String> = s["strategies"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        for name in names {
            invalidate(&mut s["strategies"][&name]["wallet"], Some(id), &clock, reason);
            let pending_keys: Vec<String> = s["strategies"][&name]["wallet"]["strategies"]["mean_reversion"]["pending"]
                .as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
            for key in pending_keys {
                let p = &s["strategies"][&name]["wallet"]["strategies"]["mean_reversion"]["pending"][&key];
                if p["action"].as_str() == Some("buy")
                    && s["strategies"][&name]["wallet"]["books"][&key]["marketId"].as_str() == Some(id) {
                    s["strategies"][&name]["wallet"]["strategies"]["mean_reversion"]["pending"]
                        .as_object_mut().map(|o| o.remove(&key));
                    let st = &mut s["strategies"][&name]["wallet"]["strategies"]["mean_reversion"];
                    st["expiredIntents"] = json!(js_int(&st["expiredIntents"]).unwrap_or(0) + 1);
                }
            }
        }
    }
    Ok(())
}

/// checkedPublicBook(s, snap, marketId, bank, at) -> {book, independent}
fn checked_public_book(s: &mut V, snap: &V, market_id: &str, bank: &V, at: &str) -> Result<V, String> {
    let c = &bank["config"];
    let spec = bank["universe"]["exchanges"].as_array().cloned().unwrap_or_default()
        .into_iter().find(|x| strict_eq(&x["marketId"], &json!(market_id)) && x["option"].as_str() == Some("YES"));
    need(snap["endpoint"].as_str() == Some(&format!("/markets/{}/orderbook?tournamentId={}&depth=20",
        market_id, js_str(&c["tournamentId"]))), "Foreign book endpoint")?;
    need(canonical(&snap["receivedAt"]) && canonical(&snap["requestStartedAt"])
        && parse(&snap["receivedAt"]) <= parse(&json!(at)), "Invalid public receipt")?;
    let b = normalize_book(&snap["data"], market_id, &js_str(&c["tournamentId"]),
        snap["receivedAt"].as_str().unwrap_or(""), snap["requestStartedAt"].as_str().unwrap_or(""),
        js_int(&c["maxDataAgeMs"]).unwrap_or(0), &BTreeSet::new(), "authoritative_receipt_v1")?;
    need(spec.as_ref().is_some_and(|sp| strict_eq(&b["exchangeId"], &sp["exchangeId"])),
        "Foreign public exchange")?;
    let old = s["seen"][market_id].clone();
    let old_present = js_truthy(Some(&old));
    let content = digest(&json!({"bids": b["bids"], "asks": b["asks"]}));
    if old_present {
        need(parse(&b["requestStartedAt"]) >= parse(&old["requestStartedAt"])
            && parse(&b["receivedAt"]) >= parse(&old["receivedAt"])
            && bigint(&b["sequence"])? >= bigint(&old["sequence"])?
            && instant_ns(b["sourceAt"].as_str().unwrap_or("")).map_err(|e| format!("{:?}", e))?
                >= instant_ns(old["sourceAt"].as_str().unwrap_or("")).map_err(|e| format!("{:?}", e))?,
            "Public book rollback")?;
        if strict_eq(&b["sequence"], &old["sequence"]) {
            need(strict_eq(&b["versionKey"], &old["versionKey"]) && content == js_str(&old["content"]),
                "Changed book under same sequence")?;
        }
    }
    if js_truthy(Some(&s["invalidatedAt"][market_id])) {
        need(parse(&b["requestStartedAt"]) > parse(&s["invalidatedAt"][market_id]),
            "Pre-invalidation public book")?;
    }
    let independent = !old_present || (!strict_eq(&b["versionKey"], &old["versionKey"])
        && instant_ns(b["sourceAt"].as_str().unwrap_or("")).map_err(|e| format!("{:?}", e))?
            > instant_ns(old["sourceAt"].as_str().unwrap_or("")).map_err(|e| format!("{:?}", e))?);
    s["seen"][market_id] = json!({"requestStartedAt": b["requestStartedAt"], "receivedAt": b["receivedAt"],
        "sequence": b["sequence"], "sourceAt": b["sourceAt"], "versionKey": b["versionKey"],
        "content": content});
    s["invalidatedAt"].as_object_mut().map(|o| o.remove(market_id));
    Ok(json!({"book": b, "independent": independent}))
}

struct Gate {
    admitted: V,
    reference: Option<f64>,
    minimum_edge: f64,
    quantity: V,
    reason: V,
    diagnostic: V,
}

fn run_wallet(s: &mut V, bank: &V, name: &str, b: &V, independent: bool,
    contamination: &str, gate: &Gate, record: &V) -> Result<(), String> {
    let c = &bank["config"];
    let p = &bank["sharedExecution"];
    let id = js_str(&b["instrument"]);
    let held = js_truthy(Some(&s["strategies"][name]["wallet"]["strategies"]["mean_reversion"]["positions"][&id]));
    let fills_before = s["strategies"][name]["wallet"]["strategies"]["mean_reversion"]["fills"]
        .as_array().map(|a| a.len()).unwrap_or(0);
    let used = s["strategies"][name]["wallet"]["strategies"]["mean_reversion"]["fills"]
        .as_array().cloned().unwrap_or_default().iter()
        .filter(|f| f["action"].as_str() == Some("buy"))
        .try_fold(BigInt::from(0), |n, f| Ok::<BigInt, String>(n + amount(&f["price"])? * bigint(&f["quantity"])?))?;
    let ask = amount(&b["asks"][0]["price"])? + BigInt::from(TICK);
    let qty_or = if gate.quantity.is_null() { 10.0 } else { js_num(&gate.quantity) };
    let budget_room = (amount(&p["cumulativeBuyBudget"])? - &used) / &ask;
    let market_room = amount(&p["maxMarketCost"])? / &ask;
    let cap = 0f64.max(js_min(10.0, js_min(qty_or, js_min(big_to_f64(&budget_room),
        js_min(big_to_f64(&market_room), js_min(
            (js_num(&b["asks"][0]["quantity"]) / 100.0).floor(),
            (js_num(&b["bids"][0]["quantity"]) / 100.0).floor()))))));
    let st = &s["strategies"][name]["wallet"]["strategies"]["mean_reversion"];
    let mut occupied: BTreeSet<String> = st["positions"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    occupied.extend(st["pending"].as_object().map(|o| o.keys().cloned().collect::<Vec<String>>()).unwrap_or_default());
    let mut reason = if !js_truthy(Some(&gate.admitted)) { "native_signal_rejected" }
        else if !independent { "no_independent_quote" }
        else if cap == 0.0 || cap.is_nan() { "depth_or_budget" }
        else if !occupied.contains(&id)
            && occupied.len() as f64 >= js_num(&p["maxOpenMarkets"]) { "maximum_open_markets" }
        else if parse(&s["strategies"][name]["cooldowns"][&js_str(&b["marketId"])]) > parse(&b["receivedAt"]) { "cooldown" }
        else { "allowed" };
    if s["strategies"][name]["wallet"]["strategies"]["mean_reversion"]["pending"][&id]["action"].as_str() == Some("buy") {
        let st = &mut s["strategies"][name]["wallet"]["strategies"]["mean_reversion"];
        if parse(&b["receivedAt"]) - parse(&st["pending"][&id]["createdAt"])
            > js_num(&p["entryLifetimeMs"]) {
            st["pending"].as_object_mut().map(|o| o.remove(&id));
            st["expiredIntents"] = json!(js_int(&st["expiredIntents"]).unwrap_or(0) + 1);
            reason = "entry_expired";
        } else {
            let q = js_min(js_num(&st["pending"][&id]["quantity"]), cap);
            st["pending"][&id]["quantity"] = js_number(q);
        }
    }
    if independent {
        count(&mut s["strategies"][name]["executionGateCounts"], reason);
        count(&mut s["strategies"][name]["contaminationCounts"], contamination);
    }
    let prior_consumed = s["strategies"][name]["wallet"]["strategies"]["mean_reversion"]["consumedVersions"][&id].clone();
    let prior_present = !prior_consumed.is_null();
    if !independent {
        s["strategies"][name]["wallet"]["strategies"]["mean_reversion"]["consumedVersions"][&id] = b["versionKey"].clone();
    }
    let wallet = s["strategies"][name]["wallet"].clone();
    let mut wallet = crate::model::step_dip(&wallet, b, c, gate.reference, reason == "allowed",
        gate.minimum_edge - js_num(&p["duplicateEdgeCheckRoundoffTolerance"]),
        &p["takeProfit"], js_int(&p["maximumHoldMs"]).unwrap_or(0), &p["maxMarketCost"],
        "responsive_v2")?;
    let next = wallet["strategies"]["mean_reversion"].clone();
    if !independent {
        let mut next2 = next.clone();
        if !prior_present { next2["consumedVersions"].as_object_mut().map(|o| o.remove(&id)); }
        else { next2["consumedVersions"][&id] = prior_consumed; }
        wallet["strategies"]["mean_reversion"] = next2;
    }
    let next = wallet["strategies"]["mean_reversion"].clone();
    if next["pending"][&id]["action"].as_str() == Some("buy") {
        let mut next2 = next.clone();
        next2["pending"][&id]["quantity"] = js_number(js_min(js_num(&next["pending"][&id]["quantity"]), cap));
        next2["pending"][&id]["reason"] = json!(name);
        if next2["pending"][&id]["signalSourceSequence"].is_null() {
            next2["pending"][&id]["signalSourceSequence"] = record["sequence"].clone();
        }
        if next2["pending"][&id]["signalSourceHash"].is_null() {
            next2["pending"][&id]["signalSourceHash"] = record["hash"].clone();
        }
        if next2["pending"][&id]["signalVersion"].is_null() {
            next2["pending"][&id]["signalVersion"] = b["versionKey"].clone();
        }
        wallet["strategies"]["mean_reversion"] = next2;
    }
    let next = wallet["strategies"]["mean_reversion"].clone();
    s["strategies"][name]["wallet"] = wallet;
    let intent = &next["pending"][&id];
    if !intent.is_null() && !js_truthy(Some(&s["strategies"][name]["decisions"][&js_str(&intent["id"])])) {
        s["strategies"][name]["decisions"][&js_str(&intent["id"])] = json!({
            "intentId": intent["id"], "action": intent["action"], "reason": intent["reason"],
            "quantity": intent["quantity"], "limit": intent["limit"], "createdAt": intent["createdAt"],
            "sourceSequence": record["sequence"], "sourceHash": record["hash"],
            "sourceVersion": b["versionKey"], "receivedAt": b["receivedAt"],
            "requestStartedAt": b["requestStartedAt"], "contamination": contamination,
            "nativeSignal": gate.diagnostic, "nativeSignalHash": digest(&gate.diagnostic),
            "reference": gate.reference.map(js_number).unwrap_or(V::Null),
            "minimumEdge": js_number(gate.minimum_edge)});
    }
    let fills_after = next["fills"].as_array().cloned().unwrap_or_default();
    for i in fills_before..fills_after.len() {
        let mut arr = s["strategies"][name]["fillAttribution"].as_array().cloned().unwrap_or_default();
        arr.push(json!({"fillIndex": i, "sourceSequence": record["sequence"], "sourceHash": record["hash"],
            "receivedAt": b["receivedAt"], "sourceVersion": b["versionKey"],
            "contamination": contamination, "intentId": fills_after[i]["intentId"],
            "model": "hypothetical_public_book_not_official_execution"}));
        s["strategies"][name]["fillAttribution"] = json!(arr);
    }
    if held && !js_truthy(Some(&next["positions"][&id])) {
        let t = parse(&b["receivedAt"]) + js_num(&p["cooldownMs"]);
        s["strategies"][name]["cooldowns"][&js_str(&b["marketId"])] = json!(ms_to_iso(t));
    }
    if name == "shock_rebound"
        && (js_truthy(Some(&next["positions"][&id])) || reason != "allowed") {
        s["proposals"].as_object_mut().map(|o| o.remove(&js_str(&b["marketId"])));
    }
    let spent = next["fills"].as_array().cloned().unwrap_or_default().iter()
        .filter(|f| f["action"].as_str() == Some("buy"))
        .try_fold(BigInt::from(0), |n, f| Ok::<BigInt, String>(n + amount(&f["price"])? * bigint(&f["quantity"])?))?;
    need(spent <= amount(&p["cumulativeBuyBudget"])?, "Comparison cumulative budget exceeded")?;
    assert_state(&s["strategies"][name]["wallet"], c)?;
    Ok(())
}

pub fn advance_entry_comparison(input: &V, record: &V, bank: &V) -> Result<V, String> {
    check_bank(bank)?;
    need(digest(bank) == js_str(&input["bankHash"]), "Comparison state bank mismatch")?;
    let mut body = record.clone();
    if let Some(o) = body.as_object_mut() { o.shift_remove("hash"); }
    let hash = &record["hash"];
    let at = if js_truthy(Some(&record["event"]["clockAt"])) { record["event"]["clockAt"].clone() }
        else { record["at"].clone() };
    // JS: record.sequence === input.sourceSequence + 1
    need(js_int(&record["sequence"]).is_some()
        && js_int(&record["sequence"]) == js_int(&input["sourceSequence"]).map(|x| x + 1)
        && strict_eq(&record["previous"], &input["lastRecordHash"])
        && hex64(hash) && digest(&body) == js_str(hash), "Comparison source continuity failure")?;
    need(canonical(&at) && (!js_truthy(Some(&input["lastSourceAt"]))
        || parse(&at) >= parse(&input["lastSourceAt"])), "Comparison source clock rollback")?;
    if js_num(&record["sequence"]) <= js_num(&bank["sourceRecords"]) {
        need(parse(&at) <= parse(&bank["cutoff"]), "Source prefix beyond cutoff")?;
    }
    let mut s = input.clone();
    let e = record["event"].clone();
    let prospective = js_num(&record["sequence"]) > js_num(&bank["sourceRecords"])
        && parse(&at) > parse(&bank["cutoff"]);
    s["sourceSequence"] = record["sequence"].clone();
    s["lastRecordHash"] = hash.clone();
    s["lastSourceAt"] = at.clone();
    if e["type"].as_str() == Some("market_history") {
        let h = &e["history"];
        let spec = bank["universe"]["exchanges"].as_array().cloned().unwrap_or_default()
            .into_iter().find(|x| strict_eq(&x["marketId"], &h["marketId"]) && market_listed(&x["marketId"]));
        if let Some(spec) = spec {
            need(strict_eq(&h["exchangeId"], &spec["exchangeId"])
                && strict_eq(&h["tournamentId"], &bank["config"]["tournamentId"])
                && canonical(&h["receivedAt"]) && canonical(&h["requestStartedAt"])
                && parse(&h["requestStartedAt"]) <= parse(&h["receivedAt"])
                && parse(&h["receivedAt"]) - parse(&h["requestStartedAt"]) <= 20000.0
                && parse(&h["receivedAt"]) <= parse(&at)
                && h["candles"].as_array().is_some_and(|a| a.len() <= 500), "Invalid comparison history")?;
            s["histories"][&js_str(&h["exchangeId"])] = h.clone();
        }
        return Ok(s);
    }
    if e["type"].as_str() == Some("account_read_outage") {
        if prospective {
            s["counters"]["accountOutages"] = js_number(js_num(&s["counters"]["accountOutages"]) + 1.0);
        }
        revoke(&mut s, None, &js_str(&at), "account_outage")?;
        return Ok(s);
    }
    if ["stopped", "started", "session_start", "session_stop"].contains(&e["type"].as_str().unwrap_or("")) {
        if prospective { count(&mut s["counters"], "collectorBarriers"); }
        revoke(&mut s, None, &js_str(&at), "collector_barrier")?;
        return Ok(s);
    }
    if e["type"].as_str() != Some("cycle") { return Ok(s); }
    let mut accepted_fresh: Vec<(String, V)> = Vec::new();
    // JS: [...(e.sourceSnapshots || []), ...(e.supplementalDepth || [])] — falsy → [];
    // a truthy non-array would spread differently in JS but never carries the
    // marketId/endpoint fields used below, so it is treated as empty here.
    let mut snapshots: Vec<V> = Vec::new();
    if js_truthy(Some(&e["sourceSnapshots"])) {
        snapshots.extend(e["sourceSnapshots"].as_array().cloned().unwrap_or_default()); }
    if js_truthy(Some(&e["supplementalDepth"])) {
        snapshots.extend(e["supplementalDepth"].as_array().cloned().unwrap_or_default()); }
    let mut indexed: Vec<(usize, V)> = snapshots.into_iter().enumerate().collect();
    indexed.sort_by(|(ai, a), (bi, b)| {
        let at_a = if js_truthy(Some(&a["skipped"])) { a["at"].clone() } else { a["receivedAt"].clone() };
        let at_b = if js_truthy(Some(&b["skipped"])) { b["at"].clone() } else { b["receivedAt"].clone() };
        let fa = if canonical(&at_a) { parse(&at_a) } else { f64::INFINITY };
        let fb = if canonical(&at_b) { parse(&at_b) } else { f64::INFINITY };
        fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
            .then(ai.cmp(bi))
    });
    for (snap_index, snap) in &indexed {
        let _ = snap_index;
        let market_v = if js_truthy(Some(&snap["marketId"])) { snap["marketId"].clone() } else {
            let ep = if js_truthy(Some(&snap["endpoint"])) { js_str(&snap["endpoint"]) } else { String::new() };
            endpoint_market(&ep).map(V::from).unwrap_or(V::Null)
        };
        if !market_listed(&market_v) { continue; }
        let market_id = js_str(&market_v);
        if js_truthy(Some(&snap["skipped"])) {
            let accepted = accepted_fresh.iter().find(|(k, _)| *k == market_id).map(|(_, v)| v.clone());
            if strict_eq(&snap["skipped"], &json!("Exchange has real account holdings or open orders"))
                && accepted.is_some()
                && e["accountExclusions"].as_array()
                    .is_some_and(|a| a.iter().any(|x| strict_eq(x, &accepted.as_ref().unwrap()["exchangeId"])))
                && canonical(&snap["at"])
                && parse(&snap["at"]) >= parse(&accepted.as_ref().unwrap()["receivedAt"])
                && parse(&snap["at"]) <= parse(&at) {
                if prospective { count(&mut s["counters"], "exclusionDiagnosticsRetained"); }
                continue;
            }
            if prospective {
                s["counters"]["skippedBooks"] = js_number(js_num(&s["counters"]["skippedBooks"]) + 1.0);
            }
            let rev_at = if canonical(&snap["at"]) && parse(&snap["at"]) <= parse(&at) {
                snap["at"].clone() } else { at.clone() };
            revoke(&mut s, Some(&market_id), &js_str(&rev_at), "read_skipped")?;
            continue;
        }
        let checked = checked_public_book(&mut s, &snap, &market_id, bank, &js_str(&at))
            .and_then(|r| {
                let shock = advance_shock_rebound(&s["shocks"][&market_id]["state"],
                    &r["book"], &s["shocks"][&market_id]["bank"])?;
                Ok(json!({"book": r["book"], "independent": r["independent"], "shock": shock}))
            });
        let checked = match checked {
            Ok(v) => v,
            Err(_) => {
                if prospective {
                    s["counters"]["invalidBooks"] = js_number(js_num(&s["counters"]["invalidBooks"]) + 1.0);
                }
                revoke(&mut s, Some(&market_id), &js_str(&at), "invalid_public_book")?;
                continue;
            }
        };
        let b = checked["book"].clone();
        let independent = js_truthy(Some(&checked["independent"]));
        let mut shock = checked["shock"].clone();
        accepted_fresh.push((market_id.clone(), b.clone()));
        s["shocks"][&market_id]["state"] = shock["state"].clone();
        let known_context = e["accountExclusions"].as_array()
            .is_some_and(|a| a.iter().all(|x| x.is_string()))
            && canonical(&e["exclusionObservedAt"])
            && parse(&e["exclusionObservedAt"]) <= parse(&b["requestStartedAt"]);
        let fresh_context = known_context && parse(&b["requestStartedAt"]) - parse(&e["exclusionObservedAt"])
            <= js_num(&bank["sharedExecution"]["accountContextMaxAgeMs"]);
        let contamination = if !fresh_context { "unknown_account_context" }
            else if e["accountExclusions"].as_array().is_some_and(|a| a.iter().any(|x| strict_eq(x, &b["exchangeId"]))) {
                "account_exposure_or_orders" } else { "no_reported_account_exposure" };
        let decision = prospective && parse(&b["requestStartedAt"]) > parse(&bank["cutoff"]);
        if !decision {
            s["counters"]["developmentBooks"] = js_number(js_num(&s["counters"]["developmentBooks"]) + 1.0);
        } else {
            s["counters"]["prospectiveBooks"] = js_number(js_num(&s["counters"]["prospectiveBooks"]) + 1.0);
            if independent {
                s["counters"]["independentBooks"] = js_number(js_num(&s["counters"]["independentBooks"]) + 1.0);
            }
            if contamination == "account_exposure_or_orders" {
                s["counters"]["contaminatedBooks"] = js_number(js_num(&s["counters"]["contaminatedBooks"]) + 1.0);
            }
            if contamination == "unknown_account_context" {
                s["counters"]["unknownContaminationBooks"] = js_number(js_num(&s["counters"]["unknownContaminationBooks"]) + 1.0);
            }
            let exchange_id = js_str(&b["exchangeId"]);
            let books: Vec<V> = s["bookHistory"][&exchange_id].as_array().cloned().unwrap_or_default();
            let reference = historical_reference(s["histories"].get(&exchange_id), &books,
                b["requestStartedAt"].as_str().unwrap_or(""), &bank["policy"])?;
            let slip = BigInt::from(TICK);
            let dip = dip_signal(&reference,
                big_to_f64(&(amount(&b["asks"][0]["price"])? + &slip)) / 1e6,
                big_to_f64(&(amount(&b["bids"][0]["price"])? - &slip)) / 1e6, &bank["policy"]);
            if js_truthy(Some(&shock["signal"])) {
                s["proposals"][&market_id] = shock["signal"].clone();
            }
            let proposal = s["proposals"][&market_id].clone();
            let proposal_set = js_truthy(Some(&proposal));
            let risk_revoked = proposal_set && {
                let revoked_prices = amount(&b["bids"][0]["price"])
                    .map(|bid0| bid0 < amount(&proposal["troughBid"]).unwrap_or_else(|_| BigInt::from(0)))
                    .unwrap_or(false);
                let sc_cfg = s["shocks"][&market_id]["bank"]["config"].clone();
                let anchor_breach = (|| -> Result<bool, String> {
                    let av = amount(&proposal["featuresAtShock"]["anchorMid"])
                        .map(|a| a * BigInt::from(2))?;
                    Ok(av - amount(&b["bids"][0]["price"])? - amount(&b["asks"][0]["price"])?
                        > amount(&sc_cfg["maximumShock"])? * BigInt::from(2))
                })().unwrap_or(false);
                revoked_prices || anchor_breach
                    || ["maximum_shock_breached", "insufficient_history", "trend_veto",
                        "missing_shock_anchor", "insufficient_trend_history"]
                        .contains(&shock["diagnostic"]["reason"].as_str().unwrap_or(""))
            };
            if risk_revoked {
                s["shocks"][&market_id]["state"] = invalidate_shock_rebound(
                    &s["shocks"][&market_id]["state"], &s["shocks"][&market_id]["bank"],
                    b["receivedAt"].as_str().unwrap_or(""))?;
                shock["diagnostic"] = json!({"reason": "proposal_risk_revoked", "at": b["receivedAt"]});
            }
            let proposal = if proposal_set
                && (risk_revoked || parse(&b["receivedAt"]) - parse(&proposal["at"])
                    > js_num(&bank["sharedExecution"]["entryLifetimeMs"])) {
                s["proposals"].as_object_mut().map(|o| o.remove(&market_id));
                V::Null
            } else { proposal };
            let proposal_set = js_truthy(Some(&proposal));
            let sc = s["shocks"][&market_id]["bank"]["config"].clone();
            let ask = amount(&b["asks"][0]["price"])? + BigInt::from(TICK);
            let bid = amount(&b["bids"][0]["price"])? - BigInt::from(TICK);
            let shock_allowed = proposal_set
                && (&ask <= &amount(&proposal["maximumEntryPrice"])?)
                && (&ask - &bid <= amount(&sc["maximumEffectiveSpread"])?)
                && (amount(&proposal["recoveryReferenceBid"])? - BigInt::from(TICK) - &ask
                    >= amount(&sc["minimumRoom"])?);
            let gates = [
                ("historical_dip", Gate {
                    admitted: dip["admitted"].clone(),
                    reference: if js_truthy(Some(&reference["ready"])) {
                        Some(js_num(&reference["center"])) } else { None },
                    minimum_edge: js_num(&bank["policy"]["minimumDip"]),
                    quantity: V::Null,
                    reason: dip["reason"].clone(),
                    diagnostic: dip.clone(),
                }),
                ("shock_rebound", Gate {
                    admitted: json!(shock_allowed),
                    reference: if proposal_set {
                        Some(big_to_f64(&(amount(&proposal["recoveryReferenceBid"])? - BigInt::from(TICK))) / 1e6)
                    } else { None },
                    minimum_edge: big_to_f64(&amount(&sc["minimumRoom"])?) / 1e6,
                    quantity: proposal["quantity"].clone(),
                    reason: shock["diagnostic"]["reason"].clone(),
                    diagnostic: shock["diagnostic"].clone(),
                }),
            ];
            for (name, gate) in &gates {
                if independent {
                    s["strategies"][*name]["uniqueObservations"] =
                        js_number(js_num(&s["strategies"][*name]["uniqueObservations"]) + 1.0);
                    count(&mut s["strategies"][*name]["gateCounts"], &js_str(&gate.reason));
                    if if *name == "historical_dip" { js_truthy(Some(&gate.admitted)) }
                        else { js_truthy(Some(&shock["signal"])) } {
                        s["strategies"][*name]["acceptedSignals"] =
                            js_number(js_num(&s["strategies"][*name]["acceptedSignals"]) + 1.0);
                    }
                }
                let mut last_signal = Map::new();
                last_signal.insert("at".into(), b["receivedAt"].clone());
                last_signal.insert("sourceVersion".into(), b["versionKey"].clone());
                last_signal.insert("contamination".into(), json!(contamination));
                last_signal.insert("admitted".into(), gate.admitted.clone());
                if let Some(d) = gate.diagnostic.as_object() {
                    for (k, v) in d { last_signal.insert(k.clone(), v.clone()); }
                }
                s["strategies"][*name]["lastSignals"][&market_id] = V::Object(last_signal);
                run_wallet(&mut s, bank, name, &b, independent, contamination, gate, record)?;
            }
        }
        if independent {
            let list = s["bookHistory"][&js_str(&b["exchangeId"])].as_array().cloned().unwrap_or_default();
            let mid = big_to_f64(&(amount(&b["bids"][0]["price"])? + amount(&b["asks"][0]["price"])?))
                / (2.0 * SCALE as f64);
            let mut next_list: Vec<V> = list.into_iter().chain([json!({"at": b["sourceAt"],
                "receivedAt": b["receivedAt"], "mid": mid, "versionKey": b["versionKey"]})])
                .filter(|x| parse(&x["at"]) >= parse(&b["receivedAt"]) - js_num(&bank["policy"]["lookbackMs"]))
                .collect();
            if next_list.len() > 512 { next_list = next_list[next_list.len() - 512..].to_vec(); }
            s["bookHistory"][&js_str(&b["exchangeId"])] = json!(next_list);
        }
    }
    if prospective {
        let names: Vec<String> = s["strategies"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        for name in names {
            let mut w = s["strategies"][&name]["wallet"].clone();
            sample_equity(&mut w, &bank["config"], &js_str(&at));
            s["strategies"][&name]["wallet"] = w;
        }
    }
    Ok(s)
}

pub fn entry_comparison_report(state: &V, bank: &V, at: &str) -> Result<V, String> {
    check_bank(bank)?;
    need(digest(bank) == js_str(&state["bankHash"]) && canonical(&json!(at))
        && (!js_truthy(Some(&state["lastSourceAt"])) || parse(&json!(at)) >= parse(&state["lastSourceAt"])),
        "Invalid comparison report")?;
    let mut strategies = Map::new();
    for name in VARIANTS {
        let v = &state["strategies"][name];
        let raw = &v["wallet"]["strategies"]["mean_reversion"];
        let metrics = report(&v["wallet"], &bank["config"], at);
        let metrics = &metrics["strategies"]["mean_reversion"];
        let label = |instrument: &V| -> String {
            let market_id = &v["wallet"]["books"][&js_str(instrument)]["marketId"];
            let mid_s = if market_id.is_null() { "undefined".to_string() } else { js_str(market_id) };
            let t = &bank["identities"][&mid_s]["title"];
            if js_truthy(Some(t)) { js_str(t) } else { format!("Market {} YES", mid_s) }
        };
        let positions: Vec<V> = metrics["positions"].as_array().cloned().unwrap_or_default()
            .into_iter().map(|mut p| {
                p["label"] = json!(label(&p["instrument"]));
                p
            }).collect();
        let recent_fills: Vec<V> = metrics["recentFills"].as_array().cloned().unwrap_or_default()
            .into_iter().map(|mut f| {
                f["label"] = json!(label(&f["instrument"]));
                f
            }).collect();
        let pending: Vec<V> = raw["pending"].as_object().map(|o| o.iter().map(|(k, intent)| {
            let mut obj = Map::new();
            obj.insert("instrument".into(), json!(k));
            obj.insert("label".into(), json!(label(&json!(k))));
            if let Some(m) = intent.as_object() { for (ik, iv) in m { obj.insert(ik.clone(), iv.clone()); } }
            V::Object(obj)
        }).collect()).unwrap_or_default();
        let cumulative = raw["fills"].as_array().cloned().unwrap_or_default().iter()
            .filter(|f| f["action"].as_str() == Some("buy"))
            .try_fold(BigInt::from(0), |n, f| Ok::<BigInt, String>(n + amount(&f["price"])? * bigint(&f["quantity"])?))?;
        let mut strat = metrics.clone();
        strat["positions"] = json!(positions);
        strat["recentFills"] = json!(recent_fills);
        strat["pending"] = json!(pending);
        strat["cumulativeBuyCost"] = json!(money(&cumulative));
        strat["uniqueObservations"] = v["uniqueObservations"].clone();
        strat["acceptedSignals"] = v["acceptedSignals"].clone();
        strat["gateCounts"] = v["gateCounts"].clone();
        strat["executionGateCounts"] = v["executionGateCounts"].clone();
        strat["contaminationCounts"] = v["contaminationCounts"].clone();
        strat["lastSignals"] = v["lastSignals"].clone();
        let fa = v["fillAttribution"].as_array().cloned().unwrap_or_default();
        let start = fa.len().saturating_sub(20);
        strat["recentFillAttribution"] = json!(fa[start..].to_vec());
        strategies.insert(name.to_string(), strat);
    }
    Ok(json!({"schemaVersion": 1, "mode": "prospective_common_feed_entry_comparison_paper",
        "cohortId": format!("{}:{}", js_str(&bank["name"]), js_str(&bank["cutoff"])),
        "updatedAt": at, "bankHash": state["bankHash"], "cutoff": bank["cutoff"],
        "sourceSequence": state["sourceSequence"], "sourceHash": state["lastRecordHash"],
        "marketIds": ENTRY_COMPARISON_MARKETS,
        "counters": state["counters"], "sharedExecution": bank["sharedExecution"],
        "strategies": V::Object(strategies),
        "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false,
        "assumptions": [
            "Separate hypothetical wallets reuse the same public observation; their capacity is not additive.",
            "Exposure/open-order contamination and unknown context are labeled, not erased; public depth may contain own orders.",
            "One adverse tick per executed side, zero added fees, one percent displayed depth, causal independent next-book fills.",
            "Native dip and shock gates differ; execution costs, capacity, holding horizon and exit policy are shared.",
            "Only post-cutoff decisions/fills count; sparse sampling cannot establish unobserved price crossings."],
    }))
}
