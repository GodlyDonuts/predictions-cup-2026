// A predeclared causal parameter grid. Historical parameter searches are
// development evidence, never proof of independent prospective profitability.
use crate::{
    engine::{Point, Wallet},
    money::{SCALE, TICK, fee},
    types::*,
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Instant,
};
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Parameters {
    pub lookback: usize,
    pub gap_reset_seconds: i64,
    pub z: f64,
    pub minimum_edge: f64,
    pub horizon_seconds: i64,
    pub take_profit: String,
}
impl Parameters {
    pub fn validate(&self) -> Result<(), String> {
        if self.lookback < 1
            || self.lookback > 1000
            || self.gap_reset_seconds < 1
            || self.gap_reset_seconds > 86400
            || !self.z.is_finite()
            || !(0.0..=10.0).contains(&self.z)
            || !self.minimum_edge.is_finite()
            || self.minimum_edge <= 0.0
            || self.minimum_edge > 1.0
            || self.horizon_seconds < 1
            || self.horizon_seconds > 86400
            || !(1..=100000).contains(&crate::money::amount(&self.take_profit)?)
        {
            return Err("INVALID_DIP_PARAMETERS".into());
        }
        Ok(())
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    parameters: Parameters,
    points: Vec<Point>,
}
enum SharedObservation {
    Book(Arc<Book>),
    Invalid { market_id: String, at: i64 },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SweepResult {
    pub mode: &'static str,
    pub source_hash: String,
    pub source_records: usize,
    pub capture_days: f64,
    pub candidates: usize,
    pub workers: usize,
    pub replay_ms: u128,
    pub selection_policy: &'static str,
    pub scorecards: Vec<Card>,
    pub competition_orders: u32,
    pub network_requests: u32,
    pub profitability_established: bool,
}
pub fn run(source: &Capture, c: &Config, workers: usize) -> Result<SweepResult, String> {
    if workers < 1 || workers > 32 || c.fee_bps != 0 {
        return Err("INVALID_ZERO_FEE_SWEEP_CONFIG".into());
    }
    let cycles: Vec<_> = source.frames.iter().filter(|f| f.is_cycle).collect();
    if cycles.len() < 30 {
        return Err("INSUFFICIENT_SWEEP_CYCLES".into());
    }
    let checks = [
        cycles[cycles.len() * 6 / 10 - 1].sequence,
        cycles[cycles.len() * 8 / 10 - 1].sequence,
        source.frames.len(),
    ];
    let mut parameters = Vec::new();
    for gap_reset_seconds in [120, 900, 3600, 21600] {
        for lookback in [6, 12, 24, 48] {
            for z in [0.0, 0.5, 1.0, 1.5, 2.0, 2.5] {
                for minimum_edge in [0.005, 0.01, 0.02, 0.03] {
                    for horizon_seconds in [60, 180, 300, 900, 1800, 3600] {
                        parameters.push(Parameters {
                            lookback,
                            gap_reset_seconds,
                            z,
                            minimum_edge,
                            horizon_seconds,
                            take_profit: "0.005000".into(),
                        });
                    }
                }
            }
        }
    }
    // Share immutable book allocations across every wallet, without API traffic.
    let frames: Vec<_> = source
        .frames
        .iter()
        .map(|f| {
            (
                f,
                f.observations
                    .iter()
                    .map(|o| match o {
                        Observation::Book(b) => SharedObservation::Book(Arc::new(b.clone())),
                        Observation::Invalid { market_id, at } => SharedObservation::Invalid {
                            market_id: market_id.clone(),
                            at: *at,
                        },
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()
        .map_err(|_| "REPLAY_POOL_FAILED")?;
    let began = Instant::now();
    let cards = pool.install(|| {
        parameters
            .par_iter()
            .map(|p| -> Result<Card, String> {
                let mut wallet = Wallet::new(c, source.meta.first_at)?;
                let mut histories: HashMap<String, VecDeque<(i64, f64, String)>> = HashMap::new();
                let mut versions: HashMap<String, (u64, i64)> = HashMap::new();
                let mut points = Vec::new();
                let mut checkpoint = 0;
                for (f, observations) in &frames {
                    if f.account_outage {
                        wallet.invalidate(None, f.at);
                        histories.clear();
                    }
                    for o in observations {
                        match o {
                            SharedObservation::Invalid { market_id, at } => {
                                wallet.invalidate(Some(market_id), *at);
                                histories
                                    .retain(|_, h| h.front().is_none_or(|v| v.2 != *market_id));
                            }
                            SharedObservation::Book(b) => {
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
                                if h.len() >= p.lookback {
                                    let mean = h.iter().map(|v| v.1).sum::<f64>() / h.len() as f64;
                                    let sd = (h.iter().map(|v| (v.1 - mean).powi(2)).sum::<f64>()
                                        / h.len() as f64)
                                        .sqrt();
                                    let ask = b.asks[0].price + TICK * c.slippage_ticks;
                                    // Charge both entry and eventual exit slippage. Spread is present in ask.
                                    let edge = mean
                                        - (ask + fee(ask, c.fee_bps) + TICK * c.slippage_ticks)
                                            as f64
                                            / SCALE as f64;
                                    if sd > 0.0 && (mean - mid) / sd >= p.z {
                                        net = Some(edge);
                                    }
                                }
                                if wallet.step_dip(
                                    b.clone(),
                                    c,
                                    net,
                                    p.horizon_seconds * 1000,
                                    p.minimum_edge,
                                    5000,
                                )? {
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
                    if checkpoint < checks.len() && f.sequence == checks[checkpoint] {
                        wallet.verify()?;
                        points.push(wallet.point(c, f.at)?);
                        checkpoint += 1;
                    }
                }
                if points.len() != 3 {
                    return Err("MISSING_SWEEP_CHECKPOINT".into());
                }
                Ok(Card {
                    parameters: p.clone(),
                    points,
                })
            })
            .collect::<Result<Vec<_>, _>>()
    })?;
    Ok(SweepResult {
        mode: "causal_multiday_dip_development_grid_v2",
        source_hash: source.meta.source_hash.clone(),
        source_records: source.meta.source_records,
        capture_days: (source.meta.through - source.meta.first_at) as f64 / 86_400_000.0,
        candidates: cards.len(),
        workers,
        replay_ms: began.elapsed().as_millis(),
        selection_policy: "predeclared_grid_all_results_no_automatic_winner_or_activation",
        scorecards: cards,
        competition_orders: 0,
        network_requests: 0,
        profitability_established: false,
    })
}
