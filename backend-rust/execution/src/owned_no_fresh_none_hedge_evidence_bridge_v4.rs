// execution/owned-no-fresh-none-hedge-evidence-bridge-v4.mjs — skip optional
// relationship/collateral reads only when validated book/FIFO mathematics
// already rejects an exit. Revision V3 separates mechanical read identity
// checks (canCheckReadIdentity) from full financial admission/network checks
// (canRead). Default callbacks retain V2 behavior. Prefix
// COMBINED_NONE_EVIDENCE_.
use crate::baal_scaled_live_host_v5::BaalScaledLiveApi;
use crate::baal_scaled_owned_no_hedge_proposal_v1::plan_baal_owned_no_hedge;
use crate::baal_scaled_profitable_exit_signal_v2::plan_baal_scaled_profitable_exit_v2;
use crate::core::payload_hash;
use crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::owned_no_fresh_none_hedge_evidence_bridge_v1::{
    NoneHedgeBridgeApi, NoneHedgeBridgeV1Deps, OwnedNoFreshNoneHedgeEvidenceBridge,
};
use crate::transport::{Dispatch, Guard, OrderTransportError};
use crate::receipts::parse_number_lexemes;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::{instant, price};
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
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
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn sha(s: &str) -> String {
    sha256(s.as_bytes())
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(payload_hash(a)? == payload_hash(b)?)
}
fn iso(n: f64) -> String {
    ms_to_iso(n)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn price_v(v: &V) -> Result<i128, String> {
    price(&v.as_str().map(str::to_string).unwrap_or_else(|| {
        if v.is_number() {
            v.to_string()
        } else {
            String::new()
        }
    }))
}
fn digit_id(v: &V) -> bool {
    v.as_str()
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && !s.starts_with('0'))
}
fn economic(r: &V) -> V {
    let mut m = r.as_object().cloned().unwrap_or_default();
    m.shift_remove("reservationHash");
    m.shift_remove("unresolvedIntentIds");
    V::Object(m)
}

pub struct OwnedNoFreshNoneBridgeV4Deps {
    /// api.dispatch / api.manualReview / api.cooldown
    pub api: NoneHedgeBridgeApi,
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub scope: V,
    pub first_exchange_id: String,
    pub pair_legs: Option<Vec<V>>,
    pub get_owned: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub get_pair_census: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub get_funding: Rc<dyn Fn() -> Result<V, String>>,
    pub get_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub can_read: Rc<dyn Fn() -> bool>,
    /// default: can_read (V3 split between identity and admission checks)
    pub can_check_read_identity: Option<Rc<dyn Fn() -> bool>>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> Result<bool, String>>,
    pub get_active_intent: Option<Rc<dyn Fn() -> Result<V, String>>>,
    pub get_order_binding: Option<Rc<dyn Fn(&V) -> Result<V, String>>>,
    pub refresh_funding_after_account: Option<Rc<dyn Fn() -> Result<V, String>>>,
    pub now: Rc<dyn Fn() -> f64>,
}

pub(crate) struct BridgeV4Inner {
    scope: V,
    first_exchange_id: String,
    legs: Vec<V>,
    hedge: RefCell<Option<OwnedNoFreshNoneHedgeEvidenceBridge>>,
    hedge_packet: RefCell<V>,
    cache: RefCell<V>,
    job: RefCell<V>,
    total_read_count: Cell<i64>,
    busy: Cell<bool>,
    closed: Cell<bool>,
    reads: Cell<i64>,
    maximum_reads: Cell<i64>,
    last_end: Cell<Option<f64>>,
    read_timings: RefCell<VecDeque<V>>,
    permits: RefCell<HashMap<usize, V>>,
    watermarks: RefCell<HashMap<String, (f64, f64)>>,
    invocations: RefCell<HashSet<String>>,
    deps: OwnedNoFreshNoneBridgeV4Deps,
}

type Inner = Rc<RefCell<BridgeV4Inner>>;

fn active(inner: &Inner) -> Result<V, String> {
    let g = inner.borrow();
    match &g.deps.get_active_intent {
        Some(f) => f(),
        None => Ok(V::Null),
    }
}
fn fund(inner: &Inner) -> Result<V, String> {
    let g = inner.borrow();
    (g.deps.get_funding)()
}
fn order_binding(inner: &Inner, id: &str) -> Result<V, String> {
    let g = inner.borrow();
    match &g.deps.get_order_binding {
        Some(f) => f(&json!(id)),
        None => Ok(V::Null),
    }
}

/// raw(r, endpoint, age=20000)
fn raw(inner: &Inner, r: &V, endpoint: &str, age: f64) -> Result<(V, BTreeMap<String, String>), String> {
    need(
        sget(r, "method") == Some("GET")
            && sget(r, "endpoint") == Some(endpoint)
            && js_int(get(r, "status")) == Some(200)
            && get(r, "requestBodyHash").is_null()
            && get(r, "bodyText").is_string()
            && sget(r, "bodyText").unwrap_or("").len() <= 8 * 1024 * 1024
            && sget(r, "sourceBodyHash") == Some(sha(sget(r, "bodyText").unwrap_or("")).as_str())
            && sget(r, "fetchInvocationId").is_some_and(|s| !s.is_empty())
            && r.as_object().is_some_and(|m| {
                m.keys().all(|k| {
                    [
                        "method",
                        "endpoint",
                        "status",
                        "requestBodyHash",
                        "bodyText",
                        "sourceBodyHash",
                        "requestStartedAt",
                        "receivedAt",
                        "fetchInvocationId",
                    ]
                    .contains(&k.as_str())
                })
            }),
        "RAW_SOURCE",
    )?;
    let start = instant_v(get(r, "requestStartedAt"))? as f64;
    let end = instant_v(get(r, "receivedAt"))? as f64;
    need(
        start <= end && end <= (inner.borrow().deps.now)() && (inner.borrow().deps.now)() - start <= age,
        "SOURCE_AGE",
    )?;
    parse_number_lexemes(sget(r, "bodyText").unwrap_or(""))
}

/// identity()
fn identity(inner: &Inner) -> Result<(), String> {
    {
        let g = inner.borrow();
        let can_identity = g
            .deps
            .can_check_read_identity
            .clone()
            .unwrap_or_else(|| g.deps.can_read.clone());
        need(!g.closed.get() && can_identity(), "STOPPED")?;
    }
    let l = (inner.borrow().deps.get_account_lease)()?;
    need(
        get(&l, "ownerId") == get(&inner.borrow().scope, "ownerId")
            && get(&l, "tournamentId") == get(&inner.borrow().scope, "tournamentId")
            && hex(get(&l, "leaseId")),
        "ACCOUNT_LEASE",
    )?;
    {
        let g = inner.borrow();
        need(
            !g.deps.api.manual_review && (g.deps.now)() >= g.deps.api.cooldown.max(0.0),
            "BACKOFF",
        )?;
    }
    Ok(())
}

