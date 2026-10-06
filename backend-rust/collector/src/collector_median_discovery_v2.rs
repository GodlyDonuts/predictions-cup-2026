// live-paper/collector-median-discovery-v2.mjs — lagged-median discovery + extra risk sources.
// Optional candidate reads through the existing collector only. No API client.
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{digest, js_int, js_truthy};
use serde_json::{json, Value as V};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

const RECORDING_FAILED: &str = "Median discovery recording failed";
/// Thrown-object stand-in: JS errors may carry `status`, `durableRecordingFailed`, `fatal`.
pub struct MedianFault {
    pub message: String,
    pub status: Option<i64>,
    pub durable_recording_failed: bool,
    pub fatal: bool,
}
impl MedianFault {
    fn code(c: &str) -> Self { MedianFault { message: format!("MEDIAN_DISCOVERY_{}", c), status: None, durable_recording_failed: false, fatal: false } }
    fn recording() -> Self { MedianFault { message: RECORDING_FAILED.into(), status: None, durable_recording_failed: true, fatal: true } }
    fn raw(m: impl Into<String>) -> Self { MedianFault { message: m.into(), status: None, durable_recording_failed: false, fatal: false } }
}
impl From<String> for MedianFault {
    fn from(s: String) -> Self { MedianFault::raw(s) }
}
type R<T> = Result<T, MedianFault>;
fn need(ok: bool, code: &str) -> R<()> { if !ok { return Err(MedianFault::code(code)); } Ok(()) }

/// MEDIAN_DISCOVERY_FILES
pub const MEDIAN_DISCOVERY_FILES: [&str; 6] = [
    "live-paper/collector-median-discovery-v2.mjs", "live-paper/runner.mjs",
    "live-paper/market-price-publication.mjs", "live-paper/runtime.mjs",
    "live-paper/transport-runtime.mjs", "execution/ledger.mjs",
];
/// medianDiscoveryIdentity(workspace) — frozen-graph file hashes.
pub fn median_discovery_identity(workspace: &Path) -> Result<V, String> {
    let files = crate::frozen_paper_graph::collect_frozen_paper_files(workspace,
        &MEDIAN_DISCOVERY_FILES.iter().map(|s| s.to_string()).collect::<Vec<_>>())?;
    Ok(V::Object(files.into_iter().map(|(k, v)| (k, json!(v))).collect()))
}
fn ticks() -> &'static HashMap<u64, i64> {
    static MAP: OnceLock<HashMap<u64, i64>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        for n in 0..=200i64 {
            m.insert((n as f64 / 200.0).to_bits(), n);
            m.insert((1.0 - (200 - n) as f64 / 200.0).to_bits(), n);
        }
        m
    })
}
fn tick_of(x: f64) -> Option<i64> {
    let x = if x == 0.0 { 0.0 } else { x };
    ticks().get(&x.to_bits()).copied()
}
fn parse_at(v: &V) -> f64 { baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN) }
fn is_hex64(v: &V) -> bool { v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())) }
fn cohort_re(s: &str, extra_max: usize) -> bool {
    // /^[a-z][a-z0-9-]{1,extra_max}$/ — total length 2..(extra_max+1).
    let b = s.as_bytes();
    b.len() >= 2 && b.len() <= extra_max + 1 && b[0].is_ascii_lowercase()
        && b[1..].iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}
fn median_fault_filter(e: &MedianFault, fallback: &str) -> String {
    if e.message.starts_with("MEDIAN_DISCOVERY_")
        && e.message.chars().all(|c| c.is_ascii_uppercase() || c == '_')
    {
        e.message.clone()
    } else {
        format!("MEDIAN_DISCOVERY_{}", fallback)
    }
}
fn canonical_hash(v: &V) -> Result<String, String> { baal_execution::core::payload_hash(v) }

