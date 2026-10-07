//! Port of execution/owned-no-fresh-none-hedge-evidence-bridge-v1.mjs.
//! Read-only NONE evidence bridge for the sole existing client. The async
//! `api.dispatch` maps to the crate's synchronous WireDispatch convention.
//! JS error objects carried retryAfterMs/fatal flags — this port keeps only
//! the error code string (matching the crate's String error convention).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use serde_json::{json, Map, Value as V};

use baal_service_runtime::fixed::instant;
use baal_service_runtime::json_text::{js_stringify, sha256 as sha};

use crate::core::payload_hash;
use crate::paired_rest_controller::{WireDispatch, WireDispatchError};
use crate::receipts::parse_number_lexemes;
use crate::transport::{Guard, OrderTransportError};

fn need(v: bool, code: &str) -> Result<(), String> {
    if v {
        Ok(())
    } else {
        Err(format!("NONE_HEDGE_EVIDENCE_{code}"))
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
fn none_evidence_code(e: &str) -> bool {
    e.strip_prefix("NONE_HEDGE_EVIDENCE_").is_some_and(|s| {
        !s.is_empty() && s.bytes().all(|c| c.is_ascii_uppercase() || c == b'_')
    })
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

pub struct EvidenceBridgeApi {
    pub dispatch: Rc<WireDispatch>,
    pub manual_review: Rc<dyn Fn() -> bool>,
    pub cooldown: Rc<dyn Fn() -> f64>,
}

#[derive(Clone)]
pub struct OwnedNoFreshNoneBridgeDeps {
    pub api: Rc<EvidenceBridgeApi>,
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub get_owned: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub get_pair_census: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub get_funding: Rc<dyn Fn() -> Result<V, String>>,
    pub get_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub can_read: Rc<dyn Fn() -> bool>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> bool>,
    pub get_active_intent: Rc<dyn Fn() -> Result<V, String>>,
    pub get_order_binding: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct BridgeInner {
    scope: V,
    first_exchange_id: String,
    opening: V,
    first: V,
    other: V,
    original_owned_hash: String,
    permits: HashMap<usize, (String, usize)>,
    watermarks: HashMap<String, (i64, i64)>,
    invocations: HashSet<String>,
    busy: bool,
    closed: bool,
    cache: Option<V>,
    reads: i64,
    last_end: Option<i64>,
    last_failure: Option<String>,
    job_binding: Option<V>,
    deps: OwnedNoFreshNoneBridgeDeps,
}

pub struct OwnedNoFreshNoneHedgeEvidenceBridge {
    pub(crate) inner: Rc<RefCell<BridgeInner>>,
}

impl OwnedNoFreshNoneHedgeEvidenceBridge {
    fn own_pending(&self, g: &V) -> Result<bool, String> {
        let inner = self.inner.borrow();
        let o = (inner.deps.get_active_intent)()?;
        let afc = get(&o, "actualFetchCount");
        let fc = get(&o, "fetchCount");
        let fetch_zero = if !afc.is_null() {
            afc.as_i64() == Some(0)
        } else {
            fc.as_i64() == Some(0)
        };
        Ok(!o.is_null()
            && sget(&o, "intentId").is_some()
            && sget(&o, "firstExchangeId") == Some(inner.first_exchange_id.as_str())
            && ["prepared", "dispatching"].contains(&sget(&o, "phase").unwrap_or(""))
            && get(&o, "orderId").is_null()
            && fetch_zero
            && (afc.is_null() || fc.is_null() || afc == fc)
            && sget(get(&o, "body"), "side") == Some("no")
            && sget(get(&o, "body"), "action") == Some("buy")
            && get(get(&o, "body"), "quantity") == &json!(1)
            && sget(get(&o, "body"), "exchangeId") == sget(&inner.other, "exchangeId")
            && sget(get(&o, "body"), "tournamentId") == sget(&inner.scope, "tournamentId")
            && sget(&o, "bodyHash") == Some(payload_hash(get(&o, "body"))?.as_str())
            && sget(get(&o, "body"), "idempotencyKey").is_some()
            && inst(get(&o, "body"), "expirationDate")? > (inner.deps.now)() as i64
            && get(g, "unresolvedIntentIds")
                .as_array()
                .is_some_and(|ids| ids.len() == 1 && &ids[0] == get(&o, "intentId")))
    }
    fn cleanup_own(&self, g: &V) -> Result<bool, String> {
        let b = {
            let inner = self.inner.borrow();
            inner.job_binding.clone().or_else(|| {
                inner
                    .cache
                    .as_ref()
                    .map(|c| get(c, "binding").clone())
                    .filter(|b| !b.is_null())
            })
        };
        let Some(b) = b else { return Ok(false) };
        let actual = (self.inner.borrow().deps.get_order_binding)(get(&b, "orderId"))?;
        if actual.is_null() {
            return Ok(false);
        }
        let inner = self.inner.borrow();
        Ok(sget(&actual, "intentId") == sget(&b, "intentId")
            && sget(&actual, "orderId") == sget(&b, "orderId")
            && sget(&actual, "bodyHash") == sget(&b, "bodyHash")
            && get(get(&actual, "body"), "quantity") == &json!(1)
            && sget(get(&actual, "body"), "action") == Some("buy")
            && sget(get(&actual, "body"), "side") == Some("no")
            && sget(get(&actual, "body"), "exchangeId") == sget(&inner.other, "exchangeId")
            && sget(get(&actual, "body"), "tournamentId") == sget(&inner.scope, "tournamentId")
            && sget(&actual, "bodyHash")
                == Some(payload_hash(get(&actual, "body"))?.as_str())
            && get(g, "unresolvedIntentIds").as_array().is_some_and(|ids| {
                ids.iter().all(|id| id == get(&b, "intentId"))
            }))
    }
    fn local(&self) -> Result<V, String> {
        {
            let inner = self.inner.borrow();
            need(!inner.closed && (inner.deps.can_read)(), "STOPPED")?;
            let lease = (inner.deps.get_account_lease)()?;
            need(
                sget(&lease, "ownerId") == sget(&inner.scope, "ownerId")
                    && sget(&lease, "tournamentId") == sget(&inner.scope, "tournamentId")
                    && hex(get(&lease, "leaseId")),
                "ACCOUNT_LEASE",
            )?;
            let owned_now =
                (inner.deps.get_owned)(&json!(inner.first_exchange_id.clone()))?;
            need(
                payload_hash(&owned_now)? == inner.original_owned_hash,
                "OWNED_SOURCE_CHANGED",
            )?;
        }
        let g = (self.inner.borrow().deps.get_global_risk)()?;
        let cleanup = self.cleanup_own(&g)?;
        let now = (self.inner.borrow().deps.now)();
        let unresolved_ok = if get(&g, "unresolvedIntentIds")
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(false)
        {
            true
        } else {
            self.own_pending(&g)? || self.cleanup_own(&g)?
        };
        let inner = self.inner.borrow();
        need(
            sget(&g, "ownerId") == sget(&inner.scope, "ownerId")
                && sget(&g, "runId") == sget(&inner.scope, "runId")
                && sget(&g, "tournamentId") == sget(&inner.scope, "tournamentId")
                && get(&g, "complete") == &V::Bool(true)
                && get(&g, "eventScopeComplete") == &V::Bool(true)
                && get(&g, "soleCollector") == &V::Bool(true)
                && get(&g, "stopped") == &V::Bool(false)
                && (get(&g, "unknownAcceptance") == &V::Bool(false) || cleanup)
                && get(&g, "unresolvedIntentIds").is_array()
                && unresolved_ok
                && hex(get(&g, "sourceHash"))
                && inst(&g, "at")? <= now as i64
                && now as i64 - inst(&g, "at")? <= 20000,
            "UNKNOWN_OR_GLOBAL_SOURCE",
        )?;
        need(
            !(inner.deps.api.manual_review)() && now >= (inner.deps.api.cooldown)(),
            "SHARED_BACKOFF",
        )?;
        Ok(g)
    }
    fn raw(
        &self,
        r: &V,
        endpoint: &str,
        at: i64,
        age: i64,
    ) -> Result<(V, BTreeMap<String, String>), String> {
        let allowed = [
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
        need(
            !r.is_null()
                && r.as_object()
                    .is_some_and(|m| m.keys().all(|k| allowed.contains(&k.as_str()))),
            "RAW_FIELD_SCOPE",
        )?;
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
                && sget(r, "fetchInvocationId").is_some_and(|s| !s.is_empty()),
            "RAW_SOURCE",
        )?;
        let start = inst(r, "requestStartedAt")?;
        let end = inst(r, "receivedAt")?;
        need(start <= end && end <= at && at - start <= age, "SOURCE_CLOCK")?;
        parse_number_lexemes(sget(r, "bodyText").unwrap_or(""))
    }
    fn none(&self, v: &V, at: i64) -> Result<V, String> {
        let (first, other, scope) = {
            let inner = self.inner.borrow();
            (inner.first.clone(), inner.other.clone(), inner.scope.clone())
        };
        let graph_endpoint = format!(
            "/relationships/graph?marketId={}&depth=4&tournamentId={}",
            sget(&first, "marketId").unwrap_or(""),
            sget(&scope, "tournamentId").unwrap_or("")
        );
        let collateral_endpoint = format!(
            "/portfolio/collateral?tournamentId={}",
            sget(&scope, "tournamentId").unwrap_or("")
        );
        let graph = self
            .raw(get(v, "graphReceipt"), &graph_endpoint, at, 5000)?
            .0;
        let collateral = self
            .raw(get(v, "collateralReceipt"), &collateral_endpoint, at, 5000)?
            .0;
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
                && get(&graph, "nodes")
                    .as_array()
                    .map(|x| x.len() <= 2)
                    .unwrap_or(false),
            "NON_NONE_OR_UNSUPPORTED_GRAPH",
        )?;
        let mut ids = HashSet::new();
        for n in get(&graph, "nodes").as_array().cloned().unwrap_or_default() {
            need(
                !n.is_null()
                    && sget(&n, "exchangeId").is_some()
                    && !ids.contains(sget(&n, "exchangeId").unwrap_or(""))
                    && [&first, &other].iter().any(|e| {
                        sget(e, "exchangeId") == sget(&n, "exchangeId")
                            && sget(e, "marketId") == sget(&n, "marketId")
                    }),
                "FOREIGN_GRAPH_NODE",
            )?;
            ids.insert(sget(&n, "exchangeId").unwrap_or("").to_string());
        }
        need(
            !collateral.is_null()
                && collateral
                    .as_object()
                    .map(|m| {
                        m.keys().map(|k| k.as_str()).collect::<Vec<_>>().join("|") == "data"
                    })
                    .unwrap_or(false)
                && get(&collateral, "data")
                    .as_array()
                    .map(|x| x.is_empty())
                    .unwrap_or(false),
            "NON_NONE_COLLATERAL",
        )?;
        let preceding = {
            let ob = get(v, "otherBookReceipt");
            if !ob.is_null() {
                inst(ob, "receivedAt")?
            } else {
                let arr = get(
                    get(get(v, "after"), "scopedOrdersPagesByExchange"),
                    sget(&other, "exchangeId").unwrap_or(""),
                )
                .as_array()
                .cloned()
                .unwrap_or_default();
                let last = arr.last().cloned().unwrap_or(V::Null);
                inst(&last, "receivedAt")?
            }
        };
        need(
            inst(get(v, "collateralReceipt"), "requestStartedAt")?
                >= inst(get(v, "graphReceipt"), "receivedAt")?
                && inst(get(v, "graphReceipt"), "requestStartedAt")? >= preceding,
            "NONE_CAUSALITY",
        )?;
        Ok(json!({
            "schemaVersion": 1,
            "mode": "fresh_owned_pair_none_read_confirmation_v1",
            "graphReceipt": get(v,"graphReceipt"),
            "collateralReceipt": get(v,"collateralReceipt"),
            "sourceHash": payload_hash(&json!({
                "graph": get(get(v,"graphReceipt"),"sourceBodyHash"),
                "collateral": get(get(v,"collateralReceipt"),"sourceBodyHash"),
            }))?,
            "confirmedAt": get(get(v,"collateralReceipt"),"receivedAt"),
            "maximumAgeMs": 5000,
            "executionAuthority": false,
        }))
    }
    fn same_pending_economics(&self, before: &V, current: &V) -> Result<bool, String> {
        let strip = |r: &V| -> V {
            let mut m = r.as_object().cloned().unwrap_or_default();
            m.shift_remove("reservationHash");
            m.shift_remove("unresolvedIntentIds");
            V::Object(m)
        };
        let hash_valid = |r: &V| -> Result<bool, String> {
            let rh = sget(r, "reservationHash").unwrap_or("").to_string();
            let mut m = r.as_object().cloned().unwrap_or_default();
            m.shift_remove("reservationHash");
            Ok(hex(&json!(rh.clone())) && payload_hash(&V::Object(m))? == rh)
        };
        let o = (self.inner.borrow().deps.get_active_intent)()?;
        Ok(hash_valid(before)?
            && hash_valid(current)?
            && get(before, "unresolvedIntentIds")
                .as_array()
                .map(|x| x.is_empty())
                .unwrap_or(false)
            && get(current, "unresolvedIntentIds")
                .as_array()
                .map(|x| x.len() == 1 && &x[0] == get(&o, "intentId"))
                .unwrap_or(false)
            && same(&strip(before), &strip(current))?)
    }
    fn evidence(&self) -> Result<V, String> {
        self.local()?;
        {
            let inner = self.inner.borrow();
            need(inner.cache.is_some() && !inner.busy, "CACHE_UNAVAILABLE")?;
        }
        let current = self.local()?;
        let (cache, scope, first_exchange_id) = {
            let inner = self.inner.borrow();
            (
                inner.cache.clone().unwrap_or(V::Null),
                inner.scope.clone(),
                inner.first_exchange_id.clone(),
            )
        };
        let c = get(&cache, "pairCensus").clone();
        let f = get(&cache, "funding").clone();
        let g = get(&cache, "accountRisk").clone();
        let live_census =
            (self.inner.borrow().deps.get_pair_census)(&json!(first_exchange_id.clone()))?;
        let live_funding = (self.inner.borrow().deps.get_funding)()?;
        let at = (self.inner.borrow().deps.now)();
        let all_none_evidence = self.none(&cache, at as i64)?;
        let pending_branch = self.own_pending(&current)?
            && self.same_pending_economics(get(&f, "record"), get(&live_funding, "record"))?
            && sget(get(&live_funding, "receipt"), "reservationHash")
                == sget(get(&live_funding, "record"), "reservationHash")
            && sget(get(&live_funding, "receipt"), "allocationHash")
                == sget(get(&live_funding, "record"), "allocationHash")
            && sget(&current, "controllerReservationHash")
                == sget(get(&live_funding, "record"), "reservationHash")
            && sget(&current, "globalLedgerHash")
                == sget(get(&live_funding, "receipt"), "globalLedgerHash")
            && same(get(&current, "reservations"), get(&g, "reservations"))?;
        need(
            sget(&live_census, "sourceHash") == sget(&c, "sourceHash")
                && same(get(&live_census, "matchedLots"), get(&c, "matchedLots"))?
                && ((same(&live_funding, &f)?
                    && sget(&current, "globalLedgerHash") == sget(&g, "globalLedgerHash")
                    && sget(&current, "controllerReservationHash")
                        == sget(&g, "controllerReservationHash"))
                    || pending_branch)
                && sget(&current, "allocationHash") == sget(&g, "allocationHash"),
            "LOCAL_SOURCE_CHANGED",
        )?;
        need(get(&c, "counterpartOwned").is_null(), "COUNTERPART_NOT_FLAT")?;
        need(
            get(&c, "matchedLots")
                .as_array()
                .map(|x| x.is_empty())
                .unwrap_or(false),
            "ALREADY_PAIRED",
        )?;
        let opening = self.inner.borrow().opening.clone();
        let result = json!({
            "scope": scope,
            "firstExchangeId": first_exchange_id,
            "owned": opening,
            "pairCensus": c,
            "funding": f,
            "globalRisk": g,
            "account": get(&cache,"account"),
            "rootReceipts": get(&cache,"rootReceipts"),
            "otherBookReceipt": get(&cache,"otherBookReceipt"),
            "now": iso(at),
        });
        let proposal =
            crate::baal_scaled_owned_no_hedge_proposal_v1::plan_baal_owned_no_hedge(&result);
        let reason = sget(&proposal, "reason").unwrap_or("").to_string();
        need(
            get(&proposal, "proposalReady") == &V::Bool(true),
            &format!("PROPOSAL_{reason}"),
        )?;
        let mut out = result.as_object().cloned().unwrap_or_default();
        out.insert("graphReceipt".into(), get(&cache, "graphReceipt").clone());
        out.insert(
            "collateralReceipt".into(),
            get(&cache, "collateralReceipt").clone(),
        );
        out.insert("allNoneEvidence".into(), all_none_evidence);
        out.insert("maximumPilotQuantity".into(), json!(1));
        Ok(V::Object(out))
    }
    /// authorizes(spec) — spec carries no function refs in Rust; guards are
    /// passed alongside and identified by pointer (WeakMap equivalent).
    pub fn authorizes(
        &self,
        spec: &V,
        before_dispatch: Option<&Guard<'_>>,
        before_network: Option<&Guard<'_>>,
    ) -> bool {
        let r: Result<bool, String> = (|| {
            self.local()?;
            let bd_ptr = before_dispatch
                .map(|x| x as *const Guard as *const () as usize)
                .unwrap_or(0);
            let bn_ptr = before_network
                .map(|x| x as *const Guard as *const () as usize)
                .unwrap_or(0);
            let inner = self.inner.borrow();
            let p = inner.permits.get(&bd_ptr);
            Ok(p.is_some()
                && sget(spec, "method") == Some("GET")
                && Some(p.unwrap().0.as_str()) == sget(spec, "endpoint")
                && p.unwrap().1 == bn_ptr
                && spec.as_object().is_some_and(|m| {
                    !m.contains_key("body")
                        && !m.contains_key("bodyJson")
                        && !m.contains_key("headers")
                }))
        })();
        r.unwrap_or(false)
    }
    fn read(&self, endpoint: &str) -> Result<V, String> {
        self.local()?;
        {
            let inner = self.inner.borrow();
            need(
                inner.reads < 10 && (inner.deps.has_read_headroom)(10 - inner.reads),
                "HEADROOM",
            )?;
        }
        let admitted = Rc::new(Cell::new(false));
        let network = Rc::new(Cell::new(false));
        let inner_a = self.inner.clone();
        let admitted_a = admitted.clone();
        let bd_impl: Box<Guard<'_>> = Box::new(move |_: &V, _: &V| {
            OwnedNoFreshNoneHedgeEvidenceBridge {
                inner: inner_a.clone(),
            }
            .local()
            .map_err(ote_for)?;
            let g = inner_a.borrow();
            let ok = !admitted_a.get() && (g.deps.has_read_headroom)(10 - g.reads);
            drop(g);
            if !ok {
                return Err(ote_for("NONE_HEDGE_EVIDENCE_ADMISSION".into()));
            }
            admitted_a.set(true);
            Ok(true)
        });
        let inner_n = self.inner.clone();
        let admitted_n = admitted.clone();
        let network_n = network.clone();
        let bn_impl: Box<Guard<'_>> = Box::new(move |_: &V, _: &V| {
            OwnedNoFreshNoneHedgeEvidenceBridge {
                inner: inner_n.clone(),
            }
            .local()
            .map_err(ote_for)?;
            let g = inner_n.borrow();
            let ok =
                admitted_n.get() && !network_n.get() && (g.deps.has_read_headroom)(9 - g.reads);
            drop(g);
            if !ok {
                return Err(ote_for("NONE_HEDGE_EVIDENCE_NETWORK".into()));
            }
            network_n.set(true);
            Ok(true)
        });
        let bd_guard: &Guard<'_> = &*bd_impl;
        let bn_guard: &Guard<'_> = &*bn_impl;
        let bd_ptr = bd_guard as *const Guard as *const () as usize;
        let bn_ptr = bn_guard as *const Guard as *const () as usize;
        let start = (self.inner.borrow().deps.now)();
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
            self.local()?;
            need(
                admitted.get()
                    && network.get()
                    && sget(&response, "endpoint") == Some(endpoint)
                    && get(&response, "status") == &json!(200),
                "RESPONSE",
            )?;
            let r =
                (self.inner.borrow().deps.receipt_of)(&response, &json!({"method": "GET"}))?;
            let age = if endpoint.contains("/nodes?") {
                300_000
            } else {
                20_000
            };
            let now_at = (self.inner.borrow().deps.now)() as i64;
            let parsed = self.raw(&r, endpoint, now_at, age)?;
            need(
                inst(&r, "requestStartedAt")? >= start as i64
                    && inst(&r, "requestStartedAt")? >= inst(&response, "requestStartedAt")?
                    && inst(&r, "receivedAt")? <= inst(&response, "receivedAt")?
                    && same(&parsed.0, get(&response, "data"))?
                    && self
                        .inner
                        .borrow()
                        .last_end
                        .map(|le| inst(&r, "requestStartedAt").map(|x| x >= le))
                        .transpose()?
                        .unwrap_or(true),
                "SOURCE_CAUSALITY",
            )?;
            need(
                !self
                    .inner
                    .borrow()
                    .invocations
                    .contains(sget(&r, "fetchInvocationId").unwrap_or("")),
                "DUPLICATE_INVOCATION",
            )?;
            self.inner
                .borrow_mut()
                .invocations
                .insert(sget(&r, "fetchInvocationId").unwrap_or("").to_string());
            {
                let inner = self.inner.borrow();
                if let Some((os, oe)) = inner.watermarks.get(endpoint) {
                    need(
                        inst(&r, "requestStartedAt")? >= *os
                            && inst(&r, "receivedAt")? >= *oe,
                        "RECEIPT_ROLLBACK",
                    )?;
                }
            }
            self.inner.borrow_mut().watermarks.insert(
                endpoint.to_string(),
                (inst(&r, "requestStartedAt")?, inst(&r, "receivedAt")?),
            );
            let end = inst(&r, "receivedAt")?;
            self.inner.borrow_mut().last_end = Some(end);
            Ok(r)
        })();
        {
            let mut inner = self.inner.borrow_mut();
            inner.reads += 1;
            inner.permits.remove(&bd_ptr);
        }
        body_result
    }
    fn page(&self, endpoint: &str) -> Result<Vec<V>, String> {
        let r = self.read(endpoint)?;
        let now_at = (self.inner.borrow().deps.now)() as i64;
        let p = self.raw(&r, endpoint, now_at, 20_000)?;
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
            "PAGINATION_REQUIRES_UNSUPPORTED_EXTRA_READ",
        )?;
        let mut ids = HashSet::new();
        for (i, row) in get(d, "data")
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
                    && is_safe_int(get(row, "id"))
                    && !ids.contains(&get(row, "id").as_i64().unwrap_or(-1)),
                "EXACT_ORDER_ID",
            )?;
            ids.insert(get(row, "id").as_i64().unwrap_or(-1));
        }
        Ok(vec![r])
    }
    /// getEvidence({operation='prepare_hedge',orderId=null})
    pub fn get_evidence(&self, operation: &str, order_id: Option<&str>) -> Result<V, String> {
        if operation == "reconcile" {
            self.local()?;
            let (cache, busy) = {
                let inner = self.inner.borrow();
                (inner.cache.clone().unwrap_or(V::Null), inner.busy)
            };
            need(
                sget(&cache, "kind") == Some("reconcile")
                    && (order_id.is_none()
                        || Some(order_id.unwrap_or(""))
                            == sget(get(&cache, "binding"), "orderId"))
                    && !busy,
                "RECONCILE_CACHE",
            )?;
            let at = (self.inner.borrow().deps.now)() as i64;
            self.none(&cache, at)?;
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
                self.raw(&r, sget(&r, "endpoint").unwrap_or(""), at, 20_000)?;
            }
            return Ok(json!({
                "order": get(&cache,"order"),
                "fillPages": get(&cache,"fillPages"),
                "after": get(&cache,"after"),
                "globalRisk": self.local()?,
                "graphReceipt": get(&cache,"graphReceipt"),
                "collateralReceipt": get(&cache,"collateralReceipt"),
            }));
        }
        need(
            ["prepare_hedge", "place_hedge", "hedge"].contains(&operation),
            "OPERATION",
        )?;
        if operation == "prepare_hedge" {
            need(
                (self.inner.borrow().deps.get_active_intent)()?.is_null(),
                "ACTIVE_INTENT_BEFORE_PREPARE",
            )?;
        }
        self.evidence()
    }
    /// refresh()
    pub fn refresh(&self) -> Result<V, String> {
        {
            let inner = self.inner.borrow();
            need(!inner.busy, "BUSY")?;
        }
        let (first, other, scope) = {
            let mut inner = self.inner.borrow_mut();
            inner.cache = None;
            inner.job_binding = None;
            (
                inner.first.clone(),
                inner.other.clone(),
                inner.scope.clone(),
            )
        };
        self.local()?;
        {
            let mut inner = self.inner.borrow_mut();
            inner.busy = true;
            inner.reads = 0;
            inner.last_end = None;
            inner.last_failure = None;
            inner.invocations.clear();
        }
        let body: Result<V, String> = (|| {
            let mut root_receipts = Vec::new();
            for leg in [&first, &other] {
                let receipt = self.read(&format!(
                    "/markets/{}/nodes?tournamentId={}",
                    sget(leg, "marketId").unwrap_or(""),
                    sget(&scope, "tournamentId").unwrap_or("")
                ))?;
                let mut lr = leg.as_object().cloned().unwrap_or_default();
                lr.insert("receipt".into(), receipt);
                root_receipts.push(V::Object(lr));
            }
            let cash = self.read(&format!(
                "/tournaments/{}",
                sget(&scope, "tournamentSlug").unwrap_or("")
            ))?;
            let positions = self.read(&format!(
                "/tournaments/{}/portfolio/positions",
                sget(&scope, "tournamentSlug").unwrap_or("")
            ))?;
            let open_orders_pages = self.page(&format!(
                "/orders?status=open&limit=200&tournamentId={}",
                sget(&scope, "tournamentId").unwrap_or("")
            ))?;
            let mut scoped_orders_pages_by_exchange = Map::new();
            for leg in [&first, &other] {
                let pages = self.page(&format!(
                    "/orders?status=all&exchangeId={}&limit=200&tournamentId={}",
                    sget(leg, "exchangeId").unwrap_or(""),
                    sget(&scope, "tournamentId").unwrap_or("")
                ))?;
                scoped_orders_pages_by_exchange.insert(
                    sget(leg, "exchangeId").unwrap_or("").to_string(),
                    V::Array(pages),
                );
            }
            let account = json!({
                "cash": cash,
                "positions": positions,
                "openOrdersPages": open_orders_pages,
                "scopedOrdersPagesByExchange": scoped_orders_pages_by_exchange,
            });
            let pair_census = (self.inner.borrow().deps.get_pair_census)(&json!(
                self.inner.borrow().first_exchange_id.clone()
            ))?;
            let funding = (self.inner.borrow().deps.get_funding)()?;
            let account_risk = self.local()?;
            let other_book_receipt = self.read(&format!(
                "/markets/{}/orderbook?tournamentId={}&depth=20",
                sget(&other, "marketId").unwrap_or(""),
                sget(&scope, "tournamentId").unwrap_or("")
            ))?;
            let graph_endpoint = format!(
                "/relationships/graph?marketId={}&depth=4&tournamentId={}",
                sget(&first, "marketId").unwrap_or(""),
                sget(&scope, "tournamentId").unwrap_or("")
            );
            let collateral_endpoint = format!(
                "/portfolio/collateral?tournamentId={}",
                sget(&scope, "tournamentId").unwrap_or("")
            );
            let graph_receipt = self.read(&graph_endpoint)?;
            let collateral_receipt = self.read(&collateral_endpoint)?;
            Ok(json!({
                "rootReceipts": root_receipts,
                "account": account,
                "otherBookReceipt": other_book_receipt,
                "graphReceipt": graph_receipt,
                "collateralReceipt": collateral_receipt,
                "pairCensus": pair_census,
                "funding": funding,
                "accountRisk": account_risk,
            }))
        })();
        {
            self.inner.borrow_mut().busy = false;
        }
        match body {
            Ok(cache) => {
                self.inner.borrow_mut().cache = Some(cache);
                self.evidence()
            }
            Err(e) => {
                let failure = if none_evidence_code(&e) {
                    e.clone()
                } else {
                    "NONE_HEDGE_EVIDENCE_READ_FAILED".to_string()
                };
                {
                    let mut inner = self.inner.borrow_mut();
                    inner.cache = None;
                    inner.last_failure = Some(failure.clone());
                }
                Err(failure)
            }
        }
    }
    /// refreshReconciliation(orderId)
    pub fn refresh_reconciliation(&self, order_id: &str) -> Result<V, String> {
        {
            let mut inner = self.inner.borrow_mut();
            need(!inner.busy && digit_id(order_id), "KNOWN_ORDER")?;
            inner.cache = None;
        }
        let binding = (self.inner.borrow().deps.get_order_binding)(&json!(order_id))?;
        self.inner.borrow_mut().job_binding = Some(binding.clone());
        {
            let inner = self.inner.borrow();
            need(
                sget(&binding, "orderId") == Some(order_id)
                    && get(get(&binding, "body"), "quantity") == &json!(1)
                    && sget(get(&binding, "body"), "exchangeId")
                        == sget(&inner.other, "exchangeId")
                    && sget(&binding, "bodyHash")
                        == Some(payload_hash(get(&binding, "body"))?.as_str()),
                "OWN_ORDER_BINDING",
            )?;
        }
        self.local()?;
        {
            let mut inner = self.inner.borrow_mut();
            inner.busy = true;
            inner.reads = 0;
            inner.last_end = None;
            inner.invocations.clear();
        }
        let (first, other, scope) = {
            let inner = self.inner.borrow();
            (
                inner.first.clone(),
                inner.other.clone(),
                inner.scope.clone(),
            )
        };
        let body: Result<V, String> = (|| {
            let order = self.read(&format!("/orders/{order_id}"))?;
            let fill_pages = self.page(&format!("/orders/{order_id}/fills?limit=200"))?;
            let cash = self.read(&format!(
                "/tournaments/{}",
                sget(&scope, "tournamentSlug").unwrap_or("")
            ))?;
            let positions = self.read(&format!(
                "/tournaments/{}/portfolio/positions",
                sget(&scope, "tournamentSlug").unwrap_or("")
            ))?;
            let open_orders_pages = self.page(&format!(
                "/orders?status=open&limit=200&tournamentId={}",
                sget(&scope, "tournamentId").unwrap_or("")
            ))?;
            let mut scoped_orders_pages_by_exchange = Map::new();
            for leg in [&first, &other] {
                let pages = self.page(&format!(
                    "/orders?status=all&exchangeId={}&limit=200&tournamentId={}",
                    sget(leg, "exchangeId").unwrap_or(""),
                    sget(&scope, "tournamentId").unwrap_or("")
                ))?;
                scoped_orders_pages_by_exchange.insert(
                    sget(leg, "exchangeId").unwrap_or("").to_string(),
                    V::Array(pages),
                );
            }
            let after = {
                let mut a = Map::new();
                a.insert("cash".into(), cash);
                a.insert("positions".into(), positions);
                a.insert("openOrdersPages".into(), V::Array(open_orders_pages));
                a.insert(
                    "scopedOrdersPagesByExchange".into(),
                    V::Object(scoped_orders_pages_by_exchange.clone()),
                );
                a.insert(
                    "scopedOrdersPages".into(),
                    scoped_orders_pages_by_exchange
                        .get(sget(&other, "exchangeId").unwrap_or(""))
                        .cloned()
                        .unwrap_or(V::Null),
                );
                V::Object(a)
            };
            let graph_endpoint = format!(
                "/relationships/graph?marketId={}&depth=4&tournamentId={}",
                sget(&first, "marketId").unwrap_or(""),
                sget(&scope, "tournamentId").unwrap_or("")
            );
            let collateral_endpoint = format!(
                "/portfolio/collateral?tournamentId={}",
                sget(&scope, "tournamentId").unwrap_or("")
            );
            let graph_receipt = self.read(&graph_endpoint)?;
            let collateral_receipt = self.read(&collateral_endpoint)?;
            let cache = json!({
                "kind": "reconcile",
                "binding": binding,
                "order": order,
                "fillPages": fill_pages,
                "after": after,
                "graphReceipt": graph_receipt,
                "collateralReceipt": collateral_receipt,
            });
            let at = (self.inner.borrow().deps.now)() as i64;
            self.none(&cache, at)?;
            Ok(cache)
        })();
        {
            let mut inner = self.inner.borrow_mut();
            inner.busy = false;
            inner.job_binding = None;
            match &body {
                Ok(cache) => {
                    inner.cache = Some(cache.clone());
                }
                Err(e) => {
                    inner.cache = None;
                    inner.last_failure = Some(if e.is_empty() {
                        "NONE_HEDGE_EVIDENCE_RECONCILE_FAILED".into()
                    } else {
                        e.clone()
                    });
                }
            }
        }
        let cache = body?;
        self.local()?;
        let at = (self.inner.borrow().deps.now)() as i64;
        self.none(&cache, at)?;
        Ok(json!({
            "order": get(&cache,"order"),
            "fillPages": get(&cache,"fillPages"),
            "after": get(&cache,"after"),
            "globalRisk": self.local()?,
            "graphReceipt": get(&cache,"graphReceipt"),
            "collateralReceipt": get(&cache,"collateralReceipt"),
        }))
    }
    pub fn status(&self) -> V {
        let inner = self.inner.borrow();
        json!({
            "mode": "owned_no_fresh_none_hedge_evidence_bridge_v1",
            "first": inner.first,
            "other": inner.other,
            "busy": inner.busy,
            "closed": inner.closed,
            "cached": inner.cache.is_some(),
            "maximumReads": 10,
            "lastReadCount": inner.reads,
            "lastFailure": inner.last_failure,
            "executionAuthority": false,
            "maximumPilotQuantity": 1,
            "additionalApiClients": 0,
        })
    }
    pub fn clear(&self) {
        self.inner.borrow_mut().cache = None;
    }
    pub fn close(&self) -> Result<(), String> {
        let mut inner = self.inner.borrow_mut();
        need(!inner.busy, "BUSY")?;
        inner.cache = None;
        inner.closed = true;
        inner.watermarks.clear();
        Ok(())
    }
}

