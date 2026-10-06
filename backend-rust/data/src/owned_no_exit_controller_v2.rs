// execution/owned-no-exit-controller-v2.mjs — explicit repair of one already
// closed, filled SELL ACK. The original byte prefix replays under its original
// code identity and null-only semantics. This revision cannot create, retry or
// cancel any competition order.
use baal_execution::core::payload_hash;
use baal_execution::gates::normalize_official_fill_tick;
use baal_execution::receipts::parse_number_lexemes;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::{amount, instant, price};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use baal_service_runtime::ledger::{acquire_owner_lease, OwnerLease};
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::no_rest_sell_reconciliation_v1::verify_scoped_rest_no_sell;
use crate::no_rest_sell_reconciliation_v2::verify_scoped_rest_no_sell_v2;
use crate::owned_no_exit_controller_v1 as v1;

fn need(v: bool, c: &str) -> Result<(), String> {
    if v { Ok(()) } else { Err(format!("OWNED_NO_CONTROLLER_{c}")) }
}
fn copy(v: &V) -> V { v.clone() }
fn sha(b: &[u8]) -> String { sha256(b) }
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn iso(n: i64) -> String { ms_to_iso(n as f64) }
fn get<'a>(v: &'a V, k: &str) -> &'a V { v.get(k).unwrap_or(&V::Null) }
fn amount_s(v: &V) -> Result<i128, String> { amount(v.as_str().unwrap_or("")) }
fn instant_v(v: &V) -> Result<i64, String> { instant(v.as_str().unwrap_or("")) }
fn fields(v: &V, keys: &[&str]) -> Result<(), String> {
    need(
        v.is_object() && !v.is_array()
            && v.as_object().unwrap().keys().all(|k| keys.contains(&k.as_str()))
            && keys.iter().all(|k| v.as_object().unwrap().contains_key(*k)),
        "EVIDENCE_FIELDS",
    )
}
fn sorted(rows: &V) -> V {
    let mut r = rows.as_array().cloned().unwrap_or_default();
    r.sort_by_key(|x| js_int(get(x, "id")).unwrap_or(0));
    V::Array(r)
}
fn raw(r: &V, method: &str, endpoint: &str) -> Result<V, String> {
    need(
        get(r, "method") == &json!(method)
            && get(r, "endpoint") == &json!(endpoint)
            && js_int(get(r, "status")).is_some()
            && get(r, "bodyText").as_str().is_some_and(|b| b.len() <= 8 * 1024 * 1024),
        "RAW_RECEIPT",
    )?;
    let hash = sha(get(r, "bodyText").as_str().unwrap_or("").as_bytes());
    need(
        r.get("sourceBodyHash").map(|x| x == &json!(hash)).unwrap_or(true),
        "RAW_RECEIPT_HASH",
    )?;
    need(
        instant_v(get(r, "receivedAt"))? >= instant_v(get(r, "requestStartedAt"))?,
        "RAW_RECEIPT_CLOCK",
    )?;
    let mut out = json!({
        "method": method, "endpoint": endpoint, "status": get(r, "status"),
        "bodyText": get(r, "bodyText"), "requestStartedAt": get(r, "requestStartedAt"),
        "receivedAt": get(r, "receivedAt"), "sourceBodyHash": hash,
    });
    if let Some(f) = r.get("fetchInvocationId") { out.as_object_mut().unwrap().insert("fetchInvocationId".into(), f.clone()); }
    if let Some(h) = r.get("requestBodyHash") { out.as_object_mut().unwrap().insert("requestBodyHash".into(), h.clone()); }
    Ok(out)
}

pub fn owned_no_exit_controller_v2_identity(execution_dir: &Path) -> V {
    let mut a = crate::paired_rest_controller::paired_rest_code_identity_file(execution_dir, "owned-no-exit-controller-v2.mjs");
    for (k, v) in crate::paired_rest_controller::paired_rest_code_identity_file(execution_dir, "owned-no-exit-controller-v1.mjs") {
        a.insert(k, v);
    }
    json!(a)
}
pub fn owned_no_exit_controller_v1_legacy_identity(execution_dir: &Path) -> V {
    v1::owned_no_exit_controller_identity(execution_dir)
}

