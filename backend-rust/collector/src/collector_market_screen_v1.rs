// live-paper/collector-market-screen-v1.mjs — collector-local minute screen host.
// Collector-local screening only. Bulk quotes never become executable fills.
use baal_execution::core::payload_hash;
use baal_service_runtime::json_text::digest;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("COLLECTOR_MARKET_SCREEN_{}", code)); } Ok(()) }
fn parse_at(v: &V) -> f64 { baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN) }
fn screen_error(e: &str, fallback: &str) -> String {
    if e.starts_with("COLLECTOR_MARKET_SCREEN_") && e.chars().all(|c| c.is_ascii_uppercase() || c == '_') { e.to_string() } else { format!("COLLECTOR_MARKET_SCREEN_{}", fallback) }
}
// The original minute model is retained as the independent test oracle. Quotes
// arrive in monotonically ordered receipt time, so prune only the expired prefix
// instead of parsing every retained timestamp on every update.
/// observeCollectorMarketHistory(state, snapshot, universe)
pub fn observe_collector_market_history(state: &V, snapshot: &V, universe: &V) -> Result<V, String> {
    crate::market_price_publication::validate_price_observation_pub(snapshot, universe)?;
    let now = parse_at(&snapshot["receivedAt"]);
    let mut s = state.clone();
    if s["universeHash"] != universe["hash"]
        || (!s["lastAt"].is_null() && now < parse_at(&s["lastAt"]))
    {
        return Err("MARKET_HISTORY_SCOPE_OR_CLOCK".into());
    }
    for id in snapshot["missingIds"].as_array().cloned().unwrap_or_default() {
        let key = id.as_str().unwrap_or("");
        if !s["minutes"][key].is_null() {
            s["minutes"][key]["unavailableAt"] = snapshot["receivedAt"].clone();
        }
    }
    let window = crate::market_history::market_history_policy()["windowMs"].as_f64().unwrap_or(0.0);
    for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
        let eid = q["exchangeId"].as_str().unwrap_or("").to_string();
        let mut series = if s["minutes"][&eid].is_null() {
            json!({"marketId": q["marketId"], "buckets": [], "latest": V::Null, "unavailableAt": V::Null})
        } else {
            s["minutes"][&eid].clone()
        };
        if q["bestBid"].is_null() || q["bestAsk"].is_null()
            || baal_service_runtime::json_text::js_truthy(q.get("crossed"))
            || q["bestBid"].as_f64().unwrap_or(0.0) <= 0.0
            || q["bestAsk"].as_f64().unwrap_or(1.0) >= 1.0
        {
            series["unavailableAt"] = snapshot["receivedAt"].clone();
            s["minutes"][&eid] = series;
            continue;
        }
        let point = json!({"bucket": (now / 60000.0).floor(), "at": snapshot["receivedAt"], "bid": q["bestBid"], "ask": q["bestAsk"]});
        let mut buckets = series["buckets"].as_array().cloned().unwrap_or_default();
        if buckets.last().map(|b| b["bucket"].as_f64() == point["bucket"].as_f64()).unwrap_or(false) {
            *buckets.last_mut().unwrap() = point.clone();
        } else {
            buckets.push(point.clone());
        }
        let mut expired = 0usize;
        while expired < buckets.len() && parse_at(&buckets[expired]["at"]) < now - window { expired += 1; }
        if expired > 0 { buckets.drain(..expired); }
        series["buckets"] = json!(buckets);
        series["latest"] = point;
        series["unavailableAt"] = V::Null;
        s["minutes"][&eid] = series;
    }
    s["lastAt"] = snapshot["receivedAt"].clone();
    s["observations"] = json!(s["observations"].as_i64().unwrap_or(0) + 1);
    Ok(s)
}
/// collectorMarketScreenIdentity(workspace) — sha256 per frozen file under the repo root.
pub fn collector_market_screen_identity(workspace: &Path) -> Result<V, String> {
    let mut map = serde_json::Map::new();
    for f in ["live-paper/collector-market-screen-v1.mjs", "live-paper/market-history.mjs",
        "live-paper/market-price-publication.mjs", "execution/ledger.mjs"] {
        let bytes = std::fs::read(workspace.join(f)).map_err(|e| e.to_string())?;
        map.insert(f.to_string(), json!(baal_service_runtime::json_text::sha256(&bytes)));
    }
    Ok(V::Object(map))
}
/// lastRecord(file, end)
fn last_record(file: &Path, end: u64) -> Result<V, String> {
    let n = end.min(2 * 1024 * 1024) as usize;
    use std::io::{Read, Seek, SeekFrom};
    let mut fd = std::fs::File::open(file).map_err(|e| e.to_string())?;
    fd.seek(SeekFrom::Start(end - n as u64)).map_err(|e| e.to_string())?;
    let mut bytes = vec![0u8; n];
    fd.read_exact(&mut bytes).map_err(|_| screen_error("", "SOURCE_READ"))?;
    need(*bytes.last().unwrap_or(&0) == b'\n', "SOURCE_BOUNDARY")?;
    let begin = bytes[..n - 1].iter().rposition(|b| *b == b'\n').map(|p| p + 1).unwrap_or(0);
    need(begin > 0 || end == n as u64, "RECORD_TOO_LARGE")?;
    serde_json::from_slice(&bytes[begin..n - 1]).map_err(|_| screen_error("", "SOURCE_JSON"))
}
/// openCollectorMarketScreen({runtimeDir, universe, publisher, signal, canOperate, now})
/// `publisher` is a `Fn() -> V` standing in for publisher.status(); `signal` an
/// optional aborted flag getter.
pub struct CollectorMarketScreen {
    file: PathBuf,
    status_file: PathBuf,
    runtime_dir: PathBuf,
    universe: V,
    publisher: Arc<dyn Fn() -> V + Send + Sync>,
    signal: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    workspace: PathBuf,
    identity: V,
    source_ino: u64,
    source_dev: u64,
    end: u64,
    head: V,
    prior: Option<V>,
    state: V,
    sequence: i64,
    previous: String,
    offset: u64,
    ready: bool,
    closed: bool,
    error: Option<String>,
    bootstrap_ms: Option<f64>,
    queue: Vec<(V, u64)>,
    prior_matched: bool,
}
impl CollectorMarketScreen {
    pub fn open(runtime_dir: &Path, universe: &V, workspace: &Path,
        publisher: Arc<dyn Fn() -> V + Send + Sync>,
        signal: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
        can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
        now: Arc<dyn Fn() -> i64 + Send + Sync>,
    ) -> Result<Self, String> {
        let file = runtime_dir.join("market-prices.jsonl");
        let status_file = runtime_dir.join("collector-market-screen-v1.json");
        let identity = collector_market_screen_identity(workspace)?;
        let stat = std::fs::metadata(&file).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        let (ino, dev) = (std::os::unix::fs::MetadataExt::ino(&stat), std::os::unix::fs::MetadataExt::dev(&stat));
        #[cfg(not(unix))]
        let (ino, dev) = (stat.len(), 0u64);
        let end = stat.len();
        let head = (publisher)();
        let prior: Option<V> = if status_file.exists() {
            Some(serde_json::from_slice(&std::fs::read(&status_file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?)
        } else { None };
        if let Some(prior) = &prior {
            let src = &prior["source"];
            need(prior["schemaVersion"].as_i64() == Some(1)
                && prior["mode"].as_str() == Some("collector_verified_minute_screen_v1")
                && prior["universeHash"] == universe["hash"]
                && baal_service_runtime::json_text::js_int(&src["bytes"]).is_some_and(|b| b >= 0 && b as u64 <= end)
                && baal_service_runtime::json_text::js_int(&src["sequence"]).is_some_and(|s| s >= 0)
                && (if src["bytes"].as_i64() == Some(0) {
                    src["sequence"].as_i64() == Some(0) && src["hash"].as_str() == Some("")
                } else {
                    src["sequence"].as_i64().is_some_and(|s| s > 0)
                        && src["hash"].as_str().is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
                }), "PRIOR_CHECKPOINT")?;
        }
        let prior_matched = prior.is_none() || prior.as_ref().unwrap()["source"]["bytes"].as_i64() == Some(0);
        Ok(Self {
            file, status_file, runtime_dir: runtime_dir.to_path_buf(), universe: universe.clone(),
            publisher, signal, can_operate, now, workspace: workspace.to_path_buf(),
            identity, source_ino: ino, source_dev: dev, end, head, prior,
            state: crate::market_history::create_market_history(universe),
            sequence: 0, previous: String::new(), offset: 0,
            ready: false, closed: false, error: None, bootstrap_ms: None,
            queue: Vec::new(), prior_matched,
        })
    }
    fn permitted(&self) -> bool {
        !self.closed
            && !self.signal.as_ref().map(|s| s()).unwrap_or(false)
            && (self.can_operate)()
            && !self.runtime_dir.join("MARKET_HISTORY_STOP").exists()
    }
    fn current_source(&self) -> Result<(), String> {
        let s = std::fs::metadata(&self.file).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        let (ino, dev) = (std::os::unix::fs::MetadataExt::ino(&s), std::os::unix::fs::MetadataExt::dev(&s));
        #[cfg(not(unix))]
        let (ino, dev) = (s.len(), 0u64);
        need(ino == self.source_ino && dev == self.source_dev && s.len() >= self.offset, "SOURCE_REPLACED")
    }
    fn accept(&mut self, row: &V, bytes: u64) -> Result<(), String> {
        let receipt_hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        body.as_object_mut().and_then(|m| m.shift_remove("hash"));
        need(row["sequence"].as_i64() == Some(self.sequence + 1)
            && row["previous"].as_str().unwrap_or("") == self.previous
            && receipt_hash == digest(&body)
            && row["state"].is_null()
            && row["event"]["type"].as_str() == Some("market_price_observation"), "SOURCE_CHAIN")?;
        self.state = observe_collector_market_history(&self.state, &row["event"]["snapshot"], &self.universe)?;
        self.sequence = row["sequence"].as_i64().unwrap_or(0);
        self.previous = receipt_hash;
        self.offset += bytes;
        if let Some(prior) = &self.prior {
            if self.offset == prior["source"]["bytes"].as_u64().unwrap_or(u64::MAX) {
                need(self.sequence == prior["source"]["sequence"].as_i64().unwrap_or(-1)
                    && self.previous == prior["source"]["hash"].as_str().unwrap_or(""), "PRIOR_SOURCE_CHANGED")?;
                self.prior_matched = true;
            }
        }
        Ok(())
    }
    /// projection()
    pub fn projection(&mut self) -> V {
        let stopped = json!({"available": false, "status": "stopped", "reason": "SCREEN_STOPPED",
            "candidateMarketIds": [], "executionAuthority": false});
        let pending = |reason: &str| json!({"available": false, "status": if reason == "VERIFIED_BOOTSTRAP_PENDING" { "loading" } else { "unavailable" },
            "reason": reason, "candidateMarketIds": [], "executionAuthority": false});
        match (|| -> Result<V, String> {
            self.current_source()?;
            need(payload_hash(&self.identity)? == payload_hash(&collector_market_screen_identity(&self.workspace)?)?, "CODE_CHANGED")?;
            if !self.permitted() { return Ok(stopped); }
            if self.error.is_some() || !self.ready {
                return Ok(pending(self.error.as_deref().unwrap_or("VERIFIED_BOOTSTRAP_PENDING")));
            }
            let clock_at = baal_service_runtime::contract::ms_to_iso((self.now)() as f64);
            let ranked = crate::market_history::rank_market_history(&self.state, &self.universe, &clock_at, &crate::market_history::market_history_policy())?;
            Ok(json!({"available": true, "status": "running", "reason": "VERIFIED_COLLECTOR_MINUTE_SCREEN",
                "candidateMarketIds": ranked["leads"].as_array().cloned().unwrap_or_default().iter().map(|x| x["marketId"].clone()).collect::<Vec<_>>(),
                "counts": ranked["counts"], "leads": ranked["leads"], "policy": ranked["policy"],
                "source": {"bytes": self.offset, "sequence": self.sequence, "hash": self.previous},
                "sourceObservations": self.state["observations"], "bootstrapMs": self.bootstrap_ms,
                "executionAuthority": false, "networkRequests": 0}))
        })() {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(screen_error(&e, "INVALID"));
                json!({"available": false, "status": "unavailable", "reason": self.error,
                    "candidateMarketIds": [], "executionAuthority": false})
            }
        }
    }
    /// publish()
    fn publish(&mut self) {
        if !self.permitted() { return; }
        let p = self.projection();
        let source = if self.error.is_some() && self.prior.is_some() {
            self.prior.as_ref().unwrap()["source"].clone()
        } else {
            json!({"bytes": self.offset, "sequence": self.sequence, "hash": self.previous})
        };
        let mut out = json!({"schemaVersion": 1, "mode": "collector_verified_minute_screen_v1",
            "universeHash": self.universe["hash"], "codeIdentity": self.identity,
            "pid": std::process::id(),
            "updatedAt": baal_service_runtime::contract::ms_to_iso((self.now)() as f64)});
        for (k, v) in p.as_object().unwrap() { out[k] = v.clone(); }
        out["source"] = source;
        let _ = baal_service_runtime::atomic::atomic_json_canonical(&self.status_file, &out);
    }
    /// initialize()
    pub fn initialize(&mut self) -> V {
        let started = std::time::Instant::now();
        let r = (|| -> Result<(), String> {
            need(self.permitted(), "STOPPED")?;
            if self.end > 0 {
                use std::io::Read;
                let mut fd = std::fs::File::open(&self.file).map_err(|e| e.to_string())?;
                let mut raw = Vec::new();
                fd.by_ref().take(self.end).read_to_end(&mut raw).map_err(|e| e.to_string())?;
                for chunk in raw.split_inclusive(|b| *b == b'\n') {
                    need(self.permitted(), "STOPPED")?;
                    let line_bytes = if chunk.ends_with(b"\n") { &chunk[..chunk.len() - 1] } else { chunk };
                    need(line_bytes.len() <= 2 * 1024 * 1024, "RECORD_TOO_LARGE")?;
                    let line = std::str::from_utf8(line_bytes).map_err(|_| screen_error("", "SOURCE_JSON"))?;
                    let row: V = serde_json::from_str(line).map_err(|_| screen_error("", "SOURCE_JSON"))?;
                    self.accept(&row, line_bytes.len() as u64 + 1)?;
                }
            }
            need(self.offset == self.end
                && self.sequence == self.head["sequence"].as_i64().unwrap_or(-1)
                && self.previous == self.head["hash"].as_str().unwrap_or(""), "BOOTSTRAP_HEAD")?;
            need(self.prior_matched, "PRIOR_BOUNDARY")?;
            self.current_source()?;
            for item in std::mem::take(&mut self.queue) {
                need(item.1 > self.offset, "QUEUED_SOURCE_REGRESSION")?;
                let bytes = item.1 - self.offset;
                self.accept(&item.0, bytes)?;
            }
            self.ready = true;
            self.bootstrap_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
            self.publish();
            Ok(())
        })();
        if let Err(e) = r {
            self.error = Some(screen_error(&e, "BOOTSTRAP_FAILED"));
            self.queue = Vec::new();
            self.publish();
        }
        self.projection()
    }
    /// onPublished(snapshot)
    pub fn on_published(&mut self, snapshot: &V) {
        if !self.permitted() || self.error.is_some() { return; }
        let r = (|| -> Result<(), String> {
            self.current_source()?;
            let bytes = std::fs::metadata(&self.file).map_err(|e| e.to_string())?.len();
            let row = last_record(&self.file, bytes)?;
            let published_head = (self.publisher)();
            need(row["sequence"] == published_head["sequence"]
                && row["hash"] == published_head["hash"]
                && payload_hash(&row["event"]["snapshot"])? == payload_hash(snapshot)?, "PUBLISHED_BINDING")?;
            if !self.ready {
                let last = self.queue.last().map(|(r, _)| r.clone()).unwrap_or_else(|| self.head.clone());
                need(self.queue.len() < 32
                    && row["sequence"].as_i64() == Some(last["sequence"].as_i64().unwrap_or(0) + 1)
                    && row["previous"] == last["hash"], "BOOTSTRAP_QUEUE_LIMIT")?;
                self.queue.push((row, bytes));
                return Ok(());
            }
            let n = bytes - self.offset;
            self.accept(&row, n)?;
            self.publish();
            Ok(())
        })();
        if let Err(e) = r {
            self.error = Some(screen_error(&e, "APPEND_FAILED"));
            self.publish();
        }
    }
    /// close()
    pub fn close(&mut self) { self.closed = true; }
}
