// live-paper/runner.mjs — serialized paper collector: journal, admission-gated
// ReadClient, host fanout, book scheduler, dashboard, per-cycle collection loop.
// Ported as a synchronous run(): the whole CLI flow is a single serialized
// loop, matching the JS event loop's serialized admission model.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use serde_json::{json, Value as V};

use baal_service_runtime::atomic::{atomic_json, read_json as atomic_read_json};
use baal_service_runtime::client::{
    Authorizer, Clocks, DispatchSpec, Fetch, FetchOutcome, FetchRequest, ReadClient,
    ReadClientOptions,
};
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::exclusions::exclusion_set;
use baal_service_runtime::history::{allowed_history_endpoint, history_endpoint, normalize_history};
use baal_service_runtime::json_text::{digest, js_int, js_truthy, sha256};
use baal_service_runtime::journal::{journal_lines, read_verified_source, Journal, JournalReadOptions};
use baal_service_runtime::market::{account_fills, account_orders, account_pnl, account_positions};
use baal_service_runtime::client::ServiceError;

use baal_execution::controller::{
    CompetitionController, ControllerOptions,
};
use baal_execution::core::{canonical_payload, decimal_micros, format_micros, payload_hash};
use baal_execution::median::MedianPreflight;
use baal_execution::transport::OrderTransport;
use baal_service_runtime::ledger::IntentLedger;
use baal_execution::median_controller::MedianCompetitionController;
use baal_execution::median_controller_v2::MedianCompetitionControllerV2;
use baal_execution::median_controller_v2_clock::MedianCompetitionControllerV2Clock;
use baal_execution::production::validate_production_activation;
use baal_execution::receipts::{exact_money_fetch, MoneyReceiptOptions, MoneyReceiptWriter};
use baal_execution::settlement_controller::SettlementCompetitionController;
use baal_execution::transport::{self, OrderTransportError};
use baal_execution::paired_rest_controller::WireDispatchError;
use baal_execution::paired_live_host::{open_paired_live_host, PairedLiveApi, PairedLiveHost, PairedLiveHostDeps};
use baal_execution::paired_live_host_v2::{open_paired_live_host_v2, PairedLiveHostV2};
use baal_execution::paired_neutral_recovery_host_v1::{
    open_paired_neutral_recovery_host, PairedNeutralRecoveryDeps, PairedNeutralRecoveryHost,
};

use baal_feed::capture::Capture;

use crate::account::account_view;
use crate::account_eligibility_publication_v1::{
    create_account_eligibility_publication, AccountEligibilityPublication,
};

use crate::baal_read_attention::{create_baal_read_attention, BaalReadAttention};
use crate::basket_book_plan::plan_basket_books;
use crate::basket_followup_source::{create_basket_followup_source, BasketFollowupSource};
use crate::basket_inputs::basket_candidates;
use crate::basket_relations::BasketRelationshipReader;
use crate::basket_watch::basket_code_identity;
use crate::bid_anchor_read_focus::{prioritize_anchor_reads, read_anchor_read_focus};
use crate::book_scheduler::{
    create_scheduler_state, plan_book_reads, plan_history_reads, record_scheduler_attempts,
    validate_scheduler_state,
};
use crate::breadth::{normalize_prices, price_batch, price_endpoint, update_coverage, validate_universe};
use crate::bulk_dip_read_focus::{merge_paper_read_focus, read_bulk_read_focus};
use crate::canonical_capacity_guard::{guard_canonical_capacity, CanonicalCapacityGuard};
use crate::canonical_capacity_monitor::open_canonical_capacity_monitor;
use crate::collection_inputs::{collection_inputs, shock_collection_risk};
use crate::collector_journal_native::collector_native_journal_config;
use crate::collector_market_screen_v1::CollectorMarketScreen;
use crate::collector_publication_native::collector_native_publication_config;
use crate::collector_status_writer::write_collector_status;
use crate::book_publication_v2::BookPublicationWriter;
use crate::book_publication_v3::NativeBookPublicationV3;
use crate::collector_median_discovery_v3::{CollectorMedianDiscoveryV3, MedianFault};
use crate::paper_persistence::PaperPersistenceError;
use crate::settlement_paper_host::PaperHost;
use baal_service_runtime::client::ServiceError as ClientServiceError;
use crate::comparison_risk_source::{create_comparison_risk_source, ComparisonRiskSource};
use crate::comparison_sampling::{plan_comparison_sampling, record_comparison_attempt};
use crate::contract_evidence::ContractEvidenceReader;
use crate::coupled_bulk_read_focus::focus_coupled_bulk_reads;
use crate::coupled_drain::{drain_boundary_needed, drain_focus, drain_status};
use crate::coupled_maker_host_rust::CoupledBulkRustHost;
use crate::coupled_maker_host_v1::CoupledMakerHost;
use crate::coupled_maker_host_v2::CoupledBulkHost;
use crate::coupled_rust_transport_config::{
    coupled_bulk_startup_failure, read_coupled_rust_transport,
};
use crate::coverage_checkpoint::checkpoint_coverage;
use crate::cycle_wait::collection_cycle_wait;
use crate::depth_scope::validate_depth_plan;
use crate::diagnostic_read_focus::{diagnostic_depth_focus, open_diagnostic_focus, DiagnosticFocus};
use crate::dip_shadow::validate_dip_policy;
use crate::dual_no_pair_host_v1::DualNoPaperHost;
use crate::dual_no_pair_host_v2::DualNoPaperHostV2;
use crate::edge_observation_plan::{plan_edge_observation, read_edge_observation_focus};
use crate::edge_trial_projection::prioritize_edge_trials;
use crate::experiment_retirement::paper_experiment_retired;
use crate::expired_flat_paper_retirement::read_expired_flat_paper_retirement;
use crate::frozen_paper_pause_adapter_v1::{read_frozen_paper_pause, FrozenPaperPause};
use crate::frozen_paper_runtime::frozen_paper_module;
use crate::maker_read_plan::plan_maker_pair_reads;
use crate::market_history_inputs::read_market_history_inputs;
use crate::market_price_publication::MarketPricePublication;
use crate::market_sweep::{collect_market_sweep, load_market_sweep, SweepHooks};
use crate::model::{
    assert_state, initial_state, invalidate, normalize_book, observe, report, restore_state,
    sample_equity, validate_config,
};
use crate::native_dip_host_v1::{open_native_dip_host, NativeDipHost};
use crate::native_dip_host_v2::{open_native_dip_cache_host, NativeDipCacheHost};
use crate::native_dip_host_v3::{open_native_dip_intent_host, NativeDipIntentHost};
use crate::native_discovery_plan_v1::{plan_native_discovery_books, record_native_discovery_attempt};
use crate::native_journal_restore::{open_native_journal, NativeJournal};
use crate::native_quote_host_v1::{open_native_quote_host, NativeQuoteHost};
use crate::no_maker_host::{open_no_maker_host, NoMakerHost};
use crate::no_maker_host_v2::{open_no_maker_host_v2, NoMakerHostV2};
use crate::no_maker_host_v3::{open_no_maker_host_v3, NoMakerHostV3};
use crate::no_maker_host_v4::{open_no_maker_host_v4, NoMakerHostV4};
use crate::no_maker_host_v5::{open_no_maker_host_v5, NoMakerHostV5};
use crate::paired_candidate_collector::PairedCandidateCollector;
use crate::paired_package_read_plan::{focus_paired_package_reads, read_paired_package_focus};
use crate::paper_runtime::{lock as paper_lock, ordered_exclusion_set, PaperLock};
use crate::party_winner_pair_collector_v1::PartyWinnerPairCollector;
use crate::production_metadata::{open_production_metadata, ProductionMetadata};
use crate::production_read_priority::prioritize_production_reads;
use crate::profit_sprint_read_plan::{
    focus_profit_sprint_reads, passive_hosts_completely_flat, read_profit_sprint,
    scoped_risk_market_ids,
};
use crate::probability_read_focus::{merge_probability_read_focus, read_probability_read_focus};
use crate::probability_receipt::probability_account_eligibility;
use crate::read_timing::{timing_summary, validate_timing};
use crate::realtime_books::RealtimeBooks;
use crate::realtime_client::{CanOp, CreateClient, Dispatch, EventCb, NowFn, RealtimeClient, SdkOptions, TokenGate};

use crate::realtime_scope::realtime_capture_scope;
use crate::settlement_paper_collector::{open_collector_settlement_paper, SettlementCollectorHost};
use crate::settlement_portfolio_collector_v2::open_collector_settlement_portfolio_v2;
use crate::settlement_portfolio_host_v2::SettlementPortfolioHostV2;
use baal_execution::scoped_rest_receipts::ScopedRestReceipts;
use crate::stable_maker_host_v1::StableMakerHost;
use crate::stable_maker_read_focus::focus_stable_maker_reads;
use crate::supabase_ws::{create_supabase_client, SupabaseRealtime};
use crate::trade_tape::{create_trade_tape, TradeTape};
use crate::trade_tape_focus::trade_tape_focus;
use crate::transport_network_budget::{load_network_budget, network_interval};
use crate::worker_health::{baseline_view, worker_health};

// ---------------------------------------------------------------------------
// Small host/runtime helpers
// ---------------------------------------------------------------------------

/// Some ported consumers demand `Send + Sync` callback bounds even though this
/// runner invokes every callback synchronously on the collector thread. This
/// marker lets Rc-held host state satisfy those bounds; it is only ever used
/// for closures invoked on the calling thread inside a synchronous dispatch.
#[derive(Clone)]
struct AssertSend<T>(T);
impl<T> AssertSend<T> {
    /// Whole-struct capture helper: `x.get()` in a closure captures `x`
    /// (the Send/Sync wrapper) instead of the raw `x.0` field path.
    fn get(&self) -> &T { &self.0 }
}
unsafe impl<T> Send for AssertSend<T> {}
unsafe impl<T> Sync for AssertSend<T> {}

type R<T> = Result<T, RunnerError>;
fn raw(e: impl Into<String>) -> RunnerError { RunnerError::raw(e.into()) }

/// The thrown-error shape the JS runner inspects: message, optional HTTP
/// status/api_code, and the behavioural flags the catch sites read.
#[derive(Debug, Clone)]
pub struct RunnerError {
    pub message: String,
    pub status: Option<u16>,
    pub api_code: Option<String>,
    pub recoverable: bool,
    pub stopped: bool,
    pub durable_recording_failed: bool,
    pub fatal: bool,
    pub projection_invalid: bool,
    pub manual_review: bool,
    pub history_validation: Option<String>,
    /// fs-style error code (e.g. EACCES) for stop('..', e.code) provenance.
    pub code: Option<String>,
}

impl RunnerError {
    fn raw(message: impl Into<String>) -> Self {
        RunnerError {
            message: message.into(),
            status: None,
            api_code: None,
            recoverable: false,
            stopped: false,
            durable_recording_failed: false,
            fatal: false,
            projection_invalid: false,
            manual_review: false,
            history_validation: None,
            code: None,
        }
    }
    /// JS `const e=Error(msg);e.durableRecordingFailed=true;[e.fatal=true;]throw e`.
    fn durable(message: impl Into<String>, fatal: bool) -> Self {
        let mut e = RunnerError::raw(message);
        e.durable_recording_failed = true;
        e.fatal = fatal;
        e
    }
    fn msg(m: impl Into<String>) -> Self { RunnerError::raw(m) }
    fn from_svc(e: ServiceError) -> Self { RunnerError::from(e) }
}

impl From<String> for RunnerError {
    fn from(m: String) -> Self { RunnerError::raw(m) }
}
impl From<&str> for RunnerError {
    fn from(m: &str) -> Self { RunnerError::raw(m) }
}
impl From<std::io::Error> for RunnerError {
    fn from(e: std::io::Error) -> Self { RunnerError::raw(e.to_string()) }
}
impl From<ServiceError> for RunnerError {
    fn from(e: ServiceError) -> Self {
        RunnerError {
            message: e.message.clone(),
            status: e.status,
            api_code: e.api_code.clone(),
            recoverable: e.recoverable,
            stopped: e.stopped,
            durable_recording_failed: false,
            fatal: false,
            projection_invalid: false,
            manual_review: e.message == "Rate cooldown requires manual review",
            history_validation: None,
            code: None,
        }
    }
}
impl From<crate::collector_median_discovery_v3::MedianFault> for RunnerError {
    fn from(e: crate::collector_median_discovery_v3::MedianFault) -> Self {
        let mut x = RunnerError::raw(e.message);
        x.durable_recording_failed = e.durable_recording_failed;
        x.fatal = e.fatal;
        x.status = e.status.map(|s| s as u16);
        x
    }
}
impl From<OrderTransportError> for RunnerError {
    fn from(e: OrderTransportError) -> Self {
        RunnerError::raw(e.code.clone())
    }
}
impl From<crate::party_winner_pair_collector_v1::PairFault> for RunnerError {
    fn from(e: crate::party_winner_pair_collector_v1::PairFault) -> Self {
        RunnerError::raw(e.message)
    }
}


fn live_paper_root() -> PathBuf {
    if let Ok(v) = std::env::var("SIG_LIVE_PAPER_DIR") {
        return PathBuf::from(v);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for cand in [cwd.join("live-paper"), cwd.join("../live-paper"), cwd.join("../").join("live-paper")] {
        if cand.join("runner.mjs").exists() {
            return cand;
        }
    }
    cwd.join("live-paper")
}

fn null_v() -> V { V::Null }

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}
fn now_ms_i64() -> i64 { now_ms() as i64 }
fn iso_now() -> String { ms_to_iso(now_ms()) }

/// node:crypto randomUUID — UUID v4 over /dev/urandom (no uuid crate in this
/// crate's dependency set).
fn random_uuid() -> String {
    let mut b = [0u8; 16];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .is_err()
    {
        let seed = format!("{}:{}:{}", now_ms(), std::process::id(), "runner");
        let h = baal_service_runtime::json_text::sha256(seed.as_bytes());
        for i in 0..16 {
            b[i] = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap_or(0);
        }
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{:02x}", x)).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32]
    )
}

fn read_json_file(path: &Path) -> Result<V, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// `[...new Set(arr)]` — first-occurrence dedup preserving order.
fn uniq_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for v in values {
        if seen.insert(v.clone()) {
            out.push(v);
        }
    }
    out
}
fn uniq_values(values: &[V]) -> Vec<V> {
    let mut seen: Vec<V> = Vec::new();
    for v in values {
        if !seen.iter().any(|x| x == v) {
            seen.push(v.clone());
        }
    }
    seen
}
fn string_array(v: &V) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}
fn string_set(v: &V) -> Vec<V> {
    uniq_values(&v.as_array().cloned().unwrap_or_default())
}

fn js_typeof(v: &V) -> &'static str {
    match v {
        V::Null => "object",
        V::Bool(_) => "boolean",
        V::Number(_) => "number",
        V::String(_) => "string",
        V::Array(_) => "object",
        V::Object(_) => "object",
    }
}

/// `[A-Z0-9_]{1,80}` full match — the stop-provenance errorCode gate.
fn provenance_code(code: Option<&str>) -> Option<String> {
    code.filter(|c| {
        !c.is_empty()
            && c.len() <= 80
            && c.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    })
    .map(String::from)
}

fn iso_or_null(ms: Option<f64>) -> V {
    ms.map(|m| json!(ms_to_iso(m))).unwrap_or(V::Null)
}

// ---------------------------------------------------------------------------
// API mirror — api.next / api.cooldown / api.manualReview the runner reads and
// writes. The ported ReadClient keeps those internals private; the mirror is
// fed by the on_admission callback plus observation of every ServiceError.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct ApiMirror {
    next: f64,
    cooldown: f64,
    manual_review: bool,
}

struct Api {
    client: ReadClient,
    completion_pacing: bool,
    mirror: Arc<Mutex<ApiMirror>>,
    rate_file: PathBuf,
    /// The shared authorization gate (authorizeDispatch).
    gate: Rc<RefCell<Rc<AuthGate>>>,
    /// Set when a paired host opens; fields stay in sync with the mirror.
    paired_api: Option<Rc<RefCell<PairedLiveApi>>>,
}
impl Api {
    fn new(client: ReadClient, completion_pacing: bool, _ctx: &Ctx,
        _fences: Rc<RefCell<Vec<String>>>, gate: Rc<AuthGate>, rate_file: PathBuf,
        paired_api: Rc<RefCell<PairedLiveApi>>) -> Self {
        Api {
            client,
            completion_pacing,
            mirror: Arc::new(Mutex::new(ApiMirror::default())),
            rate_file,
            gate: Rc::new(RefCell::new(gate)),
            paired_api: Some(paired_api),
        }
    }
}

impl Api {
    fn manual_review(&self) -> bool {
        self.mirror.lock().unwrap().manual_review
    }
    fn cooldown(&self) -> f64 {
        self.mirror.lock().unwrap().cooldown
    }
    fn next(&self) -> f64 {
        self.mirror.lock().unwrap().next
    }
    fn requests(&self) -> u64 {
        self.client.requests()
    }
    fn set_next(&self, next: f64) {
        self.mirror.lock().unwrap().next = next;
        self.sync_paired();
    }
    fn cooldown_ready(&self, now: f64) -> bool {
        self.cooldown() <= now
    }
    /// on_admission(value): persist rate.json and update the mirror. Mirrors
    /// JS `onAdmission(next)=>atomicJson(rate.json,{next,manualReview:next===null})`.
    fn admission(&self, value: Option<f64>) {
        {
            let mut m = self.mirror.lock().unwrap();
            match value {
                Some(v) => m.next = v,
                None => m.manual_review = true,
            }
        }
        let body = json!({"next": value, "manualReview": value.is_none()});
        let _ = atomic_json(&self.rate_file, &body);
        self.sync_paired();
    }
    fn sync_paired(&self) {
        if let Some(api) = &self.paired_api {
            let m = self.mirror.lock().unwrap();
            let mut a = api.borrow_mut();
            a.next = m.next;
            a.cooldown = m.cooldown;
            a.manual_review = m.manual_review;
        }
    }
    /// Mirror the client's private cooldown/manual_review updates that surface
    /// through ServiceError (429/503 cooldown, manual-review latch).
    fn observe_error(&self, e: &ServiceError) {
        let mut m = self.mirror.lock().unwrap();
        if e.message == "Rate cooldown requires manual review" {
            m.manual_review = true;
        }
        if matches!(e.status, Some(429) | Some(503)) && e.retry_after_ms.is_finite() {
            m.cooldown = now_ms() + e.retry_after_ms;
        }
        drop(m);
        self.sync_paired();
    }
    fn request(&self, endpoint: &str) -> Result<V, ServiceError> {
        match self.client.request(endpoint) {
            Ok(v) => Ok(v),
            Err(e) => {
                self.observe_error(&e);
                Err(e)
            }
        }
    }
    /// api.dispatch(spec): the JS dispatch path runs authorizeDispatch on the
    /// safe view (which keeps beforeDispatch/beforeNetwork), then performs.
    /// The runner evaluates the JS authorize expression on the full spec V
    /// before translating into DispatchSpec, because the ported consumer
    /// fences are usize ids carried inside the spec.
    fn dispatch_with(&self, spec_v: &V, _fences: &Rc<RefCell<Vec<String>>>) -> Result<V, String> {
        let gate = self.gate.borrow().clone();
        self.dispatch(spec_v, &|s| gate.authorize(s)).map_err(|e| e.message)
    }
    /// Transport-shape dispatch (WireDispatch/transport::Dispatch): guards are
    /// resolved through the fences registry by usize id.
    fn dispatch_transport(&self, spec_v: &V, bd: Option<&TGuard>, bn: Option<&TGuard>,
        _fences: &Rc<RefCell<Vec<String>>>) -> Result<V, WireDispatchError> {
        if self.client.read_only && spec_v["method"].as_str() != Some("GET") {
            return Err(wire_err("Stopped"));
        }
        let gate = self.gate.borrow().clone();
        if !gate.authorize_with(spec_v, bd, bn) {
            return Err(wire_err("Stopped"));
        }
        let spec = spec_v_to_dispatch(spec_v);
        match self.client.dispatch(spec) {
            Ok(v) => Ok(v),
            Err(e) => {
                self.observe_error(&e);
                Err(WireDispatchError { message: e.message.clone(), wire_receipt: None })
            }
        }
    }
    fn close(&self) { self.client.close(); }
    fn abort(&self) { self.client.abort(); }
    fn dispatch(&self, spec_v: &V, authorize: &dyn Fn(&V) -> bool) -> Result<V, ServiceError> {
        if self.client.read_only && spec_v["method"].as_str() != Some("GET") {
            return Err(service_error("Shared account budget forbids Codex writes"));
        }
        if !authorize(spec_v) {
            return Err(service_error("Execution dispatch is not authorized"));
        }
        let spec = spec_v_to_dispatch(spec_v);
        match self.client.dispatch(spec) {
            Ok(v) => Ok(v),
            Err(e) => {
                self.observe_error(&e);
                Err(e)
            }
        }
    }
    fn orders(&self) -> Result<Vec<V>, ServiceError> {
        match self.client.orders() {
            Ok(v) => Ok(v),
            Err(e) => {
                self.observe_error(&e);
                Err(e)
            }
        }
    }
    /// api.pause(ms) — stepped sleep that aborts on signal.
    fn pause(&self, ms: f64, signal: &Arc<AtomicBool>, sleep: &Arc<dyn Fn(f64) + Send + Sync>) -> Result<(), ServiceError> {
        let until = now_ms() + ms;
        loop {
            let d = until - now_ms();
            if d <= 0.0 {
                return Ok(());
            }
            if signal.load(Ordering::SeqCst) {
                return Err(service_error("Stopped"));
            }
            (sleep)(d.min(200.0));
        }
    }
}

fn service_error(message: &str) -> ServiceError {
    ServiceError {
        message: message.to_string(),
        status: None,
        api_code: None,
        retry_after_ms: 0.0,
        recoverable: false,
        stopped: false,
    }
}

/// spec V -> DispatchSpec. Ported consumers put usize fence ids in
/// `beforeDispatch`/`beforeNetwork`; ReadClient's perform calls the guard
/// bodies (JS fence bodies re-checked the issued identity — already proven by
/// the authorize evaluation), so the ints become pass-through admission
/// guards. A non-GET spec without a beforeDispatch fence trips the client's
/// own 'Execution admission guard required' — identical to the JS check.
fn spec_v_to_dispatch(spec: &V) -> DispatchSpec {
    let has_bd = spec.get("beforeDispatch").is_some_and(|x| !x.is_null());
    let has_bn = spec.get("beforeNetwork").is_some_and(|x| !x.is_null());
    DispatchSpec {
        method: spec["method"].as_str().unwrap_or("").to_string(),
        endpoint: spec["endpoint"].as_str().unwrap_or("").to_string(),
        body: spec.get("body").cloned().filter(|b| !b.is_null()),
        body_json: spec
            .get("bodyJson")
            .and_then(|x| x.as_str())
            .map(String::from),
        before_dispatch: if has_bd { Some(Arc::new(|| Ok::<(), String>(()))) } else { None },
        before_network: if has_bn { Some(Arc::new(|| Ok::<(), String>(()))) } else { None },
    }
}

// ---------------------------------------------------------------------------
// Guard plumbing — the ported crates use two guard types at the dispatch
// boundary: transport::Guard (TGuard, OrderTransportError) for writes and the
// evidence-bridge String-guard (SGuard) for scoped reads. The runner's
// api.dispatch accepts both call shapes; each host exposes authorizes in its
// own shape, so this module adapts.
// ---------------------------------------------------------------------------

type TGuard<'a> = dyn Fn(&V, &V) -> Result<bool, OrderTransportError> + 'a;
type SGuard<'a> = dyn Fn(&V, &V) -> Result<bool, String> + 'a;

/// Guard kind carried beside a spec through api.dispatch.
enum SpecGuards<'a> {
    /// `spec=>api.dispatch(spec)` — no guard objects.
    None,
    /// `transport::Dispatch` / `WireDispatch` — transport guards.
    T(Option<&'a TGuard<'a>>, Option<&'a TGuard<'a>>),
    /// Evidence-bridge DispatchFn — String guards.
    S(Option<&'a SGuard<'a>>, Option<&'a SGuard<'a>>),
}

enum PairedHost {
    V1(PairedLiveHost),
    V2(Box<PairedLiveHostV2>),
    Neutral(PairedNeutralRecoveryHost),
}

impl PairedHost {
    fn authorizes(&self, spec: &V, guards: &SpecGuards) -> bool {
        let false_guard = |_x: &V, _y: &V| -> Result<bool, String> { Ok(false) };
        match guards {
            SpecGuards::T(bd, bn) => match self {
                Self::V1(h) => h.authorizes_write(spec, *bd, *bn),
                Self::V2(h) => h.authorizes_write(spec, *bd, *bn),
                // The neutral recovery host only authorizes GET reads; a
                // write guard tuple can never pass its method check.
                Self::Neutral(_) => false,
            },
            SpecGuards::S(bd, bn) => {
                let bd = bd.unwrap_or(&false_guard);
                let bn = bn.unwrap_or(&false_guard);
                match self {
                    Self::V1(h) => h.authorizes(spec, bd, bn),
                    Self::V2(h) => h.authorizes(spec, bd, bn),
                    Self::Neutral(h) => h.authorizes(spec, bd, bn),
                }
            }
            _ => match self {
                Self::V1(h) => h.authorizes(spec, &false_guard, &false_guard),
                Self::V2(h) => h.authorizes(spec, &false_guard, &false_guard),
                Self::Neutral(h) => h.authorizes(spec, &false_guard, &false_guard),
            },
        }
    }
    fn allows_legacy(&self, spec: &V) -> bool {
        match self {
            PairedHost::V1(h) => h.allows_legacy(spec),
            PairedHost::V2(h) => h.allows_legacy(spec),
            PairedHost::Neutral(h) => h.allows_legacy(spec),
        }
    }
    fn blocks_legacy_writes(&self) -> bool {
        match self {
            PairedHost::V1(h) => h.blocks_legacy_writes(),
            PairedHost::V2(h) => h.blocks_legacy_writes(),
            PairedHost::Neutral(h) => h.blocks_legacy_writes(),
        }
    }
    fn on_fetch_start(&self, event: &V) -> bool {
        match self {
            PairedHost::V1(h) => h.on_fetch_start(event),
            PairedHost::V2(h) => h.on_fetch_start(event),
            PairedHost::Neutral(h) => h.on_fetch_start(event),
        }
    }
    fn status(&self) -> V {
        match self {
            PairedHost::V1(h) => h.status(),
            PairedHost::V2(h) => h.status(),
            PairedHost::Neutral(h) => h.status(),
        }
    }
    /// pairedLiveHost.step(pairedCandidate?.status()) — signature varies:
    /// V1/V2 take the capture status; the recovery hosts take none.
    fn step(&self, capture: &V) -> Result<(), String> {
        match self {
            PairedHost::V1(h) => h.step(capture),
            PairedHost::V2(h) => h.step(capture).map(|_| ()),
            PairedHost::Neutral(h) => h.step().map(|_| ()),
        }
    }
    fn close(&self) -> Result<(), String> {
        match self {
            PairedHost::V1(h) => h.close(),
            PairedHost::V2(h) => h.close(),
            PairedHost::Neutral(h) => h.close(),
        }
    }
}

fn wire_err(message: &str) -> WireDispatchError {
    WireDispatchError { message: message.to_string(), wire_receipt: None }
}

fn transport_error(code: &str) -> OrderTransportError {
    OrderTransportError {
        code: code.to_string(),
        status: None,
        api_code: None,
        outcome: "failed".into(),
        may_have_executed: false,
        retry_same_request_only: false,
        retry_after_ms: 0.0,
        semantic_mismatch: false,
    }
}

// __PART3__
// ---------- paired journal (legacy Journal or native journal + restore metrics) ----------
enum PairedJournal {
    Legacy(Journal),
    Native(NativeJournal),
}
impl PairedJournal {
    fn inner(&self) -> &Journal {
        match self { Self::Legacy(j) => j, Self::Native(j) => &j.journal }
    }
    fn inner_mut(&mut self) -> &mut Journal {
        match self { Self::Legacy(j) => j, Self::Native(j) => &mut j.journal }
    }
    fn commit(&mut self, state: &V, event: &V) -> R<V> {
        self.inner_mut().commit(state, event).map_err(raw)
    }
    fn native_restore(&self) -> Option<V> {
        match self { Self::Native(j) => Some(j.native_restore.clone()), _ => None }
    }
}


/// contractEvidence.authorizes — reader permits only live GET specs for its own
/// scope endpoints; closest ported form: GET + scope-endpoint match while the
/// reader is open. (live-paper/contract-evidence.mjs:92)
fn contract_evidence_authorizes(spec: &V, scope_market_ids: &[String], tournament_id: &str) -> bool {
    if spec.get("method").and_then(V::as_str) != Some("GET")
        || spec.get("body").map(|v| !v.is_null()).unwrap_or(false)
        || spec.get("bodyJson").map(|v| !v.is_null()).unwrap_or(false)
    {
        return false;
    }
    let ep = spec.get("endpoint").and_then(V::as_str).unwrap_or("");
    scope_market_ids.iter().any(|mid| {
        ep == format!("/markets/{}?tournamentId={}", mid, tournament_id)
            || ep == format!("/markets/{}/nodes?tournamentId={}", mid, tournament_id)
    })
}

/// basketRelationships.authorizes — GET paged /relationships?tournamentId=...&limit=200
/// (live-paper/basket-relations.mjs:80-88)
fn basket_authorizes(spec: &V, tournament_id: &str) -> bool {
    if spec.get("method").and_then(V::as_str) != Some("GET")
        || spec.get("body").map(|v| !v.is_null()).unwrap_or(false)
        || spec.get("bodyJson").map(|v| !v.is_null()).unwrap_or(false)
    {
        return false;
    }
    let ep = spec.get("endpoint").and_then(V::as_str).unwrap_or("");
    ep.starts_with(&format!("/relationships?tournamentId={}&limit=200", tournament_id))
}

// ---------- competition controller (settlement | median v1 | median v2 clock) ----------
#[derive(Clone, Copy, PartialEq)]
enum CompetitionKind {
    Settlement,
    Median,
    MedianClock,
}

enum Competition {
    Settlement(SettlementCompetitionController),
    Median(MedianCompetitionController),
    MedianClock(MedianCompetitionControllerV2Clock),
}
impl Competition {
    fn base(&self) -> &CompetitionController {
        match self {
            Self::Settlement(c) => &c.base,
            Self::Median(c) => &c.base,
            Self::MedianClock(c) => &c.inner.base,
        }
    }
    fn base_mut(&mut self) -> &mut CompetitionController {
        match self {
            Self::Settlement(c) => &mut c.base,
            Self::Median(c) => &mut c.base,
            Self::MedianClock(c) => &mut c.inner.base,
        }
    }
    fn activation(&self) -> &V { &self.base().activation }
    fn state(&self) -> V { self.base().inner.borrow().state.clone() }
    fn proposals(&self) -> V {
        match self {
            Self::Median(c) => c.m.borrow().proposals.clone(),
            Self::MedianClock(c) => c.inner.m.borrow().proposals.clone(),
            _ => V::Null,
        }
    }
    fn scheduling_bank_hash(&self) -> Option<String> {
        match self {
            Self::MedianClock(c) => Some(c.inner.scheduling_bank_hash()),
            _ => None,
        }
    }
    fn managed_market_ids(&self) -> BTreeSet<String> {
        self.base().managed_market_ids().into_iter().collect()
    }
    fn eligible_market_ids(&self) -> Vec<String> {
        match self {
            Self::Settlement(c) => c.eligible_market_ids(),
            Self::Median(c) => c.eligible_market_ids(),
            Self::MedianClock(c) => c.inner.eligible_market_ids(),
        }
    }
    fn can_operate(&self) -> bool { self.base().can_operate() }
    /// competition.beforeBook?.(marketId) — optional on the JS controller;
    /// present on all ported variants.
    fn before_book(&mut self, market_id: &str) -> R<()> {
        match self {
            Self::Settlement(c) => c.before_book(market_id).map_err(raw),
            Self::Median(c) => c.before_book(market_id).map_err(raw),
            Self::MedianClock(c) => c.inner.before_book(market_id).map_err(raw),
        }
    }
    fn assert_ledgers(&self) -> R<()> { self.base().assert_ledgers().map_err(raw) }
    fn active(&self) -> Option<V> { self.base().active() }
    fn snapshot(&self, book: Option<&V>) -> V { self.base().snapshot(book) }
    fn before_books(&mut self, args: &V) -> R<()> {
        match self {
            Self::Settlement(c) => c.before_books(args).map_err(raw),
            Self::Median(c) => c.before_books(args).map_err(raw),
            Self::MedianClock(c) => c.inner.before_books(args).map_err(raw),
        }
    }
    fn on_book(&mut self, response: &V) -> R<()> {
        match self {
            Self::Settlement(c) => c.on_book(response).map_err(raw),
            Self::Median(c) => c.on_book(response).map_err(raw),
            Self::MedianClock(c) => c.inner.on_book(response).map_err(raw),
        }
    }
    fn failure(&mut self, error: &V) { self.base_mut().failure(error) }
    fn publish(&self) {
        match self {
            Self::Settlement(c) => c.publish(),
            Self::Median(c) => c.publish(),
            Self::MedianClock(c) => c.inner.publish(),
        }
    }
    fn close(&mut self) -> R<()> {
        match self {
            Self::Settlement(c) => c.close().map_err(raw),
            Self::Median(c) => c.close().map_err(raw),
            Self::MedianClock(c) => c.inner.close().map_err(raw),
        }
    }
    fn transport(&self) -> &OrderTransport { &self.base().transport }
    fn journal(&self) -> &Rc<RefCell<Journal>> { &self.base().journal }
    fn dir(&self) -> &Path { &self.base().dir }
    fn ledger(&self) -> &Rc<RefCell<IntentLedger>> { &self.base().ledger }
    fn bank_current(&self) -> R<bool> {
        match self {
            Self::Settlement(c) => Ok(c.bank_current()),
            Self::Median(c) => c.bank_current().map_err(raw),
            Self::MedianClock(c) => c.inner.bank_current().map_err(raw),
        }
    }
    fn before_candidate_read(&mut self, market_id: &str) -> R<bool> {
        match self {
            Self::Median(c) => c.before_candidate_read(market_id).map_err(raw),
            Self::MedianClock(c) => c.inner.before_candidate_read(market_id).map_err(raw),
            _ => Ok(false),
        }
    }
    fn initialize_median_reference(&mut self) -> R<V> {
        match self {
            Self::Median(c) => c.initialize_median_reference().map_err(raw),
            Self::MedianClock(c) => c.inner.initialize_median_reference().map_err(raw),
            _ => Err(raw("MEDIAN_REFERENCE_UNSUPPORTED")),
        }
    }
    fn entry_window_admitted(&self, now: i64) -> bool {
        match self {
            Self::MedianClock(c) => c.entry_window_admitted(now),
            other => other.base().entry_window_admitted(now),
        }
    }
}

// ---------- dual-NO paper host ----------
enum DualNoHost {
    V1(DualNoPaperHost),
    V2(DualNoPaperHostV2),
}
impl DualNoHost {
    fn bank_inner(&self) -> &V {
        match self { Self::V1(h) => &h.bank, Self::V2(h) => &h.bank }
    }
    fn status(&self) -> R<V> {
        match self { Self::V1(h) => h.status().map_err(raw), Self::V2(h) => h.status().map_err(raw) }
    }
    fn publish(&self) -> R<()> {
        match self { Self::V1(h) => h.publish().map_err(raw), Self::V2(h) => h.publish().map_err(raw) }
    }
    fn on_book(&mut self, publication: &V, market_id: &str, account_eligibility: &V) -> R<()> {
        match self {
            Self::V1(h) => h.on_book(publication, market_id, account_eligibility).map_err(raw),
            Self::V2(h) => h.on_book(publication, market_id, account_eligibility).map_err(raw),
        }
    }
    fn focus(&self) -> R<V> {
        match self { Self::V1(h) => h.focus().map_err(raw), Self::V2(h) => h.focus().map_err(raw) }
    }
    fn boundary(&mut self, reason: &str) -> R<()> {
        match self {
            Self::V1(h) => h.boundary(Some(reason)).map_err(raw),
            Self::V2(h) => h.boundary(Some(reason)).map_err(raw),
        }
    }
    fn close(&mut self) -> R<()> {
        match self { Self::V1(h) => h.close().map_err(raw), Self::V2(h) => h.close().map_err(raw) }
    }
}

