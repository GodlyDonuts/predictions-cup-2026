//! Exact, bounded payoff/package analysis. No network, account or dispatch code.
//!
//! A declaration is not a settlement certificate. Callers must supply separately
//! verified rule receipts and all exception payouts before a floor is certified.
//! Amounts use millionths of one SUSQie; quantities are whole shares.
use std::collections::BTreeSet;

pub const UNIT: i128 = 1_000_000;
pub type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub sha256: String,
    pub received_ms: u64,
}
impl Source {
    fn validate(&self, now: u64, max_age: u64) -> Result<()> {
        need(self.sha256.len() == 64 && self.sha256.bytes().all(|b| b.is_ascii_hexdigit()), "SOURCE_HASH")?;
        need(self.received_ms <= now && now - self.received_ms <= max_age, "SOURCE_STALE")
    }
}

/// `Verified` is an explicit adapter attestation, never inferred by this crate
/// from a market title, a price band, an active relation, or an ALL cash advance.
#[derive(Clone, Debug)]
pub enum Settlement {
    Unknown,
    Verified {
        rules: Source,
        /// True only after the rules have been checked to cover every terminal
        /// outcome. Ordinary YES/NO pay one/zero; list every other payout here.
        complete: bool,
        /// Every possible refund/N/A payout, (YES, NO) per share. Independent
        /// Cartesian enumeration is conservative when cancellation is coupled.
        exceptions: Vec<(i128, i128)>,
    },
}
#[derive(Clone, Debug)]
pub struct Contract {
    pub exchange_id: String,
    pub market_id: String,
    pub contract_id: String,
    pub settlement: Settlement,
}

