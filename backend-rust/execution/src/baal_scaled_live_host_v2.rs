// execution/baal-scaled-live-host-v2.mjs — guarded host wiring the live
// request, capital/authority/catalog pins, controller, shared budget, and
// evidence bridge together. V2 adds the recorded host-upgrade binding and
// the per-inventory owned-no-exit read hint.
//
// Port notes:
// - `need` throws errors with code BAAL_SCALED_HOST_<code> (Object.assign
//   sets .code). Foreign/bare errors carry no .code in JS; the ported
//   channel marks foreign errors NUL-prefixed via `codeless`, and
//   `host_code_of` maps `e.code ?? fallback` accordingly: coded prefixes
//   (BAAL_SCALED_HOST_/BAAL_SCALED_EVIDENCE_) and transport codes pass,
//   bare module prefixes fall back.
// - `api` is a caller-owned mutable object -> Rc<RefCell<BaalScaledLiveApi>>;
//   `onAdmission` may throw in JS, so it returns Result here. The ported
//   error channel carries no retryAfterMs, so `backoff` is a documented
//   no-op (fatal/durableRecordingFailed are signalled by :fatal:/markers in
//   the message where a module encodes them, same as sibling hosts).
// - async fns -> sync; spec-carried closures -> HostGuard params; the OR
//   in JS authorizes is split into authorizes (read side) and
//   authorizes_write (controller side) like paired_live_host.
// - openBaalScaledLiveHost returns null when the request file is absent ->
//   Ok(None).
use crate::baal_scaled_no_controller_v1::{
    baal_scaled_no_controller_identity, open_baal_scaled_no_controller,
    BaalScaledNoController, BaalScaledNoControllerDeps,
};
use crate::baal_scaled_no_evidence_bridge_v1::{
    create_baal_scaled_no_evidence_bridge, BaalScaledEvidenceBridgeDeps,
    BaalScaledNoEvidenceBridge,
};
use crate::baal_scaled_owned_exit_read_hint_v1::owned_no_exit_read_hint;
use crate::baal_scaled_shared_budget_v1::{
    open_baal_scaled_shared_budget, BaalScaledSharedBudget, BaalScaledSharedBudgetDeps,
};
use crate::core::{payload_hash, validate_execution_state};
use crate::frozen::collect_frozen_paper_files;
use crate::no_rest_sell_reconciliation_v2::rounded_tournament_cash_interval;
use crate::paired_rest_controller::WireDispatch;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::instant;
use baal_service_runtime::json_text::{js_int, js_truthy};
use crate::transport::Guard as TGuard;
use serde_json::{json, Map, Value as V};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const T: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";

pub type HostGuard<'a> = dyn Fn(&V, &V) -> Result<bool, String> + 'a;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("BAAL_SCALED_HOST_{code}"))
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
    x.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty()
            && !s.starts_with('0')
            && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok()
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
fn is_all_caps_code(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false)
        && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}
// The JS e.code property exists only on errors created with an explicit
// code (host/bridge needs and the transport's coded errors). Bare
// `throw Error('CODE')` values from ported modules have no .code.
fn host_code_of(e: &str) -> Option<String> {
    if e.starts_with('\u{0}') {
        return None;
    }
    for coded in ["BAAL_SCALED_HOST_", "BAAL_SCALED_EVIDENCE_"] {
        if e.starts_with(coded)
            && e[coded.len()..]
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b == b'_')
            && !e[coded.len()..].is_empty()
        {
            return Some(e.to_string());
        }
    }
    for bare in [
        "BAAL_SCALED_",
        "BAAL_SHARED_BUDGET_",
        "BAAL_LEGACY_",
        "BAAL_",
        "REST_",
        "PAIRED_",
        "PARTY_",
        "SCOPED_",
        "INVALID_",
        "SYNC_",
    ] {
        if e.starts_with(bare) {
            return None;
        }
    }
    if is_all_caps_code(e) {
        return Some(e.to_string());
    }
    None
}
fn read_json_file(f: &Path) -> Result<V, String> {
    serde_json::from_str(&fs::read_to_string(f).map_err(|e| codeless(e.to_string()))?)
        .map_err(|e| codeless(e.to_string()))
}

pub fn baal_scaled_live_identity(workspace: &Path) -> Result<V, String> {
    collect_frozen_paper_files(
        workspace,
        &[
            "execution/baal-scaled-live-host-v1.mjs",
            "execution/scoped-rest-receipts-v2.mjs",
        ],
    )
}

pub fn baal_scaled_live_v2_identity(workspace: &Path) -> Result<V, String> {
    collect_frozen_paper_files(
        workspace,
        &[
            "execution/baal-scaled-live-host-v2.mjs",
            "execution/scoped-rest-receipts-v2.mjs",
        ],
    )
}

// The mutable shared api object the caller owns:
// {dispatch,manualReview,cooldown,next,onAdmission}.
pub struct BaalScaledLiveApi {
    pub dispatch: Rc<WireDispatch>,
    pub manual_review: bool,
    pub cooldown: f64,
    pub next: f64,
    pub on_admission: Box<dyn FnMut(Option<f64>) -> Result<(), String>>,
}

