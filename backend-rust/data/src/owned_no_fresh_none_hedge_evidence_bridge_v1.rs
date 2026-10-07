// execution/owned-no-fresh-none-hedge-evidence-bridge-v1.mjs — sole-existing-
// client GET bridge. No POST/DELETE permit, new owner, credential or timer;
// its cached NONE receipt cannot authorize a write.
//
// Port notes:
// - JS `permits` is a WeakMap keyed by the beforeDispatch closure; in Rust the
//   dispatch boundary carries guard trait objects and the map is keyed by the
//   guard's data pointer (usize), `beforeNetworkPtr` recorded inside the
//   permit — the convention used by baal_scaled_no_evidence_bridge_v1 and the
//   live hosts.
// - `sync(f,...)` -> direct `Rc<dyn Fn>` call; a JS `undefined`/`thenable`
//   return is unrepresentable, so callbacks return Result<V,String> and their
//   errors propagate unchanged (JS sync() rethrows them).
// - `e.retryAfterMs`/`e.fatal`/`e.durableRecordingFailed` flatten into the
//   error string convention: ":fatal:" / "durableRecordingFailed" markers are
//   preserved; retryAfterMs has no channel (matches the other ported hosts).
use baal_execution::core::payload_hash;
use baal_execution::receipts::parse_number_lexemes;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::instant;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub type Guard<'a> = dyn Fn(&V, &V) -> Result<bool, String> + 'a;
pub type BridgeDispatch =
    dyn for<'g> Fn(&V, Option<&'g Guard<'g>>, Option<&'g Guard<'g>>) -> Result<V, String>;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("NONE_HEDGE_EVIDENCE_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.as_object().and_then(|m| m.get(k)).unwrap_or(&V::Null)
}
fn copy(v: &V) -> V {
    v.clone()
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn sha(v: &str) -> String {
    sha256(v.as_bytes())
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
fn instant_s(v: &V) -> Result<i64, String> {
    instant(v.as_str().ok_or_else(|| "Timestamp must be canonical UTC ISO milliseconds".to_string())?)
}
fn truthy(v: &V) -> bool {
    js_truthy(Some(v))
}
// JS `v===n` on JSON numbers only (never strings/bools).
fn num_eq(v: &V, n: f64) -> bool {
    v.is_number() && v.as_f64() == Some(n)
}
// `v?.k` — JS optional access: Null/missing -> Null.
fn opt<'a>(v: &'a V, k: &str) -> &'a V {
    get(v, k)
}
fn is_none_field(v: &V, k: &str) -> bool {
    v.as_object().and_then(|m| m.get(k)).is_none()
}

/// The caller-owned `api` object surface the bridge reads:
/// {dispatch, manualReview, cooldown}.
pub struct OwnedNoneBridgeApi {
    pub dispatch: Rc<BridgeDispatch>,
    pub manual_review: bool,
    /// JS `api.cooldown` may be any truthy/falsy value; callers store f64
    /// where NaN behaves as 0 in `(api.cooldown||0)`.
    pub cooldown: f64,
}
impl OwnedNoneBridgeApi {
    fn cooldown_or_zero(&self) -> f64 {
        if self.cooldown.is_nan() {
            0.0
        } else {
            self.cooldown
        }
    }
}

pub struct OwnedNoneBridgeDeps {
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
    pub now: Rc<dyn Fn() -> f64>,
}

pub struct OwnedNoFreshNoneHedgeEvidenceBridgeV1 {
    api: Rc<RefCell<OwnedNoneBridgeApi>>,
    receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    deps: OwnedNoneBridgeDeps,
    scope: V,
    first_exchange_id: String,
    first: V,
    other: V,
    opening: V,
    original_owned_hash: String,
    graph_endpoint: String,
    collateral_endpoint: String,
    permits: RefCell<HashMap<usize, V>>,
    watermarks: RefCell<HashMap<String, (i64, i64)>>,
    invocations: RefCell<HashSet<String>>,
    job_binding: RefCell<V>,
    busy: Cell<bool>,
    closed: Cell<bool>,
    cache: RefCell<V>,
    has_cache: Cell<bool>,
    reads: Cell<i64>,
    last_end: Cell<Option<i64>>,
    last_failure: RefCell<Option<String>>,
}