/// guard(fenceRisk=null)
fn guard(inner: &Inner, fence_risk: Option<&V>) -> Result<V, String> {
    identity(inner)?;
    need((inner.borrow().deps.can_read)(), "STOPPED")?;
    let g = match fence_risk {
        Some(f) => f.clone(),
        None => (inner.borrow().deps.get_global_risk)()?,
    };
    let ctx = {
        let i = inner.borrow();
        let j = i.job.borrow();
        if j.is_null() {
            i.cache.borrow().clone()
        } else {
            j.clone()
        }
    };
    let a = active(inner)?;
    let (so, sr, st) = {
        let i = inner.borrow();
        (
            get(&i.scope, "ownerId").clone(),
            get(&i.scope, "runId").clone(),
            get(&i.scope, "tournamentId").clone(),
        )
    };
    need(
        get(&g, "ownerId") == &so
            && get(&g, "runId") == &sr
            && get(&g, "tournamentId") == &st
            && get(&g, "complete") == &V::Bool(true)
            && get(&g, "eventScopeComplete") == &V::Bool(true)
            && get(&g, "soleCollector") == &V::Bool(true)
            && get(&g, "stopped") == &V::Bool(false)
            && hex(get(&g, "sourceHash"))
            && get(&g, "unresolvedIntentIds").is_array()
            && instant_v(get(&g, "at"))? as f64 <= (inner.borrow().deps.now)()
            && (inner.borrow().deps.now)() - instant_v(get(&g, "at"))? as f64 <= 20000.0,
        "GLOBAL_SCOPE",
    )?;
    let actual_fetch_zero = js_int(if get(&a, "actualFetchCount").is_null() {
        get(&a, "fetchCount")
    } else {
        get(&a, "actualFetchCount")
    }) == Some(0);
    if !get(&ctx, "preparedBinding").is_null()
        && get(&a, "intentId") == get(get(&ctx, "preparedBinding"), "intentId")
        && get(&a, "orderId").is_null()
        && actual_fetch_zero
    {
        need(
            instant_v(get(get(&a, "body"), "expirationDate"))? as f64
                > (inner.borrow().deps.now)(),
            "PREPARED_EXPIRED",
        )?;
    }
    let ctx_kind = sget(&ctx, "kind").unwrap_or("");
    let owned_qty = js_int(get(get(get(&ctx, "owned"), "proof"), "remainingOwnedQuantity"))
        .unwrap_or(0);
    let pending_own = !ctx.is_null()
        && js_truthy(Some(&a))
        && sget(&a, "intentId").is_some()
        && sget(&a, "bodyHash") == Some(payload_hash(get(&a, "body"))?.as_str())
        && sget(get(&a, "body"), "exchangeId") == sget(get(&ctx, "leg"), "exchangeId")
        && sget(get(&a, "body"), "tournamentId") == st.as_str()
        && sget(get(&a, "body"), "side") == Some("no")
        && (sget(get(&a, "body"), "action") == Some("sell")
            || (ctx_kind == "hedgeFinal"
                && sget(get(&a, "body"), "action") == Some("buy")
                && js_int(get(get(&a, "body"), "quantity")) == Some(1)))
        && ["prepared", "dispatching"].contains(&sget(&a, "phase").unwrap_or(""))
        && get(&a, "orderId").is_null()
        && actual_fetch_zero
        && js_int(get(get(&a, "body"), "quantity")).is_some_and(|q| q > 0)
        && js_int(get(get(&a, "body"), "quantity")).unwrap_or(i64::MAX)
            <= if ctx_kind == "hedgeFinal" { 1 } else { owned_qty }
        && instant_v(get(get(&a, "body"), "expirationDate"))? as f64
            > (inner.borrow().deps.now)()
        && get(&g, "unresolvedIntentIds")
            .as_array()
            .map(|arr| arr.iter().all(|id| id == get(&a, "intentId")))
            .unwrap_or(false);
    let binding = get(&ctx, "binding");
    let known = if binding.is_null() {
        V::Null
    } else {
        order_binding(inner, sget(binding, "orderId").unwrap_or(""))?
    };
    let cleanup = js_truthy(Some(&known))
        && get(&known, "intentId") == get(binding, "intentId")
        && get(&known, "orderId") == get(binding, "orderId")
        && get(&known, "bodyHash") == get(binding, "bodyHash")
        && same(get(&known, "body"), get(binding, "body"))?
        && get(&g, "unresolvedIntentIds")
            .as_array()
            .map(|arr| arr.iter().all(|id| id == get(binding, "intentId")))
            .unwrap_or(false);
    need(
        (get(&g, "unknownAcceptance") == &V::Bool(false) || cleanup)
            && (get(&g, "unresolvedIntentIds")
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(false)
                || pending_own
                || cleanup),
        "UNKNOWN_OR_FOREIGN_PENDING",
    )?;
    if !get(&ctx, "preparedBinding").is_null() {
        let pb = get(&ctx, "preparedBinding").clone();
        need(
            get(&a, "intentId") == get(&pb, "intentId")
                && get(&a, "bodyHash") == get(&pb, "bodyHash")
                && same(get(&a, "body"), get(&pb, "body"))?
                && (inner.borrow().job.borrow().is_null()
                    || sget(&a, "phase") == Some("prepared")),
            "PREPARED_BODY_CHANGED",
        )?;
    }
    if !get(&ctx, "owned").is_null() {
        let ex = if !get(&ctx, "ownedExchangeId").is_null() {
            sget(&ctx, "ownedExchangeId").unwrap_or("").to_string()
        } else {
            sget(get(&ctx, "leg"), "exchangeId").unwrap_or("").to_string()
        };
        need(
            same(&(inner.borrow().deps.get_owned)(&json!(ex))?, get(&ctx, "owned"))?,
            "CURRENT_CUSTODY_CHANGED",
        )?;
    }
    Ok(g)
}

/// none(ctx)
fn none(inner: &Inner, ctx: &V) -> Result<(), String> {
    let graph_leg = if get(ctx, "graphLeg").is_null() {
        get(ctx, "leg").clone()
    } else {
        get(ctx, "graphLeg").clone()
    };
    let tid = sget(&inner.borrow().scope, "tournamentId").unwrap_or("").to_string();
    let graph = raw(
        inner,
        get(ctx, "graphReceipt"),
        &format!(
            "/relationships/graph?marketId={}&depth=4&tournamentId={tid}",
            sget(&graph_leg, "marketId").unwrap_or("")
        ),
        5000.0,
    )?
    .0;
    let c = raw(
        inner,
        get(ctx, "collateralReceipt"),
        &format!("/portfolio/collateral?tournamentId={tid}"),
        5000.0,
    )?
    .0;
    let mut keys: Vec<String> = graph
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    need(
        keys.join("|") == "nodes|relationships"
            && get(&graph, "relationships")
                .as_array()
                .is_some_and(|a| a.is_empty())
            && get(&graph, "nodes").as_array().is_some_and(|a| a.len() <= 2)
            && get(&graph, "nodes").as_array().is_some_and(|nodes| {
                nodes.iter().all(|n| {
                    inner.borrow().legs.iter().any(|e| {
                        sget(e, "marketId") == sget(n, "marketId")
                            && sget(e, "exchangeId") == sget(n, "exchangeId")
                    })
                })
            })
            && get(&graph, "nodes")
                .as_array()
                .map(|nodes| {
                    let s: HashSet<&str> =
                        nodes.iter().filter_map(|n| sget(n, "exchangeId")).collect();
                    s.len() == nodes.len()
                })
                .unwrap_or(false),
        "NON_NONE_GRAPH",
    )?;
    need(
        c.as_object()
            .map(|m| m.keys().cloned().collect::<Vec<_>>().join("|") == "data")
            .unwrap_or(false)
            && get(&c, "data").as_array().is_some_and(|a| a.is_empty()),
        "NON_NONE_COLLATERAL",
    )?;
    let leg1 = inner.borrow().legs.get(1).cloned().unwrap_or(V::Null);
    let preceding = if !get(ctx, "book").is_null() {
        get(get(ctx, "book"), "receivedAt").clone()
    } else {
        get(
            get(get(ctx, "after"), "scopedOrdersPagesByExchange"),
            sget(&leg1, "exchangeId").unwrap_or(""),
        )
        .as_array()
        .and_then(|a| a.last().cloned())
        .map(|r| get(&r, "receivedAt").clone())
        .unwrap_or(V::Null)
    };
    need(
        instant_v(get(get(ctx, "graphReceipt"), "requestStartedAt"))? >= instant_v(&preceding)?
            && instant_v(get(get(ctx, "collateralReceipt"), "requestStartedAt"))?
                >= instant_v(get(get(ctx, "graphReceipt"), "receivedAt"))?,
        "NONE_AFTER_OWN_SOURCE",
    )?;
    Ok(())
}

