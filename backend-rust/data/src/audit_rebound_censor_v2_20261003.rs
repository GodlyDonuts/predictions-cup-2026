// data/audit-rebound-censor-v2-20261003.mjs — same-capture terminal-censor
// correction audit: v1 vs v2 rebound studies on the original sealed bytes.
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> { if ok { Ok(()) } else { Err(code.into()) } }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn canonical(v: &V) -> Result<String, String> { baal_service_runtime::json_text::canonical_json(v) }
fn parse_lines(b: &[u8]) -> Result<Vec<V>, String> {
    let s = String::from_utf8_lossy(b);
    // JS: b.toString().split('\n').slice(0,-1) — drop the (empty) tail element.
    let mut parts: Vec<&str> = s.split('\n').collect();
    parts.pop();
    parts.iter().map(|l| serde_json::from_str(l).map_err(|e| e.to_string())).collect()
}

/// observed(result) — completed-economics projection used for the comparison.
fn observed(result: &V) -> serde_json::Map<String, V> {
    let mut out = serde_json::Map::new();
    for c in result["cases"].as_array().cloned().unwrap_or_default() {
        if let V::Object(branches) = &c["branches"] {
            for (strategy, b) in branches {
                if b.is_null() { continue; }
                if let V::Object(trials) = &b["trials"] {
                    for (latency, t) in trials {
                        for kind in ["quote", "depth"] {
                            if t[kind]["stage"].as_str() != Some("entered") { continue; }
                            if let V::Object(exits) = &t[kind]["exits"] {
                                for (h, x) in exits {
                                    if x["status"].as_str() != Some("observed") { continue; }
                                    out.insert(format!("{}:{}:{}:{}:{}",
                                        c["id"].as_str().unwrap_or(""), strategy, latency, kind, h),
                                        json!({"entryPrice": t[kind]["entryPrice"],
                                            "entryAt": t[kind]["entryAvailableAt"],
                                            "rawMarkout": x["rawMarkout"],
                                            "adverseMarkout": x["adverseMarkout"],
                                            "exitPrice": x["exitPrice"],
                                            "sourceHash": x["sourceHash"]}));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    out
}
fn statuses(result: &V) -> serde_json::Map<String, V> {
    result["groups"].as_object().cloned().unwrap_or_default().into_iter().map(|(phase, g)| {
        (phase, V::Object(g["strategies"].as_object().cloned().unwrap_or_default().into_iter()
            .map(|(name, s)| (name, s["outcomes"].clone())).collect()))
    }).collect()
}

pub fn run() -> Result<V, String> {
    let rt = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let original = read_json(Path::new("data/rebound-path-subsequent-20261003-v1.json"))?;
    let price_bytes = {
        let b = std::fs::read(rt.join("market-prices.jsonl")).map_err(|e| e.to_string())?;
        b[..original["sources"]["price"]["bytes"].as_u64().unwrap_or(0) as usize].to_vec()
    };
    let book_bytes = {
        let b = std::fs::read(rt.join("public-books.jsonl")).map_err(|e| e.to_string())?;
        b[..original["sources"]["book"]["bytes"].as_u64().unwrap_or(0) as usize].to_vec()
    };
    for (k, b) in [("price", &price_bytes), ("book", &book_bytes)] {
        need(sha256(b) == original["sources"][k]["sha256"].as_str().unwrap_or(""),
            "ORIGINAL_CAPTURE_CHANGED")?;
    }
    let base_bank = read_json(Path::new("data/dip-markout-protocol-20261003-v1.json"))?;
    let universe = read_json(Path::new("live-paper/price-universe.json"))?;
    let live_paper_dir = std::env::current_dir().map_err(|e| e.to_string())?.join("live-paper");
    let dips = baal_collector::dip_markout_study::study_dip_markouts(
        &price_bytes, &book_bytes, &universe, &base_bank, &live_paper_dir)?;
    let prices = parse_lines(&price_bytes)?;
    let books = parse_lines(&book_bytes)?;
    let cutoff = original["cutoff"].as_str().unwrap_or("");
    let before = baal_collector::rebound_path_study::compare_rebound_paths(
        dips["events"].as_array().cloned().unwrap_or_default().as_slice(), &prices, &books,
        cutoff, &baal_collector::rebound_path_study::rebound_study_policy())?;
    let after = baal_collector::rebound_path_study_v2::compare_rebound_paths(
        dips["events"].as_array().cloned().unwrap_or_default().as_slice(), &prices, &books,
        cutoff, &baal_collector::rebound_path_study_v2::rebound_study_policy())?;
    need(canonical(&json!(before["cases"]))? == canonical(&json!(original["cases"]))?,
        "ORIGINAL_REPLAY_DIFFERS")?;
    need(canonical(&json!(observed(&before)))? == canonical(&json!(observed(&after)))?,
        "COMPLETED_ECONOMICS_CHANGED")?;
    let ids = |r: &V| -> Vec<V> {
        r["cases"].as_array().cloned().unwrap_or_default().iter().map(|c| c["id"].clone()).collect()
    };
    need(canonical(&json!(ids(&before)))? == canonical(&json!(ids(&after)))?,
        "OPPORTUNITY_DENOMINATORS_CHANGED")?;
    let obs = observed(&before);
    let result = json!({"at": now_iso(), "mode": "same_capture_terminal_censor_correction_audit",
        "originalBankHash": original["bankHash"], "originalCutoff": original["cutoff"],
        "through": original["through"], "sources": original["sources"],
        "opportunities": before["cases"].as_array().map(|a| a.len()).unwrap_or(0),
        "originalReplayMatched": true, "completedEconomicPaths": obs.len(),
        "completedEconomicsUnchanged": true, "opportunityDenominatorsUnchanged": true,
        "beforeStatuses": V::Object(statuses(&before)), "afterStatuses": V::Object(statuses(&after)),
        "networkRequests": 0, "paperFills": 0, "competitionOrders": 0,
        "profitabilityEstablished": false,
        "limitation": "Retrospective correction audit on the original capture, not a new prospective strategy result. Original v1 artifacts remain unchanged."});
    baal_service_runtime::atomic::atomic_json_canonical(
        Path::new("data/rebound-censor-correction-audit-20261003.json"), &result)?;
    Ok(json!({"opportunities": result["opportunities"],
        "completedEconomicPaths": result["completedEconomicPaths"],
        "completedEconomicsUnchanged": true, "originalReplayMatched": true}))
}
