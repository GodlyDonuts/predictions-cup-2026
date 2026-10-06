// execution/paired-rest-evidence-bridge.mjs — read-only adapter over the
// caller's EXISTING serialized ReadClient. Every GET is identity-fenced; no
// credential, independent client, rate limiter or timer. getEvidence is
// synchronous and never queues a read from a write-admission callback. Raw
// bodies remain private in memory until the scoped writer consumes them.
// Clearing a failed refresh never falls back to an earlier account/book.
use crate::core::payload_hash;
use crate::gates::normalize_official_fill_tick;
use crate::no_rest_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::receipts::parse_number_lexemes;
use baal_service_runtime::contract::instant_ns;
use baal_service_runtime::fixed::{amount, instant};
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use baal_service_runtime::market::normalize_book;
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("PAIRED_EVIDENCE_{code}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n > 0 && n <= 9_007_199_254_740_991)
    })
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn iso(n: f64) -> String {
    baal_service_runtime::contract::ms_to_iso(n)
}
fn integer(v: &V, min: i64) -> bool {
    js_int(v).is_some_and(|x| x >= min)
}
fn fresh(r: &V, n: i64) -> Result<(), String> {
    let a = instant(get(r, "requestStartedAt").as_str().unwrap_or("")).map_err(|_| need_err("STALE_OR_FUTURE_RECEIPT"))?;
    let z = instant(get(r, "receivedAt").as_str().unwrap_or("")).map_err(|_| need_err("STALE_OR_FUTURE_RECEIPT"))?;
    need(a <= z && z <= n && n - a <= 20_000, "STALE_OR_FUTURE_RECEIPT")
}
fn need_err(c: &str) -> String {
    format!("PAIRED_EVIDENCE_{c}")
}
fn exact_leg(scope: &V, v: &V) -> Result<V, String> {
    need(
        !v.is_null()
            && get(scope, "markets").as_array().is_some_and(|ms| {
                ms.iter().any(|m| {
                    get(m, "marketId") == get(v, "marketId") && get(m, "exchangeId") == get(v, "exchangeId")
                })
            }),
        "LEG_SCOPE",
    )?;
    Ok(json!({"marketId": get(v, "marketId"), "exchangeId": get(v, "exchangeId")}))
}

type Guard<'a> = dyn Fn(&V, &V) -> Result<bool, String> + 'a;
type DispatchFn = dyn for<'g> Fn(&V, Option<&'g Guard<'g>>, Option<&'g Guard<'g>>) -> Result<V, String>;

