// data/record-native-markouts-milestone-20261004.mjs — native markouts milestone record.
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
fn eq(a: &V, b: &V, msg: Option<String>) -> Result<(), String> {
    if a == b { Ok(()) } else { Err(msg.unwrap_or_else(|| fail(&format!("assert.equal: {} !== {}", js_stringify(a), js_stringify(b))))) }
}
fn a_true(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err(fail("assertion failed")) }
}
fn re_word(text: &str, pat: &str) -> bool {
    match text.find(pat) {
        Some(i) => {
            let next = text.as_bytes().get(i + pat.len());
            next.is_none_or(|c| !c.is_ascii_alphanumeric() && *c != b'_')
        }
        None => false,
    }
}
// fs.writeFileSync(file, JSON.stringify(milestone)+'\n', {flag:'wx'}) — exclusive create.
fn write_exclusive(f: &Path, v: &V) -> Result<(), String> {
    let body = format!("{}\n", js_stringify(v));
    let mut fd = std::fs::OpenOptions::new().write(true).create_new(true).open(f).map_err(|e| e.to_string())?;
    use std::io::Write;
    fd.write_all(body.as_bytes()).map_err(|e| e.to_string())
}
fn compact(c: &V) -> V {
    json!({"strategy": c["strategy"], "scenario": c["scenario"]["id"], "featurePolicy": c["featurePolicy"],
        "episodes": c["summary"]["episodes"], "observed": c["summary"]["observed"],
        "statusCounts": c["summary"]["statusCounts"],
        "markets": c["summary"]["signalMarkets"].as_object().map(|m| m.len() as i64).unwrap_or(0),
        "signalDays": c["summary"]["signalDays"], "net": c["summary"]["quotedNetMarkout"],
        "midpointMove": c["summary"]["meanMidpointMove"], "spreadCost": c["summary"]["meanSpreadCost"],
        "slippageCost": c["summary"]["meanSlippageCost"], "latencyAskMove": c["summary"]["meanLatencyAskMove"]})
}

