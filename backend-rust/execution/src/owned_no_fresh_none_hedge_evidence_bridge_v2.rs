// execution/owned-no-fresh-none-hedge-evidence-bridge-v2.mjs — read-only
// shared-client wrapper. Current custody is supplied by the verified
// coordinator journal, not a stale original inventory snapshot or account marks.
//
// Port notes (same conventions as owned_no_fresh_none_hedge_evidence_bridge_v1):
// - `api.dispatch(spec)` -> transport::Dispatch (spec + guard args).
// - WeakMap permits -> issued map keyed by the beforeDispatch guard pointer.
// - `e.code`-style coded errors are bare Strings; foreign errors propagate.
// - `legs` ordering is insertion order from `options.pairLegs`/opening package.
use crate::baal_scaled_owned_no_hedge_proposal_v1::plan_baal_owned_no_hedge;
use crate::baal_scaled_profitable_exit_signal_v2::plan_baal_scaled_profitable_exit_v2;
use crate::core::payload_hash;
use crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::owned_no_fresh_none_hedge_evidence_bridge_v1::{
    NoneHedgeBridgeApi, OwnedNoFreshNoneHedgeEvidenceBridge,
};
use crate::receipts::parse_number_lexemes;
use crate::transport::{Dispatch, Guard, OrderTransportError};
use baal_service_runtime::fixed::{instant, price};
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use serde_json::{json, Value as V};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("COMBINED_NONE_EVIDENCE_{code}"))
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
fn js_str(v: &V) -> String {
    v.as_str()
        .map(|x| x.to_string())
        .or_else(|| v.as_i64().map(|x| x.to_string()))
        .or_else(|| v.as_u64().map(|x| x.to_string()))
        .or_else(|| v.as_f64().map(|x| x.to_string()))
        .unwrap_or_default()
}
fn sync(v: Result<V, String>) -> Result<V, String> {
    v.map(|x| x.clone())
}

pub struct NoneHedgeBridgeV2Deps {
    pub api: NoneHedgeBridgeApi,
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub scope: V,
    /// options.firstExchangeId
    pub first_exchange_id: String,
    /// options.pairLegs — falls back to opening.package.legs.
    pub pair_legs: Option<V>,
    pub get_owned: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub get_pair_census: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub get_funding: Rc<dyn Fn() -> Result<V, String>>,
    pub get_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub can_read: Rc<dyn Fn() -> bool>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> Result<bool, String>>,
    pub get_active_intent: Option<Rc<dyn Fn() -> Result<V, String>>>,
    pub get_order_binding: Option<Rc<dyn Fn(&V) -> Result<V, String>>>,
    /// refreshFundingAfterAccount — optional await hook; the return value is
    /// ignored by the JS caller.
    pub refresh_funding_after_account: Option<Rc<dyn Fn() -> Result<(), String>>>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct Inner {
    deps: NoneHedgeBridgeV2Deps,
    scope: V,
    first_exchange_id: String,
    legs: Vec<V>,
    hedge: Option<OwnedNoFreshNoneHedgeEvidenceBridge>,
    hedge_packet: Option<V>,
    cache: Option<V>,
    job: Option<V>,
    total_read_count: i64,
    busy: bool,
    closed: bool,
    reads: i64,
    maximum_reads: i64,
    last_end: Option<i64>,
    issued: HashMap<usize, V>,
    watermarks: HashMap<String, (i64, i64)>,
    invocations: HashSet<String>,
}

pub struct OwnedNoFreshNoneHedgeEvidenceBridgeV2 {
    inner: Rc<RefCell<Inner>>,
}

struct RawOut {
    data: V,
    numbers: BTreeMap<String, String>,
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

/// raw(r, endpoint, age=20000)
fn raw(inner: &Rc<RefCell<Inner>>, r: &V, endpoint: &str, age: i64) -> Result<RawOut, String> {
    need(
        !r.is_null()
            && get(r, "method").as_str() == Some("GET")
            && get(r, "endpoint").as_str() == Some(endpoint)
            && js_int(get(r, "status")) == Some(200)
            && get(r, "requestBodyHash").is_null()
            && get(r, "bodyText").as_str().is_some_and(|s| s.len() <= 8 * 1024 * 1024)
            && get(r, "sourceBodyHash").as_str()
                == Some(sha256(get(r, "bodyText").as_str().unwrap_or("").as_bytes()).as_str())
            && get(r, "fetchInvocationId").as_str().is_some_and(|s| !s.is_empty())
            && r.as_object().map(|o| o.keys().all(|k| RAW_FIELDS.contains(&k.as_str()))).unwrap_or(false),
        "RAW_SOURCE",
    )?;
    let start = instant_v(get(r, "requestStartedAt"))?;
    let end = instant_v(get(r, "receivedAt"))?;
    let n = (inner.borrow().deps.now)() as i64;
    need(start <= end && end <= n && n - start <= age, "SOURCE_AGE")?;
    let (data, numbers) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap())?;
    Ok(RawOut { data, numbers })
}

impl OwnedNoFreshNoneHedgeEvidenceBridgeV2 {
    /// createOwnedNoFreshNoneHedgeEvidenceBridgeV2(options)
    pub fn create(deps: NoneHedgeBridgeV2Deps) -> Result<Self, String> {
        let scope = deps.scope.clone();
        let first_exchange_id = deps.first_exchange_id.clone();
        let first = sync((deps.get_owned)(&json!(first_exchange_id)))?;
        let legs: Vec<V> = match &deps.pair_legs {
            Some(v) => v.as_array().cloned().unwrap_or_default(),
            None => get(get(&first, "package"), "legs")
                .as_array()
                .cloned()
                .unwrap_or_default(),
        };
        need(
            get(&scope, "ownerId").as_str() == Some("Codex")
                && legs.len() == 2
                && legs
                    .iter()
                    .map(|e| get(e, "exchangeId").as_str().unwrap_or("").to_string())
                    .collect::<HashSet<_>>()
                    .len()
                    == 2
                && legs
                    .iter()
                    .map(|e| get(e, "marketId").as_str().unwrap_or("").to_string())
                    .collect::<HashSet<_>>()
                    .len()
                    == 2
                && legs.iter().all(|e| {
                    id_str(get(e, "marketId").as_str().unwrap_or(""))
                        && id_str(get(e, "exchangeId").as_str().unwrap_or(""))
                }),
            "APPROVED_PAIR",
        )?;
        Ok(Self {
            inner: Rc::new(RefCell::new(Inner {
                deps,
                scope,
                first_exchange_id,
                legs,
                hedge: None,
                hedge_packet: None,
                cache: None,
                job: None,
                total_read_count: 0,
                busy: false,
                closed: false,
                reads: 0,
                maximum_reads: 0,
                last_end: None,
                issued: HashMap::new(),
                watermarks: HashMap::new(),
                invocations: HashSet::new(),
            })),
        })
    }

