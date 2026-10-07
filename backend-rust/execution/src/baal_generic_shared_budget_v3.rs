// execution/baal-generic-shared-budget-v3.mjs — source-replayed event-only
// generic controller budget; inherited inventory must match the complete
// validated custody seed. No extra client, collateral discount or activation.
// Durable local allocation slot.
//
// Port notes:
// - `need` throws BAAL_SHARED_BUDGET_<code>.
// - `sync(fn)` -> direct call of the Rust callback (truthy + copy).
// - `stamp` -> (ino, birthtimeMs) pair compared as a tuple.
// - replayBaalGenericNoJournalV3 / baalGenericNoControllerV3Identity /
//   projectBaalGenericReservationV3 live in baal_generic_no_controller_v3.
// - append-failure flags ride as `:fatal:durableRecordingFailed` suffixes.
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde_json::{json, Map, Value as V};

use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::{amount, instant, money, price};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use baal_service_runtime::ledger::OwnerLease;

use crate::baal_generic_custody_seed_v1::validate_baal_generic_custody_seed_v1;
use crate::baal_generic_no_controller_v3::{
    baal_generic_no_controller_v3_identity, project_baal_generic_reservation_v3,
    replay_baal_generic_no_journal_v3,
};
use crate::core::{canonical_payload, payload_hash};
use crate::ledger::acquire_owner_lease;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("BAAL_SHARED_BUDGET_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    static NULL: V = V::Null;
    v.get(k).unwrap_or(&NULL)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn sha_str(s: &str) -> String {
    sha256(s.as_bytes())
}
fn sha_bytes(b: &[u8]) -> String {
    sha256(b)
}
fn iso(n: f64) -> String {
    ms_to_iso(n)
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(payload_hash(a)? == payload_hash(b)?)
}
fn truthy_sync(v: V) -> Result<V, String> {
    need(js_truthy(Some(&v)), "SYNC_CALLBACK")?;
    Ok(v)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or("invalid"))
}
fn num_str(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        other => match other.as_f64() {
            Some(f) => f.to_string(),
            None => "NaN".to_string(),
        },
    }
}

fn file_stamp(m: &fs::Metadata) -> String {
    use std::os::unix::fs::MetadataExt;
    let birthtime = m
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0);
    format!("{}:{}", m.ino(), birthtime)
}

/// proof(p): verificationHash-bound owned proof with scopedTradeVerified.
fn check_proof(p: &V) -> Result<(), String> {
    let verification_hash = sget(p, "verificationHash").unwrap_or("").to_string();
    let mut body = p.as_object().cloned().unwrap_or_default();
    body.remove("verificationHash");
    need(
        hex(&V::String(verification_hash.clone()))
            && payload_hash(&V::Object(body))?.as_str() == verification_hash.as_str()
            && get(p, "scopedTradeVerified") == &V::Bool(true),
        "OWN_PROOF",
    )?;
    Ok(())
}

