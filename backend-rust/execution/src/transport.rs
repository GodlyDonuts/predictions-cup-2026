use baal_service_runtime::json_text::js_int;

use serde_json::{json, Map, Value as V};
use std::collections::{BTreeMap, BTreeSet};

// execution/transport.mjs — OrderTransport: the validated dispatcher adapter.
// It owns no credentials, fetch, timer, limiter or retry loop: `dispatch` is
// the collector's sole shared queue and beforeDispatch/beforeNetwork are
// invoked exactly where the JS code fences them.
pub const API_CODES: [&str; 27] = [
    "MISSING_API_KEY", "INVALID_API_KEY", "API_KEY_REVOKED", "API_KEY_EXPIRED",
    "UNAUTHORIZED", "INSUFFICIENT_SCOPES", "ACCOUNT_BANNED",
    "RESIDENCE_UPDATE_REQUIRED", "ADMIN_REQUIRED", "FORBIDDEN",
    "TERMS_NOT_ACKNOWLEDGED", "RATE_LIMITED", "INVALID_JSON", "INVALID_ID",
    "INVALID_CURSOR", "VALIDATION_ERROR", "ORDER_CONFLICT", "NOT_FOUND",
    "CONFLICT", "REQUEST_IN_FLIGHT", "TERMINAL_ENGINE_SCOPE",
    "RELATIONSHIP_VIOLATION", "DATABASE_ERROR", "INTERNAL_ERROR",
    "ORDER_STATUS_UNKNOWN", "TX_CONFLICT", "SERVICE_UNAVAILABLE",
];
const BODY_KEYS: [&str; 8] = [
    "idempotencyKey", "exchangeId", "side", "action", "quantity", "price",
    "expirationDate", "tournamentId",
];

#[derive(Debug)]
pub struct OrderTransportError {
    pub code: String,
    pub status: Option<i64>,
    pub api_code: Option<String>,
    pub outcome: String,
    pub may_have_executed: bool,
    pub retry_same_request_only: bool,
    pub retry_after_ms: f64,
    pub semantic_mismatch: bool,
}
impl std::fmt::Display for OrderTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.code)
    }
}
impl std::error::Error for OrderTransportError {}
fn err(code: &str) -> OrderTransportError {
    OrderTransportError {
        code: code.into(),
        status: None,
        api_code: None,
        outcome: "blocked".into(),
        may_have_executed: false,
        retry_same_request_only: false,
        retry_after_ms: 0.0,
        semantic_mismatch: false,
    }
}
fn need(ok: bool, code: &str) -> Result<(), OrderTransportError> {
    if ok { Ok(()) } else { Err(err(code)) }
}
fn numeric_id(v: &V) -> bool {
    v.as_str()
        .is_some_and(|s| !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit()) && js_int(&json!(s.parse::<f64>().unwrap_or(f64::NAN))).is_some())
}
fn order_id(v: &V) -> bool {
    numeric_id(v) || js_int(v).is_some_and(|n| n > 0)
}
fn object(v: &V) -> bool {
    v.is_object()
}
fn finite(v: &V) -> bool {
    v.as_f64().is_some_and(|x| x.is_finite())
}
fn clock(v: &V) -> bool {
    v.as_str().is_some_and(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok())
}
fn parse(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()
}

