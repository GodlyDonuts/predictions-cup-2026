// execution/prepare-baal-combined-read-v7.mjs — root-controlled engineering
// upgrade. Reuses every original economic identity and deadline; this tool has
// no client and does not create an activation.
use crate::baal_combined_custody_host_v6::baal_combined_custody_v6_identity;
use crate::baal_combined_custody_host_v7::baal_combined_custody_v7_identity;
use crate::core::payload_hash;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use serde_json::{json, Value as V};
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("READ_UPGRADE_V7_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}

/// writeFileSync(target, JSON.stringify(upgrade,null,2)+'\n', {flag:'wx',
/// mode:0o600}) + fsync — exclusive create, owner-only mode.
fn write_exclusive(path: &Path, body: &V) -> Result<(), String> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("READ_UPGRADE_V7_IO:{e}"))?;
    f.write_all(
        (serde_json::to_string_pretty(body).unwrap_or_default() + "\n").as_bytes(),
    )
    .map_err(|e| format!("READ_UPGRADE_V7_IO:{e}"))?;
    f.sync_all().map_err(|e| format!("READ_UPGRADE_V7_IO:{e}"))?;
    Ok(())
}

/// prepareBaalCombinedReadV7({runtimeDir,workspace,recordedAt})
pub fn prepare_baal_combined_read_v7(
    runtime_dir: &Path,
    workspace: &Path,
    recorded_at: Option<&str>,
) -> Result<V, String> {
    need(
        runtime_dir.is_absolute() && workspace.is_absolute(),
        "ABSOLUTE_PATHS",
    )?;
    let recorded = recorded_at
        .map(String::from)
        .unwrap_or_else(|| ms_to_iso(chrono::Utc::now().timestamp_millis() as f64));
    let d: V = serde_json::from_slice(
        &fs::read(runtime_dir.join("baal-combined-custody-request-v6.json"))
            .map_err(|e| format!("READ_UPGRADE_V7_IO:{e}"))?,
    )
    .map_err(|e| format!("READ_UPGRADE_V7_IO:{e}"))?;
    need(
        sget(&d, "kind") == Some("baal_combined_custody_host_v6")
            && sget(&d, "ownerId") == Some("Codex")
            && payload_hash(get(&d, "codeIdentity")).ok()
                == payload_hash(&baal_combined_custody_v6_identity(workspace)?).ok(),
        "ORIGINAL_DESCRIPTOR_GRAPH",
    )?;
    let recorded_ms = parse_iso_ms(&recorded);
    need(
        recorded_ms.is_some()
            && recorded_ms.unwrap_or(f64::NAN) >= parse_iso_ms(sget(&d, "recordedAt").unwrap_or("")).unwrap_or(f64::NAN),
        "CLOCK",
    )?;
    let epoch_dir = runtime_dir
        .join("baal-combined-custody-runs")
        .join(sget(&d, "runId").unwrap_or(""))
        .join(sget(get(&d, "migration"), "implementationEpoch").unwrap_or(""));
    for dir in [
        runtime_dir.join("baal-financial-owner"),
        epoch_dir.join("coordinator"),
        epoch_dir.join("budget"),
    ] {
        need(
            !dir.join("execution-owner.lock").exists(),
            "ACTIVE_WRITER",
        )?;
    }
    need(
        !runtime_dir.join("EXECUTION_STOP").exists(),
        "EXECUTION_STOP",
    )?;
    need(
        runtime_dir.join("COLLECTOR_STOP").exists(),
        "ENGINEERING_STOP_REQUIRED",
    )?;
    let stop: V = serde_json::from_slice(
        &fs::read(runtime_dir.join("COLLECTOR_STOP"))
            .map_err(|e| format!("READ_UPGRADE_V7_IO:{e}"))?,
    )
    .map_err(|e| format!("READ_UPGRADE_V7_IO:{e}"))?;
    need(
        sget(&stop, "kind") == Some("baal_root_controlled_engineering_stop_v1")
            && sget(&stop, "ownerId") == Some("Codex")
            && get(&stop, "humanStop") == &V::Bool(false)
            && sget(&stop, "reason") == Some("combined_read_v7_upgrade")
            && sget(&stop, "runId") == sget(&d, "runId"),
        "ENGINEERING_STOP_PROVENANCE",
    )?;
    let target = runtime_dir.join("baal-combined-read-upgrade-v7.json");
    need(!target.exists(), "NEW_UPGRADE_ONLY")?;
    let upgrade = json!({
        "schemaVersion": 1,
        "kind": "baal_combined_read_upgrade_v7",
        "ownerId": get(&d, "ownerId"),
        "runId": get(&d, "runId"),
        "tournamentId": get(&d, "tournamentId"),
        "originalDescriptorHash": payload_hash(&d)?,
        "codeIdentity": baal_combined_custody_v7_identity(workspace)?,
        "recordedAt": recorded,
    });
    write_exclusive(&target, &upgrade)?;
    Ok(json!({
        "target": target,
        "upgradeHash": payload_hash(&upgrade)?,
        "originalDescriptorHash": get(&upgrade, "originalDescriptorHash"),
        "originalHedgeExpiresAt": get(get(&d, "activation"), "hedgeExpiresAt"),
        "originalExitExpiresAt": get(get(&d, "activation"), "exitExpiresAt"),
        "networkRequests": 0,
    }))
}
