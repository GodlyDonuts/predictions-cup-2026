// execution/baal-combined-custody-host-v6.mjs — the combined custody host.
// A separately authorized financial-owner epoch. Original financial journals
// are sealed read-only sources; their lease files fence every earlier host
// out.
//
// Port notes (same conventions as baal_scaled_live_host_v5):
// - JS async fns -> sync fns; `stage`/`timed` keep the timing arrays.
// - Mutable shared `api` object -> Rc<RefCell<CombinedCustodyApi>>; dispatch
//   is the caller's WireDispatch; bridge deps take an Rc<Dispatch> adapter
//   that wraps wire errors into a coded OrderTransportError.
// - The ported error channel is a String. `safeCode(e)` -> `safe_code(&str)`:
//   an all-caps message is the code; `\0`-prefixed (codeless) or prose strings
//   fall back to BAAL_CUSTODY_V6_OPERATION_FAILED. fatal/durableRecordingFailed
//   are signalled by ':fatal:'/'durableRecordingFailed' markers in the string.
// - `e.retryAfterMs` cannot be carried on the String channel, so `backoff`
//   mirrors the JS early return (documented no-op), as in sibling hosts.
// - Error code prefix stays BAAL_CUSTODY_V6_* exactly as the .mjs throws it.
use crate::baal_neutral_owned_counterpart_pilot_v2::{
    live_coordinator_identity, BaalNeutralOwnedCounterpartPilot, NeutralPilotDeps,
};
use crate::baal_owned_hedge_shared_budget_v2::{
    BaalOwnedHedgeSharedBudget, OwnedHedgeBudgetDeps,
};
use crate::baal_owned_exit_priority_v1::rank_baal_owned_exit_read_hints_v1;
use crate::baal_scaled_controller_legacy_replay_v1::{
    baal_scaled_no_controller_identity, replay_baal_scaled_controller_v1,
};
use crate::baal_scaled_live_host_v1::baal_scaled_live_identity;
use crate::baal_scaled_live_host_v2::baal_scaled_live_v2_identity;
use crate::baal_scaled_live_host_v3::baal_scaled_live_v3_identity;
use crate::baal_scaled_live_host_v5::baal_scaled_live_v5_identity;
use crate::core::{payload_hash, validate_execution_state};
use crate::frozen::collect_frozen_paper_files;
use crate::no_rest_reconciliation_v2::rounded_tournament_cash_interval;
use crate::owned_no_fresh_none_hedge_evidence_bridge_v1::NoneHedgeBridgeApi;
use crate::owned_no_fresh_none_hedge_evidence_bridge_v2::{
    NoneHedgeBridgeV2Deps, OwnedNoFreshNoneHedgeEvidenceBridgeV2,
};
use crate::paired_rest_controller::WireDispatch;
use crate::transport::{Dispatch, Guard as TGuard, OrderTransportError};
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::fixed::{amount, instant, price};
use baal_service_runtime::journal::{journal_lines, Journal, JournalReadOptions};
use baal_service_runtime::json_text::{js_int, js_truthy};
use baal_service_runtime::ledger::{acquire_owner_lease, OwnerLease};
use serde_json::{json, Map, Value as V};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Instant;

const T: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
const SOURCE_FILES: [&str; 7] = [
    "baal-scaled-live-request.json",
    "baal-scaled-host-upgrade.json",
    "baal-scaled-host-upgrade-v3.json",
    "baal-scaled-host-upgrade-v5.json",
    "baal-capital-allocation.json",
    "paired-live-authority.json",
    "baal-scaled-rootcatalog.json",
];

pub type HostGuard<'a> = dyn Fn(&V, &V) -> Result<bool, String> + 'a;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("BAAL_CUSTODY_V6_{code}"))
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
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn iso(n: f64) -> String {
    baal_service_runtime::contract::ms_to_iso(n)
}
fn instant_v(v: &V) -> f64 {
    instant(v.as_str().unwrap_or("")).map(|m| m as f64).unwrap_or(f64::NAN)
}
fn sha(b: &[u8]) -> String {
    baal_service_runtime::json_text::sha256(b)
}
fn same(a: &V, b: &V) -> bool {
    payload_hash(a).ok() == payload_hash(b).ok()
}
fn num(v: &V) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}
fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}
fn is_all_caps_code(s: &str) -> bool {
    let n = s.len();
    (2..=121).contains(&n)
        && s.bytes().next().is_some_and(|c| c.is_ascii_uppercase())
        && s.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}
fn safe_code(e: &str) -> String {
    if is_all_caps_code(e) {
        e.to_string()
    } else {
        "BAAL_CUSTODY_V6_OPERATION_FAILED".to_string()
    }
}
fn non_none(e: &str) -> bool {
    matches!(
        e,
        "COMBINED_NONE_EVIDENCE_NON_NONE_GRAPH"
            | "COMBINED_NONE_EVIDENCE_NON_NONE_COLLATERAL"
            | "NONE_HEDGE_EVIDENCE_NON_NONE_OR_UNSUPPORTED_GRAPH"
            | "NONE_HEDGE_EVIDENCE_NON_NONE_COLLATERAL"
    )
}
fn transport_err(code: &str) -> OrderTransportError {
    OrderTransportError {
        code: code.to_string(),
        status: None,
        api_code: None,
        outcome: "unknown".into(),
        may_have_executed: false,
        retry_same_request_only: false,
        retry_after_ms: 0.0,
        semantic_mismatch: false,
    }
}
fn lock_summary(identity: &V) -> Result<V, String> {
    let hash = payload_hash(identity)?;
    let mut public = identity.clone();
    if let V::Object(m) = &mut public {
        m.shift_remove("token");
    }
    if let V::Object(m) = &mut public {
        m.insert("identityHash".into(), json!(hash));
    }
    Ok(public)
}
fn sorted_keys(v: &V) -> Vec<String> {
    let mut keys: Vec<String> = v
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    keys
}

// The mutable shared api object the caller owns:
// {dispatch,manualReview,cooldown,next,onAdmission}.
pub struct CombinedCustodyApi {
    pub dispatch: Rc<WireDispatch>,
    pub manual_review: bool,
    pub cooldown: f64,
    pub next: f64,
    pub on_admission: Box<dyn FnMut(Option<f64>) -> Result<(), String>>,
}

