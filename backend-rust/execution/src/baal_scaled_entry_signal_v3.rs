// execution/baal-scaled-entry-signal-v3.mjs — generic multi-rung successor.
// Uses exact fresh receipts, preserves original capital ceilings, and
// reserves 0.990 per share even if the displayed pair costs less. Generic
// conditional relative-value package gap; no fair-probability estimate,
// reversion promise, hedge dispatch, client, timer or execution authority.
//
// Port notes:
// - Every failure inside the plan body is caught exactly like JS: codes
//   matching /^BAAL_SCALED_ENTRY_[A-Z_]+$/ surface as `reason`; anything else
//   (fixed-money parse errors, TypeError-equivalents, verifier errors)
//   becomes 'INVALID_OR_INCOMPLETE_SCALED_ENTRY_EVIDENCE'.
// - `x ?? y` -> explicit absent-or-null fallback; `a?.b` -> Option chains.
// - JS BigInt -> i128 (all quantities/prices are bounded by the ceilings).
// - `Buffer.byteLength` -> str::len (UTF-8 bytes).
use crate::core::payload_hash;
use crate::no_rest_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::party_winner_no_screen_v1::screen_party_winner_no_pairs;
use crate::receipts::parse_number_lexemes;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use baal_service_runtime::market::normalize_book;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::{BTreeMap, BTreeSet, HashSet};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("BAAL_SCALED_ENTRY_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
fn hex(x: &V) -> bool {
    x.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty()
            && s.bytes().all(|b| b.is_ascii_digit())
            && !s.starts_with('0')
            && s.parse::<i64>().is_ok()
    })
}
fn str_amount(v: &V) -> Result<i128, String> {
    amount(v.as_str().ok_or_else(|| "INVALID_AMOUNT".to_string())?)
}
fn str_price(v: &V) -> Result<i128, String> {
    price(v.as_str().ok_or_else(|| "INVALID_PRICE".to_string())?)
}
fn str_instant(v: &V) -> Result<i64, String> {
    instant(v.as_str().ok_or_else(|| "INVALID_INSTANT".to_string())?)
}
fn is_entry_code(e: &str) -> bool {
    e.starts_with("BAAL_SCALED_ENTRY_")
        && e.len() > "BAAL_SCALED_ENTRY_".len()
        && e.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
}
fn num_key(path: &[V]) -> String {
    serde_json::to_string(path).unwrap()
}
fn lexeme_int_eq(raw: &str, value: i64) -> bool {
    // BigInt(raw.split('.')[0]) === BigInt(value)
    let part = raw.split('.').next().unwrap_or("");
    BigInt::parse_bytes(part.as_bytes(), 10) == Some(BigInt::from(value))
}

struct Raw {
    data: V,
    numbers: BTreeMap<String, String>,
    start: i64,
    end: i64,
    source_body_hash: String,
    request_started_at: V,
    received_at: V,
}
fn raw(receipt: &V, endpoint: &str, at: i64, max_age: i64) -> Result<Raw, String> {
    need(
        get(receipt, "method").as_str() == Some("GET")
            && get(receipt, "endpoint").as_str() == Some(endpoint)
            && js_int(get(receipt, "status")) == Some(200)
            && get(receipt, "bodyText")
                .as_str()
                .is_some_and(|s| s.len() <= 8 * 1024 * 1024),
        "RAW_RECEIPT",
    )?;
    let start = str_instant(get(receipt, "requestStartedAt"))?;
    let end = str_instant(get(receipt, "receivedAt"))?;
    need(
        start <= end && end <= at && at - start <= max_age && end - start <= 20000,
        "RECEIPT_TIME",
    )?;
    let source_body_hash = sha256(get(receipt, "bodyText").as_str().unwrap_or("").as_bytes());
    need(
        receipt.get("sourceBodyHash").is_none()
            || get(receipt, "sourceBodyHash").as_str() == Some(source_body_hash.as_str()),
        "RECEIPT_HASH",
    )?;
    let (data, numbers) = parse_number_lexemes(get(receipt, "bodyText").as_str().unwrap_or(""))?;
    Ok(Raw {
        data,
        numbers,
        start,
        end,
        source_body_hash,
        request_started_at: get(receipt, "requestStartedAt").clone(),
        received_at: get(receipt, "receivedAt").clone(),
    })
}

