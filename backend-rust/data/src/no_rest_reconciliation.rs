// execution/no-rest-reconciliation.mjs — port of `inspectScopedRestNoAccount`
// and its private helper chain. verifyScopedRestNoBuy already lives in
// baal_execution::no_rest_reconciliation; this file carries the missing sibling.
use baal_execution::receipts::parse_number_lexemes;
use baal_service_runtime::fixed::{amount, instant, money};
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::collections::{BTreeMap, BTreeSet};

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    static NULL: V = V::Null;
    v.get(k).unwrap_or(&NULL)
}
fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("REST_NO_{c}")) }
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty()
            && s.bytes().all(|b| b.is_ascii_digit())
            && !s.starts_with('0')
            && s.parse::<i64>().is_ok()
    })
}
fn integer(v: &V, min: i64) -> bool {
    v.as_i64().is_some_and(|n| n >= min)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn amount_v(v: &V) -> Result<i128, String> {
    amount(v.as_str().unwrap_or(""))
}
fn slug(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|seg| !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}
fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

struct Read {
    data: V,
    numbers: BTreeMap<String, String>,
    start: i64,
    end: i64,
    source_body_hash: String,
    endpoint: String,
    request_started_at: V,
    received_at: V,
}
impl Clone for Read {
    fn clone(&self) -> Self {
        Read {
            data: self.data.clone(),
            numbers: self.numbers.clone(),
            start: self.start,
            end: self.end,
            source_body_hash: self.source_body_hash.clone(),
            endpoint: self.endpoint.clone(),
            request_started_at: self.request_started_at.clone(),
            received_at: self.received_at.clone(),
        }
    }
}

fn read(r: &V, method: &str, endpoint: &str, now: i64, fresh: bool) -> Result<Read, String> {
    need(
        get(r, "method").as_str() == Some(method)
            && get(r, "endpoint").as_str() == Some(endpoint)
            && get(r, "status") == &json!(200)
            && get(r, "bodyText").is_string(),
        "RECEIPT_SCOPE",
    )?;
    let start = instant_v(get(r, "requestStartedAt"))?;
    let end = instant_v(get(r, "receivedAt"))?;
    need(
        start <= end && end <= now && end - start <= 20_000 && (!fresh || now - end <= 20_000),
        "RECEIPT_TIME",
    )?;
    let source_body_hash = sha256(get(r, "bodyText").as_str().unwrap().as_bytes());
    if let Some(h) = r.get("sourceBodyHash") {
        need(h.as_str() == Some(source_body_hash.as_str()), "RECEIPT_HASH")?;
    }
    let body_text = get(r, "bodyText").as_str().unwrap();
    let (data, numbers) = parse_number_lexemes(body_text)?;
    Ok(Read {
        data,
        numbers,
        start,
        end,
        source_body_hash,
        endpoint: endpoint.to_string(),
        request_started_at: get(r, "requestStartedAt").clone(),
        received_at: get(r, "receivedAt").clone(),
    })
}

fn scope_check(scope: &V) -> Result<(), String> {
    need(
        get(scope, "tournamentId").as_str().is_some_and(|s| !s.is_empty())
            && get(scope, "tournamentSlug").as_str().is_some_and(slug),
        "SCOPE",
    )
}

struct CashRead {
    read: Read,
    displayed: i128,
    lower: i128,
    upper: i128,
}
impl Clone for CashRead {
    fn clone(&self) -> Self {
        CashRead { read: self.read.clone(), displayed: self.displayed, lower: self.lower, upper: self.upper }
    }
}

fn cash(r: &V, scope: &V, now: i64) -> Result<CashRead, String> {
    let v = read(
        r,
        "GET",
        &format!("/tournaments/{}", get(scope, "tournamentSlug").as_str().unwrap_or("")),
        now,
        true,
    )?;
    need(
        get(&v.data, "id") == get(scope, "tournamentId")
            && get(&v.data, "isPendingEnrolment") == &json!(false),
        "CASH_SCOPE",
    )?;
    let raw = v.numbers.get("[\"myBalance\"]");
    need(raw.is_some(), "CASH_NUMERIC_SOURCE")?;
    let displayed = amount(raw.unwrap()).map_err(|_| "REST_NO_CASH_DECIMAL".to_string())?;
    need(displayed >= 0 && displayed % 10_000 == 0, "CASH_NOT_CENT_ROUNDED")?;
    Ok(CashRead {
        lower: if displayed > 10_000 { displayed - 10_000 } else { 0 },
        upper: displayed + 10_000,
        displayed,
        read: v,
    })
}

struct Pages {
    rows: Vec<V>,
    sources: Vec<Read>,
    checkpoint: Option<i64>,
}

fn pages(
    receipts: &V,
    endpoint: &str,
    now: i64,
    mut check: Option<&mut dyn FnMut(&V) -> Result<(), String>>,
) -> Result<Pages, String> {
    need(
        receipts.as_array().is_some_and(|r| !r.is_empty() && r.len() <= 10),
        "PAGES_INCOMPLETE",
    )?;
    let receipts = receipts.as_array().unwrap();
    let mut cursor: Option<String> = None;
    let mut last_end = i64::MIN;
    let mut checkpoint: Option<i64> = None;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut rows = Vec::new();
    let mut sources = Vec::new();
    for (i, receipt) in receipts.iter().enumerate() {
        let page_endpoint = match &cursor {
            None => endpoint.to_string(),
            Some(c) => format!("{endpoint}&cursor={}", encode_uri_component(c)),
        };
        let r = read(receipt, "GET", &page_endpoint, now, true)?;
        let d = r.data.clone();
        need(
            r.start >= last_end && get(&d, "data").as_array().is_some_and(|a| a.len() <= 200),
            "PAGE_CLOCK_OR_DATA",
        )?;
        last_end = r.end;
        let p = get(&d, "pagination");
        need(
            js_truthy(Some(p))
                && get(p, "limit") == &json!(200)
                && get(p, "hasMore").is_boolean()
                && (p.get("nextCursor") == Some(&V::Null)
                    || get(p, "nextCursor").as_str().is_some_and(|s| !s.is_empty() && s.len() <= 1000)),
            "PAGE_PAGINATION",
        )?;
        let coverage = get(&d, "coverage");
        let projected = coverage.get("projectedThroughSequence");
        need(
            get(coverage, "complete") == &json!(true)
                && (projected == Some(&V::Null) || projected.is_some_and(|v| integer(v, 0))),
            "PAGE_COVERAGE",
        )?;
        let projected_value = projected.and_then(js_int);
        if checkpoint.is_some() && projected_value.is_some() {
            need(projected_value.unwrap() >= checkpoint.unwrap(), "PROJECTION_ROLLBACK")?;
        }
        if projected_value.is_some() {
            checkpoint = projected_value;
        }
        if let Some(cb) = check.as_deref_mut() {
            cb(&d)?;
        }
        rows.extend(get(&d, "data").as_array().unwrap().iter().cloned());
        sources.push(r);
        if i < receipts.len() - 1 {
            need(
                js_truthy(p.get("hasMore"))
                    && p.get("nextCursor") != Some(&V::Null)
                    && get(p, "nextCursor").as_str().is_some_and(|c| !seen.contains(c)),
                "PAGE_CURSOR",
            )?;
            seen.insert(get(p, "nextCursor").as_str().unwrap().to_string());
            cursor = Some(get(p, "nextCursor").as_str().unwrap().to_string());
        } else {
            need(
                get(p, "hasMore") == &json!(false) && p.get("nextCursor") == Some(&V::Null),
                "PAGES_INCOMPLETE",
            )?;
        }
    }
    Ok(Pages { rows, sources, checkpoint })
}

struct Snapshot {
    cash: CashRead,
    positions: BTreeMap<String, V>,
    history: BTreeMap<i64, V>,
    start: i64,
    end: i64,
    sources: Vec<V>,
}

fn snapshot(s: &V, scope: &V, now: i64) -> Result<Snapshot, String> {
    need(
        !s.is_null()
            && get(s, "openOrdersPages").is_array()
            && get(s, "scopedOrdersPages").is_array(),
        "ACCOUNT_RECEIPTS",
    )?;
    let c = cash(get(s, "cash"), scope, now)?;
    let p = read(
        get(s, "positions"),
        "GET",
        &format!("/tournaments/{}/portfolio/positions", get(scope, "tournamentSlug").as_str().unwrap_or("")),
        now,
        true,
    )?;
    need(
        get(&p.data, "positions").as_array().is_some_and(|a| a.len() <= 10_000)
            && p.data.get("pagination").is_none()
            && js_truthy(p.data.get("summary"))
            && ["totalMarketValue", "totalCostBasis", "totalUnrealizedPnl"].iter()
                .all(|k| get(get(&p.data, "summary"), k).as_f64().is_some_and(|n| n.is_finite())),
        "POSITIONS_INCOMPLETE",
    )?;
    let mut positions: BTreeMap<String, V> = BTreeMap::new();
    for row in get(&p.data, "positions").as_array().unwrap().iter() {
        need(
            id(get(row, "marketId")) && id(get(row, "exchangeId"))
                && js_int(get(row, "quantity")).is_some()
                && get(row, "settled").is_boolean()
                && !positions.contains_key(get(row, "exchangeId").as_str().unwrap_or("")),
            "POSITION_SCHEMA",
        )?;
        positions.insert(
            get(row, "exchangeId").as_str().unwrap().to_string(),
            json!({
                "marketId": get(row, "marketId"),
                "quantity": get(row, "quantity"),
                "settled": get(row, "settled"),
            }),
        );
    }
    let o = pages(
        get(s, "openOrdersPages"),
        &format!("/orders?status=open&limit=200&tournamentId={}", get(scope, "tournamentId").as_str().unwrap_or("")),
        now,
        None,
    )?;
    need(o.rows.is_empty(), "OPEN_ACCOUNT_ORDER")?;
    let h = pages(
        get(s, "scopedOrdersPages"),
        &format!(
            "/orders?status=all&exchangeId={}&limit=200&tournamentId={}",
            get(scope, "exchangeId").as_str().unwrap_or(""),
            get(scope, "tournamentId").as_str().unwrap_or("")
        ),
        now,
        None,
    )?;
    let mut history: BTreeMap<i64, V> = BTreeMap::new();
    for row in &h.rows {
        need(
            integer(get(row, "id"), 1)
                && !history.contains_key(&js_int(get(row, "id")).unwrap())
                && get(row, "exchangeId") == get(scope, "exchangeId")
                && get(row, "tournamentId") == get(scope, "tournamentId")
                && matches!(get(row, "side").as_str(), Some("yes" | "no"))
                && matches!(get(row, "action").as_str(), Some("buy" | "sell"))
                && integer(get(row, "quantity"), 1)
                && get(row, "open").is_boolean()
                && instant_v(get(row, "createdAt"))? <= h.sources.last().unwrap().end
                && (row.get("expirationDate") == Some(&V::Null) || {
                    instant_v(get(row, "expirationDate"))?;
                    true
                }),
            "SCOPED_ORDER_SCHEMA",
        )?;
        need(
            get(row, "open") != &json!(true)
                || (row.get("expirationDate") != Some(&V::Null)
                    && instant_v(get(row, "expirationDate"))? <= h.sources[0].start),
            "SCOPED_ORDER_OPEN_CONTRADICTION",
        )?;
        history.insert(js_int(get(row, "id")).unwrap(), row.clone());
    }
    let mut all: Vec<Read> = vec![c.read.clone(), p.clone()];
    all.extend(o.sources.iter().cloned());
    all.extend(h.sources.iter().cloned());
    let start = all.iter().map(|v| v.start).min().unwrap_or(0);
    let end = all.iter().map(|v| v.end).max().unwrap_or(0);
    need(now - start <= 20_000, "ACCOUNT_STALE")?;
    let sources: Vec<V> = all
        .iter()
        .map(|v| {
            json!({
                "endpoint": v.endpoint,
                "requestStartedAt": v.request_started_at,
                "receivedAt": v.received_at,
                "sourceBodyHash": v.source_body_hash,
            })
        })
        .collect();
    Ok(Snapshot { cash: c, positions, history, start, end, sources })
}

/// inspectScopedRestNoAccount({account,binding,readAt,requiredFunding,reservedElsewhere})
/// Shared preflight parser for the bounded paired lifecycle. It exposes curated
/// account facts, not a write permit, and uses the same complete receipt checks.
pub fn inspect_scoped_rest_no_account(input: &V) -> Result<V, String> {
    let binding = get(input, "binding");
    scope_check(binding)?;
    need(id(get(binding, "exchangeId")), "SCOPE")?;
    let default_zero = json!("0.000000");
    let required_funding = input.get("requiredFunding").unwrap_or(&default_zero);
    let reserved_elsewhere = input.get("reservedElsewhere").unwrap_or(&default_zero);
    let a = snapshot(get(input, "account"), binding, instant_v(get(input, "readAt"))?)?;
    let required = amount_v(required_funding)? + amount_v(reserved_elsewhere)?;
    need(amount_v(required_funding)? >= 0 && amount_v(reserved_elsewhere)? >= 0, "FUNDING_NONNEGATIVE")?;
    let positions: Vec<V> = a
        .positions
        .iter()
        .map(|(exchange_id, v)| {
            let mut o = Map::new();
            o.insert("exchangeId".into(), json!(exchange_id));
            for (k, x) in v.as_object().unwrap() { o.insert(k.clone(), x.clone()); }
            V::Object(o)
        })
        .collect();
    let scoped_orders: Vec<V> = a.history.values().cloned().collect();
    Ok(json!({
        "tournamentId": binding["tournamentId"],
        "positions": positions,
        "scopedOrders": scoped_orders,
        "startedAt": baal_service_runtime::contract::ms_to_iso(a.start as f64),
        "completedAt": baal_service_runtime::contract::ms_to_iso(a.end as f64),
        "cashLower": money(a.cash.lower),
        "cashUpper": money(a.cash.upper),
        "funded": a.cash.lower >= required,
        "sourceReceipts": a.sources,
        "executionAuthority": false,
    }))
}
