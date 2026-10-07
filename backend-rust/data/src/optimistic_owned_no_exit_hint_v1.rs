// execution/optimistic-owned-no-exit-hint-v1.mjs — an advisory rejection
// bound only. It never grants execution or assumes depth.
use baal_execution::core::payload_hash;
use baal_execution::gates::normalize_official_fill_tick;
use baal_execution::party_winner_no_screen_v1::party_winner_quote_hash;
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Value as V};

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
// /^[a-f0-9]{64}$/
fn hex64(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(
        v.as_str()
            .ok_or_else(|| "Timestamp must be canonical UTC ISO milliseconds".to_string())?,
    )
}
fn str_amount(v: &V) -> Result<i128, String> {
    amount(
        v.as_str()
            .ok_or_else(|| "Money must be a canonical decimal string with at most six places".to_string())?,
    )
}

fn try_hint(
    scope: &V,
    universe_hash: &str,
    hints: &V,
    slot: &V,
    realized_profit: &V,
    now: &str,
    base: &V,
) -> Result<V, String> {
    let at = instant(now)?;
    let p = get(slot, "proof");
    let mut proof = p.as_object().cloned().unwrap_or_default();
    let verification_hash = proof.shift_remove("verificationHash").unwrap_or(V::Null);
    let proof_v = V::Object(proof);
    let own_ok = get(p, "scopedTradeVerified") == &V::Bool(true)
        && verification_hash == payload_hash(&proof_v)?
        && sget(p, "ownerId") == sget(scope, "ownerId")
        && sget(p, "runId") == sget(scope, "runId")
        && sget(p, "tournamentId") == sget(scope, "tournamentId")
        && js_int(get(p, "remainingOwnedQuantity")).is_some_and(|n| n >= 1)
        && get(p, "remainingLots")
            .as_array()
            .is_some_and(|a| !a.is_empty());
    if !own_ok {
        return Err("OWN_PROOF".into());
    }
    if sget(hints, "tournamentId") != sget(scope, "tournamentId")
        || sget(hints, "universeHash") != Some(universe_hash)
        || !get(hints, "quoteCaptures").is_array()
    {
        return Err("ENVELOPE".into());
    }
    let rows: Vec<V> = get(hints, "quoteCaptures")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|q| sget(q, "exchangeId") == sget(p, "exchangeId"))
        .collect();
    if rows.len() != 1 {
        return Err("UNIQUE_QUOTE".into());
    }
    let q = &rows[0];
    let quote_hash = party_winner_quote_hash(q)?;
    if sget(q, "marketId") != sget(p, "marketId")
        || sget(q, "tournamentId") != sget(scope, "tournamentId")
        || sget(q, "option") != Some("YES")
        || get(q, "crossed") == &V::Bool(true)
        || !hex64(get(q, "sourceHash"))
        || sget(q, "quoteHash") != Some(quote_hash.as_str())
    {
        return Err("QUOTE_SCOPE_HASH".into());
    }
    let start = instant_v(get(q, "requestStartedAt"))?;
    let received = instant_v(get(q, "receivedAt"))?;
    let known = instant_v(get(q, "knownAt"))?;
    if start > received || received > known || known > at || at - start > 20000 {
        return Err("QUOTE_CLOCK".into());
    }
    let yes_bid = str_amount(get(&normalize_official_fill_tick(get(q, "bestBid"))?, "price"))?;
    let yes_ask = str_amount(get(&normalize_official_fill_tick(get(q, "bestAsk"))?, "price"))?;
    if yes_bid >= yes_ask {
        return Err("CROSSED".into());
    }
    let optimistic_no_bid = SCALE - yes_ask;
    let first_lot = &get(p, "remainingLots").as_array().cloned().unwrap_or_default()[0];
    let first_cost = price(
        sget(first_lot, "entryPrice")
            .ok_or_else(|| "Price must be 0.005..0.995 on the 0.005 tick".to_string())?,
    )?;
    let upper = str_amount(realized_profit)? + optimistic_no_bid - first_cost;
    let defer = upper < TICK;
    let mut o = base.as_object().cloned().unwrap_or_default();
    o.insert("defer".into(), json!(defer));
    o.insert(
        "reason".into(),
        json!(if defer {
            "FIRST_FIFO_SHARE_FAILS_EVEN_ZERO_SLIPPAGE_BOUND"
        } else {
            "DEPTH_CHECK_STILL_REQUIRED"
        }),
    );
    o.insert("ageMs".into(), json!(at - start));
    o.insert("optimisticNoBid".into(), json!(money(optimistic_no_bid)));
    o.insert("firstFifoCost".into(), json!(money(first_cost)));
    o.insert("optimisticCohortAfterOneShare".into(), json!(money(upper)));
    o.insert("minimumCohortProfit".into(), json!(money(TICK)));
    o.insert("quoteHash".into(), get(q, "quoteHash").clone());
    o.insert("ownedProofHash".into(), verification_hash);
    Ok(V::Object(o))
}

/// optimisticOwnedNoExitHintV1({scope,universeHash,hints,slot,realizedProfit,now})
/// `realizedProfit` is the JS money-string value carried as a JSON value so a
/// non-string input fails into the catch arm exactly like `amount(undefined)`.
/// Any failure inside the JS try returns the base object with
/// reason 'HINT_UNAVAILABLE_OR_UNTRUSTWORTHY'.
pub fn optimistic_owned_no_exit_hint_v1(
    scope: &V,
    universe_hash: &str,
    hints: &V,
    slot: &V,
    realized_profit: &V,
    now: &str,
) -> V {
    let base = json!({
        "defer": false,
        "executionAuthority": false,
        "networkRequests": 0,
        "requiresFreshDepthBeforeOrders": true,
    });
    match try_hint(scope, universe_hash, hints, slot, realized_profit, now, &base) {
        Ok(v) => v,
        Err(_) => {
            let mut o = base.as_object().cloned().unwrap_or_default();
            o.insert(
                "reason".into(),
                json!("HINT_UNAVAILABLE_OR_UNTRUSTWORTHY"),
            );
            V::Object(o)
        }
    }
}
