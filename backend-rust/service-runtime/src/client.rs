use crate::budget::{network_interval, network_write_interval};
use crate::exclusions::exclusion_set;
use crate::history::allowed_history_endpoint;
use crate::timing::LatencyClock;
use crate::wait::retry_after_ms;
use serde_json::{json, Map, Value as V};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

pub const API_BASE: &str = "https://sig.thesuper.market/api/v1";
pub const TOURNAMENT_ID: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";

// --- Fetch abstraction (the JS `fetchFn` injection point) -------------------

pub struct FetchRequest {
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub timeout_ms: u64,
}
pub struct FetchOutcome {
    pub status: u16,
    pub retry_after: Option<String>,
    pub body: String,
}
pub trait Fetch: Send + Sync {
    fn fetch(&self, url: &str, req: &FetchRequest) -> Result<FetchOutcome, String>;
}

pub struct ReqwestFetch {
    client: reqwest::blocking::Client,
}
impl ReqwestFetch {
    pub fn new() -> Result<ReqwestFetch, String> {
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        Ok(ReqwestFetch { client })
    }
}
impl Fetch for ReqwestFetch {
    fn fetch(&self, url: &str, req: &FetchRequest) -> Result<FetchOutcome, String> {
        let mut b = self
            .client
            .request(req.method.parse().unwrap_or(reqwest::Method::GET), url)
            .timeout(Duration::from_millis(req.timeout_ms));
        for (k, v) in &req.headers {
            b = b.header(k.as_str(), v.as_str());
        }
        if let Some(body) = &req.body {
            b = b.body(body.clone());
        }
        let resp = b.send().map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        // JS fetch `redirect: 'error'` rejects 3xx before producing a response.
        if (300..400).contains(&status) {
            return Err("Redirect rejected".into());
        }
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let body = resp.text().map_err(|e| e.to_string())?;
        Ok(FetchOutcome { status, retry_after, body })
    }
}

// --- Dispatch ---------------------------------------------------------------

pub type Guard = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;
pub type Authorizer = Arc<dyn Fn(&V) -> bool + Send + Sync>;

pub struct DispatchSpec {
    pub method: String,
    pub endpoint: String,
    pub body: Option<V>,
    pub body_json: Option<String>,
    pub before_dispatch: Option<Guard>,
    pub before_network: Option<Guard>,
}
impl DispatchSpec {
    // The `safe` view JS hands to authorizeDispatch.
    fn safe_view(&self) -> V {
        let mut m = Map::new();
        m.insert("method".into(), json!(self.method));
        m.insert("endpoint".into(), json!(self.endpoint));
        if let Some(b) = &self.body {
            m.insert("body".into(), b.clone());
        }
        if let Some(j) = &self.body_json {
            m.insert("bodyJson".into(), json!(j));
        }
        V::Object(m)
    }
}

#[derive(Debug)]
pub struct ServiceError {
    pub message: String,
    pub status: Option<u16>,
    pub api_code: Option<String>,
    pub retry_after_ms: f64,
    pub recoverable: bool,
    pub stopped: bool,
}
impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for ServiceError {}
fn err(message: &str) -> ServiceError {
    ServiceError {
        message: message.to_string(),
        status: None,
        api_code: None,
        retry_after_ms: 0.0,
        recoverable: false,
        stopped: false,
    }
}
fn stopped_err() -> ServiceError {
    ServiceError { stopped: true, ..err("Stopped") }
}

// --- Allowlist ---------------------------------------------------------------

