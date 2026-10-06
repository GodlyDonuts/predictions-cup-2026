// live-paper/shock-shadow.mjs — frozen causal shock-rebound shadow cohort.
use baal_paper::fixed::{big_amount, money, TICK};
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_truthy, sha256};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::ml_shadow::{verify_source, VerifiedSource};

fn iso_now() -> String {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0);
    ms_to_iso(ms)
}
fn clock_at(e: &V, r: &V) -> String {
    e["clockAt"].as_str().filter(|s| !s.is_empty()).or_else(|| r["at"].as_str().filter(|s| !s.is_empty())).unwrap_or("").to_string()
}
fn orderbook_market(endpoint: &str) -> Option<String> {
    let rest = endpoint.strip_prefix("/markets/")?;
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if !digits.is_empty() && rest[digits.len()..].starts_with("/orderbook?") { Some(digits) } else { None }
}
fn js_bigint(v: &V) -> Result<BigInt, String> {
    match v {
        V::Number(n) => {
            if let Some(x) = n.as_i64() { return Ok(BigInt::from(x)); }
            if let Some(x) = n.as_u64() { return Ok(BigInt::from(x)); }
            let f = n.as_f64().ok_or("Cannot convert value to a BigInt")?;
            if f.fract() == 0.0 && f.is_finite() { Ok(BigInt::from(f as i64)) } else { Err("Cannot convert non-integer to a BigInt".into()) }
        }
        V::String(s) => s.parse::<i64>().map(BigInt::from).map_err(|_| "Cannot convert string to a BigInt".to_string()),
        _ => Err("Cannot convert value to a BigInt".into()),
    }
}
/// JS Number(x) coercion used by numeric comparisons (>, <=) and index math.
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
/// JS ToPropertyKey/ToString for object keys: numbers lose a trailing ".0",
/// arrays join with ",", objects become "[object Object]".
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
/// JS String(x) on scalar JSON values (used for id coercion).
fn js_str(v: &V) -> String {
    match v { V::String(s) => s.clone(), other => to_key(other) }
}
/// Emit a JS number: integral values serialize without a decimal point.
fn js_number(x: f64) -> V {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 9.007_199_254_740_992e15 { json!(x as i64) } else { json!(x) }
}
/// Math.min(a,b) with JS NaN propagation (Rust f64::min ignores NaN).
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

/// shockCodeIdentity() — sha256 of each runtime source file under workspace_dir
/// (the repo root that contains live-paper/, paper/, feed/).
pub fn shock_code_identity(workspace_dir: &Path) -> Result<V, String> {
    let mut m = serde_json::Map::new();
    for f in ["live-paper/shock-shadow.mjs", "live-paper/shock-rebound.mjs", "live-paper/model.mjs",
        "live-paper/ml-shadow.mjs", "live-paper/runtime.mjs", "live-paper/network-budget.mjs",
        "paper/fixed.mjs", "feed/contract.mjs"] {
        m.insert(f.to_string(), json!(sha256(&fs::read(workspace_dir.join(f)).map_err(|e| e.to_string())?)));
    }
    Ok(V::Object(m))
}

fn check_bank(bank: &V) -> Result<(), String> {
    crate::model::validate_config(&bank["config"])?;
    if bank["name"].as_str() != Some("shock-rebound-shadow-v1")
        || bank["cohortId"].as_str() != Some(format!("shock-rebound-{}", js_str(&bank["cutoff"])).as_str())
        || bank["config"]["feeBps"].as_f64() != Some(0.0)
        || bank["config"]["marketIds"].as_array().map(|a| a.len() > 4).unwrap_or(false)
        || bank["policy"]["maxOpenMarkets"].as_f64() != Some(3.0)
        || bank["policy"]["cumulativeBuyBudget"].as_str() != Some("100.000000")
        || big_amount(bank["config"]["maxExposure"].as_str().unwrap_or(""))? > big_amount("100.000000")?
        || bank["config"]["slippageTicks"].as_f64() != Some(1.0)
        || bank["config"]["depthFractionBps"].as_f64().unwrap_or(f64::NAN) > 100.0
        || bank["config"]["orderShares"].as_f64().unwrap_or(f64::NAN) > 10.0 {
        return Err("Invalid frozen shock cohort".into());
    }
    Ok(())
}

