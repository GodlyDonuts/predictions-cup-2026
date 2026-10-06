// execution/baal-scaled-shared-budget-v2.mjs — durable local allocation slot.
// The mandatory caller supplies the existing real account lease and
// observations; this file is not a distributed lock.
//
// Port notes:
// - `sync(fn,...)` is trivially satisfied: callbacks are synchronous Fn.
// - The actor file (controllerDir/baal-scaled-no-journal.jsonl) hash chain is
//   re-verified on every source() call: each row must round-trip
//   (JSON.stringify(row)===line => serde_json::to_string(parsed)==line) and the
//   stored hash covers sha256(JSON.stringify(body-without-hash)).
// - JS `stamp` = "ino:birthtimeMs"; kept as the same joined string.
use crate::baal_scaled_no_controller_v1::{baal_scaled_no_controller_identity, project_baal_scaled_reservation};
use crate::core::{canonical_payload, payload_hash};
use baal_service_runtime::fixed::{amount, instant, money, price};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use baal_service_runtime::ledger::{acquire_owner_lease, OwnerLease};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("BAAL_SHARED_BUDGET_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn sha(b: &[u8]) -> String {
    sha256(b)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn amount_v(v: &V) -> Result<i128, String> {
    amount(v.as_str().unwrap_or(""))
}
fn js_string_repr(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(_) | V::Bool(_) => js_stringify(v),
        V::Null => "null".to_string(),
        _ => "undefined".to_string(),
    }
}
fn same(a: &V, b: &V) -> bool {
    match (payload_hash(a), payload_hash(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}
fn stamp(ino: u64, birthtime_ms: f64) -> String {
    format!("{ino}:{birthtime_ms}")
}
fn stat_stamp(path: &Path) -> Result<(u64, f64, u64), String> {
    let s = std::fs::metadata(path).map_err(|e| e.to_string())?;
    Ok((
        s.ino(),
        s.created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0),
        s.len(),
    ))
}

// proof(p) — a scoped_rest_no_multishare_fifo_v1 self-hash.
fn proof(p: &V) -> Result<(), String> {
    let mut body = p.clone();
    let h = match &mut body {
        V::Object(m) => m.remove("verificationHash").unwrap_or(V::Null),
        _ => V::Null,
    };
    need(
        hex(&h)
            && payload_hash(&body)? == h.as_str().unwrap_or("")
            && get(p, "scopedTradeVerified") == &json!(true),
        "OWN_PROOF",
    )
}

// projection(state, scope, allocationHash)
fn projection(s: &V, scope: &V, allocation_hash: &str) -> Result<V, String> {
    need(
        get(s, "mode").as_str() == Some("baal_scaled_no_controller_v1")
            && same(get(s, "scope"), scope)
            && payload_hash(get(s, "allocation"))? == allocation_hash
            && get(s, "orders").is_array(),
        "ACTUAL_SCOPE",
    )?;
    let mut intents: HashSet<String> = HashSet::new();
    let mut keys: HashSet<String> = HashSet::new();
    let mut spent = BigInt::from(0);
    for o in get(s, "orders").as_array().cloned().unwrap_or_default() {
        let qty = js_int(get(get(&o, "body"), "quantity"));
        need(
            get(&o, "intentId").is_string()
                && !intents.contains(get(&o, "intentId").as_str().unwrap_or(""))
                && get(get(&o, "body"), "side").as_str() == Some("no")
                && ["buy", "sell"].contains(&get(get(&o, "body"), "action").as_str().unwrap_or(""))
                && qty.is_some()
                && qty.unwrap() >= 1
                && qty.unwrap() <= 100
                && get(get(&o, "body"), "tournamentId") == get(scope, "tournamentId")
                && get(scope, "exchanges")
                    .as_array()
                    .map(|a| {
                        a.iter().any(|m| {
                            get(m, "exchangeId") == get(get(&o, "body"), "exchangeId")
                                && get(m, "marketId") == get(&o, "marketId")
                        })
                    })
                    .unwrap_or(false)
                && get(&o, "bodyJson").as_str() == Some(canonical_payload(get(&o, "body"))?.as_str())
                && get(&o, "bodyHash").as_str() == Some(payload_hash(get(&o, "body"))?.as_str())
                && !keys.contains(get(get(&o, "body"), "idempotencyKey").as_str().unwrap_or(""))
                && BigInt::from(price(&js_string_repr(get(get(&o, "body"), "price")))?)
                    * qty.map(BigInt::from).unwrap_or_default()
                    <= amount_v(&{
                        let c = get(get(s, "allocation"), "maximumOrderCost");
                        if c.is_null() { json!("25.000000") } else { c.clone() }
                    })
                    .map(BigInt::from)?,
            "ORDER_OR_DUPLICATE",
        )?;
        intents.insert(get(&o, "intentId").as_str().unwrap_or("").to_string());
        keys.insert(get(get(&o, "body"), "idempotencyKey").as_str().unwrap_or("").to_string());
        if get(&o, "phase").as_str() == Some("abandoned") {
            need(
                get(&o, "proof").is_null()
                    && get(&o, "orderId").is_null()
                    && get(&o, "ack").is_null()
                    && o.get("operation").is_none()
                    && get(&o, "cancelPending") == &json!(false)
                    && get(&o, "fences").as_array().map(|a| a.is_empty()).unwrap_or(false)
                    && get(&o, "fetchStarts").as_array().map(|a| a.is_empty()).unwrap_or(false)
                    && get(&o, "receipts").as_array().map(|a| a.is_empty()).unwrap_or(false)
                    && instant_v(get(&o, "abandonedAt"))? >= instant_v(get(get(&o, "body"), "expirationDate"))?
                    && get(&o, "abandonedReason").as_str() == Some("expired_before_any_admission"),
                "ABANDONED_NOT_PROVEN_UNFETCHED",
            )?;
        }
        if !get(&o, "proof").is_null() {
            proof(get(&o, "proof"))?;
            need(
                get(&o, "phase").as_str() == Some("reconciled")
                    && get(get(&o, "proof"), "ownerId") == get(scope, "ownerId")
                    && get(get(&o, "proof"), "runId") == get(scope, "runId")
                    && get(get(&o, "proof"), "tournamentId") == get(scope, "tournamentId")
                    && get(get(&o, "proof"), "intentId") == get(&o, "intentId")
                    && get(get(&o, "proof"), "orderId") == get(&o, "orderId")
                    && get(get(&o, "proof"), "marketId") == get(&o, "marketId")
                    && get(get(&o, "proof"), "exchangeId") == get(get(&o, "body"), "exchangeId")
                    && get(get(&o, "proof"), "action") == get(get(&o, "body"), "action"),
                "ORDER_PROOF_BINDING",
            )?;
            spent += amount_v(get(get(&o, "proof"), "scopedPrincipalCost")).map(BigInt::from)?;
        }
    }
    if let V::Object(inv) = get(s, "inventory") {
        for (ex, slot) in inv {
            proof(get(slot, "proof"))?;
            need(
                get(get(slot, "proof"), "exchangeId").as_str() == Some(ex.as_str())
                    && get(s, "orders")
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .any(|o| !get(o, "proof").is_null() && same(get(o, "proof"), get(slot, "proof"))),
                "NO_OTHER_ACTOR_ADOPTION",
            )?;
        }
    }
    let record = project_baal_scaled_reservation(s)?;
    need(
        get(&record, "allocationHash").as_str() == Some(allocation_hash)
            && get(&record, "owned")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .all(|r| amount_v(get(r, "nominalCost")).map(BigInt::from).unwrap_or(BigInt::from(-1)) >= BigInt::from(0))
            && get(&record, "reservations")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .all(|r| amount_v(get(r, "maximumNominalCost")).map(BigInt::from).unwrap_or(BigInt::from(-1)) >= BigInt::from(0)),
        "NONNEGATIVE_FULL_RESERVE",
    )?;
    let pending: Vec<V> = get(s, "orders")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|o| get(o, "proof").is_null() && get(o, "phase").as_str() != Some("abandoned"))
        .collect();
    need(pending.len() <= 1, "SHADOW_PENDING")?;
    let unknown = pending.iter().any(|o| {
        ["uncertain", "semantic_mismatch"].contains(&get(o, "phase").as_str().unwrap_or(""))
            || get(o, "semanticMismatch") == &json!(true)
            || (get(o, "orderId").is_null()
                && !get(o, "fetchStarts").as_array().map(|a| a.is_empty()).unwrap_or(true))
    });
    Ok(json!({
        "record": record,
        "orders": get(s, "orders"),
        "spent": baal_paper::fixed::money(&spent),
        "unknown": unknown,
    }))
}

