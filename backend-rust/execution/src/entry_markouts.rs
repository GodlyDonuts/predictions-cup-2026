// execution/entry-markouts.mjs — attributed buy-fill bid markouts over verified
// subsequent book receipts. Entry diagnostic only; never portfolio P&L.
use baal_service_runtime::contract::{instant_ns, ms_to_iso, timestamp};
use baal_service_runtime::fixed::{amount, money, price};
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Map, Value as V};
use std::collections::{HashMap, HashSet};

/// ENTRY_MARKOUT_HORIZONS_MINUTES.
pub const ENTRY_MARKOUT_HORIZONS_MINUTES: [i64; 4] = [5, 15, 30, 60];
const MS: i128 = 1_000_000;
const MINUTE: i128 = 60_000 * MS;
fn check(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.to_string()) }
}
fn plain(v: &V) -> bool {
    v.is_object()
}
// /^[1-9]\d*$/ && Number.isSafeInteger(Number(x))
fn id(v: &V) -> bool {
    match v.as_str() {
        Some(s) if !s.is_empty() && s.as_bytes()[0] != b'0' && s.bytes().all(|b| b.is_ascii_digit()) => {
            s.parse::<i64>().map(|n| n > 0).unwrap_or(false) && s.len() <= 15 || s.parse::<u64>().map(|n| n <= 9_007_199_254_740_991).unwrap_or(false)
        }
        _ => false,
    }
}
fn quantity(v: &V) -> bool {
    baal_service_runtime::json_text::js_int(v).is_some_and(|x| x > 0 && x <= 2_147_483_647)
}
// ns -> 'YYYY-MM-DDTHH:mm:ss.<9 digits>Z'
fn iso(ns: i128) -> String {
    let s = ms_to_iso((ns / MS) as f64);
    format!("{}.{:09}Z", &s[..19], ns % 1_000_000_000)
}
fn elapsed(a: i128, b: i128) -> f64 {
    (a - b) as f64 / 1e6
}
fn count(map: &mut Map<String, V>, key: &str) {
    let n = map.get(key).and_then(V::as_i64).unwrap_or(0);
    map.insert(key.to_string(), json!(n + 1));
}
fn num(x: f64) -> V {
    if x.is_finite() && x.fract() == 0.0 && x.abs() <= 9.0e15 {
        json!(x as i64)
    } else {
        json!(x)
    }
}
// Book price level: micros price + share count.
#[derive(Clone)]
struct Level {
    price: i128,
    quantity: i64,
}
fn levels(rows: &V, descending: bool) -> Result<Vec<Level>, String> {
    let rows = rows.as_array().filter(|r| !r.is_empty() && r.len() <= 100000);
    check(rows.is_some(), "INVALID_BOOK_DEPTH")?;
    let rows = rows.unwrap();
    let mut prior: Option<i128> = None;
    let mut total: i64 = 0;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        check(plain(row) && quantity(row.get("quantity").unwrap_or(&V::Null)), "INVALID_BOOK_DEPTH")?;
        let q = row["quantity"].as_i64().unwrap_or(0);
        let p = price(row.get("price").and_then(V::as_str).unwrap_or(""))?;
        check(prior.is_none() || (if descending { p < prior.unwrap() } else { p > prior.unwrap() }), "INVALID_BOOK_DEPTH")?;
        prior = Some(p);
        total += q;
        check(total <= 9_007_199_254_740_991, "INVALID_BOOK_DEPTH")?;
        out.push(Level { price: p, quantity: q });
    }
    Ok(out)
}
struct Checked {
    started: i128,
    source: i128,
    sequence: i128,
    bids: Vec<Level>,
    asks: Vec<Level>,
    content: String,
}
fn inst(v: &V) -> Result<i128, String> {
    instant_ns(v.as_str().unwrap_or("\u{0}")).map_err(|e| e.0)
}
fn inspect(book: &V, market_id: &V, exchange_id: &V, entry_at: i128, tournament_id: &str, received: i128, now_ns: i128, max_book_latency_ms: i64) -> Result<Checked, String> {
    check(
        book.get("tournamentId").and_then(V::as_str) == Some(tournament_id)
            && book.get("marketId") == Some(market_id)
            && book.get("exchangeId") == Some(exchange_id)
            && book.get("instrument").and_then(V::as_str) == Some(format!("{}:YES", exchange_id.as_str().unwrap_or("")).as_str()),
        "BOOK_SCOPE_MISMATCH",
    )?;
    check(
        book.get("freshnessPolicy").and_then(V::as_str) == Some("authoritative_receipt_v1")
            && !js_truthy(book.get("invalidated"))
            && book.get("valid") != Some(&V::Bool(false)),
        "BOOK_NOT_AUTHORITATIVE",
    )?;
    let started = inst(book.get("requestStartedAt").unwrap_or(&V::Null))?;
    let source = inst(book.get("sourceAt").unwrap_or(&V::Null))?;
    check(started <= received && received - started <= max_book_latency_ms as i128 * MS, "INVALID_BOOK_LATENCY")?;
    // A response to a request started before this fill is not a subsequent mark.
    check(started > entry_at, "REQUEST_NOT_AFTER_FILL")?;
    check(source <= now_ns && source <= received + 2000 * MS, "FUTURE_BOOK_SOURCE")?;
    let sequence = book.get("sequence").and_then(V::as_str).unwrap_or("");
    let sequence_ok = (sequence == "0" || (!sequence.is_empty() && !sequence.starts_with('0') && sequence.bytes().all(|b| b.is_ascii_digit())))
        && sequence.parse::<u64>().map(|n| n <= 9_007_199_254_740_991).unwrap_or(false);
    let ts = timestamp(book.get("sourceAt").unwrap_or(&V::Null)).map_err(|e| e.0)?;
    check(
        sequence_ok && book.get("versionKey").and_then(V::as_str) == Some(format!("{sequence}:{ts}").as_str()),
        "INVALID_BOOK_VERSION",
    )?;
    let bids = levels(book.get("bids").unwrap_or(&V::Null), true)?;
    let asks = levels(book.get("asks").unwrap_or(&V::Null), false)?;
    check(bids[0].price < asks[0].price, "LOCKED_OR_CROSSED_BOOK")?;
    // JSON.stringify([bids, asks], bigint -> decimal string)
    let body: Vec<V> = [&bids, &asks]
        .iter()
        .map(|lv| {
            V::Array(
                lv.iter()
                    .map(|l| json!({"price": l.price.to_string(), "quantity": l.quantity}))
                    .collect(),
            )
        })
        .collect();
    let content = baal_service_runtime::json_text::js_stringify(&V::Array(body));
    Ok(Checked {
        started,
        source,
        sequence: sequence.parse::<i128>().unwrap_or(0),
        bids,
        asks,
        content,
    })
}
struct Entry {
    quantity: i64,
    at: i128,
    price_micros: i128,
    cost: i128,
}
fn mark(row_checked: &Checked, entry: &Entry) -> Map<String, V> {
    let mut remaining = entry.quantity;
    let mut proceeds: i128 = 0;
    let mut segments = Vec::new();
    for level in &row_checked.bids {
        let q = remaining.min(level.quantity);
        if q != 0 {
            proceeds += level.price * q as i128;
            segments.push(json!({"price": money(level.price), "quantity": q}));
            remaining -= q;
        }
        if remaining == 0 {
            break;
        }
    }
    let mut m = Map::new();
    m.insert("reasonCode".into(), json!(if remaining != 0 { "INSUFFICIENT_BID_DEPTH" } else { "COMPLETE" }));
    m.insert("observedQuantity".into(), json!(entry.quantity - remaining));
    m.insert("unsoldQuantity".into(), json!(remaining));
    m.insert("observedAvailableDepthValue".into(), json!(money(proceeds)));
    m.insert("observedLiquidationValue".into(), if remaining != 0 { V::Null } else { json!(money(proceeds)) });
    m.insert("entryMarkout".into(), if remaining != 0 { V::Null } else { json!(money(proceeds - entry.cost)) });
    m.insert("segments".into(), V::Array(segments));
    m
}
fn evidence(book: &V) -> V {
    json!({
        "receivedAt": book.get("receivedAt"),
        "requestStartedAt": book.get("requestStartedAt"),
        "sourceAt": book.get("sourceAt"),
        "versionKey": book.get("versionKey"),
    })
}
struct Row {
    book: V,
    received: i128,
    checked: Option<Checked>,
    reason_code: String,
}
fn uuid_tournament(s: &str) -> bool {
    // /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i
    let hex = |c: u8| c.is_ascii_hexdigit();
    let b = s.as_bytes();
    s.len() == 36
        && b[8] == b'-' && b[13] == b'-' && b[18] == b'-' && b[23] == b'-'
        && b[..8].iter().all(|c| hex(*c))
        && b[9..13].iter().all(|c| hex(*c))
        && b[14..18].iter().all(|c| hex(*c))
        && b[19..23].iter().all(|c| hex(*c))
        && b[24..36].iter().all(|c| hex(*c))
}
/// analyzeEntryMarkouts({tournamentId, fill, books, now, takeProfit, stopLoss, ...})
pub fn analyze_entry_markouts(input: &V) -> Result<V, String> {
    let tournament_id = input.get("tournamentId").and_then(V::as_str);
    check(tournament_id.is_some_and(uuid_tournament), "INVALID_MARKOUT_SCOPE")?;
    let tournament_id = tournament_id.unwrap();
    let now = baal_service_runtime::json_text::js_int(input.get("now").unwrap_or(&V::Null));
    check(now.is_some_and(|n| (0..=8_640_000_000_000_000).contains(&n)), "INVALID_MARKOUT_CLOCK")?;
    let now = now.unwrap();
    let max_horizon_delay_ms = baal_service_runtime::json_text::js_int(input.get("maxHorizonDelayMs").unwrap_or(&json!(60000))).unwrap_or(-1);
    let max_book_latency_ms = baal_service_runtime::json_text::js_int(input.get("maxBookLatencyMs").unwrap_or(&json!(20000))).unwrap_or(-1);
    check(
        (0..=60000).contains(&max_horizon_delay_ms) && (1..=20000).contains(&max_book_latency_ms),
        "INVALID_MARKOUT_POLICY",
    )?;
    let fill = input.get("fill").unwrap_or(&V::Null);
    check(
        plain(fill)
            && id(fill.get("marketId").unwrap_or(&V::Null))
            && id(fill.get("exchangeId").unwrap_or(&V::Null))
            && fill.get("action").and_then(V::as_str) == Some("buy")
            && (fill.get("side").is_none() || fill.get("side").and_then(V::as_str) == Some("yes"))
            && (fill.get("tournamentId").is_none() || fill.get("tournamentId").and_then(V::as_str) == Some(tournament_id))
            && baal_service_runtime::json_text::js_int(fill.get("id").unwrap_or(&V::Null)).is_some_and(|x| x > 0)
            && baal_service_runtime::json_text::js_int(fill.get("orderId").unwrap_or(&V::Null)).is_some_and(|x| x > 0)
            && quantity(fill.get("quantity").unwrap_or(&V::Null)),
        "INVALID_ATTRIBUTED_ENTRY_FILL",
    )?;
    let now_ns = now as i128 * MS;
    let entry = Entry {
        quantity: fill["quantity"].as_i64().unwrap(),
        at: inst(fill.get("filledAt").unwrap_or(&V::Null))?,
        price_micros: price(fill.get("price").and_then(V::as_str).unwrap_or(""))?,
        cost: 0,
    };
    check(entry.at <= now_ns, "FUTURE_ENTRY_FILL")?;
    let entry = Entry { cost: entry.price_micros * entry.quantity as i128, ..entry };
    let profit = amount(input.get("takeProfit").and_then(V::as_str).unwrap_or("0.010000"))?;
    let loss = amount(input.get("stopLoss").and_then(V::as_str).unwrap_or("0.030000"))?;
    check(profit > 0 && profit < 1_000_000 && loss > 0 && loss < 1_000_000, "INVALID_MARKOUT_THRESHOLDS")?;
    let books = input.get("books").and_then(V::as_array);
    check(books.is_some_and(|b| b.len() <= 100000), "INVALID_MARKOUT_BOOKS")?;
    let books = books.cloned().unwrap_or_default();
    let policy = json!({
        "horizonsMinutes": ENTRY_MARKOUT_HORIZONS_MINUTES,
        "maxHorizonDelayMs": max_horizon_delay_ms,
        "maxBookLatencyMs": max_book_latency_ms,
        "maxSourceClockSkewMs": 2000,
        "takeProfit": money(profit),
        "stopLoss": money(loss),
        "horizonSelection": "first_receipt_at_or_after_target_no_skipping_invalid_or_thin_evidence",
        "passageWindowMinutes": 60,
        "addedFees": "0.000000",
    });
    let mut reason_counts: Map<String, V> = Map::new();
    let supplied_books = books.len() as i64;
    let mut unrelated_books = 0i64;
    let mut pre_fill = 0i64;
    let mut future = 0i64;
    let mut invalid_clocks = 0i64;
    let mut duplicates = 0i64;
    let mut valid_receipts = 0i64;
    let mut candidates: Vec<(V, i128)> = Vec::new();
    for book in &books {
        if !plain(book)
            || (book.get("exchangeId") != fill.get("exchangeId") && book.get("marketId") != fill.get("marketId"))
        {
            unrelated_books += 1;
            continue;
        }
        let received = match inst(book.get("receivedAt").unwrap_or(&V::Null)) {
            Ok(r) => r,
            Err(_) => {
                invalid_clocks += 1;
                continue;
            }
        };
        if received <= entry.at {
            pre_fill += 1;
            continue;
        }
        if received > now_ns {
            future += 1;
            continue;
        }
        candidates.push((book.clone(), received));
    }
    // Order by actual receipt, rejecting version rollback.
    candidates.sort_by(|a, b| a.1.cmp(&b.1));
    let mut rows: Vec<Row> = Vec::new();
    let mut versions: HashMap<String, String> = HashMap::new();
    let mut contents: HashSet<String> = HashSet::new();
    let mut previous: Option<(i128, i128, i128)> = None; // (sequence, source, started)
    let mut index = 0usize;
    while index < candidates.len() {
        let (book, received) = candidates[index].clone();
        index += 1;
        let mut group: Vec<(V, i128)> = Vec::new();
        while index < candidates.len() && candidates[index].1 == received {
            group.push(candidates[index].clone());
            index += 1;
        }
        duplicates += group.len() as i64;
        let mut row = Row { book: book.clone(), received, checked: None, reason_code: String::new() };
        let result: Result<(), String> = (|| {
            let checked = inspect(&book, &fill["marketId"], &fill["exchangeId"], entry.at, tournament_id, received, now_ns, max_book_latency_ms)?;
            for (dup, dup_received) in &group {
                let other = inspect(dup, &fill["marketId"], &fill["exchangeId"], entry.at, tournament_id, *dup_received, now_ns, max_book_latency_ms)?;
                check(
                    other.content == checked.content && other.started == checked.started
                        && dup.get("versionKey") == book.get("versionKey"),
                    "CONFLICTING_RECEIPT",
                )?;
            }
            check(
                previous.is_none() || (checked.sequence >= previous.unwrap().0 && checked.source >= previous.unwrap().1 && checked.started >= previous.unwrap().2),
                "BOOK_VERSION_OR_REQUEST_ROLLBACK",
            )?;
            check(
                previous.is_none() || checked.sequence != previous.unwrap().0 || checked.source == previous.unwrap().1,
                "BOOK_SEQUENCE_IDENTITY_CONFLICT",
            )?;
            let vk = book.get("versionKey").and_then(V::as_str).unwrap_or("").to_string();
            check(versions.get(&vk).map_or(true, |c| *c == checked.content), "BOOK_VERSION_CONTENT_CONFLICT")?;
            contents.insert(checked.content.clone());
            versions.insert(vk, checked.content.clone());
            previous = Some((checked.sequence, checked.source, checked.started));
            row.checked = Some(checked);
            row.reason_code = "VALID".into();
            valid_receipts += 1;
            Ok(())
        })();
        if let Err(e) = result {
            row.checked = None;
            // /^[A-Z][A-Z_]+$/ — at least two characters.
            let ok = e.len() >= 2 && e.as_bytes()[0].is_ascii_uppercase() && e.bytes().all(|b| b.is_ascii_uppercase() || b == b'_');
            row.reason_code = if ok { e } else { "INVALID_BOOK".into() };
        }
        count(&mut reason_counts, &row.reason_code.clone());
        rows.push(row);
    }
    let horizon_rows: Vec<V> = ENTRY_MARKOUT_HORIZONS_MINUTES
        .iter()
        .map(|&minutes| {
            let target = entry.at + minutes as i128 * MINUTE;
            let row = rows.iter().find(|x| x.received >= target);
            let mut out = Map::new();
            out.insert("minutes".into(), json!(minutes));
            out.insert("targetAt".into(), json!(iso(target)));
            out.insert("available".into(), json!(false));
            out.insert("reasonCode".into(), json!("PENDING_HORIZON"));
            out.insert("evidence".into(), V::Null);
            out.insert("delayMs".into(), V::Null);
            out.insert("observedQuantity".into(), json!(0));
            out.insert("unsoldQuantity".into(), json!(entry.quantity));
            out.insert("observedAvailableDepthValue".into(), V::Null);
            out.insert("observedLiquidationValue".into(), V::Null);
            out.insert("entryMarkout".into(), V::Null);
            if now_ns < target {
                return V::Object(out);
            }
            let Some(row) = row else {
                out.insert("reasonCode".into(), json!(if now_ns > target + max_horizon_delay_ms as i128 * MS { "MISSING_HORIZON_EVIDENCE" } else { "AWAITING_HORIZON_RECEIPT" }));
                return V::Object(out);
            };
            out.insert("evidence".into(), evidence(&row.book));
            let delay_ms = elapsed(row.received, target);
            out.insert("delayMs".into(), num(delay_ms));
            if delay_ms > max_horizon_delay_ms as f64 {
                out.insert("reasonCode".into(), json!("STALE_HORIZON_EVIDENCE"));
                return V::Object(out);
            }
            let Some(checked) = &row.checked else {
                out.insert("reasonCode".into(), json!(row.reason_code));
                return V::Object(out);
            };
            for (k, v) in mark(checked, &entry) {
                out.insert(k, v);
            }
            out.insert("available".into(), json!(out["reasonCode"] == json!("COMPLETE")));
            V::Object(out)
        })
        .collect();
    let passage_through = std::cmp::min(now_ns, entry.at + 60 * MINUTE);
    let valid: Vec<&Row> = rows.iter().filter(|x| x.received <= passage_through && x.checked.is_some()).collect();
    let passage = |threshold: i128, is_profit: bool| -> V {
        let row = valid.iter().find(|x| {
            let c = x.checked.as_ref().unwrap();
            if is_profit { c.bids[0].price >= threshold } else { c.bids[0].price <= threshold }
        });
        let Some(&row) = row else {
            return json!({"observed": false, "threshold": money(threshold), "firstObservedAt": V::Null});
        };
        let prior = valid.iter().filter(|x| x.received < row.received).last();
        let checked = row.checked.as_ref().unwrap();
        let qty_at_or_better: i64 = checked.bids.iter().filter(|x| x.price >= threshold).map(|x| x.quantity).sum();
        let mut out = json!({
            "observed": true,
            "threshold": money(threshold),
            "firstObservedAt": row.book.get("receivedAt"),
            "elapsedMs": num(elapsed(row.received, entry.at)),
            "bestBid": money(checked.bids[0].price),
            "evidence": evidence(&row.book),
            "previousValidReceiptAt": prior.and_then(|p| p.book.get("receivedAt").cloned()).unwrap_or(V::Null),
            "observationGapMs": num(elapsed(row.received, prior.map(|p| p.received).unwrap_or(entry.at))),
            "quantityAtOrBetterThreshold": qty_at_or_better,
            "fullQuantityAtOrBetterThreshold": qty_at_or_better >= entry.quantity,
        });
        for (k, v) in mark(checked, &entry) {
            out.as_object_mut().unwrap().insert(k, v);
        }
        out
    };
    let gaps: Vec<f64> = valid
        .iter()
        .enumerate()
        .map(|(i, row)| elapsed(row.received, if i > 0 { valid[i - 1].received } else { entry.at }))
        .collect();
    let tail = elapsed(passage_through, valid.last().map(|r| r.received).unwrap_or(entry.at));
    let mut sorted_intervals: Vec<f64> = gaps.iter().skip(1).cloned().collect();
    sorted_intervals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let middle = sorted_intervals.len() / 2;
    let median_interval: V = if sorted_intervals.is_empty() {
        V::Null
    } else if sorted_intervals.len() % 2 == 1 {
        num(sorted_intervals[middle])
    } else {
        num((sorted_intervals[middle - 1] + sorted_intervals[middle]) / 2.0)
    };
    let mut counts = Map::new();
    counts.insert("suppliedBooks".into(), json!(supplied_books));
    counts.insert("unrelatedBooks".into(), json!(unrelated_books));
    counts.insert("preFillReceipts".into(), json!(pre_fill));
    counts.insert("futureReceipts".into(), json!(future));
    counts.insert("invalidReceiptClocks".into(), json!(invalid_clocks));
    counts.insert("duplicateReceipts".into(), json!(duplicates));
    counts.insert("observedReceipts".into(), json!(rows.len()));
    counts.insert("validReceipts".into(), json!(valid_receipts));
    counts.insert("distinctSourceVersions".into(), json!(versions.len()));
    counts.insert("distinctBookContents".into(), json!(contents.len()));
    counts.insert("reasonCounts".into(), V::Object(reason_counts));
    let max_gap = gaps.iter().cloned().fold(tail, f64::max);
    Ok(json!({
        "schemaVersion": 1,
        "measurementKind": "attributed_buy_fill_observed_bid_markouts",
        "tournamentId": tournament_id,
        "asOf": ms_to_iso(now as f64),
        "executionGuaranteed": false,
        "profitabilityEstablished": false,
        "addedFees": "0.000000",
        "policy": policy,
        "entry": {
            "marketId": fill.get("marketId"),
            "exchangeId": fill.get("exchangeId"),
            "side": "YES",
            "quantity": entry.quantity,
            "price": money(entry.price_micros),
            "cost": money(entry.cost),
            "filledAt": fill.get("filledAt"),
            "origin": "caller_supplied_reconciled_owned_fill",
        },
        "horizons": horizon_rows,
        "passages": {
            "windowThrough": iso(passage_through),
            "takeProfit": passage(entry.price_micros + profit, true),
            "stopLoss": passage(entry.price_micros - loss, false),
            "continuousCrossingClaim": false,
        },
        "sampling": {
            "suppliedBooks": counts["suppliedBooks"],
            "unrelatedBooks": counts["unrelatedBooks"],
            "preFillReceipts": counts["preFillReceipts"],
            "futureReceipts": counts["futureReceipts"],
            "invalidReceiptClocks": counts["invalidReceiptClocks"],
            "duplicateReceipts": counts["duplicateReceipts"],
            "observedReceipts": counts["observedReceipts"],
            "validReceipts": counts["validReceipts"],
            "distinctSourceVersions": counts["distinctSourceVersions"],
            "distinctBookContents": counts["distinctBookContents"],
            "reasonCounts": counts["reasonCounts"],
            "passageWindowValidReceipts": valid.len(),
            "firstValidReceiptAt": valid.first().and_then(|r| r.book.get("receivedAt").cloned()).unwrap_or(V::Null),
            "lastValidReceiptAt": valid.last().and_then(|r| r.book.get("receivedAt").cloned()).unwrap_or(V::Null),
            "gapsMs": {
                "maximumIncludingUnobservedTail": num(max_gap),
                "medianObservedInterval": median_interval,
                "firstObservationDelay": gaps.first().map(|g| num(*g)).unwrap_or(V::Null),
                "unobservedTail": num(tail),
            },
        },
        "limitations": [
            "Receipt observations are not independent trades and do not reveal crossings between samples.",
            "Historical markouts retain the original fill quantity after any actual exit; they are not realized or portfolio P&L.",
            "Displayed-depth liquidation assumes the displayed liquidity remains available; no fill, queue priority or execution is inferred.",
            "Old engine timestamps may describe a quiet book; freshness is an authoritative subsequent receipt and a bounded horizon delay.",
        ],
    }))
}
