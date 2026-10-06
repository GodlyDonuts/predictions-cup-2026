// execution/paired-rest-evidence-bridge.mjs — read-only adapter over the
// caller's EXISTING serialized ReadClient. Every GET is identity-fenced; no
// credential, independent client, rate limiter or timer.
use baal_execution::core::payload_hash;
use baal_execution::gates::normalize_official_fill_tick;
use crate::no_rest_reconciliation::inspect_scoped_rest_no_account;
use baal_execution::receipts::parse_number_lexemes;
use baal_service_runtime::contract::instant_ns;
use baal_service_runtime::fixed::{amount, instant};
use baal_service_runtime::json_text::{js_int, sha256};
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("PAIRED_EVIDENCE_{code}")) }
}
fn copy(v: &V) -> V { v.clone() }
fn sha(b: &[u8]) -> String { sha256(b) }
fn id(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        !s.is_empty() && s.as_bytes()[0] != b'0' && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<u128>().map(|n| n <= 9_007_199_254_740_991).unwrap_or(false)
    })
}
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn iso(n: i64) -> String {
    baal_service_runtime::contract::ms_to_iso(n as f64)
}
fn integer(v: &V, min: i64) -> bool { js_int(v).is_some_and(|x| x >= min) }
fn get<'a>(v: &'a V, k: &str) -> &'a V { v.get(k).unwrap_or(&V::Null) }
fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || "-_.!~*'()".contains(c) { out.push(c); }
        else { for b in c.to_string().as_bytes() { out.push_str(&format!("%{b:02X}")); } }
    }
    out
}
type FnV = Rc<dyn Fn() -> Result<V, String>>;
type Guard<'a> = dyn Fn(&V, &V) -> Result<bool, baal_execution::transport::OrderTransportError> + 'a;
type FnBool = Rc<dyn Fn(i64) -> Result<bool, String>>;
type FnU64 = Rc<dyn Fn() -> i64>;

pub struct PairedRestEvidenceBridge {
    api: Rc<dyn for<'g> Fn(&V, Option<&'g Guard<'g>>, Option<&'g Guard<'g>>) -> Result<V, String>>,
    receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    scope: V,
    get_global_risk: FnV,
    get_funding: FnV,
    get_other_reserved: FnV,
    owns_order: Rc<dyn Fn(&V, &V) -> bool>,
    can_read: Rc<dyn Fn() -> bool>,
    has_read_headroom: FnBool,
    now: FnU64,
    max_reads_per_operation: V,
    cache: RefCell<Option<V>>,
    busy: RefCell<bool>,
    closed: RefCell<bool>,
    operation: RefCell<Option<String>>,
    reads: RefCell<i64>,
    last_failure: RefCell<Option<String>>,
    last_end: RefCell<Option<i64>>,
    receipt_watermarks: RefCell<BTreeMap<String, (i64, i64)>>,
    book_watermarks: RefCell<BTreeMap<String, V>>,
    projection_watermarks: RefCell<BTreeMap<String, i64>>,
    issued: RefCell<Vec<(String, usize)>>,
}

