// data/replay-dual-no-timelapse-20261005.mjs — dual no-pair timelapse replay (module + CLI).
use baal_collector::book_publication_v2::{apply_book_publication, create_book_publication_state};
use baal_collector::canonical_no_capacity::evaluate_canonical_no_capacity;
use baal_collector::dual_no_pair_paper_v1::{advance_dual_no_pair, create_dual_no_pair_state,
    dual_no_pair_report};
use baal_collector::dual_no_pair_paper_v2::{advance_dual_no_pair_v2, create_dual_no_pair_state_v2,
    dual_no_pair_report_v2};
use baal_collector::frozen_paper_runtime::frozen_paper_module;
use baal_collector::read_timing::timing_distribution;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::js_stringify;
use baal_service_runtime::ledger::payload_hash as ledger_payload_hash;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn sha(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{:02x}", x)).collect()
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("DUAL_TIMELAPSE_{}", code)) }
}
fn instant(s: &V) -> f64 { s.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
fn payload_hash(v: &V) -> Result<String, String> {
    ledger_payload_hash(v).map_err(|e| e.to_string())
}

/// The JS paper module namespace: V1 exports unified names; V2 exports *_v2 names;
/// Empty stands in for a namespace lacking the v2 functions (e.g. `{}` in tests).
#[derive(Clone, Copy, PartialEq)]
pub enum PaperNamespace { V1, V2, Empty }

/// The adapted module object JS returns from adaptPaperModule: unified fn names.
#[derive(Clone, Copy, PartialEq)]
pub struct AdaptedPaper { pub v2: bool }

impl AdaptedPaper {
    pub fn create_dual_no_pair_state(&self, bank: &V) -> Result<V, String> {
        if self.v2 { create_dual_no_pair_state_v2(bank) } else { create_dual_no_pair_state(bank) }
    }
    pub fn advance_dual_no_pair(&self, state: &V, event: &V, bank: &V) -> Result<V, String> {
        if self.v2 { advance_dual_no_pair_v2(state, event, bank) }
        else { advance_dual_no_pair(state, event, bank) }
    }
    pub fn dual_no_pair_report(&self, state: &V, bank: &V, at: &str) -> Result<V, String> {
        if self.v2 { dual_no_pair_report_v2(state, bank, at) }
        else { dual_no_pair_report(state, bank, at) }
    }
}

/// adaptPaperModule(module, mode)
pub fn adapt_paper_module(module: PaperNamespace, mode: &str) -> Result<AdaptedPaper, String> {
    if mode == "dual_no_sequential_paper_v1" { return Ok(AdaptedPaper { v2: false }); }
    need(mode == "dual_no_sequential_paper_v2_exact_receipt" && module == PaperNamespace::V2,
        "MODE_ADAPTER")?;
    Ok(AdaptedPaper { v2: true })
}

/// compareIndependentReports(reference, current)
pub fn compare_independent_reports(reference: &V, current: &V) -> Result<V, String> {
    need(reference["schemaVersion"].as_i64() == Some(1)
        && reference["strict"]["report"].is_object()
        && reference["source"]["sha256"].is_string()
        && reference["originalBankHash"].is_string(), "REFERENCE_REPORT")?;
    let summarize = |r: &V| json!({"cohort": r["cohort"], "bankHash": r["originalBankHash"],
        "cutoff": r["strict"]["report"]["cutoff"], "captureEnd": r["source"]["lastAt"],
        "fills": r["strict"]["report"]["fills"],
        "closedPackages": r["strict"]["report"]["closedPackages"],
        "realizedPnl": r["strict"]["report"]["realizedPnl"],
        "estimatedLiquidationPnl": r["strict"]["report"]["estimatedLiquidationPnl"],
        "conditionalSettlementSurplus": r["strict"]["report"]["ordinaryConditionalSettlementSurplus"]});
    Ok(json!({"reference": summarize(reference), "current": summarize(current),
        "identicalSealedCapture": reference["source"]["sha256"] == current["source"]["sha256"],
        "sameOriginalCutoff": reference["strict"]["report"]["cutoff"]
            == current["strict"]["report"]["cutoff"],
        "profitabilityProof": false,
        "limitation": "Different frozen cohort cutoffs and sealed capture windows are independent experiments, not a paired causal cost-model comparison. Conditional surplus and open marks never prove realized or robust profit."}))
}

/// frozenStrategyModule(runtime, bank, modulePath) — returns the namespace kind.
/// A missing archive never permits an edited dependency graph: the whole original
/// graph must still match before the current checkout may be used.
fn frozen_strategy_module(root: &Path, runtime: &Path, bank: &V,
    module_path: &str) -> Result<PaperNamespace, String> {
    let namespace = if module_path.contains("-v2") { PaperNamespace::V2 }
        else { PaperNamespace::V1 };
    if let Some(_) = frozen_paper_module(runtime, bank, module_path)? {
        return Ok(namespace);
    }
    for (relative, hash) in bank["codeIdentity"].as_object().cloned().unwrap_or_default() {
        need(sha(&std::fs::read(root.join("live-paper").join(&relative))
                .map_err(|e| e.to_string())?) == hash.as_str().unwrap_or(""),
            "UNARCHIVED_GRAPH_CHANGED")?;
    }
    Ok(namespace)
}

/// sealedEnd(file)
pub fn sealed_end(file: &Path) -> Result<u64, String> {
    let size = std::fs::metadata(file).map_err(|e| e.to_string())?.len();
    let mut fd = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut b = vec![0u8; std::cmp::min(size, 1048576) as usize];
    fd.seek(SeekFrom::Start(size - b.len() as u64)).map_err(|e| e.to_string())?;
    fd.read(&mut b).map_err(|e| e.to_string())?;
    let n = b.iter().rposition(|x| *x == 10);
    need(n.is_some(), "EMPTY")?;
    Ok(size - b.len() as u64 + n.unwrap() as u64 + 1)
}

/// readEligibility(lines) -> {snapshots,sequence,hash}
pub fn read_eligibility(lines: &[String]) -> Result<V, String> {
    let mut sequence = 0i64;
    let mut previous = String::new();
    let mut last = f64::NEG_INFINITY;
    let mut snapshots: Vec<V> = Vec::new();
    for line in lines {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        if let Some(o) = body.as_object_mut() { o.remove("hash"); }
        sequence += 1;
        need(row["sequence"].as_i64() == Some(sequence)
            && row["previous"].as_str().unwrap_or("") == previous
            && sha(js_stringify(&body).as_bytes()) == hash, "ACCOUNT_CHAIN")?;
        previous = hash.clone();
        let s = &row["event"]["snapshot"];
        let excluded_ok = s["excludedMarketIds"].as_array().map(|a| a.iter().all(|x|
            x.as_str().map(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit())
                && !v.starts_with('0')).unwrap_or(false))).unwrap_or(false);
        need(row.get("state").map(|v| v.is_null()).unwrap_or(false)
            && row["event"]["type"].as_str() == Some("market_account_eligibility")
            && s["complete"] == json!(true) && excluded_ok, "ACCOUNT_SCHEMA")?;
        // Math.max propagates NaN (Rust f64::max does not)
        let known_at = [instant(&row["at"]), instant(&s["positionsAt"]),
            instant(&s["ordersAt"])].iter()
            .fold(f64::NAN, |m, v| if m.is_nan() || v.is_nan() { f64::NAN } else { m.max(*v) });
        need(known_at.is_finite() && known_at >= last
            && instant(&s["ordersAt"]) >= instant(&s["positionsAt"])
            && instant(&s["expiresAt"]) == instant(&s["positionsAt"])
                .min(instant(&s["ordersAt"])) + 20000.0, "ACCOUNT_CLOCK")?;
        last = known_at;
        snapshots.push(json!({"knownAt": known_at, "snapshot": s,
            "sequence": row["sequence"], "hash": hash}));
    }
    Ok(json!({"snapshots": snapshots, "sequence": sequence, "hash": previous}))
}

