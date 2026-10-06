// live-paper/portfolio-bank.mjs — sequential portfolio selection over the frozen
// journal source. Fit window -> candidate bank, validation window -> per-edge
// scorecards, holdout reported without reselection.
use serde_json::{json, Value as V};
use std::collections::HashSet;

use baal_paper::fixed::{big_amount as amount, money};
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::{digest, js_int, js_truthy};
use indexmap::IndexMap;
use num_bigint::BigInt;

use crate::candidate_plan::{validate_candidate_plan, PORTFOLIO_PLAN};
use crate::depth_scope::scoped_snapshots;
use crate::execution_metrics::PAPER_EDGE_MARGINS;
use crate::history_policy::ISOLATED;
use crate::ml_bank::train_bank;
use crate::ml_shadow_v2::{advance_shadow, verify_source};
use crate::model::{initial_state, invalidate, report, validate_config};
use crate::native_dip_host_v1::jstr;
use crate::roundtrip_target::ROUND_TRIP;
use crate::snapshot_order::LEGACY_SNAPSHOT_ORDER;
use crate::training_source::{training_source_prefix, VerifiedSource};
use crate::training_horizons::{parse_training_horizons, DEFAULT_HORIZONS};

/// SELECTION_POLICY frozen object.
pub fn selection_policy() -> V {
    json!({"version": PORTFOLIO_PLAN, "fitFraction": 0.6, "selectionFraction": 0.2,
        "validationBlocks": 3, "minimumCompletedTrades": 20, "minimumPositiveBlocks": 2})
}

fn clock(r: &V) -> V {
    let c = &r["event"]["clockAt"];
    if js_truthy(Some(c)) { c.clone() } else { r["at"].clone() }
}
// JS Date.parse(unknown): only string values can parse.
fn date_parse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}
fn clock_ms(r: &V) -> f64 { date_parse(&clock(r)) }