// forward(old, next) — monotonic intent/projection advance.
fn forward(old: Option<&V>, next: &V) -> Result<(), String> {
    let old = match old {
        Some(v) if !v.is_null() => v,
        _ => return Ok(()),
    };
    need(
        amount_v(get(next, "spent")).map(BigInt::from)? >= amount_v(get(old, "spent")).map(BigInt::from)?
            && get(next, "orders").as_array().map(|a| a.len()).unwrap_or(0)
                >= get(old, "orders").as_array().map(|a| a.len()).unwrap_or(0),
        "EXPENSE_OR_ORDER_RESET",
    )?;
    for o in get(old, "orders").as_array().cloned().unwrap_or_default() {
        let n = get(next, "orders")
            .as_array()
            .and_then(|a| a.iter().find(|n| get(n, "intentId") == get(&o, "intentId")))
            .cloned();
        need(
            n.is_some()
                && get(n.as_ref().unwrap(), "bodyHash") == get(&o, "bodyHash")
                && get(n.as_ref().unwrap(), "bodyJson") == get(&o, "bodyJson")
                && (get(&o, "orderId").is_null() || get(n.as_ref().unwrap(), "orderId") == get(&o, "orderId"))
                && (get(&o, "proof").is_null() || same(get(n.as_ref().unwrap(), "proof"), get(&o, "proof")))
                && (get(&o, "phase").as_str() != Some("abandoned")
                    || get(n.as_ref().unwrap(), "phase").as_str() == Some("abandoned"))
                && get(&o, "fetchStarts")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .all(|f| {
                        get(n.as_ref().unwrap(), "fetchStarts")
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                            .iter()
                            .any(|g| same(f, g))
                    }),
            "ORIGINAL_INTENT_FILL_RESET",
        )?;
    }
    Ok(())
}

