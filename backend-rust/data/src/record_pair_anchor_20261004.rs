// data/record-pair-anchor-20261004.mjs — pair-anchor milestone record (+dashboard check).
use baal_service_runtime::atomic::atomic_json_canonical as atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::path::Path;

fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn now_iso() -> String {
    ms_to_iso(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0))
}
fn truthy(v: &V) -> bool {
    match v {
        V::Null => false,
        V::Bool(b) => *b,
        V::Number(n) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        V::String(s) => !s.is_empty(),
        _ => true,
    }
}
fn arr_len(x: &V) -> i64 { x.as_array().map(|a| a.len() as i64).unwrap_or(0) }

pub fn run(root: &Path) -> Result<V, String> {
    let live = read(&root.join("data/pair-anchor-live-20261004.json"))?;
    let comparison = read(&root.join("data/pair-anchor-comparison-20261004.json"))?;
    let at = now_iso();
    let mut milestone = json!({"at": at, "status": "corrected_pair_running_with_subsequent_fills",
        "claim": 5980215995i64, "findingComment": 5980240424i64, "tests": 23, "skippedTests": 0,
        "evidence": ["data/pair-bottleneck-20261004.json", "data/pair-anchor-comparison-20261004.json",
            "data/pair-anchor-preservation-20261004.json", "data/pair-anchor-live-20261004.json"],
        "finding": "126 fresh positively admitted pair observations repeatedly recreated a confirmation anchor after the intermediate serialized first-leg refresh cleared the cached counterpart. Zero original intents/fills.",
        "correction": "Keep a bounded non-executable confirmation marker during a valid incoming refresh; both later versions/requests and a current complete cofresh admitted pair are still required. Expiry, boundaries, exclusions, uncertain account, expired review and complete ineligible pairs clear the marker.",
        "historical": {"originalFills": comparison["original"]["fills"],
            "correctedCounterfactualFills": comparison["correctedCounterfactual"]["fills"],
            "realizedPnl": comparison["correctedCounterfactual"]["realizedPnl"],
            "liquidationPnl": comparison["correctedCounterfactual"]["estimatedLiquidationPnl"],
            "ordinaryConditionalSurplus": comparison["correctedCounterfactual"]["ordinaryConditionalSettlementSurplus"],
            "parametersTuned": false},
        "subsequent": live["prospectivePaper"], "cohort": live["cohort"], "bankHash": live["bankHash"],
        "cutoff": live["cutoff"], "evidenceThrough": live["evidenceThrough"],
        "fullStateReplayVerified": true, "sourceFillCausalityAndPriceCapsVerified": true,
        "frozenFiles": live["frozenFiles"], "dashboardVerified": false, "policyUnchanged": true,
        "fees": "0.000000", "accountingReset": false, "extraPersistentProcesses": 0, "extraReadSlots": 0,
        "competitionOrders": 0, "profitabilityEstablished": false,
        "unresolved": ["Matched paper inventory has no completed profitable resale; observe actual later quotes and outcomes without resetting/tuning this cohort",
            "Conditional ordinary settlement floor has fusion/admin/refund exceptions and the existing review expiry",
            "Independent code review and sufficient subsequent economics before a named return handoff or contest activation"]});
    let response = reqwest::blocking::Client::new()
        .get("http://127.0.0.1:3000/api/wallets")
        .timeout(std::time::Duration::from_millis(10000))
        .send().map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let ok = response.status().is_success();
    let dashboard: V = response.json().map_err(|e| e.to_string())?;
    let pair = dashboard["wallets"].as_array().cloned().unwrap_or_default().into_iter()
        .find(|w| w["id"].as_str() == Some("baal-paired-inventory"));
    if !ok || pair.as_ref().map(|p| truthy(&p["available"])).unwrap_or(false) == false
        || pair.as_ref().map(|p| p["comparison"]["cohort"].clone()).unwrap_or(V::Null) != live["cohort"]
        || pair.as_ref().map(|p| p["metrics"]["fills"].as_f64().unwrap_or(f64::NAN)
            < live["prospectivePaper"]["fills"].as_f64().unwrap_or(f64::NAN)).unwrap_or(false)
        || pair.as_ref().map(|p| p["status"].as_str() != Some("running")).unwrap_or(true) {
        return Err("Corrected pair missing from local dashboard".into());
    }
    let pair = pair.unwrap();
    milestone["dashboardVerified"] = json!(true);
    milestone["dashboard"] = json!({"status": status, "id": pair["id"], "cohort": pair["comparison"]["cohort"],
        "fills": pair["metrics"]["fills"], "positions": arr_len(&pair["positions"]),
        "hypotheticalTrades": pair["trades"].as_array().cloned().unwrap_or_default().iter()
            .all(|t| t["hypothetical"] == json!(true))});
    atomic_json(&root.join("data/pair-anchor-milestone-20261004.json"), &milestone)?;
    for name in ["data/quant-development-state.json", "data/daenerys-monitor.json"] {
        let f = root.join(name);
        let mut x = read(&f)?;
        x["updatedAt"] = json!(at);
        x["pairAnchorMilestone"] = milestone.clone();
        if name.contains("quant-development") {
            x["activeProcesses"] = json!({"collector": live["pids"]["collector"], "paper": live["pids"]["paper"],
                "dashboard": live["pids"]["dashboard"], "total": 3});
            x["next"] = json!("Keep the corrected pair and original maker/probability cohorts collecting without unrelated restarts. Measure later liquidation and genuine resale/settlement economics; distinguish conditional floor from cash profit and honor review expiry. Use native chronological replay for broader measured opportunities, not repeated parameter tuning on this tiny corrected sample. Execution ownership remains Daenerys until qualified named return handoff.");
            x["previousGoalTurnClassification"] = json!("progress");
            x["goalTurn"] = json!({"at": at, "classification": "progress", "blockingCondition": V::Null,
                "consecutiveBlockedTurns": 0, "status": "active", "evidence": milestone["evidence"]});
            let mut backlog = x["baalBacklog"].as_array().cloned().unwrap_or_default();
            backlog.push(json!({"status": "completed",
                "task": "Fix paired confirmation starvation across serialized fresh-account book refreshes and verify subsequent causal paper fills",
                "completedAt": at, "evidence": milestone["evidence"][3]}));
            x["baalBacklog"] = json!(backlog);
        }
        atomic_json(&f, &x)?;
    }
    let out = json!({"status": milestone["status"], "pids": live["pids"], "subsequent": milestone["subsequent"],
        "dashboard": milestone["dashboard"], "profitabilityEstablished": false});
    println!("{}", js_stringify(&out));
    Ok(milestone)
}
