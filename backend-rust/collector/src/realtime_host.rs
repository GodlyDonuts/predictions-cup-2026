// live-paper/realtime-host.mjs — host binding RealtimeClient ↔ RealtimeBooks.
// Socket data is captured separately and never reaches the order controller.
use serde_json::{json, Value as V};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::realtime_books::RealtimeBooks;
use crate::realtime_client::{Dispatch, EventCb, RealtimeClient};

pub struct RealtimeHost<D: Fn(&V) -> Result<V, String> + Send + Sync> {
    dispatch: Arc<D>,
    client: Arc<RealtimeClient>,
    books: Arc<Mutex<RealtimeBooks<EventCb>>>,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
    can_mint_token: Arc<dyn Fn() -> bool + Send + Sync>,
    silent_recovery_interval_ms: i64,
    inner: Mutex<HostInner>,
    fences: Mutex<HashMap<usize, String>>,
}
struct HostInner {
    closed: bool,
    servicing: bool,
    cursor: usize,
    retry_at: i64,
    recovery_attempts: i64,
    recovery_successes: i64,
    shared_snapshot_attempts: i64,
    shared_snapshot_successes: i64,
    last_recovery_at: Option<String>,
    last_code: Option<String>,
    quiet_recovery_deferrals: i64,
    recovered: HashMap<String, (String, i64)>,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
}
impl<D: Fn(&V) -> Result<V, String> + Send + Sync + 'static> RealtimeHost<D> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(dispatch: Arc<D>, client: Arc<RealtimeClient>, books: RealtimeBooks<EventCb>,
        can_operate: Arc<dyn Fn() -> bool + Send + Sync>, can_mint_token: Arc<dyn Fn() -> bool + Send + Sync>,
        now: Arc<dyn Fn() -> i64 + Send + Sync>, silent_recovery_interval_ms: i64) -> Result<Self, String> {
        if silent_recovery_interval_ms < 60_000 || silent_recovery_interval_ms > 120_000 {
            return Err("REALTIME_SILENT_RECOVERY_POLICY".into());
        }
        Ok(Self { dispatch, client, books: Arc::new(Mutex::new(books)), can_operate, can_mint_token,
            silent_recovery_interval_ms, inner: Mutex::new(HostInner { closed: false, servicing: false,
                cursor: 0, retry_at: 0, recovery_attempts: 0, recovery_successes: 0,
                shared_snapshot_attempts: 0, shared_snapshot_successes: 0, last_recovery_at: None,
                last_code: None, quiet_recovery_deferrals: 0, recovered: HashMap::new(), now }),
            fences: Mutex::new(HashMap::new()) })
    }
    fn allowed(&self) -> bool { !self.inner.lock().unwrap().closed && (self.can_operate)() }
    fn token_allowed(&self) -> bool { (self.can_mint_token)() }
    fn now(&self) -> i64 { (self.inner.lock().unwrap().now)() }
    fn market_status(&self) -> Vec<V> {
        let now = self.now();
        let mut books = self.books.lock().unwrap();
        let markets = books.status().map(|s| s["markets"].as_array().cloned().unwrap_or_default()).unwrap_or_default();
        for market in &markets {
            let prior = self.inner.lock().unwrap().recovered.get(market["marketId"].as_str().unwrap_or("")).cloned();
            if market["ready"].as_bool().unwrap_or(false)
                && prior.as_ref().map(|(c, at)| *c == market["connectionId"].as_str().unwrap_or("") && now - at >= self.silent_recovery_interval_ms).unwrap_or(false) {
                let _ = books.invalidate(market["marketId"].as_str().unwrap_or(""),
                    market["connectionId"].as_str().unwrap_or(""), "PERIODIC");
            }
        }
        books.status().map(|s| s["markets"].as_array().cloned().unwrap_or_default()).unwrap_or_default()
    }
    /// authorizes(spec) — token dispatch via client or fenced shared snapshot GET.
    pub fn authorizes(&self, spec: &V, fence_id: Option<usize>) -> bool {
        if !self.allowed() { return false; }
        if self.client.authorizes(spec) { return true; }
        spec["method"].as_str() == Some("GET") && spec["body"].is_null() && spec["bodyJson"].is_null()
            && fence_id.is_some_and(|f| self.fences.lock().unwrap().get(&f) == spec["endpoint"].as_str().map(|s| &s.to_string()).as_ref())
    }
    pub fn status(&self) -> V {
        let mut inner = self.inner.lock().unwrap();
        let blocked = !inner.closed && self.allowed() && !self.token_allowed();
        let mut transport = self.client.status();
        if blocked { transport["state"] = json!("blocked"); transport["lastCode"] = json!("REALTIME_TOKEN_BUDGET_BLOCKED"); }
        let at = baal_service_runtime::contract::ms_to_iso(self.now() as f64);
        let events_received = transport["eventsReceived"].as_i64().unwrap_or(0);
        let retry_at = if blocked { V::Null } else if inner.retry_at > 0 {
            json!(baal_service_runtime::contract::ms_to_iso(inner.retry_at as f64))
        } else { V::Null };
        let markets = self.market_status().into_iter().map(|m| {
            let mut m = m.clone();
            let rows = m["books"].as_array().map(|a| a.len()).unwrap_or(0);
            m.as_object_mut().unwrap().remove("books");
            m["availableBooks"] = json!(rows);
            m
        }).collect::<Vec<_>>();
        let last_code = if blocked { Some("REALTIME_TOKEN_BUDGET_BLOCKED".to_string()) } else { inner.last_code.clone() };
        json!({"schemaVersion":1,"mode":"read_only_market_capture","executionAuthorized":false,
            "at": at, "transport": transport,
            "marketStreamObserved": events_received > 0,
            "dataEvidence": if blocked { "blocked_shared_read_only_budget" } else if events_received > 0 { "market_frames_observed" } else { "awaiting_first_market_frame" },
            "recoveryAttempts": inner.recovery_attempts, "recoverySuccesses": inner.recovery_successes,
            "silentRecoveryIntervalMs": self.silent_recovery_interval_ms,
            "quietRecoveryDeferrals": inner.quiet_recovery_deferrals,
            "sharedSnapshotAttempts": inner.shared_snapshot_attempts,
            "sharedSnapshotSuccesses": inner.shared_snapshot_successes,
            "lastRecoveryAt": inner.last_recovery_at, "lastCode": last_code,
            "retryAt": retry_at, "markets": markets})
    }
    /// service() — one recovery slot per collector cycle.
    pub fn service(&self) -> Result<bool, String> {
        if !self.allowed() { return Ok(false); }
        {
            let mut inner = self.inner.lock().unwrap();
            if inner.servicing { return Ok(false); }
            inner.servicing = true;
        }
        let result = self.service_inner();
        self.inner.lock().unwrap().servicing = false;
        result
    }
    fn service_inner(&self) -> Result<bool, String> {
        let now = self.now();
        if !self.token_allowed() {
            let mut inner = self.inner.lock().unwrap();
            inner.last_code = Some("REALTIME_TOKEN_BUDGET_BLOCKED".into());
            inner.retry_at = 0;
            let state = self.client.status()["state"].as_str().unwrap_or("").to_string();
            if !["idle","stopped","blocked"].contains(&state.as_str()) { self.client.stop(); }
            return Ok(false);
        }
        let state = self.client.status()["state"].as_str().unwrap_or("").to_string();
        if ["idle","failed","reconnecting","blocked"].contains(&state.as_str()) {
            if now < self.inner.lock().unwrap().retry_at { return Ok(false); }
            self.inner.lock().unwrap().retry_at = now + 60000;
            let res = if state == "idle" { self.client.start() } else { self.client.reconnect() };
            if res.is_err() {
                self.inner.lock().unwrap().last_code = Some("REALTIME_CONNECTION_UNAVAILABLE".into());
                return Ok(false);
            }
        }
        if !self.allowed() { return Ok(false); }
        let st = self.client.status();
        if st["state"].as_str() == Some("refused") {
            let mut inner = self.inner.lock().unwrap();
            inner.last_code = Some("REALTIME_JOIN_REFUSED".into());
            inner.retry_at = 0;
            return Ok(false);
        }
        if st["state"].as_str() != Some("subscribed") { return Ok(false); }
        let markets = self.market_status();
        for i in 0..markets.len() {
            let index = (self.inner.lock().unwrap().cursor + i) % markets.len();
            let market = &markets[index];
            if !market["needsSnapshot"].as_bool().unwrap_or(false)
                || market["reasons"].as_array().map(|r| r.iter().any(|x| x.as_str() == Some("SETTLEMENT"))).unwrap_or(false) {
                continue;
            }
            let prior = self.inner.lock().unwrap().recovered.get(market["marketId"].as_str().unwrap_or("")).cloned();
            let events_received = self.client.status()["eventsReceived"].as_i64().unwrap_or(0);
            let reasons: Vec<String> = market["reasons"].as_array().cloned().unwrap_or_default().iter()
                .filter_map(|r| r.as_str().map(String::from)).collect();
            if events_received == 0 && !reasons.is_empty() && reasons.iter().all(|r| r == "STALE")
                && prior.as_ref().map(|(c, at)| *c == market["connectionId"].as_str().unwrap_or("") && now - at < self.silent_recovery_interval_ms).unwrap_or(false) {
                self.inner.lock().unwrap().quiet_recovery_deferrals += 1;
                continue;
            }
            self.inner.lock().unwrap().cursor = (index + 1) % markets.len();
            let ticket = self.books.lock().unwrap().begin_snapshot(
                market["marketId"].as_str().unwrap_or(""), market["connectionId"].as_str().unwrap_or(""))?;
            let fence_id = self.fences.lock().unwrap().len() + 1;
            self.fences.lock().unwrap().insert(fence_id, ticket["endpoint"].as_str().unwrap_or("").to_string());
            self.inner.lock().unwrap().recovery_attempts += 1;
            let check_fence = |this: &Self, ticket: &V| -> Result<(), String> {
                if !this.allowed()
                    || this.books.lock().unwrap().view(ticket["marketId"].as_str().unwrap_or(""))
                        .map(|v| v["connectionId"] != ticket["connectionId"]).unwrap_or(true)
                    || !this.client.status()["markets"].as_array().cloned().unwrap_or_default().iter()
                        .any(|m| m["marketId"] == ticket["marketId"] && m["subscribed"].as_bool().unwrap_or(false)) {
                    return Err("REALTIME_RECOVERY_STOPPED".into());
                }
                Ok(())
            };
            let outcome = (|| -> Result<bool, String> {
                let response = (self.dispatch)(&json!({"method":"GET","endpoint":ticket["endpoint"],"beforeDispatch":fence_id,"beforeNetwork":fence_id}))?;
                check_fence(self, &ticket)?;
                let books_ok = self.books.lock().unwrap().complete_snapshot(&ticket, &response)?;
                Ok(books_ok)
            })();
            self.fences.lock().unwrap().remove(&fence_id);
            match outcome {
                Ok(true) => {
                    let mut inner = self.inner.lock().unwrap();
                    inner.recovery_successes += 1;
                    inner.last_recovery_at = Some(baal_service_runtime::contract::ms_to_iso(now as f64));
                    inner.last_code = None;
                    inner.recovered.insert(market["marketId"].as_str().unwrap_or("").to_string(),
                        (ticket["connectionId"].as_str().unwrap_or("").to_string(), now));
                }
                Ok(false) => { self.inner.lock().unwrap().last_code = Some("REALTIME_RECOVERY_REJECTED".into()); }
                Err(_) => {
                    let _ = self.books.lock().unwrap().snapshot_failed(&ticket);
                    self.inner.lock().unwrap().last_code = Some("REALTIME_RECOVERY_UNAVAILABLE".into());
                }
            }
            return Ok(true);
        }
        Ok(false)
    }
    /// beginSharedSnapshot(marketId)
    pub fn begin_shared_snapshot(&self, market_id: &str) -> Result<Option<V>, String> {
        if !self.allowed() || !self.token_allowed() || self.client.status()["state"].as_str() != Some("subscribed") {
            return Ok(None);
        }
        let connection = self.client.status()["markets"].as_array().cloned().unwrap_or_default().into_iter()
            .find(|m| m["marketId"].as_str() == Some(market_id));
        if connection.as_ref().map(|c| c["subscribed"].as_bool().unwrap_or(false)) != Some(true)
            || connection.as_ref().unwrap()["connectionId"].is_null() {
            return Ok(None);
        }
        let conn_id = connection.unwrap()["connectionId"].as_str().unwrap().to_string();
        let view = self.books.lock().unwrap().view(market_id)?;
        if view["reasons"].as_array().map(|r| r.iter().any(|x| x.as_str() == Some("SETTLEMENT"))).unwrap_or(false) {
            return Ok(None);
        }
        let ticket = self.books.lock().unwrap().begin_snapshot(market_id, &conn_id)?;
        let mut inner = self.inner.lock().unwrap();
        inner.shared_snapshot_attempts += 1;
        Ok(Some(ticket))
    }
    /// readSharedSnapshot(ticket)
    pub fn read_shared_snapshot(&self, ticket: &V, fence_id: usize) -> Result<V, String> {
        self.fences.lock().unwrap().insert(fence_id, ticket["endpoint"].as_str().unwrap_or("").to_string());
        let result = (|| -> Result<V, String> {
            if !self.allowed()
                || self.books.lock().unwrap().view(ticket["marketId"].as_str().unwrap_or(""))
                    .map(|v| v["connectionId"] != ticket["connectionId"]).unwrap_or(true)
                || !self.client.status()["markets"].as_array().cloned().unwrap_or_default().iter()
                    .any(|m| m["marketId"] == ticket["marketId"] && m["subscribed"].as_bool().unwrap_or(false)) {
                return Err("REALTIME_RECOVERY_STOPPED".into());
            }
            (self.dispatch)(&json!({"method":"GET","endpoint":ticket["endpoint"],"beforeDispatch":fence_id,"beforeNetwork":fence_id}))
        })();
        self.fences.lock().unwrap().remove(&fence_id);
        result
    }
    pub fn complete_shared_snapshot(&self, ticket: &V, response: &V) -> Result<bool, String> {
        if ticket.is_null() || !self.allowed() { return Ok(false); }
        let accepted = self.books.lock().unwrap().complete_snapshot(ticket, response)?;
        if accepted {
            let mut inner = self.inner.lock().unwrap();
            inner.shared_snapshot_successes += 1;
            inner.last_recovery_at = Some(response["receivedAt"].as_str().unwrap_or("").to_string());
            inner.recovered.insert(ticket["marketId"].as_str().unwrap_or("").to_string(),
                (ticket["connectionId"].as_str().unwrap_or("").to_string(), self.now()));
        }
        Ok(accepted)
    }
    pub fn fail_shared_snapshot(&self, ticket: &V) -> Result<bool, String> {
        if ticket.is_null() { return Ok(false); }
        self.books.lock().unwrap().snapshot_failed(ticket)
    }
    pub fn view(&self, market_id: &str) -> Option<V> {
        if !self.allowed() { return None; }
        self.books.lock().unwrap().view(market_id).ok()
    }
    pub fn stop(&self) {
        let mut inner = self.inner.lock().unwrap();
        if inner.closed { return; }
        inner.closed = true;
        drop(inner);
        self.client.stop();
        self.books.lock().unwrap().stop();
    }
}
