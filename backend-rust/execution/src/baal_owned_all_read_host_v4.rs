// execution/baal-owned-all-read-host-v4.mjs — one research capture wrapped
// around the V3 scaled live host. Its job expiry is never an order window, and
// BASKET_STOP is neither consulted nor changed: this is a separate scoped job.
//
// Port notes:
// - `permits` WeakMap -> HashMap<usize,V> keyed by before_dispatch guard
//   pointer; entries store {endpoint,beforeNetworkPtr}.
// - The local journal replays every row through reduce() and demands the
//   stored state hash exactly match the re-reduced state (EXACT_REDUCER).
// - async get()/step() become synchronous calls.
use crate::baal_scaled_live_host_v3::{
    baal_scaled_live_v3_identity, open_baal_scaled_live_host_v3, BaalScaledLiveApiV3,
    BaalScaledLiveHostV3, BaalScaledLiveHostV3Deps,
};
use crate::core::{payload_hash, validate_execution_state};
use crate::exact_money_receipts::parse_number_lexemes;
use crate::frozen_paper_graph::collect_frozen_paper_files;
use crate::transport::Guard;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::fixed::instant;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, sha256};
use baal_service_runtime::ledger::{acquire_owner_lease, OwnerLease};
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const T: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
const PREFIX: &str = "BAAL_ALL_READ_";

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
fn iso(n: f64) -> String {
    ms_to_iso(n)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn read(path: &Path) -> Result<V, String> {
    serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn sha(b: &[u8]) -> String {
    sha256(b)
}
fn is_fatal(e: &str) -> bool {
    e.contains(":fatal:") || e.contains("durableRecordingFailed")
}
fn is_reason_code(s: &str) -> bool {
    // /^[A-Z][A-Z0-9_]{0,100}$/
    !s.is_empty()
        && s.len() <= 101
        && s.bytes().next().is_some_and(|b| b.is_ascii_uppercase())
        && s.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
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
    s.strip_prefix("baal-scaled-no-").is_some_and(|t| {
        !t.is_empty()
            && t.len() <= 80
            && t.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}

/// baalOwnedAllReadV4Identity(workspace)
pub fn baal_owned_all_read_v4_identity(workspace: &Path) -> Result<V, String> {
    let files = collect_frozen_paper_files(
        workspace,
        &[
            "execution/baal-owned-all-read-host-v4.mjs".to_string(),
            "execution/scoped-rest-receipts-v3.mjs".to_string(),
        ],
    )?;
    serde_json::to_value(files).map_err(|e| e.to_string())
}

/// The parent's deps are carried verbatim (the JS `{...options, canOperate}`
/// spread) plus the wrapped can_operate the job installs.
pub struct BaalOwnedAllReadHostV4Deps {
    pub parent: BaalScaledLiveHostV3Deps,
}

struct HostV4Inner {
    runtime_dir: PathBuf,
    workspace: PathBuf,
    job_file: PathBuf,
    original_file: PathBuf,
    prior_file: PathBuf,
    dir: PathBuf,
    file: PathBuf,
    job: V,
    job_hash: String,
    code: V,
    original: V,
    lease_hash: String,
    start: i64,
    end: i64,
    endpoints: [String; 2],
    api: Rc<RefCell<BaalScaledLiveApiV3>>,
    get_account_lease: Rc<dyn Fn() -> V>,
    get_other_state: Rc<dyn Fn() -> V>,
    can_operate: Rc<dyn Fn() -> bool>,
    has_read_headroom: Rc<dyn Fn(i64) -> bool>,
    now: Rc<dyn Fn() -> f64>,
    universe: V,
    parent: Option<Rc<BaalScaledLiveHostV3>>,
    parent_receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    lease: Option<OwnerLease>,
    journal: Journal,
    state: Option<V>,
    busy: bool,
    closed: bool,
    failed: bool,
    local_size: u64,
    local_hash: String,
    last_action: String,
    last_failure: Option<String>,
    permits: HashMap<usize, V>,
}

pub struct BaalOwnedAllReadHostV4 {
    inner: Rc<RefCell<HostV4Inner>>,
}

// accountLease() — the pinned shape of the caller's account lease.
fn account_lease(get_account_lease: &Rc<dyn Fn() -> V>) -> Result<V, String> {
    let l = get_account_lease();
    need(
        get(&l, "ownerId").as_str() == Some("Codex")
            && get(&l, "tournamentId").as_str() == Some(T)
            && hex(get(&l, "leaseId")),
        "ACCOUNT_LEASE",
    )?;
    Ok(json!({
        "ownerId": get(&l, "ownerId"),
        "tournamentId": get(&l, "tournamentId"),
        "leaseId": get(&l, "leaseId"),
    }))
}

// fixed() — every externally visible mutation re-verifies the whole chain.
fn fixed_inner(inner: &Rc<RefCell<HostV4Inner>>) -> Result<(), String> {
    {
        let g = inner.borrow();
        need(!g.closed && !g.failed && (g.can_operate)(), "STOPPED")?;
        if let Some(lease) = &g.lease {
            lease.assert_ownership().map_err(|e| e.message)?;
        }
    }
    let get_lease = inner.borrow().get_account_lease.clone();
    need(
        payload_hash(&account_lease(&get_lease)?)? == inner.borrow().lease_hash,
        "LEASE_CHANGED",
    )?;
    let (job_file, original_file, prior_file, workspace, job, job_hash, prior_code_hash, code) = {
        let g = inner.borrow();
        (
            g.job_file.clone(),
            g.original_file.clone(),
            g.prior_file.clone(),
            g.workspace.clone(),
            g.job.clone(),
            g.job_hash.clone(),
            baal_scaled_live_v3_identity(&g.workspace)?,
            g.code.clone(),
        )
    };
    need(
        payload_hash(&read(&job_file)?)? == job_hash
            && payload_hash(&read(&original_file)?)?
                == get(&job, "originalRequestHash").as_str().unwrap_or("")
            && payload_hash(&read(&prior_file)?)?
                == get(&job, "priorUpgradeHash").as_str().unwrap_or("")
            && prior_code_hash == payload_hash(&baal_scaled_live_v3_identity(&workspace)?)?
            && payload_hash(&baal_owned_all_read_v4_identity(&workspace)?)?
                == payload_hash(&code)?,
        "SOURCE_CHANGED",
    )?;
    Ok(())
}

fn allowed(inner: &Rc<RefCell<HostV4Inner>>) -> bool {
    fixed_inner(inner).is_ok()
}

// ownership() — the named owned lot must still be the sole financial intent.
fn ownership(inner: &Rc<RefCell<HostV4Inner>>) -> Result<V, String> {
    let p = inner
        .borrow()
        .parent
        .as_ref()
        .map(|p| p.status())
        .unwrap_or(V::Null);
    let core = validate_execution_state(&(inner.borrow().get_other_state)())?;
    let intents_open = get(&core, "intents")
        .as_object()
        .map(|m| m.values().any(|i| get(i, "status").as_str() != Some("closed")))
        .unwrap_or(false);
    need(
        get(&p, "failed") != &json!(true)
            && get(&p, "closed") != &json!(true)
            && !get(get(&p, "controller"), "intent").is_object()
            && get(get(&p, "budget"), "unknownAcceptance") != &json!(true)
            && get(&core, "tournamentId").as_str() == Some(T)
            && !intents_open,
        "FINANCIAL_INTENT_PRIORITY",
    )?;
    let o = get(get(&p, "controller"), "inventory")
        .get("1073")
        .cloned()
        .unwrap_or(V::Null);
    need(
        get(&o, "marketId").as_str() == Some("384")
            && js_int(get(&o, "quantity")).is_some_and(|q| q > 0)
            && hex(get(&o, "proofHash")),
        "OWNED_LOT_REQUIRED",
    )?;
    Ok(json!({
        "quantity": get(&o, "quantity"),
        "proofHash": get(&o, "proofHash"),
    }))
}

fn read_guard(inner: &Rc<RefCell<HostV4Inner>>) -> Result<V, String> {
    fixed_inner(inner)?;
    let (start, end, api, has_headroom, now) = {
        let g = inner.borrow();
        (
            g.start,
            g.end,
            g.api.clone(),
            g.has_read_headroom.clone(),
            (g.now)(),
        )
    };
    need(now >= start as f64 && now < end as f64, "READ_WINDOW")?;
    {
        let a = api.borrow();
        need(a.manual_review != true && now >= a.cooldown.max(0.0), "SHARED_BACKOFF")?;
    }
    need(has_headroom(2), "HEADROOM")?;
    let own = ownership(inner)?;
    let state = inner.borrow().state.clone();
    if let Some(s) = &state {
        if !get(s, "attempt").is_null() {
            need(
                payload_hash(&own)?
                    == payload_hash(get(get(s, "attempt"), "owned"))?,
                "OWNED_SOURCE_CHANGED",
            )?;
        }
    }
    Ok(own)
}

// parseReceipt(r, index, at) — strict-shape raw receipt check.
fn parse_receipt(r: &V, index: usize, at: &V, attempt_at: &V, endpoints: &[String; 2]) -> Result<V, String> {
    let keys = [
        "fetchInvocationId",
        "method",
        "endpoint",
        "requestBodyHash",
        "requestStartedAt",
        "status",
        "receivedAt",
        "bodyText",
        "sourceBodyHash",
    ];
    need(
        sorted_keys_match(r, &keys)
            && get(r, "method").as_str() == Some("GET")
            && get(r, "endpoint").as_str() == Some(endpoints[index].as_str())
            && get(r, "status") == &json!(200)
            && get(r, "requestBodyHash").is_null()
            && get(r, "fetchInvocationId").as_str().is_some_and(|s| !s.is_empty())
            && get(r, "bodyText").is_string()
            && get(r, "bodyText").as_str().unwrap_or("").len() <= 8 * 1024 * 1024
            && get(r, "sourceBodyHash").as_str()
                == Some(sha(get(r, "bodyText").as_str().unwrap_or("").as_bytes()).as_str()),
        "RAW_RECEIPT",
    )?;
    need(
        instant_v(get(r, "requestStartedAt"))? >= instant_v(attempt_at)?
            && instant_v(get(r, "requestStartedAt"))? <= instant_v(get(r, "receivedAt"))?
            && instant_v(get(r, "receivedAt"))? <= instant_v(at)?
            && instant_v(at)? - instant_v(get(r, "requestStartedAt"))? <= 20000,
        "RECEIPT_CLOCK",
    )?;
    let (d, _lexemes) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
    need(d.is_object(), "RESPONSE_OBJECT")?;
    if index == 0 {
        need(get(&d, "data").is_array(), "COLLATERAL_RESPONSE")?;
        for c in get(&d, "data").as_array().cloned().unwrap_or_default() {
            need(
                c.is_object() && get(&c, "tournamentId").as_str() == Some(T),
                "COLLATERAL_SCOPE",
            )?;
        }
    } else {
        need(
            get(&d, "relationships").is_array() && get(&d, "nodes").is_array(),
            "GRAPH_RESPONSE",
        )?;
    }
    Ok(d)
}

fn genesis(job: &V, job_hash: &str) -> V {
    json!({
        "schemaVersion": 1,
        "jobHash": job_hash,
        "originalRequestHash": get(job, "originalRequestHash"),
        "priorUpgradeHash": get(job, "priorUpgradeHash"),
        "nonce": get(job, "nonce"),
        "attempt": V::Null,
        "result": V::Null,
    })
}

// reduce(previous, event) — the durable attempt/completed/failed machine.
fn reduce(
    job: &V,
    job_hash: &str,
    start: i64,
    end: i64,
    endpoints: &[String; 2],
    previous: Option<&V>,
    event: &V,
) -> Result<V, String> {
    let kind = get(event, "kind").as_str().unwrap_or("");
    if kind == "initialized" {
        need(previous.is_none(), "DUPLICATE_GENESIS")?;
        return Ok(genesis(job, job_hash));
    }
    let previous = previous.ok_or(format!("{PREFIX}MISSING_GENESIS"))?;
    if kind == "attempted" {
        need(
            get(previous, "attempt").is_null()
                && get(previous, "result").is_null()
                && get(event, "maximumReads") == &json!(2)
                && get(event, "nonce") == get(job, "nonce")
                && hex(get(event, "leaseHash"))
                && instant_v(get(event, "at"))? >= start
                && instant_v(get(event, "at"))? < end
                && js_int(get(get(event, "owned"), "quantity")).is_some_and(|q| q > 0)
                && hex(get(get(event, "owned"), "proofHash")),
            "ATTEMPT",
        )?;
        let mut out = previous.clone();
        if let V::Object(m) = &mut out {
            m.insert(
                "attempt".into(),
                json!({
                    "at": get(event, "at"),
                    "nonce": get(job, "nonce"),
                    "maximumReads": 2,
                    "leaseHash": get(event, "leaseHash"),
                    "owned": get(event, "owned"),
                }),
            );
        }
        return Ok(out);
    }
    need(
        !get(previous, "attempt").is_null()
            && get(previous, "result").is_null()
            && instant_v(get(event, "at"))?
                >= instant_v(get(get(previous, "attempt"), "at"))?,
        "RESULT_PHASE",
    )?;
    if kind == "failed" {
        need(
            get(event, "reason")
                .as_str()
                .is_some_and(is_reason_code),
            "FAILURE_CODE",
        )?;
        let mut out = previous.clone();
        if let V::Object(m) = &mut out {
            m.insert(
                "result".into(),
                json!({
                    "kind": "failed",
                    "at": get(event, "at"),
                    "reason": get(event, "reason"),
                }),
            );
        }
        return Ok(out);
    }
    need(
        kind == "completed"
            && get(event, "nonce") == get(job, "nonce")
            && get(event, "receipts").as_array().map(|a| a.len() == 2).unwrap_or(false),
        "COMPLETION",
    )?;
    let receipts = get(event, "receipts").as_array().cloned().unwrap_or_default();
    let attempt_at = get(get(previous, "attempt"), "at").clone();
    let mut docs: Vec<V> = Vec::new();
    for (i, r) in receipts.iter().enumerate() {
        docs.push(parse_receipt(r, i, get(event, "at"), &attempt_at, endpoints)?);
    }
    let ids: Vec<&str> = receipts
        .iter()
        .map(|r| get(r, "fetchInvocationId").as_str().unwrap_or(""))
        .collect();
    need(
        instant_v(get(&receipts[1], "requestStartedAt"))?
            >= instant_v(get(&receipts[0], "receivedAt"))?
            && ids[0] != ids[1]
            && instant_v(get(event, "at"))? < end,
        "SEQUENTIAL_RECEIPTS",
    )?;
    let mut out = previous.clone();
    if let V::Object(m) = &mut out {
        m.insert(
            "result".into(),
            json!({
                "kind": "completed",
                "at": get(event, "at"),
                "packetHash": payload_hash(&json!({
                    "nonce": get(job, "nonce"),
                    "receipts": receipts,
                }))?,
                "sourceHashes": receipts.iter().map(|r| get(r, "sourceBodyHash").clone()).collect::<Vec<V>>(),
                "collateralComponents": get(&docs[0], "data").as_array().map(|a| a.len()).unwrap_or(0),
                "graphNodes": get(&docs[1], "nodes").as_array().map(|a| a.len()).unwrap_or(0),
                "graphRelationships": get(&docs[1], "relationships").as_array().map(|a| a.len()).unwrap_or(0),
            }),
        );
    }
    Ok(out)
}

// append(event) — verify the journal bytes then commit; failure is fatal.
fn append(inner: &Rc<RefCell<HostV4Inner>>, event: &V) -> Result<(), String> {
    fixed_inner(inner)?;
    let (file, local_size, local_hash, job, job_hash, start, end, endpoints) = {
        let g = inner.borrow();
        (
            g.file.clone(),
            g.local_size,
            g.local_hash.clone(),
            g.job.clone(),
            g.job_hash.clone(),
            g.start,
            g.end,
            g.endpoints.clone(),
        )
    };
    let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
    need(
        bytes.len() as u64 == local_size && sha(&bytes) == local_hash,
        "JOURNAL_CHANGED",
    )?;
    let prev_state = inner.borrow().state.clone();
    let next = reduce(
        &job,
        &job_hash,
        start,
        end,
        &endpoints,
        prev_state.as_ref(),
        event,
    )?;
    need(
        serde_json::to_string(&json!({"state": next, "event": event}))
            .map_err(|e| e.to_string())?
            .len()
            <= 16 * 1024 * 1024,
        "PACKET_BOUND",
    )?;
    let commit = {
        let mut g = inner.borrow_mut();
        g.journal.commit(&next, event)
    };
    match commit {
        Ok(_) => {
            let after = std::fs::read(&file).map_err(|e| e.to_string())?;
            need(
                after.len() >= bytes.len() && after[..bytes.len()] == bytes[..],
                "PREFIX_CHANGED",
            )?;
            let mut g = inner.borrow_mut();
            g.state = Some(next);
            g.local_size = after.len() as u64;
            g.local_hash = sha(&after);
            Ok(())
        }
        Err(e) => {
            inner.borrow_mut().failed = true;
            Err(format!("{e}:fatal:durableRecordingFailed"))
        }
    }
}

fn status(inner: &Rc<RefCell<HostV4Inner>>) -> V {
    let (parent_status, state, job, job_hash, original, last_action, last_failure, closed, failed) = {
        let g = inner.borrow();
        (
            g.parent.as_ref().map(|p| p.status()),
            g.state.clone(),
            g.job.clone(),
            g.job_hash.clone(),
            g.original.clone(),
            g.last_action.clone(),
            g.last_failure.clone(),
            g.closed,
            g.failed,
        )
    };
    let mut out = parent_status.clone().unwrap_or(json!({}));
    if let V::Object(m) = &mut out {
        m.insert("hostVersion".into(), json!(4));
        m.insert("financialHostVersion".into(), json!(3));
        m.insert(
            "closed".into(),
            json!(closed
                || parent_status
                    .as_ref()
                    .map(|p| get(p, "closed") == &json!(true))
                    .unwrap_or(false)),
        );
        m.insert(
            "failed".into(),
            json!(failed
                || parent_status
                    .as_ref()
                    .map(|p| get(p, "failed") == &json!(true))
                    .unwrap_or(false)),
        );
        m.insert(
            "ownedAllRead".into(),
            json!({
                "schemaVersion": 1,
                "id": get(&job, "id"),
                "jobHash": job_hash,
                "marketId": "384",
                "exchangeId": "1073",
                "tournamentId": T,
                "startedAt": get(&job, "startedAt"),
                "expiresAt": get(&job, "expiresAt"),
                "originalEntryExpiresAt": get(&original, "entryExpiresAt"),
                "attempted": state.as_ref().map(|s| !get(s, "attempt").is_null()).unwrap_or(false),
                "completed": state.as_ref().map(|s| get(get(s, "result"), "kind").as_str() == Some("completed")).unwrap_or(false),
                "result": state.as_ref().map(|s| get(s, "result").clone()).unwrap_or(V::Null),
                "lastAction": last_action,
                "lastFailure": last_failure.map(V::from).unwrap_or(V::Null),
                "maximumReads": 2,
                "graphDepth": 4,
                "readOnly": true,
                "executionAuthority": false,
                "originalWindowRenewed": false,
                "additionalAccountClients": 0,
            }),
        );
    }
    out
}

fn publish(inner: &Rc<RefCell<HostV4Inner>>, action: &str) -> Result<V, String> {
    inner.borrow_mut().last_action = action.to_string();
    let file = inner.borrow().runtime_dir.join("baal-owned-all-read-stats.json");
    let s = status(inner);
    let owned = get(&s, "ownedAllRead").clone();
    let mut packet = serde_json::Map::new();
    packet.insert("observedAt".into(), json!(iso((inner.borrow().now)())));
    if let V::Object(m) = &owned {
        for (k, v) in m {
            packet.insert(k.clone(), v.clone());
        }
    }
    match atomic_json(&file, &V::Object(packet)) {
        Ok(()) => Ok(s),
        Err(e) => {
            inner.borrow_mut().failed = true;
            Err(format!("{e}:fatal:durableRecordingFailed"))
        }
    }
}

fn cleanup(inner: &Rc<RefCell<HostV4Inner>>) -> Result<(), String> {
    let mut error: Option<String> = None;
    let parent = inner.borrow().parent.clone();
    if let Some(p) = parent {
        if let Err(e) = p.close() {
            if error.is_none() {
                error = Some(e);
            }
        }
    }
    let mut lease = inner.borrow_mut().lease.take();
    if let Some(l) = lease.as_mut() {
        if let Err(e) = l.release() {
            if error.is_none() {
                error = Some(e.message);
            }
        }
    }
    match error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

// get(index) — one scoped read under the shared permit discipline.
fn get_endpoint(inner: &Rc<RefCell<HostV4Inner>>, index: usize) -> Result<V, String> {
    read_guard(inner)?;
    let (endpoint, api) = {
        let g = inner.borrow();
        (g.endpoints[index].clone(), g.api.clone())
    };
    let inner_d = inner.clone();
    let before_dispatch: &Guard<'_> = &move |_p: &V, _s: &V| {
        read_guard(&inner_d).map(|_| true).map_err(|e| {
            crate::transport::OrderTransportError {
                code: e,
                status: None,
                api_code: None,
                outcome: "blocked".into(),
                may_have_executed: false,
                retry_same_request_only: false,
                retry_after_ms: 0.0,
                semantic_mismatch: false,
            }
        })
    };
    let inner_n = inner.clone();
    let before_network: &Guard<'_> = &move |_p: &V, _s: &V| {
        read_guard(&inner_n).map(|_| true).map_err(|e| {
            crate::transport::OrderTransportError {
                code: e,
                status: None,
                api_code: None,
                outcome: "blocked".into(),
                may_have_executed: false,
                retry_same_request_only: false,
                retry_after_ms: 0.0,
                semantic_mismatch: false,
            }
        })
    };
    let bd_ptr = before_dispatch as *const Guard as *const () as usize;
    let bn_ptr = before_network as *const Guard as *const () as usize;
    inner.borrow_mut().permits.insert(
        bd_ptr,
        json!({"endpoint": endpoint, "beforeNetworkPtr": bn_ptr as u64}),
    );
    let result = (|| -> Result<V, String> {
        let spec = json!({"method": "GET", "endpoint": endpoint});
        let dispatch = api.borrow().dispatch.clone();
        let response = dispatch(&spec, Some(before_dispatch), Some(before_network))
            .map_err(|e| e.message)?;
        read_guard(inner)?;
        need(get(&response, "status") == &json!(200), "HTTP_STATUS")?;
        let receipt = (inner.borrow().deps_receipt_of())(&response, &json!({"method": "GET"}))?;
        let state = inner.borrow().state.clone();
        let attempt_at = get(get(&state.unwrap_or(V::Null), "attempt"), "at").clone();
        let at = json!(iso((inner.borrow().now)()));
        let endpoints = inner.borrow().endpoints.clone();
        parse_receipt(&receipt, index, &at, &attempt_at, &endpoints)?;
        Ok(receipt)
    })();
    inner.borrow_mut().permits.remove(&bd_ptr);
    result
}

impl HostV4Inner {
    fn deps_receipt_of(&self) -> Rc<dyn Fn(&V, &V) -> Result<V, String>> {
        self.parent_receipt_of.clone()
    }
}

impl BaalOwnedAllReadHostV4 {
    /// status()
    pub fn status(&self) -> V {
        status(&self.inner)
    }

    /// getReadHint — passthrough to the parent host.
    pub fn get_read_hint(&self) -> Option<V> {
        self.inner
            .borrow()
            .parent
            .as_ref()
            .and_then(|p| p.get_read_hint())
    }

    pub fn blocks_legacy_writes(&self) -> bool {
        self.inner
            .borrow()
            .parent
            .as_ref()
            .map(|p| p.blocks_legacy_writes())
            .unwrap_or(true)
    }

    /// allowsLegacy(spec) — passthrough.
    pub fn allows_legacy(&self, spec: &V) -> bool {
        self.inner
            .borrow()
            .parent
            .as_ref()
            .map(|p| p.allows_legacy(spec))
            .unwrap_or(false)
    }

    /// onFetchStart(meta) — passthrough.
    pub fn on_fetch_start(&self, meta: &V) -> Result<bool, String> {
        self.inner
            .borrow()
            .parent
            .as_ref()
            .ok_or("BAAL_ALL_READ_PARENT_UNAVAILABLE".to_string())?
            .on_fetch_start(meta)
    }

    /// authorizes(spec)
    pub fn authorizes(
        &self,
        spec: &V,
        before_dispatch: Option<&Guard<'_>>,
        before_network: Option<&Guard<'_>>,
    ) -> bool {
        if !allowed(&self.inner) {
            return false;
        }
        {
            let g = self.inner.borrow();
            if let Some(p) = &g.parent {
                if p.authorizes(spec, before_dispatch, before_network) {
                    return true;
                }
            }
        }
        let bd_ptr = before_dispatch
            .map(|g| g as *const Guard as *const () as usize)
            .unwrap_or(0);
        let bn_ptr = before_network
            .map(|g| g as *const Guard as *const () as usize)
            .unwrap_or(0);
        let p = self.inner.borrow().permits.get(&bd_ptr).cloned();
        p.is_some()
            && get(spec, "method").as_str() == Some("GET")
            && get(spec, "endpoint") == get(p.as_ref().unwrap(), "endpoint")
            && get(p.as_ref().unwrap(), "beforeNetworkPtr") == &json!(bn_ptr as u64)
            && spec.get("body").is_none()
            && spec.get("bodyJson").is_none()
            && spec.get("headers").is_none()
    }

    /// step()
    pub fn step(&self) -> Result<V, String> {
        {
            let mut g = self.inner.borrow_mut();
            need(!g.busy && !g.closed, "BUSY_OR_CLOSED")?;
            g.busy = true;
        }
        let inner = self.inner.clone();
        let out = (|| -> Result<V, String> {
            {
                let parent = inner.borrow().parent.clone();
                parent
                    .ok_or("BAAL_ALL_READ_PARENT_UNAVAILABLE".to_string())?
                    .step()?;
            }
            let state = inner.borrow().state.clone().unwrap_or(V::Null);
            if !get(&state, "result").is_null() {
                return publish(&inner, "finished_no_recapture");
            }
            if !get(&state, "attempt").is_null() {
                return publish(&inner, "interrupted_attempt_no_recapture");
            }
            let (start, end, now) = {
                let g = inner.borrow();
                (g.start, g.end, (g.now)())
            };
            if (now as i64) < start || (now as i64) >= end {
                return publish(&inner, "read_window_closed");
            }
            if let Err(e) = read_guard(&inner) {
                inner.borrow_mut().last_failure = Some(if is_reason_code(&e) {
                    e
                } else {
                    "BAAL_ALL_READ_NOT_READY".to_string()
                });
                return publish(&inner, "waiting_existing_financial_host");
            }
            let own = ownership(&inner)?;
            let lease_hash = inner.borrow().lease_hash.clone();
            let job = inner.borrow().job.clone();
            append(
                &inner,
                &json!({
                    "kind": "attempted",
                    "at": iso(now),
                    "nonce": get(&job, "nonce"),
                    "maximumReads": 2,
                    "leaseHash": lease_hash,
                    "owned": own,
                }),
            )?;
            let attempt = (|| -> Result<V, String> {
                let receipts = vec![get_endpoint(&inner, 0)?, get_endpoint(&inner, 1)?];
                read_guard(&inner)?;
                append(
                    &inner,
                    &json!({
                        "kind": "completed",
                        "at": iso((inner.borrow().now)()),
                        "nonce": get(&job, "nonce"),
                        "receipts": receipts,
                    }),
                )?;
                inner.borrow_mut().last_failure = None;
                publish(&inner, "read_only_packet_completed")
            })();
            match attempt {
                Ok(v) => Ok(v),
                Err(e) => {
                    let failure = if is_reason_code(&e) {
                        e.clone()
                    } else {
                        "BAAL_ALL_READ_CAPTURE_FAILED".to_string()
                    };
                    inner.borrow_mut().last_failure = Some(failure.clone());
                    if is_fatal(&e)
                        || ["EIO", "ENOSPC", "EROFS", "EPERM", "EACCES", "EBUSY"]
                            .contains(&e.as_str())
                    {
                        inner.borrow_mut().failed = true;
                        return Err(format!("{e}:fatal:durableRecordingFailed"));
                    }
                    if allowed(&inner) {
                        append(
                            &inner,
                            &json!({
                                "kind": "failed",
                                "at": iso((inner.borrow().now)()),
                                "reason": failure,
                            }),
                        )?;
                    }
                    publish(&inner, "attempt_consumed_no_retry")
                }
            }
        })();
        inner.borrow_mut().busy = false;
        out
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
        cleanup(&self.inner)
    }
}

/// openBaalOwnedAllReadHostV4(options) — None when the job file is absent.
pub fn open_baal_owned_all_read_host_v4(
    runtime_dir: &Path,
    workspace: &Path,
    universe: &V,
    api: Rc<RefCell<BaalScaledLiveApiV3>>,
    deps: BaalOwnedAllReadHostV4Deps,
) -> Result<Option<BaalOwnedAllReadHostV4>, String> {
    let job_file = runtime_dir.join("baal-owned-all-read-request.json");
    if !job_file.exists() {
        return Ok(None);
    }
    need(runtime_dir.is_absolute() && workspace.is_absolute(), "DEPENDENCIES")?;
    let job = read(&job_file)?;
    let job_hash = payload_hash(&job)?;
    let code = baal_owned_all_read_v4_identity(workspace)?;
    let keys = [
        "schemaVersion", "kind", "id", "nonce", "ownerId", "runId",
        "tournamentId", "marketId", "exchangeId", "originalRequestHash",
        "priorUpgradeHash", "codeIdentity", "startedAt", "expiresAt",
    ];
    need(
        sorted_keys_match(&job, &keys)
            && get(&job, "schemaVersion") == &json!(1)
            && get(&job, "kind").as_str() == Some("baal_owned_all_read_once_v4")
            && get(&job, "runId").as_str().is_some_and(run_id_ok)
            && get(&job, "id").as_str().is_some_and(|s| {
                !s.is_empty()
                    && s.len() <= 64
                    && s.bytes().next().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                    && s.bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
            && get(&job, "nonce").as_str().is_some_and(|s| {
                (32..=64).contains(&s.len())
                    && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            })
            && get(&job, "ownerId").as_str() == Some("Codex")
            && get(&job, "tournamentId").as_str() == Some(T)
            && get(&job, "marketId").as_str() == Some("384")
            && get(&job, "exchangeId").as_str() == Some("1073")
            && hex(get(&job, "originalRequestHash"))
            && hex(get(&job, "priorUpgradeHash"))
            && payload_hash(get(&job, "codeIdentity"))? == payload_hash(&code)?,
        "JOB",
    )?;
    let start = instant_v(get(&job, "startedAt"))?;
    let end = instant_v(get(&job, "expiresAt"))?;
    need(
        start <= (deps.parent.now)() as i64 && end > start && end - start <= 1_800_000,
        "WINDOW",
    )?;
    let original_file = runtime_dir.join("baal-scaled-live-request.json");
    let prior_file = runtime_dir.join("baal-scaled-host-upgrade-v3.json");
    let original = read(&original_file)?;
    let prior = read(&prior_file)?;
    let prior_code_hash = payload_hash(&baal_scaled_live_v3_identity(workspace)?)?;
    need(
        payload_hash(&original)? == get(&job, "originalRequestHash").as_str().unwrap_or("")
            && payload_hash(&prior)? == get(&job, "priorUpgradeHash").as_str().unwrap_or("")
            && payload_hash(get(&prior, "codeIdentity"))? == prior_code_hash
            && get(&original, "ownerId") == get(&job, "ownerId")
            && get(&original, "runId") == get(&job, "runId")
            && get(&original, "tournamentId").as_str() == Some(T)
            && get(&prior, "runId") == get(&job, "runId")
            && get(&prior, "originalRequestHash") == get(&job, "originalRequestHash")
            && get(&prior, "entryExpiresAt") == get(&original, "entryExpiresAt"),
        "ORIGINAL_HOST_BINDING",
    )?;
    need(
        get(universe, "tournamentId").as_str() == Some(T)
            && get(universe, "hash") == get(&original, "universeHash")
            && get(universe, "exchanges")
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter(|e| {
                            get(e, "marketId").as_str() == Some("384")
                                && get(e, "exchangeId").as_str() == Some("1073")
                                && get(e, "option").as_str() == Some("YES")
                        })
                        .count()
                        == 1
                })
                .unwrap_or(false)
            && get(&original, "allowedMarkets")
                .as_array()
                .map(|a| a.iter().any(|x| x.as_str() == Some("384")))
                .unwrap_or(false),
        "OWNED_MARKET_SCOPE",
    )?;
    let endpoints = [
        format!("/portfolio/collateral?tournamentId={T}"),
        format!("/relationships/graph?marketId=384&depth=4&tournamentId={T}"),
    ];
    let dir = runtime_dir
        .join("baal-scaled-live-runs")
        .join(get(&job, "runId").as_str().unwrap_or(""))
        .join("owned-all-read-v4")
        .join(get(&job, "id").as_str().unwrap_or(""));
    let file = dir.join("owned-all-read-journal.jsonl");
    let lease_hash = payload_hash(&account_lease(&deps.parent.get_account_lease)?)?;
    let parent_deps = deps.parent;
    let get_account_lease = parent_deps.get_account_lease.clone();
    let get_other_state = parent_deps.get_other_state.clone();
    let has_read_headroom = parent_deps.has_read_headroom.clone();
    let now = parent_deps.now.clone();
    let can_operate = parent_deps.can_operate.clone();
    let parent_receipt_of = parent_deps.receipt_of.clone();
    let open_result: Result<BaalOwnedAllReadHostV4, String> = (|| {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let inner = Rc::new(RefCell::new(HostV4Inner {
            runtime_dir: runtime_dir.to_path_buf(),
            workspace: workspace.to_path_buf(),
            job_file: job_file.clone(),
            original_file: original_file.clone(),
            prior_file: prior_file.clone(),
            dir: dir.clone(),
            file: file.clone(),
            job: job.clone(),
            job_hash: job_hash.clone(),
            code,
            original: original.clone(),
            lease_hash: lease_hash.clone(),
            start,
            end,
            endpoints: endpoints.clone(),
            api,
            get_account_lease: get_account_lease.clone(),
            get_other_state,
            can_operate,
            has_read_headroom,
            now: now.clone(),
            universe: universe.clone(),
            parent: None,
            lease: None,
            journal: Journal::open(&file)?,
            state: None,
            busy: false,
            closed: false,
            failed: false,
            local_size: 0,
            local_hash: String::new(),
            last_action: "initialized".to_string(),
            last_failure: None,
            permits: HashMap::new(),
            parent_receipt_of,
        }));
        // fixed() before lease exists: STOPPED + lease + source checks only.
        fixed_inner(&inner)?;
        let lease = acquire_owner_lease(
            &dir,
            get(&job, "ownerId").as_str().unwrap_or(""),
            get(&job, "runId").as_str().unwrap_or(""),
            "baal-owned-all-read-v4",
            now(),
        )
        .map_err(|e| e.message)?;
        inner.borrow_mut().lease = Some(lease);
        if !file.exists() {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&file)
                .map_err(|e| e.to_string())?;
        }
        let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
        need(
            bytes.len() <= 20 * 1024 * 1024
                && (bytes.is_empty() || *bytes.last().unwrap() == 10),
            "JOURNAL_BOUND",
        )?;
        {
            let mut g = inner.borrow_mut();
            g.journal = Journal::open(&file)?;
        }
        for line in String::from_utf8_lossy(&bytes).split('\n').filter(|l| !l.is_empty()) {
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            let prev = inner.borrow().state.clone();
            let next = reduce(
                &job,
                &job_hash,
                start,
                end,
                &endpoints,
                prev.as_ref(),
                get(&row, "event"),
            )?;
            need(
                payload_hash(get(&row, "state"))? == payload_hash(&next)?,
                "EXACT_REDUCER",
            )?;
            inner.borrow_mut().state = Some(next);
        }
        {
            let mut g = inner.borrow_mut();
            g.local_size = bytes.len() as u64;
            g.local_hash = sha(&bytes);
        }
        if inner.borrow().state.is_none() {
            append(&inner, &json!({"kind": "initialized", "at": iso(now())}))?;
        }
        // parent = openBaalScaledLiveHostV3({...options, canOperate: allowed})
        let inner_a = inner.clone();
        let v3_deps = BaalScaledLiveHostV3Deps {
            receipt_of: parent_deps.receipt_of.clone(),
            get_account_lease: parent_deps.get_account_lease.clone(),
            get_other_state: parent_deps.get_other_state.clone(),
            get_legacy_risk: parent_deps.get_legacy_risk.clone(),
            get_pair_read_hints: parent_deps.get_pair_read_hints.clone(),
            get_read_hint: parent_deps.get_read_hint.clone(),
            can_operate: Rc::new(move || allowed(&inner_a)),
            has_read_headroom: parent_deps.has_read_headroom.clone(),
            now: parent_deps.now.clone(),
        };
        let parent = open_baal_scaled_live_host_v3(
            runtime_dir,
            workspace,
            universe,
            inner.borrow().api.clone(),
            v3_deps,
        )?;
        need(parent.is_some(), "PARENT_REQUIRED")?;
        inner.borrow_mut().parent = parent.map(Rc::new);
        publish(
            &inner,
            if get(&inner.borrow().state.clone().unwrap_or(V::Null), "result").is_object() {
                "restored_result_no_recapture"
            } else if !get(&inner.borrow().state.clone().unwrap_or(V::Null), "attempt").is_null() {
                "interrupted_attempt_no_recapture"
            } else {
                "waiting_read_capture"
            },
        )?;
        Ok(BaalOwnedAllReadHostV4 { inner })
    })();
    match open_result {
        Ok(h) => Ok(Some(h)),
        Err(e) => Err(e),
    }
}
