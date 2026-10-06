// feed/collector.mjs
use crate::capture::Capture;
use crate::{js_str, ms_to_iso};
use baal_service_runtime::contract::{compare_version, config, parse_event, parse_snapshot, timestamp_str, Scope};
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Map, Value as V};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;

const REASONS: &[&str] = &[
    "INITIAL", "RECONNECT", "REFRESH", "SOCKET_ERROR", "DISCONNECTED", "GAP", "RESYNC_REQUIRED",
    "UNSEQUENCED_TRADE", "SCHEMA", "STALE", "PERIODIC", "EXPIRY", "MISSING_BOOK", "SETTLEMENT",
    "BOOK_DIRTY", "VERSION_CONFLICT", "RECOVERY_FAILED",
];

#[derive(Clone)]
struct HeldBook {
    book: V,
    observed_ms: f64,
    #[allow(dead_code)]
    source: String,
    ordinal: i64,
}

#[derive(Clone, Default)]
pub struct Stat {
    count: i64,
    sum_ms: f64,
    max_ms: Option<f64>,
}

#[derive(Clone, Default)]
pub struct Metrics {
    pub events: i64,
    pub accepted_batches: i64,
    pub duplicate_batches: i64,
    pub out_of_order_batches: i64,
    pub gaps: i64,
    pub schema_errors: i64,
    pub books_applied: i64,
    pub older_books: i64,
    pub trades: i64,
    pub duplicate_trades: i64,
    pub trade_id_evictions: i64,
    pub reconnects: i64,
    pub recoveries: i64,
    pub recovery_discards: i64,
    pub clock_skew_samples: i64,
    pub event_age: Stat,
    pub recovery_latency: Stat,
}

#[derive(Clone)]
pub struct RecoveryTicket {
    pub epoch: i64,
    pub generation: i64,
    pub started_ms: f64,
    pub book_ordinal: i64,
}

pub struct MarketCollector {
    pub scope: Scope,
    pub stale_ms: f64,
    pub now: Rc<dyn Fn() -> f64>,
    pub capture: Option<Rc<RefCell<Capture>>>,
    pub max_trade_ids: usize,
    pub state: String,
    pub epoch: i64,
    pub generation: i64,
    pub revision: Option<i64>,
    pub book_ordinal: i64,
    pub needs_market_state: bool,
    books: HashMap<String, HeldBook>,
    seen_trades: indexmap::IndexMap<String, i64>,
    reasons: BTreeSet<String>,
    pub tape_complete: bool,
    pub metrics: Metrics,
}

fn require_that(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.to_string()) }
}

impl MarketCollector {
    pub fn new(
        scope: &V,
        stale_ms: f64,
        now: Rc<dyn Fn() -> f64>,
        capture: Option<Rc<RefCell<Capture>>>,
        max_trade_ids: usize,
    ) -> Result<MarketCollector, String> {
        let scope = config(scope).map_err(|e| e.to_string())?;
        require_that(stale_ms.is_finite() && stale_ms > 0.0, "INVALID_SCHEMA")?;
        require_that(max_trade_ids > 0, "INVALID_SCHEMA")?;
        let mut c = MarketCollector {
            scope,
            stale_ms,
            now,
            capture,
            max_trade_ids,
            state: "disabled".into(),
            epoch: 0,
            generation: 0,
            revision: None,
            book_ordinal: 0,
            needs_market_state: false,
            books: HashMap::new(),
            seen_trades: indexmap::IndexMap::new(),
            reasons: BTreeSet::new(),
            tape_complete: false,
            metrics: Metrics::default(),
        };
        c.record(
            "config",
            &json!({"scope": {"tournamentId": c.scope.tournament_id, "marketId": c.scope.market_id, "exchangeIds": c.scope.exchange_ids, "topic": c.scope.topic}, "staleMs": stale_ms}),
        );
        Ok(c)
    }

    fn now_ms(&self) -> f64 {
        (self.now)()
    }

