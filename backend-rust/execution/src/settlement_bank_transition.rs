// execution/settlement-bank-transition.mjs — an engineering transition changes
// one frozen-bank pointer. The same run, activation window, execution journal,
// inventory and cumulative purchases stay.
use crate::core::{payload_hash, validate_execution_state};
use crate::production::production_handoff_current;
use crate::settlement::validate_settlement_bank;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{digest, js_stringify};
use baal_service_runtime::ledger::replay_journal;
use serde_json::{json, Map, Value as V};
use std::fs;
use std::io::Write;
use std::path::Path;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("SETTLEMENT_TRANSITION_{code}")) }
}
fn parse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}
fn immutable_candidate(c: &V) -> V {
    let mut out = Map::new();
    for k in ["id", "marketId", "exchangeId", "side", "predicateHash", "riskGroups", "settlesAtMs"] {
        out.insert(k.to_string(), c.get(k).cloned().unwrap_or(V::Null));
    }
    V::Object(out)
}

/// validateSettlementTransition({activation,previousBank,nextBank,universe,intents={},owned={},nowMs})
pub fn validate_settlement_transition(
    activation: &V,
    previous_bank: &V,
    next_bank: &V,
    universe: &V,
    intents: &V,
    owned: &V,
    now_ms: f64,
) -> Result<V, String> {
    need(
        now_ms.is_finite()
            && now_ms.fract() == 0.0
            && now_ms.abs() <= 9_007_199_254_740_991.0
            && now_ms >= parse(&activation["startedAt"])
            && now_ms < parse(&activation["entryExpiresAt"]),
        "WINDOW",
    )?;
    validate_settlement_bank(previous_bank, activation, universe)?;
    let next_bank_hash = digest(next_bank);
    need(
        Some(next_bank_hash.as_str()) != activation.get("strategyBankHash").and_then(V::as_str),
        "UNCHANGED_BANK",
    )?;
    let mut next_activation = activation.as_object().cloned().unwrap_or_default();
    next_activation.insert("strategyBankHash".into(), json!(next_bank_hash));
    let next_activation = V::Object(next_activation);
    validate_settlement_bank(next_bank, &next_activation, universe)?;
    need(
        payload_hash(&previous_bank["nativeBackend"])? == payload_hash(&next_bank["nativeBackend"])?
            && payload_hash(&previous_bank["policy"])? == payload_hash(&next_bank["policy"])?,
        "POLICY_OR_ENGINE_CHANGED",
    )?;
    let intents_obj = intents.as_object().cloned().unwrap_or_default();
    need(
        intents_obj.values().all(|i| i.get("status").and_then(V::as_str) == Some("closed")),
        "UNRESOLVED_INTENT",
    )?;
    let empty: Vec<V> = Vec::new();
    for previous in previous_bank.get("candidates").and_then(V::as_array).unwrap_or(&empty) {
        let next = next_bank
            .get("candidates")
            .and_then(V::as_array)
            .and_then(|a| a.iter().find(|c| c.get("id") == previous.get("id")));
        need(
            next.is_some()
                && payload_hash(&immutable_candidate(next.unwrap()))? == payload_hash(&immutable_candidate(previous))?,
            "EXISTING_CANDIDATE_CHANGED",
        )?;
        let market_id = previous.get("marketId").and_then(V::as_str).unwrap_or("");
        // `contracts[marketId].rootHash` on a missing entry is a JS TypeError.
        let next_root = next_bank
            .pointer(&format!("/contracts/{market_id}/rootHash"))
            .ok_or_else(|| "Cannot read properties of undefined (reading 'rootHash')".to_string())?;
        let previous_root = previous_bank
            .pointer(&format!("/contracts/{market_id}/rootHash"))
            .ok_or_else(|| "Cannot read properties of undefined (reading 'rootHash')".to_string())?;
        need(next_root == previous_root, "EXISTING_CONTRACT_CHANGED")?;
    }
    for (exchange_id, p) in owned.as_object().cloned().unwrap_or_default() {
        need(
            next_bank.get("candidates").and_then(V::as_array).is_some_and(|a| {
                a.iter().any(|c| {
                    c.get("exchangeId").and_then(V::as_str) == Some(exchange_id.as_str())
                        && c.get("marketId") == p.get("marketId")
                })
            }),
            "OWNED_SCOPE_MISSING",
        )?;
    }
    Ok(next_activation)
}