/// createLaggedMedianScreen(universe)
pub struct LaggedMedianScreen {
    universe: V,
    sequence: i64,
    previous: String,
    last: f64,
    series: HashMap<String, Vec<V>>,
    #[allow(dead_code)]
    members: HashMap<String, V>,
    signals: indexmap::IndexMap<String, V>,
    latest: indexmap::IndexMap<String, V>,
}
impl LaggedMedianScreen {
    pub fn new(universe: &V) -> Self {
        let members: HashMap<String, V> = universe["exchanges"].as_array().cloned().unwrap_or_default()
            .iter().map(|x| (x["exchangeId"].as_str().unwrap_or("").to_string(), x.clone())).collect();
        Self { universe: universe.clone(), sequence: 0, previous: String::new(), last: 0.0,
            series: HashMap::new(), members, signals: Default::default(), latest: Default::default() }
    }
    fn sorted_signals(&self) -> Vec<V> {
        let mut out: Vec<V> = self.signals.values().cloned().collect();
        out.sort_by(|a, b| {
            b["discountTicks"].as_f64().unwrap_or(0.0).partial_cmp(&a["discountTicks"].as_f64().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)
                .then(a["marketId"].as_str().unwrap_or("").cmp(b["marketId"].as_str().unwrap_or("")))
        });
        out
    }
    /// accept(row)
    pub fn accept(&mut self, row: &V) -> Result<Vec<V>, String> {
        let got = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        body.as_object_mut().and_then(|m| m.shift_remove("hash"));
        if row["sequence"].as_i64() != Some(self.sequence + 1)
            || row["previous"].as_str().unwrap_or("") != self.previous
            || got != digest(&body)
            || !row["state"].is_null()
            || row["event"]["type"].as_str() != Some("market_price_observation")
        {
            return Err("MEDIAN_DISCOVERY_SOURCE_CHAIN".into());
        }
        let s = crate::market_price_publication::validate_price_observation_pub(&row["event"]["snapshot"], &self.universe)?;
        let received = parse_at(&s["receivedAt"]);
        let request = parse_at(&s["requestStartedAt"]);
        if !(received >= self.last) { return Err("MEDIAN_DISCOVERY_SOURCE_CLOCK".into()); }
        for id in s["missingIds"].as_array().cloned().unwrap_or_default() {
            self.signals.shift_remove(id.as_str().unwrap_or(""));
            self.latest.shift_remove(id.as_str().unwrap_or(""));
        }
        for q in s["quotes"].as_array().cloned().unwrap_or_default() {
            let eid = q["exchangeId"].as_str().unwrap_or("").to_string();
            self.signals.shift_remove(&eid);
            self.latest.insert(eid.clone(), json!({"marketId": q["marketId"], "received": received, "ready": false}));
            if q["option"].as_str() != Some("YES") || js_truthy(q.get("crossed")) {
                continue;
            }
            let (Some(bid), Some(ask)) = (tick_of(q["bestBid"].as_f64().unwrap_or(f64::NAN)),
                tick_of(q["bestAsk"].as_f64().unwrap_or(f64::NAN))) else { continue; };
            if bid <= 0 || ask >= 200 || bid >= ask { continue; }
            let mut history = self.series.get(&eid).cloned().unwrap_or_default();
            history.retain(|x| x["received"].as_f64().unwrap_or(f64::NAN) >= received - 3600000.0);
            let prior: Vec<&V> = history.iter().filter(|x| x["received"].as_f64().unwrap_or(f64::NAN) < request).collect();
            if prior.len() >= 20
                && prior.last().unwrap()["received"].as_f64().unwrap_or(f64::NAN) - prior[0]["received"].as_f64().unwrap_or(f64::NAN) >= 1800000.0
            {
                let mut sorted: Vec<i64> = prior.iter().map(|x| x["bid"].as_i64().unwrap_or(0)).collect();
                sorted.sort();
                let median = (sorted[(sorted.len() - 1) / 2] + sorted[sorted.len() / 2]) as f64 / 2.0;
                self.latest.insert(eid.clone(), json!({"marketId": q["marketId"], "received": received,
                    "ready": true, "discountTicks": median - ask as f64, "historyReceipts": prior.len()}));
                if median - ask as f64 >= 1.0 {
                    self.signals.insert(eid.clone(), json!({"marketId": q["marketId"], "exchangeId": q["exchangeId"],
                        "laggedMedianBidTicks": median, "currentAskTicks": ask, "discountTicks": median - ask as f64,
                        "historyReceipts": prior.len(), "historyFirstAt": prior[0]["received"], "historyLastAt": prior.last().unwrap()["received"],
                        "requestStartedAt": request, "observedAt": received, "sourceSequence": row["sequence"], "sourceHash": got,
                        "depthVerified": false, "executionAuthority": false}));
                }
            }
            history.push(json!({"received": received, "bid": bid}));
            if history.len() > 10000 { return Err("MEDIAN_DISCOVERY_HISTORY_BOUND".into()); }
            self.series.insert(eid, history);
        }
        self.sequence = row["sequence"].as_i64().unwrap_or(0);
        self.previous = got;
        self.last = received;
        Ok(self.sorted_signals())
    }
    /// diagnostics(at, protectedIds=[])
    pub fn diagnostics(&self, at: i64, protected_ids: &[String]) -> V {
        let rows: Vec<&V> = self.latest.values().collect();
        let fresh: Vec<&V> = rows.iter().cloned()
            .filter(|x| x["ready"].as_bool().unwrap_or(false)
                && x["received"].as_f64().unwrap_or(f64::NAN) <= at as f64
                && at as f64 - x["received"].as_f64().unwrap_or(f64::NAN) <= 20000.0)
            .collect();
        let mut by_dip = serde_json::Map::new();
        let mut protected_by_dip = serde_json::Map::new();
        for n in [1i64, 2, 4] {
            by_dip.insert(n.to_string(), json!(fresh.iter().filter(|x| x["discountTicks"].as_f64().unwrap_or(0.0) >= n as f64).count()));
            protected_by_dip.insert(n.to_string(), json!(fresh.iter()
                .filter(|x| protected_ids.iter().any(|p| Some(p.as_str()) == x["marketId"].as_str())
                    && x["discountTicks"].as_f64().unwrap_or(0.0) >= n as f64).count()));
        }
        json!({"universeSize": self.universe["exchanges"].as_array().map(|e| e.len()).unwrap_or(0),
            "observedExchanges": rows.len(),
            "lagReadyExchanges": rows.iter().filter(|x| x["ready"].as_bool().unwrap_or(false)).count(),
            "freshLagReadyExchanges": fresh.len(),
            "qualifyingByDipTicks": by_dip, "qualifyingProtectedByDipTicks": protected_by_dip,
            "minimumHistoryReceipts": 20, "minimumSpanMs": 1800000, "laggedWindowMs": 3600000,
            "maximumCurrentAgeMs": 20000, "bulkIsExecutable": false, "profitabilityEstablished": false})
    }
    /// projection()
    pub fn projection(&self) -> V {
        json!({"source": {"sequence": self.sequence, "hash": self.previous}, "signals": self.sorted_signals()})
    }
}

fn account_fresh(a: &V, at: f64) -> bool {
    if a.is_null() || a["complete"] != V::Bool(true) { return false; }
    let available = parse_at(&a["availableAt"]);
    if !available.is_finite() || available > at { return false; }
    ["positionsAt", "ordersAt"].iter().all(|k| {
        let t = parse_at(&a["snapshot"][*k]);
        t.is_finite() && t <= available && at - t <= 20000.0
    })
}

