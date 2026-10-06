// live-paper/dip-markout-study.mjs — causal quote+depth markout study over
// captured price and book journals. CLI entry omitted.
use crate::bid_anchor::bid_anchor_policy;
use crate::book_publication::{apply_book_publication, create_book_publication_state};
use crate::breadth::validate_universe;
use crate::bulk_dip::price as decimal;
use crate::bulk_dip_diagnostic::diagnose_quoted_dip;
use crate::market_price_publication::validate_price_observation_pub;
use crate::model::digest;
use baal_paper::fixed::{big_amount, instant, money, TICK};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_stringify, js_truthy, sha256};
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::HashMap;
use std::path::Path;

fn need(ok: bool, reason: &str) -> Result<(), String> {
    if !ok { return Err(format!("DIP_MARKOUT_{}", reason)); }
    Ok(())
}
fn dp(v: &V) -> Result<i64, String> { instant(v.as_str().unwrap_or("")) }
fn complete(b: &[u8]) -> Vec<u8> {
    let end = b.iter().rposition(|&x| x == b'\n').map(|i| i + 1).unwrap_or(0);
    b[..end].to_vec()
}
fn parse_lines(b: &[u8]) -> Result<Vec<V>, String> {
    let text = std::str::from_utf8(b).map_err(|e| e.to_string())?;
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.pop();
    lines.iter().map(|l| serde_json::from_str::<V>(l).map_err(|e| e.to_string())).collect()
}
fn valid_quote(q: &V) -> bool {
    q["option"].as_str() == Some("YES") && !js_truthy(q.get("crossed"))
        && q["bestBid"].as_f64().is_some() && q["bestAsk"].as_f64().is_some()
        && q["bestBid"].as_f64().unwrap() > 0.0 && q["bestAsk"].as_f64().unwrap() < 1.0
        && q["bestBid"].as_f64().unwrap() < q["bestAsk"].as_f64().unwrap()
}
/// JS primitive-to-string coercion for object key/index concat semantics.
fn js_str(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => n.to_string(),
        V::Bool(b) => b.to_string(),
        V::Null => "null".to_string(),
        other => js_stringify(other),
    }
}
fn depth_price(levels: &V) -> Result<Option<BigInt>, String> {
    for x in levels.as_array().cloned().unwrap_or_default() {
        if (x["quantity"].as_f64().unwrap_or(f64::NAN) / 100.0).floor() >= 1.0 {
            return Ok(Some(big_amount(x["price"].as_str().unwrap_or(""))?));
        }
    }
    Ok(None)
}

/// DIP_MARKOUT_PROTOCOL (frozen).
pub fn dip_markout_protocol() -> V {
    json!({"schemaVersion":1,"mode":"causal_quote_and_depth_markout_study",
        "grossConcessions":["0.005000","0.010000","0.015000","0.020000"],
        "latenciesMs":[1000,3000],"cooldownMs":900000,"horizonMs":300000,
        "labelLatenessMs":60000,"candidateLifetimeMs":60000,"entryLifetimeMs":60000,
        "referencePolicy":bid_anchor_policy(),"feeBps":0,"entryLimitPadTicks":1,
        "depthFractionBps":100,"orderShares":1,
        "priceScenarios":["observed_depth_price","one_adverse_tick_each_side"],
        "developmentPurgeMs":480000})
}

/// identity() — sha256 of the eight frozen source files. `live_paper_dir` is
/// the live-paper directory; '../paper/fixed.mjs' resolves beside it.
pub fn dip_markout_identity(live_paper_dir: &Path) -> Result<V, String> {
    let mut out = Map::new();
    for p in ["./dip-markout-study.mjs", "./bulk-dip-diagnostic.mjs", "./bid-anchor.mjs",
        "./model.mjs", "./book-publication.mjs", "./market-price-publication.mjs",
        "./breadth.mjs", "../paper/fixed.mjs"] {
        let rel = p.trim_start_matches("./");
        let path = if p.starts_with("../") {
            live_paper_dir.join("..").join(rel)
        } else {
            live_paper_dir.join(rel)
        };
        out.insert(p.to_string(), json!(sha256(&std::fs::read(path).map_err(|e| e.to_string())?)));
    }
    Ok(V::Object(out))
}