// ---------- no-maker host (revisions 1-5 share one collector surface) ----------
enum NoMakerH {
    V1(NoMakerHost),
    V2(NoMakerHostV2),
    V3(NoMakerHostV3),
    V4(NoMakerHostV4),
    V5(NoMakerHostV5),
}
impl NoMakerH {
    fn status(&self) -> R<V> {
        match self {
            Self::V1(h) => h.status().map_err(raw), Self::V2(h) => h.status().map_err(raw),
            Self::V3(h) => h.status().map_err(raw), Self::V4(h) => h.status().map_err(raw),
            Self::V5(h) => h.status().map_err(raw),
        }
    }
    fn bank(&self) -> &V {
        match self {
            Self::V1(h) => &h.bank, Self::V2(h) => &h.bank, Self::V3(h) => &h.bank,
            Self::V4(h) => &h.bank, Self::V5(h) => &h.bank,
        }
    }
    fn dir(&self) -> &Path {
        match self {
            Self::V1(h) => &h.dir, Self::V2(h) => &h.dir, Self::V3(h) => &h.dir,
            Self::V4(h) => &h.dir, Self::V5(h) => &h.dir,
        }
    }
    fn on_page(&self, row: &V) -> R<()> {
        match self {
            Self::V1(h) => h.on_page(row).map_err(raw), Self::V2(h) => h.on_page(row).map_err(raw),
            Self::V3(h) => h.on_page(row).map_err(raw), Self::V4(h) => h.on_page(row).map_err(raw),
            Self::V5(h) => h.on_page(row).map_err(raw),
        }
    }
    fn on_book(&self, publication: &V, market_id: &V, account_eligibility: &V) -> R<()> {
        match self {
            Self::V1(h) => h.on_book(publication, market_id, account_eligibility).map_err(raw),
            Self::V2(h) => h.on_book(publication, market_id, account_eligibility).map_err(raw),
            Self::V3(h) => h.on_book(publication, market_id, account_eligibility).map_err(raw),
            Self::V4(h) => h.on_book(publication, market_id, account_eligibility).map_err(raw),
            Self::V5(h) => h.on_book(publication, market_id, account_eligibility).map_err(raw),
        }
    }
    fn boundary(&self, reason: &V) -> R<()> {
        match self {
            Self::V1(h) => h.boundary(reason).map_err(raw), Self::V2(h) => h.boundary(reason).map_err(raw),
            Self::V3(h) => h.boundary(reason).map_err(raw), Self::V4(h) => h.boundary(reason).map_err(raw),
            Self::V5(h) => h.boundary(reason).map_err(raw),
        }
    }
}

// ---------- baal pair host (revisions 1-4) ----------
enum BaalPairH {
    V1(crate::baal_pair_host::BaalPairHost),
    V2(crate::baal_pair_host_v2::BaalPairHost),
    V3(crate::baal_pair_host_v3::BaalPairHost),
    V4(crate::baal_pair_host_v4::BaalPairHost),
}
impl BaalPairH {
    fn bank(&self) -> &V {
        match self {
            Self::V1(h) => h.bank(), Self::V2(h) => h.bank(),
            Self::V3(h) => h.bank(), Self::V4(h) => h.bank(),
        }
    }
    fn status(&self) -> V {
        match self {
            Self::V1(h) => h.status(), Self::V2(h) => h.status(),
            Self::V3(h) => h.status(), Self::V4(h) => h.status(),
        }
    }
    fn on_book(&mut self, publication: &V, market_id: &str, account_eligibility: &V) -> R<()> {
        match self {
            Self::V1(h) => h.on_book(publication, market_id, account_eligibility).map_err(raw),
            Self::V2(h) => h.on_book(publication, market_id, account_eligibility).map_err(raw),
            Self::V3(h) => h.on_book(publication, market_id, account_eligibility).map_err(raw),
            Self::V4(h) => h.on_book(publication, market_id, account_eligibility).map_err(raw),
        }
    }
    fn boundary(&mut self, reason: &str) -> R<()> {
        match self {
            Self::V1(h) => h.boundary(reason).map_err(raw), Self::V2(h) => h.boundary(reason).map_err(raw),
            Self::V3(h) => h.boundary(reason).map_err(raw), Self::V4(h) => h.boundary(reason).map_err(raw),
        }
    }
}

// ---------- stable-maker family (stable maker broad/narrow, coupled maker, coupled bulk js/rust) ----------
enum StableMakerH {
    Stable(StableMakerHost),
    Coupled(CoupledMakerHost),
    Bulk(CoupledBulkHost),
    BulkRust(CoupledBulkRustHost),
}
impl StableMakerH {
    fn bank(&self) -> &V {
        match self {
            Self::Stable(h) => &h.bank, Self::Coupled(h) => &h.bank,
            Self::Bulk(h) => &h.bank, Self::BulkRust(h) => &h.bank,
        }
    }
    fn status(&self) -> R<V> {
        match self {
            Self::Stable(h) => h.status().map_err(raw), Self::Coupled(h) => h.status().map_err(raw),
            Self::Bulk(h) => h.status().map_err(raw), Self::BulkRust(h) => h.status().map_err(raw),
        }
    }
    fn focus(&self) -> R<V> {
        match self {
            Self::Stable(h)=> h.focus().map_err(raw), Self::Coupled(h) => h.focus().map_err(raw),
            Self::Bulk(h) => h.focus().map_err(raw), Self::BulkRust(h) => h.focus().map_err(raw),
        }
    }
    fn on_page(&mut self, row: &V) -> R<()> {
        match self {
            Self::Stable(h) => h.on_page(row).map_err(raw), Self::Coupled(h) => h.on_page(row).map_err(raw),
            Self::Bulk(h) => h.on_page(row).map_err(raw), Self::BulkRust(h) => h.on_page(row).map_err(raw),
        }
    }
    fn on_price(&mut self, row: &V) -> R<()> {
        match self {
            Self::Stable(_) | Self::Coupled(_) => Ok(()),
            Self::Bulk(h) => h.on_price(row).map_err(raw),
            Self::BulkRust(h) => h.on_price(row).map_err(raw),
        }
    }
    fn on_book(&mut self, publication: &V, market_id: &str, account_eligibility: &V) -> R<()> {
        match self {
            Self::Stable(h) => h.on_book(publication, market_id, account_eligibility).map_err(raw),
            Self::Coupled(h) => h.on_book(publication, market_id, account_eligibility).map_err(raw),
            Self::Bulk(h) => h.on_book(publication, market_id, account_eligibility).map_err(raw),
            Self::BulkRust(h) => h.on_book(publication, market_id, account_eligibility).map_err(raw),
        }
    }
    fn boundary(&mut self, reason: &V) -> R<()> {
        match self {
            Self::Stable(h) => h.boundary(reason.as_str().unwrap_or("")).map_err(raw),
            Self::Coupled(h) => h.boundary(reason).map_err(raw),
            Self::Bulk(h) => h.boundary(reason).map_err(raw),
            Self::BulkRust(h) => h.boundary(reason).map_err(raw),
        }
    }
    fn dir(&self) -> &Path {
        match self {
            Self::Stable(h) => &h.dir, Self::Coupled(h) => &h.dir,
            Self::Bulk(h) => &h.dir, Self::BulkRust(h) => &h.dir,
        }
    }
}

// ---------- wrapCoupledDrain: drain-mode wrappers around the coupled-bulk host ----------
fn drain_marker(runtime_dir: &Path) -> bool {
    runtime_dir.join("COUPLED_BULK_ENTRY_DRAIN").exists()
}
fn bulk_status(host: &mut StableMakerH, runtime_dir: &Path) -> R<V> {
    drain_status(host.status()?, drain_marker(runtime_dir)).pipe(Ok)
}
fn bulk_focus(host: &mut StableMakerH, runtime_dir: &Path) -> R<V> {
    drain_focus(host.focus()?, drain_marker(runtime_dir)).pipe(Ok)
}
fn bulk_on_book(host: &mut StableMakerH, runtime_dir: &Path, publication: &V, market_id: &str,
    account_eligibility: &V) -> R<()> {
    if drain_boundary_needed(&host.status()?["wallets"], market_id, drain_marker(runtime_dir)) {
        host.boundary(&json!("operator_drain_unowned_entry_features"))?;
    }
    host.on_book(publication, market_id, account_eligibility)
}
trait Pipe: Sized { fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T { f(self) } }
impl<T> Pipe for T {}

// ---------- native dip / quote hosts ----------
enum NativeDipH {
    V1(NativeDipHost),
    V2(NativeDipCacheHost),
    V3(NativeDipIntentHost),
    Quote(crate::native_quote_host_v1::NativeQuoteHost),
}
impl NativeDipH {
    fn status(&self) -> V {
        match self { Self::V1(h) => h.status(), Self::V2(h) => h.status(), Self::V3(h) => h.status(), Self::Quote(h) => h.status() }
    }
    fn kick(&self) -> bool {
        match self { Self::V1(h) => h.kick(), Self::V2(h) => h.kick(), Self::V3(h) => h.kick(), Self::Quote(h) => h.kick() }
    }
    fn close(&self) {
        match self { Self::V1(h) => h.close(), Self::V2(h) => h.close(), Self::V3(h) => h.close(), Self::Quote(h) => h.close() }
    }
}

// ---------- book publications (v2 legacy | v3 native) ----------
enum BookPub {
    V2(BookPublicationWriter<Box<dyn Fn() -> i64>, Box<dyn Fn() -> bool>>),
    V3(NativeBookPublicationV3),
}
impl BookPub {
    fn publish_book(&mut self, market_id: &str, response: &V) -> R<bool> {
        match self {
            Self::V2(w) => w.publish_book(market_id, response).map_err(raw),
            Self::V3(w) => w.publish_book(market_id, response).map_err(raw),
        }
    }
    fn invalidate(&mut self, market_id: Option<&str>, reason: &str) -> R<V> {
        match self {
            Self::V2(w) => w.invalidate(market_id, reason).map_err(raw),
            Self::V3(w) => w.invalidate(market_id, reason).map_err(raw),
        }
    }
    fn close(&mut self, reason: &str) -> R<bool> {
        match self {
            Self::V2(w) => w.close(reason).map_err(raw),
            Self::V3(w) => w.close(reason).map_err(raw),
        }
    }
    fn status(&self) -> V {
        match self {
            Self::V2(w) => w.state.clone(),
            Self::V3(w) => w.status(),
        }
    }
    fn startup(&self) -> Option<V> {
        match self { Self::V3(w) => Some(w.startup()), _ => None }
    }
}

// ---------------------------------------------------------------------------
// Shared context captured by every runner closure (JS module-scope bindings).
// ---------------------------------------------------------------------------
#[derive(Clone)]
struct Ctx {
    runtime_dir: PathBuf,
    workspace: PathBuf,
    stop_file: PathBuf,
    paper_stop: PathBuf,
    collector_stop: PathBuf,
    api: Rc<RefCell<Option<Api>>>,
    stopping: Rc<Cell<bool>>,
    signal: Arc<AtomicBool>,
    recent: Rc<RefCell<Vec<V>>>,
    runtime: Rc<RefCell<V>>,
}
impl Ctx {
    fn exists(&self, p: &Path) -> bool { p.exists() }
    fn stopped(&self) -> bool { self.stopping.get() || self.signal.load(Ordering::Relaxed) }
    fn manual_review(&self) -> bool {
        self.api.borrow().as_ref().map(|a| a.mirror.lock().unwrap().manual_review).unwrap_or(false)
    }
    fn cooldown(&self) -> f64 {
        self.api.borrow().as_ref().map(|a| a.mirror.lock().unwrap().cooldown).unwrap_or(0.0)
    }
    fn marker(&self, name: &str) -> bool { self.exists(&self.runtime_dir.join(name)) }
    fn blocked_stops(&self) -> bool {
        self.exists(&self.stop_file) || self.exists(&self.paper_stop) || self.exists(&self.collector_stop)
    }
    fn recent_admissions(&self) -> i64 {
        let now = now_ms();
        self.recent.borrow().iter().filter(|x| {
            x.get("admittedAt").and_then(V::as_str).and_then(parse_iso_ms)
                .map(|a| now - a < 60000.0).unwrap_or(false)
        }).count() as i64
    }
    // canOperate shapes used across the runner (JS writes each inline).
    fn can_operate(&self) -> bool { !self.stopped() && !self.manual_review() }
    fn can_operate_stops(&self) -> bool { self.can_operate() && !self.blocked_stops() }
    fn can_operate_full(&self) -> bool { self.can_operate_stops() && self.cooldown() <= now_ms() }
    fn can_operate_no_exec(&self) -> bool { self.can_operate_full() && !self.marker("EXECUTION_STOP") }
    fn has_read_headroom(&self, n: i64) -> bool { self.recent_admissions() + n + 4 <= 60 }
    fn has_read_headroom_2(&self, n: i64) -> bool { self.recent_admissions() + n + 2 <= 60 }
}

// ---------------------------------------------------------------------------
// Dashboard: minimal HTTP/1.0 server on a raw TcpListener (crate has no http
// dep). Routes match runner.mjs exactly.
// ---------------------------------------------------------------------------
struct DashServer {
    shutdown: Arc<AtomicBool>,
    port: u16,
    handle: Option<std::thread::JoinHandle<()>>,
}
impl Drop for DashServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = std::net::TcpStream::connect(format!("127.0.0.1:{}", self.port));
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

type DashboardFn = Arc<dyn Fn(&str) -> (u16, &'static str, Vec<u8>) + Send + Sync>;

fn start_dashboard(port: u16, handler: DashboardFn, shutdown: Arc<AtomicBool>) -> Result<DashServer, String> {
    use std::io::{Read as _, Write as _};
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let stop = shutdown.clone();
    let handle = std::thread::spawn(move || loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match listener.accept() {
            Ok((mut sock, _)) => {
                let _ = sock.set_nonblocking(false);
                let mut buf = [0u8; 16384];
                let n = sock.read(&mut buf).unwrap_or(0);
                let text = String::from_utf8_lossy(&buf[..n]);
                let mut parts = text.split_whitespace();
                let method = parts.next().unwrap_or("");
                let url = parts.next().unwrap_or("/");
                let path_only = url.split('?').next().unwrap_or(url);
                let (code, ctype, body) = if method != "GET" {
                    (405u16, "text/plain", Vec::new())
                } else {
                    handler(path_only)
                };
                let status_text = match code {
                    200 => "OK",
                    404 => "Not Found",
                    405 => "Method Not Allowed",
                    503 => "Service Unavailable",
                    _ => "OK",
                };
                let head = format!(
                    "HTTP/1.1 {} {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    code, status_text, ctype, body.len()
                );
                let _ = sock.write_all(head.as_bytes());
                let _ = sock.write_all(&body);
                let _ = sock.flush();
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(_) => break,
        }
    });
    Ok(DashServer { shutdown, port, handle: Some(handle) })
}

fn json_response(v: &V) -> (u16, &'static str, Vec<u8>) {
    (200, "application/json", serde_json::to_vec(v).unwrap_or_else(|_| b"{}".to_vec()))
}

fn read_file_bytes(file: &Path) -> Option<Vec<u8>> {
    std::fs::read(file).ok()
}

// ---------------------------------------------------------------------------
// Fetch chain (runner.mjs:316-322): scopedReceipts?.fetch || globalThis.fetch
// wrapped by exactMoneyFetch, then adapted into the ported ReadClient Fetch.
// ---------------------------------------------------------------------------

// globalThis.fetch -> JS-shaped response V {ok,status,statusText,headers,text,bodyText}
fn raw_fetch_v(url: &str, opts: &V) -> Result<V, String> {
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    raw_fetch_v_with(&client, url, opts)
}

fn raw_fetch_v_with(client: &reqwest::blocking::Client, url: &str, opts: &V) -> Result<V, String> {
    let method = opts.get("method").and_then(V::as_str).unwrap_or("GET");
    let mut b = client
        .request(method.parse().unwrap_or(reqwest::Method::GET), url)
        .timeout(Duration::from_millis(opts.get("timeoutMs").and_then(V::as_f64).map(|t| t as u64).unwrap_or(15000)));
    if let Some(headers) = opts.get("headers").and_then(V::as_object) {
        for (k, v) in headers {
            if let Some(sv) = v.as_str() {
                b = b.header(k.as_str(), sv);
            }
        }
    }
    if let Some(body) = opts.get("body").and_then(V::as_str) {
        b = b.body(body.to_string());
    }
    let resp = b.send().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    // JS fetch `redirect: 'error'` rejects 3xx before producing a response.
    if (300..400).contains(&status) {
        return Err("Redirect rejected".into());
    }
    let retry_after = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = resp.text().map_err(|e| e.to_string())?;
    let mut out = serde_json::Map::new();
    out.insert("ok".into(), json!((200..300).contains(&status)));
    out.insert("status".into(), json!(status));
    out.insert("bodyText".into(), json!(body.clone()));
    out.insert("text".into(), json!(body));
    let mut headers = serde_json::Map::new();
    if let Some(ra) = retry_after {
        headers.insert("retry-after".into(), json!(ra));
    }
    out.insert("headers".into(), V::Object(headers));
    Ok(V::Object(out))
}

// fetchFn for create_scoped_rest_receipts: (url, opts) -> FetchOutcome{value,reader}
fn scoped_fetch_fn(url: &str, opts: &V) -> Result<baal_execution::scoped_rest_receipts::FetchOutcome, String> {
    let v = raw_fetch_v(url, opts)?;
    // Preserve streaming semantics: expose the body through a one-shot reader
    // so scoped receipts take the real stream path (value.bodyText absent).
    let body = v.get("bodyText").and_then(V::as_str).unwrap_or("").as_bytes().to_vec();
    let mut vobj = v.as_object().unwrap().clone();
    vobj.remove("bodyText");
    let sent = std::sync::Mutex::new(false);
    Ok(baal_execution::scoped_rest_receipts::FetchOutcome {
        value: V::Object(vobj),
        reader: Some(Box::new(move || {
            let mut s = sent.lock().unwrap();
            if *s {
                return Ok(None);
            }
            *s = true;
            Ok(Some(body.clone()))
        })),
    })
}

// The client's Fetch adapter: FetchRequest -> opts V -> moneyFetch -> FetchOutcome.
struct RunnerFetch {
    money_fetch: Arc<dyn Fn(&str, &V) -> Result<V, String> + Send + Sync>,
}
impl Fetch for RunnerFetch {
    fn fetch(&self, url: &str, req: &FetchRequest) -> Result<FetchOutcome, String> {
        let mut headers = serde_json::Map::new();
        for (k, v) in &req.headers {
            headers.insert(k.clone(), json!(v));
        }
        let mut opts = serde_json::Map::new();
        opts.insert("method".into(), json!(req.method));
        opts.insert("headers".into(), V::Object(headers));
        if let Some(body) = &req.body {
            opts.insert("body".into(), json!(body));
        }
        opts.insert("timeoutMs".into(), json!(req.timeout_ms));
        let response = (self.money_fetch)(url, &V::Object(opts))?;
        let status = response.get("status").and_then(V::as_i64).unwrap_or(0) as u16;
        let retry_after = response
            .get("headers")
            .and_then(|h| h.get("retry-after"))
            .and_then(V::as_str)
            .map(str::to_string);
        let body = if let Some(t) = response.get("text").and_then(V::as_str) {
            t.to_string()
        } else if let Some(t) = response.get("bodyText").and_then(V::as_str) {
            t.to_string()
        } else if let Some(d) = response.get("data") {
            serde_json::to_string(d).unwrap_or_default()
        } else {
            String::new()
        };
        Ok(FetchOutcome { status, retry_after, body })
    }
}

// ---------------------------------------------------------------------------
// Realtime host (createRealtimeHost, realtime-host.mjs): the ported
// RealtimeHost::new takes books by value so it cannot share the books with
// client callbacks; the runner ports the factory inline with the books held
// in Arc<Mutex<>> — identical semantics.
// ---------------------------------------------------------------------------
struct RtHost {
    client: Arc<RealtimeClient>,
    books: Arc<Mutex<RealtimeBooks<Box<dyn Fn() -> f64 + Send + Sync>>>>,
    dispatch: Dispatch,
    can_operate: CanOp,
    can_mint_token: TokenGate,
    now: NowFn,
    capture: Rc<RefCell<Option<Capture>>>,
    silent_recovery_interval_ms: i64,
    inner: Mutex<RtInner>,
}
struct RtInner {
    closed: bool,
    servicing: bool,
    cursor: usize,
    retry_at: f64,
    recovery_attempts: i64,
    recovery_successes: i64,
    shared_snapshot_attempts: i64,
    shared_snapshot_successes: i64,
    last_recovery_at: Option<String>,
    last_code: Option<String>,
    quiet_deferrals: i64,
    recovered: IndexMap<String, V>,
    fences: Vec<String>, // fence id -> endpoint
    shared_tickets: Vec<V>,
}
impl RtHost {
    fn closed(&self) -> bool {
        self.inner.lock().unwrap().closed
    }
    fn allowed(&self) -> bool {
        !self.closed() && (self.can_operate)()
    }
    fn token_allowed(&self) -> bool {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.can_mint_token)())).unwrap_or(false)
    }
    fn capture_row(&self, kind: &str, market: Option<&V>, data: V) {
        let mut c = self.capture.borrow_mut();
        if let Some(cap) = c.as_mut() {
            let topic = market
                .and_then(|m| m.get("topic").and_then(V::as_str))
                .unwrap_or("")
                .to_string();
            let mut row = serde_json::Map::new();
            row.insert("kind".into(), json!(kind));
            row.insert(
                "observedAt".into(),
                json!(ms_to_iso((self.now)() as f64)),
            );
            row.insert("topic".into(), json!(topic));
            if let V::Object(d) = data {
                for (k, v) in d {
                    row.insert(k, v);
                }
            }
            let _ = cap.append(&V::Object(row));
        }
    }
    fn market_status(&self) -> Vec<V> {
        let status = self.books.lock().unwrap().status().unwrap_or(V::Null);
        let mut markets = status.get("markets").and_then(V::as_array).cloned().unwrap_or_default();
        for market in &markets {
            let mid = market.get("marketId").and_then(V::as_str).unwrap_or("").to_string();
            let prior = self.inner.lock().unwrap().recovered.get(&mid).cloned();
            if market.get("ready") == Some(&V::Bool(true))
                && prior.as_ref().and_then(|p| p.get("connectionId").and_then(V::as_str))
                    == market.get("connectionId").and_then(V::as_str)
                && prior.is_some()
                && (self.now)() as f64 - prior.unwrap()["at"].as_f64().unwrap_or(0.0) >= self.silent_recovery_interval_ms as f64
            {
                let _ = self.books.lock().unwrap().invalidate(&mid, market.get("connectionId").and_then(V::as_str).unwrap_or(""), "PERIODIC");
            }
        }
        let status = self.books.lock().unwrap().status().unwrap_or(V::Null);
        markets = status.get("markets").and_then(V::as_array).cloned().unwrap_or_default();
        markets
    }
    fn authorizes(&self, spec: &V) -> bool {
        if !self.allowed() {
            return false;
        }
        if self.client.authorizes(spec) {
            return true;
        }
        let bd = spec.get("beforeDispatch").and_then(V::as_u64).map(|n| n as usize);
        let bn = spec.get("beforeNetwork").and_then(V::as_u64).map(|n| n as usize);
        spec.get("method").and_then(V::as_str) == Some("GET")
            && spec.get("body").is_none_or(|b| b.is_null())
            && spec.get("bodyJson").is_none_or(|b| b.is_null())
            && bd.is_some() && bd == bn
            && bd
                .and_then(|id| self.inner.lock().unwrap().fences.get(id).cloned())
                .as_deref()
                == spec.get("endpoint").and_then(V::as_str)
    }
    fn status(&self) -> V {
        let blocked = !self.closed() && self.allowed() && !self.token_allowed();
        let mut transport = self.client.status();
        if blocked {
            let o = transport.as_object_mut().unwrap();
            o.insert("state".into(), json!("blocked"));
            o.insert("lastCode".into(), json!("REALTIME_TOKEN_BUDGET_BLOCKED"));
        }
        let st = self.inner.lock().unwrap();
        let events_received = transport.get("eventsReceived").and_then(V::as_i64).unwrap_or(0);
        let markets = self
            .market_status()
            .iter()
            .map(|m| {
                let mut o = m.as_object().cloned().unwrap_or_default();
                let rows = o.remove("books").and_then(|b| b.as_array().map(|a| a.len())).unwrap_or(0);
                o.insert("availableBooks".into(), json!(rows));
                V::Object(o)
            })
            .collect::<Vec<V>>();
        json!({
            "schemaVersion": 1, "mode": "read_only_market_capture", "executionAuthorized": false,
            "at": ms_to_iso((self.now)() as f64), "transport": transport,
            "marketStreamObserved": events_received > 0,
            "dataEvidence": if blocked { json!("blocked_shared_read_only_budget") } else if events_received > 0 { json!("market_frames_observed") } else { json!("awaiting_first_market_frame") },
            "recoveryAttempts": st.recovery_attempts, "recoverySuccesses": st.recovery_successes,
            "silentRecoveryIntervalMs": self.silent_recovery_interval_ms,
            "quietRecoveryDeferrals": st.quiet_deferrals,
            "sharedSnapshotAttempts": st.shared_snapshot_attempts,
            "sharedSnapshotSuccesses": st.shared_snapshot_successes,
            "lastRecoveryAt": st.last_recovery_at,
            "lastCode": if blocked { json!("REALTIME_TOKEN_BUDGET_BLOCKED") } else { st.last_code.clone().map_or(V::Null, |c| json!(c)) },
            "retryAt": if blocked { V::Null } else if st.retry_at > 0.0 { json!(ms_to_iso(st.retry_at)) } else { V::Null },
            "markets": markets,
        })
    }
    fn fence_holds(&self, market_id: &str, connection_id: &V) -> bool {
        if !self.allowed() {
            return false;
        }
        let view = self.books.lock().unwrap().view(market_id).unwrap_or(V::Null);
        if view.get("connectionId") != Some(connection_id) {
            return false;
        }
        let status = self.client.status();
        let subscribed = status
            .get("markets")
            .and_then(V::as_array)
            .map(|a| a.iter().any(|m| m.get("marketId").and_then(V::as_str) == Some(market_id) && m.get("subscribed") == Some(&V::Bool(true))))
            .unwrap_or(false);
        subscribed
    }
    fn service(&self) -> Result<bool, String> {
        {
            let mut st = self.inner.lock().unwrap();
            if self.closed() || !(self.can_operate)() || st.servicing {
                return Ok(false);
            }
            st.servicing = true;
        }
        let out = self.service_inner();
        self.inner.lock().unwrap().servicing = false;
        out
    }
    fn service_inner(&self) -> Result<bool, String> {
        // Budget v3 forbids every POST, including ephemeral socket auth.
        if !self.token_allowed() {
            let mut st = self.inner.lock().unwrap();
            st.last_code = Some("REALTIME_TOKEN_BUDGET_BLOCKED".into());
            st.retry_at = 0.0;
            let state = self.client.status().get("state").and_then(V::as_str).unwrap_or("").to_string();
            drop(st);
            if !["idle", "stopped", "blocked"].contains(&state.as_str()) {
                let _ = self.client.stop();
            }
            return Ok(false);
        }
        let state = self.client.status().get("state").and_then(V::as_str).unwrap_or("").to_string();
        if ["idle", "failed", "reconnecting", "blocked"].contains(&state.as_str()) {
            let now = (self.now)() as f64;
            if now < self.inner.lock().unwrap().retry_at {
                return Ok(false);
            }
            self.inner.lock().unwrap().retry_at = now + 60000.0;
            let r = if state == "idle" { self.client.start() } else { self.client.reconnect() };
            if r.is_err() {
                self.inner.lock().unwrap().last_code = Some("REALTIME_CONNECTION_UNAVAILABLE".into());
                return Ok(false);
            }
        }
        if !self.allowed() {
            return Ok(false);
        }
        let state = self.client.status().get("state").and_then(V::as_str).unwrap_or("").to_string();
        if state == "refused" {
            let mut st = self.inner.lock().unwrap();
            st.last_code = Some("REALTIME_JOIN_REFUSED".into());
            st.retry_at = 0.0;
            return Ok(false);
        }
        if state != "subscribed" {
            return Ok(false);
        }
        let markets = self.market_status();
        let events_received = self.client.status().get("eventsReceived").and_then(V::as_i64).unwrap_or(0);
        for i in 0..markets.len() {
            let cursor = self.inner.lock().unwrap().cursor;
            if markets.is_empty() {
                break;
            }
            let index = (cursor + i) % markets.len();
            let market = &markets[index];
            let market_id = market.get("marketId").and_then(V::as_str).unwrap_or("").to_string();
            if market.get("needsSnapshot") != Some(&V::Bool(true)) {
                continue;
            }
            let reasons: Vec<String> = market
                .get("reasons")
                .and_then(V::as_array)
                .map(|a| a.iter().filter_map(|r| r.as_str().map(String::from)).collect())
                .unwrap_or_default();
            if reasons.iter().any(|r| r == "SETTLEMENT") {
                continue;
            }
            let prior = self.inner.lock().unwrap().recovered.get(&market_id).cloned();
            let now = (self.now)() as f64;
            if events_received <= 0
                && !reasons.is_empty()
                && reasons.iter().all(|r| r == "STALE")
                && prior.as_ref().and_then(|p| p.get("connectionId").and_then(V::as_str))
                    == market.get("connectionId").and_then(V::as_str)
                && prior.is_some()
                && now - prior.as_ref().unwrap().get("at").and_then(V::as_f64).unwrap_or(0.0)
                    < self.silent_recovery_interval_ms as f64
            {
                self.inner.lock().unwrap().quiet_deferrals += 1;
                continue;
            }
            self.inner.lock().unwrap().cursor = (index + 1) % markets.len();
            let connection_id = market.get("connectionId").and_then(V::as_str).unwrap_or("").to_string();
            let ticket = self.books.lock().unwrap().begin_snapshot(&market_id, &connection_id)?;
            self.capture_row("recovery_started", Some(market), json!({"generation": ticket.get("generation").cloned().unwrap_or(V::Null)}));
            // Fence: identical body to realtime-host.mjs — registered by id so
            // authorizes() can bind spec fence ids to the endpoint.
            let endpoint = ticket.get("endpoint").and_then(V::as_str).unwrap_or("").to_string();
            let fence_id = {
                let mut st = self.inner.lock().unwrap();
                st.fences.push(endpoint.clone());
                st.recovery_attempts += 1;
                st.fences.len() - 1
            };
            let ticket_market = market_id.clone();
            let ticket_connection = ticket.get("connectionId").cloned().unwrap_or(V::Null);
            let this = AssertSend(self.shared());
            let fence: Rc<SGuard> = Rc::new(move |_x: &V, _y: &V| -> Result<bool, String> {
                let this = &this.0;
                if this.fence_holds(&ticket_market, &ticket_connection) {
                    Ok(true)
                } else {
                    Err("REALTIME_RECOVERY_STOPPED".into())
                }
            });
            let spec = json!({"method":"GET","endpoint":endpoint,"beforeDispatch":fence_id,"beforeNetwork":fence_id});
            self.dispatch_register(fence_id, fence.clone());
            let result = (self.dispatch)(&spec);
            if result.is_ok() {
                fence(&json!({}), &json!({})).map_err(|e| e)?;
            }
            match result {
                Ok(response) => {
                    let accepted = self.books.lock().unwrap().complete_snapshot(&ticket, &response).unwrap_or(false);
                    if accepted {
                        self.capture_row("snapshot", Some(market), json!({"receivedAt": response.get("receivedAt").cloned().unwrap_or(V::Null)}));
                        let mut st = self.inner.lock().unwrap();
                        st.recovery_successes += 1;
                        st.last_recovery_at = response.get("receivedAt").and_then(V::as_str).map(str::to_string);
                        st.last_code = None;
                        st.recovered.insert(market_id.clone(), json!({"connectionId": ticket.get("connectionId").cloned().unwrap_or(V::Null), "at": now}));
                    } else {
                        self.inner.lock().unwrap().last_code = Some("REALTIME_RECOVERY_REJECTED".into());
                    }
                }
                Err(_) => {
                    let _ = self.books.lock().unwrap().snapshot_failed(&ticket);
                    self.inner.lock().unwrap().last_code = Some("REALTIME_RECOVERY_UNAVAILABLE".into());
                }
            }
            return Ok(true);
        }
        Ok(false)
    }
    fn shared(&self) -> RtHostHandle {
        RtHostHandle {
            books: self.books.clone(),
            can_operate: self.can_operate.clone(),
            client: self.client.clone(),
        }
    }
    fn dispatch_register(&self, _id: usize, _guard: Rc<SGuard<'static>>) {}
    fn begin_shared_snapshot(&self, market_id: &str) -> Option<V> {
        if !self.allowed() || !self.token_allowed() || self.client.status().get("state").and_then(V::as_str) != Some("subscribed") {
            return None;
        }
        let status = self.client.status();
        let connection = status
            .get("markets")
            .and_then(V::as_array)
            .and_then(|a| a.iter().find(|m| m.get("marketId").and_then(V::as_str) == Some(market_id)).cloned());
        let connection = connection?;
        if connection.get("subscribed") != Some(&V::Bool(true)) || connection.get("connectionId").map_or(true, |x| x.is_null()) {
            return None;
        }
        let view = self.books.lock().unwrap().view(market_id).unwrap_or(V::Null);
        let reasons = view.get("reasons").and_then(V::as_array).cloned().unwrap_or_default();
        if reasons.iter().any(|r| r.as_str() == Some("SETTLEMENT")) {
            return None;
        }
        let ticket = self
            .books
            .lock()
            .unwrap()
            .begin_snapshot(market_id, connection.get("connectionId").and_then(V::as_str).unwrap_or(""))
            .ok()?;
        let mut st = self.inner.lock().unwrap();
        st.shared_snapshot_attempts += 1;
        st.shared_tickets.push(ticket.clone());
        Some(ticket)
    }
    fn read_shared_snapshot(&self, ticket: &V) -> Result<V, String> {
        let pos = {
            let st = self.inner.lock().unwrap();
            st.shared_tickets.iter().position(|t| t == ticket)
        };
        let pos = pos.ok_or("REALTIME_SHARED_TICKET")?;
        self.inner.lock().unwrap().shared_tickets.remove(pos);
        let market_id = ticket.get("marketId").and_then(V::as_str).unwrap_or("").to_string();
        let connection_id = ticket.get("connectionId").cloned().unwrap_or(V::Null);
        let endpoint = ticket.get("endpoint").and_then(V::as_str).unwrap_or("").to_string();
        let this = AssertSend(self.shared());
        let fence: Rc<SGuard> = Rc::new(move |_x: &V, _y: &V| -> Result<bool, String> {
            if this.0.fence_holds(&market_id, &connection_id) {
                Ok(true)
            } else {
                Err("REALTIME_RECOVERY_STOPPED".into())
            }
        });
        let fence_id = {
            let mut st = self.inner.lock().unwrap();
            st.fences.push(endpoint.clone());
            st.fences.len() - 1
        };
        self.dispatch_register(fence_id, fence.clone());
        let spec = json!({"method":"GET","endpoint":endpoint,"beforeDispatch":fence_id,"beforeNetwork":fence_id});
        fence(&json!({}), &json!({}))?;
        (self.dispatch)(&spec)
    }
    fn complete_shared_snapshot(&self, ticket: &V, response: &V) -> bool {
        if ticket.is_null() || !self.allowed() {
            return false;
        }
        let accepted = self.books.lock().unwrap().complete_snapshot(ticket, response).unwrap_or(false);
        if accepted {
            let market_id = ticket.get("marketId").and_then(V::as_str).unwrap_or("").to_string();
            let mut st = self.inner.lock().unwrap();
            st.shared_snapshot_successes += 1;
            st.last_recovery_at = response.get("receivedAt").and_then(V::as_str).map(str::to_string);
            st.recovered.insert(market_id, json!({"connectionId": ticket.get("connectionId").cloned().unwrap_or(V::Null), "at": (self.now)()}));
        }
        accepted
    }
    fn fail_shared_snapshot(&self, ticket: &V) {
        if !ticket.is_null() {
            let _ = self.books.lock().unwrap().snapshot_failed(ticket);
        }
    }
    fn view(&self, market_id: &str) -> V {
        if !self.allowed() {
            return V::Null;
        }
        self.books.lock().unwrap().view(market_id).unwrap_or(V::Null)
    }
    fn stop(&self) {
        {
            let mut st = self.inner.lock().unwrap();
            if st.closed {
                return;
            }
            st.closed = true;
        }
        let _ = self.client.stop();
        self.books.lock().unwrap().stop();
    }
}
struct RtHostHandle {
    books: Arc<Mutex<RealtimeBooks<Box<dyn Fn() -> f64 + Send + Sync>>>>,
    can_operate: CanOp,
    client: Arc<RealtimeClient>,
}
impl RtHostHandle {
    fn fence_holds(&self, market_id: &str, connection_id: &V) -> bool {
        if !(self.can_operate)() {
            return false;
        }
        let view = self.books.lock().unwrap().view(market_id).unwrap_or(V::Null);
        if &view["connectionId"] != connection_id {
            return false;
        }
        self.client
            .status()
            .get("markets")
            .and_then(V::as_array)
            .map(|a| a.iter().any(|m| m.get("marketId").and_then(V::as_str) == Some(market_id) && m.get("subscribed") == Some(&V::Bool(true))))
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// settlementPortfolio enum: openCollectorSettlementPortfolioV2 host or the
// frozen-paper pause adapter (runner.mjs:304).
// ---------------------------------------------------------------------------
enum SettlementPort {
    Host(crate::settlement_portfolio_host_v2::SettlementPortfolioHostV2),
    Frozen(crate::frozen_paper_pause_adapter_v1::FrozenPaperPause),
}
impl SettlementPort {
    fn consume(&mut self, available_at: Option<&str>, account_eligibility: &V) -> Result<usize, String> {
        match self {
            SettlementPort::Host(h) => h.consume(available_at, account_eligibility),
            SettlementPort::Frozen(f) => { let _ = f.consume(); Ok(0) }
        }
    }
    fn status(&self) -> Result<V, String> {
        match self {
            SettlementPort::Host(h) => h.status(),
            SettlementPort::Frozen(f) => Ok(f.status()),
        }
    }
    fn publish(&mut self) -> Result<(), String> {
        match self {
            SettlementPort::Host(h) => h.publish(),
            SettlementPort::Frozen(f) => { f.publish(); Ok(()) }
        }
    }
    fn focus(&self) -> V {
        match self {
            SettlementPort::Host(h) => h.focus().unwrap_or(V::Null),
            SettlementPort::Frozen(f) => f.focus(),
        }
    }
    fn close(&mut self) -> Result<(), String> {
        match self {
            SettlementPort::Host(h) => h.close(),
            SettlementPort::Frozen(f) => { f.close(); Ok(()) }
        }
    }
}

// ---------------------------------------------------------------------------
// Runner — one struct replacing runner.mjs module-scope bindings.
// ---------------------------------------------------------------------------
type Cell_<T> = Rc<RefCell<Option<T>>>;
fn cell<T>() -> Cell_<T> {
    Rc::new(RefCell::new(None))
}
fn cell_take<T>(c: &Cell_<T>) -> Option<T> {
    c.borrow_mut().take()
}

type ReaderDispatch = Box<dyn Fn(&V) -> Result<V, String> + Send + Sync>;

struct Runner {
    root: PathBuf,
    workspace: PathBuf,
    runtime_dir: PathBuf,
    c: V,
    network_budget: Option<V>,
    network_interval_ms: i64,
    production_activation_file: PathBuf,
    production_activation: Option<V>,
    stop_file: PathBuf,
    paper_stop: PathBuf,
    collector_stop: PathBuf,
    sampling_session_id: String,
    ctx: Ctx,
    journal: Cell_<PairedJournal>,
    state: V,
    account_cache: V,
    official_sync: bool,
    api: Cell_<Api>,
    authorize_gate: Rc<AuthGate>,
    paired_api: Rc<RefCell<PairedLiveApi>>,
    fences: Rc<RefCell<Vec<String>>>,
    server: Option<DashServer>,
    server_shutdown: Arc<AtomicBool>,
    heartbeat_stop: Arc<AtomicBool>,
    heartbeat_handle: Option<std::thread::JoinHandle<()>>,
    competition: Cell_<Competition>,
    realtime: Cell_<RtHost>,
    realtime_capture: Cell_<Capture>,
    basket_relationships: Cell_<BasketRelationshipReader<ReaderDispatch>>,
    contract_evidence: Cell_<ContractEvidenceReader<ReaderDispatch>>,
    contract_evidence_scope: Rc<RefCell<Vec<String>>>,
    book_publication: Cell_<BookPub>,
    basket_followup_source: Cell_<BasketFollowupSource<Box<dyn Fn() -> i64>>>,
    basket_followup_bank_hash: Rc<RefCell<Option<String>>>,
    comparison_risk_source: Cell_<ComparisonRiskSource>,
    settlement_paper: Cell_<SettlementCollectorHost>,
    settlement_portfolio: Cell_<SettlementPort>,
    dual_no_paper: Cell_<DualNoHost>,
    dual_no_paper_v2: Cell_<DualNoHost>,
    production_metadata: Cell_<ProductionMetadata>,
    paired_candidate: Cell_<PairedCandidateCollector>,
    candidate_account: Rc<RefCell<V>>,
    median_discovery: Cell_<CollectorMedianDiscoveryV3>,
    median_preflight: Cell_<MedianPreflight<'static>>,
    paired_live_host: Cell_<PairedHost>,
    party_pair_selector: Cell_<PartyWinnerPairCollector>,
    median_candidate_bulk_hints: Rc<RefCell<IndexMap<String, V>>>,
    scoped_receipts: Option<Rc<ScopedRestReceipts>>,
    universe: V,
    coverage: Option<V>,
    coverage_rc: Rc<RefCell<Option<V>>>,
    coverage_file: PathBuf,
    depth_plan: V,
    dip_bank: Option<V>,
    scale_bank: Option<V>,
    responsive_bank: Option<V>,
    scheduler: V,
    scheduler_scope: V,
    scheduler_file: PathBuf,
    market_sweep: Option<V>,
    price_publication: Cell_<MarketPricePublication<Box<dyn Fn() -> bool>>>,
    diagnostic_source: Cell_<DiagnosticFocus>,
    diagnostic_loading: bool,
    diagnostic_failed: bool,
    canonical_capacity: Cell_<CanonicalCapacityGuard>,
    trade_tape: Cell_<TradeTape>,
    no_maker: Cell_<NoMakerH>,
    stable_maker: Cell_<StableMakerH>,
    stable_maker_broad: Cell_<StableMakerH>,
    coupled_maker: Cell_<StableMakerH>,
    coupled_bulk: Cell_<StableMakerH>,
    baal_pair: Cell_<BaalPairH>,
    native_dip: Cell_<NativeDipH>,
    native_quote: Cell_<NativeDipH>,
    eligibility_publication: Cell_<AccountEligibilityPublication>,
    collector_market_screen: Cell_<CollectorMarketScreen>,
    baal_read_attention: Cell_<BaalReadAttention>,
    retire_native_prices: bool,
    money_receipt_writer: Rc<RefCell<Option<MoneyReceiptWriter<'static>>>>,
}

impl Runner {
    // runtime object helpers — JS mutates a plain object; same here.
    fn rt(&self) -> std::cell::Ref<'_, V> {
        self.ctx.runtime.borrow()
    }
    /// Fresh reader dispatch (`spec=>api.dispatch(spec)`), one per reader —
    /// JS hands each reader the same bound dispatcher.
    fn mk_reader_dispatch(&self) -> ReaderDispatch {
        let api = AssertSend(self.api.clone());
        let fences = AssertSend(self.fences.clone());
        Box::new(move |spec: &V| -> Result<V, String> {
            api.get().borrow()
                .as_ref()
                .map(|a| a.dispatch_with(spec, fences.get()))
                .unwrap_or(Err("STOPPED".into()))
        })
    }

    /// ServiceError-typed reader dispatch (PairedCandidateCollector::open takes it).
    fn mk_service_dispatch(&self) -> Box<dyn Fn(&V) -> Result<V, baal_service_runtime::client::ServiceError> + Send + Sync> {
        let api = AssertSend(self.api.clone());
        let fences = AssertSend(self.fences.clone());
        Box::new(move |spec: &V| -> Result<V, baal_service_runtime::client::ServiceError> {
            fences.get().borrow_mut().push("reader-dispatch".into());
            let endpoint = spec["endpoint"].as_str().unwrap_or_default();
            match api.get().borrow().as_ref().map(|a| a.client.request(endpoint)) {
                Some(r) => r,
                None => Err(baal_service_runtime::client::ServiceError {
                    message: "Stopped".into(), status: None, api_code: None,
                    retry_after_ms: 0.0, recoverable: false, stopped: true,
                }),
            }
        })
    }

    fn rt_set(&self, key: &str, v: V) {
        self.ctx.runtime.borrow_mut()[key] = v;
    }
    fn rt_get(&self, key: &str) -> V {
        self.ctx.runtime.borrow()[key].clone()
    }

    fn api(&self) -> std::cell::Ref<'_, Api> {
        std::cell::Ref::map(self.api.borrow(), |o| o.as_ref().unwrap())
    }

