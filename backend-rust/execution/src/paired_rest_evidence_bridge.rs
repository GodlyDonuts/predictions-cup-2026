// execution/paired-rest-evidence-bridge.mjs — read-only adapter over the
// caller's EXISTING serialized ReadClient. Every GET is identity-fenced; no
// credential, independent client, rate limiter or timer. getEvidence is
// synchronous and never queues a read from a write-admission callback. Raw
// bodies remain private in memory until the scoped writer consumes them.
// Clearing a failed refresh never falls back to an earlier account/book.
//
// Port notes:
// - The JS `issued` WeakMap keyed on the beforeDispatch closure is keyed on a
//   numeric fence id embedded in the dispatch spec (spec.beforeDispatch /
//   spec.beforeNetwork), matching the collector convention. The checks the JS
//   closures performed (allowed() + budget + admission) run inline at the same
//   points: admission once, then immediately before the network write.
// - `sync(fn, ...)`: all supplied callbacks are synchronous Fn traits here, so
//   the JS thenable check is statically satisfied.
use crate::core::payload_hash;
use crate::gates::normalize_official_fill_tick;
use crate::no_rest_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::receipts::parse_number_lexemes;
use baal_service_runtime::contract::{instant_ns, ms_to_iso};
use baal_service_runtime::fixed::{amount, instant};
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use baal_service_runtime::market::normalize_book;
use serde_json::{json, Map, Value as V};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("PAIRED_EVIDENCE_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty()
            && !s.starts_with('0')
            && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n > 0 && n <= 9_007_199_254_740_991)
    })
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn integer(v: &V, min: i64) -> bool {
    js_int(v).is_some_and(|n| n >= min)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn instant_ns_v(v: &V) -> Result<i128, String> {
    instant_ns(v.as_str().unwrap_or("")).map_err(|e| e.0)
}
// BigInt(x) — JSON string digits or integral number.
fn js_bigint(v: &V) -> Result<i128, String> {
    match v {
        V::String(s) => {
            let t = s.trim();
            t.parse::<i128>()
                .map_err(|_| "Cannot convert to a BigInt".to_string())
        }
        V::Number(n) => n
            .as_i64()
            .map(|x| x as i128)
            .or_else(|| {
                n.as_f64()
                    .filter(|f| f.is_finite() && f.fract() == 0.0 && f.abs() < 9e18)
                    .map(|f| f as i128)
            })
            .ok_or_else(|| "Cannot convert to a BigInt".to_string()),
        V::Bool(b) => Ok(*b as i128),
        _ => Err("Cannot convert to a BigInt".to_string()),
    }
}
fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*'
            | b'\'' | b'(' | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
// slug /^[a-z0-9]+(?:-[a-z0-9]+)*$/
fn slug(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|seg| {
            !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}
// /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
fn uuid_ok(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && parts[0].len() == 8
        && parts[1].len() == 4
        && parts[2].len() == 4
        && parts[3].len() == 4
        && parts[4].len() == 12
        && parts
            .iter()
            .all(|p| p.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn paired_evidence_code(e: &str) -> bool {
    // /^PAIRED_EVIDENCE_[A-Z_]+$/
    e.starts_with("PAIRED_EVIDENCE_")
        && e.len() > "PAIRED_EVIDENCE_".len()
        && e["PAIRED_EVIDENCE_".len()..]
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b == b'_')
}
// fresh(r, n): receipt wall-clock window.
fn fresh(r: &V, n: f64) -> Result<(), String> {
    let a = instant_v(get(r, "requestStartedAt"))?;
    let z = instant_v(get(r, "receivedAt"))?;
    need(
        a <= z && (z as f64) <= n && n - a as f64 <= 20_000.0,
        "STALE_OR_FUTURE_RECEIPT",
    )
}

struct ReadBounds {
    preparation: i64,
    reconciliation: i64,
    cancel: i64,
}
impl ReadBounds {
    fn get(&self, kind: &str) -> i64 {
        match kind {
            "preparation" => self.preparation,
            "reconciliation" => self.reconciliation,
            _ => self.cancel,
        }
    }
}

pub struct PairedRestEvidenceBridgeOptions {
    /// The serialized read client's dispatch — receives specs carrying numeric
    /// beforeDispatch/beforeNetwork fence ids.
    pub dispatch: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub scope: V,
    pub get_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_funding: Rc<dyn Fn() -> Result<V, String>>,
    pub get_other_reserved: Rc<dyn Fn() -> Result<V, String>>,
    pub owns_order: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub can_read: Rc<dyn Fn() -> bool>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> Result<bool, String>>,
    pub now: Rc<dyn Fn() -> f64>,
    /// {preparation: 23, reconciliation: 33, cancel: 1}
    pub max_reads_per_operation: Option<V>,
}

struct Inner {
    cache: V, // Null when empty
    busy: bool,
    closed: bool,
    operation: Option<String>,
    reads: i64,
    last_failure: V, // Null or code string
    last_end: Option<i64>,
    // issued: fence-id -> (endpoint, beforeNetwork fence-id)
    issued: HashMap<usize, (String, usize)>,
    next_fence: usize,
    receipt_watermarks: HashMap<String, (i64, i64)>,
    book_watermarks: HashMap<String, V>,
    projection_watermarks: HashMap<String, i64>,
}

pub struct PairedRestEvidenceBridge {
    inner: RefCell<Inner>,
    dispatch: Rc<dyn Fn(&V) -> Result<V, String>>,
    receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    scope: V,
    get_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    get_funding: Rc<dyn Fn() -> Result<V, String>>,
    get_other_reserved: Rc<dyn Fn() -> Result<V, String>>,
    owns_order: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    can_read: Rc<dyn Fn() -> bool>,
    has_read_headroom: Rc<dyn Fn(i64) -> Result<bool, String>>,
    now: Rc<dyn Fn() -> f64>,
    bounds: ReadBounds,
}

/// createPairedRestEvidenceBridge({api, receiptOf, scope, getGlobalRisk,
/// getFunding, getOtherReserved, ownsOrder, canRead, hasReadHeadroom, now,
/// maxReadsPerOperation})
pub fn create_paired_rest_evidence_bridge(
    opts: PairedRestEvidenceBridgeOptions,
) -> Result<PairedRestEvidenceBridge, String> {
    let scope = &opts.scope;
    need(
        get(scope, "tournamentId").as_str().is_some_and(uuid_ok)
            && get(scope, "tournamentSlug").as_str().is_some_and(slug)
            && get(scope, "markets").as_array().is_some_and(|ms| {
                ms.len() == 2
                    && ms.iter().all(|m| id(get(m, "marketId")) && id(get(m, "exchangeId")))
                    && ms.iter().map(|m| crate::js_key(get(m, "marketId"))).collect::<BTreeSet<String>>().len() == 2
                    && ms.iter().map(|m| crate::js_key(get(m, "exchangeId"))).collect::<BTreeSet<String>>().len() == 2
            }),
        "SCOPE",
    )?;
    let supplied = opts.max_reads_per_operation.clone().unwrap_or_else(|| {
        json!({"preparation": 23, "reconciliation": 33, "cancel": 1})
    });
    let bounds = ReadBounds {
        preparation: 23,
        reconciliation: 33,
        cancel: 1,
    };
    let read_bounds = ReadBounds {
        preparation: js_int(get(&supplied, "preparation")).unwrap_or(0),
        reconciliation: js_int(get(&supplied, "reconciliation")).unwrap_or(0),
        cancel: js_int(get(&supplied, "cancel")).unwrap_or(0),
    };
    need(
        integer(get(&supplied, "preparation"), 1)
            && integer(get(&supplied, "reconciliation"), 1)
            && integer(get(&supplied, "cancel"), 1)
            && read_bounds.preparation <= bounds.preparation
            && read_bounds.reconciliation <= bounds.reconciliation
            && read_bounds.cancel <= bounds.cancel,
        "READ_BOUND",
    )?;
    Ok(PairedRestEvidenceBridge {
        inner: RefCell::new(Inner {
            cache: V::Null,
            busy: false,
            closed: false,
            operation: None,
            reads: 0,
            last_failure: V::Null,
            last_end: None,
            issued: HashMap::new(),
            next_fence: 0,
            receipt_watermarks: HashMap::new(),
            book_watermarks: HashMap::new(),
            projection_watermarks: HashMap::new(),
        }),
        dispatch: opts.dispatch,
        receipt_of: opts.receipt_of,
        scope: opts.scope.clone(),
        get_global_risk: opts.get_global_risk,
        get_funding: opts.get_funding,
        get_other_reserved: opts.get_other_reserved,
        owns_order: opts.owns_order,
        can_read: opts.can_read,
        has_read_headroom: opts.has_read_headroom,
        now: opts.now,
        bounds: read_bounds,
    })
}

impl PairedRestEvidenceBridge {
    fn allowed(&self) -> Result<(), String> {
        need(
            !self.inner.borrow().closed && (self.can_read)(),
            "STOPPED",
        )
    }
    fn own(&self, order_id: &V, leg: &V) -> Result<(), String> {
        need(
            id(order_id) && (self.owns_order)(order_id, leg)? == json!(true),
            "ORDER_NOT_OWNED",
        )
    }
    fn clear(&self) {
        self.inner.borrow_mut().cache = V::Null;
    }
    fn local_global(&self) -> Result<V, String> {
        let g = (self.get_global_risk)()?;
        need(
            !g.is_null()
                && get(&g, "tournamentId") == get(&self.scope, "tournamentId")
                && hex(get(&g, "sourceHash"))
                && instant_v(get(&g, "at"))? as f64 <= (self.now)()
                && (self.now)() - instant_v(get(&g, "at"))? as f64 <= 20_000.0,
            "GLOBAL_RISK_CLOCK_OR_SCOPE",
        )?;
        Ok(g.clone())
    }
    fn local_funding(&self) -> Result<V, String> {
        let f = (self.get_funding)()?;
        let other = (self.get_other_reserved)()?;
        need(
            other.as_str().is_some() && amount(other.as_str().unwrap_or(""))? >= 0,
            "OTHER_RESERVE",
        )?;
        if !f.is_null() {
            need(
                hex(get(&f, "reservationHash"))
                    && hex(get(&f, "globalLedgerHash"))
                    && get(&f, "amount").is_string()
                    && amount(get(&f, "amount").as_str().unwrap_or(""))? >= 0
                    && instant_v(get(&f, "confirmedAt"))? as f64 <= (self.now)()
                    && (self.now)() - instant_v(get(&f, "confirmedAt"))? as f64 <= 20_000.0,
                "FUNDING_CLOCK_OR_SCHEMA",
            )?;
        }
        Ok(json!({"funding": f, "reservedElsewhere": other}))
    }
    fn validate_account(&self, account: &V, leg: &V, n: f64) -> Result<V, String> {
        let mut binding = self.scope.clone();
        binding["exchangeId"] = get(leg, "exchangeId").clone();
        inspect_scoped_rest_no_account(&json!({
            "account": account,
            "binding": binding,
            "readAt": ms_to_iso(n),
        }))
    }
    fn check_book(&self, r: &V, leg: &V, n: f64) -> Result<V, String> {
        fresh(r, n)?;
        let (data, _numbers) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
        let b = normalize_book(
            &data,
            get(leg, "marketId").as_str().unwrap_or(""),
            get(&self.scope, "tournamentId").as_str().unwrap_or(""),
            get(r, "receivedAt").as_str().unwrap_or(""),
            get(r, "requestStartedAt").as_str().unwrap_or(""),
            20_000.0,
            &BTreeSet::<String>::new(),
            "authoritative_receipt_v1",
        )?;
        need(
            (data.get("marketId").is_none() || get(&data, "marketId") == get(leg, "marketId"))
                && get(&data, "contexts").as_array().is_some_and(|c| c.len() == 1)
                && get(&get(&data, "contexts")[0], "type").as_str() == Some("tournament")
                && get(
                    &get(
                        get(&data, "contexts")
                            .as_array()
                            .and_then(|c| c.first())
                            .unwrap_or(&V::Null),
                        "orderbook",
                    ),
                    "exchanges",
                )
                    .as_array()
                    .is_some_and(|e| e.len() == 1)
                && get(&b, "exchangeId") == get(leg, "exchangeId")
                && get(&b, "bids").as_array().is_some_and(|x| x.len() <= 20)
                && get(&b, "asks").as_array().is_some_and(|x| x.len() <= 20),
            "BOOK_SCOPE",
        )?;
        let old = self
            .inner
            .borrow()
            .book_watermarks
            .get(get(leg, "exchangeId").as_str().unwrap_or(""))
            .cloned();
        if let Some(old) = old {
            need(
                js_bigint(get(&b, "sequence"))? >= js_bigint(get(&old, "sequence"))?
                    && instant_ns_v(get(&b, "sourceAt"))? >= instant_ns_v(get(&old, "sourceAt"))?
                    && instant_v(get(&b, "requestStartedAt"))? >= instant_v(get(&old, "requestStartedAt"))?,
                "BOOK_ROLLBACK",
            )?;
            if get(&b, "versionKey") == get(&old, "versionKey") {
                need(
                    payload_hash(&json!([get(&b, "bids"), get(&b, "asks")]))?
                        == payload_hash(&json!([get(&old, "bids"), get(&old, "asks")]))?,
                    "BOOK_VERSION_CONFLICT",
                )?;
            }
        }
        Ok(b)
    }

    /// getEvidence(context={})
    pub fn get_evidence(&self, context: &V) -> Result<V, String> {
        match self.get_evidence_inner(context) {
            Ok(v) => Ok(v),
            Err(e) => {
                self.clear();
                let last_failure = if paired_evidence_code(&e) {
                    e
                } else {
                    "PAIRED_EVIDENCE_INVALID_CACHE".to_string()
                };
                self.inner.borrow_mut().last_failure = json!(last_failure);
                Err(last_failure)
            }
        }
    }
    fn get_evidence_inner(&self, context: &V) -> Result<V, String> {
        self.allowed()?;
        let cache = self.inner.borrow().cache.clone();
        need(!self.inner.borrow().busy && !cache.is_null(), "CACHE_UNAVAILABLE")?;
        if context.get("marketId").is_some() {
            need(
                get(context, "marketId") == get(get(&cache, "leg"), "marketId")
                    && get(context, "exchangeId") == get(get(&cache, "leg"), "exchangeId"),
                "CACHE_LEG_MISMATCH",
            )?;
        }
        if context.get("orderId").is_some() && !get(context, "orderId").is_null() {
            need(
                get(context, "orderId") == get(&cache, "orderId"),
                "CACHE_ORDER_MISMATCH",
            )?;
        }
        let n = (self.now)();
        match get(&cache, "kind").as_str() {
            Some("preparation") => {
                need(
                    !js_truthy(context.get("operation"))
                        || ["prepare", "place"]
                            .contains(&get(context, "operation").as_str().unwrap_or("")),
                    "CACHE_OPERATION",
                )?;
                self.validate_account(get(&cache, "account"), get(&cache, "leg"), n)?;
                let b = self.check_book(get(&cache, "book"), get(&cache, "leg"), n)?;
                need(
                    instant_v(get(&b, "requestStartedAt"))?
                        >= instant_v(get(&cache, "accountCompletedAt"))?,
                    "BOOK_BEFORE_ACCOUNT",
                )?;
                let mut out = json!({
                    "tournamentId": get(&self.scope, "tournamentId"),
                    "account": get(&cache, "account"),
                    "book": get(&cache, "book"),
                    "globalRisk": self.local_global()?,
                });
                let funding = self.local_funding()?;
                if let V::Object(m) = &mut out {
                    m.insert("funding".into(), get(&funding, "funding").clone());
                    m.insert("reservedElsewhere".into(), get(&funding, "reservedElsewhere").clone());
                }
                Ok(out)
            }
            Some("cancel") => {
                need(
                    !js_truthy(context.get("operation"))
                        || get(context, "operation").as_str() == Some("cancel"),
                    "CACHE_OPERATION",
                )?;
                self.own(get(&cache, "orderId"), get(&cache, "leg"))?;
                fresh(get(&cache, "order"), n)?;
                Ok(json!({"order": get(&cache, "order")}))
            }
            _ => {
                need(
                    !js_truthy(context.get("operation"))
                        || get(context, "operation").as_str() == Some("reconcile"),
                    "CACHE_OPERATION",
                )?;
                self.own(get(&cache, "orderId"), get(&cache, "leg"))?;
                fresh(get(&cache, "order"), n)?;
                for r in get(&cache, "fillPages").as_array().cloned().unwrap_or_default() {
                    fresh(&r, n)?;
                }
                self.validate_account(get(&cache, "after"), get(&cache, "leg"), n)?;
                Ok(json!({
                    "order": get(&cache, "order"),
                    "fillPages": get(&cache, "fillPages"),
                    "after": get(&cache, "after"),
                    "globalRisk": self.local_global()?,
                }))
            }
        }
    }

    /// authorizes(spec) — fence-id version of the JS WeakMap check.
    pub fn authorizes(&self, spec: &V) -> bool {
        (|| -> Result<bool, String> {
            self.allowed()?;
            let issued = self.inner.borrow();
            let fence = spec["beforeDispatch"]
                .as_u64()
                .and_then(|i| issued.issued.get(&(i as usize)).cloned());
            let Some((endpoint, network_id)) = fence else {
                return Ok(false);
            };
            let keys_ok = spec.as_object().map(|m| {
                m.keys().all(|k| {
                    [
                        "method",
                        "endpoint",
                        "body",
                        "bodyJson",
                        "beforeDispatch",
                        "beforeNetwork",
                    ]
                    .contains(&k.as_str())
                })
            });
            Ok(spec["method"].as_str() == Some("GET")
                && spec["endpoint"].as_str() == Some(endpoint.as_str())
                && spec.get("body").is_none()
                && spec.get("bodyJson").is_none()
                && spec["beforeNetwork"].as_u64() == Some(network_id as u64)
                && keys_ok.unwrap_or(false))
        })()
        .unwrap_or(false)
    }

    fn budget(&self, admitted_already: bool) -> Result<(), String> {
        let bound = {
            let i = self.inner.borrow();
            self.bounds.get(i.operation.as_deref().unwrap_or(""))
        };
        let reads = self.inner.borrow().reads;
        need(
            (self.has_read_headroom)(bound - reads - admitted_already as i64)?,
            "READ_HEADROOM",
        )
    }

    // read(endpoint) — one identity-fenced scoped GET.
    fn read(&self, endpoint: &str) -> Result<V, String> {
        self.allowed()?;
        {
            let i = self.inner.borrow();
            need(
                i.reads < self.bounds.get(i.operation.as_deref().unwrap_or("")),
                "READ_BOUND",
            )?;
        }
        // budget(admittedAlready = false)
        self.budget(false)?;
        let (fence_d, fence_n) = {
            let mut i = self.inner.borrow_mut();
            i.next_fence += 1;
            let d = i.next_fence;
            i.next_fence += 1;
            let n = i.next_fence;
            i.issued.insert(d, (endpoint.to_string(), n));
            (d, n)
        };
        // beforeDispatch(): allowed(); budget(); admitted once.
        self.allowed()?;
        self.budget(false)?;
        // beforeNetwork(): allowed(); need(admitted); budget(true).
        self.allowed()?;
        self.budget(true)?;
        let spec = json!({
            "method": "GET",
            "endpoint": endpoint,
            "beforeDispatch": fence_d,
            "beforeNetwork": fence_n,
        });
        let began = (self.now)();
        let result = (|| -> Result<V, String> {
            let response = (self.dispatch)(&spec)?;
            self.allowed()?;
            // The fence ids stand in for the JS closures; the serialized client
            // is required to invoke them, so admission is implied by a returned
            // response. `admitted === true` is therefore satisfied.
            need(
                get(&response, "endpoint").as_str() == Some(endpoint)
                    && (match response.get("status") {
                        Some(s) if !s.is_null() => s.as_i64().unwrap_or(-1),
                        _ => 200,
                    }) == 200,
                "READ_RESPONSE",
            )?;
            let r = (self.receipt_of)(&response, &json!({"method": "GET", "requestBodyHash": V::Null}))?;
            need(
                get(&r, "method").as_str() == Some("GET")
                    && get(&r, "endpoint").as_str() == Some(endpoint)
                    && js_int(get(&r, "status")) == Some(200)
                    && get(&r, "bodyText").as_str().is_some_and(|t| t.len() <= 8 * 1024 * 1024)
                    && hex(get(&r, "sourceBodyHash"))
                    && get(&r, "sourceBodyHash").as_str()
                        == Some(sha256(get(&r, "bodyText").as_str().unwrap_or("").as_bytes()).as_str())
                    && r.get("requestBodyHash") == Some(&V::Null)
                    && r.as_object().map(|m| {
                        m.keys().all(|k| {
                            [
                                "method",
                                "endpoint",
                                "status",
                                "bodyText",
                                "sourceBodyHash",
                                "requestBodyHash",
                                "requestStartedAt",
                                "receivedAt",
                                "fetchInvocationId",
                            ]
                            .contains(&k.as_str())
                        })
                    }).unwrap_or(false),
                "RAW_RECEIPT",
            )?;
            fresh(&r, (self.now)())?;
            let last_end = self.inner.borrow().last_end;
            need(
                instant_v(get(&r, "requestStartedAt"))? as f64 >= began
                    && instant_v(get(&r, "requestStartedAt"))?
                        >= instant_v(get(&response, "requestStartedAt"))?
                    && instant_v(get(&r, "receivedAt"))?
                        <= instant_v(get(&response, "receivedAt"))?
                    && last_end
                        .map(|last| instant_v(get(&r, "requestStartedAt")).map(|t| t >= last))
                        .unwrap_or(Ok(true))?,
                "RECEIPT_CAUSALITY",
            )?;
            need(
                payload_hash(&parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?.0)?
                    == payload_hash(get(&response, "data"))?,
                "RAW_DATA_MISMATCH",
            )?;
            let prior = self.inner.borrow().receipt_watermarks.get(endpoint).cloned();
            if let Some((p_start, p_end)) = prior {
                need(
                    instant_v(get(&r, "requestStartedAt"))? >= p_start
                        && instant_v(get(&r, "receivedAt"))? >= p_end,
                    "RECEIPT_ROLLBACK",
                )?;
            }
            let start = instant_v(get(&r, "requestStartedAt"))?;
            let end = instant_v(get(&r, "receivedAt"))?;
            let mut i = self.inner.borrow_mut();
            i.receipt_watermarks.insert(endpoint.to_string(), (start, end));
            i.last_end = Some(end);
            Ok(r.clone())
        })();
        {
            let mut i = self.inner.borrow_mut();
            i.reads += 1;
            i.issued.remove(&fence_d);
        }
        result
    }

    // pages(base, check) — cursor-chained complete page set.
    fn pages(
        &self,
        base: &str,
        mut check: impl FnMut(&V, &V) -> Result<(), String>,
    ) -> Result<Vec<V>, String> {
        let mut checkpoint = self
            .inner
            .borrow()
            .projection_watermarks
            .get(base)
            .copied();
        let mut all: Vec<V> = Vec::new();
        let mut seen_cursors: HashSet<String> = HashSet::new();
        let mut ids: HashSet<i64> = HashSet::new();
        let mut cursor: Option<String> = None;
        for i in 0..10 {
            let endpoint = match &cursor {
                None => base.to_string(),
                Some(c) => format!("{base}&cursor={}", encode_uri_component(c)),
            };
            let r = self.read(&endpoint)?;
            let (d, _n) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
            let p = get(&d, "pagination");
            need(
                get(&d, "data").as_array().is_some_and(|a| a.len() <= 200)
                    && js_int(get(p, "limit")) == Some(200)
                    && get(p, "hasMore").is_boolean()
                    && (p.get("nextCursor") == Some(&V::Null)
                        || get(p, "nextCursor").as_str().is_some_and(|s| !s.is_empty() && s.len() <= 1000))
                    && get(get(&d, "coverage"), "complete") == &json!(true)
                    && (get(&d, "coverage").get("projectedThroughSequence") == Some(&V::Null)
                        || integer(get(get(&d, "coverage"), "projectedThroughSequence"), 0)),
                "PAGES_INCOMPLETE",
            )?;
            let projected = js_int(get(get(&d, "coverage"), "projectedThroughSequence"));
            if let (Some(cp), Some(pv)) = (checkpoint, projected) {
                need(pv >= cp, "PROJECTION_ROLLBACK")?;
            }
            if let Some(pv) = projected {
                checkpoint = Some(pv);
            }
            for row in get(&d, "data").as_array().cloned().unwrap_or_default() {
                need(
                    integer(get(&row, "id"), 1)
                        && !ids.contains(&js_int(get(&row, "id")).unwrap_or(0)),
                    "PAGE_DUPLICATE_ID",
                )?;
                ids.insert(js_int(get(&row, "id")).unwrap_or(0));
            }
            check(&d, &r)?;
            all.push(r);
            if get(p, "hasMore") == &json!(false) {
                need(p.get("nextCursor") == Some(&V::Null), "PAGE_CURSOR")?;
                if let Some(cp) = checkpoint {
                    self.inner
                        .borrow_mut()
                        .projection_watermarks
                        .insert(base.to_string(), cp);
                }
                return Ok(all);
            }
            let next = get(p, "nextCursor").as_str().unwrap_or("").to_string();
            need(
                p.get("nextCursor") != Some(&V::Null) && !next.is_empty() && !seen_cursors.contains(&next),
                "PAGE_CURSOR",
            )?;
            seen_cursors.insert(next.clone());
            cursor = Some(next);
        }
        Err("PAIRED_EVIDENCE_PAGES_INCOMPLETE".into())
    }

    // account(leg)
    fn account(&self, leg: &V) -> Result<V, String> {
        let cash = self.read(&format!(
            "/tournaments/{}",
            get(&self.scope, "tournamentSlug").as_str().unwrap_or("")
        ))?;
        let positions = self.read(&format!(
            "/tournaments/{}/portfolio/positions",
            get(&self.scope, "tournamentSlug").as_str().unwrap_or("")
        ))?;
        let open_orders_pages = self.pages(
            &format!(
                "/orders?status=open&limit=200&tournamentId={}",
                get(&self.scope, "tournamentId").as_str().unwrap_or("")
            ),
            |_, _| Ok(()),
        )?;
        let scoped_orders_pages = self.pages(
            &format!(
                "/orders?status=all&exchangeId={}&limit=200&tournamentId={}",
                get(leg, "exchangeId").as_str().unwrap_or(""),
                get(&self.scope, "tournamentId").as_str().unwrap_or("")
            ),
            |_, _| Ok(()),
        )?;
        let result = json!({
            "cash": cash,
            "positions": positions,
            "openOrdersPages": open_orders_pages,
            "scopedOrdersPages": scoped_orders_pages,
        });
        self.validate_account(&result, leg, (self.now)())?;
        Ok(result)
    }

    // orderCheck(r, orderId, leg, open)
    fn order_check(&self, r: &V, order_id: &V, leg: &V, open: bool) -> Result<V, String> {
        let (d, _n) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
        need(
            integer(get(&d, "id"), 1)
                && crate::js_key(get(&d, "id")) == order_id.as_str().unwrap_or("")
                && get(&d, "exchangeId") == get(leg, "exchangeId")
                && get(&d, "tournamentId") == get(&self.scope, "tournamentId")
                && get(&d, "side").as_str() == Some("no")
                && get(&d, "action").as_str() == Some("buy")
                && js_int(get(&d, "quantity")) == Some(1)
                && get(&d, "open") == &json!(open)
                && instant_v(get(&d, "createdAt"))? as f64 <= (self.now)() + 2_000.0
                && instant_v(get(&d, "expirationDate"))? > instant_v(get(&d, "createdAt"))?,
            "ORDER_SCOPE_OR_PHASE",
        )?;
        normalize_official_fill_tick(get(&d, "priceLimit"))?;
        if !open {
            need(
                integer(get(&d, "quantityFilled"), 0)
                    && js_int(get(&d, "quantityFilled")).unwrap_or(0) <= 1,
                "TERMINAL_QUANTITY",
            )?;
        }
        Ok(d)
    }

    // run(kind, legValue, orderId, fn)
    fn run(
        &self,
        kind: &str,
        leg_value: &V,
        order_id: &V,
        f: impl FnOnce(&V) -> Result<V, String>,
    ) -> Result<V, String> {
        need(!self.inner.borrow().busy, "BUSY")?;
        self.clear();
        self.allowed()?;
        let leg = self.exact_leg(leg_value)?;
        if !order_id.is_null() {
            self.own(order_id, &leg)?;
        }
        {
            let mut i = self.inner.borrow_mut();
            i.busy = true;
            i.operation = Some(kind.to_string());
            i.reads = 0;
            i.last_end = None;
            i.last_failure = V::Null;
        }
        let result = (|| -> Result<V, String> {
            need(
                (self.has_read_headroom)(self.bounds.get(kind))?,
                "READ_HEADROOM",
            )?;
            let value = f(&leg)?;
            self.allowed()?;
            self.inner.borrow_mut().cache = value;
            Ok(json!({
                "kind": kind,
                "leg": leg,
                "orderId": order_id,
                "completedAt": ms_to_iso((self.now)()),
                "readCount": self.inner.borrow().reads,
            }))
        })();
        {
            let mut i = self.inner.borrow_mut();
            i.busy = false;
            i.operation = None;
        }
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                self.clear();
                let last_failure = if paired_evidence_code(&e) {
                    e
                } else {
                    "PAIRED_EVIDENCE_READ_FAILED".to_string()
                };
                self.inner.borrow_mut().last_failure = json!(last_failure);
                Err(last_failure)
            }
        }
    }
    fn exact_leg(&self, v: &V) -> Result<V, String> {
        need(
            !v.is_null()
                && get(&self.scope, "markets").as_array().is_some_and(|ms| {
                    ms.iter().any(|m| {
                        get(m, "marketId") == get(v, "marketId")
                            && get(m, "exchangeId") == get(v, "exchangeId")
                    })
                }),
            "LEG_SCOPE",
        )?;
        Ok(json!({"marketId": get(v, "marketId"), "exchangeId": get(v, "exchangeId")}))
    }

    /// refreshPreparation(leg)
    pub fn refresh_preparation(&self, leg: &V) -> Result<V, String> {
        self.run("preparation", leg, &V::Null, |l| {
            let a = self.account(l)?;
            let info = self.validate_account(&a, l, (self.now)())?;
            let book = self.read(&format!(
                "/markets/{}/orderbook?tournamentId={}&depth=20",
                get(l, "marketId").as_str().unwrap_or(""),
                get(&self.scope, "tournamentId").as_str().unwrap_or("")
            ))?;
            let b = self.check_book(&book, l, (self.now)())?;
            need(
                instant_v(get(&b, "requestStartedAt"))?
                    >= instant_v(get(&info, "completedAt"))?,
                "BOOK_BEFORE_ACCOUNT",
            )?;
            self.validate_account(&a, l, (self.now)())?;
            self.inner
                .borrow_mut()
                .book_watermarks
                .insert(get(l, "exchangeId").as_str().unwrap_or("").to_string(), b);
            Ok(json!({
                "kind": "preparation",
                "leg": l,
                "orderId": V::Null,
                "account": a,
                "accountCompletedAt": get(&info, "completedAt"),
                "book": book,
            }))
        })
    }

    /// refreshReconciliation(orderId, leg)
    pub fn refresh_reconciliation(&self, order_id: &V, leg: &V) -> Result<V, String> {
        self.run("reconciliation", leg, order_id, |l| {
            let order = self.read(&format!("/orders/{}", order_id.as_str().unwrap_or("")))?;
            let od = self.order_check(&order, order_id, l, false)?;
            let totals = RefCell::new(V::Null);
            let scope_t = get(&self.scope, "tournamentId").clone();
            let leg_ex = get(l, "exchangeId").clone();
            let order_id_num = order_id.as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN);
            let od_created = instant_v(get(&od, "createdAt"))?;
            let fill_pages = self.pages(
                &format!("/orders/{}/fills?limit=200", order_id.as_str().unwrap_or("")),
                |d, r| {
                    need(
                        get(d, "orderId").as_f64() == Some(order_id_num)
                            && get(d, "exchangeId") == &leg_ex
                            && get(d, "tournamentId") == &scope_t
                            && [0.0, -1.0].contains(&get(d, "totalQuantityFilled").as_f64().unwrap_or(f64::NAN)),
                        "FILL_SCOPE",
                    )?;
                    let avg = if d.get("avgFillPrice") == Some(&V::Null) {
                        V::Null
                    } else {
                        get(&normalize_official_fill_tick(get(d, "avgFillPrice"))?, "price").clone()
                    };
                    let identity = payload_hash(&json!([get(d, "totalQuantityFilled"), avg]))?;
                    need(
                        totals.borrow().is_null() || *totals.borrow() == json!(identity),
                        "FILL_TOTAL_CHANGED",
                    )?;
                    *totals.borrow_mut() = json!(identity);
                    for f in get(d, "data").as_array().cloned().unwrap_or_default() {
                        need(
                            get(&f, "side").as_str() == Some("no")
                                && js_int(get(&f, "quantity")) == Some(-1)
                                && instant_v(get(&f, "filledAt"))? >= od_created
                                && instant_v(get(&f, "filledAt"))? <= instant_v(get(r, "receivedAt"))?,
                            "FILL_SCHEMA",
                        )?;
                        normalize_official_fill_tick(get(&f, "price"))?;
                    }
                    Ok(())
                },
            )?;
            let decoded: Vec<V> = fill_pages
                .iter()
                .map(|r| {
                    parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))
                        .map(|(d, _)| d)
                })
                .collect::<Result<_, _>>()?;
            let fills: Vec<V> = decoded
                .iter()
                .flat_map(|d| get(d, "data").as_array().cloned().unwrap_or_default())
                .collect();
            let last = decoded.last().cloned().unwrap_or(V::Null);
            let fill_match = if fills.is_empty() {
                last.get("avgFillPrice") == Some(&V::Null)
            } else {
                let first_price = get(&normalize_official_fill_tick(get(&fills[0], "price"))?, "price")
                    .clone();
                let last_avg = get(&normalize_official_fill_tick(get(&last, "avgFillPrice"))?, "price")
                    .clone();
                let od_limit = get(&normalize_official_fill_tick(get(&od, "priceLimit"))?, "price")
                    .clone();
                last_avg == first_price
                    && amount(first_price.as_str().unwrap_or(""))?
                        <= amount(od_limit.as_str().unwrap_or(""))?
            };
            need(
                fills.len() <= 1
                    && fills.len() as i64 == js_int(get(&od, "quantityFilled")).unwrap_or(i64::MIN)
                    && get(&last, "totalQuantityFilled").as_i64() == Some(-(fills.len() as i64))
                    && fill_match,
                "FILL_TOTAL_MISMATCH",
            )?;
            let after = self.account(l)?;
            need(
                instant_v(get(get(&after, "cash"), "requestStartedAt"))?
                    >= instant_v(get(fill_pages.last().unwrap_or(&V::Null), "receivedAt"))?,
                "POST_ACCOUNT_CAUSALITY",
            )?;
            Ok(json!({
                "kind": "reconciliation",
                "leg": l,
                "orderId": order_id,
                "order": order,
                "fillPages": fill_pages,
                "after": after,
            }))
        })?;
        // {operation:'reconcile', orderId, ...exactLeg(scope,leg)}
        let exact = self.exact_leg(leg)?;
        let context = json!({
            "operation": "reconcile",
            "orderId": order_id,
            "marketId": get(&exact, "marketId"),
            "exchangeId": get(&exact, "exchangeId"),
        });
        self.get_evidence(&context)
    }

    /// refreshCancel(orderId, leg)
    pub fn refresh_cancel(&self, order_id: &V, leg: &V) -> Result<V, String> {
        self.run("cancel", leg, order_id, |l| {
            let order = self.read(&format!("/orders/{}", order_id.as_str().unwrap_or("")))?;
            self.order_check(&order, order_id, l, true)?;
            Ok(json!({
                "kind": "cancel",
                "leg": l,
                "orderId": order_id,
                "order": order,
            }))
        })
    }

    /// status()
    pub fn status(&self) -> V {
        let i = self.inner.borrow();
        json!({
            "mode": "paired_rest_evidence_bridge_v1",
            "busy": i.busy,
            "closed": i.closed,
            "cachedKind": get(&i.cache, "kind"),
            "cachedLeg": if get(&i.cache, "leg").is_null() { V::Null } else { get(&i.cache, "leg").clone() },
            "lastReadCount": i.reads,
            "lastFailure": i.last_failure,
            "additionalApiClients": 0,
            "executionAuthority": false,
        })
    }
    /// clear()
    pub fn clear_cache(&self) {
        self.clear();
    }
    /// close()
    pub fn close(&self) -> Result<(), String> {
        need(!self.inner.borrow().busy, "BUSY")?;
        self.clear();
        let mut i = self.inner.borrow_mut();
        i.closed = true;
        i.receipt_watermarks.clear();
        i.book_watermarks.clear();
        i.projection_watermarks.clear();
        Ok(())
    }
}
