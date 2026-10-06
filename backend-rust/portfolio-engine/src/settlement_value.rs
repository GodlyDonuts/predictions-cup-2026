//! Pure settlement-value decisions. Estimates are scenarios, never payouts or fills.
use crate::money::{amount, money, price, SCALE, TICK};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    schema_version: u32,
    mode: String,
    as_of_ms: i64,
    policy: Policy,
    wallet: Wallet,
    candidates: Vec<Candidate>,
    receipt_policy: Option<ReceiptPolicy>,
    receipt_state: Option<ReceiptState>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReceiptPolicy { take_profit: String, stop_loss: String, maximum_hold_ms: i64, cooldown_ms: i64, order_shares: i64 }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReceiptState { risk_exits: BTreeMap<String,String>, last_exited_at_ms: BTreeMap<String,i64> }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Policy {
    minimum_edge: String,
    maximum_spread: String,
    maximum_book_age_ms: i64,
    maximum_research_age_ms: i64,
    fractional_kelly_bps: i128,
    slippage_ticks: i128,
    depth_fraction_bps: i128,
    max_order_cost: String,
    max_exposure: String,
    cash_reserve: String,
    cumulative_buy_budget: String,
    group_caps: BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Wallet {
    capital: String,
    cash: String,
    cumulative_buys: String,
    positions: Vec<Position>,
    reservations: Vec<Reservation>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Position { candidate_id: String, quantity: i64, cost: String, entered_at_ms: i64 }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reservation { candidate_id: String, maximum_cost: String }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Candidate {
    id: String, market_id: String, exchange_id: String, side: String,
    predicate_hash: String, risk_groups: Vec<String>, settles_at_ms: i64,
    research: Option<Research>, book: Option<Book>, account: Option<Account>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Research {
    id: String, predicate_hash: String, side: String, lower: String, upper: String,
    recorded_at_ms: i64, source_at_ms: i64, expires_at_ms: i64,
    method: String, settlement_reviewed: bool, flags: Vec<String>, sources: Vec<Source>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Source { url: String, content_hash: String }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Book {
    request_started_at_ms: i64, received_at_ms: i64, available_at_ms: i64,
    sequence: u64, source_hash: String, representation: String,
    bids: Vec<Level>, asks: Vec<Level>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Level { price: String, quantity: i64 }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Account {
    positions_at_ms: i64, orders_at_ms: i64, known_at_ms: i64,
    complete: bool, position_excluded: bool, open_order_excluded: bool,
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("SETTLEMENT_{code}")) }
}
fn clock(n: i64) -> bool { (0..=9_007_199_254_740_991).contains(&n) }
fn ident(s: &str) -> bool { !s.is_empty() && s.len() <= 100 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-:".contains(&b)) }
fn number(s: &str) -> bool { !s.is_empty() && !s.starts_with('0') && s.parse::<u64>().is_ok_and(|n| n <= 9_007_199_254_740_991) }
fn hash(s: &str) -> bool { s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) }
fn positive(s: &str) -> Result<i128, String> { let n = amount(s)?; need(n > 0 && n <= 1_000_000_000 * SCALE, "MONEY_RANGE")?; Ok(n) }
fn nonnegative(s: &str) -> Result<i128, String> { let n = amount(s)?; need(n >= 0 && n <= 1_000_000_000 * SCALE, "MONEY_RANGE")?; Ok(n) }

impl Input {
    fn validate(&self) -> Result<(), String> {
        let receipt = self.schema_version == 2 && self.mode == "probability_receipt_policy_v1";
        need((receipt || self.schema_version == 1 && self.mode == "settlement_value_policy_v1") && clock(self.as_of_ms), "INPUT")?;
        need(receipt == self.receipt_policy.is_some() && receipt == self.receipt_state.is_some(), "RECEIPT_BINDING")?;
        if let Some(r) = &self.receipt_policy {
            need(amount(&r.take_profit)? == 20000 && amount(&r.stop_loss)? == 50000 && r.maximum_hold_ms == 172800000 && r.cooldown_ms == 900000 && r.order_shares == 1
                && amount(&self.policy.minimum_edge)? == 20000 && amount(&self.policy.maximum_spread)? == 20000 && self.policy.slippage_ticks == 1 && self.policy.depth_fraction_bps == 100
                && self.candidates.len() <= 3 && self.wallet.positions.iter().all(|p| p.quantity == 1), "RECEIPT_POLICY")?;
        }
        let p = &self.policy;
        need((1..=2500).contains(&p.fractional_kelly_bps) && (0..=5).contains(&p.slippage_ticks) && (1..=1000).contains(&p.depth_fraction_bps)
            && (1000..=20000).contains(&p.maximum_book_age_ms) && (1000..=604800000).contains(&p.maximum_research_age_ms), "POLICY")?;
        need(positive(&p.minimum_edge)? < SCALE && positive(&p.maximum_spread)? < SCALE && !p.group_caps.is_empty(), "POLICY")?;
        let capital = positive(&self.wallet.capital)?;
        need(nonnegative(&p.cash_reserve)? < capital && positive(&p.max_exposure)? <= capital && positive(&p.max_order_cost)? <= positive(&p.max_exposure)?, "RISK")?;
        positive(&p.cumulative_buy_budget)?; nonnegative(&self.wallet.cash)?; nonnegative(&self.wallet.cumulative_buys)?;
        for (g, cap) in &p.group_caps { need(ident(g) && positive(cap)? <= capital, "GROUP_CAP")?; }
        need(!self.candidates.is_empty() && self.candidates.len() <= 600, "CANDIDATES")?;
        let mut ids = BTreeSet::new(); let mut instruments = BTreeSet::new();
        for c in &self.candidates {
            need(ident(&c.id) && number(&c.market_id) && number(&c.exchange_id) && ["YES", "NO"].contains(&c.side.as_str()) && hash(&c.predicate_hash)
                && ids.insert(&c.id) && instruments.insert((&c.exchange_id, &c.side)) && clock(c.settles_at_ms), "IDENTITY")?;
            need(!c.risk_groups.is_empty() && c.risk_groups.iter().collect::<BTreeSet<_>>().len() == c.risk_groups.len()
                && c.risk_groups.iter().all(|g| p.group_caps.contains_key(g)), "GROUP_BINDING")?;
            if let Some(r) = &c.research {
                let (lo, hi) = (nonnegative(&r.lower)?, nonnegative(&r.upper)?);
                need(ident(&r.id) && hash(&r.predicate_hash) && ["YES", "NO"].contains(&r.side.as_str()) && lo <= hi && hi <= SCALE
                    && ["subjective_range", "calibrated_model"].contains(&r.method.as_str()) && clock(r.source_at_ms) && clock(r.recorded_at_ms) && clock(r.expires_at_ms)
                    && r.source_at_ms <= r.recorded_at_ms && r.expires_at_ms > r.recorded_at_ms && !r.sources.is_empty(), "RESEARCH")?;
                need(r.flags.iter().all(|f| ["predicate_uncertain", "settlement_uncertain", "source_conflict", "superseded"].contains(&f.as_str())), "RESEARCH_FLAGS")?;
                for s in &r.sources { need(s.url.starts_with("https://") && !s.url.contains(['?', '#', '@']) && !s.url.contains("ace_") && hash(&s.content_hash), "SOURCE")?; }
            }
            if let Some(b) = &c.book {
                need(clock(b.request_started_at_ms) && clock(b.received_at_ms) && clock(b.available_at_ms) && b.request_started_at_ms <= b.received_at_ms && b.received_at_ms <= b.available_at_ms
                    && b.sequence > 0 && hash(&b.source_hash) && b.representation == if c.side == "YES" { "direct_yes" } else { "canonical_yes_complement" }, "BOOK")?;
                for (levels, ascending) in [(&b.bids, false), (&b.asks, true)] {
                    need(levels.len() <= 100, "DEPTH")?; let mut prior = None;
                    for l in levels { let v = price(&l.price)?; need((1..=1_000_000_000).contains(&l.quantity) && prior.is_none_or(|p| if ascending {v > p} else {v < p}), "DEPTH")?; prior = Some(v); }
                }
                if !b.asks.is_empty() && !b.bids.is_empty() { need(price(&b.bids[0].price)? < price(&b.asks[0].price)?, "CROSSED")?; }
            }
            if let Some(a) = &c.account { need(clock(a.positions_at_ms) && clock(a.orders_at_ms) && clock(a.known_at_ms) && a.positions_at_ms <= a.orders_at_ms && a.orders_at_ms <= a.known_at_ms, "ACCOUNT")?; }
        }
        let mut held = BTreeSet::new();
        for pos in &self.wallet.positions { need(ids.contains(&pos.candidate_id) && held.insert(&pos.candidate_id) && (1..=1_000_000_000).contains(&pos.quantity)
            && clock(pos.entered_at_ms) && pos.entered_at_ms <= self.as_of_ms && positive(&pos.cost)? <= pos.quantity as i128 * SCALE, "POSITION")?; }
        let held_basis = self.wallet.positions.iter().map(|p| amount(&p.cost)).collect::<Result<Vec<_>, _>>()?.into_iter().sum::<i128>();
        need(nonnegative(&self.wallet.cumulative_buys)? >= held_basis, "PURCHASE_ACCOUNTING")?;
        let mut reserved = BTreeSet::new();
        for r in &self.wallet.reservations { need(ids.contains(&r.candidate_id) && reserved.insert(&r.candidate_id), "RESERVATION")?; positive(&r.maximum_cost)?; }
        if let Some(s) = &self.receipt_state {
            for (id, reason) in &s.risk_exits { need(held.contains(id) && ["stop_loss","holding_timeout"].contains(&reason.as_str()), "RISK_EXIT_BINDING")?; }
            for (id, at) in &s.last_exited_at_ms { need(ids.contains(id) && !held.contains(id) && clock(*at) && *at <= self.as_of_ms, "COOLDOWN_BINDING")?; }
        }
        Ok(())
    }
}

fn book_gate<'a>(i: &Input, c: &'a Candidate, holding: bool) -> Result<&'a Book, &'static str> {
    let b = c.book.as_ref().ok_or("missing_book")?;
    if b.available_at_ms > i.as_of_ms || i.as_of_ms - b.received_at_ms > i.policy.maximum_book_age_ms || b.received_at_ms - b.request_started_at_ms > i.policy.maximum_book_age_ms { return Err("stale_or_future_book"); }
    let a = c.account.as_ref().ok_or("account_unavailable")?;
    if !a.complete || a.known_at_ms > b.request_started_at_ms || i.as_of_ms - a.positions_at_ms > 20000 || i.as_of_ms - a.orders_at_ms > 20000 { return Err("account_not_causal_or_fresh"); }
    if a.open_order_excluded { return Err("own_account_open_order"); }
    if a.position_excluded && !holding { return Err("own_account_exposure"); }
    if b.bids.is_empty() || b.asks.is_empty() { return Err("missing_depth"); }
    Ok(b)
}

fn gate<'a>(i: &Input, c: &'a Candidate, holding: bool) -> Result<(&'a Research, &'a Book), &'static str> {
    let r = c.research.as_ref().ok_or("missing_research")?;
    if r.predicate_hash != c.predicate_hash || r.side != c.side { return Err("contract_mismatch"); }
    if !r.settlement_reviewed || !r.flags.is_empty() { return Err("unresolved_evidence"); }
    let b = c.book.as_ref().ok_or("missing_book")?;
    if r.recorded_at_ms > b.request_started_at_ms || r.source_at_ms > b.request_started_at_ms { return Err("research_not_yet_known"); }
    if i.as_of_ms >= r.expires_at_ms || i.as_of_ms - r.source_at_ms > i.policy.maximum_research_age_ms { return Err("expired_research"); }
    if i.as_of_ms >= c.settles_at_ms { return Err("settlement_unobserved"); }
    book_gate(i, c, holding)?;
    Ok((r, b))
}