// totalRisk(observed, record, allocation, {allowFundingShortfall})
fn total_risk(
    observed: &V,
    record: &V,
    allocation: &V,
    allow_funding_shortfall: bool,
) -> Result<V, String> {
    let mut out_owned = get(record, "owned").as_array().cloned().unwrap_or_default();
    let mut out_res = get(record, "reservations").as_array().cloned().unwrap_or_default();
    let mut out_loans: Vec<V> = Vec::new();
    let mut ids: HashSet<String> = HashSet::new();
    for r in out_owned.iter().chain(out_res.iter()) {
        ids.insert(get(r, "exposureId").as_str().unwrap_or("").to_string());
    }
    need(
        ids.len() == out_owned.len() + out_res.len(),
        "DUPLICATE_OWN_EXPOSURE",
    )?;
    let run_id = get(record, "runId").as_str().unwrap_or("").to_string();
    for (k, f) in [
        ("owned", "nominalCost"),
        ("reservations", "maximumNominalCost"),
        ("loans", "fullNominalPrincipal"),
    ] {
        need(get(observed, k).is_array(), "OBSERVED_EXPOSURES")?;
        for r in get(observed, k).as_array().cloned().unwrap_or_default() {
            need(
                get(&r, "exposureId").as_str().is_some_and(|s| !s.is_empty())
                    && get(&r, "marketId").is_string()
                    && get(&r, "eventId").as_str().is_some_and(|s| !s.is_empty())
                    && amount_v(get(&r, f)).map(BigInt::from)? >= BigInt::from(0),
                "OBSERVED_EXPOSURE",
            )?;
            let ex_id = get(&r, "exposureId").as_str().unwrap_or("").to_string();
            if ids.contains(&ex_id) {
                let present = match k {
                    "owned" => out_owned.iter().any(|v| same(v, &r)),
                    "reservations" => out_res.iter().any(|v| same(v, &r)),
                    _ => out_loans.iter().any(|v| same(v, &r)),
                };
                need(present, "SHADOW_EXPOSURE")?;
                continue;
            }
            need(
                !ex_id.starts_with(&format!("scaled:{run_id}:")),
                "UNMATCHED_OWN_SHADOW",
            )?;
            ids.insert(ex_id);
            match k {
                "owned" => out_owned.push(r),
                "reservations" => out_res.push(r),
                _ => out_loans.push(r),
            }
        }
    }
    let mut nominal = BigInt::from(0);
    let mut reserved = BigInt::from(0);
    let mut markets: Vec<(String, BigInt)> = Vec::new();
    let mut events: Vec<(String, BigInt)> = Vec::new();
    let mut market_events: Vec<(String, V)> = Vec::new();
    for (k, f) in [
        ("owned", "nominalCost"),
        ("reservations", "maximumNominalCost"),
        ("loans", "fullNominalPrincipal"),
    ] {
        let list = match k {
            "owned" => &out_owned,
            "reservations" => &out_res,
            _ => &out_loans,
        };
        for r in list {
            let n = amount_v(get(r, f)).map(BigInt::from)?;
            nominal += n.clone();
            if k == "reservations" {
                reserved += n.clone();
            }
            let mk = get(r, "marketId").as_str().unwrap_or("").to_string();
            let ev = get(r, "eventId").clone();
            if let Some((_, prev)) = market_events.iter().find(|(m, _)| m == &mk) {
                need(prev == &ev, "EVENT_MAPPING_CONFLICT")?;
            }
            market_events.push((mk.clone(), ev.clone()));
            if let Some(e) = markets.iter_mut().find(|(m, _)| m == &mk) {
                e.1 += n.clone();
            } else {
                markets.push((mk, n.clone()));
            }
            let ev_key = ev.as_str().unwrap_or("").to_string();
            if let Some(e) = events.iter_mut().find(|(m, _)| m == &ev_key) {
                e.1 += n;
            } else {
                events.push((ev_key, n));
            }
        }
    }
    let max_markets = get(allocation, "maximumActiveMarkets");
    let max_markets = if max_markets.is_null() { 20 } else { js_int(max_markets).unwrap_or(20) };
    let max_market_cost = amount_v(&{
        let c = get(allocation, "maximumMarketCost");
        if c.is_null() { json!("100.000000") } else { c.clone() }
    })?;
    let max_event_cost = amount_v(&{
        let c = get(allocation, "maximumEventCost");
        if c.is_null() { json!("200.000000") } else { c.clone() }
    })?;
    need(
        markets.iter().filter(|(_, v)| *v > BigInt::from(0)).count() as i64 <= max_markets,
        "ACTIVE_MARKET_CEILING",
    )?;
    need(
        nominal <= BigInt::from(1_000_000_000i64)
            && markets.iter().all(|(_, v)| *v <= BigInt::from(max_market_cost))
            && events.iter().all(|(_, v)| *v <= BigInt::from(max_event_cost)),
        "NOMINAL_LIMIT",
    )?;
    let cash_lower = amount_v(get(observed, "cashLower"))?;
    need(
        cash_lower >= 0
            && (allow_funding_shortfall || reserved <= BigInt::from(cash_lower)),
        "FULL_CASH_RESERVATION",
    )?;
    let markets_obj: serde_json::Map<String, V> = markets
        .iter()
        .map(|(k, v)| (k.clone(), json!(baal_paper::fixed::money(v))))
        .collect();
    let events_obj: serde_json::Map<String, V> = events
        .iter()
        .map(|(k, v)| (k.clone(), json!(baal_paper::fixed::money(v))))
        .collect();
    Ok(json!({
        "owned": out_owned,
        "reservations": out_res,
        "loans": out_loans,
        "fundingSatisfied": reserved <= BigInt::from(cash_lower),
        "nominalExposure": baal_paper::fixed::money(&nominal),
        "reservedCash": baal_paper::fixed::money(&reserved),
        "markets": markets_obj,
        "events": events_obj,
    }))
}