pub struct EvidenceBridgeDeps {
    pub dispatch: Rc<DispatchFn>,
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub get_global_risk: Rc<dyn Fn() -> V>,
    pub get_funding: Rc<dyn Fn() -> V>,
    pub get_other_reserved: Rc<dyn Fn() -> V>,
    pub owns_order: Rc<dyn Fn(&V, &V) -> bool>,
    pub can_read: Rc<dyn Fn() -> bool>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> bool>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct Watermark {
    sequence: V,
    source_at: V,
    request_started_at: V,
    version_key: V,
    bids: V,
    asks: V,
}

struct Inner {
    scope: V,
    deps: EvidenceBridgeDeps,
    max_reads_per_operation: V,
    bounds: BTreeMap<String, i64>,
    cache: Option<V>,
    busy: bool,
    closed: bool,
    operation: Option<String>,
    reads: i64,
    last_failure: Option<String>,
    last_end: Option<i64>,
    issued: HashMap<usize, V>,
    receipt_watermarks: BTreeMap<String, (i64, i64)>,
    book_watermarks: BTreeMap<String, Watermark>,
    projection_watermarks: BTreeMap<String, i64>,
}

pub struct PairedRestEvidenceBridge {
    inner: Rc<RefCell<Inner>>,
}

// createPairedRestEvidenceBridge({api,receiptOf,scope,getGlobalRisk,getFunding,getOtherReserved,ownsOrder,canRead,hasReadHeadroom,now,maxReadsPerOperation})
pub fn create_paired_rest_evidence_bridge(
    deps: EvidenceBridgeDeps,
    scope: &V,
    max_reads_per_operation: Option<&V>,
) -> Result<PairedRestEvidenceBridge, String> {
    need(
        get(scope, "tournamentId").as_str().is_some_and(|s| {
            s.len() == 36
                && s.bytes().enumerate().all(|(i, b)| {
                    if [8, 13, 18, 23].contains(&i) { b == b'-' } else { b.is_ascii_hexdigit() }
                })
        })
            && get(scope, "tournamentSlug").as_str().is_some_and(|s| {
                !s.is_empty()
                    && s.split('-').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
            })
            && get(scope, "markets").as_array().is_some_and(|ms| {
                ms.len() == 2
                    && ms.iter().all(|m| id(get(m, "marketId")) && id(get(m, "exchangeId")))
                    && ms.iter().map(|m| get(m, "marketId").as_str().unwrap_or("")).collect::<BTreeSet<_>>().len() == 2
                    && ms.iter().map(|m| get(m, "exchangeId").as_str().unwrap_or("")).collect::<BTreeSet<_>>().len() == 2
            }),
        "SCOPE",
    )?;
    let bounds: BTreeMap<String, i64> = BTreeMap::from([
        ("preparation".into(), 23),
        ("reconciliation".into(), 33),
        ("cancel".into(), 1),
    ]);
    let default_bounds = json!({"preparation": 23, "reconciliation": 33, "cancel": 1});
    let mro = max_reads_per_operation.unwrap_or(&default_bounds).clone();
    for k in bounds.keys() {
        need(
            integer(&get(&mro, k), 1) && js_int(get(&mro, k)).unwrap() <= bounds[k],
            "READ_BOUND",
        )?;
    }
    Ok(PairedRestEvidenceBridge {
        inner: Rc::new(RefCell::new(Inner {
            scope: scope.clone(),
            deps,
            max_reads_per_operation: mro,
            bounds,
            cache: None,
            busy: false,
            closed: false,
            operation: None,
            reads: 0,
            last_failure: None,
            last_end: None,
            issued: HashMap::new(),
            receipt_watermarks: BTreeMap::new(),
            book_watermarks: BTreeMap::new(),
            projection_watermarks: BTreeMap::new(),
        })),
    })
}

impl Inner {
    fn allowed(&self) -> Result<(), String> {
        need(!self.closed && (self.deps.can_read)(), "STOPPED")
    }
    fn own(&self, order_id: &V, leg: &V) -> Result<(), String> {
        need(id(order_id) && (self.deps.owns_order)(order_id, leg), "ORDER_NOT_OWNED")
    }
    fn local_global(&self) -> Result<V, String> {
        let g = (self.deps.get_global_risk)().clone();
        need(
            !g.is_null()
                && get(&g, "tournamentId") == get(&self.scope, "tournamentId")
                && hex(get(&g, "sourceHash"))
                && instant(get(&g, "at").as_str().unwrap_or("")).map_err(|_| need_err("GLOBAL_RISK_CLOCK_OR_SCOPE"))? <= (self.deps.now)() as i64
                && (self.deps.now)() as i64 - instant(get(&g, "at").as_str().unwrap_or("")).unwrap() <= 20_000,
            "GLOBAL_RISK_CLOCK_OR_SCOPE",
        )?;
        Ok(g)
    }
    fn local_funding(&self) -> Result<(V, V), String> {
        let f = (self.deps.get_funding)().clone();
        let other = (self.deps.get_other_reserved)().clone();
        need(
            other.as_str().is_some_and(|s| amount(s).is_ok_and(|x| x >= 0)),
            "OTHER_RESERVE",
        )?;
        if !f.is_null() {
            need(
                hex(get(&f, "reservationHash"))
                    && hex(get(&f, "globalLedgerHash"))
                    && get(&f, "amount").as_str().is_some_and(|s| amount(s).is_ok_and(|x| x >= 0))
                    && instant(get(&f, "confirmedAt").as_str().unwrap_or("")).map_err(|_| need_err("FUNDING_CLOCK_OR_SCHEMA"))? <= (self.deps.now)() as i64
                    && (self.deps.now)() as i64 - instant(get(&f, "confirmedAt").as_str().unwrap_or("")).unwrap() <= 20_000,
                "FUNDING_CLOCK_OR_SCHEMA",
            )?;
        }
        Ok((if f.is_null() { V::Null } else { f }, other))
    }
    fn validate_account(&self, account: &V, leg: &V, n: i64) -> Result<V, String> {
        let mut binding = self.scope.as_object().cloned().unwrap_or_default();
        binding.insert("exchangeId".into(), get(leg, "exchangeId").clone());
        inspect_scoped_rest_no_account(&json!({
            "account": account,
            "binding": V::Object(binding),
            "readAt": iso(n as f64),
        }))
    }
    fn check_book(&mut self, r: &V, leg: &V, n: i64) -> Result<V, String> {
        fresh(r, n)?;
        let (data, _) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
        let b = normalize_book(
            &data,
            get(leg, "marketId").as_str().unwrap_or(""),
            get(&self.scope, "tournamentId").as_str().unwrap_or(""),
            get(r, "receivedAt").as_str().unwrap_or(""),
            get(r, "requestStartedAt").as_str().unwrap_or(""),
            20_000.0,
            &BTreeSet::new(),
            "authoritative_receipt_v1",
        )?;
        // (!hasOwn(data,'marketId')||data.marketId===leg.marketId) && one tournament
        // context && one exchange && exchange/book bounds.
        need(
            (data.get("marketId").is_none() || data.get("marketId") == Some(get(leg, "marketId")))
                && get(&data, "contexts").as_array().is_some_and(|c| c.len() == 1)
                && get(&get(&data, "contexts")[0], "type") == &json!("tournament")
                && get(&get(&get(&data, "contexts")[0], "orderbook"), "exchanges").as_array().is_some_and(|e| e.len() == 1)
                && get(&b, "exchangeId") == get(leg, "exchangeId")
                && get(&b, "bids").as_array().is_some_and(|a| a.len() <= 20)
                && get(&b, "asks").as_array().is_some_and(|a| a.len() <= 20),
            "BOOK_SCOPE",
        )?;
        let ex = get(leg, "exchangeId").as_str().unwrap_or("").to_string();
        if let Some(old) = self.book_watermarks.get(&ex) {
            need(
                js_int(&b.get("sequence").cloned().unwrap_or(V::Null)).unwrap_or(0) as i128
                    >= js_int(&old.sequence).unwrap_or(0) as i128
                    && instant_ns(get(&b, "sourceAt").as_str().unwrap_or("")).unwrap_or(i128::MIN)
                        >= instant_ns(old.source_at.as_str().unwrap_or("")).unwrap_or(i128::MIN)
                    && instant(get(&b, "requestStartedAt").as_str().unwrap_or("")).unwrap_or(i64::MAX)
                        >= instant(old.request_started_at.as_str().unwrap_or("")).unwrap_or(i64::MIN),
                "BOOK_ROLLBACK",
            )?;
            if get(&b, "versionKey") == &old.version_key {
                need(
                    payload_hash(&V::Array(vec![get(&b, "bids").clone(), get(&b, "asks").clone()]))?
                        == payload_hash(&V::Array(vec![old.bids.clone(), old.asks.clone()]))?,
                    "BOOK_VERSION_CONFLICT",
                )?;
            }
        }
        Ok(b)
    }
}

impl PairedRestEvidenceBridge {
    // getEvidence(context)
    pub fn get_evidence(&self, context: &V) -> Result<V, String> {
        let mut g = self.inner.borrow_mut();
        let r: Result<V, String> = (|| {
            g.allowed()?;
            need(!g.busy && g.cache.is_some(), "CACHE_UNAVAILABLE")?;
            let cache = g.cache.clone().unwrap();
            if let Some(market_id) = context.get("marketId") {
                need(
                    market_id == &get(&cache, "leg").get("marketId").cloned().unwrap_or(V::Null)
                        && context.get("exchangeId") == Some(&get(&cache, "leg").get("exchangeId").cloned().unwrap_or(V::Null)),
                    "CACHE_LEG_MISMATCH",
                )?;
            }
            if let Some(order_id) = context.get("orderId") {
                if !order_id.is_null() {
                    need(order_id == &get(&cache, "orderId").clone(), "CACHE_ORDER_MISMATCH")?;
                }
            }
            let n = (g.deps.now)() as i64;
            match get(&cache, "kind").as_str().unwrap_or("") {
                "preparation" => {
                    need(
                        context.get("operation").map(|o| o.as_str().is_some_and(|s| ["prepare", "place"].contains(&s))).unwrap_or(true),
                        "CACHE_OPERATION",
                    )?;
                    g.validate_account(get(&cache, "account"), get(&cache, "leg"), n)?;
                    let b = g.check_book(get(&cache, "book"), get(&cache, "leg"), n)?;
                    need(
                        instant(get(&b, "requestStartedAt").as_str().unwrap_or("")).map_err(|_| need_err("BOOK_BEFORE_ACCOUNT"))?
                            >= instant(get(&cache, "accountCompletedAt").as_str().unwrap_or("")).map_err(|_| need_err("BOOK_BEFORE_ACCOUNT"))?,
                        "BOOK_BEFORE_ACCOUNT",
                    )?;
                    let (funding, reserved) = g.local_funding()?;
                    Ok(json!({
                        "tournamentId": get(&g.scope, "tournamentId"),
                        "account": get(&cache, "account"),
                        "book": get(&cache, "book"),
                        "globalRisk": g.local_global()?,
                        "funding": funding,
                        "reservedElsewhere": reserved,
                    }))
                }
                "cancel" => {
                    need(
                        context.get("operation").map(|o| o.as_str() == Some("cancel")).unwrap_or(true),
                        "CACHE_OPERATION",
                    )?;
                    g.own(get(&cache, "orderId"), get(&cache, "leg"))?;
                    fresh(get(&cache, "order"), n)?;
                    Ok(json!({"order": get(&cache, "order")}))
                }
                _ => {
                    need(
                        context.get("operation").map(|o| o.as_str() == Some("reconcile")).unwrap_or(true),
                        "CACHE_OPERATION",
                    )?;
                    g.own(get(&cache, "orderId"), get(&cache, "leg"))?;
                    fresh(get(&cache, "order"), n)?;
                    for r in get(&cache, "fillPages").as_array().cloned().unwrap_or_default() {
                        fresh(&r, n)?;
                    }
                    g.validate_account(get(&cache, "after"), get(&cache, "leg"), n)?;
                    Ok(json!({
                        "order": get(&cache, "order"),
                        "fillPages": get(&cache, "fillPages"),
                        "after": get(&cache, "after"),
                        "globalRisk": g.local_global()?,
                    }))
                }
            }
        })();
        match r {
            Ok(v) => Ok(v),
            Err(e) => {
                g.cache = None;
                let code = if e.starts_with("PAIRED_EVIDENCE_")
                    && e["PAIRED_EVIDENCE_".len()..].bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
                {
                    e
                } else {
                    "PAIRED_EVIDENCE_INVALID_CACHE".to_string()
                };
                g.last_failure = Some(code.clone());
                Err(code)
            }
        }
    }