/// createMarkoutProtocol(cutoff, universe)
pub fn create_markout_protocol(cutoff: &str, universe: &V, live_paper_dir: &Path) -> Result<V, String> {
    instant(cutoff)?;
    Ok(json!({
        "name": "dip-markout-20261003-v1", "cutoff": cutoff,
        "universeHash": validate_universe(universe, universe["tournamentId"].as_str().unwrap_or(""))?["hash"],
        "protocol": dip_markout_protocol(),
        "codeIdentity": dip_markout_identity(live_paper_dir)?,
    }))
}

/// verifyMarkoutProtocol(bank, universe)
pub fn verify_markout_protocol(bank: &V, universe: &V, live_paper_dir: &Path) -> Result<V, String> {
    need(digest(&bank["protocol"]) == digest(&dip_markout_protocol())
        && bank["universeHash"]
            == validate_universe(universe, universe["tournamentId"].as_str().unwrap_or(""))?["hash"],
        "PROTOCOL")?;
    instant(bank["cutoff"].as_str().unwrap_or(""))?;
    need(digest(&bank["codeIdentity"]) == digest(&dip_markout_identity(live_paper_dir)?), "FROZEN_CODE")?;
    Ok(bank.clone())
}

struct Study<'a> {
    p: V, bank: &'a V,
    history: HashMap<String, Vec<V>>,
    boundaries_global: Option<String>,
    boundaries_markets: HashMap<String, String>,
    cooldowns: HashMap<String, i64>,
    events: Vec<V>,
    active: HashMap<String, Vec<usize>>,
    counts: Map<String, V>,
    market_by_exchange: Vec<(V, V)>,
}

impl<'a> Study<'a> {
    fn inc(&mut self, k: &str) {
        self.counts.insert(k.to_string(),
            json!(self.counts.get(k).and_then(|v| v.as_i64()).unwrap_or(0) + 1));
    }
    fn stop_event(&mut self, i: usize, status: &str) {
        let e = &mut self.events[i];
        if e["quote"]["status"] == "pending" { e["quote"]["status"] = json!(status); }
        if let Some(depth) = e["depth"].as_object_mut() {
            for (_, trial) in depth.iter_mut() {
                if ["awaiting_confirmation", "awaiting_entry", "awaiting_exit"]
                    .contains(&trial["status"].as_str().unwrap_or("")) {
                    trial["status"] = json!(status);
                }
            }
        }
    }
    /// JS invalidate(id, at, status): `id===null` -> global; `undefined` -> key 'undefined'.
    /// None = JS undefined (missing); Some(V::Null) = JS null -> global.
    fn invalidate(&mut self, id: Option<&V>, at: &str, status: &str) {
        let global = matches!(id, Some(V::Null));
        let ids: Vec<String> = if global {
            let mut all: Vec<String> = self.history.keys().cloned().collect();
            for k in self.active.keys() { if !all.contains(k) { all.push(k.clone()); } }
            all
        } else {
            vec![id.map(js_str).unwrap_or_else(|| "undefined".to_string())]
        };
        for market in ids {
            self.history.remove(&market);
            let acts = self.active.get(&market).cloned().unwrap_or_default();
            for i in acts { self.stop_event(i, status); }
            self.active.insert(market, Vec::new());
        }
        if global { self.boundaries_global = Some(at.to_string()); }
        else { self.boundaries_markets.insert(id.map(js_str).unwrap_or_else(|| "undefined".to_string()), at.to_string()); }
    }