pub struct BaalScaledSharedBudgetV2Deps {
    pub assert_account_lease: Rc<dyn Fn() -> V>,
    pub get_controller_source: Rc<dyn Fn() -> Result<V, String>>,
    pub observe_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct BudgetInnerV2 {
    scope: V,
    allocation: V,
    allocation_hash: String,
    budget_installed_hash: String,
    controller_code_identity_hash: String,
    controller_dir: PathBuf,
    file: PathBuf,
    actor_file: PathBuf,
    journal: Journal,
    state: Option<V>,
    closed: bool,
    failed: bool,
    size: u64,
    bytes_hash: String,
    ino: u64,
    birthtime_ms: f64,
    lease: OwnerLease,
    deps: BaalScaledSharedBudgetV2Deps,
    source_cache: Option<(String, V)>,
    cache_hits: i64,
    cache_misses: i64,
}

pub struct BaalScaledSharedBudgetV2 {
    inner: Rc<RefCell<BudgetInnerV2>>,
}

fn basic(inner: &Rc<RefCell<BudgetInnerV2>>) -> Result<String, String> {
    let (closed, failed, can_operate, assert_lease, scope) = {
        let g = inner.borrow();
        (
            g.closed,
            g.failed,
            g.deps.can_operate.clone(),
            g.deps.assert_account_lease.clone(),
            g.scope.clone(),
        )
    };
    need(!closed && !failed && can_operate(), "STOPPED")?;
    inner.borrow().lease.assert_ownership().map_err(|e| e.message)?;
    let l = assert_lease();
    need(
        get(&l, "ownerId") == get(&scope, "ownerId")
            && get(&l, "tournamentId") == get(&scope, "tournamentId")
            && get(&l, "leaseId").as_str().is_some_and(|s| !s.is_empty()),
        "ACTUAL_ACCOUNT_LEASE",
    )?;
    payload_hash(&l)
}

fn observed(inner: &Rc<RefCell<BudgetInnerV2>>) -> Result<V, String> {
    let (f, scope, now) = {
        let g = inner.borrow();
        (
            g.deps.observe_global_risk.clone(),
            g.scope.clone(),
            g.deps.now.clone(),
        )
    };
    let g = f()?;
    need(
        get(&g, "ownerId") == get(&scope, "ownerId")
            && get(&g, "runId") == get(&scope, "runId")
            && get(&g, "tournamentId") == get(&scope, "tournamentId")
            && get(&g, "complete") == &json!(true)
            && get(&g, "eventScopeComplete") == &json!(true)
            && get(&g, "soleCollector") == &json!(true)
            && get(&g, "stopped") == &json!(false)
            && hex(get(&g, "sourceHash"))
            && instant_v(get(&g, "at"))? <= now() as i64
            && now() as i64 - instant_v(get(&g, "at"))? <= 20000
            && get(&g, "unresolvedIntentIds").is_array()
            && get(&g, "unknownAcceptance").is_boolean(),
        "FRESH_ACTUAL_GLOBAL_OBSERVATION",
    )?;
    Ok(g)
}

// source() — verify the controller journal end to end and project it.
fn source(inner: &Rc<RefCell<BudgetInnerV2>>) -> Result<V, String> {
    basic(inner)?;
    let (actor_file, scope, alloc_hash, code_hash, budget_hash, state, controller_dir) = {
        let g = inner.borrow();
        (
            g.actor_file.clone(),
            g.scope.clone(),
            g.allocation_hash.clone(),
            g.controller_code_identity_hash.clone(),
            g.budget_installed_hash.clone(),
            g.state.clone(),
            g.controller_dir.clone(),
        )
    };
    need(
        payload_hash(&baal_scaled_no_controller_identity()?)? == code_hash,
        "ACTUAL_CONTROLLER_CODE_CHANGED",
    )?;
    let hint = (inner.borrow().deps.get_controller_source)()?;
    need(
        hint.get("journalFile")
            .and_then(V::as_str)
            .map(|f| std::fs::canonicalize(f).unwrap_or_else(|_| PathBuf::from(f)))
            .unwrap_or_else(|| actor_file.clone())
            == std::fs::canonicalize(&actor_file).unwrap_or_else(|_| actor_file.clone()),
        "FIXED_ACTOR_PATH",
    )?;
    let (before_ino, before_birth, _before_len) = stat_stamp(&actor_file)?;
    let bytes = std::fs::read(&actor_file).map_err(|e| e.to_string())?;
    need(
        !bytes.is_empty() && bytes.len() <= 64 * 1024 * 1024 && *bytes.last().unwrap() == 10,
        "ACTOR_PARTIAL_OR_SIZE",
    )?;
    if let Some(st) = &state {
        let anchor = get(st, "actorAnchor");
        if !anchor.is_null() {
            need(
                bytes.len() as u64 >= anchor.get("bytes").and_then(V::as_u64).unwrap_or(u64::MAX)
                    && sha(&bytes[..anchor.get("bytes").and_then(V::as_u64).unwrap_or(0) as usize])
                        == get(anchor, "sha256").as_str().unwrap_or(""),
                "ACTOR_PREFIX_RESET",
            )?;
        }
    }
    let actor_sha = sha(&bytes);
    let cache_key = payload_hash(&json!({
        "actorSha": actor_sha,
        "bytes": bytes.len(),
        "actorIdentity": stamp(before_ino, before_birth),
        "controllerCodeIdentityHash": code_hash,
        "budgetInstalledHash": budget_hash,
        "allocationHash": alloc_hash,
        "scope": scope,
        "controllerDir": normalize_path(&controller_dir).to_string_lossy(),
    }))?;
    let mut seq: u64 = 0;
    let mut previous = String::new();
    let mut last: Option<V> = None;
    let mut opening: Option<V> = None;
    let mut p: V = V::Null;
    let cached = inner
        .borrow()
        .source_cache
        .as_ref()
        .filter(|(k, _)| k == &cache_key)
        .map(|(_, v)| v.clone());
    if let Some(c) = cached {
        inner.borrow_mut().cache_hits += 1;
        seq = get(&c, "seq").as_u64().unwrap_or(0);
        previous = get(&c, "previous").as_str().unwrap_or("").to_string();
        last = c.get("last").cloned();
        opening = c.get("opening").cloned();
        p = get(&c, "p").clone();
    } else {
        inner.borrow_mut().cache_misses += 1;
        for line in String::from_utf8_lossy(&bytes).split('\n').filter(|l| !l.is_empty()) {
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            let hash = get(&row, "hash").as_str().unwrap_or("").to_string();
            let mut body = row.clone();
            if let V::Object(m) = &mut body {
                m.remove("hash");
            }
            seq += 1;
            need(
                get(&body, "sequence").as_u64() == Some(seq)
                    && get(&body, "previous").as_str() == Some(previous.as_str())
                    && sha(serde_json::to_string(&body).map_err(|e| e.to_string())?.as_bytes()) == hash
                    && serde_json::to_string(&row).map_err(|e| e.to_string())? == line
                    && payload_hash(get(get(&body, "event"), "codeIdentity"))? == code_hash,
                "ACTOR_CHAIN_AND_CODE",
            )?;
            instant_v(get(&body, "at"))?;
            previous = hash.clone();
            if seq == 1 {
                opening = Some(json!({
                    "bytes": line.len() + 1,
                    "sha256": sha(format!("{line}\n").as_bytes()),
                    "sequence": 1,
                    "hash": hash,
                }));
            }
            last = Some(body);
        }
        need(
            get(get(get(&last.clone().unwrap_or(V::Null), "state"), "activation"), "budgetInstalledHash").as_str()
                == Some(budget_hash.as_str()),
            "ACTOR_BUDGET_BINDING",
        )?;
        p = projection(get(&last.clone().unwrap_or(V::Null), "state"), &scope, &alloc_hash)?;
    }
    let (after_ino, after_birth, after_len) = stat_stamp(&actor_file)?;
    need(
        stamp(after_ino, after_birth) == stamp(before_ino, before_birth)
            && after_len == bytes.len() as u64,
        "ACTOR_CHANGED",
    )?;
    if let Some(st) = &state {
        if !get(st, "controllerRootAnchor").is_null() {
            need(
                same(&opening.clone().unwrap_or(V::Null), get(st, "controllerRootAnchor")),
                "ORIGINAL_ROOT_PREFIX_CHANGED",
            )?;
        }
    }
    if hint.get("rootPrefix").is_some_and(|r| !r.is_null()) {
        need(
            same(get(&hint, "rootPrefix"), &opening.clone().unwrap_or(V::Null)),
            "CALLER_ROOT_PREFIX",
        )?;
    }
    need(
        get(get(get(&last.clone().unwrap_or(V::Null), "state"), "activation"), "budgetInstalledHash").as_str()
            == Some(budget_hash.as_str()),
        "ACTOR_BUDGET_BINDING",
    )?;
    let p = projection(get(&last.clone().unwrap_or(V::Null), "state"), &scope, &alloc_hash)?;
    need(
        get(get(get(&last.clone().unwrap_or(V::Null), "state"), "activation"), "budgetInstalledHash").as_str()
            == Some(budget_hash.as_str()),
        "ACTOR_BUDGET_BINDING",
    )?;
    need(
        hint.get("record").is_some_and(|r| !r.is_null()) && same(get(&hint, "record"), get(&p, "record")),
        "CALLER_SHADOW_RECORD",
    )?;
    need(
        get(&hint, "sequence").as_u64() == Some(seq) && get(&hint, "hash").as_str() == Some(previous.as_str()),
        "CALLER_ACTOR_FRONTIER",
    )?;
    forward(
        state.as_ref().map(|s| get(s, "actorProjection")),
        &p,
    )?;
    inner.borrow_mut().source_cache = Some((
        cache_key,
        json!({
            "seq": seq,
            "previous": previous,
            "last": last,
            "opening": opening,
            "p": p,
        }),
    ));
    let mut out = p.clone();
    if let V::Object(m) = &mut out {
        m.insert(
            "anchor".into(),
            json!({
                "bytes": bytes.len(),
                "sha256": actor_sha,
                "sequence": seq,
                "hash": previous,
            }),
        );
        m.insert("opening".into(), opening.unwrap_or(V::Null));
    }
    Ok(out)
}

fn normalize_path(p: &Path) -> PathBuf {
    // JS path.resolve on an already-absolute path: lexical normalization.
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// openBaalScaledSharedBudgetV2({dir, controllerDir, allocation, scope,
/// budgetInstalledHash, controllerCodeIdentityHash, assertAccountLease,
/// getControllerSource, observeGlobalRisk, canOperate, now})
pub fn open_baal_scaled_shared_budget_v2(
    dir: &Path,
    controller_dir: &Path,
    allocation: &V,
    scope: &V,
    budget_installed_hash: &str,
    controller_code_identity_hash: &str,
    deps: BaalScaledSharedBudgetV2Deps,
) -> Result<BaalScaledSharedBudgetV2, String> {
    need(
        dir.is_absolute()
            && controller_dir.is_absolute()
            && normalize_path(dir) != normalize_path(controller_dir)
            && hex(&json!(budget_installed_hash))
            && hex(&json!(controller_code_identity_hash)),
        "DEPENDENCIES",
    )?;
    let allocation_hash = payload_hash(allocation)?;
    need(
        get(allocation, "schemaVersion") == &json!(1)
            && get(allocation, "ownerId").as_str() == Some("Codex")
            && get(allocation, "runId") == get(scope, "runId")
            && get(allocation, "tournamentId") == get(scope, "tournamentId")
            && get(allocation, "capital").as_str() == Some("1000.000000")
            && hex(get(allocation, "humanAllocationSourceHash"))
            && get(scope, "ownerId").as_str() == Some("Codex")
            && get(scope, "exchanges").is_array(),
        "ALLOCATION",
    )?;
    for (k, cap) in [
        ("maximumOrderCost", 25_000_000i128),
        ("maximumMarketCost", 100_000_000i128),
        ("maximumEventCost", 200_000_000i128),
    ] {
        let v = allocation
            .get(k)
            .filter(|x| !x.is_null())
            .cloned()
            .unwrap_or(json!(money(cap)));
        let x = amount_v(&v)?;
        need(x > 0 && x <= cap, "ALLOCATION_CEILING")?;
    }
    let active_markets = allocation
        .get("maximumActiveMarkets")
        .filter(|x| !x.is_null())
        .cloned()
        .unwrap_or(json!(20));
    need(
        js_int(&active_markets).is_some_and(|n| n >= 1 && n <= 20),
        "ACTIVE_MARKET_CEILING",
    )?;
    let shares = allocation
        .get("maximumSharesPerOrder")
        .filter(|x| !x.is_null())
        .cloned()
        .unwrap_or(json!(100));
    need(
        js_int(&shares).is_some_and(|n| n >= 1 && n <= 100)
            && instant_v(get(allocation, "recordedAt"))? <= (deps.now)() as i64,
        "ALLOCATION_CLOCK_OR_SHARES",
    )?;
    let mut lease = acquire_owner_lease(
        dir,
        get(scope, "ownerId").as_str().unwrap_or(""),
        get(scope, "runId").as_str().unwrap_or(""),
        "baal-scaled-shared-budget",
        (deps.now)(),
    )
    .map_err(|e| e.message)?;
    let file = dir.join("baal-scaled-shared-budget.jsonl");
    let actor_file = controller_dir.join("baal-scaled-no-journal.jsonl");
    let controller_dir_resolved = normalize_path(controller_dir);
    let open_result: Result<(Journal, Option<V>, u64, String, u64, f64), String> = (|| {
        // basic(): not closed/failed at open, so only canOperate + lease checks.
        need((deps.can_operate)(), "STOPPED")?;
        lease.assert_ownership().map_err(|e| e.message)?;
        let l = (deps.assert_account_lease)();
        need(
            get(&l, "ownerId") == get(scope, "ownerId")
                && get(&l, "tournamentId") == get(scope, "tournamentId")
                && get(&l, "leaseId").as_str().is_some_and(|s| !s.is_empty()),
            "ACTUAL_ACCOUNT_LEASE",
        )?;
        if !file.exists() {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&file)
                .map_err(|e| e.to_string())?;
        }
        let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
        need(bytes.is_empty() || *bytes.last().unwrap() == 10, "PARTIAL_BUDGET")?;
        let journal = Journal::open(&file)?;
        let mut state: Option<V> = None;
        for line in String::from_utf8_lossy(&bytes).split('\n').filter(|l| !l.is_empty()) {
            let r: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            need(
                get(get(&r, "state"), "allocationHash").as_str() == Some(allocation_hash.as_str())
                    && get(get(&r, "state"), "budgetInstalledHash").as_str() == Some(budget_installed_hash)
                    && get(get(&r, "state"), "controllerCodeIdentityHash").as_str() == Some(controller_code_identity_hash)
                    && get(get(&r, "state"), "controllerDir").as_str()
                        == Some(controller_dir_resolved.to_string_lossy().as_ref())
                    && same(get(get(&r, "state"), "scope"), scope),
                "BUDGET_SLOT_CHANGED",
            )?;
            if let Some(prev) = &state {
                forward(
                    Some(get(prev, "actorProjection")),
                    get(get(&r, "state"), "actorProjection"),
                )?;
            }
            state = Some(get(&r, "state").clone());
        }
        let size = bytes.len() as u64;
        let bytes_hash = sha(&bytes);
        let (ino, birth, _) = stat_stamp(&file)?;
        Ok((journal, state, size, bytes_hash, ino, birth))
    })();
    match open_result {
        Ok((journal, state, size, bytes_hash, ino, birth)) => Ok(BaalScaledSharedBudgetV2 {
            inner: Rc::new(RefCell::new(BudgetInnerV2 {
                scope: scope.clone(),
                allocation: allocation.clone(),
                allocation_hash,
                budget_installed_hash: budget_installed_hash.to_string(),
                controller_code_identity_hash: controller_code_identity_hash.to_string(),
                controller_dir: controller_dir_resolved,
                file,
                actor_file,
                journal,
                state,
                closed: false,
                failed: false,
                size,
                bytes_hash,
                ino,
                birthtime_ms: birth,
                lease,
                deps,
                source_cache: None,
                cache_hits: 0,
                cache_misses: 0,
            })),
        }),
        Err(e) => {
            let _ = lease.release();
            Err(e)
        }
    }
}

fn append(inner: &Rc<RefCell<BudgetInnerV2>>, next: &V) -> Result<(), String> {
    basic(inner)?;
    let (file, ino, birth, size, bytes_hash, alloc_hash, budget_hash) = {
        let g = inner.borrow();
        (
            g.file.clone(),
            g.ino,
            g.birthtime_ms,
            g.size,
            g.bytes_hash.clone(),
            g.allocation_hash.clone(),
            g.budget_installed_hash.clone(),
        )
    };
    let before = std::fs::read(&file).map_err(|e| e.to_string())?;
    let (ino2, birth2, _) = stat_stamp(&file)?;
    need(
        stamp(ino2, birth2) == stamp(ino, birth)
            && before.len() as u64 == size
            && sha(&before) == bytes_hash,
        "BUDGET_RESET",
    )?;
    let commit = {
        let mut g = inner.borrow_mut();
        g.journal.commit(
            next,
            &json!({
                "kind": "source_bound_nominal_budget",
                "allocationHash": alloc_hash,
                "budgetInstalledHash": budget_hash,
            }),
        )
    };
    match commit {
        Ok(_) => {
            basic(inner)?;
            let after = std::fs::read(&file).map_err(|e| e.to_string())?;
            need(
                after.len() >= before.len() && after[..before.len()] == before[..],
                "BUDGET_PREFIX_CHANGED",
            )?;
            let mut g = inner.borrow_mut();
            g.state = Some(next.clone());
            g.size = after.len() as u64;
            g.bytes_hash = sha(&after);
            Ok(())
        }
        Err(e) => {
            inner.borrow_mut().failed = true;
            Err(e)
        }
    }
}

impl BaalScaledSharedBudgetV2 {
    /// installFundingReservation(record)
    pub fn install_funding_reservation(&self, record: &V) -> Result<V, String> {
        let account_lease_hash = basic(&self.inner)?;
        let s = source(&self.inner)?;
        need(same(record, get(&s, "record")), "NOT_ACTUAL_CONTROLLER_RECORD")?;
        {
            let g = self.inner.borrow();
            if g.state.is_none() {
                need(
                    get(&s, "orders").as_array().map(|a| a.is_empty()).unwrap_or(false)
                        && get(get(&s, "record"), "owned").as_array().map(|a| a.is_empty()).unwrap_or(false)
                        && get(get(&s, "record"), "reservations").as_array().map(|a| a.is_empty()).unwrap_or(false)
                        && get(&s, "unknown") == &json!(false),
                    "NO_PREEXISTING_ACTOR_ADOPTION",
                )?;
            }
        }
        let g = observed(&self.inner)?;
        let risk = total_risk(&g, get(&s, "record"), &self.inner.borrow().allocation, false)?;
        let record_unresolved: Vec<V> = get(get(&s, "record"), "unresolvedIntentIds")
            .as_array()
            .cloned()
            .unwrap_or_default();
        let external: Vec<V> = get(&g, "unresolvedIntentIds")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|x| !record_unresolved.contains(x))
            .collect();
        need(
            external.is_empty()
                && !get(&g, "owned").as_array().cloned().unwrap_or_default().iter().any(|r| get(r, "unknownAcceptance") == &json!(true))
                && !get(&g, "loans").as_array().cloned().unwrap_or_default().iter().any(|r| get(r, "unknownAcceptance") == &json!(true)),
            "OTHER_UNKNOWN",
        )?;
        let at = iso_of((self.inner.borrow().deps.now)());
        let (scope, alloc_hash, budget_hash, code_hash, controller_dir, journal_prev) = {
            let g = self.inner.borrow();
            (
                g.scope.clone(),
                g.allocation_hash.clone(),
                g.budget_installed_hash.clone(),
                g.controller_code_identity_hash.clone(),
                g.controller_dir.clone(),
                g.journal.previous.clone(),
            )
        };
        let next = json!({
            "schemaVersion": 1,
            "mode": "baal_scaled_shared_budget_v1",
            "scope": scope,
            "allocationHash": alloc_hash,
            "budgetInstalledHash": budget_hash,
            "controllerCodeIdentityHash": code_hash,
            "controllerDir": controller_dir.to_string_lossy(),
            "controllerRootAnchor": get(&s, "opening"),
            "actorAnchor": get(&s, "anchor"),
            "actorProjection": {
                "record": get(&s, "record"),
                "orders": get(&s, "orders"),
                "spent": get(&s, "spent"),
                "unknown": get(&s, "unknown"),
            },
            "risk": risk,
            "externalObservation": g,
            "accountLeaseHash": account_lease_hash,
            "cumulativeActualExpenses": get(&s, "spent"),
            "unknownAcceptance": get(&s, "unknown") == &json!(true) || get(&g, "unknownAcceptance") == &json!(true),
            "at": at,
        });
        append(&self.inner, &next)?;
        Ok(json!({
            "reservationHash": get(get(&s, "record"), "reservationHash"),
            "allocationHash": alloc_hash,
            "globalLedgerHash": journal_prev,
            "confirmedAt": at,
        }))
    }

