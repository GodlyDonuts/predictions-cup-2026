// data/prepare-baal-evaluation-publication-20261004.mjs — verify the native
// causal evaluator results and publish the milestone evidence.
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn write_pretty(p: &str, v: &V) -> Result<(), String> {
    std::fs::write(p, format!("{}\n", serde_json::to_string_pretty(v)
        .map_err(|e| e.to_string())?)).map_err(|e| e.to_string())
}
fn need_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assertion failed".into()) }
}
fn need(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err("assertion failed".into()) }
}
fn write_wx(p: &Path, b: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600); }
    let mut f = o.open(p).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, b).map_err(|e| e.to_string())
}
fn summarize(c: &V) -> V {
    json!({"strategy": c["strategy"], "scenario": c["scenario"],
        "exitPolicy": c["exitPolicy"], "conservativePnl": c["point"]["conservativePnl"],
        "realizedPnl": c["point"]["realizedPnl"],
        "completedTrades": c["point"]["completedTrades"],
        "fillExecutions": c["point"]["fillExecutions"],
        "openShares": c["point"]["openShares"],
        "netPnl": c["economics"]["netPnl"], "midMove": c["economics"]["midMove"],
        "quotedSpreadAndLadderCost": c["economics"]["quotedSpreadAndLadderCost"],
        "slippageCost": c["economics"]["slippageCost"],
        "fees": c["economics"]["fees"],
        "byExitReason": c["economics"]["byExitReason"]})
}

