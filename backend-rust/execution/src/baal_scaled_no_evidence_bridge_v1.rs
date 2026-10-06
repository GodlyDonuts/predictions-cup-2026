// execution/baal-scaled-no-evidence-bridge-v1.mjs — decoder/evidence adapter
// over the calling collector's EXISTING API. Cached evidence is synchronous;
// callbacks never enqueue reads inside fences.
//
// Port notes:
// - `api.dispatch(spec)` -> synchronous `Rc<dyn Fn(&V, Option<&Guard>,
//   Option<&Guard>) -> Result<V, String>>`; the JS spec.beforeDispatch /
//   beforeNetwork function fields are the guard arguments, and `issued` keys
//   them by guard pointer (usize) like the controller ports.
// - `sync(fn, ...)` is trivially satisfied: all callbacks are synchronous.
// - `safeError` normalizes thrown codes to /^BAAL_SCALED_EVIDENCE_[A-Z_]+$/ or
//   READ_FAILED.
use crate::baal_scaled_entry_signal_v1::plan_baal_scaled_entry;
use crate::core::payload_hash;
use crate::gates::normalize_official_fill_tick;
use crate::model::normalize_book;
use crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::receipts::parse_number_lexemes;
use crate::transport::Guard;
use baal_service_runtime::fixed::{amount, instant};
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

const T: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("BAAL_SCALED_EVIDENCE_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn id(x: &V) -> bool {
    // /^[1-9]\d*$/
    x.as_str().is_some_and(|s| {
        s.len() >= 1 && s.as_bytes()[0] != b'0' && s.bytes().all(|b| b.is_ascii_digit())
    })
}
fn iso(n: f64) -> String {
    baal_service_runtime::contract::ms_to_iso(n)
}
fn sorted(xs: &[V]) -> Vec<V> {
    let mut v = xs.to_vec();
    v.sort_by_key(|x| js_int(get(x, "id")).unwrap_or(0));
    v
}
// String(v) — JS primitive-to-string coercion.
fn js_string_repr(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(_) | V::Bool(_) => js_stringify(v),
        V::Null => "null".to_string(),
        _ => "undefined".to_string(),
    }
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn fresh(r: &V, n: i64, age: i64) -> Result<(), String> {
    need(
        instant_v(get(r, "requestStartedAt"))? <= instant_v(get(r, "receivedAt"))?
            && instant_v(get(r, "receivedAt"))? <= n
            && n - instant_v(get(r, "requestStartedAt"))? <= age,
        "SOURCE_CLOCK",
    )
}
fn path_key(path: &[V]) -> String {
    js_stringify(&V::Array(path.to_vec()))
}
// exactInteger(parsed, path, value)
fn exact_integer(
    numbers: &std::collections::BTreeMap<String, String>,
    path: &[V],
    value: i64,
) -> Result<(), String> {
    let raw = numbers.get(&path_key(path));
    let ok = raw.is_some_and(|s| {
        s.len() <= 128 && {
            let neg = s.starts_with('-');
            let rest = if neg { &s[1..] } else { s.as_str() };
            let mut parts = rest.splitn(2, '.');
            let whole = parts.next().unwrap_or("");
            let frac = parts.next();
            (whole == "0" || (!whole.is_empty() && !whole.starts_with('0')))
                && whole.bytes().all(|b| b.is_ascii_digit())
                && match frac {
                    None => true,
                    Some(f) => !f.is_empty() && f.bytes().all(|b| b == b'0'),
                }
        }
    }) && raw
        .and_then(|s| s.split('.').next())
        .and_then(|s| s.parse::<BigInt>().ok())
        == Some(BigInt::from(value));
    need(ok, "EXACT_QUANTITY")
}
fn safe_error(e: &str) -> String {
    if e.len() > 4
        && e.starts_with("BAAL_SCALED_EVIDENCE_")
        && e[21..]
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b == b'_')
    {
        e.to_string()
    } else {
        "BAAL_SCALED_EVIDENCE_READ_FAILED".to_string()
    }
}

