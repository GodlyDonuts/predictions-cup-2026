// live-paper/realtime-microprice-dataset.mjs — causal microprice feature samples
// with entry/label endpoints, capture-gap censorship and optional endpoint-only
// gap retention. Consumer-side research dataset; never an execution permission.
use baal_service_runtime::contract::{instant_ns, ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_int, js_truthy};
use serde_json::{json, Value as V};
use std::collections::{HashMap, HashSet};

fn need(v: bool, c: &str) -> Result<(), String> { if !v { return Err(format!("MICROPRICE_DATA_{c}")); } Ok(()) }
fn ticks(p: f64) -> Result<i64, String> {
    let n = p * 200.0;
    need(n.is_finite() && (n - n.round()).abs() < 1e-7, "TICK")?;
    Ok(n.round() as i64)
}
/// JS Math.min/max propagate NaN (Rust f64::min/max return the non-NaN operand).
pub fn js_min(a: f64, b: f64) -> f64 { if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) } }
pub fn js_max(a: f64, b: f64) -> f64 { if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) } }
/// JS Math.min(...rows, seed) over an iterator.
fn js_min_fold<I: Iterator<Item = f64>>(seed: f64, it: I) -> f64 { it.fold(seed, |a, b| if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }) }
fn js_max_fold<I: Iterator<Item = f64>>(seed: f64, it: I) -> f64 { it.fold(seed, |a, b| if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }) }

pub const MICROPRICE_FEATURE_NAMES: [&str; 20] = [
    "midProbability", "spreadTicks", "imbalance1", "imbalance3", "imbalance10", "imbalance20",
    "logBidDepth", "logAskDepth", "micropriceDeviationTicks", "return60sTicks", "return300sTicks",
    "range60sTicks", "range300sTicks", "signedFlow30s", "signedFlow120s", "logVolume30s",
    "logVolume120s", "tradeCount30s", "lastTradeAgeSeconds", "imbalanceProbabilityInteraction",
];
/// MICROPRICE_FEATURES (frozen array).
pub fn microprice_features_spec() -> V { json!(MICROPRICE_FEATURE_NAMES) }
/// MICROPRICE_DATA_POLICY (frozen object).
pub fn microprice_data_policy() -> V {
    json!({"sampleMs":30000,"latencyMs":3000,"horizonMs":300000,"labelToleranceMs":20000,
        "historyMs":300000,"minimumWarmupMs":60000,"depthFractionBps":100,"quantity":1,
        "entrySlippageTicks":1,"exitSlippageTicks":1,"feeTicks":0})
}

/// Parsed pushed-book view shared with the spread-economics diagnostic.
#[derive(Clone)]
pub struct MicropriceBook {
    pub at: i64,
    pub bid: i64,
    pub ask: i64,
    pub mid: f64,
    pub bids: Vec<V>,
    pub asks: Vec<V>,
    pub sequence: f64,
    pub expiry: Option<i64>,
}
#[derive(Clone)]
pub struct MicropriceHistoryPoint { pub at: i64, pub mid: f64 }
#[derive(Clone)]
pub struct MicropriceTrade { pub at: i64, pub quantity: f64, pub sign: f64 }
/// Per-market rolling feature inputs ({book, history, trades}).
#[derive(Clone, Default)]
pub struct MicropriceMarket {
    pub book: Option<MicropriceBook>,
    pub history: Vec<MicropriceHistoryPoint>,
    pub trades: Vec<MicropriceTrade>,
}