    fn visit_price(&mut self, q: &V, snapshot: &V, row: &V) -> Result<(), String> {
        let id = q["marketId"].as_str().unwrap_or("").to_string();
        let now = dp(&snapshot["receivedAt"])?;
        let available = dp(&row["at"])?;
        let request = dp(&snapshot["requestStartedAt"])?;
        let mut permitted = true;
        for at in [self.boundaries_global.as_ref().map(|s| s.as_str()),
            self.boundaries_markets.get(&id).map(|s| s.as_str())] {
            if let Some(a) = at {
                if !(request > instant(a)?) { permitted = false; }
            }
        }
        if !permitted { self.inc("price_request_before_boundary"); return Ok(()); }
        if !valid_quote(q) {
            self.invalidate(q.get("marketId"), row["at"].as_str().unwrap_or(""), "invalid_quote");
            self.inc("invalid_quote");
            return Ok(());
        }
        if let Some(acts) = self.active.get(&id).cloned() {
            for i in acts {
                let e = &mut self.events[i];
                if e["quote"]["status"] != "pending" { continue; }
                let target = dp(&e["availableAt"])? + self.p["horizonMs"].as_i64().unwrap_or(0);
                if now - dp(&e["quote"]["lastReceiptAt"])?
                    > self.p["referencePolicy"]["maximumGapMs"].as_i64().unwrap_or(0) {
                    e["quote"]["status"] = json!("coverage_gap");
                    continue;
                }
                e["quote"]["lastReceiptAt"] = snapshot["receivedAt"].clone();
                if request < target { continue; }
                if now > target + self.p["labelLatenessMs"].as_i64().unwrap_or(0) {
                    e["quote"]["status"] = json!("late_quote");
                    continue;
                }
                let exit_bid = big_amount(&decimal(q["bestBid"].as_f64().unwrap_or(f64::NAN))?)?;
                let entry_ask = big_amount(e["quote"]["entryAsk"].as_str().unwrap_or(""))?;
                let mut quote = e["quote"].as_object().cloned().unwrap_or_default();
                quote.insert("status".to_string(), json!("observed"));
                quote.insert("outcomeReceivedAt".to_string(), snapshot["receivedAt"].clone());
                quote.insert("outcomeRequestStartedAt".to_string(), snapshot["requestStartedAt"].clone());
                quote.insert("exitBid".to_string(), json!(decimal(q["bestBid"].as_f64().unwrap_or(f64::NAN))?));
                quote.insert("rawMarkout".to_string(), json!(money(&(&exit_bid - &entry_ask))));
                quote.insert("adverseMarkout".to_string(),
                    json!(money(&(&exit_bid - &entry_ask - BigInt::from(2 * TICK)))));
                quote.insert("outcomeSequence".to_string(), row["sequence"].clone());
                quote.insert("outcomeHash".to_string(), row["hash"].clone());
                e["quote"] = V::Object(quote);
            }
        }
        let points = self.history.get(&id).cloned().unwrap_or_default();
        let gate = diagnose_quoted_dip(&points, q, snapshot["receivedAt"].as_str().unwrap_or(""),
            Some(&self.p["referencePolicy"]))?;
        let only_concession = gate["failures"].as_array()
            .map(|f| f.iter().all(|r| r == "insufficient_concession")).unwrap_or(false);
        if !gate["features"].is_null() && only_concession {
            let gross = big_amount(gate["features"]["netConcession"].as_str().unwrap_or(""))?
                + BigInt::from(2 * TICK);
            let mut thresholds: Vec<String> = Vec::new();
            for threshold in self.p["grossConcessions"].as_array().cloned().unwrap_or_default() {
                let key = format!("{}:{}", threshold.as_str().unwrap_or(""), id);
                if gross >= big_amount(threshold.as_str().unwrap_or(""))?
                    && available >= self.cooldowns.get(&key).cloned().unwrap_or(0) {
                    thresholds.push(threshold.as_str().unwrap_or("").to_string());
                }
            }
            if !thresholds.is_empty() {
                let cutoff = instant(self.bank["cutoff"].as_str().unwrap_or(""))?;
                let purge = self.p["developmentPurgeMs"].as_i64().unwrap_or(0);
                let phase = if available >= cutoff { "prospective" }
                    else if available + purge <= cutoff { "development" } else { "purged_boundary" };
                let mut depth = Map::new();
                for latency in self.p["latenciesMs"].as_array().cloned().unwrap_or_default() {
                    let lm = latency.as_i64().unwrap_or(0);
                    let ask = big_amount(&decimal(q["bestAsk"].as_f64().unwrap_or(f64::NAN))?)?;
                    depth.insert(lm.to_string(), json!({
                        "status": "awaiting_confirmation", "latencyMs": lm,
                        "maximumAsk": money(&(ask + BigInt::from(TICK))),
                        "expiresAt": ms_to_iso((available + self.p["candidateLifetimeMs"].as_i64().unwrap_or(0)) as f64),
                    }));
                }
                let e = json!({
                    "id": format!("{}:{}", id, js_str(&row["sequence"])),
                    "marketId": q["marketId"], "exchangeId": q["exchangeId"],
                    "availableAt": row["at"], "receivedAt": snapshot["receivedAt"],
                    "priceSequence": row["sequence"], "priceHash": row["hash"],
                    "phase": phase, "thresholds": thresholds, "reference": gate["features"],
                    "rawConcession": money(&gross),
                    "quote": {"status":"pending","entryAsk":decimal(q["bestAsk"].as_f64().unwrap_or(f64::NAN))?,
                        "lastReceiptAt":snapshot["receivedAt"]},
                    "depth": V::Object(depth),
                });
                self.events.push(e);
                let idx = self.events.len() - 1;
                self.active.entry(id.clone()).or_default().push(idx);
                for threshold in &thresholds {
                    self.cooldowns.insert(format!("{}:{}", threshold, id),
                        available + self.p["cooldownMs"].as_i64().unwrap_or(0));
                }
                self.inc("dip_events");
            }
        }
        let minute = now.div_euclid(60000);
        let mut points = points;
        if !points.iter().any(|x| x["minute"].as_i64() == Some(minute)) {
            points.push(json!({"minute":minute,"at":snapshot["receivedAt"],
                "bid":decimal(q["bestBid"].as_f64().unwrap_or(f64::NAN))?}));
        }
        let hist_ms = self.p["referencePolicy"]["historyMs"].as_i64().unwrap_or(0);
        let mut kept = Vec::new();
        for x in points {
            if dp(&x["at"])? >= now - hist_ms { kept.push(x); }
        }
        self.history.insert(id, kept);
        Ok(())
    }

