// live-paper/passive-observer.mjs — measurement-only passive quote observer.
// Pure market research, no orders/fills/cash/PnL. digest = sha256 over the
// sorted-key canonical JSON (ledger-style canonical, not insertion-order).
use baal_paper::fixed::{big_amount, big_price, big_sequence, instant, money, SCALE, TICK};
use baal_service_runtime::contract::{instant_ns, ms_to_iso, timestamp};
use baal_service_runtime::json_text::{canonical_hash, js_int, js_truthy};
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};

/// canonical-sorted sha256 (JS: sha256(canonical(x))); serde_json::Value cannot
/// hold non-finite numbers, so canonical_hash is infallible here.
fn digest(v: &V) -> String { canonical_hash(v).unwrap_or_default() }
fn check(ok: bool, message: &str) -> Result<(), String> {
    if !ok { return Err(message.into()); }
    Ok(())
}
fn id(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        !s.is_empty()
            && s.as_bytes()[0].is_ascii_digit()
            && s.as_bytes()[0] != b'0'
            && s.bytes().all(|b| b.is_ascii_digit())
    })
}
fn identity(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        !s.is_empty() && s.len() <= 100
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    })
}
fn truthy(v: &V) -> bool { js_truthy(Some(v)) }
fn ms_instant(v: &V) -> Result<i64, String> { instant(v.as_str().unwrap_or("")) }
fn ns_of(v: &V) -> Result<i128, String> {
    instant_ns(v.as_str().unwrap_or("")).map_err(|e| e.to_string())
}
fn seq_of(v: &V) -> Result<BigInt, String> { big_sequence(v.as_str().unwrap_or("")) }
/// JS BigInt(v) on stored strings/integers: null -> TypeError, bad string ->
/// SyntaxError, non-integer number -> RangeError.
fn to_bigint(v: &V) -> Result<BigInt, String> {
    match v {
        V::Null => Err("Cannot convert null to a BigInt".into()),
        V::Bool(b) => Err(format!("Cannot convert {} to a BigInt", b)),
        V::Number(n) => js_int(v)
            .map(BigInt::from)
            .ok_or_else(|| format!("The number {} cannot be converted to a BigInt because it is not an integer", n)),
        V::String(s) => big_sequence(s).map_err(|_| format!("Cannot convert {} to a BigInt", s)),
        _ => Err("Cannot convert [object Object] to a BigInt".into()),
    }
}
fn inc(s: &mut V, key: &str) -> Result<(), String> {
    if !s["counts"].is_object() {
        // JS: (undefined)[key] — TypeError.
        return Err(format!("Cannot read properties of undefined (reading '{}')", key));
    }
    let n = s["counts"][key].as_i64().unwrap_or(0);
    s["counts"][key] = json!(n + 1);
    Ok(())
}
/// PASSIVE_DEFAULTS (frozen).
pub fn passive_defaults() -> V {
    json!({"latencyMs":500,"maxDataAgeMs":20000,"maxSourceClockSkewMs":2000,
        "quoteLifetimeMs":60000,"quoteEveryMs":60000,"horizonsMs":[60000,120000,300000],
        "markoutLatenessMs":20000,"orderQuantity":10,"slippageTicks":1,
        "maxQuotePairs":1000,"maxTradeIds":100000})
}
const DEFAULT_KEYS: [&str; 11] = [
    "latencyMs","maxDataAgeMs","maxSourceClockSkewMs","quoteLifetimeMs","quoteEveryMs",
    "horizonsMs","markoutLatenessMs","orderQuantity","slippageTicks","maxQuotePairs","maxTradeIds",
];
fn checked_config(input: Option<&V>) -> Result<V, String> {
    // Object.keys(input): strings enumerate index keys; other primitives give
    // none; null throws a TypeError.
    let input_map: Map<String, V> = match input {
        None => Map::new(),
        Some(V::Null) => return Err("Cannot convert undefined or null to object".into()),
        Some(V::Object(m)) => m.clone(),
        Some(V::String(s)) => (0..s.chars().count())
            .map(|i| (i.to_string(), json!("")))
            .collect(),
        Some(_) => Map::new(),
    };
    check(
        input_map.keys().all(|k| DEFAULT_KEYS.contains(&k.as_str())),
        "Unknown passive observer config",
    )?;
    // structuredClone({...defaults, ...input}): default key order, overridden values.
    let mut c = passive_defaults();
    for (k, v) in &input_map {
        c[k] = v.clone();
    }
    for k in DEFAULT_KEYS {
        if k == "horizonsMs" {
            continue;
        }
        check(js_int(&c[k]).is_some_and(|n| n > 0), &format!("Invalid config {}", k))?;
    }
    let ci = |k: &str| js_int(&c[k]).unwrap_or(i64::MIN);
    check(
        ci("maxSourceClockSkewMs") <= 2000 && ci("maxDataAgeMs") <= 60000
            && ci("latencyMs") < ci("quoteLifetimeMs") && ci("quoteLifetimeMs") <= 300000
            && ci("quoteEveryMs") >= ci("quoteLifetimeMs") && ci("orderQuantity") <= 100
            && ci("slippageTicks") <= 5 && ci("maxQuotePairs") <= 10000 && ci("maxTradeIds") <= 100000,
        "Unsafe observer bounds",
    )?;
    let h_ok = c["horizonsMs"].as_array().is_some_and(|h| {
        !h.is_empty() && h.len() <= 10
            && h.iter().enumerate().all(|(i, v)| {
                js_int(v).is_some_and(|x| x > ci("latencyMs") && x <= 3600000)
                    && (i == 0 || js_int(v) > js_int(&h[i - 1]))
            })
    });
    check(h_ok, "Invalid markout horizons")?;
    Ok(c)
}
/// createPassiveObserver(scope)
pub fn create_passive_observer(scope: &V) -> Result<V, String> {
    // JS destructuring default only applies to undefined (absent key), not null.
    let tournament_id = scope
        .get("tournamentId")
        .cloned()
        .unwrap_or_else(|| json!("bda92870-621e-47b0-bc3c-3602c5c26f55"));
    let tournament_ok = tournament_id.as_str().is_some_and(|s| {
        s.len() == 36
            && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-')
    });
    check(
        id(&scope["marketId"]) && id(&scope["exchangeId"]) && tournament_ok,
        "Invalid market scope",
    )?;
    check(
        scope["sourceHash"].as_str().is_some_and(|s| {
            s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }),
        "Frozen source hash required",
    )?;
    instant(scope["cutoff"].as_str().unwrap_or(""))?;
    let c = checked_config(scope.get("config"))?;
    let config_hash = digest(&c);
    let mut bank = json!({"schemaVersion":1,"name":"passive-opportunity-observer-v1",
        "mode":"measurement_only","marketId":scope["marketId"],"exchangeId":scope["exchangeId"],
        "tournamentId":tournament_id,"cutoff":scope["cutoff"],"sourceHash":scope["sourceHash"],
        "config":c});
    bank["configHash"] = json!(config_hash);
    let bank_hash = digest(&bank);
    let state = json!({"schemaVersion":1,"bankHash":bank_hash,"index":0,
        "chainHash":bank["sourceHash"],"lastInputHash":V::Null,"lastAt":V::Null,
        "epoch":V::Null,"revision":V::Null,"ready":false,"lastBook":V::Null,
        "quotes":[],"lastQuoteAt":V::Null,"seenTrades":{},"lastTradeSequence":V::Null,
        "lastTradeSourceNs":V::Null,"pairs":0,"counts":{}});
    Ok(json!({"bank":bank,"state":state}))
}
fn assert_state(s: &V, bank: &V) -> Result<(), String> {
    // JS && short-circuit: checkedConfig runs only when name/mode pass.
    check(
        bank["name"].as_str() == Some("passive-opportunity-observer-v1")
            && bank["mode"].as_str() == Some("measurement_only"),
        "Frozen observer bank changed",
    )?;
    let cfg = checked_config(bank.get("config"))?;
    check(
        bank["configHash"].as_str() == Some(digest(&cfg).as_str())
            && s["bankHash"].as_str() == Some(digest(bank).as_str())
            && s["schemaVersion"].as_i64() == Some(1),
        "Frozen observer bank changed",
    )?;
    check(
        js_int(&s["index"]).is_some_and(|i| i >= 0) && s["quotes"].is_array(),
        "Invalid observer state",
    )?;
    Ok(())
}
fn normalized(input: &V, bank: &V) -> Result<V, String> {
    check(
        js_int(&input["index"]).is_some_and(|i| i >= 1)
            && ["book", "trade", "invalidate", "clock"].contains(&input["kind"].as_str().unwrap_or("")),
        "Invalid observer event",
    )?;
    let at = ms_instant(&input["receivedAt"])?;
    let c = &input["continuity"];
    check(
        truthy(c)
            && identity(&c["epoch"])
            && js_int(&c["revision"]).is_some_and(|r| r >= 0)
            && js_int(&c["previousRevision"]).is_some_and(|r| r >= 0)
            && js_int(&c["previousRevision"]).unwrap_or(i64::MAX) <= js_int(&c["revision"]).unwrap_or(i64::MIN)
            && c["ready"].is_boolean() && c["continuous"].is_boolean(),
        "Invalid continuity metadata",
    )?;
    // JS: (c.recovered===undefined || typeof c.recovered==='boolean').
    if let Some(r) = c.get("recovered") {
        check(r.is_boolean(), "Invalid continuity metadata")?;
    }
    let mut e = json!({"index":input["index"],"kind":input["kind"],"receivedAt":input["receivedAt"],
        "continuity":{"epoch":c["epoch"],"revision":c["revision"],"previousRevision":c["previousRevision"],
            "ready":c["ready"],"continuous":c["continuous"],
            "recovered":c["recovered"].as_bool() == Some(true)}});
    if e["kind"].as_str() == Some("invalidate") {
        check(
            ["gap", "disconnect", "stale", "resync", "settlement", "schema", "book_dirty"]
                .contains(&input["reason"].as_str().unwrap_or("")),
            "Invalid invalidation reason",
        )?;
        e["reason"] = input["reason"].clone();
    }
    if e["kind"].as_str() == Some("book") {
        let b = &input["book"];
        check(
            b["marketId"] == bank["marketId"] && b["exchangeId"] == bank["exchangeId"]
                && b["instrument"].as_str() == Some(format!("{}:YES", bank["exchangeId"].as_str().unwrap_or("")).as_str())
                && b["receivedAt"] == input["receivedAt"],
            "Book scope mismatch",
        )?;
        let request = ms_instant(&b["requestStartedAt"])?;
        seq_of(&b["sequence"])?;
        check(
            request <= at && at - request <= js_int(&bank["config"]["maxDataAgeMs"]).unwrap_or(0),
            "Invalid book request clock",
        )?;
        let source_at = timestamp(&b["sourceAt"]).map_err(|x| x.to_string())?;
        check(
            b["versionKey"].as_str() == Some(&format!("{}:{}", b["sequence"].as_str().unwrap_or(""), source_at))
                && instant_ns(&source_at).map_err(|x| x.to_string())?
                    <= (at + js_int(&bank["config"]["maxSourceClockSkewMs"]).unwrap_or(0)) as i128 * 1_000_000,
            "Invalid book source clock",
        )?;
        let mut book = json!({"marketId":b["marketId"],"exchangeId":b["exchangeId"],
            "instrument":b["instrument"],"requestStartedAt":b["requestStartedAt"],
            "receivedAt":b["receivedAt"],"sourceAt":source_at,"sequence":b["sequence"],
            "versionKey":b["versionKey"]});
        for (side, direction) in [("bids", -1i64), ("asks", 1i64)] {
            check(
                b[side].as_array().is_some_and(|a| !a.is_empty() && a.len() <= 200),
                "Missing bounded depth",
            )?;
            let raw = b[side].as_array().cloned().unwrap_or_default();
            let mut levels: Vec<V> = Vec::new();
            for (i, l) in raw.iter().enumerate() {
                let p = big_price(l["price"].as_str().unwrap_or(""))?;
                check(
                    js_int(&l["quantity"]).is_some_and(|q| q > 0),
                    "Invalid depth quantity",
                )?;
                if i > 0 {
                    let prev = big_price(raw[i - 1]["price"].as_str().unwrap_or(""))?;
                    check(
                        if direction == -1 { p < prev } else { p > prev },
                        "Unsorted depth",
                    )?;
                }
                levels.push(json!({"price":money(&p),"quantity":l["quantity"]}));
            }
            book[side] = V::Array(levels);
        }
        check(
            big_price(book["bids"][0]["price"].as_str().unwrap_or(""))?
                < big_price(book["asks"][0]["price"].as_str().unwrap_or(""))?,
            "Crossed or locked book",
        )?;
        e["book"] = book;
    }
    if e["kind"].as_str() == Some("trade") {
        let t = &input["trade"];
        check(
            t["marketId"] == bank["marketId"] && t["exchangeId"] == bank["exchangeId"]
                && t["tournamentId"] == bank["tournamentId"],
            "Trade scope mismatch",
        )?;
        check(
            identity(&t["id"]) && js_int(&t["quantity"]).is_some_and(|q| q > 0),
            "Ordered stable trade identity and integer size required",
        )?;
        seq_of(&t["sequence"])?;
        let p = big_amount(t["price"].as_str().unwrap_or(""))?;
        let source_at = timestamp(&t["sourceAt"]).map_err(|x| x.to_string())?;
        check(
            p >= BigInt::from(0) && p <= BigInt::from(SCALE)
                && instant_ns(&source_at).map_err(|x| x.to_string())?
                    <= (at + js_int(&bank["config"]["maxSourceClockSkewMs"]).unwrap_or(0)) as i128 * 1_000_000,
            "Invalid trade price or clock",
        )?;
        e["trade"] = json!({"id":t["id"],"sequence":t["sequence"],"marketId":t["marketId"],
            "exchangeId":t["exchangeId"],"tournamentId":t["tournamentId"],
            "price":money(&p),"quantity":t["quantity"],"sourceAt":source_at});
    }
    Ok(e)
}
fn censor(s: &mut V, reason: &str, at: &str, events: &mut Vec<V>) -> Result<(), String> {
    s["ready"] = json!(false);
    if let Some(quotes) = s["quotes"].as_array_mut() {
        for q in quotes.iter_mut() {
            if q["closedAt"].is_null() {
                q["closedAt"] = json!(at);
                q["closedReason"] = json!(reason);
            }
            if let Some(markouts) = q["markouts"].as_object_mut() {
                for (_h, m) in markouts.iter_mut() {
                    if m["status"].as_str() == Some("pending") {
                        *m = json!({"status":"censored_continuity","reason":reason,"at":at});
                    }
                }
            }
        }
    }
    inc(s, "continuityInvalidations")?;
    events.push(json!({"kind":"continuity_invalidated","reason":reason,"at":at}));
    Ok(())
}
fn age(s: &mut V, at: &str, bank: &V) -> Result<(), String> {
    let now = instant(at)?;
    let markout_lateness = js_int(&bank["config"]["markoutLatenessMs"]).unwrap_or(0);
    if let Some(quotes) = s["quotes"].as_array_mut() {
        for q in quotes.iter_mut() {
            if q["closedAt"].is_null()
                && now >= instant(q["expiresAt"].as_str().unwrap_or(""))? {
                q["closedAt"] = q["expiresAt"].clone();
                q["closedReason"] = json!("hypothesis_expired");
            }
            let created = instant(q["createdAt"].as_str().unwrap_or(""))?;
            if let Some(markouts) = q["markouts"].as_object_mut() {
                let entries: Vec<(String, V)> = markouts.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                for (h, m) in entries {
                    if m["status"].as_str() == Some("pending")
                        && now > created + h.parse::<i64>().unwrap_or(0) + markout_lateness {
                        markouts.insert(h, json!({"status":"missing_fresh_followup","at":at}));
                    }
                }
            }
        }
    }
    Ok(())
}
fn same_or_better(book: &V, q: &V) -> Result<BigInt, String> {
    let side_bid = q["side"].as_str() == Some("bid");
    let levels = book[if side_bid { "bids" } else { "asks" }]
        .as_array().cloned().unwrap_or_default();
    let q_price = big_amount(q["price"].as_str().unwrap_or(""))?;
    let mut n = BigInt::from(0);
    for l in &levels {
        let p = big_amount(l["price"].as_str().unwrap_or(""))?;
        let include = if side_bid { p >= q_price } else { p <= q_price };
        if include {
            // l.quantity is a validated safe integer -> JS BigInt(l.quantity).
            n += BigInt::from(js_int(&l["quantity"]).unwrap_or(0));
        }
    }
    Ok(n)
}
fn process_book(s: &mut V, e: &V, bank: &V, events: &mut Vec<V>) -> Result<(), String> {
    let b = e["book"].clone();
    let at = ms_instant(&e["receivedAt"])?;
    let source_ns = ns_of(&b["sourceAt"])?;
    let previous = s["lastBook"].clone();
    let prev_is_book = previous.is_object();
    let mut prev_ns = 0i128;
    if prev_is_book {
        prev_ns = ns_of(&previous["sourceAt"])?;
        check(
            big_sequence(b["sequence"].as_str().unwrap_or(""))?
                >= big_sequence(previous["sequence"].as_str().unwrap_or(""))?
                && source_ns >= prev_ns,
            "Book source rollback",
        )?;
        if previous["versionKey"] == b["versionKey"] {
            check(
                digest(&json!({"bids":previous["bids"],"asks":previous["asks"]}))
                    == digest(&json!({"bids":b["bids"],"asks":b["asks"]})),
                "Conflicting equal book version",
            )?;
        }
    }
    let independent = !prev_is_book
        || (source_ns > prev_ns && b["versionKey"] != previous["versionKey"]);
    s["lastBook"] = b.clone();
    inc(s, "bookObservations")?;
    if !truthy(&s["ready"]) || !independent {
        inc(s, if !truthy(&s["ready"]) { "unreadyBooks" } else { "repeatedBooks" })?;
        return Ok(());
    }
    let bid = big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?;
    let ask = big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))?;
    let s_epoch = s["epoch"].clone();
    let rev = js_int(&e["continuity"]["revision"]).unwrap_or(i64::MIN);
    // Counter increments that JS applies to s.counts mid-loop are applied after
    // the borrow of s.quotes ends; ordering of distinct counter keys is
    // irrelevant to the hashed state.
    let mut deferred_inc: Vec<&'static str> = Vec::new();
    if let Some(quotes) = s["quotes"].as_array_mut() {
        for q in quotes.iter_mut() {
            let skip = q["epoch"] != s_epoch
                || at <= instant(q["createdAt"].as_str().unwrap_or(""))?
                || BigInt::from(source_ns) <= to_bigint(&q["sourceNs"])?
                || big_sequence(b["sequence"].as_str().unwrap_or(""))? <= to_bigint(&q["sequence"])?
                || rev <= js_int(&q["revision"]).unwrap_or(0)
                || instant(b["requestStartedAt"].as_str().unwrap_or(""))?
                    < instant(q["activationAt"].as_str().unwrap_or(""))?;
            if skip {
                continue;
            }
            if q["closedAt"].is_null() {
                let visible = same_or_better(&b, q)?;
                q["visibleSameOrBetterNow"] = json!(visible.to_string());
                if visible > to_bigint(&q["visibleQueueAheadHighWater"])? {
                    q["visibleQueueAheadHighWater"] = json!(visible.to_string());
                }
                let q_price = big_amount(q["price"].as_str().unwrap_or(""))?;
                let crossed = if q["side"].as_str() == Some("bid") {
                    ask <= q_price
                } else {
                    bid >= q_price
                };
                if crossed && q["firstOppositeQuoteTouchOrCrossAt"].is_null() {
                    q["firstOppositeQuoteTouchOrCrossAt"] = e["receivedAt"].clone();
                    deferred_inc.push("oppositeQuoteTouchesOrCrossings");
                }
                if q["activationStatus"].as_str() == Some("awaiting_independent_book")
                    && at >= instant(q["activationAt"].as_str().unwrap_or(""))? {
                    q["activationStatus"] = json!(if crossed {
                        "marketable_at_first_followup"
                    } else {
                        "observed_nonmarketable"
                    });
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
            if q["markouts"].is_object() {
                let entries: Vec<(String, V)> = q["markouts"].as_object().unwrap()
                    .iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                let created = instant(q["createdAt"].as_str().unwrap_or(""))?;
                let q_price = q["price"].clone();
                let q_side = q["side"].clone();
                let after_cross = !q["firstOppositeQuoteTouchOrCrossAt"].is_null();
                let after_through = !q["firstQualifiedTradeThroughAt"].is_null();
                let q_id = q["id"].clone();
                for (h, m) in entries {
                    let hnum = h.parse::<i64>().unwrap_or(0);
                    if m["status"].as_str() == Some("pending") && at >= created + hnum {
                        let quote_price = big_amount(q_price.as_str().unwrap_or(""))?;
                        let slip = BigInt::from(js_int(&bank["config"]["slippageTicks"]).unwrap_or(0))
                            * BigInt::from(TICK);
                        let midpoint_move_twice = if q_side.as_str() == Some("bid") {
                            &bid + &ask - BigInt::from(2) * &quote_price
                        } else {
                            BigInt::from(2) * &quote_price - &bid - &ask
                        };
                        let executable_side_move = if q_side.as_str() == Some("bid") {
                            &bid - &slip - &quote_price
                        } else {
                            &quote_price - &ask - &slip
                        };
                        // JS / on BigInt truncates toward zero; num-bigint matches.
                        let markout = json!({"status":"observed_unexecuted_quote_markout",
                            "at":e["receivedAt"],"sourceVersion":b["versionKey"],
                            "latenessMs":at-created-hnum,
                            "signedMidpointMove":money(&(midpoint_move_twice/BigInt::from(2))),
                            "signedOppositeExecutableMoveAfterCushion":money(&executable_side_move),
                            "afterQuoteCross":after_cross,
                            "afterQualifiedTradeThrough":after_through});
                        q["markouts"][h.as_str()] = markout.clone();
                        deferred_inc.push("observedMarkouts");
                        let mut ev = json!({"kind":"quote_markout","quoteId":q_id,
                            "horizonMs":hnum});
                        if let (V::Object(em), V::Object(mm)) = (&mut ev, &markout) {
                            for (k, v) in mm { em.insert(k.clone(), v.clone()); }
                        }
                        events.push(ev);
                    }
                }
            }
        }
    }
    for k in deferred_inc {
        inc(s, k)?;
    }
    let cutoff = instant(bank["cutoff"].as_str().unwrap_or(""))?;
    if at <= cutoff || instant(b["requestStartedAt"].as_str().unwrap_or(""))? <= cutoff {
        inc(s, "developmentBooks")?;
        return Ok(());
    }
    if truthy(&s["lastQuoteAt"])
        && at - instant(s["lastQuoteAt"].as_str().unwrap_or(""))?
            < js_int(&bank["config"]["quoteEveryMs"]).unwrap_or(0) {
        return Ok(());
    }
    if s["pairs"].as_i64().unwrap_or(0) >= js_int(&bank["config"]["maxQuotePairs"]).unwrap_or(0) {
        inc(s, "quoteCapacityReached")?;
        return Ok(());
    }
    let inside_bid = &bid + BigInt::from(TICK);
    let inside_ask = &ask - BigInt::from(TICK);
    if &ask - &bid < BigInt::from(2 * TICK)
        || inside_bid >= inside_ask
        || inside_bid < BigInt::from(TICK)
        || inside_ask > BigInt::from(SCALE - TICK) {
        inc(s, "noDistinctInteriorPrices")?;
        return Ok(());
    }
    for (side, p) in [("bid", inside_bid.clone()), ("ask", inside_ask.clone())] {
        let qid = digest(&json!({"bankHash":s["bankHash"],"version":b["versionKey"],
            "at":e["receivedAt"],"side":side}));
        let mut markouts = Map::new();
        for h in bank["config"]["horizonsMs"].as_array().cloned().unwrap_or_default() {
            markouts.insert(js_int(&h).unwrap_or(0).to_string(), json!({"status":"pending"}));
        }
        let mut q = json!({"id":qid,"side":side,"price":money(&p),"quantity":bank["config"]["orderQuantity"],
            "createdAt":e["receivedAt"],
            "activationAt":ms_to_iso((at + js_int(&bank["config"]["latencyMs"]).unwrap_or(0)) as f64),
            "expiresAt":ms_to_iso((at + js_int(&bank["config"]["quoteLifetimeMs"]).unwrap_or(0)) as f64),
            "epoch":s_epoch,"revision":e["continuity"]["revision"],"sequence":b["sequence"],
            "sourceNs":source_ns.to_string(),"sourceVersion":b["versionKey"],
            "activationStatus":"awaiting_independent_book","activationObservedAt":V::Null,
            "closedAt":V::Null,"closedReason":V::Null,
            "visibleSameOrBetterNow":"0","visibleQueueAheadHighWater":"0",
            "qualifiedTradeThroughQuantity":"0","qualifiedTradeThroughEvents":0,
            "firstOppositeQuoteTouchOrCrossAt":V::Null,"firstQualifiedTradeThroughAt":V::Null});
        q["markouts"] = V::Object(markouts);
        let visible = same_or_better(&b, &q)?;
        q["visibleSameOrBetterNow"] = json!(visible.to_string());
        q["visibleQueueAheadHighWater"] = json!(visible.to_string());
        if let Some(arr) = s["quotes"].as_array_mut() {
            arr.push(q.clone());
        }
        events.push(json!({"kind":"hypothetical_quote","quoteId":q["id"],"side":side,
            "price":q["price"],"quantity":q["quantity"],"at":e["receivedAt"],
            "activationAt":q["activationAt"],"visibleQueueAhead":q["visibleQueueAheadHighWater"],
            "executionClaim":"none"}));
    }
    let pairs = s["pairs"].as_i64().unwrap_or(0) + 1;
    s["pairs"] = json!(pairs);
    s["lastQuoteAt"] = e["receivedAt"].clone();
    inc(s, "quotePairs")?;
    Ok(())
}
fn process_trade(s: &mut V, e: &V, bank: &V, events: &mut Vec<V>) -> Result<(), String> {
    let t = e["trade"].clone();
    let key = digest(&t);
    let tid = t["id"].as_str().unwrap_or("").to_string();
    let old = s["seenTrades"][tid.as_str()].clone();
    if !old.is_null() {
        check(old.as_str() == Some(key.as_str()), "Conflicting duplicate trade")?;
        inc(s, "duplicateTrades")?;
        return Ok(());
    }
    let seen_len = s["seenTrades"].as_object().map(|m| m.len()).unwrap_or(0);
    if seen_len >= js_int(&bank["config"]["maxTradeIds"]).unwrap_or(0) as usize {
        censor(s, "trade_identity_capacity", e["receivedAt"].as_str().unwrap_or(""), events)?;
        return Ok(());
    }
    s["seenTrades"][tid] = json!(key);
    inc(s, "uniqueTradesObserved")?;
    let ns = ns_of(&t["sourceAt"])?;
    if !s["lastTradeSequence"].is_null()
        && (big_sequence(t["sequence"].as_str().unwrap_or(""))?
            <= to_bigint(&s["lastTradeSequence"])?
            || BigInt::from(ns) < to_bigint(&s["lastTradeSourceNs"])?)
    {
        censor(s, "trade_ordering_ambiguous", e["receivedAt"].as_str().unwrap_or(""), events)?;
        return Ok(());
    }
    s["lastTradeSequence"] = t["sequence"].clone();
    s["lastTradeSourceNs"] = json!(ns.to_string());
    if !truthy(&s["ready"]) || !truthy(&e["continuity"]["continuous"]) {
        inc(s, "unqualifiedTrades")?;
        return Ok(());
    }
    let e_at = ms_instant(&e["receivedAt"])?;
    if s["lastBook"].is_null()
        || e_at - instant(s["lastBook"]["receivedAt"].as_str().unwrap_or(""))?
            > js_int(&bank["config"]["maxDataAgeMs"]).unwrap_or(0)
    {
        censor(s, "stale_book", e["receivedAt"].as_str().unwrap_or(""), events)?;
        inc(s, "unqualifiedTrades")?;
        return Ok(());
    }
    let s_epoch = s["epoch"].clone();
    let t_seq = big_sequence(t["sequence"].as_str().unwrap_or(""))?;
    let t_qty = BigInt::from(js_int(&t["quantity"]).unwrap_or(0));
    let rev = js_int(&e["continuity"]["revision"]).unwrap_or(i64::MIN);
    let mut deferred_inc: Vec<&'static str> = Vec::new();
    if let Some(quotes) = s["quotes"].as_array_mut() {
        for q in quotes.iter_mut() {
            let skip = !q["closedAt"].is_null()
                || q["epoch"] != s_epoch
                || q["activationStatus"].as_str() != Some("observed_nonmarketable")
                || e_at <= instant(q["activationObservedAt"].as_str().unwrap_or(""))?
                || BigInt::from(ns) <= to_bigint(&q["activationSourceNs"])?
                || t_seq <= to_bigint(&q["activationSequence"])?
                || rev <= js_int(&q["activationRevision"]).unwrap_or(0);
            if skip {
                continue;
            }
            let traded = big_amount(t["price"].as_str().unwrap_or(""))?;
            let limit = big_amount(q["price"].as_str().unwrap_or(""))?;
            if traded == limit {
                deferred_inc.push("qualifiedTradeTouches");
                continue;
            }
            let through = if q["side"].as_str() == Some("bid") {
                traded < limit
            } else {
                traded > limit
            };
            if !through {
                continue;
            }
            let cur = to_bigint(&q["qualifiedTradeThroughQuantity"])?;
            q["qualifiedTradeThroughQuantity"] = json!((cur + &t_qty).to_string());
            let ne = q["qualifiedTradeThroughEvents"].as_i64().unwrap_or(0) + 1;
            q["qualifiedTradeThroughEvents"] = json!(ne);
            if q["firstQualifiedTradeThroughAt"].is_null() {
                q["firstQualifiedTradeThroughAt"] = e["receivedAt"].clone();
            }
            deferred_inc.push("qualifiedTradeThroughEvents");
            events.push(json!({"kind":"trade_through_evidence","quoteId":q["id"],
                "tradeId":t["id"],"at":e["receivedAt"],"observedQuantity":t["quantity"],
                "visibleQueueAheadHighWater":q["visibleQueueAheadHighWater"],
                "executionClaim":"unverified","queueConsumed":false}));
        }
    }
    for k in deferred_inc {
        inc(s, k)?;
    }
    Ok(())
}
/// observePassive(input,event,bank) -> {state,events,record}
pub fn observe_passive(input: &V, event: &V, bank: &V) -> Result<V, String> {
    assert_state(input, bank)?;
    let e = normalized(event, bank)?;
    let input_hash = digest(&e);
    if js_int(&e["index"]) == js_int(&input["index"]) {
        check(
            input["lastInputHash"].as_str() == Some(input_hash.as_str()),
            "Conflicting replay index",
        )?;
        return Ok(json!({"state":input,"events":[],"record":V::Null}));
    }
    check(
        js_int(&e["index"]) == js_int(&input["index"]).map(|i| i + 1),
        "Missing or reordered observer event",
    )?;
    check(
        input["lastAt"].is_null()
            || ms_instant(&e["receivedAt"])? >= ms_instant(&input["lastAt"])?,
        "Receipt rollback",
    )?;
    let mut s = input.clone();
    let mut events: Vec<V> = Vec::new();
    let co = &e["continuity"];
    let epoch_changed = !s["epoch"].is_null() && s["epoch"] != co["epoch"];
    // A merged delivery may overlap revisions already received. Only a start
    // strictly beyond the watermark proves a forward gap; overlap is continuous.
    let s_rev = js_int(&s["revision"]);
    let co_rev = js_int(&co["revision"]).unwrap_or(i64::MIN);
    let co_prev = js_int(&co["previousRevision"]).unwrap_or(i64::MIN);
    let revision_gap = s_rev.is_some() && !epoch_changed
        && (co_rev < s_rev.unwrap()
            || (co_rev > s_rev.unwrap() && co_prev > s_rev.unwrap()));
    if epoch_changed || revision_gap || !truthy(&co["ready"]) || !truthy(&co["continuous"])
        || e["kind"].as_str() == Some("invalidate") {
        let fallback = if epoch_changed {
            "epoch_changed"
        } else if revision_gap {
            "delivery_gap"
        } else {
            "not_continuous"
        };
        let reason = e["reason"].as_str().filter(|r| !r.is_empty()).unwrap_or(fallback);
        censor(&mut s, reason, e["receivedAt"].as_str().unwrap_or(""), &mut events)?;
    }
    s["epoch"] = co["epoch"].clone();
    s["revision"] = co["revision"].clone();
    if e["kind"].as_str() == Some("book") && truthy(&co["recovered"])
        && truthy(&co["ready"]) && truthy(&co["continuous"]) {
        s["ready"] = json!(true);
    }
    age(&mut s, e["receivedAt"].as_str().unwrap_or(""), bank)?;
    if e["kind"].as_str() == Some("book") {
        process_book(&mut s, &e, bank, &mut events)?;
    }
    if e["kind"].as_str() == Some("trade") {
        process_trade(&mut s, &e, bank, &mut events)?;
    }
    let mut record = json!({"index":e["index"],"previousHash":s["chainHash"],"event":e});
    let h = digest(&record);
    record["hash"] = json!(h);
    s["index"] = e["index"].clone();
    s["chainHash"] = record["hash"].clone();
    s["lastInputHash"] = json!(input_hash);
    s["lastAt"] = e["receivedAt"].clone();
    Ok(json!({"state":s,"events":events,"record":record}))
}
/// passiveReport(state,bank)
pub fn passive_report(state: &V, bank: &V) -> Result<V, String> {
    assert_state(state, bank)?;
    let mut markouts = Map::new();
    for q in state["quotes"].as_array().cloned().unwrap_or_default() {
        if let Some(m) = q["markouts"].as_object() {
            for (h, mv) in m {
                if !markouts.contains_key(h) {
                    markouts.insert(h.clone(), V::Object(Map::new()));
                }
                let st = mv["status"].as_str().unwrap_or("").to_string();
                let cur = markouts[h.as_str()][st.as_str()].as_i64().unwrap_or(0);
                markouts.get_mut(h).unwrap().as_object_mut().unwrap().insert(st, json!(cur + 1));
            }
        }
    }
    Ok(json!({"mode":bank["mode"],"name":bank["name"],"cutoff":bank["cutoff"],
        "sourceHash":bank["sourceHash"],"bankHash":state["bankHash"],
        "configHash":bank["configHash"],"throughIndex":state["index"],
        "chainHash":state["chainHash"],"updatedAt":state["lastAt"],
        "ready":state["ready"],"quotePairs":state["pairs"],
        "counts":state["counts"],"markoutStatuses":V::Object(markouts),
        "execution":{"ordersSubmitted":0,"simulatedFills":V::Null,
            "partialFillQuantity":V::Null,"fillProbability":V::Null,"pnl":V::Null},
        "evidence":{"localContinuityOnly":true,"completeHistoricalTradeTape":false,
            "aggressorSideKnown":false,"queuePriorityKnown":false,
            "visibleQueueNeverDecrementedByDisappearanceOrTrade":true,
            "quoteCrossingIsNotFill":true,"profitabilityEstablished":false}}))
}
/// replayPassive(records,bank)
pub fn replay_passive(records: &V, bank: &V) -> Result<V, String> {
    check(records.is_array(), "Observer journal records required")?;
    let created = create_passive_observer(bank)?;
    check(digest(&created["bank"]) == digest(bank), "Unexpected observer bank fields")?;
    let mut state = created["state"].clone();
    for record in records.as_array().cloned().unwrap_or_default() {
        let out = observe_passive(&state, &record["event"], bank)?;
        check(
            !out["record"].is_null() && digest(&out["record"]) == digest(&record),
            "Observer journal hash or payload mismatch",
        )?;
        state = out["state"].clone();
    }
    Ok(state)
}
