// data/prepare-dual-no-sprint-20261005.mjs — initialize the Michigan dual-NO
// v1 profit-sprint cohort from captured contract metadata.
use baal_collector::dual_no_pair_host_v1::open_dual_no_paper_host;
use baal_collector::frozen_paper_graph::collect_frozen_paper_files;
use baal_collector::frozen_paper_runtime::frozen_paper_module;
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}
fn parse_iso_ms(s: &str) -> Option<f64> {
    baal_service_runtime::contract::parse_iso_ms(s)
}
fn write_wx(p: &Path, b: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600); }
    let mut f = o.open(p).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, b).map_err(|e| e.to_string())
}
fn mkdir_700(p: &Path) -> Result<(), String> {
    #[cfg(unix)] {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(p)
            .map_err(|e| e.to_string())
    }
    #[cfg(not(unix))] { std::fs::create_dir_all(p).map_err(|e| e.to_string()) }
}

pub fn run() -> Result<V, String> {
    let rt = localappdata()?.join("SigTradingCup/paper-runtime");
    let cohort = "michigan-governor-dual-no-20261005-v1";
    let dir = rt.join("production-metadata")
        .join("michigan-dual-no-profit-sprint-20261005");
    let universe = read_json(Path::new("live-paper/price-universe.json"))?;
    let expected = [("305", "994"), ("306", "995")];
    let captures: Vec<V> = expected.iter()
        .map(|(m, _)| read_json(&dir.join(format!("{}.json", m))))
        .collect::<Result<_, String>>()?;
    for (i, c) in captures.iter().enumerate() {
        if c["marketId"].as_str() != Some(expected[i].0)
            || c["rootHash"].as_str() != payload_hash(&c["root"]).ok().as_deref()
            || !universe["exchanges"].as_array().cloned().unwrap_or_default().iter()
                .any(|x| x["marketId"].as_str() == Some(expected[i].0)
                    && x["exchangeId"].as_str() == Some(expected[i].1)
                    && x["option"].as_str() == Some("YES"))
            || (now_ms() as f64)
                - parse_iso_ms(c["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
                > 3600000.0 {
            return Err("Scoped metadata missing, changed or stale".into());
        }
    }
    for f in [PathBuf::from("STOP"), rt.join("STOP"), rt.join("dual-no-pair-active.json")] {
        if f.exists() { return Err("Existing stop or pointer requires inspection".into()); }
    }
    let review = json!({"schemaVersion": 1, "kind": "conditional_no_pair_review",
        "id": cohort, "tournamentId": universe["tournamentId"],
        "evidenceHash": payload_hash(&V::Array(captures.clone()))?,
        "recordedAt": now_iso(), "expiresAt": "2026-10-05T22:00:00.000Z",
        "normalResolutionOnly": true,
        "administrativeRefundGapAcknowledged": true,
        "legs": expected.iter().enumerate().map(|(i, (market_id, exchange_id))|
            json!({"marketId": market_id, "exchangeId": exchange_id,
                "captureMarketId": captures[i]["marketId"], "outcome": "NO",
                "rulesHash": captures[i]["rootHash"], "root": captures[i]["root"]}))
            .collect::<Vec<_>>(),
        "assumptions": [
            "Both predicates identify the same General election race/stage/office/date and different party winners.",
            "Normal resolution cannot award both parties a win; a third-party win pays both NO legs.",
            "Administrative overrides, voids, refunds and inconsistent resolution are outside the normal payout floor.",
            "Sequential fills can leave an unhedged orphan; account exclusions and later executable depth remain required."],
        "scenarios": [
            {"id": "democratic_winner", "category": "ordinary", "payouts": [0, 1]},
            {"id": "republican_winner", "category": "ordinary", "payouts": [1, 0]},
            {"id": "other_winner", "category": "ordinary", "payouts": [1, 1]},
            {"id": "zero_payout_exception_stress", "category": "exception",
                "payouts": [0, 0]}]});
    let workspace = std::env::current_dir().map_err(|e| e.to_string())?;
    let files = collect_frozen_paper_files(&workspace,
        &["live-paper/dual-no-pair-host-v1.mjs".to_string()])?;
    let files_v: V = files.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    let snapshot = payload_hash(&files_v)?;
    let archive = rt.join("frozen-code").join(&snapshot);
    mkdir_700(&archive)?;
    for name in files.keys() {
        let target = archive.join(name);
        if let Some(parent) = target.parent() { mkdir_700(parent)?; }
        let bytes = std::fs::read(name).map_err(|e| e.to_string())?;
        if target.exists() {
            if sha(&std::fs::read(&target).map_err(|e| e.to_string())?) != sha(&bytes) {
                return Err("Archive collision".into());
            }
        } else {
            write_wx(&target, &bytes)?;
        }
    }
    atomic_json_canonical(&archive.join("manifest.json"),
        &json!({"schemaVersion": 1, "files": files_v}))?;
    let mut host = open_dual_no_paper_host(&workspace, &rt, cohort, true,
        Some(&json!({"review": review})),
        Arc::new(|| now_ms()), Arc::new(|| true))?;
    let bank;
    let stats;
    {
        bank = host.bank.clone();
        stats = host.status()?;
        host.publish()?;
        host.close()?;
    }
    let files2 = collect_frozen_paper_files(&workspace,
        &["live-paper/dual-no-pair-host-v1.mjs".to_string()])?;
    let files2_v: V = files2.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    if payload_hash(&files2_v)? != snapshot {
        return Err("Code changed during preparation".into());
    }
    let index_file = rt.join("frozen-code").join("index.json");
    let mut index = read_json(&index_file)?;
    index["banks"].as_object_mut().ok_or("assertion failed")?
        .insert(payload_hash(&bank)?, json!({"snapshot": snapshot}));
    atomic_json_canonical(&index_file, &index)?;
    let _ = frozen_paper_module(&rt, &bank, "live-paper/dual-no-pair-host-v1.mjs")?;
    atomic_json_canonical(&rt.join("dual-no-pair-active.json"),
        &json!({"schemaVersion": 1, "cohort": cohort,
            "bankHash": payload_hash(&bank)?}))?;
    let evidence = json!({"at": now_iso(), "cohort": cohort,
        "bankHash": payload_hash(&bank)?, "cutoff": bank["cutoff"],
        "sourcePrefixBytes": bank["sourcePrefixBytes"],
        "sourcePrefixHash": bank["sourcePrefixHash"],
        "sourcePrefixSequence": bank["sourcePrefixSequence"],
        "sourcePrefixTailHash": bank["sourcePrefixTailHash"],
        "snapshot": snapshot, "frozenFiles": files.len(), "review": review,
        "initialRealizedPnl": stats["realizedPnl"],
        "initialFills": stats["fills"],
        "initialInventory": stats["positions"].as_array().map(|a| a.len())
            .unwrap_or(0),
        "competitionOrders": 0, "additionalAccountClients": 0,
        "additionalPersistentServices": 0, "profitabilityEstablished": false});
    atomic_json_canonical(Path::new("data/dual-no-sprint-initialization-20261005.json"),
        &evidence)?;
    Ok(json!({"cohort": cohort, "bankHash": evidence["bankHash"],
        "cutoff": evidence["cutoff"], "initialFills": stats["fills"],
        "orders": 0}))
}