/// tailAt(file, end)
fn tail_at(file: &Path, end: i64) -> R<V> {
    need(end > 0 && end as u64 <= std::fs::metadata(file).map_err(|e| MedianFault::raw(e.to_string()))?.len(), "RISK_PREFIX_SIZE")?;
    let size = (end as u64).min(2 * 1024 * 1024) as usize;
    use std::io::{Read, Seek, SeekFrom};
    let mut fd = std::fs::File::open(file).map_err(|e| MedianFault::raw(e.to_string()))?;
    fd.seek(SeekFrom::Start(end as u64 - size as u64)).map_err(|e| MedianFault::raw(e.to_string()))?;
    let mut bytes = vec![0u8; size];
    fd.read_exact(&mut bytes).map_err(|_| MedianFault::code("RISK_PREFIX_READ"))?;
    need(*bytes.last().unwrap_or(&0) == b'\n', "RISK_PREFIX_BOUNDARY")?;
    let begin = bytes[..size - 1].iter().rposition(|b| *b == b'\n').map(|p| p + 1).unwrap_or(0);
    need(begin > 0 || end == size as i64, "RISK_RECORD_BOUND")?;
    serde_json::from_slice(&bytes[begin..size - 1]).map_err(|e| MedianFault::raw(e.to_string()))
}
// Same worker/projection/bank/universe/journal contracts as probability-read-focus,
// extended only to explicitly registered pointer names. Never grants execution.
/// readAdditionalProbabilityRisk({runtimeDir, universe, source, now})
/// `live_paper_dir` stands in for import.meta.url for probabilityFocusCodeHash.
pub fn read_additional_probability_risk(runtime_dir: &Path, universe: &V, source: &V, now: i64, live_paper_dir: &Path) -> R<V> {
    validate_additional_risk_sources(std::slice::from_ref(source))?;
    let read_json = |p: &Path| -> R<V> {
        serde_json::from_slice(&std::fs::read(p).map_err(|e| MedianFault::raw(e.to_string()))?)
            .map_err(|e| MedianFault::raw(e.to_string()))
    };
    let active = read_json(&runtime_dir.join(source["activeFile"].as_str().unwrap_or("")))?;
    let dir = runtime_dir.join("probability-receipt-cohorts").join(source["cohort"].as_str().unwrap_or(""));
    let bank = read_json(&dir.join("bank.json"))?;
    let stats = read_json(&dir.join("stats.json"))?;
    let lock = read_json(&dir.join("worker.lock"))?;
    let projection = read_json(&dir.join("read-focus.json"))?;
    let projection_hash = projection["projectionHash"].clone();
    let mut body = projection.clone();
    body.as_object_mut().and_then(|m| m.shift_remove("projectionHash"));
    need(active["cohort"] == source["cohort"] && active["bankHash"] == source["bankHash"], "RISK_POINTER")?;
    need(cohort_re(body["observerCohort"].as_str().unwrap_or(""), 63), "RISK_OBSERVER_SCOPE")?;
    need(!runtime_dir.join("STOP").exists()
        && !["STOP", "OBSERVER_STOP"].iter().any(|n| dir.join(n).exists())
        && !runtime_dir.join("executable-edge-cohorts").join(body["observerCohort"].as_str().unwrap()).join("OBSERVER_STOP").exists(), "RISK_STOP")?;
    need(body["schemaVersion"].as_i64() == Some(1) && body["mode"].as_str() == Some("probability_depth_reads_only")
        && body["networkRequests"].as_i64() == Some(0) && body["competitionOrders"].as_i64() == Some(0)
        && body["codeHash"].as_str() == crate::probability_read_focus::probability_focus_code_hash(live_paper_dir).ok().as_deref()
        && projection_hash.as_str() == canonical_hash(&body).ok().as_deref()
        && body["statsHash"].as_str() == canonical_hash(&stats).ok().as_deref()
        && body["pid"] == lock["pid"] && body["lockId"] == lock["id"]
        && js_int(&lock["pid"]).is_some_and(|p| p > 0)
        && lock["id"].as_str().is_some_and(|s| !s.is_empty()), "RISK_PROJECTION")?;
    // process.kill(lock.pid, 0): propagates a raw error when the pid is dead.
    let pid = js_int(&lock["pid"]).unwrap_or(0);
    if !(pid > 0 && Path::new(&format!("/proc/{}", pid)).exists()) {
        return Err(MedianFault::raw("kill ESRCH"));
    }
    if bank["schemaVersion"].as_i64() == Some(4) {
        crate::probability_receipt_v3::validate_probability_receipt_bank(&bank)?;
    } else if bank["schemaVersion"].as_i64() == Some(3) {
        crate::probability_receipt_v2::validate_probability_receipt_bank(&bank)?;
    } else {
        crate::probability_receipt::validate_probability_receipt_bank(&bank)?;
    }
    let u = crate::breadth::validate_universe(&bank["universe"], universe["tournamentId"].as_str().unwrap_or(""))
        .map_err(MedianFault::raw)?;
    need(u["hash"] == universe["hash"]
        && digest(&u["exchanges"]) == digest(&universe["exchanges"])
        && digest(&bank) == source["bankHash"].as_str().unwrap_or("")
        && stats["bankHash"] == source["bankHash"] && stats["cohort"] == source["cohort"]
        && stats["mode"] == bank["mode"]
        && stats["failed"] == V::Bool(false) && stats["sourceClosed"] == V::Bool(false)
        && stats["competitionOrders"].as_i64() == Some(0) && stats["networkRequests"].as_i64() == Some(0), "RISK_BANK_SCOPE")?;
    let updated = parse_at(&stats["updatedAt"]);
    let cutoff = parse_at(&bank["cutoff"]);
    need(updated.is_finite() && updated.fract() == 0.0 && updated <= now as f64
        && cutoff.is_finite() && cutoff.fract() == 0.0 && cutoff <= updated
        && js_int(&stats["sourceBytes"]).is_some_and(|b| b as f64 >= bank["sourcePrefixBytes"].as_f64().unwrap_or(f64::NAN))
        && js_int(&stats["sourceSequence"]).is_some_and(|s| s as f64 >= bank["sourcePrefixSequence"].as_f64().unwrap_or(f64::NAN))
        && is_hex64(&stats["sourceHash"]), "RISK_SOURCE_CLOCK")?;
    let journal_file = dir.join("journal.jsonl");
    let journal_size = std::fs::metadata(&journal_file).map_err(|e| MedianFault::raw(e.to_string()))?.len() as i64;
    let row = tail_at(&journal_file, journal_size)?;
    let journal_hash = row["hash"].clone();
    let mut checkpoint = row.clone();
    checkpoint.as_object_mut().and_then(|m| m.shift_remove("hash"));
    need(digest(&checkpoint) == journal_hash.as_str().unwrap_or("")
        && journal_hash == stats["journalHash"]
        && checkpoint["sequence"] == stats["journalSequence"]
        && checkpoint["stateHash"] == stats["stateHash"]
        && checkpoint["bankHash"] == stats["bankHash"], "RISK_JOURNAL_TAIL")?;
    if checkpoint["kind"].as_str() == Some("batch") {
        need(parse_at(&checkpoint["availableAt"]) <= updated
            && checkpoint["sourceSequence"] == stats["sourceSequence"]
            && checkpoint["sourceHash"] == stats["sourceHash"]
            && checkpoint["sourceBytes"] == stats["sourceBytes"], "RISK_CHECKPOINT_CLOCK")?;
    }
    let published = tail_at(&runtime_dir.join("public-books.jsonl"), stats["sourceBytes"].as_i64().unwrap_or(0))?;
    let source_hash = published["hash"].clone();
    let mut published_body = published.clone();
    published_body.as_object_mut().and_then(|m| m.shift_remove("hash"));
    need(source_hash == stats["sourceHash"]
        && published["sequence"] == stats["sourceSequence"]
        && canonical_hash(&published_body).ok().as_deref() == source_hash.as_str()
        && parse_at(&published["at"]).is_finite()
        && parse_at(&published["at"]) <= updated, "RISK_PUBLIC_SOURCE")?;
    let mut wallet_keys: Vec<String> = stats["wallets"].as_object().map(|w| w.keys().cloned().collect()).unwrap_or_default();
    wallet_keys.sort();
    need(stats["wallets"].is_object() && wallet_keys.join("|") == "1000|3000", "RISK_WALLETS")?;
    let mut markets: indexmap::IndexMap<String, V> = Default::default();
    let members: BTreeMap<String, V> = u["exchanges"].as_array().cloned().unwrap_or_default()
        .iter().map(|x| (format!("{}:YES", x["exchangeId"].as_str().unwrap_or("")), x.clone())).collect();
    let fresh = now as f64 - updated <= 20000.0 && now as f64 - parse_at(&published["at"]) <= 20000.0;
    let mut unidentified_pending = false;
    for (_wk, wallet) in stats["wallets"].as_object().cloned().unwrap_or_default() {
        need(wallet["positions"].is_array() && wallet["pending"].is_array(), "RISK_WALLETS")?;
        for pos in wallet["positions"].as_array().cloned().unwrap_or_default() {
            let member = members.get(pos["instrument"].as_str().unwrap_or(""));
            need(member.is_some() && js_int(&pos["quantity"]).is_some_and(|q| q > 0), "RISK_POSITION")?;
            let member = member.unwrap();
            markets.insert(member["marketId"].as_str().unwrap_or("").to_string(),
                json!({"marketId": member["marketId"], "exchangeId": member["exchangeId"],
                    "evidence": {"kind": "attributed_probability_paper_holding",
                        "bankHash": source["bankHash"], "cohort": source["cohort"],
                        "statsUpdatedAt": stats["updatedAt"]}}));
        }
        for intent in wallet["pending"].as_array().cloned().unwrap_or_default() {
            need(["buy", "sell"].contains(&intent["action"].as_str().unwrap_or("")), "RISK_PENDING")?;
            let member = members.get(intent["instrument"].as_str().unwrap_or(""));
            if member.is_none() || member.unwrap()["marketId"] != intent["marketId"] {
                unidentified_pending = true;
                continue;
            }
            let member = member.unwrap();
            if !markets.contains_key(member["marketId"].as_str().unwrap_or("")) {
                markets.insert(member["marketId"].as_str().unwrap_or("").to_string(),
                    json!({"marketId": member["marketId"], "exchangeId": member["exchangeId"],
                        "evidence": {"kind": "attributed_probability_paper_pending",
                            "bankHash": source["bankHash"], "cohort": source["cohort"],
                            "action": intent["action"], "statsUpdatedAt": stats["updatedAt"]}}));
            }
        }
    }
    let mut bootstrap = Vec::new();
    if fresh && !unidentified_pending {
        for r in bank["research"].as_array().cloned().unwrap_or_default() {
            let known = parse_at(&r["sourceAt"]);
            let recorded = parse_at(&r["recordedAt"]);
            let expiry = parse_at(&r["expiresAt"]);
            let flags_ok = match &r["flags"] {
                V::Null => return Err(MedianFault::raw("flags")),
                V::Array(a) => a.is_empty(),
                V::String(s) => s.is_empty(),
                _ => true,
            };
            if js_truthy(r.get("settlementReviewed")) && flags_ok
                && known <= now as f64 && recorded <= now as f64 && (now as f64) < expiry
                && now as f64 - known <= bank["policy"]["maximumResearchAgeMs"].as_f64().unwrap_or(0.0)
                && !markets.contains_key(r["marketId"].as_str().unwrap_or(""))
            {
                bootstrap.push(json!({"kind": "research_bootstrap_read", "marketId": r["marketId"],
                    "exchangeId": r["exchangeId"], "source": source,
                    "evidence": {"phase": "depth_required", "bankHash": source["bankHash"],
                        "cohort": source["cohort"], "researchHash": digest(&r),
                        "sourceAt": r["sourceAt"], "recordedAt": r["recordedAt"], "expiresAt": r["expiresAt"],
                        "maximumResearchAgeMs": bank["policy"]["maximumResearchAgeMs"],
                        "statsUpdatedAt": stats["updatedAt"], "sourceSequence": stats["sourceSequence"],
                        "sourceHash": stats["sourceHash"], "unowned": true}}));
            }
        }
    }
    Ok(json!({"fresh": fresh, "unidentifiedPending": unidentified_pending, "bootstrap": bootstrap,
        "markets": markets.values().cloned().collect::<Vec<_>>(),
        "sourceSequence": stats["sourceSequence"], "sourceHash": stats["sourceHash"],
        "updatedAt": stats["updatedAt"], "executionAuthority": false}))
}
/// validateAdditionalRiskSources(sources)
pub fn validate_additional_risk_sources(sources: &[V]) -> R<&[V]> {
    need(sources.len() <= 8, "ADDITIONAL_RISK_CONFIG")?;
    let mut keys = BTreeSet::new();
    let mut cohorts = BTreeSet::new();
    for source in sources {
        let mut sorted_keys: Vec<String> = source.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
        sorted_keys.sort();
        // /^[a-z][a-z0-9-]{1,79}\.json$/ — name part is 2..80 chars + '.json'.
        let active_ok = source["activeFile"].as_str().is_some_and(|s| {
            s.len() >= 7 && s.ends_with(".json") && cohort_re(&s[..s.len() - 5], 79)
        });
        need(source.is_object()
            && sorted_keys.join("|") == "activeFile|bankHash|cohort|kind"
            && source["kind"].as_str() == Some("probability_receipt")
            && active_ok
            && cohort_re(source["cohort"].as_str().unwrap_or(""), 63)
            && is_hex64(&source["bankHash"])
            && !keys.contains(source["activeFile"].as_str().unwrap_or(""))
            && !cohorts.contains(source["cohort"].as_str().unwrap_or("")), "ADDITIONAL_RISK_CONFIG")?;
        keys.insert(source["activeFile"].as_str().unwrap().to_string());
        cohorts.insert(source["cohort"].as_str().unwrap().to_string());
    }
    Ok(sources)
}