/// read(endpoint)
fn read(inner: &Inner, endpoint: &str) -> Result<V, String> {
    identity(inner)?;
    {
        let g = inner.borrow();
        need(g.reads.get() < g.maximum_reads.get(), "READ_BOUND")?;
        need(
            (g.deps.has_read_headroom)(g.maximum_reads.get() - g.reads.get())?,
            "READ_BOUND",
        )?;
    }
    let admitted = Cell::new(false);
    let network = Cell::new(false);
    let inner_a = inner.clone();
    let admitted_a = &admitted;
    let network_a = &network;
    let before_dispatch = move |_v: &V, _s: &V| -> Result<bool, OrderTransportError> {
        (|| -> Result<bool, String> {
            guard(&inner_a, None)?;
            need(!admitted_a.get(), "ADMISSION")?;
            let g = inner_a.borrow();
            need(
                (g.deps.has_read_headroom)(g.maximum_reads.get() - g.reads.get())?,
                "ADMISSION",
            )?;
            admitted_a.set(true);
            Ok(true)
        })()
        .map_err(ote)
    };
    let inner_b = inner.clone();
    let before_network = move |_v: &V, _s: &V| -> Result<bool, OrderTransportError> {
        (|| -> Result<bool, String> {
            guard(&inner_b, None)?;
            need(admitted_a.get() && !network_a.get(), "NETWORK")?;
            let g = inner_b.borrow();
            need(
                (g.deps.has_read_headroom)(g.maximum_reads.get() - g.reads.get() - 1)?,
                "NETWORK",
            )?;
            network_a.set(true);
            Ok(true)
        })()
        .map_err(ote)
    };
    let bd_ptr = &before_dispatch as &Guard as *const Guard as *const () as usize;
    let bn_ptr = &before_network as &Guard as *const Guard as *const () as usize;
    inner.borrow().permits.borrow_mut().insert(
        bd_ptr,
        json!({"endpoint": endpoint, "beforeNetworkPtr": bn_ptr as u64}),
    );
    let began = (inner.borrow().deps.now)();
    let result: Result<V, String> = (|| {
        let spec = json!({"method": "GET", "endpoint": endpoint});
        let response = {
            let dispatch = inner.borrow().deps.api.dispatch.clone();
            dispatch(&spec, Some(&before_dispatch), Some(&before_network)).map_err(|e| e.code)?
        };
        identity(inner)?;
        need(
            admitted.get()
                && network.get()
                && sget(&response, "endpoint") == Some(endpoint)
                && js_int(get(&response, "status")) == Some(200),
            "RESPONSE",
        )?;
        let r = (inner.borrow().deps.receipt_of)(&response, &json!({"method": "GET"}))?;
        let p = raw(inner, &r, endpoint, 20000.0)?;
        need(
            instant_v(get(&r, "requestStartedAt"))? as f64 >= began
                && instant_v(get(&r, "requestStartedAt"))?
                    >= instant_v(get(&response, "requestStartedAt"))?
                && instant_v(get(&r, "receivedAt"))? <= instant_v(get(&response, "receivedAt"))?
                && same(&p.0, get(&response, "data"))?
                && (inner.borrow().last_end.get().is_none()
                    || instant_v(get(&r, "requestStartedAt"))? as f64
                        >= inner.borrow().last_end.get().unwrap_or(f64::MIN))
                && !inner
                    .borrow()
                    .invocations
                    .borrow()
                    .contains(sget(&r, "fetchInvocationId").unwrap_or("")),
            "RAW_CAUSALITY",
        )?;
        let old = inner.borrow().watermarks.borrow().get(endpoint).cloned();
        need(
            old.is_none()
                || (instant_v(get(&r, "requestStartedAt"))? as f64
                    >= old.unwrap_or((0.0, 0.0)).0
                    && instant_v(get(&r, "receivedAt"))? as f64 >= old.unwrap_or((0.0, 0.0)).1),
            "RAW_ROLLBACK",
        )?;
        inner.borrow().watermarks.borrow_mut().insert(
            endpoint.to_string(),
            (
                instant_v(get(&r, "requestStartedAt"))? as f64,
                instant_v(get(&r, "receivedAt"))? as f64,
            ),
        );
        inner
            .borrow()
            .invocations
            .borrow_mut()
            .insert(sget(&r, "fetchInvocationId").unwrap_or("").to_string());
        inner
            .borrow()
            .last_end
            .set(Some(instant_v(get(&r, "receivedAt"))? as f64));
        Ok(r)
    })();
    {
        let g = inner.borrow();
        let now = (g.deps.now)();
        g.read_timings.borrow_mut().push_back(json!({
            "endpoint": endpoint,
            "startedAt": iso(began),
            "finishedAt": iso(now),
            "elapsedMs": now - began,
            "admitted": admitted.get(),
            "network": network.get(),
        }));
        while g.read_timings.borrow().len() > 20 {
            g.read_timings.borrow_mut().pop_front();
        }
        g.reads.set(g.reads.get() + 1);
        g.total_read_count.set(g.total_read_count.get() + 1);
    }
    inner.borrow().permits.borrow_mut().remove(&bd_ptr);
    result
}

/// page(endpoint)
fn page(inner: &Inner, endpoint: &str) -> Result<Vec<V>, String> {
    let r = read(inner, endpoint)?;
    let p = raw(inner, &r, endpoint, 20000.0)?;
    let d = &p.0;
    let cvg = get(d, "coverage");
    need(
        get(d, "data").as_array().is_some_and(|a| a.len() <= 200)
            && js_int(get(get(d, "pagination"), "limit")) == Some(200)
            && get(get(d, "pagination"), "hasMore") == &V::Bool(false)
            && get(get(d, "pagination"), "nextCursor").is_null()
            && get(cvg, "complete") == &V::Bool(true)
            && (get(cvg, "projectedThroughSequence").is_null()
                || js_int(get(cvg, "projectedThroughSequence")).is_some_and(|s| s >= 0)),
        "COMPLETE_SINGLE_PAGE_REQUIRED",
    )?;
    let mut ids: HashSet<i64> = HashSet::new();
    for (i, v) in get(d, "data").as_array().cloned().unwrap_or_default().iter().enumerate() {
        let key = js_stringify(&json!(["data", i as i64, "id"]));
        let n = p.1.get(&key).cloned().unwrap_or_default();
        let vid = js_int(get(v, "id"));
        need(
            digit_id(&json!(n)) && vid.is_some() && !ids.contains(&vid.unwrap_or(0)),
            "EXACT_ID",
        )?;
        ids.insert(vid.unwrap_or(0));
    }
    Ok(vec![r])
}

