// data/prepare-native-cache-host-20261004.mjs — install and smoke the frozen
// native dip cache host (v2).
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::native_dip_host_v2::{native_dip_cache_host_identity, open_native_dip_cache_host};
use baal_collector::native_dip_proof_v1::verify_native_dip_result;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use serde_json::{json, Value as V};
use std::io::Write;
use std::path::Path;

fn assert_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assert".into()) }
}
pub fn run() -> Result<V, String> {
    let r = runtime_dir();
    let read = |f: &str| read_json(Path::new(f));
    let prep = read("data/native-cache-runtime-prepared-20261004.json")?;
    let benchmark = read("data/native-cache-benchmark-20261004.json")?;
    let dir = format!("{r}/native-capture-caches/{}", prep["directory"].as_str().unwrap_or(""));
    let binding_file = format!("{dir}/binding.json");
    let binding = read(&binding_file)?;
    let cohort = binding["cohort"].as_str().unwrap_or("");
    let cohort_dir = format!("{r}/research-cohorts/{cohort}");
    let old_binding = read(&format!("{cohort_dir}/native-clock-binding.json"))?;
    let universe = read("live-paper/price-universe.json")?;
    let declaration = read(&format!("{cohort_dir}/declaration.json"))?;
    let checks = std::fs::read_to_string("data/native-cache-all-checks-20261004.tap").map_err(|e| e.to_string())?;
    if !(checks.contains("pass 132") && checks.contains("fail 0") && checks.contains("skipped 0")) {
        return Err("assert".into());
    }
    assert_eq(&benchmark["scorecardsExact"], &json!(100))?;
    assert_eq(&benchmark["oldModelEngineCardsExact"], &json!(100))?;
    assert_eq(&json!(sha(&std::fs::read(&binding_file).map_err(|e| e.to_string())?)), &prep["bindingHash"])?;
    assert_eq(&binding["modelOriginalEngineHash"], &old_binding["engineHash"])?;
    assert_eq(&binding["declarationHash"], &old_binding["declarationHash"])?;
    let budget = read(&format!("{r}/coordinated-network-budget.json"))?;
    assert_eq(&budget["maxReadsPerMinute"], &json!(20))?;
    assert_eq(&budget["writesEnabled"], &json!(false))?;
    assert_eq(&budget["executionOwner"], &json!("Daenerys"))?;
    assert_eq(&read(&format!("{r}/competition-activation.json"))?["enabled"], &json!(false))?;
    let pointer = read(&format!("{cohort_dir}/native-host-latest.json"))?;
    let seed_bytes = std::fs::read(format!("{cohort_dir}/{}", pointer["output"].as_str().unwrap_or(""))).map_err(|e| e.to_string())?;
    let seed: V = serde_json::from_slice(&seed_bytes).map_err(|e| e.to_string())?;
    let receipt_bytes = std::fs::read(format!("{cohort_dir}/{}.receipt.json", pointer["output"].as_str().unwrap_or(""))).map_err(|e| e.to_string())?;
    let receipt: V = serde_json::from_slice(&receipt_bytes).map_err(|e| e.to_string())?;
    assert_eq(&json!(sha(&seed_bytes)), &pointer["outputHash"])?;
    assert_eq(&receipt["outputHash"], &pointer["outputHash"])?;
    assert_eq(&receipt["binaryHash"], &old_binding["binaryHash"])?;
    assert_eq(&receipt["engineHash"], &old_binding["engineHash"])?;
    assert_eq(&receipt["sourceHash"], &seed["capture"]["source_hash"])?;
    assert_eq(&receipt["sourceBytes"], &seed["capture"]["source_bytes"])?;
    assert_eq(&receipt["competitionOrders"], &json!(0))?;
    verify_native_dip_result(&seed, &declaration, &old_binding, &universe, None)?;
    std::fs::OpenOptions::new().write(true).create_new(true).open(format!("{dir}/seed.json"))
        .and_then(|mut f| f.write_all(&seed_bytes)).map_err(|e| e.to_string())?;
    std::fs::OpenOptions::new().write(true).create_new(true).open(format!("{dir}/seed.json.receipt.json"))
        .and_then(|mut f| f.write_all(&receipt_bytes)).map_err(|e| e.to_string())?;
    let active = json!({
        "schemaVersion": 1, "mode": "frozen_native_dip_cache_host_v2", "cohort": cohort,
        "cacheDirectory": prep["directory"], "bindingHash": prep["bindingHash"],
        "hostSourceHashes": native_dip_cache_host_identity(Path::new("."))?,
        "intervalMs": 30000, "workers": 8,
        "seed": {"output": "seed.json", "outputHash": pointer["outputHash"],
            "binaryHash": old_binding["binaryHash"]},
    });
    std::fs::OpenOptions::new().write(true).create_new(true).open(format!("{r}/native-dip-cache-active.json"))
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&active).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    let host = open_native_dip_cache_host(
        Path::new(&r), Path::new("."), &universe, None, None, None,
    )?.ok_or("assert")?;
    host.kick();
    let s = host.status();
    if s["status"] != json!("running") { return Err(format!("assert: {}", s["lastError"])); }
    assert_eq(&s["engineHash"], &binding["engineHash"])?;
    assert_eq(&s["candidates"], &json!(100))?;
    assert_eq(&s["hostRevision"], &json!(2))?;
    if s["verifiedAt"].is_null() { return Err("assert".into()); }
    host.close();
    let e = json!({
        "schemaVersion": 1, "at": ms_to_iso(now_ms() as f64), "claim": 5981279291i64,
        "status": "cached_native_continuation_prepared", "cohort": cohort,
        "activeHash": s["activeHash"], "hostSourceHashes": active["hostSourceHashes"],
        "bindingHash": prep["bindingHash"], "declarationHash": s["declarationHash"],
        "engineHash": s["engineHash"], "binaryHash": s["binaryHash"],
        "originalModelEngineHash": old_binding["engineHash"],
        "source": {"hash": s["sourceHash"], "bytes": s["sourceBytes"],
            "records": s["sourceRecords"], "through": s["through"]},
        "seed": {"outputHash": pointer["outputHash"],
            "sourceHash": seed["capture"]["source_hash"],
            "sourceBytes": seed["capture"]["source_bytes"]},
        "cacheHash": s["cacheHash"], "cacheBytes": s["cacheBytes"],
        "lastWallMs": s["lastWallMs"], "heldMarketIds": s["heldMarketIds"],
        "candidates": s["candidates"], "uniqueFillEvents": s["uniqueFillEvents"],
        "groups": s["groups"],
        "checks": {"nodePassed": 132, "nodeFailed": 0, "rustPassed": 2, "rustFailed": 0, "skipped": 0},
        "intervalMs": 30000, "fees": 0, "parametersTuned": false, "walletsReset": false,
        "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false,
    });
    atomic_json(Path::new("data/native-cache-host-preparation-20261004.json"), &e)?;
    Ok(json!({
        "status": e["status"], "source": e["source"], "wallMs": e["lastWallMs"],
        "candidates": e["candidates"], "checks": e["checks"], "held": e["heldMarketIds"],
        "competitionOrders": 0,
    }))
}
