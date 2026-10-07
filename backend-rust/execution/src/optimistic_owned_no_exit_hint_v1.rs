// execution/optimistic-owned-no-exit-hint-v1.mjs — an advisory rejection bound
// only. It never grants execution or assumes depth. Every failure —
// missing/stale/foreign/corrupt evidence, parse errors, hash mismatches —
// collapses to the base answer with reason HINT_UNAVAILABLE_OR_UNTRUSTWORTHY.
//
// Port notes:
// - `{verificationHash, ...proof} = p ?? {}`: for a non-object p the rest
//   object is {} and verificationHash is undefined; the OWN_PROOF check throws
//   before the hash comparison can matter anyway.
// - `verificationHash !== payloadHash(proof)` — strict !==: a missing key
//   (undefined) or non-string never equals the hash string.
// - `hints.universeHash !== universeHash` — strict === on the raw JSON values.
// - `q.crossed === true` — only a literal true rejects; false/absent pass.
// - instant()/amount()/price() throw inside the try; the catch returns the
//   unavailable answer, so this function is infallible.
use crate::core::payload_hash;
use crate::gates::normalize_official_fill_tick;
use crate::party_winner_no_screen_v1::party_winner_quote_hash;
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Map, Value as V};

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
// JS === on JSON values: primitives equal by value/type; objects, arrays and
// null are only === to themselves, which two parsed values never are.
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(x), V::Number(y)) => {
            x.as_f64().map(|v| v.to_bits()) == y.as_f64().map(|v| v.to_bits())
        }
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
fn hex64(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}

/// optimisticOwnedNoExitHintV1({scope,universeHash,hints,slot,realizedProfit,now})
pub fn optimistic_owned_no_exit_hint_v1(input: &V) -> V {
    let base = json!({
        "defer": false,
        "executionAuthority": false,
        "networkRequests": 0,
        "requiresFreshDepthBeforeOrders": true,
    });
    match evaluate(input, &base) {
        Ok(v) => v,
        Err(_) => {
            let mut out = base.as_object().cloned().unwrap_or_else(Map::new);
            out.insert("reason".into(), json!("HINT_UNAVAILABLE_OR_UNTRUSTWORTHY"));
            V::Object(out)
        }
    }
}

fn evaluate(input: &V, base: &V) -> Result<V, String> {
    let scope = get(input, "scope");
    let at = instant(sget(input, "now").unwrap_or(""))?;
    let p = get(get(input, "slot"), "proof");
    let (verification_hash, proof) = match p {
        V::Object(m) => {
            let vh = m.get("verificationHash").cloned().unwrap_or(V::Null);
            // Rest spread keeps the source key order minus verificationHash.
            let rest: Map<String, V> = m
                .iter()
                .filter(|(k, _)| k.as_str() != "verificationHash")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            (vh, V::Object(rest))
        }
        _ => (V::Null, V::Object(Map::new())),
    };
    if !(get(p, "scopedTradeVerified") == &V::Bool(true)
        && strict_eq(&verification_hash, &json!(payload_hash(&proof)?))
        && strict_eq(get(p, "ownerId"), get(scope, "ownerId"))
        && strict_eq(get(p, "runId"), get(scope, "runId"))
        && strict_eq(get(p, "tournamentId"), get(scope, "tournamentId"))
        && js_int(get(p, "remainingOwnedQuantity")).is_some_and(|q| q >= 1)
        && get(p, "remainingLots")
            .as_array()
            .is_some_and(|a| !a.is_empty()))
    {
        return Err("OWN_PROOF".into());
    }
    let hints = get(input, "hints");
    if !(strict_eq(get(hints, "tournamentId"), get(scope, "tournamentId"))
        && strict_eq(get(hints, "universeHash"), get(input, "universeHash"))
        && get(hints, "quoteCaptures").is_array())
    {
        return Err("ENVELOPE".into());
    }
    let rows: Vec<&V> = get(hints, "quoteCaptures")
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|q| strict_eq(get(q, "exchangeId"), get(p, "exchangeId")))
                .collect()
        })
        .unwrap_or_default();
    if rows.len() != 1 {
        return Err("UNIQUE_QUOTE".into());
    }
    let q = rows[0];
    if !(strict_eq(get(q, "marketId"), get(p, "marketId"))
        && strict_eq(get(q, "tournamentId"), get(scope, "tournamentId"))
        && get(q, "option") == &json!("YES")
        && get(q, "crossed") != &V::Bool(true)
        && hex64(get(q, "sourceHash"))
        && strict_eq(get(q, "quoteHash"), &json!(party_winner_quote_hash(q)?)))
    {
        return Err("QUOTE_SCOPE_HASH".into());
    }
    let start = instant(sget(q, "requestStartedAt").unwrap_or(""))?;
    let received = instant(sget(q, "receivedAt").unwrap_or(""))?;
    let known = instant(sget(q, "knownAt").unwrap_or(""))?;
    if start > received || received > known || known > at || at - start > 20000 {
        return Err("QUOTE_CLOCK".into());
    }
    let bid = normalize_official_fill_tick(get(q, "bestBid"))?;
    let ask = normalize_official_fill_tick(get(q, "bestAsk"))?;
    let yes_bid = amount(sget(&bid, "price").unwrap_or(""))?;
    let yes_ask = amount(sget(&ask, "price").unwrap_or(""))?;
    if yes_bid >= yes_ask {
        return Err("CROSSED".into());
    }
    let optimistic_no_bid = SCALE - yes_ask;
    let first_lot = &get(p, "remainingLots").as_array().unwrap()[0];
    let first_cost = price(sget(first_lot, "entryPrice").unwrap_or(""))?;
    let upper = amount(sget(input, "realizedProfit").unwrap_or(""))? + optimistic_no_bid - first_cost;
    let defer = upper < TICK;
    let mut out = base.as_object().cloned().unwrap_or_else(Map::new);
    out.insert("defer".into(), json!(defer));
    out.insert(
        "reason".into(),
        json!(if defer {
            "FIRST_FIFO_SHARE_FAILS_EVEN_ZERO_SLIPPAGE_BOUND"
        } else {
            "DEPTH_CHECK_STILL_REQUIRED"
        }),
    );
    out.insert("ageMs".into(), json!(at - start));
    out.insert("optimisticNoBid".into(), json!(money(optimistic_no_bid)));
    out.insert("firstFifoCost".into(), json!(money(first_cost)));
    out.insert("optimisticCohortAfterOneShare".into(), json!(money(upper)));
    out.insert("minimumCohortProfit".into(), json!(money(TICK)));
    out.insert("quoteHash".into(), get(q, "quoteHash").clone());
    out.insert("ownedProofHash".into(), verification_hash);
    Ok(V::Object(out))
}
