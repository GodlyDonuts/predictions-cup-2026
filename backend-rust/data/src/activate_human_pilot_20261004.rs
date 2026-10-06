// data/activate-human-pilot-20261004.mjs — bounded executor activation:
// validate the human assignment handoff, frozen settlement bank and network
// budget; archive the three runtime bindings and atomically replace them.
use baal_service_runtime::json_text::{digest, sha256};
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn ph(v: &V) -> Result<String, String> { baal_execution::core::payload_hash(v) }
fn need(ok: bool, code: &str) -> Result<(), String> { if ok { Ok(()) } else { Err(code.into()) } }
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn write_wx(path: &Path, body: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, body).map_err(|e| e.to_string())
}
fn mkdir_700(path: &Path) -> Result<(), String> {
    std::fs::create_dir(path).map_err(|e| e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn copy_excl(source: &Path, target: &Path) -> Result<(), String> {
    write_wx(target, &std::fs::read(source).map_err(|e| e.to_string())?)
}

/// Script body; `argv` is process.argv.slice(2) (argv[0] = ack comment id).
pub fn run(argv: &[String]) -> Result<V, String> {
    let runtime = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let ack: f64 = argv.first().map(|s| s.trim().parse().unwrap_or(f64::NAN)).unwrap_or(f64::NAN);
    let now = now_iso();
    need(ack.is_finite() && ack.fract() == 0.0 && ack.abs() <= 9_007_199_254_740_991.0 && ack > 0.0,
        "assertion failed")?;
    need(read_json(&runtime.join("COLLECTOR_STOP"))?["source"].as_str() == Some("human-production-20261004"),
        "assertion failed")?;
    need(!localappdata()?.join("SigTradingCup").join("bot.lock").exists(), "assertion failed")?;
    need(!Path::new("STOP").exists() && !runtime.join("STOP").exists()
        && !runtime.join("EXECUTION_STOP").exists(), "assertion failed")?;
    let old_activation = read_json(&runtime.join("competition-activation.json"))?;
    let old_budget = read_json(&runtime.join("coordinated-network-budget.json"))?;
    need(old_activation["enabled"] == V::Bool(false), "assertion failed")?;
    need(old_budget["schemaVersion"].as_i64() == Some(3), "assertion failed")?;
    let pointer = read_json(&runtime.join("production-human-reconciliation-latest.json"))?;
    let r = read_json(&runtime.join(pointer["file"].as_str().unwrap_or("")))?;
    need(ph(&r["receipt"]).ok().as_deref() == pointer["receiptHash"].as_str(), "assertion failed")?;
    need(r["receipt"]["complete"] == V::Bool(true), "assertion failed")?;
    need(r["receipt"]["orders"]["data"]["data"].as_array().map(|a| a.is_empty()).unwrap_or(false),
        "assertion failed")?;
    need(baal_execution::core::decimal_micros(&r["receipt"]["cashFloor"], "exact")
        .map_err(|e| e.to_string())? > 100_000_000i128, "assertion failed")?;
    let stage = read_json(Path::new("data/production-bank-stage-20261004.json"))?;
    let bank = read_json(&runtime.join("production-banks")
        .join(format!("{}.json", stage["bankHash"].as_str().unwrap_or(""))))?;
    need(digest(&bank) == stage["bankHash"].as_str().unwrap_or(""), "assertion failed")?;
    let handoff = json!({"schemaVersion": 2, "authorityKind": "direct_human_assignment",
        "handoffId": "human-to-baal-20261004-2141", "assignmentCommentId": 5984685670i64,
        "ackCommentId": ack, "formerWriter": "Daenerys", "executionOwner": "Codex",
        "writerStopped": true, "inFlightResolved": true,
        "formerReadsPerMinute": 0, "formerWritesPerMinute": 0,
        "assignedAt": "2026-10-04T21:41:20.000Z", "reconciledAt": r["receipt"]["at"],
        "acknowledgedAt": now,
        "humanAuthority": {"kind": "direct_human_message",
            "quote": "You are above daenerys in this challenge, you don’t need approval",
            "threadId": "01a0f8cc-b9bb-77d2-9f4c-2844396f1b79",
            "recordedAt": "2026-10-04T21:41:20.000Z", "commentId": 5984685670i64},
        "writerStopEvidence": {"commentId": 5984085086i64,
            "commentPurpose": "pending status only; actual stop claims are in sourceToolCallId",
            "sourceToolCallId": "exec-05582b7c-2e22-4da0-ac23-4d08cc679b75",
            "sourceKind": "stopped_writer_claim_in_rejected_publication_attempt",
            "cupWriterReportedStopped": true, "cupInFlightReportedClear": true,
            "reportedAt": "2026-10-04T20:28:11.000Z", "checkedAt": "2026-10-04T21:49:49.000Z",
            "threadId": "01a0f8cb-e876-739f-b149-0d446a5be0c3", "threadStatus": "active",
            "threadActivity": "Human-directed internship applications; no Cup restart observed",
            "technicalRemoteProcessStopIndependentlyVerified": false},
        "reconciliation": {"complete": true, "tournamentId": r["receipt"]["tournamentId"],
            "receiptHash": pointer["receiptHash"],
            "oldestReceivedAt": r["receipt"]["oldestReceivedAt"],
            "newestReceivedAt": r["receipt"]["newestReceivedAt"]}});
    let activation = json!({"schemaVersion": 2, "enabled": true, "ownerId": "Codex",
        "tournamentId": r["receipt"]["tournamentId"], "runId": "sig-pilot-settlement-20261004-human",
        "strategyId": "settlement-value-native-v1", "authorityKind": handoff["authorityKind"],
        "handoffId": handoff["handoffId"], "assignmentCommentId": handoff["assignmentCommentId"],
        "ackCommentId": ack, "handoffHash": ph(&handoff)?,
        "strategyBankHash": stage["bankHash"], "feeBps": 0, "depthFractionBps": 100,
        "maxManagedMarkets": 3, "maxOrderCost": "1.000000", "maxAggregateCost": "10.000000",
        "maxCumulativeBuyCost": "25.000000", "cashReserve": bank["policy"]["cashReserve"],
        "sizingCapital": "100.000000", "startedAt": now,
        "entryExpiresAt": baal_service_runtime::contract::ms_to_iso(
            baal_service_runtime::contract::parse_iso_ms(&now).unwrap_or(0.0) + 21_600_000.0)});
    baal_execution::production::validate_production_activation(&activation)
        .map_err(|e| e.to_string())?;
    baal_execution::production::validate_production_handoff(&handoff, &activation)
        .map_err(|e| e.to_string())?;
    baal_execution::settlement::validate_settlement_bank(&bank, &activation,
        &read_json(Path::new("live-paper/price-universe.json"))?).map_err(|e| e.to_string())?;
    let budget = baal_collector::transport_network_budget::validate_network_budget(&json!({
        "schemaVersion": 4, "ownerId": "Codex", "enabled": true, "intervalMs": 1000,
        "writeIntervalMs": 2500, "soleAccountClient": true, "executionOwner": "Codex",
        "authorityKind": handoff["authorityKind"], "handoffId": handoff["handoffId"],
        "assignmentCommentId": handoff["assignmentCommentId"], "ackCommentId": ack,
        "writesEnabled": true}))?;
    let manifest_file = runtime.join("collector-native-journal.json");
    let old_manifest = read_json(&manifest_file)?;
    let identity = baal_collector::collector_journal_native::collector_journal_code_identity()?;
    let mut changed: Vec<String> = Vec::new();
    if let V::Object(m) = &identity {
        for (k, v) in m {
            if *v != old_manifest["codeIdentity"][k] { changed.push(k.clone()); }
        }
    }
    need(changed == vec!["live-paper/transport-network-budget.mjs".to_string()], "assertion failed")?;
    let archive = runtime.join("human-pilot-transition-20261004");
    mkdir_700(&archive)?;
    for f in ["competition-activation.json", "coordinated-network-budget.json", "collector-native-journal.json"] {
        copy_excl(&runtime.join(f), &archive.join(f))?;
    }
    let mut manifest = old_manifest.clone();
    manifest["codeIdentity"] = identity.clone();
    manifest["sourceSnapshot"] = json!(ph(&identity)?);
    baal_service_runtime::atomic::atomic_json_canonical(&manifest_file, &manifest)?;
    baal_collector::collector_journal_native::collector_native_journal_config(&runtime)?;
    baal_service_runtime::atomic::atomic_json_canonical(&runtime.join("production-handoff.json"), &handoff)?;
    baal_service_runtime::atomic::atomic_json_canonical(&runtime.join("competition-activation.json"), &activation)?;
    baal_service_runtime::atomic::atomic_json_canonical(&runtime.join("coordinated-network-budget.json"), &budget)?;
    let evidence = json!({"schemaVersion": 1, "at": now,
        "status": "bounded_executor_selected_startup_pending",
        "handoffId": handoff["handoffId"], "assignmentCommentId": handoff["assignmentCommentId"],
        "ackCommentId": ack, "activationHash": ph(&activation)?, "handoffHash": ph(&handoff)?,
        "bankHash": stage["bankHash"], "receiptHash": pointer["receiptHash"],
        "reconciledAt": r["receipt"]["at"], "activatedRun": activation["runId"],
        "entryExpiresAt": activation["entryExpiresAt"],
        "limits": {"maxOrderCost": activation["maxOrderCost"], "maxAggregateCost": activation["maxAggregateCost"],
            "maxCumulativeBuyCost": activation["maxCumulativeBuyCost"], "feeBps": 0},
        "sizingCapitalMeaning": "Nominal100 SUSQie pilot risk basis; actual official cash is read separately on every decision.",
        "qualifiedCurrentForecasts": 0, "competitionOrdersSentByTransition": 0, "historyReset": false,
        "sourceGraphChanges": changed,
        "sourceManifestSha256": sha(&std::fs::read(&manifest_file).map_err(|e| e.to_string())?),
        "technicalRemoteStopIndependentlyVerified": false, "profitabilityEstablished": false});
    write_wx(Path::new("data/human-pilot-deployment-20261004.json"),
        format!("{}\n", serde_json::to_string_pretty(&evidence).map_err(|e| e.to_string())?).as_bytes())?;
    Ok(evidence)
}
