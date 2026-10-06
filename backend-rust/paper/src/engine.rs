// paper/engine.mjs — offline, long-only, isolated per-outcome instrument replay.
use crate::fixed::{big_amount, big_fee, big_price, big_sequence, money};
use crate::schema::{validate_config, validate_event};
use baal_service_runtime::fixed::instant;
use indexmap::IndexMap;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::{BTreeMap, HashMap};

fn chrono_ms_iso(ms: i64) -> String {
    baal_service_runtime::contract::ms_to_iso(ms as f64)
}
fn now_iso(now: Option<i64>) -> V {
    match now {
        Some(n) => json!(chrono_ms_iso(n)),
        None => V::Null,
    }
}
fn big_i(n: i64) -> BigInt {
    BigInt::from(n)
}
fn big_f(n: f64) -> BigInt {
    BigInt::from(n as i64)
}

#[derive(Clone)]
struct Order {
    id: String,
    instrument: String,
    action: String,
    quantity: i64,
    remaining: i64,
    limit: BigInt,
    status: String,
    reason: Option<String>,
    #[allow(dead_code)]
    requested_at: i64,
    activated_at: Option<i64>,
    terminal_at: Option<i64>,
    expires_at: Option<i64>,
    queue_ahead: BigInt,
    ordinal: i64,
}

fn live(o: &Order) -> bool {
    ["pending", "open", "pending_cancel"].contains(&o.status.as_str())
}
fn eligible(o: &Order) -> bool {
    ["open", "pending_cancel"].contains(&o.status.as_str()) && o.activated_at.is_some()
}

#[derive(Clone, Default)]
struct Level {
    external: i64,
    total: i64,
}

struct Book {
    sequence: BigInt,
    watermark: BigInt,
    watermark_time: i64,
    received: i64,
    occurred: i64,
    expiry: Option<i64>,
    invalid: bool,
    bids: BTreeMap<BigInt, Level>,
    asks: BTreeMap<BigInt, Level>,
    source: V,
}

#[derive(Clone)]
struct Barrier {
    sequence: BigInt,
    time: i64,
    observed_source_time: i64,
}

#[derive(Clone)]
struct Lot {
    quantity: i64,
    cost: BigInt,
}

#[derive(Clone)]
struct Action {
    at: i64,
    kind: String, // expire | cancel | activate
    order: String,
    ordinal: i64,
}

#[derive(Default)]
pub struct Stats {
    pub duplicates: i64,
    pub stale_versions: i64,
    pub stale_evidence: i64,
    pub gaps: i64,
    pub rejected_orders: i64,
    pub own_aggressor_excluded: i64,
    pub simulated_write_requests: i64,
}

pub struct PaperEngine {
    config: V,
    initial_cash: BigInt,
    cash: BigInt,
    books: HashMap<String, Book>,
    orders: IndexMap<String, Order>,
    positions: IndexMap<String, Vec<Lot>>,
    marks: HashMap<String, V>,
    seen: IndexMap<String, V>,
    trade_evidence: HashMap<String, V>,
    recovery_barriers: HashMap<String, Barrier>,
    now: Option<i64>,
    actions: Vec<Action>,
    ordinal: i64,
    fills: Vec<V>,
    audit: Vec<V>,
    realized: BigInt,
    total_fees: BigInt,
    turnover: BigInt,
    submitted_shares: BigInt,
    filled_shares: BigInt,
    high_water: BigInt,
    max_drawdown: BigInt,
    equity_observations: i64,
    incomplete_equity_observations: i64,
    stats: Stats,
    failed: bool,
}

impl PaperEngine {
    pub fn new(config: &V) -> Result<PaperEngine, String> {
        let cfg = validate_config(config)?;
        let initial_cash = big_amount(cfg["initialCash"].as_str().unwrap_or(""))?;
        Ok(PaperEngine {
            config: cfg,
            initial_cash: initial_cash.clone(),
            cash: initial_cash.clone(),
            books: HashMap::new(),
            orders: IndexMap::new(),
            positions: IndexMap::new(),
            marks: HashMap::new(),
            seen: IndexMap::new(),
            trade_evidence: HashMap::new(),
            recovery_barriers: HashMap::new(),
            now: None,
            actions: Vec::new(),
            ordinal: 0,
            fills: Vec::new(),
            audit: Vec::new(),
            realized: BigInt::from(0),
            total_fees: BigInt::from(0),
            turnover: BigInt::from(0),
            submitted_shares: BigInt::from(0),
            filled_shares: BigInt::from(0),
            high_water: initial_cash,
            max_drawdown: BigInt::from(0),
            equity_observations: 0,
            incomplete_equity_observations: 0,
            stats: Stats::default(),
            failed: false,
        })
    }

    fn note(&mut self, reason: &str, details: V) {
        let mut row = Map::new();
        row.insert("at".into(), now_iso(self.now));
        row.insert("reason".into(), json!(reason));
        if let V::Object(o) = details {
            for (k, v) in o {
                row.insert(k, v);
            }
        }
        self.audit.push(V::Object(row));
    }