/// account(target, both)
fn account(inner: &Inner, target: &V, both: bool) -> Result<V, String> {
    let (slug, tid) = {
        let g = inner.borrow();
        (
            sget(&g.scope, "tournamentSlug").unwrap_or("").to_string(),
            sget(&g.scope, "tournamentId").unwrap_or("").to_string(),
        )
    };
    let cash = read(inner, &format!("/tournaments/{slug}"))?;
    let positions = read(inner, &format!("/tournaments/{slug}/portfolio/positions"))?;
    let open_orders_pages =
        page(inner, &format!("/orders?status=open&limit=200&tournamentId={tid}"))?;
    let mut scoped: Map<String, V> = Map::new();
    let legs_iter: Vec<V> = if both {
        inner.borrow().legs.clone()
    } else {
        vec![target.clone()]
    };
    let mut target_pages = V::Null;
    for e in legs_iter {
        let pages = page(
            inner,
            &format!(
                "/orders?status=all&exchangeId={}&limit=200&tournamentId={tid}",
                sget(&e, "exchangeId").unwrap_or("")
            ),
        )?;
        if sget(&e, "exchangeId") == sget(target, "exchangeId") {
            target_pages = json!(pages.clone());
        }
        scoped.insert(sget(&e, "exchangeId").unwrap_or("").to_string(), json!(pages));
    }
    Ok(json!({
        "cash": cash,
        "positions": positions,
        "openOrdersPages": open_orders_pages,
        "scopedOrdersPages": target_pages,
        "scopedOrdersPagesByExchange": V::Object(scoped),
    }))
}

/// finishNone(ctx)
fn finish_none(inner: &Inner, ctx: &RefCell<V>) -> Result<(), String> {
    let graph_leg = {
        let c = ctx.borrow();
        if get(&c, "graphLeg").is_null() {
            get(&c, "leg").clone()
        } else {
            get(&c, "graphLeg").clone()
        }
    };
    let tid = sget(&inner.borrow().scope, "tournamentId")
        .unwrap_or("")
        .to_string();
    let graph = read(
        inner,
        &format!(
            "/relationships/graph?marketId={}&depth=4&tournamentId={tid}",
            sget(&graph_leg, "marketId").unwrap_or("")
        ),
    )?;
    if let Some(m) = ctx.borrow_mut().as_object_mut() {
        m.insert("graphReceipt".into(), graph);
    }
    let collateral = read(inner, &format!("/portfolio/collateral?tournamentId={tid}"))?;
    if let Some(m) = ctx.borrow_mut().as_object_mut() {
        m.insert("collateralReceipt".into(), collateral);
    }
    let snapshot = ctx.borrow().clone();
    none(inner, &snapshot)
}

