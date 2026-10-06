use crate::atomic::need;
use crate::fixed::{amount, money, price};
use crate::json_text::{digest, js_int};
use serde_json::{json, Map, Value as V};

const TID: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
fn numeric_str(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
}
fn timestamp_ok(s: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(s).is_ok()
}
fn finite(v: &V) -> bool {
    v.as_f64().is_some_and(|x| x.is_finite())
}

// live-paper/model.mjs normalizeBook.
pub fn normalize_book(
    response: &V,
    market_id: &str,
    tournament_id: &str,
    received_at: &str,
    request_started_at: &str,
    max_age_ms: f64,
    excluded: &std::collections::BTreeSet<String>,
    freshness_policy: &str,
) -> Result<V, String> {
    need(
        ["event_age_v1", "authoritative_receipt_v1"].contains(&freshness_policy),
        "Unknown book freshness policy",
    )?;
    let now = chrono::DateTime::parse_from_rfc3339(received_at).ok().map(|d| d.timestamp_millis() as f64);
    let started = chrono::DateTime::parse_from_rfc3339(request_started_at).ok().map(|d| d.timestamp_millis() as f64);
    let (now, _started) = match (now, started) {
        (Some(n), Some(s)) if n >= s && n - s <= max_age_ms => (n, s),
        _ => return Err("Slow or invalid snapshot observation".into()),
    };
    let empty: Vec<V> = Vec::new();
    let contexts: Vec<&V> = response
        .get("contexts")
        .and_then(V::as_array)
        .unwrap_or(&empty)
        .iter()
        .filter(|x| x.get("tournament").and_then(|t| t.get("id")).and_then(V::as_str) == Some(tournament_id))
        .collect();
    need(!contexts.is_empty(), "Missing exact tournament book context")?;
    need(
        contexts.iter().all(|x| x.get("type").and_then(V::as_str) == Some("tournament")),
        "Missing exact tournament book context",
    )?;
    let first_digest = digest(contexts[0].get("orderbook").unwrap_or(&V::Null));
    need(
        contexts.iter().all(|x| digest(x.get("orderbook").unwrap_or(&V::Null)) == first_digest),
        "Conflicting tournament contexts",
    )?;
    let context = contexts[0];
    let exchanges = context.get("orderbook").and_then(|o| o.get("exchanges")).and_then(V::as_array);
    need(
        exchanges.is_some_and(|e| e.len() == 1 && e[0].get("option").and_then(V::as_str) == Some("YES")),
        "Only verified single YES exchange books supported",
    )?;
    let e = &exchanges.unwrap()[0];
    let id = e.get("exchangeId").map(|v| {
        v.as_str()
            .map(str::to_string)
            .or_else(|| v.as_i64().map(|n| n.to_string()))
    });
    let id = id.flatten().unwrap_or_default();
    let version = e.get("asOf").unwrap_or(&V::Null);
    let seq = version.get("sequence").map(|v| js_int(v)).flatten();
    let vat = version.get("at").and_then(V::as_str);
    let vat_ms = vat.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp_millis());
    need(
        numeric_str(&id)
            && !version.is_null()
            && seq.is_some_and(|s| s >= 0)
            && vat_ms.is_some()
            && vat_ms.unwrap() as f64 <= now + 2000.0
            && (freshness_policy != "event_age_v1" || now - vat_ms.unwrap() as f64 <= max_age_ms),
        "Missing/stale engine version",
    )?;
    need(!excluded.contains(&id), "Exchange has real account holdings or open orders")?;
    let levels = |rows: Option<&V>, descending: bool| -> Result<Vec<V>, String> {
        let arr = rows.and_then(V::as_array).ok_or("Missing depth")?;
        let mut seen = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for l in arr {
            let p = price(&l.get("price").map(|v| match v {
                V::String(s) => s.clone(),
                V::Number(n) => n.to_string(),
                _ => String::new(),
            }).unwrap_or_default())
            .map_err(|_| "Invalid/duplicate depth".to_string())?;
            let q = js_int(l.get("quantity").unwrap_or(&V::Null));
            need(q.is_some_and(|x| x > 0) && !seen.contains(&p), "Invalid/duplicate depth")?;
            seen.insert(p);
            out.push(json!({"price": money(p), "quantity": q.unwrap()}));
        }
        out.sort_by_key(|r| {
            let v = amount(r.get("price").and_then(V::as_str).unwrap()).unwrap();
            if descending { std::cmp::Reverse(v) } else { std::cmp::Reverse(-v) }
        });
        Ok(out)
    };
    let bids = levels(e.get("bids"), true)?;
    let asks = levels(e.get("asks"), false)?;
    need(
        !bids.is_empty()
            && !asks.is_empty()
            && amount(bids[0].get("price").and_then(V::as_str).unwrap()).unwrap()
                < amount(asks[0].get("price").and_then(V::as_str).unwrap()).unwrap(),
        "Missing, locked or crossed market",
    )?;
    let source_at = vat.unwrap().to_string();
    let mut out = Map::new();
    out.insert("marketId".into(), json!(market_id.to_string()));
    out.insert("exchangeId".into(), json!(id.clone()));
    out.insert("instrument".into(), json!(format!("{id}:YES")));
    out.insert("receivedAt".into(), json!(received_at));
    out.insert("requestStartedAt".into(), json!(request_started_at));
    out.insert("sourceAt".into(), json!(source_at.clone()));
    out.insert("sequence".into(), json!(seq.unwrap().to_string()));
    out.insert("versionKey".into(), json!(format!("{}:{}", seq.unwrap(), source_at)));
    out.insert("bids".into(), json!(bids));
    out.insert("asks".into(), json!(asks));
    if freshness_policy == "authoritative_receipt_v1" {
        out.insert("freshnessPolicy".into(), json!("authoritative_receipt_v1"));
        out.insert("sourceEventAgeMs".into(), json!(now - vat_ms.unwrap() as f64));
    }
    Ok(V::Object(out))
}

