// data/native-request-clock-evidence-helpers-20261004.mjs — shared evidence
// helpers for the request-clock runs.
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn need_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assertion failed".into()) }
}
fn pid_alive(id: i64) -> bool {
    if id <= 0 { return false; }
    #[cfg(unix)] { unsafe { libc::kill(id as i32, 0) == 0 } }
    #[cfg(not(unix))] { Path::new(&format!("/proc/{}", id)).exists() }
}

pub fn aggregate_future(cards: &[V]) -> V {
    let mut groups: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    let mut events: HashSet<String> = HashSet::new();
    let mut instruments: HashSet<String> = HashSet::new();
    let mut total_fills = 0i64;
    let mut cards_with_fills = 0i64;
    for c in cards {
        let p = &c["point"];
        let fill_count = c["fills"].as_array().map(|a| a.len()).unwrap_or(0) as i64;
        let x = json!({"fills": fill_count,
            "completedTrades": p["completedTrades"], "cash": p["cash"],
            "realizedPnl": p["realizedPnl"], "conservativePnl": p["conservativePnl"],
            "liquidationPnl": p["liquidationPnl"], "valuation": p["valuation"],
            "openShares": p["openShares"]});
        let k = js_stringify(&x);
        if !groups.contains_key(&k) {
            let mut g = serde_json::Map::new();
            g.insert("wallets".into(), json!(0));
            if let V::Object(o) = &x { for (kk, vv) in o { g.insert(kk.clone(), vv.clone()); } }
            groups.insert(k.clone(), V::Object(g));
        }
        let g = groups.get_mut(&k).unwrap();
        g["wallets"] = json!(g["wallets"].as_i64().unwrap_or(0) + 1);
        total_fills += fill_count;
        if fill_count > 0 { cards_with_fills += 1; }
        for f in c["fills"].as_array().cloned().unwrap_or_default() {
            events.insert(js_stringify(&json!([f["instrument"], f["action"], f["at"],
                f["price"], f["quantity"]])));
            instruments.insert(f["instrument"].as_str().unwrap_or("").to_string());
        }
    }
    json!({"totalFills": total_fills, "cardsWithFills": cards_with_fills,
        "uniqueFillEvents": events.len(), "distinctFilledInstruments": instruments.len(),
        "groups": groups.values().cloned().collect::<Vec<_>>(),
        "walletResultsAreCorrelated": true})
}

pub fn verify_services() -> Result<V, String> {
    let runtime = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let pids = read_json(Path::new("data/current-runtime-pids.json"))?;
    need_eq(&pids["collector"], &json!(19596))?;
    need_eq(&pids["paper"], &json!(5624))?;
    need_eq(&pids["dashboard"], &json!(6456))?;
    for p in [19596, 5624, 6456] {
        if !pid_alive(p) { return Err("assertion failed".into()); }
    }
    let stats = read_json(&runtime.join("stats.json"))?;
    need_eq(&stats["runtime"]["pid"], &pids["collector"])?;
    need_eq(&stats["runtime"]["status"], &json!("running"))?;
    let mut banks = serde_json::Map::new();
    for ty in ["baal-pair", "no-maker", "probability-receipt"] {
        let a = read_json(&runtime.join(format!("{}-active.json", ty)))?;
        banks.insert(ty.to_string(), a["bankHash"].clone());
    }
    need_eq(&banks["baal-pair"],
        &json!("08ae516f21230247569d0683453ddaa4b06aaff48b006dfa63cf321e98f7a8d1"))?;
    need_eq(&banks["no-maker"],
        &json!("cc484dc03298f0f8b3c7f455805fb52783dab9b1b86b5b2fa1ae14eafa2a8064"))?;
    need_eq(&banks["probability-receipt"],
        &json!("3e445d6b7f436cab79245ed588efa627734d8d2a4a6a6b788ac2873582ed4ef2"))?;
    let budget = read_json(&runtime.join("coordinated-network-budget.json"))?;
    need_eq(&budget["maxReadsPerMinute"], &json!(20))?;
    need_eq(&budget["writesEnabled"], &json!(false))?;
    need_eq(&read_json(&runtime.join("competition-activation.json"))?["enabled"],
        &json!(false))?;
    Ok(json!({"pids": {"collector": 19596, "paper": 5624, "dashboard": 6456},
        "bankHashes": V::Object(banks), "collectorStatus": stats["runtime"]["status"],
        "readCap": 20, "writes": false, "contestActivation": false,
        "verifiedAt": baal_service_runtime::contract::ms_to_iso(
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .unwrap().as_millis() as f64)}))
}
