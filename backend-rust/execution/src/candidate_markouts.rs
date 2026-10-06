// execution/candidate-markouts.mjs — hypothetical one-share bid recovery
// markouts for executed-ineligible decision candidates. Diagnostics only.
use crate::core::{decimal_micros, format_micros};
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy};
use serde_json::{json, Map, Value as V};
use std::collections::HashMap;

/// CANDIDATE_MARKOUT_POLICY (frozen).
pub fn candidate_markout_policy() -> V {
    json!({"horizonsMinutes": [1, 5, 15], "maxDelayMs": 60000, "maxBookLatencyMs": 20000,
        "episodeSpacingMs": 1800000, "slippageMicros": 5000, "quantity": 1, "depthFractionBps": 100})
}
fn check(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.to_string()) }
}
// x => typeof x === 'string' && Number.isFinite(Date.parse(x)) ? Date.parse(x) : NaN
fn time_ms(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}
fn signed(x: i128) -> String {
    if x < 0 { format!("-{}", format_micros(-x)) } else { format_micros(x) }
}
/// measureCandidateMarkouts({decisions, books, now, policy})
pub fn measure_candidate_markouts(input: &V) -> Result<V, String> {
    let decisions = input.get("decisions").unwrap_or(&V::Null);
    let books = input.get("books").unwrap_or(&V::Null);
    let now = input.get("now").unwrap_or(&V::Null);
    let default_policy = candidate_markout_policy();
    let policy = input.get("policy").cloned().unwrap_or_else(|| default_policy.clone());
    check(
        decisions.is_array() && books.is_array() && js_int(now).is_some() && js_stringify(&policy) == js_stringify(&default_policy),
        "INVALID_CANDIDATE_MARKOUT_INPUT",
    )?;
    let now = js_int(now).unwrap();
    let books = books.as_array().unwrap();
    let mut sorted: Vec<V> = decisions.as_array().unwrap().clone();
    sorted.sort_by(|a, b| {
        let (ta, tb) = (time_ms(a.get("receivedAt").unwrap_or(&V::Null)), time_ms(b.get("receivedAt").unwrap_or(&V::Null)));
        ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut last: HashMap<String, f64> = HashMap::new();
    let mut episodes: Vec<V> = Vec::new();
    let mut codes: Map<String, V> = Map::new();
    let mut repeated = 0i64;
    let spacing = policy["episodeSpacingMs"].as_f64().unwrap_or(0.0);
    for d in &sorted {
        let t_clock = time_ms(d.get("clockAt").unwrap_or(&V::Null));
        let t_recv = time_ms(d.get("receivedAt").unwrap_or(&V::Null));
        check(
            t_clock.is_finite() && !(t_clock < t_recv) && t_clock <= now as f64
                && t_recv.is_finite() && t_recv <= now as f64
                && d.get("marketId").map_or(false, V::is_string)
                && d.get("exchangeId").map_or(false, V::is_string)
                && d.get("code").map_or(false, V::is_string)
                && d.get("eligibleForExecution") == Some(&V::Bool(false)),
            "INVALID_CANDIDATE_DECISION",
        )?;
        let market_id = d["marketId"].as_str().unwrap().to_string();
        if let Some(prev) = last.get(&market_id) {
            if t_recv - prev < spacing {
                repeated += 1;
                continue;
            }
        }
        last.insert(market_id.clone(), t_recv);
        let code = d["code"].as_str().unwrap();
        let c = codes.get(code).and_then(V::as_i64).unwrap_or(0);
        codes.insert(code.to_string(), json!(c + 1));
        let mut matching: Vec<&V> = books
            .iter()
            .filter(|b| {
                b.get("marketId") == d.get("marketId") && b.get("exchangeId") == d.get("exchangeId")
                    && time_ms(b.get("receivedAt").unwrap_or(&V::Null)) > t_recv
                    && time_ms(b.get("receivedAt").unwrap_or(&V::Null)) <= now as f64
            })
            .collect();
        matching.sort_by(|a, b| {
            let (ta, tb) = (time_ms(a.get("receivedAt").unwrap_or(&V::Null)), time_ms(b.get("receivedAt").unwrap_or(&V::Null)));
            ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut marks = Vec::new();
        for minutes in policy["horizonsMinutes"].as_array().unwrap() {
            let minutes_i = js_int(minutes).unwrap_or(0);
            let depth_ok = js_int(d.get("bestAskQuantity").unwrap_or(&V::Null))
                .is_some_and(|q| (q as f64 * policy["depthFractionBps"].as_f64().unwrap_or(0.0) / 10000.0).floor() >= 1.0);
            if !depth_ok {
                marks.push(json!({"minutes": minutes_i, "status": "entry_depth_unavailable", "netPerShare": V::Null}));
                continue;
            }
            let target = t_clock + minutes_i as f64 * 60000.0;
            if target > now as f64 {
                marks.push(json!({"minutes": minutes_i, "status": "pending", "netPerShare": V::Null}));
                continue;
            }
            let b = matching.iter().find(|x| time_ms(x.get("receivedAt").unwrap_or(&V::Null)) >= target);
            let Some(&b) = b else {
                marks.push(json!({"minutes": minutes_i, "status": "unobserved", "netPerShare": V::Null}));
                continue;
            };
            let t_book = time_ms(b.get("receivedAt").unwrap_or(&V::Null));
            if t_book - target > policy["maxDelayMs"].as_f64().unwrap_or(0.0) {
                marks.push(json!({"minutes": minutes_i, "status": "late", "netPerShare": V::Null}));
                continue;
            }
            let t_started = time_ms(b.get("requestStartedAt").unwrap_or(&V::Null));
            if b.get("freshnessPolicy").and_then(V::as_str) != Some("authoritative_receipt_v1")
                || js_truthy(b.get("invalidated"))
                || b.get("valid") == Some(&V::Bool(false))
                || !t_started.is_finite() || t_started <= t_clock || t_started > t_book
                || t_book - t_started > policy["maxBookLatencyMs"].as_f64().unwrap_or(0.0)
                || b.get("versionKey") == d.get("versionKey")
            {
                marks.push(json!({"minutes": minutes_i, "status": "invalid_or_dependent", "netPerShare": V::Null}));
                continue;
            }
            let bid = b.get("bids").and_then(|a| a.as_array()).and_then(|a| a.first());
            let ask = b.get("asks").and_then(|a| a.as_array()).and_then(|a| a.first());
            let bid_price = bid.and_then(|x| x.get("price"));
            let ask_price = ask.and_then(|x| x.get("price"));
            let depth = bid.and_then(|x| x.get("quantity"));
            let crossed_or_thin = bid.is_none() || ask.is_none()
                || decimal_micros(bid_price.unwrap_or(&V::Null), "exact")? >= decimal_micros(ask_price.unwrap_or(&V::Null), "exact")?
                || !js_int(depth.unwrap_or(&V::Null))
                    .is_some_and(|q| (q as f64 * policy["depthFractionBps"].as_f64().unwrap_or(0.0) / 10000.0).floor() >= 1.0);
            if crossed_or_thin {
                marks.push(json!({"minutes": minutes_i, "status": "insufficient_or_crossed_depth", "netPerShare": V::Null}));
                continue;
            }
            let net = decimal_micros(bid_price.unwrap(), "exact")?
                - decimal_micros(d.get("ask").unwrap_or(&V::Null), "exact")?
                - 2 * policy["slippageMicros"].as_i64().unwrap_or(0) as i128;
            marks.push(json!({
                "minutes": minutes_i, "status": "observed", "netPerShare": signed(net),
                "receivedAt": b.get("receivedAt"), "requestStartedAt": b.get("requestStartedAt"),
                "versionKey": b.get("versionKey"),
            }));
        }
        episodes.push(json!({
            "marketId": d.get("marketId"), "exchangeId": d.get("exchangeId"),
            "decisionCode": d.get("code"), "receivedAt": d.get("receivedAt"),
            "ask": d.get("ask"), "marks": marks,
        }));
    }
    Ok(json!({
        "policy": policy,
        "episodes": episodes,
        "counts": {
            "decisions": decisions.as_array().unwrap().len(),
            "episodes": episodes.len(),
            "repeatedPolls": repeated,
            "decisionCodes": V::Object(codes),
        },
        "addedFees": "0.000000",
        "networkRequests": 0,
        "competitionOrders": 0,
        "profitabilityEstablished": false,
        "meaning": "Hypothetical one-share bid recovery with two-sided tick allowance; no assumed trade or execution.",
    }))
}