struct Leg {
    market_id: String,
    exchange_id: String,
    limit: i128,
    raw_no_ask: i128,
    depth: i64,
    source: Raw,
    book: V,
    yes_bid_level: V,
    used: Vec<V>,
}

/// consume(leg, q) — walk the unique bid curve for a q-share buy; the
/// candidate's limit is one tick over the worst consumed level.
fn consume(leg: &Leg, q: i64) -> Result<Option<(i128, i128, Vec<V>)>, String> {
    let mut left = q;
    let mut worst: Option<i128> = None;
    let mut displayed: i128 = 0;
    let mut used: Vec<V> = Vec::new();
    for (index, level) in get(&leg.book, "bids")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let qty = js_int(get(level, "quantity")).unwrap_or(0);
        let take = left.min(qty);
        if take != 0 {
            let w = SCALE - str_price(&json!(js_stringify(get(level, "price"))))?;
            worst = Some(w);
            displayed += w * take as i128;
            used.push(json!({
                "index": index,
                "quantity": take,
                "yesBidPrice": get(level, "price"),
            }));
            left -= take;
        }
        if left == 0 {
            break;
        }
    }
    if left > 0 {
        return Ok(None);
    }
    let limit = worst.unwrap_or(0) + TICK;
    if limit > SCALE - TICK {
        return Ok(None);
    }
    let _ = displayed;
    Ok(Some((worst.unwrap_or(0), limit, used)))
}

