use crate::{
    engine::{Point, Wallet},
    types::*,
};
use rayon::prelude::*;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::Instant,
};
#[derive(Clone)]
struct Model {
    id: String,
    horizon: i64,
    rmse: f64,
    weights: Vec<f64>,
    means: Vec<f64>,
    scales: Vec<f64>,
    supported: HashSet<String>,
    stumps: Option<Value>,
    base: f64,
}
impl Model {
    fn from_value(v: &Value) -> Result<Self, String> {
        let string = |k: &str| {
            v[k].as_str()
                .map(str::to_owned)
                .ok_or_else(|| "INVALID_MODEL".to_owned())
        };
        let list = |k: &str| -> Result<Vec<f64>, String> {
            v[k].as_array()
                .ok_or("INVALID_MODEL")?
                .iter()
                .map(|x| {
                    x.as_f64()
                        .filter(|n| n.is_finite())
                        .ok_or_else(|| "INVALID_MODEL".into())
                })
                .collect()
        };
        let family = v["family"].as_str().unwrap_or("ridge");
        if !["ridge", "boosted_stumps"].contains(&family) {
            return Err("UNSUPPORTED_MODEL_FAMILY".into());
        }
        let width = v["width"].as_u64().ok_or("INVALID_MODEL_WIDTH")? as usize;
        if width == 0 || width > 12 {
            return Err("UNSUPPORTED_FEATURE_WIDTH".into());
        }
        let (weights, means, scales) = if family == "ridge" {
            let w = list("weights")?;
            let m = list("means")?;
            let s = list("scales")?;
            if w.len() != width
                || m.len() != width
                || s.len() != width
                || s.iter().any(|x| *x <= 0.0)
            {
                return Err("INVALID_MODEL".into());
            }
            (w, m, s)
        } else {
            (Vec::new(), Vec::new(), Vec::new())
        };
        let mut supported = HashSet::new();
        for c in v["coverage"].as_array().ok_or("MISSING_MODEL_COVERAGE")? {
            if c["eligible"].as_bool() == Some(true) {
                let market = c["marketId"].as_str().ok_or("INVALID_MODEL_COVERAGE")?;
                for id in c["trainingInstruments"]
                    .as_array()
                    .ok_or("INVALID_MODEL_COVERAGE")?
                {
                    supported.insert(format!(
                        "{market}:{}",
                        id.as_str().ok_or("INVALID_MODEL_COVERAGE")?
                    ));
                }
            }
        }
        let horizon = v["horizon"].as_i64().ok_or("INVALID_MODEL_HORIZON")?;
        if !(1..=86400).contains(&horizon) {
            return Err("INVALID_MODEL_HORIZON".into());
        }
        Ok(Self {
            id: string("id")?,
            horizon: horizon * 1000,
            rmse: v["rmse"].as_f64().ok_or("INVALID_MODEL")?,
            weights,
            means,
            scales,
            supported,
            stumps: if family == "boosted_stumps" {
                Some(v["stumps"].clone())
            } else {
                None
            },
            base: v["base"].as_f64().unwrap_or(0.0),
        })
    }
    fn predict(&self, x: &[f64; 12]) -> Result<f64, String> {
        if let Some(stumps) = &self.stumps {
            let mut value = self.base;
            for s in stumps.as_array().ok_or("INVALID_STUMPS")? {
                let j = s["feature"].as_u64().ok_or("INVALID_STUMPS")? as usize;
                if j >= 12 {
                    return Err("INVALID_STUMPS".into());
                }
                value += if x[j] <= s["threshold"].as_f64().ok_or("INVALID_STUMPS")? {
                    s["leftValue"].as_f64().ok_or("INVALID_STUMPS")?
                } else {
                    s["rightValue"].as_f64().ok_or("INVALID_STUMPS")?
                };
            }
            return Ok(value);
        }
        Ok(self.weights.iter().enumerate().fold(0.0, |sum, (j, w)| {
            sum + w * (x[j] - self.means[j]) / self.scales[j]
        }))
    }
}
#[derive(Clone)]
enum Prepared {
    Book {
        book: Arc<Book>,
        x: [f64; 12],
        history: usize,
    },
    Invalid {
        market: String,
        at: i64,
    },
    Outage {
        at: i64,
    },
}
struct PreparedFrame {
    sequence: usize,
    at: i64,
    events: Vec<Prepared>,
}
fn support_key(b: &Book) -> String {
    format!("{}:{}", b.market_id, b.instrument)
}
fn prepare(capture: &Capture, models: &[Model], start: usize, end: usize) -> Vec<PreparedFrame> {
    let covered: HashSet<_> = models
        .iter()
        .flat_map(|m| m.supported.iter().cloned())
        .collect();
    let mut histories: HashMap<String, VecDeque<(i64, f64, String)>> = HashMap::new();
    let mut known: HashMap<String, Arc<Book>> = HashMap::new();
    let mut out = Vec::new();
    let cutoff = capture.frames[start - 1].at;
    for f in &capture.frames[start..end] {
        let mut events = Vec::new();
        if f.at <= cutoff {
            continue;
        }
        if f.account_outage {
            histories.clear();
            events.push(Prepared::Outage { at: f.at });
        }
        for obs in &f.observations {
            match obs {
                Observation::Invalid { market_id, at } => {
                    histories.retain(|_, h| h.front().is_none_or(|x| x.2 != *market_id));
                    events.push(Prepared::Invalid {
                        market: market_id.clone(),
                        at: *at,
                    });
                }
                Observation::Book(b) => {
                    if b.request_started_at <= cutoff {
                        continue;
                    }
                    let b = Arc::new(b.clone());
                    let supported = covered.contains(&support_key(&b));
                    let h = histories.entry(b.instrument.clone()).or_default();
                    if h.back().is_some_and(|x| b.received_at - x.0 > 120000) {
                        h.clear();
                    }
                    let bid = b.bids[0].price as f64 / 1e6;
                    let ask = b.asks[0].price as f64 / 1e6;
                    let mid = (bid + ask) / 2.0;
                    let sum_bid = b
                        .bids
                        .iter()
                        .take(3)
                        .map(|l| l.quantity as f64)
                        .sum::<f64>();
                    let sum_ask = b
                        .asks
                        .iter()
                        .take(3)
                        .map(|l| l.quantity as f64)
                        .sum::<f64>();
                    let delta = |n: usize| {
                        if h.len() >= n {
                            mid - h[h.len() - n].1
                        } else {
                            0.0
                        }
                    };
                    let top_bid = b.bids[0].quantity as f64;
                    let top_ask = b.asks[0].quantity as f64;
                    let x = [
                        1.0,
                        ask - bid,
                        delta(1),
                        delta(3),
                        delta(6),
                        (top_bid - top_ask) / (top_bid + top_ask),
                        (sum_bid - sum_ask) / (sum_bid + sum_ask),
                        sum_bid.ln_1p(),
                        sum_ask.ln_1p(),
                        (b.received_at - b.source_ns / 1_000_000) as f64 / 1000.0,
                        h.back()
                            .map_or(0.0, |v| (b.received_at - v.0) as f64 / 1000.0),
                        mid,
                    ];
                    let history = h.len();
                    let same = known.get(&b.instrument).is_some_and(|p| p.same_version(&b));
                    if supported && !same {
                        h.push_back((b.received_at, mid, b.market_id.clone()));
                        if h.len() > 30 {
                            h.pop_front();
                        }
                    }
                    if supported {
                        known.insert(b.instrument.clone(), b.clone());
                    }
                    events.push(Prepared::Book {
                        book: b,
                        x,
                        history,
                    });
                }
            }
        }
        out.push(PreparedFrame {
            sequence: f.sequence,
            at: f.at,
            events,
        });
    }
    out
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scorecard {
    pub id: String,
    pub model_id: String,
    pub margin: f64,
    pub rmse: f64,
    pub points: Vec<Point>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayResult {
    pub mode: &'static str,
    pub source_hash: String,
    pub source_records: usize,
    pub bank_source_binding: bool,
    pub candidates: usize,
    pub workers: usize,
    pub start_count: usize,
    pub end_count: usize,
    pub checkpoints: Vec<usize>,
    pub prepare_ms: u128,
    pub replay_ms: u128,
    pub scorecards: Vec<Scorecard>,
    pub competition_orders: u32,
    pub network_requests: u32,
    pub profitability_established: bool,
}
fn checkpoints(capture: &Capture, start: usize, end: usize) -> Result<Vec<usize>, String> {
    let cycles: Vec<_> = capture.frames[start..end]
        .iter()
        .filter(|f| f.is_cycle)
        .collect();
    if cycles.len() < 6 {
        return Err("INSUFFICIENT_VALIDATION_CYCLES".into());
    }
    let cut_after = |at| {
        capture
            .frames
            .iter()
            .position(|f| f.at > at)
            .unwrap_or(capture.frames.len())
            .min(end)
    };
    let first = cut_after(cycles[cycles.len() / 3 - 1].at);
    let second = cut_after(cycles[cycles.len() * 2 / 3 - 1].at);
    if first <= start || second <= first || second >= end {
        return Err("INVALID_VALIDATION_CHECKPOINTS".into());
    }
    Ok(vec![first, second, end])
}
pub fn replay_validation(
    capture: &Capture,
    bank: &Value,
    c: &Config,
    workers: usize,
) -> Result<ReplayResult, String> {
    if workers < 1 || workers > 32 {
        return Err("INVALID_REPLAY_WORKERS".into());
    }
    if bank["targetPolicy"].as_str() != Some("delayed_fifo_roundtrip_net_v1")
        || bank["historyPolicy"].as_str() != Some("market-isolated-v2")
        || bank["forecastLifecycle"].as_str() != Some("revoke-on-invalidation-v2")
        || bank["breadthContext"].as_bool() != Some(false)
    {
        return Err("UNSUPPORTED_FROZEN_BANK_POLICY".into());
    }
    let count = bank["sourceRecords"]
        .as_u64()
        .ok_or("MISSING_SOURCE_BINDING")? as usize;
    if count == 0
        || count > capture.frames.len()
        || capture.prefix_hashes[count - 1]
            != bank["sourceHash"]
                .as_str()
                .ok_or("MISSING_SOURCE_BINDING")?
        || capture.frames[count - 1].at
            != instant(
                bank["trainingCaptureThrough"]
                    .as_str()
                    .ok_or("MISSING_SOURCE_BINDING")?,
            )?
    {
        return Err("FROZEN_SOURCE_MISMATCH".into());
    }
    let window = &bank["portfolioSelection"]["windows"];
    let start = window["fitRecords"]
        .as_u64()
        .ok_or("MISSING_PORTFOLIO_WINDOWS")? as usize;
    let end = window["selectionRecords"]
        .as_u64()
        .ok_or("MISSING_PORTFOLIO_WINDOWS")? as usize;
    if start == 0
        || start >= end
        || end > count
        || capture.prefix_hashes[start - 1]
            != bank["portfolioSelection"]["fitSourceHash"]
                .as_str()
                .ok_or("MISSING_FIT_PREFIX")?
        || capture.prefix_hashes[end - 1]
            != bank["portfolioSelection"]["developmentSourceHash"]
                .as_str()
                .ok_or("MISSING_DEVELOPMENT_PREFIX")?
        || bank["portfolioSelection"]["policy"]["validationBlocks"].as_u64() != Some(3)
    {
        return Err("INVALID_FIT_PREFIX".into());
    }
    let models: Vec<_> = bank["models"]
        .as_array()
        .ok_or("MISSING_MODELS")?
        .iter()
        .map(Model::from_value)
        .collect::<Result<_, _>>()?;
    if models.is_empty()
        || models.len() > 4096
        || models.iter().map(|m| &m.id).collect::<HashSet<_>>().len() != models.len()
    {
        return Err("INVALID_MODEL_UNIVERSE".into());
    }
    let checks = checkpoints(capture, start, end)?;
    let started = Instant::now();
    let frames = prepare(capture, &models, start, end);
    let prepare_ms = started.elapsed().as_millis();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()
        .map_err(|_| "REPLAY_POOL_FAILED")?;
    let started = Instant::now();
    let mut cards = pool
        .install(|| {
            models
                .par_iter()
                .map(|m| -> Result<Vec<Scorecard>, String> {
                    let mut cards = Vec::new();
                    for margin in [0.005, 0.01, 0.015] {
                        let mut wallet = Wallet::new(c, capture.frames[start - 1].at)?;
                        let mut points = Vec::new();
                        let mut checkpoint = 0;
                        for f in &frames {
                            for event in &f.events {
                                match event {
                                    Prepared::Outage { at } => wallet.invalidate(None, *at),
                                    Prepared::Invalid { market, at } => {
                                        wallet.invalidate(Some(market), *at)
                                    }
                                    Prepared::Book { book, x, history } => {
                                        if !m.supported.contains(&support_key(book)) {
                                            wallet.unsupported.insert(book.instrument.clone());
                                            wallet.invalidate(
                                                Some(&book.market_id),
                                                book.received_at,
                                            );
                                            continue;
                                        }
                                        let forecast = if *history >= 6 {
                                            Some(m.predict(x)?)
                                        } else {
                                            None
                                        };
                                        if wallet.step(
                                            book.clone(),
                                            c,
                                            forecast,
                                            m.horizon,
                                            margin,
                                        )? {
                                            wallet.metrics.observations += 1;
                                            if *history < 6 {
                                                wallet.metrics.warmup += 1;
                                            } else if forecast.is_none_or(|n| !n.is_finite()) {
                                                wallet.metrics.invalid_forecast += 1;
                                            } else if forecast.unwrap() < margin {
                                                wallet.metrics.edge_below_margin += 1;
                                            } else {
                                                wallet.metrics.edge_pass += 1;
                                            }
                                            if book.asks[0].quantity as i128
                                                * c.depth_fraction_bps as i128
                                                / 10000
                                                < 1
                                            {
                                                wallet.metrics.zero_top_ask_capacity += 1;
                                            }
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
                        if points.len() != checks.len() {
                            return Err("MISSING_REPLAY_CHECKPOINT".into());
                        }
                        cards.push(Scorecard {
                            id: format!("{}-edge{margin}", m.id),
                            model_id: m.id.clone(),
                            margin,
                            rmse: m.rmse,
                            points,
                        });
                    }
                    Ok(cards)
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .map(|x| x.into_iter().flatten().collect::<Vec<_>>())?;
    cards.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(ReplayResult {
        mode: "historical_validation_replay",
        source_hash: capture.meta.source_hash.clone(),
        source_records: capture.meta.source_records,
        bank_source_binding: true,
        candidates: cards.len(),
        workers,
        start_count: start,
        end_count: end,
        checkpoints: checks,
        prepare_ms,
        replay_ms: started.elapsed().as_millis(),
        scorecards: cards,
        competition_orders: 0,
        network_requests: 0,
        profitability_established: false,
    })
}
