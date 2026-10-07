// execution/baal-generic-live-host-v7.mjs — one financial owner, one existing
// account queue, complete inherited custody. A new request is explicit and
// bounded; historical requests are never renewed. v6 is sell-only custody:
// it inherits through the cohort-recovered seed (controller/budget v7, bridge
// v7), pins its source files via pinned-code-guard-v1 (stamp-checked every
// guard, byte-checked on full), and never opens new entries.
//
// Port notes (same conventions as baal_scaled_live_host_v1):
// - `need` throws Error('GENERIC_HOST_<code>') with no .code in JS; the ported
//   channel carries codes as the error string, foreign errors NUL-prefixed via
//   `codeless`. `lastFailure` picks `e.code ?? e.message` matching
//   /^[A-Z][A-Z0-9_]{1,160}$/ else GENERIC_OPERATION_FAILED — identical for both
//   channels since bare ported-module codes still satisfy the regex.
// - `api` is the caller-owned mutable object -> Rc<RefCell<BaalGenericLiveApi>>.
//   `retryAfterMs` cannot ride on the String error channel, so the cooldown
//   backoff branch is a documented no-op (same as sibling hosts).
// - async -> sync; spec-carried closures -> HostGuard params; permits keyed by
//   guard pointer address like the scaled host.
use crate::baal_custody_frontier_v1::read_baal_custody_frontier_v1;
use crate::pinned_code_guard_v1::{create_pinned_code_guard_v1, PinnedCodeGuardV1};
use crate::baal_expansion_custody_seed_v1::build_baal_expansion_custody_seed_v1 as build_baal_recovered_custody_seed_v2;
use crate::baal_generic_no_controller_v8::{
    baal_generic_no_controller_v8_identity, open_baal_generic_no_controller_v8,
    BaalGenericNoControllerV8, BaalGenericNoControllerV8Deps,
};
use crate::baal_generic_no_evidence_bridge_v8::{
    create_baal_generic_no_evidence_bridge_v8, BaalGenericNoEvidenceBridgeV8,
    BaalGenericNoEvidenceBridgeV8Deps, Guard as HostGuard,
};
use crate::baal_generic_quote_context_v1::bind_baal_generic_quote_context_v1;
use crate::baal_generic_shared_budget_v8::{
    open_baal_generic_shared_budget_v8, BaalGenericSharedBudgetV8,
    BaalGenericSharedBudgetV8Deps,
};
use crate::core::{payload_hash, validate_execution_state};
use crate::frozen_paper_graph::collect_frozen_paper_files;
use crate::ledger::acquire_owner_lease;
use crate::no_rest_sell_reconciliation_v2::rounded_tournament_cash_interval;
use crate::gates::normalize_official_fill_tick;
use crate::generic_entry_rejection_v1::generic_entry_rejection_may_defer;
use crate::generic_read_packet_retry_v1::generic_read_packet_may_retry;
use crate::paired_rest_controller::WireDispatch;
use crate::party_winner_no_screen_v1::screen_party_winner_no_pairs;
use crate::transport::{Guard as TGuard, OrderTransportError};
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::{amount, instant};
use baal_service_runtime::json_text::{js_int, sha256};
use baal_service_runtime::ledger::OwnerLease;
use serde_json::{json, Value as V};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const T: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("GENERIC_HOST_{code}"))
    }
}
fn codeless(e: String) -> String {
    format!("\u{0}{e}")
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn iso(n: f64) -> String {
    ms_to_iso(n)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(
        v.as_str()
            .ok_or_else(|| codeless("INVALID_INSTANT".to_string()))?,
    )
    .map_err(codeless)
}
fn str_amount(v: &V) -> Result<i128, String> {
    amount(
        v.as_str()
            .ok_or_else(|| codeless("INVALID_AMOUNT".to_string()))?,
    )
    .map_err(codeless)
}
fn sha(bytes: &[u8]) -> String {
    sha256(bytes)
}
// JS === on JSON values: primitives equal by value/type; objects, arrays and
// null are only === to themselves, which two parsed values never are.
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(x), V::Number(y)) => {
            x.as_f64().map(|v| v.to_bits()) == y.as_f64().map(|v| v.to_bits())
        }
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
// JS 'entry:' + l.exchangeId — ToString coercion for the key suffix; absent
// property is `undefined`.
fn js_concat_key(v: Option<&V>) -> String {
    match v {
        None => "undefined".to_string(),
        Some(V::String(s)) => s.clone(),
        Some(V::Number(n)) => {
            let f = n.as_f64().unwrap_or(f64::NAN);
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", f as i64)
            } else {
                format!("{}", f)
            }
        }
        Some(V::Bool(b)) => b.to_string(),
        Some(V::Null) => "null".to_string(),
        Some(V::Array(a)) => a
            .iter()
            .map(|x| match x {
                V::Null => "".to_string(),
                other => js_concat_key(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(V::Object(_)) => "[object Object]".to_string(),
    }
}
// /^[A-Z][A-Z0-9_]{1,160}$/ applied to the error text (e.code ?? e.message).
fn failure_code_of(e: &str) -> String {
    let s = e.strip_prefix('\u{0}').unwrap_or(e);
    if s.len() >= 2
        && s.len() <= 161
        && s.bytes().next().is_some_and(|b| b.is_ascii_uppercase())
        && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    {
        s.to_string()
    } else {
        "GENERIC_OPERATION_FAILED".to_string()
    }
}
fn read_json_file(f: &Path) -> Result<V, String> {
    serde_json::from_str(&fs::read_to_string(f).map_err(|e| codeless(e.to_string()))?)
        .map_err(|e| codeless(e.to_string()))
}

pub fn baal_generic_live_host_v7_identity(
    workspace: &Path,
) -> Result<BTreeMap<String, String>, String> {
    collect_frozen_paper_files(
        workspace,
        &[
            "execution/baal-generic-live-host-v7.mjs".to_string(),
            "execution/scoped-rest-receipts-v6.mjs".to_string(),
        ],
    )
}

// makeBaalGenericLiveRequestV3({workspace,runtimeDir,universe,frontier,epoch,
//   startedAt,entryExpiresAt})
pub fn make_baal_generic_live_request_v7(
    workspace: &Path,
    runtime_dir: &Path,
    universe: &V,
    frontier: &V,
    epoch: &str,
    started_at: &str,
    entry_expires_at: &str,
) -> Result<V, String> {
    // v7: scope/allocation/activation bounds come from the expansion authority,
    // not the frontier state.
    let authority = read_json_file(&runtime_dir.join("baal-expansion-authority-v1.json"))?;
    let scope = get(&authority, "scope").clone();
    let allocation = get(&authority, "allocation").clone();
    let seed =
        build_baal_recovered_custody_seed_v2(frontier, &scope, &allocation, runtime_dir)?;
    let bounds = get(&authority, "activationBounds");
    if bounds.is_null() {
        return Err("TYPE_ERROR".into());
    }
    let epoch_ok = !epoch.is_empty()
        && epoch.len() <= 80
        && epoch
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    need(
        get(universe, "tournamentId").as_str() == sget(&scope, "tournamentId")
            && epoch_ok
            && instant_v(&json!(entry_expires_at))? > instant_v(&json!(started_at))?
            && strict_eq(&json!(entry_expires_at), get(bounds, "entryExpiresAt"))
            && strict_eq(&json!(started_at), get(bounds, "startedAt"))
            && instant_v(&json!(started_at))?
                < instant_v(get(get(&seed, "originalActivation"), "exitExpiresAt"))?,
        "NEW_BOUNDED_REQUEST",
    )?;
    let base = json!({
        "schemaVersion": 1,
        "kind": "baal_expansion_live_request_v1",
        "networkCoordination": {
            "kind": "baal_single_client_rolling_policy_v1",
            "readCompletionGapMs": 100,
            "writeCompletionGapMs": 2500,
            "maxReadsPerMinute": 60,
            "maxWritesPerMinute": 24,
        },
        "ownerId": get(&scope, "ownerId"),
        "runId": get(&scope, "runId"),
        "tournamentId": get(&scope, "tournamentId"),
        "universeHash": get(universe, "hash"),
        "epoch": epoch,
        "scope": scope,
        "allocation": allocation,
        "inheritanceHash": get(&seed, "seedHash"),
        "sourceStateHash": get(frontier, "currentStateHash"),
        "sourceActorAnchor": get(frontier, "currentActorAnchor"),
        "codeIdentity": json!(baal_generic_live_host_v7_identity(workspace)?),
        "recordedAt": started_at,
    });
    let budget_installed_hash = payload_hash(&json!({
        "kind": "generic_full_nominal_budget_v3",
        "epoch": epoch,
        "inheritanceHash": get(&seed, "seedHash"),
        "allocationHash": payload_hash(&allocation)?,
    }))?;
    let activation = json!({
        "schemaVersion": 1,
        "mode": "baal_generic_no_production_v8",
        "allowedActions": ["buy", "sell"],
        "minimumCohortProfit": "0.005000",
        "ownerId": get(&scope, "ownerId"),
        "runId": get(&scope, "runId"),
        "tournamentId": get(&scope, "tournamentId"),
        "scopeHash": payload_hash(&scope)?,
        "allocationHash": payload_hash(&allocation)?,
        "inheritanceHash": get(&seed, "seedHash"),
        "codeIdentityHash": payload_hash(&baal_generic_no_controller_v8_identity()?)?,
        "authorityHash": get(get(&seed, "originalActivation"), "authorityHash"),
        "budgetInstalledHash": budget_installed_hash,
        "startedAt": started_at,
        "entryExpiresAt": entry_expires_at,
        "exitExpiresAt": get(get(&seed, "originalActivation"), "exitExpiresAt"),
    });
    let mut out = base.as_object().cloned().unwrap_or_default();
    out.insert("activation".into(), activation);
    Ok(V::Object(out))
}

// {dispatch,manualReview,cooldown,next,onAdmission}
pub struct BaalGenericLiveApi {
    pub dispatch: Rc<WireDispatch>,
    pub manual_review: bool,
    pub cooldown: f64,
    pub next: f64,
    pub on_admission: Box<dyn FnMut(Option<f64>) -> Result<(), String>>,
}

pub struct BaalGenericLiveHostV7Deps {
    pub api: Rc<RefCell<BaalGenericLiveApi>>,
    pub universe: V,
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub get_account_lease: Rc<dyn Fn() -> V>,
    pub get_core_owner_record: Rc<dyn Fn() -> V>,
    pub get_other_state: Rc<dyn Fn() -> V>,
    pub get_legacy_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_pair_read_hints: Rc<dyn Fn() -> V>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> Result<bool, String>>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct HostInner {
    runtime_dir: PathBuf,
    workspace: PathBuf,
    request_file: PathBuf,
    request: V,
    request_hash: String,
    history_guard: PinnedCodeGuardV1,
    code: BTreeMap<String, String>,
    code_guard: PinnedCodeGuardV1,
    scope: V,
    allocation: V,
    activation: V,
    seed: V,
    dir: PathBuf,
    stop: PathBuf,
    financial: RefCell<Option<OwnerLease>>,
    controller: RefCell<Option<Rc<BaalGenericNoControllerV8>>>,
    budget: RefCell<Option<Rc<BaalGenericSharedBudgetV8>>>,
    bridge: RefCell<Option<Rc<BaalGenericNoEvidenceBridgeV8>>>,
    cash: RefCell<Option<V>>,
    closed: Cell<bool>,
    busy: Cell<bool>,
    failed: Cell<bool>,
    last_action: RefCell<String>,
    last_failure: RefCell<Option<String>>,
    last_candidate_scan: RefCell<V>,
    last_hint_check: RefCell<V>,
    permits: RefCell<HashMap<usize, V>>,
    cooldowns: RefCell<HashMap<String, f64>>,
    deps: BaalGenericLiveHostV7Deps,
}

pub struct BaalGenericLiveHostV7 {
    inner: Rc<RefCell<HostInner>>,
}

// v7: sealed prior-custody evidence now rides in history_guard, stamped per
// call like the code guard (bytes verified only when fullHistory).
fn mechanical(inner: &Rc<RefCell<HostInner>>, full_history: bool) -> Result<(), String> {
    inner
        .borrow()
        .history_guard
        .check(full_history)
        .map_err(codeless)?;
    let (closed, failed, api_block, stop_exists) = {
        let g = inner.borrow();
        let api = g.deps.api.borrow();
        (
            g.closed.get(),
            g.failed.get(),
            api.manual_review || (g.deps.now)() < api.cooldown,
            g.stop.exists(),
        )
    };
    need(
        !closed && !failed && (inner.borrow().deps.can_operate)() && !stop_exists && !api_block,
        "STOPPED",
    )?;
    {
        let g = inner.borrow();
        if let Some(f) = g.financial.borrow().as_ref() {
            f.assert_ownership().map_err(|e| e.message)?;
        }
    }
    let g = inner.borrow();
    let core = (g.deps.get_core_owner_record)();
    let account = (g.deps.get_account_lease)();
    need(
        sget(&core, "ownerId") == sget(&g.scope, "ownerId")
            && js_int(get(&core, "pid")) == Some(std::process::id() as i64)
            && sget(&core, "scope") == sget(&g.scope, "tournamentId")
            && payload_hash(&core).map_err(codeless)? == sget(&account, "leaseId").unwrap_or(""),
        "ACTUAL_CORE_OWNER",
    )
}

fn guard(inner: &Rc<RefCell<HostInner>>) -> Result<(), String> {
    guard_full(inner, false)
}

// guard(fullCode=false): pinned-code guard replaces the per-file hash loop —
// stamps always, bytes only on full.
fn guard_full(inner: &Rc<RefCell<HostInner>>, full: bool) -> Result<(), String> {
    mechanical(inner, full)?;
    inner.borrow().code_guard.check(full).map_err(codeless)?;
    {
        let g = inner.borrow();
        let bytes = fs::read(&g.request_file).map_err(|e| codeless(e.to_string()))?;
        need(sha(&bytes) == g.request_hash, "REQUEST_CHANGED")?;
    }
    let g = inner.borrow();
    let other = validate_execution_state(&(g.deps.get_other_state)()).map_err(codeless)?;
    let retired = (g.deps.get_legacy_risk)().map_err(codeless)?;
    need(
        get(&other, "intents")
            .as_object()
            .map(|m| m.values().all(|i| sget(i, "status") == Some("closed")))
            .unwrap_or(false)
            && get(&other, "owned")
                .as_object()
                .map(|m| {
                    m.values()
                        .all(|p| js_int(get(p, "quantity")) == Some(0))
                })
                .unwrap_or(false)
            && get(&retired, "complete") == &V::Bool(true)
            && js_int(get(&retired, "currentOwnedQuantity")) == Some(0)
            && sget(&retired, "currentReservedCash") == Some("0.000000")
            && get(&retired, "unresolvedIntentIds")
                .as_array()
                .is_some_and(|a| a.is_empty()),
        "OTHER_WRITER_LIABILITY",
    )
}

fn raw_of(inner: &Rc<RefCell<HostInner>>, response: &V, meta: &V) -> Result<V, String> {
    let g = inner.borrow();
    let v = (g.deps.receipt_of)(response, meta).map_err(codeless)?;
    if sget(meta, "method") == Some("GET")
        && sget(&v, "endpoint")
            == Some(format!("/tournaments/{}", sget(&g.scope, "tournamentSlug").unwrap_or("")).as_str())
    {
        *g.cash.borrow_mut() = Some(v.clone());
    }
    Ok(v)
}

fn observation(inner: &Rc<RefCell<HostInner>>) -> Result<V, String> {
    guard(inner)?;
    let g = inner.borrow();
    let cash = g
        .cash
        .borrow()
        .clone()
        .ok_or_else(|| "GENERIC_HOST_ACTUAL_CASH_REQUIRED".to_string())?;
    let v = rounded_tournament_cash_interval(
        &cash,
        &json!({
            "tournamentId": sget(&g.scope, "tournamentId").unwrap_or(""),
            "tournamentSlug": sget(&g.scope, "tournamentSlug").unwrap_or(""),
            "readAt": iso((g.deps.now)()),
        }),
    )
    .map_err(codeless)?;
    let mut out = g.scope.as_object().cloned().unwrap_or_default();
    out.insert("at".into(), json!(iso((g.deps.now)())));
    out.insert(
        "sourceHash".into(),
        json!(payload_hash(&json!({
            "cash": get(&v, "sourceBodyHash"),
            "requestHash": g.request_hash,
        }))
        .map_err(codeless)?),
    );
    out.insert("complete".into(), V::Bool(true));
    out.insert("eventScopeComplete".into(), V::Bool(true));
    out.insert("soleCollector".into(), V::Bool(true));
    out.insert("stopped".into(), V::Bool(false));
    out.insert("unknownAcceptance".into(), V::Bool(false));
    out.insert("unresolvedIntentIds".into(), json!([]));
    out.insert("cashLower".into(), get(&v, "lower").clone());
    out.insert("owned".into(), json!([]));
    out.insert("reservations".into(), json!([]));
    out.insert("loans".into(), json!([]));
    Ok(V::Object(out))
}

fn halt(inner: &Rc<RefCell<HostInner>>, e: &V) -> Result<V, String> {
    let g = inner.borrow();
    let source_hash = if hex(get(e, "sourceHash")) {
        sget(e, "sourceHash").unwrap_or("").to_string()
    } else {
        g.request_hash.clone()
    };
    let v = json!({
        "kind": "generic_financial_uncertainty",
        "ownerId": get(&g.scope, "ownerId"),
        "runId": get(&g.scope, "runId"),
        "epoch": get(&g.request, "epoch"),
        "at": iso((g.deps.now)()),
        "sourceHash": source_hash,
        "allWritersStop": true,
    });
    let stop = g.stop.clone();
    drop(g);
    if !stop.exists() {
        atomic_json(&stop, &v).map_err(codeless)?;
    }
    need(stop.exists(), "DURABLE_HALT")?;
    Ok(v)
}

fn financial_receipt(inner: &Rc<RefCell<HostInner>>) -> Result<V, String> {
    mechanical(inner, false)?;
    let g = inner.borrow();
    let f = g.financial.borrow();
    let identity = f.as_ref().map(|l| l.identity.clone()).unwrap_or(V::Null);
    Ok(json!({
        "ownerId": get(&g.scope, "ownerId"),
        "runId": get(&g.scope, "runId"),
        "tournamentId": get(&g.scope, "tournamentId"),
        "leaseId": payload_hash(&identity).map_err(codeless)?,
    }))
}

fn bootstrap(inner: &Rc<RefCell<HostInner>>) -> Result<(), String> {
    guard(inner)?;
    need(
        (inner.borrow().deps.has_read_headroom)(1).map_err(codeless)?,
        "CASH_HEADROOM",
    )?;
    let endpoint = {
        let g = inner.borrow();
        format!("/tournaments/{}", sget(&g.scope, "tournamentSlug").unwrap_or(""))
    };
    let inner_bd = inner.clone();
    let before_dispatch = move |_v: &V, _s: &V| -> Result<bool, String> {
        guard(&inner_bd).map(|_| true)
    };
    let inner_bn = inner.clone();
    let before_network = move |_v: &V, _s: &V| -> Result<bool, String> {
        guard(&inner_bn).map(|_| true)
    };
    let bd_ptr = &before_dispatch as &HostGuard as *const HostGuard as *const () as usize;
    let bn_ptr = &before_network as &HostGuard as *const HostGuard as *const () as usize;
    inner.borrow_mut().permits.borrow_mut().insert(
        bd_ptr,
        json!({"endpoint": endpoint, "beforeNetworkPtr": bn_ptr as u64}),
    );
    let result: Result<(), String> = (|| {
        let api = inner.borrow().deps.api.borrow().dispatch.clone();
        let guard_err = |c: &str| OrderTransportError {
            code: c.to_string(),
            outcome: "unknown".into(),
            may_have_executed: false,
            retry_same_request_only: false,
            retry_after_ms: 0.0,
            status: None,
            api_code: None,
            semantic_mismatch: false,
        };
        let bd_t = |p: &V, s: &V| -> Result<bool, OrderTransportError> {
            before_dispatch(p, s).map_err(|c| guard_err(&c))
        };
        let bn_t = |p: &V, s: &V| -> Result<bool, OrderTransportError> {
            before_network(p, s).map_err(|c| guard_err(&c))
        };
        let response = api(
            &json!({"method": "GET", "endpoint": endpoint}),
            Some(&bd_t as &TGuard),
            Some(&bn_t as &TGuard),
        )
        .map_err(|e| codeless(e.message))?;
        need(
            js_int(get(&response, "status")) == Some(200),
            "CASH_HTTP",
        )?;
        raw_of(inner, &response, &json!({"method": "GET"}))?;
        observation(inner)?;
        Ok(())
    })();
    inner.borrow_mut().permits.borrow_mut().remove(&bd_ptr);
    result
}

fn status(inner: &Rc<RefCell<HostInner>>) -> V {
    let g = inner.borrow();
    let controller_status = g.controller.borrow().as_ref().map(|c| c.status());
    let budget_status = g.budget.borrow().as_ref().map(|b| b.status());
    let bridge_status = g.bridge.borrow().as_ref().map(|b| b.status());
    json!({
        "schemaVersion": 1,
        "hostVersion": 16,
        "mode": "baal_generic_live_host_v7",
        "observedAt": iso((g.deps.now)()),
        "ownerId": get(&g.scope, "ownerId"),
        "runId": get(&g.scope, "runId"),
        "epoch": get(&g.request, "epoch"),
        "entryExpiresAt": get(&g.activation, "entryExpiresAt"),
        "exitExpiresAt": get(&g.activation, "exitExpiresAt"),
        "lastAction": g.last_action.borrow().clone(),
        "lastFailure": g.last_failure.borrow().clone().map(|s| json!(s)).unwrap_or(V::Null),
        "lastCandidateScan": g.last_candidate_scan.borrow().clone(),
        "lastHintCheck": g.last_hint_check.borrow().clone(),
        "failed": g.failed.get(),
        "closed": g.closed.get(),
        "busy": g.busy.get(),
        "financialOwnerBound": g.financial.borrow().is_some(),
        "readBootstrapComplete": g.cash.borrow().is_some(),
        "controller": controller_status.unwrap_or(V::Null),
        "budget": budget_status.unwrap_or(V::Null),
        "bridge": bridge_status.unwrap_or(V::Null),
        "additionalApiClients": 0,
        "additionalPersistentProcesses": 0,
    })
}

fn publish(inner: &Rc<RefCell<HostInner>>, action: &str) -> Result<V, String> {
    {
        let g = inner.borrow();
        *g.last_action.borrow_mut() = action.to_string();
    }
    let out = status(inner);
    let stats_path = inner
        .borrow()
        .runtime_dir
        .join("baal-scaled-live-stats.json");
    atomic_json(&stats_path, &out).map_err(codeless)?;
    Ok(out)
}

fn candidates(inner: &Rc<RefCell<HostInner>>) -> Result<Vec<V>, String> {
    let g = inner.borrow();
    let h = (g.deps.get_pair_read_hints)();
    if h.is_null()
        || sget(&h, "universeHash") != sget(&g.deps.universe, "hash")
        || sget(&h, "tournamentId") != sget(&g.scope, "tournamentId")
    {
        return Ok(Vec::new());
    }
    let catalog = read_json_file(&g.runtime_dir.join("baal-scaled-rootcatalog.json"))?;
    let quotes = bind_baal_generic_quote_context_v1(
        &h,
        sget(&g.scope, "tournamentId").unwrap_or(""),
        sget(&g.deps.universe, "hash").unwrap_or(""),
    )
    .map_err(codeless)?;
    let screen = screen_party_winner_no_pairs(&json!({
        "universe": g.deps.universe,
        "rootCaptures": get(&catalog, "rootCaptures"),
        "quoteCaptures": quotes,
        "account": V::Null,
        "now": iso((g.deps.now)()),
    }))
    .map_err(codeless)?;
    {
        let mut scan = serde_json::Map::new();
        scan.insert("at".into(), json!(iso((g.deps.now)())));
        scan.insert(
            "semanticPairs".into(),
            get(&screen, "semanticPairCount").clone(),
        );
        scan.insert("quoteCount".into(), json!(quotes.len()));
        let pairs: Vec<V> = get(&screen, "pairs")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|p| get(p, "indicativeMathValid") == &V::Bool(true))
            .take(8)
            .map(|p| {
                json!({
                    "exchangeIds": get(p, "exchangeIds"),
                    "math": get(p, "indicativeMathValid"),
                    "cofresh": get(p, "quotesCurrentAndCofresh"),
                    "gap": get(p, "protectiveSurplusQualified"),
                    "flags": get(p, "flags"),
                })
            })
            .collect();
        scan.insert("pairs".into(), json!(pairs));
        *g.last_candidate_scan.borrow_mut() = V::Object(scan);
    }
    let held: HashSet<String> = g
        .controller
        .borrow()
        .as_ref()
        .map(|c| c.status())
        .and_then(|s| get(&s, "inventory").as_object().cloned())
        .map(|m| {
            m.iter()
                .filter(|(_, p)| js_int(get(p, "quantity")).unwrap_or(0) > 0)
                .map(|(ex, _)| ex.clone())
                .collect()
        })
        .unwrap_or_default();
    let now_ms = (g.deps.now)();
    let cooldowns = g.cooldowns.borrow().clone();
    let mut pairs: Vec<V> = get(&screen, "pairs")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| {
            get(p, "indicativeMathValid") == &V::Bool(true)
                && get(p, "quotesCurrentAndCofresh") == &V::Bool(true)
                && get(p, "protectiveSurplusQualified") == &V::Bool(true)
                && get(p, "exchangeIds")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .all(|ex| {
                        let key = format!("entry:{}", ex.as_str().unwrap_or(""));
                        !held.contains(ex.as_str().unwrap_or(""))
                            && cooldowns.get(&key).cloned().unwrap_or(0.0) <= now_ms
                    })
        })
        .collect();
    pairs.sort_by(|a, b| {
        let fa = str_amount(get(a, "conditionalProtectiveSurplus")).unwrap_or(i128::MIN);
        let fb = str_amount(get(b, "conditionalProtectiveSurplus")).unwrap_or(i128::MIN);
        fb.cmp(&fa)
    });
    Ok(pairs
        .iter()
        .map(|p| {
            let market_ids = get(p, "marketIds").as_array().cloned().unwrap_or_default();
            let exchange_ids = get(p, "exchangeIds").as_array().cloned().unwrap_or_default();
            let raw_no_asks = get(p, "rawNoAsks").as_array().cloned().unwrap_or_default();
            let mut legs: Vec<V> = market_ids
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    json!({
                        "marketId": m,
                        "exchangeId": exchange_ids.get(i).cloned().unwrap_or(V::Null),
                        "indicativeNoAsk": raw_no_asks.get(i).cloned().unwrap_or(V::Null),
                    })
                })
                .collect();
            legs.sort_by(|a, b| {
                str_amount(get(a, "indicativeNoAsk"))
                    .unwrap_or(i128::MIN)
                    .cmp(&str_amount(get(b, "indicativeNoAsk")).unwrap_or(i128::MIN))
                    .then_with(|| {
                        js_int(get(a, "exchangeId"))
                            .unwrap_or(0)
                            .cmp(&js_int(get(b, "exchangeId")).unwrap_or(0))
                    })
            });
            json!(legs
                .iter()
                .map(|l| json!({"marketId": get(l, "marketId"), "exchangeId": get(l, "exchangeId")}))
                .collect::<Vec<V>>())
        })
        .collect())
}