/// projection(s,scope,allocationHash,custodySeed)
fn projection(s: &V, scope: &V, allocation_hash: &str, custody_seed: &V) -> Result<V, String> {
    need(
        sget(s, "mode") == Some("baal_generic_no_controller_v3")
            && same(get(s, "scope"), scope)?
            && get(s, "allocation")
                .as_object()
                .map(|_| payload_hash(get(s, "allocation")).map(|h| h == allocation_hash))
                .unwrap_or(Ok(false))?
            && get(s, "orders").is_array(),
        "ACTUAL_SCOPE",
    )?;
    let mut intents = HashSet::new();
    let mut keys = HashSet::new();
    let mut spent: i128 = 0;
    for o in get(s, "orders").as_array().cloned().unwrap_or_default() {
        need(
            sget(&o, "intentId").is_some()
                && !intents.contains(sget(&o, "intentId").unwrap_or(""))
                && sget(get(&o, "body"), "side") == Some("no")
                && ["buy", "sell"].contains(&sget(get(&o, "body"), "action").unwrap_or(""))
                && js_int(get(get(&o, "body"), "quantity")).is_some_and(|q| q >= 1 && q <= 100)
                && sget(get(&o, "body"), "tournamentId") == sget(scope, "tournamentId")
                && get(scope, "exchanges")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .any(|m| {
                        get(m, "exchangeId") == get(get(&o, "body"), "exchangeId")
                            && get(m, "marketId") == get(&o, "marketId")
                    })
                && sget(&o, "bodyJson")
                    == Some(
                        canonical_payload(get(&o, "body"))?.as_str(),
                    )
                && sget(&o, "bodyHash")
                    == Some(payload_hash(get(&o, "body"))?.as_str())
                && !keys.contains(sget(get(&o, "body"), "idempotencyKey").unwrap_or(""))
                && price(&num_str(get(get(&o, "body"), "price")))?
                    * js_int(get(get(&o, "body"), "quantity")).unwrap_or(0) as i128
                    <= amount(
                        match get(s, "allocation").get("maximumOrderCost") {
                            Some(x) if !x.is_null() => x.as_str().unwrap_or(""),
                            _ => "25.000000",
                        },
                    )?,
            "ORDER_OR_DUPLICATE",
        )?;
        intents.insert(sget(&o, "intentId").unwrap_or("").to_string());
        keys.insert(sget(get(&o, "body"), "idempotencyKey").unwrap_or("").to_string());
        if sget(&o, "phase") == Some("abandoned") {
            need(
                get(&o, "proof").is_null()
                    && get(&o, "orderId").is_null()
                    && get(&o, "ack").is_null()
                    && o.get("operation").is_none()
                    && get(&o, "cancelPending") == &V::Bool(false)
                    && get(&o, "fences").as_array().is_some_and(|x| x.is_empty())
                    && get(&o, "fetchStarts").as_array().is_some_and(|x| x.is_empty())
                    && get(&o, "receipts").as_array().is_some_and(|x| x.is_empty())
                    && instant_v(get(&o, "abandonedAt"))?
                        >= instant_v(get(get(&o, "body"), "expirationDate"))?
                    && sget(&o, "abandonedReason") == Some("expired_before_any_admission"),
                "ABANDONED_NOT_PROVEN_UNFETCHED",
            )?;
        }
        if !get(&o, "proof").is_null() {
            check_proof(get(&o, "proof"))?;
            need(
                sget(&o, "phase") == Some("reconciled")
                    && sget(get(&o, "proof"), "ownerId") == sget(scope, "ownerId")
                    && sget(get(&o, "proof"), "runId") == sget(scope, "runId")
                    && sget(get(&o, "proof"), "tournamentId") == sget(scope, "tournamentId")
                    && sget(get(&o, "proof"), "intentId") == sget(&o, "intentId")
                    && sget(get(&o, "proof"), "orderId") == sget(&o, "orderId")
                    && sget(get(&o, "proof"), "marketId") == sget(&o, "marketId")
                    && sget(get(&o, "proof"), "exchangeId") == sget(get(&o, "body"), "exchangeId")
                    && sget(get(&o, "proof"), "action") == sget(get(&o, "body"), "action"),
                "ORDER_PROOF_BINDING",
            )?;
            spent += amount(sget(get(&o, "proof"), "scopedPrincipalCost").unwrap_or(""))?;
        }
    }
    if let Some(inventory) = get(s, "inventory").as_object() {
        for (ex, slot) in inventory {
            check_proof(get(slot, "proof"))?;
            need(
                sget(get(slot, "proof"), "exchangeId") == Some(ex.as_str())
                    && (get(s, "orders")
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .any(|o| !get(o, "proof").is_null() && {
                            same(get(o, "proof"), get(slot, "proof")).unwrap_or(false)
                        })
                        || same(
                            get(get(custody_seed, "inventory"), ex.as_str()),
                            slot,
                        )?),
                "NO_OTHER_ACTOR_ADOPTION",
            )?;
        }
    }
    let record = project_baal_generic_reservation_v3(s)?;
    need(
        sget(&record, "allocationHash") == Some(allocation_hash)
            && get(&record, "owned")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .all(|r| amount(sget(r, "nominalCost").unwrap_or("")).unwrap_or(-1) >= 0)
            && get(&record, "reservations")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .all(|r| amount(sget(r, "maximumNominalCost").unwrap_or("")).unwrap_or(-1) >= 0),
        "NONNEGATIVE_FULL_RESERVE",
    )?;
    let pending: Vec<V> = get(s, "orders")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|o| get(o, "proof").is_null() && sget(o, "phase") != Some("abandoned"))
        .collect();
    need(pending.len() <= 1, "SHADOW_PENDING")?;
    let unknown = pending.iter().any(|o| {
        ["uncertain", "semantic_mismatch"].contains(&sget(o, "phase").unwrap_or(""))
            || get(o, "semanticMismatch") == &V::Bool(true)
            || (get(o, "orderId").is_null()
                && !get(o, "fetchStarts")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .is_empty())
    });
    let orders_out: Vec<V> = get(s, "orders")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|o| {
            json!({
                "intentId": get(o, "intentId"),
                "bodyHash": get(o, "bodyHash"),
                "bodyJson": get(o, "bodyJson"),
                "orderId": get(o, "orderId"),
                "phase": get(o, "phase"),
                "proofHash": get(o, "proof").get("verificationHash").cloned().unwrap_or(V::Null),
                "fetchStarts": get(o, "fetchStarts"),
            })
        })
        .collect();
    Ok(json!({
        "record": record,
        "orders": orders_out,
        "spent": money(spent),
        "unknown": unknown,
        "inheritanceHash": get(s, "inheritanceHash"),
        "currentInventoryHash": payload_hash(get(s, "inventory"))?,
    }))
}

/// forward(old,next)
fn forward(old: &V, next: &V) -> Result<(), String> {
    if old.is_null() {
        return Ok(());
    }
    need(
        amount(sget(next, "spent").unwrap_or(""))? >= amount(sget(old, "spent").unwrap_or(""))?
            && get(next, "orders").as_array().map(|x| x.len()).unwrap_or(0)
                >= get(old, "orders").as_array().map(|x| x.len()).unwrap_or(0),
        "EXPENSE_OR_ORDER_RESET",
    )?;
    for o in get(old, "orders").as_array().cloned().unwrap_or_default() {
        let n = get(next, "orders")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .find(|n| get(n, "intentId") == get(&o, "intentId"))
            .unwrap_or(V::Null);
        need(
            !n.is_null()
                && get(&n, "bodyHash") == get(&o, "bodyHash")
                && get(&n, "bodyJson") == get(&o, "bodyJson")
                && (get(&o, "orderId").is_null() || get(&n, "orderId") == get(&o, "orderId"))
                && (get(&o, "proofHash").is_null() || get(&n, "proofHash") == get(&o, "proofHash"))
                && (sget(&o, "phase") != Some("abandoned") || sget(&n, "phase") == Some("abandoned"))
                && get(&o, "fetchStarts")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .all(|f| {
                        get(&n, "fetchStarts")
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                            .iter()
                            .any(|g| same(f, g).unwrap_or(false))
                    }),
            "ORIGINAL_INTENT_FILL_RESET",
        )?;
    }
    Ok(())
}

