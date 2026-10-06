// data/run-native-mechanisms-20261004.mjs — native mechanisms run.
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
fn a_true(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err("assertion failed".to_string()) }
}
fn write_compact_excl(f: &Path, v: &V) -> Result<(), String> {
    use std::io::Write;
    let mut fd = std::fs::OpenOptions::new().write(true).create_new(true).open(f).map_err(|e| e.to_string())?;
    fd.write_all(format!("{}\n", js_stringify(v)).as_bytes()).map_err(|e| e.to_string())
}
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

struct Launch { result: V, receipt: V, wall_ms: f64, output_hash: String, name: String }

pub fn run(root: &Path, argv: &[String]) -> Result<V, String> {
    if !argv.iter().all(|x| x == "--future") { return Err("assertion failed".into()); }
    let future = argv.iter().any(|x| x == "--future");
    let runtime = rt();
    let protocol_file = "data/native-mechanisms-protocol-20261004.json";
    let protocol = read(&root.join(protocol_file))?;
    let dir = runtime.join("research-cohorts").join(protocol["cohort"].as_str().unwrap_or(""));
    let base_dir = runtime.join("research-cohorts").join(protocol["baseCohort"].as_str().unwrap_or(""));
    eq(&json!(sha(&std::fs::read(root.join(protocol_file)).map_err(|e| e.to_string())?)),
        &json!(sha(&std::fs::read(dir.join("protocol.json")).map_err(|e| e.to_string())?)))?;
    let executable = runtime.join("native-builds")
        .join(protocol["binaryHash"].as_str().unwrap_or("")).join("baal-backend.exe");
    eq(&json!(sha(&std::fs::read(&executable).map_err(|e| e.to_string())?)), &protocol["binaryHash"])?;
    eq(&json!(sha(&std::fs::read(dir.join("base-declaration.json")).map_err(|e| e.to_string())?)),
        &protocol["baseDeclarationHash"])?;
    let mut before = serde_json::Map::new();
    for file in ["native-quote-active.json", "native-dip-intent-active.json",
        "coordinated-network-budget.json", "competition-activation.json"] {
        before.insert(file.to_string(),
            json!(sha(&std::fs::read(runtime.join(file)).map_err(|e| e.to_string())?)));
    }
    let launch = |command: &str, sources: &Path, name: &str, extra: &[String]| -> Result<Launch, String> {
        let output = dir.join(name);
        if output.exists() { return Err("assertion failed".into()); }
        let start = Instant::now();
        let mut cmd = Command::new(&executable);
        cmd.arg(command).args(["--declaration"]).arg(dir.join("base-declaration.json"))
            .args(["--sources"]).arg(sources)
            .args(["--workers", "8", "--out"]).arg(&output);
        for x in extra { cmd.arg(x); }
        let child = cmd.output().map_err(|e| e.to_string())?;
        if child.status.code() != Some(0) {
            return Err(format!("assert.equal status 0 != {:?}: {}", child.status.code(),
                String::from_utf8_lossy(&child.stderr)));
        }
        let receipt: V = serde_json::from_slice(&child.stdout).map_err(|e| e.to_string())?;
        let result = read(&output)?;
        eq(&result["capture"]["engineHash"], &protocol["engineHash"])?;
        Ok(Launch { result, receipt, wall_ms: start.elapsed().as_secs_f64() * 1000.0,
            output_hash: sha(&std::fs::read(&output).map_err(|e| e.to_string())?), name: name.to_string() })
    };
    let mut sources = dir.join("development.sources.json");
    let stamp = now_ms() as i64;
    let mut prefix_proof = V::Null;
    if future {
        let pointer = read(&base_dir.join("host-latest.json"))?;
        let raw = std::fs::read(base_dir.join(pointer["output"].as_str().unwrap_or(""))).map_err(|e| e.to_string())?;
        eq(&json!(sha(&raw)), &pointer["outputHash"])?;
        let prior: V = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
        let declaration = read(&dir.join("base-declaration.json"))?;
        let mut input = serde_json::Map::new();
        for (key, value) in prior["capture"]["sources"].as_object().cloned().unwrap_or_default() {
            let source_path = runtime.join(match key.as_str() {
                "books" => "public-books.jsonl",
                "quotes" => "market-prices.jsonl",
                "eligibility" => "account-eligibility.jsonl",
                _ => "",
            });
            input.insert(key.clone(), json!({"path": source_path, "bytes": value["bytes"],
                "hash": value["hash"], "checkpoints": [declaration["development"][key].clone()]}));
        }
        a_true(prior["capture"]["sources"]["quotes"]["through"].as_f64().unwrap_or(f64::NAN)
            > protocol["signalAfter"].as_f64().unwrap_or(f64::NAN))
            .map_err(|_| "No post-cutoff quote prefix yet".to_string())?;
        let file = dir.join(format!("future-{}.sources.json", stamp));
        sources = file.clone();
        write_compact_excl(&file, &V::Object(input))?;
        prefix_proof = json!({"hostOutputHash": pointer["outputHash"],
            "hostEngineHash": prior["capture"]["engineHash"],
            "inputHash": sha(&std::fs::read(&file).map_err(|e| e.to_string())?),
            "quotePrefix": prior["capture"]["sources"]["quotes"]});
    }
    let study = launch("quote-mechanisms", &sources,
        &if future { format!("future-{}.json", stamp) } else { "development.json".to_string() },
        &if future { vec!["--signal-after".to_string(), match &protocol["signalAfter"] {
            V::String(s) => s.clone(), other => js_stringify(other) }] } else { vec![] })?;
    let m = &study.result["markouts"];
    eq(&m["candidates"], &json!(36))?;
    eq(&m["fees"], &json!(0))?;
    eq(&m["competitionOrders"], &json!(0))?;
    let mut preservation = V::Null;
    if !future {
        let original = read(&base_dir.join("historical-side-endpoints-v1.json"))?;
        let side = launch("quote-side-endpoints", &sources, "original-path-preservation.json", &[])?;
        for label in ["yes", "no"] {
            eq(&side.result["markouts"][label]["cards"], &original["markouts"][label]["cards"])?;
            eq(&side.result["markouts"][label]["bookBoundarySensitivityCards"],
                &original["markouts"][label]["bookBoundarySensitivityCards"])?;
            eq(&side.result["markouts"][format!("{}EndpointSensitivity", label)]["cards"],
                &original["markouts"][format!("{}EndpointSensitivity", label)]["cards"])?;
        }
        let wallet = launch("quote-replay", &base_dir.join("sides-preservation.sources.json"),
            "original-wallet-preservation.json", &[])?;
        let archived = read(&base_dir.join("sides-preservation-original.json"))?;
        eq(&wallet.result["evaluation"]["scorecards"], &archived["evaluation"]["scorecards"])?;
        preservation = json!({"strictBoundaryAndEndpointCardsExact": 648, "originalLiveWalletCardsExact": 72,
            "priorOutputHash": sha(&std::fs::read(base_dir.join("historical-side-endpoints-v1.json")).map_err(|e| e.to_string())?),
            "newOutputHash": side.output_hash, "originalWalletOutputHash": wallet.output_hash,
            "originalLiveWalletThrough": wallet.result["evaluation"]["evaluationThrough"]});
    }
    for (file, hash) in &before {
        eq(&json!(sha(&std::fs::read(runtime.join(file)).map_err(|e| e.to_string())?)), hash)?;
    }
    let mut summaries = serde_json::Map::new();
    for side in ["yes", "no"] {
        let strict = m["sides"][side]["strict"]["cards"].as_array().cloned().unwrap_or_default();
        if future {
            for card in &strict {
                for episode in card["episodes"].as_array().cloned().unwrap_or_default() {
                    a_true(episode["signal"]["publishedAt"].as_f64().unwrap_or(f64::NAN)
                        > protocol["signalAfter"].as_f64().unwrap_or(f64::NAN))?;
                    a_true(episode["signal"]["requestStartedAt"].as_f64().unwrap_or(f64::NAN)
                        > protocol["signalAfter"].as_f64().unwrap_or(f64::NAN))?;
                }
            }
        }
        let mut signal_events = HashSet::new();
        let mut observed_intervals = HashSet::new();
        for card in &strict {
            for e in card["episodes"].as_array().cloned().unwrap_or_default() {
                signal_events.insert(format!("{}/{}", e["market"].as_str().unwrap_or(""),
                    e["signal"]["sequence"].as_i64().unwrap_or(0)));
                if !e["outcome"].is_null() {
                    observed_intervals.insert(format!("{}/{}/{}", e["market"].as_str().unwrap_or(""),
                        e["entry"]["sequence"].as_i64().unwrap_or(0),
                        e["outcome"]["sequence"].as_i64().unwrap_or(0)));
                }
            }
        }
        summaries.insert(side.to_string(), json!({
            "strictCards": strict.iter().map(|c| json!({"strategy": c["strategy"],
                "parameters": c["parameters"], "scenario": c["scenario"],
                "featurePolicy": c["featurePolicy"], "summary": c["summary"],
                "signalDiagnostics": c["signalDiagnostics"]})).collect::<Vec<V>>(),
            "endpointCards": m["sides"][side]["endpointSensitivity"]["cards"].as_array().cloned()
                .unwrap_or_default().iter().map(|c| json!({"strategy": c["strategy"],
                    "parameters": c["parameters"], "scenario": c["scenario"],
                    "featurePolicy": c["featurePolicy"], "summary": c["summary"]})).collect::<Vec<V>>(),
            "uniqueSignalEvents": signal_events.len() as i64,
            "uniqueObservedIntervals": observed_intervals.len() as i64}));
    }
    let evidence = json!({"schemaVersion": 1, "at": now_iso(), "claimComment": 5983314037i64,
        "status": if future { "frozen_mechanism_post_cutoff_quote_probe" } else { "fixed_mechanism_historical_comparison_completed" },
        "protocolHash": sha(&std::fs::read(root.join(protocol_file)).map_err(|e| e.to_string())?),
        "protocol": protocol, "mode": m["mode"], "signalAfter": m["signalAfter"],
        "sourceDays": study.receipt["sourceDays"], "sourceBytes": study.receipt["sourceBytes"],
        "quoteSpanDays": (study.result["capture"]["sources"]["quotes"]["through"].as_f64().unwrap_or(0.0)
            - study.result["capture"]["sources"]["quotes"]["firstAt"].as_f64().unwrap_or(0.0)) / 86400000.0,
        "quoteRows": m["quoteRows"], "markets": m["markets"], "cases": 36,
        "sources": study.result["capture"]["sources"],
        "timing": {"wallMs": study.wall_ms, "sourceVerifyMs": study.receipt["captureMs"],
            "replayMs": study.receipt["replayMs"],
            "rereadOutputAndProcessMs": study.wall_ms - study.receipt["captureMs"].as_f64().unwrap_or(0.0)
                - study.receipt["replayMs"].as_f64().unwrap_or(0.0)},
        "fullOutputHash": study.output_hash, "fullOutputName": study.name,
        "prefixProof": prefix_proof, "preservation": preservation, "interpretation": m["interpretation"],
        "sides": V::Object(summaries), "activeHashesUnchanged": V::Object(before),
        "activeCohortReplaced": false, "originalWalletsReset": false, "newLiveWallets": 0,
        "fees": 0, "networkRequests": 0, "competitionOrders": 0, "additionalPersistentProcesses": 0,
        "profitabilityEstablished": false});
    let target = if future { root.join(format!("data/native-mechanisms-future-{}.json", stamp)) }
        else { root.join("data/native-mechanisms-evidence-20261004.json") };
    write_compact_excl(&target, &evidence)?;
    let sides_log: Vec<V> = ["yes", "no"].iter().map(|side| {
        json!({"side": side, "uniqueSignals": evidence["sides"][side]["uniqueSignalEvents"],
            "baseline": evidence["sides"][side]["strictCards"].as_array().cloned().unwrap_or_default().iter()
                .filter(|c| c["scenario"]["id"].as_str() == Some("baseline_3s"))
                .map(|c| json!({"strategy": c["strategy"], "episodes": c["summary"]["episodes"],
                    "observed": c["summary"]["observed"], "net": c["summary"]["quotedNetMarkout"]["mean"],
                    "status": c["summary"]["statusCounts"]})).collect::<Vec<V>>(),
            "endpoints": evidence["sides"][side]["endpointCards"].as_array().cloned().unwrap_or_default().iter()
                .filter(|c| c["scenario"]["id"].as_str() == Some("baseline_3s"))
                .map(|c| {
                    let mut o = serde_json::Map::new();
                    o.insert("strategy".into(), c["strategy"].clone());
                    if let Some(s) = c["summary"].as_object() { for (k, v) in s { o.insert(k.clone(), v.clone()); } }
                    V::Object(o)
                }).collect::<Vec<V>>()})
    }).collect();
    let target_rel = if future { format!("data/native-mechanisms-future-{}.json", stamp) }
        else { "data/native-mechanisms-evidence-20261004.json".to_string() };
    println!("{}", js_stringify(&json!({"target": target_rel,
        "cases": 36, "timing": evidence["timing"], "preservation": evidence["preservation"],
        "sides": sides_log, "profitabilityEstablished": false})));
    Ok(evidence)
}
