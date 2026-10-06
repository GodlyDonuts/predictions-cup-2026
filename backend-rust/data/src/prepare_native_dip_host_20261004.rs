// data/prepare-native-dip-host-20261004.mjs — install and smoke the frozen
// native dip paper host (v1).
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::native_dip_host_v1::{native_dip_host_identity, open_native_dip_host};
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
    let cohort = "baal-dip-request-clock-20261004-v2";
    let dir = format!("{r}/research-cohorts/{cohort}");
    let binding_file = format!("{dir}/native-clock-binding.json");
    let binding = read(&binding_file)?;
    let d = read(&format!("{dir}/declaration.json"))?;
    let evidence = read("data/native-request-clock-milestone-20261004.json")?;
    let universe = read("live-paper/price-universe.json")?;
    let budget = read(&format!("{r}/coordinated-network-budget.json"))?;
    assert_eq(&budget["maxReadsPerMinute"], &json!(20))?;
    assert_eq(&budget["writesEnabled"], &json!(false))?;
    assert_eq(&budget["executionOwner"], &json!("Daenerys"))?;
    assert_eq(&read(&format!("{r}/competition-activation.json"))?["enabled"], &json!(false))?;
    let output = std::fs::read_dir(&dir).map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|x| x.file_name().to_string_lossy().to_string()))
        .find(|f| {
            f.len() == 41 && f.ends_with(".json")
                && f[..36].chars().all(|c| c.is_ascii_hexdigit() || c == '-')
                && std::fs::read(format!("{dir}/{f}")).map(|b| sha(&b) == evidence["subsequent"]["outputHash"].as_str().unwrap_or("")).unwrap_or(false)
        }).ok_or("assert")?;
    let result = read(&format!("{dir}/{output}"))?;
    verify_native_dip_result(&result, &d, &binding, &universe, None)?;
    let active = json!({
        "schemaVersion": 1, "mode": "frozen_native_dip_paper_host_v1", "cohort": cohort,
        "bindingHash": sha(&std::fs::read(&binding_file).map_err(|e| e.to_string())?),
        "hostSourceHashes": native_dip_host_identity(Path::new("."))?,
        "intervalMs": 180000, "workers": 8,
        "seed": {"output": output, "outputHash": evidence["subsequent"]["outputHash"]},
    });
    std::fs::OpenOptions::new().write(true).create_new(true).open(format!("{r}/native-dip-active.json"))
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&active).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    let host = open_native_dip_host(
        Path::new(&r), Path::new("."), &universe, None, None, None,
    )?.ok_or("assert")?;
    let started = std::time::Instant::now();
    host.kick();
    let status = host.status();
    if status["status"] != json!("running") { return Err(format!("assert: {}", status["lastError"])); }
    assert_eq(&status["candidates"], &json!(100))?;
    assert_eq(&status["competitionOrders"], &json!(0))?;
    assert_eq(&status["fees"], &json!(0))?;
    host.close();
    let prepared = json!({
        "schemaVersion": 1, "at": ms_to_iso(now_ms() as f64), "claim": 5981079530i64,
        "status": "prepared_against_current_original_capture", "cohort": cohort,
        "activeHash": status["activeHash"], "hostSourceHashes": active["hostSourceHashes"],
        "declarationHash": binding["declarationHash"], "engineHash": binding["engineHash"],
        "binaryHash": binding["binaryHash"], "runtimeLibraryHash": binding["runtimeLibraryHash"],
        "priorSourceBytes": evidence["subsequent"]["source"]["bytes"],
        "sourceBytes": status["sourceBytes"], "sourceHash": status["sourceHash"],
        "sourceRecords": status["sourceRecords"], "through": status["through"],
        "candidates": 100, "heldMarketIds": status["heldMarketIds"],
        "uniqueFillEvents": status["uniqueFillEvents"], "cardsWithFills": status["cardsWithFills"],
        "groups": status["groups"], "replayWallMs": started.elapsed().as_secs_f64() * 1000.0,
        "fees": 0, "additionalPersistentProcesses": 0, "networkRequests": 0,
        "competitionOrders": 0, "parametersTuned": false, "walletsReset": false,
        "profitabilityEstablished": false,
    });
    atomic_json(Path::new("data/native-dip-host-preparation-20261004.json"), &prepared)?;
    Ok(prepared)
}
