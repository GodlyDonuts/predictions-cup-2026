// execution/performance.mjs — read-only measurement of reconciled owned fills,
// not an execution simulation. Each supplied normalized book must carry its
// explicitly verified tournamentId.
use baal_service_runtime::contract::{instant_ns, ms_to_iso, timestamp};
use baal_service_runtime::fixed::{amount, instant, money, price};
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Map, Value as V};
use std::collections::{HashMap, HashSet};

const MAX_QUANTITY: i64 = 2_147_483_647;

fn check(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.to_string()) }
}
fn id(value: &V) -> bool {
    value.as_str().is_some_and(|s| {
        !s.is_empty()
            && !s.starts_with('0')
            && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<u64>().map(|n| n <= 9_007_199_254_740_991).unwrap_or(false)
    })
}
fn quantity(value: &V) -> bool {
    js_int(value).is_some_and(|q| q > 0 && q <= MAX_QUANTITY)
}
fn plain(value: &V) -> bool {
    value.is_object()
        && !value
            .as_object()
            .unwrap()
            .keys()
            .any(|k| ["__proto__", "constructor", "prototype"].contains(&k.as_str()))
}
fn timestamp_of(v: &V) -> Result<String, String> {
    timestamp(v).map_err(|e| e.0)
}
fn instant_ns_of(v: &V) -> Result<i128, String> {
    instant_ns(v.as_str().unwrap_or("\u{0}")).map_err(|e| e.0)
}
fn instant_ms_of(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or("\u{0}"))
}
fn price_of(v: &V) -> Result<i128, String> {
    price(v.as_str().unwrap_or("\u{0}"))
}
fn amount_of(v: &V) -> Result<i128, String> {
    amount(v.as_str().unwrap_or("\u{0}"))
}

#[derive(Clone)]
struct OwnedLot {
    quantity: i64,
    cost: i128,
}
struct OwnedRow {
    market_id: String,
    quantity: i64,
    cost: i128,
    cash_flow: i128,
    lots: Vec<OwnedLot>,
    last_own_fill_at: Option<String>,
    last_own_fill_ns: i128,
}