    pub fn cash_now(&self) -> BigInt {
        self.cash.clone()
    }
    pub fn initial_cash(&self) -> BigInt {
        self.initial_cash.clone()
    }
    pub fn realized(&self) -> BigInt {
        self.realized.clone()
    }
    pub fn total_fees(&self) -> BigInt {
        self.total_fees.clone()
    }
    pub fn is_failed(&self) -> bool {
        self.failed
    }
    pub fn stats(&self) -> &Stats {
        &self.stats
    }
    pub fn fills(&self) -> &[V] {
        &self.fills
    }
    pub fn audit(&self) -> &[V] {
        &self.audit
    }
    pub fn order_ids(&self) -> Vec<String> {
        self.orders.keys().cloned().collect()
    }
    pub fn order_view(&self, id: &str) -> Option<V> {
        self.orders.get(id).map(|o| {
            json!({
                "status": o.status,
                "reason": o.reason,
                "action": o.action,
                "activatedAt": o.activated_at,
                "queueAhead": o.queue_ahead.to_string(),
                "remaining": o.remaining,
                "ordinal": o.ordinal,
                "instrument": o.instrument,
                "limit": o.limit.to_string(),
            })
        })
    }
    pub fn order_views(&self) -> Vec<V> {
        self.orders.keys().filter_map(|id| self.order_view(id)).collect()
    }
    pub fn quantity(&self, instrument: &str) -> i64 {
        self.positions
            .get(instrument)
            .map(|lots| lots.iter().map(|l| l.quantity).sum())
            .unwrap_or(0)
    }
    pub fn basis(&self, instrument: &str) -> BigInt {
        self.positions
            .get(instrument)
            .map(|lots| lots.iter().map(|l| l.cost.clone()).sum())
            .unwrap_or_else(|| BigInt::from(0))
    }
    fn reserve_for(&self, o: &Order) -> BigInt {
        let unit_fee = big_fee(&o.limit, &BigInt::from(1), &self.config["costs"]).unwrap_or_default();
        let unit = if o.action == "buy" {
            &o.limit + &unit_fee
        } else if unit_fee > o.limit {
            unit_fee - &o.limit
        } else {
            BigInt::from(0)
        };
        big_i(o.remaining) * unit
    }
    pub fn reserved_cash(&self) -> BigInt {
        self.orders
            .values()
            .filter(|o| live(o))
            .map(|o| self.reserve_for(o))
            .fold(BigInt::from(0), |a, b| a + b)
    }
    pub fn reserved_shares(&self, instrument: &str) -> i64 {
        self.orders
            .values()
            .filter(|o| live(o) && o.instrument == instrument && o.action == "sell")
            .map(|o| o.remaining)
            .sum()
    }
    fn book_fresh(&self, instrument: &str, at: i64) -> bool {
        let Some(b) = self.books.get(instrument) else { return false };
        !b.invalid
            && at >= b.received
            && at - b.received <= self.config["maxDataAgeMs"].as_i64().unwrap_or(0)
            && at - b.occurred <= self.config["maxDataAgeMs"].as_i64().unwrap_or(0)
            && b.expiry.map(|e| at < e).unwrap_or(true)
    }
    fn invalidate(&mut self, instrument: &str, observed_sequence: BigInt, at: i64) {
        let book_seq = self.books.get(instrument).map(|b| b.watermark.clone());
        if let Some(b) = self.books.get_mut(instrument) {
            b.invalid = true;
        }
        let old = self.recovery_barriers.get(instrument);
        let floor = [observed_sequence, book_seq.unwrap_or_else(|| BigInt::from(0)), old.map(|o| o.sequence.clone()).unwrap_or_else(|| BigInt::from(0))]
            .into_iter()
            .max()
            .unwrap();
        let barrier = Barrier {
            sequence: floor,
            time: at.max(old.map(|o| o.time).unwrap_or(at)),
            observed_source_time: at.max(old.map(|o| o.observed_source_time).unwrap_or(at)),
        };
        self.recovery_barriers.insert(instrument.to_string(), barrier);
    }
    fn schedule(&mut self, at: i64, kind: &str, order: &str) -> Result<(), String> {
        // JS Number.isSafeInteger guard.
        if !(-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&at) {
            return Err("Scheduled timestamp overflow".into());
        }
        let ordinal = self.ordinal;
        self.ordinal += 1;
        self.actions.push(Action { at, kind: kind.into(), order: order.into(), ordinal });
        Ok(())
    }
    fn advance(&mut self, at: i64) -> Result<(), String> {
        if let Some(n) = self.now {
            if at < n {
                return Err("Replay must be chronological by receivedAt".into());
            }
        }
        let prio = |k: &str| match k {
            "expire" => 0,
            "cancel" => 1,
            _ => 2,
        };
        self.actions.sort_by(|a, b| a.at.cmp(&b.at).then(prio(&a.kind).cmp(&prio(&b.kind))).then(a.ordinal.cmp(&b.ordinal)));
        loop {
            let Some(a) = self.actions.first() else { break };
            if a.at > at {
                break;
            }
            let a = self.actions.remove(0);
            self.now = Some(a.at);
            let oid = a.order.clone();
            let Some(o) = self.orders.get(&oid).cloned() else { continue };
            if !live(&o) {
                continue;
            }
            if a.kind == "expire" {
                self.close(&oid, "expired");
            } else if a.kind == "cancel" {
                self.close(&oid, "cancelled");
            } else if o.activated_at.is_none() {
                self.activate(&oid);
            }
        }
        self.now = Some(at);
        Ok(())
    }
    fn close(&mut self, order_id: &str, status: &str) {
        if let Some(o) = self.orders.get_mut(order_id) {
            o.status = status.into();
            o.terminal_at = self.now;
        }
        self.note(status, json!({"orderId": order_id}));
    }
    fn reject(&mut self, order_id: &str, reason: &str) {
        if let Some(o) = self.orders.get_mut(order_id) {
            o.status = "rejected".into();
            o.reason = Some(reason.into());
            o.terminal_at = self.now;
        }
        self.stats.rejected_orders += 1;
        self.note(reason, json!({"orderId": order_id}));
    }
    fn normalize_queue(&mut self, instrument: &str, action: &str, limit: &BigInt) {
        let mut floor = BigInt::from(0);
        let mut ids: Vec<String> = self
            .orders
            .values()
            .filter(|x| eligible(x) && x.instrument == instrument && x.action == action && x.limit == *limit)
            .map(|x| x.id.clone())
            .collect();
        ids.sort_by_key(|id| self.orders.get(id).map(|o| o.ordinal).unwrap_or(0));
        for id in ids {
            let o = self.orders.get_mut(&id).unwrap();
            if o.queue_ahead < floor {
                o.queue_ahead = floor.clone();
            }
            floor = o.queue_ahead.clone() + big_i(o.remaining);
        }
    }
    fn activate(&mut self, order_id: &str) {
        let o = self.orders.get(order_id).unwrap().clone();
        if !self.book_fresh(&o.instrument, self.now.unwrap()) {
            return self.reject(order_id, "book_not_fresh_at_activation");
        }
        let b = self.books.get(&o.instrument).unwrap();
        let opposing = if o.action == "buy" { &b.asks } else { &b.bids };
        if opposing
            .iter()
            .any(|(p, level)| level.total > 0 && if o.action == "buy" { p <= &o.limit } else { p >= &o.limit })
        {
            return self.reject(order_id, "marketable_order_unsupported");
        }
        if self.orders.values().any(|x| {
            eligible(x)
                && x.instrument == o.instrument
                && x.action != o.action
                && if o.action == "buy" { x.limit <= o.limit } else { x.limit >= o.limit }
        }) {
            return self.reject(order_id, "crosses_own_paper_order");
        }
        let same = if o.action == "buy" { &b.bids } else { &b.asks };
        let earlier_sum: i64 = self
            .orders
            .values()
            .filter(|x| {
                x.id != o.id
                    && eligible(x)
                    && x.instrument == o.instrument
                    && x.action == o.action
                    && x.limit == o.limit
            })
            .map(|x| x.remaining)
            .sum();
        let depth = same.get(&o.limit).map(|l| l.total).unwrap_or(0);
        let queue_ahead = big_i(depth) + big_i(earlier_sum);
        {
            let om = self.orders.get_mut(order_id).unwrap();
            om.queue_ahead = queue_ahead.clone();
            om.activated_at = self.now;
            if om.status != "pending_cancel" {
                om.status = "open".into();
            }
        }
        self.normalize_queue(&o.instrument, &o.action, &o.limit);
        self.note("activated", json!({"orderId": o.id, "queueAhead": queue_ahead.to_string()}));
    }

