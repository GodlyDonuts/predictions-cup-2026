// live-paper/edge-observation-plan.mjs — fixed observation blocks that change
// read allocation only, never execution leads.

use baal_service_runtime::json_text::{canonical_json, digest, js_int, js_truthy, sha256};
use crate::breadth::validate_universe;
use serde_json::{json, Value as V};
use std::fs;
use std::path::Path;

pub const EDGE_OBSERVATION_MS: i64 = 420_000;

fn need(ok: bool) -> Result<(), String> {
    if !ok { return Err("INVALID_EDGE_OBSERVATION".into()); }
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
fn set_has(set: &[V], v: &V) -> bool { set.iter().any(|x| strict_eq(x, v)) }
fn parse_ms(x: &V) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(x.as_str().unwrap_or("")).unwrap_or(f64::NAN)
}
fn cohort_name_ok(s: &str) -> bool {
    s.len() >= 2 && s.len() <= 64
        && s.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// rankObservationHints(coverage, universe, {now, lastAttempts}).
/// Bulk prices select which books deserve sustained measurement only.
pub fn rank_observation_hints(coverage: &V, universe: &V, now: i64, last_attempts: &V) -> Vec<V> {
    if now < 0
        || !js_truthy(Some(coverage))
        || !strict_eq(&coverage["universeHash"], &universe["hash"])
        || !strict_eq(&coverage["tournamentId"], &universe["tournamentId"])
        || !strict_eq(&coverage["executable"], &V::Bool(false))
        || !strict_eq(&coverage["depthAvailable"], &V::Bool(false))
        || !coverage.get("sourceTimestamp").is_some_and(|t| t.is_null())
        || !js_truthy(Some(&coverage["quotes"])) {
        return Vec::new();
    }
    let mut hints: Vec<V> = Vec::new();
    for spec in universe["exchanges"].as_array().cloned().unwrap_or_default() {
        let eid = to_key(&spec["exchangeId"]);
        let q = &coverage["quotes"][&eid];
        if !js_truthy(Some(q))
            || !strict_eq(&q["marketId"], &spec["marketId"])
            || !strict_eq(&q["exchangeId"], &spec["exchangeId"])
            || !strict_eq(&q["option"], &spec["option"])
            || !strict_eq(&q["option"], &json!("YES"))
            || !strict_eq(&q["crossed"], &V::Bool(false)) {
            continue;
        }
        let receipt = parse_ms(&q["receivedAt"]);
        let request = parse_ms(&q["requestStartedAt"]);
        let bid = q["bestBid"].as_f64().unwrap_or(f64::NAN);
        let ask = q["bestAsk"].as_f64().unwrap_or(f64::NAN);
        if !receipt.is_finite() || !request.is_finite() || request > receipt
            || receipt > now as f64 || now as f64 - receipt > 120_000.0
            || !q["bestBid"].is_number() || !q["bestAsk"].is_number()
            || !bid.is_finite() || !ask.is_finite()
            || bid < 0.05 || ask > 0.95 || ask <= bid || ask - bid > 0.020_000_000_001 {
            continue;
        }
        let last_attempt = last_attempts.get(&to_key(&spec["marketId"]))
            .filter(|v| js_truthy(Some(v))).cloned().unwrap_or(json!(0));
        hints.push(json!({
            "marketId": spec["marketId"],
            "spread": ((ask - bid) * 1e6 + 0.5).floor(),
            "lastAttempt": last_attempt,
        }));
    }
    hints.sort_by(|a, b| {
        js_num(&a["spread"]).partial_cmp(&js_num(&b["spread"])).unwrap_or(std::cmp::Ordering::Equal)
            .then(js_num(&a["lastAttempt"]).partial_cmp(&js_num(&b["lastAttempt"])).unwrap_or(std::cmp::Ordering::Equal))
            .then(js_num(&a["marketId"]).partial_cmp(&js_num(&b["marketId"])).unwrap_or(std::cmp::Ordering::Equal))
    });
    hints.into_iter().map(|x| x["marketId"].clone()).collect()
}

/// readEdgeObservationFocus(runtimeDir, universe, now) — try/catch → null.
/// `live_paper_dir` locates the wrapper sources for wrapperHash (import.meta.url in JS).
pub fn read_edge_observation_focus(runtime_dir: &Path, universe: &V, now: i64,
    live_paper_dir: &Path) -> Option<V> {
    (|| -> Result<V, String> {
        if runtime_dir.join("EXECUTABLE_EDGE_STOP").exists() { return Err("stop".into()); }
        let manifest: V = serde_json::from_slice(
            &fs::read(runtime_dir.join("executable-edge-active.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        need(manifest["cohort"].as_str().is_some_and(cohort_name_ok)
            && manifest["bankHash"].as_str().is_some_and(hex64))?;
        let dir = runtime_dir.join("executable-edge-cohorts").join(manifest["cohort"].as_str().unwrap_or(""));
        if dir.join("STOP").exists() { return Err("stop".into()); }
        let bank: V = serde_json::from_slice(&fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let stats: V = serde_json::from_slice(&fs::read(dir.join("stats.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        need(digest(&bank) == manifest["bankHash"].as_str().unwrap_or("")
            && strict_eq(&bank["universe"]["tournamentId"], &universe["tournamentId"])
            && strict_eq(&stats["bankHash"], &manifest["bankHash"])
            && strict_eq(&stats["cohort"], &manifest["cohort"])
            && stats["mode"].as_str() == Some("execution_aware_online_paper")
            && strict_eq(&stats["failed"], &V::Bool(false))
            && strict_eq(&stats["sourceClosed"], &V::Bool(false))
            && strict_eq(&stats["networkRequests"], &json!(0))
            && strict_eq(&stats["competitionOrders"], &json!(0)))?;
        let lock: V = serde_json::from_slice(&fs::read(dir.join("worker.lock")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let updated = parse_ms(&stats["updatedAt"]);
        need(js_int(&lock["pid"]).is_some_and(|p| p > 0)
            && lock["id"].as_str().is_some_and(|s| !s.is_empty())
            && now as f64 >= updated && now as f64 - updated <= 20_000.0)?;
        // process.kill(lock.pid, 0) — pid liveness check (no signal sent).
        need(lock["pid"].as_i64().is_some_and(|p| Path::new(&format!("/proc/{p}")).exists()))?;
        let canonical = validate_universe(&bank["universe"], universe["tournamentId"].as_str().unwrap_or(""))?;
        need(strict_eq(&canonical["hash"], &universe["hash"])
            && digest(&canonical["exchanges"]) == digest(&universe["exchanges"]))?;
        let mut focus = json!({"bankHash": manifest["bankHash"]});
        let projection_file = dir.join("trial-observation.json");
        if projection_file.exists() {
            let _ = (|| -> Result<(), String> {
                let projection: V = serde_json::from_slice(
                    &fs::read(&projection_file).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                let mut body = projection.clone();
                if let Some(o) = body.as_object_mut() { o.shift_remove("projectionHash"); }
                // The writer's digest uses this schema's insertion order; atomicJson
                // stores sorted keys. Restore that fixed order for proof.
                let trials: Vec<V> = body["trials"].as_array().cloned().unwrap_or_default()
                    .iter().map(|t| json!({
                        "marketId": t["marketId"], "phase": t["phase"],
                        "preparedAt": t["preparedAt"], "filledAt": t["filledAt"],
                        "dueAt": t["dueAt"], "expiresAt": t["expiresAt"],
                    })).collect();
                let ordered = json!({
                    "schemaVersion": body["schemaVersion"], "mode": body["mode"],
                    "workerState": body["workerState"], "pid": body["pid"],
                    "lockId": body["lockId"], "wrapperHash": body["wrapperHash"],
                    "updatedAt": body["updatedAt"], "bankHash": body["bankHash"],
                    "stateHash": body["stateHash"], "sourceSequence": body["sourceSequence"],
                    "sourceHash": body["sourceHash"], "journalSequence": body["journalSequence"],
                    "journalHash": body["journalHash"], "sourceClosed": body["sourceClosed"],
                    "trials": trials, "networkRequests": body["networkRequests"],
                    "ordersSubmitted": body["ordersSubmitted"],
                });
                let mut wrapper_bytes = fs::read(live_paper_dir.join("executable-edge-observe.mjs"))
                    .map_err(|e| e.to_string())?;
                wrapper_bytes.extend(fs::read(live_paper_dir.join("edge-trial-projection.mjs"))
                    .map_err(|e| e.to_string())?);
                let wrapper_hash = sha256(&wrapper_bytes);
                let updated_p = parse_ms(&projection["updatedAt"]);
                need(canonical_json(&ordered)? == canonical_json(&body)?
                    && digest(&ordered) == projection["projectionHash"].as_str().unwrap_or("")
                    && strict_eq(&projection["wrapperHash"], &json!(wrapper_hash))
                    && projection["schemaVersion"].as_i64() == Some(1)
                    && projection["mode"].as_str() == Some("edge_trial_observation_only")
                    && projection["workerState"].as_str() == Some("running")
                    && strict_eq(&projection["pid"], &lock["pid"])
                    && strict_eq(&projection["lockId"], &lock["id"])
                    && strict_eq(&projection["bankHash"], &stats["bankHash"])
                    && strict_eq(&projection["stateHash"], &stats["stateHash"])
                    && strict_eq(&projection["sourceSequence"], &stats["sourceSequence"])
                    && strict_eq(&projection["sourceHash"], &stats["sourceHash"])
                    && strict_eq(&projection["journalSequence"], &stats["journalSequence"])
                    && strict_eq(&projection["journalHash"], &stats["journalHash"])
                    && strict_eq(&projection["sourceClosed"], &V::Bool(false))
                    && strict_eq(&projection["networkRequests"], &json!(0))
                    && strict_eq(&projection["ordersSubmitted"], &json!(0))
                    && now as f64 >= updated_p && now as f64 - updated_p <= 20_000.0)?;
                focus["trials"] = projection["trials"].clone();
                Ok(())
            })();
        }
        Ok(focus)
    })().ok()
}

/// planEdgeObservation(plan, state, universe, {focus, now, excludedMarketIds, coverage})
/// — returns (plan, state).
pub fn plan_edge_observation(plan: &V, state: &V, universe: &V, focus: Option<&V>, now: i64,
    excluded_market_ids: &[V], coverage: Option<&V>) -> Result<(V, V), String> {
    if focus.is_none() || !js_truthy(focus)
        || js_truthy(plan.get("comparisonSampling"))
        || plan["basketMeasurement"].get("marketIds").and_then(|m| m.as_array()).is_some_and(|m| !m.is_empty())
        || plan["paperPackageFocus"].get("marketIds").and_then(|m| m.as_array()).is_some_and(|m| !m.is_empty()) {
        return Ok((plan.clone(), state.clone()));
    }
    let focus = focus.unwrap();
    need(now >= 0 && focus["bankHash"].as_str().is_some_and(hex64))?;
    let valid: Vec<V> = universe["exchanges"].as_array().cloned().unwrap_or_default()
        .iter().map(|x| x["marketId"].clone()).collect();
    let excluded: Vec<V> = excluded_market_ids.to_vec();
    let slots: Vec<(usize, V)> = plan["reads"].as_array()
        .ok_or("TypeError: plan.reads.map is not a function")?
        .iter().cloned().enumerate()
        .filter(|(_, read)| {
            read["reason"].as_str().is_some_and(|r| ["discovery", "comparison"].contains(&r))
                && !set_has(&excluded, &read["marketId"])
        }).collect();
    if slots.len() < 2 { return Ok((plan.clone(), state.clone())); }
    let saved = state.get("edgeObservation").cloned().filter(|v| js_truthy(Some(v)));
    let mut block = saved.clone();
    if let Some(sv) = &saved {
        need(sv["bankHash"].as_str().is_some_and(hex64)
            && js_int(&sv["startedAt"]).is_some()
            && js_int(&sv["until"]).is_some()
            && js_num(&sv["startedAt"]) <= now as f64
            && strict_eq(&sv["until"], &json!(js_int(&sv["startedAt"]).unwrap_or(0) + EDGE_OBSERVATION_MS))
            && sv["marketIds"].as_array().is_some_and(|ids| {
                ids.len() == 2
                    && !strict_eq(&ids[0], &ids[1])
                    && ids.iter().all(|id| set_has(&valid, id))
            }))?;
    }
    let rebuild = match &block {
        None => true,
        Some(b) => {
            !strict_eq(&b["bankHash"], &focus["bankHash"])
                || now as f64 >= js_num(&b["until"])
                || b["marketIds"].as_array().is_some_and(|ids| ids.iter().any(|id| set_has(&excluded, id)))
        }
    };
    if rebuild {
        let choices: Vec<&(usize, V)> = slots.iter()
            .filter(|(_, read)| read["reason"].as_str() == Some("discovery")
                && set_has(&valid, &read["marketId"]))
            .collect();
        if choices.len() < 2 { return Ok((plan.clone(), state.clone())); }
        let discovery = choices[0].1["marketId"].clone();
        let protected_ids: Vec<V> = plan["reads"].as_array().cloned().unwrap_or_default()
            .into_iter()
            .filter(|x| !x["reason"].as_str().is_some_and(|r| ["discovery", "comparison"].contains(&r)))
            .map(|x| x["marketId"].clone()).collect();
        // plan.reads array-ness already enforced by slots above.
        let last_attempts = state.get("bookLastAttempt").cloned()
            .filter(|v| js_truthy(Some(v))).unwrap_or_else(|| json!({}));
        let cov = coverage.cloned().unwrap_or(V::Null);
        let hints = rank_observation_hints(&cov, universe, now, &last_attempts);
        let hint = hints.iter().find(|id| {
            !strict_eq(id, &discovery) && !set_has(&excluded, id) && !set_has(&protected_ids, id)
        }).cloned().filter(|h| js_truthy(Some(h)));
        block = Some(json!({
            "bankHash": focus["bankHash"], "startedAt": now, "until": now + EDGE_OBSERVATION_MS,
            "marketIds": [discovery, hint.unwrap_or_else(|| choices[1].1["marketId"].clone())],
        }));
    }
    let block = block.unwrap();
    let present: Vec<V> = plan["reads"].as_array().cloned().unwrap_or_default()
        .iter().map(|x| x["marketId"].clone()).collect();
    // (plan.reads array-ness was enforced above by slots.)
    let missing: Vec<V> = block["marketIds"].as_array().cloned().unwrap_or_default()
        .into_iter().filter(|id| !set_has(&present, id)).collect();
    let mut replaceable: Vec<(usize, V)> = slots.iter()
        .filter(|(_, read)| !block["marketIds"].as_array().is_some_and(|ids| set_has(ids, &read["marketId"])))
        .cloned().collect();
    replaceable.sort_by_key(|(_, r)| if r["reason"].as_str() == Some("discovery") { 0 } else { 1 });
    if missing.len() > replaceable.len() { return Ok((plan.clone(), state.clone())); }
    let mut reads: Vec<V> = plan["reads"].as_array().cloned().unwrap_or_default();
    for i in 0..missing.len() {
        reads[replaceable[i].0] = json!({"marketId": missing[i], "reason": "edge_observation",
            "path": "supplementalDepth"});
    }
    let observed: Vec<V> = block["marketIds"].as_array().cloned().unwrap_or_default();
    let admitted: Vec<V> = reads.iter().map(|x| x["marketId"].clone()).collect();
    let mut new_state = state.clone();
    new_state["edgeObservation"] = block.clone();
    let mut new_plan = plan.clone();
    new_plan["reads"] = json!(reads);
    new_plan["entryMarketIds"] = json!(plan["entryMarketIds"].as_array()
        .ok_or("TypeError: plan.entryMarketIds.filter is not a function")?
        .iter().filter(|id| set_has(&admitted, id) && !set_has(&observed, id))
        .cloned().collect::<Vec<V>>());
    let mut eo = block.clone();
    eo["requestsAdded"] = json!(0);
    eo["executionAuthority"] = json!(false);
    new_plan["edgeObservation"] = eo;
    Ok((new_plan, new_state))
}