impl BaalGenericLiveHostV7 {
    pub fn step(&self) -> Result<V, String> {
        need(!self.inner.borrow().busy.get(), "BUSY")?;
        self.inner.borrow().busy.set(true);
        let result = self.step_inner();
        self.inner.borrow().busy.set(false);
        result
    }

    fn step_inner(&self) -> Result<V, String> {
        let inner = &self.inner;
        let run: Result<V, String> = (|| {
            guard(inner)?;
            *inner.borrow().last_failure.borrow_mut() = None;
            let stale = {
                let g = inner.borrow();
                let cash = g.cash.borrow().clone();
                match &cash {
                    Some(r) => {
                        (g.deps.now)() as i64 - instant_v(get(r, "requestStartedAt"))? > 12000
                    }
                    None => true,
                }
            };
            if stale {
                bootstrap(inner)?;
            }
            let claimed = inner
                .borrow()
                .budget
                .borrow()
                .as_ref()
                .map(|b| get(&b.status(), "claimed") == &V::Bool(true))
                .unwrap_or(false);
            if !claimed {
                inner
                    .borrow()
                    .controller
                    .borrow()
                    .as_ref()
                    .ok_or_else(|| "GENERIC_HOST_INTERNAL".to_string())?
                    .refresh_reservation()?;
            }
            let s = inner
                .borrow()
                .controller
                .borrow()
                .as_ref()
                .map(|c| c.status())
                .unwrap_or(V::Null);
            let intent = get(&s, "intent").clone();
            if !intent.is_null() {
                let leg = get(&inner.borrow().scope, "exchanges")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|l| sget(l, "exchangeId") == sget(&intent, "exchangeId"))
                    .unwrap_or(V::Null);
                if get(&intent, "orderId").is_null() {
                    let phase = sget(&s, "phase").unwrap_or("");
                    let fetches = js_int(get(&intent, "actualFetchCount")).unwrap_or(0);
                    let now_ms = (inner.borrow().deps.now)() as i64;
                    let expires = instant_v(get(get(&intent, "body"), "expirationDate"))?;
                    if phase == "prepared" && fetches == 0 && now_ms >= expires {
                        inner
                            .borrow()
                            .controller
                            .borrow()
                            .as_ref()
                            .unwrap()
                            .abandon_prepared()?;
                        return publish(inner, "expired_unadmitted_intent_preserved_abandoned");
                    }
                    return publish(inner, "pending_acceptance_blocks_new_orders");
                }
                let order_id = sget(&intent, "orderId").unwrap_or("").to_string();
                let reconcile_result: Result<V, String> = (|| {
                    inner
                        .borrow()
                        .bridge
                        .borrow()
                        .as_ref()
                        .ok_or_else(|| "GENERIC_HOST_INTERNAL".to_string())?
                        .refresh_reconciliation(&order_id, &leg)?;
                    inner
                        .borrow()
                        .controller
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .reconcile(&inner.borrow().bridge.borrow().as_ref().unwrap().get_evidence(
                            &json!({
                                "operation": "reconcile",
                                "orderId": order_id,
                                "exchangeId": sget(&intent, "exchangeId").unwrap_or(""),
                            }),
                        )?)
                })();
                match reconcile_result {
                    Ok(_) => return publish(inner, "terminal_order_reconciled"),
                    Err(e) => {
                        if e.contains(":fatal:") || e.contains("durableRecordingFailed") {
                            return Err(e);
                        }
                        let now_ms = (inner.borrow().deps.now)() as i64;
                        let expires = instant_v(get(get(&intent, "body"), "expirationDate"))?;
                        if now_ms >= expires && get(&intent, "cancelPending") != &V::Bool(true) {
                            inner
                                .borrow()
                                .bridge
                                .borrow()
                                .as_ref()
                                .unwrap()
                                .refresh_cancel(&order_id, &leg)?;
                            inner
                                .borrow()
                                .controller
                                .borrow()
                                .as_ref()
                                .unwrap()
                                .cancel_expired()?;
                            return publish(inner, "known_expired_order_cancel_requested");
                        }
                        return Err(e);
                    }
                }
            }
            if let Some(inv) = get(&s, "inventory").as_object().cloned() {
                for (ex, p) in inv {
                    if js_int(get(&p, "quantity")).unwrap_or(0) <= 0 {
                        continue;
                    }
                    if inner
                        .borrow()
                        .cooldowns
                        .borrow()
                        .get(&ex)
                        .cloned()
                        .unwrap_or(0.0)
                        > (inner.borrow().deps.now)()
                    {
                        continue;
                    }
                    inner
                        .borrow()
                        .cooldowns
                        .borrow_mut()
                        .insert(ex.clone(), (inner.borrow().deps.now)() + 30000.0);
                    let leg = get(&inner.borrow().scope, "exchanges")
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .find(|l| sget(l, "exchangeId") == Some(ex.as_str()))
                        .unwrap_or(V::Null);
                    let r: Result<(), String> = (|| {
                        inner
                            .borrow()
                            .bridge
                            .borrow()
                            .as_ref()
                            .unwrap()
                            .refresh_sell(&leg)?;
                        inner
                            .borrow()
                            .controller
                            .borrow()
                            .as_ref()
                            .unwrap()
                            .prepare_sell(&ex, None)?;
                        inner
                            .borrow()
                            .bridge
                            .borrow()
                            .as_ref()
                            .unwrap()
                            .refresh_final_none()?;
                        inner
                            .borrow()
                            .controller
                            .borrow()
                            .as_ref()
                            .unwrap()
                            .dispatch_prepared()?;
                        Ok(())
                    })();
                    match r {
                        Ok(()) => return publish(inner, "cohort_profit_floor_sell_dispatched"),
                        Err(e) => {
                            let has_intent = inner
                                .borrow()
                                .controller
                                .borrow()
                                .as_ref()
                                .map(|c| !get(&c.status(), "intent").is_null())
                                .unwrap_or(false);
                            if e.contains(":fatal:")
                                || e.contains("durableRecordingFailed")
                                || has_intent
                            {
                                return Err(e);
                            }
                            *inner.borrow().last_failure.borrow_mut() =
                                Some(e.strip_prefix('\u{0}').unwrap_or(&e).to_string());
                        }
                    }
                }
            }
            // v7: owned exits reconciled — attempt up to two fresh funded
            // expanded entries while candidate reads hold.
            if (inner.borrow().deps.now)() as i64
                >= instant_v(get(&inner.borrow().activation, "entryExpiresAt"))?
            {
                return publish(inner, "expanded_entry_window_expired_owned_exits_managed");
            }
            let mut attempts = 0i32;
            for pair in candidates(inner)? {
                attempts += 1;
                if attempts > 2 || !(inner.borrow().deps.has_read_headroom)(10)? {
                    break;
                }
                for l in pair.as_array().cloned().unwrap_or_default() {
                    inner
                        .borrow()
                        .cooldowns
                        .borrow_mut()
                        .insert(
                            format!("entry:{}", js_concat_key(l.get("exchangeId"))),
                            (inner.borrow().deps.now)() + 30000.0,
                        );
                }
                let r: Result<(), String> = (|| {
                    inner
                        .borrow()
                        .bridge
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .refresh_buy(&pair)?;
                    inner
                        .borrow()
                        .controller
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .prepare_buy()?;
                    inner
                        .borrow()
                        .bridge
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .refresh_final_none()?;
                    inner
                        .borrow()
                        .controller
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .dispatch_prepared()?;
                    Ok(())
                })();
                match r {
                    Ok(()) => {
                        return publish(inner, "expanded_funded_generic_entry_dispatched")
                    }
                    Err(e) => {
                        let stripped = e.strip_prefix('\u{0}').unwrap_or(&e);
                        let code = stripped.split(':').next().unwrap_or(stripped);
                        let mut err_v = json!({
                            "code": code,
                            "fatal": stripped.contains(":fatal:"),
                            "durableRecordingFailed": stripped.contains("durableRecordingFailed"),
                        });
                        if stripped.contains("retryAfterMs") {
                            err_v["retryAfterMs"] = json!(1);
                        }
                        let ctrl_status = inner
                            .borrow()
                            .controller
                            .borrow()
                            .as_ref()
                            .unwrap()
                            .status();
                        if !generic_entry_rejection_may_defer(&err_v, &ctrl_status) {
                            return Err(e);
                        }
                        for l in pair.as_array().cloned().unwrap_or_default() {
                            inner
                                .borrow()
                                .cooldowns
                                .borrow_mut()
                                .insert(
                                    format!("entry:{}", js_concat_key(l.get("exchangeId"))),
                                    (inner.borrow().deps.now)() + 300000.0,
                                );
                        }
                        *inner.borrow().last_failure.borrow_mut() =
                            Some(code.to_string());
                    }
                }
            }
            publish(inner, "no_qualified_fresh_entry_or_exit")
        })();
        match run {
            Ok(v) => Ok(v),
            Err(e) => {
                {
                    let g = inner.borrow();
                    *g.last_failure.borrow_mut() = Some(failure_code_of(&e));
                }
                let stripped = e.strip_prefix('\u{0}').unwrap_or(&e);
                if stripped.contains("NON_NEUTRAL_GRAPH") || stripped.contains("NON_NEUTRAL_COLLATERAL") {
                    let _ = halt(inner, &json!({"kind": "unsupported_live_collateral"}));
                }
                let fatal = e.contains(":fatal:") || e.contains("durableRecordingFailed");
                let may_retry = generic_read_packet_may_retry(
                    &json!({"code": e.strip_prefix('\u{0}').unwrap_or(&e)}),
                    &inner
                        .borrow()
                        .controller
                        .borrow()
                        .as_ref()
                        .map(|c| c.status())
                        .unwrap_or(V::Null),
                );
                if fatal && !may_retry {
                    inner.borrow().failed.set(true);
                    return Err(e);
                }
                publish(inner, "blocked_custody_retained")
            }
        }
    }

    pub fn status(&self) -> V {
        status(&self.inner)
    }

    pub fn on_before_fetch(&self, e: &V) -> Result<V, String> {
        guard_full(&self.inner, true)?;
        self.inner
            .borrow()
            .controller
            .borrow()
            .as_ref()
            .map(|c| c.on_before_fetch(e))
            .unwrap_or(Ok(V::Null))
    }

    pub fn on_fetch_start(&self, e: &V) -> Result<bool, String> {
        self.inner
            .borrow()
            .controller
            .borrow()
            .as_ref()
            .map(|c| c.on_fetch_start(e))
            .unwrap_or(Ok(false))
    }

    // authorizes(spec) — bootstrap permit or bridge read guard.
    pub fn authorizes(
        &self,
        spec: &V,
        before_dispatch: &HostGuard,
        before_network: &HostGuard,
    ) -> bool {
        let r: Result<bool, String> = (|| {
            mechanical(&self.inner, false)?;
            let g = self.inner.borrow();
            let key = before_dispatch as *const HostGuard as *const () as usize;
            let p = g.permits.borrow().get(&key).cloned();
            let bootstrap_ok = p.is_some()
                && sget(spec, "method") == Some("GET")
                && get(spec, "endpoint") == get(p.as_ref().unwrap(), "endpoint")
                && (before_network as *const HostGuard as *const () as usize)
                    == get(p.as_ref().unwrap(), "beforeNetworkPtr")
                        .as_u64()
                        .unwrap_or(0) as usize;
            if bootstrap_ok {
                return Ok(true);
            }
            let bridge = g.bridge.borrow().clone();
            Ok(bridge
                .as_ref()
                .map(|b| b.authorizes(spec, before_dispatch, before_network))
                .unwrap_or(false))
        })();
        r.unwrap_or(false)
    }

    // controller-issued write guard (the JS authorizes ORs it in one call).
    pub fn authorizes_write(
        &self,
        spec: &V,
        before_dispatch: Option<&TGuard<'_>>,
        before_network: Option<&TGuard<'_>>,
    ) -> bool {
        let r: Result<bool, String> = (|| {
            mechanical(&self.inner, false)?;
            let controller = self.inner.borrow().controller.borrow().clone();
            Ok(controller
                .as_ref()
                .map(|c| c.authorizes(spec, before_dispatch, before_network))
                .unwrap_or(false))
        })();
        r.unwrap_or(false)
    }

    pub fn blocks_legacy_writes(&self) -> bool {
        true
    }

    pub fn allows_legacy(&self, spec: &V) -> bool {
        sget(spec, "method") == Some("GET")
    }

    pub fn close(&self) -> Result<(), String> {
        need(!self.inner.borrow().busy.get(), "BUSY")?;
        if self.inner.borrow().closed.get() {
            return Ok(());
        }
        if let Some(b) = self.inner.borrow().bridge.borrow().as_ref() {
            let _ = b.close();
        }
        if let Some(b) = self.inner.borrow().budget.borrow().as_ref() {
            let _ = b.close();
        }
        if let Some(c) = self.inner.borrow().controller.borrow().as_ref() {
            let _ = c.close();
        }
        if let Some(f) = self.inner.borrow().financial.borrow_mut().as_mut() {
            let _ = f.release();
        }
        self.inner.borrow().closed.set(true);
        Ok(())
    }
}