// live-paper/account.mjs views.
fn id_str(v: &V) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit()))
}
fn number(v: &V) -> bool {
    v.as_f64().is_some_and(|x| x.is_finite())
}
pub fn account_positions(response: &V) -> Result<V, String> {
    let positions = response.get("data").and_then(|d| d.get("positions")).and_then(V::as_array);
    let at = response.get("receivedAt").and_then(V::as_str);
    need(positions.is_some() && at.is_some_and(|s| timestamp_ok(s)), "Invalid account positions")?;
    let mut rows = Vec::new();
    for x in positions.unwrap() {
        if x.get("quantity").and_then(V::as_f64) == Some(0.0) || x.get("quantity").and_then(V::as_i64) == Some(0) {
            continue;
        }
        let qty = js_int(x.get("quantity").unwrap_or(&V::Null));
        need(
            id_str(x.get("exchangeId").unwrap_or(&V::Null))
                && id_str(x.get("marketId").unwrap_or(&V::Null))
                && qty.is_some()
                && ["avgCost", "marketValue", "costBasis", "unrealizedPnl", "currentPrice"]
                    .iter()
                    .all(|k| number(x.get(k).unwrap_or(&V::Null))),
            "Invalid account position",
        )?;
        let q = qty.unwrap();
        rows.push(json!({
            "instrument": format!("{}:{}", x.get("exchangeId").unwrap().as_str().unwrap(), if q < 0 { "NO" } else { "YES" }),
            "exchangeId": x.get("exchangeId").unwrap().clone(),
            "marketId": x.get("marketId").unwrap().clone(),
            "label": x.get("marketTitle").and_then(V::as_str).map(|s| V::String(s.chars().take(240).collect())).unwrap_or(V::Null),
            "side": if q < 0 { "no" } else { "yes" },
            "quantity": q.abs(),
            "avgCost": x.get("avgCost").unwrap().clone(),
            "cost": x.get("costBasis").unwrap().clone(),
            "valuationPrice": x.get("currentPrice").unwrap().clone(),
            "valuationValue": x.get("marketValue").unwrap().clone(),
            "unrealizedPnl": x.get("unrealizedPnl").unwrap().clone(),
        }));
    }
    Ok(json!({"at": at.unwrap(), "rows": rows}))
}
pub fn account_orders(rows: &V, at: &str, open_only: bool) -> Result<V, String> {
    need(rows.is_array() && timestamp_ok(at), "Invalid account orders")?;
    let mut out = Vec::new();
    for x in rows.as_array().unwrap() {
        let id = js_int(x.get("id").unwrap_or(&V::Null));
        let qty = x.get("quantity").and_then(V::as_f64);
        let open = x.get("open").and_then(V::as_bool);
        let limit = x.get("priceLimit").unwrap_or(&V::Null);
        need(
            id.is_some_and(|i| i > 0)
                && id_str(x.get("exchangeId").unwrap_or(&V::Null))
                && x.get("tournamentId").and_then(V::as_str) == Some(TID)
                && ["yes", "no"].contains(&x.get("side").and_then(V::as_str).unwrap_or(""))
                && ["buy", "sell"].contains(&x.get("action").and_then(V::as_str).unwrap_or(""))
                && qty.is_some_and(|q| q.is_finite() && q >= 0.0)
                && open.is_some()
                && (!open_only || open == Some(true)),
            "Invalid account order",
        )?;
        need(
            (limit.is_null() || number(limit))
                && x.get("createdAt").and_then(V::as_str).is_some_and(|s| timestamp_ok(s)),
            "Invalid account order",
        )?;
        out.push(json!({
            "id": id.unwrap(),
            "instrument": format!("{}:{}", x.get("exchangeId").unwrap().as_str().unwrap(), x.get("side").unwrap().as_str().unwrap().to_uppercase()),
            "exchangeId": x.get("exchangeId").unwrap().clone(),
            "side": x.get("side").unwrap().clone(),
            "action": x.get("action").unwrap().clone(),
            "quantity": qty.unwrap(),
            "limit": limit.clone(),
            "createdAt": x.get("createdAt").unwrap().clone(),
            "open": open.unwrap(),
        }));
    }
    Ok(json!({"at": at, "rows": out}))
}
pub fn account_fills(response: &V) -> Result<V, String> {
    let d = response.get("data").unwrap_or(&V::Null);
    let rows_v = d.get("data").and_then(V::as_array);
    let at = response.get("receivedAt").and_then(V::as_str);
    need(
        rows_v.is_some_and(|r| r.len() <= 100)
            && d.get("pagination").and_then(|p| p.get("hasMore")).and_then(V::as_bool).is_some()
            && d.get("coverage").and_then(|c| c.get("complete")).and_then(V::as_bool) == Some(true)
            && at.is_some_and(|s| timestamp_ok(s)),
        "Invalid account fills",
    )?;
    let mut rows = Vec::new();
    for x in rows_v.unwrap() {
        let id = js_int(x.get("id").unwrap_or(&V::Null));
        let order_id = x.get("orderId").unwrap_or(&V::Null);
        let qty = x.get("quantity").and_then(V::as_f64);
        let side = x.get("side").and_then(V::as_str).unwrap_or("");
        let price_v = x.get("price").unwrap_or(&V::Null);
        need(
            id.is_some_and(|i| i > 0)
                && (order_id.is_null() || js_int(order_id).is_some())
                && id_str(x.get("exchangeId").unwrap_or(&V::Null))
                && id_str(x.get("marketId").unwrap_or(&V::Null))
                && qty.is_some_and(|q| q.is_finite() && q != 0.0)
                && ["yes", "no"].contains(&side)
                && !(side == "no" && qty.unwrap() > 0.0)
                && !(side == "yes" && qty.unwrap() < 0.0)
                && (price_v.is_null() || number(price_v))
                && x.get("filledAt").and_then(V::as_str).is_some_and(|s| timestamp_ok(s)),
            "Invalid account fill",
        )?;
        rows.push(json!({
            "id": id.unwrap(),
            "orderId": order_id.clone(),
            "instrument": format!("{}:{}", x.get("exchangeId").unwrap().as_str().unwrap(), side.to_uppercase()),
            "exchangeId": x.get("exchangeId").unwrap().clone(),
            "marketId": x.get("marketId").unwrap().clone(),
            "side": side,
            "quantity": qty.unwrap().abs(),
            "price": price_v.clone(),
            "at": x.get("filledAt").unwrap().clone(),
        }));
    }
    Ok(json!({
        "at": at.unwrap(),
        "hasMore": d.get("pagination").and_then(|p| p.get("hasMore")).and_then(V::as_bool).unwrap(),
        "rows": rows,
    }))
}
pub fn account_pnl(response: &V) -> Result<V, String> {
    let d = response.get("data").unwrap_or(&V::Null);
    let at = response.get("receivedAt").and_then(V::as_str);
    need(
        d.get("period").and_then(V::as_str) == Some("day")
            && ["totalAccountValue", "totalHoldingsValue", "totalCostBasis", "unrealizedPnl"]
                .iter()
                .all(|k| number(d.get(k).unwrap_or(&V::Null)))
            && (d.get("periodPnl").unwrap_or(&V::Null).is_null() || number(d.get("periodPnl").unwrap()))
            && at.is_some_and(|s| timestamp_ok(s)),
        "Invalid account valuation",
    )?;
    Ok(json!({
        "at": at.unwrap(),
        "accountValue": d.get("totalAccountValue").unwrap().clone(),
        "holdingsValue": d.get("totalHoldingsValue").unwrap().clone(),
        "costBasis": d.get("totalCostBasis").unwrap().clone(),
        "unrealizedPnl": d.get("unrealizedPnl").unwrap().clone(),
        "periodPnl": d.get("periodPnl").unwrap_or(&V::Null).clone(),
        "period": "day",
    }))
}

