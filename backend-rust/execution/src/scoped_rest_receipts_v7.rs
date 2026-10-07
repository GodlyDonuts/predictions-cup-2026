// execution/scoped-rest-receipts-v7.mjs — captures documented graph receipts
// for the declared universe. Capture is not dispatch authority: the sole
// account queue still requires an exact job permit.
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;

use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_int, sha256};
use serde_json::{json, Value as V};
use uuid::Uuid;

use crate::core::payload_hash;
use crate::receipts::parse_number_lexemes;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok {
        Err(code.to_string())
    } else {
        Ok(())
    }
}

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}

fn dparse(v: &V) -> f64 {
    v.as_str()
        .and_then(baal_service_runtime::contract::parse_iso_ms)
        .unwrap_or(f64::NAN)
}

fn stamp(n: f64) -> String {
    ms_to_iso(n)
}

fn sha(raw: &str) -> String {
    sha256(raw.as_bytes())
}

/// /^[1-9]\d*$/
fn market_id_ok(id: &str) -> bool {
    let mut b = id.bytes();
    matches!(b.next(), Some(c) if (b'1'..=b'9').contains(&c)) && b.all(|c| c.is_ascii_digit())
}

/// A decoded response the supplied fetch returned. `body` is the JS
/// `response.body.getReader()` stream: read() -> Some(chunk)|None at EOF.
pub struct ScopedFetchResponse {
    /// response.status — absent (None) means JS `status??200`.
    pub status: Option<i64>,
    /// response.body.getReader() — None means the response is not a stream
    /// (REST_RECEIPT_STREAM_REQUIRED on consume).
    pub body: Option<Box<dyn Iterator<Item = Result<Vec<u8>, String>> + Send>>,
    /// reader.cancel() — abort hook for the recording-failure path.
    pub cancel: Option<Box<dyn FnMut() -> Result<(), String> + Send>>,
}

type FetchFn = Box<dyn Fn(&str, &V) -> Result<ScopedFetchResponse, String>>;

/// The frozen meta object each captured request is recorded against.
fn meta_of(fetch_invocation_id: &str, method: &str, endpoint: &str, request_body_hash: &V, at: f64) -> V {
    json!({
        "fetchInvocationId": fetch_invocation_id,
        "method": method,
        "endpoint": endpoint,
        "requestBodyHash": request_body_hash,
        "requestStartedAt": stamp(at),
    })
}