/// totalRisk(observed,record,allocation,{allowFundingShortfall})
fn total_risk(observed: &V, record: &V, allocation: &V, allow_funding_shortfall: bool) -> Result<V, String> {
    let mut out: BTreeMap<String, Vec<V>> = BTreeMap::new();
    out.insert("owned".into(), get(record, "owned").as_array().cloned().unwrap_or_default());
    out.insert("reservations".into(), get(record, "reservations").as_array().cloned().unwrap_or_default());
    out.insert("loans".into(), Vec::new());
    let mut ids: HashSet<String> = HashSet::new();
    for k in ["owned", "reservations"] {
        for r in &out[k] {
            ids.insert(sget(r, "exposureId").unwrap_or("").to_string());
        }
    }
    need(
        ids.len() == out["owned"].len() + out["reservations"].len(),
        "DUPLICATE_OWN_EXPOSURE",
    )?;
    for (k, f) in [
        ("owned", "nominalCost"),
        ("reservations", "maximumNominalCost"),
        ("loans", "fullNominalPrincipal"),
    ] {
        need(get(observed, k).is_array(), "OBSERVED_EXPOSURES")?;
        for r in get(observed, k).as_array().cloned().unwrap_or_default() {
            need(
                sget(&r, "exposureId").is_some_and(|s| !s.is_empty())
                    && sget(&r, "marketId").is_some()
                    && sget(&r, "eventId").is_some_and(|s| !s.is_empty())
                    && amount(sget(&r, f).unwrap_or("")).unwrap_or(-1) >= 0,
                "OBSERVED_EXPOSURE",
            )?;
            let eid = sget(&r, "exposureId").unwrap_or("").to_string();
            if ids.contains(&eid) {
                need(
                    out[k].iter().any(|v| same(v, &r).unwrap_or(false)),
                    "SHADOW_EXPOSURE",
                )?;
                continue;
            }
            need(
                !eid.starts_with(&format!("scaled:{}:", sget(record, "runId").unwrap_or(""))),
                "UNMATCHED_OWN_SHADOW",
            )?;
            ids.insert(eid);
            out.get_mut(k).unwrap().push(r);
        }
    }
    let mut nominal: i128 = 0;
    let mut reserved: i128 = 0;
    let mut markets: Vec<(String, i128)> = Vec::new();
    let mut events: Vec<(String, i128)> = Vec::new();
    let mut market_events: Vec<(String, String)> = Vec::new();
    for (k, f) in [
        ("owned", "nominalCost"),
        ("reservations", "maximumNominalCost"),
        ("loans", "fullNominalPrincipal"),
    ] {
        for r in out[k].clone() {
            let n = amount(sget(&r, f).unwrap_or(""))?;
            nominal += n;
            if k == "reservations" {
                reserved += n;
            }
            let mid = sget(&r, "marketId").unwrap_or("").to_string();
            let eid = sget(&r, "eventId").unwrap_or("").to_string();
            need(
                !market_events.iter().any(|(m, _)| m == &mid)
                    || market_events.iter().find(|(m, _)| m == &mid).unwrap().1 == eid,
                "EVENT_MAPPING_CONFLICT",
            )?;
            if !market_events.iter().any(|(m, _)| m == &mid) {
                market_events.push((mid.clone(), eid.clone()));
            }
            if let Some(slot) = markets.iter_mut().find(|(m, _)| m == &mid) {
                slot.1 += n;
            } else {
                markets.push((mid.clone(), n));
            }
            if let Some(slot) = events.iter_mut().find(|(e, _)| e == &eid) {
                slot.1 += n;
            } else {
                events.push((eid.clone(), n));
            }
        }
    }
    let max_markets = match allocation.get("maximumActiveMarkets") {
        Some(x) if !x.is_null() => js_int(x).unwrap_or(0),
        _ => 20,
    };
    let opt_amount = |k: &str, d: &str| -> Result<i128, String> {
        let v = match allocation.get(k) {
            Some(x) if !x.is_null() => x.clone(),
            _ => json!(d),
        };
        amount(v.as_str().ok_or_else(|| "INVALID".to_string())?)
    };
    need(
        markets.iter().filter(|(_, v)| *v > 0).count() <= max_markets as usize,
        "ACTIVE_MARKET_CEILING",
    )?;
    need(
        nominal <= 1000000000
            && markets.iter().all(|(_, v)| *v <= opt_amount("maximumMarketCost", "100.000000").unwrap_or(i128::MAX))
            && events.iter().all(|(_, v)| *v <= opt_amount("maximumEventCost", "200.000000").unwrap_or(i128::MAX)),
        "NOMINAL_LIMIT",
    )?;
    need(
        amount(sget(observed, "cashLower").unwrap_or(""))? >= 0
            && (allow_funding_shortfall
                || reserved <= amount(sget(observed, "cashLower").unwrap_or(""))?),
        "FULL_CASH_RESERVATION",
    )?;
    let mut ret = Map::new();
    ret.insert("owned".into(), json!(out["owned"]));
    ret.insert("reservations".into(), json!(out["reservations"]));
    ret.insert("loans".into(), json!(out["loans"]));
    ret.insert(
        "fundingSatisfied".into(),
        json!(reserved <= amount(sget(observed, "cashLower").unwrap_or(""))?),
    );
    ret.insert("nominalExposure".into(), json!(money(nominal)));
    ret.insert("reservedCash".into(), json!(money(reserved)));
    ret.insert(
        "markets".into(),
        V::Object(
            markets
                .iter()
                .map(|(k, v)| (k.clone(), json!(money(*v))))
                .collect::<Map<String, V>>(),
        ),
    );
    ret.insert(
        "events".into(),
        V::Object(
            events
                .iter()
                .map(|(k, v)| (k.clone(), json!(money(*v))))
                .collect::<Map<String, V>>(),
        ),
    );
    Ok(V::Object(ret))
}

