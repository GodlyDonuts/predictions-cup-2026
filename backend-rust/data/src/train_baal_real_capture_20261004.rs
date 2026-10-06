// data/train-baal-real-capture-20261004.mjs — real-capture portfolio training.
use baal_collector::model::{digest, validate_config};
use baal_collector::portfolio_bank::{train_portfolio_bank, train_portfolio_bank_parallel};
use baal_collector::training_horizons::{parse_training_horizons, DEFAULT_HORIZONS};
use baal_collector::training_source::read_training_source;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn rss_mib() -> f64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let kb: f64 = s.lines().find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok()).unwrap_or(0.0);
    (kb / 1024.0).round()
}
fn peak_rss_mib() -> f64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let kb: f64 = s.lines().find(|l| l.starts_with("VmHWM:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok()).unwrap_or(0.0);
    (kb / 1024.0).round()
}
fn write_exclusive(path: &Path, text: &str) -> Result<(), String> {
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, text.as_bytes()))
        .map_err(|e| e.to_string())
}

pub fn run(root: &Path, argv: &[String]) -> Result<V, String> {
    let runtime = rt();
    let args = &argv[2.min(argv.len())..];
    let mut options = indexmap::IndexMap::new();
    let mut i = 0;
    while i < args.len() {
        let key = args[i].clone();
        let next = args.get(i + 1).cloned();
        if !["--cohort", "--end-byte", "--workers", "--horizons"].contains(&key.as_str())
            || options.contains_key(&key) || next.is_none() {
            return Err("Invalid research arguments".into());
        }
        options.insert(key, next.unwrap());
        i += 2;
    }
    let name = options.get("--cohort").cloned()
        .unwrap_or_else(|| "baal-net-20261004-a".to_string());
    let re = regex::Regex::new("^baal-net-20261004-[a-z0-9-]{1,16}$").map_err(|e| e.to_string())?;
    if !re.is_match(&name) { return Err("Invalid research cohort".into()); }
    let end_byte: Option<u64> = if options.contains_key("--end-byte") {
        let v: f64 = options["--end-byte"].parse().map_err(|_| "x".to_string())?;
        if !(v.fract() == 0.0 && v.is_finite() && v.abs() <= 9007199254740991.0) || v < 1.0 {
            return Err("Invalid frozen prefix".into());
        }
        Some(v as u64)
    } else { None };
    let workers_v: f64 = options.get("--workers").map(|s| s.parse().unwrap_or(f64::NAN))
        .unwrap_or(1.0);
    if !(workers_v.fract() == 0.0 && workers_v.is_finite()) || workers_v < 1.0 || workers_v > 8.0 {
        return Err("Invalid research worker count".into());
    }
    let workers = workers_v as i64;
    let horizons: Vec<i64> = if let Some(h) = options.get("--horizons") {
        parse_training_horizons(h)?
    } else { DEFAULT_HORIZONS.to_vec() };
    let dir = runtime.join("research-cohorts").join(&name);
    if dir.exists() { return Err("Research cohort exists; no overwrite".into()); }
    let universe: V = serde_json::from_slice(
        &std::fs::read(root.join("live-paper/price-universe.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let base_config: V = serde_json::from_slice(
        &std::fs::read(root.join("live-paper/config.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let market_ids: Vec<V> = {
        let mut seen = HashSet::new();
        universe["exchanges"].as_array().cloned().unwrap_or_default().iter()
            .filter(|x| x["option"].as_str() == Some("YES"))
            .filter_map(|x| x["marketId"].clone().as_str().map(str::to_string))
            .filter(|m| seen.insert(m.clone())).map(|m| json!(m)).collect()
    };
    let mut cfg = base_config.as_object().cloned().unwrap_or_default();
    cfg.insert("strategyUniverse".into(), json!("diversified-dip-v1"));
    cfg.insert("marketIds".into(), V::Array(market_ids));
    cfg.insert("initialCash".into(), json!("100.000000"));
    cfg.insert("cashReserve".into(), json!("90.000000"));
    cfg.insert("maxExposure".into(), json!("10.000000"));
    cfg.insert("feeBps".into(), json!(0));
    cfg.insert("latencyMs".into(), json!(3000));
    cfg.insert("orderShares".into(), json!(1));
    cfg.insert("maxPositionShares".into(), json!(1));
    cfg.insert("depthFractionBps".into(), json!(100));
    cfg.insert("slippageTicks".into(), json!(1));
    let config = V::Object(cfg);
    validate_config(&config)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let started = Instant::now();
    let mut stage = "verify_original_source";
    write_exclusive(&dir.join("config.json"),
        &serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?)?;
    let outcome: Result<V, String> = (|| {
        let source = read_training_source(&runtime.join("journal.jsonl"), end_byte)?;
        let metadata = json!({"at": ms_to_iso(now_ms()), "name": name,
            "sourceBytes": source.source_bytes() as i64,
            "sourceRecords": source.source_records() as i64,
            "sourceHash": source.source_hash(),
            "sourceThrough": source.final_event_clock(),
            "sourceFile": "journal.jsonl",
            "verifyMs": started.elapsed().as_secs_f64() * 1000.0,
            "markets": config["marketIds"].as_array().map(|a| a.len() as i64).unwrap_or(0),
            "configHash": digest(&config), "fees": 0, "sourcePreserved": true,
            "competitionOrders": 0,
            "scope": "Historical screening; later untouched prospective data is still required."});
        write_exclusive(&dir.join("source.json"),
            &serde_json::to_string_pretty(&metadata).map_err(|e| e.to_string())?)?;
        stage = "fit_and_portfolio_selection";
        let mut log_obj = metadata.as_object().cloned().unwrap_or_default();
        log_obj.insert("stage".into(), json!(stage));
        log_obj.insert("rssMiB".into(), json!(rss_mib()));
        println!("{}", js_stringify(&V::Object(log_obj)));
        let fit_started = Instant::now();
        let mut fit_options = serde_json::Map::new();
        fit_options.insert("family".into(), json!("ridge"));
        fit_options.insert("horizons".into(), json!(horizons));
        let fit_options_v = V::Object(fit_options);
        let fit_start_inner = fit_started;
        let on_progress = move |x: &V| {
            let mut o = x.as_object().cloned().unwrap_or_default();
            o.insert("elapsedMs".into(),
                json!((fit_start_inner.elapsed().as_secs_f64() * 1000.0).round()));
            o.insert("rssMiB".into(), json!(rss_mib()));
            println!("{}", js_stringify(&V::Object(o)));
        };
        let bank = if workers == 1 {
            train_portfolio_bank(None, Some(&source), &config, &fit_options_v, &on_progress)?
        } else {
            let mut o = fit_options_v.as_object().cloned().unwrap_or_default();
            o.insert("workers".into(), json!(workers));
            train_portfolio_bank_parallel(None, Some(&source), &config, &V::Object(o), &on_progress)?
        };
        write_exclusive(&dir.join("bank.json"),
            &serde_json::to_string_pretty(&bank).map_err(|e| e.to_string())?)?;
        let p = &bank["portfolioSelection"];
        let mut result = metadata.as_object().cloned().unwrap_or_default();
        result.insert("at".into(), json!(ms_to_iso(now_ms())));
        result.insert("status".into(), json!("completed"));
        result.insert("bankHash".into(), json!(digest(&bank)));
        result.insert("family".into(), bank["family"].clone());
        result.insert("horizons".into(), json!(horizons));
        result.insert("workers".into(), json!(workers));
        result.insert("snapshots".into(), bank["acceptedSnapshots"].clone());
        result.insert("attempted".into(), bank["candidatesAttempted"].clone());
        result.insert("trained".into(), bank["modelsTrained"].clone());
        result.insert("modelMarginWallets".into(), p["testedModelMarginPairs"].clone());
        result.insert("decision".into(), p["decision"].clone());
        result.insert("selected".into(), bank["candidatePlan"]["candidates"].clone());
        result.insert("windows".into(), p["windows"].clone());
        result.insert("holdout".into(), p["holdout"].clone());
        let validation: Vec<V> = p["validation"].as_array().cloned().unwrap_or_default()
            .iter().take(5).map(|x| json!({"id": x["id"], "eligible": x["eligible"],
                "rejectionReasons": x["rejectionReasons"], "blockPnl": x["blockPnl"],
                "last": x["points"].as_array().and_then(|a| a.last()).cloned().unwrap_or(V::Null)}))
            .collect();
        result.insert("validationLeaders".into(), V::Array(validation));
        result.insert("labelDiagnostics".into(), bank["labelDiagnostics"].clone());
        result.insert("fitAndReplayMs".into(),
            json!(fit_started.elapsed().as_secs_f64() * 1000.0));
        result.insert("totalMs".into(),
            json!(started.elapsed().as_secs_f64() * 1000.0));
        result.insert("peakRssMiB".into(), json!(peak_rss_mib()));
        result.insert("productionActivation".into(), json!(false));
        result.insert("prospectiveWalletStarted".into(), json!(false));
        result.insert("profitabilityEstablished".into(), json!(false));
        let result = V::Object(result);
        write_exclusive(&dir.join("result.json"),
            &serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?)?;
        write_exclusive(&root.join(format!("data/{}-result.json", name)),
            &format!("{}\n", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?))?;
        std::fs::write(root.join("data/baal-real-training-result-20261004.json"),
            format!("{}\n", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?))
            .map_err(|e| e.to_string())?;
        println!("{}", js_stringify(&json!({"status": result["status"],
            "decision": result["decision"], "trained": result["trained"],
            "modelMarginWallets": result["modelMarginWallets"],
            "holdout": result["holdout"], "elapsedMs": result["totalMs"],
            "peakRssMiB": result["peakRssMiB"], "competitionOrders": 0})));
        Ok(result)
    })();
    match outcome {
        Ok(v) => Ok(v),
        Err(e) => {
            let safe: HashSet<&str> = ["Observation exceeds event clock",
                "Missing account exclusions", "Invalid request chronology",
                "Book version rollback", "Observation time moved backward",
                "Conflicting equal book version",
                "Pre-invalidation snapshot cannot recover"].into_iter().collect();
            let code_re = regex::Regex::new(
                "^(Source|Incomplete|No accepted|Insufficient|Need at least|Invalid|Chronological|Training source|Journal)")
                .map_err(|e| e.to_string())?;
            let code = if safe.contains(e.as_str()) || code_re.is_match(&e) {
                e.clone()
            } else {
                "RESEARCH_FAILURE".to_string()
            };
            let failed = json!({"at": ms_to_iso(now_ms()), "name": name, "status": "failed",
                "stage": stage, "reason": code, "errorType": "Error",
                "frames": Vec::<V>::new(),
                "elapsedMs": started.elapsed().as_secs_f64() * 1000.0,
                "sourcePreserved": true, "competitionOrders": 0});
            write_exclusive(&dir.join("failure.json"),
                &serde_json::to_string_pretty(&failed).map_err(|e| e.to_string())?)?;
            std::fs::write(root.join("data/baal-real-training-failure-20261004.json"),
                format!("{}\n", serde_json::to_string_pretty(&failed).map_err(|e| e.to_string())?))
                .map_err(|e| e.to_string())?;
            eprintln!("{}", js_stringify(&failed));
            Err("RESEARCH_FAILURE".into())
        }
    }
}