    fn submit(&mut self, e: &V) -> Result<(), String> {
        let order_id = e["orderId"].as_str().unwrap_or("").to_string();
        if self.orders.contains_key(&order_id) {
            return Err("Order ID already used; only identical eventId replay is idempotent".into());
        }
        let now = self.now.unwrap();
        let mut o = Order {
            id: order_id.clone(),
            instrument: e["instrument"].as_str().unwrap_or("").to_string(),
            action: e["action"].as_str().unwrap_or("").to_string(),
            quantity: e["quantity"].as_i64().unwrap_or(0),
            remaining: e["quantity"].as_i64().unwrap_or(0),
            limit: big_price(e["limitPrice"].as_str().unwrap_or(""))?,
            status: "pending".into(),
            reason: None,
            requested_at: now,
            activated_at: None,
            terminal_at: None,
            expires_at: if e["expiresAt"].is_null() { None } else { Some(instant(e["expiresAt"].as_str().unwrap_or(""))?) },
            queue_ahead: BigInt::from(0),
            ordinal: self.ordinal,
        };
        self.ordinal += 1;
        self.stats.simulated_write_requests += 1;
        self.submitted_shares += big_i(o.quantity);
        let mut reject_reason: Option<&str> = None;
        if !self.book_fresh(&o.instrument, now) {
            reject_reason = Some("book_not_fresh_at_submission");
        } else if o.action == "buy" {
            let incoming: i64 = self
                .orders
                .values()
                .filter(|x| live(x) && x.action == "buy" && x.instrument == o.instrument)
                .map(|x| x.remaining)
                .sum();
            if big_i(self.quantity(&o.instrument)) + big_i(incoming) + big_i(o.quantity)
                > big_f(self.config["maxPositionShares"].as_f64().unwrap_or(0.0))
            {
                reject_reason = Some("inventory_limit");
            } else if &self.cash - &self.reserved_cash() < self.reserve_for(&o) {
                reject_reason = Some("insufficient_unreserved_cash");
            }
        } else {
            if self.quantity(&o.instrument) - self.reserved_shares(&o.instrument) < o.quantity {
                reject_reason = Some("insufficient_unreserved_inventory");
            } else if &self.cash - &self.reserved_cash() < self.reserve_for(&o) {
                reject_reason = Some("insufficient_unreserved_cash_for_sell_fees");
            }
        }
        if let Some(r) = reject_reason {
            o.status = "rejected".into();
            o.reason = Some(r.into());
            o.terminal_at = self.now;
            self.stats.rejected_orders += 1;
            self.orders.insert(order_id.clone(), o);
            self.note(r, json!({"orderId": order_id}));
            return Ok(());
        }
        self.orders.insert(order_id.clone(), o);
        self.schedule(now + self.config["orderLatencyMs"].as_i64().unwrap_or(0), "activate", &order_id)?;
        let expires = self.orders.get(&order_id).unwrap().expires_at;
        if let Some(exp) = expires {
            self.schedule(exp, "expire", &order_id)?;
        }
        self.advance(now)
    }

