// execution/no-rest-sell-reconciliation-v2.mjs — standalone, read-only evidence
// reduction. It does not weaken the older exact global-cash verifier or change
// the realtime YES-denominated fill convention. REST NO prices are SIDE
// RELATIVE in the official specification observed 2026-10-06:
// https://sig.thesuper.market/api/v1/docs (Order/Fill/order fills).
use baal_execution::core::{canonical_payload, payload_hash};
use baal_execution::gates::normalize_official_fill_tick;
use baal_execution::receipts::parse_number_lexemes;
use baal_service_runtime::fixed::{amount, instant, money, price};
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::collections::{BTreeMap, BTreeSet};

/// REST_NO_SELL_RECONCILIATION_CONTRACT_V1
pub fn rest_no_sell_reconciliation_contract_v2() -> V {
    json!({
        "schemaVersion": 1,
        "mode": "scoped_rest_no_owned_exit_with_rounded_cash_neutral_all_v2",
        "quantity": 1,
        "fee": "0.000000",
        "cashRoundingRadius": "0.010000",
        "maxAccountAgeMs": 20000,
        "maxReceiptAgeMs": 20000,
        "maxOrderLifetimeMs": 30000,
        "maxPages": 10,
        "maxServerClockSkewMs": 2000,
        "priceBasis": "side_relative_rest_no",
        "documentationUrl": "https://sig.thesuper.market/api/v1/docs",
        "primarySchemaObservedAt": "2026-10-06T13:55:54.072Z",
        "fillQuantityBasis": "outcome_signed_no_negative_even_when_action_sell",
        "positionDeltaBasis": "covered_no_sell_increases_signed_position_toward_zero",
        "nonNullAllSupported": true,
        "executionAuthority": false,
    })
}

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("REST_NO_{c}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n > 0 && n <= 9_007_199_254_740_991)
    })
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn integer(v: &V, min: i64) -> bool {
    js_int(v).is_some_and(|n| n >= min)
}
fn tick(v: &V) -> Result<i128, String> {
    let t = normalize_official_fill_tick(v)?;
    amount(get(&t, "price").as_str().unwrap_or(""))
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

fn exact_integer(r: &Read, path: &V, value: &V) -> Result<(), String> {
    let lexeme = r.numbers.get(&js_stringify(path));
    let ok = lexeme.is_some_and(|l| {
        l.len() <= 128
            && {
                // /^-?(0|[1-9]\d*)(\.0+)?$/
                let (neg, rest) = l.strip_prefix('-').map(|s| (true, s)).unwrap_or((false, l.as_str()));
                let (ip, frac) = rest.split_once('.').map(|(a, b)| (a, Some(b))).unwrap_or((rest, None));
                !ip.is_empty()
                    && ip.bytes().all(|b| b.is_ascii_digit())
                    && (ip == "0" || !ip.starts_with('0'))
                    && frac.map(|f| !f.is_empty() && f.bytes().all(|b| b == b'0')).unwrap_or(true)
                    && !(neg && ip.is_empty())
            }
    });
    need(
        ok && integer(value, i64::MIN)
            && lexeme.unwrap().split('.').next().unwrap_or("").parse::<i128>().map(|n| n == value.as_i64().unwrap_or(i64::MIN) as i128).unwrap_or(false),
        "EXACT_INTEGER_SOURCE",
    )
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

/// roundedTournamentCashInterval(receipt,{tournamentId,tournamentSlug,readAt,requiredFunding,reservedElsewhere})
pub fn rounded_tournament_cash_interval(receipt: &V, options: &V) -> Result<V, String> {
    let scope = json!({
        "tournamentId": get(options, "tournamentId"),
        "tournamentSlug": get(options, "tournamentSlug"),
    });
    scope_check(&scope)?;
    let c = cash(receipt, &scope, instant_v(get(options, "readAt"))?)?;
    let default_zero = json!("0.000000");
    let required_funding = options.get("requiredFunding").unwrap_or(&default_zero);
    let reserved_elsewhere = options.get("reservedElsewhere").unwrap_or(&default_zero);
    let needed = amount_v(required_funding)? + amount_v(reserved_elsewhere)?;
    need(amount_v(required_funding)? >= 0 && amount_v(reserved_elsewhere)? >= 0, "FUNDING_NONNEGATIVE")?;
    Ok(json!({
        "basis": "rounded_tournament_cash_interval",
        "displayed": money(c.displayed),
        "lower": money(c.lower),
        "upper": money(c.upper),
        "roundingRadius": "0.010000",
        "requiredFunding": required_funding,
        "reservedElsewhere": reserved_elsewhere,
        "conservativelyFunded": c.lower >= needed,
        "sourceBodyHash": c.read.source_body_hash,
        "receivedAt": c.read.received_at,
        "exactGrossCash": false,
        "executionAuthority": false,
    }))
}

struct Pages {
    rows: Vec<V>,
    sources: Vec<Read>,
    #[allow(dead_code)]
    checkpoint: Option<i64>,
}

fn pages(
    receipts: &V,
    endpoint: &str,
    now: i64,
    mut check: Option<&mut dyn FnMut(&V, &Read) -> Result<(), String>>,
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
            cb(&d, &r)?;
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
    for (i, row) in get(&p.data, "positions").as_array().unwrap().iter().enumerate() {
        need(
            id(get(row, "marketId")) && id(get(row, "exchangeId"))
                && js_int(get(row, "quantity")).is_some()
                && get(row, "settled").is_boolean()
                && !positions.contains_key(get(row, "exchangeId").as_str().unwrap_or("")),
            "POSITION_SCHEMA",
        )?;
        exact_integer(&p, &json!(["positions", i, "quantity"]), get(row, "quantity"))?;
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

fn same_external(before: &BTreeMap<String, V>, after: &BTreeMap<String, V>, exchange_id: &str) -> Result<(), String> {
    for k in before.keys().chain(after.keys()).cloned().collect::<BTreeSet<_>>() {
        if k != exchange_id {
            let zero = |v: Option<&V>| -> V {
                match v {
                    Some(x) if js_truthy(x.get("quantity")) => x.clone(),
                    _ => V::Null,
                }
            };
            need(
                canonical_payload(&zero(before.get(&k)))? == canonical_payload(&zero(after.get(&k)))?,
                "EXTERNAL_POSITION_CHANGED",
            )?;
        }
    }
    Ok(())
}

fn canon_or_missing(v: Option<&V>) -> Result<String, String> {
    match v {
        Some(x) => canonical_payload(x),
        None => Err("INVALID_JSON_VALUE".into()),
    }
}
fn canon_or_null(v: Option<&V>) -> Result<String, String> {
    canonical_payload(&v.cloned().unwrap_or(V::Null))
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

/// verifyScopedRestNoSell({binding,before,ack,order,fillPages,after,globalRisk,accountActivity=null,readAt,originalBuyProof,originalAfter})
///
/// All inputs are supplied by the existing serialized collector. Raw receipt
/// bodies are parsed locally but never returned, stored, logged or transmitted.
/// Unknown GLOBAL economic activity is retained as unknown. A terminal, scoped
/// one-share lot can still have an exact basis; these are different assertions.
pub fn verify_scoped_rest_no_sell_v2(input: &V) -> Result<V, String> {
    let b = get(input, "binding");
    let now = instant_v(get(input, "readAt"))?;
    scope_check(b)?;
    need(
        id(get(b, "orderId")) && id(get(b, "marketId")) && id(get(b, "exchangeId"))
            && get(b, "ownerId").is_string() && get(b, "runId").is_string()
            && get(b, "intentId").is_string() && get(b, "idempotencyKey").is_string()
            && get(b, "side") == &json!("no") && get(b, "action") == &json!("sell")
            && get(b, "quantity") == &json!(1),
        "BINDING",
    )?;
    let limit = price(get(b, "limit").as_str().unwrap_or(""))?;
    let created = instant_v(get(b, "createdAt"))?;
    let expires = instant_v(get(b, "expirationDate"))?;
    need(expires > created && expires - created <= 30_000 && created <= now, "BINDING_LIFETIME")?;
    let global_risk = get(input, "globalRisk");
    need(
        get(global_risk, "tournamentId") == get(b, "tournamentId")
            && get(global_risk, "ownerId") == get(b, "ownerId")
            && get(global_risk, "runId") == get(b, "runId")
            && get(global_risk, "soleCollector") == &json!(true)
            && get(global_risk, "complete") == &json!(true)
            && get(global_risk, "stopped") == &json!(false)
            && hex(get(global_risk, "sourceHash"))
            && get(global_risk, "unresolvedIntentIds").as_array().is_some_and(|a| {
                a.len() <= 1 && a.iter().all(|x| x == get(b, "intentId"))
            })
            && instant_v(get(global_risk, "at"))? <= now
            && now - instant_v(get(global_risk, "at"))? <= 20_000,
        "GLOBAL_UNKNOWN_OR_OWNER",
    )?;

    let pre = snapshot(get(input, "before"), b, created)?;
    let post = snapshot(get(input, "after"), b, now)?;
    need(pre.end <= created && post.start >= created, "ACCOUNT_CAUSALITY")?;
    let proof = get(input, "originalBuyProof");
    let verification_hash = proof.get("verificationHash").cloned().unwrap_or(V::Null);
    let mut original_proof_body = proof.clone();
    if let Some(m) = original_proof_body.as_object_mut() { m.shift_remove("verificationHash"); }
    need(
        hex(&verification_hash)
            && payload_hash(&original_proof_body).ok() == verification_hash.as_str().map(String::from)
            && matches!(
                get(proof, "mode").as_str(),
                Some("scoped_rest_no_lot_with_rounded_cash_v1") | Some("scoped_rest_no_lot_with_rounded_cash_neutral_all_v2")
            )
            && get(proof, "scopedLotVerified") == &json!(true)
            && get(proof, "filledQuantity") == &json!(1)
            && get(proof, "signedPositionDelta") == &json!(-1)
            && get(proof, "ownerId") == get(b, "ownerId")
            && get(proof, "runId") == get(b, "runId")
            && get(proof, "tournamentId") == get(b, "tournamentId")
            && get(proof, "marketId") == get(b, "marketId")
            && get(proof, "exchangeId") == get(b, "exchangeId")
            && get(proof, "executionAuthority") == &json!(false),
        "ORIGINAL_BUY_PROVENANCE",
    )?;
    let lot = get(proof, "lot");
    need(
        get(lot, "side") == &json!("no")
            && get(lot, "quantity") == &json!(1)
            && get(lot, "lotId").as_str().is_some_and(|s| !s.is_empty())
            && get(lot, "ownerId") == get(b, "ownerId")
            && get(lot, "runId") == get(b, "runId")
            && get(lot, "orderId") == get(proof, "orderId")
            && get(lot, "intentId") == get(proof, "intentId")
            && get(lot, "entryPrice") == get(proof, "scopedPrincipalCost")
            && get(proof, "fills").as_array().is_some_and(|f| {
                f.len() == 1
                    && get(&f[0], "price") == get(lot, "entryPrice")
                    && get(&f[0], "side") == &json!("no")
                    && get(&f[0], "signedQuantity") == &json!(-1)
                    && get(&f[0], "quantity") == &json!(1)
                    && get(&f[0], "priceBasis") == &json!("side_relative")
                    && get(&f[0], "source") == &json!("rest_order_fills")
                    && get(&f[0], "filledAt") == get(lot, "openedAt")
            }),
        "ORIGINAL_BUY_LOT",
    )?;
    price(get(lot, "entryPrice").as_str().unwrap_or(""))?;
    let original = snapshot(get(input, "originalAfter"), b, instant_v(get(proof, "readAt"))?)?;
    let orig_pos = original.positions.get(get(b, "exchangeId").as_str().unwrap_or(""));
    need(
        original.end <= pre.start
            && orig_pos.is_some_and(|p| {
                get(p, "marketId") == get(b, "marketId")
                    && get(p, "quantity") == &json!(-1)
                    && get(p, "settled") == &json!(false)
            })
            && get(proof, "sourceReceipts").get("after").is_some_and(|a| a.is_array())
            && original.sources.len() == get(proof, "sourceReceipts")["after"].as_array().unwrap().len()
            && original.sources.iter().all(|s| {
                get(proof, "sourceReceipts")["after"].as_array().unwrap().iter().any(|p| {
                    get(p, "endpoint") == get(s, "endpoint")
                        && get(p, "sourceBodyHash") == get(s, "sourceBodyHash")
                        && get(p, "receivedAt") == get(s, "receivedAt")
                })
            }),
        "ORIGINAL_AFTER_SOURCE",
    )?;
    let orig_hist: Vec<V> = original.history.iter().map(|(k, v)| json!([k, v])).collect();
    let pre_hist: Vec<V> = pre.history.iter().map(|(k, v)| json!([k, v])).collect();
    need(
        canonical_payload(&json!(orig_hist))? == canonical_payload(&json!(pre_hist))?,
        "TARGET_HISTORY_SINCE_BUY_CHANGED",
    )?;
    let oid_i64 = get(b, "orderId").as_str().unwrap_or("").parse::<i64>().unwrap_or(i64::MIN);
    let original_order = pre.history.get(&oid_i64);
    need(
        original_order.is_some_and(|o| {
            get(o, "side") == &json!("no")
                && get(o, "action") == &json!("buy")
                && get(o, "quantity") == &json!(1)
                && get(o, "quantityFilled") == &json!(1)
                && get(o, "open") == &json!(false)
        }),
        "ORIGINAL_BUY_ORDER_REQUIRED",
    )?;

    let pre_pos = pre.positions.get(get(b, "exchangeId").as_str().unwrap_or(""));
    need(
        pre_pos.is_some_and(|p| {
            get(p, "marketId") == get(b, "marketId")
                && get(p, "quantity") == &json!(-1)
                && get(p, "settled") == &json!(false)
        }),
        "PRIOR_OWNED_NO_REQUIRED",
    )?;
    let original_body = json!({
        "idempotencyKey": get(b, "idempotencyKey"),
        "exchangeId": get(b, "exchangeId"),
        "side": "no",
        "action": "sell",
        "quantity": 1,
        "price": get(b, "limit").as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN),
        "expirationDate": get(b, "expirationDate"),
        "tournamentId": get(b, "tournamentId"),
    });
    let a = read(get(input, "ack"), "POST", "/orders", now, false)?;
    need(
        get(get(input, "ack"), "requestBodyHash") == &json!(payload_hash(&original_body)?)
            && a.start >= created,
        "ACK_REQUEST_BINDING",
    )?;
    need(a.start < expires, "LATE_ACK_ORIGINAL_WIRE_PROOF_NOT_IMPLEMENTED")?;
    let ack_economics = crate::no_neutral_sell_all_v1::inspect_no_sell_ack_all_economics(get(input, "ack")).map_err(|_| "REST_NO_UNSUPPORTED_NO_SELL_ALL".to_string())?;
    let d = a.data.clone();
    need(
        integer(get(&d, "orderId"), 1)
            && get(&d, "orderId").as_i64().map(|n| n.to_string()).as_deref() == get(b, "orderId").as_str()
            && get(&d, "exchangeId") == get(b, "exchangeId")
            && get(&d, "side") == &json!("no")
            && get(&d, "action") == &json!("sell")
            && get(&d, "quantity") == &json!(1)
            && tick(get(&d, "price"))? == limit
            && get(&d, "open").is_boolean()
            && integer(get(&d, "quantityTraded"), 0)
            && integer(get(&d, "remainingQuantity"), 0)
            && js_int(get(&d, "quantityTraded")).unwrap_or(0) + js_int(get(&d, "remainingQuantity")).unwrap_or(0) == 1,
        "ACK_CANONICAL",
    )?;
    for field in ["quantity", "quantityTraded", "remainingQuantity"] {
        exact_integer(&a, &json!([field]), get(&d, field))?;
    }
    let total_lexeme = a.numbers.get("[\"totalCost\"]");
    let ack_cost = match total_lexeme {
        Some(l) => amount(l).map_err(|_| "REST_NO_EXACT_ACK_NOTIONAL".to_string())?,
        None => return Err("REST_NO_EXACT_ACK_NOTIONAL".into()),
    };
    need(
        ack_cost >= 0 && ack_cost <= 995_000 && ack_cost % 5_000 == 0
            && if get(&d, "quantityTraded") == &json!(0) { ack_cost == 0 } else { ack_cost > 0 && ack_cost >= limit },
        "ACK_NOTIONAL",
    )?;
    let terminal = read(get(input, "order"), "GET", &format!("/orders/{}", get(b, "orderId").as_str().unwrap_or("")), now, true)?;
    let od = terminal.data.clone();
    need(
        integer(get(&od, "id"), 1)
            && get(&od, "id").as_i64().map(|n| n.to_string()).as_deref() == get(b, "orderId").as_str()
            && get(&od, "exchangeId") == get(b, "exchangeId")
            && get(&od, "tournamentId") == get(b, "tournamentId")
            && get(&od, "side") == &json!("no")
            && get(&od, "action") == &json!("sell")
            && get(&od, "quantity") == &json!(1)
            && integer(get(&od, "quantityFilled"), 0)
            && js_int(get(&od, "quantityFilled")).unwrap_or(0) <= 1
            && tick(get(&od, "priceLimit"))? == limit
            && get(&od, "open") == &json!(false)
            && instant_v(get(&od, "expirationDate"))? == expires
            && instant_v(get(&od, "createdAt"))? >= created - 2_000
            && instant_v(get(&od, "createdAt"))? <= a.end.min(expires) + 2_000,
        "TERMINAL_ORDER",
    )?;
    let mut totals: Option<i64> = None;
    let mut average: Option<i128> = None;
    let order_id_str = get(b, "orderId").as_str().unwrap_or("").to_string();
    let f = pages(
        get(input, "fillPages"),
        &format!("/orders/{order_id_str}/fills?limit=200"),
        now,
        Some(&mut |data: &V, receipt: &Read| -> Result<(), String> {
            need(
                integer(get(data, "orderId"), 1)
                    && get(data, "orderId").as_i64().map(|n| n.to_string()).as_deref() == Some(order_id_str.as_str())
                    && get(data, "exchangeId") == get(b, "exchangeId")
                    && get(data, "tournamentId") == get(b, "tournamentId")
                    && integer(get(data, "totalQuantityFilled"), i64::MIN)
                    && matches!(js_int(get(data, "totalQuantityFilled")), Some(0) | Some(-1)),
                "FILLS_SCOPE_OR_TOTAL",
            )?;
            exact_integer(receipt, &json!(["totalQuantityFilled"]), get(data, "totalQuantityFilled"))?;
            for (i, row) in get(data, "data").as_array().cloned().unwrap_or_default().iter().enumerate() {
                exact_integer(receipt, &json!(["data", i, "quantity"]), get(row, "quantity"))?;
            }
            let av = if get(data, "avgFillPrice").is_null() { None } else { Some(tick(get(data, "avgFillPrice"))?) };
            if let Some(t) = totals {
                need(t == js_int(get(data, "totalQuantityFilled")).unwrap() && average == av, "FILL_PROJECTION_CHANGED")?;
            }
            totals = js_int(get(data, "totalQuantityFilled"));
            average = av;
            Ok(())
        }),
    )?;
    need(f.sources[0].start >= terminal.end, "FILLS_BEFORE_TERMINAL")?;
    let mut seen: BTreeSet<i64> = BTreeSet::new();
    let mut fills: Vec<V> = Vec::new();
    let mut principal: i128 = 0;
    let mut quantity: i64 = 0;
    for row in &f.rows {
        need(
            integer(get(row, "id"), 1)
                && !seen.contains(&js_int(get(row, "id")).unwrap_or(i64::MIN))
                && get(row, "side") == &json!("no")
                && get(row, "quantity") == &json!(-1),
            "FILL_IDENTITY_OR_QUANTITY",
        )?;
        seen.insert(js_int(get(row, "id")).unwrap());
        let at = instant_v(get(row, "filledAt"))?;
        let p = tick(get(row, "price"))?;
        need(
            at >= instant_v(get(&od, "createdAt"))? && at <= f.sources.last().unwrap().end && at <= now && p > 0 && p >= limit,
            "FILL_PRICE_OR_TIME",
        )?;
        quantity += 1;
        principal += p;
        fills.push(json!({
            "id": get(row, "id"),
            "orderId": get(b, "orderId"),
            "marketId": get(b, "marketId"),
            "exchangeId": get(b, "exchangeId"),
            "tournamentId": get(b, "tournamentId"),
            "side": "no",
            "signedQuantity": -1,
            "quantity": 1,
            "price": money(p),
            "filledAt": get(row, "filledAt"),
            "source": "rest_order_fills",
            "priceBasis": "side_relative",
        }));
    }
    need(
        quantity <= 1
            && totals == Some(-quantity)
            && if quantity == 0 { average.is_none() } else { average == Some(principal) }
            && js_int(get(&d, "quantityTraded")).unwrap_or(0) <= quantity,
        "COMPLETE_FILL_TOTAL",
    )?;
    need(js_int(get(&od, "quantityFilled")) == Some(quantity), "ORDER_FILL_TOTAL")?;
    if get(&d, "quantityTraded") == &json!(1) {
        need(principal == ack_cost, "ACK_FINAL_NOTIONAL_MISMATCH")?;
    } else {
        need(get(&d, "fillPrice").is_null(), "ACK_UNFILLED_PRICE")?;
    }
    if get(&d, "quantityTraded") == &json!(1) {
        need(tick(get(&d, "fillPrice"))? == principal, "ACK_FILL_PRICE")?;
    }
    let fills_max = fills.iter().map(|v| instant_v(get(v, "filledAt"))).collect::<Result<Vec<i64>, String>>()?;
    need(
        post.start >= *[terminal.end, f.sources.last().unwrap().end].iter().chain(fills_max.iter()).max().unwrap_or(&i64::MIN),
        "POST_ACCOUNT_BEFORE_ORDER_EVIDENCE",
    )?;
    need(instant_v(get(global_risk, "at"))? >= post.end, "GLOBAL_RISK_BEFORE_ACCOUNT")?;
    need(!pre.history.contains_key(&oid_i64) && post.history.contains_key(&oid_i64), "SCOPED_ORDER_MISSING_OR_PREEXISTING")?;
    for key in pre.history.keys().chain(post.history.keys()).cloned().collect::<BTreeSet<i64>>() {
        if key != oid_i64 {
            need(
                canon_or_null(pre.history.get(&key))? == canon_or_null(post.history.get(&key))?,
                "EXTERNAL_SCOPED_ORDER_ACTIVITY",
            )?;
        }
    }
    let final_order = post.history.get(&oid_i64).cloned().unwrap_or(V::Null);
    for key in ["id", "exchangeId", "tournamentId", "side", "action", "quantity", "priceLimit", "open", "createdAt", "expirationDate"] {
        need(
            canon_or_missing(final_order.get(key))? == canon_or_missing(od.get(key))?,
            "SCOPED_ORDER_DETAIL_MISMATCH",
        )?;
    }
    need(
        integer(get(&final_order, "quantityFilled"), 0) && js_int(get(&final_order, "quantityFilled")) == Some(quantity),
        "SCOPED_ORDER_FILL_TOTAL",
    )?;
    let target = post.positions.get(get(b, "exchangeId").as_str().unwrap_or(""));
    if quantity == 0 {
        need(
            target.is_some_and(|t| get(t, "marketId") == get(b, "marketId") && get(t, "quantity") == &json!(-1) && get(t, "settled") == &json!(false)),
            "SCOPED_POSITION_DELTA",
        )?;
    } else {
        need(!target.is_some_and(|t| js_truthy(t.get("quantity"))), "SCOPED_POSITION_DELTA")?;
    }
    if let Some(t) = target {
        need(get(t, "marketId") == get(b, "marketId") && get(t, "settled") == &json!(false), "SCOPED_POSITION_IDENTITY")?;
    }
    same_external(&pre.positions, &post.positions, get(b, "exchangeId").as_str().unwrap_or(""))?;
    let delta_low = post.cash.lower - pre.cash.upper;
    let delta_high = post.cash.upper - pre.cash.lower;
    let expected = principal;
    need(expected >= delta_low && expected <= delta_high, "CASH_INTERVAL_INCONSISTENT")?;
    let mut activity_status = "unknown".to_string();
    let mut activity_hash: Option<String> = None;
    let account_activity = input.get("accountActivity").cloned().unwrap_or(V::Null);
    if !account_activity.is_null() {
        need(
            get(&account_activity, "tournamentId") == get(b, "tournamentId")
                && hex(get(&account_activity, "sourceHash"))
                && get(&account_activity, "complete").is_boolean(),
            "ACTIVITY_SCHEMA",
        )?;
        let activity_fill_match = |e: &V| -> bool {
            get(e, "kind") == &json!("fill")
                && get(e, "orderId") == get(b, "orderId")
                && get(e, "marketId") == get(b, "marketId")
                && get(e, "exchangeId") == get(b, "exchangeId")
                && fills.iter().any(|v| get(v, "id") == get(e, "fillId") && get(v, "filledAt") == get(e, "at"))
        };
        let events = get(&account_activity, "events");
        if get(&account_activity, "complete") == &json!(true) {
            need(
                instant_v(get(&account_activity, "from"))? <= pre.start
                    && instant_v(get(&account_activity, "through"))? >= post.end
                    && instant_v(get(&account_activity, "through"))? <= now
                    && events.is_array(),
                "ACTIVITY_WINDOW",
            )?;
            let ev = events.as_array().unwrap();
            let unique: BTreeSet<String> = ev.iter().map(|e| js_stringify(get(e, "fillId"))).collect();
            need(
                ev.len() == fills.len() && ev.iter().all(|e| activity_fill_match(e)) && unique.len() == fills.len(),
                "UNRELATED_OR_MISSING_ACTIVITY",
            )?;
            activity_status = "independently_complete_isolated".into();
            activity_hash = Some(payload_hash(&account_activity)?);
        } else {
            need(
                events.is_array() && events.as_array().unwrap().iter().all(|e| activity_fill_match(e)),
                "KNOWN_UNRELATED_ACTIVITY",
            )?;
            activity_status = "incomplete".into();
            activity_hash = Some(payload_hash(&account_activity)?);
        }
    }
    let sources = json!({
        "before": pre.sources,
        "ack": {
            "endpoint": a.endpoint,
            "sourceBodyHash": a.source_body_hash,
            "receivedAt": a.received_at,
            "requestBodyHash": get(get(input, "ack"), "requestBodyHash"),
        },
        "terminal": {
            "endpoint": terminal.endpoint,
            "sourceBodyHash": terminal.source_body_hash,
            "receivedAt": terminal.received_at,
        },
        "fills": f.sources.iter().map(|r| json!({
            "endpoint": r.endpoint,
            "sourceBodyHash": r.source_body_hash,
            "receivedAt": r.received_at,
        })).collect::<Vec<V>>(),
        "after": post.sources,
    });
    let removed_principal: V = if quantity > 0 { get(proof, "scopedPrincipalCost").clone() } else { json!("0.000000") };
    let realized_pnl = money(principal - if quantity > 0 { amount_v(get(proof, "scopedPrincipalCost"))? } else { 0 });
    let post_expiry = fills.iter().map(|v| instant_v(get(v, "filledAt"))).collect::<Result<Vec<i64>, String>>()?.iter().any(|&t| t > expires);
    let mut result = json!({
        "schemaVersion": 1,
        "mode": rest_no_sell_reconciliation_contract_v2()["mode"],
        "readAt": get(input, "readAt"),
        "bindingHash": payload_hash(b)?,
        "orderId": get(b, "orderId"),
        "intentId": get(b, "intentId"),
        "ownerId": get(b, "ownerId"),
        "runId": get(b, "runId"),
        "tournamentId": get(b, "tournamentId"),
        "marketId": get(b, "marketId"),
        "exchangeId": get(b, "exchangeId"),
        "scopedExitVerified": true,
        "filledQuantity": quantity,
        "signedPositionDelta": quantity,
        "scopedProceeds": money(principal),
        "originalBuyProofHash": get(proof, "verificationHash"),
        "originalLotId": get(proof, "lot").get("lotId"),
        "originalPrincipalCost": get(proof, "scopedPrincipalCost"),
        "removedPrincipalCost": removed_principal,
        "realizedPnl": realized_pnl,
        "expectedCanonicalTradeCashDelta": money(expected),
        "fills": fills,
        "ackEconomics": ack_economics,
        "remainingOwnedLot": if quantity == 0 { get(proof, "lot").clone() } else { V::Null },
        "canonicalAckFilledQuantity": get(&d, "quantityTraded"),
        "ackCoversFinalFill": get(&d, "quantityTraded") == &json!(quantity),
        "ackNotional": money(ack_cost),
        "requestedLimit": money(limit),
        "requestedExpirationDate": get(b, "expirationDate"),
        "postExpiryFillObserved": post_expiry,
        "globalIntentCensus": {
            "sourceHash": get(global_risk, "sourceHash"),
            "at": get(global_risk, "at"),
            "unresolvedIntentIds": get(global_risk, "unresolvedIntentIds").clone(),
        },
        "inventory": {
            "priorSignedQuantity": -1,
            "remainingSignedQuantity": if quantity > 0 { 0 } else { -1 },
            "originalLotId": get(proof, "lot").get("lotId"),
            "unbackedSell": false,
        },
        "accountCash": {
            "basis": "rounded_cent_with_conservative_plus_minus_one_cent",
            "before": {
                "displayed": money(pre.cash.displayed),
                "lower": money(pre.cash.lower),
                "upper": money(pre.cash.upper),
            },
            "after": {
                "displayed": money(post.cash.displayed),
                "lower": money(post.cash.lower),
                "upper": money(post.cash.upper),
            },
            "observedDeltaInterval": {
                "lower": money(delta_low),
                "upper": money(delta_high),
            },
            "consistent": true,
            "exactDelta": V::Null,
        },
        "scopedOrderActivity": {
            "complete": true,
            "beforeOrderCount": pre.history.len(),
            "afterOrderCount": post.history.len(),
            "newOrderIds": [get(b, "orderId")],
            "unrelatedChanges": 0,
        },
        "globalActivity": {
            "status": activity_status,
            "evidenceHash": activity_hash,
            "independentlyComplete": activity_status == "independently_complete_isolated",
        },
        "globalCashReconciledExactly": false,
        "globalAccountActivityVerified": activity_status == "independently_complete_isolated",
        "sourceReceipts": sources,
        "authoritativeReconciliation": false,
        "executionAuthority": false,
        "newSubmissionAuthorized": false,
    });
    result["verificationHash"] = json!(payload_hash(&result)?);
    Ok(result)
}