    pub fn record(&mut self, kind: &str, data: &V) {
        let Some(cap) = &self.capture else { return };
        let mut row = Map::new();
        row.insert("kind".into(), json!(kind));
        row.insert("observedAt".into(), json!(ms_to_iso(self.now_ms())));
        row.insert("topic".into(), json!(self.scope.topic));
        row.insert("epoch".into(), json!(self.epoch));
        if let V::Object(o) = data {
            for (k, v) in o {
                row.insert(k.clone(), v.clone());
            }
        }
        let _ = cap.borrow_mut().append(&V::Object(row));
    }

    fn sample(&mut self, metric: &str, value: f64) {
        if value < 0.0 {
            self.metrics.clock_skew_samples += 1;
            return;
        }
        let m = if metric == "eventAge" { &mut self.metrics.event_age } else { &mut self.metrics.recovery_latency };
        m.count += 1;
        m.sum_ms += value;
        m.max_ms = Some(m.max_ms.unwrap_or(0.0).max(value));
    }

    pub fn invalidate(&mut self, reason: &str) -> Result<(), String> {
        require_that(REASONS.contains(&reason), "INVALID_REASON")?;
        if self.state == "stopped" || self.state == "disabled" {
            return Ok(());
        }
        if reason == "SETTLEMENT" {
            self.needs_market_state = true;
        }
        self.generation += 1;
        self.reasons.insert(reason.to_string());
        if self.state != "disconnected" {
            self.state = "needs_resync".into();
        }
        self.record("invalidation", &json!({"reason": reason, "generation": self.generation}));
        Ok(())
    }

    pub fn subscribed(&mut self, reason: &str) -> Result<bool, String> {
        require_that(["INITIAL", "RECONNECT", "REFRESH"].contains(&reason), "INVALID_REASON")?;
        if self.state == "stopped" {
            return Ok(false);
        }
        self.epoch += 1;
        self.revision = None;
        self.state = "needs_resync".into();
        if reason != "INITIAL" {
            self.metrics.reconnects += 1;
        }
        self.record("subscribed", &json!({"reason": reason}));
        self.invalidate(reason)?;
        Ok(true)
    }

    pub fn disconnected(&mut self, reason: &str) -> Result<(), String> {
        if self.state == "stopped" || self.state == "disabled" {
            return Ok(());
        }
        self.invalidate(reason)?;
        self.state = "disconnected".into();
        self.record("disconnected", &json!({"reason": reason}));
        Ok(())
    }

    pub fn stop(&mut self) {
        self.state = "stopped".into();
        self.generation += 1;
        self.record("stopped", &json!({}));
    }

    fn apply_book(&mut self, book: &V, observed_ms: f64, source: &str) -> bool {
        let exchange_id = js_str(&book["exchangeId"]).unwrap_or_default();
        let old = self.books.get(&exchange_id);
        let cmp = match old {
            Some(h) => compare_version(&book["asOf"], &h.book["asOf"]),
            None => 1,
        };
        if old.is_some()
            && cmp == 0
            && js_stringify(&json!([book["bids"], book["asks"]]))
                != js_stringify(&json!([old.unwrap().book["bids"], old.unwrap().book["asks"]]))
        {
            let _ = self.invalidate("VERSION_CONFLICT");
            return false;
        }
        if cmp < 0 {
            self.metrics.older_books += 1;
            return false;
        }
        if cmp > 0 || old.is_none() {
            self.book_ordinal += 1;
            self.books.insert(
                exchange_id,
                HeldBook { book: book.clone(), observed_ms, source: source.into(), ordinal: self.book_ordinal },
            );
            self.metrics.books_applied += 1;
        } else if source == "rest" {
            self.book_ordinal += 1;
            self.books.insert(
                exchange_id,
                HeldBook { book: book.clone(), observed_ms, source: source.into(), ordinal: self.book_ordinal },
            );
        }
        true
    }

