// execution/prepare-baal-generic-live-v2.mjs — explicit same-owner engineering
// migration; no API, key creation or restart. Exported function only (the CLI
// entry in the .mjs is a thin argv wrapper, no sibling pattern).
use crate::baal_custody_frontier_v1::read_baal_custody_frontier_v1;
use crate::baal_generic_custody_seed_v1::build_baal_generic_custody_seed_v1;
use crate::baal_generic_live_host_v2::make_baal_generic_live_request_v2;
use crate::baal_generic_prior_custody_v1::verify_baal_generic_prior_custody_v1;
use crate::breadth::validate_universe;
use crate::core::payload_hash;
use baal_service_runtime::fixed::instant;
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Value as V};
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("PREPARE_GENERIC_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
fn codeless(e: String) -> String {
    format!("\u{0}{e}")
}
fn instant_v(v: &V) -> f64 {
    instant(v.as_str().unwrap_or(""))
        .map(|m| m as f64)
        .unwrap_or(f64::NAN)
}

// /^(?:"[^"]*[\\/]node\.exe"|node(?:\.exe)?)\s+/i on commandLine
fn starts_with_node_exe(cl: &str) -> bool {
    let s = cl.trim_end();
    let lowered: String = s.chars().flat_map(|c| c.to_lowercase()).collect();
    if let Some(rest) = lowered.strip_prefix('"') {
        // "..."node.exe" — quoted path ending in /node.exe or \node.exe
        if let Some(end) = rest.find('"') {
            let inside = &rest[..end];
            if (inside.ends_with("/node.exe") || inside.ends_with("\\node.exe"))
                && rest[end + 1..].starts_with(|c: char| c.is_whitespace())
            {
                return true;
            }
        }
        return false;
    }
    let first = lowered.split_whitespace().next().unwrap_or("");
    first == "node" || first == "node.exe"
}
// /live-paper[\\/]runner\.mjs/ on commandLine
fn has_live_paper_runner(cl: &str) -> bool {
    cl.contains("live-paper/runner.mjs") || cl.contains("live-paper\\runner.mjs")
}

/// prepareBaalGenericLiveV2({runtimeDir,workspace,epoch,startedAt,
///   processSnapshot,now=Date.now()})
pub fn prepare_baal_generic_live_v2(
    runtime_dir: &Path,
    workspace: &Path,
    epoch: &str,
    started_at: &str,
    process_snapshot: &V,
    now: f64,
) -> Result<V, String> {
    need(
        runtime_dir.is_absolute()
            && workspace.is_absolute()
            && instant_v(&json!(started_at)).is_finite()
            && instant_v(&json!(started_at)) <= now
            && now - instant_v(&json!(started_at)) <= 300000.0,
        "FRESH_ROOT_CLOCK",
    )?;
    let stop: V = serde_json::from_str(
        &fs::read_to_string(runtime_dir.join("COLLECTOR_STOP"))
            .map_err(|e| codeless(e.to_string()))?,
    )
    .map_err(|e| codeless(e.to_string()))?;
    let snap = process_snapshot
        .as_array()
        .cloned()
        .unwrap_or_default();
    need(
        sget(&stop, "kind") == Some("baal_root_controlled_engineering_stop_v1")
            && sget(&stop, "ownerId") == Some("Codex")
            && get(&stop, "humanStop") == &V::Bool(false)
            && sget(&stop, "reason") == Some("generic_v2_upgrade")
            && process_snapshot.is_array()
            && !snap.iter().any(|p| get(p, "pid") == get(&stop, "oldPid")),
        "ROOT_STOP_AND_OLD_PID",
    )?;
    need(
        !snap.iter().any(|p| {
            let cl = sget(p, "commandLine").unwrap_or("");
            starts_with_node_exe(cl) && has_live_paper_runner(cl)
        }),
        "ANOTHER_COLLECTOR",
    )?;
    need(
        !runtime_dir.join("EXECUTION_STOP").exists(),
        "HUMAN_OR_FINANCIAL_STOP",
    )?;
    let frontier = read_baal_custody_frontier_v1(runtime_dir, workspace)?;
    for relative in [
        "baal-financial-owner/execution-owner.lock".to_string(),
        format!(
            "baal-combined-custody-runs/{}/{}/coordinator/execution-owner.lock",
            sget(get(get(&frontier, "currentState"), "scope"), "runId").unwrap_or(""),
            sget(get(&frontier, "migration"), "implementationEpoch").unwrap_or(""),
        ),
        format!(
            "baal-combined-custody-runs/{}/{}/budget/execution-owner.lock",
            sget(get(get(&frontier, "currentState"), "scope"), "runId").unwrap_or(""),
            sget(get(&frontier, "migration"), "implementationEpoch").unwrap_or(""),
        ),
        "competition-runs/sig-pilot-median-20261006-human/execution-owner.lock".to_string(),
    ] {
        need(
            !runtime_dir.join(relative).exists(),
            "PRIOR_WRITER_LEASE",
        )?;
    }
    let universe = validate_universe(
        &serde_json::from_str(
            &fs::read_to_string(workspace.join("live-paper/price-universe.json"))
                .map_err(|e| codeless(e.to_string()))?,
        )
        .map_err(|e| codeless(e.to_string()))?,
        sget(get(get(&frontier, "currentState"), "scope"), "tournamentId")
            .unwrap_or(""),
    )?;
    let entry_iso = baal_service_runtime::contract::ms_to_iso(
        instant_v(&json!(started_at)) + 3600000.0,
    );
    let request = make_baal_generic_live_request_v2(
        workspace,
        &universe,
        &frontier,
        epoch,
        started_at,
        &entry_iso,
    )?;
    let target: PathBuf = runtime_dir.join("baal-generic-live-request-v2.json");
    need(!target.exists(), "NEW_REQUEST_ONLY")?;
    let prior = verify_baal_generic_prior_custody_v1(
        runtime_dir,
        &build_baal_generic_custody_seed_v1(
            &frontier,
            get(&request, "scope"),
            get(&request, "allocation"),
        )?,
    )?;
    need(
        prior.is_some() && js_int(get(prior.as_ref().unwrap(), "newOrders")) == Some(0),
        "PRIOR_GENERIC_REPLAY",
    )?;
    for relative in ["controller", "budget"] {
        need(
            !runtime_dir
                .join("baal-generic-live-runs")
                .join(sget(&request, "runId").unwrap_or(""))
                .join(sget(prior.as_ref().unwrap(), "epoch").unwrap_or(""))
                .join(relative)
                .join("execution-owner.lock")
                .exists(),
            "PRIOR_GENERIC_LEASE",
        )?;
    }
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&target)
        .map_err(|e| codeless(e.to_string()))?;
    f.write_all(
        (serde_json::to_string_pretty(&request).unwrap_or_default() + "\n").as_bytes(),
    )
    .map_err(|e| codeless(e.to_string()))?;
    f.sync_all().map_err(|e| codeless(e.to_string()))?;
    drop(f);
    Ok(json!({
        "target": target.to_string_lossy(),
        "priorCustody": prior,
        "requestHash": payload_hash(&request)?,
        "epoch": epoch,
        "sourceActorAnchor": get(&frontier, "currentActorAnchor"),
        "inheritedSummary": get(&frontier, "summary"),
        "entryExpiresAt": get(get(&request, "activation"), "entryExpiresAt"),
        "originalExitExpiresAt": get(get(&request, "activation"), "exitExpiresAt"),
        "marketScope": get(get(&request, "scope"), "exchanges").as_array().map(|a| a.len()).unwrap_or(0),
        "networkRequests": 0,
    }))
}
