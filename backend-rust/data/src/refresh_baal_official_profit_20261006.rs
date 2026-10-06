// data/refresh-baal-official-profit-20261006.mjs — refresh the official baal-scaled
// profit report from the sealed controller journal (proof-hash verification only,
// never account queries or raw receipt publication).
//
// PORT GAP: the .mjs imports '../execution/baal-scaled-controller-legacy-replay-v1.mjs'
// which does NOT exist anywhere in the repository (the upstream script cannot run
// in Node either — the import throws ERR_MODULE_NOT_FOUND). The corresponding
// replay module therefore does not exist in Rust; `run()` surfaces the same
// failure as a missing-dependency error.
use serde_json::{Value as V};

/// Result of `replayBaalScaledControllerV1({bodyText, expectedCodeIdentity})`.
/// The upstream implementation is absent (see module note); this stub preserves
/// the call site so the gap is explicit rather than silently dropped.
pub struct BaalScaledControllerReplay {
    pub state: V,
    pub anchor: V,
}

#[allow(dead_code)]
fn replay_baal_scaled_controller_v1(
    _body_text: &str,
    _expected_code_identity: &V,
) -> Result<BaalScaledControllerReplay, String> {
    Err("BAAL_SCALED_CONTROLLER_LEGACY_REPLAY_V1_UNAVAILABLE".into())
}

/// run() — reads the sealed baal-scaled journal, verifies every sell proof hash,
/// sums realized PnL via exact decimal BigInt math, and writes the official
/// profit report. Blocked upstream and here on the missing replay module.
pub fn run() -> Result<V, String> {
    let runtime = std::path::Path::new(
        &std::env::var("LOCALAPPDATA").unwrap_or_default(),
    )
    .join("SigTradingCup")
    .join("paper-runtime");
    let file = runtime.join("baal-scaled-live-runs/baal-scaled-no-20261006-human/controller/baal-scaled-no-journal.jsonl");
    let body_text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let first: V = serde_json::from_str(body_text.split('\n').next().unwrap_or(""))
        .map_err(|e| e.to_string())?;
    let replay = replay_baal_scaled_controller_v1(
        &body_text,
        &first["event"]["codeIdentity"],
    )?;
    let sells: Vec<V> = replay.state["orders"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|o| o["body"]["action"] == "sell" && !o["proof"].is_null())
                .cloned()
                .collect::<Vec<V>>()
        })
        .unwrap_or_default()
        .iter()
        .map(|o| {
            let p = &o["proof"];
            let mut unsigned = p.clone();
            let verification_hash = unsigned
                .as_object_mut()
                .and_then(|m| m.shift_remove("verificationHash"))
                .unwrap_or(V::Null);
            assert_eq!(
                baal_execution::core::payload_hash(&unsigned)?,
                verification_hash.as_str().unwrap_or("")
            );
            assert_eq!(p["fee"].as_str().unwrap_or(""), "0.000000");
            Ok(serde_json::json!({
                "orderId": o["orderId"], "marketId": p["marketId"], "exchangeId": p["exchangeId"],
                "quantity": p["filledQuantity"],
                "removedPrincipal": p["removedPrincipalCost"], "proceeds": p["scopedProceeds"],
                "realizedPnl": p["realizedPnl"],
                "proofHash": verification_hash, "readAt": p["readAt"],
                "fills": p["fills"].as_array().map(|a| a.iter().map(|f| serde_json::json!({
                    "id": if !f["id"].is_null() { f["id"].clone() } else { f["fillId"].clone() },
                    "quantity": f["quantity"], "price": f["price"], "filledAt": f["filledAt"],
                })).collect::<Vec<_>>()).unwrap_or_default(),
            }))
        })
        .collect::<Result<Vec<V>, String>>()?;
    let inventory: serde_json::Map<String, V> = replay.state["inventory"]
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(ex, v)| {
                    (
                        ex.clone(),
                        serde_json::json!({
                            "marketId": v["proof"]["marketId"],
                            "quantity": v["proof"]["remainingOwnedQuantity"],
                            "cost": v["proof"]["remainingOwnedCost"],
                            "cumulativeRealizedPnl": v["proof"]["cumulativeRealizedPnl"],
                            "proofHash": v["proof"]["verificationHash"],
                            "eventId": v["package"]["eventId"],
                        }),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let pnl = sells.iter().try_fold(num_bigint::BigInt::from(0), |sum, p| {
        Ok::<num_bigint::BigInt, String>(
            sum + baal_paper::fixed::big_amount(p["realizedPnl"].as_str().unwrap_or(""))?,
        )
    })?;
    let last: V = serde_json::from_str(body_text.trim_end().split('\n').last().unwrap_or(""))
        .map_err(|e| e.to_string())?;
    let remaining_quantity: i64 = inventory
        .values()
        .map(|p| p["quantity"].as_i64().unwrap_or(0))
        .sum();
    let remaining_cost = inventory.values().try_fold(num_bigint::BigInt::from(0), |n, p| {
        Ok::<num_bigint::BigInt, String>(
            n + baal_paper::fixed::big_amount(p["cost"].as_str().unwrap_or(""))?,
        )
    })?;
    let out = serde_json::json!({
        "schemaVersion": 1,
        "runId": replay.state["scope"]["runId"],
        "observedAt": baal_service_runtime::contract::ms_to_iso(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.0)
        ),
        "sourceThroughAt": last["at"],
        "actualOrders": replay.state["orders"].as_array().map(|a| a.iter()
            .filter(|o| o["fetchStarts"].as_array().map(|f| f.iter()
                .any(|x| x["method"] == "POST")).unwrap_or(false)).count()).unwrap_or(0),
        "scaledRealizedPnl": baal_paper::fixed::money(&pnl),
        "oldFourClosedPilotNetRealized": "0.015000",
        "fees": "0.000000",
        "sells": sells,
        "inventory": inventory,
        "remainingQuantity": remaining_quantity,
        "remainingCost": baal_paper::fixed::money(&remaining_cost),
        "sourceActorSequence": replay.anchor["sequence"],
        "sourceActorHash": replay.anchor["hash"],
        "sourceBytesHash": replay.anchor["sha256"],
        "proofsHashVerified": true,
        "economicReplayPerformed": true,
        "includesUnrealizedPnl": false,
        "profitabilityAtScaleEstablished": false,
    });
    std::fs::write(
        "data/baal-scaled-official-profit-20261006.json",
        serde_json::to_string_pretty(&out).map_err(|e| e.to_string())? + "\n",
    )
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "actualOrders": out["actualOrders"],
        "scaledRealizedPnl": out["scaledRealizedPnl"],
        "remainingQuantity": out["remainingQuantity"],
        "remainingCost": out["remainingCost"],
        "sourceActorSequence": out["sourceActorSequence"],
    }))
}