// Basis is risk exposure, not a forecast of future liquidation proceeds.
fn liquidate(p: &Position, b: &Book, policy: &Policy) -> Result<Value, String> {
    let mut left = p.quantity as i128; let mut proceeds = 0;
    for l in &b.bids {
        let q = left.min(l.quantity as i128 * policy.depth_fraction_bps / 10000);
        let v = (price(&l.price)? - policy.slippage_ticks * TICK).max(0);
        proceeds += q * v; left -= q; if left == 0 { break; }
    }
    Ok(json!({"complete":left == 0,"observedCapacityShares":p.quantity as i128-left,"estimatedProceeds":if left == 0 {Some(money(proceeds))} else {None},
        "estimatedPnl":if left == 0 {Some(money(proceeds-amount(&p.cost)?))} else {None}}))
}

pub fn run(i: &Input) -> Result<Value, String> {
    i.validate()?;
    let p = &i.policy; let capital = amount(&i.wallet.capital)?;
    let mut held = 0; let mut reserved = 0; let mut group_used = BTreeMap::<String, i128>::new();
    for c in &i.candidates {
        let cost = i.wallet.positions.iter().filter(|h| h.candidate_id == c.id).map(|h| amount(&h.cost)).collect::<Result<Vec<_>, _>>()?.iter().sum::<i128>();
        let reservation = i.wallet.reservations.iter().filter(|h| h.candidate_id == c.id).map(|h| amount(&h.maximum_cost)).collect::<Result<Vec<_>, _>>()?.iter().sum::<i128>();
        held += cost; reserved += reservation;
        for g in &c.risk_groups { *group_used.entry(g.clone()).or_default() += cost + reservation; }
    }
    let mut budget = (amount(&i.wallet.cash)? - amount(&p.cash_reserve)? - reserved)
        .min(amount(&p.max_exposure)? - held - reserved)
        .min(amount(&p.cumulative_buy_budget)? - amount(&i.wallet.cumulative_buys)? - reserved).max(0);
    let initial_budget = budget;
    let mut risk_breaches = Vec::new();
    if held + reserved > amount(&p.max_exposure)? { risk_breaches.push(json!({"scope":"portfolio","exposure":money(held+reserved),"cap":p.max_exposure})); }
    for (g, used) in &group_used { if *used > amount(&p.group_caps[g])? { risk_breaches.push(json!({"scope":g,"exposure":money(*used),"cap":p.group_caps[g]})); } }
    let mut decisions = BTreeMap::<String, Value>::new(); let mut entries = Vec::<(&Candidate, i128)>::new();
    for c in &i.candidates {
        let pos = i.wallet.positions.iter().find(|h| h.candidate_id == c.id);
        let mut d = json!({"candidateId":c.id,"marketId":c.market_id,"exchangeId":c.exchange_id,"side":c.side,"quantity":0,
            "action":"observe","reason":"","executionAuthority":false,"orderEligible":false});
        if let Some(pos) = pos {
            d["worstCaseHeldLoss"] = json!(pos.cost); d["holdingAgeMs"] = json!(i.as_of_ms-pos.entered_at_ms);
            // Forecast expiry never hides the independent liquidation/risk view.
            match book_gate(i, c, true) {
                Ok(b) => { d["liquidation"] = liquidate(pos, b, p)?; }
                Err(reason) => { d["liquidation"] = Value::Null; d["liquidationUnavailableReason"] = json!(reason); }
            }
        }
        if let (Some(rp), Some(rs), Some(pos)) = (&i.receipt_policy, &i.receipt_state, pos) {
            // Owned exits use fresh executable depth even after research expires.
            // A previously triggered risk exit stays latched through price recovery.
            match book_gate(i, c, true) {
                Err(reason) => { d["action"] = json!("review_required"); d["reason"] = json!(reason); }
                Ok(b) => {
                    let bid = (price(&b.bids[0].price)? - p.slippage_ticks*TICK).max(0);
                    let basis = amount(&pos.cost)? / pos.quantity as i128;
                    let risk = rs.risk_exits.get(&c.id).map(String::as_str).or_else(|| {
                        if basis-bid >= amount(&rp.stop_loss).unwrap() { Some("stop_loss") }
                        else if i.as_of_ms-pos.entered_at_ms >= rp.maximum_hold_ms { Some("holding_timeout") } else { None }
                    });
                    d["effectiveBid"] = json!(money(bid));
                    if risk.is_some() || bid-basis >= amount(&rp.take_profit)? {
                        d["action"] = json!("reduce"); d["reason"] = json!(risk.unwrap_or("take_profit"));
                        d["riskExit"] = json!(risk);
                        d["quantity"] = json!((pos.quantity as i128).min(b.bids[0].quantity as i128*p.depth_fraction_bps/10000));
                        d["hardLimitWithCushion"] = json!(money(bid.max(TICK)));
                    } else { d["action"] = json!("hold"); d["reason"] = json!("receipt_exit_threshold_not_reached"); }
                }
            }
            decisions.insert(c.id.clone(), d); continue;
        }
        if let (Some(rp), Some(rs)) = (&i.receipt_policy, &i.receipt_state) {
            if rs.last_exited_at_ms.get(&c.id).is_some_and(|at| i.as_of_ms-at < rp.cooldown_ms) {
                d["reason"] = json!("cooldown"); decisions.insert(c.id.clone(), d); continue;
            }
        }
        match gate(i, c, pos.is_some()) {
            Err(reason) => { d["reason"] = json!(reason); if pos.is_some() { d["action"] = json!("review_required"); } }
            Ok((r, b)) => {
                d["researchId"] = json!(r.id); d["researchMethod"] = json!(r.method); d["bookSequence"] = json!(b.sequence); d["bookSourceHash"] = json!(b.source_hash);
                if let Some(pos) = pos {
                    d["liquidation"] = liquidate(pos, b, p)?;
                    d["scenarioSettlementPnlRange"] = json!([money(amount(&r.lower)?*pos.quantity as i128-amount(&pos.cost)?),money(amount(&r.upper)?*pos.quantity as i128-amount(&pos.cost)?)]);
                    let bid = (price(&b.bids[0].price)?-p.slippage_ticks*TICK).max(0);
                    // Evaluate future opportunity cost, including a possible loss; do not anchor to entry.
                    if bid >= amount(&r.upper)? {
                        d["action"] = json!("reduce"); d["reason"] = json!("bid_at_or_above_upper_scenario");
                        d["quantity"] = json!((pos.quantity as i128).min(b.bids[0].quantity as i128*p.depth_fraction_bps/10000)); d["effectiveBid"] = json!(money(bid));
                    } else { d["action"] = json!("hold"); d["reason"] = json!("settlement_value_retained"); }
                    // No invented payout at a deadline, no two-day timeout or price-only stop.
                } else if i.wallet.reservations.iter().any(|r| r.candidate_id == c.id) {
                    d["reason"] = json!("pending_reservation");
                } else {
                    let ask = price(&b.asks[0].price)?+p.slippage_ticks*TICK;
                    let edge = amount(&r.lower)?-ask; d["topScenarioEdge"] = json!(money(edge));
                    if price(&b.asks[0].price)?-price(&b.bids[0].price)? > amount(&p.maximum_spread)? { d["reason"] = json!("spread"); }
                    else if ask >= SCALE || edge < amount(&p.minimum_edge)? { d["reason"] = json!("insufficient_scenario_edge"); }
                    else { entries.push((c, edge)); }
                }
            }
        }
        decisions.insert(c.id.clone(), d);
    }
    entries.sort_by(|(a, ae), (b, be)| be.cmp(ae).then(a.id.cmp(&b.id)));
    let mut added = 0;
    for (c, _) in entries {
        let r = c.research.as_ref().unwrap(); let b = c.book.as_ref().unwrap();
        let group_budget = c.risk_groups.iter().map(|g| Ok(amount(&p.group_caps[g])?-group_used.get(g).copied().unwrap_or(0))).collect::<Result<Vec<i128>, String>>()?.into_iter().min().unwrap().max(0);
        let cap = budget.min(group_budget).min(amount(&p.max_order_cost)?);
        let mut chosen_q = 0; let mut chosen_cost = 0; let mut chosen_limit = 0; let mut available = 0; let mut actual_cost = 0;
        for l in &b.asks {
            let v = price(&l.price)? + p.slippage_ticks*TICK;
            if v >= SCALE || amount(&r.lower)?-v < amount(&p.minimum_edge)? { break; }
            let capacity = l.quantity as i128*p.depth_fraction_bps/10000;
            let kelly_cost = capital*(amount(&r.lower)?-v)*p.fractional_kelly_bps/(SCALE-v)/10000;
            let q = if i.receipt_policy.is_some() {(available+capacity).min(cap/v).min(1)} else {(available+capacity).min(cap.min(kelly_cost)/v)};
            if q > chosen_q && q >= available { chosen_q = q; chosen_cost = actual_cost+(q-available)*v; chosen_limit = v; }
            available += capacity; actual_cost += capacity*v;
            if q < available { break; }
        }
        let d = decisions.get_mut(&c.id).unwrap();
        if chosen_q == 0 { d["reason"] = json!(if cap == 0 {"risk_budget"} else {"insufficient_depth_or_fractional_size"}); continue; }
        let maximum_cost = chosen_q*chosen_limit;
        need(maximum_cost <= cap, "RESERVATION_BUDGET")?;
        budget -= maximum_cost; added += maximum_cost;
        for g in &c.risk_groups { *group_used.entry(g.clone()).or_default() += maximum_cost; }
        d["action"] = json!("candidate_buy"); d["reason"] = json!("positive_settlement_scenario_edge"); d["quantity"] = json!(chosen_q);
        d["hardLimitWithCushion"] = json!(money(chosen_limit)); d["maximumCost"] = json!(money(maximum_cost)); d["depthWeightedCostWithCushion"] = json!(money(chosen_cost));
        d["scenarioSettlementValueRange"] = json!([money(chosen_q*amount(&r.lower)?),money(chosen_q*amount(&r.upper)?)]);
        d["scenarioSettlementSurplusRange"] = json!([money(chosen_q*amount(&r.lower)?-maximum_cost),money(chosen_q*amount(&r.upper)?-maximum_cost)]);
        d["conditionalOrdinaryOutcomePnl"] = json!({"win":money(chosen_q*SCALE-maximum_cost),"lose":money(-maximum_cost)});
    }
    Ok(json!({"schemaVersion":if i.receipt_policy.is_some(){2}else{1},"mode":if i.receipt_policy.is_some(){"probability_receipt_policy_diagnostic_v1"}else{"settlement_value_policy_diagnostic_v1"},"asOfMs":i.as_of_ms,"engineHash":env!("BAAL_ENGINE_HASH"),
        "decisions":decisions.into_values().collect::<Vec<_>>(),"heldAcquisitionRisk":money(held),"existingReservedCost":money(reserved),"riskBreaches":risk_breaches,
        "initialAdmissionBudget":money(initial_budget),"proposedMaximumCost":money(added),"remainingAdmissionBudget":money(budget),"groupExposureAfterReservations":group_used.into_iter().map(|(g,n)|(g,money(n))).collect::<BTreeMap<_,_>>(),
        "fees":0,"fills":0,"realizedPnl":null,"competitionOrders":0,"networkRequests":0,"executionAuthority":false,"profitabilityEstablished":false,
        "limitations":["Probability intervals and predicate review are supplied attestations; subjective endpoints are not confidence bounds or guaranteed payouts.",
        "Greedy fractional Kelly based on the lower supplied scenario is a bounded sizing heuristic, not a joint optimal portfolio or calibrated loss probability.",
        "All correlated exposures must be declared in risk groups; missing economic correlations cannot be inferred from identifiers.",
        "Checked depth and slippage bound a proposed acquisition reservation; there is no later-request confirmation, queue model, order or fill.",
        if i.receipt_policy.is_some(){"Receipt strategy exits use the same adverse tick, fixed profit/loss thresholds and two-day horizon as the frozen paper strategy. A proposal is not a subsequent fill or profit."}else{"Holding policy has no arbitrary two-day timeout or price-only stop. Invalid or expired research requires review and preserves held risk without estimating a payout."},
        "Canonical NO complements describe payoff and book diagnostics, not an authenticated REST NO submission price basis.",
        "Liquidation estimates require the entire held quantity at fractional sampled depth. Settlement scenario value is separate from realized and liquidation profit."]}))
}
