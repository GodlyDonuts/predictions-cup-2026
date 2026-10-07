// execution/baal-scaled-no-evidence-bridge-v1.mjs — one decoder/evidence
// adapter over the calling collector's EXISTING API. Cached evidence is
// synchronous; callbacks never enqueue reads inside fences.
//
// Port notes:
// - `need` throws errors with code BAAL_SCALED_EVIDENCE_<code>. Errors from
//   foreign deps/helpers carry no `.code` in JS and are normalized to
//   BAAL_SCALED_EVIDENCE_READ_FAILED by `safeError` / recorded as such in
//   lastFailure. In Rust a foreign/dep error is marked codeless by wrapping
//   call sites with `codeless()` (NUL-prefixed string); `safe_error` maps any
//   marked message — or any message not matching the prefixed pattern — to
//   READ_FAILED. The one raw `throw Error('BAAL_SCALED_EVIDENCE_PAGES_BOUND')`
//   is likewise codeless in JS, so it is emitted marked here.
// - async -> synchronous closures; `sync()` -> dep Result + NULL is allowed
//   (JS checks !==undefined which a Rust dep cannot produce).
// - `issued` WeakMap keyed on spec.beforeDispatch -> pointer-keyed HashMap.
use crate::baal_scaled_entry_signal_v2::plan_baal_scaled_entry_v2 as plan_baal_scaled_entry;
use crate::core::{payload_hash, canonical_payload};
use crate::gates::normalize_official_fill_tick;
use crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::receipts::parse_number_lexemes;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::{amount, instant};
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use baal_service_runtime::market::normalize_book;
use serde_json::{json, Map, Value as V};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

const T: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("BAAL_SCALED_EVIDENCE_{code}"))
    }
}
fn codeless(e: String) -> String {
    format!("\u{0}{e}")
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
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty()
            && !s.starts_with('0')
            && s.bytes().all(|b| b.is_ascii_digit())
    })
}
fn sha(s: &str) -> String {
    sha256(s.as_bytes())
}
fn iso(n: f64) -> String {
    ms_to_iso(n)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(
        v.as_str()
            .ok_or_else(|| codeless("INVALID_INSTANT".to_string()))?,
    )
    .map_err(codeless)
}
fn str_amount(v: &V) -> Result<i128, String> {
    amount(
        v.as_str()
            .ok_or_else(|| codeless("INVALID".to_string()))?,
    )
    .map_err(codeless)
}
fn sorted(xs: &[V]) -> Vec<V> {
    let mut x: Vec<V> = xs.to_vec();
    x.sort_by(|a, b| {
        get(a, "id")
            .as_f64()
            .partial_cmp(&get(b, "id").as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    x
}
fn fresh(r: &V, n: i64, age: i64) -> Result<(), String> {
    need(
        instant_v(get(r, "requestStartedAt"))? <= instant_v(get(r, "receivedAt"))?
            && instant_v(get(r, "receivedAt"))? <= n
            && n - instant_v(get(r, "requestStartedAt"))? <= age,
        "SOURCE_CLOCK",
    )
}
fn exact_integer(parsed_numbers: &BTreeMap<String, String>, path: &[V], value: &V) -> Result<(), String> {
    let key = js_stringify(&json!(path));
    let raw = parsed_numbers.get(&key);
    let ok = raw.is_some_and(|raw| {
        raw.len() <= 128
            && {
                let t = raw.strip_prefix('-').unwrap_or(raw);
                let int_part = t.split('.').next().unwrap_or("");
                !int_part.is_empty()
                    && (!int_part.starts_with('0') || int_part == "0")
                    && int_part.bytes().all(|b| b.is_ascii_digit())
                    && raw
                        .split('.')
                        .nth(1)
                        .is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b == b'0'))
            }
            && js_int(value).is_some()
            && num_bigint::BigInt::parse_bytes(raw.split('.').next().unwrap_or("").as_bytes(), 10)
                == Some(num_bigint::BigInt::from(js_int(value).unwrap()))
    });
    need(ok, "EXACT_QUANTITY")
}
// safeError(e): coded errors keep their code (value = e.code ?? e.message
// matching /^(?:BAAL_SCALED_EVIDENCE|REST_NO|SCALED_PROFIT_EXIT)_[A-Z_]{1,120}$/);
// every other error becomes BAAL_SCALED_EVIDENCE_READ_FAILED.
fn safe_error(e: &str) -> String {
    let e = e.strip_prefix('\u{0}').unwrap_or(e);
    let prefix_ok = ["BAAL_SCALED_EVIDENCE_", "REST_NO_", "SCALED_PROFIT_EXIT_"]
        .iter()
        .any(|p| {
            e.starts_with(p)
                && (1..=120).contains(&e[p.len()..].len())
                && e[p.len()..]
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b == b'_')
        });
    if prefix_ok {
        e.to_string()
    } else {
        "BAAL_SCALED_EVIDENCE_READ_FAILED".to_string()
    }
}
fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

pub type Guard<'a> = dyn Fn(&V, &V) -> Result<bool, String> + 'a;
type BridgeDispatch =
    dyn for<'g> Fn(&V, Option<&'g Guard<'g>>, Option<&'g Guard<'g>>) -> Result<V, String>;