    // authorizes(spec, beforeDispatch, beforeNetwork)
    pub fn authorizes(&self, spec: &V, before_dispatch: &Guard, before_network: &Guard) -> bool {
        let g = self.inner.borrow();
        let r: Result<bool, String> = (|| {
            g.allowed()?;
            let key = before_dispatch as *const Guard as *const () as usize;
            let p = g.issued.get(&key).cloned();
            Ok(p.is_some()
                && get(spec, "method").as_str() == Some("GET")
                && get(spec, "endpoint") == get(p.as_ref().unwrap(), "endpoint")
                && spec.get("body").is_none()
                && spec.get("bodyJson").is_none()
                && get(spec, "beforeNetwork").is_null()
                && (before_network as *const Guard as *const () as usize)
                    == get(&p.unwrap(), "beforeNetworkPtr").as_u64().unwrap_or(0) as usize
                && spec.as_object().map(|o| {
                    o.keys().all(|k| {
                        ["method", "endpoint", "body", "bodyJson", "beforeDispatch", "beforeNetwork"]
                            .contains(&k.as_str())
                    })
                }).unwrap_or(false))
        })();
        r.unwrap_or(false)
    }

    // read(endpoint) — synchronous port of the async read.
    fn read(&self, endpoint: &str) -> Result<V, String> {
        let mut g = self.inner.borrow_mut();
        g.allowed()?;
        let max = js_int(get(&g.max_reads_per_operation, g.operation.as_deref().unwrap_or(""))).unwrap_or(0);
        need(g.reads < max, "READ_BOUND")?;
        let budget = |admitted_already: bool| -> Result<(), String> {
            need(
                (g.deps.has_read_headroom)(max - g.reads - if admitted_already { 1 } else { 0 }),
                "READ_HEADROOM",
            )
        };
        budget(false)?;
        let admitted = Rc::new(RefCell::new(false));
        let inner_a = self.inner.clone();
        let admitted_a = admitted.clone();
        let max_a = max;
        let before_dispatch = move |_v: &V, _s: &V| -> Result<bool, String> {
            let g = inner_a.borrow();
            g.allowed()?;
            need(
                (g.deps.has_read_headroom)(max_a - g.reads),
                "READ_HEADROOM",
            )?;
            need(!*admitted_a.borrow(), "DUPLICATE_ADMISSION")?;
            *admitted_a.borrow_mut() = true;
            Ok(true)
        };
        let inner_b = self.inner.clone();
        let admitted_b = admitted.clone();
        let before_network = move |_v: &V, _s: &V| -> Result<bool, String> {
            let g = inner_b.borrow();
            g.allowed()?;
            need(*admitted_b.borrow(), "NOT_ADMITTED")?;
            need(
                (g.deps.has_read_headroom)(max - g.reads - 1),
                "READ_HEADROOM",
            )?;
            Ok(true)
        };
        let bd_ptr = &before_dispatch as &Guard as *const Guard as *const () as usize;
        let bn_ptr = &before_network as &Guard as *const Guard as *const () as usize;
        let spec = json!({"method": "GET", "endpoint": endpoint});
        g.issued.insert(
            bd_ptr,
            json!({"endpoint": endpoint, "beforeNetworkPtr": bn_ptr as u64}),
        );
        let began = (g.deps.now)() as i64;
        drop(g);
        let result: Result<V, String> = (|| {
            let response = {
                let g = self.inner.borrow();
                (g.deps.dispatch)(&spec, Some(&before_dispatch as &Guard), Some(&before_network as &Guard))
            }?;
            let mut g = self.inner.borrow_mut();
            g.allowed()?;
            need(
                *admitted.borrow()
                    && get(&response, "endpoint").as_str() == Some(endpoint)
                    && get(&response, "status").as_i64().unwrap_or(200) == 200,
                "READ_RESPONSE",
            )?;
            let r = (g.deps.receipt_of)(&response, &json!({"method": "GET", "requestBodyHash": V::Null}))?;
            need(
                get(&r, "method").as_str() == Some("GET")
                    && get(&r, "endpoint").as_str() == Some(endpoint)
                    && get(&r, "status") == &json!(200)
                    && get(&r, "bodyText").as_str().is_some_and(|s| s.len() <= 8 * 1024 * 1024)
                    && hex(get(&r, "sourceBodyHash"))
                    && get(&r, "sourceBodyHash").as_str() == Some(sha256(get(&r, "bodyText").as_str().unwrap_or("").as_bytes()).as_str())
                    && get(&r, "requestBodyHash").is_null()
                    && r.as_object().map(|o| {
                        o.keys().all(|k| {
                            ["method", "endpoint", "status", "bodyText", "sourceBodyHash", "requestBodyHash", "requestStartedAt", "receivedAt", "fetchInvocationId"]
                                .contains(&k.as_str())
                        })
                    }).unwrap_or(false),
                "RAW_RECEIPT",
            )?;
            fresh(&r, (g.deps.now)() as i64)?;
            need(
                instant(get(&r, "requestStartedAt").as_str().unwrap_or("")).map_err(|_| need_err("RECEIPT_CAUSALITY"))? >= began
                    && instant(get(&r, "requestStartedAt").as_str().unwrap_or("")).unwrap()
                        >= instant(get(&response, "requestStartedAt").as_str().unwrap_or("")).map_err(|_| need_err("RECEIPT_CAUSALITY"))?
                    && instant(get(&r, "receivedAt").as_str().unwrap_or("")).map_err(|_| need_err("RECEIPT_CAUSALITY"))?
                        <= instant(get(&response, "receivedAt").as_str().unwrap_or("")).map_err(|_| need_err("RECEIPT_CAUSALITY"))?
                    && (g.last_end.is_none()
                        || instant(get(&r, "requestStartedAt").as_str().unwrap_or("")).unwrap() >= g.last_end.unwrap()),
                "RECEIPT_CAUSALITY",
            )?;
            need(
                payload_hash(&parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?.0)?
                    == payload_hash(get(&response, "data"))?,
                "RAW_DATA_MISMATCH",
            )?;
            if let Some(prior) = g.receipt_watermarks.get(endpoint) {
                need(
                    instant(get(&r, "requestStartedAt").as_str().unwrap_or("")).map_err(|_| need_err("RECEIPT_ROLLBACK"))? >= prior.0
                        && instant(get(&r, "receivedAt").as_str().unwrap_or("")).map_err(|_| need_err("RECEIPT_ROLLBACK"))? >= prior.1,
                    "RECEIPT_ROLLBACK",
                )?;
            }
            g.receipt_watermarks.insert(
                endpoint.to_string(),
                (
                    instant(get(&r, "requestStartedAt").as_str().unwrap_or("")).unwrap(),
                    instant(get(&r, "receivedAt").as_str().unwrap_or("")).unwrap(),
                ),
            );
            g.last_end = Some(instant(get(&r, "receivedAt").as_str().unwrap_or("")).unwrap());
            Ok(r)
        })();
        let mut g = self.inner.borrow_mut();
        g.reads += 1;
        g.issued.remove(&bd_ptr);
        result
    }