fn reconcile_owned(
    positions: &V,
    fills: &V,
    tournament_id: &str,
    now_ns: i128,
) -> Result<HashMap<String, OwnedRow>, String> {
    check(
        plain(positions)
            && positions.as_object().unwrap().len() <= 10000
            && fills.as_array().is_some_and(|f| f.len() <= 100000),
        "INVALID_PERFORMANCE_INPUT",
    )?;
    let fills = fills.as_array().unwrap();
    let mut ledger: HashMap<String, OwnedRow> = HashMap::new();
    let mut fill_keys: HashSet<String> = HashSet::new();
    let mut orders: HashMap<i64, String> = HashMap::new();
    let mut ordered: Vec<(usize, i128, i128)> = Vec::with_capacity(fills.len());
    for (index, fill) in fills.iter().enumerate() {
        check(
            plain(fill)
                && id(fill.get("marketId").unwrap_or(&V::Null))
                && id(fill.get("exchangeId").unwrap_or(&V::Null))
                && quantity(fill.get("quantity").unwrap_or(&V::Null))
                && fill.get("action").and_then(V::as_str).is_some_and(|a| ["buy", "sell"].contains(&a))
                && fill.get("side").map_or(true, |s| s.as_str() == Some("yes"))
                && fill.get("tournamentId").map_or(true, |t| t.as_str() == Some(tournament_id))
                && js_int(fill.get("orderId").unwrap_or(&V::Null)).is_some_and(|o| o > 0)
                && js_int(fill.get("id").unwrap_or(&V::Null)).is_some_and(|i| i > 0),
            "INVALID_RECONCILED_FILL",
        )?;
        let order_id = js_int(fill.get("orderId").unwrap()).unwrap();
        let fill_id = js_int(fill.get("id").unwrap()).unwrap();
        let key = format!("{order_id}:{fill_id}");
        let terms = format!(
            "{}:{}:{}",
            fill["marketId"].as_str().unwrap(),
            fill["exchangeId"].as_str().unwrap(),
            fill["action"].as_str().unwrap()
        );
        check(
            !fill_keys.contains(&key) && orders.get(&order_id).map_or(true, |t| *t == terms),
            "DUPLICATE_OR_CONFLICTING_FILL",
        )?;
        fill_keys.insert(key);
        orders.insert(order_id, terms);
        let at = instant_ns_of(fill.get("filledAt").unwrap_or(&V::Null))?;
        let p = price_of(fill.get("price").unwrap_or(&V::Null))?;
        check(at <= now_ns, "FUTURE_RECONCILED_FILL")?;
        ordered.push((index, at, p));
    }
    ordered.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    for &(index, at, p) in &ordered {
        let fill = &fills[index];
        let exchange_id = fill["exchangeId"].as_str().unwrap();
        let fill_quantity = js_int(fill.get("quantity").unwrap()).unwrap();
        if !ledger.contains_key(exchange_id) {
            ledger.insert(
                exchange_id.to_string(),
                OwnedRow {
                    market_id: fill["marketId"].as_str().unwrap().to_string(),
                    quantity: 0,
                    cost: 0,
                    cash_flow: 0,
                    lots: Vec::new(),
                    last_own_fill_at: None,
                    last_own_fill_ns: 0,
                },
            );
        }
        let row = ledger.get_mut(exchange_id).unwrap();
        check(row.market_id == fill["marketId"].as_str().unwrap(), "FILL_MARKET_IDENTITY_MISMATCH")?;
        let notional = p * (fill_quantity as i128);
        if fill["action"].as_str() == Some("buy") {
            row.quantity += fill_quantity;
            row.cost += notional;
            row.cash_flow -= notional;
            row.lots.push(OwnedLot { quantity: fill_quantity, cost: notional });
        } else {
            check(fill_quantity <= row.quantity, "RECONCILED_FILL_OVERSELL")?;
            let mut left = fill_quantity;
            while left > 0 {
                let lot = row.lots.first_mut().unwrap();
                let used = left.min(lot.quantity);
                let cost = lot.cost * (used as i128) / (lot.quantity as i128);
                row.cost -= cost;
                lot.cost -= cost;
                lot.quantity -= used;
                left -= used;
                if lot.quantity == 0 {
                    row.lots.remove(0);
                }
            }
            row.quantity -= fill_quantity;
            row.cash_flow += notional;
        }
        check(row.quantity <= 9_007_199_254_740_991, "OWNED_QUANTITY_OVERFLOW")?;
        row.last_own_fill_at = Some(timestamp_of(fill.get("filledAt").unwrap_or(&V::Null))?);
        row.last_own_fill_ns = at;
    }
    for (exchange_id, position) in positions.as_object().unwrap() {
        check(
            id(&V::String(exchange_id.clone()))
                && plain(position)
                && id(position.get("marketId").unwrap_or(&V::Null))
                && quantity(position.get("quantity").unwrap_or(&V::Null))
                && position.get("lots").and_then(V::as_array).is_some(),
            "INVALID_OWNED_POSITION",
        )?;
        let replay = ledger.get(exchange_id);
        let position_lots = position["lots"].as_array().unwrap();
        let provenance = match replay {
            Some(replay) => {
                if !(replay.market_id == position["marketId"].as_str().unwrap_or("")
                    && replay.quantity == js_int(&position["quantity"]).unwrap_or(-1)
                    && replay.cost == amount_of(&position["cost"])?
                    && position_lots.len() == replay.lots.len())
                {
                    false
                } else {
                    let mut ok = true;
                    for (index, lot) in position_lots.iter().enumerate() {
                        if !(plain(lot)
                            && quantity(lot.get("quantity").unwrap_or(&V::Null))
                            && js_int(&lot["quantity"]) == Some(replay.lots[index].quantity)
                            && amount_of(&lot["cost"])? == replay.lots[index].cost)
                        {
                            ok = false;
                            break;
                        }
                    }
                    ok
                }
            }
            None => false,
        };
        check(provenance, "OWNERSHIP_PROVENANCE_MISMATCH")?;
    }
    for (exchange_id, row) in &ledger {
        check(
            row.quantity == 0 || positions.as_object().unwrap().contains_key(exchange_id),
            "OWNED_POSITION_MISSING",
        )?;
    }
    Ok(ledger)
}