pub struct TransportScope {
    pub tournament_id: String,
    pub tournament_slug: String,
    pub by_exchange: BTreeMap<String, String>, // exchangeId -> marketId
    pub by_market: BTreeMap<String, Vec<String>>, // marketId -> [exchangeId]
}
pub fn scope_of(scope: &V) -> Result<TransportScope, OrderTransportError> {
    let tid = scope.get("tournamentId").and_then(V::as_str).unwrap_or("");
    let slug = scope.get("tournamentSlug").and_then(V::as_str).unwrap_or("");
    let slug_ok = !slug.is_empty()
        && slug.split('-').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()));
    let exchanges = scope.get("exchanges").and_then(V::as_array);
    need(
        tid.len() == 36
            && tid.bytes().enumerate().all(|(i, b)| {
                if [8, 13, 18, 23].contains(&i) { b == b'-' } else { b.is_ascii_hexdigit() }
            })
            && slug_ok
            && exchanges.is_some_and(|a| !a.is_empty()),
        "INVALID_EXECUTION_SCOPE",
    )?;
    let mut by_exchange = BTreeMap::new();
    let mut by_market: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for value in exchanges.unwrap() {
        let e = value.get("exchangeId").unwrap_or(&V::Null);
        let m = value.get("marketId").unwrap_or(&V::Null);
        need(
            numeric_id(e) && numeric_id(m) && !by_exchange.contains_key(e.as_str().unwrap()),
            "INVALID_EXECUTION_SCOPE",
        )?;
        let (e, m) = (e.as_str().unwrap().to_string(), m.as_str().unwrap().to_string());
        by_exchange.insert(e.clone(), m.clone());
        by_market.entry(m).or_default().push(e);
    }
    Ok(TransportScope { tournament_id: tid.into(), tournament_slug: slug.into(), by_exchange, by_market })
}
pub fn body_of(body: &V, scope: &TransportScope) -> Result<V, OrderTransportError> {
    let m = body.as_object();
    need(
        m.is_some()
            && m.unwrap().len() == BODY_KEYS.len()
            && m.unwrap().keys().all(|k| BODY_KEYS.contains(&k.as_str())),
        "INVALID_LIMIT_ORDER",
    )?;
    let key = body.get("idempotencyKey").and_then(V::as_str).unwrap_or("");
    need(
        !key.is_empty() && key.len() <= 255 && !key.bytes().any(|b| b <= 0x1f || b == 0x7f),
        "INVALID_IDEMPOTENCY_KEY",
    )?;
    need(
        body.get("tournamentId").and_then(V::as_str) == Some(scope.tournament_id.as_str())
            && scope.by_exchange.contains_key(body.get("exchangeId").and_then(V::as_str).unwrap_or("")),
        "ORDER_SCOPE_MISMATCH",
    )?;
    need(
        ["yes", "no"].contains(&body.get("side").and_then(V::as_str).unwrap_or(""))
            && ["buy", "sell"].contains(&body.get("action").and_then(V::as_str).unwrap_or(""))
            && js_int(body.get("quantity").unwrap_or(&V::Null)).is_some_and(|q| q > 0 && q <= 2_147_483_647),
        "INVALID_LIMIT_ORDER",
    )?;
    let p = body.get("price").and_then(V::as_f64).unwrap_or(f64::NAN);
    need(
        p.is_finite() && p >= 0.005 && p <= 0.995 && (p * 200.0 - p.mul_add(0.0, p * 200.0).round()).abs() < 1e-8,
        "LIMIT_PRICE_REQUIRED",
    )?;
    let exp = body.get("expirationDate").and_then(V::as_str).unwrap_or("");
    need(
        clock(&json!(exp))
            && exp.len() >= 20
            && exp.as_bytes().get(4) == Some(&b'-')
            && exp.as_bytes().get(10) == Some(&b'T')
            && (exp.ends_with('Z') || {
                let l = exp.len();
                l >= 6 && (exp.as_bytes()[l - 6] == b'+' || exp.as_bytes()[l - 6] == b'-')
            }),
        "LIMIT_EXPIRY_REQUIRED",
    )?;
    Ok(V::Object(Map::from_iter(
        BODY_KEYS
            .iter()
            .map(|k| (k.to_string(), body.get(*k).unwrap().clone())),
    )))
}
// canonicalBody: JS Object.keys().sort() (lexicographic UTF-16), one level deep.
pub fn canonical_body(body: &V) -> String {
    let m = body.as_object().unwrap();
    let mut keys: Vec<&String> = m.keys().collect();
    keys.sort();
    let parts: Vec<String> = keys
        .iter()
        .map(|k| format!("{}:{}", serde_json::to_string(k).unwrap(), baal_service_runtime::json_text::js_stringify(&m[*k])))
        .collect();
    format!("{{{}}}", parts.join(","))
}
fn cursor_suffix(cursor: Option<&V>) -> Result<String, OrderTransportError> {
    match cursor {
        None | Some(V::Null) => Ok(String::new()),
        Some(c) => {
            let s = c.as_str().unwrap_or("");
            need(
                !s.is_empty() && s.len() <= 4096 && !s.bytes().any(|b| b <= 0x1f || b == 0x7f),
                "INVALID_PAGINATION_CURSOR",
            )?;
            Ok(format!("&cursor={}", baal_service_runtime::history::encode_uri_component(s)))
        }
    }
}
fn encoded_cursor(endpoint: &str, base: &str) -> bool {
    if endpoint == base {
        return true;
    }
    let prefix = format!("{base}&cursor=");
    if !endpoint.starts_with(&prefix) {
        return false;
    }
    let encoded = &endpoint[prefix.len()..];
    match percent_decode(encoded) {
        Some(decoded) => match cursor_suffix(Some(&json!(decoded))) {
            Ok(suffix) => endpoint == format!("{base}{suffix}"),
            Err(_) => false,
        },
        None => false,
    }
}
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hi = (bytes[i + 1] as char).to_digit(16)?;
            let lo = (bytes[i + 2] as char).to_digit(16)?;
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn status_error(value: &V, method: &str) -> OrderTransportError {
    let status = value
        .get("status")
        .and_then(V::as_i64)
        .filter(|s| (400..=599).contains(s));
    let supplied = value
        .get("apiCode")
        .or_else(|| value.get("code"))
        .or_else(|| value.pointer("/data/error/code"))
        .and_then(V::as_str);
    let api_code = supplied.filter(|c| API_CODES.contains(c)).map(str::to_string);
    let wait = value
        .get("retryAfterMs")
        .and_then(V::as_f64)
        .filter(|w| w.is_finite() && *w >= 0.0)
        .unwrap_or(0.0);
    let writing = method != "GET";
    if status == Some(429) {
        return OrderTransportError {
            code: "RATE_LIMITED".into(),
            status,
            api_code,
            outcome: "rate_limited".into(),
            retry_same_request_only: writing,
            retry_after_ms: wait.max(60000.0),
            ..err("")
        };
    }
    if status == Some(409) && api_code.as_deref() == Some("REQUEST_IN_FLIGHT") {
        return OrderTransportError {
            code: "REQUEST_IN_FLIGHT".into(),
            status,
            api_code,
            outcome: "in_flight".into(),
            may_have_executed: writing,
            retry_same_request_only: writing,
            retry_after_ms: wait.max(90000.0),
            ..err("")
        };
    }
    let uncertain = writing && (status.is_none() || status.unwrap() >= 500);
    OrderTransportError {
        code: if status.is_none() { "REQUEST_UNCONFIRMED" } else { "OFFICIAL_REQUEST_REJECTED" }.into(),
        status,
        api_code,
        outcome: if uncertain { "unknown" } else if status == Some(409) { "conflict" } else { "rejected" }.into(),
        may_have_executed: uncertain,
        retry_same_request_only: uncertain,
        retry_after_ms: wait,
        ..err("")
    }
}

