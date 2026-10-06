// live-paper/paper-migration-guard.mjs — cutover comparison gate.
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Value as V};
use std::collections::HashMap;

fn need(v: bool, c: &str) -> Result<(), String> {
    if !v { return Err(format!("PAPER_MIGRATION_{c}")); }
    Ok(())
}

// A comparison gate, not an importer, lease or activation. Native source and
// economic replay must independently establish every proposed state first.
/// validatePaperMigration({observedAt, maximumAgeMs=30000, current, proposed,
/// now=Date.now()})
pub fn validate_paper_migration(options: &V) -> Result<V, String> {
    // JS default parameters apply only when the key is absent; a present
    // non-safe-integer fails the check rather than falling back.
    let now_ms = options.get("now").map(|v| js_int(v).unwrap_or(i64::MIN))
        .unwrap_or_else(|| std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0));
    let max_age = options.get("maximumAgeMs").map(|v| js_int(v).unwrap_or(i64::MIN)).unwrap_or(30000);
    let observed = options.get("observedAt").and_then(V::as_str).and_then(parse_iso_ms);
    need(now_ms != i64::MIN && max_age != i64::MIN && max_age > 0
        && observed.is_some_and(|x| x.is_finite())
        && observed.unwrap() <= now_ms as f64
        && now_ms as f64 - observed.unwrap() <= max_age as f64, "STALE_OBSERVATION")?;
    let current = options.get("current").and_then(V::as_array).cloned();
    let proposed = options.get("proposed").and_then(V::as_array).cloned();
    need(current.is_some() && proposed.is_some(), "MANIFEST")?;
    let current = current.unwrap();
    let proposed = proposed.unwrap();
    let indexes = |rows: &[V]| -> Result<HashMap<String, V>, String> {
        let mut map = HashMap::new();
        for r in rows {
            need(r["cohort"].as_str().is_some() && !map.contains_key(r["cohort"].as_str().unwrap()),
                "DUPLICATE_COHORT")?;
            map.insert(r["cohort"].as_str().unwrap().to_string(), r.clone());
        }
        Ok(map)
    };
    let owners = indexes(&current)?;
    let candidate = indexes(&proposed)?;
    need(owners.len() == candidate.len()
        && owners.keys().all(|k| candidate.contains_key(k)), "MISSING_OR_ADOPTED_COHORT")?;
    for (id, old) in &owners {
        let next = &candidate[id];
        need(next["originalBankHash"] == old["bankHash"] && next["originalBankMode"] == old["bankMode"],
            "BANK_FAMILY_OR_ID")?;
        need(baal_execution::core::payload_hash(&next["originalAnchors"])?
                == baal_execution::core::payload_hash(&old["anchors"])?, "ORIGINAL_ANCHORS")?;
        need(next["status"] == old["status"], "RETIREMENT_OR_DRAIN")?;
        if old["authoritativeOwner"].as_str() == Some("operator_overlay") {
            need(next["authoritativeOwner"].as_str() == Some("operator_overlay")
                && next["overlayHash"] == old["overlayHash"]
                && next["baseActorEnabled"] == V::Bool(false), "FENCED_BASE_RESURRECTION")?;
        } else {
            need(next["authoritativeOwner"] == old["authoritativeOwner"], "OWNER")?;
        }
        need(next["nativeReplayVerified"] == V::Bool(true) && next["originalSourceReplayVerified"] == V::Bool(true),
            "NATIVE_REPLAY_UNPROVEN")?;
        need(baal_execution::core::payload_hash(&next["accounting"])?
                == baal_execution::core::payload_hash(&old["accounting"])?, "CASH_BASIS_OR_LIABILITY")?;
        if old["status"].as_str() == Some("retired_flat") {
            need(old["accounting"]["wallets"].as_object().is_some_and(|ws| ws.values().all(|w|
                w["positions"].as_array().is_some_and(|a| a.is_empty())
                    && w["pending"].as_array().is_some_and(|a| a.is_empty())
                    && js_int(&w["unresolvedQuotes"]) == Some(0)
                    && w["reservedCost"].as_str() == Some("0.000000"))), "RETIREMENT_NOT_FLAT")?;
        }
    }
    Ok(json!({"compatible": true, "cohorts": owners.len(),
        "authoritativeInventoryPreserved": true, "activationAuthority": false, "contestOrders": 0}))
}
