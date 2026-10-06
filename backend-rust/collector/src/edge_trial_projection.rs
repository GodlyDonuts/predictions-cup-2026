// live-paper/edge-trial-projection.mjs — trial deadline projection and read-plan
// priority rewrite for the executable-edge observer.
use baal_paper::fixed::instant;
use baal_service_runtime::contract::ms_to_iso;
use serde_json::{json, Value as V};
use std::collections::{BTreeSet, HashSet};

fn need(ok: bool) -> Result<(), String> {
    if !ok { return Err("INVALID_EDGE_TRIAL_PROJECTION".into()); }
    Ok(())
}
fn at(v: &V) -> i64 { instant(v.as_str().unwrap_or("")).unwrap_or(i64::MIN) }

/// projectEdgeTrials(state, bank, stats, {pid, lockId, wrapperHash, workerState})
pub fn project_edge_trials(state: &V, bank: &V, stats: &V, pid: i64, lock_id: &str,
    wrapper_hash: &str, worker_state: Option<&str>) -> Result<V, String> {
    need(state["bankHash"].as_str() == Some(crate::model::digest(bank).as_str())
        && stats["bankHash"] == state["bankHash"]
        && stats["stateHash"].as_str() == Some(crate::model::digest(state).as_str())
        && pid > 0 && !lock_id.is_empty()
        && wrapper_hash.len() == 64 && wrapper_hash.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))?;
    let now = instant(stats["updatedAt"].as_str().unwrap_or("")).unwrap_or(i64::MIN);
    let scope: HashSet<String> = bank["universe"]["exchanges"].as_array().cloned().unwrap_or_default()
        .iter().filter_map(|x| x["marketId"].as_str().map(String::from)).collect();
    let mut trials = Vec::new();
    for (market_id, t) in state["trials"].as_object().cloned().unwrap_or_default() {
        need(scope.contains(&market_id))?;
        let prepared = instant(t["preparedAt"].as_str().unwrap_or(""))?;
        let filled = if t["filledAt"].is_null() { None }
            else { Some(instant(t["filledAt"].as_str().unwrap_or(""))?) };
        need(prepared <= now && filled.is_none_or(|f| f >= prepared && f <= now))?;
        let (due_at, expires_at) = match filled {
            None => (prepared + bank["config"]["latencyMs"].as_i64().unwrap_or(0),
                prepared + bank["policy"]["trialLifetimeMs"].as_i64().unwrap_or(0)),
            Some(f) => (f + bank["policy"]["horizonMs"].as_i64().unwrap_or(0),
                f + bank["policy"]["horizonMs"].as_i64().unwrap_or(0)
                    + bank["policy"]["labelLatenessMs"].as_i64().unwrap_or(0)),
        };
        trials.push(json!({"marketId": market_id,
            "phase": if filled.is_none() { "awaiting_fill" } else { "awaiting_outcome" },
            "preparedAt": t["preparedAt"], "filledAt": t["filledAt"],
            "dueAt": ms_to_iso(due_at as f64), "expiresAt": ms_to_iso(expires_at as f64)}));
    }
    let mut body = json!({"schemaVersion":1,"mode":"edge_trial_observation_only",
        "workerState": worker_state.unwrap_or("running"), "pid": pid, "lockId": lock_id,
        "wrapperHash": wrapper_hash, "updatedAt": stats["updatedAt"], "bankHash": state["bankHash"],
        "stateHash": stats["stateHash"], "sourceSequence": stats["sourceSequence"],
        "sourceHash": stats["sourceHash"], "journalSequence": stats["journalSequence"],
        "journalHash": stats["journalHash"], "sourceClosed": stats["sourceClosed"],
        "trials": trials, "networkRequests": 0, "ordersSubmitted": 0});
    body["projectionHash"] = json!(crate::model::digest(&body));
    Ok(body)
}