/// endpointFor(url,method) — returns the route endpoint or None.
/// new URL(url) parity: scheme+host normalization is limited to the shapes the
/// allowlist can match (absolute https URL on sig.thesuper.market).
fn endpoint_for(
    url: &str,
    method: &str,
    tournament_id: &str,
    tournament_slug: &str,
    market_ids: &BTreeSet<String>,
    graph_ids: &BTreeSet<String>,
) -> Result<Option<String>, String> {
    // new URL(url): must be absolute with scheme; a parse failure throws.
    let scheme_end = url
        .find("://")
        .ok_or_else(|| "Invalid URL".to_string())?;
    let scheme = &url[..scheme_end];
    let after_scheme = &url[scheme_end + 3..];
    let authority_end = after_scheme
        .find(|c| c == '/' || c == '?' || c == '#')
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    let host_port = authority.rsplit('@').next().unwrap_or("");
    let (host, port) = match host_port.rfind(':') {
        Some(i) => (&host_port[..i], &host_port[i + 1..]),
        None => (host_port, ""),
    };
    let host_lc = host.to_ascii_lowercase();
    let scheme_lc = scheme.to_ascii_lowercase();
    let default_port = (scheme_lc == "https" && (port.is_empty() || port == "443"))
        || (scheme_lc == "http" && (port.is_empty() || port == "80"));
    let origin = if default_port {
        format!("{scheme_lc}://{host_lc}")
    } else {
        format!("{scheme_lc}://{host_lc}:{port}")
    };
    let rest = &after_scheme[authority_end..];
    let hash_at = rest.find('#');
    let (before_hash, _hash) = match hash_at {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let has_hash = hash_at.map(|i| !rest[i + 1..].is_empty()).unwrap_or(false);
    let q_at = before_hash.find('?');
    let (pathname, mut search) = match q_at {
        Some(i) => (&before_hash[..i], &before_hash[i..]),
        None => (before_hash, ""),
    };
    // `new URL('x?')` yields u.search==='' — a bare '?' is no search.
    if search == "?" {
        search = "";
    }
    if origin != "https://sig.thesuper.market" || !pathname.starts_with("/api/v1/") {
        return Ok(None);
    }
    let p = &pathname["/api/v1".len()..];
    let endpoint = format!("{p}{search}");
    // URLSearchParams keys in insertion order.
    let search_params: Vec<(String, String)> = if !search.is_empty() {
        search[1..]
            .split('&')
            .filter(|s| !s.is_empty())
            .map(|kv| {
                let (k, v) = match kv.find('=') {
                    Some(i) => (&kv[..i], &kv[i + 1..]),
                    None => (kv, ""),
                };
                (k.to_string(), v.to_string())
            })
            .collect()
    } else {
        Vec::new()
    };
    let sp_get = |k: &str| -> Option<&str> {
        search_params
            .iter()
            .find(|(kk, _)| kk == k)
            .map(|(_, v)| v.as_str())
    };
    let sp_keys = |sorted: bool| -> String {
        let mut ks: Vec<&str> = search_params.iter().map(|(k, _)| k.as_str()).collect();
        if sorted {
            ks.sort();
        }
        ks.join("|")
    };
    if method == "POST" && p == "/orders" && search.is_empty() {
        return Ok(Some(endpoint));
    }
    if method == "DELETE" && !search.is_empty() {
        return Ok(None);
    }
    if method == "DELETE" {
        let body = &p[1..];
        let ok = p.starts_with("/orders/")
            && body.len() > "orders/".len()
            && body["orders/".len()..].bytes().next().is_some_and(|b| (b'1'..=b'9').contains(&b))
            && body["orders/".len()..].bytes().all(|b| b.is_ascii_digit());
        return Ok(if ok { Some(endpoint) } else { None });
    }
    if method != "GET" {
        return Ok(None);
    }
    if !graph_ids.is_empty() && !has_hash {
        if p == "/portfolio/collateral"
            && sp_get("tournamentId") == Some(tournament_id)
            && sp_keys(false) == "tournamentId"
        {
            return Ok(Some(endpoint));
        }
        if p == "/relationships/graph"
            && sp_get("marketId").is_some_and(|m| graph_ids.contains(m))
            && sp_get("depth") == Some("4")
            && sp_get("tournamentId") == Some(tournament_id)
            && sp_keys(true) == "depth|marketId|tournamentId"
        {
            return Ok(Some(endpoint));
        }
    }
    let tournament_path = format!("/tournaments/{tournament_slug}");
    if (p == tournament_path || p == format!("{tournament_path}/portfolio/positions"))
        && search.is_empty()
    {
        return Ok(Some(endpoint));
    }
    if p == "/orders" && sp_get("tournamentId") == Some(tournament_id) {
        return Ok(Some(endpoint));
    }
    if p.starts_with("/orders/") {
        let rest_path = &p["/orders/".len()..];
        let (id_part, fills) = match rest_path.strip_suffix("/fills") {
            Some(x) => (x, true),
            None => (rest_path, false),
        };
        let id_ok = id_part.bytes().next().is_some_and(|b| (b'1'..=b'9').contains(&b))
            && id_part.bytes().all(|b| b.is_ascii_digit());
        if id_ok && (fills || !rest_path.contains('/')) {
            return Ok(Some(endpoint));
        }
    }
    if let Some(rest_path) = p.strip_prefix("/markets/") {
        if let Some(id_part) = rest_path.strip_suffix("/nodes") {
            let id_ok = market_id_ok(id_part);
            if id_ok
                && market_ids.contains(id_part)
                && !has_hash
                && sp_get("tournamentId") == Some(tournament_id)
                && sp_keys(false) == "tournamentId"
            {
                return Ok(Some(endpoint));
            }
        }
        if let Some(id_part) = rest_path.strip_suffix("/orderbook") {
            let id_ok = market_id_ok(id_part);
            if id_ok
                && market_ids.contains(id_part)
                && sp_get("tournamentId") == Some(tournament_id)
                && sp_get("depth") == Some("20")
                && sp_keys(true) == "depth|tournamentId"
            {
                return Ok(Some(endpoint));
            }
        }
    }
    Ok(None)
}

struct ReceiptInner {
    closed: bool,
    records: Vec<V>,
    bytes: usize,
    started: u64,
    captured: u64,
}

pub struct ScopedRestReceiptsV7 {
    fetch_fn: FetchFn,
    on_fetch_start: Box<dyn Fn(&V) -> Result<(), String>>,
    on_before_fetch: Option<Box<dyn Fn(&V) -> Result<V, String>>>,
    now: Box<dyn Fn() -> f64>,
    capacity: usize,
    maximum_bytes: usize,
    tournament_id: String,
    tournament_slug: String,
    market_ids: BTreeSet<String>,
    graph_ids: BTreeSet<String>,
    inner: RefCell<ReceiptInner>,
}

/// createScopedRestReceiptsV7({fetchFn,scope,now,onFetchStart,onBeforeFetch,
/// capacity,maximumBytes})
pub fn create_scoped_rest_receipts_v7(
    fetch_fn: FetchFn,
    scope: &V,
    now: Box<dyn Fn() -> f64>,
    on_fetch_start: Box<dyn Fn(&V) -> Result<(), String>>,
    on_before_fetch: Option<Box<dyn Fn(&V) -> Result<V, String>>>,
    capacity: i64,
    maximum_bytes: i64,
) -> Result<ScopedRestReceiptsV7, String> {
    need(
        sget(scope, "tournamentId").is_some()
            && sget(scope, "tournamentSlug").is_some_and(|s| {
                !s.is_empty()
                    && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                    && !s.starts_with('-')
                    && !s.ends_with('-')
                    && !s.contains("--")
            }),
        "REST_RECEIPT_SCOPE",
    )?;
    need(
        capacity > 0
            && capacity <= 64
            && maximum_bytes > 0
            && maximum_bytes <= 64 * 1024 * 1024
            && js_int(&json!(capacity)) == Some(capacity)
            && js_int(&json!(maximum_bytes)) == Some(maximum_bytes),
        "REST_RECEIPT_BOUNDS",
    )?;
    let market_ids: BTreeSet<String> = get(scope, "marketIds")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    let graph_ids: BTreeSet<String> = get(scope, "allowedOwnedGraphMarketIds")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    let graph_list = get(scope, "allowedOwnedGraphMarketIds")
        .as_array()
        .cloned()
        .unwrap_or_default();
    need(
        get(scope, "allowedOwnedGraphMarketIds").is_array()
            && graph_ids.len() == graph_list.len()
            && graph_ids.len() <= 1000
            && graph_list
                .iter()
                .all(|id| id.as_str().is_some_and(|s| market_id_ok(s)))
            && graph_ids.iter().all(|id| market_ids.contains(id)),
        "REST_RECEIPT_OWNED_GRAPH_SCOPE",
    )?;
    let market_list = get(scope, "marketIds").as_array().cloned().unwrap_or_default();
    need(
        get(scope, "marketIds").is_array()
            && market_ids.len() < 1001
            && market_list
                .iter()
                .all(|id| id.as_str().is_some_and(|s| market_id_ok(s))),
        "REST_RECEIPT_MARKET_SCOPE",
    )?;
    Ok(ScopedRestReceiptsV7 {
        fetch_fn,
        on_fetch_start,
        on_before_fetch,
        now,
        capacity: capacity as usize,
        maximum_bytes: maximum_bytes as usize,
        tournament_id: sget(scope, "tournamentId").unwrap_or("").to_string(),
        tournament_slug: sget(scope, "tournamentSlug").unwrap_or("").to_string(),
        market_ids,
        graph_ids,
        inner: RefCell::new(ReceiptInner {
            closed: false,
            records: Vec::new(),
            bytes: 0,
            started: 0,
            captured: 0,
        }),
    })
}

impl ScopedRestReceiptsV7 {
    fn save(&self, record: &V, size: usize) -> Result<(), String> {
        let mut g = self.inner.borrow_mut();
        need(!g.closed, "REST_RECEIPT_CLOSED")?;
        while g.records.len() >= self.capacity || g.bytes + size > self.maximum_bytes {
            let old = g.records.remove(0);
            g.bytes -= get(&old, "size").as_u64().unwrap_or(0) as usize;
        }
        let mut r = record.as_object().cloned().unwrap_or_default();
        r.insert("size".into(), json!(size));
        g.records.push(V::Object(r));
        g.bytes += size;
        g.captured += 1;
        Ok(())
    }

    /// fetch(url,options) — JS async fetch: the supplied fetchFn is invoked
    /// BEFORE onFetchStart is reported, mirroring the pending-promise order.
    /// Returns the proxied response wrapper; a non-allowlisted route is passed
    /// through verbatim.
    pub fn fetch(&self, url: &str, options: &V) -> Result<ScopedReceiptResponse, String> {
        need(!self.inner.borrow().closed, "REST_RECEIPT_CLOSED")?;
        // (options.method||'GET').toUpperCase() — falsy defaults to GET; a
        // truthy non-string method throws a TypeError in JS.
        let method = match sget(options, "method") {
            Some(m) => m.to_uppercase(),
            None if baal_service_runtime::json_text::js_truthy(options.get("method")) => {
                return Err("options.method.toUpperCase is not a function".into())
            }
            None => "GET".to_string(),
        };
        let endpoint = endpoint_for(
            url,
            &method,
            &self.tournament_id,
            &self.tournament_slug,
            &self.market_ids,
            &self.graph_ids,
        )?;
        if endpoint.is_none() {
            // JS returns fetchFn(url,options) verbatim — no proxy, no record.
            let r = (self.fetch_fn)(url, options)?;
            return Ok(ScopedReceiptResponse::passthrough(r));
        }
        let endpoint = endpoint.unwrap();
        let mut request_body_hash = V::Null;
        if method == "POST" {
            need(
                get(options, "body").is_string(),
                "REST_RECEIPT_REQUEST_BODY",
            )?;
            let (data, _) = parse_number_lexemes(get(options, "body").as_str().unwrap_or(""))?;
            need(
                get(&data, "tournamentId").as_str() == Some(self.tournament_id.as_str()),
                "REST_RECEIPT_REQUEST_SCOPE",
            )?;
            request_body_hash = json!(payload_hash(&data)?);
        }
        let mut meta = meta_of(
            &Uuid::new_v4().to_string(),
            &method,
            &endpoint,
            &request_body_hash,
            (self.now)(),
        );
        if self.on_before_fetch.is_some() && (method == "POST" || method == "DELETE") {
            let fenced = (|| -> Result<(), String> {
                let fence = (self.on_before_fetch.as_ref().unwrap())(&meta)?;
                need(
                    fence.get("notAfter").is_some()
                        && js_int(get(&fence, "notAfter")).is_some(),
                    "REST_RECEIPT_FINAL_FENCE",
                )?;
                let actual_start = (self.now)();
                need(
                    actual_start < js_int(get(&fence, "notAfter")).unwrap() as f64,
                    "REST_RECEIPT_FINAL_FENCE_EXPIRED",
                )?;
                meta["requestStartedAt"] = json!(stamp(actual_start));
                Ok(())
            })();
            if fenced.is_err() {
                return Err("REST_RECEIPT_BEFORE_FETCH_GUARD".into());
            }
        }
        // Call the actual supplied fetch BEFORE reporting an invocation. The
        // durable logical intent must already exist; this callback cannot
        // authorize a write.
        let outcome = (self.fetch_fn)(url, options);
        self.inner.borrow_mut().started += 1;
        if (self.on_fetch_start)(&meta).is_err() {
            if let Ok(mut r) = outcome {
                if let Some(mut c) = r.cancel.take() {
                    let _ = c();
                }
            }
            return Err("REST_RECEIPT_START_RECORDING_FAILED".into());
        }
        let response = match outcome {
            Ok(r) => r,
            Err(_) => return Err("REST_RECEIPT_FETCH_UNAVAILABLE".into()),
        };
        Ok(ScopedReceiptResponse::captured(self, response, meta))
    }

    /// receiptOf(response,{method,requestBodyHash})
    pub fn receipt_of(&self, response: &V, method: &str, request_body_hash: &V) -> Result<V, String> {
        let g = self.inner.borrow();
        need(
            !g.closed && sget(response, "endpoint").is_some() && response.get("data").is_some(),
            "REST_RECEIPT_RESPONSE",
        )?;
        let begin = dparse(get(response, "requestStartedAt"));
        let end = dparse(get(response, "receivedAt"));
        need(begin.is_finite() && end.is_finite() && begin <= end, "REST_RECEIPT_RESPONSE_CLOCK")?;
        let data_hash = payload_hash(get(response, "data"))?;
        let status = js_int(get(response, "status")).unwrap_or(200);
        let matches: Vec<&V> = g
            .records
            .iter()
            .filter(|r| {
                sget(r, "method") == Some(method)
                    && sget(r, "endpoint") == sget(response, "endpoint")
                    && js_int(get(r, "status")) == Some(status)
                    && get(r, "requestBodyHash") == request_body_hash
                    && get(r, "dataHash").as_str() == Some(data_hash.as_str())
                    && dparse(get(r, "requestStartedAt")) >= begin
                    && dparse(get(r, "receivedAt")) <= end
            })
            .collect();
        need(matches.len() == 1, "REST_RECEIPT_RESPONSE_UNBOUND")?;
        let mut record = matches[0].as_object().cloned().unwrap_or_default();
        record.remove("size");
        record.remove("dataHash");
        Ok(V::Object(record))
    }

    /// stats()
    pub fn stats(&self) -> V {
        let g = self.inner.borrow();
        json!({
            "records": g.records.len(),
            "bytes": g.bytes,
            "started": g.started,
            "captured": g.captured,
            "closed": g.closed,
            "executionAuthority": false,
            "additionalClients": 0,
        })
    }

    /// close()
    pub fn close(&self) {
        let mut g = self.inner.borrow_mut();
        g.records.clear();
        g.bytes = 0;
        g.closed = true;
    }
}

/// The Proxy(response) wrapper: `.text()`/`.json()` share one consume() that
/// streams the body under the byte bound and saves the record.
pub struct ScopedReceiptResponse<'a> {
    host: Option<&'a ScopedRestReceiptsV7>,
    response: ScopedFetchResponse,
    meta: V,
    consumed: Cell<bool>,
}

