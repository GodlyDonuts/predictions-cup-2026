// execution/baal-scaled-owned-exit-read-hint-v1.mjs — a hint may SAVE a failed
// read packet, never grant a trade, valuation mark or waive reconciliation.
// Missing/stale/ambiguous evidence falls back to reads.
//
// Port notes:
// - catch: only OWNED_EXIT_HINT_* codes surface; anything else becomes
//   'MISSING_OR_AMBIGUOUS_HINT_FALLBACK'.
// - `{verificationHash, ...body} = p ?? {}` — on a non-object p the rest object
//   is {} and verificationHash is undefined (hex fails first, short-circuit).
// - `exitProtectionPolicy` destructure default applies only when absent
//   (undefined), not null.
// - `quote.knownAt ?? quote.receivedAt` — nullish fallback (null or absent).
use crate::core::payload_hash;
use crate::gates::normalize_official_fill_tick;
use baal_service_runtime::fixed::{instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Map, Value as V};

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
    x.as_str()
        .map(|s| {
            s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        })
        .unwrap_or(false)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn price_v(v: &V) -> Result<i128, String> {
    price(v.as_str().unwrap_or(""))
}
fn owned_code(e: &str) -> bool {
    e.len() > "OWNED_EXIT_HINT_".len()
        && e.starts_with("OWNED_EXIT_HINT_")
        && e["OWNED_EXIT_HINT_".len()..]
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b == b'_')
}

/// ownedNoExitReadHint({scope,owned,quote,activeIntent=null,
/// exitProtectionPolicy='each_consumed_fifo_lot',now})
pub fn owned_no_exit_read_hint(input: &V) -> V {
    let scope = get(input, "scope");
    let owned = get(input, "owned");
    let quote = get(input, "quote");
    let active_intent = input.get("activeIntent").cloned().unwrap_or(V::Null);
    let exit_protection_policy = input
        .get("exitProtectionPolicy")
        .cloned()
        .unwrap_or(json!("each_consumed_fifo_lot"));
    let now = get(input, "now").clone();
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
    if !active_intent.is_null() {
        let mut out = base.as_object().cloned().unwrap_or_else(Map::new);
        out.insert("reason".into(), json!("ACTIVE_INTENT_RECONCILIATION_PRECEDENCE"));
        return V::Object(out);
    }
    if exit_protection_policy != json!("each_consumed_fifo_lot") {
        let mut out = base.as_object().cloned().unwrap_or_else(Map::new);
        out.insert("reason".into(), json!("OTHER_EXIT_POLICY_REQUIRES_FULL_RECEIPT"));
        return V::Object(out);
    }
    match inner(scope, owned, quote, &now, &base) {
        Ok(v) => v,
        Err(e) => {
            let mut out = base.as_object().cloned().unwrap_or_else(Map::new);
            out.insert(
                "reason".into(),
                json!(if owned_code(&e) {
                    e
                } else {
                    "MISSING_OR_AMBIGUOUS_HINT_FALLBACK".to_string()
                }),
            );
            V::Object(out)
        }
    }
}

