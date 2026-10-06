// execution/no-rest-sell-reconciliation-v1.mjs — standalone, read-only
// evidence reduction for the covered one-share NO exit. It does not weaken the
// older exact global-cash verifier or change the realtime YES-denominated fill
// convention. REST NO prices are SIDE RELATIVE in the official specification
// observed 2026-10-06: https://sig.thesuper.market/api/v1/docs (Order/Fill/
// order fills). Unlike the buy modules this file additionally requires the raw
// lexeme for every quantity that drives the ledger (exactInteger).
use crate::core::{canonical_payload, payload_hash};
use crate::gates::normalize_official_fill_tick;
use crate::receipts::parse_number_lexemes;
use baal_service_runtime::fixed::{amount, instant, money, price};
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::collections::{BTreeMap, BTreeSet};

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
// canonicalPayload(x ?? null)
fn canon_or_null(v: Option<&V>) -> Result<String, String> {
    canonical_payload(&v.cloned().unwrap_or(V::Null))
}
// canonicalPayload(x) — absent keys throw INVALID_JSON_VALUE as in JS.
fn canon_or_missing(v: Option<&V>) -> Result<String, String> {
    match v {
        Some(x) => canonical_payload(x),
        None => Err("INVALID_JSON_VALUE".into()),
    }
}
// JSON.stringify(path) for small index-path arrays like ['positions',0,'quantity'].
fn path_key(path: &[V]) -> String {
    serde_json::to_string(path).unwrap()
}

// REST_NO_SELL_RECONCILIATION_CONTRACT_V1 (frozen object).
pub fn rest_no_sell_reconciliation_contract_v1() -> V {
    json!({
        "schemaVersion": 1,
        "mode": "scoped_rest_no_owned_exit_with_rounded_cash_v1",
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
        "nonNullAllSupported": false,
        "executionAuthority": false,
    })
}

#[derive(Clone)]
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

