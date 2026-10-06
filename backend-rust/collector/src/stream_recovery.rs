// live-paper/stream-recovery.mjs — native stream recovery paper engine (exact BigInt micros).
use crate::recovery_confirmation::{
    observe_recovery, recovery_confirmation, recovery_confirmation_policy,
};
use baal_paper::fixed::{big_amount, big_price, big_sequence, instant, money, SCALE, TICK};
use baal_service_runtime::contract::{instant_ns, ms_to_iso};
use baal_service_runtime::json_text::{digest, js_int, js_truthy};
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(code.into()); }
    Ok(())
}
fn truthy(v: &V) -> bool { js_truthy(Some(v)) }
fn inc(o: &mut V, k: &str) {
    let n = o[k].as_i64().unwrap_or(0);
    o[k] = json!(n + 1);
}
fn median(x: &[f64]) -> f64 {
    let mut a = x.to_vec();
    a.sort_by(|p, q| p.partial_cmp(q).unwrap_or(std::cmp::Ordering::Equal));
    let n = a.len();
    if n % 2 == 1 { a[(n - 1) / 2] } else { (a[n / 2 - 1] + a[n / 2]) / 2.0 }
}
/// JS Date.parse -> NaN on unparseable.
fn date_parse(s: &str) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(s).unwrap_or(f64::NAN)
}
fn ns(v: &V) -> Result<i128, String> {
    instant_ns(v.as_str().unwrap_or("")).map_err(|e| e.to_string())
}
/// JS BigInt -> Number (f64).
fn bignum_to_f64(x: &BigInt) -> f64 { x.to_string().parse::<f64>().unwrap_or(f64::INFINITY) }