fn canonical_neutral_ack(s: &V, r: &V) -> bool {
    (|| -> Result<bool, String> {
        let (d, numbers) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
        let o = get(s, "intent");
        let exact = |k: &str| -> bool {
            let lex = numbers.get(&js_stringify(&json!([k]))).cloned().unwrap_or_default();
            let int_part = lex.split('.').next().unwrap_or("");
            !int_part.is_empty() && int_part.bytes().all(|b| b.is_ascii_digit())
                && (int_part == "0" || !int_part.starts_with('0'))
                && lex.split('.').nth(1).map(|f| f.bytes().all(|b| b == b'0')).unwrap_or(true)
                && int_part.parse::<i128>().ok() == js_int(get(&d, k)).map(|x| x as i128)
        };
        let total = amount_s(&json!(numbers.get(&js_stringify(&json!(["totalCost"]))).cloned().unwrap_or_default()))?;
        let limit = price(get(&binding(s), "limit").as_str().unwrap_or(""))?;
        let economics = crate::no_neutral_sell_all_v1::inspect_no_sell_ack_all_economics(r)?;
        Ok(
            get(r, "status") == &json!(200)
                && js_int(get(&d, "orderId")).is_some_and(|x| x > 0)
                && js_key_eq(get(&d, "orderId"), get(o, "orderId"))
                && get(&d, "exchangeId") == get(get(o, "body"), "exchangeId")
                && get(&d, "side") == &json!("no")
                && get(&d, "action") == &json!("sell")
                && get(&d, "quantity") == &json!(1)
                && ["orderId", "quantity", "quantityTraded", "remainingQuantity"].iter().all(|k| exact(k))
                && [0, 1].contains(&js_int(get(&d, "quantityTraded")).unwrap_or(-1))
                && [0, 1].contains(&js_int(get(&d, "remainingQuantity")).unwrap_or(-1))
                && js_int(get(&d, "quantityTraded")).unwrap() + js_int(get(&d, "remainingQuantity")).unwrap() == 1
                && get(&normalize_official_fill_tick(get(&d, "price"))?, "price") == get(&binding(s), "limit")
                && get(&economics, "allState") == &json!("observed_neutral_sell")
                && get(&d, "open").is_boolean()
                && (if js_int(get(&d, "quantityTraded")).unwrap() == 0 {
                    total == 0 && get(&d, "fillPrice").is_null()
                } else {
                    total >= limit && total <= 995_000 && total % 5_000 == 0
                        && price(get(&normalize_official_fill_tick(get(&d, "fillPrice"))?, "price").as_str().unwrap_or(""))? == total
                }),
        )
    })().unwrap_or(false)
}
fn js_key_eq(a: &V, b: &V) -> bool {
    baal_execution::js_key(a) == baal_execution::js_key(b)
}
fn binding(s: &V) -> V {
    let b = get(s, "buyProof");
    let o = get(s, "intent");
    json!({
        "ownerId": get(b, "ownerId"),
        "runId": get(b, "runId"),
        "tournamentId": get(b, "tournamentId"),
        "tournamentSlug": get(s, "tournamentSlug"),
        "marketId": get(b, "marketId"),
        "exchangeId": get(b, "exchangeId"),
        "intentId": get(o, "intentId"),
        "idempotencyKey": get(get(o, "body"), "idempotencyKey"),
        "orderId": get(o, "orderId"),
        "side": "no",
        "action": "sell",
        "quantity": 1,
        "limit": get(o, "body").get("price").map(|p| json!(
            baal_service_runtime::fixed::money(price(&baal_execution::js_key(p)).unwrap_or(0))
        )).unwrap_or(V::Null),
        "createdAt": get(o, "createdAt"),
        "expirationDate": get(get(o, "body"), "expirationDate"),
    })
}
fn claim(s: &V) -> Result<V, String> {
    let b = get(s, "buyProof");
    let intent = get(s, "intent");
    let body = json!({
        "schemaVersion": 1,
        "kind": "single_owned_no_exit_reservation",
        "ownerId": get(b, "ownerId"),
        "runId": get(b, "runId"),
        "tournamentId": get(b, "tournamentId"),
        "marketId": get(b, "marketId"),
        "exchangeId": get(b, "exchangeId"),
        "lotId": get(get(b, "lot"), "lotId"),
        "buyProofHash": get(b, "verificationHash"),
        "originalPrincipalCost": get(b, "scopedPrincipalCost"),
        "cashReserved": "0.000000",
        "sellIntentId": get(intent, "intentId"),
        "remainingQuantity": if get(get(intent, "proof"), "filledQuantity") == &json!(1) { 0 } else { 1 },
        "terminalSell": !get(intent, "proof").is_null(),
    });
    let mut out = body.clone();
    out.as_object_mut().unwrap().insert("reservationHash".into(), json!(payload_hash(&body)?));
    Ok(out)
}

