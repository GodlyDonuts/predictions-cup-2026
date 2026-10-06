// execution/paired-live-host-v2.mjs — one selection, then the unchanged v1
// actor/bridge/funding lifecycle. No new client, account lease, limiter,
// timer, dynamic pair replacement or renewal.
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::ledger::{acquire_owner_lease, OwnerLease};
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const TID: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
const THREAD: &str = "01a0f8cc-b9bb-77d2-9f4c-2844396f1b79";
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("PAIRED_V2_{code}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V { v.get(k).unwrap_or(&V::Null) }
fn iso(n: i64) -> String { ms_to_iso(n as f64) }
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn id(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        s.bytes().all(|b| b.is_ascii_digit()) && !s.starts_with('0')
            && s.parse::<i64>().map(|n| n >= 1 && n <= i64::MAX).unwrap_or(false)
    })
}
fn clock(v: &V) -> Result<i64, String> {
    let n = parse_iso_ms(v.as_str().unwrap_or("")).ok_or_else(|| "PAIRED_V2_CLOCK".to_string())?;
    need(ms_to_iso(n as f64) == v.as_str().unwrap_or(""), "CLOCK")?;
    Ok(n as i64)
}
fn read(p: &Path) -> Result<V, String> {
    serde_json::from_str(&std::fs::read_to_string(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn write(file: &Path, value: &V) -> Result<(), String> { atomic_json(file, value) }
fn copy(v: &V) -> V { v.clone() }
pub fn paired_live_v2_identity(workspace: &Path) -> V {
    json!(baal_collector::frozen_paper_graph::collect_frozen_paper_files(
        workspace,
        &["execution/paired-live-host-v2.mjs".to_string(), "execution/scoped-rest-receipts.mjs".to_string()],
    ).unwrap_or_default())
}
fn validate_request(r: &V, u: &V) -> Result<(), String> {
    need(
        get(r, "schemaVersion") == &json!(2)
            && get(r, "mode") == &json!("one_pair_from_allowed_markets_v2")
            && get(r, "enabled") == &json!(true)
            && get(r, "ownerId") == &json!("Codex")
            && get(r, "tournamentId") == &json!(TID)
            && get(r, "tournamentSlug") == &json!("midterm-elections")
            && get(r, "runId").as_str().is_some_and(|s| {
                s.starts_with("sig-pilot-paired-")
                    && s.len() > "sig-pilot-paired-".len() && s.len() <= "sig-pilot-paired-".len() + 60
                    && s.bytes().skip("sig-pilot-paired-".len()).all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            }),
        "REQUEST",
    )?;
    need(
        get(u, "tournamentId") == &json!(TID)
            && hex(get(u, "hash"))
            && get(r, "universeHash") == get(u, "hash")
            && get(u, "exchanges").is_array(),
        "UNIVERSE",
    )?;
    need(
        get(r, "allowedMarkets").as_array().map(|ms| {
            ms.len() >= 2 && ms.len() <= 300
                && ms.iter().map(|m| m.get("marketId").map(|v| v.to_string()).unwrap_or_default()).collect::<std::collections::BTreeSet<_>>().len() == ms.len()
                && ms.iter().map(|m| m.get("exchangeId").map(|v| v.to_string()).unwrap_or_default()).collect::<std::collections::BTreeSet<_>>().len() == ms.len()
                && ms.iter().all(|m| {
                    let mut ks: Vec<&String> = m.as_object().map(|o| o.keys().collect()).unwrap_or_default();
                    ks.sort();
                    ks.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("|") == "exchangeId|marketId"
                        && id(get(m, "marketId")) && id(get(m, "exchangeId"))
                        && get(u, "exchanges").as_array().map(|xs| xs.iter().any(|x| {
                            get(x, "marketId") == get(m, "marketId")
                                && get(x, "exchangeId") == get(m, "exchangeId")
                                && get(x, "option") == &json!("YES")
                        })).unwrap_or(false)
                })
        }).unwrap_or(false),
        "ALLOWED_MARKETS",
    )?;
    need(
        clock(get(r, "entryExpiresAt"))? > clock(get(r, "startedAt"))?
            && clock(get(r, "entryExpiresAt"))? - clock(get(r, "startedAt"))? <= 3_600_000
            && hex(get(r, "authorityHash")),
        "WINDOW_OR_AUTHORITY",
    )?;
    Ok(())
}
/// A candidate is a read-only observation. Neither its positive display flag
/// nor metadata pairing alone authorizes it.
pub fn select_paired_live_candidate(request: &V, universe: &V, capture: &V, now: i64) -> Result<V, String> {
    validate_request(request, universe)?;
    if now < clock(get(request, "startedAt"))? || now >= clock(get(request, "entryExpiresAt"))? {
        return Ok(json!({"eligible": false, "reason": "entry_window_closed"}));
    }
    if get(capture, "phase") != &json!("complete") {
        return Ok(json!({"eligible": false, "reason": "awaiting_complete_capture"}));
    }
    if get(get(capture, "screen"), "positiveConditionalScreen") != &json!(true) {
        return Ok(json!({"eligible": false, "reason": "no_qualified_pair"}));
    }
    if get(capture, "legs").as_array().map(|ls| {
        ls.len() != 2 || ls.iter().any(|l| {
            !get(request, "allowedMarkets").as_array().map(|ms| ms.iter().any(|m| {
                get(m, "marketId") == get(l, "marketId") && get(m, "exchangeId") == get(l, "exchangeId")
            })).unwrap_or(false)
        })
    }).unwrap_or(true) {
        return Ok(json!({"eligible": false, "reason": "outside_allowed_markets"}));
    }
    let selection = (|| -> Result<V, String> {
        let markets: Vec<V> = get(capture, "legs").as_array().unwrap().iter()
            .map(|l| json!({"marketId": get(l, "marketId"), "exchangeId": get(l, "exchangeId")})).collect();
        let scope = json!({
            "ownerId": "Codex",
            "runId": get(request, "runId"),
            "tournamentId": TID,
            "tournamentSlug": get(request, "tournamentSlug"),
            "markets": markets,
        });
        let selected_at = iso(now);
        let plan = crate::paired_rest_controller::derive_paired_rest_plan(&json!({
            "capture": capture, "scope": scope, "readAt": selected_at,
        }))?;
        Ok(json!({
            "schemaVersion": 1,
            "requestHash": payload_hash(request)?,
            "authorityHash": get(request, "authorityHash"),
            "selectedAt": selected_at,
            "scope": scope,
            "capture": copy(capture),
            "captureHash": payload_hash(capture)?,
            "qualificationPlanHash": get(&plan, "planHash"),
            "reviewExpiresAt": get(&plan, "reviewExpiresAt"),
            "originalProtectiveCost": get(&plan, "originalProtectiveCost"),
            "pairCostCeiling": get(&plan, "pairCostCeiling"),
            "minimumOrdinaryMargin": if get(&plan, "minimumOrdinaryMargin").is_null() {
                get(&plan, "sourceMinimumOrdinaryMargin").clone()
            } else { get(&plan, "minimumOrdinaryMargin").clone() },
        }))
    })();
    match selection {
        Ok(sel) => {
            if clock(get(&sel, "reviewExpiresAt"))? > clock(get(request, "entryExpiresAt"))? {
                return Ok(json!({"eligible": false, "reason": "review_outside_entry_window"}));
            }
            Ok(json!({"eligible": true, "selection": sel}))
        }
        Err(_) => Ok(json!({"eligible": false, "reason": "awaiting_fresh_qualified_capture"})),
    }
}

type GetV = Rc<dyn Fn() -> Result<V, String>>;

pub struct PairedLiveHostV2 {
    runtime_dir: PathBuf,
    workspace: PathBuf,
    universe: V,
    api: Rc<dyn crate::paired_live_host::HostApi>,
    receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    get_account_lease: GetV,
    get_other_state: GetV,
    can_operate: Rc<dyn Fn() -> bool>,
    has_read_headroom: Rc<dyn Fn(usize) -> bool>,
    now: Rc<dyn Fn() -> i64>,
    file: PathBuf,
    legacy_request_file: PathBuf,
    authority_file: PathBuf,
    request: V,
    request_hash: String,
    authority: V,
    code: V,
    dir: PathBuf,
    child_dir: PathBuf,
    selection_file: PathBuf,
    lease: RefCell<Option<OwnerLease>>,
    selection: RefCell<Option<V>>,
    delegate: RefCell<Option<Rc<crate::paired_live_host::PairedLiveHost>>>,
    closed: RefCell<bool>,
    busy: RefCell<bool>,
    failed: RefCell<bool>,
    wait_reason: RefCell<V>,
    selection_bytes: RefCell<Vec<u8>>,
}

impl PairedLiveHostV2 {
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        runtime_dir: &Path,
        workspace: &Path,
        universe: &V,
        api: Rc<dyn crate::paired_live_host::HostApi>,
        receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
        get_account_lease: GetV,
        get_other_state: GetV,
        can_operate: Rc<dyn Fn() -> bool>,
        has_read_headroom: Rc<dyn Fn(usize) -> bool>,
        now: Rc<dyn Fn() -> i64>,
    ) -> Result<Option<Rc<Self>>, String> {
        let file = runtime_dir.join("paired-live-v2-request.json");
        if !file.exists() { return Ok(None); }
        let legacy_request_file = runtime_dir.join("paired-live-request.json");
        need(!legacy_request_file.exists(), "LEGACY_REQUEST_MUST_BE_RETIRED")?;
        need(runtime_dir.is_absolute() && workspace.is_absolute(), "DEPENDENCIES")?;
        let universe = copy(universe);
        let request = read(&file)?;
        validate_request(&request, &universe)?;
        let request = copy(&request);
        let request_hash = payload_hash(&request)?;
        let authority_file = runtime_dir.join("paired-live-authority.json");
        let authority = read(&authority_file)?;
        let code = paired_live_v2_identity(workspace);
        need(payload_hash(&code).ok() == payload_hash(get(&request, "codeIdentity")).ok(), "CODE")?;
        need(
            payload_hash(&authority).ok() == get(&request, "authorityHash").as_str().map(String::from)
                && get(&authority, "kind") == &json!("direct_human_authorized_bot_update")
                && get(&authority, "ownerId") == &json!("Codex")
                && get(&authority, "threadId") == &json!(THREAD)
                && get(&authority, "exploratory") == &json!(true)
                && get(&authority, "profitabilityEstablished") == &json!(false),
            "AUTHORITY",
        )?;
        let dir = runtime_dir.join("paired-live-v2-runs").join(get(&request, "runId").as_str().unwrap());
        let child_dir = dir.join("child-runtime");
        let selection_file = dir.join("selection.jsonl");
        let lease = acquire_owner_lease(&dir, "Codex", get(&request, "runId").as_str().unwrap(), "paired-live-selector-v2", (now)() as f64)
            .map_err(|e| format!("PAIRED_V2_{}", e.code))?;
        let this = Rc::new(Self {
            runtime_dir: runtime_dir.to_path_buf(),
            workspace: workspace.to_path_buf(),
            universe,
            api,
            receipt_of,
            get_account_lease,
            get_other_state,
            can_operate,
            has_read_headroom,
            now,
            file,
            legacy_request_file,
            authority_file,
            request,
            request_hash,
            authority,
            code,
            dir,
            child_dir,
            selection_file,
            lease: RefCell::new(Some(lease)),
            selection: RefCell::new(None),
            delegate: RefCell::new(None),
            closed: RefCell::new(false),
            busy: RefCell::new(false),
            failed: RefCell::new(false),
            wait_reason: RefCell::new(json!("awaiting_complete_capture")),
            selection_bytes: RefCell::new(Vec::new()),
        });
        let result = (|| -> Result<(), String> {
            if this.selection_file.exists() {
                let bytes = std::fs::read(&this.selection_file).map_err(|e| e.to_string())?;
                need(!bytes.is_empty() && bytes.len() <= 2 * 1024 * 1024 && bytes.last() == Some(&10), "SELECTION_JOURNAL")?;
                let journal = Journal::open(&this.selection_file)?;
                need(
                    journal.sequence == 1
                        && journal.last.as_ref().map(|l| {
                            get(get(l, "event"), "kind") == &json!("pair_selected")
                                && get(get(l, "event"), "requestHash").as_str() == Some(this.request_hash.as_str())
                        }).unwrap_or(false),
                    "SELECTION_JOURNAL",
                )?;
                let selection = copy(get(journal.last.as_ref().unwrap(), "state"));
                let qualified = select_paired_live_candidate(
                    &this.request, &this.universe, get(&selection, "capture"),
                    clock(get(&selection, "selectedAt"))?,
                )?;
                need(
                    get(&qualified, "eligible") == &json!(true)
                        && payload_hash(get(&qualified, "selection")).ok() == payload_hash(&selection).ok(),
                    "SELECTION_PROOF",
                )?;
                *this.selection.borrow_mut() = Some(selection);
                *this.selection_bytes.borrow_mut() = bytes;
                this.open_child()?;
            } else {
                need(!this.child_dir.exists(), "UNBOUND_CHILD")?;
            }
            this.publish()?;
            Ok(())
        })();
        if let Err(e) = result {
            let _ = this.cleanup();
            return Err(e);
        }
        Ok(Some(this))
    }
    fn fixed_proof(&self) -> Result<(), String> {
        self.lease.borrow().as_ref().unwrap().assert_ownership().map_err(|e| format!("PAIRED_V2_{}", e.code))?;
        need(!self.legacy_request_file.exists(), "LEGACY_REQUEST_MUST_BE_RETIRED")?;
        need(
            payload_hash(&read(&self.file)?).ok() == Some(self.request_hash.clone())
                && payload_hash(&read(&self.authority_file)?).ok() == payload_hash(&self.authority).ok()
                && payload_hash(&paired_live_v2_identity(&self.workspace)).ok() == payload_hash(&self.code).ok(),
            "SOURCE_IDENTITY",
        )?;
        if !self.selection_bytes.borrow().is_empty() {
            need(std::fs::read(&self.selection_file).map_err(|e| e.to_string())? == *self.selection_bytes.borrow(), "SELECTION_CHANGED")?;
        } else {
            need(!self.selection_file.exists(), "UNEXPECTED_SELECTION")?;
        }
        Ok(())
    }
    fn allowed(&self) -> bool {
        if *self.closed.borrow() || *self.failed.borrow() || !(self.can_operate)() { return false; }
        self.fixed_proof().is_ok()
    }
    fn child_actor(&self) -> PathBuf {
        self.child_dir.join("paired-live-runs").join(get(&self.request, "runId").as_str().unwrap())
            .join("controller").join("paired-rest-journal.jsonl")
    }
    fn child_request(&self) -> V {
        let selection = self.selection.borrow().clone().unwrap_or(V::Null);
        json!({
            "schemaVersion": 1,
            "enabled": true,
            "ownerId": "Codex",
            "runId": get(&self.request, "runId"),
            "tournamentId": TID,
            "tournamentSlug": get(&self.request, "tournamentSlug"),
            "markets": copy(get(get(&selection, "scope"), "markets")),
            "startedAt": get(&self.request, "startedAt"),
            "entryExpiresAt": get(&selection, "reviewExpiresAt"),
            "authorityHash": get(&self.request, "authorityHash"),
            "codeIdentity": crate::paired_live_host::paired_live_identity(&self.workspace),
        })
    }
    fn ensure_exact(&self, file: &Path, value: &V) -> Result<(), String> {
        if file.exists() {
            need(payload_hash(&read(file)?).ok() == payload_hash(value).ok(), "CHILD_IDENTITY")?;
        } else {
            write(file, value)?;
        }
        Ok(())
    }
    fn open_child(self: &Rc<Self>) -> Result<(), String> {
        need(self.selection.borrow().is_some(), "SELECTION_REQUIRED")?;
        std::fs::create_dir_all(&self.child_dir).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&self.child_dir, std::fs::Permissions::from_mode(0o700)).ok(); }
        self.ensure_exact(&self.child_dir.join("paired-live-authority.json"), &self.authority)?;
        self.ensure_exact(&self.child_dir.join("paired-live-request.json"), &self.child_request())?;
        let this = self.clone();
        let delegate = crate::paired_live_host::PairedLiveHost::open(
            &self.child_dir,
            &self.workspace,
            self.api.clone(),
            self.receipt_of.clone(),
            self.get_account_lease.clone(),
            self.get_other_state.clone(),
            Rc::new(move || this.allowed()),
            self.has_read_headroom.clone(),
            self.now.clone(),
        )?;
        need(delegate.is_some(), "CHILD_REQUIRED")?;
        *self.delegate.borrow_mut() = delegate;
        Ok(())
    }
    fn blocked(&self) -> bool {
        self.selection.borrow().is_some()
            && (self.delegate.borrow().as_ref().map(|d| !get(&d.status(), "initialized").as_bool().unwrap_or(false)).unwrap_or(true)
                || self.delegate.borrow().as_ref().map(|d| d.blocks_legacy_writes()).unwrap_or(false))
    }
    fn public_status(&self) -> V {
        let s = self.delegate.borrow().as_ref().map(|d| d.status());
        let selection = self.selection.borrow().clone().unwrap_or(V::Null);
        json!({
            "hostVersion": 2,
            "mode": "official_one_pair_rest",
            "runId": get(&self.request, "runId"),
            "requestHash": self.request_hash,
            "authorityHash": get(&self.request, "authorityHash"),
            "universeHash": get(&self.request, "universeHash"),
            "allowedMarketCount": get(&self.request, "allowedMarkets").as_array().map(|a| a.len()).unwrap_or(0),
            "selectedMarkets": copy(get(get(&selection, "scope"), "markets")).as_array().cloned().unwrap_or_default(),
            "selectionHash": if selection.is_null() { V::Null } else { json!(payload_hash(&selection).unwrap_or_default()) },
            "captureHash": get(&selection, "captureHash"),
            "entryExpiresAt": get(&self.request, "entryExpiresAt"),
            "reviewExpiresAt": get(&selection, "reviewExpiresAt"),
            "initialized": s.as_ref().map(|x| get(x, "initialized") == &json!(true)).unwrap_or(false),
            "phase": if s.as_ref().map(|x| get(x, "initialized") == &json!(true)).unwrap_or(false) {
                get(s.as_ref().unwrap(), "phase").clone()
            } else if !selection.is_null() {
                json!("selected_waiting_for_initialization")
            } else { self.wait_reason.borrow().clone() },
            "waitReason": self.wait_reason.borrow().clone(),
            "lastFailure": s.as_ref().map(|x| get(x, "lastFailure").clone()).unwrap_or(V::Null),
            "failed": *self.failed.borrow() || s.as_ref().map(|x| get(x, "failed") == &json!(true)).unwrap_or(false),
            "closed": *self.closed.borrow(),
            "orders": s.as_ref().map(|x| get(x, "orders").clone()).unwrap_or(json!(0)),
            "blocksLegacyWrites": self.blocked(),
            "additionalPersistentProcesses": 0,
            "additionalAccountClients": 0,
        })
    }
    fn publish(&self) -> Result<V, String> {
        let status = self.public_status();
        let child_stats_file = self.child_dir.join("paired-live-stats.json");
        let mut child = serde_json::Map::new();
        if self.delegate.borrow().is_some() && child_stats_file.exists() {
            let v = read(&child_stats_file)?;
            need(
                get(&v, "runId") == get(&self.request, "runId")
                    && get(&v, "authorityHash") == get(&self.request, "authorityHash"),
                "CHILD_STATS",
            )?;
            for key in ["action", "orders", "owned", "funding", "realizedPnl", "ordinaryMinimumPayout",
                "ordinarySurplus", "ordinaryResolutionOnly", "maximumExceptionalLoss", "profitabilityEstablished"] {
                if let Some(x) = v.get(key) { child.insert(key.to_string(), x.clone()); }
            }
        }
        let mut out = serde_json::Map::new();
        out.insert("schemaVersion".into(), json!(2));
        for (k, v) in child.iter() { out.insert(k.clone(), v.clone()); }
        for (k, v) in status.as_object().unwrap().iter() { out.insert(k.clone(), v.clone()); }
        out.insert("orders".into(), child.get("orders").cloned().unwrap_or(json!([])));
        out.insert("orderCount".into(), get(&status, "orders").clone());
        out.insert("observedAt".into(), json!(iso((self.now)())));
        write(&self.runtime_dir.join("paired-live-stats.json"), &V::Object(out))?;
        Ok(status)
    }
    fn cleanup(&self) -> Result<(), String> {
        let mut error: Option<String> = None;
        if let Some(d) = self.delegate.borrow_mut().take() { d.close(); }
        if let Some(l) = self.lease.borrow_mut().as_mut() {
            if let Err(e) = l.release() { if error.is_none() { error = Some(e.to_string()); } }
        }
        match error { Some(e) => Err(e), None => Ok(()) }
    }
    pub fn authorizes(&self, spec: &V) -> bool {
        self.allowed() && self.delegate.borrow().as_ref().map(|d| d.authorizes(spec)).unwrap_or(false)
    }
    pub fn on_fetch_start(&self, event: &V) -> bool {
        self.delegate.borrow().as_ref().map(|d| d.on_fetch_start(event)).unwrap_or(false)
    }
    pub fn blocks_legacy_writes(&self) -> bool { self.blocked() }
    pub fn allows_legacy(&self, spec: &V) -> bool {
        if self.selection.borrow().is_none() { return true; }
        self.delegate.borrow().as_ref().map(|d| {
            if get(&d.status(), "initialized") == &json!(true) { d.allows_legacy(spec) } else { get(spec, "method") == &json!("GET") }
        }).unwrap_or_else(|| get(spec, "method") == &json!("GET"))
    }
    pub fn step(self: &Rc<Self>, capture: Option<&V>) -> Result<V, String> {
        need(!*self.busy.borrow() && !*self.closed.borrow(), "BUSY_OR_CLOSED")?;
        *self.busy.borrow_mut() = true;
        let result = (|| -> Result<V, String> {
            if !self.allowed() {
                *self.wait_reason.borrow_mut() = json!("operational_stop_or_identity_changed");
                return self.publish();
            }
            if self.selection.borrow().is_none() {
                let result = select_paired_live_candidate(&self.request, &self.universe, capture.unwrap_or(&V::Null), (self.now)())?;
                *self.wait_reason.borrow_mut() = if get(&result, "eligible") == &json!(true) { V::Null } else { get(&result, "reason").clone() };
                if get(&result, "eligible") != &json!(true) { return self.publish(); }
                self.fixed_proof()?;
                let mut journal = Journal::open(&self.selection_file)?;
                journal.commit(get(&result, "selection"), &json!({"kind": "pair_selected", "requestHash": self.request_hash}))?;
                *self.selection.borrow_mut() = Some(copy(get(&result, "selection")));
                *self.selection_bytes.borrow_mut() = std::fs::read(&self.selection_file).map_err(|e| e.to_string())?;
                self.open_child()?;
            }
            if self.delegate.borrow().is_none() { self.open_child()?; }
            if !self.child_actor().exists() {
                let selection = self.selection.borrow().clone().unwrap();
                let fresh = select_paired_live_candidate(&self.request, &self.universe, get(&selection, "capture"), (self.now)())?;
                if get(&fresh, "eligible") != &json!(true) {
                    *self.wait_reason.borrow_mut() = json!("selected_source_expired_unexecuted");
                    return self.publish();
                }
            }
            *self.wait_reason.borrow_mut() = V::Null;
            let selection = self.selection.borrow().clone().unwrap();
            self.delegate.borrow().as_ref().unwrap().clone().step(Some(get(&selection, "capture")))?;
            self.publish()
        })();
        if result.is_err() && matches!(&result, Err(e) if e.contains("fatal")) { *self.failed.borrow_mut() = true; }
        *self.busy.borrow_mut() = false;
        result
    }
    pub fn status(&self) -> V { self.public_status() }
    pub fn close(&self) -> Result<(), String> {
        need(!*self.busy.borrow(), "BUSY")?;
        if *self.closed.borrow() { return Ok(()); }
        *self.closed.borrow_mut() = true;
        self.cleanup()
    }
}
