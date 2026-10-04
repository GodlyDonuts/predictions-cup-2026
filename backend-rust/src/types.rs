use crate::money::amount;
use serde::{Deserialize, Serialize};
pub const TOURNAMENT: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
#[derive(Clone, Copy, Default, Deserialize, Debug, PartialEq, Eq)]
pub enum FillClockPolicy {
    // The default retains exact historical replay; new declarations opt in.
    #[default]
    #[serde(rename = "receipt_latency_v1")]
    ReceiptLatencyV1,
    #[serde(rename = "independent_request_latency_v2")]
    IndependentRequestLatencyV2,
}
#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub schema_version: u32,
    pub tournament_id: String,
    pub market_ids: Vec<String>,
    pub initial_cash: String,
    pub cash_reserve: String,
    pub max_exposure: String,
    pub max_position_shares: i64,
    pub order_shares: i64,
    pub fee_bps: i128,
    pub slippage_ticks: i128,
    pub depth_fraction_bps: i64,
    pub max_data_age_ms: i64,
    pub cycle_interval_ms: i64,
    pub latency_ms: i64,
    #[serde(default)]
    pub fill_clock_policy: FillClockPolicy,
    pub stop_loss: String,
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.tournament_id != TOURNAMENT
            || self.market_ids.is_empty()
            || self.market_ids.len() > 300
            || self
                .market_ids
                .iter()
                .any(|x| x.is_empty() || !x.bytes().all(|n| n.is_ascii_digit()))
            || self
                .market_ids
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != self.market_ids.len()
            || self.order_shares < 1
            || self.max_position_shares < self.order_shares
            || self.max_position_shares > 9_007_199_254_740_991
            || self.latency_ms < 1
            || self.latency_ms > 86_400_000
            || self.max_data_age_ms < 1000
            || self.max_data_age_ms > 86_400_000
            || self.cycle_interval_ms < 10000
            || self.cycle_interval_ms > 86_400_000
            || !(0..=10000).contains(&self.fee_bps)
            || !(0..=5).contains(&self.slippage_ticks)
            || !(1..=1000).contains(&self.depth_fraction_bps)
        {
            return Err("INVALID_CONFIG".into());
        }
        let initial = amount(&self.initial_cash)?;
        let reserve = amount(&self.cash_reserve)?;
        let exposure = amount(&self.max_exposure)?;
        if initial <= 0
            || initial > 9_007_199_254_740_991 * 1_000_000
            || reserve < 0
            || reserve >= initial
            || exposure <= 0
            || exposure > initial
            || amount(&self.stop_loss)? <= 0
        {
            return Err("INVALID_CONFIG_RISK".into());
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Level {
    pub price: i128,
    pub quantity: i64,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Book {
    pub market_id: String,
    pub instrument: String,
    pub exchange_id: String,
    pub received_at: i64,
    pub request_started_at: i64,
    pub source_ns: i64,
    pub sequence: u64,
    pub bids: Vec<Level>,
    pub asks: Vec<Level>,
}
impl Book {
    pub fn same_version(&self, other: &Book) -> bool {
        self.sequence == other.sequence && self.source_ns == other.source_ns
    }
}
#[derive(Clone, Debug)]
pub enum Observation {
    Book(Book),
    Invalid { market_id: String, at: i64 },
}
#[derive(Clone, Debug)]
pub struct Frame {
    pub sequence: usize,
    pub at: i64,
    pub observations: Vec<Observation>,
    pub account_outage: bool,
    pub is_cycle: bool,
}
#[derive(Serialize, Debug)]
pub struct CaptureMeta {
    pub engine_hash: &'static str,
    pub source_format: &'static str,
    pub source_bytes: u64,
    pub source_records: usize,
    pub source_hash: String,
    pub first_at: i64,
    pub through: i64,
    pub cycles: usize,
    pub accepted_books: usize,
    pub invalid_books: usize,
    pub verify_ms: u128,
}
pub struct Capture {
    pub meta: CaptureMeta,
    pub frames: Vec<Frame>,
    pub prefix_hashes: Vec<String>,
    pub prefix_bytes: Vec<u64>,
}
pub fn instant(s: &str) -> Result<i64, String> {
    let t = chrono::DateTime::parse_from_rfc3339(s).map_err(|_| "INVALID_CLOCK")?;
    if t.timestamp() < 0 {
        return Err("INVALID_CLOCK".into());
    }
    Ok(t.timestamp_millis())
}
pub fn nanos(s: &str) -> Result<i64, String> {
    let t = chrono::DateTime::parse_from_rfc3339(s).map_err(|_| "INVALID_CLOCK")?;
    t.timestamp_nanos_opt()
        .ok_or_else(|| "INVALID_CLOCK".into())
}
