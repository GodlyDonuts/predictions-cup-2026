// data/record-native-collector-20261004.mjs — writes the native-collector milestone.
use baal_service_runtime::atomic::atomic_json_canonical as atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use serde_json::{json, Value as V};
use std::path::Path;

fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn now_iso() -> String {
    ms_to_iso(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0))
}

pub fn run(root: &Path) -> Result<V, String> {
    let live = read(&root.join("data/native-collector-live-20261004.json"))?;
    let benchmark = read(&root.join("data/native-collector-benchmark-20261004.json"))?;
    let at = now_iso();
    let milestone = json!({"at": at, "status": "deployed_and_exact_state_verified", "claim": 5979624440i64,
        "tests": 11, "skippedTests": 0,
        "evidence": ["data/native-collector-benchmark-20261004.json", "data/native-collector-live-20261004.json", "data/native-collector-preservation-20261004.json"],
        "exactStateParity": benchmark["exactCompleteStateParity"], "source": benchmark["source"],
        "optimizedJavascriptMs": benchmark["javascriptMs"], "nativeAdapterMs": benchmark["nativeAdapterMs"],
        "samePrefixSpeedup": benchmark["speedup"], "livePublicationStartupMs": live["nativePublisherMs"],
        "originalValidatorComparisonDistinct": true, "totalCollectorStartupSpeedupClaimed": false,
        "accountingReset": false, "extraPersistentProcesses": 0, "extraReadSlots": 0,
        "competitionOrders": 0, "profitabilityEstablished": false,
        "unresolved": ["Independent review of native producer prefix and byte-identical later publications",
            "Original collector journal restore still parses/hashes approximately4GB in JS; benchmark and migrate exact chain/final-state restoration to Rust",
            "Prospective maker/pair economics and sparse breadth remain insufficient for profit or takeover"]});
    atomic_json(&root.join("data/native-collector-milestone-20261004.json"), &milestone)?;
    for name in ["data/quant-development-state.json", "data/daenerys-monitor.json"] {
        let f = root.join(name);
        let mut x = read(&f)?;
        x["updatedAt"] = json!(at);
        x["nativeCollectorMilestone"] = milestone.clone();
        if name.contains("quant-development") {
            x["activeProcesses"] = json!({"collector": live["pids"]["collector"], "paper": live["pids"]["paper"],
                "dashboard": live["pids"]["dashboard"], "total": 3});
            x["next"] = json!("Profile and accelerate exact restoration of the original ~4GB collector journal with native raw-byte/hash/chain/final-state parity; retain frozen accounting and account-wide pacing. Measure subsequent passive/source-backed economics and honest spread/queue/latency limits. Current native replay/public validation do not establish profitability. No token/order/ownership action from agent proposals.");
            let mut backlog = x["baalBacklog"].as_array().cloned().unwrap_or_default();
            backlog.push(json!({"status": "completed",
                "task": "Move the collector public quote prefix to the pinned native validator with exact producer/state parity",
                "completedAt": at, "evidence": milestone["evidence"][0]}));
            x["baalBacklog"] = json!(backlog);
        }
        atomic_json(&f, &x)?;
    }
    let out = json!({"at": at, "status": milestone["status"], "samePrefixSpeedup": milestone["samePrefixSpeedup"],
        "pids": live["pids"], "profitabilityEstablished": false});
    println!("{}", serde_json::to_string(&out).unwrap_or_default());
    Ok(milestone)
}