fn parsed_book(b: &V, at: i64) -> Result<Option<MicropriceBook>, String> {
    if b["asOf"].is_null()
        || b["bids"].as_array().map(|a| a.is_empty()).unwrap_or(true)
        || b["asks"].as_array().map(|a| a.is_empty()).unwrap_or(true)
    { return Ok(None); }
    need(instant_ns(b["asOf"]["at"].as_str().unwrap_or("")).map_err(|e| e.0)?
        <= at as i128 * 1_000_000 + 1_000_000_000, "FUTURE_BOOK")?;
    let bid = ticks(b["bids"][0]["price"].as_f64().unwrap_or(f64::NAN))?;
    let ask = ticks(b["asks"][0]["price"].as_f64().unwrap_or(f64::NAN))?;
    need(bid < ask, "CROSSED")?;
    let expiry = if js_truthy(b.get("nextExpiryAt")) {
        b["nextExpiryAt"].as_str().and_then(parse_iso_ms).map(|x| x as i64)
    } else { None };
    Ok(Some(MicropriceBook {
        at, bid, ask, mid: (bid + ask) as f64 / 2.0,
        bids: b["bids"].as_array().cloned().unwrap_or_default(),
        asks: b["asks"].as_array().cloned().unwrap_or_default(),
        sequence: b["asOf"]["sequence"].as_f64().unwrap_or(f64::NAN),
        expiry,
    }))
}

/// micropriceFeatures(m, b, at) — 20-feature vector; throws MICROPRICE_DATA_FEATURES
/// on any non-finite output.
pub fn microprice_features(m: &MicropriceMarket, b: &MicropriceBook, at: i64) -> Result<Vec<f64>, String> {
    let depth = |rows: &[V], n: usize| -> f64 {
        rows.iter().take(n).map(|l| l["quantity"].as_f64().unwrap_or(f64::NAN)).sum()
    };
    let imb = |n: usize| -> f64 { let x = depth(&b.bids, n); let y = depth(&b.asks, n); (x - y) / (x + y) };
    let d_bid = depth(&b.bids, 20);
    let d_ask = depth(&b.asks, 20);
    let prob = b.mid / 200.0;
    let window = |ms: i64| -> Vec<f64> { m.history.iter().filter(|h| at - h.at <= ms).map(|h| h.mid).collect() };
    let old = |ms: i64| -> f64 {
        m.history.iter().filter(|h| h.at <= at - ms).last().map(|h| h.mid)
            .unwrap_or_else(|| m.history.first().map(|h| h.mid).unwrap_or(f64::NAN))
    };
    let range = |ms: i64| -> f64 {
        let rows = window(ms);
        js_max_fold(b.mid, rows.iter().cloned()) - js_min_fold(b.mid, rows.iter().cloned())
    };
    let flow = |ms: i64| -> (f64, f64, f64) {
        let rows: Vec<&MicropriceTrade> = m.trades.iter().filter(|t| at - t.at <= ms).collect();
        let volume: f64 = rows.iter().map(|t| t.quantity).sum();
        (if volume != 0.0 { rows.iter().map(|t| t.sign * t.quantity).sum::<f64>() / volume } else { 0.0 },
         volume, rows.len() as f64)
    };
    let f30 = flow(30000);
    let f120 = flow(120000);
    let q0b = b.bids[0]["quantity"].as_f64().unwrap_or(f64::NAN);
    let q0a = b.asks[0]["quantity"].as_f64().unwrap_or(f64::NAN);
    let micro = (b.ask as f64 * q0b + b.bid as f64 * q0a) / (q0b + q0a) - b.mid;
    let trade_age_ms = m.trades.last().map(|t| t.at).unwrap_or(at - 120000);
    let age_seconds = (at - trade_age_ms) as f64 / 1000.0;
    let x = vec![prob, (b.ask - b.bid) as f64, imb(1), imb(3), imb(10), imb(20),
        d_bid.ln_1p(), d_ask.ln_1p(), micro, b.mid - old(60000), b.mid - old(300000),
        range(60000), range(300000), f30.0, f120.0, f30.1.ln_1p(), f120.1.ln_1p(), f30.2,
        js_min(120.0, age_seconds),
        imb(1) * prob * (1.0 - prob)];
    need(x.iter().all(|v| v.is_finite()), "FEATURES")?;
    Ok(x)
}

