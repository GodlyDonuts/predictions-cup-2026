// live-paper/public-recovery.mjs — public REST dip-recovery paper engine:
// nine entry/payoff variant wallets over verified rest books.
use crate::public_recovery_confirmation::{
    observe_recovery, recovery_confirmation, recovery_confirmation_policy,
};
use baal_paper::fixed::{big_amount, big_price, big_sequence, instant, money, TICK};
use baal_service_runtime::contract::{instant_ns, ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{digest, js_int, js_truthy};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

fn need(ok: bool, msg: &str) -> Result<(), String> { if !ok { return Err(msg.to_string()); } Ok(()) }
fn inc(o: &mut V, k: &str) { o[k] = json!(o[k].as_i64().unwrap_or(0) + 1); }
fn f64_of_big(v: &BigInt) -> f64 { v.to_string().parse().unwrap_or(f64::NAN) }
fn instant_s(v: &V) -> Result<i64, String> { instant(v.as_str().unwrap_or("")) }
fn instant_ns_s(v: &V) -> Result<i128, String> { instant_ns(v.as_str().unwrap_or("")).map_err(|e| e.0) }
// Integer-valued JSON number (JS numbers spell without a fraction).
fn num_json(x: f64) -> V {
    if x.is_finite() && x.fract() == 0.0 && x.abs() <= 9_007_199_254_740_991.0 { json!(x as i64) } else { json!(x) }
}
fn median(x: &[f64]) -> f64 {
    let mut a = x.to_vec();
    a.sort_by(|p, q| p.partial_cmp(q).unwrap_or(std::cmp::Ordering::Equal));
    let n = a.len();
    if n % 2 == 1 { a[(n - 1) / 2] } else { (a[n / 2 - 1] + a[n / 2]) / 2.0 }
}
// JS object enumeration order: integer-index keys first in numeric order, then
// the rest in insertion order.
fn index_key(k: &str) -> Option<u32> {
    let v = k.parse::<u32>().ok()?;
    if v < u32::MAX && v.to_string() == k { Some(v) } else { None }
}
fn js_entries(v: &V) -> Vec<(String, V)> {
    let mut a: Vec<(String, V)> = v.as_object()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    a.sort_by(|(a, _), (b, _)| match (index_key(a), index_key(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    });
    a
}
fn is_lower_hex_64(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64
        && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}
fn is_pos_int_id(v: &V) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_digit()) && s.as_bytes()[0] != b'0')
}

/// PUBLIC_CONFIRMATION_POLICY (frozen object).
pub fn public_confirmation_policy() -> V {
    let mut p = recovery_confirmation_policy();
    p["windowMs"] = json!(600000);
    p["maximumGapMs"] = json!(180000);
    p
}
/// PUBLIC_RECOVERY_VARIANTS (frozen array, entry-major then payoff order).
pub fn public_recovery_variants() -> V {
    let mut out = Vec::new();
    for entry in ["baseline", "confirmed", "pricecap"] {
        for (name, tp, sl) in [
            ("legacy", "0.010000", "0.030000"),
            ("balanced", "0.020000", "0.020000"),
            ("reward", "0.030000", "0.020000"),
        ] {
            out.push(json!({"id":format!("{entry}_{name}"),"entry":entry,
                "payoff":{"name":name,"takeProfit":tp,"stopLoss":sl}}));
        }
    }
    json!(out)
}
/// publicRecoveryHash(x) = sha256(JSON.stringify(x)).
pub fn public_recovery_hash(x: &V) -> String { digest(x) }
/// PUBLIC_RECOVERY_POLICY (frozen object).
pub fn public_recovery_policy() -> V {
    json!({"feeBps":0,"historyMs":21600000,"minimumMinutes":8,"minimumSpanMs":1800000,
        "maximumHistoryAgeMs":2700000,"minimumDip":0.02,"minimumRelativeDip":0.03,
        "maximumDip":0.1,"minimumZ":1.5,"maximumSpread":0.02,"minimumPrice":0.05,
        "maximumPrice":0.95,"entryLifetimeMs":120000,"maximumQuoteAgeMs":20000,
        "takeProfit":"0.010000","stopLoss":"0.030000","maximumHoldMs":3600000,
        "cooldownMs":900000,"initialCash":"100.000000","maxMarketCost":"25.000000",
        "maxExposure":"100.000000","cumulativeBuyBudget":"100.000000",
        "maxOpenMarkets":3,"orderShares":10,"depthFractionBps":100,"slippageTicks":1,
        "latenciesMs":[1000,3000]})
}

fn check_bank(b: &V) -> Result<(), String> {
    need(b["schemaVersion"].as_i64() == Some(1)
        && b["mode"].as_str() == Some("public_rest_recovery_paper")
        && is_lower_hex_64(&b["sourceScopeHash"]), "PUBLIC_RECOVERY_BANK")?;
    instant_s(&b["cutoff"])?;
    need(b["tournamentId"].as_str() == Some("bda92870-621e-47b0-bc3c-3602c5c26f55"),
        "PUBLIC_RECOVERY_TOURNAMENT")?;
    need(js_int(&b["startEventId"]).is_some_and(|x| x >= 0), "PUBLIC_RECOVERY_START_EVENT")?;
    need(public_recovery_hash(&b["policy"]) == public_recovery_hash(&public_recovery_policy()),
        "PUBLIC_RECOVERY_POLICY_CHANGED")?;
    need(public_recovery_hash(&b["variants"]) == public_recovery_hash(&public_recovery_variants()),
        "PUBLIC_RECOVERY_VARIANTS_CHANGED")?;
    need(public_recovery_hash(&b["confirmationPolicy"])
        == public_recovery_hash(&public_confirmation_policy()), "PUBLIC_RECOVERY_CONFIRMATION_CHANGED")?;
    let markets = b["markets"].as_array();
    need(markets.is_some_and(|m| !m.is_empty() && m.len() <= 256)
        && markets.map(|m| {
            // JS Set dedupe by SameValueZero; js_stringify gives a faithful key.
            let mut ids = std::collections::HashSet::new();
            let mut xs = std::collections::HashSet::new();
            m.iter().all(|x| ids.insert(baal_service_runtime::json_text::js_stringify(&x["marketId"]))
                && xs.insert(baal_service_runtime::json_text::js_stringify(&x["exchangeId"])))
        }).unwrap_or(false), "PUBLIC_RECOVERY_SCOPE")?;
    for m in markets.map(|m| m.as_slice()).unwrap_or(&[]) {
        need(is_pos_int_id(&m["marketId"]) && is_pos_int_id(&m["exchangeId"]), "PUBLIC_RECOVERY_ID")?;
    }
    Ok(())
}

