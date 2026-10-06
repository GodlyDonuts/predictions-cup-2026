// capture-live/transport.mjs — one adapter per configured market: token
// bootstrap + private channel subscribe + snapshot reads. No account
// subscriptions or order methods exist here.
use baal_service_runtime::contract::{config, require_that, snapshot_path, Scope};
use baal_service_runtime::wait::{check_stop, retry_after_ms};
use serde_json::Value as V;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;

pub const BASE: &str = "https://sig.thesuper.market/api/v1";
const EVENTS: [&str; 3] = ["market_batch", "book_dirty", "market_settled"];
/// f64::MAX_SAFE_INTEGER sentinel for an unbounded Retry-After.
const MAX_SAFE: f64 = 9007199254740991.0;

#[derive(Clone, Default)]
pub struct AbortSignal(pub Arc<AtomicBool>);
impl AbortSignal {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
    pub fn aborted(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub fn abort(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// JS RecoveryError — retryable transport failure marker carrying the status.
pub struct RecoveryError {
    pub status: Option<u16>,
}
impl std::fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self.status {
            Some(s) => write!(f, "RecoveryError({s})"),
            None => f.write_str("RecoveryError"),
        }
    }
}

/// fetch injection point.
pub trait FetchFn {
    fn fetch(&self, url: &str, req: &FetchSpec) -> Result<FetchResponse, String>;
}
pub struct FetchSpec {
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub timeout_ms: f64,
}
pub struct FetchResponse {
    pub ok: bool,
    pub status: u16,
    pub retry_after: Option<String>,
    pub body: String,
}

/// Supabase JS client injection point.
pub trait SupabaseClient {
    fn set_auth(&self, token: &str) -> Result<(), String>;
    fn channel(&self, topic: &str) -> Result<Box<dyn SupabaseChannel>, String>;
    fn remove_channel(&self, channel: Box<dyn SupabaseChannel>) -> Result<(), String>;
    fn disconnect(&self);
}
pub trait SupabaseChannel {
    /// cb(event payload); statuses delivered to subscribe cb are SUBSCRIBED /
    /// CHANNEL_ERROR / TIMED_OUT / CLOSED.
    fn on_broadcast(&mut self, event: &str, cb: Box<dyn Fn(&V) + Send + Sync>);
    /// Synchronous subscribe: the impl must invoke the status callback before
    /// returning (the JS awaits the subscribe handshake).
    fn subscribe(&mut self, cb: Box<dyn Fn(&str) + Send + Sync>);
}

/// Shared account budget injection point.
pub trait SharedBudget {
    fn shared_authority(&self) -> bool;
    fn admit(&self, method: &str, path: &str, cost: i64, signal: &AbortSignal) -> Result<(), String>;
    fn cooldown(&self, retry_after_ms: f64, requires_manual_review: bool) -> Result<(), String>;
}

struct Session {
    origin: String,
    expires: f64,
}

pub struct BookTransport {
    key: RefCell<Option<String>>,
    session: RefCell<Option<Session>>,
    client: RefCell<Option<Rc<dyn SupabaseClient>>>,
    channel: RefCell<Option<Box<dyn SupabaseChannel>>>,
    channel_id: Arc<AtomicI64>,
    controller: AbortSignal,
    generation: Arc<AtomicI64>,
    pub scope: Scope,
    create_client: Rc<dyn Fn(&str, &str) -> Rc<dyn SupabaseClient>>,
    budget: Rc<dyn SharedBudget>,
    fetch: Rc<dyn FetchFn>,
    now: Rc<dyn Fn() -> f64>,
    timeout_ms: f64,
    pub on_disconnect: RefCell<Arc<dyn Fn() + Send + Sync>>,
    pub mode: &'static str,
    pub reads: Cell<i64>,
    pub auth_posts: Cell<i64>,
    closed: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
}

impl BookTransport {
    pub fn new(
        scope: &V,
        key: &str,
        create_client: Rc<dyn Fn(&str, &str) -> Rc<dyn SupabaseClient>>,
        budget: Rc<dyn SharedBudget>,
        token_setup_approved: bool,
        fetch: Rc<dyn FetchFn>,
        now: Rc<dyn Fn() -> f64>,
        timeout_ms: f64,
        on_disconnect: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        require_that(token_setup_approved, "TOKEN_SETUP_REVIEW_REQUIRED").map_err(|e| e.to_string())?;
        require_that(!key.is_empty(), "INVALID_RUNTIME").map_err(|e| e.to_string())?;
        require_that(budget.shared_authority(), "SHARED_ACCOUNT_BUDGET_REQUIRED")
            .map_err(|e| e.to_string())?;
        require_that(
            timeout_ms.is_finite() && timeout_ms > 0.0 && timeout_ms <= 30000.0,
            "INVALID_TIMEOUT",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            key: RefCell::new(Some(key.to_string())),
            session: RefCell::new(None),
            client: RefCell::new(None),
            channel: RefCell::new(None),
            channel_id: Arc::new(AtomicI64::new(0)),
            controller: AbortSignal::new(),
            generation: Arc::new(AtomicI64::new(0)),
            scope: config(scope).map_err(|e| e.to_string())?,
            create_client,
            budget,
            fetch,
            now,
            timeout_ms,
            on_disconnect: RefCell::new(on_disconnect),
            mode: "read_only_transport",
            reads: Cell::new(0),
            auth_posts: Cell::new(0),
            closed: Arc::new(AtomicBool::new(false)),
            connected: Arc::new(AtomicBool::new(false)),
        })
    }