// openBaalGenericLiveHostV7({runtimeDir,workspace,universe,api,receiptOf,
//   getAccountLease,getCoreOwnerRecord,getOtherState,getLegacyRisk,
//   getPairReadHints=()=>null,canOperate,hasReadHeadroom,now})
pub fn open_baal_generic_live_host_v7(
    runtime_dir: &Path,
    workspace: &Path,
    deps: BaalGenericLiveHostV7Deps,
) -> Result<Option<BaalGenericLiveHostV7>, String> {
    let request_file = runtime_dir.join("baal-expansion-live-request-v1.json");
    if !request_file.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&request_file).map_err(|e| codeless(e.to_string()))?;
    let request_hash = sha(&bytes);
    let r: V = serde_json::from_slice(&bytes).map_err(|e| codeless(e.to_string()))?;
    let code = baal_generic_live_host_v7_identity(workspace)?;
    let frontier = read_baal_custody_frontier_v1(runtime_dir, workspace)?;
    let expected = make_baal_generic_live_request_v7(
        workspace,
        runtime_dir,
        &deps.universe,
        &frontier,
        sget(&r, "epoch").unwrap_or(""),
        sget(get(&r, "activation"), "startedAt").unwrap_or(""),
        sget(get(&r, "activation"), "entryExpiresAt").unwrap_or(""),
    )?;
    need(
        payload_hash(&r).map_err(codeless)? == payload_hash(&expected).map_err(codeless)?
            && instant_v(get(&r, "recordedAt"))? <= (deps.now)() as i64,
        "EXACT_REQUEST_AND_CUSTODY",
    )?;
    let scope = get(&r, "scope").clone();
    let allocation = get(&r, "allocation").clone();
    let activation = get(&r, "activation").clone();
    let seed =
        build_baal_recovered_custody_seed_v2(&frontier, &scope, &allocation, runtime_dir)?;
    // priorCustody is re-derived from the BASE generic seed, not the recovered one.
    // v7: every custody seal now rides in one pinned-code history guard
    // keyed by runtime-dir relative path.
    let mut seals: Vec<V> = Vec::new();
    for key in [
        "sourceHashes",
        "recoverySources",
        "currentSources",
        "cohortRecoverySources",
        "expansionSources",
    ] {
        match get(&seed, key) {
            V::Array(a) => seals.extend(a.iter().cloned()),
            V::String(st) => seals.extend(st.chars().map(|c| json!(c.to_string()))),
            _ => return Err("TYPE_ERROR".into()),
        }
    }
    let prior_custody = json!({"seals": seals});
    let mut history_identity: BTreeMap<String, String> = BTreeMap::new();
    for seal in get(&prior_custody, "seals")
        .as_array()
        .cloned()
        .unwrap_or_default()
    {
        history_identity.insert(
            sget(&seal, "relative").unwrap_or("").to_string(),
            sget(&seal, "sha256").unwrap_or("").to_string(),
        );
    }
    let history_guard = create_pinned_code_guard_v1(runtime_dir, &history_identity)?;
    let dir = runtime_dir
        .join("baal-generic-live-runs")
        .join(sget(&r, "runId").unwrap_or(""))
        .join(sget(&r, "epoch").unwrap_or("").to_string());
    let controller_dir = dir.join("controller");
    let budget_dir = dir.join("budget");
    let stop = runtime_dir.join("EXECUTION_STOP");
    let code_guard = create_pinned_code_guard_v1(workspace, &code)?;

    let inner = Rc::new(RefCell::new(HostInner {
        runtime_dir: runtime_dir.to_path_buf(),
        workspace: workspace.to_path_buf(),
        request_file: request_file.clone(),
        request: r,
        request_hash,
        history_guard,
        code,
        code_guard,
        scope,
        allocation,
        activation,
        seed,
        dir,
        stop,
        financial: RefCell::new(None),
        controller: RefCell::new(None),
        budget: RefCell::new(None),
        bridge: RefCell::new(None),
        cash: RefCell::new(None),
        closed: Cell::new(false),
        busy: Cell::new(false),
        failed: Cell::new(false),
        last_action: RefCell::new("awaiting_cash".to_string()),
        last_failure: RefCell::new(None),
        last_candidate_scan: RefCell::new(V::Null),
        last_hint_check: RefCell::new(V::Null),
        permits: RefCell::new(HashMap::new()),
        cooldowns: RefCell::new(HashMap::new()),
        deps,
    }));

    let create_result: Result<(), String> = (|| {
        need(
            !inner
                .borrow()
                .runtime_dir
                .join("baal-financial-owner")
                .join("execution-owner.lock")
                .exists(),
            "PRIOR_FINANCIAL_OWNER_ACTIVE",
        )?;
        let financial = acquire_owner_lease(
            &inner.borrow().runtime_dir.join("baal-financial-owner"),
            sget(&inner.borrow().scope, "ownerId").unwrap_or(""),
            sget(&inner.borrow().scope, "runId").unwrap_or(""),
            "baal-global-financial-owner",
            (inner.borrow().deps.now)(),
        )
        .map_err(|e| e.message)?;
        *inner.borrow().financial.borrow_mut() = Some(financial);
        guard(&inner)?;

        let controller_dir_c = controller_dir.clone();
        let runtime_c = runtime_dir.to_path_buf();
        let scope_c = get(&inner.borrow().request, "scope").clone();
        let allocation_c = get(&inner.borrow().request, "allocation").clone();
        let activation_c = get(&inner.borrow().request, "activation").clone();
        let inner_for_controller = inner.clone();
        let inner_for_budget = inner.clone();
        let inner_for_bridge = inner.clone();
        let controller = Rc::new(open_baal_generic_no_controller_v8(
            &controller_dir_c,
            &runtime_c,
            &scope_c,
            &allocation_c,
            &activation_c,
            BaalGenericNoControllerV8Deps {
                dispatch: inner.borrow().deps.api.borrow().dispatch.clone(),
                raw_receipt_of: {
                    let inner_raw = inner.clone();
                    Rc::new(move |response: &V, meta: &V| raw_of(&inner_raw, response, meta))
                },
                can_operate: {
                    let i = inner.clone();
                    Rc::new(move || guard(&i).is_ok())
                },
                assert_account_lease: inner.borrow().deps.get_account_lease.clone(),
                assert_financial_owner_lease: {
                    let i = inner.clone();
                    Rc::new(move || financial_receipt(&i).unwrap_or(V::Null))
                },
                on_financial_halt: {
                    let i = inner.clone();
                    Rc::new(move |e: &V| halt(&i, e))
                },
                can_check_mechanical: Some({
                    let i = inner.clone();
                    Rc::new(move || mechanical(&i, false).is_ok())
                }),
                install_funding_reservation: {
                    Rc::new(move |r: &V| {
                        inner_for_budget
                            .borrow()
                            .budget
                            .borrow()
                            .as_ref()
                            .ok_or_else(|| "GENERIC_HOST_INTERNAL".to_string())?
                            .install_funding_reservation(r)
                    })
                },
                get_global_risk: {
                    let i = inner_for_controller.clone();
                    Rc::new(move || {
                        i.borrow()
                            .budget
                            .borrow()
                            .as_ref()
                            .ok_or_else(|| "GENERIC_HOST_INTERNAL".to_string())?
                            .global_risk()
                    })
                },
                get_evidence: {
                    Rc::new(move |c: &V| {
                        inner_for_bridge
                            .borrow()
                            .bridge
                            .borrow()
                            .as_ref()
                            .ok_or_else(|| "GENERIC_HOST_INTERNAL".to_string())?
                            .get_evidence(c)
                    })
                },
                now: inner.borrow().deps.now.clone(),
            },
        )?);
        *inner.borrow().controller.borrow_mut() = Some(controller.clone());

        let budget = Rc::new(open_baal_generic_shared_budget_v8(
            &budget_dir,
            &controller_dir,
            &runtime_c,
            &inner.borrow().seed,
            &allocation_c,
            &scope_c,
            sget(&activation_c, "budgetInstalledHash").unwrap_or(""),
            sget(&activation_c, "codeIdentityHash").unwrap_or(""),
            BaalGenericSharedBudgetV8Deps {
                assert_account_lease: inner.borrow().deps.get_account_lease.clone(),
                assert_financial_owner_lease: {
                    let i = inner.clone();
                    Rc::new(move || financial_receipt(&i).unwrap_or(V::Null))
                },
                get_controller_source: {
                    let i = inner.clone();
                    Rc::new(move || {
                        i.borrow()
                            .controller
                            .borrow()
                            .as_ref()
                            .ok_or_else(|| "GENERIC_HOST_INTERNAL".to_string())?
                            .journal_source()
                    })
                },
                observe_global_risk: {
                    let i = inner.clone();
                    Rc::new(move || observation(&i))
                },
                can_operate: {
                    let i = inner.clone();
                    Rc::new(move || guard(&i).is_ok())
                },
                now: inner.borrow().deps.now.clone(),
            },
        )?);
        *inner.borrow().budget.borrow_mut() = Some(budget.clone());

        let bridge = Rc::new(create_baal_generic_no_evidence_bridge_v8(
            &scope_c,
            &allocation_c,
            &inner.borrow().deps.universe,
            BaalGenericNoEvidenceBridgeV8Deps {
                dispatch: {
                    let api_dispatch = inner.borrow().deps.api.borrow().dispatch.clone();
                    Rc::new(
                        move |spec: &V,
                              bd: Option<&HostGuard>,
                              bn: Option<&HostGuard>|
                              -> Result<V, String> {
                            let guard_err = |c: &str| OrderTransportError {
                                code: c.to_string(),
                                outcome: "unknown".into(),
                                may_have_executed: false,
                                retry_same_request_only: false,
                                retry_after_ms: 0.0,
                                status: None,
                                api_code: None,
                                semantic_mismatch: false,
                            };
                            let bd_t = |p: &V, s: &V| -> Result<bool, OrderTransportError> {
                                bd.map(|g| g(p, s).map_err(|c| guard_err(&c)))
                                    .unwrap_or(Ok(false))
                            };
                            let bn_t = |p: &V, s: &V| -> Result<bool, OrderTransportError> {
                                bn.map(|g| g(p, s).map_err(|c| guard_err(&c)))
                                    .unwrap_or(Ok(false))
                            };
                            api_dispatch(spec, Some(&bd_t as &TGuard), Some(&bn_t as &TGuard))
                                .map_err(|e| codeless(e.message))
                        },
                    )
                },
                receipt_of: {
                    let inner_raw = inner.clone();
                    Rc::new(move |response: &V, meta: &V| raw_of(&inner_raw, response, meta))
                },
                get_account_lease: {
                    let f = inner.borrow().deps.get_account_lease.clone();
                    Rc::new(move || Ok(f()))
                },
                get_global_risk: {
                    let i = inner.clone();
                    Rc::new(move || {
                        i.borrow()
                            .budget
                            .borrow()
                            .as_ref()
                            .ok_or_else(|| "GENERIC_HOST_INTERNAL".to_string())?
                            .global_risk()
                    })
                },
                get_account_risk: None,
                get_owned_census: {
                    let i = inner.clone();
                    Rc::new(move |ex: &str| {
                        Ok(i.borrow()
                            .controller
                            .borrow()
                            .as_ref()
                            .map(|c| c.inventory(ex))
                            .unwrap_or(V::Null))
                    })
                },
                get_installed_budget: {
                    let i = inner.clone();
                    Rc::new(move || {
                        Ok(i.borrow()
                            .controller
                            .borrow()
                            .as_ref()
                            .map(|c| c.funding_reservation_status())
                            .unwrap_or(V::Null))
                    })
                },
                owns_order: {
                    let i = inner.clone();
                    Rc::new(move |oid: &str, _leg: &V| {
                        Ok(i.borrow()
                            .controller
                            .borrow()
                            .as_ref()
                            .map(|c| {
                                sget(get(&c.status(), "intent"), "orderId") == Some(oid)
                            })
                            .unwrap_or(false))
                    })
                },
                get_order_binding: {
                    let i = inner.clone();
                    Rc::new(move |oid: &str| {
                        Ok(i.borrow()
                            .controller
                            .borrow()
                            .as_ref()
                            .map(|c| c.order_binding(oid))
                            .unwrap_or(V::Null))
                    })
                },
                get_active_intent: Some({
                    let i = inner.clone();
                    Rc::new(move || {
                        Ok(i.borrow()
                            .controller
                            .borrow()
                            .as_ref()
                            .map(|c| {
                                let s = c.status();
                                if get(&s, "intent").is_null() {
                                    V::Null
                                } else {
                                    let mut o = get(&s, "intent")
                                        .as_object()
                                        .cloned()
                                        .unwrap_or_default();
                                    o.insert("phase".into(), get(&s, "phase").clone());
                                    V::Object(o)
                                }
                            })
                            .unwrap_or(V::Null))
                    })
                }),
                can_read: {
                    let i = inner.clone();
                    Rc::new(move || guard(&i).is_ok())
                },
                can_check_read_identity: Some({
                    let i = inner.clone();
                    Rc::new(move || mechanical(&i, false).is_ok())
                }),
                refresh_funding_after_account: Some({
                    let i = inner.clone();
                    Rc::new(move || {
                        i.borrow()
                            .controller
                            .borrow()
                            .as_ref()
                            .ok_or_else(|| "GENERIC_HOST_INTERNAL".to_string())?
                            .refresh_reservation()
                    })
                }),
                has_read_headroom: inner.borrow().deps.has_read_headroom.clone(),
                now: inner.borrow().deps.now.clone(),
            },
        )?);
        *inner.borrow().bridge.borrow_mut() = Some(bridge);
        Ok(())
    })();

    if let Err(e) = create_result {
        let host = BaalGenericLiveHostV7 { inner };
        let _ = host.close();
        return Err(e);
    }
    Ok(Some(BaalGenericLiveHostV7 { inner }))
}