pub struct BaalScaledNoEvidenceBridgeDeps {
    /// api.dispatch — synchronous GET admission with before/after guards.
    pub api_dispatch: Rc<dyn for<'g> Fn(&V, Option<&'g Guard<'g>>, Option<&'g Guard<'g>>) -> Result<V, String>>,
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub get_account_lease: Rc<dyn Fn() -> V>,
    pub get_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_account_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_owned_census: Rc<dyn Fn(&str) -> Option<V>>,
    pub get_installed_budget: Rc<dyn Fn() -> Option<V>>,
    pub owns_order: Rc<dyn Fn(&str, &V) -> Result<bool, String>>,
    pub get_order_binding: Rc<dyn Fn(&str) -> Result<V, String>>,
    pub can_read: Rc<dyn Fn() -> bool>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> bool>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct BridgeInner {
    scope: V,
    allocation: V,
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
    opening_lease: String,
    issued: HashMap<usize, V>,
    watermarks: HashMap<String, (i64, i64)>,
    projection_watermarks: HashMap<String, i64>,
    deps: BaalScaledNoEvidenceBridgeDeps,
}

pub struct BaalScaledNoEvidenceBridge {
    inner: Rc<RefCell<BridgeInner>>,
}

impl BaalScaledNoEvidenceBridge {
    fn member(inner: &Rc<RefCell<BridgeInner>>, x: &V) -> Result<V, String> {
        let g = inner.borrow();
        let m = g
            .members
            .get(get(x, "exchangeId").as_str().unwrap_or(""))
            .cloned();
        need(m.is_some() && get(m.as_ref().unwrap(), "marketId") == get(x, "marketId"), "MARKET_SCOPE")?;
        Ok(m.unwrap())
    }
    fn lease(inner: &Rc<RefCell<BridgeInner>>) -> Result<String, String> {
        let f = inner.borrow().deps.get_account_lease.clone();
        let l = f();
        let owner = inner.borrow().scope.get("ownerId").cloned().unwrap_or(V::Null);
        need(
            get(&l, "ownerId") == &owner
                && get(&l, "tournamentId").as_str() == Some(T)
                && hex(get(&l, "leaseId")),
            "CURRENT_OWNER",
        )?;
        Ok(payload_hash(&l)?)
    }
    fn allowed(inner: &Rc<RefCell<BridgeInner>>) -> Result<(), String> {
        let (closed, can_read, opening) = {
            let g = inner.borrow();
            (g.closed, g.deps.can_read.clone(), g.opening_lease.clone())
        };
        need(
            !closed && can_read() && Self::lease(inner)? == opening,
            "STOPPED_OR_OWNER_CHANGED",
        )
    }
    fn order_binding(inner: &Rc<RefCell<BridgeInner>>, order_id: &str, leg: &V) -> Result<V, String> {
        let (owns, get_binding, allocation) = {
            let g = inner.borrow();
            (
                g.deps.owns_order.clone(),
                g.deps.get_order_binding.clone(),
                g.allocation.clone(),
            )
        };
        need(owns(order_id, leg)? == true, "KNOWN_ORDER")?;
        let o = get_binding(order_id)?;
        need(
            get(&o, "orderId").as_str() == Some(order_id)
                && get(&o, "marketId") == get(leg, "marketId")
                && get(&o, "exchangeId") == get(leg, "exchangeId")
                && get(&o, "intentId").as_str().is_some_and(|s| !s.is_empty())
                && get(get(&o, "body"), "tournamentId").as_str() == Some(T)
                && get(get(&o, "body"), "exchangeId") == get(leg, "exchangeId")
                && get(get(&o, "body"), "side").as_str() == Some("no")
                && ["buy", "sell"].contains(&get(get(&o, "body"), "action").as_str().unwrap_or(""))
                && js_int(get(get(&o, "body"), "quantity"))
                    .is_some_and(|q| q >= 1 && q <= {
                        let cap = get(&allocation, "maximumSharesPerOrder");
                        if cap.is_null() { 100 } else { js_int(cap).unwrap_or(0) }
                    })
                && get(&o, "bodyHash").as_str() == Some(payload_hash(get(&o, "body"))?.as_str())
                && amount(
                    normalize_official_fill_tick(get(get(&o, "body"), "price"))
                        .map_err(|e| e)?
                        .get("price")
                        .and_then(V::as_str)
                        .unwrap_or(""),
                )
                .map(BigInt::from)?
                    * js_int(get(get(&o, "body"), "quantity")).map(BigInt::from).unwrap_or_default()
                    <= amount({
                        let c = get(&allocation, "maximumOrderCost");
                        if c.is_null() {
                            "25.000000"
                        } else {
                            c.as_str().unwrap_or("")
                        }
                    })
                    .map(BigInt::from)?,
            "ORDER_BODY_SCOPE",
        )?;
        Ok(o)
    }
    fn global(inner: &Rc<RefCell<BridgeInner>>, intent_id: Option<&V>) -> Result<V, String> {
        let g = (inner.borrow().deps.get_global_risk)()?;
        let (order_id, legs0, cache_order) = {
            let g2 = inner.borrow();
            let order_id = g2.current_order_id.clone().or_else(|| {
                g2.cache
                    .as_ref()
                    .and_then(|c| get(c, "orderId").as_str().map(String::from))
            });
            let legs0 = g2
                .cache
                .as_ref()
                .and_then(|c| get(c, "legs").as_array().and_then(|a| a.first()).cloned());
            (order_id, legs0, ())
        };
        let _ = cache_order;
        let leg = match legs0 {
            Some(l) => Some(l),
            None => match &order_id {
                Some(id) => {
                    let o = (inner.borrow().deps.get_order_binding)(id)?;
                    Some(Self::member(inner, &o)?)
                }
                None => None,
            },
        };
        let own = match (&order_id, &leg) {
            (Some(id), Some(leg)) => Some(Self::order_binding(inner, id, leg)?),
            _ => None,
        };
        let installed = (inner.borrow().deps.get_installed_budget)();
        let local_ids: Vec<V> = installed
            .as_ref()
            .and_then(|v| get(get(v, "record"), "unresolvedIntentIds").as_array())
            .cloned()
            .unwrap_or_default();
        let context_id = match intent_id {
            Some(v) => v.clone(),
            None => local_ids.first().cloned().filter(|_| local_ids.len() == 1).unwrap_or(V::Null),
        };
        let context_own = context_id.is_string() && local_ids.iter().any(|id| id == &context_id);
        let own_intent = own
            .as_ref()
            .map(|o| get(o, "intentId").clone())
            .unwrap_or(V::Null);
        let unresolved = get(&g, "unresolvedIntentIds").as_array().cloned().unwrap_or_default();
        need(
            get(&g, "ownerId") == get(&inner.borrow().scope, "ownerId")
                && get(&g, "runId") == get(&inner.borrow().scope, "runId")
                && get(&g, "tournamentId").as_str() == Some(T)
                && get(&g, "complete") == &json!(true)
                && get(&g, "soleCollector") == &json!(true)
                && get(&g, "stopped") == &json!(false)
                && get(&g, "unresolvedIntentIds").is_array()
                && unresolved.len() <= 1
                && unresolved
                    .iter()
                    .all(|id| id == &own_intent || (context_own && id == &context_id))
                && (get(&g, "unknownAcceptance") != &json!(true)
                    || (own.is_some() && unresolved.iter().all(|id| id == &own_intent))
                    || (context_own && unresolved.iter().all(|id| id == &context_id)))
                && hex(get(&g, "sourceHash"))
                && instant_v(get(&g, "at"))? <= (inner.borrow().deps.now)() as i64
                && (inner.borrow().deps.now)() as i64 - instant_v(get(&g, "at"))? <= 20000,
            "GLOBAL_UNKNOWN",
        )?;
        Ok(g)
    }
    fn budget(inner: &Rc<RefCell<BridgeInner>>, required: bool, intent_id: Option<&V>) -> Result<Option<V>, String> {
        let value = (inner.borrow().deps.get_installed_budget)();
        if value.is_none() || value.as_ref().is_some_and(V::is_null) {
            need(!required, "INSTALLED_BUDGET_REQUIRED")?;
            return Ok(None);
        }
        let value = value.unwrap();
        let record = get(&value, "record").clone();
        let receipt = get(&value, "receipt").clone();
        let (scope, alloc, alloc_hash) = {
            let g = inner.borrow();
            (g.scope.clone(), g.allocation.clone(), g.allocation_hash.clone())
        };
        let mut body = record.clone();
        let reservation_hash = match &mut body {
            V::Object(m) => m.remove("reservationHash").unwrap_or(V::Null),
            _ => V::Null,
        };
        need(
            hex(&reservation_hash)
                && payload_hash(&body)? == reservation_hash.as_str().unwrap_or("")
                && get(&record, "schemaVersion") == &json!(1)
                && get(&record, "kind").as_str() == Some("baal_scaled_no_full_nominal_reservation_v1")
                && get(&record, "ownerId") == get(&scope, "ownerId")
                && get(&record, "runId") == get(&scope, "runId")
                && get(&record, "tournamentId").as_str() == Some(T)
                && get(&record, "allocationHash").as_str() == Some(alloc_hash.as_str())
                && get(&record, "owned").is_array()
                && get(&record, "reservations").is_array()
                && get(&record, "unresolvedIntentIds").is_array()
                && (!required
                    || get(&record, "unresolvedIntentIds")
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|x| Some(x) == intent_id)),
            "BUDGET_BINDING",
        )?;
        let mut owned = BigInt::from(0);
        let mut reserved = BigInt::from(0);
        let mut by_market: HashMap<String, BigInt> = HashMap::new();
        let mut by_event: HashMap<String, BigInt> = HashMap::new();
        let mut seen: HashSet<String> = HashSet::new();
        for (list_key, field) in [("owned", "nominalCost"), ("reservations", "maximumNominalCost")] {
            for row in get(&record, list_key).as_array().cloned().unwrap_or_default() {
                need(
                    get(&row, "exposureId").as_str().is_some_and(|s| !s.is_empty())
                        && !seen.contains(get(&row, "exposureId").as_str().unwrap_or(""))
                        && id(get(&row, "marketId"))
                        && get(&row, "eventId").as_str().is_some_and(|s| !s.is_empty()),
                    "EXPOSURE_BINDING",
                )?;
                seen.insert(get(&row, "exposureId").as_str().unwrap_or("").to_string());
                let n = amount(get(&row, field).as_str().unwrap_or("")).map(BigInt::from)?;
                need(n >= BigInt::from(0), "NONNEGATIVE_NOMINAL")?;
                if field == "nominalCost" {
                    owned += n.clone();
                } else {
                    reserved += n.clone();
                }
                *by_market
                    .entry(get(&row, "marketId").as_str().unwrap_or("").to_string())
                    .or_insert_with(|| BigInt::from(0)) += n.clone();
                *by_event
                    .entry(get(&row, "eventId").as_str().unwrap_or("").to_string())
                    .or_insert_with(|| BigInt::from(0)) += n;
            }
        }
        let max_market = amount({
            let c = get(&alloc, "maximumMarketCost");
            if c.is_null() { "100.000000" } else { c.as_str().unwrap_or("") }
        })
        .map(BigInt::from)?;
        let max_event = amount({
            let c = get(&alloc, "maximumEventCost");
            if c.is_null() { "200.000000" } else { c.as_str().unwrap_or("") }
        })
        .map(BigInt::from)?;
        need(
            amount(get(&record, "ownedPrincipal").as_str().unwrap_or("")).map(BigInt::from)? == owned
                && amount(get(&record, "cashReserved").as_str().unwrap_or("")).map(BigInt::from)? == reserved
                && owned + reserved <= BigInt::from(1_000_000_000i64)
                && by_market.values().all(|v| v <= &max_market)
                && by_event.values().all(|v| v <= &max_event),
            "GLOBAL_NOMINAL_CAPS",
        )?;
        let now = (inner.borrow().deps.now)() as i64;
        need(
            get(&receipt, "reservationHash") == &reservation_hash
                && get(&receipt, "allocationHash").as_str() == Some(alloc_hash.as_str())
                && hex(get(&receipt, "globalLedgerHash"))
                && instant_v(get(&receipt, "confirmedAt"))? <= now
                && (!required || now - instant_v(get(&receipt, "confirmedAt"))? <= 20000),
            "BUDGET_RECEIPT",
        )?;
        Ok(Some(value))
    }
    fn account(inner: &Rc<RefCell<BridgeInner>>, raw: &V, leg: &V, n: i64) -> Result<V, String> {
        let (data, numbers) = parse_number_lexemes(get(get(raw, "positions"), "bodyText").as_str().unwrap_or(""))?;
        need(get(&data, "positions").is_array(), "POSITIONS")?;
        for (i, p) in get(&data, "positions").as_array().cloned().unwrap_or_default().iter().enumerate() {
            exact_integer(
                &numbers,
                &[json!("positions"), json!(i), json!("quantity")],
                js_int(get(p, "quantity")).unwrap_or(i64::MIN),
            )?;
        }
        let scoped = get(raw, "scopedOrdersPagesByExchange")
            .get(get(leg, "exchangeId").as_str().unwrap_or(""))
            .filter(|v| !v.is_null())
            .cloned()
            .unwrap_or_else(|| get(raw, "scopedOrdersPages").clone());
        let full = json!({
            "cash": get(raw, "cash"),
            "positions": get(raw, "positions"),
            "openOrdersPages": get(raw, "openOrdersPages"),
            "scopedOrdersPages": scoped,
        });
        let mut binding = inner.borrow().scope.clone();
        if let (V::Object(b), V::Object(l)) = (&mut binding, leg) {
            for (k, v) in l {
                b.insert(k.clone(), v.clone());
            }
        }
        let view = inspect_scoped_rest_no_account(&json!({
            "account": full,
            "binding": binding,
            "readAt": iso(n as f64),
        }))?;
        let mut v2 = view.clone();
        if let V::Object(m) = &mut v2 {
            let mut srcs = Vec::new();
            for r in [
                vec![get(&full, "cash").clone(), get(&full, "positions").clone()],
                get(&full, "openOrdersPages").as_array().cloned().unwrap_or_default(),
                get(&full, "scopedOrdersPages").as_array().cloned().unwrap_or_default(),
            ]
            .concat()
            {
                srcs.push(json!({
                    "method": get(&r, "method"),
                    "endpoint": get(&r, "endpoint"),
                    "requestStartedAt": get(&r, "requestStartedAt"),
                    "receivedAt": get(&r, "receivedAt"),
                    "sourceBodyHash": get(&r, "sourceBodyHash"),
                    "fetchInvocationId": get(&r, "fetchInvocationId"),
                }));
            }
            m.insert("sourceReceipts".into(), V::Array(srcs));
        }
        Ok(v2)
    }
    fn inventory(inner: &Rc<RefCell<BridgeInner>>, raw: &V, leg: &V, n: i64, required: bool) -> Result<Option<V>, String> {
        let view = Self::account(inner, raw, leg, n)?;
        let p = get(&view, "positions")
            .as_array()
            .and_then(|a| a.iter().find(|p| get(p, "exchangeId") == get(leg, "exchangeId")))
            .cloned();
        let owned = (inner.borrow().deps.get_owned_census)(get(leg, "exchangeId").as_str().unwrap_or(""));
        if owned.is_none() || owned.as_ref().is_some_and(V::is_null) {
            need(
                !required && p.as_ref().and_then(|p| js_int(get(p, "quantity"))).unwrap_or(0) == 0,
                "FOREIGN_POSITION",
            )?;
            return Ok(None);
        }
        let owned = owned.unwrap();
        let proof = get(&owned, "proof").clone();
        let mut proof_body = proof.clone();
        let verification_hash = match &mut proof_body {
            V::Object(m) => m.remove("verificationHash").unwrap_or(V::Null),
            _ => V::Null,
        };
        need(
            hex(&verification_hash)
                && payload_hash(&proof_body)? == verification_hash.as_str().unwrap_or("")
                && get(&proof, "mode").as_str() == Some("scoped_rest_no_multishare_fifo_v1")
                && get(&proof, "scopedTradeVerified") == &json!(true)
                && get(&proof, "ownerId") == get(&inner.borrow().scope, "ownerId")
                && get(&proof, "runId") == get(&inner.borrow().scope, "runId")
                && get(&proof, "tournamentId").as_str() == Some(T)
                && get(&proof, "marketId") == get(leg, "marketId")
                && get(&proof, "exchangeId") == get(leg, "exchangeId"),
            "OWNED_PROOF",
        )?;
        need(
            instant_v(get(&proof, "readAt"))? <= instant_v(get(&view, "startedAt"))?,
            "OWNED_PROOF_FUTURE",
        )?;
        let old = Self::account(inner, get(&owned, "after"), leg, instant_v(get(&proof, "readAt"))?)?;
        need(
            payload_hash(get(&old, "sourceReceipts"))? == payload_hash(get(get(&proof, "sourceReceipts"), "after"))?
                && payload_hash(&json!(sorted(get(&old, "scopedOrders").as_array().cloned().unwrap_or_default().as_slice())))?
                    == payload_hash(&json!(sorted(get(&view, "scopedOrders").as_array().cloned().unwrap_or_default().as_slice())))?,
            "OWNED_HISTORY_OR_SOURCE",
        )?;
        need(
            p.as_ref().and_then(|p| js_int(get(p, "quantity"))).unwrap_or(0)
                == -js_int(get(&proof, "remainingOwnedQuantity")).unwrap_or(i64::MIN)
                && (p.is_none()
                    || (get(p.as_ref().unwrap(), "marketId") == get(leg, "marketId")
                        && get(p.as_ref().unwrap(), "settled") != &json!(true)))
                && (!required || js_int(get(&proof, "remainingOwnedQuantity")).unwrap_or(0) > 0),
            "OWNED_QUANTITY",
        )?;
        Ok(Some(owned))
    }
    fn book(inner: &Rc<RefCell<BridgeInner>>, r: &V, leg: &V, n: i64) -> Result<V, String> {
        fresh(r, n, 20000)?;
        let (d, numbers) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
        need(
            (d.get("marketId").is_none() || get(&d, "marketId") == get(leg, "marketId"))
                && get(&d, "contexts").as_array().map(|x| x.len()) == Some(1)
                && get(&get(&d, "contexts")[0], "type").as_str() == Some("tournament")
                && get(&get(&get(&d, "contexts")[0], "tournament"), "id").as_str() == Some(T)
                && get(&get(&get(&d, "contexts")[0], "orderbook"), "exchanges")
                    .as_array()
                    .map(|x| x.len())
                    == Some(1)
                && get(&get(&get(&get(&d, "contexts")[0], "orderbook"), "exchanges")[0], "exchangeId") == get(leg, "exchangeId"),
            "BOOK_SCOPE",
        )?;
        for name in ["bids", "asks"] {
            for (i, l) in get(&get(&get(&d, "contexts")[0], "orderbook"), "exchanges")[0]
                .get(name)
                .and_then(V::as_array)
                .cloned()
                .unwrap_or_default()
                .iter()
                .enumerate()
            {
                exact_integer(
                    &numbers,
                    &[
                        json!("contexts"), json!(0), json!("orderbook"), json!("exchanges"),
                        json!(0), json!(name), json!(i), json!("quantity"),
                    ],
                    js_int(get(l, "quantity")).unwrap_or(i64::MIN),
                )?;
            }
        }
        normalize_book(
            &d,
            get(leg, "marketId").as_str().unwrap_or(""),
            T,
            get(r, "receivedAt").as_str().unwrap_or(""),
            get(r, "requestStartedAt").as_str().unwrap_or(""),
            20000,
            &std::collections::BTreeSet::new(),
            "authoritative_receipt_v1",
        )
        .map_err(|e| e)
    }

