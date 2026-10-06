// data/replay-funded-quote-selectors-20261005.mjs — funded quote selector replay.
use baal_collector::funded_quote_replay::{create_funded_quote_wallet_with_selector, FundedQuoteWallet};
use baal_collector::quote_outcome_forecast::quote_outcome_forecast;
use baal_collector::realtime_spread_economics::create_realtime_spread_economics;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

fn now_iso() -> String {
    ms_to_iso(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0))
}
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn write_pretty_excl(f: &Path, v: &V) -> Result<(), String> {
    use std::io::Write;
    let mut fd = std::fs::OpenOptions::new().write(true).create_new(true).open(f).map_err(|e| e.to_string())?;
    fd.write_all(format!("{}\n", serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).as_bytes())
        .map_err(|e| e.to_string())
}
fn arr_len(x: &V) -> i64 { x.as_array().map(|a| a.len() as i64).unwrap_or(0) }

struct Study {
    policy: V,
    segment: &'static str,
    selector: &'static str,
    wallet: Rc<RefCell<FundedQuoteWallet>>,
    study: baal_collector::realtime_spread_economics::RealtimeSpreadEconomics<
        Box<dyn FnMut(V) -> V>, Box<dyn FnMut(&V)>, Box<dyn FnMut(&V)>, Box<dyn FnMut(&V)>>,
}

