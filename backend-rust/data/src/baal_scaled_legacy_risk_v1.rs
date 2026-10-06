// execution/baal-scaled-legacy-risk-v1.mjs — verify the sealed retirement of
// the legacy scaled sources, then require the live execution state to be flat.
use baal_execution::core::{payload_hash, validate_execution_state};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, sha256};
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("BAAL_LEGACY_{code}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V { v.get(k).unwrap_or(&V::Null) }

// Historical retirement is verified at its original evidence clock. Current
// account/exchange state must still be freshly checked by the new execution host.
pub fn read_baal_scaled_legacy_risk(
    runtime_dir: &Path,
    get_other_state: impl Fn() -> Result<V, String>,
) -> Result<V, String> {
    let file = runtime_dir.join("baal-scaled-retired-sources.json");
    let d: V = serde_json::from_str(&std::fs::read_to_string(&file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    need(
        get(&d, "schemaVersion") == &json!(1)
            && get(&d, "ownerId") == &json!("Codex")
            && get(&d, "kind") == &json!("verified_retired_baal_sources_v1"),
        "DESCRIPTOR",
    )?;
    let rel = get(&d, "repairJournal");
    need(
        rel.as_str().is_some_and(|r| {
            !Path::new(r).is_absolute() && !r.split(['/', '\\']).any(|c| c == "..")
        }),
        "PATH",
    )?;
    let target: PathBuf = runtime_dir.join(rel.as_str().unwrap_or(""));
    let root = runtime_dir.canonicalize().map_err(|e| e.to_string())?;
    let target = target.canonicalize().map_err(|e| e.to_string())?;
    need(target.starts_with(&root) && target != root, "PATH_SCOPE")?;
    let bytes = std::fs::read(&target).map_err(|e| e.to_string())?;
    need(
        js_int(get(&d, "repairJournalBytes")) == Some(bytes.len() as i64)
            && sha256(&bytes).as_str() == get(&d, "repairJournalSha256").as_str().unwrap_or(""),
        "ORIGINAL_SOURCE_CHANGED",
    )?;
    let j = Journal::open(&target)?;
    let row = j.last.clone().unwrap_or(V::Null);
    need(
        js_int(get(&d, "repairSequence")) == Some(j.sequence as i64)
            && get(&d, "repairHash").as_str().unwrap_or("") == j.previous.as_str()
            && get(get(&row, "event"), "kind") == &json!("retired"),
        "RETIREMENT_FRONTIER",
    )?;
    let proof =
        crate::no_rest_sell_reconciliation_v2::verify_scoped_rest_no_sell_v2(get(get(&row, "event"), "sellReverification"))?;
    let overlay =
        crate::paired_owned_exit_retirement_v2::build_paired_owned_exit_retirement_v2(get(get(&row, "event"), "evidence"))?;
    need(
        payload_hash(&proof)? == payload_hash(get(get(get(&row, "event"), "evidence"), "sellProof"))?
            && payload_hash(&overlay)? == payload_hash(get(get(&row, "state"), "retirement"))?
            && get(&overlay, "overlayHash") == get(&d, "overlayHash")
            && js_int(get(&overlay, "currentOwnedQuantity")) == Some(0)
            && get(&overlay, "currentReservedCash") == &json!("0.000000"),
        "VERIFIED_RETIREMENT",
    )?;
    let core = validate_execution_state(&get_other_state()?)?;
    need(
        get(&core, "tournamentId") == get(&d, "tournamentId")
            && get(&core, "intents")
                .as_object()
                .map(|o| o.values().all(|x| get(x, "status") == &json!("closed")))
                .unwrap_or(false)
            && get(&core, "owned")
                .as_object()
                .map(|o| o.values().all(|x| js_int(get(x, "quantity")) == Some(0)))
                .unwrap_or(false),
        "OTHER_CORE_NOT_FLAT",
    )?;
    Ok(json!({
        "complete": true,
        "sourceHash": payload_hash(&json!({"descriptor": d, "repairHash": j.previous, "core": core}))?,
        "currentOwnedQuantity": 0,
        "currentReservedCash": "0.000000",
        "unresolvedIntentIds": [],
        "historicalRetirementAt": get(&overlay, "recordedAt").clone(),
        "retirementIsNewAuthority": false,
    }))
}