impl OwnedNoFreshNoneHedgeEvidenceBridgeV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        api: Rc<RefCell<OwnedNoneBridgeApi>>,
        receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
        scope: &V,
        first_exchange_id: &str,
        deps: OwnedNoneBridgeDeps,
    ) -> Result<Self, String> {
        // typeof checks for the JS deps list are enforced by the Rust types.
        let scope = copy(scope);
        need(
            get(&scope, "ownerId").as_str() == Some("Codex")
                && get(&scope, "runId").is_string()
                && get(&scope, "tournamentId").as_str()
                    == Some("bda92870-621e-47b0-bc3c-3602c5c26f55")
                && get(&scope, "tournamentSlug").is_string()
                && !first_exchange_id.is_empty()
                && num_id(first_exchange_id),
            "SCOPE",
        )?;
        let opening = (deps.get_owned)(first_exchange_id)?;
        let legs = get(get(&opening, "package"), "legs")
            .as_array()
            .cloned()
            .unwrap_or_default();
        let first = legs
            .iter()
            .find(|e| get(e, "exchangeId").as_str() == Some(first_exchange_id))
            .cloned()
            .unwrap_or(V::Null);
        let other = legs
            .iter()
            .find(|e| get(e, "exchangeId").as_str() != Some(first_exchange_id))
            .cloned()
            .unwrap_or(V::Null);
        need(
            truthy(&first)
                && num_id(get(&first, "marketId").as_str().unwrap_or(""))
                && truthy(&other)
                && num_id(get(&other, "marketId").as_str().unwrap_or(""))
                && num_id(get(&other, "exchangeId").as_str().unwrap_or(""))
                && get(&other, "marketId") != get(&first, "marketId")
                && legs.len() == 2
                && hex(get(get(&opening, "proof"), "verificationHash"))
                && get(get(&opening, "proof"), "exchangeId").as_str() == Some(first_exchange_id)
                && get(get(&opening, "proof"), "marketId") == get(&first, "marketId")
                && js_int(get(get(&opening, "proof"), "remainingOwnedQuantity"))
                    .is_some_and(|q| q > 0)
                && (get(&scope, "ownedAllMarketId").is_null()
                    || get(&scope, "ownedAllMarketId") == get(&first, "marketId")),
            "ORIGINAL_SEED",
        )?;
        let original_owned_hash = payload_hash(&opening)?;
        let graph_endpoint = format!(
            "/relationships/graph?marketId={}&depth=4&tournamentId={}",
            get(&first, "marketId").as_str().unwrap_or(""),
            get(&scope, "tournamentId").as_str().unwrap_or("")
        );
        let collateral_endpoint = format!(
            "/portfolio/collateral?tournamentId={}",
            get(&scope, "tournamentId").as_str().unwrap_or("")
        );
        Ok(Self {
            api,
            receipt_of,
            deps,
            scope,
            first_exchange_id: first_exchange_id.to_string(),
            first,
            other,
            opening,
            original_owned_hash,
            graph_endpoint,
            collateral_endpoint,
            permits: RefCell::new(HashMap::new()),
            watermarks: RefCell::new(HashMap::new()),
            invocations: RefCell::new(HashSet::new()),
            job_binding: RefCell::new(V::Null),
            busy: Cell::new(false),
            closed: Cell::new(false),
            cache: RefCell::new(V::Null),
            has_cache: Cell::new(false),
            reads: Cell::new(0),
            last_end: Cell::new(None),
            last_failure: RefCell::new(None),
        })
    }

    // ownPending(g)
    fn own_pending(&self, g: &V) -> Result<bool, String> {
        let o = (self.deps.get_active_intent)()?;
        if !truthy(&o) {
            return Ok(false);
        }
        let body = get(&o, "body");
        let afc = o.as_object().and_then(|m| m.get("actualFetchCount"));
        let fc = o.as_object().and_then(|m| m.get("fetchCount"));
        // `o.actualFetchCount ?? o.fetchCount` — JS ?? treats null AND absent
        // as nullish.
        let effective_fc = afc.filter(|v| !v.is_null()).or(fc).unwrap_or(&V::Null);
        let fetch_count_ok = num_eq(effective_fc, 0.0)
            && (afc.is_none() || fc.is_none() || afc == fc);
        Ok(get(&o, "intentId").is_string()
            && get(&o, "firstExchangeId").as_str() == Some(self.first_exchange_id.as_str())
            && ["prepared", "dispatching"]
                .contains(&get(&o, "phase").as_str().unwrap_or(""))
            && get(&o, "orderId").is_null()
            && fetch_count_ok
            && get(body, "side").as_str() == Some("no")
            && get(body, "action").as_str() == Some("buy")
            && num_eq(get(body, "quantity"), 1.0)
            && get(body, "exchangeId").as_str()
                == get(&self.other, "exchangeId").as_str()
            && get(body, "tournamentId") == get(&self.scope, "tournamentId")
            && get(&o, "bodyHash").as_str() == Some(payload_hash(body)?.as_str())
            && get(body, "idempotencyKey").is_string()
            && (instant_s(get(body, "expirationDate"))? as f64) > (self.deps.now)()
            && get(g, "unresolvedIntentIds")
                .as_array()
                .is_some_and(|a| a.len() == 1 && &a[0] == get(&o, "intentId")))
    }

    // cleanupOwn(g)
    fn cleanup_own(&self, g: &V) -> Result<bool, String> {
        let b = {
            let j = self.job_binding.borrow().clone();
            if j.is_null() {
                get(&self.cache.borrow().clone(), "binding").clone()
            } else {
                j
            }
        };
        if !truthy(&b) {
            return Ok(false);
        }
        let actual = (self.deps.get_order_binding)(get(&b, "orderId").as_str().unwrap_or(""))?;
        if !truthy(&actual) {
            return Ok(false);
        }
        let body = get(&actual, "body");
        Ok(get(&actual, "intentId") == get(&b, "intentId")
            && get(&actual, "orderId") == get(&b, "orderId")
            && get(&actual, "bodyHash") == get(&b, "bodyHash")
            && num_eq(get(body, "quantity"), 1.0)
            && get(body, "action").as_str() == Some("buy")
            && get(body, "side").as_str() == Some("no")
            && get(body, "exchangeId").as_str()
                == get(&self.other, "exchangeId").as_str()
            && get(body, "tournamentId") == get(&self.scope, "tournamentId")
            && get(&actual, "bodyHash").as_str() == Some(payload_hash(body)?.as_str())
            && get(g, "unresolvedIntentIds")
                .as_array()
                .ok_or_else(|| "g.unresolvedIntentIds.every is not a function".to_string())?
                .iter()
                .all(|id| id == get(&b, "intentId")))
    }

    // local()
    fn local(&self) -> Result<V, String> {
        need(!self.closed.get() && (self.deps.can_read)(), "STOPPED")?;
        let lease = (self.deps.get_account_lease)()?;
        need(
            get(&lease, "ownerId") == get(&self.scope, "ownerId")
                && get(&lease, "tournamentId") == get(&self.scope, "tournamentId")
                && hex(get(&lease, "leaseId")),
            "ACCOUNT_LEASE",
        )?;
        need(
            payload_hash(&(self.deps.get_owned)(&self.first_exchange_id)?)?
                == self.original_owned_hash,
            "OWNED_SOURCE_CHANGED",
        )?;
        let g = (self.deps.get_global_risk)()?;
        let unknown_ok = get(&g, "unknownAcceptance") == &V::Bool(false)
            || self.cleanup_own(&g)?;
        let unresolved_ok = get(&g, "unresolvedIntentIds")
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(false)
            || self.own_pending(&g)?
            || self.cleanup_own(&g)?;
        let now = (self.deps.now)();
        need(
            get(&g, "ownerId") == get(&self.scope, "ownerId")
                && get(&g, "runId") == get(&self.scope, "runId")
                && get(&g, "tournamentId") == get(&self.scope, "tournamentId")
                && get(&g, "complete") == &V::Bool(true)
                && get(&g, "eventScopeComplete") == &V::Bool(true)
                && get(&g, "soleCollector") == &V::Bool(true)
                && get(&g, "stopped") == &V::Bool(false)
                && unknown_ok
                && get(&g, "unresolvedIntentIds").is_array()
                && unresolved_ok
                && hex(get(&g, "sourceHash"))
                && (instant_s(get(&g, "at"))? as f64) <= now
                && now - instant_s(get(&g, "at"))? as f64 <= 20000.0,
            "UNKNOWN_OR_GLOBAL_SOURCE",
        )?;
        need(
            !self.api.borrow().manual_review && now >= self.api.borrow().cooldown_or_zero(),
            "SHARED_BACKOFF",
        )?;
        Ok(g)
    }

    // raw(r,endpoint,at,age=20000)
    fn raw(&self, r: &V, endpoint: &str, at: f64, age: i64) -> Result<V, String> {
        need(
            r.is_object()
                && r.as_object().unwrap().keys().all(|k| {
                    [
                        "method",
                        "endpoint",
                        "status",
                        "requestBodyHash",
                        "bodyText",
                        "sourceBodyHash",
                        "fetchInvocationId",
                        "requestStartedAt",
                        "receivedAt",
                    ]
                    .contains(&k.as_str())
                }),
            "RAW_FIELD_SCOPE",
        )?;
        need(
            get(r, "method").as_str() == Some("GET")
                && get(r, "endpoint").as_str() == Some(endpoint)
                && js_int(get(r, "status")) == Some(200)
                && get(r, "requestBodyHash").is_null()
                && get(r, "bodyText").is_string()
                && get(r, "bodyText").as_str().unwrap_or("").as_bytes().len() <= 8 * 1024 * 1024
                && get(r, "sourceBodyHash").as_str()
                    == Some(sha(get(r, "bodyText").as_str().unwrap_or("")).as_str())
                && get(r, "fetchInvocationId").as_str().is_some_and(|s| !s.is_empty()),
            "RAW_SOURCE",
        )?;
        let start = instant_s(get(r, "requestStartedAt"))?;
        let end = instant_s(get(r, "receivedAt"))?;
        need(
            start <= end && (end as f64) <= at && at - start as f64 <= age as f64,
            "SOURCE_CLOCK",
        )?;
        let (data, numbers) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
        Ok(json!({"data": data, "numbers": numbers}))
    }

    // none(v,at)
    fn none(&self, v: &V, at: f64) -> Result<V, String> {
        let graph_raw = self.raw(get(v, "graphReceipt"), &self.graph_endpoint, at, 5000)?;
        let graph = get(&graph_raw, "data");
        let coll_raw = self.raw(get(v, "collateralReceipt"), &self.collateral_endpoint, at, 5000)?;
        let collateral = get(&coll_raw, "data");
        need(
            graph.is_object()
                && {
                    let mut keys: Vec<&str> =
                        graph.as_object().unwrap().keys().map(|k| k.as_str()).collect();
                    keys.sort();
                    keys.join("|") == "nodes|relationships"
                }
                && get(graph, "relationships").as_array().is_some_and(|a| a.is_empty())
                && get(graph, "nodes").as_array().is_some_and(|a| a.len() <= 2),
            "NON_NONE_OR_UNSUPPORTED_GRAPH",
        )?;
        let mut ids: HashSet<String> = HashSet::new();
        for n in get(graph, "nodes").as_array().cloned().unwrap_or_default() {
            need(
                n.is_object()
                    && get(&n, "exchangeId").is_string()
                    && !ids.contains(get(&n, "exchangeId").as_str().unwrap_or(""))
                    && [&self.first, &self.other].iter().any(|e| {
                        get(e, "exchangeId") == get(&n, "exchangeId")
                            && get(e, "marketId") == get(&n, "marketId")
                    }),
                "FOREIGN_GRAPH_NODE",
            )?;
            ids.insert(get(&n, "exchangeId").as_str().unwrap_or("").to_string());
        }
        need(
            collateral.is_object()
                && collateral
                    .as_object()
                    .map(|m| m.keys().cloned().collect::<Vec<_>>().join("|") == "data")
                    .unwrap_or(false)
                && get(collateral, "data").as_array().is_some_and(|a| a.is_empty()),
            "NON_NONE_COLLATERAL",
        )?;
        let other_start = {
            let ob = opt(v, "otherBookReceipt");
            if !ob.is_null() {
                instant_s(get(ob, "receivedAt"))?
            } else {
                // v.after?.scopedOrdersPagesByExchange[other.exchangeId].at(-1)
                // .receivedAt — a miss yields Null whose instant() throws like
                // JS instant(undefined).
                let by_ex = get(get(v, "after"), "scopedOrdersPagesByExchange");
                let arr = by_ex
                    .get(get(&self.other, "exchangeId").as_str().unwrap_or(""))
                    .and_then(|x| x.as_array())
                    .cloned()
                    .unwrap_or_default();
                instant_s(get(arr.last().unwrap_or(&V::Null), "receivedAt"))?
            }
        };
        need(
            instant_s(get(get(v, "collateralReceipt"), "requestStartedAt"))?
                >= instant_s(get(get(v, "graphReceipt"), "receivedAt"))?
                && instant_s(get(get(v, "graphReceipt"), "requestStartedAt"))? >= other_start,
            "NONE_CAUSALITY",
        )?;
        Ok(json!({
            "schemaVersion": 1,
            "mode": "fresh_owned_pair_none_read_confirmation_v1",
            "graphReceipt": copy(get(v, "graphReceipt")),
            "collateralReceipt": copy(get(v, "collateralReceipt")),
            "sourceHash": payload_hash(&json!({
                "graph": get(get(v, "graphReceipt"), "sourceBodyHash"),
                "collateral": get(get(v, "collateralReceipt"), "sourceBodyHash"),
            }))?,
            "confirmedAt": get(get(v, "collateralReceipt"), "receivedAt"),
            "maximumAgeMs": 5000,
            "executionAuthority": false,
        }))
    }

    // samePendingEconomics(before,current)
    fn same_pending_economics(&self, before: &V, current: &V) -> Result<bool, String> {
        let strip = |r: &V| -> V {
            let mut m = r.as_object().cloned().unwrap_or_default();
            m.remove("reservationHash");
            m.remove("unresolvedIntentIds");
            V::Object(m)
        };
        let hash_valid = |r: &V| -> Result<bool, String> {
            let mut m = r.as_object().cloned().unwrap_or_default();
            let rh = m.remove("reservationHash").unwrap_or(V::Null);
            Ok(hex(&rh) && payload_hash(&V::Object(m))? == rh.as_str().unwrap_or(""))
        };
        let o = (self.deps.get_active_intent)()?;
        Ok(hash_valid(before)?
            && hash_valid(current)?
            && get(before, "unresolvedIntentIds")
                .as_array()
                .is_some_and(|a| a.is_empty())
            && get(current, "unresolvedIntentIds")
                .as_array()
                .is_some_and(|a| a.len() == 1 && &a[0] == get(&o, "intentId"))
            && same(&strip(before), &strip(current))?)
    }

    // evidence()
    fn evidence(&self) -> Result<V, String> {
        self.local()?;
        need(self.has_cache.get() && !self.busy.get(), "CACHE_UNAVAILABLE")?;
        let current = self.local()?;
        let cache = self.cache.borrow().clone();
        let c = get(&cache, "pairCensus").clone();
        let f = get(&cache, "funding").clone();
        let g = get(&cache, "accountRisk").clone();
        let live_census = (self.deps.get_pair_census)(&self.first_exchange_id)?;
        let live_funding = (self.deps.get_funding)()?;
        let at = (self.deps.now)();
        let all_none_evidence = self.none(&cache, at)?;
        let funding_ok = same(&live_funding, &f)?
            && get(&current, "globalLedgerHash") == get(&g, "globalLedgerHash")
            && get(&current, "controllerReservationHash") == get(&g, "controllerReservationHash")
            || self.own_pending(&current)?
                && self.same_pending_economics(get(&f, "record"), get(&live_funding, "record"))?
                && get(get(&live_funding, "receipt"), "reservationHash")
                    == get(get(&live_funding, "record"), "reservationHash")
                && get(get(&live_funding, "receipt"), "allocationHash")
                    == get(get(&live_funding, "record"), "allocationHash")
                && get(&current, "controllerReservationHash")
                    == get(get(&live_funding, "record"), "reservationHash")
                && get(&current, "globalLedgerHash")
                    == get(get(&live_funding, "receipt"), "globalLedgerHash")
                && same(get(&current, "reservations"), get(&g, "reservations"))?;
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
        let mut result = Map::new();
        result.insert("scope".into(), copy(&self.scope));
        result.insert("firstExchangeId".into(), json!(self.first_exchange_id));
        result.insert("owned".into(), copy(&self.opening));
        result.insert("pairCensus".into(), c.clone());
        result.insert("funding".into(), f.clone());
        result.insert("globalRisk".into(), g.clone());
        result.insert("account".into(), copy(get(&cache, "account")));
        result.insert("rootReceipts".into(), copy(get(&cache, "rootReceipts")));
        result.insert("otherBookReceipt".into(), copy(get(&cache, "otherBookReceipt")));
        result.insert("now".into(), json!(iso(at)));
        let result_v = V::Object(result);
        let proposal = crate::baal_scaled_owned_no_hedge_proposal_v1::plan_baal_owned_no_hedge(
            &result_v,
        );
        need(
            get(&proposal, "proposalReady") == &V::Bool(true),
            &format!("PROPOSAL_{}", get(&proposal, "reason").as_str().unwrap_or("")),
        )?;
        let mut out = result_v.as_object().cloned().unwrap_or_default();
        out.insert("graphReceipt".into(), copy(get(&cache, "graphReceipt")));
        out.insert("collateralReceipt".into(), copy(get(&cache, "collateralReceipt")));
        out.insert("allNoneEvidence".into(), all_none_evidence);
        out.insert("maximumPilotQuantity".into(), json!(1));
        Ok(V::Object(out))
    }

    // authorizes(spec) — spec carries the guard identity tokens; JS compares
    // the actual closures. With the S-guard boundary this takes the real
    // guard trait objects and the pointer-keyed permit map.

    /// authorizes by guard-pointer values (see authorizes).
    pub fn authorizes_ptrs(&self, spec: &V, bd_ptr: usize, bn_ptr: usize) -> bool {
        (|| -> Result<bool, String> {
            self.local()?;
            let p = self.permits.borrow().get(&bd_ptr).cloned();
            let p = match p {
                Some(p) => p,
                None => return Ok(false),
            };
            Ok(get(spec, "method").as_str() == Some("GET")
                && get(spec, "endpoint") == get(&p, "endpoint")
                && (bn_ptr as u64) == get(&p, "beforeNetworkPtr").as_u64().unwrap_or(0)
                && is_none_field(spec, "body")
                && is_none_field(spec, "bodyJson")
                && is_none_field(spec, "headers"))
        })()
        .unwrap_or(false)
    }

    pub fn authorizes(&self, spec: &V, before_dispatch: &Guard, before_network: &Guard) -> bool {
        (|| -> Result<bool, String> {
            self.local()?;
            let p = self
                .permits
                .borrow()
                .get(&(before_dispatch as *const Guard as *const () as usize))
                .cloned();
            let p = match p {
                Some(p) => p,
                None => return Ok(false),
            };
            Ok(get(spec, "method").as_str() == Some("GET")
                && get(spec, "endpoint") == get(&p, "endpoint")
                && (before_network as *const Guard as *const () as usize)
                    == get(&p, "beforeNetworkPtr").as_u64().unwrap_or(0) as usize
                && is_none_field(spec, "body")
                && is_none_field(spec, "bodyJson")
                && is_none_field(spec, "headers"))
        })()
        .unwrap_or(false)
    }

    // read(endpoint)
    fn read(&self, endpoint: &str) -> Result<V, String> {
        self.local()?;
        need(
            self.reads.get() < 10 && (self.deps.has_read_headroom)(10 - self.reads.get())?,
            "HEADROOM",
        )?;
        let admitted = Rc::new(Cell::new(false));
        let network = Rc::new(Cell::new(false));
        let this = self;
        let (admitted_c, network_c) = (admitted.clone(), network.clone());
        let before_dispatch = move |_p: &V, _s: &V| -> Result<bool, String> {
            let admitted = &admitted_c;
            this.local()?;
            need(
                !admitted.get() && (this.deps.has_read_headroom)(10 - this.reads.get())?,
                "ADMISSION",
            )?;
            admitted.set(true);
            Ok(true)
        };
        let (admitted_n, network_n) = (admitted.clone(), network.clone());
        let before_network = move |_p: &V, _s: &V| -> Result<bool, String> {
            let admitted = &admitted_n;
            let network = &network_n;
            this.local()?;
            need(
                admitted.get() && !network.get() && (this.deps.has_read_headroom)(9 - this.reads.get())?,
                "NETWORK",
            )?;
            network.set(true);
            Ok(true)
        };
        let spec = json!({"method": "GET", "endpoint": endpoint});
        let bd_ptr = &before_dispatch as &Guard as *const Guard as *const () as usize;
        let bn_ptr = &before_network as &Guard as *const Guard as *const () as usize;
        self.permits.borrow_mut().insert(
            bd_ptr,
            json!({"endpoint": endpoint, "beforeNetworkPtr": bn_ptr as u64}),
        );
        let start = (self.deps.now)();
        let result: Result<V, String> = (|| {
            let response = (self.api.borrow().dispatch)(
                &spec,
                Some(&before_dispatch as &Guard),
                Some(&before_network as &Guard),
            )?;
            self.local()?;
            need(
                admitted.get()
                    && network.get()
                    && get(&response, "endpoint").as_str() == Some(endpoint)
                    && num_eq(get(&response, "status"), 200.0),
                "RESPONSE",
            )?;
            let r = (self.receipt_of)(&response, &json!({"method": "GET"}))?;
            let age = if endpoint.contains("/nodes?") { 300000 } else { 20000 };
            let parsed = self.raw(&r, endpoint, (self.deps.now)(), age)?;
            need(
                (instant_s(get(&r, "requestStartedAt"))? as f64) >= start
                    && instant_s(get(&r, "requestStartedAt"))?
                        >= instant_s(get(&response, "requestStartedAt"))?
                    && instant_s(get(&r, "receivedAt"))?
                        <= instant_s(get(&response, "receivedAt"))?
                    && same(get(&parsed, "data"), get(&response, "data"))?
                    && (self.last_end.get().is_none()
                        || instant_s(get(&r, "requestStartedAt"))? >= self.last_end.get().unwrap()),
                "SOURCE_CAUSALITY",
            )?;
            need(
                !self
                    .invocations
                    .borrow()
                    .contains(get(&r, "fetchInvocationId").as_str().unwrap_or("")),
                "DUPLICATE_INVOCATION",
            )?;
            self.invocations
                .borrow_mut()
                .insert(get(&r, "fetchInvocationId").as_str().unwrap_or("").to_string());
            let old = self.watermarks.borrow().get(endpoint).copied();
            let r_start = instant_s(get(&r, "requestStartedAt"))?;
            let r_end = instant_s(get(&r, "receivedAt"))?;
            need(
                old.map(|o| r_start >= o.0 && r_end >= o.1).unwrap_or(true),
                "RECEIPT_ROLLBACK",
            )?;
            self.watermarks
                .borrow_mut()
                .insert(endpoint.to_string(), (r_start, r_end));
            self.last_end.set(Some(r_end));
            Ok(copy(&r))
        })();
        self.reads.set(self.reads.get() + 1);
        self.permits.borrow_mut().remove(&bd_ptr);
        result
    }

    // page(endpoint)
    fn page(&self, endpoint: &str) -> Result<Vec<V>, String> {
        let r = self.read(endpoint)?;
        let parsed = self.raw(&r, endpoint, (self.deps.now)(), 20000)?;
        let d = get(&parsed, "data");
        let coverage_seq = get(get(d, "coverage"), "projectedThroughSequence");
        need(
            get(d, "data").as_array().is_some_and(|a| a.len() <= 200)
                && num_eq(get(get(d, "pagination"), "limit"), 200.0)
                && get(get(d, "pagination"), "hasMore") == &V::Bool(false)
                && get(get(d, "pagination"), "nextCursor").is_null()
                && get(get(d, "coverage"), "complete") == &V::Bool(true)
                && (coverage_seq.is_null()
                    || js_int(coverage_seq).is_some_and(|x| x >= 0)),
            "PAGINATION_REQUIRES_UNSUPPORTED_EXTRA_READ",
        )?;
        let mut ids: HashSet<i64> = HashSet::new();
        for (i, row) in get(d, "data").as_array().unwrap().iter().enumerate() {
            let key = js_stringify(&json!(["data", i, "id"]));
            let n = get(&parsed, "numbers").get(&key).cloned().unwrap_or(V::Null);
            need(
                n.as_str().is_some_and(num_id)
                    && js_int(get(row, "id")).is_some()
                    && !ids.contains(&js_int(get(row, "id")).unwrap_or(0)),
                "EXACT_ORDER_ID",
            )?;
            ids.insert(js_int(get(row, "id")).unwrap());
        }
        Ok(vec![r])
    }

    /// getEvidence({operation='prepare_hedge', orderId=null}={})
    pub fn get_evidence(&self, input: &V) -> Result<V, String> {
        let operation = get(input, "operation")
            .as_str()
            .unwrap_or("prepare_hedge")
            .to_string();
        let order_id = get(input, "orderId");
        if operation == "reconcile" {
            self.local()?;
            let cache = self.cache.borrow().clone();
            need(
                get(&cache, "kind").as_str() == Some("reconcile")
                    && (order_id.is_null()
                        || order_id == get(get(&cache, "binding"), "orderId"))
                    && !self.busy.get(),
                "RECONCILE_CACHE",
            )?;
            let at = (self.deps.now)();
            self.none(&cache, at)?;
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
                self.raw(r, get(r, "endpoint").as_str().unwrap_or(""), at, 20000)?;
            }
            return Ok(json!({
                "order": copy(get(&cache, "order")),
                "fillPages": copy(get(&cache, "fillPages")),
                "after": copy(get(&cache, "after")),
                "globalRisk": self.local()?,
                "graphReceipt": copy(get(&cache, "graphReceipt")),
                "collateralReceipt": copy(get(&cache, "collateralReceipt")),
            }));
        }
        need(
            ["prepare_hedge", "place_hedge", "hedge"].contains(&operation.as_str()),
            "OPERATION",
        )?;
        if operation == "prepare_hedge" {
            need(
                (self.deps.get_active_intent)()?.is_null(),
                "ACTIVE_INTENT_BEFORE_PREPARE",
            )?;
        }
        self.evidence()
    }

    /// refresh()
    pub fn refresh(&self) -> Result<V, String> {
        need(!self.busy.get(), "BUSY")?;
        *self.cache.borrow_mut() = V::Null;
        self.has_cache.set(false);
        *self.job_binding.borrow_mut() = V::Null;
        self.local()?;
        self.busy.set(true);
        self.reads.set(0);
        self.last_end.set(None);
        *self.last_failure.borrow_mut() = None;
        self.invocations.borrow_mut().clear();
        let tid = get(&self.scope, "tournamentId").as_str().unwrap_or("").to_string();
        let slug = get(&self.scope, "tournamentSlug").as_str().unwrap_or("").to_string();
        let run = |this: &Self| -> Result<V, String> {
            let mut root_receipts: Vec<V> = Vec::new();
            for leg in [&this.first, &this.other] {
                let receipt = this.read(&format!(
                    "/markets/{}/nodes?tournamentId={}",
                    get(leg, "marketId").as_str().unwrap_or(""),
                    tid
                ))?;
                let mut l = leg.as_object().cloned().unwrap_or_default();
                l.insert("receipt".into(), receipt);
                root_receipts.push(V::Object(l));
            }
            let cash = this.read(&format!("/tournaments/{slug}"))?;
            let positions = this.read(&format!("/tournaments/{slug}/portfolio/positions"))?;
            let open_orders_pages =
                this.page(&format!("/orders?status=open&limit=200&tournamentId={tid}"))?;
            let mut scoped_pages = Map::new();
            for leg in [&this.first, &this.other] {
                let ex = get(leg, "exchangeId").as_str().unwrap_or("").to_string();
                let pages = this.page(&format!(
                    "/orders?status=all&exchangeId={ex}&limit=200&tournamentId={tid}"
                ))?;
                scoped_pages.insert(ex, json!(pages));
            }
            let account = json!({
                "cash": cash,
                "positions": positions,
                "openOrdersPages": open_orders_pages,
                "scopedOrdersPagesByExchange": V::Object(scoped_pages),
            });
            let pair_census = (this.deps.get_pair_census)(&this.first_exchange_id)?;
            let funding = (this.deps.get_funding)()?;
            let account_risk = this.local()?;
            let other_book = this.read(&format!(
                "/markets/{}/orderbook?tournamentId={}&depth=20",
                get(&this.other, "marketId").as_str().unwrap_or(""),
                tid
            ))?;
            let graph = this.read(&this.graph_endpoint)?;
            let collateral = this.read(&this.collateral_endpoint)?;
            *this.cache.borrow_mut() = json!({
                "rootReceipts": root_receipts,
                "account": account,
                "otherBookReceipt": other_book,
                "graphReceipt": graph,
                "collateralReceipt": collateral,
                "pairCensus": pair_census,
                "funding": funding,
                "accountRisk": account_risk,
            });
            this.has_cache.set(true);
            Ok(V::Null)
        };
        let result = run(self);
        if let Err(e) = &result {
            *self.cache.borrow_mut() = V::Null;
            self.has_cache.set(false);
            let code = {
                // /^NONE_HEDGE_EVIDENCE_[A-Z_]+$/.test(e.code ?? '')
                let ok = e.len() > "NONE_HEDGE_EVIDENCE_".len()
                    && e.starts_with("NONE_HEDGE_EVIDENCE_")
                    && e["NONE_HEDGE_EVIDENCE_".len()..]
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b == b'_');
                if ok {
                    e.clone()
                } else {
                    "NONE_HEDGE_EVIDENCE_READ_FAILED".to_string()
                }
            };
            *self.last_failure.borrow_mut() = Some(code.clone());
            self.busy.set(false);
            return Err(code);
        }
        self.busy.set(false);
        self.evidence()
    }

    /// refreshReconciliation(orderId)
    pub fn refresh_reconciliation(&self, order_id: &str) -> Result<V, String> {
        need(!self.busy.get() && num_id(order_id), "KNOWN_ORDER")?;
        *self.cache.borrow_mut() = V::Null;
        self.has_cache.set(false);
        let binding = (self.deps.get_order_binding)(order_id)?;
        *self.job_binding.borrow_mut() = binding.clone();
        need(
            get(&binding, "orderId").as_str() == Some(order_id)
                && num_eq(get(get(&binding, "body"), "quantity"), 1.0)
                && get(get(&binding, "body"), "exchangeId").as_str()
                    == get(&self.other, "exchangeId").as_str()
                && get(&binding, "bodyHash").as_str()
                    == Some(payload_hash(get(&binding, "body"))?.as_str()),
            "OWN_ORDER_BINDING",
        )?;
        self.local()?;
        self.busy.set(true);
        self.reads.set(0);
        self.last_end.set(None);
        self.invocations.borrow_mut().clear();
        let tid = get(&self.scope, "tournamentId").as_str().unwrap_or("").to_string();
        let slug = get(&self.scope, "tournamentSlug").as_str().unwrap_or("").to_string();
        let run = |this: &Self| -> Result<(), String> {
            let order = this.read(&format!("/orders/{order_id}"))?;
            let fill_pages = this.page(&format!("/orders/{order_id}/fills?limit=200"))?;
            let cash = this.read(&format!("/tournaments/{slug}"))?;
            let positions = this.read(&format!("/tournaments/{slug}/portfolio/positions"))?;
            let open_orders_pages =
                this.page(&format!("/orders?status=open&limit=200&tournamentId={tid}"))?;
            let mut scoped_pages = Map::new();
            for leg in [&this.first, &this.other] {
                let ex = get(leg, "exchangeId").as_str().unwrap_or("").to_string();
                let pages = this.page(&format!(
                    "/orders?status=all&exchangeId={ex}&limit=200&tournamentId={tid}"
                ))?;
                scoped_pages.insert(ex, json!(pages));
            }
            let scoped_other = scoped_pages
                .get(get(&this.other, "exchangeId").as_str().unwrap_or(""))
                .cloned()
                .unwrap_or(V::Null);
            let after = json!({
                "cash": cash,
                "positions": positions,
                "openOrdersPages": open_orders_pages,
                "scopedOrdersPagesByExchange": V::Object(scoped_pages),
                "scopedOrdersPages": scoped_other,
            });
            let graph = this.read(&this.graph_endpoint)?;
            let collateral = this.read(&this.collateral_endpoint)?;
            *this.cache.borrow_mut() = json!({
                "kind": "reconcile",
                "binding": copy(&binding),
                "order": order,
                "fillPages": fill_pages,
                "after": after,
                "graphReceipt": graph,
                "collateralReceipt": collateral,
            });
            this.has_cache.set(true);
            let at = (this.deps.now)();
            this.none(&this.cache.borrow().clone(), at)?;
            Ok(())
        };
        let result = run(self);
        self.busy.set(false);
        *self.job_binding.borrow_mut() = V::Null;
        if let Err(e) = &result {
            *self.cache.borrow_mut() = V::Null;
            self.has_cache.set(false);
            let code = if e.is_empty() {
                "NONE_HEDGE_EVIDENCE_RECONCILE_FAILED".to_string()
            } else {
                e.clone()
            };
            *self.last_failure.borrow_mut() = Some(code.clone());
            return Err(code);
        }
        self.local()?;
        let cache = self.cache.borrow().clone();
        self.none(&cache, (self.deps.now)())?;
        Ok(json!({
            "order": copy(get(&cache, "order")),
            "fillPages": copy(get(&cache, "fillPages")),
            "after": copy(get(&cache, "after")),
            "globalRisk": self.local()?,
            "graphReceipt": copy(get(&cache, "graphReceipt")),
            "collateralReceipt": copy(get(&cache, "collateralReceipt")),
        }))
    }

    /// status()
    pub fn status(&self) -> V {
        json!({
            "mode": "owned_no_fresh_none_hedge_evidence_bridge_v1",
            "first": copy(&self.first),
            "other": copy(&self.other),
            "busy": self.busy.get(),
            "closed": self.closed.get(),
            "cached": self.has_cache.get(),
            "maximumReads": 10,
            "lastReadCount": self.reads.get(),
            "lastFailure": self.last_failure.borrow().clone(),
            "executionAuthority": false,
            "maximumPilotQuantity": 1,
            "additionalApiClients": 0,
        })
    }

    /// clear()
    pub fn clear(&self) {
        *self.cache.borrow_mut() = V::Null;
        self.has_cache.set(false);
    }

    /// close()
    pub fn close(&self) -> Result<(), String> {
        need(!self.busy.get(), "BUSY")?;
        *self.cache.borrow_mut() = V::Null;
        self.has_cache.set(false);
        self.closed.set(true);
        self.watermarks.borrow_mut().clear();
        Ok(())
    }
}

/// createOwnedNoFreshNoneHedgeEvidenceBridge({api,receiptOf,scope,
/// firstExchangeId,getOwned,getPairCensus,getFunding,getGlobalRisk,
/// getAccountLease,canRead,hasReadHeadroom,getActiveIntent,getOrderBinding,now})
pub fn create_owned_no_fresh_none_hedge_evidence_bridge(
    api: Rc<RefCell<OwnedNoneBridgeApi>>,
    receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    scope: &V,
    first_exchange_id: &str,
    deps: OwnedNoneBridgeDeps,
) -> Result<OwnedNoFreshNoneHedgeEvidenceBridgeV1, String> {
    OwnedNoFreshNoneHedgeEvidenceBridgeV1::new(api, receipt_of, scope, first_exchange_id, deps)
}
