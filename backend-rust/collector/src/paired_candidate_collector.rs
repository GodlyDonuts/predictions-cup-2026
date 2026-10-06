// live-paper/paired-candidate-collector.mjs — requested paired-candidate screen runtime.
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::client::ServiceError;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_truthy};
use serde_json::{json, Value as V};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn svc(message: impl Into<String>) -> ServiceError {
    ServiceError { message: message.into(), status: None, api_code: None, retry_after_ms: 0.0, recoverable: false, stopped: false }
}
fn need(ok: bool, code: &str) -> Result<(), ServiceError> { if !ok { return Err(svc(format!("PAIRED_COLLECTOR_{}", code))); } Ok(()) }
fn cohort_ok(name: &V) -> bool {
    name.as_str().is_some_and(|s| {
        let b = s.as_bytes();
        b.len() >= 2 && b.len() <= 64 && b[0].is_ascii_lowercase() && b[1..].iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
    })
}
fn terminal(phase: &V) -> bool { ["complete", "failed", "interrupted"].contains(&phase.as_str().unwrap_or("")) }
fn fatal(message: &str) -> ServiceError {
    // JS sets e.durableRecordingFailed=true and e.fatal=true on these errors.
    ServiceError { message: message.into(), status: None, api_code: None, retry_after_ms: 0.0, recoverable: false, stopped: true }
}

