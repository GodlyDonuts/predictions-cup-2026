// data/record-native-mechanisms-milestone-20261004.mjs — mechanisms milestone record.
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
fn write_exclusive(f: &Path, v: &V) -> Result<(), String> {
    let body = format!("{}\n", js_stringify(v));
    let mut fd = std::fs::OpenOptions::new().write(true).create_new(true).open(f).map_err(|e| e.to_string())?;
    use std::io::Write;
    fd.write_all(body.as_bytes()).map_err(|e| e.to_string())
}

pub fn run(root: &Path) -> Result<V, String> {
    let evidence = read(&root.join("data/native-mechanisms-evidence-20261004.json"))?;
    let checks = std::fs::read_to_string(root.join("data/native-mechanisms-checks-20261004.tap")).map_err(|e| e.to_string())?;
    a_true(re_word(&checks, "tests 169"))?;
    a_true(re_word(&checks, "fail 0"))?;
    a_true(re_word(&checks, "skipped 0"))?;
    eq(&evidence["preservation"]["strictBoundaryAndEndpointCardsExact"], &json!(648), None)?;
    eq(&evidence["preservation"]["originalLiveWalletCardsExact"], &json!(72), None)?;
    for (file, hash) in evidence["protocol"]["sourceHashes"].as_object().cloned().unwrap_or_default() {
        let actual = sha(&std::fs::read(root.join(&file)).or_else(|_| std::fs::read(&file)).map_err(|e| e.to_string())?);
        eq(&json!(actual), &hash, Some(file.clone()))?;
    }
    let mut baseline = serde_json::Map::new();
    for side in ["yes", "no"] {
        let strict: Vec<V> = evidence["sides"][side]["strictCards"].as_array().cloned().unwrap_or_default().iter()
            .filter(|c| c["scenario"]["id"].as_str() == Some("baseline_3s"))
            .map(|c| json!({"strategy": c["strategy"], "episodes": c["summary"]["episodes"],
                "observed": c["summary"]["observed"], "meanQuotedNet": c["summary"]["quotedNetMarkout"]["mean"],
                "quotedSpreadOnlyMean": if c["summary"]["observed"].as_f64().unwrap_or(0.0) != 0.0 {
                    json!(c["summary"]["quotedNetMarkout"]["mean"].as_f64().unwrap_or(0.0)
                        + c["summary"]["meanSlippageCost"].as_f64().unwrap_or(0.0))
                } else { V::Null },
                "statusCounts": c["summary"]["statusCounts"]}))
            .collect();
        let endpoints: Vec<V> = evidence["sides"][side]["endpointCards"].as_array().cloned().unwrap_or_default().iter()
            .filter(|c| c["scenario"]["id"].as_str() == Some("baseline_3s"))
            .map(|c| {
                // {strategy:c.strategy,...c.summary,quotedSpreadOnlyMean}
                let mut o = serde_json::Map::new();
                o.insert("strategy".into(), c["strategy"].clone());
                if let Some(s) = c["summary"].as_object() {
                    for (k, v) in s { o.insert(k.clone(), v.clone()); }
                }
                o.insert("quotedSpreadOnlyMean".into(), if c["summary"]["observed"].as_f64().unwrap_or(0.0) != 0.0 {
                    json!(c["summary"]["meanQuotedNetMarkout"].as_f64().unwrap_or(0.0)
                        + c["summary"]["meanSlippageCost"].as_f64().unwrap_or(0.0))
                } else { V::Null });
                V::Object(o)
            }).collect();
        baseline.insert(side.to_string(), json!({"strict": strict, "endpoints": endpoints}));
    }
    a_true(baseline.values().all(|side| {
        side["strict"].as_array().cloned().unwrap_or_default().iter()
            .all(|c| c["quotedSpreadOnlyMean"].as_f64().unwrap_or(f64::NAN) < 0.0)
            && side["endpoints"].as_array().cloned().unwrap_or_default().iter()
                .all(|c| c["quotedSpreadOnlyMean"].as_f64().unwrap_or(f64::NAN) < 0.0)
    }))?;
    let future_file = "data/native-mechanisms-future-1791141201465.json";
    let future = read(&root.join(future_file))?;
    eq(&future["protocolHash"], &evidence["protocolHash"], None)?;
    eq(&future["signalAfter"], &evidence["protocol"]["signalAfter"], None)?;
    let milestone = json!({"schemaVersion": 1, "at": now_iso(), "claimComment": 5983314037i64,
        "frozenProtocolComment": 5983420326i64,
        "status": "native_fixed_mechanisms_compared_and_future_cutoff_verified",
        "cases": 36, "timing": evidence["timing"], "sourceDays": evidence["sourceDays"],
        "quoteSpanDays": evidence["quoteSpanDays"], "quoteRows": evidence["quoteRows"],
        "markets": evidence["markets"], "protocolHash": evidence["protocolHash"],
        "engineHash": evidence["protocol"]["engineHash"], "binaryHash": evidence["protocol"]["binaryHash"],
        "fullDevelopmentOutputHash": evidence["fullOutputHash"],
        "historicalEvidenceHash": sha(&std::fs::read(root.join("data/native-mechanisms-evidence-20261004.json")).map_err(|e| e.to_string())?),
        "protocol": "data/native-mechanisms-protocol-20261004.json",
        "fullSummaries": "data/native-mechanisms-evidence-20261004.json",
        "baseline": V::Object(baseline),
        "quotedSpreadOnlyColumn": "Arithmetic cost bound on identical selected quoted paths, not a refitted strategy, executable fill or separate zero-slippage forward cohort.",
        "futureProbe": {"evidence": future_file, "evidenceHash": sha(&std::fs::read(root.join(future_file)).map_err(|e| e.to_string())?),
            "fullOutputHash": future["fullOutputHash"], "signalAfter": future["signalAfter"],
            "uniqueSignalEventsPerSide": future["sides"]["yes"]["uniqueSignalEvents"],
            "observations": future["sides"]["yes"]["strictCards"].as_array().cloned().unwrap_or_default().iter().map(|c| c["summary"]["observed"].as_i64().unwrap_or(0)).sum::<i64>()
                + future["sides"]["no"]["strictCards"].as_array().cloned().unwrap_or_default().iter().map(|c| c["summary"]["observed"].as_i64().unwrap_or(0)).sum::<i64>(),
            "currentRightCensoring": true, "liveWalletsAdded": 0},
        "preservation": evidence["preservation"], "tests": {"node": 169, "rust": 9, "failures": 0, "skips": 0},
        "frozenBanksPreserved": true, "originalWalletsReset": false, "activeCohortReplaced": false,
        "fees": 0, "networkRequests": 0, "competitionOrders": 0, "additionalPersistentProcesses": 0,
        "profitabilityEstablished": false,
        "conclusion": "All fixed baseline continuation/reversal cases were negative over this captured period, including before the extra modeled slippage cushion. Reversal has small positive midpoint movement which falls short of observed spread. No new executable strategy is promoted.",
        "next": "Use the existing native source and book machinery to test source-backed fair probabilities or verified market relationships; short-window price-only drift has not covered spread. Preserve this protocol for separate future probes, without forcing an entry or blocking useful development."});
    write_exclusive(&root.join("data/native-mechanisms-milestone-20261004.json"), &milestone)?;
    let mut state = read(&root.join("data/quant-development-state.json"))?;
    state["updatedAt"] = milestone["at"].clone();
    state["nativeMechanismsMilestone"] = json!({"evidence": "data/native-mechanisms-milestone-20261004.json",
        "status": milestone["status"], "cases": 36, "wallMs": evidence["timing"]["wallMs"],
        "allBaselineSpreadOnlyMeansNegative": true, "originalLiveWalletsExact": 72,
        "originalStrictEndpointCardsExact": 648, "tests": milestone["tests"],
        "frozenProtocolComment": 5983420326i64, "signalAfter": milestone["futureProbe"]["signalAfter"],
        "profitabilityEstablished": false});
    state["previousGoalTurnClassification"] = json!("progress");
    state["goalTurn"] = json!({"at": milestone["at"], "classification": "progress",
        "milestone": milestone["status"], "requiredWorkRemaining": true});
    state["nextConcreteMilestone"] = milestone["next"].clone();
    state["nextImplementationMilestones"] = json!([
        {"priority": 1, "task": "Check fresh primary research and verified contract rules for a source-backed fair-probability edge, or a verified conditional settlement relationship, using the existing shared collector and native execution/risk machinery."},
        {"priority": 2, "task": "Finite replay of the frozen mechanism probe only on a new verified prefix; signal request/publication must be after the fixed cutoff. Treat sparse quote labels as diagnostic evidence, not readiness."},
        {"priority": 3, "task": "Keep held-risk observations and paper exits healthy. Four protected books currently fill all discovery slots; never drop required exits to force exploration."}]);
    atomic_json(&root.join("data/quant-development-state.json"), &state)?;
    let mut monitor = read(&root.join("data/daenerys-monitor.json"))?;
    monitor["updatedAt"] = milestone["at"].clone();
    monitor["lastCheckAt"] = milestone["at"].clone();
    monitor["nativeMechanismsMilestone"] = state["nativeMechanismsMilestone"].clone();
    monitor["lastEngineeringMilestone"] = milestone["status"].clone();
    monitor["nextConcreteMilestone"] = milestone["next"].clone();
    atomic_json(&root.join("data/daenerys-monitor.json"), &monitor)?;
    let out = json!({"status": milestone["status"], "cases": 36, "wallMs": evidence["timing"]["wallMs"],
        "allBaselineSpreadOnlyMeansNegative": true, "tests": milestone["tests"],
        "originalLiveWalletsExact": 72, "originalCardsExact": 648,
        "futureCutoff": milestone["futureProbe"]["signalAfter"], "goalClassification": "progress",
        "profitabilityEstablished": false});
    println!("{}", js_stringify(&out));
    Ok(milestone)
}
