// execution/neutral-sell-recovery-host-v1.mjs — cleanup authority only: the
// old BUY parent and existing SELL are retained. No order submission,
// cancellation, key retry, hedge, or window renewal exists.
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::instant;
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, sha256, js_truthy};
use baal_service_runtime::ledger::{acquire_owner_lease, OwnerLease};
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const T: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
fn need(v: bool, c: &str) -> Result<(), String> {
    if v { Ok(()) } else { Err(format!("NEUTRAL_SELL_RECOVERY_{c}")) }
}
fn copy(v: &V) -> V { v.clone() }
fn sha(b: &[u8]) -> String { sha256(b) }
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn iso(n: i64) -> String { ms_to_iso(n as f64) }
fn get<'a>(v: &'a V, k: &str) -> &'a V { v.get(k).unwrap_or(&V::Null) }
fn read(p: &Path) -> Result<V, String> {
    serde_json::from_str(&std::fs::read_to_string(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
pub fn neutral_sell_recovery_host_identity(workspace: &Path) -> V {
    json!(baal_collector::frozen_paper_graph::collect_frozen_paper_files(
        workspace,
        &["execution/neutral-sell-recovery-host-v1.mjs".to_string(), "execution/scoped-rest-receipts.mjs".to_string()],
    ).unwrap_or_default())
}
struct Source {
    bytes: Vec<u8>,
    journal: Journal,
    rows: Vec<V>,
    hash: String,
}
fn source(file: &Path) -> Result<Source, String> {
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    need(!bytes.is_empty() && bytes.len() <= 64 * 1024 * 1024 && bytes.last() == Some(&10), "SOURCE_BOUND")?;
    let journal = Journal::open(file)?;
    let rows: Vec<V> = String::from_utf8_lossy(&bytes).trim_end().split('\n')
        .map(|l| serde_json::from_str(l).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    Ok(Source { hash: sha(&bytes), bytes, journal, rows })
}
fn economic(p: &V) -> V {
    let mut m = serde_json::Map::new();
    for k in ["orderId", "intentId", "originalBuyProofHash", "originalLotId", "filledQuantity",
        "scopedProceeds", "originalPrincipalCost", "removedPrincipalCost", "realizedPnl",
        "remainingOwnedLot", "fills", "ackEconomics"] {
        m.insert(k.to_string(), get(p, k).clone());
    }
    m.insert("originalBefore".into(), get(get(p, "sourceReceipts"), "before").clone());
    m.insert("originalAck".into(), get(get(p, "sourceReceipts"), "ack").clone());
    V::Object(m)
}

type GetV = Rc<dyn Fn() -> Result<V, String>>;

pub struct NeutralSellRecoveryHost {
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
    request_file: PathBuf,
    request: V,
    request_hash: String,
    code: V,
    profit_file: PathBuf,
    profit: V,
    authority_file: PathBuf,
    recovery_file: PathBuf,
    recovery_descriptor: V,
    dir: PathBuf,
    actor_file: PathBuf,
    funding_file: PathBuf,
    old_file: PathBuf,
    exit_file: PathBuf,
    file: PathBuf,
    recovery: RefCell<Option<Rc<crate::paired_neutral_recovery_host_v1::PairedNeutralRecoveryHost>>>,
    exit: RefCell<Option<Rc<crate::owned_no_exit_controller_v2::OwnedNoExitControllerV2>>>,
    bridge: RefCell<Option<Rc<crate::owned_no_exit_evidence_bridge_v1::OwnedNoExitEvidenceBridge>>>,
    lease: RefCell<Option<OwnerLease>>,
    journal: RefCell<Option<Journal>>,
    state: RefCell<Option<V>>,
    buy: RefCell<V>,
    original_after: RefCell<V>,
    anchor: RefCell<Option<Source>>,
    funding: RefCell<Option<Source>>,
    old: RefCell<Option<Source>>,
    old_state: RefCell<Option<V>>,
    installed: RefCell<Option<V>>,
    closed: RefCell<bool>,
    failed: RefCell<bool>,
    busy: RefCell<bool>,
    last_failure: RefCell<V>,
    last_action: RefCell<String>,
    local_size: RefCell<usize>,
    local_hash: RefCell<String>,
    lease_hash: String,
}

impl NeutralSellRecoveryHost {
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
        let request_file = runtime_dir.join("neutral-sell-recovery-request.json");
        if !request_file.exists() { return Ok(None); }
        need(runtime_dir.is_absolute() && workspace.is_absolute(), "DEPENDENCIES")?;
        need((can_operate)(), "STOPPED")?;
        let request = read(&request_file)?;
        let request_hash = payload_hash(&request)?;
        let code = neutral_sell_recovery_host_identity(workspace);
        let keys = ["schemaVersion", "kind", "ownerId", "runId", "tournamentId", "profitRequestHash",
            "authorityHash", "oldProfitJournalHash", "oldProfitJournalBytes", "startedAt", "expiresAt",
            "codeIdentity", "neutralSellRevision"];
        let mut sorted: Vec<String> = request.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        sorted.sort();
        let mut expected: Vec<String> = keys.iter().map(|s| s.to_string()).collect();
        expected.sort();
        need(
            sorted == expected
                && get(&request, "schemaVersion") == &json!(1)
                && get(&request, "kind") == &json!("single_owned_no_neutral_sell_recovery_v1")
                && get(&request, "ownerId") == &json!("Codex")
                && get(&request, "tournamentId") == &json!(T)
                && get(&request, "runId").as_str().is_some_and(|s| {
                    s.starts_with("sig-pilot-paired-")
                        && s.len() > "sig-pilot-paired-".len() && s.len() <= "sig-pilot-paired-".len() + 60
                        && s.bytes().skip("sig-pilot-paired-".len()).all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                })
                && ["profitRequestHash", "authorityHash", "oldProfitJournalHash"].iter().all(|k| hex(get(&request, k)))
                && get(&request, "oldProfitJournalBytes").as_i64().is_some_and(|n| n > 0)
                && payload_hash(get(&request, "codeIdentity")).ok() == payload_hash(&code).ok(),
            "REQUEST",
        )?;
        need(
            instant(get(&request, "startedAt").as_str().unwrap_or(""))? <= (now)()
                && instant(get(&request, "expiresAt").as_str().unwrap_or(""))?
                    > instant(get(&request, "startedAt").as_str().unwrap_or(""))?,
            "DESCRIPTOR_CLOCK",
        )?;
        let profit_file = runtime_dir.join("owned-no-profit-request.json");
        let profit = read(&profit_file)?;
        let authority_file = runtime_dir.join("paired-live-authority.json");
        let authority = read(&authority_file)?;
        let recovery_file = runtime_dir.join("paired-neutral-ack-recovery.json");
        let recovery_descriptor = read(&recovery_file)?;
        need(
            payload_hash(&profit).ok() == get(&request, "profitRequestHash").as_str().map(String::from)
                && get(&profit, "runId") == get(&request, "runId")
                && get(&profit, "ownerId") == &json!("Codex")
                && get(&profit, "tournamentId") == &json!(T)
                && get(&profit, "authorityHash") == get(&request, "authorityHash")
                && payload_hash(get(&profit, "codeIdentity")).ok()
                    == payload_hash(&crate::owned_no_profit_host_v1::owned_no_profit_host_identity(workspace)).ok()
                && payload_hash(&authority).ok() == get(&request, "authorityHash").as_str().map(String::from)
                && get(&authority, "kind") == &json!("direct_human_authorized_bot_update")
                && get(&authority, "ownerId") == &json!("Codex")
                && get(&authority, "exploratory") == &json!(true)
                && get(&authority, "profitabilityEstablished") == &json!(false)
                && payload_hash(&recovery_descriptor).ok() == get(&profit, "recoveryDescriptorHash").as_str().map(String::from),
            "ORIGINAL_AUTHORITY",
        )?;
        let run_id = get(&request, "runId").as_str().unwrap().to_string();
        let parent_dir = runtime_dir.join("paired-live-v2-runs").join(&run_id)
            .join("child-runtime").join("paired-live-runs").join(&run_id);
        let actor_file = parent_dir.join("controller").join("paired-rest-journal.jsonl");
        let funding_file = parent_dir.join("funding").join("paired-shared-funding.jsonl");
        let dir = parent_dir.join("owned-no-profit-v1");
        let old_file = dir.join("owned-no-profit-journal.jsonl");
        let exit_dir = dir.join("controller");
        let exit_file = exit_dir.join("owned-no-exit-journal.jsonl");
        let file = dir.join("neutral-sell-repair-journal.jsonl");
        let first_lease = get_account_lease()?;
        need(
            get(&first_lease, "ownerId") == &json!("Codex") && get(&first_lease, "tournamentId") == &json!(T) && hex(get(&first_lease, "leaseId")),
            "ACCOUNT_LEASE",
        )?;
        let lease_hash = payload_hash(&json!({
            "ownerId": get(&first_lease, "ownerId"),
            "tournamentId": get(&first_lease, "tournamentId"),
            "leaseId": get(&first_lease, "leaseId"),
        }))?;
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
            request_file,
            request,
            request_hash,
            code,
            profit_file,
            profit,
            authority_file,
            recovery_file,
            recovery_descriptor,
            dir,
            actor_file,
            funding_file,
            old_file,
            exit_file,
            file,
            recovery: RefCell::new(None),
            exit: RefCell::new(None),
            bridge: RefCell::new(None),
            lease: RefCell::new(None),
            journal: RefCell::new(None),
            state: RefCell::new(None),
            buy: RefCell::new(V::Null),
            original_after: RefCell::new(V::Null),
            anchor: RefCell::new(None),
            funding: RefCell::new(None),
            old: RefCell::new(None),
            old_state: RefCell::new(None),
            installed: RefCell::new(None),
            closed: RefCell::new(false),
            failed: RefCell::new(false),
            busy: RefCell::new(false),
            last_failure: RefCell::new(V::Null),
            last_action: RefCell::new("loaded".into()),
            local_size: RefCell::new(0),
            local_hash: RefCell::new(String::new()),
            lease_hash,
        });
        let result = this.init();
        if let Err(e) = result {
            let durable = ["EIO", "ENOSPC", "EROFS", "EPERM", "EACCES", "EBUSY"].iter().any(|c| e.contains(c));
            if durable { *this.failed.borrow_mut() = true; }
            let _ = this.cleanup();
            return Err(e);
        }
        Ok(Some(this))
    }
    fn account_lease(&self) -> Result<V, String> {
        let l = (self.get_account_lease)()?;
        need(
            get(&l, "ownerId") == &json!("Codex") && get(&l, "tournamentId") == &json!(T) && hex(get(&l, "leaseId")),
            "ACCOUNT_LEASE",
        )?;
        Ok(copy(&l))
    }
    fn lease_key(l: &V) -> Result<String, String> {
        payload_hash(&json!({
            "ownerId": get(l, "ownerId"),
            "tournamentId": get(l, "tournamentId"),
            "leaseId": get(l, "leaseId"),
        }))
    }
    fn fixed(&self) -> Result<(), String> {
        need(!*self.closed.borrow() && !*self.failed.borrow() && (self.can_operate)(), "STOPPED_OR_FAILED")?;
        if let Some(l) = self.lease.borrow().as_ref() {
            l.assert_ownership().map_err(|e| format!("NEUTRAL_SELL_RECOVERY_{}", e.code))?;
        }
        need(
            payload_hash(&read(&self.request_file)?).ok() == Some(self.request_hash.clone())
                && payload_hash(&read(&self.profit_file)?).ok() == get(&self.request, "profitRequestHash").as_str().map(String::from)
                && payload_hash(&read(&self.authority_file)?).ok() == get(&self.request, "authorityHash").as_str().map(String::from)
                && payload_hash(&read(&self.recovery_file)?).ok() == get(&self.profit, "recoveryDescriptorHash").as_str().map(String::from)
                && payload_hash(&neutral_sell_recovery_host_identity(&self.workspace)).ok() == payload_hash(&self.code).ok(),
            "SOURCE_CHANGED",
        )?;
        need(Self::lease_key(&self.account_lease()?) == Ok(self.lease_hash.clone()), "LEASE_CHANGED")?;
        if let Some(anchor) = self.anchor.borrow().as_ref() {
            need(
                sha(&std::fs::read(&self.actor_file).map_err(|e| e.to_string())?) == anchor.hash
                    && sha(&std::fs::read(&self.funding_file).map_err(|e| e.to_string())?)
                        == self.funding.borrow().as_ref().map(|f| f.hash.clone()).unwrap_or_default(),
                "PARENT_CHANGED",
            )?;
        }
        if let Some(old) = self.old.borrow().as_ref() {
            need(std::fs::read(&self.old_file).map_err(|e| e.to_string())? == old.bytes, "OLD_PROFIT_CHANGED")?;
        }
        Ok(())
    }
    fn allowed(&self) -> bool { self.fixed().is_ok() }
    fn core(&self) -> Result<V, String> {
        let s = baal_execution::core::validate_execution_state(&(self.get_other_state)()?)?;
        need(
            get(&s, "tournamentId") == &json!(T)
                && !get(&s, "owned").as_object().map(|o| o.values().any(|p| {
                    get(p, "marketId") == get(&self.profit, "marketId") || get(p, "exchangeId") == get(&self.profit, "exchangeId")
                })).unwrap_or(false),
            "FOREIGN_CORE_HOLDING",
        )?;
        Ok(s)
    }
    fn global_risk(&self) -> Result<V, String> {
        let s = self.core()?;
        let x = self.exit.borrow().as_ref().map(|e| e.status());
        let mut unresolved: Vec<V> = get(&s, "intents").as_object().map(|o| {
            o.values().filter(|i| get(i, "status") != &json!("closed")).map(|i| get(i, "intentId").clone()).collect()
        }).unwrap_or_default();
        if let Some(xv) = &x {
            if !get(xv, "intent").is_null() && get(get(xv, "intent"), "proofHash").is_null() {
                unresolved.push(get(get(xv, "intent"), "intentId").clone());
            }
        }
        Ok(json!({
            "ownerId": "Codex",
            "runId": get(&self.request, "runId"),
            "tournamentId": T,
            "complete": true,
            "soleCollector": true,
            "stopped": !self.allowed(),
            "at": iso((self.now)()),
            "unresolvedIntentIds": unresolved,
            "sourceHash": payload_hash(&json!({
                "core": s,
                "actorHash": self.anchor.borrow().as_ref().map(|a| a.hash.clone()),
                "exit": x.unwrap_or(V::Null),
                "lease": self.account_lease()?,
            }))?,
        }))
    }
    fn expected_claim(&self, quantity: i64, terminal: bool) -> Result<V, String> {
        let buy = self.buy.borrow().clone();
        let mut b = json!({
            "schemaVersion": 1,
            "kind": "single_owned_no_exit_reservation",
            "ownerId": get(&buy, "ownerId"),
            "runId": get(&buy, "runId"),
            "tournamentId": T,
            "marketId": get(&buy, "marketId"),
            "exchangeId": get(&buy, "exchangeId"),
            "lotId": get(get(&buy, "lot"), "lotId"),
            "buyProofHash": get(&buy, "verificationHash"),
            "originalPrincipalCost": get(&buy, "scopedPrincipalCost"),
            "cashReserved": "0.000000",
            "sellIntentId": get(get(&self.exit.borrow().as_ref().map(|e| e.status()).unwrap_or(V::Null), "intent"), "intentId"),
            "remainingQuantity": quantity,
            "terminalSell": terminal,
        });
        let h = payload_hash(&b)?;
        b.as_object_mut().unwrap().insert("reservationHash".into(), json!(h));
        Ok(b)
    }
    fn genesis(&self) -> V {
        json!({
            "schemaVersion": 1,
            "requestHash": self.request_hash,
            "buyProofHash": get(&self.buy.borrow(), "verificationHash"),
            "parentActorHash": self.anchor.borrow().as_ref().map(|a| a.hash.clone()),
            "oldProfitJournalHash": self.old.borrow().as_ref().map(|o| o.hash.clone()),
            "reservation": copy(self.old_state.borrow().as_ref().map(|s| get(s, "reservation")).unwrap_or(&V::Null)),
            "retirement": V::Null,
            "retiredOriginalExitProofHash": V::Null,
        })
    }
    fn validate(&self, next: &V, event: &V) -> Result<(), String> {
        let state = self.state.borrow().clone();
        let expected: V;
        match get(event, "kind").as_str().unwrap_or("") {
            "initialized" => {
                need(state.is_none(), "DUPLICATE_GENESIS")?;
                expected = self.genesis();
            }
            "inventory_claim" => {
                need(
                    state.is_some()
                        && get(state.as_ref().unwrap(), "retirement").is_null()
                        && js_int(get(&self.exit.borrow().as_ref().map(|e| e.verified_exit_proof()).unwrap_or(V::Null), "filledQuantity")) == Some(1)
                        && payload_hash(get(event, "record")).ok() == payload_hash(&self.expected_claim(0, true)?).ok(),
                    "CLAIM_PROOF",
                )?;
                let mut e2 = state.unwrap();
                e2.as_object_mut().unwrap().insert("reservation".into(), copy(get(event, "record")));
                expected = e2;
            }
            "retired" => {
                need(
                    state.is_some()
                        && get(state.as_ref().unwrap(), "retirement").is_null()
                        && get(get(state.as_ref().unwrap(), "reservation"), "remainingQuantity") == &json!(0),
                    "RETIREMENT_PHASE",
                )?;
                let original = self.exit.borrow().as_ref().map(|e| e.verified_exit_proof()).unwrap_or(V::Null);
                let fresh = crate::no_rest_sell_reconciliation_v2::verify_scoped_rest_no_sell_v2(get(event, "sellReverification"))?;
                let overlay = crate::paired_owned_exit_retirement_v2::build_paired_owned_exit_retirement_v2(get(event, "evidence"))?;
                need(
                    get(&original, "verificationHash") == get(event, "originalExitProofHash")
                        && payload_hash(&economic(&original)).ok() == payload_hash(&economic(&fresh)).ok()
                        && payload_hash(get(get(event, "sellReverification"), "originalBuyProof")).ok() == payload_hash(&self.buy.borrow()).ok()
                        && payload_hash(&fresh).ok() == payload_hash(get(get(event, "evidence"), "sellProof")).ok()
                        && get(&overlay, "originalBuyProofHash") == get(&self.buy.borrow(), "verificationHash")
                        && get(get(&overlay, "originalActorSource"), "sha256").as_str()
                            == self.anchor.borrow().as_ref().map(|a| a.hash.as_str())
                        && self.funding.borrow().as_ref().map(|f| f.rows.iter().any(|r| {
                            get(r, "hash") == get(get(get(event, "evidence"), "fundingRegistryRecord"), "hash")
                                && payload_hash(r).ok() == payload_hash(get(get(event, "evidence"), "fundingRegistryRecord")).ok()
                        })).unwrap_or(false),
                    "RETIREMENT_SOURCE_MEMBERSHIP",
                )?;
                let mut e2 = state.unwrap();
                e2.as_object_mut().unwrap().insert("retirement".into(), overlay);
                e2.as_object_mut().unwrap().insert("retiredOriginalExitProofHash".into(), get(&original, "verificationHash").clone());
                expected = e2;
            }
            _ => return Err("NEUTRAL_SELL_RECOVERY_EXACT_REDUCER".into()),
        }
        need(payload_hash(next).ok() == payload_hash(&expected).ok(), "EXACT_REDUCER")?;
        Ok(())
    }
    fn append(&self, next: &V, event: &V) -> Result<(), String> {
        self.fixed()?;
        let before = std::fs::read(&self.file).map_err(|e| e.to_string())?;
        need(before.len() == *self.local_size.borrow() && sha(&before) == *self.local_hash.borrow(), "LOCAL_JOURNAL_CHANGED")?;
        self.validate(next, event)?;
        let result = (|| -> Result<(), String> {
            self.journal.borrow_mut().as_mut().unwrap().commit(next, event)?;
            *self.state.borrow_mut() = Some(copy(next));
            let bytes = std::fs::read(&self.file).map_err(|e| e.to_string())?;
            need(bytes[..before.len()] == before[..], "LOCAL_PREFIX_CHANGED")?;
            *self.local_size.borrow_mut() = bytes.len();
            *self.local_hash.borrow_mut() = sha(&bytes);
            self.fixed()
        })();
        if result.is_err() { *self.failed.borrow_mut() = true; }
        result
    }
    fn claim(&self, record: &V) -> Result<V, String> {
        self.fixed()?;
        need(
            payload_hash(record).ok()
                == self.exit.borrow().as_ref().and_then(|e| e.exit_reservation().ok()).map(|r| payload_hash(&r).ok()).flatten(),
            "CLAIM_CONTROLLER",
        )?;
        let at = iso((self.now)());
        let mut next = self.state.borrow().clone().unwrap_or(V::Null);
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
    fn parent_status(&self) -> V {
        let anchor = self.anchor.borrow();
        let a = anchor.as_ref().unwrap();
        let s = get(a.journal.last.as_ref().unwrap_or(&V::Null), "state").clone();
        json!({
            "planHash": get(get(&s, "plan"), "planHash"),
            "sequence": a.journal.sequence,
            "hash": a.journal.previous,
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
        })
    }
    fn retire(&self, evidence: &V) -> Result<(), String> {
        let original = self.exit.borrow().as_ref().map(|e| e.verified_exit_proof()).unwrap_or(V::Null);
        need(js_int(get(&original, "filledQuantity")) == Some(1), "VERIFIED_SALE")?;
        let actual = source(&self.exit_file)?;
        let o = get(get(actual.journal.last.as_ref().unwrap_or(&V::Null), "state"), "intent").clone();
        let x = self.exit.borrow().as_ref().map(|e| e.status()).unwrap_or(V::Null);
        need(
            actual.journal.sequence == js_int(get(&x, "sequence")).unwrap_or(-1) as u64
                && actual.journal.previous == get(&x, "hash").as_str().unwrap_or("")
                && get(get(&o, "proof"), "verificationHash") == get(&original, "verificationHash"),
            "ACTUAL_EXIT_JOURNAL",
        )?;
        let read_at = iso((self.now)());
        let buy = self.buy.borrow().clone();
        let mut sell_reverification = copy(evidence);
        let sr = sell_reverification.as_object_mut().unwrap();
        sr.insert("binding".into(), json!({
            "ownerId": get(&buy, "ownerId"),
            "runId": get(&buy, "runId"),
            "tournamentId": T,
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
        }));
        sr.insert("originalBuyProof".into(), copy(&buy));
        sr.insert("originalAfter".into(), copy(&self.original_after.borrow()));
        sr.insert("before".into(), copy(get(&o, "before")));
        sr.insert("ack".into(), copy(get(&o, "ack")));
        sr.insert("readAt".into(), json!(read_at));
        let fresh = crate::no_rest_sell_reconciliation_v2::verify_scoped_rest_no_sell_v2(&sell_reverification)?;
        need(
            payload_hash(&economic(&original)).ok() == payload_hash(&economic(&fresh)).ok(),
            "SALE_ECONOMICS_CHANGED",
        )?;
        let l = self.account_lease()?;
        let at = iso((self.now)());
        let risk = self.global_risk()?;
        let final_at = iso((self.now)());
        let anchor = self.anchor.borrow();
        let anchor_ref = anchor.as_ref().unwrap();
        let inputs = json!({
            "originalBuyProof": buy,
            "sellProof": fresh,
            "sellAfter": get(evidence, "after"),
            "actorJournal": {
                "bodyText": String::from_utf8_lossy(&anchor_ref.bytes).to_string(),
                "sequence": anchor_ref.journal.sequence,
                "hash": anchor_ref.journal.previous,
                "prefixBytes": get(get(&self.recovery_descriptor, "neutralAckRevision"), "originalPrefixBytes"),
                "prefixSha256": get(get(&self.recovery_descriptor, "neutralAckRevision"), "originalPrefixHash"),
            },
            "actorStatus": self.parent_status(),
            "fundingRegistryRecord": self.funding.borrow().as_ref().and_then(|f| f.rows.last().cloned()).unwrap_or(V::Null),
            "accountLease": {
                "ownerId": get(&l, "ownerId"),
                "tournamentId": get(&l, "tournamentId"),
                "leaseId": get(&l, "leaseId"),
                "at": at,
                "sourceHash": payload_hash(&json!({
                    "lease": l,
                    "at": at,
                    "after": get(get(&fresh, "sourceReceipts"), "after"),
                }))?,
            },
            "globalRisk": risk,
            "readAt": final_at,
        });
        let overlay = crate::paired_owned_exit_retirement_v2::build_paired_owned_exit_retirement_v2(&inputs)?;
        let mut next = self.state.borrow().clone().unwrap_or(V::Null);
        next.as_object_mut().unwrap().insert("retirement".into(), overlay);
        next.as_object_mut().unwrap().insert("retiredOriginalExitProofHash".into(), get(&original, "verificationHash").clone());
        self.append(&next, &json!({
            "kind": "retired",
            "evidence": inputs,
            "sellReverification": sell_reverification,
            "originalExitProofHash": get(&original, "verificationHash"),
            "at": final_at,
        }))?;
        Ok(())
    }
    fn status_view(&self) -> V {
        let x = self.exit.borrow().as_ref().map(|e| e.status());
        let p = self.exit.borrow().as_ref().map(|e| e.verified_exit_proof()).unwrap_or(V::Null);
        let state = self.state.borrow().clone();
        let buy = self.buy.borrow().clone();
        json!({
            "schemaVersion": 1,
            "mode": "single_owned_no_neutral_sell_recovery_host_v1",
            "hostVersion": 5,
            "ownerId": "Codex",
            "runId": get(&self.request, "runId"),
            "tournamentId": T,
            "marketId": get(&self.profit, "marketId"),
            "exchangeId": get(&self.profit, "exchangeId"),
            "requestHash": self.request_hash,
            "profitRequestHash": get(&self.request, "profitRequestHash"),
            "authorityHash": get(&self.request, "authorityHash"),
            "buyProofHash": get(&buy, "verificationHash"),
            "originalBuyOrderId": get(&buy, "orderId"),
            "originalPrincipalCost": get(&buy, "scopedPrincipalCost"),
            "startedAt": get(&self.request, "startedAt"),
            "entryExpiresAt": get(&self.profit, "entryExpiresAt"),
            "recoveryDescriptorExpiresAt": get(&self.request, "expiresAt"),
            "cleanupAllowedAfterDescriptorExpiry": true,
            "originalReviewExpiresAt": self.anchor.borrow().as_ref()
                .and_then(|a| a.journal.last.as_ref().and_then(|l| l.get("state").cloned()))
                .map(|s| get(get(&s, "plan"), "reviewExpiresAt").clone()).unwrap_or(V::Null),
            "phase": if state.as_ref().map(|s| !get(s, "retirement").is_null()).unwrap_or(false) {
                json!("retired")
            } else {
                x.as_ref().map(|v| get(v, "phase").clone()).unwrap_or(json!("recovery_pending"))
            },
            "orders": x.as_ref().map(|v| if get(v, "intent").is_null() { json!([]) } else { json!([copy(get(v, "intent"))]) }).unwrap_or(json!([])),
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
            "currentBudget": if state.as_ref().map(|s| !get(s, "retirement").is_null()).unwrap_or(false) {
                json!({
                    "ownedPrincipal": "0.000000",
                    "reservedCash": "0.000000",
                    "cumulativeBuy": "0.070000",
                })
            } else {
                self.funding.borrow().as_ref()
                    .and_then(|f| f.journal.last.as_ref().and_then(|l| l.get("state").cloned()))
                    .map(|s| get(&s, "funding").clone()).unwrap_or(V::Null)
            },
            "realizedPnl": get(&p, "realizedPnl").as_str().map(String::from).unwrap_or("0.000000".into()),
            "scopedProceeds": get(&p, "scopedProceeds").as_str().map(String::from).unwrap_or("0.000000".into()),
            "retirement": state.as_ref().map(|s| get(s, "retirement").clone()).unwrap_or(V::Null),
            "entryAllowed": false,
            "writeCapability": false,
            "blocksLegacyWrites": true,
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
        let mut s = self.status_view();
        s.as_object_mut().unwrap().insert("observedAt".into(), json!(iso((self.now)())));
        let r = (|| -> Result<(), String> {
            atomic_json(&self.runtime_dir.join("owned-no-profit-stats.json"), &s)?;
            atomic_json(&self.runtime_dir.join("paired-live-stats.json"), &s)?;
            atomic_json(&self.runtime_dir.join("neutral-sell-recovery-stats.json"), &s)?;
            Ok(())
        })();
        if let Err(e) = r { *self.failed.borrow_mut() = true; return Err(e); }
        Ok(self.status_view())
    }
    /// Persist server-directed retry delays through the SAME shared limiter.
    fn cooldown(&self, e: &baal_execution::transport::OrderTransportError) -> Result<(), String> {
        if e.retry_after_ms == 0.0 { return Ok(()); }
        need(true, "BACKOFF_PERSISTENCE_REQUIRED").ok();
        let api = &self.api;
        let n = (self.now)() as f64;
        if !(e.retry_after_ms.fract() == 0.0) || e.retry_after_ms <= 0.0 || !(n + e.retry_after_ms).is_finite() || n + e.retry_after_ms > 8.640e15 {
            api.set_manual_review(true);
            api.on_admission(None);
        } else {
            api.set_cooldown(api.cooldown().max(n + e.retry_after_ms));
            api.set_next(api.next().max(api.cooldown()));
            api.on_admission(Some(api.next()));
        }
        Ok(())
    }
    fn cleanup(&self) -> Result<(), String> {
        let mut error: Option<String> = None;
        if let Some(b) = self.bridge.borrow_mut().take() { if let Err(e) = b.close() { error = error.or(Some(e)); } }
        if let Some(x) = self.exit.borrow_mut().take() { if let Err(e) = x.close() { error = error.or(Some(e)); } }
        if let Some(r) = self.recovery.borrow_mut().take() { if let Err(e) = r.close() { error = error.or(Some(e)); } }
        if let Some(l) = self.lease.borrow_mut().as_mut() { if let Err(e) = l.release() { error = error.or(Some(e.to_string())); } }
        match error { Some(e) => Err(e), None => Ok(()) }
    }
    fn init(self: &Rc<Self>) -> Result<(), String> {
        self.fixed()?;
        let this = self.clone();
        let recovery = crate::paired_neutral_recovery_host_v1::PairedNeutralRecoveryHost::open(
            &self.runtime_dir,
            &self.workspace,
            &self.universe,
            self.api.clone(),
            self.receipt_of.clone(),
            self.get_account_lease.clone(),
            self.get_other_state.clone(),
            Rc::new(move || this.allowed()),
            self.has_read_headroom.clone(),
            self.now.clone(),
        )?;
        need(recovery.is_some(), "PARENT_REQUIRED")?;
        *self.recovery.borrow_mut() = recovery;
        let anchor = source(&self.actor_file)?;
        let funding = source(&self.funding_file)?;
        let orders = get(get(anchor.journal.last.as_ref().unwrap_or(&V::Null), "state"), "orders").as_array().cloned().unwrap_or_default();
        need(
            orders.len() == 1
                && get(&orders[0], "phase") == &json!("reconciled")
                && !js_truthy(orders[0].get("semanticMismatch"))
                && !js_truthy(orders[0].get("cancelPending"))
                && get(get(&orders[0], "proof"), "verificationHash") == get(&self.profit, "buyProofHash"),
            "VERIFIED_BUY",
        )?;
        let buy = copy(get(&orders[0], "proof"));
        need(
            get(&buy, "scopedPrincipalCost") == &json!("0.070000")
                && get(&buy, "marketId") == get(&self.profit, "marketId")
                && get(&buy, "exchangeId") == get(&self.profit, "exchangeId")
                && get(&buy, "runId") == get(&self.request, "runId"),
            "BUY_SCOPE",
        )?;
        let row = anchor.rows.iter().rev().find(|r| {
            get(get(r, "event"), "input").get("kind") == Some(&json!("reconciled"))
                && get(get(r, "state"), "orders").as_array().and_then(|os| os.first())
                    .map(|o| get(get(o, "proof"), "verificationHash") == get(&self.profit, "buyProofHash")).unwrap_or(false)
        }).cloned();
        need(row.as_ref().map(|r| !get(get(get(r, "event"), "input"), "evidence").get("after").map(|a| !a.is_null()).unwrap_or(false)).unwrap_or(false), "BUY_SOURCE_EVENT")?;
        *self.original_after.borrow_mut() = copy(get(get(get(&row.unwrap(), "event"), "input"), "evidence").get("after").unwrap());
        *self.buy.borrow_mut() = buy;
        *self.anchor.borrow_mut() = Some(anchor);
        *self.funding.borrow_mut() = Some(funding);
        self.core()?;
        let lease = acquire_owner_lease(&self.dir, "Codex", get(&self.request, "runId").as_str().unwrap(), "owned-no-profit-host-v1", (self.now)() as f64)
            .map_err(|e| format!("NEUTRAL_SELL_RECOVERY_{}", e.code))?;
        *self.lease.borrow_mut() = Some(lease);
        let old = source(&self.old_file)?;
        need(
            old.bytes.len() as i64 == get(&self.request, "oldProfitJournalBytes").as_i64().unwrap_or(-1)
                && old.hash == get(&self.request, "oldProfitJournalHash").as_str().unwrap_or(""),
            "OLD_PROFIT_BINDING",
        )?;
        // The existing sell controller is re-opened in reconciliation-only v2.
        let this2 = self.clone();
        let exit = crate::owned_no_exit_controller_v2::OwnedNoExitControllerV2::open(
            &self.dir.join("controller"),
            &self.buy.borrow().clone(),
            &self.original_after.borrow().clone(),
            "midterm-elections",
            Rc::new(move || this2.allowed()),
            Rc::new(|| Err("NEUTRAL_SELL_RECOVERY_ACCOUNT_LEASE_LATE".into())) as GetV,
            Rc::new(|r: &V| Err(format!("NEUTRAL_SELL_RECOVERY_CLAIM_LATE_{}", get(r, "reservationHash").as_str().unwrap_or("")))),
            get(&self.request, "neutralSellRevision"),
            self.now.clone(),
            &crate::paired_rest_controller_v3::execution_dir(),
        );
        match exit {
            Ok(x) => *self.exit.borrow_mut() = Some(Rc::new(x)),
            Err(e) => return Err(e),
        }
        // Replay the retired parent journal's inventory_claim history.
        for r in &old.rows {
            let expected: V;
            match get(get(r, "event"), "kind").as_str().unwrap_or("") {
                "initialized" => {
                    need(self.old_state.borrow().is_none(), "OLD_DUPLICATE_GENESIS")?;
                    expected = json!({
                        "schemaVersion": 1,
                        "requestHash": get(&self.request, "profitRequestHash"),
                        "buyProofHash": get(&self.buy.borrow(), "verificationHash"),
                        "parentActorHash": self.anchor.borrow().as_ref().map(|a| a.hash.clone()),
                        "reservation": V::Null,
                        "retirement": V::Null,
                        "retiredOriginalExitProofHash": V::Null,
                    });
                }
                "inventory_claim" => {
                    need(
                        self.old_state.borrow().is_some()
                            && get(self.old_state.borrow().as_ref().unwrap(), "retirement").is_null()
                            && payload_hash(get(get(r, "event"), "record")).ok() == payload_hash(&self.expected_claim(1, false)?).ok(),
                        "OLD_CLAIM",
                    )?;
                    let mut e2 = self.old_state.borrow().clone().unwrap();
                    e2.as_object_mut().unwrap().insert("reservation".into(), copy(get(get(r, "event"), "record")));
                    expected = e2;
                    let record = get(get(r, "event"), "record").clone();
                    *self.installed.borrow_mut() = Some(json!({
                        "record": copy(&record),
                        "receipt": {
                            "reservationHash": get(&record, "reservationHash"),
                            "globalLedgerHash": get(r, "hash"),
                            "confirmedAt": get(get(r, "event"), "at"),
                        },
                    }));
                }
                _ => return Err("NEUTRAL_SELL_RECOVERY_OLD_CLAIM".into()),
            }
            need(payload_hash(&expected).ok() == payload_hash(get(r, "state")).ok(), "OLD_EXACT_REDUCER")?;
            *self.old_state.borrow_mut() = Some(copy(&expected));
        }
        need(
            self.old_state.borrow().as_ref().map(|s| !get(s, "reservation").is_null()).unwrap_or(false),
            "OLD_CLAIM_REQUIRED",
        )?;
        *self.old.borrow_mut() = Some(old);
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
            let r: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            self.validate(get(&r, "state"), get(&r, "event"))?;
            *self.state.borrow_mut() = Some(copy(get(&r, "state")));
            if get(get(&r, "event"), "kind") == &json!("inventory_claim") {
                let record = get(get(&r, "event"), "record").clone();
                *self.installed.borrow_mut() = Some(json!({
                    "record": copy(&record),
                    "receipt": {
                        "reservationHash": get(&record, "reservationHash"),
                        "globalLedgerHash": get(&r, "hash"),
                        "confirmedAt": get(get(&r, "event"), "at"),
                    },
                }));
            }
        }
        *self.local_size.borrow_mut() = bytes.len();
        *self.local_hash.borrow_mut() = sha(&bytes);
        if self.state.borrow().is_none() {
            let g = self.genesis();
            self.append(&g, &json!({"kind": "initialized", "at": iso((self.now)())}))?;
        }
        // Read-only dispatch through the existing serialized client: cooldown
        // bookkeeping persists through the shared limiter.
        let api = self.api.clone();
        let this3 = self.clone();
        let read_api: Rc<dyn for<'g> Fn(&V, Option<&'g baal_execution::transport::Guard<'g>>, Option<&'g baal_execution::transport::Guard<'g>>) -> Result<V, String>> =
            Rc::new(move |spec: &V, _d, _n| {
                match api.dispatch(spec) {
                    Ok(r) => Ok(r),
                    Err(e) => {
                        let _ = this3.cooldown(&e);
                        Err(e.code)
                    }
                }
            });
        let this4 = self.clone();
        let this5 = self.clone();
        let this6 = self.clone();
        let this7 = self.clone();
        let bridge = Rc::new(crate::owned_no_exit_evidence_bridge_v1::OwnedNoExitEvidenceBridge::new(
            read_api,
            self.receipt_of.clone(),
            &json!({
                "ownerId": "Codex",
                "runId": get(&self.request, "runId"),
                "tournamentId": T,
                "tournamentSlug": "midterm-elections",
                "marketId": get(&self.buy.borrow(), "marketId"),
                "exchangeId": get(&self.buy.borrow(), "exchangeId"),
            }),
            Rc::new(move || this4.global_risk()),
            Rc::new(move || Ok(copy(&this5.buy.borrow()))),
            Rc::new(move || Ok(copy(this6.installed.borrow().as_ref().unwrap_or(&V::Null)))),
            Rc::new(move |id: &V, _leg: &V| {
                this7.exit.borrow().as_ref().map(|e| get(get(&e.status(), "intent"), "orderId") == id).unwrap_or(false)
            }),
            {
                let t = self.clone();
                Rc::new(move || t.allowed())
            },
            {
                let h = self.has_read_headroom.clone();
                Rc::new(move |n: i64| -> Result<bool, String> { Ok((h)(n as usize)) }) as std::rc::Rc<dyn Fn(i64) -> Result<bool, String>>
            },
            self.now.clone(),
            None,
        )?);
        *self.bridge.borrow_mut() = Some(bridge);
        self.publish("neutral_sell_reconciliation_loaded")?;
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
            if self.state.borrow().as_ref().map(|s| !get(s, "retirement").is_null()).unwrap_or(false) {
                return self.publish("verified_sale_retired");
            }
            if self.api.manual_review() || (self.now)() < self.api.cooldown() as i64 || !(self.has_read_headroom)(6) {
                return self.publish("waiting_existing_queue_headroom");
            }
            let x = self.exit.borrow().as_ref().map(|e| e.status()).unwrap_or(V::Null);
            let o = get(&x, "intent").clone();
            need(
                self.global_risk()?.get("unresolvedIntentIds").and_then(|a| a.as_array()).map(|a| {
                    a.iter().all(|id| id == get(&o, "intentId"))
                }).unwrap_or(false),
                "GLOBAL_UNKNOWN",
            )?;
            let buy = self.buy.borrow().clone();
            let evidence = self.bridge.borrow().as_ref().unwrap().refresh_reconciliation(
                get(&o, "orderId"),
                &json!({"marketId": get(&buy, "marketId"), "exchangeId": get(&buy, "exchangeId")}),
            )?;
            self.fixed()?;
            if self.exit.borrow().as_ref().map(|e| e.verified_exit_proof().is_null()).unwrap_or(true) {
                self.exit.borrow().as_ref().unwrap().reconcile(&evidence)?;
            } else if get(self.installed.borrow().as_ref().map(|i| get(i, "record")).unwrap_or(&V::Null), "reservationHash")
                != get(&self.exit.borrow().as_ref().unwrap().exit_reservation()?, "reservationHash")
            {
                self.exit.borrow().as_ref().unwrap().refresh_reservation()?;
            }
            self.retire(&evidence)?;
            *self.last_failure.borrow_mut() = V::Null;
            self.publish("official_owned_sale_reconciled_and_retired")
        })();
        match result {
            Ok(v) => { *self.busy.borrow_mut() = false; Ok(v) }
            Err(e) => {
                let code = if e.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') {
                    e.clone()
                } else { "NEUTRAL_SELL_RECOVERY_EVIDENCE_UNAVAILABLE".into() };
                *self.last_failure.borrow_mut() = json!(code);
                *self.busy.borrow_mut() = false;
                let durable = self.exit.borrow().as_ref().map(|x| get(&x.status(), "failed") == &json!(true)).unwrap_or(false);
                if *self.failed.borrow() || durable {
                    *self.failed.borrow_mut() = true;
                    return Err(format!("{e} (fatal/durableRecordingFailed)"));
                }
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
