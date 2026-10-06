// data/check-settlement-value-20261004.mjs — native settlement policy decision
// against the current sealed snapshot (file-only; no writes to paper).
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64
}
fn perf_ms() -> f64 { now_ms() }
fn mkdir_700(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn write_wx(path: &Path, body: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, body).map_err(|e| e.to_string())
}
fn need_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assertion failed".into()) }
}

pub fn run() -> Result<V, String> {
    let runtime = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let active_file = runtime.join("probability-receipt-active.json");
    let active_bytes = std::fs::read(&active_file).map_err(|e| e.to_string())?;
    let active: V = serde_json::from_slice(&active_bytes).map_err(|e| e.to_string())?;
    let cohort_dir = runtime.join("probability-receipt-cohorts")
        .join(active["cohort"].as_str().unwrap_or(""));
    let bank_bytes = std::fs::read(cohort_dir.join("bank.json")).map_err(|e| e.to_string())?;
    let bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
    let stats_bytes = std::fs::read(cohort_dir.join("stats.json")).map_err(|e| e.to_string())?;
    let stats: V = serde_json::from_slice(&stats_bytes).map_err(|e| e.to_string())?;
    need_eq(&stats["bankHash"], &active["bankHash"])?;
    need_eq(&stats["failed"], &json!(false))?;
    let stamp = now_ms() as u64;
    let dir = runtime.join("research-cohorts/settlement-value-diagnostic-20261004")
        .join(stamp.to_string());
    mkdir_700(&dir)?;
    let journal = runtime.join("public-books.jsonl");
    let prefix_len = stats["sourceBytes"].as_u64().unwrap_or(0);
    let mut prefix = vec![0u8; prefix_len as usize];
    {
        let mut fd = std::fs::File::open(&journal).map_err(|e| e.to_string())?;
        fd.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        fd.read_exact(&mut prefix).map_err(|e| e.to_string())?;
    }
    let scope = dir.join("scope.json");
    let out = dir.join("publication.json");
    write_wx(&scope, format!("{}\n", serde_json::to_string(&bank["universe"])
        .map_err(|e| e.to_string())?).as_bytes())?;
    let start = perf_ms();
    let executable = baal_collector::rust_backend::native_executable();
    let binary_sha = sha(&std::fs::read(&executable).map_err(|e| e.to_string())?);
    let publication = baal_collector::rust_backend::read_native_publication_prefix(
        &journal, &scope, prefix_len, &sha(&prefix), &out, &executable, &binary_sha)?;
    need_eq(&publication["state"]["sequence"], &stats["sourceSequence"])?;
    need_eq(&publication["state"]["hash"], &stats["sourceHash"])?;
    let account = read_json(&runtime.join("official-account.json")).ok()
        .and_then(|v| baal_collector::probability_receipt_v2::probability_account_eligibility(&v));
    let input = baal_collector::settlement_value_native::settlement_input_from_probability(
        &bank, &stats, &publication["state"], account.as_ref(),
        baal_service_runtime::contract::parse_iso_ms(
            stats["updatedAt"].as_str().unwrap_or("")).unwrap_or(0.0) as i64)?;
    let native = baal_collector::settlement_value_native::run_settlement_value(
        &input, &dir.join("proposal"), &executable)?;
    need_eq(&json!(sha(&std::fs::read(&active_file).map_err(|e| e.to_string())?)),
        &json!(sha(&active_bytes)))?;
    need_eq(&json!(sha(&std::fs::read(cohort_dir.join("bank.json")).map_err(|e| e.to_string())?)),
        &json!(sha(&bank_bytes)))?;
    let evidence = json!({"schemaVersion": 1,
        "at": baal_service_runtime::contract::ms_to_iso(now_ms()),
        "claimComment": 5983667980i64,
        "status": "native_settlement_policy_verified_current_snapshot",
        "source": {"cohort": active["cohort"], "bankHash": active["bankHash"],
            "bankFileHash": sha(&bank_bytes), "statsHash": sha(&stats_bytes),
            "asOf": stats["updatedAt"], "publicBookPrefixBytes": prefix_len,
            "publicBookPrefixHash": sha(&prefix),
            "publicBookSequence": publication["state"]["sequence"],
            "publicBookTailHash": publication["state"]["hash"]},
        "native": {"engineHash": native["result"]["engineHash"],
            "binaryHash": native["binaryHash"], "inputHash": native["inputHash"],
            "outputHash": native["outputHash"], "decisionWallMs": native["wallMs"],
            "totalVerificationAndDecisionMs": perf_ms() - start},
        "decisions": native["result"]["decisions"],
        "heldAcquisitionRisk": native["result"]["heldAcquisitionRisk"],
        "proposedMaximumCost": native["result"]["proposedMaximumCost"],
        "existingPaper": {"realizedPnl": stats["wallets"]["3000"]["realizedPnl"],
            "liquidationPnl": stats["wallets"]["3000"]["liquidationPnl"],
            "fills": stats["wallets"]["3000"]["fills"],
            "positions": stats["wallets"]["3000"]["positions"].as_array()
                .map(|a| a.len()).unwrap_or(0)},
        "unchanged": {"activeBinding": true, "frozenBank": true, "walletWritten": false,
            "additionalProcesses": 0, "competitionOrders": 0, "networkRequests": 0},
        "limitations": native["result"]["limitations"], "profitabilityEstablished": false});
    let file = format!("data/settlement-value-snapshot-{}.json", stamp);
    write_wx(Path::new(&file),
        format!("{}\n", serde_json::to_string(&evidence).map_err(|e| e.to_string())?).as_bytes())?;
    Ok(json!({"file": file, "status": evidence["status"], "timing": evidence["native"],
        "decisions": evidence["decisions"].as_array().cloned().unwrap_or_default().iter()
            .map(|x| json!({"candidate": x["candidateId"], "action": x["action"],
                "reason": x["reason"], "worstCaseHeldLoss": x["worstCaseHeldLoss"]}))
            .collect::<Vec<_>>(),
        "proposedMaximumCost": evidence["proposedMaximumCost"],
        "existingPaper": evidence["existingPaper"], "profitabilityEstablished": false}))
}