fn endpoint_allowed(endpoint: &str) -> bool {
    let tid = TOURNAMENT_ID;
    if endpoint == "/tournaments/midterm-elections"
        || endpoint == "/tournaments/midterm-elections/portfolio/fills?limit=100"
        || endpoint == "/tournaments/midterm-elections/portfolio/pnl?period=day"
        || endpoint == format!("/orders?status=all&limit=200&tournamentId={tid}")
        || endpoint == "/tournaments/midterm-elections/portfolio/positions"
    {
        return true;
    }
    let open = format!("/orders?status=open&limit=200&tournamentId={tid}");
    if let Some(rest) = endpoint.strip_prefix(&open) {
        if rest.is_empty() {
            return true;
        }
        if let Some(cursor) = rest.strip_prefix("&cursor=") {
            return !cursor.is_empty()
                && cursor
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "%._~-".contains(c));
        }
        return false;
    }
    if let Some(rest) = endpoint.strip_prefix("/markets/") {
        if let Some(id) = rest.split('/').next() {
            let tail = format!("/orderbook?tournamentId={tid}&depth=20");
            if !id.is_empty()
                && id.bytes().all(|b| b.is_ascii_digit())
                && rest[id.len()..] == *tail
            {
                return true;
            }
        }
    }
    let bulk_prefix = "/exchanges/prices?ids=";
    if let Some(rest) = endpoint.strip_prefix(bulk_prefix) {
        let suffix = format!("&tournamentId={tid}");
        if let Some(ids) = rest.strip_suffix(&suffix) {
            let list: Vec<&str> = ids.split(',').collect();
            let valid = !list.is_empty()
                && list.len() <= 100
                && list.iter().all(|x| {
                    !x.is_empty()
                        && !x.starts_with('0')
                        && x.bytes().all(|b| b.is_ascii_digit())
                        && x.parse::<i64>().is_ok_and(|v| v <= 9_007_199_254_740_991)
                });
            let unique = list.iter().collect::<std::collections::BTreeSet<_>>().len() == list.len();
            if valid && unique {
                return true;
            }
        }
    }
    allowed_history_endpoint(endpoint)
}

fn kind_of(endpoint: &str) -> (&'static str, Option<String>) {
    let kind = if endpoint.contains("/fills") {
        "fills"
    } else if endpoint.contains("/portfolio/pnl?") {
        "pnl"
    } else if endpoint.contains("/price-history?") {
        "history"
    } else if endpoint.starts_with("/markets/") {
        if endpoint.contains("/orderbook?") { "book" } else { "market" }
    } else if endpoint.starts_with("/orders") {
        "orders"
    } else if endpoint.starts_with("/exchanges/prices?") {
        "prices"
    } else if endpoint.ends_with("/portfolio/positions") {
        "positions"
    } else {
        "tournament"
    };
    let market_id = if kind == "book" {
        endpoint
            .strip_prefix("/markets/")
            .and_then(|r| r.split('/').next())
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .map(str::to_string)
    } else {
        None
    };
    (kind, market_id)
}

fn iso(ms: f64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}

// --- ReadClient --------------------------------------------------------------

pub struct Clocks {
    pub now: Arc<dyn Fn() -> f64 + Send + Sync>,
    pub sleep: Arc<dyn Fn(f64) + Send + Sync>,
}
impl Default for Clocks {
    fn default() -> Self {
        Clocks {
            now: Arc::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as f64)
                    .unwrap_or(0.0)
            }),
            sleep: Arc::new(|ms| std::thread::sleep(Duration::from_millis(ms as u64))),
        }
    }
}

struct Inner {
    next: f64,
    cooldown: f64,
    requests: u64,
    manual_review: bool,
}

pub struct ReadClient {
    key: String,
    pub interval: u64,
    pub write_interval: u64,
    pub read_only: bool,
    completion_pacing: bool,
    fetch: Arc<dyn Fetch>,
    signal: Arc<AtomicBool>,
    clocks: Clocks,
    on_admission: Arc<dyn Fn(Option<f64>) + Send + Sync>,
    on_timing: Arc<dyn Fn(&V) -> Result<(), String> + Send + Sync>,
    authorize_dispatch: Option<Authorizer>,
    timing: std::sync::Mutex<LatencyClock>,
    tail: Mutex<Inner>,
}