fn reduce(s: Option<&V>, e: &V) -> Result<V, String> {
    if get(e, "kind") == &json!("neutral_sell_ack_revision") {
        let s = s.ok_or("OWNED_NO_CONTROLLER_NEUTRAL_SELL_REVISION_BINDING")?;
        let o = get(s, "intent");
        need(
            !o.is_null()
                && get(o, "phase") == &json!("semantic_mismatch")
                && get(o, "semanticMismatch") == &json!(true)
                && get(o, "proof").is_null()
                && !js_truthy(o.get("cancelPending"))
                && get(o, "orderId") == get(e, "orderId")
                && get(get(o, "ack"), "sourceBodyHash") == get(e, "sourceBodyHash")
                && instant_v(get(e, "at"))? >= instant_v(get(get(o, "ack"), "receivedAt"))?
                && canonical_neutral_ack(s, get(o, "ack")),
            "NEUTRAL_SELL_REVISION_BINDING",
        )?;
        let economics = crate::no_neutral_sell_all_v1::inspect_no_sell_ack_all_economics(get(o, "ack"))?;
        need(
            get(&economics, "allState") == &json!("observed_neutral_sell")
                && payload_hash(&economics).ok() == get(e, "economicsProofHash").as_str().map(String::from)
                && get(s, "reconciliationRevision").is_null(),
            "NEUTRAL_SELL_REVISION_PROOF",
        )?;
        let mut n = copy(s);
        let nm = n.as_object_mut().unwrap();
        nm.get_mut("intent").unwrap().as_object_mut().unwrap().insert("semanticMismatch".into(), json!(false));
        nm.get_mut("intent").unwrap().as_object_mut().unwrap().insert("phase".into(), json!("acknowledged"));
        nm.insert("reconciliationRevision".into(), json!({
            "kind": "neutral_sell_ack_v2",
            "revisionHash": get(e, "revisionHash"),
            "originalPrefixBytes": get(e, "originalPrefixBytes"),
            "originalPrefixHash": get(e, "originalPrefixHash"),
            "orderId": get(e, "orderId"),
            "sourceBodyHash": get(e, "sourceBodyHash"),
            "economicsProofHash": get(e, "economicsProofHash"),
            "at": get(e, "at"),
        }));
        return Ok(n);
    }
    let s = s.ok_or("OWNED_NO_CONTROLLER_EXPLICIT_REVISION_REQUIRED")?;
    need(!get(s, "reconciliationRevision").is_null(), "EXPLICIT_REVISION_REQUIRED")?;
    if get(e, "kind") == &json!("reservation") {
        // legacy reservation event
        let mut n = copy(s);
        need(
            get(get(e, "receipt"), "reservationHash") == get(&claim(s)?, "reservationHash")
                && hex(get(get(e, "receipt"), "globalLedgerHash"))
                && instant_v(get(get(e, "receipt"), "confirmedAt"))? <= instant_v(get(e, "at"))?,
            "RESERVATION",
        )?;
        n.as_object_mut().unwrap().insert("reservation".into(), get(e, "receipt").clone());
        return Ok(n);
    }
    let o = get(s, "intent").clone();
    need(
        get(e, "kind") == &json!("reconciled")
            && !get(&o, "orderId").is_null()
            && !get(&o, "ack").is_null()
            && !js_truthy(o.get("semanticMismatch"))
            && get(&o, "proof").is_null(),
        "RECONCILE_ONLY",
    )?;
    fields(get(e, "evidence"), &["order", "fillPages", "after", "globalRisk"])?;
    let mut verify_in = get(e, "evidence").clone();
    let vm = verify_in.as_object_mut().unwrap();
    vm.insert("binding".into(), binding(s));
    vm.insert("originalBuyProof".into(), get(s, "buyProof").clone());
    vm.insert("originalAfter".into(), get(s, "originalAfter").clone());
    vm.insert("before".into(), get(&o, "before").clone());
    vm.insert("ack".into(), get(&o, "ack").clone());
    vm.insert("readAt".into(), get(e, "at").clone());
    let p = verify_scoped_rest_no_sell_v2(&verify_in)?;
    need(
        get(&p, "originalBuyProofHash") == get(get(s, "buyProof"), "verificationHash")
            && get(&p, "originalLotId") == get(get(get(s, "buyProof"), "lot"), "lotId")
            && get(&p, "filledQuantity") == &json!(1)
            && amount_s(get(&p, "realizedPnl"))? >= 5_000
            && get(&p, "remainingOwnedLot").is_null(),
        "EXIT_PROOF_OR_MINIMUM",
    )?;
    let mut n = copy(s);
    let om = n.as_object_mut().unwrap().get_mut("intent").unwrap().as_object_mut().unwrap();
    om.insert("proof".into(), p);
    om.insert("phase".into(), json!("reconciled"));
    om.insert("cancelPending".into(), json!(false));
    Ok(n)
}
fn legacy_reduce(s: Option<&V>, e: &V) -> Result<V, String> {
    // Replays the original v1 journal prefix under v1 semantics.
    crate::owned_no_exit_controller_v1::legacy_reduce(s, e)
}

