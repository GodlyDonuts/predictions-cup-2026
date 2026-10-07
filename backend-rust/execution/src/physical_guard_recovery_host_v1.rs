// execution/physical-guard-recovery-host-v1.mjs — get-only recovery host.
// Sealed by a marker file, the EXECUTION_STOP bytes and the original actor
// journal; completes one scoped read packet + reconciliation at most once.
// Same shape as blocked-admission-recovery-host-v1 with the v8 actor replay
// and the physical-guard audit/reconciliation inputs (diagnostic,
// guardFailure and the parsed EXECUTION_STOP payload).
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_int, sha256};
use serde_json::{json, Map, Value as V};

use crate::baal_generic_no_controller_v8::{
    baal_generic_no_controller_v8_identity, replay_baal_generic_no_journal_v8,
};
use crate::blocked_admission_read_collector_v1::{
    collect_blocked_admission_account_v1, BlockedAdmissionReadCollectorDeps,
};
use crate::core::payload_hash;
use crate::ledger::acquire_owner_lease;
use crate::physical_guard_admission_audit_v1::audit_physical_guard_admission_v1;
use crate::physical_guard_reconciliation_v1::reconcile_physical_guard_admission_v1;
use baal_service_runtime::ledger::OwnerLease;

/// sha(readFileSync(new URL(import.meta.url))) — this module's own bytes.
const IMPLEMENTATION: &[u8] = include_bytes!("physical_guard_recovery_host_v1.rs");

fn need(ok: bool, c: &str) -> Result<(), String> {
    if !ok {
        Err(format!("BLOCKED_RECOVERY_{c}"))
    } else {
        Ok(())
    }
}

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}

fn dparse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}

fn sha(b: &[u8]) -> String {
    sha256(b)
}

fn sha_file(f: &Path) -> Result<String, String> {
    Ok(sha(&fs::read(f).map_err(|e| e.to_string())?))
}

/// JS path.resolve lexical normalization.
fn resolve_path(base: &Path, rel: &str) -> PathBuf {
    let mut comps: Vec<String> = Vec::new();
    let rel_p = Path::new(rel);
    let push = |p: &Path, comps: &mut Vec<String>| {
        for c in p.components() {
            match c {
                Component::Normal(s) => comps.push(s.to_string_lossy().into_owned()),
                Component::ParentDir => {
                    comps.pop();
                }
                _ => {}
            }
        }
    };
    if rel_p.is_absolute() {
        push(rel_p, &mut comps);
    } else {
        push(base, &mut comps);
        push(rel_p, &mut comps);
    }
    PathBuf::from(format!("/{}", comps.join("/")))
}

pub struct PhysicalGuardRecoveryHostDeps {
    /// api.dispatch of the serialized account client.
    pub api_dispatch: Box<dyn Fn(&V) -> Result<V, String>>,
    /// receiptOf(response,{method,requestBodyHash}).
    pub receipt_of: Box<dyn Fn(&V, &V) -> Result<V, String>>,
    /// getCoreOwnerRecord() — live core owner record.
    pub get_core_owner_record: Box<dyn Fn() -> Result<V, String>>,
    /// getAccountLease() — live account lease view.
    pub get_account_lease: Box<dyn Fn() -> Result<V, String>>,
    /// canOperate() === true.
    pub can_operate: Box<dyn Fn() -> bool>,
    /// now() — epoch ms.
    pub now: Box<dyn Fn() -> f64>,
}

struct Inner {
    closed: bool,
    busy: bool,
    completed: bool,
    permits: HashMap<u64, V>,
    financial: OwnerLease,
}

pub struct PhysicalGuardRecoveryHostV1 {
    inner: Rc<RefCell<Inner>>,
    deps: PhysicalGuardRecoveryHostDeps,
    state: V,
    events: Vec<V>,
    timings: Vec<V>,
    order: V,
    replay_anchor: V,
    marker: V,
    marker_bytes: Vec<u8>,
    marker_file: PathBuf,
    stop_file: PathBuf,
    stop: V,
    actor_file: PathBuf,
    result_file: PathBuf,
    expires_at: f64,
    implementation_hash: String,
}

