// live-paper/liquidity.mjs — post-hoc liquidity/decision audit over a captured
// journal: per-market quote persistence, capacity, and per-horizon round-trip
// outcomes. Research diagnostics only.
use crate::history_policy::ISOLATED;
use crate::ml_bank::capture_rows;
use crate::roundtrip_target::round_trip_label;
use crate::snapshot_order::LEGACY_SNAPSHOT_ORDER;
use baal_paper::fixed::{big_amount, big_fee, TICK};
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::js_stringify;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::HashMap;

fn number(x: &V) -> Result<f64, String> {
    Ok(big_amount(x.as_str().unwrap_or(""))?.to_string().parse::<f64>().unwrap_or(0.0) / 1e6)
}
fn dp(v: &V) -> f64 { v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
/// JS object property key: numbers stringify, null -> "null".
fn prop_key(v: &V) -> String {
    if let Some(s) = v.as_str() { return s.to_string(); }
    if v.is_null() { return "null".to_string(); }
    js_stringify(v)
}
/// JS object iteration order: canonical array-index keys ascending, then
/// string keys in insertion order.
pub fn js_order_object(map: Map<String, V>) -> Map<String, V> {
    let mut numeric: Vec<(u64, String, V)> = Vec::new();
    let mut rest = Map::new();
    for (k, v) in map {
        let idx = if !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()) {
            k.parse::<u64>().ok().filter(|n| n.to_string() == k && *n <= 4_294_967_294)
        } else { None };
        match idx { Some(n) => numeric.push((n, k, v)), None => { rest.insert(k, v); } }
    }
    numeric.sort_by_key(|(n, _, _)| *n);
    let mut out = Map::new();
    for (_, k, v) in numeric { out.insert(k, v); }
    for (k, v) in rest { out.insert(k, v); }
    out
}

/// distribution(values) — {count,min,median,p95,max} over finite values.
pub fn distribution(values: &[f64]) -> V {
    let mut a: Vec<f64> = values.iter().cloned().filter(|v| v.is_finite()).collect();
    a.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let q = |p: f64| -> V {
        if a.is_empty() { V::Null } else { json!(a[((a.len() - 1) as f64 * p).floor() as usize]) }
    };
    json!({"count":a.len(),"min":q(0.0),"median":q(0.5),"p95":q(0.95),"max":q(1.0)})
}

fn fresh_market() -> V {
    json!({"uniqueDecisions":0,"warmup":0,"latencyMs":[],"sourceAgeMs":[],"gapMs":[],"spread":[],
        "askCapacity":[],"bidCapacity":[],"quoteUnchanged":0,"continuousPairs":0,"segmentBreaks":0,
        "askDepthChange":[],"bidDepthChange":[],"nextBidChange":[],"nextEntryCostChange":[],"horizons":{}})
}
fn fresh_horizon() -> V {
    json!({"decisions":0,"reasons":{},"enteredDecisions":0,"openDecisions":0,"missingLiquidation":0,
        "missingReference":0,"realizedPnl":[],"cashDelta":[],"fees":[],"referencePnl":[],"liquidationPnl":[],
        "outcomesByReason":{}})
}
fn fresh_group() -> V {
    json!({"count":0,"openDecisions":0,"missingLiquidation":0,"realizedPnl":[],"liquidationPnl":[],"fees":[]})
}
fn push_num(v: &mut V, key: &str, x: f64) {
    v[key].as_array_mut().map(|a| a.push(json!(x)));
}

