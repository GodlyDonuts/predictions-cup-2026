// Port of execution/baal-combined-custody-host-v7.mjs — byte-for-byte behavioral parity.
// Read-path-only upgrade: same V6 descriptor, activation, epoch, coordinator and
// budget journals. This revision cannot seed a new wallet, renew a window or
// revive an abandoned intent. A separately authorized financial-owner epoch:
// original financial journals are sealed read-only sources; their lease files
// fence every earlier host out.
//
// Note: the JS `api.dispatch` callable is shared between the ReadClient surface,
// the bridges (String errors) and the pilot (WireDispatchError). The pilot's
// `deps.dispatch` here maps the String error channel to
// WireDispatchError{message, wire_receipt:None} — a wireReceipt carried on a JS
// thrown dispatch error is not preserved through the String channel.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Instant;

use serde_json::{json, Map, Value as V};

use baal_collector::frozen_paper_graph::collect_frozen_paper_files;
use baal_execution::core::{payload_hash, validate_execution_state};
use baal_execution::paired_rest_controller::WireDispatchError;
use crate::owned_no_fresh_none_hedge_evidence_bridge_v1::Guard;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::fixed::{amount, instant, price};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::sha256;
use baal_service_runtime::ledger::{acquire_owner_lease, OwnerLease};

use crate::baal_neutral_owned_counterpart_pilot_v2::{
    live_coordinator_identity, open_baal_neutral_owned_counterpart_pilot_v2,
    NeutralOwnedPilotDeps, NeutralOwnedPilotV2,
};
use crate::baal_owned_hedge_shared_budget_v2::{
    open_baal_owned_hedge_shared_budget_v2, OwnedHedgeBudgetDeps, OwnedHedgeBudgetV2,
};
use crate::baal_owned_exit_priority_v1::rank_baal_owned_exit_read_hints_v1;
use crate::baal_scaled_controller_legacy_replay_v1::{
    baal_scaled_no_controller_identity, replay_baal_scaled_controller_v1,
};
use crate::baal_combined_custody_host_v6::baal_combined_custody_v6_identity;
use crate::baal_scaled_live_host_v1::baal_scaled_live_identity;
use crate::no_rest_reconciliation_v2::rounded_tournament_cash_interval;
use crate::owned_no_fresh_none_hedge_evidence_bridge_v3::{
    create_owned_no_fresh_none_hedge_evidence_bridge_v3, OwnedNoFreshNoneHedgeEvidenceBridgeV3,
    OwnedNoneBridgeV3Deps,
};
use crate::owned_no_fresh_none_hedge_evidence_bridge_v1::OwnedNoneBridgeApi;

pub const T: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";

fn copy(v: &V) -> V {
    v.clone()
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.as_object().and_then(|m| m.get(k)).unwrap_or(&V::Null)
}
fn sha(b: &[u8]) -> String {
    sha256(b)
}
fn hex(v: &V) -> bool {
    v.as_str().map_or(false, |s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(payload_hash(a)? == payload_hash(b)?)
}
fn iso(n: f64) -> String {
    baal_service_runtime::contract::ms_to_iso(n)
}
fn truthy(v: &V) -> bool {
    match v {
        V::Null => false,
        V::Bool(b) => *b,
        V::Number(n) => n.as_f64().map_or(false, |x| x != 0.0 && !x.is_nan()),
        V::String(s) => !s.is_empty(),
        _ => true,
    }
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(
        v.as_str()
            .ok_or("Timestamp must be canonical UTC ISO milliseconds")?,
    )
}
fn num_string(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else {
                baal_service_runtime::json_text::js_stringify(&V::Number(n.clone()))
            }
        }
        _ => "undefined".to_string(),
    }
}

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("BAAL_CUSTODY_V6_{code}"))
    }
}

fn sync(f: impl FnOnce() -> Result<V, String>) -> Result<V, String> {
    Ok(copy(&f()?))
}

fn lock_summary(r: &V) -> Result<V, String> {
    let mut out = Map::new();
    if let V::Object(m) = r {
        for (k, v) in m {
            if k != "token" {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    out.insert("identityHash".into(), json!(payload_hash(r)?));
    Ok(V::Object(out))
}

fn code_of(e: &str) -> &str {
    e.split('\u{0}').next().unwrap_or(e)
}
fn fatal(e: &str) -> String {
    format!("{e}\u{0}fatal\u{0}durableRecordingFailed")
}
fn is_fatal(e: &str) -> bool {
    e.contains("\u{0}fatal\u{0}") || e.contains("durableRecordingFailed")
}

/// JS safeCode: first of [e.code, e.message] matching /^[A-Z][A-Z0-9_]{1,120}$/,
/// else 'BAAL_CUSTODY_V6_OPERATION_FAILED'. The String channel carries just the
/// thrown code/message, so test its base segment.
pub fn safe_code(e: &str) -> String {
    let c = code_of(e);
    if c.len() >= 2
        && c.len() <= 121
        && c.as_bytes()[0].is_ascii_uppercase()
        && c[1..]
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    {
        c.to_string()
    } else {
        "BAAL_CUSTODY_V6_OPERATION_FAILED".to_string()
    }
}

// The live host identity exports are one-line collectFrozenPaperFiles calls;
// the full host v2/v3/v5 modules are not ported yet, so they are inlined here.
fn baal_scaled_live_v2_identity(workspace: &Path) -> Result<V, String> {
    Ok(json!(collect_frozen_paper_files(
        workspace,
        &[
            "execution/baal-scaled-live-host-v2.mjs".to_string(),
            "execution/scoped-rest-receipts-v2.mjs".to_string(),
        ],
    )?))
}
fn baal_scaled_live_v3_identity(workspace: &Path) -> Result<V, String> {
    Ok(json!(collect_frozen_paper_files(
        workspace,
        &[
            "execution/baal-scaled-live-host-v3.mjs".to_string(),
            "execution/scoped-rest-receipts-v2.mjs".to_string(),
        ],
    )?))
}
fn baal_scaled_live_v5_identity(workspace: &Path) -> Result<V, String> {
    Ok(json!(collect_frozen_paper_files(
        workspace,
        &[
            "execution/baal-scaled-live-host-v5.mjs".to_string(),
            "execution/scoped-rest-receipts-v2.mjs".to_string(),
        ],
    )?))
}

fn non_none(e: &str) -> bool {
    matches!(
        code_of(e),
        "COMBINED_NONE_EVIDENCE_NON_NONE_GRAPH"
            | "COMBINED_NONE_EVIDENCE_NON_NONE_COLLATERAL"
            | "NONE_HEDGE_EVIDENCE_NON_NONE_OR_UNSUPPORTED_GRAPH"
            | "NONE_HEDGE_EVIDENCE_NON_NONE_COLLATERAL"
    )
}

// JS e.retryAfterMs crossing the String error channel is carried as a
// "\u{0}retryAfterMs:<float>" suffix appended at OrderTransportError wrapping.
fn retry_after_ms(e: &str) -> Option<f64> {
    e.split("\u{0}retryAfterMs:")
        .nth(1)?
        .split('\u{0}')
        .next()?
        .parse()
        .ok()
}

pub fn baal_combined_custody_v7_identity(workspace: &Path) -> Result<V, String> {
    Ok(json!(collect_frozen_paper_files(
        workspace,
        &[
            "execution/baal-combined-custody-host-v7.mjs".to_string(),
            "execution/scoped-rest-receipts-v5.mjs".to_string(),
        ],
    )?))
}

trait PipeGet {
    fn pipe_get<'a>(&'a self, k: &str) -> &'a V;
}
impl PipeGet for &V {
    fn pipe_get<'a>(&'a self, k: &str) -> &'a V {
        get(self, k)
    }
}

/// The JS `api` object surface the custody host reads/writes. `api` is the
/// shared {dispatch, manualReview, cooldown} object also handed to the evidence
/// bridges; `api_next` and `on_admission` are the additional ReadClient fields
/// the JS host mutates/calls on the same object.
pub struct CustodyHostDeps {
    pub api: Rc<RefCell<OwnedNoneBridgeApi>>,
    /// JS `api.next` — the next permitted admission instant (f64 ms).
    pub api_next: Rc<Cell<f64>>,
    /// JS `api.onAdmission(valueOrNull)` — V::Null for manual-review clears.
    pub on_admission: Rc<dyn Fn(&V)>,
    /// receiptOf(response, meta) — raw evidence receipt producer.
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub get_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub get_core_owner_record: Rc<dyn Fn() -> Result<V, String>>,
    pub get_other_state: Rc<dyn Fn() -> Result<V, String>>,
    pub get_legacy_risk: Rc<dyn Fn() -> Result<V, String>>,
    /// getPairReadHints=()=>null — return V::Null when there is no hint feed.
    pub get_pair_read_hints: Rc<dyn Fn() -> Result<V, String>>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    /// hasReadHeadroom(count)
    pub has_read_headroom: Rc<dyn Fn(i64) -> Result<bool, String>>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct HostLive {
    coordinator: Option<Rc<NeutralOwnedPilotV2>>,
    budget: Option<Rc<OwnedHedgeBudgetV2>>,
    bridges: HashMap<String, Rc<OwnedNoFreshNoneHedgeEvidenceBridgeV3>>,
    selected_bridge: Option<Rc<OwnedNoFreshNoneHedgeEvidenceBridgeV3>>,
}

struct HostInner {
    me: RefCell<Weak<HostInner>>,
    runtime_dir: PathBuf,
    workspace: PathBuf,
    universe: V,
    deps: CustodyHostDeps,
    descriptor_file: PathBuf,
    upgrade_file: PathBuf,
    upgrade_bytes_hash: String,
    descriptor_bytes_hash: String,
    d: V,
    descriptor_hash: String,
    code: V,
    old_code: V,
    files: Vec<String>,
    source_hashes: Vec<(String, String)>,
    request: V,
    v5: V,
    scope: V,
    allocation: V,
    legacy_controller_dir: PathBuf,
    legacy_budget_dir: PathBuf,
    legacy_file: PathBuf,
    dir: PathBuf,
    coordinator_dir: PathBuf,
    budget_dir: PathBuf,
    lease_dir: PathBuf,
    lease_journal_file: PathBuf,
    stop_file: PathBuf,
    activation: V,
    migration: V,
    epoch: String,
    seed: V,
    live: RefCell<HostLive>,
    financial_lease: RefCell<Option<OwnerLease>>,
    legacy_controller_fence: RefCell<Option<OwnerLease>>,
    legacy_budget_fence: RefCell<Option<OwnerLease>>,
    epoch_journal: RefCell<Option<Journal>>,
    epoch_receipt: RefCell<V>,
    raw_account: RefCell<V>,
    cash_receipt: RefCell<V>,
    closed: Cell<bool>,
    failed: Cell<bool>,
    busy: Cell<bool>,
    last_failure: RefCell<Option<String>>,
    last_action: RefCell<String>,
    last_step_at: RefCell<Option<String>>,
    exit_skipped: Cell<i64>,
    last_hint: RefCell<V>,
    sell_cursor: Cell<usize>,
    hedge_tried_at: Cell<Option<f64>>,
    last_graph_at: RefCell<Option<V>>,
    last_book_at: RefCell<Option<V>>,
    packet_ready_at: Cell<Option<Instant>>,
    last_dispatch_timing: RefCell<V>,
    last_rejected_timing: RefCell<V>,
    last_exit_priority: RefCell<V>,
    permits: RefCell<HashMap<usize, V>>,
    timings: RefCell<VecDeque<V>>,
    stage_timings: RefCell<VecDeque<V>>,
}

impl HostInner {
    fn rc(&self) -> Rc<HostInner> {
        self.me
            .borrow()
            .upgrade()
            .expect("HostInner dropped while self-link used")
    }
    fn now(&self) -> f64 {
        (self.deps.now)()
    }

