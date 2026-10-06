// live-paper/bulk-dip.mjs — market-wide bulk price references feeding the
// bid-anchor replay. Quoted references are NOT executable depth.
use crate::bid_anchor::{bid_anchor_policy, create_bid_anchor_state};
use crate::bid_anchor_live::{advance_live_bid_anchor, live_bid_anchor_report};
use crate::breadth::validate_universe;
use crate::market_price_publication::validate_price_observation_pub;
use baal_paper::fixed::{big_amount, instant, money, TICK};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::js_truthy;
use num_bigint::BigInt;
use serde_json::{json, Value as V};

fn need(ok: bool) -> Result<(), String> { if !ok { return Err("BULK_DIP_INVALID".to_string()); } Ok(()) }
/// price(n) = money(BigInt(Math.round(n*1e6)))
pub fn price(n: f64) -> Result<String, String> {
    let r = (n * 1e6).round();
    if !r.is_finite() || r.abs() > 1.8e19 { return Err("money".to_string()); }
    Ok(money(&BigInt::from(r as i64)))
}
fn ins(v: &V) -> Result<i64, String> { instant(v.as_str().unwrap_or("")) }

/// BULK_DIP_POLICY (frozen object).
pub fn bulk_dip_policy() -> V {
    json!({"reference":"bulk_quoted_bid_not_executable","candidateLifetimeMs":60000,"maximumReceiptAgeMs":20000})
}

/// createBulkDipState(bank)
pub fn create_bulk_dip_state(bank: &V) -> Result<V, String> {
    let mut s = create_bid_anchor_state(bank)?;
    s["bulk"] = json!({"history":{},"candidates":{},"counts":{},"lastAvailableAt":bank["cutoff"]});
    Ok(s)
}

/// quotedDipCandidate(points, q, at, p)
pub fn quoted_dip_candidate(points: &[V], q: &V, at: &str, p: Option<&V>) -> Result<V, String> {
    let p = p.cloned().unwrap_or_else(bid_anchor_policy);
    let now = instant(at)?;
    let history_ms = p["historyMs"].as_i64().unwrap_or(0);
    let mut a: Vec<(&V, i64)> = Vec::new();
    for x in points {
        let t = ins(&x["at"])?;
        if t < now && t >= now - history_ms { a.push((x, t)); }
    }
    let min_minutes = p["minimumMinutes"].as_i64().unwrap_or(0);
    let min_span = p["minimumSpanMs"].as_i64().unwrap_or(0);
    let max_gap = p["maximumGapMs"].as_i64().unwrap_or(0);
    if (a.len() as i64) < min_minutes || a.last().unwrap().1 - a[0].1 < min_span
        || now - a.last().unwrap().1 > max_gap
        || (1..a.len()).any(|i| a[i].1 - a[i - 1].1 > max_gap) {
        return Ok(V::Null);
    }
    let bid_num = q["bestBid"].as_f64();
    let ask_num = q["bestAsk"].as_f64();
    if js_truthy(q.get("crossed")) || bid_num.is_none() || ask_num.is_none()
        || bid_num.unwrap() <= 0.0 || ask_num.unwrap() >= 1.0 || bid_num.unwrap() >= ask_num.unwrap() {
        return Ok(V::Null);
    }
    let mut values: Vec<BigInt> = a.iter().map(|(x, _)| big_amount(x["bid"].as_str().unwrap_or("")))
        .collect::<Result<_, _>>()?;
    values.sort();
    let anchor = values[(values.len() - 1) / 2].clone();
    let bid = big_amount(&price(bid_num.unwrap())?)?;
    let ask = big_amount(&price(ask_num.unwrap())?)?;
    let concession = &anchor - BigInt::from(2 * TICK) - &ask;
    if values.last().unwrap() - &values[0] > big_amount(p["maximumAnchorRange"].as_str().unwrap_or(""))?
        || &ask - &bid > big_amount(p["maximumSpread"].as_str().unwrap_or(""))?
        || &anchor - &bid > big_amount(p["maximumBidDrop"].as_str().unwrap_or(""))?
        || concession < big_amount(p["minimumNetConcession"].as_str().unwrap_or(""))? {
        return Ok(V::Null);
    }
    Ok(json!({
        "marketId": q["marketId"], "exchangeId": q["exchangeId"],
        "anchor": money(&anchor), "concession": money(&concession),
        "maximumAsk": money(&(&anchor - BigInt::from(2 * TICK)
            - big_amount(p["minimumNetConcession"].as_str().unwrap_or(""))?)),
        "referenceKind": "bulk_quoted_bid_not_executable", "depthVerified": false,
    }))
}

