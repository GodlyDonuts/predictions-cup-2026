use crate::atomic::need;
use crate::json_text::{digest, js_int};
use serde_json::{json, Map, Value as V};

pub const TOURNAMENT_ID: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";

fn parse_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis())
}
fn iso(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}
fn market_id_ok(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
}
pub fn encode_uri_component(s: &str) -> String {
    // JS encodeURIComponent leaves A-Za-z0-9 - _ . ! ~ * ' ( ) unescaped.
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*'
            | b'\'' | b'(' | b')' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// live-paper/history.mjs historyEndpoint.
pub fn history_endpoint(exchange_id: &str, at: &str) -> Result<String, String> {
    let ms = parse_ms(at);
    need(market_id_ok(exchange_id) && ms.is_some(), "Invalid history request")?;
    let to = iso(ms.unwrap() / 60000 * 60000);
    Ok(format!(
        "/exchanges/{exchange_id}/price-history?tournamentId={TOURNAMENT_ID}&resolution=1m&limit=500&to={}",
        encode_uri_component(&to)
    ))
}

// allowedHistoryEndpoint: URL-validate then byte-compare the regenerated form.
pub fn allowed_history_endpoint(endpoint: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(&format!("https://sig.thesuper.market/api/v1{endpoint}")) else {
        return false;
    };
    let path = u.path();
    let Some(rest) = path.strip_prefix("/api/v1/exchanges/") else { return false };
    let Some(id) = rest.strip_suffix("/price-history") else { return false };
    if !market_id_ok(id) {
        return false;
    }
    let Some(at) = u.query_pairs().find(|(k, _)| k == "to").map(|(_, v)| v.into_owned()) else {
        return false;
    };
    match history_endpoint(id, &at) {
        Ok(regen) => regen == endpoint,
        Err(_) => false,
    }
}

fn finite(v: &V) -> bool {
    v.as_f64().is_some_and(|x| x.is_finite())
}
fn price_opt(v: &V) -> bool {
    v.is_null() || (finite(v) && (0.0..=1.0).contains(&v.as_f64().unwrap()))
}

// normalizeHistory: strict response scope, then keep only candles that
// actually traded (volume>0 && tradeCount>0) with consistent OHLC bounds.
pub fn normalize_history(response: &V, spec: &V) -> Result<V, String> {
    let endpoint = response.get("endpoint").and_then(V::as_str).unwrap_or("");
    let d = response.get("data").unwrap_or(&V::Null);
    let started = response.get("requestStartedAt").and_then(V::as_str).and_then(parse_ms);
    let received = response.get("receivedAt").and_then(V::as_str).and_then(parse_ms);
    let url = reqwest::Url::parse(&format!("https://sig.thesuper.market/api/v1{endpoint}"))
        .map_err(|_| "Invalid historical response scope".to_string())?;
    let expected_path = format!("/api/v1/exchanges/{}/price-history", spec.get("exchangeId").and_then(V::as_str).unwrap_or(""));
    let timing_ok = matches!((started, received), (Some(s), Some(r)) if r >= s && r - s <= 20000);
    let q = |k: &str| url.query_pairs().find(|(x, _)| x == k).map(|(_, v)| v.into_owned());
    let coverage = d.get("coverage").unwrap_or(&V::Null);
    let projected = coverage.get("projectedThroughSequence").unwrap_or(&V::Null);
    need(
        timing_ok
            && url.path() == expected_path
            && q("tournamentId").as_deref() == Some(TOURNAMENT_ID)
            && q("resolution").as_deref() == Some("1m")
            && q("limit").as_deref() == Some("500")
            && d.get("exchangeId") == spec.get("exchangeId")
            && d.get("marketId") == spec.get("marketId")
            && d.get("resolution").and_then(V::as_str) == Some("1m")
            && d.get("candles").and_then(V::as_array).is_some_and(|c| c.len() <= 500)
            && coverage.get("complete").is_some_and(V::is_boolean)
            && (projected.is_null() || js_int(projected).is_some_and(|x| x >= 0)),
        "Invalid historical response scope",
    )?;
    let from = d.get("from").and_then(V::as_str).and_then(parse_ms);
    let to = d.get("to").and_then(V::as_str).and_then(parse_ms);
    let requested_to = q("to").and_then(|s| parse_ms(&s));
    need(
        match (from, to, requested_to, started) {
            (Some(f), Some(t), Some(r), Some(s)) => t > f && t <= r && t <= s && t % 60000 == 0,
            _ => false,
        },
        "Invalid historical window",
    )?;
    let (from, to) = (from.unwrap(), to.unwrap());
    let mut seen = std::collections::BTreeSet::new();
    let mut candles = Vec::new();
    for c in d.get("candles").and_then(V::as_array).unwrap() {
        let at = c.get("time").and_then(V::as_str).and_then(parse_ms);
        let volume = c.get("volume").and_then(V::as_f64);
        let trade_count = c.get("tradeCount").map(|v| js_int(v)).flatten();
        need(
            at.is_some_and(|a| a % 60000 == 0 && a >= from && a + 60000 <= to)
                && at.is_some_and(|a| !seen.contains(&a))
                && volume.is_some_and(|v| v.is_finite() && v >= 0.0)
                && trade_count.is_some_and(|t| t >= 0),
            "Invalid candle clock/count",
        )?;
        seen.insert(at.unwrap());
        for k in ["open", "high", "low", "close", "vwap"] {
            need(price_opt(c.get(k).unwrap_or(&V::Null)), "Invalid candle price")?;
        }
        if volume.unwrap() > 0.0 && trade_count.unwrap() > 0 {
            let get = |k: &str| c.get(k).unwrap_or(&V::Null);
            need(
                ["open", "high", "low", "close", "vwap"].iter().all(|k| !get(k).is_null())
                    && get("low").as_f64().unwrap() <= get("high").as_f64().unwrap()
                    && ["open", "close", "vwap"].iter().all(|k| {
                        let x = get(k).as_f64().unwrap();
                        x >= get("low").as_f64().unwrap() - 1e-12
                            && x <= get("high").as_f64().unwrap() + 1e-12
                    }),
                "Invalid OHLC bounds",
            )?;
            let mut c2 = c.clone();
            c2.as_object_mut().unwrap().insert("time".into(), json!(iso(at.unwrap())));
            candles.push(c2);
        }
    }
    candles.sort_by_key(|c| c.get("time").and_then(V::as_str).and_then(parse_ms).unwrap_or(0));
    let mut out = Map::new();
    out.insert("exchangeId".into(), spec.get("exchangeId").unwrap().clone());
    out.insert("marketId".into(), spec.get("marketId").unwrap().clone());
    out.insert("tournamentId".into(), json!(TOURNAMENT_ID));
    out.insert("resolution".into(), json!("1m"));
    out.insert("from".into(), d.get("from").unwrap().clone());
    out.insert("to".into(), d.get("to").unwrap().clone());
    out.insert("receivedAt".into(), response.get("receivedAt").unwrap().clone());
    out.insert("requestStartedAt".into(), response.get("requestStartedAt").unwrap().clone());
    out.insert("coverage".into(), coverage.clone());
    out.insert("candles".into(), json!(candles));
    out.insert("source".into(), json!("official_tournament_trade_candles"));
    out.insert("sparse".into(), json!(true));
    out.insert("executable".into(), json!(false));
    out.insert(
        "hash".into(),
        json!(digest(&json!({"endpoint": endpoint, "data": d}))),
    );
    Ok(V::Object(out))
}

fn median(values: &[f64]) -> f64 {
    let mut x = values.to_vec();
    x.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = x.len();
    if n % 2 == 1 { x[(n - 1) / 2] } else { (x[n / 2 - 1] + x[n / 2]) / 2.0 }
}

// historicalReference: closed 1m trade-candle vwap median, falling back to
// prior verified orderbook midpoints; robust deviation = max(0.0025, 1.4826*MAD).
pub fn historical_reference(history: Option<&V>, books: &[V], at: &str, p: &V) -> Result<V, String> {
    let now = parse_ms(at).ok_or("Invalid reference clock")?;
    let cache_age = p.get("historyCacheAgeMs").and_then(V::as_f64).unwrap_or(0.0);
    let lookback = p.get("lookbackMs").and_then(V::as_f64).unwrap_or(0.0);
    let minimum_samples = p.get("minimumSamples").and_then(V::as_f64).unwrap_or(0.0) as usize;
    let minimum_span = p.get("minimumSpanMs").and_then(V::as_f64).unwrap_or(0.0);
    let stale = p.get("lastHistoricalObservationAgeMs").and_then(V::as_f64).unwrap_or(0.0);
    let mut points: Vec<(i64, f64)> = Vec::new();
    let mut source = "";
    if let Some(h) = history {
        let received = h.get("receivedAt").and_then(V::as_str).and_then(parse_ms);
        let complete = h.get("coverage").and_then(|c| c.get("complete")).and_then(V::as_bool) == Some(true);
        if received.is_some_and(|r| r <= now && now - r <= cache_age as i64) && complete {
            if let Some(cs) = h.get("candles").and_then(V::as_array) {
                for c in cs {
                    if let (Some(t), Some(v)) = (
                        c.get("time").and_then(V::as_str).and_then(parse_ms),
                        c.get("vwap").and_then(V::as_f64),
                    ) {
                        if t + 60000 <= now && t >= now - lookback as i64 {
                            points.push((t + 60000, v));
                        }
                    }
                }
            }
            source = "closed_trade_candle_vwap_median";
        }
    }
    if points.len() < minimum_samples {
        points.clear();
        for b in books {
            if let (Some(t), Some(r), Some(mid)) = (
                b.get("at").and_then(V::as_str).and_then(parse_ms),
                b.get("receivedAt").and_then(V::as_str).and_then(parse_ms),
                b.get("mid").and_then(V::as_f64),
            ) {
                if t < now && r <= now && t >= now - lookback as i64 {
                    points.push((t, mid));
                }
            }
        }
        source = "prior_verified_orderbook_midpoint_median";
    }
    if points.len() < minimum_samples {
        return Ok(json!({"ready": false, "reason": "insufficient_history", "samples": points.len()}));
    }
    let first = points.iter().map(|x| x.0).min().unwrap();
    let last = points.iter().map(|x| x.0).max().unwrap();
    if last - first < minimum_span as i64 {
        return Ok(json!({"ready": false, "reason": "insufficient_history_span", "samples": points.len()}));
    }
    if now - last > stale as i64 {
        return Ok(json!({"ready": false, "reason": "historical_reference_stale", "samples": points.len()}));
    }
    let center = median(&points.iter().map(|x| x.1).collect::<Vec<_>>());
    let mad = median(&points.iter().map(|x| (x.1 - center).abs()).collect::<Vec<_>>());
    Ok(json!({
        "ready": true,
        "source": source,
        "samples": points.len(),
        "firstAt": iso(first),
        "lastAt": iso(last),
        "center": center,
        "robustDeviation": f64::max(0.0025, mad * 1.4826),
    }))
}