// A single requested public pair screen replaces both auxiliary reads. It
// never publishes an ordinary book, supplies a wallet or owns an API client.
pub struct PairedCandidateCollector {
    request_file: PathBuf,
    status_file: PathBuf,
    universe: V,
    dispatch: Arc<dyn Fn(&V) -> Result<V, ServiceError> + Send + Sync>,
    reserve: Arc<dyn Fn(i64) -> Result<V, String> + Send + Sync>,
    headroom: Arc<dyn Fn(&V) -> bool + Send + Sync>,
    get_account_eligibility: Arc<dyn Fn() -> V + Send + Sync>,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
    signal: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    capture: Option<crate::paired_candidate_capture::PairedCandidateCapture>,
    binding: Option<String>,
    prior: V,
    history: Journal,
    handled: HashSet<String>,
    startup: i64,
}
/// openPairedCandidateCollector({runtimeDir,universe,dispatch,reserve,headroom,getAccountEligibility,canOperate,signal,now})
impl PairedCandidateCollector {
    pub fn open(runtime_dir: &Path, universe: &V,
        dispatch: Arc<dyn Fn(&V) -> Result<V, ServiceError> + Send + Sync>,
        reserve: Arc<dyn Fn(i64) -> Result<V, String> + Send + Sync>,
        headroom: Arc<dyn Fn(&V) -> bool + Send + Sync>,
        get_account_eligibility: Arc<dyn Fn() -> V + Send + Sync>,
        can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
        signal: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
        now: Arc<dyn Fn() -> i64 + Send + Sync>) -> Result<Self, ServiceError> {
        let request_file = runtime_dir.join("paired-candidate-request.json");
        let status_file = runtime_dir.join("paired-candidate-status.json");
        let mut prior = if status_file.exists() {
            serde_json::from_str(&std::fs::read_to_string(&status_file).map_err(|e| svc(e.to_string()))?).map_err(|e| svc(e.to_string()))?
        } else { V::Null };
        let history = Journal::open(&runtime_dir.join("paired-candidate-history.jsonl")).map_err(svc)?;
        let mut handled = HashSet::new();
        if history.file.exists() {
            for line in std::fs::read_to_string(&history.file).map_err(|e| svc(e.to_string()))?.trim().split('\n').filter(|l| !l.is_empty()) {
                let row: V = serde_json::from_str(line).map_err(|e| svc(e.to_string()))?;
                need(row["event"]["kind"].as_str() == Some("terminal_candidate") && row["event"]["name"].is_string(), "HISTORY")?;
                handled.insert(row["event"]["name"].as_str().unwrap().to_string());
            }
        }
        let startup = (now)();
        if js_truthy(Some(&prior)) && !terminal(&prior["phase"]) {
            let mut p = if prior.is_object() { prior.clone() } else { json!({}) };
            p["phase"] = json!("interrupted");
            p["failureCode"] = json!("RESTART_REQUIRES_NEW_REQUEST_NAME");
            p["at"] = json!(ms_to_iso((now)() as f64));
            prior = p;
            atomic_json_canonical(&status_file, &prior).map_err(svc)?;
        }
        let mut collector = PairedCandidateCollector { request_file, status_file, universe: universe.clone(),
            dispatch, reserve, headroom, get_account_eligibility, can_operate, signal, now,
            capture: None, binding: None, prior, history, handled, startup };
        collector.archive_terminal()?;
        Ok(collector)
    }
    fn allowed(&self) -> bool { (self.can_operate)() && !self.signal.as_ref().map(|s| s()).unwrap_or(false) }
    fn archive_terminal(&mut self) -> Result<(), ServiceError> {
        if js_truthy(Some(&self.prior)) && terminal(&self.prior["phase"]) && !self.handled.contains(self.prior["name"].as_str().unwrap_or("")) {
            let event = json!({"kind":"terminal_candidate","name":self.prior["name"].clone(),
                "requestHash":if js_truthy(Some(&self.prior["requestHash"])) { self.prior["requestHash"].clone() } else { V::Null },
                "result":self.prior.clone()});
            match self.history.commit(&V::Null, &event) {
                Ok(_) => { self.handled.insert(self.prior["name"].as_str().unwrap_or("").to_string()); }
                Err(_) => return Err(fatal("Paired candidate history recording failed")),
            }
        }
        Ok(())
    }
    /// pending()
    pub fn pending(&mut self) -> Result<V, ServiceError> {
        if !self.allowed() || !self.request_file.exists() { return Ok(V::Null); }
        let r: V = serde_json::from_str(&std::fs::read_to_string(&self.request_file).map_err(|e| svc(e.to_string()))?).map_err(|e| svc(e.to_string()))?;
        let exchanges = self.universe["exchanges"].as_array().cloned().unwrap_or_default();
        let market_ids = r["marketIds"].as_array().cloned().unwrap_or_default();
        need(r["schemaVersion"].as_i64() == Some(1) && r["owner"].as_str() == Some("Codex")
            && cohort_ok(&r["name"])
            && r["marketIds"].is_array() && market_ids.len() == 2
            && market_ids.iter().map(|x| baal_service_runtime::json_text::js_stringify(x)).collect::<BTreeSet<_>>().len() == 2
            && market_ids.iter().all(|id| exchanges.iter().any(|x| x["marketId"] == *id && x["option"].as_str() == Some("YES"))), "REQUEST")?;
        let start = parse_iso_ms(r["createdAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
        let end = parse_iso_ms(r["expiresAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
        let safe = |x: f64| x.is_finite() && x.fract() == 0.0 && x.abs() <= 9_007_199_254_740_991.0;
        need(safe(start) && safe(end) && end > start && end - start <= 1_800_000.0 && start <= (self.now)() as f64, "REQUEST_CLOCK")?;
        if (self.now)() as f64 >= end || self.handled.contains(r["name"].as_str().unwrap_or(""))
            || (self.prior["name"] == r["name"] && terminal(&self.prior["phase"])) { return Ok(V::Null); }
        if js_truthy(Some(&self.prior)) && terminal(&self.prior["phase"]) && self.prior["name"] != r["name"] {
            self.archive_terminal()?;
            self.capture = None;
            self.binding = None;
        }
        if let Some(b) = &self.binding { need(payload_hash(&r).map_err(svc)? == *b, "REQUEST_CHANGED")?; }
        Ok(r)
    }
    /// authorizes(spec)
    pub fn authorizes(&self, spec: &V) -> bool {
        self.allowed() && self.capture.as_ref().map(|c| c.authorizes(spec)) == Some(true)
    }
    /// status()
    pub fn status(&self) -> V {
        if js_truthy(Some(&self.prior)) { self.prior.clone() } else { json!({"phase":"idle","executionAuthority":false}) }
    }
    fn persist(&mut self) -> Result<(), ServiceError> {
        atomic_json_canonical(&self.status_file, &self.prior)
            .map_err(|_| fatal("Paired candidate evidence recording failed"))
    }
    /// afterAccountRefresh()
    pub fn after_account_refresh(&mut self) -> Result<bool, ServiceError> {
        let request = self.pending()?;
        if !js_truthy(Some(&request)) { return Ok(false); }
        // No incomplete timing history on startup can justify additional reads.
        if (self.now)() - self.startup < 60000
            || !(self.headroom)(&json!({"reads":2,"at":ms_to_iso((self.now)() as f64)})) { return Ok(false); }
        if self.capture.is_none() {
            self.binding = Some(payload_hash(&request).map_err(svc)?);
            let expires = parse_iso_ms(request["expiresAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
            let co = self.can_operate.clone();
            let sig = self.signal.clone();
            let now_c = self.now.clone();
            let capture_operate: Arc<dyn Fn() -> bool + Send + Sync> =
                Arc::new(move || co() && !sig.as_ref().map(|s| s()).unwrap_or(false) && (now_c)() as f64 <= expires - 1.0);
            let cap = crate::paired_candidate_capture::PairedCandidateCapture::new(&self.universe,
                self.dispatch.clone(), self.headroom.clone(), self.get_account_eligibility.clone(),
                capture_operate, self.signal.clone(), self.now.clone())?;
            self.capture = Some(cap);
            self.capture.as_mut().unwrap().begin(&request)?;
        }
        let allocation = (self.reserve)(2).map_err(|_| fatal("Paired candidate reservation recording failed"))?;
        need(allocation["totalReads"].as_i64() == Some(2) && js_int(&allocation["reservedOrdinal"]).is_some(), "DURABLE_TWO_READ_RESERVATION")?;
        let issued_at = ms_to_iso((self.now)() as f64);
        let grant = json!({"kind":"auxiliary_candidate_reads","sharedAccountBudget":true,"reservedReads":2,
            "reservationId":format!("{}:{}", request["name"].as_str().unwrap_or(""), allocation["reservedOrdinal"].as_i64().unwrap_or(0)),
            "issuedAt":issued_at,"expiresAt":ms_to_iso(((self.now)() + 10000) as f64)});
        let mut prior = self.capture.as_ref().unwrap().status();
        prior["name"] = request["name"].clone();
        prior["requestHash"] = json!(self.binding);
        prior["phase"] = json!("reserved");
        prior["reservation"] = grant.clone();
        prior["at"] = json!(issued_at);
        self.prior = prior;
        self.persist()?;
        let failure = self.capture.as_mut().unwrap().advance(&json!({"reservation":grant})).err();
        let mut prior = self.capture.as_ref().unwrap().status();
        prior["name"] = request["name"].clone();
        prior["requestHash"] = json!(self.binding);
        prior["reservation"] = grant.clone();
        prior["at"] = json!(ms_to_iso((self.now)() as f64));
        prior["ordinaryPublications"] = json!(0);
        prior["competitionOrders"] = json!(0);
        self.prior = prior;
        self.persist()?;
        self.archive_terminal()?;
        if let Some(f) = &failure {
            if f.stopped || f.status == Some(401) || f.status == Some(403) { return Err(failure.unwrap()); }
        }
        Ok(true)
    }
}