pub struct BaalGenericNoEvidenceBridgeV3Deps {
    pub dispatch: Rc<BridgeDispatch>,
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub get_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub get_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_account_risk: Option<Rc<dyn Fn() -> Result<V, String>>>,
    pub get_owned_census: Rc<dyn Fn(&str) -> Result<V, String>>,
    pub get_installed_budget: Rc<dyn Fn() -> Result<V, String>>,
    pub owns_order: Rc<dyn Fn(&str, &V) -> Result<bool, String>>,
    pub get_order_binding: Rc<dyn Fn(&str) -> Result<V, String>>,
    // getActiveIntent: JS default ()=>null
    pub get_active_intent: Option<Rc<dyn Fn() -> Result<V, String>>>,
    pub can_read: Rc<dyn Fn() -> bool>,
    // canCheckReadIdentity: JS default = canRead
    pub can_check_read_identity: Option<Rc<dyn Fn() -> bool>>,
    // refreshFundingAfterAccount: JS default null
    pub refresh_funding_after_account: Option<Rc<dyn Fn() -> Result<V, String>>>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> Result<bool, String>>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct Inner {
    scope: V,
    allocation: V,
    universe: V,
    allocation_hash: String,
    members: HashMap<String, V>,
    closed: bool,
    busy: bool,
    cache: Option<V>,
    reads: i64,
    bound: i64,
    operation: Option<String>,
    last_failure: Option<String>,
    last_end: Option<i64>,
    current_order_id: Option<String>,
    issued: HashMap<usize, V>,
    watermarks: HashMap<String, (i64, i64)>,
    projection_watermarks: HashMap<String, i64>,
    opening_lease: String,
    // resolved defaults: getActiveIntent()=>null, canCheckReadIdentity=canRead
    get_active_intent: Rc<dyn Fn() -> Result<V, String>>,
    can_check_read_identity: Rc<dyn Fn() -> bool>,
    deps: BaalGenericNoEvidenceBridgeV3Deps,
}

pub struct BaalGenericNoEvidenceBridgeV3 {
    inner: Rc<RefCell<Inner>>,
}

// createBaalGenericNoEvidenceBridgeV3({api,receiptOf,scope,allocation,universe,
//   getAccountLease,getGlobalRisk,getAccountRisk=getGlobalRisk,
//   getOwnedCensus,getInstalledBudget,ownsOrder,getOrderBinding,canRead,
//   hasReadHeadroom,now})
pub fn create_baal_generic_no_evidence_bridge_v3(
    scope: &V,
    allocation: &V,
    universe: &V,
    deps: BaalGenericNoEvidenceBridgeV3Deps,
) -> Result<BaalGenericNoEvidenceBridgeV3, String> {
    need(
        sget(scope, "ownerId") == Some("Codex")
            && sget(scope, "tournamentId") == Some(T)
            && sget(scope, "tournamentSlug") == Some("midterm-elections")
            && sget(scope, "runId").is_some_and(|s| !s.is_empty())
            && get(universe, "tournamentId").as_str() == Some(T)
            && get(universe, "exchanges").is_array()
            && get(allocation, "ownerId") == get(scope, "ownerId")
            && get(allocation, "runId") == get(scope, "runId")
            && get(allocation, "tournamentId").as_str() == Some(T)
            && get(allocation, "capital").as_str() == Some("1000.000000"),
        "SCOPE_ALLOCATION",
    )?;
    let allocation_hash = payload_hash(allocation).map_err(codeless)?;
    let mut members: HashMap<String, V> = HashMap::new();
    for m in get(universe, "exchanges").as_array().cloned().unwrap_or_default() {
        need(
            id(get(&m, "marketId"))
                && id(get(&m, "exchangeId"))
                && get(&m, "option").as_str() == Some("YES")
                && !members.contains_key(sget(&m, "exchangeId").unwrap_or("")),
            "UNIVERSE",
        )?;
        members.insert(
            sget(&m, "exchangeId").unwrap_or("").to_string(),
            json!({"marketId": get(&m, "marketId"), "exchangeId": get(&m, "exchangeId")}),
        );
    }
    let mut deps = deps;
    if deps.get_account_risk.is_none() {
        deps.get_account_risk = Some(deps.get_global_risk.clone());
    }
    let get_active_intent = deps
        .get_active_intent
        .clone()
        .unwrap_or_else(|| Rc::new(|| Ok(V::Null)));
    let can_check_read_identity = deps
        .can_check_read_identity
        .clone()
        .unwrap_or_else(|| deps.can_read.clone());
    let opening_lease = {
        let l = (deps.get_account_lease)().map_err(codeless)?;
        need(
            get(&l, "ownerId") == get(scope, "ownerId")
                && get(&l, "tournamentId").as_str() == Some(T)
                && hex(get(&l, "leaseId")),
            "CURRENT_OWNER",
        )?;
        payload_hash(&l).map_err(codeless)?
    };
    Ok(BaalGenericNoEvidenceBridgeV3 {
        inner: Rc::new(RefCell::new(Inner {
            scope: scope.clone(),
            allocation: allocation.clone(),
            universe: universe.clone(),
            allocation_hash,
            members,
            closed: false,
            busy: false,
            cache: None,
            reads: 0,
            bound: 0,
            operation: None,
            last_failure: None,
            last_end: None,
            current_order_id: None,
            issued: HashMap::new(),
            watermarks: HashMap::new(),
            projection_watermarks: HashMap::new(),
            opening_lease,
            get_active_intent,
            can_check_read_identity,
            deps,
        })),
    })
}

impl Inner {
    fn member(&self, x: &V) -> Result<V, String> {
        let m = sget(x, "exchangeId").and_then(|e| self.members.get(e));
        need(
            m.is_some() && sget(m.unwrap(), "marketId") == sget(x, "marketId"),
            "MARKET_SCOPE",
        )?;
        Ok(m.unwrap().clone())
    }
    fn lease(&self) -> Result<String, String> {
        let l = (self.deps.get_account_lease)().map_err(codeless)?;
        need(
            get(&l, "ownerId") == get(&self.scope, "ownerId")
                && get(&l, "tournamentId").as_str() == Some(T)
                && hex(get(&l, "leaseId")),
            "CURRENT_OWNER",
        )?;
        payload_hash(&l).map_err(codeless)
    }
    fn allowed(&self) -> Result<(), String> {
        need(
            !self.closed
                && (self.can_check_read_identity)()
                && self.lease()? == self.opening_lease,
            "STOPPED_OR_OWNER_CHANGED",
        )
    }
    fn order_binding(&self, order_id: &str, leg: &V) -> Result<V, String> {
        need(
            (self.deps.owns_order)(order_id, leg).map_err(codeless)? == true,
            "KNOWN_ORDER",
        )?;
        let o = (self.deps.get_order_binding)(order_id).map_err(codeless)?;
        let max_shares = match self.allocation.get("maximumSharesPerOrder") {
            Some(x) if !x.is_null() => js_int(x).unwrap_or(0),
            _ => 100,
        };
        let max_order = str_amount(&match self.allocation.get("maximumOrderCost") {
            Some(x) if !x.is_null() => x.clone(),
            _ => json!("25.000000"),
        })?;
        need(
            sget(&o, "orderId") == Some(order_id)
                && get(&o, "marketId") == get(leg, "marketId")
                && get(&o, "exchangeId") == get(leg, "exchangeId")
                && sget(&o, "intentId").is_some_and(|s| !s.is_empty())
                && get(get(&o, "body"), "tournamentId").as_str() == Some(T)
                && get(get(&o, "body"), "exchangeId") == get(leg, "exchangeId")
                && get(get(&o, "body"), "side").as_str() == Some("no")
                && ["buy", "sell"]
                    .contains(&sget(get(&o, "body"), "action").unwrap_or(""))
                && js_int(get(get(&o, "body"), "quantity"))
                    .is_some_and(|q| q >= 1 && q <= max_shares)
                && get(&o, "bodyHash").as_str()
                    == Some(payload_hash(get(&o, "body")).map_err(codeless)?.as_str())
                && str_amount(get(&normalize_official_fill_tick(get(get(&o, "body"), "price"))?, "price"))?
                    * js_int(get(get(&o, "body"), "quantity")).unwrap_or(0) as i128
                    <= max_order,
            "ORDER_BODY_SCOPE",
        )?;
        Ok(o)
    }
    fn global(&self, intent_id: Option<&V>) -> Result<V, String> {
        need((self.deps.can_read)(), "STOPPED")?;
        let g = (self.deps.get_global_risk)().map_err(codeless)?;
        let order_id: Option<String> = self
            .current_order_id
            .clone()
            .or_else(|| {
                self.cache
                    .as_ref()
                    .and_then(|c| c.get("orderId"))
                    .and_then(V::as_str)
                    .map(|s| s.to_string())
            });
        let leg: Option<V> = self
            .cache
            .as_ref()
            .and_then(|c| c.get("legs"))
            .and_then(|l| l.as_array())
            .and_then(|a| a.first())
            .cloned()
            .or_else(|| {
                order_id.as_ref().and_then(|oid| {
                    (self.deps.get_order_binding)(oid)
                        .ok()
                        .and_then(|b| self.member(&b).ok())
                })
            });
        let own: Option<V> = match (&order_id, &leg) {
            (Some(oid), Some(l)) => Some(self.order_binding(oid, l)?),
            _ => None,
        };
        let installed = (self.deps.get_installed_budget)().map_err(codeless)?;
        let local_ids: Vec<V> = installed
            .get("record")
            .and_then(|r| r.get("unresolvedIntentIds"))
            .and_then(V::as_array)
            .cloned()
            .unwrap_or_default();
        let context_id: V = match intent_id {
            Some(v) if !v.is_null() => v.clone(),
            _ => {
                if local_ids.len() == 1 {
                    local_ids[0].clone()
                } else {
                    V::Null
                }
            }
        };
        let context_own = context_id.is_string() && local_ids.contains(&context_id);
        let own_intent = own.as_ref().map(|o| get(o, "intentId").clone()).unwrap_or(V::Null);
        let all_bound = get(&g, "unresolvedIntentIds")
            .as_array()
            .is_some_and(|ids| {
                ids.iter().all(|id| {
                    id == &own_intent
                        || (context_own && id == &context_id)
                })
            });
        let unknown_ok = get(&g, "unknownAcceptance") != &V::Bool(true)
            || (own.is_some()
                && get(&g, "unresolvedIntentIds")
                    .as_array()
                    .is_some_and(|ids| ids.iter().all(|id| id == &own_intent)))
            || (context_own
                && get(&g, "unresolvedIntentIds")
                    .as_array()
                    .is_some_and(|ids| ids.iter().all(|id| id == &context_id)));
        let n = (self.deps.now)() as i64;
        need(
            get(&g, "ownerId") == get(&self.scope, "ownerId")
                && get(&g, "runId") == get(&self.scope, "runId")
                && get(&g, "tournamentId").as_str() == Some(T)
                && get(&g, "complete") == &V::Bool(true)
                && get(&g, "soleCollector") == &V::Bool(true)
                && get(&g, "stopped") == &V::Bool(false)
                && get(&g, "unresolvedIntentIds").as_array().is_some_and(|x| x.len() <= 1)
                && all_bound
                && unknown_ok
                && hex(get(&g, "sourceHash"))
                && instant_v(get(&g, "at"))? <= n
                && n - instant_v(get(&g, "at"))? <= 20000,
            "GLOBAL_UNKNOWN",
        )?;
        Ok(g)
    }
    fn budget(&self, required: bool, intent_id: Option<&V>) -> Result<Option<V>, String> {
        let value = (self.deps.get_installed_budget)().map_err(codeless)?;
        if value.is_null() {
            need(!required, "INSTALLED_BUDGET_REQUIRED")?;
            return Ok(None);
        }
        let record = get(&value, "record");
        let receipt = get(&value, "receipt");
        let mut body = record.as_object().cloned().unwrap_or_default();
        let reservation_hash = body.remove("reservationHash").unwrap_or(V::Null);
        let iid_ok = |required: bool| -> bool {
            !required
                || get(record, "unresolvedIntentIds")
                    .as_array()
                    .is_some_and(|ids| match intent_id {
                        Some(iid) => ids.iter().all(|id| id == iid),
                        None => ids.is_empty(),
                    })
        };
        need(
            hex(&reservation_hash)
                && payload_hash(&V::Object(body)).map_err(codeless)?
                    == reservation_hash.as_str().unwrap_or("")
                && get(record, "schemaVersion") == &json!(1)
                && get(record, "kind").as_str()
                    == Some("baal_scaled_no_full_nominal_reservation_v1")
                && get(record, "ownerId") == get(&self.scope, "ownerId")
                && get(record, "runId") == get(&self.scope, "runId")
                && get(record, "tournamentId").as_str() == Some(T)
                && get(record, "allocationHash").as_str()
                    == Some(self.allocation_hash.as_str())
                && get(record, "owned").is_array()
                && get(record, "reservations").is_array()
                && get(record, "unresolvedIntentIds").is_array()
                && iid_ok(required),
            "BUDGET_BINDING",
        )?;
        let mut owned: i128 = 0;
        let mut reserved: i128 = 0;
        let mut by_market: HashMap<String, i128> = HashMap::new();
        let mut by_event: HashMap<String, i128> = HashMap::new();
        let mut seen: HashSet<String> = HashSet::new();
        for (list, field) in [
            ("owned", "nominalCost"),
            ("reservations", "maximumNominalCost"),
        ] {
            for row in get(record, list).as_array().cloned().unwrap_or_default() {
                let eid = sget(&row, "exposureId");
                need(
                    eid.is_some_and(|s| !s.is_empty())
                        && !seen.contains(eid.unwrap())
                        && id(get(&row, "marketId"))
                        && sget(&row, "eventId").is_some_and(|s| !s.is_empty()),
                    "EXPOSURE_BINDING",
                )?;
                seen.insert(eid.unwrap().to_string());
                let n = str_amount(get(&row, field))?;
                need(n >= 0, "NONNEGATIVE_NOMINAL")?;
                if field == "nominalCost" {
                    owned += n;
                } else {
                    reserved += n;
                }
                *by_market
                    .entry(sget(&row, "marketId").unwrap_or("").to_string())
                    .or_insert(0) += n;
                *by_event
                    .entry(sget(&row, "eventId").unwrap_or("").to_string())
                    .or_insert(0) += n;
            }
        }
        let max_market = str_amount(&match self.allocation.get("maximumMarketCost") {
            Some(x) if !x.is_null() => x.clone(),
            _ => json!("100.000000"),
        })?;
        let max_event = str_amount(&match self.allocation.get("maximumEventCost") {
            Some(x) if !x.is_null() => x.clone(),
            _ => json!("200.000000"),
        })?;
        need(
            str_amount(get(record, "ownedPrincipal"))? == owned
                && str_amount(get(record, "cashReserved"))? == reserved
                && owned + reserved <= 1000000000
                && by_market.values().all(|v| *v <= max_market)
                && by_event.values().all(|v| *v <= max_event),
            "GLOBAL_NOMINAL_CAPS",
        )?;
        let now = (self.deps.now)() as i64;
        need(
            *get(receipt, "reservationHash") == reservation_hash
                && get(receipt, "allocationHash").as_str() == Some(self.allocation_hash.as_str())
                && hex(get(receipt, "globalLedgerHash"))
                && instant_v(get(receipt, "confirmedAt"))? <= now
                && (!required || now - instant_v(get(receipt, "confirmedAt"))? <= 20000),
            "BUDGET_RECEIPT",
        )?;
        Ok(Some(value))
    }
    fn account(&self, raw: &V, leg: &V, n: i64) -> Result<V, String> {
        let parsed = parse_number_lexemes(
            get(get(raw, "positions"), "bodyText")
                .as_str()
                .ok_or_else(|| "BAAL_SCALED_EVIDENCE_POSITIONS".to_string())?,
        )
        .map_err(codeless)?;
        let positions = get(&parsed.0, "positions")
            .as_array()
            .cloned()
            .unwrap_or_default();
        need(get(&parsed.0, "positions").is_array(), "POSITIONS")?;
        for (i, p) in positions.iter().enumerate() {
            exact_integer(
                &parsed.1,
                &[json!("positions"), json!(i), json!("quantity")],
                get(p, "quantity"),
            )?;
        }
        let scoped = get(raw, "scopedOrdersPagesByExchange")
            .get(sget(leg, "exchangeId").unwrap_or(""))
            .filter(|v| !v.is_null())
            .cloned()
            .unwrap_or_else(|| get(raw, "scopedOrdersPages").clone());
        let full = json!({
            "cash": get(raw, "cash"),
            "positions": get(raw, "positions"),
            "openOrdersPages": get(raw, "openOrdersPages"),
            "scopedOrdersPages": scoped,
        });
        let mut binding = self.scope.as_object().cloned().unwrap_or_default();
        for (k, v) in leg.as_object().cloned().unwrap_or_default() {
            binding.insert(k, v);
        }
        let view = inspect_scoped_rest_no_account(&json!({
            "account": full,
            "binding": V::Object(binding),
            "readAt": iso(n as f64),
        }))
        .map_err(codeless)?;
        let mut receipts: Vec<V> = Vec::new();
        receipts.push(get(&full, "cash").clone());
        receipts.push(get(&full, "positions").clone());
        receipts.extend(
            get(&full, "openOrdersPages")
                .as_array()
                .cloned()
                .unwrap_or_default(),
        );
        receipts.extend(
            get(&full, "scopedOrdersPages")
                .as_array()
                .cloned()
                .unwrap_or_default(),
        );
        let source_receipts: Vec<V> = receipts
            .iter()
            .map(|r| {
                json!({
                    "method": get(r, "method"),
                    "endpoint": get(r, "endpoint"),
                    "requestStartedAt": get(r, "requestStartedAt"),
                    "receivedAt": get(r, "receivedAt"),
                    "sourceBodyHash": get(r, "sourceBodyHash"),
                    "fetchInvocationId": r.get("fetchInvocationId").cloned().unwrap_or(V::Null),
                })
            })
            .collect();
        let mut out = view.as_object().cloned().unwrap_or_default();
        out.insert("sourceReceipts".into(), json!(source_receipts));
        Ok(V::Object(out))
    }
    fn inventory(&self, raw: &V, leg: &V, n: i64, required: bool) -> Result<Option<V>, String> {
        let view = self.account(raw, leg, n)?;
        let p = get(&view, "positions")
            .as_array()
            .and_then(|ps| {
                ps.iter()
                    .find(|p| sget(p, "exchangeId") == sget(leg, "exchangeId"))
            })
            .cloned();
        let owned = (self.deps.get_owned_census)(sget(leg, "exchangeId").unwrap_or(""))
            .map_err(codeless)?;
        if owned.is_null() {
            need(
                !required
                    && p.as_ref()
                        .and_then(|p| js_int(get(p, "quantity")))
                        .unwrap_or(0)
                        == 0,
                "FOREIGN_POSITION",
            )?;
            return Ok(None);
        }
        let proof = get(&owned, "proof");
        let mut proof_body = proof.as_object().cloned().unwrap_or_default();
        let vh = proof_body.remove("verificationHash").unwrap_or(V::Null);
        need(
            hex(&vh)
                && payload_hash(&V::Object(proof_body)).map_err(codeless)?
                    == vh.as_str().unwrap_or("")
                && get(proof, "mode").as_str() == Some("scoped_rest_no_multishare_fifo_v1")
                && get(proof, "scopedTradeVerified") == &V::Bool(true)
                && get(proof, "ownerId") == get(&self.scope, "ownerId")
                && get(proof, "runId") == get(&self.scope, "runId")
                && get(proof, "tournamentId").as_str() == Some(T)
                && get(proof, "marketId") == get(leg, "marketId")
                && get(proof, "exchangeId") == get(leg, "exchangeId"),
            "OWNED_PROOF",
        )?;
        need(
            instant_v(get(proof, "readAt"))? <= instant_v(get(&view, "startedAt"))?,
            "OWNED_PROOF_FUTURE",
        )?;
        let old = self.account(get(&owned, "after"), leg, instant_v(get(proof, "readAt"))?)?;
        need(
            payload_hash(get(&old, "sourceReceipts")).map_err(codeless)?
                == payload_hash(get(get(proof, "sourceReceipts"), "after")).map_err(codeless)?
                && payload_hash(&json!(sorted(
                    get(&old, "scopedOrders").as_array().cloned().unwrap_or_default().as_slice()
                )))
                .map_err(codeless)?
                    == payload_hash(&json!(sorted(
                        get(&view, "scopedOrders")
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                            .as_slice()
                    )))
                    .map_err(codeless)?,
            "OWNED_HISTORY_OR_SOURCE",
        )?;
        need(
            p.as_ref().and_then(|p| js_int(get(p, "quantity"))).unwrap_or(0)
                == -js_int(get(proof, "remainingOwnedQuantity")).unwrap_or(0)
                && (p.is_none()
                    || (get(p.as_ref().unwrap(), "marketId") == get(leg, "marketId")
                        && get(p.as_ref().unwrap(), "settled") != &V::Bool(true)))
                && (!required
                    || js_int(get(proof, "remainingOwnedQuantity")).unwrap_or(0) > 0),
            "OWNED_QUANTITY",
        )?;
        Ok(Some(owned))
    }
    fn book(&self, r: &V, leg: &V, n: i64) -> Result<V, String> {
        fresh(r, n, 20000)?;
        let parsed = parse_number_lexemes(sget(r, "bodyText").unwrap_or("")).map_err(codeless)?;
        let d = parsed.0.clone();
        let contexts = get(&d, "contexts").as_array().cloned().unwrap_or_default();
        need(
            (!d.as_object().map(|m| m.contains_key("marketId")).unwrap_or(false)
                || get(&d, "marketId") == get(leg, "marketId"))
                && get(&d, "contexts").as_array().is_some_and(|c| c.len() == 1)
                && get(&contexts[0], "type").as_str() == Some("tournament")
                && get(&get(&contexts[0], "tournament"), "id").as_str() == Some(T)
                && get(&get(&contexts[0], "orderbook"), "exchanges")
                    .as_array()
                    .is_some_and(|x| x.len() == 1)
                && get(
                    &get(&get(&contexts[0], "orderbook"), "exchanges")
                        .as_array()
                        .unwrap()[0],
                    "exchangeId",
                )
                .as_str()
                    == sget(leg, "exchangeId"),
            "BOOK_SCOPE",
        )?;
        for name in ["bids", "asks"] {
            for (i, l) in get(
                &get(&get(&contexts[0], "orderbook"), "exchanges")
                    .as_array()
                    .unwrap()[0],
                name,
            )
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .enumerate()
            {
                exact_integer(
                    &parsed.1,
                    &[
                        json!("contexts"),
                        json!(0),
                        json!("orderbook"),
                        json!("exchanges"),
                        json!(0),
                        json!(name),
                        json!(i),
                        json!("quantity"),
                    ],
                    get(l, "quantity"),
                )?;
            }
        }
        normalize_book(
            &d,
            sget(leg, "marketId").unwrap_or(""),
            T,
            sget(r, "receivedAt").unwrap_or(""),
            sget(r, "requestStartedAt").unwrap_or(""),
            20000.0,
            &BTreeSet::new(),
            "authoritative_receipt_v1",
        )
        .map_err(codeless)
    }