/// Input to source(): raw journal text or an already verified in-process source.
pub enum SourceInput<'a> {
    Text(&'a str),
    Verified(&'a VerifiedSource),
}
impl<'a> From<&'a str> for SourceInput<'a> {
    fn from(s: &'a str) -> Self { SourceInput::Text(s) }
}
impl<'a> From<&'a VerifiedSource> for SourceInput<'a> {
    fn from(v: &'a VerifiedSource) -> Self { SourceInput::Verified(v) }
}

/// source() result — the object the JS verifiedPortfolioSources WeakSet marks.
#[derive(Clone)]
pub struct PortfolioSource {
    pub records: Vec<V>,
    pub prefix_hashes: Vec<String>,
    pub lines: Option<Vec<String>>,
    pub verified: Option<VerifiedSource>,
}
/// isVerifiedPortfolioSource — a typed PortfolioSource can only come from
/// source(), so the WeakSet membership check is always true here.
pub fn is_verified_portfolio_source(_s: &PortfolioSource) -> bool { true }

fn source(input: SourceInput, config: &V) -> Result<PortfolioSource, String> {
    let (records, prefix_hashes, lines, verified) = match input {
        SourceInput::Verified(v) => (v.records.clone(), v.prefix_hashes.clone(), None, Some(v.clone())),
        SourceInput::Text(raw) => {
            let vs = verify_source(raw)?;
            let vs = match vs { Some(v) => v, None => return Err("Complete immutable capture required".into()) };
            // raw.slice(0, -1).split('\n') — drops exactly one trailing character.
            let body = raw.strip_suffix('\n')
                .map(str::to_string)
                .unwrap_or_else(|| {
                    let cut = raw.chars().last().map(|c| raw.len() - c.len_utf8()).unwrap_or(0);
                    raw[..cut].to_string()
                });
            (vs.records, vs.prefix_hashes, Some(body.split('\n').map(str::to_string).collect()), None)
        }
    };
    // Reject future-dated observations concealed inside an earlier journal event.
    for r in &records {
        if r["event"]["type"].as_str() == Some("cycle") {
            if !r["event"]["accountExclusions"].is_array() {
                return Err("Missing account exclusions".into());
            }
            for snap in scoped_snapshots(&r["event"], config)? {
                let skipped = js_truthy(snap.get("skipped"));
                let at = if skipped {
                    let a = &snap["at"];
                    if js_truthy(Some(a)) { a.clone() } else { clock(r) }
                } else {
                    snap["receivedAt"].clone()
                };
                let at_ms = date_parse(&at);
                if !at_ms.is_finite() || at_ms > clock_ms(r) {
                    return Err("Observation exceeds event clock".into());
                }
                if !skipped {
                    let req_ms = date_parse(&snap["requestStartedAt"]);
                    if !req_ms.is_finite() || req_ms > at_ms {
                        return Err("Invalid request chronology".into());
                    }
                }
            }
        }
    }
    Ok(PortfolioSource { records, prefix_hashes, lines, verified })
}

/// The raw replay input produced by prefix(..., true): text lines or a verified
/// prefix — what JS hands to trainBank / readTrainingSource consumers.
pub enum PrefixRaw {
    Text(String),
    Verified(VerifiedSource),
}

pub struct SourcePrefix {
    pub records: Vec<V>,
    pub prefix_hashes: Vec<String>,
    pub raw: Option<PrefixRaw>,
    /// V::Null when n is outside the captured range (JS undefined).
    pub source_hash: V,
    pub source_records: usize,
    pub training_capture_through: V,
}

fn prefix(s: &PortfolioSource, n: usize, include_raw: bool) -> Result<SourcePrefix, String> {
    let records: Vec<V> = s.records.iter().take(n).cloned().collect();
    let prefix_hashes: Vec<String> = s.prefix_hashes.iter().take(n).cloned().collect();
    let raw = if include_raw {
        Some(match &s.verified {
            Some(v) => PrefixRaw::Verified(training_source_prefix(v, n)?),
            None => PrefixRaw::Text(
                s.lines.as_ref().map(|l| l.iter().take(n).cloned().collect::<Vec<_>>().join("\n") + "\n")
                    .unwrap_or_default()),
        })
    } else { None };
    // clock(records.at(-1)): empty prefix is a TypeError in JS.
    let last = match records.last() {
        Some(r) => r.clone(),
        None => return Err("Cannot read properties of undefined (reading 'event')".into()),
    };
    Ok(SourcePrefix {
        records,
        prefix_hashes,
        raw,
        source_hash: s.prefix_hashes.get(n.wrapping_sub(1)).cloned().map(V::String).unwrap_or(V::Null),
        source_records: n,
        training_capture_through: clock(&last),
    })
}

fn cut_after(records: &[V], at: &V) -> usize {
    let at_ms = date_parse(at);
    records.iter().position(|r| clock_ms(r) > at_ms).unwrap_or(records.len())
}

fn policy_options(options: &V) -> Result<V, String> {
    let mut p = selection_policy();
    if let Some(obj) = options.as_object() {
        for (k, v) in obj { p[k.clone()] = v.clone(); }
    }
    let known = ["version", "fitFraction", "selectionFraction", "validationBlocks",
        "minimumCompletedTrades", "minimumPositiveBlocks"];
    let finite = |k: &str| p[k].as_f64().map(|x| x.is_finite()).unwrap_or(false);
    let f64_of = |k: &str| p[k].as_f64().unwrap_or(f64::NAN);
    if p.as_object().map(|o| o.keys().any(|k| !known.contains(&k.as_str()))).unwrap_or(true)
        || p["version"].as_str() != Some(PORTFOLIO_PLAN)
        || !finite("fitFraction") || f64_of("fitFraction") <= 0.0 || f64_of("fitFraction") >= 1.0
        || !finite("selectionFraction") || f64_of("selectionFraction") <= 0.0
        || f64_of("fitFraction") + f64_of("selectionFraction") >= 1.0
        || js_int(&p["validationBlocks"]).map_or(true, |v| !(2..=10).contains(&v))
        || js_int(&p["minimumCompletedTrades"]).map_or(true, |v| v < 1)
        || js_int(&p["minimumPositiveBlocks"]).map_or(true, |v| v < 1)
        || js_int(&p["minimumPositiveBlocks"]).unwrap_or(0) > js_int(&p["validationBlocks"]).unwrap_or(0)
    {
        return Err("Invalid selection policy".into());
    }
    Ok(p)
}

/// chronologicalPartitions(records, options).
pub fn chronological_partitions(records: &[V], options: &V) -> Result<V, String> {
    let p = policy_options(options)?;
    let cycles: Vec<&V> = records.iter().filter(|r| r["event"]["type"].as_str() == Some("cycle")).collect();
    if cycles.len() < 20 { return Err("Insufficient cycles for three chronological windows".into()); }
    let fit_fraction = p["fitFraction"].as_f64().unwrap_or(0.0);
    let selection_fraction = p["selectionFraction"].as_f64().unwrap_or(0.0);
    let fit_i = (cycles.len() as f64 * fit_fraction).floor() as i64 - 1;
    let sel_i = (cycles.len() as f64 * (fit_fraction + selection_fraction)).floor() as i64 - 1;
    // cycles[-1] in JS is undefined and clock() throws TypeError.
    if fit_i < 0 || sel_i < 0 {
        return Err("Cannot read properties of undefined (reading 'event')".into());
    }
    let fit_at = clock(cycles[fit_i as usize]);
    let selected_at = clock(cycles[sel_i as usize]);
    let fit_records = cut_after(records, &fit_at);
    let selection_records = cut_after(records, &selected_at);
    if !(fit_records > 0 && fit_records < selection_records && selection_records < records.len()) {
        return Err("Chronological windows overlap or are empty".into());
    }
    Ok(json!({"fitAt": fit_at, "selectedAt": selected_at, "fitRecords": fit_records,
        "selectionRecords": selection_records,
        "finalAt": records.last().map(|r| clock(r)).unwrap_or(V::Null)}))
}

// Missing or stale liquidation depth cannot make an unfinished trade disappear.
// A cash-only floor charges the full remaining basis when its exit is unknown.
/// portfolioPoint(wallet, config, at). `at` is the raw clock value (JS checks
/// typeof string + Date.parse).
pub fn portfolio_point(wallet: &V, config: &V, at: &V) -> Result<V, String> {
    let at_s = match at.as_str().and_then(parse_iso_ms) {
        Some(ms) if ms.is_finite() => at.as_str().unwrap().to_string(),
        _ => return Err("Invalid portfolio valuation clock".into()),
    };
    let at_ms = date_parse(at);
    let mut w = wallet.clone();
    let books: Vec<V> = w["books"].as_object().map(|o| o.values().cloned().collect()).unwrap_or_default();
    for b in &books {
        let age = at_ms - date_parse(&b["receivedAt"]);
        let max_age = config["maxDataAgeMs"].as_f64().unwrap_or(f64::NAN);
        if !age.is_finite() || age < 0.0 || age > max_age {
            let mid: Option<String> = if b["marketId"].is_null() {
                None
            } else {
                Some(jstr(&b["marketId"]))
            };
            invalidate(&mut w, mid.as_deref(), &at_s, "selection_stale_book");
        }
    }
    let rep = report(&w, config, &at_s);
    let r = rep["strategies"]["mean_reversion"].clone();
    let equity = if r["estimatedLiquidationEquity"].is_null() {
        r["cash"].clone()
    } else {
        r["estimatedLiquidationEquity"].clone()
    };
    let conservative = amount(&jstr(&equity))? - amount(&jstr(&config["initialCash"]))?;
    let positions = r["positions"].as_array().cloned().unwrap_or_default();
    let open_shares: i64 = positions.iter().map(|x| js_int(&x["quantity"]).unwrap_or(0)).sum();
    let unsold_shares: i64 = positions.iter().map(|x| js_int(&x["unsoldShares"]).unwrap_or(0)).sum();
    Ok(json!({
        "at": at,
        "conservativePnl": money(&conservative),
        "liquidationPnl": r["liquidationPnl"],
        "valuation": if r["liquidationPnl"].is_null() { "cash_only_inventory_zero" } else { "depth_checked_liquidation_estimate" },
        "cash": r["cash"],
        "realizedPnl": r["realizedPnl"],
        "fees": r["fees"],
        "turnover": r["turnover"],
        "completedTrades": r["closedTrades"],
        "fillExecutions": r["fills"],
        "intents": r["intents"],
        "expiredIntents": r["expiredIntents"],
        "pendingIntents": r["pending"].as_array().map(|a| a.len()).unwrap_or(0),
        "openShares": open_shares,
        "unsoldShares": unsold_shares,
    }))
}

fn margins_v(margin: f64) -> V { json!(margin) }

/// replayCandidates(s, config, modelBank, startCount, endCounts, onProgress).
/// Returns the paths map in JS insertion order.
pub(crate) fn replay_candidates(
    s: &PortfolioSource,
    config: &V,
    model_bank: &V,
    start_count: usize,
    end_counts: &[usize],
    on_progress: &(dyn Fn(&V) + Send + Sync),
) -> Result<IndexMap<String, Vec<V>>, String> {
    let start = prefix(s, start_count, false)?;
    let mut bank = model_bank.clone();
    bank["config"] = config.clone();
    bank["configHash"] = json!(digest(config));
    bank["sourceHash"] = start.source_hash.clone();
    bank["sourceRecords"] = json!(start.source_records);
    bank["trainingCaptureThrough"] = start.training_capture_through.clone();
    bank["requireInstrumentCoverage"] = json!(true);
    bank["trainingScope"] = json!("all-markets-v1");
    bank["forecastLifecycle"] = json!("revoke-on-invalidation-v2");
    validate_candidate_plan(&bank)?;
    let mut state = json!({"bankHash": digest(&bank), "sourceSequence": 0, "sourceHash": V::Null,
        "wallets": {}, "histories": {}, "predictions": {}});
    let mut points: IndexMap<String, Vec<V>> = IndexMap::new();
    for &end in end_counts {
        on_progress(&json!({"phase": "portfolio_replay", "throughRecords": end,
            "selectedModels": bank["selected"].as_array().map(|a| a.len()).unwrap_or(0)}));
        let view = prefix(s, end, false)?;
        state = advance_shadow(&state, &crate::ml_shadow::VerifiedSource {
            records: view.records.clone(), prefix_hashes: view.prefix_hashes.clone(),
        }, &bank, false)?;
        let models = bank["selected"].as_array().cloned().unwrap_or_default();
        for model in &models {
            // bank.candidatePlan?.candidates.filter(...) ?? PAPER_EDGE_MARGINS
            let margins: Vec<V> = match bank.get("candidatePlan") {
                None | Some(V::Null) => PAPER_EDGE_MARGINS.iter().map(|&m| margins_v(m)).collect(),
                Some(cp) => {
                    let candidates = cp.get("candidates").and_then(V::as_array)
                        .ok_or("Cannot read properties of undefined (reading 'filter')")?;
                    candidates.iter()
                        .filter(|x| x["modelId"] == model["id"])
                        .map(|x| x["margin"].clone())
                        .collect()
                }
            };
            for margin in &margins {
                let id = format!("{}-edge{}", jstr(&model["id"]), jstr(margin));
                let w = state["wallets"].get(&id).cloned()
                    .map(Ok)
                    .unwrap_or_else(|| initial_state(config, &id, &jstr(&start.training_capture_through)))?;
                let mut list = points.get(&id).cloned().unwrap_or_default();
                let mut point = portfolio_point(&w, config, &view.training_capture_through)?;
                point["signalMetrics"] = state["signalMetrics"].get(&id).cloned().unwrap_or(V::Null);
                point["unsupportedInstruments"] = json!(state["unsupportedInstruments"].as_object()
                    .map(|o| o.keys().filter(|k| k.starts_with(&format!("{}:", id))).count())
                    .unwrap_or(0));
                list.push(point);
                points.insert(id, list);
            }
        }
    }
    Ok(points)
}

fn block_ends(s: &PortfolioSource, start_count: usize, end_count: usize, blocks: i64) -> Result<Vec<usize>, String> {
    let hi = end_count.min(s.records.len());
    let lo = start_count.min(hi);
    let cycles: Vec<&V> = s.records[lo..hi].iter()
        .filter(|r| r["event"]["type"].as_str() == Some("cycle")).collect();
    let blocks_us = blocks as usize;
    if cycles.len() < blocks_us * 2 { return Err("Insufficient validation block coverage".into()); }
    let ends: Vec<usize> = (0..blocks_us).map(|i| {
        if i == blocks_us - 1 {
            end_count
        } else {
            let at = clock(cycles[cycles.len() * (i + 1) / blocks_us - 1]);
            end_count.min(cut_after(&s.records, &at))
        }
    }).collect();
    let unique: HashSet<usize> = ends.iter().cloned().collect();
    if unique.len() != blocks_us || ends.first().copied().unwrap_or(0) <= start_count {
        return Err("Validation block clocks overlap".into());
    }
    Ok(ends)
}

/// rankPortfolios(scorecards, options).
pub fn rank_portfolios(scorecards: &V, options: &V) -> Result<V, String> {
    let policy = policy_options(options)?;
    let cards = scorecards.as_array().ok_or("Invalid portfolio scorecards")?;
    // new Set(ids).size !== length — SameValueZero dedupe on the id values.
    let mut seen: HashSet<String> = HashSet::new();
    for s in cards {
        if !seen.insert(baal_service_runtime::json_text::js_stringify(&s["id"])) {
            return Err("Invalid portfolio scorecards".into());
        }
    }
    let validation_blocks = js_int(&policy["validationBlocks"]).unwrap_or(0) as usize;
    let mut results: Vec<V> = Vec::new();
    for s in cards {
        if !s["id"].as_str().map(|x| !x.is_empty()).unwrap_or(false) {
            return Err("Invalid portfolio identifier".into());
        }
        let pts = s["points"].as_array().cloned().unwrap_or_default();
        if !s["points"].is_array() || pts.len() != validation_blocks {
            return Err("Missing sequential score checkpoints".into());
        }
        for p in &pts {
            if js_int(&p["completedTrades"]).map_or(true, |v| v < 0) {
                return Err("Invalid completed trade count".into());
            }
            amount(&jstr(&p["conservativePnl"]))?;
            if !p["liquidationPnl"].is_null()
                && amount(&jstr(&p["liquidationPnl"]))? != amount(&jstr(&p["conservativePnl"]))? {
                return Err("Inconsistent liquidation checkpoint".into());
            }
        }
        let mut previous = BigInt::from(0);
        let block_pnl: Vec<V> = pts.iter().map(|p| {
            let value = amount(&jstr(&p["conservativePnl"])).unwrap_or_else(|_| BigInt::from(0));
            let change = &value - &previous;
            previous = value;
            json!(money(&change))
        }).collect();
        let last = pts.last().cloned().unwrap_or(V::Null);
        let mut reasons: Vec<V> = Vec::new();
        if js_int(&last["completedTrades"]).unwrap_or(0) < js_int(&policy["minimumCompletedTrades"]).unwrap_or(0) {
            reasons.push(json!("insufficient_completed_trades"));
        }
        if amount(&jstr(&last["conservativePnl"]))? <= BigInt::from(0) {
            reasons.push(json!("does_not_beat_no_trade"));
        }
        if block_pnl.iter().any(|v| amount(&jstr(v)).map(|x| x < BigInt::from(0)).unwrap_or(false)) {
            reasons.push(json!("negative_validation_block"));
        }
        let positives = block_pnl.iter().filter(|v| amount(&jstr(v)).map(|x| x > BigInt::from(0)).unwrap_or(false)).count() as i64;
        if positives < js_int(&policy["minimumPositiveBlocks"]).unwrap_or(0) {
            reasons.push(json!("insufficient_positive_blocks"));
        }
        if last["liquidationPnl"].is_null() {
            reasons.push(json!("unresolved_terminal_liquidation"));
        }
        let mut card = s.clone();
        card["blockPnl"] = json!(block_pnl);
        card["eligible"] = json!(reasons.is_empty());
        card["rejectionReasons"] = json!(reasons);
        results.push(card);
    }
    results.sort_by(|a, b| {
        let ae = a["eligible"].as_bool().unwrap_or(false);
        let be = b["eligible"].as_bool().unwrap_or(false);
        if ae != be { return if ae { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater }; }
        let last_pnl = |v: &V| -> BigInt {
            let pts = v["points"].as_array();
            let last = pts.and_then(|p| p.last()).cloned().unwrap_or(V::Null);
            amount(&jstr(&last["conservativePnl"])).unwrap_or_else(|_| BigInt::from(0))
        };
        let x = last_pnl(a);
        let y = last_pnl(b);
        match x.cmp(&y) {
            std::cmp::Ordering::Equal => a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or("")),
            ord => ord.reverse(),
        }
    });
    let winner = results.iter().find(|x| x["eligible"].as_bool() == Some(true)).cloned().unwrap_or(V::Null);
    Ok(json!({"winner": winner, "results": results}))
}