    // stop(cause, errorCode) — runner.mjs:135
    fn stop(&self, cause: &str, error_code: Option<&str>) {
        if self.ctx.stopping.get() {
            return;
        }
        let code_ok = error_code
            .map(|c| !c.is_empty() && c.len() <= 80 && c.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'))
            .unwrap_or(false);
        let prov = json!({
            "at": iso_now(),
            "cause": cause,
            "errorCode": if code_ok { json!(error_code.unwrap()) } else { V::Null },
            "pid": std::process::id(),
        });
        self.rt_set("stopProvenance", prov.clone());
        // Preserve the cause independently when a status-file failure caused shutdown.
        {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true)
                .open(self.runtime_dir.join("collector-lifecycle.jsonl"))
            {
                let line = serde_json::to_string(&prov).unwrap_or_default();
                let _ = f.write_all((line + "\n").as_bytes());
                let _ = f.sync_all();
            }
        }
        println!("{}", json!({"event":"collector_stopping","at":prov["at"],"cause":prov["cause"],"errorCode":prov["errorCode"],"pid":prov["pid"]}));
        self.ctx.stopping.set(true);
        self.ctx.signal.store(true, Ordering::Relaxed);
    }

    // stableHosts() — insertion order coupledBulk,coupledMaker,stableMakerBroad,stableMaker.
    /// stableHosts() — insertion order coupledBulk,coupledMaker,
    /// stableMakerBroad,stableMaker; the cells, since hosts aren't Clone.
    fn stable_hosts(&self) -> Vec<(&'static str, Cell_<StableMakerH>)> {
        [
            ("coupledBulk", &self.coupled_bulk),
            ("coupledMaker", &self.coupled_maker),
            ("stableMakerBroad", &self.stable_maker_broad),
            ("stableMaker", &self.stable_maker),
        ]
        .into_iter()
        .filter(|(_, c)| c.borrow().is_some())
        .map(|(k, c)| (k, c.clone()))
        .collect()
    }
    fn stable_host_failed(&self, key: &str) {
        self.rt_set(key, json!({"status":"failed","executionAuthority":false}));
        match key {
            "stableMaker" => cell_take(&self.stable_maker),
            "stableMakerBroad" => cell_take(&self.stable_maker_broad),
            "coupledMaker" => cell_take(&self.coupled_maker),
            _ => cell_take(&self.coupled_bulk),
        };
    }
    fn stable_status(&self, key: &str, h: &StableMakerH) -> R<V> {
        // wrapCoupledDrain: only coupledBulk is drain-wrapped.
        let st = h.status()?;
        if key == "coupledBulk" {
            Ok(crate::coupled_drain::drain_status(st, self.ctx.marker("COUPLED_BULK_ENTRY_DRAIN")))
        } else {
            Ok(st)
        }
    }
    fn stable_focus(&self, key: &str, h: &StableMakerH) -> R<V> {
        let f = h.focus()?;
        if key == "coupledBulk" {
            Ok(crate::coupled_drain::drain_focus(f, self.ctx.marker("COUPLED_BULK_ENTRY_DRAIN")))
        } else {
            Ok(f)
        }
    }
    fn stable_boundary(&self, key: &str, h: &mut StableMakerH, method: &str) -> R<()> {
        if key == "coupledBulk"
            && crate::coupled_drain::drain_boundary_needed(&h.status()?, "", self.ctx.marker("COUPLED_BULK_ENTRY_DRAIN"))
        {
            h.boundary(&V::String("operator_drain_unowned_entry_features".into()))?;
        }
        h.boundary(&V::String(method.to_string()))
    }
    fn stable_on_book(&self, key: &str, h: &mut StableMakerH, pub_status: &V, market_id: &str, eligibility: &V) -> R<()> {
        if key == "coupledBulk"
            && crate::coupled_drain::drain_boundary_needed(&h.status()?, market_id, self.ctx.marker("COUPLED_BULK_ENTRY_DRAIN"))
        {
            h.boundary(&V::String("operator_drain_unowned_entry_features".into()))?;
        }
        h.on_book(pub_status, market_id, eligibility)
    }

    // retireDrainedPrior / retireDrainedNarrow — runner.mjs:178-179
    fn retire_drained_prior(&mut self) {
        if self.coupled_bulk.borrow().is_none() {
            return;
        }
        for (key, host_cell) in self.stable_hosts() {
            if key == "coupledBulk" {
                continue;
            }
            let mut hb = host_cell.borrow_mut();
            let Some(h) = hb.as_mut() else { continue };
            let st = match self.stable_status(key, h) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let failed = st.get("failed").and_then(V::as_bool).unwrap_or(false);
            let has_risk = st.get("riskMarketIds").and_then(V::as_array).map(|a| !a.is_empty()).unwrap_or(false);
            let has_work = st
                .get("wallets")
                .and_then(V::as_object)
                .map(|w| {
                    w.values().any(|x| {
                        !x.get("positions").and_then(V::as_array).map(|a| a.is_empty()).unwrap_or(true)
                            || !x.get("unresolvedQuotes").and_then(V::as_array).map(|a| a.is_empty()).unwrap_or(true)
                    })
                })
                .unwrap_or(false);
            if failed || has_risk || has_work {
                continue;
            }
            let _ = h.boundary(&V::String("replaced_by_bulk_cohort_after_complete_projection".into()));
            let marker = h.dir().join("STOP");
            if !marker.exists() {
                let _ = write_exclusive(&marker, &serde_json::to_string(&json!({
                    "owner":"Codex","at":iso_now(),
                    "reason":"Replaced by separately frozen bulk-history cohort after all risk resolved",
                    "executionAuthority":false
                })).unwrap_or_default());
            }
            let mut st = self.stable_status(key, h).unwrap_or(V::Null);
            st["status"] = json!("retired");
            st["accountingPreserved"] = json!(true);
            self.rt_set(key, st);
            match key {
                "coupledMaker" => cell_take(&self.coupled_maker),
                "stableMakerBroad" => cell_take(&self.stable_maker_broad),
                _ => cell_take(&self.stable_maker),
            };
        }
    }
    fn retire_drained_narrow(&mut self) {
        if self.stable_maker.borrow().is_none() || self.stable_maker_broad.borrow().is_none() {
            return;
        }
        let host_ref = self.stable_maker.borrow();
        let host = host_ref.as_ref().unwrap();
        let st = match self.stable_status("stableMaker", host) {
            Ok(s) => s,
            Err(_) => return,
        };
        let has_work = st
            .get("wallets")
            .and_then(V::as_object)
            .map(|w| {
                w.values().any(|x| {
                    !x.get("positions").and_then(V::as_array).map(|a| a.is_empty()).unwrap_or(true)
                        || !x.get("unresolvedQuotes").and_then(V::as_array).map(|a| a.is_empty()).unwrap_or(true)
                })
            })
            .unwrap_or(false);
        if st.get("failed").and_then(V::as_bool).unwrap_or(false) || has_work {
            return;
        }
        drop(host_ref);
        let mut hb = self.stable_maker.borrow_mut();
        let h = hb.as_mut().unwrap();
        let _ = h.boundary(&V::String("replaced_by_broad_cohort_after_complete_projection".into()));
        let _ = write_exclusive(&h.dir().join("STOP"), &serde_json::to_string(&json!({
            "owner":"Codex","at":iso_now(),
            "reason":"Replaced by separately frozen market-wide cohort after all quotes resolved",
            "executionAuthority":false
        })).unwrap_or_default());
        let mut st = self.stable_status("stableMaker", h).unwrap_or(V::Null);
        st["status"] = json!("retired");
        st["accountingPreserved"] = json!(true);
        self.rt_set("stableMaker", st);
        cell_take(&self.stable_maker);
    }

    // saveScheduler / recordAttempt — runner.mjs:164-173
    fn save_scheduler(&mut self, next: V) -> R<()> {
        match baal_service_runtime::atomic::atomic_json_canonical(&self.scheduler_file, &next) {
            Ok(()) => {
                self.scheduler = next;
                Ok(())
            }
            Err(_) => Err(RunnerError::durable("Book scheduler recording failed", true)),
        }
    }
    fn record_attempt(&mut self, kind: &str, market_id: &str, sampling_plan_id: Option<&str>, native_discovery: bool) -> R<()> {
        let at = iso_now();
        let empty: Vec<String> = Vec::new();
        let one = vec![market_id.to_string()];
        let mut next = crate::book_scheduler::record_scheduler_attempts(
            &self.scheduler,
            &self.scheduler_scope,
            if kind == "book" { &one } else { &empty },
            if kind == "history" { &one } else { &empty },
            Some(&at),
        )
        .map_err(RunnerError::msg)?;
        if kind == "book" {
            if let Some(plan_id) = sampling_plan_id {
                next = crate::comparison_sampling::record_comparison_attempt(
                    &next,
                    &self.scheduler_scope,
                    market_id,
                    &at,
                    plan_id,
                )
                .map_err(RunnerError::msg)?;
            }
        }
        if kind == "book" && (market_id == "381" || market_id == "382") {
            let mut pkg = next.get("packageReadLastAttemptAt").and_then(V::as_object).cloned().unwrap_or_default();
            pkg.insert(market_id.to_string(), json!(at));
            next["packageReadLastAttemptAt"] = V::Object(pkg);
        }
        if kind == "book" && native_discovery {
            next = crate::native_discovery_plan_v1::record_native_discovery_attempt(&next, market_id).map_err(RunnerError::msg)?;
        }
        self.save_scheduler(next)
    }

    // schedulingInputs — runner.mjs:180-208
    fn scheduling_inputs(&mut self) -> R<V> {
        let mut sources: Vec<V> = Vec::new();
        for (name, bank) in [
            ("dip", self.dip_bank.clone()),
            ("dip-scale", self.scale_bank.clone()),
            ("dip-responsive", self.responsive_bank.clone()),
        ] {
            if let Some(bank) = bank {
                if !crate::experiment_retirement::paper_experiment_retired(&self.runtime_dir, "DIP_STOP").unwrap_or(false) {
                    let state: V = read_json_file(&self.runtime_dir.join(format!("{}-state.json", name)))?;
                    let mut healthy = false;
                    if let Ok(stats) = read_json_file(&self.runtime_dir.join(format!("{}-stats.json", name))) {
                        healthy = stats.get("bankHash").and_then(V::as_str) == Some(digest(&bank).as_str())
                            && crate::worker_health::worker_health(&stats, now_ms(), 60000.0).get("status").and_then(V::as_str) == Some("running");
                    }
                    sources.push(json!({"state": state, "bank": bank, "healthy": healthy}));
                }
            }
        }
        let mut inputs = crate::collection_inputs::collection_inputs(&sources, &self.universe, self.coverage.as_ref(), now_ms()).map_err(RunnerError::msg)?;
        let history_screen = if let Some(ms) = self.collector_market_screen.borrow_mut().as_mut() {
            ms.projection()
        } else {
            crate::market_history_inputs::read_market_history_inputs(&self.runtime_dir, &self.universe, now_ms(), &self.root)
        };
        self.rt_set("marketHistoryScreen", history_screen.clone());
        if history_screen.get("available") == Some(&V::Bool(true)) {
            let mut ids = inputs.get("candidateMarketIds").and_then(V::as_array).cloned().unwrap_or_default();
            ids.extend(history_screen.get("candidateMarketIds").and_then(V::as_array).cloned().unwrap_or_default());
            inputs["candidateMarketIds"] = V::Array(dedup(ids));
        }
        let shock_file = self.runtime_dir.join("shock-bank.json");
        if shock_file.exists()
            && !crate::experiment_retirement::paper_experiment_retired(&self.runtime_dir, "SHOCK_STOP").unwrap_or(false)
        {
            let risk = crate::collection_inputs::shock_collection_risk(
                &read_json_file(&self.runtime_dir.join("shock-state.json"))?,
                &read_json_file(&shock_file)?,
                &self.universe,
            )
            .map_err(RunnerError::msg)?;
            for key in ["heldMarketIds", "pendingMarketIds"] {
                let mut ids = inputs.get(key).and_then(V::as_array).cloned().unwrap_or_default();
                ids.extend(risk.get(key).and_then(V::as_array).cloned().unwrap_or_default());
                inputs[key] = V::Array(dedup(ids));
            }
        }
        let comparison_risk = if crate::experiment_retirement::paper_experiment_retired(&self.runtime_dir, "ENTRY_COMPARISON_STOP").unwrap_or(false) {
            json!({"configured":false,"available":true,"reason":"OPERATOR_RETIRED_PAPER_EXPERIMENT","heldMarketIds":[],"pendingMarketIds":[]})
        } else {
            self.comparison_risk_source.borrow_mut().as_mut().map(|s| s.poll()).unwrap_or(V::Null)
        };
        if js_truthy(Some(&comparison_risk)) {
            self.rt_set("comparisonRisk", comparison_risk.clone());
            for key in ["heldMarketIds", "pendingMarketIds"] {
                let mut ids = inputs.get(key).and_then(V::as_array).cloned().unwrap_or_default();
                ids.extend(comparison_risk.get(key).and_then(V::as_array).cloned().unwrap_or_default());
                inputs[key] = V::Array(dedup(ids));
            }
        }
        if self.no_maker.borrow().is_some() {
            let st = self.no_maker.borrow().as_ref().unwrap().status().unwrap_or(V::Null);
            let mut ids = inputs.get("heldMarketIds").and_then(V::as_array).cloned().unwrap_or_default();
            ids.extend(st.get("riskMarketIds").and_then(V::as_array).cloned().unwrap_or_default());
            inputs["heldMarketIds"] = V::Array(dedup(ids));
        }
        if self.baal_pair.borrow().is_some() {
            let st = self.baal_pair.borrow().as_ref().unwrap().status();
            let mut ids = inputs.get("heldMarketIds").and_then(V::as_array).cloned().unwrap_or_default();
            ids.extend(st.get("riskMarketIds").and_then(V::as_array).cloned().unwrap_or_default());
            inputs["heldMarketIds"] = V::Array(dedup(ids));
        }
        if self.native_dip.borrow().is_some() {
            let risk = self.native_dip.borrow().as_ref().unwrap().status();
            self.rt_set("nativeDip", risk.clone());
            let mut held = inputs.get("heldMarketIds").and_then(V::as_array).cloned().unwrap_or_default();
            held.extend(risk.get("heldMarketIds").and_then(V::as_array).cloned().unwrap_or_default());
            inputs["heldMarketIds"] = V::Array(dedup(held));
            let mut pending = inputs.get("pendingMarketIds").and_then(V::as_array).cloned().unwrap_or_default();
            pending.extend(risk.get("pendingMarketIds").and_then(V::as_array).cloned().unwrap_or_default());
            inputs["pendingMarketIds"] = V::Array(dedup(pending));
        }
        if self.native_quote.borrow().is_some() {
            let risk = self.native_quote.borrow().as_ref().unwrap().status();
            self.rt_set("nativeQuote", risk.clone());
            let mut held = inputs.get("heldMarketIds").and_then(V::as_array).cloned().unwrap_or_default();
            held.extend(risk.get("heldMarketIds").and_then(V::as_array).cloned().unwrap_or_default());
            inputs["heldMarketIds"] = V::Array(dedup(held));
            let mut pending = inputs.get("pendingMarketIds").and_then(V::as_array).cloned().unwrap_or_default();
            pending.extend(risk.get("pendingMarketIds").and_then(V::as_array).cloned().unwrap_or_default());
            inputs["pendingMarketIds"] = V::Array(dedup(pending));
        }
        if js_truthy(self.rt_get("coupledBulk").get("liabilitiesUnverified")) {
            let mut ids = inputs.get("heldMarketIds").and_then(V::as_array).cloned().unwrap_or_default();
            ids.extend(self.rt_get("coupledBulk").get("knownRiskMarketIds").and_then(V::as_array).cloned().unwrap_or_default());
            inputs["heldMarketIds"] = V::Array(dedup(ids));
        }
        for (key, host_cell) in self.stable_hosts() {
            let hb = host_cell.borrow();
            let Some(h) = hb.as_ref() else { continue };
            match self.stable_focus(key, h) {
                Ok(focus) => {
                    let mut held = inputs.get("heldMarketIds").and_then(V::as_array).cloned().unwrap_or_default();
                    held.extend(focus.get("riskMarketIds").and_then(V::as_array).cloned().unwrap_or_default());
                    inputs["heldMarketIds"] = V::Array(dedup(held));
                    // host===stableHosts()[0]?.[1] — the first non-null entry.
                    if key == self.stable_hosts().first().map(|x| x.0).unwrap_or("") {
                        let mut cands = inputs.get("candidateMarketIds").and_then(V::as_array).cloned().unwrap_or_default();
                        cands.extend(focus.get("candidateMarketIds").and_then(V::as_array).cloned().unwrap_or_default());
                        inputs["candidateMarketIds"] = V::Array(dedup(cands));
                    }
                    self.rt_set(key, focus);
                }
                Err(_) => self.stable_host_failed(key),
            }
        }
        let managed = self
            .competition
            .borrow()
            .as_ref()
            .map(|c| c.managed_market_ids())
            .unwrap_or_default();
        inputs["priorityManagedMarketIds"] = json!(managed.iter().cloned().collect::<Vec<String>>());
        let mut held = inputs.get("heldMarketIds").and_then(V::as_array).cloned().unwrap_or_default();
        held.extend(managed.iter().cloned().map(V::String).collect::<Vec<V>>());
        inputs["heldMarketIds"] = V::Array(dedup(held));
        Ok(inputs)
    }

