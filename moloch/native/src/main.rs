use chrono::{DateTime, SecondsFormat};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Write},
    time::Instant,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn fail(s: &str) -> Box<dyn std::error::Error> {
    s.to_owned().into()
}
fn time(v: &Value) -> Result<i64> {
    let s = v.as_str().ok_or_else(|| fail("TIMESTAMP_REQUIRED"))?;
    DateTime::parse_from_rfc3339(s)?
        .timestamp_nanos_opt()
        .ok_or_else(|| fail("TIMESTAMP_RANGE"))
}
fn hash(v: &Value) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(v).unwrap()))
}
fn verify(v: &Value, key: &str) -> Result<()> {
    let mut b = v.clone();
    let h = b
        .as_object_mut()
        .ok_or_else(|| fail("OBJECT_REQUIRED"))?
        .remove(key)
        .ok_or_else(|| fail("HASH_REQUIRED"))?;
    if h.as_str() != Some(hash(&b).as_str()) {
        return Err(fail("SOURCE_HASH_MISMATCH"));
    }
    Ok(())
}
fn history_events(receipt: &Value) -> Result<Vec<Value>> {
    if receipt["source"] != "official_sig_api_v1"
        || receipt["kind"] != "history"
        || receipt["status"] != 200
    {
        return Err(fail("OFFICIAL_HISTORY_RECEIPT_REQUIRED"));
    }
    let d = &receipt["data"];
    if d["exchangeId"] != receipt["exchangeId"] || d["marketId"] != receipt["marketId"] {
        return Err(fail("HISTORY_SCOPE"));
    }
    let span = match d["resolution"].as_str() {
        Some("1m") => 60,
        Some("5m") => 300,
        Some("1h") => 3600,
        Some("1d") => 86400,
        Some("1w") => 604800,
        _ => return Err(fail("HISTORY_RESOLUTION")),
    };
    let limit = time(&receipt["requestStartedAt"])?.min(time(&d["to"])?);
    let mut events = Vec::new();
    for c in d["candles"]
        .as_array()
        .ok_or_else(|| fail("HISTORY_CANDLES"))?
    {
        let release = time(&c["time"])? + span * 1_000_000_000;
        if release > limit || c["close"].is_null() {
            continue;
        }
        let at =
            DateTime::from_timestamp(release / 1_000_000_000, (release % 1_000_000_000) as u32)
                .ok_or_else(|| fail("HISTORY_CLOCK"))?
                .to_rfc3339_opts(SecondsFormat::Millis, true);
        events.push(json!({"type":"candle","tournamentId":receipt["tournamentId"],"marketId":receipt["marketId"],"exchangeId":d["exchangeId"],"receivedAt":at,"availableAt":at,"bucketStartedAt":c["time"],"close":c["close"],"volume":c["volume"],"tradeCount":c["tradeCount"],"retrievedAt":receipt["receivedAt"],"historicalReleaseClockAssumed":true}));
    }
    Ok(events)
}