/// advanceBulkPrices(s, snapshot, bank, availableAt, {warmup})
pub fn advance_bulk_prices(s: &mut V, snapshot: &V, bank: &V, available_at: &str, warmup: bool) -> Result<(), String> {
    let canonical = validate_universe(&bank["universe"], bank["config"]["tournamentId"].as_str().unwrap_or(""))?;
    validate_price_observation_pub(snapshot, &canonical)?;
    let at = instant(available_at)?;
    need(ins(&snapshot["receivedAt"])? <= at && at >= ins(&s["bulk"]["lastAvailableAt"])?)?;
    s["bulk"]["lastAvailableAt"] = json!(available_at);
    let inc = |s: &mut V, k: &str| {
        s["bulk"]["counts"][k] = json!(s["bulk"]["counts"][k].as_i64().unwrap_or(0) + 1);
    };
    if !warmup && at - ins(&snapshot["receivedAt"])? > bank["bulkPolicy"]["maximumReceiptAgeMs"].as_i64().unwrap_or(0) {
        inc(s, "stale_price_receipt");
        s["bulk"]["candidates"] = json!({});
        return Ok(());
    }
    for mid in snapshot["missingIds"].as_array().cloned().unwrap_or_default() {
        let market = bank["universe"]["exchanges"].as_array().and_then(|xs| xs.iter()
            .find(|x| x["exchangeId"] == mid)).map(|x| x["marketId"].clone());
        if let Some(m) = market {
            if let Some(mk) = m.as_str() {
                s["bulk"]["history"].as_object_mut().map(|h| h.remove(mk));
                s["bulk"]["candidates"].as_object_mut().map(|h| h.remove(mk));
            }
        }
        inc(s, "missing_price_market");
    }
    for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
        let id = q["marketId"].as_str().unwrap_or("").to_string();
        let invalid = q["option"].as_str() != Some("YES")
            || js_truthy(q.get("crossed"))
            || q.get("bestBid").map_or(false, |v| v.is_null())
            || q.get("bestAsk").map_or(false, |v| v.is_null())
            || q["bestBid"].as_f64().map_or(false, |v| v <= 0.0)
            || q["bestAsk"].as_f64().map_or(false, |v| v >= 1.0)
            || (q["bestBid"].as_f64().is_some() && q["bestAsk"].as_f64().is_some()
                && q["bestBid"].as_f64().unwrap() >= q["bestAsk"].as_f64().unwrap());
        if invalid {
            s["bulk"]["history"].as_object_mut().map(|h| h.remove(&id));
            s["bulk"]["candidates"].as_object_mut().map(|h| h.remove(&id));
            inc(s, "invalid_reference");
            continue;
        }
        let mut points: Vec<V> = s["bulk"]["history"][&id].as_array().cloned().unwrap_or_default();
        let prior = s["bulk"]["candidates"][&id].clone();
        let candidate = if warmup { V::Null }
            else { quoted_dip_candidate(&points, &q, snapshot["receivedAt"].as_str().unwrap_or(""), Some(&bank["policy"]))? };
        s["bulk"]["candidates"].as_object_mut().map(|c| c.remove(&id));
        if !candidate.is_null() {
            let received = ins(&snapshot["receivedAt"])?;
            let mut reference_points: Vec<V> = Vec::new();
            for x in &points {
                if ins(&x["at"])? < received { reference_points.push(x.clone()); }
            }
            let mut cand = candidate.clone();
            cand["referencePoints"] = json!(reference_points);
            cand["observedAt"] = snapshot["receivedAt"].clone();
            cand["availableAt"] = json!(available_at);
            cand["expiresAt"] = json!(ms_to_iso((at + bank["bulkPolicy"]["candidateLifetimeMs"].as_i64().unwrap_or(0)) as f64));
            s["bulk"]["candidates"][&id] = cand;
            inc(s, "depth_candidate");
        } else if !warmup && !prior.is_null()
            && at <= ins(&prior["expiresAt"])?
            && big_amount(&price(q["bestAsk"].as_f64().unwrap_or(f64::NAN))?)?
                <= big_amount(prior["maximumAsk"].as_str().unwrap_or(""))?
            && big_amount(prior["anchor"].as_str().unwrap_or(""))?
                - big_amount(&price(q["bestBid"].as_f64().unwrap_or(f64::NAN))?)?
                <= big_amount(bank["policy"]["maximumBidDrop"].as_str().unwrap_or(""))?
            && big_amount(&price(q["bestAsk"].as_f64().unwrap_or(f64::NAN))?)?
                - big_amount(&price(q["bestBid"].as_f64().unwrap_or(f64::NAN))?)?
                <= big_amount(bank["policy"]["maximumSpread"].as_str().unwrap_or(""))? {
            s["bulk"]["candidates"][&id] = prior;
        }
        let minute = ins(&snapshot["receivedAt"])?.div_euclid(60000);
        if !points.iter().any(|x| x["minute"].as_i64() == Some(minute)) {
            points.push(json!({"minute":minute,"at":snapshot["receivedAt"],
                "bid":price(q["bestBid"].as_f64().unwrap_or(f64::NAN))?}));
        }
        let now = ins(&snapshot["receivedAt"])?;
        let history_ms = bank["policy"]["historyMs"].as_i64().unwrap_or(0);
        let mut filtered: Vec<V> = Vec::new();
        for x in points {
            if ins(&x["at"])? >= now - history_ms { filtered.push(x); }
        }
        s["bulk"]["history"][&id] = json!(filtered);
        inc(s, if warmup { "warmup_reference" } else { "fresh_reference" });
    }
    Ok(())
}