/// The job object the generator yields and drivers pass to replayCandidates /
/// parallelPortfolioReplay.
pub struct ReplayJob<'a> {
    pub source: &'a PortfolioSource,
    pub config: V,
    pub model_bank: V,
    pub start_count: usize,
    pub end_counts: Vec<usize>,
}
pub type ProgressFn = dyn Fn(&V) + Send + Sync;
pub type ReplayFn<'a> = dyn FnMut(&ReplayJob, &ProgressFn) -> Result<IndexMap<String, Vec<V>>, String> + 'a;

/// The options object of portfolioTrainingSteps: {policy, family, breadthContext, horizons}.
fn flow_options(options: &V) -> (V, String, bool, Vec<i64>) {
    let policy = options.get("policy").cloned().unwrap_or(json!({}));
    let family = options.get("family").and_then(V::as_str).unwrap_or("ridge").to_string();
    let breadth = js_truthy(options.get("breadthContext"));
    let horizons = options.get("horizons").and_then(V::as_array)
        .map(|a| a.iter().map(|v| js_int(v).unwrap_or(-1)).collect())
        .unwrap_or_else(|| DEFAULT_HORIZONS.to_vec());
    (policy, family, breadth, horizons)
}

/// portfolioTrainingSteps — the generator unrolled as a function driving a
/// replay callback (sequential: replayCandidates; parallel: parallelPortfolioReplay).
fn portfolio_training_flow<'a>(
    input: SourceInput<'a>,
    config: &V,
    options: &V,
    on_progress: &ProgressFn,
    mut replay: impl FnMut(&ReplayJob, &ProgressFn) -> Result<IndexMap<String, Vec<V>>, String>,
) -> Result<V, String> {
    validate_config(config)?;
    let (policy_opts, family, breadth_context, horizons) = flow_options(options);
    let policy = policy_options(&policy_opts)?;
    let s = source(input, config)?;
    let cuts = chronological_partitions(&s.records, &policy)?;
    let fit_records = js_int(&cuts["fitRecords"]).unwrap_or(0) as usize;
    let selection_records = js_int(&cuts["selectionRecords"]).unwrap_or(0) as usize;
    let development = prefix(&s, selection_records, true)?;
    // Fit on labels matured before fitAt. Every later observation, including the
    // entire holdout, is excluded from fitting. Selection compares ALL variants.
    let (raw_text, verified) = match &development.raw {
        Some(PrefixRaw::Text(t)) => (Some(t.as_str()), None),
        Some(PrefixRaw::Verified(v)) => (None, Some(v)),
        None => (None, None),
    };
    let fitted = train_bank(raw_text, verified, config, &family, ISOLATED, None, false,
        breadth_context, ROUND_TRIP, "all-markets-v1", cuts["fitAt"].as_str(),
        LEGACY_SNAPSHOT_ORDER, &horizons)?;
    let mut validation_bank = fitted.clone();
    validation_bank["selected"] = fitted["models"].clone();
    let checkpoints = block_ends(&s, fit_records, selection_records,
        js_int(&policy["validationBlocks"]).unwrap_or(0))?;
    let job = ReplayJob { source: &s, config: config.clone(), model_bank: validation_bank,
        start_count: fit_records, end_counts: checkpoints };
    let paths = replay(&job, on_progress)?;
    let models = fitted["models"].as_array().cloned().unwrap_or_default();
    let mut cards: Vec<V> = Vec::new();
    for model in &models {
        for &margin in PAPER_EDGE_MARGINS.iter() {
            let id = format!("{}-edge{}", jstr(&model["id"]), jstr(&margins_v(margin)));
            let points = paths.get(&id).cloned().map(V::Array).unwrap_or(V::Null);
            cards.push(json!({"id": id, "modelId": model["id"], "margin": margins_v(margin),
                "rmse": model["rmse"], "points": points}));
        }
    }
    let ranked = rank_portfolios(&json!(cards), &policy)?;
    let winner = ranked["winner"].clone();
    let has_winner = !winner.is_null();
    let selected: Vec<V> = if has_winner {
        models.iter().filter(|m| m["id"] == winner["modelId"]).cloned().collect()
    } else { Vec::new() };
    let selected_first = selected.first().cloned().unwrap_or(V::Null);
    let candidates: Vec<V> = if has_winner {
        vec![json!({"modelId": winner["modelId"], "margin": winner["margin"]})]
    } else { Vec::new() };
    let candidate_plan = json!({"version": PORTFOLIO_PLAN, "candidates": candidates});
    // Nothing below this line can choose another candidate or refit its weights.
    let selection_hash = digest(&json!({"policy": policy, "candidatePlan": candidate_plan,
        "selected": selected}));
    let mut holdout: V;
    if has_winner {
        let mut holdout_bank = fitted.clone();
        holdout_bank["selected"] = json!(vec![selected_first.clone()]);
        holdout_bank["candidatePlan"] = candidate_plan.clone();
        let job = ReplayJob { source: &s, config: config.clone(), model_bank: holdout_bank,
            start_count: selection_records, end_counts: vec![s.records.len()] };
        let paths = replay(&job, on_progress)?;
        let wid = jstr(&winner["id"]);
        let result = paths.get(&wid).and_then(|v| v.first()).cloned().unwrap_or(V::Null);
        let conservative = amount(&jstr(&result["conservativePnl"])).unwrap_or_else(|_| BigInt::from(0));
        let assessment = if result["liquidationPnl"].is_null() {
            "unresolved_terminal_inventory"
        } else if conservative <= BigInt::from(0) {
            "did_not_beat_no_trade"
        } else if js_int(&result["completedTrades"]).unwrap_or(0) < js_int(&policy["minimumCompletedTrades"]).unwrap_or(0) {
            "positive_but_insufficient_completed_trades"
        } else {
            "positive_simulation_requires_prospective_validation"
        };
        let mut h = result;
        h["status"] = json!("reported_without_reselection");
        h["assessment"] = json!(assessment);
        h["selectedCandidateId"] = winner["id"].clone();
        holdout = h;
    } else {
        holdout = json!({"status": "no_trade_selected", "selectedCandidateId": V::Null,
            "assessment": "no_trade_selected", "conservativePnl": "0.000000",
            "realizedPnl": "0.000000", "completedTrades": 0, "openShares": 0,
            "pendingIntents": 0, "at": cuts["finalAt"]});
    }
    let full = prefix(&s, s.records.len(), false)?;
    let fit_source_hash = prefix(&s, fit_records, false)?.source_hash;
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0);
    let mut limitations = fitted["limitations"].as_array().cloned().unwrap_or_default();
    for l in [
        "Single chronological train/validation/holdout split, not multiple independent walk-forward folds.",
        "Validation searches up to 384 correlated model/margin pairs; passing heuristic gates is not statistical significance.",
        "Validation checkpoints use one continuing wallet. They are not separate independent trades, nor a full drawdown trace.",
        "Holdout starts flat with fresh warmup. Remaining positions stay charged; missing liquidation uses a cash-only floor.",
        "Snapshot-based hypothetical fills and liquidation are not verified exchange executions or settlement payouts.",
        "Only configured market depth is evaluated; no new network collection, capital scaling, or live execution.",
        "Weights are not refitted on validation or holdout. Prospective processing starts after the complete frozen capture.",
    ] { limitations.push(json!(l)); }
    let mut bank = fitted.clone();
    bank["createdAt"] = json!(baal_service_runtime::contract::ms_to_iso(now_ms));
    bank["selected"] = json!(selected);
    bank["candidatePlan"] = candidate_plan;
    bank["sourceHash"] = full.source_hash;
    bank["sourceRecords"] = json!(full.source_records);
    bank["trainingCaptureThrough"] = full.training_capture_through;
    bank["forecastLifecycle"] = json!("revoke-on-invalidation-v2");
    bank["requireInstrumentCoverage"] = json!(true);
    bank["selection"] = json!("Validation-only sequential portfolio selection; the no-trade baseline may win. Holdout is never used for reselection.");
    bank["portfolioSelection"] = json!({
        "version": PORTFOLIO_PLAN, "policy": policy, "windows": cuts,
        "fitSourceHash": fit_source_hash,
        "developmentSourceHash": development.source_hash,
        "selectionHash": selection_hash,
        "decision": if has_winner { "candidate_for_prospective_paper" } else { "no_trade" },
        "testedModelMarginPairs": cards.len(),
        "validation": ranked["results"],
        "holdout": holdout,
        "noTradeBaseline": {"pnl": "0.000000"},
        "productionActivation": false, "scalingEvaluated": false,
    });
    bank["limitations"] = json!(limitations);
    validate_candidate_plan(&bank)?;
    Ok(bank)
}

