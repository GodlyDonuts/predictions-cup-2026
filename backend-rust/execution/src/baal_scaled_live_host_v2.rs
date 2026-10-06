// execution/baal-scaled-live-host-v2.mjs — the scaled live production host:
// human-signed request -> controller + shared budget + evidence bridge +
// candidate screening step loop.
//
// Port notes:
// - async dispatch/step become synchronous calls; `sync(fn)` is a direct call.
// - `api` is the caller-owned mutable object {dispatch, manualReview, cooldown,
//   next, onAdmission} — modelled as Rc<RefCell<BaalScaledLiveApi>> shared with
//   the bridge (adapter maps WireDispatchError -> String) and controller.
// - `bootstrapPermits` WeakMap -> HashMap<usize,V> keyed by before_dispatch
//   guard pointer; entries store {endpoint,beforeNetworkPtr}.
// - backoff(e): the ported error channel carries no retryAfterMs, so there is
//   nothing to persist; durable failures propagate via the ":fatal:" marker.
use crate::baal_scaled_no_controller_v1::{
    open_baal_scaled_no_controller, BaalScaledNoController, BaalScaledNoControllerDeps,
    WireDispatch, WireDispatchError,
};
use crate::baal_scaled_no_evidence_bridge_v1::{
    create_baal_scaled_no_evidence_bridge, BaalScaledNoEvidenceBridge,
    BaalScaledNoEvidenceBridgeDeps,
};
use crate::baal_scaled_live_host_v1::baal_scaled_live_identity;
use crate::baal_scaled_owned_exit_read_hint_v1::owned_no_exit_read_hint;
use crate::baal_scaled_shared_budget_v1::{
    open_baal_scaled_shared_budget, BaalScaledSharedBudget, BaalScaledSharedBudgetDeps,
};
use crate::core::{payload_hash, validate_execution_state};
use crate::frozen_paper_graph::collect_frozen_paper_files;
use crate::no_rest_reconciliation_v2::rounded_tournament_cash_interval;
use crate::party_winner_no_screen_v1::{party_winner_quote_hash, screen_party_winner_no_pairs};
use crate::transport::Guard;
use num_bigint::BigInt;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::fixed::instant;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_int, js_truthy};
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub const T: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
const PREFIX: &str = "BAAL_SCALED_HOST_";

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("{PREFIX}{code}"))
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
fn id(x: &V) -> bool {
    x.as_str()
        .map(|s| {
            !s.is_empty()
                && s.as_bytes()[0] != b'0'
                && s.bytes().all(|b| b.is_ascii_digit())
                && s.parse::<i64>().is_ok()
        })
        .unwrap_or(false)
}
fn iso(n: f64) -> String {
    ms_to_iso(n)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or("")).map_err(|e| e)
}
fn read(path: &Path) -> Result<V, String> {
    serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn is_fatal(e: &str) -> bool {
    e.contains(":fatal:") || e.contains("durableRecordingFailed")
}
fn is_all_caps_code(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().next() == Some(b'A')
        && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}
fn sorted_keys_match(v: &V, keys: &[&str]) -> bool {
    let mut got: Vec<String> = v
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    got.sort();
    let mut want: Vec<String> = keys.iter().map(|s| s.to_string()).collect();
    want.sort();
    got.join("|") == want.join("|")
}
fn run_id_ok(s: &str) -> bool {
    // /^baal-scaled-no-[a-z0-9-]{1,80}$/
    s.strip_prefix("baal-scaled-no-").is_some_and(|t| {
        !t.is_empty()
            && t.len() <= 80
            && t.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}

/// baalScaledLiveV2Identity(workspace)
pub fn baal_scaled_live_v2_identity(workspace: &Path) -> Result<V, String> {
    let files = collect_frozen_paper_files(
        workspace,
        &[
            "execution/baal-scaled-live-host-v2.mjs".to_string(),
            "execution/scoped-rest-receipts-v2.mjs".to_string(),
        ],
    )?;
    serde_json::to_value(files).map_err(|e| e.to_string())
}

/// The mutable shared api object the caller owns:
/// {dispatch, manualReview, cooldown, next, onAdmission}.
pub struct BaalScaledLiveApiV2 {
    pub dispatch: Rc<WireDispatch>,
    pub manual_review: bool,
    pub cooldown: f64,
    pub next: f64,
    pub on_admission: Box<dyn FnMut(Option<f64>)>,
}

pub struct BaalScaledLiveHostV2Deps {
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub get_account_lease: Rc<dyn Fn() -> V>,
    pub get_other_state: Rc<dyn Fn() -> V>,
    pub get_legacy_risk: Rc<dyn Fn() -> V>,
    pub get_pair_read_hints: Rc<dyn Fn() -> Option<V>>,
    pub get_read_hint: Rc<dyn Fn() -> Option<V>>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> bool>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct HostInner {
    runtime_dir: PathBuf,
    workspace: PathBuf,
    file: PathBuf,
    capital_file: PathBuf,
    authority_file: PathBuf,
    catalog_file: PathBuf,
    request: V,
    request_hash: String,
    catalog: V,
    code: V,
    scope: V,
    allocation: V,
    allocation_hash: String,
    budget_installed_hash: String,
    universe: V,
    members: Vec<V>,
    controller_dir: PathBuf,
    budget_dir: PathBuf,
    api: Rc<RefCell<BaalScaledLiveApiV2>>,
    deps: BaalScaledLiveHostV2Deps,
    controller: Option<Rc<BaalScaledNoController>>,
    budget: Option<Rc<BaalScaledSharedBudget>>,
    bridge: Option<Rc<BaalScaledNoEvidenceBridge>>,
    controller_cell: Rc<RefCell<Option<Rc<BaalScaledNoController>>>>,
    budget_cell: Rc<RefCell<Option<Rc<BaalScaledSharedBudget>>>>,
    bridge_cell: Rc<RefCell<Option<Rc<BaalScaledNoEvidenceBridge>>>>,
    cash_receipt: Option<V>,
    positions_receipt: Option<V>,
    closed: bool,
    failed: bool,
    busy: bool,
    last_failure: Option<String>,
    last_action: String,
    last_step_at: Option<String>,
    issued: HashMap<usize, V>,
    cooldowns: HashMap<String, f64>,
    sell_checks: HashMap<String, f64>,
    upgrade_file: PathBuf,
    upgrade_hash: String,
    original_host_code_identity_hash: String,
    exit_hint_skipped_packets: i64,
    last_exit_hint: Option<V>,
}

fn allowed(inner: &Rc<RefCell<HostInner>>) -> bool {
    (|| -> Result<bool, String> {
        let (closed, failed, can_operate) = {
            let g = inner.borrow();
            (g.closed, g.failed, g.deps.can_operate.clone())
        };
        need(!closed && !failed && can_operate(), "STOPPED")?;
        let (file, capital_file, authority_file, catalog_file, workspace, request_hash, request, code) = {
            let g = inner.borrow();
            (
                g.file.clone(),
                g.capital_file.clone(),
                g.authority_file.clone(),
                g.catalog_file.clone(),
                g.workspace.clone(),
                g.request_hash.clone(),
                g.request.clone(),
                g.code.clone(),
            )
        };
        let (upgrade_file, upgrade_hash, original_hash) = {
            let g = inner.borrow();
            (
                g.upgrade_file.clone(),
                g.upgrade_hash.clone(),
                g.original_host_code_identity_hash.clone(),
            )
        };
        Ok(payload_hash(&read(&file)?)? == request_hash
            && payload_hash(&read(&capital_file)?)? == get(&request, "humanAllocationSourceHash").as_str().unwrap_or("")
            && payload_hash(&read(&authority_file)?)? == get(&request, "authorityHash").as_str().unwrap_or("")
            && payload_hash(&read(&catalog_file)?)? == get(&request, "catalogHash").as_str().unwrap_or("")
            && payload_hash(&baal_scaled_live_identity(&workspace)?)? == original_hash
            && payload_hash(&read(&upgrade_file)?)? == upgrade_hash
            && payload_hash(&baal_scaled_live_v2_identity(&workspace)?)? == payload_hash(&code)?)
    })()
    .unwrap_or(false)
}

fn guard(inner: &Rc<RefCell<HostInner>>) -> Result<(), String> {
    need(allowed(inner), "STOPPED")?;
    legacy(inner)?;
    let l = (inner.borrow().deps.get_account_lease)();
    need(
        get(&l, "ownerId").as_str() == Some("Codex")
            && get(&l, "tournamentId").as_str() == Some(T)
            && hex(get(&l, "leaseId")),
        "ACTUAL_ACCOUNT_LEASE",
    )?;
    Ok(())
}

// legacy() — the retired legacy executor must be verifiably flat.
fn legacy(inner: &Rc<RefCell<HostInner>>) -> Result<V, String> {
    let (get_other, get_risk) = {
        let g = inner.borrow();
        (g.deps.get_other_state.clone(), g.deps.get_legacy_risk.clone())
    };
    let core = validate_execution_state(&get_other())?;
    let retired = get_risk();
    let pending: Vec<V> = get(&core, "intents")
        .as_object()
        .map(|m| {
            m.values()
                .filter(|i| get(i, "status").as_str() != Some("closed"))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let owned: Vec<V> = get(&core, "owned")
        .as_object()
        .map(|m| {
            m.values()
                .filter(|p| js_int(get(p, "quantity")).unwrap_or(0) > 0)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    need(
        get(&retired, "complete") == &json!(true)
            && hex(get(&retired, "sourceHash"))
            && get(&retired, "currentOwnedQuantity") == &json!(0)
            && get(&retired, "currentReservedCash").as_str() == Some("0.000000")
            && get(&retired, "unresolvedIntentIds")
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(false)
            && pending.is_empty()
            && owned.is_empty(),
        "LEGACY_NOT_VERIFIED_RETIRED",
    )?;
    Ok(json!({
        "coreHash": payload_hash(&core)?,
        "retirementHash": get(&retired, "sourceHash"),
    }))
}

// remember(receipt, method, endpoint) — keep the two raw tournament receipts.
fn remember(inner: &Rc<RefCell<HostInner>>, receipt: &V, method: &str, endpoint: &str) -> Result<V, String> {
    let slug = inner
        .borrow()
        .scope
        .get("tournamentSlug")
        .and_then(V::as_str)
        .unwrap_or("")
        .to_string();
    need(
        get(receipt, "method").as_str() == Some(method)
            && get(receipt, "endpoint").as_str() == Some(endpoint),
        "RAW_CALLBACK_SCOPE",
    )?;
    if method == "GET" && endpoint == format!("/tournaments/{slug}") {
        inner.borrow_mut().cash_receipt = Some(receipt.clone());
    }
    if method == "GET" && endpoint == format!("/tournaments/{slug}/portfolio/positions") {
        inner.borrow_mut().positions_receipt = Some(receipt.clone());
    }
    Ok(receipt.clone())
}

fn raw_of(inner: &Rc<RefCell<HostInner>>, response: &V, meta: &V) -> Result<V, String> {
    let receipt = (inner.borrow().deps.receipt_of)(response, meta)?;
    let method = get(meta, "method").as_str().unwrap_or("").to_string();
    let endpoint = get(response, "endpoint").as_str().unwrap_or("").to_string();
    remember(inner, &receipt, &method, &endpoint)
}

fn observation(inner: &Rc<RefCell<HostInner>>) -> Result<V, String> {
    need(allowed(inner), "STOPPED")?;
    let l = legacy(inner)?;
    let (cash_receipt, scope, now) = {
        let g = inner.borrow();
        (
            g.cash_receipt.clone(),
            g.scope.clone(),
            g.deps.now.clone(),
        )
    };
    let cash_receipt = cash_receipt.ok_or(format!("{PREFIX}ACTUAL_CASH_NOT_YET_OBSERVED"))?;
    let cash = rounded_tournament_cash_interval(
        &cash_receipt,
        &json!({
            "tournamentId": T,
            "tournamentSlug": get(&scope, "tournamentSlug"),
            "readAt": iso(now()),
        }),
    )?;
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
        "at": iso(now()),
        "sourceHash": payload_hash(&json!({
            "legacy": l,
            "cash": get(&cash, "sourceBodyHash"),
            "lease": payload_hash(&lease)?,
        }))?,
        "owned": [],
        "reservations": [],
        "loans": [],
    }))
}

fn status(inner: &Rc<RefCell<HostInner>>) -> V {
    let g = inner.borrow();
    json!({
        "schemaVersion": 1,
        "hostVersion": 2,
        "upgradeHash": g.upgrade_hash,
        "originalHostCodeIdentityHash": g.original_host_code_identity_hash,
        "hostCodeIdentityHash": payload_hash(&g.code).unwrap_or_default(),
        "exitHintSkippedPackets": g.exit_hint_skipped_packets,
        "exitHintCounterScope": "current_host_process",
        "lastExitHint": g.last_exit_hint.clone().unwrap_or(V::Null),
        "mode": "baal_scaled_live_production_v1",
        "ownerId": "Codex",
        "runId": get(&g.scope, "runId"),
        "observedAt": iso((g.deps.now)()),
        "allocationHash": g.allocation_hash,
        "budgetInstalledHash": g.budget_installed_hash,
        "requestHash": g.request_hash,
        "entryExpiresAt": get(&g.request, "entryExpiresAt"),
        "lastStepAt": g.last_step_at.clone().map(V::from).unwrap_or(V::Null),
        "lastAction": g.last_action,
        "lastFailure": g.last_failure.clone().map(V::from).unwrap_or(V::Null),
        "busy": g.busy,
        "closed": g.closed,
        "failed": g.failed,
        "capital": "1000.000000",
        "maximumActiveMarkets": 20,
        "controller": g.controller.as_ref().map(|c| c.status()).unwrap_or(V::Null),
        "budget": g.budget.as_ref().map(|b| b.status()).unwrap_or(V::Null),
        "bridge": g.bridge.as_ref().map(|b| b.status()).unwrap_or(V::Null),
        "readBootstrapComplete": g.cash_receipt.is_some(),
        "profitabilityEstablished": false,
        "additionalApiClients": 0,
        "additionalPersistentProcesses": 0,
    })
}

fn publish(inner: &Rc<RefCell<HostInner>>, action: &str) -> Result<V, String> {
    {
        let mut g = inner.borrow_mut();
        g.last_action = action.to_string();
        g.last_step_at = Some(iso((g.deps.now)()));
    }
    let out = status(inner);
    let file = inner.borrow().runtime_dir.join("baal-scaled-live-stats.json");
    atomic_json(&file, &out).map_err(|e| format!("{e}:fatal:durableRecordingFailed"))?;
    Ok(out)
}

fn backoff(_inner: &Rc<RefCell<HostInner>>, _e: &str) {
    // backoff(e): the ported error channel carries no retryAfterMs, so there
    // is nothing to persist — api.cooldown/api.next are never advanced here.
}

// candidates() — screen party-winner NO pairs over the curated quotes.
fn candidates(inner: &Rc<RefCell<HostInner>>) -> Result<Vec<Vec<V>>, String> {
    let (get_hints, positions_receipt, controller, universe, members, catalog, request, now) = {
        let g = inner.borrow();
        (
            g.deps.get_pair_read_hints.clone(),
            g.positions_receipt.clone(),
            g.controller.clone(),
            g.universe.clone(),
            g.members.clone(),
            g.catalog.clone(),
            g.request.clone(),
            g.deps.now.clone(),
        )
    };
    let hints = match get_hints() {
        Some(h) if !h.is_null() => h,
        _ => return Ok(vec![]),
    };
    let quotes = hints
        .get("quoteCaptures")
        .filter(|q| !q.is_null())
        .or_else(|| hints.get("quotes"))
        .and_then(V::as_array)
        .cloned()
        .unwrap_or_default();
    let mut excluded: HashSet<String> = HashSet::new();
    if let Some(pr) = &positions_receipt {
        if let Ok(body) = serde_json::from_str::<V>(get(pr, "bodyText").as_str().unwrap_or("")) {
            for p in get(&body, "positions").as_array().cloned().unwrap_or_default() {
                if js_truthy(Some(get(&p, "quantity"))) {
                    excluded.insert(get(&p, "exchangeId").as_str().unwrap_or("").to_string());
                }
            }
        }
    }
    if let Some(c) = &controller {
        let st = c.status();
        if let Some(inv) = get(&st, "inventory").as_object() {
            for (ex, v) in inv {
                if js_int(get(v, "quantity")).unwrap_or(0) > 0 {
                    excluded.insert(ex.clone());
                }
            }
        }
    }
    let mut captured: Vec<V> = Vec::new();
    for q in quotes {
        let mut c = q.clone();
        let need_hash = c.get("quoteHash").is_none() || get(&c, "quoteHash").is_null();
        let qh = if need_hash {
            Some(party_winner_quote_hash(&c)?)
        } else {
            None
        };
        if let V::Object(m) = &mut c {
            m.insert("tournamentId".into(), json!(T));
            m.insert(
                "knownAt".into(),
                if q.get("knownAt").is_some_and(|x| !x.is_null()) {
                    get(&q, "knownAt").clone()
                } else {
                    get(&q, "receivedAt").clone()
                },
            );
            m.insert(
                "sourceHash".into(),
                if q.get("sourceHash").is_some_and(|x| !x.is_null()) {
                    get(&q, "sourceHash").clone()
                } else {
                    get(&hints, "sourceHash").clone()
                },
            );
            if let Some(h) = qh {
                m.insert("quoteHash".into(), json!(h));
            }
        }
        captured.push(c);
    }
    let uni = {
        let mut u = universe.clone();
        if let V::Object(m) = &mut u {
            m.insert("exchanges".into(), V::Array(members.clone()));
        }
        u
    };
    let screen = screen_party_winner_no_pairs(&json!({
        "universe": uni,
        "rootCaptures": get(&catalog, "rootCaptures"),
        "quoteCaptures": captured,
        "account": V::Null,
        "now": iso(now()),
    }))?;
    let cooldowns = inner.borrow().cooldowns.clone();
    let n = now();
    let out: Vec<Vec<V>> = get(&screen, "pairs")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| {
            get(p, "indicativeMathValid") == &json!(true)
                && get(p, "quotesCurrentAndCofresh") == &json!(true)
                && get(p, "exchangeIds")
                    .as_array()
                    .map(|a| a.iter().all(|e| !excluded.contains(e.as_str().unwrap_or(""))))
                    .unwrap_or(false)
                && get(p, "protectiveSurplusQualified") == &json!(true)
                && get(p, "marketIds")
                    .as_array()
                    .map(|a| {
                        a.iter().all(|mid| {
                            get(&request, "allowedMarkets")
                                .as_array()
                                .map(|am| am.contains(mid))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
                && get(p, "marketIds").as_array().map(|a| {
                    a.iter().all(|mid| {
                        cooldowns.get(mid.as_str().unwrap_or("")).copied().unwrap_or(0.0) <= n
                    })
                }) == Some(true)
        })
        .map(|p| {
            get(&p, "marketIds")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .enumerate()
                .map(|(i, mid)| {
                    json!({
                        "marketId": mid,
                        "exchangeId": get(&p, "exchangeIds").as_array().and_then(|a| a.get(i)).cloned().unwrap_or(V::Null)
                    })
                })
                .collect()
        })
        .collect();
    Ok(out)
}

pub struct BaalScaledLiveHostV2 {
    inner: Rc<RefCell<HostInner>>,
}

/// openBaalScaledLiveHostV2({runtimeDir, workspace, universe, api, receiptOf,
/// getAccountLease, getOtherState, getLegacyRisk, getPairReadHints, getReadHint,
/// canOperate, hasReadHeadroom, now}) — None when no request file exists.
pub fn open_baal_scaled_live_host_v2(
    runtime_dir: &Path,
    workspace: &Path,
    universe: &V,
    api: Rc<RefCell<BaalScaledLiveApiV2>>,
    deps: BaalScaledLiveHostV2Deps,
) -> Result<Option<BaalScaledLiveHostV2>, String> {
    let file = runtime_dir.join("baal-scaled-live-request.json");
    let upgrade_file = runtime_dir.join("baal-scaled-host-upgrade.json");
    if !upgrade_file.exists() {
        return Ok(None);
    }
    need(runtime_dir.is_absolute() && workspace.is_absolute(), "DEPENDENCIES")?;
    need(file.exists(), "ORIGINAL_REQUEST_MISSING")?;
    let request = read(&file)?;
    let request_hash = payload_hash(&request)?;
    let keys = [
        "schemaVersion", "kind", "ownerId", "runId", "tournamentId",
        "tournamentSlug", "universeHash", "allowedMarkets",
        "humanAllocationSourceHash", "authorityHash", "catalogHash",
        "codeIdentity", "startedAt", "entryExpiresAt", "maximumActiveMarkets",
    ];
    need(
        sorted_keys_match(&request, &keys)
            && get(&request, "schemaVersion") == &json!(1)
            && get(&request, "kind").as_str() == Some("baal_scaled_live_production_v1")
            && get(&request, "ownerId").as_str() == Some("Codex")
            && get(&request, "runId").as_str().is_some_and(run_id_ok)
            && get(&request, "tournamentId").as_str() == Some(T)
            && get(&request, "tournamentSlug").as_str() == Some("midterm-elections")
            && get(&request, "maximumActiveMarkets") == &json!(20),
        "ROOT_REQUEST",
    )?;
    let capital_file = runtime_dir.join("baal-capital-allocation.json");
    let authority_file = runtime_dir.join("paired-live-authority.json");
    let catalog_file = runtime_dir.join("baal-scaled-rootcatalog.json");
    let capital = read(&capital_file)?;
    let authority = read(&authority_file)?;
    let catalog = read(&catalog_file)?;
    need(
        get(&request, "humanAllocationSourceHash").as_str() == Some(payload_hash(&capital)?.as_str())
            && get(&capital, "schemaVersion") == &json!(1)
            && get(&capital, "kind").as_str() == Some("human_directed_baal_capital_allocation_v1")
            && get(&capital, "ownerId").as_str() == Some("Codex")
            && get(&capital, "allocatedNominalCapital").as_str() == Some("1000.000000")
            && get(&capital, "maximumOrderNominalCost").as_str() == Some("25.000000")
            && get(&capital, "maximumMarketNominalExposure").as_str() == Some("100.000000")
            && get(&capital, "maximumEventNominalExposure").as_str() == Some("200.000000")
            && get(&capital, "maximumSharesPerOrder") == &json!(100)
            && get(&capital, "maximumActiveMarkets") == &json!(20)
            && get(&capital, "fee").as_str() == Some("0.000000"),
        "HUMAN_CAPITAL_SOURCE",
    )?;
    need(
        get(&request, "authorityHash").as_str() == Some(payload_hash(&authority)?.as_str())
            && get(&authority, "kind").as_str() == Some("direct_human_authorized_bot_update")
            && get(&authority, "ownerId").as_str() == Some("Codex")
            && get(&authority, "threadId").as_str() == Some("01a0f8cc-b9bb-77d2-9f4c-2844396f1b79")
            && get(&request, "catalogHash").as_str() == Some(payload_hash(&catalog)?.as_str())
            && get(&catalog, "rootCaptures").is_array()
            && get(&catalog, "tournamentId").as_str() == Some(T),
        "AUTHORITY_OR_CATALOG",
    )?;
    need(
        get(universe, "tournamentId").as_str() == Some(T)
            && get(universe, "hash") == get(&request, "universeHash")
            && get(universe, "exchanges").is_array()
            && get(&request, "allowedMarkets").is_array()
            && get(&request, "allowedMarkets").as_array().map(|a| a.len() >= 2 && a.len() <= 300).unwrap_or(false)
            && {
                let a = get(&request, "allowedMarkets").as_array().cloned().unwrap_or_default();
                let mut s: Vec<V> = Vec::new();
                for x in &a {
                    if !s.contains(x) {
                        s.push(x.clone());
                    }
                }
                s.len() == a.len()
            }
            && get(&request, "allowedMarkets")
                .as_array()
                .map(|a| a.iter().all(id))
                .unwrap_or(false),
        "ALLOWED_UNIVERSE",
    )?;
    let allowed_list = get(&request, "allowedMarkets").as_array().cloned().unwrap_or_default();
    let members: Vec<V> = get(universe, "exchanges")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| allowed_list.contains(get(m, "marketId")))
        .collect();
    need(
        members.len() == allowed_list.len()
            && members
                .iter()
                .all(|m| get(m, "option").as_str() == Some("YES") && id(get(m, "exchangeId"))),
        "EXACT_ALLOWED_MEMBERS",
    )?;
    need(
        instant_v(get(&request, "entryExpiresAt"))? > instant_v(get(&request, "startedAt"))?
            && instant_v(get(&request, "entryExpiresAt"))? - instant_v(get(&request, "startedAt"))? <= 3_600_000
            && instant_v(get(&request, "startedAt"))? <= (deps.now)() as i64,
        "NEW_ENTRY_WINDOW",
    )?;
    let original_code = baal_scaled_live_identity(workspace)?;
    let original_host_code_identity_hash = payload_hash(&original_code)?;
    need(
        original_host_code_identity_hash == payload_hash(get(&request, "codeIdentity"))?,
        "FROZEN_ORIGINAL_HOST_SOURCE",
    )?;
    let code = baal_scaled_live_v2_identity(workspace)?;
    let upgrade = read(&upgrade_file)?;
    let upgrade_hash = payload_hash(&upgrade)?;
    let upgrade_keys = [
        "schemaVersion", "kind", "ownerId", "runId", "tournamentId",
        "originalRequestHash", "originalHostCodeIdentityHash", "codeIdentity",
        "entryExpiresAt", "recordedAt",
    ];
    need(
        sorted_keys_match(&upgrade, &upgrade_keys)
            && get(&upgrade, "schemaVersion") == &json!(1)
            && get(&upgrade, "kind").as_str() == Some("baal_scaled_host_upgrade_v2")
            && get(&upgrade, "ownerId") == get(&request, "ownerId")
            && get(&upgrade, "runId") == get(&request, "runId")
            && get(&upgrade, "tournamentId").as_str() == Some(T)
            && get(&upgrade, "originalRequestHash").as_str() == Some(request_hash.as_str())
            && get(&upgrade, "originalHostCodeIdentityHash").as_str()
                == Some(original_host_code_identity_hash.as_str())
            && payload_hash(get(&upgrade, "codeIdentity"))? == payload_hash(&code)?
            && get(&upgrade, "entryExpiresAt") == get(&request, "entryExpiresAt")
            && instant_v(get(&upgrade, "recordedAt"))? >= instant_v(get(&request, "startedAt"))?
            && instant_v(get(&upgrade, "recordedAt"))? <= (deps.now)() as i64,
        "HOST_UPGRADE_BINDING",
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
    let allocation_hash = payload_hash(&allocation)?;
    let budget_installed_hash = payload_hash(&json!({
        "scope": scope.clone(),
        "allocation": allocation.clone(),
        "requestHash": request_hash.clone(),
        "authorityHash": get(&request, "authorityHash"),
    }))?;
    let activation = json!({
        "schemaVersion": 1,
        "mode": "baal_scaled_no_production_v1",
        "ownerId": "Codex",
        "runId": get(&request, "runId"),
        "tournamentId": T,
        "allocationHash": allocation_hash,
        "scopeHash": payload_hash(&scope)?,
        "authorityHash": get(&request, "authorityHash"),
        "budgetInstalledHash": budget_installed_hash,
        "codeIdentityHash": payload_hash(&crate::baal_scaled_no_controller_v1::baal_scaled_no_controller_identity()?)?,
        "startedAt": get(&request, "startedAt"),
        "entryExpiresAt": get(&request, "entryExpiresAt"),
    });
    let dir = runtime_dir.join("baal-scaled-live-runs").join(get(&request, "runId").as_str().unwrap_or(""));
    let controller_dir = dir.join("controller");
    let budget_dir = dir.join("budget");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let controller_cell: Rc<RefCell<Option<Rc<BaalScaledNoController>>>> = Rc::new(RefCell::new(None));
    let budget_cell: Rc<RefCell<Option<Rc<BaalScaledSharedBudget>>>> = Rc::new(RefCell::new(None));
    let bridge_cell: Rc<RefCell<Option<Rc<BaalScaledNoEvidenceBridge>>>> = Rc::new(RefCell::new(None));
    let inner = Rc::new(RefCell::new(HostInner {
        runtime_dir: runtime_dir.to_path_buf(),
        workspace: workspace.to_path_buf(),
        file: file.clone(),
        capital_file,
        authority_file,
        catalog_file,
        request: request.clone(),
        request_hash,
        catalog,
        code,
        scope,
        allocation,
        allocation_hash,
        budget_installed_hash,
        universe: universe.clone(),
        members,
        controller_dir: controller_dir.clone(),
        budget_dir: budget_dir.clone(),
        api,
        deps,
        controller: None,
        budget: None,
        bridge: None,
        controller_cell: controller_cell.clone(),
        budget_cell: budget_cell.clone(),
        bridge_cell: bridge_cell.clone(),
        cash_receipt: None,
        positions_receipt: None,
        closed: false,
        failed: false,
        busy: false,
        last_failure: None,
        last_action: "initialized_read_only".to_string(),
        last_step_at: None,
        issued: HashMap::new(),
        cooldowns: HashMap::new(),
        sell_checks: HashMap::new(),
        upgrade_file: upgrade_file.clone(),
        upgrade_hash,
        original_host_code_identity_hash,
        exit_hint_skipped_packets: 0,
        last_exit_hint: None,
    }));
    let init: Result<(), String> = (|| {
        guard(&inner)?;
        let (scope, allocation, activation, request) = {
            let g = inner.borrow();
            (
                g.scope.clone(),
                g.allocation.clone(),
                activation.clone(),
                g.request.clone(),
            )
        };
        let inner_a = inner.clone();
        let dispatch: Rc<WireDispatch> = Rc::new(move |spec: &V, bd: Option<&Guard>, bn: Option<&Guard>| {
            (inner_a.borrow().api.borrow().dispatch)(spec, bd, bn)
        });
        let inner_r = inner.clone();
        let raw_receipt_of = Rc::new(move |response: &V, meta: &V| raw_of(&inner_r, response, meta));
        let inner_c = inner.clone();
        let can_operate = Rc::new(move || allowed(&inner_c));
        let get_account_lease = inner.borrow().deps.get_account_lease.clone();
        let budget_for_install = budget_cell.clone();
        let install = Rc::new(move |r: &V| -> Result<V, String> {
            budget_for_install
                .borrow()
                .as_ref()
                .ok_or("BAAL_SCALED_HOST_BUDGET_UNAVAILABLE".to_string())?
                .install_funding_reservation(r)
        });
        let budget_for_risk = budget_cell.clone();
        let get_global_risk = Rc::new(move || -> Result<V, String> {
            budget_for_risk
                .borrow()
                .as_ref()
                .ok_or("BAAL_SCALED_HOST_BUDGET_UNAVAILABLE".to_string())?
                .global_risk()
        });
        let bridge_for_evidence = bridge_cell.clone();
        let get_evidence = Rc::new(move |context: &V| -> Result<V, String> {
            bridge_for_evidence
                .borrow()
                .as_ref()
                .ok_or("BAAL_SCALED_HOST_BRIDGE_UNAVAILABLE".to_string())?
                .get_evidence(context)
        });
        let now = inner.borrow().deps.now.clone();
        let controller = open_baal_scaled_no_controller(
            &controller_dir,
            &scope,
            &allocation,
            &activation,
            BaalScaledNoControllerDeps {
                dispatch,
                raw_receipt_of,
                can_operate,
                assert_account_lease: get_account_lease.clone(),
                install_funding_reservation: install,
                get_global_risk,
                get_evidence,
                now: now.clone(),
            },
        )?;
        let inner_o = inner.clone();
        let observe = Rc::new(move || -> Result<V, String> { observation(&inner_o) });
        let inner_c2 = inner.clone();
        let can_operate2 = Rc::new(move || allowed(&inner_c2));
        let controller_for_source = controller.clone();
        let get_controller_source = Rc::new(move || controller_for_source.journal_source());
        let budget = Rc::new(open_baal_scaled_shared_budget(
            &budget_dir,
            &controller_dir,
            &allocation,
            &scope,
            &budget_installed_hash_str(&inner),
            get(&activation, "codeIdentityHash").as_str().unwrap_or(""),
            BaalScaledSharedBudgetDeps {
                assert_account_lease: get_account_lease,
                get_controller_source,
                observe_global_risk: observe,
                can_operate: can_operate2,
                now,
            },
        )?);
        *controller_cell.borrow_mut() = Some(controller.clone());
        *budget_cell.borrow_mut() = Some(budget.clone());
        let mut g = inner.borrow_mut();
        g.controller = Some(controller);
        g.budget = Some(budget);
        Ok(())
    })();
    if let Err(e) = init {
        let g = inner.borrow();
        if let Some(b) = g.bridge.clone() {
            let _ = b.close();
        }
        if let Some(b) = g.budget.clone() {
            b.close();
        }
        if let Some(c) = g.controller.clone() {
            let _ = c.close();
        }
        return Err(e);
    }
    Ok(Some(BaalScaledLiveHostV2 { inner }))
}

fn budget_installed_hash_str(inner: &Rc<RefCell<HostInner>>) -> String {
    inner.borrow().budget_installed_hash.clone()
}

fn create_bridge(inner: &Rc<RefCell<HostInner>>) -> Result<(), String> {
    if inner.borrow().bridge.is_some() {
        return Ok(());
    }
    let (scope, allocation, members, universe) = {
        let g = inner.borrow();
        (
            g.scope.clone(),
            g.allocation.clone(),
            g.members.clone(),
            g.universe.clone(),
        )
    };
    let mut uni = universe.clone();
    if let V::Object(m) = &mut uni {
        m.insert("exchanges".into(), V::Array(members));
    }
    let api_dispatch = {
        let api = inner.borrow().api.clone();
        Rc::new(move |spec: &V, bd: Option<&Guard>, bn: Option<&Guard>| -> Result<V, String> {
            (api.borrow().dispatch)(spec, bd, bn).map_err(|e| e.message)
        })
    };
    let inner_r = inner.clone();
    let receipt_of = Rc::new(move |response: &V, meta: &V| raw_of(&inner_r, response, meta));
    let get_account_lease = inner.borrow().deps.get_account_lease.clone();
    let budget_for_risk = inner.borrow().budget_cell.clone();
    let get_global_risk = Rc::new(move || -> Result<V, String> {
        budget_for_risk
            .borrow()
            .as_ref()
            .ok_or("BAAL_SCALED_HOST_BUDGET_UNAVAILABLE".to_string())?
            .global_risk()
    });
    let budget_for_risk2 = inner.borrow().budget_cell.clone();
    let get_account_risk = Rc::new(move || -> Result<V, String> {
        budget_for_risk2
            .borrow()
            .as_ref()
            .ok_or("BAAL_SCALED_HOST_BUDGET_UNAVAILABLE".to_string())?
            .global_risk()
    });
    let controller_for_census = inner.borrow().controller_cell.clone();
    let get_owned_census = Rc::new(move |ex: &str| -> Option<V> {
        controller_for_census
            .borrow()
            .as_ref()
            .map(|c| c.inventory(ex))
            .filter(|v| !v.is_null())
    });
    let controller_for_budget = inner.borrow().controller_cell.clone();
    let get_installed_budget = Rc::new(move || -> Option<V> {
        controller_for_budget
            .borrow()
            .as_ref()
            .and_then(|c| c.funding_reservation_status().ok())
            .filter(|v| !v.is_null())
    });
    let controller_for_owns = inner.borrow().controller_cell.clone();
    let owns_order = Rc::new(move |order_id: &str, leg: &V| -> Result<bool, String> {
        let c = controller_for_owns.borrow();
        let i = c
            .as_ref()
            .map(|c| get(&c.status(), "intent").clone())
            .unwrap_or(V::Null);
        Ok(get(&i, "orderId").as_str() == Some(order_id)
            && get(&i, "marketId") == get(leg, "marketId")
            && get(&i, "exchangeId") == get(leg, "exchangeId"))
    });
    let controller_for_binding = inner.borrow().controller_cell.clone();
    let get_order_binding = Rc::new(move |order_id: &str| -> Result<V, String> {
        Ok(controller_for_binding
            .borrow()
            .as_ref()
            .map(|c| c.order_binding(order_id))
            .unwrap_or(V::Null))
    });
    let inner_c = inner.clone();
    let can_read = Rc::new(move || allowed(&inner_c));
    let has_read_headroom = inner.borrow().deps.has_read_headroom.clone();
    let now = inner.borrow().deps.now.clone();
    let bridge = Rc::new(create_baal_scaled_no_evidence_bridge(
        &scope,
        &allocation,
        &uni,
        BaalScaledNoEvidenceBridgeDeps {
            api_dispatch,
            receipt_of,
            get_account_lease,
            get_global_risk,
            get_account_risk,
            get_owned_census,
            get_installed_budget,
            owns_order,
            get_order_binding,
            can_read,
            has_read_headroom,
            now,
        },
    )?);
    *inner.borrow().bridge_cell.borrow_mut() = Some(bridge.clone());
    inner.borrow_mut().bridge = Some(bridge);
    Ok(())
}

fn bootstrap_cash(inner: &Rc<RefCell<HostInner>>) -> Result<(), String> {
    guard(inner)?;
    need((inner.borrow().deps.has_read_headroom)(1), "BOOTSTRAP_HEADROOM")?;
    let slug = inner
        .borrow()
        .scope
        .get("tournamentSlug")
        .and_then(V::as_str)
        .unwrap_or("")
        .to_string();
    let endpoint = format!("/tournaments/{slug}");
    let inner_d = inner.clone();
    let before_dispatch: &Guard<'_> = &move |_p: &V, _s: &V| {
        guard(&inner_d)
            .map(|_| true)
            .map_err(|e| crate::transport::OrderTransportError {
                code: e,
                status: None,
                api_code: None,
                outcome: "blocked".into(),
                may_have_executed: false,
                retry_same_request_only: false,
                retry_after_ms: 0.0,
                semantic_mismatch: false,
            })
    };
    let inner_n = inner.clone();
    let before_network: &Guard<'_> = &move |_p: &V, _s: &V| {
        guard(&inner_n)
            .map(|_| true)
            .map_err(|e| crate::transport::OrderTransportError {
                code: e,
                status: None,
                api_code: None,
                outcome: "blocked".into(),
                may_have_executed: false,
                retry_same_request_only: false,
                retry_after_ms: 0.0,
                semantic_mismatch: false,
            })
    };
    let bd_ptr = before_dispatch as *const Guard as *const () as usize;
    let bn_ptr = before_network as *const Guard as *const () as usize;
    inner.borrow_mut().issued.insert(
        bd_ptr,
        json!({
            "endpoint": endpoint,
            "beforeNetworkPtr": bn_ptr as u64,
        }),
    );
    let result = (|| -> Result<(), String> {
        let spec = json!({"method": "GET", "endpoint": endpoint});
        let dispatch = inner.borrow().api.borrow().dispatch.clone();
        let response = dispatch(&spec, Some(before_dispatch), Some(before_network))
            .map_err(|e| e.message)?;
        need(get(&response, "status") == &json!(200), "BOOTSTRAP_CASH_HTTP")?;
        raw_of(inner, &response, &json!({"method": "GET"}))?;
        observation(inner)?;
        Ok(())
    })();
    inner.borrow_mut().issued.remove(&bd_ptr);
    result
}


// exitHint(exchangeId) — curated read decision; never an executable mark.
fn exit_hint(inner: &Rc<RefCell<HostInner>>, exchange_id: &str) -> V {
    let (controller, get_hints, universe, scope, now) = {
        let g = inner.borrow();
        (
            g.controller.clone(),
            g.deps.get_pair_read_hints.clone(),
            g.universe.clone(),
            g.scope.clone(),
            g.deps.now.clone(),
        )
    };
    let owned = controller
        .as_ref()
        .map(|c| c.inventory(exchange_id))
        .unwrap_or(V::Null);
    let quote = (|| -> V {
        let hints = match get_hints() {
            Some(h) if !h.is_null() => h,
            _ => return V::Null,
        };
        let quotes = match hints
            .get("quoteCaptures")
            .filter(|q| !q.is_null())
            .or_else(|| hints.get("quotes"))
            .and_then(V::as_array)
        {
            Some(a) => a.clone(),
            None => return V::Null,
        };
        if !(hints.get("tournamentId").is_none() || get(&hints, "tournamentId").is_null()
            || get(&hints, "tournamentId").as_str() == Some(T))
            || !(hints.get("universeHash").is_none() || get(&hints, "universeHash").is_null()
                || get(&hints, "universeHash") == get(&universe, "hash"))
        {
            return V::Null;
        }
        let matches: Vec<V> = quotes
            .iter()
            .filter(|q| get(q, "exchangeId").as_str() == Some(exchange_id))
            .cloned()
            .collect();
        if matches.len() != 1
            || get(&matches[0], "marketId") != get(get(&owned, "proof"), "marketId")
        {
            return V::Null;
        }
        let q = matches[0].clone();
        let tournament_id = if q.get("tournamentId").is_some_and(|t| !t.is_null()) {
            get(&q, "tournamentId").clone()
        } else if get(&hints, "tournamentId").as_str() == Some(T)
            && get(&hints, "universeHash") == get(&universe, "hash")
        {
            json!(T)
        } else {
            V::Null
        };
        let mut out = q;
        if let V::Object(m) = &mut out {
            m.insert("tournamentId".into(), tournament_id);
        }
        out
    })();
    let active_intent = controller
        .as_ref()
        .map(|c| get(&c.status(), "intent").clone())
        .unwrap_or(V::Null);
    let hint = owned_no_exit_read_hint(&json!({
        "scope": scope,
        "owned": owned,
        "quote": quote,
        "activeIntent": active_intent,
        "now": iso(now()),
    }));
    let mut last = json!({
        "at": get(&hint, "at"),
        "marketId": get(get(&owned, "proof"), "marketId"),
        "exchangeId": exchange_id,
        "reason": get(&hint, "reason"),
        "skipPreparationReads": get(&hint, "skipPreparationReads") == &json!(true),
        "ownedProofHash": get(get(&owned, "proof"), "verificationHash"),
    });
    if !get(&hint, "hintHash").is_null() {
        if let V::Object(m) = &mut last {
            m.insert("hintHash".into(), get(&hint, "hintHash").clone());
            m.insert("quoteSourceHash".into(), get(&hint, "quoteSourceHash").clone());
            m.insert("quoteReceivedAt".into(), get(&hint, "quoteReceivedAt").clone());
            m.insert("hintAgeFromRequestMs".into(), get(&hint, "hintAgeFromRequestMs").clone());
        }
    }
    if let V::Object(m) = &mut last {
        m.insert("orderEligible".into(), json!(false));
    }
    inner.borrow_mut().last_exit_hint = Some(last);
    hint
}

impl BaalScaledLiveHostV2 {
    /// step() — one serialized advance of the host loop.
    pub fn step(&self) -> Result<V, String> {
        {
            let mut g = self.inner.borrow_mut();
            need(!g.busy, "BUSY")?;
            g.busy = true;
        }
        let inner = self.inner.clone();
        let out = (|| -> Result<V, String> {
            guard(&inner)?;
            inner.borrow_mut().last_failure = None;
            {
                let (cash, now) = {
                    let g = inner.borrow();
                    (g.cash_receipt.clone(), (g.deps.now)())
                };
                let stale = cash
                    .as_ref()
                    .map(|c| {
                        instant_v(get(c, "requestStartedAt"))
                            .map(|t| now - t as f64 > 15000.0)
                            .unwrap_or(true)
                    })
                    .unwrap_or(true);
                if cash.is_none() || stale {
                    bootstrap_cash(&inner)?;
                }
            }
            let claimed = inner
                .borrow()
                .budget
                .as_ref()
                .map(|b| get(&b.status(), "claimed") == &json!(true))
                .unwrap_or(false);
            if !claimed {
                inner
                    .borrow()
                    .controller
                    .as_ref()
                    .ok_or("BAAL_SCALED_HOST_CONTROLLER_UNAVAILABLE".to_string())?
                    .refresh_reservation()?;
                create_bridge(&inner)?;
            } else {
                create_bridge(&inner)?;
            }
            let (controller, bridge, scope, request, now) = {
                let g = inner.borrow();
                (
                    g.controller.clone().unwrap(),
                    g.bridge.clone().unwrap(),
                    g.scope.clone(),
                    g.request.clone(),
                    g.deps.now.clone(),
                )
            };
            let state = controller.status();
            let intent = get(&state, "intent").clone();
            if !intent.is_null() {
                if get(&intent, "orderId").is_null() {
                    let phase = get(&state, "phase").as_str().unwrap_or("");
                    let fetched = js_int(get(&intent, "actualFetchCount")).unwrap_or(0);
                    let expiry = instant_v(get(get(&intent, "body"), "expirationDate"))?;
                    if phase == "prepared" && fetched == 0 && now() as i64 >= expiry {
                        controller.abandon_prepared()?;
                        return publish(&inner, "expired_unfetched_prepared_abandoned");
                    }
                    if phase == "prepared" && fetched == 0 && (now() as i64) < expiry {
                        if get(&intent, "action").as_str() == Some("buy") {
                            let legs = get(&intent, "packageLegs")
                                .as_array()
                                .cloned()
                                .unwrap_or_default();
                            bridge.refresh_buy(&legs)?;
                        } else {
                            let leg = get(&scope, "exchanges")
                                .as_array()
                                .and_then(|a| {
                                    a.iter().find(|m| get(m, "exchangeId") == get(&intent, "exchangeId"))
                                })
                                .cloned()
                                .unwrap_or(V::Null);
                            bridge.refresh_sell(&leg)?;
                        }
                        controller.dispatch_prepared()?;
                        return publish(&inner, "prepared_intent_dispatched_once");
                    }
                    return publish(
                        &inner,
                        if phase == "prepared" && fetched == 0 {
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
                let order_id = get(&intent, "orderId").as_str().unwrap_or("").to_string();
                let attempt = (|| -> Result<V, String> {
                    bridge.refresh_reconciliation(&order_id, &leg)?;
                    let e = bridge.get_evidence(&json!({
                        "operation": "reconcile",
                        "orderId": order_id,
                        "intentId": get(&intent, "intentId"),
                    }))?;
                    controller.reconcile(&e)
                })();
                return match attempt {
                    Ok(_) => publish(&inner, "terminal_order_reconciled"),
                    Err(e) => {
                        if is_fatal(&e) {
                            return Err(e);
                        }
                        let expiry = instant_v(get(get(&intent, "body"), "expirationDate"))?;
                        if now() as i64 >= expiry
                            && get(&intent, "cancelPending") != &json!(true)
                        {
                            bridge.refresh_cancel(&order_id, &leg)?;
                            controller.cancel_expired()?;
                            return publish(&inner, "known_expired_order_cancel_requested");
                        }
                        inner.borrow_mut().last_failure = Some(if is_all_caps_code(&e) {
                            e.clone()
                        } else {
                            "TERMINAL_SOURCE_PENDING".to_string()
                        });
                        publish(&inner, "known_order_waiting_terminal_proof")
                    }
                };
            }
            if let V::Object(inv) = get(&state, "inventory") {
                for (ex, position) in inv.iter() {
                    if js_int(get(position, "quantity")).unwrap_or(0) <= 0 {
                        continue;
                    }
                    let last = inner.borrow().sell_checks.get(ex).copied().unwrap_or(0.0);
                    if now() - last < 15000.0 {
                        continue;
                    }
                    inner.borrow_mut().sell_checks.insert(ex.clone(), now());
                    let hint = exit_hint(&inner, ex);
                    if get(&hint, "skipPreparationReads") == &json!(true) {
                        inner.borrow_mut().exit_hint_skipped_packets += 1;
                        continue;
                    }
                    let leg = get(&scope, "exchanges")
                        .as_array()
                        .and_then(|a| a.iter().find(|m| get(m, "exchangeId").as_str() == Some(ex)))
                        .cloned()
                        .unwrap_or(V::Null);
                    let attempt = (|| -> Result<V, String> {
                        bridge.refresh_sell(&leg)?;
                        controller.prepare_sell(ex, None)?;
                        controller.dispatch_prepared()
                    })();
                    match attempt {
                        Ok(_) => return publish(&inner, "covered_profitable_sell_dispatched"),
                        Err(e) => {
                            if is_fatal(&e)
                                || get(&controller.status(), "intent").is_object()
                            {
                                return Err(e);
                            }
                            inner.borrow_mut().last_failure = Some(if is_all_caps_code(&e) {
                                e.clone()
                            } else {
                                "NO_FRESH_PROTECTED_EXIT".to_string()
                            });
                        }
                    }
                }
            }
            if now() as i64 >= instant_v(get(&request, "entryExpiresAt"))? {
                return publish(&inner, "entry_window_expired_owned_risk_retained");
            }
            if (now() as i64) < instant_v(get(&request, "startedAt"))? {
                return publish(&inner, "entry_window_not_started");
            }
            for pair in candidates(&inner)? {
                let attempt = (|| -> Result<V, String> {
                    bridge.refresh_buy(&pair)?;
                    let proposal = bridge.get_buy_proposal()?;
                    if get(&proposal, "proposalReady") != &json!(true) {
                        for l in &pair {
                            inner.borrow_mut().cooldowns.insert(
                                get(l, "marketId").as_str().unwrap_or("").to_string(),
                                now() + 300000.0,
                            );
                        }
                        inner.borrow_mut().last_failure =
                            get(&proposal, "reason").as_str().map(|s| s.to_string());
                        return publish(&inner, "candidate_rejected_no_order");
                    }
                    controller.prepare_buy()?;
                    controller.dispatch_prepared()
                })();
                match attempt {
                    Ok(_) => return publish(&inner, "scaled_buy_dispatched"),
                    Err(e) => {
                        if is_fatal(&e)
                            || get(&controller.status(), "intent").is_object()
                        {
                            return Err(e);
                        }
                        backoff(&inner, &e);
                        for l in &pair {
                            inner.borrow_mut().cooldowns.insert(
                                get(l, "marketId").as_str().unwrap_or("").to_string(),
                                now() + 300000.0,
                            );
                        }
                        inner.borrow_mut().last_failure = Some(if is_all_caps_code(&e) {
                            e.clone()
                        } else {
                            "CANDIDATE_FAILED".to_string()
                        });
                        return publish(&inner, "candidate_rejected_no_order");
                    }
                }
            }
            publish(&inner, "no_fresh_qualified_relative_value_candidate")
        })();
        let result = match out {
            Ok(v) => Ok(v),
            Err(e) => {
                backoff(&inner, &e);
                inner.borrow_mut().last_failure = Some(if is_all_caps_code(&e) {
                    e.clone()
                } else {
                    format!("{PREFIX}STEP_FAILED")
                });
                if is_fatal(&e) {
                    inner.borrow_mut().busy = false;
                    return Err(e);
                }
                publish(&inner, "blocked_existing_records_retained")
            }
        };
        inner.borrow_mut().busy = false;
        result
    }

    /// status()
    pub fn status(&self) -> V {
        status(&self.inner)
    }

    /// authorizes(spec) — bootstrap permit, bridge-issued, or controller-issued.
    pub fn authorizes(
        &self,
        spec: &V,
        before_dispatch: Option<&Guard<'_>>,
        before_network: Option<&Guard<'_>>,
    ) -> bool {
        (|| -> Result<bool, String> {
            guard(&self.inner)?;
            let bd_ptr = before_dispatch
                .map(|g| g as *const Guard as *const () as usize)
                .unwrap_or(0);
            let bn_ptr = before_network
                .map(|g| g as *const Guard as *const () as usize)
                .unwrap_or(0);
            let g = self.inner.borrow();
            let p = g.issued.get(&bd_ptr).cloned();
            if p.is_some()
                && get(spec, "method").as_str() == Some("GET")
                && get(spec, "endpoint") == get(p.as_ref().unwrap(), "endpoint")
                && get(p.as_ref().unwrap(), "beforeNetworkPtr") == &json!(bn_ptr as u64)
                && spec.get("body").is_none()
                && spec.get("bodyJson").is_none()
            {
                return Ok(true);
            }
            if let Some(b) = &g.bridge {
                if b.authorizes(spec, before_dispatch, before_network) {
                    return Ok(true);
                }
            }
            if let Some(c) = &g.controller {
                if c.authorizes(spec, before_dispatch, before_network) {
                    return Ok(true);
                }
            }
            Ok(false)
        })()
        .unwrap_or(false)
    }

    /// onFetchStart(event) — passthrough to the controller.
    pub fn on_fetch_start(&self, event: &V) -> Result<bool, String> {
        self.inner
            .borrow()
            .controller
            .as_ref()
            .ok_or("BAAL_SCALED_HOST_CONTROLLER_UNAVAILABLE".to_string())?
            .on_fetch_start(event)
    }

    pub fn blocks_legacy_writes(&self) -> bool {
        true
    }

    /// allowsLegacy(spec) — GET only.
    pub fn allows_legacy(&self, spec: &V) -> bool {
        get(spec, "method").as_str() == Some("GET")
    }

    /// getReadHint — the caller's hint supplier.
    pub fn get_read_hint(&self) -> Option<V> {
        (self.inner.borrow().deps.get_read_hint)()
    }

    /// close()
    pub fn close(&self) -> Result<(), String> {
        {
            let g = self.inner.borrow();
            need(!g.busy, "BUSY")?;
            if g.closed {
                return Ok(());
            }
        }
        self.inner.borrow_mut().closed = true;
        let mut error: Option<String> = None;
        let (bridge, budget, controller) = {
            let g = self.inner.borrow();
            (g.bridge.clone(), g.budget.clone(), g.controller.clone())
        };
        if let Some(b) = bridge {
            if let Err(e) = b.close() {
                if error.is_none() {
                    error = Some(e);
                }
            }
        }
        if let Some(b) = budget {
            b.close();
        }
        if let Some(c) = controller {
            if let Err(e) = c.close() {
                if error.is_none() {
                    error = Some(e);
                }
            }
        }
        match error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}