/// decisionAudit(rows, c, cutoff, finalSegments, {horizons})
pub fn decision_audit(rows: &[V], c: &V, cutoff: &str, final_segments: &V, horizons: Option<&[i64]>) -> Result<V, String> {
    let horizons: Vec<i64> = horizons.map(|h| h.to_vec()).unwrap_or_else(|| vec![60, 180, 600]);
    if horizons.is_empty()
        || horizons.iter().any(|x| *x < 1)
        || horizons.iter().collect::<std::collections::HashSet<_>>().len() != horizons.len() {
        return Err("Invalid diagnostic horizons".to_string());
    }
    let mut order: Vec<String> = Vec::new();
    let mut markets: HashMap<String, V> = HashMap::new();
    let mut previous: HashMap<String, V> = HashMap::new();
    let depth = |levels: &V| -> f64 {
        levels.as_array().map(|ls| ls.iter()
            .map(|l| (l["quantity"].as_f64().unwrap_or(f64::NAN)
                * c["depthFractionBps"].as_f64().unwrap_or(f64::NAN) / 10000.0).floor())
            .sum()).unwrap_or(0.0)
    };
    let entry = |book: &V| -> Result<f64, String> {
        let a = big_amount(book["asks"][0]["price"].as_str().unwrap_or(""))?
            + BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0));
        let f = big_fee(&a, &BigInt::from(1),
            &json!({"feeBps":c["feeBps"],"feePerShare":"0.000000"}))?;
        Ok((&a + &f).to_string().parse::<f64>().unwrap_or(0.0) / 1e6)
    };
    for r in rows {
        let b = &r["book"];
        let mid = prop_key(&b["marketId"]);
        if !markets.contains_key(&mid) { markets.insert(mid.clone(), fresh_market()); order.push(mid.clone()); }
        let m = markets.get_mut(&mid).unwrap();
        m["uniqueDecisions"] = json!(m["uniqueDecisions"].as_i64().unwrap_or(0) + 1);
        if r["historyLength"].as_i64().unwrap_or(0) < 6 {
            m["warmup"] = json!(m["warmup"].as_i64().unwrap_or(0) + 1);
        }
        push_num(m, "latencyMs", dp(&b["receivedAt"]) - dp(&b["requestStartedAt"]));
        push_num(m, "sourceAgeMs", dp(&b["receivedAt"]) - dp(&b["sourceAt"]));
        push_num(m, "spread", number(&b["asks"][0]["price"])? - number(&b["bids"][0]["price"])?);
        let ask_depth = depth(&b["asks"]);
        let bid_depth = depth(&b["bids"]);
        push_num(m, "askCapacity", ask_depth);
        push_num(m, "bidCapacity", bid_depth);
        let inst = prop_key(&r["instrument"]);
        if let Some(old) = previous.get(&inst) {
            push_num(m, "gapMs", dp(&r["at"]) - dp(&old["at"]));
            if old["segment"] == r["segment"] {
                m["continuousPairs"] = json!(m["continuousPairs"].as_i64().unwrap_or(0) + 1);
                if old["book"]["asks"][0]["price"] == b["asks"][0]["price"]
                    && old["book"]["bids"][0]["price"] == b["bids"][0]["price"] {
                    m["quoteUnchanged"] = json!(m["quoteUnchanged"].as_i64().unwrap_or(0) + 1);
                }
                push_num(m, "askDepthChange", depth(&b["asks"]) - depth(&old["book"]["asks"]));
                push_num(m, "bidDepthChange", depth(&b["bids"]) - depth(&old["book"]["bids"]));
                push_num(m, "nextBidChange", number(&b["bids"][0]["price"])?
                    - number(&old["book"]["bids"][0]["price"])?);
                push_num(m, "nextEntryCostChange", entry(b)? - entry(&old["book"])?);
            } else {
                m["segmentBreaks"] = json!(m["segmentBreaks"].as_i64().unwrap_or(0) + 1);
            }
        }
        previous.insert(inst, r.clone());
    }
    for horizon in &horizons {
        for i in 0..rows.len() {
            let r = &rows[i];
            let mid = prop_key(&r["book"]["marketId"]);
            let m = markets.get_mut(&mid).unwrap();
            let hk = horizon.to_string();
            if m["horizons"][&hk].is_null() { m["horizons"][&hk] = fresh_horizon(); }
            let h = &mut m["horizons"][&hk];
            let terminal_invalidated = r["segment"] != final_segments[&mid];
            let outcome = round_trip_label(rows, i, c, horizon * 1000, true, Some(cutoff), terminal_invalidated)?;
            let o = outcome["outcome"].clone();
            h["decisions"] = json!(h["decisions"].as_i64().unwrap_or(0) + 1);
            let rk = prop_key(&o["reason"]);
            h["reasons"][&rk] = json!(h["reasons"][&rk].as_i64().unwrap_or(0) + 1);
            if truthy(&o["enteredQuantity"]) { h["enteredDecisions"] = json!(h["enteredDecisions"].as_i64().unwrap_or(0) + 1); }
            if truthy(&o["openQuantity"]) { h["openDecisions"] = json!(h["openDecisions"].as_i64().unwrap_or(0) + 1); }
            if o["liquidationPnl"].is_null() { h["missingLiquidation"] = json!(h["missingLiquidation"].as_i64().unwrap_or(0) + 1); }
            if o["referencePnl"].is_null() { h["missingReference"] = json!(h["missingReference"].as_i64().unwrap_or(0) + 1); }
            if h["outcomesByReason"][&rk].is_null() { h["outcomesByReason"][&rk] = fresh_group(); }
            {
                let g = &mut h["outcomesByReason"][&rk];
                g["count"] = json!(g["count"].as_i64().unwrap_or(0) + 1);
                if truthy(&o["openQuantity"]) { g["openDecisions"] = json!(g["openDecisions"].as_i64().unwrap_or(0) + 1); }
                if o["liquidationPnl"].is_null() { g["missingLiquidation"] = json!(g["missingLiquidation"].as_i64().unwrap_or(0) + 1); }
            }
            for key in ["realizedPnl", "cashDelta", "fees", "referencePnl", "liquidationPnl"] {
                if !o[key].is_null() { h[key].as_array_mut().map(|a| a.push(o[key].clone())); }
            }
            {
                let g = &mut h["outcomesByReason"][&rk];
                for key in ["realizedPnl", "liquidationPnl", "fees"] {
                    if !o[key].is_null() { g[key].as_array_mut().map(|a| a.push(o[key].clone())); }
                }
            }
        }
    }
    let mut out = Map::new();
    for mid in &order {
        let mut m = markets.get(mid).unwrap().clone();
        for key in ["latencyMs", "sourceAgeMs", "gapMs", "spread", "askCapacity", "bidCapacity",
            "askDepthChange", "bidDepthChange", "nextBidChange", "nextEntryCostChange"] {
            let vals: Vec<f64> = m[key].as_array().map(|a| a.iter()
                .map(|v| v.as_f64().unwrap_or(f64::NAN)).collect()).unwrap_or_default();
            m[key] = distribution(&vals);
        }
        let mut horizons_obj = Map::new();
        if let Some(hs) = m["horizons"].as_object() {
            for (hk, hval) in hs {
                let mut h = hval.clone();
                for key in ["realizedPnl", "cashDelta", "fees", "referencePnl", "liquidationPnl"] {
                    let vals: Vec<f64> = h[key].as_array().map(|a| a.iter()
                        .map(|v| v.as_f64().unwrap_or(f64::NAN)).collect()).unwrap_or_default();
                    h[key] = distribution(&vals);
                }
                let mut groups = Map::new();
                if let Some(gs) = h["outcomesByReason"].as_object() {
                    for (gk, gval) in gs {
                        let mut g = gval.clone();
                        for key in ["realizedPnl", "liquidationPnl", "fees"] {
                            let vals: Vec<f64> = g[key].as_array().map(|a| a.iter()
                                .map(|v| v.as_f64().unwrap_or(f64::NAN)).collect()).unwrap_or_default();
                            g[key] = distribution(&vals);
                        }
                        groups.insert(gk.clone(), g);
                    }
                }
                h["outcomesByReason"] = V::Object(groups);
                horizons_obj.insert(hk.clone(), h);
            }
        }
        m["horizons"] = V::Object(js_order_object(horizons_obj));
        out.insert(mid.clone(), m);
    }
    Ok(V::Object(js_order_object(out)))
}
fn truthy(v: &V) -> bool { baal_service_runtime::json_text::js_truthy(Some(v)) }

