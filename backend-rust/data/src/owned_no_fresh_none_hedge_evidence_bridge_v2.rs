// execution/owned-no-fresh-none-hedge-evidence-bridge-v2.mjs — read-only
// shared-client wrapper. Current custody is supplied by the verified
// coordinator journal, not a stale original inventory snapshot or account
// marks.
//
// Port notes: same conventions as owned_no_fresh_none_hedge_evidence_bridge_v1
// (guard-pointer permit map, Rc<RefCell> state, error string prefixes). The
// in-flight operation context (`ctx`) is shared mutable state — `guard()` and
// `none()` observe fields appended mid-flight — so it is carried as
// Rc<RefCell<V>> like the JS object reference.
use baal_execution::core::payload_hash;
use baal_execution::receipts::parse_number_lexemes;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::{instant, price};
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::owned_no_fresh_none_hedge_evidence_bridge_v1::{
    create_owned_no_fresh_none_hedge_evidence_bridge, Guard, OwnedNoneBridgeApi,
    OwnedNoneBridgeDeps, OwnedNoFreshNoneHedgeEvidenceBridgeV1,
};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("COMBINED_NONE_EVIDENCE_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.as_object().and_then(|m| m.get(k)).unwrap_or(&V::Null)
}
fn copy(v: &V) -> V {
    v.clone()
}
fn sha(s: &str) -> String {
    sha256(s.as_bytes())
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(payload_hash(a)? == payload_hash(b)?)
}
fn iso(n: f64) -> String {
    ms_to_iso(n)
}
fn num_id(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
}
fn num_eq(v: &V, n: f64) -> bool {
    v.is_number() && v.as_f64() == Some(n)
}
fn truthy(v: &V) -> bool {
    js_truthy(Some(v))
}
fn instant_s(v: &V) -> Result<i64, String> {
    instant(v.as_str().ok_or_else(|| "Timestamp must be canonical UTC ISO milliseconds".to_string())?)
}
fn type_error(field: &str) -> String {
    format!("Cannot read properties of undefined (reading '{field}')")
}
fn js_str_num(v: &V) -> String {
    // String(x) on a JSON number/string.
    v.as_str()
        .map(String::from)
        .or_else(|| v.as_i64().map(|x| x.to_string()))
        .or_else(|| v.as_u64().map(|x| x.to_string()))
        .or_else(|| v.as_f64().map(|x| js_stringify(&json!(x))))
        .unwrap_or_default()
}

pub struct OwnedNoneBridgeV2Deps {
    pub get_owned: Rc<dyn Fn(&str) -> Result<V, String>>,
    pub get_pair_census: Rc<dyn Fn(&str) -> Result<V, String>>,
    pub get_funding: Rc<dyn Fn() -> Result<V, String>>,
    pub get_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub can_read: Rc<dyn Fn() -> bool>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> Result<bool, String>>,
    /// default ()=>null
    pub get_active_intent: Rc<dyn Fn() -> Result<V, String>>,
    /// default ()=>null
    pub get_order_binding: Rc<dyn Fn(&str) -> Result<V, String>>,
    /// refreshFundingAfterAccount=null
    pub refresh_funding_after_account: Option<Rc<dyn Fn() -> Result<(), String>>>,
    pub now: Rc<dyn Fn() -> f64>,
}

pub struct OwnedNoFreshNoneHedgeEvidenceBridgeV2 {
    api: Rc<RefCell<OwnedNoneBridgeApi>>,
    receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    deps: OwnedNoneBridgeV2Deps,
    scope: V,
    first_exchange_id: Option<String>,
    legs: Vec<V>,
    hedge: RefCell<Option<Rc<OwnedNoFreshNoneHedgeEvidenceBridgeV1>>>,
    hedge_packet: RefCell<V>,
    cache: RefCell<V>,
    job: RefCell<Option<Rc<RefCell<V>>>>,
    total_read_count: Cell<i64>,
    busy: Cell<bool>,
    closed: Cell<bool>,
    reads: Cell<i64>,
    maximum_reads: Cell<i64>,
    last_end: Cell<Option<i64>>,
    permits: RefCell<HashMap<usize, V>>,
    watermarks: RefCell<HashMap<String, (i64, i64)>>,
    invocations: RefCell<HashSet<String>>,
}