pub struct BaalGenericSharedBudgetV3Deps {
    pub assert_account_lease: Rc<dyn Fn() -> V>,
    pub assert_financial_owner_lease: Rc<dyn Fn() -> V>,
    pub get_controller_source: Rc<dyn Fn() -> V>,
    pub observe_global_risk: Rc<dyn Fn() -> V>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct BudgetInner {
    dir: PathBuf,
    controller_dir: PathBuf,
    source_runtime_dir: PathBuf,
    scope: V,
    allocation: V,
    allocation_hash: String,
    budget_installed_hash: String,
    controller_code_identity_hash: String,
    custody_seed: V,
    lease: OwnerLease,
    file: PathBuf,
    actor_file: PathBuf,
    journal: Journal,
    state: Option<V>,
    closed: bool,
    failed: bool,
    size: u64,
    bytes_hash: String,
    identity_stamp: String,
    cache: Option<V>,
    cache_hits: i64,
    cache_misses: i64,
    deps: BaalGenericSharedBudgetV3Deps,
}

fn basic_check(
    deps: &BaalGenericSharedBudgetV3Deps,
    lease: &OwnerLease,
    scope: &V,
    source_runtime_dir: &Path,
    closed: bool,
    failed: bool,
) -> Result<String, String> {
    need(!closed && !failed && (deps.can_operate)(), "STOPPED")?;
    lease.assert_ownership().map_err(|e| e.message)?;
    let l = truthy_sync((deps.assert_account_lease)())?;
    let f = truthy_sync((deps.assert_financial_owner_lease)())?;
    let lock_path = source_runtime_dir
        .join("baal-financial-owner")
        .join("execution-owner.lock");
    let actual: V = serde_json::from_str(
        &fs::read_to_string(&lock_path).map_err(|e| e.to_string())?,
    )
    .map_err(|_| "BAAL_SHARED_BUDGET_ACTUAL_FINANCIAL_OWNER".to_string())?;
    need(
        get(&actual, "ownerId") == get(scope, "ownerId")
            && get(&actual, "runId") == get(scope, "runId")
            && js_int(get(&actual, "pid")) == Some(std::process::id() as i64)
            && sget(&actual, "scope") == Some("baal-global-financial-owner")
            && sget(&f, "leaseId") == Some(payload_hash(&actual)?.as_str())
            && get(&f, "ownerId") == get(scope, "ownerId")
            && get(&f, "runId") == get(scope, "runId")
            && get(&f, "tournamentId") == get(scope, "tournamentId"),
        "ACTUAL_FINANCIAL_OWNER",
    )?;
    need(
        get(&l, "ownerId") == get(scope, "ownerId")
            && get(&l, "tournamentId") == get(scope, "tournamentId")
            && sget(&l, "leaseId").is_some_and(|s| !s.is_empty()),
        "ACTUAL_ACCOUNT_LEASE",
    )?;
    payload_hash(&l)
}

impl BudgetInner {
    fn basic(&self) -> Result<String, String> {
        basic_check(
            &self.deps,
            &self.lease,
            &self.scope,
            &self.source_runtime_dir,
            self.closed,
            self.failed,
        )
    }

