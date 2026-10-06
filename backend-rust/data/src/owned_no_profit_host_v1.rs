// execution/owned-no-profit-host-v1.mjs — a one-lot SELL wrapper over the
// already verified parent recovery actor. It adds no account client, token,
// timer, new pair or extension of original clocks.
use baal_execution::core::{payload_hash, validate_execution_state};
use baal_execution::gates::normalize_official_fill_tick;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::{instant, price, SCALE, TICK};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, sha256, js_truthy};
use baal_service_runtime::ledger::{acquire_owner_lease, OwnerLease};
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const TID: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
fn need(v: bool, c: &str) -> Result<(), String> {
    if v { Ok(()) } else { Err(format!("OWNED_NO_PROFIT_{c}")) }
}
fn copy(v: &V) -> V { v.clone() }
fn sha(b: &[u8]) -> String { sha256(b) }
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn iso(n: i64) -> String { ms_to_iso(n as f64) }
fn get<'a>(v: &'a V, k: &str) -> &'a V { v.get(k).unwrap_or(&V::Null) }
fn instant_v(v: &V) -> Result<i64, String> { instant(v.as_str().unwrap_or("")) }
fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
pub fn owned_no_profit_host_identity(workspace: &Path) -> V {
    json!(baal_collector::frozen_paper_graph::collect_frozen_paper_files(
        workspace,
        &["execution/owned-no-profit-host-v1.mjs".to_string(), "execution/scoped-rest-receipts.mjs".to_string()],
    ).unwrap_or_default())
}
fn economic(p: &V) -> V {
    let mut m = serde_json::Map::new();
    for k in ["orderId", "intentId", "originalBuyProofHash", "originalLotId", "filledQuantity",
        "scopedProceeds", "originalPrincipalCost", "removedPrincipalCost", "realizedPnl",
        "remainingOwnedLot", "fills"] {
        m.insert(k.to_string(), get(p, k).clone());
    }
    m.insert("originalBefore".into(), get(get(p, "sourceReceipts"), "before").clone());
    m.insert("originalAck".into(), get(get(p, "sourceReceipts"), "ack").clone());
    V::Object(m)
}
struct Source {
    bytes: Vec<u8>,
    journal: Journal,
    rows: Vec<V>,
    hash: String,
}
fn source(file: &Path) -> Result<Source, String> {
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    need(!bytes.is_empty() && bytes.len() <= 16 * 1024 * 1024 && bytes.last() == Some(&10), "SOURCE_BOUND")?;
    let journal = Journal::open(file)?;
    let rows: Vec<V> = String::from_utf8_lossy(&bytes).trim_end().split('\n')
        .map(|l| serde_json::from_str(l).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    Ok(Source { hash: sha(&bytes), bytes, journal, rows })
}

type GetV = Rc<dyn Fn() -> Result<V, String>>;

pub struct OwnedNoProfitHost {
    runtime_dir: PathBuf,
    workspace: PathBuf,
    universe: V,
    request: V,
    request_hash: String,
    recovery_descriptor: V,
    code: V,
    get_account_lease: GetV,
    get_other_state: GetV,
    can_operate: Rc<dyn Fn() -> bool>,
    get_read_hint: GetV,
    now: Rc<dyn Fn() -> i64>,
    dir: PathBuf,
    file: PathBuf,
    actor_file: PathBuf,
    funding_file: PathBuf,
    request_file: PathBuf,
    recovery_file: PathBuf,
    authority_file: PathBuf,
    lease: RefCell<Option<OwnerLease>>,
    journal: RefCell<Option<Journal>>,
    state: RefCell<Option<V>>,
    installed: RefCell<Option<V>>,
    anchor: RefCell<Option<V>>,
    anchor_bytes_hash: RefCell<String>,
    funding: RefCell<Option<Source>>,
    buy: RefCell<V>,
    original_after: RefCell<V>,
    actor_sequence: RefCell<u64>,
    actor_previous: RefCell<String>,
    closed: RefCell<bool>,
    failed: RefCell<bool>,
    busy: RefCell<bool>,
    writing: RefCell<Option<String>>,
    last_failure: RefCell<V>,
    last_action: RefCell<String>,
    local_size: RefCell<usize>,
    local_hash: RefCell<String>,
    retirement_input: RefCell<Option<V>>,
    retirement_reverification: RefCell<Option<V>>,
    exit_status_fn: RefCell<Option<Rc<dyn Fn() -> V>>>,
    exit_verified_proof_fn: RefCell<Option<Rc<dyn Fn() -> V>>>,
    exit_reservation_fn: RefCell<Option<Rc<dyn Fn() -> Result<V, String>>>>,
}

impl OwnedNoProfitHost {
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        runtime_dir: &Path,
        workspace: &Path,
        universe: &V,
        get_account_lease: GetV,
        get_other_state: GetV,
        can_operate: Rc<dyn Fn() -> bool>,
        get_read_hint: GetV,
        now: Rc<dyn Fn() -> i64>,
    ) -> Result<Option<Self>, String> {
        let request_file = runtime_dir.join("owned-no-profit-request.json");
        if !request_file.exists() { return Ok(None); }
        need(runtime_dir.is_absolute() && workspace.is_absolute(), "DEPENDENCIES")?;
        need((can_operate)(), "STOPPED")?;
        let request = read(&request_file)?;
        let request_hash = payload_hash(&request)?;
        let code = owned_no_profit_host_identity(workspace);
        let keys: Vec<String> = ["schemaVersion", "kind", "ownerId", "runId", "tournamentId", "marketId",
            "exchangeId", "recoveryDescriptorHash", "buyProofHash", "startedAt", "entryExpiresAt",
            "authorityHash", "codeIdentity"].iter().map(|s| s.to_string()).collect();
        let mut sorted_keys: Vec<String> = request.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        sorted_keys.sort();
        let mut expected = keys.clone();
        expected.sort();
        need(
            sorted_keys == expected
                && get(&request, "schemaVersion") == &json!(1)
                && get(&request, "kind") == &json!("single_owned_no_protected_profit_exit_v1")
                && get(&request, "ownerId") == &json!("Codex")
                && get(&request, "tournamentId") == &json!(TID)
                && get(&request, "runId").as_str().is_some_and(|s| {
                    s.starts_with("sig-pilot-paired-")
                        && s["sig-pilot-paired-".len()..].len() >= 1
                        && s["sig-pilot-paired-".len()..].len() <= 60
                        && s.bytes().skip("sig-pilot-paired-".len()).all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                })
                && ["marketId", "exchangeId"].iter().all(|k| {
                    get(&request, k).as_str().is_some_and(|s| {
                        !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
                    })
                })
                && ["recoveryDescriptorHash", "buyProofHash", "authorityHash"].iter().all(|k| hex(get(&request, k)))
                && payload_hash(get(&request, "codeIdentity")).ok() == payload_hash(&code).ok(),
            "REQUEST",
        )?;
        let start = instant_v(get(&request, "startedAt"))?;
        let end = instant_v(get(&request, "entryExpiresAt"))?;
        need(start < end && end - start <= 3_600_000 && start <= (now)(), "WINDOW")?;
        let recovery_file = runtime_dir.join("paired-neutral-ack-recovery.json");
        let recovery_descriptor = read(&recovery_file)?;
        let authority_file = runtime_dir.join("paired-live-authority.json");
        let authority = read(&authority_file)?;
        need(
            payload_hash(&recovery_descriptor).ok() == get(&request, "recoveryDescriptorHash").as_str().map(String::from)
                && get(&recovery_descriptor, "ownerId") == get(&request, "ownerId")
                && get(&recovery_descriptor, "runId") == get(&request, "runId")
                && get(&recovery_descriptor, "tournamentId") == &json!(TID)
                && get(&authority, "kind") == &json!("direct_human_authorized_bot_update")
                && get(&authority, "ownerId") == &json!("Codex")
                && get(&authority, "exploratory") == &json!(true)
                && get(&authority, "profitabilityEstablished") == &json!(false)
                && payload_hash(&authority).ok() == get(&request, "authorityHash").as_str().map(String::from)
                && get(&request, "authorityHash") == get(&recovery_descriptor, "authorityHash"),
            "AUTHORITY_OR_RECOVERY",
        )?;
        need(
            get(universe, "tournamentId") == &json!(TID)
                && get(universe, "exchanges").as_array().map(|ms| ms.iter().any(|m| {
                    get(m, "marketId") == get(&request, "marketId")
                        && get(m, "exchangeId") == get(&request, "exchangeId")
                        && get(m, "option") == &json!("YES")
                })).unwrap_or(false),
            "UNIVERSE",
        )?;
        let parent_dir = runtime_dir.join("paired-live-v2-runs").join(get(&request, "runId").as_str().unwrap())
            .join("child-runtime").join("paired-live-runs").join(get(&request, "runId").as_str().unwrap());
        let actor_file = parent_dir.join("controller").join("paired-rest-journal.jsonl");
        let funding_file = parent_dir.join("funding").join("paired-shared-funding.jsonl");
        let dir = parent_dir.join("owned-no-profit-v1");
        let file = dir.join("owned-no-profit-journal.jsonl");
        let this = Self {
            runtime_dir: runtime_dir.to_path_buf(),
            workspace: workspace.to_path_buf(),
            universe: copy(universe),
            request: request.clone(),
            request_hash,
            recovery_descriptor,
            code,
            get_account_lease,
            get_other_state,
            can_operate,
            get_read_hint,
            now,
            dir,
            file,
            actor_file,
            funding_file,
            request_file,
            recovery_file,
            authority_file,
            lease: RefCell::new(None),
            journal: RefCell::new(None),
            state: RefCell::new(None),
            installed: RefCell::new(None),
            anchor: RefCell::new(None),
            anchor_bytes_hash: RefCell::new(String::new()),
            funding: RefCell::new(None),
            buy: RefCell::new(V::Null),
            original_after: RefCell::new(V::Null),
            actor_sequence: RefCell::new(0),
            actor_previous: RefCell::new(String::new()),
            closed: RefCell::new(false),
            failed: RefCell::new(false),
            busy: RefCell::new(false),
            writing: RefCell::new(None),
            last_failure: RefCell::new(V::Null),
            last_action: RefCell::new("loaded".into()),
            local_size: RefCell::new(0),
            local_hash: RefCell::new(String::new()),
            retirement_input: RefCell::new(None),
            retirement_reverification: RefCell::new(None),
            exit_status_fn: RefCell::new(None),
            exit_verified_proof_fn: RefCell::new(None),
            exit_reservation_fn: RefCell::new(None),
        };
        match this.init() {
            Ok(()) => Ok(Some(this)),
            Err(e) => { this.cleanup(); Err(e) }
        }
    }
    fn account_lease(&self) -> Result<V, String> {
        let l = (self.get_account_lease)()?;
        need(
            get(&l, "ownerId") == &json!("Codex") && get(&l, "tournamentId") == &json!(TID) && hex(get(&l, "leaseId")),
            "ACCOUNT_LEASE",
        )?;
        Ok(copy(&l))
    }
    fn lease_hash(l: &V) -> Result<String, String> {
        payload_hash(&json!({
            "ownerId": get(l, "ownerId"),
            "tournamentId": get(l, "tournamentId"),
            "leaseId": get(l, "leaseId"),
        }))
    }
    fn fixed(&self) -> Result<(), String> {
        need(!*self.closed.borrow() && !*self.failed.borrow() && (self.can_operate)(), "STOPPED_OR_FAILED")?;
        if let Some(l) = self.lease.borrow().as_ref() {
            l.assert_ownership().map_err(|e| format!("OWNED_NO_PROFIT_{}", e.code))?;
        }
        need(
            payload_hash(&read(&self.request_file)?).ok() == Some(self.request_hash.clone())
                && payload_hash(&read(&self.recovery_file)?).ok() == get(&self.request, "recoveryDescriptorHash").as_str().map(String::from)
                && payload_hash(&read(&self.authority_file)?).ok() == get(&self.request, "authorityHash").as_str().map(String::from)
                && payload_hash(&owned_no_profit_host_identity(&self.workspace)).ok() == payload_hash(&self.code).ok(),
            "SOURCE_CHANGED",
        )?;
        // leaseIdentityHash is recomputed against the current lease.
        if let Some(_a) = self.anchor.borrow().as_ref() {
            need(
                sha(&std::fs::read(&self.actor_file).map_err(|e| e.to_string())?) == *self.anchor_bytes_hash.borrow()
                    && sha(&std::fs::read(&self.funding_file).map_err(|e| e.to_string())?)
                        == self.funding.borrow().as_ref().map(|f| f.hash.clone()).unwrap_or_default(),
                "PARENT_SOURCE_CHANGED",
            )?;
        }
        Ok(())
    }
    fn allowed(&self) -> bool { self.fixed().is_ok() }
    fn entry_allowed(&self) -> bool {
        let start = instant_v(get(&self.request, "startedAt")).unwrap_or(i64::MIN);
        let end = instant_v(get(&self.request, "entryExpiresAt")).unwrap_or(i64::MAX);
        if !(self.allowed() && (self.now)() >= start && (self.now)() < end) { return false; }
        if let Some(sf) = self.exit_status_fn.borrow().as_ref() {
            let x = sf();
            if !get(&x, "intent").is_null() {
                return instant_v(get(get(&x, "intent"), "body").get("expirationDate").unwrap_or(&V::Null))
                    .map(|v| v <= end).unwrap_or(false);
            }
        }
        true
    }
    fn negative_read_hint(&self) -> bool {
        (|| -> Result<bool, String> {
            let h = (self.get_read_hint)()?;
            need(
                get(&h, "basis") == &json!("documented_bulk_best_ask")
                    && get(&h, "tournamentId") == &json!(TID)
                    && get(&h, "universeHash") == get(&self.universe, "hash")
                    && get(&h, "marketId") == get(&self.request, "marketId")
                    && get(&h, "exchangeId") == get(&self.request, "exchangeId")
                    && hex(get(&h, "sourceHash")),
                "READ_HINT",
            )?;
            let a = instant_v(get(&h, "requestStartedAt"))?;
            let z = instant_v(get(&h, "receivedAt"))?;
            let n = (self.now)();
            need(a <= z && z <= n && n - a <= 20000, "READ_HINT_CLOCK")?;
            let ask = price(get(&normalize_official_fill_tick(get(&h, "bestAsk"))?, "price").as_str().unwrap_or(""))?;
            let cost = price(get(&self.buy.borrow(), "scopedPrincipalCost").as_str().unwrap_or(""))?;
            Ok(ask > SCALE - cost - 2 * TICK)
        })().unwrap_or(false)
    }
    fn core(&self) -> Result<V, String> {
        let s = validate_execution_state(&(self.get_other_state)()?)?;
        need(
            get(&s, "tournamentId") == &json!(TID)
                && !get(&s, "owned").as_object().map(|o| o.values().any(|p| {
                    get(p, "exchangeId") == get(&self.request, "exchangeId")
                        || get(p, "marketId") == get(&self.request, "marketId")
                })).unwrap_or(false),
            "FOREIGN_CORE_HOLDING",
        )?;
        Ok(s)
    }
    fn global_risk(&self) -> Result<V, String> {
        let s = self.core()?;
        let x = self.exit_status_fn.borrow().as_ref().map(|f| f());
        let mut unresolved: Vec<V> = get(&s, "intents").as_object().map(|o| {
            o.values().filter(|i| get(i, "status") != &json!("closed")).map(|i| get(i, "intentId").clone()).collect()
        }).unwrap_or_default();
        if let Some(xv) = &x {
            let intent = get(xv, "intent");
            if !intent.is_null() && get(intent, "proofHash").is_null() {
                unresolved.push(get(intent, "intentId").clone());
            }
        }
        Ok(json!({
            "ownerId": "Codex",
            "runId": get(&self.request, "runId"),
            "tournamentId": TID,
            "complete": true,
            "soleCollector": true,
            "stopped": !self.allowed(),
            "at": iso((self.now)()),
            "unresolvedIntentIds": unresolved,
            "sourceHash": payload_hash(&json!({
                "core": s,
                "parent": self.anchor.borrow().as_ref().map(|a| json!({
                    "hash": self.anchor_bytes_hash.borrow().clone(),
                    "proofHash": get(&self.buy.borrow(), "verificationHash"),
                })).unwrap_or(V::Null),
                "exit": x.unwrap_or(V::Null),
                "lease": self.account_lease()?,
            }))?,
        }))
    }
    fn ownership(&self) -> Result<V, String> {
        self.fixed()?;
        let s = self.state.borrow().clone();
        need(
            s.as_ref().map(|x| get(x, "retirement").is_null()).unwrap_or(true)
                && self.exit_verified_proof_fn.borrow().as_ref()
                    .map(|f| js_int(get(&f(), "filledQuantity")) != Some(1))
                    .unwrap_or(true),
            "LOT_ALREADY_SOLD",
        )?;
        let buy = self.buy.borrow().clone();
        Ok(json!({
            "ownerId": "Codex",
            "runId": get(&self.request, "runId"),
            "tournamentId": TID,
            "marketId": get(&self.request, "marketId"),
            "exchangeId": get(&self.request, "exchangeId"),
            "lotId": get(get(&buy, "lot"), "lotId"),
            "buyProofHash": get(&buy, "verificationHash"),
            "quantity": 1,
            "sourceHash": payload_hash(&json!({
                "actor": self.anchor_bytes_hash.borrow().clone(),
                "proofHash": get(&buy, "verificationHash"),
                "claim": s.as_ref().map(|x| get(x, "reservation").clone()).unwrap_or(V::Null),
            }))?,
            "at": iso((self.now)()),
        }))
    }
    fn genesis(&self) -> V {
        json!({
            "schemaVersion": 1,
            "requestHash": self.request_hash,
            "buyProofHash": get(&self.buy.borrow(), "verificationHash"),
            "parentActorHash": self.anchor_bytes_hash.borrow().clone(),
            "reservation": V::Null,
            "retirement": V::Null,
            "retiredOriginalExitProofHash": V::Null,
        })
    }
    fn validate_state(&self, next: &V, event: &V) -> Result<(), String> {
        let buy = self.buy.borrow().clone();
        need(
            get(next, "requestHash").as_str() == Some(self.request_hash.as_str())
                && get(next, "buyProofHash") == get(&self.request, "buyProofHash")
                && get(next, "parentActorHash").as_str() == Some(self.anchor_bytes_hash.borrow().as_str()),
            "LOCAL_BINDING",
        )?;
        let state = self.state.borrow().clone();
        let expected: V;
        match get(event, "kind").as_str().unwrap_or("") {
            "inventory_claim" => {
                need(state.as_ref().is_some_and(|s| get(s, "retirement").is_null()), "CLAIM_PHASE")?;
                let r = get(event, "record").clone();
                let mut body = r.clone();
                let rh = body.as_object_mut().and_then(|m| m.shift_remove("reservationHash")).unwrap_or(V::Null);
                need(
                    rh.as_str() == payload_hash(&body).ok().as_deref()
                        && get(&r, "buyProofHash") == get(&buy, "verificationHash")
                        && get(&r, "lotId") == get(get(&buy, "lot"), "lotId")
                        && get(&r, "ownerId") == &json!("Codex")
                        && get(&r, "runId") == get(&self.request, "runId")
                        && get(&r, "tournamentId") == &json!(TID)
                        && get(&r, "marketId") == get(&self.request, "marketId")
                        && get(&r, "exchangeId") == get(&self.request, "exchangeId")
                        && get(&r, "originalPrincipalCost") == get(&buy, "scopedPrincipalCost")
                        && get(&r, "cashReserved") == &json!("0.000000")
                        && [0, 1].contains(&js_int(get(&r, "remainingQuantity")).unwrap_or(-1))
                        && get(&r, "sellIntentId").as_str().is_some_and(|s| !s.is_empty()),
                    "CLAIM",
                )?;
                if let Some(s) = &state {
                    if !get(s, "reservation").is_null() {
                        need(
                            get(&r, "sellIntentId") == get(get(s, "reservation"), "sellIntentId"),
                            "SECOND_INTENT",
                        )?;
                        need(
                            js_int(get(&r, "remainingQuantity")).unwrap_or(0)
                                <= js_int(get(get(s, "reservation"), "remainingQuantity")).unwrap_or(0),
                            "LOT_RESTORED",
                        )?;
                    }
                }
                let mut e2 = state.clone().unwrap_or(V::Null);
                e2.as_object_mut().unwrap().insert("reservation".into(), copy(&r));
                expected = e2;
            }
            "retired" => {
                need(
                    state.as_ref().is_some_and(|s| {
                        get(s, "retirement").is_null()
                            && get(get(s, "reservation"), "remainingQuantity") == &json!(0)
                            && get(get(s, "reservation"), "terminalSell") == &json!(true)
                    })
                        && hex(get(event, "originalExitProofHash")),
                    "RETIREMENT_PHASE",
                )?;
                let fresh = crate::no_rest_sell_reconciliation_v1::verify_scoped_rest_no_sell(get(event, "sellReverification"))?;
                let proof = crate::paired_owned_exit_retirement_v1::build_paired_owned_exit_retirement(get(event, "evidence"))?;
                need(
                    payload_hash(get(get(event, "sellReverification"), "originalBuyProof")).ok() == payload_hash(&buy).ok()
                        && payload_hash(&fresh).ok() == payload_hash(get(get(event, "evidence"), "sellProof")).ok()
                        && get(&fresh, "intentId") == get(get(state.as_ref().unwrap(), "reservation"), "sellIntentId")
                        && get(&proof, "originalBuyProofHash") == get(&buy, "verificationHash")
                        && get(get(&proof, "originalActorSource"), "sha256").as_str() == Some(self.anchor_bytes_hash.borrow().as_str()),
                    "RETIREMENT_REPLAY",
                )?;
                need(
                    self.funding.borrow().as_ref().map(|f| f.rows.iter().any(|r| {
                        get(r, "hash") == get(get(get(event, "evidence"), "fundingRegistryRecord"), "hash")
                            && payload_hash(r).ok() == payload_hash(get(get(event, "evidence"), "fundingRegistryRecord")).ok()
                    })).unwrap_or(false),
                    "RETIREMENT_FUNDING_MEMBERSHIP",
                )?;
                let mut e2 = state.clone().unwrap_or(V::Null);
                e2.as_object_mut().unwrap().insert("retirement".into(), proof);
                e2.as_object_mut().unwrap().insert("retiredOriginalExitProofHash".into(), get(event, "originalExitProofHash").clone());
                expected = e2;
            }
            _ => {
                need(get(event, "kind") == &json!("initialized") && state.is_none(), "LOCAL_EVENT")?;
                expected = self.genesis();
            }
        }
        need(payload_hash(next).ok() == payload_hash(&expected).ok(), "LOCAL_REDUCER_STATE")?;
        Ok(())
    }
    fn append(&self, next: &V, event: &V) -> Result<(), String> {
        self.fixed()?;
        let old = std::fs::read(&self.file).map_err(|e| e.to_string())?;
        need(old.len() == *self.local_size.borrow() && sha(&old) == *self.local_hash.borrow(), "LOCAL_JOURNAL_CHANGED")?;
        self.validate_state(next, event)?;
        let result = (|| -> Result<(), String> {
            self.journal.borrow_mut().as_mut().unwrap().commit(next, event)?;
            *self.state.borrow_mut() = Some(copy(next));
            let bytes = std::fs::read(&self.file).map_err(|e| e.to_string())?;
            need(bytes[..old.len()] == old[..], "LOCAL_PREFIX_CHANGED")?;
            *self.local_size.borrow_mut() = bytes.len();
            *self.local_hash.borrow_mut() = sha(&bytes);
            self.fixed()
        })();
        if result.is_err() { *self.failed.borrow_mut() = true; }
        result
    }
    fn claim(&self, record: &V) -> Result<V, String> {
        self.fixed()?;
        let reservation = (self.exit_reservation_fn.borrow().as_ref()
            .ok_or("OWNED_NO_PROFIT_CLAIM_NOT_ACTUAL_CONTROLLER".to_string())?)()?;
        need(payload_hash(record).ok() == payload_hash(&reservation).ok(), "CLAIM_NOT_ACTUAL_CONTROLLER")?;
        need(
            self.global_risk()?.get("unresolvedIntentIds").and_then(|a| a.as_array()).map(|a| {
                a.iter().all(|i| i == get(record, "sellIntentId"))
            }).unwrap_or(false),
            "GLOBAL_UNKNOWN",
        )?;
        let at = iso((self.now)());
        let mut next = self.state.borrow().clone().unwrap();
        next.as_object_mut().unwrap().insert("reservation".into(), copy(record));
        self.append(&next, &json!({"kind": "inventory_claim", "record": record, "at": at}))?;
        let receipt = json!({
            "reservationHash": get(record, "reservationHash"),
            "globalLedgerHash": self.journal.borrow().as_ref().unwrap().previous.clone(),
            "confirmedAt": at,
        });
        *self.installed.borrow_mut() = Some(json!({"record": copy(record), "receipt": receipt}));
        Ok(receipt)
    }
    fn parent_status(&self) -> Result<V, String> {
        let actor = Journal::open(&self.actor_file)?;
        let s = get(actor.last.as_ref().unwrap_or(&V::Null), "state").clone();
        Ok(json!({
            "planHash": get(get(&s, "plan"), "planHash"),
            "sequence": actor.sequence,
            "hash": actor.previous,
            "orders": get(&s, "orders").as_array().map(|os| os.iter().map(|o| json!({
                "phase": get(o, "phase"),
                "orderId": get(o, "orderId"),
                "proofHash": get(get(o, "proof"), "verificationHash"),
            })).collect::<Vec<V>>()).unwrap_or_default(),
            "owned": get(&s, "orders").as_array().map(|os| os.iter().flat_map(|o| {
                if !get(get(o, "proof"), "lot").is_null() {
                    let mut m = json!({"marketId": get(o, "marketId"), "exchangeId": get(get(o, "body"), "exchangeId")});
                    for (k, v) in get(get(o, "proof"), "lot").as_object().unwrap() {
                        m.as_object_mut().unwrap().insert(k.clone(), v.clone());
                    }
                    vec![m]
                } else { vec![] }
            }).collect::<Vec<V>>()).unwrap_or_default(),
        }))
    }
    fn retire(&self, evidence: &V) -> Result<(), String> {
        let exit_proof_fn = self.exit_verified_proof_fn.borrow().clone();
        let original_proof = exit_proof_fn.map(|f| f()).unwrap_or(V::Null);
        let s = self.state.borrow().clone().unwrap();
        if !get(&s, "retirement").is_null() || js_int(get(&original_proof, "filledQuantity")) != Some(1) {
            return Ok(());
        }
        let exit_journal_file = self.dir.join("controller").join("owned-no-exit-journal.jsonl");
        let actual = source(&exit_journal_file)?;
        let o = get(get(actual.journal.last.as_ref().unwrap_or(&V::Null), "state"), "intent").clone();
        let x = self.exit_status_fn.borrow().as_ref().map(|f| f()).unwrap_or(V::Null);
        need(
            actual.journal.sequence == js_int(get(&x, "sequence")).unwrap_or(-1) as u64
                && actual.journal.previous == get(&x, "hash").as_str().unwrap_or("")
                && get(get(&o, "proof"), "verificationHash") == get(&original_proof, "verificationHash"),
            "ACTUAL_EXIT_JOURNAL",
        )?;
        let observed_at = iso((self.now)());
        let buy = self.buy.borrow().clone();
        let sell_reverification = json!({
            "binding": {
                "ownerId": get(&buy, "ownerId"),
                "runId": get(&buy, "runId"),
                "tournamentId": TID,
                "tournamentSlug": "midterm-elections",
                "marketId": get(&buy, "marketId"),
                "exchangeId": get(&buy, "exchangeId"),
                "intentId": get(&o, "intentId"),
                "idempotencyKey": get(get(&o, "body"), "idempotencyKey"),
                "orderId": get(&o, "orderId"),
                "side": "no",
                "action": "sell",
                "quantity": 1,
                "limit": get(get(&o, "signal"), "protectedLimit"),
                "createdAt": get(&o, "createdAt"),
                "expirationDate": get(get(&o, "body"), "expirationDate"),
            },
            "originalBuyProof": buy,
            "originalAfter": self.original_after.borrow().clone(),
            "before": get(&o, "before"),
            "ack": get(&o, "ack"),
            "readAt": observed_at,
            "order": get(evidence, "order"),
            "fillPages": get(evidence, "fillPages"),
            "after": get(evidence, "after"),
            "globalRisk": get(evidence, "globalRisk"),
        });
        let proof = crate::no_rest_sell_reconciliation_v1::verify_scoped_rest_no_sell(&sell_reverification)?;
        need(
            payload_hash(&economic(&proof)).ok() == payload_hash(&economic(&original_proof)).ok(),
            "EXIT_RECONCILIATION_CHANGED",
        )?;
        let l = self.account_lease()?;
        let lease_proof = json!({
            "ownerId": get(&l, "ownerId"),
            "tournamentId": get(&l, "tournamentId"),
            "leaseId": get(&l, "leaseId"),
            "at": observed_at,
            "sourceHash": payload_hash(&json!({
                "observedLease": l,
                "observedAt": observed_at,
                "after": get(get(&proof, "sourceReceipts"), "after"),
            }))?,
        });
        let anchor_bytes = std::fs::read(&self.actor_file).map_err(|e| e.to_string())?;
        let inputs = json!({
            "originalBuyProof": buy,
            "sellProof": proof,
            "sellAfter": get(evidence, "after"),
            "actorJournal": {
                "bodyText": String::from_utf8_lossy(&anchor_bytes).to_string(),
                "sequence": *self.actor_sequence.borrow(),
                "hash": self.actor_previous.borrow().clone(),
                "prefixBytes": get(get(&self.recovery_descriptor, "neutralAckRevision"), "originalPrefixBytes"),
                "prefixSha256": get(get(&self.recovery_descriptor, "neutralAckRevision"), "originalPrefixHash"),
            },
            "actorStatus": self.parent_status()?,
            "fundingRegistryRecord": self.funding.borrow().as_ref().and_then(|f| f.rows.last().cloned()).unwrap_or(V::Null),
            "accountLease": lease_proof,
            "globalRisk": self.global_risk()?,
            "readAt": observed_at,
        });
        let overlay = crate::paired_owned_exit_retirement_v1::build_paired_owned_exit_retirement(&inputs)?;
        let mut next = self.state.borrow().clone().unwrap();
        next.as_object_mut().unwrap().insert("retirement".into(), overlay);
        next.as_object_mut().unwrap().insert("retiredOriginalExitProofHash".into(), get(&original_proof, "verificationHash").clone());
        self.append(&next, &json!({
            "kind": "retired",
            "evidence": inputs,
            "sellReverification": sell_reverification,
            "originalExitProofHash": get(&original_proof, "verificationHash"),
            "at": observed_at,
        }))?;
        Ok(())
    }
    pub fn status(&self) -> V {
        let x = self.exit_status_fn.borrow().as_ref().map(|f| f());
        let p = self.exit_verified_proof_fn.borrow().as_ref().map(|f| f()).unwrap_or(V::Null);
        let s = self.state.borrow().clone();
        let buy = self.buy.borrow().clone();
        let actor_state = Journal::open(&self.actor_file).ok()
            .and_then(|a| a.last.and_then(|l| l.get("state").cloned()));
        json!({
            "schemaVersion": 1,
            "mode": "single_owned_no_protected_profit_host_v1",
            "hostVersion": 4,
            "ownerId": "Codex",
            "runId": get(&self.request, "runId"),
            "tournamentId": TID,
            "marketId": get(&self.request, "marketId"),
            "exchangeId": get(&self.request, "exchangeId"),
            "requestHash": self.request_hash,
            "recoveryDescriptorHash": get(&self.request, "recoveryDescriptorHash"),
            "authorityHash": get(&self.request, "authorityHash"),
            "buyProofHash": get(&buy, "verificationHash"),
            "originalBuyOrderId": get(&buy, "orderId"),
            "originalPrincipalCost": get(&buy, "scopedPrincipalCost"),
            "startedAt": get(&self.request, "startedAt"),
            "entryExpiresAt": get(&self.request, "entryExpiresAt"),
            "originalReviewExpiresAt": actor_state.as_ref().map(|a| get(get(&a, "plan"), "reviewExpiresAt").clone()).unwrap_or(V::Null),
            "phase": if s.as_ref().map(|x| !get(x, "retirement").is_null()).unwrap_or(false) {
                json!("retired")
            } else {
                x.as_ref().map(|v| get(v, "phase").clone()).unwrap_or(json!("awaiting_protected_profit"))
            },
            "entryAllowed": self.entry_allowed(),
            "blocksLegacyWrites": true,
            "orders": x.as_ref().map(|v| get(v, "intent")).filter(|i| !i.is_null()).map(|i| json!([i])).unwrap_or(json!([])),
            "orderCount": if x.as_ref().map(|v| !get(v, "intent").is_null()).unwrap_or(false) { 1 } else { 0 },
            "owned": if js_int(get(&p, "filledQuantity")) == Some(1) { json!([]) } else if !buy.is_null() {
                let mut m = json!({"marketId": get(&buy, "marketId"), "exchangeId": get(&buy, "exchangeId")});
                for (k, v) in get(&buy, "lot").as_object().cloned().unwrap_or_default() {
                    m.as_object_mut().unwrap().insert(k, v);
                }
                json!([m])
            } else { json!([]) },
            "historicalParentFunding": self.funding.borrow().as_ref()
                .and_then(|f| f.journal.last.as_ref().and_then(|l| l.get("state").cloned()))
                .map(|s| get(&s, "funding").clone()).unwrap_or(V::Null),
            "currentBudget": if s.as_ref().map(|x| !get(x, "retirement").is_null()).unwrap_or(false) {
                json!({
                    "ownedPrincipal": "0.000000",
                    "reservedCash": "0.000000",
                    "cumulativeBuy": get(get(s.as_ref().unwrap(), "retirement"), "preservedCumulativeBuy"),
                })
            } else {
                self.funding.borrow().as_ref()
                    .and_then(|f| f.journal.last.as_ref().and_then(|l| l.get("state").cloned()))
                    .map(|s| get(&s, "funding").clone()).unwrap_or(V::Null)
            },
            "realizedPnl": get(&p, "realizedPnl").as_str().map(String::from).unwrap_or("0.000000".into()),
            "scopedProceeds": get(&p, "scopedProceeds").as_str().map(String::from).unwrap_or("0.000000".into()),
            "retirement": s.as_ref().map(|x| get(x, "retirement").clone()).unwrap_or(V::Null),
            "lastAction": self.last_action.borrow().clone(),
            "lastFailure": self.last_failure.borrow().clone(),
            "failed": *self.failed.borrow() || x.as_ref().map(|v| get(v, "failed") == &json!(true)).unwrap_or(false),
            "closed": *self.closed.borrow(),
            "busy": *self.busy.borrow(),
            "additionalAccountClients": 0,
            "additionalPersistentProcesses": 0,
            "profitabilityEstablished": false,
        })
    }
    fn publish(&self, action: &str) -> Result<V, String> {
        *self.last_action.borrow_mut() = action.to_string();
        let mut view = self.status();
        view.as_object_mut().unwrap().insert("observedAt".into(), json!(iso((self.now)())));
        let r = (|| -> Result<(), String> {
            atomic_json(&self.runtime_dir.join("owned-no-profit-stats.json"), &view)?;
            atomic_json(&self.runtime_dir.join("paired-live-stats.json"), &view)?;
            Ok(())
        })();
        if let Err(e) = r { *self.failed.borrow_mut() = true; return Err(e); }
        Ok(self.status())
    }
    fn cleanup(&self) {
        if let Some(l) = self.lease.borrow_mut().as_mut() { let _ = l.release(); }
    }
    /// Wire the recovery host + evidence bridge + exit controller. Called by
    /// `open` once the request/descriptor checks pass.
    pub fn attach_exit<F, G, H>(&mut self, status: F, proof: G, reservation: H)
    where
        F: Fn() -> V + 'static,
        G: Fn() -> V + 'static,
        H: Fn() -> Result<V, String> + 'static,
    {
        *self.exit_status_fn.borrow_mut() = Some(Rc::new(status));
        *self.exit_verified_proof_fn.borrow_mut() = Some(Rc::new(proof));
        *self.exit_reservation_fn.borrow_mut() = Some(Rc::new(reservation));
    }
    fn init(&self) -> Result<(), String> {
        self.fixed()?;
        // Recovery host is opened by the caller via the prepared evidence
        // bridge; the parent actor journal is verified here.
        let anchor = source(&self.actor_file)?;
        *self.actor_sequence.borrow_mut() = anchor.journal.sequence;
        *self.actor_previous.borrow_mut() = anchor.journal.previous.clone();
        *self.anchor_bytes_hash.borrow_mut() = anchor.hash.clone();
        *self.anchor.borrow_mut() = Some(json!({"hash": anchor.hash.clone()}));
        *self.funding.borrow_mut() = Some(source(&self.funding_file)?);
        let orders = get(get(anchor.journal.last.as_ref().unwrap_or(&V::Null), "state"), "orders").as_array().cloned().unwrap_or_default();
        need(
            orders.len() == 1
                && get(&orders[0], "phase") == &json!("reconciled")
                && !js_truthy(orders[0].get("semanticMismatch"))
                && !js_truthy(orders[0].get("cancelPending"))
                && get(get(&orders[0], "proof"), "verificationHash") == get(&self.request, "buyProofHash"),
            "VERIFIED_BUY_REQUIRED",
        )?;
        let buy = copy(get(&orders[0], "proof"));
        need(
            get(&buy, "scopedLotVerified") == &json!(true)
                && get(&buy, "filledQuantity") == &json!(1)
                && get(&buy, "scopedPrincipalCost") == &json!("0.070000")
                && get(&buy, "marketId") == get(&self.request, "marketId")
                && get(&buy, "exchangeId") == get(&self.request, "exchangeId")
                && get(&buy, "ownerId") == &json!("Codex")
                && get(&buy, "runId") == get(&self.request, "runId")
                && get(&buy, "tournamentId") == &json!(TID),
            "BUY_SCOPE",
        )?;
        let reconciliation = anchor.rows.iter().rev().find(|r| {
            get(get(r, "event"), "input").get("kind") == Some(&json!("reconciled"))
                && get(get(r, "state"), "orders").as_array().and_then(|os| os.first())
                    .map(|o| get(get(o, "proof"), "verificationHash") == get(&self.request, "buyProofHash")).unwrap_or(false)
        }).cloned().ok_or("OWNED_NO_PROFIT_BUY_SOURCE_EVENT")?;
        need(!get(get(get(&reconciliation, "event"), "input"), "evidence").get("after").map(|x| !x.is_null()).unwrap_or(false), "BUY_SOURCE_EVENT")?;
        *self.original_after.borrow_mut() = copy(get(get(get(&reconciliation, "event"), "input"), "evidence").get("after").unwrap());
        *self.buy.borrow_mut() = buy;
        self.core()?;
        let lease = acquire_owner_lease(
            &self.dir,
            "Codex",
            get(&self.request, "runId").as_str().unwrap_or(""),
            "owned-no-profit-host-v1",
            (self.now)() as f64,
        ).map_err(|e| format!("OWNED_NO_PROFIT_{}", e.code))?;
        *self.lease.borrow_mut() = Some(lease);
        if !self.file.exists() {
            let f = std::fs::OpenOptions::new().write(true).create_new(true).open(&self.file).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&self.file, std::fs::Permissions::from_mode(0o600)).ok(); }
            drop(f);
        }
        let bytes = std::fs::read(&self.file).map_err(|e| e.to_string())?;
        need(bytes.is_empty() || bytes.last() == Some(&10), "LOCAL_PARTIAL")?;
        *self.journal.borrow_mut() = Some(Journal::open(&self.file)?);
        for line in String::from_utf8_lossy(&bytes).split('\n').filter(|l| !l.is_empty()) {
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            self.validate_state(get(&row, "state"), get(&row, "event"))?;
            *self.state.borrow_mut() = Some(copy(get(&row, "state")));
            if get(&row, "event").get("kind") == Some(&json!("inventory_claim")) {
                let record = get(get(&row, "event"), "record").clone();
                *self.installed.borrow_mut() = Some(json!({
                    "record": copy(&record),
                    "receipt": {
                        "reservationHash": get(&record, "reservationHash"),
                        "globalLedgerHash": get(&row, "hash"),
                        "confirmedAt": get(get(&row, "event"), "at"),
                    },
                }));
            }
            if get(&row, "event").get("kind") == Some(&json!("retired")) {
                *self.retirement_input.borrow_mut() = Some(get(&row, "event").clone());
            }
        }
        *self.local_size.borrow_mut() = bytes.len();
        *self.local_hash.borrow_mut() = sha(&bytes);
        if self.state.borrow().is_none() {
            let g = self.genesis();
            self.append(&g, &json!({"kind": "initialized", "at": iso((self.now)())}))?;
        }
        Ok(())
    }
    pub fn close(&self) -> Result<(), String> {
        need(!*self.busy.borrow(), "BUSY")?;
        if *self.closed.borrow() { return Ok(()); }
        *self.closed.borrow_mut() = true;
        self.cleanup();
        Ok(())
    }
}