// live-paper/market-price-publication.mjs.
fn exact_keys(v: &V, fields: &[&str]) -> bool {
    v.as_object().is_some_and(|m| {
        m.len() == fields.len() && m.keys().all(|k| fields.contains(&k.as_str()))
    })
}
pub fn validate_price_observation(snapshot: &V, universe: &V) -> Result<V, String> {
    let keys = [
        "tournamentId", "universeHash", "requestStartedAt", "receivedAt",
        "sourceTimestamp", "depthAvailable", "executable", "quotes", "missingIds",
    ];
    let quote_keys = [
        "exchangeId", "marketId", "option", "latestPrice", "bestBid", "bestAsk",
        "spread", "crossed",
    ];
    need(
        exact_keys(snapshot, &keys)
            && snapshot.get("universeHash") == universe.get("hash")
            && snapshot.get("tournamentId") == universe.get("tournamentId")
            && snapshot.get("executable") == Some(&V::Bool(false))
            && snapshot.get("depthAvailable") == Some(&V::Bool(false))
            && snapshot.get("sourceTimestamp").unwrap_or(&V::Null).is_null()
            && snapshot.get("quotes").is_some_and(V::is_array)
            && snapshot.get("missingIds").is_some_and(V::is_array),
        "MARKET_PRICE_PUBLICATION_NOT_NORMALIZED",
    )?;
    let started = snapshot.get("requestStartedAt").and_then(V::as_str).and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok());
    let ended = snapshot.get("receivedAt").and_then(V::as_str).and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok());
    need(
        started.is_some()
            && ended.is_some()
            && ended >= started
            && started.map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)) == snapshot.get("requestStartedAt").and_then(V::as_str).map(String::from)
            && ended.map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)) == snapshot.get("receivedAt").and_then(V::as_str).map(String::from),
        "MARKET_PRICE_PUBLICATION_CLOCK",
    )?;
    let members: std::collections::BTreeMap<String, &V> = universe
        .get("exchanges")
        .and_then(V::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.get("exchangeId").and_then(V::as_str).map(|id| (id.to_string(), x)))
                .collect()
        })
        .unwrap_or_default();
    let mut seen = std::collections::BTreeSet::new();
    for q in snapshot.get("quotes").and_then(V::as_array).unwrap() {
        let spec = q.get("exchangeId").and_then(V::as_str).and_then(|id| members.get(id));
        need(
            exact_keys(q, &quote_keys)
                && spec.is_some()
                && spec.unwrap().get("marketId") == q.get("marketId")
                && spec.unwrap().get("option") == q.get("option")
                && !seen.contains(q.get("exchangeId").and_then(V::as_str).unwrap_or("")),
            "MARKET_PRICE_PUBLICATION_IDENTITY",
        )?;
        for k in ["latestPrice", "bestBid", "bestAsk", "spread"] {
            let v = q.get(k).unwrap_or(&V::Null);
            let floor = if k == "spread" { -1.0 } else { 0.0 };
            need(
                v.is_null() || v.as_f64().is_some_and(|x| x.is_finite() && x <= 1.0 && x >= floor),
                "MARKET_PRICE_PUBLICATION_PRICE",
            )?;
        }
        let bid = q.get("bestBid").and_then(V::as_f64);
        let ask = q.get("bestAsk").and_then(V::as_f64);
        let crossed = bid.is_some() && ask.is_some() && bid >= ask;
        need(q.get("crossed").and_then(V::as_bool) == Some(crossed), "MARKET_PRICE_PUBLICATION_CROSSED")?;
        seen.insert(q.get("exchangeId").and_then(V::as_str).unwrap().to_string());
    }
    for id in snapshot.get("missingIds").and_then(V::as_array).unwrap() {
        let s = id.as_str().unwrap_or("");
        need(members.contains_key(s) && !seen.contains(s), "MARKET_PRICE_PUBLICATION_MISSING")?;
        seen.insert(s.to_string());
    }
    need(!seen.is_empty() && seen.len() <= 100, "MARKET_PRICE_PUBLICATION_SIZE")?;
    Ok(snapshot.clone())
}