    // none(c,n): 5s-fresh final-NONE pair receipt (graph + collateral).
    fn none(&self, c: &V, n: i64) -> Result<V, String> {
        need(
            !get(c, "graphReceipt").is_null() && !get(c, "collateralReceipt").is_null(),
            "NONE_REQUIRED",
        )?;
        for r in [get(c, "graphReceipt"), get(c, "collateralReceipt")] {
            fresh(r, n, 5000)?;
            need(
                sget(r, "method") == Some("GET")
                    && js_int(get(r, "status")) == Some(200)
                    && get(r, "requestBodyHash") == &V::Null
                    && sget(r, "sourceBodyHash")
                        == Some(sha(sget(r, "bodyText").unwrap_or("")).as_str()),
                "NONE_RAW_SOURCE",
            )?;
        }
        let g = parse_number_lexemes(sget(get(c, "graphReceipt"), "bodyText").unwrap_or(""))
            .map_err(codeless)?
            .0;
        let v = parse_number_lexemes(sget(get(c, "collateralReceipt"), "bodyText").unwrap_or(""))
            .map_err(codeless)?
            .0;
        need(
            sget(get(c, "collateralReceipt"), "endpoint")
                == Some(format!("/portfolio/collateral?tournamentId={T}").as_str())
                && sget(get(c, "graphReceipt"), "endpoint")
                    == Some(
                        format!(
                            "/relationships/graph?marketId={}&depth=4&tournamentId={T}",
                            sget(c, "graphMarketId").unwrap_or("")
                        )
                        .as_str(),
                    ),
            "NONE_ENDPOINTS",
        )?;
        let mut gkeys: Vec<&str> = g
            .as_object()
            .map(|m| m.keys().map(|k| k.as_str()).collect())
            .unwrap_or_default();
        gkeys.sort_unstable();
        let vkeys: Vec<&str> = v
            .as_object()
            .map(|m| m.keys().map(|k| k.as_str()).collect())
            .unwrap_or_default();
        need(
            g.is_object()
                && gkeys == ["nodes", "relationships"]
                && get(&g, "relationships").as_array().is_some_and(|x| x.is_empty())
                && get(&g, "nodes").as_array().is_some_and(|xs| {
                    xs.len() <= 2
                        && xs.iter().all(|x| {
                            get(c, "legs").as_array().is_some_and(|ls| {
                                ls.iter().any(|l| {
                                    sget(x, "marketId") == sget(l, "marketId")
                                        && sget(x, "exchangeId") == sget(l, "exchangeId")
                                })
                            })
                        })
                }),
            "NON_NEUTRAL_GRAPH",
        )?;
        need(
            v.is_object()
                && vkeys == ["data"]
                && get(&v, "data").as_array().is_some_and(|x| x.is_empty()),
            "NON_NEUTRAL_COLLATERAL",
        )?;
        need(
            instant_v(get(get(c, "collateralReceipt"), "requestStartedAt"))?
                >= instant_v(get(get(c, "graphReceipt"), "receivedAt"))?,
            "NONE_CAUSAL",
        )?;
        Ok(json!({
            "graphReceipt": get(c, "graphReceipt"),
            "collateralReceipt": get(c, "collateralReceipt"),
        }))
    }

}

impl BaalGenericNoEvidenceBridgeV3 {
    fn allowed(inner: &Rc<RefCell<Inner>>) -> Result<(), String> {
        inner.borrow().allowed()
    }

