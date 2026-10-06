// live-paper/edge-outcome-audit.mjs — attribute executable-edge trial labels to
// the public book rows that justify them.
// (The module's argv CLI entry is not ported; no sibling .rs CLI pattern exists.)

use baal_paper::fixed::{big_amount, money, TICK};
use baal_service_runtime::json_text::{digest, js_int, sha256};
use crate::book_publication::{apply_book_publication, create_book_publication_state};
use num_bigint::BigInt;
use serde_json::{json, Value as V};

fn need(ok: bool, reason: &str) -> Result<(), String> {
    if !ok { return Err(format!("EDGE_AUDIT_{reason}")); }
    Ok(())
}
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(x), V::Number(y)) => x.as_f64() == y.as_f64(),
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
fn js_num(v: &V) -> f64 {
    match v {
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::Bool(b) => if *b { 1.0 } else { 0.0 },
        V::Null => 0.0,
        V::String(s) => {
            let t = s.trim();
            if t.is_empty() { 0.0 } else { t.parse::<f64>().unwrap_or(f64::NAN) }
        }
        _ => f64::NAN,
    }
}
fn to_key(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Null => "null".into(),
        V::Bool(b) => b.to_string(),
        V::Number(n) => {
            let f = n.as_f64().unwrap_or(f64::NAN);
            if f.is_finite() && f.fract() == 0.0 { (f as i64).to_string() } else { n.to_string() }
        }
        V::Array(a) => a.iter().map(|x| match x { V::Null => String::new(), other => to_key(other) })
            .collect::<Vec<_>>().join(","),
        V::Object(_) => "[object Object]".into(),
    }
}
fn amount(v: &V) -> Result<BigInt, String> {
    match v {
        V::String(s) => big_amount(s),
        _ => Err("Money must be a canonical decimal string with at most six places".into()),
    }
}
/// JS BigInt(x): numbers must be integral, numeric strings parse.
fn bigint(v: &V) -> Result<BigInt, String> {
    match v {
        V::Number(_) => js_int(v).map(BigInt::from)
            .ok_or("Cannot convert value to a BigInt".into()),
        V::String(s) => s.parse::<i128>().map(BigInt::from)
            .map_err(|_| "Cannot convert string to a BigInt".to_string()),
        V::Bool(b) => Ok(BigInt::from(*b as i64)),
        _ => Err("Cannot convert value to a BigInt".into()),
    }
}
fn instant(v: &V) -> Result<i64, String> {
    baal_paper::fixed::instant(v.as_str().unwrap_or(""))
}