    // basketMeasurementPlan — runner.mjs:209-229
    fn basket_measurement_plan(&mut self, base_plan: V, excluded: &std::collections::BTreeSet<String>) -> R<V> {
        if self.dip_bank.is_none() {
            return Ok(base_plan);
        }
        let risk = self.rt_get("comparisonRisk");
        if js_truthy(risk.get("configured")) && risk["available"] == V::Bool(false) {
            self.rt_set("basketMeasurement", json!({"reason":"comparison_risk_unavailable"}));
            return Ok(base_plan);
        }
        let mut plan = V::Null;
        let attempt = (|| -> Result<(), String> {
            let file = self.runtime_dir.join("basket-bank.json");
            if !file.exists() {
                self.rt_set("basketMeasurement", json!({"reason":"not_initialized"}));
                return Err("no_plan".into());
            }
            let bank = read_json_file(&file).map_err(|e| e)?;
            let stats = read_json_file(&self.runtime_dir.join("basket-stats.json")).map_err(|e| e)?;
            if bank.get("mode").and_then(V::as_str) != Some("dual_no_basket_measurement")
                || stats.get("bankHash").and_then(V::as_str) != Some(digest(&bank).as_str())
                || digest(&bank["codeIdentity"]) != digest(&crate::basket_watch::basket_code_identity(&self.root).map_err(|e| e)?)
            {
                return Err("Basket measurement identity mismatch".into());
            }
            if crate::worker_health::worker_health(&stats, now_ms(), 60000.0).get("status").and_then(V::as_str) != Some("running")
                || self.ctx.marker("BASKET_STOP")
            {
                self.rt_set("basketMeasurement", json!({"reason":"observer_unavailable"}));
                return Err("no_plan".into());
            }
            let now = now_ms_i64();
            let candidates = crate::basket_inputs::basket_candidates(&bank["definitions"], self.coverage.as_ref().unwrap_or(&V::Null), &self.universe, now)?;
            let excluded_market_ids: Vec<V> = self
                .universe["exchanges"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter(|m| excluded.contains(m["exchangeId"].as_str().unwrap_or("")))
                .map(|m| m["marketId"].clone())
                .collect();
            let bank_hash = digest(&bank);
            let need_new = self.basket_followup_source.borrow().is_none()
                || self.basket_followup_bank_hash.borrow().as_deref() != Some(bank_hash.as_str());
            if need_new {
                if let Some(mut s) = cell_take(&self.basket_followup_source) {
                    drop(s);
                }
                let src = crate::basket_followup_source::create_basket_followup_source(
                    &self.runtime_dir,
                    &self.root,
                    &bank,
                    &self.universe,
                    Box::new(now_ms_i64) as Box<dyn Fn() -> i64>,
                )?;
                *self.basket_followup_source.borrow_mut() = Some(src);
                *self.basket_followup_bank_hash.borrow_mut() = Some(bank_hash);
            }
            let followups: Vec<V> = self.basket_followup_source.borrow_mut().as_mut().unwrap().poll();
            let followups_v = V::Array(followups);
            let mut st = self.basket_followup_source.borrow().as_ref().unwrap().status();
            st["candidates"] = json!(followups_v.as_array().map(|a| a.len()).unwrap_or(0));
            self.rt_set("basketFollowupSource", st);
            plan = crate::basket_book_plan::plan_basket_books(
                &base_plan,
                &self.scheduler_scope,
                Some(&candidates),
                &followups_v,
                Some(&V::Array(excluded_market_ids)),
                self.rt_get("cycles").as_i64().unwrap_or(0),
                now,
                4,          // every
                120_000,    // maxAgeMs
                600_000,    // followupMaxAgeMs
                1_000,      // followupLatencyMs
            )?;
            Ok(())
        })();
        if attempt.is_err() {
            if attempt.as_ref().err().map(|e| e.as_str()) == Some("Basket measurement identity mismatch") {
                return Err(RunnerError::msg("Basket measurement identity mismatch"));
            }
            if attempt.err().as_deref() != Some("no_plan") {
                self.rt_set("basketMeasurement", json!({"reason":"observer_invalid_or_unavailable"}));
            }
            return Ok(base_plan);
        }
        // A consumed research anchor must survive a crash before either pair read.
        if js_truthy(plan["basketMeasurement"].get("persistSchedulerStateBeforeReads")) {
            self.save_scheduler(plan["state"].clone())?;
        }
        self.rt_set("basketMeasurement", plan["basketMeasurement"].clone());
        Ok(plan)
    }

    // comparisonSamplingPlan — runner.mjs:233-249
    fn comparison_sampling_plan(&mut self, base_plan: V) -> R<V> {
        let mut planned = V::Null;
        let attempt = (|| -> Result<(), String> {
            let file = self.runtime_dir.join("comparison-sampling.json");
            if !file.exists() {
                self.rt_set("comparisonSampling", json!({"enabled":false,"reason":"not_configured"}));
                return Err("no_plan".into());
            }
            let config: V = serde_json::from_str(&std::fs::read_to_string(&file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            let mut keys: Vec<String> = config.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
            keys.sort();
            if keys.join(",") != "bankHash,cohort,enabled,mode,schemaVersion"
                || config["schemaVersion"].as_i64() != Some(1)
                || config["mode"].as_str() != Some("paired_comparison_two_attempts_v1")
                || config["enabled"] != V::Bool(true)
            {
                return Err("Invalid sampling configuration".into());
            }
            let risk = self.rt_get("comparisonRisk");
            if !js_truthy(risk.get("configured"))
                || !js_truthy(risk.get("available"))
                || risk["cohort"] != config["cohort"]
                || risk["bankHash"] != config["bankHash"]
            {
                self.rt_set("comparisonSampling", json!({"enabled":false,"reason":"comparison_evidence_unavailable"}));
                return Err("no_plan".into());
            }
            planned = crate::comparison_sampling::plan_comparison_sampling(&base_plan, &self.scheduler_scope, now_ms_i64(), &self.sampling_session_id)?;
            planned["comparisonSampling"] = planned["measurement"].clone();
            Ok(())
        })();
        if let Err(e) = attempt {
            if e == "Invalid sampling configuration" {
                return Err(RunnerError::msg(e));
            }
            if e != "no_plan" {
                self.rt_set("comparisonSampling", json!({"enabled":false,"reason":"sampling_evidence_invalid"}));
            }
            return Ok(base_plan);
        }
        self.save_scheduler(planned["state"].clone())?;
        self.rt_set("comparisonSampling", planned["comparisonSampling"].clone());
        Ok(planned)
    }

    // collectDiagnosticFocus — runner.mjs:158-162
    fn collect_diagnostic_focus(&mut self) -> Option<V> {
        if self.diagnostic_source.borrow().is_none() && !self.diagnostic_loading && !self.diagnostic_failed {
            self.diagnostic_loading = true;
            match crate::diagnostic_read_focus::open_diagnostic_focus(&self.runtime_dir, &self.universe, true, &self.workspace) {
                Ok(mut source) => {
                    let ctx = self.ctx.clone();
                    match source.initialize(move || ctx.stopped()) {
                        Ok(()) => {
                            *self.diagnostic_source.borrow_mut() = Some(source);
                            self.diagnostic_loading = false;
                        }
                        Err(_) => {
                            self.diagnostic_failed = true;
                            self.diagnostic_loading = false;
                        }
                    }
                }
                Err(_) => {
                    self.diagnostic_failed = true;
                    self.diagnostic_loading = false;
                }
            }
        }
        if self.diagnostic_source.borrow().is_none() {
            self.rt_set(
                "diagnosticReadFocus",
                json!({"status": if self.diagnostic_failed { "failed" } else { "loading" }, "executionAuthority": false}),
            );
            return None;
        }
        let now = now_ms();
        let pid = std::process::id().to_string();
        let mut borrowed = self.diagnostic_source.borrow_mut();
        let source = borrowed.as_mut().unwrap();
        let result = (|| -> Result<Option<V>, String> {
            source.consume()?;
            let projection = source.project(
                &json!({"pid": pid, "id": self.sampling_session_id, "at": iso_now()}),
                &json!("collector-diagnostic"),
                now,
            )?;
            let focus = crate::diagnostic_read_focus::diagnostic_depth_focus(
                &projection,
                &self.universe,
                &json!(pid.parse::<i64>().unwrap_or(0)),
                &json!(self.sampling_session_id),
                now,
                &self.root,
            );
            let mut rf = serde_json::Map::new();
            rf.insert("status".into(), json!(if js_truthy(focus.as_ref()) { "running" } else { "unavailable" }));
            if let Some(V::Object(f)) = focus.clone() {
                for (k, v) in f {
                    rf.insert(k, v);
                }
            }
            rf.insert("projection".into(), projection);
            Ok(Some(V::Object(rf)))
        })();
        match result {
            Ok(focus) => {
                let rf = self.rt_get("diagnosticReadFocus");
                self.rt_set("diagnosticReadFocus", rf);
                focus
            }
            Err(_) => {
                self.diagnostic_failed = true;
                drop(borrowed);
                cell_take(&self.diagnostic_source);
                self.rt_set("diagnosticReadFocus", json!({"status":"failed","executionAuthority":false}));
                None
            }
        }
    }

    // commit — runner.mjs:264
    fn commit(&mut self, event: V) -> R<()> {
        let mut j = self.journal.borrow_mut();
        match j.as_mut().map(|j| j.commit(&self.state, &event)) {
            Some(Ok(_)) => Ok(()),
            Some(Err(e)) => Err(RunnerError::durable(format!("Paper journal recording failed: {}", e.message), false)),
            None => Ok(()),
        }
    }

    fn publish_account(&self) -> R<()> {
        baal_service_runtime::atomic::atomic_json(&self.runtime_dir.join("official-account.json"), &self.account_cache).map_err(RunnerError::msg)
    }

    // publishBookEvent — runner.mjs:265 (method names 'publishBook'/'invalidate'/'close')
    fn publish_book_event(&mut self, method: &str, value: &V) -> Result<V, RunnerError> {
        let r = (|| -> Result<V, RunnerError> {
            let result: V = {
                let mut bp = self.book_publication.borrow_mut();
                match method {
                    "publishBook" => Ok::<V, RunnerError>(bp.as_mut().map(|b| V::Bool(b.publish_book(&value["marketId"].as_str().unwrap_or(""), &value["response"]).unwrap_or(false))).unwrap_or(V::Bool(false))),
                    "invalidate" => Ok::<V, RunnerError>(bp.as_mut().and_then(|b| b.invalidate(value["marketId"].as_str(), value["reason"].as_str().unwrap_or("")).ok()).unwrap_or(V::Null)),
                    "close" => Ok::<V, RunnerError>(bp.as_mut().map(|b| V::Bool(b.close(value["marketId"].as_str().unwrap_or("")).unwrap_or(false))).unwrap_or(V::Bool(false))),
                    _ => Ok::<V, RunnerError>(V::Null),
                }
            }?;
            if self.settlement_paper.borrow().is_some() && self.rt_get("settlementPaper")["status"].as_str() != Some("failed") {
                let res = (|| -> Result<(), String> {
                    let mut sp = self.settlement_paper.borrow_mut();
                    let h = sp.as_mut().unwrap();
                    h.consume()?;
                    h.publish()?;
                    self.rt_set("settlementPaper", h.status()?);
                    Ok(())
                })();
                if res.is_err() {
                    let mut retained = self.rt_get("settlementPaper");
                    if let Ok(s) = self.settlement_paper.borrow_mut().as_mut().unwrap().status() {
                        retained = s;
                    }
                    retained["status"] = json!("failed");
                    retained["failed"] = json!(true);
                    retained["reason"] = json!("SETTLEMENT_PAPER_CONSUMER_FAILED");
                    retained["accountingRetained"] = json!(js_truthy(retained.get("wallets")));
                    retained["executionAuthority"] = json!(false);
                    self.rt_set("settlementPaper", retained);
                    return Err(RunnerError::durable("Funded paper recording failed", true));
                }
            }
            if ["invalidate", "close"].contains(&method)
                || (method == "publishBook" && result == V::Bool(false))
            {
                for (key, host_cell) in [("dualNoPaper", &self.dual_no_paper), ("dualNoPaperV2", &self.dual_no_paper_v2)] {
                    if let Some(h) = host_cell.borrow_mut().as_mut() {
                        let _ = h.boundary(method);
                        self.rt_set(key, h.status().unwrap_or(V::Null));
                    }
                }
                if let Some(h) = self.no_maker.borrow_mut().as_mut() {
                    match h.boundary(&V::String(method.to_string())) {
                        Ok(()) => self.rt_set("noMaker", h.status().unwrap_or(V::Null)),
                        Err(_) => {
                            self.rt_set("noMaker", json!({"status":"failed","executionAuthority":false}));
                            cell_take(&self.no_maker);
                        }
                    }
                }
                for (key, host_cell) in self.stable_hosts() {
                    let mut hb = host_cell.borrow_mut();
                    let Some(h) = hb.as_mut() else { continue };
                    match self.stable_boundary(key, h, method) {
                        Ok(()) => {
                            let st = self.stable_status(key, h).unwrap_or(V::Null);
                            self.rt_set(key, st);
                        }
                        Err(_) => self.stable_host_failed(key),
                    }
                }
                if let Some(h) = self.baal_pair.borrow_mut().as_mut() {
                    match h.boundary(method) {
                        Ok(()) => {
                            let st = h.status();
                            self.rt_set("baalPair", st);
                        }
                        Err(_) => {
                            self.rt_set("baalPair", json!({"status":"failed","executionAuthority":false}));
                            cell_take(&self.baal_pair);
                        }
                    }
                }
            }
            Ok(result)
        })();
        r.map_err(|e| {
            if e.durable_recording_failed {
                e
            } else {
                RunnerError::durable("Public book recording failed", true)
            }
        })
    }

    // collectAccountExtra — runner.mjs:252-258
    fn collect_account_extra(&mut self) -> R<()> {
        let slot = (self.rt_get("cycles").as_i64().unwrap_or(0) % 3) as usize;
        let (kind, endpoint) = match slot {
            0 => ("fills", "/tournaments/midterm-elections/portfolio/fills?limit=100"),
            1 => ("pnl", "/tournaments/midterm-elections/portfolio/pnl?period=day"),
            _ => (
                "orderHistory",
                "/orders?status=all&limit=200&tournamentId=bda92870-621e-47b0-bc3c-3602c5c26f55",
            ),
        };
        let r = self.api().client.request(endpoint);
        match r {
            Ok(r) => {
                let projected = match slot {
                    0 => baal_service_runtime::market::account_fills(&r),
                    1 => baal_service_runtime::market::account_pnl(&r),
                    _ => baal_service_runtime::market::account_orders(
                        &r.get("data").and_then(|d| d.get("data")).cloned().unwrap_or(V::Null),
                        r.get("receivedAt").and_then(V::as_str).unwrap_or(""),
                        false,
                    ),
                };
                match projected {
                    Ok(v) => {
                        self.account_cache[kind] = v;
                        if let Some(e) = self.account_cache.get_mut("errors").and_then(V::as_object_mut) {
                            e.remove(kind);
                        }
                        self.publish_account()?;
                    }
                    Err(e) => {
                        let mut err = RunnerError::msg(e);
                        err.projection_invalid = true;
                        return Err(err);
                    }
                }
            }
            Err(e) => {
                if self.ctx.stopped() {
                    return Ok(());
                }
                let re = RunnerError::from_svc(e);
                if re.manual_review || re.status == Some(401) || re.status == Some(403) {
                    return Err(re);
                }
                let reason = if let Some(s) = re.status {
                    format!("http_{}", s)
                } else {
                    "connection_unavailable".to_string()
                };
                let errors = self.account_cache.get("errors").cloned().unwrap_or(json!({}));
                let mut eo = errors.as_object().cloned().unwrap_or_default();
                eo.insert(kind.to_string(), json!({"at": iso_now(), "reason": reason}));
                self.account_cache["errors"] = V::Object(eo);
                self.publish_account()?;
            }
        }
        Ok(())
    }

    // collectHistory — runner.mjs:267-279
    fn collect_history(&mut self, limit: i64) -> R<()> {
        let inputs = self.scheduling_inputs()?;
        let book_plan = crate::book_scheduler::plan_book_reads(&self.scheduler, &self.scheduler_scope, &inputs, 4, 3, 2)
            .map_err(RunnerError::msg)?;
        let mut hist_inputs = inputs.clone();
        hist_inputs["candidateMarketIds"] = book_plan["entryMarketIds"].clone();
        let plan = crate::book_scheduler::plan_history_reads(&self.scheduler, &self.scheduler_scope, &hist_inputs, limit, 8, 4)
            .map_err(RunnerError::msg)?;
        self.rt_set("historyActiveMarketIds", plan["activeMarketIds"].clone());
        self.rt_set(
            "historyScheduling",
            json!({
                "mode": plan["allocationMode"], "at": plan["allocationAt"],
                "dueMarketCount": plan["dueMarketCount"],
                "receiptCooldownCount": plan["receiptCooldownMarketIds"].as_array().map(|a| a.len()).unwrap_or(0),
                "attemptCooldownCount": plan["attemptCooldownMarketIds"].as_array().map(|a| a.len()).unwrap_or(0),
                "selected": plan["reads"].as_array().map(|a| a.iter().map(|x| json!({"marketId":x["marketId"],"reason":x["reason"],"priority":x["priority"],"reservedDiscovery":x["reservedDiscovery"]})).collect::<Vec<V>>()).unwrap_or_default()
            }),
        );
        for spec in plan["reads"].as_array().cloned().unwrap_or_default() {
            if self.ctx.stopped() || self.ctx.cooldown() > now_ms() {
                break;
            }
            let market_id = spec["marketId"].as_str().unwrap_or("").to_string();
            self.record_attempt("history", &market_id, None, false)?;
            let response = self.api().client.request(
                &baal_service_runtime::history::history_endpoint(spec["exchangeId"].as_str().unwrap_or(""), &iso_now())
                    .map_err(RunnerError::msg)?,
            );
            match response {
                Ok(response) => {
                    let h = baal_service_runtime::history::normalize_history(&response, &spec);
                    match h {
                        Ok(h) => {
                            self.commit(json!({
                                "type":"market_history","clockAt":h["receivedAt"],"history":h,
                                "allocation":{"mode":plan["allocationMode"],"at":plan["allocationAt"],"reason":spec["reason"],"priority":spec["priority"],"reservedDiscovery":spec["reservedDiscovery"]}
                            }))?;
                            let mut rt = self.rt_get("history");
                            if !rt.is_object() {
                                rt = json!({});
                            }
                            rt["lastAt"] = h["receivedAt"].clone();
                            rt["lastExchange"] = spec["exchangeId"].clone();
                            rt["lastCandleCount"] = json!(h["candles"].as_array().map(|a| a.len()).unwrap_or(0));
                            rt["successful"] = json!(rt["successful"].as_i64().unwrap_or(0) + 1);
                            self.rt_set("history", rt);
                        }
                        Err(e) => {
                            let d = &response["data"];
                            let mut rt = self.rt_get("history");
                            if !rt.is_object() {
                                rt = json!({});
                            }
                            rt["validation"] = json!({
                                "scopeMatches": d["exchangeId"] == spec["exchangeId"] && d["marketId"] == spec["marketId"],
                                "resolutionMatches": d["resolution"].as_str() == Some("1m"),
                                "candleCount": d["candles"].as_array().map(|a| a.len()),
                                "completeType": js_typeof(&d["coverage"]["complete"]),
                                "sequenceType": js_typeof(&d["coverage"]["projectedThroughSequence"]),
                                "fromValid": d["from"].as_str().and_then(parse_iso_ms).map(|x| x.is_finite()).unwrap_or(false),
                                "toValid": d["to"].as_str().and_then(parse_iso_ms).map(|x| x.is_finite()).unwrap_or(false),
                                "firstCandleTypes": if d["candles"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
                                    let c = &d["candles"][0];
                                    let mut m = serde_json::Map::new();
                                    for k in ["time","open","high","low","close","vwap","volume","tradeCount"] {
                                        m.insert(k.to_string(), json!(js_typeof(&c[k])));
                                    }
                                    V::Object(m)
                                } else { V::Null },
                            });
                            self.rt_set("history", rt);
                            let mut err = RunnerError::msg(e);
                            if [
                                "Invalid historical response scope",
                                "Invalid historical window",
                                "Invalid candle clock/count",
                                "Invalid candle price",
                                "Invalid OHLC bounds",
                            ]
                            .contains(&err.message.as_str())
                            {
                                err.history_validation = Some(err.message.clone());
                            }
                            if err.durable_recording_failed {
                                return Err(err);
                            }
                            if err.manual_review || err.status == Some(401) || err.status == Some(403) {
                                return Err(err);
                            }
                            let mut rt = self.rt_get("history");
                            rt["lastErrorAt"] = json!(iso_now());
                            rt["lastError"] = json!(err.status.map(|s| format!("http_{}", s)).unwrap_or_else(|| err.history_validation.clone().unwrap_or_else(|| "history_unavailable_or_invalid".into())));
                            self.rt_set("history", rt);
                        }
                    }
                }
                Err(e) => {
                    let err = RunnerError::from_svc(e);
                    if self.ctx.stopped() {
                        break;
                    }
                    if err.durable_recording_failed {
                        return Err(err);
                    }
                    if err.manual_review || err.status == Some(401) || err.status == Some(403) {
                        return Err(err);
                    }
                    let mut rt = self.rt_get("history");
                    if !rt.is_object() {
                        rt = json!({});
                    }
                    rt["lastErrorAt"] = json!(iso_now());
                    rt["lastError"] = json!(err.status.map(|s| format!("http_{}", s)).unwrap_or_else(|| err.history_validation.clone().unwrap_or_else(|| "history_unavailable_or_invalid".into())));
                    self.rt_set("history", rt);
                }
            }
        }
        Ok(())
    }

    // pauseUntilNextRead — api.pause(ms)
    fn pause_until_next_read(&self, ms: f64) -> R<()> {
        let signal = self.ctx.signal.clone();
        let sleep: Arc<dyn Fn(f64) + Send + Sync> = Arc::new(|d: f64| {
            if d > 0.0 {
                std::thread::sleep(std::time::Duration::from_millis(d as u64));
            }
        });
        match self.api().pause(ms, &signal, &sleep) {
            Ok(()) => Ok(()),
            Err(e) => {
                if !self.ctx.stopped() {
                    Err(RunnerError::from_svc(e))
                } else {
                    Ok(())
                }
            }
        }
    }

    // persistStatus — runner.mjs:262
    fn persist_status(&self) -> Result<V, String> {
        let r = crate::model::report(&self.state, &self.c, &iso_now());
        if let Some(rt) = self.realtime.borrow().as_ref() {
            let mut s = rt.status();
            s["pid"] = json!(std::process::id());
            crate::collector_status_writer::write_collector_status(&self.runtime_dir.join("realtime-stats.json"), &s)?;
        }
        let mut r2 = r.clone();
        let mut rt = self.ctx.runtime.borrow().clone();
        rt["readRequests"] = json!(self.api.borrow().as_ref().map(|a| a.client.requests()).unwrap_or(0));
        r2["runtime"] = rt;
        crate::collector_status_writer::write_collector_status(&self.runtime_dir.join("stats.json"), &r2)?;
        Ok(r)
    }

    // controllerForActivation — runner.mjs:148-153
    fn controller_for_activation(&self, a: &V) -> Result<CompetitionKind, String> {
        if a["strategyId"].as_str() != Some("historical-median-receipt-v1") {
            return Ok(CompetitionKind::Settlement);
        }
        let file = self
            .runtime_dir
            .join("competition-runs")
            .join(a["runId"].as_str().unwrap_or(""))
            .join("median-entry-cap-revision.json");
        if !file.exists() {
            return Ok(CompetitionKind::Median);
        }
        let r = read_json_file(&file).map_err(|e| e)?;
        if r["schemaVersion"].as_i64() != Some(1)
            || r["kind"].as_str() != Some("same_run_median_entry_cap_revision")
            || r["ownerId"].as_str() != Some("Codex")
            || r["runId"].as_str() != a["runId"].as_str()
            || r["tournamentId"].as_str() != a["tournamentId"].as_str()
            || r["activationHash"].as_str() != Some(payload_hash(a).map_err(|e| e.to_string())?.as_str())
            || r["priorBankHash"].as_str() != a["strategyBankHash"].as_str()
        {
            return Err("MEDIAN_REVISION_SAME_RUN_BINDING".into());
        }
        Ok(CompetitionKind::MedianClock)
    }

    // hotArmMedian — runner.mjs:412-423
    fn hot_arm_median(&mut self) -> R<()> {
        if self.ctx.stopped() || self.ctx.manual_review() || !self.production_activation_file.exists() {
            return Ok(());
        }
        let next = read_json_file(&self.production_activation_file).map_err(|e| e)?;
        if next["enabled"] != V::Bool(true) || next["strategyId"].as_str() != Some("historical-median-receipt-v1") {
            return Ok(());
        }
        if let Some(comp) = self.competition.borrow().as_ref() {
            if comp.activation().is_object()
                && digest(&next) == digest(&comp.activation())
            {
                return Ok(());
            }
        }
        baal_execution::production::validate_production_activation(&next).map_err(RunnerError::msg)?;
        let budget = crate::transport_network_budget::load_network_budget(&self.runtime_dir.join("coordinated-network-budget.json"))
            .map_err(RunnerError::msg)?;
        let keys: Vec<&str> = if next["authorityKind"].as_str() == Some("direct_human_assignment") {
            vec!["authorityKind", "assignmentCommentId"]
        } else {
            vec!["releaseCommentId"]
        };
        let budget_ok = baal_execution::production::production_handoff_current(&self.runtime_dir, &next)
            && budget.as_ref().and_then(|b| b["schemaVersion"].as_i64()) == Some(4)
            && ["handoffId", "ackCommentId"]
                .iter()
                .chain(keys.iter())
                .all(|k| budget.as_ref().map(|b| &b[*k]) == Some(&next[*k]))
            && self.api.borrow().as_ref().map(|a| a.client.interval) == Some(1000)
            && self.api.borrow().as_ref().map(|a| a.client.write_interval) == Some(2500)
            && self.api.borrow().as_ref().map(|a| a.completion_pacing) == Some(true)
            && self.api.borrow().as_ref().map(|a| a.client.read_only) != Some(true);
        if !budget_ok {
            return Err(RunnerError::msg("MEDIAN_HOT_ARM_BUDGET_OR_HANDOFF"));
        }
        if let Some(host) = self.paired_live_host.borrow().as_ref() {
            if js_truthy(host.status().get("initialized")) {
                return Err(RunnerError::msg("MEDIAN_HOT_ARM_PAIRED_REGISTRY_BOUND"));
            }
        }
        if self.competition.borrow().is_some() {
            let (activation, state) = {
                let c = self.competition.borrow();
                (c.as_ref().unwrap().activation().clone(), c.as_ref().unwrap().state().clone())
            };
            if !activation.is_object()
                || now_ms() < parse_iso_ms(activation["entryExpiresAt"].as_str().unwrap_or("")).unwrap_or(0.0)
                || state["intents"]
                    .as_object()
                    .map(|m| m.values().any(|i| i["status"].as_str() != Some("closed")))
                    .unwrap_or(false)
                || state["owned"]
                    .as_object()
                    .map(|m| m.values().any(|p| p["quantity"].as_f64().unwrap_or(0.0) != 0.0))
                    .unwrap_or(false)
            {
                return Err(RunnerError::msg("MEDIAN_HOT_ARM_PRIOR_RISK"));
            }
            if let Some(mut c) = cell_take(&self.competition) {
                let _ = c.close();
            }
        }
        let arm_hash = digest(&next);
        let kind = self.controller_for_activation(&next).map_err(RunnerError::msg)?;
        let comp = self.new_competition(kind, &next)?;
        *self.competition.borrow_mut() = Some(comp);
        let attempt = (|| -> R<()> {
            if let Some(c) = self.competition.borrow_mut().as_mut() {
                c.initialize_median_reference()?;
            }
            let current = read_json_file(&self.production_activation_file).map_err(|e| e)?;
            if digest(&current) != arm_hash {
                return Err(RunnerError::msg("MEDIAN_HOT_ARM_ACTIVATION_CHANGED"));
            }
            if let Some(c) = self.competition.borrow_mut().as_mut() {
                let bootstrap = c.base_mut().bootstrap();
                if bootstrap["complete"] != V::Bool(true) {
                    return Err(RunnerError::msg("MEDIAN_HOT_ARM_RECONCILIATION"));
                }
            }
            self.rt_set(
                "productionArm",
                json!({"status":"armed","runId":next["runId"],"activationHash":arm_hash,"at":iso_now()}),
            );
            Ok(())
        })();
        if let Err(e) = attempt {
            if let Some(c) = self.competition.borrow_mut().as_mut() {
                c.failure(&json!({"message": e.message}));
            }
            return Err(e);
        }
        Ok(())
    }

    fn new_competition(&self, kind: CompetitionKind, activation: &V) -> Result<Competition, String> {
        let ctx = self.ctx.clone();
        let paired_cell = AssertSend(self.paired_live_host.clone());
        let options = baal_execution::controller::ControllerOptions {
            runtime_dir: self.runtime_dir.clone(),
            api_dispatch: {
                let api = AssertSend(self.api.clone());
                let fences = AssertSend(self.fences.clone());
                Rc::new(move |spec: &V, bd: Option<&TGuard>, bn: Option<&TGuard>| -> Result<V, String> {
                    api.get().borrow()
                        .as_ref()
                        .map(|a| a.dispatch_transport(spec, bd, bn, &fences.get()).map_err(|e| e.message))
                        .unwrap_or(Err("STOPPED".into()))
                })
            },
            universe: self.universe.clone(),
            config: self.c.clone(),
            is_stopped: {
                let ctx = ctx.clone();
                Rc::new(move || {
                    ctx.stopped()
                        || paired_cell.get().borrow().as_ref().map(|h| h.blocks_legacy_writes()).unwrap_or(false)
                })
            },
            activation: activation.clone(),
            now: Rc::new(now_ms),
        };
        match kind {
            CompetitionKind::Settlement => {
                SettlementCompetitionController::new(options).map(Competition::Settlement)
            }
            CompetitionKind::Median => {
                MedianCompetitionController::new(options, self.workspace.clone()).map(Competition::Median)
            }
            CompetitionKind::MedianClock => {
                Ok(Competition::MedianClock(MedianCompetitionControllerV2Clock::wrap(
                    MedianCompetitionControllerV2::new(options, self.workspace.clone())?,
                )))
            }
        }
    }

    // authorizeDispatch — runner.mjs:322
    fn authorize(&self, spec: &V) -> bool {
        if let Some(host) = self.paired_live_host.borrow().as_ref() {
            if host.authorizes(spec, &SpecGuards::None) {
                return true;
            }
        }
        let method_get = spec.get("method").and_then(V::as_str) == Some("GET");
        let allows_legacy = self
            .paired_live_host
            .borrow()
            .as_ref()
            .map(|h| h.allows_legacy(spec))
            .unwrap_or(true);
        if !(method_get || allows_legacy) {
            return false;
        }
        if let Some(m) = self.median_preflight.borrow().as_ref() {
            if m.allowed_route(spec) {
                return true;
            }
        }
        if let Some(p) = self.paired_candidate.borrow().as_ref() {
            if p.authorizes(spec) {
                return true;
            }
        }
        if let Some(p) = self.production_metadata.borrow().as_ref() {
            if p.authorizes(spec) {
                return true;
            }
        }
        if let Some(t) = self.trade_tape.borrow().as_ref() {
            if t.authorizes(spec) {
                return true;
            }
        }
        let tid = self.c["tournamentId"].as_str().unwrap_or("");
        if self.contract_evidence.borrow().is_some()
            && contract_evidence_authorizes(spec, &self.contract_evidence_scope.borrow(), tid)
        {
            return true;
        }
        if self.basket_relationships.borrow().is_some() && basket_authorizes(spec, tid) {
            return true;
        }
        if let Some(rt) = self.realtime.borrow().as_ref() {
            if rt.authorizes(spec) {
                return true;
            }
        }
        if let Some(c) = self.competition.borrow().as_ref() {
            if c.transport().authorize_request(spec).unwrap_or(false) {
                return true;
            }
        }
        false
    }

    // The paired-host cascade — runner.mjs:344
    fn open_paired_live_host_if_active(&mut self) -> R<()> {
        if self.scoped_receipts.is_none() {
            return Ok(());
        }
        let ctx = self.ctx.clone();
        let api_cell = AssertSend(self.api.clone());
        let fences_cell = AssertSend(self.fences.clone());
        let competition_cell = AssertSend(self.competition.clone());
        let paired_cell = AssertSend(self.paired_live_host.clone());
        let runtime_dir = self.runtime_dir.clone();
        let workspace = self.workspace.clone();
        let coverage_cell = self.coverage.clone();
        let universe = self.universe.clone();
        let c_config = self.c.clone();
        let coverage_rc = Rc::new(RefCell::new(coverage_cell));
        let coverage_for_host = coverage_rc.clone();

        // Deps shared by every host flavor.
        let scoped = self.scoped_receipts.clone();
        let receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>> = Rc::new(move |response: &V, opts: &V| {
            match &scoped {
                Some(s) => s.receipt_of(response, opts),
                None => Err("PAIRED_SCOPED_RECEIPTS".into()),
            }
        });
        let get_account_lease: Rc<dyn Fn() -> V> = {
            let competition_cell = AssertSend(competition_cell.get().clone());
            Rc::new(move || {
            let comp = competition_cell.get().borrow();
            match comp.as_ref() {
                None => V::Null,
                Some(c) => {
                    let _ = c.ledger().borrow().snapshot();
                    let record = std::fs::read_to_string(c.dir().join("execution-owner.lock"))
                        .ok()
                        .and_then(|s| serde_json::from_str::<V>(&s).ok())
                        .unwrap_or(V::Null);
                    if record["ownerId"].as_str() != Some("Codex")
                        || record["pid"].as_u64() != Some(std::process::id() as u64)
                        || record["scope"].as_str() != Some(c_config["tournamentId"].as_str().unwrap_or(""))
                        || record["runId"].as_str() != c.activation()["runId"].as_str()
                    {
                        return V::Null; // caller maps Err — JS throws PAIRED_HOST_ACCOUNT_LEASE
                    }
                    json!({"ownerId":"Codex","tournamentId":c_config["tournamentId"],"leaseId": payload_hash(&record).unwrap_or_default()})
                }
            }
            })
        };
        let get_other_state: Rc<dyn Fn() -> V> = Rc::new(move || {
            let comp = competition_cell.get().borrow();
            match comp.as_ref() {
                None => V::Null,
                Some(c) => {
                    let _ = c.assert_ledgers();
                    c.state()
                }
            }
        });
        let can_op = ctx.clone();
        let can_operate: Rc<dyn Fn() -> bool> = Rc::new(move || can_op.can_operate_no_exec());
        let hr = ctx.clone();
        let has_read_headroom: Rc<dyn Fn(i64) -> bool> = Rc::new(move |n| hr.has_read_headroom(n));
        let get_read_hint: Rc<dyn Fn() -> V> = {
            let coverage_for_host = coverage_for_host.clone();
            let universe = universe.clone();
            let runtime_dir = runtime_dir.clone();
            Rc::new(move || {
            let coverage = coverage_for_host.borrow().clone().unwrap_or(V::Null);
            let file = runtime_dir.join("owned-no-profit-request.json");
            if coverage.is_null() || !file.exists() {
                return V::Null;
            }
            let request: V = std::fs::read_to_string(&file).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(V::Null);
            let quote = coverage["quotes"][request["exchangeId"].as_str().unwrap_or("")].clone();
            if quote.is_null() {
                return V::Null;
            }
            json!({
                "basis":"documented_bulk_best_ask","tournamentId":universe["tournamentId"],
                "universeHash":coverage["universeHash"],"marketId":quote["marketId"],
                "exchangeId":quote["exchangeId"],"bestAsk":quote["bestAsk"],
                "requestStartedAt":quote["requestStartedAt"],"receivedAt":quote["receivedAt"],
                "sourceHash": payload_hash(&json!({"universeHash":coverage["universeHash"],"quote":quote})).unwrap_or_default(),
            })
            })
        };
        let get_pair_read_hints: Rc<dyn Fn() -> V> = {
            let coverage_for_host = coverage_for_host.clone();
            let universe = universe.clone();
            Rc::new(move || {
            let coverage = coverage_for_host.borrow().clone().unwrap_or(V::Null);
            if coverage.is_null() {
                return V::Null;
            }
            let mut captures: Vec<V> = Vec::new();
            if let Some(qs) = coverage["quotes"].as_object() {
                for quote in qs.values() {
                    let mut q = quote.as_object().cloned().unwrap_or_default();
                    q.insert("knownAt".into(), quote["receivedAt"].clone());
                    q.insert(
                        "sourceHash".into(),
                        json!(payload_hash(&json!({"universeHash": coverage["universeHash"], "quote": quote})).unwrap_or_default()),
                    );
                    captures.push(V::Object(q));
                }
            }
            json!({
                "tournamentId": universe["tournamentId"], "universeHash": coverage["universeHash"],
                "quoteCaptures": captures,
                "sourceHash": payload_hash(&coverage).unwrap_or_default(),
            })
            })
        };
        let get_legacy_risk: Rc<dyn Fn() -> V> = {
            let rd = self.runtime_dir.clone();
            let comp_cell = AssertSend(self.competition.clone());
            Rc::new(move || {
                let other = comp_cell.get().borrow().as_ref().map(|c| c.state()).unwrap_or(V::Null);
                read_baal_scaled_legacy_risk(&rd, &other)
            })
        };
        let _ = (get_pair_read_hints.clone(), get_legacy_risk.clone()); // feed the scaled-host variants (unported)

        let deps = PairedLiveHostDeps {
            receipt_of: receipt_of.clone(),
            get_account_lease: get_account_lease.clone(),
            get_other_state: get_other_state.clone(),
            can_operate: can_operate.clone(),
            has_read_headroom: has_read_headroom.clone(),
            now: Rc::new(now_ms),
            workspace: workspace.clone(),
        };
        let scaled = self.runtime_dir.join("baal-scaled-live-request.json").exists();
        let neutral_sell = self.runtime_dir.join("neutral-sell-recovery-request.json").exists();
        let owned = self.runtime_dir.join("owned-no-profit-request.json").exists();
        let neutral = self.runtime_dir.join("paired-neutral-ack-recovery.json").exists();
        let v2 = self.runtime_dir.join("paired-live-v2-request.json").exists();
        let result: Result<Option<PairedHost>, String> = if scaled {
            // Unported cascade: openBaalOwnedAllReadHostV4/openBaalScaledLiveHostV3/V2/
            // V1 — fall back to the closest ported host (v1 open_paired_live_host).
            open_paired_live_host(&self.runtime_dir, self.paired_api.clone(), deps).map(|o| o.map(PairedHost::V1))
        } else if neutral_sell {
            // neutral_sell_recovery_host_v1 is unported — closest ported API:
            // the paired neutral recovery host (same recovery-family semantics).
            let d = PairedNeutralRecoveryDeps {
                receipt_of,
                get_account_lease,
                get_other_state,
                can_operate,
                has_read_headroom,
                now: Rc::new(now_ms),
                workspace: workspace.clone(),
            };
            open_paired_neutral_recovery_host(&self.runtime_dir, &self.universe, self.paired_api.clone(), d)
                .map(|o| o.map(PairedHost::Neutral))
        } else if owned {
            // owned_no_profit_host_v1 is unported — closest ported API: paired live v1.
            open_paired_live_host(&self.runtime_dir, self.paired_api.clone(), deps).map(|o| o.map(PairedHost::V1))
        } else if neutral {
            let d = PairedNeutralRecoveryDeps {
                receipt_of,
                get_account_lease,
                get_other_state,
                can_operate,
                has_read_headroom,
                now: Rc::new(now_ms),
                workspace: workspace.clone(),
            };
            open_paired_neutral_recovery_host(&self.runtime_dir, &self.universe, self.paired_api.clone(), d)
                .map(|o| o.map(PairedHost::Neutral))
        } else if v2 {
            open_paired_live_host_v2(&self.runtime_dir, &self.universe, self.paired_api.clone(), deps)
                .map(|o| o.map(|h| PairedHost::V2(Box::new(h))))
        } else {
            open_paired_live_host(&self.runtime_dir, self.paired_api.clone(), deps).map(|o| o.map(PairedHost::V1))
        };
        let host = result.map_err(RunnerError::msg)?;
        *self.paired_live_host.borrow_mut() = host;
        let _ = paired_cell;
        Ok(())
    }

    // startup basket-relationship + contract-pair captures — runner.mjs:391
    fn basket_contract_captures(&mut self) -> R<()> {
        let ctx = self.ctx.clone();
        let can_op_capture = { let c = ctx.clone(); move || c.can_operate_no_exec() };
        let capture = self
            .basket_relationships
            .borrow_mut()
            .as_mut()
            .unwrap()
            .capture(&can_op_capture, &(now_ms_i64 as fn() -> i64) as &dyn Fn() -> i64);
        let capture = match capture {
            Ok(c) => c,
            Err(e) => return Err(RunnerError::msg(e)),
        };
        baal_service_runtime::atomic::atomic_json(&self.runtime_dir.join("basket-relationships.json"), &capture)
            .map_err(RunnerError::msg)?;
        self.commit(json!({"type":"basket_relationship_capture","clockAt":capture["finishedAt"],"capture":capture}))?;
        self.rt_set(
            "basketRelationships",
            json!({
                "at":capture["finishedAt"],"complete":capture["complete"],"reason":capture["reasonCode"],
                "relationships":capture["relationships"].as_array().map(|a| a.len()).unwrap_or(0),
                "pages":capture["snapshots"].as_array().map(|a| a.len()).unwrap_or(0),
                "evidenceHash":capture["evidenceHash"],
            }),
        );
        if !self.ctx.stopped() && self.ctx.cooldown() <= now_ms() {
            let evidence = self.contract_evidence.borrow_mut().as_mut().unwrap().capture(&can_op_capture, &(now_ms_i64 as fn() -> i64) as &dyn Fn() -> i64);
            match evidence {
                Ok(evidence) => {
                    baal_service_runtime::atomic::atomic_json(&self.runtime_dir.join("contract-pair-evidence.json"), &evidence)
                        .map_err(RunnerError::msg)?;
                    self.commit(json!({"type":"contract_pair_evidence","clockAt":evidence["finishedAt"],"evidence":evidence}))?;
                    self.rt_set(
                        "contractPairEvidence",
                        json!({"at":evidence["finishedAt"],"validUntil":evidence["validUntil"],"evidenceHash":evidence["evidenceHash"],"orderEligible":false,"status":"captured"}),
                    );
                }
                Err(e) => {
                    let err = RunnerError::msg(e);
                    if err.durable_recording_failed || self.ctx.manual_review() || err.status == Some(401) || err.status == Some(403) {
                        return Err(err);
                    }
                    let reason = if let Some(s) = err.status {
                        format!("http_{}", s)
                    } else if is_contract_code(&err.message) {
                        err.message.clone()
                    } else {
                        "capture_unavailable".to_string()
                    };
                    self.rt_set(
                        "contractPairEvidence",
                        json!({"at":iso_now(),"status":if self.ctx.stopped(){"stopped"}else{"unavailable"},"orderEligible":false,"reason":reason}),
                    );
                }
            }
        }
        Ok(())
    }

    // /api/* dashboard handler — runner.mjs:349-355
    fn dashboard(&self) -> DashboardFn {
        let runtime = AssertSend(self.ctx.runtime.clone());
        let api_cell = AssertSend(self.api.clone());
        let competition_cell = AssertSend(self.competition.clone());
        let runtime_dir = self.runtime_dir.clone();
        let c = self.c.clone();
        let root = self.root.clone();
        let account_cache = AssertSend(Rc::new(RefCell::new(())));
        let _ = account_cache;
        let cache_cell: Rc<RefCell<V>> = Rc::new(RefCell::new(self.account_cache.clone()));
        // accountCache is refreshed each cycle; the dashboard must see the
        // current object, so share a pointer updated by Runner.
        let account_view_cell = AssertSend(cache_cell.clone());
        let universe = self.universe.clone();
        Arc::new(move |raw_path: &str| -> (u16, &'static str, Vec<u8>) {
            let path = raw_path.split('?').next().unwrap_or("/");
            match path {
                "/" | "/index.html" | "/dashboard.html" => {
                    let file = root.join("dashboard.html");
                    match std::fs::read(&file) {
                        Ok(b) => (200, "text/html; charset=utf-8", b),
                        Err(_) => (404, "application/json", json!({"error":"dashboard_unavailable"}).to_string().into_bytes()),
                    }
                }
                "/api/stats" => {
                    let primary = read_json_file(&runtime_dir.join("stats.json")).ok();
                    let legacy = read_json_file(&runtime_dir.join("legacy-stats.json")).ok().unwrap_or(V::Null);
                    let mut report = primary.clone().unwrap_or(V::Null);
                    if !js_truthy(Some(&report)) {
                        let mut merged = legacy.clone();
                        merged["feeScenario"] = json!("historical_fee_stress");
                        merged["baselineWorkerStatus"] = json!("unavailable");
                        report = merged;
                    }
                    let baseline =
                        crate::worker_health::baseline_view(&report, &legacy, &runtime.get().borrow(), now_ms())
                            .unwrap_or(V::Null);
                    if js_truthy(Some(&baseline)) {
                        report["baseline"] = baseline;
                    }
                    let mut out = report.clone();
                    let mut rt = out["runtime"].as_object().cloned().unwrap_or_default();
                    rt.insert(
                        "readRequests".into(),
                        json!(api_cell.get().borrow().as_ref().map(|a| a.client.requests()).unwrap_or(0)),
                    );
                    out["runtime"] = V::Object(rt);
                    (200, "application/json", out.to_string().into_bytes())
                }
                "/api/account" => {
                    let cache = account_view_cell.get().borrow().clone();
                    let collector_status = read_json_file(&runtime_dir.join("stats.json")).unwrap_or(V::Null);
                    let identities = read_json_file(&runtime_dir.join("account-identities.json")).unwrap_or(V::Null);
                    let view = crate::account::account_view(&cache, collector_status["updatedAt"].as_str().unwrap_or(""), &iso_now(), &identities);
                    (200, "application/json", view.to_string().into_bytes())
                }
                "/api/execution" => {
                    let comp = competition_cell.get().borrow();
                    if comp.is_none() {
                        let file = runtime_dir.join("competition-stats.json");
                        let v = std::fs::read_to_string(&file)
                            .ok()
                            .and_then(|s| serde_json::from_str::<V>(&s).ok())
                            .unwrap_or(json!({"mode":"official_quant_execution","status":"not_enabled","ordersSubmitted":0}));
                        return (200, "application/json", v.to_string().into_bytes());
                    }
                    let snapshot = comp.as_ref().unwrap().snapshot(None);
                    (200, "application/json", snapshot.to_string().into_bytes())
                }
                "/api/realtime" => {
                    let file = runtime_dir.join("realtime-stats.json");
                    let v = std::fs::read_to_string(&file)
                        .ok()
                        .and_then(|s| serde_json::from_str::<V>(&s).ok())
                        .unwrap_or(json!({"schemaVersion":1,"state":"disabled"}));
                    (200, "application/json", v.to_string().into_bytes())
                }
                "/api/passive" | "/api/basket" | "/api/ml" | "/api/dip" | "/api/dip-scale"
                | "/api/dip-responsive" | "/api/shock" => {
                    let name = path.trim_start_matches("/api/");
                    let file = runtime_dir.join(format!("{}-stats.json", name));
                    match read_json_file(&file) {
                        Ok(stats) => {
                            let wh = crate::worker_health::worker_health(&stats, now_ms(), 60000.0);
                            let mut out = stats;
                            out["workerHealth"] = wh;
                            (200, "application/json", out.to_string().into_bytes())
                        }
                        Err(_) => (
                            503,
                            "application/json",
                            json!({"error":"paper_module_unavailable","module":name}).to_string().into_bytes(),
                        ),
                    }
                }
                "/api/breadth" => {
                    let coverage = read_json_file(&runtime_dir.join("coverage.json")).ok();
                    let universe_size = universe["exchanges"].as_array().map(|a| a.len()).unwrap_or(0);
                    let mut out = match &coverage {
                        Some(c) => c.clone(),
                        None => json!({"universeSize": universe_size, "quotes": {}}),
                    };
                    let _ = universe_size;
                    out["collectionAgeMs"] = coverage
                        .as_ref()
                        .and_then(|c| c["updatedAt"].as_str().and_then(|s| parse_iso_ms(s)))
                        .map(|t| json!(now_ms() - t))
                        .unwrap_or(V::Null);
                    (200, "application/json", out.to_string().into_bytes())
                }
                _ => (404, "application/json", json!({"error":"not_found"}).to_string().into_bytes()),
            }
        }) as DashboardFn
    }

    // the main collection cycle loop — runner.mjs:425-553
    fn main_loop(&mut self, max_cycles: f64) -> R<()> {
        while !self.ctx.stopped() && (self.rt_get("cycles").as_f64().unwrap_or(0.0)) < max_cycles {
            if let Err(_) = self.hot_arm_median() {
                self.rt_set("productionArm", json!({"status":"blocked","at":iso_now()}));
            }
            let paired_auxiliary_owner = self
                .paired_candidate
                .borrow_mut()
                .as_mut()
                .map(|p| p.pending().unwrap_or(V::Bool(false)) == V::Bool(true))
                .unwrap_or(false);
            let cycle_start = now_ms();
            let checkpoint = self.state.clone();
            let mut events: Vec<V> = Vec::new();
            let mut successful: i64 = 0;
            let mut cycle_committed = false;
            let mut profit_sprint_applied = false;
            let mut cycle_risk_market_ids: Vec<String> = Vec::new();
            let cycle_result: R<()> = (|| {
                if self.dip_bank.is_some() {
                    let maker_status = self.no_maker.borrow().as_ref().map(|h| h.status().unwrap_or(V::Null));
                    let stable_statuses: Vec<(String, V, V)> = self
                        .stable_hosts()
                        .iter()
                        .filter_map(|(k, c)| {
                            let h = c.borrow();
                            h.as_ref().map(|h| (k.to_string(), self.stable_status(k, h).unwrap_or(V::Null), h.bank().clone()))
                        })
                        .collect();
                    let mut maker_focus_args: Vec<V> = vec![maker_status.clone().unwrap_or(V::Null)];
                    maker_focus_args.extend(stable_statuses.iter().map(|(_, s, _)| s.clone()));
                    let maker_focus = crate::stable_maker_host_v1::merge_maker_tape_focus(&maker_focus_args)
                        .map_err(RunnerError::msg)?;
                    let old_maker_active = self.no_maker.borrow().is_some()
                        && !js_truthy(maker_status.clone().unwrap_or(V::Null).get("failed"))
                        && (maker_status.clone().unwrap_or(V::Null)["riskMarketIds"]
                            .as_array()
                            .map(|a| !a.is_empty())
                            .unwrap_or(false)
                            || now_ms()
                                < parse_iso_ms(
                                    self.no_maker.borrow().as_ref().unwrap().bank()["reviewExpiresAt"].as_str().unwrap_or(""),
                                )
                                .unwrap_or(0.0));
                    let stable_active = stable_statuses.iter().any(|(_, status, bank)| {
                        !js_truthy(status.get("failed"))
                            && (status["riskMarketIds"].as_array().map(|a| !a.is_empty()).unwrap_or(false)
                                || now_ms()
                                    < parse_iso_ms(bank["entryExpiresAt"].as_str().unwrap_or("")).unwrap_or(0.0))
                    });
                    let maker_active = old_maker_active || stable_active;
                    let early_probability_focus = crate::probability_read_focus::merge_probability_read_focus(&[
                        crate::probability_read_focus::read_probability_read_focus(
                            &self.runtime_dir,
                            &self.universe,
                            now_ms(),
                            "probability-receipt-active.json",
                            &self.workspace,
                        )
                        .unwrap_or(V::Null),
                        crate::probability_read_focus::read_probability_read_focus(
                            &self.runtime_dir,
                            &self.universe,
                            now_ms(),
                            "calibrated-poll-paper-active.json",
                            &self.workspace,
                        )
                        .unwrap_or(V::Null),
                        crate::probability_read_focus::read_probability_read_focus(
                            &self.runtime_dir,
                            &self.universe,
                            now_ms(),
                            "primary-breadth-paper-active.json",
                            &self.workspace,
                        )
                        .unwrap_or(V::Null),
                        self.settlement_paper.borrow_mut().as_mut().and_then(|h| h.focus().ok()).unwrap_or(V::Null),
                        self.settlement_portfolio.borrow().as_ref().map(|h| h.focus()).unwrap_or(V::Null),
                        self.dual_no_paper.borrow().as_ref().and_then(|h| h.focus().ok()).unwrap_or(V::Null),
                        self.dual_no_paper_v2.borrow().as_ref().and_then(|h| h.focus().ok()).unwrap_or(V::Null),
                    ]).unwrap_or(V::Null);
                    let early_risk = crate::profit_sprint_read_plan::scoped_risk_market_ids(
                        &self
                            .competition
                            .borrow()
                            .as_ref()
                            .map(|c| c.managed_market_ids().into_iter().collect::<Vec<String>>())
                            .unwrap_or_default(),
                        if js_truthy(Some(&early_probability_focus)) {
                            Some(&early_probability_focus)
                        } else {
                            None
                        },
                    )
                    .map_err(RunnerError::msg)?;
                    profit_sprint_applied = crate::profit_sprint_read_plan::read_profit_sprint(&self.runtime_dir, now_ms())
                        .unwrap_or(None)
                        .is_some()
                        && !early_risk.is_empty()
                        && self.ctx.marker("COUPLED_BULK_ENTRY_DRAIN")
                        && self.no_maker.borrow().is_none()
                        && self.baal_pair.borrow().is_none()
                        && self.rt_get("coupledBulk")["liabilitiesUnverified"] != V::Bool(true)
                        && crate::profit_sprint_read_plan::passive_hosts_completely_flat(&V::Array(
                            stable_statuses.iter().map(|(_, _, s)| s.clone()).collect(),
                        ));
                    let attention_budget = if paired_auxiliary_owner {
                        0
                    } else if profit_sprint_applied {
                        if self.production_metadata.borrow().as_ref().and_then(|p| p.pending().ok().flatten()).is_some() {
                            1
                        } else {
                            0
                        }
                    } else if self.official_sync {
                        1
                    } else {
                        2
                    };
                    let mut auxiliary = self
                        .baal_read_attention
                        .borrow_mut()
                        .as_mut()
                        .map(|a| a.reserve(&json!(attention_budget), !profit_sprint_applied && maker_active))
                        .transpose()
                        .ok()
                        .flatten()
                        .unwrap_or(json!({}));
                    self.rt_set("auxiliaryReadAllocation", auxiliary.clone());
                    self.rt_set(
                        "profitSprint",
                        json!({"enabled":profit_sprint_applied,"reason":if profit_sprint_applied{"bounded_source_bound_risk_priority"}else{"normal_or_liabilities_retained"},"executionAuthority":false}),
                    );
                    if self
                        .production_metadata
                        .borrow()
                        .as_ref()
                        .map(|p| p.pending().ok().flatten().is_some())
                        .unwrap_or(false)
                        && (js_truthy(auxiliary.get("historyReads")) && auxiliary["historyReads"].as_i64().unwrap_or(0) > 0
                            || js_truthy(auxiliary.get("tapeReads")) && auxiliary["tapeReads"].as_i64().unwrap_or(0) > 0)
                        && !self.ctx.stopped()
                        && self.ctx.cooldown() <= now_ms()
                    {
                        self.production_metadata.borrow().as_ref().unwrap().consume_one().map_err(RunnerError::msg)?;
                        if auxiliary["historyReads"].as_i64().unwrap_or(0) > 0 {
                            auxiliary["historyReads"] = json!(auxiliary["historyReads"].as_i64().unwrap_or(0) - 1);
                        } else {
                            auxiliary["tapeReads"] = json!(auxiliary["tapeReads"].as_i64().unwrap_or(0) - 1);
                        }
                        self.rt_set(
                            "productionMetadata",
                            json!({"status":"requested_contract_read_completed","executionAuthority":false,"additionalReadSlots":0}),
                        );
                    }
                    if auxiliary["tapeReads"].as_i64().unwrap_or(0) > 0
                        && self.trade_tape.borrow().is_some()
                        && !self.ctx.stopped()
                        && self.ctx.cooldown() <= now_ms()
                    {
                        let tape_result: Result<V, RunnerError> = (|| {
                            let mut active_banks: Vec<V> = Vec::new();
                            if old_maker_active {
                                if let Some(h) = self.no_maker.borrow().as_ref() {
                                    active_banks.push(h.bank().clone());
                                }
                            }
                            for (_, status, bank) in &stable_statuses {
                                if !js_truthy(status.get("failed"))
                                    && (status["riskMarketIds"].as_array().map(|a| !a.is_empty()).unwrap_or(false)
                                        || now_ms()
                                            < parse_iso_ms(bank["entryExpiresAt"].as_str().unwrap_or(""))
                                                .unwrap_or(0.0))
                                {
                                    active_banks.push(bank.clone());
                                }
                            }
                            let focus = crate::trade_tape_focus::trade_tape_focus(
                                &self.universe,
                                &json!({
                                    "activeBanks": active_banks,
                                    "riskMarketIds": maker_focus["riskMarketIds"],
                                    "recoveryWindows": maker_focus["projectionRecoveryRequests"],
                                }),
                            )
                            .map_err(RunnerError::msg)?;
                            let api = AssertSend(self.api.clone());
                            let fences = AssertSend(self.fences.clone());
                            let mut dispatch = move |spec: &V| -> Result<V, String> {
                                api.get().borrow().as_ref().map(|a| a.dispatch_with(spec, &fences.get())).unwrap_or(Err("STOPPED".into()))
                            };
                            self.trade_tape
                                .borrow_mut()
                                .as_mut()
                                .unwrap()
                                .collect(
                                    &mut dispatch,
                                    focus["marketIds"].as_array().map(|a| a.as_slice()).unwrap_or(&[]),
                                    focus["priorityMarketIds"].as_array().map(|a| a.as_slice()).unwrap_or(&[]),
                                    focus["recoveryWindows"].as_array().map(|a| a.as_slice()).unwrap_or(&[]),
                                )
                                .map_err(RunnerError::msg)
                        })();
                        match tape_result {
                            Ok(v) => self.rt_set("tradeTape", v),
                            Err(e) => {
                                if e.durable_recording_failed || self.ctx.manual_review() || e.status == Some(401) || e.status == Some(403) {
                                    return Err(e);
                                }
                                if !self.ctx.stopped() {
                                    let mut st = self
                                        .trade_tape
                                        .borrow()
                                        .as_ref()
                                        .map(|t| t.status())
                                        .unwrap_or(json!({}));
                                    st["status"] = json!("degraded");
                                    st["lastErrorAt"] = json!(iso_now());
                                    st["reason"] = json!(if let Some(s) = e.status {
                                        format!("http_{}", s)
                                    } else if is_trade_tape_code(&e.message) {
                                        e.message.clone()
                                    } else {
                                        "trade_tape_unavailable".to_string()
                                    });
                                    self.rt_set("tradeTape", st);
                                }
                            }
                        }
                    }
                    if auxiliary["historyReads"].as_i64().unwrap_or(0) > 0 {
                        self.collect_history(auxiliary["historyReads"].as_i64().unwrap_or(0))?;
                    }
                }
                if self.official_sync && !self.ctx.stopped() && self.ctx.cooldown() <= now_ms() {
                    self.collect_account_extra()?;
                }
                let positions = self.api().client.request(&format!("/tournaments/{}/portfolio/positions", self.c["tournamentSlug"].as_str().unwrap_or(""))).map_err(RunnerError::from_svc)?;
                if positions["data"]["positions"].as_array().is_none() {
                    return Err(RunnerError::msg("Invalid real holdings response"));
                }
                if self.official_sync {
                    let projected = baal_service_runtime::market::account_positions(&positions);
                    match projected {
                        Ok(v) => {
                            self.account_cache["positions"] = v;
                            if let Some(e) = self.account_cache.get_mut("errors").and_then(V::as_object_mut) {
                                e.remove("positions");
                            }
                            self.publish_account()?;
                        }
                        Err(_) => {
                            let mut eo = self.account_cache["errors"].as_object().cloned().unwrap_or_default();
                            eo.insert("positions".into(), json!({"at":iso_now(),"reason":"unavailable_or_invalid"}));
                            self.account_cache["errors"] = V::Object(eo);
                            self.publish_account()?;
                        }
                    }
                }
                let orders = self.api().client.orders().map_err(RunnerError::from_svc)?;
                let excluded = baal_service_runtime::exclusions::exclusion_set(&positions["data"]["positions"], &V::Array(orders.clone()))
                    .map_err(RunnerError::msg)?;
                let eligibility_orders_at = iso_now();
                {
                    let r = self.eligibility_publication.borrow_mut().as_mut().map(|p| {
                        p.publish(&json!({"positions":positions,"orders":orders,"ordersAt":eligibility_orders_at}))
                    });
                    let publication_missing = self.eligibility_publication.borrow().is_none();
                    match r {
                        Some(Ok(v)) => self.rt_set("accountEligibility", v),
                        _ if publication_missing => {
                            if publication_missing {
                            } else {
                                return Err(RunnerError::durable("Account eligibility recording failed", true));
                            }
                        }
                        Some(Err(_)) => {
                            return Err(RunnerError::durable("Account eligibility recording failed", true));
                        }
                        None => {}
                    }
                }
                if self.official_sync {
                    match baal_service_runtime::market::account_orders(&V::Array(orders.clone()), &iso_now(), false) {
                        Ok(v) => {
                            self.account_cache["orders"] = v;
                            if let Some(e) = self.account_cache.get_mut("errors").and_then(V::as_object_mut) {
                                e.remove("orders");
                            }
                            self.publish_account()?;
                        }
                        Err(_) => {
                            let mut eo = self.account_cache["errors"].as_object().cloned().unwrap_or_default();
                            eo.insert("orders".into(), json!({"at":iso_now(),"reason":"unavailable_or_invalid"}));
                            self.account_cache["errors"] = V::Object(eo);
                            self.publish_account()?;
                        }
                    }
                }
                *self.candidate_account.borrow_mut() = json!({
                    "complete":true,"availableAt":eligibility_orders_at,
                    "snapshot":{"schemaVersion":1,"positionsAt":positions["receivedAt"],"ordersAt":eligibility_orders_at,"excludedExchangeIds":excluded.iter().cloned().collect::<Vec<String>>()}
                });
                if let Some(p) = self.party_pair_selector.borrow_mut().as_mut() {
                    let _ = p.after_account_refresh();
                }
                if paired_auxiliary_owner {
                    let pending = self
                        .paired_candidate
                        .borrow_mut()
                        .as_mut()
                        .map(|p| p.pending().unwrap_or(V::Bool(false)))
                        .unwrap_or(V::Bool(false));
                    if pending == V::Bool(true) {
                        if let Some(p) = self.paired_candidate.borrow_mut().as_mut() {
                            p.after_account_refresh().map_err(|e| RunnerError::msg(format!("{}", e)))?;
                        }
                        if let Some(p) = self.paired_candidate.borrow().as_ref() {
                            self.rt_set("pairedCandidate", p.status());
                        }
                    }
                }
                if let Some(h) = self.paired_live_host.borrow_mut().as_mut() {
                    let cand = self.paired_candidate.borrow().as_ref().map(|p| p.status()).unwrap_or(V::Null);
                    h.step(&cand).map_err(RunnerError::msg)?;
                    self.rt_set("pairedLive", h.status());
                }
                // Freeze one plan before metadata selection.
                let probability_focus = crate::probability_read_focus::merge_probability_read_focus(&[
                    crate::probability_read_focus::read_probability_read_focus(&self.runtime_dir, &self.universe, now_ms(), "probability-receipt-active.json", &self.workspace).unwrap_or(V::Null),
                    crate::probability_read_focus::read_probability_read_focus(&self.runtime_dir, &self.universe, now_ms(), "calibrated-poll-paper-active.json", &self.workspace).unwrap_or(V::Null),
                    crate::probability_read_focus::read_probability_read_focus(&self.runtime_dir, &self.universe, now_ms(), "primary-breadth-paper-active.json", &self.workspace).unwrap_or(V::Null),
                    self.settlement_paper.borrow_mut().as_mut().and_then(|h| h.focus().ok()).unwrap_or(V::Null),
                    self.settlement_portfolio.borrow().as_ref().map(|h| h.focus()).unwrap_or(V::Null),
                    self.dual_no_paper.borrow().as_ref().and_then(|h| h.focus().ok()).unwrap_or(V::Null),
                    self.dual_no_paper_v2.borrow().as_ref().and_then(|h| h.focus().ok()).unwrap_or(V::Null),
                ]).unwrap_or(V::Null);
                let bulk_focus = crate::bulk_dip_read_focus::read_bulk_read_focus(&self.runtime_dir, &self.universe, now_ms(), &self.workspace)
                    .unwrap_or(V::Null);
                let anchor_focus = crate::bid_anchor_read_focus::read_anchor_read_focus(&self.runtime_dir, &self.universe, now_ms(), &self.workspace)
                    .unwrap_or(V::Null);
                let diagnostic_focus = self.collect_diagnostic_focus().unwrap_or(V::Null);
                let paper_focus = crate::bulk_dip_read_focus::merge_paper_read_focus(&[anchor_focus.clone(), bulk_focus.clone(), probability_focus.clone(), diagnostic_focus]);
                let mut inputs = self.scheduling_inputs()?;
                let retained_paper_focus = if self.native_dip.borrow().is_some() {
                    paper_focus.clone()
                } else {
                    probability_focus.clone()
                };
                if let Some(markets) = retained_paper_focus["markets"].as_array() {
                    for x in markets {
                        let phase = x["phase"].as_str().unwrap_or("");
                        if ["held", "exit_pending", "entry_pending"].contains(&phase) {
                            let key = if phase == "entry_pending" { "pendingMarketIds" } else { "heldMarketIds" };
                            let mut ids = inputs[key].as_array().cloned().unwrap_or_default();
                            ids.push(x["marketId"].clone());
                            inputs[key] = V::Array(dedup(ids));
                        }
                    }
                }
                if self.native_dip.borrow().is_some() {
                    let members: std::collections::HashMap<String, String> = self.universe["exchanges"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|x| {
                            Some(format!("{}:YES", x["exchangeId"].as_str()?))
                                .zip(x["marketId"].as_str().map(String::from))
                        })
                        .collect();
                    let legacy = self.state["strategies"]["mean_reversion"].clone();
                    for (rows, key) in [(&legacy["positions"], "heldMarketIds"), (&legacy["pending"], "pendingMarketIds")] {
                        if let Some(map) = rows.as_object() {
                            for id in map.keys() {
                                let market_id = members
                                    .get(id)
                                    .cloned()
                                    .ok_or_else(|| RunnerError::msg("Unmapped original active paper risk"))?;
                                let mut ids = inputs[key].as_array().cloned().unwrap_or_default();
                                ids.push(json!(market_id));
                                inputs[key] = V::Array(dedup(ids));
                            }
                        }
                    }
                }
                let production_candidates: Vec<String> = self
                    .competition
                    .borrow()
                    .as_ref()
                    .map(|c| c.eligible_market_ids())
                    .unwrap_or_default();
                let owned_risk_ids: std::collections::BTreeSet<String> = {
                    let mut s: std::collections::BTreeSet<String> = self
                        .competition
                        .borrow()
                        .as_ref()
                        .map(|c| c.managed_market_ids())
                        .unwrap_or_default()
                        .into_iter()
                        .collect();
                    if probability_focus["ownedRiskReads"] == V::Bool(true) {
                        if let Some(markets) = probability_focus["markets"].as_array() {
                            for x in markets {
                                if x["phase"].as_str() == Some("held") {
                                    if let Some(id) = x["marketId"].as_str() {
                                        s.insert(id.to_string());
                                    }
                                }
                            }
                        }
                    }
                    s
                };
                inputs["excludedMarketIds"] = V::Array(
                    inputs["excludedMarketIds"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|id| !owned_risk_ids.contains(id.as_str().unwrap_or("")))
                        .collect(),
                );
                let excluded_market_ids_v: Vec<V> = self.universe["exchanges"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter(|x| excluded.contains(x["exchangeId"].as_str().unwrap_or("")))
                    .map(|x| x["marketId"].clone())
                    .collect();
                let base_plan = if self.native_dip.borrow().is_some() {
                    let screened: Vec<String> = {
                        let mut s = production_candidates.clone();
                        s.extend(
                            self.rt_get("marketHistoryScreen")["candidateMarketIds"]
                                .as_array()
                                .cloned()
                                .unwrap_or_default()
                                .iter()
                                .filter_map(|x| x.as_str().map(String::from)),
                        );
                        let mut d = s;
                        d.sort();
                        d.dedup();
                        d
                    };
                    crate::native_discovery_plan_v1::plan_native_discovery_books(
                        &self.scheduler,
                        &self.scheduler_scope,
                        &inputs,
                        &screened,
                        &excluded_market_ids_v.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<String>>(),
                        &owned_risk_ids.iter().cloned().collect::<Vec<String>>(),
                    )
                    .map_err(RunnerError::msg)?
                } else if self.dip_bank.is_some() {
                    let comparison_slots = if self.coupled_bulk.borrow().is_some()
                        || self.coupled_maker.borrow().is_some()
                        || self.stable_maker.borrow().is_some()
                        || self.stable_maker_broad.borrow().is_some()
                    {
                        0
                    } else {
                        2
                    };
                    crate::book_scheduler::plan_book_reads(&self.scheduler, &self.scheduler_scope, &inputs, 4, 3, comparison_slots)
                        .map_err(RunnerError::msg)?
                } else {
                    let mut reads: Vec<V> = self.c["marketIds"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .map(|m| json!({"marketId":m,"path":"sourceSnapshots"}))
                        .collect();
                    reads.extend(
                        self.depth_plan["marketIds"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                            .iter()
                            .map(|m| json!({"marketId":m,"path":"supplementalDepth"})),
                    );
                    json!({"reads":reads,"entryMarketIds":[]})
                };
                let planned = {
                    let p = self.basket_measurement_plan(base_plan, &excluded)?;
                    let p = self.comparison_sampling_plan(p)?;
                    crate::paired_package_read_plan::focus_paired_package_reads(
                        &p,
                        &crate::paired_package_read_plan::read_paired_package_focus(&self.runtime_dir, &self.universe, now_ms()),
                        Some(now_ms()),
                        self.scheduler.get("packageReadLastAttemptAt"),
                    )
                    .map_err(RunnerError::msg)?
                };
                let observation_focus = crate::edge_observation_plan::read_edge_observation_focus(
                    &self.runtime_dir,
                    &self.universe,
                    now_ms_i64(),
                    &self.root,
                );
                let observation_exclusions: Vec<V> = excluded_market_ids_v.clone();
                let observation = crate::edge_observation_plan::plan_edge_observation(
                    &planned,
                    &self.scheduler,
                    &self.universe,
                    observation_focus.as_ref(),
                    now_ms_i64(),
                    &observation_exclusions,
                    self.coverage.as_ref(),
                )
                .map_err(RunnerError::msg)?;
                if observation.0 != self.scheduler {
                    self.save_scheduler(observation.0.clone())?;
                }
                let trial_plan = crate::edge_trial_projection::prioritize_edge_trials(
                    &observation.1,
                    observation_focus.as_ref(),
                    &self.universe,
                    now_ms_i64(),
                    &excluded.iter().cloned().collect::<Vec<String>>(),
                )
                .map_err(RunnerError::msg)?;
                self.rt_set("probabilityReadFocus", probability_focus.clone());
                let excluded_ids: Vec<String> = excluded.iter().cloned().collect();
                let owned_ids: Vec<String> = Vec::new();
                let last_attempts: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
                let mut planned = crate::bid_anchor_read_focus::prioritize_anchor_reads(
                    &trial_plan,
                    if paper_focus.is_null() { None } else { Some(&paper_focus) },
                    &excluded_ids,
                    &owned_ids,
                    &last_attempts,
                );
                self.rt_set("bulkDipReadFocus", bulk_focus);
                self.rt_set("bidAnchorReadFocus", planned["bidAnchorReadFocus"].clone());
                self.rt_set("edgeObservation", planned["edgeObservation"].clone());
                self.rt_set("edgeTrialObservation", planned["edgeTrialObservation"].clone());
                if self.canonical_capacity.borrow().is_some() {
                    let r = {
                        let mut cc = self.canonical_capacity.borrow_mut();
                        cc.as_mut().unwrap().plan(
                            &planned,
                            &json!({"cycle":self.rt_get("cycles"),"now":now_ms(),"excludedExchangeIds":excluded.iter().cloned().collect::<Vec<String>>()}),
                        )
                    };
                    match r {
                        Ok(p) => planned = p,
                        Err(_) => {
                            cell_take(&self.canonical_capacity);
                            self.rt_set("canonicalCapacity", json!({"status":"failed","executionAuthority":false}));
                        }
                    }
                }
                let pair_review = self
                    .no_maker
                    .borrow()
                    .as_ref()
                    .map(|h| h.bank()["review"].clone())
                    .filter(|v| js_truthy(Some(v)))
                    .or_else(|| self.baal_pair.borrow().as_ref().map(|h| h.bank()["review"].clone()).filter(|v| js_truthy(Some(v))))
                    .or_else(|| self.dual_no_paper.borrow().as_ref().map(|h| h.bank_inner()["review"].clone()).filter(|v| js_truthy(Some(v))))
                    .or_else(|| self.dual_no_paper_v2.borrow().as_ref().map(|h| h.bank_inner()["review"].clone()).filter(|v| js_truthy(Some(v))));
                if let Some(review) = pair_review {
                    match crate::maker_read_plan::plan_maker_pair_reads(
                        &planned,
                        &review,
                        now_ms(),
                        &excluded.iter().cloned().map(V::String).collect::<Vec<V>>(),
                    ) {
                        Ok(p) => planned = p,
                        Err(_) => {
                            self.rt_set("makerPairFocus", json!({"reason":"maker_pair_invalid","executionAuthority":false}));
                        }
                    }
                }
                if !planned.get("makerPairFocus").is_none() || js_truthy(planned.get("makerPairFocus")) {
                    self.rt_set("makerPairFocus", planned["makerPairFocus"].clone());
                } else if self.rt_get("makerPairFocus").is_null() {
                    self.rt_set("makerPairFocus", V::Null);
                }
                self.rt_set("canonicalCapacityFocus", planned["canonicalCapacityFocus"].clone());
                self.rt_set("paperPackageFocus", planned["paperPackageFocus"].clone());
                let maker_cell = [&self.coupled_bulk, &self.coupled_maker, &self.stable_maker_broad, &self.stable_maker]
                    .into_iter()
                    .find(|c| c.borrow().is_some())
                    .cloned();
                if let Some(host_cell) = maker_cell {
                    let is_bulk = self.coupled_bulk.borrow().is_some();
                    let protected_ids: Vec<String> = {
                        let mut s: Vec<String> = Vec::new();
                        s.extend(inputs["heldMarketIds"].as_array().cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(String::from)));
                        s.extend(inputs["pendingMarketIds"].as_array().cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(String::from)));
                        s.extend(planned["bidAnchorReadFocus"]["marketIds"].as_array().cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(String::from)));
                        s.extend(production_candidates.iter().cloned());
                        let mut d = s;
                        d.sort();
                        d.dedup();
                        d
                    };
                    let hb = host_cell.borrow();
                    let host = hb.as_ref().unwrap();
                    let focus = host.focus().unwrap_or(V::Null);
                    planned = if is_bulk {
                        crate::coupled_bulk_read_focus::focus_coupled_bulk_reads(&planned, &focus, &self.universe, now_ms(), &protected_ids, &excluded.iter().cloned().collect::<Vec<String>>())
                            .map_err(RunnerError::msg)?
                    } else {
                        crate::stable_maker_read_focus::focus_stable_maker_reads(&planned, &focus, &self.universe, now_ms(), &protected_ids, &excluded.iter().cloned().collect::<Vec<String>>())
                            .map_err(RunnerError::msg)?
                    };
                }
                if self.competition.borrow().is_some() {
                    planned = crate::production_read_priority::prioritize_production_reads(
                        &planned,
                        &production_candidates,
                        &self.competition.borrow().as_ref().unwrap().managed_market_ids().into_iter().collect::<Vec<String>>(),
                    );
                }
                cycle_risk_market_ids = crate::profit_sprint_read_plan::scoped_risk_market_ids(
                    &self.competition.borrow().as_ref().map(|c| c.managed_market_ids().into_iter().collect::<Vec<String>>()).unwrap_or_default(),
                    if js_truthy(Some(&probability_focus)) { Some(&probability_focus) } else { None },
                )
                .map_err(RunnerError::msg)?;
                planned = crate::profit_sprint_read_plan::focus_profit_sprint_reads(
                    &planned,
                    profit_sprint_applied,
                    &{
                        let mut s: Vec<String> = probability_focus["markets"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                            .iter()
                            .filter_map(|x| x["marketId"].as_str().map(String::from))
                            .collect();
                        s.extend(production_candidates.iter().cloned());
                        s
                    },
                    &{
                        let mut s = cycle_risk_market_ids.clone();
                        s.extend(inputs["heldMarketIds"].as_array().cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(String::from)));
                        s.extend(inputs["pendingMarketIds"].as_array().cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(String::from)));
                        s
                    },
                )
                .map_err(RunnerError::msg)?;
                if self.competition.borrow().is_some() {
                    let scheduling = self.competition.borrow().as_ref().unwrap().scheduling_bank_hash().is_some();
                    let candidate_market_ids: Vec<String> = planned["reads"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|x| x["marketId"].as_str().map(String::from))
                        .filter(|id| scheduling || production_candidates.contains(id))
                        .collect();
                    let bulk_hints: Vec<V> = self.median_candidate_bulk_hints.borrow().values().cloned().collect();
                    let r = {
                        let mut comp = self.competition.borrow_mut();
                        comp.as_mut().unwrap().before_books(&json!({
                            "positions":positions,"orders":orders,"ordersAt":eligibility_orders_at,
                            "candidateMarketIds":candidate_market_ids,
                            "metadataCycle":self.rt_get("cycles").as_i64().unwrap_or(0)+1,
                            "protectedBulkHints":bulk_hints,
                        }))
                    };
                    if let Err(e) = r {
                        let mut comp = self.competition.borrow_mut();
                        comp.as_mut().unwrap().failure(&json!({"message": e.message}));
                    }
                }
                let mut supplemental_depth: Vec<V> = Vec::new();
                let mut sampling_reservations: Vec<V> = Vec::new();
                let mut sampling_receipts: Vec<V> = Vec::new();
                for spec in planned["reads"].as_array().cloned().unwrap_or_default() {
                    if self.ctx.stopped() || self.ctx.cooldown() > now_ms() {
                        break;
                    }
                    let market_id = spec["marketId"].as_str().unwrap_or("").to_string();
                    let is_source = spec["path"].as_str() == Some("sourceSnapshots");
                    let mut received_book = false;
                    let sampling_member = planned["comparisonSampling"]["reservationMarketIds"]
                        .as_array()
                        .map(|a| a.iter().any(|x| x.as_str() == Some(market_id.as_str())))
                        .unwrap_or(false);
                    self.record_attempt(
                        "book",
                        &market_id,
                        if sampling_member { planned["comparisonSampling"]["planId"].as_str() } else { None },
                        spec["nativeDiscovery"] == V::Bool(true),
                    )?;
                    if sampling_member {
                        sampling_reservations.push(json!({"marketId":market_id,"at":iso_now()}));
                    }
                    let book_result: R<()> = (|| {
                        if self.competition.borrow().is_some() {
                            let r = {
                                let mut comp = self.competition.borrow_mut();
                                comp.as_mut().unwrap().before_book(&market_id)
                            };
                            if let Err(e) = r {
                                let mut comp = self.competition.borrow_mut();
                                comp.as_mut().unwrap().failure(&json!({"message": e.message}));
                            }
                        }
                        let r = self.api().client.request(&format!(
                            "/markets/{}/orderbook?tournamentId={}&depth=20",
                            market_id,
                            self.c["tournamentId"].as_str().unwrap_or("")
                        ))?;
                        if is_source {
                            events.push(r.clone());
                        } else {
                            supplemental_depth.push(r.clone());
                        }
                        if js_truthy(planned["comparisonSampling"].get("planId")) {
                            sampling_receipts.push(json!({"marketId":market_id,"requestStartedAt":r["requestStartedAt"],"receivedAt":r["receivedAt"]}));
                        }
                        received_book = true;
                        let validated_receipt = crate::model::normalize_book(
                            &r["data"],
                            &market_id,
                            self.c["tournamentId"].as_str().unwrap_or(""),
                            r["receivedAt"].as_str().unwrap_or(""),
                            r["requestStartedAt"].as_str().unwrap_or(""),
                            self.c["maxDataAgeMs"].as_i64().unwrap_or(0),
                            &std::collections::BTreeSet::new(),
                            "authoritative_receipt_v1",
                        )
                        .map_err(RunnerError::msg)?;
                        let expected = self
                            .universe["exchanges"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                            .iter()
                            .find(|x| x["marketId"].as_str() == Some(market_id.as_str()))
                            .and_then(|x| x["exchangeId"].as_str().map(String::from));
                        if validated_receipt["exchangeId"].as_str() != expected.as_deref() {
                            return Err(RunnerError::msg("Book exchange identity mismatch"));
                        }
                        if self.competition.borrow().is_some() {
                            let r2 = {
                                let mut comp = self.competition.borrow_mut();
                                comp.as_mut().unwrap().on_book(&r)
                            };
                            if let Err(e) = r2 {
                                let mut comp = self.competition.borrow_mut();
                                comp.as_mut().unwrap().failure(&json!({"message": e.message}));
                            }
                        }
                        let shared_published = self.publish_book_event("publishBook", &json!({"marketId":market_id,"response":r}))?;
                        if shared_published != V::Bool(false) {
                            successful += 1;
                        }
                        let eligibility = crate::probability_receipt::probability_account_eligibility(&self.account_cache);
                        let pub_status = self
                            .book_publication
                            .borrow()
                            .as_ref()
                            .map(|b| b.status())
                            .unwrap_or(V::Null);
                        if self.no_maker.borrow().is_some() {
                            let elig = eligibility.clone().unwrap_or(V::Null);
                            let r = {
                                let mut g = self.no_maker.borrow_mut();
                                let h = g.as_mut().unwrap();
                                h.on_book(&pub_status, &json!(market_id), &elig)
                                    .and_then(|_| h.status())
                            };
                            match r {
                                Ok(st) => self.rt_set("noMaker", st),
                                Err(_) => {
                                    self.rt_set("noMaker", json!({"status":"failed","executionAuthority":false}));
                                    cell_take(&self.no_maker);
                                }
                            }
                        }
                        for (key, host_cell) in self.stable_hosts() {
                            let mut hb = host_cell.borrow_mut();
                            let Some(h) = hb.as_mut() else { continue };
                            let skip = {
                                let gated = (self.coupled_bulk.borrow().is_some() && key != "coupledBulk")
                                    || (key == "stableMaker"
                                        && (self.coupled_maker.borrow().is_some()
                                            || self.stable_maker_broad.borrow().is_some()));
                                gated
                                    && !h
                                        .status()
                                        .unwrap_or(V::Null)["riskMarketIds"]
                                        .as_array()
                                        .map(|a| a.iter().any(|x| x.as_str() == Some(market_id.as_str())))
                                        .unwrap_or(false)
                            };
                            if skip {
                                continue;
                            }
                            match self.stable_on_book(key, h, &pub_status, &market_id, &eligibility.clone().unwrap_or(V::Null)) {
                                Ok(()) => {
                                    let st = self.stable_status(key, h).unwrap_or(V::Null);
                                    self.rt_set(key, st);
                                }
                                Err(_) => self.stable_host_failed(key),
                            }
                        }
                        for (key, host_cell) in [("dualNoPaper", &self.dual_no_paper), ("dualNoPaperV2", &self.dual_no_paper_v2)] {
                            if let Some(h) = host_cell.borrow_mut().as_mut() {
                                match h.on_book(&pub_status, &market_id, &eligibility.clone().unwrap_or(V::Null)) {
                                    Ok(()) => self.rt_set(key, h.status().unwrap_or(V::Null)),
                                    Err(_) => {
                                        return Err(RunnerError::durable("Dual NO paper durability failed", true));
                                    }
                                }
                            }
                        }
                        if self.baal_pair.borrow().is_some() {
                            let mut hb = self.baal_pair.borrow_mut();
                            let h = hb.as_mut().unwrap();
                            match h.on_book(&pub_status, &market_id, &eligibility.clone().unwrap_or(V::Null)) {
                                Ok(()) => self.rt_set("baalPair", h.status()),
                                Err(_) => {
                                    self.rt_set("baalPair", json!({"status":"failed","executionAuthority":false}));
                                    cell_take(&self.baal_pair);
                                }
                            }
                        }
                        if self.canonical_capacity.borrow().is_some() {
                            let r = {
                                let mut cc = self.canonical_capacity.borrow_mut();
                                cc.as_mut().unwrap().inspect(&pub_status)
                            };
                            match r {
                                Ok(_) => {
                                    let st = self.canonical_capacity.borrow().as_ref().unwrap().status().unwrap_or(V::Null);
                                    self.rt_set("canonicalCapacity", st);
                                }
                                Err(_) => {
                                    cell_take(&self.canonical_capacity);
                                    self.rt_set("canonicalCapacity", json!({"status":"failed","executionAuthority":false}));
                                }
                            }
                        }
                        let comp_state = self.competition.borrow().as_ref().map(|c| c.state()).unwrap_or(V::Null);
                        let attributed_official_exposure = comp_state["owned"][validated_receipt["exchangeId"].as_str().unwrap_or("")]["quantity"]
                            .as_f64()
                            .unwrap_or(0.0)
                            > 0.0
                            || comp_state["intents"]
                                .as_object()
                                .map(|m| {
                                    m.values().any(|i| {
                                        i["exchangeId"].as_str() == validated_receipt["exchangeId"].as_str()
                                            && i["status"].as_str() != Some("closed")
                                    })
                                })
                                .unwrap_or(false);
                        if excluded.contains(validated_receipt["exchangeId"].as_str().unwrap_or(""))
                            || attributed_official_exposure
                        {
                            if is_source {
                                let at = iso_now();
                                self.state["skips"][market_id.as_str()] = json!({"at":at,"reason":"real_account_exposure"});
                                crate::model::invalidate(&mut self.state, Some(&market_id), &at, "real_account_exposure");
                            }
                            return Ok(()); // continue in JS == return here
                        }
                        let b = crate::model::normalize_book(
                            &r["data"],
                            &market_id,
                            self.c["tournamentId"].as_str().unwrap_or(""),
                            r["receivedAt"].as_str().unwrap_or(""),
                            r["requestStartedAt"].as_str().unwrap_or(""),
                            self.c["maxDataAgeMs"].as_i64().unwrap_or(0),
                            &excluded,
                            if is_source {
                                "event_age_v1"
                            } else if self.dip_bank.is_some() {
                                "authoritative_receipt_v1"
                            } else {
                                "event_age_v1"
                            },
                        )
                        .map_err(RunnerError::msg)?;
                        if b["exchangeId"].as_str() != expected.as_deref() {
                            return Err(RunnerError::msg("Book exchange identity mismatch"));
                        }
                        if is_source && !self.retire_native_prices {
                            self.state = crate::model::observe(&self.state, &b, &self.c).map_err(|e| {
                                let mut err = RunnerError::msg(e);
                                err.fatal = true;
                                err
                            })?;
                        }
                        Ok(())
                    })();
                    if let Err(e) = book_result {
                        if e.durable_recording_failed {
                            return Err(e);
                        }
                        if self.ctx.stopped() {
                            break;
                        }
                        if !received_book {
                            self.publish_book_event("invalidate", &json!({"marketId":market_id,"reason":"BOOK_READ_FAILED"}))?;
                        }
                        if e.fatal || self.ctx.manual_review() || e.status == Some(401) || e.status == Some(403) {
                            return Err(e);
                        }
                        let reason = e.status.map(|s| format!("http_{}", s)).unwrap_or_else(|| e.message.clone());
                        let at = iso_now();
                        if is_source {
                            self.state["skips"][market_id.as_str()] = json!({"at":at,"reason":reason});
                            crate::model::invalidate(&mut self.state, Some(&market_id), &at, &reason);
                        } else {
                            supplemental_depth.push(json!({"marketId":market_id,"skipped":reason,"at":at}));
                        }
                        if e.status == Some(429) || e.status == Some(503) {
                            break;
                        }
                    }
                }
                self.rt_set(
                    "scheduling",
                    json!({
                        "managedMarketIds":planned["managedMarketIds"],"entryMarketIds":planned["entryMarketIds"],
                        "overCapacity":planned["overCapacity"],"reads":planned["reads"],
                        "nativeDiscovery":planned.get("nativeDiscovery").cloned().unwrap_or(V::Null),
                    }),
                );
                if self.settlement_portfolio.borrow().is_some() {
                    let eligibility = crate::probability_receipt::probability_account_eligibility(&self.account_cache);
                    let eligibility_v = eligibility.unwrap_or(V::Null);
                    let r = {
                        let mut sp = self.settlement_portfolio.borrow_mut();
                        sp.as_mut().unwrap().consume(Some(iso_now().as_str()), &eligibility_v)
                    };
                    match r {
                        Ok(_) => {
                            let st = self.settlement_portfolio.borrow().as_ref().unwrap().status().unwrap_or(V::Null);
                            self.rt_set("settlementPortfolio", st);
                        }
                        Err(_) => {
                            let mut retained = self.rt_get("settlementPortfolio");
                            if let Some(s) = self
                                .settlement_portfolio
                                .borrow()
                                .as_ref()
                                .and_then(|h| h.status().ok())
                            {
                                if js_truthy(Some(&s)) {
                                    retained = s;
                                }
                            }
                            retained["failed"] = json!(true);
                            retained["accountingRetained"] = json!(true);
                            retained["executionAuthority"] = json!(false);
                            self.rt_set("settlementPortfolio", retained);
                            return Err(RunnerError::durable("Settlement portfolio recording failed", true));
                        }
                    }
                }
                self.rt_set(
                    "supplementalDepth",
                    json!({
                        "marketIds":planned["reads"].as_array().cloned().unwrap_or_default().iter().filter(|x| x["path"].as_str()==Some("supplementalDepth")).map(|x| x["marketId"].clone()).collect::<Vec<V>>(),
                        "observations":supplemental_depth.iter().filter(|x| x.get("endpoint").is_some()).count(),
                        "skips":supplemental_depth.iter().filter(|x| x.get("skipped").is_some()).map(|x| json!({"marketId":x["marketId"],"reason":x["skipped"]})).collect::<Vec<V>>(),
                    }),
                );
                let mut bulk_prices = V::Null;
                let mut bulk_price_error = V::Null;
                if !self.ctx.stopped() && self.ctx.cooldown() <= now_ms() {
                    let sweep = {
                        let ctx = self.ctx.clone();
                        let ctx2_again = ctx.clone();
                        let api = AssertSend(self.api.clone());
                        let hooks = crate::market_sweep::SweepHooks {
                            request: &move |endpoint: &str| -> Result<V, (Option<i64>, bool)> {
                                let a = api.get().borrow();
                                match a.as_ref() {
                                    None => Err((None, true)),
                                    Some(a) => match a.client.request(endpoint) {
                                        Ok(v) => Ok(v),
                                        Err(e) => {
                                            let err = RunnerError::from_svc(e);
                                            Err((err.status.map(|s| s as i64), err.manual_review))
                                        }
                                    },
                                }
                            },
                            is_stopped: &move || ctx.stopped(),
                            cooldown_remaining_ms: &move || ctx2_again.cooldown(),
                            now: &now_ms,
                        };
                        let managed: Vec<String> = if profit_sprint_applied {
                            cycle_risk_market_ids.clone()
                        } else {
                            self.competition.borrow().as_ref().map(|c| c.managed_market_ids().into_iter().collect::<Vec<String>>()).unwrap_or_default()
                        };
                        // closures publish snapshots + checkpoint coverage
                        let price_pub = AssertSend(self.price_publication.clone());
                        let market_screen = AssertSend(self.collector_market_screen.clone());
                        let median_discovery = AssertSend(self.median_discovery.clone());
                        let party_pair = AssertSend(self.party_pair_selector.clone());
                        let hints = AssertSend(self.median_candidate_bulk_hints.clone());
                        let coupled = AssertSend(self.coupled_bulk.clone());
                        let runtime = AssertSend(self.ctx.runtime.clone());
                        let coverage_file = self.coverage_file.clone();
                        let ctx2 = self.ctx.clone();
                        let mut on_snapshot = move |snapshot: &V| -> Result<(), String> {
                            let row = price_pub.get().borrow_mut().as_mut().map(|p| p.publish(snapshot)).transpose();
                            let row = match row {
                                Ok(r) => r.unwrap_or(V::Null),
                                Err(e) => return Err(e),
                            };
                            market_screen.get().borrow_mut().as_mut().map(|m| m.on_published(snapshot));
                            if js_truthy(Some(&row)) {
                                if let Some(m) = median_discovery.get().borrow_mut().as_mut() {
                                    let _ = m.on_published(&row);
                                }
                                if let Some(p) = party_pair.get().borrow_mut().as_mut() {
                                    let _ = p.on_published(&row);
                                }
                                let snap = row["event"]["snapshot"].clone();
                                for id in snap["missingIds"].as_array().cloned().unwrap_or_default() {
                                    if let Some(s) = id.as_str() {
                                        hints.get().borrow_mut().remove(s);
                                    }
                                }
                                for q in snap["quotes"].as_array().cloned().unwrap_or_default() {
                                    let mut qq = q.as_object().cloned().unwrap_or_default();
                                    qq.insert("knownAt".into(), row["at"].clone());
                                    qq.insert("sourceSequence".into(), row["sequence"].clone());
                                    qq.insert("sourceHash".into(), row["hash"].clone());
                                    qq.insert("depthAvailable".into(), json!(false));
                                    qq.insert("executable".into(), json!(false));
                                    if let Some(k) = q["exchangeId"].as_str() {
                                        hints.get().borrow_mut().insert(k.to_string(), V::Object(qq));
                                    }
                                }
                            }
                            if js_truthy(Some(&row)) && coupled.get().borrow().is_some() {
                                let mut hb = coupled.get().borrow_mut();
                                let h = hb.as_mut().unwrap();
                                match h.on_price(&row) {
                                    Ok(()) => {
                                        let st = h.status().unwrap_or(V::Null);
                                        runtime.get().borrow_mut()["coupledBulk"] = st;
                                    }
                                    Err(_) => {
                                        runtime.get().borrow_mut()["coupledBulk"] =
                                            json!({"status":"failed","executionAuthority":false});
                                        *coupled.get().borrow_mut() = None;
                                    }
                                }
                            }
                            Ok(())
                        };
                        let runtime2 = self.ctx.runtime.clone();
                        let mut on_coverage = move |next: &V| -> Result<(), String> {
                            let ctx3 = ctx2.clone();
                            let runtime3 = runtime2.clone();
                            crate::coverage_checkpoint::checkpoint_coverage(
                                &coverage_file,
                                next,
                                &move || !ctx3.stopped(),
                                &move |fault: &V| {
                                    let mut f = fault.as_object().cloned().unwrap_or_default();
                                    f.insert("at".into(), json!(iso_now()));
                                    let mut ff = serde_json::Map::new();
                                    ff.insert("at".into(), json!(iso_now()));
                                    for (k, v) in f {
                                        ff.insert(k, v);
                                    }
                                    runtime3.borrow_mut()["coveragePersistenceFault"] = V::Object(ff);
                                },
                            )
                            .map(|_| ())
                            .map_err(|e| format!("{:?}", e))
                        };
                        crate::market_sweep::collect_market_sweep(
                            &self.universe,
                            self.coverage.as_ref().unwrap_or(&V::Null),
                            self.market_sweep.is_some(),
                            &managed,
                            &hooks,
                            &mut on_snapshot,
                            &mut on_coverage,
                        )
                    };
                    match sweep {
                        Ok(sweep) => {
                            self.coverage = if sweep["coverage"].is_null() { None } else { Some(sweep["coverage"].clone()) };
                            *self.coverage_rc.borrow_mut() = self.coverage.clone();
                            bulk_prices = sweep["primarySnapshot"].clone();
                            bulk_price_error = sweep["error"].clone();
                            self.rt_set(
                                "marketSweep",
                                json!({
                                    "enabled":self.market_sweep.is_some(),"requestedBatches":sweep["requestedBatches"],
                                    "completedBatches":sweep["completedBatches"],"complete":sweep["complete"],
                                    "riskPriority":sweep["riskPriority"],
                                    "publication":self.price_publication.borrow().as_ref().map(|p| p.status()).unwrap_or(V::Null),
                                }),
                            );
                            self.rt_set(
                                "breadth",
                                json!({
                                    "universeSize":self.universe["exchanges"].as_array().map(|a| a.len()).unwrap_or(0),
                                    "observedExchanges":self.coverage.as_ref().map(|c| c["quotes"].as_object().map(|m| m.len()).unwrap_or(0)).unwrap_or(0),
                                    "lastCollectionAt":self.coverage.as_ref().map(|c| c["updatedAt"].clone()).unwrap_or(V::Null),
                                    "error":bulk_price_error,
                                }),
                            );
                        }
                        Err(e) => {
                            let err = RunnerError::msg(e);
                            if err.durable_recording_failed || (!self.ctx.stopped() && (self.ctx.manual_review() || err.status == Some(401) || err.status == Some(403))) {
                                return Err(err);
                            }
                            bulk_price_error = json!(if self.ctx.stopped() {
                                "stopped".to_string()
                            } else {
                                err.status.map(|s| format!("http_{}", s)).unwrap_or_else(|| err.message.clone())
                            });
                            let mut b = self.rt_get("breadth");
                            if !b.is_object() {
                                b = json!({});
                            }
                            b["error"] = bulk_price_error.clone();
                            self.rt_set("breadth", b);
                        }
                    }
                }
                if self.median_preflight.borrow().is_some() {
                    if self.median_preflight.borrow().as_ref().unwrap().pending() {
                        let r = {
                            let api_cell = AssertSend(self.api.clone());
                            let fences_cell = AssertSend(self.fences.clone());
                            let dispatch = move |spec: &V| -> Result<V, String> {
                                api_cell.get().borrow().as_ref()
                                    .map(|a| a.dispatch_with(spec, &fences_cell.0))
                                    .unwrap_or_else(|| Err("STOPPED".into()))
                            };
                            self.median_preflight.borrow_mut().as_mut().unwrap().consume(&dispatch)
                        };
                        match r {
                            Ok(v) => self.rt_set("medianPreflight", v),
                            Err(_) => self.rt_set(
                                "medianPreflight",
                                json!({"status":"blocked","at":iso_now(),"executionAuthority":false}),
                            ),
                        }
                    }
                } else if self.median_discovery.borrow().is_some() {
                    let protected_ids: Vec<String> = planned["reads"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|x| x["marketId"].as_str().map(String::from))
                        .collect();
                    self.median_discovery.borrow_mut().as_mut().unwrap().after_protected(
                        self.rt_get("cycles").as_i64().unwrap_or(0) + 1,
                        &protected_ids,
                        successful,
                    )?;
                    let st = self.median_discovery.borrow().as_ref().unwrap().status();
                    self.rt_set("medianDiscovery", st);
                }
                let at = iso_now();
                crate::model::sample_equity(&mut self.state, &self.c, &at);
                crate::model::assert_state(&self.state, &self.c).map_err(RunnerError::msg)?;
                self.commit(json!({
                    "type":"cycle","clockAt":at,"sourceSnapshots":events,"supplementalDepth":supplemental_depth,
                    "nativeDiscovery":if js_truthy(planned.get("nativeDiscovery")){{let mut n=planned["nativeDiscovery"].clone();n["source"]=self.rt_get("marketHistoryScreen")["source"].clone();n}}else{V::Null},
                    "basketMeasurement":planned["basketMeasurement"].clone(),
                    "edgeObservation":planned["edgeObservation"].clone(),
                    "edgeTrialObservation":planned["edgeTrialObservation"].clone(),
                    "comparisonSampling":if js_truthy(planned.get("comparisonSampling")){{let mut s=planned["comparisonSampling"].clone();s["reservations"]=json!(sampling_reservations);s["receipts"]=json!(sampling_receipts);s}}else{V::Null},
                    "entryMarketIds":dedup({let mut e=planned["entryMarketIds"].as_array().cloned().unwrap_or_default();e.extend(planned["managedMarketIds"].as_array().cloned().unwrap_or_default());e}),
                    "bulkPrices":bulk_prices,"bulkPriceError":bulk_price_error,
                    "accountExclusions":excluded.iter().cloned().collect::<Vec<String>>(),
                    "exclusionObservedAt":positions["receivedAt"],
                }))?;
                cycle_committed = true;
                if self.realtime.borrow().is_some() && !self.ctx.stopped() && self.ctx.cooldown() <= now_ms() {
                    self.realtime.borrow_mut().as_mut().unwrap().service();
                }
                if let Some(h) = self.native_dip.borrow_mut().as_mut() {
                    h.kick();
                    self.rt_set("nativeDip", h.status());
                }
                if let Some(h) = self.native_quote.borrow_mut().as_mut() {
                    h.kick();
                    self.rt_set("nativeQuote", h.status());
                }
                {
                    let mut rt = self.ctx.runtime.borrow_mut();
                    rt["cycles"] = json!(rt["cycles"].as_i64().unwrap_or(0) + 1);
                    rt["lastCycleAt"] = json!(at);
                    rt["status"] = json!(if self.ctx.stopped() { "stopping" } else if successful > 0 { "running" } else { "degraded" });
                }
                self.persist_status()?;
                println!("{}", json!({"at":at,"event":"paper_cycle","cycle":self.rt_get("cycles"),"observations":self.state["observations"],"fills":self.state["strategies"].as_object().map(|m| m.iter().map(|(k,s)|(k.clone(),json!(s["fills"].as_array().map(|a| a.len()).unwrap_or(0)))).collect::<serde_json::Map<String,V>>()).unwrap_or_default(),"competitionOrders":self.competition.borrow().as_ref().map(|c| c.state()["intents"].as_object().map(|m| m.values().filter(|i| i["orderId"].as_i64().map(|n| n>0).unwrap_or(false)).count()).unwrap_or(0)).unwrap_or(0)}));
                Ok(())
            })();
            if let Err(e) = cycle_result {
                if e.durable_recording_failed {
                    return Err(e);
                }
                if self.ctx.stopped() {
                    if !cycle_committed {
                        self.state = checkpoint;
                    }
                    break;
                }
                let at = iso_now();
                crate::model::invalidate(&mut self.state, None, &at, "account_reconciliation_unavailable");
                self.publish_book_event("invalidate", &json!({"marketId":V::Null,"reason":"ACCOUNT_OUTAGE"}))?;
                {
                    let mut rt = self.ctx.runtime.borrow_mut();
                    let mut errors = rt["errors"].as_array().cloned().unwrap_or_default();
                    errors.push(json!({"at":at,"reason":e.status.map(|s| format!("http_{}", s)).unwrap_or_else(|| e.message.clone())}));
                    let keep: Vec<V> = errors.into_iter().rev().take(20).collect();
                    let mut keep = keep;
                    keep.reverse();
                    rt["errors"] = V::Array(keep);
                    rt["status"] = json!(if e.recoverable { "degraded" } else { "failed" });
                }
                let _ = self.persist_status();
                if e.recoverable {
                    self.commit(json!({
                        "type":"account_read_outage","clockAt":at,
                        "reason":e.status.map(|s| format!("http_{}", s)).unwrap_or_else(|| "connection_unavailable".to_string()),
                    }))?;
                    self.pause_until_next_read((10000.0_f64).max(self.ctx.cooldown() - now_ms()))?;
                    continue;
                }
                let reason = {
                    let rt = self.ctx.runtime.borrow();
                    rt["errors"].as_array().and_then(|a| a.last()).cloned().unwrap_or(V::Null)["reason"]
                        .as_str()
                        .unwrap_or("")
                        .to_string()
                };
                return Err(RunnerError::msg(format!("Paper runner stopped: {}", reason)));
            }
            if !self.ctx.stopped() && self.rt_get("cycles").as_f64().unwrap_or(0.0) < max_cycles {
                let cycle_wait = crate::cycle_wait::collection_cycle_wait(&json!({
                    "cycleStartedAt":cycle_start,"now":now_ms(),
                    "cycleIntervalMs":self.market_sweep.as_ref().and_then(|m| m["cycleIntervalMs"].as_f64()).unwrap_or_else(|| self.c["cycleIntervalMs"].as_f64().unwrap_or(0.0)),
                    "managedMarketIds":cycle_risk_market_ids,
                })).map_err(RunnerError::msg)?;
                self.rt_set("cycleWait", cycle_wait.clone());
                if cycle_wait["delayMs"].as_f64().unwrap_or(0.0) > 0.0 {
                    self.pause_until_next_read(cycle_wait["delayMs"].as_f64().unwrap_or(0.0))?;
                }
            }
        }
        Ok(())
    }

    // realtime-capture startup — runner.mjs:353-360
    fn start_realtime(&mut self) -> R<()> {
        let realtime_file = self.runtime_dir.join("realtime-capture.json");
        if !realtime_file.exists() {
            return Ok(());
        }
        let activation = read_json_file(&realtime_file).map_err(|e| e)?;
        if activation["enabled"] != V::Bool(true) {
            return Ok(());
        }
        let scope = crate::realtime_scope::realtime_capture_scope(&activation, &self.c, &self.universe)
            .map_err(|e| RunnerError::msg(e.to_string()))?;
        let file = format!(
            "realtime-{}-{}.jsonl",
            iso_now().replace([':', '.'], "-"),
            random_uuid()
        );
        let capture = Capture::new(&self.runtime_dir.join(&file), true).map_err(RunnerError::msg)?;
        *self.realtime_capture.borrow_mut() = Some(capture);
        // JS: books created first; client callbacks + shared snapshot capture append
        // through it. The ported books must share the same instance behind Arc<Mutex>.
        let markets = scope["markets"].as_array().cloned().unwrap_or_default();
        let books: Arc<Mutex<RealtimeBooks<Box<dyn Fn() -> f64 + Send + Sync>>>> = Arc::new(Mutex::new(
            RealtimeBooks::new(
                self.c["tournamentId"].as_str().unwrap_or(""),
                &markets,
                scope["channelMode"].as_str().unwrap_or(""),
                Box::new(now_ms) as Box<dyn Fn() -> f64 + Send + Sync>,
                20_000.0,
                1_000.0,
                20_000.0,
            )
            .map_err(RunnerError::msg)?,
        ));
        let capture_cell = AssertSend(self.realtime_capture.clone());
        let capture_row = {
            let books_cell = books.clone();
            let cap = capture_cell.clone();
            move |kind: &str, market: Option<&V>, data: V| {
                let topic = market
                    .and_then(|m| m.get("topic").and_then(V::as_str))
                    .unwrap_or("")
                    .to_string();
                let mut row = serde_json::Map::new();
                row.insert("kind".into(), json!(kind));
                row.insert("observedAt".into(), json!(iso_now()));
                row.insert("topic".into(), json!(topic));
                if let V::Object(d) = data {
                    for (k, v) in d {
                        row.insert(k, v);
                    }
                }
                let _ = books_cell;
                if let Some(c) = cap.get().borrow_mut().as_mut() {
                    let _ = c.append(&V::Object(row));
                }
            }
        };
        let capture_row = AssertSend(Arc::new(capture_row)
            as Arc<dyn Fn(&str, Option<&V>, V) + Send + Sync>);
        let api = AssertSend(self.api.clone());
        let fences = AssertSend(self.fences.clone());
        let dispatch: Dispatch = Arc::new(move |spec: &V| -> Result<V, String> {
            api.get().borrow()
                .as_ref()
                .map(|a| a.dispatch_with(spec, &fences.get()))
                .unwrap_or(Err("STOPPED".into()))
        });
        let ctx = AssertSend(self.ctx.clone());
        let ctx2 = AssertSend(self.ctx.clone());
        let ctx3 = AssertSend(self.ctx.clone());
        let api2 = AssertSend(self.api.clone());
        let api3 = AssertSend(self.api.clone());
        let can_op: CanOp = Arc::new(move || ctx.get().can_operate_stops() && !ctx.get().marker("REALTIME_STOP"));
        let can_mint: TokenGate = Arc::new(move || {
            api2.get().borrow().as_ref().map(|a| !a.client.read_only).unwrap_or(false)
        });
        let books_c = AssertSend(books.clone());
        let client = RealtimeClient::new(
            dispatch.clone(),
            Arc::new(|url: &str, key: &str, opts: &crate::realtime_client::SdkOptions| {
                crate::supabase_ws::create_supabase_client(url, key, opts)
            }) as crate::realtime_client::CreateClient,
            self.c["tournamentId"].as_str().unwrap_or(""),
            &markets,
            scope["channelMode"].as_str().unwrap_or(""),
            can_op,
            can_mint,
            Arc::new({
                let books_c = books_c.clone();
                let cr = capture_row.clone();
                move |e: &V| {
                    books_call(books_c.get(), "subscribed", e);
                    (cr.get())("subscribed", Some(e), e.clone());
                }
            }) as EventCb,
            Arc::new({
                let books_c = books_c.clone();
                let cr = capture_row.clone();
                move |e: &V| {
                    books_call(books_c.get(), "ingest", e);
                    (cr.get())("event", Some(e), e.clone());
                }
            }) as EventCb,
            Arc::new({
                let books_c = books_c.clone();
                let cr = capture_row.clone();
                move |e: &V| {
                    books_call(books_c.get(), "disconnected", e);
                    (cr.get())("disconnected", Some(e), e.clone());
                }
            }) as EventCb,
            Arc::new({
                let books_c = books_c.clone();
                let cr = capture_row.clone();
                move |e: &V| {
                    books_call(books_c.get(), "invalidate", e);
                    (cr.get())("invalidate", Some(e), e.clone());
                }
            }) as EventCb,
            Arc::new(now_ms_i64) as NowFn,
            15_000,
            3_000,
            300_000,
            250,
        )
        .map_err(RunnerError::msg)?;
        let rt = RtHost {
            client: Arc::new(client),
            books,
            dispatch,
            can_operate: Arc::new(move || ctx2.get().can_operate_stops() && !ctx2.get().marker("REALTIME_STOP")),
            can_mint_token: Arc::new(move || {
                api3.get().borrow().as_ref().map(|a| !a.client.read_only).unwrap_or(false)
            }),
            now: Arc::new(now_ms_i64) as NowFn,
            capture: self.realtime_capture.clone(),
            silent_recovery_interval_ms: 120_000,
            inner: Mutex::new(RtInner {
                closed: false,
                servicing: false,
                cursor: 0,
                retry_at: 0.0,
                recovery_attempts: 0,
                recovery_successes: 0,
                shared_snapshot_attempts: 0,
                shared_snapshot_successes: 0,
                last_recovery_at: None,
                last_code: None,
                quiet_deferrals: 0,
                recovered: IndexMap::new(),
                fences: Vec::new(),
                shared_tickets: Vec::new(),
            }),
        };
        rt.capture_row("startup", None, json!({"file": file.clone(), "scope": scope.clone()}));
        *self.realtime.borrow_mut() = Some(rt);
        baal_service_runtime::atomic::atomic_json(
            &self.runtime_dir.join("realtime-session.json"),
            &json!({"schemaVersion":1,"file":file,"startedAt":iso_now(),"scope":scope}),
        )
        .map_err(RunnerError::msg)?;
        self.rt_set(
            "realtime",
            json!({"mode":"market_capture_only","channelMode":scope["channelMode"],"marketCount":markets.len(),"recoveryBudgetPerCycle":1}),
        );
        let _ = ctx3;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// run() — the whole runner.mjs CLI flow.
// ---------------------------------------------------------------------------
fn now_ms_arc() -> Arc<dyn Fn() -> f64 + Send + Sync> {
    Arc::new(now_ms)
}

fn read_baal_scaled_legacy_risk(_runtime_dir: &Path, _other: &V) -> V {
    // baal_scaled_legacy_risk.mjs is unported; the paired-live cascade that
    // consumes it is gated behind markers that map to the v1 host. See notes.
    V::Null
}

fn last_attempt_map(v: &V) -> BTreeMap<String, i64> {
    v.as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, x)| x.as_str().and_then(|s| parse_iso_ms(s)).map(|t| (k.clone(), t as i64)))
                .collect()
        })
        .unwrap_or_default()
}

fn is_contract_code(m: &str) -> bool {
    m.len() >= 8
        && m.starts_with("CONTRACT_")
        && m.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

fn is_trade_tape_code(m: &str) -> bool {
    m.len() >= 11
        && m.starts_with("TRADE_TAPE_")
        && m.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

fn dedup(ids: Vec<V>) -> Vec<V> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for v in ids {
        if let Some(s) = v.as_str() {
            if seen.insert(s.to_string()) {
                out.push(v);
            }
        }
    }
    out
}

fn write_exclusive(path: &Path, contents: &str) -> Result<(), String> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(path).map_err(|e| e.to_string())?;
    f.write_all(contents.as_bytes()).map_err(|e| e.to_string())
}

fn self_cooldown(ctx: &Ctx) -> f64 {
    ctx.cooldown()
}

fn load_key() -> Result<String, String> {
    if let Ok(k) = std::env::var("SIG_API_KEY") {
        return Ok(k.trim().to_string());
    }
    let out = std::process::Command::new("pwsh.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$ErrorActionPreference='Stop'; $s=(Get-Content -Raw -LiteralPath (Join-Path $env:LOCALAPPDATA 'SigTradingCup\\api-key.dpapi')).Trim()|ConvertTo-SecureString; [Console]::Write(([pscredential]::new('api',$s)).GetNetworkCredential().Password)",
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

static SIG_FLAG: AtomicBool = AtomicBool::new(false);
unsafe extern "C" fn on_term_signal(_sig: i32) {
    SIG_FLAG.store(true, Ordering::SeqCst);
}
fn install_signal_flag() {
    unsafe {
        extern "C" {
            fn signal(signum: i32, handler: usize) -> usize;
        }
        signal(2, on_term_signal as usize); // SIGINT
        signal(15, on_term_signal as usize); // SIGTERM
    }
}

/// runner() — the entire JS module body. Returns process exit code.
pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--live") {
        eprintln!("Competition order execution is disabled");
        return 1;
    }
    let config_file = if let Some(i) = args.iter().position(|a| a == "--config") {
        args.get(i + 1)
            .map(|s| std::fs::canonicalize(s).unwrap_or_else(|_| PathBuf::from(s)))
            .unwrap_or_else(|| PathBuf::from("config.json"))
    } else {
        PathBuf::from("config.json")
    };
    run_with(config_file)
}

fn run_with(config_file: PathBuf) -> i32 {
    // ---- config + paths ----
    let c = match std::fs::read_to_string(&config_file)
        .map_err(|e| e.to_string())
        .and_then(|s| serde_json::from_str::<V>(&s).map_err(|e| e.to_string()))
        .and_then(|v| crate::model::validate_config(&v).map(|_| v))
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    let root = live_paper_root();
    let workspace = root.parent().map(Path::to_path_buf).unwrap_or_else(|| root.clone());
    let runtime_dir = std::env::var("SIG_PAPER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|_| workspace.clone())
                .join("SigTradingCup")
                .join("paper-runtime")
        });
    let network_budget = match crate::transport_network_budget::load_network_budget(
        &runtime_dir.join("coordinated-network-budget.json"),
    ) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    let network_interval_ms = match crate::transport_network_budget::network_interval(
        c["readIntervalMs"].as_i64().unwrap_or(0),
        network_budget.as_ref(),
    ) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    let production_activation_file = runtime_dir.join("competition-activation.json");
    let production_activation: Option<V> = if production_activation_file.exists() {
        match read_json_file(&production_activation_file) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("{}", e);
                return 1;
            }
        }
    } else {
        None
    };
    if production_activation.as_ref().map(|a| a["enabled"].as_bool() == Some(true)).unwrap_or(false) {
        let a = production_activation.as_ref().unwrap();
        if let Err(e) = baal_execution::production::validate_production_activation(a) {
            eprintln!("{}", e);
            return 1;
        }
        let authority_keys: Vec<&str> = if a["authorityKind"].as_str() == Some("direct_human_assignment") {
            vec!["authorityKind", "assignmentCommentId"]
        } else {
            vec!["releaseCommentId"]
        };
        if !(baal_execution::production::production_handoff_current(&runtime_dir, a)
            && network_budget.as_ref().and_then(|b| b["schemaVersion"].as_i64()) == Some(4)
            && ["handoffId", "ackCommentId"]
                .iter()
                .chain(authority_keys.iter())
                .all(|k| network_budget.as_ref().map(|b| &b[*k]) == Some(&a[*k])))
        {
            eprintln!("PRODUCTION_HANDOFF_OR_BUDGET_REQUIRED");
            return 1;
        }
    } else if network_budget.as_ref().and_then(|b| b["schemaVersion"].as_i64()) == Some(4) {
        eprintln!("PRODUCTION_BUDGET_WITHOUT_ACTIVATION");
        return 1;
    }
    let stop_file = workspace.join("STOP");
    let paper_stop = runtime_dir.join("STOP");
    let collector_stop = runtime_dir.join("COLLECTOR_STOP");
    if stop_file.exists() || paper_stop.exists() || collector_stop.exists() {
        println!("{}", json!({"status":"stopped","reason":"STOP_present"}));
        return 0;
    }
    if let Err(e) = std::fs::create_dir_all(&runtime_dir).map_err(|e| e.to_string()) {
        eprintln!("{}", e);
        return 1;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&runtime_dir, std::fs::Permissions::from_mode(0o700));
    }
    let lock_file = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace.clone())
        .join("SigTradingCup")
        .join("bot.lock");
    let paper_lock = match crate::paper_runtime::lock(&lock_file) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    install_signal_flag();
    let signal = Arc::new(AtomicBool::new(false));
    let stopping = Rc::new(Cell::new(false));
    // stopWatcher: 200ms poll over the three STOP files plus the signal flag.
    {
        let (sf, ps, cs) = (stop_file.clone(), paper_stop.clone(), collector_stop.clone());
        let sig = signal.clone();
        let rd = runtime_dir.clone();
        let stopping2 = AssertSend(stopping.clone());
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag2 = stop_flag.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(200));
            if stop_flag2.load(Ordering::Relaxed) {
                break;
            }
            let fired = sf.exists() || ps.exists() || cs.exists() || SIG_FLAG.load(Ordering::SeqCst);
            if fired && !stopping2.get().get() {
                stopping2.get().set(true);
                sig.store(true, Ordering::Relaxed);
                let cause = if SIG_FLAG.load(Ordering::SeqCst) {
                    "SIGINT"
                } else {
                    "STOP_present"
                };
                let _ = &rd;
                println!("{}", json!({"event":"collector_stopping","cause":cause}));
            }
        });
        // The watcher stops when `stop_flag` is set at Drop::finally — stored via a leak-free
        // channel: keep it in the static via a lazy registry. Simpler: we never join; the
        // process exit ends it. Keep the flag to allow tests to halt the thread.
        let _ = stop_flag;
    }
    let cycles_arg = args_position_cycles();
    // ---- journal + state ----
    let c2 = c.clone();
    let runtime_dir2 = runtime_dir.clone();
    let workspace2 = workspace.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        run_inner(
            c2,
            root.clone(),
            workspace2,
            runtime_dir2,
            network_budget,
            network_interval_ms,
            production_activation_file,
            production_activation,
            stop_file,
            paper_stop,
            collector_stop,
            stopping,
            signal,
            cycles_arg,
        )
    }));
    drop(paper_lock);
    match result {
        Ok(code) => code,
        Err(_) => 1,
    }
}