    // pages(base,check)
    fn pages(&self, base: &str, check: &dyn Fn(&V, &V) -> Result<(), String>) -> Result<Vec<V>, String> {
        let mut cursor: Option<String> = None;
        let mut checkpoint: Option<i64> = self
            .inner
            .borrow()
            .projection_watermarks
            .get(base)
            .copied();
        let mut all: Vec<V> = Vec::new();
        let mut seen_cursors: BTreeSet<String> = BTreeSet::new();
        let mut ids: BTreeSet<i64> = BTreeSet::new();
        for _ in 0..10 {
            let url = match &cursor {
                None => base.to_string(),
                Some(c) => format!("{base}&cursor={}", urlencoding(c)),
            };
            let r = self.read(&url)?;
            let (d, _) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
            let p = get(&d, "pagination");
            need(
                get(&d, "data").as_array().is_some_and(|a| a.len() <= 200)
                    && get(p, "limit") == &json!(200)
                    && get(p, "hasMore").is_boolean()
                    && (get(p, "nextCursor").is_null()
                        || get(p, "nextCursor").as_str().is_some_and(|s| !s.is_empty() && s.len() <= 1000))
                    && get(&get(&d, "coverage"), "complete") == &json!(true)
                    && (get(&get(&d, "coverage"), "projectedThroughSequence").is_null()
                        || integer(&get(&get(&d, "coverage"), "projectedThroughSequence"), 0)),
                "PAGES_INCOMPLETE",
            )?;
            if checkpoint.is_some() && !get(&get(&d, "coverage"), "projectedThroughSequence").is_null() {
                need(
                    js_int(&get(&get(&d, "coverage"), "projectedThroughSequence")).unwrap_or(0)
                        >= checkpoint.unwrap(),
                    "PROJECTION_ROLLBACK",
                )?;
            }
            if !get(&get(&d, "coverage"), "projectedThroughSequence").is_null() {
                checkpoint = Some(js_int(&get(&get(&d, "coverage"), "projectedThroughSequence")).unwrap_or(0));
            }
            for row in get(&d, "data").as_array().cloned().unwrap_or_default() {
                need(
                    integer(&get(&row, "id"), 1) && !ids.contains(&js_int(get(&row, "id")).unwrap_or(0)),
                    "PAGE_DUPLICATE_ID",
                )?;
                ids.insert(js_int(get(&row, "id")).unwrap_or(0));
            }
            check(&d, &r)?;
            all.push(r);
            if get(p, "hasMore") == &json!(false) {
                need(get(p, "nextCursor").is_null(), "PAGE_CURSOR")?;
                if let Some(cp) = checkpoint {
                    self.inner.borrow_mut().projection_watermarks.insert(base.to_string(), cp);
                }
                return Ok(all);
            }
            let nc = get(p, "nextCursor").as_str().unwrap_or("").to_string();
            need(!nc.is_empty() && !seen_cursors.contains(&nc), "PAGE_CURSOR")?;
            seen_cursors.insert(nc.clone());
            cursor = Some(nc);
        }
        Err(need_err("PAGES_INCOMPLETE"))
    }

