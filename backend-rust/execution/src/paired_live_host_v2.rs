// execution/paired-live-host-v2.mjs — one selection, then the unchanged v1
// actor/bridge/funding lifecycle. No new client, account lease, limiter, timer,
// dynamic pair replacement or renewal.
use crate::core::payload_hash;
use crate::paired_live_host::{
    PairedLiveApi, PairedLiveHost, PairedLiveHostDeps, open_paired_live_host, paired_live_identity,
};

type HostGuard<'a> = dyn Fn(&V, &V) -> Result<bool, String> + 'a;
use crate::paired_rest_controller::derive_paired_rest_plan;
use crate::transport::Guard as TGuard;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::fixed::instant;
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::sha256;
use baal_service_runtime::ledger::acquire_owner_lease;
use serde_json::Value as V;
use serde_json::json;
use std::cell::RefCell;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const PREFIX: &str = "PAIRED_V2_";
const TID: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok {
        return Err(format!("{PREFIX}{code}"));
    }
    Ok(())
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn iso(n: f64) -> String {
    baal_service_runtime::contract::ms_to_iso(n)
}
fn read(p: &Path) -> Result<V, String> {
    serde_json::from_str(&fs::read_to_string(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn write(file: &Path, value: &V) -> Result<(), String> {
    atomic_json(file, value).map_err(|e| format!("{e}:fatal:durableRecordingFailed"))
}
fn hex(x: &V) -> bool {
    x.as_str()
        .map(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()))
        .unwrap_or(false)
}
fn id(x: &V) -> bool {
    x.as_str()
        .map(|s| {
            !s.is_empty()
                && s.bytes().all(|b| b.is_ascii_digit())
                && !s.starts_with('0')
                && s.parse::<i64>().is_ok()
        })
        .unwrap_or(false)
}
fn run_id_ok(s: &str) -> bool {
    s.strip_prefix("sig-pilot-paired-").is_some_and(|t| {
        !t.is_empty()
            && t.len() <= 60
            && t.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}
// clock(x): strict ISO-8601 ms roundtrip.
fn clock(x: &V) -> Result<i64, String> {
    let n = instant(x.as_str().unwrap_or(""))?;
    need(
        iso(n as f64) == x.as_str().unwrap_or(""),
        "CLOCK",
    )?;
    Ok(n)
}

// pairedLiveV2Identity(workspace)
pub fn paired_live_v2_identity(workspace: &Path) -> Result<V, String> {
    crate::frozen::collect_frozen_paper_files(
        workspace,
        &[
            "execution/paired-live-host-v2.mjs",
            "execution/scoped-rest-receipts.mjs",
        ],
    )
}

fn validate_request(r: &V, u: &V) -> Result<(), String> {
    need(
        get(r, "schemaVersion") == &json!(2)
            && get(r, "mode").as_str() == Some("one_pair_from_allowed_markets_v2")
            && get(r, "enabled") == &json!(true)
            && get(r, "ownerId").as_str() == Some("Codex")
            && get(r, "tournamentId").as_str() == Some(TID)
            && get(r, "tournamentSlug").as_str() == Some("midterm-elections")
            && get(r, "runId").as_str().map(run_id_ok).unwrap_or(false),
        "REQUEST",
    )?;
    need(
        get(u, "tournamentId").as_str() == Some(TID)
            && hex(get(u, "hash"))
            && get(r, "universeHash") == get(u, "hash")
            && get(u, "exchanges").is_array(),
        "UNIVERSE",
    )?;
    let markets = get(r, "allowedMarkets").as_array().cloned().unwrap_or_default();
    let exchanges = get(u, "exchanges").as_array().cloned().unwrap_or_default();
    need(
        markets.len() >= 2
            && markets.len() <= 300
            && markets
                .iter()
                .map(|m| get(m, "marketId").as_str().unwrap_or(""))
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == markets.len()
            && markets
                .iter()
                .map(|m| get(m, "exchangeId").as_str().unwrap_or(""))
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == markets.len()
            && markets.iter().all(|m| {
                let mut ks: Vec<String> = m.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
                ks.sort();
                ks.join("|") == "exchangeId|marketId"
                    && id(get(m, "marketId"))
                    && id(get(m, "exchangeId"))
                    && exchanges.iter().any(|x| {
                        get(x, "marketId") == get(m, "marketId")
                            && get(x, "exchangeId") == get(m, "exchangeId")
                            && get(x, "option").as_str() == Some("YES")
                    })
            }),
        "ALLOWED_MARKETS",
    )?;
    need(
        clock(get(r, "entryExpiresAt"))? > clock(get(r, "startedAt"))?
            && clock(get(r, "entryExpiresAt"))? - clock(get(r, "startedAt"))? <= 3600000
            && hex(get(r, "authorityHash")),
        "WINDOW_OR_AUTHORITY",
    )?;
    Ok(())
}

// selectPairedLiveCandidate({request,universe,capture,now})
pub fn select_paired_live_candidate(
    request: &V,
    universe: &V,
    capture: &V,
    now: f64,
) -> Result<V, String> {
    validate_request(request, universe)?;
    let ineligible = |reason: &str| json!({"eligible": false, "reason": reason});
    if now < clock(get(request, "startedAt"))? as f64 || now >= clock(get(request, "entryExpiresAt"))? as f64 {
        return Ok(ineligible("entry_window_closed"));
    }
    if get(capture, "phase").as_str() != Some("complete") {
        return Ok(ineligible("awaiting_complete_capture"));
    }
    if get(&get(capture, "screen"), "positiveConditionalScreen") != &json!(true) {
        return Ok(ineligible("no_qualified_pair"));
    }
    let legs = get(capture, "legs").as_array().cloned().unwrap_or_default();
    if legs.len() != 2
        || legs.iter().any(|l| {
            !get(request, "allowedMarkets")
                .as_array()
                .map(|ms| {
                    ms.iter().any(|m| {
                        get(m, "marketId") == get(l, "marketId") && get(m, "exchangeId") == get(l, "exchangeId")
                    })
                })
                .unwrap_or(false)
        })
    {
        return Ok(ineligible("outside_allowed_markets"));
    }
    let result: Result<V, String> = (|| {
        let markets: Vec<V> = legs
            .iter()
            .map(|l| {
                json!({
                    "marketId": get(l, "marketId"),
                    "exchangeId": get(l, "exchangeId"),
                })
            })
            .collect();
        let scope = json!({
            "ownerId": "Codex",
            "runId": get(request, "runId"),
            "tournamentId": TID,
            "tournamentSlug": get(request, "tournamentSlug"),
            "markets": markets,
        });
        let selected_at = iso(now);
        let plan = derive_paired_rest_plan(&json!({
            "capture": capture,
            "scope": scope,
            "readAt": selected_at,
        }))?;
        if clock(get(&plan, "reviewExpiresAt"))? > clock(get(request, "entryExpiresAt"))? {
            return Ok(ineligible("review_outside_entry_window"));
        }
        let minimum_margin = if get(&plan, "minimumOrdinaryMargin").is_null() {
            get(&plan, "sourceMinimumOrdinaryMargin").clone()
        } else {
            get(&plan, "minimumOrdinaryMargin").clone()
        };
        Ok(json!({
            "eligible": true,
            "selection": {
                "schemaVersion": 1,
                "requestHash": payload_hash(request)?,
                "authorityHash": get(request, "authorityHash"),
                "selectedAt": selected_at,
                "scope": scope,
                "capture": capture,
                "captureHash": payload_hash(capture)?,
                "qualificationPlanHash": get(&plan, "planHash"),
                "reviewExpiresAt": get(&plan, "reviewExpiresAt"),
                "originalProtectiveCost": get(&plan, "originalProtectiveCost"),
                "pairCostCeiling": get(&plan, "pairCostCeiling"),
                "minimumOrdinaryMargin": minimum_margin,
            },
        }))
    })();
    match result {
        Ok(v) => Ok(v),
        Err(_) => Ok(ineligible("awaiting_fresh_qualified_capture")),
    }
}

#[allow(clippy::type_complexity)]
pub struct PairedLiveHostV2 {
    inner: Rc<RefCell<InnerV2>>,
}

struct InnerV2 {
    request: V,
    request_hash: String,
    file: PathBuf,
    legacy_request_file: PathBuf,
    authority_file: PathBuf,
    authority: V,
    code: V,
    dir: PathBuf,
    child_dir: PathBuf,
    selection_file: PathBuf,
    runtime_dir: PathBuf,
    api: Rc<RefCell<PairedLiveApi>>,
    deps: PairedLiveHostDeps,
    universe: V,
    lease: baal_service_runtime::ledger::OwnerLease,
    selection: Option<V>,
    delegate: Option<PairedLiveHost>,
    closed: bool,
    busy: bool,
    failed: bool,
    wait_reason: Option<String>,
    selection_bytes: Option<Vec<u8>>,
}

fn fixed_proof(inner: &Rc<RefCell<InnerV2>>) -> Result<(), String> {
    let (file, legacy, authority_file, request_hash, authority_hash, code, workspace, selection_file, selection_bytes) = {
        let g = inner.borrow();
        g.lease.assert_ownership().map_err(|e| format!("{PREFIX}{}", e.message))?;
        (
            g.file.clone(),
            g.legacy_request_file.clone(),
            g.authority_file.clone(),
            g.request_hash.clone(),
            payload_hash(&g.authority)?,
            payload_hash(&g.code)?,
            g.deps.workspace.clone(),
            g.selection_file.clone(),
            g.selection_bytes.clone(),
        )
    };
    need(!legacy.exists(), "LEGACY_REQUEST_MUST_BE_RETIRED")?;
    need(
        payload_hash(&read(&file)?)? == request_hash
            && payload_hash(&read(&authority_file)?)? == authority_hash
            && payload_hash(&paired_live_v2_identity(&workspace)?)? == code,
        "SOURCE_IDENTITY",
    )?;
    if let Some(bytes) = selection_bytes {
        need(fs::read(&selection_file).map_err(|e| e.to_string())? == bytes, "SELECTION_CHANGED")?;
    } else {
        need(!selection_file.exists(), "UNEXPECTED_SELECTION")?;
    }
    Ok(())
}

fn allowed_v2(inner: &Rc<RefCell<InnerV2>>) -> bool {
    (|| -> Result<bool, String> {
        {
            let g = inner.borrow();
            if g.closed || g.failed || !(g.deps.can_operate)() {
                return Ok(false);
            }
        }
        fixed_proof(inner)?;
        Ok(true)
    })()
    .unwrap_or(false)
}

fn child_actor(inner: &Rc<RefCell<InnerV2>>) -> PathBuf {
    let g = inner.borrow();
    g.child_dir
        .join("paired-live-runs")
        .join(get(&g.request, "runId").as_str().unwrap_or(""))
        .join("controller")
        .join("paired-rest-journal.jsonl")
}

fn child_request(inner: &Rc<RefCell<InnerV2>>) -> V {
    let g = inner.borrow();
    let selection = g.selection.clone().unwrap_or(V::Null);
    json!({
        "schemaVersion": 1,
        "enabled": true,
        "ownerId": "Codex",
        "runId": get(&g.request, "runId"),
        "tournamentId": TID,
        "tournamentSlug": get(&g.request, "tournamentSlug"),
        "markets": get(&get(&selection, "scope"), "markets"),
        "startedAt": get(&g.request, "startedAt"),
        "entryExpiresAt": get(&selection, "reviewExpiresAt"),
        "authorityHash": get(&g.request, "authorityHash"),
        "codeIdentity": paired_live_identity(&g.deps.workspace).unwrap_or(V::Null),
    })
}

fn ensure_exact(file: &Path, value: &V) -> Result<(), String> {
    if file.exists() {
        need(payload_hash(&read(file)?)? == payload_hash(value)?, "CHILD_IDENTITY")
    } else {
        write(file, value)
    }
}

fn open_child(inner: &Rc<RefCell<InnerV2>>) -> Result<(), String> {
    {
        let g = inner.borrow();
        need(g.selection.is_some(), "SELECTION_REQUIRED")?;
    }
    let (child_dir, api, deps_fields) = {
        let g = inner.borrow();
        (
            g.child_dir.clone(),
            g.api.clone(),
            (
                g.deps.workspace.clone(),
                g.deps.receipt_of.clone(),
                g.deps.get_account_lease.clone(),
                g.deps.get_other_state.clone(),
                g.deps.has_read_headroom.clone(),
                g.deps.now.clone(),
            ),
        )
    };
    fs::create_dir_all(&child_dir).map_err(|e| e.to_string())?;
    fs::set_permissions(&child_dir, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let authority = inner.borrow().authority.clone();
    ensure_exact(&child_dir.join("paired-live-authority.json"), &authority)?;
    ensure_exact(&child_dir.join("paired-live-request.json"), &child_request(inner))?;
    let inner_allowed = inner.clone();
    let delegate = open_paired_live_host(
        &child_dir,
        api,
        PairedLiveHostDeps {
            receipt_of: deps_fields.1,
            get_account_lease: deps_fields.2,
            get_other_state: deps_fields.3,
            can_operate: Rc::new(move || allowed_v2(&inner_allowed)),
            has_read_headroom: deps_fields.4,
            now: deps_fields.5,
            workspace: deps_fields.0,
        },
    )?;
    need(delegate.is_some(), "CHILD_REQUIRED")?;
    inner.borrow_mut().delegate = delegate;
    Ok(())
}

fn blocked(inner: &Rc<RefCell<InnerV2>>) -> bool {
    let g = inner.borrow();
    if g.selection.is_none() {
        return false;
    }
    match &g.delegate {
        Some(d) => {
            let s = d.status();
            !get(&s, "initialized").as_bool().unwrap_or(false) || d.blocks_legacy_writes()
        }
        None => true,
    }
}

fn public_status(inner: &Rc<RefCell<InnerV2>>) -> V {
    let g = inner.borrow();
    let s = g.delegate.as_ref().map(|d| d.status());
    let initialized = s
        .as_ref()
        .map(|x| get(x, "initialized").as_bool().unwrap_or(false))
        .unwrap_or(false);
    let wait_reason = g.wait_reason.clone();
    json!({
        "hostVersion": 2,
        "mode": "official_one_pair_rest",
        "runId": get(&g.request, "runId"),
        "requestHash": g.request_hash,
        "authorityHash": get(&g.request, "authorityHash"),
        "universeHash": get(&g.request, "universeHash"),
        "allowedMarketCount": get(&g.request, "allowedMarkets").as_array().map(|a| a.len()).unwrap_or(0),
        "selectedMarkets": g.selection.as_ref().map(|x| get(&get(x, "scope"), "markets").clone()).unwrap_or(json!([])),
        "selectionHash": g.selection.as_ref().map(|x| json!(payload_hash(x).unwrap_or_default())).unwrap_or(V::Null),
        "captureHash": g.selection.as_ref().map(|x| get(x, "captureHash").clone()).unwrap_or(V::Null),
        "entryExpiresAt": get(&g.request, "entryExpiresAt"),
        "reviewExpiresAt": g.selection.as_ref().map(|x| get(x, "reviewExpiresAt").clone()).unwrap_or(V::Null),
        "initialized": initialized,
        "phase": if initialized {
            get(s.as_ref().unwrap(), "phase").clone()
        } else if g.selection.is_some() {
            json!("selected_waiting_for_initialization")
        } else {
            json!(wait_reason.clone().unwrap_or_else(|| "awaiting_complete_capture".to_string()))
        },
        "waitReason": wait_reason.map(V::from).unwrap_or(V::Null),
        "lastFailure": s.as_ref().map(|x| get(x, "lastFailure").clone()).unwrap_or(V::Null),
        "failed": g.failed || s.as_ref().map(|x| get(x, "failed").as_bool().unwrap_or(false)).unwrap_or(false),
        "closed": g.closed,
        "orders": s.as_ref().map(|x| get(x, "orders").clone()).unwrap_or(json!(0)),
        "blocksLegacyWrites": blocked(inner),
        "additionalPersistentProcesses": 0,
        "additionalAccountClients": 0,
    })
}

fn publish(inner: &Rc<RefCell<InnerV2>>) -> Result<V, String> {
    let (status, child_stats_file, runtime_dir, request_run_id, request_authority_hash, delegate_present, n) = {
        let g = inner.borrow();
        (
            public_status(inner),
            g.child_dir.join("paired-live-stats.json"),
            g.runtime_dir.join("paired-live-stats.json"),
            get(&g.request, "runId").as_str().unwrap_or("").to_string(),
            get(&g.request, "authorityHash").as_str().unwrap_or("").to_string(),
            g.delegate.is_some(),
            (g.deps.now)(),
        )
    };
    let mut child = serde_json::Map::new();
    if delegate_present && child_stats_file.exists() {
        let v = read(&child_stats_file)?;
        need(
            get(&v, "runId").as_str() == Some(request_run_id.as_str())
                && get(&v, "authorityHash").as_str() == Some(request_authority_hash.as_str()),
            "CHILD_STATS",
        )?;
        for key in [
            "action",
            "orders",
            "owned",
            "funding",
            "realizedPnl",
            "ordinaryMinimumPayout",
            "ordinarySurplus",
            "ordinaryResolutionOnly",
            "maximumExceptionalLoss",
            "profitabilityEstablished",
        ] {
            if let Some(x) = v.get(key) {
                child.insert(key.to_string(), x.clone());
            }
        }
    }
    let mut view = serde_json::Map::new();
    view.insert("schemaVersion".into(), json!(2));
    for (k, v) in child.iter() {
        view.insert(k.clone(), v.clone());
    }
    if let Some(obj) = status.as_object() {
        for (k, v) in obj.iter() {
            view.insert(k.clone(), v.clone());
        }
    }
    view.insert(
        "orders".into(),
        child.get("orders").cloned().unwrap_or(json!([])),
    );
    view.insert("orderCount".into(), get(&status, "orders").clone());
    view.insert("observedAt".into(), json!(iso(n)));
    write(&runtime_dir, &V::Object(view))?;
    Ok(status)
}

fn cleanup(inner: &Rc<RefCell<InnerV2>>) -> Result<(), String> {
    let mut error: Option<String> = None;
    {
        let g = inner.borrow();
        if let Some(d) = &g.delegate {
            if let Err(e) = d.close() {
                error = Some(e);
            }
        }
    }
    inner.borrow_mut().delegate = None;
    if let Err(e) = inner.borrow_mut().lease.release().map_err(|e| format!("{PREFIX}{}", e.message)) {
        if error.is_none() {
            error = Some(e);
        }
    }
    match error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

// openPairedLiveHostV2({runtimeDir,workspace,universe,api,receiptOf,getAccountLease,getOtherState,canOperate,hasReadHeadroom,now})
#[allow(clippy::too_many_arguments)]
pub fn open_paired_live_host_v2(
    runtime_dir: &Path,
    universe: &V,
    api: Rc<RefCell<PairedLiveApi>>,
    deps: PairedLiveHostDeps,
) -> Result<Option<PairedLiveHostV2>, String> {
    let file = runtime_dir.join("paired-live-v2-request.json");
    if !file.exists() {
        return Ok(None);
    }
    let legacy_request_file = runtime_dir.join("paired-live-request.json");
    need(!legacy_request_file.exists(), "LEGACY_REQUEST_MUST_BE_RETIRED")?;
    need(
        runtime_dir.is_absolute() && deps.workspace.is_absolute(),
        "DEPENDENCIES",
    )?;
    let request = read(&file)?;
    validate_request(&request, universe)?;
    let request_hash = payload_hash(&request)?;
    let authority_file = runtime_dir.join("paired-live-authority.json");
    let authority = read(&authority_file)?;
    let code = paired_live_v2_identity(&deps.workspace)?;
    need(
        payload_hash(&code)? == payload_hash(get(&request, "codeIdentity"))?,
        "CODE",
    )?;
    need(
        payload_hash(&authority)? == get(&request, "authorityHash").as_str().unwrap_or("")
            && get(&authority, "kind").as_str() == Some("direct_human_authorized_bot_update")
            && get(&authority, "ownerId").as_str() == Some("Codex")
            && get(&authority, "threadId").as_str() == Some("01a0f8cc-b9bb-77d2-9f4c-2844396f1b79")
            && get(&authority, "exploratory") == &json!(true)
            && get(&authority, "profitabilityEstablished") == &json!(false),
        "AUTHORITY",
    )?;
    let run_id = get(&request, "runId").as_str().unwrap_or("").to_string();
    let dir = runtime_dir.join("paired-live-v2-runs").join(&run_id);
    let child_dir = dir.join("child-runtime");
    let selection_file = dir.join("selection.jsonl");
    let lease = acquire_owner_lease(
        &dir,
        "Codex",
        &run_id,
        "paired-live-selector-v2",
        (deps.now)(),
    )
    .map_err(|e| format!("{PREFIX}{}", e.message))?;
    let inner = Rc::new(RefCell::new(InnerV2 {
        request: request.clone(),
        request_hash,
        file,
        legacy_request_file,
        authority_file,
        authority,
        code,
        dir,
        child_dir,
        selection_file,
        runtime_dir: runtime_dir.to_path_buf(),
        api,
        deps,
        universe: universe.clone(),
        lease,
        selection: None,
        delegate: None,
        closed: false,
        busy: false,
        failed: false,
        wait_reason: Some("awaiting_complete_capture".to_string()),
        selection_bytes: None,
    }));
    let open_result = (|| -> Result<(), String> {
        let selection_file = inner.borrow().selection_file.clone();
        let child_dir = inner.borrow().child_dir.clone();
        if selection_file.exists() {
            let bytes = fs::read(&selection_file).map_err(|e| e.to_string())?;
            need(
                !bytes.is_empty() && bytes.len() <= 2 * 1024 * 1024 && *bytes.last().unwrap() == 10,
                "SELECTION_JOURNAL",
            )?;
            let journal = Journal::open(&selection_file)?;
            let last = journal.last.clone().unwrap_or(V::Null);
            need(
                journal.sequence == 1
                    && get(&get(&last, "event"), "kind").as_str() == Some("pair_selected")
                    && get(&get(&last, "event"), "requestHash").as_str()
                        == Some(inner.borrow().request_hash.as_str()),
                "SELECTION_JOURNAL",
            )?;
            let selection = get(&last, "state").clone();
            let qualified = select_paired_live_candidate(
                &inner.borrow().request.clone(),
                &universe.clone(),
                &get(&selection, "capture").clone(),
                clock(get(&selection, "selectedAt"))? as f64,
            )?;
            need(
                get(&qualified, "eligible") == &json!(true)
                    && payload_hash(get(&qualified, "selection"))? == payload_hash(&selection)?,
                "SELECTION_PROOF",
            )?;
            {
                let mut g = inner.borrow_mut();
                g.selection = Some(selection);
                g.selection_bytes = Some(bytes);
            }
            open_child(&inner)?;
        } else {
            need(!child_dir.exists(), "UNBOUND_CHILD")?;
        }
        publish(&inner)?;
        Ok(())
    })();
    if let Err(e) = open_result {
        let _ = cleanup(&inner);
        return Err(e);
    }
    Ok(Some(PairedLiveHostV2 { inner }))
}

impl PairedLiveHostV2 {
    // authorizes(spec) — GET side.
    pub fn authorizes(&self, spec: &V, before_dispatch: &HostGuard, before_network: &HostGuard) -> bool {
        if !allowed_v2(&self.inner) {
            return false;
        }
        self.inner
            .borrow()
            .delegate
            .as_ref()
            .map(|d| d.authorizes(spec, before_dispatch, before_network))
            .unwrap_or(false)
    }

    // authorizes(spec) — write side.
    pub fn authorizes_write(
        &self,
        spec: &V,
        before_dispatch: Option<&TGuard<'_>>,
        before_network: Option<&TGuard<'_>>,
    ) -> bool {
        if !allowed_v2(&self.inner) {
            return false;
        }
        self.inner
            .borrow()
            .delegate
            .as_ref()
            .map(|d| d.authorizes_write(spec, before_dispatch, before_network))
            .unwrap_or(false)
    }

    pub fn on_fetch_start(&self, event: &V) -> bool {
        self.inner
            .borrow()
            .delegate
            .as_ref()
            .map(|d| d.on_fetch_start(event))
            .unwrap_or(false)
    }

    pub fn blocks_legacy_writes(&self) -> bool {
        blocked(&self.inner)
    }

    pub fn allows_legacy(&self, spec: &V) -> bool {
        let g = self.inner.borrow();
        if g.selection.is_none() {
            return true;
        }
        match &g.delegate {
            Some(d) => {
                let s = d.status();
                if get(&s, "initialized").as_bool().unwrap_or(false) {
                    d.allows_legacy(spec)
                } else {
                    get(spec, "method").as_str() == Some("GET")
                }
            }
            None => get(spec, "method").as_str() == Some("GET"),
        }
    }

    pub fn step(&self, capture: &V) -> Result<V, String> {
        need(
            !self.inner.borrow().busy && !self.inner.borrow().closed,
            "BUSY_OR_CLOSED",
        )?;
        self.inner.borrow_mut().busy = true;
        let result = (|| -> Result<V, String> {
            if !allowed_v2(&self.inner) {
                self.inner.borrow_mut().wait_reason = Some("operational_stop_or_identity_changed".into());
                return publish(&self.inner);
            }
            if self.inner.borrow().selection.is_none() {
                let result = select_paired_live_candidate(
                    &self.inner.borrow().request.clone(),
                    &self.inner.borrow().universe.clone(),
                    capture,
                    (self.inner.borrow().deps.now)(),
                )?;
                let eligible = get(&result, "eligible") == &json!(true);
                self.inner.borrow_mut().wait_reason =
                    if eligible { None } else { get(&result, "reason").as_str().map(|s| s.to_string()) };
                if !eligible {
                    return publish(&self.inner);
                }
                fixed_proof(&self.inner)?;
                let selection_file = self.inner.borrow().selection_file.clone();
                let mut journal = Journal::open(&selection_file)?;
                let selection = get(&result, "selection").clone();
                let commit_result = journal.commit(
                    &selection,
                    &json!({
                        "kind": "pair_selected",
                        "requestHash": self.inner.borrow().request_hash.clone(),
                    }),
                );
                if let Err(e) = commit_result {
                    self.inner.borrow_mut().failed = true;
                    return Err(format!("{PREFIX}{e}:fatal:durableRecordingFailed"));
                }
                let bytes = fs::read(&selection_file).map_err(|e| e.to_string())?;
                {
                    let mut g = self.inner.borrow_mut();
                    g.selection = Some(selection);
                    g.selection_bytes = Some(bytes);
                }
                open_child(&self.inner)?;
            }
            if self.inner.borrow().delegate.is_none() {
                open_child(&self.inner)?;
            }
            if !child_actor(&self.inner).exists() {
                let selection = self.inner.borrow().selection.clone().unwrap_or(V::Null);
                let fresh = select_paired_live_candidate(
                    &self.inner.borrow().request.clone(),
                    &self.inner.borrow().universe.clone(),
                    &get(&selection, "capture").clone(),
                    (self.inner.borrow().deps.now)(),
                )?;
                if get(&fresh, "eligible") != &json!(true) {
                    self.inner.borrow_mut().wait_reason =
                        Some("selected_source_expired_unexecuted".into());
                    return publish(&self.inner);
                }
            }
            self.inner.borrow_mut().wait_reason = None;
            let capture = get(&self.inner.borrow().selection.clone().unwrap_or(V::Null), "capture").clone();
            if let Some(d) = &self.inner.borrow().delegate {
                d.step(&capture)?;
            }
            publish(&self.inner)
        })();
        if let Err(e) = &result {
            if e.contains(":fatal:") || e.contains("durableRecordingFailed") {
                self.inner.borrow_mut().failed = true;
            }
        }
        self.inner.borrow_mut().busy = false;
        result
    }

    pub fn status(&self) -> V {
        public_status(&self.inner)
    }

    pub fn close(&self) -> Result<(), String> {
        need(!self.inner.borrow().busy, "BUSY")?;
        if self.inner.borrow().closed {
            return Ok(());
        }
        self.inner.borrow_mut().closed = true;
        cleanup(&self.inner)
    }
}