/// createOwnedNoFreshNoneHedgeEvidenceBridge(options)
pub fn create_owned_no_fresh_none_hedge_evidence_bridge(
    scope: &V,
    first_exchange_id: &str,
    deps: OwnedNoFreshNoneBridgeDeps,
) -> Result<OwnedNoFreshNoneHedgeEvidenceBridge, String> {
    need(
        sget(scope, "ownerId") == Some("Codex")
            && sget(scope, "runId").is_some()
            && sget(scope, "tournamentId") == Some("bda92870-621e-47b0-bc3c-3602c5c26f55")
            && sget(scope, "tournamentSlug").is_some()
            && digit_id(first_exchange_id),
        "SCOPE",
    )?;
    let opening = (deps.get_owned)(&json!(first_exchange_id))?;
    let legs = get(get(&opening, "package"), "legs")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let first = legs
        .iter()
        .find(|e| sget(e, "exchangeId") == Some(first_exchange_id))
        .cloned()
        .unwrap_or(V::Null);
    let other = legs
        .iter()
        .find(|e| sget(e, "exchangeId") != Some(first_exchange_id))
        .cloned()
        .unwrap_or(V::Null);
    need(
        !first.is_null()
            && sget(&first, "marketId").is_some_and(digit_id)
            && !other.is_null()
            && sget(&other, "marketId").is_some_and(digit_id)
            && sget(&other, "exchangeId").is_some_and(digit_id)
            && sget(&other, "marketId") != sget(&first, "marketId")
            && get(get(&opening, "package"), "legs")
                .as_array()
                .map(|x| x.len() == 2)
                .unwrap_or(false)
            && hex(get(get(&opening, "proof"), "verificationHash"))
            && sget(get(&opening, "proof"), "exchangeId") == Some(first_exchange_id)
            && sget(get(&opening, "proof"), "marketId") == sget(&first, "marketId")
            && is_safe_int(get(get(&opening, "proof"), "remainingOwnedQuantity"))
            && get(get(&opening, "proof"), "remainingOwnedQuantity")
                .as_i64()
                .unwrap_or(0)
                > 0
            && (sget(scope, "ownedAllMarketId").is_none()
                || sget(scope, "ownedAllMarketId") == sget(&first, "marketId")),
        "ORIGINAL_SEED",
    )?;
    let original_owned_hash = payload_hash(&opening)?;
    Ok(OwnedNoFreshNoneHedgeEvidenceBridge {
        inner: Rc::new(RefCell::new(BridgeInner {
            scope: scope.clone(),
            first_exchange_id: first_exchange_id.to_string(),
            opening,
            first,
            other,
            original_owned_hash,
            permits: HashMap::new(),
            watermarks: HashMap::new(),
            invocations: HashSet::new(),
            busy: false,
            closed: false,
            cache: None,
            reads: 0,
            last_end: None,
            last_failure: None,
            job_binding: None,
            deps,
        })),
    })
}