    /// stage=(name,fn): record {name,startedAt,finishedAt,elapsedClockMs,
    /// elapsedLocalMs,...errorCode} capped at 18.
    fn stage(
        &self,
        name: &str,
        f: impl FnOnce(&Self) -> Result<V, String>,
    ) -> Result<V, String> {
        let start = Instant::now();
        let at = self.now();
        let mut error_code: Option<String> = None;
        let out = f(self).map_err(|e| {
            error_code = Some(safe_code(&e));
            e
        });
        let mut row = Map::new();
        row.insert("name".into(), json!(name));
        row.insert("startedAt".into(), json!(iso(at)));
        row.insert("finishedAt".into(), json!(iso(self.now())));
        row.insert("elapsedClockMs".into(), json!(self.now() - at));
        row.insert(
            "elapsedLocalMs".into(),
            json!((start.elapsed().as_secs_f64() * 1e3 * 1000.0).round() / 1000.0),
        );
        if let Some(c) = error_code {
            row.insert("errorCode".into(), json!(c));
        }
        let mut t = self.stage_timings.borrow_mut();
        t.push_back(V::Object(row));
        while t.len() > 18 {
            t.pop_front();
        }
        drop(t);
        out
    }

    /// timed=(name,fn): record {name,durationMs} capped at 24.
    fn timed(
        &self,
        name: &str,
        f: impl FnOnce(&Self) -> Result<V, String>,
    ) -> Result<V, String> {
        let start = Instant::now();
        let out = f(self);
        let mut t = self.timings.borrow_mut();
        t.push_back(json!({
            "name": name,
            "durationMs": (start.elapsed().as_secs_f64() * 1e3 * 1000.0).round() / 1000.0,
        }));
        while t.len() > 24 {
            t.pop_front();
        }
        drop(t);
        out
    }

    /// Validated immutable sources are still fully read and SHA-checked on
    /// every guard.
    fn immutable(&self) -> Result<(), String> {
        need(
            !self.closed.get()
                && !self.failed.get()
                && (self.deps.can_operate)()
                && !self.stop_file.exists(),
            "STOPPED",
        )?;
        need(
            sha(&fs::read(&self.upgrade_file).map_err(|e| e.to_string())?)
                == self.upgrade_bytes_hash
                && sha(&fs::read(&self.descriptor_file).map_err(|e| e.to_string())?)
                == self.descriptor_bytes_hash,
            "DESCRIPTOR_OR_CODE_CHANGED",
        )?;
        if let V::Object(m) = &self.code {
            for (file, hash) in m {
                need(
                    sha(&fs::read(self.workspace.join(file)).map_err(|e| e.to_string())?)
                        == hash.as_str().unwrap_or(""),
                    "DESCRIPTOR_OR_CODE_CHANGED",
                )?;
            }
        }
        for (f, hash) in &self.source_hashes {
            need(
                sha(&fs::read(self.runtime_dir.join(f)).map_err(|e| e.to_string())?)
                    == *hash,
                "ORIGINAL_SOURCE_CHANGED",
            )?;
        }
        Ok(())
    }

    fn actual_account(&self) -> Result<V, String> {
        let a = sync(|| (self.deps.get_account_lease)())?;
        let keys_ok = matches!(&a, V::Object(m) if {
            let mut ks: Vec<&str> = m.keys().map(|k| k.as_str()).collect();
            ks.sort();
            ks.join("|") == "leaseId|ownerId|tournamentId"
        });
        need(
            keys_ok
                && get(&a, "ownerId").as_str() == get(&self.scope, "ownerId").as_str()
                && get(&a, "tournamentId").as_str() == Some(T)
                && hex(get(&a, "leaseId")),
            "RAW_ACCOUNT_LEASE",
        )?;
        let r = sync(|| (self.deps.get_core_owner_record)())?;
        need(
            get(&r, "ownerId").as_str() == get(&self.scope, "ownerId").as_str()
                && get(&r, "scope").as_str() == Some(T)
                && get(&r, "pid").as_i64() == Some(std::process::id() as i64)
                && payload_hash(&r)? == get(&a, "leaseId").as_str().unwrap_or(""),
            "CURRENT_CORE_PID_OWNER",
        )?;
        {
            let prev = self.raw_account.borrow();
            if !prev.is_null() {
                need(same(&a, &prev)?, "ACCOUNT_OWNER_CHANGED")?;
            }
        }
        Ok(a)
    }

    fn assert_leases(&self) -> Result<(), String> {
        if let Some(l) = self.financial_lease.borrow().as_ref() {
            l.assert_ownership().map_err(|e| e.message.clone())?;
        }
        if let Some(l) = self.legacy_controller_fence.borrow().as_ref() {
            l.assert_ownership().map_err(|e| e.message.clone())?;
        }
        if let Some(l) = self.legacy_budget_fence.borrow().as_ref() {
            l.assert_ownership().map_err(|e| e.message.clone())?;
        }
        Ok(())
    }

    fn lease_view(&self) -> Result<V, String> {
        Ok(json!({
            "ownerId": get(&self.scope, "ownerId"),
            "runId": get(&self.scope, "runId"),
            "tournamentId": T,
            "leaseId": payload_hash(&self.epoch_receipt.borrow().clone())?,
        }))
    }

    fn leases(&self) -> Result<V, String> {
        self.immutable()?;
        self.assert_leases()?;
        let a = self.actual_account()?;
        need(
            !self.epoch_receipt.borrow().is_null()
                && same(get(&self.epoch_receipt.borrow(), "rawAccountLease"), &a)?,
            "DURABLE_OWNER_BINDING",
        )?;
        self.lease_view()
    }

    fn mechanical_leases(&self) -> Result<V, String> {
        need(
            !self.closed.get()
                && !self.failed.get()
                && (self.deps.can_operate)()
                && !self.stop_file.exists()
                && !self.deps.api.borrow().manual_review
                && self.now() >= self.api_cooldown(),
            "STOPPED",
        )?;
        self.assert_leases()?;
        let a = self.actual_account()?;
        need(
            !self.epoch_receipt.borrow().is_null()
                && same(get(&self.epoch_receipt.borrow(), "rawAccountLease"), &a)?,
            "DURABLE_OWNER_BINDING",
        )?;
        self.lease_view()
    }

    fn api_cooldown(&self) -> f64 {
        let c = self.deps.api.borrow().cooldown;
        if c.is_nan() {
            0.0
        } else {
            c
        }
    }

    fn financial(&self) -> Result<V, String> {
        let mut l = self.leases()?;
        if let V::Object(ref mut m) = l {
            m.insert("implementationEpoch".into(), json!(self.epoch));
        }
        Ok(l)
    }

    fn legacy(&self) -> Result<V, String> {
        let core = validate_execution_state(&sync(|| (self.deps.get_other_state)())?)
            .map_err(|e| e.to_string())?;
        let retired = sync(|| (self.deps.get_legacy_risk)())?;
        let intents_open = get(&core, "intents")
            .as_object()
            .map_or(false, |m| {
                m.values().any(|i| get(i, "status").as_str() != Some("closed"))
            });
        let owned_nonzero = get(&core, "owned")
            .as_object()
            .map_or(false, |m| {
                m.values()
                    .any(|p| get(p, "quantity").as_f64().map_or(false, |q| q > 0.0))
            });
        need(
            !intents_open
                && !owned_nonzero
                && *get(&retired, "complete") == V::Bool(true)
                && hex(get(&retired, "sourceHash"))
                && get(&retired, "currentOwnedQuantity").as_f64() == Some(0.0)
                && get(&retired, "currentReservedCash").as_str() == Some("0.000000")
                && get(&retired, "unresolvedIntentIds")
                    .as_array()
                    .map_or(false, |a| a.is_empty()),
            "OTHER_WRITER_LIABILITY",
        )?;
        Ok(json!({
            "coreHash": payload_hash(&core)?,
            "retiredHash": get(&retired, "sourceHash"),
        }))
    }

    fn guard(&self) -> Result<(), String> {
        self.leases()?;
        self.legacy()?;
        need(
            !self.deps.api.borrow().manual_review && self.now() >= self.api_cooldown(),
            "SHARED_BACKOFF",
        )?;
        Ok(())
    }

