// data/native-cache-preservation-20261004.mjs — snapshot active banks and the
// cache binding before/after the native cache work (mode: before|after).
use baal_collector::native_journal_restore::source_prefix_hash;
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn need_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assertion failed".into()) }
}
fn need(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err("assertion failed".into()) }
}

pub fn run(mode: &str) -> Result<V, String> {
    let r = localappdata()?.join("SigTradingCup").join("paper-runtime");
    need(mode == "before" || mode == "after")?;
    let file = "data/native-cache-preservation-20261004.json";
    let prior = if mode == "after" { Some(read_json(Path::new(file))?) } else { None };
    let mut banks = serde_json::Map::new();
    for ty in ["baal-pair", "no-maker", "probability-receipt"] {
        let active_file = r.join(format!("{}-active.json", ty));
        let active = read_json(&active_file)?;
        let dir = r.join(format!("{}-cohorts", ty))
            .join(active["cohort"].as_str().unwrap_or(""));
        let journal = dir.join("journal.jsonl");
        let bytes = std::fs::metadata(&journal).map_err(|e| e.to_string())?.len();
        let x = json!({"cohort": active["cohort"],
            "activeHash": sha(&std::fs::read(&active_file).map_err(|e| e.to_string())?),
            "bankHash": sha(&std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?),
            "journalBytes": bytes,
            "journalPrefixHash": source_prefix_hash(&journal, bytes, None)?});
        if let Some(p) = &prior {
            let old = &p["banks"][ty];
            need_eq(&x["activeHash"], &old["activeHash"])?;
            need_eq(&x["bankHash"], &old["bankHash"])?;
            need(bytes >= old["journalBytes"].as_u64().unwrap_or(0))?;
            need_eq(&json!(source_prefix_hash(&journal,
                old["journalBytes"].as_u64().unwrap_or(0), None)?), &old["journalPrefixHash"])?;
        }
        banks.insert(ty.to_string(), x);
    }
    let cohort = "baal-dip-request-clock-20261004-v2";
    let d = r.join("research-cohorts").join(cohort).join("declaration.json");
    let old_active_hash = sha(&std::fs::read(r.join("native-dip-active.json"))
        .map_err(|e| e.to_string())?);
    let old_binding_hash = sha(&std::fs::read(r.join("research-cohorts")
        .join(cohort).join("native-clock-binding.json")).map_err(|e| e.to_string())?);
    let declaration_hash = sha(&std::fs::read(&d).map_err(|e| e.to_string())?);
    if let Some(p) = &prior {
        need_eq(&json!(old_active_hash), &p["oldActiveHash"])?;
        need_eq(&json!(old_binding_hash), &p["oldBindingHash"])?;
        need_eq(&json!(declaration_hash), &p["declarationHash"])?;
    }
    let out = json!({"schemaVersion": 1, "at": now_iso(), "mode": mode,
        "banks": V::Object(banks), "oldActiveHash": old_active_hash,
        "oldBindingHash": old_binding_hash, "declarationHash": declaration_hash,
        "frozenBanksPreserved": true, "originalJournalsPreserved": true,
        "competitionOrders": 0});
    let target = if mode == "before" { file }
        else { "data/native-cache-preservation-after-20261004.json" };
    baal_service_runtime::atomic::atomic_json_canonical(Path::new(target), &out)?;
    Ok(json!({"mode": mode,
        "banks": out["banks"].as_object().map(|m| m.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default(), "preserved": true}))
}
