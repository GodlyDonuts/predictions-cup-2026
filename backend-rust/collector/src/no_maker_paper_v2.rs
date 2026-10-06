// live-paper/no-maker-paper-v2.mjs — hypothetical strict trade-through paper bank with coverage windows.
use baal_execution::core::payload_hash;
use baal_paper::fixed::{big_amount, money, SCALE, TICK};
use baal_service_runtime::contract::{instant_ns, parse_iso_ms};
use baal_service_runtime::json_text::js_truthy;
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok {
        return Err(format!("NO_MAKER_{}", code));
    }
    Ok(())
}
// paper/fixed.mjs instant(value): throws plain errors (no NO_MAKER_ prefix).
fn instant(v: &V) -> Result<i64, String> { crate::model::instant(v.as_str().unwrap_or("")) }
fn instant_ns_of(v: &V) -> Result<i128, String> {
    instant_ns(v.as_str().unwrap_or("")).map_err(|e| e.0)
}
// Date.parse(value) — NaN on failure (all `>=`/`<` comparisons false), like JS.
fn date_parse(v: &V) -> f64 { parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN) }
fn jnum(v: &V) -> f64 {
    match v {
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::String(s) => s.trim().parse::<f64>().unwrap_or(f64::NAN),
        V::Bool(b) => i32::from(*b) as f64,
        V::Null => 0.0,
        _ => f64::NAN,
    }
}
// JS BigInt(v): decimal string or integer number only.
fn big_json(v: &V) -> Result<BigInt, String> {
    match v {
        V::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                return Ok(BigInt::from(0));
            }
            t.parse::<BigInt>().map_err(|_| "Cannot convert to BigInt".to_string())
        }
        V::Number(n) => n
            .as_i64()
            .map(BigInt::from)
            .or_else(|| n.as_u64().map(BigInt::from))
            .ok_or_else(|| "Cannot convert to BigInt".to_string()),
        _ => Err("Cannot convert to BigInt".into()),
    }
}
// JS String(v) for JSON scalars.
fn js_string(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => {
            let x = n.as_f64().unwrap_or(0.0);
            if x.fract() == 0.0 && x.is_finite() && x.abs() < 1e21 {
                format!("{}", x as i64)
            } else {
                format!("{}", x)
            }
        }
        V::Bool(b) => b.to_string(),
        V::Null => "null".into(),
        _ => "[object Object]".into(),
    }
}
// JS === over JSON values: distinct types never equal; numbers compare by value.
fn js_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(_), V::Number(_)) => a.as_f64() == b.as_f64(),
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
// JS property order: integer-index keys ascending first, then insertion order.
fn array_index(k: &str) -> Option<u64> {
    if k.is_empty() || k.len() > 10 || !k.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if k.len() > 1 && k.starts_with('0') {
        return None;
    }
    let n: u64 = k.parse().ok()?;
    if n < 4_294_967_295 {
        Some(n)
    } else {
        None
    }
}
fn js_keys(v: &V) -> Vec<String> {
    let m = match v.as_object() {
        Some(m) => m,
        None => return vec![],
    };
    let mut ints: Vec<(u64, String)> = vec![];
    let mut rest: Vec<String> = vec![];
    for k in m.keys() {
        match array_index(k) {
            Some(n) => ints.push((n, k.clone())),
            None => rest.push(k.clone()),
        }
    }
    ints.sort_by_key(|x| x.0);
    ints.into_iter().map(|x| x.1).chain(rest).collect()
}
fn js_values(v: &V) -> Vec<V> { js_keys(v).into_iter().map(|k| v[&k].clone()).collect() }
fn prop_key(v: &V) -> String { js_string(v) }
fn falsy(v: &V, k: &str) -> bool { !js_truthy(v.get(k)) }
// v.k !== null — JS: missing (undefined) !== null is true.
fn not_null(v: &V, k: &str) -> bool { v.get(k).map(|x| !x.is_null()).unwrap_or(true) }
fn del(v: &mut V, k: &str) {
    if let Some(m) = v.as_object_mut() {
        m.remove(k);
    }
}

