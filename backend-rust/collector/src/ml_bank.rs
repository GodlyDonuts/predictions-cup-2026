// live-paper/ml-bank.mjs — feature extraction, model fitting, bank training.
use baal_paper::fixed::{big_amount, big_fee, TICK};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::{BTreeMap, BTreeSet};

fn num(x: &V) -> f64 { big_amount(x.as_str().unwrap_or("0")).unwrap_or(BigInt::from(0)).to_string().parse::<f64>().unwrap_or(0.0) / 1e6 }
fn parse_at(v: &V) -> f64 { baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN) }
/// features(book, history) -> 12-element vector.
pub fn features(book: &V, history: &[V]) -> Vec<f64> {
    let mid = (num(&book["bids"][0]["price"]) + num(&book["asks"][0]["price"])) / 2.0;
    let bid: i64 = book["bids"].as_array().map(|b| b.iter().take(3).map(|l| l["quantity"].as_i64().unwrap_or(0)).sum()).unwrap_or(0);
    let ask: i64 = book["asks"].as_array().map(|b| b.iter().take(3).map(|l| l["quantity"].as_i64().unwrap_or(0)).sum()).unwrap_or(0);
    let delta = |n: usize| -> f64 {
        if history.len() >= n { mid - history[history.len() - n]["mid"].as_f64().unwrap_or(0.0) } else { 0.0 }
    };
    let b0 = book["bids"][0]["quantity"].as_i64().unwrap_or(0);
    let a0 = book["asks"][0]["quantity"].as_i64().unwrap_or(0);
    vec![1.0,
        num(&book["asks"][0]["price"]) - num(&book["bids"][0]["price"]),
        delta(1), delta(3), delta(6),
        (b0 - a0) as f64 / (b0 + a0) as f64,
        (bid - ask) as f64 / (bid + ask) as f64,
        (bid as f64).ln_1p(), (ask as f64).ln_1p(),
        (parse_at(&book["receivedAt"]) - parse_at(&book["sourceAt"])) / 1000.0,
        if history.is_empty() { 0.0 } else { (parse_at(&book["receivedAt"]) - parse_at(&history.last().unwrap()["at"])) / 1000.0 },
        mid]
}
/// captureRows(raw|verified, c, {historyPolicy, since, breadthContext, snapshotOrderPolicy, observationValidation})
pub fn capture_rows(raw: Option<&str>, verified: Option<&crate::training_source::VerifiedSource>, c: &V,
    history_policy: &str, since: Option<&str>, breadth_context: bool, snapshot_order_policy: &str,
    observation_validation: &str) -> Result<V, String> {
    crate::model::validate_config(c)?;
    if !["features_only_v1","legacy_portfolio_validation_v1"].contains(&observation_validation) {
        return Err("Unknown feature observation validation".into());
    }
    crate::snapshot_order::validate_snapshot_order(snapshot_order_policy)?;
    crate::history_policy::history_policy(history_policy)?;
    if let Some(s) = since {
        if baal_service_runtime::contract::parse_iso_ms(s).is_none() { return Err("Invalid training window".into()); }
    }
    let source_hash;
    let source_records;
    let final_clock;
    let records: Vec<V>;
    if let Some(v) = verified {
        source_hash = v.source_hash().to_string();
        source_records = v.source_records() as i64;
        final_clock = v.final_event_clock();
        records = v.records.clone();
    } else {
        let rep = crate::replay::replay_journal(raw, None, c, snapshot_order_policy)?;
        source_hash = rep["replay"]["sourceHash"].as_str().unwrap_or("").to_string();
        source_records = rep["replay"]["sourceRecords"].as_i64().unwrap_or(0);
        final_clock = rep["replay"]["finalEventClock"].clone();
        records = raw.unwrap().trim_end().split('\n').map(|l| serde_json::from_str(l).unwrap()).collect();
    }
    let mut s: Option<V> = None;
    let mut segment = 0i64;
    let mut context: Option<V> = None;
    let mut histories = serde_json::Map::<String, V>::new();
    let mut rows: Vec<V> = Vec::new();
    let mut market_segments: BTreeMap<String, i64> = BTreeMap::new();
    let legacy = crate::history_policy::LEGACY;
    let isolated = crate::history_policy::ISOLATED;
    let mut reset = |market_id: Option<&str>, histories: &mut serde_json::Map<String, V>,
        segment: &mut i64, market_segments: &mut BTreeMap<String, i64>| -> Result<(), String> {
        crate::history_policy::reset_history(histories, market_id, history_policy)?;
        if history_policy == legacy || market_id.is_none() { *segment += 1; }
        else { *market_segments.entry(market_id.unwrap().into()).or_insert(0) += 1; }
        Ok(())
    };
    for r in &records {
        let e = &r["event"];
        let clock = e["clockAt"].as_str().or(r["at"].as_str()).unwrap_or("").to_string();
        if let Some(since) = since {
            if baal_service_runtime::contract::parse_iso_ms(&clock).unwrap_or(f64::NAN)
                < baal_service_runtime::contract::parse_iso_ms(since).unwrap_or(0.0) { continue; }
        }
        if e["type"].as_str() == Some("account_read_outage") {
            reset(None, &mut histories, &mut segment, &mut market_segments)?;
            if let Some(st) = s.as_mut() { crate::model::invalidate(st, None, &clock, e["reason"].as_str().unwrap_or("")); }
            continue;
        }
        if e["type"].as_str() != Some("cycle") { continue; }
        for snap in crate::snapshot_order::ordered_snapshots(e, c, snapshot_order_policy, Some(&clock))? {
            if !snap["skipped"].is_null() {
                let mid = snap["marketId"].as_str().unwrap_or("").to_string();
                reset(Some(&mid), &mut histories, &mut segment, &mut market_segments)?;
                if let Some(st) = s.as_mut() {
                    let at = if !snap["at"].is_null() { snap["at"].clone() } else { r["at"].clone() };
                    crate::model::invalidate(st, Some(&mid), at.as_str().unwrap_or(&clock), snap["skipped"].as_str().unwrap_or(""));
                }
                continue;
            }
            let endpoint = snap["endpoint"].as_str().unwrap_or("");
            let mid = endpoint.strip_prefix("/markets/").and_then(|x| x.split('/').next()).unwrap_or("").to_string();
            if mid.is_empty() || !endpoint.contains("/orderbook?") { continue; }
            if !c["marketIds"].as_array().map(|a| a.iter().any(|x| x.as_str() == Some(&mid))).unwrap_or(false) { continue; }
            if let Some(since) = since {
                if parse_at(&snap["requestStartedAt"]) < baal_service_runtime::contract::parse_iso_ms(since).unwrap_or(0.0) { continue; }
            }
            if s.is_none() {
                s = Some(crate::model::initial_state(c, "features", snap["requestStartedAt"].as_str().unwrap_or(""))?);
            }
            if observation_validation == "features_only_v1" {
                let st = s.as_mut().unwrap();
                st["strategies"] = json!({}); st["histories"] = json!({}); st["signals"] = json!({});
            }
            let excluded: BTreeSet<String> = e["accountExclusions"].as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            let b = match crate::model::normalize_book(&snap["data"], &mid, c["tournamentId"].as_str().unwrap_or(""),
                snap["receivedAt"].as_str().unwrap_or(""), snap["requestStartedAt"].as_str().unwrap_or(""),
                c["maxDataAgeMs"].as_i64().unwrap_or(0), &excluded, "authoritative_receipt_v1") {
                Ok(b) => b,
                Err(_) => { reset(Some(&mid), &mut histories, &mut segment, &mut market_segments)?; continue; }
            };
            let inst = b["instrument"].as_str().unwrap_or("").to_string();
            let prev = s.as_ref().unwrap()["books"][&inst].clone();
            let st = s.take().unwrap();
            s = Some(crate::model::observe(&st, &b, c)?);
            if prev["versionKey"] == s.as_ref().unwrap()["books"][&inst]["versionKey"] { continue; }
            let mut h: Vec<V> = histories.get(&inst).and_then(|v| v.as_array()).cloned().unwrap_or_default();
            if !h.is_empty() && parse_at(&b["receivedAt"]) - parse_at(&h.last().unwrap()["at"]) > 120000.0 {
                if history_policy == isolated {
                    *market_segments.entry(b["marketId"].as_str().unwrap_or("").into()).or_insert(0) += 1;
                } else { segment += 1; }
                h = Vec::new();
            }
            let base = features(&b, &h);
            let x = if breadth_context {
                let mut v = base.clone();
                v.extend(crate::breadth_context::context_features(context.as_ref().unwrap_or(&V::Null), &b,
                    c["tournamentId"].as_str().unwrap_or("")).unwrap_or([0.0; 7]));
                v
            } else { base };
            let seg_val = if history_policy == isolated {
                json!(format!("{}:{}", segment, market_segments.get(b["marketId"].as_str().unwrap_or("")).copied().unwrap_or(0)))
            } else { json!(segment) };
            let row = json!({"at": b["receivedAt"], "marketId": b["marketId"], "instrument": b["instrument"],
                "historyLength": h.len(), "segment": seg_val, "x": x, "mid": x[11],
                "bid": num(&b["bids"][0]["price"]), "ask": num(&b["asks"][0]["price"]), "book": b});
            h.push(row.clone());
            if h.len() > 30 { h.remove(0); }
            histories.insert(inst, json!(h));
            rows.push(row);
        }
        if breadth_context {
            context = Some(crate::breadth_context::update_context(context.as_ref(), Some(&e["bulkPrices"]), Some(&clock))?);
        }
    }
    if verified.is_some() && since.is_none() && rows.is_empty() {
        return Err("No accepted real source observations".into());
    }
    let mut final_segments = serde_json::Map::new();
    for m in c["marketIds"].as_array().cloned().unwrap_or_default() {
        let m = m.as_str().unwrap_or("").to_string();
        final_segments.insert(m.clone(), if history_policy == isolated {
            json!(format!("{}:{}", segment, market_segments.get(&m).copied().unwrap_or(0)))
        } else { json!(segment) });
    }
    Ok(json!({"rows": rows, "finalSegments": final_segments, "sourceHash": source_hash,
        "sourceRecords": source_records, "cutoff": final_clock}))
}
/// fitRidge(rows, width, lambda)
pub fn fit_ridge(rows: &[V], width: usize, lambda: f64) -> Result<V, String> {
    if rows.len() < 20 { return Err("Insufficient training labels".into()); }
    let means: Vec<f64> = (0..width).map(|j| if j == 0 { 0.0 } else { rows.iter().map(|r| r["x"][j].as_f64().unwrap_or(0.0)).sum::<f64>() / rows.len() as f64 }).collect();
    let scales: Vec<f64> = means.iter().enumerate().map(|(j, m)| {
        if j == 0 { 1.0 } else { (rows.iter().map(|r| (r["x"][j].as_f64().unwrap_or(0.0) - m).powi(2)).sum::<f64>() / rows.len() as f64).sqrt().max(1e-6) }
    }).collect();
    let mut a = vec![vec![0.0f64; width + 1]; width];
    for r in rows {
        let x: Vec<f64> = (0..width).map(|j| (r["x"][j].as_f64().unwrap_or(0.0) - means[j]) / scales[j]).collect();
        for i in 0..width {
            for j in 0..width { a[i][j] += x[i] * x[j]; }
            a[i][width] += x[i] * r["y"].as_f64().unwrap_or(0.0);
        }
    }
    for i in 1..width { a[i][i] += lambda; }
    for i in 0..width {
        let mut p = i;
        for j in i + 1..width { if a[j][i].abs() > a[p][i].abs() { p = j; } }
        a.swap(i, p);
        if a[i][i].abs() < 1e-12 { return Err("Singular model".into()); }
        let v = a[i][i];
        for j in i..=width { a[i][j] /= v; }
        for k in 0..width {
            if k != i {
                let f = a[k][i];
                for j in i..=width { a[k][j] -= f * a[i][j]; }
            }
        }
    }
    Ok(json!({"width": width, "lambda": lambda, "means": means, "scales": scales,
        "weights": a.iter().map(|r| r[width]).collect::<Vec<_>>()}))
}
/// fitStumps(rows, width, iterations, learningRate, minLeaf)
pub fn fit_stumps(rows: &[V], width: usize, iterations: usize, learning_rate: f64, min_leaf: usize) -> Result<V, String> {
    if rows.len() < 20 || min_leaf < 2 || iterations < 1 || learning_rate <= 0.0 || learning_rate > 1.0 {
        return Err("Invalid boosted training data".into());
    }
    let base = rows.iter().map(|r| r["y"].as_f64().unwrap_or(0.0)).sum::<f64>() / rows.len() as f64;
    let mut predictions = vec![base; rows.len()];
    let mut stumps: Vec<V> = Vec::new();
    let mut splits = Vec::new();
    for feature in 1..width {
        let mut sorted: Vec<f64> = rows.iter().map(|r| r["x"][feature].as_f64().unwrap_or(0.0)).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let thresholds: BTreeSet<String> = (1..9).map(|q| format!("{:e}", sorted[sorted.len() * q / 9])).collect();
        for ts in thresholds {
            let threshold: f64 = ts.parse().unwrap_or(f64::NAN);
            let (mut left, mut right) = (Vec::new(), Vec::new());
            for (i, r) in rows.iter().enumerate() {
                if r["x"][feature].as_f64().unwrap_or(0.0) <= threshold { left.push(i); } else { right.push(i); }
            }
            if left.len() >= min_leaf && right.len() >= min_leaf { splits.push((feature, threshold, left, right)); }
        }
    }
    for _ in 0..iterations {
        let residuals: Vec<f64> = rows.iter().enumerate().map(|(i, r)| r["y"].as_f64().unwrap_or(0.0) - predictions[i]).collect();
        let mut best: Option<(usize, f64, f64, f64, f64)> = None;
        for (feature, threshold, left, right) in &splits {
            let l: f64 = left.iter().map(|i| residuals[*i]).sum();
            let r: f64 = right.iter().map(|i| residuals[*i]).sum();
            let gain = l * l / (left.len() as f64 + 10.0) + r * r / (right.len() as f64 + 10.0);
            if best.is_none() || gain > best.unwrap().4 {
                best = Some((*feature, *threshold, gain,
                    learning_rate * l / (left.len() as f64 + 10.0), learning_rate * r / (right.len() as f64 + 10.0)));
            }
        }
        let (feature, threshold, gain, left_value, right_value) = match best {
            Some(b) if b.2 >= 1e-15 => b,
            _ => break,
        };
        let _ = gain;
        stumps.push(json!({"feature": feature, "threshold": threshold, "leftValue": left_value, "rightValue": right_value}));
        for (i, r) in rows.iter().enumerate() {
            predictions[i] += if r["x"][feature].as_f64().unwrap_or(0.0) <= threshold { left_value } else { right_value };
        }
    }
    Ok(json!({"family": "boosted_stumps", "width": width, "iterations": iterations,
        "learningRate": learning_rate, "minLeaf": min_leaf, "base": base, "stumps": stumps}))
}
/// predict(m, x)
pub fn predict(m: &V, x: &[f64]) -> Result<f64, String> {
    match m["family"].as_str().unwrap_or("") {
        "gpu_mlp" => crate::gpu_inference::predict_mlp(m, x),
        "boosted_stumps" => Ok(m["stumps"].as_array().map(|s| s.iter().fold(m["base"].as_f64().unwrap_or(0.0), |n, s| {
            n + if x[s["feature"].as_u64().unwrap_or(0) as usize] <= s["threshold"].as_f64().unwrap_or(0.0) {
                s["leftValue"].as_f64().unwrap_or(0.0)
            } else { s["rightValue"].as_f64().unwrap_or(0.0) }
        })).unwrap_or(0.0)),
        _ => Ok(m["weights"].as_array().map(|w| w.iter().enumerate().fold(0.0, |n, (j, w)| {
            n + w.as_f64().unwrap_or(0.0) * (x[j] - m["means"][j].as_f64().unwrap_or(0.0)) / m["scales"][j].as_f64().unwrap_or(1.0)
        })).unwrap_or(0.0)),
    }
}
/// instrumentCoverage(train, test, marketIds)
pub fn instrument_coverage(train: &[V], test: &[V], market_ids: &[String]) -> Vec<V> {
    market_ids.iter().map(|market_id| {
        let a: Vec<&V> = train.iter().filter(|r| r["marketId"].as_str() == Some(market_id)).collect();
        let b: Vec<&V> = test.iter().filter(|r| r["marketId"].as_str() == Some(market_id)).collect();
        let training_instruments: BTreeSet<String> = a.iter().filter_map(|r| r["instrument"].as_str().map(String::from)).collect();
        let test_instruments: BTreeSet<String> = b.iter().filter_map(|r| r["instrument"].as_str().map(String::from)).collect();
        json!({"marketId": market_id, "train": a.len(), "test": b.len(),
            "trainingInstruments": training_instruments.iter().collect::<Vec<_>>(),
            "testInstruments": test_instruments.iter().collect::<Vec<_>>(),
            "eligible": a.len() >= 8 && b.len() >= 4 && test_instruments.iter().all(|i| training_instruments.contains(i))})
    }).collect()
}
pub fn validate_training_scope(scope: &str, coverage: bool, target: &str) -> Result<(), String> {
    if !["all-markets-v1","supported-instruments-v1"].contains(&scope)
        || (scope == "supported-instruments-v1" && (!coverage || target != crate::roundtrip_target::ROUND_TRIP)) {
        return Err("Supported scope requires coverage-gated roundtrip target".into());
    }
    Ok(())
}
/// trainingSupport(train, test, marketIds, scope)
pub fn training_support(train: Vec<V>, test: Vec<V>, market_ids: &[String], scope: &str) -> Result<V, String> {
    if !["all-markets-v1","supported-instruments-v1"].contains(&scope) { return Err("Invalid training scope".into()); }
    let coverage = instrument_coverage(&train, &test, market_ids);
    let supported: BTreeSet<String> = coverage.iter().filter(|x| x["eligible"].as_bool().unwrap_or(false))
        .flat_map(|x| x["trainingInstruments"].as_array().unwrap().iter().map(|i| format!("{}:{}", x["marketId"].as_str().unwrap(), i.as_str().unwrap())).collect::<Vec<_>>()).collect();
    let filter = |rows: Vec<V>| -> Vec<V> {
        if scope == "all-markets-v1" { rows } else {
            rows.into_iter().filter(|r| supported.contains(&format!("{}:{}", r["marketId"].as_str().unwrap_or(""), r["instrument"].as_str().unwrap_or("")))).collect()
        }
    };
    let train_len = train.len();
    let test_len = test.len();
    let train = filter(train);
    let test = filter(test);
    let eligible = if scope == "all-markets-v1" {
        coverage.iter().all(|x| x["eligible"].as_bool().unwrap_or(false))
    } else { coverage.iter().any(|x| x["eligible"].as_bool().unwrap_or(false)) };
    let _ = (train_len, test_len);
    Ok(json!({"coverage": coverage, "train": train, "test": test, "eligible": eligible}))
}
/// trainBank(source, c, options) — options keys mirror the JS defaults.
#[allow(clippy::too_many_arguments)]
pub fn train_bank(raw: Option<&str>, verified: Option<&crate::training_source::VerifiedSource>, c: &V,
    family: &str, history_policy: &str, since: Option<&str>, require_instrument_coverage: bool,
    breadth_context: bool, target_policy: &str, training_scope: &str, development_split_at: Option<&str>,
    snapshot_order_policy: &str, horizons: &[i64]) -> Result<V, String> {
    let horizons = crate::training_horizons::training_horizons(horizons)?;
    crate::snapshot_order::validate_snapshot_order(snapshot_order_policy)?;
    crate::roundtrip_target::target_policy(target_policy)?;
    validate_training_scope(training_scope, require_instrument_coverage, target_policy)?;
    if !["ridge","boosted_stumps"].contains(&family) { return Err("Unsupported model family".into()); }
    let source = capture_rows(raw, verified, c, history_policy, since, breadth_context, snapshot_order_policy, "features_only_v1")?;
    let rows: Vec<V> = source["rows"].as_array().cloned().unwrap_or_default();
    if rows.len() < 80 { return Err("Need at least 80 accepted snapshots before model fitting".into()); }
    let split = development_split_at.map(String::from)
        .unwrap_or_else(|| rows[rows.len() * 65 / 100]["at"].as_str().unwrap_or("").to_string());
    let mut models: Vec<V> = Vec::new();
    let mut rejected: Vec<V> = Vec::new();
    let mut label_diagnostics: Vec<V> = Vec::new();
    let label_paths = if target_policy == crate::roundtrip_target::ROUND_TRIP {
        Some(crate::label_paths::instrument_label_paths(&rows))
    } else { None };
    if baal_service_runtime::contract::parse_iso_ms(&split).is_none()
        || baal_service_runtime::contract::parse_iso_ms(&split).unwrap() <= parse_at(&rows[0]["at"])
        || baal_service_runtime::contract::parse_iso_ms(&split).unwrap() >= parse_at(&rows.last().unwrap()["at"]) {
        return Err("Invalid development split".into());
    }
    let market_ids: Vec<String> = c["marketIds"].as_array().cloned().unwrap_or_default().iter()
        .filter_map(|x| x.as_str().map(String::from)).collect();
    let split_ms = baal_service_runtime::contract::parse_iso_ms(&split).unwrap();
    for horizon in &horizons {
        let mut labels: Vec<V> = Vec::new();
        let mut reasons: BTreeMap<String, i64> = BTreeMap::new();
        let mut by_market: BTreeMap<String, V> = BTreeMap::new();
        for i in 0..rows.len() {
            let r = &rows[i];
            let mid = r["marketId"].as_str().unwrap_or("").to_string();
            let diagnostic = by_market.entry(mid.clone()).or_insert_with(|| json!({"marketId": mid, "observations": 0, "accepted": 0, "rejected": {}}));
            diagnostic["observations"] = json!(diagnostic["observations"].as_i64().unwrap_or(0) + 1);
            if target_policy == crate::roundtrip_target::ROUND_TRIP {
                let lp = label_paths.as_ref().unwrap();
                let (sid, ord) = lp.paths[i];
                let stream: Vec<V> = lp.streams[sid].iter().map(|j| rows[*j].clone()).collect();
                let result = crate::roundtrip_target::round_trip_label(&stream, ord, c, horizon * 1000, false, None, false)?;
                if !result["label"].is_null() {
                    labels.push(result["label"].clone());
                    diagnostic["accepted"] = json!(diagnostic["accepted"].as_i64().unwrap_or(0) + 1);
                } else {
                    let reason = result["reason"].as_str().unwrap_or("").to_string();
                    *reasons.entry(reason.clone()).or_insert(0) += 1;
                    let d = diagnostic["rejected"].as_object().map(|o| o.get(&reason).and_then(|v| v.as_i64()).unwrap_or(0)).unwrap_or(0);
                    diagnostic["rejected"][&reason] = json!(d + 1);
                }
                continue;
            }
            let mut future: Option<&V> = None;
            for f in &rows[i + 1..] {
                if f["instrument"] != r["instrument"] { continue; }
                if f["segment"] != r["segment"] { break; }
                let dt = parse_at(&f["at"]) - parse_at(&r["at"]);
                if dt >= *horizon as f64 * 1000.0 {
                    if dt <= *horizon as f64 * 1000.0 + 120000.0 { future = Some(f); }
                    break;
                }
            }
            let future = match future { Some(f) => f, None => continue };
            let exit = big_amount(future["book"]["bids"][0]["price"].as_str().unwrap_or("0"))?
                - BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0));
            if exit <= BigInt::from(0) { continue; }
            let current_exit = big_amount(r["book"]["bids"][0]["price"].as_str().unwrap_or("0"))?
                - BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0));
            let costs = json!({"feeBps": c["feeBps"], "feePerShare": "0.000000"});
            let persistence = (current_exit.clone() - big_fee(&current_exit, &BigInt::from(1), &costs)?)
                .to_string().parse::<f64>().unwrap_or(0.0) / 1e6 - r["mid"].as_f64().unwrap_or(0.0);
            diagnostic["accepted"] = json!(diagnostic["accepted"].as_i64().unwrap_or(0) + 1);
            let mut label = r.clone();
            label["labelAt"] = future["at"].clone();
            label["persistence"] = json!(persistence);
            label["y"] = json!((exit.clone() - big_fee(&exit, &BigInt::from(1), &costs)?).to_string().parse::<f64>().unwrap_or(0.0) / 1e6 - r["mid"].as_f64().unwrap_or(0.0));
            labels.push(label);
        }
        let raw_train: Vec<V> = labels.iter().filter(|r| parse_at(&r["labelAt"]) < split_ms).cloned().collect();
        let raw_test: Vec<V> = labels.iter().filter(|r| parse_at(&r["at"]) >= split_ms).cloned().collect();
        let support = training_support(raw_train.clone(), raw_test.clone(), &market_ids, training_scope)?;
        let train: Vec<V> = support["train"].as_array().cloned().unwrap_or_default();
        let test: Vec<V> = support["test"].as_array().cloned().unwrap_or_default();
        let coverage = support["coverage"].clone();
        label_diagnostics.push(json!({"horizon": horizon, "accepted": labels.len(), "train": train.len(), "test": test.len(),
            "beforeSupport": {"train": raw_train.len(), "test": raw_test.len()},
            "excludedUnsupported": {"train": raw_train.len() - train.len(), "test": raw_test.len() - test.len()},
            "rejected": reasons,
            "markets": by_market.values().map(|d| {
                let mut d = d.clone();
                d["coverage"] = coverage.as_array().unwrap().iter().find(|x| x["marketId"] == d["marketId"]).cloned().unwrap_or(V::Null);
                d
            }).collect::<Vec<_>>()}));
        let mut variants: Vec<(String, usize, f64, usize, f64, usize)> = Vec::new();
        for width in if breadth_context { vec![12, 15, 17, 19] } else { vec![4, 6, 9, 12] } {
            if family == "ridge" {
                for lambda in [0.01, 0.1, 1.0, 10.0, 100.0, 1000.0, 10000.0, 100000.0] {
                    variants.push((format!("ridge-h{}-f{}-l{}", horizon, width, lambda), width, lambda, 0, 0.0, 0));
                }
            } else {
                for iterations in [16usize, 64] {
                    for learning_rate in [0.05, 0.1] {
                        for min_leaf in [5usize, 10] {
                            variants.push((format!("stumps-h{}-f{}-n{}-r{}-leaf{}", horizon, width, iterations, learning_rate, min_leaf), width, 0.0, iterations, learning_rate, min_leaf));
                        }
                    }
                }
            }
        }
        for (id, width, lambda, iterations, learning_rate, min_leaf) in variants {
            if train.len() < 20 || test.len() < 10 {
                rejected.push(json!({"id": id, "reason": "insufficient_matured_labels", "train": train.len(), "test": test.len()}));
                continue;
            }
            if require_instrument_coverage && support["eligible"] != V::Bool(true) {
                rejected.push(json!({"id": id, "reason": "insufficient_instrument_coverage", "coverage": coverage}));
                continue;
            }
            let model = if family == "ridge" { fit_ridge(&train, width, lambda)? } else { fit_stumps(&train, width, iterations, learning_rate, min_leaf)? };
            let errors: Vec<f64> = test.iter().map(|r| predict(&model, &r["x"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap_or(0.0)).collect::<Vec<_>>()).unwrap_or(0.0) - r["y"].as_f64().unwrap_or(0.0)).collect();
            let rmse = (errors.iter().map(|v| v * v).sum::<f64>() / errors.len() as f64).sqrt();
            let baseline_rmse = (test.iter().map(|r| (r["y"].as_f64().unwrap_or(0.0) - r["persistence"].as_f64().unwrap_or(0.0)).powi(2)).sum::<f64>() / test.len() as f64).sqrt();
            let mut m = json!({"id": id, "horizon": horizon});
            for (k, v) in model.as_object().unwrap() { m[k] = v.clone(); }
            if target_policy == crate::roundtrip_target::ROUND_TRIP {
                let preds: Vec<f64> = test.iter().map(|r| predict(&model, &r["x"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap_or(0.0)).collect::<Vec<_>>()).unwrap_or(0.0)).collect();
                m["executionDiagnostics"] = crate::execution_metrics::net_edge_diagnostics(&test, &preds, &crate::execution_metrics::PAPER_EDGE_MARGINS)?;
            }
            m["coverage"] = coverage.clone();
            m["trainRows"] = json!(train.len());
            m["testRows"] = json!(test.len());
            m["rmse"] = json!(rmse);
            m["baselineRmse"] = json!(baseline_rmse);
            m["baseline"] = json!(if target_policy == crate::roundtrip_target::ROUND_TRIP {
                "Decision book unchanged, entry and exit ticks and quantity-rounded fees"
            } else { "Current bid persists, with the same assumed exit tick and fee" });
            m["beatsPersistence"] = json!(rmse < baseline_rmse);
            let max_label = train.iter().map(|r| parse_at(&r["labelAt"])).fold(0.0, f64::max);
            m["maxTrainingLabelAt"] = json!(baal_service_runtime::contract::ms_to_iso(max_label));
            models.push(m);
        }
    }
    let selected: Vec<V> = horizons.iter().flat_map(|h| {
        let mut hs: Vec<V> = models.iter().filter(|m| m["horizon"].as_i64() == Some(*h)).cloned().collect();
        hs.sort_by(|a, b| a["rmse"].as_f64().unwrap_or(0.0).partial_cmp(&b["rmse"].as_f64().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal));
        hs.into_iter().take(3)
    }).collect();
    let mut limitations: Vec<V> = Vec::new();
    if target_policy == crate::roundtrip_target::ROUND_TRIP {
        limitations.push(json!("Labels conditional on admitted entry and completed exit; censored/no-fill decisions excluded and counted, selection bias possible"));
        limitations.push(json!("Flat-wallet counterfactual sizing; live wallet exposure and cash can differ"));
    }
    limitations.extend([json!("Short correlated capture; no profitability established"),
        json!("Chronological development split with training labels purged at split; not untouched future validation"),
        json!("Top bid label is not verified full-size exit liquidity"),
        json!("No guarantee of future depth, latency, settlement equivalence or execution")]);
    Ok(json!({"schemaVersion":1,"trainingHorizonsSeconds":horizons,"snapshotOrderPolicy":snapshot_order_policy,
        "targetPolicy":target_policy,"trainingScope":training_scope,"labelDiagnostics":label_diagnostics,
        "family":family,"historyPolicy":history_policy,"trainingWindowStart":since,
        "requireInstrumentCoverage":require_instrument_coverage,"breadthContext":breadth_context,
        "createdAt":baal_service_runtime::contract::ms_to_iso(baal_service_runtime::timing::LatencyClock::injected().now()),
        "config":c,"configHash":crate::model::digest(c),"sourceHash":source["sourceHash"],
        "sourceRecords":source["sourceRecords"],"trainingCaptureThrough":source["cutoff"],
        "developmentSplitAt":split,"acceptedSnapshots":rows.len(),
        "candidatesAttempted":horizons.len()*32,"modelsTrained":models.len(),"models":models,"rejected":rejected,
        "selected":selected,
        "selection":"Three lowest development-split RMSE per horizon; exploration only, all tried variants retained",
        "target": if target_policy == crate::roundtrip_target::ROUND_TRIP {
            "Conditional net realized round-trip PnL per filled share from delayed entry, post-fill holding/stop and delayed FIFO exit"
        } else { "Future haircut-free best bid minus adverse exit tick and assumed exit fee, relative to current midpoint; entry costs charged separately" },
        "limitations": limitations}))
}