/// openCollectorMedianDiscovery({runtimeDir, workspace, universe, publisher,
/// request, refreshAccount, getAccount, headroom, publishBook,
/// beforeCandidateRead, afterCandidateReceipt, canOperate, now})
#[allow(clippy::type_complexity)]
pub struct CollectorMedianDiscoveryV2 {
    runtime_dir: PathBuf,
    workspace: PathBuf,
    universe: V,
    publisher: Arc<dyn Fn() -> V + Send + Sync>,
    request: Arc<dyn Fn(&str) -> Result<V, MedianFault> + Send + Sync>,
    refresh_account: Arc<dyn Fn() -> Result<V, MedianFault> + Send + Sync>,
    get_account: Arc<dyn Fn() -> V + Send + Sync>,
    headroom: Arc<dyn Fn(i64) -> bool + Send + Sync>,
    publish_book: Arc<dyn Fn(&str, &V) -> V + Send + Sync>,
    before_candidate_read: Option<Arc<dyn Fn(&str) -> Result<V, MedianFault> + Send + Sync>>,
    after_candidate_receipt: Option<Arc<dyn Fn(&str, &V) -> Result<V, MedianFault> + Send + Sync>>,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    live_paper_dir: PathBuf,
    config_file: PathBuf,
    config: V,
    identity: V,
    source: PathBuf,
    end: u64,
    head: V,
    screen: LaggedMedianScreen,
    attempts: Journal,
    watch: indexmap::IndexMap<String, V>,
    held: indexmap::IndexMap<String, V>,
    additional_held: indexmap::IndexMap<String, indexmap::IndexMap<String, V>>,
    last_attempt: HashMap<String, f64>,
    last_protected: Vec<V>,
    ready: bool,
    failed: bool,
    queue: Vec<V>,
    status: V,
    started: i64,
}
#[allow(clippy::too_many_arguments)]
impl CollectorMedianDiscoveryV2 {
    /// Returns Ok(None) when the config file is missing or disabled (JS null).
    pub fn open(runtime_dir: &Path, workspace: &Path, universe: &V,
        publisher: Arc<dyn Fn() -> V + Send + Sync>,
        request: Arc<dyn Fn(&str) -> Result<V, MedianFault> + Send + Sync>,
        refresh_account: Arc<dyn Fn() -> Result<V, MedianFault> + Send + Sync>,
        get_account: Arc<dyn Fn() -> V + Send + Sync>,
        headroom: Arc<dyn Fn(i64) -> bool + Send + Sync>,
        publish_book: Arc<dyn Fn(&str, &V) -> V + Send + Sync>,
        before_candidate_read: Option<Arc<dyn Fn(&str) -> Result<V, MedianFault> + Send + Sync>>,
        after_candidate_receipt: Option<Arc<dyn Fn(&str, &V) -> Result<V, MedianFault> + Send + Sync>>,
        can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
        now: Arc<dyn Fn() -> i64 + Send + Sync>,
    ) -> R<Option<Self>> {
        let config_file = runtime_dir.join("median-discovery.json");
        if !config_file.exists() { return Ok(None); }
        let config: V = serde_json::from_slice(&std::fs::read(&config_file).map_err(|e| MedianFault::raw(e.to_string()))?)
            .map_err(|e| MedianFault::raw(e.to_string()))?;
        if config["enabled"] != V::Bool(true) { return Ok(None); }
        need(config["schemaVersion"].as_i64() == Some(2)
            && config["owner"].as_str() == Some("Codex")
            && config["universeHash"] == universe["hash"]
            && config["everyCycles"].as_i64() == Some(3)
            && config["maximumAuxiliarySlots"].as_i64() == Some(2)
            && config["protectedBookSlots"].as_i64() == Some(4)
            && config["maximumAccountAgeMs"].as_i64() == Some(20000)
            && cohort_re(config["riskSource"]["cohort"].as_str().unwrap_or(""), 79)
            && is_hex64(&config["riskSource"]["bankHash"]), "CONFIG")?;
        validate_additional_risk_sources(config["additionalRiskSources"].as_array().map(|a| a.as_slice()).unwrap_or(&[]))?;
        let identity = median_discovery_identity(workspace).map_err(MedianFault::raw)?;
        need(digest(&config["codeIdentity"]) == digest(&identity), "CODE_IDENTITY")?;
        let source = runtime_dir.join("market-prices.jsonl");
        let end = std::fs::metadata(&source).map_err(|e| MedianFault::raw(e.to_string()))?.len();
        let head = (publisher)();
        let screen = LaggedMedianScreen::new(universe);
        let attempts = Journal::open(&runtime_dir.join("median-discovery-attempts.jsonl")).map_err(MedianFault::raw)?;
        let mut watch: indexmap::IndexMap<String, V> = Default::default();
        let mut held: indexmap::IndexMap<String, V> = Default::default();
        let mut last_attempt: HashMap<String, f64> = HashMap::new();
        if attempts.file.exists() {
            let raw = std::fs::read_to_string(&attempts.file).map_err(|e| MedianFault::raw(e.to_string()))?;
            for line in raw.trim().split('\n').filter(|l| !l.is_empty()) {
                let row: V = serde_json::from_str(line).map_err(|e| MedianFault::raw(e.to_string()))?;
                let e = &row["event"];
                if e["kind"].as_str() == Some("candidate_read_intent") {
                    last_attempt.insert(e["candidate"]["marketId"].as_str().unwrap_or("").to_string(), parse_at(&e["at"]));
                    if e["candidate"]["kind"].is_null() || e["candidate"]["kind"].as_str() == Some("research_bootstrap_read") {
                        watch.insert(e["candidate"]["marketId"].as_str().unwrap_or("").to_string(),
                            json!({"marketId": e["candidate"]["marketId"], "exchangeId": e["candidate"]["exchangeId"],
                                "until": parse_at(&e["at"]) + 150000.0, "evidence": e["candidate"]}));
                    }
                }
                if e["kind"].as_str() == Some("protected_receipt_watch") {
                    for market_id in e["marketIds"].as_array().cloned().unwrap_or_default() {
                        let member = universe["exchanges"].as_array().cloned().unwrap_or_default()
                            .into_iter().find(|x| x["marketId"] == market_id);
                        watch.insert(market_id.as_str().unwrap_or("").to_string(),
                            json!({"marketId": market_id, "exchangeId": member.map(|m| m["exchangeId"].clone()).unwrap_or(V::Null),
                                "until": parse_at(&e["at"]) + 150000.0, "evidence": {"kind": "protected_receipt_watch"}}));
                    }
                }
                if e["kind"].as_str() == Some("median_risk_projection") {
                    held.clear();
                    for x in e["held"].as_array().cloned().unwrap_or_default() {
                        held.insert(x["marketId"].as_str().unwrap_or("").to_string(), x);
                    }
                }
            }
        }
        let mut additional_held: indexmap::IndexMap<String, indexmap::IndexMap<String, V>> = Default::default();
        if attempts.file.exists() {
            let raw = std::fs::read_to_string(&attempts.file).map_err(|e| MedianFault::raw(e.to_string()))?;
            for line in raw.trim().split('\n').filter(|l| !l.is_empty()) {
                let row: V = serde_json::from_str(line).map_err(|e| MedianFault::raw(e.to_string()))?;
                let e = &row["event"];
                if e["kind"].as_str() == Some("additional_probability_risk_projection") {
                    let inner: indexmap::IndexMap<String, V> = e["markets"].as_array().cloned().unwrap_or_default()
                        .into_iter().map(|x| (x["marketId"].as_str().unwrap_or("").to_string(), x)).collect();
                    additional_held.insert(e["sourceKey"].as_str().unwrap_or("").to_string(), inner);
                }
            }
        }
        Ok(Some(Self {
            runtime_dir: runtime_dir.to_path_buf(), workspace: workspace.to_path_buf(),
            universe: universe.clone(), publisher, request, refresh_account, get_account,
            headroom, publish_book, before_candidate_read, after_candidate_receipt,
            can_operate, now: now.clone(), live_paper_dir: workspace.join("live-paper"),
            config_file, config, identity, source, end, head, screen, attempts, watch, held,
            additional_held, last_attempt,
            last_protected: Vec::new(), ready: false, failed: false, queue: Vec::new(),
            status: json!({"status": "loading", "executionAuthority": false}),
            started: (now)(),
        }))
    }
    fn basic_allowed(&self) -> bool {
        (self.can_operate)() && !self.runtime_dir.join("MEDIAN_DISCOVERY_STOP").exists()
    }
    fn allowed(&self) -> bool {
        self.basic_allowed()
            && !self.runtime_dir.join("MEDIAN_DISCOVERY_STOP").exists()
            && digest(&self.identity)
                == median_discovery_identity(&self.workspace).map(|i| digest(&i)).unwrap_or_default()
            && digest(&self.config)
                == std::fs::read(&self.config_file)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<V>(&b).ok())
                    .map(|c| digest(&c))
                    .unwrap_or_default()
    }
    fn record(&mut self, event: &V) -> R<()> {
        self.attempts.commit(&V::Null, event).map_err(|_| MedianFault::recording())?;
        Ok(())
    }
    fn risks(&mut self) -> R<V> {
        let mut fresh = false;
        let mut pending_unknown = true;
        let mut bootstrap: indexmap::IndexMap<String, V> = Default::default();
        let file = self.runtime_dir.join("median-paper-cohorts")
            .join(self.config["riskSource"]["cohort"].as_str().unwrap_or("")).join("stats.json");
        let r = (|| -> R<()> {
            let stats: V = serde_json::from_slice(&std::fs::read(&file).map_err(|e| MedianFault::raw(e.to_string()))?)
                .map_err(|e| MedianFault::raw(e.to_string()))?;
            need(stats["cohort"] == self.config["riskSource"]["cohort"]
                && stats["bankHash"] == self.config["riskSource"]["bankHash"]
                && stats["executionAuthority"] == V::Bool(false)
                && stats["counterfactualOnly"] == V::Bool(true)
                && parse_at(&stats["updatedAt"]) <= (self.now)() as f64, "RISK_SCOPE")?;
            let wallets: Vec<V> = stats["wallets"].as_object().map(|w| w.values().cloned().collect()).unwrap_or_default();
            need(wallets.len() == 3 && wallets.iter().all(|w| w["positions"].is_array()
                && js_int(&w["pendingIntents"]).is_some_and(|p| p >= 0)), "RISK_WALLETS")?;
            fresh = (self.now)() as f64 - parse_at(&stats["updatedAt"]) <= 120000.0;
            let pending = wallets.iter().map(|w| w["pendingIntents"].as_i64().unwrap_or(0)).max().unwrap_or(0);
            pending_unknown = pending > 0;
            if fresh {
                let mut next: indexmap::IndexMap<String, V> = Default::default();
                for w in &wallets {
                    for pos in w["positions"].as_array().cloned().unwrap_or_default() {
                        need(self.universe["exchanges"].as_array().cloned().unwrap_or_default().iter()
                            .any(|x| x["marketId"] == pos["marketId"] && x["exchangeId"] == pos["exchangeId"]), "RISK_MARKET")?;
                        next.insert(pos["marketId"].as_str().unwrap_or("").to_string(),
                            json!({"marketId": pos["marketId"], "exchangeId": pos["exchangeId"],
                                "evidence": {"kind": "attributed_median_paper_holding",
                                    "bankHash": stats["bankHash"], "statsUpdatedAt": stats["updatedAt"]}}));
                    }
                }
                if !pending_unknown { self.held.clear(); }
                for (k, v) in next { self.held.insert(k, v); }
                self.record(&json!({"kind": "median_risk_projection", "held": self.held.values().cloned().collect::<Vec<_>>(),
                    "pendingUnknown": pending_unknown, "sourceUpdatedAt": stats["updatedAt"], "bankHash": stats["bankHash"],
                    "at": baal_service_runtime::contract::ms_to_iso((self.now)() as f64), "executionAuthority": false}))?;
            }
            Ok(())
        })();
        if let Err(e) = r {
            if e.durable_recording_failed || e.fatal { return Err(e); }
            fresh = false;
            pending_unknown = true;
        }
        let sources = self.config["additionalRiskSources"].as_array().cloned().unwrap_or_default();
        for source in sources {
            let key = format!("{}:{}", source["activeFile"].as_str().unwrap_or(""), source["bankHash"].as_str().unwrap_or(""));
            let mut prior = self.additional_held.get(&key).cloned().unwrap_or_default();
            let observed = read_additional_probability_risk(&self.runtime_dir.clone(), &self.universe.clone(), &source,
                (self.now)(), &self.live_paper_dir.clone()).ok();
            match &observed {
                None => { fresh = false; pending_unknown = true; }
                Some(observed) => {
                    fresh = fresh && observed["fresh"].as_bool().unwrap_or(false);
                    pending_unknown = pending_unknown || js_truthy(observed.get("unidentifiedPending"));
                    if observed["fresh"].as_bool().unwrap_or(false) && !js_truthy(observed.get("unidentifiedPending")) {
                        for m in observed["bootstrap"].as_array().cloned().unwrap_or_default() {
                            bootstrap.insert(m["marketId"].as_str().unwrap_or("").to_string(), m);
                        }
                    }
                    {
                        if observed["fresh"].as_bool().unwrap_or(false) && !js_truthy(observed.get("unidentifiedPending")) {
                            prior.clear();
                        }
                        for m in observed["markets"].as_array().cloned().unwrap_or_default() {
                            prior.insert(m["marketId"].as_str().unwrap_or("").to_string(), m);
                        }
                        self.additional_held.insert(key.clone(), prior.clone());
                        self.record(&json!({"kind": "additional_probability_risk_projection",
                            "sourceKey": key, "source": source,
                            "markets": prior.values().cloned().collect::<Vec<_>>(),
                            "unidentifiedPending": observed["unidentifiedPending"],
                            "sourceSequence": observed["sourceSequence"], "sourceHash": observed["sourceHash"],
                            "updatedAt": observed["updatedAt"],
                            "at": baal_service_runtime::contract::ms_to_iso((self.now)() as f64),
                            "executionAuthority": false}))?;
                    }
                }
            }
            for (id, m) in prior { self.held.insert(id, m); }
        }
        let registered: BTreeSet<String> = self.config["additionalRiskSources"].as_array().cloned().unwrap_or_default()
            .iter().map(|s| format!("{}:{}", s["activeFile"].as_str().unwrap_or(""), s["bankHash"].as_str().unwrap_or(""))).collect();
        for (key, prior) in self.additional_held.clone() {
            if !registered.contains(&key) {
                fresh = false;
                pending_unknown = true;
                for (id, m) in prior { self.held.insert(id, m); }
            }
        }
        Ok(json!({"fresh": fresh, "pendingUnknown": pending_unknown,
            "bootstrap": bootstrap.values().cloned().collect::<Vec<_>>()}))
    }
    fn persist(&mut self) {
        let mut out = json!({"schemaVersion": 1});
        for (k, v) in self.status.as_object().cloned().unwrap_or_default() { out[k] = v; }
        out["updatedAt"] = json!(baal_service_runtime::contract::ms_to_iso((self.now)() as f64));
        out["source"] = self.screen.projection()["source"].clone();
        out["diagnostics"] = self.screen.diagnostics((self.now)(), &self.last_protected.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect::<Vec<_>>());
        out["universeHash"] = self.universe["hash"].clone();
        out["codeIdentity"] = self.identity.clone();
        out["executionAuthority"] = V::Bool(false);
        out["pid"] = json!(std::process::id());
        let _ = baal_service_runtime::atomic::atomic_json_canonical(&self.runtime_dir.join("median-discovery-status.json"), &out);
    }
    /// initialize()
    pub fn initialize(&mut self) {
        let r = (|| -> R<()> {
            need(self.allowed(), "STOPPED")?;
            if self.end > 0 {
                use std::io::Read;
                let mut fd = std::fs::File::open(&self.source).map_err(|e| MedianFault::raw(e.to_string()))?;
                let mut raw = Vec::new();
                fd.by_ref().take(self.end).read_to_end(&mut raw).map_err(|e| MedianFault::raw(e.to_string()))?;
                for chunk in raw.split_inclusive(|b| *b == b'\n') {
                    need(self.basic_allowed(), "STOPPED")?;
                    let line_bytes = if chunk.ends_with(b"\n") { &chunk[..chunk.len() - 1] } else { chunk };
                    need(line_bytes.len() <= 2 * 1024 * 1024, "SOURCE_ROW_BOUND")?;
                    let row: V = serde_json::from_slice(line_bytes).map_err(|e| MedianFault::raw(e.to_string()))?;
                    self.screen.accept(&row)?;
                }
            }
            need(self.allowed(), "STOPPED")?;
            need(self.screen.projection()["source"]["sequence"].as_i64() == self.head["sequence"].as_i64()
                && self.screen.projection()["source"]["hash"] == self.head["hash"], "BOOTSTRAP_HEAD")?;
            for row in std::mem::take(&mut self.queue) {
                self.screen.accept(&row)?;
            }
            self.ready = true;
            self.status = json!({"status": "ready", "executionAuthority": false});
            self.persist();
            Ok(())
        })();
        if let Err(e) = r {
            self.failed = true;
            self.status = json!({"status": "unavailable", "reason": median_fault_filter(&e, "BOOTSTRAP_FAILED"),
                "executionAuthority": false});
            self.persist();
        }
    }
    /// onPublished(row)
    pub fn on_published(&mut self, row: &V) {
        if self.failed { return; }
        let r = (|| -> R<()> {
            need(self.allowed(), "STOPPED")?;
            if !self.ready {
                need(self.queue.len() < 32, "BOOTSTRAP_QUEUE")?;
                self.queue.push(row.clone());
                return Ok(());
            }
            self.screen.accept(row)?;
            Ok(())
        })();
        if r.is_err() {
            self.failed = true;
            self.status = json!({"status": "unavailable", "reason": "MEDIAN_DISCOVERY_SOURCE_FAILED",
                "executionAuthority": false});
            self.persist();
        }
    }
    /// status()
    pub fn status(&self) -> V {
        let mut out = self.status.clone();
        out["diagnostics"] = self.screen.diagnostics((self.now)(), &self.last_protected.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect::<Vec<_>>());
        out
    }
    /// afterProtected({cycle, protectedMarketIds, successfulProtected})
    pub fn after_protected(&mut self, cycle: i64, protected_market_ids: &[String], successful_protected: i64) -> R<()> {
        if !self.ready || self.failed || !self.allowed() || successful_protected != 4
            || protected_market_ids.len() != 4
            || protected_market_ids.iter().collect::<BTreeSet<_>>().len() != 4
            || !protected_market_ids.iter().all(|id| {
                self.universe["exchanges"].as_array().cloned().unwrap_or_default().iter()
                    .any(|x| x["marketId"].as_str() == Some(id.as_str()))
            })
        {
            return Ok(());
        }
        self.last_protected = protected_market_ids.iter().map(|s| json!(s)).collect();
        let watched_at = (self.now)();
        self.record(&json!({"kind": "protected_receipt_watch", "marketIds": protected_market_ids,
            "at": baal_service_runtime::contract::ms_to_iso(watched_at as f64), "executionAuthority": false}))?;
        for market_id in protected_market_ids {
            let member = self.universe["exchanges"].as_array().cloned().unwrap_or_default()
                .into_iter().find(|x| x["marketId"].as_str() == Some(market_id.as_str()));
            need(member.is_some(), "PROTECTED_SCOPE")?;
            let member = member.unwrap();
            self.watch.insert(market_id.clone(), json!({"marketId": market_id,
                "exchangeId": member["exchangeId"], "until": watched_at as f64 + 150000.0,
                "evidence": {"kind": "protected_receipt_watch"}}));
        }
        if (self.now)() - self.started < 60000 || cycle % 3 != 0 || !(self.headroom)(4) {
            return Ok(());
        }
        let risk = self.risks()?;
        let pending_unknown = risk["pendingUnknown"].as_bool().unwrap_or(true);
        let risk_fresh = risk["fresh"].as_bool().unwrap_or(false);
        let mut risk_rows: indexmap::IndexMap<String, V> = Default::default();
        for x in self.watch.values() {
            if x["until"].as_f64().unwrap_or(0.0) >= (self.now)() as f64 || pending_unknown {
                risk_rows.insert(x["marketId"].as_str().unwrap_or("").to_string(),
                    json!({"marketId": x["marketId"], "exchangeId": x["exchangeId"],
                        "evidence": x["evidence"], "kind": "retained_receipt_watch"}));
            }
        }
        for (id, x) in self.held.clone() {
            let mut v = x.clone();
            v["kind"] = json!("attributed_paper_risk");
            risk_rows.insert(id, v);
        }
        let mut priority: Vec<V> = risk_rows.values().cloned()
            .filter(|x| !protected_market_ids.iter().any(|p| Some(p.as_str()) == x["marketId"].as_str()))
            .collect();
        priority.sort_by(|a, b| {
            (if a["kind"].as_str() == Some("attributed_paper_risk") { 0 } else { 1 })
                .cmp(&(if b["kind"].as_str() == Some("attributed_paper_risk") { 0 } else { 1 }))
                .then(self.last_attempt.get(a["marketId"].as_str().unwrap_or("")).copied().unwrap_or(0.0)
                    .partial_cmp(&self.last_attempt.get(b["marketId"].as_str().unwrap_or("")).copied().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal))
                .then(a["marketId"].as_str().unwrap_or("").cmp(b["marketId"].as_str().unwrap_or("")))
        });
        let account = (self.get_account)();
        let excluded: Vec<String> = account["snapshot"]["excludedExchangeIds"].as_array().cloned().unwrap_or_default()
            .iter().filter_map(|x| x.as_str().map(String::from)).collect();
        let new_rows: Vec<V> = if risk_fresh && !pending_unknown && priority.len() < 2 {
            self.screen.projection()["signals"].as_array().cloned().unwrap_or_default().into_iter()
                .filter(|s| {
                    !protected_market_ids.iter().any(|p| Some(p.as_str()) == s["marketId"].as_str())
                        && !risk_rows.contains_key(s["marketId"].as_str().unwrap_or(""))
                        && !excluded.iter().any(|e| Some(e.as_str()) == s["exchangeId"].as_str())
                        && (self.now)() as f64 >= s["observedAt"].as_f64().unwrap_or(f64::NAN)
                        && (self.now)() as f64 - s["observedAt"].as_f64().unwrap_or(f64::NAN) <= 20000.0
                })
                .collect()
        } else { Vec::new() };
        let mut bootstrap_rows: Vec<V> = if risk_fresh && !pending_unknown {
            risk["bootstrap"].as_array().cloned().unwrap_or_default().into_iter()
                .filter(|m| {
                    !protected_market_ids.iter().any(|p| Some(p.as_str()) == m["marketId"].as_str())
                        && !risk_rows.contains_key(m["marketId"].as_str().unwrap_or(""))
                        && !excluded.iter().any(|e| Some(e.as_str()) == m["exchangeId"].as_str())
                })
                .collect()
        } else { Vec::new() };
        bootstrap_rows.sort_by(|a, b| {
            self.last_attempt.get(a["marketId"].as_str().unwrap_or("")).copied().unwrap_or(0.0)
                .partial_cmp(&self.last_attempt.get(b["marketId"].as_str().unwrap_or("")).copied().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)
                .then(a["marketId"].as_str().unwrap_or("").cmp(b["marketId"].as_str().unwrap_or("")))
        });
        let mut selected: indexmap::IndexMap<String, V> = Default::default();
        for m in priority.into_iter().chain(bootstrap_rows).chain(new_rows) {
            if !selected.contains_key(m["marketId"].as_str().unwrap_or("")) {
                selected.insert(m["marketId"].as_str().unwrap_or("").to_string(), m);
            }
        }
        let signals: Vec<V> = selected.into_values().take(2).collect();
        if signals.is_empty() { return Ok(()); }
        if !account_fresh(&(self.get_account)(), (self.now)() as f64 + 10000.0) {
            (self.refresh_account)()?;
        }
        for candidate in signals {
            let kind_present = !candidate["kind"].is_null();
            if !self.allowed() || !(self.headroom)(1)
                || (!kind_present && (self.now)() as f64 - candidate["observedAt"].as_f64().unwrap_or(f64::NAN) > 20000.0)
            {
                continue;
            }
            let mut official_prepared = false;
            if let Some(before) = &self.before_candidate_read {
                match before(candidate["marketId"].as_str().unwrap_or("")) {
                    Ok(v) => official_prepared = v == V::Bool(true),
                    Err(e) => {
                        if e.durable_recording_failed || e.fatal { return Err(e); }
                    }
                }
            }
            if !self.allowed() || !(self.headroom)(1) { continue; }
            if !account_fresh(&(self.get_account)(), (self.now)() as f64 + 4000.0) {
                if !(self.headroom)(4) { continue; }
                (self.refresh_account)()?;
            }
            if !account_fresh(&(self.get_account)(), (self.now)() as f64 + 4000.0) { continue; }
            let account = (self.get_account)().clone();
            let bootstrap = candidate["kind"].as_str() == Some("research_bootstrap_read");
            if (!kind_present || bootstrap)
                && account["snapshot"]["excludedExchangeIds"].as_array().cloned().unwrap_or_default()
                    .iter().any(|x| x.as_str() == candidate["exchangeId"].as_str())
            {
                continue;
            }
            if bootstrap {
                let current = read_additional_probability_risk(&self.runtime_dir.clone(), &self.universe.clone(),
                    &candidate["source"].clone(), (self.now)(), &self.live_paper_dir.clone()).ok();
                let ok = current.as_ref().map(|c| {
                    c["fresh"].as_bool().unwrap_or(false) && !js_truthy(c.get("unidentifiedPending"))
                        && c["bootstrap"].as_array().cloned().unwrap_or_default().iter().any(|m| {
                            m["marketId"] == candidate["marketId"]
                                && m["evidence"]["researchHash"] == candidate["evidence"]["researchHash"]
                        })
                }).unwrap_or(false);
                if !ok { continue; }
                self.record(&json!({"kind": "research_bootstrap_read", "candidate": candidate,
                    "accountEligibility": account, "at": baal_service_runtime::contract::ms_to_iso((self.now)() as f64),
                    "unowned": true, "executionAuthority": false}))?;
            }
            let market_id = candidate["marketId"].as_str().unwrap_or("").to_string();
            self.last_attempt.insert(market_id.clone(), (self.now)() as f64);
            if !kind_present || bootstrap {
                self.watch.insert(market_id.clone(), json!({"marketId": candidate["marketId"],
                    "exchangeId": candidate["exchangeId"], "until": (self.now)() as f64 + 150000.0,
                    "evidence": candidate}));
            }
            self.record(&json!({"kind": "candidate_read_intent", "candidate": candidate,
                "protectedMarketIds": protected_market_ids, "accountEligibility": account,
                "at": baal_service_runtime::contract::ms_to_iso((self.now)() as f64), "executionAuthority": false}))?;
            let endpoint = format!("/markets/{}/orderbook?tournamentId={}&depth=20",
                market_id, self.universe["tournamentId"].as_str().unwrap_or(""));
            let receipt = match (self.request)(&endpoint) {
                Ok(r) => r,
                Err(e) => {
                    self.record(&json!({"kind": "candidate_read_failure", "marketId": market_id,
                        "status": e.status.map(|s| json!(s)).unwrap_or(V::Null),
                        "at": baal_service_runtime::contract::ms_to_iso((self.now)() as f64),
                        "executionAuthority": false}))?;
                    if e.durable_recording_failed || e.fatal || e.status == Some(401) || e.status == Some(403) {
                        return Err(e);
                    }
                    if e.status == Some(429) || e.status == Some(503) { break; }
                    continue;
                }
            };
            let request_at = parse_at(&receipt["requestStartedAt"]);
            let received_at = parse_at(&receipt["receivedAt"]);
            let available = (self.now)() as f64;
            let causal = account_fresh(&account, request_at)
                && parse_at(&account["availableAt"]) <= request_at
                && received_at >= request_at && available >= received_at
                && available - received_at <= 20000.0
                && received_at - request_at <= 20000.0
                && account_fresh(&account, available)
                && (kind_present || available - candidate["observedAt"].as_f64().unwrap_or(f64::NAN) <= 20000.0)
                && (!bootstrap || (parse_at(&candidate["evidence"]["sourceAt"]) <= request_at
                    && parse_at(&candidate["evidence"]["recordedAt"]) <= request_at
                    && available < parse_at(&candidate["evidence"]["expiresAt"])
                    && available - parse_at(&candidate["evidence"]["sourceAt"]) <= candidate["evidence"]["maximumResearchAgeMs"].as_f64().unwrap_or(0.0)));
            let mut published = false;
            if causal {
                published = (self.publish_book)(&market_id, &receipt) != V::Bool(false);
            }
            self.record(&json!({"kind": "candidate_read_receipt", "marketId": market_id,
                "requestStartedAt": receipt["requestStartedAt"], "receivedAt": receipt["receivedAt"],
                "availableAt": baal_service_runtime::contract::ms_to_iso(available),
                "causal": causal, "published": published, "sourceEvidence": candidate,
                "executionAuthority": false}))?;
            if causal && published && official_prepared {
                if let Some(after) = &self.after_candidate_receipt {
                    after(&market_id, &receipt)?;
                }
            }
        }
        self.status = json!({"status": "ready", "lastCycle": cycle, "protectedMarketIds": protected_market_ids,
            "source": self.screen.projection()["source"], "executionAuthority": false});
        self.persist();
        Ok(())
    }
}
