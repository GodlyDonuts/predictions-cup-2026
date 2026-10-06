// live-paper/book-scheduler.mjs — bounded fair book/history read scheduling.
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::collections::{HashMap, HashSet};

fn is_id(v: &V) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
        && s.as_bytes()[0] != b'0' && s.parse::<i64>().is_ok())
}
fn unique(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    values.iter().filter(|v| seen.insert((*v).clone())).cloned().collect()
}
fn canonical_clock(v: &V) -> bool {
    v.as_str().is_some_and(|s| parse_iso_ms(s).is_some_and(|ms| baal_service_runtime::contract::ms_to_iso(ms) == s))
}

struct Scope {
    market_ids: Vec<String>,
    markets: HashMap<String, V>,
    core: HashSet<String>,
    scope_hash: String,
}
fn scope_of(scope: &V) -> Result<Scope, String> {
    let universe = scope.get("universe").ok_or("Invalid scheduler universe")?;
    if !universe.get("tournamentId").and_then(V::as_str).is_some_and(|s| !s.is_empty())
        || !universe.get("exchanges").and_then(V::as_array).is_some_and(|a| !a.is_empty()) {
        return Err("Invalid scheduler universe".into());
    }
    let mut exchanges = HashSet::new();
    let mut markets: HashMap<String, V> = HashMap::new();
    for spec in universe["exchanges"].as_array().unwrap() {
        let mid = spec.get("marketId").cloned().unwrap_or(V::Null);
        let eid = spec.get("exchangeId").cloned().unwrap_or(V::Null);
        if !is_id(&mid) || !is_id(&eid) || spec.get("option").and_then(V::as_str) != Some("YES")
            || !exchanges.insert(eid.as_str().unwrap().to_string())
            || markets.contains_key(mid.as_str().unwrap()) {
            return Err("Ambiguous scheduler market scope".into());
        }
        markets.insert(mid.as_str().unwrap().to_string(), json!({
            "marketId": mid, "exchangeId": eid, "option": spec["option"]}));
    }
    let core_list: Vec<String> = scope.get("coreMarketIds").and_then(V::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    let core_set: HashSet<String> = core_list.iter().cloned().collect();
    if !scope.get("coreMarketIds").is_some_and(V::is_array) || core_set.len() != core_list.len()
        || core_list.iter().any(|v| !is_id(&json!(v)) || !markets.contains_key(v)) {
        return Err("Invalid scheduler core scope".into());
    }
    let market_ids: Vec<String> = markets.keys().cloned().collect();
    let mut sorted_core = core_list.clone();
    sorted_core.sort();
    let scope_hash = sha256(js_stringify(&json!({
        "tournamentId": universe["tournamentId"],
        "exchanges": market_ids.iter().map(|m| markets[m].clone()).collect::<Vec<_>>(),
        "coreMarketIds": sorted_core,
    })).as_bytes());
    Ok(Scope { market_ids, markets, core: core_set, scope_hash })
}

fn proposals(values: Option<&V>, scope: &Scope, label: &str) -> Result<Vec<String>, String> {
    let Some(v) = values else { return Ok(vec![]); };
    let arr = v.as_array().ok_or_else(|| format!("Invalid or foreign scheduler {}", label))?;
    if arr.iter().any(|x| !is_id(x) || !scope.markets.contains_key(x.as_str().unwrap())) {
        return Err(format!("Invalid or foreign scheduler {}", label));
    }
    Ok(unique(&arr.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>()))
}

fn checked_state(state: &V, scope: &Scope) -> Result<V, String> {
    if state.get("schemaVersion").and_then(V::as_i64) != Some(1)
        || state.get("scopeHash").and_then(V::as_str) != Some(scope.scope_hash.as_str()) {
        return Err("Scheduler state scope mismatch".into());
    }
    for kind in ["book", "history"] {
        let attempts = state.get(format!("{}Attempts", kind)).and_then(V::as_i64);
        let cursor = state.get(format!("{}Cursor", kind)).and_then(V::as_i64);
        if !attempts.is_some_and(|a| a >= 0)
            || !cursor.is_some_and(|c| c >= 0 && c < scope.market_ids.len() as i64) {
            return Err("Invalid scheduler cursor".into());
        }
        let last = state.get(format!("{}LastAttempt", kind));
        if !last.is_some_and(|l| l.is_object()) { return Err("Invalid scheduler attempt ledger".into()); }
        let mut ordinals = HashSet::new();
        for (mid, seq) in last.unwrap().as_object().unwrap() {
            if !scope.markets.contains_key(mid) || !seq.as_i64().is_some_and(|s| s >= 1 && s <= attempts.unwrap())
                || !ordinals.insert(seq.as_i64().unwrap()) {
                return Err("Invalid scheduler attempt ledger".into());
            }
        }
    }
    if let Some(clocks) = state.get("historyLastAttemptAt") {
        let last = state.get("historyLastAttempt").and_then(V::as_object);
        if !clocks.is_object() || clocks.as_object().unwrap().iter().any(|(mid, at)| {
            !scope.markets.contains_key(mid) || !last.is_some_and(|l| l.contains_key(mid)) || !canonical_clock(at)
        }) {
            return Err("Invalid scheduler attempt clock ledger".into());
        }
    }
    Ok(state.clone())
}

fn fair_order(ids: Vec<String>, state: &V, scope: &Scope, kind: &str) -> Vec<String> {
    let cursor = state.get(format!("{}Cursor", kind)).and_then(V::as_i64).unwrap_or(0) as usize;
    let last = state.get(format!("{}LastAttempt", kind)).and_then(V::as_object).cloned().unwrap_or_default();
    let n = scope.market_ids.len().max(1);
    let rank: HashMap<String, usize> = scope.market_ids.iter().enumerate()
        .map(|(i, m)| (m.clone(), (i + n - cursor % n) % n)).collect();
    let mut out = unique(&ids);
    out.sort_by_key(|m| (last.get(m).and_then(V::as_i64).unwrap_or(0), *rank.get(m).unwrap_or(&usize::MAX)));
    out
}

struct Inputs {
    priority_managed: Vec<String>,
    held: Vec<String>,
    pending: Vec<String>,
    candidate: Vec<String>,
    excluded: Vec<String>,
    usable: Vec<String>,
}
fn inputs_of(inputs: &V, scope: &Scope) -> Result<Inputs, String> {
    Ok(Inputs {
        priority_managed: proposals(inputs.get("priorityManagedMarketIds"), scope, "priorityManagedMarketIds")?,
        held: proposals(inputs.get("heldMarketIds"), scope, "heldMarketIds")?,
        pending: proposals(inputs.get("pendingMarketIds"), scope, "pendingMarketIds")?,
        candidate: proposals(inputs.get("candidateMarketIds"), scope, "candidateMarketIds")?,
        excluded: proposals(inputs.get("excludedMarketIds"), scope, "excludedMarketIds")?,
        usable: proposals(inputs.get("usableMarketIds"), scope, "usableMarketIds")?,
    })
}

pub fn create_scheduler_state(scope: &V) -> Result<V, String> {
    let s = scope_of(scope)?;
    Ok(json!({"schemaVersion": 1, "scopeHash": s.scope_hash, "bookAttempts": 0, "bookCursor": 0,
        "bookLastAttempt": {}, "historyAttempts": 0, "historyCursor": 0, "historyLastAttempt": {}}))
}
pub fn validate_scheduler_state(state: &V, scope: &V) -> Result<V, String> {
    checked_state(state, &scope_of(scope)?)
}

/// planBookReads(state, scope, inputs, {budget, managedCapacity, comparisonSlots})
pub fn plan_book_reads(state: &V, raw_scope: &V, inputs: &V, budget: i64, managed_capacity: i64, comparison_slots: i64) -> Result<V, String> {
    let scope = scope_of(raw_scope)?;
    let copy = checked_state(state, &scope)?;
    let values = inputs_of(inputs, &scope)?;
    if !(1..=4).contains(&budget) || !(0..budget).contains(&managed_capacity)
        || !(0..=budget).contains(&comparison_slots) {
        return Err("Invalid scheduler book budget or headroom".into());
    }
    let priority_managed = values.priority_managed.clone();
    let priority_managed_set: HashSet<&str> = priority_managed.iter().map(String::as_str).collect();
    let mut managed_ids = priority_managed.clone();
    managed_ids.extend(values.held.clone());
    managed_ids.extend(values.pending.clone());
    let managed_market_ids = unique(&managed_ids);
    let excluded: HashSet<&str> = values.excluded.iter().map(String::as_str).collect();
    let managed: HashSet<&str> = managed_market_ids.iter().map(String::as_str).collect();
    let mut reads: Vec<V> = Vec::new();
    let mut selected: HashSet<String> = HashSet::new();
    let add = |market_id: &str, reason: &str, reads: &mut Vec<V>, selected: &mut HashSet<String>| -> bool {
        if reads.len() as i64 >= budget || selected.contains(market_id) || excluded.contains(market_id) { return false; }
        selected.insert(market_id.to_string());
        let mut read = scope.markets[market_id].clone();
        read["path"] = json!(if scope.core.contains(market_id) { "sourceSnapshots" } else { "supplementalDepth" });
        read["reason"] = json!(reason);
        reads.push(read);
        true
    };
    for mid in fair_order(priority_managed.clone(), &copy, &scope, "book") {
        add(&mid, "priority_held_or_pending", &mut reads, &mut selected);
    }
    let rest: Vec<String> = managed_market_ids.iter().filter(|m| !priority_managed_set.contains(m.as_str())).cloned().collect();
    for mid in fair_order(rest, &copy, &scope, "book") {
        add(&mid, "held_or_pending", &mut reads, &mut selected);
    }
    let available_capacity = (managed_capacity - managed.len() as i64).max(0);
    let mut entry_market_ids: Vec<String> = Vec::new();
    let cand: Vec<String> = values.candidate.iter().filter(|m| !managed.contains(m.as_str())).cloned().collect();
    for mid in fair_order(cand, &copy, &scope, "book") {
        if entry_market_ids.len() as i64 >= available_capacity { break; }
        if add(&mid, "eligible_candidate", &mut reads, &mut selected) { entry_market_ids.push(mid); }
    }
    let mut comparison_count = 0;
    for mid in fair_order(scope.core.iter().cloned().collect(), &copy, &scope, "book") {
        if comparison_count >= comparison_slots { break; }
        if add(&mid, "comparison", &mut reads, &mut selected) { comparison_count += 1; }
    }
    let discovery: Vec<String> = scope.market_ids.iter().filter(|m| !scope.core.contains(*m)).cloned().collect();
    for mid in fair_order(discovery, &copy, &scope, "book") {
        add(&mid, "discovery", &mut reads, &mut selected);
    }
    Ok(json!({
        "state": copy, "reads": reads, "managedCapacity": managed_capacity,
        "managedMarketIds": managed_market_ids, "priorityManagedMarketIds": priority_managed,
        "availableCapacity": available_capacity, "entryMarketIds": entry_market_ids,
        "overCapacity": managed.len() as i64 > managed_capacity,
        "unserviceableManagedMarketIds": managed_market_ids.iter().filter(|m| excluded.contains(m.as_str())).collect::<Vec<_>>(),
        "deferredCandidateMarketIds": values.candidate.iter().filter(|m| !managed.contains(m.as_str()) && !entry_market_ids.contains(m)).collect::<Vec<_>>(),
    }))
}

struct Allocation { at: String, now: f64, rows: HashMap<String, V> }
fn history_allocation(raw: &V, scope: &Scope) -> Result<Allocation, String> {
    let err = "Invalid history allocation";
    if raw.get("schemaVersion").and_then(V::as_i64) != Some(1)
        || !canonical_clock(&raw.get("at").cloned().unwrap_or(V::Null))
        || !raw.get("markets").is_some_and(V::is_array)
        || raw.get("markets").and_then(V::as_array).map(|m| m.len()) != Some(scope.market_ids.len())
        || raw.as_object().map(|o| o.keys().any(|k| !["schemaVersion", "at", "markets"].contains(&k.as_str()))).unwrap_or(true) {
        return Err(err.into());
    }
    let now = parse_iso_ms(raw["at"].as_str().unwrap()).unwrap_or(f64::NAN);
    let fields = ["marketId", "referenceReady", "validUntil", "receivedAt", "identityKnown", "quotePlausible", "opportunityGapMicros"];
    let mut rows = HashMap::new();
    for row in raw["markets"].as_array().unwrap() {
        let bad_row = || "Invalid or foreign history allocation row".to_string();
        if !row.is_object() || row.as_object().unwrap().len() != fields.len()
            || row.as_object().unwrap().keys().any(|k| !fields.contains(&k.as_str())) {
            return Err(bad_row());
        }
        let mid = row["marketId"].as_str().ok_or_else(bad_row)?;
        if !scope.markets.contains_key(mid) || rows.contains_key(mid) { return Err(bad_row()); }
        for k in ["referenceReady", "identityKnown", "quotePlausible"] {
            if !row[k].is_boolean() { return Err(bad_row()); }
        }
        let received = &row["receivedAt"];
        if !received.is_null() && (!canonical_clock(received) || parse_iso_ms(received.as_str().unwrap()).unwrap() > now) { return Err(bad_row()); }
        let valid_until = &row["validUntil"];
        if !valid_until.is_null() && !canonical_clock(valid_until) { return Err(bad_row()); }
        let gap = &row["opportunityGapMicros"];
        if !gap.is_null() && !gap.as_i64().is_some_and(|g| g.abs() <= 1_000_000) { return Err(bad_row()); }
        rows.insert(mid.to_string(), row.clone());
    }
    Ok(Allocation { at: raw["at"].as_str().unwrap().to_string(), now, rows })
}

pub fn record_scheduler_attempts(state: &V, raw_scope: &V, book_market_ids: &[String], history_market_ids: &[String], attempted_at: Option<&str>) -> Result<V, String> {
    let scope = scope_of(raw_scope)?;
    let mut copy = checked_state(state, &scope)?;
    if let Some(at) = attempted_at {
        if !canonical_clock(&json!(at)) { return Err("Invalid scheduler attempt clock".into()); }
    }
    for (kind, attempted) in [("book", book_market_ids), ("history", history_market_ids)] {
        let ids = proposals(Some(&json!(attempted)), &scope, &format!("{} attempts", kind))?;
        if ids.len() != attempted.len() { return Err("Duplicate scheduler attempts".into()); }
        for market_id in ids {
            let sequence = copy.get(format!("{}Attempts", kind)).and_then(V::as_i64).unwrap_or(0) + 1;
            copy[format!("{}Attempts", kind)] = json!(sequence);
            copy[format!("{}LastAttempt", kind)][market_id.clone()] = json!(sequence);
            let idx = scope.market_ids.iter().position(|m| *m == market_id).unwrap();
            copy[format!("{}Cursor", kind)] = json!(((idx + 1) % scope.market_ids.len()) as i64);
            if kind == "history" {
                let obj = copy.as_object_mut().unwrap();
                let clock_map = obj.entry("historyLastAttemptAt".to_string()).or_insert(json!({}));
                match attempted_at {
                    Some(at) => { clock_map[market_id] = json!(at); }
                    None => { clock_map.as_object_mut().unwrap().remove(&market_id); }
                }
            }
        }
    }
    Ok(copy)
}

/// planHistoryReads(state, scope, inputs, {budget, activeSetLimit, discoveryEvery})
/// inputs.historyAllocation (optional V) selects the due-aware path.
pub fn plan_history_reads(state: &V, raw_scope: &V, inputs: &V, budget: i64, active_set_limit: i64, discovery_every: i64) -> Result<V, String> {
    let scope = scope_of(raw_scope)?;
    let copy = checked_state(state, &scope)?;
    let values = inputs_of(inputs, &scope)?;
    if !(1..=2).contains(&budget) || !(1..=8).contains(&active_set_limit) || !(2..=8).contains(&discovery_every) {
        return Err("Invalid scheduler history budget".into());
    }
    let excluded: HashSet<String> = values.excluded.iter().cloned().collect();
    let mut managed_ids = values.priority_managed.clone();
    managed_ids.extend(values.held.clone());
    managed_ids.extend(values.pending.clone());
    let managed = unique(&managed_ids);
    if let Some(raw_alloc) = inputs.get("historyAllocation") {
        return plan_allocated_history(copy, &scope, &values, raw_alloc, budget, active_set_limit, discovery_every, &excluded, &managed, raw_scope);
    }
    let mut priority = fair_order(values.priority_managed.clone(), &copy, &scope, "history");
    priority.extend(fair_order(managed.clone(), &copy, &scope, "history"));
    priority.extend(fair_order(values.candidate.clone(), &copy, &scope, "book"));
    priority.extend(scope.market_ids.iter().filter(|m| values.usable.contains(m)).cloned());
    let priority = unique(&priority);
    let priority: Vec<String> = priority.into_iter().filter(|m| !excluded.contains(m)).collect();
    let active_market_ids: Vec<String> = priority.iter().take(active_set_limit as usize).cloned().collect();
    let active: HashSet<&str> = active_market_ids.iter().map(String::as_str).collect();
    let mut reads = Vec::new();
    let mut chosen = HashSet::new();
    let mut simulated = copy.clone();
    for _ in 0..budget {
        let discover = active.is_empty()
            || (simulated["historyAttempts"].as_i64().unwrap_or(0) + 1) % discovery_every == 0;
        let discovery = fair_order(scope.market_ids.iter().filter(|m| !active.contains(m.as_str()) && !excluded.contains(*m)).cloned().collect(), &simulated, &scope, "history");
        let refresh = fair_order(active_market_ids.clone(), &simulated, &scope, "history");
        let ordered: Vec<String> = if discover { discovery.into_iter().chain(refresh).collect() } else { refresh.into_iter().chain(discovery).collect() };
        let Some(market_id) = ordered.iter().find(|m| !chosen.contains(*m)) else { break };
        chosen.insert(market_id.clone());
        let mut read = scope.markets[market_id].clone();
        read["reason"] = json!(if active.contains(market_id.as_str()) { "active_history_refresh" } else { "history_discovery" });
        reads.push(read);
        simulated = record_scheduler_attempts(&simulated, raw_scope, &[], &[market_id.clone()], None)?;
    }
    Ok(json!({"state": copy, "reads": reads, "activeMarketIds": active_market_ids,
        "deferredManagedMarketIds": managed.iter().filter(|m| !active.contains(m.as_str())).collect::<Vec<_>>()}))
}

fn plan_allocated_history(copy: V, scope: &Scope, values: &Inputs, raw: &V, budget: i64,
    active_set_limit: i64, discovery_every: i64, excluded: &HashSet<String>, managed: &[String], raw_scope: &V) -> Result<V, String> {
    let allocation = history_allocation(raw, scope)?;
    if copy.get("historyLastAttemptAt").and_then(V::as_object).map(|c| c.values().any(|at| {
        parse_iso_ms(at.as_str().unwrap_or("")).unwrap_or(f64::NEG_INFINITY) > allocation.now
    })).unwrap_or(false) {
        return Err("History allocation clock precedes attempt".into());
    }
    let chosen = std::cell::RefCell::new(HashSet::<String>::new());
    let due = |mid: &str| -> bool {
        let row = &allocation.rows[mid];
        !row["referenceReady"].as_bool().unwrap_or(false)
            || row["validUntil"].is_null()
            || parse_iso_ms(row["validUntil"].as_str().unwrap_or("")).unwrap_or(f64::INFINITY) <= allocation.now + 600000.0
    };
    let receipt_cooling = |mid: &str| -> bool {
        let row = &allocation.rows[mid];
        !row["referenceReady"].as_bool().unwrap_or(false) && !row["receivedAt"].is_null()
            && allocation.now - parse_iso_ms(row["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NEG_INFINITY) < 600000.0
    };
    let repeat_cooling = |mid: &str, state: &V| -> bool {
        let at = state.get("historyLastAttemptAt").and_then(|m| m.get(mid)).and_then(V::as_str);
        match at {
            None => false,
            Some(at) => state["historyAttempts"].as_i64().unwrap_or(0)
                - state["historyLastAttempt"][mid].as_i64().unwrap_or(0) < 8
                && allocation.now - parse_iso_ms(at).unwrap_or(f64::NEG_INFINITY) < 600000.0,
        }
    };
    let eligible = |mid: &str, state: &V| -> bool {
        !excluded.contains(mid) && !chosen.borrow().contains(mid) && due(mid) && !receipt_cooling(mid) && !repeat_cooling(mid, state)
    };
    let known_order = |mids: Vec<String>, state: &V| -> Vec<String> {
        let fair = fair_order(mids, state, scope, "history");
        let rank: HashMap<String, usize> = fair.iter().enumerate().map(|(i, m)| (m.clone(), i)).collect();
        let mut out = fair;
        out.sort_by(|a, b| {
            let (x, y) = (&allocation.rows[a], &allocation.rows[b]);
            let qp = (y["quotePlausible"].as_bool().unwrap_or(false) as i64)
                .cmp(&(x["quotePlausible"].as_bool().unwrap_or(false) as i64));
            if qp != std::cmp::Ordering::Equal { return qp; }
            if x["quotePlausible"].as_bool().unwrap_or(false) {
                let gx = x["opportunityGapMicros"].as_i64().unwrap_or(1_000_001);
                let gy = y["opportunityGapMicros"].as_i64().unwrap_or(1_000_001);
                let g = gx.cmp(&gy);
                if g != std::cmp::Ordering::Equal { return g; }
            }
            rank[a].cmp(&rank[b])
        });
        out
    };
    let known_due: Vec<String> = scope.market_ids.iter()
        .filter(|m| allocation.rows[*m]["identityKnown"].as_bool().unwrap_or(false) && due(m) && !excluded.contains(*m))
        .cloned().collect();
    let mut active_pool = fair_order(values.priority_managed.clone(), &copy, scope, "history");
    active_pool.extend(fair_order(managed.to_vec(), &copy, scope, "history"));
    active_pool.extend(fair_order(values.candidate.clone(), &copy, scope, "history"));
    active_pool.extend(known_order(known_due.clone(), &copy));
    let active_market_ids: Vec<String> = unique(&active_pool).into_iter()
        .filter(|m| !excluded.contains(m)).take(active_set_limit as usize).collect();
    let mut reads = Vec::new();
    let mut simulated = copy.clone();
    for _ in 0..budget {
        let global = fair_order(scope.market_ids.iter().filter(|m| eligible(m, &simulated)).cloned().collect(), &simulated, scope, "history");
        if global.is_empty() { break; }
        let reserved_discovery = (simulated["historyAttempts"].as_i64().unwrap_or(0) + 1) % discovery_every == 0;
        let mut groups: Vec<(Vec<String>, &str, i64)> = if reserved_discovery { vec![] } else { vec![
            (fair_order(values.priority_managed.clone(), &simulated, scope, "history"), "due_official_history", 0),
            (fair_order(managed.to_vec(), &simulated, scope, "history"), "due_managed_history", 1),
            (fair_order(values.candidate.clone(), &simulated, scope, "history"), "due_candidate_history", 2),
            (known_order(known_due.clone(), &simulated), "due_known_history", 3),
        ]};
        groups.push((global, "fair_history_discovery", 4));
        let mut selection: Option<(String, &str, i64)> = None;
        for (mids, reason, prio) in &groups {
            if let Some(mid) = mids.iter().find(|m| eligible(m, &simulated)) {
                selection = Some((mid.clone(), *reason, *prio));
                break;
            }
        }
        let Some((market_id, reason, priority)) = selection else { break };
        let row = allocation.rows[&market_id].clone();
        chosen.borrow_mut().insert(market_id.clone());
        let mut read = scope.markets[&market_id].clone();
        read["reason"] = json!(reason);
        read["priority"] = json!(priority);
        read["reservedDiscovery"] = json!(reserved_discovery);
        read["referenceReady"] = row["referenceReady"].clone();
        read["validUntil"] = row["validUntil"].clone();
        read["opportunityGapMicros"] = row["opportunityGapMicros"].clone();
        reads.push(read);
        simulated = record_scheduler_attempts(&simulated, raw_scope, &[], &[market_id], Some(&allocation.at))?;
    }
    let act_set: HashSet<&str> = active_market_ids.iter().map(String::as_str).collect();
    Ok(json!({"state": copy, "reads": reads, "activeMarketIds": active_market_ids,
        "deferredManagedMarketIds": managed.iter().filter(|m| !act_set.contains(m.as_str())).collect::<Vec<_>>(),
        "allocationMode": "due_aware_v1", "allocationAt": allocation.at,
        "dueMarketCount": scope.market_ids.iter().filter(|m| !excluded.contains(*m) && due(m)).count(),
        "receiptCooldownMarketIds": scope.market_ids.iter().filter(|m| !excluded.contains(*m) && due(m) && receipt_cooling(m)).collect::<Vec<_>>(),
        "attemptCooldownMarketIds": scope.market_ids.iter().filter(|m| !excluded.contains(*m) && due(m) && repeat_cooling(m, &copy)).collect::<Vec<_>>()}))
}