    pub fn ingest(&mut self, event: &str, raw: &V, received_at: &str, epoch: i64) -> bool {
        if ["stopped", "disabled", "disconnected"].contains(&self.state.as_str()) || epoch != self.epoch {
            return false;
        }
        self.tick();
        self.metrics.events += 1;
        let parsed = match parse_event(event, raw, &self.scope) {
            Ok(p) => p,
            Err(_) => {
                self.metrics.schema_errors += 1;
                let _ = self.invalidate("SCHEMA");
                self.record("rejected", &json!({"code": "INVALID_SCHEMA"}));
                return false;
            }
        };
        let received_ms = match timestamp_str(received_at) {
            Ok(t) => match baal_service_runtime::contract::parse_iso_ms(&t) {
                Some(ms) => ms,
                None => {
                    self.metrics.schema_errors += 1;
                    let _ = self.invalidate("SCHEMA");
                    self.record("rejected", &json!({"code": "INVALID_SCHEMA"}));
                    return false;
                }
            },
            Err(_) => {
                self.metrics.schema_errors += 1;
                let _ = self.invalidate("SCHEMA");
                self.record("rejected", &json!({"code": "INVALID_SCHEMA"}));
                return false;
            }
        };
        if received_ms > self.now_ms() + 1000.0 {
            self.metrics.schema_errors += 1;
            let _ = self.invalidate("SCHEMA");
            self.record("rejected", &json!({"code": "INVALID_SCHEMA"}));
            return false;
        }
        let mut evt = parsed["payload"].clone();
        if let V::Object(ref mut o) = evt {
            o.insert("receivedAt".into(), json!(ms_to_iso(received_ms)));
        }
        let mut rec = Map::new();
        if let V::Object(o) = &evt {
            for (k, v) in o {
                rec.insert(k.clone(), v.clone());
            }
        }
        rec.insert("event".into(), json!(event));
        self.record("event", &V::Object(rec));
        let p = parsed["payload"].clone();
        if event != "market_batch" {
            let _ = self.invalidate(if event == "book_dirty" { "BOOK_DIRTY" } else { "SETTLEMENT" });
            return true;
        }
        let books = p["books"].as_array().cloned().unwrap_or_default();
        for b in &books {
            let at = b["asOf"]["at"].as_str().unwrap_or("");
            let b_ms = baal_service_runtime::contract::parse_iso_ms(at).unwrap_or(0.0);
            self.apply_book(b, received_ms, "websocket");
            self.sample("eventAge", received_ms - b_ms);
        }
        self.tick();
        let previous = self.revision;
        let revision = p["delivery"]["revision"].as_i64().unwrap_or(0);
        if p["resyncRequired"].as_bool().unwrap_or(false) {
            self.revision = Some(self.revision.unwrap_or(0).max(revision));
            let _ = self.invalidate("RESYNC_REQUIRED");
            return true;
        }
        if let Some(prev) = previous {
            if revision <= prev {
                self.metrics.duplicate_batches += 1;
                if revision < prev {
                    self.metrics.out_of_order_batches += 1;
                }
                return true;
            }
        }
        self.revision = Some(revision);
        self.metrics.accepted_batches += 1;
        if let Some(prev) = previous {
            if p["delivery"]["previousRevision"].as_i64().unwrap_or(0) > prev {
                self.metrics.gaps += 1;
                self.tape_complete = false;
                let _ = self.invalidate("GAP");
            }
        }
        let mut named: HashSet<String> = HashSet::new();
        for x in p["trades"]
            .as_array()
            .into_iter()
            .flat_map(|a| a.iter())
            .chain(p["bookDirty"].as_array().into_iter().flat_map(|a| a.iter()))
        {
            if let Some(id) = x["exchangeId"].as_str() {
                named.insert(id.to_string());
            }
        }
        if named
            .iter()
            .any(|id| !books.iter().any(|b| b["exchangeId"].as_str() == Some(id)))
        {
            let _ = self.invalidate("MISSING_BOOK");
        }
        if p["marketSettled"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
            let _ = self.invalidate("SETTLEMENT");
        }
        let trades = p["trades"].as_array().cloned().unwrap_or_default();
        if trades.iter().any(|t| t["sequence"].is_null()) {
            let _ = self.invalidate("UNSEQUENCED_TRADE");
        }
        for t in &trades {
            let at = t["executedAt"].as_str().unwrap_or("");
            let t_ms = baal_service_runtime::contract::parse_iso_ms(at).unwrap_or(0.0);
            self.sample("eventAge", received_ms - t_ms);
            if t["sequence"].is_null() {
                continue;
            }
            let id = t["id"].as_str().unwrap_or("").to_string();
            if self.seen_trades.contains_key(&id) {
                self.metrics.duplicate_trades += 1;
                continue;
            }
            self.seen_trades.insert(id.clone(), t["sequence"].as_i64().unwrap_or(0));
            self.metrics.trades += 1;
            if self.seen_trades.len() > self.max_trade_ids {
                self.seen_trades.shift_remove_index(0);
                self.metrics.trade_id_evictions += 1;
            }
            self.record(
                "trade_observed",
                &json!({"trade": t, "continuousAtReceipt": self.state == "ready"}),
            );
        }
        true
    }

