// Frozen subsequent-data comparison. Prior data warms causal features only;
// it never contributes positions, fills or cash to an evaluation wallet.
use crate::{
    engine::{DipExitPolicy, Fill, Point, Wallet},
    money::{SCALE, TICK, amount, money},
    sweep::Parameters,
    types::*,
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::Instant,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Development {
    pub records: usize,
    pub bytes: u64,
    pub hash: String,
    pub through: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Strategy {
    pub id: String,
    pub parameters: Parameters,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Scenario {
    pub id: String,
    pub latency_ms: i64,
    pub slippage_ticks: i128,
    pub depth_fraction_bps: i64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Declaration {
    pub schema_version: u32,
    pub mode: String,
    pub cohort: String,
    pub declared_at: String,
    pub config: Config,
    pub development: Development,
    pub strategies: Vec<Strategy>,
    pub scenarios: Vec<Scenario>,
    pub stop_buffer_ticks: u8,
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}
impl Declaration {
    pub fn validate(&self, source: &Capture) -> Result<i64, String> {
        self.config.validate()?;
        if self.schema_version != 1
            || !matches!(
                self.mode.as_str(),
                "causal_dip_subsequent_v1" | "causal_dip_development_diagnostic_v1"
            )
            || self.config.fee_bps != 0
            || !identifier(&self.cohort)
            || self.strategies.is_empty()
            || self.strategies.len() > 64
            || self.scenarios.is_empty()
            || self.scenarios.len() > 16
            || self.stop_buffer_ticks < 1
            || self.stop_buffer_ticks > 5
            || self.development.records < 1
            || self.development.records >= source.frames.len()
        {
            return Err("INVALID_EVALUATION_DECLARATION".into());
        }
        let cutoff = instant(&self.development.through)?;
        if instant(&self.declared_at)? <= cutoff
            || source.frames[self.development.records - 1].at != cutoff
            || source.prefix_hashes[self.development.records - 1] != self.development.hash
            || source.prefix_bytes[self.development.records - 1] != self.development.bytes
        {
            return Err("EVALUATION_DEVELOPMENT_BINDING_MISMATCH".into());
        }
        let unique = |ids: Vec<&str>| {
            ids.iter().all(|id| identifier(id))
                && ids.iter().collect::<HashSet<_>>().len() == ids.len()
        };
        if !unique(self.strategies.iter().map(|s| s.id.as_str()).collect())
            || !unique(self.scenarios.iter().map(|s| s.id.as_str()).collect())
        {
            return Err("DUPLICATE_EVALUATION_ID".into());
        }
        for s in &self.strategies {
            s.parameters.validate()?;
        }
        for s in &self.scenarios {
            let mut c = self.config.clone();
            c.latency_ms = s.latency_ms;
            c.slippage_ticks = s.slippage_ticks;
            c.depth_fraction_bps = s.depth_fraction_bps;
            c.validate()?;
        }
        Ok(cutoff)
    }
}
enum Shared {
    Book(Arc<Book>),
    Invalid(String, i64),
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchedTrade {
    instrument: String,
    quantity: i64,
    entry_at: String,
    exit_at: String,
    held_ms: i64,
    exit_reason: String,
    net_pnl: String,
    mid_move: String,
    quoted_spread_and_ladder_cost: String,
    slippage_cost: String,
    fees: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Economics {
    matched_shares: i64,
    matched_rows: usize,
    net_pnl: String,
    mid_move: String,
    quoted_spread_and_ladder_cost: String,
    slippage_cost: String,
    fees: String,
    wins: usize,
    losses: usize,
    flat: usize,
    by_exit_reason: HashMap<String, String>,
    trades: Vec<MatchedTrade>,
}
#[derive(Clone)]
struct Lot {
    quantity: i64,
    price: i128,
    mid: i128,
    quote: i128,
    fee: i128,
    at: i64,
}
pub fn economics(fills: &[Fill], point: &Point) -> Result<Economics, String> {
    let mut lots: HashMap<&str, VecDeque<Lot>> = HashMap::new();
    let mut trades = Vec::new();
    let mut total = 0;
    let mut mid_total = 0;
    let mut spread_total = 0;
    let mut slip_total = 0;
    let mut fees_total = 0;
    let mut shares = 0;
    let mut by: HashMap<String, i128> = HashMap::new();
    let mut wins = 0;
    let mut losses = 0;
    let mut flat = 0;
    for f in fills {
        let audit = f.economics.as_ref().ok_or("MISSING_FILL_ECONOMICS")?;
        let price = amount(&f.price)?;
        let quote = amount(&audit.quote_price)?;
        let mid = amount(&audit.mid_price)?;
        let charge = amount(&f.fee)?;
        if f.quantity < 1 || f.at <= audit.decision_at || audit.decision_at < 0 {
            return Err("INVALID_FILL_ECONOMICS".into());
        }
        let ledger = lots.entry(&f.instrument).or_default();
        if f.action == "buy" {
            ledger.push_back(Lot {
                quantity: f.quantity,
                price,
                mid,
                quote,
                fee: charge,
                at: f.at,
            });
            continue;
        }
        if f.action != "sell" {
            return Err("INVALID_FILL_ACTION".into());
        }
        let mut left = f.quantity;
        let mut remaining_fee = charge;
        while left > 0 {
            let l = ledger.front_mut().ok_or("ECONOMICS_OVERSELL")?;
            let qty = left.min(l.quantity);
            let q = qty as i128;
            let buy_fee = if qty == l.quantity {
                l.fee
            } else {
                l.fee * q / l.quantity as i128
            };
            let sell_fee = if qty == left {
                remaining_fee
            } else {
                remaining_fee * q / left as i128
            };
            let fees = buy_fee + sell_fee;
            let pnl = (price - l.price) * q - fees;
            let movement = (mid - l.mid) * q;
            let spread = ((l.quote - l.mid) + (mid - quote)) * q;
            let slip = ((l.price - l.quote) + (quote - price)) * q;
            if pnl != movement - spread - slip - fees || slip < 0 {
                return Err("ECONOMICS_RECONCILIATION_FAILED".into());
            }
            total += pnl;
            mid_total += movement;
            spread_total += spread;
            slip_total += slip;
            fees_total += fees;
            shares += qty;
            *by.entry(audit.reason.into()).or_default() += pnl;
            if pnl > 0 {
                wins += 1;
            } else if pnl < 0 {
                losses += 1;
            } else {
                flat += 1;
            }
            trades.push(MatchedTrade {
                instrument: f.instrument.clone(),
                quantity: qty,
                entry_at: crate::engine::iso(l.at),
                exit_at: crate::engine::iso(f.at),
                held_ms: f.at - l.at,
                exit_reason: audit.reason.into(),
                net_pnl: money(pnl),
                mid_move: money(movement),
                quoted_spread_and_ladder_cost: money(spread),
                slippage_cost: money(slip),
                fees: money(fees),
            });
            l.fee -= buy_fee;
            l.quantity -= qty;
            remaining_fee -= sell_fee;
            left -= qty;
            if l.quantity == 0 {
                ledger.pop_front();
            }
        }
    }
    if total != amount(&point.realized_pnl)? {
        return Err("ECONOMICS_REALIZED_MISMATCH".into());
    }
    Ok(Economics {
        matched_shares: shares,
        matched_rows: trades.len(),
        net_pnl: money(total),
        mid_move: money(mid_total),
        quoted_spread_and_ladder_cost: money(spread_total),
        slippage_cost: money(slip_total),
        fees: money(fees_total),
        wins,
        losses,
        flat,
        by_exit_reason: by.into_iter().map(|(k, v)| (k, money(v))).collect(),
        trades,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    strategy: String,
    parameters: Parameters,
    scenario: Scenario,
    exit_policy: &'static str,
    point: Point,
    signal_diagnostics: SignalDiagnostics,
    economics: Economics,
    fills: Vec<Fill>,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalDiagnostics {
    warmup: u64,
    flat_history: u64,
    below_z_threshold: u64,
    cost_adjusted_margin_fail: u64,
    edge_qualified: u64,
    maximum_historical_reversion: Option<f64>,
    maximum_cost_adjusted_edge: Option<f64>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Evaluation {
    pub mode: String,
    pub cohort: String,
    pub evaluation_start: String,
    pub evaluation_through: String,
    pub development_records: usize,
    pub subsequent_records: usize,
    pub subsequent_cycles: usize,
    pub workers: usize,
    pub replay_ms: u128,
    pub candidates: usize,
    pub scorecards: Vec<Card>,
    pub wallet_initialization: &'static str,
    pub evaluation_classification: &'static str,
    pub competition_orders: u32,
    pub network_requests: u32,
    pub profitability_established: bool,
}
pub fn run(source: &Capture, d: &Declaration, workers: usize) -> Result<Evaluation, String> {
    let cutoff = d.validate(source)?;
    if workers < 1 || workers > 32 {
        return Err("INVALID_REPLAY_WORKERS".into());
    }
    let frames: Vec<_> = source
        .frames
        .iter()
        .map(|f| {
            (
                f,
                f.observations
                    .iter()
                    .map(|o| match o {
                        Observation::Book(b) => Shared::Book(Arc::new(b.clone())),
                        Observation::Invalid { market_id, at } => {
                            Shared::Invalid(market_id.clone(), *at)
                        }
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let jobs: Vec<_> = d
        .strategies
        .iter()
        .flat_map(|s| {
            d.scenarios.iter().flat_map(move |scenario| {
                [false, true]
                    .into_iter()
                    .map(move |improved| (s, scenario, improved))
            })
        })
        .collect();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()
        .map_err(|_| "REPLAY_POOL_FAILED")?;
    let started = Instant::now();
    let scorecards = pool.install(|| {
        jobs.par_iter()
            .map(|(strategy, scenario, improved)| -> Result<Card, String> {
                let mut c = d.config.clone();
                c.latency_ms = scenario.latency_ms;
                c.slippage_ticks = scenario.slippage_ticks;
                c.depth_fraction_bps = scenario.depth_fraction_bps;
                let p = &strategy.parameters;
                let policy = if *improved {
                    DipExitPolicy::ProfitFloorStopBuffer(d.stop_buffer_ticks)
                } else {
                    DipExitPolicy::SnapshotLimit
                };
                let mut wallet = Wallet::new(&c, cutoff)?;
                let mut diagnostics = SignalDiagnostics::default();
                let mut histories: HashMap<String, VecDeque<(i64, f64, String)>> = HashMap::new();
                let mut versions: HashMap<String, (u64, i64)> = HashMap::new();
                for (f, observations) in &frames {
                    if f.account_outage {
                        histories.clear();
                        if f.sequence > d.development.records {
                            wallet.invalidate(None, f.at);
                        }
                    }
                    for o in observations {
                        match o {
                            Shared::Invalid(market, at) => {
                                histories.retain(|_, h| h.front().is_none_or(|v| v.2 != *market));
                                if f.sequence > d.development.records && *at > cutoff {
                                    wallet.invalidate(Some(market), *at);
                                }
                            }
                            Shared::Book(b) => {
                                let h = histories.entry(b.instrument.clone()).or_default();
                                if h.back().is_some_and(|v| {
                                    b.received_at - v.0 > p.gap_reset_seconds * 1000
                                }) {
                                    h.clear();
                                }
                                let same =
                                    versions.get(&b.instrument) == Some(&(b.sequence, b.source_ns));
                                let mid =
                                    (b.bids[0].price + b.asks[0].price) as f64 / 2.0 / SCALE as f64;
                                let mut net = None;
                                let mut gate = 0;
                                let mut reversion = None;
                                let mut cost_edge = None;
                                if h.len() >= p.lookback {
                                    let mean = h.iter().map(|v| v.1).sum::<f64>() / h.len() as f64;
                                    let sd = (h.iter().map(|v| (v.1 - mean).powi(2)).sum::<f64>()
                                        / h.len() as f64)
                                        .sqrt();
                                    reversion = Some(mean - mid);
                                    cost_edge = Some(
                                        mean - (b.asks[0].price + 2 * TICK * c.slippage_ticks)
                                            as f64
                                            / SCALE as f64,
                                    );
                                    gate = if sd <= 0.0 { 1 } else { 2 };
                                    if sd > 0.0 && (mean - mid) / sd >= p.z {
                                        net = cost_edge;
                                        gate = if net.is_some_and(|v| v >= p.minimum_edge) {
                                            4
                                        } else {
                                            3
                                        };
                                    }
                                }
                                if f.sequence > d.development.records && b.received_at > cutoff {
                                    if wallet.step_dip_policy(
                                        b.clone(),
                                        &c,
                                        net,
                                        p.horizon_seconds * 1000,
                                        p.minimum_edge,
                                        amount(&p.take_profit)?,
                                        policy,
                                    )? {
                                        match gate {
                                            0 => diagnostics.warmup += 1,
                                            1 => diagnostics.flat_history += 1,
                                            2 => diagnostics.below_z_threshold += 1,
                                            3 => diagnostics.cost_adjusted_margin_fail += 1,
                                            _ => diagnostics.edge_qualified += 1,
                                        }
                                        if let Some(v) = reversion {
                                            diagnostics.maximum_historical_reversion = Some(
                                                diagnostics
                                                    .maximum_historical_reversion
                                                    .map_or(v, |prior| prior.max(v)),
                                            );
                                        }
                                        if let Some(v) = cost_edge {
                                            diagnostics.maximum_cost_adjusted_edge = Some(
                                                diagnostics
                                                    .maximum_cost_adjusted_edge
                                                    .map_or(v, |prior| prior.max(v)),
                                            );
                                        }
                                        wallet.metrics.observations += 1;
                                        if h.len() < p.lookback {
                                            wallet.metrics.warmup += 1;
                                        } else if net.is_some_and(|v| v >= p.minimum_edge) {
                                            wallet.metrics.edge_pass += 1;
                                        } else {
                                            wallet.metrics.edge_below_margin += 1;
                                        }
                                        if b.asks[0].quantity as i128 * c.depth_fraction_bps as i128
                                            / 10000
                                            < 1
                                        {
                                            wallet.metrics.zero_top_ask_capacity += 1;
                                        }
                                    }
                                }
                                if !same {
                                    h.push_back((b.received_at, mid, b.market_id.clone()));
                                    while h.len() > p.lookback {
                                        h.pop_front();
                                    }
                                    versions
                                        .insert(b.instrument.clone(), (b.sequence, b.source_ns));
                                }
                            }
                        }
                    }
                }
                wallet.verify()?;
                let point = wallet.point(&c, source.meta.through)?;
                let audit = economics(&wallet.fills, &point)?;
                Ok(Card {
                    strategy: strategy.id.clone(),
                    parameters: p.clone(),
                    scenario: (*scenario).clone(),
                    exit_policy: if *improved {
                        "profit_floor_stop_buffer_v1"
                    } else {
                        "snapshot_limit_v1"
                    },
                    point,
                    signal_diagnostics: diagnostics,
                    economics: audit,
                    fills: wallet.fills,
                })
            })
            .collect::<Result<Vec<_>, _>>()
    })?;
    Ok(Evaluation {
        mode: d.mode.clone(),
        cohort: d.cohort.clone(),
        evaluation_start: crate::engine::iso(cutoff),
        evaluation_through: crate::engine::iso(source.meta.through),
        development_records: d.development.records,
        subsequent_records: source.frames.len() - d.development.records,
        subsequent_cycles: source
            .frames
            .iter()
            .filter(|f| f.sequence > d.development.records && f.is_cycle)
            .count(),
        workers,
        replay_ms: started.elapsed().as_millis(),
        candidates: scorecards.len(),
        scorecards,
        wallet_initialization: "fresh_cash_no_development_positions_fills_or_pnl_prior_features_only",
        evaluation_classification: if d.mode == "causal_dip_development_diagnostic_v1" {
            "reused_development_data_cost_diagnostic_not_holdout_or_profit_proof"
        } else {
            "retrospective_subsequent_data_selection_provenance_external_not_live_prospective_trading"
        },
        competition_orders: 0,
        network_requests: 0,
        profitability_established: false,
    })
}