    fn cancel(&mut self, e: &V) -> Result<(), String> {
        let order_id = e["orderId"].as_str().unwrap_or("");
        let Some(o) = self.orders.get(order_id).cloned() else {
            return Err("Cannot cancel unknown order".into());
        };
        if !live(&o) || o.status == "pending_cancel" {
            self.note("cancel_noop", json!({"orderId": order_id}));
            return Ok(());
        }
        self.stats.simulated_write_requests += 1;
        {
            let om = self.orders.get_mut(order_id).unwrap();
            om.status = "pending_cancel".into();
        }
        self.schedule(self.now.unwrap() + self.config["cancelLatencyMs"].as_i64().unwrap_or(0), "cancel", order_id)?;
        self.advance(self.now.unwrap())
    }

    fn snapshot(&mut self, e: &V) -> Result<(), String> {
        let instrument = e["instrument"].as_str().unwrap_or("").to_string();
        let seq = big_sequence(e["sequence"].as_str().unwrap_or(""))?;
        let occurred = instant(e["occurredAt"].as_str().unwrap_or(""))?;
        let mut barrier_entry = false;
        if let Some(barrier) = self.recovery_barriers.get_mut(&instrument) {
            barrier_entry = true;
            if seq > barrier.sequence {
                barrier.sequence = seq.clone();
            }
            barrier.observed_source_time = barrier.observed_source_time.max(occurred);
            let b = barrier.clone();
            if !e["authoritative"].as_bool().unwrap_or(false)
                || seq < b.sequence
                || occurred <= b.time
                || occurred < b.observed_source_time
            {
                self.stats.stale_versions += 1;
                self.note("snapshot_before_recovery_barrier", json!({"eventId": e["eventId"]}));
                return Ok(());
            }
        }
        let _ = barrier_entry;
        let old = self.books.get(&instrument);
        if let Some(old) = old {
            if seq < old.watermark || (seq == old.watermark && occurred < old.watermark_time) {
                self.stats.stale_versions += 1;
                return Ok(());
            }
            if occurred < old.watermark_time {
                self.invalidate(&instrument, seq, self.now.unwrap());
                self.stats.gaps += 1;
                self.note("backward_snapshot_time_requires_resync", json!({"eventId": e["eventId"]}));
                return Ok(());
            }
            if seq == old.sequence && occurred == old.occurred {
                let content = |x: &V| {
                    json!({
                        "instrument": x["instrument"],
                        "sequence": x["sequence"],
                        "occurredAt": x["occurredAt"],
                        "bids": x["bids"],
                        "asks": x["asks"],
                        "nextExpiryAt": x["nextExpiryAt"],
                    })
                };
                if content(&old.source) != content(e) {
                    return Err("Conflicting snapshot at identical version".into());
                }
                self.stats.stale_versions += 1;
                return Ok(());
            }
        }
        if self.books.get(&instrument).map(|o| o.invalid).unwrap_or(true) && !e["authoritative"].as_bool().unwrap_or(false) {
            self.note("authoritative_snapshot_required", json!({"instrument": instrument}));
            return Ok(());
        }
        let make = |levels: &V| -> Result<BTreeMap<BigInt, Level>, String> {
            let mut m = BTreeMap::new();
            for l in levels.as_array().unwrap_or(&vec![]) {
                let p = big_price(l["price"].as_str().unwrap_or(""))?;
                m.insert(
                    p,
                    Level {
                        external: l["quantity"].as_i64().unwrap_or(0) - l["ownQuantity"].as_i64().unwrap_or(0),
                        total: l["quantity"].as_i64().unwrap_or(0),
                    },
                );
            }
            Ok(m)
        };
        let now = self.now.unwrap();
        let book = Book {
            sequence: seq.clone(),
            watermark: seq,
            watermark_time: occurred,
            received: now,
            occurred,
            expiry: if e["nextExpiryAt"].is_null() { None } else { Some(instant(e["nextExpiryAt"].as_str().unwrap_or(""))?) },
            invalid: false,
            bids: make(&e["bids"])?,
            asks: make(&e["asks"])?,
            source: e.clone(),
        };
        self.books.insert(instrument.clone(), book);
        self.recovery_barriers.remove(&instrument);
        let ids: Vec<String> = self
            .orders
            .values()
            .filter(|o| eligible(o) && o.instrument == instrument)
            .map(|o| o.id.clone())
            .collect();
        for id in ids {
            let o = self.orders.get(&id).unwrap().clone();
            let b = self.books.get(&instrument).unwrap();
            let same = if o.action == "buy" { &b.bids } else { &b.asks };
            let earlier: i64 = self
                .orders
                .values()
                .filter(|x| {
                    eligible(x)
                        && x.ordinal < o.ordinal
                        && x.instrument == instrument
                        && x.action == o.action
                        && x.limit == o.limit
                })
                .map(|x| x.remaining)
                .sum();
            let ahead = big_i(same.get(&o.limit).map(|l| l.total).unwrap_or(0)) + big_i(earlier);
            let om = self.orders.get_mut(&id).unwrap();
            if ahead > om.queue_ahead {
                om.queue_ahead = ahead;
            }
            self.normalize_queue(&instrument, &o.action, &o.limit);
        }
        Ok(())
    }

