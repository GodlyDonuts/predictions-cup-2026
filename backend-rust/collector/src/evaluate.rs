// live-paper/evaluate.mjs — frozen-plan derived-window evaluation.
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{digest, js_int, js_stringify};
use serde_json::{json, Map, Value as V};
use std::collections::BTreeSet;

const OVERRIDE_KEYS: [&str; 4] = ["feeBps", "slippageTicks", "latencyMs", "depthFractionBps"];

fn truthy(v: &V) -> bool {
    match v { V::Null => false, V::Bool(b) => *b, V::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0, V::String(s) => !s.is_empty(), _ => true }
}
/// Object.keys(v): own enumerable index/key names for arrays/strings/objects;
/// numbers and booleans yield none.
fn object_keys(v: &V) -> Vec<String> {
    match v {
        V::Object(m) => m.keys().cloned().collect(),
        V::Array(a) => (0..a.len()).map(|i| i.to_string()).collect(),
        V::String(s) => (0..s.chars().count()).map(|i| i.to_string()).collect(),
        _ => Vec::new(),
    }
}
/// {...base, ...overrides}: overrides win, existing keys keep position.
fn spread(base: &V, over: &V) -> V {
    let mut out = base.as_object().cloned().unwrap_or_default();
    if let V::Object(o) = over {
        for (k, v) in o { out.insert(k.clone(), v.clone()); }
    }
    V::Object(out)
}
/// parse_iso_ms on the first truthy of the given string fields, else `fallback`.
fn first_ms(v: &V, keys: &[&str], fallback: &V) -> Option<f64> {
    for k in keys {
        if truthy(&v[k]) { return v[k].as_str().and_then(parse_iso_ms); }
    }
    fallback.as_str().and_then(parse_iso_ms)
}

/// validatePlan(p)
pub fn validate_plan(p: &V) -> Result<V, String> {
    let created = p["createdAt"].as_str().and_then(parse_iso_ms);
    let start = p["startAt"].as_str().and_then(parse_iso_ms);
    let end = p["endAt"].as_str().and_then(parse_iso_ms);
    if p["schemaVersion"].as_i64() != Some(1)
        || !created.is_some_and(|x| x.is_finite())
        || !start.is_some_and(|x| x.is_finite())
        || !end.is_some_and(|x| x.is_finite())
        || !(created.unwrap() < start.unwrap() && start.unwrap() < end.unwrap())
    {
        return Err("Plan must be frozen before a valid future window".into());
    }
    crate::model::validate_config(&p["baseConfig"])?;
    if p["baseConfigHash"].as_str() != Some(digest(&p["baseConfig"]).as_str())
        || js_int(&p["minimumClosedTrades"]).filter(|&x| x >= 100).is_none()
        || !p["variants"].as_array().is_some_and(|v| !v.is_empty())
    {
        return Err("Invalid frozen plan".into());
    }
    let mut names = BTreeSet::new();
    for v in p["variants"].as_array().unwrap() {
        let name_ok = v["name"].as_str().is_some_and(|n| !n.is_empty()) && !names.contains(v["name"].as_str().unwrap());
        let overrides_ok = truthy(&v["overrides"]) && object_keys(&v["overrides"]).iter().all(|k| OVERRIDE_KEYS.contains(&k.as_str()));
        if !name_ok || !overrides_ok { return Err("Invalid sensitivity variant".into()); }
        names.insert(v["name"].as_str().unwrap().to_string());
        let c = spread(&p["baseConfig"], &v["overrides"]);
        crate::model::validate_config(&c)?;
        if v["configHash"].as_str() != Some(digest(&c).as_str()) {
            return Err("Variant config checksum mismatch".into());
        }
    }
    Ok(p.clone())
}