#[derive(Default)]
struct Forecast {
    last: Option<f64>,
    last_return: Option<f64>,
    prediction: Option<f64>,
    pairs: u64,
    sx: f64,
    sy: f64,
    sxx: f64,
    sxy: f64,
    tests: u64,
    squared_error: f64,
    zero_squared_error: f64,
}
impl Forecast {
    fn observe(&mut self, p: f64) {
        if let Some(old) = self.last {
            let r = p - old;
            // Test the prediction made before this observation, then fit the new pair.
            if let Some(pred) = self.prediction {
                self.tests += 1;
                self.squared_error += (r - pred).powi(2);
                self.zero_squared_error += r * r;
            }
            if let Some(x) = self.last_return {
                self.pairs += 1;
                self.sx += x;
                self.sy += r;
                self.sxx += x * x;
                self.sxy += x * r;
            }
            let n = self.pairs as f64;
            let denominator = n * self.sxx - self.sx * self.sx;
            let beta = if self.pairs >= 3 && denominator.abs() > 1e-15 {
                ((n * self.sxy - self.sx * self.sy) / denominator).clamp(-1.0, 1.0)
            } else {
                0.0
            };
            let alpha = if self.pairs >= 3 {
                (self.sy - beta * self.sx) / n
            } else {
                0.0
            };
            self.prediction = Some((alpha + beta * r).clamp(-0.05, 0.05));
            self.last_return = Some(r);
        }
        self.last = Some(p);
    }
    fn report(&self) -> Value {
        json!({"evaluatedPredictions":self.tests,"rmse":if self.tests>0{Some((self.squared_error/self.tests as f64).sqrt())}else{None},"zeroChangeRmse":if self.tests>0{Some((self.zero_squared_error/self.tests as f64).sqrt())}else{None},"nextPriceChange":self.prediction,"profitabilityEstablished":false})
    }
}
struct Replay {
    last: Option<i64>,
    first: Option<i64>,
    count: u64,
    models: BTreeMap<String, Forecast>,
    paper: moloch_replay::engine::Engine,
}
impl Replay {
    fn new() -> Self {
        Self {
            last: None,
            first: None,
            count: 0,
            models: BTreeMap::new(),
            paper: moloch_replay::engine::Engine::new(&json!({})).expect("default policy"),
        }
    }
    fn accept(&mut self, event: &Value, out: &mut impl Write) -> Result<()> {
        let at = time(&event["receivedAt"])?;
        if self.last.is_some_and(|old| at < old) {
            return Err(fail("NONCAUSAL_SOURCE_ORDER"));
        }
        if event.get("processedAt").is_some() {
            return Err(fail(
                "REPLAY_REQUIRES_ORIGINAL_CLOCK_NOT_LIVE_PROCESSING_TIME",
            ));
        }
        let kind = event["type"].as_str().ok_or_else(|| fail("EVENT_TYPE"))?;
        let feature = match kind {
            "book" => {
                let b = &event["book"];
                if time(&b["requestStartedAt"])? > time(&b["receivedAt"])?
                    || b["receivedAt"] != event["receivedAt"]
                {
                    return Err(fail("BOOK_RECEIPT_CLOCK"));
                }
                let bid = b["bids"][0]["price"]
                    .as_str()
                    .ok_or_else(|| fail("DECIMAL_PRICE"))?
                    .parse::<f64>()?;
                let ask = b["asks"][0]["price"]
                    .as_str()
                    .ok_or_else(|| fail("DECIMAL_PRICE"))?
                    .parse::<f64>()?;
                Some((
                    b["exchangeId"]
                        .as_str()
                        .ok_or_else(|| fail("EXCHANGE_SCOPE"))?
                        .to_owned(),
                    (bid + ask) / 2.0,
                ))
            }
            "candle" => {
                if time(&event["availableAt"])? != at {
                    return Err(fail("CANDLE_RELEASE_CLOCK"));
                }
                Some((
                    event["exchangeId"]
                        .as_str()
                        .ok_or_else(|| fail("EXCHANGE_SCOPE"))?
                        .to_owned(),
                    event["close"]
                        .as_f64()
                        .ok_or_else(|| fail("CANDLE_CLOSE"))?,
                ))
            }
            "trade" => {
                if time(&event["executedAt"])? > at {
                    return Err(fail("TRADE_PRECEDES_EXECUTION"));
                }
                None
            }
            "tick" | "coverage" | "gap" => None,
            _ => return Err(fail("UNKNOWN_EVENT")),
        };
        self.paper.advance(event).map_err(fail)?;
        if let Some((ex, p)) = feature {
            if !p.is_finite() || !(0.0..=1.0).contains(&p) {
                return Err(fail("PRICE_RANGE"));
            }
            self.models.entry(ex).or_default().observe(p);
        }
        serde_json::to_writer(&mut *out, event)?;
        out.write_all(b"\n")?;
        self.count += 1;
        self.first.get_or_insert(at);
        self.last = Some(at);
        Ok(())
    }
}
fn run() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    let mut o = BTreeMap::new();
    for pair in args.chunks(2) {
        if pair.len() != 2 {
            return Err(fail(
                "USAGE: --input FILE --output NEW_FILE --stats NEW_FILE [--format events|book-export]",
            ));
        }
        o.insert(pair[0].clone(), pair[1].clone());
    }
    let input = o.get("--input").ok_or_else(|| fail("INPUT_REQUIRED"))?;
    let output = o.get("--output").ok_or_else(|| fail("OUTPUT_REQUIRED"))?;
    let stats = o.get("--stats").ok_or_else(|| fail("STATS_REQUIRED"))?;
    if std::path::Path::new(output).exists() || std::path::Path::new(stats).exists() {
        return Err(fail("PRESERVE_EXISTING_RUN"));
    }
    let began = Instant::now();
    let mut replay = Replay::new();
    if let Some(file) = o.get("--policy") {
        replay.paper =
            moloch_replay::engine::Engine::new(&serde_json::from_reader(File::open(file)?)?)
                .map_err(fail)?;
    }
    let mut out = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)?,
    );
    match o.get("--format").map(String::as_str).unwrap_or("events") {
        "events" => {
            for line in BufReader::new(File::open(input)?).lines() {
                let line = line?;
                if !line.trim().is_empty() {
                    replay.accept(&serde_json::from_str::<Value>(&line)?, &mut out)?;
                }
            }
        }
        "history" => {
            let raw = fs::read_to_string(input)?;
            let receipt: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}'))?;
            for event in history_events(&receipt)? {
                replay.accept(&event, &mut out)?;
            }
        }
        "book-export" => {
            let document: Value = serde_json::from_reader(BufReader::new(File::open(input)?))?;
            verify(&document, "exportHash")?;
            let mut prior: Option<String> = None;
            let mut sequence: Option<u64> = None;
            for row in document["rows"]
                .as_array()
                .ok_or_else(|| fail("EXPORT_ROWS"))?
            {
                verify(row, "hash")?;
                let seq = row["sequence"]
                    .as_u64()
                    .ok_or_else(|| fail("SOURCE_SEQUENCE"))?;
                if prior
                    .as_ref()
                    .is_some_and(|h| row["previousHash"].as_str() != Some(h.as_str()))
                    || sequence.is_some_and(|n| seq != n + 1)
                {
                    return Err(fail("SOURCE_CHAIN"));
                }
                prior = Some(row["hash"].as_str().unwrap().to_owned());
                sequence = Some(seq);
                if row["kind"] != "book" {
                    return Err(fail("EXPORT_BOUNDARY_REQUIRES_GAP"));
                }
                let b = &row["payload"]["book"];
                replay.accept(
                    &json!({"type":"book","receivedAt":b["receivedAt"],"book":b}),
                    &mut out,
                )?;
            }
        }
        _ => return Err(fail("FORMAT_INVALID")),
    }
    out.flush()?;
    let seconds = began.elapsed().as_secs_f64();
    let source_seconds = match (replay.first, replay.last) {
        (Some(a), Some(b)) => (b - a) as f64 / 1e9,
        _ => 0.0,
    };
    let models: BTreeMap<_, _> = replay.models.iter().map(|(k, v)| (k, v.report())).collect();
    let report = json!({"bot":"Moloch","mode":"rust_virtual_clock_replay","events":replay.count,"wallSeconds":seconds,"sourceSeconds":source_seconds,"eventsPerSecond":replay.count as f64/seconds.max(1e-9),"clockCompression":source_seconds/seconds.max(1e-9),"models":models,"sleeps":0,"networkRequests":0,"contestOrders":0,"historicalCandleClockAssumed":o.get("--format").is_some_and(|x|x=="history"),"financialEngine":"rust","paper":replay.paper.report().map_err(fail)?,"throughputScope":"Rust validation, forecasting, paper quoting, fills and exact wallet accounting","profitabilityEstablished":false});
    let mut report_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(stats)?;
    serde_json::to_writer_pretty(&mut report_file, &report)?;
    report_file.write_all(b"\n")?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("MOLOCH_REPLAY_FAILED: {e}");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clock_does_not_sort_future_backwards() {
        let mut r = Replay::new();
        let mut b = Vec::new();
        r.accept(
            &json!({"type":"tick","receivedAt":"2026-10-13T00:00:02Z"}),
            &mut b,
        )
        .unwrap();
        assert!(
            r.accept(
                &json!({"type":"tick","receivedAt":"2026-10-13T00:00:01Z"}),
                &mut b
            )
            .is_err()
        );
    }
    #[test]
    fn forecast_is_scored_before_current_price_is_fitted() {
        let mut f = Forecast::default();
        f.observe(0.4);
        f.observe(0.5);
        f.observe(0.4);
        assert_eq!(f.tests, 1);
        assert!((f.squared_error - 0.01).abs() < 1e-12);
    }
    #[test]
    fn future_execution_is_rejected() {
        let mut r = Replay::new();
        assert!(r.accept(&json!({"type":"trade","receivedAt":"2026-10-13T00:00:01Z","executedAt":"2026-10-13T00:00:02Z"}),&mut Vec::new()).is_err());
    }
    #[test]
    fn historical_close_is_not_released_at_bucket_start_and_incomplete_bucket_is_excluded() {
        let r = json!({"source":"official_sig_api_v1","kind":"history","status":200,"marketId":"11","exchangeId":"101","tournamentId":"cup","requestStartedAt":"2026-10-13T01:30:00Z","receivedAt":"2026-10-13T01:30:01Z","data":{"marketId":"11","exchangeId":"101","resolution":"1h","to":"2026-10-13T02:00:00Z","candles":[{"time":"2026-10-13T00:00:00Z","close":0.45},{"time":"2026-10-13T01:00:00Z","close":0.5}]}});
        let e = history_events(&r).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0]["receivedAt"], "2026-10-13T01:00:00.000Z");
        assert_eq!(e[0]["historicalReleaseClockAssumed"], true);
    }
}
