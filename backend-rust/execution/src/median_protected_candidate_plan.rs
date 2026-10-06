// execution/median-protected-candidate-plan.mjs — pure metadata selection.
// Bulk prices never authorize an order or a fill.
use baal_service_runtime::fixed::{instant, price, TICK};
use serde_json::{json, Value as V};
use std::collections::{HashMap, HashSet};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("MEDIAN_PROTECTED_PLAN_{code}"))
    }
}
fn hex(x: &V) -> bool {
    x.as_str().map(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())).unwrap_or(false)
}

pub fn plan_median_protected_candidates(
    universe: &V,
    run_id: &str,
    bank_hash: &str,
    planned_market_ids: &[String],
    owned_market_ids: &[String],
    proposals: &[V],
    bulk_hints: &[V],
    now: i64,
    cycle: i64,
    last_metadata_cycles: &V,
) -> Result<V, String> {
    need(
        universe["tournamentId"].as_str() == Some(crate::TOURNAMENT_ID)
            && universe["exchanges"].is_array()
            && run_id.len() >= 2
            && run_id.len() <= 101
            && run_id.chars().next().unwrap().is_ascii_lowercase()
            && run_id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && bank_hash.len() == 64
            && bank_hash.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            && cycle >= 0,
        "SCOPE",
    )?;
    let scope: HashMap<String, String> = universe["exchanges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| (x["marketId"].as_str().unwrap_or("").to_string(), x["exchangeId"].as_str().unwrap_or("").to_string()))
        .collect();
    let planned_set: HashSet<&String> = planned_market_ids.iter().collect();
    let owned_set: HashSet<&String> = owned_market_ids.iter().collect();
    need(
        planned_market_ids.len() <= 4
            && planned_set.len() == planned_market_ids.len()
            && planned_market_ids.iter().all(|id| scope.contains_key(id))
            && owned_market_ids.len() <= 3
            && owned_set.len() == owned_market_ids.len()
            && owned_market_ids.iter().all(|id| scope.contains_key(id))
            && proposals.len() <= 3,
        "INPUTS",
    )?;
    let owned: HashSet<&str> = owned_market_ids.iter().map(|s| s.as_str()).collect();
    let mut pending: HashMap<String, &V> = HashMap::new();
    for p in proposals {
        let created = instant(p["createdAt"].as_str().unwrap_or(""))?;
        let expires = instant(p["expiresAt"].as_str().unwrap_or(""))?;
        need(
            p["bankHash"].as_str() == Some(bank_hash)
                && scope.get(p["marketId"].as_str().unwrap_or("")) == Some(&p["exchangeId"].as_str().unwrap_or("").to_string())
                && p["status"].as_str() == Some("proposed")
                && ["buy", "sell"].contains(&p["action"].as_str().unwrap_or(""))
                && created <= now
                && expires - created == 120000,
            "PROPOSAL_SCOPE_CLOCK",
        )?;
        if now < expires && planned_market_ids.contains(&p["marketId"].as_str().unwrap_or("").to_string())
            && !owned.contains(p["marketId"].as_str().unwrap_or(""))
        {
            need(!pending.contains_key(p["marketId"].as_str().unwrap_or("")), "DUPLICATE_PROPOSAL")?;
            pending.insert(p["marketId"].as_str().unwrap_or("").to_string(), p);
        }
    }
    let mut hints: HashMap<String, V> = HashMap::new();
    for h in bulk_hints {
        let usable = (|| -> Result<V, String> {
            need(
                scope.get(h["marketId"].as_str().unwrap_or("")) == Some(&h["exchangeId"].as_str().unwrap_or("").to_string())
                    && hex(&h["sourceHash"])
                    && h["sourceSequence"].as_i64().map(|s| s > 0).unwrap_or(false)
                    && h["depthAvailable"].as_bool() == Some(false)
                    && h["executable"].as_bool() == Some(false)
                    && h["outcome"].as_str() == Some("reference")
                    && instant(h["knownAt"].as_str().unwrap_or(""))? <= now
                    && now - instant(h["knownAt"].as_str().unwrap_or(""))? <= 20000,
                "HINT",
            )?;
            let gap = price(h["reference"].as_str().unwrap_or(""))? - price(h["ask"].as_str().unwrap_or(""))?;
            need(gap >= TICK, "HINT_GAP")?;
            let mut hint = h.clone();
            hint["gapTicks"] = json!(gap / TICK);
            Ok(hint)
        })();
        // An unusable hint does not erase or exclude an already planned book.
        if let Ok(hint) = usable {
            if planned_market_ids.contains(&h["marketId"].as_str().unwrap_or("").to_string()) {
                hints.insert(h["marketId"].as_str().unwrap_or("").to_string(), hint);
            }
        }
    }
    if let V::Object(m) = last_metadata_cycles {
        for (id, n) in m {
            need(scope.contains_key(id) && n.as_i64().map(|x| x >= 0 && x <= cycle).unwrap_or(false), "ROTATION_STATE")?;
        }
    }
    let mut candidates: Vec<V> = planned_market_ids
        .iter()
        .filter(|id| !owned.contains(id.as_str()))
        .map(|id| {
            json!({
                "marketId": id,
                "phase": if pending.contains_key(id) { "official_proposal" }
                    else if hints.contains_key(id) { "causal_reference_hint" }
                    else { "planned_fair_rotation" },
                "hint": hints.get(id).cloned().unwrap_or(V::Null),
                "proposal": pending.get(id).map(|p| (*p).clone()).unwrap_or(V::Null),
                "lastMetadataCycle": last_metadata_cycles[id].as_i64().unwrap_or(-1),
            })
        })
        .collect();
    let planned_index = |id: &str| planned_market_ids.iter().position(|x| x == id).unwrap_or(usize::MAX) as i64;
    candidates.sort_by(|a, b| {
        let a_prop = !a["proposal"].is_null();
        let b_prop = !b["proposal"].is_null();
        (!a_prop as i32)
            .cmp(&(!b_prop as i32))
            .then(if a_prop && b_prop {
                let at = instant(a["proposal"]["expiresAt"].as_str().unwrap_or("")).unwrap_or(0);
                let bt = instant(b["proposal"]["expiresAt"].as_str().unwrap_or("")).unwrap_or(0);
                at.cmp(&bt)
            } else {
                std::cmp::Ordering::Equal
            })
            .then(
                (a["lastMetadataCycle"].as_i64() == Some(cycle - 1))
                    .cmp(&(b["lastMetadataCycle"].as_i64() == Some(cycle - 1))),
            )
            .then((!a["hint"].is_null() as i32).cmp(&(!b["hint"].is_null() as i32)))
            .then(
                (b["hint"]["gapTicks"].as_i64().unwrap_or(0)).cmp(&a["hint"]["gapTicks"].as_i64().unwrap_or(0)),
            )
            .then(a["lastMetadataCycle"].as_i64().cmp(&b["lastMetadataCycle"].as_i64()))
            .then(
                planned_index(a["marketId"].as_str().unwrap_or(""))
                    .cmp(&planned_index(b["marketId"].as_str().unwrap_or(""))),
            )
    });
    let selected = candidates.first().cloned();
    let mut next = last_metadata_cycles.clone();
    if let Some(s) = &selected {
        next[s["marketId"].as_str().unwrap_or("")] = json!(cycle);
    }
    Ok(json!({
        "schemaVersion": 1,
        "runId": run_id,
        "bankHash": bank_hash,
        "plannedMarketIds": planned_market_ids,
        "candidateMarketIds": owned_market_ids.iter().cloned().chain(candidates.iter().map(|x| x["marketId"].as_str().unwrap_or("").to_string())).collect::<Vec<_>>(),
        "nextMetadataMarketId": selected.as_ref().map(|s| s["marketId"].clone()).unwrap_or(V::Null),
        "nextMetadataPhase": selected.as_ref().map(|s| s["phase"].clone()).unwrap_or(V::Null),
        "lastMetadataCycles": next,
        "metadataCandidateBudget": 1,
        "plannedBookBudget": planned_market_ids.len(),
        "sourceProof": selected.as_ref().filter(|s| !s["hint"].is_null()).map(|s| {
            let h = &s["hint"];
            json!({"sequence": h["sourceSequence"], "hash": h["sourceHash"], "knownAt": h["knownAt"],
                "reference": h["reference"], "ask": h["ask"], "depthAvailable": false, "executable": false})
        }).unwrap_or(V::Null),
        "executionAuthority": false,
        "thresholdChanged": false,
    }))
}
