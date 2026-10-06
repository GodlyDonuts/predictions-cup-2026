// data/check-receipt-policy-live-20261005.mjs — native settlement proposal on
// the current probability-receipt evidence (file-only, no orders).
use baal_service_runtime::json_text::{digest, sha256};
use serde_json::{json, Value as V};
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
fn ms_to_iso(ms: f64) -> String { baal_service_runtime::contract::ms_to_iso(ms) }
fn parse_iso(s: &str) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(s).unwrap_or(f64::NAN)
}
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

pub fn run() -> Result<V, String> {
    let r = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let candidate = read_json(Path::new("data/receipt-production-candidate-20261005.json"))?;
    let bank = candidate["bank"].clone();
    let paper_file = r.join("probability-receipt-cohorts/calibrated-kansas-poll-20261005-v1/bank.json");
    let paper = read_json(&paper_file)?;
    if digest(&paper) != bank["paperBinding"]["bankHash"].as_str().unwrap_or("") {
        return Err("Original paper bank changed".into());
    }
    let stats = read_json(&paper_file.parent().unwrap().join("stats.json"))?;
    let w = &stats["wallets"]["3000"];
    if stats["bankHash"].as_str() != Some(digest(&paper).as_str())
        || baal_service_runtime::json_text::js_truthy(Some(&stats["failed"]))
        || now_ms() - parse_iso(stats["updatedAt"].as_str().unwrap_or("")) > 20000.0
        || w["pending"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
        return Err("Paper evidence unavailable".into());
    }
    let executable = Path::new("backend-rust/target/receipt-strategy-20261005/release")
        .canonicalize().unwrap_or_else(|_| PathBuf::from(
            "backend-rust/target/receipt-strategy-20261005/release"))
        .join("baal-backend.exe");
    let dir = r.join("receipt-policy-dryruns").join(uuid::Uuid::new_v4().to_string());
    mkdir_700(&dir)?;
    let journal = r.join("public-books.jsonl");
    let bytes = std::fs::read(&journal).map_err(|e| e.to_string())?;
    let end_byte = bytes.iter().rposition(|c| *c == b'\n').map(|n| n + 1).unwrap_or(0) as u64;
    let expected_sha = sha(&bytes[..end_byte as usize]);
    let scope = dir.join("scope.json");
    write_wx(&scope, format!("{}\n", serde_json::to_string(&paper["universe"])
        .map_err(|e| e.to_string())?).as_bytes())?;
    let publication = baal_collector::rust_backend::read_native_publication_prefix(
        &journal, &scope, end_byte, &expected_sha, &dir.join("publication.json"),
        &executable, bank["nativeBackend"]["binaryHash"].as_str().unwrap_or(""))?;
    if publication["engineHash"] != bank["nativeBackend"]["engineHash"] {
        return Err("Native source graph mismatch".into());
    }
    let eligibility: Vec<V> = std::fs::read_to_string(r.join("account-eligibility.jsonl"))
        .map_err(|e| e.to_string())?
        .trim().split('\n')
        .map(|l| serde_json::from_str::<V>(l).map(|x| x["event"]["snapshot"].clone()))
        .collect::<Result<_, _>>().map_err(|e| e.to_string())?;
    let now = now_ms();
    let mut candidates: Vec<V> = Vec::new();
    for c in bank["candidates"].as_array().cloned().unwrap_or_default() {
        let key = format!("{}:YES", c["exchangeId"].as_str().unwrap_or(""));
        let node = &publication["state"]["books"][&key];
        let b = if node["book"].is_null() { node.clone() } else { node["book"].clone() };
        if b.is_null() { return Err("No public book".into()); }
        let available_at_ms = parse_iso(node["availableAt"].as_str()
            .or(node["publishedAt"].as_str())
            .or(b["receivedAt"].as_str()).unwrap_or(""));
        let start = parse_iso(b["requestStartedAt"].as_str().unwrap_or(""));
        let a = eligibility.iter().filter(|x| {
            parse_iso(x["positionsAt"].as_str().unwrap_or("")) <= start
                && parse_iso(x["ordersAt"].as_str().unwrap_or("")) <= start
        }).last().cloned();
        let a = match a {
            Some(a) if baal_service_runtime::json_text::js_truthy(Some(&a["complete"])) => a,
            _ => return Err("No causal account eligibility".into()),
        };
        let excluded: Vec<String> = a["excludedMarketIds"].as_array().cloned()
            .unwrap_or_default().iter().filter_map(|x| x.as_str().map(String::from)).collect();
        let mut c = c.clone();
        c["book"] = json!({"requestStartedAtMs": start,
            "receivedAtMs": parse_iso(b["receivedAt"].as_str().unwrap_or("")),
            "availableAtMs": available_at_ms,
            "sequence": b["sequence"], 
            "sourceHash": if !node["publicationHash"].is_null() { node["publicationHash"].clone() }
                else if !node["hash"].is_null() { node["hash"].clone() }
                else { publication["state"]["hash"].clone() },
            "representation": "direct_yes", "bids": b["bids"], "asks": b["asks"]});
        c["account"] = json!({
            "positionsAtMs": parse_iso(a["positionsAt"].as_str().unwrap_or("")),
            "ordersAtMs": parse_iso(a["ordersAt"].as_str().unwrap_or("")),
            "knownAtMs": parse_iso(a["ordersAt"].as_str().unwrap_or("")),
            "complete": true,
            "positionExcluded": excluded.contains(&c["marketId"].as_str().unwrap_or("").to_string()),
            "openOrderExcluded": excluded.contains(&c["marketId"].as_str().unwrap_or("").to_string())});
        candidates.push(c);
    }
    let zero = num_bigint::BigInt::from(0);
    let cumulative_buys = w["recentFills"].as_array().cloned().unwrap_or_default().iter()
        .filter(|f| f["action"].as_str() == Some("buy"))
        .try_fold(zero, |n, f| -> Result<num_bigint::BigInt, String> {
            Ok(n + baal_paper::fixed::big_amount(f["price"].as_str().unwrap_or(""))?
                * num_bigint::BigInt::from(f["quantity"].as_i64().unwrap_or(0)))
        })?;
    let positions: Vec<V> = w["positions"].as_array().cloned().unwrap_or_default().iter()
        .map(|p| -> Result<V, String> {
            let c = bank["candidates"].as_array().cloned().unwrap_or_default().into_iter()
                .find(|c| format!("{}:YES", c["exchangeId"].as_str().unwrap_or(""))
                    == p["instrument"].as_str().unwrap_or(""));
            let f = w["recentFills"].as_array().cloned().unwrap_or_default().into_iter()
                .filter(|f| f["instrument"] == p["instrument"] && f["action"].as_str() == Some("buy"))
                .last();
            match (c, f) {
                (Some(c), Some(f)) => Ok(json!({"candidateId": c["id"],
                    "quantity": p["quantity"], "cost": p["cost"],
                    "enteredAtMs": parse_iso(f["at"].as_str().unwrap_or(""))})),
                _ => Err("Paper attribution missing".into()),
            }
        }).collect::<Result<_, _>>()?;
    let input = json!({"schemaVersion": 2, "mode": "probability_receipt_policy_v1",
        "asOfMs": now, "policy": bank["policy"], "receiptPolicy": bank["receiptPolicy"],
        "receiptState": {"riskExits": {}, "lastExitedAtMs": {}},
        "wallet": {"capital": paper["config"]["initialCash"], "cash": w["cash"],
            "cumulativeBuys": baal_paper::fixed::money(&cumulative_buys),
            "positions": positions, "reservations": []},
        "candidates": candidates});
    let checked = baal_collector::settlement_value_native::run_settlement_value(
        &input, &dir.join("decision"), &executable)?;
    if checked["result"]["engineHash"] != bank["nativeBackend"]["engineHash"] {
        return Err("Native identity mismatch".into());
    }
    let record = json!({"schemaVersion": 1, "at": ms_to_iso(now),
        "kind": "file_only_native_proposal_against_live_paper_evidence",
        "paperBankHash": digest(&paper), "candidateBankHash": candidate["bankHash"],
        "sourceHash": expected_sha, "sourceBytes": end_byte,
        "sourceSequence": publication["sourceRecords"],
        "nativeBackend": bank["nativeBackend"], "inputHash": checked["inputHash"],
        "outputHash": checked["outputHash"], "wallMs": checked["wallMs"],
        "decisions": checked["result"]["decisions"], "officialAccountPositionsUsed": false,
        "networkRequests": 0, "competitionOrders": 0, "executionAuthority": false,
        "profitabilityEstablished": false});
    write_wx(Path::new("data/receipt-native-live-evidence-20261005.json"),
        format!("{}\n", serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?).as_bytes())?;
    Ok(json!({"at": record["at"], "decisions": record["decisions"],
        "networkRequests": 0, "competitionOrders": 0}))
}
