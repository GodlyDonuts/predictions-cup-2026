// data/run-stable-maker-development-20261005.mjs — stable maker v1 development replay.
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::io::Write;
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn write_pretty_excl(f: &Path, v: &V) -> Result<(), String> {
    let mut fd = std::fs::OpenOptions::new().write(true).create_new(true).open(f).map_err(|e| e.to_string())?;
    fd.write_all(format!("{}\n", serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).as_bytes())
        .map_err(|e| e.to_string())
}

pub const CODE_FILES_V1: &[&str] = &["live-paper/stable-maker-paper-v1.mjs",
    "live-paper/stable-maker-replay-v1.mjs", "live-paper/canonical-entry-exit-study.mjs",
    "live-paper/probability-receipt.mjs", "paper/fixed.mjs", "execution/ledger.mjs",
    "feed/contract.mjs"];

pub fn run(root: &Path) -> Result<V, String> {
    let runtime_dir = rt();
    let mut code_identity = serde_json::Map::new();
    for f in CODE_FILES_V1 {
        code_identity.insert(f.to_string(),
            json!(sha(&std::fs::read(root.join(f)).map_err(|e| e.to_string())?)));
    }
    let declared_at = ms_to_iso(now_ms());
    let legs = json!([{"marketId": "363", "exchangeId": "1052"}, {"marketId": "297", "exchangeId": "986"},
        {"marketId": "381", "exchangeId": "1070"}, {"marketId": "382", "exchangeId": "1071"}]);
    let declaration = json!({"schemaVersion": 1, "mode": "stable_two_side_trade_tape_paper_v1",
        "name": "baal-stable-spread-20261005-v1", "cutoff": declared_at,
        "entryExpiresAt": ms_to_iso(now_ms() + 21600000.0),
        "tournamentId": "bda92870-621e-47b0-bc3c-3602c5c26f55", "legs": legs,
        "policy": baal_collector::stable_maker_paper_v1::stable_maker_policy(),
        "codeIdentity": V::Object(code_identity),
        "dataUse": "Fixed new mechanism; earlier replay is development evidence. Only later post-declaration entries can supply prospective evidence.",
        "executionAuthority": false, "competitionOrders": 0, "fees": 0});
    write_pretty_excl(&root.join("data/stable-maker-declaration-20261005.json"), &declaration)?;
    let mut bank = declaration.clone();
    bank["name"] = json!(format!("{}-development", declaration["name"].as_str().unwrap_or("")));
    bank["cutoff"] = json!("2026-10-04T16:59:49.117Z");
    bank["entryExpiresAt"] = json!(declared_at);
    let out = root.join("data/stable-maker-development-20261005.json");
    let record = baal_collector::stable_maker_replay_v1::run_stable_maker_replay(&runtime_dir,
        &bank, &root.join("backend-rust/target/receipt-strategy-20261005/release/baal-backend.exe"),
        "cc775b6e91a9f27193a91adea6a5eda40b0a7735fd77b857ed52221835a81fd5", &out, None)?;
    let log = json!({"out": "data/stable-maker-development-20261005.json",
        "declaration": declared_at, "processed": record["processed"],
        "timing": record["timing"], "counts": record["report"]["counts"],
        "wallets": record["report"]["wallets"], "profitabilityEstablished": false});
    println!("{}", js_stringify(&log));
    Ok(record)
}
