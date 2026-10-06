// live-paper/realtime-spread-economics.mjs — offline market-only diagnostic.
// Samples overlap; their sum is NOT portfolio P&L.
use crate::realtime_microprice_dataset::{
    js_max, js_min, microprice_features, MicropriceBook, MicropriceHistoryPoint,
    MicropriceMarket, MicropriceTrade,
};
use baal_service_runtime::contract::{instant_ns, ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Value as V};
use std::collections::{HashMap, HashSet};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("REALTIME_ECONOMICS_{}", code)); }
    Ok(())
}
fn tick(n: f64) -> Result<i64, String> {
    let x = n * 200.0;
    need(x.is_finite() && (x - x.round()).abs() < 1e-7 && x >= 0.0 && x <= 200.0, "TICK")?;
    Ok(x.round() as i64)
}
fn num(v: &V) -> f64 { v.as_f64().unwrap_or(f64::NAN) }
/// JS `v?.length` truthiness for the value shapes that expose .length.
fn has_length(v: &V) -> bool {
    match v {
        V::Array(a) => !a.is_empty(),
        V::String(s) => !s.is_empty(),
        _ => false,
    }
}

/// REALTIME_ECONOMICS_POLICIES — 6 frozen policies (improvement × latency grid).
pub fn realtime_economics_policies() -> Vec<V> {
    let mut out = Vec::new();
    for improvement_ticks in [0i64, 1] {
        for latency_ms in [3000i64, 6000, 10000] {
            out.push(json!({"id":format!("improve{}-latency{}",improvement_ticks,latency_ms),
                "improvementTicks":improvement_ticks,"latencyMs":latency_ms,"quoteMs":120000,
                "markoutMs":300000,"sampleEveryMs":30000,"historyMs":300000,"minimumSpanMs":240000,
                "maximumMidpointRangeTicks":1,"maximumBookAgeMs":20000,"depthFractionBps":100,
                "quantity":1,"feeTicks":0,"exitSlippageTicks":1}));
        }
    }
    out
}

#[derive(Clone)]
struct NBook {
    bid: i64, ask: i64, mid: f64, bids: Vec<V>, asks: Vec<V>,
    bid_size: f64, ask_size: f64, sequence: i64, source_ns: i128, at: i64,
    expiry: Option<f64>,
}
impl NBook {
    fn as_micro_book(&self) -> MicropriceBook {
        MicropriceBook {
            at: self.at, bid: self.bid, ask: self.ask, mid: self.mid,
            bids: self.bids.clone(), asks: self.asks.clone(),
            sequence: self.sequence as f64,
            expiry: self.expiry.map(|e| e as i64),
        }
    }
}
#[derive(Clone)]
struct Quote {
    market_id: String, at: i64, activation: i64, expiry: i64,
    limits: [i64; 2], fills: [Option<V>; 2], gap_exposed: bool,
    x: V, decision_source: V,
}
struct BankMarket {
    book: Option<NBook>, history: Vec<V>, sample_at: f64, revision: Option<f64>, quote: Option<Quote>,
}
impl BankMarket {
    fn new() -> Self {
        Self { book: None, history: Vec::new(), sample_at: f64::NEG_INFINITY,
            revision: None, quote: None }
    }
}
struct BankState {
    policy: V, markets: HashMap<String, BankMarket>, counts: V,
    paired_gain_ticks: i64, one_sided_markout_ticks: i64, outcomes: Vec<V>,
}
#[derive(Clone, Default)]
struct FeatureMarket {
    book: Option<NBook>, history: Vec<MicropriceHistoryPoint>, trades: Vec<MicropriceTrade>,
}

fn normalized_book(raw: &V, at: i64) -> Result<Option<NBook>, String> {
    if raw.is_null() || !js_truthy(raw.get("asOf"))
        || !has_length(&raw["bids"]) || !has_length(&raw["asks"]) {
        return Ok(None);
    }
    let source_ns = instant_ns(raw["asOf"]["at"].as_str().unwrap_or("")).map_err(|e| e.0)?;
    let now_ns = at as i128 * 1_000_000;
    need(source_ns <= now_ns + 1_000_000_000, "FUTURE_BOOK")?;
    let bid = tick(num(&raw["bids"][0]["price"]))?;
    let ask = tick(num(&raw["asks"][0]["price"]))?;
    need(bid < ask && raw["asOf"]["sequence"].as_i64().is_some(), "BOOK")?;
    let expiry = if js_truthy(raw.get("nextExpiryAt")) {
        Some(raw["nextExpiryAt"].as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN))
    } else { None };
    Ok(Some(NBook {
        bid, ask, mid: (bid + ask) as f64 / 2.0,
        bids: raw["bids"].as_array().cloned().unwrap_or_default(),
        asks: raw["asks"].as_array().cloned().unwrap_or_default(),
        bid_size: num(&raw["bids"][0]["quantity"]),
        ask_size: num(&raw["asks"][0]["quantity"]),
        sequence: raw["asOf"]["sequence"].as_i64().unwrap_or(0),
        source_ns, at, expiry,
    }))
}

