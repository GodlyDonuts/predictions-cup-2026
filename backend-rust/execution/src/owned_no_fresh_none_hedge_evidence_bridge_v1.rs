// execution/owned-no-fresh-none-hedge-evidence-bridge-v1.mjs — sole existing
// client only. This bridge has no POST/DELETE permit, new owner, credential or
// timer. Its cached NONE receipt cannot authorize a write.
//
// Port notes:
// - `need` errors carry .code in JS (Object.assign(Error, {code})) -> bare
//   coded Strings here; foreign/callback errors propagate unchanged
//   (callers apply `codeless` if they must distinguish).
// - async fns are synchronous; `api.dispatch(spec)` takes the spec plus the
//   beforeDispatch/beforeNetwork guards as arguments (transport::Dispatch).
// - JS WeakMap permits keyed on the beforeDispatch function object ->
//   issued: HashMap<usize,V> keyed by the guard pointer (beforeNetworkPtr
//   stored alongside, same as owned_no_exit_evidence_bridge_v1).
// - `e.retryAfterMs`/`e.fatal`/`e.durableRecordingFailed` propagation from
//   dispatch errors is not modeled (String errors) — same trade as siblings.
// - `structuredClone` -> .clone(). `Date.now()` `now` -> Rc<dyn Fn() -> f64>.
use crate::baal_scaled_owned_no_hedge_proposal_v1::plan_baal_owned_no_hedge;
use crate::core::payload_hash;
use crate::receipts::parse_number_lexemes;
use crate::transport::{Dispatch, Guard, OrderTransportError};
use baal_service_runtime::fixed::instant;
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use serde_json::{json, Value as V};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("NONE_HEDGE_EVIDENCE_{code}"))
    }
}
fn ote(code: String) -> OrderTransportError {
    OrderTransportError {
        code,
        status: None,
        api_code: None,
        outcome: "blocked".into(),
        may_have_executed: false,
        retry_same_request_only: false,
        retry_after_ms: 0.0,
        semantic_mismatch: false,
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn hex(v: &V) -> bool {
    v.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(payload_hash(a)? == payload_hash(b)?)
}
fn iso(n: f64) -> String {
    baal_service_runtime::contract::ms_to_iso(n)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn id_str(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
}

/// `api` — the caller's existing serialized client (dispatch + backoff fields).
pub struct NoneHedgeBridgeApi {
    pub dispatch: Rc<Dispatch>,
    /// api.manualReview — JS reads `api.manualReview!==true`.
    pub manual_review: bool,
    /// api.cooldown — numeric ms; JS `api.cooldown||0`.
    pub cooldown: f64,
}

pub struct NoneHedgeBridgeV1Deps {
    pub api: NoneHedgeBridgeApi,
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub scope: V,
    pub first_exchange_id: String,
    pub get_owned: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub get_pair_census: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub get_funding: Rc<dyn Fn() -> Result<V, String>>,
    pub get_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub can_read: Rc<dyn Fn() -> bool>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> Result<bool, String>>,
    pub get_active_intent: Option<Rc<dyn Fn() -> Result<V, String>>>,
    pub get_order_binding: Option<Rc<dyn Fn(&V) -> Result<V, String>>>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct Inner {
    deps: NoneHedgeBridgeV1Deps,
    scope: V,
    first_exchange_id: String,
    first: V,
    other: V,
    original_owned_hash: String,
    cache: Option<V>,
    job_binding: Option<V>,
    busy: bool,
    closed: bool,
    reads: i64,
    last_end: Option<i64>,
    last_failure: Option<String>,
    issued: HashMap<usize, V>,
    watermarks: HashMap<String, (i64, i64)>,
    invocations: HashSet<String>,
}

#[derive(Clone)]
pub struct OwnedNoFreshNoneHedgeEvidenceBridge {
    inner: Rc<RefCell<Inner>>,
}

/// sync(f, ...a) — the result must not be a thenable; structuredClone on the
/// return. In the port the callbacks are already synchronous Results.
fn sync(v: Result<V, String>) -> Result<V, String> {
    v.map(|x| x.clone())
}

fn own_pending(inner: &Rc<RefCell<Inner>>, g: &V) -> Result<bool, String> {
    let (o_v, scope, first_ex, other_ex) = {
        let b = inner.borrow();
        let f = b
            .deps
            .get_active_intent
            .clone()
            .unwrap_or_else(|| Rc::new(|| Ok(V::Null)));
        (sync(f())?, b.scope.clone(), b.first_exchange_id.clone(), get(&b.other, "exchangeId").as_str().unwrap_or("").to_string())
    };
    let o = &o_v;
    if !js_truthy(Some(o)) {
        return Ok(false);
    }
    let actual_fetch = if !get(o, "actualFetchCount").is_null() {
        get(o, "actualFetchCount")
    } else {
        get(o, "fetchCount")
    };
    let counts_agree = o.get("actualFetchCount").is_none()
        || o.get("actualFetchCount").is_some_and(V::is_null)
        || o.get("fetchCount").is_none()
        || o.get("fetchCount").is_some_and(V::is_null)
        || get(o, "actualFetchCount") == get(o, "fetchCount");
    let _ = counts_agree;
    Ok(get(o, "intentId").is_string()
        && get(o, "firstExchangeId").as_str() == Some(first_ex.as_str())
        && ["prepared", "dispatching"].contains(&get(o, "phase").as_str().unwrap_or(""))
        && get(o, "orderId").is_null()
        && js_int(actual_fetch) == Some(0)
        && (o.get("actualFetchCount").is_none()
            || get(o, "actualFetchCount").is_null()
            || o.get("fetchCount").is_none()
            || get(o, "fetchCount").is_null()
            || get(o, "actualFetchCount") == get(o, "fetchCount"))
        && get(get(o, "body"), "side").as_str() == Some("no")
        && get(get(o, "body"), "action").as_str() == Some("buy")
        && js_int(get(get(o, "body"), "quantity")) == Some(1)
        && get(get(o, "body"), "exchangeId").as_str() == Some(other_ex.as_str())
        && get(get(o, "body"), "tournamentId") == get(&scope, "tournamentId")
        && get(o, "bodyHash").as_str() == Some(payload_hash(get(o, "body"))?.as_str())
        && get(get(o, "body"), "idempotencyKey").is_string()
        && instant_v(get(get(o, "body"), "expirationDate"))? > {
            let n = inner.borrow();
            (n.deps.now)() as i64
        }
        && get(g, "unresolvedIntentIds").as_array().is_some_and(|a| a.len() == 1)
        && &get(g, "unresolvedIntentIds")[0] == get(o, "intentId"))
}

fn cleanup_own(inner: &Rc<RefCell<Inner>>, g: &V) -> Result<bool, String> {
    let (binding, other_ex, scope) = {
        let b = inner.borrow();
        (
            b.job_binding
                .clone()
                .or_else(|| b.cache.as_ref().and_then(|c| c.get("binding").cloned())),
            get(&b.other, "exchangeId").as_str().unwrap_or("").to_string(),
            b.scope.clone(),
        )
    };
    let binding = match binding {
        Some(v) if !v.is_null() => v,
        _ => return Ok(false),
    };
    let actual = {
        let b = inner.borrow();
        let f = b
            .deps
            .get_order_binding
            .clone()
            .unwrap_or_else(|| Rc::new(|_| Ok(V::Null)));
        sync(f(get(&binding, "orderId")))?
    };
    Ok(js_truthy(Some(&actual))
        && get(&actual, "intentId") == get(&binding, "intentId")
        && get(&actual, "orderId") == get(&binding, "orderId")
        && get(&actual, "bodyHash") == get(&binding, "bodyHash")
        && js_int(get(get(&actual, "body"), "quantity")) == Some(1)
        && get(get(&actual, "body"), "action").as_str() == Some("buy")
        && get(get(&actual, "body"), "side").as_str() == Some("no")
        && get(get(&actual, "body"), "exchangeId").as_str() == Some(other_ex.as_str())
        && get(get(&actual, "body"), "tournamentId") == get(&scope, "tournamentId")
        && get(&actual, "bodyHash").as_str() == Some(payload_hash(get(&actual, "body"))?.as_str())
        && get(g, "unresolvedIntentIds")
            .as_array()
            .is_some_and(|a| a.iter().all(|id| id == get(&binding, "intentId"))))
}

fn local(inner: &Rc<RefCell<Inner>>) -> Result<V, String> {
    let deps_can_read;
    let scope;
    {
        let b = inner.borrow();
        need(!b.closed && (b.deps.can_read)(), "STOPPED")?;
        deps_can_read = ();
        let _ = deps_can_read;
        scope = b.scope.clone();
    }
    let lease = {
        let b = inner.borrow();
        sync((b.deps.get_account_lease)())?
    };
    need(
        get(&lease, "ownerId") == get(&scope, "ownerId")
            && get(&lease, "tournamentId") == get(&scope, "tournamentId")
            && hex(get(&lease, "leaseId")),
        "ACCOUNT_LEASE",
    )?;
    let (fresh_owned, original_owned_hash) = {
        let b = inner.borrow();
        (sync((b.deps.get_owned)(&json!(b.first_exchange_id)))?, b.original_owned_hash.clone())
    };
    need(payload_hash(&fresh_owned)? == original_owned_hash, "OWNED_SOURCE_CHANGED")?;
    let g = {
        let b = inner.borrow();
        sync((b.deps.get_global_risk)())?
    };
    let n = inner.borrow().deps.now.clone()();
    need(
        get(&g, "ownerId") == get(&scope, "ownerId")
            && get(&g, "runId") == get(&scope, "runId")
            && get(&g, "tournamentId") == get(&scope, "tournamentId")
            && get(&g, "complete") == &json!(true)
            && get(&g, "eventScopeComplete") == &json!(true)
            && get(&g, "soleCollector") == &json!(true)
            && get(&g, "stopped") == &json!(false)
            && (get(&g, "unknownAcceptance") == &json!(false) || cleanup_own(inner, &g)?)
            && get(&g, "unresolvedIntentIds").is_array()
            && (get(&g, "unresolvedIntentIds").as_array().unwrap().is_empty()
                || own_pending(inner, &g)?
                || cleanup_own(inner, &g)?)
            && hex(get(&g, "sourceHash"))
            && instant_v(get(&g, "at"))? <= n as i64
            && n as i64 - instant_v(get(&g, "at"))? <= 20_000,
        "UNKNOWN_OR_GLOBAL_SOURCE",
    )?;
    let (manual_review, cooldown) = {
        let b = inner.borrow();
        (b.deps.api.manual_review, b.deps.api.cooldown)
    };
    need(!manual_review && n >= cooldown, "SHARED_BACKOFF")?;
    Ok(g)
}

const RAW_FIELDS: [&str; 9] = [
    "method",
    "endpoint",
    "status",
    "requestBodyHash",
    "bodyText",
    "sourceBodyHash",
    "fetchInvocationId",
    "requestStartedAt",
    "receivedAt",
];

struct RawOut {
    data: V,
    numbers: std::collections::BTreeMap<String, String>,
}

/// raw(r, endpoint, at, age=20000)
fn raw(r: &V, endpoint: &str, at: i64, age: i64) -> Result<RawOut, String> {
    need(
        !r.is_null()
            && r.as_object().map(|o| o.keys().all(|k| RAW_FIELDS.contains(&k.as_str()))).unwrap_or(false),
        "RAW_FIELD_SCOPE",
    )?;
    need(
        get(r, "method").as_str() == Some("GET")
            && get(r, "endpoint").as_str() == Some(endpoint)
            && js_int(get(r, "status")) == Some(200)
            && get(r, "requestBodyHash").is_null()
            && get(r, "bodyText").as_str().is_some_and(|s| s.len() <= 8 * 1024 * 1024)
            && get(r, "sourceBodyHash").as_str()
                == Some(sha256(get(r, "bodyText").as_str().unwrap_or("").as_bytes()).as_str())
            && get(r, "fetchInvocationId").as_str().is_some_and(|s| !s.is_empty()),
        "RAW_SOURCE",
    )?;
    let start = instant_v(get(r, "requestStartedAt"))?;
    let end = instant_v(get(r, "receivedAt"))?;
    need(start <= end && end <= at && at - start <= age, "SOURCE_CLOCK")?;
    let (data, numbers) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap())?;
    Ok(RawOut { data, numbers })
}

fn none(inner: &Rc<RefCell<Inner>>, v: &V, at: i64) -> Result<V, String> {
    let (first, other, scope) = {
        let b = inner.borrow();
        (b.first.clone(), b.other.clone(), b.scope.clone())
    };
    let graph_endpoint = format!(
        "/relationships/graph?marketId={}&depth=4&tournamentId={}",
        get(&first, "marketId").as_str().unwrap_or(""),
        get(&scope, "tournamentId").as_str().unwrap_or("")
    );
    let collateral_endpoint = format!(
        "/portfolio/collateral?tournamentId={}",
        get(&scope, "tournamentId").as_str().unwrap_or("")
    );
    let graph = raw(get(v, "graphReceipt"), &graph_endpoint, at, 5_000)?.data;
    let collateral = raw(get(v, "collateralReceipt"), &collateral_endpoint, at, 5_000)?.data;
    need(
        !graph.is_null()
            && {
                let mut ks: Vec<String> = graph
                    .as_object()
                    .map(|o| o.keys().cloned().collect())
                    .unwrap_or_default();
                ks.sort();
                ks.join("|") == "nodes|relationships"
            }
            && get(&graph, "relationships").as_array().is_some_and(|a| a.is_empty())
            && get(&graph, "nodes").as_array().is_some_and(|a| a.len() <= 2),
        "NON_NONE_OR_UNSUPPORTED_GRAPH",
    )?;
    let mut ids: HashSet<String> = HashSet::new();
    for n in get(&graph, "nodes").as_array().unwrap() {
        need(
            !n.is_null()
                && get(n, "exchangeId").is_string()
                && !ids.contains(get(n, "exchangeId").as_str().unwrap())
                && [&first, &other].iter().any(|e| {
                    get(e, "exchangeId") == get(n, "exchangeId")
                        && get(e, "marketId") == get(n, "marketId")
                }),
            "FOREIGN_GRAPH_NODE",
        )?;
        ids.insert(get(n, "exchangeId").as_str().unwrap().to_string());
    }
    need(
        !collateral.is_null()
            && collateral
                .as_object()
                .map(|o| o.keys().cloned().collect::<Vec<String>>().join("|") == "data")
                .unwrap_or(false)
            && get(&collateral, "data").as_array().is_some_and(|a| a.is_empty()),
        "NON_NONE_COLLATERAL",
    )?;
    let preceding = if !get(v, "otherBookReceipt").is_null() {
        instant_v(get(get(v, "otherBookReceipt"), "receivedAt"))?
    } else {
        let pages = get(get(v, "after"), "scopedOrdersPagesByExchange")
            .get(get(&other, "exchangeId").as_str().unwrap_or(""))
            .and_then(V::as_array)
            .cloned()
            .unwrap_or_default();
        let last = match pages.last() {
            Some(r) => r.clone(),
            None => return Err("\u{0}TYPE_ERROR".to_string()),
        };
        instant_v(get(&last, "receivedAt"))?
    };
    need(
        instant_v(get(get(v, "collateralReceipt"), "requestStartedAt"))?
            >= instant_v(get(get(v, "graphReceipt"), "receivedAt"))?
            && instant_v(get(get(v, "graphReceipt"), "requestStartedAt"))? >= preceding,
        "NONE_CAUSALITY",
    )?;
    Ok(json!({
        "schemaVersion": 1,
        "mode": "fresh_owned_pair_none_read_confirmation_v1",
        "graphReceipt": get(v, "graphReceipt"),
        "collateralReceipt": get(v, "collateralReceipt"),
        "sourceHash": payload_hash(&json!({
            "graph": get(get(v, "graphReceipt"), "sourceBodyHash"),
            "collateral": get(get(v, "collateralReceipt"), "sourceBodyHash"),
        }))?,
        "confirmedAt": get(get(v, "collateralReceipt"), "receivedAt"),
        "maximumAgeMs": 5000,
        "executionAuthority": false,
    }))
}

fn same_pending_economics(inner: &Rc<RefCell<Inner>>, before: &V, current: &V) -> Result<bool, String> {
    let strip = |r: &V| -> V {
        let mut b = r.as_object().cloned().unwrap_or_default();
        b.shift_remove("reservationHash");
        b.shift_remove("unresolvedIntentIds");
        V::Object(b)
    };
    let hash_valid = |r: &V| -> Result<bool, String> {
        let mut b = r.as_object().cloned().unwrap_or_default();
        let h = b.shift_remove("reservationHash").unwrap_or(V::Null);
        Ok(hex(&h) && payload_hash(&V::Object(b))? == h.as_str().unwrap_or(""))
    };
    let o = {
        let b = inner.borrow();
        let f = b
            .deps
            .get_active_intent
            .clone()
            .unwrap_or_else(|| Rc::new(|| Ok(V::Null)));
        sync(f())?
    };
    Ok(hash_valid(before)?
        && hash_valid(current)?
        && get(before, "unresolvedIntentIds").as_array().is_some_and(|a| a.is_empty())
        && get(current, "unresolvedIntentIds").as_array().is_some_and(|a| a.len() == 1)
        && &get(current, "unresolvedIntentIds")[0] == get(&o, "intentId")
        && same(&strip(before), &strip(current))?)
}

impl OwnedNoFreshNoneHedgeEvidenceBridge {
    /// createOwnedNoFreshNoneHedgeEvidenceBridge({api,receiptOf,scope,
    /// firstExchangeId,getOwned,getPairCensus,getFunding,getGlobalRisk,
    /// getAccountLease,canRead,hasReadHeadroom,getActiveIntent,getOrderBinding,
    /// now})
    pub fn create(deps: NoneHedgeBridgeV1Deps) -> Result<Self, String> {
        let scope = deps.scope.clone();
        let first_exchange_id = deps.first_exchange_id.clone();
        need(
            get(&scope, "ownerId").as_str() == Some("Codex")
                && get(&scope, "runId").is_string()
                && get(&scope, "tournamentId").as_str() == Some("bda92870-621e-47b0-bc3c-3602c5c26f55")
                && get(&scope, "tournamentSlug").is_string()
                && id_str(&first_exchange_id),
            "SCOPE",
        )?;
        let opening = sync((deps.get_owned)(&json!(first_exchange_id)))?;
        let legs = get(get(&opening, "package"), "legs")
            .as_array()
            .cloned()
            .unwrap_or_default();
        let first = legs
            .iter()
            .find(|e| get(e, "exchangeId").as_str() == Some(first_exchange_id.as_str()))
            .cloned();
        let other = legs
            .iter()
            .find(|e| get(e, "exchangeId").as_str() != Some(first_exchange_id.as_str()))
            .cloned();
        need(
            first.is_some()
                && id_str(get(first.as_ref().unwrap(), "marketId").as_str().unwrap_or(""))
                && other.is_some()
                && id_str(get(other.as_ref().unwrap(), "marketId").as_str().unwrap_or(""))
                && id_str(get(other.as_ref().unwrap(), "exchangeId").as_str().unwrap_or(""))
                && get(other.as_ref().unwrap(), "marketId") != get(first.as_ref().unwrap(), "marketId")
                && legs.len() == 2
                && hex(get(get(&opening, "proof"), "verificationHash"))
                && get(get(&opening, "proof"), "exchangeId").as_str() == Some(first_exchange_id.as_str())
                && get(get(&opening, "proof"), "marketId") == get(first.as_ref().unwrap(), "marketId")
                && js_int(get(get(&opening, "proof"), "remainingOwnedQuantity")).map(|q| q > 0).unwrap_or(false)
                && (get(&scope, "ownedAllMarketId").is_null()
                    || get(&scope, "ownedAllMarketId") == get(first.as_ref().unwrap(), "marketId")),
            "ORIGINAL_SEED",
        )?;
        let original_owned_hash = payload_hash(&opening)?;
        Ok(Self {
            inner: Rc::new(RefCell::new(Inner {
                deps,
                scope,
                first_exchange_id,
                first: first.unwrap(),
                other: other.unwrap(),
                original_owned_hash,
                cache: None,
                job_binding: None,
                busy: false,
                closed: false,
                reads: 0,
                last_end: None,
                last_failure: None,
                issued: HashMap::new(),
                watermarks: HashMap::new(),
                invocations: HashSet::new(),
            })),
        })
    }

    /// authorizes(spec)
    pub fn authorizes(&self, spec: &V, before_dispatch: &Guard, before_network: &Guard) -> bool {
        let r: Result<bool, String> = (|| {
            local(&self.inner)?;
            let key = before_dispatch as *const Guard as *const () as usize;
            let p = self.inner.borrow().issued.get(&key).cloned();
            let p = match p {
                Some(p) => p,
                None => return Ok(false),
            };
            Ok(get(spec, "method").as_str() == Some("GET")
                && get(spec, "endpoint") == get(&p, "endpoint")
                && (before_network as *const Guard as *const () as usize)
                    == get(&p, "beforeNetworkPtr").as_u64().unwrap_or(0) as usize
                && spec.get("body").is_none()
                && spec.get("bodyJson").is_none()
                && spec.get("headers").is_none())
        })();
        r.unwrap_or(false)
    }

    fn read(inner: &Rc<RefCell<Inner>>, endpoint: &str) -> Result<V, String> {
        local(inner)?;
        {
            let b = inner.borrow();
            need(b.reads < 10 && (b.deps.has_read_headroom)(10 - b.reads)?, "HEADROOM")?;
        }
        let admitted = Rc::new(Cell::new(false));
        let network = Rc::new(Cell::new(false));
        let inner2 = inner.clone();
        let admitted2 = admitted.clone();
        let before_dispatch = move |_p: &V, _s: &V| -> Result<bool, OrderTransportError> {
            (|| -> Result<bool, String> {
                local(&inner2)?;
                {
                    let b = inner2.borrow();
                    need(
                        !admitted2.get() && (b.deps.has_read_headroom)(10 - b.reads)?,
                        "ADMISSION",
                    )?;
                }
                admitted2.set(true);
                Ok(true)
            })()
            .map_err(ote)
        };
        let inner3 = inner.clone();
        let admitted3 = admitted.clone();
        let network2 = network.clone();
        let before_network = move |_p: &V, _s: &V| -> Result<bool, OrderTransportError> {
            (|| -> Result<bool, String> {
                local(&inner3)?;
                {
                    let b = inner3.borrow();
                    need(
                        admitted3.get()
                            && !network2.get()
                            && (b.deps.has_read_headroom)(9 - b.reads)?,
                        "NETWORK",
                    )?;
                }
                network2.set(true);
                Ok(true)
            })()
            .map_err(ote)
        };
        let bd_ptr = &before_dispatch as &Guard as *const Guard as *const () as usize;
        let bn_ptr = &before_network as &Guard as *const Guard as *const () as usize;
        let spec = json!({
            "method": "GET",
            "endpoint": endpoint,
        });
        inner.borrow_mut().issued.insert(
            bd_ptr,
            json!({"endpoint": endpoint, "beforeNetworkPtr": bn_ptr as u64}),
        );
        let start = (inner.borrow().deps.now)();
        let result = (|| -> Result<V, String> {
            let response = (inner.borrow().deps.api.dispatch)(&spec, Some(&before_dispatch), Some(&before_network))
                .map_err(|e| e.code)?;
            local(inner)?;
            need(
                admitted.get()
                    && network.get()
                    && get(&response, "endpoint").as_str() == Some(endpoint)
                    && js_int(get(&response, "status")) == Some(200),
                "RESPONSE",
            )?;
            let r = {
                let b = inner.borrow();
                sync((b.deps.receipt_of)(&response, &json!({"method": "GET"})))?
            };
            let n = (inner.borrow().deps.now)() as i64;
            let age = if endpoint.contains("/nodes?") { 300_000 } else { 20_000 };
            let parsed = raw(&r, endpoint, n, age)?;
            need(
                instant_v(get(&r, "requestStartedAt"))? >= start as i64
                    && instant_v(get(&r, "requestStartedAt"))?
                        >= instant_v(get(&response, "requestStartedAt"))?
                    && instant_v(get(&r, "receivedAt"))? <= instant_v(get(&response, "receivedAt"))?
                    && same(&parsed.data, get(&response, "data"))?
                    && inner
                        .borrow()
                        .last_end
                        .map(|e| instant_v(get(&r, "requestStartedAt")).unwrap_or(0) >= e)
                        .unwrap_or(true),
                "SOURCE_CAUSALITY",
            )?;
            need(
                !inner.borrow().invocations.contains(get(&r, "fetchInvocationId").as_str().unwrap_or("")),
                "DUPLICATE_INVOCATION",
            )?;
            inner
                .borrow_mut()
                .invocations
                .insert(get(&r, "fetchInvocationId").as_str().unwrap_or("").to_string());
            let old = inner.borrow().watermarks.get(endpoint).copied();
            if let Some((os, oe)) = old {
                need(
                    instant_v(get(&r, "requestStartedAt"))? >= os
                        && instant_v(get(&r, "receivedAt"))? >= oe,
                    "RECEIPT_ROLLBACK",
                )?;
            }
            inner.borrow_mut().watermarks.insert(
                endpoint.to_string(),
                (instant_v(get(&r, "requestStartedAt"))?, instant_v(get(&r, "receivedAt"))?),
            );
            inner.borrow_mut().last_end = Some(instant_v(get(&r, "receivedAt"))?);
            Ok(r.clone())
        })();
        inner.borrow_mut().reads += 1;
        inner.borrow_mut().issued.remove(&bd_ptr);
        result
    }

    fn page(inner: &Rc<RefCell<Inner>>, endpoint: &str) -> Result<Vec<V>, String> {
        let r = Self::read(inner, endpoint)?;
        let p = raw(&r, endpoint, (inner.borrow().deps.now)() as i64, 20_000)?;
        let d = &p.data;
        need(
            get(d, "data").as_array().is_some_and(|a| a.len() <= 200)
                && js_int(get(get(d, "pagination"), "limit")) == Some(200)
                && get(get(d, "pagination"), "hasMore") == &json!(false)
                && get(get(d, "pagination"), "nextCursor").is_null()
                && get(get(d, "coverage"), "complete") == &json!(true)
                && (get(get(d, "coverage"), "projectedThroughSequence").is_null()
                    || js_int(get(get(d, "coverage"), "projectedThroughSequence")).map(|n| n >= 0).unwrap_or(false)),
            "PAGINATION_REQUIRES_UNSUPPORTED_EXTRA_READ",
        )?;
        let mut ids: HashSet<i64> = HashSet::new();
        for (i, row) in get(d, "data").as_array().unwrap().iter().enumerate() {
            let n = p
                .numbers
                .get(&baal_service_runtime::json_text::js_stringify(&json!(["data", i, "id"])));
            need(
                n.is_some()
                    && id_str(n.unwrap())
                    && js_int(get(row, "id")).is_some()
                    && !ids.contains(&js_int(get(row, "id")).unwrap()),
                "EXACT_ORDER_ID",
            )?;
            ids.insert(js_int(get(row, "id")).unwrap());
        }
        Ok(vec![r])
    }

    fn evidence(&self) -> Result<V, String> {
        let inner = self.inner.clone();
        local(&inner)?;
        {
            let b = inner.borrow();
            need(b.cache.is_some() && !b.busy, "CACHE_UNAVAILABLE")?;
        }
        let current = local(&inner)?;
        let (cache, scope, opening, first_ex) = {
            let b = inner.borrow();
            (
                b.cache.clone().unwrap(),
                b.scope.clone(),
                (b.deps.get_owned)(&json!(b.first_exchange_id)).ok(),
                b.first_exchange_id.clone(),
            )
        };
        let opening = opening.ok_or_else(|| "NONE_HEDGE_EVIDENCE_OWNED_SOURCE_CHANGED".to_string())?;
        let c = get(&cache, "pairCensus").clone();
        let f = get(&cache, "funding").clone();
        let g = get(&cache, "accountRisk").clone();
        let live_census = sync((inner.borrow().deps.get_pair_census)(&json!(first_ex)))?;
        let live_funding = sync((inner.borrow().deps.get_funding)())?;
        let at = (inner.borrow().deps.now)() as i64;
        let all_none_evidence = none(&inner, &cache, at)?;
        let funding_ok = (same(&live_funding, &f)?
            && get(&current, "globalLedgerHash") == get(&g, "globalLedgerHash")
            && get(&current, "controllerReservationHash") == get(&g, "controllerReservationHash"))
            || (own_pending(&inner, &current)?
                && same_pending_economics(&inner, get(&f, "record"), get(&live_funding, "record"))?
                && get(get(&live_funding, "receipt"), "reservationHash")
                    == get(get(&live_funding, "record"), "reservationHash")
                && get(get(&live_funding, "receipt"), "allocationHash")
                    == get(get(&live_funding, "record"), "allocationHash")
                && get(&current, "controllerReservationHash")
                    == get(get(&live_funding, "record"), "reservationHash")
                && get(&current, "globalLedgerHash")
                    == get(get(&live_funding, "receipt"), "globalLedgerHash")
                && same(get(&current, "reservations"), get(&g, "reservations"))?);
        need(
            get(&live_census, "sourceHash") == get(&c, "sourceHash")
                && same(get(&live_census, "matchedLots"), get(&c, "matchedLots"))?
                && funding_ok
                && get(&current, "allocationHash") == get(&g, "allocationHash"),
            "LOCAL_SOURCE_CHANGED",
        )?;
        need(
            get(&c, "counterpartOwned").is_null(),
            "COUNTERPART_NOT_FLAT",
        )?;
        need(
            get(&c, "matchedLots").as_array().is_some_and(|a| a.is_empty()),
            "ALREADY_PAIRED",
        )?;
        let result = json!({
            "scope": scope,
            "firstExchangeId": first_ex,
            "owned": opening,
            "pairCensus": c,
            "funding": f,
            "globalRisk": g,
            "account": get(&cache, "account"),
            "rootReceipts": get(&cache, "rootReceipts"),
            "otherBookReceipt": get(&cache, "otherBookReceipt"),
            "now": iso(at as f64),
        });
        let proposal = plan_baal_owned_no_hedge(&result);
        need(
            get(&proposal, "proposalReady") == &json!(true),
            &format!("PROPOSAL_{}", get(&proposal, "reason").as_str().unwrap_or("")),
        )?;
        let mut out = result.as_object().cloned().unwrap();
        out.insert("graphReceipt".into(), get(&cache, "graphReceipt").clone());
        out.insert("collateralReceipt".into(), get(&cache, "collateralReceipt").clone());
        out.insert("allNoneEvidence".into(), all_none_evidence);
        out.insert("maximumPilotQuantity".into(), json!(1));
        Ok(V::Object(out))
    }

    /// getEvidence({operation='prepare_hedge',orderId=null}={})
    pub fn get_evidence(&self, context: &V) -> Result<V, String> {
        let inner = self.inner.clone();
        let operation = get(context, "operation");
        let operation = if operation.is_null() {
            "prepare_hedge".to_string()
        } else {
            operation.as_str().unwrap_or("").to_string()
        };
        let order_id = get(context, "orderId");
        if operation == "reconcile" {
            local(&inner)?;
            let (cache, busy) = {
                let b = inner.borrow();
                (b.cache.clone(), b.busy)
            };
            let cache = match cache {
                Some(c) => c,
                None => return Err("NONE_HEDGE_EVIDENCE_RECONCILE_CACHE".into()),
            };
            need(
                get(&cache, "kind").as_str() == Some("reconcile")
                    && (order_id.is_null()
                        || *order_id == get(get(&cache, "binding"), "orderId").clone())
                    && !busy,
                "RECONCILE_CACHE",
            )?;
            let at = (inner.borrow().deps.now)() as i64;
            none(&inner, &cache, at)?;
            let mut rows: Vec<V> = Vec::new();
            rows.push(get(&cache, "order").clone());
            rows.extend(get(&cache, "fillPages").as_array().cloned().unwrap_or_default());
            let after = get(&cache, "after").clone();
            rows.push(get(&after, "cash").clone());
            rows.push(get(&after, "positions").clone());
            rows.extend(get(&after, "openOrdersPages").as_array().cloned().unwrap_or_default());
            if let Some(m) = get(&after, "scopedOrdersPagesByExchange").as_object() {
                for (_, pages) in m {
                    rows.extend(pages.as_array().cloned().unwrap_or_default());
                }
            }
            for r in &rows {
                let ep = get(r, "endpoint").as_str().unwrap_or("").to_string();
                let _ = raw(r, &ep, at, 20_000)?;
            }
            let g = local(&inner)?;
            return Ok(json!({
                "order": get(&cache, "order"),
                "fillPages": get(&cache, "fillPages"),
                "after": after,
                "globalRisk": g,
                "graphReceipt": get(&cache, "graphReceipt"),
                "collateralReceipt": get(&cache, "collateralReceipt"),
            }));
        }
        need(
            ["prepare_hedge", "place_hedge", "hedge"].contains(&operation.as_str()),
            "OPERATION",
        )?;
        if operation == "prepare_hedge" {
            let o = {
                let b = inner.borrow();
                let f = b
                    .deps
                    .get_active_intent
                    .clone()
                    .unwrap_or_else(|| Rc::new(|| Ok(V::Null)));
                sync(f())?
            };
            need(o.is_null(), "ACTIVE_INTENT_BEFORE_PREPARE")?;
        }
        self.evidence()
    }

    /// refresh()
    pub fn refresh(&self) -> Result<V, String> {
        let inner = self.inner.clone();
        {
            let b = inner.borrow();
            need(!b.busy, "BUSY")?;
        }
        inner.borrow_mut().cache = None;
        inner.borrow_mut().job_binding = None;
        local(&inner)?;
        inner.borrow_mut().busy = true;
        inner.borrow_mut().reads = 0;
        inner.borrow_mut().last_end = None;
        inner.borrow_mut().last_failure = None;
        inner.borrow_mut().invocations.clear();
        let result = (|| -> Result<V, String> {
            let (first, other, scope) = {
                let b = inner.borrow();
                (b.first.clone(), b.other.clone(), b.scope.clone())
            };
            let mut root_receipts: Vec<V> = Vec::new();
            for leg in [&first, &other] {
                let receipt = Self::read(
                    &inner,
                    &format!(
                        "/markets/{}/nodes?tournamentId={}",
                        get(leg, "marketId").as_str().unwrap_or(""),
                        get(&scope, "tournamentId").as_str().unwrap_or("")
                    ),
                )?;
                let mut o = leg.as_object().cloned().unwrap();
                o.insert("receipt".into(), receipt);
                root_receipts.push(V::Object(o));
            }
            let slug = get(&scope, "tournamentSlug").as_str().unwrap_or("").to_string();
            let tid = get(&scope, "tournamentId").as_str().unwrap_or("").to_string();
            let cash = Self::read(&inner, &format!("/tournaments/{slug}"))?;
            let positions = Self::read(&inner, &format!("/tournaments/{slug}/portfolio/positions"))?;
            let open_orders_pages = Self::page(
                &inner,
                &format!("/orders?status=open&limit=200&tournamentId={tid}"),
            )?;
            let mut scoped_pages = serde_json::Map::new();
            for leg in [&first, &other] {
                scoped_pages.insert(
                    get(leg, "exchangeId").as_str().unwrap_or("").to_string(),
                    V::Array(Self::page(
                        &inner,
                        &format!(
                            "/orders?status=all&exchangeId={}&limit=200&tournamentId={tid}",
                            get(leg, "exchangeId").as_str().unwrap_or("")
                        ),
                    )?),
                );
            }
            let account = json!({
                "cash": cash,
                "positions": positions,
                "openOrdersPages": open_orders_pages,
                "scopedOrdersPagesByExchange": V::Object(scoped_pages),
            });
            let pair_census = sync((inner.borrow().deps.get_pair_census)(&json!(inner.borrow().first_exchange_id)))?;
            let funding = sync((inner.borrow().deps.get_funding)())?;
            let account_risk = local(&inner)?;
            let other_book_receipt = Self::read(
                &inner,
                &format!(
                    "/markets/{}/orderbook?tournamentId={tid}&depth=20",
                    get(&other, "marketId").as_str().unwrap_or("")
                ),
            )?;
            let graph_endpoint = format!(
                "/relationships/graph?marketId={}&depth=4&tournamentId={tid}",
                get(&first, "marketId").as_str().unwrap_or("")
            );
            let collateral_endpoint = format!("/portfolio/collateral?tournamentId={tid}");
            let graph_receipt = Self::read(&inner, &graph_endpoint)?;
            let collateral_receipt = Self::read(&inner, &collateral_endpoint)?;
            inner.borrow_mut().cache = Some(json!({
                "rootReceipts": root_receipts,
                "account": account,
                "otherBookReceipt": other_book_receipt,
                "graphReceipt": graph_receipt,
                "collateralReceipt": collateral_receipt,
                "pairCensus": pair_census,
                "funding": funding,
                "accountRisk": account_risk,
            }));
            Ok(V::Null)
        })();
        inner.borrow_mut().busy = false;
        match result {
            Ok(_) => self.evidence(),
            Err(e) => {
                inner.borrow_mut().cache = None;
                let failure = if !e.starts_with('\u{0}')
                    && e.len() > "NONE_HEDGE_EVIDENCE_".len()
                    && e.starts_with("NONE_HEDGE_EVIDENCE_")
                    && e["NONE_HEDGE_EVIDENCE_".len()..]
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b == b'_')
                {
                    e.clone()
                } else {
                    "NONE_HEDGE_EVIDENCE_READ_FAILED".to_string()
                };
                inner.borrow_mut().last_failure = Some(failure.clone());
                Err(failure)
            }
        }
    }

    /// refreshReconciliation(orderId)
    pub fn refresh_reconciliation(&self, order_id: &V) -> Result<V, String> {
        let inner = self.inner.clone();
        {
            let b = inner.borrow();
            need(!b.busy, "BUSY")?;
        }
        need(
            order_id.as_str().is_some_and(id_str),
            "KNOWN_ORDER",
        )?;
        let order_id = order_id.as_str().unwrap().to_string();
        inner.borrow_mut().cache = None;
        let job_binding = {
            let b = inner.borrow();
            let f = b
                .deps
                .get_order_binding
                .clone()
                .unwrap_or_else(|| Rc::new(|_| Ok(V::Null)));
            sync(f(&json!(order_id)))?
        };
        inner.borrow_mut().job_binding = Some(job_binding.clone());
        {
            let b = inner.borrow();
            need(
                get(&job_binding, "orderId").as_str() == Some(order_id.as_str())
                    && js_int(get(get(&job_binding, "body"), "quantity")) == Some(1)
                    && get(get(&job_binding, "body"), "exchangeId") == get(&b.other, "exchangeId")
                    && get(&job_binding, "bodyHash").as_str()
                        == Some(payload_hash(get(&job_binding, "body"))?.as_str()),
                "OWN_ORDER_BINDING",
            )?;
        }
        local(&inner)?;
        inner.borrow_mut().busy = true;
        inner.borrow_mut().reads = 0;
        inner.borrow_mut().last_end = None;
        inner.borrow_mut().invocations.clear();
        let result = (|| -> Result<(), String> {
            let (other, scope) = {
                let b = inner.borrow();
                (b.other.clone(), b.scope.clone())
            };
            let tid = get(&scope, "tournamentId").as_str().unwrap_or("").to_string();
            let slug = get(&scope, "tournamentSlug").as_str().unwrap_or("").to_string();
            let order = Self::read(&inner, &format!("/orders/{order_id}"))?;
            let fill_pages = Self::page(&inner, &format!("/orders/{order_id}/fills?limit=200"))?;
            let cash = Self::read(&inner, &format!("/tournaments/{slug}"))?;
            let positions = Self::read(&inner, &format!("/tournaments/{slug}/portfolio/positions"))?;
            let open_orders_pages = Self::page(
                &inner,
                &format!("/orders?status=open&limit=200&tournamentId={tid}"),
            )?;
            let mut scoped_pages = serde_json::Map::new();
            let (first, _o) = {
                let b = inner.borrow();
                (b.first.clone(), b.other.clone())
            };
            for leg in [&first, &other] {
                scoped_pages.insert(
                    get(leg, "exchangeId").as_str().unwrap_or("").to_string(),
                    V::Array(Self::page(
                        &inner,
                        &format!(
                            "/orders?status=all&exchangeId={}&limit=200&tournamentId={tid}",
                            get(leg, "exchangeId").as_str().unwrap_or("")
                        ),
                    )?),
                );
            }
            let scoped_other = scoped_pages
                .get(get(&other, "exchangeId").as_str().unwrap_or(""))
                .cloned()
                .unwrap_or(V::Null);
            let after = json!({
                "cash": cash,
                "positions": positions,
                "openOrdersPages": open_orders_pages,
                "scopedOrdersPagesByExchange": V::Object(scoped_pages),
                "scopedOrdersPages": scoped_other,
            });
            let graph_endpoint = format!(
                "/relationships/graph?marketId={}&depth=4&tournamentId={tid}",
                get(&first, "marketId").as_str().unwrap_or("")
            );
            let collateral_endpoint = format!("/portfolio/collateral?tournamentId={tid}");
            let graph_receipt = Self::read(&inner, &graph_endpoint)?;
            let collateral_receipt = Self::read(&inner, &collateral_endpoint)?;
            inner.borrow_mut().cache = Some(json!({
                "kind": "reconcile",
                "binding": job_binding,
                "order": order,
                "fillPages": fill_pages,
                "after": after,
                "graphReceipt": graph_receipt,
                "collateralReceipt": collateral_receipt,
            }));
            let at = (inner.borrow().deps.now)() as i64;
            let cache = inner.borrow().cache.clone().unwrap();
            none(&inner, &cache, at)?;
            Ok(())
        })();
        inner.borrow_mut().busy = false;
        inner.borrow_mut().job_binding = None;
        match result {
            Ok(()) => {
                local(&inner)?;
                let (cache, now_v) = {
                    let b = inner.borrow();
                    (b.cache.clone().unwrap(), (b.deps.now)() as i64)
                };
                none(&inner, &cache, now_v)?;
                let g = local(&inner)?;
                Ok(json!({
                    "order": get(&cache, "order"),
                    "fillPages": get(&cache, "fillPages"),
                    "after": get(&cache, "after"),
                    "globalRisk": g,
                    "graphReceipt": get(&cache, "graphReceipt"),
                    "collateralReceipt": get(&cache, "collateralReceipt"),
                }))
            }
            Err(e) => {
                inner.borrow_mut().cache = None;
                let failure = if e.starts_with('\u{0}') {
                    "NONE_HEDGE_EVIDENCE_RECONCILE_FAILED".to_string()
                } else {
                    e.clone()
                };
                inner.borrow_mut().last_failure = Some(failure.clone());
                Err(failure)
            }
        }
    }

    /// status()
    pub fn status(&self) -> V {
        let b = self.inner.borrow();
        json!({
            "mode": "owned_no_fresh_none_hedge_evidence_bridge_v1",
            "first": b.first,
            "other": b.other,
            "busy": b.busy,
            "closed": b.closed,
            "cached": b.cache.is_some(),
            "maximumReads": 10,
            "lastReadCount": b.reads,
            "lastFailure": b.last_failure,
            "executionAuthority": false,
            "maximumPilotQuantity": 1,
            "additionalApiClients": 0,
        })
    }

    /// clear()
    pub fn clear(&self) {
        self.inner.borrow_mut().cache = None;
    }

    /// close()
    pub fn close(&self) -> Result<(), String> {
        let mut b = self.inner.borrow_mut();
        need(!b.busy, "BUSY")?;
        b.cache = None;
        b.closed = true;
        b.watermarks.clear();
        Ok(())
    }
}