/// causalAccount(snapshots, requestAt, scope)
pub fn causal_account(snapshots: &[V], request_at: f64, scope: &V) -> Result<V, String> {
    let mut lo = 0usize;
    let mut hi: i64 = snapshots.len() as i64 - 1;
    let mut index: i64 = -1;
    while lo as i64 <= hi {
        let mid = ((lo as i64 + hi) >> 1) as usize;
        if snapshots[mid]["knownAt"].as_f64().unwrap_or(f64::NAN) <= request_at {
            index = mid as i64;
            lo = mid + 1;
        } else {
            hi = mid as i64 - 1;
        }
    }
    if index < 0 { return Ok(V::Null); }
    let s = &snapshots[index as usize]["snapshot"];
    need(s["tournamentId"] == scope["tournamentId"], "ACCOUNT_TOURNAMENT")?;
    let members: serde_json::Map<String, V> = scope["exchanges"].as_array().cloned()
        .unwrap_or_default().iter()
        .filter_map(|x| x["marketId"].as_str().map(|m| (m.to_string(), x["exchangeId"].clone())))
        .collect();
    if s["excludedMarketIds"].as_array().cloned().unwrap_or_default().iter()
        .any(|id| id.as_str().map(|m| !members.contains_key(m)).unwrap_or(true)) {
        return Ok(V::Null);
    }
    let mut excluded: Vec<V> = s["excludedMarketIds"].as_array().cloned().unwrap_or_default()
        .iter().filter_map(|id| id.as_str().and_then(|m| members.get(m).cloned())).collect();
    excluded.sort_by(|a, b| {
        a.as_str().and_then(|x| x.parse::<i64>().ok())
            .cmp(&b.as_str().and_then(|x| x.parse::<i64>().ok()))
    });
    Ok(json!({"schemaVersion": 1, "positionsAt": s["positionsAt"],
        "ordersAt": s["ordersAt"], "excludedExchangeIds": excluded}))
}

