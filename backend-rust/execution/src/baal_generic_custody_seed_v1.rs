// execution/baal-generic-custody-seed-v1.mjs — pure private-state conversion
// after complete source replay. Preserves all attributable lots and
// closed/abandoned history; never grants write authority.
use crate::baal_custody_frontier_v1::assert_baal_custody_frontier_can_continue;
use crate::core::payload_hash;
use serde_json::{json, Map, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("GENERIC_CUSTODY_SEED_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}

/// buildBaalGenericCustodySeedV1({frontier, scope, allocation})
pub fn build_baal_generic_custody_seed_v1(frontier: &V, scope: &V, allocation: &V) -> Result<V, String> {
    assert_baal_custody_frontier_can_continue(frontier)?;
    let s = get(frontier, "currentState");
    for k in ["ownerId", "runId", "tournamentId"] {
        need(
            get(scope, k) == get(get(s, "scope"), k) && get(allocation, k) == get(get(s, "allocation"), k),
            "ATTRIBUTION_SCOPE",
        )?;
    }
    need(
        payload_hash(allocation)? == payload_hash(get(s, "allocation"))?,
        "ORIGINAL_ALLOCATION",
    )?;
    let exchanges = get(scope, "exchanges");
    need(
        exchanges.is_array()
            && exchanges
                .as_array()
                .unwrap()
                .iter()
                .map(|e| get(e, "exchangeId"))
                .collect::<std::collections::HashSet<_>>()
                .len()
                == exchanges.as_array().unwrap().len(),
        "UNIQUE_SCOPE",
    )?;
    for (ex, p) in get(s, "currentInventory")
        .as_object()
        .cloned()
        .unwrap_or_default()
    {
        need(
            exchanges
                .as_array()
                .unwrap()
                .iter()
                .any(|e| {
                    sget(e, "exchangeId") == Some(ex.as_str())
                        && get(e, "marketId") == get(get(&p, "proof"), "marketId")
                }),
            "ALL_INHERITED_POSITIONS_IN_SCOPE",
        )?;
    }
    need(
        get(s, "originalOrders")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .chain(get(s, "orders").as_array().cloned().unwrap_or_default().iter())
            .all(|o| !get(o, "proof").is_null() || sget(o, "phase") == Some("abandoned")),
        "NO_INHERITED_PENDING",
    )?;
    let mut seed = Map::new();
    seed.insert("schemaVersion".into(), json!(1));
    seed.insert("kind".into(), json!("baal_generic_custody_seed_v1"));
    seed.insert("ownerId".into(), get(scope, "ownerId").clone());
    seed.insert("runId".into(), get(scope, "runId").clone());
    seed.insert("tournamentId".into(), get(scope, "tournamentId").clone());
    seed.insert("scopeHash".into(), json!(payload_hash(scope)?));
    seed.insert("allocationHash".into(), json!(payload_hash(allocation)?));
    seed.insert(
        "sourceStateHash".into(),
        get(frontier, "currentStateHash").clone(),
    );
    seed.insert(
        "sourceDescriptorHash".into(),
        get(frontier, "descriptorHash").clone(),
    );
    seed.insert(
        "sourceActorAnchor".into(),
        get(frontier, "currentActorAnchor").clone(),
    );
    seed.insert("sourceHashes".into(), get(frontier, "sourceHashes").clone());
    seed.insert("inventory".into(), get(s, "currentInventory").clone());
    let mut inherited = get(s, "originalOrders").as_array().cloned().unwrap_or_default();
    inherited.extend(get(s, "orders").as_array().cloned().unwrap_or_default());
    seed.insert("inheritedClosedOrders".into(), V::Array(inherited));
    seed.insert(
        "inheritedPairAssignments".into(),
        get(s, "assignments").clone(),
    );
    seed.insert(
        "inheritedPairRetirements".into(),
        get(s, "pairRetirements").clone(),
    );
    seed.insert(
        "originalActivation".into(),
        get(frontier, "originalActivation").clone(),
    );
    seed.insert("summary".into(), get(frontier, "summary").clone());
    seed.insert("executionAuthority".into(), json!(false));
    let hash = payload_hash(&V::Object(seed.clone()))?;
    seed.insert("seedHash".into(), json!(hash));
    Ok(V::Object(seed))
}

/// validateBaalGenericCustodySeedV1(seed, scope, allocation)
pub fn validate_baal_generic_custody_seed_v1(seed: &V, scope: &V, allocation: &V) -> Result<V, String> {
    let mut body = seed.as_object().cloned().unwrap_or_default();
    let seed_hash = body.shift_remove("seedHash").unwrap_or(V::Null);
    need(
        sget(seed, "kind") == Some("baal_generic_custody_seed_v1")
            && get(seed, "executionAuthority") == &V::Bool(false)
            && payload_hash(&V::Object(body))? == seed_hash.as_str().unwrap_or(""),
        "HASH",
    )?;
    need(
        get(seed, "scopeHash").as_str() == Some(payload_hash(scope)?.as_str())
            && get(seed, "allocationHash").as_str()
                == Some(payload_hash(allocation)?.as_str()),
        "CONFIGURATION",
    )?;
    for k in ["ownerId", "runId", "tournamentId"] {
        need(get(seed, k) == get(scope, k), "ATTRIBUTION")?;
    }
    need(
        get(seed, "inheritedClosedOrders")
            .as_array()
            .map(|a| {
                a.iter()
                    .all(|o| !get(o, "proof").is_null() || sget(o, "phase") == Some("abandoned"))
            })
            .unwrap_or(false),
        "PENDING",
    )?;
    Ok(seed_hash)
}
