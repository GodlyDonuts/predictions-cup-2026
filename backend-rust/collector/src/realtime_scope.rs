// live-paper/realtime-scope.mjs
use baal_service_runtime::contract::{config, require_that};
use serde_json::{json, Value as V};
pub fn realtime_scopes(tournament_id: &str, markets: &[V], channel_mode: &str) -> Result<V, baal_service_runtime::contract::ContractError> {
    use baal_service_runtime::contract::require_that as rt;
    rt(["per_market", "tournament"].contains(&channel_mode), "INVALID_CHANNEL_MODE")?;
    rt(!markets.is_empty() && markets.len() <= if channel_mode == "tournament" { 300 } else { 4 }, "INVALID_MARKET_SCOPE")?;
    let scopes: Result<Vec<V>, _> = markets.iter().map(|m| {
        config(&json!({"tournamentId": tournament_id, "marketId": m["marketId"], "exchangeIds": [m["exchangeId"]]}))
            .map(|s| json!({"tournamentId": s.tournament_id, "marketId": s.market_id, "exchangeIds": s.exchange_ids}))
    }).collect();
    let scopes = scopes?;
    let mut mids = std::collections::HashSet::new();
    let mut eids = std::collections::HashSet::new();
    let dup = scopes.iter().any(|s| !mids.insert(s["marketId"].as_str().unwrap().to_string()))
        || scopes.iter().flat_map(|s| s["exchangeIds"].as_array().unwrap().iter().filter_map(|e| e.as_str())).collect::<Vec<_>>().iter().any(|e| !eids.insert(e.to_string()))
        || eids.len() != scopes.len();
    rt(!dup, "DUPLICATE_MARKET_SCOPE")?;
    Ok(json!({"scopes": scopes, "channelMode": channel_mode,
        "topic": if channel_mode == "tournament" { json!(format!("tournament:{}:markets", tournament_id)) } else { V::Null }}))
}
/// realtimeCaptureScope(activation, baseline, universe)
pub fn realtime_capture_scope(activation: &V, baseline: &V, universe: &V) -> Result<V, baal_service_runtime::contract::ContractError> {
    use baal_service_runtime::contract::require_that as rt;
    rt(activation.get("mode").and_then(V::as_str) == Some("market_capture_only")
        && activation.get("enabled") == Some(&V::Bool(true)), "INVALID_REALTIME_ACTIVATION")?;
    let sv = activation.get("schemaVersion").and_then(V::as_i64);
    rt(sv == Some(1) && (activation.get("channelMode").is_none() || activation["channelMode"] == "per_market")
        || sv == Some(2) && activation["channelMode"].as_str() == Some("tournament"), "INVALID_REALTIME_ACTIVATION")?;
    let channel_mode = if sv == Some(2) { "tournament" } else { "per_market" };
    rt(universe.get("tournamentId") == baseline.get("tournamentId")
        && universe.get("exchanges").is_some_and(V::is_array), "INVALID_REALTIME_UNIVERSE")?;
    let exchanges = universe["exchanges"].as_array().unwrap();
    let markets: Vec<V> = if channel_mode == "tournament" {
        exchanges.iter().map(|m| json!({"marketId": m["marketId"], "exchangeId": m["exchangeId"]})).collect()
    } else {
        baseline["marketIds"].as_array().cloned().unwrap_or_default().iter().map(|mid| {
            let spec = exchanges.iter().find(|m| m["marketId"] == *mid);
            json!({"marketId": mid, "exchangeId": spec.map(|s| s["exchangeId"].clone()).unwrap_or(V::Null)})
        }).collect()
    };
    let scope = json!({"tournamentId": baseline["tournamentId"], "markets": markets, "channelMode": channel_mode});
    realtime_scopes(tournament_id_from(&scope), scope["markets"].as_array().unwrap(), channel_mode)?;
    Ok(scope)
}
fn tournament_id_from(scope: &V) -> &str { scope["tournamentId"].as_str().unwrap_or("") }