    // getEvidence(context)
    pub fn get_evidence(&self, context: &V) -> Result<V, String> {
        let run = || -> Result<V, String> {
            Self::allowed(&self.inner)?;
            let (cache, now_ms) = {
                let g = self.inner.borrow();
                need(!g.busy && g.cache.is_some(), "CACHE_UNAVAILABLE")?;
                (g.cache.clone().unwrap(), (g.deps.now)())
            };
            if let Some(oid) = context.get("orderId").filter(|v| !v.is_null()) {
                need(get(&cache, "orderId") == oid, "CACHE_ORDER")?;
            }
            if let Some(ex) = sget(context, "exchangeId") {
                need(
                    get(&cache, "legs")
                        .as_array()
                        .is_some_and(|ls| ls.iter().any(|l| sget(l, "exchangeId") == Some(ex))),
                    "CACHE_SCOPE",
                )?;
            }
            let g = self
                .inner
                .borrow()
                .global(context.get("intentId").filter(|v| !v.is_null()))?;
            let n = now_ms as i64;
            let kind = sget(&cache, "kind").unwrap_or("");
            if kind == "buy" {
                need(
                    ["prepare_buy", "place_buy"]
                        .contains(&sget(context, "operation").unwrap_or("")),
                    "CACHE_OPERATION",
                )?;
                self.inner.borrow().budget(
                    sget(context, "operation") == Some("place_buy"),
                    context.get("intentId").filter(|v| !v.is_null()),
                )?;
                let legs0 = get(&cache, "legs").as_array().cloned().unwrap_or_default();
                let a = self
                    .inner
                    .borrow()
                    .account(get(&cache, "account"), &legs0[0], n)?;
                need(
                    !get(&a, "positions")
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .any(|p| {
                            legs0.iter().any(|l| {
                                sget(l, "exchangeId") == sget(p, "exchangeId")
                            }) && js_int(get(p, "quantity")).unwrap_or(0) != 0
                        }),
                    "BUY_TARGET_NOT_FLAT",
                )?;
                for b in get(&cache, "bookReceipts").as_array().cloned().unwrap_or_default() {
                    self.inner.borrow().book(get(&b, "receipt"), &b, n)?;
                }
                for r in get(&cache, "rootReceipts").as_array().cloned().unwrap_or_default() {
                    fresh(get(&r, "receipt"), n, 300000)?;
                }
                let mut out = json!({
                    "account": get(&cache, "account"),
                    "rootReceipts": get(&cache, "rootReceipts"),
                    "bookReceipts": get(&cache, "bookReceipts"),
                    "accountRisk": get(&cache, "accountRisk"),
                    "currentGlobalRisk": g,
                });
                for (k, v) in self
                    .inner
                    .borrow()
                    .none(&cache, n)?
                    .as_object()
                    .cloned()
                    .unwrap_or_default()
                {
                    out[k] = v;
                }
                return Ok(out);
            }
            if kind == "sell" {
                need(
                    ["prepare_sell", "place_sell"]
                        .contains(&sget(context, "operation").unwrap_or("")),
                    "CACHE_OPERATION",
                )?;
                self.inner.borrow().budget(
                    sget(context, "operation") == Some("place_sell"),
                    context.get("intentId").filter(|v| !v.is_null()),
                )?;
                let leg = get(&cache, "legs").as_array().cloned().unwrap_or_default()[0].clone();
                self.inner
                    .borrow()
                    .inventory(get(&cache, "account"), &leg, n, true)?;
                self.inner.borrow().book(get(&cache, "book"), &leg, n)?;
                let mut out = json!({
                    "account": get(&cache, "account"),
                    "book": get(&cache, "book"),
                    "globalRisk": g,
                    "currentGlobalRisk": g,
                });
                for (k, v) in self
                    .inner
                    .borrow()
                    .none(&cache, n)?
                    .as_object()
                    .cloned()
                    .unwrap_or_default()
                {
                    out[k] = v;
                }
                return Ok(out);
            }
            need(
                ["reconcile", "cancel"].contains(&sget(context, "operation").unwrap_or(""))
                    && get(context, "operation").as_str() == get(&cache, "kind").as_str(),
                "KNOWN_ORDER",
            )?;
            let leg = get(&cache, "legs").as_array().cloned().unwrap_or_default()[0].clone();
            self.inner.borrow().order_binding(
                sget(&cache, "orderId").unwrap_or(""),
                &leg,
            )?;
            fresh(get(&cache, "order"), n, 20000)?;
            if kind == "cancel" {
                return Ok(json!({"order": get(&cache, "order"), "globalRisk": g}));
            }
            for r in get(&cache, "fillPages").as_array().cloned().unwrap_or_default() {
                fresh(&r, n, 20000)?;
            }
            self.inner.borrow().account(get(&cache, "after"), &leg, n)?;
            let mut out = json!({
                "order": get(&cache, "order"),
                "fillPages": get(&cache, "fillPages"),
                "after": get(&cache, "after"),
                "globalRisk": g,
            });
            for (k, v) in self
                .inner
                .borrow()
                .none(&cache, n)?
                .as_object()
                .cloned()
                .unwrap_or_default()
            {
                out[k] = v;
            }
            Ok(out)
        };
        match run() {
            Ok(v) => Ok(v),
            Err(e) => {
                let mut g2 = self.inner.borrow_mut();
                g2.cache = None;
                g2.last_failure = Some(safe_error(&e));
                Err(safe_error(&e))
            }
        }
    }