pub struct BaalScaledLiveHostV2Deps {
    pub api: Rc<RefCell<BaalScaledLiveApi>>,
    pub universe: V,
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub get_account_lease: Rc<dyn Fn() -> V>,
    pub get_other_state: Rc<dyn Fn() -> V>,
    pub get_legacy_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_pair_read_hints: Option<Rc<dyn Fn() -> V>>,
    pub get_read_hint: Option<Rc<dyn Fn() -> V>>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> Result<bool, String>>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct HostInner {
    file: PathBuf,
    capital_file: PathBuf,
    authority_file: PathBuf,
    catalog_file: PathBuf,
    catalog: V,
    runtime_dir: PathBuf,
    workspace: PathBuf,
    request: V,
    request_hash: String,
    scope: V,
    allocation: V,
    allocation_hash: String,
    budget_installed_hash: String,
    activation: V,
    code: V,
    upgrade_file: PathBuf,
    upgrade: V,
    upgrade_hash: String,
    original_host_code_identity_hash: String,
    controller: RefCell<Option<Rc<BaalScaledNoController>>>,
    budget: RefCell<Option<Rc<BaalScaledSharedBudget>>>,
    bridge: RefCell<Option<Rc<BaalScaledNoEvidenceBridge>>>,
    cash_receipt: RefCell<Option<V>>,
    positions_receipt: RefCell<Option<V>>,
    closed: Cell<bool>,
    failed: Cell<bool>,
    busy: Cell<bool>,
    last_failure: RefCell<Option<String>>,
    last_action: RefCell<String>,
    last_step_at: RefCell<V>,
    bootstrap_permits: RefCell<HashMap<usize, V>>,
    cooldowns: RefCell<HashMap<String, f64>>,
    sell_checks: RefCell<HashMap<String, f64>>,
    exit_hint_skipped_packets: Cell<i64>,
    last_exit_hint: RefCell<V>,
    deps: BaalScaledLiveHostV2Deps,
}

pub struct BaalScaledLiveHostV2 {
    pub(crate) inner: Rc<RefCell<HostInner>>,
}

fn allowed(inner: &Rc<RefCell<HostInner>>) -> bool {
    let r: Result<bool, String> = (|| {
        let g = inner.borrow();
        let request_hash = payload_hash(&read_json_file(&g.file)?).map_err(|e| e)?;
        let capital_hash = payload_hash(&read_json_file(&g.capital_file)?)?;
        let authority_hash = payload_hash(&read_json_file(&g.authority_file)?)?;
        let catalog_hash = payload_hash(&read_json_file(&g.catalog_file)?)?;
        let ident = baal_scaled_live_identity(&g.workspace)?;
        let upgrade_hash = payload_hash(&read_json_file(&g.upgrade_file)?)?;
        let ident_v2 = baal_scaled_live_v2_identity(&g.workspace)?;
        Ok(!g.closed.get()
            && !g.failed.get()
            && (g.deps.can_operate)()
            && request_hash == g.request_hash
            && capital_hash == get(&g.request, "humanAllocationSourceHash").as_str().unwrap_or("")
            && authority_hash == get(&g.request, "authorityHash").as_str().unwrap_or("")
            && catalog_hash == get(&g.request, "catalogHash").as_str().unwrap_or("")
            && payload_hash(&ident)? == g.original_host_code_identity_hash
            && upgrade_hash == g.upgrade_hash
            && payload_hash(&ident_v2)? == payload_hash(&g.code)?)
    })();
    r.unwrap_or(false)
}

fn legacy(inner: &Rc<RefCell<HostInner>>) -> Result<V, String> {
    let (get_other_state, get_legacy_risk) = {
        let g = inner.borrow();
        (g.deps.get_other_state.clone(), g.deps.get_legacy_risk.clone())
    };
    let core = validate_execution_state(&(get_other_state)()).map_err(codeless)?;
    let retired = (get_legacy_risk)().map_err(codeless)?;
    let pending = get(&core, "intents")
        .as_object()
        .map(|m| {
            m.values()
                .filter(|i| get(i, "status").as_str() != Some("closed"))
                .count()
        })
        .unwrap_or(0);
    let owned = get(&core, "owned")
        .as_object()
        .map(|m| {
            m.values()
                .filter(|p| get(p, "quantity").as_f64().unwrap_or(0.0) > 0.0)
                .count()
        })
        .unwrap_or(0);
    need(
        get(&retired, "complete") == &V::Bool(true)
            && hex(get(&retired, "sourceHash"))
            && get(&retired, "currentOwnedQuantity") == &json!(0)
            && get(&retired, "currentReservedCash").as_str() == Some("0.000000")
            && get(&retired, "unresolvedIntentIds")
                .as_array()
                .is_some_and(|a| a.is_empty())
            && pending == 0
            && owned == 0,
        "LEGACY_NOT_VERIFIED_RETIRED",
    )?;
    Ok(json!({
        "coreHash": payload_hash(&core).map_err(codeless)?,
        "retirementHash": get(&retired, "sourceHash"),
    }))
}

fn remember(inner: &Rc<RefCell<HostInner>>, receipt: &V, method: &str, endpoint: &str) -> Result<V, String> {
    need(
        sget(receipt, "method") == Some(method) && sget(receipt, "endpoint") == Some(endpoint),
        "RAW_CALLBACK_SCOPE",
    )?;
    let slug = {
        let g = inner.borrow();
        sget(&g.scope, "tournamentSlug").unwrap_or("").to_string()
    };
    if method == "GET" && endpoint == format!("/tournaments/{slug}") {
        *inner.borrow_mut().cash_receipt.borrow_mut() = Some(receipt.clone());
    }
    if method == "GET" && endpoint == format!("/tournaments/{slug}/portfolio/positions") {
        *inner.borrow_mut().positions_receipt.borrow_mut() = Some(receipt.clone());
    }
    Ok(receipt.clone())
}

fn raw_of(inner: &Rc<RefCell<HostInner>>, response: &V, meta: &V) -> Result<V, String> {
    let receipt_of = inner.borrow().deps.receipt_of.clone();
    let r = receipt_of(response, meta).map_err(codeless)?;
    remember(inner, &r, sget(meta, "method").unwrap_or(""), sget(response, "endpoint").unwrap_or(""))
}

fn observation(inner: &Rc<RefCell<HostInner>>) -> Result<V, String> {
    need(allowed(inner), "STOPPED")?;
    let l = legacy(inner)?;
    let (cash_receipt, scope, deps_now) = {
        let g = inner.borrow();
        (
            g.cash_receipt.borrow().clone(),
            g.scope.clone(),
            g.deps.now.clone(),
        )
    };
    need(cash_receipt.is_some(), "ACTUAL_CASH_NOT_YET_OBSERVED")?;
    let cash = rounded_tournament_cash_interval(
        &cash_receipt.unwrap(),
        &json!({
            "tournamentId": T,
            "tournamentSlug": sget(&scope, "tournamentSlug"),
            "readAt": iso(deps_now()),
        }),
    )
    .map_err(codeless)?;
    let lease = (inner.borrow().deps.get_account_lease)();
    need(
        get(&lease, "ownerId").as_str() == Some("Codex")
            && get(&lease, "tournamentId").as_str() == Some(T)
            && hex(get(&lease, "leaseId")),
        "ACTUAL_ACCOUNT_LEASE",
    )?;
    Ok(json!({
        "ownerId": "Codex",
        "runId": get(&scope, "runId"),
        "tournamentId": T,
        "complete": true,
        "eventScopeComplete": true,
        "soleCollector": true,
        "stopped": false,
        "unknownAcceptance": false,
        "unresolvedIntentIds": [],
        "cashLower": get(&cash, "lower"),
        "at": iso((inner.borrow().deps.now)()),
        "sourceHash": payload_hash(&json!({
            "legacy": l,
            "cash": get(&cash, "sourceBodyHash"),
            "lease": payload_hash(&lease).map_err(codeless)?,
        })).map_err(codeless)?,
        "owned": [],
        "reservations": [],
        "loans": [],
    }))
}

fn backoff(inner: &Rc<RefCell<HostInner>>, _e: &str) -> Result<(), String> {
    // Ported error channel carries no retryAfterMs: there is nothing to
    // persist, matching the JS early return when e?.retryAfterMs is absent.
    let _ = inner;
    Ok(())
}

fn guard(inner: &Rc<RefCell<HostInner>>) -> Result<(), String> {
    need(allowed(inner), "STOPPED")?;
    legacy(inner)?;
    let lease = (inner.borrow().deps.get_account_lease)();
    need(
        get(&lease, "ownerId").as_str() == Some("Codex")
            && get(&lease, "tournamentId").as_str() == Some(T)
            && hex(get(&lease, "leaseId")),
        "ACTUAL_ACCOUNT_LEASE",
    )
}

fn bootstrap_cash(inner: &Rc<RefCell<HostInner>>) -> Result<(), String> {
    guard(inner)?;
    need(
        (inner.borrow().deps.has_read_headroom)(1).map_err(codeless)?,
        "BOOTSTRAP_HEADROOM",
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
    inner.borrow_mut().bootstrap_permits.borrow_mut().insert(
        bd_ptr,
        json!({"endpoint": endpoint, "beforeNetworkPtr": bn_ptr as u64}),
    );
    let result: Result<(), String> = (|| {
        // api.dispatch({method:'GET',endpoint,beforeDispatch,beforeNetwork})
        let response = {
            let api = inner.borrow().deps.api.borrow().dispatch.clone();
            let guard_err = |c: &str| crate::transport::OrderTransportError {
                code: c.to_string(),
                outcome: "unknown".into(),
                may_have_executed: false,
                retry_same_request_only: false,
                retry_after_ms: 0.0,
                status: None,
                api_code: None,
                semantic_mismatch: false,
            };
            let bd_t = |p: &V, s: &V| -> Result<bool, crate::transport::OrderTransportError> {
                before_dispatch(p, s).map_err(|c| guard_err(&c))
            };
            let bn_t = |p: &V, s: &V| -> Result<bool, crate::transport::OrderTransportError> {
                before_network(p, s).map_err(|c| guard_err(&c))
            };
            api(&json!({"method": "GET", "endpoint": endpoint}), Some(&bd_t as &TGuard), Some(&bn_t as &TGuard))
                .map_err(|e| codeless(e.message))?
        };
        need(
            js_int(get(&response, "status")) == Some(200),
            "BOOTSTRAP_CASH_HTTP",
        )?;
        raw_of(inner, &response, &json!({"method": "GET"}))?;
        observation(inner)?;
        Ok(())
    })();
    inner.borrow_mut().bootstrap_permits.borrow_mut().remove(&bd_ptr);
    result
}

fn status(inner: &Rc<RefCell<HostInner>>) -> V {
    let g = inner.borrow();
    let controller_status = g.controller.borrow().as_ref().map(|c| c.status());
    let budget_status = g.budget.borrow().as_ref().map(|b| b.status());
    let bridge_status = g.bridge.borrow().as_ref().map(|b| b.status());
    json!({
        "schemaVersion": 1,
        "hostVersion": 2,
        "upgradeHash": g.upgrade_hash,
        "originalHostCodeIdentityHash": g.original_host_code_identity_hash,
        "hostCodeIdentityHash": payload_hash(&g.code).unwrap_or_default(),
        "exitHintSkippedPackets": g.exit_hint_skipped_packets.get(),
        "exitHintCounterScope": "current_host_process",
        "lastExitHint": g.last_exit_hint.borrow().clone(),
        "mode": "baal_scaled_live_production_v1",
        "ownerId": "Codex",
        "runId": get(&g.scope, "runId"),
        "observedAt": iso((g.deps.now)()),
        "allocationHash": g.allocation_hash,
        "budgetInstalledHash": g.budget_installed_hash,
        "requestHash": g.request_hash,
        "entryExpiresAt": get(&g.request, "entryExpiresAt"),
        "lastStepAt": g.last_step_at.borrow().clone(),
        "lastAction": g.last_action.borrow().clone(),
        "lastFailure": g.last_failure.borrow().clone().map(|s| json!(s)).unwrap_or(V::Null),
        "busy": g.busy.get(),
        "closed": g.closed.get(),
        "failed": g.failed.get(),
        "capital": "1000.000000",
        "maximumActiveMarkets": 20,
        "controller": controller_status.unwrap_or(V::Null),
        "budget": budget_status.unwrap_or(V::Null),
        "bridge": bridge_status.unwrap_or(V::Null),
        "readBootstrapComplete": g.cash_receipt.borrow().is_some(),
        "profitabilityEstablished": false,
        "additionalApiClients": 0,
        "additionalPersistentProcesses": 0,
    })
}

fn publish(inner: &Rc<RefCell<HostInner>>, action: &str) -> Result<V, String> {
    {
        let g = inner.borrow();
        *g.last_action.borrow_mut() = action.to_string();
        *g.last_step_at.borrow_mut() = json!(iso((g.deps.now)()));
    }
    let out = status(inner);
    let stats_path = inner.borrow().runtime_dir.join("baal-scaled-live-stats.json");
    atomic_json(&stats_path, &out)
        .map_err(|e| format!("{e}:fatal:durableRecordingFailed"))?;
    Ok(out)
}

fn owns_order(inner: &Rc<RefCell<HostInner>>, order_id: &str, leg: &V) -> bool {
    let controller = inner.borrow().controller.borrow().clone();
    match controller {
        Some(c) => {
            let i = c.status().get("intent").cloned().unwrap_or(V::Null);
            get(&i, "orderId").as_str() == Some(order_id)
                && get(&i, "marketId") == get(leg, "marketId")
                && get(&i, "exchangeId") == get(leg, "exchangeId")
        }
        None => false,
    }
}

fn create_bridge(inner: &Rc<RefCell<HostInner>>) -> Result<(), String> {
    if inner.borrow().bridge.borrow().is_some() {
        return Ok(());
    }
    let (scope, allocation, universe_members, deps) = {
        let g = inner.borrow();
        (
            g.scope.clone(),
            g.allocation.clone(),
            g.deps.universe.clone(),
            (),
        )
    };
    let _ = deps;
    let g = inner.borrow();
    let mut uni = universe_members.as_object().cloned().unwrap_or_default();
    uni.insert("exchanges".into(), get(&scope, "exchanges").clone());
    let api_dispatch = g.deps.api.borrow().dispatch.clone();
    let receipt_of_inner = inner.clone();
    let account_lease = g.deps.get_account_lease.clone();
    let inner_budget = inner.clone();
    let inner_budget2 = inner.clone();
    let inner_census = inner.clone();
    let inner_budget_status = inner.clone();
    let inner_owns = inner.clone();
    let inner_binding = inner.clone();
    let inner_can = inner.clone();
    let has_read_headroom = g.deps.has_read_headroom.clone();
    let now = g.deps.now.clone();
    let bridge = create_baal_scaled_no_evidence_bridge(
        &scope,
        &allocation,
        &V::Object(uni),
        BaalScaledEvidenceBridgeDeps {
            dispatch: Rc::new(
                move |spec: &V,
                      bd: Option<&crate::baal_scaled_no_evidence_bridge_v1::Guard>,
                      bn: Option<&crate::baal_scaled_no_evidence_bridge_v1::Guard>|
                      -> Result<V, String> {
                    let guard_err = |c: &str| crate::transport::OrderTransportError {
                        code: c.to_string(),
                        outcome: "unknown".into(),
                        may_have_executed: false,
                        retry_same_request_only: false,
                        retry_after_ms: 0.0,
                        status: None,
                        api_code: None,
                        semantic_mismatch: false,
                    };
                    let bd_t = |p: &V, s: &V| -> Result<bool, crate::transport::OrderTransportError> {
                        bd.map(|g| g(p, s).map_err(|c| guard_err(&c)))
                            .unwrap_or(Ok(false))
                    };
                    let bn_t = |p: &V, s: &V| -> Result<bool, crate::transport::OrderTransportError> {
                        bn.map(|g| g(p, s).map_err(|c| guard_err(&c)))
                            .unwrap_or(Ok(false))
                    };
                    api_dispatch(
                        spec,
                        bd.map(|_| &bd_t as &TGuard),
                        bn.map(|_| &bn_t as &TGuard),
                    )
                    .map_err(|e| e.message)
                },
            ),
            receipt_of: Rc::new(move |r: &V, meta: &V| raw_of(&receipt_of_inner, r, meta)),
            get_account_lease: Rc::new(move || Ok((account_lease)())),
            get_global_risk: Rc::new(move || {
                inner_budget
                    .borrow()
                    .budget
                    .borrow()
                    .as_ref()
                    .map(|b| b.global_risk())
                    .unwrap_or_else(|| Err("BAAL_SCALED_EVIDENCE_READ_FAILED".to_string()))
            }),
            get_account_risk: Some(Rc::new(move || {
                inner_budget2
                    .borrow()
                    .budget
                    .borrow()
                    .as_ref()
                    .map(|b| b.global_risk())
                    .unwrap_or_else(|| Err("BAAL_SCALED_EVIDENCE_READ_FAILED".to_string()))
            })),
            get_owned_census: Rc::new(move |ex: &str| {
                Ok(inner_census
                    .borrow()
                    .controller
                    .borrow()
                    .as_ref()
                    .map(|c| c.inventory(ex))
                    .unwrap_or(V::Null))
            }),
            get_installed_budget: Rc::new(move || {
                Ok(inner_budget_status
                    .borrow()
                    .controller
                    .borrow()
                    .as_ref()
                    .map(|c| c.funding_reservation_status())
                    .unwrap_or(V::Null))
            }),
            owns_order: Rc::new(move |oid: &str, leg: &V| Ok(owns_order(&inner_owns, oid, leg))),
            get_order_binding: Rc::new(move |oid: &str| {
                Ok(inner_binding
                    .borrow()
                    .controller
                    .borrow()
                    .as_ref()
                    .map(|c| c.order_binding(oid))
                    .unwrap_or(V::Null))
            }),
            can_read: Rc::new(move || allowed(&inner_can)),
            has_read_headroom,
            now,
        },
    )?;
    *inner.borrow().bridge.borrow_mut() = Some(Rc::new(bridge));
    Ok(())
}

fn candidates(inner: &Rc<RefCell<HostInner>>) -> Result<Vec<V>, String> {
    let (hints, positions, controller, scope, catalog, request, universe, now) = {
        let g = inner.borrow();
        (
            g.deps
                .get_pair_read_hints
                .as_ref()
                .map(|f| f())
                .unwrap_or(V::Null),
            g.positions_receipt.borrow().clone(),
            g.controller.borrow().clone(),
            g.scope.clone(),
            g.catalog.clone(),
            g.request.clone(),
            g.deps.universe.clone(),
            g.deps.now.clone(),
        )
    };
    if hints.is_null() {
        return Ok(Vec::new());
    }
    let quotes = hints
        .get("quoteCaptures")
        .filter(|v| !v.is_null())
        .or_else(|| hints.get("quotes").filter(|v| !v.is_null()))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let mut excluded: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some(pr) = positions {
        let rows: V = serde_json::from_str(sget(&pr, "bodyText").unwrap_or(""))
            .unwrap_or(json!({"positions": []}));
        for p in get(&rows, "positions").as_array().cloned().unwrap_or_default() {
            if js_truthy(Some(get(&p, "quantity"))) {
                if let Some(ex) = sget(&p, "exchangeId") {
                    excluded.insert(ex.to_string());
                }
            }
        }
    }
    if let Some(c) = controller {
        let inv = c.status().get("inventory").cloned().unwrap_or(json!({}));
        if let Some(m) = inv.as_object() {
            for (ex, p) in m {
                if get(p, "quantity").as_f64().unwrap_or(0.0) > 0.0 {
                    excluded.insert(ex.clone());
                }
            }
        }
    }
    let mut captured: Vec<V> = Vec::new();
    for q in quotes.as_array().cloned().unwrap_or_default().iter() {
        let mut c = q.as_object().cloned().unwrap_or_default();
        c.insert("tournamentId".into(), json!(T));
        if !c.contains_key("knownAt") || c["knownAt"].is_null() {
            c.insert("knownAt".into(), q.get("receivedAt").cloned().unwrap_or(V::Null));
        }
        if !c.contains_key("sourceHash") || c["sourceHash"].is_null() {
            c.insert("sourceHash".into(), get(&hints, "sourceHash").clone());
        }
        if c.get("quoteHash").is_none_or(V::is_null) {
            let h = crate::party_winner_no_screen_v1::party_winner_quote_hash(&V::Object(c.clone()))
                .map_err(codeless)?;
            c.insert("quoteHash".into(), json!(h));
        }
        captured.push(V::Object(c));
    }
    let mut uni = universe.as_object().cloned().unwrap_or_default();
    uni.insert("exchanges".into(), get(&scope, "exchanges").clone());
    let screen = crate::party_winner_no_screen_v1::screen_party_winner_no_pairs(&json!({
        "universe": V::Object(uni),
        "rootCaptures": get(&catalog, "rootCaptures"),
        "quoteCaptures": captured,
        "account": V::Null,
        "now": iso(now()),
    }))
    .map_err(codeless)?;
    let now_ms = now();
    let allowed_markets: Vec<String> = get(&request, "allowedMarkets")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    let cooldowns = inner.borrow().cooldowns.borrow().clone();
    let pairs: Vec<V> = get(&screen, "pairs")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| {
            get(p, "indicativeMathValid") == &V::Bool(true)
                && get(p, "quotesCurrentAndCofresh") == &V::Bool(true)
                && get(p, "exchangeIds")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .all(|ex| !excluded.contains(ex.as_str().unwrap_or("")))
                && get(p, "protectiveSurplusQualified") == &V::Bool(true)
                && get(p, "marketIds")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .all(|mid| allowed_markets.contains(&mid.as_str().unwrap_or("").to_string()))
                && !get(p, "marketIds")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .any(|mid| {
                        cooldowns
                            .get(mid.as_str().unwrap_or(""))
                            .cloned()
                            .unwrap_or(0.0)
                            > now_ms
                    })
        })
        .map(|p| {
            let mids = get(&p, "marketIds").as_array().cloned().unwrap_or_default();
            let exs = get(&p, "exchangeIds").as_array().cloned().unwrap_or_default();
            json!(mids
                .iter()
                .enumerate()
                .map(|(i, mid)| json!({"marketId": mid, "exchangeId": exs.get(i).cloned().unwrap_or(V::Null)}))
                .collect::<Vec<V>>())
        })
        .collect();
    Ok(pairs)
}

// openBaalScaledLiveHost({runtimeDir,workspace,universe,api,receiptOf,
//   getAccountLease,getOtherState,getLegacyRisk,getPairReadHints,getReadHint,
//   canOperate,hasReadHeadroom,now})
pub fn open_baal_scaled_live_host_v2(
    runtime_dir: &Path,
    workspace: &Path,
    deps: BaalScaledLiveHostV2Deps,
) -> Result<Option<BaalScaledLiveHostV2>, String> {
    let file = runtime_dir.join("baal-scaled-live-request.json");
    let upgrade_file = runtime_dir.join("baal-scaled-host-upgrade.json");
    if !upgrade_file.exists() {
        return Ok(None);
    }
    need(file.exists(), "ORIGINAL_REQUEST_MISSING")?;
    need(
        runtime_dir.is_absolute() && workspace.is_absolute(),
        "DEPENDENCIES",
    )?;
    let request = read_json_file(&file)?;
    let request_hash = payload_hash(&request).map_err(codeless)?;
    let keys: BTreeSet<&str> = [
        "schemaVersion", "kind", "ownerId", "runId", "tournamentId", "tournamentSlug",
        "universeHash", "allowedMarkets", "humanAllocationSourceHash", "authorityHash",
        "catalogHash", "codeIdentity", "startedAt", "entryExpiresAt", "maximumActiveMarkets",
    ]
    .into_iter()
    .collect();
    let actual_keys: BTreeSet<String> = request
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let run_id_ok = sget(&request, "runId").is_some_and(|r| {
        r.strip_prefix("baal-scaled-no-").is_some_and(|t| {
            !t.is_empty()
                && t.len() <= 80
                && t.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    });
    need(
        actual_keys == keys.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>()
            && get(&request, "schemaVersion") == &json!(1)
            && sget(&request, "kind") == Some("baal_scaled_live_production_v1")
            && sget(&request, "ownerId") == Some("Codex")
            && run_id_ok
            && sget(&request, "tournamentId") == Some(T)
            && sget(&request, "tournamentSlug") == Some("midterm-elections")
            && get(&request, "maximumActiveMarkets") == &json!(20),
        "ROOT_REQUEST",
    )?;
    let capital_file = runtime_dir.join("baal-capital-allocation.json");
    let authority_file = runtime_dir.join("paired-live-authority.json");
    let catalog_file = runtime_dir.join("baal-scaled-rootcatalog.json");
    let capital = read_json_file(&capital_file)?;
    let authority = read_json_file(&authority_file)?;
    let catalog = read_json_file(&catalog_file)?;
    need(
        get(&request, "humanAllocationSourceHash").as_str()
            == Some(payload_hash(&capital).map_err(codeless)?.as_str())
            && get(&capital, "schemaVersion") == &json!(1)
            && sget(&capital, "kind") == Some("human_directed_baal_capital_allocation_v1")
            && sget(&capital, "ownerId") == Some("Codex")
            && sget(&capital, "allocatedNominalCapital") == Some("1000.000000")
            && sget(&capital, "maximumOrderNominalCost") == Some("25.000000")
            && sget(&capital, "maximumMarketNominalExposure") == Some("100.000000")
            && sget(&capital, "maximumEventNominalExposure") == Some("200.000000")
            && get(&capital, "maximumSharesPerOrder") == &json!(100)
            && get(&capital, "maximumActiveMarkets") == &json!(20)
            && sget(&capital, "fee") == Some("0.000000"),
        "HUMAN_CAPITAL_SOURCE",
    )?;
    need(
        get(&request, "authorityHash").as_str()
            == Some(payload_hash(&authority).map_err(codeless)?.as_str())
            && sget(&authority, "kind") == Some("direct_human_authorized_bot_update")
            && sget(&authority, "ownerId") == Some("Codex")
            && sget(&authority, "threadId") == Some("01a0f8cc-b9bb-77d2-9f4c-2844396f1b79")
            && get(&request, "catalogHash").as_str()
                == Some(payload_hash(&catalog).map_err(codeless)?.as_str())
            && get(&catalog, "rootCaptures").is_array()
            && get(&catalog, "tournamentId").as_str() == Some(T),
        "AUTHORITY_OR_CATALOG",
    )?;
    let universe = &deps.universe;
    need(
        get(universe, "tournamentId").as_str() == Some(T)
            && get(universe, "hash") == get(&request, "universeHash")
            && get(universe, "exchanges").is_array()
            && get(&request, "allowedMarkets").as_array().is_some_and(|a| {
                a.len() >= 2
                    && a.len() <= 300
                    && a.iter().filter_map(V::as_str).collect::<BTreeSet<_>>().len() == a.len()
                    && a.iter().all(id)
            }),
        "ALLOWED_UNIVERSE",
    )?;
    let allowed_markets: Vec<V> = get(&request, "allowedMarkets").as_array().cloned().unwrap();
    let members: Vec<V> = get(universe, "exchanges")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| allowed_markets.contains(get(m, "marketId")))
        .collect();
    need(
        members.len() == allowed_markets.len()
            && members
                .iter()
                .all(|m| sget(m, "option") == Some("YES") && id(get(m, "exchangeId"))),
        "EXACT_ALLOWED_MEMBERS",
    )?;
    need(
        instant_v(get(&request, "entryExpiresAt"))? > instant_v(get(&request, "startedAt"))?
            && instant_v(get(&request, "entryExpiresAt"))? - instant_v(get(&request, "startedAt"))?
                <= 3600000
            && instant_v(get(&request, "startedAt"))? <= (deps.now)() as i64,
        "NEW_ENTRY_WINDOW",
    )?;
    let original_code = baal_scaled_live_identity(workspace)?;
    let original_host_code_identity_hash = payload_hash(&original_code).map_err(codeless)?;
    need(
        original_host_code_identity_hash
            == payload_hash(get(&request, "codeIdentity")).map_err(codeless)?,
        "FROZEN_ORIGINAL_HOST_SOURCE",
    )?;
    let code = baal_scaled_live_v2_identity(workspace)?;
    let upgrade = read_json_file(&upgrade_file)?;
    let upgrade_hash = payload_hash(&upgrade).map_err(codeless)?;
    let upgrade_keys: BTreeSet<&str> = [
        "schemaVersion", "kind", "ownerId", "runId", "tournamentId",
        "originalRequestHash", "originalHostCodeIdentityHash", "codeIdentity",
        "entryExpiresAt", "recordedAt",
    ]
    .into_iter()
    .collect();
    let upgrade_actual_keys: BTreeSet<String> = upgrade
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    need(
        upgrade_actual_keys
            == upgrade_keys
                .iter()
                .map(|s| s.to_string())
                .collect::<BTreeSet<_>>()
            && get(&upgrade, "schemaVersion") == &json!(1)
            && sget(&upgrade, "kind") == Some("baal_scaled_host_upgrade_v2")
            && get(&upgrade, "ownerId") == get(&request, "ownerId")
            && get(&upgrade, "runId") == get(&request, "runId")
            && get(&upgrade, "tournamentId").as_str() == Some(T)
            && get(&upgrade, "originalRequestHash").as_str() == Some(request_hash.as_str())
            && get(&upgrade, "originalHostCodeIdentityHash").as_str()
                == Some(original_host_code_identity_hash.as_str())
            && payload_hash(get(&upgrade, "codeIdentity")).map_err(codeless)?
                == payload_hash(&code).map_err(codeless)?
            && get(&upgrade, "entryExpiresAt") == get(&request, "entryExpiresAt")
            && instant_v(get(&upgrade, "recordedAt"))?
                >= instant_v(get(&request, "startedAt"))?
            && instant_v(get(&upgrade, "recordedAt"))? <= (deps.now)() as i64,
        "HOST_UPGRADE_BINDING",
    )?;
    let scope = json!({
        "ownerId": "Codex",
        "runId": get(&request, "runId"),
        "tournamentId": T,
        "tournamentSlug": get(&request, "tournamentSlug"),
        "exchanges": members.iter().map(|m| json!({"marketId": get(m, "marketId"), "exchangeId": get(m, "exchangeId")})).collect::<Vec<V>>(),
    });
    let allocation = json!({
        "schemaVersion": 1,
        "ownerId": "Codex",
        "runId": get(&request, "runId"),
        "tournamentId": T,
        "capital": "1000.000000",
        "maximumOrderCost": "25.000000",
        "maximumMarketCost": "100.000000",
        "maximumEventCost": "200.000000",
        "maximumSharesPerOrder": 100,
        "maximumActiveMarkets": 20,
        "humanAllocationSourceHash": get(&request, "humanAllocationSourceHash"),
        "recordedAt": get(&request, "startedAt"),
    });
    let allocation_hash = payload_hash(&allocation).map_err(codeless)?;
    let budget_installed_hash = payload_hash(&json!({
        "scope": scope,
        "allocation": allocation,
        "requestHash": request_hash,
        "authorityHash": get(&request, "authorityHash"),
    }))
    .map_err(codeless)?;
    let activation = json!({
        "schemaVersion": 1,
        "mode": "baal_scaled_no_production_v1",
        "ownerId": "Codex",
        "runId": get(&request, "runId"),
        "tournamentId": T,
        "allocationHash": allocation_hash,
        "scopeHash": payload_hash(&scope).map_err(codeless)?,
        "authorityHash": get(&request, "authorityHash"),
        "budgetInstalledHash": budget_installed_hash,
        "codeIdentityHash": payload_hash(&baal_scaled_no_controller_identity()?).map_err(codeless)?,
        "startedAt": get(&request, "startedAt"),
        "entryExpiresAt": get(&request, "entryExpiresAt"),
    });
    let dir = runtime_dir
        .join("baal-scaled-live-runs")
        .join(sget(&request, "runId").unwrap_or(""));
    let controller_dir = dir.join("controller");
    let budget_dir = dir.join("budget");
    fs::create_dir_all(&dir).map_err(|e| codeless(e.to_string()))?;

    let inner = Rc::new(RefCell::new(HostInner {
        file,
        capital_file,
        authority_file,
        catalog_file,
        runtime_dir: runtime_dir.to_path_buf(),
        workspace: workspace.to_path_buf(),
        request,
        request_hash,
        catalog: catalog.clone(),
        scope,
        allocation,
        allocation_hash,
        budget_installed_hash,
        activation,
        code,
        upgrade_file,
        upgrade,
        upgrade_hash,
        original_host_code_identity_hash,
        controller: RefCell::new(None),
        budget: RefCell::new(None),
        bridge: RefCell::new(None),
        cash_receipt: RefCell::new(None),
        positions_receipt: RefCell::new(None),
        closed: Cell::new(false),
        failed: Cell::new(false),
        busy: Cell::new(false),
        last_failure: RefCell::new(None),
        last_action: RefCell::new("initialized_read_only".to_string()),
        last_step_at: RefCell::new(V::Null),
        bootstrap_permits: RefCell::new(HashMap::new()),
        cooldowns: RefCell::new(HashMap::new()),
        sell_checks: RefCell::new(HashMap::new()),
        exit_hint_skipped_packets: Cell::new(0),
        last_exit_hint: RefCell::new(V::Null),
        deps,
    }));

    let build: Result<(), String> = (|| {
        guard(&inner)?;
        let (controller_dir, budget_dir, scope, allocation, activation) = {
            let g = inner.borrow();
            (
                g.runtime_dir
                    .join("baal-scaled-live-runs")
                    .join(sget(&g.request, "runId").unwrap_or(""))
                    .join("controller"),
                g.runtime_dir
                    .join("baal-scaled-live-runs")
                    .join(sget(&g.request, "runId").unwrap_or(""))
                    .join("budget"),
                g.scope.clone(),
                g.allocation.clone(),
                g.activation.clone(),
            )
        };
        let inner_budget_for_ctrl = inner.clone();
        let inner_budget_for_risk = inner.clone();
        let inner_bridge_for_ctrl = inner.clone();
        let inner_can_operate = inner.clone();
        let account_lease = inner.borrow().deps.get_account_lease.clone();
        let api_dispatch = inner.borrow().deps.api.borrow().dispatch.clone();
        let now = inner.borrow().deps.now.clone();
        let inner_raw = inner.clone();
        let controller = Rc::new(open_baal_scaled_no_controller(
            &controller_dir,
            &scope,
            &allocation,
            &activation,
            BaalScaledNoControllerDeps {
                dispatch: api_dispatch,
                raw_receipt_of: Rc::new(move |r: &V, meta: &V| raw_of(&inner_raw, r, meta)),
                can_operate: Rc::new(move || allowed(&inner_can_operate)),
                assert_account_lease: account_lease,
                install_funding_reservation: Rc::new(move |r: &V| {
                    inner_budget_for_ctrl
                        .borrow()
                        .budget
                        .borrow()
                        .as_ref()
                        .map(|b| b.install_funding_reservation(r))
                        .unwrap_or_else(|| Err("BAAL_SCALED_RESERVATION_UNAVAILABLE".to_string()))
                }),
                get_global_risk: Rc::new(move || {
                    inner_budget_for_risk
                        .borrow()
                        .budget
                        .borrow()
                        .as_ref()
                        .map(|b| b.global_risk())
                        .unwrap_or_else(|| Err("BAAL_SCALED_RISK_UNAVAILABLE".to_string()))
                }),
                get_evidence: Rc::new(move |c: &V| {
                    inner_bridge_for_ctrl
                        .borrow()
                        .bridge
                        .borrow()
                        .as_ref()
                        .map(|b| b.get_evidence(c))
                        .unwrap_or_else(|| Err("BAAL_SCALED_EVIDENCE_READ_FAILED".to_string()))
                }),
                now,
            },
        )?);
        *inner.borrow().controller.borrow_mut() = Some(controller.clone());
        let inner_ctrl_src = inner.clone();
        let inner_obs = inner.clone();
        let inner_can2 = inner.clone();
        let account_lease2 = inner.borrow().deps.get_account_lease.clone();
        let now2 = inner.borrow().deps.now.clone();
        let activation_hash = get(&inner.borrow().activation, "codeIdentityHash")
            .as_str()
            .unwrap_or("")
            .to_string();
        let budget_installed = inner.borrow().budget_installed_hash.clone();
        let budget = Rc::new(open_baal_scaled_shared_budget(
            &budget_dir,
            &controller_dir,
            &inner.borrow().allocation.clone(),
            &inner.borrow().scope.clone(),
            &budget_installed,
            &activation_hash,
            BaalScaledSharedBudgetDeps {
                assert_account_lease: account_lease2,
                get_controller_source: Rc::new(move || {
                    inner_ctrl_src
                        .borrow()
                        .controller
                        .borrow()
                        .as_ref()
                        .map(|c| c.journal_source())
                        .unwrap_or_else(|| Err("BAAL_SHARED_BUDGET_ACTOR_MISSING".to_string()))
                }),
                observe_global_risk: Rc::new(move || observation(&inner_obs)),
                can_operate: Rc::new(move || allowed(&inner_can2)),
                now: now2,
            },
        )?);
        *inner.borrow().budget.borrow_mut() = Some(budget);
        Ok(())
    })();
    if let Err(e) = build {
        // try{...}catch: close anything built before rethrowing.
        for h in ["bridge", "budget", "controller"] {
            let _ = h;
            let g = inner.borrow();
            match h {
                "bridge" => drop(g.bridge.borrow().as_ref().map(|b| {
                    let _ = b.close();
                })),
                "budget" => drop(g.budget.borrow().as_ref().map(|b| {
                    let _ = b.close();
                })),
                "controller" => drop(g.controller.borrow().as_ref().map(|c| {
                    let _ = c.close();
                })),
                _ => {}
            }
        }
        return Err(e);
    }
    Ok(Some(BaalScaledLiveHostV2 { inner }))
}

impl BaalScaledLiveHostV2 {
    // exitHint(exchangeId)
    fn exit_hint(&self, exchange_id: &str) -> V {
        let inner = &self.inner;
        let owned = {
            let g = inner.borrow();
            g.controller
                .borrow()
                .as_ref()
                .map(|c| c.inventory(exchange_id))
                .unwrap_or(V::Null)
        };
        let mut quote = V::Null;
        let try_quote: Result<V, String> = (|| {
            let (hints, universe_hash) = {
                let g = inner.borrow();
                (
                    g.deps
                        .get_pair_read_hints
                        .as_ref()
                        .map(|f| f())
                        .unwrap_or(V::Null),
                    get(&g.deps.universe, "hash").clone(),
                )
            };
            let quotes = hints
                .get("quoteCaptures")
                .filter(|v| !v.is_null())
                .or_else(|| hints.get("quotes").filter(|v| !v.is_null()))
                .cloned()
                .unwrap_or(V::Null);
            need(quotes.is_array(), "EXIT_HINT_ARRAY")?;
            need(
                (hints.get("tournamentId").is_none()
                    || get(&hints, "tournamentId").as_str() == Some(T))
                    && (hints.get("universeHash").is_none()
                        || get(&hints, "universeHash") == &universe_hash),
                "EXIT_HINT_WRAPPER_SCOPE",
            )?;
            let market_id = get(get(&owned, "proof"), "marketId").clone();
            let matches: Vec<V> = quotes
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|q| sget(q, "exchangeId") == Some(exchange_id))
                .collect();
            need(
                matches.len() == 1 && get(&matches[0], "marketId") == &market_id,
                "EXIT_HINT_AMBIGUOUS",
            )?;
            let q = &matches[0];
            let tournament_id = if !q.get("tournamentId").is_none_or(V::is_null) {
                get(q, "tournamentId").clone()
            } else if get(&hints, "tournamentId").as_str() == Some(T)
                && get(&hints, "universeHash") == &universe_hash
            {
                json!(T)
            } else {
                V::Null
            };
            let mut out = q.as_object().cloned().unwrap_or_default();
            out.insert("tournamentId".into(), tournament_id);
            Ok(V::Object(out))
        })();
        quote = try_quote.unwrap_or(V::Null);
        let (scope, intent, now) = {
            let g = inner.borrow();
            (
                g.scope.clone(),
                g.controller
                    .borrow()
                    .as_ref()
                    .map(|c| get(&c.status(), "intent").clone())
                    .unwrap_or(V::Null),
                g.deps.now.clone(),
            )
        };
        let hint = owned_no_exit_read_hint(&json!({
            "scope": scope,
            "owned": owned,
            "quote": quote,
            "activeIntent": intent,
            "now": iso(now()),
        }));
        // Curated counters describe a read decision, never an executable mark.
        let mut last = Map::new();
        last.insert("at".into(), get(&hint, "at").clone());
        last.insert("marketId".into(), get(get(&owned, "proof"), "marketId").clone());
        last.insert("exchangeId".into(), json!(exchange_id));
        last.insert("reason".into(), get(&hint, "reason").clone());
        last.insert(
            "skipPreparationReads".into(),
            json!(get(&hint, "skipPreparationReads") == &V::Bool(true)),
        );
        last.insert(
            "ownedProofHash".into(),
            get(get(&owned, "proof"), "verificationHash").clone(),
        );
        if !hint.get("hintHash").is_none_or(V::is_null) {
            last.insert("hintHash".into(), get(&hint, "hintHash").clone());
            last.insert("quoteSourceHash".into(), get(&hint, "quoteSourceHash").clone());
            last.insert(
                "quoteReceivedAt".into(),
                get(&hint, "quoteReceivedAt").clone(),
            );
            last.insert(
                "hintAgeFromRequestMs".into(),
                get(&hint, "hintAgeFromRequestMs").clone(),
            );
        }
        last.insert("orderEligible".into(), json!(false));
        *inner.borrow().last_exit_hint.borrow_mut() = V::Object(last);
        hint
    }

    // step()
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
            let (cash_receipt, budget_claimed) = {
                let g = inner.borrow();
                (
                    g.cash_receipt.borrow().clone(),
                    g.budget.borrow().as_ref().map(|b| b.status()).unwrap_or(json!({"claimed": false})),
                )
            };
            let cash_stale = match &cash_receipt {
                Some(r) => {
                    (inner.borrow().deps.now)() as i64
                        - instant_v(get(r, "requestStartedAt"))?
                        > 15000
                }
                None => true,
            };
            if cash_stale {
                bootstrap_cash(inner)?;
            }
            let claimed = get(&budget_claimed, "claimed") == &V::Bool(true);
            if !claimed {
                inner
                    .borrow()
                    .controller
                    .borrow()
                    .as_ref()
                    .ok_or_else(|| "BAAL_SCALED_HOST_INTERNAL".to_string())?
                    .refresh_reservation()?;
                create_bridge(inner)?;
            } else {
                create_bridge(inner)?;
            }
            let state = inner
                .borrow()
                .controller
                .borrow()
                .as_ref()
                .map(|c| c.status())
                .unwrap_or(V::Null);
            let intent = get(&state, "intent").clone();
            if !intent.is_null() {
                if get(&intent, "orderId").is_null() {
                    let phase = get(&state, "phase").as_str().unwrap_or("").to_string();
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
                        return publish(inner, "expired_unfetched_prepared_abandoned");
                    }
                    if phase == "prepared" && fetches == 0 && now_ms < expires {
                        if sget(&intent, "action") == Some("buy") {
                            inner
                                .borrow()
                                .bridge
                                .borrow()
                                .as_ref()
                                .unwrap()
                                .refresh_buy(get(&intent, "packageLegs"))?;
                        } else {
                            let ex = sget(&intent, "exchangeId").unwrap_or("").to_string();
                            let leg = get(&inner.borrow().scope, "exchanges")
                                .as_array()
                                .cloned()
                                .unwrap_or_default()
                                .into_iter()
                                .find(|m| sget(m, "exchangeId") == Some(ex.as_str()))
                                .unwrap_or(V::Null);
                            inner
                                .borrow()
                                .bridge
                                .borrow()
                                .as_ref()
                                .unwrap()
                                .refresh_sell(&leg)?;
                        }
                        inner
                            .borrow()
                            .controller
                            .borrow()
                            .as_ref()
                            .unwrap()
                            .dispatch_prepared()?;
                        return publish(inner, "prepared_intent_dispatched_once");
                    }
                    return publish(
                        inner,
                        if phase == "prepared" && fetches == 0 {
                            "expired_unfetched_prepared_requires_operator_release"
                        } else {
                            "unknown_acceptance_requires_reconciliation"
                        },
                    );
                }
                let leg = json!({
                    "marketId": get(&intent, "marketId"),
                    "exchangeId": get(&intent, "exchangeId"),
                });
                let order_id = sget(&intent, "orderId").unwrap_or("").to_string();
                let bridge = inner.borrow().bridge.borrow().clone();
                let reconcile_result: Result<V, String> = (|| {
                    bridge
                        .as_ref()
                        .ok_or_else(|| "BAAL_SCALED_HOST_INTERNAL".to_string())?
                        .refresh_reconciliation(&order_id, &leg)?;
                    let evidence = bridge.as_ref().unwrap().get_evidence(&json!({
                        "operation": "reconcile",
                        "orderId": order_id,
                        "intentId": get(&intent, "intentId"),
                    }))?;
                    inner
                        .borrow()
                        .controller
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .reconcile(&evidence)?;
                    publish(inner, "terminal_order_reconciled")
                })();
                match reconcile_result {
                    Ok(v) => return Ok(v),
                    Err(e) => {
                        if e.contains(":fatal:") || e.contains("durableRecordingFailed") {
                            return Err(e);
                        }
                        let now_ms = (inner.borrow().deps.now)() as i64;
                        let expires =
                            instant_v(get(get(&intent, "body"), "expirationDate"))?;
                        if now_ms >= expires
                            && get(&intent, "cancelPending") != &V::Bool(true)
                        {
                            bridge
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
                        *inner.borrow().last_failure.borrow_mut() = Some(
                            host_code_of(&e).unwrap_or_else(|| "TERMINAL_SOURCE_PENDING".to_string()),
                        );
                        return publish(inner, "known_order_waiting_terminal_proof");
                    }
                }
            }
            let inventory = get(&state, "inventory").as_object().cloned().unwrap_or_default();
            for (ex, position) in inventory {
                let now_ms = (inner.borrow().deps.now)() as i64;
                let last_check = inner.borrow().sell_checks.borrow().get(&ex).cloned().unwrap_or(0.0);
                if js_int(get(&position, "quantity")).unwrap_or(0) <= 0
                    || (now_ms as f64 - last_check) < 15000.0
                {
                    continue;
                }
                inner
                    .borrow()
                    .sell_checks
                    .borrow_mut()
                    .insert(ex.clone(), now_ms as f64);
                let hint = self.exit_hint(&ex);
                if get(&hint, "skipPreparationReads") == &V::Bool(true) {
                    self.inner
                        .borrow()
                        .exit_hint_skipped_packets
                        .set(self.inner.borrow().exit_hint_skipped_packets.get() + 1);
                    continue;
                }
                let leg = get(&inner.borrow().scope, "exchanges")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|m| sget(m, "exchangeId") == Some(ex.as_str()))
                    .unwrap_or(V::Null);
                let sell_result: Result<V, String> = (|| {
                    let bridge = inner.borrow().bridge.borrow().clone();
                    bridge
                        .as_ref()
                        .ok_or_else(|| "BAAL_SCALED_HOST_INTERNAL".to_string())?
                        .refresh_sell(&leg)?;
                    let controller = inner.borrow().controller.borrow().clone();
                    controller.as_ref().unwrap().prepare_sell(&ex, None)?;
                    controller.as_ref().unwrap().dispatch_prepared()?;
                    publish(inner, "covered_profitable_sell_dispatched")
                })();
                match sell_result {
                    Ok(v) => return Ok(v),
                    Err(e) => {
                        let has_intent = inner
                            .borrow()
                            .controller
                            .borrow()
                            .as_ref()
                            .map(|c| !c.status().get("intent").cloned().unwrap_or(V::Null).is_null())
                            .unwrap_or(false);
                        if e.contains(":fatal:") || e.contains("durableRecordingFailed") || has_intent {
                            return Err(e);
                        }
                        *inner.borrow().last_failure.borrow_mut() = Some(
                            host_code_of(&e)
                                .unwrap_or_else(|| "NO_FRESH_PROTECTED_EXIT".to_string()),
                        );
                    }
                }
            }
            let now_ms = (inner.borrow().deps.now)() as i64;
            let request = inner.borrow().request.clone();
            if now_ms >= instant_v(get(&request, "entryExpiresAt")).unwrap_or(0) {
                return publish(inner, "entry_window_expired_owned_risk_retained");
            }
            if now_ms < instant_v(get(&request, "startedAt")).unwrap_or(0) {
                return publish(inner, "entry_window_not_started");
            }
            for pair in candidates(inner)? {
                let pair_result: Result<V, String> = (|| {
                    let bridge = inner.borrow().bridge.borrow().clone();
                    bridge
                        .as_ref()
                        .ok_or_else(|| "BAAL_SCALED_HOST_INTERNAL".to_string())?
                        .refresh_buy(&pair)?;
                    let proposal = bridge.as_ref().unwrap().get_buy_proposal()?;
                    if get(&proposal, "proposalReady") != &V::Bool(true) {
                        for l in pair.as_array().cloned().unwrap_or_default() {
                            inner.borrow().cooldowns.borrow_mut().insert(
                                sget(&l, "marketId").unwrap_or("").to_string(),
                                now_ms as f64 + 300000.0,
                            );
                        }
                        *inner.borrow().last_failure.borrow_mut() = Some(
                            sget(&proposal, "reason").unwrap_or("").to_string(),
                        );
                        return publish(inner, "candidate_rejected_no_order");
                    }
                    let controller = inner.borrow().controller.borrow().clone();
                    controller.as_ref().unwrap().prepare_buy()?;
                    controller.as_ref().unwrap().dispatch_prepared()?;
                    publish(inner, "scaled_buy_dispatched")
                })();
                match pair_result {
                    Ok(v) => return Ok(v),
                    Err(e) => {
                        let has_intent = inner
                            .borrow()
                            .controller
                            .borrow()
                            .as_ref()
                            .map(|c| !c.status().get("intent").cloned().unwrap_or(V::Null).is_null())
                            .unwrap_or(false);
                        if e.contains(":fatal:") || e.contains("durableRecordingFailed") || has_intent {
                            return Err(e);
                        }
                        backoff(inner, &e)?;
                        for l in pair.as_array().cloned().unwrap_or_default() {
                            inner.borrow().cooldowns.borrow_mut().insert(
                                sget(&l, "marketId").unwrap_or("").to_string(),
                                now_ms as f64 + 300000.0,
                            );
                        }
                        *inner.borrow().last_failure.borrow_mut() = Some(
                            host_code_of(&e).unwrap_or_else(|| "CANDIDATE_FAILED".to_string()),
                        );
                        return publish(inner, "candidate_rejected_no_order");
                    }
                }
            }
            publish(inner, "no_fresh_qualified_relative_value_candidate")
        })();
        match run {
            Ok(v) => Ok(v),
            Err(e) => {
                backoff(inner, &e)?;
                *self.inner.borrow().last_failure.borrow_mut() = Some(
                    host_code_of(&e)
                        .unwrap_or_else(|| "BAAL_SCALED_HOST_STEP_FAILED".to_string()),
                );
                if e.contains(":fatal:") || e.contains("durableRecordingFailed") {
                    return Err(e);
                }
                publish(inner, "blocked_existing_records_retained")
            }
        }
    }

    pub fn status(&self) -> V {
        status(&self.inner)
    }

    // authorizes(spec, beforeDispatch, beforeNetwork) — read side:
    // bootstrap permit or bridge-issued guard.
    pub fn authorizes(
        &self,
        spec: &V,
        before_dispatch: &HostGuard,
        before_network: &HostGuard,
    ) -> bool {
        let r: Result<bool, String> = (|| {
            guard(&self.inner)?;
            let g = self.inner.borrow();
            let key = before_dispatch as *const HostGuard as *const () as usize;
            let p = g.bootstrap_permits.borrow().get(&key).cloned();
            let bootstrap_ok = p.is_some()
                && sget(spec, "method") == Some("GET")
                && get(spec, "endpoint") == get(p.as_ref().unwrap(), "endpoint")
                && (before_network as *const HostGuard as *const () as usize)
                    == get(p.as_ref().unwrap(), "beforeNetworkPtr")
                        .as_u64()
                        .unwrap_or(0) as usize
                && spec.get("body").is_none()
                && spec.get("bodyJson").is_none();
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

    // authorizes(spec) — write side: controller-issued transport guard.
    pub fn authorizes_write(
        &self,
        spec: &V,
        before_dispatch: Option<&TGuard<'_>>,
        before_network: Option<&TGuard<'_>>,
    ) -> bool {
        let r: Result<bool, String> = (|| {
            guard(&self.inner)?;
            let controller = self.inner.borrow().controller.borrow().clone();
            Ok(controller
                .as_ref()
                .map(|c| c.authorizes(spec, before_dispatch, before_network))
                .unwrap_or(false))
        })();
        r.unwrap_or(false)
    }

    pub fn on_fetch_start(&self, event: &V) -> Result<bool, String> {
        self.inner
            .borrow()
            .controller
            .borrow()
            .as_ref()
            .map(|c| c.on_fetch_start(event))
            .unwrap_or(Ok(false))
    }

    pub fn blocks_legacy_writes(&self) -> bool {
        true
    }

    pub fn allows_legacy(&self, spec: &V) -> bool {
        sget(spec, "method") == Some("GET")
    }

    pub fn get_read_hint(&self) -> V {
        self.inner
            .borrow()
            .deps
            .get_read_hint
            .as_ref()
            .map(|f| f())
            .unwrap_or(V::Null)
    }

    pub fn close(&self) -> Result<(), String> {
        need(!self.inner.borrow().busy.get(), "BUSY")?;
        if self.inner.borrow().closed.get() {
            return Ok(());
        }
        self.inner.borrow().closed.set(true);
        let mut first_err: Option<String> = None;
        let (bridge, budget, controller) = {
            let g = self.inner.borrow();
            (
                g.bridge.borrow().clone(),
                g.budget.borrow().clone(),
                g.controller.borrow().clone(),
            )
        };
        if let Some(b) = bridge {
            if let Err(e) = b.close() {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
        if let Some(b) = budget {
            if let Err(e) = b.close() {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
        if let Some(c) = controller {
            if let Err(e) = c.close() {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}