/// createShock(raw, config, universe, codeIdentity, cutoffAt) -> {bank, state}
pub fn create_shock(raw: &str, config: &V, universe: &V, code_identity: &V, cutoff_at: Option<&str>) -> Result<V, String> {
    let Some(records) = verify_source(raw)? else { return Err("Complete source required".into()); };
    if records.records.is_empty() { return Err("Complete source required".into()); }
    let c = config.clone();
    crate::model::validate_config(&c)?;
    let last = records.records.last().unwrap();
    let source_at = clock_at(&last["event"], last);
    let cutoff = cutoff_at.filter(|x| !x.is_empty()).unwrap_or(&source_at).to_string();
    let source_hash = records.prefix_hashes.last().cloned().unwrap_or_default();
    if parse_iso_ms(&cutoff).is_none()
        || parse_iso_ms(&cutoff).unwrap() < parse_iso_ms(&source_at).unwrap_or(f64::NAN) {
        return Err("Cutoff precedes frozen source".into());
    }
    let mut markets = Vec::new();
    for market_id in c["marketIds"].as_array().cloned().unwrap_or_default() {
        let mid = market_id.as_str().unwrap_or("");
        let matches: Vec<V> = universe["exchanges"].as_array().cloned().unwrap_or_default().into_iter()
            .filter(|x| x["marketId"].as_str() == Some(mid) && x["option"].as_str() == Some("YES")).collect();
        if matches.len() != 1 || universe["tournamentId"] != c["tournamentId"] {
            return Err("Shock universe scope mismatch".into());
        }
        markets.push(crate::shock_rebound::create_shock_rebound(
            &js_str(&matches[0]["marketId"]), &js_str(&matches[0]["exchangeId"]),
            &source_hash, &cutoff, &json!({"maxDataAgeMs": c["maxDataAgeMs"]}))?);
    }
    let bank = json!({"schemaVersion": 1, "name": "shock-rebound-shadow-v1", "cohortId": format!("shock-rebound-{}", cutoff),
        "config": c, "cutoff": cutoff, "sourceHash": source_hash, "sourceRecords": records.records.len(),
        "codeIdentity": code_identity.clone(),
        "markets": markets.iter().map(|x| x["bank"].clone()).collect::<Vec<_>>(),
        "policy": {"maxOpenMarkets": 3, "cumulativeBuyBudget": "100.000000", "entryLifetimeMs": 60000,
            "takeProfit": "0.010000", "maximumHoldMs": 600000, "maxMarketCost": "25.000000"}});
    check_bank(&bank)?;
    let mut candidates = serde_json::Map::new();
    for x in &markets {
        candidates.insert(x["bank"]["marketId"].as_str().unwrap_or("").to_string(), x["state"].clone());
    }
    let state = json!({"schemaVersion": 1, "bankHash": crate::model::digest(&bank), "sourceSequence": 0,
        "lastRecordHash": V::Null, "candidates": candidates, "proposals": {}, "signals": {}, "diagnosticCounts": {},
        "wallet": crate::model::initial_state(&bank["config"], bank["cohortId"].as_str().unwrap_or(""), &cutoff)?});
    Ok(json!({"bank": bank.clone(), "state": advance_shock(&state, &records, &bank)?}))
}

