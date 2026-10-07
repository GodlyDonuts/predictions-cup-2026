// data/replay-baal-entry-depth-v2-20261006.mjs — read-only replay of the
// original baal-scaled-no controller journal against both the v1 scaled-entry
// policy and the v2 depth-swept successor. Writes
// data/baal-entry-depth-v2-replay-20261006.json; run() returns the JSON the
// script prints.
use baal_execution::baal_scaled_controller_legacy_replay_v1::{
    baal_scaled_no_controller_identity, replay_baal_scaled_controller_v1,
};
use baal_execution::baal_scaled_entry_signal_v1::plan_baal_scaled_entry;
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::Path;

use crate::baal_scaled_entry_signal_v2::plan_baal_scaled_entry_v2;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn type_error() -> String {
    "TypeError: Cannot read properties of null or undefined".to_string()
}
// x?.quantity ?? 0
fn chosen_quantity(p: &V) -> V {
    get(p, "chosenLeg")
        .get("quantity")
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or(json!(0))
}
// x?.field ?? null
fn chosen_field(p: &V, k: &str) -> V {
    get(p, "chosenLeg").get(k).cloned().unwrap_or(V::Null)
}

pub fn run() -> Result<V, String> {
    let runtime = crate::publication_pack::runtime_dir();
    let file = Path::new(&runtime)
        .join("baal-scaled-live-runs")
        .join("baal-scaled-no-20261006-human")
        .join("controller")
        .join("baal-scaled-no-journal.jsonl");
    let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&bytes).to_string();
    let seed = replay_baal_scaled_controller_v1(&json!({
        "bodyText": text,
        "expectedCodeIdentity": baal_scaled_no_controller_identity()?,
    }))?;
    let mut packets: Vec<V> = Vec::new();
    for line in text.trim_end().split('\n') {
        let row: V = serde_json::from_str(line).map_err(|e| format!("SyntaxError: {e}"))?;
        let event = get(&row, "event");
        if event.is_null() {
            return Err(type_error());
        }
        let e = get(event, "input");
        if e.is_null() {
            return Err(type_error());
        }
        if get(e, "kind").as_str() != Some("prepared") || get(e, "action").as_str() != Some("buy") {
            continue;
        }
        let st = get(&row, "state");
        if st.is_null() {
            return Err(type_error());
        }
        let mut input = get(e, "evidence").as_object().cloned().unwrap_or_default();
        input.insert("scope".into(), get(st, "scope").clone());
        input.insert("allocation".into(), get(st, "allocation").clone());
        input.insert("now".into(), get(e, "at").clone());
        let input_v = V::Object(input);
        let old = plan_baal_scaled_entry(&input_v);
        let next = plan_baal_scaled_entry_v2(&input_v);
        let orders = get(st, "orders");
        if !orders.is_array() {
            return Err(type_error());
        }
        // orders.at(-1).body.quantity: missing, null, or non-object last
        // order (or a null body) throws TypeError in JS.
        let last = orders.as_array().unwrap().last().ok_or_else(type_error)?;
        if !last.is_object() || get(last, "body").is_null() {
            return Err(type_error());
        }
        packets.push(json!({
            "originalSequence": get(&row, "sequence"),
            "originalDecisionAt": get(e, "at"),
            "originalPreparedQuantity": get(get(last, "body"), "quantity"),
            "originalPreparedExchangeId": get(get(last, "body"), "exchangeId"),
            "originalPolicy": {
                "ready": get(&old, "proposalReady"),
                "reason": get(&old, "reason"),
                "quantity": chosen_quantity(&old),
            },
            "depthPolicy": {
                "ready": get(&next, "proposalReady"),
                "reason": get(&next, "reason"),
                "quantity": chosen_quantity(&next),
                "chosenExchangeId": chosen_field(&next, "exchangeId"),
                "unitMargin": get(&next, "conditionalOrdinaryGap"),
                "fullPackageReservation": get(&next, "fullPackageReservation"),
            },
        }));
    }
    let reread = std::fs::read(&file).map_err(|e| e.to_string())?;
    // seed.anchor.sha256: a null anchor throws TypeError in JS.
    if get(&seed, "anchor").is_null() {
        return Err(type_error());
    }
    if sha256(&reread) != get(get(&seed, "anchor"), "sha256").as_str().unwrap_or("") {
        return Err("SOURCE_CHANGED".to_string());
    }
    let out = json!({
        "schemaVersion": 1,
        "observedAt": crate::publication_pack::now_iso(),
        "sourceAnchor": get(&seed, "anchor"),
        "packets": packets,
        "interpretation": "Original clocks and account/risk/rule/depth receipts. Proposal capacity only; no new fill, profitability or historical counterfactual P&L. Selected preparation packets include abandoned attempts and cannot establish broad strategy performance.",
        "networkRequests": 0,
        "actualAdditionalOrders": 0,
    });
    std::fs::write(
        "data/baal-entry-depth-v2-replay-20261006.json",
        format!("{}\n", serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?),
    )
    .map_err(|e| e.to_string())?;
    Ok(out)
}