    fn observed(&self) -> Result<V, String> {
        let g = truthy_sync((self.deps.observe_global_risk)())?;
        let now = (self.deps.now)() as i64;
        need(
            get(&g, "ownerId") == get(&self.scope, "ownerId")
                && get(&g, "runId") == get(&self.scope, "runId")
                && get(&g, "tournamentId") == get(&self.scope, "tournamentId")
                && get(&g, "complete") == &V::Bool(true)
                && get(&g, "eventScopeComplete") == &V::Bool(true)
                && get(&g, "soleCollector") == &V::Bool(true)
                && get(&g, "stopped") == &V::Bool(false)
                && hex(get(&g, "sourceHash"))
                && instant_v(get(&g, "at"))? <= now
                && now - instant_v(get(&g, "at"))? <= 20000
                && get(&g, "unresolvedIntentIds").is_array()
                && get(&g, "unknownAcceptance").is_boolean(),
            "FRESH_ACTUAL_GLOBAL_OBSERVATION",
        )?;
        Ok(g)
    }
}

/// source(): replay the controller journal and project it.
fn source(inner: &Rc<RefCell<BudgetInner>>) -> Result<V, String> {
    inner.borrow().basic()?;
    need(
        payload_hash(&baal_generic_no_controller_v3_identity()?)?
            == inner.borrow().controller_code_identity_hash,
        "ACTUAL_CONTROLLER_CODE_CHANGED",
    )?;
    let hint = truthy_sync((inner.borrow().deps.get_controller_source)())?;
    let actor_file = inner.borrow().actor_file.clone();
    let hint_file = if !get(&hint, "journalFile").is_null() {
        PathBuf::from(sget(&hint, "journalFile").unwrap_or(""))
    } else {
        actor_file.clone()
    };
    need(
        hint_file.canonicalize().unwrap_or(hint_file.clone()) == actor_file.canonicalize().unwrap_or(actor_file.clone()),
        "FIXED_ACTOR_PATH",
    )?;
    let before = fs::metadata(&actor_file).map_err(|e| e.to_string())?;
    let before_stamp = file_stamp(&before);
    let bytes = fs::read(&actor_file).map_err(|e| e.to_string())?;
    need(
        !bytes.is_empty() && bytes.len() <= 128 * 1024 * 1024 && *bytes.last().unwrap() == 10,
        "ACTOR_PARTIAL_OR_SIZE",
    )?;
    if let Some(aa) = inner.borrow().state.as_ref().map(|s| get(s, "actorAnchor").clone()) {
        if !aa.is_null() {
            let anchor_bytes = js_int(get(&aa, "bytes")).unwrap_or(0) as usize;
            need(
                bytes.len() >= anchor_bytes
                    && sha_bytes(&bytes[..anchor_bytes]) == sget(&aa, "sha256").unwrap_or(""),
                "ACTOR_PREFIX_RESET",
            )?;
        }
    }
    let current_hash = sha_bytes(&bytes);
    let key = json!({
        "currentHash": current_hash,
        "size": bytes.len(),
        "identity": before_stamp,
        "controllerCodeIdentityHash": inner.borrow().controller_code_identity_hash,
        "seedHash": get(&inner.borrow().custody_seed, "seedHash"),
    });
    let mut result: V;
    let mut p: V;
    let cached = {
        let g = inner.borrow();
        g.cache
            .as_ref()
            .filter(|c| same(get(c, "key"), &key).unwrap_or(false))
            .map(|c| get(c, "value").clone())
    };
    if let Some(cached_value) = cached {
        inner.borrow_mut().cache_hits += 1;
        result = get(&cached_value, "result").clone();
        p = get(&cached_value, "p").clone();
    } else {
        inner.borrow_mut().cache_misses += 1;
        let body_text = String::from_utf8_lossy(&bytes).to_string();
        let replay = replay_baal_generic_no_journal_v3(&json!({
            "bodyText": body_text,
            "expectedCodeIdentity": baal_generic_no_controller_v3_identity()?,
            "custodySeed": inner.borrow().custody_seed,
        }))?;
        let replay_state = get(&replay, "state").clone();
        need(
            get(get(&replay_state, "activation"), "budgetInstalledHash")
                .as_str()
                == Some(inner.borrow().budget_installed_hash.as_str()),
            "ACTOR_BUDGET_BINDING",
        )?;
        let (scope, allocation_hash, custody_seed) = {
            let g = inner.borrow();
            (g.scope.clone(), g.allocation_hash.clone(), g.custody_seed.clone())
        };
        p = projection(&replay_state, &scope, &allocation_hash, &custody_seed)?;
        need(
            get(&p, "inheritanceHash") == get(&custody_seed, "seedHash"),
            "EXACT_INHERITED_ROOT",
        )?;
        let nl = bytes.iter().position(|b| *b == 10).unwrap_or(0);
        let line = &bytes[..nl + 1];
        let row: V = serde_json::from_str(&String::from_utf8_lossy(line))
            .map_err(|_| "BAAL_SHARED_BUDGET_REPLAY".to_string())?;
        result = json!({
            "anchor": get(&replay, "anchor"),
            "opening": {
                "bytes": line.len(),
                "sha256": sha_bytes(line),
                "sequence": 1,
                "hash": get(&row, "hash"),
            },
        });
        inner.borrow_mut().cache = Some(json!({
            "key": key,
            "value": {"result": result, "p": p},
        }));
        result = get(get(&inner.borrow().cache.clone().unwrap_or(V::Null), "value"), "result").clone();
        p = get(get(&inner.borrow().cache.clone().unwrap_or(V::Null), "value"), "p").clone();
    }
    let opening = get(&result, "opening").clone();
    let seq = get(get(&result, "anchor"), "sequence").clone();
    let previous = get(get(&result, "anchor"), "hash").clone();
    let after = fs::metadata(&actor_file).map_err(|e| e.to_string())?;
    need(
        file_stamp(&after) == before_stamp && after.len() == bytes.len() as u64,
        "ACTOR_CHANGED",
    )?;
    {
        let g = inner.borrow();
        if let Some(cra) = g.state.as_ref().map(|s| get(s, "controllerRootAnchor").clone()) {
            if !cra.is_null() {
                need(same(&opening, &cra)?, "ORIGINAL_ROOT_PREFIX_CHANGED")?;
            }
        }
    }
    if !get(&hint, "rootPrefix").is_null() {
        need(same(get(&hint, "rootPrefix"), &opening)?, "CALLER_ROOT_PREFIX")?;
    }
    need(
        get(&hint, "record").is_null() || same(get(&hint, "record"), get(&p, "record"))?,
        "CALLER_SHADOW_RECORD",
    )?;
    need(
        get(&hint, "sequence") == &seq && get(&hint, "hash") == &previous,
        "CALLER_ACTOR_FRONTIER",
    )?;
    {
        let g = inner.borrow();
        if let Some(ap) = g.state.as_ref().map(|s| get(s, "actorProjection").clone()) {
            forward(&ap, &p)?;
        }
    }
    Ok(json!({
        "record": get(&p, "record"),
        "orders": get(&p, "orders"),
        "spent": get(&p, "spent"),
        "unknown": get(&p, "unknown"),
        "inheritanceHash": get(&p, "inheritanceHash"),
        "currentInventoryHash": get(&p, "currentInventoryHash"),
        "anchor": {
            "bytes": bytes.len(),
            "sha256": sha_bytes(&bytes),
            "sequence": seq,
            "hash": previous,
        },
        "opening": opening,
    }))
}

fn append(inner: &Rc<RefCell<BudgetInner>>, next: &V) -> Result<(), String> {
    inner.borrow().basic()?;
    let (before, s, size, bytes_hash, identity_stamp, file) = {
        let g = inner.borrow();
        let before = fs::read(&g.file).map_err(|e| e.to_string())?;
        let s = fs::metadata(&g.file).map_err(|e| e.to_string())?;
        (
            before,
            s,
            g.size,
            g.bytes_hash.clone(),
            g.identity_stamp.clone(),
            g.file.clone(),
        )
    };
    need(
        file_stamp(&s) == identity_stamp
            && before.len() as u64 == size
            && sha_bytes(&before) == bytes_hash,
        "BUDGET_RESET",
    )?;
    let block = (|| -> Result<(), String> {
        let (allocation_hash, budget_installed_hash) = {
            let g = inner.borrow();
            (g.allocation_hash.clone(), g.budget_installed_hash.clone())
        };
        inner.borrow_mut().journal.commit(
            next,
            &json!({
                "kind": "source_bound_nominal_budget",
                "allocationHash": allocation_hash,
                "budgetInstalledHash": budget_installed_hash,
            }),
        )?;
        inner.borrow().basic()?;
        let after_bytes = fs::read(&file).map_err(|e| e.to_string())?;
        need(
            after_bytes[..size as usize] == before[..] && after_bytes.len() as u64 > size,
            "BUDGET_PREFIX_CHANGED",
        )?;
        let mut g = inner.borrow_mut();
        g.state = Some(next.clone());
        g.size = after_bytes.len() as u64;
        g.bytes_hash = sha_bytes(&after_bytes);
        Ok(())
    })();
    match block {
        Ok(()) => Ok(()),
        Err(e) => {
            inner.borrow_mut().failed = true;
            if e.contains(":fatal") || e.contains("durableRecordingFailed") {
                Err(e)
            } else {
                Err(format!("{e}:fatal:durableRecordingFailed"))
            }
        }
    }
}

pub struct BaalGenericSharedBudgetV3 {
    inner: Rc<RefCell<BudgetInner>>,
}

/// openBaalGenericSharedBudgetV3({dir,controllerDir,sourceRuntimeDir,custodySeed,
///   allocation,scope,budgetInstalledHash,controllerCodeIdentityHash,...})
pub fn open_baal_generic_shared_budget_v3(
    dir: &Path,
    controller_dir: &Path,
    source_runtime_dir: &Path,
    custody_seed: &V,
    allocation: &V,
    scope: &V,
    budget_installed_hash: &str,
    controller_code_identity_hash: &str,
    deps: BaalGenericSharedBudgetV3Deps,
) -> Result<BaalGenericSharedBudgetV3, String> {
    need(
        dir.is_absolute() && controller_dir.is_absolute() && dir != controller_dir,
        "DEPENDENCIES",
    )?;
    need(
        hex(&json!(budget_installed_hash)) && hex(&json!(controller_code_identity_hash)),
        "DEPENDENCIES",
    )?;
    need(source_runtime_dir.is_absolute(), "SOURCE_RUNTIME")?;
    validate_baal_generic_custody_seed_v1(custody_seed, scope, allocation)?;
    let allocation = allocation.clone();
    let scope = scope.clone();
    let allocation_hash = payload_hash(&allocation)?;
    need(
        js_int(get(&allocation, "schemaVersion")) == Some(1)
            && sget(&allocation, "ownerId") == Some("Codex")
            && get(&allocation, "runId") == get(&scope, "runId")
            && get(&allocation, "tournamentId") == get(&scope, "tournamentId")
            && sget(&allocation, "capital") == Some("1000.000000")
            && hex(get(&allocation, "humanAllocationSourceHash"))
            && sget(&scope, "ownerId") == Some("Codex")
            && get(&scope, "exchanges").is_array(),
        "ALLOCATION",
    )?;
    for (k, cap) in [
        ("maximumOrderCost", 25000000i128),
        ("maximumMarketCost", 100000000i128),
        ("maximumEventCost", 200000000i128),
    ] {
        let v = match allocation.get(k) {
            Some(x) if !x.is_null() => x.clone(),
            _ => json!(money(cap)),
        };
        let v = amount(v.as_str().ok_or_else(|| "INVALID".to_string())?)?;
        need(v > 0 && v <= cap, "ALLOCATION_CEILING")?;
    }
    let opt_int = |k: &str, d: i64| -> Option<i64> {
        match allocation.get(k) {
            Some(x) if !x.is_null() => js_int(x),
            _ => Some(d),
        }
    };
    need(
        opt_int("maximumActiveMarkets", 20).is_some_and(|v| v >= 1 && v <= 20),
        "ACTIVE_MARKET_CEILING",
    )?;
    need(
        opt_int("maximumSharesPerOrder", 100).is_some_and(|v| v >= 1 && v <= 100)
            && instant_v(get(&allocation, "recordedAt"))? <= (deps.now)() as i64,
        "ALLOCATION_CLOCK_OR_SHARES",
    )?;
    let mut lease = match acquire_owner_lease(
        dir,
        sget(&scope, "ownerId").unwrap_or(""),
        sget(&scope, "runId").unwrap_or(""),
        "baal-generic-shared-budget-v3",
        (deps.now)(),
    ) {
        Ok(l) => l,
        Err(e) => return Err(e.message),
    };
    let file = dir.join("baal-generic-shared-budget-v3.jsonl");
    let actor_file = controller_dir.join("baal-generic-no-v3-journal.jsonl");
    let open_result: Result<(Journal, Option<V>, u64, String, String), String> = (|| {
        basic_check(&deps, &lease, &scope, source_runtime_dir, false, false)?;
        if !file.exists() {
            use std::os::unix::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&file)
                .map_err(|e| e.to_string())?;
        }
        let bytes = fs::read(&file).map_err(|e| e.to_string())?;
        need(
            bytes.is_empty() || *bytes.last().unwrap() == 10,
            "PARTIAL_BUDGET",
        )?;
        let journal = Journal::open(&file)?;
        let mut state: Option<V> = None;
        for line in String::from_utf8_lossy(&bytes).split('\n').filter(|l| !l.is_empty()) {
            let row: V = serde_json::from_str(line).map_err(|_| "BAAL_SHARED_BUDGET_SLOT".to_string())?;
            need(
                sget(get(&row, "state"), "allocationHash") == Some(allocation_hash.as_str())
                    && sget(get(&row, "state"), "budgetInstalledHash") == Some(budget_installed_hash)
                    && sget(get(&row, "state"), "controllerCodeIdentityHash")
                        == Some(controller_code_identity_hash)
                    && sget(get(&row, "state"), "controllerDir")
                        == Some(
                            controller_dir
                                .canonicalize()
                                .unwrap_or(controller_dir.to_path_buf())
                                .to_string_lossy()
                                .as_ref(),
                        )
                    && same(
                        &get(&row, "state").get("scope").cloned().unwrap_or(V::Null),
                        &scope,
                    )?,
                "BUDGET_SLOT_CHANGED",
            )?;
            if let Some(st) = state.as_ref() {
                let ap_next = get(&row, "state")
                    .get("actorProjection")
                    .cloned()
                    .unwrap_or(V::Null);
                forward(get(st, "actorProjection"), &ap_next)?;
            }
            state = Some(get(&row, "state").clone());
        }
        let size = bytes.len() as u64;
        let bytes_hash = sha_bytes(&bytes);
        let identity_stamp = file_stamp(&fs::metadata(&file).map_err(|e| e.to_string())?);
        Ok((journal, state, size, bytes_hash, identity_stamp))
    })();
    let (journal, state, size, bytes_hash, identity_stamp) = match open_result {
        Ok(v) => v,
        Err(e) => {
            let _ = lease.release();
            return Err(e);
        }
    };
    let inner = Rc::new(RefCell::new(BudgetInner {
        dir: dir.to_path_buf(),
        controller_dir: controller_dir.to_path_buf(),
        source_runtime_dir: source_runtime_dir.to_path_buf(),
        scope,
        allocation,
        allocation_hash,
        budget_installed_hash: budget_installed_hash.to_string(),
        controller_code_identity_hash: controller_code_identity_hash.to_string(),
        custody_seed: custody_seed.clone(),
        lease,
        file,
        actor_file,
        journal,
        state,
        closed: false,
        failed: false,
        size,
        bytes_hash,
        identity_stamp,
        cache: None,
        cache_hits: 0,
        cache_misses: 0,
        deps,
    }));
    Ok(BaalGenericSharedBudgetV3 { inner })
}

