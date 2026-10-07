// execution/baal-generic-shared-budget-v9.mjs — durable local allocation
// slot; source-replayed event-only generic controller budget. Inherited
// inventory must match the complete validated custody seed. No extra client,
// collateral discount or activation. The mandatory caller supplies the
// existing real account lease and observations; this file is not a
// distributed lock.
//
// Port notes:
// - `need` throws BAAL_SHARED_BUDGET_<code>; verifier/hash errors propagate.
// - sync(fn) => Fn traits returning Result + a truthy check on the value.
// - `stamp` (ino,birthtimeMs) -> (u64, f64) identity tuple (a "ino:ms" string
//   inside the cache key, matching JS join(':')).
// - `path.resolve` comparisons -> lexical absolute-path equality.
// - The actor journal is fully re-verified via
//   replay_baal_generic_no_journal_v9 (economic replay) instead of the
//   scaled module's chain-only walk.
use crate::baal_expansion_recovered_seed_v2::validate_baal_expansion_recovered_seed_v2;
use crate::baal_generic_no_controller_v9::{
    baal_generic_no_controller_v9_identity, project_baal_generic_reservation_v9,
    replay_baal_generic_no_journal_v9,
};
use baal_execution::core::{canonical_payload, payload_hash};
use crate::proven_unfetched_fences_v2::proven_unfetched_fences_v2;
use baal_execution::ledger::acquire_owner_lease;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::{amount, instant, money, price};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use baal_service_runtime::ledger::OwnerLease;
use serde_json::{json, Map, Value as V};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("BAAL_SHARED_BUDGET_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
fn hex(x: &V) -> bool {
    x.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn sha_bytes(b: &[u8]) -> String {
    sha256(b)
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(payload_hash(a)? == payload_hash(b)?)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().ok_or_else(|| "INVALID_INSTANT".to_string())?)
}
fn str_amount(v: &V) -> Result<i128, String> {
    amount(v.as_str().ok_or_else(|| "INVALID".to_string())?)
}
fn num_str(v: &V) -> String {
    js_stringify(v)
}
// JS Number#toString for the cache-key stamp: integer-valued floats render
// without a decimal point; Rust Display is shortest-roundtrip like V9.
fn js_f64(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i128)
    } else {
        format!("{n}")
    }
}
fn stat_identity(m: &fs::Metadata) -> (u64, f64) {
    use std::os::unix::fs::MetadataExt;
    let birthtime = m
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0);
    (m.ino(), birthtime)
}

fn proof(p: &V) -> Result<(), String> {
    let mut body = p.as_object().cloned().unwrap_or_default();
    let vh = body.remove("verificationHash").unwrap_or(V::Null);
    need(
        hex(&vh)
            && payload_hash(&V::Object(body))? == vh.as_str().unwrap_or("")
            && get(p, "scopedTradeVerified") == &V::Bool(true),
        "OWN_PROOF",
    )?;
    Ok(())
}

#[derive(Clone)]
struct Projection {
    record: V,
    orders: Vec<V>,
    spent: String,
    unknown: bool,
    inheritance_hash: V,
    current_inventory_hash: String,
}

// cache: the JS {key,value:copy({result:{anchor,opening},p})} memo.
#[derive(Clone)]
struct SourceCache {
    key: String,
    result: V,
    p: Projection,
}

