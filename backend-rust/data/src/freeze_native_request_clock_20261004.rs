// data/freeze-native-request-clock-20261004.mjs — declare the v2 request-clock
// dip cohort derived verbatim from the sealed v1 declaration.
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
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
fn need(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err("assertion failed".into()) }
}

pub fn run() -> Result<V, String> {
    let runtime = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let old = runtime.join("research-cohorts")
        .join("baal-dip-subsequent-20261004-v1").join("declaration.json");
    let bytes = std::fs::read(&old).map_err(|e| e.to_string())?;
    need(sha(&bytes) == "eb1a6b525682762a323e68492096fd1f23e7ff080f4a51ce1bb832e594505456")?;
    let prior: V = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    need(prior["config"]["feeBps"].as_i64() == Some(0))?;
    let cohort = "baal-dip-request-clock-20261004-v2";
    let dir = runtime.join("research-cohorts").join(cohort);
    need(!dir.exists())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    let mut declaration = prior.clone();
    if let V::Object(m) = &mut declaration {
        m.insert("mode".into(), json!("causal_dip_subsequent_v2"));
        m.insert("cohort".into(), json!(cohort));
        m.insert("declaredAt".into(), json!(now_iso()));
        let mut config = prior["config"].clone();
        if let V::Object(cm) = &mut config {
            cm.insert("fillClockPolicy".into(), json!("independent_request_latency_v2"));
        }
        m.insert("config".into(), config);
    }
    let encoded = format!("{}\n",
        serde_json::to_string_pretty(&declaration).map_err(|e| e.to_string())?);
    write_wx(&dir.join("declaration.json"), encoded.as_bytes())?;
    let receipt = json!({"at": declaration["declaredAt"], "cohort": cohort,
        "declarationHash": sha(encoded.as_bytes()),
        "priorDeclarationHash": sha(&bytes), "development": declaration["development"],
        "candidates": declaration["strategies"].as_array().map(|a| a.len()).unwrap_or(0)
            * declaration["scenarios"].as_array().map(|a| a.len()).unwrap_or(0) * 2,
        "strategiesUnchanged": true, "scenariosUnchanged": true,
        "parametersTuned": false,
        "fillClockPolicy": declaration["config"]["fillClockPolicy"],
        "tradingStartsAfter": declaration["declaredAt"],
        "preDeclarationWalletActivity": false, "fees": 0, "networkRequests": 0,
        "competitionOrders": 0, "profitabilityEstablished": false});
    write_wx(&dir.join("declaration-receipt.json"),
        format!("{}\n", serde_json::to_string_pretty(&receipt).map_err(|e| e.to_string())?).as_bytes())?;
    write_wx(Path::new("data/native-request-clock-declaration-20261004.json"),
        format!("{}\n", serde_json::to_string_pretty(
            &json!({"receipt": receipt, "declaration": declaration}))
            .map_err(|e| e.to_string())?).as_bytes())?;
    Ok(receipt)
}