pub fn attribute_edge_outcome(label: &V, entry_row: Option<&V>, exit_row: Option<&V>,
    bank: &V) -> Result<V, String> {
    // entryRow?.payload?.book — a missing book yields JS undefined whose later
    // .marketId access throws TypeError only after the kind gate passes.
    let entry = entry_row.and_then(|r| r.get("payload")).and_then(|p| p.get("book"));
    let exit = exit_row.and_then(|r| r.get("payload")).and_then(|p| p.get("book"));
    let kinds_ok = entry_row.is_some_and(|r| strict_eq(&r["kind"], &json!("book")))
        && exit_row.is_some_and(|r| strict_eq(&r["kind"], &json!("book")));
    if !kinds_ok { return Err("EDGE_AUDIT_SCOPE".into()); }
    let entry = entry.ok_or("TypeError: Cannot read properties of undefined")?;
    let exit = exit.ok_or("TypeError: Cannot read properties of undefined")?;
    need(strict_eq(&entry["marketId"], &label["marketId"])
        && strict_eq(&exit["marketId"], &label["marketId"])
        && strict_eq(&entry["exchangeId"], &exit["exchangeId"])
        && strict_eq(&entry["tournamentId"], &bank["config"]["tournamentId"])
        && strict_eq(&exit["tournamentId"], &bank["config"]["tournamentId"]), "SCOPE")?;
    need(strict_eq(&entry["versionKey"], &label["entryVersion"])
        && strict_eq(&exit["versionKey"], &label["exitVersion"])
        && strict_eq(&entry["receivedAt"], &label["filledAt"])
        && strict_eq(&exit["receivedAt"], &label["at"])
        && strict_eq(&exit_row.unwrap()["sequence"], &label["sourceSequence"])
        && strict_eq(&exit_row.unwrap()["hash"], &label["sourceHash"]), "LABEL_SOURCE")?;
    need(strict_eq(&bank["config"]["feeBps"], &json!(0))
        && strict_eq(&bank["config"]["slippageTicks"], &json!(1))
        && strict_eq(&bank["config"]["depthFractionBps"], &json!(100))
        && strict_eq(&bank["config"]["orderShares"], &json!(1)), "COST_POLICY")?;
    need(instant(&entry["requestStartedAt"])? as f64
        >= instant(&label["preparedAt"])? as f64 + js_num(&bank["config"]["latencyMs"])
        && instant(&exit["requestStartedAt"])? as f64
            >= instant(&label["filledAt"])? as f64 + js_num(&bank["policy"]["horizonMs"])
        && instant(&label["at"])? as f64
            <= instant(&label["filledAt"])? as f64 + js_num(&bank["policy"]["horizonMs"])
                + js_num(&bank["policy"]["labelLatenessMs"])
        && !strict_eq(&entry["versionKey"], &exit["versionKey"]), "CAUSAL_WINDOW")?;
    let entry_bid = amount(&entry["bids"][0]["price"])?;
    let entry_ask = amount(&entry["asks"][0]["price"])?;
    let exit_bid = amount(&exit["bids"][0]["price"])?;
    need(entry_ask > entry_bid
        && [&entry["bids"][0], &entry["asks"][0], &exit["bids"][0], &exit["asks"][0]]
            .iter().all(|x| js_int(&x["quantity"]).is_some_and(|q| q >= 100)), "DEPTH")?;
    need(amount(&label["entry"])? == &entry_ask + BigInt::from(TICK)
        && amount(&label["exitBid"])? == &exit_bid - BigInt::from(TICK)
        && js_int(&label["netMicros"]).is_some(), "PRICE")?;
    let bid_move = &exit_bid - &entry_bid;
    let spread = &entry_ask - &entry_bid;
    let raw_crossed = &exit_bid - &entry_ask;
    let modeled = &raw_crossed - BigInt::from(2 * TICK);
    need(modeled == bigint(&label["netMicros"])?, "ACCOUNTING")?;
    Ok(json!({
        "marketId": label["marketId"], "cell": label["cell"],
        "preparedAt": label["preparedAt"], "filledAt": label["filledAt"], "exitAt": label["at"],
        "entrySequence": entry_row.unwrap()["sequence"], "exitSequence": exit_row.unwrap()["sequence"],
        "entryHash": entry_row.unwrap()["hash"], "exitHash": exit_row.unwrap()["hash"],
        "bidMovement": money(&bid_move), "entrySpread": money(&spread),
        "rawCrossedReturn": money(&raw_crossed), "modeledAdverseTicks": money(&BigInt::from(2 * TICK)),
        "modeledNet": money(&modeled), "explicitFees": "0.000000",
        "prospective": instant(&label["preparedAt"])? > instant(&bank["cutoff"])?,
    }))
}

fn audit_summary(rows: &[V]) -> Result<V, String> {
    let values = |key: &str| -> Result<Vec<BigInt>, String> {
        let mut v: Vec<BigInt> = Vec::new();
        for x in rows { v.push(amount(&x[key])?); }
        v.sort();
        Ok(v)
    };
    let median = |key: &str| -> Result<V, String> {
        let v = values(key)?;
        Ok(if v.is_empty() { V::Null } else {
            json!(money(&v[(v.len() - 1) / 2]))
        })
    };
    let mut markets: Vec<V> = Vec::new();
    for x in rows {
        if !markets.iter().any(|m| strict_eq(m, &x["marketId"])) { markets.push(x["marketId"].clone()); }
    }
    let positive = |key: &str| -> Result<usize, String> {
        let mut n = 0;
        for x in rows { if amount(&x[key])? > BigInt::from(0) { n += 1; } }
        Ok(n)
    };
    Ok(json!({
        "labels": rows.len(),
        "markets": markets.len(),
        "positiveBidMovements": positive("bidMovement")?,
        "positiveRawCrossedReturns": positive("rawCrossedReturn")?,
        "positiveModeledReturns": positive("modeledNet")?,
        "medianBidMovement": median("bidMovement")?,
        "medianEntrySpread": median("entrySpread")?,
        "medianRawCrossedReturn": median("rawCrossedReturn")?,
        "medianModeledNet": median("modeledNet")?,
    }))
}

