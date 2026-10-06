// data/run-native-quote-markouts-20261004.mjs — native quote markouts run (v2 study).
use crate::archive_checked_native_graph::{engine_graph_files, engine_graph_list};
use baal_collector::rust_backend::native_executable;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

fn sha(b: &[u8]) -> String { sha256(b) }
fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn now_iso() -> String {
    ms_to_iso(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0))
}
fn eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err(format!("assert.equal: {} !== {}", js_stringify(a), js_stringify(b))) }
}
fn a_true(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err("assertion failed".to_string()) }
}
fn write_excl(f: &Path, b: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut fd = std::fs::OpenOptions::new().write(true).create_new(true).open(f).map_err(|e| e.to_string())?;
    fd.write_all(b).map_err(|e| e.to_string())
}
fn write_pretty_excl(f: &Path, v: &V) -> Result<(), String> {
    write_excl(f, format!("{}\n", serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).as_bytes())
}

pub fn run(root: &Path, argv: &[String]) -> Result<V, String> {
    let r = rt();
    let active = read(&r.join("native-quote-active.json"))?;
    let dir = r.join("research-cohorts").join(active["cohort"].as_str().unwrap_or(""));
    let preparation = read(&root.join("data/native-quote-preparation-20261004.json"))?;
    eq(&active["cohort"], &json!("baal-quote-history-20261004-v1"))?;
    eq(&json!(sha(&std::fs::read(r.join("native-quote-active.json")).map_err(|e| e.to_string())?)),
        &preparation["activeHash"])?;
    eq(&json!(sha(&std::fs::read(dir.join("declaration.json")).map_err(|e| e.to_string())?)),
        &preparation["declarationHash"])?;
    let frozen_files = ["native-quote-active.json", "native-dip-intent-active.json",
        "coordinated-network-budget.json", "competition-activation.json"];
    let mut before = serde_json::Map::new();
    for f in frozen_files {
        before.insert(f.to_string(),
            json!(sha(&std::fs::read(r.join(f)).map_err(|e| e.to_string())?)));
    }
    if !argv.iter().all(|x| x == "--v2") {
        return Err("Only the current V2 study is runnable with this source".into());
    }
    let revision = "v2";
    let output = dir.join("historical-markouts-v2.json");
    if output.exists() { return Err("Original evidence retained".into()); }
    let native_executable = native_executable();
    let start = Instant::now();
    let child = Command::new(&native_executable)
        .args(["quote-markouts", "--declaration"])
        .arg(dir.join("declaration.json"))
        .args(["--sources"]).arg(dir.join("seed.sources.json"))
        .args(["--workers", "8", "--out"]).arg(&output)
        .output().map_err(|e| e.to_string())?;
    let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
    if child.status.code() != Some(0) {
        return Err(format!("assert.equal status 0 != {:?}: {}", child.status.code(),
            String::from_utf8_lossy(&child.stderr)));
    }
    let receipt: V = serde_json::from_slice(&child.stdout).map_err(|e| e.to_string())?;
    let result = read(&output)?;
    let m = &result["markouts"];
    let files = engine_graph_list(root)?;
    let (engine_hash, _graph_sources, named) = engine_graph_files(root, &files)?;
    let mut source_hashes = serde_json::Map::new();
    for (n, b) in &named { source_hashes.insert(format!("backend-rust/{}", n), json!(sha(b))); }
    eq(&json!(engine_hash), &result["capture"]["engineHash"])?;
    eq(&m["candidates"], &json!(108))?;
    eq(&m["markets"], &json!(237))?;
    eq(&m["fees"], &json!(0))?;
    eq(&m["networkRequests"], &json!(0))?;
    eq(&m["competitionOrders"], &json!(0))?;
    eq(&m["profitabilityEstablished"], &json!(false))?;
    for key in ["backend-rust/src/engine.rs", "backend-rust/src/evaluation.rs", "backend-rust/src/quote_replay.rs"] {
        eq(&json!(source_hashes.get(key).cloned().unwrap_or(V::Null)), &preparation["sourceHashes"][key])?;
    }
    let compact = |c: &V| -> V {
        json!({"strategy": c["strategy"], "parameters": c["parameters"], "scenario": c["scenario"],
            "featurePolicy": c["featurePolicy"], "boundaryPolicy": c["boundaryPolicy"],
            "summary": c["summary"], "signalDiagnostics": c["signalDiagnostics"]})
    };
    let stats: Vec<V> = m["cards"].as_array().cloned().unwrap_or_default().iter().map(&compact).collect();
    let sensitivity: Vec<V> = m["bookBoundarySensitivityCards"].as_array().cloned().unwrap_or_default()
        .iter().map(&compact).collect();
    for c in m["cards"].as_array().cloned().unwrap_or_default().iter()
        .chain(m["bookBoundarySensitivityCards"].as_array().cloned().unwrap_or_default().iter()) {
        let episodes = c["episodes"].as_array().cloned().unwrap_or_default();
        eq(&json!(c["summary"]["statusCounts"].as_object().cloned().unwrap_or_default()
                .values().map(|v| v.as_i64().unwrap_or(0)).sum::<i64>()),
            &json!(episodes.len() as i64))?;
        eq(&c["summary"]["observed"],
            &json!(episodes.iter().filter(|e| e["status"].as_str() == Some("observed")).count() as i64))?;
        let mut prior: HashMap<String, f64> = HashMap::new();
        for e in &episodes {
            let market = e["market"].as_str().unwrap_or("").to_string();
            let published = e["signal"]["publishedAt"].as_f64().unwrap_or(f64::NAN);
            if let Some(last) = prior.get(&market) {
                a_true(published - last
                    >= c["parameters"]["horizonMs"].as_f64().unwrap_or(0.0)
                        + c["scenario"]["latencyMs"].as_f64().unwrap_or(0.0) + 240000.0)?;
            }
            prior.insert(market, published);
            if !e["entry"].is_null() {
                a_true(e["entry"]["requestStartedAt"].as_f64().unwrap_or(f64::NAN)
                    >= published + c["scenario"]["latencyMs"].as_f64().unwrap_or(0.0))?;
                a_true(e["entry"]["ask"].as_f64().unwrap_or(f64::NAN)
                    <= e["signal"]["ask"].as_f64().unwrap_or(f64::NAN))?;
            }
            if !e["outcome"].is_null() {
                a_true(e["outcome"]["requestStartedAt"].as_f64().unwrap_or(f64::NAN)
                    >= e["targetAt"].as_f64().unwrap_or(f64::NAN))?;
                eq(&json!(e["midpointMoveTwiceMicros"].as_i64().unwrap_or(0)
                        - e["spreadCostTwiceMicros"].as_i64().unwrap_or(0)
                        - 2 * e["slippageCostMicros"].as_i64().unwrap_or(0)),
                    &json!(2 * e["quotedNetMarkoutMicros"].as_i64().unwrap_or(0)))?;
            } else {
                eq(&e["quotedNetMarkoutMicros"], &V::Null)?;
            }
        }
    }
    for f in &frozen_files {
        eq(&json!(sha(&std::fs::read(r.join(f)).map_err(|e| e.to_string())?)),
            &before[*f])?;
    }
    let mut evidence = json!({"schemaVersion": 2, "revision": revision, "at": now_iso(),
        "claimComment": 5982524583i64, "status": "native_quote_markouts_measured",
        "mode": m["mode"], "dataUse": m["interpretation"]["dataUse"], "cohort": active["cohort"],
        "declarationHash": preparation["declarationHash"], "engineHash": engine_hash,
        "binaryHash": sha(&std::fs::read(&native_executable).map_err(|e| e.to_string())?),
        "runtimeLibraryHash": sha(&std::fs::read(native_executable.parent().unwrap_or(Path::new("."))
            .join("libunwind.dll")).map_err(|e| e.to_string())?),
        "sourceHashes": V::Object(source_hashes.clone()),
        "outputHash": sha(&std::fs::read(&output).map_err(|e| e.to_string())?),
        "sourceDays": receipt["sourceDays"], "sourceBytes": receipt["sourceBytes"],
        "sources": result["capture"]["sources"], "quoteRows": m["quoteRows"], "markets": m["markets"],
        "candidates": m["candidates"], "diagnosticPaths": m["diagnosticPaths"],
        "distinctFeaturePasses": m["distinctFeaturePasses"],
        "fallingReboundFeatureRejections": m["fallingReboundFeatureRejections"],
        "timing": {"wallMs": wall_ms, "sourceVerifyMs": receipt["captureMs"],
            "featuresAndMarkoutsMs": receipt["replayMs"],
            "quoteReReadAndOutputMs": wall_ms - receipt["captureMs"].as_f64().unwrap_or(0.0)
                - receipt["replayMs"].as_f64().unwrap_or(0.0)},
        "workers": 8, "interpretation": m["interpretation"], "cards": stats,
        "bookBoundarySensitivityCards": sensitivity, "fees": 0,
        "existingActiveHashesUnchanged": V::Object(before.clone()), "originalWalletsReset": false,
        "activeV1EngineUnchanged": true, "activeCohortReplaced": false, "networkRequests": 0,
        "competitionOrders": 0, "profitabilityEstablished": false});
    if revision == "v2" {
        let original = read(&root.join("data/native-quote-markouts-20261004.json"))?;
        evidence["initialBoundaryCensoredStudy"] = json!({
            "evidenceHash": sha(&std::fs::read(root.join("data/native-quote-markouts-20261004.json")).map_err(|e| e.to_string())?),
            "engineHash": original["engineHash"], "binaryHash": original["binaryHash"],
            "outputHash": original["outputHash"], "notOverwritten": true});
    }
    let archive = r.join("native-sources").join(&engine_hash);
    for (file, hash) in &source_hashes {
        let dest = archive.join(file.replacen("backend-rust/", "", 1));
        let b = std::fs::read(root.join(file)).map_err(|e| e.to_string())?;
        eq(&json!(sha(&b)), hash)?;
        std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
        if dest.exists() {
            eq(&json!(sha(&std::fs::read(&dest).map_err(|e| e.to_string())?)), hash)?;
        } else {
            write_excl(&dest, &b)?;
        }
    }
    let build = r.join("native-builds").join(evidence["binaryHash"].as_str().unwrap_or(""));
    std::fs::create_dir_all(&build).map_err(|e| e.to_string())?;
    for file in ["baal-backend.exe", "libunwind.dll"] {
        let dest = build.join(file);
        let b = std::fs::read(native_executable.parent().unwrap_or(Path::new(".")).join(file))
            .map_err(|e| e.to_string())?;
        if dest.exists() {
            eq(&json!(sha(&std::fs::read(&dest).map_err(|e| e.to_string())?)), &json!(sha(&b)))?;
        } else {
            write_excl(&dest, &b)?;
        }
    }
    write_pretty_excl(&root.join(format!("data/native-quote-markouts-{}20261004.json",
        if revision == "v2" { "v2-" } else { "" })), &evidence)?;
    let baseline: Vec<V> = stats.iter()
        .filter(|c| c["scenario"]["id"].as_str() == Some("baseline_3s")
            && c["featurePolicy"].as_str() == Some("original_self_comparison_v1"))
        .map(|c| json!({"id": c["strategy"], "episodes": c["summary"]["episodes"],
            "observed": c["summary"]["observed"], "mean": c["summary"]["quotedNetMarkout"]["mean"],
            "statuses": c["summary"]["statusCounts"]}))
        .collect();
    println!("{}", js_stringify(&json!({"sourceDays": evidence["sourceDays"],
        "sourceBytes": evidence["sourceBytes"], "quoteRows": m["quoteRows"], "candidates": 108,
        "diagnosticPaths": m["diagnosticPaths"], "markets": 237, "timing": evidence["timing"],
        "fallingReboundFeatureRejections": m["fallingReboundFeatureRejections"],
        "baseline": baseline, "profitabilityEstablished": false})));
    Ok(evidence)
}