pub fn run(root: &Path) -> Result<V, String> {
    let e = read(&root.join("data/native-quote-markouts-v2-20261004.json"))?;
    let p = read(&root.join("data/native-markouts-preservation-20261004.json"))?;
    let tap = std::fs::read_to_string(root.join("data/native-markouts-checks-20261004.tap")).map_err(|e| e.to_string())?;
    a_true(re_word(&tap, "tests 158"))?;
    a_true(re_word(&tap, "fail 0"))?;
    a_true(re_word(&tap, "skipped 0"))?;
    eq(&p["exactOriginalCards"], &json!(72), None)?;
    eq(&p["activeCohortReplaced"], &json!(false), None)?;
    let cards = e["cards"].as_array().cloned().unwrap_or_default();
    let mut strategies: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    for c in &cards {
        let k = c["strategy"].as_str().unwrap_or("").to_string();
        strategies.insert(k, json!({"id": c["strategy"], "parameters": c["parameters"]}));
    }
    let mut scenarios: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    for c in &cards {
        let k = c["scenario"]["id"].as_str().unwrap_or("").to_string();
        scenarios.insert(k, c["scenario"].clone());
    }
    let recovery_pairs: Vec<V> = cards.iter()
        .filter(|c| c["featurePolicy"].as_str() == Some("prior_distinct_minute_nonfalling_v2"))
        .map(|c| {
            let original = cards.iter().find(|o| o["strategy"] == c["strategy"]
                && o["scenario"]["id"] == c["scenario"]["id"]
                && o["featurePolicy"].as_str() == Some("original_self_comparison_v1"));
            let Some(original) = original else {
                return Err::<V, _>(fail("Cannot read properties of undefined (reading 'summary')"));
            };
            Ok(json!({"strategy": c["strategy"], "scenario": c["scenario"]["id"],
                "sameAggregateOutcomes": js_stringify(&c["summary"]) == js_stringify(&original["summary"])}))
        }).collect::<Result<Vec<V>, String>>()?;
    let milestone = json!({"schemaVersion": 1, "at": now_iso(), "claimComment": 5982524583i64,
        "status": "native_chronological_quote_diagnostics_implemented_measured_and_preserved",
        "mode": e["mode"], "cohort": e["cohort"], "declarationHash": e["declarationHash"],
        "engineHash": e["engineHash"], "binaryHash": e["binaryHash"], "runtimeLibraryHash": e["runtimeLibraryHash"],
        "sourceHashes": e["sourceHashes"], "outputHash": e["outputHash"],
        "fullLocalEvidenceHash": sha(&std::fs::read(root.join("data/native-quote-markouts-v2-20261004.json")).map_err(|e| e.to_string())?),
        "sourceDays": e["sourceDays"],
        "quoteSpanDays": (e["sources"]["quotes"]["through"].as_f64().unwrap_or(0.0)
            - e["sources"]["quotes"]["firstAt"].as_f64().unwrap_or(0.0)) / 86400000.0,
        "sourceBytes": e["sourceBytes"], "sourceQuotes": e["quoteRows"],
        "sourceEvents": e["sources"].as_object().map(|m| m.values().map(|s| s["records"].as_i64().unwrap_or(0)).sum::<i64>()).unwrap_or(0),
        "sources": e["sources"], "markets": e["markets"], "candidates": e["candidates"],
        "diagnosticPaths": e["diagnosticPaths"], "distinctFeaturePasses": e["distinctFeaturePasses"],
        "fallingReboundFeatureRejections": e["fallingReboundFeatureRejections"],
        "recoveryRejectionCountsAreCorrelatedFeatureEvaluations": true,
        "timing": e["timing"], "workers": e["workers"], "interpretation": e["interpretation"],
        "strategies": strategies.values().cloned().collect::<Vec<V>>(),
        "scenarios": scenarios.values().cloned().collect::<Vec<V>>(),
        "cards": cards.iter().map(compact).collect::<Vec<V>>(),
        "bookBoundarySensitivityCards": e["bookBoundarySensitivityCards"].as_array().cloned().unwrap_or_default().iter().map(compact).collect::<Vec<V>>(),
        "recoveryComparison": {"pairs": recovery_pairs.len() as i64,
            "sameAggregateOutcomes": recovery_pairs.iter().filter(|p| p["sameAggregateOutcomes"].as_bool() == Some(true)).count() as i64,
            "comparison": recovery_pairs},
        "initialBoundaryCensoredStudy": e["initialBoundaryCensoredStudy"],
        "tests": {"node": 158, "rust": 7, "failures": 0, "skips": 0},
        "originalWallets": {"cards": 72, "allOriginalFillsExact": true, "activeCohortReplaced": false,
            "engineSourceUnchanged": true, "evaluationSourceUnchanged": true, "quoteReplaySourceUnchanged": true,
            "evidence": "data/native-markouts-preservation-20261004.json"},
        "live": {"through": p["through"], "paperGroups": p["paperGroups"], "collector": p["collector"],
            "pids": p["pids"], "persistentProcesses": 3, "additionalPersistentProcesses": 0},
        "fees": 0, "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false,
        "next": "Use fast replay to investigate different economic signals; no promotion from these reused sparse quote paths. Apply recovery correction only through a separately declared future cohort, retaining active V1 accounting."});
    write_exclusive(&root.join("data/native-markouts-milestone-20261004.json"), &milestone)?;
    let mut state = read(&root.join("data/quant-development-state.json"))?;
    state["updatedAt"] = milestone["at"].clone();
    state["nativeMarkoutsMilestone"] = json!({"evidence": "data/native-markouts-milestone-20261004.json",
        "status": milestone["status"], "sourceDays": milestone["sourceDays"], "quoteRows": e["quoteRows"],
        "diagnosticPaths": 216, "wallMs": e["timing"]["wallMs"], "tests": milestone["tests"],
        "recoveryComparison": milestone["recoveryComparison"], "allOriginalCardsExact": 72,
        "profitabilityEstablished": false});
    state["goalTurn"] = json!({"at": milestone["at"], "classification": "progress",
        "milestone": "Native parallel chronological quote markouts, versioned recovery correction, independent feed/boundary diagnostics and live original-wallet equivalence completed",
        "requiredWorkRemaining": true});
    state["nextConcreteMilestone"] = milestone["next"].clone();
    state["nextImplementationMilestones"] = json!([
        {"priority": 1, "task": "Different economic signal comparison on chronological data with quote/depth distinction, missing/censored denominators and untouched future evidence. Current dip and rebound settings remain negative or sparsely observed."},
        {"priority": 2, "task": "Correct the rebound slope in a named future paper cohort with independent wallets; preserve V1 engine, original journals, sources and accounting."},
        {"priority": 3, "task": "Keep single collector and actual later-request paper exits healthy; improve measured verified-prefix admission speed without adding network clients or contest authority."}]);
    atomic_json(&root.join("data/quant-development-state.json"), &state)?;
    let mut monitor = read(&root.join("data/daenerys-monitor.json"))?;
    monitor["updatedAt"] = milestone["at"].clone();
    monitor["lastCheckAt"] = milestone["at"].clone();
    monitor["nativeMarkoutsMilestone"] = state["nativeMarkoutsMilestone"].clone();
    monitor["lastEngineeringMilestone"] = milestone["status"].clone();
    monitor["nextConcreteMilestone"] = milestone["next"].clone();
    atomic_json(&root.join("data/daenerys-monitor.json"), &monitor)?;
    let out = json!({"status": milestone["status"], "candidates": 108, "diagnosticPaths": 216,
        "wallMs": e["timing"]["wallMs"],
        "recoveryComparison": format!("{}/{}", milestone["recoveryComparison"]["sameAggregateOutcomes"].as_i64().unwrap_or(0),
            milestone["recoveryComparison"]["pairs"].as_i64().unwrap_or(0)),
        "tests": milestone["tests"], "goalClassification": "progress", "profitabilityEstablished": false});
    println!("{}", js_stringify(&out));
    Ok(milestone)
}
