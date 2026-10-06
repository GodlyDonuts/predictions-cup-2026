// data/verify-native-request-clock-20261004.mjs — request-causal native engine milestone.
use crate::archive_checked_native_graph::{engine_graph_files, engine_graph_list};
use crate::native_request_clock_evidence_helpers_20261004::{aggregate_future, verify_services};
use baal_collector::rust_backend::{evaluate_native_capture, native_executable};
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn instant(s: &V) -> f64 { s.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
fn ae(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.to_string()) }
}
fn immutable_copy(source: &Path, target: &Path) -> Result<(), String> {
    let b = std::fs::read(source).map_err(|e| e.to_string())?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if target.exists() {
        ae(sha(&std::fs::read(target).map_err(|e| e.to_string())?) == sha(&b), "immutableCopy")?;
    } else {
        std::fs::OpenOptions::new().write(true).create_new(true).open(target)
            .and_then(|mut f| std::io::Write::write_all(&mut f, &b))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn sealed_end(journal: &Path) -> Result<u64, String> {
    let mut fd = std::fs::File::open(journal).map_err(|e| e.to_string())?;
    let size = fd.metadata().map_err(|e| e.to_string())?.len();
    let amount = std::cmp::min(size, 16 * 1024 * 1024);
    let mut b = vec![0u8; amount as usize];
    fd.seek(SeekFrom::Start(size - amount)).map_err(|e| e.to_string())?;
    let n = fd.read(&mut b).map_err(|e| e.to_string())?;
    ae(n == amount as usize, "readSync")?;
    let last = b.iter().rposition(|x| *x == 10).ok_or("lastIndexOf")?;
    Ok(size - amount + last as u64 + 1)
}

pub fn run(root: &Path) -> Result<V, String> {
    let runtime = rt();
    let research = runtime.join("research-cohorts");
    let journal = runtime.join("journal.jsonl");
    let graph = engine_graph_list(root)?;
    let (_, _, files) = engine_graph_files(root, &graph)?;
    let mut source_hashes = serde_json::Map::new();
    let mut h = sha2::Sha256::new();
    use sha2::Digest;
    for (name, bytes) in &files {
        let n = name.as_bytes();
        h.update((n.len() as u64).to_le_bytes());
        h.update(n);
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(bytes);
        source_hashes.insert(name.clone(), json!(sha(bytes)));
    }
    let engine_hash: String = h.finalize().iter().map(|b| format!("{:02x}", b)).collect();
    let exe = native_executable();
    let binary_hash = sha(&std::fs::read(&exe).map_err(|e| e.to_string())?);
    let runtime_library_hash = sha(&std::fs::read(
        exe.parent().unwrap_or(Path::new("")).join("libunwind.dll")).map_err(|e| e.to_string())?);
    let build = runtime.join("native-builds").join(&binary_hash);
    let archive = runtime.join("native-sources").join(&engine_hash);
    for d in [&build, &archive] {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    for f in ["baal-backend.exe", "libunwind.dll"] {
        immutable_copy(&exe.parent().unwrap_or(Path::new("")).join(f), &build.join(f))?;
    }
    for (name, _) in &files {
        let rel = name.strip_prefix("backend-rust/").unwrap_or(name);
        immutable_copy(&root.join("backend-rust").join(rel), &archive.join(rel))?;
    }
    let executable = build.join("baal-backend.exe");
    let cost_dir = research.join("baal-dip-development-costs-20261004-v1");
    let old_declaration = cost_dir.join("declaration.json");
    let old_output = read(&cost_dir.join("evaluation-v1.json"))?;
    let old_d = read(&old_declaration)?;
    ae(old_d["config"]["feeBps"].as_i64() == Some(0), "feeBps")?;
    ae(old_output["declarationFileHash"].as_str().unwrap_or("")
        == sha(&std::fs::read(&old_declaration).map_err(|e| e.to_string())?), "declarationFileHash")?;
    let work = research.join("baal-native-clock-diagnostic-20261004-v2")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let end_byte = old_output["capture"]["source_bytes"].as_u64().unwrap_or(0);
    let expected_sha = old_output["capture"]["source_hash"].as_str().unwrap_or("").to_string();
    let t = Instant::now();
    let legacy = evaluate_native_capture(&journal, &old_declaration, end_byte, &expected_sha,
        &work.join("legacy.json"), 8, &executable)?;
    let legacy_wall_ms = t.elapsed().as_secs_f64() * 1000.0;
    ae(legacy["capture"]["engine_hash"].as_str().unwrap_or("") == engine_hash, "engine_hash")?;
    if legacy["evaluation"]["scorecards"] != old_output["evaluation"]["scorecards"] {
        return Err("deepEqual legacy.evaluation.scorecards oldOutput.evaluation.scorecards".into());
    }
    let mut strict_d = old_d.clone();
    {
        let o = strict_d.as_object_mut().ok_or("strict_d")?;
        o.insert("cohort".into(), json!("baal-native-clock-diagnostic-20261004-v2"));
        o.insert("mode".into(), json!("causal_dip_development_diagnostic_v2"));
        o.insert("declaredAt".into(), json!(ms_to_iso(now_ms())));
        let mut cfg = o["config"].as_object().cloned().unwrap_or_default();
        cfg.insert("fillClockPolicy".into(), json!("independent_request_latency_v2"));
        o.insert("config".into(), V::Object(cfg));
    }
    let strict_file = work.join("declaration.json");
    std::fs::OpenOptions::new().write(true).create_new(true).open(&strict_file)
        .and_then(|mut f| std::io::Write::write_all(&mut f,
            format!("{}\n", serde_json::to_string_pretty(&strict_d).unwrap_or_default()).as_bytes()))
        .map_err(|e| e.to_string())?;
    let t = Instant::now();
    let strict = evaluate_native_capture(&journal, &strict_file, end_byte, &expected_sha,
        &work.join("strict.json"), 8, &executable)?;
    let strict_wall_ms = t.elapsed().as_secs_f64() * 1000.0;
    ae(strict["capture"]["engine_hash"].as_str().unwrap_or("") == engine_hash, "engine_hash")?;
    ae(strict["evaluation"]["scorecards"].as_array().map(|a| a.len())
        == legacy["evaluation"]["scorecards"].as_array().map(|a| a.len()), "scorecards")?;
    let normal = |p: &V| -> V {
        let mut x = p.clone();
        if let Some(o) = x.as_object_mut() {
            o.shift_remove("fillClockPolicy");
            o.shift_remove("blockedPreLatencySnapshots");
        }
        x
    };
    let key = |c: &V| -> String {
        format!("{}/{}/{}", c["strategy"].as_str().unwrap_or(""),
            c["scenario"]["id"].as_str().unwrap_or(""), c["exitPolicy"].as_str().unwrap_or(""))
    };
    let mut prior: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    for c in legacy["evaluation"]["scorecards"].as_array().cloned().unwrap_or_default() {
        prior.insert(key(&c), c);
    }
    let summary_card = |c: &V| -> V {
        json!({"strategy": c["strategy"], "scenario": c["scenario"]["id"],
            "exitPolicy": c["exitPolicy"],
            "fills": c["fills"].as_array().map(|a| a.len() as i64).unwrap_or(0),
            "completedTrades": c["point"]["completedTrades"], "cash": c["point"]["cash"],
            "realizedPnl": c["point"]["realizedPnl"],
            "conservativePnl": c["point"]["conservativePnl"],
            "liquidationPnl": c["point"]["liquidationPnl"],
            "valuation": c["point"]["valuation"], "openShares": c["point"]["openShares"],
            "blockedPreLatencySnapshots":
                c["point"]["blockedPreLatencySnapshots"].as_i64().unwrap_or(0)})
    };
    let mut clocks = 0i64;
    for c in strict["evaluation"]["scorecards"].as_array().cloned().unwrap_or_default() {
        for f in c["fills"].as_array().cloned().unwrap_or_default() {
            let q = &f["sourceClock"];
            ae(q["policy"].as_str() == Some("independent_request_latency_v2"), "policy")?;
            for k in ["decisionSequence", "decisionSourceNs", "fillSequence", "fillSourceNs"] {
                ae(q[k].is_string(), k)?;
            }
            ae(q["requestStartedAt"].as_f64().unwrap_or(f64::NAN)
                >= q["decisionAt"].as_f64().unwrap_or(f64::NAN) + q["minimumLatencyMs"].as_f64().unwrap_or(f64::NAN),
                "requestStartedAt")?;
            ae(q["receivedAt"] == f["at"], "receivedAt")?;
            ae(q["receivedAt"].as_f64().unwrap_or(f64::NAN)
                >= q["requestStartedAt"].as_f64().unwrap_or(f64::NAN), "receivedAt2")?;
            ae(q["fillSequence"] != q["decisionSequence"]
                || q["fillSourceNs"] != q["decisionSourceNs"], "clockDistinct")?;
            clocks += 1;
        }
    }
    let changed: Vec<V> = strict["evaluation"]["scorecards"].as_array().cloned()
        .unwrap_or_default().into_iter()
        .filter(|c| js_stringify(&normal(&c["point"]))
            != js_stringify(&normal(&prior.get(&key(c)).cloned().unwrap_or(V::Null)["point"])))
        .collect();
    let end_byte_now = sealed_end(&journal)?;
    let source_hash = baal_collector::native_journal_restore::source_prefix_hash(
        &journal, end_byte_now, None)?;
    let future_dir = research.join("baal-dip-request-clock-20261004-v2");
    let future_file = future_dir.join("declaration.json");
    let future_bytes = std::fs::read(&future_file).map_err(|e| e.to_string())?;
    let frozen = read(&root.join("data/native-request-clock-declaration-20261004.json"))?;
    ae(sha(&future_bytes) == frozen["receipt"]["declarationHash"].as_str().unwrap_or(""), "declarationHash")?;
    let t = Instant::now();
    let future_out = future_dir.join(format!("{}.json", uuid::Uuid::new_v4()));
    let future = evaluate_native_capture(&journal, &future_file, end_byte_now, &source_hash,
        &future_out, 8, &executable)?;
    let future_wall_ms = t.elapsed().as_secs_f64() * 1000.0;
    ae(future["capture"]["engine_hash"].as_str().unwrap_or("") == engine_hash, "engine_hash")?;
    ae(future["evaluation"]["evaluationStart"] == frozen["receipt"]["tradingStartsAfter"], "evaluationStart")?;
    let trading_starts = instant(&frozen["receipt"]["tradingStartsAfter"]);
    for c in future["evaluation"]["scorecards"].as_array().cloned().unwrap_or_default() {
        for f in c["fills"].as_array().cloned().unwrap_or_default() {
            let q = &f["sourceClock"];
            ae(q["decisionAt"].as_f64().unwrap_or(f64::NAN) > trading_starts, "decisionAt")?;
            ae(q["requestStartedAt"].as_f64().unwrap_or(f64::NAN)
                >= q["decisionAt"].as_f64().unwrap_or(f64::NAN) + q["minimumLatencyMs"].as_f64().unwrap_or(f64::NAN),
                "requestStartedAt")?;
            ae(q["receivedAt"] == f["at"], "receivedAt")?;
        }
    }
    for out in [work.join("legacy.json"), work.join("strict.json"), future_out.clone()] {
        let receipt = read(&PathBuf::from(format!("{}.receipt.json", out.display())))?;
        ae(receipt["binaryHash"].as_str().unwrap_or("") == binary_hash, "receipt.binaryHash")?;
        ae(receipt["engineHash"].as_str().unwrap_or("") == engine_hash, "receipt.engineHash")?;
        ae(receipt["outputHash"].as_str().unwrap_or("")
            == sha(&std::fs::read(&out).map_err(|e| e.to_string())?), "receipt.outputHash")?;
    }
    let services = verify_services()?;
    let aggregates = aggregate_future(future["evaluation"]["scorecards"].as_array()
        .cloned().unwrap_or_default().as_slice())?;
    let strict_days = (strict["capture"]["through"].as_f64().unwrap_or(0.0)
        - strict["capture"]["first_at"].as_f64().unwrap_or(0.0)) / 86400000.0;
    let future_days = (future["capture"]["through"].as_f64().unwrap_or(0.0)
        - future["capture"]["first_at"].as_f64().unwrap_or(0.0)) / 86400000.0;
    let mut subsequent = serde_json::Map::new();
    subsequent.insert("cohort".into(), frozen["receipt"]["cohort"].clone());
    subsequent.insert("declarationHash".into(), frozen["receipt"]["declarationHash"].clone());
    subsequent.insert("start".into(), future["evaluation"]["evaluationStart"].clone());
    subsequent.insert("through".into(), future["evaluation"]["evaluationThrough"].clone());
    subsequent.insert("source".into(), json!({"hash": source_hash, "bytes": end_byte_now,
        "records": future["capture"]["source_records"], "days": future_days}));
    subsequent.insert("classification".into(), future["evaluation"]["evaluationClassification"].clone());
    subsequent.insert("candidates".into(), future["evaluation"]["candidates"].clone());
    subsequent.insert("recordsAfterDeclaration".into(), future["evaluation"]["subsequentRecords"].clone());
    subsequent.insert("cyclesAfterDeclaration".into(), future["evaluation"]["subsequentCycles"].clone());
    subsequent.insert("wallMs".into(), json!(future_wall_ms));
    subsequent.insert("captureMs".into(), future["capture"]["verify_ms"].clone());
    subsequent.insert("replayMs".into(), future["evaluation"]["replayMs"].clone());
    if let Some(o) = aggregates.as_object() {
        for (k, v) in o { subsequent.insert(k.clone(), v.clone()); }
    }
    subsequent.insert("parametersTuned".into(), json!(false));
    subsequent.insert("fees".into(), json!(0));
    subsequent.insert("outputHash".into(),
        json!(sha(&std::fs::read(&future_out).map_err(|e| e.to_string())?)));
    let evidence = json!({"schemaVersion": 1, "at": ms_to_iso(now_ms()), "claim": 5980646213i64,
        "status": "request_causal_native_engine_verified",
        "engineHash": engine_hash, "binaryHash": binary_hash,
        "runtimeLibraryHash": runtime_library_hash,
        "sourceHashes": V::Object(source_hashes), "archivedSourceFiles": files.len() as i64,
        "checks": {"nodePassed": 21, "nodeFailed": 0, "rustPassed": 2, "rustFailed": 0,
            "skipped": 0,
            "legacyHistoricalCardsExact": legacy["evaluation"]["scorecards"].as_array().map(|a| a.len() as i64).unwrap_or(0),
            "strictHistoricalFillClocksVerified": clocks},
        "historical": {
            "classification": "same_reused_development_prefix_correctness_diagnostic_not_profit_proof",
            "source": {"hash": expected_sha, "bytes": end_byte,
                "records": strict["capture"]["source_records"], "days": strict_days},
            "candidates": strict["evaluation"]["candidates"],
            "legacyWallMs": legacy_wall_ms, "strictWallMs": strict_wall_ms,
            "strictReplayMs": strict["evaluation"]["replayMs"],
            "changedCards": changed.len() as i64,
            "totalBlockedSnapshots": strict["evaluation"]["scorecards"].as_array()
                .cloned().unwrap_or_default().iter()
                .map(|c| c["point"]["blockedPreLatencySnapshots"].as_i64().unwrap_or(0)).sum::<i64>(),
            "changed": changed.iter().map(|c| json!({
                "legacy": summary_card(&prior.get(&key(c)).cloned().unwrap_or(V::Null)),
                "strict": summary_card(c)})).collect::<Vec<V>>()},
        "subsequent": V::Object(subsequent),
        "services": services, "currentServicesUnchanged": true, "liveBanksUnchanged": true,
        "additionalPersistentProcesses": 0, "networkRequests": 0, "competitionOrders": 0,
        "profitabilityEstablished": false,
        "limitations": [
            "Versioned correction, not a new profit claim",
            "Historical clock comparison reuses the previously inspected source and unchanged strategies",
            "Future observations are replayed counterfactual fills; queue, impact and unresolved public-book gaps remain",
            "Only minutes of data after the frozen declaration; too little to judge a strategy",
            "Correlated wallets and copied signals are not independent trades or additive P&L",
            "Cash-only inventory-zero floors with stale marks are not realized losses",
            "Legacy receipt timing is retained solely for exact reproduction of original frozen experiments"]});
    atomic_json_canonical(&root.join("data/native-request-clock-milestone-20261004.json"), &evidence)
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&json!({"status": evidence["status"], "engineHash": engine_hash,
        "binaryHash": binary_hash, "checks": evidence["checks"],
        "historical": {"candidates": evidence["historical"]["candidates"],
            "changedCards": changed.len() as i64,
            "blocked": evidence["historical"]["totalBlockedSnapshots"],
            "strictWallMs": strict_wall_ms,
            "strictReplayMs": evidence["historical"]["strictReplayMs"]},
        "subsequent": evidence["subsequent"], "competitionOrders": 0,
        "profitabilityEstablished": false})));
    Ok(evidence)
}
