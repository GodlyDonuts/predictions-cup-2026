// data/check-actual-receipt-fill-pricing-20261006.mjs — replay the sealed
// kansas-horizon portfolio journal verifying exact intent+fill pricing under
// the receipt model (baseline raw + 1 adverse tick).
use baal_collector::settlement_portfolio_paper_v2::assert_settlement_portfolio_state;
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::js_stringify;
use indexmap::IndexMap;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn sealed_end(path: &Path) -> Result<u64, String> {
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    let mut fd = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let back = std::cmp::min(len, 1024 * 1024);
    std::io::Seek::seek(&mut fd, std::io::SeekFrom::Start(len - back)).map_err(|e| e.to_string())?;
    let mut tail = vec![0u8; back as usize];
    std::io::Read::read_exact(&mut fd, &mut tail).map_err(|e| e.to_string())?;
    match tail.iter().rposition(|c| *c == b'\n') {
        Some(i) => Ok(len - back + i as u64 + 1),
        None => Err("DUAL_TIMELAPSE_EMPTY".into()),
    }
}
fn sha(b: &[u8]) -> String { baal_service_runtime::json_text::sha256(b) }
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}

pub fn run() -> Result<V, String> {
    let rt = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let dir = rt.join("settlement-portfolio-v2-cohorts/kansas-horizon-portfolio-20261005-v2");
    let bank: V = serde_json::from_slice(&std::fs::read(dir.join("bank.json"))
        .map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let file = dir.join("journal.jsonl");
    let end = sealed_end(&file)?;
    let mut hash = Sha256::new();
    let mut intents: IndexMap<String, V> = IndexMap::new();
    let mut results: Vec<V> = Vec::new();
    let mut sequence = 0i64;
    let mut previous = String::new();
    let mut known_fills: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in journal_lines(&file, &JournalReadOptions { end_byte: Some(end), ..Default::default() })? {
        hash.update(line.as_bytes());
        hash.update(b"\n");
        let r: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let mut body = r.clone();
        let h = r["hash"].as_str().unwrap_or("").to_string();
        if let V::Object(m) = &mut body { m.shift_remove("hash"); }
        sequence += 1;
        if r["sequence"].as_i64() != Some(sequence)
            || r["previous"].as_str().unwrap_or("") != previous
            || sha(js_stringify(&body).as_bytes()) != h {
            return Err("ORIGINAL_CHAIN".into());
        }
        previous = h;
        assert_settlement_portfolio_state(&r["state"]["paper"], &bank)?;
        let w = &r["state"]["paper"]["wallets"]["1000"];
        for p in w["pending"].as_object().map(|o| o.values()).into_iter().flatten() {
            intents.insert(p["id"].as_str().unwrap_or("").to_string(), p.clone());
        }
        for f in w["fills"].as_array().cloned().unwrap_or_default() {
            let intent_id = f["intentId"].as_str().unwrap_or("").to_string();
            if known_fills.contains(&intent_id) || f["action"].as_str() != Some("buy") { continue; }
            known_fills.insert(intent_id.clone());
            let p = intents.get(&intent_id).cloned();
            let row = r["event"]["rows"].as_array().cloned().unwrap_or_default().into_iter()
                .find(|row| row["sequence"] == f["sourceSequence"] && row["hash"] == f["sourceHash"]);
            if p.is_none() || row.is_none() {
                return Err("EXACT_INTENT_AND_FILLED_SOURCE_REQUIRED".into());
            }
            let p = p.unwrap();
            let row = row.unwrap();
            let created_ms = baal_service_runtime::contract::parse_iso_ms(
                p["createdAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
            let lifetime = bank["fillPolicy"]["intentLifetimeMs"].as_f64().unwrap_or(f64::NAN);
            let mut intent = p.clone();
            if let V::Object(m) = &mut intent {
                m.insert("exchangeId".into(), f["exchangeId"].clone());
                m.insert("expiresAt".into(),
                    json!(baal_service_runtime::contract::ms_to_iso(created_ms + lifetime)));
                m.insert("decisionVersion".into(),
                    if p["decisionVersion"].is_null() { json!("not_recorded_in_v2_intent") }
                    else { p["decisionVersion"].clone() });
            }
            let fill_price = |residual_ticks: i64| -> Result<V, String> {
                baal_collector::receipt_fill_price::receipt_fill_price(
                    &intent, &row["payload"]["book"], &row["sequence"], &row["hash"],
                    f["at"].as_str().unwrap_or(""), 1000, 100, residual_ticks, 20000)
            };
            let raw = fill_price(0)?;
            let stress = fill_price(1)?;
            if !baal_service_runtime::json_text::js_truthy(Some(&raw["eligible"]))
                || !baal_service_runtime::json_text::js_truthy(Some(&stress["eligible"]))
                || stress["total"] != f["price"] {
                return Err("ORIGINAL_STRESS_FILL_PARITY".into());
            }
            results.push(json!({"intentId": p["id"], "filledAt": f["at"],
                "marketId": f["marketId"], "originalStressRecordedPrice": f["price"],
                "originalProtectiveLimit": p["limit"], "rawReceiptBaseline": raw["total"],
                "residualTickStress": stress["total"], "entryQuantity": f["quantity"],
                "protectiveLimitUnchanged": true, "originalRecordedFillUnchanged": true,
                "rawBaselineIsCounterfactual": true, "newBankCreated": false,
                "profitabilityEstablished": false}));
        }
    }
    let out = json!({"schemaVersion": 1, "at": now_iso(), "cohort": bank["name"],
        "source": {"bytes": end, "sha256": format!("{:x}", hash.finalize()),
            "sequence": sequence, "tailHash": previous},
        "distinctPurchases": results.len(), "results": results,
        "originalJournalModified": false, "activeCostModelChanged": false,
        "fees": "0.000000", "accountRequests": 0, "competitionOrders": 0,
        "profitabilityEstablished": false});
    std::fs::write("data/receipt-fill-pricing-actual-proof-20261006.json",
        format!("{}\n", serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    Ok(out)
}