    fn fill(&mut self, order_id: &str, quantity: i64, e: &V) -> Result<(), String> {
        let (instrument, action, limit) = {
            let o = self.orders.get(order_id).unwrap();
            (o.instrument.clone(), o.action.clone(), o.limit.clone())
        };
        let notional = &limit * big_i(quantity);
        let charge = big_fee(&notional, &big_i(quantity), &self.config["costs"])?;
        if action == "buy" {
            self.cash -= &notional + &charge;
            self.positions
                .entry(instrument.clone())
                .or_default()
                .push(Lot { quantity, cost: &notional + &charge });
        } else {
            self.cash += &notional - &charge;
            let mut left = quantity;
            let mut basis = BigInt::from(0);
            let lots = self.positions.get_mut(&instrument).ok_or("Inventory invariant violated")?;
            while left > 0 {
                let Some(lot) = lots.first_mut() else {
                    return Err("Inventory invariant violated".into());
                };
                let n = left.min(lot.quantity);
                let used = if n == lot.quantity {
                    lot.cost.clone()
                } else {
                    &lot.cost * big_i(n) / big_i(lot.quantity)
                };
                lot.cost -= &used;
                lot.quantity -= n;
                basis += used;
                left -= n;
                if lot.quantity == 0 {
                    lots.remove(0);
                }
            }
            self.realized += &notional - &charge - &basis;
        }
        {
            let o = self.orders.get_mut(order_id).unwrap();
            o.remaining -= quantity;
        }
        self.total_fees += &charge;
        self.turnover += &notional;
        self.filled_shares += big_i(quantity);
        self.fills.push(json!({
            "orderId": order_id,
            "eventId": e["eventId"],
            "instrument": instrument,
            "action": action,
            "quantity": quantity,
            "price": money(&limit),
            "notional": money(&notional),
            "fee": money(&charge),
            "occurredAt": e["occurredAt"],
            "receivedAt": e["receivedAt"],
            "model": "explicit_trade_pessimistic_queue",
        }));
        if self.orders.get(order_id).unwrap().remaining == 0 {
            self.close(order_id, "filled");
        }
        if self.cash < BigInt::from(0) || self.cash < self.reserved_cash() {
            return Err("Cash reservation invariant violated".into());
        }
        Ok(())
    }