/// trainPortfolioBank(raw, config, options).
pub fn train_portfolio_bank(raw: Option<&str>, verified: Option<&VerifiedSource>, config: &V,
    options: &V, on_progress: &ProgressFn) -> Result<V, String> {
    let input = match (raw, verified) {
        (Some(t), _) => SourceInput::Text(t),
        (None, Some(v)) => SourceInput::Verified(v),
        (None, None) => return Err("Complete immutable capture required".into()),
    };
    portfolio_training_flow(input, config, options, on_progress,
        |job, progress| replay_candidates(job.source, &job.config, &job.model_bank,
            job.start_count, &job.end_counts, progress))
}

/// trainPortfolioBankParallel(raw, config, {workers, ...options}).
pub fn train_portfolio_bank_parallel(raw: Option<&str>, verified: Option<&VerifiedSource>, config: &V,
    options: &V, on_progress: &ProgressFn) -> Result<V, String> {
    let workers = options.get("workers").map(|w| js_int(w).unwrap_or(-1)).unwrap_or(4);
    if workers < 1 || workers > 8 { return Err("Invalid offline replay worker count".into()); }
    let input = match (raw, verified) {
        (Some(t), _) => SourceInput::Text(t),
        (None, Some(v)) => SourceInput::Verified(v),
        (None, None) => return Err("Complete immutable capture required".into()),
    };
    portfolio_training_flow(input, config, options, on_progress,
        |job, progress| crate::portfolio_replay_pool::parallel_portfolio_replay(job, workers, progress))
}