impl<'a> ScopedReceiptResponse<'a> {
    fn passthrough(response: ScopedFetchResponse) -> Self {
        ScopedReceiptResponse {
            host: None,
            response,
            meta: V::Null,
            consumed: Cell::new(false),
        }
    }
    fn captured(host: &'a ScopedRestReceiptsV7, response: ScopedFetchResponse, meta: V) -> Self {
        ScopedReceiptResponse {
            host: Some(host),
            response,
            meta,
            consumed: Cell::new(false),
        }
    }

    /// response.status
    pub fn status(&self) -> Option<i64> {
        self.response.status
    }

    /// Shared consume(): bounds the stream, builds bodyText + parsed.data,
    /// records the receipt, and returns (bodyText, data) exactly once.
    fn consume(&mut self) -> Result<(String, V), String> {
        // Passthrough: JS returned the raw response — read it verbatim with no
        // bound, no closed check and no record (plain Response.json()).
        if self.host.is_none() {
            let mut all: Vec<u8> = Vec::new();
            if let Some(reader) = self.response.body.take() {
                for chunk in reader {
                    all.extend_from_slice(&chunk?);
                }
            }
            let text = String::from_utf8_lossy(&all).into_owned();
            let data: V = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            return Ok((text, data));
        }
        let host = self.host.unwrap();
        need(!host.inner.borrow().closed, "REST_RECEIPT_CLOSED")?;
        need(!self.consumed.get(), "REST_RECEIPT_BODY_USED")?;
        self.consumed.set(true);
        let reader = match self.response.body.take() {
            Some(r) => r,
            None => return Err("REST_RECEIPT_STREAM_REQUIRED".into()),
        };
        let limit = std::cmp::min(host.maximum_bytes, 8 * 1024 * 1024);
        let mut all: Vec<u8> = Vec::new();
        let mut count = 0usize;
        let mut read_err: Option<String> = None;
        for chunk in reader {
            if host.inner.borrow().closed {
                read_err = Some("REST_RECEIPT_CLOSED".into());
                break;
            }
            match chunk {
                Ok(bytes) => {
                    count += bytes.len();
                    if count > limit {
                        if let Some(mut c) = self.response.cancel.take() {
                            let _ = c();
                        }
                        return Err("REST_RECEIPT_BODY_BOUND".into());
                    }
                    all.extend_from_slice(&bytes);
                }
                Err(e) => {
                    read_err = Some(e);
                    break;
                }
            }
        }
        if read_err.is_some() {
            if let Some(mut c) = self.response.cancel.take() {
                let _ = c();
            }
            return Err(read_err.unwrap());
        }
        let body_text = String::from_utf8_lossy(&all).into_owned();
        let (data, _) = parse_number_lexemes(&body_text)?;
        let mut record = self.meta.as_object().cloned().unwrap_or_default();
        record.insert("status".into(), json!(self.response.status.unwrap_or(200)));
        record.insert("receivedAt".into(), json!(stamp((host.now)())));
        record.insert("bodyText".into(), json!(body_text));
        record.insert("sourceBodyHash".into(), json!(sha(&body_text)));
        record.insert("dataHash".into(), json!(payload_hash(&data)?));
        need(
            dparse(get(&V::Object(record.clone()), "receivedAt"))
                >= dparse(get(&V::Object(record.clone()), "requestStartedAt")),
            "REST_RECEIPT_CLOCK_ROLLBACK",
        )?;
        host.save(&V::Object(record), body_text.len())?;
        Ok((body_text, data))
    }

    /// proxy.text()
    pub fn text(&mut self) -> Result<String, String> {
        Ok(self.consume()?.0)
    }

    /// proxy.json()
    pub fn json(&mut self) -> Result<V, String> {
        Ok(self.consume()?.1)
    }
}