#[derive(Clone, Debug)]
pub enum Logic {
    MutuallyExclusive { members: Vec<usize>, exhaustive: bool },
    Implication { antecedent: usize, consequent: usize },
    Complement { a: usize, b: usize },
    And { inputs: Vec<usize>, output: usize },
    Or { inputs: Vec<usize>, output: usize },
    Not { input: usize, output: usize },
    /// In particular monotonic and conditional_if enums alone are insufficient.
    Unsupported,
}
#[derive(Clone, Debug)]
pub struct Declaration {
    pub relationship_id: String,
    pub version: u64,
    pub active: bool,
    pub source: Source,
    /// Independent authoritative rule receipt establishing this exact terminal
    /// logic, as distinct from the API's quoted-price inequality description.
    pub settlement_logic_source: Option<Source>,
    pub logic: Logic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side { Yes, No }
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instrument { pub contract: usize, pub side: Side }
#[derive(Clone, Debug)]
pub struct Compiled {
    pub contracts: Vec<Contract>,
    pub relationships: Vec<(String, u64)>,
    /// Each row contains (YES, NO) terminal payouts for every contract.
    pub states: Vec<Vec<(i128, i128)>>,
    pub compiled_ms: u64,
    pub source_expiry_ms: u64,
}

fn need(ok: bool, why: &'static str) -> Result<()> { if ok { Ok(()) } else { Err(why) } }
fn add(a: i128, b: i128) -> Result<i128> { a.checked_add(b).ok_or("OVERFLOW") }
fn mul(a: i128, b: i128) -> Result<i128> { a.checked_mul(b).ok_or("OVERFLOW") }
fn uuid(s: &str) -> bool {
    s.len() == 36 && s.bytes().enumerate().all(|(i,b)|
        if [8,13,18,23].contains(&i) { b == b'-' } else { b.is_ascii_hexdigit() })
}
fn clean(s: &str) -> bool { !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control) }
impl Logic {
    fn references(&self) -> Result<Vec<usize>> {
        Ok(match self {
            Self::MutuallyExclusive { members, .. } => { need(members.len() >= 2, "LOGIC_ARITY")?; members.clone() },
            Self::Implication { antecedent, consequent } => vec![*antecedent,*consequent],
            Self::Complement { a,b } => vec![*a,*b],
            Self::And { inputs,output } | Self::Or { inputs,output } => {
                need(!inputs.is_empty(), "LOGIC_ARITY")?;
                let mut v = inputs.clone(); v.push(*output); v
            },
            Self::Not { input,output } => vec![*input,*output],
            Self::Unsupported => return Err("UNSUPPORTED_LOGIC"),
        })
    }
    fn permits(&self, truth: &[bool]) -> bool {
        match self {
            Self::MutuallyExclusive { members,exhaustive } => {
                let n = members.iter().filter(|i| truth[**i]).count(); n <= 1 && (!exhaustive || n == 1)
            },
            Self::Implication { antecedent,consequent } => !truth[*antecedent] || truth[*consequent],
            Self::Complement { a,b } => truth[*a] != truth[*b],
            Self::And { inputs,output } => truth[*output] == inputs.iter().all(|i| truth[*i]),
            Self::Or { inputs,output } => truth[*output] == inputs.iter().any(|i| truth[*i]),
            Self::Not { input,output } => truth[*output] != truth[*input],
            Self::Unsupported => false,
        }
    }
}

pub fn compile(contracts: Vec<Contract>, declarations: &[Declaration], now: u64, max_age: u64) -> Result<Compiled> {
    need((1..=3).contains(&contracts.len()) && declarations.len() <= 16, "COMPONENT_BUDGET")?;
    need(contracts.len() == 1 || !declarations.is_empty(), "MISSING_RELATIONSHIPS")?;
    let mut exchanges = BTreeSet::new();
    let mut contract_ids = BTreeSet::new();
    let mut expiry = u64::MAX;
    let mut variants = vec![];
    for c in &contracts {
        need(clean(&c.exchange_id) && clean(&c.market_id) && clean(&c.contract_id), "CONTRACT_ID")?;
        need(exchanges.insert(&c.exchange_id) && contract_ids.insert(&c.contract_id), "DUPLICATE_CONTRACT")?;
        let Settlement::Verified { rules,complete,exceptions } = &c.settlement else { return Err("SETTLEMENT_UNKNOWN") };
        rules.validate(now,max_age)?;
        expiry = expiry.min(rules.received_ms.saturating_add(max_age));
        need(*complete && exceptions.len() <= 4, "SETTLEMENT_INCOMPLETE")?;
        let mut v = vec![(UNIT,0),(0,UNIT)];
        for &(y,n) in exceptions {
            need((0..=UNIT).contains(&y) && (0..=UNIT).contains(&n), "REFUND_PAYOUT")?;
            if !v.contains(&(y,n)) { v.push((y,n)); }
        }
        variants.push(v);
    }
    let mut ids = BTreeSet::new();
    let mut covered = BTreeSet::new();
    for d in declarations {
        need(uuid(&d.relationship_id) && d.version > 0 && d.active && ids.insert(&d.relationship_id), "DECLARATION_ID_STATUS")?;
        d.source.validate(now,max_age)?;
        let proof = d.settlement_logic_source.as_ref().ok_or("SETTLEMENT_LOGIC_UNVERIFIED")?;
        proof.validate(now,max_age)?;
        expiry = expiry.min(d.source.received_ms.saturating_add(max_age)).min(proof.received_ms.saturating_add(max_age));
        let refs = d.logic.references()?;
        let unique: BTreeSet<_> = refs.iter().copied().collect();
        need(unique.len() == refs.len() && refs.iter().all(|i| *i < contracts.len()), "LOGIC_REFERENCE")?;
        covered.extend(unique);
    }
    need(contracts.len() == 1 || covered.len() == contracts.len(), "UNRELATED_CONTRACT")?;
    // Ordinary states use declared logic. Exception combinations are admitted
    // conservatively without ordinary Boolean implications, which need not hold
    // during cancellation. No omission of a low-paying cancellation state.
    let total: usize = variants.iter().map(Vec::len).product();
    need(total <= 216, "STATE_BUDGET")?;
    let mut states = vec![];
    let mut ordinary_count = 0;
    for serial in 0..total {
        let mut cursor = serial;
        let mut row = vec![];
        let mut truth = vec![];
        let mut exception = false;
        for v in &variants {
            let digit = cursor % v.len(); cursor /= v.len();
            row.push(v[digit]); truth.push(digit == 0); exception |= digit >= 2;
        }
        let allowed = declarations.iter().all(|d| d.logic.permits(&truth));
        if !exception && allowed { ordinary_count += 1; }
        if exception || allowed { states.push(row); }
    }
    need(ordinary_count > 0 && !states.is_empty(), "INCONSISTENT_LOGIC")?;
    Ok(Compiled { contracts,relationships:declarations.iter().map(|d|(d.relationship_id.clone(),d.version)).collect(),states,compiled_ms:now,source_expiry_ms:expiry })
}

#[derive(Clone, Copy, Debug)]
pub struct Level { pub price: i128, pub quantity: u64 }
#[derive(Clone, Debug)]
pub struct Book {
    pub exchange_id: String,
    pub source: Source,
    pub yes_asks: Vec<Level>,
    pub no_asks: Vec<Level>,
    pub yes_bids: Vec<Level>,
    pub no_bids: Vec<Level>,
}
/// Full-level executable notional; never extrapolates the best price.
pub fn depth_cost(levels: &[Level], quantity: u64, ascending: bool) -> Result<i128> {
    need(quantity > 0 && levels.len() <= 200, "DEPTH_BUDGET")?;
    let mut previous = if ascending { 0 } else { UNIT };
    // Validate the complete receipt, including levels beyond the requested size.
    for l in levels {
        need((0..=UNIT).contains(&l.price) && l.quantity > 0, "DEPTH_LEVEL")?;
        need(if ascending { l.price >= previous } else { l.price <= previous }, "DEPTH_ORDER")?;
        previous = l.price;
    }
    let mut left = quantity;
    let mut cost = 0;
    for l in levels {
        let take = left.min(l.quantity);
        cost = add(cost,mul(l.price,i128::from(take))?)?;
        left -= take;
        if left == 0 { return Ok(cost); }
    }
    Err("INSUFFICIENT_DEPTH")
}
#[derive(Clone, Debug)]
pub struct Limits {
    pub now_ms: u64,
    pub max_book_age_ms: u64,
    pub max_book_skew_ms: u64,
    pub cash_available: i128,
    pub max_package_cost: i128,
    pub max_orphan_cash_loss: i128,
    pub fee_per_share_per_leg: i128,
    pub buffer_per_share: i128,
    pub minimum_surplus: i128,
    pub max_quantity: u64,
}
#[derive(Clone, Debug)]
pub struct Plan {
    pub instruments: Vec<Instrument>,
    pub quantity: u64,
    pub leg_costs: Vec<i128>,
    pub acquisition_cost: i128,
    pub fees: i128,
    pub buffer: i128,
    pub terminal_floor: i128,
    pub guaranteed_surplus: i128,
    /// Hard gross cash bound if every acquired orphan becomes worthless. Bids
    /// may disappear; observed liquidation depth is never a hard loss guarantee.
    pub worst_orphan_cash_loss: i128,
    pub observed_orphan_unwind_loss: Option<i128>,
    pub relationship_versions: Vec<(String,u64)>,
    pub book_sources: Vec<Source>,
}
impl Compiled {
    pub fn evaluate(&self, instruments: &[Instrument], quantity: u64, books: &[Book], l: &Limits) -> Result<Plan> {
        need((2..=3).contains(&instruments.len()) && quantity > 0 && quantity <= l.max_quantity, "PACKAGE_BUDGET")?;
        need(l.now_ms >= self.compiled_ms && l.now_ms <= self.source_expiry_ms, "COMPILED_STALE")?;
        need([l.cash_available,l.max_package_cost,l.max_orphan_cash_loss,l.fee_per_share_per_leg,l.buffer_per_share,l.minimum_surplus].iter().all(|x| *x >= 0), "NEGATIVE_LIMIT")?;
        need(books.len() == self.contracts.len(), "BOOK_SCOPE")?;
        let mut seen = BTreeSet::new();
        let mut leg_costs = vec![];
        let mut sources = vec![];
        let mut unwind = vec![];
        for i in instruments {
            need(i.contract < self.contracts.len() && seen.insert(*i), "INSTRUMENT_SCOPE")?;
            let b = &books[i.contract];
            need(b.exchange_id == self.contracts[i.contract].exchange_id, "BOOK_SCOPE")?;
            b.source.validate(l.now_ms,l.max_book_age_ms)?;
            let (asks,bids) = match i.side { Side::Yes => (&b.yes_asks,&b.yes_bids),Side::No => (&b.no_asks,&b.no_bids) };
            leg_costs.push(depth_cost(asks,quantity,true)?);
            unwind.push(depth_cost(bids,quantity,false).ok());
            sources.push(b.source.clone());
        }
        let first = sources.iter().map(|s|s.received_ms).min().unwrap();
        let last = sources.iter().map(|s|s.received_ms).max().unwrap();
        need(last-first <= l.max_book_skew_ms, "BOOK_SKEW")?;
        let cost = leg_costs.iter().try_fold(0,|a,b|add(a,*b))?;
        let fees = mul(mul(l.fee_per_share_per_leg,i128::from(quantity))?,instruments.len() as i128)?;
        let buffer = mul(l.buffer_per_share,i128::from(quantity))?;
        let total = add(cost,fees)?;
        need(total <= l.cash_available && total <= l.max_package_cost, "PACKAGE_CAPITAL")?;
        let mut floor = i128::MAX;
        for row in &self.states {
            let payout = instruments.iter().try_fold(0,|a,i|add(a,match i.side { Side::Yes=>row[i.contract].0,Side::No=>row[i.contract].1 }))?;
            floor = floor.min(mul(payout,i128::from(quantity))?);
        }
        let surplus = floor.checked_sub(add(total,buffer)?).ok_or("OVERFLOW")?;
        need(surplus > 0 && surplus >= l.minimum_surplus, "NO_GUARANTEED_SURPLUS")?;
        let fee_leg = mul(l.fee_per_share_per_leg,i128::from(quantity))?;
        // Full funded acquisition is the hard bound even when every leg is
        // partially filled in different proportions. Full-leg subsets alone
        // understate that exposure, so do not use their maximum as the cap.
        let orphan = total;
        let mut observed = Some(0);
        // Every proper subset, not just the first leg. Partial fills cannot cost
        // more than this full-size gross bound; a completed basket uses its floor.
        for subset in 1..(1usize << instruments.len())-1 {
            let mut paid = 0;
            let mut received = Some(0);
            for j in 0..instruments.len() {
                if subset & (1 << j) != 0 {
                    paid = add(paid,add(leg_costs[j],fee_leg)?)?;
                    received = match (received,unwind[j]) { (Some(a),Some(b))=>Some(add(a,b)?),_=>None };
                }
            }
            observed = match (observed,received) { (Some(a),Some(b))=>Some(a.max(paid.saturating_sub(b).max(0))),_=>None };
        }
        need(orphan <= l.max_orphan_cash_loss, "ORPHAN_CASH_BOUND")?;
        Ok(Plan { instruments:instruments.to_vec(),quantity,leg_costs,acquisition_cost:cost,fees,buffer,terminal_floor:floor,guaranteed_surplus:surplus,worst_orphan_cash_loss:orphan,observed_orphan_unwind_loss:observed,relationship_versions:self.relationships.clone(),book_sources:sources })
    }

