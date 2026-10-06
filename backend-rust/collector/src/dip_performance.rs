// live-paper/dip-performance.mjs — FIFO episode accounting in BigInt micros.
use baal_paper::fixed::{big_amount, money};
use baal_service_runtime::contract::parse_iso_ms;
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::HashMap;
/// dipPerformance(fills, positions, now)
pub fn dip_performance(fills: &[V], positions: &[V], now: &str) -> Result<V, String> {
    let mut inventory: HashMap<String, Vec<(i64, BigInt)>> = HashMap::new();
    let mut episodes: HashMap<String, (BigInt, BigInt, i64)> = HashMap::new(); // cost, pnl, shares
    let mut completed: Vec<V> = Vec::new();
    let (mut sold_basis, mut realized, mut buy_cost, mut sold_shares) = (BigInt::from(0), BigInt::from(0), BigInt::from(0), 0i64);
    for f in fills {
        let inst = f["instrument"].as_str().unwrap_or("").to_string();
        let lots = inventory.remove(&inst).unwrap_or_default();
        let mut ep = episodes.remove(&inst).unwrap_or((BigInt::from(0), BigInt::from(0), 0));
        let qty = f["quantity"].as_i64().unwrap_or(0);
        let notional = big_amount(f["price"].as_str().unwrap_or(""))? * BigInt::from(qty);
        let charge = big_amount(f["fee"].as_str().unwrap_or(""))?;
        let mut lots = lots;
        match f["action"].as_str().unwrap_or("") {
            "buy" => {
                let cost = notional + charge;
                lots.push((qty, cost.clone()));
                buy_cost += &cost; ep.0 += &cost; ep.2 += qty;
            }
            "sell" => {
                let mut left = qty;
                let mut basis = BigInt::from(0);
                while left > 0 {
                    let Some(lot) = lots.first_mut() else { return Err("Performance ledger oversell".into()); };
                    let q = left.min(lot.0);
                    let part = if q == lot.0 { lot.1.clone() } else { &lot.1 * BigInt::from(q) / BigInt::from(lot.0) };
                    basis += &part; lot.1 -= part; lot.0 -= q; left -= q;
                    if lot.0 == 0 { lots.remove(0); }
                }
                let pnl = notional - charge - &basis;
                sold_basis += &basis; sold_shares += qty; realized += &pnl; ep.1 += &pnl;
                if lots.is_empty() {
                    completed.push(json!({"instrument": inst, "cost": money(&ep.0), "pnl": money(&ep.1),
                        "shares": ep.2, "closedAt": f["at"]}));
                    episodes.remove(&inst);
                }
            }
            _ => return Err("Invalid performance fill".into()),
        }
        inventory.insert(inst.clone(), lots.clone());
        if !lots.is_empty() { episodes.insert(inst, ep); }
    }
    let wins = completed.iter().filter(|e| big_amount(e["pnl"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0)) > BigInt::from(0)).count();
    let losses = completed.iter().filter(|e| big_amount(e["pnl"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0)) < BigInt::from(0)).count();
    let positive: BigInt = completed.iter().filter_map(|e| { let p = big_amount(e["pnl"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0)); if p > BigInt::from(0) { Some(p) } else { None } }).sum();
    let negative: BigInt = completed.iter().filter_map(|e| { let p = big_amount(e["pnl"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0)); if p < BigInt::from(0) { Some(-p) } else { None } }).sum();
    let open_cost: BigInt = inventory.values().flatten().map(|l| l.1.clone()).sum();
    let now_ms = parse_iso_ms(now).unwrap_or(f64::NAN);
    Ok(json!({
        "closedRoundTrips": completed.len(), "wins": wins, "losses": losses,
        "breakeven": completed.len() - wins - losses,
        "winRate": if completed.is_empty() { V::Null } else { json!(wins as f64 / completed.len() as f64) },
        "realizedPnl": money(&realized), "totalEntryCost": money(&buy_cost),
        "soldCostBasis": money(&sold_basis), "openCostBasis": money(&open_cost),
        "fillWeightedRealizedReturn": if sold_basis == BigInt::from(0) { V::Null } else {
            json!(realized.to_string().parse::<f64>().unwrap_or(0.0) / sold_basis.to_string().parse::<f64>().unwrap_or(1.0)) },
        "realizedPnlPerSoldShare": if sold_shares == 0 { V::Null } else {
            json!(realized.to_string().parse::<f64>().unwrap_or(0.0) / 1e6 / sold_shares as f64) },
        "closedProfitFactor": if negative == BigInt::from(0) { V::Null } else {
            json!(positive.to_string().parse::<f64>().unwrap_or(0.0) / negative.to_string().parse::<f64>().unwrap_or(1.0)) },
        "oldestInventoryAgeMs": if positions.is_empty() { V::Null } else {
            json!(positions.iter().map(|p| (now_ms - parse_iso_ms(p["enteredAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)).max(0.0))
                .fold(f64::NEG_INFINITY, f64::max)) },
        "recentClosedEpisodes": completed.iter().rev().take(25).rev().cloned().collect::<Vec<_>>(),
    }))
}
