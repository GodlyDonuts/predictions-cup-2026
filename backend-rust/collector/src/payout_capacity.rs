// live-paper/payout-capacity.mjs — offline conditional-payout capacity analysis.
// No network, credentials or orders. CLI entry omitted (no sibling Rust CLI pattern).
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{digest, js_int};
use num_bigint::BigInt;
use serde_json::{json, Value as V};

const SCALE_I64: i64 = 1_000_000;
fn scale() -> BigInt { BigInt::from(SCALE_I64) }
fn fail<T>(code: &str) -> Result<T, String> { Err(code.to_string()) }

/// Local decimal: /^(0|[1-9]\d{0,12})(\.\d{1,6})?$/ — stricter than fixed.mjs amount.
fn money(value: &V) -> Result<BigInt, String> {
    let s = value.as_str().ok_or("INVALID_DECIMAL")?;
    let (whole, frac) = match s.split_once('.') { Some((w, f)) => (w, f), None => (s, "") };
    let whole_ok = whole == "0"
        || (whole.len() <= 13 && !whole.is_empty() && !whole.starts_with('0')
            && whole.bytes().all(|b| b.is_ascii_digit()));
    let frac_ok = if s.contains('.') {
        !frac.is_empty() && frac.len() <= 6 && frac.bytes().all(|b| b.is_ascii_digit())
    } else { true };
    if !(whole_ok && frac_ok) { return fail("INVALID_DECIMAL"); }
    let w: BigInt = whole.parse().map_err(|_| "INVALID_DECIMAL".to_string())?;
    let f: BigInt = format!("{frac:0<6}").parse().map_err(|_| "INVALID_DECIMAL".to_string())?;
    Ok(w * scale() + f)
}
fn fmt(value: &BigInt) -> String {
    let s = scale();
    let (sign, x) = if *value < BigInt::from(0) { ("-", -value.clone()) } else { ("", value.clone()) };
    format!("{}{}.{:0>6}", sign, &x / &s, (&x % &s).to_string())
}
fn integer(value: &V, max: i64) -> Result<i64, String> {
    let v = js_int(value).ok_or("INVALID_INTEGER")?;
    if v < 0 || v > max { return fail("INVALID_INTEGER"); }
    Ok(v)
}
fn token(value: &V) -> Result<String, String> {
    let s = value.as_str().ok_or("INVALID_ID")?;
    if s.is_empty() || s.len() > 200
        || !s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'_' | b'.' | b'/' | b'-')) {
        return fail("INVALID_ID");
    }
    Ok(s.to_string())
}
fn sha(value: &V) -> Result<String, String> {
    let s = value.as_str().ok_or("INVALID_HASH")?;
    if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return fail("INVALID_HASH");
    }
    Ok(s.to_string())
}
fn clock(value: &V) -> Result<f64, String> {
    let s = value.as_str().ok_or("INVALID_CLOCK")?;
    let b = s.as_bytes();
    let re_ok = b.len() == 24 && b[4] == b'-' && b[7] == b'-' && b[10] == b'T' && b[13] == b':'
        && b[16] == b':' && b[19] == b'.' && b[23] == b'Z'
        && b[0..4].iter().all(|c| c.is_ascii_digit()) && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[8..10].iter().all(|c| c.is_ascii_digit()) && b[11..13].iter().all(|c| c.is_ascii_digit())
        && b[14..16].iter().all(|c| c.is_ascii_digit()) && b[17..19].iter().all(|c| c.is_ascii_digit())
        && b[20..23].iter().all(|c| c.is_ascii_digit());
    if !re_ok { return fail("INVALID_CLOCK"); }
    let n = parse_iso_ms(s).unwrap_or(f64::NAN);
    if !n.is_finite() || ms_to_iso(n) != s { return fail("INVALID_CLOCK"); }
    Ok(n)
}
fn big_min(xs: &[BigInt]) -> BigInt {
    xs.iter().cloned().reduce(|a, b| if a < b { a } else { b }).unwrap()
}

