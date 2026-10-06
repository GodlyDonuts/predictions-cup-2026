// data/run-coupled-bulk-replay-20261005.mjs — coupled bulk replay driver.
use baal_collector::breadth::validate_universe;
use baal_collector::coupled_maker_paper_v2::coupled_bulk_policy;
use baal_collector::coupled_maker_replay_v2::run_coupled_bulk_replay;
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::path::Path;

fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

pub fn run(root: &Path) -> Result<V, String> {
    let runtime_dir = Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default())
        .join("SigTradingCup").join("paper-runtime");
    let raw = read(&root.join("live-paper/price-universe.json"))?;
    let universe = validate_universe(&raw, raw["tournamentId"].as_str().unwrap_or(""))?;
    let bank = json!({"schemaVersion": 1, "mode": "coupled_bulk_history_trade_tape_paper_v2",
        "name": "baal-coupled-bulk-development", "cutoff": "2026-10-03T23:09:25.374Z",
        "entryExpiresAt": "2026-10-05T23:09:25.374Z", "tournamentId": universe["tournamentId"],
        "universe": universe,
        "policy": coupled_bulk_policy(),
        "legs": universe["exchanges"].as_array().cloned().unwrap_or_default().iter()
            .map(|e| json!({"marketId": e["marketId"], "exchangeId": e["exchangeId"]})).collect::<Vec<V>>()});
    let executable = std::env::current_dir().map_err(|e| e.to_string())?
        .join("backend-rust/target/receipt-strategy-20261005/release/baal-backend.exe");
    let r = run_coupled_bulk_replay(&runtime_dir, &bank, &executable,
        "cc775b6e91a9f27193a91adea6a5eda40b0a7735fd77b857ed52221835a81fd5",
        &root.join("data/coupled-bulk-development-20261005.json"), None)?;
    let w = &r["report"]["wallets"]["COUPLED"];
    let out = json!({"at": r["at"], "processed": r["processed"], "through": r["through"],
        "timing": r["timing"], "counts": r["report"]["counts"], "cash": w["cash"],
        "fills": w["fills"], "closed": w["closedTrades"], "pairs": w["pairedRedemptions"],
        "realized": w["realizedPnl"], "liquidation": w["estimatedLiquidationPnl"],
        "held": w["positions"].as_array().map(|a| a.len() as i64).unwrap_or(0),
        "unresolved": w["unresolvedQuotes"].as_array().map(|a| a.len() as i64).unwrap_or(0),
        "profitabilityEstablished": false});
    println!("{}", js_stringify(&out));
    Ok(out)
}