/// prioritizeEdgeTrials(plan, focus, universe, {now, excludedMarketIds})
pub fn prioritize_edge_trials(plan: &V, focus: Option<&V>, universe: &V, now: i64,
    excluded_market_ids: &[String]) -> Result<V, String> {
    let empty = V::Null;
    let focus = focus.unwrap_or(&empty);
    if !baal_service_runtime::json_text::js_truthy(Some(&focus["trials"]))
        || baal_service_runtime::json_text::js_truthy(Some(&plan["comparisonSampling"]))
        || plan["basketMeasurement"]["marketIds"].as_array().is_some_and(|m| !m.is_empty())
        || plan["paperPackageFocus"]["marketIds"].as_array().is_some_and(|m| !m.is_empty()) {
        return Ok(plan.clone());
    }
    need(now >= 0 && focus["trials"].is_array())?;
    let valid: HashSet<String> = universe["exchanges"].as_array().cloned().unwrap_or_default()
        .iter().filter_map(|x| x["marketId"].as_str().map(String::from)).collect();
    let excluded: HashSet<&String> = excluded_market_ids.iter().collect();
    let mut seen = HashSet::new();
    let mut eligible: Vec<V> = Vec::new();
    for t in focus["trials"].as_array().unwrap() {
        let mid = t["marketId"].as_str().unwrap_or("");
        need(valid.contains(mid) && !seen.contains(mid)
            && ["awaiting_fill", "awaiting_outcome"].contains(&t["phase"].as_str().unwrap_or("")))?;
        seen.insert(mid.to_string());
        let due = at(&t["dueAt"]);
        let expiry = at(&t["expiresAt"]);
        need(due <= expiry)?;
        if excluded.contains(&mid.to_string()) { continue; }
        let ok = if t["phase"].as_str() == Some("awaiting_fill") { now <= expiry }
            else { due <= now + 40000 };
        if ok { eligible.push(t.clone()); }
    }
    eligible.sort_by(|a, b| {
        let rank = |t: &V| if t["phase"].as_str() == Some("awaiting_outcome") {
            if now <= at(&t["expiresAt"]) { 0 } else { 2 }
        } else { 1 };
        rank(a).cmp(&rank(b)).then(at(&a["dueAt"]).cmp(&at(&b["dueAt"])))
            .then(a["marketId"].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0)
                .cmp(&b["marketId"].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0)))
    });
    let mut reads: Vec<V> = plan["reads"].as_array().cloned().unwrap_or_default();
    let mut present: HashSet<String> = reads.iter().map(|x| x["marketId"].as_str().unwrap_or("").to_string()).collect();
    let mut replacements = 0i64;
    let mut observed: Vec<String> = Vec::new();
    for trial in &eligible {
        let mid = trial["marketId"].as_str().unwrap_or("").to_string();
        if present.contains(&mid) { observed.push(mid); continue; }
        if replacements >= 2 { break; }
        let index = reads.iter().position(|x|
            ["discovery", "edge_observation", "comparison"].contains(&x["reason"].as_str().unwrap_or(""))
            && !observed.contains(&x["marketId"].as_str().unwrap_or("").to_string())
            && !eligible.iter().any(|t| t["marketId"] == x["marketId"]));
        let Some(index) = index else { continue };
        present.remove(reads[index]["marketId"].as_str().unwrap_or(""));
        reads[index] = json!({"marketId": mid, "path": "supplementalDepth", "reason": "edge_trial_deadline"});
        present.insert(mid.clone());
        observed.push(mid);
        replacements += 1;
    }
    if observed.is_empty() { return Ok(plan.clone()); }
    let present_set: HashSet<String> = present.clone();
    let mut out = plan.clone();
    out["reads"] = json!(reads);
    out["entryMarketIds"] = json!(plan["entryMarketIds"].as_array().cloned().unwrap_or_default()
        .into_iter().filter(|id| present_set.contains(id.as_str().unwrap_or(""))
            && !observed.contains(&id.as_str().unwrap_or("").to_string())).collect::<Vec<V>>());
    let mut unique = BTreeSet::new();
    for m in &observed { unique.insert(m.clone()); }
    out["edgeTrialObservation"] = json!({"marketIds": observed, "requestsAdded": 0,
        "replacedSlots": replacements, "executionAuthority": false});
    Ok(out)
}