    fn observation(&self) -> Result<V, String> {
        self.guard()?;
        let cr = self.cash_receipt.borrow().clone();
        need(!cr.is_null(), "NO_ACTUAL_CASH")?;
        let c = rounded_tournament_cash_interval(
            &cr,
            &json!({
                "tournamentId": T,
                "tournamentSlug": get(&self.scope, "tournamentSlug"),
                "readAt": iso(self.now()),
            }),
        )?;
        let mut out = Map::new();
        if let V::Object(m) = &self.scope {
            for (k, v) in m {
                out.insert(k.clone(), v.clone());
            }
        }
        out.insert("at".into(), json!(iso(self.now())));
        out.insert(
            "sourceHash".into(),
            json!(payload_hash(&json!({
                "cash": get(&c, "sourceBodyHash"),
                "lease": payload_hash(&self.epoch_receipt.borrow().clone())?,
                "other": self.legacy()?,
            }))?),
        );
        out.insert("complete".into(), V::Bool(true));
        out.insert("eventScopeComplete".into(), V::Bool(true));
        out.insert("soleCollector".into(), V::Bool(true));
        out.insert("stopped".into(), V::Bool(false));
        out.insert("unknownAcceptance".into(), V::Bool(false));
        out.insert("unresolvedIntentIds".into(), json!([]));
        out.insert("cashLower".into(), get(&c, "lower").clone());
        out.insert("owned".into(), json!([]));
        out.insert("reservations".into(), json!([]));
        out.insert("loans".into(), json!([]));
        Ok(V::Object(out))
    }

    fn raw_of(&self, response: &V, meta: &V) -> Result<V, String> {
        let r = sync(|| (self.deps.receipt_of)(response, meta))?;
        need(
            get(&r, "method").as_str() == get(meta, "method").as_str()
                && get(&r, "endpoint").as_str() == get(response, "endpoint").as_str(),
            "RAW_CALLBACK",
        )?;
        let tourney_endpoint = format!(
            "/tournaments/{}",
            get(&self.scope, "tournamentSlug").as_str().unwrap_or("")
        );
        if get(meta, "method").as_str() == Some("GET")
            && get(&r, "endpoint").as_str() == Some(tourney_endpoint.as_str())
        {
            *self.cash_receipt.borrow_mut() = copy(&r);
        }
        if get(meta, "method").as_str() == Some("GET")
            && get(&r, "endpoint")
                .as_str()
                .map_or(false, |e| e.starts_with("/relationships/graph?"))
        {
            *self.last_graph_at.borrow_mut() = Some(get(&r, "requestStartedAt").clone());
        }
        if get(meta, "method").as_str() == Some("GET")
            && get(&r, "endpoint")
                .as_str()
                .map_or(false, |e| e.contains("/orderbook?"))
        {
            *self.last_book_at.borrow_mut() = Some(get(&r, "requestStartedAt").clone());
        }
        Ok(r)
    }

    fn halt(&self, input: &V) -> Result<V, String> {
        let kind = get(input, "kind").as_str().unwrap_or("");
        let reason = if [
            "non_neutral_or_unknown_hedge",
            "uncertain_hedge",
            "non_neutral_post_account",
        ]
        .contains(&kind)
        {
            kind
        } else {
            "financial_uncertainty"
        };
        let mut out = json!({
            "schemaVersion": 1,
            "source": "baal_combined_custody_v6",
            "reason": reason,
            "ownerId": get(&self.scope, "ownerId"),
            "runId": get(&self.scope, "runId"),
            "tournamentId": T,
            "implementationEpoch": self.epoch,
            "descriptorHash": self.descriptor_hash,
            "at": iso(self.now()),
            "allWritersStop": true,
        });
        if hex(get(input, "sourceHash")) {
            if let V::Object(ref mut m) = out {
                m.insert("sourceHash".into(), get(input, "sourceHash").clone());
            }
        }
        if let Err(e) = (|| -> Result<(), String> {
            if !self.stop_file.exists() {
                atomic_json(&self.stop_file, &out)?;
            }
            need(self.stop_file.exists(), "STOP_NOT_DURABLE")?;
            Ok(())
        })() {
            self.failed.set(true);
            return Err(fatal(&e));
        }
        Ok(out)
    }

    fn backoff(&self, e: &str) -> Result<(), String> {
        let Some(ms) = retry_after_ms(e) else {
            return Ok(());
        };
        let r: Result<(), String> = (|| {
            let end = self.now() + ms;
            let safe_ms = ms.fract() == 0.0 && ms >= 0.0 && ms.abs() <= 9.007199254740991e15;
            let safe_end =
                end.fract() == 0.0 && end.abs() <= 9.007199254740991e15 && end <= 8.64e15;
            if !safe_ms || !safe_end {
                self.deps.api.borrow_mut().manual_review = true;
                (self.deps.on_admission)(&V::Null);
                return Ok(());
            }
            let cd = self.api_cooldown().max(end);
            self.deps.api.borrow_mut().cooldown = cd;
            let nx = self.deps.api_next.get().max(cd);
            self.deps.api_next.set(nx);
            (self.deps.on_admission)(&json!(nx));
            Ok(())
        })();
        match r {
            Ok(()) => Ok(()),
            Err(err) => {
                self.failed.set(true);
                Err(fatal(&format!("BAAL_CUSTODY_V6_BACKOFF_PERSISTENCE\u{0}{err}")))
            }
        }
    }

    fn bootstrap(&self) -> Result<V, String> {
        self.guard()?;
        need(
            (self.deps.has_read_headroom)(1)?,
            "BOOTSTRAP_HEADROOM",
        )?;
        let endpoint = format!(
            "/tournaments/{}",
            get(&self.scope, "tournamentSlug").as_str().unwrap_or("")
        );
        let me = self.rc();
        let before_dispatch: Box<Guard> =
            Box::new(move |_p: &V, _s: &V| me.guard().map(|_| true));
        let me2 = self.rc();
        let before_network: Box<Guard> =
            Box::new(move |_p: &V, _s: &V| me2.guard().map(|_| true));
        let bd_ptr = &*before_dispatch as *const Guard as *const () as usize;
        let bn_ptr = &*before_network as *const Guard as *const () as usize;
        self.permits.borrow_mut().insert(
            bd_ptr,
            json!({"endpoint": endpoint, "beforeNetworkPtr": bn_ptr as u64}),
        );
        let r = {
            let api = self.deps.api.borrow();
            (api.dispatch)(
                &json!({"method": "GET", "endpoint": endpoint}),
                Some(&*before_dispatch),
                Some(&*before_network),
            )
        };
        self.permits.borrow_mut().remove(&bd_ptr);
        let r = r?;
        need(get(&r, "status").as_i64() == Some(200), "CASH_HTTP")?;
        self.raw_of(&r, &json!({"method": "GET"}))?;
        self.observation()?;
        Ok(r)
    }

    fn bridge_for(
        &self,
        exchange_id: &str,
    ) -> Result<Rc<OwnedNoFreshNoneHedgeEvidenceBridgeV3>, String> {
        let coord = self
            .live
            .borrow()
            .coordinator
            .clone()
            .ok_or_else(|| "BAAL_CUSTODY_V6_OPERATION_FAILED".to_string())?;
        let slot = coord.inventory(exchange_id);
        let legs = get(get(&slot, "package"), "legs")
            .as_array()
            .cloned()
            .unwrap_or_default();
        need(legs.len() == 2, "OWNED_PAIR_PACKAGE")?;
        let mut key_parts: Vec<String> = legs
            .iter()
            .map(|l| get(l, "exchangeId").as_str().unwrap_or("").to_string())
            .collect();
        key_parts.sort();
        let key = key_parts.join(":");
        if !self.live.borrow().bridges.contains_key(&key) {
            let first = legs
                .iter()
                .find(|l| get(l, "exchangeId").as_str() == Some(exchange_id))
                .cloned()
                .unwrap_or(V::Null);
            let me = self.rc();
            let api = Rc::clone(&self.deps.api);
            let live = self.rc();
            let live2 = self.rc();
            let live3 = self.rc();
            let live5 = self.rc();
            let live7 = self.rc();
            let live8 = self.rc();
            let live9 = self.rc();
            let refresh_funding_after_account: Rc<dyn Fn() -> Result<(), String>> =
                Rc::new(move || {
                    live3
                        .stage("funding_after_account", |h| {
                            h.live
                                .borrow()
                                .coordinator
                                .as_ref()
                                .map(|c| c.refresh_reservation())
                                .unwrap_or_else(|| Ok(V::Null))
                        })
                        .map(|_| ())
                });
            let bridge = create_owned_no_fresh_none_hedge_evidence_bridge_v3(
                api,
                Rc::new(move |response: &V, meta: &V| me.raw_of(response, meta)),
                &self.scope,
                get(&first, "exchangeId").as_str(),
                Some(&V::Array(legs.clone())),
                OwnedNoneBridgeV3Deps {
                    get_owned: Rc::new(move |ex: &str| {
                        Ok(live
                            .live
                            .borrow()
                            .coordinator
                            .as_ref()
                            .map(|c| c.inventory(ex))
                            .unwrap_or(V::Null))
                    }),
                    get_pair_census: Rc::new(move |ex: &str| {
                        live2
                            .live
                            .borrow()
                            .coordinator
                            .as_ref()
                            .map(|c| c.pair_census(ex))
                            .unwrap_or_else(|| Ok(V::Null))
                    }),
                    get_funding: Rc::new(move || {
                        live9
                            .live
                            .borrow()
                            .coordinator
                            .as_ref()
                            .map(|c| c.funding_reservation_status())
                            .unwrap_or_else(|| Ok(V::Null))
                    }),
                    refresh_funding_after_account: Some(refresh_funding_after_account),
                    get_global_risk: Rc::new(move || {
                        live5.timed("current_budget_risk", |h| {
                            h.live
                                .borrow()
                                .budget
                                .as_ref()
                                .map(|b| b.global_risk())
                                .unwrap_or_else(|| Ok(V::Null))
                        })
                    }),
                    get_account_lease: Rc::clone(&self.deps.get_account_lease),
                    can_read: Rc::new(move || live7.guard().is_ok()),
                    has_read_headroom: Rc::clone(&self.deps.has_read_headroom),
                    can_check_read_identity: {
                        let me_cr = self.rc();
                        Rc::new(move || me_cr.mechanical_leases().is_ok())
                    },
                    get_active_intent: Rc::new(move || {
                        Ok(live8
                            .live
                            .borrow()
                            .coordinator
                            .as_ref()
                            .map(|c| c.active_intent_status())
                            .unwrap_or(V::Null))
                    }),
                    get_order_binding: Rc::new({
                        let me_ob = self.rc();
                        move |id: &str| {
                            Ok(me_ob
                                .live
                                .borrow()
                                .coordinator
                                .as_ref()
                                .map(|c| c.order_binding(&json!(id)))
                                .unwrap_or(V::Null))
                        }
                    }),
                    now: Rc::clone(&self.deps.now),
                },
            )?;
            self.live
                .borrow_mut()
                .bridges
                .insert(key.clone(), Rc::new(bridge));
        }
        let b = self.live.borrow().bridges.get(&key).cloned();
        self.live.borrow_mut().selected_bridge = b.clone();
        b.ok_or_else(|| "BAAL_CUSTODY_V6_OWNED_PAIR_PACKAGE".to_string())
    }