    // authorizes(spec, beforeDispatch, beforeNetwork)
    pub fn authorizes(&self, spec: &V, before_dispatch: &Guard, before_network: &Guard) -> bool {
        let r: Result<bool, String> = (|| {
            Self::allowed(&self.inner)?;
            let g = self.inner.borrow();
            let key = before_dispatch as *const Guard as *const () as usize;
            let p = g.issued.get(&key).cloned();
            Ok(p.is_some()
                && sget(spec, "method") == Some("GET")
                && get(spec, "endpoint") == get(p.as_ref().unwrap(), "endpoint")
                && (before_network as *const Guard as *const () as usize)
                    == get(p.as_ref().unwrap(), "beforeNetworkPtr")
                        .as_u64()
                        .unwrap_or(0) as usize
                && spec.get("body").is_none()
                && spec.get("bodyJson").is_none()
                && spec.as_object().is_some_and(|m| {
                    m.keys().all(|k| {
                        ["method", "endpoint", "body", "bodyJson", "beforeDispatch", "beforeNetwork"]
                            .contains(&k.as_str())
                    })
                }))
        })();
        r.unwrap_or(false)
    }

    // read(endpoint)
    fn read(&self, endpoint: &str) -> Result<V, String> {
        {
            let g = self.inner.borrow();
            g.allowed()?;
            need(g.reads < g.bound, "READ_BOUND")?;
            need(
                (g.deps.has_read_headroom)(g.bound - g.reads).map_err(codeless)?,
                "HEADROOM",
            )?;
        }
        let admitted = Cell::new(false);
        let networked = Cell::new(false);
        let inner_a = self.inner.clone();
        let admitted_a = &admitted;
        let before_dispatch = move |_v: &V, _s: &V| -> Result<bool, String> {
            let g = inner_a.borrow();
            g.allowed()?;
            g.global(None)?;
            need(
                (g.deps.has_read_headroom)(g.bound - g.reads).map_err(codeless)?,
                "HEADROOM",
            )?;
            need(!admitted_a.get(), "DUPLICATE_ADMISSION")?;
            admitted_a.set(true);
            Ok(true)
        };
        let inner_b = self.inner.clone();
        let admitted_b = &admitted;
        let networked_b = &networked;
        let before_network = move |_v: &V, _s: &V| -> Result<bool, String> {
            let g = inner_b.borrow();
            g.allowed()?;
            g.global(None)?;
            need(admitted_b.get(), "NOT_ADMITTED")?;
            need(
                (g.deps.has_read_headroom)(g.bound - g.reads - 1).map_err(codeless)?,
                "HEADROOM",
            )?;
            networked_b.set(true);
            Ok(true)
        };
        let bd_ptr = &before_dispatch as &Guard as *const Guard as *const () as usize;
        let bn_ptr = &before_network as &Guard as *const Guard as *const () as usize;
        let spec = json!({"method": "GET", "endpoint": endpoint});
        self.inner.borrow_mut().issued.insert(
            bd_ptr,
            json!({"endpoint": endpoint, "beforeNetworkPtr": bn_ptr as u64}),
        );
        let at = (self.inner.borrow().deps.now)() as i64;
        let result: Result<V, String> = (|| {
            let response = (self.inner.borrow().deps.dispatch)(
                &spec,
                Some(&before_dispatch as &Guard),
                Some(&before_network as &Guard),
            )
            .map_err(codeless)?;
            Self::allowed(&self.inner)?;
            need(
                admitted.get()
                    && networked.get()
                    && get(&response, "endpoint").as_str() == Some(endpoint)
                    && js_int(get(&response, "status")) == Some(200),
                "RESPONSE",
            )?;
            let r = (self.inner.borrow().deps.receipt_of)(
                &response,
                &json!({"method": "GET", "requestBodyHash": V::Null}),
            )
            .map_err(codeless)?;
            need(
                sget(&r, "method") == Some("GET")
                    && sget(&r, "endpoint") == Some(endpoint)
                    && js_int(get(&r, "status")) == Some(200)
                    && get(&r, "requestBodyHash") == &V::Null
                    && sget(&r, "sourceBodyHash")
                        == Some(sha(sget(&r, "bodyText").unwrap_or("")).as_str())
                    && sget(&r, "fetchInvocationId").is_some_and(|s| !s.is_empty()),
                "RAW_SOURCE",
            )?;
            let now2 = (self.inner.borrow().deps.now)() as i64;
            fresh(&r, now2, if endpoint.contains("/nodes?") { 300000 } else { 20000 })?;
            need(
                instant_v(get(&r, "requestStartedAt"))? >= at
                    && instant_v(get(&r, "requestStartedAt"))?
                        >= instant_v(get(&response, "requestStartedAt"))?
                    && instant_v(get(&r, "receivedAt"))?
                        <= instant_v(get(&response, "receivedAt"))?
                    && (self.inner.borrow().last_end.is_none()
                        || instant_v(get(&r, "requestStartedAt"))?
                            >= self.inner.borrow().last_end.unwrap())
                    && payload_hash(
                        &parse_number_lexemes(sget(&r, "bodyText").unwrap_or(""))
                            .map_err(codeless)?
                            .0,
                    )
                    .map_err(codeless)?
                        == payload_hash(get(&response, "data")).map_err(codeless)?,
                "SOURCE_BINDING",
            )?;
            let previous = self.inner.borrow().watermarks.get(endpoint).cloned();
            if let Some((pstart, pend)) = previous {
                need(
                    instant_v(get(&r, "requestStartedAt"))? >= pstart
                        && instant_v(get(&r, "receivedAt"))? >= pend,
                    "SOURCE_ROLLBACK",
                )?;
            }
            self.inner.borrow_mut().watermarks.insert(
                endpoint.to_string(),
                (
                    instant_v(get(&r, "requestStartedAt"))?,
                    instant_v(get(&r, "receivedAt"))?,
                ),
            );
            self.inner.borrow_mut().last_end = Some(instant_v(get(&r, "receivedAt"))?);
            Ok(r)
        })();
        {
            let mut g = self.inner.borrow_mut();
            g.reads += 1;
            g.issued.remove(&bd_ptr);
        }
        result
    }