fn plan_inner(input: &V) -> Result<V, String> {
    let scope = get(input, "scope");
    let a = get(input, "allocation");
    let account = get(input, "account");
    let g = get(input, "accountRisk");
    let root_receipts = get(input, "rootReceipts");
    let book_receipts = get(input, "bookReceipts");
    let now_v = get(input, "now");
    let at = str_instant(now_v)?;
    need(
        sget(scope, "ownerId") == Some("Codex")
            && sget(scope, "runId").is_some_and(|s| !s.is_empty())
            && sget(scope, "tournamentId").is_some_and(|s| !s.is_empty())
            && sget(scope, "tournamentSlug").is_some_and(|s| {
                !s.is_empty()
                    && s.split('-').all(|p| {
                        !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                    })
            }),
        "SCOPE",
    )?;
    need(
        js_int(get(a, "schemaVersion")) == Some(1)
            && get(a, "ownerId") == get(scope, "ownerId")
            && get(a, "runId") == get(scope, "runId")
            && get(a, "tournamentId") == get(scope, "tournamentId")
            && get(a, "capital") == &json!("10000.000000")
            && hex(get(a, "humanAllocationSourceHash"))
            && str_instant(get(a, "recordedAt"))? <= at,
        "HUMAN_ALLOCATION",
    )?;
    let opt = |v: &V, k: &str, d: &str| -> V {
        match v.get(k) {
            Some(x) if !x.is_null() => x.clone(),
            _ => json!(d),
        }
    };
    let order_cap = str_amount(&opt(a, "maximumOrderCost", "25.000000"))?;
    let market_cap = str_amount(&opt(a, "maximumMarketCost", "100.000000"))?;
    let event_cap = str_amount(&opt(a, "maximumEventCost", "200.000000"))?;
    let share_cap = match a.get("maximumSharesPerOrder") {
        Some(x) if !x.is_null() => js_int(x),
        _ => Some(100),
    };
    need(
        order_cap > 0
            && order_cap <= 100000000
            && market_cap > 0
            && market_cap <= 1000000000
            && event_cap > 0
            && event_cap <= 2000000000
            && share_cap.is_some_and(|s| (1..=100).contains(&s)),
        "CAPITAL_CEILINGS",
    )?;
    let share_cap = share_cap.unwrap();
    need(
        get(g, "ownerId") == get(scope, "ownerId")
            && get(g, "runId") == get(scope, "runId")
            && get(g, "tournamentId") == get(scope, "tournamentId")
            && get(g, "complete") == &V::Bool(true)
            && get(g, "eventScopeComplete") == &V::Bool(true)
            && get(g, "unknownAcceptance") == &V::Bool(false)
            && get(g, "unresolvedIntentIds")
                .as_array()
                .is_some_and(|x| x.is_empty())
            && get(g, "stopped") == &V::Bool(false)
            && get(g, "soleCollector") == &V::Bool(true)
            && hex(get(g, "sourceHash"))
            && str_instant(get(g, "at"))? <= at
            && at - str_instant(get(g, "at"))? <= 20000
            && ["owned", "reservations", "loans"]
                .iter()
                .all(|k| g.get(k).is_some_and(V::is_array)),
        "FRESH_RISK_OR_UNKNOWN",
    )?;
    need(
        root_receipts.as_array().is_some_and(|x| x.len() == 2)
            && book_receipts.as_array().is_some_and(|x| x.len() == 2),
        "TWO_ROOTS_AND_BOOKS",
    )?;
    let mut roots: Vec<V> = Vec::new();
    for c in root_receipts.as_array().unwrap() {
        need(id(get(c, "marketId")) && id(get(c, "exchangeId")), "ROOT_MEMBER")?;
        let r = raw(
            get(c, "receipt"),
            &format!(
                "/markets/{}/nodes?tournamentId={}",
                get(c, "marketId").as_str().unwrap(),
                sget(scope, "tournamentId").unwrap()
            ),
            at,
            300000,
        )?;
        let d = &r.data;
        let contexts = d.get("contexts").and_then(V::as_array);
        need(
            get(d, "market_id") == get(c, "marketId")
                && contexts.is_some_and(|x| x.len() == 1)
                && contexts
                    .map(|x| get(&x[0], "type").as_str())
                    == Some(Some("tournament"))
                && contexts
                    .and_then(|x| x[0].get("tournament"))
                    .and_then(|t| t.get("id"))
                    .and_then(V::as_str)
                    == sget(scope, "tournamentId"),
            "ROOT_SCOPE",
        )?;
        roots.push(json!({
            "marketId": get(c, "marketId"),
            "exchangeId": get(c, "exchangeId"),
            "tournamentId": get(scope, "tournamentId"),
            "option": "YES",
            "root": get(d, "root"),
            "rootHash": payload_hash(get(d, "root"))?,
            "sourceHash": r.source_body_hash,
            "requestStartedAt": r.request_started_at,
            "receivedAt": r.received_at,
            "knownAt": r.received_at,
        }));
    }
    need(
        roots
            .iter()
            .map(|r| get(r, "marketId").as_str().unwrap_or(""))
            .collect::<HashSet<_>>()
            .len()
            == 2
            && roots
                .iter()
                .map(|r| get(r, "exchangeId").as_str().unwrap_or(""))
                .collect::<HashSet<_>>()
                .len()
                == 2,
        "DISTINCT_CONTRACTS",
    )?;
    let semantic = screen_party_winner_no_pairs(&json!({
        "universe": {
            "tournamentId": get(scope, "tournamentId"),
            "exchanges": roots.iter().map(|r| json!({
                "marketId": get(r, "marketId"),
                "exchangeId": get(r, "exchangeId"),
                "option": get(r, "option"),
            })).collect::<Vec<V>>(),
        },
        "rootCaptures": roots,
        "quoteCaptures": [],
        "account": V::Null,
        "now": now_v,
    }))?;
    let pairs = get(&semantic, "pairs").as_array();
    need(
        js_int(get(&semantic, "semanticPairCount")) == Some(1)
            && pairs.is_some_and(|p| !p.is_empty())
            && js_truthy(pairs.and_then(|p| p[0].get("rootsCurrent"))),
        "OPPOSING_SAME_SINGLE_WINNER_RACE",
    )?;
    let event_id = pairs.unwrap()[0].get("eventKey").cloned().unwrap_or(V::Null);

    let mut legs: Vec<Leg> = Vec::new();
    for c in book_receipts.as_array().unwrap() {
        let member = roots.iter().find(|r| {
            get(r, "marketId") == get(c, "marketId") && get(r, "exchangeId") == get(c, "exchangeId")
        });
        need(member.is_some(), "BOOK_MEMBER")?;
        let r = raw(
            get(c, "receipt"),
            &format!(
                "/markets/{}/orderbook?tournamentId={}&depth=20",
                get(c, "marketId").as_str().unwrap_or(""),
                sget(scope, "tournamentId").unwrap()
            ),
            at,
            20000,
        )?;
        let d = &r.data;
        let contexts = d.get("contexts").and_then(V::as_array);
        need(
            contexts.is_some_and(|x| x.len() == 1)
                && contexts.map(|x| get(&x[0], "type").as_str()) == Some(Some("tournament"))
                && contexts
                    .and_then(|x| x[0].get("tournament"))
                    .and_then(|t| t.get("id"))
                    .and_then(V::as_str)
                    == sget(scope, "tournamentId")
                && roots.iter().all(|v| {
                    str_instant(get(v, "receivedAt")).map(|t| t <= r.start).unwrap_or(false)
                }),
            "RULES_BEFORE_EXACT_BOOK",
        )?;
        let b = normalize_book(
            d,
            get(c, "marketId").as_str().unwrap_or(""),
            sget(scope, "tournamentId").unwrap(),
            r.received_at.as_str().unwrap_or(""),
            r.request_started_at.as_str().unwrap_or(""),
            20000.0,
            &BTreeSet::new(),
            "authoritative_receipt_v1",
        )?;
        need(
            get(&b, "exchangeId") == get(c, "exchangeId")
                && get(&b, "bids").as_array().is_some_and(|x| x.len() <= 20)
                && get(&b, "asks").as_array().is_some_and(|x| x.len() <= 20),
            "BOOK_DEPTH20",
        )?;
        let bids = contexts
            .and_then(|x| x[0].get("orderbook"))
            .and_then(|o| o.get("exchanges"))
            .and_then(V::as_array)
            .and_then(|e| e.first())
            .and_then(|e| e.get("bids"))
            .and_then(V::as_array)
            .cloned()
            .ok_or_else(|| "INVALID".to_string())?;
        for (i, l) in bids.iter().enumerate() {
            let q = r.numbers.get(&num_key(&[
                json!("contexts"),
                json!(0),
                json!("orderbook"),
                json!("exchanges"),
                json!(0),
                json!("bids"),
                json!(i),
                json!("quantity"),
            ]));
            let p = r.numbers.get(&num_key(&[
                json!("contexts"),
                json!(0),
                json!("orderbook"),
                json!("exchanges"),
                json!(0),
                json!("bids"),
                json!(i),
                json!("price"),
            ]));
            let qv = js_int(get(l, "quantity")).ok_or_else(|| "INVALID".to_string())?;
            need(
                q.is_some_and(|q| {
                    q.len() <= 128
                        && q.bytes().filter(|b| *b == b'.').count() <= 1
                        && {
                            let int_part = q.split('.').next().unwrap_or("");
                            !int_part.is_empty()
                                && !int_part.starts_with('0')
                                && int_part.bytes().all(|b| b.is_ascii_digit())
                                && q.split('.').nth(1).is_none_or(|f| {
                                    !f.is_empty() && f.bytes().all(|b| b == b'0')
                                })
                        }
                        && lexeme_int_eq(q, qv)
                }),
                "EXACT_DEPTH_AND_PRICE_LEXEMES",
            )?;
            // JS evaluates `price(p)` inside the need expression: an absent or
            // non-string lexeme throws a non-entry error before the
            // comparison can fail, so reason stays INVALID_OR_INCOMPLETE.
            let px = p
                .ok_or_else(|| "INVALID_PRICE".to_string())
                .and_then(|p| price(p))?;
            let lpx = str_price(&json!(js_stringify(get(l, "price"))))?;
            need(px == lpx, "EXACT_DEPTH_AND_PRICE_LEXEMES")?;
        }
        let bids_norm = get(&b, "bids").as_array().cloned().unwrap_or_default();
        need(!bids_norm.is_empty(), "NO_DISPLAYED_DEPTH")?;
        let mut previous = SCALE;
        for l in &bids_norm {
            let px = str_price(&json!(js_stringify(get(l, "price"))))?;
            need(px < previous, "STRICT_UNIQUE_BID_CURVE")?;
            previous = px;
        }
        let level = bids_norm[0].clone();
        let raw_no_ask = SCALE - str_price(&json!(js_stringify(get(&level, "price"))))?;
        let limit = raw_no_ask + TICK;
        str_price(&json!(money(limit)))?;
        let total_qty: i128 = bids_norm
            .iter()
            .map(|l| js_int(get(l, "quantity")).unwrap_or(0) as i128)
            .sum();
        legs.push(Leg {
            market_id: get(c, "marketId").as_str().unwrap().to_string(),
            exchange_id: get(c, "exchangeId").as_str().unwrap().to_string(),
            limit,
            raw_no_ask,
            depth: if total_qty > 100 { 100 } else { total_qty as i64 },
            source: r,
            book: b,
            yes_bid_level: level,
            used: Vec::new(),
        });
    }
    need(
        legs.iter().map(|l| l.exchange_id.clone()).collect::<HashSet<_>>().len() == 2
            && legs.iter().map(|l| l.source.end).max().unwrap()
                - legs.iter().map(|l| l.source.start).min().unwrap()
                <= 10000,
        "COFRESH_DISTINCT_BOOKS",
    )?;
    legs.sort_by(|x, y| {
        x.limit
            .cmp(&y.limit)
            .then_with(|| {
                x.exchange_id
                    .parse::<i64>()
                    .unwrap_or(0)
                    .cmp(&y.exchange_id.parse::<i64>().unwrap_or(0))
            })
    });
    let first_limit = legs[0].limit;
    let second_limit = legs[1].limit;
    let mut total = first_limit + second_limit;
    need(total <= 990000, "MINIMUM_PROTECTIVE_PACKAGE_GAP")?;

    let scoped = get(account, "scopedOrdersPagesByExchange")
        .get(&legs[0].exchange_id)
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or_else(|| get(account, "scopedOrdersPages").clone());
    let full = json!({
        "cash": get(account, "cash"),
        "positions": get(account, "positions"),
        "openOrdersPages": get(account, "openOrdersPages"),
        "scopedOrdersPages": scoped,
    });
    let mut binding = scope.as_object().cloned().unwrap_or_default();
    binding.insert("exchangeId".into(), json!(legs[0].exchange_id));
    let current = inspect_scoped_rest_no_account(&json!({
        "account": full,
        "binding": V::Object(binding),
        "readAt": now_v,
    }))?;
    let raw_positions =
        parse_number_lexemes(get(get(account, "positions"), "bodyText").as_str().unwrap_or(""))?;
    for (i, p) in get(&raw_positions.0, "positions")
        .as_array()
        .ok_or_else(|| "INVALID".to_string())?
        .iter()
        .enumerate()
    {
        let q = raw_positions
            .1
            .get(&num_key(&[json!("positions"), json!(i), json!("quantity")]));
        let pv = js_int(get(p, "quantity")).ok_or_else(|| "INVALID".to_string())?;
        need(
            q.is_some_and(|q| {
                q.len() <= 128
                    && q.bytes().filter(|b| *b == b'.').count() <= 1
                    && {
                        let t = q.strip_prefix('-').unwrap_or(q);
                        let int_part = t.split('.').next().unwrap_or("");
                        !int_part.is_empty()
                            && (!int_part.starts_with('0') || int_part == "0")
                            && int_part.bytes().all(|b| b.is_ascii_digit())
                            && q.split('.').nth(1).is_none_or(|f| {
                                !f.is_empty() && f.bytes().all(|b| b == b'0')
                            })
                    }
                    && lexeme_int_eq(q, pv)
            }),
            "EXACT_ACCOUNT_POSITION_QUANTITY",
        )?;
    }
    let min_book_start = legs.iter().map(|l| l.source.start).min().unwrap();
    let cur_positions = get(&current, "positions")
        .as_array()
        .ok_or_else(|| "INVALID".to_string())?;
    need(
        !cur_positions
            .iter()
            .any(|p| {
                legs.iter().any(|l| get(p, "exchangeId").as_str() == Some(l.exchange_id.as_str()))
                    && get(p, "quantity").as_f64() != Some(0.0)
            })
            && str_instant(get(&current, "completedAt"))? <= min_book_start
            && str_instant(get(g, "at"))? >= str_instant(get(&current, "completedAt"))?
            && str_instant(get(g, "at"))? <= min_book_start,
        "WHOLE_ACCOUNT_TARGET_FLAT_AND_CAUSAL",
    )?;

    let mut nominal: i128 = 0;
    let mut event_exposure: i128 = 0;
    let mut markets: std::collections::HashMap<String, i128> = legs
        .iter()
        .map(|l| (l.market_id.clone(), 0i128))
        .collect();
    for (k, field) in [
        ("owned", "nominalCost"),
        ("reservations", "maximumNominalCost"),
        ("loans", "fullNominalPrincipal"),
    ] {
        for e in get(g, k).as_array().cloned().unwrap_or_default() {
            need(
                !e.is_null()
                    && id(get(&e, "marketId"))
                    && sget(&e, "eventId").is_some_and(|s| !s.is_empty())
                    && sget(&e, "exposureId").is_some_and(|s| !s.is_empty())
                    && get(&e, "unknownAcceptance") != &V::Bool(true),
                "EXACT_NOMINAL_EXPOSURE",
            )?;
            let n = str_amount(get(&e, field))?;
            need(
                n >= 0
                    && (!markets.contains_key(get(&e, "marketId").as_str().unwrap_or(""))
                        || get(&e, "eventId") == &event_id),
                "EVENT_SCOPE",
            )?;
            nominal += n;
            if get(&e, "eventId") == &event_id {
                event_exposure += n;
            }
            if let Some(v) = markets.get_mut(get(&e, "marketId").as_str().unwrap_or("")) {
                *v += n;
            }
        }
    }
    let cash_lower = str_amount(get(&current, "cashLower"))?;
    let cash = std::cmp::min(
        cash_lower,
        if g.get("cashLower").is_none() {
            cash_lower
        } else {
            str_amount(get(g, "cashLower"))?
        },
    );
    let capital_remaining = 10000000000i128 - nominal;
    let event_remaining = event_cap - event_exposure;
    need(cash >= 0, "CASH_LOWER")?;
    let first_remaining = market_cap - markets.get(&legs[0].market_id).copied().unwrap_or(0);
    let second_remaining = market_cap - markets.get(&legs[1].market_id).copied().unwrap_or(0);
    let mut depth_candidates: Vec<(i64, [(i128, i128, Vec<V>); 2], i128, i128)> = Vec::new();
    for q in 1..=share_cap {
        let c0 = consume(&legs[0], q)?;
        let c1 = consume(&legs[1], q)?;
        let (c0, c1) = match (c0, c1) {
            (Some(a), Some(b)) => (a, b),
            _ => continue,
        };
        let consumed = [c0, c1];
        let cost = [consumed[0].1 * q as i128, consumed[1].1 * q as i128];
        let unit = consumed[0].1 + consumed[1].1;
        let reservation = 990000i128 * q as i128;
        if unit > 990000
            || cost.iter().any(|x| *x > order_cap)
            || cost[0] > first_remaining
            || cost[1] > second_remaining
            || reservation > capital_remaining
            || reservation > event_remaining
            || reservation > cash
        {
            continue;
        }
        let margin = (SCALE - unit) * q as i128;
        depth_candidates.push((q, consumed, margin, unit));
    }
    depth_candidates.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    let quantity = depth_candidates.first().map(|c| c.0).unwrap_or(0);
    if let Some(chosen) = depth_candidates.first() {
        // Object.assign(l, chosen.consumed[i]); yesBidLevel = last used bid level.
        for (i, l) in legs.iter_mut().enumerate() {
            l.raw_no_ask = chosen.1[i].0;
            l.limit = chosen.1[i].1;
            l.used = chosen.1[i].2.clone();
            let idx = l
                .used
                .last()
                .and_then(|u| get(u, "index").as_u64())
                .unwrap_or(0) as usize;
            l.yes_bid_level = get(&l.book, "bids")
                .as_array()
                .and_then(|a| a.get(idx).cloned())
                .unwrap_or(V::Null);
        }
        total = chosen.3;
    }

    let mut risk = Map::new();
    risk.insert("nominalExposure".into(), json!(money(nominal)));
    risk.insert("eventNominalExposure".into(), json!(money(event_exposure)));
    risk.insert("capitalRemaining".into(), json!(money(capital_remaining)));
    risk.insert("eventRemaining".into(), json!(money(event_remaining)));
    risk.insert("firstMarketRemaining".into(), json!(money(first_remaining)));
    risk.insert("secondMarketRemaining".into(), json!(money(second_remaining)));
    risk.insert("cashLower".into(), json!(money(cash)));
    risk.insert("fullNominalLoansAndReservationsCounted".into(), json!(true));
    risk.insert("loanAdvanceDiscount".into(), json!(false));
    let risk = V::Object(risk);
    if quantity < 1 {
        let mut out = base_map(now_v);
        out.insert("reason".into(), json!("NO_FULL_PACKAGE_FUNDED_INTEGER_QUANTITY"));
        out.insert("risk".into(), risk);
        return Ok(V::Object(out));
    }
    let expires_at = ms_to_iso(
        roots
            .iter()
            .map(|r| str_instant(get(r, "receivedAt")).unwrap_or(i64::MAX) + 300000)
            .chain(legs.iter().map(|l| l.source.start + 20000))
            .chain(std::iter::once(
                str_instant(get(&current, "startedAt")).unwrap_or(i64::MAX) + 20000,
            ))
            .min()
            .unwrap() as f64,
    );
    need(str_instant(&json!(expires_at.clone()))? > at, "EXPIRED_ENTRY_SOURCE")?;
    let public_leg = |l: &Leg, quantity: i64| -> V {
        let mut p = Map::new();
        p.insert("marketId".into(), json!(l.market_id));
        p.insert("exchangeId".into(), json!(l.exchange_id));
        p.insert("side".into(), json!("no"));
        p.insert("limit".into(), json!(money(l.limit)));
        p.insert("rawObservedNoAsk".into(), json!(money(l.raw_no_ask)));
        p.insert("maximumQuantity".into(), json!(quantity));
        p.insert("observedDepthQuantityWithinShareCap".into(), json!(l.depth));
        p.insert(
            "consumedBidLevels".into(),
            json!(l.used),
        );
        p.insert("yesBidSourceLevel".into(), l.yes_bid_level.clone());
        p.insert(
            "maximumObservedNotional".into(),
            json!(money(l.limit * quantity as i128)),
        );
        p.insert("bookVersion".into(), get(&l.book, "versionKey").clone());
        V::Object(p)
    };
    let mut residual = Map::new();
    residual.insert("kind".into(), json!("conditional_two_no_relative_value_gap"));
    residual.insert("eventId".into(), event_id.clone());
    residual.insert("protectiveUnitCost".into(), json!(money(total)));
    residual.insert("ordinaryPayoutFloor".into(), json!("1.000000"));
    residual.insert("conditionalGap".into(), json!(money(SCALE - total)));
    residual.insert(
        "rootHashes".into(),
        json!(roots.iter().map(|r| get(r, "rootHash").clone()).collect::<Vec<V>>()),
    );
    residual.insert(
        "bookHashes".into(),
        json!(legs
            .iter()
            .map(|l| json!(l.source.source_body_hash))
            .collect::<Vec<V>>()),
    );
    let residual = V::Object(residual);

    let mut proposal = base_map(now_v);
    let mut put = |m: &mut Map<String, V>, k: &str, v: V| {
        m.insert(k.into(), v);
    };
    proposal.insert("proposalReady".into(), json!(true));
    put(&mut proposal, "reason", json!("FUNDED_CONDITIONAL_PACKAGE_GAP_CHEAP_FIRST_PROPOSAL"));
    put(&mut proposal, "ownerId", get(scope, "ownerId").clone());
    put(&mut proposal, "runId", get(scope, "runId").clone());
    put(&mut proposal, "tournamentId", get(scope, "tournamentId").clone());
    put(&mut proposal, "tournamentSlug", get(scope, "tournamentSlug").clone());
    put(&mut proposal, "eventId", event_id.clone());
    put(&mut proposal, "allocationHash", json!(payload_hash(a)?));
    put(&mut proposal, "riskHash", json!(payload_hash(g)?));
    put(
        &mut proposal,
        "legs",
        json!(legs.iter().map(|l| public_leg(l, quantity)).collect::<Vec<V>>()),
    );
    let mut chosen = Map::new();
    chosen.insert("marketId".into(), json!(legs[0].market_id));
    chosen.insert("exchangeId".into(), json!(legs[0].exchange_id));
    chosen.insert("side".into(), json!("no"));
    chosen.insert("action".into(), json!("buy"));
    chosen.insert("quantity".into(), json!(quantity));
    chosen.insert("limit".into(), json!(money(legs[0].limit)));
    put(&mut proposal, "chosenLeg", V::Object(chosen));
    put(
        &mut proposal,
        "fullPackageReservation",
        json!(money(990000i128 * quantity as i128)),
    );
    put(&mut proposal, "unitPackageCeiling", json!("0.990000"));
    put(&mut proposal, "minimumOrdinaryMargin", json!("0.010000"));
    put(&mut proposal, "observedProtectivePackageUnitCost", json!(money(total)));
    put(&mut proposal, "conditionalOrdinaryGap", json!(money(SCALE - total)));
    put(&mut proposal, "conditionalOrdinaryFloor", json!("1.000000"));
    put(&mut proposal, "unconditionalFloor", V::Null);
    put(
        &mut proposal,
        "exceptionLossAtFullReservation",
        json!(money(-990000i128 * quantity as i128)),
    );
    put(
        &mut proposal,
        "maximumFirstOrderNotional",
        json!(money(legs[0].limit * quantity as i128)),
    );
    put(
        &mut proposal,
        "maximumFutureHedgeOrderNotional",
        json!(money(std::cmp::min(order_cap, second_remaining))),
    );
    put(&mut proposal, "futureHedgeLimitRule", json!("min(0.990-actual_first_unit_cost, tick_floor(max_order_notional/remaining_quantity), tick_floor(fresh_market_remaining/remaining_quantity))"));
    put(&mut proposal, "futureHedgeAutomaticallyAuthorized", json!(false));
    put(&mut proposal, "sellTargetRule", json!("actual_verified_unit_cost + 0.005 with fresh protected net exit/depth and separate authority"));
    put(&mut proposal, "valuationKind", get(&residual, "kind").clone());
    put(&mut proposal, "valuationHash", json!(payload_hash(&residual)?));
    put(&mut proposal, "risk", risk);
    put(&mut proposal, "expiresAt", json!(expires_at));
    let mut receipts = Map::new();
    receipts.insert(
        "roots".into(),
        json!(roots
            .iter()
            .map(|r| {
                json!({
                    "marketId": get(r, "marketId"),
                    "exchangeId": get(r, "exchangeId"),
                    "rootHash": get(r, "rootHash"),
                    "sourceBodyHash": get(r, "sourceHash"),
                    "requestStartedAt": get(r, "requestStartedAt"),
                    "receivedAt": get(r, "receivedAt"),
                })
            })
            .collect::<Vec<V>>()),
    );
    receipts.insert(
        "books".into(),
        json!(legs
            .iter()
            .map(|l| {
                json!({
                    "marketId": l.market_id,
                    "exchangeId": l.exchange_id,
                    "sourceBodyHash": l.source.source_body_hash,
                    "requestStartedAt": l.source.request_started_at,
                    "receivedAt": l.source.received_at,
                    "versionKey": get(&l.book, "versionKey"),
                })
            })
            .collect::<Vec<V>>()),
    );
    receipts.insert("account".into(), get(&current, "sourceReceipts").clone());
    put(&mut proposal, "sourceReceipts", V::Object(receipts));
    put(&mut proposal, "fees", json!("0.000000"));
    put(&mut proposal, "depthCombinedAcrossLevels", json!(true));
    put(&mut proposal, "depthPolicy", json!("full_observed_unique_bid_curve_at_protected_worst_limit"));
    put(&mut proposal, "objective", json!("maximum_conditional_ordinary_package_dollar_margin"));
    put(&mut proposal, "minimumFirstLotCorpusProfitabilityProven", json!(false));
    put(
        &mut proposal,
        "assumptions",
        json!([
            "Opposing same-race Party Winner NO/NO has only a conditional ordinary-resolution floor; separate contracts are not exhaustive complements.",
            "The first leg is directional until a independently funded/verified hedge fills; cheap is relative to the package, not a fair point probability.",
            "Administrative, void/refund, affiliation or inconsistent-resolution exceptions can lose the full package cost.",
            "Observed gaps need not revert on a short horizon; one original profitable share does not prove scaled profitability.",
            "No entry window, order, automatic hedge or guaranteed sale is authorized by this proposal."
        ]),
    );
    let proposal_v = V::Object(proposal);
    let hash = payload_hash(&proposal_v)?;
    let mut out = proposal_v.as_object().unwrap().clone();
    out.insert("proposalHash".into(), json!(hash));
    Ok(V::Object(out))
}

fn base_map(now: &V) -> Map<String, V> {
    let mut m = Map::new();
    m.insert("schemaVersion".into(), json!(1));
    m.insert("mode".into(), json!("baal_scaled_depth_swept_conditional_party_winner_entry_v2"));
    m.insert("at".into(), now.clone());
    m.insert("proposalReady".into(), json!(false));
    m.insert("orderEligible".into(), json!(false));
    m.insert("executionAuthority".into(), json!(false));
    m.insert("networkRequests".into(), json!(0));
    m.insert("competitionOrders".into(), json!(0));
    m.insert("profitabilityEstablished".into(), json!(false));
    m.insert("priorOneShareTradeProvesScale".into(), json!(false));
    m
}

pub fn plan_baal_scaled_entry_v3(input: &V) -> V {
    let now = get(input, "now").clone();
    match plan_inner(input) {
        Ok(v) => v,
        Err(e) => {
            let mut out = base_map(&now);
            out.insert(
                "reason".into(),
                json!(if is_entry_code(&e) {
                    e
                } else {
                    "INVALID_OR_INCOMPLETE_SCALED_ENTRY_EVIDENCE".to_string()
                }),
            );
            V::Object(out)
        }
    }
}