    // account(leg)
    fn account(&self, leg: &V) -> Result<V, String> {
        let slug = get(&self.inner.borrow().scope, "tournamentSlug").as_str().unwrap_or("").to_string();
        let tid = get(&self.inner.borrow().scope, "tournamentId").as_str().unwrap_or("").to_string();
        let cash = self.read(&format!("/tournaments/{slug}"))?;
        let positions = self.read(&format!("/tournaments/{slug}/portfolio/positions"))?;
        let open_orders_pages = self.pages(&format!("/orders?status=open&limit=200&tournamentId={tid}"), &|_, _| Ok(()))?;
        let scoped_orders_pages = self.pages(
            &format!("/orders?status=all&exchangeId={}&limit=200&tournamentId={tid}", get(leg, "exchangeId").as_str().unwrap_or("")),
            &|_, _| Ok(()),
        )?;
        let result = json!({
            "cash": cash,
            "positions": positions,
            "openOrdersPages": open_orders_pages,
            "scopedOrdersPages": scoped_orders_pages,
        });
        let n = (self.inner.borrow().deps.now)() as i64;
        self.inner.borrow().validate_account(&result, leg, n)?;
        Ok(result)
    }

    // orderCheck(r,orderId,leg,open)
    fn order_check(&self, r: &V, order_id: &V, leg: &V, open: bool) -> Result<V, String> {
        let (d, _) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
        let tid = get(&self.inner.borrow().scope, "tournamentId").as_str().unwrap_or("").to_string();
        need(
            integer(get(&d, "id"), 1)
                && js_int(get(&d, "id")).map(|x| x.to_string()) == order_id.as_str().map(|s| s.to_string())
                && get(&d, "exchangeId") == get(leg, "exchangeId")
                && get(&d, "tournamentId") == &json!(tid)
                && get(&d, "side").as_str() == Some("no")
                && get(&d, "action").as_str() == Some("buy")
                && get(&d, "quantity") == &json!(1)
                && get(&d, "open") == &json!(open)
                && instant(get(&d, "createdAt").as_str().unwrap_or("")).map_err(|_| need_err("ORDER_SCOPE_OR_PHASE"))?
                    <= (self.inner.borrow().deps.now)() as i64 + 2000
                && instant(get(&d, "expirationDate").as_str().unwrap_or("")).map_err(|_| need_err("ORDER_SCOPE_OR_PHASE"))?
                    > instant(get(&d, "createdAt").as_str().unwrap_or("")).unwrap(),
            "ORDER_SCOPE_OR_PHASE",
        )?;
        normalize_official_fill_tick(get(&d, "priceLimit"))?;
        if !open {
            need(
                integer(get(&d, "quantityFilled"), 0) && js_int(get(&d, "quantityFilled")).unwrap_or(0) <= 1,
                "TERMINAL_QUANTITY",
            )?;
        }
        Ok(d)
    }