    /// getEvidence(context)
    pub fn get_evidence(&self, context: &V) -> Result<V, String> {
        let result: Result<V, String> = (|| {
            Self::allowed(&self.inner)?;
            let cache = {
                let g = self.inner.borrow();
                need(!g.busy && g.cache.is_some(), "CACHE_UNAVAILABLE")?;
                g.cache.clone().unwrap()
            };
            let operation = get(context, "operation").as_str().unwrap_or("").to_string();
            if let Some(cid) = context.get("orderId") {
                if !cid.is_null() {
                    need(cid == get(&cache, "orderId"), "CACHE_ORDER")?;
                }
            }
            if context.get("exchangeId").is_some_and(|x| !x.is_null()) {
                need(
                    get(&cache, "legs")
                        .as_array()
                        .map(|a| a.iter().any(|l| get(l, "exchangeId") == context.get("exchangeId").unwrap()))
                        .unwrap_or(false),
                    "CACHE_SCOPE",
                )?;
            }
            let g = Self::global(&self.inner, context.get("intentId"))?;
            let n = (self.inner.borrow().deps.now)() as i64;
            match get(&cache, "kind").as_str().unwrap_or("") {
                "buy" => {
                    need(
                        ["prepare_buy", "place_buy"].contains(&operation.as_str()),
                        "CACHE_OPERATION",
                    )?;
                    Self::budget(&self.inner, operation == "place_buy", context.get("intentId"))?;
                    for l in get(&cache, "legs").as_array().cloned().unwrap_or_default() {
                        let a = Self::account(&self.inner, get(&cache, "account"), &l, n)?;
                        need(
                            !get(&a, "positions")
                                .as_array()
                                .cloned()
                                .unwrap_or_default()
                                .iter()
                                .any(|p| {
                                    get(p, "exchangeId") == get(&l, "exchangeId")
                                        && js_int(get(p, "quantity")).unwrap_or(0) != 0
                                }),
                            "BUY_TARGET_NOT_FLAT",
                        )?;
                    }
                    for b in get(&cache, "bookReceipts").as_array().cloned().unwrap_or_default() {
                        Self::book(&self.inner, get(&b, "receipt"), &b, n)?;
                    }
                    for r in get(&cache, "rootReceipts").as_array().cloned().unwrap_or_default() {
                        fresh(get(&r, "receipt"), n, 300000)?;
                    }
                    Ok(json!({
                        "account": get(&cache, "account"),
                        "rootReceipts": get(&cache, "rootReceipts"),
                        "bookReceipts": get(&cache, "bookReceipts"),
                        "accountRisk": get(&cache, "accountRisk"),
                    }))
                }
                "sell" => {
                    need(
                        ["prepare_sell", "place_sell"].contains(&operation.as_str()),
                        "CACHE_OPERATION",
                    )?;
                    Self::budget(&self.inner, operation == "place_sell", context.get("intentId"))?;
                    let leg = get(&cache, "legs").as_array().and_then(|a| a.first()).cloned().unwrap_or(V::Null);
                    Self::inventory(&self.inner, get(&cache, "account"), &leg, n, true)?;
                    Self::book(&self.inner, get(&cache, "book"), &leg, n)?;
                    Ok(json!({
                        "account": get(&cache, "account"),
                        "book": get(&cache, "book"),
                        "globalRisk": g,
                    }))
                }
                _ => {
                    need(
                        ["reconcile", "cancel"].contains(&operation.as_str())
                            && operation == get(&cache, "kind").as_str().unwrap_or(""),
                        "KNOWN_ORDER",
                    )?;
                    let leg = get(&cache, "legs").as_array().and_then(|a| a.first()).cloned().unwrap_or(V::Null);
                    Self::order_binding(
                        &self.inner,
                        get(&cache, "orderId").as_str().unwrap_or(""),
                        &leg,
                    )?;
                    fresh(get(&cache, "order"), n, 20000)?;
                    if get(&cache, "kind").as_str() == Some("cancel") {
                        return Ok(json!({
                            "order": get(&cache, "order"),
                            "globalRisk": g,
                        }));
                    }
                    for r in get(&cache, "fillPages").as_array().cloned().unwrap_or_default() {
                        fresh(&r, n, 20000)?;
                    }
                    Self::account(&self.inner, get(&cache, "after"), &leg, n)?;
                    Ok(json!({
                        "order": get(&cache, "order"),
                        "fillPages": get(&cache, "fillPages"),
                        "after": get(&cache, "after"),
                        "globalRisk": g,
                    }))
                }
            }
        })();
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                let mut g = self.inner.borrow_mut();
                g.cache = None;
                g.last_failure = Some(e.clone());
                Err(safe_error(&e))
            }
        }
    }

    /// authorizes(spec) — true only for GET reads admitted through `read`.
    pub fn authorizes(
        &self,
        spec: &V,
        before_dispatch: Option<&Guard<'_>>,
        before_network: Option<&Guard<'_>>,
    ) -> bool {
        (|| -> Result<bool, String> {
            Self::allowed(&self.inner)?;
            let bd_ptr = before_dispatch.map(|g| g as *const Guard as *const () as usize).unwrap_or(0);
            let bn_ptr = before_network.map(|g| g as *const Guard as *const () as usize).unwrap_or(0);
            let p = self.inner.borrow().issued.get(&bd_ptr).cloned();
            Ok(p.is_some()
                && get(spec, "method").as_str() == Some("GET")
                && get(spec, "endpoint") == get(p.as_ref().unwrap(), "endpoint")
                && get(p.as_ref().unwrap(), "beforeNetworkPtr") == &json!(bn_ptr as u64)
                && spec.get("body").is_none()
                && spec.get("bodyJson").is_none()
                && spec.as_object().map(|m| {
                    m.keys().all(|k| {
                        ["method", "endpoint", "body", "bodyJson", "beforeDispatch", "beforeNetwork"]
                            .contains(&k.as_str())
                    })
                }).unwrap_or(false))
        })()
        .unwrap_or(false)
    }

    fn read(&self, endpoint: &str) -> Result<V, String> {
        Self::allowed(&self.inner)?;
        {
            let g = self.inner.borrow();
            need(g.reads < g.bound, "READ_BOUND")?;
        }
        let (has_headroom, dispatch, receipt_of) = {
            let g = self.inner.borrow();
            (
                g.deps.has_read_headroom.clone(),
                g.deps.api_dispatch.clone(),
                g.deps.receipt_of.clone(),
            )
        };
        let head = |network: bool| -> Result<(), String> {
            let (bound, reads) = {
                let g = self.inner.borrow();
                (g.bound, g.reads)
            };
            need(
                has_headroom(bound - reads - network as i64),
                "HEADROOM",
            )
        };
        head(false)?;
        let admitted = std::rc::Rc::new(Cell::new(false));
        let networked = std::rc::Rc::new(Cell::new(false));
        let admitted_d = admitted.clone();
        let networked_d = networked.clone();
        let inner = self.inner.clone();
        let allowed_e = |inner: &Rc<RefCell<BridgeInner>>| Self::allowed(inner);
        let inner_g = inner.clone();
        let before_dispatch: &Guard<'_> = &move |_p: &V, _s: &V| -> Result<bool, crate::transport::OrderTransportError> {
            allowed_e(&inner_g).map_err(|e| crate::transport::OrderTransportError {
                code: e,
                status: None,
                api_code: None,
                outcome: "unknown".into(),
                may_have_executed: false,
                retry_same_request_only: false,
                retry_after_ms: 0.0,
                semantic_mismatch: false,
            })?;
            Self::global(&inner_g, None).map_err(|e| crate::transport::OrderTransportError {
                code: e,
                status: None,
                api_code: None,
                outcome: "unknown".into(),
                may_have_executed: false,
                retry_same_request_only: false,
                retry_after_ms: 0.0,
                semantic_mismatch: false,
            })?;
            {
                let (bound, reads) = {
                    let g = inner_g.borrow();
                    (g.bound, g.reads)
                };
                let hh = inner_g.borrow().deps.has_read_headroom.clone();
                if !hh(bound - reads) {
                    return Err(crate::transport::OrderTransportError {
                        code: "BAAL_SCALED_EVIDENCE_HEADROOM".into(),
                        status: None,
                        api_code: None,
                        outcome: "unknown".into(),
                        may_have_executed: false,
                        retry_same_request_only: false,
                        retry_after_ms: 0.0,
                        semantic_mismatch: false,
                    });
                }
            }
            if admitted_d.get() {
                return Err(crate::transport::OrderTransportError {
                    code: "BAAL_SCALED_EVIDENCE_DUPLICATE_ADMISSION".into(),
                    status: None,
                    api_code: None,
                    outcome: "unknown".into(),
                    may_have_executed: false,
                    retry_same_request_only: false,
                    retry_after_ms: 0.0,
                    semantic_mismatch: false,
                });
            }
            admitted_d.set(true);
            Ok(true)
        };
        let inner_n = inner.clone();
        let admitted_n = admitted.clone();
        let networked_n = networked.clone();
        let before_network: &Guard<'_> = &move |_p: &V, _s: &V| -> Result<bool, crate::transport::OrderTransportError> {
            let to_err = |e: String| crate::transport::OrderTransportError {
                code: e,
                status: None,
                api_code: None,
                outcome: "unknown".into(),
                may_have_executed: false,
                retry_same_request_only: false,
                retry_after_ms: 0.0,
                semantic_mismatch: false,
            };
            Self::allowed(&inner_n).map_err(&to_err)?;
            Self::global(&inner_n, None).map_err(&to_err)?;
            if !admitted_n.get() {
                return Err(to_err("BAAL_SCALED_EVIDENCE_NOT_ADMITTED".into()));
            }
            {
                let (bound, reads) = {
                    let g = inner_n.borrow();
                    (g.bound, g.reads)
                };
                let hh = inner_n.borrow().deps.has_read_headroom.clone();
                if !hh(bound - reads - 1) {
                    return Err(to_err("BAAL_SCALED_EVIDENCE_HEADROOM".into()));
                }
            }
            networked_n.set(true);
            Ok(true)
        };
        let bd_ptr = before_dispatch as *const Guard<'_> as *const () as usize;
        let bn_ptr = before_network as *const Guard<'_> as *const () as usize;
        {
            let mut g = inner.borrow_mut();
            g.issued.insert(
                bd_ptr,
                json!({"endpoint": endpoint, "beforeNetworkPtr": bn_ptr as u64}),
            );
        }
        let at = (inner.borrow().deps.now)();
        let spec = json!({"method": "GET", "endpoint": endpoint});
        let result: Result<V, String> = (|| {
            let response = dispatch(&spec, Some(before_dispatch), Some(before_network))?;
            Self::allowed(&inner)?;
            need(
                admitted.get()
                    && networked.get()
                    && get(&response, "endpoint").as_str() == Some(endpoint)
                    && get(&response, "status") == &json!(200),
                "RESPONSE",
            )?;
            let r = receipt_of(&response, &json!({"method": "GET", "requestBodyHash": V::Null}))?;
            need(
                get(&r, "method").as_str() == Some("GET")
                    && get(&r, "endpoint").as_str() == Some(endpoint)
                    && get(&r, "status") == &json!(200)
                    && get(&r, "requestBodyHash").is_null()
                    && get(&r, "sourceBodyHash").as_str()
                        == Some(sha256(get(&r, "bodyText").as_str().unwrap_or("").as_bytes()).as_str())
                    && get(&r, "fetchInvocationId").as_str().is_some_and(|s| !s.is_empty()),
                "RAW_SOURCE",
            )?;
            fresh(
                &r,
                (inner.borrow().deps.now)() as i64,
                if endpoint.contains("/nodes?") { 300000 } else { 20000 },
            )?;
            let (d, _) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
            let last_end = inner.borrow().last_end;
            need(
                instant_v(get(&r, "requestStartedAt"))? >= at as i64
                    && instant_v(get(&r, "requestStartedAt"))? >= instant_v(get(&response, "requestStartedAt"))?
                    && instant_v(get(&r, "receivedAt"))? <= instant_v(get(&response, "receivedAt"))?
                    && (last_end.is_none() || instant_v(get(&r, "requestStartedAt"))? >= last_end.unwrap())
                    && payload_hash(&d)? == payload_hash(get(&response, "data"))?,
                "SOURCE_BINDING",
            )?;
            let previous = inner.borrow().watermarks.get(endpoint).cloned();
            need(
                previous.is_none()
                    || (instant_v(get(&r, "requestStartedAt"))? >= previous.unwrap().0
                        && instant_v(get(&r, "receivedAt"))? >= previous.unwrap().1),
                "SOURCE_ROLLBACK",
            )?;
            {
                let mut g = inner.borrow_mut();
                g.watermarks.insert(
                    endpoint.to_string(),
                    (
                        instant_v(get(&r, "requestStartedAt"))?,
                        instant_v(get(&r, "receivedAt"))?,
                    ),
                );
                g.last_end = Some(instant_v(get(&r, "receivedAt"))?);
            }
            Ok(r)
        })();
        {
            let mut g = inner.borrow_mut();
            g.reads += 1;
            g.issued.remove(&bd_ptr);
        }
        result
    }

    fn pages(&self, endpoint: &str) -> Result<Vec<V>, String> {
        let mut cursor: Option<String> = None;
        let mut frontier: Option<i64> = self.inner.borrow().projection_watermarks.get(endpoint).cloned();
        let mut result: Vec<V> = Vec::new();
        let mut cursors: HashSet<String> = HashSet::new();
        let mut ids: HashSet<i64> = HashSet::new();
        for _ in 0..10 {
            let url = match &cursor {
                None => endpoint.to_string(),
                Some(c) => format!("{}&cursor={}", endpoint, urlencoding(c)),
            };
            let r = self.read(&url)?;
            let (d, _) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
            let p = get(&d, "pagination");
            need(
                get(&d, "data").as_array().map(|a| a.len() <= 200).unwrap_or(false)
                    && get(p, "limit") == &json!(200)
                    && get(p, "hasMore").is_boolean()
                    && get(get(&d, "coverage"), "complete") == &json!(true)
                    && (get(get(&d, "coverage"), "projectedThroughSequence").is_null()
                        || js_int(get(get(&d, "coverage"), "projectedThroughSequence"))
                            .is_some_and(|x| x >= 0)),
                "COMPLETE_PAGES",
            )?;
            let current = js_int(get(get(&d, "coverage"), "projectedThroughSequence"));
            need(
                current.is_none() || frontier.is_none() || current.unwrap() >= frontier.unwrap(),
                "PROJECTION_ROLLBACK",
            )?;
            frontier = current.or(frontier);
            for row in get(&d, "data").as_array().cloned().unwrap_or_default() {
                let rid = js_int(get(&row, "id"));
                need(rid.is_some_and(|x| x > 0) && !ids.contains(&rid.unwrap()), "DUPLICATE_ROW")?;
                ids.insert(rid.unwrap());
            }
            result.push(r);
            if get(p, "hasMore") == &json!(false) {
                need(get(p, "nextCursor").is_null(), "CURSOR")?;
                if let Some(f) = frontier {
                    self.inner
                        .borrow_mut()
                        .projection_watermarks
                        .insert(endpoint.to_string(), f);
                }
                return Ok(result);
            }
            need(
                get(p, "nextCursor").as_str().is_some_and(|s| !s.is_empty() && s.len() <= 1000)
                    && !cursors.contains(get(p, "nextCursor").as_str().unwrap_or("")),
                "CURSOR",
            )?;
            cursors.insert(get(p, "nextCursor").as_str().unwrap().to_string());
            cursor = Some(get(p, "nextCursor").as_str().unwrap().to_string());
        }
        need(false, "PAGES_BOUND")?;
        unreachable!()
    }

    fn collect_account(&self, legs: &[V]) -> Result<V, String> {
        let slug = self.inner.borrow().scope.get("tournamentSlug").cloned().unwrap_or(V::Null);
        let slug = slug.as_str().unwrap_or("");
        let cash = self.read(&format!("/tournaments/{slug}"))?;
        let positions = self.read(&format!("/tournaments/{slug}/portfolio/positions"))?;
        let open_orders_pages = self.pages(&format!(
            "/orders?status=open&limit=200&tournamentId={T}"
        ))?;
        let mut by_exchange = serde_json::Map::new();
        for l in legs {
            by_exchange.insert(
                get(l, "exchangeId").as_str().unwrap_or("").to_string(),
                V::Array(self.pages(&format!(
                    "/orders?status=all&exchangeId={}&limit=200&tournamentId={T}",
                    get(l, "exchangeId").as_str().unwrap_or("")
                ))?),
            );
        }
        let mut out = json!({
            "cash": cash,
            "positions": positions,
            "openOrdersPages": open_orders_pages,
            "scopedOrdersPagesByExchange": V::Object(by_exchange),
        });
        if legs.len() == 1 {
            let scoped = get(
                get(&out, "scopedOrdersPagesByExchange"),
                get(&legs[0], "exchangeId").as_str().unwrap_or(""),
            )
            .clone();
            out.as_object_mut()
                .map(|m| m.insert("scopedOrdersPages".into(), scoped));
        }
        Ok(out)
    }

    fn run(
        &self,
        kind: &str,
        legs: Vec<V>,
        order_id: Option<String>,
        maximum: i64,
        f: impl FnOnce(&Self) -> Result<V, String>,
    ) -> Result<V, String> {
        {
            let mut g = self.inner.borrow_mut();
            need(!g.busy, "BUSY")?;
        }
        Self::allowed(&self.inner)?;
        {
            let mut g = self.inner.borrow_mut();
            g.cache = None;
            g.busy = true;
            g.operation = Some(kind.to_string());
            g.current_order_id = order_id.clone();
            g.reads = 0;
            g.bound = maximum;
            g.last_end = None;
            g.last_failure = None;
        }
        let result = (|| -> Result<V, String> {
            need(
                (self.inner.borrow().deps.has_read_headroom)(maximum),
                "HEADROOM",
            )?;
            if let Some(oid) = &order_id {
                need(id(&json!(oid)), "KNOWN_ORDER")?;
                Self::order_binding(&self.inner, oid, &legs[0])?;
            }
            Self::global(&self.inner, None)?;
            let value = f(self)?;
            Self::allowed(&self.inner)?;
            let mut cache = value.clone();
            if let V::Object(m) = &mut cache {
                m.insert("kind".into(), json!(kind));
                m.insert("legs".into(), V::Array(legs.clone()));
                m.insert("orderId".into(), order_id.map(|x| json!(x)).unwrap_or(V::Null));
            }
            self.inner.borrow_mut().cache = Some(cache);
            Ok(json!({
                "kind": kind,
                "readCount": self.inner.borrow().reads,
                "completedAt": iso((self.inner.borrow().deps.now)()),
            }))
        })();
        {
            let mut g = self.inner.borrow_mut();
            g.busy = false;
            g.operation = None;
            g.current_order_id = None;
        }
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                let mut g = self.inner.borrow_mut();
                g.cache = None;
                g.last_failure = Some(safe_error(&e));
                Err(safe_error(&e))
            }
        }
    }

    /// refreshBuy(pair) — collect roots+account+books for a candidate pair.
    pub fn refresh_buy(&self, pair: &[V]) -> Result<V, String> {
        need(pair.len() == 2, "PAIR")?;
        let legs: Vec<V> = pair
            .iter()
            .map(|x| Self::member(&self.inner, x))
            .collect::<Result<_, _>>()?;
        need(
            get(&legs[0], "exchangeId") != get(&legs[1], "exchangeId")
                && get(&legs[0], "marketId") != get(&legs[1], "marketId"),
            "DISTINCT_PAIR",
        )?;
        self.run("buy", legs.clone(), None, 10, move |this| {
            let mut root_receipts = Vec::new();
            for l in &legs {
                let mut lr = l.clone();
                let receipt = this.read(&format!(
                    "/markets/{}/nodes?tournamentId={T}",
                    get(l, "marketId").as_str().unwrap_or("")
                ))?;
                lr.as_object_mut()
                    .map(|m| m.insert("receipt".into(), receipt));
                root_receipts.push(lr);
            }
            let account = this.collect_account(&legs)?;
            let scope = this.inner.borrow().scope.clone();
            let mut views = Vec::new();
            for l in &legs {
                let mut binding = scope.clone();
                if let (V::Object(b), V::Object(lm)) = (&mut binding, l) {
                    for (k, v) in lm {
                        b.insert(k.clone(), v.clone());
                    }
                }
                views.push(inspect_scoped_rest_no_account(&json!({
                    "account": {
                        "cash": get(&account, "cash"),
                        "positions": get(&account, "positions"),
                        "openOrdersPages": get(&account, "openOrdersPages"),
                        "scopedOrdersPages": get(get(&account, "scopedOrdersPagesByExchange"), get(l, "exchangeId").as_str().unwrap_or("")),
                    },
                    "binding": binding,
                    "readAt": iso((this.inner.borrow().deps.now)()),
                }))?);
            }
            need(
                views.iter().all(|v| {
                    !get(v, "positions")
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .any(|p| {
                            legs.iter().any(|l| get(l, "exchangeId") == get(p, "exchangeId"))
                                && js_int(get(p, "quantity")).unwrap_or(0) != 0
                        })
                }),
                "BUY_TARGET_NOT_FLAT",
            )?;
            let account_risk = (this.inner.borrow().deps.get_account_risk)()?;
            need(
                instant_v(get(&account_risk, "at"))?
                    >= views
                        .iter()
                        .map(|v| instant_v(get(v, "completedAt")))
                        .collect::<Result<Vec<i64>, String>>()?
                        .into_iter()
                        .max()
                        .unwrap_or(i64::MIN),
                "ACCOUNT_RISK_BEFORE_SOURCE",
            )?;
            let mut book_receipts = Vec::new();
            for l in &legs {
                let r = this.read(&format!(
                    "/markets/{}/orderbook?tournamentId={T}&depth=20",
                    get(l, "marketId").as_str().unwrap_or("")
                ))?;
                Self::book(&this.inner, &r, l, (this.inner.borrow().deps.now)() as i64)?;
                need(
                    instant_v(get(&account_risk, "at"))? <= instant_v(get(&r, "requestStartedAt"))?,
                    "RISK_BEFORE_BOOK",
                )?;
                let mut lr = l.clone();
                lr.as_object_mut()
                    .map(|m| m.insert("receipt".into(), r));
                book_receipts.push(lr);
            }
            Ok(json!({
                "account": account,
                "rootReceipts": root_receipts,
                "bookReceipts": book_receipts,
                "accountRisk": account_risk,
            }))
        })
    }

    /// refreshSell(leg)
    pub fn refresh_sell(&self, leg: &V) -> Result<V, String> {
        let leg = Self::member(&self.inner, leg)?;
        let legs = vec![leg.clone()];
        self.run("sell", legs, None, 5, move |this| {
            let raw = this.collect_account(std::slice::from_ref(&leg))?;
            let current = Self::inventory(&this.inner, &raw, &leg, (this.inner.borrow().deps.now)() as i64, true)?
                .ok_or("BAAL_SCALED_EVIDENCE_FOREIGN_POSITION")?;
            let book_receipt = this.read(&format!(
                "/markets/{}/orderbook?tournamentId={T}&depth=20",
                get(&leg, "marketId").as_str().unwrap_or("")
            ))?;
            Self::book(&this.inner, &book_receipt, &leg, (this.inner.borrow().deps.now)() as i64)?;
            need(
                instant_v(get(&book_receipt, "requestStartedAt"))?
                    >= instant_v(get(&Self::account(&this.inner, &raw, &leg, (this.inner.borrow().deps.now)() as i64)?, "completedAt"))?,
                "ACCOUNT_BEFORE_BOOK",
            )?;
            Ok(json!({
                "account": {
                    "cash": get(&raw, "cash"),
                    "positions": get(&raw, "positions"),
                    "openOrdersPages": get(&raw, "openOrdersPages"),
                    "scopedOrdersPages": get(&raw, "scopedOrdersPages"),
                },
                "book": book_receipt,
                "ownedProofHash": get(get(&current, "proof"), "verificationHash"),
            }))
        })
    }

    /// refreshReconciliation(orderId, leg)
    pub fn refresh_reconciliation(&self, order_id: &str, leg: &V) -> Result<V, String> {
        let leg = Self::member(&self.inner, leg)?;
        let order_id_owned = order_id.to_string();
        self.run("reconcile", vec![leg.clone()], Some(order_id_owned.clone()), 8, move |this| {
            let expected = Self::order_binding(&this.inner, &order_id_owned, &leg)?;
            let order = this.read(&format!("/orders/{order_id_owned}"))?;
            let (d, parsed_numbers) = parse_number_lexemes(get(&order, "bodyText").as_str().unwrap_or(""))?;
            need(
                js_int(get(&d, "id")).map(|x| x.to_string()) == Some(order_id_owned.clone())
                    && get(&d, "exchangeId") == get(&leg, "exchangeId")
                    && get(&d, "tournamentId").as_str() == Some(T)
                    && get(&d, "side").as_str() == Some("no")
                    && get(&d, "action") == get(get(&expected, "body"), "action")
                    && js_int(get(&d, "quantity")).is_some_and(|q| q > 0)
                    && js_int(get(&d, "quantity")).unwrap_or(i64::MAX)
                        <= js_int(get(get(&expected, "body"), "quantity")).unwrap_or(i64::MIN)
                    && get(&d, "expirationDate") == get(get(&expected, "body"), "expirationDate")
                    && normalize_official_fill_tick(get(&d, "priceLimit"))
                        .ok()
                        .and_then(|t| t.get("price").cloned())
                        == normalize_official_fill_tick(get(get(&expected, "body"), "price"))
                            .ok()
                            .and_then(|t| t.get("price").cloned())
                    && get(&d, "open") == &json!(false),
                "TERMINAL_ORDER",
            )?;
            for k in ["id", "quantity", "quantityFilled"] {
                exact_integer(&parsed_numbers, &[json!(k)], js_int(get(&d, k)).unwrap_or(i64::MIN))?;
            }
            need(
                js_int(get(&d, "quantityFilled")).unwrap_or(-1) >= 0
                    && js_int(get(&d, "quantityFilled")).unwrap_or(i64::MAX)
                        <= js_int(get(&d, "quantity")).unwrap_or(i64::MIN),
                "TERMINAL_FILLED_QUANTITY",
            )?;
            let fill_pages = this.pages(&format!("/orders/{order_id_owned}/fills?limit=200"))?;
            let mut total: Option<i64> = None;
            let mut seen: HashSet<i64> = HashSet::new();
            let mut filled: i64 = 0;
            for r in &fill_pages {
                let (v, pnum) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
                need(
                    js_int(get(&v, "orderId")).map(|x| x.to_string()) == Some(order_id_owned.clone())
                        && get(&v, "exchangeId") == get(&leg, "exchangeId")
                        && get(&v, "tournamentId").as_str() == Some(T)
                        && js_int(get(&v, "totalQuantityFilled"))
                            .is_some_and(|x| x <= 0 && x >= -js_int(get(get(&expected, "body"), "quantity")).unwrap_or(i64::MIN)),
                    "FILL_SCOPE",
                )?;
                exact_integer(&pnum, &[json!("orderId")], js_int(get(&v, "orderId")).unwrap_or(i64::MIN))?;
                exact_integer(&pnum, &[json!("totalQuantityFilled")], js_int(get(&v, "totalQuantityFilled")).unwrap_or(i64::MIN))?;
                need(
                    total.is_none() || total == js_int(get(&v, "totalQuantityFilled")),
                    "FILL_TOTAL_CHANGED",
                )?;
                total = js_int(get(&v, "totalQuantityFilled"));
                for (i, f) in get(&v, "data").as_array().cloned().unwrap_or_default().iter().enumerate() {
                    exact_integer(&pnum, &[json!("data"), json!(i), json!("quantity")], js_int(get(f, "quantity")).unwrap_or(i64::MIN))?;
                    need(
                        get(f, "side").as_str() == Some("no")
                            && js_int(get(f, "quantity")).unwrap_or(0) < 0
                            && !seen.contains(&js_int(get(f, "id")).unwrap_or(i64::MIN))
                            && instant_v(get(f, "filledAt"))? >= instant_v(get(&d, "createdAt"))?
                            && instant_v(get(f, "filledAt"))? <= instant_v(get(r, "receivedAt"))?,
                        "FILL_IDENTITY",
                    )?;
                    seen.insert(js_int(get(f, "id")).unwrap_or(i64::MIN));
                    filled -= js_int(get(f, "quantity")).unwrap_or(0);
                    let px = amount(
                        normalize_official_fill_tick(get(f, "price"))
                            .map_err(|e| e)?
                            .get("price")
                            .and_then(V::as_str)
                            .unwrap_or(""),
                    )
                    .map(BigInt::from)?;
                    let limit = amount(
                        normalize_official_fill_tick(get(get(&expected, "body"), "price"))
                            .map_err(|e| e)?
                            .get("price")
                            .and_then(V::as_str)
                            .unwrap_or(""),
                    )
                    .map(BigInt::from)?;
                    need(
                        if get(get(&expected, "body"), "action").as_str() == Some("buy") {
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
            let raw = this.collect_account(std::slice::from_ref(&leg))?;
            need(
                instant_v(get(get(&raw, "cash"), "requestStartedAt"))?
                    >= instant_v(get(fill_pages.last().unwrap_or(&V::Null), "receivedAt"))?,
                "AFTER_ACCOUNT_CAUSALITY",
            )?;
            Ok(json!({
                "order": order,
                "fillPages": fill_pages,
                "after": {
                    "cash": get(&raw, "cash"),
                    "positions": get(&raw, "positions"),
                    "openOrdersPages": get(&raw, "openOrdersPages"),
                    "scopedOrdersPages": get(&raw, "scopedOrdersPages"),
                },
            }))
        })
    }

    /// refreshCancel(orderId, leg)
    pub fn refresh_cancel(&self, order_id: &str, leg: &V) -> Result<V, String> {
        let leg = Self::member(&self.inner, leg)?;
        let order_id_owned = order_id.to_string();
        self.run("cancel", vec![leg.clone()], Some(order_id_owned.clone()), 1, move |this| {
            let expected = Self::order_binding(&this.inner, &order_id_owned, &leg)?;
            let order = this.read(&format!("/orders/{order_id_owned}"))?;
            let (d, parsed_numbers) = parse_number_lexemes(get(&order, "bodyText").as_str().unwrap_or(""))?;
            need(
                js_int(get(&d, "id")).map(|x| x.to_string()) == Some(order_id_owned.clone())
                    && get(&d, "exchangeId") == get(&leg, "exchangeId")
                    && get(&d, "tournamentId").as_str() == Some(T)
                    && get(&d, "side").as_str() == Some("no")
                    && get(&d, "action") == get(get(&expected, "body"), "action")
                    && js_int(get(&d, "quantity")).is_some_and(|q| q > 0)
                    && js_int(get(&d, "quantity")).unwrap_or(i64::MAX)
                        <= js_int(get(get(&expected, "body"), "quantity")).unwrap_or(i64::MIN)
                    && get(&d, "expirationDate") == get(get(&expected, "body"), "expirationDate")
                    && normalize_official_fill_tick(get(&d, "priceLimit"))
                        .ok()
                        .and_then(|t| t.get("price").cloned())
                        == normalize_official_fill_tick(get(get(&expected, "body"), "price"))
                            .ok()
                            .and_then(|t| t.get("price").cloned())
                    && get(&d, "open") == &json!(true),
                "OPEN_OWNED_ORDER",
            )?;
            exact_integer(&parsed_numbers, &[json!("id")], js_int(get(&d, "id")).unwrap_or(i64::MIN))?;
            exact_integer(&parsed_numbers, &[json!("quantity")], js_int(get(&d, "quantity")).unwrap_or(i64::MIN))?;
            Ok(json!({"order": order}))
        })
    }

    /// getBuyProposal()
    pub fn get_buy_proposal(&self) -> Result<V, String> {
        let evidence = self.get_evidence(&json!({"operation": "prepare_buy"}))?;
        let (allocation, scope) = {
            let g = self.inner.borrow();
            (g.allocation.clone(), g.scope.clone())
        };
        let mut args = evidence;
        if let V::Object(m) = &mut args {
            m.insert("allocation".into(), allocation);
            m.insert("scope".into(), scope);
            m.insert("now".into(), json!(iso((self.inner.borrow().deps.now)())));
        }
        Ok(plan_baal_scaled_entry(&args))
    }


    /// status()
    pub fn status(&self) -> V {
        let g = self.inner.borrow();
        json!({
            "mode": "baal_scaled_no_evidence_bridge_v1",
            "busy": g.busy,
            "closed": g.closed,
            "lastReadCount": g.reads,
            "maximumReads": g.bound,
            "cachedKind": g.cache.as_ref().and_then(|c| get(c, "kind").as_str().map(String::from)),
            "lastFailure": g.last_failure,
            "additionalApiClients": 0,
            "executionAuthority": false,
        })
    }
    /// clear()
    pub fn clear(&self) {
        self.inner.borrow_mut().cache = None;
    }
    /// close()
    pub fn close(&self) -> Result<(), String> {
        let mut g = self.inner.borrow_mut();
        need(!g.busy, "BUSY")?;
        g.cache = None;
        g.closed = true;
        g.watermarks.clear();
        Ok(())
    }
}

fn urlencoding(s: &str) -> String {
    // encodeURIComponent
    let mut out = String::new();
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// createBaalScaledNoEvidenceBridge({api, receiptOf, scope, allocation,
/// universe, getAccountLease, getGlobalRisk, getAccountRisk, getOwnedCensus,
/// getInstalledBudget, ownsOrder, getOrderBinding, canRead, hasReadHeadroom, now})
pub fn create_baal_scaled_no_evidence_bridge(
    scope: &V,
    allocation: &V,
    universe: &V,
    deps: BaalScaledNoEvidenceBridgeDeps,
) -> Result<BaalScaledNoEvidenceBridge, String> {
    need(
        get(scope, "ownerId").as_str() == Some("Codex")
            && get(scope, "tournamentId").as_str() == Some(T)
            && get(scope, "tournamentSlug").as_str() == Some("midterm-elections")
            && get(scope, "runId").as_str().is_some_and(|s| !s.is_empty())
            && get(universe, "tournamentId").as_str() == Some(T)
            && get(universe, "exchanges").is_array()
            && get(allocation, "ownerId") == get(scope, "ownerId")
            && get(allocation, "runId") == get(scope, "runId")
            && get(allocation, "tournamentId").as_str() == Some(T)
            && get(allocation, "capital").as_str() == Some("1000.000000"),
        "SCOPE_ALLOCATION",
    )?;
    let allocation_hash = payload_hash(allocation)?;
    let mut members: HashMap<String, V> = HashMap::new();
    for m in get(universe, "exchanges").as_array().cloned().unwrap_or_default() {
        need(
            id(get(&m, "marketId"))
                && id(get(&m, "exchangeId"))
                && get(&m, "option").as_str() == Some("YES")
                && !members.contains_key(get(&m, "exchangeId").as_str().unwrap_or("")),
            "UNIVERSE",
        )?;
        members.insert(
            get(&m, "exchangeId").as_str().unwrap_or("").to_string(),
            json!({
                "marketId": get(&m, "marketId"),
                "exchangeId": get(&m, "exchangeId"),
            }),
        );
    }
    let l = (deps.get_account_lease)();
    need(
        get(&l, "ownerId") == get(scope, "ownerId")
            && get(&l, "tournamentId").as_str() == Some(T)
            && hex(get(&l, "leaseId")),
        "CURRENT_OWNER",
    )?;
    let opening_lease = payload_hash(&l)?;
    Ok(BaalScaledNoEvidenceBridge {
        inner: Rc::new(RefCell::new(BridgeInner {
            scope: scope.clone(),
            allocation: allocation.clone(),
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
            opening_lease,
            issued: HashMap::new(),
            watermarks: HashMap::new(),
            projection_watermarks: HashMap::new(),
            deps,
        })),
    })
}
