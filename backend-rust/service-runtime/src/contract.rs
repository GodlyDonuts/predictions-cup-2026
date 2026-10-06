use serde_json::{json, Map, Value as V};

// feed/contract.mjs: supported API v1 market-feed subset. No transport/auth.
pub const CAPTURE_SCHEMA_VERSION: i64 = 1;
pub const DELIVERY_MODEL: &str = "best-effort-authoritative-resync";

#[derive(Debug)]
pub struct ContractError(pub String);
impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ContractError {}
pub fn require_that(value: bool, code: &str) -> Result<(), ContractError> {
    if value { Ok(()) } else { Err(ContractError(code.to_string())) }
}
fn fail<T>(code: &str) -> Result<T, ContractError> {
    Err(ContractError(code.to_string()))
}

// timestamp(): strict ISO with optional fractional seconds and offset; output
// normalizes to nanosecond-precise UTC spelling.
pub fn timestamp(v: &V) -> Result<String, ContractError> {
    let s = v.as_str().ok_or_else(|| ContractError("INVALID_SCHEMA".into()))?;
    timestamp_str(s)
}
pub fn timestamp_str(s: &str) -> Result<String, ContractError> {
    // Shape: YYYY-MM-DDThh:mm:ss[.fraction](Z|±hh:mm)
    let b = s.as_bytes();
    let valid_shape = s.len() >= 20
        && b.get(4) == Some(&b'-')
        && b.get(7) == Some(&b'-')
        && b.get(10) == Some(&b'T')
        && b.get(13) == Some(&b':')
        && b.get(16) == Some(&b':');
    require_that(valid_shape, "INVALID_SCHEMA")?;
    let zone_start = s[19..]
        .find(|c| c == 'Z' || c == '+' || c == '-')
        .map(|i| i + 19)
        .unwrap_or(s.len());
    let has_dot = s.as_bytes().get(19) == Some(&b'.');
    let frac_part = if has_dot { &s[20..zone_start] } else { "" };
    require_that(
        !has_dot || (!frac_part.is_empty() && frac_part.len() <= 9 && frac_part.bytes().all(|b| b.is_ascii_digit())),
        "INVALID_SCHEMA",
    )?;
    let zone = &s[zone_start..];
    let zone_ok = zone == "Z"
        || (zone.len() == 6
            && (zone.starts_with('+') || zone.starts_with('-'))
            && zone.as_bytes()[3] == b':'
            && zone[1..3].bytes().all(|b| b.is_ascii_digit())
            && zone[4..6].bytes().all(|b| b.is_ascii_digit()));
    require_that(zone_ok, "INVALID_SCHEMA")?;
    // JS Date.parse(...fraction.padEnd(3,'0').slice(0,3)...): millisecond only.
    let frac3 = format!("{frac_part:0<3}");
    let ms = chrono::DateTime::parse_from_rfc3339(
        &format!("{}.{}{}", &s[..19], &frac3[..3], zone),
    )
    .map_err(|_| ContractError("INVALID_SCHEMA".into()))?
    .timestamp_millis();
    let iso = chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default();
    // JS: iso.slice(0,19) + '.' + fraction.padEnd(9,'0') + 'Z'
    Ok(format!("{}.{:0<9}Z", &iso[..19], frac_part))
}
// instantNs: ms*1e6 + last 6 digits of the 9-digit fraction.
pub fn instant_ns(v: &str) -> Result<i128, ContractError> {
    let iso = timestamp_str(v)?;
    let ms = chrono::DateTime::parse_from_rfc3339(&iso)
        .map_err(|_| ContractError("INVALID_SCHEMA".into()))?
        .timestamp_millis() as i128;
    // iso is YYYY-MM-DDThh:mm:ss.sss<frac6>Z — JS uses iso.slice(23,29).
    let tail = iso[23..29].parse::<i128>().unwrap_or(0);
    Ok(ms * 1_000_000 + tail)
}
pub fn numeric_id(v: &V) -> Result<String, ContractError> {
    match v {
        V::String(s) => {
            require_that(
                !s.is_empty() && !s.starts_with('0') && s.len() <= 16 && s.bytes().all(|b| b.is_ascii_digit()),
                "INVALID_SCHEMA",
            )?;
            Ok(s.clone())
        }
        V::Number(_) => {
            let n = crate::json_text::js_int(v).ok_or_else(|| ContractError("INVALID_SCHEMA".into()))?;
            require_that(n >= 1, "INVALID_SCHEMA")?;
            Ok(n.to_string())
        }
        _ => fail("INVALID_SCHEMA"),
    }
}
fn uuid(v: &V) -> Result<String, ContractError> {
    let s = v.as_str().ok_or_else(|| ContractError("INVALID_SCHEMA".into()))?;
    let ok = s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) { b == b'-' } else { b.is_ascii_hexdigit() }
        });
    require_that(ok, "INVALID_SCHEMA")?;
    Ok(s.to_lowercase())
}
fn integer(v: &V, min: i64) -> Result<i64, ContractError> {
    let n = crate::json_text::js_int(v).ok_or_else(|| ContractError("INVALID_SCHEMA".into()))?;
    require_that(n >= min, "INVALID_SCHEMA")?;
    Ok(n)
}
fn object(v: &V) -> Result<&V, ContractError> {
    require_that(v.is_object(), "INVALID_SCHEMA")?;
    Ok(v)
}
fn array(v: &V) -> Result<Vec<V>, ContractError> {
    match v.as_array() {
        Some(a) if a.len() <= 100_000 => Ok(a.clone()),
        _ => fail("INVALID_SCHEMA"),
    }
}
#[derive(Clone)]
pub struct Scope {
    pub tournament_id: String,
    pub market_id: String,
    pub exchange_ids: Vec<String>,
    pub topic: String,
}
pub fn config(v: &V) -> Result<Scope, ContractError> {
    object(v)?;
    let tournament_id = uuid(v.get("tournamentId").unwrap_or(&V::Null))?;
    let market_id = numeric_id(v.get("marketId").unwrap_or(&V::Null))?;
    let exchange_ids: Result<Vec<String>, _> =
        array(v.get("exchangeIds").unwrap_or(&V::Null))?.iter().map(numeric_id).collect();
    let exchange_ids = exchange_ids?;
    require_that(
        !exchange_ids.is_empty()
            && exchange_ids.iter().collect::<std::collections::BTreeSet<_>>().len() == exchange_ids.len(),
        "INVALID_SCHEMA",
    )?;
    Ok(Scope {
        topic: format!("tournament:{tournament_id}:market:{market_id}"),
        tournament_id,
        market_id,
        exchange_ids,
    })
}
fn version(v: &V, nullable: bool) -> Result<Option<V>, ContractError> {
    if v.is_null() && nullable {
        return Ok(None);
    }
    object(v)?;
    let sequence = integer(v.get("sequence").unwrap_or(&V::Null), 0)?;
    let at = timestamp_str(v.get("at").and_then(V::as_str).unwrap_or(""))?;
    Ok(Some(json!({"sequence": sequence, "at": at})))
}
pub fn compare_version(a: &V, b: &V) -> i32 {
    let (an, bn) = (a.is_null(), b.is_null());
    if an && bn {
        return 0;
    }
    if an {
        return -1;
    }
    if bn {
        return 1;
    }
    let sa = a.get("sequence").and_then(V::as_i64).unwrap_or(0);
    let sb = b.get("sequence").and_then(V::as_i64).unwrap_or(0);
    match sa.cmp(&sb) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Equal => {
            let ta = instant_ns(a.get("at").and_then(V::as_str).unwrap_or("")).unwrap_or(0);
            let tb = instant_ns(b.get("at").and_then(V::as_str).unwrap_or("")).unwrap_or(0);
            ta.cmp(&tb) as i32
        }
    }
}
fn level(v: &V) -> Result<V, ContractError> {
    object(v)?;
    let price = v.get("price").and_then(V::as_f64);
    let qty = v.get("quantity").and_then(V::as_f64);
    require_that(
        price.is_some_and(|p| p.is_finite() && (0.0..=1.0).contains(&p)),
        "INVALID_SCHEMA",
    )?;
    require_that(
        qty.is_some_and(|q| q.is_finite() && q > 0.0 && q <= 9_007_199_254_740_991.0),
        "INVALID_SCHEMA",
    )?;
    Ok(json!({"price": price.unwrap(), "quantity": qty.unwrap()}))
}
fn levels(v: &V, direction: i64) -> Result<Vec<V>, ContractError> {
    let rows = array(v)?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(level(r)?);
    }
    for i in 1..out.len() {
        let a = out[i - 1].get("price").and_then(V::as_f64).unwrap();
        let b = out[i].get("price").and_then(V::as_f64).unwrap();
        require_that((direction as f64 * (b - a)) > 0.0, "UNSORTED_LEVELS")?;
    }
    Ok(out)
}
fn exchange(v: &V, scope: &Scope) -> Result<String, ContractError> {
    let id = numeric_id(v)?;
    require_that(scope.exchange_ids.contains(&id), "UNEXPECTED_EXCHANGE")?;
    Ok(id)
}
fn scope_check(v: &V, scope: &Scope) -> Result<(), ContractError> {
    require_that(
        numeric_id(v.get("marketId").unwrap_or(&V::Null)).ok().as_deref() == Some(scope.market_id.as_str())
            && uuid(v.get("tournamentId").unwrap_or(&V::Null)).ok().as_deref() == Some(scope.tournament_id.as_str()),
        "CONTEXT_MISMATCH",
    )
}
pub fn book(v: &V, scope: &Scope, rest: bool) -> Result<V, ContractError> {
    object(v)?;
    let exchange_id = exchange(v.get("exchangeId").unwrap_or(&V::Null), scope)?;
    if !rest {
        require_that(v.get("nextExpiryAt").is_some(), "INVALID_SCHEMA")?;
    }
    let as_of = version(v.get("asOf").unwrap_or(&V::Null), rest)?;
    let next_expiry = v.get("nextExpiryAt").unwrap_or(&V::Null);
    let next = if next_expiry.is_null() { None } else { Some(timestamp_str(next_expiry.as_str().unwrap_or(""))?) };
    Ok(json!({
        "exchangeId": exchange_id,
        "asOf": as_of.unwrap_or(V::Null),
        "nextExpiryAt": next,
        "expiryKnown": !rest && v.get("nextExpiryAt").is_some(),
        "bids": levels(v.get("bids").unwrap_or(&V::Null), -1)?,
        "asks": levels(v.get("asks").unwrap_or(&V::Null), 1)?,
    }))
}
fn trade(v: &V, scope: &Scope) -> Result<V, ContractError> {
    object(v)?;
    scope_check(v, scope)?;
    let id = v.get("id").unwrap_or(&V::Null);
    require_that(
        id.is_null() || id.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')),
        "INVALID_SCHEMA",
    )?;
    let seq = v.get("sequence").unwrap_or(&V::Null);
    let sequence = if seq.is_null() { None } else { Some(integer(seq, 0)?) };
    require_that(id.is_null() == sequence.is_none(), "UNORDERED_TRADE")?;
    let l = level(v)?;
    Ok(json!({
        "id": id.clone(),
        "sequence": sequence,
        "exchangeId": exchange(v.get("exchangeId").unwrap_or(&V::Null), scope)?,
        "marketId": scope.market_id,
        "tournamentId": scope.tournament_id,
        "price": l.get("price").unwrap().clone(),
        "quantity": l.get("quantity").unwrap().clone(),
        "executedAt": timestamp_str(v.get("executedAt").and_then(V::as_str).unwrap_or(""))?,
    }))
}
fn dirty(v: &V, scope: &Scope) -> Result<V, ContractError> {
    object(v)?;
    scope_check(v, scope)?;
    Ok(json!({
        "exchangeId": exchange(v.get("exchangeId").unwrap_or(&V::Null), scope)?,
        "marketId": scope.market_id,
        "tournamentId": scope.tournament_id,
        "at": timestamp_str(v.get("at").and_then(V::as_str).unwrap_or(""))?,
    }))
}
fn settled(v: &V, scope: &Scope) -> Result<V, ContractError> {
    object(v)?;
    scope_check(v, scope)?;
    let sw = v.get("settledWith").and_then(V::as_str);
    require_that(sw.is_some_and(|s| s.len() <= 256), "INVALID_SCHEMA")?;
    Ok(json!({
        "marketId": scope.market_id,
        "tournamentId": scope.tournament_id,
        "refund": sw == Some("REFUND"),
        "at": timestamp_str(v.get("at").and_then(V::as_str).unwrap_or(""))?,
    }))
}
pub fn parse_event(event: &str, payload: &V, scope: &Scope) -> Result<V, ContractError> {
    object(payload)?;
    match event {
        "book_dirty" => Ok(json!({"event": event, "payload": dirty(payload, scope)?})),
        "market_settled" => Ok(json!({"event": event, "payload": settled(payload, scope)?})),
        "market_batch" => {
            let d = payload.get("delivery").unwrap_or(&V::Null);
            object(d)?;
            require_that(
                d.get("model").and_then(V::as_str) == Some(DELIVERY_MODEL),
                "UNKNOWN_DELIVERY_MODEL",
            )?;
            let revision = integer(d.get("revision").unwrap_or(&V::Null), 1)?;
            let previous = integer(d.get("previousRevision").unwrap_or(&V::Null), 0)?;
            require_that(revision > previous, "INVALID_REVISION")?;
            let from = d.get("sourceSequenceFrom").unwrap_or(&V::Null);
            let through = d.get("sourceSequenceThrough").unwrap_or(&V::Null);
            let (f, t) = (
                if from.is_null() { None } else { Some(integer(from, 0)?) },
                if through.is_null() { None } else { Some(integer(through, 0)?) },
            );
            require_that(f.is_none() == t.is_none(), "INVALID_SCHEMA")?;
            require_that(f.is_none() || f <= t, "INVALID_SCHEMA")?;
            require_that(
                d.get("correlationId").and_then(V::as_str).is_some_and(|s| s.len() <= 256),
                "INVALID_SCHEMA",
            )?;
            require_that(
                payload.get("resyncRequired").is_none_or(|r| r.is_boolean()),
                "INVALID_SCHEMA",
            )?;
            let compact = payload.get("resyncRequired").and_then(V::as_bool) == Some(true);
            let books = array(payload.get("books").unwrap_or(&V::Array(vec![])))?
                .iter()
                .map(|v| book(v, scope, false))
                .collect::<Result<Vec<_>, _>>()?;
            require_that(
                books
                    .iter()
                    .map(|b| b.get("exchangeId").and_then(V::as_str).unwrap().to_string())
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    == books.len(),
                "DUPLICATE_BOOK",
            )?;
            let take = |key: &str| -> Vec<V> {
                if compact {
                    payload.get(key).and_then(V::as_array).cloned().unwrap_or_default()
                } else {
                    array(payload.get(key).unwrap_or(&V::Null)).unwrap_or_default()
                }
            };
            let trades = take("trades")
                .iter()
                .map(|v| trade(v, scope))
                .collect::<Result<Vec<_>, _>>()?;
            let book_dirty = take("bookDirty")
                .iter()
                .map(|v| dirty(v, scope))
                .collect::<Result<Vec<_>, _>>()?;
            let market_settled = take("marketSettled")
                .iter()
                .map(|v| settled(v, scope))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({
                "event": event,
                "payload": {
                    "delivery": {
                        "model": DELIVERY_MODEL,
                        "revision": revision,
                        "previousRevision": previous,
                        "sourceSequenceFrom": f,
                        "sourceSequenceThrough": t,
                    },
                    "resyncRequired": compact,
                    "books": books,
                    "trades": trades,
                    "bookDirty": book_dirty,
                    "marketSettled": market_settled,
                },
            }))
        }
        _ => fail("UNSUPPORTED_EVENT"),
    }
}
pub fn parse_snapshot(raw: &V, scope: &Scope) -> Result<Vec<V>, ContractError> {
    object(raw)?;
    let contexts = array(raw.get("contexts").unwrap_or(&V::Null))?;
    let selected: Vec<&V> = contexts
        .iter()
        .filter(|c| {
            c.get("type").and_then(V::as_str) == Some("tournament")
                && c.get("tournament").and_then(|t| t.get("id")).and_then(V::as_str) == Some(scope.tournament_id.as_str())
        })
        .collect();
    require_that(selected.len() == 1, "CONTEXT_MISMATCH")?;
    let exchanges = array(
        selected[0]
            .get("orderbook")
            .and_then(|o| object(o).ok())
            .and_then(|o| o.get("exchanges"))
            .unwrap_or(&V::Null),
    )?;
    let result: Vec<V> = exchanges.iter().map(|v| book(v, scope, true)).collect::<Result<_, _>>()?;
    require_that(
        result.len() == scope.exchange_ids.len()
            && result
                .iter()
                .map(|b| b.get("exchangeId").and_then(V::as_str).unwrap_or(""))
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == result.len(),
        "INCOMPLETE_SNAPSHOT",
    )?;
    Ok(result)
}
pub fn snapshot_path(scope: &Scope) -> String {
    format!(
        "/markets/{}/orderbook?tournamentId={}&depth=200",
        scope.market_id, scope.tournament_id
    )
}