fn revoke(s: &mut V, bank: &V, market_id: Option<&str>, at: &str, reason: &V) -> Result<(), String> {
    // JS invalidate(s.wallet, marketId, at, reason) stores the RAW reason value
    // (model::invalidate's &str signature cannot express non-string reasons).
    {
        let w = &mut s["wallet"];
        if w.get("invalidated").is_none() { w["invalidated"] = json!({}); }
        let ids: Vec<String> = w["books"].as_object().map(|o| o.iter()
            .filter(|(_, b)| market_id.is_none() || b["marketId"].as_str() == market_id)
            .map(|(k, _)| k.clone()).collect()).unwrap_or_default();
        for id in ids { w["invalidated"][&id] = json!({"at": at, "reason": reason}); }
    }
    for m in bank["markets"].as_array().cloned().unwrap_or_default() {
        let mid = m["marketId"].as_str().unwrap_or("").to_string();
        if !(market_id.is_none() || mid == market_id.unwrap_or("")) { continue; }
        // JS: latest = s.candidates[m.marketId].last?.at || 0 — raw value kept,
        // Math.max does Number() coercion (NaN for non-numeric) and propagates NaN.
        let lv = &s["candidates"][&mid]["last"]["at"];
        let latest = if js_truthy(Some(lv)) { js_num(lv) } else { 0.0 };
        let at_ms = parse_iso_ms(at).unwrap_or(f64::NAN);
        let max_ms = if at_ms.is_nan() || latest.is_nan() { f64::NAN } else { at_ms.max(latest) };
        if !max_ms.is_finite() { return Err("Invalid time value".into()); }
        s["candidates"][&mid] = crate::shock_rebound::invalidate_shock_rebound(&s["candidates"][&mid], &m, &ms_to_iso(max_ms))?;
        if let Some(p) = s["proposals"].as_object_mut() { p.remove(&mid); }
        let id = format!("{}:YES", js_str(&m["exchangeId"]));
        {
            let st = &mut s["wallet"]["strategies"]["mean_reversion"];
            if st["pending"][&id]["action"].as_str() == Some("buy") {
                if let Some(p) = st["pending"].as_object_mut() { p.remove(&id); }
                let n = st["expiredIntents"].as_i64().unwrap_or(0);
                st["expiredIntents"] = json!(n + 1);
            }
        }
        s["signals"][&mid] = json!({"reason": reason, "at": at});
        let key = to_key(reason);
        let n = s["diagnosticCounts"][&key].as_i64().unwrap_or(0);
        s["diagnosticCounts"][&key] = json!(n + 1);
    }
    Ok(())
}

