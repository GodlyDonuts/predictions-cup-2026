// execution/baal-scaled-legacy-risk-v1.mjs — historical retirement is
// verified at its original evidence clock. Current account/exchange state
// must still be freshly checked by the new execution host.
//
// Port notes:
// - `need` throws BAAL_LEGACY_<code>; fs/JSON/verifier errors propagate
//   as their own messages (JSON.parse -> serde error string is mapped to
//   a non-prefixed message, matching the JS non-need throw behavior).
// - `getOtherState` is a synchronous dep returning the other core's state.
use crate::core::{payload_hash, validate_execution_state};
use crate::no_rest_sell_reconciliation_v2::verify_scoped_rest_no_sell_v2;
use crate::paired_owned_exit_retirement_v2::build_paired_owned_exit_retirement_v2;
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::fs;
use std::path::Path;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("BAAL_LEGACY_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

// readBaalScaledLegacyRisk({runtimeDir,getOtherState})
pub fn read_baal_scaled_legacy_risk(
    runtime_dir: &Path,
    get_other_state: &dyn Fn() -> V,
) -> Result<V, String> {
    let file = runtime_dir.join("baal-scaled-retired-sources.json");
    let d: V = serde_json::from_str(
        &fs::read_to_string(&file).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    need(
        get(&d, "schemaVersion") == &json!(1)
            && get(&d, "ownerId").as_str() == Some("Codex")
            && get(&d, "kind").as_str() == Some("verified_retired_baal_sources_v1"),
        "DESCRIPTOR",
    )?;
    let rel = get(&d, "repairJournal").as_str();
    need(
        rel.is_some()
            && !Path::new(rel.unwrap()).is_absolute()
            && !rel.unwrap().split(['/', '\\']).any(|p| p == ".."),
        "PATH",
    )?;
    let target = runtime_dir.join(rel.unwrap());
    let root = fs::canonicalize(runtime_dir).map_err(|e| e.to_string())?;
    let real_target = fs::canonicalize(&target).map_err(|e| e.to_string())?;
    need(
        real_target
            .to_string_lossy()
            .starts_with(&format!("{}{}", root.to_string_lossy(), std::path::MAIN_SEPARATOR)),
        "PATH_SCOPE",
    )?;
    let bytes = fs::read(&target).map_err(|e| e.to_string())?;
    need(
        get(&d, "repairJournalBytes").as_f64() == Some(bytes.len() as f64)
            && sha256(&bytes) == get(&d, "repairJournalSha256").as_str().unwrap_or(""),
        "ORIGINAL_SOURCE_CHANGED",
    )?;
    let j = Journal::open(&target)?;
    let row = j.last.clone().unwrap_or(V::Null);
    need(
        get(&d, "repairSequence").as_f64() == Some(j.sequence as f64)
            && j.previous == get(&d, "repairHash").as_str().unwrap_or("")
            && get(get(&row, "event"), "kind").as_str() == Some("retired"),
        "RETIREMENT_FRONTIER",
    )?;
    let proof = verify_scoped_rest_no_sell_v2(
        get(get(&row, "event"), "sellReverification"),
    )?;
    let overlay = build_paired_owned_exit_retirement_v2(get(get(&row, "event"), "evidence"))?;
    need(
        payload_hash(&proof)? == payload_hash(get(get(&row, "event"), "evidence").get("sellProof").unwrap_or(&V::Null))?
            && payload_hash(&overlay)? == payload_hash(get(get(&row, "state"), "retirement"))?
            && get(&overlay, "overlayHash") == get(&d, "overlayHash")
            && get(&overlay, "currentOwnedQuantity") == &json!(0)
            && get(&overlay, "currentReservedCash").as_str() == Some("0.000000"),
        "VERIFIED_RETIREMENT",
    )?;
    let core = validate_execution_state(&get_other_state())?;
    need(
        get(&core, "tournamentId") == get(&d, "tournamentId")
            && get(&core, "intents")
                .as_object()
                .is_some_and(|m| m.values().all(|x| get(x, "status").as_str() == Some("closed")))
            && get(&core, "owned")
                .as_object()
                .is_some_and(|m| m.values().all(|x| get(x, "quantity") == &json!(0))),
        "OTHER_CORE_NOT_FLAT",
    )?;
    Ok(json!({
        "complete": true,
        "sourceHash": payload_hash(&json!({
            "descriptor": d,
            "repairHash": j.previous,
            "core": core,
        }))?,
        "currentOwnedQuantity": 0,
        "currentReservedCash": "0.000000",
        "unresolvedIntentIds": [],
        "historicalRetirementAt": get(&overlay, "recordedAt"),
        "retirementIsNewAuthority": false,
    }))
}

