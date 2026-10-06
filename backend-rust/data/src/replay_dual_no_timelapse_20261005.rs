// data/replay-dual-no-timelapse-20261005.mjs — sealed-prefix dual-NO replay with
// causal account eligibility, publication reconstruction and optional historical
// development transport. The module selects the v1 or v2 frozen paper engine by
// bank.mode; JS dynamic import becomes the verified-archive check plus a
// statically-dispatched function set.
use baal_collector::book_publication_v2::{apply_book_publication, create_book_publication_state};
use baal_collector::frozen_paper_runtime::frozen_paper_module;
use baal_collector::read_timing::timing_distribution;
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("DUAL_TIMELAPSE_{}", code)) }
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn ph(v: &V) -> Result<String, String> { baal_execution::core::payload_hash(v) }
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn parse_ms(s: &V) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(s.as_str().unwrap_or("")).unwrap_or(f64::NAN)
}
// JS Math.max/Math.min propagate NaN.
fn fmax(a: f64, b: f64) -> f64 { if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) } }
fn fmin(a: f64, b: f64) -> f64 { if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) } }

/// The resolved paper engine: which createDualNoPairState / advanceDualNoPair /
/// dualNoPairReport set the frozen module provides.
#[derive(Clone, Copy)]
pub struct PaperModule { pub exact_v2: bool }
impl PaperModule {
    pub fn create(&self, bank: &V) -> Result<V, String> {
        if self.exact_v2 { baal_collector::dual_no_pair_paper_v2::create_dual_no_pair_state_v2(bank) }
        else { baal_collector::dual_no_pair_paper_v1::create_dual_no_pair_state(bank) }
    }
    pub fn advance(&self, s: &V, e: &V, bank: &V) -> Result<V, String> {
        if self.exact_v2 { baal_collector::dual_no_pair_paper_v2::advance_dual_no_pair_v2(s, e, bank) }
        else { baal_collector::dual_no_pair_paper_v1::advance_dual_no_pair(s, e, bank) }
    }
    pub fn report(&self, s: &V, b: &V, at: &str) -> Result<V, String> {
        if self.exact_v2 { baal_collector::dual_no_pair_paper_v2::dual_no_pair_report_v2(s, b, at) }
        else { baal_collector::dual_no_pair_paper_v1::dual_no_pair_report(s, b, at) }
    }
}

/// adaptPaperModule(module, mode). In Rust the module namespace is always the
// matching crate module, so only the mode tag selects the function set.
pub fn adapt_paper_module(mode: &str) -> Result<PaperModule, String> {
    if mode == "dual_no_sequential_paper_v1" { return Ok(PaperModule { exact_v2: false }); }
    need(mode == "dual_no_sequential_paper_v2_exact_receipt", "MODE_ADAPTER")?;
    Ok(PaperModule { exact_v2: true })
}

/// compareIndependentReports(reference, current).
pub fn compare_independent_reports(reference: &V, current: &V) -> Result<V, String> {
    need(reference["schemaVersion"].as_i64() == Some(1)
        && !reference["strict"]["report"].is_null() && reference["source"]["sha256"].is_string()
        && reference["originalBankHash"].is_string(), "REFERENCE_REPORT")?;
    let summarize = |r: &V| -> V {
        json!({"cohort": r["cohort"], "bankHash": r["originalBankHash"],
            "cutoff": r["strict"]["report"]["cutoff"], "captureEnd": r["source"]["lastAt"],
            "fills": r["strict"]["report"]["fills"],
            "closedPackages": r["strict"]["report"]["closedPackages"],
            "realizedPnl": r["strict"]["report"]["realizedPnl"],
            "estimatedLiquidationPnl": r["strict"]["report"]["estimatedLiquidationPnl"],
            "conditionalSettlementSurplus": r["strict"]["report"]["ordinaryConditionalSettlementSurplus"]})
    };
    Ok(json!({"reference": summarize(reference), "current": summarize(current),
        "identicalSealedCapture": reference["source"]["sha256"] == current["source"]["sha256"],
        "sameOriginalCutoff": reference["strict"]["report"]["cutoff"] == current["strict"]["report"]["cutoff"],
        "profitabilityProof": false,
        "limitation": "Different frozen cohort cutoffs and sealed capture windows are independent experiments, not a paired causal cost-model comparison. Conditional surplus and open marks never prove realized or robust profit."}))
}