/// advanceShock(input, records, bank) — records is a verify_source() output.
pub fn advance_shock(input: &V, source: &VerifiedSource, bank: &V) -> Result<V, String> {
    check_bank(bank)?;
    let seq_f = js_num(&input["sourceSequence"]);
    let bank_n = js_num(&bank["sourceRecords"]);
    let prefix_bad = match (if bank_n >= 1.0 { source.prefix_hashes.get((bank_n - 1.0) as usize) } else { None },
                            bank.get("sourceHash")) {
        (Some(a), Some(b)) => b.as_str() != Some(a.as_str()),
        (None, None) => false,
        _ => true,
    };
    // JS: input.sourceSequence && records[input.sourceSequence-1]?.hash !== input.lastRecordHash
    // — the truthy short-circuit stays; non-integer indices read as undefined.
    let prior_bad = if !js_truthy(input.get("sourceSequence")) { false } else {
        let idx = seq_f - 1.0;
        let rec = if idx >= 0.0 && idx.fract() == 0.0 && idx <= usize::MAX as f64 {
            source.records.get(idx as usize)
        } else { None };
        match (rec, input.get("lastRecordHash")) {
            (Some(r), Some(lh)) => r.get("hash") != Some(lh),
            (Some(_), None) => true,
            (None, None) => false,
            (None, Some(_)) => true,
        }
    };
    if input["bankHash"].as_str() != Some(crate::model::digest(bank).as_str())
        || prefix_bad || seq_f > source.records.len() as f64 || prior_bad {
        return Err("Shock source or bank changed".into());
    }
    let mut s = input.clone();
    let c = &bank["config"];
    let p = &bank["policy"];
    crate::model::assert_state(&s["wallet"], c)?;
    for r in &source.records {
        if js_num(&r["sequence"]) <= js_num(&s["sourceSequence"]) { continue; }
        let e = &r["event"];
        let at = clock_at(e, r);
        let prospective = js_num(&r["sequence"]) > js_num(&bank["sourceRecords"]);
        s["sourceSequence"] = r["sequence"].clone();
        s["lastRecordHash"] = r["hash"].clone();
        if e["type"].as_str() == Some("account_read_outage") {
            revoke(&mut s, bank, None, &at, &json!("account_outage"))?;
            continue;
        }
        if e["type"].as_str() != Some("cycle") { continue; }
        let mut snaps: Vec<V> = e["sourceSnapshots"].as_array().cloned().unwrap_or_default();
        snaps.extend(e["supplementalDepth"].as_array().cloned().unwrap_or_default());
        snaps.sort_by(|a, b| {
            let pick = |x: &V| -> String {
                x["receivedAt"].as_str().filter(|v| !v.is_empty())
                    .or_else(|| x["at"].as_str().filter(|v| !v.is_empty()))
                    .unwrap_or(&at).to_string()
            };
            let (ka, kb) = (parse_iso_ms(&pick(a)).unwrap_or(f64::NAN), parse_iso_ms(&pick(b)).unwrap_or(f64::NAN));
            ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
        });
        for snap in snaps {
            // JS: marketId stays the raw JSON value; bank.markets lookup is strict ===,
            // so a non-string marketId can never match (markets store string ids).
            let market_id_v = if js_truthy(snap.get("marketId")) {
                snap["marketId"].clone()
            } else {
                orderbook_market(snap["endpoint"].as_str().unwrap_or("")).map(V::from).unwrap_or(V::Null)
            };
            let Some(m) = bank["markets"].as_array().and_then(|a| a.iter().find(|x| x["marketId"] == market_id_v).cloned()) else { continue; };
            let market_id = m["marketId"].as_str().unwrap_or("").to_string();
            if js_truthy(snap.get("skipped")) {
                let snap_at = snap["at"].as_str().filter(|x| !x.is_empty()).unwrap_or(&at).to_string();
                revoke(&mut s, bank, Some(&market_id), &snap_at, &snap["skipped"])?;
                continue;
            }
            let parsed = (|| -> Result<(V, V), String> {
                if !e["accountExclusions"].is_array() { return Err("Missing reconciled exclusions".into()); }
                let excluded: BTreeSet<String> = e["accountExclusions"].as_array().unwrap().iter()
                    .filter_map(|x| x.as_str().map(String::from)).collect();
                let b = crate::model::normalize_book(&snap["data"], &market_id, c["tournamentId"].as_str().unwrap_or(""),
                    snap["receivedAt"].as_str().unwrap_or(""), snap["requestStartedAt"].as_str().unwrap_or(""),
                    c["maxDataAgeMs"].as_i64().unwrap_or(0), &excluded, "authoritative_receipt_v1")?;
                if parse_iso_ms(b["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
                    > parse_iso_ms(&at).unwrap_or(f64::NAN) {
                    return Err("Future book".into());
                }
                let out = crate::shock_rebound::advance_shock_rebound(&s["candidates"][&market_id], &b, &m)?;
                Ok((b, out))
            })();
            let (b, out) = match parsed {
                Ok(x) => x,
                Err(_) => { revoke(&mut s, bank, Some(&market_id), &at, &json!("invalid_or_excluded_book"))?; continue; }
            };
            s["candidates"][&market_id] = out["state"].clone();
            s["signals"][&market_id] = out["diagnostic"].clone();
            {
                let dr = to_key(&out["diagnostic"]["reason"]);
                let n = s["diagnosticCounts"][&dr].as_i64().unwrap_or(0);
                s["diagnosticCounts"][&dr] = json!(n + 1);
            }
            let dr = out["diagnostic"]["reason"].as_str().unwrap_or("");
            if !prospective
                || parse_iso_ms(b["requestStartedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
                    <= parse_iso_ms(bank["cutoff"].as_str().unwrap_or("")).unwrap_or(f64::NAN) { continue; }
            if dr == "request_before_invalidation" { continue; }
            let independent = dr != "no_independent_quote";
            if !independent {
                if let Some(pr) = s["proposals"].as_object_mut() { pr.remove(&market_id); }
            }
            if js_truthy(out.get("signal")) { s["proposals"][&market_id] = out["signal"].clone(); }
            let proposal = s["proposals"][&market_id].clone();
            let id = b["instrument"].as_str().unwrap_or("").to_string();
            // Confirmation does not license buying a later collapse. Preserve the
            // original shock anchor and apply its cap through the delayed fill.
            let total_drop_twice: BigInt = if proposal.is_null() { BigInt::from(0) } else {
                BigInt::from(2) * big_amount(proposal["featuresAtShock"]["anchorMid"].as_str().unwrap_or(""))?
                    - big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?
                    - big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))?
            };
            if js_truthy(Some(&proposal)) {
                let risk = big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?
                        < big_amount(proposal["troughBid"].as_str().unwrap_or(""))?
                    || total_drop_twice > BigInt::from(2) * big_amount(m["config"]["maximumShock"].as_str().unwrap_or(""))?
                    || ["maximum_shock_breached", "observation_gap", "insufficient_history", "trend_veto",
                        "missing_shock_anchor", "insufficient_trend_history"].contains(&dr);
                if risk {
                    revoke(&mut s, bank, Some(&market_id), b["receivedAt"].as_str().unwrap_or(""), &json!("proposal_risk_revoked"))?;
                    continue;
                }
            }
            let mut used = BigInt::from(0);
            for f in s["wallet"]["strategies"]["mean_reversion"]["fills"].as_array().cloned().unwrap_or_default() {
                if f["action"].as_str() == Some("buy") {
                    used += big_amount(f["price"].as_str().unwrap_or(""))? * js_bigint(&f["quantity"])?;
                }
            }
            let remaining = big_amount(p["cumulativeBuyBudget"].as_str().unwrap_or(""))? - &used;
            let occupied: BTreeSet<String> = s["wallet"]["strategies"]["mean_reversion"]["positions"].as_object()
                .map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default().into_iter()
                .chain(s["wallet"]["strategies"]["mean_reversion"]["pending"].as_object()
                    .map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default())
                .collect();
            let ask = big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))? + BigInt::from(TICK);
            let bid = big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))? - BigInt::from(TICK);
            // JS: proposal ? Math.min(proposal.quantity, Math.max(0, Number(remaining/per))) : 0
            let quantity_cap: f64 = if proposal.is_null() { 0.0 } else {
                let per = big_amount(proposal["maximumEntryPrice"].as_str().unwrap_or(""))?;
                if per == BigInt::from(0) { return Err("Division by zero".into()); }
                let q = (remaining.clone() / per).to_string().parse::<f64>().unwrap_or(f64::NAN);
                js_min(proposal["quantity"].as_f64().unwrap_or(f64::NAN), q.max(0.0))
            };
            let eligible = js_truthy(Some(&proposal))
                && (parse_iso_ms(b["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
                    - parse_iso_ms(proposal["at"].as_str().unwrap_or("")).unwrap_or(f64::NAN))
                    <= p["entryLifetimeMs"].as_f64().unwrap_or(f64::NAN)
                && ask <= big_amount(proposal["maximumEntryPrice"].as_str().unwrap_or(""))?
                && &ask - &bid <= big_amount(m["config"]["maximumEffectiveSpread"].as_str().unwrap_or(""))?
                && big_amount(proposal["recoveryReferenceBid"].as_str().unwrap_or(""))? - BigInt::from(TICK) - &ask
                    >= big_amount(m["config"]["minimumRoom"].as_str().unwrap_or(""))?
                && (occupied.contains(&id)
                    || (occupied.len() as f64) < p["maxOpenMarkets"].as_f64().unwrap_or(f64::NAN))
                && quantity_cap > 0.0;
            {
                let st = &mut s["wallet"]["strategies"]["mean_reversion"];
                if st["pending"][&id]["action"].as_str() == Some("buy") && js_truthy(Some(&proposal))
                    && quantity_cap > 0.0 {
                    let cur = st["pending"][&id]["quantity"].as_f64().unwrap_or(f64::NAN);
                    st["pending"][&id]["quantity"] = js_number(js_min(cur, quantity_cap));
                }
            }
            // A fresh receipt may age an owned position into a timeout even if its
            // engine quote did not change. Evaluate that exit while withholding all
            // same-version liquidity from the hypothetical fill engine.
            let prior_consumed = s["wallet"]["strategies"]["mean_reversion"]["consumedVersions"].get(&id).cloned();
            if !independent {
                s["wallet"]["strategies"]["mean_reversion"]["consumedVersions"][&id] = b["versionKey"].clone();
            }
            let reference: Option<f64> = if proposal.is_null() { None } else {
                Some((big_amount(proposal["recoveryReferenceBid"].as_str().unwrap_or(""))? - BigInt::from(TICK))
                    .to_string().parse::<f64>().unwrap_or(0.0) / 1e6)
            };
            let minimum_edge = big_amount(m["config"]["minimumRoom"].as_str().unwrap_or(""))?
                .to_string().parse::<f64>().unwrap_or(0.0) / 1e6;
            s["wallet"] = crate::model::step_dip(&s["wallet"], &b, c, reference, eligible, minimum_edge,
                &p["takeProfit"], p["maximumHoldMs"].as_i64().unwrap_or(0), &p["maxMarketCost"], "responsive_v2")?;
            if !independent {
                if let Some(cv) = s["wallet"]["strategies"]["mean_reversion"]["consumedVersions"].as_object_mut() {
                    match &prior_consumed {
                        None => { cv.remove(&id); }
                        Some(v) => { cv.insert(id.clone(), v.clone()); }
                    }
                }
            }
            {
                let st = &mut s["wallet"]["strategies"]["mean_reversion"];
                if st["pending"][&id]["action"].as_str() == Some("buy") && js_truthy(Some(&proposal)) {
                    let cur = st["pending"][&id]["quantity"].as_f64().unwrap_or(f64::NAN);
                    st["pending"][&id]["quantity"] = js_number(js_min(cur, quantity_cap));
                    st["pending"][&id]["reason"] = json!("causal_shock_rebound");
                    st["pending"][&id]["signalId"] = proposal["signalId"].clone();
                }
            }
            if !s["wallet"]["strategies"]["mean_reversion"]["positions"][&id].is_null() || !eligible {
                if let Some(pr) = s["proposals"].as_object_mut() { pr.remove(&market_id); }
            }
        }
        if prospective { crate::model::sample_equity(&mut s["wallet"], c, &at); }
    }
    crate::model::assert_state(&s["wallet"], c)?;
    Ok(s)
}