/// verifyLiveJournal(lines, bank, module)
pub fn verify_live_journal(lines: &[String], bank: &V,
    module: &AdaptedPaper) -> Result<V, String> {
    let mut seq = 0i64;
    let mut previous = String::new();
    let mut state = json!({"paper": module.create_dual_no_pair_state(bank)?,
        "sourceSequence": bank["sourcePrefixSequence"],
        "sourceHash": bank["sourcePrefixTailHash"]});
    for line in lines {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        if let Some(o) = body.as_object_mut() { o.remove("hash"); }
        seq += 1;
        need(row["sequence"].as_i64() == Some(seq)
            && row["previous"].as_str().unwrap_or("") == previous
            && sha(js_stringify(&body).as_bytes()) == hash, "LIVE_CHAIN")?;
        previous = hash;
        let e = &row["event"];
        if seq == 1 {
            need(e["kind"].as_str() == Some("genesis")
                && e["bankHash"].as_str().unwrap_or("") == payload_hash(bank)?
                && e["cutoff"] == bank["cutoff"], "LIVE_GENESIS")?;
        } else {
            need(["book", "boundary"].contains(&e["kind"].as_str().unwrap_or("")),
                "LIVE_EVENT")?;
            let p = module.advance_dual_no_pair(&state["paper"], e, bank)?;
            if e["kind"].as_str() == Some("book") {
                need(e["sourceSequence"].as_i64().unwrap_or(0)
                    > state["sourceSequence"].as_i64().unwrap_or(0), "LIVE_FRONTIER")?;
                state = json!({"paper": p, "sourceSequence": e["sourceSequence"],
                    "sourceHash": e["sourceHash"]});
            } else {
                state["paper"] = p;
            }
        }
        need(payload_hash(&state)? == payload_hash(&row["state"])?, "LIVE_STATE_PARITY")?;
    }
    Ok(json!({"records": seq, "journalHash": previous,
        "stateHash": payload_hash(&state)?,
        "paperStateHash": payload_hash(&state["paper"])?,
        "fills": state["paper"]["fills"].as_array().map(|a| a.len() as i64).unwrap_or(0),
        "realizedPnl": state["paper"]["realizedPnl"],
        "sourceSequence": state["sourceSequence"], "sourceHash": state["sourceHash"],
        "exactAsRecordedIdentityParity": true}))
}

struct Experiment {
    bank: V,
    state: V,
    label: String,
    scope_books: i64,
    eligible_account_books: i64,
    no_eligible_account_books: i64,
    conditional_capacity_books: i64,
    eligible_pair_book_events: i64,
    account_ages_ms: Vec<f64>,
    account_missing_books: i64,
    current_pair_run_start: Option<f64>,
    last_eligible_pair_at: Option<f64>,
    longest_continuous_eligible_pair_ms: f64,
    first: Option<V>,
    last: Option<V>,
    days: serde_json::Map<String, V>,
    boundaries: i64,
}
fn new_experiment(bank: &V, module: &AdaptedPaper, label: &str) -> Result<Experiment, String> {
    Ok(Experiment {
        bank: bank.clone(), state: module.create_dual_no_pair_state(bank)?,
        label: label.to_string(), scope_books: 0, eligible_account_books: 0,
        no_eligible_account_books: 0, conditional_capacity_books: 0,
        eligible_pair_book_events: 0, account_ages_ms: Vec::new(),
        account_missing_books: 0, current_pair_run_start: None,
        last_eligible_pair_at: None, longest_continuous_eligible_pair_ms: 0.0,
        first: None, last: None, days: serde_json::Map::new(), boundaries: 0 })
}

/// consume(x, row, pub, eligibility, module)
fn consume<F>(x: &mut Experiment, row: &V, pub_: &V, eligibility: &V,
    module: &AdaptedPaper, evaluate_capacity: &F) -> Result<(), String>
