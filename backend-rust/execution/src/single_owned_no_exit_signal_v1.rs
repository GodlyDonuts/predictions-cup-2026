// execution/single-owned-no-exit-signal-v1.mjs — pure owned-lot liquidation
// SIGNAL, never an order or activation. Every internal failure collapses into
// the base object with a `reason`; only this module's OWNED_NO_EXIT_* codes are
// surfaced verbatim.
use crate::core::payload_hash;
use crate::receipts::parse_number_lexemes;
use baal_service_runtime::contract::{instant_ns, timestamp_str};
use baal_service_runtime::fixed::{instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use baal_service_runtime::market::normalize_book;
use serde_json::{json, Map, Value as V};
use std::collections::BTreeSet;

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("OWNED_NO_EXIT_{c}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn hex(s: &V) -> bool {
    s.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn id(s: &V) -> bool {
    s.as_str().is_some_and(|s| {
        !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n > 0 && n <= 9_007_199_254_740_991)
    })
}
fn integer(v: &V) -> bool {
    js_int(v).is_some()
}
fn slug(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|seg| !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}

// SINGLE_OWNED_NO_EXIT_POLICY (frozen object).
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
        get(r, "method").as_str() == Some("GET")
            && get(r, "endpoint").as_str() == Some(endpoint)
            && get(r, "status") == &json!(200)
            && get(r, "bodyText").as_str().is_some_and(|s| s.len() <= 8 * 1024 * 1024),
        "RAW_RECEIPT",
    )?;
    let start = instant_v(get(r, "requestStartedAt"))?;
    let end = instant_v(get(r, "receivedAt"))?;
    need(
        start <= end && end <= now && now - start <= 20_000 && end - start <= 20_000,
        "RECEIPT_CLOCK",
    )?;
    let source_hash = sha256(get(r, "bodyText").as_str().unwrap().as_bytes());
    if let Some(h) = r.get("sourceBodyHash") {
        need(h.as_str() == Some(source_hash.as_str()), "RECEIPT_HASH")?;
    }
    let (data, _) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap())?;
    Ok(Receipt { data, start, end, source_hash })
}

fn verified_lot(p: &V) -> Result<(V, i128), String> {
    let mut body = p.clone();
    let verification_hash = body.as_object_mut().and_then(|m| m.remove("verificationHash"));
    need(
        verification_hash.as_ref().is_some_and(|h| hex(h))
            && payload_hash(&body)? == verification_hash.as_ref().and_then(|h| h.as_str()).unwrap_or(""),
        "BUY_PROOF_HASH",
    )?;
    need(
        matches!(
            get(p, "mode").as_str(),
            Some("scoped_rest_no_lot_with_rounded_cash_v1" | "scoped_rest_no_lot_with_rounded_cash_neutral_all_v2")
        ) && get(p, "scopedLotVerified") == &json!(true)
            && get(p, "filledQuantity") == &json!(1)
            && get(p, "signedPositionDelta") == &json!(-1)
            && id(get(p, "orderId"))
            && id(get(p, "marketId"))
            && id(get(p, "exchangeId"))
            && get(p, "ownerId") == &json!("Codex")
            && get(p, "runId").as_str().is_some_and(|s| !s.is_empty())
            && get(p, "tournamentId").as_str().is_some_and(|s| !s.is_empty()),
        "BUY_PROOF_SCOPE",
    )?;
    let lot = get(p, "lot");
    need(
        get(lot, "side").as_str() == Some("no")
            && get(lot, "quantity") == &json!(1)
            && get(lot, "ownerId") == get(p, "ownerId")
            && get(lot, "runId") == get(p, "runId")
            && get(lot, "intentId") == get(p, "intentId")
            && get(lot, "orderId") == get(p, "orderId")
            && get(lot, "lotId").as_str().is_some_and(|s| !s.is_empty())
            && get(p, "fills").as_array().is_some_and(|f| f.len() == 1),
        "ONE_EXACT_OWNED_LOT",
    )?;
    let f = &get(p, "fills")[0];
    let cost = price(get(p, "scopedPrincipalCost").as_str().unwrap_or(""))?;
    need(
        get(f, "quantity") == &json!(1)
            && get(f, "signedQuantity") == &json!(-1)
            && get(f, "side").as_str() == Some("no")
            && get(f, "orderId") == get(p, "orderId")
            && get(f, "marketId") == get(p, "marketId")
            && get(f, "exchangeId") == get(p, "exchangeId")
            && get(f, "tournamentId") == get(p, "tournamentId")
            && get(f, "source").as_str() == Some("rest_order_fills")
            && get(f, "priceBasis").as_str() == Some("side_relative")
            && price(get(f, "price").as_str().unwrap_or(""))? == cost
            && price(get(lot, "entryPrice").as_str().unwrap_or(""))? == cost
            && get(f, "filledAt") == get(lot, "openedAt")
            && instant_v(get(lot, "openedAt"))? <= instant_v(get(p, "readAt"))?,
        "EXACT_BUY_BASIS",
    )?;
    Ok((lot.clone(), cost))
}