// Re-run existing frozen weights without fitting or changing selection. Used by
// the native backend parity benchmark; the original byte prefix remains bound.
/// replayFrozenPortfolioBank(raw, bank, {workers}).
pub fn replay_frozen_portfolio_bank(raw: Option<&str>, verified: Option<&VerifiedSource>, bank: &V,
    workers: i64, on_progress: &ProgressFn) -> Result<V, String> {
    validate_config(&bank["config"])?;
    let input = match (raw, verified) {
        (Some(t), _) => SourceInput::Text(t),
        (None, Some(v)) => SourceInput::Verified(v),
        (None, None) => return Err("Complete immutable capture required".into()),
    };
    let s = source(input, &bank["config"])?;
    let windows = &bank["portfolioSelection"]["windows"];
    let last_clock = s.records.last().map(|r| clock(r)).unwrap_or(V::Null);
    if windows.is_null()
        || js_int(&bank["sourceRecords"]).map_or(true, |v| v as usize != s.records.len())
        || bank["sourceHash"].as_str() != s.prefix_hashes.last().map(String::as_str)
        || bank["trainingCaptureThrough"] != last_clock {
        return Err("Frozen source mismatch".into());
    }
    let start = js_int(&windows["fitRecords"]).unwrap_or(-1);
    let end = js_int(&windows["selectionRecords"]).unwrap_or(-1);
    if start < 1 || start >= end || end < 1 || end as usize > s.records.len()
        || bank["portfolioSelection"]["fitSourceHash"].as_str()
            != s.prefix_hashes.get((start - 1) as usize).map(String::as_str)
        || bank["portfolioSelection"]["developmentSourceHash"].as_str()
            != s.prefix_hashes.get((end - 1) as usize).map(String::as_str) {
        return Err("Frozen replay windows mismatch".into());
    }
    if js_int(&bank["portfolioSelection"]["policy"]["validationBlocks"]) != Some(3) {
        return Err("Unsupported native comparison checkpoints".into());
    }
    let checks = block_ends(&s, start as usize, end as usize, 3)?;
    let mut model_bank = bank.clone();
    model_bank["selected"] = bank["models"].clone();
    if let Some(o) = model_bank.as_object_mut() { o.remove("candidatePlan"); }
    let job = ReplayJob { source: &s, config: bank["config"].clone(), model_bank,
        start_count: start as usize, end_counts: checks };
    let paths = crate::portfolio_replay_pool::parallel_portfolio_replay(&job, workers, on_progress)?;
    let mut out: Vec<V> = Vec::new();
    for model in bank["models"].as_array().cloned().unwrap_or_default().iter() {
        for &margin in PAPER_EDGE_MARGINS.iter() {
            let id = format!("{}-edge{}", jstr(&model["id"]), jstr(&margins_v(margin)));
            let points = paths.get(&id).cloned().map(V::Array).unwrap_or(V::Null);
            out.push(json!({"id": id, "modelId": model["id"], "margin": margins_v(margin),
                "rmse": model["rmse"], "points": points}));
        }
    }
    out.sort_by(|a, b| jstr(&a["id"]).cmp(&jstr(&b["id"])));
    Ok(json!(out))
}

// This receives only an already verified in-process source from the parent.
/// replayPortfolioWorkerJob({records, prefixHashes, config, modelBank, startCount, endCounts}, onProgress).
pub fn replay_portfolio_worker_job(payload: &V,
    on_progress: &(dyn Fn(&V) + Send + Sync))
    -> Result<IndexMap<String, Vec<V>>, String> {
    let records = payload["records"].as_array().cloned().unwrap_or_default();
    let prefix_hashes: Vec<String> = payload["prefixHashes"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let s = PortfolioSource { records, prefix_hashes, lines: None, verified: None };
    let end_counts: Vec<usize> = payload["endCounts"].as_array()
        .map(|a| a.iter().filter_map(|v| js_int(v).map(|x| x as usize)).collect())
        .unwrap_or_default();
    replay_candidates(&s, &payload["config"], &payload["modelBank"],
        js_int(&payload["startCount"]).unwrap_or(0) as usize, &end_counts, on_progress)
}

/// CLI helper: parse --horizons via the shared horizons parser.
pub fn cli_training_horizons(text: Option<&str>) -> Result<Vec<i64>, String> {
    match text { Some(t) => parse_training_horizons(t), None => Ok(DEFAULT_HORIZONS.to_vec()) }
}