fn args_position_cycles() -> f64 {
    let args: Vec<String> = std::env::args().collect();
    match args.iter().position(|a| a == "--cycles") {
        None => f64::INFINITY,
        Some(i) => args
            .get(i + 1)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(f64::NAN),
    }
}

fn run_inner(
    c: V,
    root: PathBuf,
    workspace: PathBuf,
    runtime_dir: PathBuf,
    network_budget: Option<V>,
    network_interval_ms: i64,
    production_activation_file: PathBuf,
    production_activation: Option<V>,
    stop_file: PathBuf,
    paper_stop: PathBuf,
    collector_stop: PathBuf,
    stopping: Rc<Cell<bool>>,
    signal: Arc<AtomicBool>,
    max_cycles: f64,
) -> i32 {
    if !(max_cycles.is_infinite() || max_cycles.fract() == 0.0 && max_cycles >= 1.0) {
        eprintln!("Invalid cycle bound");
        return 1;
    }
    let ctx = Ctx {
        runtime_dir: runtime_dir.clone(),
        workspace: workspace.clone(),
        stop_file: stop_file.clone(),
        paper_stop: paper_stop.clone(),
        collector_stop: collector_stop.clone(),
        api: Rc::new(RefCell::new(None)),
        stopping: stopping.clone(),
        signal: signal.clone(),
        recent: Rc::new(RefCell::new(Vec::new())),
        runtime: Rc::new(RefCell::new(json!({
            "status":"starting","pid":std::process::id(),"runtimeDir":runtime_dir,"readRequests":0,
            "errors":[],"cycles":0,"lastCycleAt":null_v(),
            "dashboard":format!("http://127.0.0.1:{}", c["port"].as_i64().unwrap_or(0)),
        }))),
    };
    let mut exit_code = 0i32;
    let mut failure: Option<RunnerError> = None;
    let mut first_close: Option<String> = None;
    let main = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_startup_and_loop(&ctx, &c, &root, &network_budget, network_interval_ms, &production_activation_file, &production_activation, max_cycles)
    }));
    match main {
        Ok(Ok(mut runner)) => {
            // ---- finally: ordered closes (runner.mjs:555) ----
            let reason = if runner.ctx.runtime.borrow()["status"].as_str() == Some("failed") {
                "COLLECTOR_FAILURE"
            } else {
                "STOP"
            };
            let _ = runner.publish_book_event("close", &json!({"marketId":V::Null,"reason":reason}));
            if let Some(mut m) = cell_take(&runner.collector_market_screen) {
                m.close();
            }
            if let Some(h) = cell_take(&runner.native_dip) {
                h.close();
            }
            if let Some(h) = cell_take(&runner.native_quote) {
                h.close();
            }
            if let Some(mut rt) = cell_take(&runner.realtime) {
                rt.stop();
            }
            for close in [
                Box::new(|| {
                    if let Some(mut h) = cell_take(&runner.dual_no_paper_v2) {
                        let _ = h.close();
                    }
                }) as Box<dyn FnMut()>,
                Box::new(|| {
                    if let Some(mut h) = cell_take(&runner.dual_no_paper) {
                        let _ = h.close();
                    }
                }),
                Box::new(|| {
                    if let Some(mut h) = cell_take(&runner.settlement_paper) {
                        h.close();
                    }
                }),
                Box::new(|| {
                    if let Some(mut h) = cell_take(&runner.settlement_portfolio) {
                        h.close();
                    }
                }),
                Box::new(|| {
                    if let Some(mut s) = cell_take(&runner.comparison_risk_source) {
                        drop(s);
                    }
                }),
                Box::new(|| {
                    if let Some(mut s) = cell_take(&runner.basket_followup_source) {
                        drop(s);
                    }
                }),
                Box::new(|| {
                    if let Some(mut cp) = cell_take(&runner.realtime_capture) {
                        let _ = cp.close();
                    }
                }),
                Box::new(|| {
                    if let Some(mut h) = cell_take(&runner.paired_live_host) {
                        let _ = h.close();
                    }
                }),
                Box::new(|| {
                    if let Some(s) = &runner.scoped_receipts {
                        let _ = drop(s);
                    }
                }),
                Box::new(|| {
                    if let Some(mut comp) = cell_take(&runner.competition) {
                        let _ = comp.close();
                    }
                }),
                Box::new(|| {
                    if let Some(a) = cell_take(&runner.api) {
                        a.close();
                    }
                }),
                Box::new(|| {
                    if let Some(mut s) = runner.server.take() {
                        drop(s);
                    }
                }),
            ] {
                let mut close = close;
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| close()))
                    .err()
                    .map(|_| ());
                // fallible closers were already mapped inside each closure; capture the first
                // error message for parity with the JS `closeFailure ??= error` ordering.
            }
            drop(runner);
        }
        Ok(Err(e)) => {
            failure = Some(e);
        }
        Err(payload) => {
            failure = Some(RunnerError::msg(
                payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "panic".into()),
            ));
        }
    }
    if let Some(err) = failure {
        let stopped = ctx.stopped()
            && !err.durable_recording_failed
            && ctx.runtime.borrow()["stopProvenance"]["cause"].as_str() != Some("status_publication_failed");
        {
            let mut rt = ctx.runtime.borrow_mut();
            rt["status"] = json!(if stopped { "stopped" } else { "failed" });
            let mut errors = rt["errors"].as_array().cloned().unwrap_or_default();
            errors.push(json!({"at":iso_now(),"reason":if stopped {"stop_requested".to_string()} else {err.message.clone()}}));
            rt["errors"] = V::Array(errors);
        }
        eprintln!("{}", if stopped { "Paper runner stopped" } else { "Paper runner failed; see local stats" });
        exit_code = if stopped { 0 } else { 1 };
    }
    exit_code
}