/// advanceBulkBook(s, row, bank, availableAt)
pub fn advance_bulk_book(s: &mut V, row: &V, bank: &V, available_at: &str) -> Result<(), String> {
    need(instant(available_at)? >= ins(&s["bulk"]["lastAvailableAt"])?)?;
    let kind = row["kind"].as_str().unwrap_or("");
    if ["session_start", "session_stop"].contains(&kind)
        || (kind == "invalidation" && row["payload"].get("marketId").map_or(false, |v| v.is_null())) {
        s["bulk"]["history"] = json!({});
        s["bulk"]["candidates"] = json!({});
    } else if kind == "invalidation" {
        if let Some(mid) = row["payload"]["marketId"].as_str() {
            let mid = mid.to_string();
            s["bulk"]["history"].as_object_mut().map(|h| h.remove(&mid));
            s["bulk"]["candidates"].as_object_mut().map(|h| h.remove(&mid));
        }
    }
    if kind == "book" {
        let b = row["payload"]["book"].clone();
        // References predate the depth request. The engine still verifies actual
        // spread/size and cannot fill a new intent on this same observation.
        let market_id = b["marketId"].as_str().unwrap_or("").to_string();
        let candidate = s["bulk"]["candidates"][&market_id].clone();
        let points: Vec<V> = if !candidate.is_null()
            && ins(&candidate["expiresAt"])? >= instant(available_at)? {
            candidate["referencePoints"].as_array().cloned().unwrap_or_default()
        } else {
            s["bulk"]["history"][&market_id].as_array().cloned().unwrap_or_default()
        };
        let req = ins(&b["requestStartedAt"])?;
        let mut filtered: Vec<V> = Vec::new();
        for x in points {
            if ins(&x["at"])? < req { filtered.push(x); }
        }
        let mut obj = serde_json::Map::new();
        obj.insert(market_id, json!(filtered));
        s["histories"] = V::Object(obj);
    }
    advance_live_bid_anchor(s, row, bank, available_at)?;
    Ok(())
}

/// bulkDipReport(s, bank, at)
pub fn bulk_dip_report(s: &V, bank: &V, at: &str) -> Result<V, String> {
    let mut r = live_bid_anchor_report(s, bank, at);
    let at_ms = instant(at)?;
    let candidates: Vec<V> = s["bulk"]["candidates"].as_object().map(|o| o.values()
        .filter(|x| ins(&x["expiresAt"]).unwrap_or(i64::MIN) >= at_ms).cloned().collect()).unwrap_or_default();
    r["mode"] = json!("market_wide_bulk_reference_paper");
    r["bulkReference"] = json!({
        "marketCount": s["bulk"]["history"].as_object().map(|h| h.len()).unwrap_or(0),
        "counts": s["bulk"]["counts"],
        "candidates": candidates,
    });
    r["limitations"] = json!([
        "Bulk prices supply quoted historical reference only; their source quote age and executable size are unknown.",
        "Every fill requires later independent verified depth, actual consumer receipt, original limit, adverse ticks and one-percent participation.",
        "Two latency wallets are correlated local counterfactuals, not contest orders or profit evidence."
    ]);
    Ok(r)
}