struct DatasetMarket {
    fm: MicropriceMarket,
    pending: Vec<V>,
    revision: Option<i64>,
    last_sample: f64,
}
impl DatasetMarket {
    fn new() -> Self {
        Self { fm: MicropriceMarket::default(), pending: Vec::new(), revision: None,
            last_sample: f64::NEG_INFINITY }
    }
}

/// createMicropriceDataset({onSample, policy}) — consume() + report() closure object.
pub struct MicropriceDataset<F: FnMut(V)> {
    on_sample: F,
    policy: V,
    markets: HashMap<String, DatasetMarket>,
    seen: HashSet<String>,
    clock: i64,
    index: i64,
    source: String,
    counts: V,
}
fn num(v: &V) -> f64 { v.as_f64().unwrap_or(f64::NAN) }
fn done(s: &V) -> bool { js_truthy(s.get("done")) }

impl<F: FnMut(V)> MicropriceDataset<F> {
    pub fn new(on_sample: F, policy: Option<V>) -> Result<Self, String> {
        let policy = policy.unwrap_or_else(microprice_data_policy);
        need(js_int(&policy["horizonMs"]).is_some()
            && num(&policy["horizonMs"]) > num(&policy["latencyMs"]), "HORIZON")?;
        need(policy.get("endpointResearchAcrossGaps").map_or(true, |v| v.is_boolean()), "GAP_POLICY")?;
        Ok(Self {
            on_sample,
            policy,
            markets: HashMap::new(),
            seen: HashSet::new(),
            clock: 0,
            index: -1,
            source: String::new(),
            counts: json!({"books":0,"trades":0,"labels":0,"samples":0,"censored":0,"gaps":0,
                "retainedEndpointGapEvents":0,"labelsAcrossGaps":0}),
        })
    }

    fn market(&mut self, id: &str) -> &mut DatasetMarket {
        self.markets.entry(id.to_string()).or_insert_with(DatasetMarket::new)
    }

    fn clear(m: &mut DatasetMarket, policy: &V, clock: i64, counts: &mut V, reason: &str, settled: bool) {
        let mut retained: Vec<V> = Vec::new();
        for sample in m.pending.drain(..) {
            // Independent entry and liquidation endpoints can support a price forecast
            // across an outage. They cannot establish what happened to a resting quote,
            // a stop or an inventory position during that outage.
            if policy["endpointResearchAcrossGaps"].as_bool() == Some(true) && !settled
                && js_truthy(sample.get("entryAt"))
                && clock <= sample["targetAt"].as_i64().unwrap_or(i64::MIN)
                    + policy["labelToleranceMs"].as_i64().unwrap_or(0) {
                let mut sample = sample;
                sample["pathGapCount"] = json!(sample["pathGapCount"].as_i64().unwrap_or(0) + 1);
                if sample.get("firstPathGapReason").map(|v| v.is_null()).unwrap_or(true) {
                    sample["firstPathGapReason"] = json!(reason);
                }
                retained.push(sample);
                counts["retainedEndpointGapEvents"] = json!(counts["retainedEndpointGapEvents"].as_i64().unwrap_or(0) + 1);
            } else {
                counts["censored"] = json!(counts["censored"].as_i64().unwrap_or(0) + 1);
            }
        }
        m.fm.book = None; m.fm.history.clear(); m.fm.trades.clear();
        m.pending = retained; m.revision = None;
    }