pub struct ReadClientOptions {
    pub interval: u64,
    pub network_budget: Option<V>,
    pub fetch: Option<Arc<dyn Fetch>>,
    pub signal: Option<Arc<AtomicBool>>,
    pub clocks: Clocks,
    pub on_admission: Option<Arc<dyn Fn(Option<f64>) + Send + Sync>>,
    pub on_timing: Option<Arc<dyn Fn(&V) -> Result<(), String> + Send + Sync>>,
    pub authorize_dispatch: Option<Authorizer>,
    pub monitor_event_loop: bool,
    // Mirrors createLatencyClock({now !== Date.now}) → source 'injected'.
    pub injected_clock: bool,
}
impl Default for ReadClientOptions {
    fn default() -> Self {
        ReadClientOptions {
            interval: 6000,
            network_budget: None,
            fetch: None,
            signal: None,
            clocks: Clocks::default(),
            on_admission: None,
            on_timing: None,
            authorize_dispatch: None,
            monitor_event_loop: true,
            injected_clock: false,
        }
    }
}

impl ReadClient {
    pub fn new(key: &str, options: ReadClientOptions) -> Result<ReadClient, String> {
        let interval = network_interval(options.interval as i64, options.network_budget.as_ref())?;
        let write_interval =
            network_write_interval(options.interval as i64, options.network_budget.as_ref())?;
        let read_only = options
            .network_budget
            .as_ref()
            .and_then(|b| b.get("schemaVersion"))
            .and_then(V::as_i64)
            == Some(3);
        Ok(ReadClient {
            key: key.to_string(),
            interval,
            write_interval,
            read_only,
            completion_pacing: options.network_budget.is_some(),
            fetch: options.fetch.unwrap_or_else(|| Arc::new(ReqwestFetch::new().unwrap())),
            signal: options.signal.unwrap_or_else(|| Arc::new(AtomicBool::new(false))),
            clocks: options.clocks,
            on_admission: options.on_admission.unwrap_or_else(|| Arc::new(|_| {})),
            on_timing: options.on_timing.unwrap_or_else(|| Arc::new(|_| Ok(()))),
            authorize_dispatch: options.authorize_dispatch,
            timing: std::sync::Mutex::new(if options.injected_clock {
                LatencyClock::injected()
            } else {
                LatencyClock::new(options.monitor_event_loop)
            }),
            tail: Mutex::new(Inner { next: 0.0, cooldown: 0.0, requests: 0, manual_review: false }),
        })
    }
    fn mono(&self) -> f64 {
        self.timing.lock().map(|t| t.now()).unwrap_or(0.0)
    }
    fn now(&self) -> f64 {
        (self.clocks.now)()
    }
    fn aborted(&self) -> bool {
        self.signal.load(Ordering::SeqCst)
    }
    fn pause(&self, ms: f64) -> Result<(), ServiceError> {
        let until = self.now() + ms;
        while self.now() < until {
            if self.aborted() {
                return Err(stopped_err());
            }
            (self.clocks.sleep)((until - self.now()).min(200.0));
        }
        Ok(())
    }
    pub fn abort(&self) {
        self.signal.store(true, Ordering::SeqCst);
    }

    // request(endpoint): serialized on the tail; queueWaitMs covers the wait
    // for earlier calls on this same client.
    pub fn request(&self, endpoint: &str) -> Result<V, ServiceError> {
        let queued_at = self.now();
        let queued_mono = self.mono();
        let mut inner = self.tail.lock().map_err(|_| err("Stopped"))?;
        self.perform(&mut inner, endpoint, queued_at, None, queued_mono)
    }
    pub fn dispatch(&self, spec: DispatchSpec) -> Result<V, ServiceError> {
        if self.read_only && spec.method != "GET" {
            return Err(err("Shared account budget forbids Codex writes"));
        }
        let safe = spec.safe_view();
        let authorized = self
            .authorize_dispatch
            .as_ref()
            .is_some_and(|f| f(&safe));
        if !authorized {
            return Err(err("Execution dispatch is not authorized"));
        }
        let queued_at = self.now();
        let queued_mono = self.mono();
        let mut inner = self.tail.lock().map_err(|_| err("Stopped"))?;
        self.perform(&mut inner, &spec.endpoint.clone(), queued_at, Some(&spec), queued_mono)
    }