// exactInteger(r,path,value) — the raw numeric lexeme at `path` must be an
// exact integer whose canonical digits equal `value`.
fn exact_integer(r: &Read, path: &[V], value: &V) -> Result<(), String> {
    let lexeme = r.numbers.get(&path_key(path)).cloned();
    let ok = match &lexeme {
        Some(l) => {
            let canonical = l.len() <= 128 && {
                let body = l.strip_prefix('-').unwrap_or(l);
                let mut parts = body.splitn(2, '.');
                let whole = parts.next().unwrap_or("");
                let frac = parts.next();
                (whole == "0" || (!whole.is_empty() && !whole.starts_with('0')))
                    && whole.bytes().all(|b| b.is_ascii_digit())
                    && match frac {
                        None => true,
                        Some(f) => !f.is_empty() && f.bytes().all(|b| b == b'0'),
                    }
            };
            let digits_equal = js_int(value).is_some_and(|v| {
                l.split('.').next().unwrap_or("").parse::<i64>() == Ok(v)
                    || l.split('.').next().unwrap_or("").parse::<i128>().is_ok_and(|x| x == v as i128)
            });
            canonical && js_int(value).is_some() && digits_equal
        }
        None => false,
    };
    need(ok, "EXACT_INTEGER_SOURCE")
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
    // JS `r.sourceBodyHash!==undefined`: an explicit null value is checked and
    // fails, while an absent key is skipped.
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

#[derive(Clone)]
struct CashRead {
    read: Read,
    displayed: i128,
    lower: i128,
    upper: i128,
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

// roundedTournamentCashInterval(receipt,{tournamentId,tournamentSlug,readAt,requiredFunding,reservedElsewhere})
pub fn rounded_tournament_cash_interval(receipt: &V, options: &V) -> Result<V, String> {
    let scope = json!({
        "tournamentId": get(options, "tournamentId"),
        "tournamentSlug": get(options, "tournamentSlug"),
    });
    scope_check(&scope)?;
    let c = cash(receipt, &scope, instant_v(get(options, "readAt"))?)?;
    // Destructured defaults apply only when the key is absent (JS undefined),
    // not for an explicit null, which then fails inside amount().
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

// pages(receipts,{endpoint,now,check}) — here `check` receives (data, receipt).
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

// JS `new Map()` preserves insertion order; Vec<(K,V)> keeps it exactly.
struct Snapshot {
    cash: CashRead,
    positions: Vec<(String, V)>,
    history: Vec<(i64, V)>,
    start: i64,
    end: i64,
    sources: Vec<V>,
}
fn map_get<'a>(m: &'a [(String, V)], k: &str) -> Option<&'a V> {
    m.iter().find(|(kk, _)| kk == k).map(|(_, v)| v)
}
fn hist_get<'a>(h: &'a [(i64, V)], k: i64) -> Option<&'a V> {
    h.iter().find(|(kk, _)| *kk == k).map(|(_, v)| v)
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
    let mut positions: Vec<(String, V)> = Vec::new();
    for (i, row) in get(&p.data, "positions").as_array().unwrap().iter().enumerate() {
        need(
            id(get(row, "marketId")) && id(get(row, "exchangeId"))
                && js_int(get(row, "quantity")).is_some()
                && get(row, "settled").is_boolean()
                && map_get(&positions, get(row, "exchangeId").as_str().unwrap_or("")).is_none(),
            "POSITION_SCHEMA",
        )?;
        exact_integer(&p, &[json!("positions"), json!(i), json!("quantity")], get(row, "quantity"))?;
        positions.push((
            get(row, "exchangeId").as_str().unwrap().to_string(),
            json!({
                "marketId": get(row, "marketId"),
                "quantity": get(row, "quantity"),
                "settled": get(row, "settled"),
            }),
        ));
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
    let mut history: Vec<(i64, V)> = Vec::new();
    for row in &h.rows {
        need(
            integer(get(row, "id"), 1)
                && hist_get(&history, js_int(get(row, "id")).unwrap()).is_none()
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
        history.push((js_int(get(row, "id")).unwrap(), row.clone()));
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

fn same_external(before: &[(String, V)], after: &[(String, V)], exchange_id: &str) -> Result<(), String> {
    let keys: BTreeSet<String> = before.iter().chain(after.iter()).map(|(k, _)| k.clone()).collect();
    for k in keys {
        if k != exchange_id {
            let zero = |v: Option<&V>| -> V {
                match v {
                    Some(x) if js_truthy(x.get("quantity")) => x.clone(),
                    _ => V::Null,
                }
            };
            need(
                canonical_payload(&zero(map_get(before, &k)))? == canonical_payload(&zero(map_get(after, &k)))?,
                "EXTERNAL_POSITION_CHANGED",
            )?;
        }
    }
    Ok(())
}

// inspectScopedRestNoAccount({account,binding,readAt,requiredFunding,reservedElsewhere})
// Shared preflight parser for the bounded owned-exit lifecycle. It exposes
// curated account facts, not a write permit, and uses the same complete
// receipt checks. This copy additionally pins every position quantity lexeme.
pub fn inspect_scoped_rest_no_account(input: &V) -> Result<V, String> {
    let account = get(input, "account");
    let binding = get(input, "binding");
    let read_at = get(input, "readAt");
    let default_zero = json!("0.000000");
    let required_funding = input.get("requiredFunding").unwrap_or(&default_zero);
    let reserved_elsewhere = input.get("reservedElsewhere").unwrap_or(&default_zero);
    scope_check(binding)?;
    need(id(get(binding, "exchangeId")), "SCOPE")?;
    let a = snapshot(account, binding, instant_v(read_at)?)?;
    let required = amount_v(required_funding)? + amount_v(reserved_elsewhere)?;
    need(
        amount_v(required_funding)? >= 0 && amount_v(reserved_elsewhere)? >= 0,
        "FUNDING_NONNEGATIVE",
    )?;
    Ok(json!({
        "tournamentId": get(binding, "tournamentId"),
        "positions": a.positions.iter().map(|(exchange_id, v)| {
            let mut o = Map::new();
            o.insert("exchangeId".into(), json!(exchange_id));
            if let Some(m) = v.as_object() {
                for (k, x) in m {
                    o.insert(k.clone(), x.clone());
                }
            }
            V::Object(o)
        }).collect::<Vec<V>>(),
        "scopedOrders": a.history.iter().map(|(_, v)| v.clone()).collect::<Vec<V>>(),
        "startedAt": baal_service_runtime::contract::ms_to_iso(a.start as f64),
        "completedAt": baal_service_runtime::contract::ms_to_iso(a.end as f64),
        "cashLower": money(a.cash.lower),
        "cashUpper": money(a.cash.upper),
        "funded": a.cash.lower >= required,
        "sourceReceipts": a.sources,
        "executionAuthority": false,
    }))
}

// verifyScopedRestNoSell({binding,before,ack,order,fillPages,after,globalRisk,
// accountActivity,readAt,originalBuyProof,originalAfter})
//
// All inputs are supplied by the existing serialized collector. Raw receipt
// bodies are parsed locally but never returned, stored, logged or transmitted.
// Unknown GLOBAL economic activity is retained as unknown. A terminal, scoped
// one-share exit can still have an exact basis; these are different assertions.
pub fn verify_scoped_rest_no_sell(input: &V) -> Result<V, String> {
    let b = get(input, "binding");
    let ack = get(input, "ack");
    let order = get(input, "order");
    let fill_pages = get(input, "fillPages");
    let global_risk = get(input, "globalRisk");
    let account_activity = get(input, "accountActivity");
    let read_at = get(input, "readAt");
    let original_buy_proof = get(input, "originalBuyProof");
    let original_after = get(input, "originalAfter");
    let now = instant_v(read_at)?;
    scope_check(b)?;
    need(
        id(get(b, "orderId")) && id(get(b, "marketId")) && id(get(b, "exchangeId"))
            && get(b, "ownerId").is_string() && get(b, "runId").is_string()
            && get(b, "intentId").is_string() && get(b, "idempotencyKey").is_string()
            && get(b, "side").as_str() == Some("no")
            && get(b, "action").as_str() == Some("sell")
            && get(b, "quantity") == &json!(1),
        "BINDING",
    )?;
    let limit = price(get(b, "limit").as_str().unwrap_or(""))?;
    let created = instant_v(get(b, "createdAt"))?;
    let expires = instant_v(get(b, "expirationDate"))?;
    need(expires > created && expires - created <= 30_000 && created <= now, "BINDING_LIFETIME")?;
    need(
        get(global_risk, "tournamentId") == get(b, "tournamentId")
            && get(global_risk, "ownerId") == get(b, "ownerId")
            && get(global_risk, "runId") == get(b, "runId")
            && get(global_risk, "soleCollector") == &json!(true)
            && get(global_risk, "complete") == &json!(true)
            && get(global_risk, "stopped") == &json!(false)
            && hex(get(global_risk, "sourceHash"))
            && get(global_risk, "unresolvedIntentIds").as_array().is_some_and(|u| {
                u.len() <= 1 && u.iter().all(|x| x == get(b, "intentId"))
            })
            && instant_v(get(global_risk, "at"))? <= now
            && now - instant_v(get(global_risk, "at"))? <= 20_000,
        "GLOBAL_UNKNOWN_OR_OWNER",
    )?;
    let pre = snapshot(get(input, "before"), b, created)?;
    let post = snapshot(get(input, "after"), b, now)?;
    need(pre.end <= created && post.start >= created, "ACCOUNT_CAUSALITY")?;
    // const {verificationHash,...originalProofBody}=proof??{}
    let mut original_proof_body = original_buy_proof.clone();
    let verification_hash = original_proof_body
        .as_object_mut()
        .and_then(|m| m.remove("verificationHash"));
    need(
        verification_hash.as_ref().is_some_and(|h| hex(h))
            && payload_hash(&original_proof_body)? == verification_hash.as_ref().and_then(|h| h.as_str()).unwrap_or("")
            && matches!(
                get(original_buy_proof, "mode").as_str(),
                Some("scoped_rest_no_lot_with_rounded_cash_v1" | "scoped_rest_no_lot_with_rounded_cash_neutral_all_v2")
            )
            && get(original_buy_proof, "scopedLotVerified") == &json!(true)
            && get(original_buy_proof, "filledQuantity") == &json!(1)
            && get(original_buy_proof, "signedPositionDelta") == &json!(-1)
            && get(original_buy_proof, "ownerId") == get(b, "ownerId")
            && get(original_buy_proof, "runId") == get(b, "runId")
            && get(original_buy_proof, "tournamentId") == get(b, "tournamentId")
            && get(original_buy_proof, "marketId") == get(b, "marketId")
            && get(original_buy_proof, "exchangeId") == get(b, "exchangeId")
            && get(original_buy_proof, "executionAuthority") == &json!(false),
        "ORIGINAL_BUY_PROVENANCE",
    )?;
    let lot = get(original_buy_proof, "lot");
    need(
        get(lot, "side").as_str() == Some("no")
            && get(lot, "quantity") == &json!(1)
            && get(lot, "lotId").as_str().is_some_and(|s| !s.is_empty())
            && get(lot, "ownerId") == get(b, "ownerId")
            && get(lot, "runId") == get(b, "runId")
            && get(lot, "orderId") == get(original_buy_proof, "orderId")
            && get(lot, "intentId") == get(original_buy_proof, "intentId")
            && get(lot, "entryPrice") == get(original_buy_proof, "scopedPrincipalCost")
            && get(original_buy_proof, "fills").as_array().is_some_and(|f| f.len() == 1)
            && get(&get(original_buy_proof, "fills")[0], "price") == get(lot, "entryPrice")
            && get(&get(original_buy_proof, "fills")[0], "side").as_str() == Some("no")
            && get(&get(original_buy_proof, "fills")[0], "signedQuantity") == &json!(-1)
            && get(&get(original_buy_proof, "fills")[0], "quantity") == &json!(1)
            && get(&get(original_buy_proof, "fills")[0], "priceBasis").as_str() == Some("side_relative")
            && get(&get(original_buy_proof, "fills")[0], "source").as_str() == Some("rest_order_fills")
            && get(&get(original_buy_proof, "fills")[0], "filledAt") == get(lot, "openedAt"),
        "ORIGINAL_BUY_LOT",
    )?;
    price(get(lot, "entryPrice").as_str().unwrap_or(""))?;
    let binding_ex = get(b, "exchangeId").as_str().unwrap_or("").to_string();
    let original = snapshot(original_after, b, instant_v(get(original_buy_proof, "readAt"))?)?;
    need(
        original.end <= pre.start
            && map_get(&original.positions, &binding_ex).map(|v| get(v, "marketId")) == Some(get(b, "marketId"))
            && map_get(&original.positions, &binding_ex).map(|v| get(v, "quantity")) == Some(&json!(-1))
            && map_get(&original.positions, &binding_ex).map(|v| get(v, "settled")) == Some(&json!(false))
            && get(original_buy_proof, "sourceReceipts").get("after").is_some_and(V::is_array)
            && original.sources.len() == get(original_buy_proof, "sourceReceipts").get("after").and_then(V::as_array).unwrap().len()
            && original.sources.iter().all(|s| {
                get(original_buy_proof, "sourceReceipts").get("after").and_then(V::as_array).unwrap().iter().any(|p| {
                    get(p, "endpoint") == get(s, "endpoint")
                        && get(p, "sourceBodyHash") == get(s, "sourceBodyHash")
                        && get(p, "receivedAt") == get(s, "receivedAt")
                })
            }),
        "ORIGINAL_AFTER_SOURCE",
    )?;
    need(
        canonical_payload(&V::Array(
            original.history.iter().map(|(k, v)| json!([json!(k), v])).collect(),
        ))? == canonical_payload(&V::Array(
            pre.history.iter().map(|(k, v)| json!([json!(k), v])).collect(),
        ))?,
        "TARGET_HISTORY_SINCE_BUY_CHANGED",
    )?;
    let original_order = hist_get(&pre.history, js_int(get(original_buy_proof, "orderId")).unwrap_or(0)).cloned().unwrap_or(V::Null);
    need(
        get(&original_order, "side").as_str() == Some("no")
            && get(&original_order, "action").as_str() == Some("buy")
            && get(&original_order, "quantity") == &json!(1)
            && get(&original_order, "quantityFilled") == &json!(1)
            && get(&original_order, "open") == &json!(false),
        "ORIGINAL_BUY_ORDER_REQUIRED",
    )?;
    need(
        map_get(&pre.positions, &binding_ex).map(|v| get(v, "marketId")) == Some(get(b, "marketId"))
            && map_get(&pre.positions, &binding_ex).map(|v| get(v, "quantity")) == Some(&json!(-1))
            && map_get(&pre.positions, &binding_ex).map(|v| get(v, "settled")) == Some(&json!(false)),
        "PRIOR_OWNED_NO_REQUIRED",
    )?;
    let price_number: f64 = get(b, "limit").as_str().unwrap_or("").parse().unwrap_or(f64::NAN);
    let original_body = json!({
        "idempotencyKey": get(b, "idempotencyKey"),
        "exchangeId": get(b, "exchangeId"),
        "side": "no",
        "action": "sell",
        "quantity": 1,
        "price": price_number,
        "expirationDate": get(b, "expirationDate"),
        "tournamentId": get(b, "tournamentId"),
    });
    let a = read(ack, "POST", "/orders", now, false)?;
    need(
        get(ack, "requestBodyHash").as_str() == Some(payload_hash(&original_body)?.as_str())
            && a.start >= created,
        "ACK_REQUEST_BINDING",
    )?;
    need(a.start < expires, "LATE_ACK_ORIGINAL_WIRE_PROOF_NOT_IMPLEMENTED")?;
    need(a.data.get("all") == Some(&V::Null), "UNSUPPORTED_NO_SELL_ALL")?;
    let d = &a.data;
    need(
        integer(get(d, "orderId"), 1)
            && crate::js_key(get(d, "orderId")) == get(b, "orderId").as_str().unwrap_or("")
            && get(d, "exchangeId") == get(b, "exchangeId")
            && get(d, "side").as_str() == Some("no")
            && get(d, "action").as_str() == Some("sell")
            && get(d, "quantity") == &json!(1)
            && tick(get(d, "price"))? == limit
            && get(d, "open").is_boolean()
            && integer(get(d, "quantityTraded"), 0)
            && integer(get(d, "remainingQuantity"), 0)
            && js_int(get(d, "quantityTraded")).unwrap_or(0) + js_int(get(d, "remainingQuantity")).unwrap_or(0) == 1,
        "ACK_CANONICAL",
    )?;
    for field in ["quantity", "quantityTraded", "remainingQuantity"] {
        exact_integer(&a, &[json!(field)], get(d, field))?;
    }
    let total_lexeme = a.numbers.get("[\"totalCost\"]").cloned().unwrap_or_default();
    let ack_cost = amount(&total_lexeme).map_err(|_| "REST_NO_EXACT_ACK_NOTIONAL".to_string())?;
    need(
        ack_cost >= 0
            && ack_cost <= 995_000
            && ack_cost % 5_000 == 0
            && (if get(d, "quantityTraded") == &json!(0) { ack_cost == 0 } else { ack_cost > 0 && ack_cost >= limit }),
        "ACK_NOTIONAL",
    )?;
    let terminal = read(order, "GET", &format!("/orders/{}", get(b, "orderId").as_str().unwrap_or("")), now, true)?;
    let od = &terminal.data;
    need(
        integer(get(od, "id"), 1)
            && crate::js_key(get(od, "id")) == get(b, "orderId").as_str().unwrap_or("")
            && get(od, "exchangeId") == get(b, "exchangeId")
            && get(od, "tournamentId") == get(b, "tournamentId")
            && get(od, "side").as_str() == Some("no")
            && get(od, "action").as_str() == Some("sell")
            && get(od, "quantity") == &json!(1)
            && integer(get(od, "quantityFilled"), 0)
            && js_int(get(od, "quantityFilled")).unwrap_or(0) <= 1
            && tick(get(od, "priceLimit"))? == limit
            && get(od, "open") == &json!(false)
            && instant_v(get(od, "expirationDate"))? == expires
            && instant_v(get(od, "createdAt"))? >= created - 2_000
            && instant_v(get(od, "createdAt"))? <= std::cmp::min(a.end, expires) + 2_000,
        "TERMINAL_ORDER",
    )?;
    let mut totals: Option<i64> = None;
    let mut average: Option<i128> = None;
    let order_id_s = get(b, "orderId").as_str().unwrap_or("").to_string();
    let exchange_id_v = get(b, "exchangeId").clone();
    let tournament_id_v = get(b, "tournamentId").clone();
    let mut fills_check = |data: &V, receipt: &Read| -> Result<(), String> {
        need(
            integer(get(data, "orderId"), 1)
                && crate::js_key(get(data, "orderId")) == order_id_s
                && get(data, "exchangeId") == &exchange_id_v
                && get(data, "tournamentId") == &tournament_id_v
                && js_int(get(data, "totalQuantityFilled")).is_some_and(|n| n == 0 || n == -1),
            "FILLS_SCOPE_OR_TOTAL",
        )?;
        exact_integer(receipt, &[json!("totalQuantityFilled")], get(data, "totalQuantityFilled"))?;
        for (i, row) in get(data, "data").as_array().cloned().unwrap_or_default().iter().enumerate() {
            exact_integer(receipt, &[json!("data"), json!(i), json!("quantity")], get(row, "quantity"))?;
        }
        let av = if data.get("avgFillPrice") == Some(&V::Null) {
            None
        } else {
            Some(tick(get(data, "avgFillPrice"))?)
        };
        if let Some(prev) = totals {
            need(
                prev == js_int(get(data, "totalQuantityFilled")).unwrap_or(-2) && average == av,
                "FILL_PROJECTION_CHANGED",
            )?;
        }
        totals = js_int(get(data, "totalQuantityFilled"));
        average = av;
        Ok(())
    };
    let f = pages(
        fill_pages,
        &format!("/orders/{}/fills?limit=200", order_id_s),
        now,
        Some(&mut fills_check),
    )?;
    need(f.sources[0].start >= terminal.end, "FILLS_BEFORE_TERMINAL")?;
    let mut seen: BTreeSet<i64> = BTreeSet::new();
    let mut fills: Vec<V> = Vec::new();
    let mut principal: i128 = 0;
    let mut quantity: i64 = 0;
    for row in &f.rows {
        need(
            integer(get(row, "id"), 1)
                && !seen.contains(&js_int(get(row, "id")).unwrap_or(0))
                && get(row, "side").as_str() == Some("no")
                && get(row, "quantity") == &json!(-1),
            "FILL_IDENTITY_OR_QUANTITY",
        )?;
        seen.insert(js_int(get(row, "id")).unwrap());
        let at = instant_v(get(row, "filledAt"))?;
        let p = tick(get(row, "price"))?;
        need(
            at >= instant_v(get(od, "createdAt"))?
                && at <= f.sources.last().unwrap().end
                && at <= now
                && p > 0
                && p >= limit,
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
            && (if quantity == 0 { average.is_none() } else { average == Some(principal) })
            && js_int(get(d, "quantityTraded")).unwrap_or(0) <= quantity,
        "COMPLETE_FILL_TOTAL",
    )?;
    need(js_int(get(od, "quantityFilled")) == Some(quantity), "ORDER_FILL_TOTAL")?;
    if get(d, "quantityTraded") == &json!(1) {
        need(principal == ack_cost, "ACK_FINAL_NOTIONAL_MISMATCH")?;
    } else {
        need(d.get("fillPrice") == Some(&V::Null), "ACK_UNFILLED_PRICE")?;
    }
    if get(d, "quantityTraded") == &json!(1) {
        need(tick(get(d, "fillPrice"))? == principal, "ACK_FILL_PRICE")?;
    }
    let mut post_floor = std::cmp::max(terminal.end, f.sources.last().unwrap().end);
    for v in &fills {
        post_floor = post_floor.max(instant_v(get(v, "filledAt"))?);
    }
    need(post.start >= post_floor, "POST_ACCOUNT_BEFORE_ORDER_EVIDENCE")?;
    need(instant_v(get(global_risk, "at"))? >= post.end, "GLOBAL_RISK_BEFORE_ACCOUNT")?;
    let oid = js_int(get(b, "orderId")).unwrap_or(0);
    need(
        hist_get(&pre.history, oid).is_none() && hist_get(&post.history, oid).is_some(),
        "SCOPED_ORDER_MISSING_OR_PREEXISTING",
    )?;
    let history_keys: BTreeSet<i64> = pre.history.iter().chain(post.history.iter()).map(|(k, _)| *k).collect();
    for key in history_keys {
        if key != oid {
            need(
                canon_or_null(hist_get(&pre.history, key))? == canon_or_null(hist_get(&post.history, key))?,
                "EXTERNAL_SCOPED_ORDER_ACTIVITY",
            )?;
        }
    }
    let final_order = hist_get(&post.history, oid).cloned().unwrap_or(V::Null);
    for key in ["id", "exchangeId", "tournamentId", "side", "action", "quantity", "priceLimit", "open", "createdAt", "expirationDate"] {
        need(
            canon_or_missing(final_order.get(key))?
                == canon_or_missing(od.as_object().and_then(|m| m.get(key)))?,
            "SCOPED_ORDER_DETAIL_MISMATCH",
        )?;
    }
    need(
        integer(get(&final_order, "quantityFilled"), 0)
            && js_int(get(&final_order, "quantityFilled")) == Some(quantity),
        "SCOPED_ORDER_FILL_TOTAL",
    )?;
    let target = map_get(&post.positions, &binding_ex);
    need(
        if quantity == 0 {
            target.is_some()
                && get(target.unwrap(), "marketId") == get(b, "marketId")
                && get(target.unwrap(), "quantity") == &json!(-1)
                && get(target.unwrap(), "settled") == &json!(false)
        } else {
            !target.is_some_and(|v| js_truthy(v.get("quantity")))
        },
        "SCOPED_POSITION_DELTA",
    )?;
    if let Some(t) = target {
        need(
            get(t, "marketId") == get(b, "marketId") && get(t, "settled") == &json!(false),
            "SCOPED_POSITION_IDENTITY",
        )?;
    }
    same_external(&pre.positions, &post.positions, &binding_ex)?;
    let delta_low = post.cash.lower - pre.cash.upper;
    let delta_high = post.cash.upper - pre.cash.lower;
    let expected = principal;
    need(expected >= delta_low && expected <= delta_high, "CASH_INTERVAL_INCONSISTENT")?;
    let mut activity_status = "unknown".to_string();
    let mut activity_hash = V::Null;
    if !account_activity.is_null() {
        need(
            get(account_activity, "tournamentId") == get(b, "tournamentId")
                && hex(get(account_activity, "sourceHash"))
                && get(account_activity, "complete").is_boolean(),
            "ACTIVITY_SCHEMA",
        )?;
        if get(account_activity, "complete") == &json!(true) {
            need(
                instant_v(get(account_activity, "from"))? <= pre.start
                    && instant_v(get(account_activity, "through"))? >= post.end
                    && instant_v(get(account_activity, "through"))? <= now
                    && get(account_activity, "events").is_array(),
                "ACTIVITY_WINDOW",
            )?;
            let events = get(account_activity, "events").as_array().unwrap();
            let event_ok = |e: &V| {
                get(e, "kind").as_str() == Some("fill")
                    && get(e, "orderId") == get(b, "orderId")
                    && get(e, "marketId") == get(b, "marketId")
                    && get(e, "exchangeId") == get(b, "exchangeId")
                    && fills.iter().any(|v| {
                        get(v, "id") == get(e, "fillId") && get(v, "filledAt") == get(e, "at")
                    })
            };
            need(
                events.len() == fills.len()
                    && events.iter().all(|e| event_ok(e))
                    && events.iter().map(|e| get(e, "fillId").to_string()).collect::<BTreeSet<_>>().len() == fills.len(),
                "UNRELATED_OR_MISSING_ACTIVITY",
            )?;
            activity_status = "independently_complete_isolated".to_string();
            activity_hash = json!(payload_hash(account_activity)?);
        } else {
            let events = get(account_activity, "events").as_array();
            need(
                events.is_some_and(|es| {
                    es.iter().all(|e| {
                        get(e, "kind").as_str() == Some("fill")
                            && get(e, "orderId") == get(b, "orderId")
                            && get(e, "marketId") == get(b, "marketId")
                            && get(e, "exchangeId") == get(b, "exchangeId")
                            && fills.iter().any(|v| {
                                get(v, "id") == get(e, "fillId") && get(v, "filledAt") == get(e, "at")
                            })
                    })
                }),
                "KNOWN_UNRELATED_ACTIVITY",
            )?;
            activity_status = "incomplete".to_string();
            activity_hash = json!(payload_hash(account_activity)?);
        }
    }
    let sources = json!({
        "before": pre.sources,
        "ack": {
            "endpoint": a.endpoint,
            "sourceBodyHash": a.source_body_hash,
            "receivedAt": a.received_at,
            "requestBodyHash": get(ack, "requestBodyHash"),
        },
        "terminal": {
            "endpoint": terminal.endpoint,
            "sourceBodyHash": terminal.source_body_hash,
            "receivedAt": terminal.received_at,
        },
        "fills": f.sources.iter().map(|r| {
            json!({"endpoint": r.endpoint, "sourceBodyHash": r.source_body_hash, "receivedAt": r.received_at})
        }).collect::<Vec<V>>(),
        "after": post.sources,
    });
    let remaining_owned_lot = if quantity == 0 {
        get(original_buy_proof, "lot").clone()
    } else {
        V::Null
    };
    let original_cost = amount_v(get(original_buy_proof, "scopedPrincipalCost"))?;
    let mut post_expiry_fill = false;
    for v in &fills {
        if instant_v(get(v, "filledAt"))? > expires {
            post_expiry_fill = true;
        }
    }
    let result = Map::from_iter([
        ("schemaVersion".into(), json!(1)),
        ("mode".into(), get(&rest_no_sell_reconciliation_contract_v1(), "mode").clone()),
        ("readAt".into(), read_at.clone()),
        ("bindingHash".into(), json!(payload_hash(b)?)),
        ("orderId".into(), get(b, "orderId").clone()),
        ("intentId".into(), get(b, "intentId").clone()),
        ("ownerId".into(), get(b, "ownerId").clone()),
        ("runId".into(), get(b, "runId").clone()),
        ("tournamentId".into(), get(b, "tournamentId").clone()),
        ("marketId".into(), get(b, "marketId").clone()),
        ("exchangeId".into(), get(b, "exchangeId").clone()),
        ("scopedExitVerified".into(), json!(true)),
        ("filledQuantity".into(), json!(quantity)),
        ("signedPositionDelta".into(), json!(quantity)),
        ("scopedProceeds".into(), json!(money(principal))),
        ("originalBuyProofHash".into(), get(original_buy_proof, "verificationHash").clone()),
        ("originalLotId".into(), get(get(original_buy_proof, "lot"), "lotId").clone()),
        ("originalPrincipalCost".into(), get(original_buy_proof, "scopedPrincipalCost").clone()),
        ("removedPrincipalCost".into(), json!(if quantity != 0 { get(original_buy_proof, "scopedPrincipalCost").clone() } else { json!("0.000000") })),
        ("realizedPnl".into(), json!(money(principal - if quantity != 0 { original_cost } else { 0 }))),
        ("expectedCanonicalTradeCashDelta".into(), json!(money(expected))),
        ("fills".into(), V::Array(fills)),
        ("remainingOwnedLot".into(), remaining_owned_lot),
        ("canonicalAckFilledQuantity".into(), get(d, "quantityTraded").clone()),
        ("ackCoversFinalFill".into(), json!(get(d, "quantityTraded") == &json!(quantity))),
        ("ackNotional".into(), json!(money(ack_cost))),
        ("requestedLimit".into(), json!(money(limit))),
        ("requestedExpirationDate".into(), get(b, "expirationDate").clone()),
        ("postExpiryFillObserved".into(), json!(post_expiry_fill)),
        (
            "globalIntentCensus".into(),
            json!({
                "sourceHash": get(global_risk, "sourceHash"),
                "at": get(global_risk, "at"),
                "unresolvedIntentIds": get(global_risk, "unresolvedIntentIds"),
            }),
        ),
        (
            "inventory".into(),
            json!({
                "priorSignedQuantity": -1,
                "remainingSignedQuantity": if quantity != 0 { json!(0) } else { json!(-1) },
                "originalLotId": get(get(original_buy_proof, "lot"), "lotId"),
                "unbackedSell": false,
            }),
        ),
        (
            "accountCash".into(),
            json!({
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
                "observedDeltaInterval": {"lower": money(delta_low), "upper": money(delta_high)},
                "consistent": true,
                "exactDelta": V::Null,
            }),
        ),
        (
            "scopedOrderActivity".into(),
            json!({
                "complete": true,
                "beforeOrderCount": pre.history.len(),
                "afterOrderCount": post.history.len(),
                "newOrderIds": [get(b, "orderId")],
                "unrelatedChanges": 0,
            }),
        ),
        (
            "globalActivity".into(),
            json!({
                "status": activity_status,
                "evidenceHash": activity_hash,
                "independentlyComplete": activity_status == "independently_complete_isolated",
            }),
        ),
        ("globalCashReconciledExactly".into(), json!(false)),
        ("globalAccountActivityVerified".into(), json!(activity_status == "independently_complete_isolated")),
        ("sourceReceipts".into(), sources),
        ("authoritativeReconciliation".into(), json!(false)),
        ("executionAuthority".into(), json!(false)),
        ("newSubmissionAuthorized".into(), json!(false)),
    ]);
    let vh = payload_hash(&V::Object(result.clone()))?;
    let mut result = result;
    result.insert("verificationHash".into(), json!(vh));
    Ok(V::Object(result))
}