    fn visit_book(&mut self, row: &V) -> Result<(), String> {
        let kind = row["kind"].as_str().unwrap_or("");
        if ["session_start", "session_stop"].contains(&kind) || kind == "invalidation" {
            // session_start/stop -> JS passes literal null (global); 'invalidation' passes
            // payload.marketId verbatim (undefined -> 'undefined' key, null -> global).
            let target_owned;
            let target: Option<&V> = if kind == "invalidation" {
                row["payload"].get("marketId")
            } else {
                target_owned = V::Null;
                Some(&target_owned)
            };
            self.invalidate(target, row["at"].as_str().unwrap_or(""), "source_boundary");
            self.inc("book_boundaries");
            return Ok(());
        }
        if kind != "book" { return Ok(()); }
        let b = &row["payload"]["book"];
        let at = dp(&row["at"])?;
        let request = dp(&b["requestStartedAt"])?;
        let mid = b["marketId"].as_str().unwrap_or("").to_string();
        let horizon = self.p["horizonMs"].as_i64().unwrap_or(0);
        let label_late = self.p["labelLatenessMs"].as_i64().unwrap_or(0);
        let entry_life = self.p["entryLifetimeMs"].as_i64().unwrap_or(0);
        let max_spread = big_amount(self.p["referencePolicy"]["maximumSpread"].as_str().unwrap_or(""))?;
        if let Some(acts) = self.active.get(&mid).cloned() {
            for i in acts {
                for (latency_key, _tv) in self.events[i]["depth"].as_object().cloned().unwrap_or_default() {
                    let e = &mut self.events[i];
                    let t = &mut e["depth"][&latency_key];
                    match t["status"].as_str().unwrap_or("") {
                        "awaiting_confirmation" => {
                            if at > dp(&t["expiresAt"])? {
                                t["status"] = json!("confirmation_not_observed");
                                continue;
                            }
                            let avail = dp(&e["availableAt"])?;
                            let t = &mut e["depth"][&latency_key];
                            if request <= avail { continue; }
                            let ask = depth_price(&b["asks"])?;
                            let bid = depth_price(&b["bids"])?;
                            if ask.is_none() || bid.is_none() {
                                self.inc("confirmation_depth_unavailable");
                                continue;
                            }
                            let ask = ask.unwrap();
                            let spread = big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))?
                                - big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?;
                            if ask > big_amount(t["maximumAsk"].as_str().unwrap_or(""))?
                                || spread > max_spread {
                                self.inc("confirmation_price_rejected");
                                continue;
                            }
                            t["status"] = json!("awaiting_entry");
                            t["preparedAt"] = row["at"].clone();
                            t["preparedVersion"] = b["versionKey"].clone();
                            t["preparationSequence"] = row["sequence"].clone();
                            t["preparationHash"] = row["hash"].clone();
                            t["limit"] = json!(money(&(ask + BigInt::from(TICK))));
                            t["expiresAt"] = json!(ms_to_iso((at + entry_life) as f64));
                        }
                        "awaiting_entry" => {
                            if at > dp(&t["expiresAt"])? {
                                t["status"] = json!("entry_not_observed");
                                continue;
                            }
                            if request < dp(&t["preparedAt"])? + t["latencyMs"].as_i64().unwrap_or(0)
                                || b["versionKey"] == t["preparedVersion"] { continue; }
                            let ask = depth_price(&b["asks"])?;
                            if ask.is_none() {
                                self.inc("entry_depth_unavailable");
                                continue;
                            }
                            let ask = ask.unwrap();
                            if ask.clone() + BigInt::from(TICK)
                                > big_amount(t["limit"].as_str().unwrap_or(""))? {
                                self.inc("entry_price_rejected");
                                continue;
                            }
                            t["status"] = json!("awaiting_exit");
                            t["entryRawPrice"] = json!(money(&ask));
                            t["entryAdversePrice"] = json!(money(&(ask + BigInt::from(TICK))));
                            t["entryReceivedAt"] = b["receivedAt"].clone();
                            t["entryRequestStartedAt"] = b["requestStartedAt"].clone();
                            t["entryAvailableAt"] = row["at"].clone();
                            t["entryVersion"] = b["versionKey"].clone();
                            t["entrySequence"] = row["sequence"].clone();
                            t["entryHash"] = row["hash"].clone();
                            t["dueAt"] = json!(ms_to_iso((at + horizon) as f64));
                            t["expiresAt"] = json!(ms_to_iso((at + horizon + label_late) as f64));
                        }
                        "awaiting_exit" => {
                            if at > dp(&t["expiresAt"])? {
                                t["status"] = json!("exit_not_observed");
                                continue;
                            }
                            if request < dp(&t["dueAt"])? || b["versionKey"] == t["entryVersion"] { continue; }
                            let bid = depth_price(&b["bids"])?;
                            if bid.is_none() {
                                self.inc("exit_depth_unavailable");
                                continue;
                            }
                            let bid = bid.unwrap();
                            let entry_raw = big_amount(t["entryRawPrice"].as_str().unwrap_or(""))?;
                            let entry_adv = big_amount(t["entryAdversePrice"].as_str().unwrap_or(""))?;
                            t["status"] = json!("observed");
                            t["exitRawPrice"] = json!(money(&bid));
                            t["exitAdversePrice"] = json!(money(&(&bid - BigInt::from(TICK))));
                            t["exitReceivedAt"] = b["receivedAt"].clone();
                            t["exitRequestStartedAt"] = b["requestStartedAt"].clone();
                            t["exitSequence"] = row["sequence"].clone();
                            t["exitHash"] = row["hash"].clone();
                            t["rawMarkout"] = json!(money(&(&bid - &entry_raw)));
                            t["adverseMarkout"] = json!(money(&(&bid - BigInt::from(TICK) - &entry_adv)));
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }
}

/// metrics(series) — statuses + observed/adverse markout aggregates.
fn metrics(series: &[V]) -> Result<V, String> {
    let mut statuses = Map::new();
    let mut done: Vec<&V> = Vec::new();
    for s in series {
        let k = s["status"].as_str().unwrap_or("null");
        statuses.insert(k.to_string(),
            json!(statuses.get(k).and_then(|v| v.as_i64()).unwrap_or(0) + 1));
        if s["status"] == "observed" { done.push(s); }
    }
    let measure = |key: &str| -> Result<V, String> {
        let mut values: Vec<BigInt> = done.iter()
            .map(|x| big_amount(x[key].as_str().unwrap_or(""))).collect::<Result<_, _>>()?;
        values.sort();
        let total: BigInt = values.iter().cloned().sum();
        Ok(json!({
            "count": values.len(),
            "positive": values.iter().filter(|x| **x > BigInt::from(0)).count(),
            "negative": values.iter().filter(|x| **x < BigInt::from(0)).count(),
            "zero": values.iter().filter(|x| **x == BigInt::from(0)).count(),
            "average": if values.is_empty() { V::Null } else { json!(money(&(total / BigInt::from(values.len() as i64)))) },
            "median": if values.is_empty() { V::Null } else { json!(money(&values[(values.len() - 1) / 2])) },
            "minimum": if values.is_empty() { V::Null } else { json!(money(&values[0])) },
            "maximum": if values.is_empty() { V::Null } else { json!(money(values.last().unwrap())) },
        }))
    };
    Ok(json!({
        "events": series.len(), "statuses": V::Object(statuses),
        "observedPrices": measure("rawMarkout")?,
        "adverseTicks": measure("adverseMarkout")?,
    }))
}

/// studyDipMarkouts({priceBytes, bookBytes, universe, bank})
pub fn study_dip_markouts(price_bytes: &[u8], book_bytes: &[u8], universe: &V, bank: &V,
    live_paper_dir: &Path) -> Result<V, String> {
    verify_markout_protocol(bank, universe, live_paper_dir)?;
    let u = validate_universe(universe, universe["tournamentId"].as_str().unwrap_or(""))?;
    let p = bank["protocol"].clone();
    let pb = complete(price_bytes);
    let bb = complete(book_bytes);
    let prices = parse_lines(&pb)?;
    let books = parse_lines(&bb)?;
    need(!prices.is_empty() && !books.is_empty(), "EMPTY")?;
    let mut publication = create_book_publication_state(universe)?;
    for row in &books { publication = apply_book_publication(&publication, row)?; }
    let mut sequence = 0i64;
    let mut previous = V::String(String::new());
    let mut last_published = i64::MIN;
    let mut last_receipt = i64::MIN;
    for row in &prices {
        let hash = row["hash"].clone();
        let mut body = row.clone();
        body.as_object_mut().map(|o| o.shift_remove("hash"));
        let snapshot = body["event"]["snapshot"].clone();
        sequence += 1;
        need(body["sequence"].as_i64() == Some(sequence)
            && body["previous"] == previous
            && json!(digest(&body)) == hash
            && body["state"].is_null()
            && body["event"]["type"] == "market_price_observation", "PRICE_CHAIN")?;
        validate_price_observation_pub(&snapshot, &u)?;
        let at = dp(&body["at"])?;
        let received = dp(&snapshot["receivedAt"])?;
        need(at >= received && at >= last_published && received >= last_receipt, "PRICE_CLOCK")?;
        previous = hash;
        last_published = at;
        last_receipt = received;
    }
    let mut merged: Vec<(bool, &V, i64)> = Vec::new();
    for row in &prices { merged.push((false, row, dp(&row["at"])?)); }
    for row in &books { merged.push((true, row, dp(&row["at"])?)); }
    merged.sort_by(|a, b| {
        a.2.cmp(&b.2).then(a.0.cmp(&b.0))
            .then(a.1["sequence"].as_i64().unwrap_or(0).cmp(&b.1["sequence"].as_i64().unwrap_or(0)))
    });
    let market_by_exchange: Vec<(V, V)> = u["exchanges"].as_array().cloned().unwrap_or_default()
        .iter().map(|x| (x["exchangeId"].clone(), x["marketId"].clone())).collect();
    let mut st = Study {
        p: p.clone(), bank,
        history: HashMap::new(), boundaries_global: None,
        boundaries_markets: HashMap::new(), cooldowns: HashMap::new(),
        events: Vec::new(), active: HashMap::new(), counts: Map::new(),
        market_by_exchange,
    };
    for (is_book, row, _) in &merged {
        if *is_book {
            st.visit_book(row)?;
        } else {
            for id in row["event"]["snapshot"]["missingIds"].as_array().cloned().unwrap_or_default() {
                let market = st.market_by_exchange.iter().find(|(x, _)| *x == id)
                    .map(|(_, m)| m.clone());
                st.invalidate(market.as_ref(), row["at"].as_str().unwrap_or(""), "missing_quote");
            }
            for q in row["event"]["snapshot"]["quotes"].as_array().cloned().unwrap_or_default() {
                st.visit_price(&q, &row["event"]["snapshot"], row)?;
            }
        }
    }
    let through = merged.last().map(|(_, r, _)| r["at"].clone()).unwrap_or(V::Null);
    let through_ms = dp(&through)?;
    for i in 0..st.events.len() {
        let e = &mut st.events[i];
        if e["quote"]["status"] == "pending" {
            e["quote"]["status"] = json!(
                if through_ms > dp(&e["availableAt"])? + p["horizonMs"].as_i64().unwrap_or(0)
                    + p["labelLatenessMs"].as_i64().unwrap_or(0) {
                    "outcome_not_observed"
                } else { "right_censored" });
        }
        if let Some(depth) = e["depth"].as_object().cloned() {
            for (k, _) in depth {
                let t = &mut e["depth"][&k];
                if t["status"].as_str().unwrap_or("").starts_with("awaiting_") {
                    let new_status = if through_ms > dp(&t["expiresAt"])? {
                        format!("{}_not_observed",
                            t["status"].as_str().unwrap_or("").replace("awaiting_", ""))
                    } else { "right_censored".to_string() };
                    t["status"] = json!(new_status);
                }
            }
        }
    }
    let mut groups = Map::new();
    for phase in ["development", "purged_boundary", "prospective"] {
        let mut per_threshold = Map::new();
        for threshold in p["grossConcessions"].as_array().cloned().unwrap_or_default() {
            let th = threshold.as_str().unwrap_or("");
            let chosen: Vec<&V> = st.events.iter()
                .filter(|e| e["phase"] == phase && e["thresholds"].as_array()
                    .map(|t| t.iter().any(|x| x.as_str() == Some(th))).unwrap_or(false))
                .collect();
            let markets = chosen.iter().map(|e| e["marketId"].as_str().unwrap_or("").to_string())
                .collect::<std::collections::HashSet<_>>().len();
            let quote_series: Vec<V> = chosen.iter().map(|e| e["quote"].clone()).collect();
            let mut depth_markouts = Map::new();
            for latency in p["latenciesMs"].as_array().cloned().unwrap_or_default() {
                let lk = latency.as_i64().unwrap_or(0).to_string();
                let series: Vec<V> = chosen.iter()
                    .map(|e| e["depth"][&lk].clone()).collect();
                depth_markouts.insert(lk, metrics(&series)?);
            }
            per_threshold.insert(th.to_string(), json!({
                "events": chosen.len(), "markets": markets,
                "quoteMarkouts": metrics(&quote_series)?,
                "depthMarkouts": V::Object(crate::liquidity::js_order_object(depth_markouts)),
            }));
        }
        groups.insert(phase.to_string(), V::Object(per_threshold));
    }
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0);
    Ok(json!({
        "mode": "causal_quote_and_depth_markout_study",
        "createdAt": ms_to_iso(now_ms),
        "bankHash": digest(bank), "cutoff": bank["cutoff"], "protocol": p,
        "sources": {
            "price": {"bytes": pb.len(), "sha256": sha256(&pb), "sequence": sequence,
                "hash": previous, "incompleteTailBytes": price_bytes.len() - pb.len()},
            "book": {"bytes": bb.len(), "sha256": sha256(&bb),
                "sequence": publication["sequence"], "hash": publication["hash"],
                "incompleteTailBytes": book_bytes.len() - bb.len()},
        },
        "through": through, "counts": V::Object(st.counts), "groups": V::Object(groups),
        "events": st.events,
        "networkRequests": 0, "competitionOrders": 0, "paperFills": 0,
        "profitabilityEstablished": false,
        "limitations": [
            "Depth pairs are historical causal quote diagnostics, not actual orders, fills, wallet P&L or guaranteed execution.",
            "Bulk quotes have unknown source age/size; source publication is hypothetical availability, not a live consumer receipt.",
            "Missing, revoked and right-censored paths remain in denominators. Complete-path averages may be selected and are not profitability estimates.",
            "Thresholds and latency paths overlap and market/common-event observations correlate; no independent sample or winner claim."
        ],
    }))
}