    pub fn portfolio_floor(&self, positions: &[(Instrument,u64)]) -> Result<i128> {
        need(positions.len() <= 6, "POSITION_BUDGET")?;
        let mut seen = BTreeSet::new();
        for (i,_) in positions {
            need(i.contract < self.contracts.len() && seen.insert(*i), "INSTRUMENT_SCOPE")?;
        }
        self.states.iter().map(|row| positions.iter().try_fold(0,|a,(i,q)| {
            let payout = match i.side { Side::Yes=>row[i.contract].0,Side::No=>row[i.contract].1 };
            add(a,mul(payout,i128::from(*q))?)
        })).try_fold(i128::MAX,|a,b|Ok(a.min(b?)))
    }

    /// Change in the entire portfolio's guaranteed floor; not an independent
    /// fair-value forecast, realized profit, or collateral release estimate.
    pub fn marginal_floor(&self, positions: &[(Instrument,u64)], add_instrument: Instrument, quantity: u64) -> Result<i128> {
        let before = self.portfolio_floor(positions)?;
        let mut after = positions.to_vec();
        if let Some((_,q)) = after.iter_mut().find(|(i,_)|*i == add_instrument) {
            *q = q.checked_add(quantity).ok_or("OVERFLOW")?;
        } else { after.push((add_instrument,quantity)); }
        self.portfolio_floor(&after)?.checked_sub(before).ok_or("OVERFLOW")
    }