    fn active(inner: &Rc<RefCell<Inner>>) -> Result<V, String> {
        let b = inner.borrow();
        let f = b
            .deps
            .get_active_intent
            .clone()
            .unwrap_or_else(|| Rc::new(|| Ok(V::Null)));
        sync(f())
    }
    fn fund(inner: &Rc<RefCell<Inner>>) -> Result<V, String> {
        let b = inner.borrow();
        sync((b.deps.get_funding)())
    }

    fn identity(inner: &Rc<RefCell<Inner>>) -> Result<(), String> {
        {
            let b = inner.borrow();
            need(!b.closed && (b.deps.can_read)(), "STOPPED")?;
        }
        let l = {
            let b = inner.borrow();
            sync((b.deps.get_account_lease)())?
        };
        let (scope, manual_review, cooldown, n) = {
            let b = inner.borrow();
            (b.scope.clone(), b.deps.api.manual_review, b.deps.api.cooldown, (b.deps.now)())
        };
        need(
            get(&l, "ownerId") == get(&scope, "ownerId")
                && get(&l, "tournamentId") == get(&scope, "tournamentId")
                && hex(get(&l, "leaseId")),
            "ACCOUNT_LEASE",
        )?;
        need(!manual_review && n >= cooldown, "BACKOFF")?;
        Ok(())
    }

    /// guard(fenceRisk=null)
    fn guard(inner: &Rc<RefCell<Inner>>, fence_risk: Option<&V>) -> Result<V, String> {
        Self::identity(inner)?;
        let g = match fence_risk {
            Some(v) => v.clone(),
            None => {
                let b = inner.borrow();
                sync((b.deps.get_global_risk)())?
            }
        };
        let (scope, ctx, n) = {
            let b = inner.borrow();
            (
                b.scope.clone(),
                b.job.clone().or_else(|| b.cache.clone()).unwrap_or(V::Null),
                (b.deps.now)(),
            )
        };
        let a = Self::active(inner)?;
        need(
            get(&g, "ownerId") == get(&scope, "ownerId")
                && get(&g, "runId") == get(&scope, "runId")
                && get(&g, "tournamentId") == get(&scope, "tournamentId")
                && get(&g, "complete") == &json!(true)
                && get(&g, "eventScopeComplete") == &json!(true)
                && get(&g, "soleCollector") == &json!(true)
                && get(&g, "stopped") == &json!(false)
                && hex(get(&g, "sourceHash"))
                && get(&g, "unresolvedIntentIds").is_array()
                && instant_v(get(&g, "at"))? <= n as i64
                && n as i64 - instant_v(get(&g, "at"))? <= 20_000,
            "GLOBAL_SCOPE",
        )?;
        let prepared_binding = get(&ctx, "preparedBinding");
        let prepared_binding = if prepared_binding.is_null() { None } else { Some(prepared_binding.clone()) };
        if let Some(pb) = &prepared_binding {
            if get(&a, "intentId") == get(pb, "intentId")
                && get(&a, "orderId").is_null()
                && js_int(if !get(&a, "actualFetchCount").is_null() {
                    get(&a, "actualFetchCount")
                } else {
                    get(&a, "fetchCount")
                }) == Some(0)
            {
                need(
                    instant_v(get(get(&a, "body"), "expirationDate"))? > n as i64,
                    "PREPARED_EXPIRED",
                )?;
            }
        }
        let leg = get(&ctx, "leg");
        let kind = get(&ctx, "kind").as_str().unwrap_or("").to_string();
        let pending_own = !ctx.is_null()
            && js_truthy(Some(&a))
            && get(&a, "intentId").is_string()
            && get(&a, "bodyHash").as_str() == Some(payload_hash(get(&a, "body"))?.as_str())
            && get(get(&a, "body"), "exchangeId") == get(leg, "exchangeId")
            && get(get(&a, "body"), "tournamentId") == get(&scope, "tournamentId")
            && get(get(&a, "body"), "side").as_str() == Some("no")
            && (get(get(&a, "body"), "action").as_str() == Some("sell")
                || (kind == "hedgeFinal"
                    && get(get(&a, "body"), "action").as_str() == Some("buy")
                    && js_int(get(get(&a, "body"), "quantity")) == Some(1)))
            && ["prepared", "dispatching"].contains(&get(&a, "phase").as_str().unwrap_or(""))
            && get(&a, "orderId").is_null()
            && js_int(if !get(&a, "actualFetchCount").is_null() {
                get(&a, "actualFetchCount")
            } else {
                get(&a, "fetchCount")
            }) == Some(0)
            && js_int(get(get(&a, "body"), "quantity")).is_some()
            && js_int(get(get(&a, "body"), "quantity")).unwrap() > 0
            && js_int(get(get(&a, "body"), "quantity")).unwrap()
                <= if kind == "hedgeFinal" {
                    1
                } else {
                    js_int(get(&get(&get(&ctx, "owned"), "proof"), "remainingOwnedQuantity"))
                        .unwrap_or(i64::MIN)
                }
            && instant_v(get(get(&a, "body"), "expirationDate"))? > n as i64
            && get(&g, "unresolvedIntentIds")
                .as_array()
                .is_some_and(|ids| ids.iter().all(|id| id == get(&a, "intentId")));
        let binding = if get(&ctx, "binding").is_null() { None } else { Some(get(&ctx, "binding").clone()) };
        let known = match &binding {
            Some(b) => {
                let f = {
                    let bb = inner.borrow();
                    bb.deps
                        .get_order_binding
                        .clone()
                        .unwrap_or_else(|| Rc::new(|_| Ok(V::Null)))
                };
                Some(sync(f(get(b, "orderId")))?)
            }
            None => None,
        };
        let cleanup = match (&binding, &known) {
            (Some(binding), Some(known)) => {
                js_truthy(Some(known))
                    && get(known, "intentId") == get(binding, "intentId")
                    && get(known, "orderId") == get(binding, "orderId")
                    && get(known, "bodyHash") == get(binding, "bodyHash")
                    && same(get(known, "body"), get(binding, "body"))?
                    && get(&g, "unresolvedIntentIds")
                        .as_array()
                        .is_some_and(|ids| ids.iter().all(|id| id == get(binding, "intentId")))
            }
            _ => false,
        };
        need(
            (get(&g, "unknownAcceptance") == &json!(false) || cleanup)
                && (get(&g, "unresolvedIntentIds").as_array().unwrap().is_empty()
                    || pending_own
                    || cleanup),
            "UNKNOWN_OR_FOREIGN_PENDING",
        )?;
        if let Some(pb) = &prepared_binding {
            let job_exists = inner.borrow().job.is_some();
            need(
                get(&a, "intentId") == get(pb, "intentId")
                    && get(&a, "bodyHash") == get(pb, "bodyHash")
                    && same(get(&a, "body"), get(pb, "body"))?
                    && (!job_exists || get(&a, "phase").as_str() == Some("prepared")),
                "PREPARED_BODY_CHANGED",
            )?;
        }
        if !get(&ctx, "owned").is_null() {
            let owned_ex = if !get(&ctx, "ownedExchangeId").is_null() {
                get(&ctx, "ownedExchangeId").clone()
            } else {
                get(leg, "exchangeId").clone()
            };
            let fresh = {
                let b = inner.borrow();
                sync((b.deps.get_owned)(&owned_ex))?
            };
            need(same(&fresh, get(&ctx, "owned"))?, "CURRENT_CUSTODY_CHANGED")?;
        }
        Ok(g)
    }