    fn perform(
        &self,
        inner: &mut Inner,
        endpoint: &str,
        queued_at: f64,
        dispatch: Option<&DispatchSpec>,
        queued_mono: f64,
    ) -> Result<V, ServiceError> {
        if self.read_only && dispatch.is_some_and(|d| d.method != "GET") {
            return Err(err("Shared account budget forbids Codex writes"));
        }
        let executing_mono = self.mono();
        let interval =
            if dispatch.is_some_and(|d| d.method != "GET") { self.write_interval } else { self.interval } as f64;
        if let Some(d) = dispatch {
            let authorized = self.authorize_dispatch.as_ref().is_some_and(|f| f(&d.safe_view()));
            if !authorized {
                return Err(err("Execution dispatch is not authorized"));
            }
        } else if !endpoint_allowed(endpoint) {
            return Err(err("Endpoint outside paper GET allowlist"));
        }
        let (kind, market_id) = kind_of(endpoint);
        let mut event = json!({
            "schemaVersion": 2,
            "durationClock": self.timing.lock().map(|t| t.source()).unwrap_or("monotonic"),
            "kind": kind,
            "marketId": market_id,
            "queuedAt": iso(queued_at),
            "admittedAt": V::Null,
            "finishedAt": V::Null,
            "queueWaitMs": (executing_mono - queued_mono).max(0.0),
            "admissionWaitMs": V::Null,
            "headersMs": V::Null,
            "bodyMs": V::Null,
            "serviceMs": V::Null,
            "totalMs": V::Null,
            "statusCode": V::Null,
            "outcome": "blocked",
        });
        let mut admitted_at: Option<f64> = None;
        let mut admitted_mono: Option<f64> = None;
        let mut headers_mono: Option<f64> = None;
        let result = self.perform_inner(
            inner, endpoint, dispatch, &mut event, interval, executing_mono,
            &mut admitted_at, &mut admitted_mono, &mut headers_mono,
        );
        // finally-block bookkeeping
        let finished = self.now();
        let finished_mono = self.mono();
        event["finishedAt"] = json!(iso(finished));
        event["totalMs"] = json!((finished_mono - queued_mono).max(0.0));
        if let Some(am) = admitted_mono {
            event["serviceMs"] = json!((finished_mono - am).max(0.0));
            if headers_mono.is_none() {
                event["headersMs"] = event["serviceMs"].clone();
            }
        } else {
            event["admissionWaitMs"] = json!((finished_mono - executing_mono).max(0.0));
        }
        if let Ok(mut t) = self.timing.lock() {
            let snap = t.snapshot();
            for k in ["eventLoopSamples", "eventLoopP99Ms", "eventLoopMaxMs"] {
                event[k] = snap.get(k).cloned().unwrap_or(V::Null);
            }
        }
        if self.completion_pacing && admitted_at.is_some() && !inner.manual_review {
            inner.next = inner.next.max(finished + interval).max(inner.cooldown);
            (self.on_admission)(Some(inner.next));
        }
        if let Err(e) = (self.on_timing)(&event) {
            let _ = e;
            inner.manual_review = true;
            (self.on_admission)(None);
            return Err(err("Read timing recording failed"));
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn perform_inner(
        &self,
        inner: &mut Inner,
        endpoint: &str,
        dispatch: Option<&DispatchSpec>,
        event: &mut V,
        interval: f64,
        executing_mono: f64,
        admitted_at: &mut Option<f64>,
        admitted_mono: &mut Option<f64>,
        headers_mono: &mut Option<f64>,
    ) -> Result<V, ServiceError> {
        if inner.manual_review {
            return Err(err("Rate cooldown requires manual review"));
        }
        let wait = inner.next.max(inner.cooldown) - self.now();
        if let Err(e) = self.pause(wait.max(0.0)) {
            event["outcome"] = json!("aborted");
            return Err(e);
        }
        if self.aborted() {
            event["outcome"] = json!("aborted");
            return Err(stopped_err());
        }
        if let Some(d) = dispatch {
            let authorized = self.authorize_dispatch.as_ref().is_some_and(|f| f(&d.safe_view()));
            if !authorized {
                return Err(err("Execution dispatch is not authorized"));
            }
            if d.method != "GET" && d.before_dispatch.is_none() {
                return Err(err("Execution admission guard required"));
            }
            if let Some(g) = &d.before_dispatch {
                g().map_err(|e| ServiceError { message: e, ..err("") })?;
            }
            if self.aborted() {
                return Err(stopped_err());
            }
        }
        inner.next = self.now() + interval;
        inner.requests += 1;
        (self.on_admission)(Some(inner.next));
        if let Some(d) = dispatch {
            if let Some(g) = &d.before_network {
                g().map_err(|e| ServiceError { message: e, ..err("") })?;
            }
        }
        if self.aborted() {
            return Err(stopped_err());
        }
        let at = self.now();
        let am = self.mono();
        *admitted_at = Some(at);
        *admitted_mono = Some(am);
        event["admittedAt"] = json!(iso(at));
        event["admissionWaitMs"] = json!((am - executing_mono).max(0.0));
        let request_started_at = event["admittedAt"].as_str().unwrap_or("").to_string();

        // The fetch runs on a worker thread so an abort is honored mid-request
        // exactly like AbortSignal.any([signal, timeout(20000)]).
        let url = format!("{API_BASE}{endpoint}");
        let fetch = self.fetch.clone();
        let req = FetchRequest {
            method: dispatch.map(|d| d.method.clone()).unwrap_or_else(|| "GET".into()),
            headers: {
                let mut h = vec![("Authorization".to_string(), format!("Bearer {}", self.key))];
                if dispatch.and_then(|d| d.body.as_ref()).is_some() {
                    h.push(("Content-Type".to_string(), "application/json".to_string()));
                }
                h
            },
            body: dispatch.and_then(|d| {
                d.body
                    .as_ref()
                    .map(|b| d.body_json.clone().unwrap_or_else(|| crate::json_text::js_stringify(b)))
            }),
            timeout_ms: 20000,
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(fetch.fetch(&url, &req));
        });
        let outcome = loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(r) => break Ok(r),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if self.aborted() {
                        break Err(());
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break Ok(Err("fetch worker died".into())),
            }
        };
        let outcome = match outcome {
            Err(()) => {
                event["outcome"] = json!("aborted");
                return Err(stopped_err());
            }
            Ok(Err(_)) => {
                event["outcome"] = json!(if self.aborted() { "aborted" } else { "connection_error" });
                let mut e = err(if self.aborted() { "Stopped" } else { "Market API connection unavailable" });
                e.recoverable = !self.aborted();
                e.stopped = self.aborted();
                return Err(e);
            }
            Ok(Ok(o)) => o,
        };
        let hm = self.mono();
        *headers_mono = Some(hm);
        event["headersMs"] = json!((hm - admitted_mono.unwrap_or(hm)).max(0.0));
        event["statusCode"] = json!(outcome.status);
        if !(200..300).contains(&outcome.status) {
            event["outcome"] = json!("http_error");
            if outcome.status == 429 || outcome.status == 503 {
                let delay = (if outcome.status == 429 { 60000.0f64 } else { 10000.0f64 })
                    .max(retry_after_ms(outcome.retry_after.as_deref(), self.now()));
                if delay == 9_007_199_254_740_991.0 || delay > 8.64e15 - self.now() {
                    inner.manual_review = true;
                    (self.on_admission)(None);
                    return Err(err("Rate cooldown requires manual review"));
                }
                inner.cooldown = self.now() + delay;
                (self.on_admission)(Some(inner.next.max(inner.cooldown)));
            }
            let mut api_code: Option<String> = None;
            if let Ok(failure) = serde_json::from_str::<V>(&outcome.body) {
                let code = failure
                    .get("code")
                    .or_else(|| failure.get("error").and_then(|e| e.get("code")))
                    .and_then(V::as_str);
                if let Some(c) = code {
                    if [
                        "REQUEST_IN_FLIGHT", "ORDER_STATUS_UNKNOWN", "IDEMPOTENCY_KEY_REUSED",
                        "IDEMPOTENCY_CONFLICT", "IDEMPOTENCY_KEY_CONFLICT", "ORDER_NOT_FOUND",
                        "INSUFFICIENT_BALANCE", "INVALID_ORDER",
                    ]
                    .contains(&c)
                    {
                        api_code = Some(c.to_string());
                    }
                }
            }
            event["bodyMs"] = json!((self.mono() - hm).max(0.0));
            let mut e = err(&format!("Market API HTTP {}", outcome.status));
            e.status = Some(outcome.status);
            e.api_code = api_code.clone();
            e.retry_after_ms = [
                0f64,
                inner.cooldown - self.now(),
                if api_code.as_deref() == Some("REQUEST_IN_FLIGHT") { 90000.0 } else { 0.0 },
            ]
            .iter()
            .cloned()
            .fold(0.0, f64::max);
            e.recoverable = [429, 500, 502, 503, 504].contains(&outcome.status);
            return Err(e);
        }
        let data: V = match serde_json::from_str(&outcome.body) {
            Ok(v) => v,
            Err(_) => {
                event["outcome"] = json!(if self.aborted() { "aborted" } else { "body_error" });
                event["bodyMs"] = json!((self.mono() - hm).max(0.0));
                return Err(err("Invalid market API JSON"));
            }
        };
        event["bodyMs"] = json!((self.mono() - hm).max(0.0));
        if self.aborted() {
            event["outcome"] = json!("aborted");
            return Err(stopped_err());
        }
        event["outcome"] = json!("success");
        let mut r = Map::new();
        r.insert("endpoint".into(), json!(endpoint));
        r.insert("requestStartedAt".into(), json!(request_started_at));
        r.insert("receivedAt".into(), json!(iso(self.now())));
        r.insert("data".into(), data);
        if dispatch.is_some() {
            r.insert("status".into(), json!(outcome.status));
            r.insert("retryAfterMs".into(), json!(0));
        }
        Ok(V::Object(r))
    }