pub struct OwnedNoExitControllerV2 {
    dir: PathBuf,
    buy_proof: V,
    original_after: V,
    tournament_slug: String,
    revision: V,
    revision_hash: String,
    can_operate: Rc<dyn Fn() -> bool>,
    assert_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    install_exit_reservation: Rc<dyn Fn(&V) -> Result<V, String>>,
    now: Rc<dyn Fn() -> i64>,
    lease: RefCell<OwnerLease>,
    file: PathBuf,
    code: V,
    legacy_code: V,
    state: RefCell<V>,
    closed: RefCell<bool>,
    failed: RefCell<bool>,
    busy: RefCell<bool>,
    size: RefCell<usize>,
    bytes_hash: RefCell<String>,
    stat: RefCell<u64>,
    journal: RefCell<Journal>,
}

impl OwnedNoExitControllerV2 {
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        dir: &Path,
        buy_proof: &V,
        original_after: &V,
        tournament_slug: &str,
        can_operate: Rc<dyn Fn() -> bool>,
        assert_account_lease: Rc<dyn Fn() -> Result<V, String>>,
        install_exit_reservation: Rc<dyn Fn(&V) -> Result<V, String>>,
        neutral_sell_revision: &V,
        now: Rc<dyn Fn() -> i64>,
        execution_dir: &Path,
    ) -> Result<Self, String> {
        need(dir.is_absolute(), "DEPENDENCIES")?;
        let buy_proof = copy(buy_proof);
        let original_after = copy(original_after);
        v1::original(&buy_proof, &original_after, tournament_slug)?;
        let revision = copy(neutral_sell_revision);
        let required = ["kind", "ownerId", "runId", "tournamentId", "lotId", "buyProofHash", "orderId", "originalPrefixBytes", "originalPrefixHash", "ackSourceBodyHash"];
        fields(&revision, &required)?;
        need(
            get(&revision, "kind") == &json!("same_run_neutral_all_sell_ack_reconciliation")
                && get(&revision, "ownerId") == get(&buy_proof, "ownerId")
                && get(&revision, "runId") == get(&buy_proof, "runId")
                && get(&revision, "tournamentId") == get(&buy_proof, "tournamentId")
                && get(&revision, "lotId") == get(get(&buy_proof, "lot"), "lotId")
                && get(&revision, "buyProofHash") == get(&buy_proof, "verificationHash")
                && get(&revision, "orderId").as_str().is_some_and(|s| {
                    !s.is_empty() && s.as_bytes()[0] != b'0' && s.bytes().all(|b| b.is_ascii_digit())
                })
                && js_int(get(&revision, "originalPrefixBytes")).is_some_and(|x| x > 0)
                && ["originalPrefixHash", "ackSourceBodyHash"].iter().all(|k| hex(get(&revision, k))),
            "REVISION_DESCRIPTOR",
        )?;
        let revision_hash = payload_hash(&revision)?;
        let file = dir.join("owned-no-exit-journal.jsonl");
        need(file.exists(), "EXISTING_JOURNAL_REQUIRED")?;
        let code = owned_no_exit_controller_v2_identity(execution_dir);
        let legacy_code = owned_no_exit_controller_v1_legacy_identity(execution_dir);
        let lease = acquire_owner_lease(
            dir,
            get(&buy_proof, "ownerId").as_str().unwrap_or(""),
            get(&buy_proof, "runId").as_str().unwrap_or(""),
            &format!("single-owned-no-exit:{}", get(get(&buy_proof, "lot"), "lotId").as_str().unwrap_or("")),
            (now)() as f64,
        ).map_err(|e| format!("OWNED_NO_CONTROLLER_{}", e.code))?;
        let journal = Journal::open(&file)?;
        let this = Self {
            dir: dir.to_path_buf(),
            buy_proof,
            original_after,
            tournament_slug: tournament_slug.to_string(),
            revision,
            revision_hash,
            can_operate,
            assert_account_lease,
            install_exit_reservation,
            now,
            lease: RefCell::new(lease),
            file: file.clone(),
            code,
            legacy_code,
            state: RefCell::new(V::Null),
            closed: RefCell::new(false),
            failed: RefCell::new(false),
            busy: RefCell::new(false),
            size: RefCell::new(0),
            bytes_hash: RefCell::new(String::new()),
            stat: RefCell::new(0),
            journal: RefCell::new(journal),
        };
        match this.init() {
            Ok(()) => Ok(this),
            Err(e) => {
                this.lease.borrow_mut().release().ok();
                Err(e)
            }
        }
    }
    fn basic(&self) -> Result<V, String> {
        need(!*self.closed.borrow() && !*self.failed.borrow() && (self.can_operate)(), "STOPPED_OR_FAILED")?;
        self.lease.borrow().assert_ownership().map_err(|e| format!("OWNED_NO_CONTROLLER_{}", e.code))?;
        let l = (self.assert_account_lease)()?;
        need(
            get(&l, "ownerId") == get(&self.buy_proof, "ownerId")
                && get(&l, "tournamentId") == get(&self.buy_proof, "tournamentId")
                && get(&l, "leaseId").as_str().is_some_and(|s| !s.is_empty()),
            "ACCOUNT_LEASE",
        )?;
        Ok(l)
    }
    fn event_revision(&self, e: &V) -> Result<(), String> {
        need(
            get(e, "revisionHash").as_str() == Some(self.revision_hash.as_str())
                && get(e, "orderId") == get(&self.revision, "orderId")
                && get(e, "sourceBodyHash") == get(&self.revision, "ackSourceBodyHash")
                && get(e, "originalPrefixHash") == get(&self.revision, "originalPrefixHash")
                && get(e, "originalPrefixBytes") == get(&self.revision, "originalPrefixBytes"),
            "REVISION_EVENT_CHANGED",
        )
    }
    fn init(&self) -> Result<(), String> {
        self.basic()?;
        let bytes = std::fs::read(&self.file).map_err(|e| e.to_string())?;
        let prefix_bytes = js_int(get(&self.revision, "originalPrefixBytes")).unwrap_or(0) as usize;
        need(
            bytes.len() <= 64 * 1024 * 1024
                && bytes.last() == Some(&10)
                && prefix_bytes <= bytes.len()
                && bytes.get(prefix_bytes.wrapping_sub(1)) == Some(&10)
                && sha(&bytes[..prefix_bytes]) == get(&self.revision, "originalPrefixHash").as_str().unwrap_or(""),
            "REVISION_PREFIX_BINDING",
        )?;
        let mut seen: usize = 0;
        let mut state: Option<V> = None;
        for line in String::from_utf8_lossy(&bytes).split('\n').filter(|l| !l.is_empty()) {
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            let length = line.len() + 1;
            let legacy = seen < prefix_bytes;
            need(!legacy || seen + length <= prefix_bytes, "REVISION_PARTIAL_ROW")?;
            need(
                payload_hash(get(get(&row, "event"), "codeIdentity")).ok()
                    == payload_hash(if legacy { &self.legacy_code } else { &self.code }).ok(),
                "CODE_CHANGED",
            )?;
            if !legacy && get(get(&row, "event"), "input").get("kind") == Some(&json!("neutral_sell_ack_revision")) {
                self.event_revision(get(&row, "event").get("input").unwrap())?;
            }
            let input = get(&row, "event").get("input").cloned().unwrap_or(V::Null);
            state = Some(if legacy { legacy_reduce(state.as_ref(), &input)? } else { reduce(state.as_ref(), &input)? });
            need(
                payload_hash(state.as_ref().unwrap()).ok() == payload_hash(get(&row, "state")).ok(),
                "REPLAY",
            )?;
            seen += length;
        }
        need(
            seen == bytes.len()
                && payload_hash(get(state.as_ref().unwrap(), "buyProof")).ok() == payload_hash(&self.buy_proof).ok()
                && payload_hash(get(state.as_ref().unwrap(), "originalAfter")).ok() == payload_hash(&self.original_after).ok()
                && get(state.as_ref().unwrap(), "tournamentSlug").as_str() == Some(self.tournament_slug.as_str()),
            "ORIGINAL_BINDING",
        )?;
        *self.state.borrow_mut() = state.unwrap();
        *self.size.borrow_mut() = bytes.len();
        *self.bytes_hash.borrow_mut() = sha(&bytes);
        let meta = std::fs::metadata(&self.file).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        { use std::os::unix::fs::MetadataExt; *self.stat.borrow_mut() = meta.ino(); }
        if get(&self.state.borrow(), "reconciliationRevision").is_null() {
            let s = self.state.borrow().clone();
            let o = get(&s, "intent").clone();
            need(
                get(&o, "orderId") == get(&self.revision, "orderId")
                    && get(get(&o, "ack"), "sourceBodyHash") == get(&self.revision, "ackSourceBodyHash"),
                "REVISION_ORDER_BINDING",
            )?;
            let economics = crate::no_neutral_sell_all_v1::inspect_no_sell_ack_all_economics(get(&o, "ack"))?;
            self.append(&json!({
                "kind": "neutral_sell_ack_revision",
                "revisionHash": self.revision_hash,
                "orderId": get(&o, "orderId"),
                "sourceBodyHash": get(get(&o, "ack"), "sourceBodyHash"),
                "economicsProofHash": payload_hash(&economics)?,
                "originalPrefixBytes": get(&self.revision, "originalPrefixBytes"),
                "originalPrefixHash": get(&self.revision, "originalPrefixHash"),
                "at": iso((self.now)()),
            }))?;
        }
        Ok(())
    }
    fn append(&self, input: &V) -> Result<(), String> {
        self.basic()?;
        let before = std::fs::read(&self.file).map_err(|e| e.to_string())?;
        let meta = std::fs::metadata(&self.file).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        { use std::os::unix::fs::MetadataExt; need(meta.ino() == *self.stat.borrow(), "JOURNAL_CHANGED")?; }
        need(
            before.len() == *self.size.borrow()
                && sha(&before) == *self.bytes_hash.borrow()
                && payload_hash(&self.code).ok() == payload_hash(&self.code).ok()
                && payload_hash(&self.legacy_code).ok() == payload_hash(&self.legacy_code).ok(),
            "JOURNAL_CHANGED",
        )?;
        if get(input, "kind") == &json!("neutral_sell_ack_revision") {
            self.event_revision(input)?;
        }
        let next = reduce(Some(&self.state.borrow().clone()), input)?;
        need(
            *self.size.borrow() + js_stringify(&next).len() + js_stringify(input).len() < 64 * 1024 * 1024,
            "JOURNAL_BOUND",
        )?;
        let result = (|| -> Result<(), String> {
            self.journal.borrow_mut().commit(&next, &json!({"codeIdentity": self.code, "input": input}))?;
            self.basic()?;
            let after = std::fs::read(&self.file).map_err(|e| e.to_string())?;
            need(after[..*self.size.borrow()] == before[..], "JOURNAL_PREFIX_CHANGED")?;
            *self.state.borrow_mut() = next;
            *self.size.borrow_mut() = after.len();
            *self.bytes_hash.borrow_mut() = sha(&after);
            Ok(())
        })();
        if result.is_err() { *self.failed.borrow_mut() = true; }
        result
    }
    pub fn status(&self) -> V {
        let s = self.state.borrow().clone();
        let o = get(&s, "intent");
        let p = get(o, "proof");
        let journal = self.journal.borrow();
        json!({
            "mode": "single_owned_no_exit_controller_v2_reconciliation_only",
            "ownerId": get(&self.buy_proof, "ownerId"),
            "runId": get(&self.buy_proof, "runId"),
            "tournamentId": get(&self.buy_proof, "tournamentId"),
            "marketId": get(&self.buy_proof, "marketId"),
            "exchangeId": get(&self.buy_proof, "exchangeId"),
            "lotId": get(get(&self.buy_proof, "lot"), "lotId"),
            "buyProofHash": get(&self.buy_proof, "verificationHash"),
            "originalPrincipalCost": get(&self.buy_proof, "scopedPrincipalCost"),
            "sequence": journal.sequence,
            "hash": journal.previous,
            "phase": get(o, "phase"),
            "remainingOwnedLot": if !p.is_null() { get(p, "remainingOwnedLot").clone() } else { copy(get(&self.buy_proof, "lot")) },
            "realizedPnl": if !p.is_null() { get(p, "realizedPnl").clone() } else { json!("0.000000") },
            "scopedProceeds": if !p.is_null() { get(p, "scopedProceeds").clone() } else { json!("0.000000") },
            "intent": {
                "intentId": get(o, "intentId"),
                "orderId": get(o, "orderId"),
                "body": get(o, "body"),
                "bodyHash": get(o, "bodyHash"),
                "createdAt": get(o, "createdAt"),
                "semanticMismatch": get(o, "semanticMismatch"),
                "cancelPending": get(o, "cancelPending"),
                "actualFetchCount": get(o, "fetchStarts").as_array().map(|a| a.len()).unwrap_or(0),
                "lastActualFetchStartAt": get(o, "fetchStarts").as_array().and_then(|f| f.last()).map(|v| get(v, "requestStartedAt").clone()).unwrap_or(V::Null),
                "proofHash": get(p, "verificationHash"),
            },
            "reservation": claim(&s).unwrap_or(V::Null),
            "revision": get(&s, "reconciliationRevision"),
            "failed": *self.failed.borrow(),
            "closed": *self.closed.borrow(),
            "busy": *self.busy.borrow(),
            "writeCapability": false,
            "newSubmissionAuthorized": false,
            "additionalApiClients": 0,
        })
    }
    pub fn authorizes(&self, _spec: &V) -> bool { false }
    pub fn on_fetch_start(&self, _meta: &V) -> bool { false }
    pub fn exit_reservation(&self) -> Result<V, String> {
        claim(&self.state.borrow().clone())
    }
    pub fn exit_reservation_status(&self) -> Result<V, String> {
        let s = self.state.borrow().clone();
        Ok(json!({"record": claim(&s)?, "receipt": get(&s, "reservation")}))
    }
    pub fn verified_exit_proof(&self) -> V {
        get(get(&self.state.borrow(), "intent"), "proof").clone()
    }
    pub fn prepare(&self) -> Result<V, String> { Err("OWNED_NO_CONTROLLER_RECONCILIATION_ONLY".into()) }
    pub fn dispatch_prepared(&self) -> Result<V, String> { Err("OWNED_NO_CONTROLLER_RECONCILIATION_ONLY".into()) }
    pub fn cancel_expired(&self) -> Result<V, String> { Err("OWNED_NO_CONTROLLER_RECONCILIATION_ONLY".into()) }
    pub fn refresh_reservation(&self) -> Result<V, String> {
        need(!*self.busy.borrow(), "BUSY")?;
        *self.busy.borrow_mut() = true;
        let result = (|| {
            self.basic()?;
            let s = self.state.borrow().clone();
            need(
                !get(get(&s, "intent"), "proof").is_null() && get(get(&s, "intent"), "phase") == &json!("reconciled"),
                "VERIFIED_EXIT_REQUIRED",
            )?;
            drop(s);
            let receipt = (self.install_exit_reservation)(&claim(&self.state.borrow().clone())?)?;
            self.append(&json!({"kind": "reservation", "receipt": receipt, "at": iso((self.now)())}))?;
            Ok(self.status())
        })();
        *self.busy.borrow_mut() = false;
        result
    }
    pub fn reconcile(&self, evidence: &V) -> Result<V, String> {
        need(!*self.busy.borrow(), "BUSY")?;
        *self.busy.borrow_mut() = true;
        let result = (|| {
            self.basic()?;
            self.append(&json!({"kind": "reconciled", "evidence": evidence, "at": iso((self.now)())}))?;
            let receipt = (self.install_exit_reservation)(&claim(&self.state.borrow().clone())?)?;
            self.append(&json!({"kind": "reservation", "receipt": receipt, "at": iso((self.now)())}))?;
            Ok(self.status())
        })();
        *self.busy.borrow_mut() = false;
        result
    }
    pub fn close(&self) -> Result<(), String> {
        need(!*self.busy.borrow(), "BUSY")?;
        if *self.closed.borrow() { return Ok(()); }
        *self.closed.borrow_mut() = true;
        self.lease.borrow_mut().release().map_err(|e| e.message)
    }
}