impl PairedRestEvidenceBridge {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        api: Rc<dyn for<'g> Fn(&V, Option<&'g Guard<'g>>, Option<&'g Guard<'g>>) -> Result<V, String>>,
        receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
        scope: &V,
        get_global_risk: FnV,
        get_funding: FnV,
        get_other_reserved: FnV,
        owns_order: Rc<dyn Fn(&V, &V) -> bool>,
        can_read: Rc<dyn Fn() -> bool>,
        has_read_headroom: FnBool,
        now: FnU64,
        max_reads_per_operation: Option<V>,
    ) -> Result<Self, String> {
        let scope_ok = get(scope, "tournamentId").as_str().is_some()
            && uuid_like(get(scope, "tournamentId").as_str().unwrap_or(""))
            && slug_re(get(scope, "tournamentSlug").as_str().unwrap_or(""))
            && get(scope, "markets").as_array().is_some_and(|m| m.len() == 2)
            && get(scope, "markets").as_array().map(|m| m.iter().all(|x| id(get(x, "marketId")) && id(get(x, "exchangeId")))).unwrap_or(false)
            && get(scope, "markets").as_array().map(|m| m.iter().map(|x| get(x, "marketId").to_string()).collect::<BTreeSet<_>>().len() == 2).unwrap_or(false)
            && get(scope, "markets").as_array().map(|m| m.iter().map(|x| get(x, "exchangeId").to_string()).collect::<BTreeSet<_>>().len() == 2).unwrap_or(false);
        need(scope_ok, "SCOPE")?;
        let defaults = json!({"preparation": 23, "reconciliation": 33, "cancel": 1});
        let bounds = defaults.clone();
        let mut mro = max_reads_per_operation.unwrap_or_else(|| defaults.clone());
        if !mro.is_object() { mro = defaults.clone(); }
        for (k, b) in bounds.as_object().unwrap() {
            need(integer(get(&mro, k), 1) && js_int(get(&mro, k)).unwrap() <= b.as_i64().unwrap(), "READ_BOUND")?;
        }
        Ok(Self {
            api,
            receipt_of,
            scope: copy(scope),
            get_global_risk,
            get_funding,
            get_other_reserved,
            owns_order,
            can_read,
            has_read_headroom,
            now,
            max_reads_per_operation: mro,
            cache: RefCell::new(None),
            busy: RefCell::new(false),
            closed: RefCell::new(false),
            operation: RefCell::new(None),
            reads: RefCell::new(0),
            last_failure: RefCell::new(None),
            last_end: RefCell::new(None),
            receipt_watermarks: RefCell::new(BTreeMap::new()),
            book_watermarks: RefCell::new(BTreeMap::new()),
            projection_watermarks: RefCell::new(BTreeMap::new()),
            issued: RefCell::new(Vec::new()),
        })
    }
    fn allowed(&self) -> Result<(), String> {
        need(!*self.closed.borrow() && (self.can_read)(), "STOPPED")
    }
    fn sync_v(&self, f: &FnV) -> Result<V, String> { f().map(|v| copy(&v)) }
    fn sync_b(&self, f: &FnBool, arg: i64) -> Result<bool, String> { f(arg) }
    fn own(&self, order_id: &V, leg: &V) -> Result<(), String> {
        need(id(order_id) && (self.owns_order)(order_id, leg), "ORDER_NOT_OWNED")
    }
    fn clear(&self) { *self.cache.borrow_mut() = None; }
    fn local_global(&self) -> Result<V, String> {
        let g = self.sync_v(&self.get_global_risk)?;
        need(
            g.is_object()
                && get(&g, "tournamentId") == get(&self.scope, "tournamentId")
                && hex(get(&g, "sourceHash"))
                && instant(get(&g, "at").as_str().unwrap_or("")).map(|x| x <= (self.now)()).unwrap_or(false)
                && (self.now)() - instant(get(&g, "at").as_str().unwrap_or("")).map_err(|e| e)? <= 20000,
            "GLOBAL_RISK_CLOCK_OR_SCOPE",
        )?;
        Ok(g)
    }
    fn local_funding(&self) -> Result<V, String> {
        let f = self.sync_v(&self.get_funding)?;
        let other = self.sync_v(&self.get_other_reserved)?;
        need(
            other.as_str().is_some() && amount(other.as_str().unwrap_or("")).map(|x| x >= 0).unwrap_or(false),
            "OTHER_RESERVE",
        )?;
        if !f.is_null() {
            need(
                hex(get(&f, "reservationHash"))
                    && hex(get(&f, "globalLedgerHash"))
                    && get(&f, "amount").is_string()
                    && amount(get(&f, "amount").as_str().unwrap_or("")).map(|x| x >= 0).unwrap_or(false)
                    && instant(get(&f, "confirmedAt").as_str().unwrap_or("")).map(|x| x <= (self.now)()).unwrap_or(false)
                    && (self.now)() - instant(get(&f, "confirmedAt").as_str().unwrap_or("")).map_err(|e| e)? <= 20000,
                "FUNDING_CLOCK_OR_SCHEMA",
            )?;
        }
        Ok(json!({"funding": f, "reservedElsewhere": other}))
    }
    fn validate_account(&self, account: &V, leg: &V, n: i64) -> Result<V, String> {
        let mut binding = self.scope.clone();
        binding.as_object_mut().unwrap().insert("exchangeId".into(), get(leg, "exchangeId").clone());
        inspect_scoped_rest_no_account(&json!({
            "account": account,
            "binding": binding,
            "readAt": iso(n),
        }))
    }
    fn check_book(&self, r: &V, leg: &V, n: i64) -> Result<V, String> {
        fresh(r, n)?;
        let (data, _) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or("")).map_err(|_| "PAIRED_EVIDENCE_RAW_RECEIPT".to_string())?;
        let b = baal_collector::model::normalize_book(
            &data,
            get(leg, "marketId").as_str().unwrap_or(""),
            get(&self.scope, "tournamentId").as_str().unwrap_or(""),
            get(r, "receivedAt").as_str().unwrap_or(""),
            get(r, "requestStartedAt").as_str().unwrap_or(""),
            20000,
            &BTreeSet::new(),
            "authoritative_receipt_v1",
        ).map_err(|e| e)?;
        need(
            (!data.as_object().map(|m| m.contains_key("marketId")).unwrap_or(false) || get(&data, "marketId") == get(leg, "marketId"))
                && get(&data, "contexts").as_array().is_some_and(|c| c.len() == 1)
                && get(&get(&data, "contexts").as_array().unwrap()[0], "type") == &json!("tournament")
                && get(get(&get(&data, "contexts").as_array().unwrap()[0], "orderbook"), "exchanges").as_array().is_some_and(|e| e.len() == 1)
                && get(&b, "exchangeId") == get(leg, "exchangeId")
                && get(&b, "bids").as_array().is_some_and(|x| x.len() <= 20)
                && get(&b, "asks").as_array().is_some_and(|x| x.len() <= 20),
            "BOOK_SCOPE",
        )?;
        if let Some(old) = self.book_watermarks.borrow().get(get(leg, "exchangeId").as_str().unwrap_or("")) {
            need(
                get(&b, "sequence").as_str().unwrap_or("").parse::<i128>().ok() >= get(old, "sequence").as_str().unwrap_or("").parse::<i128>().ok()
                    && instant_ns(get(&b, "sourceAt").as_str().unwrap_or("")).unwrap_or(i128::MIN) >= instant_ns(get(old, "sourceAt").as_str().unwrap_or("")).unwrap_or(i128::MIN)
                    && instant(get(&b, "requestStartedAt").as_str().unwrap_or("")).unwrap_or(i64::MIN) >= instant(get(old, "requestStartedAt").as_str().unwrap_or("")).unwrap_or(i64::MIN),
                "BOOK_ROLLBACK",
            )?;
            if get(&b, "versionKey") == get(old, "versionKey") {
                need(
                    payload_hash(&json!([get(&b, "bids"), get(&b, "asks")])).ok() == payload_hash(&json!([get(old, "bids"), get(old, "asks")])).ok(),
                    "BOOK_VERSION_CONFLICT",
                )?;
            }
        }
        Ok(b)
    }
    pub fn get_evidence(&self, context: &V) -> Result<V, String> {
        (|| -> Result<V, String> {
            self.allowed()?;
            need(!*self.busy.borrow() && self.cache.borrow().is_some(), "CACHE_UNAVAILABLE")?;
            let cache = self.cache.borrow().clone().unwrap();
            if context.get("marketId").is_some() {
                need(
                    context.get("marketId") == get(&cache, "leg").get("marketId")
                        && context.get("exchangeId") == get(&cache, "leg").get("exchangeId"),
                    "CACHE_LEG_MISMATCH",
                )?;
            }
            if context.get("orderId").is_some_and(|o| !o.is_null()) {
                need(context.get("orderId") == Some(get(&cache, "orderId")), "CACHE_ORDER_MISMATCH")?;
            }
            let n = (self.now)();
            if get(&cache, "kind").as_str() == Some("preparation") {
                need(
                    !get(context, "operation").is_string()
                        || ["prepare", "place"].contains(&get(context, "operation").as_str().unwrap_or("")),
                    "CACHE_OPERATION",
                )?;
                self.validate_account(get(&cache, "account"), get(&cache, "leg"), n)?;
                let b = self.check_book(get(&cache, "book"), get(&cache, "leg"), n)?;
                need(
                    instant(get(&b, "requestStartedAt").as_str().unwrap_or("")).map_err(|e| e)?
                        >= instant(get(&cache, "accountCompletedAt").as_str().unwrap_or("")).map_err(|e| e)?,
                    "BOOK_BEFORE_ACCOUNT",
                )?;
                let mut out = json!({
                    "tournamentId": get(&self.scope, "tournamentId"),
                    "account": copy(get(&cache, "account")),
                    "book": copy(get(&cache, "book")),
                    "globalRisk": self.local_global()?,
                });
                for (k, v) in self.local_funding()?.as_object().unwrap() {
                    out.as_object_mut().unwrap().insert(k.clone(), v.clone());
                }
                return Ok(out);
            }
            if get(&cache, "kind").as_str() == Some("cancel") {
                need(
                    !get(context, "operation").is_string() || get(context, "operation").as_str() == Some("cancel"),
                    "CACHE_OPERATION",
                )?;
                self.own(get(&cache, "orderId"), get(&cache, "leg"))?;
                fresh(get(&cache, "order"), n)?;
                return Ok(json!({"order": copy(get(&cache, "order"))}));
            }
            need(
                !get(context, "operation").is_string() || get(context, "operation").as_str() == Some("reconcile"),
                "CACHE_OPERATION",
            )?;
            self.own(get(&cache, "orderId"), get(&cache, "leg"))?;
            fresh(get(&cache, "order"), n)?;
            for r in get(&cache, "fillPages").as_array().cloned().unwrap_or_default() { fresh(&r, n)?; }
            self.validate_account(get(&cache, "after"), get(&cache, "leg"), n)?;
            Ok(json!({
                "order": copy(get(&cache, "order")),
                "fillPages": copy(get(&cache, "fillPages")),
                "after": copy(get(&cache, "after")),
                "globalRisk": self.local_global()?,
            }))
        })()
        .map_err(|e| {
            self.clear();
            let code = if e.starts_with("PAIRED_EVIDENCE_") { e } else { "PAIRED_EVIDENCE_INVALID_CACHE".to_string() };
            *self.last_failure.borrow_mut() = Some(code.clone());
            code
        })
    }
    pub fn authorizes(&self, spec: &V) -> bool {
        (|| -> Result<bool, String> {
            self.allowed()?;
            let p = self.issued.borrow().iter().find(|(tag, _)| {
                !tag.is_empty() && tag == spec.get("beforeDispatch").and_then(|v| v.as_str()).unwrap_or("")
            }).map(|(_, i)| *i);
            Ok(p.is_some()
                && spec.get("method").and_then(|m| m.as_str()) == Some("GET")
                && spec.get("body").is_none() && spec.get("bodyJson").is_none()
                && spec.as_object().map(|m| m.keys().all(|k| {
                    ["method", "endpoint", "body", "bodyJson", "beforeDispatch", "beforeNetwork"].contains(&k.as_str())
                })).unwrap_or(false))
        })().unwrap_or(false)
    }
    fn read(&self, endpoint: &str) -> Result<V, String> {
        self.allowed()?;
        let op = self.operation.borrow().clone().unwrap_or_default();
        need(*self.reads.borrow() < js_int(get(&self.max_reads_per_operation, &op)).unwrap_or(0), "READ_BOUND")?;
        let budget = |admitted_already: bool| -> Result<(), String> {
            let need_n = js_int(get(&self.max_reads_per_operation, &op)).unwrap_or(0)
                - *self.reads.borrow()
                - if admitted_already { 1 } else { 0 };
            need((self.has_read_headroom)(need_n)?, "READ_HEADROOM")
        };
        budget(false)?;
        let admitted = std::cell::Cell::new(false);
        let before_dispatch = |_: &V, _: &V| -> Result<bool, baal_execution::transport::OrderTransportError> {
            self.allowed().map_err(|m| to_terr(&m))?;
            budget(false).map_err(|m| to_terr(&m))?;
            if admitted.get() { return Err(to_terr("PAIRED_EVIDENCE_DUPLICATE_ADMISSION")); }
            admitted.set(true);
            Ok(true)
        };
        let before_network = |_: &V, _: &V| -> Result<bool, baal_execution::transport::OrderTransportError> {
            self.allowed().map_err(|m| to_terr(&m))?;
            if !admitted.get() { return Err(to_terr("PAIRED_EVIDENCE_NOT_ADMITTED")); }
            budget(true).map_err(|m| to_terr(&m))?;
            Ok(true)
        };
        let spec = json!({"method": "GET", "endpoint": endpoint});
        let began = (self.now)();
        let result: Result<V, String> = (|| {
            let response = (self.api)(&spec, Some(&before_dispatch), Some(&before_network)).map_err(|e| e)?;
            self.allowed()?;
            need(
                admitted.get()
                    && get(&response, "endpoint") == &json!(endpoint)
                    && get(&response, "status").as_i64().unwrap_or(200) == 200,
                "READ_RESPONSE",
            )?;
            let r = (self.receipt_of)(&response, &json!({"method": "GET", "requestBodyHash": V::Null})).map_err(|e| e)?;
            let keys_ok = r.as_object().map(|m| m.keys().all(|k| {
                ["method", "endpoint", "status", "bodyText", "sourceBodyHash", "requestBodyHash", "requestStartedAt", "receivedAt", "fetchInvocationId"].contains(&k.as_str())
            })).unwrap_or(false);
            need(
                get(&r, "method") == &json!("GET")
                    && get(&r, "endpoint") == &json!(endpoint)
                    && get(&r, "status") == &json!(200)
                    && get(&r, "bodyText").as_str().is_some_and(|b| b.len() <= 8 * 1024 * 1024)
                    && hex(get(&r, "sourceBodyHash"))
                    && get(&r, "sourceBodyHash") == &json!(sha(get(&r, "bodyText").as_str().unwrap_or("").as_bytes()))
                    && get(&r, "requestBodyHash") == &V::Null
                    && keys_ok,
                "RAW_RECEIPT",
            )?;
            fresh(&r, (self.now)())?;
            need(
                instant(get(&r, "requestStartedAt").as_str().unwrap_or("")).map_err(|e| e)? >= began
                    && instant(get(&r, "requestStartedAt").as_str().unwrap_or("")).map_err(|e| e)? >= instant(get(&response, "requestStartedAt").as_str().unwrap_or("")).map_err(|e| e)?
                    && instant(get(&r, "receivedAt").as_str().unwrap_or("")).map_err(|e| e)? <= instant(get(&response, "receivedAt").as_str().unwrap_or("")).map_err(|e| e)?
                    && self.last_end.borrow().map(|x| instant(get(&r, "requestStartedAt").as_str().unwrap_or("")).unwrap_or(i64::MIN) >= x).unwrap_or(true),
                "RECEIPT_CAUSALITY",
            )?;
            let (raw_data, _) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or("")).map_err(|_| "PAIRED_EVIDENCE_RAW_RECEIPT".to_string())?;
            need(
                payload_hash(&raw_data).ok() == payload_hash(get(&response, "data")).ok(),
                "RAW_DATA_MISMATCH",
            )?;
            if let Some(prior) = self.receipt_watermarks.borrow().get(endpoint) {
                need(
                    instant(get(&r, "requestStartedAt").as_str().unwrap_or("")).unwrap_or(i64::MIN) >= prior.0
                        && instant(get(&r, "receivedAt").as_str().unwrap_or("")).unwrap_or(i64::MIN) >= prior.1,
                    "RECEIPT_ROLLBACK",
                )?;
            }
            self.receipt_watermarks.borrow_mut().insert(endpoint.to_string(), (
                instant(get(&r, "requestStartedAt").as_str().unwrap_or("")).unwrap_or(0),
                instant(get(&r, "receivedAt").as_str().unwrap_or("")).unwrap_or(0),
            ));
            *self.last_end.borrow_mut() = Some(instant(get(&r, "receivedAt").as_str().unwrap_or("")).unwrap_or(0));
            Ok(copy(&r))
        })();
        *self.reads.borrow_mut() += 1;
        result
    }
    fn pages(&self, base: &str, check: &mut dyn FnMut(&V, &V) -> Result<(), String>) -> Result<Vec<V>, String> {
        let mut cursor: Option<String> = None;
        let mut checkpoint = self.projection_watermarks.borrow().get(base).copied();
        let mut all: Vec<V> = Vec::new();
        let mut seen_cursors = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for _ in 0..10 {
            let url = match &cursor {
                None => base.to_string(),
                Some(c) => format!("{base}&cursor={}", encode_uri_component(c)),
            };
            let r = self.read(&url)?;
            let (d, _) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or("")).map_err(|_| "PAIRED_EVIDENCE_PAGES_INCOMPLETE".to_string())?;
            let p = get(&d, "pagination");
            need(
                get(&d, "data").is_array() && get(&d, "data").as_array().unwrap().len() <= 200
                    && get(p, "limit") == &json!(200)
                    && get(p, "hasMore").is_boolean()
                    && (get(p, "nextCursor").is_null() || get(p, "nextCursor").as_str().is_some_and(|s| !s.is_empty() && s.len() <= 1000))
                    && get(get(&d, "coverage"), "complete") == &json!(true)
                    && (get(get(&d, "coverage"), "projectedThroughSequence").is_null() || integer(get(get(&d, "coverage"), "projectedThroughSequence"), 0)),
                "PAGES_INCOMPLETE",
            )?;
            if checkpoint.is_some() && !get(get(&d, "coverage"), "projectedThroughSequence").is_null() {
                need(
                    js_int(get(get(&d, "coverage"), "projectedThroughSequence")).unwrap() >= checkpoint.unwrap(),
                    "PROJECTION_ROLLBACK",
                )?;
            }
            if !get(get(&d, "coverage"), "projectedThroughSequence").is_null() {
                checkpoint = js_int(get(get(&d, "coverage"), "projectedThroughSequence"));
            }
            for row in get(&d, "data").as_array().unwrap() {
                need(integer(get(row, "id"), 1) && !ids.contains(&js_int(get(row, "id")).unwrap()), "PAGE_DUPLICATE_ID")?;
                ids.insert(js_int(get(row, "id")).unwrap());
            }
            check(&d, &r)?;
            all.push(r);
            if get(p, "hasMore") == &json!(false) {
                need(get(p, "nextCursor").is_null(), "PAGE_CURSOR")?;
                if let Some(cp) = checkpoint { self.projection_watermarks.borrow_mut().insert(base.to_string(), cp); }
                return Ok(all);
            }
            need(
                get(p, "nextCursor").is_string() && !seen_cursors.contains(get(p, "nextCursor").as_str().unwrap()),
                "PAGE_CURSOR",
            )?;
            seen_cursors.insert(get(p, "nextCursor").as_str().unwrap().to_string());
            cursor = Some(get(p, "nextCursor").as_str().unwrap().to_string());
        }
        Err("PAIRED_EVIDENCE_PAGES_INCOMPLETE".to_string())
    }
    fn account(&self, leg: &V) -> Result<V, String> {
        let slug = get(&self.scope, "tournamentSlug").as_str().unwrap_or("");
        let tid = get(&self.scope, "tournamentId").as_str().unwrap_or("");
        let cash = self.read(&format!("/tournaments/{slug}"))?;
        let positions = self.read(&format!("/tournaments/{slug}/portfolio/positions"))?;
        let open_orders_pages = self.pages(&format!("/orders?status=open&limit=200&tournamentId={tid}"), &mut |_, _| Ok(()))?;
        let scoped_orders_pages = self.pages(&format!("/orders?status=all&exchangeId={}&limit=200&tournamentId={tid}", get(leg, "exchangeId").as_str().unwrap_or("")), &mut |_, _| Ok(()))?;
        let result = json!({
            "cash": cash,
            "positions": positions,
            "openOrdersPages": open_orders_pages,
            "scopedOrdersPages": scoped_orders_pages,
        });
        self.validate_account(&result, leg, (self.now)())?;
        Ok(result)
    }
    fn order_check(&self, r: &V, order_id: &str, leg: &V, open: bool) -> Result<V, String> {
        let (d, _) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or("")).map_err(|_| "PAIRED_EVIDENCE_ORDER_SCOPE_OR_PHASE".to_string())?;
        need(
            integer(get(&d, "id"), 1)
                && baal_execution::js_key(get(&d, "id")) == order_id
                && get(&d, "exchangeId") == get(leg, "exchangeId")
                && get(&d, "tournamentId") == get(&self.scope, "tournamentId")
                && get(&d, "side") == &json!("no")
                && get(&d, "action") == &json!("buy")
                && get(&d, "quantity") == &json!(1)
                && get(&d, "open") == &json!(open)
                && instant(get(&d, "createdAt").as_str().unwrap_or("")).map(|x| x <= (self.now)() + 2000).unwrap_or(false)
                && instant(get(&d, "expirationDate").as_str().unwrap_or("")).map(|x| x > instant(get(&d, "createdAt").as_str().unwrap_or("")).unwrap_or(i64::MAX)).unwrap_or(false),
            "ORDER_SCOPE_OR_PHASE",
        )?;
        normalize_official_fill_tick(get(&d, "priceLimit")).map_err(|e| format!("PAIRED_EVIDENCE_{e}"))?;
        if !open {
            need(
                integer(get(&d, "quantityFilled"), 0) && js_int(get(&d, "quantityFilled")).unwrap() <= 1,
                "TERMINAL_QUANTITY",
            )?;
        }
        Ok(d)
    }
    fn run(&self, kind: &str, leg_value: &V, order_id: Option<V>, f: &mut dyn FnMut(&V) -> Result<V, String>) -> Result<V, String> {
        need(!*self.busy.borrow(), "BUSY")?;
        self.clear();
        self.allowed()?;
        let leg = exact_leg(&self.scope, leg_value)?;
        if let Some(o) = &order_id { self.own(o, &leg)?; }
        *self.busy.borrow_mut() = true;
        *self.operation.borrow_mut() = Some(kind.to_string());
        *self.reads.borrow_mut() = 0;
        *self.last_end.borrow_mut() = None;
        *self.last_failure.borrow_mut() = None;
        let result: Result<V, String> = (|| {
            need((self.has_read_headroom)(js_int(get(&self.max_reads_per_operation, kind)).unwrap_or(0))?, "READ_HEADROOM")?;
            let value = f(&leg)?;
            self.allowed()?;
            *self.cache.borrow_mut() = Some(value);
            Ok(json!({
                "kind": kind,
                "leg": copy(&leg),
                "orderId": order_id,
                "completedAt": iso((self.now)()),
                "readCount": *self.reads.borrow(),
            }))
        })();
        *self.busy.borrow_mut() = false;
        *self.operation.borrow_mut() = None;
        result.map_err(|e| {
            self.clear();
            let code = if e.starts_with("PAIRED_EVIDENCE_") { e } else { "PAIRED_EVIDENCE_READ_FAILED".to_string() };
            *self.last_failure.borrow_mut() = Some(code.clone());
            code
        })
    }
    pub fn clear_cache(&self) { self.clear(); }
    pub fn refresh_preparation(&self, leg: &V) -> Result<V, String> {
        self.run("preparation", leg, None, &mut |l| {
            let a = self.account(l)?;
            let info = self.validate_account(&a, l, (self.now)())?;
            let book = self.read(&format!("/markets/{}/orderbook?tournamentId={}&depth=20",
                get(l, "marketId").as_str().unwrap_or(""), get(&self.scope, "tournamentId").as_str().unwrap_or("")))?;
            let b = self.check_book(&book, l, (self.now)())?;
            need(
                instant(get(&b, "requestStartedAt").as_str().unwrap_or("")).map_err(|e| e)?
                    >= instant(get(&info, "completedAt").as_str().unwrap_or("")).map_err(|e| e)?,
                "BOOK_BEFORE_ACCOUNT",
            )?;
            self.validate_account(&a, l, (self.now)())?;
            self.book_watermarks.borrow_mut().insert(get(l, "exchangeId").as_str().unwrap_or("").to_string(), b);
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
    pub fn refresh_reconciliation(&self, order_id: &V, leg: &V) -> Result<V, String> {
        let oid_s = order_id.as_str().unwrap_or("").to_string();
        self.run("reconciliation", leg, Some(order_id.clone()), &mut |l| {
            let order = self.read(&format!("/orders/{oid_s}"))?;
            let od = self.order_check(&order, &oid_s, l, false)?;
            let mut totals: Option<String> = None;
            let od_created = instant(get(&od, "createdAt").as_str().unwrap_or("")).map_err(|e| e)?;
            let scope_tid = get(&self.scope, "tournamentId").as_str().unwrap_or("").to_string();
            let leg_ex = get(l, "exchangeId").clone();
            let fill_pages = self.pages(&format!("/orders/{oid_s}/fills?limit=200"), &mut |d, r| {
                need(
                    get(d, "orderId").as_i64().map(|x| x.to_string()) == Some(oid_s.clone())
                        && get(d, "exchangeId") == &leg_ex
                        && get(d, "tournamentId") == &json!(scope_tid)
                        && [0, -1].contains(&js_int(get(d, "totalQuantityFilled")).unwrap_or(-2)),
                    "FILL_SCOPE",
                )?;
                let avg = if get(d, "avgFillPrice").is_null() { V::Null } else {
                    get(&normalize_official_fill_tick(get(d, "avgFillPrice"))?, "price").clone()
                };
                let identity = payload_hash(&json!([get(d, "totalQuantityFilled"), avg]))?;
                need(totals.as_ref().is_none_or(|t| t == &identity), "FILL_TOTAL_CHANGED")?;
                totals = Some(identity);
                for f in get(d, "data").as_array().cloned().unwrap_or_default() {
                    need(
                        get(&f, "side") == &json!("no")
                            && get(&f, "quantity") == &json!(-1)
                            && instant(get(&f, "filledAt").as_str().unwrap_or("")).unwrap_or(i64::MIN) >= od_created
                            && instant(get(&f, "filledAt").as_str().unwrap_or("")).unwrap_or(i64::MAX) <= instant(get(r, "receivedAt").as_str().unwrap_or("")).unwrap_or(i64::MIN),
                        "FILL_SCHEMA",
                    )?;
                    normalize_official_fill_tick(get(&f, "price"))?;
                }
                Ok(())
            })?;
            let decoded: Vec<V> = fill_pages.iter()
                .map(|r| parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or("")).map(|(d, _)| d))
                .collect::<Result<Vec<V>, _>>()?;
            let fills: Vec<V> = decoded.iter().flat_map(|d| get(d, "data").as_array().cloned().unwrap_or_default()).collect();
            let last = decoded.last().cloned().unwrap_or(V::Null);
            need(
                fills.len() <= 1
                    && fills.len() as i64 == js_int(get(&od, "quantityFilled")).unwrap_or(-2)
                    && js_int(get(&last, "totalQuantityFilled")) == Some(-(fills.len() as i64))
                    && (if fills.is_empty() {
                        get(&last, "avgFillPrice").is_null()
                    } else {
                        get(&normalize_official_fill_tick(get(&last, "avgFillPrice"))?, "price")
                            == get(&normalize_official_fill_tick(get(&fills[0], "price"))?, "price")
                            && amount(get(&normalize_official_fill_tick(get(&fills[0], "price"))?, "price").as_str().unwrap_or("")).unwrap_or(i128::MAX)
                                <= amount(get(&normalize_official_fill_tick(get(&od, "priceLimit"))?, "price").as_str().unwrap_or("")).unwrap_or(i128::MIN)
                    }),
                "FILL_TOTAL_MISMATCH",
            )?;
            let after = self.account(l)?;
            need(
                instant(get(&after, "cash").get("requestStartedAt").and_then(|v| v.as_str()).unwrap_or("")).map_err(|e| e)?
                    >= instant(get(&fill_pages.last().cloned().unwrap_or(V::Null), "receivedAt").as_str().unwrap_or("")).map_err(|e| e)?,
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
        let mut ctx = json!({"operation": "reconcile", "orderId": order_id});
        let e = exact_leg(&self.scope, leg)?;
        for (k, v) in e.as_object().unwrap() { ctx.as_object_mut().unwrap().insert(k.clone(), v.clone()); }
        self.get_evidence(&ctx)
    }
    pub fn refresh_cancel(&self, order_id: &V, leg: &V) -> Result<V, String> {
        let oid_s = order_id.as_str().unwrap_or("").to_string();
        self.run("cancel", leg, Some(order_id.clone()), &mut |l| {
            let order = self.read(&format!("/orders/{oid_s}"))?;
            self.order_check(&order, &oid_s, l, true)?;
            Ok(json!({"kind": "cancel", "leg": l, "orderId": order_id, "order": order}))
        })
    }
    pub fn status(&self) -> V {
        let cache = self.cache.borrow();
        json!({
            "mode": "paired_rest_evidence_bridge_v1",
            "busy": *self.busy.borrow(),
            "closed": *self.closed.borrow(),
            "cachedKind": cache.as_ref().and_then(|c| get(c, "kind").as_str().map(String::from)),
            "cachedLeg": cache.as_ref().map(|c| copy(get(c, "leg"))).unwrap_or(V::Null),
            "lastReadCount": *self.reads.borrow(),
            "lastFailure": self.last_failure.borrow().clone(),
            "additionalApiClients": 0,
            "executionAuthority": false,
        })
    }
    pub fn close(&self) -> Result<(), String> {
        need(!*self.busy.borrow(), "BUSY")?;
        self.clear();
        *self.closed.borrow_mut() = true;
        self.receipt_watermarks.borrow_mut().clear();
        self.book_watermarks.borrow_mut().clear();
        self.projection_watermarks.borrow_mut().clear();
        Ok(())
    }
}

fn fresh(r: &V, n: i64) -> Result<(), String> {
    let a = instant(get(r, "requestStartedAt").as_str().unwrap_or("")).map_err(|_| "PAIRED_EVIDENCE_STALE_OR_FUTURE_RECEIPT".to_string())?;
    let z = instant(get(r, "receivedAt").as_str().unwrap_or("")).map_err(|_| "PAIRED_EVIDENCE_STALE_OR_FUTURE_RECEIPT".to_string())?;
    need(a <= z && z <= n && n - a <= 20000, "STALE_OR_FUTURE_RECEIPT")
}
fn exact_leg(scope: &V, v: &V) -> Result<V, String> {
    need(
        v.is_object()
            && get(scope, "markets").as_array().map(|m| m.iter().any(|x| get(x, "marketId") == get(v, "marketId") && get(x, "exchangeId") == get(v, "exchangeId"))).unwrap_or(false),
        "LEG_SCOPE",
    )?;
    Ok(json!({"marketId": get(v, "marketId"), "exchangeId": get(v, "exchangeId")}))
}
fn uuid_like(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    let lens = [8usize, 4, 4, 4, 12];
    parts.len() == 5 && parts.iter().zip(lens.iter()).all(|(p, l)| p.len() == *l && p.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn slug_re(s: &str) -> bool {
    !s.is_empty() && s.split('-').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

fn to_terr(msg: &str) -> baal_execution::transport::OrderTransportError {
    baal_execution::transport::OrderTransportError {
        code: msg.to_string(),
        status: None,
        api_code: None,
        outcome: "rejected".into(),
        may_have_executed: false,
        retry_same_request_only: false,
        retry_after_ms: 0.0,
        semantic_mismatch: false,
    }
}

impl crate::paired_rest_cycle::PairedCycleBridge for PairedRestEvidenceBridge {
    fn refresh_preparation(&self, leg: &V) -> Result<V, String> { self.refresh_preparation(leg) }
    fn refresh_reconciliation(&self, order_id: &V, leg: &V) -> Result<V, String> { self.refresh_reconciliation(order_id, leg) }
    fn refresh_cancel(&self, order_id: &V, leg: &V) -> Result<V, String> { self.refresh_cancel(order_id, leg) }
}