fn projection(
    s: &V,
    scope: &V,
    allocation_hash: &str,
    custody_seed: &V,
) -> Result<Projection, String> {
    need(
        get(s, "mode").as_str() == Some("baal_generic_no_controller_v9")
            && same(get(s, "scope"), scope)?
            && payload_hash(get(s, "allocation"))? == allocation_hash
            && get(s, "orders").is_array(),
        "ACTUAL_SCOPE",
    )?;
    let mut intents: HashSet<String> = HashSet::new();
    let mut keys: HashSet<String> = HashSet::new();
    let mut spent: i128 = 0;
    let max_order = str_amount(&match get(s, "allocation").get("maximumOrderCost") {
        Some(x) if !x.is_null() => x.clone(),
        _ => json!("25.000000"),
    })?;
    for o in get(s, "orders").as_array().unwrap() {
        let iid = sget(o, "intentId");
        let body = get(o, "body");
        need(
            iid.is_some()
                && !intents.contains(iid.unwrap())
                && sget(body, "side") == Some("no")
                && ["buy", "sell"].contains(&sget(body, "action").unwrap_or(""))
                && js_int(get(body, "quantity")).is_some_and(|q| q >= 1 && q <= 100)
                && get(body, "tournamentId") == get(scope, "tournamentId")
                && get(scope, "exchanges")
                    .as_array()
                    .is_some_and(|xs| {
                        xs.iter().any(|m| {
                            sget(m, "exchangeId") == sget(body, "exchangeId")
                                && sget(m, "marketId") == sget(o, "marketId")
                        })
                    })
                && get(o, "bodyJson").as_str()
                    == Some(canonical_payload(body)?.as_str())
                && get(o, "bodyHash").as_str() == Some(payload_hash(body)?.as_str())
                && !keys.contains(sget(body, "idempotencyKey").unwrap_or("\u{0}"))
                && price(&num_str(&get(body, "price")))?
                    * js_int(get(body, "quantity")).unwrap_or(0) as i128
                    <= max_order,
            "ORDER_OR_DUPLICATE",
        )?;
        intents.insert(iid.unwrap().to_string());
        keys.insert(sget(body, "idempotencyKey").unwrap_or("").to_string());
        if sget(o, "phase") == Some("abandoned") {
            need(
                get(o, "proof") == &V::Null
                    && get(o, "orderId") == &V::Null
                    && get(o, "ack") == &V::Null
                    && o.get("operation").is_none()
                    && get(o, "cancelPending") == &V::Bool(false)
                    && proven_unfetched_fences_v2(o)?
                    && get(o, "fetchStarts").as_array().is_some_and(|x| x.is_empty())
                    && get(o, "receipts").as_array().is_some_and(|x| x.is_empty())
                    && instant_v(get(o, "abandonedAt"))?
                        >= instant_v(get(get(o, "body"), "expirationDate"))?
                    && sget(o, "abandonedReason") == Some("expired_before_any_admission"),
                "ABANDONED_NOT_PROVEN_UNFETCHED",
            )?;
        }
        if !get(o, "proof").is_null() {
            proof(get(o, "proof"))?;
            need(
                sget(o, "phase") == Some("reconciled")
                    && get(get(o, "proof"), "ownerId") == get(scope, "ownerId")
                    && get(get(o, "proof"), "runId") == get(scope, "runId")
                    && get(get(o, "proof"), "tournamentId") == get(scope, "tournamentId")
                    && get(get(o, "proof"), "intentId") == get(o, "intentId")
                    && get(get(o, "proof"), "orderId") == get(o, "orderId")
                    && get(get(o, "proof"), "marketId") == get(o, "marketId")
                    && get(get(o, "proof"), "exchangeId") == get(get(o, "body"), "exchangeId")
                    && get(get(o, "proof"), "action") == get(get(o, "body"), "action"),
                "ORDER_PROOF_BINDING",
            )?;
            spent += str_amount(get(get(o, "proof"), "scopedPrincipalCost"))?;
        }
    }
    for (ex, slot) in get(s, "inventory").as_object().map(|m| m.iter().collect::<Vec<_>>()).unwrap_or_default() {
        proof(get(slot, "proof"))?;
        need(
            sget(get(slot, "proof"), "exchangeId") == Some(ex.as_str())
                && (get(s, "orders").as_array().unwrap().iter().any(|o| {
                    !get(o, "proof").is_null() && same(get(o, "proof"), get(slot, "proof")).unwrap_or(false)
                }) || same(
                    get(get(custody_seed, "inventory"), ex.as_str()),
                    slot,
                )?),
            "NO_OTHER_ACTOR_ADOPTION",
        )?;
    }
    let record = project_baal_generic_reservation_v9(s)?;
    need(
        get(&record, "allocationHash").as_str() == Some(allocation_hash)
            && get(&record, "owned")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .all(|r| str_amount(get(r, "nominalCost")).unwrap_or(-1) >= 0)
            && get(&record, "reservations")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .all(|r| str_amount(get(r, "maximumNominalCost")).unwrap_or(-1) >= 0),
        "NONNEGATIVE_FULL_RESERVE",
    )?;
    let pending: Vec<&V> = get(s, "orders")
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| get(o, "proof").is_null() && sget(o, "phase") != Some("abandoned"))
        .collect();
    need(pending.len() <= 1, "SHADOW_PENDING")?;
    let unknown = pending.iter().any(|o| {
        ["uncertain", "semantic_mismatch"].contains(&sget(o, "phase").unwrap_or(""))
            || get(o, "semanticMismatch") == &V::Bool(true)
            || (get(o, "orderId").is_null()
                && get(o, "fetchStarts").as_array().is_some_and(|f| !f.is_empty()))
    });
    Ok(Projection {
        record,
        orders: get(s, "orders")
            .as_array()
            .unwrap()
            .iter()
            .map(|o| {
                json!({
                    "intentId": get(o, "intentId"),
                    "bodyHash": get(o, "bodyHash"),
                    "bodyJson": get(o, "bodyJson"),
                    "orderId": get(o, "orderId"),
                    "phase": get(o, "phase"),
                    "proofHash": get(get(o, "proof"), "verificationHash"),
                    "fetchStarts": get(o, "fetchStarts"),
                })
            })
            .collect(),
        spent: money(spent),
        unknown,
        inheritance_hash: get(s, "inheritanceHash").clone(),
        current_inventory_hash: payload_hash(get(s, "inventory"))?,
    })
}