pub struct CombinedCustodyHostV6Deps {
    pub api: Rc<RefCell<CombinedCustodyApi>>,
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub get_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub get_core_owner_record: Option<Rc<dyn Fn() -> Result<V, String>>>,
    pub get_other_state: Rc<dyn Fn() -> Result<V, String>>,
    pub get_legacy_risk: Rc<dyn Fn() -> Result<V, String>>,
    /// getPairReadHints — JS default `()=>null`.
    pub get_pair_read_hints: Option<Rc<dyn Fn() -> Result<V, String>>>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> Result<bool, String>>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct HostInner {
    deps: CombinedCustodyHostV6Deps,
    runtime_dir: PathBuf,
    workspace: PathBuf,
    universe: V,
    descriptor_file: PathBuf,
    descriptor_bytes_hash: String,
    descriptor_hash: String,
    descriptor: V,
    activation: V,
    migration: V,
    epoch: String,
    request: V,
    source_files: Vec<String>,
    source_hashes: Map<String, V>,
    code: V,
    code_files: Map<String, V>,
    scope: V,
    allocation: V,
    seed: V,
    old_code: V,
    dir: PathBuf,
    coordinator_dir: PathBuf,
    budget_dir: PathBuf,
    lease_dir: PathBuf,
    lease_journal_file: PathBuf,
    legacy_controller_dir: PathBuf,
    legacy_budget_dir: PathBuf,
    legacy_file: PathBuf,
    stop_file: PathBuf,
    coordinator: Option<Rc<BaalNeutralOwnedCounterpartPilot>>,
    budget: Option<Rc<BaalOwnedHedgeSharedBudget>>,
    financial_lease: Option<OwnerLease>,
    legacy_controller_fence: Option<OwnerLease>,
    legacy_budget_fence: Option<OwnerLease>,
    epoch_journal: Option<Journal>,
    epoch_receipt: V,
    raw_account: Option<V>,
    cash_receipt: Option<V>,
    closed: bool,
    failed: bool,
    busy: bool,
    last_failure: Option<String>,
    last_action: String,
    last_step_at: Option<String>,
    bridges: HashMap<String, Rc<OwnedNoFreshNoneHedgeEvidenceBridgeV2>>,
    selected_bridge: Option<Rc<OwnedNoFreshNoneHedgeEvidenceBridgeV2>>,
    permits: HashMap<usize, V>,
    timings: Vec<V>,
    stage_timings: Vec<V>,
    exit_skipped: i64,
    last_hint: V,
    sell_cursor: usize,
    hedge_tried_at: Option<f64>,
    last_graph_at: Option<String>,
    last_book_at: Option<String>,
    packet_ready_at: Option<Instant>,
    last_dispatch_timing: V,
    last_rejected_timing: V,
    last_exit_priority: V,
}

fn now(inner: &Rc<RefCell<HostInner>>) -> f64 {
    (inner.borrow().deps.now)()
}

// const stage=async(name,fn)=>{...timing arrays capped at 18/24...}
fn stage<F, T>(inner: &Rc<RefCell<HostInner>>, name: &str, f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    let start = Instant::now();
    let at = now(inner);
    let result = f();
    let mut entry = json!({
        "name": name,
        "startedAt": iso(at),
        "finishedAt": iso(now(inner)),
        "elapsedClockMs": now(inner) - at,
        "elapsedLocalMs": round3(start.elapsed().as_secs_f64() * 1000.0),
    });
    if let Err(e) = &result {
        entry
            .as_object_mut()
            .unwrap()
            .insert("errorCode".into(), json!(safe_code(e)));
    }
    let mut g = inner.borrow_mut();
    g.stage_timings.push(entry);
    if g.stage_timings.len() > 18 {
        g.stage_timings.remove(0);
    }
    drop(g);
    result
}

fn timed<F, T>(inner: &Rc<RefCell<HostInner>>, name: &str, f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    let start = Instant::now();
    let result = f();
    let mut g = inner.borrow_mut();
    g.timings.push(json!({
        "name": name,
        "durationMs": round3(start.elapsed().as_secs_f64() * 1000.0),
    }));
    if g.timings.len() > 24 {
        g.timings.remove(0);
    }
    drop(g);
    result
}

// const immutable=()=>{...}
fn immutable(inner: &Rc<RefCell<HostInner>>) -> Result<(), String> {
    let (closed, failed, stop_file, descriptor_file, descriptor_bytes_hash, workspace, code_files, runtime_dir, source_files, source_hashes) = {
        let g = inner.borrow();
        (
            g.closed,
            g.failed,
            g.stop_file.clone(),
            g.descriptor_file.clone(),
            g.descriptor_bytes_hash.clone(),
            g.workspace.clone(),
            g.code_files.clone(),
            g.runtime_dir.clone(),
            g.source_files.clone(),
            g.source_hashes.clone(),
        )
    };
    need(
        !closed && !failed && (inner.borrow().deps.can_operate)() && !stop_file.exists(),
        "STOPPED",
    )?;
    need(
        sha(&fs::read(&descriptor_file).map_err(|e| codeless(e.to_string()))?)
            == descriptor_bytes_hash,
        "DESCRIPTOR_OR_CODE_CHANGED",
    )?;
    for (file, hash) in &code_files {
        need(
            sha(&fs::read(workspace.join(file)).map_err(|e| codeless(e.to_string()))?)
                == hash.as_str().unwrap_or(""),
            "DESCRIPTOR_OR_CODE_CHANGED",
        )?;
    }
    for f in &source_files {
        need(
            sha(&fs::read(runtime_dir.join(f)).map_err(|e| codeless(e.to_string()))?)
                == source_hashes.get(f).and_then(V::as_str).unwrap_or(""),
            "ORIGINAL_SOURCE_CHANGED",
        )?;
    }
    Ok(())
}

// function actualAccount(){const a=sync(getAccountLease);...}
fn actual_account(inner: &Rc<RefCell<HostInner>>) -> Result<V, String> {
    let a = (inner.borrow().deps.get_account_lease)()?;
    let scope_owner = sget(&inner.borrow().scope, "ownerId").unwrap_or("").to_string();
    need(
        sorted_keys(&a).join("|") == "leaseId|ownerId|tournamentId"
            && sget(&a, "ownerId") == Some(scope_owner.as_str())
            && sget(&a, "tournamentId") == Some(T)
            && hex(get(&a, "leaseId")),
        "RAW_ACCOUNT_LEASE",
    )?;
    let get_core = inner.borrow().deps.get_core_owner_record.clone();
    if let Some(f) = get_core {
        let r = f()?;
        need(
            sget(&r, "ownerId") == Some(scope_owner.as_str())
                && sget(&r, "scope") == Some(T)
                && js_int(get(&r, "pid")) == Some(std::process::id() as i64)
                && same(&r, &a),
            "CURRENT_CORE_PID_OWNER",
        )?;
    }
    if let Some(raw) = inner.borrow().raw_account.clone() {
        need(same(&a, &raw), "ACCOUNT_OWNER_CHANGED")?;
    }
    Ok(a)
}

// function leases(){immutable();financialLease?.assertOwnership();...}
fn leases(inner: &Rc<RefCell<HostInner>>) -> Result<V, String> {
    immutable(inner)?;
    let g = inner.borrow();
    if let Some(l) = &g.financial_lease {
        l.assert_ownership().map_err(|e| codeless(e.message))?;
    }
    if let Some(l) = &g.legacy_controller_fence {
        l.assert_ownership().map_err(|e| codeless(e.to_string()))?;
    }
    if let Some(l) = &g.legacy_budget_fence {
        l.assert_ownership().map_err(|e| codeless(e.to_string()))?;
    }
    drop(g);
    let a = actual_account(inner)?;
    let g = inner.borrow();
    need(
        !g.epoch_receipt.is_null() && same(get(&g.epoch_receipt, "rawAccountLease"), &a),
        "DURABLE_OWNER_BINDING",
    )?;
    Ok(json!({
        "ownerId": get(&g.scope, "ownerId"),
        "runId": get(&g.scope, "runId"),
        "tournamentId": T,
        "leaseId": payload_hash(&g.epoch_receipt)?,
    }))
}

// function mechanicalLeases(){need(...api.manualReview!==true&&now()>=(api.cooldown??0)...)
fn mechanical_leases(inner: &Rc<RefCell<HostInner>>) -> Result<V, String> {
    let g = inner.borrow();
    let api = g.deps.api.borrow();
    need(
        !g.closed
            && !g.failed
            && (g.deps.can_operate)()
            && !g.stop_file.exists()
            && !api.manual_review
            && now(inner) >= api.cooldown,
        "STOPPED",
    )?;
    drop(api);
    drop(g);
    let g = inner.borrow();
    if let Some(l) = &g.financial_lease {
        l.assert_ownership().map_err(|e| codeless(e.message))?;
    }
    if let Some(l) = &g.legacy_controller_fence {
        l.assert_ownership().map_err(|e| codeless(e.to_string()))?;
    }
    if let Some(l) = &g.legacy_budget_fence {
        l.assert_ownership().map_err(|e| codeless(e.to_string()))?;
    }
    drop(g);
    let a = actual_account(inner)?;
    let g = inner.borrow();
    need(
        !g.epoch_receipt.is_null() && same(get(&g.epoch_receipt, "rawAccountLease"), &a),
        "DURABLE_OWNER_BINDING",
    )?;
    Ok(json!({
        "ownerId": get(&g.scope, "ownerId"),
        "runId": get(&g.scope, "runId"),
        "tournamentId": T,
        "leaseId": payload_hash(&g.epoch_receipt)?,
    }))
}

// const financial=()=>({...leases(),implementationEpoch:epoch});
fn financial(inner: &Rc<RefCell<HostInner>>) -> Result<V, String> {
    let mut v = leases(inner)?;
    let epoch = inner.borrow().epoch.clone();
    v.as_object_mut()
        .unwrap()
        .insert("implementationEpoch".into(), json!(epoch));
    Ok(v)
}

// function legacy(){const core=validateExecutionState(sync(getOtherState)),retired=sync(getLegacyRisk);...}
fn legacy(inner: &Rc<RefCell<HostInner>>) -> Result<V, String> {
    let other = (inner.borrow().deps.get_other_state)()?;
    let core = validate_execution_state(&other)?;
    let retired = (inner.borrow().deps.get_legacy_risk)()?;
    let intents_open = get(&core, "intents")
        .as_object()
        .map(|m| m.values().any(|i| sget(i, "status") != Some("closed")))
        .unwrap_or(false);
    let owned_positive = get(&core, "owned")
        .as_object()
        .map(|m| m.values().any(|p| num(get(p, "quantity")) > 0.0))
        .unwrap_or(false);
    need(
        !intents_open
            && !owned_positive
            && get(&retired, "complete") == &V::Bool(true)
            && hex(get(&retired, "sourceHash"))
            && js_int(get(&retired, "currentOwnedQuantity")) == Some(0)
            && sget(&retired, "currentReservedCash") == Some("0.000000")
            && get(&retired, "unresolvedIntentIds")
                .as_array()
                .is_some_and(|a| a.is_empty()),
        "OTHER_WRITER_LIABILITY",
    )?;
    Ok(json!({
        "coreHash": payload_hash(&core)?,
        "retiredHash": get(&retired, "sourceHash"),
    }))
}

// const guard=()=>{leases();legacy();need(api.manualReview!==true&&now()>=(api.cooldown??0),'SHARED_BACKOFF');};
fn guard(inner: &Rc<RefCell<HostInner>>) -> Result<(), String> {
    leases(inner)?;
    legacy(inner)?;
    let api = inner.borrow().deps.api.clone();
    let api = api.borrow();
    need(
        !api.manual_review && now(inner) >= api.cooldown,
        "SHARED_BACKOFF",
    )
}

// const observation=()=>{...}
fn observation(inner: &Rc<RefCell<HostInner>>) -> Result<V, String> {
    guard(inner)?;
    let (cash_receipt, scope, epoch_receipt) = {
        let g = inner.borrow();
        (
            g.cash_receipt.clone(),
            g.scope.clone(),
            g.epoch_receipt.clone(),
        )
    };
    need(cash_receipt.is_some(), "NO_ACTUAL_CASH")?;
    let slug = sget(&scope, "tournamentSlug").unwrap_or("").to_string();
    let c = rounded_tournament_cash_interval(
        cash_receipt.as_ref().unwrap(),
        &json!({"tournamentId": T, "tournamentSlug": slug, "readAt": iso(now(inner))}),
    )?;
    let source_hash = payload_hash(&json!({
        "cash": get(&c, "sourceBodyHash"),
        "lease": payload_hash(&epoch_receipt)?,
        "other": legacy(inner)?,
    }))?;
    let mut out = scope.clone();
    if let V::Object(m) = &mut out {
        m.insert("at".into(), json!(iso(now(inner))));
        m.insert("sourceHash".into(), json!(source_hash));
        m.insert("complete".into(), json!(true));
        m.insert("eventScopeComplete".into(), json!(true));
        m.insert("soleCollector".into(), json!(true));
        m.insert("stopped".into(), json!(false));
        m.insert("unknownAcceptance".into(), json!(false));
        m.insert("unresolvedIntentIds".into(), json!([]));
        m.insert("cashLower".into(), get(&c, "lower").clone());
        m.insert("owned".into(), json!([]));
        m.insert("reservations".into(), json!([]));
        m.insert("loans".into(), json!([]));
    }
    Ok(out)
}

// function rawOf(response,meta){const r=sync(receiptOf,response,meta);...}
fn raw_of(inner: &Rc<RefCell<HostInner>>, response: &V, meta: &V) -> Result<V, String> {
    let r = (inner.borrow().deps.receipt_of)(response, meta)?;
    need(
        sget(&r, "method") == sget(meta, "method")
            && get(&r, "endpoint") == get(response, "endpoint"),
        "RAW_CALLBACK",
    )?;
    let endpoint = sget(&r, "endpoint").unwrap_or("").to_string();
    let slug = sget(&inner.borrow().scope, "tournamentSlug")
        .unwrap_or("")
        .to_string();
    if sget(meta, "method") == Some("GET") && endpoint == format!("/tournaments/{slug}") {
        inner.borrow_mut().cash_receipt = Some(r.clone());
    }
    if sget(meta, "method") == Some("GET") && endpoint.starts_with("/relationships/graph?") {
        inner.borrow_mut().last_graph_at = sget(&r, "requestStartedAt").map(String::from);
    }
    if sget(meta, "method") == Some("GET") && endpoint.contains("/orderbook?") {
        inner.borrow_mut().last_book_at = sget(&r, "requestStartedAt").map(String::from);
    }
    Ok(r)
}

// function halt(input){...}
fn halt(inner: &Rc<RefCell<HostInner>>, input: &V) -> Result<V, String> {
    let reason = match sget(input, "kind") {
        Some("non_neutral_or_unknown_hedge")
        | Some("uncertain_hedge")
        | Some("non_neutral_post_account") => sget(input, "kind").unwrap().to_string(),
        _ => "financial_uncertainty".to_string(),
    };
    let g = inner.borrow();
    let mut out = json!({
        "schemaVersion": 1,
        "source": "baal_combined_custody_v6",
        "reason": reason,
        "ownerId": get(&g.scope, "ownerId"),
        "runId": get(&g.scope, "runId"),
        "tournamentId": T,
        "implementationEpoch": g.epoch,
        "descriptorHash": g.descriptor_hash,
        "at": iso(now(inner)),
        "allWritersStop": true,
    });
    let sh = get(input, "sourceHash");
    if hex(sh) {
        out.as_object_mut()
            .unwrap()
            .insert("sourceHash".into(), sh.clone());
    }
    let stop_file = g.stop_file.clone();
    drop(g);
    let r = (|| -> Result<(), String> {
        if !stop_file.exists() {
            atomic_json(&stop_file, &out).map_err(|e| codeless(e.to_string()))?;
        }
        need(stop_file.exists(), "STOP_NOT_DURABLE")?;
        Ok(())
    })();
    if let Err(e) = r {
        inner.borrow_mut().failed = true;
        return Err(codeless(format!("{e}:fatal:durableRecordingFailed")));
    }
    Ok(out)
}

// function backoff(e){if(e?.retryAfterMs===undefined)return;...}
fn backoff(_inner: &Rc<RefCell<HostInner>>, _e: &str) -> Result<(), String> {
    // Ported error channel carries no retryAfterMs: there is nothing to
    // persist, matching the JS early return when e?.retryAfterMs is absent.
    Ok(())
}

// async function bootstrap(){guard();need(sync(hasReadHeadroom,1)===true,'BOOTSTRAP_HEADROOM');...}
fn bootstrap(inner: &Rc<RefCell<HostInner>>) -> Result<(), String> {
    guard(inner)?;
    need(
        (inner.borrow().deps.has_read_headroom)(1)? == true,
        "BOOTSTRAP_HEADROOM",
    )?;
    let slug = sget(&inner.borrow().scope, "tournamentSlug")
        .unwrap_or("")
        .to_string();
    let endpoint = format!("/tournaments/{slug}");
    let w = Rc::downgrade(inner);
    let before_dispatch = move |_p: &V, _s: &V| -> Result<bool, String> {
        let i = w.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
        guard(&i)?;
        Ok(true)
    };
    let w2 = Rc::downgrade(inner);
    let before_network = move |_p: &V, _s: &V| -> Result<bool, String> {
        let i = w2.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
        guard(&i)?;
        Ok(true)
    };
    let bd_ptr = &before_dispatch as &HostGuard as *const HostGuard as *const () as usize;
    let bn_ptr = &before_network as &HostGuard as *const HostGuard as *const () as usize;
    inner.borrow_mut().permits.insert(
        bd_ptr,
        json!({"endpoint": endpoint, "beforeNetworkPtr": bn_ptr as u64}),
    );
    let result: Result<(), String> = (|| {
        let api = inner.borrow().deps.api.borrow().dispatch.clone();
        let bd_t = |p: &V, s: &V| -> Result<bool, OrderTransportError> {
            before_dispatch(p, s).map_err(|c| transport_err(&c))
        };
        let bn_t = |p: &V, s: &V| -> Result<bool, OrderTransportError> {
            before_network(p, s).map_err(|c| transport_err(&c))
        };
        let r = api(
            &json!({"method": "GET", "endpoint": endpoint}),
            Some(&bd_t as &TGuard),
            Some(&bn_t as &TGuard),
        )
        .map_err(|e| e.message)?;
        need(js_int(get(&r, "status")) == Some(200), "CASH_HTTP")?;
        raw_of(inner, &r, &json!({"method": "GET"}))?;
        observation(inner)?;
        Ok(())
    })();
    inner.borrow_mut().permits.remove(&bd_ptr);
    result
}

// function bridgeFor(exchangeId){const slot=coordinator.inventory(exchangeId);...}
fn bridge_for(
    inner: &Rc<RefCell<HostInner>>,
    exchange_id: &str,
) -> Result<Rc<OwnedNoFreshNoneHedgeEvidenceBridgeV2>, String> {
    let (coordinator, key, legs, first_ex) = {
        let g = inner.borrow();
        let coordinator = g
            .coordinator
            .clone()
            .ok_or_else(|| "BAAL_CUSTODY_V6_INTERNAL".to_string())?;
        let slot = coordinator.inventory(exchange_id);
        let legs = get(&get(&slot, "package"), "legs").clone();
        need(
            legs.as_array().is_some_and(|a| a.len() == 2),
            "OWNED_PAIR_PACKAGE",
        )?;
        let mut ids: Vec<String> = legs
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|l| sget(l, "exchangeId").map(String::from))
            .collect();
        ids.sort();
        let key = ids.join(":");
        let first = legs
            .as_array()
            .unwrap()
            .iter()
            .find(|l| sget(l, "exchangeId") == Some(exchange_id))
            .cloned()
            .unwrap_or(V::Null);
        (coordinator, key, legs, first)
    };
    if let Some(b) = inner.borrow().bridges.get(&key) {
        inner.borrow_mut().selected_bridge = Some(b.clone());
        return Ok(b.clone());
    }
    let weak = Rc::downgrade(inner);
    let api = inner.borrow().deps.api.clone();
    let (api_dispatch, api_manual, api_cooldown) = {
        let a = api.borrow();
        (a.dispatch.clone(), a.manual_review, a.cooldown)
    };
    let wire_to_dispatch = |d: Rc<WireDispatch>| -> Rc<Dispatch> {
        Rc::new(move |spec: &V, bd, bn| {
            (d)(spec, bd, bn).map_err(|e| transport_err(&e.message))
        })
    };
    let receipt_of = {
        let w = weak.clone();
        Rc::new(move |response: &V, meta: &V| -> Result<V, String> {
            let i = w.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
            raw_of(&i, response, meta)
        })
    };
    let scope = inner.borrow().scope.clone();
    let get_owned = {
        let c = coordinator.clone();
        Rc::new(move |ex: &V| -> Result<V, String> {
            Ok(c.inventory(ex.as_str().unwrap_or("")))
        })
    };
    let get_pair_census = {
        let c = coordinator.clone();
        Rc::new(move |ex: &V| -> Result<V, String> {
            c.pair_census(ex.as_str().unwrap_or(""))
        })
    };
    let get_funding = {
        let c = coordinator.clone();
        Rc::new(move || -> Result<V, String> { c.funding_reservation_status() })
    };
    let refresh_funding = {
        let w = weak.clone();
        let c = coordinator.clone();
        Rc::new(move || -> Result<(), String> {
            let i = w.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
            stage(&i, "funding_after_account", || c.refresh_reservation())?;
            Ok(())
        })
    };
    let get_global_risk = {
        let w = weak.clone();
        Rc::new(move || -> Result<V, String> {
            let i = w.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
            timed(&i, "current_budget_risk", || {
                let b = i.borrow().budget.clone();
                b.ok_or_else(|| {
                    codeless(
                        "TypeError: Cannot read properties of null (reading 'globalRisk')"
                            .to_string(),
                    )
                })?
                .global_risk()
            })
        })
    };
    let get_account_lease = inner.borrow().deps.get_account_lease.clone();
    let can_read = {
        let w = weak.clone();
        Rc::new(move || -> bool {
            w.upgrade()
                .map(|i| guard(&i).is_ok())
                .unwrap_or(false)
        })
    };
    let has_read_headroom = inner.borrow().deps.has_read_headroom.clone();
    let get_active_intent = {
        let c = coordinator.clone();
        Rc::new(move || -> Result<V, String> { Ok(c.active_intent_status()) })
    };
    let get_order_binding = {
        let c = coordinator.clone();
        Rc::new(move |id: &V| -> Result<V, String> { Ok(c.order_binding(id)) })
    };
    let now_fn = inner.borrow().deps.now.clone();
    let bridge = OwnedNoFreshNoneHedgeEvidenceBridgeV2::create(NoneHedgeBridgeV2Deps {
        api: NoneHedgeBridgeApi {
            dispatch: wire_to_dispatch(api_dispatch),
            manual_review: api_manual,
            cooldown: api_cooldown,
        },
        receipt_of,
        scope,
        first_exchange_id: sget(&first_ex, "exchangeId").unwrap_or("").to_string(),
        pair_legs: Some(legs),
        get_owned,
        get_pair_census,
        get_funding,
        get_global_risk,
        get_account_lease,
        can_read,
        has_read_headroom,
        get_active_intent: Some(get_active_intent),
        get_order_binding: Some(get_order_binding),
        refresh_funding_after_account: Some(refresh_funding),
        now: now_fn,
    })?;
    let bridge = Rc::new(bridge);
    inner.borrow_mut().bridges.insert(key, bridge.clone());
    inner.borrow_mut().selected_bridge = Some(bridge.clone());
    Ok(bridge)
}