    fn trade(&mut self, e: &V) -> Result<(), String> {
        let instrument = e["instrument"].as_str().unwrap_or("").to_string();
        let seq = big_sequence(e["sequence"].as_str().unwrap_or(""))?;
        let occurred = instant(e["occurredAt"].as_str().unwrap_or(""))?;
        let p = big_price(e["price"].as_str().unwrap_or(""))?;
        let action = if e["aggressor"] == "sell" { "buy" } else { "sell" };
        let evidence_key = format!("{}/{}", instrument, e["sequence"].as_str().unwrap_or(""));
        let evidence = json!({
            "occurredAt": e["occurredAt"],
            "price": e["price"],
            "quantity": e["quantity"],
            "aggressor": e["aggressor"],
            "bookSequence": e["bookSequence"],
            "externalAggressor": e["externalAggressor"],
        });
        if let Some(old) = self.trade_evidence.get(&evidence_key) {
            if *old != evidence {
                return Err("Conflicting trade evidence sequence".into());
            }
            self.stats.stale_versions += 1;
            return Ok(());
        }
        for o in self.orders.values() {
            if e["externalAggressor"].as_bool().unwrap_or(false)
                && o.instrument == instrument
                && o.action == action
                && o.limit == p
                && o.remaining > 0
                && ["cancelled", "expired"].contains(&o.status.as_str())
                && o.activated_at.is_some()
                && occurred > o.activated_at.unwrap()
                && occurred < o.terminal_at.unwrap_or(i64::MAX)
            {
                return Err("Ambiguous late cancel/expiry fill race; ordered recovery evidence required".into());
            }
        }
        if let Some(b) = self.books.get(&instrument) {
            if seq <= b.watermark {
                self.stats.stale_versions += 1;
                return Ok(());
            }
        }
        let now = self.now.unwrap();
        if !self.book_fresh(&instrument, now) || now - occurred > self.config["maxDataAgeMs"].as_i64().unwrap_or(0) {
            self.invalidate(&instrument, seq, now);
            self.stats.stale_evidence += 1;
            self.note("stale_trade_evidence", json!({"eventId": e["eventId"]}));
            return Ok(());
        }
        let b = self.books.get(&instrument).unwrap();
        let book_seq = big_sequence(e["bookSequence"].as_str().unwrap_or(""))?;
        if book_seq != b.sequence || occurred < b.watermark_time {
            self.invalidate(&instrument, seq, now);
            self.stats.gaps += 1;
            self.note("trade_book_mismatch_requires_resync", json!({"eventId": e["eventId"]}));
            return Ok(());
        }
        self.trade_evidence.insert(evidence_key, evidence);
        {
            let b = self.books.get_mut(&instrument).unwrap();
            b.watermark = seq;
            b.watermark_time = occurred;
            let side = if action == "buy" { &mut b.bids } else { &mut b.asks };
            if let Some(level) = side.get_mut(&p) {
                level.total = (level.total - e["quantity"].as_i64().unwrap_or(0)).max(0);
                level.external = (level.external - e["quantity"].as_i64().unwrap_or(0)).max(0);
            }
        }
        if !e["externalAggressor"].as_bool().unwrap_or(false) {
            self.stats.own_aggressor_excluded += 1;
            return Ok(());
        }
        self.normalize_queue(&instrument, action, &p);
        let mut ids: Vec<String> = self
            .orders
            .values()
            .filter(|o| {
                eligible(o)
                    && o.instrument == instrument
                    && o.action == action
                    && o.limit == p
                    && occurred > o.activated_at.unwrap_or(i64::MAX)
            })
            .map(|o| o.id.clone())
            .collect();
        ids.sort_by_key(|id| self.orders.get(id).map(|o| o.ordinal).unwrap_or(0));
        let volume = big_i(e["quantity"].as_i64().unwrap_or(0));
        for id in ids {
            let mut fill_n = 0i64;
            {
                let o = self.orders.get_mut(&id).unwrap();
                let blocked = volume.clone().min(o.queue_ahead.clone());
                o.queue_ahead -= &blocked;
                let remaining = volume.clone() - blocked;
                let f = remaining.min(big_i(o.remaining));
                if let Some(i) = f.to_string().parse::<i64>().ok() {
                    fill_n = i;
                }
            }
            if fill_n > 0 {
                self.fill(&id, fill_n, e)?;
            }
        }
        Ok(())
    }

    pub fn apply(&mut self, event: &V) -> Result<(), String> {
        if self.failed {
            return Err("Engine is failed closed; start a new replay after fixing the dataset".into());
        }
        match self.apply_validated(event) {
            Ok(r) => Ok(r),
            Err(e) => {
                self.failed = true;
                Err(e)
            }
        }
    }