#[allow(clippy::too_many_arguments)]
fn run_startup_and_loop(
    ctx: &Ctx,
    c: &V,
    root: &Path,
    network_budget: &Option<V>,
    network_interval_ms: i64,
    production_activation_file: &Path,
    production_activation: &Option<V>,
    max_cycles: f64,
) -> R<Runner> {
    let runtime_dir = ctx.runtime_dir.clone();
    let workspace = ctx.workspace.clone();
    // ---- journal ----
    let journal_started = Instant::now();
    let native_journal = crate::collector_journal_native::collector_native_journal_config(&runtime_dir)
        .map_err(RunnerError::msg)?;
    let journal_file = runtime_dir.join("journal.jsonl");
    let ctxj = ctx.clone();
    let sig_flag = ctx.signal.clone();
    let journal = match native_journal {
        Some(binding) => {
            let sig_fn: &(dyn Fn() -> bool + Send + Sync) = &move || sig_flag.load(Ordering::Relaxed);
            // open_native_journal's signal parameter borrows this closure; the host keeps
            // it for the duration of the open only.
            let sig_fn_static: &'static (dyn Fn() -> bool + Send + Sync) =
                unsafe { std::mem::transmute(sig_fn) };
            let j = crate::native_journal_restore::open_native_journal(
                &runtime_dir,
                &binding["nativeBackend"],
                &journal_file,
                &runtime_dir.join("collector-native-journal-receipts"),
                Some(sig_fn_static),
                &move || !ctxj.stopped() && !ctxj.blocked_stops(),
            )
            .map_err(RunnerError::msg)?;
            PairedJournal::Native(j)
        }
        None => PairedJournal::Legacy(Journal::open(&journal_file).map_err(RunnerError::msg)?),
    };
    let journal_last = journal.inner().last.clone();
    let native_restore_v = journal.native_restore();
    let mut state = match &journal_last {
        Some(last) => crate::model::restore_state(&last["state"], c).map_err(RunnerError::msg)?,
        None => crate::model::initial_state(c, &random_uuid(), &iso_now()).map_err(RunnerError::msg)?,
    };
    crate::model::assert_state(&state, c).map_err(RunnerError::msg)?;
    {
        let mut rt = ctx.runtime.borrow_mut();
        let mut oj = serde_json::Map::new();
        oj.insert("revision".into(), json!(if native_restore_v.is_some() { 2 } else { 1 }));
        oj.insert("startupReplayMs".into(), json!(journal_started.elapsed().as_secs_f64() * 1000.0));
        if let Some(nr) = native_restore_v {
            oj.insert("native".into(), nr);
        }
        rt["originalJournal"] = V::Object(oj);
    }
    let official_sync = {
        let sync_file = runtime_dir.join("official-sync.json");
        sync_file.exists()
            && read_json_file(&sync_file).map(|v| v["enabled"] == V::Bool(true)).unwrap_or(false)
    };
    let mut account_cache = json!({"errors":{}});
    if official_sync {
        let file = runtime_dir.join("official-account.json");
        if file.exists() {
            account_cache = read_json_file(&file).map_err(|e| e)?;
        }
        if !account_cache.get("errors").is_some_and(|e| e.is_object()) {
            account_cache["errors"] = json!({});
        }
    }
    let universe = crate::breadth::validate_universe(
        &read_json_file(&root.join("price-universe.json")).map_err(RunnerError::msg)?,
        c["tournamentId"].as_str().unwrap_or(""),
    )
    .map_err(RunnerError::msg)?;
    let market_sweep = crate::market_sweep::load_market_sweep(
        &runtime_dir.join("market-sweep.json"),
        &universe,
        &network_budget.clone().unwrap_or(V::Null),
    )
    .map_err(RunnerError::msg)?;
    let ctxp = ctx.clone();
    let price_publication = if market_sweep.is_some() {
        Some(
            crate::market_price_publication::MarketPricePublication::new(
                &runtime_dir.join("market-prices.jsonl"),
                &universe,
                Box::new(move || !ctxp.stopped()) as Box<dyn Fn() -> bool>,
            )
            .map_err(RunnerError::msg)?,
        )
    } else {
        None
    };
    let comparison_risk_source = crate::comparison_risk_source::create_comparison_risk_source(
        runtime_dir.to_str().unwrap_or(""),
        &universe,
        now_ms,
        &workspace,
    )
    .ok();
    let retire_native_prices =
        crate::experiment_retirement::paper_experiment_retired(&runtime_dir, "NATIVE_PRICE_PAPER_STOP")
            .unwrap_or(false);
    if retire_native_prices {
        ctx.runtime.borrow_mut()["retiredPaperStrategies"] = json!({
            "nativeDip":true,"nativeQuote":true,"legacyEntryModel":true,
            "accountingPreserved":true,"executionAuthority":false
        });
    }
    let sig_opt = Some(ctx.signal.clone());
    let ctxh = AssertSend(ctx.clone());
    let can_op_host: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || !ctxh.get().stopped());
    let now_arc_i64: Arc<dyn Fn() -> i64 + Send + Sync> = Arc::new(now_ms_i64);
    let mut native_dip: Option<NativeDipH> = if retire_native_prices {
        None
    } else if runtime_dir.join("native-dip-intent-active.json").exists() {
        crate::native_dip_host_v3::open_native_dip_intent_host(
            &runtime_dir,
            &workspace,
            &universe,
            sig_opt.clone(),
            Some(can_op_host.clone()),
            Some(now_arc_i64.clone()),
        )
        .map_err(RunnerError::msg)?
        .map(NativeDipH::V3)
    } else if runtime_dir.join("native-dip-cache-active.json").exists() {
        crate::native_dip_host_v2::open_native_dip_cache_host(
            &runtime_dir,
            &workspace,
            &universe,
            sig_opt.clone(),
            Some(can_op_host.clone()),
            Some(now_arc_i64.clone()),
        )
        .map_err(RunnerError::msg)?
        .map(NativeDipH::V2)
    } else {
        crate::native_dip_host_v1::open_native_dip_host(
            &runtime_dir,
            &workspace,
            &universe,
            sig_opt.clone(),
            Some(can_op_host.clone()),
            Some(now_arc_i64.clone()),
        )
        .map_err(RunnerError::msg)?
        .map(NativeDipH::V1)
    };
    if let Some(h) = &native_dip {
        h.kick();
    }
    let ctxe = AssertSend(ctx.clone());
    let eligibility_publication = crate::account_eligibility_publication_v1::create_account_eligibility_publication(
        &runtime_dir.join("account-eligibility.jsonl"),
        &universe,
        Some(Arc::new(move || !ctxe.get().stopped()) as Arc<dyn Fn() -> bool + Send + Sync>),
    )
    .map_err(RunnerError::msg)?;
    let mut native_quote: Option<NativeDipH> = if retire_native_prices {
        None
    } else {
        crate::native_quote_host_v1::open_native_quote_host(
            &runtime_dir,
            &workspace,
            &universe,
            sig_opt.clone(),
            Some(can_op_host.clone()),
            Some(now_arc_i64.clone()),
        )
        .map_err(RunnerError::msg)?
        .map(NativeDipH::Quote)
    };
    if let Some(h) = &native_quote {
        h.kick();
    }
    if retire_native_prices {
        ctx.runtime.borrow_mut()["nativeDip"] = json!({"status":"retired","executionAuthority":false});
        ctx.runtime.borrow_mut()["nativeQuote"] = json!({"status":"retired","executionAuthority":false});
    }
    let price_publication_cell: Rc<RefCell<Option<MarketPricePublication<Box<dyn Fn() -> bool>>>>> = cell();
    *price_publication_cell.borrow_mut() = price_publication;
    let collector_market_screen = if native_dip.is_some() && price_publication_cell.borrow().is_some() {
        match (|| -> Result<CollectorMarketScreen, String> {
            let pp2 = AssertSend(price_publication_cell.clone());
            let ctxm = AssertSend(ctx.clone());
            let ctxm2 = AssertSend(ctx.clone());
            let mut m = crate::collector_market_screen_v1::CollectorMarketScreen::open(
                &runtime_dir,
                &universe,
                &workspace,
                Arc::new(move || pp2.get().borrow().as_ref().map(|p| p.status()).unwrap_or(V::Null)),
                Some(Arc::new(move || ctxm.get().signal.load(Ordering::Relaxed)) as Arc<dyn Fn() -> bool + Send + Sync>),
                Arc::new(move || !ctxm2.get().stopped()) as Arc<dyn Fn() -> bool + Send + Sync>,
                now_arc_i64.clone(),
            )?;
            let _ = m.initialize();
            Ok(m)
        })() {
            Ok(m) => Some(m),
            Err(_) => {
                ctx.runtime.borrow_mut()["marketHistoryScreen"] = json!({
                    "available":false,"status":"unavailable","reason":"COLLECTOR_MARKET_SCREEN_STARTUP_FAILED",
                    "candidateMarketIds":[],"executionAuthority":false
                });
                None
            }
        }
    } else {
        None
    };
    // ---- book publication ----
    let publication_started = Instant::now();
    let native_publication =
        crate::collector_publication_native::collector_native_publication_config(&runtime_dir, &universe, &workspace)
            .map_err(RunnerError::msg)?;
    let sid = random_uuid();
    let ctxb = ctx.clone();
    let ctxb2 = ctx.clone();
    let book_publication: Option<BookPub> = match &native_publication {
        Some(np) => Some(BookPub::V3(
            crate::book_publication_v3::NativeBookPublicationV3::open(
                &runtime_dir.join("public-books.jsonl"),
                &universe,
                &sid,
                &runtime_dir,
                &np["nativeBackend"],
                &runtime_dir.join("collector-native-publication-receipts"),
                now_arc_i64.clone(),
                {
                    let ctxb = AssertSend(ctxb);
                    Arc::new(move || !ctxb.get().stopped())
                },
            )
            .map_err(RunnerError::msg)?,
        )),
        None => Some(BookPub::V2(
            BookPublicationWriter::new(
                &runtime_dir.join("public-books.jsonl"),
                &universe,
                &sid,
                Box::new(now_ms_i64) as Box<dyn Fn() -> i64>,
                Box::new({
                    let ctxb2 = ctxb2.clone();
                    move || !ctxb2.stopped()
                }) as Box<dyn Fn() -> bool>,
            )
            .map_err(RunnerError::msg)?,
        )),
    };
    {
        let mut rt = ctx.runtime.borrow_mut();
        let mut bp = serde_json::Map::new();
        bp.insert("revision".into(), json!(if native_publication.is_some() { 3 } else { 2 }));
        bp.insert("startupReplayMs".into(), json!(publication_started.elapsed().as_secs_f64() * 1000.0));
        bp.insert("frozenPublisherFileChanged".into(), json!(false));
        if let Some(BookPub::V3(w)) = &book_publication {
            bp.insert("native".into(), w.startup());
        }
        rt["bookPublisher"] = V::Object(bp);
    }
    let settlement_paper = crate::settlement_paper_collector::open_collector_settlement_paper(
        &runtime_dir,
        &universe,
        now_arc_i64.clone(),
        &workspace,
        root,
    )
    .map_err(RunnerError::msg)?;
    if let Some(h) = &settlement_paper {
        let _ = h; // consume()+publish() called via Runner once constructed
    }
    // ---- runner ----
    let sampling_session_id = random_uuid();
    let paired_live_host_cell = cell();
    let median_preflight_cell = cell();
    let paired_candidate_cell = cell();
    let production_metadata_cell = cell();
    let trade_tape_cell = cell();
    let realtime_cell = cell();
    let competition_cell = cell();
    let basket_relationships_cell = cell();
    let contract_evidence_cell = cell();
    let contract_evidence_scope_cell = Rc::new(RefCell::new(Vec::new()));
    let mut runner = Runner {
        root: root.to_path_buf(),
        workspace: workspace.clone(),
        runtime_dir: runtime_dir.clone(),
        c: c.clone(),
        network_budget: network_budget.clone(),
        network_interval_ms,
        production_activation_file: production_activation_file.to_path_buf(),
        production_activation: production_activation.clone(),
        stop_file: ctx.stop_file.clone(),
        paper_stop: ctx.paper_stop.clone(),
        collector_stop: ctx.collector_stop.clone(),
        sampling_session_id,
        ctx: ctx.clone(),
        journal: cell(),
        state: state.clone(),
        account_cache,
        official_sync,
        api: cell(),
        authorize_gate: Rc::new(AuthGate {
            paired_live_host: paired_live_host_cell.clone(),
            median_preflight: median_preflight_cell.clone(),
            paired_candidate: paired_candidate_cell.clone(),
            production_metadata: production_metadata_cell.clone(),
            trade_tape: trade_tape_cell.clone(),
            realtime: realtime_cell.clone(),
            competition: competition_cell.clone(),
            basket_relationships: basket_relationships_cell.clone(),
            contract_evidence: contract_evidence_cell.clone(),
            contract_evidence_scope: contract_evidence_scope_cell.clone(),
            tournament_id: c["tournamentId"].as_str().unwrap_or("").to_string(),
        }),
        paired_api: Rc::new(RefCell::new(PairedLiveApi {
            dispatch: Rc::new(|_: &V, _: Option<&TGuard>, _: Option<&TGuard>| -> Result<V, WireDispatchError> {
                Err(wire_err("Stopped"))
            }),
            manual_review: false,
            cooldown: 0.0,
            next: 0.0,
            on_admission: Box::new(|_| {}),
        })),
        fences: Rc::new(RefCell::new(Vec::new())),
        server: None,
        server_shutdown: Arc::new(AtomicBool::new(false)),
        heartbeat_stop: Arc::new(AtomicBool::new(false)),
        heartbeat_handle: None,
        competition: competition_cell,
        realtime: realtime_cell,
        realtime_capture: cell(),
        basket_relationships: basket_relationships_cell,
        contract_evidence: contract_evidence_cell,
        contract_evidence_scope: contract_evidence_scope_cell,
        book_publication: cell(),
        basket_followup_source: cell(),
        basket_followup_bank_hash: Rc::new(RefCell::new(None)),
        comparison_risk_source: cell(),
        settlement_paper: cell(),
        settlement_portfolio: cell(),
        dual_no_paper: cell(),
        dual_no_paper_v2: cell(),
        production_metadata: production_metadata_cell,
        paired_candidate: paired_candidate_cell,
        candidate_account: Rc::new(RefCell::new(V::Null)),
        median_discovery: cell(),
        median_preflight: median_preflight_cell,
        paired_live_host: paired_live_host_cell,
        party_pair_selector: cell(),
        median_candidate_bulk_hints: Rc::new(RefCell::new(IndexMap::new())),
        scoped_receipts: None,
        universe: universe.clone(),
        coverage: None,
        coverage_rc: Rc::new(RefCell::new(None)),
        coverage_file: runtime_dir.join("price-coverage.json"),
        depth_plan: V::Null,
        dip_bank: None,
        scale_bank: None,
        responsive_bank: None,
        scheduler: V::Null,
        scheduler_scope: V::Null,
        scheduler_file: runtime_dir.join("book-scheduler.json"),
        market_sweep,
        price_publication: cell(),
        diagnostic_source: cell(),
        diagnostic_loading: false,
        diagnostic_failed: false,
        canonical_capacity: cell(),
        trade_tape: trade_tape_cell,
        no_maker: cell(),
        stable_maker: cell(),
        stable_maker_broad: cell(),
        coupled_maker: cell(),
        coupled_bulk: cell(),
        baal_pair: cell(),
        native_dip: cell(),
        native_quote: cell(),
        eligibility_publication: Rc::new(RefCell::new(Some(eligibility_publication))),
        collector_market_screen: cell(),
        baal_read_attention: cell(),
        retire_native_prices,
        money_receipt_writer: Rc::new(RefCell::new(None)),
    };
    *runner.journal.borrow_mut() = Some(journal);
    runner.price_publication = price_publication_cell;
    *runner.comparison_risk_source.borrow_mut() = comparison_risk_source;
    *runner.collector_market_screen.borrow_mut() = collector_market_screen;
    *runner.native_dip.borrow_mut() = native_dip;
    *runner.native_quote.borrow_mut() = native_quote;
    *runner.book_publication.borrow_mut() = book_publication;
    *runner.settlement_paper.borrow_mut() = settlement_paper;
    if runner.settlement_paper.borrow().is_some() {
        let mut sp = runner.settlement_paper.borrow_mut();
        let h = sp.as_mut().unwrap();
        h.consume().map_err(RunnerError::msg)?;
        h.publish().map_err(RunnerError::msg)?;
        let st = h.status().map_err(RunnerError::msg)?;
        drop(sp);
        runner.rt_set("settlementPaper", st);
    }
    runner.startup_after_journal()?;
    Ok(runner)
}

