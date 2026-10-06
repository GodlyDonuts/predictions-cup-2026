// frontend/app/comparison-view.mjs — comparison wallet views over paper reports.
use crate::frontend__app__wallet_view::paper_wallet_view;
use serde_json::{json, Value as V};

fn parse_ms(s: &str) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(s).unwrap_or(f64::NAN)
}
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn count(v: &V) -> i64 {
    // Number.isSafeInteger(x) && x >= 0 ? x : 0
    if v.as_f64().is_some_and(|x| x.fract() == 0.0 && x >= 0.0 && x <= 9007199254740991.0) {
        v.as_f64().unwrap() as i64
    } else { 0 }
}

pub fn comparison_cohorts() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![("comparison-dip", "Matched dip", "historical_dip"), ("comparison-rebound", "Matched rebound", "shock_rebound")]
}

pub fn comparison_name_valid(value: &V) -> bool {
    let Some(t) = value.as_str() else { return false };
    // /^[a-z][a-z0-9-]{0,63}$/ and not /^con|prn|aux|nul|com[1-9]|lpt[1-9]$/
    let shape = t.len() >= 1 && t.len() <= 64 && t.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && t.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    let reserved = matches!(t, "con" | "prn" | "aux" | "nul")
        || (t.len() == 4 && (t.starts_with("com") || t.starts_with("lpt")) && t.bytes().nth(3).is_some_and(|b| (b'1'..=b'9').contains(&b)));
    shape && !reserved
}

pub fn age_comparison_wallet(view: &V, now: Option<f64>) -> V {
    let now = now.unwrap_or_else(now_ms);
    if view["comparison"].is_null() { return view.clone(); }
    let mut v = view.clone();
    let age = now - parse_ms(v["updatedAt"].as_str().unwrap_or(""));
    let valid_clock = age.is_finite() && age >= 0.0;
    let fresh = valid_clock && age <= 60000.0 && v["status"].as_str() == Some("running");
    let limit = v["comparison"]["maxMarkAgeMs"].as_f64();
    let mut invalid_mark = !fresh;
    let promote_unsold = v["noMaker"].as_bool() == Some(true) || v["baalPair"].as_bool() == Some(true);
    if let Some(positions) = v["positions"].as_array_mut() {
        for p in positions.iter_mut() {
            let quote_age = now - parse_ms(p["quoteReceivedAt"].as_str().unwrap_or(""));
            let base_age = p["observationAgeMs"].as_f64();
            let valid = fresh && limit.is_some_and(|l| l.is_finite() && l > 0.0)
                && quote_age.is_finite() && quote_age >= 0.0 && quote_age <= limit.unwrap_or(0.0)
                && base_age.is_some_and(|b| b.is_finite() && b >= 0.0 && b + age <= limit.unwrap_or(0.0))
                && p["quoteInvalidated"].as_bool() != Some(true);
            p["quoteAgeMs"] = if quote_age.is_finite() && quote_age >= 0.0 { json!(quote_age) } else { V::Null };
            if !valid {
                invalid_mark = true;
                p["liquidationValue"] = V::Null;
                p["estimatedProceeds"] = V::Null;
                p["quoteInvalidated"] = json!(true);
                if promote_unsold {
                    let q = p["quantity"].clone();
                    p["unsoldShares"] = q;
                }
            }
        }
    }
    if invalid_mark {
        for k in ["referencePnl", "referenceEquity", "liquidationPnl", "equity"] {
            v["metrics"][k] = V::Null;
        }
    }
    if !fresh {
        v["status"] = json!(if valid_clock { "stale" } else { "invalid_clock" });
    }
    v
}

pub fn comparison_wallet_views(report: &V, manifest: &V, now: Option<f64>) -> Result<Vec<V>, String> {
    let now = now.unwrap_or_else(now_ms);
    if manifest["schemaVersion"].as_i64() != Some(1) || !comparison_name_valid(&manifest["cohort"])
        || !manifest["bankHash"].as_str().is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        || report["mode"].as_str() != Some("prospective_common_feed_entry_comparison_paper")
        || report["bankHash"] != manifest["bankHash"] || report["cohort"] != manifest["cohort"]
        || report["sharedExecution"]["feeBps"].as_i64() != Some(0)
        || report["networkRequests"].as_i64() != Some(0) || report["competitionOrders"].as_i64() != Some(0) {
        return Err("Comparison identity unavailable".into());
    }
    let age = now - parse_ms(report["updatedAt"].as_str().unwrap_or(""));
    let fresh = age.is_finite() && age >= 0.0 && age <= 60000.0 && report["status"].as_str() == Some("running");
    comparison_cohorts().into_iter().map(|(id, name, key)| {
        if report["strategies"][key].is_null() { return Err("Comparison wallet unavailable".into()); }
        let strategy = &report["strategies"][key];
        let mut v = paper_wallet_view(&json!({
            "updatedAt": report["updatedAt"],
            "status": if fresh { "running" } else if age.is_finite() && age >= 0.0 { "stale" } else { "invalid_clock" },
            "feeBps": 0,
            "observations": strategy["uniqueObservations"], "universeSize": report["marketIds"].as_array().map(|a| a.len()),
            "strategy": strategy,
            "performance": {"closedRoundTrips": strategy["closedTrades"]}}), id, name)?;
        v["comparison"] = json!({"cohort": manifest["cohort"], "cutoff": report["cutoff"],
            "maxMarkAgeMs": report["maxMarkAgeMs"],
            "acceptedSignals": count(&strategy["acceptedSignals"]),
            "contamination": {
                "account_exposure_or_orders": count(&strategy["contaminationCounts"]["account_exposure_or_orders"]),
                "no_reported_account_exposure": count(&strategy["contaminationCounts"]["no_reported_account_exposure"]),
                "unknown_account_context": count(&strategy["contaminationCounts"]["unknown_account_context"])}});
        Ok(age_comparison_wallet(&v, Some(now)))
    }).collect()
}
