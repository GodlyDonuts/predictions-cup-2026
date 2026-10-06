// data/check-paper-migration-inventory-20261006.mjs — verify the captured
// inventory rejects both unproven native imports and base resurrections.
use serde_json::{json, Value as V};
use std::path::Path;

fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}

pub fn run() -> Result<V, String> {
    let inventory = read_json(Path::new("data/paper-migration-inventory-20261006.json"))?;
    let now_ms = baal_service_runtime::contract::parse_iso_ms(
        inventory["observedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
    let proposed: Vec<V> = inventory["current"].as_array().cloned().unwrap_or_default()
        .into_iter().map(|r| json!({
            "cohort": r["cohort"], "originalBankHash": r["bankHash"],
            "originalBankMode": r["bankMode"], "originalAnchors": r["anchors"],
            "authoritativeOwner": r["authoritativeOwner"], "overlayHash": r["overlayHash"],
            "baseActorEnabled": r["authoritativeOwner"].as_str() != Some("operator_overlay"),
            "status": r["status"], "accounting": r["accounting"],
            "nativeReplayVerified": false, "originalSourceReplayVerified": false
        })).collect();
    let input = json!({"observedAt": inventory["observedAt"], "now": now_ms,
        "current": inventory["current"], "proposed": proposed});
    let mut rejected_unproven = false;
    match baal_collector::paper_migration_guard::validate_paper_migration(&input) {
        Ok(_) => {}
        Err(e) => {
            if !e.contains("NATIVE_REPLAY_UNPROVEN") { return Err(e); }
            rejected_unproven = true;
        }
    }
    // Deliberately malformed test candidates; setting these flags supplies no
    // actual native proof and can never activate a runtime.
    let mut wrong = input.clone();
    if let V::Array(ps) = &mut wrong["proposed"] {
        for r in ps.iter_mut() {
            r["nativeReplayVerified"] = json!(true);
            r["originalSourceReplayVerified"] = json!(true);
        }
        let tx = ps.iter_mut().find(|r| r["authoritativeOwner"].as_str() == Some("operator_overlay"));
        let tx = match tx {
            Some(t) => t,
            None => return Err("Missing actual overlay".into()),
        };
        tx["authoritativeOwner"] = json!("original_reducer");
        tx["baseActorEnabled"] = json!(true);
    }
    let mut rejected_resurrection = false;
    match baal_collector::paper_migration_guard::validate_paper_migration(&wrong) {
        Ok(_) => {}
        Err(e) => {
            if !e.contains("FENCED_BASE_RESURRECTION") { return Err(e); }
            rejected_resurrection = true;
        }
    }
    if !rejected_unproven || !rejected_resurrection {
        return Err("Actual inventory rejection check failed".into());
    }
    let overlay = inventory["current"].as_array().cloned().unwrap_or_default().into_iter()
        .find(|r| r["authoritativeOwner"].as_str() == Some("operator_overlay"))
        .unwrap_or(V::Null);
    let out = json!({"schemaVersion": 1, "at": now_iso(),
        "inventoryObservedAt": inventory["observedAt"],
        "currentReducers": inventory["current"].as_array().map(|a| a.len()).unwrap_or(0),
        "rejectedUnprovenNativeImport": true, "rejectedHistoricalBaseResurrection": true,
        "actualOverlayCash": overlay["accounting"]["wallets"]["1000"]["cash"],
        "actualOverlayRealizedPnl": overlay["accounting"]["wallets"]["1000"]["realizedPnl"],
        "nativeImportProven": false, "cutoverReady": false, "runtimeChanges": 0,
        "accountRequests": 0, "competitionOrders": 0});
    std::fs::write("data/paper-migration-rejection-proof-20261006.json",
        format!("{}\n", serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    Ok(out)
}