    /// none(ctx)
    fn none(inner: &Rc<RefCell<Inner>>, ctx: &V) -> Result<(), String> {
        let (scope, legs) = {
            let b = inner.borrow();
            (b.scope.clone(), b.legs.clone())
        };
        let graph_leg = if get(ctx, "graphLeg").is_null() {
            get(ctx, "leg").clone()
        } else {
            get(ctx, "graphLeg").clone()
        };
        let graph_endpoint = format!(
            "/relationships/graph?marketId={}&depth=4&tournamentId={}",
            get(&graph_leg, "marketId").as_str().unwrap_or(""),
            get(&scope, "tournamentId").as_str().unwrap_or("")
        );
        let collateral_endpoint = format!(
            "/portfolio/collateral?tournamentId={}",
            get(&scope, "tournamentId").as_str().unwrap_or("")
        );
        let graph = raw(inner, get(ctx, "graphReceipt"), &graph_endpoint, 5_000)?.data;
        let collateral = raw(inner, get(ctx, "collateralReceipt"), &collateral_endpoint, 5_000)?.data;
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
                && get(&graph, "nodes").as_array().is_some_and(|a| a.len() <= 2)
                && get(&graph, "nodes").as_array().unwrap().iter().all(|n| {
                    legs.iter().any(|e| {
                        get(e, "marketId") == get(n, "marketId")
                            && get(e, "exchangeId") == get(n, "exchangeId")
                    })
                })
                && get(&graph, "nodes")
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|n| get(n, "exchangeId").as_str().unwrap_or("").to_string())
                            .collect::<HashSet<_>>()
                            .len()
                            == a.len()
                    })
                    .unwrap_or(false),
            "NON_NONE_GRAPH",
        )?;
        need(
            !collateral.is_null()
                && collateral
                    .as_object()
                    .map(|o| o.keys().cloned().collect::<Vec<String>>().join("|") == "data")
                    .unwrap_or(false)
                && get(&collateral, "data").as_array().is_some_and(|a| a.is_empty()),
            "NON_NONE_COLLATERAL",
        )?;
        let preceding = if !get(ctx, "book").is_null() {
            instant_v(get(get(ctx, "book"), "receivedAt"))?
        } else {
            let second_ex = get(&legs[1], "exchangeId").as_str().unwrap_or("").to_string();
            let pages = get(get(ctx, "after"), "scopedOrdersPagesByExchange")
                .get(&second_ex)
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
            instant_v(get(get(ctx, "graphReceipt"), "requestStartedAt"))? >= preceding
                && instant_v(get(get(ctx, "collateralReceipt"), "requestStartedAt"))?
                    >= instant_v(get(get(ctx, "graphReceipt"), "receivedAt"))?,
            "NONE_AFTER_OWN_SOURCE",
        )?;
        Ok(())
    }

    fn read(inner: &Rc<RefCell<Inner>>, endpoint: &str) -> Result<V, String> {
        Self::identity(inner)?;
        {
            let b = inner.borrow();
            need(
                b.reads < b.maximum_reads && (b.deps.has_read_headroom)(b.maximum_reads - b.reads)?,
                "READ_BOUND",
            )?;
        }
        let admitted = Rc::new(Cell::new(false));
        let network = Rc::new(Cell::new(false));
        let inner2 = inner.clone();
        let admitted2 = admitted.clone();
        let before_dispatch = move |_p: &V, _s: &V| -> Result<bool, OrderTransportError> {
            (|| -> Result<bool, String> {
                Self::guard(&inner2, None)?;
                {
                    let b = inner2.borrow();
                    need(
                        !admitted2.get()
                            && (b.deps.has_read_headroom)(b.maximum_reads - b.reads)?,
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
                Self::guard(&inner3, None)?;
                {
                    let b = inner3.borrow();
                    need(
                        admitted3.get()
                            && !network2.get()
                            && (b.deps.has_read_headroom)(b.maximum_reads - b.reads - 1)?,
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
        let spec = json!({"method": "GET", "endpoint": endpoint});
        inner.borrow_mut().issued.insert(
            bd_ptr,
            json!({"endpoint": endpoint, "beforeNetworkPtr": bn_ptr as u64}),
        );
        let began = (inner.borrow().deps.now)();
        let result = (|| -> Result<V, String> {
            let response = (inner.borrow().deps.api.dispatch)(
                &spec,
                Some(&before_dispatch),
                Some(&before_network),
            )
            .map_err(|e| e.code)?;
            Self::identity(inner)?;
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
            let p = raw(inner, &r, endpoint, 20_000)?;
            need(
                instant_v(get(&r, "requestStartedAt"))? >= began as i64
                    && instant_v(get(&r, "requestStartedAt"))?
                        >= instant_v(get(&response, "requestStartedAt"))?
                    && instant_v(get(&r, "receivedAt"))? <= instant_v(get(&response, "receivedAt"))?
                    && same(&p.data, get(&response, "data"))?
                    && inner
                        .borrow()
                        .last_end
                        .map(|e| instant_v(get(&r, "requestStartedAt")).unwrap_or(0) >= e)
                        .unwrap_or(true)
                    && !inner
                        .borrow()
                        .invocations
                        .contains(get(&r, "fetchInvocationId").as_str().unwrap_or("")),
                "RAW_CAUSALITY",
            )?;
            let old = inner.borrow().watermarks.get(endpoint).copied();
            need(
                old.map(|(os, oe)| {
                    instant_v(get(&r, "requestStartedAt")).unwrap_or(0) >= os
                        && instant_v(get(&r, "receivedAt")).unwrap_or(0) >= oe
                })
                .unwrap_or(true),
                "RAW_ROLLBACK",
            )?;
            inner.borrow_mut().watermarks.insert(
                endpoint.to_string(),
                (instant_v(get(&r, "requestStartedAt"))?, instant_v(get(&r, "receivedAt"))?),
            );
            inner
                .borrow_mut()
                .invocations
                .insert(get(&r, "fetchInvocationId").as_str().unwrap_or("").to_string());
            inner.borrow_mut().last_end = Some(instant_v(get(&r, "receivedAt"))?);
            Ok(r.clone())
        })();
        inner.borrow_mut().reads += 1;
        inner.borrow_mut().total_read_count += 1;
        inner.borrow_mut().issued.remove(&bd_ptr);
        result
    }

    fn page(inner: &Rc<RefCell<Inner>>, endpoint: &str) -> Result<Vec<V>, String> {
        let r = Self::read(inner, endpoint)?;
        let p = raw(inner, &r, endpoint, 20_000)?;
        let d = &p.data;
        need(
            get(d, "data").as_array().is_some_and(|a| a.len() <= 200)
                && js_int(get(get(d, "pagination"), "limit")) == Some(200)
                && get(get(d, "pagination"), "hasMore") == &json!(false)
                && get(get(d, "pagination"), "nextCursor").is_null()
                && get(get(d, "coverage"), "complete") == &json!(true)
                && (get(get(d, "coverage"), "projectedThroughSequence").is_null()
                    || js_int(get(get(d, "coverage"), "projectedThroughSequence"))
                        .map(|n| n >= 0)
                        .unwrap_or(false)),
            "COMPLETE_SINGLE_PAGE_REQUIRED",
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
                "EXACT_ID",
            )?;
            ids.insert(js_int(get(row, "id")).unwrap());
        }
        Ok(vec![r])
    }

    /// account(target, both)
    fn account(inner: &Rc<RefCell<Inner>>, target: &V, both: bool) -> Result<V, String> {
        let (scope, legs) = {
            let b = inner.borrow();
            (b.scope.clone(), b.legs.clone())
        };
        let slug = get(&scope, "tournamentSlug").as_str().unwrap_or("").to_string();
        let tid = get(&scope, "tournamentId").as_str().unwrap_or("").to_string();
        let cash = Self::read(inner, &format!("/tournaments/{slug}"))?;
        let positions = Self::read(inner, &format!("/tournaments/{slug}/portfolio/positions"))?;
        let open_orders_pages = Self::page(
            inner,
            &format!("/orders?status=open&limit=200&tournamentId={tid}"),
        )?;
        let mut scoped = serde_json::Map::new();
        let targets: Vec<V> = if both { legs.clone() } else { vec![target.clone()] };
        for e in &targets {
            scoped.insert(
                get(e, "exchangeId").as_str().unwrap_or("").to_string(),
                V::Array(Self::page(
                    inner,
                    &format!(
                        "/orders?status=all&exchangeId={}&limit=200&tournamentId={tid}",
                        get(e, "exchangeId").as_str().unwrap_or("")
                    ),
                )?),
            );
        }
        let scoped_target = scoped
            .get(get(target, "exchangeId").as_str().unwrap_or(""))
            .cloned()
            .unwrap_or(V::Null);
        Ok(json!({
            "cash": cash,
            "positions": positions,
            "openOrdersPages": open_orders_pages,
            "scopedOrdersPages": scoped_target,
            "scopedOrdersPagesByExchange": V::Object(scoped),
        }))
    }

    /// finishNone(ctx) — mutates ctx: graphReceipt/collateralReceipt.
    fn finish_none(inner: &Rc<RefCell<Inner>>, ctx: &mut V) -> Result<(), String> {
        let (scope, _) = {
            let b = inner.borrow();
            (b.scope.clone(), ())
        };
        let graph_leg = if get(ctx, "graphLeg").is_null() {
            get(ctx, "leg").clone()
        } else {
            get(ctx, "graphLeg").clone()
        };
        let graph_receipt = Self::read(
            inner,
            &format!(
                "/relationships/graph?marketId={}&depth=4&tournamentId={}",
                get(&graph_leg, "marketId").as_str().unwrap_or(""),
                get(&scope, "tournamentId").as_str().unwrap_or("")
            ),
        )?;
        let collateral_receipt = Self::read(
            inner,
            &format!(
                "/portfolio/collateral?tournamentId={}",
                get(&scope, "tournamentId").as_str().unwrap_or("")
            ),
        )?;
        ctx.as_object_mut()
            .unwrap()
            .insert("graphReceipt".into(), graph_receipt);
        ctx.as_object_mut()
            .unwrap()
            .insert("collateralReceipt".into(), collateral_receipt);
        Self::none(inner, ctx)
    }

    /// sellEvidence(operation, fenceRisk=null)
    fn sell_evidence(inner: &Rc<RefCell<Inner>>, operation: &str, fence_risk: Option<&V>) -> Result<V, String> {
        let g = Self::guard(inner, fence_risk)?;
        let (cache, busy, scope, legs) = {
            let b = inner.borrow();
            (b.cache.clone(), b.busy, b.scope.clone(), b.legs.clone())
        };
        let cache = match cache {
            Some(c) => c,
            None => return Err("COMBINED_NONE_EVIDENCE_SELL_CACHE".into()),
        };
        need(get(&cache, "kind").as_str() == Some("sell") && !busy, "SELL_CACHE")?;
        Self::none(inner, &cache)?;
        if operation == "prepare_sell" {
            need(Self::active(inner)?.is_null(), "ACTIVE_BEFORE_PREPARE")?;
        }
        let f = Self::fund(inner)?;
        need(
            get(get(&f, "record"), "reservationHash") == get(&g, "controllerReservationHash")
                && get(get(&f, "receipt"), "globalLedgerHash") == get(&g, "globalLedgerHash")
                && get(get(&f, "receipt"), "reservationHash")
                    == get(get(&f, "record"), "reservationHash"),
            "CURRENT_INSTALLED_BUDGET",
        )?;
        let economic = |r: &V| -> V {
            let mut b = r.as_object().cloned().unwrap_or_default();
            b.shift_remove("reservationHash");
            b.shift_remove("unresolvedIntentIds");
            V::Object(b)
        };
        need(
            same(&economic(get(&f, "record")), &economic(get(get(&cache, "sourceFunding"), "record")))?,
            "CURRENT_FULL_ECONOMICS_CHANGED",
        )?;
        let a = Self::active(inner)?;
        need(
            get(get(&cache, "sourceRisk"), "unresolvedIntentIds")
                .as_array()
                .is_some_and(|v| v.is_empty())
                && (get(get(&f, "record"), "unresolvedIntentIds")
                    .as_array()
                    .map(|v| v.is_empty())
                    .unwrap_or(true)
                    || get(get(&f, "record"), "unresolvedIntentIds")
                        .as_array()
                        .is_some_and(|v| v.len() == 1 && &v[0] == get(&a, "intentId"))),
            "EXACT_OWN_SELL_PENDING",
        )?;
        let signal_risk = get(&cache, "sourceRisk").clone();
        let mut scope_ex = scope.as_object().cloned().unwrap();
        scope_ex.insert("exchanges".into(), V::Array(legs.clone()));
        let p = plan_baal_scaled_profitable_exit_v2(&json!({
            "scope": V::Object(scope_ex),
            "owned": get(&cache, "owned"),
            "account": get(&cache, "account"),
            "bookReceipt": get(&cache, "book"),
            "globalRisk": signal_risk,
            "now": iso((inner.borrow().deps.now)()),
        }));
        need(
            get(&p, "proposalReady") == &json!(true),
            &format!("CURRENT_FIFO_OR_PROFIT_{}", get(&p, "reason").as_str().unwrap_or("")),
        )?;
        if js_truthy(Some(&a)) {
            need(
                js_int(get(get(&a, "body"), "quantity")).unwrap_or(i64::MAX)
                    <= js_int(get(&p, "quantity")).unwrap_or(i64::MIN)
                    && price(&js_str(get(get(&a, "body"), "price")))?
                        <= price(get(&p, "limit").as_str().unwrap_or(""))?,
                "FIXED_PROFITABLE_INTENT_DEPTH",
            )?;
        }
        Ok(json!({
            "account": get(&cache, "account"),
            "book": get(&cache, "book"),
            "globalRisk": signal_risk,
            "currentGlobalRisk": g,
            "graphReceipt": get(&cache, "graphReceipt"),
            "collateralReceipt": get(&cache, "collateralReceipt"),
            "funding": f,
            "owned": get(&cache, "owned"),
        }))
    }

    /// evidence(c={})
    fn evidence(&self, c: &V) -> Result<V, String> {
        let inner = self.inner.clone();
        let op_v = get(c, "operation");
        let op = if op_v.is_null() {
            "prepare_hedge".to_string()
        } else {
            op_v.as_str().unwrap_or("").to_string()
        };
        if ["prepare_hedge", "place_hedge", "hedge"].contains(&op.as_str()) {
            let (hedge_exists, cache_kind) = {
                let b = inner.borrow();
                (
                    b.hedge.is_some(),
                    b.cache
                        .as_ref()
                        .map(|c| get(c, "kind").as_str().unwrap_or("").to_string()),
                )
            };
            need(hedge_exists || cache_kind.as_deref() == Some("hedgeFinal"), "HEDGE_CACHE")?;
            if cache_kind.as_deref() == Some("hedgeFinal") {
                let fence = get(c, "fenceRisk");
                let g = Self::guard(&inner, if fence.is_null() { None } else { Some(fence) })?;
                let cache = inner.borrow().cache.clone().unwrap();
                Self::none(&inner, &cache)?;
                let f = Self::fund(&inner)?;
                let economic = |r: &V| -> V {
                    let mut b = r.as_object().cloned().unwrap_or_default();
                    b.shift_remove("reservationHash");
                    b.shift_remove("unresolvedIntentIds");
                    V::Object(b)
                };
                need(
                    same(
                        &economic(get(&f, "record")),
                        &economic(get(get(get(&cache, "hedgePacket"), "funding"), "record")),
                    )?
                    && get(get(&f, "record"), "reservationHash")
                        == get(&g, "controllerReservationHash")
                    && get(get(&f, "receipt"), "globalLedgerHash") == get(&g, "globalLedgerHash"),
                    "CURRENT_HEDGE_ECONOMICS_CHANGED",
                )?;
                let packet = get(&cache, "hedgePacket").clone();
                let mut e = packet.as_object().cloned().unwrap_or_default();
                e.insert("graphReceipt".into(), get(&cache, "graphReceipt").clone());
                e.insert("collateralReceipt".into(), get(&cache, "collateralReceipt").clone());
                let mut ane = get(&packet, "allNoneEvidence")
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                ane.insert("graphReceipt".into(), get(&cache, "graphReceipt").clone());
                ane.insert("collateralReceipt".into(), get(&cache, "collateralReceipt").clone());
                ane.insert(
                    "sourceHash".into(),
                    json!(payload_hash(&json!({
                        "graph": get(get(&cache, "graphReceipt"), "sourceBodyHash"),
                        "collateral": get(get(&cache, "collateralReceipt"), "sourceBodyHash"),
                    }))?),
                );
                ane.insert(
                    "confirmedAt".into(),
                    get(get(&cache, "collateralReceipt"), "receivedAt").clone(),
                );
                e.insert("allNoneEvidence".into(), V::Object(ane));
                e.insert("now".into(), json!(iso((inner.borrow().deps.now)())));
                let e = V::Object(e);
                let p = plan_baal_owned_no_hedge(&e);
                need(
                    get(&p, "proposalReady") == &json!(true),
                    &format!("ORIGINAL_HEDGE_PACKET_{}", get(&p, "reason").as_str().unwrap_or("")),
                )?;
                return Ok(e);
            }
            let hedge = inner.borrow().hedge.clone();
            return hedge.unwrap().get_evidence(c);
        }
        if ["prepare_sell", "place_sell"].contains(&op.as_str()) {
            let fence = get(c, "fenceRisk");
            return Self::sell_evidence(
                &inner,
                &op,
                if fence.is_null() { None } else { Some(fence) },
            );
        }
        let order_id = get(c, "orderId");
        let (cache, busy) = {
            let b = inner.borrow();
            (b.cache.clone(), b.busy)
        };
        let cache = match cache {
            Some(v) => v,
            None => V::Null,
        };
        need(
            op == "reconcile"
                && get(&cache, "kind").as_str() == Some("reconcile")
                && !busy
                && (order_id.is_null() || *order_id == get(get(&cache, "binding"), "orderId").clone()),
            "RECONCILE_CACHE",
        )?;
        let g = Self::guard(&inner, None)?;
        Self::none(&inner, &cache)?;
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
            let _ = raw(&inner, r, &ep, 20_000)?;
        }
        Ok(json!({
            "order": get(&cache, "order"),
            "fillPages": get(&cache, "fillPages"),
            "after": after,
            "globalRisk": g,
            "graphReceipt": get(&cache, "graphReceipt"),
            "collateralReceipt": get(&cache, "collateralReceipt"),
        }))
    }

    /// getEvidence(c)
    pub fn get_evidence(&self, c: &V) -> Result<V, String> {
        self.evidence(c)
    }

    /// authorizes(spec)
    pub fn authorizes(&self, spec: &V, before_dispatch: &Guard, before_network: &Guard) -> bool {
        let r: Result<bool, String> = (|| {
            need(!self.inner.borrow().closed, "CLOSED")?;
            let key = before_dispatch as *const Guard as *const () as usize;
            let p = self.inner.borrow().issued.get(&key).cloned();
            let p = match p {
                Some(p) => p,
                None => {
                    return Ok(self
                        .inner
                        .borrow()
                        .hedge
                        .as_ref()
                        .map(|h| h.authorizes(spec, before_dispatch, before_network))
                        .unwrap_or(false));
                }
            };
            Self::identity(&self.inner)?;
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

    /// begin(ctx, max, fn)
    fn begin<F>(inner: &Rc<RefCell<Inner>>, mut ctx: V, max: i64, f: F) -> Result<V, String>
    where
        F: FnOnce(&Rc<RefCell<Inner>>, &mut V) -> Result<(), String>,
    {
        need(!inner.borrow().busy, "BUSY")?;
        inner.borrow_mut().cache = None;
        inner.borrow_mut().job = Some(ctx.clone());
        inner.borrow_mut().busy = true;
        inner.borrow_mut().reads = 0;
        inner.borrow_mut().maximum_reads = max;
        inner.borrow_mut().last_end = None;
        inner.borrow_mut().invocations.clear();
        let result = (|| -> Result<(), String> {
            Self::identity(inner)?;
            f(inner, &mut ctx)?;
            inner.borrow_mut().cache = Some(ctx.clone());
            Ok(())
        })();
        inner.borrow_mut().busy = false;
        inner.borrow_mut().job = None;
        result?;
        Ok(ctx)
    }

    /// refresh()
    pub fn refresh(&self) -> Result<V, String> {
        let inner = self.inner.clone();
        need(!inner.borrow().busy, "BUSY")?;
        inner.borrow_mut().cache = None;
        inner.borrow_mut().job = None;
        if let Some(h) = inner.borrow().hedge.as_ref() {
            let _ = h.close();
        }
        inner.borrow_mut().hedge = None;
        if inner.borrow().deps.refresh_funding_after_account.is_some() {
            let first_ex = inner.borrow().first_exchange_id.clone();
            let owned = sync((inner.borrow().deps.get_owned)(&json!(first_ex)))?;
            let legs = inner.borrow().legs.clone();
            let graph_leg = legs
                .iter()
                .find(|e| get(e, "exchangeId").as_str() == Some(first_ex.as_str()))
                .cloned();
            let leg = legs
                .iter()
                .find(|e| get(e, "exchangeId").as_str() != Some(first_ex.as_str()))
                .cloned();
            need(
                js_int(get(get(&owned, "proof"), "remainingOwnedQuantity"))
                    .map(|q| q > 0)
                    .unwrap_or(false)
                    && graph_leg.is_some()
                    && leg.is_some(),
                "CURRENT_HEDGE_SEED",
            )?;
            let graph_leg = graph_leg.unwrap();
            let leg = leg.unwrap();
            let ctx_in = json!({
                "kind": "hedgeFinal",
                "leg": leg,
                "graphLeg": graph_leg,
                "owned": owned,
                "ownedExchangeId": first_ex,
            });
            let inner_c = inner.clone();
            Self::begin(&inner, ctx_in, 10, move |inner, ctx| {
                let (scope, first_ex) = {
                    let b = inner.borrow();
                    (b.scope.clone(), b.first_exchange_id.clone())
                };
                let tid = get(&scope, "tournamentId").as_str().unwrap_or("").to_string();
                let graph_leg = get(ctx, "graphLeg").clone();
                let leg = get(ctx, "leg").clone();
                let mut root_receipts: Vec<V> = Vec::new();
                for e in [&graph_leg, &leg] {
                    let receipt = Self::read(
                        inner,
                        &format!(
                            "/markets/{}/nodes?tournamentId={tid}",
                            get(e, "marketId").as_str().unwrap_or("")
                        ),
                    )?;
                    let mut o = e.as_object().cloned().unwrap();
                    o.insert("receipt".into(), receipt);
                    root_receipts.push(V::Object(o));
                }
                let account_source = Self::account(inner, &graph_leg, true)?;
                let mut binding = scope.as_object().cloned().unwrap();
                for (k, v) in graph_leg.as_object().unwrap() {
                    binding.insert(k.clone(), v.clone());
                }
                let view = inspect_scoped_rest_no_account(&json!({
                    "account": account_source,
                    "binding": V::Object(binding),
                    "readAt": iso((inner.borrow().deps.now)()),
                }))?;
                let owned = get(ctx, "owned").clone();
                need(
                    get(&view, "positions").as_array().is_some_and(|ps| {
                        ps.iter().any(|p| {
                            get(p, "exchangeId") == get(&graph_leg, "exchangeId")
                                && get(p, "marketId") == get(&graph_leg, "marketId")
                                && js_int(get(p, "quantity"))
                                    == js_int(get(get(&owned, "proof"), "remainingOwnedQuantity"))
                                        .map(|q| -q)
                                && !js_truthy(Some(get(p, "settled")))
                        })
                    }) && !get(&view, "positions").as_array().is_some_and(|ps| {
                        ps.iter().any(|p| {
                            get(p, "exchangeId") == get(&leg, "exchangeId")
                                && js_int(get(p, "quantity")).map(|q| q != 0).unwrap_or(true)
                        })
                    }),
                    "CURRENT_SOURCE_CUSTODY",
                )?;
                {
                    let b = inner.borrow();
                    let f = b.deps.refresh_funding_after_account.clone().unwrap();
                    f()?;
                }
                let funding = Self::fund(inner)?;
                let pair_census = {
                    let b = inner.borrow();
                    sync((b.deps.get_pair_census)(&json!(first_ex)))?
                };
                let global_risk = Self::guard(inner, None)?;
                let other_book_receipt = Self::read(
                    inner,
                    &format!(
                        "/markets/{}/orderbook?tournamentId={tid}&depth=20",
                        get(&leg, "marketId").as_str().unwrap_or("")
                    ),
                )?;
                ctx.as_object_mut()
                    .unwrap()
                    .insert("otherBookReceipt".into(), other_book_receipt.clone());
                ctx.as_object_mut()
                    .unwrap()
                    .insert("book".into(), other_book_receipt.clone());
                Self::finish_none(inner, ctx)?;
                let hedge_packet = json!({
                    "scope": scope,
                    "firstExchangeId": first_ex,
                    "owned": owned,
                    "pairCensus": pair_census,
                    "funding": funding,
                    "globalRisk": global_risk,
                    "account": account_source,
                    "rootReceipts": root_receipts,
                    "otherBookReceipt": other_book_receipt,
                    "graphReceipt": get(ctx, "graphReceipt"),
                    "collateralReceipt": get(ctx, "collateralReceipt"),
                    "now": iso((inner.borrow().deps.now)()),
                    "maximumPilotQuantity": 1,
                });
                ctx.as_object_mut()
                    .unwrap()
                    .insert("hedgePacket".into(), hedge_packet);
                Ok(())
            })?;
            let packet = get(inner.borrow().cache.as_ref().unwrap(), "hedgePacket").clone();
            inner.borrow_mut().hedge_packet = Some(packet);
            return self.evidence(&json!({"operation": "prepare_hedge"}));
        }
        let (deps_first_ex, scope, api, receipt_of, get_owned, get_pair_census, get_funding, get_global_risk, get_account_lease, can_read, has_read_headroom, get_active_intent, get_order_binding, now) = {
            let b = inner.borrow();
            (
                b.first_exchange_id.clone(),
                b.scope.clone(),
                NoneHedgeBridgeApi {
                    dispatch: b.deps.api.dispatch.clone(),
                    manual_review: b.deps.api.manual_review,
                    cooldown: b.deps.api.cooldown,
                },
                b.deps.receipt_of.clone(),
                b.deps.get_owned.clone(),
                b.deps.get_pair_census.clone(),
                b.deps.get_funding.clone(),
                b.deps.get_global_risk.clone(),
                b.deps.get_account_lease.clone(),
                b.deps.can_read.clone(),
                b.deps.has_read_headroom.clone(),
                b.deps.get_active_intent.clone(),
                b.deps.get_order_binding.clone(),
                b.deps.now.clone(),
            )
        };
        let hedge = OwnedNoFreshNoneHedgeEvidenceBridge::create(crate::owned_no_fresh_none_hedge_evidence_bridge_v1::NoneHedgeBridgeV1Deps {
            api,
            receipt_of,
            scope,
            first_exchange_id: deps_first_ex,
            get_owned,
            get_pair_census,
            get_funding,
            get_global_risk,
            get_account_lease,
            can_read,
            has_read_headroom,
            get_active_intent,
            get_order_binding,
            now,
        })?;
        let refresh_result = hedge.refresh();
        let status = hedge.status();
        inner.borrow_mut().total_read_count += js_int(get(&status, "lastReadCount")).unwrap_or(0);
        let packet = refresh_result?;
        inner.borrow_mut().hedge = Some(hedge);
        inner.borrow_mut().hedge_packet = Some(packet.clone());
        Ok(packet)
    }

    /// refreshSell(leg)
    pub fn refresh_sell(&self, leg: &V) -> Result<V, String> {
        let inner = self.inner.clone();
        {
            let b = inner.borrow();
            need(b.legs.iter().any(|e| same(e, leg).unwrap_or(false)), "OWN_SELL_SCOPE")?;
        }
        let owned = {
            let b = inner.borrow();
            sync((b.deps.get_owned)(get(leg, "exchangeId")))?
        };
        need(
            get(get(&owned, "proof"), "marketId") == get(leg, "marketId")
                && get(get(&owned, "proof"), "exchangeId") == get(leg, "exchangeId")
                && js_int(get(get(&owned, "proof"), "remainingOwnedQuantity"))
                    .map(|q| q > 0)
                    .unwrap_or(false),
            "OWN_CURRENT_SELL",
        )?;
        let ctx_in = json!({
            "kind": "sell",
            "leg": leg,
            "owned": owned,
        });
        Self::begin(&inner, ctx_in, 7, |inner, ctx| {
            let scope = inner.borrow().scope.clone();
            let leg = get(ctx, "leg").clone();
            let account = Self::account(inner, &leg, false)?;
            ctx.as_object_mut().unwrap().insert("account".into(), account.clone());
            let mut binding = scope.as_object().cloned().unwrap();
            for (k, v) in leg.as_object().unwrap() {
                binding.insert(k.clone(), v.clone());
            }
            let _ = inspect_scoped_rest_no_account(&json!({
                "account": account,
                "binding": V::Object(binding),
                "readAt": iso((inner.borrow().deps.now)()),
            }))?;
            {
                let b = inner.borrow();
                if let Some(f) = b.deps.refresh_funding_after_account.clone() {
                    f()?;
                }
            }
            let source_funding = Self::fund(inner)?;
            ctx.as_object_mut()
                .unwrap()
                .insert("sourceFunding".into(), source_funding);
            let source_risk = Self::guard(inner, None)?;
            ctx.as_object_mut()
                .unwrap()
                .insert("sourceRisk".into(), source_risk.clone());
            need(
                get(&source_risk, "unresolvedIntentIds")
                    .as_array()
                    .is_some_and(|a| a.is_empty()),
                "PENDING_BEFORE_BOOK",
            )?;
            let (scope, _) = (inner.borrow().scope.clone(), ());
            let book = Self::read(
                inner,
                &format!(
                    "/markets/{}/orderbook?tournamentId={}&depth=20",
                    get(&leg, "marketId").as_str().unwrap_or(""),
                    get(&scope, "tournamentId").as_str().unwrap_or("")
                ),
            )?;
            ctx.as_object_mut().unwrap().insert("book".into(), book);
            Self::finish_none(inner, ctx)
        })?;
        Self::sell_evidence(&inner, "prepare_sell", None)
    }

    /// refreshPreparedNone()
    pub fn refresh_prepared_none(&self) -> Result<V, String> {
        let inner = self.inner.clone();
        need(!inner.borrow().busy, "BUSY")?;
        let a = Self::active(&inner)?;
        need(
            get(&a, "phase").as_str() == Some("prepared")
                && get(&a, "orderId").is_null()
                && js_int(if !get(&a, "actualFetchCount").is_null() {
                    get(&a, "actualFetchCount")
                } else {
                    get(&a, "fetchCount")
                }) == Some(0)
                && get(&a, "bodyHash").as_str() == Some(payload_hash(get(&a, "body"))?.as_str())
                && get(get(&a, "body"), "side").as_str() == Some("no")
                && instant_v(get(get(&a, "body"), "expirationDate"))?
                    > (inner.borrow().deps.now)() as i64,
            "EXACT_UNFETCHED_PREPARED",
        )?;
        let is_buy = get(get(&a, "body"), "action").as_str() == Some("buy");
        let mut ctx: V;
        if is_buy {
            let (hedge_packet, legs, scope, first_ex) = {
                let b = inner.borrow();
                (b.hedge_packet.clone(), b.legs.clone(), b.scope.clone(), b.first_exchange_id.clone())
            };
            need(
                hedge_packet.is_some()
                    && js_int(get(get(&a, "body"), "quantity")) == Some(1)
                    && get(&a, "firstExchangeId") == get(hedge_packet.as_ref().unwrap(), "firstExchangeId"),
                "ORIGINAL_HEDGE_BINDING",
            )?;
            let hedge_packet = hedge_packet.unwrap();
            let leg = legs
                .iter()
                .find(|e| get(e, "exchangeId") == get(get(&a, "body"), "exchangeId"))
                .cloned();
            let graph_leg = legs
                .iter()
                .find(|e| get(e, "exchangeId") == get(&a, "firstExchangeId"))
                .cloned();
            need(
                leg.is_some()
                    && graph_leg.is_some()
                    && get(get(&a, "body"), "tournamentId") == get(&scope, "tournamentId"),
                "ORIGINAL_HEDGE_SCOPE",
            )?;
            let _ = first_ex;
            ctx = json!({
                "kind": "hedgeFinal",
                "leg": leg,
                "graphLeg": graph_leg,
                "owned": get(&hedge_packet, "owned"),
                "ownedExchangeId": get(&a, "firstExchangeId"),
                "otherBookReceipt": get(&hedge_packet, "otherBookReceipt"),
                "book": get(&hedge_packet, "otherBookReceipt"),
                "hedgePacket": hedge_packet,
            });
        } else {
            let cache = inner.borrow().cache.clone().unwrap_or(V::Null);
            need(
                get(get(&a, "body"), "action").as_str() == Some("sell")
                    && get(&cache, "kind").as_str() == Some("sell")
                    && get(get(&cache, "leg"), "exchangeId") == get(get(&a, "body"), "exchangeId")
                    && js_int(get(get(&a, "body"), "quantity")).unwrap_or(i64::MAX)
                        <= js_int(get(get(get(&cache, "owned"), "proof"), "remainingOwnedQuantity"))
                            .unwrap_or(i64::MIN),
                "ORIGINAL_SELL_BINDING",
            )?;
            ctx = cache;
        }
        ctx.as_object_mut()
            .unwrap()
            .insert("preparedBinding".into(), a.clone());
        let source_pin = if is_buy {
            payload_hash(get(&ctx, "hedgePacket"))?
        } else {
            payload_hash(&json!({
                "account": get(&ctx, "account"),
                "book": get(&ctx, "book"),
                "owned": get(&ctx, "owned"),
                "sourceRisk": get(&ctx, "sourceRisk"),
                "sourceFunding": get(&ctx, "sourceFunding"),
            }))?
        };
        Self::begin(&inner, ctx, 2, Self::finish_none)?;
        {
            let b = inner.borrow();
            let cache = b.cache.clone().unwrap();
            let after_pin = if is_buy {
                payload_hash(get(&cache, "hedgePacket"))?
            } else {
                payload_hash(&json!({
                    "account": get(&cache, "account"),
                    "book": get(&cache, "book"),
                    "owned": get(&cache, "owned"),
                    "sourceRisk": get(&cache, "sourceRisk"),
                    "sourceFunding": get(&cache, "sourceFunding"),
                }))?
            };
            need(source_pin == after_pin, "ORIGINAL_PACKET_CHANGED")?;
        }
        self.evidence(&json!({
            "operation": if is_buy { "place_hedge" } else { "place_sell" }
        }))
    }

    /// refreshFinalNone()
    pub fn refresh_final_none(&self) -> Result<V, String> {
        self.refresh_prepared_none()
    }

    /// refreshReconciliation(orderId)
    pub fn refresh_reconciliation(&self, order_id: &V) -> Result<V, String> {
        let inner = self.inner.clone();
        let binding = {
            let b = inner.borrow();
            let f = b
                .deps
                .get_order_binding
                .clone()
                .unwrap_or_else(|| Rc::new(|_| Ok(V::Null)));
            sync(f(order_id))?
        };
        let legs = inner.borrow().legs.clone();
        let leg = legs
            .iter()
            .find(|e| get(e, "exchangeId") == get(get(&binding, "body"), "exchangeId"))
            .cloned();
        need(
            get(&binding, "orderId") == order_id
                && leg.is_some()
                && get(&binding, "bodyHash").as_str()
                    == Some(payload_hash(get(&binding, "body"))?.as_str())
                && get(get(&binding, "body"), "side").as_str() == Some("no")
                && ["buy", "sell"].contains(&get(get(&binding, "body"), "action").as_str().unwrap_or(""))
                && get(get(&binding, "body"), "tournamentId")
                    == get(&inner.borrow().scope, "tournamentId")
                && js_int(get(get(&binding, "body"), "quantity"))
                    .map(|q| q > 0 && q <= 100)
                    .unwrap_or(false),
            "KNOWN_OWN_BINDING",
        )?;
        let leg = leg.unwrap();
        let first_ex = inner.borrow().first_exchange_id.clone();
        let graph_leg = if get(get(&binding, "body"), "action").as_str() == Some("buy") {
            let fe = if !get(&binding, "firstExchangeId").is_null() {
                get(&binding, "firstExchangeId").as_str().unwrap_or("").to_string()
            } else {
                first_ex
            };
            legs.iter()
                .find(|e| get(e, "exchangeId").as_str() == Some(fe.as_str()))
                .cloned()
        } else {
            Some(leg.clone())
        };
        need(graph_leg.is_some(), "ACTUAL_ORDER_GRAPH_SCOPE")?;
        let graph_leg = graph_leg.unwrap();
        let ctx_in = json!({
            "kind": "reconcile",
            "leg": leg,
            "graphLeg": graph_leg,
            "binding": binding,
        });
        let order_id_str = order_id.as_str().unwrap_or("").to_string();
        Self::begin(&inner, ctx_in, 9, move |inner, ctx| {
            let (scope, _legs) = {
                let b = inner.borrow();
                (b.scope.clone(), b.legs.clone())
            };
            let order = Self::read(inner, &format!("/orders/{order_id_str}"))?;
            ctx.as_object_mut().unwrap().insert("order".into(), order.clone());
            let od_ep = get(&order, "endpoint").as_str().unwrap_or("").to_string();
            let parsed = raw(inner, &order, &od_ep, 20_000)?;
            let od = parsed.data.clone();
            need(
                js_int(get(&od, "id")) == order_id_str.parse::<i64>().ok()
                    && parsed
                        .numbers
                        .get(&baal_service_runtime::json_text::js_stringify(&json!(["id"])))
                        .map(|s| s.as_str())
                        == Some(order_id_str.as_str())
                    && get(&od, "exchangeId") == get(&leg, "exchangeId")
                    && get(&od, "tournamentId") == get(&scope, "tournamentId")
                    && get(&od, "side").as_str() == Some("no")
                    && get(&od, "action") == get(get(&binding, "body"), "action")
                    && js_int(get(&od, "quantity")).map(|q| q >= 1).unwrap_or(false)
                    && js_int(get(&od, "quantity")).unwrap_or(i64::MAX)
                        <= js_int(get(get(&binding, "body"), "quantity")).unwrap_or(i64::MIN)
                    && js_int(get(&od, "quantityFilled")).map(|q| q >= 0).unwrap_or(false)
                    && get(&od, "open").is_boolean()
                    && (get(&od, "open") == &json!(true)
                        || js_int(get(&od, "quantityFilled")).unwrap_or(i64::MAX)
                            <= js_int(get(&od, "quantity")).unwrap_or(i64::MIN)),
                "EXACT_OWN_ORDER_RESPONSE",
            )?;
            let fill_pages = Self::page(inner, &format!("/orders/{order_id_str}/fills?limit=200"))?;
            ctx.as_object_mut()
                .unwrap()
                .insert("fillPages".into(), json!(fill_pages));
            let fp_ep = get(&fill_pages[0], "endpoint").as_str().unwrap_or("").to_string();
            let fills = raw(inner, &fill_pages[0], &fp_ep, 20_000)?.data;
            need(
                js_int(get(&fills, "orderId")) == order_id_str.parse::<i64>().ok()
                    && get(&fills, "exchangeId") == get(&leg, "exchangeId")
                    && get(&fills, "tournamentId") == get(&scope, "tournamentId")
                    && get(&fills, "data").as_array().is_some_and(|a| {
                        a.iter().all(|v| {
                            get(v, "side").as_str() == Some("no")
                                && js_int(get(v, "quantity")).map(|q| q < 0).unwrap_or(false)
                        })
                    }),
                "EXACT_OWN_FILL_RESPONSE",
            )?;
            let after = Self::account(inner, &leg, true)?;
            ctx.as_object_mut().unwrap().insert("after".into(), after);
            Self::finish_none(inner, ctx)
        })?;
        self.evidence(&json!({"operation": "reconcile", "orderId": order_id}))
    }

    /// status()
    pub fn status(&self) -> V {
        let b = self.inner.borrow();
        json!({
            "mode": "combined_owned_no_none_evidence_bridge_v2",
            "busy": b.busy,
            "closed": b.closed,
            "cacheKind": b.cache.as_ref().map(|c| get(c, "kind").clone()).unwrap_or(V::Null),
            "lastReadCount": b.reads,
            "maximumReads": b.maximum_reads,
            "totalReadCount": b.total_read_count,
            "hedge": b.hedge.as_ref().map(|h| h.status()).unwrap_or(V::Null),
            "executionAuthority": false,
            "additionalApiClients": 0,
        })
    }

    /// clear()
    pub fn clear(&self) {
        self.inner.borrow_mut().cache = None;
        if let Some(h) = self.inner.borrow().hedge.as_ref() {
            h.clear();
        }
    }

    /// close()
    pub fn close(&self) -> Result<(), String> {
        {
            let b = self.inner.borrow();
            need(!b.busy, "BUSY")?;
        }
        {
            let mut b = self.inner.borrow_mut();
            b.closed = true;
            b.cache = None;
            b.watermarks.clear();
        }
        if let Some(h) = self.inner.borrow().hedge.as_ref() {
            h.close()?;
        }
        Ok(())
    }
}
