// data/run-native-quote-sides-20261004.mjs — native quote sides run.
use crate::archive_checked_native_graph::{engine_graph_files, engine_graph_list};
use baal_collector::rust_backend::native_executable;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::collections::HashSet;
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
fn write_excl(f: &Path, b: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut fd = std::fs::OpenOptions::new().write(true).create_new(true).open(f).map_err(|e| e.to_string())?;
    fd.write_all(b).map_err(|e| e.to_string())
}

pub fn run(root: &Path, argv: &[String]) -> Result<V, String> {
    if !argv.iter().all(|x| x == "--endpoints") { return Err("assertion failed".into()); }
    let endpoints = argv.iter().any(|x| x == "--endpoints");
    let r = rt();
    let active = read(&r.join("native-quote-active.json"))?;
    let dir = r.join("research-cohorts").join(active["cohort"].as_str().unwrap_or(""));
    let mut before = serde_json::Map::new();
    for f in ["native-quote-active.json", "native-dip-intent-active.json",
        "coordinated-network-budget.json", "competition-activation.json"] {
        before.insert(f.to_string(), json!(sha(&std::fs::read(r.join(f)).map_err(|e| e.to_string())?)));
    }
    let prior = read(&dir.join("historical-markouts-v2.json"))?;
    let out = dir.join(if endpoints { "historical-side-endpoints-v1.json" } else { "historical-side-markouts-v1.json" });
    if out.exists() { return Err("assertion failed".into()); }
    let exe = native_executable();
    let start = Instant::now();
    let child = Command::new(&exe)
        .arg(if endpoints { "quote-side-endpoints" } else { "quote-side-markouts" })
        .args(["--declaration"]).arg(dir.join("declaration.json"))
        .args(["--sources"]).arg(dir.join("seed.sources.json"))
        .args(["--workers", "8", "--out"]).arg(&out)
        .output().map_err(|e| e.to_string())?;
    let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
    if child.status.code() != Some(0) {
        return Err(format!("assert.equal status 0 != {:?}: {}", child.status.code(),
            String::from_utf8_lossy(&child.stderr)));
    }
    let receipt: V = serde_json::from_slice(&child.stdout).map_err(|e| e.to_string())?;
    let result = read(&out)?;
    let m = &result["markouts"];
    eq(&m["yes"]["cards"], &prior["markouts"]["cards"])?;
    eq(&m["yes"]["bookBoundarySensitivityCards"], &prior["markouts"]["bookBoundarySensitivityCards"])?;
    eq(&m["candidates"], &json!(216))?;
    eq(&m["diagnosticPaths"], &json!(432))?;
    eq(&m["canonicalDepthChecks"]["sourceBookEvents"], &m["canonicalDepthChecks"]["noBooksDerivedAndValidated"])?;
    eq(&m["networkRequests"], &json!(0))?;
    eq(&m["competitionOrders"], &json!(0))?;
    eq(&m["projection"]["fees"], &json!(0))?;
    for f in before.keys() {
        eq(&json!(sha(&std::fs::read(r.join(f)).map_err(|e| e.to_string())?)), &before[f])?;
    }
    let files = engine_graph_list(root)?;
    let (engine_hash, _g, named) = engine_graph_files(root, &files)?;
    let mut source_hashes = serde_json::Map::new();
    for (n, b) in &named { source_hashes.insert(format!("backend-rust/{}", n), json!(sha(b))); }
    eq(&json!(engine_hash), &result["capture"]["engineHash"])?;
    let compact = |c: &V| -> V {
        json!({"strategy": c["strategy"], "scenario": c["scenario"]["id"],
            "featurePolicy": c["featurePolicy"], "episodes": c["summary"]["episodes"],
            "observed": c["summary"]["observed"], "statusCounts": c["summary"]["statusCounts"],
            "markets": c["summary"]["signalMarkets"].as_object().map(|o| o.len() as i64).unwrap_or(0),
            "days": c["summary"]["signalDays"], "net": c["summary"]["quotedNetMarkout"],
            "midpointMove": c["summary"]["meanMidpointMove"],
            "spreadCost": c["summary"]["meanSpreadCost"],
            "slippageCost": c["summary"]["meanSlippageCost"],
            "quotedBeforeSlippageMean": if c["summary"]["observed"].as_f64().unwrap_or(0.0) != 0.0 {
                json!(c["summary"]["quotedNetMarkout"]["mean"].as_f64().unwrap_or(0.0)
                    + c["summary"]["meanSlippageCost"].as_f64().unwrap_or(0.0))
            } else { V::Null },
            "latencyAskMove": c["summary"]["meanLatencyAskMove"]})
    };
    let side = |label: &str| -> V {
        let cards = m[label]["cards"].as_array().cloned().unwrap_or_default();
        let mut signal_events = HashSet::new();
        let mut observed_intervals = HashSet::new();
        for c in &cards {
            for e in c["episodes"].as_array().cloned().unwrap_or_default() {
                signal_events.insert(format!("{}/{}", e["market"].as_str().unwrap_or(""),
                    e["signal"]["sequence"].as_i64().unwrap_or(0)));
                if !e["outcome"].is_null() {
                    observed_intervals.insert(format!("{}/{}/{}", e["market"].as_str().unwrap_or(""),
                        e["entry"]["sequence"].as_i64().unwrap_or(0),
                        e["outcome"]["sequence"].as_i64().unwrap_or(0)));
                }
            }
        }
        json!({"cards": cards.iter().map(&compact).collect::<Vec<V>>(),
            "bookBoundarySensitivityCards": m[label]["bookBoundarySensitivityCards"].as_array()
                .cloned().unwrap_or_default().iter().map(&compact).collect::<Vec<V>>(),
            "uniqueSignalEvents": signal_events.len() as i64,
            "uniqueObservedIntervals": observed_intervals.len() as i64})
    };
    let mut e = json!({"schemaVersion": 1, "at": now_iso(), "claimComment": 5982841708i64,
        "status": "canonical_two_side_quote_paths_implemented_and_measured",
        "mode": m["mode"], "dataUse": "reused_exploratory_development_data",
        "cohort": active["cohort"], "declarationHash": result["declarationFileHash"],
        "engineHash": engine_hash,
        "binaryHash": sha(&std::fs::read(&exe).map_err(|e| e.to_string())?),
        "runtimeLibraryHash": sha(&std::fs::read(exe.parent().unwrap_or(Path::new("."))
            .join("libunwind.dll")).map_err(|e| e.to_string())?),
        "sourceHashes": V::Object(source_hashes.clone()),
        "outputHash": sha(&std::fs::read(&out).map_err(|e| e.to_string())?),
        "originalYesOutputHash": sha(&std::fs::read(dir.join("historical-markouts-v2.json")).map_err(|e| e.to_string())?),
        "originalYesCardsExact": 108, "originalYesSensitivityCardsExact": 108,
        "sourceDays": receipt["sourceDays"],
        "quoteSpanDays": (result["capture"]["sources"]["quotes"]["through"].as_f64().unwrap_or(0.0)
            - result["capture"]["sources"]["quotes"]["firstAt"].as_f64().unwrap_or(0.0)) / 86400000.0,
        "sourceBytes": receipt["sourceBytes"], "sources": result["capture"]["sources"],
        "quoteRows": m["yes"]["quoteRows"], "markets": m["yes"]["markets"],
        "candidates": 216, "diagnosticPaths": 432, "workers": 8, "sideWorkers": 4,
        "timing": {"wallMs": wall_ms, "sourceVerifyMs": receipt["captureMs"],
            "combinedSideReplayMs": receipt["replayMs"],
            "quoteRereadAndOutputMs": wall_ms - receipt["captureMs"].as_f64().unwrap_or(0.0)
                - receipt["replayMs"].as_f64().unwrap_or(0.0)},
        "projection": m["projection"], "canonicalDepthChecks": m["canonicalDepthChecks"],
        "interpretation": m["yes"]["interpretation"], "yes": side("yes"), "no": side("no"),
        "zeroSlippageColumn": "arithmetic_bound_on_identical_selected_paths_not_a_new_strategy_or_execution_result",
        "activeHashesUnchanged": V::Object(before), "activeCohortReplaced": false,
        "originalWalletsReset": false, "fees": 0, "networkRequests": 0, "competitionOrders": 0,
        "profitabilityEstablished": false});
    if endpoints {
        let first = read(&dir.join("historical-side-markouts-v1.json"))?;
        for label in ["yes", "no"] {
            eq(&m[label]["cards"], &first["markouts"][label]["cards"])?;
            eq(&m[label]["bookBoundarySensitivityCards"], &first["markouts"][label]["bookBoundarySensitivityCards"])?;
            let view = &m[format!("{}EndpointSensitivity", label)];
            e[label]["endpointSensitivityCards"] = json!(view["cards"].as_array().cloned()
                .unwrap_or_default().iter().map(|c| json!({"strategy": c["strategy"],
                    "scenario": c["scenario"]["id"], "featurePolicy": c["featurePolicy"],
                    "summary": c["summary"]})).collect::<Vec<V>>());
            e[label]["endpointInterpretation"] = view["interpretation"].clone();
        }
        e["originalSideCardsExact"] = json!(432);
        e["initialSideStudyOutputHash"] = json!(sha(&std::fs::read(dir.join("historical-side-markouts-v1.json")).map_err(|e| e.to_string())?));
        e["initialSideStudyEvidenceHash"] = json!(sha(&std::fs::read(root.join("data/native-quote-sides-20261004.json")).map_err(|e| e.to_string())?));
        e["endpointSensitivityMs"] = m["endpointSensitivityMs"].clone();
    }
    for (key, hash) in &source_hashes {
        let target = r.join("native-sources").join(&engine_hash).join(key.replacen("backend-rust/", "", 1));
        let b = std::fs::read(root.join(key)).map_err(|e| e.to_string())?;
        eq(&json!(sha(&b)), hash)?;
        std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        if target.exists() {
            eq(&json!(sha(&std::fs::read(&target).map_err(|e| e.to_string())?)), hash)?;
        } else {
            write_excl(&target, &b)?;
        }
    }
    let build = r.join("native-builds").join(e["binaryHash"].as_str().unwrap_or(""));
    std::fs::create_dir_all(&build).map_err(|e| e.to_string())?;
    for f in ["baal-backend.exe", "libunwind.dll"] {
        let target = build.join(f);
        let b = std::fs::read(exe.parent().unwrap_or(Path::new(".")).join(f)).map_err(|e| e.to_string())?;
        if target.exists() {
            eq(&json!(sha(&std::fs::read(&target).map_err(|e| e.to_string())?)), &json!(sha(&b)))?;
        } else {
            write_excl(&target, &b)?;
        }
    }
    write_excl(&root.join(format!("data/native-quote-{}-20261004.json",
        if endpoints { "endpoints" } else { "sides" })),
        format!("{}\n", js_stringify(&e)).as_bytes())?;
    let sides_log: Vec<V> = ["yes", "no"].iter().map(|label| {
        let label_s = label.to_string();
        json!({"side": label_s,
            "uniqueSignals": e[label]["uniqueSignalEvents"],
            "uniqueIntervals": e[label]["uniqueObservedIntervals"],
            "baseline": e[label]["cards"].as_array().cloned().unwrap_or_default().iter()
                .filter(|c| c["scenario"].as_str() == Some("baseline_3s")
                    && c["featurePolicy"].as_str() == Some("original_self_comparison_v1")
                    && c["strategy"].as_str().map(|s| s.contains("_r0_e2_")).unwrap_or(false))
                .map(|c| json!({"id": c["strategy"], "episodes": c["episodes"], "n": c["observed"],
                    "mean": c["net"]["mean"], "beforeSlippage": c["quotedBeforeSlippageMean"],
                    "status": c["statusCounts"]})).collect::<Vec<V>>(),
            "endpoints": e[label]["endpointSensitivityCards"].as_array().cloned().unwrap_or_default().iter()
                .filter(|c| c["scenario"].as_str() == Some("baseline_3s")
                    && c["featurePolicy"].as_str() == Some("original_self_comparison_v1")
                    && c["strategy"].as_str().map(|s| s.contains("_r0_e2_")).unwrap_or(false))
                .map(|c| {
                    let mut o = serde_json::Map::new();
                    o.insert("id".into(), c["strategy"].clone());
                    if let Some(s) = c["summary"].as_object() { for (k, v) in s { o.insert(k.clone(), v.clone()); } }
                    V::Object(o)
                }).collect::<Vec<V>>()})
    }).collect();
    // JS drops undefined keys in JSON.stringify — omit endpointSensitivityMs when unset.
    let mut log_obj = serde_json::Map::new();
    for (k, v) in [("candidates", e["candidates"].clone()), ("diagnosticPaths", e["diagnosticPaths"].clone()),
        ("timing", e["timing"].clone())] { log_obj.insert(k.into(), v); }
    if let Some(esm) = e.as_object().and_then(|o| o.get("endpointSensitivityMs")) {
        log_obj.insert("endpointSensitivityMs".into(), esm.clone());
    }
    for (k, v) in [("canonicalDepthChecks", e["canonicalDepthChecks"].clone()),
        ("originalYesCardsExact", json!(108)), ("originalSideCardsExact", e["originalSideCardsExact"].clone()),
        ("sides", json!(sides_log)), ("profitabilityEstablished", json!(false))] {
        log_obj.insert(k.into(), v);
    }
    println!("{}", js_stringify(&V::Object(log_obj)));
    Ok(e)
}