impl Runner {
    /// startup continuation after the journal exists — runner.mjs:295-410.
    fn startup_after_journal(&mut self) -> R<()> {
        let runtime_dir = self.runtime_dir.clone();
        let workspace = self.workspace.clone();
        let universe = self.universe.clone();
        let now_arc_i64: Arc<dyn Fn() -> i64 + Send + Sync> = Arc::new(now_ms_i64);
        let now_arc_f64: Arc<dyn Fn() -> f64 + Send + Sync> = Arc::new(now_ms);
        // settlementPortfolio — frozen-paper pause adapter takes priority.
        let frozen = if runtime_dir.join("paper-comparison-pauses.json").exists() {
            crate::frozen_paper_pause_adapter_v1::read_frozen_paper_pause(&json!({
                "runtimeDir": runtime_dir.to_str().unwrap_or(""),
                "kind": "settlement_portfolio_v2",
                "pointerFile": "settlement-portfolio-v2-active.json",
                "cohortDirName": "settlement-portfolio-v2-cohorts",
                "now": iso_now(),
            }))
            .map_err(RunnerError::msg)?
        } else {
            None
        };
        let settlement_portfolio = match frozen {
            Some(f) => Some(SettlementPort::Frozen(f)),
            None => {
                let ctx = AssertSend(self.ctx.clone());
                let can = Arc::new(move || ctx.get().can_operate_stops()) as Arc<dyn Fn() -> bool + Send + Sync>;
                crate::settlement_portfolio_collector_v2::open_collector_settlement_portfolio_v2(
                    &runtime_dir,
                    &universe,
                    can,
                    now_arc_i64.clone(),
                    &workspace,
                    &self.root,
                )
                .map_err(RunnerError::msg)?
                .map(SettlementPort::Host)
            }
        };
        if let Some(h) = &settlement_portfolio {
            self.rt_set("settlementPortfolio", h.status().unwrap_or(V::Null));
        }
        *self.settlement_portfolio.borrow_mut() = settlement_portfolio;
        let ctx = AssertSend(self.ctx.clone());
        let can_no_exec = Arc::new(move || !ctx.get().stopped() && !ctx.get().manual_review())
            as Arc<dyn Fn() -> bool + Send + Sync>;
        *self.dual_no_paper.borrow_mut() = crate::dual_no_pair_collector_v1::open_collector_dual_no_paper(
            &workspace,
            &runtime_dir,
            &universe,
            can_no_exec.clone(),
        )
        .map_err(RunnerError::msg)?
        .map(DualNoHost::V1);
        if let Some(h) = self.dual_no_paper.borrow().as_ref() {
            self.rt_set("dualNoPaper", h.status().unwrap_or(V::Null));
        }
        *self.dual_no_paper_v2.borrow_mut() = crate::dual_no_pair_collector_v2::open_collector_dual_no_paper_v2(
            &workspace,
            &runtime_dir,
            &universe,
            can_no_exec.clone(),
        )
        .map_err(RunnerError::msg)?
        .map(DualNoHost::V2);
        if let Some(h) = self.dual_no_paper_v2.borrow().as_ref() {
            self.rt_set("dualNoPaperV2", h.status().unwrap_or(V::Null));
        }
        match crate::canonical_capacity_monitor::open_canonical_capacity_monitor(
            &runtime_dir,
            &universe,
            &self.root,
            Some(now_arc_f64.clone()),
        )
        .map_err(RunnerError::msg)
        .and_then(|m| crate::canonical_capacity_guard::guard_canonical_capacity(m).map_err(RunnerError::msg))
        {
            Ok(guard) => {
                let st = guard.as_ref().and_then(|g| g.status().ok()).unwrap_or(json!({"status":"not_initialized","executionAuthority":false}));
                self.rt_set("canonicalCapacity", st);
                *self.canonical_capacity.borrow_mut() = guard;
            }
            Err(_) => {
                self.rt_set("canonicalCapacity", json!({"status":"unavailable","executionAuthority":false}));
            }
        }
        self.depth_plan = crate::depth_scope::validate_depth_plan(
            &read_json_file(&self.root.join("depth-plan.json")).map_err(RunnerError::msg)?,
            &self.c,
            &self.universe,
        )
        .map_err(RunnerError::msg)?;
        let dip_file = runtime_dir.join("dip-bank.json");
        if dip_file.exists() {
            let bank = read_json_file(&dip_file).map_err(|e| e)?;
            crate::dip_shadow::validate_dip_policy(&bank["policy"]).map_err(RunnerError::msg)?;
            if bank["universe"]["hash"] != universe["hash"]
                || bank["config"]["feeBps"].as_i64() != Some(0)
                || bank["policy"]["historyReadsPerCycle"].as_i64() != Some(2)
                || bank["policy"]["comparisonBooksPerCycle"].as_i64() != Some(2)
                || bank["policy"]["dynamicBooksPerCycle"].as_i64() != Some(2)
            {
                return Err(RunnerError::msg("Invalid dip collection plan"));
            }
            self.rt_set(
                "collectionPlan",
                json!({
                    "mode":"diversified_dip","historyReads":if self.official_sync {1} else {2},
                    "fullBookBudget":4,"managedCapacity":3,
                    "bulkReads":if self.market_sweep.is_some() { (universe["exchanges"].as_array().map(|a| a.len()).unwrap_or(0) + 99) / 100 } else { 1 },
                    "accountReads":"positions plus paginated orders","readIntervalMs":self.c["readIntervalMs"],
                }),
            );
            self.dip_bank = Some(bank);
        }
        let scale_file = runtime_dir.join("dip-scale-bank.json");
        if scale_file.exists() {
            let bank = read_json_file(&scale_file).map_err(|e| e)?;
            crate::model::validate_config(&bank["config"]).map_err(RunnerError::msg)?;
            crate::dip_shadow::validate_dip_policy(&bank["policy"]).map_err(RunnerError::msg)?;
            if self.dip_bank.is_none()
                || bank["universe"]["hash"] != universe["hash"]
                || bank["policy"]["profile"].as_str() != Some("scale")
            {
                return Err(RunnerError::msg("Invalid scaled dip bank"));
            }
            self.scale_bank = Some(bank);
        }
        let responsive_file = runtime_dir.join("dip-responsive-bank.json");
        if responsive_file.exists() {
            let bank = read_json_file(&responsive_file).map_err(|e| e)?;
            crate::model::validate_config(&bank["config"]).map_err(RunnerError::msg)?;
            crate::dip_shadow::validate_dip_policy(&bank["policy"]).map_err(RunnerError::msg)?;
            if bank["universe"]["hash"] != universe["hash"]
                || bank["policy"]["executionPolicy"].as_str() != Some("responsive_v2")
            {
                return Err(RunnerError::msg("Invalid responsive bank"));
            }
            self.responsive_bank = Some(bank);
        }
        self.scheduler_scope = json!({"universe":universe,"coreMarketIds":self.c["marketIds"]});
        self.scheduler = if self.scheduler_file.exists() {
            crate::book_scheduler::validate_scheduler_state(
                &read_json_file(&self.scheduler_file).map_err(RunnerError::msg)?,
                &self.scheduler_scope,
            )
            .map_err(RunnerError::msg)?
        } else {
            crate::book_scheduler::create_scheduler_state(&self.scheduler_scope).map_err(RunnerError::msg)?
        };
        if self.coverage_file.exists() {
            let cov = read_json_file(&self.coverage_file).map_err(|e| e)?;
            self.coverage = if cov["universeHash"] == universe["hash"] { Some(cov) } else { None };
        }
        *self.coverage_rc.borrow_mut() = self.coverage.clone();
        let last_commit = self.journal.borrow().as_ref().map(|j| j.inner().last.clone()).unwrap_or(None);
        if let Some(last) = &last_commit {
            if last["state"]["engineRevision"] != self.state["engineRevision"] {
                self.commit(json!({
                    "type":"paper_engine_upgrade",
                    "fromRevision":if last["state"]["engineRevision"].is_null() {json!(1)} else {last["state"]["engineRevision"].clone()},
                    "toRevision":self.state["engineRevision"],
                }))?;
            }
        }
        // ---- scoped receipts + money fetch + the serialized ReadClient ----
        if runtime_dir.join("paired-live-request.json").exists()
            || runtime_dir.join("paired-live-v2-request.json").exists()
        {
            let paired_cell = AssertSend(self.paired_live_host.clone());
            let ctx = self.ctx.clone();
            let scope = json!({
                "tournamentId": self.c["tournamentId"], "tournamentSlug": self.c["tournamentSlug"],
                "marketIds": universe["exchanges"].as_array().cloned().unwrap_or_default().iter().map(|x| x["marketId"].clone()).collect::<Vec<V>>(),
                "ownedAllMarketId": if runtime_dir.join("baal-owned-all-read-request.json").exists() { json!("384") } else { V::Null },
            });
            let scope = if scope["ownedAllMarketId"].is_null() {
                let mut m = scope.as_object().cloned().unwrap_or_default();
                m.remove("ownedAllMarketId");
                V::Object(m)
            } else {
                scope
            };
            let receipts = baal_execution::scoped_rest_receipts::create_scoped_rest_receipts(
                Rc::new(|url: &str, opts: &V| scoped_fetch_fn(url, opts)),
                &scope,
                Rc::new(now_ms),
                Rc::new(move |event: &V| -> Result<(), String> {
                    let h = paired_cell.get().borrow();
                    match h.as_ref() {
                        Some(h) if h.on_fetch_start(event) => Ok(()),
                        _ => Err("REST_RECEIPT_START_RECORDING_FAILED".into()),
                    }
                }),
                64,
                16 * 1024 * 1024,
            )
            .map_err(RunnerError::msg)?;
            self.scoped_receipts = Some(Rc::new(receipts));
        }
        let ctxw = AssertSend(self.ctx.clone());
        let money_writer_cell = AssertSend(self.money_receipt_writer.clone());
        let scoped = AssertSend(self.scoped_receipts.clone());
        let base_fetch: &'static dyn Fn(&str, &V) -> Result<V, String> =
            Box::leak(Box::new(move |u: &str, o: &V| -> Result<V, String> {
                match &scoped.get() {
                    Some(s) => s.fetch(u, o).and_then(|mut r| {
                        let mut value = r.value.clone();
                        let body = r.text().unwrap_or_default();
                        value["bodyText"] = json!(body);
                        Ok(value)
                    }),
                    None => raw_fetch_v(u, o),
                }
            }));
        let on_receipt_inner = AssertSend(move |receipt: &V| -> Result<(), String> {
            {
                let mut guard = money_writer_cell.get().borrow_mut();
                if guard.is_none() {
                    let runtime_dir = ctxw.get().runtime_dir.clone();
                    let ctx3 = ctxw.get().clone();
                    *guard = Some(
                        MoneyReceiptWriter::open(
                            runtime_dir.join("exact-money-receipts.jsonl"),
                            Box::leak(Box::new(move || {
                                !ctx3.stopped() && !ctx3.blocked_stops()
                            })) as &'static dyn Fn() -> bool,
                        )
                        .map_err(|e| e.to_string())?,
                    );
                }
            }
            let mut guard = money_writer_cell.get().borrow_mut();
            let w = guard.as_mut().unwrap();
            match w.commit(receipt) {
                Ok(checkpoint) => {
                    ctxw.get().runtime.borrow_mut()["exactMoneyReceipts"] = json!({
                        "sequence":checkpoint["sequence"],"lastKind":receipt["kind"],
                        "receivedAt":receipt["receivedAt"],
                        "exact":receipt["fields"].as_object().map(|m| m.values().all(|x| x["available"].as_bool()==Some(true))).unwrap_or(false),
                        "executionAuthority":false
                    });
                }
                Err(_) => {}
            }
            Ok(())
        });
        let money_options: &'static MoneyReceiptOptions<'static> = Box::leak(Box::new(
            MoneyReceiptOptions {
                tournament_id: Box::leak(
                    self.c["tournamentId"].as_str().unwrap_or("").to_string().into_boxed_str(),
                ),
                tournament_slug: Box::leak(
                    self.c["tournamentSlug"].as_str().unwrap_or("").to_string().into_boxed_str(),
                ),
                on_receipt: Box::leak(Box::new(move |r: &V| (on_receipt_inner.get())(r)))
                    as &'static dyn Fn(&V) -> Result<(), String>,
                now: Box::leak(Box::new(now_ms_i64)) as &'static dyn Fn() -> i64,
            },
        ));
        let money_f = AssertSend(baal_execution::receipts::exact_money_fetch(
            base_fetch,
            money_options,
        ));
        let money_fetch: Arc<dyn Fn(&str, &V) -> Result<V, String> + Send + Sync> =
            Arc::new(move |u: &str, o: &V| (money_f.get())(u, o));
        // ---- ReadClient ----
        let ctxa = self.ctx.clone();
        let ctxr = self.ctx.clone();
        let recent = self.ctx.recent.clone();
        let runtime_for_timing = self.ctx.runtime.clone();
        let rate_file = runtime_dir.join("rate.json");
        let api_holder = AssertSend(self.api.clone());
        let authorize_cell = AssertSend(self.paired_live_host.clone());
        let gate = AuthGate {
            paired_live_host: self.paired_live_host.clone(),
            median_preflight: self.median_preflight.clone(),
            paired_candidate: self.paired_candidate.clone(),
            production_metadata: self.production_metadata.clone(),
            trade_tape: self.trade_tape.clone(),
            realtime: self.realtime.clone(),
            competition: self.competition.clone(),
            basket_relationships: self.basket_relationships.clone(),
            contract_evidence: self.contract_evidence.clone(),
            contract_evidence_scope: self.contract_evidence_scope.clone(),
            tournament_id: self.c["tournamentId"].as_str().unwrap_or("").to_string(),
        };
        let gate = Rc::new(gate);
        self.authorize_gate = gate.clone();
        let gate = AssertSend(gate);
        let options = ReadClientOptions {
            interval: self.c["readIntervalMs"].as_i64().unwrap_or(0) as u64,
            network_budget: self.network_budget.clone(),
            fetch: Some(Arc::new(RunnerFetch { money_fetch: money_fetch.clone() })),
            signal: Some(self.ctx.signal.clone()),
            authorize_dispatch: Some(Arc::new(move |spec: &V| gate.get().authorize(spec)) as Authorizer),
            on_admission: Some(Arc::new(move |next: Option<f64>| {
                let _ = baal_service_runtime::atomic::atomic_json(
                    &rate_file,
                    &json!({"next":next,"manualReview":next.is_none()}),
                );
            }) as Arc<dyn Fn(Option<f64>) + Send + Sync>),
            on_timing: Some(Arc::new({
                let recent = AssertSend(recent.clone());
                let runtime_for_timing = AssertSend(runtime_for_timing.clone());
                move |event: &V| -> Result<(), String> {
                let recent = recent.get();
                let runtime_for_timing = runtime_for_timing.get();
                let _ = crate::read_timing::validate_timing(event);
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(runtime_dir.join("read-timings.jsonl"))
                {
                    use std::io::Write;
                    let _ = f.write_all((serde_json::to_string(event).unwrap_or_default() + "\n").as_bytes());
                }
                let mut rec = recent.borrow_mut();
                rec.push(event.clone());
                if rec.len() > 1000 {
                    rec.remove(0);
                }
                drop(rec);
                let rec2 = recent.borrow();
                if let Ok(ts) = crate::read_timing::timing_summary(&rec2) {
                    runtime_for_timing.borrow_mut()["readTiming"] = ts;
                }
                Ok(())
                }
            }) as Arc<dyn Fn(&V) -> Result<(), String> + Send + Sync>),
            ..Default::default()
        };
        let key = load_key().map_err(RunnerError::msg)?;
        let client = ReadClient::new(&key, options).map_err(RunnerError::msg)?;
        *self.api.borrow_mut() = Some(Api::new(
            client,
            self.network_budget.is_some(),
            &self.ctx,
            self.fences.clone(),
            self.authorize_gate.clone(),
            self.runtime_dir.join("rate.json"),
            self.paired_api.clone(),
        ));
        self.sync_paired_api();
        let _ = api_holder;
        let _ = authorize_cell;
        self.host_construction().map_err(|e| e)?;
        Ok(())
    }

    fn sync_paired_api(&self) {
        let mut pa = self.paired_api.borrow_mut();
        let ctx = self.ctx.clone();
        let fences = self.fences.clone();
        let api_cell = self.api.clone();
        pa.dispatch = Rc::new(move |spec: &V, bd: Option<&TGuard>, bn: Option<&TGuard>| -> Result<V, WireDispatchError> {
            api_cell.borrow().as_ref().map(|a| a.dispatch_transport(spec, bd, bn, &fences)).unwrap_or(Err(wire_err("Stopped")))
        });
    }

    // host construction after `api` exists — runner.mjs:323-345
    fn host_construction(&mut self) -> R<()> {
        let runtime_dir = self.runtime_dir.clone();
        let universe = self.universe.clone();
        let ctx = self.ctx.clone();
        let now_i64: Arc<dyn Fn() -> i64 + Send + Sync> = Arc::new(now_ms_i64);
        let now_f64: Arc<dyn Fn() -> f64 + Send + Sync> = Arc::new(now_ms);
        *self.production_metadata.borrow_mut() = Some(crate::production_metadata::open_production_metadata(
            &runtime_dir,
            &universe,
            Arc::from(self.mk_reader_dispatch()),
            Some({
                let ctx = AssertSend(ctx.clone());
                Arc::new(move || !ctx.get().stopped()) as Arc<dyn Fn() -> bool + Send + Sync>
            }),
            Some({
                Arc::new(now_ms) as Arc<dyn Fn() -> f64 + Send + Sync>
            }),
        ));
        *self.median_preflight.borrow_mut() = {
            let ctx = ctx.clone();
            let can_op: &'static dyn Fn() -> bool = Box::leak(Box::new(move || ctx.can_operate_stops()));
            let api = AssertSend(self.api.clone());
            let fences = AssertSend(self.fences.clone());
            median_preflight_wrapper(
                &runtime_dir,
                &universe,
                Box::new(move |spec: &V, bd: Option<&TGuard>, bn: Option<&TGuard>| {
                    api.get()
                        .borrow()
                        .as_ref()
                        .map(|a| a.dispatch_transport(spec, bd, bn, fences.get()).map_err(|e| transport_error(&e.message)))
                        .unwrap_or_else(|| Err(transport_error("STOPPED")))
                }),
                can_op,
                Rc::new(now_ms_i64),
            )
        };
        self.rt_set(
            "networkBudget",
            json!({
                "mode":match self.network_budget.as_ref().and_then(|b| b["schemaVersion"].as_i64()){Some(3)=>"coordinated_shared_read_only",Some(_)=>"coordinated_sole_client",None=>"default"},
                "schemaVersion":self.network_budget.as_ref().map(|b| b["schemaVersion"].clone()).unwrap_or(V::Null),
                "intervalMs":self.network_interval_ms,
                "writeIntervalMs":self.api().client.write_interval,
                "completionPacing":self.api().completion_pacing,
                "maxReadsPerMinute":if self.network_budget.as_ref().and_then(|b| b["schemaVersion"].as_i64())==Some(3){json!(20)}else{V::Null},
                "writesEnabled":if self.network_budget.as_ref().and_then(|b| b["schemaVersion"].as_i64())==Some(3){json!(false)}else{V::Null},
                "executionOwner":if self.network_budget.as_ref().and_then(|b| b["schemaVersion"].as_i64())==Some(3){json!("Daenerys")}else{V::Null},
            }),
        );
        if let Some(cp) = self.ctx.runtime.borrow_mut().get_mut("collectionPlan") {
            cp["readIntervalMs"] = json!(self.network_interval_ms);
        }
        // competition
        if let Some(a) = &self.production_activation {
            if a["enabled"] == V::Bool(true) {
                let kind = self.controller_for_activation(a).map_err(RunnerError::msg)?;
                let comp = self.new_competition(kind, a).map_err(RunnerError::msg)?;
                *self.competition.borrow_mut() = Some(comp);
                if a["strategyId"].as_str() == Some("historical-median-receipt-v1") {
                    self.competition.borrow_mut().as_mut().unwrap().initialize_median_reference()?;
                }
            }
        }
        // paired live host
        self.open_paired_live_host_if_active()?;
        // party pair selector
        if runtime_dir.join("party-pair-source-plan.json").exists() {
            let candidate_cell = AssertSend(self.candidate_account.clone());
            let comp_cell = AssertSend(self.competition.clone());
            let capture_cell = AssertSend(self.paired_candidate.clone());
            let host_cell = AssertSend(self.paired_live_host.clone());
            let ctx2 = ctx.clone();
            *self.party_pair_selector.borrow_mut() =
                crate::party_winner_pair_collector_v1::PartyWinnerPairCollector::open(
                    &runtime_dir,
                    &universe,
                    Arc::new(move || {
                        let acc = candidate_cell.get().borrow().clone();
                        if acc.is_null() {
                            return V::Null;
                        }
                        let mut a = acc.as_object().cloned().unwrap_or_default();
                        let uncertain = comp_cell
                            .get()
                            .borrow()
                            .as_ref()
                            .and_then(|c| c.active())
                            .map(|a| a["acceptanceUncertain"].as_bool() == Some(true))
                            .unwrap_or(false);
                        a.insert("unknownAcceptance".into(), json!(uncertain));
                        V::Object(a)
                    }),
                    Arc::new(move || {
                        capture_cell
                            .get()
                            .borrow()
                            .as_ref()
                            .map(|p| p.status())
                            .unwrap_or(V::Null)
                    }),
                    Arc::new(move || {
                        host_cell
                            .get()
                            .borrow()
                            .as_ref()
                            .map(|h| h.status()["selectionHash"].clone())
                            .unwrap_or(V::Null)
                    }),
                    {
                        let ctx2 = AssertSend(ctx2.clone());
                        Arc::new(move || ctx2.get().can_operate_full())
                    },
                    Arc::new(now_ms) as Arc<dyn Fn() -> f64 + Send + Sync>,
                )
                .map_err(|e: crate::party_winner_pair_collector_v1::PairFault| RunnerError::msg(e.message))?;
        }
        // rate.json restore
        let rate_file = runtime_dir.join("rate.json");
        if rate_file.exists() {
            let rate = read_json_file(&rate_file).map_err(|e| e)?;
            if rate["manualReview"] == V::Bool(true) || rate["next"].is_null() {
                return Err(RunnerError::msg("Rate cooldown requires manual review"));
            }
            let next = rate["next"].as_f64().unwrap_or(f64::NAN);
            if !(next.is_finite() && next >= 0.0 && next <= 8640000000000000.0) {
                return Err(RunnerError::msg("Invalid persisted rate state"));
            }
            if let Some(a) = self.api.borrow_mut().as_mut() {
                a.set_next(next);
            }
        }
        // realtime capture + host
        self.start_realtime()?;
        if self.network_budget.is_some() {
            self.commit(json!({
                "type":"collection_network_budget","clockAt":iso_now(),
                "cadencePolicy":{
                    "mode":self.rt_get("networkBudget")["mode"],
                    "schemaVersion":self.network_budget.as_ref().map(|b| b["schemaVersion"].clone()).unwrap_or(V::Null),
                    "intervalMs":self.api().client.interval,
                    "writeIntervalMs":self.api().client.write_interval,
                    "completionPacing":true,
                    "maxReadsPerMinute":if self.network_budget.as_ref().and_then(|b| b["schemaVersion"].as_i64())==Some(3){json!(20)}else{V::Null},
                    "writesEnabled":if self.network_budget.as_ref().and_then(|b| b["schemaVersion"].as_i64())==Some(3){json!(false)}else{V::Null},
                },
            }))?;
        }
        self.rt_set("startupToConnectionMs", json!(now_ms() - 0.0));
        self.rt_set("status", json!("connecting"));
        let _ = self.persist_status();
        self.host_chain_and_loop()?;
        Ok(())
    }

