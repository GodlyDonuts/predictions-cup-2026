//! Port of execution/owned-no-fresh-none-hedge-evidence-bridge-v2.mjs.
//! Read-only shared-client combined evidence wrapper. async -> sync closures.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use serde_json::{json, Map, Value as V};

use baal_service_runtime::fixed::{instant, price};
use baal_service_runtime::json_text::{js_stringify, sha256 as sha};

use crate::core::payload_hash;
use crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::owned_no_fresh_none_hedge_evidence_bridge_v1::{
    create_owned_no_fresh_none_hedge_evidence_bridge, EvidenceBridgeApi,
    OwnedNoFreshNoneBridgeDeps, OwnedNoFreshNoneHedgeEvidenceBridge,
};
use crate::paired_rest_controller::WireDispatchError;
use crate::receipts::parse_number_lexemes;
use crate::transport::{Guard, OrderTransportError};

fn need(v: bool, code: &str) -> Result<(), String> {
    if v {
        Ok(())
    } else {
        Err(format!("COMBINED_NONE_EVIDENCE_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    get(v, k).as_str()
}
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    })
}
fn digit_id(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(payload_hash(a)? == payload_hash(b)?)
}
fn iso(n: f64) -> String {
    baal_service_runtime::contract::ms_to_iso(n)
}
fn inst(v: &V, k: &str) -> Result<i64, String> {
    instant(sget(v, k).unwrap_or(""))
}
fn is_safe_int(v: &V) -> bool {
    v.as_f64()
        .is_some_and(|x| x.is_finite() && x.fract() == 0.0 && x.abs() <= 9007199254740991.0)
}
fn js_num_string(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else {
                n.as_f64().map(|f| format!("{f}")).unwrap_or_default()
            }
        }
        _ => v.to_string(),
    }
}
fn ote_for(code: String) -> OrderTransportError {
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

#[derive(Clone)]
pub struct CombinedNoneEvidenceDeps {
    pub api: Rc<EvidenceBridgeApi>,
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub get_owned: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub get_pair_census: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub get_funding: Rc<dyn Fn() -> Result<V, String>>,
    pub get_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub can_read: Rc<dyn Fn() -> bool>,
    pub can_check_read_identity: Option<Rc<dyn Fn() -> bool>>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> bool>,
    pub get_active_intent: Rc<dyn Fn() -> Result<V, String>>,
    pub get_order_binding: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub refresh_funding_after_account: Option<Rc<dyn Fn() -> Result<V, String>>>,
    pub now: Rc<dyn Fn() -> f64>,
}

pub(crate) struct BridgeV2Inner {
    scope: V,
    first_exchange_id: String,
    legs: Vec<V>,
    hedge: Option<OwnedNoFreshNoneHedgeEvidenceBridge>,
    hedge_packet: Option<V>,
    cache: Option<V>,
    job: Option<V>,
    read_timings: Vec<V>,
    total_read_count: i64,
    busy: bool,
    closed: bool,
    reads: i64,
    maximum_reads: i64,
    last_end: Option<i64>,
    permits: HashMap<usize, (String, usize)>,
    watermarks: HashMap<String, (i64, i64)>,
    invocations: HashSet<String>,
    deps: CombinedNoneEvidenceDeps,
    v2: bool,
}

pub struct OwnedNoFreshNoneHedgeEvidenceBridgeV2 {
    inner: Rc<RefCell<BridgeV2Inner>>,
}

impl OwnedNoFreshNoneHedgeEvidenceBridgeV2 {
    fn active(&self) -> Result<V, String> {
        (self.inner.borrow().deps.get_active_intent)()
    }
    fn fund(&self) -> Result<V, String> {
        (self.inner.borrow().deps.get_funding)()
    }
    fn raw(
        &self,
        r: &V,
        endpoint: &str,
        age: i64,
    ) -> Result<(V, BTreeMap<String, String>), String> {
        let allowed = [
            "method",
            "endpoint",
            "status",
            "requestBodyHash",
            "bodyText",
            "sourceBodyHash",
            "requestStartedAt",
            "receivedAt",
            "fetchInvocationId",
        ];
        let now = (self.inner.borrow().deps.now)() as i64;
        need(
            sget(r, "method") == Some("GET")
                && sget(r, "endpoint") == Some(endpoint)
                && get(r, "status") == &json!(200)
                && get(r, "requestBodyHash").is_null()
                && get(r, "bodyText").is_string()
                && sget(r, "bodyText")
                    .map(|s| s.as_bytes().len())
                    .unwrap_or(0)
                    <= 8 * 1024 * 1024
                && sget(r, "sourceBodyHash")
                    == Some(sha(sget(r, "bodyText").unwrap_or("").as_bytes()).as_str())
                && sget(r, "fetchInvocationId").is_some_and(|s| !s.is_empty())
                && r.as_object()
                    .is_some_and(|m| m.keys().all(|k| allowed.contains(&k.as_str()))),
            "RAW_SOURCE",
        )?;
        let start = inst(r, "requestStartedAt")?;
        let end = inst(r, "receivedAt")?;
        need(
            start <= end && end <= now && now - start <= age,
            "SOURCE_AGE",
        )?;
        parse_number_lexemes(sget(r, "bodyText").unwrap_or(""))
    }
    fn identity(&self) -> Result<(), String> {
        let inner = self.inner.borrow();
        let check = inner
            .deps
            .can_check_read_identity
            .clone()
            .unwrap_or_else(|| inner.deps.can_read.clone());
        let check = if inner.v2 { inner.deps.can_read.clone() } else { check };
        need(!inner.closed && check(), "STOPPED")?;
        let l = (inner.deps.get_account_lease)()?;
        need(
            sget(&l, "ownerId") == sget(&inner.scope, "ownerId")
                && sget(&l, "tournamentId") == sget(&inner.scope, "tournamentId")
                && hex(get(&l, "leaseId")),
            "ACCOUNT_LEASE",
        )?;
        need(
            !(inner.deps.api.manual_review)() && (inner.deps.now)() >= (inner.deps.api.cooldown)(),
            "BACKOFF",
        )?;
        Ok(())
    }
    fn ctx_now(&self) -> Option<V> {
        let inner = self.inner.borrow();
        inner.job.clone().or_else(|| inner.cache.clone())
    }
    fn guard(&self, fence_risk: Option<&V>) -> Result<V, String> {
        self.identity()?;
        {
            let inner = self.inner.borrow();
            if !inner.v2 {
                need((inner.deps.can_read)(), "STOPPED")?;
            }
        }
        let g = match fence_risk {
            None => (self.inner.borrow().deps.get_global_risk)()?,
            Some(f) => f.clone(),
        };
        let ctx = self.ctx_now().unwrap_or(V::Null);
        let a = self.active()?;
        let inner = self.inner.borrow();
        let now = (inner.deps.now)();
        need(
            sget(&g, "ownerId") == sget(&inner.scope, "ownerId")
                && sget(&g, "runId") == sget(&inner.scope, "runId")
                && sget(&g, "tournamentId") == sget(&inner.scope, "tournamentId")
                && get(&g, "complete") == &V::Bool(true)
                && get(&g, "eventScopeComplete") == &V::Bool(true)
                && get(&g, "soleCollector") == &V::Bool(true)
                && get(&g, "stopped") == &V::Bool(false)
                && hex(get(&g, "sourceHash"))
                && get(&g, "unresolvedIntentIds").is_array()
                && inst(&g, "at")? <= now as i64
                && now as i64 - inst(&g, "at")? <= 20000,
            "GLOBAL_SCOPE",
        )?;
        let pb = get(&ctx, "preparedBinding");
        if !pb.is_null()
            && sget(&a, "intentId") == sget(pb, "intentId")
            && get(&a, "orderId").is_null()
            && {
                let afc = get(&a, "actualFetchCount");
                let fc = get(&a, "fetchCount");
                if !afc.is_null() {
                    afc.as_i64() == Some(0)
                } else {
                    fc.as_i64() == Some(0)
                }
            }
        {
            need(
                inst(get(&a, "body"), "expirationDate")? > now as i64,
                "PREPARED_EXPIRED",
            )?;
        }
        let leg = get(&ctx, "leg");
        let pending_own = if !ctx.is_null()
            && !a.is_null()
            && sget(&a, "intentId").is_some()
            && sget(&a, "bodyHash") == Some(payload_hash(get(&a, "body"))?.as_str())
            && sget(get(&a, "body"), "exchangeId") == sget(leg, "exchangeId")
            && sget(get(&a, "body"), "tournamentId") == sget(&inner.scope, "tournamentId")
            && sget(get(&a, "body"), "side") == Some("no")
            && (sget(get(&a, "body"), "action") == Some("sell")
                || (sget(&ctx, "kind") == Some("hedgeFinal")
                    && sget(get(&a, "body"), "action") == Some("buy")
                    && get(get(&a, "body"), "quantity") == &json!(1)))
            && ["prepared", "dispatching"].contains(&sget(&a, "phase").unwrap_or(""))
            && get(&a, "orderId").is_null()
            && {
                let afc = get(&a, "actualFetchCount");
                let fc = get(&a, "fetchCount");
                if !afc.is_null() {
                    afc.as_i64() == Some(0)
                } else {
                    fc.as_i64() == Some(0)
                }
            }
            && is_safe_int(get(get(&a, "body"), "quantity"))
            && get(get(&a, "body"), "quantity").as_i64().unwrap_or(0) > 0
            && get(get(&a, "body"), "quantity").as_i64().unwrap_or(0)
                <= if sget(&ctx, "kind") == Some("hedgeFinal") {
                    1
                } else {
                    get(get(get(&ctx, "owned"), "proof"), "remainingOwnedQuantity")
                        .as_i64()
                        .unwrap_or(0)
                }
            && inst(get(&a, "body"), "expirationDate")? > now as i64
        {
            get(&g, "unresolvedIntentIds")
                .as_array()
                .is_some_and(|ids| ids.iter().all(|id| id == get(&a, "intentId")))
        } else {
            false
        };
        let binding = get(&ctx, "binding");
        let known = if binding.is_null() {
            V::Null
        } else {
            (inner.deps.get_order_binding)(get(binding, "orderId"))?
        };
        let cleanup = !binding.is_null()
            && !known.is_null()
            && sget(&known, "intentId") == sget(binding, "intentId")
            && sget(&known, "orderId") == sget(binding, "orderId")
            && sget(&known, "bodyHash") == sget(binding, "bodyHash")
            && same(get(&known, "body"), get(binding, "body"))?
            && get(&g, "unresolvedIntentIds").as_array().is_some_and(|ids| {
                ids.iter().all(|id| id == get(binding, "intentId"))
            });
        need(
            (get(&g, "unknownAcceptance") == &V::Bool(false) || cleanup)
                && (get(&g, "unresolvedIntentIds")
                    .as_array()
                    .map(|x| x.is_empty())
                    .unwrap_or(false)
                    || pending_own
                    || cleanup),
            "UNKNOWN_OR_FOREIGN_PENDING",
        )?;
        if !pb.is_null() {
            need(
                sget(&a, "intentId") == sget(pb, "intentId")
                    && sget(&a, "bodyHash") == sget(pb, "bodyHash")
                    && same(get(&a, "body"), get(pb, "body"))?
                    && (inner.job.is_none() || sget(&a, "phase") == Some("prepared")),
                "PREPARED_BODY_CHANGED",
            )?;
        }
        let owned_ctx = get(&ctx, "owned");
        if !owned_ctx.is_null() {
            let exid = if sget(&ctx, "ownedExchangeId").is_some() {
                sget(&ctx, "ownedExchangeId").unwrap_or("").to_string()
            } else {
                sget(leg, "exchangeId").unwrap_or("").to_string()
            };
            let current = (inner.deps.get_owned)(&json!(exid))?;
            need(same(&current, owned_ctx)?, "CURRENT_CUSTODY_CHANGED")?;
        }
        Ok(g)
    }
    fn none(&self, ctx: &V) -> Result<(), String> {
        let (scope, legs) = {
            let inner = self.inner.borrow();
            (inner.scope.clone(), inner.legs.clone())
        };
        let graph_leg = if !get(ctx, "graphLeg").is_null() {
            get(ctx, "graphLeg").clone()
        } else {
            get(ctx, "leg").clone()
        };
        let graph_endpoint = format!(
            "/relationships/graph?marketId={}&depth=4&tournamentId={}",
            sget(&graph_leg, "marketId").unwrap_or(""),
            sget(&scope, "tournamentId").unwrap_or("")
        );
        let collateral_endpoint = format!(
            "/portfolio/collateral?tournamentId={}",
            sget(&scope, "tournamentId").unwrap_or("")
        );
        let graph = self.raw(get(ctx, "graphReceipt"), &graph_endpoint, 5000)?.0;
        let c = self
            .raw(get(ctx, "collateralReceipt"), &collateral_endpoint, 5000)?
            .0;
        let nodes = get(&graph, "nodes");
        let uniq: HashSet<String> = nodes
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|n| sget(n, "exchangeId").unwrap_or("").to_string())
            .collect();
        need(
            !graph.is_null()
                && graph
                    .as_object()
                    .map(|m| {
                        let mut ks: Vec<&String> = m.keys().collect();
                        ks.sort();
                        ks.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("|")
                            == "nodes|relationships"
                    })
                    .unwrap_or(false)
                && get(&graph, "relationships")
                    .as_array()
                    .map(|x| x.is_empty())
                    .unwrap_or(false)
                && nodes.as_array().map(|x| x.len() <= 2).unwrap_or(false)
                && nodes.as_array().cloned().unwrap_or_default().iter().all(|n| {
                    legs.iter().any(|e| {
                        sget(e, "marketId") == sget(n, "marketId")
                            && sget(e, "exchangeId") == sget(n, "exchangeId")
                    })
                })
                && uniq.len() == nodes.as_array().map(|x| x.len()).unwrap_or(0),
            "NON_NONE_GRAPH",
        )?;
        need(
            !c.is_null()
                && c.as_object()
                    .map(|m| {
                        m.keys().map(|k| k.as_str()).collect::<Vec<_>>().join("|") == "data"
                    })
                    .unwrap_or(false)
                && get(&c, "data")
                    .as_array()
                    .map(|x| x.is_empty())
                    .unwrap_or(false),
            "NON_NONE_COLLATERAL",
        )?;
        let preceding = {
            let book = get(ctx, "book");
            if !book.is_null() {
                inst(book, "receivedAt")?
            } else {
                let leg1_ex = sget(&legs[1], "exchangeId").unwrap_or("");
                let arr = get(
                    get(get(ctx, "after"), "scopedOrdersPagesByExchange"),
                    leg1_ex,
                )
                .as_array()
                .cloned()
                .unwrap_or_default();
                inst(&arr.last().cloned().unwrap_or(V::Null), "receivedAt")?
            }
        };
        need(
            inst(get(ctx, "graphReceipt"), "requestStartedAt")? >= preceding
                && inst(get(ctx, "collateralReceipt"), "requestStartedAt")?
                    >= inst(get(ctx, "graphReceipt"), "receivedAt")?,
            "NONE_AFTER_OWN_SOURCE",
        )?;
        Ok(())
    }
    fn read(&self, endpoint: &str) -> Result<V, String> {
        self.identity()?;
        {
            let inner = self.inner.borrow();
            need(
                inner.reads < inner.maximum_reads
                    && (inner.deps.has_read_headroom)(inner.maximum_reads - inner.reads),
                "READ_BOUND",
            )?;
        }
        let admitted = Rc::new(Cell::new(false));
        let network = Rc::new(Cell::new(false));
        let inner_a = self.inner.clone();
        let admitted_a = admitted.clone();
        let self_a = OwnedNoFreshNoneHedgeEvidenceBridgeV2 {
            inner: self.inner.clone(),
        };
        let bd_impl: Box<Guard<'_>> = Box::new(move |_: &V, _: &V| {
            self_a.guard(None).map_err(ote_for)?;
            let g = inner_a.borrow();
            let ok = !admitted_a.get()
                && (g.deps.has_read_headroom)(g.maximum_reads - g.reads);
            drop(g);
            if !ok {
                return Err(ote_for("COMBINED_NONE_EVIDENCE_ADMISSION".into()));
            }
            admitted_a.set(true);
            Ok(true)
        });
        let inner_n = self.inner.clone();
        let admitted_n = admitted.clone();
        let network_n = network.clone();
        let self_n = OwnedNoFreshNoneHedgeEvidenceBridgeV2 {
            inner: self.inner.clone(),
        };
        let bn_impl: Box<Guard<'_>> = Box::new(move |_: &V, _: &V| {
            self_n.guard(None).map_err(ote_for)?;
            let g = inner_n.borrow();
            let ok = admitted_n.get()
                && !network_n.get()
                && (g.deps.has_read_headroom)(g.maximum_reads - g.reads - 1);
            drop(g);
            if !ok {
                return Err(ote_for("COMBINED_NONE_EVIDENCE_NETWORK".into()));
            }
            network_n.set(true);
            Ok(true)
        });
        let bd_guard: &Guard<'_> = &*bd_impl;
        let bn_guard: &Guard<'_> = &*bn_impl;
        let bd_ptr = bd_guard as *const Guard as *const () as usize;
        let bn_ptr = bn_guard as *const Guard as *const () as usize;
        let began = (self.inner.borrow().deps.now)();
        self.inner
            .borrow_mut()
            .permits
            .insert(bd_ptr, (endpoint.to_string(), bn_ptr));
        let spec = json!({"method": "GET", "endpoint": endpoint});
        let body_result: Result<V, String> = (|| {
            let response = (self.inner.borrow().deps.api.dispatch)(
                &spec,
                Some(bd_guard),
                Some(bn_guard),
            )
            .map_err(|e: WireDispatchError| e.message)?;
            self.identity()?;
            need(
                admitted.get()
                    && network.get()
                    && sget(&response, "endpoint") == Some(endpoint)
                    && get(&response, "status") == &json!(200),
                "RESPONSE",
            )?;
            let r =
                (self.inner.borrow().deps.receipt_of)(&response, &json!({"method": "GET"}))?;
            let p = self.raw(&r, endpoint, 20_000)?;
            need(
                inst(&r, "requestStartedAt")? >= began as i64
                    && inst(&r, "requestStartedAt")? >= inst(&response, "requestStartedAt")?
                    && inst(&r, "receivedAt")? <= inst(&response, "receivedAt")?
                    && same(&p.0, get(&response, "data"))?
                    && self
                        .inner
                        .borrow()
                        .last_end
                        .map(|le| inst(&r, "requestStartedAt").map(|x| x >= le))
                        .transpose()?
                        .unwrap_or(true)
                    && !self
                        .inner
                        .borrow()
                        .invocations
                        .contains(sget(&r, "fetchInvocationId").unwrap_or("")),
                "RAW_CAUSALITY",
            )?;
            {
                let inner = self.inner.borrow();
                if let Some((os, oe)) = inner.watermarks.get(endpoint) {
                    need(
                        inst(&r, "requestStartedAt")? >= *os
                            && inst(&r, "receivedAt")? >= *oe,
                        "RAW_ROLLBACK",
                    )?;
                }
            }
            self.inner.borrow_mut().watermarks.insert(
                endpoint.to_string(),
                (inst(&r, "requestStartedAt")?, inst(&r, "receivedAt")?),
            );
            self.inner
                .borrow_mut()
                .invocations
                .insert(sget(&r, "fetchInvocationId").unwrap_or("").to_string());
            let end = inst(&r, "receivedAt")?;
            self.inner.borrow_mut().last_end = Some(end);
            Ok(r)
        })();
        {
            let mut inner = self.inner.borrow_mut();
            if !inner.v2 {
                let now_ms = (inner.deps.now)();
                inner.read_timings.push(json!({
                    "endpoint": endpoint,
                    "startedAt": iso(began),
                    "finishedAt": iso(now_ms),
                    "elapsedMs": now_ms - began,
                    "admitted": admitted.get(),
                    "network": network.get(),
                }));
                if inner.read_timings.len() > 20 {
                    inner.read_timings.remove(0);
                }
            }
            inner.reads += 1;
            inner.total_read_count += 1;
            inner.permits.remove(&bd_ptr);
        }
        body_result
    }
    fn page(&self, endpoint: &str) -> Result<Vec<V>, String> {
        let r = self.read(endpoint)?;
        let p = self.raw(&r, endpoint, 20_000)?;
        let d = &p.0;
        need(
            get(d, "data").as_array().is_some_and(|x| x.len() <= 200)
                && get(get(d, "pagination"), "limit") == &json!(200)
                && get(get(d, "pagination"), "hasMore") == &V::Bool(false)
                && get(get(d, "pagination"), "nextCursor").is_null()
                && get(get(d, "coverage"), "complete") == &V::Bool(true)
                && (get(get(d, "coverage"), "projectedThroughSequence").is_null()
                    || (is_safe_int(get(get(d, "coverage"), "projectedThroughSequence"))
                        && get(get(d, "coverage"), "projectedThroughSequence")
                            .as_i64()
                            .unwrap_or(-1)
                            >= 0)),
            "COMPLETE_SINGLE_PAGE_REQUIRED",
        )?;
        let mut ids = HashSet::new();
        for (i, v) in get(d, "data")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let key = js_stringify(&json!(["data", i, "id"]));
            let n = p.1.get(&key);
            need(
                n.is_some_and(|n| digit_id(n))
                    && is_safe_int(get(v, "id"))
                    && !ids.contains(&get(v, "id").as_i64().unwrap_or(-1)),
                "EXACT_ID",
            )?;
            ids.insert(get(v, "id").as_i64().unwrap_or(-1));
        }
        Ok(vec![r])
    }
    fn account(&self, target: &V, both: bool) -> Result<V, String> {
        let scope = self.inner.borrow().scope.clone();
        let legs = self.inner.borrow().legs.clone();
        let slug = sget(&scope, "tournamentSlug").unwrap_or("");
        let tid = sget(&scope, "tournamentId").unwrap_or("");
        let cash = self.read(&format!("/tournaments/{slug}"))?;
        let positions = self.read(&format!("/tournaments/{slug}/portfolio/positions"))?;
        let open_orders_pages = self.page(&format!(
            "/orders?status=open&limit=200&tournamentId={tid}"
        ))?;
        let mut scoped = Map::new();
        let selected: Vec<V> = if both {
            legs.clone()
        } else {
            vec![target.clone()]
        };
        for e in &selected {
            let pages = self.page(&format!(
                "/orders?status=all&exchangeId={}&limit=200&tournamentId={tid}",
                sget(e, "exchangeId").unwrap_or("")
            ))?;
            scoped.insert(
                sget(e, "exchangeId").unwrap_or("").to_string(),
                V::Array(pages),
            );
        }
        let scoped_for_target = scoped
            .get(sget(target, "exchangeId").unwrap_or(""))
            .cloned()
            .unwrap_or(V::Null);
        let mut out = Map::new();
        out.insert("cash".into(), cash);
        out.insert("positions".into(), positions);
        out.insert("openOrdersPages".into(), V::Array(open_orders_pages));
        out.insert("scopedOrdersPages".into(), scoped_for_target);
        out.insert("scopedOrdersPagesByExchange".into(), V::Object(scoped));
        Ok(V::Object(out))
    }
    fn finish_none(&self, ctx: &mut V) -> Result<(), String> {
        let (scope, legs) = {
            let inner = self.inner.borrow();
            (inner.scope.clone(), inner.legs.clone())
        };
        let graph_leg = if !get(ctx, "graphLeg").is_null() {
            get(ctx, "graphLeg").clone()
        } else {
            get(ctx, "leg").clone()
        };
        let _ = legs;
        let graph_receipt = self.read(&format!(
            "/relationships/graph?marketId={}&depth=4&tournamentId={}",
            sget(&graph_leg, "marketId").unwrap_or(""),
            sget(&scope, "tournamentId").unwrap_or("")
        ))?;
        let collateral_receipt = self.read(&format!(
            "/portfolio/collateral?tournamentId={}",
            sget(&scope, "tournamentId").unwrap_or("")
        ))?;
        ctx.as_object_mut()
            .unwrap()
            .insert("graphReceipt".into(), graph_receipt);
        ctx.as_object_mut()
            .unwrap()
            .insert("collateralReceipt".into(), collateral_receipt);
        self.none(ctx)
    }
    fn sell_evidence(&self, operation: &str, fence_risk: Option<&V>) -> Result<V, String> {
        let g = self.guard(fence_risk)?;
        let (cache, busy, scope, legs) = {
            let inner = self.inner.borrow();
            (
                inner.cache.clone().unwrap_or(V::Null),
                inner.busy,
                inner.scope.clone(),
                inner.legs.clone(),
            )
        };
        need(sget(&cache, "kind") == Some("sell") && !busy, "SELL_CACHE")?;
        self.none(&cache)?;
        if operation == "prepare_sell" {
            need(self.active()?.is_null(), "ACTIVE_BEFORE_PREPARE")?;
        }
        let f = self.fund()?;
        need(
            sget(get(&f, "record"), "reservationHash")
                == sget(&g, "controllerReservationHash")
                && sget(get(&f, "receipt"), "globalLedgerHash")
                    == sget(&g, "globalLedgerHash")
                && sget(get(&f, "receipt"), "reservationHash")
                    == sget(get(&f, "record"), "reservationHash"),
            "CURRENT_INSTALLED_BUDGET",
        )?;
        let economic = |r: &V| -> V {
            let mut m = r.as_object().cloned().unwrap_or_default();
            m.shift_remove("reservationHash");
            m.shift_remove("unresolvedIntentIds");
            V::Object(m)
        };
        need(
            same(
                &economic(get(&f, "record")),
                &economic(get(get(&cache, "sourceFunding"), "record")),
            )?,
            "CURRENT_FULL_ECONOMICS_CHANGED",
        )?;
        let active_now = self.active()?;
        need(
            get(get(&cache, "sourceRisk"), "unresolvedIntentIds")
                .as_array()
                .map(|x| x.is_empty())
                .unwrap_or(false)
                && (get(get(&f, "record"), "unresolvedIntentIds")
                    .as_array()
                    .map(|x| x.is_empty())
                    .unwrap_or(true)
                    || (get(get(&f, "record"), "unresolvedIntentIds")
                        .as_array()
                        .map(|x| x.len() == 1 && &x[0] == get(&active_now, "intentId"))
                        .unwrap_or(false))),
            "EXACT_OWN_SELL_PENDING",
        )?;
        let signal_risk = get(&cache, "sourceRisk").clone();
        let mut scope_v = scope.as_object().cloned().unwrap_or_default();
        scope_v.insert("exchanges".into(), V::Array(legs.clone()));
        let p = crate::baal_scaled_profitable_exit_signal_v2::plan_baal_scaled_profitable_exit_v2(
            &json!({
                "scope": V::Object(scope_v),
                "owned": get(&cache,"owned"),
                "account": get(&cache,"account"),
                "bookReceipt": get(&cache,"book"),
                "globalRisk": signal_risk,
                "now": iso((self.inner.borrow().deps.now)()),
            }),
        );
        let reason = sget(&p, "reason").unwrap_or("").to_string();
        need(
            get(&p, "proposalReady") == &V::Bool(true),
            &format!("CURRENT_FIFO_OR_PROFIT_{reason}"),
        )?;
        if !active_now.is_null() {
            need(
                get(get(&active_now, "body"), "quantity").as_i64().unwrap_or(0)
                    <= get(&p, "quantity").as_i64().unwrap_or(0)
                    && price(&js_num_string(get(get(&active_now, "body"), "price")))?
                        <= price(sget(&p, "limit").unwrap_or(""))?,
                "FIXED_PROFITABLE_INTENT_DEPTH",
            )?;
        }
        Ok(json!({
            "account": get(&cache,"account"),
            "book": get(&cache,"book"),
            "globalRisk": signal_risk,
            "currentGlobalRisk": g,
            "graphReceipt": get(&cache,"graphReceipt"),
            "collateralReceipt": get(&cache,"collateralReceipt"),
            "funding": f,
            "owned": get(&cache,"owned"),
        }))
    }
    /// evidence(c)
    pub fn get_evidence(&self, c: &V) -> Result<V, String> {
        let op = sget(c, "operation").unwrap_or("prepare_hedge").to_string();
        if ["prepare_hedge", "place_hedge", "hedge"].contains(&op.as_str()) {
            let (hedge_has, cache, busy) = {
                let inner = self.inner.borrow();
                (
                    inner.hedge.is_some(),
                    inner.cache.clone().unwrap_or(V::Null),
                    inner.busy,
                )
            };
            let _ = busy;
            need(
                hedge_has || sget(&cache, "kind") == Some("hedgeFinal"),
                "HEDGE_CACHE",
            )?;
            if sget(&cache, "kind") == Some("hedgeFinal") {
                let fence = if get(c, "fenceRisk").is_null() {
                    None
                } else {
                    Some(get(c, "fenceRisk").clone())
                };
                let g = self.guard(fence.as_ref())?;
                self.none(&cache)?;
                let f = self.fund()?;
                let economic = |r: &V| -> V {
                    let mut m = r.as_object().cloned().unwrap_or_default();
                    m.shift_remove("reservationHash");
                    m.shift_remove("unresolvedIntentIds");
                    V::Object(m)
                };
                need(
                    same(
                        &economic(get(&f, "record")),
                        &economic(get(
                            get(&cache, "hedgePacket"),
                            "funding",
                        ).get("record")
                        .unwrap_or(&V::Null)),
                    )? && sget(get(&f, "record"), "reservationHash")
                        == sget(&g, "controllerReservationHash")
                        && sget(get(&f, "receipt"), "globalLedgerHash")
                            == sget(&g, "globalLedgerHash"),
                    "CURRENT_HEDGE_ECONOMICS_CHANGED",
                )?;
                let mut e = get(&cache, "hedgePacket")
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                e.insert(
                    "graphReceipt".into(),
                    get(&cache, "graphReceipt").clone(),
                );
                e.insert(
                    "collateralReceipt".into(),
                    get(&cache, "collateralReceipt").clone(),
                );
                let mut ane = get(get(&cache, "hedgePacket"), "allNoneEvidence")
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                ane.insert(
                    "graphReceipt".into(),
                    get(&cache, "graphReceipt").clone(),
                );
                ane.insert(
                    "collateralReceipt".into(),
                    get(&cache, "collateralReceipt").clone(),
                );
                ane.insert(
                    "sourceHash".into(),
                    json!(payload_hash(&json!({
                        "graph": get(get(&cache,"graphReceipt"),"sourceBodyHash"),
                        "collateral": get(get(&cache,"collateralReceipt"),"sourceBodyHash"),
                    }))?),
                );
                ane.insert(
                    "confirmedAt".into(),
                    get(get(&cache, "collateralReceipt"), "receivedAt").clone(),
                );
                e.insert("allNoneEvidence".into(), V::Object(ane));
                e.insert(
                    "now".into(),
                    json!(iso((self.inner.borrow().deps.now)())),
                );
                let e = V::Object(e);
                let p =
                    crate::baal_scaled_owned_no_hedge_proposal_v1::plan_baal_owned_no_hedge(&e);
                let reason = sget(&p, "reason").unwrap_or("").to_string();
                need(
                    get(&p, "proposalReady") == &V::Bool(true),
                    &format!("ORIGINAL_HEDGE_PACKET_{reason}"),
                )?;
                return Ok(e);
            }
            let inner = self.inner.borrow();
            let h = inner.hedge.as_ref().unwrap();
            let order_id = sget(c, "orderId");
            return h.get_evidence(&op, order_id);
        }
        if ["prepare_sell", "place_sell"].contains(&op.as_str()) {
            let fence = if get(c, "fenceRisk").is_null() {
                None
            } else {
                Some(get(c, "fenceRisk").clone())
            };
            return self.sell_evidence(&op, fence.as_ref());
        }
        let (cache, busy) = {
            let inner = self.inner.borrow();
            (inner.cache.clone().unwrap_or(V::Null), inner.busy)
        };
        need(
            op == "reconcile"
                && sget(&cache, "kind") == Some("reconcile")
                && !busy
                && (sget(c, "orderId").is_none()
                    || sget(c, "orderId") == sget(get(&cache, "binding"), "orderId")),
            "RECONCILE_CACHE",
        )?;
        let g = self.guard(None)?;
        self.none(&cache)?;
        let mut to_check: Vec<V> = Vec::new();
        to_check.push(get(&cache, "order").clone());
        to_check.extend(
            get(&cache, "fillPages")
                .as_array()
                .cloned()
                .unwrap_or_default(),
        );
        for key in ["cash", "positions"] {
            to_check.push(get(get(&cache, "after"), key).clone());
        }
        to_check.extend(
            get(get(&cache, "after"), "openOrdersPages")
                .as_array()
                .cloned()
                .unwrap_or_default(),
        );
        for (_, pages) in get(get(&cache, "after"), "scopedOrdersPagesByExchange")
            .as_object()
            .cloned()
            .unwrap_or_default()
        {
            to_check.extend(pages.as_array().cloned().unwrap_or_default());
        }
        for r in to_check {
            self.raw(&r, sget(&r, "endpoint").unwrap_or(""), 20_000)?;
        }
        Ok(json!({
            "order": get(&cache,"order"),
            "fillPages": get(&cache,"fillPages"),
            "after": get(&cache,"after"),
            "globalRisk": g,
            "graphReceipt": get(&cache,"graphReceipt"),
            "collateralReceipt": get(&cache,"collateralReceipt"),
        }))
    }
    /// authorizes(spec, beforeDispatch, beforeNetwork)
    pub fn authorizes(
        &self,
        spec: &V,
        before_dispatch: Option<&Guard<'_>>,
        before_network: Option<&Guard<'_>>,
    ) -> bool {
        let r: Result<bool, String> = (|| {
            need(!self.inner.borrow().closed, "CLOSED")?;
            let bd_ptr = before_dispatch
                .map(|x| x as *const Guard as *const () as usize)
                .unwrap_or(0);
            let bn_ptr = before_network
                .map(|x| x as *const Guard as *const () as usize)
                .unwrap_or(0);
            let inner = self.inner.borrow();
            let p = inner.permits.get(&bd_ptr);
            if p.is_none() {
                return Ok(inner
                    .hedge
                    .as_ref()
                    .map(|h| h.authorizes(spec, before_dispatch, before_network))
                    .unwrap_or(false));
            }
            let p = p.unwrap().clone();
            drop(inner);
            self.identity()?;
            Ok(sget(spec, "method") == Some("GET")
                && sget(spec, "endpoint") == Some(p.0.as_str())
                && p.1 == bn_ptr
                && spec.as_object().is_some_and(|m| {
                    !m.contains_key("body")
                        && !m.contains_key("bodyJson")
                        && !m.contains_key("headers")
                }))
        })();
        r.unwrap_or(false)
    }
    fn begin<F>(&self, ctx: &mut V, max: i64, f: F) -> Result<V, String>
    where
        F: FnOnce(&OwnedNoFreshNoneHedgeEvidenceBridgeV2, &mut V) -> Result<(), String>,
    {
        need(!self.inner.borrow().busy, "BUSY")?;
        {
            let mut inner = self.inner.borrow_mut();
            inner.cache = None;
            inner.job = Some(ctx.clone());
            inner.busy = true;
            inner.reads = 0;
            inner.maximum_reads = max;
            inner.last_end = None;
            inner.invocations.clear();
        }
        let result: Result<(), String> = (|| {
            self.identity()?;
            f(self, ctx)?;
            self.inner.borrow_mut().cache = Some(ctx.clone());
            Ok(())
        })();
        {
            let mut inner = self.inner.borrow_mut();
            inner.busy = false;
            inner.job = None;
        }
        result?;
        Ok(ctx.clone())
    }
    /// refresh()
    pub fn refresh(&self) -> Result<V, String> {
        need(!self.inner.borrow().busy, "BUSY")?;
        {
            let mut inner = self.inner.borrow_mut();
            inner.cache = None;
            inner.job = None;
            if let Some(h) = inner.hedge.take() {
                drop(inner);
                let _ = h.close();
                self.inner.borrow_mut().hedge = None;
            }
        }
        if self.inner.borrow().deps.refresh_funding_after_account.is_some() {
            let first_exchange_id = self.inner.borrow().first_exchange_id.clone();
            let owned =
                (self.inner.borrow().deps.get_owned)(&json!(first_exchange_id.clone()))?;
            let legs = self.inner.borrow().legs.clone();
            let graph_leg = legs
                .iter()
                .find(|e| sget(e, "exchangeId") == Some(first_exchange_id.as_str()))
                .cloned()
                .unwrap_or(V::Null);
            let leg = legs
                .iter()
                .find(|e| sget(e, "exchangeId") != Some(first_exchange_id.as_str()))
                .cloned()
                .unwrap_or(V::Null);
            need(
                get(get(&owned, "proof"), "remainingOwnedQuantity")
                    .as_i64()
                    .unwrap_or(0)
                    > 0
                    && !graph_leg.is_null()
                    && !leg.is_null(),
                "CURRENT_HEDGE_SEED",
            )?;
            let mut ctx = json!({
                "kind": "hedgeFinal",
                "leg": leg,
                "graphLeg": graph_leg,
                "owned": owned,
                "ownedExchangeId": first_exchange_id,
            });
            self.begin(&mut ctx, 10, |this, ctx| {
                let mut root_receipts = Vec::new();
                for e in [
                    get(ctx, "graphLeg").clone(),
                    get(ctx, "leg").clone(),
                ] {
                    let receipt = this.read(&format!(
                        "/markets/{}/nodes?tournamentId={}",
                        sget(&e, "marketId").unwrap_or(""),
                        sget(&this.inner.borrow().scope, "tournamentId").unwrap_or("")
                    ))?;
                    let mut er = e.as_object().cloned().unwrap_or_default();
                    er.insert("receipt".into(), receipt);
                    root_receipts.push(V::Object(er));
                }
                let account_source = this.account(get(ctx, "graphLeg"), true)?;
                let mut binding = this
                    .inner
                    .borrow()
                    .scope
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                if let Some(m) = get(ctx, "graphLeg").as_object() {
                    for (k, v) in m {
                        binding.insert(k.clone(), v.clone());
                    }
                }
                let view = inspect_scoped_rest_no_account(&json!({
                    "account": account_source,
                    "binding": V::Object(binding),
                    "readAt": iso((this.inner.borrow().deps.now)()),
                }))?;
                let positions = get(&view, "positions").as_array().cloned().unwrap_or_default();
                need(
                    positions.iter().any(|p| {
                        sget(p, "exchangeId")
                            == sget(get(ctx, "graphLeg"), "exchangeId")
                            && sget(p, "marketId")
                                == sget(get(ctx, "graphLeg"), "marketId")
                            && get(p, "quantity").as_i64().unwrap_or(0)
                                == -get(get(get(ctx, "owned"), "proof"), "remainingOwnedQuantity")
                                    .as_i64()
                                    .unwrap_or(0)
                            && get(p, "settled") != &V::Bool(true)
                    }) && !positions.iter().any(|p| {
                        sget(p, "exchangeId") == sget(get(ctx, "leg"), "exchangeId")
                            && get(p, "quantity").as_i64().unwrap_or(0) != 0
                    }),
                    "CURRENT_SOURCE_CUSTODY",
                )?;
                if let Some(rfa) = &this.inner.borrow().deps.refresh_funding_after_account {
                    let rfa = rfa.clone();
                    drop(this.inner.borrow());
                    rfa()?;
                }
                let funding = this.fund()?;
                let pair_census = (this.inner.borrow().deps.get_pair_census)(&json!(
                    this.inner.borrow().first_exchange_id.clone()
                ))?;
                let global_risk = this.guard(None)?;
                let other_book_receipt = this.read(&format!(
                    "/markets/{}/orderbook?tournamentId={}&depth=20",
                    sget(get(ctx, "leg"), "marketId").unwrap_or(""),
                    sget(&this.inner.borrow().scope, "tournamentId").unwrap_or("")
                ))?;
                ctx.as_object_mut().unwrap().insert(
                    "otherBookReceipt".into(),
                    other_book_receipt.clone(),
                );
                ctx.as_object_mut()
                    .unwrap()
                    .insert("book".into(), other_book_receipt.clone());
                this.finish_none(ctx)?;
                let hedge_packet = json!({
                    "scope": this.inner.borrow().scope.clone(),
                    "firstExchangeId": this.inner.borrow().first_exchange_id.clone(),
                    "owned": get(ctx,"owned"),
                    "pairCensus": pair_census,
                    "funding": funding,
                    "globalRisk": global_risk,
                    "account": account_source,
                    "rootReceipts": root_receipts,
                    "otherBookReceipt": other_book_receipt,
                    "graphReceipt": get(ctx,"graphReceipt"),
                    "collateralReceipt": get(ctx,"collateralReceipt"),
                    "now": iso((this.inner.borrow().deps.now)()),
                    "maximumPilotQuantity": 1,
                });
                ctx.as_object_mut()
                    .unwrap()
                    .insert("hedgePacket".into(), hedge_packet);
                Ok(())
            })?;
            let hp = {
                let inner = self.inner.borrow();
                get(inner.cache.as_ref().unwrap_or(&V::Null), "hedgePacket").clone()
            };
            self.inner.borrow_mut().hedge_packet = Some(hp);
            return self.get_evidence(&json!({"operation": "prepare_hedge"}));
        }
        let inner = self.inner.borrow();
        let base = OwnedNoFreshNoneBridgeDeps {
            api: inner.deps.api.clone(),
            receipt_of: inner.deps.receipt_of.clone(),
            get_owned: inner.deps.get_owned.clone(),
            get_pair_census: inner.deps.get_pair_census.clone(),
            get_funding: inner.deps.get_funding.clone(),
            get_global_risk: inner.deps.get_global_risk.clone(),
            get_account_lease: inner.deps.get_account_lease.clone(),
            can_read: inner.deps.can_read.clone(),
            has_read_headroom: inner.deps.has_read_headroom.clone(),
            get_active_intent: inner.deps.get_active_intent.clone(),
            get_order_binding: inner.deps.get_order_binding.clone(),
            now: inner.deps.now.clone(),
        };
        let scope = inner.scope.clone();
        let first_exchange_id = inner.first_exchange_id.clone();
        drop(inner);
        let hedge =
            create_owned_no_fresh_none_hedge_evidence_bridge(&scope, &first_exchange_id, base)?;
        let result = hedge.refresh();
        {
            let mut inner = self.inner.borrow_mut();
            inner.total_read_count +=
                get(&hedge.status(), "lastReadCount").as_i64().unwrap_or(0);
            if let Ok(hp) = &result {
                inner.hedge_packet = Some(hp.clone());
            }
            inner.hedge = Some(hedge);
        }
        result
    }
    /// refreshSell(leg)
    pub fn refresh_sell(&self, leg: &V) -> Result<V, String> {
        let legs = self.inner.borrow().legs.clone();
        let mut matched = false;
        for e in &legs {
            if same(e, leg)? {
                matched = true;
                break;
            }
        }
        need(matched, "OWN_SELL_SCOPE")?;
        let owned = (self.inner.borrow().deps.get_owned)(get(leg, "exchangeId"))?;
        need(
            sget(get(&owned, "proof"), "marketId") == sget(leg, "marketId")
                && sget(get(&owned, "proof"), "exchangeId") == sget(leg, "exchangeId")
                && get(get(&owned, "proof"), "remainingOwnedQuantity")
                    .as_i64()
                    .unwrap_or(0)
                    > 0,
            "OWN_CURRENT_SELL",
        )?;
        let mut ctx = json!({"kind": "sell", "leg": leg, "owned": owned});
        self.begin(&mut ctx, 7, |this, ctx| {
            let account = this.account(get(ctx, "leg"), false)?;
            ctx.as_object_mut()
                .unwrap()
                .insert("account".into(), account.clone());
            let mut binding = this
                .inner
                .borrow()
                .scope
                .as_object()
                .cloned()
                .unwrap_or_default();
            if let Some(m) = get(ctx, "leg").as_object() {
                for (k, v) in m {
                    binding.insert(k.clone(), v.clone());
                }
            }
            inspect_scoped_rest_no_account(&json!({
                "account": account,
                "binding": V::Object(binding),
                "readAt": iso((this.inner.borrow().deps.now)()),
            }))?;
            if let Some(rfa) = &this.inner.borrow().deps.refresh_funding_after_account {
                let rfa = rfa.clone();
                drop(this.inner.borrow());
                rfa()?;
            }
            let source_funding = this.fund()?;
            ctx.as_object_mut()
                .unwrap()
                .insert("sourceFunding".into(), source_funding);
            let source_risk = this.guard(None)?;
            need(
                get(&source_risk, "unresolvedIntentIds")
                    .as_array()
                    .map(|x| x.is_empty())
                    .unwrap_or(false),
                "PENDING_BEFORE_BOOK",
            )?;
            ctx.as_object_mut()
                .unwrap()
                .insert("sourceRisk".into(), source_risk);
            let book = this.read(&format!(
                "/markets/{}/orderbook?tournamentId={}&depth=20",
                sget(get(ctx, "leg"), "marketId").unwrap_or(""),
                sget(&this.inner.borrow().scope, "tournamentId").unwrap_or("")
            ))?;
            ctx.as_object_mut().unwrap().insert("book".into(), book);
            this.finish_none(ctx)
        })?;
        self.sell_evidence("prepare_sell", None)
    }
    /// refreshPreparedNone()
    pub fn refresh_prepared_none(&self) -> Result<V, String> {
        need(!self.inner.borrow().busy, "BUSY")?;
        let a = self.active()?;
        need(
            sget(&a, "phase") == Some("prepared")
                && get(&a, "orderId").is_null()
                && {
                    let afc = get(&a, "actualFetchCount");
                    let fc = get(&a, "fetchCount");
                    if !afc.is_null() {
                        afc.as_i64() == Some(0)
                    } else {
                        fc.as_i64() == Some(0)
                    }
                }
                && sget(&a, "bodyHash") == Some(payload_hash(get(&a, "body"))?.as_str())
                && sget(get(&a, "body"), "side") == Some("no")
                && inst(get(&a, "body"), "expirationDate")?
                    > (self.inner.borrow().deps.now)() as i64,
            "EXACT_UNFETCHED_PREPARED",
        )?;
        let is_buy = sget(get(&a, "body"), "action") == Some("buy");
        let mut ctx: V;
        if is_buy {
            let hedge_packet = self.inner.borrow().hedge_packet.clone().unwrap_or(V::Null);
            need(
                !hedge_packet.is_null()
                    && get(get(&a, "body"), "quantity") == &json!(1)
                    && sget(&a, "firstExchangeId")
                        == sget(&hedge_packet, "firstExchangeId"),
                "ORIGINAL_HEDGE_BINDING",
            )?;
            let legs = self.inner.borrow().legs.clone();
            let leg = legs
                .iter()
                .find(|e| {
                    sget(e, "exchangeId") == sget(get(&a, "body"), "exchangeId")
                })
                .cloned()
                .unwrap_or(V::Null);
            let graph_leg = legs
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
            ctx = json!({
                "kind": "hedgeFinal",
                "leg": leg,
                "graphLeg": graph_leg,
                "owned": get(&hedge_packet,"owned"),
                "ownedExchangeId": get(&a,"firstExchangeId"),
                "otherBookReceipt": get(&hedge_packet,"otherBookReceipt"),
                "book": get(&hedge_packet,"otherBookReceipt"),
                "hedgePacket": hedge_packet,
            });
        } else {
            let cache = self.inner.borrow().cache.clone().unwrap_or(V::Null);
            need(
                sget(get(&a, "body"), "action") == Some("sell")
                    && sget(&cache, "kind") == Some("sell")
                    && sget(get(&cache, "leg"), "exchangeId")
                        == sget(get(&a, "body"), "exchangeId")
                    && get(get(&a, "body"), "quantity").as_i64().unwrap_or(0)
                        <= get(get(get(&cache, "owned"), "proof"), "remainingOwnedQuantity")
                            .as_i64()
                            .unwrap_or(0),
                "ORIGINAL_SELL_BINDING",
            )?;
            ctx = cache;
        }
        ctx.as_object_mut()
            .unwrap()
            .insert("preparedBinding".into(), a.clone());
        let source_pin = payload_hash(&if is_buy {
            get(&ctx, "hedgePacket").clone()
        } else {
            json!({
                "account": get(&ctx,"account"),
                "book": get(&ctx,"book"),
                "owned": get(&ctx,"owned"),
                "sourceRisk": get(&ctx,"sourceRisk"),
                "sourceFunding": get(&ctx,"sourceFunding"),
            })
        })?;
        self.begin(&mut ctx, 2, |this, ctx| this.finish_none(ctx))?;
        let cache = self.inner.borrow().cache.clone().unwrap_or(V::Null);
        let after_pin = payload_hash(&if is_buy {
            get(&cache, "hedgePacket").clone()
        } else {
            json!({
                "account": get(&cache,"account"),
                "book": get(&cache,"book"),
                "owned": get(&cache,"owned"),
                "sourceRisk": get(&cache,"sourceRisk"),
                "sourceFunding": get(&cache,"sourceFunding"),
            })
        })?;
        need(source_pin == after_pin, "ORIGINAL_PACKET_CHANGED")?;
        self.get_evidence(&json!({
            "operation": if is_buy { "place_hedge" } else { "place_sell" }
        }))
    }
    /// refreshFinalNone() -> refreshPreparedNone()
    pub fn refresh_final_none(&self) -> Result<V, String> {
        self.refresh_prepared_none()
    }
    /// refreshReconciliation(orderId)
    pub fn refresh_reconciliation(&self, order_id: &str) -> Result<V, String> {
        let binding = (self.inner.borrow().deps.get_order_binding)(&json!(order_id))?;
        let legs = self.inner.borrow().legs.clone();
        let leg = legs
            .iter()
            .find(|e| {
                sget(e, "exchangeId") == sget(get(&binding, "body"), "exchangeId")
            })
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
                && is_safe_int(get(get(&binding, "body"), "quantity"))
                && get(get(&binding, "body"), "quantity").as_i64().unwrap_or(0) > 0
                && get(get(&binding, "body"), "quantity").as_i64().unwrap_or(0) <= 100,
            "KNOWN_OWN_BINDING",
        )?;
        let graph_leg = if sget(get(&binding, "body"), "action") == Some("buy") {
            let fe = if sget(&binding, "firstExchangeId").is_some() {
                sget(&binding, "firstExchangeId").unwrap_or("").to_string()
            } else {
                self.inner.borrow().first_exchange_id.clone()
            };
            legs.iter()
                .find(|e| sget(e, "exchangeId") == Some(fe.as_str()))
                .cloned()
                .unwrap_or(V::Null)
        } else {
            leg.clone()
        };
        need(!graph_leg.is_null(), "ACTUAL_ORDER_GRAPH_SCOPE")?;
        let mut ctx = json!({
            "kind": "reconcile",
            "leg": leg,
            "graphLeg": graph_leg,
            "binding": binding,
        });
        self.begin(&mut ctx, 9, |this, ctx| {
            let order = this.read(&format!("/orders/{order_id}"))?;
            ctx.as_object_mut()
                .unwrap()
                .insert("order".into(), order.clone());
            let parsed = this.raw(&order, sget(&order, "endpoint").unwrap_or(""), 20_000)?;
            let od = &parsed.0;
            let binding_body = get(get(ctx, "binding"), "body").clone();
            let scope_tid = sget(&this.inner.borrow().scope, "tournamentId")
                .unwrap_or("")
                .to_string();
            let leg_ex = sget(get(ctx, "leg"), "exchangeId")
                .unwrap_or("")
                .to_string();
            let order_id_num = order_id.parse::<i64>().unwrap_or(0);
            let key = js_stringify(&json!(["id"]));
            need(
                get(od, "id").as_i64() == Some(order_id_num)
                    && parsed.1.get(&key).map(|s| s.as_str()) == Some(order_id)
                    && sget(od, "exchangeId") == Some(leg_ex.as_str())
                    && sget(od, "tournamentId") == Some(scope_tid.as_str())
                    && sget(od, "side") == Some("no")
                    && sget(od, "action") == sget(&binding_body, "action")
                    && is_safe_int(get(od, "quantity"))
                    && get(od, "quantity").as_i64().unwrap_or(0) >= 1
                    && get(od, "quantity").as_i64().unwrap_or(0)
                        <= get(&binding_body, "quantity").as_i64().unwrap_or(0)
                    && is_safe_int(get(od, "quantityFilled"))
                    && get(od, "quantityFilled").as_i64().unwrap_or(-1) >= 0
                    && get(od, "open").is_boolean()
                    && (get(od, "open") == &V::Bool(true)
                        || get(od, "quantityFilled").as_i64().unwrap_or(0)
                            <= get(od, "quantity").as_i64().unwrap_or(0)),
                "EXACT_OWN_ORDER_RESPONSE",
            )?;
            let fill_pages = this.page(&format!("/orders/{order_id}/fills?limit=200"))?;
            ctx.as_object_mut()
                .unwrap()
                .insert("fillPages".into(), V::Array(fill_pages.clone()));
            let fills = this
                .raw(&fill_pages[0], sget(&fill_pages[0], "endpoint").unwrap_or(""), 20_000)?
                .0;
            need(
                get(&fills, "orderId").as_i64() == Some(order_id_num)
                    && sget(&fills, "exchangeId") == Some(leg_ex.as_str())
                    && sget(&fills, "tournamentId") == Some(scope_tid.as_str())
                    && get(&fills, "data").as_array().is_some_and(|xs| {
                        xs.iter().all(|v| {
                            sget(v, "side") == Some("no")
                                && is_safe_int(get(v, "quantity"))
                                && get(v, "quantity").as_i64().unwrap_or(0) < 0
                        })
                    }),
                "EXACT_OWN_FILL_RESPONSE",
            )?;
            let after = this.account(get(ctx, "leg"), true)?;
            ctx.as_object_mut()
                .unwrap()
                .insert("after".into(), after);
            this.finish_none(ctx)
        })?;
        self.get_evidence(&json!({"operation": "reconcile", "orderId": order_id}))
    }
    pub fn status(&self) -> V {
        let inner = self.inner.borrow();
        json!({
            "mode": if inner.v2 {"combined_owned_no_none_evidence_bridge_v2"} else {"combined_owned_no_none_evidence_bridge_v3"},
            "busy": inner.busy,
            "closed": inner.closed,
            "cacheKind": inner.cache.as_ref().map(|c| get(c,"kind").clone()).unwrap_or(V::Null),
            "lastReadCount": inner.reads,
            "maximumReads": inner.maximum_reads,
            "totalReadCount": inner.total_read_count,
            "readTimings": inner.read_timings,
            "hedge": inner.hedge.as_ref().map(|h| h.status()).unwrap_or(V::Null),
            "executionAuthority": false,
            "additionalApiClients": 0,
        })
    }
    pub fn clear(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.cache = None;
        if let Some(h) = &inner.hedge {
            h.clear();
        }
    }
    pub fn close(&self) -> Result<(), String> {
        let mut inner = self.inner.borrow_mut();
        need(!inner.busy, "BUSY")?;
        inner.closed = true;
        inner.cache = None;
        if let Some(h) = &inner.hedge {
            let _ = h.close();
        }
        inner.watermarks.clear();
        Ok(())
    }
}

