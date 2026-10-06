// data/reconcile-production-human-20261004.mjs — one-shot human-requested
// production account reconciliation with exact-money receipt capture.
use crate::publication_pack::{now_ms, read_json, runtime_dir};
fn load_key() -> Result<String, String> {
    if let Ok(k) = std::env::var("SIG_API_KEY") {
        return Ok(k.trim().to_string());
    }
    let script = "$ErrorActionPreference='Stop'; $s=(Get-Content -Raw -LiteralPath (Join-Path $env:LOCALAPPDATA 'SigTradingCup\\api-key.dpapi')).Trim()|ConvertTo-SecureString; [Console]::Write(([pscredential]::new('api',$s)).GetNetworkCredential().Password)";
    let out = std::process::Command::new("pwsh.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|_| "API key unavailable: SIG_API_KEY unset and DPAPI requires Windows".to_string())?;
    if !out.status.success() {
        return Err("API key unavailable: DPAPI credential read failed".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
use baal_collector::paper_runtime::lock;
use baal_execution::production::{fills_read_authorized, reconcile_production_account};
use baal_execution::receipts::{exact_money_fetch, MoneyReceiptOptions};
use baal_execution::transport::{OrderTransport, OrderTransportOptions};
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::client::{
    Fetch, FetchOutcome, FetchRequest, ReadClient, ReadClientOptions,
};
use baal_service_runtime::contract::ms_to_iso;
use serde_json::{json, Value as V};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

fn assert_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assert".into()) }
}
struct ExactFetch;
impl Fetch for ExactFetch {
    fn fetch(&self, url: &str, req: &FetchRequest) -> Result<FetchOutcome, String> {
        baal_service_runtime::client::ReqwestFetch::new()?.fetch(url, req)
    }
}
pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let read = |f: &str| read_json(Path::new(f));
    let marker = read(&format!("{rt}/COLLECTOR_STOP"))?;
    assert_eq(&marker["source"], &json!("human-production-20261004"))?;
    assert_eq(&read(&format!("{rt}/competition-activation.json"))?["enabled"], &json!(false))?;
    let release = lock(Path::new(&format!(
        "{}/SigTradingCup/bot.lock", std::env::var("LOCALAPPDATA").unwrap_or_default())))?;
    let budget = read(&format!("{rt}/coordinated-network-budget.json"))?;
    assert_eq(&budget["schemaVersion"], &json!(3))?;
    assert_eq(&budget["writesEnabled"], &json!(false))?;
    let rate = read(&format!("{rt}/rate.json"))?;
    assert_eq(&rate["manualReview"], &json!(false))?;
    if !rate["next"].is_number() && !rate["next"].is_null() { return Err("assert".into()); }
    let universe = read("live-paper/price-universe.json")?;
    let receipts = Arc::new(Mutex::new(Vec::<V>::new()));
    let receipts_c = receipts.clone();
    let tournament_id = universe["tournamentId"].as_str().unwrap_or("").to_string();
    let fetch = Arc::new(ExactFetch);
    let money_opts = MoneyReceiptOptions {
        tournament_id: &tournament_id,
        tournament_slug: "midterm-elections",
        on_receipt: &move |r: &V| { receipts_c.lock().unwrap().push(r.clone()); Ok(()) },
        now: &|| crate::publication_pack::now_ms(),
    };
    let plain = |url: &str, opts: &V| -> Result<V, String> {
        let _ = (url, opts);
        Err("FETCH_DIRECT".into())
    };
    let exact = exact_money_fetch(&plain, &money_opts);
    let _ = &exact;
    let rt_file = format!("{rt}/rate.json");
    let client = ReadClient::new(&load_key()?, ReadClientOptions {
        network_budget: Some(budget),
        fetch: Some(Arc::new(WrapExact { tournament_id: tournament_id.clone() })),
        authorize_dispatch: Some(Arc::new(|s: &V| {
            s["method"] == json!("GET") && fills_read_authorized(s)
        })),
        on_admission: Some(Arc::new(move |next: Option<f64>| {
            let _ = atomic_json(Path::new(&rt_file), &json!({
                "next": next, "manualReview": next.is_none(),
            }));
        })),
        ..Default::default()
    }).map_err(|e| e.to_string())?;
    let client = Arc::new(client);
    let dispatch_client = client.clone();
    let transport = OrderTransport::new(OrderTransportOptions {
        dispatch: Box::new(move |spec: &V, _bd, _bn| {
            dispatch_client.dispatch(baal_service_runtime::client::DispatchSpec {
                method: spec["method"].as_str().unwrap_or("").to_string(),
                endpoint: spec["endpoint"].as_str().unwrap_or("").to_string(),
                body: None, body_json: None, before_dispatch: None, before_network: None,
            }).map_err(|e| baal_execution::transport::OrderTransportError {
                status: e.status.map(|x| x as i64), code: e.message.clone(),
                api_code: e.api_code.clone(), outcome: "rejected".into(),
                may_have_executed: false, retry_same_request_only: false,
                retry_after_ms: e.retry_after_ms, semantic_mismatch: false,
            })
        }),
        scope: baal_execution::transport::scope_of(&json!({
            "tournamentId": universe["tournamentId"], "tournamentSlug": "midterm-elections",
            "exchanges": universe["exchanges"],
        })).map_err(|e| e.code)?,
        can_write: Box::new(|| false),
        owns_order: Box::new(|_| false),
        validate_lease: Box::new(|_: &V, _: &V| Ok(false)),
        validate_permit: Box::new(|_: &V, _: &V| Ok(false)),
        validate_network_permit: Box::new(|_: &V, _: &V| Ok(false)),
        now: Box::new(|| crate::publication_pack::now_ms()),
        max_pages: 1000,
    }).map_err(|e| e.code)?;
    let _ = &fetch;
    let dispatch = |s: &V| -> Result<V, String> {
        client.dispatch(baal_service_runtime::client::DispatchSpec {
            method: s["method"].as_str().unwrap_or("").to_string(),
            endpoint: s["endpoint"].as_str().unwrap_or("").to_string(),
            body: None, body_json: None, before_dispatch: None, before_network: None,
        }).map_err(|e| e.message)
    };
    let result = reconcile_production_account(&transport, &dispatch, &universe,
        &|| crate::publication_pack::now_ms(), 1000)?;
    let captured = receipts.lock().unwrap().clone();
    assert_eq(&json!(captured.len()), &json!(1))?;
    assert_eq(&json!(captured[0]["sourceBodyHash"].as_str().unwrap_or("").len()), &json!(64))?;
    let file = format!("{rt}/production-human-reconciliation-{}.json", now_ms());
    std::fs::OpenOptions::new().write(true).create_new(true).open(&file)
        .and_then(|mut f| {
            let mut o = result.clone();
            o.as_object_mut().unwrap().insert("exactCashReceipts".into(), json!(captured));
            f.write_all(format!("{}\n", serde_json::to_string_pretty(&o).unwrap()).as_bytes())
        }).map_err(|e| e.to_string())?;
    atomic_json(Path::new(&format!("{rt}/production-human-reconciliation-latest.json")), &json!({
        "file": Path::new(&file).file_name().unwrap().to_string_lossy(),
        "receiptHash": result["receiptHash"], "at": result["receipt"]["at"],
    }))?;
    client.close();
    drop(release);
    let mut out = result["summary"].clone();
    out.as_object_mut().unwrap().insert("at".into(), result["receipt"]["at"].clone());
    out.as_object_mut().unwrap().insert("requests".into(), json!(client.requests()));
    out.as_object_mut().unwrap().insert("competitionWrites".into(), json!(0));
    Ok(out)
}
struct WrapExact { tournament_id: String }
impl Fetch for WrapExact {
    fn fetch(&self, url: &str, req: &FetchRequest) -> Result<FetchOutcome, String> {
        let _ = self.tournament_id.clone();
        baal_service_runtime::client::ReqwestFetch::new()?.fetch(url, req)
    }
}
