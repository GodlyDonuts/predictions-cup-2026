// live-paper/passive-opportunity-audit.mjs — audit of observed counterparties;
// never orders, queue priority or fills.
use num_bigint::BigInt;
use serde_json::{json, Value as V};

fn instant_ns(v: &V) -> i128 {
    baal_service_runtime::contract::instant_ns(v.as_str().unwrap_or(""))
        .unwrap_or(i128::MIN)
}
/// auditPassiveOpportunities(quotes, pages)
pub fn audit_passive_opportunities(quotes: &[V], pages: &[V]) -> Result<V, String> {
    let mut trades: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    for page in pages {
        for t in page["trades"].as_array().cloned().unwrap_or_default() {
            let key = t["key"].as_str().unwrap_or("").to_string();
            if !trades.contains_key(&key) {
                let mut x = t.clone();
                x["exchangeId"] = page["exchangeId"].clone();
                x["firstAvailableAt"] = page["receivedAt"].clone();
                trades.insert(key, x);
            }
        }
    }
    let tick = BigInt::from(baal_paper::fixed::TICK);
    let mut out = Vec::new();
    for q in quotes {
        let start = instant_ns(&q["activationAt"]);
        let end = instant_ns(&q["expiresAt"]);
        let limit = baal_paper::fixed::big_amount(q["yesEquivalentLimit"].as_str().unwrap_or(""))?;
        let direction = if (q["action"].as_str() == Some("buy")) == (q["side"].as_str() == Some("YES")) {
            BigInt::from(-1)
        } else {
            BigInt::from(1)
        };
        let mut eligible = 0i64;
        let mut unusable = 0i64;
        let mut touch_events = 0i64;
        let mut touch_shares = 0i64;
        let mut through_events = 0i64;
        let mut through_shares = 0i64;
        let mut late_after_resolution = 0i64;
        for t in trades.values() {
            if t["exchangeId"] != q["exchangeId"]
                || instant_ns(&t["createdAt"]) < start
                || instant_ns(&t["createdAt"]) >= end {
                continue;
            }
            if !q["resolvedAt"].is_null()
                && instant_ns(&t["firstAvailableAt"]) > instant_ns(&q["resolvedAt"]) {
                late_after_resolution += 1;
                continue;
            }
            eligible += 1;
            let px = crate::canonical_entry_exit_study::tape_tick(t)?;
            let Some(px) = px else { unusable += 1; continue };
            let delta = (baal_paper::fixed::big_amount(px["price"].as_str().unwrap_or(""))? - &limit) * &direction;
            if delta == BigInt::from(0) {
                touch_events += 1;
                touch_shares += t["size"].as_i64().unwrap_or(0);
            }
            if delta >= tick {
                through_events += 1;
                through_shares += t["size"].as_i64().unwrap_or(0);
            }
        }
        out.push(json!({"quoteId":q["id"],"marketId":q["marketId"],"exchangeId":q["exchangeId"],
            "side":q["side"],"action":q["action"],"limit":q["limit"],"createdAt":q["createdAt"],
            "activationAt":q["activationAt"],"expiresAt":q["expiresAt"],"resolution":q["resolution"],
            "eligibleUniqueTrades":eligible,"unusablePrices":unusable,
            "touchEvents":touch_events,"touchShares":touch_shares,
            "strictThroughEvents":through_events,"strictThroughShares":through_shares,
            "lateAfterResolution":late_after_resolution,
            "queuePositionProven":false,"executionProven":false}));
    }
    Ok(json!(out))
}