    fn apply_validated(&mut self, event: &V) -> Result<(), String> {
        let e = event.clone();
        validate_event(&e)?;
        if let Some(prev) = self.seen.get(e["eventId"].as_str().unwrap_or("")) {
            if *prev != e {
                return Err("Conflicting duplicate event ID".into());
            }
            self.stats.duplicates += 1;
            return Ok(());
        }
        self.advance(instant(e["receivedAt"].as_str().unwrap_or(""))?)?;
        match e["type"].as_str().unwrap_or("") {
            "snapshot" => self.snapshot(&e)?,
            "trade" => self.trade(&e)?,
            "submit" => self.submit(&e)?,
            "cancel" => self.cancel(&e)?,
            "gap" => {
                self.invalidate(e["instrument"].as_str().unwrap_or(""), BigInt::from(0), self.now.unwrap());
                self.stats.gaps += 1;
                self.note(
                    "gap_requires_authoritative_resync",
                    json!({"instrument": e["instrument"], "detail": e["reason"]}),
                );
            }
            "mark" => {
                let key = format!(
                    "{}/{}",
                    e["instrument"].as_str().unwrap_or(""),
                    e["source"].as_str().unwrap_or("")
                );
                let replace = match self.marks.get(&key) {
                    None => true,
                    Some(old) => {
                        instant(e["occurredAt"].as_str().unwrap_or(""))?
                            > instant(old["occurredAt"].as_str().unwrap_or(""))?
                    }
                };
                if replace {
                    self.marks.insert(key, e.clone());
                }
            }
            _ => {}
        }
        self.seen.insert(e["eventId"].as_str().unwrap_or("").to_string(), e);
        let valuation = self.valuation();
        if !valuation["referenceEquity"].is_null() {
            let equity = big_amount(valuation["referenceEquity"].as_str().unwrap_or("0"))?;
            if equity > self.high_water {
                self.high_water = equity.clone();
            }
            if &self.high_water - &equity > self.max_drawdown {
                self.max_drawdown = &self.high_water - &equity;
            }
            self.equity_observations += 1;
        } else {
            self.incomplete_equity_observations += 1;
        }
        Ok(())
    }

    pub fn valuation(&self) -> V {
        let mut ref_total = BigInt::from(0);
        let mut ref_complete = true;
        let mut executable = BigInt::from(0);
        let mut depth_complete = true;
        let mut remaining_basis = BigInt::from(0);
        let mut fee_funding_required = BigInt::from(0);
        let mut positions = Vec::new();
        for (instrument, _lots) in &self.positions {
            let quantity = self.quantity(instrument);
            if quantity == 0 {
                continue;
            }
            let basis = self.basis(instrument);
            remaining_basis += &basis;
            let mut marks = Map::new();
            for source in ["reference", "last", "tournament"] {
                let key = format!("{instrument}/{source}");
                let mark = self.marks.get(&key);
                let entry = match mark {
                    Some(mark) => {
                        let stale = self.now.unwrap_or(0)
                            - instant(mark["occurredAt"].as_str().unwrap_or("")).unwrap_or(0)
                            > self.config["maxDataAgeMs"].as_i64().unwrap_or(0);
                        json!({
                            "price": mark["price"],
                            "occurredAt": mark["occurredAt"],
                            "receivedAt": mark["receivedAt"],
                            "stale": stale,
                            "value": money(&(big_amount(mark["price"].as_str().unwrap_or("0")).unwrap_or_default() * big_i(quantity))),
                        })
                    }
                    None => V::Null,
                };
                marks.insert(source.into(), entry);
            }
            let reference = marks.get("reference").cloned().unwrap_or(V::Null);
            if reference.is_null() || reference["stale"].as_bool().unwrap_or(false) {
                ref_complete = false;
            } else {
                ref_total += big_amount(reference["value"].as_str().unwrap_or("0")).unwrap_or_default();
            }
            let mut unsold = quantity;
            let mut gross = BigInt::from(0);
            let mut costs = BigInt::from(0);
            let mut levels = Vec::new();
            if self.book_fresh(instrument, self.now.unwrap_or(0)) {
                if let Some(b) = self.books.get(instrument) {
                    for (p, level) in b.bids.iter().rev() {
                        let q = unsold.min(level.external);
                        if q == 0 {
                            continue;
                        }
                        let notional = p * big_i(q);
                        let charge = big_fee(p, &BigInt::from(1), &self.config["costs"]).unwrap_or_default() * big_i(q);
                        gross += &notional;
                        costs += &charge;
                        unsold -= q;
                        levels.push(json!({"price": money(p), "quantity": q}));
                        if unsold == 0 {
                            break;
                        }
                    }
                }
            }
            let proceeds = &gross - &costs;
            executable += &proceeds;
            if unsold > 0 {
                depth_complete = false;
            }
            if proceeds < BigInt::from(0) {
                fee_funding_required -= &proceeds;
            }
            positions.push(json!({
                "instrument": instrument,
                "quantity": quantity,
                "costBasis": money(&basis),
                "marks": V::Object(marks),
                "referenceUnrealizedPnl": if !reference.is_null() && !reference["stale"].as_bool().unwrap_or(false) {
                    json!(money(&(big_amount(reference["value"].as_str().unwrap_or("0")).unwrap_or_default() - &basis)))
                } else { V::Null },
                "executable": {
                    "bookFresh": self.book_fresh(instrument, self.now.unwrap_or(0)),
                    "estimatedNetProceeds": money(&proceeds),
                    "estimatedFees": money(&costs),
                    "soldQuantity": quantity - unsold,
                    "unsoldQuantity": unsold,
                    "fullPositionValue": if unsold == 0 { json!(money(&proceeds)) } else { V::Null },
                    "levels": levels,
                },
            }));
        }
        let available_cash = &self.cash - &self.reserved_cash();
        let fee_funding_shortfall = if fee_funding_required > available_cash {
            &fee_funding_required - &available_cash
        } else {
            BigInt::from(0)
        };
        let executable_complete = depth_complete && fee_funding_shortfall == BigInt::from(0);
        json!({
            "positions": positions,
            "remainingCostBasis": money(&remaining_basis),
            "referenceEquity": if ref_complete { json!(money(&(&self.cash + &ref_total))) } else { V::Null },
            "referenceUnrealizedPnl": if ref_complete { json!(money(&(&ref_total - &remaining_basis))) } else { V::Null },
            "estimatedExecutableEquity": if executable_complete { json!(money(&(&self.cash + &executable))) } else { V::Null },
            "partialExecutableCash": money(&(&self.cash + &executable)),
            "executableComplete": executable_complete,
            "depthComplete": depth_complete,
            "estimatedLiquidationFeeFundingRequired": money(&fee_funding_required),
            "liquidationFeeFundingShortfall": money(&fee_funding_shortfall),
        })
    }

