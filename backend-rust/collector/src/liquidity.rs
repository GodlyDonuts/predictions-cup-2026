// live-paper/liquidity.mjs — decision-unit liquidity audit over capture_rows.
use baal_paper::fixed::{big_amount, big_fee, TICK};
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_int, js_truthy};
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::HashMap;

const ISOLATED: &str = "market-isolated-v2";

fn parse(v: &V) -> f64 { v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
fn js_str(v: &V) -> String {
    match v { V::String(s) => s.clone(), V::Number(n) => n.to_string(), V::Bool(b) => b.to_string(), _ => String::new() }
}
fn truthy(v: &V) -> bool { js_truthy(Some(v)) }
fn strict_null(v: &V, key: &str) -> bool { v.get(key).is_some_and(|x| x.is_null()) }
/// `Number(amount(x)) / 1e6` — decimal-string price/amount to f64 dollars.
fn number(v: &V) -> f64 {
    big_amount(&js_str(v)).map(|x| x.to_string().parse::<f64>().unwrap_or(f64::NAN) / 1e6).unwrap_or(f64::NAN)
}
/// JS object key order: integer-index keys ("0".."4294967294") ascend first,
/// then remaining keys in insertion order.
fn js_ordered(m: Map<String, V>) -> Map<String, V> {
    fn int_key(k: &str) -> Option<u64> {
        if k.is_empty() || k.len() > 10 || !k.bytes().all(|b| b.is_ascii_digit()) { return None; }
        if k.len() > 1 && k.starts_with('0') { return None; }
        k.parse::<u64>().ok().filter(|x| *x < 4_294_967_295)
    }
    let mut ints: Vec<(u64, String)> = m.keys().filter_map(|k| int_key(k).map(|i| (i, k.clone()))).collect();
    ints.sort_by_key(|x| x.0);
    let mut out = Map::new();
    for (_, k) in ints { out.insert(k.clone(), m[&k].clone()); }
    for (k, v) in m { if int_key(&k).is_none() { out.insert(k, v); } }
    out
}

/// distribution(values)
pub fn distribution(values: &[f64]) -> V {
    let mut a: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    a.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let q = |p: f64| if a.is_empty() { V::Null } else { json!(a[((a.len() - 1) as f64 * p).floor() as usize]) };
    json!({"count": a.len(), "min": q(0.0), "median": q(0.5), "p95": q(0.95), "max": q(1.0)})
}

/// decisionAudit(rows, c, cutoff, finalSegments, {horizons=[60,180,600]}).
/// `options` is the JS options object (`{"horizons": [...]}` or missing).
pub fn decision_audit(rows: &[V], c: &V, cutoff: &V, final_segments: &V, options: &V) -> Result<V, String> {
    let horizons: Vec<i64> = match options.get("horizons") {
        None => vec![60, 180, 600],
        Some(V::Array(a)) => a.iter().map(|x| js_int(x).unwrap_or(i64::MIN)).collect(),
        // null/non-array is a TypeError in JS; it lands in the same error path.
        Some(_) => Vec::new(),
    };
    if horizons.is_empty()
        || horizons.iter().any(|x| *x == i64::MIN || *x < 1)
        || horizons.iter().collect::<std::collections::BTreeSet<_>>().len() != horizons.len()
    {
        return Err("Invalid diagnostic horizons".into());
    }
    let depth_fraction = c["depthFractionBps"].as_f64().unwrap_or(f64::NAN);
    let depth = |levels: &V| -> i64 {
        levels.as_array().map(|a| a.iter()
            .map(|l| (l["quantity"].as_f64().unwrap_or(f64::NAN) * depth_fraction / 10000.0).floor() as i64)
            .sum()).unwrap_or(0)
    };
    let entry = |book: &V| -> Result<f64, String> {
        let a = big_amount(book["asks"][0]["price"].as_str().unwrap_or(""))?
            + BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0));
        let total = &a + &big_fee(&a, &BigInt::from(1), &json!({"feeBps": c["feeBps"], "feePerShare": "0.000000"}))?;
        Ok(total.to_string().parse::<f64>().unwrap_or(f64::NAN) / 1e6)
    };
    let mut market_order: Vec<String> = Vec::new();
    let mut markets: HashMap<String, V> = HashMap::new();
    let mut previous: HashMap<String, V> = HashMap::new();
    for r in rows {
        let b = &r["book"];
        let mkey = js_str(&b["marketId"]);
        let m = markets.entry(mkey.clone()).or_insert_with(|| {
            market_order.push(mkey.clone());
            json!({"uniqueDecisions": 0, "warmup": 0, "latencyMs": [], "sourceAgeMs": [], "gapMs": [],
                "spread": [], "askCapacity": [], "bidCapacity": [], "quoteUnchanged": 0,
                "continuousPairs": 0, "segmentBreaks": 0, "askDepthChange": [], "bidDepthChange": [],
                "nextBidChange": [], "nextEntryCostChange": [], "horizons": {}})
        });
        let mv = m.as_object_mut().unwrap();
        let inc = |k: &str, mv: &mut Map<String, V>| { mv[k] = json!(mv[k].as_i64().unwrap_or(0) + 1); };
        let push = |k: &str, v: f64, mv: &mut Map<String, V>| { mv[k].as_array_mut().unwrap().push(json!(v)); };
        inc("uniqueDecisions", mv);
        if r["historyLength"].as_f64().unwrap_or(f64::NAN) < 6.0 { inc("warmup", mv); }
        push("latencyMs", parse(&b["receivedAt"]) - parse(&b["requestStartedAt"]), mv);
        push("sourceAgeMs", parse(&b["receivedAt"]) - parse(&b["sourceAt"]), mv);
        push("spread", number(&b["asks"][0]["price"]) - number(&b["bids"][0]["price"]), mv);
        let ask_cap = depth(&b["asks"]);
        let bid_cap = depth(&b["bids"]);
        push("askCapacity", ask_cap as f64, mv);
        push("bidCapacity", bid_cap as f64, mv);
        let ikey = js_str(&r["instrument"]);
        if let Some(old) = previous.get(&ikey) {
            push("gapMs", parse(&r["at"]) - parse(&old["at"]), mv);
            if old["segment"] == r["segment"] {
                inc("continuousPairs", mv);
                if old["book"]["asks"][0]["price"] == b["asks"][0]["price"]
                    && old["book"]["bids"][0]["price"] == b["bids"][0]["price"]
                {
                    inc("quoteUnchanged", mv);
                }
                push("askDepthChange", (ask_cap - depth(&old["book"]["asks"])) as f64, mv);
                push("bidDepthChange", (bid_cap - depth(&old["book"]["bids"])) as f64, mv);
                push("nextBidChange", number(&b["bids"][0]["price"]) - number(&old["book"]["bids"][0]["price"]), mv);
                push("nextEntryCostChange", entry(b)? - entry(&old["book"])?, mv);
            } else {
                inc("segmentBreaks", mv);
            }
        }
        previous.insert(ikey, r.clone());
    }
    for &horizon in &horizons {
        for (i, r) in rows.iter().enumerate() {
            let mkey = js_str(&r["book"]["marketId"]);
            let m = markets.get_mut(&mkey).unwrap();
            let hmap = m["horizons"].as_object_mut().unwrap();
            let hkey = horizon.to_string();
            if !hmap.contains_key(&hkey) {
                hmap.insert(hkey.clone(), json!({"decisions": 0, "reasons": {}, "enteredDecisions": 0,
                    "openDecisions": 0, "missingLiquidation": 0, "missingReference": 0,
                    "realizedPnl": [], "cashDelta": [], "fees": [], "referencePnl": [],
                    "liquidationPnl": [], "outcomesByReason": {}}));
            }
            let h = hmap.get_mut(&hkey).unwrap().as_object_mut().unwrap();
            let terminal = r["segment"] != final_segments[js_str(&r["book"]["marketId"]).as_str()];
            let labelled = crate::roundtrip_target::round_trip_label(
                rows, i, c, horizon.saturating_mul(1000), true, cutoff.as_str(), terminal)?;
            let o = &labelled["outcome"];
            let inc = |k: &str, h: &mut Map<String, V>| { h[k] = json!(h[k].as_i64().unwrap_or(0) + 1); };
            inc("decisions", h);
            let reason = js_str(&o["reason"]);
            {
                let reasons = h["reasons"].as_object_mut().unwrap();
                reasons[&reason] = json!(reasons.get(&reason).and_then(|x| x.as_i64()).unwrap_or(0) + 1);
            }
            if truthy(&o["enteredQuantity"]) { inc("enteredDecisions", h); }
            if truthy(&o["openQuantity"]) { inc("openDecisions", h); }
            if strict_null(o, "liquidationPnl") { inc("missingLiquidation", h); }
            if strict_null(o, "referencePnl") { inc("missingReference", h); }
            {
                let groups = h["outcomesByReason"].as_object_mut().unwrap();
                if !groups.contains_key(&reason) {
                    groups.insert(reason.clone(), json!({"count": 0, "openDecisions": 0,
                        "missingLiquidation": 0, "realizedPnl": [], "liquidationPnl": [], "fees": []}));
                }
                let g = groups.get_mut(&reason).unwrap().as_object_mut().unwrap();
                g["count"] = json!(g["count"].as_i64().unwrap_or(0) + 1);
                if truthy(&o["openQuantity"]) { g["openDecisions"] = json!(g["openDecisions"].as_i64().unwrap_or(0) + 1); }
                if strict_null(o, "liquidationPnl") { g["missingLiquidation"] = json!(g["missingLiquidation"].as_i64().unwrap_or(0) + 1); }
                for key in ["realizedPnl", "liquidationPnl", "fees"] {
                    if !o[key].is_null() { g[key].as_array_mut().unwrap().push(o[key].clone()); }
                }
            }
            for key in ["realizedPnl", "cashDelta", "fees", "referencePnl", "liquidationPnl"] {
                if !o[key].is_null() { h[key].as_array_mut().unwrap().push(o[key].clone()); }
            }
        }
    }
    let mut out = Map::new();
    for mkey in &market_order {
        let m = markets.get_mut(mkey).unwrap().as_object_mut().unwrap();
        for key in ["latencyMs", "sourceAgeMs", "gapMs", "spread", "askCapacity", "bidCapacity",
            "askDepthChange", "bidDepthChange", "nextBidChange", "nextEntryCostChange"] {
            let vals: Vec<f64> = m[key].as_array().unwrap().iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect();
            m[key] = distribution(&vals);
        }
        {
            let hmap = m["horizons"].as_object_mut().unwrap();
            for (_, hv) in hmap.iter_mut() {
                let h = hv.as_object_mut().unwrap();
                for key in ["realizedPnl", "cashDelta", "fees", "referencePnl", "liquidationPnl"] {
                    let vals: Vec<f64> = h[key].as_array().unwrap().iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect();
                    h[key] = distribution(&vals);
                }
                let groups = h["outcomesByReason"].as_object_mut().unwrap();
                for (_, gv) in groups.iter_mut() {
                    let g = gv.as_object_mut().unwrap();
                    for key in ["realizedPnl", "liquidationPnl", "fees"] {
                        let vals: Vec<f64> = g[key].as_array().unwrap().iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect();
                        g[key] = distribution(&vals);
                    }
                }
            }
            let ordered = js_ordered(std::mem::take(hmap));
            *hmap = ordered;
        }
        out.insert(mkey.clone(), V::Object(js_ordered(m.clone())));
    }
    Ok(V::Object(js_ordered(out)))
}

