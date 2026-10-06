// execution/neutral-sell-recovery-host-v1.mjs — cleanup authority only: the
// old BUY parent and existing SELL are retained. No order submission,
// cancellation, key retry, hedge, or window renewal exists.
use crate::core::{payload_hash, validate_execution_state};
use crate::no_rest_sell_reconciliation_v2::verify_scoped_rest_no_sell_v2;
use crate::owned_no_exit_controller_v2::{
    open_owned_no_exit_controller_v2, OwnedNoExitControllerV2,
    OwnedNoExitControllerV2Deps,
};
use crate::owned_no_exit_evidence_bridge_v1::{
    create_owned_no_exit_evidence_bridge, OwnedNoExitEvidenceBridge,
};
use crate::owned_no_profit_host_v1::owned_no_profit_host_identity;
use crate::paired_live_host::PairedLiveApi;
use crate::paired_neutral_recovery_host_v1::{
    open_paired_neutral_recovery_host, PairedNeutralRecoveryDeps,
    PairedNeutralRecoveryHost,
};
use crate::paired_owned_exit_retirement_v2::build_paired_owned_exit_retirement_v2;
use crate::transport::Guard;
use baal_service_runtime::fixed::instant;
use baal_service_runtime::json_text::js_int;
use baal_service_runtime::journal::Journal;
use baal_service_runtime::ledger::{acquire_owner_lease, OwnerLease};
use baal_service_runtime::atomic::atomic_json;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::fs;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const T: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    static NULL: V = V::Null;
    v.get(k).unwrap_or(&NULL)
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("NEUTRAL_SELL_RECOVERY_{code}"))
    }
}
fn read(p: &Path) -> Result<V, String> {
    serde_json::from_str(&fs::read_to_string(p).map_err(|e| e.to_string())?)
        .map_err(|e| format!("NEUTRAL_SELL_RECOVERY_REQUEST:{e}"))
}
fn sha(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn iso(n: f64) -> String {
    baal_service_runtime::contract::ms_to_iso(n)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn run_id_ok(s: &str) -> bool {
    s.strip_prefix("sig-pilot-paired-").is_some_and(|r| {
        (1..=60).contains(&r.len())
            && r.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}
fn truthy(v: &V) -> bool {
    baal_service_runtime::json_text::js_truthy(Some(v))
}

// neutralSellRecoveryHostIdentity(workspace)
pub fn neutral_sell_recovery_host_identity(workspace: &Path) -> Result<V, String> {
    crate::frozen::collect_frozen_paper_files(
        workspace,
        &[
            "execution/neutral-sell-recovery-host-v1.mjs",
            "execution/scoped-rest-receipts.mjs",
        ],
    )
}

struct Source {
    bytes: Vec<u8>,
    hash: String,
    journal: Journal,
    rows: Vec<V>,
}

fn source(file: &Path) -> Result<Source, String> {
    let bytes = fs::read(file).map_err(|e| format!("NEUTRAL_SELL_RECOVERY_SOURCE_BOUND:{e}"))?;
    need(
        !bytes.is_empty() && bytes.len() <= 64 * 1024 * 1024 && bytes.last() == Some(&b'\n'),
        "SOURCE_BOUND",
    )?;
    let journal = Journal::open(file)?;
    let rows = String::from_utf8_lossy(&bytes)
        .trim_end()
        .split('\n')
        .map(|l| serde_json::from_str(l).unwrap_or(V::Null))
        .collect();
    Ok(Source {
        hash: sha(&bytes),
        bytes,
        journal,
        rows,
    })
}

// economic(p) — here with the v2 'ackEconomics' field added.
fn economic(p: &V) -> V {
    let mut o = serde_json::Map::new();
    for k in [
        "orderId",
        "intentId",
        "originalBuyProofHash",
        "originalLotId",
        "filledQuantity",
        "scopedProceeds",
        "originalPrincipalCost",
        "removedPrincipalCost",
        "realizedPnl",
        "remainingOwnedLot",
        "fills",
        "ackEconomics",
    ] {
        o.insert(k.to_string(), get(p, k).clone());
    }
    o.insert(
        "originalBefore".into(),
        get(get(p, "sourceReceipts"), "before").clone(),
    );
    o.insert(
        "originalAck".into(),
        get(get(p, "sourceReceipts"), "ack").clone(),
    );
    V::Object(o)
}

#[allow(clippy::type_complexity)]
pub struct NeutralSellRecoveryHostDeps {
    pub universe: V,
    pub api: Rc<RefCell<PairedLiveApi>>,
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub get_account_lease: Rc<dyn Fn() -> V>,
    pub get_other_state: Rc<dyn Fn() -> V>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> bool>,
    pub now: Rc<dyn Fn() -> f64>,
    pub workspace: PathBuf,
}

struct Inner {
    deps: NeutralSellRecoveryHostDeps,
    runtime_dir: PathBuf,
    request: V,
    request_hash: String,
    code: V,
    profit: V,
    recovery_descriptor: V,
    request_file: PathBuf,
    profit_file: PathBuf,
    authority_file: PathBuf,
    recovery_file: PathBuf,
    dir: PathBuf,
    file: PathBuf,
    actor_file: PathBuf,
    funding_file: PathBuf,
    old_file: PathBuf,
    exit_file: PathBuf,
    lease_hash: String,
    lease: Option<OwnerLease>,
    journal: Option<Journal>,
    recovery: Option<PairedNeutralRecoveryHost>,
    exit: Option<OwnedNoExitControllerV2>,
    bridge: Option<Rc<OwnedNoExitEvidenceBridge>>,
    state: V,
    old_state: V,
    installed: V,
    anchor: Option<Source>,
    funding: Option<Source>,
    old: Option<Source>,
    buy: V,
    original_after: V,
    closed: bool,
    failed: bool,
    busy: bool,
    last_failure: Option<String>,
    last_action: String,
    local_size: u64,
    local_hash: String,
}

pub struct NeutralSellRecoveryHost {
    inner: Rc<RefCell<Inner>>,
}

fn account_lease(inner: &Rc<RefCell<Inner>>) -> Result<V, String> {
    let l = (inner.borrow().deps.get_account_lease)();
    need(
        get(&l, "ownerId").as_str() == Some("Codex")
            && get(&l, "tournamentId").as_str() == Some(T)
            && hex(get(&l, "leaseId")),
        "ACCOUNT_LEASE",
    )?;
    Ok(l)
}
fn lease_key(l: &V) -> String {
    payload_hash(&json!({
        "ownerId": get(l, "ownerId"),
        "tournamentId": get(l, "tournamentId"),
        "leaseId": get(l, "leaseId"),
    }))
    .unwrap_or_default()
}

fn fixed(inner: &Rc<RefCell<Inner>>) -> Result<(), String> {
    let (
        closed,
        failed,
        request_file,
        profit_file,
        authority_file,
        recovery_file,
        request_hash,
        profit_hash,
        authority_hash,
        recovery_hash,
        code,
        workspace,
        lease_hash,
        actor_file,
        funding_file,
        old_file,
        anchor_hash,
        funding_hash,
        old_bytes,
    ) = {
        let i = inner.borrow();
        (
            i.closed,
            i.failed,
            i.request_file.clone(),
            i.profit_file.clone(),
            i.authority_file.clone(),
            i.recovery_file.clone(),
            i.request_hash.clone(),
            get(&i.request, "profitRequestHash").as_str().unwrap_or("").to_string(),
            get(&i.request, "authorityHash").as_str().unwrap_or("").to_string(),
            get(&i.profit, "recoveryDescriptorHash").as_str().unwrap_or("").to_string(),
            i.code.clone(),
            i.deps.workspace.clone(),
            i.lease_hash.clone(),
            i.actor_file.clone(),
            i.funding_file.clone(),
            i.old_file.clone(),
            i.anchor.as_ref().map(|a| a.hash.clone()),
            i.funding.as_ref().map(|f| f.hash.clone()),
            i.old.as_ref().map(|o| o.bytes.clone()),
        )
    };
    need(
        !closed && !failed && (inner.borrow().deps.can_operate)(),
        "STOPPED_OR_FAILED",
    )?;
    {
        let mut i = inner.borrow_mut();
        if let Some(l) = i.lease.as_mut() {
            l.assert_ownership()
                .map_err(|e| format!("NEUTRAL_SELL_RECOVERY_{}", e.message))?;
        }
    }
    need(
        payload_hash(&read(&request_file)?)? == request_hash
            && payload_hash(&read(&profit_file)?)? == profit_hash
            && payload_hash(&read(&authority_file)?)? == authority_hash
            && payload_hash(&read(&recovery_file)?)? == recovery_hash
            && payload_hash(&neutral_sell_recovery_host_identity(&workspace)?)?
                == payload_hash(&code)?,
        "SOURCE_CHANGED",
    )?;
    need(
        lease_key(&account_lease(inner)?) == lease_hash,
        "LEASE_CHANGED",
    )?;
    if let Some(a) = anchor_hash {
        let ab = fs::read(&actor_file)
            .map_err(|e| format!("NEUTRAL_SELL_RECOVERY_PARENT_CHANGED:{e}"))?;
        let fb = fs::read(&funding_file)
            .map_err(|e| format!("NEUTRAL_SELL_RECOVERY_PARENT_CHANGED:{e}"))?;
        need(
            sha(&ab) == a && sha(&fb) == funding_hash.unwrap_or_default(),
            "PARENT_CHANGED",
        )?;
    }
    if let Some(ob) = old_bytes {
        let now_bytes =
            fs::read(&old_file).map_err(|e| format!("NEUTRAL_SELL_RECOVERY_OLD_PROFIT_CHANGED:{e}"))?;
        need(now_bytes == ob, "OLD_PROFIT_CHANGED")?;
    }
    Ok(())
}

fn allowed(inner: &Rc<RefCell<Inner>>) -> bool {
    fixed(inner).is_ok()
}

fn core(inner: &Rc<RefCell<Inner>>) -> Result<V, String> {
    let s = validate_execution_state(&(inner.borrow().deps.get_other_state)())
        .map_err(|e| format!("NEUTRAL_SELL_RECOVERY_{e}"))?;
    let i = inner.borrow();
    need(
        get(&s, "tournamentId").as_str() == Some(T)
            && !get(&s, "owned").as_object().is_some_and(|m| {
                m.values().any(|p| {
                    get(p, "marketId") == get(&i.profit, "marketId")
                        || get(p, "exchangeId") == get(&i.profit, "exchangeId")
                })
            }),
        "FOREIGN_CORE_HOLDING",
    )?;
    Ok(s)
}

fn global_risk(inner: &Rc<RefCell<Inner>>) -> Result<V, String> {
    let s = core(inner)?;
    let (x, run_id, stopped, at, actor_hash, lease) = {
        let i = inner.borrow();
        (
            i.exit.as_ref().map(|e| e.status()),
            get(&i.request, "runId").clone(),
            !allowed(inner),
            iso((i.deps.now)()),
            i.anchor.as_ref().map(|a| a.hash.clone()).unwrap_or_default(),
            account_lease(inner)?,
        )
    };
    let mut unresolved: Vec<V> = get(&s, "intents")
        .as_object()
        .map(|m| {
            m.values()
                .filter(|x| get(x, "status").as_str() != Some("closed"))
                .map(|x| get(x, "intentId").clone())
                .collect()
        })
        .unwrap_or_default();
    if let Some(x) = &x {
        let intent = get(x, "intent");
        if !intent.is_null() && get(intent, "proofHash").is_null() {
            unresolved.push(get(intent, "intentId").clone());
        }
    }
    Ok(json!({
        "ownerId": "Codex",
        "runId": run_id,
        "tournamentId": T,
        "complete": true,
        "soleCollector": true,
        "stopped": stopped,
        "at": at,
        "unresolvedIntentIds": unresolved,
        "sourceHash": payload_hash(&json!({
            "core": s,
            "actorHash": actor_hash,
            "exit": x,
            "lease": lease,
        }))?,
    }))
}

fn expected_claim(inner: &Rc<RefCell<Inner>>, quantity: i64, terminal: bool) -> Result<V, String> {
    let i = inner.borrow();
    let mut b = serde_json::Map::new();
    b.insert("schemaVersion".into(), json!(1));
    b.insert("kind".into(), json!("single_owned_no_exit_reservation"));
    b.insert("ownerId".into(), get(&i.buy, "ownerId").clone());
    b.insert("runId".into(), get(&i.buy, "runId").clone());
    b.insert("tournamentId".into(), json!(T));
    b.insert("marketId".into(), get(&i.buy, "marketId").clone());
    b.insert("exchangeId".into(), get(&i.buy, "exchangeId").clone());
    b.insert("lotId".into(), get(&get(&i.buy, "lot"), "lotId").clone());
    b.insert(
        "buyProofHash".into(),
        get(&i.buy, "verificationHash").clone(),
    );
    b.insert(
        "originalPrincipalCost".into(),
        get(&i.buy, "scopedPrincipalCost").clone(),
    );
    b.insert("cashReserved".into(), json!("0.000000"));
    let sell_intent = i
        .exit
        .as_ref()
        .map(|e| get(&get(&e.status(), "intent"), "intentId").clone())
        .unwrap_or(V::Null);
    b.insert("sellIntentId".into(), sell_intent);
    b.insert("remainingQuantity".into(), json!(quantity));
    b.insert("terminalSell".into(), json!(terminal));
    let bh = payload_hash(&V::Object(b.clone()))?;
    b.insert("reservationHash".into(), json!(bh));
    Ok(V::Object(b))
}

fn genesis(inner: &Rc<RefCell<Inner>>) -> V {
    let i = inner.borrow();
    json!({
        "schemaVersion": 1,
        "requestHash": i.request_hash,
        "buyProofHash": get(&i.buy, "verificationHash"),
        "parentActorHash": i.anchor.as_ref().map(|a| a.hash.clone()).unwrap_or_default(),
        "oldProfitJournalHash": i.old.as_ref().map(|o| o.hash.clone()).unwrap_or_default(),
        "reservation": get(&i.old_state, "reservation"),
        "retirement": V::Null,
        "retiredOriginalExitProofHash": V::Null,
    })
}

fn validate(inner: &Rc<RefCell<Inner>>, next: &V, event: &V) -> Result<(), String> {
    let expected: V;
    let state = inner.borrow().state.clone();
    match get(event, "kind").as_str().unwrap_or("") {
        "initialized" => {
            need(state.is_null(), "DUPLICATE_GENESIS")?;
            expected = genesis(inner);
        }
        "inventory_claim" => {
            let filled = inner
                .borrow()
                .exit
                .as_ref()
                .map(|e| js_int(get(&e.verified_exit_proof(), "filledQuantity")))
                .unwrap_or(None)
                == Some(1);
            need(
                !state.is_null()
                    && get(&state, "retirement").is_null()
                    && filled
                    && payload_hash(get(event, "record"))?
                        == payload_hash(&expected_claim(inner, 0, true)?)?,
                "CLAIM_PROOF",
            )?;
            let mut e2 = state.clone();
            e2.as_object_mut().unwrap().insert(
                "reservation".into(),
                get(event, "record").clone(),
            );
            expected = e2;
        }
        _ => {
            need(
                get(event, "kind").as_str() == Some("retired")
                    && !state.is_null()
                    && get(&state, "retirement").is_null()
                    && js_int(get(&get(&state, "reservation"), "remainingQuantity")) == Some(0),
                "RETIREMENT_PHASE",
            )?;
            let original = inner
                .borrow()
                .exit
                .as_ref()
                .map(|e| e.verified_exit_proof())
                .unwrap_or(V::Null);
            let fresh = verify_scoped_rest_no_sell_v2(get(event, "sellReverification"))
                .map_err(|e| format!("NEUTRAL_SELL_RECOVERY_{e}"))?;
            let overlay = build_paired_owned_exit_retirement_v2(get(event, "evidence"))
                .map_err(|e| format!("NEUTRAL_SELL_RECOVERY_{e}"))?;
            let (buy, anchor_hash, fund_row) = {
                let i = inner.borrow();
                (
                    i.buy.clone(),
                    i.anchor.as_ref().map(|a| a.hash.clone()).unwrap_or_default(),
                    get(get(event, "evidence"), "fundingRegistryRecord").clone(),
                )
            };
            let funding_rows = inner
                .borrow()
                .funding
                .as_ref()
                .map(|f| f.rows.clone())
                .unwrap_or_default();
            need(
                get(&original, "verificationHash") == get(event, "originalExitProofHash")
                    && payload_hash(&economic(&original))? == payload_hash(&economic(&fresh))?
                    && payload_hash(get(&get(event, "sellReverification"), "originalBuyProof"))?
                        == payload_hash(&buy)?
                    && payload_hash(&fresh)?
                        == payload_hash(get(&get(event, "evidence"), "sellProof"))?
                    && get(&overlay, "originalBuyProofHash") == get(&buy, "verificationHash")
                    && get(&get(&overlay, "originalActorSource"), "sha256").as_str()
                        == Some(anchor_hash.as_str())
                    && funding_rows.iter().any(|r| {
                        get(r, "hash") == get(&fund_row, "hash")
                            && payload_hash(r).ok().as_deref() == payload_hash(&fund_row).ok().as_deref()
                    }),
                "RETIREMENT_SOURCE_MEMBERSHIP",
            )?;
            let mut e2 = state.clone();
            {
                let m = e2.as_object_mut().unwrap();
                m.insert("retirement".into(), overlay);
                m.insert(
                    "retiredOriginalExitProofHash".into(),
                    get(&original, "verificationHash").clone(),
                );
            }
            expected = e2;
        }
    }
    need(
        payload_hash(next)? == payload_hash(&expected)?,
        "EXACT_REDUCER",
    )
}

fn append(inner: &Rc<RefCell<Inner>>, next: &V, event: &V) -> Result<(), String> {
    fixed(inner)?;
    let file = inner.borrow().file.clone();
    let before = fs::read(&file).map_err(|e| e.to_string())?;
    need(
        before.len() as u64 == inner.borrow().local_size
            && sha(&before) == inner.borrow().local_hash,
        "LOCAL_JOURNAL_CHANGED",
    )?;
    validate(inner, next, event)?;
    let commit = {
        let mut i = inner.borrow_mut();
        i.journal.as_mut().unwrap().commit(next, event)
    };
    match commit {
        Ok(_) => {
            inner.borrow_mut().state = next.clone();
            let bytes = fs::read(&file).map_err(|e| e.to_string())?;
            need(
                bytes.len() >= before.len() && bytes[..before.len()] == before[..],
                "LOCAL_PREFIX_CHANGED",
            )?;
            {
                let mut i = inner.borrow_mut();
                i.local_size = bytes.len() as u64;
                i.local_hash = sha(&bytes);
            }
            fixed(inner)
        }
        Err(e) => {
            inner.borrow_mut().failed = true;
            Err(format!("{e}:fatal:durableRecordingFailed"))
        }
    }
}

fn claim(inner: &Rc<RefCell<Inner>>, record: &V) -> Result<V, String> {
    fixed(inner)?;
    need(
        payload_hash(record)?
            == payload_hash(
                &inner
                    .borrow()
                    .exit
                    .as_ref()
                    .map(|e| e.exit_reservation())
                    .unwrap_or(V::Null),
            )?,
        "CLAIM_CONTROLLER",
    )?;
    let at = iso((inner.borrow().deps.now)());
    let mut next = inner.borrow().state.clone();
    next.as_object_mut()
        .unwrap()
        .insert("reservation".into(), record.clone());
    append(
        inner,
        &next,
        &json!({"kind": "inventory_claim", "record": record, "at": at}),
    )?;
    let receipt = json!({
        "reservationHash": get(record, "reservationHash"),
        "globalLedgerHash": inner.borrow().journal.as_ref().map(|j| j.previous.clone()).unwrap_or_default(),
        "confirmedAt": at,
    });
    inner.borrow_mut().installed = json!({"record": record, "receipt": receipt});
    Ok(receipt)
}

fn parent_status(inner: &Rc<RefCell<Inner>>) -> V {
    let i = inner.borrow();
    let a = i.anchor.as_ref().unwrap();
    let s = get(&a.journal.last.clone().unwrap_or(V::Null), "state").clone();
    json!({
        "planHash": get(get(&s, "plan"), "planHash"),
        "sequence": a.journal.sequence,
        "hash": a.journal.previous,
        "orders": get(&s, "orders").as_array().map(|os| os.iter().map(|o| json!({
            "phase": get(o, "phase"),
            "orderId": get(o, "orderId"),
            "proofHash": get(get(o, "proof"), "verificationHash"),
        })).collect::<Vec<V>>()),
        "owned": get(&s, "orders").as_array().map(|os| os.iter().filter_map(|o| {
            let lot = get(get(o, "proof"), "lot");
            if lot.is_null() { return None; }
            let mut m = serde_json::Map::new();
            m.insert("marketId".into(), get(o, "marketId").clone());
            m.insert("exchangeId".into(), get(get(o, "body"), "exchangeId").clone());
            if let Some(lo) = lot.as_object() { for (k,v) in lo { m.insert(k.clone(), v.clone()); } }
            Some(V::Object(m))
        }).collect::<Vec<V>>()),
    })
}

fn retire(inner: &Rc<RefCell<Inner>>, evidence: &V) -> Result<(), String> {
    let original = {
        let i = inner.borrow();
        i.exit
            .as_ref()
            .map(|e| e.verified_exit_proof())
            .unwrap_or(V::Null)
    };
    need(
        js_int(get(&original, "filledQuantity")) == Some(1),
        "VERIFIED_SALE",
    )?;
    let exit_file = inner.borrow().exit_file.clone();
    let actual = source(&exit_file)?;
    let o = get(&actual.journal.last.clone().unwrap_or(V::Null), "state")
        .get("intent")
        .cloned()
        .unwrap_or(V::Null);
    let exit_status = inner.borrow().exit.as_ref().unwrap().status();
    need(
        actual.journal.sequence as i64 == js_int(get(&exit_status, "sequence")).unwrap_or(-1)
            && actual.journal.previous == get(&exit_status, "hash").as_str().unwrap_or("")
            && get(get(&o, "proof"), "verificationHash")
                == get(&original, "verificationHash"),
        "ACTUAL_EXIT_JOURNAL",
    )?;
    let (buy, original_after, read_at) = {
        let i = inner.borrow();
        (i.buy.clone(), i.original_after.clone(), iso((i.deps.now)()))
    };
    let sell_reverification = {
        let mut m = evidence.clone();
        let mm = m.as_object_mut().unwrap();
        mm.insert(
            "binding".into(),
            json!({
                "ownerId": get(&buy, "ownerId"),
                "runId": get(&buy, "runId"),
                "tournamentId": T,
                "tournamentSlug": "midterm-elections",
                "marketId": get(&buy, "marketId"),
                "exchangeId": get(&buy, "exchangeId"),
                "intentId": get(&o, "intentId"),
                "idempotencyKey": get(&get(&o, "body"), "idempotencyKey"),
                "orderId": get(&o, "orderId"),
                "side": "no",
                "action": "sell",
                "quantity": 1,
                "limit": get(&get(&o, "signal"), "protectedLimit"),
                "createdAt": get(&o, "createdAt"),
                "expirationDate": get(&get(&o, "body"), "expirationDate"),
            }),
        );
        mm.insert("originalBuyProof".into(), buy.clone());
        mm.insert("originalAfter".into(), original_after.clone());
        mm.insert("before".into(), get(&o, "before").clone());
        mm.insert("ack".into(), get(&o, "ack").clone());
        mm.insert("readAt".into(), json!(read_at));
        m
    };
    let fresh = verify_scoped_rest_no_sell_v2(&sell_reverification)
        .map_err(|e| format!("NEUTRAL_SELL_RECOVERY_{e}"))?;
    need(
        payload_hash(&economic(&original))? == payload_hash(&economic(&fresh))?,
        "SALE_ECONOMICS_CHANGED",
    )?;
    let l = account_lease(inner)?;
    let at = iso((inner.borrow().deps.now)());
    let risk = global_risk(inner)?;
    let final_at = iso((inner.borrow().deps.now)());
    let (anchor_bytes, anchor_seq, anchor_prev, fund_last, prefix_bytes, prefix_hash) = {
        let i = inner.borrow();
        (
            i.anchor.as_ref().map(|a| a.bytes.clone()).unwrap_or_default(),
            i.anchor.as_ref().map(|a| a.journal.sequence).unwrap_or(0),
            i.anchor.as_ref().map(|a| a.journal.previous.clone()).unwrap_or_default(),
            i.funding
                .as_ref()
                .and_then(|f| f.rows.last().cloned())
                .unwrap_or(V::Null),
            js_int(get(&get(&i.recovery_descriptor, "neutralAckRevision"), "originalPrefixBytes"))
                .unwrap_or(0),
            get(&get(&i.recovery_descriptor, "neutralAckRevision"), "originalPrefixHash").clone(),
        )
    };
    let lease_proof = {
        let mut lp = l.clone();
        let m = lp.as_object_mut().unwrap();
        m.insert("at".into(), json!(at));
        m.insert(
            "sourceHash".into(),
            json!(payload_hash(&json!({
                "lease": l,
                "at": at,
                "after": get(&get(&fresh, "sourceReceipts"), "after"),
            }))?),
        );
        lp
    };
    let inputs = json!({
        "originalBuyProof": buy,
        "sellProof": fresh,
        "sellAfter": get(evidence, "after"),
        "actorJournal": {
            "bodyText": String::from_utf8_lossy(&anchor_bytes).to_string(),
            "sequence": anchor_seq,
            "hash": anchor_prev,
            "prefixBytes": prefix_bytes,
            "prefixSha256": prefix_hash,
        },
        "actorStatus": parent_status(inner),
        "fundingRegistryRecord": fund_last,
        "accountLease": lease_proof,
        "globalRisk": risk,
        "readAt": final_at,
    });
    let overlay = build_paired_owned_exit_retirement_v2(&inputs)
        .map_err(|e| format!("NEUTRAL_SELL_RECOVERY_{e}"))?;
    let mut next = inner.borrow().state.clone();
    {
        let m = next.as_object_mut().unwrap();
        m.insert("retirement".into(), overlay);
        m.insert(
            "retiredOriginalExitProofHash".into(),
            get(&original, "verificationHash").clone(),
        );
    }
    append(
        inner,
        &next,
        &json!({
            "kind": "retired",
            "evidence": inputs,
            "sellReverification": sell_reverification,
            "originalExitProofHash": get(&original, "verificationHash"),
            "at": final_at,
        }),
    )
}

fn status(inner: &Rc<RefCell<Inner>>) -> V {
    let i = inner.borrow();
    let x = i.exit.as_ref().map(|e| e.status());
    let p = i.exit.as_ref().map(|e| e.verified_exit_proof());
    let intent = x.as_ref().map(|v| get(v, "intent").clone());
    let funding_state = i
        .funding
        .as_ref()
        .and_then(|f| f.journal.last.clone())
        .map(|l| get(&l, "state").clone())
        .unwrap_or(V::Null);
    let phase = if !get(&i.state, "retirement").is_null() {
        json!("retired")
    } else {
        x.as_ref()
            .map(|v| get(v, "phase").clone())
            .unwrap_or(json!("recovery_pending"))
    };
    let owned: V = if p.as_ref().is_some_and(|p| js_int(get(p, "filledQuantity")) == Some(1)) {
        json!([])
    } else if !i.buy.is_null() {
        let mut m = serde_json::Map::new();
        m.insert("marketId".into(), get(&i.buy, "marketId").clone());
        m.insert("exchangeId".into(), get(&i.buy, "exchangeId").clone());
        if let Some(lo) = get(&i.buy, "lot").as_object() {
            for (k, v) in lo {
                m.insert(k.clone(), v.clone());
            }
        }
        json!([V::Object(m)])
    } else {
        json!([])
    };
    json!({
        "schemaVersion": 1,
        "mode": "single_owned_no_neutral_sell_recovery_host_v1",
        "hostVersion": 5,
        "ownerId": "Codex",
        "runId": get(&i.request, "runId"),
        "tournamentId": T,
        "marketId": get(&i.profit, "marketId"),
        "exchangeId": get(&i.profit, "exchangeId"),
        "requestHash": i.request_hash,
        "profitRequestHash": get(&i.request, "profitRequestHash"),
        "authorityHash": get(&i.request, "authorityHash"),
        "buyProofHash": get(&i.buy, "verificationHash"),
        "originalBuyOrderId": get(&i.buy, "orderId"),
        "originalPrincipalCost": get(&i.buy, "scopedPrincipalCost"),
        "startedAt": get(&i.request, "startedAt"),
        "entryExpiresAt": get(&i.profit, "entryExpiresAt"),
        "recoveryDescriptorExpiresAt": get(&i.request, "expiresAt"),
        "cleanupAllowedAfterDescriptorExpiry": true,
        "originalReviewExpiresAt": i.anchor.as_ref()
            .map(|a| get(&get(&get(&a.journal.last.clone().unwrap_or(V::Null), "state"), "plan"), "reviewExpiresAt").clone())
            .unwrap_or(V::Null),
        "phase": phase,
        "orders": intent.as_ref().filter(|v| !v.is_null()).map(|v| json!([v])).unwrap_or(json!([])),
        "orderCount": if intent.as_ref().is_some_and(|v| !v.is_null()) { 1 } else { 0 },
        "owned": owned,
        "historicalParentFunding": get(&funding_state, "funding"),
        "currentBudget": if !get(&i.state, "retirement").is_null() {
            json!({
                "ownedPrincipal": "0.000000",
                "reservedCash": "0.000000",
                "cumulativeBuy": "0.070000",
            })
        } else {
            get(&funding_state, "funding").clone()
        },
        "realizedPnl": p.as_ref().map(|v| get(v, "realizedPnl").clone()).unwrap_or(json!("0.000000")),
        "scopedProceeds": p.as_ref().map(|v| get(v, "scopedProceeds").clone()).unwrap_or(json!("0.000000")),
        "retirement": get(&i.state, "retirement"),
        "entryAllowed": false,
        "writeCapability": false,
        "blocksLegacyWrites": true,
        "lastAction": i.last_action,
        "lastFailure": i.last_failure,
        "failed": i.failed || x.as_ref().map(|v| get(v, "failed") == &json!(true)).unwrap_or(false),
        "closed": i.closed,
        "busy": i.busy,
        "additionalAccountClients": 0,
        "additionalPersistentProcesses": 0,
        "profitabilityEstablished": false,
    })
}

fn publish(inner: &Rc<RefCell<Inner>>, action: &str) -> Result<V, String> {
    inner.borrow_mut().last_action = action.to_string();
    let (f1, f2, f3, now) = {
        let i = inner.borrow();
        (
            i.runtime_dir.join("owned-no-profit-stats.json"),
            i.runtime_dir.join("paired-live-stats.json"),
            i.runtime_dir.join("neutral-sell-recovery-stats.json"),
            (i.deps.now)(),
        )
    };
    let mut s = status(inner);
    s.as_object_mut()
        .unwrap()
        .insert("observedAt".into(), json!(iso(now)));
    for f in [&f1, &f2, &f3] {
        atomic_json(f, &s).map_err(|e| {
            inner.borrow_mut().failed = true;
            format!("{e}:fatal:durableRecordingFailed")
        })?;
    }
    Ok(status(inner))
}

// JS cooldown(e): persists api retry state when the wire error carries
// retryAfterMs. String errors cannot carry it, so there is nothing to persist.
fn cooldown(_inner: &Rc<RefCell<Inner>>, _e: &str) {}

fn cleanup(inner: &Rc<RefCell<Inner>>) -> Result<(), String> {
    let mut first: Option<String> = None;
    let mut i = inner.borrow_mut();
    if let Some(b) = i.bridge.take() {
        if let Err(e) = b.close() {
            if first.is_none() {
                first = Some(e);
            }
        }
    }
    if let Some(x) = i.exit.take() {
        if let Err(e) = x.close() {
            if first.is_none() {
                first = Some(e);
            }
        }
    }
    if let Some(r) = i.recovery.take() {
        if let Err(e) = r.close() {
            if first.is_none() {
                first = Some(e);
            }
        }
    }
    if let Some(mut l) = i.lease.take() {
        if let Err(e) = l.release() {
            if first.is_none() {
                first = Some(format!("NEUTRAL_SELL_RECOVERY_{}", e.message));
            }
        }
    }
    match first {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

// openNeutralSellRecoveryHost({runtimeDir,workspace,universe,api,receiptOf,getAccountLease,getOtherState,canOperate,hasReadHeadroom,now})
pub fn open_neutral_sell_recovery_host(
    runtime_dir: &Path,
    deps: NeutralSellRecoveryHostDeps,
) -> Result<Option<NeutralSellRecoveryHost>, String> {
    let request_file = runtime_dir.join("neutral-sell-recovery-request.json");
    if !request_file.exists() {
        return Ok(None);
    }
    need(
        runtime_dir.is_absolute() && deps.workspace.is_absolute(),
        "DEPENDENCIES",
    )?;
    need((deps.can_operate)(), "STOPPED")?;
    let request = read(&request_file)?;
    let request_hash = payload_hash(&request)?;
    let code = neutral_sell_recovery_host_identity(&deps.workspace)?;
    let mut keys = vec![
        "schemaVersion",
        "kind",
        "ownerId",
        "runId",
        "tournamentId",
        "profitRequestHash",
        "authorityHash",
        "oldProfitJournalHash",
        "oldProfitJournalBytes",
        "startedAt",
        "expiresAt",
        "codeIdentity",
        "neutralSellRevision",
    ];
    keys.sort();
    let actual_keys: Vec<String> = request
        .as_object()
        .map(|o| {
            let mut ks: Vec<String> = o.keys().cloned().collect();
            ks.sort();
            ks
        })
        .unwrap_or_default();
    need(
        actual_keys.join("|") == keys.join("|")
            && js_int(get(&request, "schemaVersion")) == Some(1)
            && get(&request, "kind").as_str() == Some("single_owned_no_neutral_sell_recovery_v1")
            && get(&request, "ownerId").as_str() == Some("Codex")
            && get(&request, "tournamentId").as_str() == Some(T)
            && get(&request, "runId").as_str().is_some_and(run_id_ok)
            && ["profitRequestHash", "authorityHash", "oldProfitJournalHash"]
                .iter()
                .all(|k| hex(get(&request, k)))
            && js_int(get(&request, "oldProfitJournalBytes")).is_some_and(|b| b > 0)
            && payload_hash(get(&request, "codeIdentity"))? == payload_hash(&code)?,
        "REQUEST",
    )?;
    need(
        instant_v(get(&request, "startedAt"))? as f64 <= (deps.now)()
            && instant_v(get(&request, "expiresAt"))? > instant_v(get(&request, "startedAt"))?,
        "DESCRIPTOR_CLOCK",
    )?;
    let profit_file = runtime_dir.join("owned-no-profit-request.json");
    let profit = read(&profit_file)?;
    let authority_file = runtime_dir.join("paired-live-authority.json");
    let authority = read(&authority_file)?;
    let recovery_file = runtime_dir.join("paired-neutral-ack-recovery.json");
    let recovery_descriptor = read(&recovery_file)?;
    need(
        payload_hash(&profit)? == get(&request, "profitRequestHash").as_str().unwrap_or("")
            && get(&profit, "runId") == get(&request, "runId")
            && get(&profit, "ownerId").as_str() == Some("Codex")
            && get(&profit, "tournamentId").as_str() == Some(T)
            && get(&profit, "authorityHash") == get(&request, "authorityHash")
            && payload_hash(get(&profit, "codeIdentity"))?
                == payload_hash(&owned_no_profit_host_identity(&deps.workspace)?)?
            && payload_hash(&authority)? == get(&request, "authorityHash").as_str().unwrap_or("")
            && get(&authority, "kind").as_str() == Some("direct_human_authorized_bot_update")
            && get(&authority, "ownerId").as_str() == Some("Codex")
            && get(&authority, "exploratory") == &json!(true)
            && get(&authority, "profitabilityEstablished") == &json!(false)
            && payload_hash(&recovery_descriptor)?
                == get(&profit, "recoveryDescriptorHash").as_str().unwrap_or(""),
        "ORIGINAL_AUTHORITY",
    )?;
    let run_id = get(&request, "runId").as_str().unwrap_or("").to_string();
    let parent_dir = runtime_dir
        .join("paired-live-v2-runs")
        .join(&run_id)
        .join("child-runtime")
        .join("paired-live-runs")
        .join(&run_id);
    let actor_file = parent_dir.join("controller").join("paired-rest-journal.jsonl");
    let funding_file = parent_dir.join("funding").join("paired-shared-funding.jsonl");
    let dir = parent_dir.join("owned-no-profit-v1");
    let old_file = dir.join("owned-no-profit-journal.jsonl");
    let exit_dir = dir.join("controller");
    let exit_file = exit_dir.join("owned-no-exit-journal.jsonl");
    let file = dir.join("neutral-sell-repair-journal.jsonl");
    let lease0 = {
        let l = (deps.get_account_lease)();
        need(
            get(&l, "ownerId").as_str() == Some("Codex")
                && get(&l, "tournamentId").as_str() == Some(T)
                && hex(get(&l, "leaseId")),
            "ACCOUNT_LEASE",
        )?;
        l
    };
    let lease_hash = lease_key(&lease0);
    let inner = Rc::new(RefCell::new(Inner {
        request,
        request_hash,
        code,
        profit,
        recovery_descriptor,
        request_file,
        profit_file,
        authority_file,
        recovery_file,
        dir,
        file,
        actor_file,
        funding_file,
        old_file,
        exit_file,
        lease_hash,
        deps,
        runtime_dir: runtime_dir.to_path_buf(),
        lease: None,
        journal: None,
        recovery: None,
        exit: None,
        bridge: None,
        state: V::Null,
        old_state: V::Null,
        installed: V::Null,
        anchor: None,
        funding: None,
        old: None,
        buy: V::Null,
        original_after: V::Null,
        closed: false,
        failed: false,
        busy: false,
        last_failure: None,
        last_action: "loaded".into(),
        local_size: 0,
        local_hash: String::new(),
    }));
    let result = (|| -> Result<(), String> {
        fixed(&inner)?;
        let (runtime_dir2, universe, api, receipt_of, get_account_lease, get_other_state, has_read_headroom, now, workspace) = {
            let i = inner.borrow();
            (
                i.runtime_dir.clone(),
                i.deps.universe.clone(),
                i.deps.api.clone(),
                i.deps.receipt_of.clone(),
                i.deps.get_account_lease.clone(),
                i.deps.get_other_state.clone(),
                i.deps.has_read_headroom.clone(),
                i.deps.now.clone(),
                i.deps.workspace.clone(),
            )
        };
        let inner_c = inner.clone();
        let recovery = open_paired_neutral_recovery_host(
            &runtime_dir2,
            &universe,
            api.clone(),
            PairedNeutralRecoveryDeps {
                receipt_of,
                get_account_lease,
                get_other_state,
                can_operate: Rc::new(move || allowed(&inner_c)),
                has_read_headroom,
                now,
                workspace,
            },
        )?;
        need(recovery.is_some(), "PARENT_REQUIRED")?;
        inner.borrow_mut().recovery = recovery;
        let anchor = source(&inner.borrow().actor_file.clone())?;
        let funding = source(&inner.borrow().funding_file.clone())?;
        let actor_state = get(
            &anchor.journal.last.clone().unwrap_or(V::Null),
            "state",
        )
        .clone();
        let orders = get(&actor_state, "orders").as_array().cloned().unwrap_or_default();
        let buy_hash = get(&inner.borrow().profit, "buyProofHash").clone();
        need(
            orders.len() == 1
                && get(&orders[0], "phase").as_str() == Some("reconciled")
                && !truthy(get(&orders[0], "semanticMismatch"))
                && !truthy(get(&orders[0], "cancelPending"))
                && get(&get(&orders[0], "proof"), "verificationHash") == &buy_hash,
            "VERIFIED_BUY",
        )?;
        let buy = get(&orders[0], "proof").clone();
        {
            let i = inner.borrow();
            need(
                get(&buy, "scopedPrincipalCost").as_str() == Some("0.070000")
                    && get(&buy, "marketId") == get(&i.profit, "marketId")
                    && get(&buy, "exchangeId") == get(&i.profit, "exchangeId")
                    && get(&buy, "runId") == get(&i.request, "runId"),
                "BUY_SCOPE",
            )?;
        }
        let row = anchor.rows.iter().rev().find(|r| {
            let kind_ok =
                get(&get(&get(r, "event"), "input"), "kind").as_str() == Some("reconciled");
            let order0 = get(&get(r, "state"), "orders")
                .as_array()
                .and_then(|a| a.first().cloned())
                .unwrap_or(V::Null);
            kind_ok && get(&get(&order0, "proof"), "verificationHash") == &buy_hash
        });
        need(
            row.map(|r| {
                !get(&get(&get(&r, "event"), "input"), "evidence")
                    .get("after")
                    .is_some_and(|v| !v.is_null())
            })
            .unwrap_or(false),
            "BUY_SOURCE_EVENT",
        )?;
        let original_after = get(
            &get(&get(row.unwrap(), "event"), "input")
                .get("evidence")
                .cloned()
                .unwrap_or(V::Null),
            "after",
        )
        .clone();
        {
            let mut i = inner.borrow_mut();
            i.anchor = Some(anchor);
            i.funding = Some(funding);
            i.buy = buy;
            i.original_after = original_after;
        }
        core(&inner)?;
        {
            let (d, rid, now2) = {
                let i = inner.borrow();
                (
                    i.dir.clone(),
                    get(&i.request, "runId").as_str().unwrap_or("").to_string(),
                    (i.deps.now)(),
                )
            };
            inner.borrow_mut().lease = Some(
                acquire_owner_lease(&d, "Codex", &rid, "owned-no-profit-host-v1", now2)
                    .map_err(|e| format!("NEUTRAL_SELL_RECOVERY_{}", e.message))?,
            );
        }
        let old = source(&inner.borrow().old_file.clone())?;
        {
            let i = inner.borrow();
            need(
                old.bytes.len() as i64
                    == js_int(get(&i.request, "oldProfitJournalBytes")).unwrap_or(-1)
                    && old.hash == get(&i.request, "oldProfitJournalHash").as_str().unwrap_or(""),
                "OLD_PROFIT_BINDING",
            )?;
        }
        inner.borrow_mut().old = Some(old);
        // openOwnedNoExitControllerV2
        {
            let (exit_dir, buy, original_after, revision, now3) = {
                let i = inner.borrow();
                (
                    i.dir.join("controller"),
                    i.buy.clone(),
                    i.original_after.clone(),
                    get(&i.request, "neutralSellRevision").clone(),
                    i.deps.now.clone(),
                )
            };
            let inner_c2 = inner.clone();
            let can_operate = Rc::new(move || allowed(&inner_c2));
            let inner_al = inner.clone();
            let assert_account_lease = Rc::new(move || account_lease(&inner_al));
            let inner_ir = inner.clone();
            let install_exit_reservation = Rc::new(move |r: &V| claim(&inner_ir, r));
            let now_i = Rc::new(move || now3() as i64);
            let exit = open_owned_no_exit_controller_v2(
                &exit_dir,
                &buy,
                &original_after,
                "midterm-elections",
                OwnedNoExitControllerV2Deps {
                    can_operate,
                    assert_account_lease,
                    install_exit_reservation,
                    neutral_sell_revision: revision,
                    now: now_i,
                },
            )?;
            inner.borrow_mut().exit = Some(exit);
        }
        // replay the old profit journal rows
        let old_rows = inner
            .borrow()
            .old
            .as_ref()
            .map(|o| o.rows.clone())
            .unwrap_or_default();
        for r in old_rows {
            let event = get(&r, "event");
            let kind = get(event, "kind").as_str().unwrap_or("").to_string();
            let mut expected;
            if kind == "initialized" {
                need(inner.borrow().old_state.is_null(), "OLD_DUPLICATE_GENESIS")?;
                let (req_hash, buy_hash2, actor_hash) = {
                    let i = inner.borrow();
                    (
                        get(&i.request, "profitRequestHash").clone(),
                        get(&i.buy, "verificationHash").clone(),
                        i.anchor.as_ref().map(|a| a.hash.clone()).unwrap_or_default(),
                    )
                };
                expected = json!({
                    "schemaVersion": 1,
                    "requestHash": req_hash,
                    "buyProofHash": buy_hash2,
                    "parentActorHash": actor_hash,
                    "reservation": V::Null,
                    "retirement": V::Null,
                    "retiredOriginalExitProofHash": V::Null,
                });
            } else {
                let old_state = inner.borrow().old_state.clone();
                need(
                    kind == "inventory_claim"
                        && !old_state.is_null()
                        && get(&old_state, "retirement").is_null()
                        && payload_hash(get(event, "record"))?
                            == payload_hash(&expected_claim(&inner, 1, false)?)?,
                    "OLD_CLAIM",
                )?;
                expected = old_state.clone();
                expected.as_object_mut().unwrap().insert(
                    "reservation".into(),
                    get(event, "record").clone(),
                );
                inner.borrow_mut().installed = json!({
                    "record": get(event, "record"),
                    "receipt": {
                        "reservationHash": get(&get(event, "record"), "reservationHash"),
                        "globalLedgerHash": get(&r, "hash"),
                        "confirmedAt": get(event, "at"),
                    },
                });
            }
            need(
                payload_hash(&expected)? == payload_hash(get(&r, "state"))?,
                "OLD_EXACT_REDUCER",
            )?;
            inner.borrow_mut().old_state = expected.clone();
        }
        need(
            !get(&inner.borrow().old_state, "reservation").is_null(),
            "OLD_CLAIM_REQUIRED",
        )?;
        // local repair journal
        let file = inner.borrow().file.clone();
        fs::create_dir_all(inner.borrow().dir.clone()).map_err(|e| e.to_string())?;
        if !file.exists() {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&file)
                .map_err(|e| format!("NEUTRAL_SELL_RECOVERY_JOURNAL:{e}"))?;
        }
        let bytes = fs::read(&file).map_err(|e| e.to_string())?;
        need(
            bytes.is_empty() || bytes.last() == Some(&b'\n'),
            "LOCAL_PARTIAL",
        )?;
        let journal = Journal::open(&file)?;
        for line in String::from_utf8_lossy(&bytes)
            .split('\n')
            .filter(|l| !l.is_empty())
        {
            let r: V =
                serde_json::from_str(line).map_err(|_| "NEUTRAL_SELL_RECOVERY_JOURNAL".to_string())?;
            validate(&inner, get(&r, "state"), get(&r, "event"))?;
            inner.borrow_mut().state = get(&r, "state").clone();
            if get(&get(&r, "event"), "kind").as_str() == Some("inventory_claim") {
                inner.borrow_mut().installed = json!({
                    "record": get(&get(&r, "event"), "record"),
                    "receipt": {
                        "reservationHash": get(&get(&get(&r, "event"), "record"), "reservationHash"),
                        "globalLedgerHash": get(&r, "hash"),
                        "confirmedAt": get(&get(&r, "event"), "at"),
                    },
                });
            }
        }
        inner.borrow_mut().local_size = bytes.len() as u64;
        inner.borrow_mut().local_hash = sha(&bytes);
        inner.borrow_mut().journal = Some(journal);
        if inner.borrow().state.is_null() {
            let g = genesis(&inner);
            append(
                &inner,
                &g,
                &json!({"kind": "initialized", "at": iso((inner.borrow().deps.now)())}),
            )?;
        }
        // evidence bridge on the readApi wrapper
        let (api2, receipt2, has_headroom2, now3) = {
            let i = inner.borrow();
            (
                i.deps.api.clone(),
                i.deps.receipt_of.clone(),
                i.deps.has_read_headroom.clone(),
                i.deps.now.clone(),
            )
        };
        let scope = {
            let i = inner.borrow();
            json!({
                "ownerId": "Codex",
                "runId": get(&i.request, "runId"),
                "tournamentId": T,
                "tournamentSlug": "midterm-elections",
                "marketId": get(&i.buy, "marketId"),
                "exchangeId": get(&i.buy, "exchangeId"),
            })
        };
        let api_dispatch = api2.borrow().dispatch.clone();
        let inner_rd = inner.clone();
        let dispatch = Rc::new(move |s: &V| -> Result<V, String> {
            match (api_dispatch)(s, None, None) {
                Ok(v) => Ok(v),
                Err(e) => {
                    cooldown(&inner_rd, &e.message);
                    if e.message.contains(":fatal:") || e.message.contains("durableRecordingFailed")
                    {
                        inner_rd.borrow_mut().failed = true;
                    }
                    Err(e.message)
                }
            }
        });
        let inner_gr = inner.clone();
        let get_global_risk = Rc::new(move || global_risk(&inner_gr));
        let inner_gl = inner.clone();
        let get_owned_lot =
            Rc::new(move || -> Result<V, String> { Ok(inner_gl.borrow().buy.clone()) });
        let inner_ge = inner.clone();
        let get_exit_reservation =
            Rc::new(move || -> Result<V, String> { Ok(inner_ge.borrow().installed.clone()) });
        let inner_oo = inner.clone();
        let owns_order = Rc::new(move |id: &V, _leg: &V| -> Result<bool, String> {
            Ok(inner_oo
                .borrow()
                .exit
                .as_ref()
                .map(|e| get(&get(&e.status(), "intent"), "orderId") == id)
                .unwrap_or(false))
        });
        let inner_cr = inner.clone();
        let can_read = Rc::new(move || -> bool { allowed(&inner_cr) });
        let has_headroom_b = {
            let h = has_headroom2;
            Rc::new(move |n: i64| -> Result<bool, String> { Ok(h(n)) })
        };
        let bridge = create_owned_no_exit_evidence_bridge(
            dispatch,
            &scope,
            receipt2,
            get_global_risk,
            get_owned_lot,
            get_exit_reservation,
            owns_order,
            can_read,
            has_headroom_b,
            Rc::new(move || (now3)() as i64),
            None,
        )?;
        inner.borrow_mut().bridge = Some(Rc::new(bridge));
        publish(&inner, "neutral_sell_reconciliation_loaded")?;
        Ok(())
    })();
    if let Err(e) = result {
        let durable = e.contains(":fatal:") || e.contains("durableRecordingFailed");
        let io_fatal = ["EIO", "ENOSPC", "EROFS", "EPERM", "EACCES", "EBUSY"]
            .iter()
            .any(|c| e == *c || e.ends_with(&format!(":{c}")) || e.starts_with(&format!("{c}:")));
        let e = if durable || io_fatal {
            inner.borrow_mut().failed = true;
            if durable {
                e
            } else {
                format!("{e}:fatal:durableRecordingFailed")
            }
        } else {
            e
        };
        let _ = cleanup(&inner);
        return Err(e);
    }
    Ok(Some(NeutralSellRecoveryHost { inner }))
}

impl NeutralSellRecoveryHost {
    // authorizes(spec): GET reads only, through the owned-exit bridge.
    pub fn authorizes(&self, spec: &V, before_dispatch: &Guard, before_network: &Guard) -> bool {
        get(spec, "method").as_str() == Some("GET")
            && allowed(&self.inner)
            && self
                .inner
                .borrow()
                .bridge
                .as_ref()
                .map(|b| b.authorizes(spec, before_dispatch, before_network))
                .unwrap_or(false)
    }
    pub fn on_fetch_start(&self, _meta: &V) -> bool {
        false
    }
    pub fn blocks_legacy_writes(&self) -> bool {
        true
    }
    pub fn allows_legacy(&self, spec: &V) -> bool {
        get(spec, "method").as_str() == Some("GET")
    }
    pub fn status(&self) -> V {
        status(&self.inner)
    }
    pub fn step(&self) -> Result<V, String> {
        {
            let mut i = self.inner.borrow_mut();
            need(!i.busy && !i.closed, "BUSY_OR_CLOSED")?;
            i.busy = true;
        }
        let r = self.step_inner();
        self.inner.borrow_mut().busy = false;
        r
    }
    fn step_inner(&self) -> Result<V, String> {
        let result = (|| -> Result<V, String> {
            if !allowed(&self.inner) {
                return publish(&self.inner, "operational_stop_or_identity_changed");
            }
            if !get(&self.inner.borrow().state, "retirement").is_null() {
                return publish(&self.inner, "verified_sale_retired");
            }
            {
                let i = self.inner.borrow();
                let api = i.deps.api.borrow();
                if api.manual_review
                    || (i.deps.now)() < api.cooldown
                    || !(i.deps.has_read_headroom)(6)
                {
                    drop(api);
                    drop(i);
                    return publish(&self.inner, "waiting_existing_queue_headroom");
                }
            }
            let o = {
                let i = self.inner.borrow();
                get(
                    &i.exit.as_ref().map(|e| e.status()).unwrap_or(V::Null),
                    "intent",
                )
                .clone()
            };
            let risk = global_risk(&self.inner)?;
            need(
                get(&risk, "unresolvedIntentIds")
                    .as_array()
                    .is_some_and(|us| us.iter().all(|u| u == get(&o, "intentId"))),
                "GLOBAL_UNKNOWN",
            )?;
            let leg = {
                let i = inner_borrowed(&self.inner);
                json!({
                    "marketId": get(&i.buy, "marketId"),
                    "exchangeId": get(&i.buy, "exchangeId"),
                })
            };
            let evidence = {
                let i = self.inner.borrow();
                i.bridge
                    .as_ref()
                    .ok_or("NEUTRAL_SELL_RECOVERY_EVIDENCE".to_string())?
                    .refresh_reconciliation(get(&o, "orderId"), &leg)?
            };
            fixed(&self.inner)?;
            let has_proof = self
                .inner
                .borrow()
                .exit
                .as_ref()
                .map(|e| !e.verified_exit_proof().is_null())
                .unwrap_or(false);
            if !has_proof {
                self.inner
                    .borrow()
                    .exit
                    .as_ref()
                    .ok_or("NEUTRAL_SELL_RECOVERY_EXIT".to_string())?
                    .reconcile(&evidence)?;
            } else {
                let (installed_hash, now_hash) = {
                    let i = self.inner.borrow();
                    (
                        get(&i.installed, "record").get("reservationHash").cloned(),
                        i.exit
                            .as_ref()
                            .map(|e| get(&e.exit_reservation(), "reservationHash").clone()),
                    )
                };
                if installed_hash != now_hash {
                    self.inner
                        .borrow()
                        .exit
                        .as_ref()
                        .ok_or("NEUTRAL_SELL_RECOVERY_EXIT".to_string())?
                        .refresh_reservation()?;
                }
            }
            retire(&self.inner, &evidence)?;
            self.inner.borrow_mut().last_failure = None;
            publish(&self.inner, "official_owned_sale_reconciled_and_retired")
        })();
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                cooldown(&self.inner, &e);
                let code = e.as_str();
                let ok = !code.is_empty()
                    && code.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                    && code
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
                self.inner.borrow_mut().last_failure = Some(if ok {
                    code.to_string()
                } else {
                    "NEUTRAL_SELL_RECOVERY_EVIDENCE_UNAVAILABLE".to_string()
                });
                let exit_failed = self
                    .inner
                    .borrow()
                    .exit
                    .as_ref()
                    .map(|x| get(&x.status(), "failed") == &json!(true))
                    .unwrap_or(false);
                if self.inner.borrow().failed
                    || e.contains(":fatal:")
                    || e.contains("durableRecordingFailed")
                    || exit_failed
                {
                    self.inner.borrow_mut().failed = true;
                    return Err(format!("{e}:fatal:durableRecordingFailed"));
                }
                publish(&self.inner, "reconciliation_blocked")
            }
        }
    }
    pub fn close(&self) -> Result<(), String> {
        {
            let mut i = self.inner.borrow_mut();
            need(!i.busy, "BUSY")?;
            if i.closed {
                return Ok(());
            }
            i.closed = true;
        }
        cleanup(&self.inner)
    }
}

fn inner_borrowed(inner: &Rc<RefCell<Inner>>) -> std::cell::Ref<'_, Inner> {
    inner.borrow()
}