    // pages(endpoint)
    fn pages(&self, endpoint: &str) -> Result<Vec<V>, String> {
        let mut cursor: Option<String> = None;
        let mut frontier: Option<i64> = self
            .inner
            .borrow()
            .projection_watermarks
            .get(endpoint)
            .cloned();
        let mut result: Vec<V> = Vec::new();
        let mut cursors: HashSet<String> = HashSet::new();
        let mut ids: HashSet<i64> = HashSet::new();
        for _ in 0..10 {
            let url = match &cursor {
                None => endpoint.to_string(),
                Some(c) => format!("{}&cursor={}", endpoint, encode_uri_component(c)),
            };
            let r = self.read(&url)?;
            let d = parse_number_lexemes(sget(&r, "bodyText").unwrap_or(""))
                .map_err(codeless)?
                .0;
            let p = get(&d, "pagination");
            let projected = get(get(&d, "coverage"), "projectedThroughSequence");
            need(
                get(&d, "data").as_array().is_some_and(|a| a.len() <= 200)
                    && js_int(get(p, "limit")) == Some(200)
                    && get(p, "hasMore").is_boolean()
                    && get(get(&d, "coverage"), "complete") == &V::Bool(true)
                    && (projected.is_null() || js_int(projected).is_some_and(|x| x >= 0)),
                "COMPLETE_PAGES",
            )?;
            let current: Option<i64> = if projected.is_null() {
                None
            } else {
                js_int(projected)
            };
            need(
                current.is_none() || frontier.is_none() || current.unwrap() >= frontier.unwrap(),
                "PROJECTION_ROLLBACK",
            )?;
            frontier = current.or(frontier);
            for row in get(&d, "data").as_array().cloned().unwrap_or_default() {
                need(
                    js_int(get(&row, "id")).is_some_and(|i| i > 0) && !ids.contains(&js_int(get(&row, "id")).unwrap_or(0)),
                    "DUPLICATE_ROW",
                )?;
                ids.insert(js_int(get(&row, "id")).unwrap());
            }
            result.push(r);
            if get(p, "hasMore") == &V::Bool(false) {
                need(get(p, "nextCursor") == &V::Null, "CURSOR")?;
                if let Some(f) = frontier {
                    self.inner
                        .borrow_mut()
                        .projection_watermarks
                        .insert(endpoint.to_string(), f);
                }
                return Ok(result);
            }
            need(
                sget(p, "nextCursor")
                    .is_some_and(|c| !c.is_empty() && c.len() <= 1000 && !cursors.contains(c)),
                "CURSOR",
            )?;
            cursors.insert(sget(p, "nextCursor").unwrap().to_string());
            cursor = Some(sget(p, "nextCursor").unwrap().to_string());
        }
        // JS throws a bare Error (no .code) — marked codeless so the caller's
        // safeError maps it to BAAL_SCALED_EVIDENCE_READ_FAILED.
        Err(codeless("BAAL_SCALED_EVIDENCE_PAGES_BOUND".to_string()))
    }

    // collectNone(marketId): graph read then collateral read.
    fn collect_none(&self, market_id: &str) -> Result<V, String> {
        let graph_receipt = self.read(&format!(
            "/relationships/graph?marketId={market_id}&depth=4&tournamentId={T}"
        ))?;
        let collateral_receipt =
            self.read(&format!("/portfolio/collateral?tournamentId={T}"))?;
        Ok(json!({
            "graphMarketId": market_id,
            "graphReceipt": graph_receipt,
            "collateralReceipt": collateral_receipt,
        }))
    }

    // collectAccount(legs)
    fn collect_account(&self, legs: &[V]) -> Result<V, String> {
        let slug = sget(&self.inner.borrow().scope.clone(), "tournamentSlug")
            .unwrap_or("")
            .to_string();
        let slug = slug.as_str();
        let cash = self.read(&format!("/tournaments/{slug}"))?;
        let positions = self.read(&format!("/tournaments/{slug}/portfolio/positions"))?;
        let open_orders_pages = self.pages(&format!(
            "/orders?status=open&limit=200&tournamentId={T}"
        ))?;
        let mut by_ex = Map::new();
        for l in legs {
            let ex = sget(l, "exchangeId").unwrap_or("");
            let ps = self.pages(&format!(
                "/orders?status=all&exchangeId={ex}&limit=200&tournamentId={T}"
            ))?;
            by_ex.insert(ex.to_string(), json!(ps));
        }
        let mut out = Map::new();
        out.insert("cash".into(), cash);
        out.insert("positions".into(), positions);
        out.insert("openOrdersPages".into(), json!(open_orders_pages));
        out.insert("scopedOrdersPagesByExchange".into(), V::Object(by_ex));
        if legs.len() == 1 {
            let ex = sget(&legs[0], "exchangeId").unwrap_or("");
            out.insert(
                "scopedOrdersPages".into(),
                get(&out["scopedOrdersPagesByExchange"], ex).clone(),
            );
        }
        Ok(V::Object(out))
    }

    // run(kind,legs,orderId,maximum,fn)
    fn run(
        &self,
        kind: &str,
        legs: &[V],
        order_id: Option<&str>,
        maximum: i64,
        f: impl FnOnce(&Self) -> Result<V, String>,
    ) -> Result<V, String> {
        need(!self.inner.borrow().busy, "BUSY")?;
        Self::allowed(&self.inner)?;
        {
            let mut g = self.inner.borrow_mut();
            g.cache = None;
            g.busy = true;
            g.operation = Some(kind.to_string());
            g.current_order_id = order_id.map(|s| s.to_string());
            g.reads = 0;
            g.bound = maximum;
            g.last_end = None;
            g.last_failure = None;
        }
        let result = (|| -> Result<V, String> {
            need(
                (self.inner.borrow().deps.has_read_headroom)(maximum).map_err(codeless)?,
                "HEADROOM",
            )?;
            if let Some(oid) = order_id {
                need(id(&json!(oid)), "KNOWN_ORDER")?;
                self.inner.borrow().order_binding(oid, &legs[0])?;
            }
            self.inner.borrow().global(None)?;
            let result = f(self)?;
            Self::allowed(&self.inner)?;
            let mut cache = result.as_object().cloned().unwrap_or_default();
            cache.insert("kind".into(), json!(kind));
            cache.insert("legs".into(), json!(legs));
            cache.insert("orderId".into(), json!(order_id));
            self.inner.borrow_mut().cache = Some(V::Object(cache));
            Ok(json!({
                "kind": kind,
                "readCount": self.inner.borrow().reads,
                "completedAt": iso((self.inner.borrow().deps.now)()),
            }))
        })();
        let result = match result {
            Ok(v) => Ok(v),
            Err(e) => {
                let mut g = self.inner.borrow_mut();
                g.cache = None;
                g.last_failure = Some(safe_error(&e));
                Err(safe_error(&e))
            }
        };
        {
            let mut g = self.inner.borrow_mut();
            g.busy = false;
            g.operation = None;
            g.current_order_id = None;
        }
        result
    }