fn open_exclusive(file: &Path) -> Result<fs::File, String> {
    let mut o = fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(file).map_err(|e| e.to_string())
}
fn write_exclusive(file: &Path, body: &str) -> Result<(), String> {
    let mut fd = open_exclusive(file)?;
    let result = fd.write_all(body.as_bytes()).and_then(|_| fd.sync_all());
    drop(fd);
    result.map_err(|e| e.to_string())
}
fn read_json_file(file: &Path) -> Result<V, String> {
    serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// applyStoppedSettlementTransition({runtimeDir,collectorLockFile,nextBank,universe,now=Date.now})
/// The collector must be absent and have released both runtime leases first.
pub fn apply_stopped_settlement_transition(
    runtime_dir: &Path,
    collector_lock_file: &Path,
    next_bank: &V,
    universe: &V,
    now: &dyn Fn() -> f64,
) -> Result<V, String> {
    let guard = runtime_dir.join("settlement-bank-transition.lock");
    let guard_fd = open_exclusive(&guard)?;
    let result = apply_inner(runtime_dir, collector_lock_file, next_bank, universe, now);
    drop(guard_fd);
    // finally{fs.closeSync(fd);fs.unlinkSync(guard);} — an unlink failure replaces
    // the inner outcome exactly as a finally-block throw does in JS.
    fs::remove_file(&guard).map_err(|e| e.to_string())?;
    result
}

fn apply_inner(
    runtime_dir: &Path,
    collector_lock_file: &Path,
    next_bank: &V,
    universe: &V,
    now: &dyn Fn() -> f64,
) -> Result<V, String> {
    let file = runtime_dir.join("competition-activation.json");
    let before_bytes = fs::read(&file).map_err(|e| e.to_string())?;
    let activation: V = serde_json::from_slice(&before_bytes).map_err(|e| e.to_string())?;
    let dir = runtime_dir.join("competition-runs").join(activation["runId"].as_str().unwrap_or(""));
    let stop_file = runtime_dir.join("COLLECTOR_STOP");
    let stop_bytes = fs::read(&stop_file).map_err(|e| e.to_string())?;
    let stop: V = serde_json::from_slice(&stop_bytes).map_err(|e| e.to_string())?;
    need(
        stop.get("source").and_then(V::as_str) == Some("settlement_bank_transition")
            && stop.get("nextBankHash").and_then(V::as_str) == Some(digest(next_bank).as_str()),
        "ENGINEERING_STOP_REQUIRED",
    )?;
    need(
        !collector_lock_file.exists() && !dir.join("execution-owner.lock").exists(),
        "WRITER_PRESENT",
    )?;
    need(
        !runtime_dir.join("EXECUTION_STOP").exists() && !runtime_dir.join("STOP").exists(),
        "STOP",
    )?;
    need(production_handoff_current(runtime_dir, &activation), "HANDOFF")?;
    let previous_bank = read_json_file(
        &runtime_dir.join("production-banks").join(format!(
            "{}.json",
            activation["strategyBankHash"].as_str().unwrap_or("")
        )),
    )?;
    let core_file = dir.join("core-journal.jsonl");
    let intent_file = dir.join("execution-journal.jsonl");
    let core_journal = if core_file.exists() { Some(Journal::open(&core_file)?) } else { None };
    let core = core_journal
        .as_ref()
        .and_then(|j| j.last.as_ref())
        .and_then(|b| b.get("state"))
        .cloned();
    if let Some(c) = &core {
        validate_execution_state(c)?;
    }
    let raw = if intent_file.exists() {
        fs::read_to_string(&intent_file).map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    let ledger = replay_journal(&raw, activation.get("tournamentId").and_then(V::as_str))
        .map_err(|e| e.message)?;
    need(
        ledger.get("intents").and_then(V::as_object).is_some_and(|m| {
            m.values().all(|i| i.get("status").and_then(V::as_str) == Some("closed"))
        }),
        "LEDGER_UNRESOLVED",
    )?;
    let core_intents = core
        .as_ref()
        .and_then(|c| c.get("intents"))
        .and_then(V::as_object)
        .cloned()
        .unwrap_or_default();
    need(
        ledger.get("intents").and_then(V::as_object).map(|m| m.len()).unwrap_or(0) == core_intents.len()
            && ledger
                .get("intents")
                .and_then(V::as_object)
                .map(|m| {
                    m.keys().all(|id| {
                        core_intents.get(id).and_then(|i| i.get("status")).and_then(V::as_str) == Some("closed")
                    })
                })
                .unwrap_or(false),
        "CORE_LEDGER_MISMATCH",
    )?;
    let empty = json!({});
    let next_activation = validate_settlement_transition(
        &activation,
        &previous_bank,
        next_bank,
        universe,
        core.as_ref().and_then(|c| c.get("intents")).unwrap_or(&empty),
        core.as_ref().and_then(|c| c.get("owned")).unwrap_or(&empty),
        now(),
    )?;
    let next_file = runtime_dir.join("production-banks").join(format!(
        "{}.json",
        next_activation["strategyBankHash"].as_str().unwrap_or("")
    ));
    if next_file.exists() {
        need(
            digest(&read_json_file(&next_file)?)
                == next_activation["strategyBankHash"].as_str().unwrap_or(""),
            "NEXT_BANK_FILE",
        )?;
    } else {
        write_exclusive(&next_file, &format!("{}\n", js_stringify(next_bank)))?;
    }
    let transition_dir = runtime_dir.join("production-bank-transitions");
    if !transition_dir.exists() {
        fs::create_dir_all(&transition_dir).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&transition_dir, fs::Permissions::from_mode(0o700));
        }
    }
    let record = json!({
        "schemaVersion": 1,
        "at": ms_to_iso(now()),
        "runId": activation.get("runId").cloned().unwrap_or(V::Null),
        "previousBankHash": activation.get("strategyBankHash").cloned().unwrap_or(V::Null),
        "nextBankHash": next_activation.get("strategyBankHash").cloned().unwrap_or(V::Null),
        "previousActivationHash": payload_hash(&activation)?,
        "nextActivationHash": payload_hash(&next_activation)?,
        "previousActivation": activation,
        "nextActivation": next_activation,
        "coreSequence": core_journal.as_ref().map(|j| j.sequence).unwrap_or(0),
        "coreHash": core_journal.as_ref().map(|j| j.previous.clone()).unwrap_or_default(),
        "coreIntents": core_intents.len(),
        "ledgerSequence": ledger.get("sequence").cloned().unwrap_or(V::Null),
        "ledgerHash": ledger.get("lastHash").cloned().unwrap_or(V::Null),
        "runChanged": false,
        "windowChanged": false,
        "policyChanged": false,
        "accountingReset": false,
        "networkRequests": 0,
        "ordersSubmitted": 0,
    });
    let record_hash = payload_hash(&record)?;
    let record_file = transition_dir.join(format!("{record_hash}.json"));
    write_exclusive(&record_file, &format!("{}\n", js_stringify(&record)))?;
    {
        let fd = fs::OpenOptions::new().read(true).write(true).open(&record_file).map_err(|e| e.to_string())?;
        fd.sync_all().map_err(|e| e.to_string())?;
    }
    // An interrupted pointer swap leaves the old activation valid and a prepared
    // immutable record. Never reset a run to recover from a partial transition.
    need(
        fs::read(&file).map_err(|e| e.to_string())? == before_bytes
            && fs::read(&stop_file).map_err(|e| e.to_string())? == stop_bytes
            && !collector_lock_file.exists()
            && !dir.join("execution-owner.lock").exists()
            && !runtime_dir.join("EXECUTION_STOP").exists()
            && !runtime_dir.join("STOP").exists(),
        "CONCURRENT_CHANGE",
    )?;
    atomic_json_canonical(&file, &next_activation)?;
    need(
        payload_hash(&read_json_file(&file)?)?
            == record.get("nextActivationHash").and_then(V::as_str).unwrap_or(""),
        "READBACK",
    )?;
    Ok(json!({
        "recordFile": record_file.to_string_lossy(),
        "recordHash": record_hash,
        "nextActivation": next_activation,
    }))
}