/// auditLiquidity(raw, c, {since=null, horizons=[60,180,600]})
pub fn audit_liquidity(raw: &str, c: &V, options: &V) -> Result<V, String> {
    let since = options.get("since").filter(|s| s.is_string()).map(|s| s.as_str().unwrap().to_string());
    let source = crate::ml_bank::capture_rows(Some(raw), None, c, ISOLATED, since.as_deref(),
        false, crate::snapshot_order::LEGACY_SNAPSHOT_ORDER, "features_only_v1")?;
    let markets = decision_audit(source["rows"].as_array().map(|a| a.as_slice()).unwrap_or(&[]),
        c, &source["cutoff"], &source["finalSegments"], options)?;
    let mut capture_health = json!({"accountReadOutages": 0, "scopedSkips": {}});
    for line in raw.trim_end().split('\n') {
        let r: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let e = &r["event"];
        let clock = if truthy(&e["clockAt"]) { &e["clockAt"] } else { &r["at"] };
        if since.is_some() && parse(clock) < parse_iso_ms(since.as_deref().unwrap_or("")).unwrap_or(f64::NAN) { continue; }
        if e["type"].as_str() == Some("account_read_outage") {
            capture_health["accountReadOutages"] = json!(capture_health["accountReadOutages"].as_i64().unwrap_or(0) + 1);
        }
        for s in e["sourceSnapshots"].as_array().cloned().unwrap_or_default().iter()
            .chain(e["supplementalDepth"].as_array().cloned().unwrap_or_default().iter())
        {
            if truthy(&s["skipped"])
                && c["marketIds"].as_array().is_some_and(|a| a.iter().any(|x| x.as_str() == Some(js_str(&s["marketId"]).as_str())))
            {
                let key = js_str(&s["skipped"]);
                let m = capture_health["scopedSkips"].as_object_mut().unwrap();
                m[&key] = json!(m.get(&key).and_then(|x| x.as_i64()).unwrap_or(0) + 1);
            }
        }
    }
    let mut interpretation = Map::new();
    interpretation.insert("networkRequests".into(), json!(0));
    interpretation.insert("liveWalletsChanged".into(), json!(false));
    interpretation.insert("profitabilityEstablished".into(), json!(false));
    interpretation.insert("decisionUnit".into(), json!("Unique accepted scoped book; warmup and every outcome retained"));
    interpretation.insert("counterfactual".into(), json!("Independent flat-wallet entry probes using the preserved v1 label admission; not actual ML signals or a portfolio"));
    interpretation.insert("quoteUnchanged".into(), json!("Top bid/ask prices unchanged across adjacent accepted versions in one uninterrupted segment; not continuous persistence"));
    interpretation.insert("depthChange".into(), json!("Observed capacity difference at sampled levels; not proven replenishment or queue evidence"));
    interpretation.insert("nextPriceChange".into(), json!("Next sampled book change with corresponding gap distribution; not signal-conditioned execution quality"));
    interpretation.insert("limitations".into(), json!([
        "Overlapping probes are correlated and cannot be summed as portfolio PnL or independent trades",
        "Open inventory and missing liquidation are retained; zero realized PnL on an open probe does not mean zero loss",
        "Segment boundaries/outages suppress executable marks; stale or insufficient depth liquidation is unavailable",
        "REST snapshots do not establish maker fills or effective independent sample size",
        "Historical STOP-affected capture evidence still requires per-boundary audit; this report is exploratory",
    ]));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0);
    Ok(json!({
        "schemaVersion": 1, "createdAt": ms_to_iso(now), "sourceHash": source["sourceHash"],
        "sourceRecords": source["sourceRecords"], "captureThrough": source["cutoff"],
        "windowStart": options.get("since").cloned().unwrap_or(V::Null), "targetPolicy": crate::roundtrip_target::ROUND_TRIP,
        "decisionCount": source["rows"].as_array().map(|a| a.len()).unwrap_or(0),
        "captureHealth": capture_health, "markets": markets,
        "interpretation": V::Object(interpretation),
    }))
}