    fn signal(&self, signal: Option<&AbortSignal>) -> AbortSignal {
        let _ = signal;
        self.controller.clone()
    }

    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
    pub fn connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    pub fn request(
        &self,
        method: &str,
        path: &str,
        signal: Option<&AbortSignal>,
    ) -> Result<V, String> {
        let signal = self.signal(signal);
        check_stop(signal.aborted(), &|| false).map_err(|e| e.to_string())?;
        require_that(!self.closed(), "TRANSPORT_CLOSED").map_err(|e| e.to_string())?;
        let snap = snapshot_path(&self.scope);
        require_that(
            (method == "POST" && path == "/realtime/token") || (method == "GET" && path == snap),
            "ENDPOINT_NOT_ALLOWED",
        )
        .map_err(|e| e.to_string())?;
        self.budget.admit(method, path, 1, &signal)?;
        check_stop(signal.aborted(), &|| false).map_err(|e| e.to_string())?;
        require_that(!self.closed(), "TRANSPORT_CLOSED").map_err(|e| e.to_string())?;
        if method == "POST" {
            self.auth_posts.set(self.auth_posts.get() + 1);
        } else {
            self.reads.set(self.reads.get() + 1);
        }
        let key = self.key.borrow().clone().unwrap_or_default();
        let resp = self
            .fetch
            .fetch(
                &format!("{BASE}{path}"),
                &FetchSpec {
                    method: method.into(),
                    headers: vec![
                        ("Authorization".into(), format!("Bearer {key}")),
                        ("Content-Type".into(), "application/json".into()),
                    ],
                    timeout_ms: self.timeout_ms,
                },
            )
            .map_err(|_| RecoveryError { status: None }.to_string())?;
        check_stop(signal.aborted(), &|| false).map_err(|e| e.to_string())?;
        if !resp.ok {
            let delay = retry_after_ms(resp.retry_after.as_deref(), (self.now)())
                .max(if resp.status == 429 { 60000.0 } else { 0.0 });
            let _ = self
                .budget
                .cooldown(delay, delay == MAX_SAFE);
            return Err(RecoveryError { status: Some(resp.status) }.to_string());
        }
        serde_json::from_str(&resp.body).map_err(|_| RecoveryError { status: None }.to_string())
    }