    // run(kind,legValue,orderId,fn) — fn is a sync closure in Rust.
    fn run(
        &self,
        kind: &str,
        leg_value: &V,
        order_id: &V,
        f: &dyn Fn(&V) -> Result<V, String>,
    ) -> Result<V, String> {
        {
            let mut g = self.inner.borrow_mut();
            need(!g.busy, "BUSY")?;
            g.cache = None;
            g.allowed()?;
        }
        let leg = exact_leg(&self.inner.borrow().scope, leg_value)?;
        if !order_id.is_null() {
            self.inner.borrow().own(order_id, &leg)?;
        }
        {
            let mut g = self.inner.borrow_mut();
            g.busy = true;
            g.operation = Some(kind.to_string());
            g.reads = 0;
            g.last_end = None;
            g.last_failure = None;
        }
        let result: Result<V, String> = (|| {
            need(
                (self.inner.borrow().deps.has_read_headroom)(
                    js_int(get(&self.inner.borrow().max_reads_per_operation, kind)).unwrap_or(0),
                ),
                "READ_HEADROOM",
            )?;
            let value = f(&leg)?;
            self.inner.borrow().allowed()?;
            self.inner.borrow_mut().cache = Some(value);
            Ok(json!({
                "kind": kind,
                "leg": leg,
                "orderId": order_id,
                "completedAt": iso((self.inner.borrow().deps.now)()),
                "readCount": self.inner.borrow().reads,
            }))
        })();
        let mut g = self.inner.borrow_mut();
        g.busy = false;
        g.operation = None;
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                g.cache = None;
                let code = if e.starts_with("PAIRED_EVIDENCE_")
                    && e["PAIRED_EVIDENCE_".len()..].bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
                {
                    e
                } else {
                    "PAIRED_EVIDENCE_READ_FAILED".to_string()
                };
                g.last_failure = Some(code.clone());
                Err(code)
            }
        }
    }

    // refreshPreparation(leg)
    pub fn refresh_preparation(&self, leg: &V) -> Result<V, String> {
        self.run("preparation", leg, &V::Null, &|l| {
            let a = self.account(l)?;
            let n = (self.inner.borrow().deps.now)() as i64;
            let info = self.inner.borrow().validate_account(&a, l, n)?;
            let tid = get(&self.inner.borrow().scope, "tournamentId").as_str().unwrap_or("").to_string();
            let book = self.read(&format!(
                "/markets/{}/orderbook?tournamentId={tid}&depth=20",
                get(l, "marketId").as_str().unwrap_or("")
            ))?;
            let b = self.inner.borrow_mut().check_book(&book, l, (self.inner.borrow().deps.now)() as i64)?;
            need(
                instant(get(&b, "requestStartedAt").as_str().unwrap_or("")).map_err(|_| need_err("BOOK_BEFORE_ACCOUNT"))?
                    >= instant(get(&info, "completedAt").as_str().unwrap_or("")).map_err(|_| need_err("BOOK_BEFORE_ACCOUNT"))?,
                "BOOK_BEFORE_ACCOUNT",
            )?;
            let n2 = (self.inner.borrow().deps.now)() as i64;
            self.inner.borrow().validate_account(&a, l, n2)?;
            self.inner.borrow_mut().book_watermarks.insert(
                get(l, "exchangeId").as_str().unwrap_or("").to_string(),
                Watermark {
                    sequence: get(&b, "sequence").clone(),
                    source_at: get(&b, "sourceAt").clone(),
                    request_started_at: get(&b, "requestStartedAt").clone(),
                    version_key: get(&b, "versionKey").clone(),
                    bids: get(&b, "bids").clone(),
                    asks: get(&b, "asks").clone(),
                },
            );
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

    // refreshReconciliation(orderId,leg)
    pub fn refresh_reconciliation(&self, order_id: &V, leg: &V) -> Result<V, String> {
        self.run("reconciliation", leg, order_id, &|l| {
            let order = self.read(&format!("/orders/{}", order_id.as_str().unwrap_or("")))?;
            let od = self.order_check(&order, order_id, l, false)?;
            let mut totals: Option<String> = None;
            let fill_pages = {
                let l2 = l.clone();
                let od_created = get(&od, "createdAt").clone();
                let totals_cell = Rc::new(RefCell::new(totals.clone()));
                let pages = self.pages(
                    &format!("/orders/{}/fills?limit=200", order_id.as_str().unwrap_or("")),
                    &|d: &V, r: &V| -> Result<(), String> {
                        need(
                            js_int(get(d, "orderId")) == order_id.as_str().and_then(|s| s.parse::<i64>().ok())
                                && get(d, "exchangeId") == get(&l2, "exchangeId")
                                && get(d, "tournamentId") == get(&self.inner.borrow().scope, "tournamentId")
                                && matches!(js_int(get(d, "totalQuantityFilled")), Some(0) | Some(-1)),
                            "FILL_SCOPE",
                        )?;
                        let avg = if get(d, "avgFillPrice").is_null() {
                            V::Null
                        } else {
                            normalize_official_fill_tick(get(d, "avgFillPrice"))
                                .map(|v| get(&v, "price").clone())
                                .unwrap_or(V::Null)
                        };
                        let identity = payload_hash(&json!([get(d, "totalQuantityFilled"), avg]))?;
                        let mut t = totals_cell.borrow_mut();
                        need(t.is_none() || t.as_ref() == Some(&identity), "FILL_TOTAL_CHANGED")?;
                        *t = Some(identity);
                        drop(t);
                        for f in get(d, "data").as_array().cloned().unwrap_or_default() {
                            need(
                                get(&f, "side").as_str() == Some("no")
                                    && get(&f, "quantity") == &json!(-1)
                                    && instant(get(&f, "filledAt").as_str().unwrap_or("")).map_err(|_| need_err("FILL_SCHEMA"))?
                                        >= instant(od_created.as_str().unwrap_or("")).map_err(|_| need_err("FILL_SCHEMA"))?
                                    && instant(get(&f, "filledAt").as_str().unwrap_or("")).unwrap()
                                        <= instant(get(r, "receivedAt").as_str().unwrap_or("")).map_err(|_| need_err("FILL_SCHEMA"))?,
                                "FILL_SCHEMA",
                            )?;
                            normalize_official_fill_tick(get(&f, "price"))?;
                        }
                        Ok(())
                    },
                )?;
                totals = totals_cell.borrow().clone();
                let _ = totals;
                pages
            };
            let decoded: Vec<V> = fill_pages
                .iter()
                .map(|r| parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or("")).map(|x| x.0))
                .collect::<Result<_, _>>()?;
            let fills: Vec<V> = decoded
                .iter()
                .flat_map(|d| get(d, "data").as_array().cloned().unwrap_or_default())
                .collect();
            let last = decoded.last().cloned().unwrap_or(V::Null);
            need(
                fills.len() <= 1
                    && fills.len() as i64 == js_int(get(&od, "quantityFilled")).unwrap_or(-1)
                    && js_int(get(&last, "totalQuantityFilled")).unwrap_or(0) == -(fills.len() as i64)
                    && (if fills.is_empty() {
                        get(&last, "avgFillPrice").is_null()
                    } else {
                        let a = normalize_official_fill_tick(get(&last, "avgFillPrice"))
                            .map(|v| get(&v, "price").clone());
                        let f0 = normalize_official_fill_tick(get(&fills[0], "price"))
                            .map(|v| get(&v, "price").clone());
                        let l0 = normalize_official_fill_tick(get(&od, "priceLimit"))
                            .map(|v| get(&v, "price").clone());
                        if !(a.is_ok() && f0.is_ok() && l0.is_ok()) {
                            false
                        } else {
                            let (a, f0, l0) = (a.unwrap(), f0.unwrap(), l0.unwrap());
                            a == f0
                                && f0.as_str().and_then(|x| amount(x).ok())
                                    .zip(l0.as_str().and_then(|x| amount(x).ok()))
                                    .map(|(p, limit)| p <= limit)
                                    .unwrap_or(false)
                        }
                    }),
                "FILL_TOTAL_MISMATCH",
            )?;
            let after = self.account(l)?;
            need(
                instant(get(&get(&after, "cash"), "requestStartedAt").as_str().unwrap_or(""))
                    .map_err(|_| need_err("POST_ACCOUNT_CAUSALITY"))?
                    >= instant(get(&fill_pages.last().unwrap_or(&V::Null), "receivedAt").as_str().unwrap_or(""))
                        .map_err(|_| need_err("POST_ACCOUNT_CAUSALITY"))?,
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
        self.get_evidence(&json!({
            "operation": "reconcile",
            "orderId": order_id,
            "marketId": get(&exact_leg(&self.inner.borrow().scope, leg)?, "marketId"),
            "exchangeId": get(&exact_leg(&self.inner.borrow().scope, leg)?, "exchangeId"),
        }))
    }

    // refreshCancel(orderId,leg)
    pub fn refresh_cancel(&self, order_id: &V, leg: &V) -> Result<V, String> {
        self.run("cancel", leg, order_id, &|l| {
            let order = self.read(&format!("/orders/{}", order_id.as_str().unwrap_or("")))?;
            self.order_check(&order, order_id, l, true)?;
            Ok(json!({"kind": "cancel", "leg": l, "orderId": order_id, "order": order}))
        })
    }

    // status()
    pub fn status(&self) -> V {
        let g = self.inner.borrow();
        json!({
            "mode": "paired_rest_evidence_bridge_v1",
            "busy": g.busy,
            "closed": g.closed,
            "cachedKind": g.cache.as_ref().map(|c| get(c, "kind").clone()).unwrap_or(V::Null),
            "cachedLeg": g.cache.as_ref().map(|c| get(c, "leg").clone()).unwrap_or(V::Null),
            "lastReadCount": g.reads,
            "lastFailure": g.last_failure,
            "additionalApiClients": 0,
            "executionAuthority": false,
        })
    }

    // clear()
    pub fn clear(&self) {
        self.inner.borrow_mut().cache = None;
    }

    // close()
    pub fn close(&self) -> Result<(), String> {
        let mut g = self.inner.borrow_mut();
        need(!g.busy, "BUSY")?;
        g.cache = None;
        g.closed = true;
        g.receipt_watermarks.clear();
        g.book_watermarks.clear();
        g.projection_watermarks.clear();
        Ok(())
    }
}

fn urlencoding(s: &str) -> String {
    // encodeURIComponent: unreserved [A-Za-z0-9-_.!~*'()] stay, rest percent-encoded UTF-8.
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[allow(dead_code)]
fn unused(_: &V) -> bool {
    js_truthy(None)
}
