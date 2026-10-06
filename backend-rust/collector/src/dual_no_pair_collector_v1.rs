// live-paper/dual-no-pair-collector-v1.mjs — collector-side dual-NO host binding.
use baal_execution::core::payload_hash;
use serde_json::Value as V;
use std::path::Path;
use std::sync::Arc;

fn cohort_ok(cohort: &V) -> bool {
    cohort.as_str().is_some_and(|s| {
        let b = s.as_bytes();
        b.len() >= 2 && b.len() <= 64 && b[0].is_ascii_lowercase() && b[1..].iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
    })
}
fn hex64(v: &V) -> bool { v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))) }

/// openCollectorDualNoPaper({runtimeDir,universe,canOperate}) — synchronous.
/// `workspace_dir` is the source root used for the code-identity hashes (replaces import.meta.url).
pub fn open_collector_dual_no_paper(workspace_dir: &Path, runtime_dir: &Path, universe: &V,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>) -> Result<Option<crate::dual_no_pair_host_v1::DualNoPaperHost>, String> {
    let file = runtime_dir.join("dual-no-pair-active.json");
    if !file.exists() { return Ok(None); }
    let p: V = serde_json::from_str(&std::fs::read_to_string(&file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if p["schemaVersion"].as_i64() != Some(1) || !cohort_ok(&p["cohort"]) || !hex64(&p["bankHash"]) {
        return Err("DUAL_NO_COLLECTOR_POINTER".into());
    }
    let cohort = p["cohort"].as_str().unwrap();
    let dir = runtime_dir.join("dual-no-pair-cohorts").join(cohort);
    let bank: V = serde_json::from_str(&std::fs::read_to_string(dir.join("bank.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if payload_hash(&bank)? != p["bankHash"].as_str().unwrap_or("")
        || bank["review"]["tournamentId"] != universe["tournamentId"]
        || bank["review"]["legs"].as_array().cloned().unwrap_or_default().iter().any(|l|
            !universe["exchanges"].as_array().cloned().unwrap_or_default().iter()
                .any(|x| x["marketId"] == l["marketId"] && x["exchangeId"] == l["exchangeId"] && x["option"].as_str() == Some("YES"))) {
        return Err("DUAL_NO_COLLECTOR_SCOPE".into());
    }
    if dir.join("STOP").exists() || dir.join("OBSERVER_STOP").exists() {
        return Err("DUAL_NO_COLLECTOR_STOPPED_RECONCILE_INVENTORY".into());
    }
    let _module = crate::frozen_paper_runtime::frozen_paper_module(runtime_dir, &bank, "live-paper/dual-no-pair-host-v1.mjs")?;
    crate::dual_no_pair_host_v1::open_dual_no_paper_host(workspace_dir, runtime_dir, cohort, false, None,
        Arc::new(crate::dual_no_pair_host_v1::system_now), can_operate).map(Some)
}