    fn negative_hint(&self, ex: &str) -> bool {
        (|| -> Result<bool, String> {
            let h = sync(|| (self.deps.get_pair_read_hints)())?;
            need(
                get(&h, "tournamentId").as_str() == Some(T)
                    && get(&h, "universeHash").as_str() == get(&self.universe, "hash").as_str(),
                "HINT_SCOPE",
            )?;
            let all = {
                let qc = get(&h, "quoteCaptures");
                if !qc.is_null() {
                    qc.as_array().cloned().unwrap_or_default()
                } else {
                    get(&h, "quotes").as_array().cloned().unwrap_or_default()
                }
            };
            let q: Vec<V> = all
                .into_iter()
                .filter(|q| get(q, "exchangeId").as_str() == Some(ex))
                .collect();
            need(q.len() == 1, "HINT_IDENTITY")?;
            let quote = &q[0];
            let coord = self
                .live
                .borrow()
                .coordinator
                .clone()
                .ok_or_else(|| "BAAL_CUSTODY_V6_OPERATION_FAILED".to_string())?;
            let slot = coord.inventory(ex);
            let lot0 = get(get(&slot, "proof"), "remainingLots")
                .as_array()
                .and_then(|a| a.first())
                .cloned()
                .unwrap_or(V::Null);
            let q_started = instant_v(get(quote, "requestStartedAt"))?;
            let q_recv = instant_v(get(quote, "receivedAt"))?;
            let known_ok = !truthy(get(quote, "knownAt")) || {
                let k = instant_v(get(quote, "knownAt"))?;
                q_recv <= k && (k as f64) <= self.now()
            };
            need(
                get(quote, "marketId").as_str()
                    == get(get(&slot, "proof"), "marketId").as_str()
                    && (get(quote, "tournamentId").is_null()
                        || get(quote, "tournamentId").as_str() == Some(T))
                    && get(quote, "option").as_str() == Some("YES")
                    && *get(quote, "crossed") != V::Bool(true)
                    && hex(get(quote, "sourceHash"))
                    && q_started <= q_recv
                    && (q_recv as f64) <= self.now()
                    && self.now() - (q_started as f64) <= 20000.0
                    && known_ok,
                "HINT_FRESH",
            )?;
            let yes_bid = price(&num_string(get(quote, "bestBid")))?;
            let yes_ask = price(&num_string(get(quote, "bestAsk")))?;
            need(yes_bid <= yes_ask, "HINT_CROSSED")?;
            let bid = 1_000_000i128 - yes_ask;
            let entry = amount(get(&lot0, "entryPrice").as_str().unwrap_or(""))?;
            let hint = json!({
                "marketId": get(quote, "marketId"),
                "exchangeId": ex,
                "sourceHash": get(quote, "sourceHash"),
                "receivedAt": get(quote, "receivedAt"),
                "skipPreparationReads": bid < entry + 5000,
                "orderEligible": false,
            });
            let skip = *get(&hint, "skipPreparationReads") == V::Bool(true);
            *self.last_hint.borrow_mut() = hint;
            Ok(skip)
        })()
        .unwrap_or(false)
    }

    pub fn status(&self) -> V {
        let live = self.live.borrow();
        let controller = live
            .coordinator
            .as_ref()
            .and_then(|c| c.status().ok())
            .unwrap_or(V::Null);
        let budget = live
            .budget
            .as_ref()
            .and_then(|b| b.status().ok())
            .unwrap_or(V::Null);
        let bridge = live
            .selected_bridge
            .as_ref()
            .map(|b| b.status())
            .unwrap_or(V::Null);
        json!({
            "schemaVersion": 1,
            "hostVersion": 7,
            "mode": "baal_combined_custody_host_v7",
            "ownerId": get(&self.scope, "ownerId"),
            "runId": get(&self.scope, "runId"),
            "tournamentId": T,
            "observedAt": iso(self.now()),
            "descriptorHash": self.descriptor_hash,
            "hostCodeIdentityHash": payload_hash(&self.code).unwrap_or_default(),
            "seedHash": get(&self.d, "seedHash"),
            "legacyAnchor": copy(get(&self.seed, "anchor")),
            "originalRequestHash": get(&self.d, "originalRequestHash"),
            "priorV5UpgradeHash": get(&self.d, "priorV5UpgradeHash"),
            "entryExpiresAt": get(&self.request, "entryExpiresAt"),
            "hedgeExpiresAt": get(&self.activation, "hedgeExpiresAt"),
            "exitExpiresAt": get(&self.activation, "exitExpiresAt"),
            "implementationEpoch": self.epoch,
            "lastAction": self.last_action.borrow().clone(),
            "lastFailure": self.last_failure.borrow().clone(),
            "lastStepAt": self.last_step_at.borrow().clone(),
            "busy": self.busy.get(),
            "failed": self.failed.get(),
            "closed": self.closed.get(),
            "readBootstrapComplete": !self.cash_receipt.borrow().is_null(),
            "financialOwnerBound": !self.epoch_receipt.borrow().is_null(),
            "exitHintSkippedPackets": self.exit_skipped.get(),
            "lastExitHint": copy(&self.last_hint.borrow()),
            "lastExitReadPriority": copy(&self.last_exit_priority.borrow()),
            "controller": controller,
            "budget": budget,
            "bridge": bridge,
            "recentLocalTimings": V::Array(self.timings.borrow().iter().cloned().collect()),
            "stageTimings": V::Array(self.stage_timings.borrow().iter().cloned().collect()),
            "lastDispatchTiming": copy(&self.last_dispatch_timing.borrow()),
            "lastRejectedTiming": copy(&self.last_rejected_timing.borrow()),
            "fullRungExitPolicy": true,
            "additionalApiClients": 0,
            "additionalPersistentProcesses": 0,
            "oldWriterHandbackAllowed": false,
        })
    }

    fn publish(&self, action: &str) -> Result<V, String> {
        *self.last_action.borrow_mut() = action.to_string();
        *self.last_step_at.borrow_mut() = Some(iso(self.now()));
        let out = self.status();
        if let Err(e) = atomic_json(&self.runtime_dir.join("baal-scaled-live-stats.json"), &out)
        {
            self.failed.set(true);
            return Err(fatal(&e));
        }
        Ok(out)
    }

    fn coordinator(&self) -> Result<Rc<NeutralOwnedPilotV2>, String> {
        self.live
            .borrow()
            .coordinator
            .clone()
            .ok_or_else(|| "BAAL_CUSTODY_V6_OPERATION_FAILED".to_string())
    }

    fn dispatch_prepared_guarded(&self) -> Result<V, String> {
        self.coordinator()?.dispatch_prepared()
    }

    pub fn step(&self) -> Result<V, String> {
        need(!self.busy.get(), "BUSY")?;
        self.busy.set(true);
        let out = self.step_inner();
        self.busy.set(false);
        out
    }

