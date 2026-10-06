// live-paper/party-winner-pair-collector-v1.mjs — schedules existing
// paired-candidate reads from current published bulk data. No client, secret,
// timer, order or additional ordinary book slot is created.
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use indexmap::IndexMap;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use crate::market_price_publication::validate_price_observation_pub;
use crate::party_winner_no_screen_v1::{party_winner_quote_hash, screen_party_winner_no_pairs};
use crate::party_winner_pair_selector_v1::{create_party_pair_selector_state, select_party_winner_pair_capture};

// Thrown-object stand-in: persist errors carry durableRecordingFailed+fatal.
pub struct PairFault {
    pub message: String,
    pub durable_recording_failed: bool,
    pub fatal: bool,
}
impl PairFault {
    fn code(c: &str) -> Self { PairFault { message: c.into(), durable_recording_failed: false, fatal: false } }
    fn recording(m: impl Into<String>) -> Self { PairFault { message: m.into(), durable_recording_failed: true, fatal: true } }
    fn raw(m: impl Into<String>) -> Self { PairFault { message: m.into(), durable_recording_failed: false, fatal: false } }
}
impl From<String> for PairFault {
    fn from(s: String) -> Self { PairFault::raw(s) }
}
type R<T> = Result<T, PairFault>;
// need=(ok,code)=>{if(!ok)throw Error(code);} — the code IS the message.
fn need(ok: bool, code: &str) -> R<()> { if !ok { return Err(PairFault::code(code)); } Ok(()) }
fn raw(m: impl Into<String>) -> PairFault { PairFault::raw(m) }

fn payload_hash(v: &V) -> R<String> {
    baal_service_runtime::ledger::payload_hash(v).map_err(|e| raw(e.message))
}
fn js_eq(a: &V, b: &V) -> bool {
    if let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) { return x == y; }
    a == b
}
fn terminal(phase: &V) -> bool {
    ["complete", "failed", "interrupted"].contains(&phase.as_str().unwrap_or(""))
}
// new Date(ms).toISOString() — out-of-range ms throws 'Invalid time value'.
fn iso_of(ms: f64) -> R<String> {
    if !ms.is_finite() || ms.abs() > 8.64e15 { return Err(raw("Invalid time value")); }
    Ok(ms_to_iso(ms))
}
fn read_json(file: &Path) -> R<V> {
    serde_json::from_slice(&std::fs::read(file).map_err(|e| raw(e.to_string()))?)
        .map_err(|e| raw(e.to_string()))
}
// `{...v}` own-enumerable spread into a fresh object.
fn spread_map(v: &V) -> serde_json::Map<String, V> {
    let mut out = serde_json::Map::new();
    match v {
        V::Object(m) => out.extend(m.iter().map(|(k, x)| (k.clone(), x.clone()))),
        V::Array(a) => out.extend(a.iter().enumerate().map(|(i, x)| (i.to_string(), x.clone()))),
        V::String(s) => out.extend(s.encode_utf16().enumerate()
            .map(|(i, c)| (i.to_string(), json!(String::from_utf16_lossy(&[c]))))),
        _ => {}
    }
    out
}