impl BaalGenericSharedBudgetV3 {
    /// installFundingReservation(record)
    pub fn install_funding_reservation(&self, record: &V) -> Result<V, String> {
        let account_lease_hash = self.inner.borrow().basic()?;
        let s = source(&self.inner)?;
        need(
            same(record, get(&s, "record"))?,
            "NOT_ACTUAL_CONTROLLER_RECORD",
        )?;
        {
            let g = self.inner.borrow();
            if g.state.is_none() {
                need(
                    get(&s, "orders").as_array().is_some_and(|x| x.is_empty())
                        && get(&s, "unknown") == &V::Bool(false)
                        && get(&s, "inheritanceHash") == get(&g.custody_seed, "seedHash")
                        && get(&s, "currentInventoryHash").as_str()
                            == Some(
                                payload_hash(get(&g.custody_seed, "inventory"))?
                                    .as_str(),
                            ),
                    "EXACT_INHERITED_ACTOR_ONLY",
                )?;
            }
        }
        let g = self.inner.borrow().observed()?;
        let risk = total_risk(&g, get(&s, "record"), &self.inner.borrow().allocation, false)?;
        let external_unresolved: Vec<V> = get(&g, "unresolvedIntentIds")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|id| {
                !get(get(&s, "record"), "unresolvedIntentIds")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .any(|x| x == id)
            })
            .collect();
        need(
            external_unresolved.is_empty()
                && !get(&g, "owned")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .any(|r| get(r, "unknownAcceptance") == &V::Bool(true))
                && !get(&g, "loans")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .any(|r| get(r, "unknownAcceptance") == &V::Bool(true)),
            "OTHER_UNKNOWN",
        )?;
        let at = iso((self.inner.borrow().deps.now)());
        let next = {
            let st = self.inner.borrow();
            json!({
                "schemaVersion": 1,
                "mode": "baal_generic_shared_budget_v3",
                "scope": st.scope,
                "allocationHash": st.allocation_hash,
                "budgetInstalledHash": st.budget_installed_hash,
                "controllerCodeIdentityHash": st.controller_code_identity_hash,
                "controllerDir": st.controller_dir.canonicalize().unwrap_or(st.controller_dir.clone()).to_string_lossy(),
                "controllerRootAnchor": get(&s, "opening"),
                "actorAnchor": get(&s, "anchor"),
                "actorProjection": {
                    "record": get(&s, "record"),
                    "orders": get(&s, "orders"),
                    "spent": get(&s, "spent"),
                    "unknown": get(&s, "unknown"),
                    "inheritanceHash": get(&s, "inheritanceHash"),
                    "currentInventoryHash": get(&s, "currentInventoryHash"),
                },
                "risk": risk,
                "externalObservation": g_raw(&g),
                "accountLeaseHash": account_lease_hash,
                "cumulativeActualExpenses": get(&s, "spent"),
                "unknownAcceptance": js_truthy(Some(get(&s, "unknown"))) || js_truthy(Some(get(&g, "unknownAcceptance"))),
                "at": at,
            })
        };
        append(&self.inner, &next)?;
        let previous = self.inner.borrow().journal.previous.clone();
        Ok(json!({
            "reservationHash": get(get(&s, "record"), "reservationHash"),
            "allocationHash": self.inner.borrow().allocation_hash,
            "globalLedgerHash": previous,
            "confirmedAt": at,
        }))
    }

    /// globalRisk / getGlobalRisk
    pub fn global_risk(&self) -> Result<V, String> {
        self.inner.borrow().basic()?;
        need(self.inner.borrow().state.is_some(), "BUDGET_NOT_INSTALLED")?;
        let s = source(&self.inner)?;
        let g = self.inner.borrow().observed()?;
        {
            let st = self.inner.borrow();
            need(
                get(get(&s, "record"), "reservationHash")
                    == get(
                        get(st.state.as_ref().unwrap(), "actorProjection"),
                        "record",
                    )
                    .get("reservationHash")
                    .unwrap_or(&V::Null),
                "CURRENT_RESERVE_NOT_INSTALLED",
            )?;
        }
        let r = total_risk(&g, get(&s, "record"), &self.inner.borrow().allocation, true)?;
        let mut unresolved: Vec<V> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for id in get(get(&s, "record"), "unresolvedIntentIds")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .chain(
                get(&g, "unresolvedIntentIds")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter(),
            )
        {
            let k = serde_json::to_string(&id).unwrap_or_default();
            if seen.insert(k) {
                unresolved.push(id);
            }
        }
        let mut out = g.as_object().cloned().unwrap_or_default();
        for (k, v) in r.as_object().unwrap() {
            out.insert(k.clone(), v.clone());
        }
        {
            let st = self.inner.borrow();
            out.insert(
                "installedCashLower".into(),
                get(st.state.as_ref().unwrap(), "externalObservation")
                    .get("cashLower")
                    .cloned()
                    .unwrap_or(V::Null),
            );
            out.insert("allocationHash".into(), json!(st.allocation_hash));
            out.insert(
                "budgetInstalledHash".into(),
                json!(st.budget_installed_hash),
            );
            out.insert(
                "controllerReservationHash".into(),
                get(get(&s, "record"), "reservationHash").clone(),
            );
            out.insert("globalLedgerHash".into(), json!(st.journal.previous));
            out.insert(
                "sourceHash".into(),
                json!(payload_hash(&json!({
                    "observation": get(&g, "sourceHash"),
                    "actorAnchor": get(&s, "anchor"),
                    "reservation": get(get(&s, "record"), "reservationHash"),
                    "globalLedgerHash": st.journal.previous,
                }))?),
            );
        }
        let any_unknown = js_truthy(Some(get(&s, "unknown")))
            || js_truthy(Some(get(&g, "unknownAcceptance")))
            || ["owned", "reservations", "loans"]
                .iter()
                .any(|k| {
                    get(&r, k)
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .any(|v| get(v, "unknownAcceptance") == &V::Bool(true))
                });
        out.insert("unknownAcceptance".into(), json!(any_unknown));
        out.insert("unresolvedIntentIds".into(), json!(unresolved));
        out.insert("loansDiscounted".into(), json!(false));
        out.insert("executionAuthority".into(), json!(false));
        Ok(V::Object(out))
    }

    pub fn status(&self) -> V {
        let mut g = self.inner.borrow_mut();
        let mut unknown = g
            .state
            .as_ref()
            .map(|s| get(s, "unknownAcceptance") == &V::Bool(true))
            .unwrap_or(false);
        if g.state.is_some() && !g.closed {
            let inner2 = self.inner.clone();
            drop(g);
            match source(&inner2) {
                Ok(s) => {
                    unknown = unknown || js_truthy(Some(get(&s, "unknown")));
                }
                Err(_) => {
                    unknown = true;
                }
            }
            g = self.inner.borrow_mut();
        }
        let (sequence, previous, claimed, expenses, risk, reservation_hash, actor_anchor, root_anchor, closed, failed, hits, misses) = {
            let st = &g;
            (
                st.journal.sequence,
                st.journal.previous.clone(),
                st.state.is_some(),
                st.state
                    .as_ref()
                    .map(|s| get(s, "cumulativeActualExpenses").clone())
                    .unwrap_or(json!("0.000000")),
                st.state
                    .as_ref()
                    .map(|s| get(s, "risk").clone())
                    .unwrap_or(V::Null),
                st.state
                    .as_ref()
                    .map(|s| get(get(get(s, "actorProjection"), "record"), "reservationHash").clone())
                    .unwrap_or(V::Null),
                st.state
                    .as_ref()
                    .map(|s| get(s, "actorAnchor").clone())
                    .unwrap_or(V::Null),
                st.state
                    .as_ref()
                    .map(|s| get(s, "controllerRootAnchor").clone())
                    .unwrap_or(V::Null),
                st.closed,
                st.failed,
                st.cache_hits,
                st.cache_misses,
            )
        };
        json!({
            "mode": "baal_generic_shared_budget_v3",
            "allocationHash": g.allocation_hash,
            "budgetInstalledHash": g.budget_installed_hash,
            "controllerCodeIdentityHash": g.controller_code_identity_hash,
            "claimed": claimed,
            "globalLedgerHash": previous,
            "sequence": sequence,
            "controllerReservationHash": reservation_hash,
            "cumulativeActualExpenses": expenses,
            "risk": risk,
            "unknownAcceptance": unknown,
            "actorAnchor": actor_anchor,
            "controllerRootAnchor": root_anchor,
            "closed": closed,
            "failed": failed,
            "cacheHits": hits,
            "cacheMisses": misses,
            "executionAuthority": false,
            "additionalApiClients": 0,
        })
    }

    pub fn close(&self) -> Result<(), String> {
        let mut g = self.inner.borrow_mut();
        if g.closed {
            return Ok(());
        }
        g.closed = true;
        g.lease.release().map_err(|e| e.message)?;
        Ok(())
    }
}

trait BorrowV {
    fn borrow_v(&self) -> &V;
}
impl BorrowV for V {
    fn borrow_v(&self) -> &V {
        self
    }
}
fn g_raw(g: &V) -> V {
    g.clone()
}