/// createPublicRecovery(bank)
pub fn create_public_recovery(bank: &V) -> Result<V, String> {
    check_bank(bank)?;
    let mut wallets = serde_json::Map::new();
    for variant in bank["variants"].as_array().cloned().unwrap_or_default() {
        for latency in bank["policy"]["latenciesMs"].as_array().cloned().unwrap_or_default() {
            let key = format!("{}_{}", variant["id"].as_str().unwrap_or(""),
                latency.as_i64().unwrap_or(0));
            wallets.insert(key, json!({"variant":variant,"latencyMs":latency,
                "cash":bank["policy"]["initialCash"],"realizedPnl":"0.000000",
                "cumulativeBuys":"0.000000","positions":{},"pending":{},"outcomes":[],
                "fills":[],"roundTrips":[],"gateCounts":{},"signals":{},"cooldowns":{},
                "closedTrades":0,"nextOrder":1}));
        }
    }
    Ok(json!({"schemaVersion":1,"bankHash":public_recovery_hash(bank),
        "lastEventId":bank["startEventId"],"lastAvailableAt":null,"histories":{},
        "recoveryPoints":{},"seen":{},"latest":{},"invalid":{},"warmBooks":0,
        "observations":0,"wallets":wallets}))
}

fn checked_book<'a>(e: &'a V, bank: &V) -> Result<&'a V, String> {
    let b = &e["book"];
    let m = bank["markets"].as_array()
        .and_then(|ms| ms.iter().find(|x| x["marketId"] == e["marketId"]));
    need(m.is_some()
        && e["tournamentId"] == bank["tournamentId"]
        && e["exchangeId"] == m.unwrap()["exchangeId"]
        && b["marketId"] == m.unwrap()["marketId"]
        && b["exchangeId"] == m.unwrap()["exchangeId"]
        && b["instrument"].as_str().unwrap_or("")
            == format!("{}:YES", m.unwrap()["exchangeId"].as_str().unwrap_or("")),
        "PUBLIC_RECOVERY_BOOK_SCOPE")?;
    big_sequence(b["sequence"].as_str().unwrap_or(""))?;
    instant_ns_s(&b["sourceAt"])?;
    let version_key = format!("{}:{}", b["sequence"].as_str().unwrap_or(""),
        b["sourceAt"].as_str().unwrap_or(""));
    need(b["versionKey"].as_str() == Some(version_key.as_str()),
        "PUBLIC_RECOVERY_VERSION")?;
    for side in ["bids", "asks"] {
        need(b[side].as_array().is_some_and(|a| !a.is_empty()), "PUBLIC_RECOVERY_DEPTH")?;
        let mut previous: Option<BigInt> = None;
        for l in b[side].as_array().cloned().unwrap_or_default() {
            let p = big_price(l["price"].as_str().unwrap_or(""))?;
            need(js_int(&l["quantity"]).is_some_and(|q| q > 0), "PUBLIC_RECOVERY_QUANTITY")?;
            if let Some(prev) = &previous {
                need(if side == "bids" { p < *prev } else { p > *prev },
                    "PUBLIC_RECOVERY_DEPTH_ORDER")?;
            }
            previous = Some(p);
        }
    }
    need(big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?
        < big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))?,
        "PUBLIC_RECOVERY_CROSSED")?;
    need(e["origin"].as_str() == Some("rest_observation"), "PUBLIC_RECOVERY_ORIGIN")?;
    need(e["requestStartedAt"].is_string(), "PUBLIC_RECOVERY_REQUEST_CLOCK")?;
    let received = instant_s(&e["quoteReceivedAt"])?;
    let confirmed = instant_s(&e["confirmedAt"])?;
    let available = instant_s(&e["availableAt"])?;
    need(received <= confirmed + 1000 && confirmed <= available, "PUBLIC_RECOVERY_CLOCK")?;
    if js_truthy(e.get("requestStartedAt")) {
        need(instant_ns_s(&e["requestStartedAt"])? <= instant_ns_s(&e["quoteReceivedAt"])?,
            "PUBLIC_RECOVERY_REQUEST_CLOCK")?;
    }
    if js_truthy(e.get("nextExpiryAt")) { instant_ns_s(&e["nextExpiryAt"])?; }
    Ok(b)
}

fn append_history(s: &mut V, e: &V, p: &V) -> Result<(), String> {
    let b = &e["book"];
    let id = e["marketId"].as_str().unwrap_or("").to_string();
    let minute = instant_s(&e["quoteReceivedAt"])?.div_euclid(60000);
    let mut a: Vec<V> = s["histories"][&id].as_array().cloned().unwrap_or_default();
    // Keep the first accepted version in each minute; broadcast intensity has no weight.
    if !a.iter().any(|x| x["minute"].as_i64() == Some(minute)) {
        a.push(json!({"minute":minute,"receivedAt":e["quoteReceivedAt"],
            "confirmedAt":e["confirmedAt"],"sourceAt":b["sourceAt"],
            "mid":(f64_of_big(&big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?)
                + f64_of_big(&big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))?)) / 2e6,
            "versionKey":b["versionKey"]}));
    }
    a.sort_by(|x, y| x["minute"].as_f64().unwrap_or(f64::NAN)
        .partial_cmp(&y["minute"].as_f64().unwrap_or(f64::NAN))
        .unwrap_or(std::cmp::Ordering::Equal));
    let newest = a.last().map(|x| x["minute"].as_i64().unwrap_or(0)).unwrap_or(0) * 60000;
    let history_ms = p["historyMs"].as_i64().unwrap_or(0);
    let mut kept: Vec<V> = a.into_iter()
        .filter(|x| x["minute"].as_i64().unwrap_or(0) * 60000 >= newest - history_ms).collect();
    if kept.len() > 361 { kept = kept[kept.len() - 361..].to_vec(); }
    s["histories"][&id] = json!(kept);
    Ok(())
}

