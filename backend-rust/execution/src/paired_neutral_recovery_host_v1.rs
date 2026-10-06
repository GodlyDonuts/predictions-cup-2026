// execution/paired-neutral-recovery-host-v1.mjs — an explicit reconciliation
// revision, not a replacement trading host. It reuses the original actor,
// funding registry, account owner and one API. It cannot issue POST/DELETE,
// renew a window, select a new pair, or retry a key.
use crate::core::{decimal_micros, format_micros, payload_hash};
use crate::paired_live_host::{PairedLiveApi, paired_live_identity};
use crate::paired_live_host_v2::{paired_live_v2_identity, select_paired_live_candidate};
use crate::paired_rest_controller_v3::WireDispatchError as V3DispatchError;
use crate::paired_rest_controller_v3::{PairedRestControllerV3, PairedRestControllerDeps, open_paired_rest_controller_v3};
use crate::paired_rest_evidence_bridge::{
    EvidenceBridgeDeps, PairedRestEvidenceBridge, create_paired_rest_evidence_bridge,
};
use crate::paired_shared_funding::{PairedSharedFunding, PairedSharedFundingDeps, open_paired_shared_funding};
use crate::transport::{Guard as TGuard, OrderTransportError};
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::fixed::instant;
use baal_service_runtime::journal::Journal;
use baal_service_runtime::ledger::acquire_owner_lease;
use serde_json::Value as V;
use serde_json::json;
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const PREFIX: &str = "PAIRED_NEUTRAL_RECOVERY_";
const TID: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok {
        return Err(format!("{PREFIX}{code}"));
    }
    Ok(())
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn iso(n: f64) -> String {
    baal_service_runtime::contract::ms_to_iso(n)
}
fn read(p: &Path) -> Result<V, String> {
    serde_json::from_str(&fs::read_to_string(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn hex(x: &V) -> bool {
    x.as_str()
        .map(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()))
        .unwrap_or(false)
}
fn run_id_ok(s: &str) -> bool {
    s.strip_prefix("sig-pilot-paired-").is_some_and(|t| {
        !t.is_empty()
            && t.len() <= 60
            && t.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn is_all_caps_code(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false)
        && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

// pairedNeutralRecoveryIdentity(workspace)
pub fn paired_neutral_recovery_identity(workspace: &Path) -> Result<V, String> {
    crate::frozen::collect_frozen_paper_files(
        workspace,
        &[
            "execution/paired-neutral-recovery-host-v1.mjs",
            "execution/scoped-rest-receipts.mjs",
        ],
    )
}

#[allow(clippy::type_complexity)]
pub struct PairedNeutralRecoveryDeps {
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub get_account_lease: Rc<dyn Fn() -> V>,
    pub get_other_state: Rc<dyn Fn() -> V>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> bool>,
    pub now: Rc<dyn Fn() -> f64>,
    pub workspace: PathBuf,
}

pub struct PairedNeutralRecoveryHost {
    inner: Rc<RefCell<Inner>>,
}

struct Inner {
    descriptor: V,
    descriptor_hash: String,
    descriptor_file: PathBuf,
    outer_file: PathBuf,
    legacy_file: PathBuf,
    authority_file: PathBuf,
    child_request_file: PathBuf,
    child_authority_file: PathBuf,
    activation_file: PathBuf,
    capture_file: PathBuf,
    selection_file: PathBuf,
    selection_bytes: Vec<u8>,
    runtime_dir: PathBuf,
    controller_dir: PathBuf,
    registry_dir: PathBuf,
    scope: V,
    outer: V,
    selection: V,
    code: V,
    api: Rc<RefCell<PairedLiveApi>>,
    deps: PairedNeutralRecoveryDeps,
    selector_lease: baal_service_runtime::ledger::OwnerLease,
    controller: Option<Rc<PairedRestControllerV3>>,
    registry: Option<PairedSharedFunding>,
    bridge: Option<Rc<PairedRestEvidenceBridge>>,
    last_funding: Option<V>,
    closed: bool,
    busy: bool,
    failed: bool,
    last_failure: Option<String>,
    last_action: String,
}

fn fixed(inner: &Rc<RefCell<Inner>>) -> Result<(), String> {
    let (
        legacy_file,
        descriptor_file,
        descriptor_hash,
        workspace,
        code,
        files,
        selection_file,
        selection_bytes,
    ) = {
        let g = inner.borrow();
        g.selector_lease
            .assert_ownership()
            .map_err(|e| format!("{PREFIX}{}", e.message))?;
        let capture_hash = payload_hash(&read(&g.capture_file)?).unwrap_or_default();
        (
            g.legacy_file.clone(),
            g.descriptor_file.clone(),
            g.descriptor_hash.clone(),
            g.deps.workspace.clone(),
            payload_hash(&g.code)?,
            vec![
                (g.outer_file.clone(), get(&g.descriptor, "outerRequestHash").as_str().unwrap_or("").to_string()),
                (g.authority_file.clone(), get(&g.descriptor, "authorityHash").as_str().unwrap_or("").to_string()),
                (g.child_request_file.clone(), get(&g.descriptor, "childRequestHash").as_str().unwrap_or("").to_string()),
                (g.child_authority_file.clone(), get(&g.descriptor, "authorityHash").as_str().unwrap_or("").to_string()),
                (g.activation_file.clone(), get(&g.descriptor, "activationHash").as_str().unwrap_or("").to_string()),
                (g.capture_file.clone(), capture_hash),
            ],
            g.selection_file.clone(),
            g.selection_bytes.clone(),
        )
    };
    need(
        !legacy_file.exists()
            && payload_hash(&read(&descriptor_file)?)? == descriptor_hash
            && payload_hash(&paired_neutral_recovery_identity(&workspace)?)? == code,
        "REVISION_CHANGED",
    )?;
    for (file, hash) in files {
        need(payload_hash(&read(&file)?)? == hash, "ORIGINAL_SOURCE_CHANGED")?;
    }
    need(
        fs::read(&selection_file).map_err(|e| e.to_string())? == selection_bytes,
        "SELECTION_CHANGED",
    )?;
    Ok(())
}

fn allowed(inner: &Rc<RefCell<Inner>>) -> bool {
    (|| -> Result<bool, String> {
        {
            let g = inner.borrow();
            if g.closed || g.failed || !(g.deps.can_operate)() {
                return Ok(false);
            }
        }
        fixed(inner)?;
        Ok(true)
    })()
    .unwrap_or(false)
}

fn global_risk(inner: &Rc<RefCell<Inner>>) -> V {
    let g = inner.borrow();
    let core = (g.deps.get_other_state)();
    let pair = g.controller.as_ref().map(|c| c.status()).unwrap_or(V::Null);
    let mut unresolved: Vec<V> = get(&core, "intents")
        .as_object()
        .map(|m| {
            m.values()
                .filter(|i| get(i, "status").as_str() != Some("closed"))
                .map(|i| get(i, "intentId").clone())
                .collect()
        })
        .unwrap_or_default();
    for o in get(&pair, "orders").as_array().cloned().unwrap_or_default() {
        if get(&o, "phase").as_str() != Some("reconciled") {
            unresolved.push(get(&o, "intentId").clone());
        }
    }
    json!({
        "ownerId": "Codex",
        "runId": get(&g.descriptor, "runId"),
        "tournamentId": TID,
        "complete": true,
        "soleCollector": true,
        "stopped": !allowed(inner),
        "at": iso((g.deps.now)()),
        "unresolvedIntentIds": unresolved,
        "sourceHash": payload_hash(&json!({
            "core": core,
            "paired": pair,
            "accountLease": (g.deps.get_account_lease)(),
        })).unwrap_or_default(),
    })
}

fn host_status(inner: &Rc<RefCell<Inner>>) -> V {
    let g = inner.borrow();
    let s = g.controller.as_ref().map(|c| c.status());
    json!({
        "hostVersion": 3,
        "mode": "paired_neutral_ack_recovery_only",
        "runId": get(&g.descriptor, "runId"),
        "requestHash": get(&g.descriptor, "outerRequestHash"),
        "recoveryDescriptorHash": g.descriptor_hash,
        "authorityHash": get(&g.descriptor, "authorityHash"),
        "selectionHash": get(&g.descriptor, "selectionHash"),
        "selectedMarkets": get(&g.scope, "markets"),
        "entryExpiresAt": get(&g.outer, "entryExpiresAt"),
        "reviewExpiresAt": get(&g.selection, "reviewExpiresAt"),
        "initialized": g.controller.is_some(),
        "phase": s.as_ref().map(|x| get(x, "phase").clone()).unwrap_or(json!("recovery_unavailable")),
        "orders": s.as_ref().map(|x| get(x, "orders").as_array().map(|a| a.len()).unwrap_or(0)).unwrap_or(0),
        "blocksLegacyWrites": true,
        "entryAllowed": false,
        "writeCapability": false,
        "lastAction": g.last_action,
        "lastFailure": g.last_failure.clone().map(V::from).unwrap_or(V::Null),
        "failed": g.failed || s.as_ref().map(|x| get(x, "failed").as_bool().unwrap_or(false)).unwrap_or(false),
        "closed": g.closed,
        "additionalPersistentProcesses": 0,
        "additionalAccountClients": 0,
    })
}

fn publish(inner: &Rc<RefCell<Inner>>, action: &str) -> Result<V, String> {
    inner.borrow_mut().last_action = action.to_string();
    let (s, view, stats_file) = {
        let g = inner.borrow();
        let s = g.controller.as_ref().map(|c| c.status());
        let status = host_status(inner);
        let orders: Vec<V> = s
            .as_ref()
            .map(|x| get(x, "orders").as_array().cloned().unwrap_or_default())
            .unwrap_or_default()
            .iter()
            .map(|o| {
                json!({
                    "intentId": get(o, "intentId"),
                    "orderId": get(o, "orderId"),
                    "marketId": get(o, "marketId"),
                    "exchangeId": get(o, "exchangeId"),
                    "phase": get(o, "phase"),
                    "limit": get(&get(o, "body"), "price"),
                    "quantity": 1,
                    "cancelPending": get(o, "cancelPending"),
                    "proofHash": get(o, "proofHash"),
                })
            })
            .collect();
        let mut v = serde_json::Map::new();
        v.insert("schemaVersion".into(), json!(3));
        for (k, val) in status.as_object().unwrap().iter() {
            v.insert(k.clone(), val.clone());
        }
        v.insert("observedAt".into(), json!(iso((g.deps.now)())));
        v.insert("action".into(), json!(action));
        v.insert("orders".into(), V::Array(orders));
        v.insert("orderCount".into(), s.as_ref().map(|x| get(x, "orders").as_array().map(|a| a.len()).unwrap_or(0)).map(|x| json!(x)).unwrap_or(json!(0)));
        v.insert("owned".into(), s.as_ref().map(|x| get(x, "owned").clone()).unwrap_or(json!([])));
        v.insert("funding".into(), s.as_ref().map(|x| get(x, "funding").clone()).unwrap_or(V::Null));
        v.insert("realizedPnl".into(), json!("0.000000"));
        v.insert("ordinaryMinimumPayout".into(), V::Null);
        v.insert("ordinarySurplus".into(), V::Null);
        v.insert("ordinaryResolutionOnly".into(), json!(true));
        v.insert(
            "maximumExceptionalLoss".into(),
            s.as_ref()
                .and_then(|x| x.get("funding"))
                .map(|f| get(f, "ownedPrincipal").clone())
                .unwrap_or(json!("0.000000")),
        );
        v.insert("profitabilityEstablished".into(), json!(false));
        (s, V::Object(v), g.runtime_dir.join("paired-live-stats.json"))
    };
    let _ = s;
    match atomic_json(&stats_file, &view) {
        Ok(()) => Ok(host_status(inner)),
        Err(e) => {
            inner.borrow_mut().failed = true;
            Err(format!("{e}:fatal:durableRecordingFailed"))
        }
    }
}

fn cleanup(inner: &Rc<RefCell<Inner>>) -> Result<(), String> {
    let mut error: Option<String> = None;
    {
        let g = inner.borrow();
        if let Some(b) = &g.bridge {
            if let Err(e) = b.close() {
                if error.is_none() {
                    error = Some(e);
                }
            }
        }
        if let Some(r) = &g.registry {
            r.close();
        }
        if let Some(c) = &g.controller {
            if let Err(e) = c.close() {
                if error.is_none() {
                    error = Some(e);
                }
            }
        }
    }
    if let Err(e) = inner.borrow_mut().selector_lease.release().map_err(|e| format!("{PREFIX}{}", e.message)) {
        if error.is_none() {
            error = Some(e);
        }
    }
    {
        let mut g = inner.borrow_mut();
        g.bridge = None;
        g.registry = None;
        g.controller = None;
    }
    match error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn is_durable_error(code: &str) -> bool {
    ["EIO", "ENOSPC", "EROFS", "EPERM", "EACCES", "EBUSY"].iter().any(|c| code.contains(c))
}

// openPairedNeutralRecoveryHost({runtimeDir,workspace,universe,api,receiptOf,getAccountLease,getOtherState,canOperate,hasReadHeadroom,now})
#[allow(clippy::too_many_arguments)]
pub fn open_paired_neutral_recovery_host(
    runtime_dir: &Path,
    universe: &V,
    api: Rc<RefCell<PairedLiveApi>>,
    deps: PairedNeutralRecoveryDeps,
) -> Result<Option<PairedNeutralRecoveryHost>, String> {
    let descriptor_file = runtime_dir.join("paired-neutral-ack-recovery.json");
    if !descriptor_file.exists() {
        return Ok(None);
    }
    need(
        runtime_dir.is_absolute() && deps.workspace.is_absolute(),
        "DEPENDENCIES",
    )?;
    need((deps.can_operate)(), "STOPPED")?;
    let descriptor = read(&descriptor_file)?;
    let descriptor_hash = payload_hash(&descriptor)?;
    let code = paired_neutral_recovery_identity(&deps.workspace)?;
    need(
        get(&descriptor, "schemaVersion") == &json!(1)
            && get(&descriptor, "kind").as_str() == Some("paired_neutral_ack_recovery_only")
            && get(&descriptor, "ownerId").as_str() == Some("Codex")
            && get(&descriptor, "tournamentId").as_str() == Some(TID)
            && get(&descriptor, "runId").as_str().map(run_id_ok).unwrap_or(false)
            && ["outerRequestHash", "selectionHash", "childRequestHash", "authorityHash", "activationHash"]
                .iter()
                .all(|k| hex(get(&descriptor, k)))
            && payload_hash(get(&descriptor, "codeIdentity"))? == payload_hash(&code)?,
        "DESCRIPTOR",
    )?;
    let outer_file = runtime_dir.join("paired-live-v2-request.json");
    let legacy_file = runtime_dir.join("paired-live-request.json");
    let authority_file = runtime_dir.join("paired-live-authority.json");
    need(!legacy_file.exists(), "LEGACY_REQUEST_PRESENT")?;
    let outer = read(&outer_file)?;
    let authority = read(&authority_file)?;
    let run_id = get(&descriptor, "runId").as_str().unwrap_or("").to_string();
    need(
        payload_hash(&outer)? == get(&descriptor, "outerRequestHash").as_str().unwrap_or("")
            && get(&outer, "runId").as_str() == Some(run_id.as_str())
            && get(&outer, "authorityHash") == get(&descriptor, "authorityHash")
            && payload_hash(&authority)? == get(&descriptor, "authorityHash").as_str().unwrap_or("")
            && payload_hash(get(&outer, "codeIdentity"))?
                == payload_hash(&paired_live_v2_identity(&deps.workspace)?)?,
        "ORIGINAL_OUTER",
    )?;
    need(
        get(&authority, "kind").as_str() == Some("direct_human_authorized_bot_update")
            && get(&authority, "ownerId").as_str() == Some("Codex")
            && get(&authority, "threadId").as_str() == Some("01a0f8cc-b9bb-77d2-9f4c-2844396f1b79")
            && get(&authority, "exploratory") == &json!(true)
            && get(&authority, "profitabilityEstablished") == &json!(false),
        "AUTHORITY",
    )?;
    let dir = runtime_dir.join("paired-live-v2-runs").join(&run_id);
    let child_dir = dir.join("child-runtime");
    let selection_file = dir.join("selection.jsonl");
    let selection_bytes = fs::read(&selection_file).map_err(|e| e.to_string())?;
    need(
        !selection_bytes.is_empty()
            && selection_bytes.len() <= 2 * 1024 * 1024
            && *selection_bytes.last().unwrap() == 10,
        "SELECTION",
    )?;
    let selected_journal = Journal::open(&selection_file)?;
    let selection = selected_journal
        .last
        .as_ref()
        .map(|x| get(x, "state").clone())
        .unwrap_or(V::Null);
    need(
        selected_journal.sequence == 1
            && get(&get(selected_journal.last.as_ref().unwrap(), "event"), "kind").as_str() == Some("pair_selected")
            && get(&get(selected_journal.last.as_ref().unwrap(), "event"), "requestHash")
                == get(&descriptor, "outerRequestHash")
            && payload_hash(&selection)? == get(&descriptor, "selectionHash").as_str().unwrap_or(""),
        "SELECTION",
    )?;
    let qualified = select_paired_live_candidate(
        &outer,
        universe,
        get(&selection, "capture"),
        instant_v(get(&selection, "selectedAt"))? as f64,
    )?;
    need(
        get(&qualified, "eligible") == &json!(true)
            && payload_hash(get(&qualified, "selection"))?
                == get(&descriptor, "selectionHash").as_str().unwrap_or(""),
        "ORIGINAL_SELECTION_PROOF",
    )?;
    let child_request_file = child_dir.join("paired-live-request.json");
    let child_authority_file = child_dir.join("paired-live-authority.json");
    let child_request = read(&child_request_file)?;
    let expected_child = json!({
        "schemaVersion": 1,
        "enabled": true,
        "ownerId": "Codex",
        "runId": run_id,
        "tournamentId": TID,
        "tournamentSlug": get(&outer, "tournamentSlug"),
        "markets": get(&get(&selection, "scope"), "markets"),
        "startedAt": get(&outer, "startedAt"),
        "entryExpiresAt": get(&selection, "reviewExpiresAt"),
        "authorityHash": get(&descriptor, "authorityHash"),
        "codeIdentity": paired_live_identity(&deps.workspace)?,
    });
    need(
        payload_hash(&child_request)? == get(&descriptor, "childRequestHash").as_str().unwrap_or("")
            && payload_hash(&child_request)? == payload_hash(&expected_child)?
            && payload_hash(&read(&child_authority_file)?)?
                == get(&descriptor, "authorityHash").as_str().unwrap_or(""),
        "ORIGINAL_CHILD",
    )?;
    let run_dir = child_dir.join("paired-live-runs").join(&run_id);
    let controller_dir = run_dir.join("controller");
    let registry_dir = run_dir.join("funding");
    let activation_file = run_dir.join("activation.json");
    let capture_file = run_dir.join("capture.json");
    let activation = read(&activation_file)?;
    let capture = read(&capture_file)?;
    let scope = get(&selection, "scope").clone();
    let controller_dir_resolved = controller_dir.canonicalize().unwrap_or(controller_dir.clone());
    let registry_dir_resolved = registry_dir.canonicalize().unwrap_or(registry_dir.clone());
    let activation_controller_dir = PathBuf::from(get(&activation, "controllerDir").as_str().unwrap_or(""))
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(get(&activation, "controllerDir").as_str().unwrap_or("")));
    let activation_registry_dir = PathBuf::from(get(&activation, "registryDir").as_str().unwrap_or(""))
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(get(&activation, "registryDir").as_str().unwrap_or("")));
    need(
        payload_hash(&activation)? == get(&descriptor, "activationHash").as_str().unwrap_or("")
            && get(&activation, "runId").as_str() == Some(run_id.as_str())
            && get(&activation, "activationAuthorityHash") == get(&descriptor, "authorityHash")
            && get(&activation, "captureHash") == get(&selection, "captureHash")
            && get(&activation, "entryExpiresAt") == get(&selection, "reviewExpiresAt")
            && payload_hash(&capture)? == get(&selection, "captureHash").as_str().unwrap_or("")
            && activation_controller_dir == controller_dir_resolved
            && activation_registry_dir == registry_dir_resolved,
        "ORIGINAL_ACTIVATION",
    )?;
    need(
        [
            controller_dir.join("paired-rest-journal.jsonl"),
            registry_dir.join("paired-shared-funding.jsonl"),
        ]
        .iter()
        .all(|f| f.exists() && fs::metadata(f).map(|m| m.len() > 0).unwrap_or(false)),
        "EXISTING_JOURNALS_REQUIRED",
    )?;
    let prior_actor = Journal::open(&controller_dir.join("paired-rest-journal.jsonl"))?;
    let prior_state = prior_actor
        .last
        .as_ref()
        .map(|x| get(x, "state").clone())
        .unwrap_or(V::Null);
    need(
        get(&get(&prior_state, "plan"), "planHash") == get(&activation, "planHash")
            && get(&get(&prior_state, "plan"), "captureHash") == get(&selection, "captureHash")
            && payload_hash(&get(&get(&prior_state, "plan"), "scope").clone())? == payload_hash(&scope)?,
        "ORIGINAL_ACTOR_PLAN",
    )?;
    let revision = get(&descriptor, "neutralAckRevision");
    need(
        get(revision, "scopeHash").as_str() == Some(payload_hash(&scope)?.as_str())
            && get(revision, "ownerId").as_str() == Some("Codex")
            && get(revision, "runId").as_str() == Some(run_id.as_str()),
        "REVISION_SCOPE",
    )?;
    let selector_lease = acquire_owner_lease(&dir, "Codex", &run_id, "paired-live-selector-v2", (deps.now)())
        .map_err(|e| format!("{PREFIX}{}", e.message))?;
    let inner = Rc::new(RefCell::new(Inner {
        descriptor: descriptor.clone(),
        descriptor_hash,
        descriptor_file,
        outer_file,
        legacy_file,
        authority_file,
        child_request_file,
        child_authority_file,
        activation_file,
        capture_file,
        selection_file,
        selection_bytes,
        runtime_dir: runtime_dir.to_path_buf(),
        controller_dir,
        registry_dir,
        scope,
        outer,
        selection,
        code,
        api,
        deps,
        selector_lease,
        controller: None,
        registry: None,
        bridge: None,
        last_funding: None,
        closed: false,
        busy: false,
        failed: false,
        last_failure: None,
        last_action: "recovery_loaded".to_string(),
    }));
    let open_result = (|| -> Result<(), String> {
        need(allowed(&inner), "STOPPED_OR_CHANGED")?;
        let registry_cell: Rc<RefCell<Option<PairedSharedFunding>>> = Rc::new(RefCell::new(None));
        let bridge_cell: Rc<RefCell<Option<Rc<PairedRestEvidenceBridge>>>> = Rc::new(RefCell::new(None));
        let last_funding: Rc<RefCell<Option<V>>> = Rc::new(RefCell::new(None));
        let inner_for_allowed = inner.clone();
        let can_operate_controller: Rc<dyn Fn() -> bool> =
            Rc::new(move || allowed(&inner_for_allowed));
        let registry_for_install = registry_cell.clone();
        let funding_for_install = last_funding.clone();
        let bridge_for_evidence = bridge_cell.clone();
        let get_account_lease = inner.borrow().deps.get_account_lease.clone();
        let raw_receipt_of = inner.borrow().deps.receipt_of.clone();
        let now = inner.borrow().deps.now.clone();
        let workspace = inner.borrow().deps.workspace.clone();
        let controller = open_paired_rest_controller_v3(
            &inner.borrow().controller_dir.clone(),
            &capture,
            &inner.borrow().scope.clone(),
            PairedRestControllerDeps {
                dispatch: Rc::new(|_spec: &V, _bd, _bn| -> Result<V, V3DispatchError> {
                    Err(V3DispatchError {
                        message: "PAIRED_NEUTRAL_RECOVERY_WRITES_DISABLED".into(),
                        wire_receipt: None,
                    })
                }),
                can_operate: can_operate_controller.clone(),
                assert_account_lease: get_account_lease.clone(),
                install_funding_reservation: Rc::new(move |r: &V| -> Result<V, String> {
                    let receipt = registry_for_install
                        .borrow()
                        .as_ref()
                        .ok_or("PAIRED_NEUTRAL_RECOVERY_REGISTRY_UNAVAILABLE")?
                        .install_funding_reservation(r)?;
                    *funding_for_install.borrow_mut() = Some(receipt.clone());
                    Ok(receipt)
                }),
                get_evidence: Rc::new(move |context: &V| -> Result<V, String> {
                    bridge_for_evidence
                        .borrow()
                        .as_ref()
                        .ok_or("PAIRED_NEUTRAL_RECOVERY_BRIDGE_UNAVAILABLE".to_string())
                        .and_then(|b| b.get_evidence(context))
                }),
                raw_receipt_of,
                now: now.clone(),
                workspace_dir: workspace,
            },
            descriptor.get("neutralAckRevision"),
        )?;
        let controller = Rc::new(controller);
        let status = controller.status();
        need(
            get(&status, "planHash") == get(&activation, "planHash"),
            "ORIGINAL_ACTOR_PLAN",
        )?;
        let orders = get(&status, "orders").as_array().cloned().unwrap_or_default();
        need(
            orders.len() == 1
                && get(&orders[0], "orderId") == get(&descriptor, "neutralAckRevision").get("orderId").unwrap_or(&V::Null)
                && get(&orders[0], "marketId")
                    == get(&get(&status, "planLegs").as_array().cloned().unwrap_or_default()[0], "marketId")
                && ["acknowledged", "reconciled"].contains(&get(&orders[0], "phase").as_str().unwrap_or("")),
            "ONLY_ORIGINAL_FIRST_ORDER",
        )?;
        let get_other_state = inner.borrow().deps.get_other_state.clone();
        let controller_for_funding = controller.clone();
        let controller_for_status = controller.clone();
        let registry = open_paired_shared_funding(
            &inner.borrow().registry_dir.clone(),
            &activation,
            &inner.borrow().controller_dir.clone(),
            PairedSharedFundingDeps {
                get_account_lease: get_account_lease.clone(),
                get_other_state: get_other_state.clone(),
                get_paired_funding: Rc::new(move || controller_for_funding.funding_reservation()),
                get_paired_status: Rc::new(move || controller_for_status.status()),
                can_operate: can_operate_controller.clone(),
                now: now.clone(),
            },
        )?;
        let first_funding = registry.install_funding_reservation(&controller.funding_reservation())?;
        *last_funding.borrow_mut() = Some(first_funding);
        let revision_order_id = get(&descriptor, "neutralAckRevision").get("orderId").cloned().unwrap_or(V::Null);
        let controller_for_owns = controller.clone();
        let inner_for_risk = inner.clone();
        let funding_for_bridge = last_funding.clone();
        let get_other_state_b = get_other_state.clone();
        let inner_for_can_read = inner.clone();
        let has_read_headroom = inner.borrow().deps.has_read_headroom.clone();
        let api_dispatch = inner.borrow().api.borrow().dispatch.clone();
        let scope = inner.borrow().scope.clone();
        let bridge = Rc::new(create_paired_rest_evidence_bridge(
            EvidenceBridgeDeps {
                dispatch: Rc::new(
                    move |spec: &V,
                          bd: Option<&dyn Fn(&V, &V) -> Result<bool, String>>,
                          bn: Option<&dyn Fn(&V, &V) -> Result<bool, String>>|
                          -> Result<V, String> {
                        let bd_t = |p: &V, s: &V| -> Result<bool, OrderTransportError> {
                            bd.map(|g| g(p, s).map_err(|c| guard_err(&c))).unwrap_or(Ok(false))
                        };
                        let bn_t = |p: &V, s: &V| -> Result<bool, OrderTransportError> {
                            bn.map(|g| g(p, s).map_err(|c| guard_err(&c))).unwrap_or(Ok(false))
                        };
                        api_dispatch(
                            spec,
                            bd.map(|_| &bd_t as &TGuard),
                            bn.map(|_| &bn_t as &TGuard),
                        )
                        .map_err(|e: crate::paired_rest_controller::WireDispatchError| e.message)
                    },
                ),
                receipt_of: inner.borrow().deps.receipt_of.clone(),
                get_global_risk: Rc::new(move || global_risk(&inner_for_risk)),
                get_funding: Rc::new(move || funding_for_bridge.borrow().clone().unwrap_or(V::Null)),
                get_other_reserved: Rc::new(move || {
                    let s = get_other_state_b();
                    let total: i128 = get(&s, "intents")
                        .as_object()
                        .map(|m| {
                            m.values()
                                .filter(|i| {
                                    get(i, "status").as_str() != Some("closed")
                                        && get(i, "action").as_str() == Some("buy")
                                })
                                .map(|i| {
                                    let remaining = get(i, "quantity").as_i64().unwrap_or(0)
                                        - get(i, "reconciledFills")
                                            .as_array()
                                            .map(|fs| {
                                                fs.iter()
                                                    .map(|f| get(f, "quantity").as_i64().unwrap_or(0))
                                                    .sum::<i64>()
                                            })
                                            .unwrap_or(0);
                                    decimal_micros(get(i, "price"), "exact").unwrap_or(0)
                                        * remaining as i128
                                })
                                .sum()
                        })
                        .unwrap_or(0);
                    json!(format_micros(total))
                }),
                owns_order: Rc::new(move |oid: &V, leg: &V| -> bool {
                    oid == &revision_order_id
                        && controller_for_owns
                            .status()
                            .get("orders")
                            .and_then(|x| x.as_array())
                            .map(|os| {
                                os.iter().any(|o| {
                                    get(o, "orderId") == oid
                                        && get(o, "marketId") == get(leg, "marketId")
                                        && get(o, "exchangeId") == get(leg, "exchangeId")
                                })
                            })
                            .unwrap_or(false)
                }),
                can_read: Rc::new(move || allowed(&inner_for_can_read)),
                has_read_headroom,
                now: now.clone(),
            },
            &scope,
            Some(&json!({"preparation": 5, "reconciliation": 6, "cancel": 1})),
        )?);
        *bridge_cell.borrow_mut() = Some(bridge.clone());
        *registry_cell.borrow_mut() = Some(registry);
        {
            let mut g = inner.borrow_mut();
            g.controller = Some(controller);
            g.registry = registry_cell.borrow_mut().take();
            g.bridge = Some(bridge);
            g.last_funding = last_funding.borrow().clone();
        }
        publish(&inner, "recovery_loaded")?;
        Ok(())
    })();
    if let Err(e) = open_result {
        let durable_failed = {
            let g = inner.borrow();
            g.controller
                .as_ref()
                .map(|c| get(&c.status(), "failed").as_bool().unwrap_or(false))
                .unwrap_or(false)
                || g.registry
                    .as_ref()
                    .map(|r| get(&r.status(), "failed").as_bool().unwrap_or(false))
                    .unwrap_or(false)
                || is_durable_error(&e)
        };
        let _ = cleanup(&inner);
        if durable_failed {
            return Err(format!("{e}:fatal:durableRecordingFailed"));
        }
        return Err(e);
    }
    Ok(Some(PairedNeutralRecoveryHost { inner }))
}

fn guard_err(code: &str) -> OrderTransportError {
    OrderTransportError {
        code: code.into(),
        status: None,
        api_code: None,
        outcome: "blocked".into(),
        may_have_executed: false,
        retry_same_request_only: false,
        retry_after_ms: 0.0,
        semantic_mismatch: false,
    }
}

impl PairedNeutralRecoveryHost {
    // authorizes(spec) — GET reads only.
    pub fn authorizes(
        &self,
        spec: &V,
        before_dispatch: &dyn Fn(&V, &V) -> Result<bool, String>,
        before_network: &dyn Fn(&V, &V) -> Result<bool, String>,
    ) -> bool {
        if get(spec, "method").as_str() != Some("GET") || !allowed(&self.inner) {
            return false;
        }
        self.inner
            .borrow()
            .bridge
            .as_ref()
            .map(|b| b.authorizes(spec, before_dispatch, before_network))
            .unwrap_or(false)
    }

    pub fn on_fetch_start(&self, _event: &V) -> bool {
        false
    }

    pub fn blocks_legacy_writes(&self) -> bool {
        true
    }

    pub fn allows_legacy(&self, spec: &V) -> bool {
        get(spec, "method").as_str() == Some("GET")
    }

    pub fn status(&self) -> V {
        host_status(&self.inner)
    }

    pub fn step(&self) -> Result<V, String> {
        need(
            !self.inner.borrow().busy && !self.inner.borrow().closed,
            "BUSY_OR_CLOSED",
        )?;
        self.inner.borrow_mut().busy = true;
        let result = (|| -> Result<V, String> {
            if !allowed(&self.inner) {
                return publish(&self.inner, "operational_stop_or_identity_changed");
            }
            let s = self
                .inner
                .borrow()
                .controller
                .as_ref()
                .map(|c| c.status())
                .unwrap_or(V::Null);
            let o = get(&s, "orders")
                .as_array()
                .and_then(|os| os.iter().find(|o| get(o, "phase").as_str() != Some("reconciled")))
                .cloned();
            let o = match o {
                Some(o) => o,
                None => return publish(&self.inner, "original_lot_reconciled_reserve_retained"),
            };
            need(
                get(&o, "orderId")
                    == get(&self.inner.borrow().descriptor, "neutralAckRevision")
                        .get("orderId")
                        .unwrap_or(&V::Null),
                "ORDER_CHANGED",
            )?;
            let leg = get(&s, "planLegs")
                .as_array()
                .and_then(|ls| {
                    ls.iter().find(|l| {
                        get(l, "marketId") == get(&o, "marketId") && get(l, "exchangeId") == get(&o, "exchangeId")
                    })
                })
                .cloned()
                .unwrap_or(V::Null);
            let bridge = self.inner.borrow().bridge.clone();
            let evidence = bridge
                .ok_or("PAIRED_NEUTRAL_RECOVERY_BRIDGE_UNAVAILABLE")?
                .refresh_reconciliation(get(&o, "orderId"), &leg)?;
            need(allowed(&self.inner), "STOPPED_AFTER_READS")?;
            let controller = self.inner.borrow().controller.clone();
            controller
                .ok_or("PAIRED_NEUTRAL_RECOVERY_CONTROLLER_UNAVAILABLE")?
                .reconcile(&evidence)?;
            self.inner.borrow_mut().last_failure = None;
            publish(&self.inner, "official_scoped_reconciliation")
        })();
        match result {
            Ok(v) => {
                self.inner.borrow_mut().busy = false;
                Ok(v)
            }
            Err(e) => {
                let failure = if is_all_caps_code(&e) {
                    e.clone()
                } else {
                    "PAIRED_NEUTRAL_RECOVERY_EVIDENCE_UNAVAILABLE".into()
                };
                self.inner.borrow_mut().last_failure = Some(failure.clone());
                let durable_failed = {
                    let g = self.inner.borrow();
                    g.controller
                        .as_ref()
                        .map(|c| get(&c.status(), "failed").as_bool().unwrap_or(false))
                        .unwrap_or(false)
                        || g.registry
                            .as_ref()
                            .map(|r| get(&r.status(), "failed").as_bool().unwrap_or(false))
                            .unwrap_or(false)
                        || e.contains(":fatal:")
                        || e.contains("durableRecordingFailed")
                };
                self.inner.borrow_mut().busy = false;
                if durable_failed {
                    self.inner.borrow_mut().failed = true;
                    return Err(format!("{failure}:fatal:durableRecordingFailed"));
                }
                let _ = publish(&self.inner, "reconciliation_blocked");
                Err(failure)
            }
        }
    }

    pub fn close(&self) -> Result<(), String> {
        need(!self.inner.borrow().busy, "BUSY")?;
        if self.inner.borrow().closed {
            return Ok(());
        }
        self.inner.borrow_mut().closed = true;
        cleanup(&self.inner)
    }
}