/// evaluatePayoutCapacity(input) — equal-quantity purchase of two explicitly
/// reviewed native outcomes. Hypothetical costs, NOT fills/orders/profit.
pub fn evaluate_payout_capacity(input: &V) -> Result<V, String> {
    let now = &input["now"];
    let p = &input["policy"];
    let r = &input["review"];
    let books = &input["books"];
    let at = clock(now)?;
    if p.is_null() || r.is_null()
        || !books.is_array() || books.as_array().unwrap().len() != 2 { return fail("INVALID_INPUT"); }
    integer(&p["maxAgeMs"], i64::MAX)?;
    integer(&p["maxSkewMs"], i64::MAX)?;
    integer(&p["maxReviewAgeMs"], i64::MAX)?;
    let max_quantity = BigInt::from(integer(&p["maxQuantity"], i64::MAX)?);
    let budget = money(&p["maxCost"])?;
    let partial_cap = money(&p["maxUnmatchedCost"])?;
    let edge_floor = money(&p["minMarginalEdge"])?;
    let profit_floor = money(&p["minTotalProfit"])?;
    if r["kind"] != "reviewed_conditional_scenarios" || r["completeUnderAssumptions"] != json!(true)
        || !r["assumptions"].is_array() || r["assumptions"].as_array().unwrap().is_empty()
        || r["assumptions"].as_array().unwrap().iter()
            .any(|x| x.as_str().map_or(true, |s| s.trim().is_empty() || s.len() > 2000))
        || !r["legs"].is_array() || r["legs"].as_array().unwrap().len() != 2
        || !r["scenarios"].is_array() || r["scenarios"].as_array().unwrap().is_empty()
        || r["scenarios"].as_array().unwrap().len() > 100 { return fail("INVALID_REVIEW"); }
    token(&r["id"])?;
    token(&r["tournamentId"])?;
    sha(&r["evidenceHash"])?;
    let reviewed = clock(&r["reviewedAt"])?;
    let expires = clock(&r["expiresAt"])?;
    if reviewed > at || at >= expires || expires <= reviewed
        || at - reviewed > p["maxReviewAgeMs"].as_f64().unwrap_or(f64::NAN) { return fail("EXPIRED_REVIEW"); }
    let legs_v = r["legs"].as_array().unwrap();
    let mut identities = Vec::with_capacity(2);
    for l in legs_v {
        if l.is_null() || !["YES", "NO"].contains(&l["outcome"].as_str().unwrap_or("")) { return fail("INVALID_LEG"); }
        token(&l["marketId"])?;
        token(&l["exchangeId"])?;
        sha(&l["rulesHash"])?;
        identities.push(format!("{}:{}:{}", r["tournamentId"].as_str().unwrap_or(""),
            l["exchangeId"].as_str().unwrap_or(""), l["outcome"].as_str().unwrap_or("")));
    }
    if identities[0] == identities[1] { return fail("DUPLICATE_LEG"); }
    if legs_v[0]["exchangeId"] == legs_v[1]["exchangeId"]
        && legs_v[0]["marketId"] != legs_v[1]["marketId"] { return fail("EXCHANGE_MARKET_CONFLICT"); }
    let mut scenario_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut scenarios: Vec<(String, BigInt)> = Vec::new();
    for s in r["scenarios"].as_array().unwrap() {
        let sid = token(&s["id"])?;
        let payouts_ok = s["payouts"].as_array().map_or(false, |a| a.len() == 2
            && a.iter().all(|v| v.as_f64() == Some(0.0) || v.as_f64() == Some(1.0)));
        if scenario_ids.contains(&sid) || !payouts_ok { return fail("INVALID_SCENARIO"); }
        scenario_ids.insert(sid.clone());
        let sum = s["payouts"][0].as_i64().unwrap_or(0) + s["payouts"][1].as_i64().unwrap_or(0);
        if legs_v[0]["exchangeId"] == legs_v[1]["exchangeId"] && sum > 1 {
            return fail("CONTRADICTORY_NATIVE_PAYOUT");
        }
        scenarios.push((sid, BigInt::from(sum) * scale()));
    }
    let floor = big_min(&scenarios.iter().map(|s| s.1.clone()).collect::<Vec<_>>());
    let books_v = books.as_array().unwrap();
    let mut receipts: Vec<f64> = Vec::new();
    let mut starts: Vec<f64> = Vec::new();
    struct Leg { asks: Vec<(BigInt, BigInt, BigInt)>, fixed_fee: BigInt }
    let mut legs: Vec<Leg> = Vec::with_capacity(2);
    for (i, b) in books_v.iter().enumerate() {
        let l = &legs_v[i];
        if b.is_null() || b["tournamentId"] != r["tournamentId"] || b["marketId"] != l["marketId"]
            || b["exchangeId"] != l["exchangeId"] || b["outcome"] != l["outcome"]
            || b["rulesHash"] != l["rulesHash"] || b["nativeOutcome"] != json!(true)
            || b["status"] != "active" { return fail("BOOK_SCOPE"); }
        sha(&b["sourceHash"])?;
        token(&b["version"])?;
        let started = clock(&b["requestStartedAt"])?;
        let received = clock(&b["receivedAt"])?;
        if started > received || received > at
            || at - started > p["maxAgeMs"].as_f64().unwrap_or(f64::NAN) { return fail("STALE_BOOK"); }
        starts.push(started);
        receipts.push(received);
        if !b["asks"].is_array() || b["asks"].as_array().unwrap().is_empty()
            || b["asks"].as_array().unwrap().len() > 10000 { return fail("INVALID_DEPTH"); }
        let tick = money(&b["tickSize"])?;
        let slip = money(&b["adversePricePerShare"])?;
        let fee = money(&b["feePerShare"])?;
        let fixed_fee = money(&b["fixedFee"])?;
        if tick <= BigInt::from(0) || tick > scale() || scale() % &tick != BigInt::from(0)
            || &slip % &tick != BigInt::from(0) { return fail("INVALID_TICK"); }
        let mut previous = -BigInt::from(1);
        let mut asks: Vec<(BigInt, BigInt, BigInt)> = Vec::new();
        for level in b["asks"].as_array().unwrap() {
            let price = money(&level["price"])?;
            let quantity = BigInt::from(integer(&level["quantity"], i64::MAX)?);
            if quantity == BigInt::from(0) || price <= BigInt::from(0) || price >= scale()
                || &price % &tick != BigInt::from(0) || price <= previous { return fail("INVALID_DEPTH"); }
            previous = price.clone();
            // An adverse-price sensitivity beyond the binary domain is not a valid cost model.
            if &price + &slip > scale() { return fail("INVALID_ADVERSE_PRICE"); }
            asks.push((price.clone(), quantity, price + &slip + &fee));
        }
        legs.push(Leg { asks, fixed_fee });
    }
    // Bound the entire acquisition window, not only last-response timestamps.
    if receipts.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        - starts.iter().cloned().fold(f64::INFINITY, f64::min)
        > p["maxSkewMs"].as_f64().unwrap_or(f64::NAN) { return fail("SKEWED_BOOKS"); }
    let fixed = &legs[0].fixed_fee + &legs[1].fixed_fee;
    let mut costs = vec![legs[0].fixed_fee.clone(), legs[1].fixed_fee.clone()];
    let mut quantity = BigInt::from(0);
    let mut indexes = [0usize, 0];
    let mut remaining = vec![legs[0].asks[0].1.clone(), legs[1].asks[0].1.clone()];
    let mut stop_reason = "DEPTH_EXHAUSTED";
    let mut segments: Vec<V> = Vec::new();
    while indexes[0] < legs[0].asks.len() && indexes[1] < legs[1].asks.len() && quantity < max_quantity {
        let levels: Vec<&(BigInt, BigInt, BigInt)> =
            vec![&legs[0].asks[indexes[0]], &legs[1].asks[indexes[1]]];
        let unit_cost = &levels[0].2 + &levels[1].2;
        if &floor - &unit_cost < edge_floor || floor <= unit_cost { stop_reason = "MARGINAL_EDGE"; break; }
        if &costs[0] + &costs[1] > budget || costs.iter().any(|c| *c > partial_cap) {
            stop_reason = "CAPITAL_LIMIT"; break;
        }
        let take = big_min(&[
            remaining[0].clone(), remaining[1].clone(), &max_quantity - &quantity,
            (&budget - &costs[0] - &costs[1]) / &unit_cost,
            (&partial_cap - &costs[0]) / &levels[0].2,
            (&partial_cap - &costs[1]) / &levels[1].2,
        ]);
        if take <= BigInt::from(0) { stop_reason = "CAPITAL_LIMIT"; break; }
        for i in 0..2 {
            costs[i] += &take * &levels[i].2;
            remaining[i] -= &take;
        }
        quantity += &take;
        segments.push(json!({"quantity":take.to_string(),
            "prices":[fmt(&levels[0].0), fmt(&levels[1].0)],
            "adverseUnitCost":fmt(&unit_cost)}));
        for i in 0..2 {
            if remaining[i] == BigInt::from(0) {
                indexes[i] += 1;
                remaining[i] = legs[i].asks.get(indexes[i]).map(|a| a.1.clone()).unwrap_or_else(|| BigInt::from(0));
            }
        }
    }
    if quantity == max_quantity { stop_reason = "QUANTITY_LIMIT"; }
    let cost = if quantity > BigInt::from(0) { &costs[0] + &costs[1] } else { BigInt::from(0) };
    let profit = &quantity * &floor - &cost;
    let accepted = quantity > BigInt::from(0) && profit >= profit_floor && profit > BigInt::from(0);
    Ok(json!({
        "schemaVersion": 1, "kind": "conditional_payout_capacity", "evaluatedAt": now,
        "inputHash": digest(input), "reviewId": r["id"], "evidenceHash": r["evidenceHash"],
        "sourceHashes": books_v.iter().map(|b| b["sourceHash"].clone()).collect::<Vec<_>>(),
        "versions": books_v.iter().map(|b| b["version"].clone()).collect::<Vec<_>>(),
        "assumptions": r["assumptions"].as_array().unwrap().clone(),
        "scenarioCompletenessVerified": false,
        "executionAuthority": false, "ordersSubmitted": 0, "networkRequests": 0,
        "fills": null, "realizedPnl": null,
        "acceptedForResearch": accepted,
        "reason": if accepted { "CONDITIONAL_POSITIVE_CAPACITY" }
            else if quantity > BigInt::from(0) { "TOTAL_PROFIT" } else { stop_reason },
        "stopReason": stop_reason,
        "matchedQuantity": quantity.to_string(),
        "acquisitionCost": fmt(&cost),
        "fixedFeesIncluded": fmt(&(if quantity > BigInt::from(0) { fixed.clone() } else { BigInt::from(0) })),
        "conditionalMinimumPayout": fmt(&(&quantity * &floor)),
        "conditionalMinimumProfit": fmt(&profit),
        "maximumSingleLegCostAtRisk": fmt(&(if quantity > BigInt::from(0) {
            if costs[0] > costs[1] { costs[0].clone() } else { costs[1].clone() }
        } else { BigInt::from(0) })),
        "scenarioProfits": scenarios.iter()
            .map(|s| json!({"id":s.0,"profit":fmt(&(&quantity * &s.1 - &cost))})).collect::<Vec<_>>(),
        "segments": segments,
        "limitations": [
            "Conditional on caller-reviewed scenario completeness and unchanged rules.",
            "Native snapshot depth is not an atomic package, a queue model or a fill guarantee.",
            "Unmatched-leg bound assumes a worthless lone leg; it is not a live liquidation estimate.",
            "Fees must be explicit conservative inputs; unknown fees must not be entered as zero.",
            "Refund/override outcomes require separate scenarios or exclusion; no guaranteed arbitrage is asserted."
        ],
    }))
}