/// publicRecoveryReference(points, at, p)
pub fn public_recovery_reference(points: &V, at: &str, p: &V) -> Result<V, String> {
    let now = instant(at)?;
    let history_ms = p["historyMs"].as_i64().unwrap_or(0);
    let mut a: Vec<V> = Vec::new();
    for x in points.as_array().cloned().unwrap_or_default() {
        if instant_s(&x["receivedAt"])? < now
            && instant_s(&x["confirmedAt"])? <= now
            && instant_ns_s(&x["sourceAt"])? < (now as i128) * 1_000_000
            && x["minute"].as_f64().unwrap_or(f64::NAN) * 60000.0 >= (now - history_ms) as f64
        {
            a.push(x);
        }
    }
    if (a.len() as i64) < p["minimumMinutes"].as_i64().unwrap_or(0) {
        return Ok(json!({"ready":false,"reason":"insufficient_minutes","samples":a.len()}));
    }
    let first = instant_s(&a[0]["receivedAt"])?;
    let last = instant_s(&a.last().unwrap()["receivedAt"])?;
    if last - first < p["minimumSpanMs"].as_i64().unwrap_or(0) {
        return Ok(json!({"ready":false,"reason":"insufficient_history_span","samples":a.len()}));
    }
    if now - last > p["maximumHistoryAgeMs"].as_i64().unwrap_or(0) {
        return Ok(json!({"ready":false,"reason":"historical_reference_stale","samples":a.len()}));
    }
    let mids: Vec<f64> = a.iter().map(|x| x["mid"].as_f64().unwrap_or(f64::NAN)).collect();
    let center = median(&mids);
    let mad = median(&a.iter()
        .map(|x| (x["mid"].as_f64().unwrap_or(f64::NAN) - center).abs())
        .collect::<Vec<f64>>());
    Ok(json!({"ready":true,"source":"minute_balanced_prior_verified_book_midpoints",
        "samples":a.len(),"center":center,
        "robustDeviation":f64::max(0.0025, mad * 1.4826)}))
}

/// warmPublicRecovery(input, books, bank)
pub fn warm_public_recovery(input: &V, books: &V, bank: &V) -> Result<V, String> {
    check_bank(bank)?;
    need(input["bankHash"].as_str() == Some(public_recovery_hash(bank).as_str())
        && input["lastEventId"] == bank["startEventId"]
        && !js_truthy(input.get("lastAvailableAt")), "PUBLIC_RECOVERY_WARM_AFTER_START")?;
    let mut s = input.clone();
    for e in books.as_array().cloned().unwrap_or_default() {
        checked_book(&e, bank)?;
        need(instant_s(&e["confirmedAt"])? <= instant_s(&bank["cutoff"])?
            && instant_s(&e["quoteReceivedAt"])? <= instant_s(&bank["cutoff"])?,
            "PUBLIC_RECOVERY_FUTURE_WARM")?;
        append_history(&mut s, &e, &bank["policy"])?;
        let id = e["marketId"].as_str().unwrap_or("").to_string();
        let points = if js_truthy(s["recoveryPoints"].get(id.as_str())) {
            s["recoveryPoints"][&id].clone()
        } else { json!([]) };
        s["recoveryPoints"][&id] = observe_recovery(&points, &e, &bank["confirmationPolicy"])?;
        s["warmBooks"] = json!(s["warmBooks"].as_i64().unwrap_or(0) + 1);
    }
    Ok(s)
}

fn resolve(w: &mut V, id: &str, at: &str, status: &str) {
    if w["pending"][id].is_null() { return; }
    let order = w["pending"][id].clone();
    if let Some(o) = w["outcomes"].as_array_mut()
        .and_then(|a| a.iter_mut().find(|x| x["id"] == order["id"]))
    {
        o["status"] = json!(status);
        o["resolvedAt"] = json!(at);
        o["unfilledQuantity"] = order["remaining"].clone();
        if status == "expired" {
            o["censoring"] = json!(if o["independentReceipts"].as_i64() == Some(0) {
                "no_independent_in_window_quote"
            } else { "observed_without_full_execution" });
        }
    }
    w["pending"].as_object_mut().map(|p| p.remove(id));
}

fn assert_wallet(w: &V, p: &V) -> Result<(), String> {
    let mut cost = BigInt::from(0);
    for (_, pos) in js_entries(&w["positions"]) {
        need(js_int(&pos["quantity"]).is_some_and(|q| q > 0)
            && big_amount(pos["cost"].as_str().unwrap_or(""))? > BigInt::from(0),
            "PUBLIC_RECOVERY_POSITION")?;
        cost += big_amount(pos["cost"].as_str().unwrap_or(""))?;
    }
    need(big_amount(w["cash"].as_str().unwrap_or(""))? >= BigInt::from(0)
        && big_amount(w["cash"].as_str().unwrap_or(""))? + &cost
            - big_amount(p["initialCash"].as_str().unwrap_or(""))?
            == big_amount(w["realizedPnl"].as_str().unwrap_or(""))?,
        "PUBLIC_RECOVERY_ACCOUNTING")?;
    need(cost <= big_amount(p["maxExposure"].as_str().unwrap_or(""))?
        && big_amount(w["cumulativeBuys"].as_str().unwrap_or(""))?
            <= big_amount(p["cumulativeBuyBudget"].as_str().unwrap_or(""))?,
        "PUBLIC_RECOVERY_BUDGET")?;
    Ok(())
}

