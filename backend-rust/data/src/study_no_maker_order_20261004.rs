// data/study-no-maker-order-20261004.mjs — no-maker order diagnostic.
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_stringify, sha256};
use baal_service_runtime::ledger::payload_hash as ledger_payload_hash;
fn payload_hash(v: &V) -> Result<String, String> {
    ledger_payload_hash(v).map_err(|e| e.to_string())
}
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn now_iso() -> String {
    ms_to_iso(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0))
}
fn read_json(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    let name = "no-maker-tape-20261004-v1";
    let dir = r.join("no-maker-cohorts").join(name);
    let bank = read_json(&dir.join("bank.json"))?;
    let bytes = std::fs::read(dir.join("journal.jsonl")).map_err(|e| e.to_string())?;
    if bytes.last() != Some(&10) {
        return Err("Incomplete source; retry after append completes".into());
    }
    let mut sequence = 0i64;
    let mut previous = String::new();
    let mut cases: Vec<V> = Vec::new();
    for line in String::from_utf8_lossy(&bytes).trim_end().split('\n') {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        if let Some(o) = body.as_object_mut() { o.remove("hash"); }
        sequence += 1;
        if row["sequence"].as_i64() != Some(sequence)
            || row["previous"].as_str().unwrap_or("") != previous
            || sha(js_stringify(&body).as_bytes()) != hash {
            return Err("Original journal integrity".into());
        }
        previous = hash.clone();
        let e = &row["event"];
        if e["kind"].as_str() != Some("book") || e["entryAllowed"] != json!(true) { continue; }
        let books: Vec<V> = bank["legs"].as_array().cloned().unwrap_or_default().iter()
            .map(|l| row["state"]["paper"]["books"][l["exchangeId"].as_str().unwrap_or("")].clone())
            .collect();
        let frame = baal_collector::canonical_no_capacity::evaluate_canonical_no_capacity(
            &bank["review"], &books,
            if e["accountEligibility"].is_null() { None } else { Some(&e["accountEligibility"]) },
            e["availableAt"].as_str().unwrap_or(""), &bank["capacityPolicy"])?;
        if frame["positiveConditionalCapacity"] != json!(true) {
            return Err("Original admission differs".into());
        }
        let book_mid = e["book"]["marketId"].as_str().unwrap_or("");
        let omitted = books.iter().find(|b| b["marketId"].as_str().unwrap_or("") != book_mid)
            .cloned().unwrap_or(V::Null);
        let replay = baal_collector::no_maker_paper::advance_no_maker(&row["state"]["paper"],
            &json!({"kind": "book", "availableAt": e["availableAt"], "book": omitted,
                "accountEligibility": e["accountEligibility"], "entryAllowed": true}), &bank)?;
        let mut newly_quoted: Vec<V> = Vec::new();
        for (family, w) in replay["wallets"].as_object().cloned().unwrap_or_default() {
            for (_id, q) in w["quotes"].as_object().cloned().unwrap_or_default() {
                if row["state"]["paper"]["wallets"][&family]["quotes"][q["id"].as_str().unwrap_or("")]
                    .is_null() {
                    newly_quoted.push(json!({"family": family, "marketId": q["marketId"],
                        "limit": q["limit"], "createdAt": q["createdAt"],
                        "activationAt": q["activationAt"],
                        "sourcePublicationHash": q["publicationHash"]}));
                }
            }
        }
        if !newly_quoted.is_empty() {
            cases.push(json!({"sequence": row["sequence"], "recordHash": hash,
                "availableAt": e["availableAt"], "omittedMarketId": omitted["marketId"],
                "incomingMarketId": e["book"]["marketId"],
                "sourceVersions": books.iter().map(|b| json!({"marketId": b["marketId"],
                    "versionKey": b["versionKey"], "publicationHash": b["publicationHash"],
                    "requestStartedAt": b["requestStartedAt"], "receivedAt": b["receivedAt"]}))
                    .collect::<Vec<V>>(),
                "newlyQuoted": newly_quoted}));
        }
    }
    let out = json!({"at": now_iso(),
        "mode": "real_capture_source_order_admission_development_diagnostic",
        "originalCohort": name, "originalBankHash": payload_hash(&bank)?,
        "source": {"bytes": bytes.len(), "sha256": sha(&bytes), "rows": sequence,
            "tailHash": previous},
        "missedCoherentFrames": cases.len(), "cases": cases,
        "fills": V::Null, "realizedPnl": V::Null, "profitabilityEstablished": false,
        "limitations": ["These are overlapping counterfactual admission checks, not independent trades or earnings.",
            "Original frozen source/accounting is unchanged; the correction gets a new prospective cutoff."]});
    std::fs::write(root.join("data/no-maker-order-diagnostic-20261004.json"),
        serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&json!({"rows": sequence,
        "missedCoherentFrames": out["missedCoherentFrames"], "firstCase": out["cases"][0],
        "fills": V::Null, "realizedPnl": V::Null})));
    Ok(out)
}
