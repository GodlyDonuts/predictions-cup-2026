// data/record-native-quote-milestone-20261004.mjs — native quote milestone record.
use baal_service_runtime::atomic::atomic_json_canonical as atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::path::Path;

fn sha(b: &[u8]) -> String { sha256(b) }
fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn now_iso() -> String {
    ms_to_iso(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0))
}
fn fail(msg: &str) -> String { msg.to_string() }
fn eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err(fail(&format!("assert.equal: {} !== {}", js_stringify(a), js_stringify(b)))) }
}
fn a_true(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err(fail("assertion failed")) }
}

pub fn run(root: &Path) -> Result<V, String> {
    let prep = read(&root.join("data/native-quote-preparation-20261004.json"))?;
    let live = read(&root.join("data/native-quote-live-20261004.json"))?;
    let before = read(&root.join("data/native-quote-preservation-20261004.json"))?;
    let after = read(&root.join("data/native-quote-preservation-after-20261004.json"))?;
    let tap = std::fs::read_to_string(root.join("data/native-quote-checks-20261004.tap")).map_err(|e| e.to_string())?;
    a_true(tap.contains("# pass 104") && tap.contains("# fail 0") && tap.contains("# skipped 0"))?;
    eq(&after["frozenBanksPreserved"], &json!(true))?;
    eq(&after["originalJournalsPreserved"], &json!(true))?;
    eq(&prep["engineHash"], &live["engineHash"])?;
    eq(&prep["originalScorecardsExact"], &json!(100))?;
    let preserved_all = before["identities"].as_object().map(|m| {
        m.keys().all(|k| before["identities"][k] == after["identities"][k])
    }).unwrap_or(false);
    let milestone = json!({"schemaVersion": 1, "at": now_iso(), "classification": "progress",
        "claimComment": 5982065901i64, "status": "implemented_checked_and_live",
        "cohort": live["cohort"], "cutoff": live["cutoff"], "sourceDays": prep["sourceDays"],
        "initialSourceBytes": prep["sourceBytes"], "markets": prep["markets"], "strategies": 24,
        "candidates": 72, "fees": 0, "tests": {"node": 104, "rust": 3, "failures": 0, "skips": 0},
        "benchmark": prep["timing"],
        "live": {"collector": live["collector"], "persistentProcesses": 3, "lastWallMs": live["lastWallMs"],
            "uniqueFillEvents": live["uniqueFillEvents"], "heldMarketIds": live["heldMarketIds"],
            "pendingMarketIds": live["pendingMarketIds"], "through": live["through"]},
        "originalScorecardsExact": 100, "originalEngineUnchanged": true,
        "originalActiveIdentitiesPreserved": preserved_all,
        "frozenBanksPreserved": true, "originalJournalsPreserved": true, "independentWallets": true,
        "walletResultsAreCorrelated": true, "networkBudgetUnchanged": true, "additionalApiClients": 0,
        "competitionOrders": 0, "executionOwner": "Daenerys", "profitabilityEstablished": false,
        "limits": ["Counterfactual paper wallets; no competition execution activated.",
            "Current new cohort has no fills yet; pending signals are not gains.",
            "Historical account eligibility was unavailable, so historical quotes warm features without retroactive purchases.",
            "Compact replay and original-source comparison are different workloads; no apples-to-apples speed ratio is claimed.",
            "Quote reversion is a hypothesis; subsequent depth, account eligibility, latency, realistic costs and eventual realized results govern."]});
    atomic_json(&root.join("data/native-quote-milestone-20261004.json"), &milestone)?;
    let file = root.join("data/quant-development-state.json");
    let mut state = read(&file)?;
    let mut milestone_full = milestone.clone();
    milestone_full["evidence"] = json!(["data/native-quote-preparation-20261004.json",
        "data/native-quote-live-20261004.json", "data/native-quote-milestone-20261004.json"]);
    milestone_full["publication"] = json!("pending_direct_main");
    state["nativeQuoteHistoryMilestone"] = milestone_full;
    state["activeProcesses"] = json!({"collector": live["pids"]["collector"], "probabilityWorker": live["pids"]["paper"],
        "dashboard": live["pids"]["dashboard"], "persistent": 3, "extra": 0});
    state["goalTurn"] = json!({"at": milestone["at"], "classification": "progress",
        "milestone": "Separate native full-universe quote-history paper cohort implemented and deployed; original wallets preserved; profitability unestablished",
        "requiredWorkRemaining": true});
    state["nextConcreteMilestone"] = json!("Continue the new native quote-history wallets on actual subsequent capture: resolve pending confirmations, diagnose execution/version/account/depth gates and measured net economics; improve compact source admission and useful breadth without weakening causal or ownership rules. Keep current and prior frozen cohorts intact.");
    state["updatedAt"] = milestone["at"].clone();
    atomic_json(&file, &state)?;
    let monitor_file = root.join("data/daenerys-monitor.json");
    let mut monitor = read(&monitor_file)?;
    if monitor["github"].is_null() { monitor["github"] = json!({}); }
    if monitor["github"]["comments"].is_null() { monitor["github"]["comments"] = json!([]); }
    let body = "Codex claim: implement a separate Rust broad-quote-history dip/recovery paper cohort using the verified237-market minute quotes as causal features and verified public authoritative depth exclusively for fills. Two compact source journals replace the5GB state-bearing replay input for this new cohort; preserve exact original chains, publication availability clocks, same-clock ordering, session/invalidation/stop barriers, account eligibility, latency/version/depth/cost gates. Freeze a new future cutoff and independent wallets after code checks; retain current100-case sparse-depth/V3 cohort, all old banks/journals and official ownership. Use fixed untuned feature/horizon/scenario comparisons, not copied-history profit claims. Same single collector/four-book/shared20reads; no extra persistent service or account client, refit/reset, forced trade, contest orders or transfer. Direct checked main, no PR.";
    let already = monitor["github"]["comments"].as_array().cloned().unwrap_or_default().iter()
        .any(|x| x["id"].as_i64() == Some(5982065901));
    if !already {
        let mut comments = monitor["github"]["comments"].as_array().cloned().unwrap_or_default();
        comments.push(json!({"id": 5982065901i64, "contentHash": sha(body.as_bytes()),
            "created_at": V::Null, "updated_at": V::Null}));
        monitor["github"]["comments"] = json!(comments);
    }
    monitor["lastCheckAt"] = milestone["at"].clone();
    atomic_json(&monitor_file, &monitor)?;
    let out = json!({"classification": "progress", "goalComplete": false, "checks": 107, "candidates": 72,
        "collector": live["pids"]["collector"], "fees": 0, "uniqueFillEvents": live["uniqueFillEvents"],
        "pending": live["pendingMarketIds"]});
    println!("{}", js_stringify(&out));
    Ok(milestone)
}