// function negativeHint(ex){...return lastHint.skipPreparationReads;}catch{return false;}}
fn negative_hint(inner: &Rc<RefCell<HostInner>>, ex: &str) -> bool {
    (|| -> Result<bool, String> {
        let hints_fn = inner.borrow().deps.get_pair_read_hints.clone();
        let h = match hints_fn {
            Some(f) => f()?,
            None => V::Null,
        };
        let universe_hash = sget(&inner.borrow().universe, "hash")
            .unwrap_or("")
            .to_string();
        need(
            sget(&h, "tournamentId") == Some(T) && sget(&h, "universeHash") == Some(universe_hash.as_str()),
            "HINT_SCOPE",
        )?;
        let captures = if get(&h, "quoteCaptures").is_null() {
            get(&h, "quotes").clone()
        } else {
            get(&h, "quoteCaptures").clone()
        };
        let q: Vec<V> = captures
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|x| sget(x, "exchangeId") == Some(ex))
            .collect();
        need(q.len() == 1, "HINT_IDENTITY")?;
        let quote = q[0].clone();
        let coordinator = inner
            .borrow()
            .coordinator
            .clone()
            .ok_or_else(|| "BAAL_CUSTODY_V6_INTERNAL".to_string())?;
        let slot = coordinator.inventory(ex);
        let lot = get(get(&slot, "proof"), "remainingLots");
        let lot = lot.as_array().and_then(|a| a.first()).cloned().unwrap_or(V::Null);
        let at = now(inner);
        need(
            sget(&quote, "marketId") == sget(get(&slot, "proof"), "marketId")
                && (quote.get("tournamentId").is_none()
                    || sget(&quote, "tournamentId") == Some(T))
                && sget(&quote, "option") == Some("YES")
                && get(&quote, "crossed") != &V::Bool(true)
                && hex(get(&quote, "sourceHash"))
                && instant_v(get(&quote, "requestStartedAt")) <= instant_v(get(&quote, "receivedAt"))
                && instant_v(get(&quote, "receivedAt")) <= at
                && at - instant_v(get(&quote, "requestStartedAt")) <= 20000.0
                && (quote.get("knownAt").is_none_or(|k| k.is_null())
                    || (instant_v(get(&quote, "receivedAt")) <= instant_v(get(&quote, "knownAt"))
                        && instant_v(get(&quote, "knownAt")) <= at)),
            "HINT_FRESH",
        )?;
        let yes_bid = price(sget(&quote, "bestBid").unwrap_or(""))?;
        let yes_ask = price(sget(&quote, "bestAsk").unwrap_or(""))?;
        need(yes_bid <= yes_ask, "HINT_CROSSED")?;
        let bid = 1_000_000i128 - yes_ask;
        let skip = bid
            < amount(sget(&lot, "entryPrice").unwrap_or("0"))? + 5_000i128;
        let last_hint = json!({
            "marketId": get(&quote, "marketId"),
            "exchangeId": ex,
            "sourceHash": get(&quote, "sourceHash"),
            "receivedAt": get(&quote, "receivedAt"),
            "skipPreparationReads": skip,
            "orderEligible": false,
        });
        inner.borrow_mut().last_hint = last_hint;
        Ok(skip)
    })()
    .unwrap_or(false)
}