// forward(old,next): every recorded intent must still be present, identical
// and not regressed; abandoned stays abandoned; fetchStarts only grow.
fn forward(old: Option<&V>, next_orders: &[V], next_spent: &str) -> Result<(), String> {
    let old = match old {
        Some(o) if !o.is_null() => o,
        _ => return Ok(()),
    };
    need(
        str_amount(&json!(next_spent.to_string()))? >= str_amount(get(old, "spent"))?
            && next_orders.len()
                >= get(old, "orders").as_array().cloned().unwrap_or_default().len(),
        "EXPENSE_OR_ORDER_RESET",
    )?;
    for o in get(old, "orders").as_array().cloned().unwrap_or_default() {
        let n = next_orders
            .iter()
            .find(|n| get(n, "intentId") == get(&o, "intentId"))
            .cloned()
            .unwrap_or(V::Null);
        let fetches_ok = get(&o, "fetchStarts")
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
            });
        need(
            !n.is_null()
                && get(&n, "bodyHash") == get(&o, "bodyHash")
                && get(&n, "bodyJson") == get(&o, "bodyJson")
                && (get(&o, "orderId").is_null() || get(&n, "orderId") == get(&o, "orderId"))
                && (get(&o, "proofHash").is_null() || get(&n, "proofHash") == get(&o, "proofHash"))
                && (sget(&o, "phase") != Some("abandoned") || sget(&n, "phase") == Some("abandoned"))
                && fetches_ok,
            "ORIGINAL_INTENT_FILL_RESET",
        )?;
    }
    Ok(())
}

fn total_risk(
    observed: &V,
    record: &V,
    allocation: &V,
    allow_funding_shortfall: bool,
) -> Result<V, String> {
    let mut out = Map::new();
    out.insert("owned".into(), get(record, "owned").clone());
    out.insert("reservations".into(), get(record, "reservations").clone());
    out.insert("loans".into(), json!([]));
    let mut ids: HashSet<String> = get(record, "owned")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .chain(get(record, "reservations").as_array().cloned().unwrap_or_default().iter())
        .map(|r| sget(r, "exposureId").unwrap_or("").to_string())
        .collect();
    need(
        ids.len()
            == get(record, "owned").as_array().unwrap().len()
                + get(record, "reservations").as_array().unwrap().len(),
        "DUPLICATE_OWN_EXPOSURE",
    )?;
    for (k, f) in [
        ("owned", "nominalCost"),
        ("reservations", "maximumNominalCost"),
        ("loans", "fullNominalPrincipal"),
    ] {
        need(observed.get(k).is_some_and(V::is_array), "OBSERVED_EXPOSURES")?;
        for r in get(observed, k).as_array().unwrap() {
            need(
                sget(r, "exposureId").is_some_and(|s| !s.is_empty())
                    && sget(r, "marketId").is_some()
                    && sget(r, "eventId").is_some_and(|s| !s.is_empty())
                    && str_amount(get(r, f))? >= 0,
                "OBSERVED_EXPOSURE",
            )?;
            if ids.contains(sget(r, "exposureId").unwrap_or("")) {
                need(
                    get(&V::Object(out.clone()), k)
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .any(|v| same(v, r).unwrap_or(false)),
                    "SHADOW_EXPOSURE",
                )?;
                continue;
            }
            need(
                !sget(r, "exposureId")
                    .unwrap_or("")
                    .starts_with(&format!("scaled:{}:", sget(record, "runId").unwrap_or(""))),
                "UNMATCHED_OWN_SHADOW",
            )?;
            ids.insert(sget(r, "exposureId").unwrap().to_string());
            out.get_mut(k)
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(r.clone());
        }
    }
    let mut nominal: i128 = 0;
    let mut reserved: i128 = 0;
    let mut markets: Vec<(String, i128)> = Vec::new();
    let mut events: Vec<(String, i128)> = Vec::new();
    let mut market_events: HashMap<String, String> = HashMap::new();
    for (k, f) in [
        ("owned", "nominalCost"),
        ("reservations", "maximumNominalCost"),
        ("loans", "fullNominalPrincipal"),
    ] {
        for r in get(&V::Object(out.clone()), k).as_array().cloned().unwrap_or_default() {
            let n = str_amount(get(&r, f))?;
            nominal += n;
            if k == "reservations" {
                reserved += n;
            }
            let market = sget(&r, "marketId").unwrap_or("").to_string();
            let event = sget(&r, "eventId").unwrap_or("").to_string();
            need(
                market_events.get(&market).map(|e| e == &event).unwrap_or(true),
                "EVENT_MAPPING_CONFLICT",
            )?;
            market_events.insert(market.clone(), event.clone());
            if let Some(m) = markets.iter_mut().find(|(mk, _)| *mk == market) {
                m.1 += n;
            } else {
                markets.push((market, n));
            }
            if let Some(m) = events.iter_mut().find(|(ek, _)| *ek == event) {
                m.1 += n;
            } else {
                events.push((event, n));
            }
        }
    }
    let max_active = match allocation.get("maximumActiveMarkets") {
        Some(x) if !x.is_null() => js_int(x).unwrap_or(0),
        _ => 20,
    };
    need(
        markets.iter().filter(|(_, n)| *n > 0).count() as i64 <= max_active,
        "ACTIVE_MARKET_CEILING",
    )?;
    let max_market = str_amount(&match allocation.get("maximumMarketCost") {
        Some(x) if !x.is_null() => x.clone(),
        _ => json!("100.000000"),
    })?;
    let max_event = str_amount(&match allocation.get("maximumEventCost") {
        Some(x) if !x.is_null() => x.clone(),
        _ => json!("200.000000"),
    })?;
    need(
        nominal <= 10000000000
            && markets.iter().all(|(_, n)| *n <= max_market)
            && events.iter().all(|(_, n)| *n <= max_event),
        "NOMINAL_LIMIT",
    )?;
    let cash_lower = str_amount(get(observed, "cashLower"))?;
    need(
        cash_lower >= 0 && (allow_funding_shortfall || reserved <= cash_lower),
        "FULL_CASH_RESERVATION",
    )?;
    out.insert("fundingSatisfied".into(), json!(reserved <= cash_lower));
    out.insert("nominalExposure".into(), json!(money(nominal)));
    out.insert("reservedCash".into(), json!(money(reserved)));
    let mut markets_map = Map::new();
    for (k, v) in markets {
        markets_map.insert(k, json!(money(v)));
    }
    let mut events_map = Map::new();
    for (k, v) in events {
        events_map.insert(k, json!(money(v)));
    }
    out.insert("markets".into(), V::Object(markets_map));
    out.insert("events".into(), V::Object(events_map));
    Ok(V::Object(out))
}

