// live-paper/market-price-publication.mjs — price observation journal (publisher side).
use baal_service_runtime::journal::Journal;
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;
use std::path::Path;

const KEYS: [&str; 9] = ["tournamentId","universeHash","requestStartedAt","receivedAt","sourceTimestamp","depthAvailable","executable","quotes","missingIds"];
const QUOTE_KEYS: [&str; 8] = ["exchangeId","marketId","option","latestPrice","bestBid","bestAsk","spread","crossed"];
fn exact(v: &V, fields: &[&str]) -> bool { v.as_object().is_some_and(|m| m.len() == fields.len() && fields.iter().all(|k| m.contains_key(*k))) }
/// validatePriceObservation(snapshot, universe)
pub fn validate_price_observation_pub(snapshot: &V, universe: &V) -> Result<V, String> {
    if !exact(snapshot, &KEYS)
        || snapshot["universeHash"] != universe["hash"] || snapshot["tournamentId"] != universe["tournamentId"]
        || snapshot["executable"] != V::Bool(false) || snapshot["depthAvailable"] != V::Bool(false)
        || !snapshot["sourceTimestamp"].is_null() || !snapshot["quotes"].is_array() || !snapshot["missingIds"].is_array() {
        return Err("MARKET_PRICE_PUBLICATION_NOT_NORMALIZED".into());
    }
    let start = parse_iso_ms(snapshot["requestStartedAt"].as_str().unwrap_or(""));
    let end = parse_iso_ms(snapshot["receivedAt"].as_str().unwrap_or(""));
    if start.is_none() || end.is_none() || end.unwrap() < start.unwrap()
        || baal_service_runtime::contract::ms_to_iso(start.unwrap()) != snapshot["requestStartedAt"].as_str().unwrap_or("")
        || baal_service_runtime::contract::ms_to_iso(end.unwrap()) != snapshot["receivedAt"].as_str().unwrap_or("") {
        return Err("MARKET_PRICE_PUBLICATION_CLOCK".into());
    }
    let rows = universe["exchanges"].as_array().cloned().unwrap_or_default();
    let members: std::collections::HashMap<String, V> = rows.into_iter()
        .map(|x| (x["exchangeId"].as_str().unwrap_or("").to_string(), x)).collect();
    let mut seen = BTreeSet::new();
    for q in snapshot["quotes"].as_array().unwrap() {
        let spec = members.get(q["exchangeId"].as_str().unwrap_or("")).cloned();
        if !exact(q, &QUOTE_KEYS) || spec.is_none()
            || spec.as_ref().unwrap()["marketId"] != q["marketId"] || spec.as_ref().unwrap()["option"] != q["option"]
            || seen.contains(q["exchangeId"].as_str().unwrap_or("")) {
            return Err("MARKET_PRICE_PUBLICATION_IDENTITY".into());
        }
        for k in ["latestPrice", "bestBid", "bestAsk", "spread"] {
            if !q[k].is_null()
                && (!q[k].is_f64() || q[k].as_f64().unwrap() > 1.0 || q[k].as_f64().unwrap() < if k == "spread" { -1.0 } else { 0.0 }) {
                return Err("MARKET_PRICE_PUBLICATION_PRICE".into());
            }
        }
        let crossed = !q["bestBid"].is_null() && !q["bestAsk"].is_null()
            && q["bestBid"].as_f64().unwrap() >= q["bestAsk"].as_f64().unwrap();
        if q["crossed"] != V::Bool(crossed) { return Err("MARKET_PRICE_PUBLICATION_CROSSED".into()); }
        seen.insert(q["exchangeId"].as_str().unwrap_or(""));
    }
    for mid in snapshot["missingIds"].as_array().unwrap() {
        if !members.contains_key(mid.as_str().unwrap_or("")) || seen.contains(mid.as_str().unwrap_or("")) {
            return Err("MARKET_PRICE_PUBLICATION_MISSING".into());
        }
        seen.insert(mid.as_str().unwrap_or(""));
    }
    if seen.is_empty() || seen.len() > 100 { return Err("MARKET_PRICE_PUBLICATION_SIZE".into()); }
    Ok(snapshot.clone())
}
/// createMarketPricePublication({file, universe, canOperate})
pub struct MarketPricePublication<F: Fn() -> bool> { journal: Journal, universe: V, can_operate: F }
impl<F: Fn() -> bool> MarketPricePublication<F> {
    pub fn new(file: &Path, universe: &V, can_operate: F) -> Result<Self, String> {
        let journal = Journal::open(file)?;
        let last_snap = journal.last.as_ref().map(|l| l["event"]["snapshot"]["universeHash"].clone()).unwrap_or(V::Null);
        if !last_snap.is_null() && last_snap != universe["hash"] {
            return Err("MARKET_PRICE_PUBLICATION_SCOPE_CHANGED".into());
        }
        Ok(Self { journal, universe: universe.clone(), can_operate })
    }
    pub fn publish(&mut self, snapshot: &V) -> Result<V, String> {
        if !(self.can_operate)() { return Err("MARKET_PRICE_PUBLICATION_STOPPED".into()); }
        validate_price_observation_pub(snapshot, &self.universe)?;
        let last = self.journal.last.as_ref().map(|l| l["event"]["snapshot"].clone()).unwrap_or(V::Null);
        if !last.is_null()
            && parse_iso_ms(snapshot["receivedAt"].as_str().unwrap_or("")).unwrap_or(0.0)
                < parse_iso_ms(last["receivedAt"].as_str().unwrap_or("")).unwrap_or(0.0) {
            return Err("MARKET_PRICE_PUBLICATION_CLOCK_REVERSED".into());
        }
        self.journal.commit(&V::Null, &json!({"type": "market_price_observation", "snapshot": snapshot}))?;
        let last = self.journal.last.clone().unwrap_or(V::Null);
        Ok(json!({"sequence": self.journal.sequence, "previous": self.journal.previous,
            "at": last["at"], "event": last["event"], "hash": self.journal.previous}))
    }
    pub fn status(&self) -> V {
        json!({"sequence": self.journal.sequence, "hash": self.journal.previous,
            "universeHash": self.universe["hash"], "networkRequests": 0})
    }
}
