// execution/owned-no-profit-host-v1.mjs — a one-lot SELL wrapper over the
// already verified parent recovery actor. It adds no account client, token,
// timer, new pair or extension of original clocks.
use crate::core::{payload_hash, validate_execution_state};
use crate::no_rest_sell_reconciliation_v1::verify_scoped_rest_no_sell;
use crate::owned_no_exit_controller_v1::{
    open_owned_no_exit_controller, OwnedNoExitController, OwnedNoExitControllerDeps,
};
use crate::owned_no_exit_evidence_bridge_v1::{
    create_owned_no_exit_evidence_bridge, OwnedNoExitEvidenceBridge,
};
use crate::paired_live_host::PairedLiveApi;
use crate::paired_neutral_recovery_host_v1::{
    open_paired_neutral_recovery_host, PairedNeutralRecoveryDeps,
    PairedNeutralRecoveryHost,
};
use crate::paired_owned_exit_retirement_v1::build_paired_owned_exit_retirement;
use crate::gates::normalize_official_fill_tick;
use crate::single_owned_no_exit_signal_v1::evaluate_single_owned_no_exit;
use crate::transport::Guard;
use baal_service_runtime::fixed::{instant, price, SCALE, TICK};
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

const TID: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    static NULL: V = V::Null;
    v.get(k).unwrap_or(&NULL)
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("OWNED_NO_PROFIT_{code}"))
    }
}
fn read(p: &Path) -> Result<V, String> {
    serde_json::from_str(&fs::read_to_string(p).map_err(|e| e.to_string())?)
        .map_err(|e| format!("OWNED_NO_PROFIT_REQUEST:{e}"))
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
fn id_ok(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
        && s.parse::<i64>().is_ok()
}
fn run_id_ok(s: &str) -> bool {
    s.strip_prefix("sig-pilot-paired-").is_some_and(|r| {
        (1..=60).contains(&r.len())
            && r.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}

// ownedNoProfitHostIdentity(workspace)
pub fn owned_no_profit_host_identity(workspace: &Path) -> Result<V, String> {
    crate::frozen::collect_frozen_paper_files(
        workspace,
        &[
            "execution/owned-no-profit-host-v1.mjs",
            "execution/scoped-rest-receipts.mjs",
        ],
    )
}

// economic(p): the invariant fields two reconciliation proofs must agree on.
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

struct Source {
    bytes: Vec<u8>,
    hash: String,
    journal: Journal,
    rows: Vec<V>,
}

fn source(file: &Path) -> Result<Source, String> {
    let bytes = fs::read(file).map_err(|e| format!("OWNED_NO_PROFIT_SOURCE_BOUND:{e}"))?;
    need(
        !bytes.is_empty() && bytes.len() <= 16 * 1024 * 1024 && bytes.last() == Some(&b'\n'),
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

#[allow(clippy::type_complexity)]
pub struct OwnedNoProfitHostDeps {
    pub universe: V,
    pub api: Rc<RefCell<PairedLiveApi>>,
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub get_account_lease: Rc<dyn Fn() -> V>,
    pub get_other_state: Rc<dyn Fn() -> V>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> bool>,
    pub get_read_hint: Rc<dyn Fn() -> V>,
    pub now: Rc<dyn Fn() -> f64>,
    pub workspace: PathBuf,
}

struct Inner {
    deps: OwnedNoProfitHostDeps,
    runtime_dir: PathBuf,
    request: V,
    request_hash: String,
    code: V,
    recovery_descriptor: V,
    start: i64,
    end: i64,
    request_file: PathBuf,
    recovery_file: PathBuf,
    authority_file: PathBuf,
    dir: PathBuf,
    file: PathBuf,
    actor_file: PathBuf,
    funding_file: PathBuf,
    lease_identity_hash: String,
    lease: Option<OwnerLease>,
    journal: Option<Journal>,
    recovery: Option<PairedNeutralRecoveryHost>,
    exit: Option<OwnedNoExitController>,
    bridge: Option<Rc<OwnedNoExitEvidenceBridge>>,
    state: V,
    installed: V,
    retirement_input: V,
    anchor: Option<Source>,
    funding: Option<Source>,
    buy: V,
    original_after: V,
    closed: bool,
    failed: bool,
    busy: bool,
    writing: Option<&'static str>,
    last_failure: Option<String>,
    last_action: String,
    local_size: u64,
    local_hash: String,
}

pub struct OwnedNoProfitHost {
    inner: Rc<RefCell<Inner>>,
}

fn account_lease(inner: &Rc<RefCell<Inner>>) -> Result<V, String> {
    let l = (inner.borrow().deps.get_account_lease)();
    need(
        get(&l, "ownerId").as_str() == Some("Codex")
            && get(&l, "tournamentId").as_str() == Some(TID)
            && hex(get(&l, "leaseId")),
        "ACCOUNT_LEASE",
    )?;
    Ok(l)
}
fn lease_hash(l: &V) -> String {
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
        recovery_file,
        authority_file,
        request_hash,
        recovery_hash,
        authority_hash,
        code,
        workspace,
        lease_identity_hash,
        actor_file,
        funding_file,
        anchor_hash,
        funding_hash,
    ) = {
        let i = inner.borrow();
        (
            i.closed,
            i.failed,
            i.request_file.clone(),
            i.recovery_file.clone(),
            i.authority_file.clone(),
            i.request_hash.clone(),
            get(&i.request, "recoveryDescriptorHash").as_str().unwrap_or("").to_string(),
            get(&i.request, "authorityHash").as_str().unwrap_or("").to_string(),
            i.code.clone(),
            i.deps.workspace.clone(),
            i.lease_identity_hash.clone(),
            i.actor_file.clone(),
            i.funding_file.clone(),
            i.anchor.as_ref().map(|a| a.hash.clone()),
            i.funding.as_ref().map(|f| f.hash.clone()),
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
                .map_err(|e| format!("OWNED_NO_PROFIT_{}", e.message))?;
        }
    }
    need(
        payload_hash(&read(&request_file)?)? == request_hash
            && payload_hash(&read(&recovery_file)?)? == recovery_hash
            && payload_hash(&read(&authority_file)?)? == authority_hash
            && payload_hash(&owned_no_profit_host_identity(&workspace)?)?
                == payload_hash(&code)?,
        "SOURCE_CHANGED",
    )?;
    need(
        lease_hash(&account_lease(inner)?) == lease_identity_hash,
        "LEASE_CHANGED",
    )?;
    if let Some(a) = anchor_hash {
        let ab = fs::read(&actor_file)
            .map_err(|e| format!("OWNED_NO_PROFIT_PARENT_SOURCE_CHANGED:{e}"))?;
        let fb = fs::read(&funding_file)
            .map_err(|e| format!("OWNED_NO_PROFIT_PARENT_SOURCE_CHANGED:{e}"))?;
        need(
            sha(&ab) == a && sha(&fb) == funding_hash.unwrap_or_default(),
            "PARENT_SOURCE_CHANGED",
        )?;
    }
    Ok(())
}

fn allowed(inner: &Rc<RefCell<Inner>>) -> bool {
    fixed(inner).is_ok()
}

fn entry_allowed(inner: &Rc<RefCell<Inner>>) -> bool {
    if !allowed(inner) {
        return false;
    }
    let i = inner.borrow();
    let now = (i.deps.now)();
    if !(now >= i.start as f64 && now < i.end as f64) {
        return false;
    }
    match i.exit.as_ref() {
        Some(e) => {
            let st = e.status();
            let intent = get(&st, "intent");
            if intent.is_null() {
                true
            } else {
                instant_v(get(&get(intent, "body"), "expirationDate"))
                    .map(|x| x <= i.end)
                    .unwrap_or(false)
            }
        }
        None => true,
    }
}

fn negative_read_hint(inner: &Rc<RefCell<Inner>>) -> bool {
    (|| -> Result<bool, String> {
        let (h, universe_hash, market_id, exchange_id, buy_cost) = {
            let i = inner.borrow();
            (
                (i.deps.get_read_hint)(),
                get(&i.deps.universe, "hash").clone(),
                get(&i.request, "marketId").clone(),
                get(&i.request, "exchangeId").clone(),
                price(get(&i.buy, "scopedPrincipalCost").as_str().unwrap_or(""))
                    .unwrap_or(0),
            )
        };
        need(
            !h.is_null()
                && get(&h, "basis").as_str() == Some("documented_bulk_best_ask")
                && get(&h, "tournamentId").as_str() == Some(TID)
                && get(&h, "universeHash") == &universe_hash
                && get(&h, "marketId") == &market_id
                && get(&h, "exchangeId") == &exchange_id
                && hex(get(&h, "sourceHash")),
            "READ_HINT",
        )?;
        let a = instant_v(get(&h, "requestStartedAt"))?;
        let z = instant_v(get(&h, "receivedAt"))?;
        let n = (inner.borrow().deps.now)();
        need(a as f64 <= z as f64 && z as f64 <= n && n - a as f64 <= 20000.0, "READ_HINT_CLOCK")?;
        let best = price(
            &get(&normalize_official_fill_tick(get(&h, "bestAsk"))?, "price")
                .as_str()
                .unwrap_or("")
                .to_string(),
        )?;
        Ok(best > SCALE - buy_cost - 2 * TICK)
    })()
    .unwrap_or(false)
}

fn core(inner: &Rc<RefCell<Inner>>) -> Result<V, String> {
    let s = validate_execution_state(&(inner.borrow().deps.get_other_state)())
        .map_err(|e| format!("OWNED_NO_PROFIT_{e}"))?;
    let i = inner.borrow();
    need(
        get(&s, "tournamentId").as_str() == Some(TID)
            && !get(&s, "owned").as_object().is_some_and(|m| {
                m.values().any(|p| {
                    get(p, "exchangeId") == get(&i.request, "exchangeId")
                        || get(p, "marketId") == get(&i.request, "marketId")
                })
            }),
        "FOREIGN_CORE_HOLDING",
    )?;
    Ok(s)
}

fn global_risk(inner: &Rc<RefCell<Inner>>) -> Result<V, String> {
    let s = core(inner)?;
    let (x, run_id, stopped, at, parent, lease) = {
        let i = inner.borrow();
        (
            i.exit.as_ref().map(|e| e.status()),
            get(&i.request, "runId").clone(),
            !allowed(inner),
            iso((i.deps.now)()),
            if i.anchor.is_some() {
                json!({
                    "hash": i.anchor.as_ref().unwrap().hash,
                    "proofHash": get(&i.buy, "verificationHash"),
                })
            } else {
                V::Null
            },
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
        "tournamentId": TID,
        "complete": true,
        "soleCollector": true,
        "stopped": stopped,
        "at": at,
        "unresolvedIntentIds": unresolved,
        "sourceHash": payload_hash(&json!({
            "core": s,
            "parent": parent,
            "exit": x,
            "lease": lease,
        }))?,
    }))
}

fn ownership(inner: &Rc<RefCell<Inner>>) -> Result<V, String> {
    fixed(inner)?;
    let i = inner.borrow();
    need(
        get(&i.state, "retirement").is_null()
            && !i
                .exit
                .as_ref()
                .is_some_and(|e| js_truthy_i(get(&e.verified_exit_proof(), "filledQuantity"))),
        "LOT_ALREADY_SOLD",
    )?;
    Ok(json!({
        "ownerId": "Codex",
        "runId": get(&i.request, "runId"),
        "tournamentId": TID,
        "marketId": get(&i.request, "marketId"),
        "exchangeId": get(&i.request, "exchangeId"),
        "lotId": get(&get(&i.buy, "lot"), "lotId"),
        "buyProofHash": get(&i.buy, "verificationHash"),
        "quantity": 1,
        "sourceHash": payload_hash(&json!({
            "actor": i.anchor.as_ref().map(|a| a.hash.clone()),
            "proofHash": get(&i.buy, "verificationHash"),
            "claim": get(&i.state, "reservation"),
        }))?,
        "at": iso((i.deps.now)()),
    }))
}

fn js_truthy_i(v: &V) -> bool {
    baal_service_runtime::json_text::js_truthy(Some(v))
}

fn genesis(inner: &Rc<RefCell<Inner>>) -> V {
    let i = inner.borrow();
    json!({
        "schemaVersion": 1,
        "requestHash": i.request_hash,
        "buyProofHash": get(&i.buy, "verificationHash"),
        "parentActorHash": i.anchor.as_ref().map(|a| a.hash.clone()).unwrap_or_default(),
        "reservation": V::Null,
        "retirement": V::Null,
        "retiredOriginalExitProofHash": V::Null,
    })
}

fn validate_state(inner: &Rc<RefCell<Inner>>, next: &V, event: &V) -> Result<(), String> {
    let (request_hash, buy_proof_hash, anchor_hash, state, buy, anchor_h) = {
        let i = inner.borrow();
        (
            i.request_hash.clone(),
            get(&i.request, "buyProofHash").clone(),
            i.anchor.as_ref().map(|a| a.hash.clone()).unwrap_or_default(),
            i.state.clone(),
            i.buy.clone(),
            i.anchor.as_ref().map(|a| a.hash.clone()).unwrap_or_default(),
        )
    };
    need(
        get(next, "requestHash").as_str() == Some(request_hash.as_str())
            && get(next, "buyProofHash") == &buy_proof_hash
            && get(next, "parentActorHash").as_str() == Some(anchor_h.as_str()),
        "LOCAL_BINDING",
    )?;
    let expected: V;
    match get(event, "kind").as_str().unwrap_or("") {
        "inventory_claim" => {
            need(
                !state.is_null() && get(&state, "retirement").is_null(),
                "CLAIM_PHASE",
            )?;
            let r = get(event, "record");
            let mut body = r.clone();
            body.as_object_mut()
                .and_then(|m| m.remove("reservationHash"));
            need(
                get(r, "reservationHash").as_str() == Some(payload_hash(&body)?.as_str())
                    && get(r, "buyProofHash") == get(&buy, "verificationHash")
                    && get(r, "lotId") == get(&get(&buy, "lot"), "lotId")
                    && get(r, "ownerId").as_str() == Some("Codex")
                    && get(r, "runId") == get(&inner.borrow().request, "runId")
                    && get(r, "tournamentId").as_str() == Some(TID)
                    && get(r, "marketId") == get(&inner.borrow().request, "marketId")
                    && get(r, "exchangeId") == get(&inner.borrow().request, "exchangeId")
                    && get(r, "originalPrincipalCost")
                        == get(&buy, "scopedPrincipalCost")
                    && get(r, "cashReserved").as_str() == Some("0.000000")
                    && [0, 1].contains(&js_int(get(r, "remainingQuantity")).unwrap_or(-1))
                    && get(r, "sellIntentId").as_str().is_some_and(|s| !s.is_empty()),
                "CLAIM",
            )?;
            let reservation = get(&state, "reservation");
            if !reservation.is_null() {
                need(
                    get(r, "sellIntentId") == get(reservation, "sellIntentId"),
                    "SECOND_INTENT",
                )?;
                need(
                    js_int(get(r, "remainingQuantity")).unwrap_or(-1)
                        <= js_int(get(reservation, "remainingQuantity")).unwrap_or(-1),
                    "LOT_RESTORED",
                )?;
            }
            let mut e2 = state.clone();
            e2.as_object_mut()
                .unwrap()
                .insert("reservation".into(), r.clone());
            expected = e2;
        }
        "retired" => {
            need(
                !state.is_null()
                    && get(&state, "retirement").is_null()
                    && js_int(get(&get(&state, "reservation"), "remainingQuantity")) == Some(0)
                    && get(&get(&state, "reservation"), "terminalSell") == &json!(true)
                    && hex(get(event, "originalExitProofHash")),
                "RETIREMENT_PHASE",
            )?;
            let fresh = verify_scoped_rest_no_sell(get(event, "sellReverification"))
                .map_err(|e| format!("OWNED_NO_PROFIT_{e}"))?;
            let proof = build_paired_owned_exit_retirement(get(event, "evidence"))
                .map_err(|e| format!("OWNED_NO_PROFIT_{e}"))?;
            let fund_row = get(get(event, "evidence"), "fundingRegistryRecord");
            let funding_rows = inner
                .borrow()
                .funding
                .as_ref()
                .map(|f| f.rows.clone())
                .unwrap_or_default();
            need(
                payload_hash(get(&get(event, "sellReverification"), "originalBuyProof"))?
                    == payload_hash(&buy)?
                    && payload_hash(&fresh)?
                        == payload_hash(get(&get(event, "evidence"), "sellProof"))?
                    && get(&fresh, "intentId")
                        == get(&get(&state, "reservation"), "sellIntentId")
                    && get(&proof, "originalBuyProofHash")
                        == get(&buy, "verificationHash")
                    && get(&get(&proof, "originalActorSource"), "sha256").as_str()
                        == Some(anchor_hash.as_str()),
                "RETIREMENT_REPLAY",
            )?;
            need(
                funding_rows.iter().any(|r| {
                    get(r, "hash") == get(fund_row, "hash")
                        && payload_hash(r).ok().as_deref()
                            == payload_hash(fund_row).ok().as_deref()
                }),
                "RETIREMENT_FUNDING_MEMBERSHIP",
            )?;
            let mut e2 = state.clone();
            {
                let m = e2.as_object_mut().unwrap();
                m.insert("retirement".into(), proof);
                m.insert(
                    "retiredOriginalExitProofHash".into(),
                    get(event, "originalExitProofHash").clone(),
                );
            }
            expected = e2;
        }
        _ => {
            need(
                get(event, "kind").as_str() == Some("initialized") && state.is_null(),
                "LOCAL_EVENT",
            )?;
            expected = genesis(inner);
        }
    }
    need(
        payload_hash(next)? == payload_hash(&expected)?,
        "LOCAL_REDUCER_STATE",
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
    validate_state(inner, next, event)?;
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
        "CLAIM_NOT_ACTUAL_CONTROLLER",
    )?;
    let risk = global_risk(inner)?;
    need(
        get(&risk, "unresolvedIntentIds")
            .as_array()
            .is_some_and(|us| us.iter().all(|u| u == get(record, "sellIntentId"))),
        "GLOBAL_UNKNOWN",
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

fn open_exit(inner: &Rc<RefCell<Inner>>) -> Result<(), String> {
    if inner.borrow().exit.is_some() {
        return Ok(());
    }
    let (exit_dir, buy, original_after, dispatch, receipt_of, now) = {
        let i = inner.borrow();
        (
            i.dir.join("controller"),
            i.buy.clone(),
            i.original_after.clone(),
            i.deps.api.borrow().dispatch.clone(),
            i.deps.receipt_of.clone(),
            i.deps.now.clone(),
        )
    };
    let inner_co = inner.clone();
    let can_operate = Rc::new(move || {
        let i = inner_co.borrow();
        allowed(&inner_co)
            && (i.writing != Some("place") || entry_allowed(&inner_co))
    });
    let inner_al = inner.clone();
    let assert_account_lease = Rc::new(move || account_lease(&inner_al));
    let inner_ow = inner.clone();
    let get_ownership = Rc::new(move || ownership(&inner_ow));
    let inner_ge = inner.clone();
    let get_evidence = Rc::new(move |c: &V| -> Result<V, String> {
        inner_ge
            .borrow()
            .bridge
            .as_ref()
            .map(|b| b.get_evidence(c))
            .unwrap_or(Err("OWNED_NO_PROFIT_EVIDENCE".into()))
    });
    let inner_ir = inner.clone();
    let install_exit_reservation =
        Rc::new(move |r: &V| claim(&inner_ir, r));
    let now_i = Rc::new(move || now() as i64);
    let exit = open_owned_no_exit_controller(
        &exit_dir,
        &buy,
        &original_after,
        &json!("midterm-elections"),
        OwnedNoExitControllerDeps {
            dispatch,
            raw_receipt_of: receipt_of,
            can_operate,
            assert_account_lease,
            get_ownership,
            get_evidence,
            install_exit_reservation,
            now: now_i,
        },
    )?;
    inner.borrow_mut().exit = Some(exit);
    Ok(())
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
    let original_proof = {
        let i = inner.borrow();
        i.exit
            .as_ref()
            .map(|e| e.verified_exit_proof())
            .unwrap_or(V::Null)
    };
    if !get(&inner.borrow().state, "retirement").is_null()
        || js_int(get(&original_proof, "filledQuantity")) != Some(1)
    {
        return Ok(());
    }
    let exit_file = inner.borrow().dir.join("controller").join("owned-no-exit-journal.jsonl");
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
                == get(&original_proof, "verificationHash"),
        "ACTUAL_EXIT_JOURNAL",
    )?;
    let (buy, original_after, read_at) = {
        let i = inner.borrow();
        (
            i.buy.clone(),
            i.original_after.clone(),
            iso((i.deps.now)()),
        )
    };
    let sell_reverification = {
        let mut m = evidence.clone();
        let mm = m.as_object_mut().unwrap();
        mm.insert(
            "binding".into(),
            json!({
                "ownerId": get(&buy, "ownerId"),
                "runId": get(&buy, "runId"),
                "tournamentId": TID,
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
    let proof = verify_scoped_rest_no_sell(&sell_reverification)
        .map_err(|e| format!("OWNED_NO_PROFIT_{e}"))?;
    need(
        payload_hash(&economic(&proof))? == payload_hash(&economic(&original_proof))?,
        "EXIT_RECONCILIATION_CHANGED",
    )?;
    let l = account_lease(inner)?;
    let lease_proof = {
        let mut lp = l.clone();
        let m = lp.as_object_mut().unwrap();
        m.insert("at".into(), json!(read_at));
        m.insert(
            "sourceHash".into(),
            json!(payload_hash(&json!({
                "observedLease": l,
                "observedAt": read_at,
                "after": get(&get(&proof, "sourceReceipts"), "after"),
            }))?),
        );
        lp
    };
    let (anchor_bytes, anchor_seq, anchor_prev, fund_last, prefix_bytes, prefix_hash, risk) = {
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
            global_risk(inner)?,
        )
    };
    let inputs = json!({
        "originalBuyProof": buy,
        "sellProof": proof,
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
        "readAt": read_at,
    });
    let overlay = build_paired_owned_exit_retirement(&inputs)
        .map_err(|e| format!("OWNED_NO_PROFIT_{e}"))?;
    let mut next = inner.borrow().state.clone();
    {
        let m = next.as_object_mut().unwrap();
        m.insert("retirement".into(), overlay);
        m.insert(
            "retiredOriginalExitProofHash".into(),
            get(&original_proof, "verificationHash").clone(),
        );
    }
    append(
        inner,
        &next,
        &json!({
            "kind": "retired",
            "evidence": inputs,
            "sellReverification": sell_reverification,
            "originalExitProofHash": get(&original_proof, "verificationHash"),
            "at": read_at,
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
            .unwrap_or(json!("awaiting_protected_profit"))
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
    let retired_budget = get(&get(&i.state, "retirement"), "preservedCumulativeBuy").clone();
    json!({
        "schemaVersion": 1,
        "mode": "single_owned_no_protected_profit_host_v1",
        "hostVersion": 4,
        "ownerId": "Codex",
        "runId": get(&i.request, "runId"),
        "tournamentId": TID,
        "marketId": get(&i.request, "marketId"),
        "exchangeId": get(&i.request, "exchangeId"),
        "requestHash": i.request_hash,
        "recoveryDescriptorHash": get(&i.request, "recoveryDescriptorHash"),
        "authorityHash": get(&i.request, "authorityHash"),
        "buyProofHash": get(&i.buy, "verificationHash"),
        "originalBuyOrderId": get(&i.buy, "orderId"),
        "originalPrincipalCost": get(&i.buy, "scopedPrincipalCost"),
        "startedAt": get(&i.request, "startedAt"),
        "entryExpiresAt": get(&i.request, "entryExpiresAt"),
        "originalReviewExpiresAt": i.anchor.as_ref()
            .map(|a| get(&get(&get(&a.journal.last.clone().unwrap_or(V::Null), "state"), "plan"), "reviewExpiresAt").clone())
            .unwrap_or(V::Null),
        "phase": phase,
        "entryAllowed": entry_allowed(inner),
        "blocksLegacyWrites": true,
        "orders": intent.as_ref().filter(|v| !v.is_null()).map(|v| json!([v])).unwrap_or(json!([])),
        "orderCount": if intent.as_ref().is_some_and(|v| !v.is_null()) { 1 } else { 0 },
        "owned": owned,
        "historicalParentFunding": get(&funding_state, "funding"),
        "currentBudget": if !get(&i.state, "retirement").is_null() {
            json!({
                "ownedPrincipal": "0.000000",
                "reservedCash": "0.000000",
                "cumulativeBuy": retired_budget,
            })
        } else {
            get(&funding_state, "funding").clone()
        },
        "realizedPnl": p.as_ref().map(|v| get(v, "realizedPnl").clone()).unwrap_or(json!("0.000000")),
        "scopedProceeds": p.as_ref().map(|v| get(v, "scopedProceeds").clone()).unwrap_or(json!("0.000000")),
        "retirement": get(&i.state, "retirement"),
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
    let (f1, f2, now) = {
        let i = inner.borrow();
        (
            i.runtime_dir.join("owned-no-profit-stats.json"),
            i.runtime_dir.join("paired-live-stats.json"),
            (i.deps.now)(),
        )
    };
    let mut s = status(inner);
    s.as_object_mut()
        .unwrap()
        .insert("observedAt".into(), json!(iso(now)));
    for f in [&f1, &f2] {
        atomic_json(f, &s).map_err(|e| {
            inner.borrow_mut().failed = true;
            format!("{e}:fatal:durableRecordingFailed")
        })?;
    }
    Ok(status(inner))
}

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
                first = Some(format!("OWNED_NO_PROFIT_{}", e.message));
            }
        }
    }
    match first {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

// JS cooldown(e): persists api retry state when the wire error carries
// retryAfterMs. String errors cannot carry it, so there is nothing to persist.
fn cooldown(_inner: &Rc<RefCell<Inner>>, _e: &str) {}

// openOwnedNoProfitHost({runtimeDir,workspace,universe,api,receiptOf,getAccountLease,getOtherState,canOperate,hasReadHeadroom,getReadHint,now})
pub fn open_owned_no_profit_host(
    runtime_dir: &Path,
    deps: OwnedNoProfitHostDeps,
) -> Result<Option<OwnedNoProfitHost>, String> {
    let request_file = runtime_dir.join("owned-no-profit-request.json");
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
    let code = owned_no_profit_host_identity(&deps.workspace)?;
    let mut keys = vec![
        "schemaVersion",
        "kind",
        "ownerId",
        "runId",
        "tournamentId",
        "marketId",
        "exchangeId",
        "recoveryDescriptorHash",
        "buyProofHash",
        "startedAt",
        "entryExpiresAt",
        "authorityHash",
        "codeIdentity",
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
            && get(&request, "kind").as_str()
                == Some("single_owned_no_protected_profit_exit_v1")
            && get(&request, "ownerId").as_str() == Some("Codex")
            && get(&request, "tournamentId").as_str() == Some(TID)
            && get(&request, "runId").as_str().is_some_and(run_id_ok)
            && ["marketId", "exchangeId"]
                .iter()
                .all(|k| get(&request, k).as_str().is_some_and(id_ok))
            && ["recoveryDescriptorHash", "buyProofHash", "authorityHash"]
                .iter()
                .all(|k| hex(get(&request, k)))
            && payload_hash(get(&request, "codeIdentity"))? == payload_hash(&code)?,
        "REQUEST",
    )?;
    let start = instant_v(get(&request, "startedAt"))?;
    let end = instant_v(get(&request, "entryExpiresAt"))?;
    need(
        start < end && end - start <= 3_600_000 && (start as f64) <= (deps.now)(),
        "WINDOW",
    )?;
    let recovery_file = runtime_dir.join("paired-neutral-ack-recovery.json");
    let recovery_descriptor = read(&recovery_file)?;
    let authority_file = runtime_dir.join("paired-live-authority.json");
    let authority = read(&authority_file)?;
    need(
        payload_hash(&recovery_descriptor)?
            == get(&request, "recoveryDescriptorHash").as_str().unwrap_or("")
            && get(&recovery_descriptor, "ownerId") == get(&request, "ownerId")
            && get(&recovery_descriptor, "runId") == get(&request, "runId")
            && get(&recovery_descriptor, "tournamentId").as_str() == Some(TID)
            && get(&authority, "kind").as_str() == Some("direct_human_authorized_bot_update")
            && get(&authority, "ownerId").as_str() == Some("Codex")
            && get(&authority, "exploratory") == &json!(true)
            && get(&authority, "profitabilityEstablished") == &json!(false)
            && payload_hash(&authority)? == get(&request, "authorityHash").as_str().unwrap_or("")
            && get(&request, "authorityHash") == get(&recovery_descriptor, "authorityHash"),
        "AUTHORITY_OR_RECOVERY",
    )?;
    need(
        get(&deps.universe, "tournamentId").as_str() == Some(TID)
            && get(&deps.universe, "exchanges").as_array().is_some_and(|xs| {
                xs.iter().any(|m| {
                    get(m, "marketId") == get(&request, "marketId")
                        && get(m, "exchangeId") == get(&request, "exchangeId")
                        && get(m, "option").as_str() == Some("YES")
                })
            }),
        "UNIVERSE",
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
    let file = dir.join("owned-no-profit-journal.jsonl");
    let lease0 = account_lease_raw(&deps)?;
    let lease_identity_hash = lease_hash(&lease0);
    let inner = Rc::new(RefCell::new(Inner {
        request,
        request_hash,
        code,
        recovery_descriptor,
        start,
        end,
        request_file,
        recovery_file,
        authority_file,
        dir,
        file,
        actor_file,
        funding_file,
        lease_identity_hash,
        deps,
        runtime_dir: runtime_dir.to_path_buf(),
        lease: None,
        journal: None,
        recovery: None,
        exit: None,
        bridge: None,
        state: V::Null,
        installed: V::Null,
        retirement_input: V::Null,
        anchor: None,
        funding: None,
        buy: V::Null,
        original_after: V::Null,
        closed: false,
        failed: false,
        busy: false,
        writing: None,
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
        need(recovery.is_some(), "RECOVERY_REQUIRED")?;
        inner.borrow_mut().recovery = recovery;
        let anchor = source(&inner.borrow().actor_file.clone())?;
        let funding = source(&inner.borrow().funding_file.clone())?;
        let actor_state = get(
            &anchor.journal.last.clone().unwrap_or(V::Null),
            "state",
        )
        .clone();
        let orders = get(&actor_state, "orders").as_array().cloned().unwrap_or_default();
        let buy_hash = get(&inner.borrow().request, "buyProofHash").clone();
        need(
            orders.len() == 1
                && get(&orders[0], "phase").as_str() == Some("reconciled")
                && !js_truthy_i(get(&orders[0], "semanticMismatch"))
                && !js_truthy_i(get(&orders[0], "cancelPending"))
                && get(&get(&orders[0], "proof"), "verificationHash") == &buy_hash,
            "VERIFIED_BUY_REQUIRED",
        )?;
        let buy = get(&orders[0], "proof").clone();
        {
            let i = inner.borrow();
            need(
                get(&buy, "scopedLotVerified") == &json!(true)
                    && get(&buy, "filledQuantity") == &json!(1)
                    && get(&buy, "scopedPrincipalCost").as_str() == Some("0.070000")
                    && get(&buy, "marketId") == get(&i.request, "marketId")
                    && get(&buy, "exchangeId") == get(&i.request, "exchangeId")
                    && get(&buy, "ownerId").as_str() == Some("Codex")
                    && get(&buy, "runId") == get(&i.request, "runId")
                    && get(&buy, "tournamentId").as_str() == Some(TID),
                "BUY_SCOPE",
            )?;
        }
        let reconciliation = anchor.rows.iter().rev().find(|r| {
            let kind_ok = get(&get(&get(r, "event"), "input"), "kind").as_str()
                == Some("reconciled");
            let order0 = get(&get(r, "state"), "orders")
                .as_array()
                .and_then(|a| a.first().cloned())
                .unwrap_or(V::Null);
            kind_ok && get(&get(&order0, "proof"), "verificationHash") == &buy_hash
        });
        need(
            reconciliation
                .map(|r| {
                    !get(
                        &get(&get(&get(r, "event"), "input").clone(), "evidence").clone(),
                        "after",
                    )
                    .is_null()
                })
                .unwrap_or(false),
            "BUY_SOURCE_EVENT",
        )?;
        let original_after = get(
            &get(&get(reconciliation.unwrap(), "event"), "input")
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
            let (d, rid, now) = {
                let i = inner.borrow();
                (
                    i.dir.clone(),
                    get(&i.request, "runId").as_str().unwrap_or("").to_string(),
                    (i.deps.now)(),
                )
            };
            inner.borrow_mut().lease = Some(
                acquire_owner_lease(&d, "Codex", &rid, "owned-no-profit-host-v1", now)
                    .map_err(|e| format!("OWNED_NO_PROFIT_{}", e.message))?,
            );
        }
        let file = inner.borrow().file.clone();
        fs::create_dir_all(inner.borrow().dir.clone()).map_err(|e| e.to_string())?;
        if !file.exists() {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&file)
                .map_err(|e| format!("OWNED_NO_PROFIT_JOURNAL:{e}"))?;
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
                serde_json::from_str(line).map_err(|_| "OWNED_NO_PROFIT_JOURNAL".to_string())?;
            validate_state(&inner, get(&r, "state"), get(&r, "event"))?;
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
            if get(&get(&r, "event"), "kind").as_str() == Some("retired") {
                inner.borrow_mut().retirement_input = get(&r, "event").clone();
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
        // evidence bridge bound to the scoped lot
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
                "tournamentId": TID,
                "tournamentSlug": "midterm-elections",
                "marketId": get(&i.request, "marketId"),
                "exchangeId": get(&i.request, "exchangeId"),
            })
        };
        let api_dispatch = api2.borrow().dispatch.clone();
        let dispatch = Rc::new(move |s: &V| -> Result<V, String> {
            (api_dispatch)(s, None, None).map_err(|e| e.message)
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
        // if an exit journal already exists, open the controller
        let exit_journal = inner
            .borrow()
            .dir
            .join("controller")
            .join("owned-no-exit-journal.jsonl");
        if exit_journal.exists() {
            open_exit(&inner)?;
        }
        if !get(&inner.borrow().state, "reservation").is_null() {
            let sell_intent = get(&get(&inner.borrow().state, "reservation"), "sellIntentId").clone();
            let actual_intent = inner
                .borrow()
                .exit
                .as_ref()
                .map(|e| get(&get(&e.status(), "intent"), "intentId").clone())
                .unwrap_or(V::Null);
            need(sell_intent == actual_intent, "CLAIM_EXIT_JOURNAL")?;
        }
        if !get(&inner.borrow().state, "retirement").is_null() {
            let original = inner
                .borrow()
                .exit
                .as_ref()
                .map(|e| e.verified_exit_proof())
                .unwrap_or(V::Null);
            let retirement_input = inner.borrow().retirement_input.clone();
            need(
                get(&original, "verificationHash")
                    == get(&inner.borrow().state, "retiredOriginalExitProofHash")
                    && payload_hash(&economic(&original))?
                        == payload_hash(&economic(
                            &verify_scoped_rest_no_sell(get(&retirement_input, "sellReverification"))
                                .map_err(|e| format!("OWNED_NO_PROFIT_{e}"))?,
                        ))?,
                "RETIREMENT_EXIT_JOURNAL",
            )?;
        }
        publish(&inner, "verified_owned_lot_loaded")?;
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
    Ok(Some(OwnedNoProfitHost { inner }))
}

fn account_lease_raw(deps: &OwnedNoProfitHostDeps) -> Result<V, String> {
    let l = (deps.get_account_lease)();
    need(
        get(&l, "ownerId").as_str() == Some("Codex")
            && get(&l, "tournamentId").as_str() == Some(TID)
            && hex(get(&l, "leaseId")),
        "ACCOUNT_LEASE",
    )?;
    Ok(l)
}

impl OwnedNoProfitHost {
    // authorizes(spec): GET reads go to bridge/recovery; writes need the exit
    // controller to admit them (POST also requires the entry window).
    pub fn authorizes(&self, spec: &V, before_dispatch: &Guard, before_network: &Guard) -> bool {
        if !allowed(&self.inner) {
            return false;
        }
        let i = self.inner.borrow();
        if get(spec, "method").as_str() == Some("GET") {
            let bridge_ok = i
                .bridge
                .as_ref()
                .map(|b| b.authorizes(spec, before_dispatch, before_network))
                .unwrap_or(false);
            if bridge_ok {
                return true;
            }
            // recovery's authorizes uses String-typed guards; adapt.
            let bd_s = |p: &V, s: &V| -> Result<bool, String> {
                before_dispatch(p, s).map_err(|e| e.code)
            };
            let bn_s = |p: &V, s: &V| -> Result<bool, String> {
                before_network(p, s).map_err(|e| e.code)
            };
            return i
                .recovery
                .as_ref()
                .map(|r| r.authorizes(spec, &bd_s, &bn_s))
                .unwrap_or(false);
        }
        i.exit
            .as_ref()
            .map(|e| {
                e.authorizes(spec, Some(before_dispatch), Some(before_network))
                    && (get(spec, "method").as_str() != Some("POST") || entry_allowed(&self.inner))
            })
            .unwrap_or(false)
    }
    pub fn on_fetch_start(&self, meta: &V) -> Result<bool, String> {
        match self.inner.borrow().exit.as_ref() {
            Some(e) => e.on_fetch_start(meta),
            None => Ok(false),
        }
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
                return publish(&self.inner, "owned_exit_retired");
            }
            let (x, end) = {
                let i = self.inner.borrow();
                (i.exit.as_ref().map(|e| e.status()), i.end)
            };
            let o = x.as_ref().map(|v| get(v, "intent").clone());
            let leg = {
                let i = self.inner.borrow();
                json!({
                    "marketId": get(&i.request, "marketId"),
                    "exchangeId": get(&i.request, "exchangeId"),
                })
            };
            if o.as_ref().is_some_and(|o| !get(o, "proofHash").is_null()) {
                if x.as_ref().is_some_and(|v| !get(v, "remainingOwnedLot").is_null()) {
                    return publish(&self.inner, "terminal_unfilled_original_lot_retained");
                }
                let o = o.unwrap();
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
                    let reservation = self
                        .inner
                        .borrow()
                        .exit
                        .as_ref()
                        .map(|e| e.exit_reservation())
                        .unwrap_or(V::Null);
                    claim(&self.inner, &reservation)?;
                }
                let evidence = {
                    let i = self.inner.borrow();
                    i.bridge
                        .as_ref()
                        .ok_or("OWNED_NO_PROFIT_EVIDENCE".to_string())?
                        .refresh_reconciliation(get(&o, "orderId"), &leg)?
                };
                fixed(&self.inner)?;
                retire(&self.inner, &evidence)?;
                self.inner.borrow_mut().last_failure = None;
                return publish(&self.inner, "verified_sale_retirement_completed");
            }
            let phase = x.as_ref().map(|v| get(v, "phase").as_str().unwrap_or("").to_string());
            if o.is_some()
                && ["dispatching", "uncertain", "semantic_mismatch"]
                    .contains(&phase.as_deref().unwrap_or(""))
            {
                return publish(&self.inner, "unknown_acceptance_blocks_replacement");
            }
            if o.as_ref().is_some_and(|o| !get(o, "orderId").is_null()) {
                let o = o.clone().unwrap();
                let attempt: Result<V, String> = (|| {
                    let evidence = {
                        let i = self.inner.borrow();
                        i.bridge
                            .as_ref()
                            .ok_or("OWNED_NO_PROFIT_EVIDENCE".to_string())?
                            .refresh_reconciliation(get(&o, "orderId"), &leg)?
                    };
                    fixed(&self.inner)?;
                    self.inner
                        .borrow()
                        .exit
                        .as_ref()
                        .ok_or("OWNED_NO_PROFIT_EXIT".to_string())?
                        .reconcile(&evidence)?;
                    retire(&self.inner, &evidence)?;
                    self.inner.borrow_mut().last_failure = None;
                    publish(&self.inner, "official_owned_sale_reconciled")
                })();
                match attempt {
                    Err(e)
                        if e == "OWNED_NO_EXIT_EVIDENCE_ORDER_SCOPE_OR_PHASE"
                            && (self.inner.borrow().deps.now)()
                                >= instant_v(get(&get(&o, "body"), "expirationDate"))?
                                    as f64
                            && !js_truthy_i(get(&o, "cancelPending")) =>
                    {
                        {
                            let i = self.inner.borrow();
                            i.bridge
                                .as_ref()
                                .ok_or("OWNED_NO_PROFIT_EVIDENCE".to_string())?
                                .refresh_cancel(get(&o, "orderId"), &leg)?;
                        }
                        self.inner.borrow_mut().writing = Some("cancel");
                        let cancel = self
                            .inner
                            .borrow()
                            .exit
                            .as_ref()
                            .map(|e| e.cancel_expired());
                        self.inner.borrow_mut().writing = None;
                        cancel.ok_or("OWNED_NO_PROFIT_EXIT".to_string())??;
                        self.inner.borrow_mut().last_failure = None;
                        return publish(&self.inner, "known_expired_sell_cancel_requested");
                    }
                    other => return other,
                }
            }
            let now = (self.inner.borrow().deps.now)();
            if !entry_allowed(&self.inner) || (o.is_none() && now + 30000.0 > end as f64) {
                return publish(&self.inner, "entry_window_closed_inventory_retained");
            }
            if o.is_none() && negative_read_hint(&self.inner) {
                self.inner.borrow_mut().last_failure = None;
                return publish(&self.inner, "holding_negative_fresh_read_hint");
            }
            {
                let i = self.inner.borrow();
                i.bridge
                    .as_ref()
                    .ok_or("OWNED_NO_PROFIT_EVIDENCE".to_string())?
                    .refresh_preparation(&leg)?;
            }
            fixed(&self.inner)?;
            let now = (self.inner.borrow().deps.now)();
            if !entry_allowed(&self.inner) || (o.is_none() && now + 30000.0 > end as f64) {
                return publish(&self.inner, "entry_window_closed_inventory_retained");
            }
            if o.is_none() {
                let e = {
                    let i = self.inner.borrow();
                    i.bridge
                        .as_ref()
                        .ok_or("OWNED_NO_PROFIT_EVIDENCE".to_string())?
                        .get_evidence(&json!({"operation": "prepare"}))?
                };
                let buy = self.inner.borrow().buy.clone();
                let signal = evaluate_single_owned_no_exit(&json!({
                    "buyProof": buy,
                    "positionsReceipt": get(&get(&e, "account"), "positions"),
                    "bookReceipt": get(&e, "book"),
                    "globalRisk": get(&e, "globalRisk"),
                    "tournamentSlug": "midterm-elections",
                    "now": iso((self.inner.borrow().deps.now)()),
                }));
                if get(&signal, "signalReady") != &json!(true) {
                    self.inner.borrow_mut().last_failure = None;
                    return publish(&self.inner, "holding_no_protected_profit");
                }
                open_exit(&self.inner)?;
                self.inner
                    .borrow()
                    .exit
                    .as_ref()
                    .ok_or("OWNED_NO_PROFIT_EXIT".to_string())?
                    .prepare()?;
            }
            need(entry_allowed(&self.inner), "ENTRY_WINDOW")?;
            self.inner.borrow_mut().writing = Some("place");
            let dispatch = self
                .inner
                .borrow()
                .exit
                .as_ref()
                .map(|e| e.dispatch_prepared());
            self.inner.borrow_mut().writing = None;
            dispatch.ok_or("OWNED_NO_PROFIT_EXIT".to_string())??;
            self.inner.borrow_mut().last_failure = None;
            publish(&self.inner, "owned_sell_acknowledgment_recorded")
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
                    "OWNED_EXIT_EVIDENCE_UNAVAILABLE".to_string()
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
                publish(&self.inner, "owned_exit_blocked")
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