struct Level {
    price: i128,
    quantity: i64,
}
struct CheckedBook {
    bids: Vec<Level>,
    asks: Vec<Level>,
    book_age_ms: i64,
    source_event_age_ms: f64,
}

fn checked_book(
    book: &V,
    position: &V,
    exchange_id: &str,
    tournament_id: &str,
    now: i64,
    last_own_fill_ns: i128,
    max_book_age_ms: i64,
    max_book_latency_ms: i64,
) -> Result<CheckedBook, String> {
    let instrument = format!("{exchange_id}:YES");
    check(
        plain(book)
            && book.get("tournamentId").and_then(V::as_str) == Some(tournament_id)
            && book.get("marketId") == position.get("marketId")
            && book.get("exchangeId").and_then(V::as_str) == Some(exchange_id)
            && book.get("instrument").and_then(V::as_str) == Some(instrument.as_str()),
        "BOOK_SCOPE_MISMATCH",
    )?;
    check(
        book.get("freshnessPolicy").and_then(V::as_str) == Some("authoritative_receipt_v1")
            && !baal_service_runtime::json_text::js_truthy(book.get("invalidated"))
            && book.get("valid") != Some(&V::Bool(false)),
        "BOOK_NOT_AUTHORITATIVE",
    )?;
    let received = instant_ms_of(book.get("receivedAt").unwrap_or(&V::Null))?;
    let started = instant_ms_of(book.get("requestStartedAt").unwrap_or(&V::Null))?;
    let source = instant_ns_of(book.get("sourceAt").unwrap_or(&V::Null))?;
    let now_ns = (now as i128) * 1_000_000;
    check(received <= now && started <= now, "FUTURE_BOOK_RECEIPT")?;
    check(started <= received && received - started <= max_book_latency_ms, "INVALID_BOOK_LATENCY")?;
    check(now - received <= max_book_age_ms, "STALE_BOOK")?;
    check((received as i128) * 1_000_000 >= last_own_fill_ns, "BOOK_PRECEDES_OWN_FILL")?;
    // Preserve the raw engine clock. Even the normalizer's bounded source-clock
    // skew does not permit a future source mark at this measurement's as-of time.
    check(source <= now_ns && source <= (received as i128 + 2000) * 1_000_000, "FUTURE_BOOK_SOURCE")?;
    let sequence_ok = book.get("sequence").and_then(V::as_str).is_some_and(|s| {
        !s.is_empty()
            && s.bytes().all(|b| b.is_ascii_digit())
            && (s == "0" || !s.starts_with('0'))
            && s.parse::<u64>().map(|n| n <= 9_007_199_254_740_991).unwrap_or(false)
    });
    // JS && is lazy: a bad sequence never evaluates versionKey/timestamp.
    if sequence_ok {
        let t = timestamp_of(book.get("sourceAt").unwrap_or(&V::Null))?;
        let expected = format!("{}:{}", book["sequence"].as_str().unwrap(), t);
        check(book.get("versionKey").and_then(V::as_str) == Some(expected.as_str()), "INVALID_BOOK_VERSION")?;
    } else {
        check(false, "INVALID_BOOK_VERSION")?;
    }
    let levels = |rows: &V, descending: bool| -> Result<Vec<Level>, String> {
        let arr = rows.as_array();
        check(arr.is_some_and(|r| !r.is_empty() && r.len() <= 100000), "INVALID_BOOK_DEPTH")?;
        let mut previous: Option<i128> = None;
        let mut total: i128 = 0;
        let mut out = Vec::with_capacity(arr.unwrap().len());
        for row in arr.unwrap() {
            check(
                plain(row) && js_int(row.get("quantity").unwrap_or(&V::Null)).is_some_and(|q| q > 0),
                "INVALID_BOOK_DEPTH",
            )?;
            let p = price_of(row.get("price").unwrap_or(&V::Null))?;
            check(
                previous.is_none() || (if descending { p < previous.unwrap() } else { p > previous.unwrap() }),
                "INVALID_BOOK_DEPTH",
            )?;
            previous = Some(p);
            total += js_int(&row["quantity"]).unwrap() as i128;
            check(total <= 9_007_199_254_740_991, "INVALID_BOOK_DEPTH")?;
            out.push(Level { price: p, quantity: js_int(&row["quantity"]).unwrap() });
        }
        Ok(out)
    };
    let bids = levels(book.get("bids").unwrap_or(&V::Null), true)?;
    let asks = levels(book.get("asks").unwrap_or(&V::Null), false)?;
    check(bids[0].price < asks[0].price, "LOCKED_OR_CROSSED_BOOK")?;
    Ok(CheckedBook {
        bids,
        asks,
        book_age_ms: now - received,
        source_event_age_ms: (now_ns - source) as f64 / 1_000_000.0,
    })
}

