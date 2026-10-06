// data/prepare-settlement-portfolio-v2-20261005.mjs — declare the funded
// settlement portfolio v2 cohort with a pinned journal-append backend.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::breadth::validate_universe;
use baal_collector::model::digest;
use baal_collector::native_journal_append::journal_append_binding;
use baal_collector::settlement_paper::settlement_paper_fills;
use baal_collector::settlement_portfolio_host_v2::{
    settlement_portfolio_host_code_identity, verify_settlement_portfolio_source_prefix,
};
use baal_collector::settlement_portfolio_paper_v2::validate_settlement_portfolio_bank;
use baal_service_runtime::contract::ms_to_iso;
use serde_json::{json, Value as V};
use std::io::Write;
use std::path::{Path, PathBuf};

fn copy_exact(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
    let b = std::fs::read(from).map_err(|e| e.to_string())?;
    if to.exists() {
        if sha(&std::fs::read(to).map_err(|e| e.to_string())?) != sha(&b) {
            return Err("Pinned artifact conflict".into());
        }
        Ok(())
    } else {
        std::fs::OpenOptions::new().write(true).create_new(true).open(to)
            .and_then(|mut f| f.write_all(&b)).map_err(|e| e.to_string())
    }
}
pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    for f in ["STOP", "COLLECTOR_STOP", "EXECUTION_STOP"] {
        if Path::new(&format!("{rt}/{f}")).exists() {
            return Err("Existing STOP requires provenance review".into());
        }
    }
    let proposal_file = "data/kansas-settlement-portfolio-proposal-20261005-v2.json";
    let proposal = read_json(Path::new(proposal_file))?;
    let dir = format!("{}/settlement-portfolio-v2-cohorts/{}", rt, proposal["name"].as_str().unwrap_or(""));
    if Path::new(&dir).exists() {
        return Err("Never overwrite an existing prospective bank".into());
    }
    let source_root = Path::new("backend-rust/journal-append").canonicalize().map_err(|e| e.to_string())?;
    let binary_path = source_root.join("target/release/baal-journal-append.exe");
    let library = format!(
        "{}/SigTradingCup/rust-toolchain/rustup/toolchains/stable-x86_64-pc-windows-gnullvm/bin/libunwind.dll",
        std::env::var("LOCALAPPDATA").unwrap_or_default());
    let development = journal_append_binding(&binary_path, &source_root, &[&library])?;
    let installed_root = format!("{rt}/journal-append-native-sources/{}", development["engineHash"].as_str().unwrap_or(""));
    let installed_binary_dir = format!("{rt}/journal-append-native-builds/{}", development["binaryHash"].as_str().unwrap_or(""));
    std::fs::create_dir_all(&installed_root).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&installed_binary_dir).map_err(|e| e.to_string())?;
    for f in development["sources"].as_object().unwrap().keys() {
        copy_exact(&source_root.join(f), &PathBuf::from(format!("{installed_root}/{f}")))?;
    }
    let installed_binary = format!("{installed_binary_dir}/baal-journal-append.exe");
    let installed_library = format!("{installed_binary_dir}/libunwind.dll");
    copy_exact(&binary_path, &PathBuf::from(&installed_binary))?;
    copy_exact(Path::new(&library), &PathBuf::from(&installed_library))?;
    let binding = journal_append_binding(
        &PathBuf::from(&installed_binary), &PathBuf::from(&installed_root), &[&installed_library])?;
    if binding["binaryHash"] != development["binaryHash"] || binding["engineHash"] != development["engineHash"] {
        return Err("Installed native identity changed".into());
    }
    let raw_universe = read_json(Path::new("live-paper/price-universe.json"))?;
    let universe = validate_universe(&raw_universe, raw_universe["tournamentId"].as_str().unwrap_or(""))?;
    let prefix = verify_settlement_portfolio_source_prefix(
        Path::new(&format!("{rt}/public-books.jsonl")), &universe, None, now_ms())?;
    let old_pointer = read_json(Path::new(&format!("{rt}/settlement-paper-active.json")))?;
    let old_bank = read_json(Path::new(&format!("{rt}/settlement-paper-cohorts/{}/bank.json",
        old_pointer["cohort"].as_str().unwrap_or(""))))?;
    let bank = json!({
        "schemaVersion": 2, "mode": "funded_settlement_portfolio_paper_v2",
        "name": proposal["name"], "cutoff": prefix["cutoff"], "universe": universe,
        "identities": proposal["identities"], "research": proposal["research"],
        "researchEvidence": proposal["researchEvidence"], "riskGroups": proposal["riskGroups"],
        "valuationDeadlineMs": proposal["valuationDeadlineMs"], "policy": proposal["policy"],
        "fillPolicy": settlement_paper_fills(),
        "sourcePrefixBytes": prefix["sourcePrefixBytes"],
        "sourcePrefixHash": prefix["sourcePrefixHash"],
        "sourcePrefixSequence": prefix["sourcePrefixSequence"],
        "sourcePrefixJournalHash": prefix["sourcePrefixJournalHash"],
        "nativeBackend": old_bank["nativeBackend"], "appendBackend": binding,
        "codeIdentity": settlement_portfolio_host_code_identity(
            Path::new("live-paper"), Path::new("backend-rust/journal-append"))?,
        "experiment": {
            "sourceProposalHash": sha(&std::fs::read(proposal_file).map_err(|e| e.to_string())?),
            "pointForecastNotConfidenceBound": true, "independentNewCashLedger": true,
            "existingWalletsImported": false, "meanValuePilotNotProfitProof": true,
        },
    });
    validate_settlement_portfolio_bank(&bank)?;
    std::fs::create_dir_all(format!("{rt}/settlement-portfolio-v2-cohorts")).map_err(|e| e.to_string())?;
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    std::fs::OpenOptions::new().write(true).create_new(true).open(format!("{dir}/bank.json"))
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&bank).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    let evidence = json!({
        "schemaVersion": 1, "at": ms_to_iso(now_ms() as f64), "cohort": bank["name"],
        "bankHash": digest(&bank),
        "bankFileHash": sha(&std::fs::read(format!("{dir}/bank.json")).map_err(|e| e.to_string())?),
        "cutoff": bank["cutoff"], "sourcePrefixBytes": bank["sourcePrefixBytes"],
        "sourcePrefixHash": bank["sourcePrefixHash"],
        "sourcePrefixSequence": bank["sourcePrefixSequence"],
        "nativeAppend": {
            "binaryHash": binding["binaryHash"], "engineHash": binding["engineHash"],
            "binaryPathRelative": PathBuf::from(&installed_binary).strip_prefix(&rt).unwrap()
                .to_string_lossy().replace('\\', "/"),
            "sourceRootRelative": PathBuf::from(&installed_root).strip_prefix(&rt).unwrap()
                .to_string_lossy().replace('\\', "/"),
        },
        "researchSourceAt": bank["research"][0]["sourceAt"],
        "researchExpiry": bank["research"][0]["expiresAt"],
        "modelPointValue": bank["research"][0]["lower"],
        "pointIsNotConfidenceBound": true, "initialCash": "100.000000",
        "markets": ["356"], "preparedOnly": true, "activated": false,
        "existingBanksChanged": false, "competitionOrders": 0,
        "profitabilityEstablished": false,
    });
    std::fs::OpenOptions::new().write(true).create_new(true).open("data/settlement-portfolio-prepared-20261005-v2.json")
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&evidence).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    Ok(evidence)
}
