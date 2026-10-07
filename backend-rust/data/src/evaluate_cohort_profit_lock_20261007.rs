// data/evaluate-cohort-profit-lock-20261007.mjs — dated source-backed
// candidate, no API or trading. Each sequential full fill is assumed only to
// illustrate the floor; every partial prefix is also bounded.
use baal_execution::core::payload_hash;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::{amount, money, SCALE};
use baal_service_runtime::json_text::{js_stringify, js_truthy};
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

use crate::baal_generic_no_controller_v5::{
    baal_generic_no_controller_v5_identity, replay_baal_generic_no_journal_v5,
};
use crate::cohort_profit_locked_no_exit_v1::size_cohort_profit_locked_no_exit_v1;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default())
        .join("SigTradingCup")
        .join("paper-runtime")
}
// JS `===` on JSON scalars: numbers compare by value across int/float forms.
fn js_strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Number(x), V::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    let req: V = serde_json::from_slice(
        &std::fs::read(r.join("baal-generic-live-request-v3.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let actor = std::fs::read(
        r.join("baal-generic-live-runs")
            .join(get(&req, "runId").as_str().unwrap_or(""))
            .join(get(&req, "epoch").as_str().unwrap_or(""))
            .join("controller")
            .join("baal-generic-no-v5-journal.jsonl"),
    )
    .map_err(|e| e.to_string())?;
    let actor_text = String::from_utf8_lossy(&actor).to_string();
    let mut rows: Vec<V> = Vec::new();
    for line in actor_text.trim_end().split('\n') {
        rows.push(serde_json::from_str(line).map_err(|e| e.to_string())?);
    }
    let custody_seed = get(
        &get(&get(rows.first().unwrap_or(&V::Null), "event"), "input"),
        "custodySeed",
    )
    .clone();
    let replay = replay_baal_generic_no_journal_v5(
        &actor_text,
        &baal_generic_no_controller_v5_identity()?,
        &custody_seed,
    )?;
    let depth: V = serde_json::from_slice(
        &std::fs::read(root.join("data/baal-current-depth-value-20261007.json"))
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut realized = amount(get(&depth, "realized").as_str().unwrap_or(""))?;
    let inventory = get(&get(&replay, "state"), "inventory");
    let mut plans: Vec<V> = Vec::new();
    for m in get(&depth, "markets").as_array().cloned().unwrap_or_default() {
        if !js_truthy(m.get("afterOwnership"))
            || !js_truthy(get(&m, "baseline").get("complete"))
        {
            return Err("No post-ownership full depth".into());
        }
        let exchange_id = get(&m, "exchangeId").as_str().unwrap_or("");
        let proof = inventory.get(exchange_id).and_then(|p| p.get("proof"));
        let p = match proof {
            Some(p) if !p.is_null() => p,
            _ => return Err("Changed position".into()),
        };
        if !js_strict_eq(get(p, "remainingOwnedQuantity"), get(&m, "quantity"))
            || !js_strict_eq(get(p, "remainingOwnedCost"), get(&m, "cost"))
        {
            return Err("Changed position".into());
        }
        let consumed = get(&get(&m, "baseline"), "consumed");
        let worst = consumed
            .as_array()
            .and_then(|a| a.last())
            .cloned()
            .unwrap_or(V::Null);
        let plan = size_cohort_profit_locked_no_exit_v1(&json!({
            "remainingLots": get(p, "remainingLots"),
            "remainingOwnedQuantity": get(p, "remainingOwnedQuantity"),
            "yesAskLevel": {
                "price": money(SCALE - amount(get(&worst, "noPrice").as_str().unwrap_or(""))?),
                "quantity": get(&m, "quantity"),
            },
            "realizedProfit": money(realized),
        }))?;
        plans.push(json!({
            "exchangeId": get(&m, "exchangeId"),
            "marketId": get(&m, "marketId"),
            "sourceRecordHash": get(&get(&m, "source"), "recordHash"),
            "sourceReceivedAt": get(&get(&m, "source"), "receivedAt"),
            "plan": plan,
        }));
        realized += amount(get(&plan, "protectedTradePnlIfFilled").as_str().unwrap_or(""))?;
    }
    let selected_shares: i64 = plans
        .iter()
        .map(|p| get(&get(p, "plan"), "quantity").as_i64().unwrap_or(0))
        .sum();
    let remaining_shares: i64 = get(&depth, "markets")
        .as_array()
        .map(|a| {
            a.iter()
                .map(|m| get(m, "quantity").as_i64().unwrap_or(0))
                .sum()
        })
        .unwrap_or(0)
        - selected_shares;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0);
    let report = json!({
        "checkedAt": ms_to_iso(now),
        "mode": "dated_sequential_cohort_profit_lock_candidate_v1",
        "actorAnchor": get(&replay, "anchor"),
        "depthReportHash": payload_hash(&depth)?,
        "startingRealized": get(&depth, "realized"),
        "selectedShares": selected_shares,
        "remainingShares": remaining_shares,
        "conditionalCohortRealizedAfterAllSelectedFills": money(realized),
        "minimumCohortFloor": "0.005000",
        "plans": plans,
        "executionAuthority": false,
        "actualOrders": 0,
        "deployed": false,
        "limitations": [
            "All fills are hypothetical; later fresh account/book/rule and graph/collateral checks remain required.",
            "Displayed liquidity and a profit floor do not guarantee fills or exclude own resting orders.",
            "Realized profit floor alone does not bound losses on any shares left unsold.",
        ],
    });
    std::fs::write(
        root.join("data/baal-cohort-profit-lock-candidate-20261007.json"),
        format!("{}\n", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?),
    )
    .map_err(|e| e.to_string())?;
    println!(
        "{}",
        js_stringify(&json!({
            "selectedShares": get(&report, "selectedShares"),
            "remainingShares": get(&report, "remainingShares"),
            "conditionalRealized": get(&report, "conditionalCohortRealizedAfterAllSelectedFills"),
            "actualOrders": 0,
            "deployed": false,
        }))
    );
    Ok(report)
}
