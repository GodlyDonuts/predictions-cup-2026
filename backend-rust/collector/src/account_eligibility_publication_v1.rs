// live-paper/account-eligibility-publication-v1.mjs — private sanitized
// eligibility evidence. No balances, sizes, orders or keys.
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_truthy};
use serde_json::{json, Value as V};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("ACCOUNT_ELIGIBILITY_{}", code)); }
    Ok(())
}
fn clock(v: &V) -> Result<i64, String> {
    let s = v.as_str().unwrap_or("");
    let n = parse_iso_ms(s).filter(|x| x.fract() == 0.0 && x.abs() <= 9_007_199_254_740_991.0);
    need(n.is_some() && ms_to_iso(n.unwrap()) == s, "CLOCK")?;
    Ok(n.unwrap() as i64)
}
fn js_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Number(_), V::Number(_)) => a.as_f64().unwrap_or(f64::NAN) == b.as_f64().unwrap_or(f64::NAN),
        _ => a == b,
    }
}
fn jnum(v: &V) -> f64 {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok())).unwrap_or(f64::NAN)
}
// Map.get has no string coercion in JS — tag keys by JSON type.
fn map_key(v: &V) -> String {
    match v {
        V::Number(_) => format!("n\u{1}{}", jnum(v)),
        V::String(s) => format!("s\u{1}{}", s),
        _ => format!("o\u{1}{}", baal_service_runtime::json_text::js_stringify(v)),
    }
}
fn push_unique(v: &mut Vec<V>, x: V) {
    if !v.iter().any(|e| js_eq(e, &x)) { v.push(x); }
}
/// eligibilitySnapshot({positions, orders, ordersAt, universe})
pub fn eligibility_snapshot(positions: &V, orders: &V, orders_at: &V, universe: &V) -> Result<V, String> {
    need(positions["data"]["positions"].is_array() && orders.is_array(), "INCOMPLETE")?;
    let p = clock(&positions["receivedAt"])?;
    let o = clock(orders_at)?;
    let mut members: HashMap<String, V> = HashMap::new();
    for x in universe["exchanges"].as_array().cloned().unwrap_or_default() {
        members.insert(map_key(&x["exchangeId"]), x["marketId"].clone());
    }
    let mut excluded: Vec<V> = Vec::new();
    need(o >= p, "CLOCK")?;
    for (rows, is_order) in [
        (positions["data"]["positions"].as_array().cloned().unwrap_or_default(), false),
        (orders.as_array().cloned().unwrap_or_default(), true),
    ] {
        for row in rows {
            need(js_truthy(Some(&row)) && row["exchangeId"].is_string()
                && js_int(&row["quantity"]).is_some()
                && (!is_order || js_int(&row["quantity"]).unwrap() > 0), "ROW")?;
            if !is_order && js_int(&row["quantity"]) == Some(0) { continue; }
            let market = members.get(&map_key(&row["exchangeId"])).cloned();
            need(market.is_some()
                && (!js_truthy(Some(&row["marketId"])) || js_eq(&row["marketId"], market.as_ref().unwrap())), "SCOPE")?;
            push_unique(&mut excluded, market.unwrap());
        }
    }
    excluded.sort_by(|a, b| jnum(a).partial_cmp(&jnum(b)).unwrap_or(std::cmp::Ordering::Equal));
    Ok(json!({"tournamentId": universe["tournamentId"], "universeHash": universe["hash"],
        "positionsAt": positions["receivedAt"], "ordersAt": orders_at,
        "expiresAt": ms_to_iso((p.min(o) + 20000) as f64),
        "excludedMarketIds": excluded, "complete": true}))
}
/// createAccountEligibilityPublication({file, universe, canOperate})
pub struct AccountEligibilityPublication {
    journal: Journal,
    universe: V,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
}
impl AccountEligibilityPublication {
    pub fn publish(&mut self, input: &V) -> Result<V, String> {
        need((self.can_operate)(), "STOPPED")?;
        let snapshot = eligibility_snapshot(&input["positions"], &input["orders"], &input["ordersAt"], &self.universe)?;
        if let Some(last) = &self.journal.last {
            need(clock(&snapshot["positionsAt"])? >= clock(&last["event"]["snapshot"]["positionsAt"])?
                && clock(&snapshot["ordersAt"])? >= clock(&last["event"]["snapshot"]["ordersAt"])?, "ROLLBACK")?;
        }
        self.journal.commit(&V::Null, &json!({"type": "market_account_eligibility", "snapshot": snapshot}))?;
        Ok(json!({"sequence": self.journal.sequence, "expiresAt": snapshot["expiresAt"]}))
    }
    pub fn status(&self) -> V {
        json!({"sequence": self.journal.sequence, "hash": self.journal.previous,
            "networkRequests": 0, "competitionOrders": 0})
    }
}
/// createAccountEligibilityPublication({file, universe, canOperate})
pub fn create_account_eligibility_publication(file: &Path, universe: &V,
    can_operate: Option<Arc<dyn Fn() -> bool + Send + Sync>>) -> Result<AccountEligibilityPublication, String> {
    let journal = Journal::open(file)?;
    if let Some(last) = &journal.last {
        need(last["state"].is_null()
            && last["event"]["type"].as_str() == Some("market_account_eligibility")
            && js_eq(&last["event"]["snapshot"]["universeHash"], &universe["hash"]), "SCOPE_CHANGED")?;
    }
    Ok(AccountEligibilityPublication {
        journal,
        universe: universe.clone(),
        can_operate: can_operate.unwrap_or_else(|| Arc::new(|| true)),
    })
}