/// NO_MAKER_POLICY
pub fn no_maker_policy() -> V {
    json!({"schemaVersion":1,"initialCash":"100.000000","maximumReservedAndOwned":"5.000000",
        "maximumOwnedMarkets":2,"quantity":1,"latencyMs":3000,"lifetimeMs":120000,"cooldownMs":60000,
        "maximumHoldMs":1800000,"stopLoss":"0.030000","exitAdverseTicks":1,"depthFractionBps":100,
        "feePerShare":"0.000000","families":["at_quote","improve_one_tick"],
        "fillModel":"hypothetical_strict_trade_through_not_proven_execution"})
}
/// createNoMakerState(bank)
pub fn create_no_maker_state(bank: &V) -> Result<V, String> {
    need(
        bank["mode"].as_str() == Some("no_maker_trade_tape_paper")
            && payload_hash(&bank["policy"])? == payload_hash(&no_maker_policy())?,
        "BANK",
    )?;
    instant(&bank["cutoff"])?;
    let mut wallets = serde_json::Map::new();
    for f in bank["policy"]["families"].as_array().cloned().unwrap_or_default() {
        wallets.insert(
            prop_key(&f),
            json!({"cash": bank["policy"]["initialCash"], "realized": "0.000000", "fees": "0.000000",
                "positions": {}, "quotes": {}, "fills": [], "cooldowns": {}, "risk": {}}),
        );
    }
    Ok(json!({"schemaVersion":1,"bankHash": payload_hash(bank)?,
        "lastAt": bank["cutoff"], "nextId": 1, "seen": {}, "books": {}, "coverage": {}, "counts": {},
        "wallets": wallets}))
}
fn inc(s: &mut V, k: &str) {
    s["counts"][k] = json!(s["counts"][k].as_i64().unwrap_or(0) + 1);
}
fn reserved(w: &V) -> BigInt {
    js_values(&w["quotes"])
        .iter()
        .filter(|q| falsy(q, "resolvedAt") && q["action"].as_str() == Some("buy"))
        .map(|q| {
            big_amount(q["limit"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0))
                * big_json(&q["quantity"]).unwrap_or(BigInt::from(0))
        })
        .sum()
}
fn owned(w: &V) -> BigInt {
    js_values(&w["positions"])
        .iter()
        .map(|p| big_amount(p["cost"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0)))
        .sum()
}
/// assertNoMaker(state, bank)
pub fn assert_no_maker(state: &V, bank: &V) -> Result<(), String> {
    need(
        state["bankHash"].as_str().is_some() && state["bankHash"].as_str() == Some(payload_hash(bank)?.as_str()),
        "BANK_CHANGED",
    )?;
    for w in js_values(&state["wallets"]) {
        let buys: BigInt = w["fills"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|f| f["action"].as_str() == Some("buy"))
            .map(|f| {
                big_amount(f["price"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0))
                    * big_json(&f["quantity"]).unwrap_or(BigInt::from(0))
            })
            .sum();
        let sells: BigInt = w["fills"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|f| f["action"].as_str() == Some("sell"))
            .map(|f| {
                big_amount(f["price"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0))
                    * big_json(&f["quantity"]).unwrap_or(BigInt::from(0))
            })
            .sum();
        need(
            big_amount(w["cash"].as_str().unwrap_or(""))?
                == big_amount(bank["policy"]["initialCash"].as_str().unwrap_or(""))? - &buys + &sells
                && big_amount(w["cash"].as_str().unwrap_or(""))? >= reserved(&w),
            "CASH_CONSERVATION",
        )?;
        need(
            owned(&w) + reserved(&w)
                <= big_amount(bank["policy"]["maximumReservedAndOwned"].as_str().unwrap_or(""))?
                && js_values(&w["positions"]).iter().all(|p| js_eq(&p["quantity"], &json!(1))),
            "RISK",
        )?;
        need(
            big_amount(w["realized"].as_str().unwrap_or(""))? == &sells - (&buys - owned(&w))
                && w["fees"].as_str() == Some("0.000000"),
            "FIFO_CONSERVATION",
        )?;
    }
    Ok(())
}
fn fill(s: &mut V, w: &mut V, q: &mut V, at: &str, source: &V, bank: &V) -> Result<(), String> {
    let cost = big_amount(q["limit"].as_str().unwrap_or(""))? * big_json(&q["quantity"])?;
    let key = prop_key(&q["exchangeId"]);
    if q["action"].as_str() == Some("buy") {
        need(
            !w["positions"].as_object().is_some_and(|p| p.contains_key(&key))
                && big_amount(w["cash"].as_str().unwrap_or(""))? >= cost,
            "BUY_RESERVATION",
        )?;
        w["cash"] = json!(money(&(big_amount(w["cash"].as_str().unwrap_or(""))? - &cost)));
        w["positions"][&key] = json!({"marketId": q["marketId"], "exchangeId": key,
            "quantity": q["quantity"], "cost": money(&cost), "enteredAt": at,
            "sourceEntryAt": if source["kind"].as_str() == Some("trade") { source["createdAt"].clone() } else { json!(at) }});
    } else {
        let p = w["positions"][&key].clone();
        need(js_eq(&p["quantity"], &q["quantity"]), "COVERED_EXIT")?;
        w["cash"] = json!(money(&(big_amount(w["cash"].as_str().unwrap_or(""))? + &cost)));
        w["realized"] = json!(money(
            &(big_amount(w["realized"].as_str().unwrap_or(""))? + &cost
                - big_amount(p["cost"].as_str().unwrap_or(""))?)
        ));
        del(&mut w["positions"], &key);
        del(&mut w["risk"], &key);
        let at_ms = instant(&json!(at))?;
        w["cooldowns"][&key] = json!(at_ms + bank["policy"]["cooldownMs"].as_i64().unwrap_or(0));
    }
    q["resolvedAt"] = json!(at);
    q["resolution"] = json!("hypothetical_fill");
    if let Some(f) = w["fills"].as_array_mut() {
        f.push(json!({"action": q["action"], "marketId": q["marketId"], "exchangeId": key,
            "quantity": q["quantity"], "price": q["limit"], "at": at, "quoteId": q["id"],
            "model": if source["kind"].as_str() == Some("trade") { bank["policy"]["fillModel"].clone() } else { json!("hypothetical_later_request_risk_taker") },
            "source": source.clone()}));
    }
    inc(s, &format!("hypothetical_{}", q["action"].as_str().unwrap_or("")));
    Ok(())
}
fn open_for(w: &V, key: &str) -> bool {
    js_values(&w["quotes"])
        .iter()
        .any(|q| q["exchangeId"].as_str() == Some(key) && falsy(q, "resolvedAt"))
}
fn record_coverage(s: &mut V, page: &V) -> Result<(), String> {
    let key = prop_key(&page["exchangeId"]);
    if s["coverage"].get(&key).map(|v| v.is_null()).unwrap_or(true) {
        s["coverage"][&key] = json!({"pending": V::Null, "windows": []});
    }
    let mut c = s["coverage"][&key].clone();
    let request = match page.get("requestCursor") {
        Some(v) if !v.is_null() => v.clone(),
        _ => V::Null,
    };
    let mut chain = if request.is_null() {
        json!({"from": page["from"], "to": page["to"], "expected": V::Null, "complete": true,
            "checkpoint": V::Null, "pages": 0})
    } else {
        c["pending"].clone()
    };
    if chain.is_null()
        || !js_eq(&chain["expected"], &request)
        || !js_eq(&chain["from"], &page["from"])
        || !js_eq(&chain["to"], &page["to"])
    {
        c["pending"] = V::Null;
        s["coverage"][&key] = c;
        inc(s, "coverage_cursor_gap");
        return Ok(());
    }
    let checkpoint = page["coverage"]["projectedThroughSequence"].clone();
    chain["complete"] = json!(
        js_truthy(Some(&chain["complete"]))
            && page["coverage"]["complete"] == V::Bool(true)
            && !checkpoint.is_null()
    );
    if !checkpoint.is_null() {
        chain["checkpoint"] = if chain["checkpoint"].is_null() {
            json!(js_string(&checkpoint))
        } else if big_json(&chain["checkpoint"])? < big_json(&checkpoint)? {
            json!(js_string(&chain["checkpoint"]))
        } else {
            json!(js_string(&checkpoint))
        };
    }
    chain["pages"] = json!(chain["pages"].as_i64().unwrap_or(0) + 1);
    if js_truthy(page["pagination"].get("hasMore")) {
        let nc = page["pagination"]["nextCursor"].clone();
        if nc.as_str().map(|x| x.is_empty()).unwrap_or(true) || js_eq(&nc, &request) {
            c["pending"] = V::Null;
            s["coverage"][&key] = c;
            inc(s, "coverage_cursor_gap");
            return Ok(());
        }
        chain["expected"] = nc;
        c["pending"] = chain;
    } else {
        let complete = js_truthy(Some(&chain["complete"]));
        if complete {
            if let Some(w) = c["windows"].as_array_mut() {
                w.push(json!({"from": chain["from"], "to": chain["to"], "checkpoint": chain["checkpoint"]}));
            }
        }
        c["pending"] = V::Null;
        if !complete {
            s["coverage"][&key] = c.clone();
            inc(s, "coverage_incomplete_window");
        }
    }
    s["coverage"][&key] = c;
    let active: Vec<i64> = js_values(&s["wallets"])
        .iter()
        .flat_map(|w| js_values(&w["quotes"]))
        .filter(|q| falsy(q, "resolvedAt") && js_eq(&q["exchangeId"], &page["exchangeId"]))
        .map(|q| instant(&q["activationAt"]))
        .collect::<Result<_, _>>()?;
    let oldest = active.iter().copied().min().unwrap_or(instant(&s["lastAt"])?);
    let mut c = s["coverage"][&key].clone();
    let mut kept = Vec::new();
    for w in c["windows"].as_array().cloned().unwrap_or_default() {
        if instant(&w["to"])? > oldest {
            kept.push(w);
        }
    }
    c["windows"] = json!(kept);
    s["coverage"][&key] = c;
    Ok(())
}
fn covers_quote(coverage: &V, q: &V) -> Result<bool, String> {
    let mut through = instant(&q["activationAt"])?;
    let barrier = big_json(&q["coverageBarrierSequence"])?;
    let mut windows: Vec<(i64, V)> = Vec::new();
    for w in coverage
        .get("windows")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default()
    {
        if big_json(&w["checkpoint"])? >= barrier {
            windows.push((instant(&w["from"])?, w));
        }
    }
    windows.sort_by_key(|x| x.0);
    for (from, w) in windows {
        if from > through {
            break;
        }
        through = through.max(instant(&w["to"])?);
        if through >= instant(&q["expiresAt"])? {
            return Ok(true);
        }
    }
    Ok(false)
}
#[allow(clippy::too_many_arguments)]
fn quote(s: &mut V, w: &mut V, book: &V, at: &str, action: &str, limit: &str, bank: &V) -> Result<(), String> {
    let px = big_amount(limit)?;
    need(
        px >= BigInt::from(TICK) && px <= BigInt::from(SCALE - TICK) && &px % BigInt::from(TICK) == BigInt::from(0),
        "QUOTE_TICK",
    )?;
    let activation = (instant(&json!(at))? + bank["policy"]["latencyMs"].as_i64().unwrap_or(0))
        .max((instant_ns_of(&book["sourceAt"])? / 1_000_000 + 2_000) as i64);
    let id = s["nextId"].as_i64().unwrap_or(0);
    s["nextId"] = json!(id + 1);
    let q = json!({"id": id, "action": action, "marketId": book["marketId"], "exchangeId": book["exchangeId"],
        "quantity": 1, "limit": limit,
        "yesEquivalentLimit": money(&(BigInt::from(SCALE) - &px)),
        "createdAt": at,
        "activationAt": baal_service_runtime::contract::ms_to_iso(activation as f64),
        "expiresAt": baal_service_runtime::contract::ms_to_iso((instant(&json!(at))? + bank["policy"]["lifetimeMs"].as_i64().unwrap_or(0)) as f64),
        "bookVersion": book["versionKey"], "bookSequence": book["sequence"],
        "publicationHash": book["publicationHash"], "publicationSequence": book["publicationSequence"],
        "coverageBarrierSequence": V::Null, "resolvedAt": V::Null, "resolution": V::Null});
    w["quotes"][id.to_string()] = q;
    inc(s, &format!("quote_{}", action));
    Ok(())
}
/// advanceNoMaker(state, event, bank)
pub fn advance_no_maker(state: &V, event: &V, bank: &V) -> Result<V, String> {
    need(
        state["bankHash"].as_str().is_some() && state["bankHash"].as_str() == Some(payload_hash(bank)?.as_str()),
        "BANK_CHANGED",
    )?;
    let mut s = state.clone();
    let p = &bank["policy"];
    let at_ms = instant(&event["availableAt"])?;
    need(
        at_ms >= instant(&s["lastAt"])? && at_ms >= instant(&bank["cutoff"])?,
        "CONSUMER_CLOCK",
    )?;
    s["lastAt"] = event["availableAt"].clone();
    let at = event["availableAt"].as_str().unwrap_or("").to_string();
    let kind = event["kind"].as_str().unwrap_or("");
    if kind == "boundary" {
        let wallet_keys = js_keys(&s["wallets"]);
        for wk in wallet_keys {
            let qkeys = js_keys(&s["wallets"][&wk]["quotes"]);
            for qk in qkeys {
                let mut q = s["wallets"][&wk]["quotes"][&qk].clone();
                if falsy(&q, "resolvedAt") && at_ms < instant(&q["expiresAt"])? {
                    q["expiresAt"] = json!(at);
                }
                s["wallets"][&wk]["quotes"][&qk] = q;
            }
        }
        s["books"] = json!({});
        inc(&mut s, "source_boundary");
    } else if kind == "book" {
        let b = &event["book"];
        let key = prop_key(&b["exchangeId"]);
        need(
            bank["legs"].as_array().cloned().unwrap_or_default().iter().any(|l| {
                js_eq(&l["marketId"], &b["marketId"]) && js_eq(&l["exchangeId"], &b["exchangeId"])
            }) && b["instrument"].as_str() == Some(format!("{}:YES", key).as_str()),
            "BOOK_SCOPE",
        )?;
        need(instant(&b["receivedAt"])? <= at_ms, "FUTURE_RECEIPT")?;
        for id in js_keys(&s["books"]) {
            let mark = s["books"][&id].clone();
            if crate::probability_receipt::probability_account_gate(&event["accountEligibility"], &mark, &at)
                .is_some()
            {
                del(&mut s["books"], &id);
            }
        }
        let fresh = at_ms - instant(&b["requestStartedAt"])? <= 20_000
            && crate::probability_receipt::probability_account_gate(&event["accountEligibility"], b, &at)
                .is_none();
        if fresh {
            s["books"][&key] = b.clone();
        } else {
            del(&mut s["books"], &key);
        }
        let wallet_keys = js_keys(&s["wallets"]);
        for family in wallet_keys {
            let mut w = s["wallets"][&family].clone();
            let qkeys = js_keys(&w["quotes"]);
            for qk in qkeys {
                let mut q = w["quotes"][&qk].clone();
                if falsy(&q, "resolvedAt")
                    && js_eq(&q["exchangeId"], &b["exchangeId"])
                    && instant(&b["requestStartedAt"])? >= instant(&q["expiresAt"])?
                    && instant_ns_of(&b["sourceAt"])?
                        >= (instant(&q["expiresAt"])? as i128) * 1_000_000
                {
                    q["coverageBarrierSequence"] = b["sequence"].clone();
                }
                w["quotes"][&qk] = q;
            }
            if !fresh {
                inc(&mut s, "book_not_fresh_or_excluded");
                s["wallets"][&family] = w;
                continue;
            }
            let position = w["positions"][&key].clone();
            let bid = BigInt::from(SCALE) - big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))?;
            let ask = BigInt::from(SCALE) - big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?;
            if !position.is_null() {
                let mark = &bid - BigInt::from(TICK);
                let held_ms = at_ms as f64 - date_parse(&position["sourceEntryAt"]);
                let stop = big_amount(position["cost"].as_str().unwrap_or(""))? - &mark
                    >= big_amount(p["stopLoss"].as_str().unwrap_or(""))?
                    || held_ms >= jnum(&p["maximumHoldMs"]);
                if stop && falsy(&w["risk"], &key) {
                    w["risk"][&key] = json!({"createdAt": at,
                        "reason": if held_ms >= jnum(&p["maximumHoldMs"]) { "holding_limit" } else { "loss_limit" }});
                    let qkeys = js_keys(&w["quotes"]);
                    for qk in qkeys {
                        let mut q = w["quotes"][&qk].clone();
                        if js_eq(&q["exchangeId"], &b["exchangeId"])
                            && falsy(&q, "resolvedAt")
                            && at_ms < instant(&q["expiresAt"])?
                        {
                            q["expiresAt"] = json!(at);
                        }
                        w["quotes"][&qk] = q;
                    }
                }
                if js_truthy(w["risk"].get(&key)) {
                    // Pending resting exits must be resolved by the reporting projection before
                    // a second, potentially duplicate, hypothetical sell is admitted.
                    if !open_for(&w, &key)
                        && instant(&b["requestStartedAt"])?
                            >= instant(&w["risk"][&key]["createdAt"])?
                                + p["latencyMs"].as_i64().unwrap_or(0)
                        && mark > BigInt::from(0)
                        && (jnum(&b["asks"][0]["quantity"]) * jnum(&p["depthFractionBps"]) / 10000.0).floor()
                            >= 1.0
                    {
                        let id = s["nextId"].as_i64().unwrap_or(0);
                        s["nextId"] = json!(id + 1);
                        let mut q = json!({"id": id, "action": "sell", "marketId": b["marketId"],
                            "exchangeId": key, "quantity": 1, "limit": money(&mark)});
                        fill(
                            &mut s,
                            &mut w,
                            &mut q,
                            &at,
                            &json!({"kind": "book", "version": b["versionKey"],
                                "publicationHash": b["publicationHash"],
                                "publicationSequence": b["publicationSequence"]}),
                            bank,
                        )?;
                    }
                    s["wallets"][&family] = w;
                    continue;
                }
                if !open_for(&w, &key) {
                    let limit = &ask
                        - if family == "improve_one_tick" && &ask - &bid >= BigInt::from(3 * TICK) {
                            BigInt::from(TICK)
                        } else {
                            BigInt::from(0)
                        };
                    if limit > bid && limit > big_amount(position["cost"].as_str().unwrap_or(""))? {
                        quote(&mut s, &mut w, b, &at, "sell", &money(&limit), bank)?;
                    }
                }
                s["wallets"][&family] = w;
                continue;
            }
            if !js_truthy(event.get("entryAllowed"))
                || instant(&b["requestStartedAt"])? <= instant(&bank["cutoff"])?
                || at_ms >= instant(&bank["reviewExpiresAt"])?
                || open_for(&w, &key)
                || {
                    let cd = &w["cooldowns"][&key];
                    (if js_truthy(Some(cd)) { jnum(cd) } else { 0.0 }) > at_ms as f64
                }
            {
                s["wallets"][&family] = w;
                continue;
            }
            let buy_keys: BTreeSet<String> = js_values(&w["quotes"])
                .iter()
                .filter(|q| falsy(q, "resolvedAt") && q["action"].as_str() == Some("buy"))
                .map(|q| prop_key(&q["exchangeId"]))
                .collect();
            if w["positions"].as_object().map(|m| m.len()).unwrap_or(0) + buy_keys.len()
                >= p["maximumOwnedMarkets"].as_i64().unwrap_or(0) as usize
            {
                s["wallets"][&family] = w;
                continue;
            }
            let improve = family == "improve_one_tick" && &ask - &bid >= BigInt::from(3 * TICK);
            let limit = &bid + if improve { BigInt::from(TICK) } else { BigInt::from(0) };
            if &ask - &limit < BigInt::from(2 * TICK)
                || (jnum(&b["asks"][0]["quantity"]) * jnum(&p["depthFractionBps"]) / 10000.0).floor() < 1.0
            {
                inc(&mut s, "insufficient_spread_or_depth");
                s["wallets"][&family] = w;
                continue;
            }
            if owned(&w) + reserved(&w) + &limit
                > big_amount(p["maximumReservedAndOwned"].as_str().unwrap_or(""))?
                || big_amount(w["cash"].as_str().unwrap_or(""))? - reserved(&w) < limit
            {
                s["wallets"][&family] = w;
                continue;
            }
            quote(&mut s, &mut w, b, &at, "buy", &money(&limit), bank)?;
            s["wallets"][&family] = w;
        }
    } else if kind == "trades" {
        let page = &event["page"];
        need(
            js_eq(&page["tournamentId"], &bank["tournamentId"])
                && bank["legs"].as_array().cloned().unwrap_or_default().iter().any(|l| {
                    js_eq(&l["marketId"], &page["marketId"]) && js_eq(&l["exchangeId"], &page["exchangeId"])
                })
                && instant(&page["receivedAt"])? <= at_ms,
            "PAGE_SCOPE",
        )?;
        let mut trades = page["trades"].as_array().cloned().unwrap_or_default();
        for t in &trades {
            instant_ns_of(&t["createdAt"])?;
        }
        trades.sort_by(|a, b| {
            let an = instant_ns_of(&a["createdAt"]).unwrap_or(0);
            let bn = instant_ns_of(&b["createdAt"]).unwrap_or(0);
            an.cmp(&bn).then_with(|| {
                a["key"].as_str().unwrap_or("").encode_utf16().cmp(b["key"].as_str().unwrap_or("").encode_utf16())
            })
        });
        for t in &trades {
            need(
                instant_ns_of(&t["createdAt"])? < (instant(&page["to"])? as i128) * 1_000_000
                    && instant_ns_of(&t["createdAt"])? >= (instant(&page["from"])? as i128) * 1_000_000,
                "TRADE_WINDOW",
            )?;
            let tkey = prop_key(&t["key"]);
            if js_truthy(s["seen"].get(tkey.as_str())) {
                continue;
            }
            s["seen"][&tkey] = json!(true);
            let tick = crate::canonical_entry_exit_study::tape_tick(t)?;
            if tick.is_none()
                || tick.as_ref().and_then(|x| x["tick"].as_i64()) == Some(0)
                || tick.as_ref().and_then(|x| x["tick"].as_i64()) == Some(200)
            {
                inc(&mut s, "unusable_trade_price");
                continue;
            }
            let tick = tick.unwrap();
            let wallet_keys = js_keys(&s["wallets"]);
            for wk in wallet_keys {
                let qkeys = js_keys(&s["wallets"][&wk]["quotes"]);
                for qk in qkeys {
                    let q = s["wallets"][&wk]["quotes"][&qk].clone();
                    if js_truthy(q.get("resolvedAt"))
                        || !js_eq(&q["exchangeId"], &page["exchangeId"])
                        || instant_ns_of(&t["createdAt"])?
                            < (instant(&q["activationAt"])? as i128) * 1_000_000
                        || instant_ns_of(&t["createdAt"])?
                            >= (instant(&q["expiresAt"])? as i128) * 1_000_000
                    {
                        continue;
                    }
                    let yes = big_amount(tick["price"].as_str().unwrap_or(""))?;
                    let limit = big_amount(q["yesEquivalentLimit"].as_str().unwrap_or(""))?;
                    let through = if q["action"].as_str() == Some("buy") {
                        yes >= &limit + BigInt::from(TICK)
                    } else {
                        yes <= &limit - BigInt::from(TICK)
                    };
                    if !through {
                        continue;
                    }
                    need(jnum(&t["size"]) >= 1.0, "TRADE_QUANTITY")?;
                    let mut w = s["wallets"][&wk].clone();
                    let mut q = q;
                    fill(
                        &mut s,
                        &mut w,
                        &mut q,
                        &at,
                        &json!({"kind": "trade", "key": t["key"], "id": t["id"],
                            "createdAt": t["createdAt"], "firstAvailableAt": page["receivedAt"],
                            "normalizedYesPrice": tick["price"], "rawYesPrice": t["yesPrice"],
                            "priceRepresentation": tick["representation"],
                            "aggressorSide": V::Null, "queuePositionProven": false,
                            "executionProven": false}),
                        bank,
                    )?;
                    w["quotes"][&qk] = q;
                    s["wallets"][&wk] = w;
                }
            }
        }
        // Every portion of the active interval needs a fully paginated reported
        // window with a checkpoint beyond the post-expiry book. Later windows
        // cannot erase a gap or upgrade an older window's projection checkpoint.
        record_coverage(&mut s, page)?;
        let cov = s["coverage"][prop_key(&page["exchangeId"])].clone();
        let wallet_keys = js_keys(&s["wallets"]);
        for wk in wallet_keys {
            let qkeys = js_keys(&s["wallets"][&wk]["quotes"]);
            for qk in qkeys {
                let mut q = s["wallets"][&wk]["quotes"][&qk].clone();
                if falsy(&q, "resolvedAt")
                    && js_eq(&q["exchangeId"], &page["exchangeId"])
                    && not_null(&q, "coverageBarrierSequence")
                    && covers_quote(&cov, &q)?
                {
                    q["resolvedAt"] = json!(at);
                    q["resolution"] = json!("projection_observed_without_qualifying_trade");
                    inc(&mut s, "quote_unfilled_projection_resolved");
                }
                s["wallets"][&wk]["quotes"][&qk] = q;
            }
        }
    } else {
        need(false, "EVENT_KIND")?;
    }
    assert_no_maker(&s, bank)?;
    Ok(s)
}
/// noMakerReport(s, bank, at)
pub fn no_maker_report(s: &V, bank: &V, at: &str) -> Result<V, String> {
    assert_no_maker(s, bank)?;
    let at_ms = instant(&json!(at))?;
    let mut wallets = serde_json::Map::new();
    for family in js_keys(&s["wallets"]) {
        let w = &s["wallets"][&family];
        let mut proceeds = BigInt::from(0);
        let mut complete = true;
        let mut positions = Vec::new();
        for pos in js_values(&w["positions"]) {
            let pkey = prop_key(&pos["exchangeId"]);
            let b = s["books"].as_object().and_then(|m| m.get(&pkey)).cloned().unwrap_or(V::Null);
            let fresh = js_truthy(Some(&b))
                && at_ms >= instant(&b["receivedAt"])?
                && at_ms - instant(&b["receivedAt"])? <= 20_000;
            let value = if fresh
                && (jnum(&b["asks"][0]["quantity"]) / 100.0).floor() >= jnum(&pos["quantity"])
            {
                Some(BigInt::from(SCALE) - big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))? - BigInt::from(TICK))
            } else {
                None
            };
            if value.is_none() || *value.as_ref().unwrap() <= BigInt::from(0) {
                complete = false;
            } else {
                proceeds += value.as_ref().unwrap() * big_json(&pos["quantity"])?;
            }
            let mut out = pos.clone();
            out["estimatedLiquidationProceeds"] = match &value {
                None => V::Null,
                Some(v) => json!(money(&(v * big_json(&pos["quantity"])?))),
            };
            out["quoteAgeMs"] = if js_truthy(Some(&b)) {
                json!(at_ms - instant(&b["receivedAt"])?)
            } else {
                V::Null
            };
            positions.push(out);
        }
        let fills_len = w["fills"].as_array().map(|f| f.len()).unwrap_or(0);
        let closed = w["fills"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|f| f["action"].as_str() == Some("sell"))
            .count();
        let unresolved: Vec<V> = js_values(&w["quotes"])
            .into_iter()
            .filter(|q| falsy(q, "resolvedAt"))
            .collect();
        let fills = w["fills"].as_array().cloned().unwrap_or_default();
        let recent: Vec<V> = fills.iter().skip(fills_len.saturating_sub(10)).cloned().collect();
        let liquidation_pnl = if complete {
            json!(money(
                &(big_amount(w["cash"].as_str().unwrap_or(""))? + &proceeds
                    - big_amount(bank["policy"]["initialCash"].as_str().unwrap_or(""))?)
            ))
        } else {
            V::Null
        };
        wallets.insert(
            family,
            json!({"cash": w["cash"], "realizedPnl": w["realized"], "fees": w["fees"],
                "fills": fills_len, "closedTrades": closed, "positions": positions,
                "unresolvedQuotes": unresolved, "reserved": money(&reserved(w)),
                "recentFills": recent, "estimatedLiquidationPnl": liquidation_pnl}),
        );
    }
    Ok(json!({"schemaVersion":1,"mode": bank["mode"], "cohort": bank["name"],
        "bankHash": s["bankHash"], "updatedAt": at, "cutoff": bank["cutoff"], "counts": s["counts"],
        "wallets": wallets,
        "executionAuthority": false, "competitionOrders": 0, "networkRequests": 0,
        "profitabilityEstablished": false,
        "limitations": ["All executions are local counterfactual models based on strict price-through or later-request risk depth.",
            "Anonymous trade space is not aggressor side; unseen market reaction and hidden liquidity can change actual fills.",
            "Two quote families share correlated opportunities; paper profit is not official-account profit."]}))
}