fn fill_pending(w: &mut V, e: &V, p: &V) -> Result<(), String> {
    let id = e["marketId"].as_str().unwrap_or("");
    if w["pending"][id].is_null() { return Ok(()); }
    let mut o = w["pending"][id].clone();
    let b = e["book"].clone();
    let receipt = instant_s(&e["quoteReceivedAt"])?;
    let confirmed = instant_s(&e["confirmedAt"])?;
    let independent = big_sequence(b["sequence"].as_str().unwrap_or(""))?
        > big_sequence(o["sequence"].as_str().unwrap_or(""))?
        && instant_ns_s(&b["sourceAt"])? > instant_ns_s(&o["sourceAt"])?;
    if !independent
        || receipt < instant_s(&o["at"])? + w["latencyMs"].as_i64().unwrap_or(0)
        || receipt >= instant_s(&o["expiresAt"])?
        || confirmed >= instant_s(&o["expiresAt"])?
        || !js_truthy(e["continuity"].get("ready"))
        || !js_truthy(e["continuity"].get("continuous"))
        || (js_truthy(e.get("nextExpiryAt"))
            && instant_ns_s(&e["nextExpiryAt"])? <= instant_ns_s(&e["confirmedAt"])?)
    {
        return Ok(());
    }
    let outcome_idx = w["outcomes"].as_array()
        .and_then(|a| a.iter().position(|x| x["id"] == o["id"])).unwrap();
    w["outcomes"][outcome_idx]["independentReceipts"] =
        num_json(w["outcomes"][outcome_idx]["independentReceipts"].as_f64().unwrap_or(0.0) + 1.0);
    let levels = if o["side"].as_str() == Some("buy") { &b["asks"] } else { &b["bids"] };
    let limit = big_amount(o["limit"].as_str().unwrap_or(""))?;
    let mut remaining = o["remaining"].as_f64().unwrap_or(0.0);
    let tick = BigInt::from(TICK);
    for l in levels.as_array().cloned().unwrap_or_default() {
        let raw = big_amount(l["price"].as_str().unwrap_or(""))?;
        if if o["side"].as_str() == Some("buy") { raw > limit } else { raw < limit } { break; }
        let depth = (l["quantity"].as_f64().unwrap_or(0.0)
            * p["depthFractionBps"].as_f64().unwrap_or(0.0) / 10000.0).floor();
        let qty = remaining.min(depth);
        if qty == 0.0 { continue; }
        // This is a counterfactual executable opportunity, not queue priority or a real fill.
        let qty_big = BigInt::from(qty as i64);
        let execution = if o["side"].as_str() == Some("buy") {
            if &raw + &tick < limit { &raw + &tick } else { limit.clone() }
        } else if &raw - &tick > limit { &raw - &tick } else { limit.clone() };
        let notional = &execution * &qty_big;
        let mut gain = BigInt::from(0);
        if o["side"].as_str() == Some("buy") {
            need(big_amount(w["cash"].as_str().unwrap_or(""))? >= notional, "PUBLIC_RECOVERY_CASH")?;
            let mut pos = if w["positions"][id].is_null() {
                json!({"quantity":0,"cost":"0.000000","openedAt":e["confirmedAt"],
                    "realizedPnl":"0.000000","enteredQuantity":0,"entryCost":"0.000000"})
            } else { w["positions"][id].clone() };
            pos["enteredQuantity"] =
                num_json(pos["enteredQuantity"].as_f64().unwrap_or(0.0) + qty);
            pos["entryCost"] = json!(money(
                &(big_amount(pos["entryCost"].as_str().unwrap_or(""))? + &notional)));
            pos["quantity"] = num_json(pos["quantity"].as_f64().unwrap_or(0.0) + qty);
            pos["cost"] = json!(money(
                &(big_amount(pos["cost"].as_str().unwrap_or(""))? + &notional)));
            w["positions"][id] = pos;
            w["cash"] = json!(money(&(big_amount(w["cash"].as_str().unwrap_or(""))? - &notional)));
            w["cumulativeBuys"] = json!(money(
                &(big_amount(w["cumulativeBuys"].as_str().unwrap_or(""))? + &notional)));
        } else {
            need(!w["positions"][id].is_null()
                && w["positions"][id]["quantity"].as_f64().unwrap_or(0.0) >= qty,
                "PUBLIC_RECOVERY_OVERSELL")?;
            let mut pos = w["positions"][id].clone();
            let pos_qty = pos["quantity"].as_f64().unwrap_or(0.0);
            let released = if qty == pos_qty {
                big_amount(pos["cost"].as_str().unwrap_or(""))?
            } else {
                big_amount(pos["cost"].as_str().unwrap_or(""))? * &qty_big
                    / BigInt::from(pos_qty as i64)
            };
            gain = &notional - &released;
            pos["quantity"] = num_json(pos_qty - qty);
            pos["cost"] = json!(money(
                &(big_amount(pos["cost"].as_str().unwrap_or(""))? - &released)));
            pos["realizedPnl"] = json!(money(
                &(big_amount(pos["realizedPnl"].as_str().unwrap_or(""))? + &gain)));
            w["cash"] = json!(money(&(big_amount(w["cash"].as_str().unwrap_or(""))? + &notional)));
            w["realizedPnl"] = json!(money(
                &(big_amount(w["realizedPnl"].as_str().unwrap_or(""))? + &gain)));
            if pos["quantity"].as_f64().unwrap_or(0.0) == 0.0 {
                w["roundTrips"].as_array_mut().map(|a| a.push(json!({"marketId":id,
                    "openedAt":pos["openedAt"],"closedAt":e["confirmedAt"],
                    "quantity":pos["enteredQuantity"],"entryCost":pos["entryCost"],
                    "realizedPnl":pos["realizedPnl"]})));
                w["positions"].as_object_mut().map(|x| x.remove(id));
                w["closedTrades"] = json!(w["closedTrades"].as_i64().unwrap_or(0) + 1);
                w["cooldowns"][id] = json!(ms_to_iso(
                    (confirmed + p["cooldownMs"].as_i64().unwrap_or(0)) as f64));
            } else {
                w["positions"][id] = pos;
            }
        }
        w["fills"].as_array_mut().map(|a| a.push(json!({"orderId":o["id"],"marketId":id,
            "side":o["side"],"quantity":num_json(qty),"price":money(&execution),
            "realizedPnl":money(&gain),"fee":"0.000000","sourceVersion":b["versionKey"],
            "quoteReceivedAt":e["quoteReceivedAt"],"confirmedAt":e["confirmedAt"],
            "gatewayAvailableAt":e["availableAt"],
            "evidence":"counterfactual_executable_opportunity"})));
        remaining -= qty;
        o["remaining"] = num_json(remaining);
        w["outcomes"][outcome_idx]["filledQuantity"] = num_json(
            w["outcomes"][outcome_idx]["filledQuantity"].as_f64().unwrap_or(0.0) + qty);
        if remaining == 0.0 { break; }
    }
    // One accepted version consumes depth once, including partial fills.
    o["sequence"] = b["sequence"].clone();
    o["sourceAt"] = b["sourceAt"].clone();
    w["pending"][id] = o;
    if remaining == 0.0 {
        resolve(w, id, e["confirmedAt"].as_str().unwrap_or(""), "filled");
    }
    assert_wallet(w, p)
}

