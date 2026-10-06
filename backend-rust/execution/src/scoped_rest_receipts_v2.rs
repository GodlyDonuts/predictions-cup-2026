// execution/scoped-rest-receipts-v2.mjs — a decoder around the sole collector's
// existing fetch. It adds no client, admission, credential, persistence, timer
// or execution authority.
//
// Port notes:
// - The JS module returns `new Proxy(response, ...)` whose `json()`/`text()`
//   consume and record the streamed body. Rust models that as
//   ScopedFetchResponse::json()/text(); the supplied fetchFn returns a
//   FetchOutcome carrying the response value plus its streaming reader
//   (JS `response.body.getReader()`).
// - JS throws `Object.assign(Error(code), {code, fatal, durableRecordingFailed,
//   mayHaveExecuted, requestStartedAt, fetchInvocationId})`; Rust keeps the
//   identical code string in Err(String) and drops the extra fields.
use crate::core::payload_hash;
use crate::receipts::parse_number_lexemes;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, VecDeque};
use std::rc::Rc;
use uuid::Uuid;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(code.to_string())
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn slug(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|seg| {
            !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}
fn num_id(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('0')
        && s.bytes().all(|b| b.is_ascii_digit())
        && s.parse::<u64>().is_ok_and(|n| n <= 9_007_199_254_740_991)
}

// Minimal `new URL` for https origins: scheme://authority[/path][?query][#frag].
struct ParsedUrl {
    origin: String,
    path: String,   // pathname only
    query: String,  // without leading '?'; "" means no search
    has_hash: bool, // JS `u.hash` truthiness — a '#fragment' was present
}
fn parse_url(url: &str) -> Result<ParsedUrl, String> {
    let scheme_end = url.find("://").ok_or_else(|| "Invalid URL".to_string())?;
    let scheme = &url[..scheme_end];
    need(!scheme.is_empty(), "Invalid URL")?;
    let rest = &url[scheme_end + 3..];
    need(!rest.is_empty(), "Invalid URL")?;
    let (authority, tail) = match rest.find(|c| c == '/' || c == '?' || c == '#') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    need(!authority.is_empty(), "Invalid URL")?;
    let host = authority.split('@').next_back().unwrap_or("");
    need(!host.is_empty(), "Invalid URL")?;
    let has_hash = tail.contains('#');
    let tail_no_frag = tail.split('#').next().unwrap_or("");
    let (path, query) = match tail_no_frag.find('?') {
        Some(i) => (&tail_no_frag[..i], &tail_no_frag[i + 1..]),
        None => (tail_no_frag, ""),
    };
    let path = if path.is_empty() { "/" } else { path };
    need(path.starts_with('/'), "Invalid URL")?;
    Ok(ParsedUrl {
        origin: format!("{scheme}://{authority}"),
        path: path.to_string(),
        query: query.to_string(),
        has_hash,
    })
}
// application/x-www-form-urlencoded component decode.
fn form_decode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len()
                && b[i + 1].is_ascii_hexdigit()
                && b[i + 2].is_ascii_hexdigit() =>
            {
                out.push(
                    u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap_or(b'%'),
                );
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
// URLSearchParams.get(key): first value, in pair order.
fn query_param_get(query: &str, key: &str) -> Option<String> {
    if query.is_empty() {
        return None;
    }
    for pair in query.split('&') {
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        if form_decode(k) == key {
            return Some(form_decode(v));
        }
    }
    None
}
// [...u.searchParams.keys()] — one entry per pair, in order.
fn query_keys(query: &str) -> Vec<String> {
    if query.is_empty() {
        return Vec::new();
    }
    query
        .split('&')
        .map(|pair| form_decode(pair.split_once('=').map(|(k, _)| k).unwrap_or(pair)))
        .collect()
}

/// The supplied fetch result: the response's own fields plus its body reader
/// (JS `response.body.getReader()` -> `reader()` yields `Some(chunk)` until
/// `None`; `reader: None` means the response exposes no stream).
pub struct FetchOutcome {
    pub value: V,
    pub reader: Option<Box<dyn FnMut() -> Result<Option<Vec<u8>>, String>>>,
}
impl FetchOutcome {
    /// A JSON-only response with no stream (records will fail
    /// REST_RECEIPT_STREAM_REQUIRED on consume, as in JS).
    pub fn json(value: V) -> Self {
        FetchOutcome { value, reader: None }
    }
}

/// What `fetch(url, options)` returns. For endpoints outside the scope this is
/// the supplied fetch's own result (the JS pass-through branch); for scoped
/// endpoints `json()`/`text()` stream the body once and record the receipt.
pub struct ScopedFetchResponse {
    /// The response's own properties (JS Proxy target).
    pub value: V,
    reader: Option<Box<dyn FnMut() -> Result<Option<Vec<u8>>, String>>>,
    meta: Option<V>,
    inner: Option<Rc<RefCell<Inner>>>,
    consumed: bool,
}
impl ScopedFetchResponse {
    /// `response.json()` — consumes and records the body on scoped responses.
    pub fn json(&mut self) -> Result<V, String> {
        if self.meta.is_some() {
            return Ok(self.consume()?.1);
        }
        if let Some(d) = self.value.get("data") {
            return Ok(d.clone());
        }
        serde_json::from_str(&self.raw_body()?).map_err(|e| e.to_string())
    }
    /// `response.text()` — consumes and records the body on scoped responses.
    pub fn text(&mut self) -> Result<String, String> {
        if self.meta.is_some() {
            return Ok(self.consume()?.0);
        }
        Ok(self.raw_body()?)
    }
    fn raw_body(&mut self) -> Result<String, String> {
        if let Some(t) = self.value.get("bodyText").and_then(|t| t.as_str()) {
            return Ok(t.to_string());
        }
        let mut reader = self
            .reader
            .take()
            .ok_or_else(|| "REST_RECEIPT_STREAM_REQUIRED".to_string())?;
        let mut bytes = Vec::new();
        while let Some(chunk) = reader()? {
            bytes.extend(chunk);
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
    // consume() — decode the bounded stream, build the record, save it.
    fn consume(&mut self) -> Result<(String, V), String> {
        let inner = self.inner.clone().ok_or("REST_RECEIPT_CLOSED")?;
        need(!inner.borrow().closed, "REST_RECEIPT_CLOSED")?;
        need(!self.consumed, "REST_RECEIPT_BODY_USED")?;
        self.consumed = true;
        need(self.reader.is_some(), "REST_RECEIPT_STREAM_REQUIRED")?;
        let mut reader = self.reader.take().unwrap();
        let limit = std::cmp::min(inner.borrow().maximum_bytes, 8 * 1024 * 1024);
        let mut bytes: Vec<u8> = Vec::new();
        let read_result = (|| -> Result<(), String> {
            while let Some(chunk) = reader()? {
                need(!inner.borrow().closed, "REST_RECEIPT_CLOSED")?;
                bytes.extend_from_slice(&chunk);
                if bytes.len() > limit {
                    return Err("REST_RECEIPT_BODY_BOUND".into());
                }
            }
            Ok(())
        })();
        drop(reader); // reader.cancel() / releaseLock()
        read_result?;
        // TextDecoder({stream}) semantics == lossy-decoding the whole buffer.
        let body_text = String::from_utf8_lossy(&bytes).into_owned();
        let (data, _numbers) = parse_number_lexemes(&body_text)?;
        let meta = self.meta.clone().unwrap_or(V::Null);
        let mut record = match meta {
            V::Object(m) => V::Object(m),
            _ => Map::new().into(),
        };
        let record_map = record.as_object_mut().unwrap();
        // response.status ?? 200
        let status = match self.value.get("status") {
            Some(s) if !s.is_null() => s.clone(),
            _ => json!(200),
        };
        record_map.insert("status".into(), status);
        record_map.insert("receivedAt".into(), json!(ms_to_iso((inner.borrow().now)())));
        record_map.insert("bodyText".into(), json!(body_text));
        record_map.insert("sourceBodyHash".into(), json!(sha256(body_text.as_bytes())));
        record_map.insert("dataHash".into(), json!(payload_hash(&data)?));
        need(
            parse_iso_ms(get(&record, "receivedAt").as_str().unwrap_or("")).unwrap_or(f64::NAN)
                >= parse_iso_ms(get(&record, "requestStartedAt").as_str().unwrap_or("")).unwrap_or(f64::NAN),
            "REST_RECEIPT_CLOCK_ROLLBACK",
        )?;
        inner.borrow_mut().save(record)?;
        Ok((body_text, data))
    }
}
struct Inner {
    closed: bool,
    records: VecDeque<V>, // each record carries a private `size` field
    bytes: usize,
    started: u64,
    captured: u64,
    capacity: usize,
    maximum_bytes: usize,
    now: Rc<dyn Fn() -> f64>,
}
impl Inner {
    // save(record) — bounded FIFO capture.
    fn save(&mut self, record: V) -> Result<(), String> {
        need(!self.closed, "REST_RECEIPT_CLOSED")?;
        let size = get(&record, "bodyText").as_str().unwrap_or("").len();
        need(size <= self.maximum_bytes, "REST_RECEIPT_BODY_BOUND")?;
        while self.records.len() >= self.capacity || self.bytes + size > self.maximum_bytes {
            let evicted = self.records.pop_front();
            self.bytes -= get(&evicted.unwrap_or(V::Null), "size")
                .as_u64()
                .unwrap_or(0) as usize;
        }
        let mut with_size = record;
        with_size
            .as_object_mut()
            .map(|m| m.insert("size".into(), json!(size)));
        self.records.push_back(with_size);
        self.bytes += size;
        self.captured += 1;
        Ok(())
    }
}

pub struct ScopedRestReceipts {
    inner: Rc<RefCell<Inner>>,
    fetch_fn: Rc<dyn Fn(&str, &V) -> Result<FetchOutcome, String>>,
    on_fetch_start: Rc<dyn Fn(&V) -> Result<(), String>>,
    tournament_id: String,
    tournament_slug: String,
    market_ids: BTreeSet<String>,
    maximum_bytes: usize,
    capacity: usize,
    closed: Cell<bool>,
}

/// createScopedRestReceiptsV2({fetchFn, scope, now, onFetchStart, capacity,
/// maximumBytes})
pub fn create_scoped_rest_receipts_v2(
    fetch_fn: Rc<dyn Fn(&str, &V) -> Result<FetchOutcome, String>>,
    scope: &V,
    now: Rc<dyn Fn() -> f64>,
    on_fetch_start: Rc<dyn Fn(&V) -> Result<(), String>>,
    capacity: i64,
    maximum_bytes: i64,
) -> Result<ScopedRestReceipts, String> {
    need(
        get(scope, "tournamentId").is_string()
            && get(scope, "tournamentSlug").as_str().is_some_and(slug),
        "REST_RECEIPT_SCOPE",
    )?;
    need(
        capacity >= 1
            && capacity <= 64
            && maximum_bytes > 0
            && maximum_bytes <= 64 * 1024 * 1024,
        "REST_RECEIPT_BOUNDS",
    )?;
    // scope.marketIds || [] — a falsy value means an empty set; a truthy
    // non-array fails Array.isArray.
    let market_ids_raw = scope.get("marketIds").cloned().unwrap_or(V::Null);
    need(
        !js_truthy(Some(&market_ids_raw)) || market_ids_raw.is_array(),
        "REST_RECEIPT_MARKET_SCOPE",
    )?;
    let market_ids_list = market_ids_raw.as_array().cloned().unwrap_or_default();
    let market_ids: BTreeSet<String> = market_ids_list
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    need(
        market_ids.len() < 1001
            && market_ids_list.iter().all(|v| v.as_str().is_some_and(num_id)),
        "REST_RECEIPT_MARKET_SCOPE",
    )?;
    Ok(ScopedRestReceipts {
        inner: Rc::new(RefCell::new(Inner {
            closed: false,
            records: VecDeque::new(),
            bytes: 0,
            started: 0,
            captured: 0,
            capacity: capacity as usize,
            maximum_bytes: maximum_bytes as usize,
            now,
        })),
        fetch_fn,
        on_fetch_start,
        tournament_id: get(scope, "tournamentId").as_str().unwrap_or("").to_string(),
        tournament_slug: get(scope, "tournamentSlug").as_str().unwrap_or("").to_string(),
        market_ids,
        maximum_bytes: maximum_bytes as usize,
        capacity: capacity as usize,
        closed: Cell::new(false),
    })
}

impl ScopedRestReceipts {
    // endpointFor(url, method) — the scoped allowlist, or null.
    fn endpoint_for(&self, url: &str, method: &str) -> Result<Option<String>, String> {
        let u = parse_url(url)?;
        if u.origin != "https://sig.thesuper.market" || !u.path.starts_with("/api/v1/") {
            return Ok(None);
        }
        let p = &u.path["/api/v1".len()..];
        let endpoint = format!("{p}{}", if u.query.is_empty() { "" } else { "?" })
            + &u.query;
        let no_search = u.query.is_empty();
        if method == "POST" && p == "/orders" && no_search {
            return Ok(Some(endpoint));
        }
        if method == "DELETE" && no_search && p.starts_with("/orders/") && num_id(&p["/orders/".len()..]) {
            return Ok(Some(endpoint));
        }
        if method != "GET" {
            return Ok(None);
        }
        if (p == format!("/tournaments/{}", self.tournament_slug)
            || p == format!("/tournaments/{}/portfolio/positions", self.tournament_slug))
            && no_search
        {
            return Ok(Some(endpoint));
        }
        if p == "/orders" && query_param_get(&u.query, "tournamentId").as_deref() == Some(self.tournament_id.as_str()) {
            return Ok(Some(endpoint));
        }
        // /^\/orders\/[1-9]\d*(?:\/fills)?$/
        if let Some(rest) = p.strip_prefix("/orders/") {
            let id_part = rest.strip_suffix("/fills").unwrap_or(rest);
            if (rest == id_part || rest.ends_with("/fills")) && num_id(id_part) {
                return Ok(Some(endpoint));
            }
        }
        // /^\/markets\/([1-9]\d*)\/nodes$/ — v2 additionally accepts the
        // market-nodes read when the only query key is tournamentId.
        if let Some(market_id) = p.strip_prefix("/markets/").and_then(|r| r.strip_suffix("/nodes")) {
            if num_id(market_id)
                && self.market_ids.contains(market_id)
                && !u.has_hash
                && query_param_get(&u.query, "tournamentId").as_deref() == Some(self.tournament_id.as_str())
                && query_keys(&u.query).join("|") == "tournamentId"
            {
                return Ok(Some(endpoint));
            }
        }
        // /^\/markets\/([1-9]\d*)\/orderbook$/
        if let Some(rest) = p.strip_prefix("/markets/") {
            if let Some(market_id) = rest.strip_suffix("/orderbook") {
                let mut keys = query_keys(&u.query);
                keys.sort();
                if num_id(market_id)
                    && self.market_ids.contains(market_id)
                    && query_param_get(&u.query, "tournamentId").as_deref() == Some(self.tournament_id.as_str())
                    && query_param_get(&u.query, "depth").as_deref() == Some("20")
                    && keys.join("|") == "depth|tournamentId"
                {
                    return Ok(Some(endpoint));
                }
            }
        }
        Ok(None)
    }

    /// fetch(url, options) — scoped requests are decoded and their receipts
    /// recorded; everything else passes through the supplied fetch untouched.
    pub fn fetch(&self, url: &str, options: &V) -> Result<ScopedFetchResponse, String> {
        need(!self.closed.get(), "REST_RECEIPT_CLOSED")?;
        let method_v = options.get("method").cloned().unwrap_or(V::Null);
        let method = if js_truthy(options.get("method")) {
            method_v
                .as_str()
                .ok_or_else(|| "(options.method || 'GET').toUpperCase is not a function".to_string())?
                .to_uppercase()
        } else {
            "GET".to_string()
        };
        let endpoint = self.endpoint_for(url, &method)?;
        if endpoint.is_none() {
            let out = (self.fetch_fn)(url, options)?;
            return Ok(ScopedFetchResponse {
                value: out.value,
                reader: out.reader,
                meta: None,
                inner: None,
                consumed: false,
            });
        }
        let endpoint = endpoint.unwrap();
        let mut request_body_hash = V::Null;
        if method == "POST" {
            need(get(options, "body").is_string(), "REST_RECEIPT_REQUEST_BODY")?;
            let (body, _n) = parse_number_lexemes(get(options, "body").as_str().unwrap())?;
            need(
                get(&body, "tournamentId").as_str() == Some(self.tournament_id.as_str()),
                "REST_RECEIPT_REQUEST_SCOPE",
            )?;
            request_body_hash = json!(payload_hash(&body)?);
        }
        let meta = json!({
            "fetchInvocationId": Uuid::new_v4().to_string(),
            "method": method,
            "endpoint": endpoint,
            "requestBodyHash": request_body_hash,
            "requestStartedAt": ms_to_iso((self.inner.borrow().now)()),
        });
        // Call the actual supplied fetch BEFORE reporting an invocation. The
        // durable logical intent must already exist; this callback cannot
        // authorize a write.
        let outcome = (self.fetch_fn)(url, options);
        self.inner.borrow_mut().started += 1;
        if let Err(e) = (self.on_fetch_start)(&meta) {
            let _ = e;
            // await pending; cancel the body — the reader is dropped here.
            let _ = outcome;
            return Err("REST_RECEIPT_START_RECORDING_FAILED".into());
        }
        let outcome = match outcome {
            Ok(o) => o,
            Err(_) => return Err("REST_RECEIPT_FETCH_UNAVAILABLE".into()),
        };
        let response = outcome.value;
        if !response.is_object() {
            return Ok(ScopedFetchResponse {
                value: response,
                reader: outcome.reader,
                meta: None,
                inner: None,
                consumed: false,
            });
        }
        Ok(ScopedFetchResponse {
            value: response,
            reader: outcome.reader,
            meta: Some(meta),
            inner: Some(self.inner.clone()),
            consumed: false,
        })
    }

    /// receiptOf(response, {method='GET', requestBodyHash=null}) — bind a used
    /// response to exactly one recorded scoped receipt.
    pub fn receipt_of(&self, response: &V, options: &V) -> Result<V, String> {
        need(
            !self.closed.get()
                && get(response, "endpoint").is_string()
                && response.get("data").is_some(),
            "REST_RECEIPT_RESPONSE",
        )?;
        // {method='GET', requestBodyHash=null} — destructured defaults apply
        // only when the key is absent, not for an explicit null.
        let method = match options.get("method") {
            Some(m) => m.as_str().unwrap_or("GET"),
            None => "GET",
        };
        let request_body_hash = options.get("requestBodyHash").cloned().unwrap_or(V::Null);
        let begin = parse_iso_ms(get(response, "requestStartedAt").as_str().unwrap_or(""));
        let end = parse_iso_ms(get(response, "receivedAt").as_str().unwrap_or(""));
        need(
            begin.is_some() && end.is_some() && begin <= end,
            "REST_RECEIPT_RESPONSE_CLOCK",
        )?;
        let begin = begin.unwrap_or(f64::NAN);
        let end = end.unwrap_or(f64::NAN);
        let data_hash = payload_hash(get(response, "data"))?;
        let inner = self.inner.borrow();
        let matches: Vec<&V> = inner
            .records
            .iter()
            .filter(|r| {
                // r.status === (response.status ?? 200)
                let response_status = match response.get("status") {
                    Some(s) if !s.is_null() => s.clone(),
                    _ => json!(200),
                };
                get(r, "method").as_str() == Some(method)
                    && get(r, "endpoint") == get(response, "endpoint")
                    && get(r, "status") == &response_status
                    && get(r, "requestBodyHash") == &request_body_hash
                    && get(r, "dataHash").as_str() == Some(data_hash.as_str())
                    && parse_iso_ms(get(r, "requestStartedAt").as_str().unwrap_or("")).unwrap_or(f64::NAN) >= begin
                    && parse_iso_ms(get(r, "receivedAt").as_str().unwrap_or("")).unwrap_or(f64::NAN) <= end
            })
            .collect();
        need(matches.len() == 1, "REST_RECEIPT_RESPONSE_UNBOUND")?;
        // const {size, dataHash: ignored, ...record} = matches[0]
        let mut record = matches[0].clone();
        if let V::Object(m) = &mut record {
            m.shift_remove("size");
            m.shift_remove("dataHash");
        }
        Ok(record)
    }

    pub fn stats(&self) -> V {
        let inner = self.inner.borrow();
        json!({
            "records": inner.records.len(),
            "bytes": inner.bytes,
            "started": inner.started,
            "captured": inner.captured,
            "closed": self.closed.get(),
            "executionAuthority": false,
            "additionalClients": 0,
        })
    }
    pub fn close(&self) {
        self.inner.borrow_mut().records.clear();
        self.inner.borrow_mut().bytes = 0;
        self.closed.set(true);
        self.inner.borrow_mut().closed = true;
    }
}