    pub fn begin_recovery(&mut self) -> Result<RecoveryTicket, String> {
        if ["disabled", "stopped", "disconnected"].contains(&self.state.as_str()) {
            return Err("NOT_SUBSCRIBED".into());
        }
        self.state = "resyncing".into();
        self.record("recovery_started", &json!({"generation": self.generation}));
        Ok(RecoveryTicket {
            epoch: self.epoch,
            generation: self.generation,
            started_ms: self.now_ms(),
            book_ordinal: self.book_ordinal,
        })
    }

    pub fn complete_recovery(&mut self, ticket: &RecoveryTicket, raw: &V, received_at: &str) -> bool {
        if ticket.epoch != self.epoch || ticket.generation != self.generation || self.state != "resyncing" {
            self.metrics.recovery_discards += 1;
            return false;
        }
        let (books, observed_ms) = match parse_snapshot(raw, &self.scope) {
            Ok(b) => match timestamp_str(received_at)
                .ok()
                .and_then(|t| baal_service_runtime::contract::parse_iso_ms(&t))
            {
                Some(ms) if ms >= ticket.started_ms && ms <= self.now_ms() + 1000.0 => (b, ms),
                _ => {
                    self.metrics.schema_errors += 1;
                    let _ = self.invalidate("SCHEMA");
                    return false;
                }
            },
            Err(_) => {
                self.metrics.schema_errors += 1;
                let _ = self.invalidate("SCHEMA");
                return false;
            }
        };
        self.record("snapshot", &json!({"receivedAt": ms_to_iso(observed_ms), "books": books}));
        if books.iter().any(|b| b["asOf"].is_null()) {
            let _ = self.invalidate("RECOVERY_FAILED");
            return false;
        }
        for b in &books {
            let exchange_id = js_str(&b["exchangeId"]).unwrap_or_default();
            if let Some(held) = self.books.get(&exchange_id) {
                if compare_version(&b["asOf"], &held.book["asOf"]) < 0 && held.ordinal <= ticket.book_ordinal {
                    let _ = self.invalidate("RECOVERY_FAILED");
                    return false;
                }
            }
            self.apply_book(b, observed_ms, "rest");
        }
        if ticket.generation != self.generation {
            return false;
        }
        if self.needs_market_state {
            let _ = self.invalidate("SETTLEMENT");
            return false;
        }
        if self.books.values().any(|x| x.book["asOf"].is_null()) {
            let _ = self.invalidate("RECOVERY_FAILED");
            return false;
        }
        self.reasons.clear();
        self.state = "ready".into();
        self.metrics.recoveries += 1;
        self.sample("recoveryLatency", observed_ms - ticket.started_ms);
        self.tick();
        self.record(
            "recovery_finished",
            &json!({"booksReady": self.state == "ready", "generation": self.generation}),
        );
        self.state == "ready"
    }

