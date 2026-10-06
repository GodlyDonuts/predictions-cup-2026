// data/prepare-frozen-comparison-pause-20261006.mjs — record the frozen
// settlement-portfolio-v2 pause snapshot and manifest, then verify via the
// pause adapter.
use crate::frozen_paper_pause_adapter_v1::read_frozen_paper_pause;
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::journal::journal_lines;
use baal_service_runtime::journal::JournalReadOptions;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::path::Path;

fn assert_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assert".into()) }
}
pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let read = |p: &str| read_json(Path::new(p));
    let stop = read(&format!("{rt}/COLLECTOR_STOP"))?;
    assert_eq(&stop["source"], &json!("owned-no-profit-host-install-20261006"))?;
    if pid_alive(stop["oldPid"].as_i64().unwrap_or(0)) {
        return Err("OLD_COLLECTOR_PRESENT".into());
    }
    let pointer_file = format!("{rt}/settlement-portfolio-v2-active.json");
    let pointer = read(&pointer_file)?;
    let cohort = pointer["cohort"].as_str().unwrap_or("");
    let dir = format!("{rt}/settlement-portfolio-v2-cohorts/{cohort}");
    if Path::new(&format!("{dir}/worker.lock")).exists() { return Err("assert".into()); }
    let journal_file = format!("{dir}/journal.jsonl");
    let report = read(&format!("{dir}/stats.json"))?;
    assert_eq(&json!(report["unknownAppendAcceptance"] == json!(true)), &json!(false))?;
    assert_eq(&json!(report["failed"] == json!(true)), &json!(false))?;
    let mut last = V::Null;
    let mut h = Sha256::new();
    for line in journal_lines(Path::new(&journal_file), &JournalReadOptions::default())? {
        h.update(format!("{line}\n").as_bytes());
        last = serde_json::from_str(&line).map_err(|e| e.to_string())?;
    }
    if last["state"]["paper"].is_null() { return Err("assert".into()); }
    assert_eq(&report["journalSequence"], &last["sequence"])?;
    assert_eq(&report["journalHash"], &last["hash"])?;
    let prefix_hash = format!("{:x}", h.finalize());
    let journal_anchor = json!({
        "bytes": std::fs::metadata(&journal_file).map_err(|e| e.to_string())?.len(),
        "prefixHash": prefix_hash, "sequence": last["sequence"], "hash": last["hash"],
    });
    let source_anchor = json!({
        "bytes": last["state"]["sourceBytes"], "prefixHash": last["state"]["sourcePrefixHash"],
        "sequence": last["state"]["sourceSequence"], "hash": last["state"]["sourceHash"],
    });
    let saved_at = ms_to_iso(now_ms() as f64);
    let kind = "settlement_portfolio_v2";
    let snapshot = json!({
        "schemaVersion": 1, "kind": kind, "cohort": pointer["cohort"],
        "bankHash": pointer["bankHash"],
        "bankFileHash": sha(&std::fs::read(format!("{dir}/bank.json")).map_err(|e| e.to_string())?),
        "pointerHash": sha(&std::fs::read(&pointer_file).map_err(|e| e.to_string())?),
        "savedAt": saved_at, "paperState": last["state"]["paper"], "report": report,
        "journalAnchor": journal_anchor, "sourceAnchor": source_anchor,
    });
    let snapshot_file = "frozen-engineering-pause.json";
    if Path::new(&format!("{dir}/{snapshot_file}")).exists() { return Err("assert".into()); }
    atomic_json(Path::new(&format!("{dir}/{snapshot_file}")), &snapshot)?;
    let pause_record = json!({
        "schemaVersion": 1, "owner": "Codex", "cohort": pointer["cohort"],
        "bankHash": pointer["bankHash"],
        "reason": "Prioritize official bot execution; preserve counterfactual inventory and bypass expensive frozen economic replay",
        "policy": "pause_retain_inventory", "createdAt": saved_at,
        "bankPreserved": true, "journalPreserved": true, "sourcePreserved": true,
        "resumeRequiresFullOriginalReconciliation": true,
        "reportHash": payload_hash(&report), "journalAnchor": journal_anchor,
        "sourceAnchor": source_anchor,
    });
    let manifest = json!({
        "schemaVersion": 1, "owner": "Codex",
        "entries": [{
            "kind": kind, "cohort": pointer["cohort"], "snapshotFile": snapshot_file,
            "snapshotFileHash": sha(&std::fs::read(format!("{dir}/{snapshot_file}")).map_err(|e| e.to_string())?),
            "pauseRecord": pause_record,
        }],
    });
    if Path::new(&format!("{rt}/paper-comparison-pauses.json")).exists() { return Err("assert".into()); }
    atomic_json(Path::new(&format!("{rt}/paper-comparison-pauses.json")), &manifest)?;
    let checked = read_frozen_paper_pause(
        Path::new(&rt), kind, "settlement-portfolio-v2-active.json",
        "settlement-portfolio-v2-cohorts", &saved_at,
    )?.ok_or("assert")?;
    assert_eq(&checked.fields["paused"], &json!(true))?;
    atomic_json(Path::new("data/frozen-comparison-pause-20261006.json"), &json!({
        "schemaVersion": 1, "observedAt": saved_at, "cohort": pointer["cohort"],
        "bankHash": pointer["bankHash"], "pauseHash": checked.fields["pauseHash"],
        "retainedRisk": checked.fields["retainedRisk"],
        "valuationStatus": checked.fields["valuationStatus"],
        "journalAnchor": journal_anchor, "sourceAnchor": source_anchor,
        "officialReconciliationAffected": false, "accountingReset": false, "verified": true,
    }))?;
    Ok(json!({
        "cohort": pointer["cohort"], "paused": true,
        "retainedRisk": checked.fields["retainedRisk"], "officialAccountUnaffected": true,
    }))
}
fn pid_alive(pid: i64) -> bool {
    if pid <= 0 { return false; }
    std::process::Command::new("kill").arg("-0").arg(pid.to_string())
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .status().map(|s| s.success()).unwrap_or(false)
}