    // refreshBuy(pair)
    pub fn refresh_buy(&self, pair: &V) -> Result<V, String> {
        need(
            pair.is_array() && pair.as_array().unwrap().len() == 2,
            "PAIR",
        )?;
        let legs: Vec<V> = pair
            .as_array()
            .unwrap()
            .iter()
            .map(|m| self.inner.borrow().member(m))
            .collect::<Result<_, _>>()?;
        need(
            legs.iter()
                .map(|l| sget(l, "exchangeId").unwrap_or(""))
                .collect::<BTreeSet<_>>()
                .len()
                == 2
                && legs
                    .iter()
                    .map(|l| sget(l, "marketId").unwrap_or(""))
                    .collect::<BTreeSet<_>>()
                    .len()
                    == 2,
            "DISTINCT_PAIR",
        )?;
        let legs_c = legs.clone();
        self.run("buy", &legs, None, 10, move |this| {
            let mut root_receipts: Vec<V> = Vec::new();
            for l in &legs_c {
                let r = this.read(&format!(
                    "/markets/{}/nodes?tournamentId={T}",
                    sget(l, "marketId").unwrap_or("")
                ))?;
                let mut entry = l.as_object().cloned().unwrap_or_default();
                entry.insert("receipt".into(), r);
                root_receipts.push(V::Object(entry));
            }
            let account = this.collect_account(&legs_c[..1])?;
            let l0 = &legs_c[0];
            let scoped = get(&account, "scopedOrdersPagesByExchange")
                .get(sget(l0, "exchangeId").unwrap_or(""))
                .cloned()
                .unwrap_or(V::Null);
            let mut binding = this
                .inner
                .borrow()
                .scope
                .as_object()
                .cloned()
                .unwrap_or_default();
            for (k, v) in l0.as_object().cloned().unwrap_or_default() {
                binding.insert(k, v);
            }
            let a = inspect_scoped_rest_no_account(&json!({
                "account": {
                    "cash": get(&account, "cash"),
                    "positions": get(&account, "positions"),
                    "openOrdersPages": get(&account, "openOrdersPages"),
                    "scopedOrdersPages": scoped,
                },
                "binding": V::Object(binding),
                "readAt": iso((this.inner.borrow().deps.now)()),
            }))
            .map_err(codeless)?;
            need(
                !get(&a, "positions")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .any(|p| {
                        legs_c.iter().any(|l| {
                            sget(l, "exchangeId") == sget(p, "exchangeId")
                        }) && js_int(get(p, "quantity")).unwrap_or(0) != 0
                    }),
                "BUY_TARGET_NOT_FLAT",
            )?;
            let funding = this
                .inner
                .borrow()
                .deps
                .refresh_funding_after_account
                .clone();
            if let Some(f) = funding {
                f().map_err(codeless)?;
            }
            let account_risk = (this.inner.borrow().deps.get_account_risk.as_ref().unwrap().clone())()
                .map_err(codeless)?;
            need(
                instant_v(get(&account_risk, "at"))? >= instant_v(get(&a, "completedAt"))?,
                "ACCOUNT_RISK_BEFORE_SOURCE",
            )?;
            let mut book_receipts: Vec<V> = Vec::new();
            for l in &legs_c {
                let r = this.read(&format!(
                    "/markets/{}/orderbook?tournamentId={T}&depth=20",
                    sget(l, "marketId").unwrap_or("")
                ))?;
                let n2 = (this.inner.borrow().deps.now)() as i64;
                this.inner.borrow().book(&r, l, n2)?;
                need(
                    instant_v(get(&account_risk, "at"))? <= instant_v(get(&r, "requestStartedAt"))?,
                    "RISK_BEFORE_BOOK",
                )?;
                let mut entry = l.as_object().cloned().unwrap_or_default();
                entry.insert("receipt".into(), r);
                book_receipts.push(V::Object(entry));
            }
            // CHEAP_FIRST: order legs by descending top bid, then ascending exchangeId.
            let top_bid = |m: &V| -> Result<i128, String> {
                let parsed = parse_number_lexemes(
                    sget(get(m, "receipt"), "bodyText").unwrap_or(""),
                )
                .map_err(codeless)?;
                let ctx = get(&parsed.0, "contexts")
                    .as_array()
                    .and_then(|a| a.first())
                    .cloned()
                    .unwrap_or(V::Null);
                let ex = get(get(&ctx, "orderbook"), "exchanges")
                    .as_array()
                    .and_then(|a| a.first())
                    .cloned()
                    .unwrap_or(V::Null);
                let bid = get(&ex, "bids")
                    .as_array()
                    .and_then(|a| a.first())
                    .cloned()
                    .unwrap_or(V::Null);
                str_amount(get(
                    &normalize_official_fill_tick(get(&bid, "price"))?,
                    "price",
                ))
            };
            let mut ordered = book_receipts.clone();
            let mut sort_err: Option<String> = None;
            ordered.sort_by(|x, y| {
                let mut top = |m: &V| -> i128 {
                    match top_bid(m) {
                        Ok(v) => v,
                        Err(e) => {
                            sort_err = Some(e);
                            i128::MIN
                        }
                    }
                };
                let (ta, tb) = (top(x), top(y));
                tb.cmp(&ta).then_with(|| {
                    sget(x, "exchangeId")
                        .unwrap_or("")
                        .parse::<i64>()
                        .unwrap_or(0)
                        .cmp(&sget(y, "exchangeId").unwrap_or("").parse::<i64>().unwrap_or(0))
                })
            });
            if let Some(e) = sort_err {
                return Err(e);
            }
            need(
                ordered
                    .first()
                    .and_then(|b| sget(b, "exchangeId"))
                    == sget(&legs_c[0], "exchangeId"),
                "CHEAP_FIRST_CHANGED_REQUIRES_NEW_PACKET",
            )?;
            let proposal = plan_baal_scaled_entry(&json!({
                "scope": this.inner.borrow().scope.clone(),
                "allocation": this.inner.borrow().allocation.clone(),
                "account": account,
                "rootReceipts": root_receipts,
                "bookReceipts": book_receipts,
                "accountRisk": account_risk,
                "now": iso((this.inner.borrow().deps.now)()),
            }));
            if get(&proposal, "proposalReady") != &V::Bool(true) {
                return Err(format!(
                    "BAAL_SCALED_EVIDENCE_ENTRY_{}",
                    sget(&proposal, "reason").unwrap_or("")
                ));
            }
            let chosen = sget(get(&proposal, "chosenLeg"), "marketId")
                .unwrap_or("")
                .to_string();
            let none_part = this.collect_none(&chosen)?;
            let mut out = json!({
                "account": account,
                "rootReceipts": root_receipts,
                "bookReceipts": book_receipts,
                "accountRisk": account_risk,
            });
            for (k, v) in none_part.as_object().cloned().unwrap_or_default() {
                out[k] = v;
            }
            Ok(out)
        })
    }

    // refreshSell(leg)
    pub fn refresh_sell(&self, leg: &V) -> Result<V, String> {
        let leg = self.inner.borrow().member(leg)?;
        let leg_c = leg.clone();
        self.run("sell", &[leg.clone()], None, 7, move |this| {
            let raw = this.collect_account(&[leg_c.clone()])?;
            let n = (this.inner.borrow().deps.now)() as i64;
            let current = this
                .inner
                .borrow()
                .inventory(&raw, &leg_c, n, true)?;
            let book_receipt = this.read(&format!(
                "/markets/{}/orderbook?tournamentId={T}&depth=20",
                sget(&leg_c, "marketId").unwrap_or("")
            ))?;
            let n2 = (this.inner.borrow().deps.now)() as i64;
            this.inner.borrow().book(&book_receipt, &leg_c, n2)?;
            let n3 = (this.inner.borrow().deps.now)() as i64;
            let acc = this.inner.borrow().account(&raw, &leg_c, n3)?;
            need(
                instant_v(get(&book_receipt, "requestStartedAt"))?
                    >= instant_v(get(&acc, "completedAt"))?,
                "ACCOUNT_BEFORE_BOOK",
            )?;
            let mut out = json!({
                "account": {
                    "cash": get(&raw, "cash"),
                    "positions": get(&raw, "positions"),
                    "openOrdersPages": get(&raw, "openOrdersPages"),
                    "scopedOrdersPages": get(&raw, "scopedOrdersPages"),
                },
                "book": book_receipt,
                "ownedProofHash": get(get(&current.unwrap_or(V::Null), "proof"), "verificationHash"),
            });
            for (k, v) in this
                .collect_none(sget(&leg_c, "marketId").unwrap_or(""))?
                .as_object()
                .cloned()
                .unwrap_or_default()
            {
                out[k] = v;
            }
            Ok(out)
        })
    }

    // refreshReconciliation(orderId,leg)
    pub fn refresh_reconciliation(&self, order_id: &str, leg: &V) -> Result<V, String> {
        let leg = self.inner.borrow().member(leg)?;
        let leg_c = leg.clone();
        let oid = order_id.to_string();
        self.run("reconcile", &[leg.clone()], Some(order_id), 10, move |this| {
            let expected = this.inner.borrow().order_binding(&oid, &leg_c)?;
            let order = this.read(&format!("/orders/{oid}"))?;
            let parsed = parse_number_lexemes(sget(&order, "bodyText").unwrap_or(""))
                .map_err(codeless)?;
            let d = parsed.0.clone();
            need(
                get(&d, "id").as_f64() == oid.parse::<f64>().ok()
                    && get(&d, "exchangeId") == get(&leg_c, "exchangeId")
                    && get(&d, "tournamentId").as_str() == Some(T)
                    && get(&d, "side").as_str() == Some("no")
                    && get(&d, "action") == get(get(&expected, "body"), "action")
                    && js_int(get(&d, "quantity")).is_some_and(|q| {
                        q > 0 && q <= js_int(get(get(&expected, "body"), "quantity")).unwrap_or(0)
                    })
                    && get(&d, "expirationDate") == get(get(&expected, "body"), "expirationDate")
                    && get(&normalize_official_fill_tick(get(&d, "priceLimit"))?, "price")
                        == get(
                            &normalize_official_fill_tick(get(get(&expected, "body"), "price"))?,
                            "price",
                        )
                    && get(&d, "open") == &V::Bool(false),
                "TERMINAL_ORDER",
            )?;
            for k in ["id", "quantity", "quantityFilled"] {
                exact_integer(&parsed.1, &[json!(k)], get(&d, k))?;
            }
            need(
                js_int(get(&d, "quantityFilled")).is_some_and(|q| {
                    q >= 0 && q <= js_int(get(&d, "quantity")).unwrap_or(i64::MIN)
                }),
                "TERMINAL_FILLED_QUANTITY",
            )?;
            let fill_pages = this.pages(&format!("/orders/{oid}/fills?limit=200"))?;
            let mut total: Option<i64> = None;
            let mut seen: HashSet<V> = HashSet::new();
            let mut filled: i64 = 0;
            for r in &fill_pages {
                let p = parse_number_lexemes(sget(r, "bodyText").unwrap_or(""))
                    .map_err(codeless)?;
                let v = p.0.clone();
                need(
                    get(&v, "orderId").as_f64() == oid.parse::<f64>().ok()
                        && get(&v, "exchangeId") == get(&leg_c, "exchangeId")
                        && get(&v, "tournamentId").as_str() == Some(T)
                        && js_int(get(&v, "totalQuantityFilled")).is_some_and(|t| {
                            t <= 0
                                && t >= -js_int(get(get(&expected, "body"), "quantity"))
                                    .unwrap_or(0)
                        }),
                    "FILL_SCOPE",
                )?;
                exact_integer(&p.1, &[json!("orderId")], get(&v, "orderId"))?;
                exact_integer(
                    &p.1,
                    &[json!("totalQuantityFilled")],
                    get(&v, "totalQuantityFilled"),
                )?;
                need(
                    total.is_none()
                        || total == js_int(get(&v, "totalQuantityFilled")),
                    "FILL_TOTAL_CHANGED",
                )?;
                total = js_int(get(&v, "totalQuantityFilled"));
                for (i, f) in get(&v, "data").as_array().cloned().unwrap_or_default().iter().enumerate() {
                    exact_integer(
                        &p.1,
                        &[json!("data"), json!(i), json!("quantity")],
                        get(f, "quantity"),
                    )?;
                    need(
                        sget(f, "side") == Some("no")
                            && js_int(get(f, "quantity")).is_some_and(|q| q < 0)
                            && !seen.contains(f.get("id").unwrap_or(&V::Null))
                            && instant_v(get(f, "filledAt"))?
                                >= instant_v(get(&d, "createdAt"))?
                            && instant_v(get(f, "filledAt"))?
                                <= instant_v(get(r, "receivedAt"))?,
                        "FILL_IDENTITY",
                    )?;
                    seen.insert(f.get("id").cloned().unwrap_or(V::Null));
                    filled -= js_int(get(f, "quantity")).unwrap_or(0);
                    let px = str_amount(get(
                        &normalize_official_fill_tick(get(f, "price"))?,
                        "price",
                    ))?;
                    let limit = str_amount(get(
                        &normalize_official_fill_tick(get(get(&expected, "body"), "price"))?,
                        "price",
                    ))?;
                    need(
                        if sget(get(&expected, "body"), "action") == Some("buy") {
                            px <= limit
                        } else {
                            px >= limit
                        },
                        "FILL_LIMIT",
                    )?;
                }
            }
            need(
                filled == js_int(get(&d, "quantityFilled")).unwrap_or(i64::MIN)
                    && total == Some(-filled),
                "COMPLETE_FILL_QUANTITY",
            )?;
            let raw = this.collect_account(&[leg_c.clone()])?;
            need(
                instant_v(get(get(&raw, "cash"), "requestStartedAt"))?
                    >= instant_v(get(fill_pages.last().unwrap_or(&V::Null), "receivedAt"))?,
                "AFTER_ACCOUNT_CAUSALITY",
            )?;
            let mut out = json!({
                "order": order,
                "fillPages": fill_pages,
                "after": {
                    "cash": get(&raw, "cash"),
                    "positions": get(&raw, "positions"),
                    "openOrdersPages": get(&raw, "openOrdersPages"),
                    "scopedOrdersPages": get(&raw, "scopedOrdersPages"),
                },
            });
            for (k, v) in this
                .collect_none(sget(&leg_c, "marketId").unwrap_or(""))?
                .as_object()
                .cloned()
                .unwrap_or_default()
            {
                out[k] = v;
            }
            Ok(out)
        })
    }