impl OwnedNoFreshNoneHedgeEvidenceBridgeV2 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        api: Rc<RefCell<OwnedNoneBridgeApi>>,
        receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
        scope: &V,
        first_exchange_id: Option<&str>,
        pair_legs: Option<&V>,
        deps: OwnedNoneBridgeV2Deps,
    ) -> Result<Self, String> {
        // `options.firstExchangeId` may be absent; getOwned receives it.
        let first = (deps.get_owned)(first_exchange_id.unwrap_or(""))?;
        let legs_v = match pair_legs {
            Some(l) if !l.is_null() => l.clone(),
            _ => get(get(&first, "package"), "legs").clone(),
        };
        let legs = legs_v.as_array().cloned().unwrap_or_default();
        let exchange_ids: HashSet<String> = legs
            .iter()
            .map(|e| get(e, "exchangeId").as_str().unwrap_or("").to_string())
            .collect();
        let market_ids: HashSet<String> = legs
            .iter()
            .map(|e| get(e, "marketId").as_str().unwrap_or("").to_string())
            .collect();
        need(
            get(scope, "ownerId").as_str() == Some("Codex")
                && legs_v.is_array()
                && legs.len() == 2
                && exchange_ids.len() == 2
                && market_ids.len() == 2
                && legs.iter().all(|e| {
                    num_id(get(e, "marketId").as_str().unwrap_or(""))
                        && num_id(get(e, "exchangeId").as_str().unwrap_or(""))
                }),
            "APPROVED_PAIR",
        )?;
        Ok(Self {
            api,
            receipt_of,
            deps,
            scope: scope.clone(),
            first_exchange_id: first_exchange_id.map(String::from),
            legs,
            hedge: RefCell::new(None),
            hedge_packet: RefCell::new(V::Null),
            cache: RefCell::new(V::Null),
            job: RefCell::new(None),
            total_read_count: Cell::new(0),
            busy: Cell::new(false),
            closed: Cell::new(false),
            reads: Cell::new(0),
            maximum_reads: Cell::new(0),
            last_end: Cell::new(None),
            permits: RefCell::new(HashMap::new()),
            watermarks: RefCell::new(HashMap::new()),
            invocations: RefCell::new(HashSet::new()),
        })
    }

    fn active(&self) -> Result<V, String> {
        (self.deps.get_active_intent)()
    }
    fn fund(&self) -> Result<V, String> {
        (self.deps.get_funding)()
    }
    fn now(&self) -> f64 {
        (self.deps.now)()
    }
    fn ctx(&self) -> V {
        self.job
            .borrow()
            .as_ref()
            .map(|rc| rc.borrow().clone())
            .unwrap_or_else(|| self.cache.borrow().clone())
    }

    fn raw(&self, r: &V, endpoint: &str, age: i64) -> Result<V, String> {
        need(
            truthy(r)
                && get(r, "method").as_str() == Some("GET")
                && get(r, "endpoint").as_str() == Some(endpoint)
                && num_eq(get(r, "status"), 200.0)
                && get(r, "requestBodyHash").is_null()
                && get(r, "bodyText").is_string()
                && get(r, "bodyText").as_str().unwrap_or("").as_bytes().len() <= 8 * 1024 * 1024
                && get(r, "sourceBodyHash").as_str()
                    == Some(sha(get(r, "bodyText").as_str().unwrap_or("")).as_str())
                && get(r, "fetchInvocationId").as_str().is_some_and(|s| !s.is_empty())
                && r.as_object().map(|m| {
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
                }) == Some(true),
            "RAW_SOURCE",
        )?;
        let start = instant_s(get(r, "requestStartedAt"))?;
        let end = instant_s(get(r, "receivedAt"))?;
        let now = self.now();
        need(
            start <= end && (end as f64) <= now && now - start as f64 <= age as f64,
            "SOURCE_AGE",
        )?;
        let (data, numbers) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
        Ok(json!({"data": data, "numbers": numbers}))
    }

    fn identity(&self) -> Result<(), String> {
        need(!self.closed.get() && (self.deps.can_read)(), "STOPPED")?;
        let l = (self.deps.get_account_lease)()?;
        need(
            get(&l, "ownerId") == get(&self.scope, "ownerId")
                && get(&l, "tournamentId") == get(&self.scope, "tournamentId")
                && hex(get(&l, "leaseId")),
            "ACCOUNT_LEASE",
        )?;
        let (mr, cd) = {
            let a = self.api.borrow();
            (a.manual_review, a.cooldown)
        };
        let cd = if cd.is_nan() { 0.0 } else { cd };
        need(!mr && self.now() >= cd, "BACKOFF")?;
        Ok(())
    }

    // guard(fenceRisk=null)
    fn guard(&self, fence_risk: Option<&V>) -> Result<V, String> {
        self.identity()?;
        let g = match fence_risk {
            None => (self.deps.get_global_risk)()?,
            Some(f) if f.is_null() => (self.deps.get_global_risk)()?,
            Some(f) => copy(f),
        };
        let ctx = self.ctx();
        let a = self.active()?;
        let now = self.now();
        need(
            get(&g, "ownerId") == get(&self.scope, "ownerId")
                && get(&g, "runId") == get(&self.scope, "runId")
                && get(&g, "tournamentId") == get(&self.scope, "tournamentId")
                && get(&g, "complete") == &V::Bool(true)
                && get(&g, "eventScopeComplete") == &V::Bool(true)
                && get(&g, "soleCollector") == &V::Bool(true)
                && get(&g, "stopped") == &V::Bool(false)
                && hex(get(&g, "sourceHash"))
                && get(&g, "unresolvedIntentIds").is_array()
                && (instant_s(get(&g, "at"))? as f64) <= now
                && now - instant_s(get(&g, "at"))? as f64 <= 20000.0,
            "GLOBAL_SCOPE",
        )?;
        let pb = get(&ctx, "preparedBinding");
        if !pb.is_null()
            && get(&a, "intentId") == get(&pb, "intentId")
            && get(&a, "orderId").is_null()
            && num_eq(
                {
                    let afc = a.as_object().and_then(|m| m.get("actualFetchCount"));
                    let fc = a.as_object().and_then(|m| m.get("fetchCount"));
                    afc.filter(|v| !v.is_null()).or(fc).unwrap_or(&V::Null)
                },
                0.0,
            )
        {
            need(
                (instant_s(get(get(&a, "body"), "expirationDate"))? as f64) > now,
                "PREPARED_EXPIRED",
            )?;
        }
        // pendingOwn — every term after the first three short-circuits lazily;
        // TypeError-level violations surface as uncoded errors like JS.
        let pending_own = {
            let leg = get(&ctx, "leg");
            let owned = get(&ctx, "owned");
            let kind = get(&ctx, "kind").as_str().unwrap_or("");
            let body = get(&a, "body");
            let afc = a.as_object().and_then(|m| m.get("actualFetchCount"));
            let fc = a.as_object().and_then(|m| m.get("fetchCount"));
            let eff = afc.filter(|v| !v.is_null()).or(fc).unwrap_or(&V::Null);
            let base = !ctx.is_null()
                && truthy(&a)
                && truthy(get(&a, "intentId"))
                && get(&a, "bodyHash").as_str() == Some(payload_hash(body)?.as_str())
                && get(body, "exchangeId") == get(leg, "exchangeId")
                && get(body, "tournamentId") == get(&self.scope, "tournamentId")
                && get(body, "side").as_str() == Some("no")
                && (get(body, "action").as_str() == Some("sell")
                    || (kind == "hedgeFinal"
                        && get(body, "action").as_str() == Some("buy")
                        && num_eq(get(body, "quantity"), 1.0)))
                && ["prepared", "dispatching"].contains(&get(&a, "phase").as_str().unwrap_or(""))
                && get(&a, "orderId").is_null()
                && num_eq(eff, 0.0)
                && js_int(get(body, "quantity")).is_some_and(|q| q > 0);
            if !base {
                false
            } else {
                let bound = if kind == "hedgeFinal" {
                    1.0
                } else {
                    let bound_v = get(get(owned, "proof"), "remainingOwnedQuantity");
                    match bound_v.as_f64() {
                        Some(x) => x,
                        None => return Err(type_error("remainingOwnedQuantity")),
                    }
                };
                let exp = instant_s(get(body, "expirationDate"))?;
                js_int(get(body, "quantity")).is_some_and(|q| (q as f64) <= bound)
                    && (exp as f64) > now
                    && get(&g, "unresolvedIntentIds")
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|id| id == get(&a, "intentId"))
            }
        };
        let binding = get(&ctx, "binding");
        let known = if !binding.is_null() {
            (self.deps.get_order_binding)(binding.get("orderId").and_then(V::as_str).unwrap_or(""))?
        } else {
            V::Null
        };
        let cleanup = !binding.is_null()
            && truthy(&known)
            && get(&known, "intentId") == get(&binding, "intentId")
            && get(&known, "orderId") == get(&binding, "orderId")
            && get(&known, "bodyHash") == get(&binding, "bodyHash")
            && same(get(&known, "body"), get(&binding, "body"))?
            && get(&g, "unresolvedIntentIds")
                .as_array()
                .unwrap()
                .iter()
                .all(|id| id == get(&binding, "intentId"));
        need(
            (get(&g, "unknownAcceptance") == &V::Bool(false) || cleanup)
                && (get(&g, "unresolvedIntentIds").as_array().unwrap().is_empty()
                    || pending_own
                    || cleanup),
            "UNKNOWN_OR_FOREIGN_PENDING",
        )?;
        if !pb.is_null() {
            need(
                get(&a, "intentId") == get(&pb, "intentId")
                    && get(&a, "bodyHash") == get(&pb, "bodyHash")
                    && same(get(&a, "body"), get(&pb, "body"))?
                    && (self.job.borrow().is_none()
                        || get(&a, "phase").as_str() == Some("prepared")),
                "PREPARED_BODY_CHANGED",
            )?;
        }
        if !get(&ctx, "owned").is_null() {
            let ex = {
                let oe = get(&ctx, "ownedExchangeId");
                if oe.is_null() {
                    get(get(&ctx, "leg"), "exchangeId").as_str().unwrap_or("").to_string()
                } else {
                    oe.as_str().unwrap_or("").to_string()
                }
            };
            need(
                same(&(self.deps.get_owned)(&ex)?, get(&ctx, "owned"))?,
                "CURRENT_CUSTODY_CHANGED",
            )?;
        }
        Ok(g)
    }

    // none(ctx)
    fn none(&self, ctx: &V) -> Result<(), String> {
        let leg = {
            let gl = get(ctx, "graphLeg");
            if gl.is_null() {
                get(ctx, "leg").clone()
            } else {
                gl.clone()
            }
        };
        let graph_endpoint = format!(
            "/relationships/graph?marketId={}&depth=4&tournamentId={}",
            get(&leg, "marketId").as_str().unwrap_or(""),
            get(&self.scope, "tournamentId").as_str().unwrap_or("")
        );
        let graph = self.raw(get(ctx, "graphReceipt"), &graph_endpoint, 5000)?;
        let graph = get(&graph, "data");
        let c = self.raw(
            get(ctx, "collateralReceipt"),
            &format!(
                "/portfolio/collateral?tournamentId={}",
                get(&self.scope, "tournamentId").as_str().unwrap_or("")
            ),
            5000,
        )?;
        let c = get(&c, "data");
        let graph_nodes = get(graph, "nodes").as_array().cloned().unwrap_or_default();
        let node_ids: HashSet<String> = graph_nodes
            .iter()
            .map(|n| get(n, "exchangeId").as_str().unwrap_or("").to_string())
            .collect();
        need(
            graph.is_object()
                && {
                    let mut keys: Vec<&str> =
                        graph.as_object().unwrap().keys().map(|k| k.as_str()).collect();
                    keys.sort();
                    keys.join("|") == "nodes|relationships"
                }
                && get(graph, "relationships").as_array().is_some_and(|a| a.is_empty())
                && get(graph, "nodes").as_array().is_some_and(|a| a.len() <= 2)
                && graph_nodes.iter().all(|n| {
                    self.legs.iter().any(|e| {
                        get(e, "marketId") == get(n, "marketId")
                            && get(e, "exchangeId") == get(n, "exchangeId")
                    })
                })
                && node_ids.len() == graph_nodes.len(),
            "NON_NONE_GRAPH",
        )?;
        need(
            c.is_object()
                && c.as_object()
                    .map(|m| m.keys().cloned().collect::<Vec<_>>().join("|") == "data")
                    .unwrap_or(false)
                && get(c, "data").as_array().is_some_and(|a| a.is_empty()),
            "NON_NONE_COLLATERAL",
        )?;
        let preceding = {
            let book = get(ctx, "book");
            if !book.is_null() {
                get(&book, "receivedAt").clone()
            } else {
                let by_ex = get(get(ctx, "after"), "scopedOrdersPagesByExchange");
                let arr = by_ex
                    .get(get(&self.legs[1], "exchangeId").as_str().unwrap_or(""))
                    .and_then(|x| x.as_array())
                    .cloned()
                    .unwrap_or_default();
                get(arr.last().unwrap_or(&V::Null), "receivedAt").clone()
            }
        };
        need(
            instant_s(get(get(ctx, "graphReceipt"), "requestStartedAt"))? >= instant_s(&preceding)?
                && instant_s(get(get(ctx, "collateralReceipt"), "requestStartedAt"))?
                    >= instant_s(get(get(ctx, "graphReceipt"), "receivedAt"))?,
            "NONE_AFTER_OWN_SOURCE",
        )?;
        Ok(())
    }

    fn read(&self, endpoint: &str) -> Result<V, String> {
        self.identity()?;
        need(
            self.reads.get() < self.maximum_reads.get()
                && (self.deps.has_read_headroom)(self.maximum_reads.get() - self.reads.get())?,
            "READ_BOUND",
        )?;
        let admitted = Rc::new(Cell::new(false));
        let network = Rc::new(Cell::new(false));
        let this = self;
        let (admitted_c, network_c) = (admitted.clone(), network.clone());
        let before_dispatch = move |_p: &V, _s: &V| -> Result<bool, String> {
            let admitted = &admitted_c;
            this.guard(None)?;
            need(
                !admitted.get()
                    && (this.deps.has_read_headroom)(
                        this.maximum_reads.get() - this.reads.get()
                    )?,
                "ADMISSION",
            )?;
            admitted.set(true);
            Ok(true)
        };
        let (admitted_n, network_n) = (admitted.clone(), network.clone());
        let before_network = move |_p: &V, _s: &V| -> Result<bool, String> {
            let admitted = &admitted_n;
            let network = &network_n;
            this.guard(None)?;
            need(
                admitted.get()
                    && !network.get()
                    && (this.deps.has_read_headroom)(
                        this.maximum_reads.get() - this.reads.get() - 1
                    )?,
                "NETWORK",
            )?;
            network.set(true);
            Ok(true)
        };
        let bd_ptr = &before_dispatch as &Guard as *const Guard as *const () as usize;
        let bn_ptr = &before_network as &Guard as *const Guard as *const () as usize;
        self.permits.borrow_mut().insert(
            bd_ptr,
            json!({"endpoint": endpoint, "beforeNetworkPtr": bn_ptr as u64}),
        );
        let began = self.now();
        let result: Result<V, String> = (|| {
            let spec = json!({"method": "GET", "endpoint": endpoint});
            let response = (self.api.borrow().dispatch)(
                &spec,
                Some(&before_dispatch as &Guard),
                Some(&before_network as &Guard),
            )?;
            self.identity()?;
            need(
                admitted.get()
                    && network.get()
                    && get(&response, "endpoint").as_str() == Some(endpoint)
                    && num_eq(get(&response, "status"), 200.0),
                "RESPONSE",
            )?;
            let r = (self.receipt_of)(&response, &json!({"method": "GET"}))?;
            let p = self.raw(&r, endpoint, 20000)?;
            need(
                (instant_s(get(&r, "requestStartedAt"))? as f64) >= began
                    && instant_s(get(&r, "requestStartedAt"))?
                        >= instant_s(get(&response, "requestStartedAt"))?
                    && instant_s(get(&r, "receivedAt"))?
                        <= instant_s(get(&response, "receivedAt"))?
                    && same(get(&p, "data"), get(&response, "data"))?
                    && (self.last_end.get().is_none()
                        || instant_s(get(&r, "requestStartedAt"))? >= self.last_end.get().unwrap())
                    && !self
                        .invocations
                        .borrow()
                        .contains(get(&r, "fetchInvocationId").as_str().unwrap_or("")),
                "RAW_CAUSALITY",
            )?;
            let old = self.watermarks.borrow().get(endpoint).copied();
            let r_start = instant_s(get(&r, "requestStartedAt"))?;
            let r_end = instant_s(get(&r, "receivedAt"))?;
            need(
                old.map(|o| r_start >= o.0 && r_end >= o.1).unwrap_or(true),
                "RAW_ROLLBACK",
            )?;
            self.watermarks
                .borrow_mut()
                .insert(endpoint.to_string(), (r_start, r_end));
            self.invocations
                .borrow_mut()
                .insert(get(&r, "fetchInvocationId").as_str().unwrap_or("").to_string());
            self.last_end.set(Some(r_end));
            Ok(copy(&r))
        })();
        self.reads.set(self.reads.get() + 1);
        self.total_read_count.set(self.total_read_count.get() + 1);
        self.permits.borrow_mut().remove(&bd_ptr);
        result
    }

    fn page(&self, endpoint: &str) -> Result<Vec<V>, String> {
        let r = self.read(endpoint)?;
        let parsed = self.raw(&r, endpoint, 20000)?;
        let d = get(&parsed, "data");
        let coverage_seq = get(get(d, "coverage"), "projectedThroughSequence");
        need(
            get(d, "data").as_array().is_some_and(|a| a.len() <= 200)
                && num_eq(get(get(d, "pagination"), "limit"), 200.0)
                && get(get(d, "pagination"), "hasMore") == &V::Bool(false)
                && get(get(d, "pagination"), "nextCursor").is_null()
                && get(get(d, "coverage"), "complete") == &V::Bool(true)
                && (coverage_seq.is_null() || js_int(coverage_seq).is_some_and(|x| x >= 0)),
            "COMPLETE_SINGLE_PAGE_REQUIRED",
        )?;
        let mut ids: HashSet<i64> = HashSet::new();
        for (i, row) in get(d, "data").as_array().unwrap().iter().enumerate() {
            let key = js_stringify(&json!(["data", i, "id"]));
            let n = get(&parsed, "numbers").get(&key).cloned().unwrap_or(V::Null);
            need(
                n.as_str().is_some_and(num_id)
                    && js_int(get(row, "id")).is_some()
                    && !ids.contains(&js_int(get(row, "id")).unwrap_or(0)),
                "EXACT_ID",
            )?;
            ids.insert(js_int(get(row, "id")).unwrap());
        }
        Ok(vec![r])
    }

    // account(target, both)
    fn account(&self, target: &V, both: bool) -> Result<V, String> {
        let tid = get(&self.scope, "tournamentId").as_str().unwrap_or("").to_string();
        let slug = get(&self.scope, "tournamentSlug").as_str().unwrap_or("").to_string();
        let cash = self.read(&format!("/tournaments/{slug}"))?;
        let positions = self.read(&format!("/tournaments/{slug}/portfolio/positions"))?;
        let open_orders_pages =
            self.page(&format!("/orders?status=open&limit=200&tournamentId={tid}"))?;
        let mut scoped = Map::new();
        let iter: Vec<V> = if both {
            self.legs.clone()
        } else {
            vec![target.clone()]
        };
        for e in &iter {
            let ex = get(e, "exchangeId").as_str().unwrap_or("").to_string();
            let pages = self.page(&format!(
                "/orders?status=all&exchangeId={ex}&limit=200&tournamentId={tid}"
            ))?;
            scoped.insert(ex, json!(pages));
        }
        let target_ex = get(target, "exchangeId").as_str().unwrap_or("");
        Ok(json!({
            "cash": cash,
            "positions": positions,
            "openOrdersPages": open_orders_pages,
            "scopedOrdersPages": scoped.get(target_ex).cloned().unwrap_or(V::Null),
            "scopedOrdersPagesByExchange": V::Object(scoped),
        }))
    }

    // finishNone(ctx)
    fn finish_none(&self, ctx: &Rc<RefCell<V>>) -> Result<(), String> {
        let leg = {
            let c = ctx.borrow();
            let gl = get(&c, "graphLeg");
            if gl.is_null() {
                get(&c, "leg").clone()
            } else {
                gl.clone()
            }
        };
        let tid = get(&self.scope, "tournamentId").as_str().unwrap_or("").to_string();
        let graph = self.read(&format!(
            "/relationships/graph?marketId={}&depth=4&tournamentId={tid}",
            get(&leg, "marketId").as_str().unwrap_or("")
        ))?;
        ctx.borrow_mut()["graphReceipt"] = graph;
        let collateral = self.read(&format!("/portfolio/collateral?tournamentId={tid}"))?;
        ctx.borrow_mut()["collateralReceipt"] = collateral;
        self.none(&ctx.borrow().clone())
    }

    // sellEvidence(operation, fenceRisk=null)
    fn sell_evidence(&self, operation: &str, fence_risk: Option<&V>) -> Result<V, String> {
        let g = self.guard(fence_risk)?;
        let cache = self.cache.borrow().clone();
        need(
            get(&cache, "kind").as_str() == Some("sell") && !self.busy.get(),
            "SELL_CACHE",
        )?;
        self.none(&cache)?;
        if operation == "prepare_sell" {
            need(self.active()?.is_null(), "ACTIVE_BEFORE_PREPARE")?;
        }
        let f = self.fund()?;
        need(
            get(get(&f, "record"), "reservationHash") == get(&g, "controllerReservationHash")
                && get(get(&f, "receipt"), "globalLedgerHash") == get(&g, "globalLedgerHash")
                && get(get(&f, "receipt"), "reservationHash")
                    == get(get(&f, "record"), "reservationHash"),
            "CURRENT_INSTALLED_BUDGET",
        )?;
        let economic = |r: &V| -> V {
            let mut m = r.as_object().cloned().unwrap_or_default();
            m.remove("reservationHash");
            m.remove("unresolvedIntentIds");
            V::Object(m)
        };
        need(
            same(
                &economic(get(&f, "record")),
                &economic(get(get(&cache, "sourceFunding"), "record")),
            )?,
            "CURRENT_FULL_ECONOMICS_CHANGED",
        )?;
        need(
            get(get(&cache, "sourceRisk"), "unresolvedIntentIds")
                .as_array()
                .is_some_and(|a| a.is_empty())
                && (get(get(&f, "record"), "unresolvedIntentIds")
                    .as_array()
                    .map(|a| a.is_empty())
                    .unwrap_or(true)
                    || {
                        let a = self.active()?;
                        get(get(&f, "record"), "unresolvedIntentIds")
                            .as_array()
                            .map(|ids| ids.len() == 1 && &ids[0] == get(&a, "intentId"))
                            .unwrap_or(false)
                    }),
            "EXACT_OWN_SELL_PENDING",
        )?;
        let signal_risk = get(&cache, "sourceRisk");
        // {scope:{...scope,exchanges:legs}, owned, account, bookReceipt,
        //  globalRisk: signalRisk, now: iso(now())}
        let mut scope_m = self.scope.as_object().cloned().unwrap_or_default();
        scope_m.insert("exchanges".into(), json!(self.legs));
        let plan_input = json!({
            "scope": V::Object(scope_m),
            "owned": get(&cache, "owned"),
            "account": get(&cache, "account"),
            "bookReceipt": get(&cache, "book"),
            "globalRisk": signal_risk,
            "now": iso(self.now()),
        });
        let p = crate::baal_scaled_profitable_exit_signal_v2::plan_baal_scaled_profitable_exit_v2(
            &plan_input,
        );
        need(
            get(&p, "proposalReady") == &V::Bool(true),
            &format!("CURRENT_FIFO_OR_PROFIT_{}", get(&p, "reason").as_str().unwrap_or("")),
        )?;
        if truthy(&self.active()?) {
            let a = self.active()?;
            need(
                get(get(&a, "body"), "quantity").as_f64().unwrap_or(f64::INFINITY)
                    <= get(&p, "quantity").as_f64().unwrap_or(f64::NEG_INFINITY)
                    && price(&js_str_num(get(get(&a, "body"), "price")))?
                        <= price(get(&p, "limit").as_str().unwrap_or(""))?,
                "FIXED_PROFITABLE_INTENT_DEPTH",
            )?;
        }
        Ok(json!({
            "account": copy(get(&cache, "account")),
            "book": copy(get(&cache, "book")),
            "globalRisk": copy(signal_risk),
            "currentGlobalRisk": g,
            "graphReceipt": copy(get(&cache, "graphReceipt")),
            "collateralReceipt": copy(get(&cache, "collateralReceipt")),
            "funding": f,
            "owned": copy(get(&cache, "owned")),
        }))
    }

    /// getEvidence(c={})
    pub fn get_evidence(&self, c: &V) -> Result<V, String> {
        let op_v = get(c, "operation");
        let op = if op_v.is_null() { "prepare_hedge".to_string() } else { op_v.as_str().unwrap_or("").to_string() };
        if ["prepare_hedge", "place_hedge", "hedge"].contains(&op.as_str()) {
            let cache = self.cache.borrow().clone();
            need(
                self.hedge.borrow().is_some() || get(&cache, "kind").as_str() == Some("hedgeFinal"),
                "HEDGE_CACHE",
            )?;
            if get(&cache, "kind").as_str() == Some("hedgeFinal") {
                let fr = get(c, "fenceRisk");
                let g = self.guard(if fr.is_null() { None } else { Some(fr) })?;
                self.none(&cache)?;
                let f = self.fund()?;
                let economic = |r: &V| -> V {
                    let mut m = r.as_object().cloned().unwrap_or_default();
                    m.remove("reservationHash");
                    m.remove("unresolvedIntentIds");
                    V::Object(m)
                };
                let hp_funding_record = {
                    let hp_funding = get(get(&cache, "hedgePacket"), "funding").clone();
                    get(&hp_funding, "record").clone()
                };
                need(
                    same(&economic(get(&f, "record")), &economic(&hp_funding_record))?
                        && get(get(&f, "record"), "reservationHash")
                            == get(&g, "controllerReservationHash")
                        && get(get(&f, "receipt"), "globalLedgerHash")
                            == get(&g, "globalLedgerHash"),
                    "CURRENT_HEDGE_ECONOMICS_CHANGED",
                )?;
                let graph_r = copy(get(&cache, "graphReceipt"));
                let coll_r = copy(get(&cache, "collateralReceipt"));
                let mut e = get(&cache, "hedgePacket").as_object().cloned().unwrap_or_default();
                e.insert("graphReceipt".into(), graph_r.clone());
                e.insert("collateralReceipt".into(), coll_r.clone());
                let mut ane = get(get(&cache, "hedgePacket"), "allNoneEvidence")
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                ane.insert("graphReceipt".into(), graph_r.clone());
                ane.insert("collateralReceipt".into(), coll_r.clone());
                ane.insert(
                    "sourceHash".into(),
                    json!(payload_hash(&json!({
                        "graph": get(&graph_r, "sourceBodyHash"),
                        "collateral": get(&coll_r, "sourceBodyHash"),
                    }))?),
                );
                ane.insert("confirmedAt".into(), get(&coll_r, "receivedAt").clone());
                e.insert("allNoneEvidence".into(), V::Object(ane));
                e.insert("now".into(), json!(iso(self.now())));
                let e = V::Object(e);
                let p = crate::baal_scaled_owned_no_hedge_proposal_v1::plan_baal_owned_no_hedge(&e);
                need(
                    get(&p, "proposalReady") == &V::Bool(true),
                    &format!("ORIGINAL_HEDGE_PACKET_{}", get(&p, "reason").as_str().unwrap_or("")),
                )?;
                return Ok(e);
            }
            let h = self.hedge.borrow().clone().unwrap();
            return h.get_evidence(c);
        }
        if ["prepare_sell", "place_sell"].contains(&op.as_str()) {
            let fr = get(c, "fenceRisk");
            return self.sell_evidence(&op, if fr.is_null() { None } else { Some(fr) });
        }
        let cache = self.cache.borrow().clone();
        need(
            op == "reconcile"
                && get(&cache, "kind").as_str() == Some("reconcile")
                && !self.busy.get()
                && (get(c, "orderId").is_null()
                    || get(c, "orderId") == get(get(&cache, "binding"), "orderId")),
            "RECONCILE_CACHE",
        )?;
        let g = self.guard(None)?;
        self.none(&cache)?;
        let mut receipts: Vec<V> = Vec::new();
        receipts.push(get(&cache, "order").clone());
        receipts.extend(get(&cache, "fillPages").as_array().cloned().unwrap_or_default());
        receipts.push(get(get(&cache, "after"), "cash").clone());
        receipts.push(get(get(&cache, "after"), "positions").clone());
        receipts.extend(
            get(get(&cache, "after"), "openOrdersPages").as_array().cloned().unwrap_or_default(),
        );
        for (_, pages) in get(get(&cache, "after"), "scopedOrdersPagesByExchange")
            .as_object()
            .map(|m| m.iter().collect::<Vec<_>>())
            .unwrap_or_default()
        {
            receipts.extend(pages.as_array().cloned().unwrap_or_default());
        }
        for r in &receipts {
            self.raw(r, get(r, "endpoint").as_str().unwrap_or(""), 20000)?;
        }
        Ok(json!({
            "order": copy(get(&cache, "order")),
            "fillPages": copy(get(&cache, "fillPages")),
            "after": copy(get(&cache, "after")),
            "globalRisk": g,
            "graphReceipt": copy(get(&cache, "graphReceipt")),
            "collateralReceipt": copy(get(&cache, "collateralReceipt")),
        }))
    }

    // begin(ctx,max,fn)
    fn begin<F>(&self, ctx: Rc<RefCell<V>>, max: i64, f: F) -> Result<(), String>
    where
        F: FnOnce(&Self) -> Result<(), String>,
    {
        need(!self.busy.get(), "BUSY")?;
        *self.cache.borrow_mut() = V::Null;
        *self.job.borrow_mut() = Some(ctx.clone());
        self.busy.set(true);
        self.reads.set(0);
        self.maximum_reads.set(max);
        self.last_end.set(None);
        self.invocations.borrow_mut().clear();
        let result = self
            .identity()
            .and_then(|_| f(self))
            .and_then(|_| {
                *self.cache.borrow_mut() = ctx.borrow().clone();
                Ok(())
            });
        self.busy.set(false);
        *self.job.borrow_mut() = None;
        result
    }


    /// authorizes by guard-pointer values (see authorizes).
    pub fn authorizes_ptrs(&self, spec: &V, bd_ptr: usize, bn_ptr: usize) -> bool {
        (|| -> Result<bool, String> {
            need(!self.closed.get(), "CLOSED")?;
            let p = self.permits.borrow().get(&bd_ptr).cloned();
            let p = match p {
                Some(p) => p,
                None => {
                    let h = self.hedge.borrow().clone();
                    return Ok(h
                        .map(|h| h.authorizes_ptrs(spec, bd_ptr, bn_ptr))
                        .unwrap_or(false));
                }
            };
            self.identity()?;
            Ok(get(spec, "method").as_str() == Some("GET")
                && get(spec, "endpoint") == get(&p, "endpoint")
                && (bn_ptr as u64) == get(&p, "beforeNetworkPtr").as_u64().unwrap_or(0)
                && spec.get("body").is_none()
                && spec.get("bodyJson").is_none()
                && spec.get("headers").is_none())
        })()
        .unwrap_or(false)
    }

    /// authorizes(spec)
    pub fn authorizes(&self, spec: &V, before_dispatch: &Guard, before_network: &Guard) -> bool {
        (|| -> Result<bool, String> {
            need(!self.closed.get(), "CLOSED")?;
            let p = self
                .permits
                .borrow()
                .get(&(before_dispatch as *const Guard as *const () as usize))
                .cloned();
            let p = match p {
                Some(p) => p,
                None => {
                    let h = self.hedge.borrow().clone();
                    return Ok(h
                        .map(|h| h.authorizes(spec, before_dispatch, before_network))
                        .unwrap_or(false));
                }
            };
            self.identity()?;
            Ok(get(spec, "method").as_str() == Some("GET")
                && get(spec, "endpoint") == get(&p, "endpoint")
                && (before_network as *const Guard as *const () as usize)
                    == get(&p, "beforeNetworkPtr").as_u64().unwrap_or(0) as usize
                && spec.get("body").is_none()
                && spec.get("bodyJson").is_none()
                && spec.get("headers").is_none())
        })()
        .unwrap_or(false)
    }

    /// refresh()
    pub fn refresh(&self) -> Result<V, String> {
        need(!self.busy.get(), "BUSY")?;
        *self.cache.borrow_mut() = V::Null;
        *self.job.borrow_mut() = None;
        if let Some(h) = self.hedge.borrow().as_ref() {
            h.close()?;
        }
        *self.hedge.borrow_mut() = None;
        if let Some(refresh_after) = &self.deps.refresh_funding_after_account {
            let fex = self.first_exchange_id.clone().unwrap_or_default();
            let owned = (self.deps.get_owned)(&fex)?;
            let graph_leg = self
                .legs
                .iter()
                .find(|e| get(e, "exchangeId").as_str() == Some(fex.as_str()))
                .cloned()
                .unwrap_or(V::Null);
            let leg = self
                .legs
                .iter()
                .find(|e| get(e, "exchangeId").as_str() != Some(fex.as_str()))
                .cloned()
                .unwrap_or(V::Null);
            need(
                js_int(get(get(&owned, "proof"), "remainingOwnedQuantity"))
                    .is_some_and(|q| q > 0)
                    && truthy(&graph_leg)
                    && truthy(&leg),
                "CURRENT_HEDGE_SEED",
            )?;
            let ctx = Rc::new(RefCell::new(json!({
                "kind": "hedgeFinal",
                "leg": leg,
                "graphLeg": graph_leg,
                "owned": owned,
                "ownedExchangeId": fex,
            })));
            let tid = get(&self.scope, "tournamentId").as_str().unwrap_or("").to_string();
            let ctx2 = ctx.clone();
            self.begin(ctx.clone(), 10, move |this| -> Result<(), String> {
                let ctx = ctx2.borrow().clone();
                let mut root_receipts: Vec<V> = Vec::new();
                for e in [get(&ctx, "graphLeg").clone(), get(&ctx, "leg").clone()] {
                    let receipt = this.read(&format!(
                        "/markets/{}/nodes?tournamentId={}",
                        get(&e, "marketId").as_str().unwrap_or(""),
                        tid
                    ))?;
                    let mut l = e.as_object().cloned().unwrap_or_default();
                    l.insert("receipt".into(), receipt);
                    root_receipts.push(V::Object(l));
                }
                let account_source = this.account(get(&ctx, "graphLeg"), true)?;
                let binding_v = {
                    let mut m = this.scope.as_object().cloned().unwrap_or_default();
                    for (k, v) in get(&ctx, "graphLeg").as_object().cloned().unwrap_or_default() {
                        m.insert(k, v);
                    }
                    V::Object(m)
                };
                let view =
                    crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account(&json!({
                        "account": account_source,
                        "binding": binding_v,
                        "readAt": iso(this.now()),
                    }))?;
                let remaining = js_int(get(&get(get(&ctx, "owned"), "proof").clone(), "remainingOwnedQuantity"))
                    .unwrap_or(0);
                need(
                    get(&view, "positions").as_array().is_some_and(|ps| {
                        ps.iter().any(|p| {
                            get(p, "exchangeId")
                                == get(get(&ctx, "graphLeg"), "exchangeId")
                                && get(p, "marketId") == get(get(&ctx, "graphLeg"), "marketId")
                                && num_eq(get(p, "quantity"), -(remaining as f64))
                                && get(p, "settled") != &V::Bool(true)
                        })
                    }) && !get(&view, "positions").as_array().is_some_and(|ps| {
                        ps.iter().any(|p| {
                            get(p, "exchangeId") == get(get(&ctx, "leg"), "exchangeId")
                                && !num_eq(get(p, "quantity"), 0.0)
                        })
                    }),
                    "CURRENT_SOURCE_CUSTODY",
                )?;
                (refresh_after)()?;
                let funding = this.fund()?;
                let pair_census = (this.deps.get_pair_census)(this.first_exchange_id.as_deref().unwrap_or(""))?;
                let global_risk = this.guard(None)?;
                let other_book = this.read(&format!(
                    "/markets/{}/orderbook?tournamentId={}&depth=20",
                    get(get(&ctx, "leg"), "marketId").as_str().unwrap_or(""),
                    tid
                ))?;
                {
                    let mut c = ctx2.borrow_mut();
                    c["otherBookReceipt"] = other_book.clone();
                    c["book"] = other_book;
                }
                this.finish_none(&ctx2)?;
                let (graph_r, coll_r) = {
                    let c = ctx2.borrow();
                    (get(&c, "graphReceipt").clone(), get(&c, "collateralReceipt").clone())
                };
                {
                    let mut c = ctx2.borrow_mut();
                    c["hedgePacket"] = json!({
                        "scope": copy(&this.scope),
                        "firstExchangeId": this.first_exchange_id.clone().map(|x| json!(x)),
                        "owned": copy(get(&ctx, "owned")),
                        "pairCensus": pair_census,
                        "funding": funding,
                        "globalRisk": global_risk,
                        "account": account_source,
                        "rootReceipts": root_receipts,
                        "otherBookReceipt": get(&ctx2.borrow(), "otherBookReceipt").clone(),
                        "graphReceipt": copy(&graph_r),
                        "collateralReceipt": copy(&coll_r),
                        "now": iso(this.now()),
                        "maximumPilotQuantity": 1,
                    });
                }
                Ok(())
            })?;
            *self.hedge_packet.borrow_mut() = get(&self.cache.borrow().clone(), "hedgePacket").clone();
            return self.get_evidence(&json!({"operation": "prepare_hedge"}));
        }
        let h = create_owned_no_fresh_none_hedge_evidence_bridge(
            Rc::clone(&self.api),
            self.receipt_of.clone(),
            &self.scope,
            self.first_exchange_id.as_deref().unwrap_or(""),
            OwnedNoneBridgeDeps {
                get_owned: self.deps.get_owned.clone(),
                get_pair_census: self.deps.get_pair_census.clone(),
                get_funding: self.deps.get_funding.clone(),
                get_global_risk: self.deps.get_global_risk.clone(),
                get_account_lease: self.deps.get_account_lease.clone(),
                can_read: self.deps.can_read.clone(),
                has_read_headroom: self.deps.has_read_headroom.clone(),
                get_active_intent: self.deps.get_active_intent.clone(),
                get_order_binding: self.deps.get_order_binding.clone(),
                now: self.deps.now.clone(),
            },
        )?;
        *self.hedge.borrow_mut() = Some(Rc::new(h));
        let result = self
            .hedge
            .borrow()
            .as_ref()
            .unwrap()
            .refresh();
        match result {
            Ok(packet) => {
                *self.hedge_packet.borrow_mut() = copy(&packet);
                let last = self
                    .hedge
                    .borrow()
                    .as_ref()
                    .map(|h| js_int(get(&h.status(), "lastReadCount")).unwrap_or(0))
                    .unwrap_or(0);
                self.total_read_count.set(self.total_read_count.get() + last);
                Ok(copy(&packet))
            }
            Err(e) => {
                let last = self
                    .hedge
                    .borrow()
                    .as_ref()
                    .map(|h| js_int(get(&h.status(), "lastReadCount")).unwrap_or(0))
                    .unwrap_or(0);
                self.total_read_count.set(self.total_read_count.get() + last);
                Err(e)
            }
        }
    }

    /// refreshSell(leg)
    pub fn refresh_sell(&self, leg: &V) -> Result<V, String> {
        need(
            self.legs.iter().any(|e| same(e, leg).unwrap_or(false)),
            "OWN_SELL_SCOPE",
        )?;
        let owned = (self.deps.get_owned)(get(leg, "exchangeId").as_str().unwrap_or(""))?;
        need(
            get(get(&owned, "proof"), "marketId") == get(leg, "marketId")
                && get(get(&owned, "proof"), "exchangeId") == get(leg, "exchangeId")
                && js_int(get(get(&owned, "proof"), "remainingOwnedQuantity"))
                    .is_some_and(|q| q > 0),
            "OWN_CURRENT_SELL",
        )?;
        let ctx = Rc::new(RefCell::new(json!({
            "kind": "sell",
            "leg": copy(leg),
            "owned": owned,
        })));
        let tid = get(&self.scope, "tournamentId").as_str().unwrap_or("").to_string();
        let ctx2 = ctx.clone();
        let leg2 = leg.clone();
        self.begin(ctx.clone(), 7, move |this| -> Result<(), String> {
            let account = this.account(&leg2, false)?;
            ctx2.borrow_mut()["account"] = account;
            let mut binding = this.scope.as_object().cloned().unwrap_or_default();
            for (k, v) in leg2.as_object().cloned().unwrap_or_default() {
                binding.insert(k, v);
            }
            crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account(&json!({
                "account": ctx2.borrow().get("account").cloned().unwrap_or(V::Null),
                "binding": V::Object(binding),
                "readAt": iso(this.now()),
            }))?;
            if let Some(f) = &this.deps.refresh_funding_after_account {
                f()?;
            }
            let source_funding = this.fund()?;
            ctx2.borrow_mut()["sourceFunding"] = source_funding;
            let source_risk = this.guard(None)?;
            need(
                get(&source_risk, "unresolvedIntentIds")
                    .as_array()
                    .is_some_and(|a| a.is_empty()),
                "PENDING_BEFORE_BOOK",
            )?;
            ctx2.borrow_mut()["sourceRisk"] = source_risk;
            let book = this.read(&format!(
                "/markets/{}/orderbook?tournamentId={}&depth=20",
                get(&leg2, "marketId").as_str().unwrap_or(""),
                tid
            ))?;
            ctx2.borrow_mut()["book"] = book;
            this.finish_none(&ctx2)?;
            Ok(())
        })?;
        self.sell_evidence("prepare_sell", None)
    }

    /// refreshPreparedNone()
    pub fn refresh_prepared_none(&self) -> Result<V, String> {
        need(!self.busy.get(), "BUSY")?;
        let a = self.active()?;
        let body = get(&a, "body");
        let afc = a.as_object().and_then(|m| m.get("actualFetchCount"));
        let fc = a.as_object().and_then(|m| m.get("fetchCount"));
        let eff = afc.filter(|v| !v.is_null()).or(fc).unwrap_or(&V::Null);
        need(
            get(&a, "phase").as_str() == Some("prepared")
                && get(&a, "orderId").is_null()
                && num_eq(eff, 0.0)
                && get(&a, "bodyHash").as_str() == Some(payload_hash(body)?.as_str())
                && get(body, "side").as_str() == Some("no")
                && (instant_s(get(body, "expirationDate"))? as f64) > self.now(),
            "EXACT_UNFETCHED_PREPARED",
        )?;
        let ctx: Rc<RefCell<V>>;
        if get(body, "action").as_str() == Some("buy") {
            let hp = self.hedge_packet.borrow().clone();
            need(
                truthy(&hp)
                    && num_eq(get(body, "quantity"), 1.0)
                    && get(&a, "firstExchangeId") == get(&hp, "firstExchangeId"),
                "ORIGINAL_HEDGE_BINDING",
            )?;
            let leg = self
                .legs
                .iter()
                .find(|e| get(e, "exchangeId") == get(body, "exchangeId"))
                .cloned()
                .unwrap_or(V::Null);
            let graph_leg = self
                .legs
                .iter()
                .find(|e| get(e, "exchangeId") == get(&a, "firstExchangeId"))
                .cloned()
                .unwrap_or(V::Null);
            need(
                truthy(&leg)
                    && truthy(&graph_leg)
                    && get(body, "tournamentId") == get(&self.scope, "tournamentId"),
                "ORIGINAL_HEDGE_SCOPE",
            )?;
            ctx = Rc::new(RefCell::new(json!({
                "kind": "hedgeFinal",
                "leg": leg,
                "graphLeg": graph_leg,
                "owned": copy(get(&hp, "owned")),
                "ownedExchangeId": get(&a, "firstExchangeId"),
                "otherBookReceipt": copy(get(&hp, "otherBookReceipt")),
                "book": copy(get(&hp, "otherBookReceipt")),
                "hedgePacket": copy(&hp),
            })));
        } else {
            let cache = self.cache.borrow().clone();
            need(
                get(body, "action").as_str() == Some("sell")
                    && get(&cache, "kind").as_str() == Some("sell")
                    && get(get(&cache, "leg"), "exchangeId") == get(body, "exchangeId")
                    && (get(body, "quantity").as_f64().unwrap_or(f64::INFINITY)
                        <= get(get(get(&cache, "owned"), "proof"), "remainingOwnedQuantity")
                            .as_f64()
                            .unwrap_or(f64::NEG_INFINITY)),
                "ORIGINAL_SELL_BINDING",
            )?;
            ctx = Rc::new(RefCell::new(copy(&cache)));
        }
        ctx.borrow_mut()["preparedBinding"] = copy(&a);
        let is_buy = get(body, "action").as_str() == Some("buy");
        let source_pin = {
            let c = ctx.borrow();
            payload_hash(&if is_buy {
                get(&c, "hedgePacket").clone()
            } else {
                json!({
                    "account": get(&c, "account"),
                    "book": get(&c, "book"),
                    "owned": get(&c, "owned"),
                    "sourceRisk": get(&c, "sourceRisk"),
                    "sourceFunding": get(&c, "sourceFunding"),
                })
            })?
        };
        self.begin(ctx.clone(), 2, move |this| this.finish_none(&ctx))?;
        let cache = self.cache.borrow().clone();
        let after_pin = payload_hash(&if is_buy {
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
        need(source_pin == after_pin, "ORIGINAL_PACKET_CHANGED")?;
        self.get_evidence(&json!({
            "operation": if is_buy { "place_hedge" } else { "place_sell" }
        }))
    }

    /// refreshFinalNone()
    pub fn refresh_final_none(&self) -> Result<V, String> {
        self.refresh_prepared_none()
    }

    /// refreshReconciliation(orderId)
    pub fn refresh_reconciliation(&self, order_id: &str) -> Result<V, String> {
        let binding = (self.deps.get_order_binding)(order_id)?;
        let leg = self
            .legs
            .iter()
            .find(|e| get(e, "exchangeId") == get(get(&binding, "body"), "exchangeId"))
            .cloned()
            .unwrap_or(V::Null);
        let body = get(&binding, "body");
        need(
            get(&binding, "orderId").as_str() == Some(order_id)
                && truthy(&leg)
                && get(&binding, "bodyHash").as_str() == Some(payload_hash(body)?.as_str())
                && get(body, "side").as_str() == Some("no")
                && ["buy", "sell"].contains(&get(body, "action").as_str().unwrap_or(""))
                && get(body, "tournamentId") == get(&self.scope, "tournamentId")
                && js_int(get(body, "quantity")).is_some_and(|q| q > 0 && q <= 100),
            "KNOWN_OWN_BINDING",
        )?;
        let graph_leg = if get(body, "action").as_str() == Some("buy") {
            let bfe = get(&binding, "firstExchangeId");
            let fex = if bfe.is_null() {
                self.first_exchange_id.clone().unwrap_or_default()
            } else {
                bfe.as_str().unwrap_or("").to_string()
            };
            self.legs
                .iter()
                .find(|e| get(e, "exchangeId").as_str() == Some(fex.as_str()))
                .cloned()
                .unwrap_or(V::Null)
        } else {
            leg.clone()
        };
        need(truthy(&graph_leg), "ACTUAL_ORDER_GRAPH_SCOPE")?;
        let tid = get(&self.scope, "tournamentId").as_str().unwrap_or("").to_string();
        let ctx = Rc::new(RefCell::new(json!({
            "kind": "reconcile",
            "leg": leg,
            "graphLeg": graph_leg,
            "binding": binding,
        })));
        let order_id_s = order_id.to_string();
        let ctx2 = ctx.clone();
        self.begin(ctx.clone(), 9, move |this| -> Result<(), String> {
            let order = this.read(&format!("/orders/{order_id_s}"))?;
            ctx2.borrow_mut()["order"] = order;
            let order_v = ctx2.borrow().get("order").cloned().unwrap_or(V::Null);
            let order_endpoint = get(&order_v, "endpoint").as_str().unwrap_or("").to_string();
            let parsed = this.raw(&order_v, &order_endpoint, 20000)?;
            let binding_body = {
                let b = ctx2.borrow().get("binding").cloned().unwrap_or(V::Null);
                get(&b, "body").clone()
            };
            let od = get(&parsed, "data");
            let n_key = js_stringify(&json!(["id"]));
            let lexeme = get(&parsed, "numbers").get(&n_key).cloned().unwrap_or(V::Null);
            need(
                num_eq(get(od, "id"), order_id_s.parse::<f64>().unwrap_or(f64::NAN))
                    && lexeme.as_str() == Some(order_id_s.as_str())
                    && get(od, "exchangeId") == get(&ctx2.borrow().get("leg").cloned().unwrap_or(V::Null), "exchangeId")
                    && get(od, "tournamentId") == get(&this.scope, "tournamentId")
                    && get(od, "side").as_str() == Some("no")
                    && get(od, "action") == get(&binding_body, "action")
                    && js_int(get(od, "quantity")).is_some_and(|q| q >= 1)
                    && (js_int(get(od, "quantity")).unwrap_or(i64::MAX) as f64)
                        <= get(&binding_body, "quantity").as_f64().unwrap_or(f64::NEG_INFINITY)
                    && js_int(get(od, "quantityFilled")).is_some_and(|q| q >= 0)
                    && get(od, "open").is_boolean()
                    && (get(od, "open") == &V::Bool(true)
                        || get(od, "quantityFilled").as_f64().unwrap_or(f64::INFINITY)
                            <= get(od, "quantity").as_f64().unwrap_or(f64::NEG_INFINITY)),
                "EXACT_OWN_ORDER_RESPONSE",
            )?;
            let fill_pages = this.page(&format!("/orders/{order_id_s}/fills?limit=200"))?;
            ctx2.borrow_mut()["fillPages"] = json!(fill_pages);
            let fill_r = ctx2.borrow().get("fillPages").cloned().unwrap_or(V::Null);
            let first_page = fill_r.as_array().and_then(|a| a.first().cloned()).unwrap_or(V::Null);
            let fills = this.raw(&first_page, get(&first_page, "endpoint").as_str().unwrap_or(""), 20000)?;
            let fd = get(&fills, "data");
            need(
                num_eq(get(fd, "orderId"), order_id_s.parse::<f64>().unwrap_or(f64::NAN))
                    && get(fd, "exchangeId") == get(&ctx2.borrow().get("leg").cloned().unwrap_or(V::Null), "exchangeId")
                    && get(fd, "tournamentId") == get(&this.scope, "tournamentId")
                    && get(fd, "data").as_array().is_some_and(|rows| {
                        rows.iter().all(|v| {
                            get(v, "side").as_str() == Some("no")
                                && js_int(get(v, "quantity")).is_some_and(|q| q < 0)
                        })
                    }),
                "EXACT_OWN_FILL_RESPONSE",
            )?;
            let after = this.account(&ctx2.borrow().get("leg").cloned().unwrap_or(V::Null), true)?;
            ctx2.borrow_mut()["after"] = after;
            this.finish_none(&ctx2)?;
            Ok(())
        })?;
        self.get_evidence(&json!({"operation": "reconcile", "orderId": order_id}))
    }

    /// status()
    pub fn status(&self) -> V {
        json!({
            "mode": "combined_owned_no_none_evidence_bridge_v2",
            "busy": self.busy.get(),
            "closed": self.closed.get(),
            "cacheKind": get(&self.cache.borrow().clone(), "kind").clone(),
            "lastReadCount": self.reads.get(),
            "maximumReads": self.maximum_reads.get(),
            "totalReadCount": self.total_read_count.get(),
            "hedge": self.hedge.borrow().as_ref().map(|h| h.status()).unwrap_or(V::Null),
            "executionAuthority": false,
            "additionalApiClients": 0,
        })
    }

    /// clear()
    pub fn clear(&self) {
        *self.cache.borrow_mut() = V::Null;
        if let Some(h) = self.hedge.borrow().as_ref() {
            h.clear();
        }
    }

    /// close()
    pub fn close(&self) -> Result<(), String> {
        need(!self.busy.get(), "BUSY")?;
        self.closed.set(true);
        *self.cache.borrow_mut() = V::Null;
        if let Some(h) = self.hedge.borrow().as_ref() {
            h.close()?;
        }
        self.watermarks.borrow_mut().clear();
        Ok(())
    }
}



/// createOwnedNoFreshNoneHedgeEvidenceBridgeV2(options)
pub fn create_owned_no_fresh_none_hedge_evidence_bridge_v2(
    api: Rc<RefCell<OwnedNoneBridgeApi>>,
    receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    scope: &V,
    first_exchange_id: Option<&str>,
    pair_legs: Option<&V>,
    deps: OwnedNoneBridgeV2Deps,
) -> Result<OwnedNoFreshNoneHedgeEvidenceBridgeV2, String> {
    OwnedNoFreshNoneHedgeEvidenceBridgeV2::new(
        api,
        receipt_of,
        scope,
        first_exchange_id,
        pair_legs,
        deps,
    )
}
