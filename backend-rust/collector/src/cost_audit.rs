// live-paper/cost-audit.mjs — dataset validation and fee/slippage stress
// scenarios over derived normalized decisions. CLI entry omitted.
use crate::history_policy::ISOLATED;
use crate::liquidity::decision_audit;
use crate::ml_bank::capture_rows;
use crate::model::{digest, validate_config};
use crate::roundtrip_target::round_trip_label;
use crate::snapshot_order::LEGACY_SNAPSHOT_ORDER;
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy};
use serde_json::{json, Map, Value as V};

const ROW_KEYS: [&str; 6] = ["at", "marketId", "instrument", "historyLength", "segment", "book"];
const BOOK_KEYS: [&str; 10] = ["marketId", "exchangeId", "instrument", "receivedAt",
    "requestStartedAt", "sourceAt", "sequence", "versionKey", "bids", "asks"];

fn exact(v: &V, keys: &[&str]) -> Result<(), String> {
    if !js_truthy(Some(v)) { return Err("Unexpected dataset fields".to_string()); }
    let mut ks: Vec<String> = v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    ks.sort();
    let mut want: Vec<&str> = keys.to_vec();
    want.sort();
    if ks.join("|") != want.join("|") { return Err("Unexpected dataset fields".to_string()); }
    Ok(())
}
fn dp(v: &V) -> f64 { v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
/// JS string coercion for 'x'+':YES' style concat: number->digits, bool->true/false,
/// null->null, string->itself.
fn js_str(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => n.to_string(),
        V::Bool(b) => b.to_string(),
        V::Null => "null".to_string(),
        other => js_stringify(other),
    }
}
fn all_digits(v: &V) -> bool {
    let s = js_str(v);
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}
fn digit_pair(v: &V) -> bool {
    let s = js_str(v);
    let mut parts = s.split(':');
    let a = parts.next();
    let b = parts.next();
    parts.next().is_none()
        && a.is_some_and(|x| !x.is_empty() && x.bytes().all(|c| c.is_ascii_digit()))
        && b.is_some_and(|x| !x.is_empty() && x.bytes().all(|c| c.is_ascii_digit()))
}

/// validateDataset(d)
pub fn validate_dataset(d: &V) -> Result<V, String> {
    exact(d, &["schemaVersion", "kind", "windowStart", "cutoff", "finalSegments", "rows"])?;
    if d["schemaVersion"] != json!(1) || d["kind"] != "derived_normalized_market_decisions_v1"
        || !dp(&d["windowStart"]).is_finite() || !dp(&d["cutoff"]).is_finite() {
        return Err("Invalid dataset metadata".to_string());
    }
    let mut previous = dp(&d["windowStart"]);
    for r in d["rows"].as_array().cloned().unwrap_or_default() {
        exact(&r, &ROW_KEYS)?;
        exact(&r["book"], &BOOK_KEYS)?;
        let b = &r["book"];
        let instrument_ok = r["instrument"].as_str() == Some(format!("{}:YES", js_str(&b["exchangeId"])).as_str());
        let version_ok = b["versionKey"].as_str()
            == Some(format!("{}:{}", js_str(&b["sequence"]), js_str(&b["sourceAt"])).as_str());
        if !all_digits(&r["marketId"]) || !all_digits(&b["exchangeId"])
            || b["marketId"] != r["marketId"] || b["instrument"] != r["instrument"]
            || !instrument_ok || r["at"] != b["receivedAt"]
            || js_int(&r["historyLength"]).map_or(true, |h| h < 0)
            || js_int(&r["historyLength"]).is_none()
            || !digit_pair(&r["segment"]) || !all_digits(&b["sequence"]) || !version_ok {
            return Err("Invalid decision identity".to_string());
        }
        let times: Vec<f64> = [&r["at"], &b["requestStartedAt"], &b["sourceAt"]].iter().map(|v| dp(v)).collect();
        if times.iter().any(|x| !x.is_finite()) || times[0] < previous
            || times[0] > dp(&d["cutoff"]) || times[1] > times[0] {
            return Err("Invalid decision clock".to_string());
        }
        previous = times[0];
        for side in ["bids", "asks"] {
            let levels = &b[side];
            if levels.as_array().map(|a| a.is_empty()).unwrap_or(true) {
                return Err("Missing depth".to_string());
            }
            for l in levels.as_array().unwrap() {
                exact(l, &["price", "quantity"])?;
                let price_ok = l["price"].as_str().map(|p| {
                    (p.len() == 8 && p.starts_with("0.") && p[2..].bytes().all(|c| c.is_ascii_digit()))
                        || p == "1.000000"
                }).unwrap_or(false);
                if !price_ok || js_int(&l["quantity"]).map_or(true, |q| q < 1) {
                    return Err("Invalid depth".to_string());
                }
            }
        }
    }
    if let Some(fs) = d["finalSegments"].as_object() {
        for (id, segment) in fs {
            if !all_digits(&json!(id)) || !digit_pair(segment) {
                return Err("Invalid final segment".to_string());
            }
        }
    }
    Ok(d.clone())
}