/// openPartyWinnerPairCollector({runtimeDir,universe,getAccount,getCapture,
/// getBoundPlanHash,canOperate,now})
pub struct PartyWinnerPairCollector {
    plan_file: PathBuf,
    catalog_file: PathBuf,
    state_file: PathBuf,
    request_file: PathBuf,
    status_file: PathBuf,
    universe: V,
    plan: V,
    plan_hash: String,
    catalog: V,
    catalog_hash: String,
    get_account: Arc<dyn Fn() -> V + Send + Sync>,
    get_capture: Arc<dyn Fn() -> V + Send + Sync>,
    get_bound_plan_hash: Arc<dyn Fn() -> V + Send + Sync>,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
    state: V,
    quotes: IndexMap<String, V>,
    sequence: Option<i64>,
    previous: Option<String>,
    last_received_at: Option<f64>,
    last_known_at: Option<f64>,
    failed: bool,
    reason: String,
}
impl PartyWinnerPairCollector {
    pub fn open(runtime_dir: &Path, universe: &V,
        get_account: Arc<dyn Fn() -> V + Send + Sync>,
        get_capture: Arc<dyn Fn() -> V + Send + Sync>,
        get_bound_plan_hash: Arc<dyn Fn() -> V + Send + Sync>,
        can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
        now: Arc<dyn Fn() -> f64 + Send + Sync>) -> R<Option<Self>> {
        let plan_file = runtime_dir.join("party-pair-source-plan.json");
        if !plan_file.exists() { return Ok(None); }
        let plan = read_json(&plan_file)?;
        let plan_hash = payload_hash(&plan)?;
        let catalog_file = runtime_dir.join("party-pair-root-catalog.json");
        let catalog = read_json(&catalog_file)?;
        need(js_eq(&plan["universeHash"], &universe["hash"])
            && js_eq(&catalog["universeHash"], &universe["hash"])
            && catalog["rootCaptures"].is_array()
            && payload_hash(&catalog["rootCaptures"])?
                == plan["rootCatalogHash"].as_str().unwrap_or(""),
            "PARTY_PAIR_CATALOG")?;
        let catalog_hash = payload_hash(&catalog)?;
        let state_file = runtime_dir.join("party-pair-selector-state.json");
        let request_file = runtime_dir.join("paired-candidate-request.json");
        let status_file = runtime_dir.join("party-pair-selector-status.json");
        let state = if state_file.exists() {
            read_json(&state_file)?
        } else {
            create_party_pair_selector_state(&universe["hash"])?
        };
        Ok(Some(PartyWinnerPairCollector {
            plan_file, catalog_file, state_file, request_file, status_file,
            universe: universe.clone(), plan, plan_hash, catalog, catalog_hash,
            get_account, get_capture, get_bound_plan_hash, can_operate, now,
            state, quotes: IndexMap::new(), sequence: None, previous: None,
            last_received_at: None, last_known_at: None,
            failed: false, reason: "awaiting_bulk_publication".into(),
        }))
    }
    // fixed(): the plan and catalog are re-read and re-hashed on every call;
    // read/parse/hash failures propagate exactly as in JS.
    fn fixed(&self) -> R<bool> {
        if self.failed || !(self.can_operate)() { return Ok(false); }
        Ok(payload_hash(&read_json(&self.plan_file)?)? == self.plan_hash
            && payload_hash(&read_json(&self.catalog_file)?)? == self.catalog_hash)
    }
    fn persist(&mut self, file: &Path, value: &V) -> R<()> {
        match atomic_json_canonical(file, value) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.failed = true;
                self.reason = "durability_failure".into();
                Err(PairFault::recording(e))
            }
        }
    }
    // schedule() — also exported as afterAccountRefresh.
    fn schedule(&mut self) -> R<()> {
        if !self.fixed()? { return Ok(()); }
        let account = (self.get_account)();
        if !js_truthy(Some(&account)) { return Ok(()); }
        let capture = (self.get_capture)();
        let request: Option<V> = if self.request_file.exists() {
            Some(read_json(&self.request_file)?)
        } else { None };
        // account.snapshot.X — a missing/null snapshot is a TypeError in JS.
        match account.get("snapshot") {
            None => return Err(raw("Cannot read properties of undefined (reading 'positionsAt')")),
            Some(V::Null) => return Err(raw("Cannot read properties of null (reading 'positionsAt')")),
            _ => {}
        }
        let account_view = json!({
            "complete":account["complete"] == V::Bool(true),
            "tournamentId":self.universe["tournamentId"],
            "availableAt":account["availableAt"],
            "positionsAt":account["snapshot"]["positionsAt"],
            "ordersAt":account["snapshot"]["ordersAt"],
            "excludedExchangeIds":account["snapshot"]["excludedExchangeIds"],
            "unknownAcceptance":account["unknownAcceptance"] == V::Bool(true),
            "sourceHash":payload_hash(&account)?});
        let at = iso_of((self.now)())?;
        let screen = screen_party_winner_no_pairs(&json!({
            "universe":self.universe,"rootCaptures":self.catalog["rootCaptures"],
            "quoteCaptures":self.quotes.values().cloned().collect::<Vec<V>>(),
            "account":account_view,"now":at}))?;
        let capture_ok = js_truthy(Some(&capture)) && terminal(&capture["phase"]);
        // capture.legs?.map(l=>l.marketId)||request?.marketIds
        let terminal_records: Vec<V> = if capture_ok {
            let market_ids = match capture.get("legs") {
                None | Some(V::Null) => request.as_ref()
                    .and_then(|r| r.get("marketIds")).cloned().unwrap_or(V::Null),
                Some(V::Array(a)) => json!(a.iter()
                    .map(|l| l["marketId"].clone()).collect::<Vec<V>>()),
                Some(_) => return Err(raw("capture.legs?.map is not a function")),
            };
            vec![json!({"sourceId":capture["name"],"name":capture["name"],
                "marketIds":market_ids,"phase":capture["phase"],
                "updatedAt":capture["at"],
                "positiveConditionalScreen":
                    capture["screen"]["positiveConditionalScreen"] == V::Bool(true)})]
        } else { Vec::new() };
        let result = select_party_winner_pair_capture(&json!({
            "universe":self.universe,"screen":screen,"account":account_view,
            "state":self.state,"sourcePlan":self.plan,"currentRequest":request,
            "capture":capture,"terminalRecords":terminal_records,
            "boundPlanHash":(self.get_bound_plan_hash)(),
            "collectorCanRead":true,"now":at}))?;
        // Record the admission proposal before installing the existing
        // collector's request. A crash never turns the same source into a
        // duplicate request.
        self.persist(&self.state_file.clone(), &result["nextState"])?;
        self.state = result["nextState"].clone();
        self.reason = result["reason"].as_str().unwrap_or("").to_string();
        if js_truthy(result.get("request")) {
            self.persist(&self.request_file.clone(), &result["request"])?;
        }
        self.persist(&self.status_file.clone(), &json!({
            "schemaVersion":1,"observedAt":at,"reason":self.reason,
            "selected":result["selected"],"request":result["request"],
            "sourcePlanHash":self.plan_hash,"priceSequence":self.sequence,
            "priceHash":self.previous,"rootPairs":screen["semanticPairCount"],
            "orders":0,"additionalClients":0,"ordinaryBookSlots":4,
            "auxiliaryBookSlots":2}))?;
        Ok(())
    }
    /// onPublished(row)
    pub fn on_published(&mut self, row: &V) -> R<()> {
        if !self.fixed()? { return Ok(()); }
        // {hash,...body}=row
        if row.is_null() {
            return Err(raw("Cannot destructure property 'hash' of 'null' as it is null."));
        }
        let mut body = row.clone();
        let hash_v = body.as_object_mut().and_then(|m| m.shift_remove("hash"));
        let hash = match &hash_v { Some(V::String(s)) => s.clone(), _ => String::new() };
        need(hash == sha256(js_stringify(&body).as_bytes())
            && js_int(&row["sequence"]).is_some_and(|s| s > 0)
            && (row["previous"].as_str() == Some("")
                || row["previous"].as_str().is_some_and(|p| p.len() == 64
                    && p.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))))
            && row["event"]["type"].as_str() == Some("market_price_observation")
            && row.get("state").map(|v| v.is_null()).unwrap_or(false),
            "PARTY_PAIR_PRICE_SOURCE")?;
        if let Some(seq) = self.sequence {
            need(row["sequence"].as_i64() == Some(seq + 1)
                && row["previous"].as_str() == self.previous.as_deref(),
                "PARTY_PAIR_PRICE_CHAIN")?;
        }
        let snapshot = validate_price_observation_pub(&row["event"]["snapshot"], &self.universe)?;
        let known = row["at"].as_str()
            .and_then(baal_service_runtime::contract::parse_iso_ms).unwrap_or(f64::NAN);
        let received = snapshot["receivedAt"].as_str()
            .and_then(baal_service_runtime::contract::parse_iso_ms).unwrap_or(f64::NAN);
        need(js_int(&json!(known)).is_some()
            && ms_to_iso(known) == row["at"].as_str().unwrap_or("")
            && received <= known
            && known <= (self.now)()
            && self.last_received_at.map(|l| received >= l).unwrap_or(true)
            && self.last_known_at.map(|l| known >= l).unwrap_or(true),
            "PARTY_PAIR_PRICE_CLOCK")?;
        // for(const id of snapshot.missingIds): non-array throws in JS.
        let missing_ids = match snapshot["missingIds"].as_array() {
            Some(a) => a.clone(),
            None => return Err(raw("snapshot.missingIds is not iterable")),
        };
        for id in &missing_ids {
            // Map keys hold the raw value — js_stringify keeps distinct JSON
            // values as distinct keys (5 vs "5" stay separate).
            self.quotes.shift_remove(&js_stringify(id));
        }
        let quote_list = match snapshot["quotes"].as_array() {
            Some(a) => a.clone(),
            None => return Err(raw("snapshot.quotes is not iterable")),
        };
        for q in &quote_list {
            let mut value = spread_map(q);
            value.insert("tournamentId".into(), self.universe["tournamentId"].clone());
            value.insert("requestStartedAt".into(), snapshot["requestStartedAt"].clone());
            value.insert("receivedAt".into(), snapshot["receivedAt"].clone());
            value.insert("knownAt".into(), row["at"].clone());
            value.insert("sourceHash".into(), json!(hash));
            let mut value = V::Object(value);
            value["quoteHash"] = json!(party_winner_quote_hash(&value)?);
            self.quotes.insert(js_stringify(&q["exchangeId"]), value);
        }
        self.sequence = js_int(&row["sequence"]);
        self.previous = Some(hash);
        self.last_received_at = Some(received);
        self.last_known_at = Some(known);
        self.schedule()
    }
    /// afterAccountRefresh — schedule() under its other name.
    pub fn after_account_refresh(&mut self) -> R<()> { self.schedule() }
    /// status()
    pub fn status(&self) -> V {
        json!({"reason":self.reason,"priceSequence":self.sequence,
            "knownQuotes":self.quotes.len(),"failed":self.failed,
            "additionalClients":0,"orders":0})
    }
}