/// frozenStrategyModule(runtime, bank, modulePath): verified-archive check; the
/// returned object is only a marker since dispatch is static in Rust.
pub fn frozen_strategy_module(runtime_dir: &Path, bank: &V, module_path: &str,
    live_paper_dir: &Path) -> Result<(), String> {
    if frozen_paper_module(runtime_dir, bank, module_path)?.is_some() { return Ok(()); }
    // A missing archive never permits a future edited dependency graph. The full
    // original graph must still match before any current checkout fallback loads.
    if let V::Object(m) = &bank["codeIdentity"] {
        for (relative, hash) in m {
            let bytes = std::fs::read(live_paper_dir.join(relative)).map_err(|e| e.to_string())?;
            need(sha(&bytes) == hash.as_str().unwrap_or(""), "UNARCHIVED_GRAPH_CHANGED")?;
        }
    }
    Ok(())
}

/// evaluateCanonicalNoCapacity wrapper matching the JS single-object call shape.
pub fn evaluate_capacity(input: &V) -> Result<V, String> {
    let books: Vec<V> = input["books"].as_array().cloned().unwrap_or_default();
    let account = if input["account"].is_null() { None } else { Some(input["account"].clone()) };
    baal_collector::canonical_no_capacity::evaluate_canonical_no_capacity(
        &input["review"], &books, account.as_ref(),
        input["now"].as_str().unwrap_or(""), &input["policy"])
}

/// sealedEnd(file) — byte offset after the last newline in the ≤1MB tail.
pub fn sealed_end(file: &Path) -> Result<u64, String> {
    let size = std::fs::metadata(file).map_err(|e| e.to_string())?.len();
    let mut fd = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let want = size.min(1_048_576) as usize;
    let mut b = vec![0u8; want];
    fd.seek(SeekFrom::Start(size - want as u64)).and_then(|_| fd.read_exact(&mut b))
        .map_err(|e| e.to_string())?;
    match b.iter().rposition(|c| *c == b'\n') {
        Some(n) => Ok(size - want as u64 + n as u64 + 1),
        None => Err("DUAL_TIMELAPSE_EMPTY".into()),
    }
}

/// readEligibility(lines) — chain-verify account eligibility rows.
pub fn read_eligibility(lines: &[String]) -> Result<V, String> {
    let mut sequence = 0i64;
    let mut previous = String::new();
    let mut last = f64::NEG_INFINITY;
    let mut snapshots: Vec<V> = Vec::new();
    for line in lines {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let mut body = row.clone();
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        if let V::Object(m) = &mut body { m.shift_remove("hash"); }
        sequence += 1;
        need(baal_service_runtime::json_text::js_int(&row["sequence"]) == Some(sequence)
            && row["previous"].as_str().unwrap_or("") == previous
            && sha(js_stringify(&body).as_bytes()) == hash, "ACCOUNT_CHAIN")?;
        previous = hash.clone();
        let s = &row["event"]["snapshot"];
        need(row["state"].is_null()
            && row["event"]["type"].as_str() == Some("market_account_eligibility")
            && s["complete"] == V::Bool(true)
            && s["excludedMarketIds"].as_array().is_some_and(|a| a.iter().all(|x| {
                x.as_str().is_some_and(|s| {
                    s.bytes().all(|b| b.is_ascii_digit())
                        && s.len() > 0 && s.as_bytes()[0] != b'0'
                })
            })), "ACCOUNT_SCHEMA")?;
        let known_at = fmax(fmax(parse_ms(&row["at"]), parse_ms(&s["positionsAt"])), parse_ms(&s["ordersAt"]));
        need(known_at.is_finite() && known_at >= last
            && parse_ms(&s["ordersAt"]) >= parse_ms(&s["positionsAt"])
            && parse_ms(&s["expiresAt"]) == fmin(parse_ms(&s["positionsAt"]), parse_ms(&s["ordersAt"])) + 20000.0,
            "ACCOUNT_CLOCK")?;
        last = known_at;
        snapshots.push(json!({"knownAt": known_at, "snapshot": s, "sequence": sequence, "hash": hash}));
    }
    Ok(json!({"snapshots": snapshots, "sequence": sequence, "hash": previous}))
}