fn fresh(s: &V, e: &V, now: i64, p: &V) -> Result<bool, String> {
    Ok(js_truthy(e.get("decisionEligible"))
        && !js_truthy(s["invalid"].get(e["marketId"].as_str().unwrap_or("")))
        && js_truthy(e["continuity"].get("ready"))
        && js_truthy(e["continuity"].get("continuous"))
        && now - instant_s(&e["quoteReceivedAt"])? >= 0
        && now - instant_s(&e["quoteReceivedAt"])? < p["maximumQuoteAgeMs"].as_i64().unwrap_or(0)
        && (!js_truthy(e.get("nextExpiryAt"))
            || instant_ns_s(&e["nextExpiryAt"])? > (now as i128) * 1_000_000))
}

fn submit(w: &mut V, e: &V, at: &str, side: &str, quantity: f64, limit: &str, p: &V, reason: &str)
    -> Result<(), String>
{
    let id = e["marketId"].as_str().unwrap_or("");
    let next = w["nextOrder"].as_i64().unwrap_or(0);
    w["nextOrder"] = json!(next + 1);
    let order_id = format!("recovery-{}-{}-{}", w["variant"]["id"].as_str().unwrap_or(""),
        w["latencyMs"].as_i64().unwrap_or(0), next);
    let o = json!({"id":order_id,"side":side,"quantity":num_json(quantity),
        "remaining":num_json(quantity),"limit":limit,"at":at,
        "expiresAt":ms_to_iso((instant(at)? + p["entryLifetimeMs"].as_i64().unwrap_or(0)) as f64),
        "sequence":e["book"]["sequence"],"sourceAt":e["book"]["sourceAt"],"reason":reason});
    w["pending"][id] = o.clone();
    let mut outcome = o.clone();
    outcome["marketId"] = json!(id);
    outcome["status"] = json!("pending");
    outcome["filledQuantity"] = json!(0);
    outcome["independentReceipts"] = json!(0);
    w["outcomes"].as_array_mut().map(|a| a.push(outcome));
    Ok(())
}