    pub fn report(&self) -> Result<V, String> {
        if self.failed {
            return Err("Engine is failed closed; no valid completed report".into());
        }
        let valuation = self.valuation();
        let mut out = Map::new();
        out.insert("schemaVersion".into(), json!(1));
        out.insert("mode".into(), json!("offline_paper_only"));
        out.insert("asOf".into(), now_iso(self.now));
        out.insert(
            "assumptions".into(),
            json!({
                "fillModel": "explicit_trade_pessimistic_queue",
                "costs": self.config["costs"],
                "noProfitabilityClaim": true,
                "syntheticUntilRealDatasetValidated": true,
                "liveExecutionSupported": false,
                "eligibleForStrategyPromotion": false,
                "queuePessimismIsNotAPnlLowerBound": true,
            }),
        );
        out.insert("cash".into(), json!(money(&self.cash)));
        out.insert("reservedCash".into(), json!(money(&self.reserved_cash())));
        out.insert("realizedPnl".into(), json!(money(&self.realized)));
        out.insert("totalFees".into(), json!(money(&self.total_fees)));
        if let V::Object(o) = valuation {
            for (k, v) in o {
                out.insert(k, v);
            }
        }
        out.insert(
            "orders".into(),
            V::Array(
                self.orders
                    .values()
                    .map(|o| {
                        json!({
                            "orderId": o.id,
                            "instrument": o.instrument,
                            "action": o.action,
                            "quantity": o.quantity,
                            "remaining": o.remaining,
                            "limitPrice": money(&o.limit),
                            "status": o.status,
                            "reason": o.reason,
                            "queueAhead": o.queue_ahead.to_string(),
                        })
                    })
                    .collect(),
            ),
        );
        out.insert("fills".into(), V::Array(self.fills.clone()));
        out.insert(
            "metrics".into(),
            json!({
                "duplicates": self.stats.duplicates,
                "staleVersions": self.stats.stale_versions,
                "staleEvidence": self.stats.stale_evidence,
                "gaps": self.stats.gaps,
                "rejectedOrders": self.stats.rejected_orders,
                "ownAggressorExcluded": self.stats.own_aggressor_excluded,
                "simulatedWriteRequests": self.stats.simulated_write_requests,
                "submittedShares": self.submitted_shares.to_string(),
                "filledShares": self.filled_shares.to_string(),
                "fillRatio": if self.submitted_shares > BigInt::from(0) { json!(format!("{}/{}", self.filled_shares, self.submitted_shares)) } else { V::Null },
                "turnover": money(&self.turnover),
                "observedReferenceMaxDrawdown": money(&self.max_drawdown),
                "equityObservations": self.equity_observations,
                "incompleteEquityObservations": self.incomplete_equity_observations,
                "drawdownComplete": self.incomplete_equity_observations == 0,
                "realizedPnlPerWriteRequest": if self.stats.simulated_write_requests > 0 {
                    json!({"numerator": money(&self.realized), "denominator": self.stats.simulated_write_requests})
                } else { V::Null },
                "sharpe": V::Null,
                "adverseSelection": V::Null,
                "fillCalibration": V::Null,
                "unavailableMetricsReason": "No regular-return series, fixed-horizon post-fill labels, or authoritative real order/fill calibration dataset",
            }),
        );
        out.insert(
            "noTradeBaseline".into(),
            json!({"cash": money(&self.initial_cash), "pnl": money(&BigInt::from(0))}),
        );
        out.insert("audit".into(), V::Array(self.audit.clone()));
        Ok(V::Object(out))
    }
}

pub fn replay(events: &[V], config: &V) -> Result<V, String> {
    let mut engine = PaperEngine::new(config)?;
    for event in events {
        engine.apply(event)?;
    }
    engine.report()
}