where F: Fn(&V, &[V], Option<&V>, &str, &V) -> Result<V, String> {
    let at = row["at"].as_str().unwrap_or("");
    if instant(&row["at"]) < instant(&x.bank["cutoff"]) { return Ok(()); }
    if row["kind"].as_str() != Some("book") {
        x.state = module.advance_dual_no_pair(&x.state,
            &json!({"kind": "boundary", "availableAt": at}), &x.bank)?;
        x.boundaries += 1;
        x.current_pair_run_start = None;
        x.last_eligible_pair_at = None;
        return Ok(());
    }
    let leg = x.bank["review"]["legs"].as_array().cloned().unwrap_or_default().into_iter()
        .find(|l| l["marketId"] == row["payload"]["book"]["marketId"]);
    let leg = match leg { Some(l) => l, None => return Ok(()) };
    let key = format!("{}:YES", leg["exchangeId"].as_str().unwrap_or(""));
    let book = pub_["books"].get(&key).cloned().unwrap_or(V::Null);
    if book.is_null() { return Ok(()); }
    let books: Vec<V> = x.bank["review"]["legs"].as_array().cloned().unwrap_or_default()
        .iter().map(|l| pub_["books"]
            .get(&format!("{}:YES", l["exchangeId"].as_str().unwrap_or("")))
            .cloned().unwrap_or(V::Null)).collect();
    let snapshots = eligibility["snapshots"].as_array().cloned().unwrap_or_default();
    let account = causal_account(&snapshots,
        instant(&book["requestStartedAt"]), &pub_["scope"])?;
    let mut admission = json!({"positiveConditionalCapacity": false});
    if pub_["closed"] != json!(true) && books.iter().all(|b| !b.is_null()) {
        match evaluate_capacity(&x.bank["review"], &books, if account.is_null() { None }
                else { Some(&account) }, at, &x.bank["capacityPolicy"]) {
            Ok(v) => admission = v,
            Err(_) => admission = json!({"positiveConditionalCapacity": false,
                "reason": "invalid_pair"}),
        }
    }
    x.scope_books += 1;
    if x.first.is_none() { x.first = Some(json!(at)); }
    x.last = Some(json!(at));
    let day_key = &at[..10.min(at.len())];
    let mut day = x.days.get(day_key).cloned()
        .unwrap_or_else(|| json!({"marketCounts": {}, "accountEligibleBooks": 0,
            "accountMissingOrStaleBooks": 0}));
    {
        let counts = day["marketCounts"].as_object_mut()
            .ok_or_else(|| "marketCounts".to_string())?;
        let id = leg["marketId"].as_str().unwrap_or("").to_string();
        let c = counts.entry(id).or_insert(json!(0));
        *c = json!(c.as_i64().unwrap_or(0) + 1);
    }
    let account_fresh = !account.is_null()
        && [account["positionsAt"].clone(), account["ordersAt"].clone()].iter()
            .all(|t| instant(t) <= instant(&book["requestStartedAt"])
                && instant(&row["at"]) - instant(t) <= 20000.0);
    if !account.is_null() {
        x.account_ages_ms.push(instant(&row["at"])
            - instant(&account["positionsAt"]).min(instant(&account["ordersAt"])));
    } else {
        x.account_missing_books += 1;
    }
    if account_fresh {
        x.eligible_account_books += 1;
        day["accountEligibleBooks"] = json!(day["accountEligibleBooks"].as_i64().unwrap_or(0) + 1);
    } else {
        x.no_eligible_account_books += 1;
        day["accountMissingOrStaleBooks"] =
            json!(day["accountMissingOrStaleBooks"].as_i64().unwrap_or(0) + 1);
    }
    if admission["positiveConditionalCapacity"] == json!(true) {
        x.conditional_capacity_books += 1;
    }
    let live_books: Vec<&V> = books.iter().filter(|b| !b.is_null()).collect();
    let pair_starts: Vec<f64> = live_books.iter()
        .map(|b| instant(&b["requestStartedAt"])).collect();
    let pair_receipts: Vec<f64> = live_books.iter()
        .map(|b| instant(&b["receivedAt"])).collect();
    let eligible_pair = books.iter().all(|b| !b.is_null())
        && pair_starts.iter().all(|t| instant(&row["at"]) - t
            <= x.bank["policy"]["maxAgeMs"].as_f64().unwrap_or(f64::NAN))
        && pair_receipts.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - pair_starts.iter().cloned().fold(f64::INFINITY, f64::min)
            <= x.bank["policy"]["maxSkewMs"].as_f64().unwrap_or(f64::NAN)
        && !account.is_null()
        && [account["positionsAt"].clone(), account["ordersAt"].clone()].iter()
            .all(|t| instant(t) <= pair_starts.iter().cloned().fold(f64::INFINITY, f64::min)
                && instant(&row["at"]) - instant(t) <= 20000.0)
        && !x.bank["review"]["legs"].as_array().cloned().unwrap_or_default().iter()
            .any(|l| account["excludedExchangeIds"].as_array().cloned().unwrap_or_default()
                .contains(&l["exchangeId"]));
    if eligible_pair {
        x.eligible_pair_book_events += 1;
        day["eligiblePairBookEvents"] =
            json!(day["eligiblePairBookEvents"].as_i64().unwrap_or(0) + 1);
        let now = instant(&row["at"]);
        if x.last_eligible_pair_at.is_none()
            || now - x.last_eligible_pair_at.unwrap()
                > x.bank["policy"]["maxAgeMs"].as_f64().unwrap_or(f64::NAN) {
            x.current_pair_run_start = Some(now);
        }
        x.last_eligible_pair_at = Some(now);
        x.longest_continuous_eligible_pair_ms = x.longest_continuous_eligible_pair_ms
            .max(now - x.current_pair_run_start.unwrap_or(now));
    } else {
        x.current_pair_run_start = None;
        x.last_eligible_pair_at = None;
    }
    x.days.insert(day_key.to_string(), day);
    x.state = module.advance_dual_no_pair(&x.state, &json!({"kind": "book",
        "book": book,
        "pairBooks": if pub_["closed"] == json!(true) { Vec::<V>::new() }
            else { books.iter().filter(|b| !b.is_null()).cloned().collect() },
        "availableAt": at,
        "accountEligibility": if pub_["closed"] == json!(true) { V::Null } else { account },
        "admission": admission}), &x.bank)?;
    Ok(())
}