fn inner(scope: &V, owned: &V, quote: &V, now: &V, base: &V) -> Result<V, String> {
    let at = instant_v(now)?;
    let p = get(owned, "proof");
    // const {verificationHash, ...body} = p ?? {}
    let mut body = if p.is_object() {
        p.as_object().cloned().unwrap_or_else(Map::new)
    } else {
        Map::new()
    };
    let verification_hash = body.remove("verificationHash").unwrap_or(V::Null);
    need(
        get(scope, "ownerId").as_str() == Some("Codex")
            && get(p, "ownerId") == get(scope, "ownerId")
            && get(p, "runId") == get(scope, "runId")
            && get(p, "tournamentId") == get(scope, "tournamentId")
            && get(p, "mode").as_str() == Some("scoped_rest_no_multishare_fifo_v1")
            && get(p, "side").as_str() == Some("no")
            && get(p, "scopedTradeVerified") == &json!(true)
            && hex(&verification_hash)
            && payload_hash(&V::Object(body))? == verification_hash.as_str().unwrap_or("")
            && js_int(get(p, "remainingOwnedQuantity")).map(|q| q > 0).unwrap_or(false)
            && get(p, "remainingLots").as_array().map(|a| !a.is_empty()).unwrap_or(false),
        "VERIFIED_OWNED_FIFO",
    )?;
    let mut count: i64 = 0;
    for l in get(p, "remainingLots").as_array().cloned().unwrap_or_default() {
        need(
            js_int(get(&l, "quantity")).map(|q| q > 0).unwrap_or(false),
            "WHOLE_FIFO_QUANTITY",
        )?;
        price_v(get(&l, "entryPrice"))?;
        count += js_int(get(&l, "quantity")).unwrap();
    }
    need(
        count == js_int(get(p, "remainingOwnedQuantity")).unwrap_or(i64::MIN)
            && instant_v(get(p, "readAt"))? <= at,
        "FIFO_SOURCE",
    )?;
    need(
        js_truthy_for_quote(quote)
            && get(quote, "marketId") == get(p, "marketId")
            && get(quote, "exchangeId") == get(p, "exchangeId")
            && get(quote, "option").as_str() == Some("YES")
            && get(quote, "tournamentId") == get(p, "tournamentId")
            && hex(get(quote, "sourceHash"))
            && get(quote, "crossed") == &json!(false),
        "QUOTE_SCOPE",
    )?;
    let start = instant_v(get(quote, "requestStartedAt"))?;
    let received = instant_v(get(quote, "receivedAt"))?;
    let known_src = {
        let k = get(quote, "knownAt");
        if k.is_null() {
            get(quote, "receivedAt")
        } else {
            k
        }
    };
    let known = instant_v(known_src)?;
    need(
        start <= received
            && received <= known
            && known <= at
            && at - start <= 20000
            && received - start <= 20000,
        "FRESH_KNOWN_HINT",
    )?;
    let ask = price_v(get(&normalize_official_fill_tick(get(quote, "bestAsk"))?, "price"))?;
    let bid = price_v(get(&normalize_official_fill_tick(get(quote, "bestBid"))?, "price"))?;
    need(bid < ask, "UNLOCKED_HINT")?;
    let first_cost = price_v(get(&get(p, "remainingLots")[0], "entryPrice"))?;
    let required_raw_no_bid = first_cost + 2 * TICK;
    let observed_raw_no_bid = SCALE - ask;
    let observed_protected_limit = observed_raw_no_bid - TICK;
    let negative = observed_raw_no_bid < required_raw_no_bid;
    let mut result = base.as_object().cloned().unwrap_or_else(Map::new);
    for (k, v) in [
        ("readRequired", json!(!negative)),
        ("skipPreparationReads", json!(negative)),
        (
            "reason",
            json!(if negative {
                "FRESH_BULK_PRICE_CANNOT_CLEAR_FIRST_FIFO_PROTECTION"
            } else {
                "PRICE_HINT_MAY_CLEAR_FIFO_REQUIRES_FULL_RECEIPT"
            }),
        ),
        ("ownerId", get(p, "ownerId").clone()),
        ("runId", get(p, "runId").clone()),
        ("marketId", get(p, "marketId").clone()),
        ("exchangeId", get(p, "exchangeId").clone()),
        ("ownedProofHash", verification_hash.clone()),
        ("firstFifoUnitCost", json!(money(first_cost))),
        (
            "minimumProtectedNetProfitPerShare",
            json!("0.005000"),
        ),
        ("protectiveTicks", json!(1)),
        ("requiredRawNoBid", json!(money(required_raw_no_bid))),
        (
            "maximumYesAskForPossibleProtectedExit",
            json!(money(SCALE - required_raw_no_bid)),
        ),
        ("hintRawNoBid", json!(money(observed_raw_no_bid))),
        (
            "hintProtectedLimit",
            json!(money(observed_protected_limit)),
        ),
        ("quoteSourceHash", get(quote, "sourceHash").clone()),
        ("quoteReceivedAt", get(quote, "receivedAt").clone()),
        ("hintAgeFromRequestMs", json!(at - start)),
        ("fees", json!("0.000000")),
        (
            "notAnExecutableDepthOrProfitProof",
            json!(true),
        ),
        ("onlySellPreparationMayBeSkipped", json!(true)),
    ] {
        result.insert(k.to_string(), v);
    }
    let mut out = result.clone();
    out.insert("hintHash".into(), json!(payload_hash(&V::Object(result))?));
    Ok(V::Object(out))
}

// quote && ... — the JS truthy check on the quote value itself.
fn js_truthy_for_quote(v: &V) -> bool {
    baal_service_runtime::json_text::js_truthy(Some(v))
}
