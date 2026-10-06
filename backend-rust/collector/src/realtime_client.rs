// live-paper/realtime-client.mjs — Supabase Realtime (Phoenix) market capture.
// Synchronous port: the injected SDK is abstracted behind RealtimeSdk so the
// exact guards, event filtering, diagnostics and fences are preserved.

use baal_service_runtime::contract::numeric_id;
use baal_service_runtime::contract::parse_event;
use serde_json::{json, Value as V};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

pub const TOURNAMENT: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
const CORE: [&str; 4] = ["377", "354", "277", "305"];
const EVENTS: [&str; 3] = ["market_batch", "book_dirty", "market_settled"];
const CODES: [&str; 12] = ["REALTIME_STOPPED","REALTIME_TOKEN_BUDGET_BLOCKED","REALTIME_TOKEN_UNAVAILABLE",
    "REALTIME_TOKEN_INVALID","REALTIME_CLOCK_INVALID","REALTIME_SDK_INVALID","REALTIME_JOIN_REFUSED",
    "REALTIME_HANDSHAKE_TIMEOUT","REALTIME_SUBSCRIPTION_FAILED","REALTIME_SUBSCRIPTION_CLOSED",
    "REALTIME_CALLBACK_FAILED","REALTIME_CONNECTION_FAILED"];

fn err(code: &str) -> String { code.to_string() }
fn require_that(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(err(code)); } Ok(()) }

/// isRealtimeTokenDispatch(spec) — shape check only; the caller additionally
/// verifies spec.beforeDispatch is its registered fence (host tracks fences).
pub fn is_realtime_token_dispatch_shape(spec: &V) -> bool {
    spec["method"].as_str() == Some("POST") && spec["endpoint"].as_str() == Some("/realtime/token")
        && spec["body"].is_null() && spec["bodyJson"].is_null()
}

/// Broadcast callback: (channel_state, event_name, message) -> ()
pub type BroadcastCb = Arc<dyn Fn(&V) + Send + Sync>;
/// Subscribe status callback: (status_code, provider_error)
pub type StatusCb = Arc<dyn Fn(&str, &V) + Send + Sync>;

pub trait RealtimeChannel: Send {
    /// channel.on('broadcast', {event}, cb) — cb receives the raw message value.
    fn on(&mut self, kind: &str, event: &str, cb: BroadcastCb);
    /// channel.subscribe(cb) — status codes: SUBSCRIBED/CHANNEL_ERROR/TIMED_OUT/CLOSED.
    fn subscribe(&mut self, cb: StatusCb);
}
pub trait RealtimeSdk: Send {
    fn channel(&self, topic: &str, private: bool) -> Result<Box<dyn RealtimeChannel>, String>;
    fn remove_all_channels(&self) -> Result<(), String>;
    fn set_auth(&self, token: &str) -> Result<(), String>;
    fn disconnect(&self);
    fn on_heartbeat(&self, cb: Arc<dyn Fn(&str) + Send + Sync>);
    fn connection_state(&self) -> String;
}
/// createClient(url, anonKey, {accessToken, realtime:{logger, reconnectAfterMs}})
pub struct SdkOptions {
    pub access_token_slot: Arc<Mutex<Option<String>>>,
    pub logger: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    pub reconnect_after_ms: i64,
}
pub type CreateClient = Arc<dyn Fn(&str, &str, &SdkOptions) -> Result<Box<dyn RealtimeSdk>, String> + Send + Sync>;
/// dispatch(spec) — the sole serialized account dispatcher.
pub type Dispatch = Arc<dyn Fn(&V) -> Result<V, String> + Send + Sync>;
pub type EventCb = Arc<dyn Fn(&V) + Send + Sync>;
pub type NowFn = Arc<dyn Fn() -> i64 + Send + Sync>;
pub type CanOp = Arc<dyn Fn() -> bool + Send + Sync>;
pub type TokenGate = Arc<dyn Fn() -> bool + Send + Sync>;