    /// globalRisk() / getGlobalRisk()
    pub fn global_risk(&self) -> Result<V, String> {
        basic(&self.inner)?;
        need(self.inner.borrow().state.is_some(), "BUDGET_NOT_INSTALLED")?;
        let s = source(&self.inner)?;
        let g = observed(&self.inner)?;
        let state = self.inner.borrow().state.clone().unwrap_or(V::Null);
        need(
            get(get(&s, "record"), "reservationHash")
                == get(get(get(&state, "actorProjection"), "record"), "reservationHash"),
            "CURRENT_RESERVE_NOT_INSTALLED",
        )?;
        let r = total_risk(&g, get(&s, "record"), &self.inner.borrow().allocation, true)?;
        let mut unresolved: Vec<V> = Vec::new();
        for x in get(get(&s, "record"), "unresolvedIntentIds")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .chain(get(&g, "unresolvedIntentIds").as_array().cloned().unwrap_or_default())
        {
            if !unresolved.contains(&x) {
                unresolved.push(x);
            }
        }
        let (alloc_hash, budget_hash, journal_prev) = {
            let g2 = self.inner.borrow();
            (
                g2.allocation_hash.clone(),
                g2.budget_installed_hash.clone(),
                g2.journal.previous.clone(),
            )
        };
        let rows: Vec<V> = get(&r, "owned")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .chain(get(&r, "reservations").as_array().cloned().unwrap_or_default())
            .chain(get(&r, "loans").as_array().cloned().unwrap_or_default())
            .collect();
        let unknown = get(&s, "unknown") == &json!(true)
            || get(&g, "unknownAcceptance") == &json!(true)
            || rows.iter().any(|v| get(v, "unknownAcceptance") == &json!(true));
        let mut out = g.clone();
        if let (V::Object(m), V::Object(rv)) = (&mut out, r) {
            for (k, v) in rv {
                m.insert(k.clone(), v);
            }
            m.insert("allocationHash".into(), json!(alloc_hash));
            m.insert("budgetInstalledHash".into(), json!(budget_hash));
            m.insert("controllerReservationHash".into(), get(get(&s, "record"), "reservationHash").clone());
            m.insert("globalLedgerHash".into(), json!(journal_prev));
            m.insert(
                "sourceHash".into(),
                json!(payload_hash(&json!({
                    "observation": get(&g, "sourceHash"),
                    "actorAnchor": get(&s, "anchor"),
                    "reservation": get(get(&s, "record"), "reservationHash"),
                    "globalLedgerHash": journal_prev,
                }))?),
            );
            m.insert("unknownAcceptance".into(), json!(unknown));
            m.insert("unresolvedIntentIds".into(), V::Array(unresolved));
            m.insert("loansDiscounted".into(), json!(false));
            m.insert("executionAuthority".into(), json!(false));
        }
        Ok(out)
    }