pub struct BaalGenericSharedBudgetV9Deps {
    pub assert_account_lease: Rc<dyn Fn() -> V>,
    pub assert_financial_owner_lease: Rc<dyn Fn() -> V>,
    pub get_controller_source: Rc<dyn Fn() -> Result<V, String>>,
    pub observe_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct BudgetInner {
    dir: PathBuf,
    scope: V,
    allocation: V,
    allocation_hash: String,
    budget_installed_hash: String,
    controller_code_identity_hash: String,
    controller_dir: PathBuf,
    source_runtime_dir: PathBuf,
    custody_seed: V,
    file: PathBuf,
    actor_file: PathBuf,
    journal: Journal,
    state: Option<V>,
    closed: bool,
    failed: bool,
    size: u64,
    bytes_hash: String,
    identity: (u64, f64),
    lease: OwnerLease,
    source_cache: RefCell<Option<SourceCache>>,
    cache_hits: Cell<u64>,
    cache_misses: Cell<u64>,
    deps: BaalGenericSharedBudgetV9Deps,
}

pub struct BaalGenericSharedBudgetV9 {
    inner: Rc<RefCell<BudgetInner>>,
}

struct SourceResult {
    record: V,
    orders: Vec<V>,
    spent: String,
    unknown: bool,
    inheritance_hash: V,
    current_inventory_hash: String,
    anchor: V,
    opening: V,
}

impl BudgetInner {
    fn basic(&self) -> Result<String, String> {
        need(!self.closed && !self.failed && (self.deps.can_operate)(), "STOPPED")?;
        self.lease.assert_ownership().map_err(|e| e.message)?;
        let l = (self.deps.assert_account_lease)();
        need(js_truthy(Some(&l)), "SYNC_CALLBACK")?;
        let f = (self.deps.assert_financial_owner_lease)();
        need(js_truthy(Some(&f)), "SYNC_CALLBACK")?;
        let actual: V = serde_json::from_str(&String::from_utf8_lossy(
            &fs::read(
                self.source_runtime_dir
                    .join("baal-financial-owner")
                    .join("execution-owner.lock"),
            )
            .map_err(|e| e.to_string())?,
        ))
        .map_err(|_| "BAAL_SHARED_BUDGET_ACTUAL_FINANCIAL_OWNER".to_string())?;
        need(
            get(&actual, "ownerId") == get(&self.scope, "ownerId")
                && get(&actual, "runId") == get(&self.scope, "runId")
                && js_int(get(&actual, "pid")) == Some(std::process::id() as i64)
                && sget(&actual, "scope") == Some("baal-global-financial-owner")
                && sget(&f, "leaseId") == Some(payload_hash(&actual)?.as_str())
                && get(&f, "ownerId") == get(&self.scope, "ownerId")
                && get(&f, "runId") == get(&self.scope, "runId")
                && get(&f, "tournamentId") == get(&self.scope, "tournamentId"),
            "ACTUAL_FINANCIAL_OWNER",
        )?;
        need(
            get(&l, "ownerId") == get(&self.scope, "ownerId")
                && get(&l, "tournamentId") == get(&self.scope, "tournamentId")
                && sget(&l, "leaseId").is_some_and(|s| !s.is_empty()),
            "ACTUAL_ACCOUNT_LEASE",
        )?;
        payload_hash(&l)
    }