/// /:market:(\d+)$/ topic suffix extraction.
fn topic_market(topic: &str) -> Option<String> {
    let idx = topic.rfind(":market:")? + ":market:".len();
    let id = &topic[idx..];
    if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) { Some(id.to_string()) } else { None }
}

fn fresh_counts() -> V {
    json!({"quotes":0,"paired":0,"unfilled":0,"oneSided":0,"marked":0,"censored":0,
        "gapExposed":0,"expiryInvalidations":0})
}
fn inc(v: &mut V, k: &str) { v[k] = json!(v[k].as_i64().unwrap_or(0) + 1); }

pub struct RealtimeSpreadEconomics<D, Q, F, O>
where D: FnMut(V) -> V, Q: FnMut(&V), F: FnMut(&V), O: FnMut(&V) {
    banks: Vec<BankState>, seen_trades: HashSet<String>,
    feature_markets: HashMap<String, FeatureMarket>,
    last_at: i64, last_index: i64, source: Option<String>, source_record: V,
    metrics: V,
    decide_quote: D, on_quote: Q, on_fill: F, on_outcome: O,
}

fn finish<O: FnMut(&V)>(s: &mut BankState, id: &str, q: Quote, at: i64,
    kind: &str, mark: Option<&NBook>, on_outcome: &mut O) {
    let fills: Vec<V> = q.fills.iter().filter_map(|f| f.clone()).collect();
    s.counts[kind] = json!(s.counts[kind].as_i64().unwrap_or(0) + 1);
    let mut value = V::Null;
    if kind == "paired" {
        let v = 200 - q.limits[0] - q.limits[1];
        s.paired_gain_ticks += v;
        value = json!(v);
    }
    if kind == "marked" {
        let side = if q.fills[0].is_some() { 0usize } else { 1 };
        let mark = mark.unwrap();
        let v = (if side == 0 { mark.bid } else { 200 - mark.ask })
            - s.policy["exitSlippageTicks"].as_i64().unwrap_or(0) - q.limits[side];
        s.one_sided_markout_ticks += v;
        value = json!(v);
    }
    if fills.len() == 1 && kind != "paired" { inc(&mut s.counts, "oneSided"); }
    if q.gap_exposed { inc(&mut s.counts, "gapExposed"); }
    let outcome = json!({
        "policyId": s.policy["id"], "marketId": q.market_id,
        "createdAt": ms_to_iso(q.at as f64), "resolvedAt": ms_to_iso(at as f64),
        "limitsTicks": q.limits, "kind": kind, "counterfactualNetTicks": value,
        "gapExposed": q.gap_exposed, "tradeSources": fills,
        "x": q.x, "decisionSource": q.decision_source,
    });
    s.outcomes.push(outcome.clone());
    on_outcome(&outcome);
    if let Some(m) = s.markets.get_mut(id) { m.quote = None; }
}