const KNOWN_REASONS: &[&str] = &[
    "MISSING_BOOK",
    "DUPLICATE_BOOK",
    "BOOK_SCOPE_MISMATCH",
    "BOOK_NOT_AUTHORITATIVE",
    "FUTURE_BOOK_RECEIPT",
    "INVALID_BOOK_LATENCY",
    "STALE_BOOK",
    "BOOK_PRECEDES_OWN_FILL",
    "FUTURE_BOOK_SOURCE",
    "INVALID_BOOK_VERSION",
    "INVALID_BOOK_DEPTH",
    "LOCKED_OR_CROSSED_BOOK",
];

fn uuid_ok(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    for (i, c) in b.iter().enumerate() {
        let want_dash = matches!(i, 8 | 13 | 18 | 23);
        if want_dash {
            if *c != b'-' {
                return false;
            }
        } else if !c.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

/// measureExecutionPerformance({tournamentId, positions, fills, books=[], now,
/// maxBookAgeMs=20000, maxBookLatencyMs=20000} = {})
pub fn measure_execution_performance(input: &V) -> Result<V, String> {
    let tournament_id = input.get("tournamentId").unwrap_or(&V::Null);
    let positions = input.get("positions").unwrap_or(&V::Null);
    let fills = input.get("fills").unwrap_or(&V::Null);
    let books = input.get("books").cloned().unwrap_or(V::Array(Vec::new()));
    let now = js_int(input.get("now").unwrap_or(&V::Null));
    let max_book_age_ms = input.get("maxBookAgeMs").cloned().unwrap_or(json!(20000));
    let max_book_latency_ms = input.get("maxBookLatencyMs").cloned().unwrap_or(json!(20000));
    check(
        tournament_id.as_str().is_some_and(uuid_ok),
        "INVALID_PERFORMANCE_SCOPE",
    )?;
    check(now.is_some_and(|n| n >= 0 && n <= 8_640_000_000_000_000), "INVALID_PERFORMANCE_CLOCK")?;
    check(
        [&max_book_age_ms, &max_book_latency_ms]
            .iter()
            .all(|x| js_int(x).is_some_and(|n| n > 0 && n <= 20000)),
        "INVALID_PERFORMANCE_FRESHNESS",
    )?;
    check(
        books.as_array().is_some_and(|b| b.len() <= 10000),
        "INVALID_PERFORMANCE_BOOKS",
    )?;
    let books = books.as_array().unwrap();
    let now = now.unwrap();
    let ledger = reconcile_owned(positions, fills, tournament_id.as_str().unwrap(), (now as i128) * 1_000_000)?;
    let mut rows: Vec<V> = Vec::new();
    let mut cash_flow: i128 = 0;
    let mut owned_cost: i128 = 0;
    let mut observed_value: i128 = 0;
    let mut midpoint_value: i128 = 0;
    let mut liquidation_complete = true;
    let mut midpoint_complete = true;
    let positions_obj = positions.as_object().unwrap();
    let mut coverage = Map::new();
    coverage.insert("ownedPositions".into(), json!(positions_obj.len()));
    coverage.insert("validBooks".into(), json!(0));
    coverage.insert("liquidationCompletePositions".into(), json!(0));
    coverage.insert("midpointMarkedPositions".into(), json!(0));
    coverage.insert("ownedQuantity".into(), json!(0));
    coverage.insert("observedLiquidationQuantity".into(), json!(0));
    coverage.insert("unsoldQuantity".into(), json!(0));
    coverage.insert("reasonCounts".into(), json!(Map::new()));
    for row in ledger.values() {
        cash_flow += row.cash_flow;
        owned_cost += row.cost;
    }
    let mut sorted: Vec<(&String, &V)> = positions_obj.iter().collect();
    sorted.sort_by(|a, b| {
        a.0.parse::<f64>()
            .unwrap_or(f64::NAN)
            .partial_cmp(&b.0.parse::<f64>().unwrap_or(f64::NAN))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (exchange_id, position) in sorted {
        let own = ledger.get(exchange_id).ok_or("OWNERSHIP_PROVENANCE_MISMATCH")?;
        let candidates: Vec<&V> = books
            .iter()
            .filter(|b| b.get("exchangeId").and_then(V::as_str) == Some(exchange_id.as_str()))
            .collect();
        let position_quantity = js_int(&position["quantity"]).unwrap();
        let mut out = json!({
            "marketId": position.get("marketId").cloned().unwrap_or(V::Null),
            "exchangeId": exchange_id,
            "side": "YES",
            "quantity": position.get("quantity").cloned().unwrap_or(V::Null),
            "costBasis": money(own.cost),
            "lastOwnFillAt": own.last_own_fill_at.clone().map_or(V::Null, V::String),
            "reasonCode": "MISSING_BOOK",
            "book": V::Null,
            "availableBidQuantity": V::Null,
            "observedLiquidationQuantity": 0,
            "unsoldQuantity": position.get("quantity").cloned().unwrap_or(V::Null),
            "observedAvailableDepthValue": V::Null,
            "observedLiquidationValue": V::Null,
            "observedLiquidationUnrealizedPnl": V::Null,
            "midpointPrice": V::Null,
            "midpointValue": V::Null,
            "midpointUnrealizedPnl": V::Null,
        });
        let measured = (|| -> Result<(), String> {
            check(candidates.len() <= 1, "DUPLICATE_BOOK")?;
            check(
                candidates.len() == 1,
                if books.iter().any(|b| b.get("marketId") == position.get("marketId")) {
                    "BOOK_SCOPE_MISMATCH"
                } else {
                    "MISSING_BOOK"
                },
            )?;
            let book = candidates[0];
            let checked = checked_book(
                book,
                position,
                exchange_id,
                tournament_id.as_str().unwrap(),
                now,
                own.last_own_fill_ns,
                js_int(&max_book_age_ms).unwrap(),
                js_int(&max_book_latency_ms).unwrap(),
            )?;
            out["book"] = json!({
                "receivedAt": book.get("receivedAt").cloned().unwrap_or(V::Null),
                "requestStartedAt": book.get("requestStartedAt").cloned().unwrap_or(V::Null),
                "sourceAt": book.get("sourceAt").cloned().unwrap_or(V::Null),
                "versionKey": book.get("versionKey").cloned().unwrap_or(V::Null),
                "freshnessPolicy": book.get("freshnessPolicy").cloned().unwrap_or(V::Null),
                "ageMs": checked.book_age_ms,
                "sourceEventAgeMs": checked.source_event_age_ms,
            });
            let available: i64 = checked.bids.iter().map(|l| l.quantity).sum();
            out["availableBidQuantity"] = json!(available);
            let mut remaining = position_quantity;
            let mut proceeds: i128 = 0;
            for level in &checked.bids {
                let take = remaining.min(level.quantity);
                proceeds += level.price * (take as i128);
                remaining -= take;
                if remaining == 0 {
                    break;
                }
            }
            out["observedLiquidationQuantity"] = json!(position_quantity - remaining);
            out["unsoldQuantity"] = json!(remaining);
            out["observedAvailableDepthValue"] = json!(money(proceeds));
            out["reasonCode"] = json!(if remaining > 0 { "INSUFFICIENT_BID_DEPTH" } else { "COMPLETE" });
            if remaining == 0 {
                out["observedLiquidationValue"] = json!(money(proceeds));
                out["observedLiquidationUnrealizedPnl"] = json!(money(proceeds - own.cost));
                observed_value += proceeds;
                *coverage.get_mut("liquidationCompletePositions").unwrap() =
                    json!(coverage["liquidationCompletePositions"].as_i64().unwrap() + 1);
            }
            let mid = (checked.bids[0].price + checked.asks[0].price) / 2;
            let mark = mid * (position_quantity as i128);
            out["midpointPrice"] = json!(money(mid));
            out["midpointValue"] = json!(money(mark));
            out["midpointUnrealizedPnl"] = json!(money(mark - own.cost));
            midpoint_value += mark;
            *coverage.get_mut("validBooks").unwrap() = json!(coverage["validBooks"].as_i64().unwrap() + 1);
            *coverage.get_mut("midpointMarkedPositions").unwrap() =
                json!(coverage["midpointMarkedPositions"].as_i64().unwrap() + 1);
            Ok(())
        })();
        if let Err(code) = measured {
            out["reasonCode"] =
                json!(if KNOWN_REASONS.contains(&code.as_str()) { code.as_str() } else { "INVALID_BOOK" });
        }
        if out["observedLiquidationValue"].is_null() {
            liquidation_complete = false;
        }
        if out["midpointValue"].is_null() {
            midpoint_complete = false;
        }
        *coverage.get_mut("ownedQuantity").unwrap() =
            json!(coverage["ownedQuantity"].as_i64().unwrap() + position_quantity);
        *coverage.get_mut("observedLiquidationQuantity").unwrap() = json!(
            coverage["observedLiquidationQuantity"].as_i64().unwrap()
                + out["observedLiquidationQuantity"].as_i64().unwrap()
        );
        *coverage.get_mut("unsoldQuantity").unwrap() =
            json!(coverage["unsoldQuantity"].as_i64().unwrap() + out["unsoldQuantity"].as_i64().unwrap());
        let reason = out["reasonCode"].as_str().unwrap().to_string();
        let counts = coverage.get_mut("reasonCounts").and_then(V::as_object_mut).unwrap();
        let count = counts.get(&reason).and_then(V::as_i64).unwrap_or(0);
        counts.insert(reason, json!(count + 1));
        rows.push(out);
    }
    let realized = cash_flow + owned_cost;
    coverage.insert("liquidationComplete".into(), json!(liquidation_complete));
    coverage.insert("midpointComplete".into(), json!(midpoint_complete));
    Ok(json!({
        "schemaVersion": 1,
        "measurementKind": "reconciled_owned_observed_depth_estimate",
        "tournamentId": tournament_id.clone(),
        "asOf": ms_to_iso(now as f64),
        "executionGuaranteed": false,
        "addedFees": "0.000000",
        "ownedCost": money(owned_cost),
        "realizedPnl": money(realized),
        "observedLiquidationValue": if liquidation_complete { json!(money(observed_value)) } else { V::Null },
        "observedLiquidationUnrealizedPnl": if liquidation_complete { json!(money(observed_value - owned_cost)) } else { V::Null },
        "observedLiquidationTotalPnl": if liquidation_complete { json!(money(realized + observed_value - owned_cost)) } else { V::Null },
        "midpointValue": if midpoint_complete { json!(money(midpoint_value)) } else { V::Null },
        "midpointUnrealizedPnl": if midpoint_complete { json!(money(midpoint_value - owned_cost)) } else { V::Null },
        "midpointTotalPnl": if midpoint_complete { json!(money(realized + midpoint_value - owned_cost)) } else { V::Null },
        "coverage": V::Object(coverage),
        "positions": rows,
    }))
}