pub fn run(root: &Path) -> Result<V, String> {
    let rt = rt();
    let model_file = "data/realtime-quote-selectors-20261005.json";
    let raw = std::fs::read(root.join(model_file)).map_err(|e| e.to_string())?;
    let report: V = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let active_file = read(&rt.join("realtime-session.json"))?["file"].as_str().unwrap_or("").to_string();
    let original = report["sources"].as_array().cloned().unwrap_or_default();
    let mut additional: Vec<String> = std::fs::read_dir(&rt).map_err(|e| e.to_string())?
        .filter_map(|x| x.ok().map(|d| d.file_name().to_string_lossy().to_string()))
        .filter(|f| f.starts_with("realtime-") && f.ends_with(".jsonl")
            && f.as_str() > "realtime-2026-10-05T16-38" && f.as_str() < "realtime-2026-10-05T17-50"
            && *f != active_file)
        .collect();
    additional.sort();
    let mut sources = original.clone();
    for file in &additional {
        let bytes = std::fs::metadata(rt.join(file)).map_err(|e| e.to_string())?.len();
        sources.push(json!({"file": file, "bytes": bytes as i64}));
    }
    let started = Instant::now();
    let mut studies: Vec<Study> = Vec::new();
    for bank in report["banks"].as_array().cloned().unwrap_or_default() {
        if bank["status"].as_str() != Some("research_selector_frozen") { continue; }
        for (segment, start_at) in [("development_later_period", bank["testStartsAt"].clone()),
            ("subsequent_sealed_sessions", json!("2026-10-05T16:38:18.000Z"))] {
            for selected in [false, true] {
                let selector: Box<dyn Fn(&V) -> bool> = if selected {
                    let report_c = report.clone();
                    let bank_c = bank.clone();
                    Box::new(move |q: &V| {
                        quote_outcome_forecast(&report_c, &bank_c, &q["x"])
                            .map(|r| r["selected"].as_bool() == Some(true)).unwrap_or(false)
                    })
                } else {
                    Box::new(|_| true)
                };
                let wallet = Rc::new(RefCell::new(create_funded_quote_wallet_with_selector(
                    &json!({"policyId": bank["policy"]["id"], "startAt": start_at}), selector)?));
                let w1 = wallet.clone();
                let w2 = wallet.clone();
                let w3 = wallet.clone();
                let study = create_realtime_spread_economics(
                    Some(vec![bank["policy"].clone()]),
                    Box::new(move |q: V| -> V {
                        match w1.borrow_mut().decide_quote(&q) {
                            Ok(b) => json!(b),
                            Err(e) => panic!("{}", e),
                        }
                    }) as Box<dyn FnMut(V) -> V>,
                    Box::new(|_: &V| {}) as Box<dyn FnMut(&V)>,
                    Box::new(move |e: &V| {
                        if let Err(x) = w2.borrow_mut().on_fill(e) { panic!("{}", x); }
                    }) as Box<dyn FnMut(&V)>,
                    Box::new(move |e: &V| {
                        if let Err(x) = w3.borrow_mut().on_outcome(e) { panic!("{}", x); }
                    }) as Box<dyn FnMut(&V)>,
                );
                studies.push(Study { policy: bank["policy"]["id"].clone(), segment,
                    selector: if selected { "frozen_arrival_and_conditional_value" } else { "unfiltered_control" },
                    wallet, study });
            }
        }
    }
    let manifest_file = root.join("data/funded-quote-replay-policy-20261005.json");
    let out_file = root.join("data/funded-quote-replay-20261005.json");
    if out_file.exists() { return Err("FROZEN_REPLAY_EXISTS".into()); }
    let manifest = json!({"schemaVersion": 1, "frozenAt": now_iso(), "modelSha256": sha256(&raw),
        "sources": sources.iter().map(|s| {
            let mut m = s.as_object().cloned().unwrap_or_default();
            let sh = m.shift_remove("sha256").unwrap_or(V::Null);
            m.insert("sha256".into(), sh);
            V::Object(m)
        }).collect::<Vec<V>>(),
        "policies": report["banks"].as_array().cloned().unwrap_or_default().iter()
            .map(|b| b["policy"].clone()).collect::<Vec<V>>(),
        "initialCash": "100.000000", "maximumCommitted": "5.000000",
        "maximumCumulativePurchases": "25.000000", "maximumMarkets": 3, "fees": "0.000000",
        "competitionOrders": 0});
    write_pretty_excl(&manifest_file, &manifest)?;
    for source in &mut sources {
        let file = rt.join(source["file"].as_str().unwrap_or(""));
        let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&bytes);
        let mut rows = 0i64;
        for line in text.split('\n') {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.is_empty() { continue; }
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            for s in &mut studies { s.study.consume(&row, source["file"].as_str().unwrap_or(""))?; }
            rows += 1;
        }
        let sha = sha256(&bytes);
        if std::fs::metadata(&file).map_err(|e| e.to_string())?.len() as i64 != source["bytes"].as_i64().unwrap_or(-1)
            || (!source["sha256"].is_null() && sha != source["sha256"].as_str().unwrap_or("")) {
            return Err("SOURCE_CHANGED".into());
        }
        source["sha256"] = json!(sha);
        source["rows"] = json!(rows);
        println!("{}", baal_service_runtime::json_text::js_stringify(
            &json!({"file": source["file"], "rows": rows,
                "elapsedMs": (started.elapsed().as_secs_f64() * 1000.0).round() as i64})));
    }
    let wallets: Vec<V> = studies.iter().map(|s| {
        json!({"policy": s.policy, "segment": s.segment, "selector": s.selector,
            "wallet": s.wallet.borrow().report(), "sourceMetrics": s.study.report()["metrics"]})
    }).collect();
    let result = json!({"schemaVersion": 1, "frozenAt": now_iso(), "modelFile": model_file,
        "modelSha256": sha256(&raw), "sources": sources, "additionalSealedSessions": additional.len() as i64,
        "elapsedMs": started.elapsed().as_secs_f64() * 1000.0, "wallets": wallets,
        "networkRequests": 0, "competitionOrders": 0, "executionAuthority": false,
        "profitabilityEstablished": false,
        "limitations": ["Development-period data were previously examined; subsequent sealed sessions are a new selector comparison, not prospective live competition performance.",
            "Censored quote paths retain known holdings and unresolved reservations. Missing economic observations are not zero returns.",
            "Arrival probabilities and conditional-value estimates are uncalibrated; no confidence or guaranteed profit follows from a point forecast.",
            "Control and selected wallets are separate counterfactual alternatives and cannot be added as capital or profit."]});
    write_pretty_excl(&out_file, &result)?;
    let summary_wallets: Vec<V> = result["wallets"].as_array().cloned().unwrap_or_default().iter().map(|w| {
        let mut x = w.as_object().cloned().unwrap_or_default();
        let mut wl = w["wallet"].as_object().cloned().unwrap_or_default();
        wl.insert("fills".into(), json!(arr_len(&w["wallet"]["fills"])));
        wl.insert("outcomes".into(), json!(arr_len(&w["wallet"]["outcomes"])));
        x.insert("wallet".into(), V::Object(wl));
        V::Object(x)
    }).collect();
    let mut summary = result.clone();
    summary["wallets"] = json!(summary_wallets);
    write_pretty_excl(&root.join("data/funded-quote-replay-summary-20261005.json"), &summary)?;
    let log_wallets: Vec<V> = summary["wallets"].as_array().cloned().unwrap_or_default().iter().map(|w| {
        json!({"policy": w["policy"], "segment": w["segment"], "selector": w["selector"],
            "cash": w["wallet"]["cash"], "realized": w["wallet"]["realizedPnl"],
            "buys": w["wallet"]["buyFills"], "closed": w["wallet"]["closedTrades"],
            "reserved": w["wallet"]["reserved"], "basis": w["wallet"]["inventoryCost"],
            "complete": w["wallet"]["completeEconomicObservation"], "counts": w["wallet"]["counts"]})
    }).collect();
    println!("{}", serde_json::to_string_pretty(&json!({"elapsedMs": result["elapsedMs"],
        "additional": additional.len() as i64, "wallets": log_wallets})).map_err(|e| e.to_string())?);
    Ok(result)
}
fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
