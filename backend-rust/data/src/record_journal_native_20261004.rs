// data/record-journal-native-20261004.mjs — write the native journal milestone
// and update the quant-development and daenerys-monitor state files.
use crate::publication_pack::{now_ms, read_json};
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use serde_json::{json, Value as V};
use std::path::Path;

pub fn run() -> Result<V, String> {
    let read = |f: &str| read_json(Path::new(f));
    let live = read("data/journal-native-live-20261004.json")?;
    let benchmark = read("data/journal-native-benchmark-20261004.json")?;
    let at = ms_to_iso(now_ms() as f64);
    let milestone = json!({
        "at": at, "status": "deployed_and_whole_state_verified", "claim": 5979819623i64,
        "nodeChecks": 16, "rustChecks": 2, "skippedChecks": 0,
        "evidence": [
            "data/journal-native-benchmark-20261004.json",
            "data/journal-native-preservation-20261004.json",
            "data/journal-native-live-20261004.json",
        ],
        "source": benchmark["source"],
        "exactLastBodyParity": benchmark["exactLastBodyParity"],
        "samePrefixSpeedupIncludingIndependentHash": benchmark["speedupIncludingPrefixBinding"],
        "liveSource": live["source"],
        "liveJournalStartupMs": live["startup"]["startupReplayMs"],
        "archivedInitialPrototypeResults": [
            "data/journal-native-benchmark-20261004-v1.json",
            "data/journal-native-benchmark-20261004-v2.json",
        ],
        "deploymentFinding": "Preparation initially assumed an empty predecessor for every wallet. Probability journals start from their frozen bank hash. Corrected the preservation check and repeated full-source parity before restart; no wallet mutations.",
        "originalAppendInterfacePreserved": true, "sourceFiles": live["sourceFiles"],
        "accountingReset": false, "extraPersistentProcesses": 0, "extraReadSlots": 0,
        "competitionOrders": 0, "profitabilityEstablished": false,
        "limitations": [
            "Same-prefix measurements are single runs and distinct from model replay or API request pacing",
            "Compact canonical journal, 16MiB record and 256 nesting bounds; noncanonical histories reject rather than fall back",
            "Live API transport, incremental publication/economics and model fitting remain JavaScript",
            "Quantitative trading edge and takeover readiness remain unestablished",
        ],
    });
    atomic_json(Path::new("data/journal-native-milestone-20261004.json"), &milestone)?;
    for f in ["data/quant-development-state.json", "data/daenerys-monitor.json"] {
        let mut x = read(f)?;
        x.as_object_mut().unwrap().insert("updatedAt".into(), json!(at));
        x.as_object_mut().unwrap().insert("nativeJournalMilestone".into(), milestone.clone());
        if f.contains("quant-development") {
            x.as_object_mut().unwrap().insert("activeProcesses".into(), json!({
                "collector": live["pids"]["collector"], "paper": live["pids"]["paper"],
                "dashboard": live["pids"]["dashboard"], "total": 3,
            }));
            x.as_object_mut().unwrap().insert("next".into(), json!(
                "Use the accelerated native replay/sweep on new causal passive/source-backed comparisons; preserve frozen wallets and measure subsequent untouched data with actual spreads, queue prints, depth and liquidation. Diagnose fresh signal economics rather than force turnover or infer profitability from speed. Keep the one serialized collector and the existing execution ownership."
            ));
            x.as_object_mut().unwrap().insert("previousGoalTurnClassification".into(), json!("progress"));
            x.as_object_mut().unwrap().insert("goalTurn".into(), json!({
                "at": at, "classification": "progress", "blockingCondition": V::Null,
                "consecutiveBlockedTurns": 0, "status": "active",
                "evidence": milestone["evidence"],
            }));
            x["baalBacklog"].as_array_mut().unwrap().push(json!({
                "status": "completed",
                "task": "Migrate the original collector journal restoration to Rust with canonical serialization, full state/chain parity and preserved accounting",
                "completedAt": at, "evidence": milestone["evidence"][0],
            }));
        }
        atomic_json(Path::new(f), &x)?;
    }
    Ok(json!({
        "at": at, "status": milestone["status"], "pids": live["pids"],
        "speedup": milestone["samePrefixSpeedupIncludingIndependentHash"],
        "profitabilityEstablished": false,
    }))
}