    fn step_inner(&self) -> Result<V, String> {
        let result = (|| -> Result<V, String> {
            self.guard()?;
            *self.last_failure.borrow_mut() = None;
            let stale = {
                let cr = self.cash_receipt.borrow();
                cr.is_null()
                    || instant(get(&cr, "requestStartedAt").as_str().unwrap_or(""))
                        .map(|a| self.now() - a as f64 > 12000.0)
                        .unwrap_or(true)
            };
            if stale {
                self.stage("cash_bootstrap", |h| h.bootstrap())?;
            }
            if self.live.borrow().budget.is_none() {
                let me = self.rc();
                let me2 = self.rc();
                let me3 = self.rc();
                let live_src = self.rc();
                let budget = open_baal_owned_hedge_shared_budget_v2(
                    &self.budget_dir,
                    &self.coordinator_dir,
                    &self.workspace,
                    &self.legacy_file,
                    &self.old_code,
                    &self.scope,
                    &self.allocation,
                    &self.activation,
                    &self.migration,
                    get(&self.activation, "codeIdentityHash")
                        .as_str()
                        .unwrap_or(""),
                    OwnedHedgeBudgetDeps {
                        assert_account_lease: Rc::new(move || me.leases()),
                        assert_financial_owner_lease: Rc::new(move || me2.financial()),
                        get_coordinator_source: Rc::new(move || {
                            live_src
                                .live
                                .borrow()
                                .coordinator
                                .as_ref()
                                .map(|c| c.journal_source())
                                .unwrap_or_else(|| Ok(V::Null))
                        }),
                        observe_global_risk: Rc::new(move || me3.observation()),
                        can_operate: Rc::new({
                            let me4 = self.rc();
                            move || me4.guard().is_ok()
                        }),
                        now: Rc::clone(&self.deps.now),
                    },
                )?;
                self.live.borrow_mut().budget = Some(Rc::new(budget));
            }
            let b = self
                .live
                .borrow()
                .budget
                .as_ref()
                .map(|x| x.status())
                .transpose()?
                .unwrap_or(V::Null);
            let f = self
                .live
                .borrow()
                .coordinator
                .as_ref()
                .map(|c| c.funding_reservation_status())
                .transpose()?
                .unwrap_or(V::Null);
            if *get(&b, "claimed") != V::Bool(true)
                || get(&b, "controllerReservationHash")
                    != get(get(&f, "record"), "reservationHash")
                || get(get(&f, "receipt"), "globalLedgerHash")
                    != get(&b, "globalLedgerHash")
            {
                let coord = self.coordinator()?;
                self.stage("funding_refresh", move |_| coord.refresh_reservation())?;
            }
            let coord = self.coordinator()?;
            let s = coord.status()?;
            let intent = get(&s, "intent").clone();
            if !intent.is_null() {
                if get(&intent, "orderId").is_null() {
                    if get(&intent, "phase").as_str() == Some("prepared")
                        && *get(&intent, "mayHaveExecuted") == V::Bool(false)
                        && get(&intent, "actualFetchCount").as_f64() == Some(0.0)
                        && instant_v(get(get(&intent, "body"), "expirationDate"))
                            .map(|a| self.now() >= a as f64)
                            .unwrap_or(false)
                    {
                        coord.abandon_prepared()?;
                        return self.publish("expired_never_admitted_intent_abandoned");
                    }
                    if *get(&s, "unknownAcceptance") == V::Bool(true) {
                        self.halt(&json!({"kind": "uncertain_hedge"}))?;
                    }
                    return self.publish("existing_unconfirmed_intent_blocks_new_orders");
                }
                let ex = get(&intent, "firstExchangeId")
                    .as_str()
                    .or_else(|| get(&intent, "exchangeId").as_str())
                    .unwrap_or("")
                    .to_string();
                let bridge = self.bridge_for(&ex)?;
                let order_id = get(&intent, "orderId").as_str().unwrap_or("").to_string();
                let res: Result<V, String> = (|| {
                    self.stage("terminal_packet", |_| {
                        bridge.refresh_reconciliation(&order_id)
                    })?;
                    let ev = bridge.get_evidence(&json!({
                        "operation": "reconcile",
                        "orderId": order_id,
                    }))?;
                    self.stage("terminal_proof", |_| coord.reconcile(&ev))?;
                    Ok(V::Null)
                })();
                match res {
                    Ok(_) => return self.publish("terminal_order_reconciled"),
                    Err(e) => {
                        if is_fatal(&e) {
                            return Err(e);
                        }
                        if non_none(&e) {
                            self.halt(&json!({"kind": "non_neutral_post_account"}))?;
                            *self.last_failure.borrow_mut() = Some(safe_code(&e));
                            return self.publish("non_neutral_post_account_quarantined");
                        }
                        self.backoff(&e)?;
                        *self.last_failure.borrow_mut() = Some(safe_code(&e));
                        return self.publish("known_order_waiting_exact_terminal_proof");
                    }
                }
            }
            let mut preferred_attempt: Option<String> = None;
            *self.last_exit_priority.borrow_mut() = V::Null;
            if self.now()
                < instant_v(get(&self.activation, "exitExpiresAt"))
                    .map(|a| a as f64)
                    .unwrap_or(f64::NAN)
            {
                let hints = sync(|| (self.deps.get_pair_read_hints)()).unwrap_or(V::Null);
                let owned: Vec<V> = get(&s, "inventory")
                    .as_object()
                    .map(|m| {
                        m.keys()
                            .map(|ex| coord.inventory(ex))
                            .filter(|v| !v.is_null())
                            .collect()
                    })
                    .unwrap_or_default();
                let ranked = rank_baal_owned_exit_read_hints_v1(&json!({
                    "scope": self.scope,
                    "universeHash": get(&self.universe, "hash"),
                    "owned": V::Array(owned),
                    "hints": hints,
                    "now": self.now(),
                }));
                let candidate = get(&ranked, "ranked")
                    .as_array()
                    .and_then(|a| a.first())
                    .cloned()
                    .unwrap_or(V::Null);
                if !candidate.is_null() {
                    let cex = get(&candidate, "exchangeId")
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    preferred_attempt = Some(cex.clone());
                    let mut lp = candidate.clone();
                    if let V::Object(ref mut m) = lp {
                        m.insert("depthVerified".into(), V::Bool(false));
                        m.insert("actualProfitClaim".into(), V::Bool(false));
                        m.insert("orderEligible".into(), V::Bool(false));
                    }
                    *self.last_exit_priority.borrow_mut() = lp;
                    let bridge = self.bridge_for(&cex)?;
                    let leg = get(&self.scope, "exchanges")
                        .as_array()
                        .and_then(|a| {
                            a.iter()
                                .find(|l| get(l, "exchangeId").as_str() == Some(cex.as_str()))
                                .cloned()
                        })
                        .unwrap_or(V::Null);
                    let res: Result<V, String> = (|| {
                        self.stage("priority_sell_packet", |_| bridge.refresh_sell(&leg))?;
                        self.stage("prepare_sell", |_| coord.prepare_sell(&cex, None))?;
                        self.stage("final_none", |_| bridge.refresh_final_none())?;
                        self.packet_ready_at.set(Some(Instant::now()));
                        self.stage("dispatch_sell", |_| self.dispatch_prepared_guarded())?;
                        Ok(V::Null)
                    })();
                    match res {
                        Ok(_) => return self.publish("covered_profitable_fifo_sell_dispatched"),
                        Err(e) => {
                            if is_fatal(&e) || !get(&coord.status()?, "intent").is_null() {
                                return Err(e);
                            }
                            if non_none(&e) && code_of(&e).ends_with("COLLATERAL") {
                                self.halt(&json!({"kind": "non_neutral_post_account"}))?;
                                *self.last_failure.borrow_mut() = Some(safe_code(&e));
                                return self
                                    .publish("global_collateral_requires_explicit_model");
                            }
                            self.backoff(&e)?;
                            *self.last_failure.borrow_mut() = Some(safe_code(&e));
                        }
                    }
                }
            }
            if self.now()
                < instant_v(get(&self.activation, "hedgeExpiresAt"))
                    .map(|a| a as f64)
                    .unwrap_or(f64::NAN)
                && self.hedge_tried_at.get().is_none()
                && get(&s, "hedgeIntentCount").as_f64() == Some(0.0)
                && get(
                    get(
                        &coord.inventory(
                            get(&self.d, "firstExchangeId").as_str().unwrap_or(""),
                        ),
                        "proof",
                    ),
                    "remainingOwnedQuantity",
                )
                .as_f64()
                .map_or(false, |q| q > 0.0)
            {
                self.hedge_tried_at.set(Some(self.now()));
                let fex = get(&self.d, "firstExchangeId")
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let bridge = self.bridge_for(&fex)?;
                let res: Result<V, String> = (|| {
                    self.stage("hedge_packet", |_| bridge.refresh())?;
                    self.stage("prepare_hedge", |_| coord.prepare_hedge(&fex))?;
                    self.stage("final_none", |_| bridge.refresh_final_none())?;
                    self.packet_ready_at.set(Some(Instant::now()));
                    self.stage("dispatch_hedge", |_| self.dispatch_prepared_guarded())?;
                    Ok(V::Null)
                })();
                match res {
                    Ok(_) => return self.publish("one_share_owned_counterpart_dispatched"),
                    Err(e) => {
                        if is_fatal(&e) || !get(&coord.status()?, "intent").is_null() {
                            return Err(e);
                        }
                        if non_none(&e) && code_of(&e).ends_with("COLLATERAL") {
                            self.halt(&json!({"kind": "non_neutral_post_account"}))?;
                            *self.last_failure.borrow_mut() = Some(safe_code(&e));
                            return self.publish("global_collateral_requires_explicit_model");
                        }
                        self.backoff(&e)?;
                        *self.last_failure.borrow_mut() = Some(safe_code(&e));
                    }
                }
            }
            if self.now()
                >= instant_v(get(&self.activation, "exitExpiresAt"))
                    .map(|a| a as f64)
                    .unwrap_or(f64::NAN)
            {
                return self.publish("bounded_exit_window_expired_custody_retained");
            }
            let owned: Vec<String> = get(&coord.status()?, "inventory")
                .as_object()
                .map(|m| {
                    m.iter()
                        .filter(|(_, v)| get(v, "quantity").as_f64().map_or(false, |q| q > 0.0))
                        .map(|(k, _)| k.clone())
                        .collect()
                })
                .unwrap_or_default();
            let n = owned.len();
            if n > 0 {
                for i in 0..n {
                    let ex = owned[(self.sell_cursor.get() + i) % n].clone();
                    if Some(&ex) == preferred_attempt.as_ref() {
                        continue;
                    }
                    if self.negative_hint(&ex) {
                        self.exit_skipped.set(self.exit_skipped.get() + 1);
                        continue;
                    }
                    self.sell_cursor
                        .set((self.sell_cursor.get() + i + 1) % n);
                    let bridge = self.bridge_for(&ex)?;
                    let leg = get(&self.scope, "exchanges")
                        .as_array()
                        .and_then(|a| {
                            a.iter()
                                .find(|l| get(l, "exchangeId").as_str() == Some(ex.as_str()))
                                .cloned()
                        })
                        .unwrap_or(V::Null);
                    let res: Result<V, String> = (|| {
                        self.stage("sell_packet", |_| bridge.refresh_sell(&leg))?;
                        self.stage("prepare_sell", |_| coord.prepare_sell(&ex, None))?;
                        self.stage("final_none", |_| bridge.refresh_final_none())?;
                        self.packet_ready_at.set(Some(Instant::now()));
                        self.stage("dispatch_sell", |_| self.dispatch_prepared_guarded())?;
                        Ok(V::Null)
                    })();
                    match res {
                        Ok(_) => {
                            return self.publish("covered_profitable_fifo_sell_dispatched")
                        }
                        Err(e) => {
                            if is_fatal(&e) || !get(&coord.status()?, "intent").is_null() {
                                return Err(e);
                            }
                            if non_none(&e) && code_of(&e).ends_with("COLLATERAL") {
                                self.halt(&json!({"kind": "non_neutral_post_account"}))?;
                                *self.last_failure.borrow_mut() = Some(safe_code(&e));
                                return self
                                    .publish("global_collateral_requires_explicit_model");
                            }
                            self.backoff(&e)?;
                            *self.last_failure.borrow_mut() = Some(safe_code(&e));
                            return self.publish("owned_exit_not_currently_executable");
                        }
                    }
                }
            }
            self.publish("owned_custody_retained_no_profitable_hint")
        })();
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                let last_graph = self.last_graph_at.borrow().clone();
                let cr = self.cash_receipt.borrow().clone();
                let packet = self.packet_ready_at.get();
                *self.last_rejected_timing.borrow_mut() = json!({
                    "at": iso(self.now()),
                    "graphAgeMs": last_graph.as_ref().and_then(|g| {
                        instant_v(g).ok().map(|a| json!(self.now() - a as f64))
                    }).map(|v| v).unwrap_or(V::Null),
                    "cashAgeMs": if cr.is_null() { V::Null } else {
                        instant(get(&cr, "requestStartedAt").as_str().unwrap_or(""))
                            .ok().map(|a| json!(self.now() - a as f64)).unwrap_or(V::Null)
                    },
                    "localElapsedMsSincePacketReady": packet
                        .map(|p| json!((p.elapsed().as_secs_f64() * 1e3 * 1000.0).round() / 1000.0))
                        .unwrap_or(V::Null),
                });
                if non_none(&e) && code_of(&e).ends_with("COLLATERAL") {
                    self.halt(&json!({"kind": "non_neutral_post_account"}))?;
                    *self.last_failure.borrow_mut() = Some(safe_code(&e));
                    return self.publish("global_collateral_requires_explicit_model");
                }
                self.backoff(&e)?;
                *self.last_failure.borrow_mut() = Some(safe_code(&e));
                if is_fatal(&e) {
                    self.failed.set(true);
                    return Err(e);
                }
                self.publish("blocked_existing_custody_retained")
            }
        }
    }
}

