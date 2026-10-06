// live-paper/realtime-books.mjs — market-only in-memory realtime bridge.
use baal_feed::collector::MarketCollector;
use baal_service_runtime::contract::{compare_version, config, instant_ns, numeric_id, parse_event, parse_snapshot, snapshot_path, timestamp_str};
use baal_paper::fixed::big_price;
use serde_json::{json, Value as V};
use std::collections::{BTreeSet, HashMap};

const INVALIDATION_REASONS: &[&str] = &["INITIAL","RECONNECT","REFRESH","SOCKET_ERROR","DISCONNECTED","GAP","RESYNC_REQUIRED","UNSEQUENCED_TRADE","SCHEMA","STALE","PERIODIC","EXPIRY","MISSING_BOOK","SETTLEMENT","BOOK_DIRTY","VERSION_CONFLICT","RECOVERY_FAILED"];
fn require_that(ok: bool, code: &str) -> Result<(), String> { if ok { Ok(()) } else { Err(code.to_string()) } }

struct Entry {
    scope: V,
    scope_obj: baal_service_runtime::contract::Scope,
    collector: MarketCollector,
    snapshot_receipt: Option<String>,
    connection_id: Option<String>,
    connections: BTreeSet<String>,
    subscribed_ns: Option<i128>,
    last_receipt_ns: Option<i128>,
    ticket: Option<V>,
    known_expiry: Option<(V, String)>,
}
pub struct RealtimeBooks<F: Fn() -> f64> {
    markets: HashMap<String, Entry>,
    now: F,
    last_now: f64,
    stale_ms: f64,
    skew_ms: f64,
    snapshot_ms: f64,
}
impl<F: Fn() -> f64> RealtimeBooks<F> {
    pub fn new(tournament_id: &str, markets: &[V], channel_mode: &str, now: F,
        stale_ms: f64, max_clock_skew_ms: f64, max_snapshot_ms: f64) -> Result<Self, String> {
        crate::realtime_scope::realtime_scopes(tournament_id, markets, channel_mode).map_err(|e| e.0)?;
        for n in [stale_ms, max_snapshot_ms] {
            require_that(n > 0.0 && n <= 60_000.0 && n.fract() == 0.0, "INVALID_FRESHNESS_POLICY")?;
        }
        require_that(max_clock_skew_ms >= 0.0 && max_clock_skew_ms <= 1000.0 && max_clock_skew_ms.fract() == 0.0, "INVALID_CLOCK_POLICY")?;
        let mut map = HashMap::new();
        let mut exchange_ids = BTreeSet::new();
        for market in markets {
            let scope = config(&json!({"tournamentId": tournament_id, "marketId": market["marketId"],
                "exchangeIds": [market["exchangeId"]]})).map_err(|e| e.0)?;
            require_that(!map.contains_key(&scope.market_id) && !exchange_ids.contains(&scope.exchange_ids[0]), "DUPLICATE_MARKET_SCOPE")?;
            exchange_ids.insert(scope.exchange_ids[0].clone());
            let scope_v = json!({"tournamentId": scope.tournament_id, "marketId": scope.market_id,
                "exchangeIds": scope.exchange_ids, "topic": scope.topic});
            let collector = MarketCollector::new(&scope_v, stale_ms, std::rc::Rc::new(|| f64::NAN), None, 10000)?;
            map.insert(scope.market_id.clone(), Entry { scope: scope_v, scope_obj: scope.clone(), collector,
                snapshot_receipt: None, connection_id: None, connections: BTreeSet::new(),
                subscribed_ns: None, last_receipt_ns: None, ticket: None, known_expiry: None });
        }
        Ok(Self { markets: map, now, last_now: 0.0, stale_ms, skew_ms: max_clock_skew_ms, snapshot_ms: max_snapshot_ms })
    }
    fn clock(&mut self) -> Result<f64, String> {
        let now = (self.now)();
        if now.is_nan() || now.fract() != 0.0 || now < self.last_now {
            for entry in self.markets.values_mut() { entry.collector.schema_clock_failure(); }
            return require_that(false, "CLOCK_ROLLBACK").map(|_| 0.0);
        }
        self.last_now = now;
        Ok(now)
    }
    fn entry(&self, market_id: &str) -> Result<&Entry, String> {
        self.markets.get(&numeric_id(&json!(market_id)).map_err(|e| e.0)?).ok_or_else(|| "UNKNOWN_MARKET".to_string())
    }
    fn entry_mut(&mut self, market_id: &str) -> Result<&mut Entry, String> {
        self.markets.get_mut(&numeric_id(&json!(market_id)).map_err(|e| e.0)?).ok_or_else(|| "UNKNOWN_MARKET".to_string())
    }
    fn connection(id: &str) -> Result<String, String> {
        require_that(!id.is_empty() && id.len() <= 80 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'), "INVALID_CONNECTION_ID")?;
        Ok(id.to_string())
    }
    fn expire(&mut self, market_id: &str) -> Result<(), String> {
        let now = self.clock()?;
        let entry = self.entry_mut(market_id)?;
        if let Some((_, at)) = &entry.known_expiry {
            if instant_ns(at).unwrap_or(i128::MAX) <= now as i128 * 1_000_000 && entry.collector.state == "ready" {
                let _ = entry.collector.invalidate("EXPIRY");
            }
        }
        Ok(())
    }
    fn receipt(&mut self, value: &str, market_id: &str, advance: bool) -> Result<i128, String> {
        let now = self.clock()? as i128 * 1_000_000;
        let ns = instant_ns(value).map_err(|e| e.0)?;
        let (skew, stale) = (self.skew_ms, self.stale_ms);
        let entry = self.entry_mut(market_id)?;
        require_that(ns <= now + (skew as i128) * 1_000_000 && now - ns < (stale as i128) * 1_000_000, "INVALID_RECEIPT_CLOCK")?;
        require_that(entry.last_receipt_ns.is_none_or(|x| ns >= x), "RECEIPT_ROLLBACK")?;
        if advance { entry.last_receipt_ns = Some(ns); }
        Ok(ns)
    }
    fn books(&self, books: &[V], received_ns: i128) -> Result<(), String> {
        for book in books {
            require_that(!book["asOf"].is_null()
                && instant_ns(book["asOf"]["at"].as_str().unwrap_or("")).map_err(|e| e.0)? <= received_ns + (self.skew_ms as i128) * 1_000_000, "INVALID_BOOK_CLOCK")?;
            for levels in [book["bids"].clone(), book["asks"].clone()] {
                for level in levels.as_array().cloned().unwrap_or_default() {
                    require_that(level["quantity"].as_i64().is_some_and(|q| q > 0), "INVALID_INTEGER_DEPTH")?;
                    let _ = big_price(&level["price"].to_string().trim_matches('"').to_string())
                        .or_else(|_| big_price(level["price"].as_str().unwrap_or("")))?;
                }
            }
            let bids = book["bids"].as_array().cloned().unwrap_or_default();
            let asks = book["asks"].as_array().cloned().unwrap_or_default();
            require_that(bids.is_empty() || asks.is_empty()
                || bids[0]["price"].as_f64().unwrap_or(f64::NAN) < asks[0]["price"].as_f64().unwrap_or(f64::NAN)
                || bids[0]["price"].as_str().and_then(|b| asks[0]["price"].as_str().map(|a| b < a)).unwrap_or(false), "CROSSED_BOOK")?;
        }
        Ok(())
    }
    pub fn subscribed(&mut self, market_id: &str, connection_id: &str, reason: &str, received_at: Option<String>) -> Result<V, String> {
        let entry_exists = { let e = self.entry(market_id)?; e.connection_id.as_deref() == Some(connection_id)
            && !["disabled", "disconnected", "stopped"].contains(&e.collector.state.as_str()) };
        if entry_exists { return self.view(market_id); }
        Self::connection(connection_id)?;
        {
            let entry = self.entry(market_id)?;
            require_that(!entry.connections.contains(connection_id) && entry.connections.len() < 10000, "REUSED_CONNECTION_ID")?;
            require_that(["INITIAL", "RECONNECT", "REFRESH"].contains(&reason), "INVALID_SUBSCRIPTION_REASON")?;
        }
        let received = received_at.unwrap_or_else(|| baal_service_runtime::contract::ms_to_iso(self.last_now));
        let ns = self.receipt(&received, market_id, true)?;
        {
            let entry = self.entry_mut(market_id)?;
            entry.connections.insert(connection_id.to_string());
            entry.connection_id = Some(connection_id.to_string());
            entry.subscribed_ns = Some(ns);
            entry.ticket = None;
            entry.collector.subscribed(reason)?;
        }
        self.view(market_id)
    }
    pub fn disconnected(&mut self, market_id: &str, connection_id: &str, reason: &str) -> Result<bool, String> {
        let entry = self.entry_mut(market_id)?;
        if Some(connection_id.to_string()) != entry.connection_id { return Ok(false); }
        entry.ticket = None;
        entry.collector.disconnected(if ["REFRESH", "SOCKET_ERROR", "RECOVERY_FAILED"].contains(&reason) { reason } else { "DISCONNECTED" })?;
        Ok(true)
    }
    pub fn invalidate(&mut self, market_id: &str, connection_id: &str, reason: &str) -> Result<bool, String> {
        let entry = self.entry_mut(market_id)?;
        require_that(INVALIDATION_REASONS.contains(&reason), "INVALID_INVALIDATION_REASON")?;
        if Some(connection_id.to_string()) != entry.connection_id { return Ok(false); }
        entry.ticket = None;
        entry.collector.invalidate(reason)?;
        Ok(true)
    }
    pub fn ingest(&mut self, market_id: &str, connection_id: &str, event: &str, payload: &V,
        received_at: Option<String>, topic: Option<&str>) -> Result<bool, String> {
        let mid = numeric_id(&json!(market_id)).map_err(|e| e.0)?;
        {
            let entry = self.entry(&mid)?;
            if Some(connection_id.to_string()) != entry.connection_id
                || ["disabled", "disconnected", "stopped"].contains(&entry.collector.state.as_str()) {
                return Ok(false);
            }
        }
        let result = (|| -> Result<bool, String> {
            self.expire(&mid)?;
            {
                let entry = self.entry(&mid)?;
                require_that(topic.is_none() || topic == Some(&entry.scope_obj.topic), "CONTEXT_MISMATCH")?;
            }
            let received = received_at.unwrap_or_else(|| baal_service_runtime::contract::ms_to_iso(self.last_now));
            let received_ns = self.receipt(&received, &mid, false)?;
            let scope = self.entry(&mid)?.scope_obj.clone();
            let parsed = parse_event(event, payload, &scope).map_err(|e| e.0)?;
            if event == "market_batch" {
                self.books(parsed["payload"]["books"].as_array().cloned().unwrap_or_default().as_slice(), received_ns)?;
                for trade in parsed["payload"]["trades"].as_array().cloned().unwrap_or_default() {
                    require_that(instant_ns(trade["executedAt"].as_str().unwrap_or("")).map_err(|e| e.0)? <= received_ns + (self.skew_ms as i128) * 1_000_000, "INVALID_TRADE_CLOCK")?;
                }
                for hint in parsed["payload"]["bookDirty"].as_array().cloned().unwrap_or_default().iter()
                    .chain(parsed["payload"]["marketSettled"].as_array().cloned().unwrap_or_default().iter()) {
                    require_that(instant_ns(hint["at"].as_str().unwrap_or("")).map_err(|e| e.0)? <= received_ns + (self.skew_ms as i128) * 1_000_000, "INVALID_HINT_CLOCK")?;
                }
            } else {
                require_that(instant_ns(parsed["payload"]["at"].as_str().unwrap_or("")).map_err(|e| e.0)? <= received_ns + (self.skew_ms as i128) * 1_000_000, "INVALID_HINT_CLOCK")?;
            }
            let epoch = self.entry(&mid)?.collector.epoch;
            let accepted = {
                let entry = self.entry_mut(&mid)?;
                entry.last_receipt_ns = Some(received_ns);
                entry.collector.ingest(event, payload, &timestamp_str(&received).unwrap_or(received.clone()), epoch)
            };
            if event == "market_batch" {
                for book in parsed["payload"]["books"].as_array().cloned().unwrap_or_default() {
                    let entry = self.entry_mut(&mid)?;
                    let held = entry.collector.held_book(book["exchangeId"].as_str().unwrap_or("")).cloned();
                    if let Some(held) = held {
                        if compare_version(&book["asOf"], &held["asOf"]) == 0
                            && entry.known_expiry.as_ref().is_none_or(|(v, _)| compare_version(&book["asOf"], v) > 0) {
                            entry.known_expiry = Some((book["asOf"].clone(), book["nextExpiryAt"].as_str().unwrap_or("").to_string()));
                        }
                    }
                }
            }
            Ok(accepted)
        })();
        match result {
            Ok(v) => Ok(v),
            Err(_) => {
                let entry = self.entry_mut(&mid)?;
                entry.collector.metrics.schema_errors += 1;
                let _ = entry.collector.invalidate("SCHEMA");
                Ok(false)
            }
        }
    }
    pub fn begin_snapshot(&mut self, market_id: &str, connection_id: &str) -> Result<V, String> {
        let entry = self.entry_mut(market_id)?;
        require_that(Some(connection_id.to_string()) == entry.connection_id, "STALE_CONNECTION")?;
        let recovery = entry.collector.begin_recovery()?;
        let ticket = json!({"marketId": entry.scope["marketId"], "connectionId": connection_id, "method": "GET",
            "endpoint": snapshot_path(&entry.scope_obj), "epoch": recovery.epoch, "generation": recovery.generation,
            "startedMs": recovery.started_ms, "bookOrdinal": recovery.book_ordinal});
        entry.ticket = Some(ticket.clone());
        Ok(ticket)
    }
    pub fn complete_snapshot(&mut self, ticket: &V, response: &V) -> Result<bool, String> {
        let mid = numeric_id(&json!(ticket["marketId"].as_str().unwrap_or(""))).map_err(|e| e.0)?;
        {
            let entry = self.entry(&mid)?;
            if entry.ticket.as_ref() != Some(ticket) || ticket["connectionId"].as_str() != entry.connection_id.as_deref() {
                return Ok(false);
            }
        }
        let result = (|| -> Result<bool, String> {
            {
                let entry = self.entry_mut(&mid)?;
                entry.ticket = None;
            }
            require_that(response["endpoint"].as_str() == ticket["endpoint"].as_str()
                && (response["status"].is_null() || response["status"].as_i64() == Some(200)), "SNAPSHOT_SCOPE_MISMATCH")?;
            let received_ns = self.receipt(response["receivedAt"].as_str().unwrap_or(""), &mid, false)?;
            let request_ns = instant_ns(response["requestStartedAt"].as_str().unwrap_or("")).map_err(|e| e.0)?;
            let entry = self.entry(&mid)?;
            require_that(request_ns >= entry.subscribed_ns.unwrap_or(i128::MAX)
                && request_ns >= (ticket["startedMs"].as_f64().unwrap_or(0.0) as i128) * 1_000_000
                && request_ns <= received_ns
                && received_ns - request_ns <= (self.snapshot_ms as i128) * 1_000_000, "INVALID_SNAPSHOT_CLOCK")?;
            let books = parse_snapshot(&response["data"], &entry.scope_obj).map_err(|e| e.0)?;
            self.books(&books, received_ns)?;
            let raw = response["data"]["contexts"].as_array().cloned().unwrap_or_default().iter()
                .find(|c| c["type"].as_str() == Some("tournament") && c["tournament"]["id"].as_str() == Some(&entry.scope_obj.tournament_id))
                .cloned().unwrap_or(V::Null);
            require_that(raw["orderbook"]["exchanges"].as_array().cloned().unwrap_or_default().iter()
                .all(|b| b["option"].as_str() == Some("YES")), "SNAPSHOT_OPTION_MISMATCH")?;
            let epoch = { let e = self.entry_mut(&mid)?; e.last_receipt_ns = Some(received_ns); e.collector.epoch };
            let recovered = {
                let entry = self.entry_mut(&mid)?;
                entry.snapshot_receipt = Some(response["requestStartedAt"].as_str().unwrap_or("").to_string());
                let r = entry.collector.complete_recovery(
                    &baal_feed::collector::RecoveryTicket { epoch: ticket["epoch"].as_i64().unwrap_or(0),
                        generation: ticket["generation"].as_i64().unwrap_or(0),
                        started_ms: ticket["startedMs"].as_f64().unwrap_or(0.0),
                        book_ordinal: ticket["bookOrdinal"].as_i64().unwrap_or(0) },
                    &response["data"], response["receivedAt"].as_str().unwrap_or(""));
                entry.snapshot_receipt = None;
                let _ = epoch;
                r
            };
            let entry = self.entry_mut(&mid)?;
            if recovered {
                if let Some((version, _)) = entry.known_expiry.clone() {
                    let held = entry.collector.held_book(entry.scope["exchangeIds"][0].as_str().unwrap_or("")).cloned();
                    if held.as_ref().is_some_and(|h| compare_version(&h["asOf"], &version) > 0) {
                        entry.known_expiry = None;
                    }
                }
            }
            Ok(recovered && self.view(&mid)?["ready"].as_bool().unwrap_or(false))
        })();
        match result {
            Ok(v) => Ok(v),
            Err(_) => {
                let entry = self.entry_mut(&mid)?;
                entry.collector.metrics.schema_errors += 1;
                let _ = entry.collector.invalidate("RECOVERY_FAILED");
                Ok(false)
            }
        }
    }
    pub fn snapshot_failed(&mut self, ticket: &V) -> Result<bool, String> {
        let mid = numeric_id(&json!(ticket["marketId"].as_str().unwrap_or(""))).map_err(|e| e.0)?;
        let entry = self.entry_mut(&mid)?;
        if entry.ticket.as_ref() != Some(ticket) || ticket["connectionId"].as_str() != entry.connection_id.as_deref() {
            return Ok(false);
        }
        entry.ticket = None;
        entry.collector.invalidate("RECOVERY_FAILED")?;
        Ok(true)
    }
    pub fn view(&mut self, market_id: &str) -> Result<V, String> {
        self.expire(market_id)?;
        let entry = self.entry_mut(market_id)?;
        let status = entry.collector.status();
        let ready = status["coverage"]["booksReady"].as_bool().unwrap_or(false);
        let books: Vec<V> = if ready {
            entry.collector.held_books_view().iter().map(|(book, observed_ms, source)| {
                let mut b = book.clone();
                b["receivedAt"] = json!(baal_service_runtime::contract::ms_to_iso(*observed_ms));
                b["source"] = json!(source);
                b
            }).collect()
        } else { vec![] };
        Ok(json!({"mode": "read_only_market_capture", "executionAuthorized": false,
            "marketId": entry.scope["marketId"], "exchangeId": entry.scope["exchangeIds"][0],
            "connectionId": entry.connection_id, "topic": entry.scope["topic"], "ready": ready,
            "needsSnapshot": !entry.collector.needs_market_state && ["needs_resync", "resyncing"].contains(&status["state"].as_str().unwrap_or("")),
            "state": status["state"], "epoch": status["epoch"], "revision": status["revision"],
            "reasons": status["reasons"],
            "nextKnownExpiryAt": entry.known_expiry.as_ref().map(|(_, at)| json!(at)).unwrap_or(V::Null),
            "books": books, "metrics": status["metrics"]}))
    }
    pub fn status(&mut self) -> Result<V, String> {
        let ids: Vec<String> = self.markets.keys().cloned().collect();
        let mut markets = Vec::new();
        for id in ids { markets.push(self.view(&id)?); }
        Ok(json!({"mode": "read_only_market_capture", "executionAuthorized": false, "markets": markets}))
    }
    pub fn needs_recovery(&mut self) -> Result<Vec<V>, String> {
        let status = self.status()?;
        Ok(status["markets"].as_array().cloned().unwrap_or_default().into_iter()
            .filter(|m| m["needsSnapshot"].as_bool().unwrap_or(false)).collect())
    }
    pub fn stop(&mut self) {
        for entry in self.markets.values_mut() {
            entry.ticket = None;
            entry.collector.stop();
        }
    }
}
