use crate::{money::*, types::*};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};
#[derive(Clone, Debug)]
struct Lot {
    quantity: i64,
    cost: i128,
}
#[derive(Clone, Debug)]
struct Position {
    quantity: i64,
    cost: i128,
    entered_at: i64,
    lots: VecDeque<Lot>,
}
#[derive(Clone, Debug)]
struct Intent {
    id: u64,
    at: i64,
    buy: bool,
    quantity: i64,
    limit: i128,
}
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Fill {
    pub intent_id: u64,
    pub instrument: String,
    pub action: String,
    pub quantity: i64,
    pub price: String,
    pub fee: String,
    pub at: i64,
}
#[derive(Clone, Default, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub observations: u64,
    pub warmup: u64,
    pub invalid_forecast: u64,
    pub edge_below_margin: u64,
    pub edge_pass: u64,
    pub zero_top_ask_capacity: u64,
}
#[derive(Clone, Debug)]
pub struct Wallet {
    initial: i128,
    cash: i128,
    realized: i128,
    fees: i128,
    turnover: i128,
    positions: HashMap<String, Position>,
    pending: HashMap<String, Intent>,
    books: HashMap<String, Arc<Book>>,
    invalidated: HashMap<String, i64>,
    consumed: HashMap<String, (u64, i64)>,
    pub fills: Vec<Fill>,
    intents: u64,
    expired: u64,
    closed: u64,
    last_at: i64,
    pub metrics: Metrics,
    pub unsupported: HashSet<String>,
}
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub at: String,
    pub conservative_pnl: String,
    pub liquidation_pnl: Option<String>,
    pub valuation: &'static str,
    pub cash: String,
    pub realized_pnl: String,
    pub fees: String,
    pub turnover: String,
    pub completed_trades: u64,
    pub fill_executions: usize,
    pub intents: u64,
    pub expired_intents: u64,
    pub pending_intents: usize,
    pub open_shares: i64,
    pub unsold_shares: i64,
    pub signal_metrics: Option<Metrics>,
    pub unsupported_instruments: usize,
}
pub fn iso(at: i64) -> String {
    chrono::DateTime::from_timestamp_millis(at)
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
impl Wallet {
    pub fn new(c: &Config, at: i64) -> Result<Self, String> {
        let initial = amount(&c.initial_cash)?;
        Ok(Self {
            initial,
            cash: initial,
            realized: 0,
            fees: 0,
            turnover: 0,
            positions: HashMap::new(),
            pending: HashMap::new(),
            books: HashMap::new(),
            invalidated: HashMap::new(),
            consumed: HashMap::new(),
            fills: Vec::new(),
            intents: 0,
            expired: 0,
            closed: 0,
            last_at: at,
            metrics: Metrics::default(),
            unsupported: HashSet::new(),
        })
    }
    pub fn invalidate(&mut self, market: Option<&str>, at: i64) {
        for (id, b) in &self.books {
            if market.is_none_or(|m| m == b.market_id) {
                self.invalidated.insert(id.clone(), at);
            }
        }
        let remove: Vec<_> = self
            .pending
            .iter()
            .filter(|(id, p)| {
                p.buy
                    && market.is_none_or(|m| self.books.get(*id).is_some_and(|b| b.market_id == m))
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in remove {
            self.pending.remove(&id);
            self.expired += 1;
        }
    }
    fn enqueue(&mut self, b: &Book, buy: bool, quantity: i64, limit: i128) {
        self.intents += 1;
        self.pending.insert(
            b.instrument.clone(),
            Intent {
                id: self.intents,
                at: b.received_at,
                buy,
                quantity,
                limit,
            },
        );
    }
    fn fill(&mut self, p: &Intent, b: &Book, c: &Config) -> Result<(), String> {
        if b.received_at - p.at < c.latency_ms
            || b.received_at == p.at
            || self.consumed.get(&b.instrument) == Some(&(b.sequence, b.source_ns))
        {
            return Ok(());
        }
        let levels = if p.buy { &b.asks } else { &b.bids };
        let mut remaining = p.quantity;
        let mut total = 0;
        let mut exposure: i128 = self.positions.values().map(|p| p.cost).sum();
        for l in levels {
            if remaining == 0 {
                break;
            }
            let price = l.price + if p.buy { 1 } else { -1 } * TICK * c.slippage_ticks;
            if !(TICK..=SCALE - TICK).contains(&price)
                || if p.buy {
                    price > p.limit
                } else {
                    price < p.limit
                }
            {
                break;
            }
            let mut quantity =
                remaining.min((l.quantity as i128 * c.depth_fraction_bps as i128 / 10_000) as i64);
            if p.buy {
                quantity = quantity.min(
                    c.max_position_shares
                        - self.positions.get(&b.instrument).map_or(0, |p| p.quantity),
                );
                let budget = (self.cash - amount(&c.cash_reserve)?)
                    .min(amount(&c.max_exposure)? - exposure)
                    .max(0);
                let capacity = budget * 10_000 / (price * (10_000 + c.fee_bps));
                quantity = quantity.min(capacity.min(i64::MAX as i128) as i64);
            } else {
                quantity =
                    quantity.min(self.positions.get(&b.instrument).map_or(0, |p| p.quantity));
            }
            if quantity < 1 {
                continue;
            }
            let notional = price * quantity as i128;
            let charge = fee(notional, c.fee_bps);
            if p.buy {
                self.cash -= notional + charge;
                exposure += notional + charge;
                let pos = self
                    .positions
                    .entry(b.instrument.clone())
                    .or_insert(Position {
                        quantity: 0,
                        cost: 0,
                        entered_at: b.received_at,
                        lots: VecDeque::new(),
                    });
                pos.quantity += quantity;
                pos.cost += notional + charge;
                pos.lots.push_back(Lot {
                    quantity,
                    cost: notional + charge,
                });
            } else {
                let pos = self.positions.get_mut(&b.instrument).ok_or("OVERSELL")?;
                let mut left = quantity;
                let mut basis = 0;
                while left > 0 {
                    let lot = pos.lots.front_mut().ok_or("FIFO_OVERSELL")?;
                    let n = left.min(lot.quantity);
                    let used = if n == lot.quantity {
                        lot.cost
                    } else {
                        lot.cost * n as i128 / lot.quantity as i128
                    };
                    basis += used;
                    lot.cost -= used;
                    lot.quantity -= n;
                    left -= n;
                    if lot.quantity == 0 {
                        pos.lots.pop_front();
                    }
                }
                pos.cost -= basis;
                pos.quantity -= quantity;
                self.realized += notional - charge - basis;
                self.cash += notional - charge;
                if pos.quantity == 0 {
                    self.positions.remove(&b.instrument);
                    self.closed += 1;
                }
            }
            self.fees += charge;
            self.turnover += notional;
            self.fills.push(Fill {
                intent_id: p.id,
                instrument: b.instrument.clone(),
                action: if p.buy { "buy" } else { "sell" }.into(),
                quantity,
                price: money(price),
                fee: money(charge),
                at: b.received_at,
            });
            remaining -= quantity;
            total += quantity;
        }
        if total > 0 {
            self.consumed
                .insert(b.instrument.clone(), (b.sequence, b.source_ns));
            self.pending.remove(&b.instrument);
        }
        Ok(())
    }
    pub fn step(
        &mut self,
        b: Arc<Book>,
        c: &Config,
        net_edge: Option<f64>,
        horizon_ms: i64,
        margin: f64,
    ) -> Result<bool, String> {
        self.step_inner(b, c, net_edge, horizon_ms, margin, None)
    }
    pub fn step_dip(
        &mut self,
        b: Arc<Book>,
        c: &Config,
        net_edge: Option<f64>,
        horizon_ms: i64,
        margin: f64,
        take_profit: i128,
    ) -> Result<bool, String> {
        if take_profit <= 0 {
            return Err("INVALID_TAKE_PROFIT".into());
        }
        self.step_inner(b, c, net_edge, horizon_ms, margin, Some(take_profit))
    }
    fn step_inner(
        &mut self,
        b: Arc<Book>,
        c: &Config,
        net_edge: Option<f64>,
        horizon_ms: i64,
        margin: f64,
        take_profit: Option<i128>,
    ) -> Result<bool, String> {
        if horizon_ms < 1000 || !margin.is_finite() || margin <= 0.0 {
            return Err("INVALID_FORECAST_POLICY".into());
        }
        if b.received_at < self.last_at {
            return Err("OBSERVATION_TIME_MOVED_BACKWARD".into());
        }
        let previous = self.books.get(&b.instrument);
        let same = previous.is_some_and(|p| p.same_version(&b));
        if let Some(p) = previous {
            if b.sequence < p.sequence || b.source_ns < p.source_ns {
                return Err("BOOK_VERSION_ROLLBACK".into());
            }
            if same && (b.bids != p.bids || b.asks != p.asks) {
                return Err("CONFLICTING_EQUAL_BOOK_VERSION".into());
            }
        }
        if self
            .invalidated
            .get(&b.instrument)
            .is_some_and(|t| b.request_started_at <= *t)
        {
            return Err("PRE_INVALIDATION_SNAPSHOT".into());
        }
        self.invalidated.remove(&b.instrument);
        self.books.insert(b.instrument.clone(), b.clone());
        self.last_at = b.received_at;
        if same {
            return Ok(false);
        }
        if let Some(p) = self.pending.get(&b.instrument).cloned() {
            if b.received_at - p.at > c.max_data_age_ms + c.cycle_interval_ms * 4 {
                self.pending.remove(&b.instrument);
                self.expired += 1;
            } else {
                self.fill(&p, &b, c)?;
            }
        }
        if !self.pending.contains_key(&b.instrument) {
            if let Some(pos) = self.positions.get(&b.instrument) {
                let executable = b.bids[0].price - TICK * c.slippage_ticks;
                let gross = executable * pos.quantity as i128;
                let profitable = take_profit.is_some_and(|edge| {
                    gross - fee(gross, c.fee_bps) - pos.cost >= edge * pos.quantity as i128
                });
                if profitable
                    || b.received_at - pos.entered_at >= horizon_ms
                    || pos.cost / pos.quantity as i128
                        - if take_profit.is_some() {
                            executable
                        } else {
                            b.bids[0].price
                        }
                        >= amount(&c.stop_loss)?
                {
                    self.enqueue(&b, false, pos.quantity, executable);
                }
            } else if net_edge.is_some_and(|x| x.is_finite() && x >= margin) {
                self.enqueue(
                    &b,
                    true,
                    c.order_shares,
                    b.asks[0].price + TICK * c.slippage_ticks,
                );
            }
        }
        Ok(true)
    }
    pub fn point(&self, c: &Config, at: i64) -> Result<Point, String> {
        let mut liquidation = self.cash;
        let mut complete = true;
        let mut unsold = 0;
        let mut shares = 0;
        for (id, p) in &self.positions {
            shares += p.quantity;
            let mut left = p.quantity;
            if let Some(b) = self.books.get(id) {
                let age = at - b.received_at;
                if !self.invalidated.contains_key(id) && age >= 0 && age <= c.max_data_age_ms {
                    for l in &b.bids {
                        let q = left.min(
                            (l.quantity as i128 * c.depth_fraction_bps as i128 / 10_000) as i64,
                        );
                        if q > 0 {
                            let price = l.price - TICK * c.slippage_ticks;
                            if price < TICK {
                                break;
                            }
                            let notional = price * q as i128;
                            liquidation += notional - fee(notional, c.fee_bps);
                            left -= q;
                        }
                        if left == 0 {
                            break;
                        }
                    }
                }
            }
            if left > 0 {
                complete = false;
            }
            unsold += left;
        }
        let value = if complete { liquidation } else { self.cash };
        Ok(Point {
            at: iso(at),
            conservative_pnl: money(value - self.initial),
            liquidation_pnl: if complete {
                Some(money(value - self.initial))
            } else {
                None
            },
            valuation: if complete {
                "depth_checked_liquidation_estimate"
            } else {
                "cash_only_inventory_zero"
            },
            cash: money(self.cash),
            realized_pnl: money(self.realized),
            fees: money(self.fees),
            turnover: money(self.turnover),
            completed_trades: self.closed,
            fill_executions: self.fills.len(),
            intents: self.intents,
            expired_intents: self.expired,
            pending_intents: self.pending.len(),
            open_shares: shares,
            unsold_shares: unsold,
            signal_metrics: if self.metrics.observations > 0 {
                Some(self.metrics.clone())
            } else {
                None
            },
            unsupported_instruments: self.unsupported.len(),
        })
    }
    pub fn verify(&self) -> Result<(), String> {
        let mut cash = self.initial;
        let mut fees = 0;
        let mut turnover = 0;
        let mut realized = 0;
        let mut qty: HashMap<&str, i64> = HashMap::new();
        let mut lots: HashMap<&str, VecDeque<Lot>> = HashMap::new();
        for f in &self.fills {
            let notional = amount(&f.price)? * f.quantity as i128;
            let charge = amount(&f.fee)?;
            cash += if f.action == "buy" {
                -notional - charge
            } else {
                notional - charge
            };
            fees += charge;
            turnover += notional;
            *qty.entry(&f.instrument).or_default() += if f.action == "buy" {
                f.quantity
            } else {
                -f.quantity
            };
            let ledger = lots.entry(&f.instrument).or_default();
            if f.action == "buy" {
                ledger.push_back(Lot {
                    quantity: f.quantity,
                    cost: notional + charge,
                });
            } else {
                let mut left = f.quantity;
                let mut basis = 0;
                while left > 0 {
                    let l = ledger.front_mut().ok_or("LEDGER_OVERSELL")?;
                    let q = left.min(l.quantity);
                    let used = if q == l.quantity {
                        l.cost
                    } else {
                        l.cost * q as i128 / l.quantity as i128
                    };
                    basis += used;
                    l.cost -= used;
                    l.quantity -= q;
                    left -= q;
                    if l.quantity == 0 {
                        ledger.pop_front();
                    }
                }
                realized += notional - charge - basis;
            }
        }
        if cash != self.cash || cash < 0 || fees != self.fees || turnover != self.turnover {
            return Err("CASH_CONSERVATION_FAILED".into());
        }
        if realized != self.realized {
            return Err("REALIZED_BASIS_CONSERVATION_FAILED".into());
        }
        for (id, n) in qty {
            if n < 0 || n != self.positions.get(id).map_or(0, |p| p.quantity) {
                return Err("INVENTORY_CONSERVATION_FAILED".into());
            }
        }
        for (id, p) in &self.positions {
            let ledger = lots.get(id.as_str()).ok_or("FIFO_CONSERVATION_FAILED")?;
            if p.cost != ledger.iter().map(|l| l.cost).sum::<i128>()
                || p.quantity != ledger.iter().map(|l| l.quantity).sum::<i64>()
                || p.lots
                    .iter()
                    .zip(ledger)
                    .any(|(a, b)| a.quantity != b.quantity || a.cost != b.cost)
                || p.lots.len() != ledger.len()
            {
                return Err("FIFO_CONSERVATION_FAILED".into());
            }
        }
        Ok(())
    }
}
