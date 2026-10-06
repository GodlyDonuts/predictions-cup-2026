// live-paper/settlement-value-native.mjs — settlement value policy adapter.
use baal_paper::fixed::{big_amount, money};
use baal_service_runtime::json_text::{digest, sha256};
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::{json, Value as V};
use num_bigint::BigInt;
use std::path::Path;

fn sha(s: &str) -> String { sha256(s.as_bytes()) }
/// settlementInputFromProbability({bank, stats, publication, account, asOfMs})
pub fn settlement_input_from_probability(bank: &V, stats: &V, publication: &V, account: Option<&V>, as_of_ms: i64) -> Result<V, String> {
    if stats["bankHash"].as_str() != Some(digest(bank).as_str())
        || publication["sequence"] != stats["sourceSequence"]
        || publication["hash"] != stats["sourceHash"] {
        return Err("SETTLEMENT_ADAPTER_FRONTIER".into());
    }
    let wallet = &stats["wallets"]["3000"];
    if wallet.is_null() || wallet["recentFills"] != wallet["fills"] || !wallet["pending"].as_array().map(|p| p.is_empty()).unwrap_or(true) {
        return Err("SETTLEMENT_ADAPTER_COMPLETE_WALLET_REQUIRED".into());
    }
    let group_for = |r: &V| -> Result<String, String> {
        let evidence = bank["researchEvidence"].as_array().cloned().unwrap_or_default();
        let found = evidence.iter().find(|x| x["researchId"] == r["id"]).cloned();
        let ev = found.and_then(|e| e["binding"]["event"].as_str().map(String::from))
            .ok_or("SETTLEMENT_ADAPTER_EVENT_REQUIRED".to_string())?;
        Ok(format!("race_{}", sha(&ev)[..24].to_string()))
    };
    let mut positions = Vec::new();
    for p in wallet["positions"].as_array().cloned().unwrap_or_default() {
        let rows = bank["research"].as_array().cloned().unwrap_or_default();
        let r = rows.iter()
            .find(|x| format!("{}:{}", x["exchangeId"].as_str().unwrap_or(""), x["side"].as_str().unwrap_or("")) == p["instrument"].as_str().unwrap_or(""))
            .cloned().ok_or("SETTLEMENT_ADAPTER_POSITION_UNBOUND".to_string())?;
        positions.push(json!({"candidateId": format!("{}:{}", r["marketId"].as_str().unwrap(), r["side"].as_str().unwrap()),
            "quantity": p["quantity"], "cost": p["cost"],
            "enteredAtMs": parse_iso_ms(p["enteredAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN) as i64}));
    }
    let mut candidates = Vec::new();
    for r in bank["research"].as_array().cloned().unwrap_or_default() {
        let key = format!("{}:{}", r["exchangeId"].as_str().unwrap_or(""), r["side"].as_str().unwrap_or(""));
        let public_book = publication["books"][&key].clone();
        let b = if !public_book["book"].is_null() { public_book["book"].clone() } else { public_book.clone() };
        let available_at = if !public_book["availableAt"].is_null() { public_book["availableAt"].clone() } else { public_book["publishedAt"].clone() };
        let book = if !b.is_null() {
            let bk = json!({
                "requestStartedAtMs": parse_iso_ms(b["requestStartedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN) as i64,
                "receivedAtMs": parse_iso_ms(b["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN) as i64,
                "availableAtMs": parse_iso_ms(available_at.as_str().unwrap_or("")).unwrap_or(f64::NAN) as i64,
                "sequence": b["sequence"].as_str().and_then(|s| s.parse::<i64>().ok()).or(b["sequence"].as_i64()).unwrap_or(0),
                "sourceHash": if !public_book["publicationHash"].is_null() { public_book["publicationHash"].clone() }
                    else if !public_book["hash"].is_null() { public_book["hash"].clone() } else { publication["hash"].clone() },
                "representation": if r["side"].as_str() == Some("YES") { "direct_yes" } else { "canonical_yes_complement" },
                "bids": b["bids"], "asks": b["asks"]});
            Some(bk)
        } else { None };
        let eligibility = account.map(|a| json!({
            "positionsAtMs": parse_iso_ms(a["positionsAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN) as i64,
            "ordersAtMs": parse_iso_ms(a["ordersAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN) as i64,
            "knownAtMs": (parse_iso_ms(a["positionsAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
                .max(parse_iso_ms(a["ordersAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN))) as i64,
            "complete": true,
            "positionExcluded": a["positionExchangeIds"].as_array().map(|x| x.contains(&r["exchangeId"])).unwrap_or(false),
            "openOrderExcluded": a["openOrderExchangeIds"].as_array().map(|x| x.contains(&r["exchangeId"])).unwrap_or(false)
        }));
        candidates.push(json!({
            "id": format!("{}:{}", r["marketId"].as_str().unwrap_or(""), r["side"].as_str().unwrap_or("")),
            "marketId": r["marketId"], "exchangeId": r["exchangeId"], "side": r["side"],
            "predicateHash": r["predicateHash"],
            "riskGroups": ["national_election", group_for(&r)?],
            "settlesAtMs": parse_iso_ms("2026-11-04T17:00:00.000Z").unwrap() as i64,
            "research": {"id": r["id"], "predicateHash": r["predicateHash"], "side": r["side"],
                "lower": r["lower"], "upper": r["upper"],
                "recordedAtMs": parse_iso_ms(r["recordedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN) as i64,
                "sourceAtMs": parse_iso_ms(r["sourceAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN) as i64,
                "expiresAtMs": parse_iso_ms(r["expiresAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN) as i64,
                "method": r["method"], "settlementReviewed": r["settlementReviewed"],
                "flags": r["flags"], "sources": r["sources"]},
            "book": book, "account": eligibility}));
    }
    let cumulative_buys: BigInt = wallet["recentFills"].as_array().cloned().unwrap_or_default().iter()
        .filter(|f| f["action"].as_str() == Some("buy"))
        .map(|f| big_amount(f["price"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0)) * BigInt::from(f["quantity"].as_i64().unwrap_or(0)))
        .sum();
    let mut group_caps = serde_json::Map::new();
    group_caps.insert("national_election".into(), json!("5.000000"));
    for c in &candidates {
        group_caps.insert(c["riskGroups"][1].as_str().unwrap().to_string(), json!("2.000000"));
    }
    Ok(json!({"schemaVersion": 1, "mode": "settlement_value_policy_v1", "asOfMs": as_of_ms,
        "policy": {"minimumEdge": "0.020000", "maximumSpread": "0.050000", "maximumBookAgeMs": 20000,
            "maximumResearchAgeMs": 21600000, "fractionalKellyBps": 2500, "slippageTicks": 1,
            "depthFractionBps": 100, "maxOrderCost": "1.000000", "maxExposure": "10.000000",
            "cashReserve": "90.000000", "cumulativeBuyBudget": "100.000000", "groupCaps": group_caps},
        "wallet": {"capital": bank["config"]["initialCash"], "cash": wallet["cash"],
            "cumulativeBuys": money(&cumulative_buys), "positions": positions, "reservations": []},
        "candidates": candidates}))
}
/// runSettlementValue({input, dir, executable})
pub fn run_settlement_value(input: &V, dir: &Path, executable: &Path) -> Result<V, String> {
    std::fs::create_dir_all(dir).map_err(|_| "SETTLEMENT_ADAPTER_DIR".to_string())?;
    let file = dir.join("input.json");
    let out = dir.join("output.json");
    let bytes = serde_json::to_string(input).map_err(|e| e.to_string())? + "\n";
    let binary_hash = sha256(&std::fs::read(executable).map_err(|_| "SETTLEMENT_ADAPTER_NATIVE_REJECTED".to_string())?);
    std::fs::OpenOptions::new().write(true).create_new(true).open(&file)
        .and_then(|mut f| std::io::Write::write_all(&mut f, bytes.as_bytes()))
        .map_err(|_| "SETTLEMENT_ADAPTER_DIR".to_string())?;
    let start = std::time::Instant::now();
    let status = std::process::Command::new(executable)
        .args(["settlement-policy", "--input", &file.display().to_string(),
            "--expected-sha", &sha(&bytes), "--out", &out.display().to_string()])
        .output().map_err(|_| "SETTLEMENT_ADAPTER_NATIVE_REJECTED".to_string())?;
    if !status.status.success() { return Err("SETTLEMENT_ADAPTER_NATIVE_REJECTED".into()); }
    let result: V = serde_json::from_str(&std::fs::read_to_string(&out).map_err(|_| "SETTLEMENT_ADAPTER_OUTPUT".to_string())?)
        .map_err(|_| "SETTLEMENT_ADAPTER_OUTPUT".to_string())?;
    if result["inputHash"].as_str() != Some(&sha(&bytes))
        || sha256(&std::fs::read(executable).unwrap_or_default()) != binary_hash
        || result["executionAuthority"] != V::Bool(false)
        || result["fills"].as_i64() != Some(0) || result["competitionOrders"].as_i64() != Some(0) {
        return Err("SETTLEMENT_ADAPTER_OUTPUT".into());
    }
    Ok(json!({"result": result, "binaryHash": binary_hash, "inputHash": sha(&bytes),
        "outputHash": sha256(&std::fs::read(&out).unwrap_or_default()),
        "wallMs": start.elapsed().as_secs_f64() * 1000.0}))
}
