// live-paper/comparison-sampling.mjs — operator collection treatment.
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

pub fn comparison_sampling() -> V {
    json!({"mode": "paired_comparison_two_attempts_v1", "attemptsPerMarket": 2,
        "carryMs": 140000, "panel": ["377", "354", "277", "305"]})
}
fn need(ok: bool, msg: &str) -> Result<(), String> { if !ok { return Err(msg.into()); } Ok(()) }
fn uuid(s: &str) -> bool {
    s.len() == 36 && s.bytes().enumerate().all(|(i, b)| match i { 8 | 13 | 18 | 23 => b == b'-', _ => b.is_ascii_hexdigit() })
}
fn canonical(s: &V) -> bool {
    s.as_str().and_then(parse_iso_ms).is_some_and(|ms| ms_to_iso(ms) == s.as_str().unwrap())
}
fn hash_js(v: &V) -> String { baal_service_runtime::json_text::digest(v) }
fn plan_hash(p: &V) -> String {
    hash_js(&json!({"sequence": p["sequence"], "sessionId": p["sessionId"], "at": p["at"],
        "phase": p["phase"], "phaseHash": p["phaseHash"], "marketIds": p["marketIds"]}))
}
struct Ctx { state: V, ledger_index: Vec<String>, panel: Vec<String> }
fn context(input: &V, scope: &V) -> Result<Ctx, String> {
    let mut state = crate::book_scheduler::validate_scheduler_state(input, scope)?;
    let panel: Vec<String> = comparison_sampling()["panel"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().into()).collect();
    let core: Vec<String> = scope["coreMarketIds"].as_array().cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(String::from)).collect();
    need(core.len() == 4 && panel.iter().all(|id| core.contains(id)), "Comparison sampling requires the fixed four-market panel")?;
    let ledger = state.get("comparisonSampling").cloned().unwrap_or_else(|| json!({
        "schemaVersion": 1, "mode": comparison_sampling()["mode"], "scopeHash": state["scopeHash"],
        "phase": 0, "cursor": 0, "totalAttempts": 0, "phaseAttemptStart": 0,
        "selectedMarketIds": [], "remainingAttempts": {}, "startedAt": null, "expiresAt": null,
        "lastAttemptAt": {}, "lastPhase": null, "sessionId": null, "plans": 0, "lastPlan": null}));
    need(ledger["schemaVersion"].as_i64() == Some(1) && ledger["mode"] == comparison_sampling()["mode"]
        && ledger["scopeHash"] == state["scopeHash"], "Invalid comparison sampling identity")?;
    for key in ["phase", "cursor", "totalAttempts", "phaseAttemptStart", "plans"] {
        need(ledger[key].as_i64().is_some_and(|x| x >= 0), "Invalid comparison sampling counter")?;
    }
    need(ledger["sessionId"].is_null() || ledger["sessionId"].as_str().is_some_and(uuid), "Invalid comparison session")?;
    need((ledger["cursor"].as_i64().unwrap() as usize) < panel.len()
        && ledger["phaseAttemptStart"].as_i64().unwrap() <= ledger["totalAttempts"].as_i64().unwrap()
        && ledger["selectedMarketIds"].as_array().is_some_and(|a| [0usize, 2].contains(&a.len())
            && a.iter().map(|x| x.as_str().unwrap_or("")).collect::<BTreeSet<_>>().len() == a.len()
            && a.iter().all(|id| panel.contains(&id.as_str().unwrap_or("").to_string())))
        && ledger["remainingAttempts"].is_object() && ledger["lastAttemptAt"].is_object(), "Invalid comparison sampling phase")?;
    let sel: Vec<String> = ledger["selectedMarketIds"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().into()).collect();
    need(ledger["remainingAttempts"].as_object().unwrap().len() == sel.len()
        && ledger["remainingAttempts"].as_object().unwrap().keys().all(|k| sel.contains(k)), "Invalid comparison remaining scope")?;
    for id in &sel {
        need(ledger["remainingAttempts"][id].as_i64().is_some_and(|n| (0..=2).contains(&n)), "Invalid comparison remaining attempts")?;
    }
    for (id, at) in ledger["lastAttemptAt"].as_object().unwrap() {
        need(panel.contains(id) && canonical(at), "Invalid comparison attempt clock")?;
    }
    if !sel.is_empty() {
        need(ledger["phase"].as_i64().unwrap() > 0 && canonical(&ledger["startedAt"]) && canonical(&ledger["expiresAt"])
            && parse_iso_ms(ledger["expiresAt"].as_str().unwrap()).unwrap() - parse_iso_ms(ledger["startedAt"].as_str().unwrap()).unwrap() == 140_000.0
            && ledger["totalAttempts"].as_i64().unwrap() - ledger["phaseAttemptStart"].as_i64().unwrap()
                == 4 - ledger["remainingAttempts"].as_object().unwrap().values().map(|v| v.as_i64().unwrap_or(0)).sum::<i64>(),
            "Invalid comparison phase lineage")?;
    } else {
        need(ledger["startedAt"].is_null() && ledger["expiresAt"].is_null()
            && ledger["phaseAttemptStart"] == ledger["totalAttempts"], "Invalid idle comparison phase")?;
    }
    if !ledger["lastPhase"].is_null() {
        let lp = &ledger["lastPhase"];
        need(lp.is_object() && lp["phase"].as_i64().is_some_and(|p| p > 0 && p <= ledger["phase"].as_i64().unwrap())
            && ["completed", "expired", "collector_restart"].contains(&lp["reason"].as_str().unwrap_or(""))
            && canonical(&lp["at"])
            && lp["marketIds"].as_array().is_some_and(|a| a.len() == 2
                && a.iter().map(|x| x.as_str().unwrap_or("")).collect::<BTreeSet<_>>().len() == 2
                && a.iter().all(|id| panel.contains(&id.as_str().unwrap_or("").to_string()))), "Invalid prior comparison phase")?;
    }
    if !ledger["lastPlan"].is_null() {
        let p = &ledger["lastPlan"];
        need(p.is_object() && p["sequence"] == ledger["plans"] && p["sessionId"] == ledger["sessionId"] && canonical(&p["at"])
            && p["phase"].as_i64().is_some_and(|x| x <= ledger["phase"].as_i64().unwrap())
            && p["marketIds"].as_array().is_some_and(|a| a.len() <= 2
                && a.iter().map(|x| x.as_str().unwrap_or("")).collect::<BTreeSet<_>>().len() == a.len()
                && a.iter().all(|id| panel.contains(&id.as_str().unwrap_or("").to_string())))
            && p["attemptedMarketIds"].as_array().is_some_and(|a| a.iter().map(|x| x.as_str().unwrap_or("")).collect::<BTreeSet<_>>().len() == a.len()
                && a.iter().all(|id| p["marketIds"].as_array().unwrap().contains(id)))
            && p["planId"].as_str() == Some(plan_hash(p).as_str()), "Invalid comparison reservation plan")?;
    }
    state["comparisonSampling"] = ledger;
    Ok(Ctx { state, ledger_index: sel, panel })
}
fn finish_phase(ledger: &mut V, reason: &str, at: &str) {
    ledger["lastPhase"] = json!({"phase": ledger["phase"], "reason": reason, "at": at,
        "marketIds": ledger["selectedMarketIds"], "remainingAttempts": ledger["remainingAttempts"]});
    ledger["selectedMarketIds"] = json!([]);
    ledger["remainingAttempts"] = json!({});
    ledger["startedAt"] = V::Null;
    ledger["expiresAt"] = V::Null;
    ledger["phaseAttemptStart"] = ledger["totalAttempts"].clone();
}
fn check_clock(ledger: &V, now: i64) -> Result<(), String> {
    need(now >= 0 && now <= 8_640_000_000_000_000, "Invalid comparison sampling clock")?;
    let mut clocks: Vec<String> = ledger["lastAttemptAt"].as_object().map(|m| m.values().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    for k in ["startedAt", "expiresAt"] {
        if let Some(v) = ledger[k].as_str() { clocks.push(v.into()); }
    }
    if let Some(v) = ledger["lastPhase"]["at"].as_str() { clocks.push(v.into()); }
    if let Some(v) = ledger["lastPlan"]["at"].as_str() { clocks.push(v.into()); }
    need(clocks.iter().all(|at| parse_iso_ms(at).map(|m| m <= now as f64).unwrap_or(false)), "Comparison sampling clock rollback")?;
    Ok(())
}
/// planComparisonSampling(basePlan, scope, {now, sessionId})
pub fn plan_comparison_sampling(base_plan: &V, scope: &V, now: i64, session_id: &str) -> Result<V, String> {
    let ctx = context(&base_plan["state"], scope)?;
    let mut state = ctx.state;
    let mut ledger = state["comparisonSampling"].clone();
    check_clock(&ledger, now)?;
    need(uuid(session_id), "Invalid comparison collector session")?;
    let reads = base_plan["reads"].as_array().cloned().unwrap_or_default();
    need(reads.len() <= 4, "Invalid comparison read budget")?;
    let basket_ids: Vec<String> = base_plan["basketMeasurement"]["marketIds"].as_array().cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(String::from)).collect();
    let markets: std::collections::BTreeMap<String, V> = scope["universe"]["exchanges"].as_array().cloned().unwrap_or_default().iter().map(|x| (x["marketId"].as_str().unwrap_or("").into(), x.clone())).collect();
    need(basket_ids.iter().collect::<BTreeSet<_>>().len() == basket_ids.len()
        && basket_ids.iter().all(|id| markets.contains_key(id) && reads.iter().any(|r| r["marketId"].as_str() == Some(id.as_str()))),
        "Invalid basket reservation scope")?;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut slots: Vec<usize> = Vec::new();
    let mut protected_ids: BTreeSet<String> = BTreeSet::new();
    for (index, read) in reads.iter().enumerate() {
        let mid = read["marketId"].as_str().unwrap_or("");
        need(markets.get(mid).is_some_and(|m| m["exchangeId"] == read["exchangeId"]) && !seen.contains(mid)
            && read["path"].as_str() == Some(if ctx.panel.contains(&mid.to_string()) { "sourceSnapshots" } else { "supplementalDepth" }),
            "Invalid comparison base read")?;
        seen.insert(mid.into());
        if read["reason"].as_str() == Some("comparison") && !basket_ids.contains(&mid.to_string()) {
            need(ctx.panel.contains(&mid.to_string()), "Foreign comparison slot")?;
            slots.push(index);
        } else {
            protected_ids.insert(mid.into());
        }
    }
    need(slots.len() <= 2, "Comparison sampling has only two replaceable slots")?;
    let mut output = base_plan.clone();
    output["state"] = state.clone();
    let mut expired = false;
    let mut restarted = false;
    if !ledger["sessionId"].is_null() && ledger["sessionId"].as_str() != Some(session_id) {
        if !ledger["selectedMarketIds"].as_array().unwrap().is_empty() {
            finish_phase(&mut ledger, "collector_restart", &ms_to_iso(now as f64));
        }
        ledger["lastPlan"] = V::Null;
        restarted = true;
    }
    ledger["sessionId"] = json!(session_id);
    if !ledger["selectedMarketIds"].as_array().unwrap().is_empty()
        && now as f64 >= parse_iso_ms(ledger["expiresAt"].as_str().unwrap()).unwrap() {
        finish_phase(&mut ledger, "expired", &ms_to_iso(now as f64));
        expired = true;
    }
    if ledger["selectedMarketIds"].as_array().unwrap().is_empty() && slots.len() >= 2 {
        let order: Vec<String> = if ledger["phase"].as_i64().unwrap() == 0 {
            slots.iter().map(|i| reads[*i]["marketId"].as_str().unwrap().into()).collect()
        } else {
            (0..ctx.panel.len()).map(|i| ctx.panel[(ledger["cursor"].as_i64().unwrap() as usize + i) % ctx.panel.len()].clone()).collect()
        };
        let selected: Vec<String> = order.iter().filter(|id| !protected_ids.contains(*id)).take(2).cloned().collect();
        if selected.len() == 2 {
            ledger["phase"] = json!(ledger["phase"].as_i64().unwrap() + 1);
            ledger["selectedMarketIds"] = json!(selected);
            ledger["remainingAttempts"] = json!(selected.iter().map(|id| (id.clone(), V::from(2))).collect::<serde_json::Map<_, _>>());
            ledger["startedAt"] = json!(ms_to_iso(now as f64));
            ledger["expiresAt"] = json!(ms_to_iso(now as f64 + 140_000.0));
            ledger["phaseAttemptStart"] = ledger["totalAttempts"].clone();
            let last_idx = ctx.panel.iter().position(|p| p == selected.last().unwrap()).unwrap();
            ledger["cursor"] = json!((last_idx + 1) % ctx.panel.len());
        }
    }
    let pass_remaining = ledger["remainingAttempts"].as_object().unwrap().values().map(|v| v.as_i64().unwrap_or(0)).max().unwrap_or(0);
    let pass_members: Vec<String> = ledger["selectedMarketIds"].as_array().unwrap().iter()
        .filter(|id| ledger["remainingAttempts"][id.as_str().unwrap()].as_i64().unwrap_or(0) == pass_remaining && pass_remaining > 0)
        .map(|x| x.as_str().unwrap().into()).collect();
    let required: Vec<String> = pass_members.iter().filter(|id| !protected_ids.contains(*id)).cloned().collect();
    let mut assigned: BTreeSet<String> = protected_ids.clone();
    let order: Vec<String> = required.iter().cloned()
        .chain(slots.iter().map(|i| reads[*i]["marketId"].as_str().unwrap().into()))
        .chain((0..ctx.panel.len()).map(|i| ctx.panel[(ledger["cursor"].as_i64().unwrap() as usize + i) % ctx.panel.len()].clone()))
        .collect();
    let mut out_reads = reads.clone();
    for index in &slots {
        let market_id = order.iter().find(|id| !assigned.contains(*id)).ok_or("Comparison slot could not remain unique")?;
        assigned.insert(market_id.clone());
        let original = &reads[*index];
        out_reads[*index] = if market_id == original["marketId"].as_str().unwrap_or("") {
            original.clone()
        } else {
            let spec = &markets[market_id];
            let mut r = original.clone();
            r["marketId"] = json!(market_id);
            r["exchangeId"] = spec["exchangeId"].clone();
            r["option"] = spec["option"].clone();
            r["path"] = json!("sourceSnapshots");
            r
        };
    }
    let sel: Vec<String> = ledger["selectedMarketIds"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().into()).collect();
    let phase_identity = if !sel.is_empty() {
        Some(json!({"scopeHash": ledger["scopeHash"], "mode": ledger["mode"], "phase": ledger["phase"],
            "marketIds": sel, "startedAt": ledger["startedAt"], "expiresAt": ledger["expiresAt"]}))
    } else { None };
    ledger["plans"] = json!(ledger["plans"].as_i64().unwrap() + 1);
    let reservation_market_ids: Vec<String> = out_reads.iter().filter(|r| pass_members.contains(&r["marketId"].as_str().unwrap_or("").to_string()))
        .map(|r| r["marketId"].as_str().unwrap().into()).collect();
    let mut plan = json!({"sequence": ledger["plans"], "sessionId": session_id, "at": ms_to_iso(now as f64),
        "phase": ledger["phase"], "phaseHash": phase_identity.as_ref().map(|p| json!(hash_js(p))).unwrap_or(V::Null),
        "marketIds": reservation_market_ids, "attemptedMarketIds": []});
    plan["planId"] = json!(plan_hash(&plan));
    ledger["lastPlan"] = plan.clone();
    output["reads"] = json!(out_reads);
    output["state"]["comparisonSampling"] = ledger.clone();
    output["measurement"] = json!({"mode": comparison_sampling()["mode"],
        "reason": if phase_identity.is_some() { "paired_comparison_sampling" } else { "insufficient_comparison_slots" },
        "expiredCarry": expired, "abandonedOnRestart": restarted, "phase": ledger["phase"],
        "phaseHash": phase_identity.as_ref().map(|p| json!(hash_js(p))).unwrap_or(V::Null),
        "planId": plan["planId"], "marketIds": sel,
        "remainingAttempts": ledger["remainingAttempts"],
        "startedAt": ledger["startedAt"], "expiresAt": ledger["expiresAt"],
        "reservationMarketIds": reservation_market_ids,
        "persistBeforeDispatch": true,
        "attemptMeaning": "durable_pre_request_reservation_not_confirmed_admission_or_receipt",
        "requestsAdded": 0, "outcomesGuaranteed": false});
    Ok(output)
}
/// recordComparisonAttempt(input, scope, {marketId, at, planId})
pub fn record_comparison_attempt(input: &V, scope: &V, market_id: &str, at: &str, plan_id: &str) -> Result<V, String> {
    let ctx = context(input, scope)?;
    let mut state = ctx.state;
    let mut ledger = state["comparisonSampling"].clone();
    let markets: std::collections::BTreeMap<String, V> = scope["universe"]["exchanges"].as_array().cloned().unwrap_or_default().iter().map(|x| (x["marketId"].as_str().unwrap_or("").into(), x.clone())).collect();
    need(markets.contains_key(market_id) && canonical(&json!(at)), "Invalid comparison attempt")?;
    check_clock(&ledger, parse_iso_ms(at).unwrap() as i64)?;
    need(!ledger["lastPlan"].is_null() && plan_id == ledger["lastPlan"]["planId"].as_str().unwrap_or(""), "Comparison reservation plan mismatch")?;
    if !ledger["lastPlan"]["marketIds"].as_array().unwrap().iter().any(|m| m.as_str() == Some(market_id)) {
        state["comparisonSampling"] = ledger;
        return Ok(state);
    }
    need(!ledger["lastPlan"]["attemptedMarketIds"].as_array().unwrap().iter().any(|m| m.as_str() == Some(market_id)), "Duplicate comparison plan reservation")?;
    ledger["lastPlan"]["attemptedMarketIds"].as_array_mut().unwrap().push(json!(market_id));
    if ledger["selectedMarketIds"].as_array().unwrap().is_empty() {
        state["comparisonSampling"] = ledger;
        return Ok(state);
    }
    if parse_iso_ms(at).unwrap() >= parse_iso_ms(ledger["expiresAt"].as_str().unwrap()).unwrap() {
        finish_phase(&mut ledger, "expired", at);
        state["comparisonSampling"] = ledger;
        return Ok(state);
    }
    if !ledger["selectedMarketIds"].as_array().unwrap().iter().any(|m| m.as_str() == Some(market_id))
        || ledger["remainingAttempts"][market_id].as_i64().unwrap_or(0) == 0 {
        state["comparisonSampling"] = ledger;
        return Ok(state);
    }
    ledger["remainingAttempts"][market_id] = json!(ledger["remainingAttempts"][market_id].as_i64().unwrap() - 1);
    ledger["totalAttempts"] = json!(ledger["totalAttempts"].as_i64().unwrap() + 1);
    ledger["lastAttemptAt"][market_id] = json!(at);
    if ledger["remainingAttempts"].as_object().unwrap().values().all(|n| n.as_i64() == Some(0)) {
        finish_phase(&mut ledger, "completed", at);
    }
    state["comparisonSampling"] = ledger;
    Ok(state)
}
