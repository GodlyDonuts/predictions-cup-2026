//! Exact paper accounting shared by replay and the future live strategy loop.
//! No networking or contest order submission is performed by this library.
use std::collections::{BTreeMap, VecDeque};
pub mod engine;
pub mod journal;
pub mod legacy;
pub mod realtime;
pub mod session;
pub mod source;
pub const SCALE: i128 = 1_000_000;
pub const TICK: i128 = 5_000;
pub type Result<T> = std::result::Result<T, &'static str>;
pub fn amount(s: &str) -> Result<i128> {
    let (sign, s) = if let Some(x) = s.strip_prefix('-') {
        (-1, x)
    } else {
        (1, s)
    };
    let mut parts = s.split('.');
    let whole = parts.next().ok_or("MONEY")?;
    let fraction = parts.next();
    if parts.next().is_some()
        || whole.is_empty()
        || whole.len() > 1 && whole.starts_with('0')
        || !whole.bytes().all(|c| c.is_ascii_digit())
    {
        return Err("MONEY");
    }
    let mut value = whole
        .parse::<i128>()
        .map_err(|_| "MONEY_RANGE")?
        .checked_mul(SCALE)
        .ok_or("MONEY_RANGE")?;
    if let Some(f) = fraction {
        if f.is_empty() || f.len() > 6 || !f.bytes().all(|c| c.is_ascii_digit()) {
            return Err("MONEY");
        }
        value = value
            .checked_add(f.parse::<i128>().map_err(|_| "MONEY")? * 10i128.pow(6 - f.len() as u32))
            .ok_or("MONEY_RANGE")?;
    }
    value.checked_mul(sign).ok_or("MONEY_RANGE")
}
pub fn money(v: i128) -> String {
    format!(
        "{}{}.{:06}",
        if v < 0 { "-" } else { "" },
        v.unsigned_abs() / SCALE as u128,
        v.unsigned_abs() % SCALE as u128
    )
}
pub fn price(v: i128) -> Result<i128> {
    if !(TICK..=SCALE - TICK).contains(&v) || v % TICK != 0 {
        Err("PRICE_TICK")
    } else {
        Ok(v)
    }
}
pub fn fee(value: i128, quantity: u64, bps: u32, per_share: i128) -> Result<i128> {
    if value < 0 || per_share < 0 {
        return Err("FEE");
    }
    value
        .checked_mul(bps as i128)
        .and_then(|v| v.checked_add(9999))
        .map(|v| v / 10000)
        .and_then(|v| {
            per_share
                .checked_mul(quantity as i128)
                .and_then(|p| v.checked_add(p))
        })
        .ok_or("FEE_RANGE")
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    Yes,
    No,
}
impl Side {
    fn other(self) -> Self {
        match self {
            Self::Yes => Self::No,
            Self::No => Self::Yes,
        }
    }
}
#[derive(Clone, Debug)]
struct Lot {
    quantity: u64,
    price: i128,
    executed_at: Option<serde_json::Value>,
}
#[derive(Clone, Debug)]
pub struct Reservation {
    pub exchange: String,
    pub side: Side,
    pub quantity: u64,
    pub limit: i128,
}
#[derive(Clone, Debug)]
pub struct Wallet {
    pub initial: i128,
    pub cash: i128,
    pub realized: i128,
    pub fees: i128,
    pub maximum_committed: i128,
    pub fee_bps: u32,
    pub fee_per_share: i128,
    lots: BTreeMap<(String, Side), VecDeque<Lot>>,
    reservations: BTreeMap<String, Reservation>,
}
impl Wallet {
    pub fn positions(&self) -> serde_json::Value {
        let mut out = serde_json::json!({});
        for ((ex, side), lots) in &self.lots {
            if out.get(ex).is_none() {
                out[ex] = serde_json::json!({"YES":[],"NO":[]});
            }
            out[ex][if *side == Side::Yes { "YES" } else { "NO" }] = serde_json::json!(
                lots.iter()
                    .map(|l| {
                        let mut v =
                            serde_json::json!({"quantity":l.quantity,"price":money(l.price)});
                        if let Some(at) = &l.executed_at {
                            v["executedAt"] = at.clone();
                        }
                        v
                    })
                    .collect::<Vec<_>>()
            );
        }
        out
    }
    pub fn market_cost(&self, exchange: &str) -> i128 {
        [Side::Yes, Side::No]
            .into_iter()
            .flat_map(|side| {
                self.lots
                    .get(&(exchange.to_owned(), side))
                    .into_iter()
                    .flatten()
            })
            .map(|l| l.price * l.quantity as i128)
            .sum()
    }
    pub fn new(
        initial: i128,
        maximum_committed: i128,
        fee_bps: u32,
        fee_per_share: i128,
    ) -> Result<Self> {
        if initial <= 0 || maximum_committed <= 0 || fee_per_share < 0 {
            return Err("WALLET_POLICY");
        }
        Ok(Self {
            initial,
            cash: initial,
            realized: 0,
            fees: 0,
            maximum_committed,
            fee_bps,
            fee_per_share,
            lots: BTreeMap::new(),
            reservations: BTreeMap::new(),
        })
    }
    pub fn quantity(&self, exchange: &str, side: Side) -> u64 {
        self.lots
            .get(&(exchange.to_owned(), side))
            .map_or(0, |v| v.iter().map(|l| l.quantity).sum())
    }
    pub fn owned_cost(&self) -> i128 {
        self.lots
            .values()
            .flat_map(|x| x.iter())
            .map(|l| l.price * l.quantity as i128)
            .sum()
    }
    pub fn reserved(&self) -> Result<i128> {
        let mut total = 0i128;
        for r in self.reservations.values() {
            let unit = r
                .limit
                .checked_add(fee(r.limit, 1, self.fee_bps, self.fee_per_share)?)
                .ok_or("RESERVE_RANGE")?;
            total = total
                .checked_add(
                    unit.checked_mul(r.quantity as i128)
                        .ok_or("RESERVE_RANGE")?,
                )
                .ok_or("RESERVE_RANGE")?;
        }
        Ok(total)
    }
    pub fn reserve(&mut self, id: &str, r: Reservation) -> Result<()> {
        price(r.limit)?;
        if id.is_empty()
            || r.exchange.is_empty()
            || r.quantity == 0
            || self.reservations.contains_key(id)
        {
            return Err("RESERVATION_ID");
        }
        let mut next = self.clone();
        next.reservations.insert(id.to_owned(), r);
        next.audit()?;
        *self = next;
        Ok(())
    }
    // The event engine must prove cancellation coverage before calling release.
    pub fn release(&mut self, id: &str) -> Result<()> {
        self.reservations.remove(id).ok_or("RESERVATION_UNKNOWN")?;
        self.audit()
    }
    fn take(&mut self, exchange: &str, side: Side, mut n: u64) -> Result<i128> {
        if self.quantity(exchange, side) < n {
            return Err("UNCOVERED_SELL");
        }
        let lots = self
            .lots
            .get_mut(&(exchange.to_owned(), side))
            .ok_or("LOTS")?;
        let mut cost = 0;
        while n > 0 {
            let lot = lots.front_mut().ok_or("LOTS")?;
            let k = n.min(lot.quantity);
            cost += lot.price * k as i128;
            lot.quantity -= k;
            n -= k;
            if lot.quantity == 0 {
                lots.pop_front();
            }
        }
        Ok(cost)
    }
    pub fn buy_fill(&mut self, id: &str, n: u64) -> Result<()> {
        let r = self.reservations.get(id).ok_or("RESERVATION_UNKNOWN")?;
        let value = r.limit.checked_mul(n as i128).ok_or("FILL_RANGE")?;
        self.buy_fill_exact(id, n, r.limit, fee(value,n,self.fee_bps,self.fee_per_share)?)
    }
    /// Account fills use verified execution prices and explicit fees.
    pub fn buy_fill_exact(&mut self,id:&str,n:u64,executed_price:i128,charge:i128)->Result<()> {
        let r = self
            .reservations
            .get(id)
            .ok_or("RESERVATION_UNKNOWN")?
            .clone();
        price(executed_price)?;
        if n == 0 || n > r.quantity || executed_price > r.limit || charge < 0 || charge > fee(r.limit,1,self.fee_bps,self.fee_per_share)?.checked_mul(n as i128).ok_or("FILL_RANGE")? {
            return Err("FILL_QUANTITY");
        }
        let mut next = self.clone();
        let value = executed_price.checked_mul(n as i128).ok_or("FILL_RANGE")?;
        next.cash = next
            .cash
            .checked_sub(value)
            .and_then(|v| v.checked_sub(charge))
            .ok_or("FILL_RANGE")?;
        next.realized -= charge;
        next.fees += charge;
        next.lots
            .entry((r.exchange.clone(), r.side))
            .or_default()
            .push_back(Lot {
                quantity: n,
                price: executed_price,
                executed_at: None,
            });
        if n == r.quantity {
            next.reservations.remove(id);
        } else {
            next.reservations.get_mut(id).unwrap().quantity -= n;
        }
        let pairs = next
            .quantity(&r.exchange, r.side)
            .min(next.quantity(&r.exchange, r.side.other()));
        if pairs > 0 {
            let cost = next.take(&r.exchange, r.side, pairs)?
                + next.take(&r.exchange, r.side.other(), pairs)?;
            let payout = SCALE * pairs as i128;
            next.cash += payout;
            next.realized += payout - cost;
        }
        next.audit()?;
        *self = next;
        Ok(())
    }
    pub fn sell_fill(&mut self, exchange: &str, side: Side, n: u64, limit: i128) -> Result<()> {
        let charge=fee(limit.checked_mul(n as i128).ok_or("FILL_RANGE")?,n,self.fee_bps,self.fee_per_share)?;
        self.sell_fill_exact(exchange,side,n,limit,charge)
    }
    pub fn sell_fill_exact(&mut self,exchange:&str,side:Side,n:u64,limit:i128,charge:i128)->Result<()> {
        price(limit)?;
        if charge < 0 || charge > fee(limit,1,self.fee_bps,self.fee_per_share)?.checked_mul(n as i128).ok_or("FILL_RANGE")? { return Err("ACTUAL_SELL_FEE_BOUNDS") }
        if n == 0 {
            return Err("FILL_QUANTITY");
        }
        let mut next = self.clone();
        let cost = next.take(exchange, side, n)?;
        let value = limit.checked_mul(n as i128).ok_or("FILL_RANGE")?;
        next.cash += value - charge;
        next.realized += value - cost - charge;
        next.fees += charge;
        next.audit()?;
        *self = next;
        Ok(())
    }
    /// Exact official settlement, not an exchange sale or a fabricated fill.
    /// Supports zero/full payouts; keeps the unlevered accounting identity.
    pub fn settle_owned_exact(&mut self, exchange:&str, side:Side, n:u64, payout:i128)->Result<i128> {
        if n==0||payout<0||payout> (n as i128).checked_mul(SCALE).ok_or("SETTLEMENT_RANGE")? {return Err("SETTLEMENT_PAYOUT")}
        let mut next=self.clone();let cost=next.take(exchange,side,n)?;
        next.cash=next.cash.checked_add(payout).ok_or("SETTLEMENT_RANGE")?;
        next.realized=next.realized.checked_add(payout-cost).ok_or("SETTLEMENT_RANGE")?;
        next.audit()?;*self=next;Ok(cost)
    }
    pub fn audit(&self) -> Result<()> {
        let reserved = self.reserved()?;
        let owned = self.owned_cost();
        if self.cash < reserved
            || owned.checked_add(reserved).ok_or("ACCOUNT_RANGE")? > self.maximum_committed
        {
            return Err("FUNDING");
        }
        if self
            .cash
            .checked_add(owned)
            .and_then(|v| v.checked_sub(self.initial))
            != Some(self.realized)
        {
            return Err("ACCOUNTING_IDENTITY");
        }
        Ok(())
    }
    pub fn restore_record(
        state: &serde_json::Value,
        maximum_committed: i128,
        fee_bps: u32,
        fee_per_share: i128,
    ) -> Result<Self> {
        use serde_json::{Value, json};
        let decimal = |v: &Value| amount(v.as_str().ok_or("SEED_DECIMAL")?);
        let mut replay = Self::new(
            decimal(&state["policy"]["initialCash"])?,
            i128::MAX,
            fee_bps,
            fee_per_share,
        )?;
        let mut redemptions = Vec::new();
        for f in state["fills"].as_array().ok_or("SEED_FILLS")? {
            let exchange = f["exchangeId"]
                .as_str()
                .filter(|s| {
                    !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
                })
                .ok_or("SEED_EXCHANGE")?;
            let side = match f["side"].as_str() {
                Some("YES") => Side::Yes,
                Some("NO") => Side::No,
                _ => return Err("SEED_SIDE"),
            };
            let n = f["quantity"]
                .as_u64()
                .filter(|n| *n > 0 && *n <= 2_147_483_647)
                .ok_or("SEED_QUANTITY")?;
            let limit = price(decimal(&f["price"])?)?;
            let charge = decimal(&f["fee"])?;
            if charge < 0 {
                return Err("SEED_FEE");
            }
            let value = limit * n as i128;
            match f["action"].as_str() {
                Some("buy") => {
                    if replay.cash < value + charge {
                        return Err("SEED_CASH");
                    }
                    replay.cash -= value + charge;
                    replay.realized -= charge;
                    replay
                        .lots
                        .entry((exchange.to_owned(), side))
                        .or_default()
                        .push_back(Lot {
                            quantity: n,
                            price: limit,
                            executed_at: f.get("executedAt").cloned(),
                        });
                }
                Some("sell") => {
                    let cost = replay.take(exchange, side, n)?;
                    replay.cash += value - charge;
                    replay.realized += value - cost - charge;
                }
                _ => return Err("SEED_ACTION"),
            }
            replay.fees += charge;
            let pairs = replay
                .quantity(exchange, Side::Yes)
                .min(replay.quantity(exchange, Side::No));
            if pairs > 0 {
                let cost = replay.take(exchange, Side::Yes, pairs)?
                    + replay.take(exchange, Side::No, pairs)?;
                let payout = SCALE * pairs as i128;
                replay.cash += payout;
                replay.realized += payout - cost;
                redemptions.push(json!({"exchangeId":exchange,"quantity":pairs,"cost":money(cost),"payout":money(payout)}));
            }
            replay.audit()?;
        }
        let declared = state["redemptions"].as_array().ok_or("SEED_REDEMPTIONS")?;
        if declared.len() != redemptions.len() {
            return Err("SEED_REDEMPTIONS");
        }
        for (actual, old) in redemptions.iter().zip(declared) {
            for key in ["exchangeId", "quantity", "cost", "payout"] {
                if actual[key] != old[key] {
                    return Err("SEED_REDEMPTION_FACTS");
                }
            }
        }
        if replay.cash != decimal(&state["cash"])?
            || replay.realized != decimal(&state["realized"])?
            || replay.fees != decimal(&state["fees"])?
        {
            return Err("SEED_FINANCIAL_HISTORY");
        }
        let positions = state["positions"].as_object().ok_or("SEED_POSITIONS")?;
        let actual = replay.positions();
        for (ex, p) in positions {
            for side in ["YES", "NO"] {
                let lots = p[side].as_array().ok_or("SEED_LOTS")?;
                let owned = actual[ex][side].as_array();
                if !lots.is_empty() && owned.is_none() || owned.is_some_and(|a| a != lots) {
                    return Err("SEED_FIFO_POSITIONS");
                }
            }
        }
        for (ex, p) in actual.as_object().unwrap() {
            for side in ["YES", "NO"] {
                if !p[side].as_array().unwrap().is_empty() && positions.get(ex).is_none() {
                    return Err("SEED_MISSING_POSITION");
                }
            }
        }
        replay.maximum_committed = maximum_committed;
        replay.audit()?;
        Ok(replay)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn reserve(w: &mut Wallet, id: &str, side: Side, n: u64, p: &str) {
        w.reserve(
            id,
            Reservation {
                exchange: "1".into(),
                side,
                quantity: n,
                limit: amount(p).unwrap(),
            },
        )
        .unwrap()
    }
    #[test]
    fn decimals() {
        for s in ["0", "1.005", "-150.280", "100000.000000"] {
            assert_eq!(
                amount(&money(amount(s).unwrap())).unwrap(),
                amount(s).unwrap()
            )
        }
        for s in ["01", "1.", "1.0000001", "NaN", "+1", "1e3"] {
            assert!(amount(s).is_err())
        }
        assert!(price(amount("0.001").unwrap()).is_err())
    }
    #[test]
    fn paired_fifo_netting() {
        let mut w = Wallet::new(amount("1000").unwrap(), amount("40").unwrap(), 0, 0).unwrap();
        reserve(&mut w, "y", Side::Yes, 5, "0.4");
        reserve(&mut w, "n", Side::No, 5, "0.58");
        w.buy_fill("y", 5).unwrap();
        w.buy_fill("n", 5).unwrap();
        assert_eq!(money(w.cash), "1000.100000");
        assert_eq!(money(w.realized), "0.100000");
        assert_eq!(w.owned_cost(), 0);
        w.audit().unwrap()
    }
    #[test]
    fn losses_and_fifo_preserved() {
        let mut w = Wallet::new(amount("100").unwrap(), amount("40").unwrap(), 0, 0).unwrap();
        reserve(&mut w, "a", Side::Yes, 3, "0.4");
        w.buy_fill("a", 3).unwrap();
        reserve(&mut w, "b", Side::Yes, 2, "0.5");
        w.buy_fill("b", 2).unwrap();
        w.sell_fill("1", Side::Yes, 4, amount("0.3").unwrap())
            .unwrap();
        assert_eq!(money(w.realized), "-0.500000");
        assert_eq!(money(w.owned_cost()), "0.500000");
        w.sell_fill("1", Side::Yes, 1, amount("0.4").unwrap())
            .unwrap();
        assert_eq!(money(w.cash), "99.400000")
    }
    #[test]
    fn failed_mutations_are_atomic() {
        let mut w = Wallet::new(amount("10").unwrap(), amount("2").unwrap(), 0, 0).unwrap();
        let before = format!("{w:?}");
        assert!(
            w.reserve(
                "a",
                Reservation {
                    exchange: "1".into(),
                    side: Side::Yes,
                    quantity: 10,
                    limit: 500000
                }
            )
            .is_err()
        );
        assert!(w.sell_fill("1", Side::No, 1, 500000).is_err());
        assert_eq!(format!("{w:?}"), before)
    }
    #[test]
    fn partial_fills_keep_reservations_and_round_fees() {
        let mut w = Wallet::new(amount("10").unwrap(), amount("5").unwrap(), 1, 0).unwrap();
        reserve(&mut w, "a", Side::Yes, 3, "0.005");
        assert_eq!(w.reserved().unwrap(), 15003);
        w.buy_fill("a", 1).unwrap();
        assert_eq!(w.reserved().unwrap(), 10002);
        assert_eq!(w.fees, 1);
        assert!(w.buy_fill("a", 3).is_err());
        w.buy_fill("a", 2).unwrap();
        assert_eq!(w.reserved().unwrap(), 0);
        assert_eq!(w.fees, 2);
        w.audit().unwrap()
    }
}