/// createOwnedNoFreshNoneHedgeEvidenceBridgeV2/V3(options)
/// `v3`: Revision V3 splits mechanical read identity (canCheckReadIdentity)
/// from full admission checks (canRead inside guard). v2 keeps them identical.
pub fn create_owned_no_fresh_none_hedge_evidence_bridge_v2(
    scope: &V,
    first_exchange_id: &str,
    pair_legs: Option<&V>,
    deps: CombinedNoneEvidenceDeps,
) -> Result<OwnedNoFreshNoneHedgeEvidenceBridgeV2, String> {
    create_inner(scope, first_exchange_id, pair_legs, deps, true)
}
pub fn create_owned_no_fresh_none_hedge_evidence_bridge_v3(
    scope: &V,
    first_exchange_id: &str,
    pair_legs: Option<&V>,
    deps: CombinedNoneEvidenceDeps,
) -> Result<OwnedNoFreshNoneHedgeEvidenceBridgeV2, String> {
    create_inner(scope, first_exchange_id, pair_legs, deps, false)
}

fn create_inner(
    scope: &V,
    first_exchange_id: &str,
    pair_legs: Option<&V>,
    deps: CombinedNoneEvidenceDeps,
    v2: bool,
) -> Result<OwnedNoFreshNoneHedgeEvidenceBridgeV2, String> {
    let first = (deps.get_owned)(&json!(first_exchange_id))?;
    let legs = pair_legs
        .cloned()
        .or_else(|| get(get(&first, "package"), "legs").clone().into())
        .unwrap_or(V::Null);
    let legs_arr = legs.as_array().cloned().unwrap_or_default();
    let ex_ids: HashSet<String> = legs_arr
        .iter()
        .map(|e| sget(e, "exchangeId").unwrap_or("").to_string())
        .collect();
    let m_ids: HashSet<String> = legs_arr
        .iter()
        .map(|e| sget(e, "marketId").unwrap_or("").to_string())
        .collect();
    need(
        sget(scope, "ownerId") == Some("Codex")
            && legs.is_array()
            && legs_arr.len() == 2
            && ex_ids.len() == 2
            && m_ids.len() == 2
            && legs_arr.iter().all(|e| {
                sget(e, "marketId").is_some_and(digit_id)
                    && sget(e, "exchangeId").is_some_and(digit_id)
            }),
        "APPROVED_PAIR",
    )?;
    Ok(OwnedNoFreshNoneHedgeEvidenceBridgeV2 {
        inner: Rc::new(RefCell::new(BridgeV2Inner {
            scope: scope.clone(),
            first_exchange_id: first_exchange_id.to_string(),
            legs: legs_arr,
            hedge: None,
            hedge_packet: None,
            cache: None,
            job: None,
            read_timings: Vec::new(),
            total_read_count: 0,
            busy: false,
            closed: false,
            reads: 0,
            maximum_reads: 0,
            last_end: None,
            permits: HashMap::new(),
            watermarks: HashMap::new(),
            invocations: HashSet::new(),
            deps,
            v2,
        })),
    })
}