    pub fn tick(&mut self) {
        if self.state != "ready" {
            return;
        }
        let now = self.now_ms();
        if self.books.values().any(|x| now - x.observed_ms >= self.stale_ms) {
            let _ = self.invalidate("STALE");
        }
        if self.books.values().any(|x| {
            !x.book["nextExpiryAt"].is_null()
                && baal_service_runtime::contract::parse_iso_ms(x.book["nextExpiryAt"].as_str().unwrap_or(""))
                    .map(|e| e <= now)
                    .unwrap_or(false)
        }) {
            let _ = self.invalidate("EXPIRY");
        }
    }

    pub fn status(&mut self) -> V {
        self.tick();
        let mut metrics = serde_json::to_value(&MetricsJson::from(&self.metrics)).unwrap_or(V::Null);
        for name in ["eventAge", "recoveryLatency"] {
            let (count, sum) = if name == "eventAge" {
                (self.metrics.event_age.count, self.metrics.event_age.sum_ms)
            } else {
                (self.metrics.recovery_latency.count, self.metrics.recovery_latency.sum_ms)
            };
            let mean = if count != 0 { json!(sum / count as f64) } else { V::Null };
            metrics[name]["meanMs"] = mean;
        }
        json!({
            "schemaVersion": 1,
            "mode": "read_only_scaffold",
            "state": self.state,
            "topic": self.scope.topic,
            "epoch": self.epoch,
            "revision": self.revision,
            "reasons": self.reasons.iter().cloned().collect::<Vec<_>>(),
            "coverage": {
                "configuredExchanges": self.scope.exchange_ids.len(),
                "observedExchanges": self.books.len(),
                "versionedExchanges": self.books.values().filter(|x| !x.book["asOf"].is_null()).count(),
                "booksReady": self.state == "ready",
                "tradeTapeComplete": false,
                "universeComplete": false,
            },
            "metrics": metrics,
        })
    }
}

#[derive(serde::Serialize)]
#[allow(non_snake_case)]
struct StatJson {
    count: i64,
    sumMs: f64,
    maxMs: Option<f64>,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MetricsJson {
    events: i64,
    accepted_batches: i64,
    duplicate_batches: i64,
    out_of_order_batches: i64,
    gaps: i64,
    schema_errors: i64,
    books_applied: i64,
    older_books: i64,
    trades: i64,
    duplicate_trades: i64,
    trade_id_evictions: i64,
    reconnects: i64,
    recoveries: i64,
    recovery_discards: i64,
    clock_skew_samples: i64,
    #[serde(rename = "eventAge")]
    event_age: StatJson,
    #[serde(rename = "recoveryLatency")]
    recovery_latency: StatJson,
}
impl From<&Metrics> for MetricsJson {
    fn from(m: &Metrics) -> Self {
        MetricsJson {
            events: m.events,
            accepted_batches: m.accepted_batches,
            duplicate_batches: m.duplicate_batches,
            out_of_order_batches: m.out_of_order_batches,
            gaps: m.gaps,
            schema_errors: m.schema_errors,
            books_applied: m.books_applied,
            older_books: m.older_books,
            trades: m.trades,
            duplicate_trades: m.duplicate_trades,
            trade_id_evictions: m.trade_id_evictions,
            reconnects: m.reconnects,
            recoveries: m.recoveries,
            recovery_discards: m.recovery_discards,
            clock_skew_samples: m.clock_skew_samples,
            event_age: StatJson { count: m.event_age.count, sumMs: m.event_age.sum_ms, maxMs: m.event_age.max_ms },
            recovery_latency: StatJson {
                count: m.recovery_latency.count,
                sumMs: m.recovery_latency.sum_ms,
                maxMs: m.recovery_latency.max_ms,
            },
        }
    }
}