/// causalAccount(snapshots, requestAt, scope).
pub fn causal_account(snapshots: &[V], request_at: f64, scope: &V) -> Result<Option<V>, String> {
    let mut lo = 0usize;
    let mut hi = snapshots.len().saturating_sub(1);
    let mut index: i64 = -1;
    if !snapshots.is_empty() {
        while lo <= hi {
            let mid = (lo + hi) >> 1;
            if snapshots[mid]["knownAt"].as_f64().unwrap_or(f64::NAN) <= request_at {
                index = mid as i64;
                lo = mid + 1;
            } else if mid == 0 { break } else { hi = mid - 1; }
        }
    }
    if index < 0 { return Ok(None); }
    let s = &snapshots[index as usize]["snapshot"];
    need(s["tournamentId"] == scope["tournamentId"], "ACCOUNT_TOURNAMENT")?;
    let mut members: indexmap::IndexMap<String, String> = indexmap::IndexMap::new();
    for x in scope["exchanges"].as_array().cloned().unwrap_or_default() {
        members.insert(x["marketId"].as_str().unwrap_or("").to_string(),
            x["exchangeId"].as_str().unwrap_or("").to_string());
    }
    for id in s["excludedMarketIds"].as_array().cloned().unwrap_or_default() {
        if !members.contains_key(id.as_str().unwrap_or("")) { return Ok(None); }
    }
    let mut excluded: Vec<V> = s["excludedMarketIds"].as_array().cloned().unwrap_or_default().iter()
        .filter_map(|id| members.get(id.as_str().unwrap_or("")).map(|e| json!(e))).collect();
    excluded.sort_by(|a, b| {
        a.as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN)
            .partial_cmp(&b.as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN)).unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(Some(json!({"schemaVersion": 1, "positionsAt": s["positionsAt"], "ordersAt": s["ordersAt"],
        "excludedExchangeIds": excluded})))
}

/// verifyLiveJournal(lines, bank, module).
pub fn verify_live_journal(lines: &[String], bank: &V, module: &PaperModule) -> Result<V, String> {
    let mut seq = 0i64;
    let mut previous = String::new();
    let mut state = json!({"paper": module.create(bank)?,
        "sourceSequence": bank["sourcePrefixSequence"], "sourceHash": bank["sourcePrefixTailHash"]});
    for line in lines {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let mut body = row.clone();
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        if let V::Object(m) = &mut body { m.shift_remove("hash"); }
        seq += 1;
        need(baal_service_runtime::json_text::js_int(&row["sequence"]) == Some(seq)
            && row["previous"].as_str().unwrap_or("") == previous
            && sha(js_stringify(&body).as_bytes()) == hash, "LIVE_CHAIN")?;
        previous = hash;
        let e = &row["event"];
        if seq == 1 {
            need(e["kind"].as_str() == Some("genesis") && e["bankHash"].as_str() == ph(bank).ok().as_deref()
                && e["cutoff"] == bank["cutoff"], "LIVE_GENESIS")?;
        } else {
            need(["book", "boundary"].contains(&e["kind"].as_str().unwrap_or("")), "LIVE_EVENT")?;
            let p = module.advance(&state["paper"], e, bank)?;
            if e["kind"].as_str() == Some("book") {
                need(e["sourceSequence"].as_f64().unwrap_or(f64::NAN)
                    > state["sourceSequence"].as_f64().unwrap_or(f64::NAN), "LIVE_FRONTIER")?;
                state = json!({"paper": p, "sourceSequence": e["sourceSequence"], "sourceHash": e["sourceHash"]});
            } else {
                state["paper"] = p;
            }
        }
        need(ph(&state).ok() == ph(&row["state"]).ok(), "LIVE_STATE_PARITY")?;
    }
    Ok(json!({"records": seq, "journalHash": previous, "stateHash": ph(&state)?,
        "paperStateHash": ph(&state["paper"])?,
        "fills": state["paper"]["fills"].as_array().map(|a| a.len()).unwrap_or(0),
        "realizedPnl": state["paper"]["realizedPnl"],
        "sourceSequence": state["sourceSequence"], "sourceHash": state["sourceHash"],
        "exactAsRecordedIdentityParity": true}))
}

struct Experiment {
    bank: V, state: V, label: String,
    scope_books: i64, eligible_account_books: i64, no_eligible_account_books: i64,
    conditional_capacity_books: i64, eligible_pair_book_events: i64,
    account_ages_ms: Vec<f64>, account_missing_books: i64,
    current_pair_run_start: Option<f64>, last_eligible_pair_at: Option<f64>,
    longest_continuous_eligible_pair_ms: f64,
    first: Option<V>, last: Option<V>,
    days: serde_json::Map<String, V>, boundaries: i64,
}
fn new_experiment(bank: &V, module: &PaperModule, label: &str) -> Result<Experiment, String> {
    Ok(Experiment { bank: bank.clone(), state: module.create(bank)?, label: label.to_string(),
        scope_books: 0, eligible_account_books: 0, no_eligible_account_books: 0,
        conditional_capacity_books: 0, eligible_pair_book_events: 0,
        account_ages_ms: Vec::new(), account_missing_books: 0,
        current_pair_run_start: None, last_eligible_pair_at: None,
        longest_continuous_eligible_pair_ms: 0.0,
        first: None, last: None, days: serde_json::Map::new(), boundaries: 0 })
}

