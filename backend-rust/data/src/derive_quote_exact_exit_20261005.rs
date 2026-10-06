// data/derive-quote-exact-exit-20261005.mjs — derive an exact-exit sensitivity
// dataset from the frozen quote dataset (idempotent).
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::Path;

fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn write_wx(path: &Path, body: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, body).map_err(|e| e.to_string())
}
fn strip_suffix(s: &V) -> V {
    match s {
        V::String(t) => json!(t.strip_suffix("-exact-exit").unwrap_or(t.as_str())),
        other => other.clone(),
    }
}
fn with_exact_exit(id: &V) -> V {
    match id { V::String(t) => json!(format!("{}-exact-exit", t)), other => other.clone() }
}

/// JS normalized(data): policies id de-suffixed + exitSlippageTicks=1,
/// quotes/rows policyId de-suffixed, marked rows counterfactualNetTicks-1.
fn normalized(data: &V) -> V {
    let mut out = serde_json::Map::new();
    out.insert("features".into(), data["features"].clone());
    out.insert("sources".into(), data["sources"].clone());
    out.insert("policies".into(), json!(data["policies"].as_array().cloned().unwrap_or_default()
        .iter().map(|p| {
            let mut p = p.clone();
            p["id"] = strip_suffix(&p["id"]);
            p["exitSlippageTicks"] = json!(1);
            p
        }).collect::<Vec<_>>()));
    out.insert("quotes".into(), json!(data["quotes"].as_array().cloned().unwrap_or_default()
        .iter().map(|q| {
            let mut q = q.clone();
            q["policyId"] = strip_suffix(&q["policyId"]);
            q
        }).collect::<Vec<_>>()));
    out.insert("rows".into(), json!(data["rows"].as_array().cloned().unwrap_or_default()
        .iter().map(|r| {
            let mut r = r.clone();
            r["policyId"] = strip_suffix(&r["policyId"]);
            if r["kind"].as_str() == Some("marked") {
                r["counterfactualNetTicks"] = json!(
                    r["counterfactualNetTicks"].as_f64().unwrap_or(f64::NAN) - 1.0);
            }
            r
        }).collect::<Vec<_>>()));
    V::Object(out)
}

pub fn run() -> Result<V, String> {
    let source = "data/realtime-quote-dataset-20261005.json";
    let out = "data/realtime-quote-exact-exit-dataset-20261005.json";
    let raw = std::fs::read(source).map_err(|e| e.to_string())?;
    let original: V = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let sha256 = sha256(&raw);
    let expected = read_json(Path::new("data/realtime-quote-dataset-summary-20261005.json"))?
        ["sha256"].as_str().unwrap_or("").to_string();
    if sha256 != expected { return Err("Original frozen dataset changed".into()); }
    if Path::new(out).exists() {
        let derived = read_json(Path::new(out))?;
        let lhs = normalized(&derived);
        let rhs = json!({"features": original["features"], "sources": original["sources"],
            "policies": original["policies"], "quotes": original["quotes"],
            "rows": original["rows"]});
        // assert.deepEqual — key order-insensitive structural equality.
        if lhs != rhs { return Err("assertion failed".into()); }
        return Ok(json!({"mode": "verified_existing_exact_exit_derivation",
            "sourceSha256": sha256,
            "rows": original["rows"].as_array().map(|a| a.len()).unwrap_or(0),
            "changedLabels": original["rows"].as_array().cloned().unwrap_or_default().iter()
                .filter(|r| r["kind"].as_str() == Some("marked")).count()}));
    }
    let mut data = original.clone();
    if let V::Array(ps) = &mut data["policies"] {
        for p in ps.iter_mut() {
            p["id"] = with_exact_exit(&p["id"]);
            p["exitSlippageTicks"] = json!(0);
        }
    }
    if let V::Array(qs) = &mut data["quotes"] {
        for q in qs.iter_mut() { q["policyId"] = with_exact_exit(&q["policyId"]); }
    }
    if let V::Array(rs) = &mut data["rows"] {
        for r in rs.iter_mut() {
            r["policyId"] = with_exact_exit(&r["policyId"]);
            if r["kind"].as_str() == Some("marked") {
                r["counterfactualNetTicks"] = json!(
                    r["counterfactualNetTicks"].as_f64().unwrap_or(f64::NAN) + 1.0);
            }
        }
    }
    data["derivation"] = json!({"source": source, "sha256": sha256,
        "operation": "Add one tick to supported one-sided markouts; paired, unfilled and censored outcomes unchanged.",
        "assumption": "Exact subsequent executable bid for one share; strict-through maker fills and uncertain execution remain counterfactual.",
        "stressModel": "data/realtime-quote-selectors-20261005.json"});
    write_wx(Path::new(out),
        format!("{}\n", serde_json::to_string(&data).map_err(|e| e.to_string())?).as_bytes())?;
    Ok(json!({"mode": "created_exact_exit_sensitivity", "sourceSha256": sha256,
        "rows": data["rows"].as_array().map(|a| a.len()).unwrap_or(0)}))
}