    /// status()
    pub fn status(&self) -> V {
        let g = self.inner.borrow();
        let mut unknown = g
            .state
            .as_ref()
            .map(|s| get(s, "unknownAcceptance").clone())
            .unwrap_or(json!(false));
        let _ = &mut unknown;
        let mut u = unknown == json!(true);
        drop(g);
        if self.inner.borrow().state.is_some() && !self.inner.borrow().closed {
            match source(&self.inner) {
                Ok(s) => u = u || get(&s, "unknown") == &json!(true),
                Err(_) => u = true,
            }
        }
        let g = self.inner.borrow();
        json!({
            "mode": "baal_scaled_shared_budget_v1",
            "cacheHits": g.cache_hits,
            "cacheMisses": g.cache_misses,
            "allocationHash": g.allocation_hash,
            "budgetInstalledHash": g.budget_installed_hash,
            "controllerCodeIdentityHash": g.controller_code_identity_hash,
            "claimed": g.state.is_some(),
            "globalLedgerHash": g.journal.previous,
            "sequence": g.journal.sequence,
            "controllerReservationHash": g.state.as_ref().map(|s| get(get(get(s, "actorProjection"), "record"), "reservationHash").clone()).unwrap_or(V::Null),
            "cumulativeActualExpenses": g.state.as_ref().map(|s| get(s, "cumulativeActualExpenses").clone()).unwrap_or(json!("0.000000")),
            "risk": g.state.as_ref().map(|s| get(s, "risk").clone()).unwrap_or(V::Null),
            "unknownAcceptance": u,
            "actorAnchor": g.state.as_ref().map(|s| get(s, "actorAnchor").clone()).unwrap_or(V::Null),
            "controllerRootAnchor": g.state.as_ref().map(|s| get(s, "controllerRootAnchor").clone()).unwrap_or(V::Null),
            "closed": g.closed,
            "failed": g.failed,
            "executionAuthority": false,
            "additionalApiClients": 0,
        })
    }

    /// close()
    pub fn close(&self) {
        let mut g = self.inner.borrow_mut();
        if g.closed {
            return;
        }
        g.closed = true;
        let _ = g.lease.release();
    }
}

fn iso_of(n: f64) -> String {
    baal_service_runtime::contract::ms_to_iso(n)
}