// function status(){return{schemaVersion:1,hostVersion:6,...}}
fn status(inner: &Rc<RefCell<HostInner>>) -> V {
    let g = inner.borrow();
    json!({
        "schemaVersion": 1,
        "hostVersion": 6,
        "mode": "baal_combined_custody_host_v6",
        "ownerId": get(&g.scope, "ownerId"),
        "runId": get(&g.scope, "runId"),
        "tournamentId": T,
        "observedAt": iso(now(inner)),
        "descriptorHash": g.descriptor_hash,
        "hostCodeIdentityHash": payload_hash(&g.code).ok(),
        "seedHash": get(&g.descriptor, "seedHash"),
        "legacyAnchor": get(&g.seed, "anchor"),
        "originalRequestHash": get(&g.descriptor, "originalRequestHash"),
        "priorV5UpgradeHash": get(&g.descriptor, "priorV5UpgradeHash"),
        "entryExpiresAt": get(&g.request, "entryExpiresAt"),
        "hedgeExpiresAt": get(&g.activation, "hedgeExpiresAt"),
        "exitExpiresAt": get(&g.activation, "exitExpiresAt"),
        "implementationEpoch": g.epoch,
        "lastAction": g.last_action,
        "lastFailure": g.last_failure,
        "lastStepAt": g.last_step_at,
        "busy": g.busy,
        "failed": g.failed,
        "closed": g.closed,
        "readBootstrapComplete": g.cash_receipt.is_some(),
        "financialOwnerBound": !g.epoch_receipt.is_null(),
        "exitHintSkippedPackets": g.exit_skipped,
        "lastExitHint": g.last_hint,
        "lastExitReadPriority": g.last_exit_priority,
        "controller": g.coordinator.as_ref().map(|c| c.status()).unwrap_or(V::Null),
        "budget": g.budget.as_ref().map(|b| b.status()).unwrap_or(V::Null),
        "bridge": g.selected_bridge.as_ref().map(|b| b.status()).unwrap_or(V::Null),
        "recentLocalTimings": g.timings,
        "stageTimings": g.stage_timings,
        "lastDispatchTiming": g.last_dispatch_timing,
        "lastRejectedTiming": g.last_rejected_timing,
        "fullRungExitPolicy": true,
        "additionalApiClients": 0,
        "additionalPersistentProcesses": 0,
        "oldWriterHandbackAllowed": false,
    })
}

// function publish(action){lastAction=action;...atomicJson(stats,out)}
fn publish(inner: &Rc<RefCell<HostInner>>, action: &str) -> Result<V, String> {
    inner.borrow_mut().last_action = action.to_string();
    inner.borrow_mut().last_step_at = Some(iso(now(inner)));
    let out = status(inner);
    let stats_file = inner.borrow().runtime_dir.join("baal-scaled-live-stats.json");
    if let Err(e) = atomic_json(&stats_file, &out) {
        inner.borrow_mut().failed = true;
        return Err(codeless(format!("{e}:fatal:durableRecordingFailed")));
    }
    Ok(out)
}

pub struct BaalCombinedCustodyHostV6 {
    inner: Rc<RefCell<HostInner>>,
}