fn version_compare(a: &V, b: &V) -> Result<i64, String> {
    // BigInt(a.sequence) — digit strings can exceed i64.
    let x = num_bigint::BigInt::parse_bytes(get(a, "sequence").as_str().unwrap_or("").as_bytes(), 10)
        .ok_or_else(|| "PARSE".to_string())?;
    let y = num_bigint::BigInt::parse_bytes(get(b, "sequence").as_str().unwrap_or("").as_bytes(), 10)
        .ok_or_else(|| "PARSE".to_string())?;
    if x > y {
        return Ok(1);
    }
    if x < y {
        return Ok(-1);
    }
    let sa = instant_ns(get(a, "sourceAt").as_str().unwrap_or("")).map_err(|e| e.0)?;
    let sb = instant_ns(get(b, "sourceAt").as_str().unwrap_or("")).map_err(|e| e.0)?;
    Ok(if sa > sb { 1 } else if sa < sb { -1 } else { 0 })
}

// evaluateSingleOwnedNoExit({buyProof,positionsReceipt,bookReceipt,globalRisk,tournamentSlug,previousBookVersion,now})
pub fn evaluate_single_owned_no_exit(input: &V) -> V {
    let buy_proof = get(input, "buyProof");
    let positions_receipt = get(input, "positionsReceipt");
    let book_receipt = get(input, "bookReceipt");
    let global_risk = get(input, "globalRisk");
    let default_slug = json!("midterm-elections");
    let tournament_slug = input.get("tournamentSlug").unwrap_or(&default_slug);
    let previous_book_version = get(input, "previousBookVersion");
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
    let inner = (|| -> Result<V, String> {
        let at = instant_v(now)?;
        let (lot, cost) = verified_lot(buy_proof)?;
        need(slug(tournament_slug.as_str().unwrap_or("")), "TOURNAMENT_SLUG")?;
        need(
            get(global_risk, "ownerId") == get(buy_proof, "ownerId")
                && get(global_risk, "runId") == get(buy_proof, "runId")
                && get(global_risk, "tournamentId") == get(buy_proof, "tournamentId")
                && get(global_risk, "complete") == &json!(true)
                && get(global_risk, "soleCollector") == &json!(true)
                && get(global_risk, "stopped") == &json!(false)
                && hex(get(global_risk, "sourceHash"))
                && get(global_risk, "unresolvedIntentIds").as_array().is_some_and(|a| a.is_empty())
                && instant_v(get(global_risk, "at"))? <= at
                && at - instant_v(get(global_risk, "at"))? <= 20_000,
            "GLOBAL_UNKNOWN_OR_PENDING",
        )?;
        let endpoint = format!(
            "/tournaments/{}/portfolio/positions",
            tournament_slug.as_str().unwrap_or("")
        );
        let p = receipt(positions_receipt, &endpoint, at)?;
        need(
            get(&p.data, "positions").is_array()
                && p.data.get("pagination").is_none()
                && js_truthy(p.data.get("summary"))
                && ["totalMarketValue", "totalCostBasis", "totalUnrealizedPnl"].iter()
                    .all(|k| get(get(&p.data, "summary"), k).as_f64().is_some_and(|n| n.is_finite())),
            "CURRENT_POSITIONS_INCOMPLETE",
        )?;
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for r in get(&p.data, "positions").as_array().unwrap() {
            need(
                id(get(r, "exchangeId"))
                    && id(get(r, "marketId"))
                    && integer(get(r, "quantity"))
                    && get(r, "settled").is_boolean()
                    && !seen.contains(get(r, "exchangeId").as_str().unwrap_or("")),
                "CURRENT_POSITION_SCHEMA",
            )?;
            seen.insert(get(r, "exchangeId").as_str().unwrap().to_string());
        }
        let held = get(&p.data, "positions")
            .as_array()
            .unwrap()
            .iter()
            .find(|r| get(r, "exchangeId") == get(buy_proof, "exchangeId"))
            .cloned()
            .unwrap_or(V::Null);
        need(
            get(&held, "marketId") == get(buy_proof, "marketId")
                && get(&held, "quantity") == &json!(-1)
                && get(&held, "settled") == &json!(false),
            "CURRENT_EXACT_SIGNED_NO_HOLDING",
        )?;
        let endpoint = format!(
            "/markets/{}/orderbook?tournamentId={}&depth=20",
            get(buy_proof, "marketId").as_str().unwrap_or(""),
            get(buy_proof, "tournamentId").as_str().unwrap_or("")
        );
        let b = receipt(book_receipt, &endpoint, at)?;
        need(
            instant_v(get(buy_proof, "readAt"))? <= p.start
                && p.end <= b.start
                && instant_v(get(global_risk, "at"))? >= p.end,
            "CURRENT_HOLDING_BEFORE_BOOK",
        )?;
        need(
            get(&b.data, "contexts").as_array().is_some_and(|c| {
                c.len() == 1
                    && get(&c[0], "type") == &json!("tournament")
                    && get(&get(&c[0], "tournament"), "id") == get(buy_proof, "tournamentId")
            }),
            "BOOK_SCOPE",
        )?;
        let book = normalize_book(
            &b.data,
            get(buy_proof, "marketId").as_str().unwrap_or(""),
            get(buy_proof, "tournamentId").as_str().unwrap_or(""),
            get(book_receipt, "receivedAt").as_str().unwrap_or(""),
            get(book_receipt, "requestStartedAt").as_str().unwrap_or(""),
            20_000.0,
            &BTreeSet::new(),
            "authoritative_receipt_v1",
        )?;
        let version_key = format!(
            "{}:{}",
            get(&book, "sequence").as_str().unwrap_or(""),
            get(&book, "sourceAt").as_str().unwrap_or("")
        );
        need(
            get(&book, "exchangeId") == get(buy_proof, "exchangeId")
                && get(&book, "bids").as_array().is_some_and(|a| a.len() <= 20)
                && get(&book, "asks").as_array().is_some_and(|a| a.len() <= 20)
                && get(&book, "sequence").as_str().is_some_and(|s| {
                    !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
                })
                && {
                    let sa = get(&book, "sourceAt").as_str().unwrap_or("");
                    timestamp_str(sa).is_ok_and(|t| t == sa)
                }
                && get(&book, "versionKey").as_str() == Some(version_key.as_str())
                && instant_ns(get(&book, "sourceAt").as_str().unwrap_or("")).map_err(|e| e.0)?
                    <= (b.end as i128 + 2_000) * 1_000_000,
            "BOOK_VERSION_OR_DEPTH",
        )?;
        let depth_hash = payload_hash(&json!({
            "bids": get(&book, "bids"),
            "asks": get(&book, "asks"),
        }))?;
        if !previous_book_version.is_null() {
            need(
                get(previous_book_version, "sequence").as_str().is_some_and(|s| {
                    !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
                }) && {
                    let sa = get(previous_book_version, "sourceAt").as_str();
                    sa.is_some() && timestamp_str(sa.unwrap()).is_ok_and(|t| t == sa.unwrap())
                } && hex(get(previous_book_version, "depthHash")),
                "PREVIOUS_BOOK_VERSION",
            )?;
            let comparison = version_compare(&book, previous_book_version)?;
            need(comparison >= 0, "BOOK_VERSION_ROLLBACK")?;
            if comparison == 0 {
                need(
                    depth_hash == get(previous_book_version, "depthHash").as_str().unwrap_or(""),
                    "EXACT_VERSION_DEPTH_CONFLICT",
                )?;
            }
        }
        // NO SELL consumes canonical YES asks; each rung has independent 1%
        // capacity.
        let level = get(&book, "asks").as_array().cloned().unwrap_or_default().iter().find(|l| {
            js_int(get(l, "quantity")).is_some()
                && (js_int(get(l, "quantity")).unwrap() as f64 / 100.0).floor() >= 1.0
                && price(get(l, "price").as_str().unwrap_or("")).is_ok_and(|p| SCALE - p - TICK >= cost + TICK)
        }).cloned();
        let mut common = Map::from_iter([
            ("ownerId".into(), get(buy_proof, "ownerId").clone()),
            ("runId".into(), get(buy_proof, "runId").clone()),
            ("tournamentId".into(), get(buy_proof, "tournamentId").clone()),
            ("marketId".into(), get(buy_proof, "marketId").clone()),
            ("exchangeId".into(), get(buy_proof, "exchangeId").clone()),
            ("lotId".into(), get(&lot, "lotId").clone()),
            ("buyOrderId".into(), get(buy_proof, "orderId").clone()),
            ("buyProofHash".into(), get(buy_proof, "verificationHash").clone()),
            ("exactLotCost".into(), json!(money(cost))),
            ("side".into(), json!("no")),
            ("action".into(), json!("sell")),
            ("quantity".into(), json!(1)),
            ("currentSignedQuantity".into(), json!(-1)),
            ("positionsSourceHash".into(), json!(p.source_hash)),
            ("positionsReceivedAt".into(), get(positions_receipt, "receivedAt").clone()),
            ("bookSourceHash".into(), json!(b.source_hash)),
            (
                "bookVersion".into(),
                json!({
                    "sequence": get(&book, "sequence"),
                    "sourceAt": get(&book, "sourceAt"),
                    "versionKey": get(&book, "versionKey"),
                    "depthHash": depth_hash,
                    "requestStartedAt": get(book_receipt, "requestStartedAt"),
                    "receivedAt": get(book_receipt, "receivedAt"),
                }),
            ),
            ("globalRiskSourceHash".into(), get(global_risk, "sourceHash").clone()),
            ("priceBasis".into(), json!("no_sell_bid_from_same_exchange_yes_ask_complement")),
            ("nativeNoBookObserved".into(), json!(false)),
        ]);
        // {...base, ...common} — base key order, common overrides in place.
        let mut merged = base.as_object().unwrap().clone();
        for (k, v) in common {
            merged.insert(k, v);
        }
        let common = V::Object(merged);
        if level.is_none() {
            let mut hold = common.as_object().unwrap().clone();
            hold.insert("reason".into(), json!("NO_DEPTH_QUALIFIED_PROTECTED_PROFIT"));
            hold.insert("depthQualifiedQuantity".into(), json!(0));
            hold.insert("protectedLimit".into(), V::Null);
            hold.insert("protectedNetProfit".into(), V::Null);
            let h = payload_hash(&V::Object(hold.clone()))?;
            hold.insert("signalHash".into(), json!(h));
            return Ok(V::Object(hold));
        }
        let level = level.unwrap();
        let bid = SCALE - price(get(&level, "price").as_str().unwrap_or(""))?;
        let limit = bid - TICK;
        price(&money(limit))?;
        let profit = limit - cost;
        let mut signal = common.as_object().unwrap().clone();
        signal.insert("signalReady".into(), json!(true));
        signal.insert("reason".into(), json!("ONE_SHARE_PROTECTED_PROFIT_CANDIDATE"));
        signal.insert("rawExecutableNoBid".into(), json!(money(bid)));
        signal.insert("protectedLimit".into(), json!(money(limit)));
        signal.insert("protectedNetProfit".into(), json!(money(profit)));
        signal.insert("minimumNetProfit".into(), json!("0.005000"));
        signal.insert("fees".into(), json!("0.000000"));
        signal.insert(
            "depthQualifiedQuantity".into(),
            json!((js_int(get(&level, "quantity")).unwrap() as f64 / 100.0).floor()),
        );
        signal.insert(
            "yesAskSourceLevel".into(),
            json!({"price": get(&level, "price"), "quantity": get(&level, "quantity")}),
        );
        signal.insert("conditionalOnActualFilledSale".into(), json!(true));
        let h = payload_hash(&V::Object(signal.clone()))?;
        signal.insert("signalHash".into(), json!(h));
        Ok(V::Object(signal))
    })();
    match inner {
        Ok(v) => v,
        Err(e) => {
            let owned = e.len() > "OWNED_NO_EXIT_".len()
                && e.starts_with("OWNED_NO_EXIT_")
                && e["OWNED_NO_EXIT_".len()..].bytes().all(|b| b.is_ascii_uppercase() || b == b'_');
            let mut out = base.as_object().unwrap().clone();
            out.insert("reason".into(), json!(if owned { e } else { "INVALID_OR_UNAVAILABLE_EVIDENCE".to_string() }));
            out.insert("holdExistingOwnedNo".into(), json!(true));
            V::Object(out)
        }
    }
}
