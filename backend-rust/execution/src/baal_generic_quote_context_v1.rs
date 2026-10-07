// execution/baal-generic-quote-context-v1.mjs — the collector's bulk rows omit
// tournamentId. Fill only missing context from the already checked envelope;
// never overwrite foreign or future source data.
use crate::party_winner_no_screen_v1::party_winner_quote_hash;
use serde_json::{json, Value as V};

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

/// bindBaalGenericQuoteContextV1({hints,tournamentId,universeHash})
pub fn bind_baal_generic_quote_context_v1(
    hints: &V,
    tournament_id: &str,
    universe_hash: &str,
) -> Result<Vec<V>, String> {
    if get(hints, "tournamentId").as_str() != Some(tournament_id)
        || get(hints, "universeHash").as_str() != Some(universe_hash)
        || !get(hints, "quoteCaptures").is_array()
    {
        return Err("GENERIC_QUOTE_ENVELOPE".to_string());
    }
    let mut out = Vec::new();
    for q in get(hints, "quoteCaptures").as_array().unwrap() {
        let mut value = q.as_object().cloned().unwrap_or_default();
        if q.get("tournamentId").map(|v| v.is_null()).unwrap_or(true) {
            value.insert("tournamentId".into(), json!(tournament_id));
        }
        if q.get("knownAt").map(|v| v.is_null()).unwrap_or(true) {
            value.insert("knownAt".into(), get(q, "receivedAt").clone());
        }
        if q.get("quoteHash").map(|v| v.is_null()).unwrap_or(true) {
            let h = party_winner_quote_hash(&V::Object(value.clone()))?;
            value.insert("quoteHash".into(), json!(h));
        }
        out.push(V::Object(value));
    }
    Ok(out)
}