    fn observed(&self) -> Result<V, String> {
        let g = (self.deps.observe_global_risk)()?;
        need(js_truthy(Some(&g)), "SYNC_CALLBACK")?;
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

    fn source(&self) -> Result<SourceResult, String> {
        self.basic()?;
        need(
            payload_hash(&baal_generic_no_controller_v9_identity()?)?
                == self.controller_code_identity_hash,
            "ACTUAL_CONTROLLER_CODE_CHANGED",
        )?;
        let hint = (self.deps.get_controller_source)()?;
        need(js_truthy(Some(&hint)), "SYNC_CALLBACK")?;
        // path.resolve(): lexical normalization of the absolute path.
        let resolve = |p: &Path| -> PathBuf {
            let mut out = PathBuf::from("/");
            for c in p.components() {
                match c {
                    std::path::Component::ParentDir => {
                        out.pop();
                    }
                    std::path::Component::CurDir => {}
                    _ => out.push(c.as_os_str()),
                }
            }
            out
        };
        let hinted = match hint.get("journalFile") {
            Some(x) if !x.is_null() => PathBuf::from(x.as_str().unwrap_or("")),
            _ => self.actor_file.clone(),
        };
        need(resolve(&hinted) == resolve(&self.actor_file), "FIXED_ACTOR_PATH")?;
        let before = fs::metadata(&self.actor_file).map_err(|e| e.to_string())?;
        let bytes = fs::read(&self.actor_file).map_err(|e| e.to_string())?;
        need(
            !bytes.is_empty() && bytes.len() <= 128 * 1024 * 1024 && *bytes.last().unwrap() == 10,
            "ACTOR_PARTIAL_OR_SIZE",
        )?;
        if let Some(anchor) = self
            .state
            .as_ref()
            .and_then(|st| st.get("actorAnchor"))
            .filter(|v| !v.is_null())
        {
            need(
                bytes.len() as u64 >= anchor.get("bytes").and_then(V::as_u64).unwrap_or(0)
                    && sha_bytes(&bytes[..anchor.get("bytes").and_then(V::as_u64).unwrap_or(0) as usize])
                        == get(anchor, "sha256").as_str().unwrap_or(""),
                "ACTOR_PREFIX_RESET",
            )?;
        }
        let current_hash = sha_bytes(&bytes);
        let (ino, birthtime) = stat_identity(&before);
        // JS [ino, birthtimeMs].join(':'): f64 rendered like Number#toString.
        let identity = format!("{}:{}", ino, js_f64(birthtime));
        let cache_key = payload_hash(&json!({
            "currentHash": current_hash,
            "size": bytes.len(),
            "identity": identity,
            "controllerCodeIdentityHash": self.controller_code_identity_hash,
            "seedHash": get(&self.custody_seed, "seedHash"),
        }))?;
        let opening: V;
        let seq: u64;
        let previous: String;
        let p: Projection;
        let cached = self
            .source_cache
            .borrow()
            .as_ref()
            .filter(|c| c.key == cache_key)
            .cloned();
        let mut result: V;
        if let Some(c) = cached {
            self.cache_hits.set(self.cache_hits.get() + 1);
            result = c.result;
            p = c.p;
        } else {
            self.cache_misses.set(self.cache_misses.get() + 1);
            let replay = replay_baal_generic_no_journal_v9(
                &String::from_utf8_lossy(&bytes),
                &baal_generic_no_controller_v9_identity()?,
                &self.custody_seed,
            )?;
            need(
                get(&get(&replay, "state"), "activation")
                    .get("budgetInstalledHash")
                    .and_then(V::as_str)
                    == Some(self.budget_installed_hash.as_str()),
                "ACTOR_BUDGET_BINDING",
            )?;
            let pr = projection(
                get(&replay, "state"),
                &self.scope,
                &self.allocation_hash,
                &self.custody_seed,
            )?;
            need(
                pr.inheritance_hash == *get(&self.custody_seed, "seedHash"),
                "EXACT_INHERITED_ROOT",
            )?;
            let nl = bytes.iter().position(|b| *b == 10).unwrap_or(0);
            let line = &bytes[..nl + 1];
            let r: V = serde_json::from_str(&String::from_utf8_lossy(line))
                .map_err(|_| "BAAL_SHARED_BUDGET_ACTOR_CHAIN_AND_CODE".to_string())?;
            result = json!({
                "anchor": get(&replay, "anchor"),
                "opening": {
                    "bytes": line.len(),
                    "sha256": sha_bytes(line),
                    "sequence": 1,
                    "hash": get(&r, "hash"),
                },
            });
            *self.source_cache.borrow_mut() = Some(SourceCache {
                key: cache_key,
                result: result.clone(),
                p: pr.clone(),
            });
            p = pr;
        }
        opening = get(&result, "opening").clone();
        seq = js_int(get(&get(&result, "anchor"), "sequence")).unwrap_or(0) as u64;
        previous = get(&get(&result, "anchor"), "hash")
            .as_str()
            .unwrap_or("")
            .to_string();
        need(
            stat_identity(&fs::metadata(&self.actor_file).map_err(|e| e.to_string())?)
                == stat_identity(&before)
                && fs::metadata(&self.actor_file).map_err(|e| e.to_string())?.len() as u64
                    == bytes.len() as u64,
            "ACTOR_CHANGED",
        )?;
        if let Some(root) = self
            .state
            .as_ref()
            .and_then(|st| st.get("controllerRootAnchor"))
            .filter(|v| !v.is_null())
        {
            need(same(&opening, root)?, "ORIGINAL_ROOT_PREFIX_CHANGED")?;
        }
        if let Some(hint_root) = hint.get("rootPrefix").filter(|v| !v.is_null()) {
            need(same(hint_root, &opening)?, "CALLER_ROOT_PREFIX")?;
        }
        need(
            !hint.get("record").is_none()
                && same(hint.get("record").unwrap(), &p.record)?,
            "CALLER_SHADOW_RECORD",
        )?;
        need(
            js_int(get(&hint, "sequence")) == Some(seq as i64)
                && get(&hint, "hash").as_str() == Some(previous.as_str()),
            "CALLER_ACTOR_FRONTIER",
        )?;
        forward(
            self.state.as_ref().and_then(|st| st.get("actorProjection")),
            &p.orders,
            &p.spent,
        )?;
        let anchor = json!({
            "bytes": bytes.len(),
            "sha256": current_hash,
            "sequence": seq,
            "hash": previous,
        });
        Ok(SourceResult {
            record: p.record,
            orders: p.orders,
            spent: p.spent,
            unknown: p.unknown,
            inheritance_hash: p.inheritance_hash,
            current_inventory_hash: p.current_inventory_hash,
            anchor,
            opening,
        })
    }
}
fn sha_str(s: &str) -> String {
    sha256(s.as_bytes())
}

fn append_event(inner: &Rc<RefCell<BudgetInner>>, next: &V) -> Result<(), String> {
    inner.borrow().basic()?;
    let (before, size, bytes_hash, identity) = {
        let g = inner.borrow();
        let before = fs::read(&g.file).map_err(|e| e.to_string())?;
        let s = fs::metadata(&g.file).map_err(|e| e.to_string())?;
        (before, g.size, g.bytes_hash.clone(), stat_identity(&s))
    };
    need(
        identity == inner.borrow().identity
            && before.len() as u64 == size
            && sha_bytes(&before) == bytes_hash,
        "BUDGET_RESET",
    )?;
    let event = {
        let g = inner.borrow();
        json!({
            "kind": "source_bound_nominal_budget",
            "allocationHash": g.allocation_hash,
            "budgetInstalledHash": g.budget_installed_hash,
        })
    };
    let commit_result = inner.borrow_mut().journal.commit(next, &event);
    match commit_result {
        Ok(_) => {
            inner.borrow().basic()?;
            let after = fs::read(inner.borrow().file.clone()).map_err(|e| e.to_string())?;
            need(after[..size as usize] == before[..], "BUDGET_PREFIX_CHANGED")?;
            let mut g = inner.borrow_mut();
            g.state = Some(next.clone());
            g.size = after.len() as u64;
            g.bytes_hash = sha_bytes(&after);
            Ok(())
        }
        Err(e) => {
            inner.borrow_mut().failed = true;
            Err(e)
        }
    }
}

// openBaalGenericSharedBudgetV9({dir,controllerDir,sourceRuntimeDir,
//   custodySeed,allocation,scope,budgetInstalledHash,
//   controllerCodeIdentityHash,assertAccountLease,assertFinancialOwnerLease,
//   getControllerSource,observeGlobalRisk,canOperate,now})
pub fn open_baal_generic_shared_budget_v9(
    dir: &Path,
    controller_dir: &Path,
    source_runtime_dir: &Path,
    custody_seed: &V,
    allocation: &V,
    scope: &V,
    budget_installed_hash: &str,
    controller_code_identity_hash: &str,
    deps: BaalGenericSharedBudgetV9Deps,
) -> Result<BaalGenericSharedBudgetV9, String> {
    need(
        dir.is_absolute()
            && controller_dir.is_absolute()
            && dir != controller_dir
            && hex(&json!(budget_installed_hash))
            && hex(&json!(controller_code_identity_hash)),
        "DEPENDENCIES",
    )?;
    need(source_runtime_dir.is_absolute(), "SOURCE_RUNTIME")?;
    validate_baal_expansion_recovered_seed_v2(custody_seed, scope, allocation)?;
    let allocation_hash = payload_hash(allocation)?;
    need(
        js_int(get(allocation, "schemaVersion")) == Some(1)
            && sget(allocation, "ownerId") == Some("Codex")
            && get(allocation, "runId") == get(scope, "runId")
            && get(allocation, "tournamentId") == get(scope, "tournamentId")
            && sget(allocation, "capital") == Some("10000.000000")
            && hex(get(allocation, "humanAllocationSourceHash"))
            && sget(scope, "ownerId") == Some("Codex")
            && scope.get("exchanges").is_some_and(V::is_array),
        "ALLOCATION",
    )?;
    for (k, cap) in [
        ("maximumOrderCost", 100000000i128),
        ("maximumMarketCost", 1000000000i128),
        ("maximumEventCost", 2000000000i128),
    ] {
        let v = match allocation.get(k) {
            Some(x) if !x.is_null() => x.clone(),
            _ => json!(money(cap)),
        };
        let n = str_amount(&v)?;
        need(n > 0 && n <= cap, "ALLOCATION_CEILING")?;
    }
    let opt_int = |k: &str, d: i64| -> Option<i64> {
        match allocation.get(k) {
            Some(x) if !x.is_null() => js_int(x),
            _ => Some(d),
        }
    };
    need(
        opt_int("maximumActiveMarkets", 20).is_some_and(|v| (1..=100).contains(&v)),
        "ACTIVE_MARKET_CEILING",
    )?;
    need(
        opt_int("maximumSharesPerOrder", 100).is_some_and(|v| (1..=100).contains(&v))
            && instant_v(get(allocation, "recordedAt"))? <= (deps.now)() as i64,
        "ALLOCATION_CLOCK_OR_SHARES",
    )?;
    let mut lease = acquire_owner_lease(
        dir,
        sget(scope, "ownerId").unwrap_or(""),
        sget(scope, "runId").unwrap_or(""),
        "baal-generic-shared-budget-v9",
        (deps.now)(),
    )
    .map_err(|e| e.message)?;
    let file = dir.join("baal-generic-shared-budget-v9.jsonl");
    let actor_file = controller_dir.join("baal-generic-no-v9-journal.jsonl");
    let open_result: Result<(Journal, Option<V>, u64, String, (u64, f64)), String> = (|| {
        if !file.exists() {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
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
            let r: V = serde_json::from_str(line).map_err(|_| "BAAL_SHARED_BUDGET_REPLAY".to_string())?;
            need(
                get(get(&r, "state"), "allocationHash").as_str() == Some(allocation_hash.as_str())
                    && get(get(&r, "state"), "budgetInstalledHash").as_str()
                        == Some(budget_installed_hash)
                    && get(get(&r, "state"), "controllerCodeIdentityHash").as_str()
                        == Some(controller_code_identity_hash)
                    && get(get(&r, "state"), "controllerDir").as_str()
                        == Some(controller_dir.to_string_lossy().as_ref())
                    && same(get(get(&r, "state"), "scope"), scope)?,
                "BUDGET_SLOT_CHANGED",
            )?;
            if let Some(st) = state.as_ref() {
                forward(
                    st.get("actorProjection"),
                    get(get(&r, "state"), "actorProjection")
                        .get("orders")
                        .and_then(V::as_array)
                        .cloned()
                        .unwrap_or_default()
                        .as_slice(),
                    get(get(&r, "state"), "actorProjection")
                        .get("spent")
                        .and_then(V::as_str)
                        .unwrap_or("0.000000"),
                )?;
            }
            state = Some(get(&r, "state").clone());
        }
        let size = bytes.len() as u64;
        let bytes_hash = sha_bytes(&bytes);
        let identity = stat_identity(&fs::metadata(&file).map_err(|e| e.to_string())?);
        Ok((journal, state, size, bytes_hash, identity))
    })();
    let (journal, state, size, bytes_hash, identity) = match open_result {
        Ok(v) => v,
        Err(e) => {
            let _ = lease.release();
            return Err(e);
        }
    };
    Ok(BaalGenericSharedBudgetV9 {
        inner: Rc::new(RefCell::new(BudgetInner {
            dir: dir.to_path_buf(),
            scope: scope.clone(),
            allocation: allocation.clone(),
            allocation_hash,
            budget_installed_hash: budget_installed_hash.to_string(),
            controller_code_identity_hash: controller_code_identity_hash.to_string(),
            controller_dir: controller_dir.to_path_buf(),
            source_runtime_dir: source_runtime_dir.to_path_buf(),
            custody_seed: custody_seed.clone(),
            file,
            actor_file,
            journal,
            state,
            closed: false,
            failed: false,
            size,
            bytes_hash,
            identity,
            lease,
            source_cache: RefCell::new(None),
            cache_hits: Cell::new(0),
            cache_misses: Cell::new(0),
            deps,
        })),
    })
}

impl BaalGenericSharedBudgetV9 {
    pub fn install_funding_reservation(&self, record: &V) -> Result<V, String> {
        let account_lease_hash = self.inner.borrow().basic()?;
        let s = self.inner.borrow().source()?;
        need(same(record, &s.record)?, "NOT_ACTUAL_CONTROLLER_RECORD")?;
        {
            let g = self.inner.borrow();
            if g.state.is_none() {
                need(
                    s.orders.is_empty()
                        && !s.unknown
                        && s.inheritance_hash == *get(&g.custody_seed, "seedHash")
                        && s.current_inventory_hash
                            == payload_hash(get(&g.custody_seed, "inventory"))?,
                    "EXACT_INHERITED_ACTOR_ONLY",
                )?;
            }
        }
        let g = self.inner.borrow().observed()?;
        let risk = {
            let b = self.inner.borrow();
            total_risk(&g, &s.record, &b.allocation, false)?
        };
        let external_unresolved: Vec<V> = get(&g, "unresolvedIntentIds")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|id| {
                !get(&s.record, "unresolvedIntentIds")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .contains(id)
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
        let at = ms_to_iso((self.inner.borrow().deps.now)());
        let next = {
            let b = self.inner.borrow();
            json!({
                "schemaVersion": 1,
                "mode": "baal_generic_shared_budget_v9",
                "scope": b.scope,
                "allocationHash": b.allocation_hash,
                "budgetInstalledHash": b.budget_installed_hash,
                "controllerCodeIdentityHash": b.controller_code_identity_hash,
                "controllerDir": b.controller_dir.to_string_lossy(),
                "controllerRootAnchor": s.opening,
                "actorAnchor": s.anchor,
                "actorProjection": {
                    "record": s.record,
                    "orders": s.orders,
                    "spent": s.spent,
                    "unknown": s.unknown,
                    "inheritanceHash": s.inheritance_hash,
                    "currentInventoryHash": s.current_inventory_hash,
                },
                "risk": risk,
                "externalObservation": g,
                "accountLeaseHash": account_lease_hash,
                "cumulativeActualExpenses": s.spent,
                "unknownAcceptance": s.unknown || get(&g, "unknownAcceptance") == &V::Bool(true),
                "at": at,
            })
        };
        append_event(&self.inner, &next)?;
        Ok(json!({
            "reservationHash": get(&next["actorProjection"]["record"], "reservationHash"),
            "allocationHash": self.inner.borrow().allocation_hash,
            "globalLedgerHash": self.inner.borrow().journal.previous,
            "confirmedAt": at,
        }))
    }

    pub fn global_risk(&self) -> Result<V, String> {
        self.inner.borrow().basic()?;
        need(self.inner.borrow().state.is_some(), "BUDGET_NOT_INSTALLED")?;
        let s = self.inner.borrow().source()?;
        let g = self.inner.borrow().observed()?;
        {
            let b = self.inner.borrow();
            let installed = get(&get(
                b.state.as_ref().unwrap(),
                "actorProjection",
            ), "record");
            need(
                get(&s.record, "reservationHash") == get(installed, "reservationHash"),
                "CURRENT_RESERVE_NOT_INSTALLED",
            )?;
        }
        let r = {
            let b = self.inner.borrow();
            total_risk(&g, &s.record, &b.allocation, true)?
        };
        let mut unresolved: Vec<V> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for id in get(&s.record, "unresolvedIntentIds")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .chain(
                get(&g, "unresolvedIntentIds")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter(),
            )
        {
            let key = js_stringify(id);
            if seen.insert(key) {
                unresolved.push(id.clone());
            }
        }
        let b = self.inner.borrow();
        let mut out = g.as_object().cloned().unwrap_or_default();
        for (k, v) in r.as_object().unwrap() {
            out.insert(k.clone(), v.clone());
        }
        out.insert(
            "installedCashLower".into(),
            get(
                &get(b.state.as_ref().unwrap(), "externalObservation"),
                "cashLower",
            )
            .clone(),
        );
        out.insert("allocationHash".into(), json!(b.allocation_hash));
        out.insert(
            "budgetInstalledHash".into(),
            json!(b.budget_installed_hash),
        );
        out.insert(
            "controllerReservationHash".into(),
            get(&s.record, "reservationHash").clone(),
        );
        out.insert("globalLedgerHash".into(), json!(b.journal.previous));
        out.insert(
            "sourceHash".into(),
            json!(payload_hash(&json!({
                "observation": get(&g, "sourceHash"),
                "actorAnchor": s.anchor,
                "reservation": get(&s.record, "reservationHash"),
                "globalLedgerHash": b.journal.previous,
            }))?),
        );
        let unknown_acceptance = s.unknown
            || get(&g, "unknownAcceptance") == &V::Bool(true)
            || ["owned", "reservations", "loans"].iter().any(|k| {
                get(&r, k)
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .any(|v| get(v, "unknownAcceptance") == &V::Bool(true))
            });
        out.insert("unknownAcceptance".into(), json!(unknown_acceptance));
        out.insert("unresolvedIntentIds".into(), json!(unresolved));
        out.insert("loansDiscounted".into(), json!(false));
        out.insert("executionAuthority".into(), json!(false));
        Ok(V::Object(out))
    }

    pub fn get_global_risk(&self) -> Result<V, String> {
        self.global_risk()
    }

    pub fn status(&self) -> V {
        let g = self.inner.borrow();
        let mut unknown = g
            .state
            .as_ref()
            .and_then(|st| st.get("unknownAcceptance"))
            .and_then(V::as_bool)
            .unwrap_or(false);
        if g.state.is_some() && !g.closed {
            match g.source() {
                Ok(s) => unknown = unknown || s.unknown,
                Err(_) => unknown = true,
            }
        }
        json!({
            "mode": "baal_generic_shared_budget_v9",
            "allocationHash": g.allocation_hash,
            "budgetInstalledHash": g.budget_installed_hash,
            "controllerCodeIdentityHash": g.controller_code_identity_hash,
            "claimed": g.state.is_some(),
            "globalLedgerHash": g.journal.previous,
            "sequence": g.journal.sequence,
            "controllerReservationHash": g.state.as_ref()
                .map(|st| get(&get(st, "actorProjection").get("record").cloned().unwrap_or(V::Null), "reservationHash").clone())
                .unwrap_or(V::Null),
            "cumulativeActualExpenses": g.state.as_ref()
                .map(|st| get(st, "cumulativeActualExpenses").clone())
                .unwrap_or(json!("0.000000")),
            "risk": g.state.as_ref().map(|st| get(st, "risk").clone()).unwrap_or(V::Null),
            "unknownAcceptance": unknown,
            "actorAnchor": g.state.as_ref().map(|st| get(st, "actorAnchor").clone()).unwrap_or(V::Null),
            "controllerRootAnchor": g.state.as_ref().map(|st| get(st, "controllerRootAnchor").clone()).unwrap_or(V::Null),
            "closed": g.closed,
            "failed": g.failed,
            "cacheHits": g.cache_hits.get(),
            "cacheMisses": g.cache_misses.get(),
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
        g.lease.release().map_err(|e| e.message)
    }
}
