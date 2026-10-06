// live-paper/passive-observer.mjs — pure market research: no orders, fill
// simulation, cash or PnL.
use num_bigint::BigInt;
use serde_json::{json, Value as V};

fn canonical_digest(x: &V) -> String {
    baal_service_runtime::json_text::sha256(
        baal_service_runtime::json_text::canonical_json(x).unwrap_or_default().as_bytes())
}
fn check(v: bool, message: &str) -> Result<(), String> {
    if !v { return Err(message.to_string()); }
    Ok(())
}
fn is_id(v: &V) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty() && !s.starts_with('0')
        && s.bytes().all(|b| b.is_ascii_digit()))
}
fn is_identity(v: &V) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 100
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
}
fn inc(s: &mut V, key: &str) {
    let c = s["counts"][key].as_i64().unwrap_or(0) + 1;
    s["counts"][key] = json!(c);
}
fn instant_ms(v: &V) -> i64 {
    baal_paper::fixed::instant(v.as_str().unwrap_or("")).unwrap_or(i64::MIN)
}
/// PASSIVE_DEFAULTS
pub fn passive_defaults() -> V {
    json!({"latencyMs":500,"maxDataAgeMs":20000,"maxSourceClockSkewMs":2000,
        "quoteLifetimeMs":60000,"quoteEveryMs":60000,"horizonsMs":[60000,120000,300000],
        "markoutLatenessMs":20000,"orderQuantity":10,"slippageTicks":1,
        "maxQuotePairs":1000,"maxTradeIds":100000})
}
fn checked_config(input: Option<&V>) -> Result<V, String> {
    let defaults = passive_defaults();
    let input = input.cloned().unwrap_or(json!({}));
    check(input.as_object().map(|m| m.keys().all(|k|
        defaults.as_object().unwrap().contains_key(k))).unwrap_or(false),
        "Unknown passive observer config")?;
    let mut c = serde_json::Map::new();
    for (k, v) in defaults.as_object().unwrap() { c.insert(k.clone(), v.clone()); }
    for (k, v) in input.as_object().unwrap() { c.insert(k.clone(), v.clone()); }
    let c = V::Object(c);
    for (key, n) in c.as_object().unwrap() {
        if key != "horizonsMs" {
            check(n.as_i64().is_some_and(|v| v > 0), &format!("Invalid config {}", key))?;
        }
    }
    check(c["maxSourceClockSkewMs"].as_i64().unwrap_or(0) <= 2000
        && c["maxDataAgeMs"].as_i64().unwrap_or(0) <= 60000
        && c["latencyMs"].as_i64().unwrap_or(0) < c["quoteLifetimeMs"].as_i64().unwrap_or(0)
        && c["quoteLifetimeMs"].as_i64().unwrap_or(0) <= 300000
        && c["quoteEveryMs"].as_i64().unwrap_or(0) >= c["quoteLifetimeMs"].as_i64().unwrap_or(0)
        && c["orderQuantity"].as_i64().unwrap_or(0) <= 100
        && c["slippageTicks"].as_i64().unwrap_or(0) <= 5
        && c["maxQuotePairs"].as_i64().unwrap_or(0) <= 10000
        && c["maxTradeIds"].as_i64().unwrap_or(0) <= 100000, "Unsafe observer bounds")?;
    let h = &c["horizonsMs"];
    check(h.is_array() && !h.as_array().unwrap().is_empty() && h.as_array().unwrap().len() <= 10
        && h.as_array().unwrap().iter().enumerate().all(|(i, v)|
            v.as_i64().is_some_and(|x| x > c["latencyMs"].as_i64().unwrap_or(0) && x <= 3600000
                && (i == 0 || x > h.as_array().unwrap()[i - 1].as_i64().unwrap_or(0)))),
        "Invalid markout horizons")?;
    Ok(c)
}
/// createPassiveObserver({marketId, exchangeId, tournamentId, sourceHash, cutoff, config})
pub fn create_passive_observer(market_id: &str, exchange_id: &str, tournament_id: &str,
    source_hash: &str, cutoff: &str, config: Option<&V>) -> Result<V, String> {
    check(is_id(&json!(market_id)) && is_id(&json!(exchange_id))
        && tournament_id.len() == 36
        && tournament_id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase() || b == b'-'),
        "Invalid market scope")?;
    check(source_hash.len() == 64 && source_hash.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "Frozen source hash required")?;
    baal_paper::fixed::instant(cutoff)?;
    let c = checked_config(config)?;
    let bank = json!({"schemaVersion":1,"name":"passive-opportunity-observer-v1","mode":"measurement_only",
        "marketId":market_id,"exchangeId":exchange_id,"tournamentId":tournament_id,"cutoff":cutoff,
        "sourceHash":source_hash,"config":c,"configHash":canonical_digest(&c)});
    Ok(json!({"bank": bank, "state": {"schemaVersion":1,"bankHash":canonical_digest(&bank),
        "index":0,"chainHash":source_hash,"lastInputHash":V::Null,"lastAt":V::Null,"epoch":V::Null,
        "revision":V::Null,"ready":false,"lastBook":V::Null,"quotes":[],"lastQuoteAt":V::Null,
        "seenTrades":{},"lastTradeSequence":V::Null,"lastTradeSourceNs":V::Null,"pairs":0,"counts":{}}}))
}
fn assert_state(s: &V, bank: &V) -> Result<(), String> {
    check(bank["name"].as_str() == Some("passive-opportunity-observer-v1")
        && bank["mode"].as_str() == Some("measurement_only")
        && bank["configHash"].as_str() == Some(canonical_digest(&checked_config(Some(&bank["config"]))?).as_str())
        && s["bankHash"].as_str() == Some(canonical_digest(bank).as_str())
        && s["schemaVersion"].as_i64() == Some(1), "Frozen observer bank changed")?;
    check(s["index"].as_i64().is_some_and(|i| i >= 0) && s["quotes"].is_array(),
        "Invalid observer state")
}
fn normalized(input: &V, bank: &V) -> Result<V, String> {
    check(input["index"].as_i64().is_some_and(|i| i >= 1)
        && ["book", "trade", "invalidate", "clock"].contains(&input["kind"].as_str().unwrap_or("")),
        "Invalid observer event")?;
    let at = baal_paper::fixed::instant(input["receivedAt"].as_str().unwrap_or(""))?;
    let c = &input["continuity"];
    check(c.is_object() && is_identity(&c["epoch"])
        && c["revision"].as_i64().is_some_and(|r| r >= 0)
        && c["previousRevision"].as_i64().is_some_and(|p| p >= 0
            && p <= c["revision"].as_i64().unwrap_or(-1))
        && c["ready"].is_boolean() && c["continuous"].is_boolean()
        && (c["recovered"].is_null() || c["recovered"].is_boolean()),
        "Invalid continuity metadata")?;
    let mut e = json!({"index": input["index"], "kind": input["kind"], "receivedAt": input["receivedAt"],
        "continuity": {"epoch": c["epoch"], "revision": c["revision"],
            "previousRevision": c["previousRevision"], "ready": c["ready"],
            "continuous": c["continuous"], "recovered": c["recovered"] == V::Bool(true)}});
    if e["kind"].as_str() == Some("invalidate") {
        check(["gap", "disconnect", "stale", "resync", "settlement", "schema", "book_dirty"]
            .contains(&input["reason"].as_str().unwrap_or("")), "Invalid invalidation reason")?;
        e["reason"] = input["reason"].clone();
    }
    if e["kind"].as_str() == Some("book") {
        let b = &input["book"];
        check(b["marketId"] == bank["marketId"] && b["exchangeId"] == bank["exchangeId"]
            && b["instrument"].as_str()
                == Some(format!("{}:YES", bank["exchangeId"].as_str().unwrap_or("")).as_str())
            && b["receivedAt"] == e["receivedAt"], "Book scope mismatch")?;
        let request = baal_paper::fixed::instant(b["requestStartedAt"].as_str().unwrap_or(""))?;
        baal_paper::fixed::big_sequence(b["sequence"].as_str().unwrap_or(""))?;
        check(request <= at && at - request <= bank["config"]["maxDataAgeMs"].as_i64().unwrap_or(0),
            "Invalid book request clock")?;
        let source_at = baal_service_runtime::contract::timestamp_str(
            b["sourceAt"].as_str().unwrap_or("")).map_err(|e| e.to_string())?;
        check(b["versionKey"].as_str() == Some(format!("{}:{}",
            b["sequence"].as_str().unwrap_or(""), source_at).as_str())
            && baal_service_runtime::contract::instant_ns(&source_at).map_err(|e| e.to_string())?
                <= i128::from(at + bank["config"]["maxSourceClockSkewMs"].as_i64().unwrap_or(0)) * 1_000_000,
            "Invalid book source clock")?;
        let mut book = json!({"marketId": b["marketId"], "exchangeId": b["exchangeId"],
            "instrument": b["instrument"], "requestStartedAt": b["requestStartedAt"],
            "receivedAt": b["receivedAt"], "sourceAt": source_at, "sequence": b["sequence"],
            "versionKey": b["versionKey"]});
        for (side, direction) in [("bids", -1i64), ("asks", 1i64)] {
            let levels = &b[side];
            check(levels.is_array() && !levels.as_array().unwrap().is_empty()
                && levels.as_array().unwrap().len() <= 200, "Missing bounded depth")?;
            let mut out = Vec::new();
            for (i, l) in levels.as_array().unwrap().iter().enumerate() {
                let p = baal_paper::fixed::big_price(l["price"].as_str().unwrap_or(""))?;
                check(l["quantity"].as_i64().is_some_and(|q| q > 0), "Invalid depth quantity")?;
                if i > 0 {
                    let prev = baal_paper::fixed::big_price(
                        levels.as_array().unwrap()[i - 1]["price"].as_str().unwrap_or(""))?;
                    check(if direction == -1 { p < prev } else { p > prev }, "Unsorted depth")?;
                }
                out.push(json!({"price": baal_paper::fixed::money(&p), "quantity": l["quantity"]}));
            }
            book[side] = json!(out);
        }
        check(baal_paper::fixed::big_price(book["bids"][0]["price"].as_str().unwrap_or(""))?
            < baal_paper::fixed::big_price(book["asks"][0]["price"].as_str().unwrap_or(""))?,
            "Crossed or locked book")?;
        e["book"] = book;
    }
    if e["kind"].as_str() == Some("trade") {
        let t = &input["trade"];
        check(t["marketId"] == bank["marketId"] && t["exchangeId"] == bank["exchangeId"]
            && t["tournamentId"] == bank["tournamentId"], "Trade scope mismatch")?;
        check(is_identity(&t["id"]) && t["quantity"].as_i64().is_some_and(|q| q > 0),
            "Ordered stable trade identity and integer size required")?;
        baal_paper::fixed::big_sequence(t["sequence"].as_str().unwrap_or(""))?;
        let p = baal_paper::fixed::big_amount(t["price"].as_str().unwrap_or(""))?;
        let source_at = baal_service_runtime::contract::timestamp_str(
            t["sourceAt"].as_str().unwrap_or("")).map_err(|e| e.to_string())?;
        check(p >= BigInt::from(0) && p <= BigInt::from(baal_paper::fixed::SCALE)
            && baal_service_runtime::contract::instant_ns(&source_at).map_err(|e| e.to_string())?
                <= i128::from(at + bank["config"]["maxSourceClockSkewMs"].as_i64().unwrap_or(0)) * 1_000_000,
            "Invalid trade price or clock")?;
        e["trade"] = json!({"id": t["id"], "sequence": t["sequence"], "marketId": t["marketId"],
            "exchangeId": t["exchangeId"], "tournamentId": t["tournamentId"],
            "price": baal_paper::fixed::money(&p), "quantity": t["quantity"], "sourceAt": source_at});
    }
    Ok(e)
}
fn censor(s: &mut V, reason: &str, at: &str, events: &mut Vec<V>) {
    s["ready"] = json!(false);
    for q in s["quotes"].as_array_mut().map(|a| a.iter_mut().collect::<Vec<_>>()).unwrap_or_default() {
        if q["closedAt"].is_null() {
            q["closedAt"] = json!(at);
            q["closedReason"] = json!(reason);
        }
        for (h, m) in q["markouts"].as_object().cloned().unwrap_or_default() {
            if m["status"].as_str() == Some("pending") {
                q["markouts"][&h] = json!({"status":"censored_continuity","reason":reason,"at":at});
            }
        }
    }
    inc(s, "continuityInvalidations");
    events.push(json!({"kind":"continuity_invalidated","reason":reason,"at":at}));
}
fn age(s: &mut V, at: &str, bank: &V) {
    let now = baal_paper::fixed::instant(at).unwrap_or(0);
    for q in s["quotes"].as_array_mut().map(|a| a.iter_mut().collect::<Vec<_>>()).unwrap_or_default() {
        if q["closedAt"].is_null() && now >= instant_ms(&q["expiresAt"]) {
            q["closedAt"] = q["expiresAt"].clone();
            q["closedReason"] = json!("hypothesis_expired");
        }
        for (h, m) in q["markouts"].as_object().cloned().unwrap_or_default() {
            if m["status"].as_str() == Some("pending")
                && now > instant_ms(&q["createdAt"]) + h.parse::<i64>().unwrap_or(0)
                    + bank["config"]["markoutLatenessMs"].as_i64().unwrap_or(0) {
                q["markouts"][&h] = json!({"status":"missing_fresh_followup","at":at});
            }
        }
    }
}
fn same_or_better(book: &V, q: &V) -> BigInt {
    let bid_side = q["side"].as_str() == Some("bid");
    let levels = if bid_side { &book["bids"] } else { &book["asks"] };
    let q_price = baal_paper::fixed::big_amount(q["price"].as_str().unwrap_or("")).unwrap_or_default();
    levels.as_array().cloned().unwrap_or_default().iter().fold(BigInt::from(0), |n, l| {
        let p = baal_paper::fixed::big_amount(l["price"].as_str().unwrap_or("")).unwrap_or_default();
        let qualifies = if bid_side { p >= q_price } else { p <= q_price };
        if qualifies { n + BigInt::from(l["quantity"].as_i64().unwrap_or(0)) } else { n }
    })
}
fn process_book(s: &mut V, e: &V, bank: &V, events: &mut Vec<V>) -> Result<(), String> {
    let b = &e["book"];
    let c = &bank["config"];
    let at = baal_paper::fixed::instant(e["receivedAt"].as_str().unwrap_or(""))?;
    let source_ns = baal_service_runtime::contract::instant_ns(b["sourceAt"].as_str().unwrap_or(""))
        .map_err(|e| e.to_string())?;
    let previous = s["lastBook"].clone();
    if !previous.is_null() {
        check(BigInt::from(previous["sequence"].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0))
            <= BigInt::from(b["sequence"].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0))
            && source_ns >= baal_service_runtime::contract::instant_ns(
                previous["sourceAt"].as_str().unwrap_or("")).map_err(|e| e.to_string())?,
            "Book source rollback")?;
        if previous["versionKey"] == b["versionKey"] {
            check(canonical_digest(&json!({"bids": previous["bids"], "asks": previous["asks"]}))
                == canonical_digest(&json!({"bids": b["bids"], "asks": b["asks"]})),
                "Conflicting equal book version")?;
        }
    }
    let independent = previous.is_null()
        || (source_ns > baal_service_runtime::contract::instant_ns(
            previous["sourceAt"].as_str().unwrap_or("")).map_err(|e| e.to_string())?
            && b["versionKey"] != previous["versionKey"]);
    s["lastBook"] = b.clone();
    inc(s, "bookObservations");
    if s["ready"] != V::Bool(true) || !independent {
        inc(s, if s["ready"] != V::Bool(true) { "unreadyBooks" } else { "repeatedBooks" });
        return Ok(());
    }
    let bid = baal_paper::fixed::big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?;
    let ask = baal_paper::fixed::big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))?;
    let b_seq: BigInt = b["sequence"].as_str().unwrap_or("0").parse().unwrap_or_default();
    for qi in 0..s["quotes"].as_array().map(|a| a.len()).unwrap_or(0) {
        let q = s["quotes"][qi].clone();
        if q["epoch"] != s["epoch"] || at <= instant_ms(&q["createdAt"])
            || source_ns <= i128::from(q["sourceNs"].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0))
            || b_seq <= q["sequence"].as_str().unwrap_or("0").parse::<BigInt>().unwrap_or_default()
            || e["continuity"]["revision"].as_i64().unwrap_or(0) <= q["revision"].as_i64().unwrap_or(0)
            || baal_paper::fixed::instant(b["requestStartedAt"].as_str().unwrap_or("")).unwrap_or(i64::MIN)
                < baal_paper::fixed::instant(q["activationAt"].as_str().unwrap_or("")).unwrap_or(i64::MIN) {
            continue;
        }
        let mut q = q;
        if q["closedAt"].is_null() {
            let visible = same_or_better(b, &q);
            q["visibleSameOrBetterNow"] = json!(visible.to_string());
            let hwm: BigInt = q["visibleQueueAheadHighWater"].as_str().unwrap_or("0").parse().unwrap_or_default();
            if visible > hwm { q["visibleQueueAheadHighWater"] = json!(visible.to_string()); }
            let q_price = baal_paper::fixed::big_amount(q["price"].as_str().unwrap_or(""))?;
            let crossed = if q["side"].as_str() == Some("bid") { ask <= q_price } else { bid >= q_price };
            if crossed && q["firstOppositeQuoteTouchOrCrossAt"].is_null() {
                q["firstOppositeQuoteTouchOrCrossAt"] = e["receivedAt"].clone();
                inc(s, "oppositeQuoteTouchesOrCrossings");
            }
            if q["activationStatus"].as_str() == Some("awaiting_independent_book")
                && at >= baal_paper::fixed::instant(q["activationAt"].as_str().unwrap_or("")).unwrap_or(i64::MAX) {
                q["activationStatus"] = json!(if crossed { "marketable_at_first_followup" } else { "observed_nonmarketable" });
                q["activationObservedAt"] = e["receivedAt"].clone();
                q["activationSourceNs"] = json!(source_ns.to_string());
                q["activationSequence"] = b["sequence"].clone();
                q["activationRevision"] = e["continuity"]["revision"].clone();
                if crossed {
                    q["closedAt"] = e["receivedAt"].clone();
                    q["closedReason"] = json!("not_passive_at_activation_observation");
                }
                events.push(json!({"kind":"activation_observation","quoteId":q["id"],
                    "status":q["activationStatus"],"at":e["receivedAt"]}));
            }
        }
        for (h, m) in q["markouts"].as_object().cloned().unwrap_or_default() {
            if m["status"].as_str() == Some("pending")
                && at >= instant_ms(&q["createdAt"]) + h.parse::<i64>().unwrap_or(0) {
                let quote_price = baal_paper::fixed::big_amount(q["price"].as_str().unwrap_or(""))?;
                let slip = BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0))
                    * BigInt::from(baal_paper::fixed::TICK);
                let mid_twice = if q["side"].as_str() == Some("bid") {
                    &bid + &ask - BigInt::from(2) * &quote_price
                } else {
                    BigInt::from(2) * &quote_price - &bid - &ask
                };
                let exec_move = if q["side"].as_str() == Some("bid") {
                    &bid - &slip - &quote_price
                } else {
                    &quote_price - &ask - &slip
                };
                let markout = json!({"status":"observed_unexecuted_quote_markout","at":e["receivedAt"],
                    "sourceVersion":b["versionKey"],
                    "latenessMs": at - instant_ms(&q["createdAt"]) - h.parse::<i64>().unwrap_or(0),
                    "signedMidpointMove": baal_paper::fixed::money(&(mid_twice / BigInt::from(2))),
                    "signedOppositeExecutableMoveAfterCushion": baal_paper::fixed::money(&exec_move),
                    "afterQuoteCross": !q["firstOppositeQuoteTouchOrCrossAt"].is_null(),
                    "afterQualifiedTradeThrough": !q["firstQualifiedTradeThroughAt"].is_null()});
                q["markouts"][&h] = markout.clone();
                inc(s, "observedMarkouts");
                let mut ev = json!({"kind":"quote_markout","quoteId":q["id"],
                    "horizonMs": h.parse::<i64>().unwrap_or(0)});
                for (k, v) in markout.as_object().unwrap() { ev[k] = v.clone(); }
                events.push(ev);
            }
        }
        s["quotes"][qi] = q;
    }
    if at <= instant_ms(&bank["cutoff"]) || instant_ms(&b["requestStartedAt"]) <= instant_ms(&bank["cutoff"]) {
        inc(s, "developmentBooks");
        return Ok(());
    }
    if !s["lastQuoteAt"].is_null()
        && at - instant_ms(&s["lastQuoteAt"]) < c["quoteEveryMs"].as_i64().unwrap_or(0) {
        return Ok(());
    }
    if s["pairs"].as_i64().unwrap_or(0) >= c["maxQuotePairs"].as_i64().unwrap_or(0) {
        inc(s, "quoteCapacityReached");
        return Ok(());
    }
    let tick = BigInt::from(baal_paper::fixed::TICK);
    let scale = BigInt::from(baal_paper::fixed::SCALE);
    let inside_bid = &bid + &tick;
    let inside_ask = &ask - &tick;
    if &ask - &bid < BigInt::from(2) * &tick || inside_bid >= inside_ask
        || inside_bid < tick || inside_ask > &scale - &tick {
        inc(s, "noDistinctInteriorPrices");
        return Ok(());
    }
    for (side, p) in [("bid", inside_bid.clone()), ("ask", inside_ask.clone())] {
        let mut q = json!({"id": canonical_digest(&json!({"bankHash": s["bankHash"],
                "version": b["versionKey"], "at": e["receivedAt"], "side": side})),
            "side": side, "price": baal_paper::fixed::money(&p), "quantity": c["orderQuantity"],
            "createdAt": e["receivedAt"],
            "activationAt": baal_service_runtime::contract::ms_to_iso(
                (at + c["latencyMs"].as_i64().unwrap_or(0)) as f64),
            "expiresAt": baal_service_runtime::contract::ms_to_iso(
                (at + c["quoteLifetimeMs"].as_i64().unwrap_or(0)) as f64),
            "epoch": s["epoch"], "revision": e["continuity"]["revision"],
            "sequence": b["sequence"], "sourceNs": source_ns.to_string(),
            "sourceVersion": b["versionKey"],
            "activationStatus": "awaiting_independent_book", "activationObservedAt": V::Null,
            "closedAt": V::Null, "closedReason": V::Null,
            "visibleSameOrBetterNow": "0", "visibleQueueAheadHighWater": "0",
            "qualifiedTradeThroughQuantity": "0", "qualifiedTradeThroughEvents": 0,
            "firstOppositeQuoteTouchOrCrossAt": V::Null, "firstQualifiedTradeThroughAt": V::Null,
            "markouts": c["horizonsMs"].as_array().cloned().unwrap_or_default().into_iter()
                .map(|h| (h.as_i64().unwrap_or(0).to_string(), json!({"status":"pending"})))
                .collect::<serde_json::Map<String, V>>()});
        let visible = same_or_better(b, &q).to_string();
        q["visibleSameOrBetterNow"] = json!(visible);
        q["visibleQueueAheadHighWater"] = q["visibleSameOrBetterNow"].clone();
        let quotes = s["quotes"].as_array_mut().unwrap();
        quotes.push(q.clone());
        events.push(json!({"kind":"hypothetical_quote","quoteId":q["id"],"side":side,
            "price":q["price"],"quantity":q["quantity"],"at":e["receivedAt"],
            "activationAt":q["activationAt"],"visibleQueueAhead":q["visibleQueueAheadHighWater"],
            "executionClaim":"none"}));
    }
    s["pairs"] = json!(s["pairs"].as_i64().unwrap_or(0) + 1);
    s["lastQuoteAt"] = e["receivedAt"].clone();
    inc(s, "quotePairs");
    Ok(())
}
fn process_trade(s: &mut V, e: &V, bank: &V, events: &mut Vec<V>) -> Result<(), String> {
    let t = &e["trade"];
    let key = canonical_digest(t);
    let old = s["seenTrades"][t["id"].as_str().unwrap_or("")].clone();
    if !old.is_null() {
        check(old.as_str() == Some(key.as_str()), "Conflicting duplicate trade")?;
        inc(s, "duplicateTrades");
        return Ok(());
    }
    if s["seenTrades"].as_object().map(|m| m.len()).unwrap_or(0)
        >= bank["config"]["maxTradeIds"].as_i64().unwrap_or(0) as usize {
        censor(s, "trade_identity_capacity", e["receivedAt"].as_str().unwrap_or(""), events);
        return Ok(());
    }
    s["seenTrades"][t["id"].as_str().unwrap_or("")] = json!(key);
    inc(s, "uniqueTradesObserved");
    let ns = baal_service_runtime::contract::instant_ns(t["sourceAt"].as_str().unwrap_or(""))
        .map_err(|e| e.to_string())?;
    let t_seq: BigInt = t["sequence"].as_str().unwrap_or("0").parse().unwrap_or_default();
    if !s["lastTradeSequence"].is_null()
        && (t_seq <= s["lastTradeSequence"].as_str().unwrap_or("0").parse::<BigInt>().unwrap_or_default()
            || ns < i128::from(s["lastTradeSourceNs"].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0))) {
        censor(s, "trade_ordering_ambiguous", e["receivedAt"].as_str().unwrap_or(""), events);
        return Ok(());
    }
    s["lastTradeSequence"] = t["sequence"].clone();
    s["lastTradeSourceNs"] = json!(ns.to_string());
    if s["ready"] != V::Bool(true) || e["continuity"]["continuous"] != V::Bool(true) {
        inc(s, "unqualifiedTrades");
        return Ok(());
    }
    if s["lastBook"].is_null()
        || instant_ms(&e["receivedAt"]) - instant_ms(&s["lastBook"]["receivedAt"])
            > bank["config"]["maxDataAgeMs"].as_i64().unwrap_or(0) {
        censor(s, "stale_book", e["receivedAt"].as_str().unwrap_or(""), events);
        inc(s, "unqualifiedTrades");
        return Ok(());
    }
    for qi in 0..s["quotes"].as_array().map(|a| a.len()).unwrap_or(0) {
        let q = s["quotes"][qi].clone();
        if !q["closedAt"].is_null() || q["epoch"] != s["epoch"]
            || q["activationStatus"].as_str() != Some("observed_nonmarketable")
            || instant_ms(&e["receivedAt"]) <= instant_ms(&q["activationObservedAt"])
            || ns <= i128::from(q["activationSourceNs"].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0))
            || t_seq <= q["activationSequence"].as_str().unwrap_or("0").parse::<BigInt>().unwrap_or_default()
            || e["continuity"]["revision"].as_i64().unwrap_or(0) <= q["activationRevision"].as_i64().unwrap_or(0) {
            continue;
        }
        let mut q = q;
        let traded = baal_paper::fixed::big_amount(t["price"].as_str().unwrap_or(""))?;
        let limit = baal_paper::fixed::big_amount(q["price"].as_str().unwrap_or(""))?;
        if traded == limit {
            inc(s, "qualifiedTradeTouches");
            continue;
        }
        let through = if q["side"].as_str() == Some("bid") { traded < limit } else { traded > limit };
        if !through { continue; }
        let total: BigInt = q["qualifiedTradeThroughQuantity"].as_str().unwrap_or("0").parse::<BigInt>().unwrap_or_default()
            + BigInt::from(t["quantity"].as_i64().unwrap_or(0));
        q["qualifiedTradeThroughQuantity"] = json!(total.to_string());
        q["qualifiedTradeThroughEvents"] = json!(q["qualifiedTradeThroughEvents"].as_i64().unwrap_or(0) + 1);
        if q["firstQualifiedTradeThroughAt"].is_null() {
            q["firstQualifiedTradeThroughAt"] = e["receivedAt"].clone();
        }
        inc(s, "qualifiedTradeThroughEvents");
        events.push(json!({"kind":"trade_through_evidence","quoteId":q["id"],"tradeId":t["id"],
            "at":e["receivedAt"],"observedQuantity":t["quantity"],
            "visibleQueueAheadHighWater":q["visibleQueueAheadHighWater"],
            "executionClaim":"unverified","queueConsumed":false}));
        s["quotes"][qi] = q;
    }
    Ok(())
}
/// observePassive(input, event, bank)
pub fn observe_passive(input: &V, event: &V, bank: &V) -> Result<V, String> {
    assert_state(input, bank)?;
    let e = normalized(event, bank)?;
    let input_hash = canonical_digest(&e);
    if e["index"] == input["index"] {
        check(input_hash == input["lastInputHash"].as_str().unwrap_or(""), "Conflicting replay index")?;
        return Ok(json!({"state": input, "events": [], "record": V::Null}));
    }
    check(e["index"].as_i64() == Some(input["index"].as_i64().unwrap_or(0) + 1),
        "Missing or reordered observer event")?;
    check(input["lastAt"].is_null() || instant_ms(&e["receivedAt"]) >= instant_ms(&input["lastAt"]),
        "Receipt rollback")?;
    let mut s = input.clone();
    let mut events: Vec<V> = Vec::new();
    let co = &e["continuity"];
    let epoch_changed = !s["epoch"].is_null() && s["epoch"] != co["epoch"];
    let revision_gap = !s["revision"].is_null() && !epoch_changed
        && (co["revision"].as_i64().unwrap_or(0) < s["revision"].as_i64().unwrap_or(0)
            || (co["revision"].as_i64().unwrap_or(0) > s["revision"].as_i64().unwrap_or(0)
                && co["previousRevision"].as_i64().unwrap_or(0) > s["revision"].as_i64().unwrap_or(0)));
    if epoch_changed || revision_gap || co["ready"] != V::Bool(true)
        || co["continuous"] != V::Bool(true) || e["kind"].as_str() == Some("invalidate") {
        let reason = if !e["reason"].is_null() {
            e["reason"].as_str().unwrap_or("").to_string()
        } else if epoch_changed {
            "epoch_changed".to_string()
        } else if revision_gap {
            "delivery_gap".to_string()
        } else {
            "not_continuous".to_string()
        };
        censor(&mut s, &reason, e["receivedAt"].as_str().unwrap_or(""), &mut events);
    }
    s["epoch"] = co["epoch"].clone();
    s["revision"] = co["revision"].clone();
    if e["kind"].as_str() == Some("book") && co["recovered"] == V::Bool(true)
        && co["ready"] == V::Bool(true) && co["continuous"] == V::Bool(true) {
        s["ready"] = json!(true);
    }
    age(&mut s, e["receivedAt"].as_str().unwrap_or(""), bank);
    if e["kind"].as_str() == Some("book") { process_book(&mut s, &e, bank, &mut events)?; }
    if e["kind"].as_str() == Some("trade") { process_trade(&mut s, &e, bank, &mut events)?; }
    let mut record = json!({"index": e["index"], "previousHash": s["chainHash"], "event": e});
    record["hash"] = json!(canonical_digest(&record));
    s["index"] = e["index"].clone();
    s["chainHash"] = record["hash"].clone();
    s["lastInputHash"] = json!(input_hash);
    s["lastAt"] = e["receivedAt"].clone();
    Ok(json!({"state": s, "events": events, "record": record}))
}
/// passiveReport(state, bank)
pub fn passive_report(state: &V, bank: &V) -> Result<V, String> {
    assert_state(state, bank)?;
    let mut markouts: serde_json::Map<String, V> = serde_json::Map::new();
    for q in state["quotes"].as_array().cloned().unwrap_or_default() {
        for (h, m) in q["markouts"].as_object().cloned().unwrap_or_default() {
            if !markouts.contains_key(&h) { markouts.insert(h.clone(), json!({})); }
            let status = m["status"].as_str().unwrap_or("");
            let n = markouts[&h][status].as_i64().unwrap_or(0) + 1;
            markouts.get_mut(&h).unwrap()[status] = json!(n);
        }
    }
    Ok(json!({"mode": bank["mode"], "name": bank["name"], "cutoff": bank["cutoff"],
        "sourceHash": bank["sourceHash"], "bankHash": state["bankHash"],
        "configHash": bank["configHash"], "throughIndex": state["index"],
        "chainHash": state["chainHash"], "updatedAt": state["lastAt"], "ready": state["ready"],
        "quotePairs": state["pairs"], "counts": state["counts"], "markoutStatuses": markouts,
        "execution": {"ordersSubmitted":0,"simulatedFills":V::Null,"partialFillQuantity":V::Null,
            "fillProbability":V::Null,"pnl":V::Null},
        "evidence": {"localContinuityOnly":true,"completeHistoricalTradeTape":false,
            "aggressorSideKnown":false,"queuePriorityKnown":false,
            "visibleQueueNeverDecrementedByDisappearanceOrTrade":true,
            "quoteCrossingIsNotFill":true,"profitabilityEstablished":false}}))
}
/// replayPassive(records, bank)
pub fn replay_passive(records: &V, bank: &V) -> Result<V, String> {
    check(records.is_array(), "Observer journal records required")?;
    let created = create_passive_observer(
        bank["marketId"].as_str().unwrap_or(""), bank["exchangeId"].as_str().unwrap_or(""),
        bank["tournamentId"].as_str().unwrap_or(""), bank["sourceHash"].as_str().unwrap_or(""),
        bank["cutoff"].as_str().unwrap_or(""), Some(&bank["config"]))?;
    check(canonical_digest(&created["bank"]) == canonical_digest(bank),
        "Unexpected observer bank fields")?;
    let mut state = created["state"].clone();
    for record in records.as_array().unwrap() {
        let out = observe_passive(&state, &record["event"], bank)?;
        check(!out["record"].is_null()
            && canonical_digest(&out["record"]) == canonical_digest(record),
            "Observer journal hash or payload mismatch")?;
        state = out["state"].clone();
    }
    Ok(state)
}