/// openPhysicalGuardRecoveryHostV1({runtimeDir,api,receiptOf,
/// getCoreOwnerRecord,getAccountLease,canOperate,now}).
/// Returns Ok(None) when no marker file exists.
pub fn open_physical_guard_recovery_host_v1(
    runtime_dir: &Path,
    deps: PhysicalGuardRecoveryHostDeps,
) -> Result<Option<PhysicalGuardRecoveryHostV1>, String> {
    let implementation_hash = sha(IMPLEMENTATION);
    let marker_file = runtime_dir.join("physical-guard-recovery-request-v1.json");
    if !marker_file.exists() {
        return Ok(None);
    }
    let marker_bytes = fs::read(&marker_file).map_err(|e| e.to_string())?;
    let marker: V = serde_json::from_slice(&marker_bytes).map_err(|e| e.to_string())?;
    let stop_file = runtime_dir.join("EXECUTION_STOP");
    let stop_bytes = fs::read(&stop_file).map_err(|e| e.to_string())?;
    need(
        sget(&marker, "ownerId") == Some("Codex")
            && sget(&marker, "kind") == Some("physical_guard_get_only_request_v1")
            && sget(&marker, "financialStopHash") == Some(sha(&stop_bytes).as_str())
            && sget(&marker, "implementationHash") == Some(implementation_hash.as_str())
            && dparse(get(&marker, "expiresAt")) > (deps.now)(),
        "EXPLICIT_REQUEST",
    )?;
    let actor_rel = sget(&marker, "actorRelative").unwrap_or("");
    let actor_file = resolve_path(runtime_dir, actor_rel);
    let runtime_resolved = resolve_path(Path::new("/"), runtime_dir.to_string_lossy().as_ref());
    need(
        actor_file
            .to_string_lossy()
            .starts_with(&format!("{}/", runtime_resolved.to_string_lossy())),
        "ACTOR_PATH",
    )?;
    let bytes = fs::read(&actor_file).map_err(|e| e.to_string())?;
    need(
        sget(&marker, "actorSha256") == Some(sha(&bytes).as_str()),
        "ACTOR_SEAL",
    )?;
    let body_text = String::from_utf8_lossy(&bytes).into_owned();
    let rows: Vec<V> = body_text
        .trim_end()
        .split('\n')
        .map(|l| serde_json::from_str(l).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let custody_seed = get(
        get(get(rows.first().unwrap_or(&V::Null), "event"), "input"),
        "custodySeed",
    )
    .clone();
    let replay = replay_baal_generic_no_journal_v8(
        &body_text,
        &baal_generic_no_controller_v8_identity()?,
        &custody_seed,
    )?;
    let state = get(&replay, "state").clone();
    let events: Vec<V> = rows
        .iter()
        .map(|r| get(get(r, "event"), "input").clone())
        .collect();
    let timing_file = runtime_dir.join("read-timings.jsonl");
    let timing_all = fs::read(&timing_file).map_err(|e| e.to_string())?;
    let prefix_len = js_int(get(&marker, "timingPrefixBytes")).unwrap_or(-1);
    let timing_bytes = if prefix_len >= 0 {
        &timing_all[..(prefix_len as usize).min(timing_all.len())]
    } else {
        &timing_all[..0]
    };
    need(
        prefix_len >= 0
            && timing_bytes.len() as i64 == prefix_len
            && sget(&marker, "timingPrefixHash") == Some(sha(timing_bytes).as_str()),
        "TIMING_SEAL",
    )?;
    let timing_text = String::from_utf8_lossy(timing_bytes).into_owned();
    let timings: Vec<V> = timing_text
        .trim_end()
        .split('\n')
        .map(|l| serde_json::from_str(l).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let stop: V =
        serde_json::from_slice(&stop_bytes).map_err(|e| e.to_string())?;
    let audit = audit_physical_guard_admission_v1(&json!({
        "state": state,
        "events": events,
        "timings": timings,
        "diagnostic": get(&marker, "diagnostic"),
        "guardFailure": get(&marker, "guardFailure"),
        "stop": stop,
        "now": (deps.now)(),
    }))?;
    let order = get(&state, "orders")
        .as_array()
        .and_then(|a| a.last())
        .cloned()
        .unwrap_or(V::Null);
    need(
        get(get(&state, "scope"), "runId") == get(&marker, "runId")
            && get(&audit, "intentBodyHash") == get(&marker, "intentBodyHash"),
        "EXACT_INTENT",
    )?;
    let expires_at = dparse(get(&marker, "expiresAt"));
    let financial = acquire_owner_lease(
        &runtime_dir.join("baal-financial-owner"),
        "Codex",
        sget(&marker, "runId").unwrap_or(""),
        "baal-global-financial-owner",
        (deps.now)(),
    )
    .map_err(|e| e.to_string())?;
    Ok(Some(PhysicalGuardRecoveryHostV1 {
        inner: Rc::new(RefCell::new(Inner {
            closed: false,
            busy: false,
            completed: false,
            permits: HashMap::new(),
            financial,
        })),
        deps,
        state,
        events,
        timings,
        order,
        replay_anchor: get(&replay, "anchor").clone(),
        marker,
        marker_bytes,
        marker_file,
        stop_file,
        stop,
        actor_file,
        result_file: runtime_dir.join("physical-guard-recovery-result-v1.json"),
        expires_at,
        implementation_hash,
    }))
}

impl PhysicalGuardRecoveryHostV1 {
    /// guard() — reseals implementation, marker, stop file and actor bytes,
    /// then reasserts the financial lease and the core/account owner record.
    fn guard(&self) -> Result<(), String> {
        let closed = self.inner.borrow().closed;
        need(
            sha(IMPLEMENTATION) == self.implementation_hash
                && !closed
                && (self.deps.can_operate)()
                && (self.deps.now)() < self.expires_at
                && sha_file(&self.marker_file)? == sha(&self.marker_bytes)
                && sha_file(&self.stop_file)? == sget(&self.marker, "financialStopHash").unwrap_or("")
                && sha_file(&self.actor_file)? == sget(&self.marker, "actorSha256").unwrap_or(""),
            "OWNER_STOP_SOURCE",
        )?;
        // assertOwnership errors propagate without the BLOCKED_RECOVERY prefix.
        self.inner
            .borrow()
            .financial
            .assert_ownership()
            .map_err(|e| e.to_string())?;
        let core = (self.deps.get_core_owner_record)()?;
        let lease = (self.deps.get_account_lease)()?;
        need(
            js_int(get(&core, "pid")) == Some(std::process::id() as i64)
                && sget(&core, "ownerId") == Some("Codex")
                && get(&core, "scope") == get(get(&self.state, "scope"), "tournamentId")
                && get(&lease, "leaseId").as_str() == Some(payload_hash(&core)?.as_str()),
            "CORE_OWNER",
        )?;
        Ok(())
    }

    /// adapter.dispatch — guard + GET-only + permit capture, then api.dispatch.
    fn adapter_dispatch(&self, spec: &V) -> Result<V, String> {
        self.guard()?;
        need(sget(spec, "method") == Some("GET"), "READ_ONLY")?;
        let token = get(spec, "beforeDispatch").as_u64().unwrap_or(u64::MAX);
        self.inner.borrow_mut().permits.insert(
            token,
            json!({
                "endpoint": get(spec, "endpoint"),
                "beforeNetwork": get(spec, "beforeNetwork"),
            }),
        );
        (self.deps.api_dispatch)(spec)
    }

    /// status()
    pub fn status(&self) -> V {
        let g = self.inner.borrow();
        let mut out = Map::new();
        out.insert("mode".into(), json!("physical_guard_get_only_recovery_v1"));
        out.insert("ownerId".into(), json!("Codex"));
        out.insert("runId".into(), get(&self.marker, "runId").clone());
        out.insert("completed".into(), json!(g.completed));
        out.insert("closed".into(), json!(g.closed));
        out.insert("busy".into(), json!(g.busy));
        out.insert("executionAuthority".into(), json!(false));
        out.insert("additionalApiClients".into(), json!(0));
        V::Object(out)
    }

    /// authorizes(s)
    pub fn authorizes(&self, s: &V) -> bool {
        let run = || -> Result<bool, String> {
            self.guard()?;
            let token = get(s, "beforeDispatch").as_u64().unwrap_or(u64::MAX);
            let p = match self.inner.borrow().permits.get(&token).cloned() {
                Some(p) => p,
                None => return Ok(false),
            };
            Ok(sget(s, "method") == Some("GET")
                && get(&p, "endpoint") == get(s, "endpoint")
                && get(&p, "beforeNetwork") == get(s, "beforeNetwork"))
        };
        run().unwrap_or(false)
    }

    /// blocksLegacyWrites()
    pub fn blocks_legacy_writes(&self) -> bool {
        true
    }

    /// allowsLegacy(s)
    pub fn allows_legacy(&self, s: &V) -> bool {
        sget(s, "method") == Some("GET")
    }

    /// step() — one scoped read packet + reconciliation, exactly once.
    pub fn step(&self) -> Result<V, String> {
        if self.inner.borrow().completed {
            return Ok(self.status());
        }
        need(!self.inner.borrow().busy, "BUSY")?;
        self.inner.borrow_mut().busy = true;
        let result = self.step_body();
        self.inner.borrow_mut().busy = false;
        result
    }

    fn step_body(&self) -> Result<V, String> {
        self.guard()?;
        let actual_fetch_count = get(&self.order, "fetchStarts")
            .as_array()
            .map(|a| a.len() as i64)
            .unwrap_or(0);
        let mut intent = self.order.as_object().cloned().unwrap_or_default();
        intent.insert("actualFetchCount".into(), json!(actual_fetch_count));
        let adapter = |spec: &V| self.adapter_dispatch(spec);
        let can_read = || -> Result<bool, String> { Ok(self.guard().is_ok()) };
        let collect_deps = BlockedAdmissionReadCollectorDeps {
            api_dispatch: &adapter,
            receipt_of: &*self.deps.receipt_of,
            can_read: &can_read,
            now: &*self.deps.now,
        };
        let after = collect_blocked_admission_account_v1(
            get(&self.state, "scope"),
            &V::Object(intent),
            &collect_deps,
        )?;
        let at = ms_to_iso((self.deps.now)());
        let result = reconcile_physical_guard_admission_v1(&json!({
            "state": self.state,
            "events": self.events,
            "timings": self.timings,
            "diagnostic": get(&self.marker, "diagnostic"),
            "guardFailure": get(&self.marker, "guardFailure"),
            "stop": self.stop,
            "after": after,
            "at": at,
        }))?;
        self.guard()?;
        need(!self.result_file.exists(), "RESULT_EXISTS")?;
        atomic_json(
            &self.result_file,
            &json!({
                "markerHash": sha(&self.marker_bytes),
                "actorAnchor": self.replay_anchor,
                "after": after,
                "result": result,
            }),
        )?;
        self.inner.borrow_mut().completed = true;
        Ok(self.status())
    }

    /// close() — releases the financial owner lease once.
    pub fn close(&self) -> Result<(), String> {
        need(!self.inner.borrow().busy, "BUSY")?;
        let mut g = self.inner.borrow_mut();
        if !g.closed {
            g.closed = true;
            g.financial.release().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