pub fn audit_recent_edge_outcomes(bank: &V, stats: &V, source_bytes: &[u8]) -> Result<V, String> {
    need(digest(bank) == stats["bankHash"].as_str().unwrap_or("")
        && stats["mode"].as_str() == Some("execution_aware_online_paper")
        && stats["recentLabels"].is_array(), "BANK")?;
    let complete = source_bytes.iter().rposition(|b| *b == b'\n').map(|i| i + 1).unwrap_or(0);
    need(complete > 0, "EMPTY_SOURCE")?;
    let bytes = &source_bytes[..complete];
    let text = String::from_utf8_lossy(bytes).into_owned();
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.pop();
    let mut rows: Vec<V> = Vec::new();
    for line in lines {
        rows.push(serde_json::from_str(line).map_err(|e| e.to_string())?);
    }
    let mut publication = create_book_publication_state(&bank["universe"])?;
    let mut entries: std::collections::HashMap<String, V> = Default::default();
    let mut by_sequence: std::collections::HashMap<String, V> = Default::default();
    for row in &rows {
        publication = apply_book_publication(&publication, row)?;
        by_sequence.insert(to_key(&row["sequence"]), row.clone());
        if row["kind"].as_str() == Some("book") {
            let b = &row["payload"]["book"];
            entries.insert(format!("{}|{}|{}", to_key(&b["marketId"]),
                to_key(&b["versionKey"]), to_key(&b["receivedAt"])), row.clone());
        }
    }
    need(js_num(&publication["sequence"]) >= js_num(&stats["sourceSequence"]), "SOURCE_BEHIND_STATS")?;
    let mut seen: Vec<String> = Vec::new();
    let mut outcomes: Vec<V> = Vec::new();
    for label in stats["recentLabels"].as_array().cloned().unwrap_or_default() {
        let key = format!("{}|{}|{}|{}", to_key(&label["marketId"]),
            to_key(&label["entryVersion"]), to_key(&label["filledAt"]), to_key(&label["exitVersion"]));
        need(!seen.contains(&key), "DUPLICATE_LABEL")?;
        seen.push(key);
        let entry = entries.get(&format!("{}|{}|{}", to_key(&label["marketId"]),
            to_key(&label["entryVersion"]), to_key(&label["filledAt"])));
        let exit = by_sequence.get(&to_key(&label["sourceSequence"]));
        outcomes.push(attribute_edge_outcome(&label, entry, exit, bank)?);
    }
    let development: Vec<V> = outcomes.iter().filter(|x| x["prospective"].as_bool() != Some(true)).cloned().collect();
    let prospective: Vec<V> = outcomes.iter().filter(|x| x["prospective"].as_bool() == Some(true)).cloned().collect();
    Ok(json!({
        "schemaVersion": 1, "mode": "recent_executable_label_attribution",
        "at": stats["updatedAt"], "cutoff": bank["cutoff"], "bankHash": stats["bankHash"],
        "statsHash": digest(stats), "sourceBytes": complete,
        "incompleteTailBytes": source_bytes.len() - complete,
        "sourceBytesHash": sha256(bytes), "sourceSequence": publication["sequence"],
        "sourceHash": publication["hash"],
        "availableRecentLabels": outcomes.len(),
        "totalCompletedLabels": stats["counts"]["labels_completed"],
        "development": audit_summary(&development)?,
        "prospective": audit_summary(&prospective)?,
        "outcomes": outcomes,
        "networkRequests": 0, "ordersSubmitted": 0, "profitabilityEstablished": false,
        "limitations": [
            "Only the recent label window retained in current stats is attributed; this is not an all-history summary.",
            "Labels are hypothetical one-share probes, not wallet fills, independent samples, or portfolio P&L.",
            "Raw crossed return still pays the observed entry spread; removing assumed ticks does not imply attainable fills.",
            "Prospective labels may overlap, share event exposure, and be conditioned on fill and data availability.",
        ],
    }))
}