/// replayPublicationLines({lines,eligibility,bank,module,development,evaluateCapacity})
pub fn replay_publication_lines<F>(lines: &[String], eligibility: &V, bank: &V,
    module: &AdaptedPaper, development: bool, evaluate_capacity: F) -> Result<V, String>
where F: Fn(&V, &[V], Option<&V>, &str, &V) -> Result<V, String> {
    let mut pub_: Option<V> = None;
    let mut records = 0i64;
    let mut bytes = 0u64;
    let mut first: Option<V> = None;
    let mut last: Option<V> = None;
    let mut prefix_verified = false;
    let mut hasher = Sha256::new();
    let mut strict = new_experiment(bank, module, "subsequent_original_frozen_bank")?;
    strict.bank["capacityPolicy"] = bank["capacityPolicy"].clone();
    let mut dev: Option<Experiment> = None;
    let mut coverage = json!({"firstAt": V::Null, "lastAt": V::Null, "rows": 0,
        "marketIds": bank["review"]["legs"].as_array().cloned().unwrap_or_default().iter()
            .map(|x| x["marketId"].clone()).collect::<Vec<V>>(),
        "days": {}, "nanosecondSourceValidation": true,
        "missingDataInterpolated": false});
    let mut leg_clocks: std::collections::HashMap<String, f64> = Default::default();
    let mut leg_gaps: std::collections::HashMap<String, Vec<f64>> = Default::default();
    for line in lines {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if pub_.is_none() {
            need(row["kind"].as_str() == Some("session_start"), "FIRST_SESSION")?;
            pub_ = Some(create_book_publication_state(&row["payload"]["scope"])?);
            if development {
                let mut historical = bank.clone();
                historical["cutoff"] = row["at"].clone();
                let t = (instant(&row["at"]) - 1.0) as i64;
                let secs = t / 1000;
                let rem = t % 1000;
                historical["review"]["recordedAt"] = json!(chrono::DateTime::
                    from_timestamp(secs, (rem.max(0) * 1_000_000) as u32)
                    .map(|d| d.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
                    .unwrap_or_default());
                historical["review"]["expiresAt"] = json!("2030-01-01T00:00:00.000Z");
                historical["capacityPolicy"]["maximumReviewAgeMs"] =
                    json!(366.0 * 86400000.0);
                dev = Some(new_experiment(&historical, module,
                    "historical_development_static_rule_transport_assumption")?);
            }
        }
        let p = pub_.take().unwrap();
        pub_ = Some(apply_book_publication(&p, &row)?);
        records += 1;
        if first.is_none() { first = Some(row["at"].clone()); }
        last = Some(row["at"].clone());
        let line_bytes = format!("{}\n", line);
        bytes += line_bytes.len() as u64;
        hasher.update(line_bytes.as_bytes());
        if bytes == bank["sourcePrefixBytes"].as_u64().unwrap_or(u64::MAX) {
            let h: String = hasher.clone().finalize().iter()
                .map(|x| format!("{:02x}", x)).collect();
            need(h == bank["sourcePrefixHash"].as_str().unwrap_or("")
                && row["sequence"] == bank["sourcePrefixSequence"]
                && row["hash"] == bank["sourcePrefixTailHash"], "FROZEN_PREFIX")?;
            prefix_verified = true;
        }
        if row["kind"].as_str() == Some("book")
            && bank["review"]["legs"].as_array().cloned().unwrap_or_default().iter()
                .any(|l| l["marketId"] == row["payload"]["book"]["marketId"]) {
            coverage["rows"] = json!(coverage["rows"].as_i64().unwrap_or(0) + 1);
            if coverage["firstAt"].is_null() { coverage["firstAt"] = row["at"].clone(); }
            coverage["lastAt"] = row["at"].clone();
            let id = row["payload"]["book"]["marketId"].as_str().unwrap_or("").to_string();
            let clock = instant(&row["payload"]["book"]["receivedAt"]);
            if let Some(prev) = leg_clocks.get(&id) {
                leg_gaps.entry(id.clone()).or_default().push(clock - prev);
            }
            leg_clocks.insert(id.clone(), clock);
            let day_key = row["at"].as_str().unwrap_or("")[..10.min(
                row["at"].as_str().unwrap_or("").len())].to_string();
            let days = coverage["days"].as_object_mut()
                .ok_or_else(|| "days".to_string())?;
            let day = days.entry(day_key).or_insert(json!({}));
            let d = day.as_object_mut().ok_or_else(|| "day".to_string())?;
            let c = d.entry(id).or_insert(json!(0));
            *c = json!(c.as_i64().unwrap_or(0) + 1);
        }
        if row["sequence"].as_i64().unwrap_or(0)
            > bank["sourcePrefixSequence"].as_i64().unwrap_or(0) {
            consume(&mut strict, &row, pub_.as_ref().unwrap(), eligibility, module,
                &evaluate_capacity)?;
        }
        if let Some(d) = dev.as_mut() {
            consume(d, &row, pub_.as_ref().unwrap(), eligibility, module, &evaluate_capacity)?;
        }
    }
    need(prefix_verified, "PREFIX_NOT_IN_CAPTURE")?;
    let summarize = |x: &Experiment| -> Result<V, String> {
        let at = last.clone().unwrap_or_else(|| x.bank["cutoff"].clone());
        let report = module.dual_no_pair_report(&x.state, &x.bank,
            at.as_str().unwrap_or(""))?;
        let positions_len = report["positions"].as_array().map(|a| a.len()).unwrap_or(0);
        Ok(json!({"label": x.label, "bankHash": payload_hash(&x.bank)?,
            "scopeBooks": x.scope_books, "eligibleAccountBooks": x.eligible_account_books,
            "noEligibleAccountBooks": x.no_eligible_account_books,
            "accountMissingBooks": x.account_missing_books,
            "causalAccountAgeMs": timing_distribution(&x.account_ages_ms.iter()
                .map(|v| json!(v)).collect::<Vec<V>>()),
            "eligiblePairBookEvents": x.eligible_pair_book_events,
            "longestContinuousEligiblePairMs": x.longest_continuous_eligible_pair_ms,
            "conditionalCapacityBooks": x.conditional_capacity_books,
            "firstBookAt": x.first, "lastBookAt": x.last,
            "days": V::Object(x.days.clone()), "boundaries": x.boundaries,
            "stateHash": payload_hash(&x.state)?,
            "report": report,
            "openOrPendingCensored": positions_len > 0
                || !(report["pending"].is_null() || report["pending"] == json!(false)
                    || report["pending"] == json!(0)),
            "nearTermProfitabilityEstablished": false,
            "ruleTransport": if x.label.starts_with("historical_development") {
                json!({"historicalAsOfRulesKnown": false,
                    "originalRuleReviewRecordedAt": bank["review"]["recordedAt"],
                    "assumptions": [
                        "Today's exact party-winner predicates transported backward without historical rule verification",
                        "Review validity/age transported across captured history solely for development; not actual historical authorization",
                        "Original fill, depth, latency, cash, package limits and risk policy remain fixed",
                        "No current account snapshot backfill; only causally published eligibility records"]})
            } else { V::Null }}))
    };
    let source_hours = (instant(&last.clone().unwrap_or(V::Null))
        - instant(&first.clone().unwrap_or(V::Null))) / 3600000.0;
    let leg_gap_dist: serde_json::Map<String, V> = coverage["marketIds"].as_array()
        .cloned().unwrap_or_default().iter().map(|id| {
            let key = id.as_str().unwrap_or("").to_string();
            (key.clone(), timing_distribution(&leg_gaps.get(&key).cloned()
                .unwrap_or_default().iter().map(|v| json!(v)).collect::<Vec<V>>()))
        }).collect();
    let days_with_both = coverage["days"].as_object().cloned().unwrap_or_default().values()
        .filter(|d| coverage["marketIds"].as_array().cloned().unwrap_or_default().iter()
            .all(|id| d[id.as_str().unwrap_or("")].as_i64().unwrap_or(0) > 0))
        .count();
    let span_hours = if coverage["firstAt"].is_null() { 0.0 } else {
        (instant(&coverage["lastAt"]) - instant(&coverage["firstAt"])) / 3600000.0
    };
    let mut cov = coverage.as_object().cloned().unwrap_or_default();
    cov.insert("spanHours".into(), json!(span_hours));
    cov.insert("daysWithBothMarkets".into(), json!(days_with_both as i64));
    cov.insert("legReceiptGapsMs".into(), V::Object(leg_gap_dist));
    cov.insert("threeDaysOfContinuousEligibleCoverageEstablished".into(),
        json!(strict.longest_continuous_eligible_pair_ms
            .max(dev.as_ref().map(|d| d.longest_continuous_eligible_pair_ms)
                .unwrap_or(0.0)) >= 72.0 * 3600000.0));
    Ok(json!({"source": {"records": records, "bytes": bytes,
            "sha256": sha(&hasher.finalize().to_vec()),
            "firstAt": first, "lastAt": last, "hours": source_hours,
            "sealedCompleteRecordsOnly": true, "frozenPrefixVerified": true},
        "coverage": V::Object(cov),
        "strict": summarize(&strict)?,
        "development": match &dev { Some(d) => summarize(d)?, None => V::Null }}))
}

pub fn run(root: &Path, argv: &[String]) -> Result<V, String> {
    let args = &argv[2.min(argv.len())..];
    let option = |key: &str, fallback: Option<&str>| -> Option<String> {
        args.iter().position(|a| a == key)
            .and_then(|i| args.get(i + 1)).cloned()
            .or_else(|| fallback.map(String::from))
    };
    let runtime = option("--runtime", None)
        .map(PathBuf::from).unwrap_or_else(|| Path::new(
            &std::env::var("LOCALAPPDATA").unwrap_or_default())
            .join("SigTradingCup").join("paper-runtime"));
    let scenario = option("--scenario", Some("stress")).unwrap();
    need(["stress", "exact", "v2"].contains(&scenario.as_str()), "SCENARIO")?;
    let exact = scenario != "stress";
    let pointer_file = option("--pointer",
        Some(if exact { "dual-no-pair-v2-active.json" } else { "dual-no-pair-active.json" }))
        .unwrap();
    let explicit = option("--cohort", None);
    let cohort = match explicit {
        Some(c) => c,
        None => serde_json::from_slice::<V>(
                &std::fs::read(runtime.join(&pointer_file)).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?["cohort"].as_str().unwrap_or("").to_string(),
    };
    let re = regex::Regex::new("^[a-z][a-z0-9-]{1,63}$").map_err(|e| e.to_string())?;
    need(re.is_match(&cohort), "COHORT")?;
    let dir = runtime.join(if exact { "dual-no-pair-v2-cohorts" } else { "dual-no-pair-cohorts" })
        .join(&cohort);
    let bank_bytes = std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?;
    let bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
    let requested = option("--markets", None).unwrap_or_else(|| {
        bank["review"]["legs"].as_array().cloned().unwrap_or_default().iter()
            .map(|l| l["marketId"].as_str().unwrap_or("").to_string())
            .collect::<Vec<String>>().join(",")
    });
    need(requested == bank["review"]["legs"].as_array().cloned().unwrap_or_default().iter()
        .map(|l| l["marketId"].as_str().unwrap_or("").to_string())
        .collect::<Vec<String>>().join(","), "MARKET_BANK_SCOPE")?;
    need(bank["mode"].as_str() == Some(if exact {
            "dual_no_sequential_paper_v2_exact_receipt" } else { "dual_no_sequential_paper_v1" }),
        "SCENARIO_BANK_MODE")?;
    let module_path = format!("live-paper/dual-no-pair-paper-{}.mjs",
        if exact { "v2" } else { "v1" });
    let module = adapt_paper_module(
        frozen_strategy_module(root, &runtime, &bank, &module_path)?,
        bank["mode"].as_str().unwrap_or(""))?;
    let _capacity_namespace = frozen_strategy_module(root, &runtime, &bank,
        "live-paper/canonical-no-capacity.mjs")?;
    let evaluate_capacity = |review: &V, books: &[V], account: Option<&V>, now: &str,
        policy: &V| evaluate_canonical_no_capacity(review, books, account, now, policy);
    let start = Instant::now();
    let source = runtime.join("public-books.jsonl");
    let account_file = runtime.join("account-eligibility.jsonl");
    let live = dir.join("journal.jsonl");
    let ends = json!({"books": sealed_end(&source)?, "account": sealed_end(&account_file)?,
        "live": sealed_end(&live)?});
    let account_lines = journal_lines(&account_file, &JournalReadOptions {
        end_byte: Some(ends["account"].as_u64().unwrap()), ..Default::default() })?;
    let eligibility = read_eligibility(&account_lines)?;
    let live_lines = journal_lines(&live, &JournalReadOptions {
        end_byte: Some(ends["live"].as_u64().unwrap()), ..Default::default() })?;
    let identity = verify_live_journal(&live_lines, &bank, &module)?;
    let source_lines = journal_lines(&source, &JournalReadOptions {
        end_byte: Some(ends["books"].as_u64().unwrap()), ..Default::default() })?;
    let result = replay_publication_lines(&source_lines, &eligibility, &bank, &module,
        args.iter().any(|a| a == "--development"), evaluate_capacity)?;
    need(sha(&std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?)
        == sha(&bank_bytes), "BANK_CHANGED")?;
    let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
    let mut output = json!({"schemaVersion": 1, "createdAt": ms_to_iso(now_ms()),
        "cohort": cohort,
        "scenario": if exact { "exact_later_receipt_v2" } else { "one_adverse_tick_stress_v1" },
        "modulePath": module_path, "frozenCapacityModule": true,
        "originalBankHash": payload_hash(&bank)?,
        "originalBankBytesSha256": sha(&bank_bytes),
        "frozenCodeIdentity": bank["codeIdentity"],
        "parserHash": sha(&std::fs::read(root.join("live-paper/book-publication-v2.mjs"))
            .map_err(|e| e.to_string())?),
        "eligibility": {"records": eligibility["sequence"],
            "journalHash": eligibility["hash"],
            "firstKnownAt": ms_to_iso(eligibility["snapshots"][0]["knownAt"]
                .as_f64().unwrap_or(f64::NAN)),
            "lastKnownAt": ms_to_iso(eligibility["snapshots"].as_array().cloned()
                .unwrap_or_default().last().cloned().unwrap_or(V::Null)["knownAt"]
                .as_f64().unwrap_or(f64::NAN)),
            "currentAccountBackfilled": false},
        "identity": identity,
        "source": result["source"], "coverage": result["coverage"],
        "strict": result["strict"], "development": result["development"],
        "performance": {"wallMs": wall_ms,
            "simulatedSourceHours": result["source"]["hours"],
            "speedupAgainstWallClock": result["source"]["hours"].as_f64().unwrap_or(0.0)
                * 3600000.0 / wall_ms,
            "recordsPerSecond": result["source"]["records"].as_f64().unwrap_or(0.0)
                / (wall_ms / 1000.0),
            "intentSleeps": 0},
        "fees": "0.000000", "networkRequests": 0, "contestOrders": 0,
        "profitabilityEstablished": false,
        "limitations": [
            "Accelerated replay changes wall time, never the chronological availability or latency gates.",
            "As-recorded journal parity is exact; row-at replay is a separate virtual-consumption experiment, not necessarily bit-identical to live processing delays.",
            "Coverage gaps and pre-eligibility hours are censored; three days of executable paired observations are not manufactured.",
            "Historical static-rule transport is a development assumption, never held-out profitability evidence.",
            "Conditional settlement surplus is never realized P&L; unsold and unmatched inventory remains in results."]});
    if let Some(reference) = option("--reference-report", None) {
        let r: V = serde_json::from_slice(&std::fs::read(Path::new(&reference))
            .map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        output["independentComparison"] = compare_independent_reports(&r, &output)?;
    }
    let default_out = if exact { "dual-no-timelapse-v2-results-20261005.json" }
        else { "dual-no-timelapse-results-20261005.json" };
    let out = option("--out", None)
        .map(PathBuf::from)
        .map(|p| if p.is_absolute() { p } else { root.join(p) })
        .unwrap_or_else(|| root.join("data").join(default_out));
    std::fs::write(&out, format!("{}\n",
        serde_json::to_string_pretty(&output).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&json!({"out": out.to_string_lossy(),
        "cohort": cohort, "wallMs": wall_ms,
        "sourceHours": result["source"]["hours"],
        "strictFills": result["strict"]["report"]["fills"],
        "strictRealized": result["strict"]["report"]["realizedPnl"],
        "developmentFills": result["development"]["report"]["fills"],
        "developmentRealized": result["development"]["report"]["realizedPnl"]})));
    Ok(output)
}
