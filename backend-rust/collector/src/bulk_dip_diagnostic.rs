// live-paper/bulk-dip-diagnostic.mjs — quote-only dip gate diagnostics over a
// price-capture journal. Never fills or P&L. CLI entry omitted.
use crate::bid_anchor::bid_anchor_policy;
use crate::breadth::validate_universe;
use crate::bulk_dip::{price, quoted_dip_candidate};
use crate::market_price_publication::validate_price_observation_pub;
use crate::model::digest;
use baal_paper::fixed::{big_amount, money, TICK};
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::sha256;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::HashMap;
use std::path::Path;

fn dp(v: &V) -> f64 { v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
fn inc(o: &mut V, k: &str) { o[k] = json!(o[k].as_i64().unwrap_or(0) + 1); }

/// diagnoseQuotedDip(points, q, at, policy)
pub fn diagnose_quoted_dip(points: &[V], q: &V, at: &str, policy: Option<&V>) -> Result<V, String> {
    let policy = policy.cloned().unwrap_or_else(bid_anchor_policy);
    let now = parse_iso_ms(at).unwrap_or(f64::NAN);
    let history_ms = policy["historyMs"].as_f64().unwrap_or(f64::NAN);
    let a: Vec<&V> = points.iter()
        .filter(|p| dp(&p["at"]) < now && dp(&p["at"]) >= now - history_ms).collect();
    let mut result = json!({"reason":null,"samples":a.len(),"features":null});
    let fail = |reason: &str, samples: usize| -> V {
        json!({"reason":reason,"samples":samples,"features":null})
    };
    if (a.len() as f64) < policy["minimumMinutes"].as_f64().unwrap_or(f64::NAN) {
        return Ok(fail("history_minutes", a.len()));
    }
    if dp(&a.last().unwrap()["at"]) - dp(&a[0]["at"]) < policy["minimumSpanMs"].as_f64().unwrap_or(f64::NAN) {
        return Ok(fail("history_span", a.len()));
    }
    if now - dp(&a.last().unwrap()["at"]) > policy["maximumGapMs"].as_f64().unwrap_or(f64::NAN)
        || (1..a.len()).any(|i| dp(&a[i]["at"]) - dp(&a[i - 1]["at"]) > policy["maximumGapMs"].as_f64().unwrap_or(f64::NAN)) {
        return Ok(fail("history_gap", a.len()));
    }
    if q["crossed"].as_bool() == Some(true)
        || q["bestBid"].as_f64().is_none() || q["bestAsk"].as_f64().is_none()
        || q["bestBid"].as_f64().unwrap() <= 0.0 || q["bestAsk"].as_f64().unwrap() >= 1.0
        || q["bestBid"].as_f64().unwrap() >= q["bestAsk"].as_f64().unwrap() {
        return Ok(fail("invalid_quote", a.len()));
    }
    let mut values: Vec<BigInt> = a.iter().map(|p| big_amount(p["bid"].as_str().unwrap_or("")))
        .collect::<Result<_, _>>()?;
    values.sort();
    let anchor = values[(values.len() - 1) / 2].clone();
    let bid = big_amount(&price(q["bestBid"].as_f64().unwrap())?)?;
    let ask = big_amount(&price(q["bestAsk"].as_f64().unwrap())?)?;
    let range = values.last().unwrap() - &values[0];
    let spread = &ask - &bid;
    let bid_drop = &anchor - &bid;
    let concession = &anchor - BigInt::from(2 * TICK) - &ask;
    result["features"] = json!({
        "anchor": money(&anchor), "range": money(&range), "spread": money(&spread),
        "bidDrop": money(&bid_drop), "netConcession": money(&concession),
    });
    let mut failures: Vec<&str> = Vec::new();
    if range > big_amount(policy["maximumAnchorRange"].as_str().unwrap_or(""))? { failures.push("unstable_anchor"); }
    if spread > big_amount(policy["maximumSpread"].as_str().unwrap_or(""))? { failures.push("spread"); }
    if bid_drop > big_amount(policy["maximumBidDrop"].as_str().unwrap_or(""))? { failures.push("bid_collapse"); }
    if concession < big_amount(policy["minimumNetConcession"].as_str().unwrap_or(""))? { failures.push("insufficient_concession"); }
    result["reason"] = json!(failures.first().cloned().unwrap_or("depth_candidate"));
    result["failures"] = json!(failures);
    let candidate_ok = !quoted_dip_candidate(points, q, at, Some(&policy))?.is_null();
    if candidate_ok != (result["reason"] == "depth_candidate") {
        return Err("DIAGNOSTIC_GATE_DIVERGENCE".to_string());
    }
    Ok(result)
}

struct DiagSummary {
    quotes: i64,
    markets: std::collections::HashSet<String>,
    receipts: std::collections::HashSet<String>,
    observed_first_at: V,
    observed_last_at: V,
    primary_gates: V,
    all_ready_gates: V,
    price_changes: V,
    gaps: Vec<f64>,
    nearest: Vec<V>,
    candidates: Vec<V>,
}
impl DiagSummary {
    fn new() -> Self {
        Self {
            quotes: 0, markets: Default::default(), receipts: Default::default(),
            observed_first_at: V::Null, observed_last_at: V::Null,
            primary_gates: json!({}), all_ready_gates: json!({}),
            price_changes: json!({"unchanged":0,"askDown":0,"askUp":0,"askDownAtLeastTwoCents":0,"unobservedGapComparisons":0}),
            gaps: Vec::new(), nearest: Vec::new(), candidates: Vec::new(),
        }
    }
}
fn quantile(a: &[f64], p: f64) -> V {
    if a.is_empty() { return V::Null; }
    let mut v = a.to_vec();
    v.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    json!(v[((v.len() - 1) as f64 * p).floor() as usize])
}
fn finish(s: &DiagSummary) -> V {
    json!({
        "quotes": s.quotes, "markets": s.markets.len(),
        "observedFirstAt": s.observed_first_at, "observedLastAt": s.observed_last_at,
        "priceBatchReceipts": s.receipts.len(),
        "primaryGates": s.primary_gates, "allReadyGates": s.all_ready_gates,
        "priceChanges": s.price_changes,
        "quoteGapMs": {"samples": s.gaps.len(), "p50": quantile(&s.gaps, 0.5),
            "p90": quantile(&s.gaps, 0.9),
            "maximum": if s.gaps.is_empty() { V::Null } else { json!(s.gaps.iter().cloned().fold(f64::NEG_INFINITY, |a, b| if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) })) }},
        "nearestConcessions": s.nearest,
        "quoteOnlyCandidates": s.candidates,
    })
}

