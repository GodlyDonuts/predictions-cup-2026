// execution/single-owned-no-exit-signal-v1.mjs — pure owned-lot liquidation
// SIGNAL, never an order or activation.
use baal_execution::core::payload_hash;
use baal_execution::receipts::parse_number_lexemes;
use baal_service_runtime::contract::{instant_ns, timestamp};
use baal_service_runtime::fixed::{instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, sha256};
use serde_json::{json, Map, Value as V};
use std::collections::BTreeSet;

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("OWNED_NO_EXIT_{c}")) }
}
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn id(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && s.as_bytes()[0] != b'0'
            && s.parse::<u128>().map(|n| n <= 9_007_199_254_740_991).unwrap_or(false)
    })
}
pub fn single_owned_no_exit_policy() -> V {
    json!({
        "quantity": 1,
        "depthFractionBps": 100,
        "protectiveTicks": 1,
        "minimumProtectedNetProfit": "0.005000",
        "fee": "0.000000",
        "maximumAgeMs": 20000,
        "maximumLatencyMs": 20000,
        "maximumServerClockSkewMs": 2000,
    })
}
struct Receipt {
    data: V,
    start: i64,
    end: i64,
    source_hash: String,
}
fn receipt(r: &V, endpoint: &str, now: i64) -> Result<Receipt, String> {
    need(
        r.get("method").and_then(|m| m.as_str()) == Some("GET")
            && r.get("endpoint").and_then(|e| e.as_str()) == Some(endpoint)
            && r.get("status") == Some(&json!(200))
            && r.get("bodyText").is_some_and(|b| b.is_string() && b.as_str().unwrap().len() <= 8 * 1024 * 1024),
        "RAW_RECEIPT",
    )?;
    let start = instant(r.get("requestStartedAt").and_then(|v| v.as_str()).unwrap_or(""))
        .map_err(|_| "Timestamp must be canonical UTC ISO milliseconds".to_string())?;
    let end = instant(r.get("receivedAt").and_then(|v| v.as_str()).unwrap_or(""))
        .map_err(|_| "Timestamp must be canonical UTC ISO milliseconds".to_string())?;
    need(start <= end && end <= now && now - start <= 20000 && end - start <= 20000, "RECEIPT_CLOCK")?;
    let body = r["bodyText"].as_str().unwrap_or("");
    let source_hash = sha256(body.as_bytes());
    need(r.get("sourceBodyHash").map_or(true, |h| h.as_str() == Some(source_hash.as_str())), "RECEIPT_HASH")?;
    let (data, _) = parse_number_lexemes(body).map_err(|_| "OWNED_NO_EXIT_RAW_RECEIPT".to_string())?;
    Ok(Receipt { data, start, end, source_hash })
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn verified_lot(p: &V) -> Result<(V, i128), String> {
    let mut body = p.clone();
    let vh = body.as_object_mut().and_then(|m| m.shift_remove("verificationHash")).unwrap_or(V::Null);
    need(hex(&vh) && payload_hash(&body).ok().as_deref() == vh.as_str(), "BUY_PROOF_HASH")?;
    need(
        get(p, "mode").as_str().is_some_and(|m| {
            m == "scoped_rest_no_lot_with_rounded_cash_v1" || m == "scoped_rest_no_lot_with_rounded_cash_neutral_all_v2"
        }) && get(p, "scopedLotVerified") == &json!(true)
            && get(p, "filledQuantity") == &json!(1)
            && get(p, "signedPositionDelta") == &json!(-1)
            && id(get(p, "orderId")) && id(get(p, "marketId")) && id(get(p, "exchangeId"))
            && get(p, "ownerId") == &json!("Codex")
            && get(p, "runId").as_str().is_some_and(|s| !s.is_empty())
            && get(p, "tournamentId").as_str().is_some_and(|s| !s.is_empty()),
        "BUY_PROOF_SCOPE",
    )?;
    let lot = get(p, "lot");
    need(
        get(lot, "side") == &json!("no") && get(lot, "quantity") == &json!(1)
            && get(lot, "ownerId") == get(p, "ownerId")
            && get(lot, "runId") == get(p, "runId")
            && get(lot, "intentId") == get(p, "intentId")
            && get(lot, "orderId") == get(p, "orderId")
            && get(lot, "lotId").as_str().is_some_and(|s| !s.is_empty())
            && get(p, "fills").as_array().is_some_and(|f| f.len() == 1),
        "ONE_EXACT_OWNED_LOT",
    )?;
    let f = &get(p, "fills").as_array().unwrap()[0];
    let cost = price(get(p, "scopedPrincipalCost").as_str().unwrap_or(""))
        .map_err(|e| e)?;
    need(
        get(f, "quantity") == &json!(1) && get(f, "signedQuantity") == &json!(-1)
            && get(f, "side") == &json!("no")
            && get(f, "orderId") == get(p, "orderId")
            && get(f, "marketId") == get(p, "marketId")
            && get(f, "exchangeId") == get(p, "exchangeId")
            && get(f, "tournamentId") == get(p, "tournamentId")
            && get(f, "source") == &json!("rest_order_fills")
            && get(f, "priceBasis") == &json!("side_relative")
            && price(get(f, "price").as_str().unwrap_or("")).ok() == Some(cost)
            && price(get(lot, "entryPrice").as_str().unwrap_or("")).ok() == Some(cost)
            && get(f, "filledAt") == get(lot, "openedAt")
            && instant(get(lot, "openedAt").as_str().unwrap_or("")).ok() <= instant(get(p, "readAt").as_str().unwrap_or("")).ok(),
        "EXACT_BUY_BASIS",
    )?;
    Ok((lot.clone(), cost))
}
fn version_compare(a: &V, b: &V) -> i64 {
    let x: i128 = get(a, "sequence").as_str().and_then(|s| s.parse::<i128>().ok()).unwrap_or(0);
    let y: i128 = get(b, "sequence").as_str().and_then(|s| s.parse::<i128>().ok()).unwrap_or(0);
    if x > y { return 1; }
    if x < y { return -1; }
    let an = instant_ns(get(a, "sourceAt").as_str().unwrap_or("")).unwrap_or(0);
    let bn = instant_ns(get(b, "sourceAt").as_str().unwrap_or("")).unwrap_or(0);
    if an > bn { 1 } else if an < bn { -1 } else { 0 }
}
pub fn evaluate_single_owned_no_exit(input: &V) -> V {
    let buy_proof = get(input, "buyProof");
    let positions_receipt = get(input, "positionsReceipt");
    let book_receipt = get(input, "bookReceipt");
    let global_risk = get(input, "globalRisk");
    let tournament_slug = get(input, "tournamentSlug").as_str().unwrap_or("midterm-elections").to_string();
    let previous_book_version = input.get("previousBookVersion").cloned().unwrap_or(V::Null);
    let now = get(input, "now");
    let base = json!({
        "schemaVersion": 1,
        "mode": "single_owned_no_protected_profit_exit_signal_v1",
        "decisionAt": now,
        "policy": single_owned_no_exit_policy(),
        "signalReady": false,
        "orderEligible": false,
        "executionAuthority": false,
        "networkRequests": 0,
        "competitionOrders": 0,
        "profitabilityEstablished": false,
    });
    let invalid = |reason: &str| {
        let mut b = base.clone();
        let m = b.as_object_mut().unwrap();
        m.insert("reason".into(), json!(reason));
        m.insert("holdExistingOwnedNo".into(), json!(true));
        b
    };
    (|| -> Result<V, String> {
        let at = instant(now.as_str().unwrap_or("")).map_err(|e| e)?;
        let (lot, cost) = verified_lot(buy_proof)?;
        let slug_ok = !tournament_slug.is_empty() && tournament_slug.split('-').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()));
        need(slug_ok, "TOURNAMENT_SLUG")?;
        need(
            get(global_risk, "ownerId") == get(buy_proof, "ownerId")
                && get(global_risk, "runId") == get(buy_proof, "runId")
                && get(global_risk, "tournamentId") == get(buy_proof, "tournamentId")
                && get(global_risk, "complete") == &json!(true)
                && get(global_risk, "soleCollector") == &json!(true)
                && get(global_risk, "stopped") == &json!(false)
                && hex(get(global_risk, "sourceHash"))
                && get(global_risk, "unresolvedIntentIds").as_array().is_some_and(|a| a.is_empty())
                && instant(get(global_risk, "at").as_str().unwrap_or("")).map(|x| x <= at).unwrap_or(false)
                && at - instant(get(global_risk, "at").as_str().unwrap_or(""))? <= 20000,
            "GLOBAL_UNKNOWN_OR_PENDING",
        )?;
        let p = receipt(positions_receipt, &format!("/tournaments/{tournament_slug}/portfolio/positions"), at)?;
        need(
            get(&p.data, "positions").is_array()
                && p.data.get("pagination").is_none()
                && get(&p.data, "summary").is_object()
                && ["totalMarketValue", "totalCostBasis", "totalUnrealizedPnl"].iter()
                    .all(|k| get(get(&p.data, "summary"), k).is_number()),
            "CURRENT_POSITIONS_INCOMPLETE",
        )?;
        let mut seen = BTreeSet::new();
        for r in get(&p.data, "positions").as_array().unwrap() {
            need(
                id(get(r, "exchangeId")) && id(get(r, "marketId"))
                    && js_int(get(r, "quantity")).is_some()
                    && get(r, "settled").is_boolean()
                    && !seen.contains(&get(r, "exchangeId").as_str().unwrap_or("").to_string()),
                "CURRENT_POSITION_SCHEMA",
            )?;
            seen.insert(get(r, "exchangeId").as_str().unwrap_or("").to_string());
        }
        let held = get(&p.data, "positions").as_array().unwrap().iter()
            .find(|r| get(r, "exchangeId") == get(buy_proof, "exchangeId")).cloned().unwrap_or(V::Null);
        need(
            get(&held, "marketId") == get(buy_proof, "marketId")
                && get(&held, "quantity") == &json!(-1)
                && get(&held, "settled") == &json!(false),
            "CURRENT_EXACT_SIGNED_NO_HOLDING",
        )?;
        let endpoint = format!("/markets/{}/orderbook?tournamentId={}&depth=20",
            get(buy_proof, "marketId").as_str().unwrap_or(""), get(buy_proof, "tournamentId").as_str().unwrap_or(""));
        let b = receipt(book_receipt, &endpoint, at)?;
        need(
            instant(get(buy_proof, "readAt").as_str().unwrap_or(""))? <= p.start
                && p.end <= b.start
                && instant(get(global_risk, "at").as_str().unwrap_or(""))? >= p.end,
            "CURRENT_HOLDING_BEFORE_BOOK",
        )?;
        need(
            get(&b.data, "contexts").as_array().is_some_and(|c| c.len() == 1)
                && get(&get(&b.data, "contexts").as_array().unwrap()[0], "type") == &json!("tournament")
                && get(get(&get(&b.data, "contexts").as_array().unwrap()[0], "tournament"), "id") == get(buy_proof, "tournamentId"),
            "BOOK_SCOPE",
        )?;
        let book = baal_collector::model::normalize_book(
            &b.data,
            get(buy_proof, "marketId").as_str().unwrap_or(""),
            get(buy_proof, "tournamentId").as_str().unwrap_or(""),
            get(book_receipt, "receivedAt").as_str().unwrap_or(""),
            get(book_receipt, "requestStartedAt").as_str().unwrap_or(""),
            20000,
            &BTreeSet::new(),
            "authoritative_receipt_v1",
        ).map_err(|e| e)?;
        let sequence_ok = get(&book, "sequence").as_str().is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
        need(
            get(&book, "exchangeId") == get(buy_proof, "exchangeId")
                && get(&book, "bids").as_array().is_some_and(|x| x.len() <= 20)
                && get(&book, "asks").as_array().is_some_and(|x| x.len() <= 20)
                && sequence_ok
                && get(&book, "sourceAt").as_str() == timestamp(&get(&book, "sourceAt")).ok().as_deref()
                && get(&book, "versionKey").as_str() == Some(&format!("{}:{}", get(&book, "sequence").as_str().unwrap_or(""), get(&book, "sourceAt").as_str().unwrap_or("")))
                && instant_ns(get(&book, "sourceAt").as_str().unwrap_or("")).map(|n| n <= (b.end + 2000) as i128 * 1_000_000).unwrap_or(false),
            "BOOK_VERSION_OR_DEPTH",
        )?;
        let depth_hash = payload_hash(&json!({"bids": get(&book, "bids"), "asks": get(&book, "asks")}))?;
        if !previous_book_version.is_null() {
            need(
                get(&previous_book_version, "sequence").as_str().is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                    && get(&previous_book_version, "sourceAt").is_string()
                    && get(&previous_book_version, "sourceAt").as_str() == timestamp(&get(&previous_book_version, "sourceAt")).ok().as_deref()
                    && hex(get(&previous_book_version, "depthHash")),
                "PREVIOUS_BOOK_VERSION",
            )?;
            let comparison = version_compare(&book, &previous_book_version);
            need(comparison >= 0, "BOOK_VERSION_ROLLBACK")?;
            if comparison == 0 { need(depth_hash == get(&previous_book_version, "depthHash").as_str().unwrap_or(""), "EXACT_VERSION_DEPTH_CONFLICT")?; }
        }
        // NO SELL consumes canonical YES asks; each rung has independent 1%
        // capacity.
        let level = get(&book, "asks").as_array().unwrap().iter().find(|l| {
            let q = get(l, "quantity").as_i64().unwrap_or(-1);
            q / 100 >= 1
                && price(get(l, "price").as_str().unwrap_or("")).map(|p| SCALE - p - TICK >= cost + TICK).unwrap_or(false)
        }).cloned().unwrap_or(V::Null);
        let common = json!({
            "ownerId": get(buy_proof, "ownerId"),
            "runId": get(buy_proof, "runId"),
            "tournamentId": get(buy_proof, "tournamentId"),
            "marketId": get(buy_proof, "marketId"),
            "exchangeId": get(buy_proof, "exchangeId"),
            "lotId": get(&lot, "lotId"),
            "buyOrderId": get(buy_proof, "orderId"),
            "buyProofHash": get(buy_proof, "verificationHash"),
            "exactLotCost": money(cost),
            "side": "no",
            "action": "sell",
            "quantity": 1,
            "currentSignedQuantity": -1,
            "positionsSourceHash": p.source_hash,
            "positionsReceivedAt": get(positions_receipt, "receivedAt"),
            "bookSourceHash": b.source_hash,
            "bookVersion": {
                "sequence": get(&book, "sequence"),
                "sourceAt": get(&book, "sourceAt"),
                "versionKey": get(&book, "versionKey"),
                "depthHash": depth_hash,
                "requestStartedAt": get(book_receipt, "requestStartedAt"),
                "receivedAt": get(book_receipt, "receivedAt"),
            },
            "globalRiskSourceHash": get(global_risk, "sourceHash"),
            "priceBasis": "no_sell_bid_from_same_exchange_yes_ask_complement",
            "nativeNoBookObserved": false,
        });
        let merged = |extra: V| -> V {
            let mut m = base.as_object().unwrap().clone();
            for (k, v) in common.as_object().unwrap() { m.insert(k.clone(), v.clone()); }
            for (k, v) in extra.as_object().unwrap() { m.insert(k.clone(), v.clone()); }
            V::Object(m)
        };
        if level.is_null() {
            let hold = merged(json!({
                "reason": "NO_DEPTH_QUALIFIED_PROTECTED_PROFIT",
                "depthQualifiedQuantity": 0,
                "protectedLimit": V::Null,
                "protectedNetProfit": V::Null,
            }));
            let mut out = hold.clone();
            out.as_object_mut().unwrap().insert("signalHash".into(), json!(payload_hash(&hold)?));
            return Ok(out);
        }
        let bid = SCALE - price(get(&level, "price").as_str().unwrap_or(""))?;
        let limit = bid - TICK;
        price(&money(limit))?;
        let profit = limit - cost;
        let signal = merged(json!({
            "signalReady": true,
            "reason": "ONE_SHARE_PROTECTED_PROFIT_CANDIDATE",
            "rawExecutableNoBid": money(bid),
            "protectedLimit": money(limit),
            "protectedNetProfit": money(profit),
            "minimumNetProfit": "0.005000",
            "fees": "0.000000",
            "depthQualifiedQuantity": get(&level, "quantity").as_i64().unwrap_or(0) / 100,
            "yesAskSourceLevel": {"price": get(&level, "price"), "quantity": get(&level, "quantity")},
            "conditionalOnActualFilledSale": true,
        }));
        let mut out = signal.clone();
        out.as_object_mut().unwrap().insert("signalHash".into(), json!(payload_hash(&signal)?));
        Ok(out)
    })().unwrap_or_else(|e| {
        let reason = if e.starts_with("OWNED_NO_EXIT_") { e } else { "INVALID_OR_UNAVAILABLE_EVIDENCE".to_string() };
        invalid(&reason)
    })
}