    fn host_chain_and_loop(&mut self) -> R<()> {
        let runtime_dir = self.runtime_dir.clone();
        let workspace = self.workspace.clone();
        let universe = self.universe.clone();
        let ctx = self.ctx.clone();
        let now_i64: Arc<dyn Fn() -> i64 + Send + Sync> = Arc::new(now_ms_i64);
        let max_cycles = self.c["cycles"].as_f64().unwrap_or(f64::INFINITY);
        // median discovery + paired candidate + maker hosts + trade tape + readers
        if self.price_publication.borrow().is_some()
            && !runtime_dir.join("baal-scaled-live-request.json").exists()
        {
            let ctx2 = ctx.clone();
            let api = AssertSend(self.api.clone());
            let comp_cell = AssertSend(self.competition.clone());
            let universe2 = universe.clone();
            let cand = AssertSend(self.candidate_account.clone());
            let rt = AssertSend(ctx.runtime.clone());
            let eligibility_cell = AssertSend(Rc::new(RefCell::new(()))); // placeholder — eligibility publication is &mut
            let _ = eligibility_cell;
            let price_cell = AssertSend(self.price_publication.clone());
            let runner_self = AssertSend(self as *mut Runner); // synchronous self-callbacks
            let _ = runner_self;
            let md = crate::collector_median_discovery_v3::CollectorMedianDiscoveryV3::open(
                &runtime_dir,
                &workspace,
                &universe,
                Arc::new(move || price_cell.get().borrow().as_ref().map(|p| p.status()).unwrap_or(V::Null)),
                Arc::new(move |endpoint: &str| -> Result<V, crate::collector_median_discovery_v3::MedianFault> {
                    match api.get().borrow().as_ref().map(|a| a.client.request(endpoint)) {
                        None => Err(crate::collector_median_discovery_v3::MedianFault {
                            message: "STOPPED".into(), status: None,
                            durable_recording_failed: false, fatal: false,
                        }),
                        Some(Ok(v)) => Ok(v),
                        Some(Err(e)) => Err(crate::collector_median_discovery_v3::MedianFault {
                            message: e.message, status: None,
                            durable_recording_failed: false, fatal: false,
                        }),
                    }
                }),
                Arc::new(move || -> Result<V, crate::collector_median_discovery_v3::MedianFault> {
                    Err(crate::collector_median_discovery_v3::MedianFault {
                        message: "refresh_account_called_before_bind".into(),
                        status: None,
                        durable_recording_failed: false,
                        fatal: false,
                    })
                }),
                Arc::new(move || cand.get().borrow().clone()),
                {
                    let ctx2 = AssertSend(ctx2.clone());
                    Arc::new(move |reads: i64| ctx2.get().can_operate_full() && ctx2.get().has_read_headroom(reads)) as Arc<dyn Fn(i64) -> bool + Send + Sync>
                },
                {
                    let api = AssertSend(self.api.clone());
                    let fences = AssertSend(self.fences.clone());
                    let ctx3 = AssertSend(ctx.clone());
                    Arc::new(move |market_id: &str, response: &V| -> V {
                        let _ = &ctx3;
                        let _ = crate::model::normalize_book(
                            &response["data"], market_id, "", response["receivedAt"].as_str().unwrap_or(""),
                            response["requestStartedAt"].as_str().unwrap_or(""), 0, &BTreeSet::new(), "authoritative_receipt_v1",
                        );
                        let _ = (&api, &fences, &ctx3);
                        V::Null
                    })
                },
                Some(Arc::new(move || -> Result<V, crate::collector_median_discovery_v3::MedianFault> {
                    let comp = comp_cell.get().borrow();
                    match comp.as_ref() {
                        Some(c) if c.activation()["strategyId"].as_str() == Some("historical-median-receipt-v1") && c.can_operate() => {
                            let _ = c.bank_current();
                            let st = c.state();
                            let universe3 = universe2.clone();
                            Ok(json!({
                                "schemaVersion":1,"strategyId":c.activation()["strategyId"],"runId":c.activation()["runId"],
                                "bankHash":c.scheduling_bank_hash().map(V::from).unwrap_or_else(|| c.activation()["strategyBankHash"].clone()),
                                "universeHash":universe3["hash"],"observedAt":iso_now(),
                                "owned":st["owned"].as_object().map(|m| m.iter().filter(|(_,p)| p["quantity"].as_f64().unwrap_or(0.0)>0.0).map(|(ex,p)| json!({"marketId":universe3["exchanges"].as_array().cloned().unwrap_or_default().iter().find(|x| x["exchangeId"].as_str()==Some(ex.as_str())).and_then(|x| x["marketId"].as_str().map(String::from)),"exchangeId":ex,"quantity":p["quantity"]})).collect::<Vec<V>>()).unwrap_or_default(),
                                "proposals":c.proposals()["proposals"].as_object().map(|m| m.values().filter(|p| p["status"].as_str()==Some("proposed")&&now_ms()<parse_iso_ms(p["expiresAt"].as_str().unwrap_or("")).unwrap_or(0.0)).map(|p| json!({"intentId":p["intentId"],"marketId":p["marketId"],"exchangeId":p["exchangeId"],"bankHash":p["bankHash"],"action":p["action"],"status":p["status"],"createdAt":p["createdAt"],"expiresAt":p["expiresAt"]})).collect::<Vec<V>>()).unwrap_or_default(),
                            }))
                        }
                        _ => Ok(V::Null),
                    }
                })),
                None,
                {
                    let comp_cell = AssertSend(self.competition.clone());
                    let cand2 = AssertSend(self.candidate_account.clone());
                    Some(Arc::new(move |market_id: &str, response: &V| -> Result<V, crate::collector_median_discovery_v3::MedianFault> {
                        let _ = market_id;
                        let mut guard = comp_cell.get().borrow_mut();
                        if let Some(c) = guard.as_mut() {
                            if let Err(e) = c.on_book(response) {
                                c.failure(&json!({"message": e.message}));
                            }
                        }
                        drop(guard);
                        *cand2.get().borrow_mut() = V::Null;
                        Ok(V::Null)
                    }))
                },
                {
                    let ctx_ops = AssertSend(ctx.clone());
                    Arc::new(move || ctx_ops.get().can_operate_full()) as Arc<dyn Fn() -> bool + Send + Sync>
                },
                now_i64.clone(),
            )
            .map_err(|e: crate::collector_median_discovery_v3::MedianFault| RunnerError::msg(e.message))?;
            *self.median_discovery.borrow_mut() = md;
            self.median_discovery.borrow_mut().as_mut().unwrap().initialize();
            let _ = rt;
        }
        *self.paired_candidate.borrow_mut() = Some(crate::paired_candidate_collector::PairedCandidateCollector::open(
            &runtime_dir,
            &universe,
            Arc::from(self.mk_service_dispatch()),
            {
                let attention = AssertSend(self.baal_read_attention.clone());
                Arc::new(move |n: i64| -> Result<V, String> {
                    attention.get().borrow_mut().as_mut().map(|a| a.reserve(&json!(n), true)).unwrap_or(Err("READ_ATTENTION_MISSING".into()))
                })
            },
            {
                let ctx = AssertSend(ctx.clone());
                Arc::new(move |_: &V| ctx.get().can_operate_full()) as Arc<dyn Fn(&V) -> bool + Send + Sync>
            },
            {
                let cand = AssertSend(self.candidate_account.clone());
                Arc::new(move || cand.get().borrow().clone()) as Arc<dyn Fn() -> V + Send + Sync>
            },
            {
                let ctx = AssertSend(ctx.clone());
                Arc::new(move || ctx.get().can_operate_stops()) as Arc<dyn Fn() -> bool + Send + Sync>
            },
            Some({
                let sig = self.ctx.signal.clone();
                Arc::new(move || sig.load(Ordering::Relaxed)) as Arc<dyn Fn() -> bool + Send + Sync>
            }),
            Arc::new(now_ms_i64) as Arc<dyn Fn() -> i64 + Send + Sync>,
        )
        .map_err(|e: baal_service_runtime::client::ServiceError| RunnerError::msg(e.message))?);
        // noMaker
        {
            let retired = crate::expired_flat_paper_retirement::read_expired_flat_paper_retirement(
                &runtime_dir,
                "no-maker-active.json",
                "no-maker-cohorts",
            )
            .map_err(RunnerError::msg)?;
            if !retired.is_null() {
                self.rt_set("noMaker", retired);
        } else {
            match (|| -> Result<Option<NoMakerH>, String> {
                let active_file = runtime_dir.join("no-maker-active.json");
                let revision = if active_file.exists() {
                    read_json_file(&active_file)?["hostRevision"].as_i64().unwrap_or(1)
                } else {
                    1
                };
                if !(1..=5).contains(&revision) {
                    return Err("Unknown maker host revision".into());
                }
                if active_file.exists() {
                    let manifest = read_json_file(&active_file).map_err(|e| e)?;
                    let bank = read_json_file(
                        &runtime_dir.join("no-maker-cohorts").join(manifest["cohort"].as_str().unwrap_or("")).join("bank.json"),
                    )
                    .map_err(|e| e)?;
                    let module_path = match revision {
                        5 => "live-paper/no-maker-host-v5.mjs",
                        4 => "live-paper/no-maker-host-v4.mjs",
                        3 => "live-paper/no-maker-host-v3.mjs",
                        2 => "live-paper/no-maker-host-v2.mjs",
                        _ => "live-paper/no-maker-host.mjs",
                    };
                    crate::frozen_paper_runtime::frozen_paper_module(&runtime_dir, &bank, module_path)?;
                }
                let ctx2 = AssertSend(self.ctx.clone());
                let can = Arc::new(move || !ctx2.get().stopped() && !ctx2.get().manual_review())
                    as Arc<dyn Fn() -> bool + Send + Sync>;
                let now = Arc::new(now_ms_i64) as Arc<dyn Fn() -> i64 + Send + Sync>;
                Ok(match revision {
                    5 => crate::no_maker_host_v5::open_no_maker_host_v5(&runtime_dir, &self.root, now, can)?.map(NoMakerH::V5),
                    4 => crate::no_maker_host_v4::open_no_maker_host_v4(&runtime_dir, &self.root, now, can)?.map(NoMakerH::V4),
                    3 => crate::no_maker_host_v3::open_no_maker_host_v3(&runtime_dir, &self.root, now, can)?.map(NoMakerH::V3),
                    2 => crate::no_maker_host_v2::open_no_maker_host_v2(&runtime_dir, &self.root, now, can)?.map(NoMakerH::V2),
                    _ => crate::no_maker_host::open_no_maker_host(&runtime_dir, &self.root, now, can)?.map(NoMakerH::V1),
                })
            })() {
                Ok(h) => {
                    let st = h.as_ref().and_then(|h| h.status().ok()).unwrap_or(json!({"status":"not_initialized","executionAuthority":false}));
                    *self.no_maker.borrow_mut() = h;
                    self.rt_set("noMaker", st);
                }
                Err(_) => {
                    self.rt_set("noMaker", json!({"status":"failed","executionAuthority":false}));
                }
            }
            }
        }
        // baalPair
        {
            let retired = crate::expired_flat_paper_retirement::read_expired_flat_paper_retirement(
                &runtime_dir,
                "baal-pair-active.json",
                "baal-pair-cohorts",
            )
            .map_err(RunnerError::msg)?;
            if !retired.is_null() {
                self.rt_set("baalPair", retired);
            } else {
            match (|| -> Result<Option<BaalPairH>, String> {
                let pair_file = runtime_dir.join("baal-pair-active.json");
                let revision = if pair_file.exists() {
                    read_json_file(&pair_file)?["hostRevision"].as_i64().unwrap_or(1)
                } else {
                    1
                };
                if !(1..=4).contains(&revision) {
                    return Err("Unknown pair host revision".into());
                }
                if pair_file.exists() {
                    let manifest = read_json_file(&pair_file).map_err(|e| e)?;
                    let bank = read_json_file(
                        &runtime_dir.join("baal-pair-cohorts").join(manifest["cohort"].as_str().unwrap_or("")).join("bank.json"),
                    )
                    .map_err(|e| e)?;
                    let module_path = match revision {
                        4 => "live-paper/baal-pair-host-v4.mjs",
                        3 => "live-paper/baal-pair-host-v3.mjs",
                        2 => "live-paper/baal-pair-host-v2.mjs",
                        _ => "live-paper/baal-pair-host.mjs",
                    };
                    crate::frozen_paper_runtime::frozen_paper_module(&runtime_dir, &bank, module_path)?;
                }
                let ctx2 = AssertSend(self.ctx.clone());
                let can = Arc::new(move || !ctx2.get().stopped() && !ctx2.get().manual_review())
                    as Arc<dyn Fn() -> bool + Send + Sync>;
                let now = Arc::new(now_ms_i64) as Arc<dyn Fn() -> i64 + Send + Sync>;
                Ok(match revision {
                    4 => crate::baal_pair_host_v4::BaalPairHost::open(&runtime_dir, now, can)?.map(BaalPairH::V4),
                    3 => crate::baal_pair_host_v3::BaalPairHost::open(&runtime_dir, now, can)?.map(BaalPairH::V3),
                    2 => crate::baal_pair_host_v2::BaalPairHost::open(&runtime_dir, now, can)?.map(BaalPairH::V2),
                    _ => crate::baal_pair_host::BaalPairHost::open(&runtime_dir, now, can)?.map(BaalPairH::V1),
                })
            })() {
                Ok(h) => {
                    let st = h.as_ref().map(|h| h.status()).unwrap_or(json!({"status":"not_initialized","executionAuthority":false}));
                    *self.baal_pair.borrow_mut() = h;
                    self.rt_set("baalPair", st);
                }
                Err(_) => {
                    self.rt_set("baalPair", json!({"status":"failed","executionAuthority":false}));
                }
            }
            }
        }
        // stableMakerBroad + stableMaker
        for (key, active_name) in [
            ("stableMakerBroad", "stable-maker-broad-active.json"),
            ("stableMaker", "stable-maker-active.json"),
        ] {
            let slot = if key == "stableMaker" { &self.stable_maker } else { &self.stable_maker_broad };
            let _ = slot;
            let result: Result<Option<StableMakerH>, ()> = (|| {
                let active_file = runtime_dir.join(active_name);
                if !active_file.exists() {
                    return Ok(None);
                }
                let active = read_json_file(&active_file).map_err(|_| ())?;
                let dir = runtime_dir.join("stable-maker-cohorts").join(active["cohort"].as_str().unwrap_or(""));
                if dir.join("STOP").exists() {
                    self.rt_set(key, json!({"status":"retired","accountingPreserved":true,"executionAuthority":false}));
                    return Ok(None);
                }
                let bank = read_json_file(&dir.join("bank.json")).map_err(|_| ())?;
                crate::frozen_paper_runtime::frozen_paper_module(&runtime_dir, &bank, "live-paper/stable-maker-host-v1.mjs").map_err(|_| ())?;
                let ctx2 = AssertSend(self.ctx.clone());
                let host = crate::stable_maker_host_v1::StableMakerHost::open(
                    &runtime_dir,
                    Arc::new(now_ms_i64),
                    Arc::new(move || !ctx2.get().stopped() && !ctx2.get().manual_review()),
                    active_name,
                    &workspace,
                )
                .map_err(|_| ())?;
                Ok(host.map(StableMakerH::Stable))
            })();
            match result {
                Ok(Some(h)) => {
                    let st = h.status().unwrap_or(V::Null);
                    if key == "stableMaker" {
                        *self.stable_maker.borrow_mut() = Some(h);
                    } else {
                        *self.stable_maker_broad.borrow_mut() = Some(h);
                    }
                    self.rt_set(key, st);
                }
                Ok(None) => {}
                Err(()) => self.stable_host_failed(key),
            }
        }
        // coupledMaker
        {
            let result: Result<(), ()> = (|| {
                let active_file = runtime_dir.join("coupled-maker-active.json");
                if !active_file.exists() {
                    return Ok(());
                }
                let active = read_json_file(&active_file).map_err(|_| ())?;
                let dir = runtime_dir.join("coupled-maker-cohorts").join(active["cohort"].as_str().unwrap_or(""));
                if dir.join("STOP").exists() {
                    self.rt_set("coupledMaker", json!({"status":"stopped","accountingPreserved":true,"executionAuthority":false}));
                    return Ok(());
                }
                let bank = read_json_file(&dir.join("bank.json")).map_err(|_| ())?;
                crate::frozen_paper_runtime::frozen_paper_module(&runtime_dir, &bank, "live-paper/coupled-maker-host-v1.mjs").map_err(|_| ())?;
                let ctx2 = AssertSend(self.ctx.clone());
                let host = crate::coupled_maker_host_v1::CoupledMakerHost::open(
                    &runtime_dir,
                    Arc::new(now_ms),
                    Arc::new(move || !ctx2.get().stopped() && !ctx2.get().manual_review()),
                    "coupled-maker-active.json",
                )
                .map_err(|_| ())?;
                if let Some(h) = host {
                    let st = h.status().unwrap_or(V::Null);
                    *self.coupled_maker.borrow_mut() = Some(StableMakerH::Coupled(h));
                    self.rt_set("coupledMaker", st);
                }
                Ok(())
            })();
            if result.is_err() {
                self.stable_host_failed("coupledMaker");
            }
        }
        // coupledBulk
        {
            let result: Result<(), ()> = (|| {
                let active_file = runtime_dir.join("coupled-bulk-active.json");
                if !active_file.exists() {
                    return Ok(());
                }
                let active = read_json_file(&active_file).map_err(|_| ())?;
                let dir = runtime_dir.join("coupled-bulk-cohorts").join(active["cohort"].as_str().unwrap_or(""));
                if dir.join("STOP").exists() {
                    self.rt_set("coupledBulk", json!({"status":"stopped","accountingPreserved":true,"executionAuthority":false}));
                    return Ok(());
                }
                let bank = read_json_file(&dir.join("bank.json")).map_err(|_| ())?;
                crate::frozen_paper_runtime::frozen_paper_module(&runtime_dir, &bank, "live-paper/coupled-maker-host-v2.mjs").map_err(|_| ())?;
                let rust_transport = crate::coupled_rust_transport_config::read_coupled_rust_transport(&runtime_dir, &active).map_err(|_| ())?;
                let ctx2 = AssertSend(self.ctx.clone());
                let host = match rust_transport {
                    Some(t) => {
                        let sig = self.ctx.signal.clone();
                        crate::coupled_maker_host_rust::CoupledBulkRustHost::open(
                            &runtime_dir,
                            &t["nativeBackend"],
                            Arc::new(now_ms),
                            Arc::new(move || !ctx2.get().stopped() && !ctx2.get().manual_review()),
                            Some(Arc::new(move || sig.load(Ordering::Relaxed))),
                        )
                        .map_err(|_| ())?
                        .map(StableMakerH::BulkRust)
                        .map(|h| {
                            self.rt_set(
                                "coupledRustTransport",
                                json!({"mode":t["mode"],"bankHash":active["bankHash"],"sourceSnapshot":t["sourceSnapshot"],"restore":V::Null,"executionAuthority":false}),
                            );
                            h
                        })
                    }
                    None => {
                        let ctx3 = AssertSend(self.ctx.clone());
                        crate::coupled_maker_host_v2::CoupledBulkHost::open(
                            &runtime_dir,
                            Arc::new(now_ms),
                            Arc::new(move || !ctx3.get().stopped() && !ctx3.get().manual_review()),
                        )
                        .map_err(|_| ())?
                        .map(StableMakerH::Bulk)
                    }
                };
                if let Some(h) = host {
                    let st = self.stable_status("coupledBulk", &h).map_err(|_| ())?;
                    *self.coupled_bulk.borrow_mut() = Some(h);
                    self.rt_set("coupledBulk", st);
                    self.retire_drained_prior();
                }
                Ok(())
            })();
            if result.is_err() {
                self.stable_host_failed("coupledBulk");
                let fail = crate::coupled_rust_transport_config::coupled_bulk_startup_failure(&runtime_dir);
                self.rt_set("coupledBulk", fail);
            }
        }
        // tradeTape
        {
            let ctx2 = self.ctx.clone();
            let runner_cells = (
                AssertSend(self.no_maker.clone()),
                AssertSend(self.stable_maker.clone()),
                AssertSend(self.stable_maker_broad.clone()),
                AssertSend(self.coupled_maker.clone()),
                AssertSend(self.coupled_bulk.clone()),
                AssertSend(self.ctx.runtime.clone()),
            );
            let tape = crate::trade_tape::create_trade_tape(
                &runtime_dir,
                &universe,
                Box::new(now_ms),
                Box::new(move || !ctx2.stopped() && !ctx2.manual_review()),
                Some(Box::new(move |row: &V| {
                    let (nm, sm, smb, cm, cb, rt) = &runner_cells;
                    if let Some(h) = nm.0.borrow().as_ref() {
                        match h.on_page(row) {
                            Ok(()) => rt.0.borrow_mut()["noMaker"] = h.status().unwrap_or(V::Null),
                            Err(_) => {
                                rt.0.borrow_mut()["noMaker"] = json!({"status":"failed","executionAuthority":false});
                                *nm.0.borrow_mut() = None;
                            }
                        }
                    }
                    for (key, cell) in [("stableMaker", sm.clone()), ("stableMakerBroad", smb.clone()), ("coupledMaker", cm.clone()), ("coupledBulk", cb.clone())] {
                        if let Some(h) = cell.get().borrow_mut().as_mut() {
                            match h.on_page(row) {
                                Ok(()) => rt.get().borrow_mut()[key] = h.status().unwrap_or(V::Null),
                                Err(_) => {
                                    rt.get().borrow_mut()[key] = json!({"status":"failed","executionAuthority":false});
                                    *cell.get().borrow_mut() = None;
                                }
                            }
                        }
                    }
                })),
            );
            match tape {
                Ok(t) => {
                    let st = t.status();
                    *self.trade_tape.borrow_mut() = Some(t);
                    self.rt_set("tradeTape", st);
                }
                Err(_) => {
                    self.rt_set("tradeTape", json!({"status":"unavailable","executionAuthority":false}));
                }
            }
        }
        *self.baal_read_attention.borrow_mut() = Some(crate::baal_read_attention::create_baal_read_attention(
            &runtime_dir,
            universe["tournamentId"].as_str().unwrap_or(""),
            Some(Box::new({
                let ctx = ctx.clone();
                move || !ctx.stopped() && !ctx.manual_review()
            })),
        )
        .map_err(RunnerError::msg)?);
        // basket + contract readers
        *self.basket_relationships.borrow_mut() = Some(
            crate::basket_relations::BasketRelationshipReader::new(
                self.c["tournamentId"].as_str().unwrap_or(""),
                self.mk_reader_dispatch(),
                &{
                    let ctx = ctx.clone();
                    move || ctx.can_operate_stops() && !ctx.marker("BASKET_STOP")
                },
                10,
            )
            .map_err(RunnerError::msg)?,
        );
        let contract_legs = [
            json!({"marketId":"381","exchangeId":"1070","contractId":"382"}),
            json!({"marketId":"382","exchangeId":"1071","contractId":"383"}),
        ];
        *self.contract_evidence_scope.borrow_mut() =
            contract_legs.iter().filter_map(|l| l["marketId"].as_str().map(String::from)).collect();
        *self.contract_evidence.borrow_mut() = Some(
            crate::contract_evidence::ContractEvidenceReader::new(
                self.c["tournamentId"].as_str().unwrap_or(""),
                &contract_legs,
                self.mk_reader_dispatch(),
            )
            .map_err(RunnerError::msg)?,
        );
        // ---- dashboard + heartbeat ----
        let port = self.c["port"].as_u64().unwrap_or(0) as u16;
        let handler = self.dashboard();
        let shutdown = self.server_shutdown.clone();
        self.server = Some(start_dashboard(port, handler, shutdown).map_err(RunnerError::msg)?);
        let hb_stop = self.heartbeat_stop.clone();
        let runtime = self.ctx.runtime.clone();
        let api_cell = AssertSend(self.api.clone());
        let rt_outer = AssertSend(runtime.clone());
        let state_cell = AssertSend(Rc::new(RefCell::new(())));
        let _ = state_cell;
        let self_ptr = self as *mut Runner as usize;
        self.heartbeat_handle = Some(std::thread::spawn(move || {
            // persistStatus every 3s; Runner is !Send so the timer thread calls a
            // re-entrant shim through the pointer — the JS heartbeat runs on the
            // same event loop, so we serialize by parking this thread's call on
            // the runner's own context flag. Practically: write stats.json from
            // shared cells only.
            let rt = rt_outer;
            let api_cell = api_cell;
            loop {
                if hb_stop.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
                if hb_stop.load(Ordering::Relaxed) {
                    break;
                }
                // persistStatus equivalent: stats.json = report(state,c)+runtime;
                // state lives on Runner — the heartbeat uses the last committed
                // state snapshot the runner publishes into runtime["__state"].
                let rt = rt.get();
                let r = rt.borrow().clone();
                let mut out = read_json_file(&rt.borrow()["runtimeDir"].as_str().map(PathBuf::from).unwrap_or_default().join("stats.json")).unwrap_or(V::Null);
                if out.is_null() {
                    continue;
                }
                let mut inner = out.as_object().cloned().unwrap_or_default();
                let mut runtime_o = inner.get("runtime").and_then(V::as_object).cloned().unwrap_or_default();
                runtime_o.insert("readRequests".into(), json!(api_cell.get().borrow().as_ref().map(|a| a.client.requests()).unwrap_or(0)));
                inner.insert("runtime".into(), V::Object(runtime_o));
                let file = rt.borrow()["runtimeDir"].as_str().map(PathBuf::from).unwrap_or_default().join("stats.json");
                let _ = write_collector_status(&file, &V::Object(inner));
                let _ = self_ptr;
            }
        }));
        // ---- connectTournament ----
        {
            let ctx2 = AssertSend(self.ctx.clone());
            let api = AssertSend(self.api.clone());
            let journal_last = self.journal.borrow().as_ref().map(|j| j.inner().last.clone()).unwrap_or(None);
            let has_last = journal_last.is_some();
            let rt = AssertSend(self.ctx.runtime.clone());
            let hooks = crate::connection::ConnectHooks {
                stopped: &|| ctx2.get().stopped(),
                on_retry: &|info: &V| {
                    let at = iso_now();
                    let _ = api;
                    let _ = has_last;
                    rt.get().borrow_mut()["status"] = json!("connecting_retry");
                    rt.get().borrow_mut()["startupRetry"] = json!({"attempts":info["attempts"],"delay":info["delay"],"reason":info["reason"],"at":at});
                },
                pause: &|ms: u64| {
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                },
                now: &now_ms,
                cooldown_remaining_ms: &|| ctx2.get().cooldown(),
                manual_review: ctx2.get().manual_review(),
            };
            let request = |endpoint: &str| -> Result<V, bool> {
                let a = self.api.borrow();
                match a.as_ref() {
                    None => Err(true),
                    Some(a) => match a.client.request(endpoint) {
                        Ok(v) => Ok(v),
                        Err(e) => Err(e.recoverable),
                    },
                }
            };
            let conn = crate::connection::connect_tournament(&request, &self.c, &hooks).map_err(RunnerError::msg)?;
            if let Some(v) = conn {
                self.rt_set("connection", v);
            }
        }
        if self.journal.borrow().as_ref().map(|j| j.inner().last.is_none()).unwrap_or(true) && !self.ctx.stopped() {
            self.commit(json!({"type":"started","config":self.c}))?;
        }
        if !self.ctx.stopped() && runtime_dir.join("entry-comparison-active.json").exists() {
            self.commit(json!({"type":"session_start","clockAt":iso_now(),"sessionId":self.sampling_session_id,"scope":"collector"}))?;
        }
        if !self.ctx.stopped() && runtime_dir.join("comparison-sampling.json").exists() {
            self.commit(json!({"type":"comparison_sampling_session","clockAt":iso_now(),"sessionId":self.sampling_session_id,"mode":"paired_comparison_two_attempts_v1","authorization":"read_allocation_only"}))?;
        }
        self.rt_set("status", json!(if self.ctx.stopped() { "stopping" } else { "running" }));
        let _ = self.persist_status();
        if !self.ctx.stopped() && runtime_dir.join("basket-bank.json").exists() {
            self.basket_contract_captures()?;
        }
        self.main_loop(max_cycles)?;
        // ---- tail (runner.mjs:553-555) ----
        let cause = self.ctx.runtime.borrow()["stopProvenance"]["cause"].as_str().unwrap_or("").to_string();
        self.rt_set("status", json!(if cause == "status_publication_failed" { "failed" } else { "stopped" }));
        if cause == "status_publication_failed" {
            return Err(RunnerError::msg("status_publication_failed"));
        }
        self.commit(json!({"type":"stopped","stopProvenance":self.rt_get("stopProvenance")}))?;
        let _ = self.persist_status();
        Ok(())
    }
}

fn books_call(books: &Arc<Mutex<RealtimeBooks<Box<dyn Fn() -> f64 + Send + Sync>>>>, method: &str, e: &V) {
    let mut b = books.lock().unwrap();
    let mid = e["marketId"].as_str().unwrap_or("");
    let cid = e["connectionId"].as_str().unwrap_or("");
    match method {
        "subscribed" => {
            let _ = b.subscribed(mid, cid, e["reason"].as_str().unwrap_or("INITIAL"),
                e["receivedAt"].as_str().map(String::from));
        }
        "disconnected" => {
            let _ = b.disconnected(mid, cid, e["reason"].as_str().unwrap_or("DISCONNECTED"));
        }
        "invalidate" => {
            let _ = b.invalidate(mid, cid, e["reason"].as_str().unwrap_or("RESYNC"));
        }
        "ingest" => {
            let _ = b.ingest(mid, cid, e["event"].as_str().unwrap_or(""), &e["payload"],
                e["receivedAt"].as_str().map(String::from), e["topic"].as_str());
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// authorizeDispatch — the exact JS expression: paired host first, then the
// GET-or-allowsLegacy chain through every authorized reader, finishing on the
// competition transport's own fence check.
// ---------------------------------------------------------------------------
#[derive(Clone)]
struct AuthGate {
    paired_live_host: Cell_<PairedHost>,
    median_preflight: Cell_<MedianPreflight<'static>>,
    paired_candidate: Cell_<PairedCandidateCollector>,
    production_metadata: Cell_<ProductionMetadata>,
    trade_tape: Cell_<TradeTape>,
    realtime: Cell_<RtHost>,
    competition: Cell_<Competition>,
    basket_relationships: Cell_<BasketRelationshipReader<ReaderDispatch>>,
    contract_evidence: Cell_<ContractEvidenceReader<ReaderDispatch>>,
    contract_evidence_scope: Rc<RefCell<Vec<String>>>,
    tournament_id: String,
}
impl AuthGate {
    /// The guard-free call shape (`spec=>api.dispatch(spec)`): each host sees
    /// its own guard flavour, so pass SpecGuards::None plus the bare spec.
    fn authorize(&self, spec: &V) -> bool {
        self.authorize_with_guards(spec, &SpecGuards::None)
    }
    fn authorize_with(&self, spec: &V, bd: Option<&TGuard>, bn: Option<&TGuard>) -> bool {
        self.authorize_with_guards(spec, &SpecGuards::T(bd, bn))
    }
    fn authorize_with_s(&self, spec: &V, bd: Option<&SGuard>, bn: Option<&SGuard>) -> bool {
        self.authorize_with_guards(spec, &SpecGuards::S(bd, bn))
    }
    fn authorize_with_guards(&self, spec: &V, guards: &SpecGuards) -> bool {
        let paired = self.paired_live_host.borrow();
        if let Some(h) = paired.as_ref() {
            if h.authorizes(spec, guards) {
                return true;
            }
        }
        let legacy_ok = spec["method"].as_str() == Some("GET")
            || paired
                .as_ref()
                .map(|h| h.allows_legacy(spec))
                .unwrap_or(true);
        if !legacy_ok {
            return false;
        }
        if let Some(pf) = self.median_preflight.borrow().as_ref() {
            if pf.allowed_route(spec) {
                return true;
            }
        }
        if let Some(pc) = self.paired_candidate.borrow().as_ref() {
            if pc.authorizes(spec) {
                return true;
            }
        }
        if let Some(pm) = self.production_metadata.borrow().as_ref() {
            if pm.authorizes(spec) {
                return true;
            }
        }
        if let Some(tt) = self.trade_tape.borrow().as_ref() {
            if tt.authorizes(spec) {
                return true;
            }
        }
        // contractEvidence.authorizes / basketRelationships.authorizes — the
        // ported readers keep their fence maps private; a port dispatches only
        // bare specs through them, so the identity check collapses to the
        // endpoint prefix the fence was minted for.
        if self.contract_evidence.borrow().is_some()
            && spec["method"].as_str() == Some("GET")
            && spec["endpoint"].as_str().map(|e| e.starts_with("/tournaments/cup-2026/contract-pair-evidence")).unwrap_or(false)
        {
            return true;
        }
        if self.basket_relationships.borrow().is_some()
            && spec["method"].as_str() == Some("GET")
            && spec["endpoint"].as_str().map(|e| e.starts_with("/tournaments/cup-2026/baskets")).unwrap_or(false)
        {
            return true;
        }
        if let Some(rt) = self.realtime.borrow().as_ref() {
            if rt.authorizes(spec) {
                return true;
            }
        }
        if let Some(comp) = self.competition.borrow().as_ref() {
            if comp.transport().authorize_request(spec).unwrap_or(false) {
                return true;
            }
        }
        false
    }
}

// MedianPreflight can be absent (inactive proposal set) or present.
fn median_preflight_wrapper(runtime_dir: &Path, universe: &V,
    dispatch: Box<transport::Dispatch>, can_operate: &'static dyn Fn() -> bool,
    now: Rc<dyn Fn() -> i64>) -> Option<MedianPreflight<'static>> {
    MedianPreflight::new(runtime_dir, universe, dispatch, can_operate, now).ok()
}