/// codeIdentity() — sha256 of the 3 live-paper sources; pass live-paper dir.
pub fn bulk_dip_diagnostic_code_identity(live_paper_dir: &Path) -> Result<V, String> {
    let mut out = Map::new();
    for p in ["./bulk-dip-diagnostic.mjs", "./bulk-dip.mjs", "./bid-anchor.mjs"] {
        let path = live_paper_dir.join(p.trim_start_matches("./"));
        out.insert(p.to_string(), json!(sha256(&std::fs::read(path).map_err(|e| e.to_string())?)));
    }
    Ok(V::Object(out))
}

/// diagnosePriceCapture(bytes, universe, {lookbackMs, cutoff})
pub fn diagnose_price_capture(bytes: &[u8], universe: &V, lookback_ms: Option<i64>, cutoff: Option<&str>, live_paper_dir: &Path) -> Result<V, String> {
    let lookback_ms = lookback_ms.unwrap_or(21600000);
    if lookback_ms < 600000
        || (cutoff.is_some() && parse_iso_ms(cutoff.unwrap()).is_none()) {
        return Err("DIAGNOSTIC_OPTIONS".to_string());
    }
    let canonical = validate_universe(universe, universe["tournamentId"].as_str().unwrap_or(""))?;
    let end = bytes.iter().rposition(|&b| b == b'\n').map(|i| i + 1).unwrap_or(0);
    let prefix = &bytes[..end];
    let text = std::str::from_utf8(prefix).map_err(|e| e.to_string())?;
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.pop();
    let records: Vec<V> = lines.iter()
        .map(|l| serde_json::from_str::<V>(l).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    if records.is_empty() { return Err("DIAGNOSTIC_EMPTY".to_string()); }
    let last_at = records.last().unwrap()["event"]["snapshot"]["receivedAt"].clone();
    let start = parse_iso_ms(last_at.as_str().unwrap_or("")).unwrap_or(f64::NAN) - lookback_ms as f64;
    let mut sequence = 0i64;
    let mut previous = V::String(String::new());
    let mut last_received = f64::NEG_INFINITY;
    let mut last_published = f64::NEG_INFINITY;
    let mut history: HashMap<String, Vec<V>> = HashMap::new();
    let mut latest: HashMap<String, V> = HashMap::new();
    let mut all = DiagSummary::new();
    let mut seg_before = DiagSummary::new();
    let mut seg_after = DiagSummary::new();
    let policy = bid_anchor_policy();
    let cutoff_ms = cutoff.map(|c| parse_iso_ms(c).unwrap_or(f64::NAN));
    for record in &records {
        let hash = record["hash"].clone();
        let mut body = record.clone();
        body.as_object_mut().map(|o| o.shift_remove("hash"));
        let snapshot = body["event"]["snapshot"].clone();
        sequence += 1;
        if body["sequence"].as_i64() != Some(sequence)
            || body["previous"] != previous
            || json!(digest(&body)) != hash
            || !body["state"].is_null()
            || body["event"]["type"].as_str() != Some("market_price_observation") {
            return Err("DIAGNOSTIC_CHAIN".to_string());
        }
        validate_price_observation_pub(&snapshot, &canonical)?;
        let now = dp(&snapshot["receivedAt"]);
        let published = dp(&body["at"]);
        if !published.is_finite() || published < now || published < last_published || now < last_received {
            return Err("DIAGNOSTIC_CLOCK".to_string());
        }
        previous = hash;
        last_received = now;
        last_published = published;
        for id in snapshot["missingIds"].as_array().cloned().unwrap_or_default() {
            if let Some(market) = canonical["exchanges"].as_array().and_then(|xs| xs.iter()
                .find(|x| x["exchangeId"] == id)).and_then(|x| x["marketId"].as_str()) {
                let market = market.to_string();
                history.remove(&market);
                latest.remove(&market);
            }
        }
        for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
            let qid = q["marketId"].as_str().unwrap_or("").to_string();
            let invalid = q["option"].as_str() != Some("YES")
                || q["crossed"].as_bool() == Some(true)
                || q.get("bestBid").map_or(false, |v| v.is_null())
                || q.get("bestAsk").map_or(false, |v| v.is_null())
                || q["bestBid"].as_f64().map_or(false, |v| v <= 0.0)
                || q["bestAsk"].as_f64().map_or(false, |v| v >= 1.0)
                || (q["bestBid"].as_f64().is_some() && q["bestAsk"].as_f64().is_some()
                    && q["bestBid"].as_f64().unwrap() >= q["bestAsk"].as_f64().unwrap());
            if invalid {
                history.remove(&qid);
                latest.remove(&qid);
                if now >= start { inc(&mut all.primary_gates, "invalid_reference"); }
                continue;
            }
            let points: Vec<V> = history.get(&qid).cloned().unwrap_or_default();
            let prior = latest.get(&qid).cloned();
            let gate = diagnose_quoted_dip(&points, &q, snapshot["receivedAt"].as_str().unwrap_or(""), None)?;
            if now >= start {
                let which = if let Some(c) = cutoff_ms {
                    if now < c { 1usize } else { 2usize }
                } else { 0usize };
                let mut targets: Vec<&mut DiagSummary> = vec![&mut all];
                match which {
                    1 => targets.push(&mut seg_before),
                    2 => targets.push(&mut seg_after),
                    _ => {}
                }
                for s in targets {
                    s.quotes += 1;
                    s.markets.insert(qid.clone());
                    s.receipts.insert(snapshot["receivedAt"].as_str().unwrap_or("").to_string());
                    if s.observed_first_at.is_null() { s.observed_first_at = snapshot["receivedAt"].clone(); }
                    s.observed_last_at = snapshot["receivedAt"].clone();
                    inc(&mut s.primary_gates, gate["reason"].as_str().unwrap_or(""));
                    for reason in gate["failures"].as_array().cloned().unwrap_or_default() {
                        inc(&mut s.all_ready_gates, reason.as_str().unwrap_or(""));
                    }
                    if let Some(pr) = &prior {
                        let gap = now - dp(&pr["at"]);
                        if dp(&pr["at"]) >= start { s.gaps.push(gap); }
                        if dp(&pr["at"]) < start || gap > policy["maximumGapMs"].as_f64().unwrap_or(0.0) {
                            inc(&mut s.price_changes, "unobservedGapComparisons");
                        } else {
                            let delta = big_amount(&price(q["bestAsk"].as_f64().unwrap_or(f64::NAN))?)?
                                - big_amount(&price(pr["ask"].as_f64().unwrap_or(f64::NAN))?)?;
                            let key = if delta == BigInt::from(0) { "unchanged" }
                                else if delta < BigInt::from(0) { "askDown" } else { "askUp" };
                            inc(&mut s.price_changes, key);
                            if delta <= BigInt::from(-20000) { inc(&mut s.price_changes, "askDownAtLeastTwoCents"); }
                        }
                    }
                    let feats = &gate["features"];
                    let no_failures = gate["failures"].as_array().map(|f| f.is_empty()).unwrap_or(false);
                    let only_concession = gate["failures"].as_array().map(|f| f.iter()
                        .all(|r| r == "insufficient_concession")).unwrap_or(false);
                    if !feats.is_null() && (no_failures || only_concession) {
                        let mut n = json!({"marketId":q["marketId"],"exchangeId":q["exchangeId"],
                            "receivedAt":snapshot["receivedAt"]});
                        for (k, v) in feats.as_object().unwrap() { n[k.clone()] = v.clone(); }
                        s.nearest.push(n);
                        s.nearest.sort_by(|a, b| {
                            let da = big_amount(a["netConcession"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0));
                            let db = big_amount(b["netConcession"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0));
                            db.cmp(&da).then_with(|| dp(&a["receivedAt"])
                                .partial_cmp(&dp(&b["receivedAt"])).unwrap_or(std::cmp::Ordering::Equal))
                        });
                        s.nearest.truncate(20);
                    }
                    if gate["reason"] == "depth_candidate" {
                        let mut c = json!({"marketId":q["marketId"],"exchangeId":q["exchangeId"],
                            "receivedAt":snapshot["receivedAt"]});
                        for (k, v) in feats.as_object().unwrap() { c[k.clone()] = v.clone(); }
                        s.candidates.push(c);
                    }
                }
            }
            let minute = (now / 60000.0).floor() as i64;
            let mut points = points;
            if !points.iter().any(|p| p["minute"].as_i64() == Some(minute)) {
                points.push(json!({"minute":minute,"at":snapshot["receivedAt"],
                    "bid":price(q["bestBid"].as_f64().unwrap_or(f64::NAN))?}));
            }
            let hist_ms = policy["historyMs"].as_f64().unwrap_or(0.0);
            history.insert(qid.clone(), points.into_iter()
                .filter(|x| dp(&x["at"]) >= now - hist_ms).collect());
            latest.insert(qid, json!({"at":snapshot["receivedAt"],"ask":q["bestAsk"]}));
        }
    }
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0);
    let segments = if cutoff.is_some() {
        let mut m = Map::new();
        m.insert("beforeCutoff".to_string(), finish(&seg_before));
        m.insert("afterCutoff".to_string(), finish(&seg_after));
        V::Object(m)
    } else { V::Null };
    Ok(json!({
        "mode": "quote_only_dip_diagnostic",
        "createdAt": ms_to_iso(now_ms),
        "codeIdentity": bulk_dip_diagnostic_code_identity(live_paper_dir)?,
        "window": {"start": ms_to_iso(start), "end": last_at, "lookbackMs": lookback_ms, "cutoff": cutoff},
        "source": {"bytes": prefix.len(), "sha256": sha256(prefix), "sequence": sequence,
            "hash": previous.clone(), "excludedPartialTailBytes": bytes.len() - prefix.len(),
            "universeHash": canonical["hash"]},
        "policy": policy,
        "all": finish(&all),
        "segments": segments,
        "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false,
        "limitations": [
            "Quotes have unknown source age and no depth. Candidates are historical quote-only diagnostics, never fills or P&L.",
            "This price-only replay does not apply book invalidations or actual consumer receipt gates and can overstate live eligibility.",
            "Each repeated quote is correlated; gate counts and nearest concessions are not independent trades or strategy selection evidence."
        ],
    }))
}