fn decide(s: &V, w: &mut V, e: &V, at: &str, bank: &V) -> Result<(), String> {
    let id = e["marketId"].as_str().unwrap_or("").to_string();
    let mut p = bank["policy"].clone();
    for (k, v) in w["variant"]["payoff"].as_object().cloned().unwrap_or_default() {
        p[k.as_str()] = v;
    }
    let b = &e["book"];
    let now = instant(at)?;
    if !fresh(s, e, now, &p)? {
        inc(&mut w["gateCounts"], "stale_or_invalid");
        return Ok(());
    }
    if js_truthy(w["pending"].get(id.as_str())) { return Ok(()); }
    let bid = big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?;
    let ask = big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))?;
    let tick = BigInt::from(TICK);
    if !w["positions"][&id].is_null() {
        let pos = w["positions"][&id].clone();
        let per = big_amount(pos["cost"].as_str().unwrap_or(""))?
            / BigInt::from(pos["quantity"].as_i64().unwrap_or(0));
        let exit = &bid - &tick;
        if &exit - &per >= big_amount(p["takeProfit"].as_str().unwrap_or(""))?
            || &per - &exit >= big_amount(p["stopLoss"].as_str().unwrap_or(""))?
            || now - instant_s(&pos["openedAt"])? >= p["maximumHoldMs"].as_i64().unwrap_or(0)
        {
            submit(w, e, at, "sell", pos["quantity"].as_f64().unwrap_or(0.0),
                &money(&(if exit > BigInt::from(0) { exit } else { tick.clone() })),
                &p, "profit_loss_or_hold_exit")?;
        }
        return Ok(());
    }
    let reference = public_recovery_reference(&s["histories"][&id], at, &p)?;
    let ask_f = f64_of_big(&ask) / 1e6;
    let padded_ask_f = f64_of_big(&(&ask + &tick)) / 1e6;
    let mut sig = reference.clone();
    sig["at"] = json!(at);
    sig["rawAsk"] = json!(money(&ask));
    sig["rawBid"] = json!(money(&bid));
    sig["paddedAsk"] = json!(money(&(&ask + &tick)));
    sig["paddedBid"] = json!(money(&(&bid - &tick)));
    sig["rawSpread"] = json!(f64_of_big(&(&ask - &bid)) / 1e6);
    sig["paddedSpread"] = json!(f64_of_big(&(&ask - &bid + &tick * 2)) / 1e6);
    if js_truthy(reference.get("ready")) {
        sig["paddedDiscount"] =
            json!(reference["center"].as_f64().unwrap_or(f64::NAN) - padded_ask_f);
        sig["conditionalDiscount"] =
            json!(reference["center"].as_f64().unwrap_or(f64::NAN) - ask_f);
    }
    w["signals"][&id] = sig;
    if !js_truthy(reference.get("ready")) {
        inc(&mut w["gateCounts"], reference["reason"].as_str().unwrap_or(""));
        return Ok(());
    }
    let cooldown_end = w["cooldowns"][&id].as_str().and_then(parse_iso_ms);
    if cooldown_end.map_or(false, |x| x > now as f64) {
        inc(&mut w["gateCounts"], "cooldown");
        return Ok(());
    }
    let mut occupied = BTreeSet::new();
    for (k, _) in js_entries(&w["positions"]) { occupied.insert(k); }
    for (k, _) in js_entries(&w["pending"]) { occupied.insert(k); }
    if occupied.len() >= p["maxOpenMarkets"].as_i64().unwrap_or(0) as usize {
        inc(&mut w["gateCounts"], "market_count");
        return Ok(());
    }
    let identity = bank["markets"].as_array()
        .and_then(|ms| ms.iter().find(|x| x["marketId"].as_str() == Some(id.as_str())))
        .cloned().unwrap_or(V::Null);
    let others: Vec<V> = occupied.iter().map(|x| bank["markets"].as_array()
        .and_then(|ms| ms.iter().find(|m| m["marketId"].as_str() == Some(x.as_str())))
        .cloned().unwrap_or(V::Null)).collect();
    if !js_truthy(identity.get("state")) || !js_truthy(identity.get("office")) {
        if others.iter().any(|x| !js_truthy(x.get("state")) || !js_truthy(x.get("office"))) {
            inc(&mut w["gateCounts"], "unknown_identity_concentration");
            return Ok(());
        }
    }
    if js_truthy(identity.get("state"))
        && others.iter().filter(|x| x["state"] == identity["state"]).count() >= 2
    {
        inc(&mut w["gateCounts"], "state_concentration");
        return Ok(());
    }
    if instant_s(&e["quoteReceivedAt"])? <= instant_s(&bank["cutoff"])? {
        inc(&mut w["gateCounts"], "pre_cutoff_quote");
        return Ok(());
    }
    let center = reference["center"].as_f64().unwrap_or(f64::NAN);
    let robust_dev = reference["robustDeviation"].as_f64().unwrap_or(f64::NAN);
    let bid_value = f64_of_big(&(&bid - &tick)) / 1e6;
    let admitted = |candidate: &BigInt| -> bool {
        let a = f64_of_big(candidate) / 1e6;
        let d = center - a;
        a >= p["minimumPrice"].as_f64().unwrap_or(f64::NAN)
            && a <= p["maximumPrice"].as_f64().unwrap_or(f64::NAN)
            && a > bid_value
            && a - bid_value <= p["maximumSpread"].as_f64().unwrap_or(f64::NAN) + 1e-12
            && d >= p["minimumDip"].as_f64().unwrap_or(f64::NAN) - 1e-12
            && d <= p["maximumDip"].as_f64().unwrap_or(f64::NAN)
            && d / f64::max(center, 0.001) >= p["minimumRelativeDip"].as_f64().unwrap_or(f64::NAN)
            && d / robust_dev >= p["minimumZ"].as_f64().unwrap_or(f64::NAN)
    };
    let points = if js_truthy(s["recoveryPoints"].get(id.as_str())) {
        s["recoveryPoints"][&id].clone()
    } else { json!([]) };
    let confirmation = recovery_confirmation(&points, e, at, &bank["confirmationPolicy"])?;
    w["signals"][&id]["recovery"] = confirmation.clone();
    if w["variant"]["entry"].as_str() != Some("baseline") && !js_truthy(confirmation.get("admitted"))
    {
        inc(&mut w["gateCounts"], confirmation["reason"].as_str().unwrap_or(""));
        return Ok(());
    }
    let entry = w["variant"]["entry"].as_str().unwrap_or("");
    let mut limit: Option<BigInt> = None;
    if entry == "pricecap" {
        if ask > tick && admitted(&ask) { limit = Some(&ask - &tick); }
    } else if admitted(&(&ask + &tick)) {
        limit = Some(ask.clone());
    } else if entry == "baseline" && ask > tick && admitted(&ask) {
        limit = Some(&ask - &tick);
    }
    // JS `if(limit&&...)`: limit is a nonzero BigInt on every reachable path.
    if limit.is_some() && entry != "baseline"
        && limit.clone().unwrap() - (&bid - &tick.clone())
            >= big_amount(p["stopLoss"].as_str().unwrap_or(""))?
    {
        inc(&mut w["gateCounts"], "insufficient_exit_headroom");
        return Ok(());
    }
    if limit.is_none() {
        inc(&mut w["gateCounts"], "discount_spread_or_regime_gate");
        let a = ask_f;
        let discount = center - a;
        inc(&mut w["gateCounts"], if a < p["minimumPrice"].as_f64().unwrap_or(f64::NAN)
            || a > p["maximumPrice"].as_f64().unwrap_or(f64::NAN) { "price_band" }
            else if f64_of_big(&(&ask - &bid + &tick)) / 1e6
                > p["maximumSpread"].as_f64().unwrap_or(f64::NAN) + 1e-12 { "spread" }
            else if discount < p["minimumDip"].as_f64().unwrap_or(f64::NAN) - 1e-12 {
                "minimum_discount"
            } else if discount > p["maximumDip"].as_f64().unwrap_or(f64::NAN) { "regime_change" }
            else if discount / f64::max(center, 0.001)
                < p["minimumRelativeDip"].as_f64().unwrap_or(f64::NAN) { "relative_discount" }
            else { "robust_z" });
        return Ok(());
    }
    let limit = limit.unwrap();
    let mut reserved = BigInt::from(0);
    for (_, x) in js_entries(&w["pending"]) {
        if x["side"].as_str() == Some("buy") {
            reserved += big_amount(x["limit"].as_str().unwrap_or(""))?
                * BigInt::from(x["remaining"].as_i64().unwrap_or(0));
        }
    }
    let budget = big_amount(p["cumulativeBuyBudget"].as_str().unwrap_or(""))?
        - big_amount(w["cumulativeBuys"].as_str().unwrap_or(""))? - &reserved;
    let qty = [
        BigInt::from(p["orderShares"].as_i64().unwrap_or(0)),
        big_amount(p["maxMarketCost"].as_str().unwrap_or(""))? / &limit,
        &budget / &limit,
        (big_amount(w["cash"].as_str().unwrap_or(""))? - &reserved) / &limit,
    ].into_iter().min().unwrap_or(BigInt::from(0));
    if qty < BigInt::from(1) {
        inc(&mut w["gateCounts"], "budget");
        return Ok(());
    }
    let qty_i = qty.to_string().parse::<i64>().unwrap_or(0);
    submit(w, e, at, "buy", qty_i as f64, &money(&limit), &p,
        if limit == ask { "native_dip" } else { "conditional_one_tick_lower" })?;
    inc(&mut w["gateCounts"], "admitted");
    Ok(())
}