    // orders(): paginated open orders; cursor must advance and never repeat.
    pub fn orders(&self) -> Result<Vec<V>, ServiceError> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        let mut seen = std::collections::BTreeSet::new();
        loop {
            let endpoint = format!(
                "/orders?status=open&limit=200&tournamentId={TOURNAMENT_ID}{}",
                cursor
                    .as_ref()
                    .map(|c| format!("&cursor={}", crate::history::encode_uri_component(c)))
                    .unwrap_or_default()
            );
            let r = self.request(&endpoint)?;
            let data = r.get("data").cloned().unwrap_or(V::Null);
            let rows = data.get("data").and_then(V::as_array).cloned().unwrap_or_default();
            if data.get("data").and_then(V::as_array).is_none() {
                return Err(err("Invalid open orders response"));
            }
            let _ = exclusion_set(&json!([]), &V::Array(rows.clone())).map_err(err_str)?;
            out.extend(rows);
            let pg = data.get("pagination").cloned().unwrap_or(V::Null);
            let has_more = pg.get("hasMore").and_then(V::as_bool);
            match has_more {
                None => return Err(err("Incomplete order pagination")),
                Some(false) => break,
                Some(true) => {
                    let next = pg.get("nextCursor").and_then(V::as_str).map(str::to_string);
                    match next {
                        Some(c) if !c.is_empty() && !seen.contains(&c) => {
                            seen.insert(c.clone());
                            cursor = Some(c);
                        }
                        _ => return Err(err("Incomplete order pagination")),
                    }
                }
            }
        }
        Ok(out)
    }
    pub fn requests(&self) -> u64 {
        self.tail.lock().map(|i| i.requests).unwrap_or(0)
    }
    pub fn close(&self) {
        if let Ok(mut t) = self.timing.lock() {
            let _ = t.snapshot();
        }
    }
}
fn err_str(e: String) -> ServiceError {
    err(&e)
}