// The original complete chain is verified before selecting any observations.
// A derived chain is an explicit window transform, never original source evidence.
/// evaluateJournal(raw, plan)
pub fn evaluate_journal(raw: &str, plan: &V) -> Result<V, String> {
    validate_plan(plan)?;
    let original = crate::replay::replay_journal(Some(raw), None, &plan["baseConfig"], crate::snapshot_order::LEGACY_SNAPSHOT_ORDER)?;
    let start = parse_iso_ms(plan["startAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
    let end = parse_iso_ms(plan["endAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
    let mut previous = String::new();
    let mut sequence = 0i64;
    let mut selected: Vec<String> = Vec::new();
    let mut any_cycle = false;
    let mut excluded_snapshots = 0i64;
    // append(event, at): canonical hash-chained derived record line.
    fn append(event: V, at: &V, previous: &mut String, sequence: &mut i64, selected: &mut Vec<String>) {
        *sequence += 1;
        let mut body = Map::new();
        body.insert("sequence".into(), json!(*sequence));
        body.insert("previous".into(), json!(previous.clone()));
        body.insert("at".into(), at.clone());
        body.insert("event".into(), event);
        body.insert("state".into(), json!({}));
        let hash = digest(&V::Object(body.clone()));
        *previous = hash.clone();
        body.insert("hash".into(), json!(hash));
        selected.push(js_stringify(&V::Object(body)));
    }
    for line in raw.trim_end().split('\n') {
        let r: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let e = r["event"].clone();
        let clock_v = if truthy(&e["clockAt"]) { e["clockAt"].clone() } else { r["at"].clone() };
        let t = clock_v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN);
        if t < start { continue; }
        if e["type"].as_str() == Some("cycle") {
            let scoped = crate::depth_scope::scoped_snapshots(&e, &plan["baseConfig"])?;
            let bad_exclusion = !e["accountExclusions"].as_array().is_some_and(|a| a.iter().all(|id| {
                id.as_str().is_some_and(|s| {
                    s.as_bytes().first().is_some_and(|b| (b'1'..=b'9').contains(b))
                        && s.bytes().all(|b| b.is_ascii_digit())
                        && s.parse::<u64>().is_ok_and(|n| n <= 9_007_199_254_740_991)
                })
            }));
            if bad_exclusion { return Err("Incomplete window account exclusions".into()); }
            let reconciliation = e["exclusionObservedAt"].as_str().and_then(parse_iso_ms);
            if !reconciliation.is_some_and(|x| x.is_finite()) || reconciliation.unwrap() < start || reconciliation.unwrap() >= end {
                excluded_snapshots += scoped.len() as i64;
                continue;
            }
            let reconciliation = reconciliation.unwrap();
            let mut eligible = |s: &V| -> bool {
                let received = first_ms(s, &["receivedAt", "at"], &clock_v);
                let requested = first_ms(s, &["requestStartedAt", "at"], &clock_v);
                let inside = received.is_some_and(|x| x.is_finite()) && requested.is_some_and(|x| x.is_finite())
                    && requested.unwrap() >= start && requested.unwrap() >= reconciliation
                    && received.unwrap() >= requested.unwrap() && received.unwrap() <= t && received.unwrap() < end;
                if !inside { excluded_snapshots += 1; }
                inside
            };
            let kept: Vec<V> = scoped.iter().filter(|s| eligible(s)).cloned().collect();
            let in_kept = |s: &V| kept.iter().any(|k| k == s);
            let source_snapshots: Vec<V> = e["sourceSnapshots"].as_array().cloned().unwrap_or_default()
                .into_iter().filter(|s| in_kept(s)).collect();
            let supplemental: Vec<V> = e["supplementalDepth"].as_array().cloned().unwrap_or_default()
                .into_iter().filter(|s| in_kept(s)).collect();
            if !kept.is_empty() {
                let at = json!(ms_to_iso(t.min(end)));
                let mut ev = e.as_object().cloned().unwrap_or_default();
                ev.insert("clockAt".into(), at.clone());
                ev.insert("sourceSnapshots".into(), json!(source_snapshots));
                ev.insert("supplementalDepth".into(), json!(supplemental));
                append(V::Object(ev), &at, &mut previous, &mut sequence, &mut selected);
                any_cycle = true;
            }
        } else if e["type"].as_str() == Some("account_read_outage") && t < end {
            append(e, &clock_v, &mut previous, &mut sequence, &mut selected);
        }
    }
    let completed = original["replay"]["finalEventClock"].as_str().and_then(parse_iso_ms).map(|x| x >= end).unwrap_or(false);
    let final_clock = if completed { plan["endAt"].clone() } else { original["replay"]["finalEventClock"].clone() };
    let mut metadata = Map::new();
    metadata.insert("planHash".into(), json!(digest(plan)));
    metadata.insert("originalSourceHash".into(), original["replay"]["sourceHash"].clone());
    metadata.insert("originalSourceRecords".into(), original["replay"]["sourceRecords"].clone());
    metadata.insert("startAt".into(), plan["startAt"].clone());
    metadata.insert("endAt".into(), plan["endAt"].clone());
    metadata.insert("captureThrough".into(), original["replay"]["finalEventClock"].clone());
    metadata.insert("windowComplete".into(), json!(completed));
    metadata.insert("excludedSnapshots".into(), json!(excluded_snapshots));
    metadata.insert("networkRequests".into(), json!(0));
    metadata.insert("competitionOrders".into(), json!(0));
    metadata.insert("profitabilityEstablished".into(), json!(false));
    if !any_cycle {
        let mut out = metadata.clone();
        out.insert("status".into(), json!("awaiting_in_window_capture"));
        out.insert("variants".into(), json!([]));
        return Ok(V::Object(out));
    }
    append(json!({"type": "evaluation_cutoff", "clockAt": final_clock}), &final_clock, &mut previous, &mut sequence, &mut selected);
    let derived = selected.join("\n") + "\n";
    let mut variants = Vec::new();
    for v in plan["variants"].as_array().unwrap() {
        let c = spread(&plan["baseConfig"], &v["overrides"]);
        let r = match crate::replay::replay_journal(Some(&derived), None, &c, crate::snapshot_order::LEGACY_SNAPSHOT_ORDER) {
            Ok(r) => r,
            Err(e) if e == "No accepted real source observations" => {
                variants.push(json!({"name": v["name"], "configHash": v["configHash"], "observations": 0,
                    "sampleStatus": "no_accepted_window_books"}));
                continue;
            }
            Err(e) => return Err(e),
        };
        let mut strategies = Map::new();
        for (name, s) in r["strategies"].as_object().cloned().unwrap_or_default() {
            let positions = s["positions"].as_array().cloned().unwrap_or_default();
            strategies.insert(name, json!({
                "cash": s["cash"], "realizedPnl": s["realizedPnl"], "referencePnl": s["referencePnl"],
                "liquidationPnl": s["liquidationPnl"], "fees": s["fees"], "fills": s["fills"],
                "intents": s["intents"], "closedTrades": s["closedTrades"], "winRate": s["winRate"],
                "openShares": positions.iter().map(|p| p["quantity"].as_i64().unwrap_or(0)).sum::<i64>(),
                "pendingIntents": s["pending"].as_array().map(|a| a.len()).unwrap_or(0),
                "unresolvedPositions": positions.iter().map(|p| json!({"instrument": p["instrument"],
                    "quantity": p["quantity"], "unsoldShares": p["unsoldShares"],
                    "observationAgeMs": p["observationAgeMs"]})).collect::<Vec<_>>(),
                "maxObservedDrawdown": s["maxObservedDrawdown"],
            }));
        }
        let sample_status = if !completed { "window_incomplete" }
            else if r["strategies"]["mean_reversion"]["closedTrades"].as_i64().unwrap_or(0) < js_int(&plan["minimumClosedTrades"]).unwrap_or(0) {
                "insufficient_closed_trades"
            } else { "eligible_for_independent_analysis" };
        variants.push(json!({"name": v["name"], "configHash": r["configHash"], "observations": r["observations"],
            "derivedWindowHash": r["replay"]["sourceHash"], "strategies": V::Object(strategies),
            "noTradeBaseline": r["noTradeBaseline"], "sampleStatus": sample_status}));
    }
    let mut out = metadata;
    out.insert("status".into(), json!(if completed { "window_complete_pending_review" } else { "collecting_frozen_window" }));
    out.insert("variants".into(), json!(variants));
    out.insert("limitations".into(), json!([
        "Synthetic tests verify mechanics only",
        "Fees are unverified scenarios; every listed variant must be reported",
        "Later REST snapshots are hypothetical taker opportunities, not confirmed fills",
        "Unresolved positions are not forcibly liquidated at an invented price",
        "Midpoint returns exclude executable exit costs; liquidation can be unavailable",
        "Trade count alone does not establish independence, calibration or profitability",
        "Original capture history remains exploratory; each window starts flat with in-window warmup",
    ]));
    Ok(V::Object(out))
}