/// advancePublicRecovery(input, events, bank, {availableAt,evidenceThrough,allowDecisions})
pub fn advance_public_recovery(input: &V, events: &V, bank: &V, options: &V) -> Result<V, String> {
    check_bank(bank)?;
    need(input["bankHash"].as_str() == Some(public_recovery_hash(bank).as_str()),
        "PUBLIC_RECOVERY_BANK_CHANGED")?;
    let available_at = options["availableAt"].as_str().unwrap_or("");
    let evidence_through = match options.get("evidenceThrough") {
        None => available_at,
        Some(v) => v.as_str().unwrap_or(""),
    };
    let allow_decisions = options["allowDecisions"].as_bool().unwrap_or(true);
    let now = instant(available_at)?;
    let watermark = instant(evidence_through)?;
    need(watermark <= now
        && (!js_truthy(input.get("lastEvidenceThrough"))
            || watermark >= instant_s(&input["lastEvidenceThrough"])?)
        && (options.get("allowDecisions").is_none() || options["allowDecisions"].is_boolean()),
        "PUBLIC_RECOVERY_EVIDENCE_CLOCK")?;
    need(now >= instant_s(&bank["cutoff"])?
        && (!js_truthy(input.get("lastAvailableAt"))
            || now >= instant_s(&input["lastAvailableAt"])?), "PUBLIC_RECOVERY_HOST_CLOCK")?;
    let mut s = input.clone();
    let mut last: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    for e in events.as_array().cloned().unwrap_or_default() {
        need(js_int(&e["eventId"]) == js_int(&s["lastEventId"]).map(|x| x + 1)
            && instant_s(&e["availableAt"])? <= now
            && instant_s(&e["confirmedAt"])? <= now, "PUBLIC_RECOVERY_EVENT_ORDER")?;
        need(e["tournamentId"] == bank["tournamentId"]
            && bank["markets"].as_array().is_some_and(|ms| ms.iter()
                .any(|x| x["marketId"] == e["marketId"] && x["exchangeId"] == e["exchangeId"])),
            "PUBLIC_RECOVERY_SCOPE")?;
        s["lastEventId"] = e["eventId"].clone();
        if e["kind"].as_str() == Some("invalidate") {
            s["invalid"][e["marketId"].as_str().unwrap_or("")] = json!(true);
            s["latest"].as_object_mut()
                .map(|m| m.remove(e["marketId"].as_str().unwrap_or("")));
            last.shift_remove(e["marketId"].as_str().unwrap_or(""));
            let wallet_keys: Vec<String> = s["wallets"].as_object()
                .map(|w| w.keys().cloned().collect()).unwrap_or_default();
            for key in wallet_keys {
                let mut w = s["wallets"][&key].clone();
                resolve(&mut w, e["marketId"].as_str().unwrap_or(""),
                    e["confirmedAt"].as_str().unwrap_or(""), "invalidated");
                s["wallets"][&key] = w;
            }
            continue;
        }
        need(e["kind"].as_str() == Some("book"), "PUBLIC_RECOVERY_EVENT_KIND")?;
        let b = checked_book(&e, bank)?.clone();
        let old = s["seen"][e["marketId"].as_str().unwrap_or("")].clone();
        let content_hash = public_recovery_hash(&json!({"bids":b["bids"],"asks":b["asks"]}));
        if !old.is_null() {
            need(big_sequence(b["sequence"].as_str().unwrap_or(""))?
                >= big_sequence(old["sequence"].as_str().unwrap_or(""))?
                && instant_ns_s(&b["sourceAt"])? >= instant_ns_s(&old["sourceAt"])?,
                "PUBLIC_RECOVERY_SOURCE_ROLLBACK")?;
            if b["sequence"] == old["sequence"] {
                need(b["sourceAt"] == old["sourceAt"]
                    && json!(content_hash.clone()) == old["contentHash"],
                    "PUBLIC_RECOVERY_SAME_VERSION_CHANGED")?;
            }
        }
        s["seen"][e["marketId"].as_str().unwrap_or("")] = json!({"sequence":b["sequence"],
            "sourceAt":b["sourceAt"],"contentHash":content_hash});
        s["invalid"].as_object_mut().map(|m| m.remove(e["marketId"].as_str().unwrap_or("")));
        s["latest"][e["marketId"].as_str().unwrap_or("")] = e.clone();
        if old.is_null() || b["sequence"] != old["sequence"] {
            s["observations"] = json!(s["observations"].as_i64().unwrap_or(0) + 1);
        }
        let wallet_keys: Vec<String> = s["wallets"].as_object()
            .map(|w| w.keys().cloned().collect()).unwrap_or_default();
        for key in wallet_keys {
            let mut w = s["wallets"][&key].clone();
            fill_pending(&mut w, &e, &bank["policy"])?;
            s["wallets"][&key] = w;
        }
        let id = e["marketId"].as_str().unwrap_or("").to_string();
        let points = if js_truthy(s["recoveryPoints"].get(id.as_str())) {
            s["recoveryPoints"][&id].clone()
        } else { json!([]) };
        s["recoveryPoints"][&id] = observe_recovery(&points, &e, &bank["confirmationPolicy"])?;
        last.insert(id, e.clone());
    }
    // Resolve the entire ordered evidence batch before expiring old orders or making
    // new decisions. A delayed consumer cannot erase an adverse pre-expiry fill.
    let wallet_keys: Vec<String> = s["wallets"].as_object()
        .map(|w| w.keys().cloned().collect()).unwrap_or_default();
    for key in wallet_keys {
        let mut w = s["wallets"][&key].clone();
        let pending_snapshot = js_entries(&w["pending"]);
        for (pid, o) in pending_snapshot {
            if watermark >= instant_s(&o["expiresAt"])? {
                resolve(&mut w, &pid, available_at, "expired");
            }
        }
        if allow_decisions {
            for (_, e) in last.iter() {
                decide(&s, &mut w, e, available_at, bank)?;
            }
        }
        assert_wallet(&w, &bank["policy"])?;
        s["wallets"][&key] = w;
    }
    for (_, e) in last.iter() {
        append_history(&mut s, e, &bank["policy"])?;
    }
    s["lastAvailableAt"] = json!(available_at);
    s["lastEvidenceThrough"] = json!(evidence_through);
    Ok(s)
}

// streamDipReferenceForReport(points, at, p) — report's ready-only reference probe.
fn stream_dip_reference_for_report(points: &V, at: &str, p: &V) -> bool {
    public_recovery_reference(points, at, p)
        .map(|r| js_truthy(r.get("ready"))).unwrap_or(false)
}