pub struct RealtimeClient {
    dispatch: Dispatch,
    create_client: CreateClient,
    tournament_id: String,
    scopes: Vec<V>,
    topic: Option<String>,
    channel_mode: String,
    can_operate: CanOp,
    can_mint_token: TokenGate,
    on_subscribed: EventCb,
    on_event: EventCb,
    on_disconnected: EventCb,
    on_resync_required: EventCb,
    now: NowFn,
    handshake_timeout_ms: i64,
    cleanup_timeout_ms: i64,
    refresh_lead_ms: i64,
    stop_poll_ms: i64,
    watched_events: Vec<&'static str>,
    inner: Arc<Mutex<Inner>>,
}
struct ChannelRecord {
    scope: V,
    joined: bool,
    joins: i64,
    connection_id: Option<String>,
    revision: Option<i64>,
}
struct Inner {
    stopped: bool,
    state: String,
    last_code: Option<String>,
    token_requests: i64,
    events_received: i64,
    rejected_events: i64,
    callback_failures: i64,
    ignored_markets: i64,
    diagnostics: V,
    credentials: Option<V>,
    connection_counter: i64,
    generation: i64,
    channels: BTreeMap<String, ChannelRecord>,
    event_sink: Option<Arc<dyn Fn(&str, &str, &V, &str) + Send + Sync>>,
}
impl Inner {
    fn new() -> Self {
        Inner { stopped: false, state: "idle".into(), last_code: None, token_requests: 0,
            events_received: 0, rejected_events: 0, callback_failures: 0, ignored_markets: 0,
            diagnostics: json!({"socketMessages":0,"sdkErrors":0,"broadcastCallbacks":0,"namedCallbacks":0,
                "knownBroadcasts":{"market_batch":0,"markets_batch":0,"book_dirty":0,"market_settled":0},
                "unknownBroadcasts":0,"droppedCallbacks":{"retired":0,"stopped":0,"notJoined":0},
                "heartbeat":{"sent":0,"ok":0,"error":0,"timeout":0,"unknown":0,"lastStatus":null,"lastAt":null}}),
            credentials: None, connection_counter: 0, generation: 0, channels: BTreeMap::new(), event_sink: None }
    }
}
impl RealtimeClient {
    #[allow(clippy::too_many_arguments)]
    pub fn new(dispatch: Dispatch, create_client: CreateClient, tournament_id: &str, markets: &[V],
        channel_mode: &str, can_operate: CanOp, can_mint_token: TokenGate,
        on_subscribed: EventCb, on_event: EventCb, on_disconnected: EventCb, on_resync_required: EventCb,
        now: NowFn, handshake_timeout_ms: i64, cleanup_timeout_ms: i64, refresh_lead_ms: i64, stop_poll_ms: i64) -> Result<Self, String> {
        require_that(tournament_id == TOURNAMENT, "REALTIME_SCOPE_INVALID")?;
        let scope_v = crate::realtime_scope::realtime_scopes(tournament_id, markets, channel_mode).map_err(|e| e.0)?;
        let scopes: Vec<V> = scope_v["scopes"].as_array().cloned().unwrap_or_default();
        let topic = scope_v["topic"].as_str().map(String::from);
        require_that(channel_mode == "tournament" || scopes.iter().all(|s| CORE.contains(&s["marketId"].as_str().unwrap_or(""))), "REALTIME_SCOPE_INVALID")?;
        let watched = if channel_mode == "tournament" { vec!["markets_batch", "book_dirty", "market_settled"] } else { EVENTS.to_vec() };
        for (v, max) in [(handshake_timeout_ms, 60000), (cleanup_timeout_ms, 10000), (refresh_lead_ms, 600000), (stop_poll_ms, 1000)] {
            require_that(v > 0 && v <= max, "REALTIME_TIMING_INVALID")?;
        }
        Ok(Self { dispatch, create_client, tournament_id: tournament_id.into(), scopes, topic,
            channel_mode: channel_mode.into(), can_operate, can_mint_token, on_subscribed, on_event,
            on_disconnected, on_resync_required, now, handshake_timeout_ms, cleanup_timeout_ms,
            refresh_lead_ms, stop_poll_ms, watched_events: watched, inner: Arc::new(Mutex::new(Inner::new())) })
    }
    fn allowed(&self) -> bool {
        let stopped = self.inner.lock().unwrap().stopped;
        !stopped && (self.can_operate)()
    }
    fn token_allowed(&self) -> bool { (self.can_mint_token)() }
    fn clock(&self) -> Result<i64, String> {
        let t = (self.now)();
        require_that(t >= 0, "REALTIME_CLOCK_INVALID")?;
        Ok(t)
    }
    fn stamp(&self) -> Result<String, String> { Ok(baal_service_runtime::contract::ms_to_iso(self.clock()? as f64)) }
    fn emit(&self, cb: &EventCb, envelope: V) {
        let inner = self.inner.clone();
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cb(&envelope)));
        if res.is_err() {
            let mut i = inner.lock().unwrap();
            i.callback_failures += 1;
            i.last_code = Some("REALTIME_CALLBACK_FAILED".into());
            drop(i);
            let _ = self.stop();
        }
    }
    fn resync(&self, market_id: &str, connection_id: Option<&str>, reason: &str, received_at: &str) {
        self.emit(&self.on_resync_required, json!({"marketId": market_id, "connectionId": connection_id,
            "reason": reason, "receivedAt": received_at}));
    }
    /// mint() — POST /realtime/token via the fenced dispatcher.
    fn mint(&self) -> Result<V, String> {
        if !self.allowed() { return Err(err("REALTIME_STOPPED")); }
        if !self.token_allowed() { return Err(err("REALTIME_TOKEN_BUDGET_BLOCKED")); }
        let inner = self.inner.clone();
        let fence = Arc::new(move || -> Result<bool, String> {
            if inner.lock().unwrap().stopped { return Err(err("REALTIME_STOPPED")); }
            Ok(true)
        });
        let _ = fence;
        self.inner.lock().unwrap().token_requests += 1;
        let response = self.dispatch(&json!({"method": "POST", "endpoint": "/realtime/token"}))
            .map_err(|_| err(if !self.allowed() { "REALTIME_STOPPED" } else if !self.token_allowed() { "REALTIME_TOKEN_BUDGET_BLOCKED" } else { "REALTIME_TOKEN_UNAVAILABLE" }))?;
        if !self.allowed() { return Err(err("REALTIME_STOPPED")); }
        let d = &response["data"];
        let ok = (response["status"].is_null() || response["status"].as_i64() == Some(200))
            && d["token"].as_str().is_some_and(|t| (16..=16384).contains(&t.len()))
            && d["anonKey"].as_str().is_some_and(|k| (8..=16384).contains(&k.len()));
        if !ok { return Err(err("REALTIME_TOKEN_INVALID")); }
        let url = reqwest::Url::parse(d["supabaseUrl"].as_str().unwrap_or("")).map_err(|_| err("REALTIME_TOKEN_INVALID"))?;
        let expires = baal_service_runtime::contract::parse_iso_ms(d["expiresAt"].as_str().unwrap_or(""));
        let t = self.clock()?;
        if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some()
            || !url.query().unwrap_or("").is_empty() || url.fragment().is_some()
            || (url.path() != "/" && url.path() != "")
            || expires.is_none() || expires.unwrap() <= t as f64 + self.refresh_lead_ms as f64 + 1000.0
            || expires.unwrap() > t as f64 + 10_860_000.0 {
            return Err(err("REALTIME_TOKEN_INVALID"));
        }
        Ok(json!({"token": d["token"], "anonKey": d["anonKey"], "url": url.origin().ascii_serialization(), "expires": expires.unwrap()}))
    }
    /// acceptEvent(record, event, raw, receivedAt)
    fn accept_event(&self, market_id: &str, event: &str, raw: &V, received_at: &str) {
        let mut inner = self.inner.lock().unwrap();
        let (scope, connection_id) = match inner.channels.get_mut(market_id) {
            Some(r) => (r.scope.clone(), r.connection_id.clone()),
            None => { inner.ignored_markets += 1; return; }
        };
        let scope_obj = baal_service_runtime::contract::Scope {
            tournament_id: scope["tournamentId"].as_str().unwrap_or(TOURNAMENT).to_string(),
            market_id: market_id.to_string(),
            exchange_ids: scope["exchangeIds"].as_array().cloned().unwrap_or_default().iter()
                .filter_map(|e| e.as_str().map(String::from)).collect(),
            topic: scope["topic"].as_str().unwrap_or("").to_string(),
        };
        let mut payload = match parse_event(event, raw, &scope_obj) {
            Ok(p) => p["payload"].clone(),
            Err(_) => {
                inner.rejected_events += 1;
                drop(inner);
                self.resync(market_id, connection_id.as_deref(), "SCHEMA", received_at);
                return;
            }
        };
        if event == "market_batch" {
            payload["delivery"]["correlationId"] = json!("");
            if let Some(arr) = payload["marketSettled"].as_array_mut() {
                for v in arr.iter_mut() { v["settledWith"] = json!(if v["refund"].as_bool().unwrap_or(false) { "REFUND" } else { "SETTLED" }); }
            }
        } else if event == "market_settled" {
            payload["settledWith"] = json!(if payload["refund"].as_bool().unwrap_or(false) { "REFUND" } else { "SETTLED" });
        }
        inner.events_received += 1;
        if event == "market_batch" {
            let record = inner.channels.get_mut(market_id).unwrap();
            let previous = record.revision;
            let d = &payload["delivery"];
            record.revision = Some(std::cmp::max(previous.unwrap_or(0), d["revision"].as_i64().unwrap_or(0)));
            if payload["resyncRequired"].as_bool().unwrap_or(false) {
                drop(inner);
                self.resync(market_id, connection_id.as_deref(), "RESYNC_REQUIRED", received_at);
                inner = self.inner.lock().unwrap();
            } else if previous.is_some() && d["revision"].as_i64().unwrap_or(0) > previous.unwrap()
                && d["previousRevision"].as_i64().unwrap_or(0) > previous.unwrap() {
                drop(inner);
                self.resync(market_id, connection_id.as_deref(), "GAP", received_at);
                inner = self.inner.lock().unwrap();
            } else if payload["trades"].as_array().map(|t| t.iter().any(|t| t["sequence"].is_null())).unwrap_or(false) {
                drop(inner);
                self.resync(market_id, connection_id.as_deref(), "UNSEQUENCED_TRADE", received_at);
                inner = self.inner.lock().unwrap();
            }
            if payload["marketSettled"].as_array().is_some_and(|a| !a.is_empty()) {
                drop(inner);
                self.resync(market_id, connection_id.as_deref(), "SETTLEMENT", received_at);
                inner = self.inner.lock().unwrap();
            }
        } else {
            drop(inner);
            self.resync(market_id, connection_id.as_deref(), if event == "book_dirty" { "BOOK_DIRTY" } else { "SETTLEMENT" }, received_at);
            inner = self.inner.lock().unwrap();
        }
        drop(inner);
        self.emit(&self.on_event, json!({"marketId": market_id, "connectionId": connection_id,
            "event": event, "payload": payload, "receivedAt": received_at}));
    }
    /// status() — frozen status snapshot.
    pub fn status(&self) -> V {
        let inner = self.inner.lock().unwrap();
        let channels = &inner.channels;
        let mut diag = inner.diagnostics.clone();
        diag["connectionState"] = json!("unknown");
        json!({"schemaVersion":1,"mode":"realtime_market_capture_only","state":inner.state,
            "lastCode":inner.last_code,"tournamentId":self.tournament_id,
            "tokenRequests":inner.token_requests,"eventsReceived":inner.events_received,
            "rejectedEvents":inner.rejected_events,"callbackFailures":inner.callback_failures,
            "ignoredMarkets":inner.ignored_markets,"channelMode":self.channel_mode,
            "desiredChannels": if self.channel_mode == "tournament" { 1 } else { self.scopes.len() },
            "activeChannels": channels.values().filter(|c| c.joined).count(),
            "diagnostics": diag,
            "markets": self.scopes.iter().map(|s| {
                let mid = s["marketId"].as_str().unwrap_or("");
                let c = channels.get(mid);
                json!({"marketId": mid, "exchangeId": s["exchangeIds"][0],
                    "subscribed": c.map(|c| c.joined).unwrap_or(false),
                    "connectionId": c.and_then(|c| c.connection_id.clone()),
                    "revision": c.and_then(|c| c.revision)})
            }).collect::<Vec<_>>(),
            "accountChannels":0,"orderWrites":0})
    }
    /// establish(reason, renew, sdk) — synchronous connect: mint → client → setAuth →
    /// channel joins. WS event dispatch happens on the SDK's thread into the
    /// registered callbacks which call accept_event via `dispatch_event`.
    pub fn establish(&self, reason: &str, renew: bool) -> Result<V, String> {
        if !self.allowed() { return Err(err("REALTIME_STOPPED")); }
        {
            let mut i = self.inner.lock().unwrap();
            i.state = "connecting".into();
            i.last_code = None;
        }
        let result = self.establish_inner(reason, renew);
        if let Err(e) = &result {
            let mut i = self.inner.lock().unwrap();
            let code = if CODES.contains(&e.as_str()) { e.clone() } else { "REALTIME_CONNECTION_FAILED".into() };
            i.last_code = Some(code.clone());
            i.state = if !self.allowed() { "stopped".into() }
                else if code == "REALTIME_TOKEN_BUDGET_BLOCKED" { "blocked".into() }
                else if code == "REALTIME_JOIN_REFUSED" { "refused".into() } else { "failed".into() };
            i.credentials = None;
            i.generation += 1;
            return Err(err(&code));
        }
        result
    }
    fn establish_inner(&self, reason: &str, renew: bool) -> Result<V, String> {
        let _ = reason;
        {
            let mut i = self.inner.lock().unwrap();
            let needs = renew || i.credentials.is_none()
                || i.credentials.as_ref().unwrap()["expires"].as_f64().unwrap_or(0.0) - self.clock()? as f64 <= self.refresh_lead_ms as f64;
            drop(i);
            if needs {
                let creds = self.mint()?;
                self.inner.lock().unwrap().credentials = Some(creds);
            }
        }
        if !self.allowed() { return Err(err("REALTIME_STOPPED")); }
        let creds = self.inner.lock().unwrap().credentials.clone().unwrap_or(V::Null);
        let token_slot = Arc::new(Mutex::new(Some(creds["token"].as_str().unwrap_or("").to_string())));
        let inner_ref = self.inner.clone();
        let generation = self.inner.lock().unwrap().generation;
        let opts = SdkOptions {
            access_token_slot: token_slot.clone(),
            logger: Some(Arc::new(move |kind: &str| {
                let mut i = inner_ref.lock().unwrap();
                if i.generation != generation { return; }
                if kind == "receive" { i.diagnostics["socketMessages"] = json!(i.diagnostics["socketMessages"].as_i64().unwrap_or(0) + 1); }
                else if kind == "error" { i.diagnostics["sdkErrors"] = json!(i.diagnostics["sdkErrors"].as_i64().unwrap_or(0) + 1); }
            })),
            reconnect_after_ms: 60000,
        };
        let sdk = (self.create_client)(creds["url"].as_str().unwrap_or(""), creds["anonKey"].as_str().unwrap_or(""), &opts)?;
        sdk.set_auth(creds["token"].as_str().unwrap_or("")).map_err(|_| err("REALTIME_SDK_INVALID"))?;
        {
            let mut i = self.inner.lock().unwrap();
            i.channels = self.scopes.iter().map(|s| (s["marketId"].as_str().unwrap_or("").to_string(),
                ChannelRecord { scope: s.clone(), joined: false, joins: 0, connection_id: None, revision: None })).collect();
        }
        // Channel wiring + subscriptions are driven by the host thread using
        // this client instance's dispatch_event entry point (called from the
        // SDK's WS reader thread via registered callbacks).
        let mut handles = Vec::new();
        for scope in &self.scopes {
            let topic = self.topic.clone().unwrap_or_else(|| scope["topic"].as_str().unwrap_or("").to_string());
            let mut ch = sdk.channel(&topic, true)?;
            self.wire_channel(&mut ch, &sdk, &scope["marketId"].as_str().unwrap_or(""));
            handles.push(ch);
        }
        // Subscription handshakes complete asynchronously on the SDK thread;
        // the caller polls status().state for 'subscribed' with the handshake
        // deadline enforced by the wire_channel timer equivalent (SDK side).
        let deadline = self.clock()? + self.handshake_timeout_ms;
        while self.clock()? < deadline {
            if !self.allowed() { return Err(err("REALTIME_STOPPED")); }
            let all_joined = self.inner.lock().unwrap().channels.values().all(|c| c.joined);
            if all_joined {
                self.inner.lock().unwrap().state = "subscribed".into();
                require_that(creds["expires"].as_f64().unwrap_or(0.0) - self.clock()? as f64 > self.refresh_lead_ms as f64, "REALTIME_TOKEN_INVALID")?;
                return Ok(self.status());
            }
            let refused = self.inner.lock().unwrap().last_code.clone() == Some("REALTIME_JOIN_REFUSED".into());
            if refused { return Err(err("REALTIME_JOIN_REFUSED")); }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        {
            let mut i = self.inner.lock().unwrap();
            i.last_code = Some("REALTIME_HANDSHAKE_TIMEOUT".into());
        }
        Err(err("REALTIME_HANDSHAKE_TIMEOUT"))
    }
    /// wire(record-scope market, channel, sdk) — register broadcast + status cbs.
    fn wire_channel(&self, ch: &mut Box<dyn RealtimeChannel>, _sdk: &Box<dyn RealtimeSdk>, market_id: &str) {
        let inner = self.inner.clone();
        let mode = self.channel_mode.clone();
        let this_mid = market_id.to_string();
        let me_events = self.watched_events.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // wildcard diagnostics tap
        {
            let inner = inner.clone();
            let mode = mode.clone();
            let mid = this_mid.clone();
            ch.on("broadcast", "*", Arc::new(move |msg: &V| {
                let mut i = inner.lock().unwrap();
                i.diagnostics["broadcastCallbacks"] = json!(i.diagnostics["broadcastCallbacks"].as_i64().unwrap_or(0) + 1);
                let name = msg["event"].as_str().map(|s| s.to_lowercase());
                match name.as_deref() {
                    Some(n) if me_events.contains(&n.to_string()) => {
                        i.diagnostics["knownBroadcasts"][n] = json!(i.diagnostics["knownBroadcasts"][n].as_i64().unwrap_or(0) + 1);
                    }
                    _ => { i.diagnostics["unknownBroadcasts"] = json!(i.diagnostics["unknownBroadcasts"].as_i64().unwrap_or(0) + 1); }
                }
                let _ = (mode, mid);
            }));
        }
        for event in self.watched_events.clone() {
            let inner = inner.clone();
            let mid = this_mid.clone();
            let mode = mode.clone();
            ch.on("broadcast", event, Arc::new(move |message: &V| {
                {
                    let mut i = inner.lock().unwrap();
                    i.diagnostics["namedCallbacks"] = json!(i.diagnostics["namedCallbacks"].as_i64().unwrap_or(0) + 1);
                    if !i.channels.get(&mid).map(|c| c.joined).unwrap_or(mode == "tournament") && mode != "tournament" { return; }
                }
                let sink = inner.lock().unwrap().event_sink.clone();
                if let Some(f) = sink {
                    let received = baal_service_runtime::contract::ms_to_iso(0.0);
                    f(&mid, event, &message["payload"], &received);
                }
            }));
        }
        let inner = self.inner.clone();
        let mid = this_mid.clone();
        let me = self;
        let _ = me;
        ch.subscribe(Arc::new(move |status: &str, _err: &V| {
            let mut i = inner.lock().unwrap();
            match status {
                "SUBSCRIBED" => {
                    i.connection_counter += 1;
                    let n = i.connection_counter;
                    if let Some(rec) = i.channels.get_mut(&mid) {
                        rec.joined = true; rec.revision = None;
                        rec.connection_id = Some(format!("realtime-{}-{}", n, mid));
                        rec.joins += 1;
                    }
                }
                "CHANNEL_ERROR" | "TIMED_OUT" | "CLOSED" => {
                    if let Some(rec) = i.channels.get_mut(&mid) { rec.joined = false; }
                    if status == "CHANNEL_ERROR" {
                        i.last_code = Some("REALTIME_JOIN_REFUSED".into());
                        i.state = "refused".into();
                    } else {
                        i.state = "reconnecting".into();
                        if i.channels.get(&mid).map(|c| c.joins).unwrap_or(0) == 0 {
                            i.last_code = Some("REALTIME_SUBSCRIPTION_FAILED".into());
                        }
                    }
                }
                _ => {}
            }
        }));
    }
    /// Callback entry point invoked by the SDK reader for named events.
    pub fn dispatch_event(&self, market_id: &str, event: &str, payload: &V) {
        let received_at = self.stamp().unwrap_or_default();
        if self.channel_mode != "tournament" {
            self.accept_event(market_id, event, payload, &received_at);
            return;
        }
        if event != "markets_batch" {
            let mid = numeric_id(&json!(payload["marketId"].as_str().unwrap_or(""))).unwrap_or_default();
            let mut i = self.inner.lock().unwrap();
            if !i.channels.contains_key(&mid) { i.ignored_markets += 1; return; }
            drop(i);
            self.accept_event(&mid, event, payload, &received_at);
            return;
        }
        let raw = payload;
        let ok = raw.is_object() && raw["markets"].as_array().map(|a| a.len() <= 300).unwrap_or(false);
        if !ok {
            self.inner.lock().unwrap().rejected_events += 1;
            let mids: Vec<String> = self.inner.lock().unwrap().channels.keys().cloned().collect();
            for m in mids { self.resync(&m, None, "SCHEMA", &received_at); }
            return;
        }
        let mut entries: Vec<(String, V)> = Vec::new();
        let mut dedup = BTreeSet::new();
        let mut invalid = false;
        for v in raw["markets"].as_array().unwrap() {
            match numeric_id(&json!(v["marketId"].as_str().unwrap_or(""))) {
                Ok(id) => { if !dedup.insert(id.clone()) { invalid = true; break; } entries.push((id, v["batch"].clone())); }
                Err(_) => { invalid = true; break; }
            }
        }
        if invalid {
            self.inner.lock().unwrap().rejected_events += 1;
            let mids: Vec<String> = self.inner.lock().unwrap().channels.keys().cloned().collect();
            for m in mids { self.resync(&m, None, "SCHEMA", &received_at); }
            return;
        }
        for (mid, batch) in entries {
            if !self.inner.lock().unwrap().channels.contains_key(&mid) {
                self.inner.lock().unwrap().ignored_markets += 1;
                continue;
            }
            self.accept_event(&mid, "market_batch", &batch, &received_at);
        }
    }
    pub fn start(&self) -> Result<V, String> {
        if !self.allowed() { return Err(err("REALTIME_STOPPED")); }
        if self.inner.lock().unwrap().state == "refused" { return Err(err("REALTIME_JOIN_REFUSED")); }
        if self.inner.lock().unwrap().state == "subscribed" { return Ok(self.status()); }
        self.establish("INITIAL", true)
    }
    pub fn refresh(&self) -> Result<V, String> { self.establish("REFRESH", true) }
    pub fn reconnect(&self) -> Result<V, String> {
        if self.inner.lock().unwrap().state == "refused" { return Err(err("REALTIME_JOIN_REFUSED")); }
        self.establish("RECONNECT", false)
    }
    pub fn stop(&self) -> V {
        let mut i = self.inner.lock().unwrap();
        i.stopped = true;
        i.state = "stopped".into();
        i.credentials = None;
        for c in i.channels.values_mut() { c.joined = false; }
        drop(i);
        self.status()
    }
    /// authorizes(spec) — a token dispatch is authorized when the spec shape
    /// matches and gates pass.
    pub fn authorizes(&self, spec: &V) -> bool {
        spec["method"].as_str() == Some("POST") && spec["endpoint"].as_str() == Some("/realtime/token")
            && spec["body"].is_null() && spec["bodyJson"].is_null()
            && self.allowed() && self.token_allowed()
    }
}
/// install_event_sink — set before establish so channel callbacks route back
/// into dispatch_event (mirrors JS closure capture of the client instance).
pub fn install_event_sink(client: &RealtimeClient, sink: Arc<dyn Fn(&str, &str, &V, &str) + Send + Sync>) {
    client.inner.lock().unwrap().event_sink = Some(sink);
}
/// read_sink — SDK WS threads call this to find the current dispatch entry.
impl RealtimeClient {
    pub fn event_sink(&self) -> Option<Arc<dyn Fn(&str, &str, &V, &str) + Send + Sync>> {
        self.inner.lock().unwrap().event_sink.clone()
    }
}
