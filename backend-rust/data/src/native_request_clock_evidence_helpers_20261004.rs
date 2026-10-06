// data/native-request-clock-evidence-helpers-20261004.mjs — aggregate/service checks.
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn read_json(file: &Path) -> Result<V, String> {
    let b = std::fs::read(file).map_err(|e| e.to_string())?;
    serde_json::from_slice(&b).map_err(|e| e.to_string())
}

fn runtime_dir() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}

fn kill0(pid: i64) -> Result<(), String> {
    let st = Command::new("/bin/kill").arg("-0").arg(pid.to_string()).status().map_err(|e| e.to_string())?;
    if !st.success() { return Err(format!("process {} not running", pid)); }
    Ok(())
}

fn now_iso() -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0);
    baal_service_runtime::contract::ms_to_iso(ms)
}

pub fn aggregate_future(cards: &[V]) -> Result<V, String> {
    let mut groups: Vec<(String, V)> = Vec::new();
    let mut events = HashSet::new();
    let mut instruments = HashSet::new();
    let mut total_fills = 0i64;
    let mut cards_with_fills = 0i64;
    for c in cards {
        let p = &c["point"];
        let mut x = serde_json::Map::new();
        x.insert("fills".into(), json!(c["fills"].as_array().map(|a| a.len() as i64).unwrap_or(0)));
        for k in ["completedTrades", "cash", "realizedPnl", "conservativePnl", "liquidationPnl", "valuation", "openShares"] {
            x.insert(k.into(), p[k].clone());
        }
        let key = js_stringify(&V::Object(x.clone()));
        if let Some((_, g)) = groups.iter_mut().find(|(k, _)| *k == key) {
            let n = g["wallets"].as_i64().unwrap_or(0);
            g["wallets"] = json!(n + 1);
        } else {
            let mut g = serde_json::Map::new();
            g.insert("wallets".into(), json!(0));
            for (k, v) in &x { g.insert(k.clone(), v.clone()); }
            groups.push((key, V::Object(g)));
        }
        total_fills += c["fills"].as_array().map(|a| a.len() as i64).unwrap_or(0);
        if c["fills"].as_array().map(|a| !a.is_empty()).unwrap_or(false) { cards_with_fills += 1; }
        for f in c["fills"].as_array().cloned().unwrap_or_default() {
            events.insert(js_stringify(&json!([f["instrument"].clone(), f["action"].clone(), f["at"].clone(), f["price"].clone(), f["quantity"].clone()])));
            if let Some(s) = f["instrument"].as_str() { instruments.insert(s.to_string()); }
        }
    }
    Ok(json!({"totalFills": total_fills, "cardsWithFills": cards_with_fills,
        "uniqueFillEvents": events.len() as i64, "distinctFilledInstruments": instruments.len() as i64,
        "groups": groups.into_iter().map(|(_, g)| g).collect::<Vec<V>>(),
        "walletResultsAreCorrelated": true}))
}

pub fn verify_services() -> Result<V, String> {
    let runtime = runtime_dir();
    let pids = read_json(Path::new("data/current-runtime-pids.json"))?;
    let eq = |a: &V, b: &V| -> Result<(), String> {
        if a != b { return Err(format!("assert.equal failed: {} != {}", js_stringify(a), js_stringify(b))); }
        Ok(())
    };
    eq(&pids["collector"], &json!(19596))?;
    eq(&pids["paper"], &json!(5624))?;
    eq(&pids["dashboard"], &json!(6456))?;
    for pid in [19596i64, 5624, 6456] { kill0(pid)?; }
    let stats = read_json(&runtime.join("stats.json"))?;
    eq(&stats["runtime"]["pid"], &pids["collector"])?;
    eq(&stats["runtime"]["status"], &json!("running"))?;
    let mut banks = serde_json::Map::new();
    for ty in ["baal-pair", "no-maker", "probability-receipt"] {
        let a = read_json(&runtime.join(format!("{}-active.json", ty)))?;
        banks.insert(ty.to_string(), a["bankHash"].clone());
    }
    let expected = [("baal-pair", "08ae516f21230247569d0683453ddaa4b06aaff48b006dfa63cf321e98f7a8d1"),
        ("no-maker", "cc484dc03298f0f8b3c7f455805fb52783dab9b1b86b5b2fa1ae14eafa2a8064"),
        ("probability-receipt", "3e445d6b7f436cab79245ed588efa627734d8d2a4a6a6b788ac2873582ed4ef2")];
    for (k, h) in expected {
        eq(banks.get(k).unwrap(), &json!(h))?;
    }
    let budget = read_json(&runtime.join("coordinated-network-budget.json"))?;
    eq(&budget["maxReadsPerMinute"], &json!(20))?;
    eq(&budget["writesEnabled"], &json!(false))?;
    eq(&read_json(&runtime.join("competition-activation.json"))?["enabled"], &json!(false))?;
    Ok(json!({"pids": {"collector": 19596, "paper": 5624, "dashboard": 6456}, "bankHashes": V::Object(banks),
        "collectorStatus": stats["runtime"]["status"], "readCap": 20, "writes": false,
        "contestActivation": false, "verifiedAt": now_iso()}))
}
