// data/run-realtime-spread-economics-20261005.mjs — realtime spread economics run.
use baal_collector::realtime_spread_economics::{create_realtime_spread_economics,
    realtime_economics_policies};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn now_iso() -> String {
    ms_to_iso(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0))
}
fn write_pretty_excl(f: &Path, v: &V) -> Result<(), String> {
    let mut fd = std::fs::OpenOptions::new().write(true).create_new(true).open(f).map_err(|e| e.to_string())?;
    fd.write_all(format!("{}\n", serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).as_bytes())
        .map_err(|e| e.to_string())
}
fn sha_file(f: &Path) -> Result<String, String> {
    sha(&std::fs::read(f).map_err(|e| e.to_string())?)
}
fn sha(b: &[u8]) -> Result<String, String> { Ok(sha256(b)) }

pub fn run(root: &Path) -> Result<V, String> {
    let rt_dir = rt();
    let (start, through) = ("realtime-2026-10-04T21-59", "realtime-2026-10-05T16-15");
    let out = root.join("data/realtime-spread-economics-20261005.json");
    let mut names: Vec<String> = std::fs::read_dir(&rt_dir).map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().and_then(|e| e.file_name().to_str().map(String::from)))
        .filter(|x| x.ends_with(".jsonl") && x.as_str() >= start
            && x.as_str() <= format!("{}\u{ffff}", through).as_str()
            && x.starts_with("realtime-"))
        .collect();
    names.sort();
    let mut sources: Vec<V> = Vec::new();
    let mut study = create_realtime_spread_economics(None, |_| V::Null, |_| {}, |_| {}, |_| {});
    let started = Instant::now();
    let mut rows = 0i64;
    write_pretty_excl(&root.join("data/realtime-spread-economics-policy-20261005.json"), &json!({
        "frozenAt": now_iso(), "policies": realtime_economics_policies(), "files": names,
        "mode": "development_diagnostic_no_profit_claim"}))?;
    for file in &names {
        let full = rt_dir.join(file);
        let size = std::fs::metadata(&full).map_err(|e| e.to_string())?.len();
        let bytes = std::fs::read(&full).map_err(|e| e.to_string())?;
        let h = sha256(&bytes);
        let mut count = 0i64;
        // readline with crlfDelay Infinity: split on \n and strip a trailing \r.
        for line in bytes.split(|b| *b == b'\n') {
            let mut line = line;
            if line.ends_with(b"\r") { line = &line[..line.len() - 1]; }
            if line.is_empty() { continue; }
            let row: V = serde_json::from_slice(line).map_err(|e| e.to_string())?;
            study.consume(&row, file);
            count += 1;
            rows += 1;
        }
        if std::fs::metadata(&full).map_err(|e| e.to_string())?.len() != size {
            return Err("SOURCE_CHANGED".into());
        }
        sources.push(json!({"file": file, "bytes": size, "sha256": h, "rows": count}));
        println!("{}", js_stringify(&json!({"file": file, "rows": count,
            "elapsedMs": (started.elapsed().as_secs_f64() * 1000.0).round()})));
    }
    let mut r = study.report();
    r["sources"] = json!(sources);
    r["elapsedMs"] = json!(started.elapsed().as_secs_f64() * 1000.0);
    r["rows"] = json!(rows);
    write_pretty_excl(&out, &r)?;
    let mut log = r.clone();
    if let Some(o) = log.as_object_mut() {
        if let Some(policies) = o.get_mut("policies") {
            if let Some(arr) = policies.as_array_mut() {
                for p in arr.iter_mut() {
                    if let Some(po) = p.as_object_mut() { po.remove("outcomes"); }
                }
            }
        }
        o.remove("sources");
    }
    println!("{}", serde_json::to_string_pretty(&log).map_err(|e| e.to_string())?);
    Ok(r)
}