/// streamRecoveryHash (JSON.stringify sha256, insertion order).
pub fn stream_recovery_hash(v: &V) -> String { digest(v) }
/// RECOVERY_VARIANTS (frozen): 3 entries x 3 payoffs.
pub fn recovery_variants() -> V {
    let payoffs = [
        json!({"name":"legacy","takeProfit":"0.010000","stopLoss":"0.030000"}),
        json!({"name":"balanced","takeProfit":"0.020000","stopLoss":"0.020000"}),
        json!({"name":"reward","takeProfit":"0.030000","stopLoss":"0.020000"}),
    ];
    let mut out = Vec::new();
    for entry in ["baseline", "confirmed", "pricecap"] {
        for payoff in &payoffs {
            out.push(json!({"id":format!("{}_{}",entry,payoff["name"].as_str().unwrap_or("")),
                "entry":entry,"payoff":payoff}));
        }
    }
    V::Array(out)
}
/// STREAM_RECOVERY_POLICY (frozen).
pub fn stream_recovery_policy() -> V {
    json!({"feeBps":0,"historyMs":21600000,"minimumMinutes":8,"minimumSpanMs":1800000,
        "maximumHistoryAgeMs":2700000,"minimumDip":0.02,"minimumRelativeDip":0.03,
        "maximumDip":0.1,"minimumZ":1.5,"maximumSpread":0.02,"minimumPrice":0.05,
        "maximumPrice":0.95,"entryLifetimeMs":30000,"maximumQuoteAgeMs":20000,
        "takeProfit":"0.010000","stopLoss":"0.030000","maximumHoldMs":3600000,
        "cooldownMs":900000,"initialCash":"100.000000","maxMarketCost":"25.000000",
        "maxExposure":"100.000000","cumulativeBuyBudget":"100.000000",
        "maxOpenMarkets":3,"orderShares":10,"depthFractionBps":100,"slippageTicks":1,
        "latenciesMs":[1000,3000]})
}
fn id_ok(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        !s.is_empty() && s.as_bytes()[0] != b'0' && s.bytes().all(|b| b.is_ascii_digit())
    })
}
fn check_bank(b: &V) -> Result<(), String> {
    need(
        b["schemaVersion"].as_i64() == Some(1)
            && b["mode"].as_str() == Some("stream_recovery_paper")
            && b["inputBankHash"].as_str().is_some_and(|s| {
                s.len() == 64 && s.bytes().all(|x| x.is_ascii_digit() || (b'a'..=b'f').contains(&x))
            }),
        "STREAM_RECOVERY_BANK",
    )?;
    instant(b["cutoff"].as_str().unwrap_or(""))?;
    need(
        b["tournamentId"].as_str() == Some("bda92870-621e-47b0-bc3c-3602c5c26f55"),
        "STREAM_RECOVERY_TOURNAMENT",
    )?;
    need(
        js_int(&b["startEventId"]).is_some_and(|x| x >= 0),
        "STREAM_RECOVERY_START_EVENT",
    )?;
    need(
        stream_recovery_hash(&b["policy"]) == stream_recovery_hash(&stream_recovery_policy()),
        "STREAM_RECOVERY_POLICY_CHANGED",
    )?;
    need(
        stream_recovery_hash(&b["variants"]) == stream_recovery_hash(&recovery_variants()),
        "STREAM_RECOVERY_VARIANTS_CHANGED",
    )?;
    let markets = b["markets"].as_array();
    need(
        markets.is_some_and(|m| {
            !m.is_empty() && m.len() <= 4
                && {
                    let mut s = std::collections::BTreeSet::new();
                    m.iter().all(|x| s.insert(x["marketId"].as_str().unwrap_or("").to_string()))
                }
        }),
        "STREAM_RECOVERY_SCOPE",
    )?;
    for m in markets.unwrap() {
        need(id_ok(&m["marketId"]) && id_ok(&m["exchangeId"]), "STREAM_RECOVERY_ID")?;
    }
    Ok(())
}
/// createStreamRecovery(bank)
pub fn create_stream_recovery(bank: &V) -> Result<V, String> {
    check_bank(bank)?;
    let mut wallets = Map::new();
    for variant in bank["variants"].as_array().cloned().unwrap_or_default() {
        for latency in bank["policy"]["latenciesMs"].as_array().cloned().unwrap_or_default() {
            let lm = js_int(&latency).unwrap_or(0);
            wallets.insert(format!("{}_{}", variant["id"].as_str().unwrap_or(""), lm), json!({
                "variant":variant,"latencyMs":lm,"cash":bank["policy"]["initialCash"],
                "realizedPnl":"0.000000","cumulativeBuys":"0.000000","positions":{},
                "pending":{},"outcomes":[],"fills":[],"roundTrips":[],"gateCounts":{},
                "signals":{},"cooldowns":{},"closedTrades":0,"nextOrder":1}));
        }
    }
    Ok(json!({"schemaVersion":1,"bankHash":stream_recovery_hash(bank),
        "lastEventId":bank["startEventId"],"lastAvailableAt":V::Null,
        "histories":{},"recoveryPoints":{},"seen":{},"latest":{},"invalid":{},
        "warmBooks":0,"observations":0,"wallets":wallets}))
}
fn checked_book(e: &V, bank: &V) -> Result<V, String> {
    let b = &e["book"];
    let m = bank["markets"].as_array().cloned().unwrap_or_default()
        .into_iter().find(|x| x["marketId"] == e["marketId"]);
    let m_ok = m.as_ref().is_some_and(|m| {
        e["tournamentId"] == bank["tournamentId"]
            && e["exchangeId"] == m["exchangeId"]
            && b["marketId"] == m["marketId"]
            && b["exchangeId"] == m["exchangeId"]
            && b["instrument"].as_str()
                == Some(format!("{}:YES", m["exchangeId"].as_str().unwrap_or("")).as_str())
    });
    need(m_ok, "STREAM_RECOVERY_BOOK_SCOPE")?;
    big_sequence(b["sequence"].as_str().unwrap_or(""))?;
    ns(&b["sourceAt"])?;
    need(
        b["versionKey"].as_str()
            == Some(format!("{}:{}", b["sequence"].as_str().unwrap_or(""), b["sourceAt"].as_str().unwrap_or("")).as_str()),
        "STREAM_RECOVERY_VERSION",
    )?;
    for side in ["bids", "asks"] {
        let levels = b[side].as_array().cloned().unwrap_or_default();
        need(!levels.is_empty(), "STREAM_RECOVERY_DEPTH")?;
        let mut previous: Option<BigInt> = None;
        for l in &levels {
            let p = big_price(l["price"].as_str().unwrap_or(""))?;
            need(
                js_int(&l["quantity"]).is_some_and(|q| q > 0),
                "STREAM_RECOVERY_QUANTITY",
            )?;
            if let Some(prev) = &previous {
                need(
                    if side == "bids" { p < *prev } else { p > *prev },
                    "STREAM_RECOVERY_DEPTH_ORDER",
                )?;
            }
            previous = Some(p);
        }
    }
    need(
        big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?
            < big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))?,
        "STREAM_RECOVERY_CROSSED",
    )?;
    need(
        ["stream", "rest_recovery", "buffered_stream_recovery"]
            .contains(&e["origin"].as_str().unwrap_or("")),
        "STREAM_RECOVERY_ORIGIN",
    )?;
    need(
        if e["origin"].as_str() == Some("rest_recovery") {
            e["requestStartedAt"].is_string()
        } else {
            e["requestStartedAt"].is_null()
        },
        "STREAM_RECOVERY_REQUEST_CLOCK",
    )?;
    let received = instant(e["quoteReceivedAt"].as_str().unwrap_or(""))?;
    let confirmed = instant(e["confirmedAt"].as_str().unwrap_or(""))?;
    let available = instant(e["availableAt"].as_str().unwrap_or(""))?;
    need(
        received <= confirmed + 1000 && confirmed <= available,
        "STREAM_RECOVERY_CLOCK",
    )?;
    if truthy(&e["requestStartedAt"]) {
        need(ns(&e["requestStartedAt"])? <= ns(&e["quoteReceivedAt"])?, "STREAM_RECOVERY_REQUEST_CLOCK")?;
    }
    if truthy(&e["nextExpiryAt"]) {
        ns(&e["nextExpiryAt"])?;
    }
    Ok(b.clone())
}
fn append_history(s: &mut V, e: &V, p: &V) -> Result<(), String> {
    let b = &e["book"];
    let id = e["marketId"].as_str().unwrap_or("").to_string();
    let minute = instant(e["quoteReceivedAt"].as_str().unwrap_or(""))?.div_euclid(60000);
    let mut a = s["histories"][id.as_str()].as_array().cloned().unwrap_or_default();
    // Keep the first accepted version in each minute; broadcast intensity has
    // no weight.
    if !a.iter().any(|x| x["minute"].as_i64() == Some(minute)) {
        let mid = (bignum_to_f64(&big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?)
            + bignum_to_f64(&big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))?)) / 2e6;
        a.push(json!({"minute":minute,"receivedAt":e["quoteReceivedAt"],
            "confirmedAt":e["confirmedAt"],"sourceAt":b["sourceAt"],
            "mid":mid,"versionKey":b["versionKey"]}));
    }
    a.sort_by(|x, y| x["minute"].as_i64().unwrap_or(0).cmp(&y["minute"].as_i64().unwrap_or(0)));
    let newest = a.last().map(|x| x["minute"].as_i64().unwrap_or(0)).unwrap_or(0) * 60000;
    let window = js_int(&p["historyMs"]).unwrap_or(0);
    let mut kept: Vec<V> = a
        .into_iter()
        .filter(|x| x["minute"].as_i64().unwrap_or(0) * 60000 >= newest - window)
        .collect();
    if kept.len() > 361 {
        kept = kept[kept.len() - 361..].to_vec();
    }
    s["histories"][id] = V::Array(kept);
    Ok(())
}
/// streamRecoveryReference(points,at,p)
pub fn stream_recovery_reference(points: &V, at: &str, p: &V) -> Result<V, String> {
    let now = instant(at)?;
    let history_ms = js_int(&p["historyMs"]).unwrap_or(0);
    let raw = if truthy(points) { points.as_array().cloned().unwrap_or_default() } else { Vec::new() };
    let mut a: Vec<V> = Vec::new();
    for x in raw {
        let received = instant(x["receivedAt"].as_str().unwrap_or(""))?;
        let confirmed = instant(x["confirmedAt"].as_str().unwrap_or(""))?;
        let source_ns = ns(&x["sourceAt"])?;
        let minute_ms = x["minute"].as_i64().unwrap_or(0) * 60000;
        if received < now && confirmed <= now
            && source_ns < now as i128 * 1_000_000
            && minute_ms >= now - history_ms {
            a.push(x.clone());
        }
    }
    if (a.len() as i64) < js_int(&p["minimumMinutes"]).unwrap_or(0) {
        return Ok(json!({"ready":false,"reason":"insufficient_minutes","samples":a.len()}));
    }
    let first = instant(a[0]["receivedAt"].as_str().unwrap_or(""))?;
    let last = instant(a[a.len() - 1]["receivedAt"].as_str().unwrap_or(""))?;
    if last - first < js_int(&p["minimumSpanMs"]).unwrap_or(0) {
        return Ok(json!({"ready":false,"reason":"insufficient_history_span","samples":a.len()}));
    }
    if now - last > js_int(&p["maximumHistoryAgeMs"]).unwrap_or(0) {
        return Ok(json!({"ready":false,"reason":"historical_reference_stale","samples":a.len()}));
    }
    let mids: Vec<f64> = a.iter().map(|x| x["mid"].as_f64().unwrap_or(0.0)).collect();
    let center = median(&mids);
    let deviations: Vec<f64> = mids.iter().map(|m| (m - center).abs()).collect();
    let mad = median(&deviations);
    Ok(json!({"ready":true,"source":"minute_balanced_prior_verified_book_midpoints",
        "samples":a.len(),"center":center,
        "robustDeviation":0.0025_f64.max(mad * 1.4826)}))
}
/// warmStreamRecovery(input,books,bank)
pub fn warm_stream_recovery(input: &V, books: &V, bank: &V) -> Result<V, String> {
    check_bank(bank)?;
    need(
        input["bankHash"].as_str() == Some(stream_recovery_hash(bank).as_str())
            && js_int(&input["lastEventId"]) == js_int(&bank["startEventId"])
            && !truthy(&input["lastAvailableAt"]),
        "STREAM_RECOVERY_WARM_AFTER_START",
    )?;
    let mut s = input.clone();
    for e in books.as_array().cloned().unwrap_or_default() {
        checked_book(&e, bank)?;
        need(
            instant(e["confirmedAt"].as_str().unwrap_or(""))? <= instant(bank["cutoff"].as_str().unwrap_or(""))?
                && instant(e["quoteReceivedAt"].as_str().unwrap_or(""))?
                    <= instant(bank["cutoff"].as_str().unwrap_or(""))?,
            "STREAM_RECOVERY_FUTURE_WARM",
        )?;
        append_history(&mut s, &e, &bank["policy"])?;
        let n = s["warmBooks"].as_i64().unwrap_or(0) + 1;
        s["warmBooks"] = json!(n);
    }
    Ok(s)
}
fn resolve(w: &mut V, id: &str, at: &str, status: &str) {
    let order = w["pending"][id].clone();
    if order.is_null() {
        return;
    }
    let order_id = order["id"].clone();
    if let Some(outcomes) = w["outcomes"].as_array_mut() {
        if let Some(o) = outcomes.iter_mut().find(|x| x["id"] == order_id) {
            o["status"] = json!(status);
            o["resolvedAt"] = json!(at);
            o["unfilledQuantity"] = order["remaining"].clone();
            if status == "expired" {
                o["censoring"] = json!(if o["independentReceipts"].as_i64() == Some(0) {
                    "no_independent_in_window_quote"
                } else {
                    "observed_without_full_execution"
                });
            }
        }
    }
    if let V::Object(m) = &mut w["pending"] {
        m.remove(id);
    }
}
fn assert_wallet(w: &V, p: &V) -> Result<(), String> {
    let mut cost = BigInt::from(0);
    if let Some(pos) = w["positions"].as_object() {
        for (_k, pos_v) in pos {
            need(
                js_int(&pos_v["quantity"]).is_some_and(|q| q > 0)
                    && big_amount(pos_v["cost"].as_str().unwrap_or(""))? > BigInt::from(0),
                "STREAM_RECOVERY_POSITION",
            )?;
            cost += big_amount(pos_v["cost"].as_str().unwrap_or(""))?;
        }
    }
    need(
        big_amount(w["cash"].as_str().unwrap_or(""))? >= BigInt::from(0)
            && big_amount(w["cash"].as_str().unwrap_or(""))? + &cost
                - big_amount(p["initialCash"].as_str().unwrap_or(""))?
                == big_amount(w["realizedPnl"].as_str().unwrap_or(""))?,
        "STREAM_RECOVERY_ACCOUNTING",
    )?;
    need(
        cost <= big_amount(p["maxExposure"].as_str().unwrap_or(""))?
            && big_amount(w["cumulativeBuys"].as_str().unwrap_or(""))?
                <= big_amount(p["cumulativeBuyBudget"].as_str().unwrap_or(""))?,
        "STREAM_RECOVERY_BUDGET",
    )?;
    Ok(())
}
fn fill_pending(w: &mut V, e: &V, p: &V) -> Result<(), String> {
    let id = e["marketId"].as_str().unwrap_or("").to_string();
    let mut o = w["pending"][id.as_str()].clone();
    if o.is_null() {
        return Ok(());
    }
    let b = e["book"].clone();
    let receipt = instant(e["quoteReceivedAt"].as_str().unwrap_or(""))?;
    let confirmed = instant(e["confirmedAt"].as_str().unwrap_or(""))?;
    let independent = big_sequence(b["sequence"].as_str().unwrap_or(""))?
        > big_sequence(o["sequence"].as_str().unwrap_or(""))?
        && ns(&b["sourceAt"])? > ns(&o["sourceAt"])?;
    if !independent
        || receipt < instant(o["at"].as_str().unwrap_or(""))? + js_int(&w["latencyMs"]).unwrap_or(0)
        || receipt >= instant(o["expiresAt"].as_str().unwrap_or(""))?
        || confirmed >= instant(o["expiresAt"].as_str().unwrap_or(""))?
        || !truthy(&e["continuity"]["ready"]) || !truthy(&e["continuity"]["continuous"])
        || (truthy(&e["nextExpiryAt"]) && ns(&e["nextExpiryAt"])? <= ns(&e["confirmedAt"])?)
    {
        return Ok(());
    }
    let o_id = o["id"].clone();
    if let Some(outcomes) = w["outcomes"].as_array_mut() {
        if let Some(outcome) = outcomes.iter_mut().find(|x| x["id"] == o_id) {
            let n = outcome["independentReceipts"].as_i64().unwrap_or(0) + 1;
            outcome["independentReceipts"] = json!(n);
        }
    }
    let levels = if o["side"].as_str() == Some("buy") {
        b["asks"].as_array().cloned().unwrap_or_default()
    } else {
        b["bids"].as_array().cloned().unwrap_or_default()
    };
    let limit = big_amount(o["limit"].as_str().unwrap_or(""))?;
    let mut remaining = js_int(&o["remaining"]).unwrap_or(0);
    let buy = o["side"].as_str() == Some("buy");
    for l in &levels {
        let raw = big_amount(l["price"].as_str().unwrap_or(""))?;
        if (buy && raw > limit) || (!buy && raw < limit) {
            break;
        }
        let depth = (l["quantity"].as_f64().unwrap_or(0.0)
            * js_int(&p["depthFractionBps"]).unwrap_or(0) as f64 / 10000.0)
            .floor() as i64;
        let qty = remaining.min(depth);
        if qty == 0 {
            continue;
        }
        // This is a counterfactual executable opportunity, not queue priority
        // or a real fill.
        let tick = BigInt::from(TICK);
        let execution = if buy {
            if &raw + &tick < limit { &raw + &tick } else { limit.clone() }
        } else if &raw - &tick > limit {
            &raw - &tick
        } else {
            limit.clone()
        };
        let notional = &execution * BigInt::from(qty);
        let mut gain = BigInt::from(0);
        if buy {
            need(
                big_amount(w["cash"].as_str().unwrap_or(""))? >= notional,
                "STREAM_RECOVERY_CASH",
            )?;
            let mut pos = if w["positions"][id.as_str()].is_object() {
                w["positions"][id.as_str()].clone()
            } else {
                json!({"quantity":0,"cost":"0.000000","openedAt":e["confirmedAt"],
                    "realizedPnl":"0.000000","enteredQuantity":0,"entryCost":"0.000000"})
            };
            let nq = js_int(&pos["quantity"]).unwrap_or(0) + qty;
            let entered = js_int(&pos["enteredQuantity"]).unwrap_or(0) + qty;
            pos["enteredQuantity"] = json!(entered);
            let entry_cost = big_amount(pos["entryCost"].as_str().unwrap_or(""))? + &notional;
            pos["entryCost"] = json!(money(&entry_cost));
            pos["quantity"] = json!(nq);
            let ncost = big_amount(pos["cost"].as_str().unwrap_or(""))? + &notional;
            pos["cost"] = json!(money(&ncost));
            w["positions"][id.as_str()] = pos;
            let cash = big_amount(w["cash"].as_str().unwrap_or(""))? - &notional;
            w["cash"] = json!(money(&cash));
            let cb = big_amount(w["cumulativeBuys"].as_str().unwrap_or(""))? + &notional;
            w["cumulativeBuys"] = json!(money(&cb));
        } else {
            let mut pos = w["positions"][id.as_str()].clone();
            need(
                pos.is_object() && js_int(&pos["quantity"]).unwrap_or(0) >= qty,
                "STREAM_RECOVERY_OVERSELL",
            )?;
            let pos_qty = js_int(&pos["quantity"]).unwrap_or(0);
            let pos_cost = big_amount(pos["cost"].as_str().unwrap_or(""))?;
            let released = if qty == pos_qty {
                pos_cost.clone()
            } else {
                pos_cost.clone() * BigInt::from(qty) / BigInt::from(pos_qty)
            };
            gain = &notional - &released;
            pos["quantity"] = json!(pos_qty - qty);
            pos["cost"] = json!(money(&(pos_cost - &released)));
            let ppnl = big_amount(pos["realizedPnl"].as_str().unwrap_or(""))? + &gain;
            pos["realizedPnl"] = json!(money(&ppnl));
            let cash = big_amount(w["cash"].as_str().unwrap_or(""))? + &notional;
            w["cash"] = json!(money(&cash));
            let pnl = big_amount(w["realizedPnl"].as_str().unwrap_or(""))? + &gain;
            w["realizedPnl"] = json!(money(&pnl));
            if pos["quantity"].as_i64() == Some(0) {
                if let Some(rt) = w["roundTrips"].as_array_mut() {
                    rt.push(json!({"marketId":id,"openedAt":pos["openedAt"],
                        "closedAt":e["confirmedAt"],"quantity":pos["enteredQuantity"],
                        "entryCost":pos["entryCost"],"realizedPnl":pos["realizedPnl"]}));
                }
                if let V::Object(m) = &mut w["positions"] {
                    m.remove(id.as_str());
                }
                let ct = w["closedTrades"].as_i64().unwrap_or(0) + 1;
                w["closedTrades"] = json!(ct);
                w["cooldowns"][id.as_str()] = json!(ms_to_iso(
                    (confirmed + js_int(&p["cooldownMs"]).unwrap_or(0)) as f64
                ));
            } else {
                w["positions"][id.as_str()] = pos;
            }
        }
        if let Some(fills) = w["fills"].as_array_mut() {
            fills.push(json!({"orderId":o["id"],"marketId":id,"side":o["side"],
                "quantity":qty,"price":money(&execution),"realizedPnl":money(&gain),
                "fee":"0.000000","sourceVersion":b["versionKey"],
                "quoteReceivedAt":e["quoteReceivedAt"],"confirmedAt":e["confirmedAt"],
                "gatewayAvailableAt":e["availableAt"],
                "evidence":"counterfactual_executable_opportunity"}));
        }
        remaining -= qty;
        o["remaining"] = json!(remaining);
        if let Some(outcomes) = w["outcomes"].as_array_mut() {
            if let Some(outcome) = outcomes.iter_mut().find(|x| x["id"] == o_id) {
                let n = outcome["filledQuantity"].as_i64().unwrap_or(0) + qty;
                outcome["filledQuantity"] = json!(n);
            }
        }
        if remaining == 0 {
            break;
        }
    }
    // One accepted version consumes depth once, including partial fills.
    o["sequence"] = b["sequence"].clone();
    o["sourceAt"] = b["sourceAt"].clone();
    w["pending"][id.as_str()] = o.clone();
    if remaining == 0 {
        resolve(w, &id, e["confirmedAt"].as_str().unwrap_or(""), "filled");
    }
    assert_wallet(w, p)?;
    Ok(())
}
fn fresh(s: &V, e: &V, now: i64, p: &V) -> Result<bool, String> {
    Ok(truthy(&e["decisionEligible"])
        && !truthy(&s["invalid"][e["marketId"].as_str().unwrap_or("")])
        && truthy(&e["continuity"]["ready"]) && truthy(&e["continuity"]["continuous"])
        && now - instant(e["quoteReceivedAt"].as_str().unwrap_or(""))? >= 0
        && now - instant(e["quoteReceivedAt"].as_str().unwrap_or(""))?
            < js_int(&p["maximumQuoteAgeMs"]).unwrap_or(0)
        && (!truthy(&e["nextExpiryAt"])
            || ns(&e["nextExpiryAt"])? > now as i128 * 1_000_000))
}
#[allow(clippy::too_many_arguments)]
fn submit(w: &mut V, e: &V, at: &str, side: &str, quantity: i64, limit: &str, p: &V, reason: &str)
    -> Result<(), String> {
    let id = e["marketId"].as_str().unwrap_or("").to_string();
    let next = w["nextOrder"].as_i64().unwrap_or(0);
    let order_id = format!("recovery-{}-{}-{}", w["variant"]["id"].as_str().unwrap_or(""),
        js_int(&w["latencyMs"]).unwrap_or(0), next);
    w["nextOrder"] = json!(next + 1);
    let expires = ms_to_iso((instant(at)? + js_int(&p["entryLifetimeMs"]).unwrap_or(0)) as f64);
    let o = json!({"id":order_id,"side":side,"quantity":quantity,"remaining":quantity,
        "limit":limit,"at":at,"expiresAt":expires,"sequence":e["book"]["sequence"],
        "sourceAt":e["book"]["sourceAt"],"reason":reason});
    w["pending"][id.as_str()] = o.clone();
    let mut outcome = o.as_object().cloned().unwrap_or_default();
    outcome.insert("marketId".into(), json!(id));
    outcome.insert("status".into(), json!("pending"));
    outcome.insert("filledQuantity".into(), json!(0));
    outcome.insert("independentReceipts".into(), json!(0));
    if let Some(a) = w["outcomes"].as_array_mut() {
        a.push(V::Object(outcome));
    }
    Ok(())
}
/// decide(s,w,e,at,bank) — reads s (histories/invalid) and mutates wallet w.
fn decide(s: &V, w: &mut V, e: &V, at: &str, bank: &V) -> Result<(), String> {
    let id = e["marketId"].as_str().unwrap_or("").to_string();
    // p = {...bank.policy, ...w.variant.payoff} — payoff keys replace in place.
    let mut p = bank["policy"].clone();
    if let V::Object(pm) = &mut p {
        for (k, v) in w["variant"]["payoff"].as_object().cloned().unwrap_or_default() {
            pm.insert(k, v);
        }
    }
    let b = e["book"].clone();
    let now = instant(at)?;
    {
        if !fresh(s, e, now, &p)? {
            inc(&mut w["gateCounts"], "stale_or_invalid");
            return Ok(());
        }
        if truthy(&w["pending"][id.as_str()]) {
            return Ok(());
        }
        let bid = big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?;
        let ask = big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))?;
        let pos = w["positions"][id.as_str()].clone();
        if pos.is_object() {
            let pos_qty = js_int(&pos["quantity"]).unwrap_or(0);
            let per = if pos_qty != 0 {
                big_amount(pos["cost"].as_str().unwrap_or(""))? / BigInt::from(pos_qty)
            } else {
                return Err("Division by zero".into());
            };
            let exit = &bid - BigInt::from(TICK);
            if &exit - &per >= big_amount(p["takeProfit"].as_str().unwrap_or(""))?
                || &per - &exit >= big_amount(p["stopLoss"].as_str().unwrap_or(""))?
                || now - instant(pos["openedAt"].as_str().unwrap_or(""))?
                    >= js_int(&p["maximumHoldMs"]).unwrap_or(0)
            {
                let limit = if exit > BigInt::from(0) { exit } else { BigInt::from(TICK) };
                submit(w, e, at, "sell", pos_qty, &money(&limit), &p, "profit_loss_or_hold_exit")?;
                return Ok(());
            }
        }
        let ref_ = stream_recovery_reference(&s["histories"][id.as_str()], at, &p)?;
        let mut signal = ref_.as_object().cloned().unwrap_or_default();
        signal.insert("at".into(), json!(at));
        signal.insert("rawAsk".into(), json!(money(&ask)));
        signal.insert("rawBid".into(), json!(money(&bid)));
        signal.insert("paddedAsk".into(), json!(money(&(&ask + BigInt::from(TICK)))));
        signal.insert("paddedBid".into(), json!(money(&(&bid - BigInt::from(TICK)))));
        signal.insert("rawSpread".into(),
            json!(bignum_to_f64(&(&ask - &bid)) / 1e6));
        signal.insert("paddedSpread".into(),
            json!(bignum_to_f64(&(&ask - &bid + BigInt::from(2 * TICK))) / 1e6));
        if truthy(&ref_["ready"]) {
            let center = ref_["center"].as_f64().unwrap_or(0.0);
            signal.insert("paddedDiscount".into(),
                json!(center - bignum_to_f64(&(&ask + BigInt::from(TICK))) / 1e6));
            signal.insert("conditionalDiscount".into(),
                json!(center - bignum_to_f64(&ask) / 1e6));
        }
        w["signals"][id.as_str()] = V::Object(signal);
        if !truthy(&ref_["ready"]) {
            inc(&mut w["gateCounts"], ref_["reason"].as_str().unwrap_or(""));
            return Ok(());
        }
        let cooldown_end = date_parse(w["cooldowns"][id.as_str()].as_str().unwrap_or(""));
        if cooldown_end > now as f64 {
            inc(&mut w["gateCounts"], "cooldown");
            return Ok(());
        }
        let mut occupied: Vec<String> = Vec::new();
        if let Some(m) = w["positions"].as_object() {
            for k in m.keys() {
                occupied.push(k.clone());
            }
        }
        if let Some(m) = w["pending"].as_object() {
            for k in m.keys() {
                occupied.push(k.clone());
            }
        }
        occupied.sort();
        occupied.dedup();
        if occupied.len() as i64 >= js_int(&p["maxOpenMarkets"]).unwrap_or(0) {
            inc(&mut w["gateCounts"], "market_count");
            return Ok(());
        }
        let markets = bank["markets"].as_array().cloned().unwrap_or_default();
        let identity = markets.iter().find(|x| x["marketId"].as_str() == Some(id.as_str()));
        let others: Vec<&V> = occupied.iter().map(|x| {
            markets.iter().find(|m| m["marketId"].as_str() == Some(x.as_str()))
                .cloned().unwrap_or(V::Null)
        }).collect::<Vec<V>>().iter().map(|v| v).collect();
        // note: recompute to avoid returning references into a temp vec
        let others: Vec<V> = occupied.iter().map(|x| {
            markets.iter().find(|m| m["marketId"].as_str() == Some(x.as_str()))
                .cloned().unwrap_or(V::Null)
        }).collect();
        let _ = others;
        let others_v: Vec<V> = occupied.iter().map(|x| {
            markets.iter().find(|m| m["marketId"].as_str() == Some(x.as_str()))
                .cloned().unwrap_or(V::Null)
        }).collect();
        let identity_ok = identity.is_some();
        let id_state = identity.map(|m| m["state"].clone()).unwrap_or(V::Null);
        let id_office = identity.map(|m| m["office"].clone()).unwrap_or(V::Null);
        if !truthy(&id_state) || !truthy(&id_office) {
            if others_v.iter().any(|x| !truthy(&x["state"]) || !truthy(&x["office"])) {
                inc(&mut w["gateCounts"], "unknown_identity_concentration");
                return Ok(());
            }
        }
        if truthy(&id_state)
            && others_v.iter().filter(|x| x["state"] == id_state).count() >= 2 {
            inc(&mut w["gateCounts"], "state_concentration");
            return Ok(());
        }
        if instant(e["quoteReceivedAt"].as_str().unwrap_or(""))?
            <= instant(bank["cutoff"].as_str().unwrap_or(""))? {
            inc(&mut w["gateCounts"], "pre_cutoff_quote");
            return Ok(());
        }
        let center = ref_["center"].as_f64().unwrap_or(0.0);
        let robust = ref_["robustDeviation"].as_f64().unwrap_or(0.0);
        let admitted = |candidate: &BigInt| -> bool {
            let a = bignum_to_f64(candidate) / 1e6;
            let bid_value = bignum_to_f64(&(&bid - BigInt::from(TICK))) / 1e6;
            let d = center - a;
            a >= p["minimumPrice"].as_f64().unwrap_or(0.0)
                && a <= p["maximumPrice"].as_f64().unwrap_or(0.0)
                && a > bid_value
                && a - bid_value <= p["maximumSpread"].as_f64().unwrap_or(0.0) + 1e-12
                && d >= p["minimumDip"].as_f64().unwrap_or(0.0) - 1e-12
                && d <= p["maximumDip"].as_f64().unwrap_or(0.0)
                && d / center.max(0.001) >= p["minimumRelativeDip"].as_f64().unwrap_or(0.0)
                && d / robust >= p["minimumZ"].as_f64().unwrap_or(0.0)
        };
        // Confirmation barrier runs after the regime gates, before the limit.
        let confirmation = recovery_confirmation(
            &s["recoveryPoints"][id.as_str()], e, at, &recovery_confirmation_policy())?;
        w["signals"][id.as_str()]["recovery"] = confirmation.clone();
        if w["variant"]["entry"].as_str() != Some("baseline") && !truthy(&confirmation["admitted"]) {
            inc(&mut w["gateCounts"], confirmation["reason"].as_str().unwrap_or("undefined"));
            return Ok(());
        }
        let entry = w["variant"]["entry"].as_str().unwrap_or("");
        let mut limit: Option<BigInt> = None;
        if entry == "pricecap" {
            if ask > BigInt::from(TICK) && admitted(&ask) {
                limit = Some(&ask - BigInt::from(TICK));
            }
        } else if admitted(&(&ask + BigInt::from(TICK))) {
            limit = Some(ask.clone());
        } else if entry == "baseline" && ask > BigInt::from(TICK) && admitted(&ask) {
            limit = Some(&ask - BigInt::from(TICK));
        }
        if limit.is_some() && entry != "baseline"
            && &limit.clone().unwrap() - &(&bid - BigInt::from(TICK))
                >= big_amount(p["stopLoss"].as_str().unwrap_or(""))? {
            inc(&mut w["gateCounts"], "insufficient_exit_headroom");
            return Ok(());
        }
        let Some(limit) = limit else {
            inc(&mut w["gateCounts"], "discount_spread_or_regime_gate");
            let a = bignum_to_f64(&ask) / 1e6;
            let discount = center - a;
            let reason = if a < p["minimumPrice"].as_f64().unwrap_or(0.0)
                || a > p["maximumPrice"].as_f64().unwrap_or(0.0) {
                "price_band"
            } else if bignum_to_f64(&(&ask - &bid + BigInt::from(TICK))) / 1e6
                > p["maximumSpread"].as_f64().unwrap_or(0.0) + 1e-12 {
                "spread"
            } else if discount < p["minimumDip"].as_f64().unwrap_or(0.0) - 1e-12 {
                "minimum_discount"
            } else if discount > p["maximumDip"].as_f64().unwrap_or(0.0) {
                "regime_change"
            } else if discount / center.max(0.001) < p["minimumRelativeDip"].as_f64().unwrap_or(0.0) {
                "relative_discount"
            } else {
                "robust_z"
            };
            inc(&mut w["gateCounts"], reason);
            return Ok(());
        };
        let mut reserved = BigInt::from(0);
        if let Some(m) = w["pending"].as_object() {
            for (_k, x) in m {
                if x["side"].as_str() == Some("buy") {
                    reserved += big_amount(x["limit"].as_str().unwrap_or(""))?
                        * BigInt::from(js_int(&x["remaining"]).unwrap_or(0));
                }
            }
        }
        let budget = big_amount(p["cumulativeBuyBudget"].as_str().unwrap_or(""))?
            - big_amount(w["cumulativeBuys"].as_str().unwrap_or(""))? - &reserved;
        let cash_minus = big_amount(w["cash"].as_str().unwrap_or(""))? - &reserved;
        if limit == BigInt::from(0) {
            // JS BigInt division by zero — RangeError.
            return Err("Division by zero".into());
        }
        let qty = js_int(&p["orderShares"]).unwrap_or(0)
            .min((big_amount(p["maxMarketCost"].as_str().unwrap_or(""))? / &limit)
                .to_string().parse::<i64>().unwrap_or(0))
            .min((&budget / &limit).to_string().parse::<i64>().unwrap_or(0))
            .min((&cash_minus / &limit).to_string().parse::<i64>().unwrap_or(0));
        if qty < 1 {
            inc(&mut w["gateCounts"], "budget");
            return Ok(());
        }
        submit(w, e, at, "buy", qty, &money(&limit), &p,
            if limit == ask { "native_dip" } else { "conditional_one_tick_lower" })?;
        inc(&mut w["gateCounts"], "admitted");
        Ok(())
    }
}
/// advanceStreamRecovery(input,events,bank,{availableAt,evidenceThrough,allowDecisions})
pub fn advance_stream_recovery(input: &V, events: &V, bank: &V, opts: &V) -> Result<V, String> {
    check_bank(bank)?;
    need(
        input["bankHash"].as_str() == Some(stream_recovery_hash(bank).as_str()),
        "STREAM_RECOVERY_BANK_CHANGED",
    )?;
    let available_at = opts["availableAt"].as_str().unwrap_or("");
    let now = instant(available_at)?;
    let evidence_through = match opts.get("evidenceThrough") {
        None => available_at.to_string(),
        Some(v) => v.as_str().unwrap_or("").to_string(),
    };
    let allow_decisions = match opts.get("allowDecisions") {
        None => Some(true),
        Some(v) => v.as_bool(),
    };
    let watermark = instant(&evidence_through)?;
    need(
        watermark <= now
            && (input["lastEvidenceThrough"].is_null()
                || watermark >= instant(input["lastEvidenceThrough"].as_str().unwrap_or(""))?)
            && allow_decisions.is_some(),
        "STREAM_RECOVERY_EVIDENCE_CLOCK",
    )?;
    let allow_decisions = allow_decisions.unwrap_or(true);
    need(
        now >= instant(bank["cutoff"].as_str().unwrap_or(""))?
            && (input["lastAvailableAt"].is_null()
                || now >= instant(input["lastAvailableAt"].as_str().unwrap_or(""))?),
        "STREAM_RECOVERY_HOST_CLOCK",
    )?;
    let mut s = input.clone();
    // JS Map: insertion order preserved, set() on existing key keeps position.
    let mut last: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    for e in events.as_array().cloned().unwrap_or_default() {
        need(
            js_int(&e["eventId"]) == Some(js_int(&s["lastEventId"]).unwrap_or(0) + 1)
                && instant(e["availableAt"].as_str().unwrap_or(""))? <= now
                && instant(e["confirmedAt"].as_str().unwrap_or(""))? <= now,
            "STREAM_RECOVERY_EVENT_ORDER",
        )?;
        need(
            e["tournamentId"] == bank["tournamentId"]
                && bank["markets"].as_array().cloned().unwrap_or_default().iter().any(|x| {
                    x["marketId"] == e["marketId"] && x["exchangeId"] == e["exchangeId"]
                }),
            "STREAM_RECOVERY_SCOPE",
        )?;
        s["lastEventId"] = e["eventId"].clone();
        let mid = e["marketId"].as_str().unwrap_or("").to_string();
        if e["kind"].as_str() == Some("invalidate") {
            s["invalid"][mid.as_str()] = json!(true);
            if let V::Object(m) = &mut s["latest"] {
                m.remove(mid.as_str());
            }
            // JS Map.delete preserves insertion order.
            last.shift_remove(&mid);
            let wkeys: Vec<String> = s["wallets"].as_object()
                .map(|m| m.keys().cloned().collect()).unwrap_or_default();
            for wkey in wkeys {
                let mut w = s["wallets"][wkey.as_str()].clone();
                resolve(&mut w, &mid, e["confirmedAt"].as_str().unwrap_or(""), "invalidated");
                s["wallets"][wkey.as_str()] = w;
            }
            continue;
        }
        need(e["kind"].as_str() == Some("book"), "STREAM_RECOVERY_EVENT_KIND")?;
        let b = checked_book(&e, bank)?;
        let old = s["seen"][mid.as_str()].clone();
        let content_hash = stream_recovery_hash(&json!({"bids":b["bids"],"asks":b["asks"]}));
        if old.is_object() {
            need(
                big_sequence(b["sequence"].as_str().unwrap_or(""))?
                    >= big_sequence(old["sequence"].as_str().unwrap_or(""))?
                    && ns(&b["sourceAt"])? >= ns(&old["sourceAt"])?,
                "STREAM_RECOVERY_SOURCE_ROLLBACK",
            )?;
            if b["sequence"] == old["sequence"] {
                need(
                    b["sourceAt"] == old["sourceAt"] && content_hash == old["contentHash"],
                    "STREAM_RECOVERY_SAME_VERSION_CHANGED",
                )?;
            }
        }
        s["seen"][mid.as_str()] = json!({"sequence":b["sequence"],"sourceAt":b["sourceAt"],
            "contentHash":content_hash});
        if let V::Object(m) = &mut s["invalid"] {
            m.remove(mid.as_str());
        }
        s["latest"][mid.as_str()] = e.clone();
        if old.is_null() || b["sequence"] != old["sequence"] {
            let n = s["observations"].as_i64().unwrap_or(0) + 1;
            s["observations"] = json!(n);
        }
        let wkeys: Vec<String> = s["wallets"].as_object()
            .map(|m| m.keys().cloned().collect()).unwrap_or_default();
        for wkey in wkeys {
            let mut w = s["wallets"][wkey.as_str()].clone();
            fill_pending(&mut w, &e, &bank["policy"])?;
            s["wallets"][wkey.as_str()] = w;
        }
        s["recoveryPoints"][mid.as_str()] = observe_recovery(
            &s["recoveryPoints"][mid.as_str()], &e, &recovery_confirmation_policy())?;
        last.insert(mid, e.clone());
    }
    // Resolve the entire ordered evidence batch before expiring old orders or
    // making new decisions. A delayed consumer cannot erase an adverse
    // pre-expiry fill.
    let wkeys: Vec<String> = s["wallets"].as_object()
        .map(|m| m.keys().cloned().collect()).unwrap_or_default();
    for wkey in wkeys {
        let mut w = s["wallets"][wkey.as_str()].clone();
        let pending_ids: Vec<String> = w["pending"].as_object()
            .map(|m| m.keys().cloned().collect()).unwrap_or_default();
        for id in pending_ids {
            let o = w["pending"][id.as_str()].clone();
            if watermark >= instant(o["expiresAt"].as_str().unwrap_or(""))? {
                resolve(&mut w, &id, available_at, "expired");
            }
        }
        if allow_decisions {
            for (_mid, e) in &last {
                decide(&s, &mut w, e, available_at, bank)?;
            }
        }
        assert_wallet(&w, &bank["policy"])?;
        s["wallets"][wkey.as_str()] = w;
    }
    for (_mid, e) in &last {
        append_history(&mut s, e, &bank["policy"])?;
    }
    s["lastAvailableAt"] = json!(available_at);
    s["lastEvidenceThrough"] = json!(evidence_through);
    Ok(s)
}
/// streamRecoveryReport(s,bank,at)
pub fn stream_recovery_report(s: &V, bank: &V, at: &str) -> Result<V, String> {
    need(
        s["bankHash"].as_str() == Some(stream_recovery_hash(bank).as_str()),
        "STREAM_RECOVERY_REPORT_BANK",
    )?;
    let now = instant(at)?;
    let mut wallets = Map::new();
    for (name, w) in s["wallets"].as_object().cloned().unwrap_or_default() {
        let mut liquidation = BigInt::from(0);
        let mut complete = true;
        let mut positions = Vec::new();
        for (id, pos) in w["positions"].as_object().cloned().unwrap_or_default() {
            let e = s["latest"][id.as_str()].clone();
            let ready = truthy(&e) && fresh(s, &e, now, &bank["policy"])?;
            let mut mark: Option<String> = None;
            if ready {
                let mut q = js_int(&pos["quantity"]).unwrap_or(0);
                let mut value = BigInt::from(0);
                for l in e["book"]["bids"].as_array().cloned().unwrap_or_default() {
                    let depth = (l["quantity"].as_f64().unwrap_or(0.0)
                        * js_int(&bank["policy"]["depthFractionBps"]).unwrap_or(0) as f64
                        / 10000.0)
                        .floor() as i64;
                    let n = q.min(depth);
                    let px = big_amount(l["price"].as_str().unwrap_or(""))?;
                    let contrib = if px > BigInt::from(TICK) {
                        px - BigInt::from(TICK)
                    } else {
                        BigInt::from(0)
                    };
                    value += contrib * BigInt::from(n);
                    q -= n;
                    if q == 0 {
                        break;
                    }
                }
                if q == 0 {
                    mark = Some(money(&value));
                }
            }
            if mark.is_none() {
                complete = false;
            } else {
                liquidation += big_amount(mark.as_deref().unwrap_or(""))?;
            }
            let mut row = Map::new();
            row.insert("marketId".into(), json!(id));
            if let V::Object(pm) = &pos {
                for (k, v) in pm { row.insert(k.clone(), v.clone()); }
            }
            row.insert("liquidationValue".into(),
                mark.map(|m| json!(m)).unwrap_or(V::Null));
            row.insert("markReady".into(),
                json!(row["liquidationValue"] != V::Null));
            positions.push(V::Object(row));
        }
        let pending: Vec<V> = w["pending"].as_object()
            .map(|m| m.iter().map(|(mk, o)| {
                let mut row = Map::new();
                row.insert("marketId".into(), json!(mk));
                if let V::Object(om) = o {
                    for (k, v) in om { row.insert(k.clone(), v.clone()); }
                }
                V::Object(row)
            }).collect()).unwrap_or_default();
        let mut expired_count = 0i64;
        for o in &pending {
            if now >= instant(o["expiresAt"].as_str().unwrap_or(""))? {
                expired_count += 1;
            }
        }
        let total_pnl: V = if complete {
            json!(money(&(big_amount(w["cash"].as_str().unwrap_or(""))? + &liquidation
                - big_amount(bank["policy"]["initialCash"].as_str().unwrap_or(""))?)))
        } else {
            V::Null
        };
        let fills_len = w["fills"].as_array().map(|a| a.len()).unwrap_or(0);
        let outcomes_len = w["outcomes"].as_array().map(|a| a.len()).unwrap_or(0);
        let recent_fills = w["fills"].as_array().cloned().unwrap_or_default();
        let outcomes = w["outcomes"].as_array().cloned().unwrap_or_default();
        let rf: Vec<V> = if recent_fills.len() > 30 {
            recent_fills[recent_fills.len() - 30..].to_vec()
        } else {
            recent_fills
        };
        let oc: Vec<V> = if outcomes.len() > 50 {
            outcomes[outcomes.len() - 50..].to_vec()
        } else {
            outcomes
        };
        let mut wins = 0i64;
        let mut losses = 0i64;
        let mut gross_wins = BigInt::from(0);
        let mut gross_losses = BigInt::from(0);
        let round_trips = w["roundTrips"].as_array().cloned().unwrap_or_default();
        for x in &round_trips {
            let pnl = big_amount(x["realizedPnl"].as_str().unwrap_or(""))?;
            if pnl > BigInt::from(0) {
                wins += 1;
                gross_wins += pnl;
            } else if pnl < BigInt::from(0) {
                losses += 1;
                gross_losses -= pnl;
            }
        }
        let rt_count = round_trips.len() as i64;
        let closed_round_trips = json!({"count":rt_count,"wins":wins,"losses":losses,
            "flat":rt_count - wins - losses,
            "winRate":if rt_count > 0 { json!(wins as f64 / rt_count as f64) } else { V::Null },
            "grossWins":money(&gross_wins),"grossLosses":money(&gross_losses),
            "profitFactor":if gross_losses != BigInt::from(0) {
                json!(bignum_to_f64(&gross_wins) / bignum_to_f64(&gross_losses))
            } else { V::Null },
            "recent":if round_trips.len() > 30 {
                json!(round_trips[round_trips.len() - 30..].to_vec())
            } else { json!(round_trips) }});
        let sl = big_amount(w["variant"]["payoff"]["stopLoss"].as_str().unwrap_or(""))?;
        let tp = big_amount(w["variant"]["payoff"]["takeProfit"].as_str().unwrap_or(""))?;
        let breakeven = bignum_to_f64(&sl) / bignum_to_f64(&(&sl + &tp));
        wallets.insert(name, json!({"variant":w["variant"],"latencyMs":w["latencyMs"],
            "cash":w["cash"],"realizedPnl":w["realizedPnl"],
            "cumulativeBuys":w["cumulativeBuys"],"closedTrades":w["closedTrades"],
            "positions":positions,"pending":pending,
            "closedRoundTrips":closed_round_trips,
            "idealizedBreakEvenWinRate":breakeven,
            "totalPnl":total_pnl,
            "marksComplete":complete,"clockExpiredAwaitingSource":expired_count,
            "gateCounts":w["gateCounts"],"signals":w["signals"],
            "orders":outcomes_len,"fills":fills_len,"recentFills":rf,"outcomes":oc}));
    }
    Ok(json!({"mode":bank["mode"],"bankHash":s["bankHash"],
        "inputBankHash":bank["inputBankHash"],"updatedAt":at,"cutoff":bank["cutoff"],
        "lastEventId":s["lastEventId"],
        "variants":bank["variants"],"confirmationPolicy":recovery_confirmation_policy(),
        "warmBooks":s["warmBooks"],
        "observations":s["observations"],"networkRequests":0,"competitionOrders":0,
        "feeBps":0,"profitabilityEstablished":false,"wallets":wallets}))
}