    fn apply_book(&mut self, id: &str, raw: &V, at: i64, provenance: &V) -> Result<(), String> {
        let policy = self.policy.clone();
        let counts = &mut self.counts;
        let on_sample = &mut self.on_sample;
        let m = self.markets.entry(id.to_string()).or_insert_with(DatasetMarket::new);
        let b = match parsed_book(raw, at)? { Some(b) => b, None => return Ok(()) };
        if b.expiry.is_some_and(|e| e <= at) { return Ok(()); }
        if let Some(cur) = &m.fm.book {
            if b.sequence < cur.sequence { return Ok(()); }
            if b.sequence == cur.sequence { return Ok(()); }
        }
        *counts.get_mut("books").unwrap() = json!(counts["books"].as_i64().unwrap_or(0) + 1);
        m.fm.book = Some(b.clone());
        let latency_ms = policy["latencyMs"].as_i64().unwrap_or(0);
        let tolerance = policy["labelToleranceMs"].as_i64().unwrap_or(0);
        let horizon_ms = policy["horizonMs"].as_i64().unwrap_or(0);
        let depth_fraction_bps = num(&policy["depthFractionBps"]);
        let quantity = num(&policy["quantity"]);
        let min_depth = || -> f64 {
            (js_min(b.bids[0]["quantity"].as_f64().unwrap_or(f64::NAN),
                    b.asks[0]["quantity"].as_f64().unwrap_or(f64::NAN))
                * depth_fraction_bps / 10000.0).floor()
        };
        let raw_ns = instant_ns(raw["asOf"]["at"].as_str().unwrap_or("")).map_err(|e| e.0)?;
        for i in 0..m.pending.len() {
            let sample = &mut m.pending[i];
            if !js_truthy(sample.get("entryAt"))
                && at >= parse_iso_ms(sample["at"].as_str().unwrap_or("")).unwrap_or(f64::NAN) as i64 + latency_ms {
                let sample_at_ms = parse_iso_ms(sample["at"].as_str().unwrap_or("")).unwrap_or(f64::NAN) as i64;
                if at > sample_at_ms + latency_ms + tolerance || min_depth() < quantity {
                    *counts.get_mut("censored").unwrap() = json!(counts["censored"].as_i64().unwrap_or(0) + 1);
                    sample["done"] = json!(true);
                    continue;
                }
                if raw_ns < (sample_at_ms + latency_ms) as i128 * 1_000_000 { continue; }
                sample["entryAt"] = json!(ms_to_iso(at as f64));
                sample["entryBidTicks"] = json!(b.bid);
                sample["entryAskTicks"] = json!(b.ask);
                sample["entrySource"] = provenance.clone();
            }
            let target_at = sample["targetAt"].as_i64().unwrap_or(i64::MIN);
            if at < target_at { continue; }
            if !js_truthy(sample.get("entryAt")) || at > target_at + tolerance {
                *counts.get_mut("censored").unwrap() = json!(counts["censored"].as_i64().unwrap_or(0) + 1);
                sample["done"] = json!(true);
                continue;
            }
            if raw_ns < target_at as i128 * 1_000_000 { continue; }
            let depth = js_min(b.bids[0]["quantity"].as_f64().unwrap_or(f64::NAN),
                b.asks[0]["quantity"].as_f64().unwrap_or(f64::NAN));
            if (depth * depth_fraction_bps / 10000.0).floor() < quantity {
                *counts.get_mut("censored").unwrap() = json!(counts["censored"].as_i64().unwrap_or(0) + 1);
                sample["done"] = json!(true);
                continue;
            }
            let mut out = sample.clone();
            out["labelAt"] = json!(ms_to_iso(at as f64));
            out["labelSource"] = provenance.clone();
            out["endpointOnlyResearch"] = json!(policy["endpointResearchAcrossGaps"].as_bool() == Some(true));
            out["pathContinuity"] = json!(if js_truthy(sample.get("pathGapCount")) { "unknown_between_endpoints" } else { "observed_capture" });
            out["y"] = json!(b.mid - sample["mid"].as_f64().unwrap_or(f64::NAN));
            out["futureBidTicks"] = json!(b.bid);
            out["futureAskTicks"] = json!(b.ask);
            out["yesTakerNetTicks"] = json!(b.bid - policy["exitSlippageTicks"].as_i64().unwrap_or(0)
                - sample["entryAskTicks"].as_i64().unwrap_or(0) - policy["entrySlippageTicks"].as_i64().unwrap_or(0));
            out["noTakerNetTicks"] = json!(sample["entryBidTicks"].as_i64().unwrap_or(0) - b.ask
                - policy["exitSlippageTicks"].as_i64().unwrap_or(0) - policy["entrySlippageTicks"].as_i64().unwrap_or(0));
            on_sample(out);
            sample["done"] = json!(true);
            *counts.get_mut("labels").unwrap() = json!(counts["labels"].as_i64().unwrap_or(0) + 1);
            if sample["pathGapCount"].as_i64().unwrap_or(0) != 0 {
                *counts.get_mut("labelsAcrossGaps").unwrap() = json!(counts["labelsAcrossGaps"].as_i64().unwrap_or(0) + 1);
            }
        }
        m.pending.retain(|s| !done(s));
        let prior = m.fm.history.last().map(|h| h.at);
        if prior.is_none() || at - prior.unwrap() >= 1000 {
            m.fm.history.push(MicropriceHistoryPoint { at, mid: b.mid });
        }
        m.fm.history.retain(|h| at - h.at <= policy["historyMs"].as_i64().unwrap_or(0) + 1000);
        m.fm.trades.retain(|t| at - t.at <= 120000);
        if m.fm.history.is_empty()
            || at - m.fm.history[0].at < policy["minimumWarmupMs"].as_i64().unwrap_or(0)
            || (at as f64 - m.last_sample) < num(&policy["sampleMs"])
            || min_depth() < quantity { return Ok(()); }
        let x = microprice_features(&m.fm, m.fm.book.as_ref().unwrap(), at)?;
        m.pending.push(json!({"marketId":id,"at":ms_to_iso(at as f64),"targetAt":at + horizon_ms,
            "mid":b.mid,"bid":b.bid,"ask":b.ask,"x":x,"source":provenance,"pathGapCount":0}));
        m.last_sample = at as f64;
        *self.counts.get_mut("samples").unwrap() = json!(self.counts["samples"].as_i64().unwrap_or(0) + 1);
        Ok(())
    }

