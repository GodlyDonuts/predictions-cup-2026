// data/capture-paper-migration-inventory-20261006.mjs — capture the paper
// migration inventory for every current cohort.
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}

pub fn run() -> Result<V, String> {
    let runtime_dir = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let inventory = baal_collector::paper_migration_inventory::capture_paper_migration_inventory(
        &json!({"runtimeDir": runtime_dir.to_string_lossy()}))?;
    std::fs::write("data/paper-migration-inventory-20261006.json",
        format!("{}\n", serde_json::to_string_pretty(&inventory).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    let current: Vec<V> = inventory["current"].as_array().cloned().unwrap_or_default().iter()
        .map(|x| {
            let wallets = x["accounting"]["wallets"].as_object().cloned().unwrap_or_default();
            json!({"cohort": x["cohort"], "owner": x["authoritativeOwner"], "status": x["status"],
                "cash": V::Object(wallets.iter().map(|(id, w)| (id.clone(), w["cash"].clone())).collect()),
                "realizedPnl": V::Object(wallets.iter().map(|(id, w)| (id.clone(), w["realizedPnl"].clone())).collect())})
        }).collect();
    Ok(json!({"at": inventory["observedAt"], "cohorts": current,
        "cutoverReady": false, "accountRequests": 0}))
}
