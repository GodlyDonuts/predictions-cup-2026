//! Causal paper execution. A trade-through is a counterfactual fill, never an
//! actual exchange acknowledgment. Cancellation alone never frees cash.
#[path = "engine_live.rs"]
mod live;
#[path = "engine_packages.rs"]
mod packages;
#[path = "engine_collateral.rs"]
mod collateral;
#[path = "reviewed_inventory_exit.rs"]
mod reviewed_inventory_exit;
#[path = "reviewed_fundamental_entry.rs"]
mod reviewed_fundamental_entry;
#[path = "reviewed_rank_campaign.rs"]
mod reviewed_rank_campaign;
#[path = "ordinary_entry_exclusions.rs"]
mod ordinary_entry_exclusions;
#[path = "live_market_capacity.rs"]
mod live_market_capacity;
#[path = "engine_capital.rs"]
mod capital;
use crate::{Reservation, Result, SCALE, Side, TICK, Wallet, amount, fee, money, price};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
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
            "inventoryTakerAfterMs",
            "maximumInventoryAgeMs",
            "inventoryMinimumGain",
            "inventoryExitMode",
            "inventoryAgeClock",
            "inventoryExitDepthBps",
            "retainProfitableHedge",
            "repriceProfitableHedge",
            "maximumHedgeOvershoot",
            "throughFillCapacity",
            "minimumDepthImbalanceBps",
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
        for key in ["inventoryTakerAfterMs", "maximumInventoryAgeMs"] {
            if raw
                .get(key)
                .is_some_and(|v| !v.as_u64().is_some_and(|n| n <= 86400000))
            {
                return Err("INVENTORY_AGE_POLICY");
            }
        }
        for key in [
            "retainProfitableHedge",
            "repriceProfitableHedge",
            "throughFillCapacity",
        ] {
            if raw.get(key).is_some_and(|v| !v.is_boolean()) {
                return Err("POLICY_BOOL");
            }
        }
        if raw["repriceProfitableHedge"] == true && raw["retainProfitableHedge"] != true
            || raw["throughFillCapacity"] == true && p.conditional
        {
            return Err("HEDGE_OR_CAPACITY_POLICY");
        }
        if raw
            .get("maximumHedgeOvershoot")
            .is_some_and(|v| !v.as_u64().is_some_and(|n| n <= p.quantity))
        {
            return Err("HEDGE_OVERSHOOT");
        }
        if let Some(v) = raw.get("inventoryMinimumGain") {
            let x = dec(v)?;
            if x < 0 || x >= SCALE {
                return Err("INVENTORY_GAIN");
            }
        }
        if let Some(v) = raw.get("inventoryExitMode") {
            if v != "join"
                && (v != "fixedGain"
                    || !raw["maximumInventoryAgeMs"].as_u64().is_some_and(|n| n > 0))
            {
                return Err("INVENTORY_EXIT_MODE");
            }
        }
        if raw
            .get("inventoryAgeClock")
            .is_some_and(|v| v != "receipt" && v != "execution")
        {
            return Err("INVENTORY_AGE_CLOCK");
        }
        if raw
            .get("inventoryExitDepthBps")
            .is_some_and(|v| !v.as_u64().is_some_and(|n| n > 0 && n <= 10000))
        {
            return Err("INVENTORY_EXIT_DEPTH");
        }
        if raw.get("minimumDepthImbalanceBps").is_some_and(|v| {
            p.quote != "microprice" || !v.as_u64().is_some_and(|n| n > 0 && n <= 10000)
        }) {
            return Err("DEPTH_IMBALANCE");
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
    aon: bool,
    age_reason: bool,
    through: bool,
    participation: u64,
    conditional: bool,
    touch_bps: u64,
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
    book: Arc<Book>,
    side: Side,
    buy: bool,
    quantity: u64,
    limit: i128,
    rank: f64,
    aon: bool,
    age_reason: bool,
    hedge_for: Option<Side>,
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
struct InheritedInventory {
    receipt_hash: String,
    adopted_at: Value,
    opening_cost: i128,
    fresh_market_limit: usize,
    source_positions: Value,
    remaining: BTreeMap<(String, Side), std::collections::VecDeque<crate::Lot>>,
    realized_gross: i128,
}
#[derive(Clone)]
pub struct Engine {
    live_mode: bool,
    live_orders: BTreeMap<u64,String>,
    live_fill_ids: BTreeMap<String,String>,
    inherited: Option<InheritedInventory>,
    collateral: Option<collateral::Ledger>,
    daily_entry_windows: Option<Value>,
    live_packages: BTreeMap<String, packages::LivePackage>,
    package_orders: BTreeMap<u64,String>,
    reviewed_inventory_exits: BTreeMap<u64,Value>,
    reviewed_fundamental_entries: BTreeMap<u64,Value>,
    reviewed_rank_campaigns: BTreeMap<u64,Value>,
    ordinary_entry_exclusions: Option<Value>,
    profitable_depth_mode: Option<Value>,
    managed_inventory_planning: Option<Value>,
    live_market_capacity: Option<Value>,
    capital_allocation: Option<Value>,
    capital_mandate_revision: Option<Value>,
    policy: Policy,
    pub wallet: Wallet,
    books: BTreeMap<String, Arc<Book>>,
    orders: Vec<Order>,
    // Orders are append-only and retirement is permanent. This derived cursor
    // skips only the fully retired leading prefix; no history is removed.
    retired_prefix: usize,
    fills: Vec<Value>,
    seen: BTreeMap<String, (String, i64, i128, u64)>,
    flow: BTreeMap<String, Vec<(i64, u64)>>,
    writes: Vec<i64>,
    last: Option<i64>,
    last_string: Value,
    events: u64,
    counts: BTreeMap<String, u64>,
    entry_flow: BTreeMap<String, EntryFlow>,
    aon_consumed: BTreeMap<(String, String, Side, u128), u128>,
    prior_orders: u64,
    legacy_seen: std::sync::Arc<BTreeMap<String, String>>,
    baseline: Option<Value>,
}
impl Engine {
    pub fn from_seed_state(state: &Value) -> Result<Self> {
        if state.get("ownerCapitalAllocation").is_some()||state.get("ownerMandateRevision").is_some(){return Err("SEED_OWNER_CAPITAL_REQUIRE_REPLAY")}
        if state.get("liveMarketCapacity").is_some(){return Err("SEED_LIVE_MARKET_CAPACITY_REQUIRE_REPLAY")}
        if state.get("profitableDepthMode").is_some(){return Err("SEED_PROFITABLE_DEPTH_REQUIRE_REPLAY")}
        if state.get("ordinaryEntryExclusions").is_some(){return Err("SEED_ORDINARY_ENTRY_EXCLUSIONS_REQUIRE_REPLAY")}
        if state.get("reviewedInventoryExitReservations").is_some(){return Err("SEED_REVIEWED_EXIT_REQUIRE_REPLAY")}
        if state.get("reviewedFundamentalEntryReservations").is_some(){return Err("SEED_REVIEWED_ENTRY_REQUIRE_REPLAY")}
        if state.get("collateralAccounting").is_some_and(|v|!v.is_null()){return Err("SEED_ACTUAL_COLLATERAL_REQUIRE_REPLAY")}
        if state.get("livePackages").is_some_and(|v|!v.is_null()) {
            return Err("SEED_LIVE_PACKAGES_REQUIRE_REPLAY");
        }
        // A standalone continuation cannot silently discard a live mandate.
        // Durable live windows are restored by replaying their original events.
        if state.get("dailyEntryWindows").is_some_and(|v|!v.is_null()) {
            return Err("SEED_LIVE_ENTRY_WINDOWS_REQUIRE_REPLAY");
        }
        if state["policyHash"].as_str() != Some(hash(&state["policy"]).as_str()) {
            return Err("SEED_POLICY_HASH");
        }
        let orders = state["orders"].as_array().ok_or("SEED_ORDERS")?;
        if orders.iter().any(|q| q["retired"] != true) {
            return Err("SEED_UNRESOLVED_ORDERS");
        }
        if state["nextOrderId"].as_u64() != Some(orders.len() as u64 + 1) {
            return Err("SEED_ORDER_IDS");
        }
        let mut engine = Self::new(&state["policy"])?;
        engine.wallet = Wallet::restore_record(
            state,
            engine.policy.committed,
            engine.policy.fee_bps,
            engine.policy.fee_share,
        )?;
        engine.fills = state["fills"].as_array().ok_or("SEED_FILLS")?.clone();
        engine.prior_orders = orders.len() as u64;
        engine.last = state
            .get("lastAt")
            .filter(|v| !v.is_null())
            .map(ns)
            .transpose()?;
        engine.last_string = state["lastAt"].clone();
        engine.events = state["events"].as_u64().ok_or("SEED_EVENTS")?;
        for (k, v) in state["counts"].as_object().ok_or("SEED_COUNTS")? {
            engine
                .counts
                .insert(k.clone(), v.as_u64().ok_or("SEED_COUNT")?);
        }
        for (k, v) in state["seen"].as_object().ok_or("SEED_SEEN")? {
            let h = v
                .as_str()
                .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or("SEED_TRADE_HASH")?;
            std::sync::Arc::make_mut(&mut engine.legacy_seen).insert(k.clone(), h.into());
        }
        for v in state["writes"].as_array().ok_or("SEED_WRITES")? {
            engine.writes.push(v.as_i64().ok_or("SEED_WRITE")?);
        }
        engine.baseline = Some(
            json!({"mode":"verified_financial_continuation","cash":state["cash"],"realized":state["realized"],"fees":state["fees"],"priorRetiredOrders":orders.len(),"priorFills":engine.fills.len(),"at":state["lastAt"],"sourcePolicyHash":state["policyHash"],"booksRequireFreshResync":true}),
        );
        engine.audit()?;
        Ok(engine)
    }
    pub fn update_policy(&mut self, overrides: &Value) -> Result<()> {
        let policy = Policy::new(overrides)?;
        if self.live_mode && (policy.conditional || policy.raw["throughFillCapacity"]==true) {
            return Err("LIVE_COUNTERFACTUAL_FILL_MODEL");
        }
        if policy.initial != self.policy.initial {
            return Err("INITIAL_CAPITAL_CHANGED");
        }
        let mut next = self.clone();
        next.wallet.maximum_committed = policy.committed;
        next.wallet.fee_bps = policy.fee_bps;
        next.wallet.fee_per_share = policy.fee_share;
        next.policy = policy;
        next.audit()?;
        *self = next;
        Ok(())
    }
    fn advance_retired_prefix(&mut self) {
        while self.orders.get(self.retired_prefix).is_some_and(|q|q.retired) {
            self.retired_prefix += 1;
        }
        debug_assert!(self.retired_prefix <= self.orders.len());
        debug_assert!(self.orders[..self.retired_prefix].iter().all(|q|q.retired));
    }
    fn audit(&self) -> Result<()> {
        self.wallet.audit()?;
        self.audit_packages()?;
        self.audit_collateral()?;
        self.audit_owner_capital()?;
        // Temporary exact sums only: no persisted state, planning change, or
        // weaker audit. Retired orders are excluded; expired/cancel-pending
        // unretired orders count exactly as pledge()/opposite_buys() did.
        // Only audited wallet keys participate. Error sentinels defer range
        // rejection to the original wallet-key iteration order, so an earlier
        // shortage is never hidden by a later scope's malformed order.
        let mut active:BTreeMap<(&str,Side),(Option<u64>,Option<u64>)>=self.wallet.lots.keys()
            .map(|(ex,side)|((ex.as_str(),*side),(Some(0),Some(0)))).collect();
        for q in self.orders[self.retired_prefix..].iter().filter(|q|!q.retired){
            let key=(q.exchange.as_str(),if q.buy{q.side.other()}else{q.side});
            let Some(sums)=active.get_mut(&key)else{continue};
            let n=q.quantity.checked_sub(q.filled);
            let sum=if q.buy{&mut sums.1}else{&mut sums.0};
            *sum=sum.and_then(|total|n.and_then(|n|total.checked_add(n)));
        }
        for (ex, side) in self.wallet.lots.keys() {
            let held = self.wallet.quantity(ex, *side);
            let sums=active.get(&(ex.as_str(),*side)).ok_or("SHARE_PLEDGE_RANGE")?;
            let sells=sums.0.ok_or("SHARE_PLEDGE_RANGE")?;
            let opposite=sums.1.ok_or("SHARE_PLEDGE_RANGE")?;
            if held < sells.checked_add(held.min(opposite)).ok_or("SHARE_PLEDGE_RANGE")? {
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
            retired_prefix: 0,
            fills: Vec::new(),
            seen: BTreeMap::new(),
            flow: BTreeMap::new(),
            writes: Vec::new(),
            last: None,
            last_string: Value::Null,
            events: 0,
            counts: BTreeMap::new(),
            entry_flow: BTreeMap::new(),
            aon_consumed: BTreeMap::new(),
            prior_orders: 0,
            legacy_seen: std::sync::Arc::new(BTreeMap::new()),
            baseline: None,
            live_mode: false,
            live_orders: BTreeMap::new(),
            live_fill_ids: BTreeMap::new(),
            inherited: None,
            collateral: None,
            daily_entry_windows: None,
            live_packages: BTreeMap::new(),
            package_orders: BTreeMap::new(),
            reviewed_inventory_exits: BTreeMap::new(),
            reviewed_fundamental_entries: BTreeMap::new(),
            reviewed_rank_campaigns: BTreeMap::new(),
            ordinary_entry_exclusions: None,
            profitable_depth_mode: None,
            managed_inventory_planning: None,
            live_market_capacity: None,
            capital_allocation: None,
            capital_mandate_revision: None,
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
    fn retainable_hedge(&self, q: &Order, at: i64) -> Result<bool> {
        if self.policy.raw["retainProfitableHedge"] != true
            || !self.policy.entry(&q.exchange, at)
            || !q.buy
            || q.remaining() == 0
            || q.cancel.is_some()
        {
            return Ok(false);
        }
        let side = q.side.other();
        let held = self.wallet.quantity(&q.exchange, side);
        if held == 0
            || self.opposite_buys(&q.exchange, side) + self.pledge(&q.exchange, side)
                > held
                    + self.policy.raw["maximumHedgeOvershoot"]
                        .as_u64()
                        .unwrap_or(0)
        {
            return Ok(false);
        }
        let worst = self
            .wallet
            .lots
            .get(&(q.exchange.clone(), side))
            .into_iter()
            .flatten()
            .map(|l| l.price)
            .max()
            .unwrap_or(0);
        Ok(
            SCALE - worst - q.limit - self.policy.fee(worst, 1)? - self.policy.fee(q.limit, 1)?
                >= self.policy.edge,
        )
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
    fn fill(
        &mut self,
        i: usize,
        n: u64,
        at: &Value,
        source: &Value,
        executed: &Value,
    ) -> Result<()> {
        let q = &self.orders[i];
        if q.buy {
            self.wallet.buy_fill(&q.id.to_string(), n)?;
            if self.policy.raw["inventoryAgeClock"] == "execution" {
                if let Some(lot) = self
                    .wallet
                    .lots
                    .get_mut(&(q.exchange.clone(), q.side))
                    .and_then(|lots| lots.back_mut())
                {
                    lot.executed_at = Some(executed.clone());
                }
            }
        } else {
            self.wallet.sell_fill(&q.exchange, q.side, n, q.limit)?
        }
        self.fills.push(json!({"orderId":q.id,"marketId":q.market,"exchangeId":q.exchange,"side":label(q.side),"action":if q.buy{"buy"}else{"sell"},"quantity":n,"price":money(q.limit),"fee":money(self.policy.fee(q.limit,n)?),"at":at,"sourceId":source,"counterfactual":true}));
        if self.policy.raw["inventoryAgeClock"] == "execution" {
            self.fills.last_mut().unwrap()["executedAt"] = executed.clone();
        }
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
            if self.package_protects_exchange(&b.exchange) {continue;}
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
            // A reviewed resting BUY owns this flat scope's entry selection.
            // Filled inventory still uses normal exit generation, with runtime
            // review authority controlling every physical disposition.
            if (!self.reviewed_fundamental_entries.is_empty()||!self.reviewed_rank_campaigns.is_empty())&&yes==0&&no==0&&self.orders[self.retired_prefix..].iter().any(|q|q.exchange==b.exchange&&!q.retired&&(self.reviewed_fundamental_entries.contains_key(&q.id)||self.reviewed_rank_campaigns.contains_key(&q.id))){continue;}
            if yes > 0 || no > 0 {
                if self.managed_inventory_planning_excludes(&b.exchange){continue;}
                if p.raw["repriceProfitableHedge"] != true {
                    for q in &self.orders[self.retired_prefix..] {
                        if !q.retired
                            && q.exchange == b.exchange
                            && q.end() > at
                            && self.retainable_hedge(q, at)?
                        {
                            out.push(Target {
                                book: b.clone(),
                                side: q.side,
                                buy: true,
                                quantity: q.remaining(),
                                limit: q.limit,
                                rank: 1e12,
                                aon: false,
                                age_reason: false,
                                hedge_for: None,
                            });
                        }
                    }
                }
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
                let available = self.wallet.quantity(&b.exchange, side) as i128
                    - self.pledge(&b.exchange, side) as i128
                    - self.opposite_buys(&b.exchange, side) as i128;
                let n = (p.quantity as i128).min((available + existing as i128).max(0)) as u64;
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
                let executable = if side == Side::Yes {
                    best_bid
                } else {
                    SCALE - best_ask
                };
                let last_buy = if p.raw.get("inventoryTakerAfterMs").is_some()
                    || p.raw.get("maximumInventoryAgeMs").is_some()
                {
                    self.fills.iter().rev().find(|f| {
                        f["exchangeId"] == b.exchange
                            && f["side"] == label(side)
                            && f["action"] == "buy"
                    })
                } else {
                    None
                };
                let inherited_birth = self.inherited.as_ref().filter(|i| i.remaining.get(&(b.exchange.clone(), side)).is_some_and(|lots| !lots.is_empty())).map(|i| ms(&i.adopted_at)).transpose()?;
                let birth = if let Some(fill) = last_buy {
                    if p.raw["inventoryAgeClock"] == "execution" {
                        let mut oldest = None;
                        for lot in self
                            .wallet
                            .lots
                            .get(&(b.exchange.clone(), side))
                            .into_iter()
                            .flatten()
                        {
                            let t = if let Some(executed) = &lot.executed_at { ms(executed)? } else { inherited_birth.ok_or("INVENTORY_EXECUTION_CLOCK_MISSING")? };
                            oldest = Some(oldest.map_or(t, |old: i64| old.min(t)));
                        }
                        oldest
                    } else {
                        {let receipt_birth=ms(&fill["at"])?;Some(inherited_birth.map_or(receipt_birth, |birth| birth.min(receipt_birth)))}
                    }
                } else {
                    inherited_birth
                };
                // Opening holdings have no new-entry loss timer. Only an
                // actually owned post-adoption BUY lot can start that timer.
                let maximum_age_birth = if let Some(adopted_at) = inherited_birth {
                    if p.raw["inventoryAgeClock"] == "execution" {
                        let mut oldest_new = None;
                        for lot in self.wallet.lots.get(&(b.exchange.clone(),side)).into_iter().flatten() {
                            if let Some(executed) = &lot.executed_at {
                                let t = ms(executed)?;
                                if t >= adopted_at { oldest_new = Some(oldest_new.map_or(t,|old:i64|old.min(t))); }
                            }
                        }
                        oldest_new
                    } else if let Some(fill) = last_buy {
                        let t = ms(&fill["at"])?;
                        if t >= adopted_at {Some(t)} else {None}
                    } else {None}
                } else {birth};
                let maximum_age = maximum_age_birth.is_some_and(|t| {
                    p.raw["maximumInventoryAgeMs"]
                        .as_u64()
                        .is_some_and(|age| at - t >= age as i64)
                });
                let profitable_age = if let Some(birth) = birth {
                    p.raw["inventoryTakerAfterMs"]
                        .as_u64()
                        .is_some_and(|age| at - birth >= age as i64)
                        && executable - worst - p.fee(worst, 1)? - p.fee(executable, 1)?
                            >= p.raw
                                .get("inventoryMinimumGain")
                                .map(dec)
                                .transpose()?
                                .unwrap_or(0)
                } else {
                    false
                };
                // Opt-in affects only future profitable exits. An insufficient
                // profitable curve leaves the original maximum-age loss path intact.
                if n > 0 && profitable_age && self.profitable_depth_enabled()
                    && !self.ordinary_entry_excluded(&b.exchange) {
                    if let Some((quantity, limit)) = self.profitable_depth_target(b, side, n, worst)? {
                        out.push(Target { book: b.clone(), side, buy: false, quantity,
                            limit, rank: 1e12, aon: true, age_reason: maximum_age, hedge_for: None });
                        continue;
                    }
                }
                if n > 0 && (maximum_age || profitable_age) {
                    let quantity = if let Some(bps) = p.raw["inventoryExitDepthBps"].as_u64() {
                        n.min(
                            ((if side == Side::Yes {
                                b.bids[0].1
                            } else {
                                b.asks[0].1
                            }) as u128
                                * bps as u128
                                / 10000) as u64,
                        )
                    } else {
                        n
                    };
                    if quantity > 0 {
                        out.push(Target {
                            book: b.clone(),
                            side,
                            buy: false,
                            quantity,
                            limit: executable,
                            rank: 1e12,
                            aon: true,
                            age_reason: maximum_age,
                            hedge_for: None,
                        });
                    }
                    continue;
                }
                if p.raw["repriceProfitableHedge"] == true
                    && !maximum_age
                    && !profitable_age
                    && p.entry(&b.exchange, at)
                {
                    let hedge_side = side.other();
                    let limit = if side == Side::Yes {
                        SCALE - best_ask
                    } else {
                        best_bid
                    };
                    let existing_hedge: u64 = self
                        .orders
                        .iter()
                        .filter(|q| {
                            !q.retired
                                && q.exchange == b.exchange
                                && q.side == hedge_side
                                && q.buy
                                && q.end() > at
                        })
                        .map(Order::remaining)
                        .sum();
                    let quantity = (p.quantity as i128)
                        .min((available + existing_hedge as i128).max(0))
                        as u64;
                    if quantity > 0
                        && SCALE - worst - limit - p.fee(worst, 1)? - p.fee(limit, 1)? >= p.edge
                    {
                        out.push(Target {
                            book: b.clone(),
                            side: hedge_side,
                            buy: true,
                            quantity,
                            limit,
                            rank: 1e12,
                            aon: false,
                            age_reason: false,
                            hedge_for: Some(side),
                        });
                        continue;
                    }
                }
                while improve > 0
                    && base - improve - worst - p.fee(worst, 1)? - p.fee(base - improve, 1)?
                        < p.edge
                {
                    improve -= TICK;
                }
                let mut exit_price = base - improve;
                if p.raw["inventoryExitMode"] == "fixedGain" {
                    let gain = p
                        .raw
                        .get("inventoryMinimumGain")
                        .map(dec)
                        .transpose()?
                        .unwrap_or(p.edge);
                    exit_price = (worst + gain + TICK - 1) / TICK * TICK;
                    while exit_price < SCALE
                        && exit_price - worst - p.fee(worst, 1)? - p.fee(exit_price, 1)? < gain
                    {
                        exit_price += TICK;
                    }
                    if exit_price >= SCALE {
                        continue;
                    }
                }
                if n > 0 {
                    out.push(Target {
                        book: b.clone(),
                        side,
                        buy: false,
                        quantity: n,
                        limit: exit_price,
                        rank: 1e12,
                        aon: false,
                        age_reason: false,
                        hedge_for: None,
                    })
                }
                continue;
            }
            if self.ordinary_entry_excluded(&b.exchange)||!p.entry(&b.exchange, at)
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
                if let Some(bps) = p.raw["minimumDepthImbalanceBps"].as_i64() {
                    let bids: i128 = b.bids.iter().map(|(_, n)| *n as i128).sum();
                    let asks: i128 = b.asks.iter().map(|(_, n)| *n as i128).sum();
                    let signed = (bids - asks) * if side == Side::Yes { 1 } else { -1 };
                    if signed * 10000 < (bps as i128) * (bids + asks) {
                        continue;
                    }
                }
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
                    aon: false,
                    age_reason: false,
                    hedge_for: None,
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
        for i in self.retired_prefix..self.orders.len() {
            let q = &self.orders[i];
            if q.retired || q.end() <= at || q.cancel.is_some() { continue; }
            if self.is_package_order(q.id)||self.reviewed_inventory_exits.contains_key(&q.id)||self.reviewed_fundamental_entries.contains_key(&q.id)||self.reviewed_rank_campaigns.contains_key(&q.id) {continue;}
            // Planning exclusion cannot manufacture cancellation or release old liabilities.
            if self.managed_inventory_planning_excludes(&q.exchange){continue;}
            // Installing a planning exclusion cannot cancel an already physical
            // flat-scope BUY. The worker separately proves never-sent retirement;
            // original expiration/owned terminal handling remain authoritative.
            if q.buy&&self.ordinary_entry_excluded(&q.exchange)&&self.wallet.quantity(&q.exchange,Side::Yes)==0&&self.wallet.quantity(&q.exchange,Side::No)==0{continue;}
            let target = targets.iter().find(|t| t.key() == q.key());
            let book = self.books.get(&q.exchange);
            let covered = if q.buy {
                self.wallet.quantity(&q.exchange, Side::Yes)
                    + self.wallet.quantity(&q.exchange, Side::No)
                    == 0
                    || self.retainable_hedge(q, at)?
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
            if (target.is_none_or(|t| t.limit != q.limit || t.aon != q.aon) || renew)
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
        for q in &self.orders[self.retired_prefix..] {
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
        for q in &self.orders[self.retired_prefix..] {
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
                || t.aon && !batch.is_empty()
            {
                continue;
            }
            if !markets.contains(&t.book.exchange) && markets.len() >= self.policy.markets {
                continue;
            }
            if !markets.contains(&t.book.exchange) && self.inherited.as_ref().is_some_and(|opening| {
                let inherited_market = |exchange: &String| opening.remaining.iter().any(|((ex, _), lots)| ex == exchange && !lots.is_empty());
                !inherited_market(&t.book.exchange) && markets.iter().filter(|ex| !inherited_market(ex)).count() >= opening.fresh_market_limit
            }) {
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
                if let Some(side) = t.hedge_for {
                    n = n.min(self.wallet.quantity(&t.book.exchange, side).saturating_sub(
                        self.pledge(&t.book.exchange, side)
                            + self.opposite_buys(&t.book.exchange, side),
                    ));
                }
                let mate = batch
                    .iter()
                    .find(|q| q.book.exchange == t.book.exchange && q.buy && q.side != t.side);
                if let Some(q) = mate {
                    n = n.min(q.quantity)
                }
                let opposite = if self.policy.quote == "pairedSpread"
                    && mate.is_none()
                    && !self.orders[self.retired_prefix..].iter().any(|q| {
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
                && (self.orders[self.retired_prefix..].iter().any(|q| {
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
            if t.aon {
                break;
            }
        }
        if !batch.is_empty() {
            self.writes.push(at);
            for t in batch {
                let id = self.prior_orders + self.orders.len() as u64 + 1;
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
                    market: t.book.market.clone(),
                    exchange: t.book.exchange.clone(),
                    tournament: t.book.tournament.clone(),
                    side: t.side,
                    buy: t.buy,
                    quantity: t.quantity,
                    filled: 0,
                    limit: t.limit,
                    created: at,
                    activation: at + self.policy.latency,
                    expires: at
                        + if t.aon {
                            self.policy.latency + self.policy.book_age
                        } else {
                            self.policy.lifetime
                        },
                    cancel: None,
                    retired: false,
                    credit: 0,
                    barrier: None,
                    same,
                    better,
                    visible_same: same,
                    visible_better: better,
                    touch_remainder: 0,
                    aon: t.aon,
                    age_reason: t.age_reason,
                    through: self.policy.raw["throughFillCapacity"] == true,
                    participation: self.policy.participation,
                    conditional: self.policy.conditional,
                    touch_bps: self.policy.touch,
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
    /// Only for construction of an unpublished replay owner. The engine is
    /// consumed, so an apply/audit error drops the whole possibly changed state
    /// instead of exposing it to any caller. Same semantic checks as advance;
    /// live admission retains its transactional clone and rollback contract.
    pub(crate) fn advance_replay_unpublished(mut self, event: &Value) -> Result<Self> {
        self.apply(event)?;
        self.audit()?;
        Ok(self)
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
            "reviewed_inventory_exit_reserved_v1"|"reviewed_cohort_exit_reserved_v1" => self.reserve_reviewed_inventory_exit(e,at)?,
            "managed_inventory_planning_v1" => self.apply_managed_inventory_planning(e,at)?,
            "reviewed_fundamental_entry_reserved_v1" => self.reserve_reviewed_fundamental_entry(e,at)?,
            "reviewed_rank_campaign_reserved_v1" => self.reserve_reviewed_rank_campaign(e,at)?,
            "ordinary_profitable_depth_mode_v1" => self.apply_profitable_depth_mode(e,at)?,
            "ordinary_entry_exclusions_v1" => self.apply_ordinary_entry_exclusions(e,at)?,
            "live_market_capacity_v1" => self.apply_live_market_capacity(e,at)?,
            "live_capital_allocation_v1" => self.apply_owner_capital(e,at)?,
            "live_owner_mandate_revision_v1" => self.apply_owner_mandate_revision(e,at)?,
            "actual_order_ack" | "actual_fill" | "actual_order_terminal" | "expired_attempt_exposure_reconciled_v1" | "unsubmitted_intent_retired" | "actual_position_adoption" | "live_entry_window" | "actual_collateral_baseline" | "actual_collateral_execution" | "actual_collateral_settlement" | "actual_collateral_checkpoint" | "actual_collateral_ordinary_cash_sync" => self.apply_live(e,at)?,
            "live_package_reserve" | "live_package_attempt" | "live_package_no_execution" | "live_package_ack" | "live_package_unknown" | "live_package_complete" => self.apply_package(e,at)?,
            "probabilistic_portfolio_reserve_v1" | "probabilistic_portfolio_attempt_v1" | "probabilistic_portfolio_no_execution_v1" | "probabilistic_portfolio_ack_v1" | "probabilistic_portfolio_unknown_v1" | "probabilistic_portfolio_complete_v1" => self.apply_probabilistic_package(e,at)?,
            "book" => {
                let b = Book::parse(&e["book"], &e["receivedAt"])?;
                if let Some(old) = self.books.get(&b.exchange) {
                    if old.market != b.market
                        || old.tournament != b.tournament
                        || b.sequence < old.sequence
                        || b.sequence == old.sequence
                            && ns(&b.raw["sourceAt"])? < ns(&old.raw["sourceAt"])?
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
                for i in self.retired_prefix..self.orders.len() {
                    let q = &self.orders[i];
                    if self.live_mode || q.retired
                        || !q.aon
                        || q.exchange != b.exchange
                        || !b.fresh(&self.policy, at)
                        || b.clock() < q.activation
                        || b.clock() >= q.end()
                    {
                        continue;
                    }
                    let capacity: u128 = (if q.side == Side::Yes {
                        &b.bids
                    } else {
                        &b.asks
                    })
                    .iter()
                    .filter(|(p, _)| {
                        if q.side == Side::Yes {
                            *p >= q.limit
                        } else {
                            SCALE - *p >= q.limit
                        }
                    })
                    .map(|(_, n)| *n as u128)
                    .sum();
                    let key = (q.tournament.clone(), q.exchange.clone(), q.side, b.sequence);
                    let consumed = self.aon_consumed.get(&key).copied().unwrap_or(0);
                    let eligible = capacity.saturating_sub(consumed);
                    let n = if eligible >= q.remaining() as u128 {
                        q.remaining()
                    } else {
                        0
                    };
                    let reason = q.age_reason;
                    let source = json!(format!("book:{}:{}:{}", b.exchange, b.sequence, q.id));
                    if n > 0 {
                        self.aon_consumed.insert(key, consumed + n as u128);
                        self.fill(i, n, clock, &source, clock)?;
                        let f = self.fills.last_mut().unwrap();
                        f["executionStyle"] = json!("inventoryAON");
                        if reason {
                            f["inventoryExitReason"] = json!("maximumAge");
                        }
                    }
                    self.orders[i].retired = true;
                    self.count("inventoryAonEvaluations");
                }
                for q in &mut self.orders[self.retired_prefix..] {
                    if q.retired || q.exchange != b.exchange {
                        continue;
                    }
                    if q.conditional {
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
                self.books.insert(b.exchange.clone(), Arc::new(b));
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
                if !self.seen.contains_key(&key) {
                    if let Some(expected) = self.legacy_seen.get(&key) {
                        let stamp = chrono::DateTime::from_timestamp_nanos(executed)
                            .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
                        let prefix = &stamp[..19];
                        let fraction = &stamp[20..29];
                        let mut matched = false;
                        for digits in 0..=9 {
                            let t = if digits == 0 {
                                format!("{prefix}Z")
                            } else {
                                format!("{prefix}.{}Z", &fraction[..digits])
                            };
                            if ns(&json!(t))? == executed
                                && hash(
                                    &json!({"marketId":market,"executedAt":t,"yesPrice":e["yesPrice"],"quantity":quantity}),
                                ) == *expected
                            {
                                matched = true;
                                break;
                            }
                        }
                        if !matched {
                            return Err("LEGACY_TRADE_PROVENANCE_REQUIRED");
                        }
                        duplicate = true;
                    }
                }
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
                    for i in self.retired_prefix..self.orders.len() {
                        if self.live_mode { break; }
                        let q = &mut self.orders[i];
                        if q.retired
                            || q.aon
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
                        let touch = q.conditional && trade_price == limit;
                        if !through && !touch {
                            continue;
                        }
                        let mut eligible = quantity;
                        if through && q.conditional {
                            q.same = 0;
                            q.better = 0
                        }
                        if touch {
                            let numerator = quantity * q.touch_bps + q.touch_remainder;
                            let attributed = numerator / 10000;
                            q.touch_remainder = numerator % 10000;
                            if attributed > 0 {
                                q.better = 0
                            }
                            let consumed = attributed.min(q.same);
                            q.same -= consumed;
                            eligible = attributed - consumed;
                            if eligible == 0 {
                                self.count("conditionalTouches");
                                break;
                            }
                        }
                        if through && q.through {
                            let n = quantity.min(q.remaining());
                            if n > 0 {
                                self.fill(i, n, clock, &e["id"], &e["executedAt"])?;
                                self.fills.last_mut().unwrap()["throughCapacityFill"] = json!(true);
                                self.count("throughCapacityFills");
                                break;
                            }
                        }
                        q.credit += eligible * q.participation;
                        let n = quantity
                            .min(q.remaining())
                            .min(q.credit / 10000)
                            .min((quantity * q.participation / 10000).max(1));
                        if n > 0 {
                            q.credit -= n * 10000;
                            let touch_bps = q.touch_bps;
                            self.fill(i, n, clock, &e["id"], &e["executedAt"])?;
                            if touch {
                                self.fills.last_mut().unwrap()["conditionalQueueFill"] =
                                    json!(true);
                                self.fills.last_mut().unwrap()["touchAttributionBps"] =
                                    json!(touch_bps);
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
                        // Transactional Engine/Session clones and desired
                        // targets may share this immutable snapshot. Invalidate
                        // only this candidate's owned copy, never another fork.
                        Arc::make_mut(b).invalid = true
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
                for q in &mut self.orders[self.retired_prefix..] {
                    if !self.live_mode && !q.retired
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
        // This durable authorization transition changes only the entry cutoff;
        // an ordinary subsequent tick/book performs normal quote planning.
        // Only a proved ordinary unsubmitted-intent retirement may defer this
        // event's plan, so the sole owner can immediately reserve a package.
        // No persistent state is set: a subsequent tick/book plans normally.
        if kind!="unsubmitted_intent_retired"&&e.get("deferPlanningForPackage").is_some(){return Err("PACKAGE_PLANNING_FLAG_EVENT")}
        let defer_package_plan=kind=="unsubmitted_intent_retired"&&e["deferPlanningForPackage"]==true;
        if kind!="managed_inventory_planning_v1"&&kind!="reviewed_cohort_exit_reserved_v1"&&kind!="ordinary_profitable_depth_mode_v1"&&kind!="live_entry_window"&&kind!="ordinary_entry_exclusions_v1"&&kind!="live_market_capacity_v1"&&kind!="live_capital_allocation_v1"&&kind!="live_owner_mandate_revision_v1"&&kind!="reviewed_inventory_exit_reserved_v1"&&kind!="reviewed_fundamental_entry_reserved_v1"&&kind!="reviewed_rank_campaign_reserved_v1"&&!kind.starts_with("live_package_")&&!kind.starts_with("probabilistic_portfolio_")&&!kind.starts_with("actual_collateral_")&&!defer_package_plan{self.reconcile(at)?;}
        self.advance_retired_prefix();
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
        let mut report = json!({"bot":"Moloch","runtime":"rust","at":self.last_string,"policyHash":hash(&self.policy.raw),"events":self.events,"cash":money(self.wallet.cash),"realized":money(self.wallet.realized),"fees":money(self.wallet.fees),"ownedCost":money(self.wallet.owned_cost()),"reservedCash":money(self.wallet.reserved()?),"netLiquidationPnl":if known{json!(money(pnl))}else{Value::Null},"positions":self.wallet.positions(),"fills":self.fills.len(),"orders":self.prior_orders+self.orders.len() as u64,"activeOrders":self.orders.iter().filter(|q|!q.retired&&self.last.is_some_and(|t|q.end()>t/1_000_000)).count(),"unresolvedOrders":unresolved,"executionMode":if self.live_mode{"live"}else{"paper"},"fillModel":if self.live_mode{"actualAccountFills"}else if self.policy.conditional{"conditionalFifo"}else{"strictThrough"},"conditionalQueueFills":self.fills.iter().filter(|f|f["conditionalQueueFill"]==true).count(),"profitSignalEligible":!self.policy.conditional&&!self.fills.iter().any(|f|f["conditionalQueueFill"]==true||f["throughCapacityFill"]==true||f["executionStyle"]=="inventoryAON")&&unresolved==0&&known&&self.wallet.realized>0&&pnl>0,"counts":self.counts,"continuation":self.baseline,"contestOrders":if self.live_mode{self.live_orders.len()}else{0},"networkRequests":0,"profitabilityEstablished":false});
        if !self.reviewed_inventory_exits.is_empty(){report["reviewedInventoryExitReservations"]=json!(self.reviewed_inventory_exits);}
        if !self.reviewed_fundamental_entries.is_empty(){report["reviewedFundamentalEntryReservations"]=json!(self.reviewed_fundamental_entries);}
        if !self.reviewed_rank_campaigns.is_empty(){report["reviewedRankCampaignReservations"]=json!(self.reviewed_rank_campaigns);}
        if let Some(record)=&self.profitable_depth_mode{report["profitableDepthMode"]=record.clone();}
        if let Some(record)=&self.managed_inventory_planning{report["managedInventoryPlanning"]=record.clone();}
        if let Some(record)=&self.ordinary_entry_exclusions{report["ordinaryEntryExclusions"]=record.clone();}
        if let Some(record)=&self.live_market_capacity{report["liveMarketCapacity"]=record.clone();}
        if let Some(record)=&self.capital_allocation{report["ownerCapitalAllocation"]=record.clone();}
        if let Some(record)=&self.capital_mandate_revision{report["ownerMandateRevision"]=record.clone();}
        if let Some(inherited) = &self.inherited {
            report["openingInventoryCost"] = json!(money(inherited.opening_cost));
            if self.wallet.realized - inherited.realized_gross <= 0 { report["profitSignalEligible"] = json!(false); }
            report["inheritedRealizedGross"] = json!(money(inherited.realized_gross));
            report["newTradingRealized"] = json!(money(self.wallet.realized - inherited.realized_gross));
            report["inheritedInventory"] = json!({"receiptHash":inherited.receipt_hash,"managementStartedAt":inherited.adopted_at,"sourcePositions":inherited.source_positions,"adoptedExchangeIds":inherited.remaining.keys().map(|(ex,_)|ex.clone()).collect::<BTreeSet<_>>(),"historicalFillsImported":false,"openingCost":money(inherited.opening_cost),"freshMarketLimit":inherited.fresh_market_limit,"feesRemainInNewTradingRealized":true,"basis":"verified_open_lot_principal_fifo","officialAccountRealizedPnl":Value::Null});
        }
        if let Some(mandate)=&self.daily_entry_windows {
            report["dailyEntryWindows"]=json!({"mandate":mandate,"entryStopAt":self.policy.raw["entryStopAt"]});
        }
        if self.collateral.is_some(){report["collateralAccounting"]=self.collateral_report()?;report["profitSignalEligible"]=json!(false);}
        if !self.live_packages.is_empty(){report["livePackages"]=json!({"records":self.package_records(),"blocksNewSubmissions":self.package_blocks_new_submissions()});}
        Ok(report)
    }
    pub fn fills(&self) -> &[Value] {
        &self.fills
    }
    pub fn policy(&self) -> &Value {
        &self.policy.raw
    }
    pub fn recovery_from(&self, exchange: &str, now_ms: i64) -> Result<String> {
        let mut from = now_ms.checked_sub(900000).ok_or("RECOVERY_CLOCK")?;
        for q in self
            .orders
            .iter()
            .filter(|q| !q.retired && q.exchange == exchange)
        {
            from = from.min(q.activation);
        }
        if let Some(at) = self
            .baseline
            .as_ref()
            .and_then(|v| v.get("at"))
            .filter(|v| !v.is_null())
        {
            from = from.max(ms(at)?);
        }
        if from >= now_ms {
            return Err("RECOVERY_CLOCK");
        }
        Ok(chrono::DateTime::from_timestamp_millis(from)
            .ok_or("RECOVERY_CLOCK")?
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
    }
    pub fn exit_book_windows(&self) -> Vec<(String, i64, i64)> {
        self.orders
            .iter()
            .filter(|q| !q.retired && q.aon)
            .map(|q| (q.exchange.clone(), q.activation, q.end()))
            .collect()
    }
    pub fn required_exchanges(&self) -> BTreeSet<String> {
        let mut required = BTreeSet::new();
        for ((ex, _), lots) in &self.wallet.lots {
            if lots.iter().any(|l| l.quantity > 0) {
                required.insert(ex.clone());
            }
        }
        for q in &self.orders {
            if !q.retired {
                required.insert(q.exchange.clone());
            }
        }
        required
    }
    pub fn known_exchanges(&self) -> BTreeSet<String> {
        self.books
            .keys()
            .cloned()
            .chain(self.required_exchanges())
            .collect()
    }
    pub fn expired_recovery_window(
        &self,
        exchange: &str,
        now_ms: i64,
    ) -> Result<Option<(String, String)>> {
        let expired = self
            .orders
            .iter()
            .filter(|q| !q.retired && q.exchange == exchange && q.end() <= now_ms)
            .collect::<Vec<_>>();
        if expired.is_empty() {
            return Ok(None);
        }
        let from = expired
            .iter()
            .map(|q| q.activation)
            .min()
            .ok_or("RECOVERY_WINDOW")?;
        let to = expired
            .iter()
            .map(|q| q.end())
            .max()
            .ok_or("RECOVERY_WINDOW")?;
        if from >= to {
            return Err("RECOVERY_WINDOW");
        }
        let iso = |t| {
            chrono::DateTime::from_timestamp_millis(t)
                .map(|v| v.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
                .ok_or("RECOVERY_WINDOW")
        };
        Ok(Some((iso(from)?, iso(to)?)))
    }
    pub fn order_records(&self) -> Vec<Value> {
        self.orders.iter().map(|q|{let mut v=json!({"id":q.id,"marketId":q.market,"exchangeId":q.exchange,"side":label(q.side),"action":if q.buy{"buy"}else{"sell"},"quantity":q.quantity,"filled":q.filled,"price":money(q.limit),"createdAt":q.created,"activationAt":q.activation,"expiresAt":q.expires,"retired":q.retired,"participationCredit":q.credit,"barrier":q.barrier.map(|(sequence,end)|json!({"sequence":sequence.to_string(),"endAt":end}))});
        if let Some(proof)=self.reviewed_inventory_exits.get(&q.id){v["reviewedInventoryExit"]=proof.clone();}
        if let Some(proof)=self.reviewed_fundamental_entries.get(&q.id){v["reviewedFundamentalEntry"]=proof.clone();}
        if let Some(proof)=self.reviewed_rank_campaigns.get(&q.id){v["reviewedRankCampaign"]=proof.clone();}
        if let Some(at)=q.cancel{v["cancelEffectiveAt"]=json!(at)}if q.aon{v["executionStyle"]=json!("inventoryAON")}if q.age_reason{v["inventoryExitReason"]=json!("maximumAge")}if q.participation!=100{v["participationBps"]=json!(q.participation)}if q.through{v["throughFillCapacity"]=json!(true)}if q.conditional{v["fillModel"]=json!("conditionalFifo");v["touchAttributionBps"]=json!(q.touch_bps);v["queueAhead"]=json!((q.same+q.better).to_string());v["queueSame"]=json!(q.same.to_string());v["queueBetter"]=json!(q.better.to_string());v["queueVisibleSame"]=json!(q.visible_same.to_string());v["queueVisibleBetter"]=json!(q.visible_better.to_string());v["touchRemainder"]=json!(q.touch_remainder)}v}).collect()
    }
}

#[cfg(test)]mod active_pledge_audit_tests{
 use super::*;
 fn fixture()->(Engine,Order){
  let mut e=Engine::new(&json!({"initialCash":"1000.000000","maximumCommitted":"1000.000000","maximumMarketCost":"1000.000000","quantity":10,"maximumMarkets":3})).unwrap();
  e.advance(&json!({"type":"book","receivedAt":"2026-10-17T00:00:00Z","book":{"exchangeId":"101","marketId":"201","tournamentId":"cup","sequence":"1","sourceAt":"2026-10-17T00:00:00Z","receivedAt":"2026-10-17T00:00:00Z","requestStartedAt":"2026-10-17T00:00:00Z","bids":[{"price":"0.400000","quantity":100}],"asks":[{"price":"0.450000","quantity":100}]}})).unwrap();
  let q=e.orders[0].clone();e.orders.clear();e.wallet.reservations.clear();(e,q)
 }
 fn hold(e:&mut Engine,ex:&str,side:Side,n:u64){let id=format!("fixture-held-{ex}");e.wallet.reserve(&id,Reservation{exchange:ex.into(),side,quantity:n,limit:400000}).unwrap();e.wallet.buy_fill(&id,n).unwrap();}
 fn old_check(e:&Engine)->Result<()>{for(ex,side)in e.wallet.lots.keys(){let held=e.wallet.quantity(ex,*side);if held<e.pledge(ex,*side)+held.min(e.opposite_buys(ex,*side)){return Err("SHARE_PLEDGE")}}Ok(())}
 fn order(q:&Order,ex:&str,side:Side,buy:bool,n:u64,filled:u64)->Order{let mut out=q.clone();out.exchange=ex.into();out.side=side;out.buy=buy;out.quantity=n;out.filled=filled;out.retired=false;out}
 #[test]fn retired_prefix_keeps_every_record_and_all_live_share_pledges(){
  let(mut e,q)=fixture();hold(&mut e,"101",Side::Yes,10);
  for i in 0..20000{let mut old=q.clone();old.id=i+1;old.retired=true;e.orders.push(old);}
  let mut active=order(&q,"101",Side::Yes,false,10,0);active.id=20001;e.orders.push(active);
  let before=e.order_records();let report=e.report().unwrap();e.advance_retired_prefix();
  assert_eq!(e.retired_prefix,20000);assert_eq!(e.order_records(),before);assert_eq!(e.report().unwrap(),report);assert_eq!(e.audit(),old_check(&e));
  e.orders[20000].quantity=11;assert_eq!(e.audit(),Err("SHARE_PLEDGE"));
  e.orders[20000].retired=true;e.advance_retired_prefix();assert_eq!(e.retired_prefix,20001);assert!(e.audit().is_ok());
  let mut next=order(&q,"101",Side::Yes,false,11,0);next.id=20002;e.orders.push(next);
  e.advance_retired_prefix();assert_eq!(e.retired_prefix,20001);assert_eq!(e.audit(),Err("SHARE_PLEDGE"));assert_eq!(e.orders.len(),20002);
 }
 #[test]fn retired_prefix_stops_at_expired_and_cancel_pending_unretired_liabilities(){
  for canceled in [false,true]{let(mut e,q)=fixture();hold(&mut e,"101",Side::Yes,10);
   let mut old=q.clone();old.retired=true;e.orders.push(old);let mut active=order(&q,"101",Side::Yes,false,11,0);
   active.expires=0;if canceled{active.cancel=Some(0)}e.orders.push(active);
   e.advance_retired_prefix();assert_eq!(e.retired_prefix,1);assert_eq!(e.audit(),old_check(&e));assert_eq!(e.audit(),Err("SHARE_PLEDGE"));
  }
 }
 #[test]fn partial_sells_opposite_buys_and_retired_history_match_original_equation(){
  let(mut e,q)=fixture();hold(&mut e,"101",Side::Yes,10);hold(&mut e,"102",Side::No,8);
  e.orders.push(order(&q,"101",Side::Yes,false,10,3));e.orders.push(order(&q,"101",Side::No,true,5,2));
  e.orders.push(order(&q,"102",Side::No,false,6,1));e.orders.push(order(&q,"102",Side::Yes,true,4,1));
  for _ in 0..10000{let mut old=order(&q,"101",Side::Yes,false,u64::MAX,0);old.retired=true;e.orders.push(old);}
  let before=e.report().unwrap();assert!(old_check(&e).is_ok());assert!(e.audit().is_ok());assert_eq!(e.report().unwrap(),before);
  e.orders[1].quantity+=1;assert_eq!(old_check(&e),Err("SHARE_PLEDGE"));assert_eq!(e.audit(),Err("SHARE_PLEDGE"));
 }
 #[test]fn cancel_pending_and_expired_unretired_orders_remain_pledged(){
  let(mut e,q)=fixture();hold(&mut e,"101",Side::Yes,10);let mut sell=order(&q,"101",Side::Yes,false,11,0);sell.cancel=Some(0);sell.expires=0;e.orders.push(sell);
  assert_eq!(old_check(&e),Err("SHARE_PLEDGE"));assert_eq!(e.audit(),Err("SHARE_PLEDGE"));
 }
 #[test]fn irrelevant_unheld_scope_and_same_side_buy_never_create_new_audit_failure(){
  let(mut e,q)=fixture();hold(&mut e,"101",Side::Yes,10);e.orders.push(order(&q,"999",Side::Yes,false,0,1));e.orders.push(order(&q,"101",Side::Yes,true,0,1));
  assert!(old_check(&e).is_ok());assert!(e.audit().is_ok());
 }
 #[test]fn first_wallet_key_shortage_precedes_later_scope_overflow(){
  let(mut e,q)=fixture();hold(&mut e,"101",Side::Yes,10);hold(&mut e,"102",Side::Yes,1);e.orders.push(order(&q,"101",Side::Yes,false,11,0));e.orders.push(order(&q,"102",Side::Yes,false,u64::MAX,0));e.orders.push(order(&q,"102",Side::Yes,false,1,0));
  assert_eq!(old_check(&e),Err("SHARE_PLEDGE"));assert_eq!(e.audit(),Err("SHARE_PLEDGE"));
 }
 #[test]fn audited_scope_overflow_or_negative_remaining_rejects_checked_without_wrap(){
  for malformed in [false,true]{let(mut e,q)=fixture();hold(&mut e,"101",Side::Yes,10);if malformed{e.orders.push(order(&q,"101",Side::Yes,false,0,1));}else{e.orders.push(order(&q,"101",Side::Yes,false,u64::MAX,0));e.orders.push(order(&q,"101",Side::Yes,false,1,0));}assert_eq!(e.audit(),Err("SHARE_PLEDGE_RANGE"));}
 }
}