    /// consume(row, {sourceId}).
    pub fn consume(&mut self, row: &V, source_id: &str) -> Result<(), String> {
        need(row.get("format").is_some_and(|v| v == "sig-feed-capture") && row["schemaVersion"] == json!(1), "ENVELOPE")?;
        let at = parse_iso_ms(row["observedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
        need(at.is_finite() && at.fract() == 0.0 && at.abs() <= 9_007_199_254_740_991.0 && at >= self.clock as f64, "CLOCK")?;
        self.clock = at as i64;
        if source_id != self.source {
            self.source = source_id.to_string();
            self.index = -1;
            let counts = &mut self.counts;
            let policy = &self.policy;
            let clock = self.clock;
            for m in self.markets.values_mut() { Self::clear(m, policy, clock, counts, "capture_boundary", false); }
        }
        need(row["captureIndex"].as_i64() == Some(self.index + 1), "INDEX")?;
        self.index = row["captureIndex"].as_i64().unwrap_or(self.index + 1);
        let id = market_topic(row["topic"].as_str().unwrap_or(""));
        need(id.is_some(), "SCOPE")?;
        let id = id.unwrap();
        let provenance = json!({"file":source_id,"captureIndex":self.index});
        let kind = row["kind"].as_str().unwrap_or("");
        if ["invalidation", "subscribed", "disconnected", "rejected", "stopped"].contains(&kind) {
            let counts = &mut self.counts; let policy = self.policy.clone(); let clock = self.clock;
            let m = self.markets.entry(id.clone()).or_insert_with(DatasetMarket::new);
            Self::clear(m, &policy, clock, counts, kind, false);
            return Ok(());
        }
        if kind == "snapshot" {
            for b in row["books"].as_array().cloned().unwrap_or_default() { self.apply_book(&id, &b, at as i64, &provenance)?; }
            return Ok(());
        }
        if kind != "event" { return Ok(()); }
        if row["event"].as_str() != Some("market_batch") {
            let counts = &mut self.counts; let policy = self.policy.clone(); let clock = self.clock;
            let m = self.markets.entry(id.clone()).or_insert_with(DatasetMarket::new);
            Self::clear(m, &policy, clock, counts, "capture_boundary", false);
            return Ok(());
        }
        let p = &row["payload"];
        let revision_gap = js_truthy(p.get("resyncRequired"))
            || (self.markets.get(&id).and_then(|m| m.revision).is_some()
                && p["delivery"]["previousRevision"].as_i64().unwrap_or(i64::MIN)
                    > self.markets.get(&id).and_then(|m| m.revision).unwrap());
        if revision_gap {
            *self.counts.get_mut("gaps").unwrap() = json!(self.counts["gaps"].as_i64().unwrap_or(0) + 1);
            let counts = &mut self.counts; let policy = self.policy.clone(); let clock = self.clock;
            let m = self.markets.entry(id.clone()).or_insert_with(DatasetMarket::new);
            Self::clear(m, &policy, clock, counts, "delivery_gap", false);
        }
        let new_rev = p["delivery"]["revision"].as_i64().unwrap_or(0);
        let m = self.market(&id);
        m.revision = Some(std::cmp::max(m.revision.unwrap_or(0), new_rev));
        if p["marketSettled"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
            let counts = &mut self.counts; let policy = self.policy.clone(); let clock = self.clock;
            let m = self.markets.entry(id.clone()).or_insert_with(DatasetMarket::new);
            Self::clear(m, &policy, clock, counts, "market_settled", true);
            return Ok(());
        }
        let trades_null_seq = p["trades"].as_array().map(|ts| ts.iter().any(|t| t["sequence"].is_null())).unwrap_or(false);
        let dirty_dangling = p["bookDirty"].as_array().map(|ds| ds.iter().any(|d|
            !p["books"].as_array().map(|bs| bs.iter().any(|b| b["exchangeId"] == d["exchangeId"])).unwrap_or(false)
        )).unwrap_or(false);
        if trades_null_seq || dirty_dangling {
            let counts = &mut self.counts; let policy = self.policy.clone(); let clock = self.clock;
            let m = self.markets.entry(id.clone()).or_insert_with(DatasetMarket::new);
            Self::clear(m, &policy, clock, counts, "incomplete_event", false);
            return Ok(());
        }
        for t in p["trades"].as_array().cloned().unwrap_or_default() {
            let key = format!("{id}:{}", t["id"].as_str().unwrap_or(""));
            if self.seen.contains(&key) { continue; }
            self.seen.insert(key);
            *self.counts.get_mut("trades").unwrap() = json!(self.counts["trades"].as_i64().unwrap_or(0) + 1);
            let price = ticks(num(&t["price"]))?;
            let sign = match &self.markets.get(&id).and_then(|m| m.fm.book.clone()) {
                Some(book) => if price >= book.ask { 1.0 } else if price <= book.bid { -1.0 } else { 0.0 },
                None => 0.0,
            };
            self.market(&id).fm.trades.push(MicropriceTrade {
                at: at as i64, quantity: num(&t["quantity"]), sign });
        }
        for b in p["books"].as_array().cloned().unwrap_or_default() { self.apply_book(&id, &b, at as i64, &provenance)?; }
        Ok(())
    }

    /// report()
    pub fn report(&self) -> V {
        json!({
            "policy": self.policy,
            "features": microprice_features_spec(),
            "counts": self.counts,
            "unresolved": self.markets.values().map(|m| m.pending.len() as i64).sum::<i64>(),
            "through": ms_to_iso(self.clock as f64),
        })
    }
}

/// /:market:(\d+)$/ topic suffix.
fn market_topic(topic: &str) -> Option<String> {
    let idx = topic.rfind(":market:")? + ":market:".len();
    let id = &topic[idx..];
    if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) { Some(id.to_string()) } else { None }
}
