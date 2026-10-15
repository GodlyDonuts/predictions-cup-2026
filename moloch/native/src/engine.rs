//! Causal paper execution. A trade-through is a counterfactual fill, never an
//! actual exchange acknowledgment. Cancellation alone never frees cash.
use crate::{Reservation, Result, SCALE, Side, TICK, Wallet, amount, fee, money, price};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
fn ns(v: &Value) -> Result<i64> {
    chrono::DateTime::parse_from_rfc3339(v.as_str().ok_or("TIMESTAMP")?)
        .map_err(|_| "TIMESTAMP")?
        .timestamp_nanos_opt()
        .ok_or("TIMESTAMP_RANGE")
}
fn ms(v: &Value) -> Result<i64> {
    Ok(ns(v)? / 1_000_000)
}
fn string(v: &Value) -> Result<String> {
    Ok(v.as_str().ok_or("STRING")?.to_owned())
}
fn id(v: &Value) -> Result<String> {
    let s = string(v)?;
    if s.is_empty() || s.starts_with('0') || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err("SCOPE_ID");
    }
    Ok(s)
}
fn dec(v: &Value) -> Result<i128> {
    amount(v.as_str().ok_or("DECIMAL_STRING")?)
}
fn hash(v: &Value) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(v).expect("JSON")))
}
fn label(s: Side) -> &'static str {
    if s == Side::Yes { "YES" } else { "NO" }
}
#[derive(Clone)]
pub struct Policy {
    raw: Value,
    initial: i128,
    committed: i128,
    market: i128,
    quantity: u64,
    markets: usize,
    latency: i64,
    lifetime: i64,
    book_age: i64,
    silence: i64,
    min_spread: i128,
    edge: i128,
    participation: u64,
    fee_bps: u32,
    fee_share: i128,
    quote: String,
    entries: bool,
    inside: u64,
    backoff: i128,
    write_gap: i64,
    writes: usize,
    min_age: i64,
    renew: i64,
    stop: Option<i64>,
    conditional: bool,
    touch: u64,
}
impl Policy {
    pub fn new(overrides: &Value) -> Result<Self> {
        let mut raw = json!({"initialCash":"1000.000000","maximumCommitted":"40.000000","maximumMarketCost":"10.000000","quantity":5,"maximumMarkets":4,"latencyMs":3000,"lifetimeMs":120000,"bookAgeMs":10000,"maximumSourceSilenceMs":60000,"minimumSpreadTicks":1,"minimumEdge":"0.001000","participationBps":100,"feeBps":0,"feePerShare":"0.000000","quoteMode":"pairedSpread","entriesEnabled":true,"fillModel":"strictThrough","touchAttributionBps":10000,"insideTicks":0,"renewBeforeMs":0,"minimumQuoteAgeMs":2500,"writeGapMs":2500,"writesPerMinute":24});
        let allowed = [
            "entryBackoffTicks",
            "entryStopAt",
            "entryExchangeIds",
            "entryDepthBps",
            "maximumEntryPrice",
            "requireCurrentFlow",
        ];
        for (k, v) in overrides.as_object().ok_or("POLICY_OBJECT")? {
            if raw.get(k).is_none() && !allowed.contains(&k.as_str()) {
                return Err("POLICY_NOT_PORTED");
            }
            raw[k] = v.clone();
        }
        let n = |k: &str| {
            raw[k]
                .as_u64()
                .filter(|x| *x <= 2_147_483_647)
                .ok_or("POLICY_INTEGER")
        };
        let p = Self {
            initial: dec(&raw["initialCash"])?,
            committed: dec(&raw["maximumCommitted"])?,
            market: dec(&raw["maximumMarketCost"])?,
            quantity: n("quantity")?,
            markets: n("maximumMarkets")? as usize,
            latency: n("latencyMs")? as i64,
            lifetime: n("lifetimeMs")? as i64,
            book_age: n("bookAgeMs")? as i64,
            silence: n("maximumSourceSilenceMs")? as i64,
            min_spread: n("minimumSpreadTicks")? as i128,
            edge: dec(&raw["minimumEdge"])?,
            participation: n("participationBps")?,
            fee_bps: n("feeBps")? as u32,
            fee_share: dec(&raw["feePerShare"])?,
            quote: string(&raw["quoteMode"])?,
            entries: raw["entriesEnabled"].as_bool().ok_or("POLICY_BOOL")?,
            inside: n("insideTicks")?,
            backoff: raw
                .get("entryBackoffTicks")
                .map(|v| v.as_u64().ok_or("BACKOFF"))
                .transpose()?
                .unwrap_or(0) as i128,
            write_gap: n("writeGapMs")? as i64,
            writes: n("writesPerMinute")? as usize,
            min_age: n("minimumQuoteAgeMs")? as i64,
            renew: n("renewBeforeMs")? as i64,
            stop: raw.get("entryStopAt").map(ms).transpose()?,
            conditional: raw["fillModel"] == "conditionalFifo",
            touch: n("touchAttributionBps")?,
            raw: raw.clone(),
        };
        if p.quantity == 0
            || p.markets == 0
            || p.latency == 0
            || p.book_age == 0
            || p.silence == 0
            || p.min_spread == 0
            || p.min_age == 0
            || p.participation == 0
            || p.participation > 10000
            || p.touch > 10000
            || p.writes == 0
            || p.writes > 24
            || p.write_gap < 2500
            || p.lifetime <= p.latency + p.min_age
            || p.renew >= p.lifetime - p.latency - p.min_age
            || p.edge <= 0
            || p.market <= 0
            || p.inside > 3
            || p.backoff > 10
            || p.backoff > 0 && p.inside > 0
            || !["pairedSpread", "microprice", "cheapSpread"].contains(&p.quote.as_str())
            || !["strictThrough", "conditionalFifo"]
                .contains(&raw["fillModel"].as_str().unwrap_or(""))
        {
            return Err("POLICY_LIMITS");
        }
        if let Some(v) = raw.get("entryDepthBps") {
            if !v.as_u64().is_some_and(|n| n > 0 && n <= 10000) {
                return Err("ENTRY_DEPTH");
            }
        }
        if raw
            .get("requireCurrentFlow")
            .is_some_and(|v| !v.is_boolean())
        {
            return Err("CURRENT_FLOW_POLICY");
        }
        if let Some(v) = raw.get("entryExchangeIds") {
            let xs = v.as_array().ok_or("ENTRY_EXCHANGES")?;
            let mut seen = BTreeSet::new();
            if xs.len() > p.markets {
                return Err("ENTRY_EXCHANGES");
            }
            for x in xs {
                if !seen.insert(id(x)?) {
                    return Err("ENTRY_EXCHANGES");
                }
            }
        }
        if let Some(v) = raw.get("maximumEntryPrice") {
            let x = dec(v)?;
            if x <= 0 || x > SCALE / 2 {
                return Err("ENTRY_PRICE");
            }
        }
        Wallet::new(p.initial, p.committed, p.fee_bps, p.fee_share)?;
        Ok(p)
    }
    fn fee(&self, p: i128, n: u64) -> Result<i128> {
        fee(p * n as i128, n, self.fee_bps, self.fee_share)
    }
    fn unit(&self, p: i128) -> Result<i128> {
        Ok(p + self.fee(p, 1)?)
    }
    fn entry(&self, ex: &str, at: i64) -> bool {
        self.entries
            && self.stop.is_none_or(|s| at < s)
            && self
                .raw
                .get("entryExchangeIds")
                .is_none_or(|v| v.as_array().unwrap().iter().any(|x| x == ex))
    }
}
#[derive(Clone)]
struct Book {
    raw: Value,
    market: String,
    exchange: String,
    tournament: String,
    sequence: u128,
    source: i64,
    request: i64,
    received: i64,
    bids: Vec<(i128, u64)>,
    asks: Vec<(i128, u64)>,
    invalid: bool,
}
impl Book {
    fn parse(b: &Value, received: &Value) -> Result<Self> {
        let request = ms(&b["requestStartedAt"])?;
        let source = ms(&b["sourceAt"])?;
        let recv = ms(&b["receivedAt"])?;
        if b["receivedAt"] != *received
            || ns(&b["requestStartedAt"])? > ns(received)?
            || ns(&b["sourceAt"])?
                > ns(received)?
                    .checked_add(1_000_000_000)
                    .ok_or("CLOCK_RANGE")?
        {
            return Err("BOOK_CLOCK");
        }
        let levels = |k: &str, bid: bool| -> Result<Vec<(i128, u64)>> {
            let mut out = Vec::new();
            for l in b[k].as_array().ok_or("BOOK_DEPTH")? {
                let p = price(dec(&l["price"])?)?;
                let n = l["quantity"]
                    .as_u64()
                    .filter(|n| *n > 0 && *n <= 9_007_199_254_740_991)
                    .ok_or("BOOK_SIZE")?;
                if out
                    .last()
                    .is_some_and(|(old, _)| if bid { *old <= p } else { *old >= p })
                {
                    return Err("BOOK_SORT");
                }
                out.push((p, n));
            }
            if out.is_empty() {
                return Err("BOOK_DEPTH");
            }
            Ok(out)
        };
        let bids = levels("bids", true)?;
        let asks = levels("asks", false)?;
        if bids[0].0 >= asks[0].0 {
            return Err("BOOK_CROSSED");
        }
        let tournament = string(&b["tournamentId"])?;
        if tournament.is_empty() {
            return Err("TOURNAMENT");
        }
        Ok(Self {
            market: id(&b["marketId"])?,
            exchange: id(&b["exchangeId"])?,
            tournament,
            sequence: b["sequence"]
                .as_str()
                .ok_or("BOOK_SEQUENCE")?
                .parse()
                .map_err(|_| "BOOK_SEQUENCE")?,
            source,
            request,
            received: recv,
            bids,
            asks,
            invalid: b["invalid"] == true,
            raw: b.clone(),
        })
    }
    fn clock(&self) -> i64 {
        if self.raw["transport"] == "official_sig_realtime" {
            self.source
        } else {
            self.request
        }
    }
    fn fresh(&self, p: &Policy, at: i64) -> bool {
        !self.invalid && at - self.clock() <= p.book_age && self.received <= at
    }
}
#[derive(Clone)]
struct Order {
    id: u64,
    market: String,
    exchange: String,
    tournament: String,
    side: Side,
    buy: bool,
    quantity: u64,
    filled: u64,
    limit: i128,
    created: i64,
    activation: i64,
    expires: i64,
    cancel: Option<i64>,
    retired: bool,
    credit: u64,
    barrier: Option<(u128, i64)>,
    same: u64,
    better: u64,
    visible_same: u64,
    visible_better: u64,
    touch_remainder: u64,
}
impl Order {
    fn end(&self) -> i64 {
        self.expires.min(self.cancel.unwrap_or(i64::MAX))
    }
    fn remaining(&self) -> u64 {
        self.quantity - self.filled
    }
    fn key(&self) -> (String, Side, bool) {
        (self.exchange.clone(), self.side, self.buy)
    }
}
#[derive(Clone)]
struct Target {
    book: Book,
    side: Side,
    buy: bool,
    quantity: u64,
    limit: i128,
    rank: f64,
}
impl Target {
    fn key(&self) -> (String, Side, bool) {
        (self.book.exchange.clone(), self.side, self.buy)
    }
}
#[derive(Clone)]
struct EntryBook {
    bid: i128,
    ask: i128,
    source: i64,
    received: i64,
    bid_since: i64,
    ask_since: i64,
}
#[derive(Clone, Default)]
struct EntryFlow {
    history: Vec<EntryBook>,
    prints: Vec<(i64, bool, i128)>,
}
#[derive(Clone)]
pub struct Engine {
    policy: Policy,
    pub wallet: Wallet,
    books: BTreeMap<String, Book>,
    orders: Vec<Order>,
    fills: Vec<Value>,
    seen: BTreeMap<String, (String, i64, i128, u64)>,
    flow: BTreeMap<String, Vec<(i64, u64)>>,
    writes: Vec<i64>,
    last: Option<i64>,
    last_string: Value,
    events: u64,
    counts: BTreeMap<String, u64>,
    entry_flow: BTreeMap<String, EntryFlow>,
}
impl Engine {
    fn audit(&self) -> Result<()> {
        self.wallet.audit()?;
        for (ex, side) in self.wallet.lots.keys() {
            let held = self.wallet.quantity(ex, *side);
            if held < self.pledge(ex, *side) + held.min(self.opposite_buys(ex, *side)) {
                return Err("SHARE_PLEDGE");
            }
        }
        Ok(())
    }
    fn current_flow_ready(&self, b: &Book, at: i64) -> bool {
        if self.policy.raw["requireCurrentFlow"] != true {
            return true;
        }
        let Some(m) = self.entry_flow.get(&b.exchange) else {
            return false;
        };
        let Some(latest) = m.history.last() else {
            return false;
        };
        if latest.bid != b.bids[0].0 || latest.ask != b.asks[0].0 {
            return false;
        }
        let valid = |t: &(i64, bool, i128)| {
            t.0 >= at - 180000
                && t.0 <= at
                && t.0
                    >= if t.1 {
                        latest.bid_since
                    } else {
                        latest.ask_since
                    }
                && t.2 == if t.1 { latest.bid } else { latest.ask }
        };
        m.prints.iter().any(|t| t.1 && valid(t)) && m.prints.iter().any(|t| !t.1 && valid(t))
    }
    fn observe_entry_flow(&mut self, e: &Value, duplicate: bool) -> Result<()> {
        if self.policy.raw["requireCurrentFlow"] != true || duplicate {
            return Ok(());
        }
        if e["type"] == "gap" {
            let ids = if let Some(xs) = e["exchangeIds"].as_array() {
                xs.clone()
            } else {
                vec![e["exchangeId"].clone()]
            };
            for id in ids {
                self.entry_flow.remove(id.as_str().ok_or("GAP_ID")?);
            }
            return Ok(());
        }
        if e["type"] == "book" {
            let b = &e["book"];
            let m = self
                .entry_flow
                .entry(string(&b["exchangeId"])?)
                .or_default();
            if b["invalid"] == true {
                m.history.clear();
                m.prints.clear();
                return Ok(());
            }
            let bid = dec(&b["bids"][0]["price"])?;
            let ask = dec(&b["asks"][0]["price"])?;
            let received = ms(&e["receivedAt"])?;
            let bid_since = m
                .history
                .last()
                .filter(|old| old.bid == bid)
                .map_or(received, |old| old.bid_since);
            let ask_since = m
                .history
                .last()
                .filter(|old| old.ask == ask)
                .map_or(received, |old| old.ask_since);
            m.history.push(EntryBook {
                bid,
                ask,
                source: ns(&b["sourceAt"])?,
                received,
                bid_since,
                ask_since,
            });
            if m.history.len() > 128 {
                m.history.remove(0);
            }
            return Ok(());
        }
        if e["type"] != "trade" || e["idNamespace"] != "sig_engine_match" {
            return Ok(());
        }
        let executed = ns(&e["executedAt"])?;
        let at = executed / 1_000_000;
        let m = self
            .entry_flow
            .entry(string(&e["exchangeId"])?)
            .or_default();
        let Some(prior) = m.history.iter().rev().find(|b| {
            b.source < executed && b.received <= at && at - b.source / 1_000_000 <= 10000
        }) else {
            return Ok(());
        };
        let value = dec(&e["yesPrice"])?;
        let bid = if value <= prior.bid {
            true
        } else if value >= prior.ask {
            false
        } else {
            return Ok(());
        };
        let since = if bid {
            prior.bid_since
        } else {
            prior.ask_since
        };
        if at - since < self.policy.latency {
            return Ok(());
        }
        m.prints
            .push((at, bid, if bid { prior.bid } else { prior.ask }));
        m.prints.retain(|t| t.0 >= at - 180000);
        if m.prints.len() > 2048 {
            m.prints.drain(..m.prints.len() - 2048);
        }
        Ok(())
    }
    pub fn new(overrides: &Value) -> Result<Self> {
        let policy = Policy::new(overrides)?;
        let wallet = Wallet::new(
            policy.initial,
            policy.committed,
            policy.fee_bps,
            policy.fee_share,
        )?;
        Ok(Self {
            policy,
            wallet,
            books: BTreeMap::new(),
            orders: Vec::new(),
            fills: Vec::new(),
            seen: BTreeMap::new(),
            flow: BTreeMap::new(),
            writes: Vec::new(),
            last: None,
            last_string: Value::Null,
            events: 0,
            counts: BTreeMap::new(),
            entry_flow: BTreeMap::new(),
        })
    }
    fn count(&mut self, k: &str) {
        *self.counts.entry(k.to_owned()).or_default() += 1;
    }
    fn pledge(&self, ex: &str, side: Side) -> u64 {
        self.orders
            .iter()
            .filter(|q| !q.retired && !q.buy && q.exchange == ex && q.side == side)
            .map(Order::remaining)
            .sum()
    }
    fn opposite_buys(&self, ex: &str, side: Side) -> u64 {
        self.orders
            .iter()
            .filter(|q| !q.retired && q.buy && q.exchange == ex && q.side != side)
            .map(Order::remaining)
            .sum()
    }
    fn visible(b: &Book, side: Side, buy: bool, limit: i128) -> (u64, u64) {
        let yes = if side == Side::Yes {
            limit
        } else {
            SCALE - limit
        };
        let bid = (side == Side::Yes) == buy;
        let mut same = 0;
        let mut better = 0;
        for (p, n) in if bid { &b.bids } else { &b.asks } {
            if *p == yes {
                same += n
            } else if if bid { *p > yes } else { *p < yes } {
                better += n
            }
        }
        (same, better)
    }
    fn fill(&mut self, i: usize, n: u64, at: &Value, source: &Value) -> Result<()> {
        let q = &self.orders[i];
        if q.buy {
            self.wallet.buy_fill(&q.id.to_string(), n)?
        } else {
            self.wallet.sell_fill(&q.exchange, q.side, n, q.limit)?
        }
        self.fills.push(json!({"orderId":q.id,"marketId":q.market,"exchangeId":q.exchange,"side":label(q.side),"action":if q.buy{"buy"}else{"sell"},"quantity":n,"price":money(q.limit),"fee":money(self.policy.fee(q.limit,n)?),"at":at,"sourceId":source,"counterfactual":true}));
        let q = &mut self.orders[i];
        q.filled += n;
        if q.remaining() == 0 {
            q.retired = true
        }
        Ok(())
    }
    fn desired(&self, at: i64) -> Result<Vec<Target>> {
        let mut out = Vec::new();
        let p = &self.policy;
        for b in self.books.values() {
            if !b.fresh(p, at) {
                continue;
            }
            let best_bid = b.bids[0].0;
            let best_ask = b.asks[0].0;
            let spread = best_ask - best_bid;
            let micro = (best_ask * b.bids[0].1 as i128 + best_bid * b.asks[0].1 as i128)
                / (b.bids[0].1 as i128 + b.asks[0].1 as i128);
            let yes = self.wallet.quantity(&b.exchange, Side::Yes);
            let no = self.wallet.quantity(&b.exchange, Side::No);
            if yes > 0 || no > 0 {
                let side = if yes > 0 { Side::Yes } else { Side::No };
                let existing: u64 = self
                    .orders
                    .iter()
                    .filter(|q| {
                        !q.retired
                            && !q.buy
                            && q.exchange == b.exchange
                            && q.side == side
                            && q.end() > at
                    })
                    .map(Order::remaining)
                    .sum();
                let n = p.quantity.min(
                    self.wallet.quantity(&b.exchange, side).saturating_sub(
                        self.pledge(&b.exchange, side) + self.opposite_buys(&b.exchange, side),
                    ) + existing,
                );
                let base = if side == Side::Yes {
                    best_ask
                } else {
                    SCALE - best_bid
                };
                let mut improve = TICK * (p.inside as i128).min((spread / TICK - 1).max(0));
                let worst = self
                    .wallet
                    .lots
                    .get(&(b.exchange.clone(), side))
                    .into_iter()
                    .flatten()
                    .map(|l| l.price)
                    .max()
                    .unwrap_or(0);
                while improve > 0
                    && base - improve - worst - p.fee(worst, 1)? - p.fee(base - improve, 1)?
                        < p.edge
                {
                    improve -= TICK;
                }
                if n > 0 {
                    out.push(Target {
                        book: b.clone(),
                        side,
                        buy: false,
                        quantity: n,
                        limit: base - improve,
                        rank: 1e12,
                    })
                }
                continue;
            }
            if !p.entry(&b.exchange, at)
                || !self.current_flow_ready(b, at)
                || spread < TICK * p.min_spread
            {
                continue;
            }
            let mut bid = best_bid;
            let mut ask = best_ask;
            for _ in 0..p.inside {
                let sides = if micro - bid >= ask - micro {
                    [Side::Yes, Side::No]
                } else {
                    [Side::No, Side::Yes]
                };
                for side in sides {
                    let nb = bid + if side == Side::Yes { TICK } else { 0 };
                    let na = ask - if side == Side::No { TICK } else { 0 };
                    let limit = if side == Side::Yes { nb } else { SCALE - na };
                    let value = if side == Side::Yes {
                        micro
                    } else {
                        SCALE - micro
                    };
                    if na - nb < TICK
                        || na - nb - p.fee(nb, 1)? - p.fee(SCALE - na, 1)? < 2 * p.edge
                        || value - limit - p.fee(limit, 1)? < p.edge
                    {
                        continue;
                    }
                    bid = nb;
                    ask = na;
                }
            }
            bid -= p.backoff * TICK;
            ask += p.backoff * TICK;
            if bid < TICK || ask > SCALE - TICK {
                continue;
            }
            let quoted = ask - bid;
            if p.quote == "pairedSpread"
                && quoted - p.fee(bid, 1)? - p.fee(SCALE - ask, 1)? < 2 * p.edge
            {
                continue;
            }
            let activity: u64 = self
                .flow
                .get(&b.exchange)
                .into_iter()
                .flatten()
                .filter(|(t, _)| at - t <= 900000)
                .map(|(_, n)| *n)
                .sum();
            let rank = spread as f64 * ((activity as f64) + 2.0).ln() / (1.0 + bid as f64);
            for (side, limit, value) in [
                (Side::Yes, bid, micro),
                (Side::No, SCALE - ask, SCALE - micro),
            ] {
                if p.quote == "microprice" && value - limit - p.fee(limit, 1)? < p.edge {
                    continue;
                }
                if p.quote == "cheapSpread"
                    && (side
                        != if bid <= SCALE - ask {
                            Side::Yes
                        } else {
                            Side::No
                        }
                        || limit
                            > p.raw
                                .get("maximumEntryPrice")
                                .map(dec)
                                .transpose()?
                                .unwrap_or(100000)
                        || quoted - p.fee(limit, 1)? - p.fee(limit + quoted, 1)? < p.edge)
                {
                    continue;
                }
                let rank = if p.quote == "cheapSpread" {
                    (quoted - p.fee(limit, 1)? - p.fee(limit + quoted, 1)?) as f64
                        / (2 * limit + quoted) as f64
                        * ((activity as f64) + 2.0).ln()
                } else {
                    rank
                };
                out.push(Target {
                    book: b.clone(),
                    side,
                    buy: true,
                    quantity: p.quantity,
                    limit,
                    rank,
                });
            }
        }
        out.sort_by(|a, b| {
            b.rank
                .total_cmp(&a.rank)
                .then_with(|| a.book.exchange.cmp(&b.book.exchange))
                .then_with(|| label(a.side).cmp(label(b.side)))
        });
        Ok(out)
    }
    fn reconcile(&mut self, at: i64) -> Result<()> {
        let targets = self.desired(at)?;
        self.writes.retain(|t| at - t < 60000);
        if self.writes.len() >= self.policy.writes
            || self
                .writes
                .last()
                .is_some_and(|t| at - t < self.policy.write_gap)
        {
            self.count("writeBudgetHolds");
            return Ok(());
        }
        for i in 0..self.orders.len() {
            let q = &self.orders[i];
            if q.retired || q.end() <= at || q.cancel.is_some() {
                continue;
            }
            let target = targets.iter().find(|t| t.key() == q.key());
            let book = self.books.get(&q.exchange);
            let covered = if q.buy {
                self.wallet.quantity(&q.exchange, Side::Yes)
                    + self.wallet.quantity(&q.exchange, Side::No)
                    == 0
            } else {
                self.wallet.quantity(&q.exchange, q.side) >= q.remaining()
            };
            if target.is_none()
                && book.is_some_and(|b| {
                    !b.invalid
                        && !b.fresh(&self.policy, at)
                        && at - b.received < self.policy.silence
                })
                && covered
                && (q.buy && self.policy.entry(&q.exchange, at) || !q.buy)
            {
                continue;
            }
            let renew = target.is_some_and(|t| t.limit == q.limit)
                && book.is_some_and(|b| b.fresh(&self.policy, at))
                && self.policy.renew > 0
                && q.expires - at <= self.policy.renew;
            if (target.is_none_or(|t| t.limit != q.limit) || renew)
                && at - q.created >= self.policy.min_age
            {
                self.orders[i].cancel = Some(at + self.policy.latency);
                self.writes.push(at);
                self.count("cancels");
                if renew {
                    self.count("earlyRenewals")
                }
                return Ok(());
            }
        }
        let mut markets = BTreeSet::new();
        for ((ex, _), lots) in &self.wallet.lots {
            if !lots.is_empty() {
                markets.insert(ex.clone());
            }
        }
        for q in &self.orders {
            if !q.retired {
                markets.insert(q.exchange.clone());
            }
        }
        let mut available = self.wallet.cash - self.wallet.reserved()?;
        let mut headroom =
            self.policy.committed - self.wallet.owned_cost() - self.wallet.reserved()?;
        let mut costs = BTreeMap::new();
        for ex in &markets {
            costs.insert(ex.clone(), self.wallet.market_cost(ex));
        }
        for q in &self.orders {
            if !q.retired && q.buy {
                *costs.entry(q.exchange.clone()).or_default() +=
                    self.policy.unit(q.limit)? * q.remaining() as i128;
            }
        }
        let mut batch: Vec<Target> = Vec::new();
        for t in targets.iter() {
            if self
                .orders
                .iter()
                .any(|q| !q.retired && q.end() > at && q.key() == t.key())
                || batch.len() >= 8
            {
                continue;
            }
            if !markets.contains(&t.book.exchange) && markets.len() >= self.policy.markets {
                continue;
            }
            let mut n = t.quantity;
            if !t.buy {
                n = n.min(
                    self.wallet
                        .quantity(&t.book.exchange, t.side)
                        .saturating_sub(
                            self.pledge(&t.book.exchange, t.side)
                                + self.opposite_buys(&t.book.exchange, t.side),
                        ),
                );
            } else {
                let mate = batch
                    .iter()
                    .find(|q| q.book.exchange == t.book.exchange && q.buy && q.side != t.side);
                if let Some(q) = mate {
                    n = n.min(q.quantity)
                }
                let opposite = if self.policy.quote == "pairedSpread"
                    && mate.is_none()
                    && !self.orders.iter().any(|q| {
                        !q.retired
                            && q.end() > at
                            && q.exchange == t.book.exchange
                            && q.buy
                            && q.side != t.side
                    }) {
                    targets
                        .iter()
                        .find(|q| q.book.exchange == t.book.exchange && q.buy && q.side != t.side)
                } else {
                    None
                };
                if let Some(bps) = self.policy.raw["entryDepthBps"].as_u64() {
                    let depth = |q: &Target| {
                        if q.side == Side::Yes {
                            q.book.bids[0].1
                        } else {
                            q.book.asks[0].1
                        }
                    };
                    n = n.min((depth(t) as u128 * bps as u128 / 10000) as u64);
                    if let Some(q) = opposite {
                        n = n.min((depth(q) as u128 * bps as u128 / 10000) as u64)
                    }
                }
                let unit = self.policy.unit(t.limit)?
                    + opposite
                        .map(|q| self.policy.unit(q.limit))
                        .transpose()?
                        .unwrap_or(0);
                let room = available
                    .min(headroom)
                    .min(self.policy.market - costs.get(&t.book.exchange).copied().unwrap_or(0));
                n = n.min(if room > 0 { (room / unit) as u64 } else { 0 });
                if n > 0 {
                    let cost = self.policy.unit(t.limit)? * n as i128;
                    available -= cost;
                    headroom -= cost;
                    *costs.entry(t.book.exchange.clone()).or_default() += cost;
                }
            }
            if n == 0 {
                continue;
            }
            if t.buy
                && (self.orders.iter().any(|q| {
                    !q.retired
                        && q.end() > at
                        && q.buy
                        && q.exchange == t.book.exchange
                        && q.side != t.side
                        && q.limit + t.limit >= SCALE
                }) || batch.iter().any(|q| {
                    q.buy
                        && q.book.exchange == t.book.exchange
                        && q.side != t.side
                        && q.limit + t.limit >= SCALE
                }))
            {
                continue;
            }
            let mut target = t.clone();
            target.quantity = n;
            markets.insert(t.book.exchange.clone());
            batch.push(target);
        }
        if !batch.is_empty() {
            self.writes.push(at);
            for t in batch {
                let id = self.orders.len() as u64 + 1;
                let (same, better) = Self::visible(&t.book, t.side, t.buy, t.limit);
                if t.buy {
                    self.wallet.reserve(
                        &id.to_string(),
                        Reservation {
                            exchange: t.book.exchange.clone(),
                            side: t.side,
                            quantity: t.quantity,
                            limit: t.limit,
                        },
                    )?
                }
                self.orders.push(Order {
                    id,
                    market: t.book.market,
                    exchange: t.book.exchange,
                    tournament: t.book.tournament,
                    side: t.side,
                    buy: t.buy,
                    quantity: t.quantity,
                    filled: 0,
                    limit: t.limit,
                    created: at,
                    activation: at + self.policy.latency,
                    expires: at + self.policy.lifetime,
                    cancel: None,
                    retired: false,
                    credit: 0,
                    barrier: None,
                    same,
                    better,
                    visible_same: same,
                    visible_better: better,
                    touch_remainder: 0,
                });
            }
            self.count("submissionBatches");
        }
        Ok(())
    }
    /// Transactional event admission keeps rejected source data out of the wallet.
    pub fn advance(&mut self, event: &Value) -> Result<()> {
        let mut next = self.clone();
        next.apply(event)?;
        next.audit()?;
        *self = next;
        Ok(())
    }
    fn apply(&mut self, e: &Value) -> Result<()> {
        let received = ns(&e["receivedAt"])?;
        let clock = e.get("processedAt").unwrap_or(&e["receivedAt"]);
        let at_ns = ns(clock)?;
        if at_ns < received || self.last.is_some_and(|t| at_ns < t) {
            return Err("NONCAUSAL_EVENT");
        }
        let at = at_ns / 1_000_000;
        let kind = e["type"].as_str().ok_or("EVENT_TYPE")?;
        let mut duplicate = false;
        match kind {
            "book" => {
                let b = Book::parse(&e["book"], &e["receivedAt"])?;
                if let Some(old) = self.books.get(&b.exchange) {
                    if old.market != b.market
                        || old.tournament != b.tournament
                        || b.sequence < old.sequence
                        || b.sequence == old.sequence && b.source < old.source
                    {
                        return Err("BOOK_ROLLBACK_SCOPE");
                    }
                    if b.sequence == old.sequence
                        && e["book"]["sourceAt"] == old.raw["sourceAt"]
                        && (b.bids != old.bids || b.asks != old.asks)
                    {
                        return Err("BOOK_CONFLICT");
                    }
                }
                for q in &mut self.orders {
                    if q.retired || q.exchange != b.exchange {
                        continue;
                    }
                    if self.policy.conditional {
                        let (same, better) = Self::visible(&b, q.side, q.buy, q.limit);
                        if same > q.visible_same
                            && ns(&b.raw["sourceAt"])? <= q.activation * 1_000_000
                        {
                            q.same += same - q.visible_same
                        }
                        if better > q.visible_better {
                            q.better += better - q.visible_better
                        }
                        q.visible_same = same;
                        q.visible_better = better;
                    }
                    if b.clock() >= q.end() && q.barrier.is_none() {
                        q.barrier = Some((b.sequence, q.end()))
                    }
                }
                self.books.insert(b.exchange.clone(), b);
            }
            "trade" => {
                let exchange = id(&e["exchangeId"])?;
                let market = id(&e["marketId"])?;
                let tournament = string(&e["tournamentId"])?;
                let trade_id = string(&e["id"])?;
                let executed = ns(&e["executedAt"])?;
                let quantity = e["quantity"]
                    .as_u64()
                    .filter(|n| *n > 0 && *n <= 2_147_483_647)
                    .ok_or("TRADE_QUANTITY")?;
                let trade_price = price(dec(&e["yesPrice"])?)?;
                if trade_id.is_empty() || tournament.is_empty() || executed > received {
                    return Err("TRADE_CLOCK_SCOPE");
                }
                if self
                    .books
                    .get(&exchange)
                    .is_some_and(|b| b.market != market || b.tournament != tournament)
                {
                    return Err("TRADE_SCOPE");
                }
                let key = format!("{tournament}:{exchange}:{trade_id}");
                let facts = (market.clone(), executed, trade_price, quantity);
                if let Some(old) = self.seen.get(&key) {
                    if old != &facts {
                        return Err("TRADE_CONFLICT");
                    }
                    duplicate = true;
                } else {
                    self.seen.insert(key, facts);
                }
                if !duplicate {
                    self.flow
                        .entry(exchange.clone())
                        .or_default()
                        .push((executed / 1_000_000, quantity));
                    self.count("uniqueTrades");
                    for i in 0..self.orders.len() {
                        let q = &mut self.orders[i];
                        if q.retired
                            || q.exchange != exchange
                            || q.market != market
                            || q.tournament != tournament
                            || executed < q.activation * 1_000_000
                            || executed >= q.end() * 1_000_000
                        {
                            continue;
                        }
                        let limit = if q.side == Side::Yes {
                            q.limit
                        } else {
                            SCALE - q.limit
                        };
                        let bid = (q.side == Side::Yes) == q.buy;
                        let through = if bid {
                            trade_price <= limit - TICK
                        } else {
                            trade_price >= limit + TICK
                        };
                        let touch = self.policy.conditional && trade_price == limit;
                        if !through && !touch {
                            continue;
                        }
                        let mut eligible = quantity;
                        if through && self.policy.conditional {
                            q.same = 0;
                            q.better = 0
                        }
                        if touch {
                            let numerator = quantity * self.policy.touch + q.touch_remainder;
                            let attributed = numerator / 10000;
                            q.touch_remainder = numerator % 10000;
                            if attributed > 0 {
                                q.better = 0
                            }
                            let consumed = attributed.min(q.same);
                            q.same -= consumed;
                            eligible = attributed - consumed;
                        }
                        q.credit += eligible * self.policy.participation;
                        let n = quantity
                            .min(q.remaining())
                            .min(q.credit / 10000)
                            .min((quantity * self.policy.participation / 10000).max(1));
                        if n > 0 {
                            q.credit -= n * 10000;
                            self.fill(i, n, clock, &e["id"])?;
                            if touch {
                                self.fills.last_mut().unwrap()["conditionalQueueFill"] =
                                    json!(true);
                            }
                            if touch {
                                self.count("conditionalTouches")
                            }
                            break;
                        }
                        if touch {
                            self.count("conditionalTouches");
                            break;
                        }
                    }
                }
            }
            "gap" => {
                let ids = if let Some(xs) = e["exchangeIds"].as_array() {
                    xs.clone()
                } else {
                    vec![e["exchangeId"].clone()]
                };
                let mut seen = BTreeSet::new();
                if ids.is_empty() || ids.len() > 1000 {
                    return Err("GAP_SCOPE");
                }
                for x in ids {
                    let ex = id(&x)?;
                    if !seen.insert(ex.clone()) {
                        return Err("GAP_SCOPE");
                    }
                    if let Some(b) = self.books.get_mut(&ex) {
                        b.invalid = true
                    }
                    self.count("gaps");
                }
            }
            "coverage" => {
                let from = ns(&e["from"])?;
                let to = ns(&e["to"])?;
                let seq = e["projectedThroughSequence"]
                    .as_str()
                    .ok_or("COVERAGE_SEQUENCE")?
                    .parse::<u128>()
                    .map_err(|_| "COVERAGE_SEQUENCE")?;
                if e["complete"] != true || from >= to || to > at_ns {
                    return Err("COVERAGE");
                }
                for q in &mut self.orders {
                    if !q.retired
                        && e["marketId"] == q.market
                        && e["exchangeId"] == q.exchange
                        && e["tournamentId"] == q.tournament
                        && q.barrier.is_some_and(|(barrier, end)| {
                            end == q.end()
                                && from <= q.activation * 1_000_000
                                && to >= q.end() * 1_000_000
                                && seq >= barrier
                        })
                    {
                        if q.buy {
                            self.wallet.release(&q.id.to_string())?
                        }
                        q.retired = true;
                    }
                }
            }
            "candle" => {
                if e["historicalReleaseClockAssumed"] != true
                    || ns(&e["availableAt"])? != received
                    || ns(&e["bucketStartedAt"])? >= received
                    || !e["close"]
                        .as_f64()
                        .is_some_and(|x| (0.0..=1.0).contains(&x))
                {
                    return Err("CANDLE");
                }
                self.count("priceOnlyObservations");
            }
            "tick" => {}
            _ => return Err("EVENT_TYPE"),
        }
        self.last = Some(at_ns);
        self.last_string = clock.clone();
        self.events += 1;
        if duplicate {
            self.count("duplicateTrades")
        }
        self.observe_entry_flow(e, duplicate)?;
        self.reconcile(at)?;
        Ok(())
    }
    pub fn report(&self) -> Result<Value> {
        let mut liquidation = 0;
        let mut known = true;
        for ((ex, side), lots) in &self.wallet.lots {
            let mut n: u64 = lots.iter().map(|l| l.quantity).sum();
            if n == 0 {
                continue;
            }
            let Some(b) = self.books.get(ex).filter(|b| {
                self.last
                    .is_some_and(|t| b.fresh(&self.policy, t / 1_000_000))
            }) else {
                known = false;
                continue;
            };
            for (p, size) in if *side == Side::Yes { &b.bids } else { &b.asks } {
                let k = n.min(*size);
                let v = if *side == Side::Yes { *p } else { SCALE - p };
                liquidation += v * k as i128 - self.policy.fee(v, k)?;
                n -= k;
                if n == 0 {
                    break;
                }
            }
            if n > 0 {
                known = false
            }
        }
        let unresolved = self.orders.iter().filter(|q| !q.retired).count();
        let pnl = self.wallet.cash + liquidation - self.wallet.initial;
        Ok(
            json!({"bot":"Moloch","runtime":"rust","at":self.last_string,"policyHash":hash(&self.policy.raw),"events":self.events,"cash":money(self.wallet.cash),"realized":money(self.wallet.realized),"fees":money(self.wallet.fees),"ownedCost":money(self.wallet.owned_cost()),"reservedCash":money(self.wallet.reserved()?),"netLiquidationPnl":if known{json!(money(pnl))}else{Value::Null},"positions":self.wallet.positions(),"fills":self.fills.len(),"orders":self.orders.len(),"activeOrders":self.orders.iter().filter(|q|!q.retired&&self.last.is_some_and(|t|q.end()>t/1_000_000)).count(),"unresolvedOrders":unresolved,"fillModel":if self.policy.conditional{"conditionalFifo"}else{"strictThrough"},"conditionalQueueFills":self.fills.iter().filter(|f|f["conditionalQueueFill"]==true).count(),"profitSignalEligible":!self.policy.conditional&&unresolved==0&&known&&self.wallet.realized>0&&pnl>0,"counts":self.counts,"contestOrders":0,"networkRequests":0,"profitabilityEstablished":false}),
        )
    }
    pub fn fills(&self) -> &[Value] {
        &self.fills
    }
}
