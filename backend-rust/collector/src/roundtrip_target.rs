// live-paper/roundtrip-target.mjs — delayed FIFO roundtrip net label.
use baal_paper::fixed::{big_amount, big_fee, TICK};
use num_bigint::BigInt;
use serde_json::{json, Value as V};

pub const ROUND_TRIP: &str = "delayed_fifo_roundtrip_net_v1";
pub const LEGACY_TARGET: &str = "legacy_decision_time_bid_forecast_not_executable_roundtrip_target";
pub fn target_policy(value: &str) -> Result<&'static str, String> {
    match value { ROUND_TRIP => Ok(ROUND_TRIP), LEGACY_TARGET => Ok(LEGACY_TARGET), _ => Err("Unsupported target policy".into()) }
}
fn instant(s: &V) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(s.as_str().unwrap_or("")).unwrap_or(f64::NAN)
}
/// roundTripLabel(rows, index, c, horizonMs, {includeOutcome, asOf, terminalInvalidated})
pub fn round_trip_label(rows: &[V], index: usize, c: &V, horizon_ms: i64,
    include_outcome: bool, as_of: Option<&str>, terminal_invalidated: bool) -> Result<V, String> {
    let r = &rows[index];
    let mut s: Option<V> = None;
    let mut last_at = r["at"].clone();
    if let Some(a) = as_of {
        if instant(&json!(a)).is_nan() || instant(&json!(a)) < instant(&rows.last().unwrap()["at"]) {
            return Err("Invalid outcome cutoff".into());
        }
    }
    let finish = |result: V, at: &V, boundary: bool, s: &Option<V>| -> V {
        if !include_outcome { return result; }
        let mut snapshot = s.clone().unwrap_or_else(|| crate::model::initial_state(c, "diagnostic-empty", r["at"].as_str().unwrap_or("")).unwrap_or(json!({})));
        if boundary || (terminal_invalidated && !result["reason"].is_null()) {
            crate::model::invalidate(&mut snapshot, r["marketId"].as_str().or(r["book"]["marketId"].as_str()), at.as_str().unwrap_or(""), "diagnostic_source_boundary");
        }
        let st = &snapshot["strategies"]["mean_reversion"];
        let rep = crate::model::report(&snapshot, c, at.as_str().unwrap_or(""));
        let v = &rep["strategies"]["mean_reversion"];
        let buys: Vec<&V> = st["fills"].as_array().map(|f| f.iter().filter(|x| x["action"].as_str() == Some("buy")).collect()).unwrap_or_default();
        let sells: Vec<&V> = st["fills"].as_array().map(|f| f.iter().filter(|x| x["action"].as_str() == Some("sell")).collect()).unwrap_or_default();
        let inst = r["instrument"].as_str().unwrap_or("");
        let mut out = result.clone();
        out["outcome"] = json!({"decisionAt": r["at"], "observedThrough": at,
            "marketId": if !r["marketId"].is_null() { r["marketId"].clone() } else { r["book"]["marketId"].clone() },
            "instrument": r["instrument"],
            "reason": if result["reason"].is_null() { json!("completed") } else { result["reason"].clone() },
            "enteredQuantity": buys.iter().map(|x| x["quantity"].as_i64().unwrap_or(0)).sum::<i64>(),
            "soldQuantity": sells.iter().map(|x| x["quantity"].as_i64().unwrap_or(0)).sum::<i64>(),
            "openQuantity": st["positions"][inst]["quantity"].as_i64().unwrap_or(0),
            "pendingAction": st["pending"][inst]["action"],
            "cashDelta": (big_amount(st["cash"].as_str().unwrap_or("0")).unwrap_or(BigInt::from(0))
                - big_amount(c["initialCash"].as_str().unwrap_or("0")).unwrap_or(BigInt::from(0))).to_string().parse::<f64>().unwrap_or(0.0) / 1e6,
            "realizedPnl": big_amount(st["realized"].as_str().unwrap_or("0")).unwrap_or(BigInt::from(0)).to_string().parse::<f64>().unwrap_or(0.0) / 1e6,
            "fees": big_amount(st["fees"].as_str().unwrap_or("0")).unwrap_or(BigInt::from(0)).to_string().parse::<f64>().unwrap_or(0.0) / 1e6,
            "referencePnl": if v["referencePnl"].is_null() { V::Null } else { json!(big_amount(v["referencePnl"].as_str().unwrap_or("0")).unwrap_or(BigInt::from(0)).to_string().parse::<f64>().unwrap_or(0.0) / 1e6) },
            "liquidationPnl": if v["liquidationPnl"].is_null() { V::Null } else { json!(big_amount(v["liquidationPnl"].as_str().unwrap_or("0")).unwrap_or(BigInt::from(0)).to_string().parse::<f64>().unwrap_or(0.0) / 1e6) },
            "fillExecutions": st["fills"].as_array().map(|f| f.len()).unwrap_or(0),
            "lastObservationAt": s.as_ref().map(|x| x["books"][inst]["receivedAt"].clone()).unwrap_or(V::Null)});
        out
    };
    if r["historyLength"].as_i64().unwrap_or(-1) < 6 { return Ok(finish(json!({"reason":"decision_warmup"}), &last_at, false, &s)); }
    s = Some(crate::model::step_forecast(&crate::model::initial_state(c, "training-label", r["at"].as_str().unwrap_or(""))?,
        &r["book"], c, Some(1.0), None, horizon_ms, 1e-9)?);
    let inst = r["instrument"].as_str().unwrap_or("");
    if s.as_ref().unwrap()["strategies"]["mean_reversion"]["pending"][inst].is_null() {
        return Ok(finish(json!({"reason":"entry_not_admitted"}), &last_at, false, &s));
    }
    for f in &rows[index + 1..] {
        if f["instrument"] != r["instrument"] { continue; }
        if f["segment"] != r["segment"] { return Ok(finish(json!({"reason":"segment_boundary"}), &f["at"], true, &s)); }
        last_at = f["at"].clone();
        s = Some(crate::model::step_forecast(s.as_ref().unwrap(), &f["book"], c, None, None, horizon_ms, 1e-9)?);
        let st = &s.as_ref().unwrap()["strategies"]["mean_reversion"];
        let buys: Vec<&V> = st["fills"].as_array().map(|x| x.iter().filter(|x| x["action"].as_str() == Some("buy")).collect()).unwrap_or_default();
        if buys.is_empty() && st["pending"][inst].is_null() {
            return Ok(finish(json!({"reason":"entry_unfilled"}), &last_at, false, &s));
        }
        if !buys.is_empty() && st["positions"][inst].is_null() && st["pending"][inst].is_null() {
            let quantity = buys.iter().map(|x| x["quantity"].as_i64().unwrap_or(0)).sum::<i64>();
            let bid = big_amount(r["book"]["bids"][0]["price"].as_str().unwrap_or(""))? - BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0));
            let ask = big_amount(r["book"]["asks"][0]["price"].as_str().unwrap_or(""))? + BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0));
            let costs = json!({"feeBps": c["feeBps"], "feePerShare": "0.000000"});
            let q = BigInt::from(quantity);
            let persistence = ((&bid - &ask) * &q
                - big_fee(&(&bid * &q), &q, &costs)? - big_fee(&(&ask * &q), &q, &costs)?)
                .to_string().parse::<f64>().unwrap_or(0.0) / 1e6 / quantity as f64;
            let mut label = r.clone();
            label["y"] = json!(big_amount(st["realized"].as_str().unwrap_or("0"))?.to_string().parse::<f64>().unwrap_or(0.0) / 1e6 / quantity as f64);
            label["persistence"] = json!(persistence);
            label["labelAt"] = f["at"].clone();
            label["entryAt"] = buys[0]["at"].clone();
            label["quantity"] = json!(quantity);
            label["fillExecutions"] = json!(st["fills"].as_array().map(|x| x.len()).unwrap_or(0));
            return Ok(finish(json!({"label": label}), &last_at, false, &s));
        }
    }
    let reason = if !s.as_ref().unwrap()["strategies"]["mean_reversion"]["positions"][inst].is_null() { "exit_censored" } else { "entry_censored" };
    let end_at = as_of.map(|a| json!(a)).unwrap_or(last_at);
    Ok(finish(json!({"reason": reason}), &end_at, false, &s))
}