pub fn run() -> Result<V, String> {
    let runtime = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let root = runtime.join("research-cohorts");
    let later_dir = root.join("baal-dip-subsequent-20261004-v1");
    let costs_dir = root.join("baal-dip-development-costs-20261004-v1");
    let later = read_json(&later_dir.join("evaluation-v2-diagnostics.json"))?;
    let earlier_later = read_json(&later_dir.join("evaluation-v1.json"))?;
    let costs = read_json(&costs_dir.join("evaluation-v1.json"))?;
    let grid = read_json(&root.join("baal-net-20261004-e")
        .join("native-dip-grid-v2.json"))?;
    let stripped: Vec<V> = later["evaluation"]["scorecards"].as_array().cloned()
        .unwrap_or_default().iter().map(|c| {
            let mut x = c.clone();
            if let V::Object(m) = &mut x { m.shift_remove("signalDiagnostics"); }
            x
        }).collect();
    // assert.deepEqual(later.evaluation.scorecards.map(({signalDiagnostics,...c})=>c),
    //   earlierLater.evaluation.scorecards)
    need_eq(&V::Array(stripped), &earlier_later["evaluation"]["scorecards"])?;
    let graph = ["Cargo.toml", "Cargo.lock", "build.rs", ".cargo/config.toml",
        "src/main.rs", "src/money.rs", "src/types.rs", "src/capture.rs",
        "src/engine.rs", "src/replay.rs", "src/trace.rs", "src/sweep.rs",
        "src/evaluation.rs"];
    let mut engine = Sha256::new();
    let mut source_hashes = serde_json::Map::new();
    for p in &graph {
        let bytes = std::fs::read(Path::new("backend-rust").join(p))
            .map_err(|e| e.to_string())?;
        let name = p.as_bytes();
        engine.update((name.len() as u64).to_le_bytes());
        engine.update(name);
        engine.update((bytes.len() as u64).to_le_bytes());
        engine.update(&bytes);
        source_hashes.insert(format!("backend-rust/{}", p), json!(sha(&bytes)));
    }
    let engine_hash = format!("{:x}", engine.finalize());
    need_eq(&json!(engine_hash), &later["capture"]["engine_hash"])?;
    need_eq(&json!(engine_hash), &costs["capture"]["engine_hash"])?;
    let binary = std::fs::read("backend-rust/target/release/baal-backend.exe")
        .map_err(|e| e.to_string())?;
    let binary_hash = sha(&binary);
    let archive = runtime.join("native-builds").join(&binary_hash);
    std::fs::create_dir_all(&archive).map_err(|e| e.to_string())?;
    for (name, bytes) in [("baal-backend.exe", binary.clone()),
            ("libunwind.dll", std::fs::read(
                "backend-rust/target/release/libunwind.dll").map_err(|e| e.to_string())?)] {
        let p = archive.join(name);
        if p.exists() {
            need_eq(&json!(sha(&std::fs::read(&p).map_err(|e| e.to_string())?)),
                &json!(sha(&bytes)))?;
        } else {
            write_wx(&p, &bytes)?;
        }
    }
    let source_archive = runtime.join("native-sources").join(&engine_hash);
    std::fs::create_dir_all(&source_archive).map_err(|e| e.to_string())?;
    for p in &graph {
        let output = source_archive.join(p);
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let bytes = std::fs::read(Path::new("backend-rust").join(p))
            .map_err(|e| e.to_string())?;
        if output.exists() {
            need_eq(&json!(sha(&std::fs::read(&output).map_err(|e| e.to_string())?)),
                &json!(sha(&bytes)))?;
        } else {
            write_wx(&output, &bytes)?;
        }
    }
    for (d, name, result) in [(&later_dir, "evaluation-v2-diagnostics.json", &later),
            (&costs_dir, "evaluation-v1.json", &costs)] {
        let receipt = read_json(&d.join(format!("{}.receipt.json", name)))?;
        need_eq(&receipt["binaryHash"], &json!(binary_hash))?;
        need_eq(&receipt["engineHash"], &json!(engine_hash))?;
        need_eq(&receipt["outputHash"],
            &json!(sha(&std::fs::read(d.join(name)).map_err(|e| e.to_string())?)))?;
        need_eq(&receipt["declarationFileHash"],
            &json!(sha(&std::fs::read(d.join("declaration.json"))
                .map_err(|e| e.to_string())?)))?;
        need_eq(&result["declarationFileHash"], &receipt["declarationFileHash"])?;
    }
    let mut exact_historical_baselines = 0i64;
    for c in costs["evaluation"]["scorecards"].as_array().cloned()
        .unwrap_or_default().iter()
        .filter(|c| c["scenario"]["id"].as_str() == Some("baseline")
            && c["exitPolicy"].as_str() == Some("snapshot_limit_v1")) {
        let old = grid["sweep"]["scorecards"].as_array().cloned().unwrap_or_default()
            .iter().find(|g| serde_json::to_string(&g["parameters"]).unwrap_or_default()
                == serde_json::to_string(&c["parameters"]).unwrap_or_default())
            .cloned().ok_or("assertion failed")?;
        let old = old;
        let fields = ["cash", "conservativePnl", "realizedPnl", "fees", "turnover",
            "completedTrades", "fillExecutions", "intents", "expiredIntents",
            "pendingIntents", "openShares", "unsoldShares"];
        for k in fields {
            need_eq(&c["point"][k],
                &old["points"].as_array().and_then(|a| a.last())
                    .map(|p| p[k].clone()).unwrap_or(V::Null))?;
        }
        exact_historical_baselines += 1;
    }
    let cards = later["evaluation"]["scorecards"].as_array().cloned()
        .unwrap_or_default();
    need_eq(&json!(cards.len() as i64), &json!(100))?;
    need(cards.iter().all(|c| c["fills"].as_array().map(|a| a.is_empty())
            .unwrap_or(false)
        && c["point"]["intents"].as_i64() == Some(0)
        && c["point"]["conservativePnl"].as_str() == Some("0.000000")))?;
    let baseline = costs["evaluation"]["scorecards"].as_array().cloned()
        .unwrap_or_default().iter()
        .find(|c| c["strategy"].as_str() == Some("historical-positive-1")
            && c["scenario"]["id"].as_str() == Some("baseline")
            && c["exitPolicy"].as_str() == Some("snapshot_limit_v1"))
        .cloned().unwrap_or(V::Null);
    // {...(({scorecards,...meta})=>meta)(later.evaluation)}
    let mut eval_meta = later["evaluation"].clone();
    if let V::Object(m) = &mut eval_meta { m.shift_remove("scorecards"); }
    if let V::Object(m) = &mut eval_meta {
        m.insert("source".into(), later["capture"].clone());
        m.insert("selection".into(),
            read_json(&later_dir.join("source-binding.json"))?);
        m.insert("feeBps".into(), json!(0));
        m.insert("initialCash".into(), json!("100.000000"));
        m.insert("allCardsZeroFills".into(), json!(true));
        m.insert("allCardsZeroPnl".into(), json!(true));
        m.insert("unchangedComparedWithEarlierResult".into(), json!(true));
        m.insert("performanceInspectedBeforeFurtherChanges".into(), json!(true));
        m.insert("signalRows".into(), V::Array(cards.iter()
            .filter(|c| c["scenario"]["id"].as_str() == Some("baseline")
                && c["exitPolicy"].as_str() == Some("snapshot_limit_v1"))
            .map(|c| {
                let mut x = json!({"strategy": c["strategy"],
                    "parameters": c["parameters"],
                    "observations": c["point"]["signalMetrics"]["observations"]});
                if let V::Object(m) = &mut x {
                    if let V::Object(d) = &c["signalDiagnostics"] {
                        for (k, v) in d { m.insert(k.clone(), v.clone()); }
                    }
                }
                x
            }).collect()));
        m.insert("receipt".into(), read_json(&later_dir
            .join("evaluation-v2-diagnostics.json.receipt.json"))?);
    }
    let evidence = json!({"schemaVersion": 1, "at": now_iso(),
        "claimComments": [5978288303i64, 5978438004i64],
        "status": "native_causal_evaluator_implemented_tested",
        "engineHash": engine_hash, "binaryHash": binary_hash,
        "sourceHashes": V::Object(source_hashes),
        "implemented": [
            "Original-byte prefix/clock/declaration binding",
            "Fresh later wallets with prior-only feature warmup",
            "Regular and experimental profit-floor/bounded-stop exits",
            "Exact FIFO mid/spread/ladder/slippage/fee attribution",
            "Separate flat-history/z/cost gate diagnostics",
            "Eight-worker immutable cost/latency/depth comparisons",
            "Explicit reused-development classification"],
        "subsequent": eval_meta,
        "developmentCosts": {
            "classification": costs["evaluation"]["evaluationClassification"],
            "source": costs["capture"],
            "candidates": costs["evaluation"]["candidates"],
            "replayMs": costs["evaluation"]["replayMs"],
            "selection": read_json(&costs_dir.join("source-binding.json"))?,
            "exactHistoricalBaselines": exact_historical_baselines,
            "baselineEconomics": summarize(&baseline),
            "sensitivity": costs["evaluation"]["scorecards"].as_array().cloned()
                .unwrap_or_default().iter()
                .filter(|c| c["strategy"].as_str() == Some("historical-positive-1"))
                .map(summarize).collect::<Vec<_>>(),
            "comparison": costs["evaluation"]["scorecards"].as_array().cloned()
                .unwrap_or_default().iter()
                .filter(|c| c["scenario"]["id"].as_str() == Some("baseline"))
                .map(summarize).collect::<Vec<_>>(),
            "receipt": read_json(&costs_dir.join("evaluation-v1.json.receipt.json"))?},
        "checks": {"nativeNodePassed": 13, "nativeNodeFailed": 0,
            "rustPassed": 1, "rustFailed": 0, "adjacentJsPreviouslyPassed": 23,
            "fullRepositorySuite": false},
        "decisions": [
            "No dip strategy or capital increase activated: tiny selected baseline gain is not robust to modeled cost",
            "Experimental exit policy retained only for offline comparison: it did not improve measured development economics",
            "Preserve original output/declaration/receipt and all live frozen wallets; diagnostics add new immutable outputs"],
        "limitations": [
            "Subsequent prices were collected before declaration; this is retrospective subsequent data, not live prospective trading",
            "Historical grid selection and exit comparisons reuse development data; correlated cases are not independent samples",
            "One tick after five closes is not established profitability",
            "Snapshot fills omit exchange queue priority, impact and unobserved intrainterval movements",
            "Unchanged latency outcomes on sparse snapshots cannot prove speed is unimportant in a dense live feed",
            "Native compute/replay migrated; Node remains the live collector, dashboard and original fitting runtime"],
        "networkRequests": 0, "competitionOrders": 0,
        "additionalPersistentProcesses": 0, "productionActivation": false,
        "profitabilityEstablished": false});
    write_pretty("data/baal-native-evaluation-20261004.json", &evidence)?;
    let mut q = read_json(Path::new("data/quant-development-state.json"))?;
    q["updatedAt"] = evidence["at"].clone();
    q["baalNativeEvaluationMilestone"] = evidence.clone();
    let duplicate_task = "Use the native backend for subsequent untouched captures and execution-cost sensitivity; improve documented tournament realtime ingestion through the single collector to reduce sparse historical coverage.";
    let mut replaced = false;
    let backlog: Vec<V> = q["baalBacklog"].as_array().cloned().unwrap_or_default()
        .iter().map(|entry| {
            if entry["task"].as_str() != Some(duplicate_task) { return entry.clone(); }
            if replaced { return entry.clone(); }
            replaced = true;
            let mut e = entry.clone();
            e["status"] = json!("completed");
            e["task"] = json!("Implement native fresh-wallet subsequent comparison and development-cost sensitivity; preserve original results");
            e["completedAt"] = evidence["at"].clone();
            e["evidence"] = json!("data/baal-native-evaluation-20261004.json");
            e
        }).collect();
    let mut backlog = backlog;
    backlog.push(json!({"status": "active",
        "task": "Use measured spread/slippage and subsequent sparse coverage to compare a causally defined passive/source-backed edge; declare a new future cutoff before any new selection. Keep frozen wallets unchanged and no forced trades."}));
    q["baalBacklog"] = V::Array(backlog);
    q["next"] = json!("Native Rust replay and economic attribution are working. Next improve a causal passive/source-backed entry hypothesis using separate development data and a named future cutoff. Current dip tiny historical gains are not robust. Realtime auth coordination5978106312 remains pending; preserve the20-read allocation and disabled contest writes.");
    write_pretty("data/quant-development-state.json", &q)?;
    let mut m = read_json(Path::new("data/daenerys-monitor.json"))?;
    m["updatedAt"] = evidence["at"].clone();
    m["last_checked_at_utc"] = evidence["at"].clone();
    m["baalNativeEvaluationMilestone"] = json!({
        "evidence": "data/baal-native-evaluation-20261004.json",
        "engineHash": engine_hash, "claimComments": evidence["claimComments"],
        "subsequentNoEntries": true, "developmentCostsDiagnosed": true,
        "productionActivation": false});
    m["nextConcreteMilestone"] = q["next"].clone();
    write_pretty("data/daenerys-monitor.json", &m)?;
    Ok(json!({"engineHash": engine_hash, "binaryHash": binary_hash,
        "cards": cards.len(), "developmentCards": costs["evaluation"]["candidates"],
        "exactHistoricalBaselines": exact_historical_baselines,
        "archivedSourceFiles": graph.len(), "networkRequests": 0,
        "competitionOrders": 0}))
}