impl BaalCombinedCustodyHostV6 {
    fn step_inner(&self) -> Result<V, String> {
        let inner = &self.inner;
        guard(inner)?;
        inner.borrow_mut().last_failure = None;
        let (cash_stale, budget_missing) = {
            let g = inner.borrow();
            (
                match &g.cash_receipt {
                    Some(r) => {
                        now(inner) - instant_v(get(r, "requestStartedAt")) > 12000.0
                    }
                    None => true,
                },
                g.budget.is_none(),
            )
        };
        if cash_stale {
            stage(inner, "cash_bootstrap", || bootstrap(inner))?;
        }
        if budget_missing {
            let weak = Rc::downgrade(inner);
            let g = inner.borrow();
            let deps = OwnedHedgeBudgetDeps {
                assert_account_lease: {
                    let w = weak.clone();
                    Rc::new(move || -> Result<V, String> {
                        let i = w.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
                        leases(&i)
                    })
                },
                assert_financial_owner_lease: {
                    let w = weak.clone();
                    Rc::new(move || -> Result<V, String> {
                        let i = w.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
                        financial(&i)
                    })
                },
                get_coordinator_source: {
                    let w = weak.clone();
                    Rc::new(move || -> Result<V, String> {
                        let i = w.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
                        let c = i.borrow().coordinator.clone().ok_or_else(|| {
                            codeless(
                                "TypeError: Cannot read properties of null (reading 'journalSource')"
                                    .to_string(),
                            )
                        })?;
                        c.journal_source()
                    })
                },
                observe_global_risk: {
                    let w = weak.clone();
                    Rc::new(move || -> Result<V, String> {
                        let i = w.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
                        observation(&i)
                    })
                },
                can_operate: {
                    let w = weak.clone();
                    Rc::new(move || -> bool {
                        w.upgrade().map(|i| guard(&i).is_ok()).unwrap_or(false)
                    })
                },
                now: g.deps.now.clone(),
            };
            let budget = BaalOwnedHedgeSharedBudget::open(
                &g.budget_dir,
                &g.coordinator_dir,
                &g.workspace,
                &g.legacy_file,
                &g.old_code,
                &g.scope,
                &g.allocation,
                &g.activation,
                &g.migration,
                sget(&g.activation, "codeIdentityHash").unwrap_or(""),
                deps,
            )?;
            drop(g);
            inner.borrow_mut().budget = Some(Rc::new(budget));
        }
        let (b, f) = {
            let g = inner.borrow();
            (
                g.budget.as_ref().map(|x| x.status()).unwrap_or(V::Null),
                g.coordinator
                    .as_ref()
                    .map(|c| c.funding_reservation_status())
                    .unwrap_or(Ok(V::Null))?,
            )
        };
        let refresh = if get(&b, "claimed") != &V::Bool(true) {
            true
        } else {
            let record = get(&f, "record");
            if record.is_null() {
                return Err(codeless(
                    "TypeError: Cannot read properties of null (reading 'reservationHash')"
                        .to_string(),
                ));
            }
            get(&b, "controllerReservationHash") != get(record, "reservationHash")
                || get(get(&f, "receipt"), "globalLedgerHash") != get(&b, "globalLedgerHash")
        };
        if refresh {
            let c = inner.borrow().coordinator.clone().unwrap();
            stage(inner, "funding_refresh", || c.refresh_reservation())?;
        }
        let s = inner
            .borrow()
            .coordinator
            .as_ref()
            .map(|c| c.status())
            .unwrap_or(V::Null);
        let intent = get(&s, "intent").clone();
        if js_truthy(Some(&intent)) {
            if get(&intent, "orderId").is_null() {
                if sget(&intent, "phase") == Some("prepared")
                    && get(&intent, "mayHaveExecuted") == &V::Bool(false)
                    && js_int(get(&intent, "actualFetchCount")) == Some(0)
                    && now(inner) >= instant_v(get(&get(&intent, "body"), "expirationDate"))
                {
                    let c = inner.borrow().coordinator.clone().unwrap();
                    c.abandon_prepared()?;
                    return publish(inner, "expired_never_admitted_intent_abandoned");
                }
                if get(&s, "unknownAcceptance") == &V::Bool(true) {
                    halt(inner, &json!({"kind": "uncertain_hedge"}))?;
                }
                return publish(inner, "existing_unconfirmed_intent_blocks_new_orders");
            }
            let order_id = get(&intent, "orderId").clone();
            let first_ex = if get(&intent, "firstExchangeId").is_null() {
                get(&intent, "exchangeId").clone()
            } else {
                get(&intent, "firstExchangeId").clone()
            };
            let bridge = bridge_for(inner, first_ex.as_str().unwrap_or(""))?;
            let coordinator = inner.borrow().coordinator.clone().unwrap();
            let r: Result<V, String> = (|| {
                stage(inner, "terminal_packet", || {
                    bridge.refresh_reconciliation(&order_id)
                })?;
                let e = bridge.get_evidence(
                    &json!({"operation": "reconcile", "orderId": order_id}),
                )?;
                stage(inner, "terminal_proof", || coordinator.reconcile(&e))?;
                publish(inner, "terminal_order_reconciled")
            })();
            match r {
                Ok(v) => return Ok(v),
                Err(e) => {
                    if e.contains(":fatal:") || e.contains("durableRecordingFailed") {
                        return Err(e);
                    }
                    if non_none(&e) {
                        halt(inner, &json!({"kind": "non_neutral_post_account"}))?;
                        inner.borrow_mut().last_failure = Some(safe_code(&e));
                        return publish(inner, "non_neutral_post_account_quarantined");
                    }
                    backoff(inner, &e)?;
                    inner.borrow_mut().last_failure = Some(safe_code(&e));
                    return publish(inner, "known_order_waiting_exact_terminal_proof");
                }
            }
        }
        let mut preferred_attempt: Option<String> = None;
        inner.borrow_mut().last_exit_priority = V::Null;
        let activation = inner.borrow().activation.clone();
        if now(inner) < instant_v(get(&activation, "exitExpiresAt")) {
            let hints = match inner.borrow().deps.get_pair_read_hints.clone() {
                Some(f) => f().unwrap_or(V::Null),
                None => V::Null,
            };
            let universe_hash = sget(&inner.borrow().universe, "hash")
                .unwrap_or("")
                .to_string();
            let owned: Vec<V> = get(&s, "inventory")
                .as_object()
                .map(|m| {
                    let c = inner.borrow().coordinator.clone().unwrap();
                    m.keys()
                        .filter_map(|ex| {
                            let v = c.inventory(ex);
                            if v.is_null() {
                                None
                            } else {
                                Some(v)
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let ranked = rank_baal_owned_exit_read_hints_v1(&json!({
                "scope": inner.borrow().scope,
                "universeHash": universe_hash,
                "owned": owned,
                "hints": hints,
                "now": now(inner),
            }));
            let candidate = get(&ranked, "ranked")
                .as_array()
                .and_then(|a| a.first())
                .cloned();
            if let Some(candidate) = candidate {
                let ex = sget(&candidate, "exchangeId").unwrap_or("").to_string();
                preferred_attempt = Some(ex.clone());
                let mut priority = candidate.clone();
                if let V::Object(m) = &mut priority {
                    m.insert("depthVerified".into(), json!(false));
                    m.insert("actualProfitClaim".into(), json!(false));
                    m.insert("orderEligible".into(), json!(false));
                }
                inner.borrow_mut().last_exit_priority = priority;
                let bridge = bridge_for(inner, &ex)?;
                let leg = get(&inner.borrow().scope, "exchanges")
                    .as_array()
                    .and_then(|a| {
                        a.iter()
                            .find(|l| sget(l, "exchangeId") == Some(ex.as_str()))
                            .cloned()
                    })
                    .unwrap_or(V::Null);
                let coordinator = inner.borrow().coordinator.clone().unwrap();
                let r: Result<V, String> = (|| {
                    stage(inner, "priority_sell_packet", || bridge.refresh_sell(&leg))?;
                    stage(inner, "prepare_sell", || {
                        coordinator.prepare_sell(&ex, None)
                    })?;
                    stage(inner, "final_none", || bridge.refresh_final_none())?;
                    inner.borrow_mut().packet_ready_at = Some(Instant::now());
                    stage(inner, "dispatch_sell", || coordinator.dispatch_prepared())?;
                    publish(inner, "covered_profitable_fifo_sell_dispatched")
                })();
                match r {
                    Ok(v) => return Ok(v),
                    Err(e) => {
                        let has_intent = {
                            let g = inner.borrow();
                            g.coordinator
                                .as_ref()
                                .map(|c| !get(&c.status(), "intent").is_null())
                                .unwrap_or(false)
                        };
                        if e.contains(":fatal:")
                            || e.contains("durableRecordingFailed")
                            || has_intent
                        {
                            return Err(e);
                        }
                        if non_none(&e) && e.ends_with("COLLATERAL") {
                            halt(inner, &json!({"kind": "non_neutral_post_account"}))?;
                            inner.borrow_mut().last_failure = Some(safe_code(&e));
                            return publish(inner, "global_collateral_requires_explicit_model");
                        }
                        backoff(inner, &e)?;
                        inner.borrow_mut().last_failure = Some(safe_code(&e));
                    }
                }
            }
        }
        {
            let g = inner.borrow();
            let first_ex = sget(&g.descriptor, "firstExchangeId").unwrap_or("").to_string();
            let hedge_ok = now(inner) < instant_v(get(&g.activation, "hedgeExpiresAt"))
                && g.hedge_tried_at.is_none()
                && js_int(get(&s, "hedgeIntentCount")) == Some(0)
                && g.coordinator
                    .as_ref()
                    .map(|c| {
                        js_int(get(&get(&c.inventory(&first_ex), "proof"), "remainingOwnedQuantity"))
                            .unwrap_or(0)
                            > 0
                    })
                    .unwrap_or(false);
            drop(g);
            if hedge_ok {
                inner.borrow_mut().hedge_tried_at = Some(now(inner));
                let bridge = bridge_for(inner, &first_ex)?;
                let coordinator = inner.borrow().coordinator.clone().unwrap();
                let r: Result<V, String> = (|| {
                    stage(inner, "hedge_packet", || bridge.refresh())?;
                    stage(inner, "prepare_hedge", || {
                        coordinator.prepare_hedge(&first_ex)
                    })?;
                    stage(inner, "final_none", || bridge.refresh_final_none())?;
                    inner.borrow_mut().packet_ready_at = Some(Instant::now());
                    stage(inner, "dispatch_hedge", || coordinator.dispatch_prepared())?;
                    publish(inner, "one_share_owned_counterpart_dispatched")
                })();
                match r {
                    Ok(v) => return Ok(v),
                    Err(e) => {
                        let has_intent = {
                            let g = inner.borrow();
                            g.coordinator
                                .as_ref()
                                .map(|c| !get(&c.status(), "intent").is_null())
                                .unwrap_or(false)
                        };
                        if e.contains(":fatal:")
                            || e.contains("durableRecordingFailed")
                            || has_intent
                        {
                            return Err(e);
                        }
                        if non_none(&e) && e.ends_with("COLLATERAL") {
                            halt(inner, &json!({"kind": "non_neutral_post_account"}))?;
                            inner.borrow_mut().last_failure = Some(safe_code(&e));
                            return publish(inner, "global_collateral_requires_explicit_model");
                        }
                        backoff(inner, &e)?;
                        inner.borrow_mut().last_failure = Some(safe_code(&e));
                    }
                }
            }
        }
        if now(inner) >= instant_v(get(&activation, "exitExpiresAt")) {
            return publish(inner, "bounded_exit_window_expired_custody_retained");
        }
        let owned: Vec<(String, V)> = {
            let g = inner.borrow();
            g.coordinator
                .as_ref()
                .map(|c| {
                    get(&c.status(), "inventory")
                        .as_object()
                        .map(|m| {
                            m.iter()
                                .filter(|(_, v)| num(get(v, "quantity")) > 0.0)
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .unwrap_or_default()
        };
        for i in 0..owned.len() {
            let cursor = inner.borrow().sell_cursor;
            let (ex, _) = owned[(cursor + i) % owned.len()].clone();
            if preferred_attempt.as_deref() == Some(ex.as_str()) {
                continue;
            }
            if negative_hint(inner, &ex) {
                inner.borrow_mut().exit_skipped += 1;
                continue;
            }
            inner.borrow_mut().sell_cursor = (cursor + i + 1) % owned.len();
            let bridge = bridge_for(inner, &ex)?;
            let leg = get(&inner.borrow().scope, "exchanges")
                .as_array()
                .and_then(|a| {
                    a.iter()
                        .find(|l| sget(l, "exchangeId") == Some(ex.as_str()))
                        .cloned()
                })
                .unwrap_or(V::Null);
            let coordinator = inner.borrow().coordinator.clone().unwrap();
            let r: Result<V, String> = (|| {
                stage(inner, "sell_packet", || bridge.refresh_sell(&leg))?;
                stage(inner, "prepare_sell", || {
                    coordinator.prepare_sell(&ex, None)
                })?;
                stage(inner, "final_none", || bridge.refresh_final_none())?;
                inner.borrow_mut().packet_ready_at = Some(Instant::now());
                stage(inner, "dispatch_sell", || coordinator.dispatch_prepared())?;
                publish(inner, "covered_profitable_fifo_sell_dispatched")
            })();
            match r {
                Ok(v) => return Ok(v),
                Err(e) => {
                    let has_intent = {
                        let g = inner.borrow();
                        g.coordinator
                            .as_ref()
                            .map(|c| !get(&c.status(), "intent").is_null())
                            .unwrap_or(false)
                    };
                    if e.contains(":fatal:")
                        || e.contains("durableRecordingFailed")
                        || has_intent
                    {
                        return Err(e);
                    }
                    if non_none(&e) && e.ends_with("COLLATERAL") {
                        halt(inner, &json!({"kind": "non_neutral_post_account"}))?;
                        inner.borrow_mut().last_failure = Some(safe_code(&e));
                        return publish(inner, "global_collateral_requires_explicit_model");
                    }
                    backoff(inner, &e)?;
                    inner.borrow_mut().last_failure = Some(safe_code(&e));
                    return publish(inner, "owned_exit_not_currently_executable");
                }
            }
        }
        publish(inner, "owned_custody_retained_no_profitable_hint")
    }

    pub fn step(&self) -> Result<V, String> {
        need(!self.inner.borrow().busy, "BUSY")?;
        self.inner.borrow_mut().busy = true;
        let inner = self.inner.clone();
        let result: Result<V, String> = (|| {
            match self.step_inner() {
                Ok(v) => Ok(v),
                Err(e) => {
                    // JS outer catch records the rejection timing first.
                    let (last_graph, cash, packet) = {
                        let g = inner.borrow();
                        (
                            g.last_graph_at.clone(),
                            g.cash_receipt.clone(),
                            g.packet_ready_at,
                        )
                    };
                    let at = now(&inner);
                    inner.borrow_mut().last_rejected_timing = json!({
                        "at": iso(at),
                        "graphAgeMs": last_graph.as_ref().map(|g| at - instant_v(&json!(g))),
                        "cashAgeMs": cash.as_ref().map(|c| at - instant_v(get(c, "requestStartedAt"))),
                        "localElapsedMsSincePacketReady": packet.map(|p| round3(p.elapsed().as_secs_f64() * 1000.0)),
                    });
                    if non_none(&e) && e.ends_with("COLLATERAL") {
                        halt(&inner, &json!({"kind": "non_neutral_post_account"}))?;
                        inner.borrow_mut().last_failure = Some(safe_code(&e));
                        return publish(&inner, "global_collateral_requires_explicit_model");
                    }
                    backoff(&inner, &e)?;
                    inner.borrow_mut().last_failure = Some(safe_code(&e));
                    if e.contains(":fatal:") || e.contains("durableRecordingFailed") {
                        inner.borrow_mut().failed = true;
                        return Err(e);
                    }
                    publish(&inner, "blocked_existing_custody_retained")
                }
            }
        })();
        self.inner.borrow_mut().busy = false;
        result
    }

    pub fn status(&self) -> V {
        status(&self.inner)
    }

    /// authorizes(spec) — JS consults `permits` keyed on spec.beforeDispatch,
    /// then every bridge and the coordinator. Here the caller passes the
    /// guards (they are unrepresentable inside a JSON spec), matching the
    /// sibling-host signature.
    pub fn authorizes(
        &self,
        spec: &V,
        before_dispatch: &HostGuard,
        before_network: &HostGuard,
    ) -> bool {
        (|| -> Result<bool, String> {
            guard(&self.inner)?;
            let bd_ptr = before_dispatch as *const HostGuard as *const () as usize;
            let bn_ptr = before_network as *const HostGuard as *const () as usize;
            let g = self.inner.borrow();
            let p = g.permits.get(&bd_ptr).cloned();
            let permit_ok = p.is_some()
                && sget(spec, "method") == Some("GET")
                && get(spec, "endpoint") == get(p.as_ref().unwrap(), "endpoint")
                && bn_ptr
                    == get(p.as_ref().unwrap(), "beforeNetworkPtr")
                        .as_u64()
                        .unwrap_or(0) as usize
                && spec.get("body").is_none()
                && spec.get("bodyJson").is_none()
                && spec.get("headers").is_none();
            if permit_ok {
                return Ok(true);
            }
            let bd_owned = before_dispatch;
            let bn_owned = before_network;
            let bd_t = |p: &V, s: &V| -> Result<bool, OrderTransportError> {
                bd_owned(p, s).map_err(|c| transport_err(&c))
            };
            let bn_t = |p: &V, s: &V| -> Result<bool, OrderTransportError> {
                bn_owned(p, s).map_err(|c| transport_err(&c))
            };
            for b in g.bridges.values() {
                if b.authorizes(spec, &bd_t as &TGuard, &bn_t as &TGuard) {
                    return Ok(true);
                }
            }
            if let Some(c) = &g.coordinator {
                if c.authorizes(spec, &bd_t as &TGuard, &bn_t as &TGuard) {
                    return Ok(true);
                }
            }
            Ok(false)
        })()
        .unwrap_or(false)
    }

    /// onBeforeFetch(e) — coordinator's final-fence hook.
    pub fn on_before_fetch(&self, e: &V) -> Result<V, String> {
        let c = self.inner.borrow().coordinator.clone();
        c.ok_or_else(|| "BAAL_CUSTODY_V6_CLOSED".to_string())?
            .on_before_fetch(e)
    }

    /// onFetchStart(e) — records the POST dispatch timing after the
    /// coordinator's own hook.
    pub fn on_fetch_start(&self, e: &V) -> Result<bool, String> {
        let at_fetch_callback = Instant::now();
        let c = self.inner.borrow().coordinator.clone();
        let result = c
            .ok_or_else(|| "BAAL_CUSTODY_V6_CLOSED".to_string())?
            .on_fetch_start(e)?;
        if sget(e, "method") == Some("POST") {
            let g = self.inner.borrow();
            let fetched = instant_v(get(e, "requestStartedAt"));
            let timing = json!({
                "actualFetchStartAt": get(e, "requestStartedAt"),
                "graphRequestStartedAt": g.last_graph_at,
                "graphAgeMsAtFetch": g.last_graph_at.as_ref().map(|x| fetched - instant_v(&json!(x))),
                "cashRequestStartedAt": g.cash_receipt.as_ref().map(|c| get(c, "requestStartedAt")).cloned().unwrap_or(V::Null),
                "cashAgeMsAtFetch": g.cash_receipt.as_ref().map(|c| fetched - instant_v(get(c, "requestStartedAt"))),
                "bookRequestStartedAt": g.last_book_at,
                "bookAgeMsAtFetch": g.last_book_at.as_ref().map(|x| fetched - instant_v(&json!(x))),
                "localElapsedMsSincePacketReady": g.packet_ready_at.map(|p| round3(at_fetch_callback.duration_since(p).as_secs_f64() * 1000.0)),
            });
            drop(g);
            self.inner.borrow_mut().last_dispatch_timing = timing;
        }
        Ok(result)
    }

    pub fn blocks_legacy_writes(&self) -> bool {
        true
    }
    pub fn allows_legacy(&self, s: &V) -> bool {
        sget(s, "method") == Some("GET")
    }

    /// close(){need(!busy,'BUSY');...}
    pub fn close(&self) -> Result<(), String> {
        need(!self.inner.borrow().busy, "BUSY")?;
        if self.inner.borrow().closed {
            return Ok(());
        }
        self.inner.borrow_mut().closed = true;
        let mut error: Option<String> = None;
        let (bridges, budget, coordinator) = {
            let g = self.inner.borrow();
            (
                g.bridges.values().cloned().collect::<Vec<_>>(),
                g.budget.clone(),
                g.coordinator.clone(),
            )
        };
        for b in bridges {
            if let Err(e) = b.close() {
                if error.is_none() {
                    error = Some(e);
                }
            }
        }
        if let Some(b) = &budget {
            b.close();
        }
        if let Some(c) = &coordinator {
            if let Err(e) = c.close() {
                if error.is_none() {
                    error = Some(e);
                }
            }
        }
        let mut g = self.inner.borrow_mut();
        for mut l in [
            g.legacy_budget_fence.take(),
            g.legacy_controller_fence.take(),
            g.financial_lease.take(),
        ]
        .into_iter()
        .flatten()
        {
            if let Err(e) = l.release() {
                if error.is_none() {
                    error = Some(e.message);
                }
            }
        }
        drop(g);
        match error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// baalCombinedCustodyV6Identity(workspace)
pub fn baal_combined_custody_v6_identity(workspace: &Path) -> Result<V, String> {
    collect_frozen_paper_files(
        workspace,
        &[
            "execution/baal-combined-custody-host-v6.mjs",
            "execution/scoped-rest-receipts-v5.mjs",
        ],
    )
}

/// createBaalCombinedCustodyV6Descriptor({...}) — pure descriptor construction
/// for the root's explicit stopped-writer handoff.
pub fn create_baal_combined_custody_v6_descriptor(
    workspace: &Path,
    universe: &V,
    original_request: &V,
    prior_v5_upgrade: &V,
    seed: &V,
    first_exchange_id: &str,
    implementation_epoch: &str,
    started_at: &str,
    hedge_expires_at: &str,
    exit_expires_at: &str,
    recorded_at: &str,
) -> Result<V, String> {
    need(
        get(seed, "economicReplayPerformed") == &V::Bool(true)
            && sget(seed, "stateHash") == Some(payload_hash(get(seed, "state"))?.as_str())
            && js_int(get(
                &get(
                    &get(&get(&get(seed, "state"), "inventory"), first_exchange_id),
                    "proof",
                ),
                "remainingOwnedQuantity",
            ))
            .unwrap_or(0)
                > 0
            && sget(get(&get(seed, "state"), "scope"), "runId")
                == sget(original_request, "runId")
            && sget(get(&get(seed, "state"), "scope"), "tournamentId")
                == sget(universe, "tournamentId")
            && sget(original_request, "universeHash") == sget(universe, "hash"),
        "DESCRIPTOR_VERIFIED_SEED",
    )?;
    need(
        !implementation_epoch.is_empty()
            && implementation_epoch.len() <= 100
            && implementation_epoch
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            && instant_v(&json!(hedge_expires_at)) > instant_v(&json!(started_at))
            && instant_v(&json!(hedge_expires_at)) - instant_v(&json!(started_at)) <= 3_600_000.0
            && instant_v(&json!(exit_expires_at)) > instant_v(&json!(started_at))
            && instant_v(&json!(exit_expires_at)) - instant_v(&json!(started_at)) <= 86_400_000.0,
        "DESCRIPTOR_WINDOWS",
    )?;
    let common = json!({
        "ownerId": get(original_request, "ownerId"),
        "runId": get(original_request, "runId"),
        "tournamentId": get(original_request, "tournamentId"),
    });
    let seed_hash = payload_hash(seed)?;
    let migration = json!({
        "schemaVersion": 1,
        "seedHash": seed_hash,
        "legacyAnchor": get(seed, "anchor"),
        "legacyCodeIdentityHash": get(seed, "legacyCodeIdentityHash"),
        "implementationEpoch": implementation_epoch,
        "authorityHash": get(original_request, "authorityHash"),
    });
    let mut activation = json!({
        "schemaVersion": 1,
        "mode": "baal_neutral_owned_counterpart_pilot_v2",
    });
    if let (V::Object(a), V::Object(c)) = (&mut activation, &common) {
        for (k, v) in c {
            a.insert(k.clone(), v.clone());
        }
        a.insert("seedHash".into(), json!(seed_hash));
        a.insert("implementationEpoch".into(), json!(implementation_epoch));
        a.insert(
            "codeIdentityHash".into(),
            json!(payload_hash(&live_coordinator_identity(workspace)?)?),
        );
        a.insert("authorityHash".into(), get(original_request, "authorityHash").clone());
        a.insert("maximumHedgeOrders".into(), json!(1));
        a.insert("quantity".into(), json!(1));
        a.insert("maximumOrderCost".into(), json!("0.995000"));
        a.insert("maximumPairUnitCost".into(), json!("0.990000"));
        a.insert("allowedFirstExchangeIds".into(), json!([first_exchange_id]));
        a.insert("startedAt".into(), json!(started_at));
        a.insert("hedgeExpiresAt".into(), json!(hedge_expires_at));
        a.insert("exitExpiresAt".into(), json!(exit_expires_at));
    }
    let mut d = json!({
        "schemaVersion": 1,
        "kind": "baal_combined_custody_host_v6",
    });
    if let V::Object(m) = &mut d {
        for (k, v) in common.as_object().unwrap() {
            m.insert(k.clone(), v.clone());
        }
        m.insert("universeHash".into(), get(universe, "hash").clone());
        m.insert("originalRequestHash".into(), json!(payload_hash(original_request)?));
        m.insert("priorV5UpgradeHash".into(), json!(payload_hash(prior_v5_upgrade)?));
        m.insert(
            "originalV5GraphHash".into(),
            json!(payload_hash(&baal_scaled_live_v5_identity(workspace)?)?),
        );
        m.insert("oldEntryExpiresAt".into(), get(original_request, "entryExpiresAt").clone());
        m.insert("legacyAnchor".into(), get(seed, "anchor").clone());
        m.insert(
            "legacyCodeIdentityHash".into(),
            get(seed, "legacyCodeIdentityHash").clone(),
        );
        m.insert("seedHash".into(), json!(seed_hash));
        m.insert("activation".into(), activation);
        m.insert("migration".into(), migration);
        m.insert("firstExchangeId".into(), json!(first_exchange_id));
        m.insert(
            "codeIdentity".into(),
            baal_combined_custody_v6_identity(workspace)?,
        );
        m.insert("recordedAt".into(), json!(recorded_at));
    }
    Ok(d)
}

/// openBaalCombinedCustodyHostV6({runtimeDir,workspace,universe,api,...}).
/// Returns Ok(None) when the descriptor file does not exist.
pub fn open_baal_combined_custody_host_v6(
    runtime_dir: &Path,
    workspace: &Path,
    universe: &V,
    deps: CombinedCustodyHostV6Deps,
) -> Result<Option<BaalCombinedCustodyHostV6>, String> {
    let descriptor_file = runtime_dir.join("baal-combined-custody-request-v6.json");
    if !descriptor_file.exists() {
        return Ok(None);
    }
    need(runtime_dir.is_absolute() && workspace.is_absolute(), "DEPENDENCIES")?;
    let read_bytes = |f: &Path| -> Result<Vec<u8>, String> {
        fs::read(f).map_err(|e| codeless(e.to_string()))
    };
    let read_json = |f: &Path| -> Result<V, String> {
        serde_json::from_slice(&read_bytes(f)?).map_err(|e| codeless(e.to_string()))
    };
    let source_files: Vec<String> = SOURCE_FILES.iter().map(|s| s.to_string()).collect();
    let mut source_bytes = Map::new();
    let mut source_hashes = Map::new();
    let mut values = Map::new();
    let mut hashes = Map::new();
    for f in &source_files {
        let b = read_bytes(&runtime_dir.join(f))?;
        source_hashes.insert(f.clone(), json!(sha(&b)));
        let v: V = serde_json::from_slice(&b).map_err(|e| codeless(e.to_string()))?;
        hashes.insert(f.clone(), json!(payload_hash(&v)?));
        source_bytes.insert(f.clone(), json!(b.len()));
        values.insert(f.clone(), v);
    }
    let _ = source_bytes;
    let request = values[SOURCE_FILES[0]].clone();
    let v2 = values[SOURCE_FILES[1]].clone();
    let v3 = values[SOURCE_FILES[2]].clone();
    let v5 = values[SOURCE_FILES[3]].clone();
    let capital = values[SOURCE_FILES[4]].clone();
    let authority = values[SOURCE_FILES[5]].clone();
    let catalog = values[SOURCE_FILES[6]].clone();
    let descriptor_bytes = read_bytes(&descriptor_file)?;
    let descriptor_bytes_hash = sha(&descriptor_bytes);
    let d: V = serde_json::from_slice(&descriptor_bytes).map_err(|e| codeless(e.to_string()))?;
    let descriptor_hash = payload_hash(&d)?;
    let code = baal_combined_custody_v6_identity(workspace)?;
    let code_files = get(&code, "files").as_object().cloned().unwrap_or_default();
    let old_code = baal_scaled_no_controller_identity()?;
    let now_ms = (deps.now)();
    need(
        js_int(get(&request, "schemaVersion")) == Some(1)
            && sget(&request, "kind") == Some("baal_scaled_live_production_v1")
            && sget(&request, "ownerId") == Some("Codex")
            && sget(&request, "runId").is_some_and(|r| {
                let b = r.as_bytes();
                r.starts_with("baal-scaled-no-")
                    && b.len() >= 16
                    && b.len() <= 95
                    && r[15..]
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                    && !r[15..].is_empty()
            })
            && sget(&request, "tournamentId") == Some(T)
            && sget(&request, "tournamentSlug") == Some("midterm-elections")
            && js_int(get(&request, "maximumActiveMarkets")) == Some(20)
            && instant_v(get(&request, "entryExpiresAt")) > instant_v(get(&request, "startedAt"))
            && instant_v(get(&request, "entryExpiresAt"))
                - instant_v(get(&request, "startedAt"))
                <= 3_600_000.0,
        "ORIGINAL_REQUEST",
    )?;
    need(
        sget(&request, "humanAllocationSourceHash")
            == hashes[SOURCE_FILES[4]].as_str()
            && sget(&capital, "kind") == Some("human_directed_baal_capital_allocation_v1")
            && sget(&capital, "ownerId") == Some("Codex")
            && sget(&capital, "allocatedNominalCapital") == Some("1000.000000")
            && sget(&capital, "maximumOrderNominalCost") == Some("25.000000")
            && sget(&capital, "maximumMarketNominalExposure") == Some("100.000000")
            && sget(&capital, "maximumEventNominalExposure") == Some("200.000000")
            && js_int(get(&capital, "maximumSharesPerOrder")) == Some(100)
            && js_int(get(&capital, "maximumActiveMarkets")) == Some(20)
            && sget(&capital, "fee") == Some("0.000000"),
        "HUMAN_ALLOCATION",
    )?;
    need(
        sget(&request, "authorityHash") == hashes[SOURCE_FILES[5]].as_str()
            && sget(&authority, "kind") == Some("direct_human_authorized_bot_update")
            && sget(&authority, "ownerId") == Some("Codex")
            && sget(&authority, "threadId") == Some("01a0f8cc-b9bb-77d2-9f4c-2844396f1b79")
            && sget(&request, "catalogHash") == hashes[SOURCE_FILES[6]].as_str()
            && sget(&catalog, "tournamentId") == Some(T),
        "ORIGINAL_AUTHORITY",
    )?;
    let old_host_hash = payload_hash(&baal_scaled_live_identity(workspace)?)?;
    let v2hash = payload_hash(&baal_scaled_live_v2_identity(workspace)?)?;
    let v3hash = payload_hash(&baal_scaled_live_v3_identity(workspace)?)?;
    let v5hash = payload_hash(&baal_scaled_live_v5_identity(workspace)?)?;
    need(
        payload_hash(get(&request, "codeIdentity"))? == old_host_hash,
        "ORIGINAL_HOST_CODE",
    )?;
    for (u, kind, graph, previous) in [
        (&v2, "baal_scaled_host_upgrade_v2", &v2hash, None),
        (
            &v3,
            "baal_scaled_host_upgrade_v3",
            &v3hash,
            Some(hashes[SOURCE_FILES[1]].as_str().unwrap_or("").to_string()),
        ),
        (
            &v5,
            "baal_scaled_host_upgrade_v5",
            &v5hash,
            Some(hashes[SOURCE_FILES[2]].as_str().unwrap_or("").to_string()),
        ),
    ] {
        need(
            js_int(get(u, "schemaVersion")) == Some(1)
                && sget(u, "kind") == Some(kind)
                && sget(u, "ownerId") == sget(&request, "ownerId")
                && sget(u, "runId") == sget(&request, "runId")
                && sget(u, "tournamentId") == Some(T)
                && sget(u, "originalRequestHash")
                    == hashes[SOURCE_FILES[0]].as_str()
                && sget(u, "originalHostCodeIdentityHash") == Some(old_host_hash.as_str())
                && payload_hash(get(u, "codeIdentity"))? == *graph
                && sget(u, "entryExpiresAt") == sget(&request, "entryExpiresAt")
                && instant_v(get(u, "recordedAt")) >= instant_v(get(&request, "startedAt"))
                && instant_v(get(u, "recordedAt")) <= now_ms
                && (previous.is_none()
                    || sget(u, "priorUpgradeHash") == previous.as_deref()),
            "PRIOR_UPGRADE",
        )?;
    }
    need(
        sget(&v5, "priorHostCodeIdentityHash") == Some(v3hash.as_str()),
        "PRIOR_V5_GRAPH",
    )?;
    let allowed = get(&request, "allowedMarkets")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let allowed_set: std::collections::BTreeSet<String> = allowed
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    need(
        sget(universe, "tournamentId") == Some(T)
            && sget(universe, "hash") == sget(&request, "universeHash")
            && universe.get("exchanges").is_some_and(V::is_array)
            && get(&request, "allowedMarkets").is_array()
            && allowed_set.len() == allowed.len(),
        "UNIVERSE",
    )?;
    let members: Vec<V> = get(universe, "exchanges")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| allowed_set.contains(sget(m, "marketId").unwrap_or("")))
        .collect();
    need(
        members.len() == allowed.len()
            && members.iter().all(|m| sget(m, "option") == Some("YES")),
        "EXACT_MEMBERS",
    )?;
    let scope = json!({
        "ownerId": "Codex",
        "runId": get(&request, "runId"),
        "tournamentId": T,
        "tournamentSlug": get(&request, "tournamentSlug"),
        "exchanges": members.iter().map(|m| json!({
            "marketId": get(m, "marketId"),
            "exchangeId": get(m, "exchangeId"),
        })).collect::<Vec<V>>(),
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
    let run_id = sget(&request, "runId").unwrap_or("").to_string();
    let legacy_dir = runtime_dir.join("baal-scaled-live-runs").join(&run_id);
    let legacy_controller_dir = legacy_dir.join("controller");
    let legacy_budget_dir = legacy_dir.join("budget");
    let legacy_file = legacy_controller_dir.join("baal-scaled-no-journal.jsonl");
    let original_bytes = read_bytes(&legacy_file)?;
    let seed = replay_baal_scaled_controller_v1(&json!({
        "bodyText": std::str::from_utf8(&original_bytes)
            .map_err(|e| codeless(e.to_string()))?,
        "expectedCodeIdentity": old_code,
        "expectedScope": scope,
        "expectedAllocation": allocation,
    }))?;
    let descriptor_keys = [
        "schemaVersion",
        "kind",
        "ownerId",
        "runId",
        "tournamentId",
        "universeHash",
        "originalRequestHash",
        "priorV5UpgradeHash",
        "originalV5GraphHash",
        "oldEntryExpiresAt",
        "legacyAnchor",
        "legacyCodeIdentityHash",
        "seedHash",
        "activation",
        "migration",
        "firstExchangeId",
        "codeIdentity",
        "recordedAt",
    ];
    need(
        sorted_keys(&d).join("|") == descriptor_keys.join("|")
            && js_int(get(&d, "schemaVersion")) == Some(1)
            && sget(&d, "kind") == Some("baal_combined_custody_host_v6")
            && sget(&d, "ownerId") == sget(&scope, "ownerId")
            && sget(&d, "runId") == sget(&scope, "runId")
            && sget(&d, "tournamentId") == Some(T)
            && sget(&d, "universeHash") == sget(universe, "hash")
            && sget(&d, "originalRequestHash") == hashes[SOURCE_FILES[0]].as_str()
            && sget(&d, "priorV5UpgradeHash") == hashes[SOURCE_FILES[3]].as_str()
            && sget(&d, "originalV5GraphHash") == Some(v5hash.as_str())
            && sget(&d, "oldEntryExpiresAt") == sget(&request, "entryExpiresAt")
            && same(get(&d, "legacyAnchor"), get(&seed, "anchor"))
            && sget(&d, "legacyCodeIdentityHash")
                == sget(&seed, "legacyCodeIdentityHash")
            && sget(&d, "seedHash") == Some(payload_hash(&seed)?.as_str())
            && same(get(&d, "codeIdentity"), &code)
            && instant_v(get(&d, "recordedAt")) <= now_ms
            && instant_v(get(&d, "recordedAt")) >= instant_v(get(&v5, "recordedAt")),
        "DESCRIPTOR",
    )?;
    let activation = get(&d, "activation").clone();
    let migration = get(&d, "migration").clone();
    let epoch = sget(&migration, "implementationEpoch").unwrap_or("").to_string();
    need(
        !epoch.is_empty()
            && epoch.len() <= 100
            && epoch
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            && sget(&migration, "authorityHash") == sget(&request, "authorityHash")
            && sget(&activation, "authorityHash") == sget(&request, "authorityHash")
            && get(&activation, "allowedFirstExchangeIds")
                .as_array()
                .is_some_and(|a| a.len() == 1)
            && get(&activation, "allowedFirstExchangeIds")
                .as_array()
                .and_then(|a| a.first())
                .and_then(V::as_str)
                == sget(&d, "firstExchangeId")
            && sget(&activation, "codeIdentityHash")
                == Some(payload_hash(&live_coordinator_identity(workspace)?)?.as_str())
            && js_int(get(
                &get(
                    &get(
                        &get(&get(&seed, "state"), "inventory"),
                        sget(&d, "firstExchangeId").unwrap_or(""),
                    ),
                    "proof",
                ),
                "remainingOwnedQuantity",
            ))
            .unwrap_or(0)
                > 0
            && instant_v(get(&activation, "startedAt")) <= now_ms,
        "ACTIVATION_EPOCH",
    )?;
    let dir = runtime_dir
        .join("baal-combined-custody-runs")
        .join(&run_id)
        .join(&epoch);
    let coordinator_dir = dir.join("coordinator");
    let budget_dir = dir.join("budget");
    let lease_dir = runtime_dir.join("baal-financial-owner");
    let lease_journal_file = dir.join("financial-owner-epochs.jsonl");
    let stop_file = runtime_dir.join("EXECUTION_STOP");

    let inner = HostInner {
        deps,
        runtime_dir: runtime_dir.to_path_buf(),
        workspace: workspace.to_path_buf(),
        universe: universe.clone(),
        descriptor_file: descriptor_file.clone(),
        descriptor_bytes_hash,
        descriptor_hash: descriptor_hash.clone(),
        descriptor: d.clone(),
        activation: activation.clone(),
        migration: migration.clone(),
        epoch: epoch.clone(),
        request: request.clone(),
        source_files,
        source_hashes,
        code: code.clone(),
        code_files,
        scope: scope.clone(),
        allocation: allocation.clone(),
        seed: seed.clone(),
        old_code: old_code.clone(),
        dir: dir.clone(),
        coordinator_dir: coordinator_dir.clone(),
        budget_dir: budget_dir.clone(),
        lease_dir: lease_dir.clone(),
        lease_journal_file: lease_journal_file.clone(),
        legacy_controller_dir: legacy_controller_dir.clone(),
        legacy_budget_dir: legacy_budget_dir.clone(),
        legacy_file: legacy_file.clone(),
        stop_file: stop_file.clone(),
        coordinator: None,
        budget: None,
        financial_lease: None,
        legacy_controller_fence: None,
        legacy_budget_fence: None,
        epoch_journal: None,
        epoch_receipt: V::Null,
        raw_account: None,
        cash_receipt: None,
        closed: false,
        failed: false,
        busy: false,
        last_failure: None,
        last_action: "awaiting_fresh_cash".to_string(),
        last_step_at: None,
        bridges: HashMap::new(),
        selected_bridge: None,
        permits: HashMap::new(),
        timings: Vec::new(),
        stage_timings: Vec::new(),
        exit_skipped: 0,
        last_hint: V::Null,
        sell_cursor: 0,
        hedge_tried_at: None,
        last_graph_at: None,
        last_book_at: None,
        packet_ready_at: None,
        last_dispatch_timing: V::Null,
        last_rejected_timing: V::Null,
        last_exit_priority: V::Null,
    };

    let inner = Rc::new(RefCell::new(inner));
    let init: Result<(), String> = (|| {
        immutable(&inner)?;
        legacy(&inner)?;
        let raw = actual_account(&inner)?;
        inner.borrow_mut().raw_account = Some(raw);
        need(
            !legacy_controller_dir.join("execution-owner.lock").exists()
                && !legacy_budget_dir.join("execution-owner.lock").exists(),
            "ORIGINAL_WRITER_STILL_OWNS",
        )?;
        let owner_id = sget(&scope, "ownerId").unwrap_or("").to_string();
        let financial_lease = acquire_owner_lease(
            &lease_dir,
            &owner_id,
            &run_id,
            "baal-global-financial-owner",
            now(&inner),
        )
        .map_err(|e| codeless(e.message))?;
        let controller_fence = acquire_owner_lease(
            &legacy_controller_dir,
            &owner_id,
            &run_id,
            "baal-custody-v6-legacy-fence",
            now(&inner),
        )
        .map_err(|e| codeless(e.message))?;
        let budget_fence = acquire_owner_lease(
            &legacy_budget_dir,
            &owner_id,
            &run_id,
            "baal-custody-v6-legacy-budget-fence",
            now(&inner),
        )
        .map_err(|e| codeless(e.message))?;
        fs::create_dir_all(&dir).map_err(|e| codeless(e.to_string()))?;
        let mut epoch_journal =
            Journal::open(&lease_journal_file).map_err(|e| codeless(e.to_string()))?;
        if lease_journal_file.exists() {
            let bytes = read_bytes(&lease_journal_file)?;
            need(
                bytes.is_empty() || bytes.last() == Some(&10),
                "PARTIAL_OWNER_JOURNAL",
            )?;
            for line in journal_lines(&lease_journal_file, &JournalReadOptions::default())
                .map_err(|e| codeless(e))?
            {
                if line.trim().is_empty() {
                    continue;
                }
                let r: V =
                    serde_json::from_str(&line).map_err(|e| codeless(e.to_string()))?;
                need(
                    sget(&get(&r, "event"), "kind") == Some("financial_owner_bound")
                        && sget(&get(&r, "state"), "descriptorHash")
                            == Some(descriptor_hash.as_str())
                        && sget(&get(&r, "state"), "seedHash") == sget(&d, "seedHash")
                        && sget(&get(&r, "state"), "implementationEpoch")
                            == Some(epoch.as_str())
                        && sget(&get(&r, "state"), "ownerId") == sget(&scope, "ownerId")
                        && sget(&get(&r, "state"), "runId") == sget(&scope, "runId")
                        && sget(&get(&r, "state"), "tournamentId") == Some(T)
                        && hex(get(&get(&get(&r, "state"), "rawAccountLease"), "leaseId"))
                        && sget(&get(&get(&r, "state"), "financialOwnerLock"), "runId")
                            == sget(&scope, "runId")
                        && sget(&get(&get(&r, "state"), "financialOwnerLock"), "scope")
                            == Some("baal-global-financial-owner")
                        && js_int(get(&get(&get(&r, "state"), "financialOwnerLock"), "pid"))
                            .unwrap_or(0)
                            > 0
                        && hex(get(&get(&get(&r, "state"), "financialOwnerLock"), "identityHash"))
                        && get(&get(&r, "state"), "financialOwnerLock")
                            .get("token")
                            .is_none()
                        && get(&get(&r, "state"), "legacyControllerFence")
                            .get("token")
                            .is_none()
                        && get(&get(&r, "state"), "legacyBudgetFence")
                            .get("token")
                            .is_none(),
                    "OWNER_JOURNAL_BINDING",
                )?;
            }
        }
        let epoch_receipt = json!({
            "schemaVersion": 1,
            "kind": "derived_financial_run_account_lease_v1",
            "ownerId": get(&scope, "ownerId"),
            "runId": get(&scope, "runId"),
            "tournamentId": T,
            "implementationEpoch": epoch,
            "descriptorHash": descriptor_hash,
            "seedHash": get(&d, "seedHash"),
            "rawAccountLease": inner.borrow().raw_account.clone().unwrap_or(V::Null),
            "financialOwnerLock": lock_summary(&financial_lease.identity)?,
            "legacyControllerFence": lock_summary(&controller_fence.identity)?,
            "legacyBudgetFence": lock_summary(&budget_fence.identity)?,
            "at": iso(now(&inner)),
        });
        epoch_journal
            .commit(&epoch_receipt, &json!({"kind": "financial_owner_bound"}))
            .map_err(|e| codeless(e))?;
        {
            let mut g = inner.borrow_mut();
            g.financial_lease = Some(financial_lease);
            g.legacy_controller_fence = Some(controller_fence);
            g.legacy_budget_fence = Some(budget_fence);
            g.epoch_journal = Some(epoch_journal);
            g.epoch_receipt = epoch_receipt;
        }
        leases(&inner)?;
        // coordinator=openBaalNeutralOwnedCounterpartPilotV2({...})
        let weak = Rc::downgrade(&inner);
        let pilot_deps = NeutralPilotDeps {
            dispatch: inner.borrow().deps.api.borrow().dispatch.clone(),
            raw_receipt_of: {
                let w = weak.clone();
                Rc::new(move |response: &V, meta: &V| -> Result<V, String> {
                    let i = w.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
                    raw_of(&i, response, meta)
                })
            },
            assert_account_lease: {
                let w = weak.clone();
                Rc::new(move || -> Result<V, String> {
                    let i = w.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
                    leases(&i)
                })
            },
            assert_financial_owner_lease: {
                let w = weak.clone();
                Rc::new(move || -> Result<V, String> {
                    let i = w.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
                    financial(&i)
                })
            },
            install_funding_reservation: {
                let w = weak.clone();
                Rc::new(move |r: &V| -> Result<V, String> {
                    let i = w.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
                    let b = i.borrow().budget.clone();
                    b.ok_or_else(|| {
                        codeless(
                            "TypeError: Cannot read properties of null (reading 'installFundingReservation')"
                                .to_string(),
                        )
                    })?
                    .install_funding_reservation(r)
                })
            },
            get_global_risk: {
                let w = weak.clone();
                Rc::new(move || -> Result<V, String> {
                    let i = w.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
                    timed(&i, "current_budget_risk", || {
                        let b = i.borrow().budget.clone();
                        b.ok_or_else(|| {
                            codeless(
                                "TypeError: Cannot read properties of null (reading 'globalRisk')"
                                    .to_string(),
                            )
                        })?
                        .global_risk()
                    })
                })
            },
            get_evidence: {
                let w = weak.clone();
                Rc::new(move |c: &V| -> Result<V, String> {
                    let i = w.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
                    let b = i.borrow().selected_bridge.clone();
                    b.ok_or_else(|| {
                        codeless(
                            "TypeError: Cannot read properties of null (reading 'getEvidence')"
                                .to_string(),
                        )
                    })?
                    .get_evidence(c)
                })
            },
            on_financial_halt: {
                let w = weak.clone();
                Rc::new(move |input: &V| -> Result<V, String> {
                    let i = w.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
                    halt(&i, input)
                })
            },
            can_operate: {
                let w = weak.clone();
                Rc::new(move || -> bool {
                    w.upgrade().map(|i| guard(&i).is_ok()).unwrap_or(false)
                })
            },
            can_check_mechanical: {
                let w = weak.clone();
                Some(Rc::new(move || -> bool {
                    w.upgrade()
                        .map(|i| mechanical_leases(&i).is_ok())
                        .unwrap_or(false)
                }))
            },
            assert_mechanical_account_lease: {
                let w = weak.clone();
                Some(Rc::new(move || -> Result<V, String> {
                    let i = w.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
                    mechanical_leases(&i)
                }))
            },
            assert_mechanical_financial_owner_lease: {
                let w = weak.clone();
                Some(Rc::new(move || -> Result<V, String> {
                    let i = w.upgrade().ok_or("BAAL_CUSTODY_V6_CLOSED")?;
                    let mut v = mechanical_leases(&i)?;
                    let epoch = i.borrow().epoch.clone();
                    v.as_object_mut()
                        .unwrap()
                        .insert("implementationEpoch".into(), json!(epoch));
                    Ok(v)
                }))
            },
            now: inner.borrow().deps.now.clone(),
            workspace_dir: workspace.to_path_buf(),
        };
        let coordinator = BaalNeutralOwnedCounterpartPilot::open(
            &coordinator_dir,
            workspace,
            &legacy_file,
            &old_code,
            &scope,
            &allocation,
            &activation,
            &migration,
            pilot_deps,
        )?;
        inner.borrow_mut().coordinator = Some(Rc::new(coordinator));
        Ok(())
    })();
    if let Err(e) = init {
        let mut g = inner.borrow_mut();
        for b in g.bridges.values() {
            let _ = b.close();
        }
        if let Some(b) = &g.budget {
            b.close();
        }
        if let Some(c) = &g.coordinator {
            let _ = c.close();
        }
        for mut l in [
            g.legacy_budget_fence.take(),
            g.legacy_controller_fence.take(),
            g.financial_lease.take(),
        ]
        .into_iter()
        .flatten()
        {
            let _ = l.release();
        }
        drop(g);
        return Err(e);
    }
    Ok(Some(BaalCombinedCustodyHostV6 { inner }))
}