/// auditLiquidity(raw, c, {since, horizons})
pub fn audit_liquidity(raw: &str, c: &V, since: Option<&str>, horizons: Option<Vec<i64>>) -> Result<V, String> {
    let source = capture_rows(Some(raw), None, c, ISOLATED, since, false, LEGACY_SNAPSHOT_ORDER, "features_only_v1")?;
    let markets = decision_audit(source["rows"].as_array().map(|a| a.as_slice()).unwrap_or(&[]),
        c, source["cutoff"].as_str().unwrap_or(""), &source["finalSegments"], horizons.as_deref())?;
    let mut capture_health = json!({"accountReadOutages":0,"scopedSkips":{}});
    for line in raw.trim_end().split('\n') {
        let r: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let e = &r["event"];
        if since.is_some()
            && dp(if !e["clockAt"].is_null() { &e["clockAt"] } else { &r["at"] })
                < parse_iso_ms(since.unwrap()).unwrap_or(f64::NAN) {
            continue;
        }
        if e["type"] == "account_read_outage" {
            capture_health["accountReadOutages"] =
                json!(capture_health["accountReadOutages"].as_i64().unwrap_or(0) + 1);
        }
        let snaps: Vec<V> = [e["sourceSnapshots"].as_array().cloned().unwrap_or_default(),
            e["supplementalDepth"].as_array().cloned().unwrap_or_default()].concat();
        for s in snaps {
            let mid_str = match &s["marketId"] {
                V::String(x) => x.clone(),
                V::Number(n) => n.to_string(),
                V::Bool(b) => b.to_string(),
                V::Null => "null".to_string(),
                other => js_stringify(other),
            };
            if truthy(&s["skipped"])
                && c["marketIds"].as_array().map(|ms| ms.iter()
                    .any(|m| m.as_str() == Some(mid_str.as_str()))).unwrap_or(false) {
                let k = prop_key(&s["skipped"]);
                capture_health["scopedSkips"][&k] =
                    json!(capture_health["scopedSkips"][&k].as_i64().unwrap_or(0) + 1);
            }
        }
    }
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0);
    Ok(json!({
        "schemaVersion": 1, "createdAt": ms_to_iso(now_ms),
        "sourceHash": source["sourceHash"], "sourceRecords": source["sourceRecords"],
        "captureThrough": source["cutoff"], "windowStart": since,
        "targetPolicy": crate::roundtrip_target::ROUND_TRIP,
        "decisionCount": source["rows"].as_array().map(|a| a.len()).unwrap_or(0),
        "captureHealth": capture_health,
        "markets": markets,
        "interpretation": {
            "networkRequests": 0, "liveWalletsChanged": false, "profitabilityEstablished": false,
            "decisionUnit": "Unique accepted scoped book; warmup and every outcome retained",
            "counterfactual": "Independent flat-wallet entry probes using the preserved v1 label admission; not actual ML signals or a portfolio",
            "quoteUnchanged": "Top bid/ask prices unchanged across adjacent accepted versions in one uninterrupted segment; not continuous persistence",
            "depthChange": "Observed capacity difference at sampled levels; not proven replenishment or queue evidence",
            "nextPriceChange": "Next sampled book change with corresponding gap distribution; not signal-conditioned execution quality",
            "limitations": [
                "Overlapping probes are correlated and cannot be summed as portfolio PnL or independent trades",
                "Open inventory and missing liquidation are retained; zero realized PnL on an open probe does not mean zero loss",
                "Segment boundaries/outages suppress executable marks; stale or insufficient depth liquidation is unavailable",
                "REST snapshots do not establish maker fills or effective independent sample size",
                "Historical STOP-affected capture evidence still requires per-boundary audit; this report is exploratory"
            ]
        },
    }))
}