    /// Deterministically enumerate every distinct 2-/3-instrument long-only
    /// basket. Quantity is explicit; no hidden tuning or capital multiplication.
    pub fn scan(&self, quantities: &[u64], books: &[Book], limits: &Limits) -> Result<Vec<Plan>> {
        need(quantities.len() <= 32, "SCAN_BUDGET")?;
        let unique: BTreeSet<_> = quantities.iter().copied().collect();
        need(unique.len() == quantities.len() && quantities.iter().all(|q| *q > 0 && *q <= limits.max_quantity), "SCAN_QUANTITY")?;
        let instruments: Vec<_> = (0..self.contracts.len()).flat_map(|contract|[Instrument{contract,side:Side::Yes},Instrument{contract,side:Side::No}]).collect();
        let mut plans = vec![];
        for a in 0..instruments.len() { for b in a+1..instruments.len() {
            for &q in quantities {
                if let Ok(p) = self.evaluate(&[instruments[a],instruments[b]],q,books,limits) { plans.push(p); }
            }
            for c in b+1..instruments.len() { for &q in quantities {
                if let Ok(p) = self.evaluate(&[instruments[a],instruments[b],instruments[c]],q,books,limits) { plans.push(p); }
            }}
        }}
        plans.sort_by(|a,b|b.guaranteed_surplus.cmp(&a.guaranteed_surplus).then(a.acquisition_cost.cmp(&b.acquisition_cost)).then(a.instruments.cmp(&b.instruments)).then(a.quantity.cmp(&b.quantity)));
        Ok(plans)
    }
}