/// publicRecoveryReport(s, bank, at)
pub fn public_recovery_report(s: &V, bank: &V, at: &str) -> Result<V, String> {
    need(s["bankHash"].as_str() == Some(public_recovery_hash(bank).as_str()),
        "PUBLIC_RECOVERY_REPORT_BANK")?;
    let now = instant(at)?;
    let usable = bank["markets"].as_array().map(|ms| ms.iter()
        .filter(|m| stream_dip_reference_for_report(
            &s["histories"][m["marketId"].as_str().unwrap_or("")], at, &bank["policy"]))
        .count()).unwrap_or(0);
    let mut wallets = serde_json::Map::new();
    for (name, w) in s["wallets"].as_object().map(|m| m.iter()).into_iter().flatten() {
        let mut liquidation = BigInt::from(0);
        let mut complete = true;
        let mut positions: Vec<V> = Vec::new();
        for (id, pos) in js_entries(&w["positions"]) {
            let e = s["latest"][&id].clone();
            let ready = !e.is_null() && fresh(s, &e, now, &bank["policy"])?;
            let mut mark = V::Null;
            if ready {
                let mut q = pos["quantity"].as_f64().unwrap_or(0.0);
                let mut value = BigInt::from(0);
                for l in e["book"]["bids"].as_array().cloned().unwrap_or_default() {
                    let n = q.min((l["quantity"].as_f64().unwrap_or(0.0)
                        * bank["policy"]["depthFractionBps"].as_f64().unwrap_or(0.0)
                        / 10000.0).floor());
                    let lp = big_amount(l["price"].as_str().unwrap_or(""))?;
                    value += (if lp > BigInt::from(TICK) { lp - BigInt::from(TICK) }
                        else { BigInt::from(0) }) * BigInt::from(n as i64);
                    q -= n;
                    if q == 0.0 { break; }
                }
                if q == 0.0 { mark = json!(money(&value)); }
            }
            if mark.is_null() { complete = false; } else {
                liquidation += big_amount(mark.as_str().unwrap_or(""))?;
            }
            let mut entry = serde_json::Map::new();
            entry.insert("marketId".to_string(), json!(id));
            for (k, v) in pos.as_object().cloned().unwrap_or_default() { entry.insert(k, v); }
            entry.insert("liquidationValue".to_string(), mark.clone());
            entry.insert("markReady".to_string(), json!(!mark.is_null()));
            entry.insert("quoteReceivedAt".to_string(),
                if e.is_null() { V::Null } else { e["quoteReceivedAt"].clone() });
            entry.insert("quoteInvalidated".to_string(), json!(!ready));
            entry.insert("observationAgeMs".to_string(), if e.is_null() { V::Null } else {
                json!(now - instant_s(&e["quoteReceivedAt"])?)
            });
            positions.push(V::Object(entry));
        }
        let trips = w["roundTrips"].as_array().cloned().unwrap_or_default();
        let wins: Vec<V> = trips.iter()
            .filter(|x| big_amount(x["realizedPnl"].as_str().unwrap_or(""))
                .map(|v| v > BigInt::from(0)).unwrap_or(false))
            .cloned().collect();
        let losses: Vec<V> = trips.iter()
            .filter(|x| big_amount(x["realizedPnl"].as_str().unwrap_or(""))
                .map(|v| v < BigInt::from(0)).unwrap_or(false))
            .cloned().collect();
        let gross_wins: BigInt = wins.iter()
            .map(|x| big_amount(x["realizedPnl"].as_str().unwrap_or(""))
                .unwrap_or(BigInt::from(0))).sum();
        let gross_losses: BigInt = losses.iter()
            .map(|x| BigInt::from(0) - big_amount(x["realizedPnl"].as_str().unwrap_or(""))
                .unwrap_or(BigInt::from(0))).sum();
        let pending_arr: Vec<V> = js_entries(&w["pending"]).iter().map(|(market_id, o)| {
            let mut entry = serde_json::Map::new();
            entry.insert("marketId".to_string(), json!(market_id));
            for (k, v) in o.as_object().cloned().unwrap_or_default() { entry.insert(k, v); }
            V::Object(entry)
        }).collect();
        let mut clock_expired = 0usize;
        for (_, o) in js_entries(&w["pending"]) {
            if now >= instant_s(&o["expiresAt"])? { clock_expired += 1; }
        }
        let fills: Vec<V> = w["fills"].as_array().cloned().unwrap_or_default();
        let outcomes: Vec<V> = w["outcomes"].as_array().cloned().unwrap_or_default();
        let sl = big_amount(w["variant"]["payoff"]["stopLoss"].as_str().unwrap_or(""))?;
        let tp = big_amount(w["variant"]["payoff"]["takeProfit"].as_str().unwrap_or(""))?;
        let slice_tail = |a: &[V], n: usize| -> Vec<V> {
            if a.len() > n { a[a.len() - n..].to_vec() } else { a.to_vec() }
        };
        wallets.insert(name.clone(), json!({
            "variant":w["variant"],"latencyMs":w["latencyMs"],"cash":w["cash"],
            "realizedPnl":w["realizedPnl"],"cumulativeBuys":w["cumulativeBuys"],
            "closedTrades":w["closedTrades"],"positions":positions,"pending":pending_arr,
            "closedRoundTrips":{"count":trips.len(),"wins":wins.len(),"losses":losses.len(),
                "flat":trips.len() - wins.len() - losses.len(),
                "winRate":if !trips.is_empty() {
                    json!(wins.len() as f64 / trips.len() as f64) } else { V::Null },
                "grossWins":money(&gross_wins),"grossLosses":money(&gross_losses),
                "profitFactor":if gross_losses != BigInt::from(0) {
                    json!(f64_of_big(&gross_wins) / f64_of_big(&gross_losses)) } else { V::Null },
                "recent":slice_tail(&trips, 30)},
            "idealizedBreakEvenWinRate":f64_of_big(&sl) / f64_of_big(&(&sl + &tp)),
            "totalPnl":if complete {
                json!(money(&(big_amount(w["cash"].as_str().unwrap_or(""))? + &liquidation
                    - big_amount(bank["policy"]["initialCash"].as_str().unwrap_or(""))?)))
            } else { V::Null },
            "marksComplete":complete,
            "clockExpiredAwaitingSource":clock_expired,
            "gateCounts":w["gateCounts"],"signals":w["signals"],
            "orders":outcomes.len(),"fills":fills.len(),
            "recentFills":slice_tail(&fills, 30),
            "outcomes":slice_tail(&outcomes, 50)}));
    }
    Ok(json!({"mode":bank["mode"],"bankHash":s["bankHash"],
        "sourceScopeHash":bank["sourceScopeHash"],"updatedAt":at,"cutoff":bank["cutoff"],
        "lastEventId":s["lastEventId"],"variants":bank["variants"],
        "confirmationPolicy":bank["confirmationPolicy"],
        "configuredMarkets":bank["markets"].as_array().map(|m| m.len()).unwrap_or(0),
        "historyCoverage":{
            "marketsWithBookHistory":s["histories"].as_object().map(|m| m.len()).unwrap_or(0),
            "marketsWithRecoveryContext":s["recoveryPoints"].as_object().map(|m| m.len()).unwrap_or(0),
            "usableHistoricalReferences":usable},
        "warmBooks":s["warmBooks"],"observations":s["observations"],"networkRequests":0,
        "competitionOrders":0,"feeBps":0,"profitabilityEstablished":false,
        "wallets":wallets}))
}
