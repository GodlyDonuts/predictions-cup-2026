// data/deploy-production-collector-20261004.mjs — update the collector native
// journal manifest to the production host graph (writes remain disabled).
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn ph(v: &V) -> Result<String, String> { baal_execution::core::payload_hash(v) }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn write_wx(path: &Path, body: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, body).map_err(|e| e.to_string())
}

pub fn run() -> Result<V, String> {
    let runtime = localappdata()?.join("SigTradingCup/paper-runtime");
    let file = runtime.join("collector-native-journal.json");
    let original = std::fs::read(&file).map_err(|e| e.to_string())?;
    let c: V = serde_json::from_slice(&original).map_err(|e| e.to_string())?;
    let activation = read_json(&runtime.join("competition-activation.json"))?;
    let budget = read_json(&runtime.join("coordinated-network-budget.json"))?;
    if activation["enabled"] != json!(false)
        || budget["schemaVersion"] != json!(3)
        || budget["maxReadsPerMinute"] != json!(20)
        || budget["writesEnabled"] != json!(false) {
        return Err("READ_ONLY_DEPLOYMENT_REQUIRED".into());
    }
    let identity = baal_collector::collector_journal_native::collector_journal_code_identity()?;
    let mut changed: Vec<String> = Vec::new();
    if let V::Object(id) = &identity {
        for k in id.keys() {
            if identity[k] != c["codeIdentity"][k] { changed.push(k.clone()); }
        }
    }
    if changed != ["live-paper/transport-network-budget.mjs"] {
        return Err("UNEXPECTED_HOST_GRAPH_CHANGE".into());
    }
    let backup = runtime.join("collector-native-journal-before-production-20261004.json");
    write_wx(&backup, &original)?;
    let active = read_json(&runtime.join("probability-receipt-active.json"))?;
    let bank_file = runtime.join("probability-receipt-cohorts")
        .join(active["cohort"].as_str().unwrap_or("")).join("bank.json");
    let mut before = json!({"at": now_iso(), "oldCollectorPid": 27488,
        "activationHash": sha(&std::fs::read(runtime.join("competition-activation.json"))
            .map_err(|e| e.to_string())?),
        "budgetHash": sha(&std::fs::read(runtime.join("coordinated-network-budget.json"))
            .map_err(|e| e.to_string())?),
        "probabilityActiveHash": sha(&std::fs::read(runtime.join("probability-receipt-active.json"))
            .map_err(|e| e.to_string())?),
        "probabilityBankHash": sha(&std::fs::read(&bank_file).map_err(|e| e.to_string())?),
        "oldManifestHash": sha(&original), "newManifestHash": V::Null,
        "changed": changed, "restWritesEnabled": false});
    let mut next = c.clone();
    if let V::Object(m) = &mut next {
        m.insert("codeIdentity".into(), identity.clone());
        m.insert("sourceSnapshot".into(), json!(ph(&identity)?));
    }
    baal_service_runtime::atomic::atomic_json_canonical(&file, &next)?;
    let _ = baal_collector::collector_journal_native::collector_native_journal_config(&runtime)?;
    before["newManifestHash"] = json!(sha(&std::fs::read(&file).map_err(|e| e.to_string())?));
    baal_service_runtime::atomic::atomic_json_canonical(
        &runtime.join("production-metadata-request.json"),
        &json!({"schemaVersion": 1, "name": "baal-production-20261004-v1",
            "marketIds": ["297", "363", "357"]}))?;
    let marker = runtime.join("COLLECTOR_STOP");
    if marker.exists() { return Err("EXISTING_STOP_MUST_BE_REVIEWED".into()); }
    let body = format!("{}\n", serde_json::to_string(&json!({"owner": "Codex",
        "claimComment": 5984073274i64, "at": now_iso(),
        "reason": "Checked production-integration collector restart; writes remain disabled",
        "explicitHumanStop": false})).map_err(|e| e.to_string())?);
    write_wx(&marker, body.as_bytes())?;
    before["deploymentStopHash"] = json!(sha(body.as_bytes()));
    write_wx(Path::new("data/production-collector-deployment-20261004.json"),
        format!("{}\n", serde_json::to_string_pretty(&before).map_err(|e| e.to_string())?).as_bytes())?;
    Ok(json!({"deploymentStopRequested": true, "changed": before["changed"],
        "readCap": 20, "writesEnabled": false}))
}
