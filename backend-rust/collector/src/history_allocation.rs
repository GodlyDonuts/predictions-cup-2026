// live-paper/history-allocation.mjs — read-priority allocation hints.
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::history::historical_reference;
use serde_json::{json, Value as V};

fn canonical_iso(value: &V) -> bool {
    value.as_str().is_some_and(|s| parse_iso_ms(s).is_some()
        && baal_service_runtime::contract::ms_to_iso(parse_iso_ms(s).unwrap()) == s)
}
fn identity_known(v: &V) -> bool {
    v["state"].as_str().is_some_and(|s| !s.trim().is_empty()) && v["office"].as_str().is_some_and(|s| !s.trim().is_empty())
}
fn support_expiry(history: &V, books: &[V], reference: &V, policy: &V, now: i64) -> Result<i64, String> {
    let lookback = policy["lookbackMs"].as_i64().unwrap_or(0);
    let candles = reference["source"].as_str() == Some("closed_trade_candle_vwap_median");
    let mut points: Vec<(i64, i64)> = if candles {
        history["candles"].as_array().cloned().unwrap_or_default().iter()
            .filter_map(|c| parse_iso_ms(c["time"].as_str().unwrap_or("")))
            .filter(|t| *t as i64 + 60000 <= now && *t as i64 >= now - lookback)
            .map(|t| (t as i64, t as i64 + 60000)).collect()
    } else {
        books.iter()
            .filter(|b| parse_iso_ms(b["at"].as_str().unwrap_or("")).map(|t| (t as i64) < now).unwrap_or(false)
                && parse_iso_ms(b["receivedAt"].as_str().unwrap_or("")).map(|t| (t as i64) <= now).unwrap_or(false)
                && parse_iso_ms(b["at"].as_str().unwrap_or("")).map(|t| (t as i64) >= now - lookback).unwrap_or(false))
            .map(|b| (parse_iso_ms(b["at"].as_str().unwrap()).unwrap() as i64,
                      parse_iso_ms(b["at"].as_str().unwrap()).unwrap() as i64)).collect()
    };
    points.sort_by_key(|p| p.0);
    let last = parse_iso_ms(reference["lastAt"].as_str().unwrap_or("")).unwrap_or(0.0) as i64;
    let span_support = points.iter().filter(|p| p.1 <= last - policy["minimumSpanMs"].as_i64().unwrap_or(0)).last();
    let min_samples = policy["minimumSamples"].as_u64().unwrap_or(0) as usize;
    if points.len() < min_samples || span_support.is_none() || min_samples == 0 {
        return Err("History allocation support mismatch".into());
    }
    Ok(points[points.len() - min_samples].0.min(span_support.unwrap().0) + lookback)
}
/// historyAllocation(sources, universe, coverage, now)
pub fn history_allocation(sources: &[V], universe: &V, coverage: Option<&V>, now: i64) -> Result<V, String> {
    if !universe["exchanges"].is_array() { return Err("Invalid history allocation scope/clock".into()); }
    let at = baal_service_runtime::contract::ms_to_iso(now as f64);
    for s in sources {
        if s["state"]["bankHash"].as_str() != Some(&crate::model::digest(&s["bank"]))
            || s["bank"]["universe"]["hash"] != universe["hash"]
            || s["bank"]["config"]["tournamentId"] != universe["tournamentId"] {
            return Err("History allocation source identity mismatch".into());
        }
    }
    let scoped_coverage = coverage.is_some_and(|c| c["universeHash"] == universe["hash"] && c["tournamentId"] == universe["tournamentId"]);
    let mut markets = Vec::new();
    for spec in universe["exchanges"].as_array().unwrap() {
        let quote = if scoped_coverage { coverage.unwrap()["quotes"][spec["exchangeId"].as_str().unwrap_or("")].clone() } else { V::Null };
        let quote_fresh = quote["marketId"] == spec["marketId"] && quote["exchangeId"] == spec["exchangeId"]
            && quote["option"].as_str() == Some("YES")
            && canonical_iso(&quote["receivedAt"])
            && parse_iso_ms(quote["receivedAt"].as_str().unwrap()).map(|t| t <= now as f64 && now as f64 - t <= 600000.0).unwrap_or(false)
            && quote["bestAsk"].is_number() && quote["bestBid"].is_number()
            && quote["bestBid"].as_f64().unwrap_or(0.0) >= 0.0 && quote["bestAsk"].as_f64().unwrap_or(2.0) <= 1.0
            && quote["bestAsk"].as_f64().unwrap() > quote["bestBid"].as_f64().unwrap()
            && quote["crossed"] != V::Bool(true);
        let mut known = false;
        let mut received_at = V::Null;
        let mut best = V::Null;
        let mut plausible = false;
        for s in sources {
            known |= identity_known(&s["bank"]["identities"][spec["marketId"].as_str().unwrap_or("")]);
            let h = &s["state"]["histories"][spec["exchangeId"].as_str().unwrap_or("")];
            let p = &s["bank"]["policy"];
            if !h.is_null()
                && (h["exchangeId"] != spec["exchangeId"] || h["marketId"] != spec["marketId"] || h["tournamentId"] != universe["tournamentId"]) {
                return Err("History allocation capture scope mismatch".into());
            }
            if canonical_iso(&h["receivedAt"]) && parse_iso_ms(h["receivedAt"].as_str().unwrap()).map(|t| t <= now as f64).unwrap_or(false)
                && (received_at.is_null() || parse_iso_ms(h["receivedAt"].as_str().unwrap()).unwrap_or(0.0) > parse_iso_ms(received_at.as_str().unwrap_or("")).unwrap_or(f64::MAX)) {
                received_at = h["receivedAt"].clone();
            }
            let cushion = p.get("slippageTicks").and_then(|x| x.as_f64()).unwrap_or(0.0) * 0.005;
            let plausible_here = quote_fresh
                && quote["bestAsk"].as_f64().unwrap() + cushion >= p["minimumPrice"].as_f64().unwrap_or(0.0)
                && quote["bestAsk"].as_f64().unwrap() + cushion <= p["maximumPrice"].as_f64().unwrap_or(0.0)
                && quote["bestAsk"].as_f64().unwrap() - quote["bestBid"].as_f64().unwrap() + 2.0 * cushion <= p["maximumSpread"].as_f64().unwrap_or(0.0) + 1e-12;
            plausible |= plausible_here;
            if s["healthy"] != V::Bool(true) { continue; }
            let books: Vec<V> = s["state"]["bookHistory"][spec["exchangeId"].as_str().unwrap_or("")].as_array().cloned().unwrap_or_default();
            let ref_v = match historical_reference(if h.is_null() { None } else { Some(h) }, &books, &at, p) {
                Ok(r) => r,
                Err(_) => continue,
            };
            if ref_v["ready"] != V::Bool(true) { continue; }
            let book_history: Vec<V> = books;
            let mut expiry = (parse_iso_ms(ref_v["lastAt"].as_str().unwrap_or("")).unwrap_or(0.0) as i64
                + p["lastHistoricalObservationAgeMs"].as_i64().unwrap_or(0))
                .min(support_expiry(h, &book_history, &ref_v, p, now)?);
            if ref_v["source"].as_str() == Some("closed_trade_candle_vwap_median") {
                expiry = expiry.min(parse_iso_ms(h["receivedAt"].as_str().unwrap_or("")).unwrap_or(0.0) as i64
                    + p["historyCacheAgeMs"].as_i64().unwrap_or(0));
            }
            let required = (p["minimumDip"].as_f64().unwrap_or(0.0))
                .max(ref_v["center"].as_f64().unwrap_or(0.0) * p["minimumRelativeDip"].as_f64().unwrap_or(0.0))
                .max(p["minimumZ"].as_f64().unwrap_or(0.0) * ref_v["robustDeviation"].as_f64().unwrap_or(0.0));
            let gap = if quote_fresh {
                Some(((required - (ref_v["center"].as_f64().unwrap_or(0.0) - quote["bestAsk"].as_f64().unwrap() - cushion)) * 1e6).ceil().clamp(-1000000.0, 1000000.0) as i64)
            } else { None };
            let candidate = json!({"validUntil": baal_service_runtime::contract::ms_to_iso(expiry as f64),
                "opportunityGapMicros": gap});
            if best.is_null() || expiry > parse_iso_ms(best["validUntil"].as_str().unwrap_or("")).unwrap_or(0.0) as i64 {
                best = candidate;
            }
        }
        markets.push(json!({"marketId": spec["marketId"], "referenceReady": !best.is_null(),
            "validUntil": best["validUntil"], "receivedAt": received_at,
            "identityKnown": known, "quotePlausible": plausible,
            "opportunityGapMicros": best["opportunityGapMicros"]}));
    }
    Ok(json!({"schemaVersion": 1, "at": at, "markets": markets}))
}