    // refreshFinalNone(): re-read the NONE pair for a prepared-but-unfetched intent.
    pub fn refresh_final_none(&self) -> Result<V, String> {
        {
            let g = self.inner.borrow();
            let kind_ok = g
                .cache
                .as_ref()
                .and_then(|c| sget(c, "kind"))
                .is_some_and(|k| ["buy", "sell"].contains(&k));
            need(!g.busy && g.cache.is_some() && kind_ok, "PREPARED_CACHE")?;
        }
        let active = (self.inner.borrow().get_active_intent)().map_err(codeless)?;
        need(
            sget(&active, "phase") == Some("prepared")
                && get(&active, "orderId") == &V::Null
                && js_int(get(&active, "actualFetchCount")) == Some(0)
                && sget(&active, "bodyHash")
                    == Some(payload_hash(get(&active, "body")).map_err(codeless)?.as_str()),
            "EXACT_UNFETCHED_INTENT",
        )?;
        need(
            instant_v(get(get(&active, "body"), "expirationDate"))?
                > (self.inner.borrow().deps.now)() as i64,
            "EXACT_UNFETCHED_INTENT",
        )?;
        let saved = self.inner.borrow().cache.clone().unwrap_or(V::Null);
        let pin = payload_hash(&json!({
            "account": get(&saved, "account"),
            "rootReceipts": get(&saved, "rootReceipts"),
            "bookReceipts": get(&saved, "bookReceipts"),
            "book": get(&saved, "book"),
            "accountRisk": get(&saved, "accountRisk"),
        }))
        .map_err(codeless)?;
        let body_hash = sget(&active, "bodyHash").unwrap_or("").to_string();
        let market_id = sget(&active, "marketId").unwrap_or("").to_string();
        let kind = sget(&saved, "kind").unwrap_or("").to_string();
        let legs = get(&saved, "legs").as_array().cloned().unwrap_or_default();
        let saved_c = saved.clone();
        self.run(&kind, &legs, None, 2, move |this| {
            let mut out = saved_c.as_object().cloned().unwrap_or_default();
            for (k, v) in this
                .collect_none(&market_id)?
                .as_object()
                .cloned()
                .unwrap_or_default()
            {
                out.insert(k, v);
            }
            Ok(V::Object(out))
        })?;
        let current = (self.inner.borrow().get_active_intent)().map_err(codeless)?;
        need(
            sget(&current, "bodyHash") == Some(body_hash.as_str())
                && payload_hash(get(&current, "body")).map_err(codeless)? == body_hash
                && sget(&current, "phase") == Some("prepared")
                && js_int(get(&current, "actualFetchCount")) == Some(0),
            "FIXED_BODY_CHANGED",
        )?;
        let cache_now = self.inner.borrow().cache.clone().unwrap_or(V::Null);
        need(
            pin
                == payload_hash(&json!({
                    "account": get(&cache_now, "account"),
                    "rootReceipts": get(&cache_now, "rootReceipts"),
                    "bookReceipts": get(&cache_now, "bookReceipts"),
                    "book": get(&cache_now, "book"),
                    "accountRisk": get(&cache_now, "accountRisk"),
                }))
                .map_err(codeless)?,
            "ORIGINAL_SOURCES_CHANGED",
        )?;
        self.get_evidence(&json!({
            "operation": if kind == "buy" { "place_buy" } else { "place_sell" },
            "intentId": get(&current, "intentId"),
            "exchangeId": get(get(&current, "body"), "exchangeId"),
        }))
    }

    // refreshCancel(orderId,leg)
    pub fn refresh_cancel(&self, order_id: &str, leg: &V) -> Result<V, String> {
        let leg = self.inner.borrow().member(leg)?;
        let leg_c = leg.clone();
        let oid = order_id.to_string();
        self.run("cancel", &[leg.clone()], Some(order_id), 1, move |this| {
            let expected = this.inner.borrow().order_binding(&oid, &leg_c)?;
            let order = this.read(&format!("/orders/{oid}"))?;
            let parsed = parse_number_lexemes(sget(&order, "bodyText").unwrap_or(""))
                .map_err(codeless)?;
            let d = parsed.0.clone();
            need(
                get(&d, "id").as_f64() == oid.parse::<f64>().ok()
                    && get(&d, "exchangeId") == get(&leg_c, "exchangeId")
                    && get(&d, "tournamentId").as_str() == Some(T)
                    && get(&d, "side").as_str() == Some("no")
                    && get(&d, "action") == get(get(&expected, "body"), "action")
                    && js_int(get(&d, "quantity")).is_some_and(|q| {
                        q > 0 && q <= js_int(get(get(&expected, "body"), "quantity")).unwrap_or(0)
                    })
                    && get(&d, "expirationDate") == get(get(&expected, "body"), "expirationDate")
                    && get(&normalize_official_fill_tick(get(&d, "priceLimit"))?, "price")
                        == get(
                            &normalize_official_fill_tick(get(get(&expected, "body"), "price"))?,
                            "price",
                        )
                    && get(&d, "open") == &V::Bool(true),
                "OPEN_OWNED_ORDER",
            )?;
            exact_integer(&parsed.1, &[json!("id")], get(&d, "id"))?;
            exact_integer(&parsed.1, &[json!("quantity")], get(&d, "quantity"))?;
            Ok(json!({"order": order}))
        })
    }

    // getBuyProposal()
    pub fn get_buy_proposal(&self) -> Result<V, String> {
        let evidence = self.get_evidence(&json!({"operation": "prepare_buy"}))?;
        let mut input = evidence.as_object().cloned().unwrap_or_default();
        input.insert("allocation".into(), self.inner.borrow().allocation.clone());
        input.insert("scope".into(), self.inner.borrow().scope.clone());
        input.insert("now".into(), json!(iso((self.inner.borrow().deps.now)())));
        Ok(plan_baal_scaled_entry(&V::Object(input)))
    }

    // status()
    pub fn status(&self) -> V {
        let g = self.inner.borrow();
        json!({
            "mode": "baal_generic_no_evidence_bridge_v3",
            "busy": g.busy,
            "closed": g.closed,
            "lastReadCount": g.reads,
            "maximumReads": g.bound,
            "cachedKind": g.cache.as_ref().map(|c| get(c, "kind").clone()).unwrap_or(V::Null),
            "lastFailure": g.last_failure.clone().map(|s| json!(s)).unwrap_or(V::Null),
            "additionalApiClients": 0,
            "executionAuthority": false,
        })
    }

    pub fn clear(&self) {
        self.inner.borrow_mut().cache = None;
    }

    pub fn close(&self) -> Result<(), String> {
        need(!self.inner.borrow().busy, "BUSY")?;
        let mut g = self.inner.borrow_mut();
        g.cache = None;
        g.closed = true;
        g.watermarks.clear();
        Ok(())
    }
}