/// exportDataset(raw, c, {since})
pub fn export_dataset(raw: &str, c: &V, since: Option<&str>) -> Result<V, String> {
    let source = capture_rows(Some(raw), None, c, ISOLATED, since, false, LEGACY_SNAPSHOT_ORDER, "features_only_v1")?;
    let mut rows = Vec::new();
    for r in source["rows"].as_array().cloned().unwrap_or_default() {
        let mut row = Map::new();
        for k in ROW_KEYS {
            if k == "book" {
                let mut book = Map::new();
                for f in BOOK_KEYS {
                    if f == "bids" || f == "asks" {
                        let levels: Vec<V> = r["book"][f].as_array().map(|a| a.iter()
                            .map(|l| json!({"price":l["price"],"quantity":l["quantity"]}))
                            .collect()).unwrap_or_default();
                        book.insert(f.to_string(), json!(levels));
                    } else {
                        book.insert(f.to_string(), r["book"][f].clone());
                    }
                }
                row.insert(k.to_string(), V::Object(book));
            } else {
                row.insert(k.to_string(), r[k].clone());
            }
        }
        rows.push(V::Object(row));
    }
    validate_dataset(&json!({
        "schemaVersion": 1, "kind": "derived_normalized_market_decisions_v1",
        "windowStart": since, "cutoff": source["cutoff"],
        "finalSegments": source["finalSegments"], "rows": rows,
    }))
}

/// compareCosts(dataset, c, {horizons})
pub fn compare_costs(dataset: &V, c: &V, horizons: Option<Vec<i64>>) -> Result<V, String> {
    validate_dataset(dataset)?;
    validate_config(c)?;
    let horizons = horizons.unwrap_or_else(|| vec![60]);
    if horizons.is_empty()
        || horizons.iter().any(|h| *h < 1)
        || horizons.iter().collect::<std::collections::HashSet<_>>().len() != horizons.len() {
        return Err("Invalid horizons".to_string());
    }
    let definitions: [(&str, i64, i64); 4] = [
        ("observed_price_zero_additional_fee", 0, 0), ("fee_10bps_stress", 10, 0),
        ("extra_tick_stress", 0, 1), ("fee_and_tick_stress", 10, 1)];
    let mut scenarios = Vec::with_capacity(4);
    let rows = dataset["rows"].as_array().cloned().unwrap_or_default();
    for (name, fee_bps, slippage_ticks) in definitions {
        let mut config = c.clone();
        config["feeBps"] = json!(fee_bps);
        config["slippageTicks"] = json!(slippage_ticks);
        let mut outcomes = Vec::new();
        for horizon in &horizons {
            for i in 0..rows.len() {
                let r = &rows[i];
                let label = round_trip_label(&rows, i, &config, horizon * 1000, true,
                    dataset["cutoff"].as_str(),
                    r["segment"] != dataset["finalSegments"][r["marketId"].as_str().unwrap_or("")])?;
                let mut o = Map::new();
                o.insert("horizon".to_string(), json!(horizon));
                if let Some(oo) = label["outcome"].as_object() {
                    for (k, v) in oo { o.insert(k.clone(), v.clone()); }
                }
                outcomes.push(V::Object(o));
            }
        }
        scenarios.push(json!({
            "name": name, "feeBps": fee_bps, "slippageTicks": slippage_ticks,
            "configHash": digest(&config),
            "markets": decision_audit(&rows, &config, dataset["cutoff"].as_str().unwrap_or(""),
                &dataset["finalSegments"], Some(&horizons))?,
            "outcomes": outcomes,
        }));
    }
    Ok(json!({
        "schemaVersion": 1, "datasetContentHash": digest(dataset), "baseConfigHash": digest(c),
        "decisionCount": rows.len(), "horizons": horizons, "scenarios": scenarios,
        "interpretation": {
            "actualWalletTrades": false, "profitabilityEstablished": false,
            "baseline": "Observed delayed snapshot execution prices and sampled depth; zero additional explicit fee and zero extra tick penalty. Does not establish all future platform fees.",
            "stress": "10 basis points on each fill and/or one adverse extra tick; explicit assumptions, not inferred Cup fees.",
            "fixed": "Same latency, depth fraction, order quantity, risk limits, warmup, FIFO and outage/censor rules in each scenario.",
            "caution": "Independent overlapping flat-wallet probes, not a portfolio, actual ML signals, held-out model performance or independent trades. Scenario admission and fills can differ near limits. Full quantities and censored outcomes retained. No sum of correlated outcomes is claimed as portfolio PnL.",
            "source": "Derived normalized accepted market decisions; cannot independently verify original private journal or source filtering. Original frozen bank source bindings unchanged."
        },
    }))
}