fn consume(x: &mut Experiment, row: &V, pub_state: &V, eligibility: &V, module: &PaperModule) -> Result<(), String> {
    let at = row["at"].as_str().unwrap_or("").to_string();
    if parse_ms(&row["at"]) < parse_ms(&x.bank["cutoff"]) { return Ok(()); }
    if row["kind"].as_str() != Some("book") {
        x.state = module.advance(&x.state, &json!({"kind": "boundary", "availableAt": at}), &x.bank)?;
        x.boundaries += 1;
        x.current_pair_run_start = None;
        x.last_eligible_pair_at = None;
        return Ok(());
    }
    let legs = x.bank["review"]["legs"].as_array().cloned().unwrap_or_default();
    let Some(leg) = legs.iter().find(|l| l["marketId"] == row["payload"]["book"]["marketId"]).cloned()
        else { return Ok(()); };
    let book = &pub_state["books"][&format!("{}:YES", leg["exchangeId"].as_str().unwrap_or(""))];
    if book.is_null() { return Ok(()); }
    let books: Vec<V> = legs.iter()
        .map(|l| pub_state["books"][&format!("{}:YES", l["exchangeId"].as_str().unwrap_or(""))].clone())
        .collect();
    let account = causal_account(eligibility["snapshots"].as_array().cloned().unwrap_or_default().as_slice(),
        parse_ms(&book["requestStartedAt"]), &pub_state["scope"])?;
    let mut admission = json!({"positiveConditionalCapacity": false});
    let all_present = books.iter().all(|b| !b.is_null());
    if !(pub_state["closed"] == V::Bool(true)) && all_present {
        match evaluate_capacity(&json!({"review": x.bank["review"], "policy": x.bank["capacityPolicy"],
            "books": books, "account": account.clone().unwrap_or(V::Null), "now": at})) {
            Ok(a) => admission = a,
            Err(_) => admission = json!({"positiveConditionalCapacity": false, "reason": "invalid_pair"}),
        }
    }
    x.scope_books += 1;
    if x.first.is_none() { x.first = Some(json!(at)); }
    x.last = Some(json!(at.clone()));
    let day_key = at.get(..10).unwrap_or("").to_string();
    let day = x.days.entry(day_key).or_insert_with(|| json!({"marketCounts": {},
        "accountEligibleBooks": 0, "accountMissingOrStaleBooks": 0}));
    {
        let mid = leg["marketId"].as_str().unwrap_or("").to_string();
        let mc = day["marketCounts"].as_object_mut();
        if let Some(mc) = mc {
            let cur = mc.get(&mid).and_then(|v| v.as_i64()).unwrap_or(0);
            mc.insert(mid, json!(cur + 1));
        }
    }
    let start_ms = parse_ms(&book["requestStartedAt"]);
    let at_ms = parse_ms(&row["at"]);
    let account_fresh = account.as_ref().is_some_and(|a| {
        [a["positionsAt"].as_str().unwrap_or(""), a["ordersAt"].as_str().unwrap_or("")].iter().all(|t| {
            let t = baal_service_runtime::contract::parse_iso_ms(t).unwrap_or(f64::NAN);
            t <= start_ms && at_ms - t <= 20000.0
        })
    });
    if let Some(a) = &account {
        x.account_ages_ms.push(at_ms - fmin(parse_ms(&a["positionsAt"]), parse_ms(&a["ordersAt"])));
    } else {
        x.account_missing_books += 1;
    }
    if account_fresh {
        x.eligible_account_books += 1;
        day["accountEligibleBooks"] = json!(day["accountEligibleBooks"].as_i64().unwrap_or(0) + 1);
    } else {
        x.no_eligible_account_books += 1;
        day["accountMissingOrStaleBooks"] = json!(day["accountMissingOrStaleBooks"].as_i64().unwrap_or(0) + 1);
    }
    if admission["positiveConditionalCapacity"] == V::Bool(true) { x.conditional_capacity_books += 1; }
    let present: Vec<&V> = books.iter().filter(|b| !b.is_null()).collect();
    let pair_starts: Vec<f64> = present.iter().map(|b| parse_ms(&b["requestStartedAt"])).collect();
    let pair_receipts: Vec<f64> = present.iter().map(|b| parse_ms(&b["receivedAt"])).collect();
    let min_start = pair_starts.iter().copied().fold(f64::INFINITY, fmin);
    let max_age = x.bank["policy"]["maxAgeMs"].as_f64().unwrap_or(f64::NAN);
    let max_skew = x.bank["policy"]["maxSkewMs"].as_f64().unwrap_or(f64::NAN);
    let excluded_ids: Vec<String> = account.as_ref()
        .map(|a| a["excludedExchangeIds"].as_array().cloned().unwrap_or_default().iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let eligible_pair = all_present
        && pair_starts.iter().all(|t| at_ms - t <= max_age)
        && pair_receipts.iter().copied().fold(f64::NEG_INFINITY, fmax) - min_start <= max_skew
        && account.as_ref().is_some_and(|a| {
            [a["positionsAt"].as_str().unwrap_or(""), a["ordersAt"].as_str().unwrap_or("")].iter().all(|t| {
                let t = baal_service_runtime::contract::parse_iso_ms(t).unwrap_or(f64::NAN);
                t <= min_start && at_ms - t <= 20000.0
            })
        })
        && !legs.iter().any(|l| excluded_ids.contains(&l["exchangeId"].as_str().unwrap_or("").to_string()));
    if eligible_pair {
        x.eligible_pair_book_events += 1;
        day["eligiblePairBookEvents"] = json!(day["eligiblePairBookEvents"].as_i64().unwrap_or(0) + 1);
        let now = at_ms;
        if x.last_eligible_pair_at.is_none() || now - x.last_eligible_pair_at.unwrap() > max_age {
            x.current_pair_run_start = Some(now);
        }
        x.last_eligible_pair_at = Some(now);
        x.longest_continuous_eligible_pair_ms = x.longest_continuous_eligible_pair_ms
            .max(now - x.current_pair_run_start.unwrap_or(now));
    } else {
        x.current_pair_run_start = None;
        x.last_eligible_pair_at = None;
    }
    let closed = pub_state["closed"] == V::Bool(true);
    let pair_books: Vec<V> = if closed { Vec::new() } else { present.iter().map(|b| (*b).clone()).collect() };
    x.state = module.advance(&x.state, &json!({"kind": "book", "book": book,
        "pairBooks": pair_books, "availableAt": at,
        "accountEligibility": if closed { V::Null } else { account.clone().unwrap_or(V::Null) },
        "admission": admission}), &x.bank)?;
    Ok(())
}

/// replayPublicationLines({lines, eligibility, bank, module, development, evaluateCapacity}).
pub fn replay_publication_lines(lines: &[String], eligibility: &V, bank: &V, module: &PaperModule,
    development: bool) -> Result<V, String> {
    let mut pub_state: Option<V> = None;
    let mut records = 0i64;
    let mut bytes = 0u64;
    let mut first: Option<V> = None;
    let mut last: Option<V> = None;
    let mut prefix_verified = false;
    let mut running_hash = Sha256::new();
    let mut strict = new_experiment(bank, module, "subsequent_original_frozen_bank")?;
    let mut dev: Option<Experiment> = None;
    let leg_ids: Vec<String> = bank["review"]["legs"].as_array().cloned().unwrap_or_default().iter()
        .map(|x| x["marketId"].as_str().unwrap_or("").to_string()).collect();
    let mut coverage_days: serde_json::Map<String, V> = serde_json::Map::new();
    let mut coverage_first: Option<V> = None;
    let mut coverage_last: Option<V> = None;
    let mut coverage_rows = 0i64;
    let mut leg_clocks: indexmap::IndexMap<String, f64> = indexmap::IndexMap::new();
    let mut leg_gaps: indexmap::IndexMap<String, Vec<f64>> = indexmap::IndexMap::new();
    for line in lines {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if pub_state.is_none() {
            need(row["kind"].as_str() == Some("session_start"), "FIRST_SESSION")?;
            pub_state = Some(create_book_publication_state(&row["payload"]["scope"])?);
            if development {
                let mut historical = bank.clone();
                historical["cutoff"] = row["at"].clone();
                historical["review"]["recordedAt"] =
                    json!(baal_service_runtime::contract::ms_to_iso(parse_ms(&row["at"]) - 1.0));
                historical["review"]["expiresAt"] = json!("2030-01-01T00:00:00.000Z");
                historical["capacityPolicy"]["maximumReviewAgeMs"] = json!(366.0 * 86_400_000.0);
                dev = Some(new_experiment(&historical, module,
                    "historical_development_static_rule_transport_assumption")?);
            }
        }
        let p = pub_state.take().unwrap();
        pub_state = Some(apply_book_publication(&p, &row)?);
        records += 1;
        if first.is_none() { first = Some(row["at"].clone()); }
        last = Some(row["at"].clone());
        bytes += line.len() as u64 + 1;
        running_hash.update(line.as_bytes());
        running_hash.update(b"\n");
        if bytes == bank["sourcePrefixBytes"].as_u64().unwrap_or(u64::MAX) {
            need(format!("{:x}", running_hash.clone().finalize()) == bank["sourcePrefixHash"].as_str().unwrap_or("")
                && baal_service_runtime::json_text::js_int(&row["sequence"])
                    == baal_service_runtime::json_text::js_int(&bank["sourcePrefixSequence"])
                && row["hash"].as_str().unwrap_or("") == bank["sourcePrefixTailHash"].as_str().unwrap_or(""),
                "FROZEN_PREFIX")?;
            prefix_verified = true;
        }
        if row["kind"].as_str() == Some("book")
            && leg_ids.contains(&row["payload"]["book"]["marketId"].as_str().unwrap_or("").to_string()) {
            coverage_rows += 1;
            if coverage_first.is_none() { coverage_first = Some(row["at"].clone()); }
            coverage_last = Some(row["at"].clone());
            let id = row["payload"]["book"]["marketId"].as_str().unwrap_or("").to_string();
            let clock = parse_ms(&row["payload"]["book"]["receivedAt"]);
            if let Some(prev) = leg_clocks.get(&id) {
                leg_gaps.entry(id.clone()).or_default().push(clock - prev);
            }
            leg_clocks.insert(id.clone(), clock);
            let day_key = row["at"].as_str().unwrap_or("").get(..10).unwrap_or("").to_string();
            let day = coverage_days.entry(day_key).or_insert_with(|| json!({}));
            let cur = day[&id].as_i64().unwrap_or(0);
            day[&id] = json!(cur + 1);
        }
        if baal_service_runtime::json_text::js_int(&row["sequence"])
            .zip(baal_service_runtime::json_text::js_int(&bank["sourcePrefixSequence"]))
            .is_some_and(|(a, b)| a > b) {
            consume(&mut strict, &row, pub_state.as_ref().unwrap(), eligibility, module)?;
        }
        if let Some(d) = dev.as_mut() {
            consume(d, &row, pub_state.as_ref().unwrap(), eligibility, module)?;
        }
    }
    need(prefix_verified, "PREFIX_NOT_IN_CAPTURE")?;
    let summarize = |x: &Experiment, last: &Option<V>| -> Result<V, String> {
        let at = last.clone().map(|v| v.as_str().unwrap_or("").to_string())
            .unwrap_or_else(|| x.bank["cutoff"].as_str().unwrap_or("").to_string());
        let report = module.report(&x.state, &x.bank, &at)?;
        let is_dev = x.label == "historical_development_static_rule_transport_assumption";
        Ok(json!({"label": x.label, "bankHash": ph(&x.bank)?,
            "scopeBooks": x.scope_books, "eligibleAccountBooks": x.eligible_account_books,
            "noEligibleAccountBooks": x.no_eligible_account_books,
            "accountMissingBooks": x.account_missing_books,
            "causalAccountAgeMs": timing_distribution(&x.account_ages_ms.iter().map(|a| json!(a)).collect::<Vec<_>>()),
            "eligiblePairBookEvents": x.eligible_pair_book_events,
            "longestContinuousEligiblePairMs": x.longest_continuous_eligible_pair_ms,
            "conditionalCapacityBooks": x.conditional_capacity_books,
            "firstBookAt": x.first, "lastBookAt": x.last,
            "days": V::Object(x.days.clone()), "boundaries": x.boundaries,
            "stateHash": ph(&x.state)?, "report": report,
            "openOrPendingCensored": report["positions"].as_array().map(|a| !a.is_empty()).unwrap_or(false)
                || baal_service_runtime::json_text::js_truthy(report.get("pending")),
            "nearTermProfitabilityEstablished": false,
            "ruleTransport": if is_dev { json!({
                "historicalAsOfRulesKnown": false,
                "originalRuleReviewRecordedAt": bank["review"]["recordedAt"],
                "assumptions": ["Today's exact party-winner predicates transported backward without historical rule verification",
                    "Review validity/age transported across captured history solely for development; not actual historical authorization",
                    "Original fill, depth, latency, cash, package limits and risk policy remain fixed",
                    "No current account snapshot backfill; only causally published eligibility records"]})
            } else { V::Null }}))
    };
    let first_ms = first.as_ref().map(|v| parse_ms(v)).unwrap_or(f64::NAN);
    let last_ms = last.as_ref().map(|v| parse_ms(v)).unwrap_or(f64::NAN);
    let cov_first_ms = coverage_first.as_ref().map(|v| parse_ms(v)).unwrap_or(f64::NAN);
    let cov_last_ms = coverage_last.as_ref().map(|v| parse_ms(v)).unwrap_or(f64::NAN);
    let days_with_both = coverage_days.values()
        .filter(|d| leg_ids.iter().all(|id| d[id].as_i64().unwrap_or(0) > 0)).count();
    let leg_gap_dist: serde_json::Map<String, V> = leg_ids.iter().map(|id| {
        let vals: Vec<V> = leg_gaps.get(id).cloned().unwrap_or_default().iter().map(|g| json!(g)).collect();
        (id.clone(), timing_distribution(&vals))
    }).collect();
    let longest = strict.longest_continuous_eligible_pair_ms
        .max(dev.as_ref().map(|d| d.longest_continuous_eligible_pair_ms).unwrap_or(0.0));
    Ok(json!({"source": {"records": records, "bytes": bytes,
        "sha256": format!("{:x}", running_hash.finalize()),
        "firstAt": first, "lastAt": last, "hours": (last_ms - first_ms) / 3_600_000.0,
        "sealedCompleteRecordsOnly": true, "frozenPrefixVerified": true},
        "coverage": {"firstAt": coverage_first, "lastAt": coverage_last, "rows": coverage_rows,
            "marketIds": leg_ids, "days": V::Object(coverage_days),
            "nanosecondSourceValidation": true, "missingDataInterpolated": false,
            "spanHours": if coverage_first.is_some() { (cov_last_ms - cov_first_ms) / 3_600_000.0 } else { 0.0 },
            "daysWithBothMarkets": days_with_both,
            "legReceiptGapsMs": V::Object(leg_gap_dist),
            "threeDaysOfContinuousEligibleCoverageEstablished": longest >= 72.0 * 3_600_000.0},
        "strict": summarize(&strict, &last)?,
        "development": match &dev { Some(d) => summarize(d, &last)?, None => V::Null }}))
}

/// main(argv) — argv is process.argv.slice(2).
pub fn main(argv: &[String]) -> Result<V, String> {
    let option = |key: &str, fallback: Option<String>| -> Option<String> {
        argv.iter().position(|a| a == key).map(|i| argv[i + 1].clone()).or(fallback)
    };
    let runtime = option("--runtime", None)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
            .join("SigTradingCup").join("paper-runtime"));
    let runtime = if runtime.is_absolute() { runtime } else {
        std::env::current_dir().map_err(|e| e.to_string())?.join(runtime)
    };
    let scenario = option("--scenario", Some("stress".into())).unwrap();
    need(["stress", "exact", "v2"].contains(&scenario.as_str()), "SCENARIO")?;
    let exact = scenario != "stress";
    let pointer_file = option("--pointer", Some(if exact { "dual-no-pair-v2-active.json" } else { "dual-no-pair-active.json" }.into())).unwrap();
    let explicit_cohort = option("--cohort", None);
    let cohort = match explicit_cohort {
        Some(c) => c,
        None => serde_json::from_slice::<V>(&std::fs::read(runtime.join(&pointer_file)).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?["cohort"].as_str().unwrap_or("").to_string(),
    };
    need(cohort.len() >= 2 && cohort.len() <= 64 && {
            let mut it = cohort.bytes();
            matches!(it.next(), Some(b) if b.is_ascii_lowercase())
                && it.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        }, "COHORT")?;
    let dir = runtime.join(if exact { "dual-no-pair-v2-cohorts" } else { "dual-no-pair-cohorts" }).join(&cohort);
    let bank_bytes = std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?;
    let bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
    let leg_markets: Vec<String> = bank["review"]["legs"].as_array().cloned().unwrap_or_default().iter()
        .map(|l| l["marketId"].as_str().unwrap_or("").to_string()).collect();
    let requested = option("--markets", Some(leg_markets.join(","))).unwrap();
    need(requested == leg_markets.join(","), "MARKET_BANK_SCOPE")?;
    need(bank["mode"].as_str() == Some(if exact { "dual_no_sequential_paper_v2_exact_receipt" } else { "dual_no_sequential_paper_v1" }),
        "SCENARIO_BANK_MODE")?;
    let live_paper_dir = std::env::current_dir().map_err(|e| e.to_string())?.join("live-paper");
    let module_path = format!("live-paper/dual-no-pair-paper-{}.mjs", if exact { "v2" } else { "v1" });
    frozen_strategy_module(&runtime, &bank, &module_path, &live_paper_dir)?;
    let module = adapt_paper_module(bank["mode"].as_str().unwrap_or(""))?;
    frozen_strategy_module(&runtime, &bank, "live-paper/canonical-no-capacity.mjs", &live_paper_dir)?;
    let start = std::time::Instant::now();
    let source = runtime.join("public-books.jsonl");
    let account = runtime.join("account-eligibility.jsonl");
    let live = dir.join("journal.jsonl");
    let ends = (sealed_end(&source)?, sealed_end(&account)?, sealed_end(&live)?);
    let opts = |end: u64| JournalReadOptions { end_byte: Some(end), ..Default::default() };
    let eligibility = read_eligibility(&journal_lines(&account, &opts(ends.1))?)?;
    let identity = verify_live_journal(&journal_lines(&live, &opts(ends.2))?, &bank, &module)?;
    let result = replay_publication_lines(&journal_lines(&source, &opts(ends.0))?,
        &eligibility, &bank, &module, argv.iter().any(|a| a == "--development"))?;
    need(sha(&std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?) == sha(&bank_bytes), "BANK_CHANGED")?;
    let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
    let parser_hash = sha(&std::fs::read(live_paper_dir.join("book-publication-v2.mjs")).map_err(|e| e.to_string())?);
    let snaps = eligibility["snapshots"].as_array().cloned().unwrap_or_default();
    let mut output = json!({"schemaVersion": 1, "createdAt": now_iso(), "cohort": cohort,
        "scenario": if exact { "exact_later_receipt_v2" } else { "one_adverse_tick_stress_v1" },
        "modulePath": module_path, "frozenCapacityModule": true,
        "originalBankHash": ph(&bank)?, "originalBankBytesSha256": sha(&bank_bytes),
        "frozenCodeIdentity": bank["codeIdentity"], "parserHash": parser_hash,
        "eligibility": {"records": eligibility["sequence"], "journalHash": eligibility["hash"],
            "firstKnownAt": baal_service_runtime::contract::ms_to_iso(snaps.first().map(|s| s["knownAt"].as_f64().unwrap_or(0.0)).unwrap_or(0.0)),
            "lastKnownAt": baal_service_runtime::contract::ms_to_iso(snaps.last().map(|s| s["knownAt"].as_f64().unwrap_or(0.0)).unwrap_or(0.0)),
            "currentAccountBackfilled": false},
        "identity": identity,
        "source": result["source"], "coverage": result["coverage"],
        "strict": result["strict"], "development": result["development"],
        "performance": {"wallMs": wall_ms,
            "simulatedSourceHours": result["source"]["hours"],
            "speedupAgainstWallClock": result["source"]["hours"].as_f64().unwrap_or(0.0) * 3_600_000.0 / wall_ms,
            "recordsPerSecond": result["source"]["records"].as_f64().unwrap_or(0.0) / (wall_ms / 1000.0),
            "intentSleeps": 0},
        "fees": "0.000000", "networkRequests": 0, "contestOrders": 0, "profitabilityEstablished": false,
        "limitations": ["Accelerated replay changes wall time, never the chronological availability or latency gates.",
            "As-recorded journal parity is exact; row-at replay is a separate virtual-consumption experiment, not necessarily bit-identical to live processing delays.",
            "Coverage gaps and pre-eligibility hours are censored; three days of executable paired observations are not manufactured.",
            "Historical static-rule transport is a development assumption, never held-out profitability evidence.",
            "Conditional settlement surplus is never realized P&L; unsold and unmatched inventory remains in results."]});
    if let Some(reference) = option("--reference-report", None) {
        let p = PathBuf::from(&reference);
        let p = if p.is_absolute() { p } else { std::env::current_dir().map_err(|e| e.to_string())?.join(p) };
        let reference: V = serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        output["independentComparison"] = compare_independent_reports(&reference, &output)?;
    }
    let default_out = format!("data/dual-no-timelapse{}-results-20261005.json", if exact { "-v2" } else { "" });
    let out = option("--out", Some(default_out)).unwrap();
    let out_path = PathBuf::from(&out);
    let out_path = if out_path.is_absolute() { out_path } else {
        std::env::current_dir().map_err(|e| e.to_string())?.join(out_path)
    };
    std::fs::write(&out_path, format!("{}\n", serde_json::to_string_pretty(&output).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    Ok(json!({"out": out_path, "cohort": cohort, "wallMs": wall_ms,
        "sourceHours": result["source"]["hours"],
        "strictFills": result["strict"]["report"]["fills"],
        "strictRealized": result["strict"]["report"]["realizedPnl"],
        "developmentFills": result["development"]["report"]["fills"],
        "developmentRealized": result["development"]["report"]["realizedPnl"]}))
}