impl<D, Q, F, O> RealtimeSpreadEconomics<D, Q, F, O>
where D: FnMut(V) -> V, Q: FnMut(&V), F: FnMut(&V), O: FnMut(&V) {
    fn bank_market(&mut self, bi: usize, id: &str) -> &mut BankMarket {
        self.banks[bi].markets.entry(id.to_string()).or_insert_with(BankMarket::new)
    }

    fn checkpoint(&mut self, bi: usize, id: &str, at: i64) -> Result<(), String> {
        // Read gate values first (immutable phase over the quote).
        let (done, action) = {
            let s = &self.banks[bi];
            let Some(m) = s.markets.get(id) else { return Ok(()); };
            let Some(q) = m.quote.as_ref() else { return Ok(()); };
            let fills_count = q.fills.iter().filter(|f| f.is_some()).count();
            if fills_count == 2 {
                (true, "paired")
            } else if at < q.expiry {
                (true, "")
            } else if fills_count == 0 {
                (true, if q.gap_exposed { "censored" } else { "unfilled" })
            } else {
                let learned = q.fills.iter().filter_map(|f| f.as_ref()).next().unwrap()["learnedAt"]
                    .as_i64().unwrap_or(0);
                let horizon = s.policy["markoutMs"].as_i64().unwrap_or(0)
                    + s.policy["latencyMs"].as_i64().unwrap_or(0);
                if at < learned + horizon {
                    (true, "")
                } else {
                    let mark_ok = m.book.as_ref().map(|b| {
                        b.at >= learned + horizon
                            && at - b.at <= s.policy["maximumBookAgeMs"].as_i64().unwrap_or(0)
                            && b.expiry.map_or(true, |e| e == 0.0 || e.is_nan() || e > at as f64)
                            && {
                                let side = if q.fills[0].is_some() { 0 } else { 1 };
                                let quantity = if side == 0 { b.bid_size } else { b.ask_size };
                                (quantity * s.policy["depthFractionBps"].as_f64().unwrap_or(f64::NAN)
                                    / 10000.0).floor()
                                    >= s.policy["quantity"].as_f64().unwrap_or(f64::NAN)
                            }
                    }).unwrap_or(false);
                    if mark_ok {
                        (true, if q.gap_exposed { "censored" } else { "marked" })
                    } else if at > learned + horizon + s.policy["maximumBookAgeMs"].as_i64().unwrap_or(0) {
                        (true, "censored")
                    } else {
                        (true, "")
                    }
                }
            }
        };
        if !done || action.is_empty() { return Ok(()); }
        let mark_book = self.banks[bi].markets.get(id).and_then(|m| m.book.clone());
        let q = self.banks[bi].markets.get(id).and_then(|m| m.quote.clone()).unwrap();
        let s = &mut self.banks[bi];
        finish(s, id, q, at, action, mark_book.as_ref(), &mut self.on_outcome);
        Ok(())
    }

    fn invalidate(&mut self, id: &str, at: i64) -> Result<(), String> {
        inc(&mut self.metrics, "invalidations");
        self.feature_markets.remove(id);
        for bi in 0..self.banks.len() {
            {
                let m = self.bank_market(bi, id);
                m.book = None;
                m.history = Vec::new();
                m.revision = None;
                if let Some(q) = m.quote.as_mut() { q.gap_exposed = true; }
            }
            self.checkpoint(bi, id, at)?;
        }
        Ok(())
    }

    fn trades(&mut self, id: &str, rows: &[V], at: i64) -> Result<(), String> {
        for t in rows {
            need(t["marketId"].as_str() == Some(id)
                && t["id"].is_string()
                && t["sequence"].as_i64().is_some()
                && t["quantity"].as_i64().is_some()
                && t["quantity"].as_i64().unwrap_or(0) > 0, "TRADE")?;
            let key = format!("{}:{}", id, t["id"].as_str().unwrap_or(""));
            if self.seen_trades.contains(&key) { inc(&mut self.metrics, "duplicateTrades"); continue; }
            self.seen_trades.insert(key);
            inc(&mut self.metrics, "trades");
            let source_ns = instant_ns(t["executedAt"].as_str().unwrap_or("")).map_err(|e| e.0)?;
            need(source_ns <= at as i128 * 1_000_000 + 1_000_000_000, "FUTURE_TRADE")?;
            let price = tick(num(&t["price"]))?;
            {
                let fm = self.feature_markets.entry(id.to_string()).or_insert_with(FeatureMarket::default);
                let sign = fm.book.as_ref().map(|b| {
                    if price >= b.ask { 1.0 } else if price <= b.bid { -1.0 } else { 0.0 }
                }).unwrap_or(0.0);
                fm.trades.push(MicropriceTrade { at, quantity: num(&t["quantity"]), sign });
            }
            for bi in 0..self.banks.len() {
                let mut fills_events: Vec<V> = Vec::new();
                let proceed = {
                    let s = &mut self.banks[bi];
                    let m = s.markets.entry(id.to_string()).or_insert_with(BankMarket::new);
                    match m.quote.as_ref() {
                        Some(q) if !(q.gap_exposed || at >= q.expiry
                            || source_ns < q.activation as i128 * 1_000_000
                            || source_ns >= q.expiry as i128 * 1_000_000) => {
                            let policy_id = s.policy["id"].clone();
                            let q = m.quote.as_mut().unwrap();
                            let mut remaining = t["quantity"].as_i64().unwrap_or(0);
                            for side in 0..2usize {
                                if remaining == 0 { break; }
                                if q.fills[side].is_none()
                                    && (if side == 0 { price < q.limits[0] } else { 200 - price < q.limits[1] }) {
                                    q.fills[side] = Some(json!({
                                        "id": t["id"], "sequence": t["sequence"],
                                        "executedAt": t["executedAt"], "learnedAt": at,
                                        "priceTicks": price,
                                    }));
                                    fills_events.push(json!({
                                        "policyId": policy_id, "marketId": id,
                                        "createdAt": ms_to_iso(q.at as f64),
                                        "learnedAt": ms_to_iso(at as f64),
                                        "side": if side == 0 { "YES" } else { "NO" },
                                        "limitTicks": q.limits[side],
                                        "source": q.fills[side].clone().unwrap(),
                                    }));
                                    remaining -= 1;
                                }
                            }
                            true
                        }
                        _ => false,
                    }
                };
                for e in &fills_events { (self.on_fill)(e); }
                if proceed { self.checkpoint(bi, id, at)?; }
            }
        }
        Ok(())
    }

    fn book(&mut self, id: &str, raw: &V, at: i64) -> Result<(), String> {
        let b = normalized_book(raw, at)?;
        let Some(b) = b else { return Ok(()); };
        inc(&mut self.metrics, "books");
        {
            let fm = self.feature_markets.entry(id.to_string()).or_insert_with(FeatureMarket::default);
            if fm.book.as_ref().map(|x| b.sequence > x.sequence).unwrap_or(true) {
                fm.book = Some(b.clone());
                let push = fm.history.last().map(|h| at - h.at >= 1000).unwrap_or(true);
                if push { fm.history.push(MicropriceHistoryPoint { at, mid: b.mid }); }
                fm.history.retain(|h| at - h.at <= 301000);
                fm.trades.retain(|t| at - t.at <= 120000);
            }
        }
        for bi in 0..self.banks.len() {
            enum BookGate { Skip, Expired, Conflict, Ok }
            let (p, gate) = {
                let s = &mut self.banks[bi];
                let p = s.policy.clone();
                let m = s.markets.entry(id.to_string()).or_insert_with(BankMarket::new);
                if m.book.as_ref().map(|x| b.sequence < x.sequence).unwrap_or(false) {
                    (p, BookGate::Skip)
                } else if m.book.as_ref().map(|x| b.sequence == x.sequence).unwrap_or(false) {
                    let mb = m.book.as_ref().unwrap();
                    if b.bid == mb.bid && b.ask == mb.ask { (p, BookGate::Skip) } else { (p, BookGate::Conflict) }
                } else if b.expiry.map_or(false, |e| e != 0.0 && !e.is_nan() && e <= at as f64) {
                    inc(&mut s.counts, "expiryInvalidations");
                    m.book = None;
                    (p, BookGate::Expired)
                } else {
                    m.book = Some(b.clone());
                    let push = m.history.last()
                        .map(|h| at - h["at"].as_i64().unwrap_or(0) >= 1000).unwrap_or(true);
                    if push { m.history.push(json!({"at":at,"mid":b.bid + b.ask})); }
                    let hist_ms = p["historyMs"].as_i64().unwrap_or(0);
                    m.history.retain(|x| at - x["at"].as_i64().unwrap_or(0) <= hist_ms);
                    (p, BookGate::Ok)
                }
            };
            match gate {
                BookGate::Conflict => need(false, "VERSION_CONFLICT")?,
                BookGate::Skip | BookGate::Expired => continue,
                BookGate::Ok => {}
            }
            self.checkpoint(bi, id, at)?;
            // Skip-gate chain (JS `continue` conditions) then quote creation.
            let fm_snapshot = self.feature_markets.get(id).cloned().unwrap_or_default();
            let source_record = self.source_record.clone();
            let s = &mut self.banks[bi];
            let m = s.markets.get_mut(id).unwrap();
            let mids: Vec<f64> = m.history.iter().map(|x| num(&x["mid"])).collect();
            let mid_range = mids.iter().cloned().fold(f64::NEG_INFINITY, js_max)
                - mids.iter().cloned().fold(f64::INFINITY, js_min);
            let spread = b.ask - b.bid;
            let improvement = p["improvementTicks"].as_i64().unwrap_or(0);
            let limits = [b.bid + improvement, 200 - b.ask + improvement];
            let depth_ok = (js_min(b.bid_size, b.ask_size)
                * p["depthFractionBps"].as_f64().unwrap_or(f64::NAN) / 10000.0).floor()
                >= p["quantity"].as_f64().unwrap_or(f64::NAN);
            let skip = m.quote.is_some()
                || at as f64 - m.sample_at < p["sampleEveryMs"].as_f64().unwrap_or(f64::NAN)
                || m.history.is_empty()
                || at - m.history[0]["at"].as_i64().unwrap_or(0)
                    < p["minimumSpanMs"].as_i64().unwrap_or(0)
                || mid_range > 2.0 * p["maximumMidpointRangeTicks"].as_f64().unwrap_or(f64::NAN)
                || spread > 10
                || limits.iter().any(|x| *x < 10 || *x > 190)
                || limits[0] + limits[1] > 199
                || limits[0] >= b.ask
                || limits[1] >= 200 - b.bid
                || !depth_ok;
            if skip { continue; }
            let activation = std::cmp::max(at + p["latencyMs"].as_i64().unwrap_or(0),
                (b.source_ns / 1_000_000) as i64 + 2000);
            m.sample_at = at as f64;
            let candidate_x = microprice_features(&MicropriceMarket {
                book: fm_snapshot.book.as_ref().map(|x| x.as_micro_book()),
                history: fm_snapshot.history,
                trades: fm_snapshot.trades,
            }, &b.as_micro_book(), at)?;
            let candidate = json!({
                "policyId": p["id"], "marketId": id,
                "createdAt": ms_to_iso(at as f64),
                "activationAt": ms_to_iso(activation as f64),
                "expiresAt": ms_to_iso((at + p["quoteMs"].as_i64().unwrap_or(0)) as f64),
                "limitsTicks": limits, "x": candidate_x, "decisionSource": source_record,
            });
            if (self.decide_quote)(candidate.clone()) != json!(true) {
                inc(&mut self.metrics, "rejectedQuotes");
                continue;
            }
            m.quote = Some(Quote {
                market_id: id.to_string(), at, activation,
                expiry: at + p["quoteMs"].as_i64().unwrap_or(0),
                limits, fills: [None, None], gap_exposed: false,
                x: candidate["x"].clone(), decision_source: source_record,
            });
            inc(&mut s.counts, "quotes");
            (self.on_quote)(&candidate);
        }
        Ok(())
    }

    /// consume(row, {sourceId})
    pub fn consume(&mut self, row: &V, source_id: &str) -> Result<(), String> {
        need(row["format"] == "sig-feed-capture" && row["schemaVersion"] == json!(1)
            && row["captureIndex"].as_i64().is_some(), "ENVELOPE")?;
        let at_f = row["observedAt"].as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN);
        need(at_f.fract() == 0.0 && at_f.abs() <= 9_007_199_254_740_991.0
            && at_f >= self.last_at as f64, "CLOCK")?;
        let at = at_f as i64;
        if self.source.as_deref() != Some(source_id) {
            self.source = Some(source_id.to_string());
            self.last_index = -1;
            inc(&mut self.metrics, "sessions");
            self.feature_markets.clear();
            for s in &mut self.banks {
                for (_, m) in s.markets.iter_mut() {
                    m.book = None;
                    m.history = Vec::new();
                    m.revision = None;
                    if let Some(q) = m.quote.as_mut() { q.gap_exposed = true; }
                }
            }
        }
        need(row["captureIndex"].as_i64().unwrap_or(i64::MIN) == self.last_index + 1, "SOURCE_INDEX")?;
        self.last_index = row["captureIndex"].as_i64().unwrap_or(0);
        self.last_at = at;
        self.source_record = json!({"file":source_id,"captureIndex":row["captureIndex"],
            "observedAt":row["observedAt"]});
        let id = topic_market(row["topic"].as_str().unwrap_or(""));
        need(id.is_some(), "TOPIC")?;
        let id = id.unwrap();
        if ["subscribed", "disconnected", "stopped", "invalidation", "rejected"]
            .contains(&row["kind"].as_str().unwrap_or("")) {
            self.invalidate(&id, at)?;
            return Ok(());
        }
        if row["kind"] == "snapshot" {
            for b in row["books"].as_array().cloned().unwrap_or_default() {
                self.book(&id, &b, at)?;
            }
            return Ok(());
        }
        if row["kind"] != "event" { return Ok(()); }
        inc(&mut self.metrics, "events");
        if row["event"] != "market_batch" {
            self.invalidate(&id, at)?;
            return Ok(());
        }
        let p = row["payload"].clone();
        need(js_truthy(Some(&p)) && js_truthy(p.get("delivery")), "DELIVERY")?;
        let mut gap = p["resyncRequired"] == json!(true);
        for s in &mut self.banks {
            let m = s.markets.entry(id.clone()).or_insert_with(BankMarket::new);
            let prev_rev = num(&p["delivery"]["previousRevision"]);
            if let Some(rev) = m.revision {
                if prev_rev > rev { gap = true; }
            }
            m.revision = Some(js_max(m.revision.unwrap_or(0.0), num(&p["delivery"]["revision"])));
        }
        if gap {
            inc(&mut self.metrics, "gaps");
            self.invalidate(&id, at)?;
        }
        let settled = p["marketSettled"].as_array().map(|a| !a.is_empty()).unwrap_or(false);
        let null_seq = p["trades"].as_array().map(|a| a.iter()
            .any(|t| t.get("sequence").map_or(false, |v| v.is_null()))).unwrap_or(false);
        let dirty_missing = p["bookDirty"].as_array().map(|ds| ds.iter()
            .any(|d| !p["books"].as_array().map(|bs| bs.iter()
                .any(|b| b["exchangeId"] == d["exchangeId"])).unwrap_or(false))).unwrap_or(false);
        if settled || null_seq || dirty_missing {
            self.invalidate(&id, at)?;
            return Ok(());
        }
        self.trades(&id, p["trades"].as_array().map(|a| a.as_slice()).unwrap_or(&[]), at)?;
        for b in p["books"].as_array().cloned().unwrap_or_default() {
            self.book(&id, &b, at)?;
        }
        Ok(())
    }

    /// report()
    pub fn report(&self) -> V {
        let policies: Vec<V> = self.banks.iter().map(|s| json!({
            "policy": s.policy.clone(), "counts": s.counts.clone(),
            "unresolved": s.markets.values().filter(|m| m.quote.is_some()).count(),
            "pairedGainTicks": s.paired_gain_ticks,
            "oneSidedMarkoutTicks": s.one_sided_markout_ticks,
            "conditionalDiagnosticTicks": s.paired_gain_ticks + s.one_sided_markout_ticks,
            "outcomes": s.outcomes.clone(),
        })).collect();
        json!({
            "schemaVersion": 1,
            "at": ms_to_iso(self.last_at.max(0) as f64),
            "mode": "offline_overlapping_quote_opportunity_diagnostic",
            "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false,
            "fees": 0, "metrics": self.metrics.clone(),
            "policies": policies,
            "limitations": [
                "Overlapping opportunities are not a funded portfolio and cannot be added as returns.",
                "Strict-through anonymous prints do not establish actual queue position or execution.",
                "Research uses full pushed snapshots; production additionally requires fresh REST/account eligibility and validated continuity.",
                "Any quote exposed to a known capture gap is censored; unknown transport losses may remain.",
                "One-sided markouts require a later fresh book and depth, with one-tick exit slippage."
            ],
        })
    }
}

/// createRealtimeSpreadEconomics({policies,decideQuote,onQuote,onFill,onOutcome})
pub fn create_realtime_spread_economics<D, Q, F, O>(
    policies: Option<Vec<V>>, decide_quote: D, on_quote: Q, on_fill: F, on_outcome: O,
) -> RealtimeSpreadEconomics<D, Q, F, O>
where D: FnMut(V) -> V, Q: FnMut(&V), F: FnMut(&V), O: FnMut(&V) {
    let banks = policies.unwrap_or_else(realtime_economics_policies).into_iter()
        .map(|p| BankState {
            policy: p, markets: HashMap::new(), counts: fresh_counts(),
            paired_gain_ticks: 0, one_sided_markout_ticks: 0, outcomes: Vec::new(),
        }).collect();
    RealtimeSpreadEconomics {
        banks, seen_trades: HashSet::new(), feature_markets: HashMap::new(),
        last_at: 0, last_index: -1, source: None, source_record: V::Null,
        metrics: json!({"events":0,"books":0,"trades":0,"duplicateTrades":0,"gaps":0,
            "invalidations":0,"sessions":0,"rejectedQuotes":0}),
        decide_quote, on_quote, on_fill, on_outcome,
    }
}