/// sellEvidence(operation, fenceRisk)
fn sell_evidence(inner: &Inner, operation: &str, fence_risk: Option<&V>) -> Result<V, String> {
    let g = guard(inner, fence_risk)?;
    let cache = inner.borrow().cache.borrow().clone();
    need(
        sget(&cache, "kind") == Some("sell") && !inner.borrow().busy.get(),
        "SELL_CACHE",
    )?;
    none(inner, &cache)?;
    if operation == "prepare_sell" {
        need(active(inner)?.is_null(), "ACTIVE_BEFORE_PREPARE")?;
    }
    let f = fund(inner)?;
    need(
        get(get(&f, "record"), "reservationHash") == get(&g, "controllerReservationHash")
            && get(get(&f, "receipt"), "globalLedgerHash") == get(&g, "globalLedgerHash")
            && get(get(&f, "receipt"), "reservationHash")
                == get(get(&f, "record"), "reservationHash"),
        "CURRENT_INSTALLED_BUDGET",
    )?;
    need(
        same(
            &economic(get(&f, "record")),
            &economic(get(get(&cache, "sourceFunding"), "record")),
        )?,
        "CURRENT_FULL_ECONOMICS_CHANGED",
    )?;
    let active_intent = active(inner)?;
    need(
        get(get(&cache, "sourceRisk"), "unresolvedIntentIds")
            .as_array()
            .is_some_and(|a| a.is_empty())
            && (get(get(&f, "record"), "unresolvedIntentIds")
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(true)
                || (get(get(&f, "record"), "unresolvedIntentIds")
                    .as_array()
                    .is_some_and(|a| a.len() == 1)
                    && get(get(&f, "record"), "unresolvedIntentIds")
                        .as_array()
                        .and_then(|a| a.first())
                        == Some(get(&active_intent, "intentId")))),
        "EXACT_OWN_SELL_PENDING",
    )?;
    let signal_risk = get(&cache, "sourceRisk").clone();
    let mut scope_with_legs = inner.borrow().scope.as_object().cloned().unwrap_or_default();
    scope_with_legs.insert("exchanges".into(), json!(inner.borrow().legs));
    let p = plan_baal_scaled_profitable_exit_v2(&json!({
        "scope": V::Object(scope_with_legs),
        "owned": get(&cache, "owned"),
        "account": get(&cache, "account"),
        "bookReceipt": get(&cache, "book"),
        "globalRisk": signal_risk,
        "now": iso((inner.borrow().deps.now)()),
    }));
    need(
        get(&p, "proposalReady") == &V::Bool(true),
        &format!("CURRENT_FIFO_OR_PROFIT_{}", sget(&p, "reason").unwrap_or("")),
    )?;
    let active_now = active(inner)?;
    if js_truthy(Some(&active_now)) {
        need(
            js_int(get(get(&active_now, "body"), "quantity")).unwrap_or(i64::MAX)
                <= js_int(get(&p, "quantity")).unwrap_or(i64::MIN)
                && price_v(get(get(&active_now, "body"), "price"))? <= price_v(get(&p, "limit"))?,
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

/// evidence(c)
fn evidence(inner: &Inner, c: &V) -> Result<V, String> {
    let op = sget(c, "operation").unwrap_or("prepare_hedge").to_string();
    if ["prepare_hedge", "place_hedge", "hedge"].contains(&op.as_str()) {
        need(
            inner.borrow().hedge.borrow().is_some()
                || sget(&inner.borrow().cache.borrow(), "kind") == Some("hedgeFinal"),
            "HEDGE_CACHE",
        )?;
        if sget(&inner.borrow().cache.borrow(), "kind") == Some("hedgeFinal") {
            let cache = inner.borrow().cache.borrow().clone();
            let fence = get(c, "fenceRisk").clone();
            let fence_ref = if fence.is_null() { None } else { Some(fence) };
            let g = guard(inner, fence_ref.as_ref())?;
            none(inner, &cache)?;
            let f = fund(inner)?;
            let hp = get(&cache, "hedgePacket").clone();
            need(
                same(
                    &economic(get(&f, "record")),
                    &economic(get(get(&hp, "funding"), "record")),
                )? && get(get(&f, "record"), "reservationHash")
                    == get(&g, "controllerReservationHash")
                    && get(get(&f, "receipt"), "globalLedgerHash")
                        == get(&g, "globalLedgerHash"),
                "CURRENT_HEDGE_ECONOMICS_CHANGED",
            )?;
            let mut e = hp.as_object().cloned().unwrap_or_default();
            e.insert("graphReceipt".into(), get(&cache, "graphReceipt").clone());
            e.insert("collateralReceipt".into(), get(&cache, "collateralReceipt").clone());
            let mut ane = get(&hp, "allNoneEvidence").as_object().cloned().unwrap_or_default();
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
                get(&p, "proposalReady") == &V::Bool(true),
                &format!("ORIGINAL_HEDGE_PACKET_{}", sget(&p, "reason").unwrap_or("")),
            )?;
            return Ok(e);
        }
        let hedge = inner.borrow().hedge.borrow().clone();
        if let Some(h) = hedge {
            return h.get_evidence(c);
        }
        return Err("COMBINED_NONE_EVIDENCE_HEDGE_CACHE".into());
    }
    if ["prepare_sell", "place_sell"].contains(&op.as_str()) {
        let fence = get(c, "fenceRisk").clone();
        let fence_ref = if fence.is_null() { None } else { Some(fence) };
        return sell_evidence(inner, &op, fence_ref.as_ref());
    }
    need(
        op == "reconcile"
            && sget(&inner.borrow().cache.borrow(), "kind") == Some("reconcile")
            && !inner.borrow().busy.get()
            && (get(c, "orderId").is_null()
                || get(c, "orderId")
                    == get(get(&inner.borrow().cache.borrow(), "binding"), "orderId")),
        "RECONCILE_CACHE",
    )?;
    let cache = inner.borrow().cache.borrow().clone();
    let g = guard(inner, None)?;
    none(inner, &cache)?;
    let mut raw_list: Vec<V> = vec![get(&cache, "order").clone()];
    raw_list.extend(get(&cache, "fillPages").as_array().cloned().unwrap_or_default());
    raw_list.push(get(get(&cache, "after"), "cash").clone());
    raw_list.push(get(get(&cache, "after"), "positions").clone());
    raw_list.extend(
        get(get(&cache, "after"), "openOrdersPages")
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    for (_k, v) in get(get(&cache, "after"), "scopedOrdersPagesByExchange")
        .as_object()
        .cloned()
        .unwrap_or_default()
    {
        raw_list.extend(v.as_array().cloned().unwrap_or_default());
    }
    for r in &raw_list {
        raw(inner, r, sget(r, "endpoint").unwrap_or(""), 20000.0)?;
    }
    Ok(json!({
        "order": get(&cache, "order"),
        "fillPages": get(&cache, "fillPages"),
        "after": get(&cache, "after"),
        "globalRisk": g,
        "graphReceipt": get(&cache, "graphReceipt"),
        "collateralReceipt": get(&cache, "collateralReceipt"),
    }))
}

/// begin(ctx, max, fn)
fn begin<F>(inner: &Inner, ctx: V, max: i64, f: F) -> Result<V, String>
where
    F: FnOnce(&RefCell<V>) -> Result<(), String>,
{
    need(!inner.borrow().busy.get(), "BUSY")?;
    *inner.borrow().cache.borrow_mut() = V::Null;
    *inner.borrow().job.borrow_mut() = ctx.clone();
    inner.borrow().busy.set(true);
    inner.borrow().reads.set(0);
    inner.borrow().maximum_reads.set(max);
    inner.borrow().last_end.set(None);
    inner.borrow().invocations.borrow_mut().clear();
    let cell = RefCell::new(ctx);
    let result: Result<(), String> = (|| {
        identity(inner)?;
        f(&cell)?;
        *inner.borrow().cache.borrow_mut() = cell.borrow().clone();
        Ok(())
    })();
    inner.borrow().busy.set(false);
    *inner.borrow().job.borrow_mut() = V::Null;
    result?;
    Ok(cell.into_inner())
}

#[derive(Clone)]
pub struct OwnedNoFreshNoneHedgeEvidenceBridgeV4 {
    pub(crate) inner: Inner,
}

/// createOwnedNoFreshNoneHedgeEvidenceBridgeV4(options)
pub fn create_owned_no_fresh_none_hedge_evidence_bridge_v4(
    deps: OwnedNoFreshNoneBridgeV4Deps,
) -> Result<OwnedNoFreshNoneHedgeEvidenceBridgeV4, String> {
    let first = (deps.get_owned)(&json!(deps.first_exchange_id.clone()))?;
    let legs: Vec<V> = match &deps.pair_legs {
        Some(l) => l.clone(),
        None => get(get(&first, "package"), "legs")
            .as_array()
            .cloned()
            .unwrap_or_default(),
    };
    need(
        sget(&deps.scope, "ownerId") == Some("Codex")
            && legs.len() == 2
            && legs
                .iter()
                .map(|e| sget(e, "exchangeId").unwrap_or(""))
                .collect::<HashSet<_>>()
                .len()
                == 2
            && legs
                .iter()
                .map(|e| sget(e, "marketId").unwrap_or(""))
                .collect::<HashSet<_>>()
                .len()
                == 2
            && legs
                .iter()
                .all(|e| digit_id(get(e, "marketId")) && digit_id(get(e, "exchangeId"))),
        "APPROVED_PAIR",
    )?;
    Ok(OwnedNoFreshNoneHedgeEvidenceBridgeV4 {
        inner: Rc::new(RefCell::new(BridgeV4Inner {
            scope: deps.scope.clone(),
            first_exchange_id: deps.first_exchange_id.clone(),
            legs,
            hedge: RefCell::new(None),
            hedge_packet: RefCell::new(V::Null),
            cache: RefCell::new(V::Null),
            job: RefCell::new(V::Null),
            total_read_count: Cell::new(0),
            busy: Cell::new(false),
            closed: Cell::new(false),
            reads: Cell::new(0),
            maximum_reads: Cell::new(0),
            last_end: Cell::new(None),
            read_timings: RefCell::new(VecDeque::new()),
            permits: RefCell::new(HashMap::new()),
            watermarks: RefCell::new(HashMap::new()),
            invocations: RefCell::new(HashSet::new()),
            deps,
        })),
    })
}

impl OwnedNoFreshNoneHedgeEvidenceBridgeV4 {
    /// authorizes(spec, beforeDispatch, beforeNetwork)
    pub fn authorizes(
        &self,
        spec: &V,
        before_dispatch: &Guard,
        before_network: &Guard,
    ) -> bool {
        let r: Result<bool, String> = (|| {
            need(!self.inner.borrow().closed.get(), "CLOSED")?;
            let key = before_dispatch as *const Guard as *const () as usize;
            let p = self.inner.borrow().permits.borrow().get(&key).cloned();
            if p.is_none() {
                let h = self.inner.borrow().hedge.borrow().clone();
                return Ok(h
                    .map(|h| h.authorizes(spec, before_dispatch, before_network))
                    .unwrap_or(false));
            }
            identity(&self.inner)?;
            let bn = before_network as *const Guard as *const () as usize;
            Ok(sget(spec, "method") == Some("GET")
                && sget(spec, "endpoint") == sget(&p.clone().unwrap_or(V::Null), "endpoint")
                && bn
                    == get(&p.unwrap_or(V::Null), "beforeNetworkPtr")
                        .as_u64()
                        .unwrap_or(0) as usize
                && spec.get("body").is_none()
                && spec.get("bodyJson").is_none()
                && spec.get("headers").is_none())
        })();
        r.unwrap_or(false)
    }

    pub fn get_evidence(&self, c: &V) -> Result<V, String> {
        evidence(&self.inner, c)
    }

    /// refresh()
    pub fn refresh(&self) -> Result<V, String> {
        need(!self.inner.borrow().busy.get(), "BUSY")?;
        *self.inner.borrow().cache.borrow_mut() = V::Null;
        *self.inner.borrow().job.borrow_mut() = V::Null;
        if let Some(h) = &*self.inner.borrow().hedge.borrow() {
            let _ = h.close();
        }
        *self.inner.borrow().hedge.borrow_mut() = None;
        if self.inner.borrow().deps.refresh_funding_after_account.is_some() {
            let owned = (self.inner.borrow().deps.get_owned)(
                &json!(self.inner.borrow().first_exchange_id.clone()),
            )?;
            let graph_leg = self
                .inner
                .borrow()
                .legs
                .iter()
                .find(|e| {
                    sget(e, "exchangeId")
                        == Some(self.inner.borrow().first_exchange_id.as_str())
                })
                .cloned()
                .unwrap_or(V::Null);
            let leg = self
                .inner
                .borrow()
                .legs
                .iter()
                .find(|e| {
                    sget(e, "exchangeId")
                        != Some(self.inner.borrow().first_exchange_id.as_str())
                })
                .cloned()
                .unwrap_or(V::Null);
            need(
                js_int(get(get(&owned, "proof"), "remainingOwnedQuantity")).is_some_and(|q| q > 0)
                    && !graph_leg.is_null()
                    && !leg.is_null(),
                "CURRENT_HEDGE_SEED",
            )?;
            let first_id = self.inner.borrow().first_exchange_id.clone();
            let ctx = json!({
                "kind": "hedgeFinal",
                "leg": leg,
                "graphLeg": graph_leg,
                "owned": owned,
                "ownedExchangeId": first_id,
            });
            let inner = self.inner.clone();
            begin(&inner, ctx, 10, |ctx| {
                let mut root_receipts: Vec<V> = Vec::new();
                let ordered = {
                    let c = ctx.borrow();
                    vec![get(&c, "graphLeg").clone(), get(&c, "leg").clone()]
                };
                for e in ordered {
                    let mut l = e.as_object().cloned().unwrap_or_default();
                    let receipt = read(
                        &inner,
                        &format!(
                            "/markets/{}/nodes?tournamentId={}",
                            sget(&e, "marketId").unwrap_or(""),
                            sget(&inner.borrow().scope, "tournamentId").unwrap_or("")
                        ),
                    )?;
                    l.insert("receipt".into(), receipt);
                    root_receipts.push(V::Object(l));
                }
                let graph_leg = get(&ctx.borrow(), "graphLeg").clone();
                let account_source = account(&inner, &graph_leg, true)?;
                let mut binding = inner.borrow().scope.clone();
                if let (Some(m), Some(gm)) = (binding.as_object_mut(), graph_leg.as_object()) {
                    for (k, v) in gm {
                        m.insert(k.clone(), v.clone());
                    }
                }
                let view = inspect_scoped_rest_no_account(&json!({
                    "account": account_source,
                    "binding": binding,
                    "readAt": iso((inner.borrow().deps.now)()),
                }))?;
                let owned = get(&ctx.borrow(), "owned").clone();
                let leg = get(&ctx.borrow(), "leg").clone();
                need(
                    get(&view, "positions").as_array().is_some_and(|a| {
                        a.iter().any(|p| {
                            sget(p, "exchangeId") == sget(&graph_leg, "exchangeId")
                                && sget(p, "marketId") == sget(&graph_leg, "marketId")
                                && js_int(get(p, "quantity"))
                                    == Some(
                                        -js_int(
                                            get(get(&owned, "proof"), "remainingOwnedQuantity"),
                                        )
                                        .unwrap_or(0),
                                    )
                                && get(p, "settled") != &V::Bool(true)
                        })
                    }) && !get(&view, "positions").as_array().is_some_and(|a| {
                        a.iter().any(|p| {
                            sget(p, "exchangeId") == sget(&leg, "exchangeId")
                                && js_int(get(p, "quantity")).unwrap_or(0) != 0
                        })
                    }),
                    "CURRENT_SOURCE_CUSTODY",
                )?;
                (inner.borrow().deps.refresh_funding_after_account.as_ref().unwrap())()?;
                let funding = fund(&inner)?;
                let pair_census =
                    (inner.borrow().deps.get_pair_census)(&json!(first_id))?;
                let global_risk = guard(&inner, None)?;
                let tid = sget(&inner.borrow().scope, "tournamentId")
                    .unwrap_or("")
                    .to_string();
                let leg = get(&ctx.borrow(), "leg").clone();
                let other_book = read(
                    &inner,
                    &format!(
                        "/markets/{}/orderbook?tournamentId={tid}&depth=20",
                        sget(&leg, "marketId").unwrap_or("")
                    ),
                )?;
                if let Some(m) = ctx.borrow_mut().as_object_mut() {
                    m.insert("otherBookReceipt".into(), other_book.clone());
                    m.insert("book".into(), other_book);
                }
                finish_none(&inner, ctx)?;
                let (scope, first_id2, now_iso) = {
                    let g = inner.borrow();
                    (
                        g.scope.clone(),
                        g.first_exchange_id.clone(),
                        iso((g.deps.now)()),
                    )
                };
                let hedge_packet = json!({
                    "scope": scope,
                    "firstExchangeId": first_id2,
                    "owned": owned,
                    "pairCensus": pair_census,
                    "funding": funding,
                    "globalRisk": global_risk,
                    "account": account_source,
                    "rootReceipts": root_receipts,
                    "otherBookReceipt": get(&ctx.borrow(), "otherBookReceipt"),
                    "graphReceipt": get(&ctx.borrow(), "graphReceipt"),
                    "collateralReceipt": get(&ctx.borrow(), "collateralReceipt"),
                    "now": now_iso,
                    "maximumPilotQuantity": 1,
                });
                if let Some(m) = ctx.borrow_mut().as_object_mut() {
                    m.insert("hedgePacket".into(), hedge_packet);
                }
                Ok(())
            })?;
            *self.inner.borrow().hedge_packet.borrow_mut() =
                get(&self.inner.borrow().cache.borrow(), "hedgePacket").clone();
            return evidence(&self.inner, &json!({"operation": "prepare_hedge"}));
        }
        // No refreshFundingAfterAccount: delegate the hedge read packet to a
        // fresh v1 bridge built over the same options.
        let v1 = {
            let d = &self.inner.borrow().deps;
            OwnedNoFreshNoneHedgeEvidenceBridge::create(NoneHedgeBridgeV1Deps {
                api: NoneHedgeBridgeApi {
                    dispatch: d.api.dispatch.clone(),
                    manual_review: d.api.manual_review,
                    cooldown: d.api.cooldown,
                },
                receipt_of: d.receipt_of.clone(),
                scope: d.scope.clone(),
                first_exchange_id: d.first_exchange_id.clone(),
                get_owned: d.get_owned.clone(),
                get_pair_census: d.get_pair_census.clone(),
                get_funding: d.get_funding.clone(),
                get_global_risk: d.get_global_risk.clone(),
                get_account_lease: d.get_account_lease.clone(),
                can_read: d.can_read.clone(),
                has_read_headroom: d.has_read_headroom.clone(),
                get_active_intent: d.get_active_intent.clone(),
                get_order_binding: d.get_order_binding.clone(),
                now: d.now.clone(),
            })?
        };
        *self.inner.borrow().hedge.borrow_mut() = Some(v1);
        let result = {
            let h = self.inner.borrow().hedge.borrow().clone();
            match h {
                Some(h) => h.refresh(),
                None => Err("COMBINED_NONE_EVIDENCE_HEDGE_CACHE".to_string()),
            }
        };
        let extra = self
            .inner
            .borrow()
            .hedge
            .borrow()
            .as_ref()
            .map(|h| get(&h.status(), "lastReadCount").as_i64().unwrap_or(0))
            .unwrap_or(0);
        self.inner
            .borrow()
            .total_read_count
            .set(self.inner.borrow().total_read_count.get() + extra);
        match result {
            Ok(packet) => {
                *self.inner.borrow().hedge_packet.borrow_mut() = packet.clone();
                Ok(packet)
            }
            Err(e) => Err(e),
        }
    }

    /// refreshSell(leg)
    pub fn refresh_sell(&self, leg: &V) -> Result<V, String> {
        need(
            self.inner
                .borrow()
                .legs
                .iter()
                .any(|e| same(e, leg).unwrap_or(false)),
            "OWN_SELL_SCOPE",
        )?;
        let owned =
            (self.inner.borrow().deps.get_owned)(&json!(sget(leg, "exchangeId").unwrap_or("")))?;
        need(
            sget(get(&owned, "proof"), "marketId") == sget(leg, "marketId")
                && sget(get(&owned, "proof"), "exchangeId") == sget(leg, "exchangeId")
                && js_int(get(get(&owned, "proof"), "remainingOwnedQuantity"))
                    .is_some_and(|q| q > 0),
            "OWN_CURRENT_SELL",
        )?;
        let inner = self.inner.clone();
        let ctx = json!({"kind": "sell", "leg": leg, "owned": owned});
        begin(&inner, ctx, 7, |ctx| {
            let leg = get(&ctx.borrow(), "leg").clone();
            let acct = account(&inner, &leg, false)?;
            if let Some(m) = ctx.borrow_mut().as_object_mut() {
                m.insert("account".into(), acct.clone());
            }
            let mut binding = inner.borrow().scope.clone();
            if let (Some(m), Some(lm)) = (binding.as_object_mut(), leg.as_object()) {
                for (k, v) in lm {
                    m.insert(k.clone(), v.clone());
                }
            }
            inspect_scoped_rest_no_account(&json!({
                "account": acct,
                "binding": binding,
                "readAt": iso((inner.borrow().deps.now)()),
            }))?;
            if let Some(f) = &inner.borrow().deps.refresh_funding_after_account {
                f()?;
            }
            let source_funding = fund(&inner)?;
            if let Some(m) = ctx.borrow_mut().as_object_mut() {
                m.insert("sourceFunding".into(), source_funding);
            }
            let source_risk = guard(&inner, None)?;
            if let Some(m) = ctx.borrow_mut().as_object_mut() {
                m.insert("sourceRisk".into(), source_risk.clone());
            }
            need(
                get(&source_risk, "unresolvedIntentIds")
                    .as_array()
                    .is_some_and(|a| a.is_empty()),
                "PENDING_BEFORE_BOOK",
            )?;
            let book = read(
                &inner,
                &format!(
                    "/markets/{}/orderbook?tournamentId={}&depth=20",
                    sget(&leg, "marketId").unwrap_or(""),
                    sget(&inner.borrow().scope, "tournamentId").unwrap_or("")
                ),
            )?;
            if let Some(m) = ctx.borrow_mut().as_object_mut() {
                m.insert("book".into(), book);
            }
            let (scope, legs) = {
                let g = inner.borrow();
                (g.scope.clone(), g.legs.clone())
            };
            let mut scope_with_legs = scope.as_object().cloned().unwrap_or_default();
            scope_with_legs.insert("exchanges".into(), json!(legs));
            let pre_none = plan_baal_scaled_profitable_exit_v2(&json!({
                "scope": V::Object(scope_with_legs),
                "owned": get(&ctx.borrow(), "owned"),
                "account": get(&ctx.borrow(), "account"),
                "bookReceipt": get(&ctx.borrow(), "book"),
                "globalRisk": get(&ctx.borrow(), "sourceRisk"),
                "now": iso((inner.borrow().deps.now)()),
            }));
            need(
                get(&pre_none, "proposalReady") == &V::Bool(true),
                &format!("PRE_NONE_BOOK_{}", sget(&pre_none, "reason").unwrap_or("")),
            )?;
            finish_none(&inner, ctx)
        })?;
        sell_evidence(&self.inner, "prepare_sell", None)
    }

    /// refreshPreparedNone()
    pub fn refresh_prepared_none(&self) -> Result<V, String> {
        need(!self.inner.borrow().busy.get(), "BUSY")?;
        let a = active(&self.inner)?;
        need(
            sget(&a, "phase") == Some("prepared")
                && get(&a, "orderId").is_null()
                && js_int(if get(&a, "actualFetchCount").is_null() {
                    get(&a, "fetchCount")
                } else {
                    get(&a, "actualFetchCount")
                }) == Some(0)
                && sget(&a, "bodyHash") == Some(payload_hash(get(&a, "body"))?.as_str())
                && sget(get(&a, "body"), "side") == Some("no")
                && instant_v(get(get(&a, "body"), "expirationDate"))? as f64
                    > (self.inner.borrow().deps.now)(),
            "EXACT_UNFETCHED_PREPARED",
        )?;
        let hedge_packet = self.inner.borrow().hedge_packet.borrow().clone();
        let action = sget(get(&a, "body"), "action").unwrap_or("").to_string();
        let ctx: V = if action == "buy" {
            need(
                !hedge_packet.is_null()
                    && js_int(get(get(&a, "body"), "quantity")) == Some(1)
                    && sget(&a, "firstExchangeId") == sget(&hedge_packet, "firstExchangeId"),
                "ORIGINAL_HEDGE_BINDING",
            )?;
            let leg = self
                .inner
                .borrow()
                .legs
                .iter()
                .find(|e| sget(e, "exchangeId") == sget(get(&a, "body"), "exchangeId"))
                .cloned()
                .unwrap_or(V::Null);
            let graph_leg = self
                .inner
                .borrow()
                .legs
                .iter()
                .find(|e| sget(e, "exchangeId") == sget(&a, "firstExchangeId"))
                .cloned()
                .unwrap_or(V::Null);
            need(
                !leg.is_null()
                    && !graph_leg.is_null()
                    && sget(get(&a, "body"), "tournamentId")
                        == sget(&self.inner.borrow().scope, "tournamentId"),
                "ORIGINAL_HEDGE_SCOPE",
            )?;
            json!({
                "kind": "hedgeFinal",
                "leg": leg,
                "graphLeg": graph_leg,
                "owned": get(&hedge_packet, "owned"),
                "ownedExchangeId": get(&a, "firstExchangeId"),
                "otherBookReceipt": get(&hedge_packet, "otherBookReceipt"),
                "book": get(&hedge_packet, "otherBookReceipt"),
                "hedgePacket": hedge_packet,
            })
        } else {
            let cache = self.inner.borrow().cache.borrow().clone();
            need(
                sget(get(&a, "body"), "action") == Some("sell")
                    && sget(&cache, "kind") == Some("sell")
                    && sget(get(&cache, "leg"), "exchangeId")
                        == sget(get(&a, "body"), "exchangeId")
                    && js_int(get(get(&a, "body"), "quantity")).unwrap_or(i64::MAX)
                        <= js_int(
                            get(get(get(&cache, "owned"), "proof"), "remainingOwnedQuantity"),
                        )
                        .unwrap_or(i64::MIN),
                "ORIGINAL_SELL_BINDING",
            )?;
            cache
        };
        let mut ctx_obj = ctx.as_object().cloned().unwrap_or_default();
        ctx_obj.insert("preparedBinding".into(), a.clone());
        let ctx = V::Object(ctx_obj);
        let pin_subject = if action == "buy" {
            get(&ctx, "hedgePacket").clone()
        } else {
            json!({
                "account": get(&ctx, "account"),
                "book": get(&ctx, "book"),
                "owned": get(&ctx, "owned"),
                "sourceRisk": get(&ctx, "sourceRisk"),
                "sourceFunding": get(&ctx, "sourceFunding"),
            })
        };
        let source_pin = payload_hash(&pin_subject)?;
        let inner = self.inner.clone();
        let cache = begin(&inner, ctx, 2, |ctx| finish_none(&inner, ctx))?;
        let pin_after = payload_hash(&if action == "buy" {
            get(&cache, "hedgePacket").clone()
        } else {
            json!({
                "account": get(&cache, "account"),
                "book": get(&cache, "book"),
                "owned": get(&cache, "owned"),
                "sourceRisk": get(&cache, "sourceRisk"),
                "sourceFunding": get(&cache, "sourceFunding"),
            })
        })?;
        need(source_pin == pin_after, "ORIGINAL_PACKET_CHANGED")?;
        evidence(
            &self.inner,
            &json!({
                "operation": if action == "buy" { "place_hedge" } else { "place_sell" }
            }),
        )
    }

    /// refreshFinalNone() — alias of refreshPreparedNone
    pub fn refresh_final_none(&self) -> Result<V, String> {
        self.refresh_prepared_none()
    }

    /// refreshReconciliation(orderId)
    pub fn refresh_reconciliation(&self, order_id: &str) -> Result<V, String> {
        let binding = order_binding(&self.inner, order_id)?;
        let leg = self
            .inner
            .borrow()
            .legs
            .iter()
            .find(|e| sget(e, "exchangeId") == sget(get(&binding, "body"), "exchangeId"))
            .cloned()
            .unwrap_or(V::Null);
        need(
            sget(&binding, "orderId") == Some(order_id)
                && !leg.is_null()
                && sget(&binding, "bodyHash")
                    == Some(payload_hash(get(&binding, "body"))?.as_str())
                && sget(get(&binding, "body"), "side") == Some("no")
                && ["buy", "sell"].contains(&sget(get(&binding, "body"), "action").unwrap_or(""))
                && sget(get(&binding, "body"), "tournamentId")
                    == sget(&self.inner.borrow().scope, "tournamentId")
                && js_int(get(get(&binding, "body"), "quantity"))
                    .is_some_and(|q| q > 0 && q <= 100),
            "KNOWN_OWN_BINDING",
        )?;
        let graph_leg = if sget(get(&binding, "body"), "action") == Some("buy") {
            let first = if sget(&binding, "firstExchangeId").is_some() {
                sget(&binding, "firstExchangeId").unwrap_or("").to_string()
            } else {
                self.inner.borrow().first_exchange_id.clone()
            };
            self.inner
                .borrow()
                .legs
                .iter()
                .find(|e| sget(e, "exchangeId") == Some(first.as_str()))
                .cloned()
                .unwrap_or(V::Null)
        } else {
            leg.clone()
        };
        need(!graph_leg.is_null(), "ACTUAL_ORDER_GRAPH_SCOPE")?;
        let inner = self.inner.clone();
        let inner2 = inner.clone();
        let oid = order_id.to_string();
        begin(
            &inner2,
            json!({"kind": "reconcile", "leg": leg, "graphLeg": graph_leg, "binding": binding}),
            9,
            move |ctx| {
                let order = read(&inner, &format!("/orders/{oid}"))?;
                if let Some(m) = ctx.borrow_mut().as_object_mut() {
                    m.insert("order".into(), order.clone());
                }
                let parsed = raw(&inner, &order, sget(&order, "endpoint").unwrap_or(""), 20000.0)?;
                let od = &parsed.0;
                let binding = get(&ctx.borrow(), "binding").clone();
                let leg = get(&ctx.borrow(), "leg").clone();
                let tid = sget(&inner.borrow().scope, "tournamentId")
                    .unwrap_or("")
                    .to_string();
                need(
                    js_int(get(od, "id")) == oid.parse::<i64>().ok()
                        && parsed.1.get(&js_stringify(&json!(["id"]))).cloned()
                            == Some(oid.clone())
                        && sget(od, "exchangeId") == sget(&leg, "exchangeId")
                        && sget(od, "tournamentId") == Some(tid.as_str())
                        && sget(od, "side") == Some("no")
                        && sget(od, "action") == sget(get(&binding, "body"), "action")
                        && js_int(get(od, "quantity")).is_some_and(|q| {
                            q >= 1
                                && q <= js_int(get(get(&binding, "body"), "quantity"))
                                    .unwrap_or(0)
                        })
                        && js_int(get(od, "quantityFilled")).is_some_and(|q| q >= 0)
                        && get(od, "open").is_boolean()
                        && (get(od, "open") == &V::Bool(true)
                            || js_int(get(od, "quantityFilled")).unwrap_or(i64::MAX)
                                <= js_int(get(od, "quantity")).unwrap_or(i64::MIN)),
                    "EXACT_OWN_ORDER_RESPONSE",
                )?;
                let fill_pages =
                    page(&inner, &format!("/orders/{oid}/fills?limit=200"))?;
                if let Some(m) = ctx.borrow_mut().as_object_mut() {
                    m.insert("fillPages".into(), json!(fill_pages.clone()));
                }
                let fills_parsed = raw(
                    &inner,
                    &fill_pages[0],
                    sget(&fill_pages[0], "endpoint").unwrap_or(""),
                    20000.0,
                )?;
                let fills = &fills_parsed.0;
                need(
                    js_int(get(fills, "orderId")) == oid.parse::<i64>().ok()
                        && sget(fills, "exchangeId") == sget(&leg, "exchangeId")
                        && sget(fills, "tournamentId") == Some(tid.as_str())
                        && get(fills, "data").as_array().is_some_and(|a| {
                            a.iter().all(|v| {
                                sget(v, "side") == Some("no")
                                    && js_int(get(v, "quantity")).is_some_and(|q| q < 0)
                            })
                        }),
                    "EXACT_OWN_FILL_RESPONSE",
                )?;
                let leg = get(&ctx.borrow(), "leg").clone();
                let after = account(&inner, &leg, true)?;
                if let Some(m) = ctx.borrow_mut().as_object_mut() {
                    m.insert("after".into(), after);
                }
                finish_none(&inner, ctx)
            },
        )?;
        evidence(
            &self.inner,
            &json!({"operation": "reconcile", "orderId": order_id}),
        )
    }

    pub fn status(&self) -> V {
        let g = self.inner.borrow();
        let hedge_status = g
            .hedge
            .borrow()
            .as_ref()
            .map(|h| h.status())
            .unwrap_or(V::Null);
        let cache_kind = {
            let c = g.cache.borrow();
            sget(&c, "kind").map(str::to_string).unwrap_or_default()
        };
        json!({
            "mode": "combined_owned_no_none_evidence_bridge_v4",
            "busy": g.busy.get(),
            "closed": g.closed.get(),
            "cacheKind": if cache_kind.is_empty() { V::Null } else { V::String(cache_kind) },
            "lastReadCount": g.reads.get(),
            "maximumReads": g.maximum_reads.get(),
            "totalReadCount": g.total_read_count.get(),
            "readTimings": g.read_timings.borrow().iter().cloned().collect::<Vec<V>>(),
            "hedge": hedge_status,
            "executionAuthority": false,
            "additionalApiClients": 0,
        })
    }

    pub fn clear(&self) {
        *self.inner.borrow().cache.borrow_mut() = V::Null;
        if let Some(h) = &*self.inner.borrow().hedge.borrow() {
            h.clear();
        }
    }

    pub fn close(&self) -> Result<(), String> {
        need(!self.inner.borrow().busy.get(), "BUSY")?;
        self.inner.borrow().closed.set(true);
        *self.inner.borrow().cache.borrow_mut() = V::Null;
        if let Some(h) = &*self.inner.borrow().hedge.borrow() {
            let _ = h.close();
        }
        self.inner.borrow().watermarks.borrow_mut().clear();
        Ok(())
    }
}