    pub fn bootstrap(&self, signal: Option<&AbortSignal>) -> Result<(), String> {
        let signal = self.signal(signal);
        let value = self.request("POST", "/realtime/token", Some(&signal))?;
        check_stop(signal.aborted(), &|| false).map_err(|e| e.to_string())?;
        // Destination comes only from the official credential bootstrap; the
        // SIG API key is never sent to it.
        let url = url::Url::parse(value["supabaseUrl"].as_str().unwrap_or(""))
            .map_err(|_| "INVALID_BOOTSTRAP".to_string())?;
        require_that(
            url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "INVALID_BOOTSTRAP",
        )
        .map_err(|e| e.to_string())?;
        let token = value["token"].as_str().unwrap_or("");
        let anon = value["anonKey"].as_str().unwrap_or("");
        require_that(!token.is_empty() && !anon.is_empty(), "INVALID_BOOTSTRAP")
            .map_err(|e| e.to_string())?;
        let expires = parse_iso(value["expiresAt"].as_str().unwrap_or(""));
        require_that(
            expires.is_finite() && expires > (self.now)() + 60000.0,
            "INVALID_TOKEN_EXPIRY",
        )
        .map_err(|e| e.to_string())?;
        let origin = url.origin().ascii_serialization();
        if let Some(s) = self.session.borrow().as_ref() {
            require_that(s.origin == origin, "BOOTSTRAP_ORIGIN_CHANGED").map_err(|e| e.to_string())?;
        }
        if let Some(c) = self.client.borrow().as_ref() {
            c.disconnect();
        }
        *self.session.borrow_mut() = Some(Session { origin: origin.clone(), expires });
        let client = (self.create_client)(&origin, anon);
        client.set_auth(token)?;
        *self.client.borrow_mut() = Some(client);
        check_stop(signal.aborted(), &|| false).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn refresh_due(&self) -> bool {
        match self.session.borrow().as_ref() {
            None => true,
            Some(s) => s.expires - (self.now)() <= 60000.0,
        }
    }

    /// subscribe: replace any channel, bootstrap if the token is due, then
    /// bind the broadcast events pinned to this generation+channel.
    pub fn subscribe(
        &self,
        topic: &str,
        signal: Option<&AbortSignal>,
        on_event: Arc<dyn Fn(&str, V) + Send + Sync>,
    ) -> Result<(), String> {
        let signal = self.signal(signal);
        require_that(topic == self.scope.topic, "TOPIC_NOT_ALLOWED").map_err(|e| e.to_string())?;
        check_stop(signal.aborted(), &|| false).map_err(|e| e.to_string())?;
        self.disconnect()?;
        if self.refresh_due() || self.client.borrow().is_none() {
            self.bootstrap(Some(&signal))?;
        }
        check_stop(signal.aborted(), &|| false).map_err(|e| e.to_string())?;
        let client = self.client.borrow().clone().unwrap();
        let mut channel = client.channel(topic)?;
        let generation = self.generation.load(Ordering::SeqCst) + 1;
        self.generation.store(generation, Ordering::SeqCst);
        let channel_id = self.channel_id.load(Ordering::SeqCst) + 1;
        let cur_channel = self.channel_id.clone();
        let terminal = Arc::new(AtomicBool::new(false));
        self.connected.store(false, Ordering::SeqCst);
        for event in EVENTS {
            let on_event = on_event.clone();
            let signal_c = signal.clone();
            let closed = self.closed.clone();
            let gen_shared = self.generation.clone();
            let term = terminal.clone();
            let cur_channel_cb = cur_channel.clone();

            channel.on_broadcast(event, Box::new(move |payload| {
                if gen_shared.load(Ordering::SeqCst) == generation
                    && cur_channel_cb.load(Ordering::SeqCst) == channel_id
                    && !term.load(Ordering::SeqCst)
                    && !closed.load(Ordering::SeqCst)
                    && !signal_c.aborted()
                {
                    on_event(event, payload.clone());
                }
            }));
        }
        let acknowledged = Arc::new(AtomicBool::new(false));
        let outcome = Arc::new(std::sync::Mutex::new(None::<Result<(), String>>));
        let on_disconnect = self.on_disconnect.borrow().clone();
        channel.subscribe(Box::new({
            let ack = acknowledged.clone();
            let term = terminal.clone();
            let conn = self.connected.clone();
            let gen_shared = self.generation.clone();
            let out = outcome.clone();
            move |status: &str| {
                if gen_shared.load(Ordering::SeqCst) != generation || term.load(Ordering::SeqCst) {
                    return;
                }
                match status {
                    "SUBSCRIBED" => {
                        ack.store(true, Ordering::SeqCst);
                        conn.store(true, Ordering::SeqCst);
                        *out.lock().unwrap() = Some(Ok(()));
                    }
                    "CHANNEL_ERROR" | "TIMED_OUT" | "CLOSED" => {
                        term.store(true, Ordering::SeqCst);
                        conn.store(false, Ordering::SeqCst);
                        if ack.load(Ordering::SeqCst) {
                            on_disconnect();
                            *out.lock().unwrap() = Some(Err("DISCONNECTED_AFTER_ACK".into()));
                        } else {
                            *out.lock().unwrap() = Some(Err("SUBSCRIPTION_FAILED".into()));
                        }
                    }
                    _ => {}
                }
            }
        }));
        check_stop(signal.aborted(), &|| false).map_err(|e| e.to_string())?;
        match outcome.lock().unwrap().take() {
            Some(Ok(()))
                if !terminal.load(Ordering::SeqCst)
                    && self.connected()
                    && self.generation.load(Ordering::SeqCst) == generation =>
            {
                self.channel_id.store(channel_id, Ordering::SeqCst);
                *self.channel.borrow_mut() = Some(channel);
                Ok(())
            }
            _ => {
                let _ = client.remove_channel(channel);
                Err("SUBSCRIPTION_FAILED".into())
            }
        }
    }

    pub fn read_snapshot(
        &self,
        method: &str,
        path: &str,
        signal: Option<&AbortSignal>,
    ) -> Result<V, String> {
        let snap = snapshot_path(&self.scope);
        require_that(method == "GET" && path == snap, "ENDPOINT_NOT_ALLOWED")
            .map_err(|e| e.to_string())?;
        self.request(method, path, signal)
    }

    pub fn disconnect(&self) -> Result<(), String> {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.connected.store(false, Ordering::SeqCst);
        if let Some(channel) = self.channel.borrow_mut().take() {
            if let Some(client) = self.client.borrow().as_ref() {
                // Cleanup never exposes raw SDK diagnostics.
                let _ = client.remove_channel(channel);
            }
        }
        Ok(())
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.controller.abort();
        let _ = self.disconnect();
        if let Some(client) = self.client.borrow().as_ref() {
            client.disconnect();
        }
        *self.client.borrow_mut() = None;
        *self.key.borrow_mut() = None;
    }
}

fn parse_iso(s: &str) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(s).unwrap_or(f64::NAN)
}