/// shockReport(s, bank, at)
pub fn shock_report(s: &V, bank: &V, at: &str) -> Result<V, String> {
    let r = crate::model::report(&s["wallet"], &bank["config"], at);
    let st = s["wallet"]["strategies"]["mean_reversion"].clone();
    let label = |id: &str| -> String {
        let base = match s["wallet"]["books"][id]["marketId"].as_str() {
            Some("377") => "Alaska Senate".to_string(),
            Some("354") => "Florida Senate".to_string(),
            Some("277") => "North Carolina Senate".to_string(),
            Some("305") => "Michigan Governor".to_string(),
            other => format!("Market {}", other.unwrap_or("undefined")),
        };
        format!("{} Democratic YES", base)
    };
    let mut strategy = r["strategies"]["mean_reversion"].clone();
    let pending: Vec<V> = st["pending"].as_object().cloned().unwrap_or_default().into_iter()
        .map(|(instrument, p)| {
            let mut o = p.as_object().cloned().unwrap_or_default();
            o.insert("instrument".into(), json!(instrument));
            o.insert("label".into(), json!(label(&instrument)));
            V::Object(o)
        }).collect();
    strategy["pending"] = json!(pending);
    if let Some(pos) = strategy["positions"].as_array_mut() {
        for p in pos.iter_mut() {
            let inst = p["instrument"].as_str().unwrap_or("").to_string();
            p["label"] = json!(label(&inst));
        }
    }
    if let Some(fl) = strategy["recentFills"].as_array_mut() {
        for f in fl.iter_mut() {
            let inst = f["instrument"].as_str().unwrap_or("").to_string();
            f["label"] = json!(label(&inst));
        }
    }
    let mut used = BigInt::from(0);
    for f in st["fills"].as_array().cloned().unwrap_or_default() {
        if f["action"].as_str() == Some("buy") {
            used += big_amount(f["price"].as_str().unwrap_or(""))? * js_bigint(&f["quantity"])?;
        }
    }
    let mut assumptions = r["assumptions"].as_object().cloned().unwrap_or_default();
    assumptions.insert("feeBps".into(), json!(0));
    assumptions.insert("feeStatus".into(), json!("zero_fee_scenario"));
    Ok(json!({"mode": "prospective_shock_rebound_paper", "cohortId": bank["cohortId"], "updatedAt": at,
        "bankHash": s["bankHash"], "cutoff": bank["cutoff"], "sourceSequence": s["sourceSequence"],
        "feeBps": 0, "universeSize": bank["markets"].as_array().map(|a| a.len()).unwrap_or(0),
        "historiesAvailable": s["candidates"].as_object().map(|o| o.values()
            .filter(|v| v["history"].as_array().map(|h| !h.is_empty()).unwrap_or(false)).count()).unwrap_or(0),
        "sourceHash": s["lastRecordHash"], "frozenSourceHash": bank["sourceHash"],
        "codeIdentity": bank["codeIdentity"], "marketIds": bank["config"]["marketIds"],
        "observations": r["observations"], "signals": s["signals"], "diagnosticCounts": s["diagnosticCounts"],
        "historyCounts": s["candidates"].as_object().map(|o| o.iter()
            .map(|(id, v)| (id.clone(), json!(v["history"].as_array().map(|h| h.len()).unwrap_or(0))))
            .collect::<serde_json::Map<String, V>>()).map(V::Object).unwrap_or(V::Null),
        "strategy": strategy, "cumulativeBuyCost": money(&used),
        "policy": bank["policy"], "assumptions": assumptions,
        "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false}))
}

/// runShock({dir, initialize, once, workspaceDir, now}) — shared by v1
/// (verifySource) and v2 (readIncrementalSource + sourceReadMetrics).
/// `read_source` supplies the verified source each poll; `metrics` supplies the
/// optional sourceReadMetrics payload appended to published stats (v2 only).
pub fn run_shock_with_source<F, M>(dir: &Path, initialize: bool, once: bool, workspace_dir: &Path,
    now: &dyn Fn() -> i64, mut read_source: F, metrics: M) -> Result<V, String>
where F: FnMut() -> Result<Option<VerifiedSource>, String>, M: Fn() -> Option<V> {
    let stopped = || ["STOP", "SHOCK_STOP"].iter().any(|f| dir.join(f).exists()) || workspace_dir.join("STOP").exists();
    if stopped() { return Err("STOP exists".into()); }
    let _release = crate::paper_runtime::lock(&dir.join("shock-shadow.lock"))?;
    let bank_file = dir.join("shock-bank.json");
    let journal_file = dir.join("shock-journal.jsonl");
    let mut journal = crate::paper_runtime::Journal::open(&journal_file)?;
    let bank: V;
    let mut state: V;
    if initialize {
        if bank_file.exists() || journal.sequence > 0 { return Err("Frozen shock cohort already exists".into()); }
        let base: V = serde_json::from_str(&fs::read_to_string(dir.join("dip-responsive-bank.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let mut c = base["config"].as_object().cloned().unwrap_or_default();
        c.insert("marketIds".into(), json!(["377", "354", "277", "305"]));
        c.insert("feeBps".into(), json!(0));
        c.insert("orderShares".into(), json!(10));
        c.insert("maxPositionShares".into(), json!(50));
        c.insert("maxExposure".into(), json!("100.000000"));
        c.insert("cashReserve".into(), json!(money(&(big_amount(base["config"]["initialCash"].as_str().unwrap_or(""))?
            - big_amount("100.000000")?))));
        c.insert("exitAfterMs".into(), json!(600000));
        let c = V::Object(c);
        let raw = fs::read_to_string(dir.join("journal.jsonl")).map_err(|e| e.to_string())?;
        let created = create_shock(&raw, &c, &base["universe"], &shock_code_identity(workspace_dir)?,
            Some(&ms_to_iso(now() as f64)))?;
        bank = created["bank"].clone();
        state = created["state"].clone();
        let pretty = serde_json::to_string_pretty(&bank).map_err(|e| e.to_string())?;
        let mut o = fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
        o.open(&bank_file).and_then(|mut f| f.write_all(pretty.as_bytes())).map_err(|e| e.to_string())?;
        journal.commit(&state, &json!({"type": "cohort_frozen", "sourceSequence": state["sourceSequence"],
            "sourceHash": bank["sourceHash"], "cutoff": bank["cutoff"]}))?;
    } else {
        bank = serde_json::from_str(&fs::read_to_string(&bank_file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        state = journal.last.as_ref().and_then(|l| l.get("state").cloned()).unwrap_or(V::Null);
        if !js_truthy(Some(&state)) { return Err("Shock journal missing; explicit recovery required".into()); }
    }
    if crate::model::digest(&bank["codeIdentity"]) != crate::model::digest(&shock_code_identity(workspace_dir)?) {
        return Err("Frozen shock code changed; new cohort required".into());
    }
    let mut partials = 0;
    let publish = |state: &V, bank: &V, status: &str| -> Result<(), String> {
        baal_service_runtime::atomic::atomic_json(&dir.join("shock-state.json"), state)?;
        let mut stats = shock_report(state, bank, &iso_now())?;
        if let Some(o) = stats.as_object_mut() {
            o.insert("status".into(), json!(status));
            if let Some(m) = metrics() { o.insert("sourceReadMetrics".into(), m); }
        }
        baal_service_runtime::atomic::atomic_json(&dir.join("shock-stats.json"), &stats)
    };
    let inner = (|| -> Result<(), String> {
        loop {
            if stopped() { break; }
            match read_source()? {
                None => {
                    partials += 1;
                    if partials > 3 { return Err("Incomplete source".into()); }
                    publish(&state, &bank, "awaiting_capture")?;
                }
                Some(records) => {
                    partials = 0;
                    let next = advance_shock(&state, &records, &bank)?;
                    if next["sourceSequence"] != state["sourceSequence"] {
                        journal.commit(&next, &json!({"type": "source_advanced",
                            "sourceSequence": next["sourceSequence"], "sourceHash": next["lastRecordHash"]}))?;
                    }
                    state = next;
                    publish(&state, &bank, "running")?;
                }
            }
            if once { break; }
            let mut i = 0;
            while i < 75 && !stopped() { std::thread::sleep(Duration::from_millis(200)); i += 1; }
            if stopped() { break; }
        }
        if stopped() { publish(&state, &bank, "stopped")?; }
        Ok(())
    })();
    if let Err(e) = inner { publish(&state, &bank, "failed")?; return Err(e); }
    Ok(json!({"bank": bank, "state": state}))
}

/// runShock with v1 journal semantics (verifySource over full bytes each poll).
pub fn run_shock(dir: &Path, initialize: bool, once: bool, workspace_dir: &Path, now: &dyn Fn() -> i64) -> Result<V, String> {
    let source_file = dir.join("journal.jsonl");
    run_shock_with_source(dir, initialize, once, workspace_dir, now,
        || verify_source(&fs::read_to_string(&source_file).map_err(|e| e.to_string())?), || None)
}
