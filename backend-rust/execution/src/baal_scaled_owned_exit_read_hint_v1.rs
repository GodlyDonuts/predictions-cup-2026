// execution/baal-scaled-owned-exit-read-hint-v1.mjs — a hint may SAVE a failed
// read packet, never grant a trade, valuation mark or waive reconciliation.
// Missing/stale/ambiguous evidence falls back to reads.
//
// The deployed controller protects EVERY consumed FIFO lot (max unit basis),
// not just aggregate average P&L. Every admissible prefix consumes its first
// lot.
use crate::core::payload_hash;
use crate::gates::normalize_official_fill_tick;
use baal_service_runtime::fixed::{instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("OWNED_EXIT_HINT_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}

/// ownedNoExitReadHint({scope, owned, quote, activeIntent=null,
/// exitProtectionPolicy='each_consumed_fifo_lot', now})
pub fn owned_no_exit_read_hint(args: &V) -> V {
    let scope = get(args, "scope");
    let owned = get(args, "owned");
    let quote = get(args, "quote");
    let active_intent = args.get("activeIntent").cloned().unwrap_or(V::Null);
    let exit_protection_policy = args
        .get("exitProtectionPolicy")
        .cloned()
        .unwrap_or(json!("each_consumed_fifo_lot"));
    let now = get(args, "now");
    let base = json!({
        "schemaVersion": 1,
        "mode": "fresh_negative_owned_no_exit_read_hint_v1",
        "at": now,
        "exitProtectionPolicy": exit_protection_policy,
        "readRequired": true,
        "skipPreparationReads": false,
        "orderEligible": false,
        "executionAuthority": false,
        "depthKnown": false,
        "inventoryChanged": false,
        "profitabilityEstablished": false,
    });
    let with_reason = |reason: &str| -> V {
        let mut b = base.clone();
        b.as_object_mut().map(|m| m.insert("reason".into(), json!(reason)));
        b
    };
    if !active_intent.is_null() {
        return with_reason("ACTIVE_INTENT_RECONCILIATION_PRECEDENCE");
    }
    if exit_protection_policy != json!("each_consumed_fifo_lot") {
        return with_reason("OTHER_EXIT_POLICY_REQUIRES_FULL_RECEIPT");
    }
    let attempt: Result<V, String> = (|| {
        let at = instant(now.as_str().unwrap_or(""))?;
        let p = get(owned, "proof").clone();
        let mut body = p.clone();
        let verification_hash = body
            .as_object_mut()
            .and_then(|m| m.shift_remove("verificationHash"))
            .unwrap_or(V::Null);
        need(
            get(scope, "ownerId").as_str() == Some("Codex")
                && get(&p, "ownerId") == get(scope, "ownerId")
                && get(&p, "runId") == get(scope, "runId")
                && get(&p, "tournamentId") == get(scope, "tournamentId")
                && get(&p, "mode").as_str() == Some("scoped_rest_no_multishare_fifo_v1")
                && get(&p, "side").as_str() == Some("no")
                && get(&p, "scopedTradeVerified") == &json!(true)
                && hex(&verification_hash)
                && payload_hash(&body)? == verification_hash.as_str().unwrap_or("")
                && js_int(get(&p, "remainingOwnedQuantity")).is_some_and(|n| n > 0)
                && get(&p, "remainingLots").is_array()
                && !get(&p, "remainingLots").as_array().unwrap().is_empty(),
            "VERIFIED_OWNED_FIFO",
        )?;
        let mut count: i64 = 0;
        for l in get(&p, "remainingLots").as_array().unwrap() {
            need(
                js_int(get(l, "quantity")).is_some_and(|q| q > 0),
                "WHOLE_FIFO_QUANTITY",
            )?;
            price(get(l, "entryPrice").as_str().unwrap_or(""))
                .map_err(|e| e)?;
            count += js_int(get(l, "quantity")).unwrap_or(0);
        }
        need(
            count == js_int(get(&p, "remainingOwnedQuantity")).unwrap_or(-1)
                && instant(get(&p, "readAt").as_str().unwrap_or(""))
                    .map(|t| t <= at)
                    .unwrap_or(false),
            "FIFO_SOURCE",
        )?;
        need(
            quote.is_object()
                && get(quote, "marketId") == get(&p, "marketId")
                && get(quote, "exchangeId") == get(&p, "exchangeId")
                && get(quote, "option").as_str() == Some("YES")
                && get(quote, "tournamentId") == get(&p, "tournamentId")
                && hex(get(quote, "sourceHash"))
                && get(quote, "crossed") == &json!(false),
            "QUOTE_SCOPE",
        )?;
        let start = instant(get(quote, "requestStartedAt").as_str().unwrap_or(""))?;
        let received = instant(get(quote, "receivedAt").as_str().unwrap_or(""))?;
        let known = instant(
            quote
                .get("knownAt")
                .filter(|k| !k.is_null())
                .unwrap_or(get(quote, "receivedAt"))
                .as_str()
                .unwrap_or(""),
        )?;
        need(
            start <= received
                && received <= known
                && known <= at
                && at - start <= 20000
                && received - start <= 20000,
            "FRESH_KNOWN_HINT",
        )?;
        let ask = price(
            normalize_official_fill_tick(get(quote, "bestAsk"))?
                .get("price")
                .and_then(V::as_str)
                .unwrap_or(""),
        )?;
        let bid = price(
            normalize_official_fill_tick(get(quote, "bestBid"))?
                .get("price")
                .and_then(V::as_str)
                .unwrap_or(""),
        )?;
        need(bid < ask, "UNLOCKED_HINT")?;
        let first_cost = price(
            get(&p, "remainingLots")
                .as_array()
                .and_then(|a| a.first())
                .map(|l| get(l, "entryPrice"))
                .and_then(V::as_str)
                .unwrap_or(""),
        )?;
        let required_raw_no_bid = first_cost + 2 * TICK;
        let observed_raw_no_bid = SCALE - ask;
        let observed_protected_limit = observed_raw_no_bid - TICK;
        let negative = observed_raw_no_bid < required_raw_no_bid;
        let mut result = base.clone();
        if let V::Object(m) = &mut result {
            // Insertion order follows the JS {...base, ...} literal.
            m.insert(
                "readRequired".into(),
                json!(!negative),
            );
            m.insert("skipPreparationReads".into(), json!(negative));
            m.insert(
                "reason".into(),
                json!(if negative {
                    "FRESH_BULK_PRICE_CANNOT_CLEAR_FIRST_FIFO_PROTECTION"
                } else {
                    "PRICE_HINT_MAY_CLEAR_FIFO_REQUIRES_FULL_RECEIPT"
                }),
            );
            m.insert("ownerId".into(), get(&p, "ownerId").clone());
            m.insert("runId".into(), get(&p, "runId").clone());
            m.insert("marketId".into(), get(&p, "marketId").clone());
            m.insert("exchangeId".into(), get(&p, "exchangeId").clone());
            m.insert("ownedProofHash".into(), verification_hash.clone());
            m.insert("firstFifoUnitCost".into(), json!(money(first_cost)));
            m.insert(
                "minimumProtectedNetProfitPerShare".into(),
                json!("0.005000"),
            );
            m.insert("protectiveTicks".into(), json!(1));
            m.insert(
                "requiredRawNoBid".into(),
                json!(money(required_raw_no_bid)),
            );
            m.insert(
                "maximumYesAskForPossibleProtectedExit".into(),
                json!(money(SCALE - required_raw_no_bid)),
            );
            m.insert("hintRawNoBid".into(), json!(money(observed_raw_no_bid)));
            m.insert(
                "hintProtectedLimit".into(),
                json!(money(observed_protected_limit)),
            );
            m.insert("quoteSourceHash".into(), get(quote, "sourceHash").clone());
            m.insert("quoteReceivedAt".into(), get(quote, "receivedAt").clone());
            m.insert("hintAgeFromRequestMs".into(), json!(at - start));
            m.insert("fees".into(), json!("0.000000"));
            m.insert(
                "notAnExecutableDepthOrProfitProof".into(),
                json!(true),
            );
            m.insert(
                "onlySellPreparationMayBeSkipped".into(),
                json!(true),
            );
        }
        let hash = payload_hash(&result)?;
        result
            .as_object_mut()
            .map(|m| m.insert("hintHash".into(), json!(hash)));
        Ok(result)
    })();
    match attempt {
        Ok(v) => v,
        Err(e) => {
            // /^OWNED_EXIT_HINT_[A-Z_]+$/.test(e.message) ? e.message : fallback
            let is_hint_code = e.starts_with("OWNED_EXIT_HINT_")
                && e.len() > "OWNED_EXIT_HINT_".len()
                && e["OWNED_EXIT_HINT_".len()..]
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b == b'_');
            let reason = if is_hint_code {
                e
            } else {
                "MISSING_OR_AMBIGUOUS_HINT_FALLBACK".to_string()
            };
            with_reason(&reason)
        }
    }
}