// `dispatch` receives the spec plus the two write fences. It must invoke
// before_dispatch at queue-admission and before_network immediately before the
// network call — inside the collector's serialized admission — or the write
// fails DISPATCH_GUARD_NOT_CALLED.
// `for<'g>` lets each call pass guards of any lifetime.
pub type Guard<'a> = dyn Fn(&V, &V) -> Result<bool, OrderTransportError> + 'a;
pub type Dispatch = dyn for<'g> Fn(
    &V,
    Option<&'g Guard<'g>>,
    Option<&'g Guard<'g>>,
) -> Result<V, OrderTransportError>;
pub type TimeFn = dyn Fn() -> i64;

pub struct OrderTransport {
    dispatch: Box<Dispatch>,
    scope: TransportScope,
    can_write: Box<dyn Fn() -> bool>,
    owns_order: Box<dyn Fn(&str) -> bool>,
    validate_permit: Box<Guard<'static>>,
    validate_lease: Box<Guard<'static>>,
    validate_network_permit: Box<Guard<'static>>,
    now: Box<TimeFn>,
    max_pages: usize,
    logical_bodies: std::cell::RefCell<BTreeMap<String, String>>,
    key_bodies: std::cell::RefCell<BTreeMap<String, String>>,
}
pub struct OrderTransportOptions {
    pub dispatch: Box<Dispatch>,
    pub scope: TransportScope,
    pub can_write: Box<dyn Fn() -> bool>,
    pub owns_order: Box<dyn Fn(&str) -> bool>,
    pub validate_permit: Box<Guard<'static>>,
    pub validate_lease: Box<Guard<'static>>,
    pub validate_network_permit: Box<Guard<'static>>,
    pub now: Box<TimeFn>,
    pub max_pages: usize,
}
impl OrderTransport {
    pub fn new(o: OrderTransportOptions) -> Result<Self, OrderTransportError> {
        need(o.max_pages >= 1 && o.max_pages <= 1000, "INVALID_TRANSPORT_DEPENDENCIES")?;
        Ok(Self {
            dispatch: o.dispatch,
            scope: o.scope,
            can_write: o.can_write,
            owns_order: o.owns_order,
            validate_permit: o.validate_permit,
            validate_lease: o.validate_lease,
            validate_network_permit: o.validate_network_permit,
            now: o.now,
            max_pages: o.max_pages,
            logical_bodies: std::cell::RefCell::new(BTreeMap::new()),
            key_bodies: std::cell::RefCell::new(BTreeMap::new()),
        })
    }
    fn owned(&self, id: &V) -> Result<String, OrderTransportError> {
        need(order_id(id), "INVALID_ORDER_ID")?;
        let s = id.as_str().map(str::to_string).unwrap_or_else(|| js_int(id).unwrap().to_string());
        let owned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.owns_order)(&s)))
            .unwrap_or(false);
        need(owned, "ORDER_NOT_OWNED")?;
        Ok(s)
    }
    pub fn authorize_request(&self, spec: &V) -> Result<bool, OrderTransportError> {
        need(
            object(spec)
                && spec.as_object().unwrap().keys().all(|k| {
                    ["method", "endpoint", "body", "bodyJson", "beforeDispatch", "beforeNetwork"]
                        .contains(&k.as_str())
                })
                && spec.get("endpoint").and_then(V::as_str).is_some(),
            "REQUEST_OUTSIDE_ALLOWLIST",
        )?;
        let method = spec.get("method").and_then(V::as_str).unwrap_or("");
        let endpoint = spec.get("endpoint").and_then(V::as_str).unwrap();
        if method == "POST" && endpoint == "/orders" {
            body_of(spec.get("body").unwrap_or(&V::Null), &self.scope)?;
            if let Some(bj) = spec.get("bodyJson") {
                need(
                    bj == &json!(canonical_body(spec.get("body").unwrap())),
                    "FROZEN_ORDER_BODY_MISMATCH",
                )?;
            }
            return Ok(true);
        }
        let own = strip_order_id(endpoint);
        if method == "DELETE" && own.is_some() {
            need(
                spec.get("body").is_none() && spec.get("bodyJson").is_none(),
                "WRITE_GUARD_REQUIRED",
            )?;
            self.owned(&json!(own.clone().unwrap()))?;
            return Ok(true);
        }
        need(
            method == "GET" && spec.get("body").is_none() && spec.get("bodyJson").is_none(),
            "REQUEST_OUTSIDE_ALLOWLIST",
        )?;
        if endpoint == format!("/tournaments/{}", self.scope.tournament_slug)
            || endpoint == format!("/tournaments/{}/portfolio/positions", self.scope.tournament_slug)
        {
            return Ok(true);
        }
        if let Some(cap) = market_route(endpoint) {
            let (market_id, kind) = cap;
            let allowed_tail = self.scope.by_market.contains_key(&market_id)
                && endpoint
                    == format!(
                        "/markets/{market_id}{kind}?tournamentId={}{}",
                        self.scope.tournament_id,
                        if kind == "/orderbook" { "&depth=20" } else { "" }
                    );
            if allowed_tail {
                return Ok(true);
            }
        }
        for status in ["all", "open", "closed", "expired"] {
            if encoded_cursor(
                endpoint,
                &format!("/orders?status={status}&limit=200&tournamentId={}", self.scope.tournament_id),
            ) {
                return Ok(true);
            }
        }
        if let Some(oid) = own {
            self.owned(&json!(oid))?;
            return Ok(true);
        }
        if let Some(oid) = strip_fills_id(endpoint) {
            if encoded_cursor(endpoint, &format!("/orders/{oid}/fills?limit=200")) {
                self.owned(&json!(oid))?;
                return Ok(true);
            }
        }
        Err(err("REQUEST_OUTSIDE_ALLOWLIST"))
    }
    fn request(
        &self,
        spec: &V,
        before_dispatch: Option<&Guard<'_>>,
        before_network: Option<&Guard<'_>>,
    ) -> Result<V, OrderTransportError> {
        self.authorize_request(spec)?;
        let method = spec.get("method").and_then(V::as_str).unwrap_or("");
        let response = match (self.dispatch)(spec, before_dispatch, before_network) {
            Ok(r) => r,
            Err(e) => return Err(e),
        };
        let status = response.get("status").and_then(V::as_i64);
        if !response.is_object() || !status.is_some_and(|s| (200..300).contains(&s)) {
            return Err(status_error(&response, method));
        }
        let started = response.get("requestStartedAt").and_then(V::as_str);
        let received = response.get("receivedAt").and_then(V::as_str);
        if !(started.is_some_and(|s| clock(&json!(s)))
            && received.is_some_and(|s| clock(&json!(s)))
            && parse(received.unwrap()) >= parse(started.unwrap())
            && response.get("data").is_some_and(V::is_object))
        {
            return Err(OrderTransportError {
                code: "INVALID_OFFICIAL_RESPONSE".into(),
                status,
                outcome: if method == "GET" { "rejected".into() } else { "unknown".into() },
                may_have_executed: method != "GET",
                semantic_mismatch: method != "GET",
                ..err("")
            });
        }
        Ok(response)
    }
    fn scoped_exchange(&self, exchange_id: &V) -> Result<(), OrderTransportError> {
        need(
            exchange_id.as_str().is_some_and(|s| self.scope.by_exchange.contains_key(s)),
            "RESPONSE_SCOPE_MISMATCH",
        )
    }
    fn coverage(&self, coverage: &V, required: bool) -> Result<(), OrderTransportError> {
        if coverage.is_null() && !required {
            return Ok(());
        }
        need(
            coverage.is_object()
                && coverage.get("complete") == Some(&V::Bool(true))
                && (coverage.get("projectedThroughSequence").unwrap_or(&V::Null).is_null()
                    || js_int(coverage.get("projectedThroughSequence").unwrap())
                        .is_some_and(|n| n >= 0)),
            "INCOMPLETE_OFFICIAL_COVERAGE",
        )
    }
    fn validate_order_row(&self, row: &V, expected_id: Option<&str>) -> Result<(), OrderTransportError> {
        let id_ok = order_id(row.get("id").unwrap_or(&V::Null))
            && (expected_id.is_none()
                || row.get("id").and_then(V::as_str).map(str::to_string).or_else(|| row.get("id").and_then(V::as_i64).map(|n| n.to_string())).as_deref()
                    == expected_id);
        need(
            object(row) && id_ok && row.get("tournamentId").and_then(V::as_str) == Some(self.scope.tournament_id.as_str()),
            "RESPONSE_SCOPE_MISMATCH",
        )?;
        self.scoped_exchange(row.get("exchangeId").unwrap_or(&V::Null))?;
        need(
            ["yes", "no"].contains(&row.get("side").and_then(V::as_str).unwrap_or(""))
                && ["buy", "sell"].contains(&row.get("action").and_then(V::as_str).unwrap_or(""))
                && js_int(row.get("quantity").unwrap_or(&V::Null)).is_some_and(|q| q >= 0)
                && row.get("open").and_then(V::as_bool).is_some()
                && (row.get("priceLimit").unwrap_or(&V::Null).is_null()
                    || row.get("priceLimit").and_then(V::as_f64).is_some_and(|p| p.is_finite() && (0.0..=1.0).contains(&p)))
                && clock(row.get("createdAt").unwrap_or(&V::Null))
                && (row.get("expirationDate").unwrap_or(&V::Null).is_null()
                    || clock(row.get("expirationDate").unwrap())),
            "INVALID_OFFICIAL_ORDER",
        )
    }
    fn market_context(&self, data: &V, market_id: &str, book: bool) -> Result<(), OrderTransportError> {
        need(
            object(data)
                && (book || data.get("id").and_then(V::as_str) == Some(market_id) || data.get("id").and_then(V::as_i64).map(|n| n.to_string()).as_deref() == Some(market_id))
                && data.get("contexts").and_then(V::as_array).is_some_and(|c| {
                    c.len() == 1
                        && c[0].get("type").and_then(V::as_str) == Some("tournament")
                        && c[0].pointer("/tournament/id").and_then(V::as_str) == Some(self.scope.tournament_id.as_str())
                }),
            "RESPONSE_SCOPE_MISMATCH",
        )?;
        let exchanges = if book {
            data.pointer("/contexts/0/orderbook/exchanges")
        } else {
            data.get("exchanges")
        };
        need(
            exchanges.and_then(V::as_array).is_some_and(|a| {
                a.len() == self.scope.by_market.get(market_id).map(|v| v.len()).unwrap_or(usize::MAX)
            }),
            "RESPONSE_SCOPE_MISMATCH",
        )?;
        let mut seen = BTreeSet::new();
        for value in exchanges.and_then(V::as_array).unwrap() {
            let exchange_id = if book {
                value.get("exchangeId").and_then(V::as_str)
            } else {
                value.get("id").and_then(V::as_str)
            };
            need(
                exchange_id.is_some()
                    && self.scope.by_exchange.get(exchange_id.unwrap()).map(|m| m.as_str()) == Some(market_id)
                    && !seen.contains(exchange_id.unwrap()),
                "RESPONSE_SCOPE_MISMATCH",
            )?;
            seen.insert(exchange_id.unwrap().to_string());
        }
        Ok(())
    }
    pub fn tournament(&self) -> Result<V, OrderTransportError> {
        let response = self.request(&json!({
            "method": "GET",
            "endpoint": format!("/tournaments/{}", self.scope.tournament_slug),
        }), None, None)?;
        let data = response.get("data").unwrap();
        need(
            data.get("id").and_then(V::as_str) == Some(self.scope.tournament_id.as_str())
                && data.get("slug").and_then(V::as_str) == Some(self.scope.tournament_slug.as_str())
                && data.get("myBalance").and_then(V::as_f64).is_some_and(|b| b.is_finite() && b >= 0.0)
                && data.get("status").and_then(V::as_str).is_some()
                && data.get("isPendingEnrolment").and_then(V::as_bool).is_some(),
            "RESPONSE_SCOPE_MISMATCH",
        )?;
        Ok(response)
    }
    pub fn market(&self, market_id: &str) -> Result<V, OrderTransportError> {
        let response = self.request(&json!({
            "method": "GET",
            "endpoint": format!("/markets/{market_id}?tournamentId={}", self.scope.tournament_id),
        }), None, None)?;
        self.market_context(response.get("data").unwrap(), market_id, false)?;
        Ok(response)
    }
    pub fn orderbook(&self, market_id: &str) -> Result<V, OrderTransportError> {
        let response = self.request(&json!({
            "method": "GET",
            "endpoint": format!("/markets/{market_id}/orderbook?tournamentId={}&depth=20", self.scope.tournament_id),
        }), None, None)?;
        self.market_context(response.get("data").unwrap(), market_id, true)?;
        Ok(response)
    }
    pub fn nodes(&self, market_id: &str) -> Result<V, OrderTransportError> {
        let response = self.request(&json!({
            "method": "GET",
            "endpoint": format!("/markets/{market_id}/nodes?tournamentId={}", self.scope.tournament_id),
        }), None, None)?;
        let data = response.get("data").unwrap();
        need(
            data.get("market_id").and_then(V::as_str) == Some(market_id)
                && data.get("contexts").and_then(V::as_array).is_some_and(|c| {
                    c.len() == 1
                        && c[0].get("type").and_then(V::as_str) == Some("tournament")
                        && c[0].pointer("/tournament/id").and_then(V::as_str) == Some(self.scope.tournament_id.as_str())
                })
                && data.get("root").is_some_and(V::is_object)
                && data.pointer("/root/node_type").and_then(V::as_str) == Some("contract")
                && data.pointer("/root/contract_type").and_then(V::as_str) == Some("Election Outcome"),
            "RESPONSE_SCOPE_MISMATCH",
        )?;
        Ok(response)
    }
    pub fn positions(&self) -> Result<V, OrderTransportError> {
        let response = self.request(&json!({
            "method": "GET",
            "endpoint": format!("/tournaments/{}/portfolio/positions", self.scope.tournament_slug),
        }), None, None)?;
        let data = response.get("data").unwrap();
        let positions = data.get("positions").and_then(V::as_array);
        need(
            positions.is_some()
                && data.get("summary").is_some_and(V::is_object)
                && ["totalMarketValue", "totalCostBasis", "totalUnrealizedPnl"]
                    .iter()
                    .all(|k| finite(data.pointer(&format!("/summary/{k}")).unwrap_or(&V::Null)))
                && data.get("pagination").is_none(),
            "INCOMPLETE_OFFICIAL_POSITIONS",
        )?;
        let mut seen = BTreeSet::new();
        for row in positions.unwrap() {
            self.scoped_exchange(row.get("exchangeId").unwrap_or(&V::Null))?;
            need(
                self.scope
                    .by_exchange
                    .get(row.get("exchangeId").and_then(V::as_str).unwrap_or(""))
                    .map(|m| m.as_str())
                    == row.get("marketId").and_then(V::as_str)
                    && !seen.contains(row.get("exchangeId").and_then(V::as_str).unwrap_or(""))
                    && js_int(row.get("quantity").unwrap_or(&V::Null)).is_some()
                    && row.get("lots").is_some_and(V::is_array),
                "INVALID_OFFICIAL_POSITION",
            )?;
            seen.insert(row.get("exchangeId").and_then(V::as_str).unwrap().to_string());
        }
        Ok(response)
    }
    pub fn order(&self, id: &V) -> Result<V, OrderTransportError> {
        let id = self.owned(id)?;
        let response = self.request(&json!({
            "method": "GET",
            "endpoint": format!("/orders/{id}"),
        }), None, None)?;
        self.validate_order_row(response.get("data").unwrap(), Some(&id))?;
        Ok(response)
    }
    fn pages(
        &self,
        base: &str,
        validate: impl Fn(&V, &V) -> Result<(), OrderTransportError>,
        identity: impl Fn(&V) -> String,
    ) -> Result<V, OrderTransportError> {
        let mut rows = Vec::new();
        let mut pages = Vec::new();
        let mut cursors = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut cursor: Option<V> = None;
        let mut first: Option<V> = None;
        for _ in 0..self.max_pages {
            let suffix = cursor_suffix(cursor.as_ref())?;
            let response = self.request(&json!({
                "method": "GET",
                "endpoint": format!("{base}{suffix}"),
            }), None, None)?;
            let data = response.get("data").unwrap().clone();
            if first.is_none() {
                first = Some(response.clone());
            }
            validate(&data, first.as_ref().unwrap().get("data").unwrap())?;
            let page = data.get("pagination").unwrap_or(&V::Null);
            need(
                data.get("data").is_some_and(V::is_array)
                    && page.is_object()
                    && js_int(page.get("limit").unwrap_or(&V::Null)).is_some_and(|l| (1..=200).contains(&l))
                    && page.get("hasMore").and_then(V::as_bool).is_some(),
                "INCOMPLETE_OFFICIAL_PAGINATION",
            )?;
            for row in data.get("data").unwrap().as_array().unwrap() {
                let key = identity(row);
                need(!seen.contains(&key), "INCONSISTENT_OFFICIAL_PAGINATION")?;
                seen.insert(key);
                rows.push(row.clone());
            }
            pages.push(json!({
                "receivedAt": response.get("receivedAt").unwrap().clone(),
                "coverage": data.get("coverage").cloned().unwrap_or(V::Null),
            }));
            let next = page.get("nextCursor").cloned().unwrap_or(V::Null);
            if page.get("hasMore") == Some(&V::Bool(false)) {
                need(next.is_null(), "INCOMPLETE_OFFICIAL_PAGINATION")?;
                let mut last = response.clone();
                let last_data = last.get_mut("data").unwrap().as_object_mut().unwrap();
                last_data.insert("data".into(), json!(rows));
                last_data.insert(
                    "pagination".into(),
                    json!({"limit": 200, "hasMore": false, "nextCursor": null}),
                );
                last.as_object_mut().unwrap().insert(
                    "requestStartedAt".into(),
                    first.as_ref().unwrap().get("requestStartedAt").unwrap().clone(),
                );
                last.as_object_mut().unwrap().insert("pages".into(), json!(pages));
                return Ok(last);
            }
            let next_s = next.as_str().unwrap_or("");
            cursor_suffix(Some(&next))?;
            need(
                !next_s.is_empty() && !cursors.contains(next_s) && !data.get("data").unwrap().as_array().unwrap().is_empty(),
                "INCOMPLETE_OFFICIAL_PAGINATION",
            )?;
            cursors.insert(next_s.to_string());
            cursor = Some(next);
        }
        Err(err("PAGINATION_LIMIT_EXCEEDED"))
    }
    pub fn orders(&self, status: &str) -> Result<V, OrderTransportError> {
        need(["all", "open", "closed", "expired"].contains(&status), "INVALID_ORDER_STATUS")?;
        let base = format!("/orders?status={status}&limit=200&tournamentId={}", self.scope.tournament_id);
        self.pages(
            &base,
            |data, _first| {
                self.coverage(data.get("coverage").unwrap_or(&V::Null), false)?;
                need(data.get("data").is_some_and(V::is_array), "INCOMPLETE_OFFICIAL_PAGINATION")?;
                for row in data.get("data").unwrap().as_array().unwrap() {
                    self.validate_order_row(row, None)?;
                }
                Ok(())
            },
            |row| row.get("id").map(|v| match v {
                V::String(s) => s.clone(),
                V::Number(n) => n.to_string(),
                _ => "null".into(),
            }).unwrap_or_default(),
        )
    }
    pub fn fills(&self, id: &V) -> Result<V, OrderTransportError> {
        let id = self.owned(id)?;
        let base = format!("/orders/{id}/fills?limit=200");
        self.pages(
            &base,
            |data, first| {
                need(
                    data.get("orderId").map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.as_i64().map(|n| n.to_string()).unwrap_or_default())).as_deref() == Some(id.as_str())
                        && data.get("tournamentId").and_then(V::as_str) == Some(self.scope.tournament_id.as_str()),
                    "RESPONSE_SCOPE_MISMATCH",
                )?;
                self.scoped_exchange(data.get("exchangeId").unwrap_or(&V::Null))?;
                self.coverage(data.get("coverage").unwrap_or(&V::Null), true)?;
                need(
                    finite(data.get("totalQuantityFilled").unwrap_or(&V::Null))
                        && (data.get("avgFillPrice").unwrap_or(&V::Null).is_null()
                            || data.get("avgFillPrice").and_then(V::as_f64).is_some_and(|p| p.is_finite() && (0.0..=1.0).contains(&p)))
                        && data.get("totalQuantityFilled") == first.get("totalQuantityFilled")
                        && data.get("avgFillPrice") == first.get("avgFillPrice")
                        && data.get("exchangeId") == first.get("exchangeId"),
                    "INCONSISTENT_OFFICIAL_PAGINATION",
                )?;
                need(data.get("data").is_some_and(V::is_array), "INCOMPLETE_OFFICIAL_PAGINATION")?;
                for row in data.get("data").unwrap().as_array().unwrap() {
                    let q = js_int(row.get("quantity").unwrap_or(&V::Null));
                    need(
                        order_id(row.get("id").unwrap_or(&V::Null))
                            && clock(row.get("filledAt").unwrap_or(&V::Null))
                            && q.is_some()
                            && (row.get("side").and_then(V::as_str) == Some("yes") && q.unwrap() > 0
                                || row.get("side").and_then(V::as_str) == Some("no") && q.unwrap() < 0)
                            && (row.get("price").unwrap_or(&V::Null).is_null()
                                || row.get("price").and_then(V::as_f64).is_some_and(|p| p.is_finite() && (0.0..=1.0).contains(&p))),
                        "INVALID_OFFICIAL_FILL",
                    )?;
                }
                Ok(())
            },
            |row| {
                baal_service_runtime::json_text::js_stringify(&json!([
                    row.get("id").unwrap_or(&V::Null).clone(),
                    row.get("filledAt").unwrap_or(&V::Null).clone(),
                    row.get("side").unwrap_or(&V::Null).clone(),
                    row.get("quantity").unwrap_or(&V::Null).clone(),
                    row.get("price").unwrap_or(&V::Null).clone(),
                ]))
            },
        )
    }
    fn write(&self, spec: V, authority: &V) -> Result<V, OrderTransportError> {
        let permit = authority.get("permit").unwrap_or(&V::Null);
        need(
            permit.is_object()
                && permit.get("intentId").and_then(V::as_str).is_some_and(|s| !s.is_empty())
                && !authority.get("lease").is_none_or(V::is_null),
            "PERMIT_AND_LEASE_REQUIRED",
        )?;
        let permit = permit.clone();
        let lease = authority.get("lease").unwrap().clone();
        let spec_v = spec.clone();
        let admitted = std::cell::Cell::new(false);
        let spec_body = spec.get("body").cloned().unwrap_or(V::Null);
        let method = spec.get("method").and_then(V::as_str).unwrap_or("").to_string();
        let endpoint = spec.get("endpoint").and_then(V::as_str).unwrap_or("").to_string();
        // spec.beforeDispatch — runs at serialized queue admission.
        let before_dispatch = |_v: &V, _s: &V| -> Result<bool, OrderTransportError> {
            need(!admitted.get(), "DUPLICATE_WRITE_ADMISSION")?;
            (|| -> Result<(), OrderTransportError> {
                need((self.can_write)(), "WRITE_STOPPED_OR_OWNER_LOST")?;
                need((self.validate_lease)(&lease, &spec_v).unwrap_or(false), "INVALID_EXECUTION_LEASE")?;
                need((self.validate_permit)(&permit, &spec_v).unwrap_or(false), "INVALID_EXECUTION_PERMIT")?;
                // Validators may await IO. Check the owner/STOP fence again.
                need((self.can_write)(), "WRITE_STOPPED_OR_OWNER_LOST")?;
                if method == "DELETE" {
                    self.owned(&json!(endpoint.trim_start_matches("/orders/")))?;
                }
                Ok(())
            })()
            .map_err(|e| if e.code == "" { e } else { e })?;
            if method == "POST" {
                let identity = format!(
                    "{}:{}",
                    permit.get("runId").and_then(V::as_str).unwrap_or(""),
                    permit.get("intentId").and_then(V::as_str).unwrap_or("")
                );
                let encoded = spec.get("bodyJson").and_then(V::as_str).unwrap_or("").to_string();
                let mut logical = self.logical_bodies.borrow_mut();
                need(
                    !logical.contains_key(&identity) || logical.get(&identity).unwrap() == &encoded,
                    "LOGICAL_REQUEST_CHANGED",
                )?;
                let mut keys = self.key_bodies.borrow_mut();
                let key = spec_body.get("idempotencyKey").and_then(V::as_str).unwrap_or("");
                need(
                    !keys.contains_key(key) || keys.get(key).unwrap() == &encoded,
                    "IDEMPOTENCY_PAYLOAD_CHANGED",
                )?;
                need(
                    logical.contains_key(&identity)
                        || permit.get("retry") == Some(&V::Bool(true))
                        || parse(spec_body.get("expirationDate").and_then(V::as_str).unwrap_or("")) > (self.now)(),
                    "LIMIT_ORDER_EXPIRED",
                )?;
                logical.insert(identity, encoded.clone());
                keys.insert(key.to_string(), encoded);
            }
            admitted.set(true);
            Ok(true)
        };
        // spec.beforeNetwork — immediately before the network call.
        let before_network = |_v: &V, _s: &V| -> Result<bool, OrderTransportError> {
            (|| -> Result<(), OrderTransportError> {
                need(admitted.get(), "WRITE_NOT_ADMITTED")?;
                need((self.can_write)(), "WRITE_STOPPED_OR_OWNER_LOST")?;
                need((self.validate_lease)(&lease, &spec_v).unwrap_or(false), "INVALID_EXECUTION_LEASE")?;
                need(
                    (self.validate_network_permit)(&permit, &spec_v).unwrap_or(false),
                    "INVALID_NETWORK_EXECUTION_PERMIT",
                )?;
                need((self.can_write)(), "WRITE_STOPPED_OR_OWNER_LOST")?;
                if method == "DELETE" {
                    self.owned(&json!(endpoint.trim_start_matches("/orders/")))?;
                }
                Ok(())
            })()?;
            Ok(true)
        };
        let response = self.request(&spec, Some(&before_dispatch), Some(&before_network));
        if !admitted.get() {
            return Err(OrderTransportError {
                code: "DISPATCH_GUARD_NOT_CALLED".into(),
                outcome: "unknown".into(),
                may_have_executed: true,
                retry_same_request_only: true,
                ..err("")
            });
        }
        response
    }
    pub fn place(&self, body: &V, authority: &V) -> Result<V, OrderTransportError> {
        let payload = body_of(body, &self.scope)?;
        self.write(
            json!({
                "method": "POST",
                "endpoint": "/orders",
                "body": payload.clone(),
                "bodyJson": canonical_body(&payload),
            }),
            authority,
        )
    }
    pub fn cancel(&self, id: &V, authority: &V) -> Result<V, OrderTransportError> {
        let id = self.owned(id)?;
        let response = self.write(
            json!({"method": "DELETE", "endpoint": format!("/orders/{id}")}),
            authority,
        )?;
        if response.get("data").and_then(|d| d.get("orderId")).map(|v| match v {
            V::String(s) => s.clone(),
            V::Number(n) => n.to_string(),
            _ => String::new(),
        }) != Some(id.clone())
            || response.pointer("/data/tournamentId").and_then(V::as_str) != Some(self.scope.tournament_id.as_str())
        {
            return Err(OrderTransportError {
                code: "INVALID_CANCEL_RESPONSE".into(),
                status: response.get("status").and_then(V::as_i64),
                outcome: "unknown".into(),
                may_have_executed: true,
                semantic_mismatch: true,
                ..err("")
            });
        }
        Ok(response)
    }
}
fn strip_order_id(endpoint: &str) -> Option<Option<String>> {
    let s = endpoint.strip_prefix("/orders/")?;
    if s.contains('/') || s.is_empty() {
        return None;
    }
    if s.bytes().all(|b| b.is_ascii_digit()) && !s.starts_with('0') {
        Some(Some(s.to_string()))
    } else {
        None
    }
}
fn strip_fills_id(endpoint: &str) -> Option<String> {
    let rest = endpoint.strip_prefix("/orders/")?;
    let (id, tail) = rest.split_once("/fills?")?;
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) || id.starts_with('0') {
        return None;
    }
    Some(format!("{id}?{tail}").split('?').next().unwrap().to_string())
}
fn market_route(endpoint: &str) -> Option<(String, String)> {
    // /markets/{id}[/orderbook|/nodes]?...
    let rest = endpoint.strip_prefix("/markets/")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.starts_with('0') {
        return None;
    }
    let after = &rest[digits.len()..];
    let kind = if after.starts_with("/orderbook?") {
        "/orderbook"
    } else if after.starts_with("/nodes?") {
        "/nodes"
    } else if after.starts_with('?') {
        ""
    } else {
        return None;
    };
    Some((digits, kind.to_string()))
}