/// openBaalCombinedCustodyHostV7 — returns Ok(None) when the descriptor file is
/// absent.
pub fn open_baal_combined_custody_host_v7(
    runtime_dir: &Path,
    workspace: &Path,
    universe: &V,
    deps: CustodyHostDeps,
) -> Result<Option<CustodyHostV7>, String> {
    let upgrade_file = runtime_dir.join("baal-combined-read-upgrade-v7.json");
    if !upgrade_file.exists() {
        return Ok(None);
    }
    let descriptor_file = runtime_dir.join("baal-combined-custody-request-v6.json");
    if !descriptor_file.exists() {
        return Ok(None);
    }
    need(runtime_dir.is_absolute() && workspace.is_absolute(), "DEPENDENCIES")?;
    let files: Vec<String> = [
        "baal-scaled-live-request.json",
        "baal-scaled-host-upgrade.json",
        "baal-scaled-host-upgrade-v3.json",
        "baal-scaled-host-upgrade-v5.json",
        "baal-capital-allocation.json",
        "paired-live-authority.json",
        "baal-scaled-rootcatalog.json",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut source_hashes: Vec<(String, String)> = Vec::new();
    let mut values: Map<String, V> = Map::new();
    for f in &files {
        let bytes = fs::read(runtime_dir.join(f)).map_err(|e| e.to_string())?;
        source_hashes.push((f.clone(), sha(&bytes)));
        values.insert(
            f.clone(),
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
        );
    }
    let hashes: Map<String, V> = values
        .iter()
        .map(|(f, v)| Ok((f.clone(), json!(payload_hash(v)?))))
        .collect::<Result<_, String>>()?;
    let request = values[files[0].as_str()].clone();
    let v2 = values[files[1].as_str()].clone();
    let v3 = values[files[2].as_str()].clone();
    let v5 = values[files[3].as_str()].clone();
    let capital = values[files[4].as_str()].clone();
    let authority = values[files[5].as_str()].clone();
    let catalog = values[files[6].as_str()].clone();
    let descriptor_bytes = fs::read(&descriptor_file).map_err(|e| e.to_string())?;
    let descriptor_bytes_hash = sha(&descriptor_bytes);
    let d: V = serde_json::from_slice(&descriptor_bytes).map_err(|e| e.to_string())?;
    let descriptor_hash = payload_hash(&d)?;
    let upgrade_bytes = fs::read(&upgrade_file).map_err(|e| e.to_string())?;
    let upgrade_bytes_hash = sha(&upgrade_bytes);
    let u: V = serde_json::from_slice(&upgrade_bytes).map_err(|e| e.to_string())?;
    let code = baal_combined_custody_v7_identity(workspace)?;
    let old_code = baal_scaled_no_controller_identity()?;

    let req_entry = instant_v(get(&request, "entryExpiresAt"));
    let req_started = instant_v(get(&request, "startedAt"));
    need(
        get(&request, "schemaVersion").as_i64() == Some(1)
            && get(&request, "kind").as_str() == Some("baal_scaled_live_production_v1")
            && get(&request, "ownerId").as_str() == Some("Codex")
            && get(&request, "runId").as_str().map_or(false, |r| {
                let b = r.as_bytes();
                r.starts_with("baal-scaled-no-")
                    && b.len() - 15 >= 1
                    && b.len() - 15 <= 80
                    && b[15..]
                        .iter()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
            })
            && get(&request, "tournamentId").as_str() == Some(T)
            && get(&request, "tournamentSlug").as_str() == Some("midterm-elections")
            && get(&request, "maximumActiveMarkets").as_i64() == Some(20)
            && req_entry.as_ref().ok().zip(req_started.as_ref().ok()).map_or(false, |(e, s)| {
                e > s && e - s <= 3_600_000
            }),
        "ORIGINAL_REQUEST",
    )?;
    need(
        *get(&request, "humanAllocationSourceHash") == hashes[files[4].as_str()]
            && get(&capital, "kind").as_str() == Some("human_directed_baal_capital_allocation_v1")
            && get(&capital, "ownerId").as_str() == Some("Codex")
            && get(&capital, "allocatedNominalCapital").as_str() == Some("1000.000000")
            && get(&capital, "maximumOrderNominalCost").as_str() == Some("25.000000")
            && get(&capital, "maximumMarketNominalExposure").as_str() == Some("100.000000")
            && get(&capital, "maximumEventNominalExposure").as_str() == Some("200.000000")
            && get(&capital, "maximumSharesPerOrder").as_i64() == Some(100)
            && get(&capital, "maximumActiveMarkets").as_i64() == Some(20)
            && get(&capital, "fee").as_str() == Some("0.000000"),
        "HUMAN_ALLOCATION",
    )?;
    need(
        *get(&request, "authorityHash") == hashes[files[5].as_str()]
            && get(&authority, "kind").as_str() == Some("direct_human_authorized_bot_update")
            && get(&authority, "ownerId").as_str() == Some("Codex")
            && get(&authority, "threadId").as_str()
                == Some("01a0f8cc-b9bb-77d2-9f4c-2844396f1b79")
            && *get(&request, "catalogHash") == hashes[files[6].as_str()]
            && get(&catalog, "tournamentId").as_str() == Some(T),
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
    let upgrades: [(&V, &str, &str, Option<&V>); 3] = [
        (&v2, "baal_scaled_host_upgrade_v2", v2hash.as_str(), None),
        (
            &v3,
            "baal_scaled_host_upgrade_v3",
            v3hash.as_str(),
            Some(&hashes[files[1].as_str()]),
        ),
        (
            &v5,
            "baal_scaled_host_upgrade_v5",
            v5hash.as_str(),
            Some(&hashes[files[2].as_str()]),
        ),
    ];
    for (u, kind, graph, previous) in &upgrades {
        need(
            get(u, "schemaVersion").as_i64() == Some(1)
                && get(u, "kind").as_str() == Some(*kind)
                && get(u, "ownerId").as_str() == get(&request, "ownerId").as_str()
                && get(u, "runId").as_str() == get(&request, "runId").as_str()
                && get(u, "tournamentId").as_str() == Some(T)
                && *get(u, "originalRequestHash") == hashes[files[0].as_str()]
                && get(u, "originalHostCodeIdentityHash").as_str()
                    == Some(old_host_hash.as_str())
                && payload_hash(get(u, "codeIdentity"))? == *graph
                && get(u, "entryExpiresAt").as_str()
                    == get(&request, "entryExpiresAt").as_str()
                && instant_v(get(u, "recordedAt"))
                    .map(|a| {
                        instant_v(get(&request, "startedAt"))
                            .map(|s| a >= s && (a as f64) <= (deps.now)())
                            .unwrap_or(false)
                    })
                    .unwrap_or(false)
                && previous.map_or(true, |p| {
                    get(u, "priorUpgradeHash").as_str() == p.as_str()
                }),
            "PRIOR_UPGRADE",
        )?;
    }
    need(
        get(&v5, "priorHostCodeIdentityHash").as_str() == Some(v3hash.as_str()),
        "PRIOR_V5_GRAPH",
    )?;
    need(
        get(universe, "tournamentId").as_str() == Some(T)
            && get(universe, "hash").as_str() == get(&request, "universeHash").as_str()
            && get(universe, "exchanges").is_array()
            && get(&request, "allowedMarkets").is_array()
            && get(&request, "allowedMarkets")
                .as_array()
                .map_or(false, |a| {
                    let mut seen = std::collections::HashSet::new();
                    a.iter().all(|x| seen.insert(x.clone()))
                }),
        "UNIVERSE",
    )?;
    let allowed: Vec<V> = get(&request, "allowedMarkets")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let members: Vec<V> = get(universe, "exchanges")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| allowed.contains(get(m, "marketId")))
        .collect();
    need(
        members.len() == allowed.len()
            && members.iter().all(|m| get(m, "option").as_str() == Some("YES")),
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
    let legacy_dir = runtime_dir
        .join("baal-scaled-live-runs")
        .join(get(&request, "runId").as_str().unwrap_or(""));
    let legacy_controller_dir = legacy_dir.join("controller");
    let legacy_budget_dir = legacy_dir.join("budget");
    let legacy_file = legacy_controller_dir.join("baal-scaled-no-journal.jsonl");
    let original_bytes = fs::read(&legacy_file).map_err(|e| e.to_string())?;
    let seed = replay_baal_scaled_controller_v1(&json!({
        "bodyText": String::from_utf8_lossy(&original_bytes),
        "expectedCodeIdentity": old_code,
        "expectedScope": scope,
        "expectedAllocation": allocation,
    }))?;
    let keys: Vec<&str> = vec![
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
    let mut dkeys: Vec<String> = d
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    dkeys.sort();
    let mut skeys: Vec<String> = keys.iter().map(|s| s.to_string()).collect();
    skeys.sort();
    let d_rec = instant_v(get(&d, "recordedAt"));
    need(
        dkeys.join("|") == skeys.join("|")
            && get(&d, "schemaVersion").as_i64() == Some(1)
            && get(&d, "kind").as_str() == Some("baal_combined_custody_host_v7")
            && get(&d, "ownerId").as_str() == get(&scope, "ownerId").as_str()
            && get(&d, "runId").as_str() == get(&scope, "runId").as_str()
            && get(&d, "tournamentId").as_str() == Some(T)
            && get(&d, "universeHash").as_str() == get(universe, "hash").as_str()
            && *get(&d, "originalRequestHash") == hashes[files[0].as_str()]
            && *get(&d, "priorV5UpgradeHash") == hashes[files[3].as_str()]
            && get(&d, "originalV5GraphHash").as_str() == Some(v5hash.as_str())
            && get(&d, "oldEntryExpiresAt").as_str()
                == get(&request, "entryExpiresAt").as_str()
            && same(get(&d, "legacyAnchor"), get(&seed, "anchor"))?
            && get(&d, "legacyCodeIdentityHash").as_str()
                == get(&seed, "legacyCodeIdentityHash").as_str()
            && get(&d, "seedHash").as_str() == Some(payload_hash(&seed)?.as_str())
            && same(get(&d, "codeIdentity"), &baal_combined_custody_v6_identity(workspace)?)?
            && d_rec.as_ref().map(|x| *x as f64 <= (deps.now)()).unwrap_or(false)
            && d_rec
                .as_ref()
                .map(|x| {
                    instant_v(get(&v5, "recordedAt"))
                        .map(|v| *x >= v)
                        .unwrap_or(false)
                })
                .unwrap_or(false),
        "DESCRIPTOR",
    )?;
    let recorded_at = instant_v(get(&u, "recordedAt"));
    let ended_at = instant_v(get(&u, "endedAt"));
    need(
        get(&u, "schemaVersion").as_i64() == Some(1)
            && get(&u, "kind").as_str() == Some("baal_combined_read_upgrade_v7")
            && get(&u, "originalDescriptorHash").as_str() == Some(descriptor_hash.as_str())
            && matches!(&recorded_at, Ok(r) if *r > 0)
            && matches!(&ended_at, Ok(e2) if *e2 == 0),
        "READ_ONLY_UPGRADE_BINDING",
    )?;
    let activation = get(&d, "activation").clone();
    let migration = get(&d, "migration").clone();
    let epoch = get(&migration, "implementationEpoch")
        .as_str()
        .unwrap_or("")
        .to_string();
    let epoch_ok = {
        let b = epoch.as_bytes();
        !b.is_empty()
            && b.len() <= 100
            && b.iter()
                .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
    };
    let act_started = instant_v(get(&activation, "startedAt"));
    need(
        epoch_ok
            && get(&migration, "authorityHash").as_str()
                == get(&request, "authorityHash").as_str()
            && get(&activation, "authorityHash").as_str()
                == get(&request, "authorityHash").as_str()
            && get(&activation, "allowedFirstExchangeIds")
                .as_array()
                .map_or(false, |a| {
                    a.len() == 1
                        && a[0].as_str() == get(&d, "firstExchangeId").as_str()
                })
            && get(&activation, "codeIdentityHash").as_str()
                == Some(payload_hash(&live_coordinator_identity(workspace)?)?.as_str())
            && get(
                get(
                    get(get(&seed, "state"), "inventory"),
                    get(&d, "firstExchangeId").as_str().unwrap_or(""),
                ),
                "proof",
            )
            .pipe_get("remainingOwnedQuantity")
            .as_f64()
            .map_or(false, |q| q > 0.0)
            && act_started.as_ref().map(|a| *a as f64 <= (deps.now)()).unwrap_or(false),
        "ACTIVATION_EPOCH",
    )?;
    let dir = runtime_dir
        .join("baal-combined-custody-runs")
        .join(get(&request, "runId").as_str().unwrap_or(""))
        .join(&epoch);
    let coordinator_dir = dir.join("coordinator");
    let budget_dir = dir.join("budget");
    let lease_dir = runtime_dir.join("baal-financial-owner");
    let lease_journal_file = dir.join("financial-owner-epochs.jsonl");
    let stop_file = runtime_dir.join("EXECUTION_STOP");

    let inner = Rc::new(HostInner {
        me: RefCell::new(Weak::new()),
        runtime_dir: runtime_dir.to_path_buf(),
        workspace: workspace.to_path_buf(),
        universe: universe.clone(),
        deps,
        descriptor_file: descriptor_file.clone(),
        upgrade_file: upgrade_file.clone(),
        upgrade_bytes_hash,
        descriptor_bytes_hash,
        d: d.clone(),
        descriptor_hash,
        code,
        old_code: old_code.clone(),
        files: files.clone(),
        source_hashes,
        request: request.clone(),
        v5: v5.clone(),
        scope: scope.clone(),
        allocation: allocation.clone(),
        legacy_controller_dir: legacy_controller_dir.clone(),
        legacy_budget_dir: legacy_budget_dir.clone(),
        legacy_file: legacy_file.clone(),
        dir: dir.clone(),
        coordinator_dir: coordinator_dir.clone(),
        budget_dir: budget_dir.clone(),
        lease_dir: lease_dir.clone(),
        lease_journal_file: lease_journal_file.clone(),
        stop_file: stop_file.clone(),
        activation,
        migration,
        epoch,
        seed: seed.clone(),
        live: RefCell::new(HostLive {
            coordinator: None,
            budget: None,
            bridges: HashMap::new(),
            selected_bridge: None,
        }),
        financial_lease: RefCell::new(None),
        legacy_controller_fence: RefCell::new(None),
        legacy_budget_fence: RefCell::new(None),
        epoch_journal: RefCell::new(None),
        epoch_receipt: RefCell::new(V::Null),
        raw_account: RefCell::new(V::Null),
        cash_receipt: RefCell::new(V::Null),
        closed: Cell::new(false),
        failed: Cell::new(false),
        busy: Cell::new(false),
        last_failure: RefCell::new(None),
        last_action: RefCell::new("awaiting_fresh_cash".to_string()),
        last_step_at: RefCell::new(None),
        exit_skipped: Cell::new(0),
        last_hint: RefCell::new(V::Null),
        sell_cursor: Cell::new(0),
        hedge_tried_at: Cell::new(None),
        last_graph_at: RefCell::new(None),
        last_book_at: RefCell::new(None),
        packet_ready_at: Cell::new(None),
        last_dispatch_timing: RefCell::new(V::Null),
        last_rejected_timing: RefCell::new(V::Null),
        last_exit_priority: RefCell::new(V::Null),
        permits: RefCell::new(HashMap::new()),
        timings: RefCell::new(VecDeque::new()),
        stage_timings: RefCell::new(VecDeque::new()),
    });
    *inner.me.borrow_mut() = Rc::downgrade(&inner);

    let boot: Result<(), String> = (|| {
        inner.immutable()?;
        inner.legacy()?;
        *inner.raw_account.borrow_mut() = inner.actual_account()?;
        need(
            !legacy_controller_dir.join("execution-owner.lock").exists()
                && !legacy_budget_dir.join("execution-owner.lock").exists(),
            "ORIGINAL_WRITER_STILL_OWNS",
        )?;
        let financial_lease = acquire_owner_lease(
            &inner.lease_dir,
            get(&inner.scope, "ownerId").as_str().unwrap_or(""),
            get(&inner.scope, "runId").as_str().unwrap_or(""),
            "baal-global-financial-owner",
            (inner.deps.now)(),
        )
        .map_err(|e| e.message.clone())?;
        let controller_fence = acquire_owner_lease(
            &inner.legacy_controller_dir,
            get(&inner.scope, "ownerId").as_str().unwrap_or(""),
            get(&inner.scope, "runId").as_str().unwrap_or(""),
            "baal-custody-v6-legacy-fence",
            (inner.deps.now)(),
        )
        .map_err(|e| e.message.clone())?;
        let budget_fence = acquire_owner_lease(
            &inner.legacy_budget_dir,
            get(&inner.scope, "ownerId").as_str().unwrap_or(""),
            get(&inner.scope, "runId").as_str().unwrap_or(""),
            "baal-custody-v6-legacy-budget-fence",
            (inner.deps.now)(),
        )
        .map_err(|e| e.message.clone())?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&inner.dir)
            .map_err(|e| e.to_string())?;
        let mut epoch_journal = Journal::open(&inner.lease_journal_file)?;
        if inner.lease_journal_file.exists() {
            let bytes = fs::read(&inner.lease_journal_file).map_err(|e| e.to_string())?;
            need(
                bytes.is_empty() || *bytes.last().unwrap() == b'\n',
                "PARTIAL_OWNER_JOURNAL",
            )?;
            for line in String::from_utf8_lossy(&bytes)
                .split('\n')
                .filter(|l| !l.is_empty())
            {
                let r: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
                need(
                    get(get(&r, "event"), "kind").as_str() == Some("financial_owner_bound")
                        && get(get(&r, "state"), "descriptorHash").as_str()
                            == Some(inner.descriptor_hash.as_str())
                        && get(get(&r, "state"), "seedHash").as_str()
                            == get(&inner.d, "seedHash").as_str()
                        && get(get(&r, "state"), "implementationEpoch").as_str()
                            == Some(inner.epoch.as_str())
                        && get(get(&r, "state"), "ownerId").as_str()
                            == get(&inner.scope, "ownerId").as_str()
                        && get(get(&r, "state"), "runId").as_str()
                            == get(&inner.scope, "runId").as_str()
                        && get(get(&r, "state"), "tournamentId").as_str() == Some(T)
                        && hex(get(get(get(&r, "state"), "rawAccountLease"), "leaseId"))
                        && get(get(get(&r, "state"), "financialOwnerLock"), "runId")
                            .as_str()
                            == get(&inner.scope, "runId").as_str()
                        && get(get(get(&r, "state"), "financialOwnerLock"), "scope")
                            .as_str()
                            == Some("baal-global-financial-owner")
                        && get(get(get(&r, "state"), "financialOwnerLock"), "pid")
                            .as_i64()
                            .map_or(false, |p| p > 0)
                        && hex(get(get(get(&r, "state"), "financialOwnerLock"), "identityHash"))
                        && !get(get(&r, "state"), "financialOwnerLock")
                            .as_object()
                            .map_or(false, |m| m.contains_key("token"))
                        && !get(get(&r, "state"), "legacyControllerFence")
                            .as_object()
                            .map_or(false, |m| m.contains_key("token"))
                        && !get(get(&r, "state"), "legacyBudgetFence")
                            .as_object()
                            .map_or(false, |m| m.contains_key("token")),
                    "OWNER_JOURNAL_BINDING",
                )?;
            }
        }
        let epoch_receipt = json!({
            "schemaVersion": 1,
            "kind": "derived_financial_run_account_lease_v1",
            "ownerId": get(&inner.scope, "ownerId"),
            "runId": get(&inner.scope, "runId"),
            "tournamentId": T,
            "implementationEpoch": inner.epoch,
            "descriptorHash": inner.descriptor_hash,
            "seedHash": get(&inner.d, "seedHash"),
            "rawAccountLease": copy(&inner.raw_account.borrow()),
            "financialOwnerLock": lock_summary(&financial_lease.identity)?,
            "legacyControllerFence": lock_summary(&controller_fence.identity)?,
            "legacyBudgetFence": lock_summary(&budget_fence.identity)?,
            "at": iso((inner.deps.now)()),
        });
        epoch_journal.commit(&epoch_receipt, &json!({"kind": "financial_owner_bound"}))?;
        *inner.epoch_journal.borrow_mut() = Some(epoch_journal);
        *inner.epoch_receipt.borrow_mut() = epoch_receipt;
        *inner.financial_lease.borrow_mut() = Some(financial_lease);
        *inner.legacy_controller_fence.borrow_mut() = Some(controller_fence);
        *inner.legacy_budget_fence.borrow_mut() = Some(budget_fence);
        inner.leases()?;
        let me = inner.rc();
        let me2 = inner.rc();
        let me3 = inner.rc();
        let me4 = inner.rc();
        let me5 = inner.rc();
        let me6 = inner.rc();
        let me7 = inner.rc();
        let me8 = inner.rc();
        let api_dispatch = Rc::clone(&inner.deps.api);
        let epoch_clone = inner.epoch.clone();
        let coordinator = open_baal_neutral_owned_counterpart_pilot_v2(
            &inner.coordinator_dir,
            &inner.workspace,
            &inner.legacy_file,
            &inner.old_code,
            &inner.scope,
            &inner.allocation,
            &inner.activation,
            &inner.migration,
            NeutralOwnedPilotDeps {
                dispatch: Rc::new(move |spec: &V,
                    _bd: Option<&baal_execution::transport::Guard>,
                    _bn: Option<&baal_execution::transport::Guard>| {
                    (api_dispatch.borrow().dispatch)(spec, None, None).map_err(|m| {
                        WireDispatchError {
                            message: m,
                            wire_receipt: None,
                        }
                    })
                }),
                raw_receipt_of: Rc::new(move |response: &V, meta: &V| {
                    me.raw_of(response, meta)
                }),
                assert_account_lease: Rc::new(move || me2.leases()),
                assert_financial_owner_lease: Rc::new(move || me3.financial()),
                install_funding_reservation: Rc::new(move |r: &V| {
                    me4.live
                        .borrow()
                        .budget
                        .as_ref()
                        .map(|b| b.install_funding_reservation(r))
                        .unwrap_or_else(|| Ok(V::Null))
                }),
                get_global_risk: Rc::new(move || {
                    me5.timed("current_budget_risk", |h| {
                        h.live
                            .borrow()
                            .budget
                            .as_ref()
                            .map(|b| b.global_risk())
                            .unwrap_or_else(|| Ok(V::Null))
                    })
                }),
                get_evidence: Rc::new(move |c: &V| {
                    me6.live
                        .borrow()
                        .selected_bridge
                        .as_ref()
                        .map(|b| b.get_evidence(c))
                        .unwrap_or_else(|| Ok(V::Null))
                }),
                on_financial_halt: Rc::new(move |h: &V| me7.halt(h)),
                can_operate: Rc::new({
                    let me = inner.rc();
                    move || me.guard().is_ok()
                }),
                can_check_mechanical: Rc::new({
                    let me = inner.rc();
                    move || me.mechanical_leases().is_ok()
                }),
                assert_mechanical_account_lease: Rc::new(move || me8.mechanical_leases()),
                assert_mechanical_financial_owner_lease: Rc::new({
                    let me = inner.rc();
                    move || {
                        let mut v = me.mechanical_leases()?;
                        if let V::Object(ref mut m) = v {
                            m.insert("implementationEpoch".into(), json!(epoch_clone));
                        }
                        Ok(v)
                    }
                }),
                now: Rc::clone(&inner.deps.now),
            },
        )?;
        inner.live.borrow_mut().coordinator = Some(Rc::new(coordinator));
        Ok(())
    })();
    if boot.is_err() {
        let e = boot.err().unwrap();
        let mut live = inner.live.borrow_mut();
        for (_, b) in live.bridges.drain() {
            let _ = b.close();
        }
        if let Some(b) = live.budget.take() {
            let _ = b.close();
        }
        if let Some(c) = live.coordinator.take() {
            let _ = c.close();
        }
        drop(live);
        for l in [
            &inner.legacy_budget_fence,
            &inner.legacy_controller_fence,
            &inner.financial_lease,
        ] {
            if let Some(mut lease) = l.borrow_mut().take() {
                let _ = lease.release();
            }
        }
        return Err(e);
    }
    Ok(Some(CustodyHostV7 { inner }))
}

pub struct CustodyHostV7 {
    inner: Rc<HostInner>,
}

impl CustodyHostV7 {
    pub fn step(&self) -> Result<V, String> {
        self.inner.step()
    }
    pub fn status(&self) -> V {
        self.inner.status()
    }
    /// authorizes(spec): the GET bootstrap permit stored for the spec's
    /// beforeDispatch pointer matches method/endpoint/beforeNetwork pointer
    /// with no body/bodyJson/headers, else any bridge's or the coordinator's
    /// authorizes. bd_ptr/bn_ptr are the data addresses of the guard fns the
    /// caller attached to this spec (JS reads them off the spec object).
    pub fn authorizes(&self, spec: &V, bd_ptr: usize, bn_ptr: usize) -> bool {
        (|| -> Result<bool, String> {
            self.inner.guard()?;
            let p = self.inner.permits.borrow().get(&bd_ptr).cloned();
            let ok = match p {
                Some(p) => {
                    get(spec, "method").as_str() == Some("GET")
                        && get(spec, "endpoint").as_str() == get(&p, "endpoint").as_str()
                        && get(&p, "beforeNetworkPtr").as_u64() == Some(bn_ptr as u64)
                        && spec.get("body").is_none()
                        && spec.get("bodyJson").is_none()
                        && spec.get("headers").is_none()
                }
                None => false,
            };
            if ok {
                return Ok(true);
            }
            {
                let live = self.inner.live.borrow();
                for b in live.bridges.values() {
                    if b.authorizes_ptrs(spec, bd_ptr, bn_ptr) {
                        return Ok(true);
                    }
                }
                if let Some(c) = live.coordinator.as_ref() {
                    if c.authorizes_ptrs(spec, bd_ptr, bn_ptr) {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        })()
        .unwrap_or(false)
    }
    pub fn on_before_fetch(&self, e: &V) -> Result<V, String> {
        self.inner
            .live
            .borrow()
            .coordinator
            .as_ref()
            .map(|c| c.on_before_fetch(e))
            .unwrap_or_else(|| Ok(V::Null))
    }
    pub fn on_fetch_start(&self, e: &V) -> Result<bool, String> {
        let at_fetch_callback = Instant::now();
        let coord = self.inner.live.borrow().coordinator.clone();
        let result = coord
            .as_ref()
            .map(|c| c.on_fetch_start(e))
            .transpose()?
            .unwrap_or(false);
        if get(e, "method").as_str() == Some("POST") {
            let last_graph = self.inner.last_graph_at.borrow().clone();
            let cr = self.inner.cash_receipt.borrow().clone();
            let last_book = self.inner.last_book_at.borrow().clone();
            let packet = self.inner.packet_ready_at.get();
            *self.inner.last_dispatch_timing.borrow_mut() = json!({
                "actualFetchStartAt": get(e, "requestStartedAt"),
                "graphRequestStartedAt": last_graph.clone().unwrap_or(V::Null),
                "graphAgeMsAtFetch": last_graph.as_ref().and_then(|g| {
                    let gi = instant_v(g).ok()?;
                    let ei = instant_v(get(e, "requestStartedAt")).ok()?;
                    Some(json!(ei - gi))
                }).unwrap_or(V::Null),
                "cashRequestStartedAt": if cr.is_null() { V::Null } else { get(&cr, "requestStartedAt").clone() },
                "cashAgeMsAtFetch": if cr.is_null() { V::Null } else {
                    instant(get(&cr, "requestStartedAt").as_str().unwrap_or("")).ok()
                        .and_then(|c| instant_v(get(e, "requestStartedAt")).ok().map(|t| t - c))
                        .map(|x| json!(x)).unwrap_or(V::Null)
                },
                "bookRequestStartedAt": last_book.clone().unwrap_or(V::Null),
                "bookAgeMsAtFetch": last_book.as_ref().and_then(|b| {
                    let bi = instant_v(b).ok()?;
                    let ei = instant_v(get(e, "requestStartedAt")).ok()?;
                    Some(json!(ei - bi))
                }).unwrap_or(V::Null),
                "localElapsedMsSincePacketReady": packet
                    .map(|p| json!((at_fetch_callback.duration_since(p).as_secs_f64() * 1e3 * 1000.0).round() / 1000.0))
                    .unwrap_or(V::Null),
            });
        }
        Ok(result)
    }
    pub fn blocks_legacy_writes(&self) -> bool {
        true
    }
    pub fn allows_legacy(&self, s: &V) -> bool {
        get(s, "method").as_str() == Some("GET")
    }
    pub fn close(&self) -> Result<(), String> {
        need(!self.inner.busy.get(), "BUSY")?;
        if self.inner.closed.get() {
            return Ok(());
        }
        self.inner.closed.set(true);
        let mut error: Option<String> = None;
        {
            let mut live = self.inner.live.borrow_mut();
            let mut closeables: Vec<Box<dyn FnOnce() -> Result<(), String>>> = Vec::new();
            for (_, b) in live.bridges.drain() {
                closeables.push(Box::new(move || b.close()));
            }
            if let Some(b) = live.budget.take() {
                closeables.push(Box::new(move || b.close()));
            }
            if let Some(c) = live.coordinator.take() {
                closeables.push(Box::new(move || c.close()));
            }
            for h in closeables {
                if let Err(e) = h() {
                    if error.is_none() {
                        error = Some(e);
                    }
                }
            }
        }
        for l in [
            &self.inner.legacy_budget_fence,
            &self.inner.legacy_controller_fence,
            &self.inner.financial_lease,
        ] {
            if let Some(mut lease) = l.borrow_mut().take() {
                if let Err(e) = lease.release() {
                    if error.is_none() {
                        error = Some(e.message.clone());
                    }
                }
            }
        }
        match error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}
