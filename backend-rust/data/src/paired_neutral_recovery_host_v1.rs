// execution/paired-neutral-recovery-host-v1.mjs — an explicit reconciliation
// revision, not a replacement trading host. It reuses the original actor,
// funding registry, account owner and one API. It cannot issue POST/DELETE,
// renew a window, select a new pair, or retry a key.
use baal_execution::core::{decimal_micros, format_micros, payload_hash};
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::journal::Journal;
use baal_service_runtime::ledger::{acquire_owner_lease, OwnerLease};
use baal_execution::transport::OrderTransportError;
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const TID: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
const THREAD: &str = "01a0f8cc-b9bb-77d2-9f4c-2844396f1b79";
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("PAIRED_NEUTRAL_RECOVERY_{code}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V { v.get(k).unwrap_or(&V::Null) }
fn iso(n: i64) -> String { ms_to_iso(n as f64) }
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn copy(v: &V) -> V { v.clone() }
fn read(p: &Path) -> Result<V, String> {
    serde_json::from_str(&std::fs::read_to_string(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
pub fn paired_neutral_recovery_identity(workspace: &Path) -> V {
    json!(baal_collector::frozen_paper_graph::collect_frozen_paper_files(
        workspace,
        &["execution/paired-neutral-recovery-host-v1.mjs".to_string(), "execution/scoped-rest-receipts.mjs".to_string()],
    ).unwrap_or_default())
}

type GetV = Rc<dyn Fn() -> Result<V, String>>;

pub struct PairedNeutralRecoveryHost {
    runtime_dir: PathBuf,
    workspace: PathBuf,
    universe: V,
    api: Rc<dyn crate::paired_live_host::HostApi>,
    receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    get_account_lease: GetV,
    get_other_state: GetV,
    can_operate: Rc<dyn Fn() -> bool>,
    has_read_headroom: Rc<dyn Fn(usize) -> bool>,
    now: Rc<dyn Fn() -> i64>,
    descriptor_file: PathBuf,
    descriptor: V,
    descriptor_hash: String,
    code: V,
    outer_file: PathBuf,
    legacy_file: PathBuf,
    authority_file: PathBuf,
    outer: V,
    run_id: String,
    dir: PathBuf,
    child_dir: PathBuf,
    selection_file: PathBuf,
    selection_bytes: Vec<u8>,
    selection: V,
    scope: V,
    child_request_file: PathBuf,
    child_authority_file: PathBuf,
    activation_file: PathBuf,
    capture_file: PathBuf,
    activation: V,
    capture: V,
    controller_dir: PathBuf,
    registry_dir: PathBuf,
    selector_lease: RefCell<Option<OwnerLease>>,
    controller: RefCell<Option<Rc<crate::paired_rest_controller_v3::PairedRestControllerV3>>>,
    registry: RefCell<Option<Rc<crate::paired_shared_funding::PairedSharedFunding>>>,
    bridge: RefCell<Option<Rc<crate::paired_rest_evidence_bridge::PairedRestEvidenceBridge>>>,
    last_funding: RefCell<V>,
    closed: RefCell<bool>,
    busy: RefCell<bool>,
    failed: RefCell<bool>,
    last_failure: RefCell<V>,
    last_action: RefCell<String>,
}

impl PairedNeutralRecoveryHost {
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        runtime_dir: &Path,
        workspace: &Path,
        universe: &V,
        api: Rc<dyn crate::paired_live_host::HostApi>,
        receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
        get_account_lease: GetV,
        get_other_state: GetV,
        can_operate: Rc<dyn Fn() -> bool>,
        has_read_headroom: Rc<dyn Fn(usize) -> bool>,
        now: Rc<dyn Fn() -> i64>,
    ) -> Result<Option<Rc<Self>>, String> {
        let descriptor_file = runtime_dir.join("paired-neutral-ack-recovery.json");
        if !descriptor_file.exists() { return Ok(None); }
        need(runtime_dir.is_absolute() && workspace.is_absolute(), "DEPENDENCIES")?;
        need((can_operate)(), "STOPPED")?;
        let descriptor = read(&descriptor_file)?;
        let descriptor_hash = payload_hash(&descriptor)?;
        let code = paired_neutral_recovery_identity(workspace);
        need(
            get(&descriptor, "schemaVersion") == &json!(1)
                && get(&descriptor, "kind") == &json!("paired_neutral_ack_recovery_only")
                && get(&descriptor, "ownerId") == &json!("Codex")
                && get(&descriptor, "tournamentId") == &json!(TID)
                && get(&descriptor, "runId").as_str().is_some_and(|s| {
                    s.starts_with("sig-pilot-paired-")
                        && s.len() > "sig-pilot-paired-".len() && s.len() <= "sig-pilot-paired-".len() + 60
                        && s.bytes().skip("sig-pilot-paired-".len()).all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                })
                && ["outerRequestHash", "selectionHash", "childRequestHash", "authorityHash", "activationHash"].iter()
                    .all(|k| hex(get(&descriptor, k)))
                && payload_hash(get(&descriptor, "codeIdentity")).ok() == payload_hash(&code).ok(),
            "DESCRIPTOR",
        )?;
        let outer_file = runtime_dir.join("paired-live-v2-request.json");
        let legacy_file = runtime_dir.join("paired-live-request.json");
        let authority_file = runtime_dir.join("paired-live-authority.json");
        need(!legacy_file.exists(), "LEGACY_REQUEST_PRESENT")?;
        let outer = read(&outer_file)?;
        let authority = read(&authority_file)?;
        let run_id = get(&descriptor, "runId").as_str().unwrap().to_string();
        need(
            payload_hash(&outer).ok() == get(&descriptor, "outerRequestHash").as_str().map(String::from)
                && get(&outer, "runId").as_str() == Some(run_id.as_str())
                && get(&outer, "authorityHash") == get(&descriptor, "authorityHash")
                && payload_hash(&authority).ok() == get(&descriptor, "authorityHash").as_str().map(String::from)
                && payload_hash(get(&outer, "codeIdentity")).ok() == payload_hash(&crate::paired_live_host_v2::paired_live_v2_identity(workspace)).ok(),
            "ORIGINAL_OUTER",
        )?;
        need(
            get(&authority, "kind") == &json!("direct_human_authorized_bot_update")
                && get(&authority, "ownerId") == &json!("Codex")
                && get(&authority, "threadId") == &json!(THREAD)
                && get(&authority, "exploratory") == &json!(true)
                && get(&authority, "profitabilityEstablished") == &json!(false),
            "AUTHORITY",
        )?;
        let dir = runtime_dir.join("paired-live-v2-runs").join(&run_id);
        let child_dir = dir.join("child-runtime");
        let selection_file = dir.join("selection.jsonl");
        let selection_bytes = std::fs::read(&selection_file).map_err(|e| e.to_string())?;
        need(!selection_bytes.is_empty() && selection_bytes.len() <= 2 * 1024 * 1024 && selection_bytes.last() == Some(&10), "SELECTION")?;
        let selected_journal = Journal::open(&selection_file)?;
        let selection = copy(get(selected_journal.last.as_ref().unwrap_or(&V::Null), "state"));
        need(
            selected_journal.sequence == 1
                && get(get(selected_journal.last.as_ref().unwrap_or(&V::Null), "event"), "kind") == &json!("pair_selected")
                && get(get(selected_journal.last.as_ref().unwrap_or(&V::Null), "event"), "requestHash") == get(&descriptor, "outerRequestHash")
                && payload_hash(&selection).ok() == get(&descriptor, "selectionHash").as_str().map(String::from),
            "SELECTION",
        )?;
        let qualified = crate::paired_live_host_v2::select_paired_live_candidate(
            &outer, universe, get(&selection, "capture"),
            baal_service_runtime::contract::parse_iso_ms(get(&selection, "selectedAt").as_str().unwrap_or("")).unwrap_or(0.0) as i64,
        )?;
        need(
            get(&qualified, "eligible") == &json!(true)
                && payload_hash(get(&qualified, "selection")).ok() == get(&descriptor, "selectionHash").as_str().map(String::from),
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
            "markets": copy(get(get(&selection, "scope"), "markets")),
            "startedAt": get(&outer, "startedAt"),
            "entryExpiresAt": get(&selection, "reviewExpiresAt"),
            "authorityHash": get(&descriptor, "authorityHash"),
            "codeIdentity": crate::paired_live_host::paired_live_identity(workspace),
        });
        need(
            payload_hash(&child_request).ok() == get(&descriptor, "childRequestHash").as_str().map(String::from)
                && payload_hash(&child_request).ok() == payload_hash(&expected_child).ok()
                && payload_hash(&read(&child_authority_file)?).ok() == get(&descriptor, "authorityHash").as_str().map(String::from),
            "ORIGINAL_CHILD",
        )?;
        let run_dir = child_dir.join("paired-live-runs").join(&run_id);
        let controller_dir = run_dir.join("controller");
        let registry_dir = run_dir.join("funding");
        let activation_file = run_dir.join("activation.json");
        let capture_file = run_dir.join("capture.json");
        let activation = read(&activation_file)?;
        let capture = read(&capture_file)?;
        let scope = copy(get(&selection, "scope"));
        need(
            payload_hash(&activation).ok() == get(&descriptor, "activationHash").as_str().map(String::from)
                && get(&activation, "runId").as_str() == Some(run_id.as_str())
                && get(&activation, "activationAuthorityHash") == get(&descriptor, "authorityHash")
                && get(&activation, "captureHash") == get(&selection, "captureHash")
                && get(&activation, "entryExpiresAt") == get(&selection, "reviewExpiresAt")
                && payload_hash(&capture).ok() == get(&selection, "captureHash").as_str().map(String::from)
                && std::fs::canonicalize(get(&activation, "controllerDir").as_str().unwrap_or("")).ok() == std::fs::canonicalize(&controller_dir).ok()
                && std::fs::canonicalize(get(&activation, "registryDir").as_str().unwrap_or("")).ok() == std::fs::canonicalize(&registry_dir).ok(),
            "ORIGINAL_ACTIVATION",
        )?;
        need(
            [controller_dir.join("paired-rest-journal.jsonl"), registry_dir.join("paired-shared-funding.jsonl")].iter()
                .all(|f| f.exists() && std::fs::metadata(f).map(|m| m.len() > 0).unwrap_or(false)),
            "EXISTING_JOURNALS_REQUIRED",
        )?;
        // Selection qualification and actual actor initialization have
        // distinct wall clocks, hence distinct plan hashes.
        let prior_actor = Journal::open(&controller_dir.join("paired-rest-journal.jsonl"))?;
        need(
            get(get(get(prior_actor.last.as_ref().unwrap_or(&V::Null), "state"), "plan"), "planHash") == get(&activation, "planHash")
                && get(get(get(prior_actor.last.as_ref().unwrap_or(&V::Null), "state"), "plan"), "captureHash") == get(&selection, "captureHash")
                && payload_hash(get(get(get(prior_actor.last.as_ref().unwrap_or(&V::Null), "state"), "plan"), "scope")).ok() == payload_hash(&scope).ok(),
            "ORIGINAL_ACTOR_PLAN",
        )?;
        need(
            get(get(&descriptor, "neutralAckRevision"), "scopeHash").as_str() == payload_hash(&scope).ok().as_deref()
                && get(get(&descriptor, "neutralAckRevision"), "ownerId") == &json!("Codex")
                && get(get(&descriptor, "neutralAckRevision"), "runId").as_str() == Some(run_id.as_str()),
            "REVISION_SCOPE",
        )?;
        let selector_lease = acquire_owner_lease(&dir, "Codex", &run_id, "paired-live-selector-v2", (now)() as f64)
            .map_err(|e| format!("PAIRED_NEUTRAL_RECOVERY_{}", e.code))?;
        let this = Rc::new(Self {
            runtime_dir: runtime_dir.to_path_buf(),
            workspace: workspace.to_path_buf(),
            universe: universe.clone(),
            api,
            receipt_of,
            get_account_lease,
            get_other_state,
            can_operate,
            has_read_headroom,
            now,
            descriptor_file,
            descriptor,
            descriptor_hash,
            code,
            outer_file,
            legacy_file,
            authority_file,
            outer,
            run_id,
            dir,
            child_dir,
            selection_file,
            selection_bytes,
            selection,
            scope,
            child_request_file,
            child_authority_file,
            activation_file,
            capture_file,
            activation,
            capture,
            controller_dir,
            registry_dir,
            selector_lease: RefCell::new(Some(selector_lease)),
            controller: RefCell::new(None),
            registry: RefCell::new(None),
            bridge: RefCell::new(None),
            last_funding: RefCell::new(V::Null),
            closed: RefCell::new(false),
            busy: RefCell::new(false),
            failed: RefCell::new(false),
            last_failure: RefCell::new(V::Null),
            last_action: RefCell::new("recovery_loaded".into()),
        });
        let result = this.init();
        if let Err(e) = result {
            let durable = this.controller.borrow().as_ref().map(|c| get(&c.status(), "failed") == &json!(true)).unwrap_or(false)
                || this.registry.borrow().as_ref().map(|r| get(&r.status(), "failed") == &json!(true)).unwrap_or(false)
                || ["EIO", "ENOSPC", "EROFS", "EPERM", "EACCES", "EBUSY"].iter().any(|c| e.contains(c));
            let _ = this.cleanup();
            if durable { return Err(format!("{e} (fatal/durableRecordingFailed)")); }
            return Err(e);
        }
        Ok(Some(this))
    }
    fn fixed(&self) -> Result<(), String> {
        if let Some(l) = self.selector_lease.borrow().as_ref() {
            l.assert_ownership().map_err(|e| format!("PAIRED_NEUTRAL_RECOVERY_{}", e.code))?;
        }
        need(
            !self.legacy_file.exists()
                && payload_hash(&read(&self.descriptor_file)?).ok() == Some(self.descriptor_hash.clone())
                && payload_hash(&paired_neutral_recovery_identity(&self.workspace)).ok() == payload_hash(&self.code).ok(),
            "REVISION_CHANGED",
        )?;
        for (file, hash) in [
            (&self.outer_file, get(&self.descriptor, "outerRequestHash").as_str().unwrap_or("")),
            (&self.authority_file, get(&self.descriptor, "authorityHash").as_str().unwrap_or("")),
            (&self.child_request_file, get(&self.descriptor, "childRequestHash").as_str().unwrap_or("")),
            (&self.child_authority_file, get(&self.descriptor, "authorityHash").as_str().unwrap_or("")),
            (&self.activation_file, get(&self.descriptor, "activationHash").as_str().unwrap_or("")),
            (&self.capture_file, get(&self.selection, "captureHash").as_str().unwrap_or("")),
        ] {
            need(payload_hash(&read(file)?).ok().as_deref() == Some(hash), "ORIGINAL_SOURCE_CHANGED")?;
        }
        need(std::fs::read(&self.selection_file).map_err(|e| e.to_string())? == self.selection_bytes, "SELECTION_CHANGED")?;
        Ok(())
    }
    fn allowed(&self) -> bool {
        if *self.closed.borrow() || *self.failed.borrow() || !(self.can_operate)() { return false; }
        self.fixed().is_ok()
    }
    fn global_risk(&self) -> Result<V, String> {
        let core = (self.get_other_state)()?;
        let pair = self.controller.borrow().as_ref().map(|c| c.status()).unwrap_or(V::Null);
        let mut unresolved: Vec<V> = get(&core, "intents").as_object().map(|o| {
            o.values().filter(|i| get(i, "status") != &json!("closed")).map(|i| get(i, "intentId").clone()).collect()
        }).unwrap_or_default();
        unresolved.extend(get(&pair, "orders").as_array().map(|os| {
            os.iter().filter(|o| get(o, "phase") != &json!("reconciled")).map(|o| get(o, "intentId").clone()).collect::<Vec<V>>()
        }).unwrap_or_default());
        Ok(json!({
            "ownerId": "Codex",
            "runId": self.run_id,
            "tournamentId": TID,
            "complete": true,
            "soleCollector": true,
            "stopped": !self.allowed(),
            "at": iso((self.now)()),
            "unresolvedIntentIds": unresolved,
            "sourceHash": payload_hash(&json!({
                "core": core,
                "paired": pair,
                "accountLease": (self.get_account_lease)()?,
            }))?,
        }))
    }
    fn status_view(&self) -> V {
        let s = self.controller.borrow().as_ref().map(|c| c.status());
        json!({
            "hostVersion": 3,
            "mode": "paired_neutral_ack_recovery_only",
            "runId": self.run_id,
            "requestHash": get(&self.descriptor, "outerRequestHash"),
            "recoveryDescriptorHash": self.descriptor_hash,
            "authorityHash": get(&self.descriptor, "authorityHash"),
            "selectionHash": get(&self.descriptor, "selectionHash"),
            "selectedMarkets": copy(get(&self.scope, "markets")),
            "entryExpiresAt": get(&self.outer, "entryExpiresAt"),
            "reviewExpiresAt": get(&self.selection, "reviewExpiresAt"),
            "initialized": self.controller.borrow().is_some(),
            "phase": s.as_ref().map(|x| get(x, "phase").clone()).unwrap_or(json!("recovery_unavailable")),
            "orders": s.as_ref().map(|x| get(x, "orders").as_array().map(|a| a.len()).unwrap_or(0)).unwrap_or(0),
            "blocksLegacyWrites": true,
            "entryAllowed": false,
            "writeCapability": false,
            "lastAction": self.last_action.borrow().clone(),
            "lastFailure": self.last_failure.borrow().clone(),
            "failed": *self.failed.borrow() || s.as_ref().map(|x| get(x, "failed") == &json!(true)).unwrap_or(false),
            "closed": *self.closed.borrow(),
            "additionalPersistentProcesses": 0,
            "additionalAccountClients": 0,
        })
    }
    fn publish(&self, action: &str) -> Result<V, String> {
        *self.last_action.borrow_mut() = action.to_string();
        let s = self.controller.borrow().as_ref().map(|c| c.status());
        let mut view = self.status_view().as_object().cloned().unwrap_or_default();
        view.insert("schemaVersion".into(), json!(3));
        view.insert("observedAt".into(), json!(iso((self.now)())));
        view.insert("action".into(), json!(action));
        view.insert("orders".into(), s.as_ref().map(|x| get(x, "orders").as_array().map(|os| os.iter().map(|o| json!({
            "intentId": get(o, "intentId"),
            "orderId": get(o, "orderId"),
            "marketId": get(o, "marketId"),
            "exchangeId": get(o, "exchangeId"),
            "phase": get(o, "phase"),
            "limit": get(get(o, "body"), "price"),
            "quantity": 1,
            "cancelPending": get(o, "cancelPending"),
            "proofHash": get(o, "proofHash"),
        })).collect::<Vec<V>>()).unwrap_or_default()).map(V::from).unwrap_or(json!([])));
        view.insert("orderCount".into(), s.as_ref().map(|x| get(x, "orders").as_array().map(|a| a.len() as u64).unwrap_or(0)).map(|n| json!(n)).unwrap_or(json!(0)));
        view.insert("owned".into(), s.as_ref().map(|x| get(x, "owned").clone()).unwrap_or(json!([])));
        view.insert("funding".into(), s.as_ref().map(|x| get(x, "funding").clone()).unwrap_or(V::Null));
        view.insert("realizedPnl".into(), json!("0.000000"));
        view.insert("ordinaryMinimumPayout".into(), V::Null);
        view.insert("ordinarySurplus".into(), V::Null);
        view.insert("ordinaryResolutionOnly".into(), json!(true));
        view.insert("maximumExceptionalLoss".into(), s.as_ref().map(|x| get(get(x, "funding"), "ownedPrincipal").clone()).unwrap_or(json!("0.000000")));
        view.insert("profitabilityEstablished".into(), json!(false));
        atomic_json(&self.runtime_dir.join("paired-live-stats.json"), &V::Object(view))?;
        Ok(self.status_view())
    }
    fn cleanup(&self) -> Result<(), String> {
        let mut error: Option<String> = None;
        if let Some(b) = self.bridge.borrow_mut().take() { if let Err(e) = b.close() { error = error.or(Some(e)); } }
        if let Some(r) = self.registry.borrow_mut().take() { if let Err(e) = r.close() { error = error.or(Some(e)); } }
        if let Some(c) = self.controller.borrow_mut().take() { if let Err(e) = c.close() { error = error.or(Some(e)); } }
        if let Some(l) = self.selector_lease.borrow_mut().as_mut() { if let Err(e) = l.release() { error = error.or(Some(e.to_string())); } }
        match error { Some(e) => Err(e), None => Ok(()) }
    }
    fn init(self: &Rc<Self>) -> Result<(), String> {
        need(self.allowed(), "STOPPED_OR_CHANGED")?;
        let this = self.clone();
        let this2 = self.clone();
        let this3 = self.clone();
        let controller = crate::paired_rest_controller_v3::PairedRestControllerV3::open_v3(
            &self.controller_dir,
            Some(&self.capture),
            &self.scope,
            Rc::new(move |_s: &V, _d, _n| -> Result<V, OrderTransportError> {
                Err(OrderTransportError {
                    code: "PAIRED_NEUTRAL_RECOVERY_WRITES_DISABLED".into(),
                    status: None, api_code: None, outcome: "rejected".into(),
                    may_have_executed: false, retry_same_request_only: false,
                    retry_after_ms: 0.0, semantic_mismatch: false,
                })
            }),
            Rc::new(move || this.allowed()),
            self.get_account_lease.clone(),
            Rc::new(move |r: V| {
                let reg = this2.registry.borrow();
                let r2 = reg.as_ref().unwrap().install_funding_reservation(&r)?;
                *this2.last_funding.borrow_mut() = r2.clone();
                Ok(r2)
            }),
            Rc::new(move |context: &V| {
                let b = this3.bridge.borrow();
                b.as_ref().unwrap().get_evidence(context)
            }),
            self.receipt_of.clone(),
            self.now.clone(),
            Some(get(&self.descriptor, "neutralAckRevision").clone()),
        )?;
        need(
            get(&controller.status(), "planHash") == get(&self.activation, "planHash"),
            "ORIGINAL_ACTOR_PLAN",
        )?;
        let original_order = get(&controller.status(), "orders").as_array().cloned().unwrap_or_default();
        need(
            original_order.len() == 1
                && get(&original_order[0], "orderId") == get(get(&self.descriptor, "neutralAckRevision"), "orderId")
                && get(&original_order[0], "marketId") == get(get(&controller.status(), "planLegs").as_array().and_then(|a| a.first()).unwrap_or(&V::Null), "marketId")
                && ["acknowledged", "reconciled"].contains(&get(&original_order[0], "phase").as_str().unwrap_or("")),
            "ONLY_ORIGINAL_FIRST_ORDER",
        )?;
        let this4 = self.clone();
        let this5 = self.clone();
        let registry = Rc::new(crate::paired_shared_funding::PairedSharedFunding::open(
            &self.registry_dir,
            &self.activation,
            &self.controller_dir,
            self.get_account_lease.clone(),
            self.get_other_state.clone(),
            Rc::new(move || this4.controller.borrow().as_ref().unwrap().funding_reservation()),
            Rc::new(move || Ok(this5.controller.borrow().as_ref().unwrap().status())),
            {
                let t = self.clone();
                Rc::new(move || t.allowed())
            },
            self.now.clone(),
        )?);
        *self.last_funding.borrow_mut() = registry.install_funding_reservation(&controller.funding_reservation()?)?;
        let this6 = self.clone();
        let this7 = self.clone();
        let this8 = self.clone();
        let this9 = self.clone();
        let api = self.api.clone();
        let order_id = get(get(&self.descriptor, "neutralAckRevision"), "orderId").clone();
        let bridge = Rc::new(crate::paired_rest_evidence_bridge::PairedRestEvidenceBridge::new(
            Rc::new(move |s: &V, _d, _n| api.dispatch(s).map_err(|e| e.code)),
            self.receipt_of.clone(),
            &self.scope,
            Rc::new(move || this6.global_risk()),
            Rc::new(move || Ok(this7.last_funding.borrow().clone())),
            Rc::new(move || {
                let s = (this8.get_other_state)()?;
                Ok(json!(format_micros(get(&s, "intents").as_object().map(|o| o.values()
                    .filter(|i| get(i, "status") != &json!("closed") && get(i, "action") == &json!("buy"))
                    .map(|i| {
                        decimal_micros(get(i, "price"), "exact").unwrap_or(0) * {
                            let fills: i128 = get(i, "reconciledFills").as_array().map(|fs| fs.iter().map(|f| baal_service_runtime::json_text::js_int(get(f, "quantity")).unwrap_or(0) as i128).sum::<i128>()).unwrap_or(0);
                            baal_service_runtime::json_text::js_int(get(i, "quantity")).unwrap_or(0) as i128 - fills
                        }
                    }).sum::<i128>()).unwrap_or(0))))
            }),
            {
                let this9b = this9.clone();
                Rc::new(move |id: &V, leg: &V| {
                    id == &order_id
                        && this9b.controller.borrow().as_ref().map(|c| {
                            get(&c.status(), "orders").as_array().map(|os| os.iter().any(|o| {
                                get(o, "orderId") == id && get(o, "marketId") == get(leg, "marketId") && get(o, "exchangeId") == get(leg, "exchangeId")
                            })).unwrap_or(false)
                        }).unwrap_or(false)
                })
            },
            {
                let t = self.clone();
                Rc::new(move || t.allowed())
            },
            {
                let h = self.has_read_headroom.clone();
                std::rc::Rc::new(move |n: i64| -> Result<bool, String> { Ok((h)(n as usize)) }) as std::rc::Rc<dyn Fn(i64) -> Result<bool, String>>
            },
            self.now.clone(),
            Some(json!({"preparation": 5, "reconciliation": 6, "cancel": 1})),
        )?);
        *self.controller.borrow_mut() = Some(controller);
        *self.registry.borrow_mut() = Some(registry);
        *self.bridge.borrow_mut() = Some(bridge);
        self.publish("recovery_loaded")?;
        Ok(())
    }
    pub fn authorizes(&self, spec: &V) -> bool {
        get(spec, "method") == &json!("GET")
            && self.allowed()
            && self.bridge.borrow().as_ref().map(|b| b.authorizes(spec)).unwrap_or(false)
    }
    pub fn on_fetch_start(&self) -> bool { false }
    pub fn blocks_legacy_writes(&self) -> bool { true }
    pub fn allows_legacy(&self, spec: &V) -> bool { get(spec, "method") == &json!("GET") }
    pub fn status(&self) -> V { self.status_view() }
    pub fn step(&self) -> Result<V, String> {
        need(!*self.busy.borrow() && !*self.closed.borrow(), "BUSY_OR_CLOSED")?;
        *self.busy.borrow_mut() = true;
        let result = (|| -> Result<V, String> {
            if !self.allowed() { return self.publish("operational_stop_or_identity_changed"); }
            let s = self.controller.borrow().as_ref().unwrap().status();
            let o = get(&s, "orders").as_array().map(|os| {
                os.iter().find(|o| get(o, "phase") != &json!("reconciled")).cloned()
            }).flatten();
            let Some(o) = o else { return self.publish("original_lot_reconciled_reserve_retained"); };
            need(get(&o, "orderId") == get(get(&self.descriptor, "neutralAckRevision"), "orderId"), "ORDER_CHANGED")?;
            let leg = get(&s, "planLegs").as_array().map(|ls| {
                ls.iter().find(|l| get(l, "marketId") == get(&o, "marketId") && get(l, "exchangeId") == get(&o, "exchangeId")).cloned()
            }).flatten().unwrap_or(V::Null);
            let evidence = self.bridge.borrow().as_ref().unwrap().refresh_reconciliation(get(&o, "orderId"), &leg)?;
            need(self.allowed(), "STOPPED_AFTER_READS")?;
            self.controller.borrow().as_ref().unwrap().reconcile(&evidence)?;
            *self.last_failure.borrow_mut() = V::Null;
            self.publish("official_scoped_reconciliation")
        })();
        match result {
            Ok(v) => { *self.busy.borrow_mut() = false; Ok(v) }
            Err(e) => {
                let code = if e.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') {
                    e.clone()
                } else { "RECOVERY_EVIDENCE_UNAVAILABLE".into() };
                *self.last_failure.borrow_mut() = json!(code);
                let durable = self.controller.borrow().as_ref().map(|c| get(&c.status(), "failed") == &json!(true)).unwrap_or(false)
                    || self.registry.borrow().as_ref().map(|r| get(&r.status(), "failed") == &json!(true)).unwrap_or(false);
                *self.busy.borrow_mut() = false;
                if durable { *self.failed.borrow_mut() = true; return Err(format!("{e} (fatal)")); }
                self.publish("reconciliation_blocked")
            }
        }
    }
    pub fn close(&self) -> Result<(), String> {
        need(!*self.busy.borrow(), "BUSY")?;
        if *self.closed.borrow() { return Ok(()); }
        *self.closed.borrow_mut() = true;
        self.cleanup()
    }
}
