// paper/inventory-quote-wallet-v1.mjs — counterfactual inventory quoting
// wallet. Deps feed/contract.mjs instantNs + live-paper tapeTick are inlined
// below (baal-paper only depends on baal-service-runtime).
use crate::fixed::{big_amount, instant, money, SCALE, TICK};
use crate::inventory_aware_quote_v1::inventory_aware_quotes_v1;
use baal_service_runtime::contract::instant_ns;
use baal_service_runtime::json_text::sha256;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::BTreeMap;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn amount(v: &V) -> Result<BigInt, String> {
    big_amount(v.as_str().unwrap_or(""))
}
fn scale() -> BigInt {
    BigInt::from(SCALE)
}
fn tick() -> BigInt {
    BigInt::from(TICK)
}
fn instant_ms(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn instant_ns_v(v: &V) -> Result<i128, String> {
    instant_ns(v.as_str().unwrap_or("")).map_err(|e| e.to_string())
}
fn js_int(v: &V) -> Option<i64> {
    v.as_i64()
}
fn obj_insert(v: &mut V, k: &str, x: V) {
    v.as_object_mut().unwrap().insert(k.to_string(), x);
}
fn err<T>(c: &str) -> Result<T, String> {
    Err(c.to_string())
}

fn format_micros(value: i128) -> String {
    format!("{}.{:06}", value / SCALE as i128, value % SCALE as i128)
}

// execution/core.mjs payloadHash (canonical sorted-key JSON + sha256).
fn canonical_payload(value: &V) -> Result<String, String> {
    fn walk(v: &V, out: &mut String) -> Result<(), String> {
        match v {
            V::Null => out.push_str("null"),
            V::Bool(_) | V::Number(_) | V::String(_) => {
                out.push_str(&serde_json::to_string(v).map_err(|e| e.to_string())?)
            }
            V::Array(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    walk(x, out)?;
                }
                out.push(']');
            }
            V::Object(m) => {
                out.push('{');
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
                for (i, k) in keys.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(k).map_err(|e| e.to_string())?);
                    out.push(':');
                    walk(&m[*k], out)?;
                }
                out.push('}');
            }
        }
        Ok(())
    }
    let mut s = String::new();
    walk(value, &mut s)?;
    Ok(s)
}
fn payload_hash(value: &V) -> Result<String, String> {
    Ok(sha256(canonical_payload(value)?.as_bytes()))
}

// execution/gates.mjs normalizeOfficialFillTick: accept exactly the binary
// representations of each official tick plus IEEE-754 complement artifacts.
fn normalize_official_fill_tick(value: &V) -> Result<V, String> {
    fn build_representations() -> BTreeMap<u64, V> {
        let mut map: BTreeMap<u64, V> = BTreeMap::new();
        for tick_i in 0..=200i64 {
            let micros = tick_i as i128 * TICK as i128;
            let price = format_micros(micros);
            let canonical: f64 = price.parse().unwrap();
            let opposite: f64 = format_micros(SCALE as i128 - micros).parse().unwrap();
            for (v, kind) in [
                (canonical, "canonical_tick"),
                (1.0 - opposite, "binary_tick_complement"),
                (1.0 - (1.0 - canonical), "binary_complement_roundtrip"),
            ] {
                if let Some(prior) = map.get(&v.to_bits()) {
                    if prior.get("price").and_then(V::as_str) != Some(price.as_str()) {
                        panic!("API_TICK_REPRESENTATION_COLLISION");
                    }
                } else {
                    map.insert(
                        v.to_bits(),
                        json!({"price": price, "tick": tick_i, "representation": kind}),
                    );
                }
            }
        }
        map
    }
    if let Some(n) = value.as_f64() {
        let found = if n.is_finite() && n.to_bits() != (-0.0f64).to_bits() {
            build_representations().get(&n.to_bits()).cloned()
        } else {
            None
        };
        return found.ok_or_else(|| "UNRECOGNIZED_API_TICK_REPRESENTATION".to_string());
    }
    err("UNRECOGNIZED_API_TICK_REPRESENTATION")
}

// live-paper/canonical-entry-exit-study.mjs tapeTick.
fn tape_tick(trade: &V) -> Result<Option<V>, String> {
    if trade["yesPrice"].is_null() {
        return Ok(None);
    }
    if !(trade["yesPrice"].as_str().is_some_and(|s| {
        s.parse::<f64>().map(|n| n.to_string() == s).unwrap_or(false)
    })) {
        return err("RAW_TRADE_PRICE");
    }
    let n: f64 = trade["yesPrice"]
        .as_str()
        .unwrap()
        .parse()
        .map_err(|_| "RAW_TRADE_PRICE".to_string())?;
    match normalize_official_fill_tick(&json!(n)) {
        Ok(v) => Ok(Some(v)),
        Err(_) => Ok(None),
    }
}

pub fn create_inventory_quote_wallet_v1(policy: &V) -> Result<V, String> {
    instant_ms(get(policy, "cutoff"))?;
    let ints_ok = [
        "quantity",
        "maximumMarkets",
        "latencyMs",
        "lifetimeMs",
        "maximumGapMs",
        "minimumSamples",
        "minimumHistoryMs",
        "depthFractionBps",
    ]
    .iter()
    .all(|k| matches!(js_int(get(policy, k)), Some(n) if n >= 0));
    if amount(get(policy, "initialCash"))? <= BigInt::from(0)
        || amount(get(policy, "maximumCommitted"))? <= BigInt::from(0)
        || amount(get(policy, "maximumOrderCost"))? <= BigInt::from(0)
        || !ints_ok
        || js_int(get(policy, "quantity")).unwrap_or(0) < 1
        || js_int(get(policy, "quantity")).unwrap_or(0) > 100
        || js_int(get(policy, "maximumMarkets")).unwrap_or(0) < 1
        || js_int(get(policy, "latencyMs")).unwrap_or(0) < 1
        || js_int(get(policy, "lifetimeMs")).unwrap_or(0)
            <= js_int(get(policy, "latencyMs")).unwrap_or(0)
        || js_int(get(policy, "maximumGapMs")).unwrap_or(0) < 1
        || js_int(get(policy, "minimumSamples")).unwrap_or(0) < 1
        || js_int(get(policy, "depthFractionBps")).unwrap_or(0) < 1
        || js_int(get(policy, "depthFractionBps")).unwrap_or(0) > 10000
    {
        return err("INVENTORY_WALLET_CONFIGURATION");
    }
    Ok(json!({
        "policyHash": payload_hash(policy)?,
        "cash": get(policy, "initialCash").clone(),
        "realized": "0.000000",
        "positions": {},
        "quotes": [],
        "history": {},
        "tradeCoverage": {},
        "fills": [],
        "redemptions": [],
        "seenTrades": {},
        "counts": {},
        "lastAt": get(policy, "cutoff").clone(),
        "nextId": 1,
    }))
}

fn inc(s: &mut V, k: &str) {
    let counts = s
        .as_object_mut()
        .unwrap()
        .entry("counts".to_string())
        .or_insert_with(|| json!({}));
    let n = get(counts, k).as_i64().unwrap_or(0);
    counts
        .as_object_mut()
        .unwrap()
        .insert(k.to_string(), json!(n + 1));
}
fn open(s: &V) -> Vec<V> {
    get(s, "quotes")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|q| get(q, "closedAt").is_null())
        .collect()
}
fn basis(s: &V) -> Result<BigInt, String> {
    get(s, "positions")
        .as_object()
        .map(|m| {
            m.values().try_fold(BigInt::from(0), |n, p| {
                Ok::<BigInt, String>(n + amount(get(p, "cost"))?)
            })
        })
        .unwrap_or(Ok(BigInt::from(0)))
}
fn reserved(s: &V) -> Result<BigInt, String> {
    open(s).iter().try_fold(BigInt::from(0), |n, q| {
        if get(q, "action").as_str() == Some("buy") {
            Ok::<BigInt, String>(
                n + amount(get(q, "limit"))? * BigInt::from(get(q, "remaining").as_i64().unwrap_or(0)),
            )
        } else {
            Ok(n)
        }
    })
}

fn consume(position: &mut V, n: i64) -> Result<BigInt, String> {
    let mut left = n;
    let mut cost = BigInt::from(0);
    {
        let lots = position
            .as_object_mut()
            .unwrap()
            .get_mut("lots")
            .and_then(|l| l.as_array_mut())
            .unwrap();
        for l in lots.iter_mut() {
            let have = get(l, "quantity").as_i64().unwrap_or(0);
            let take = std::cmp::min(left, have);
            cost += amount(get(l, "price"))? * BigInt::from(take);
            let q = have - take;
            l.as_object_mut().unwrap().insert("quantity".into(), json!(q));
            left -= take;
            if left == 0 {
                break;
            }
        }
    }
    if left > 0 {
        return err("INVENTORY_WALLET_NAKED");
    }
    {
        let lots = position
            .as_object_mut()
            .unwrap()
            .get_mut("lots")
            .and_then(|l| l.as_array_mut())
            .unwrap();
        lots.retain(|l| get(l, "quantity").as_i64().unwrap_or(0) > 0);
    }
    let q = get(position, "quantity").as_i64().unwrap_or(0) - n;
    obj_insert(position, "quantity", json!(q));
    let new_cost = amount(get(position, "cost"))? - &cost;
    obj_insert(position, "cost", json!(money(&new_cost)));
    Ok(cost)
}

pub fn assert_inventory_quote_wallet_v1(s: &V, p: &V) -> Result<bool, String> {
    if get(s, "policyHash") != &json!(payload_hash(p)?) {
        return err("INVENTORY_WALLET_POLICY");
    }
    let mut cash = amount(get(p, "initialCash"))?;
    let mut realized = BigInt::from(0);
    for f in get(s, "fills").as_array().cloned().unwrap_or_default() {
        let value = amount(get(&f, "price"))? * BigInt::from(get(&f, "quantity").as_i64().unwrap_or(0));
        if get(&f, "action").as_str() == Some("buy") {
            cash -= &value;
        } else {
            cash += &value;
            realized += &value - amount(get(&f, "removedCost"))?;
        }
    }
    for r in get(s, "redemptions").as_array().cloned().unwrap_or_default() {
        let qty = BigInt::from(get(&r, "quantity").as_i64().unwrap_or(0));
        cash += &scale() * &qty;
        realized += &scale() * &qty - amount(get(&r, "oldCost"))? - amount(get(&r, "newCost"))?;
    }
    if cash != amount(get(s, "cash"))?
        || realized != amount(get(s, "realized"))?
        || cash < reserved(s)?
        || basis(s)? + reserved(s)? > amount(get(p, "maximumCommitted"))?
    {
        return err("INVENTORY_WALLET_CASH_OR_RISK");
    }
    for x in get(s, "positions")
        .as_object()
        .map(|m| m.values().cloned().collect::<Vec<_>>())
        .unwrap_or_default()
        .iter()
    {
        let x = x;
        let lot_qty: i64 = get(x, "lots")
            .as_array()
            .map(|a| a.iter().map(|l| get(l, "quantity").as_i64().unwrap_or(0)).sum())
            .unwrap_or(0);
        let lot_cost: BigInt = get(x, "lots")
            .as_array()
            .map(|a| {
                a.iter().try_fold(BigInt::from(0), |n, l| {
                    Ok::<BigInt, String>(
                        n + amount(get(l, "price"))?
                            * BigInt::from(get(l, "quantity").as_i64().unwrap_or(0)),
                    )
                })
            })
            .unwrap_or_else(|| Ok(BigInt::from(0)))?;
        if get(x, "quantity").as_i64().unwrap_or(0) < 1
            || get(x, "quantity").as_i64().unwrap_or(0) != lot_qty
            || amount(get(x, "cost"))? != lot_cost
        {
            return err("INVENTORY_WALLET_FIFO");
        }
    }
    for (ex, x) in get(s, "positions").as_object().cloned().unwrap_or_default() {
        let xq = get(&x, "quantity").as_i64().unwrap_or(0);
        let xside = get(&x, "side").as_str().unwrap_or("");
        let sell: i64 = open(s)
            .iter()
            .filter(|q| {
                get(q, "exchangeId").as_str() == Some(ex.as_str())
                    && get(q, "action").as_str() == Some("sell")
                    && get(q, "side").as_str() == Some(xside)
            })
            .map(|q| get(q, "remaining").as_i64().unwrap_or(0))
            .sum();
        let opposite: i64 = open(s)
            .iter()
            .filter(|q| {
                get(q, "exchangeId").as_str() == Some(ex.as_str())
                    && get(q, "action").as_str() == Some("buy")
                    && get(q, "side").as_str() != Some(xside)
            })
            .map(|q| get(q, "remaining").as_i64().unwrap_or(0))
            .sum();
        if sell + std::cmp::min(opposite, xq) > xq {
            return err("INVENTORY_WALLET_DOUBLE_PLEDGE");
        }
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn fill(s: &mut V, q_id: i64, n: i64, at: &V, source: &V) -> Result<(), String> {
    // q_id identifies the open quote inside s.quotes (its "id").
    let qi = {
        let quotes = get(s, "quotes").as_array().cloned().unwrap_or_default();
        quotes
            .iter()
            .position(|x| get(x, "id").as_i64() == Some(q_id))
            .ok_or("INVENTORY_WALLET_QUOTE")?
    };
    let q = get(s, "quotes").as_array().unwrap()[qi].clone();
    let value = amount(get(&q, "limit"))? * BigInt::from(n);
    let ex = get(&q, "exchangeId").as_str().unwrap_or("").to_string();
    let has_position = !get(get(s, "positions"), &ex).is_null();
    let mut removed_cost = BigInt::from(0);
    if get(&q, "action").as_str() == Some("sell") {
        if !has_position
            || get(get(get(s, "positions"), &ex), "side").as_str()
                != get(&q, "side").as_str()
        {
            return err("INVENTORY_WALLET_UNOWNED_SELL");
        }
        let mut pos = get(get(s, "positions"), &ex).clone();
        removed_cost = consume(&mut pos, n)?;
        let cash = amount(get(s, "cash"))? + &value;
        obj_insert(s, "cash", json!(money(&cash)));
        let realized = amount(get(s, "realized"))? + &value - &removed_cost;
        obj_insert(s, "realized", json!(money(&realized)));
        if get(&pos, "quantity").as_i64().unwrap_or(0) == 0 {
            s.as_object_mut()
                .unwrap()
                .get_mut("positions")
                .unwrap()
                .as_object_mut()
                .unwrap()
                .shift_remove(&ex);
        } else {
            s.as_object_mut()
                .unwrap()
                .get_mut("positions")
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(ex.clone(), pos);
        }
    } else {
        let cash = amount(get(s, "cash"))? - &value;
        obj_insert(s, "cash", json!(money(&cash)));
        let mut left = n;
        if has_position
            && get(get(get(s, "positions"), &ex), "side").as_str()
                != get(&q, "side").as_str()
        {
            let pos_q = get(get(get(s, "positions"), &ex), "quantity")
                .as_i64()
                .unwrap_or(0);
            let paired = std::cmp::min(pos_q, left);
            let mut pos = get(get(s, "positions"), &ex).clone();
            let old_cost = consume(&mut pos, paired)?;
            let new_cost = amount(get(&q, "limit"))? * BigInt::from(paired);
            let cash2 = amount(get(s, "cash"))? + &scale() * BigInt::from(paired);
            obj_insert(s, "cash", json!(money(&cash2)));
            let realized = amount(get(s, "realized"))? + &scale() * BigInt::from(paired)
                - &old_cost - &new_cost;
            obj_insert(s, "realized", json!(money(&realized)));
            s.as_object_mut()
                .unwrap()
                .get_mut("redemptions")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "quantity": paired, "oldCost": money(&old_cost), "newCost": money(&new_cost),
                    "at": at.clone(), "exchangeId": ex.clone(), "conditionalModel": true,
                }));
            left -= paired;
            if get(&pos, "quantity").as_i64().unwrap_or(0) == 0 {
                s.as_object_mut()
                    .unwrap()
                    .get_mut("positions")
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .shift_remove(&ex);
            } else {
                s.as_object_mut()
                    .unwrap()
                    .get_mut("positions")
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .insert(ex.clone(), pos);
            }
        }
        if left > 0 {
            let mut x = if has_position {
                get(get(s, "positions"), &ex).clone()
            } else {
                json!({"side": get(&q, "side").clone(), "quantity": 0, "cost": "0.000000", "lots": []})
            };
            let q2 = get(&x, "quantity").as_i64().unwrap_or(0) + left;
            obj_insert(&mut x, "quantity", json!(q2));
            let c = amount(get(&x, "cost"))? + amount(get(&q, "limit"))? * BigInt::from(left);
            obj_insert(&mut x, "cost", json!(money(&c)));
            x.as_object_mut()
                .unwrap()
                .get_mut("lots")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(json!({"quantity": left, "price": get(&q, "limit").clone()}));
            s.as_object_mut()
                .unwrap()
                .get_mut("positions")
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(ex.clone(), x);
        }
    }
    {
        let quotes = s
            .as_object_mut()
            .unwrap()
            .get_mut("quotes")
            .unwrap()
            .as_array_mut()
            .unwrap();
        let rem = get(&quotes[qi], "remaining").as_i64().unwrap_or(0) - n;
        quotes[qi]
            .as_object_mut()
            .unwrap()
            .insert("remaining".into(), json!(rem));
        if rem == 0 {
            quotes[qi]
                .as_object_mut()
                .unwrap()
                .insert("closedAt".into(), at.clone());
        }
    }
    let q2 = get(s, "quotes").as_array().unwrap()[qi].clone();
    s.as_object_mut()
        .unwrap()
        .get_mut("fills")
        .unwrap()
        .as_array_mut()
        .unwrap()
        .push(json!({
            "quoteId": get(&q2, "id").clone(),
            "marketId": get(&q2, "marketId").clone(),
            "exchangeId": get(&q2, "exchangeId").clone(),
            "side": get(&q2, "side").clone(),
            "action": get(&q2, "action").clone(),
            "quantity": n,
            "price": get(&q2, "limit").clone(),
            "removedCost": money(&removed_cost),
            "at": at.clone(),
            "source": source.clone(),
            "actualFill": false,
        }));
    inc(s, "counterfactual_fill");
    Ok(())
}

fn set_quote_field(s: &mut V, q_id: i64, k: &str, x: V) {
    if let Some(q) = s
        .as_object_mut()
        .unwrap()
        .get_mut("quotes")
        .and_then(|v| v.as_array_mut())
        .and_then(|a| a.iter_mut().find(|q| get(q, "id").as_i64() == Some(q_id)))
    {
        q.as_object_mut().unwrap().insert(k.to_string(), x);
    }
}

pub fn advance_inventory_quote_wallet_v1(s: &mut V, event: &V, p: &V) -> Result<(), String> {
    assert_inventory_quote_wallet_v1(s, p)?;
    let at = instant_ms(get(event, "at"))?;
    if at < instant_ms(get(s, "lastAt"))? {
        return err("INVENTORY_WALLET_CLOCK");
    }
    obj_insert(s, "lastAt", get(event, "at").clone());

    if get(event, "kind").as_str() == Some("trades") {
        if get(p, "fillModel").as_str() != Some("strict_tape_through") {
            return Ok(());
        }
        let page = get(event, "page");
        if instant_ms(get(page, "receivedAt"))? > at
            || instant_ms(get(page, "from"))? >= instant_ms(get(page, "to"))?
        {
            return err("INVENTORY_WALLET_PAGE_CLOCK");
        }
        obj_insert(
            s,
            "tradeCoverage",
            {
                let mut tc = get(s, "tradeCoverage").as_object().cloned().unwrap_or_default();
                tc.insert(
                    get(page, "exchangeId").as_str().unwrap_or("").to_string(),
                    json!({
                        "knownAt": get(event, "at").clone(),
                        "to": get(page, "to").clone(),
                        "complete": get(get(page, "coverage"), "complete").as_bool() == Some(true)
                            && get(get(page, "pagination"), "hasMore").as_bool() == Some(false),
                    }),
                );
                V::Object(tc)
            },
        );
        let mut trades = get(page, "trades").as_array().cloned().unwrap_or_default();
        trades.sort_by(|a, b| {
            let na = instant_ns_v(get(a, "createdAt")).unwrap_or(0);
            let nb = instant_ns_v(get(b, "createdAt")).unwrap_or(0);
            na.cmp(&nb).then_with(|| {
                get(a, "key")
                    .as_str()
                    .unwrap_or("")
                    .cmp(get(b, "key").as_str().unwrap_or(""))
            })
        });
        for t in &trades {
            let key = get(t, "key").as_str().unwrap_or("").to_string();
            if !get(get(s, "seenTrades"), &key).is_null() {
                continue;
            }
            let tns = instant_ns_v(get(t, "createdAt"))?;
            let from_ns = instant_ms(get(page, "from"))? as i128 * 1_000_000;
            let to_ns = instant_ms(get(page, "to"))? as i128 * 1_000_000;
            if tns < from_ns || tns >= to_ns {
                return err("INVENTORY_WALLET_TRADE_WINDOW");
            }
            obj_insert(
                s,
                "seenTrades",
                {
                    let mut m = get(s, "seenTrades").as_object().cloned().unwrap_or_default();
                    m.insert(key.clone(), json!(true));
                    V::Object(m)
                },
            );
            let tick_v = tape_tick(t)?;
            let tick_zero = match &tick_v {
                None => true,
                Some(v) => {
                    let ti = get(v, "tick").as_i64().unwrap_or(-1);
                    ti == 0 || ti == 200
                }
            };
            if tick_zero {
                continue;
            }
            let tick_v = tick_v.unwrap();
            let mut available = get(t, "size").as_i64().unwrap_or(-1);
            if available < 1 {
                return err("INVENTORY_WALLET_TRADE_SIZE");
            }
            let candidates: Vec<V> = open(s)
                .into_iter()
                .filter(|q| {
                    get(q, "exchangeId") == get(page, "exchangeId")
                        && get(q, "marketId") == get(page, "marketId")
                        && !get(q, "censored").as_bool().unwrap_or(false)
                })
                .collect();
            for q in &candidates {
                let created = instant_ns_v(get(t, "createdAt"))?;
                let end = std::cmp::min(
                    get(q, "expiresAt").as_i64().unwrap_or(i64::MAX),
                    get(q, "cancelEffectiveAt").as_i64().unwrap_or(i64::MAX),
                );
                let created_ms = created / 1_000_000;
                if created < (get(q, "activationAt").as_i64().unwrap_or(0) as i128 * 1_000_000)
                    || created_ms as i128 >= end as i128
                {
                    continue;
                }
                let limit = if get(q, "side").as_str() == Some("YES") {
                    amount(get(q, "limit"))?
                } else {
                    &scale() - amount(get(q, "limit"))?
                };
                let asks = (get(q, "side").as_str() == Some("YES"))
                    == (get(q, "action").as_str() == Some("buy"));
                let tick_price = amount(get(&tick_v, "price"))?;
                let through = if asks {
                    tick_price <= &limit - &tick()
                } else {
                    tick_price >= &limit + &tick()
                };
                if !through {
                    continue;
                }
                let n = std::cmp::min(available, get(q, "remaining").as_i64().unwrap_or(0));
                if n > 0 {
                    fill(
                        s,
                        get(q, "id").as_i64().unwrap_or(0),
                        n,
                        get(event, "at"),
                        &json!({
                            "tradeKey": get(t, "key").clone(),
                            "tradeId": get(t, "id").clone(),
                            "createdAt": get(t, "createdAt").clone(),
                            "knownAt": get(page, "receivedAt").clone(),
                            "strictTradeThrough": true,
                            "queuePositionProven": false,
                        }),
                    )?;
                    available -= n;
                }
                if available == 0 {
                    break;
                }
            }
        }
        if get(get(page, "coverage"), "complete").as_bool() == Some(true)
            && get(get(page, "pagination"), "hasMore").as_bool() == Some(false)
        {
            let candidates: Vec<V> = open(s)
                .into_iter()
                .filter(|q| {
                    get(q, "exchangeId") == get(page, "exchangeId")
                        && get(q, "marketId") == get(page, "marketId")
                        && !get(q, "censored").as_bool().unwrap_or(false)
                })
                .collect();
            for q in &candidates {
                let covered = if get(q, "coveredThrough").is_null() {
                    get(q, "activationAt").as_i64().unwrap_or(0)
                } else {
                    get(q, "coveredThrough").as_i64().unwrap_or(0)
                };
                if instant_ms(get(page, "from"))? <= covered
                    && instant_ms(get(page, "to"))? >= covered
                {
                    set_quote_field(
                        s,
                        get(q, "id").as_i64().unwrap_or(0),
                        "coveredThrough",
                        json!(std::cmp::max(covered, instant_ms(get(page, "to"))?)),
                    );
                }
                let covered2 = {
                    let qq = get(s, "quotes")
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|x| get(x, "id").as_i64() == get(q, "id").as_i64())
                        .cloned()
                        .unwrap_or(V::Null);
                    if get(&qq, "coveredThrough").is_null() {
                        get(&qq, "activationAt").as_i64().unwrap_or(0)
                    } else {
                        get(&qq, "coveredThrough").as_i64().unwrap_or(0)
                    }
                };
                let end = std::cmp::min(
                    get(q, "expiresAt").as_i64().unwrap_or(i64::MAX),
                    get(q, "cancelEffectiveAt").as_i64().unwrap_or(i64::MAX),
                );
                if covered2 >= end {
                    set_quote_field(
                        s,
                        get(q, "id").as_i64().unwrap_or(0),
                        "closedAt",
                        get(event, "at").clone(),
                    );
                    inc(s, "tape_expiry_resolved");
                }
            }
        }
        assert_inventory_quote_wallet_v1(s, p)?;
        return Ok(());
    }

    if get(event, "kind").as_str() != Some("book") {
        let market = get(event, "marketId");
        let candidates: Vec<V> = open(s)
            .into_iter()
            .filter(|q| market.is_null() || get(q, "marketId") == market)
            .collect();
        let strict = get(p, "fillModel").as_str() == Some("strict_tape_through");
        for q in &candidates {
            let qid = get(q, "id").as_i64().unwrap_or(0);
            if strict {
                if get(q, "cancelEffectiveAt").is_null() {
                    set_quote_field(s, qid, "cancelEffectiveAt", json!(at + get(p, "latencyMs").as_i64().unwrap_or(0)));
                    inc(s, "model_cancel_pending");
                }
            } else if !get(q, "censored").as_bool().unwrap_or(false) {
                set_quote_field(s, qid, "censored", json!(true));
                inc(s, "censored_quote");
            }
        }
        return Ok(());
    }

    let b = get(event, "book");
    let ex = get(b, "exchangeId").as_str().unwrap_or("").to_string();
    let t = instant_ms(get(b, "requestStartedAt"))?;
    let old = get(get(s, "history"), &ex).clone();
    if t <= instant_ms(get(p, "cutoff"))? {
        return Ok(());
    }
    if !old.is_null()
        && t - get(&old, "requestAt").as_i64().unwrap_or(0)
            > get(p, "maximumGapMs").as_i64().unwrap_or(0)
        && get(p, "fillModel").as_str() != Some("strict_tape_through")
    {
        let candidates: Vec<V> = open(s)
            .into_iter()
            .filter(|q| get(q, "exchangeId").as_str() == Some(ex.as_str()))
            .collect();
        for q in &candidates {
            if !get(q, "censored").as_bool().unwrap_or(false) {
                set_quote_field(s, get(q, "id").as_i64().unwrap_or(0), "censored", json!(true));
                inc(s, "censored_quote");
            }
        }
    }

    let dfb = get(p, "depthFractionBps").as_i64().unwrap_or(0);
    let ask_budget: Vec<(BigInt, i64)> = get(b, "asks")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|l| {
            Ok((
                amount(get(l, "price"))?,
                get(l, "quantity").as_i64().unwrap_or(0) * dfb / 10000,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let bid_budget: Vec<(BigInt, i64)> = get(b, "bids")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|l| {
            Ok((
                amount(get(l, "price"))?,
                get(l, "quantity").as_i64().unwrap_or(0) * dfb / 10000,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;

    {
        let strict = get(p, "fillModel").as_str() == Some("strict_tape_through");
        let candidates: Vec<V> = open(s)
            .into_iter()
            .filter(|q| get(q, "exchangeId").as_str() == Some(ex.as_str()))
            .collect();
        let mut budgets = Map::new();
        budgets.insert("ask".to_string(), json!(ask_budget.iter().map(|(pr, r)| json!({"price": money(pr), "remaining": r})).collect::<Vec<_>>()));
        budgets.insert("bid".to_string(), json!(bid_budget.iter().map(|(pr, r)| json!({"price": money(pr), "remaining": r})).collect::<Vec<_>>()));
        let mut ask_left: Vec<(BigInt, i64)> = ask_budget.clone();
        let mut bid_left: Vec<(BigInt, i64)> = bid_budget.clone();
        let _ = &mut budgets;
        for q in &candidates {
            if get(q, "censored").as_bool().unwrap_or(false) {
                continue;
            }
            let qid = get(q, "id").as_i64().unwrap_or(0);
            if t >= get(q, "expiresAt").as_i64().unwrap_or(i64::MAX) {
                if !strict {
                    set_quote_field(s, qid, "closedAt", get(event, "at").clone());
                    inc(s, "expired_quote");
                }
                continue;
            }
            if strict
                || t < get(q, "activationAt").as_i64().unwrap_or(0)
                || get(q, "decisionVersion") == get(b, "versionKey")
            {
                continue;
            }
            let asks_side = (get(q, "side").as_str() == Some("YES"))
                == (get(q, "action").as_str() == Some("buy"));
            let yes_limit = if get(q, "side").as_str() == Some("YES") {
                amount(get(q, "limit"))?
            } else {
                &scale() - amount(get(q, "limit"))?
            };
            let levels = if asks_side { &mut ask_left } else { &mut bid_left };
            for l in levels.iter_mut() {
                let through = if asks_side {
                    l.0 <= &yes_limit - &tick()
                } else {
                    l.0 >= &yes_limit + &tick()
                };
                if !through {
                    continue;
                }
                let n = std::cmp::min(get(q, "remaining").as_i64().unwrap_or(0), l.1);
                if n > 0 {
                    fill(
                        s,
                        qid,
                        n,
                        get(event, "at"),
                        &json!({
                            "sequence": get(event, "sequence").clone(),
                            "hash": get(event, "hash").clone(),
                            "requestStartedAt": get(b, "requestStartedAt").clone(),
                            "version": get(b, "versionKey").clone(),
                            "strictBookThrough": true,
                            "queuePositionProven": false,
                        }),
                    )?;
                    l.1 -= n;
                }
                let rem_now = {
                    let qq = get(s, "quotes")
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|x| get(x, "id").as_i64() == Some(qid))
                        .cloned()
                        .unwrap_or(V::Null);
                    get(&qq, "remaining").as_i64().unwrap_or(0)
                };
                if rem_now == 0 {
                    break;
                }
            }
        }
    }

    // mid = (Number(amount(bids[0].price)) + Number(amount(asks[0].price))) / 2e6
    let first_price = |side: &str| -> f64 {
        get(b, side)
            .as_array()
            .and_then(|a| a.first())
            .map(|l| get(l, "price").clone())
            .and_then(|v| amount(&v).ok())
            .and_then(|x| x.to_string().parse::<f64>().ok())
            .unwrap_or(f64::NAN)
    };
    let mid = (first_price("bids") + first_price("asks")) / 2e6;
    let dt = if old.is_null() {
        0.0
    } else {
        (t - get(&old, "requestAt").as_i64().unwrap_or(0)) as f64 / 1000.0
    };
    let gap_ok = dt * 1000.0 <= get(p, "maximumGapMs").as_f64().unwrap_or(0.0);
    let variance = if !old.is_null() && dt > 0.0 && gap_ok {
        let e = (-dt / 600.0).exp();
        e * get(&old, "variance").as_f64().unwrap_or(0.0)
            + (1.0 - e) * (mid - get(&old, "mid").as_f64().unwrap_or(0.0)).powi(2) / dt
    } else {
        0.0
    };
    {
        let mut h = get(s, "history").as_object().cloned().unwrap_or_default();
        let count = if !old.is_null() && gap_ok {
            get(&old, "count").as_i64().unwrap_or(0) + 1
        } else {
            1
        };
        let start_at = if !old.is_null() && gap_ok {
            get(&old, "startAt").clone()
        } else {
            json!(t)
        };
        h.insert(
            ex.clone(),
            json!({"mid": mid, "variance": variance, "requestAt": t, "count": count, "startAt": start_at}),
        );
        obj_insert(s, "history", V::Object(h));
    }
    let h = get(get(s, "history"), &ex).clone();
    let any_open = open(s)
        .iter()
        .any(|q| get(q, "exchangeId").as_str() == Some(ex.as_str()));
    if any_open
        || get(&h, "count").as_i64().unwrap_or(0) < get(p, "minimumSamples").as_i64().unwrap_or(0)
        || t - get(&h, "startAt").as_i64().unwrap_or(0)
            < get(p, "minimumHistoryMs").as_i64().unwrap_or(0)
    {
        return Ok(());
    }
    if get(p, "fillModel").as_str() == Some("strict_tape_through")
        && get(p, "requirePriorTape").as_bool().unwrap_or(false)
    {
        let c = get(get(s, "tradeCoverage"), &ex);
        if !(get(c, "complete").as_bool() == Some(true)
            && instant_ms(get(c, "knownAt")).map(|k| k <= at).unwrap_or(false)
            && instant_ms(get(c, "knownAt")).map(|k| at - k <= 120000).unwrap_or(false)
            && instant_ms(get(c, "to")).map(|k| at - k <= 120000).unwrap_or(false))
        {
            inc(s, "missing_causal_tape_coverage");
            return Ok(());
        }
    }
    let mut active: Vec<String> = get(s, "positions")
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    for q in open(s) {
        let e = get(&q, "exchangeId").as_str().unwrap_or("").to_string();
        if !active.contains(&e) {
            active.push(e);
        }
    }
    if !active.contains(&ex) && active.len() as i64 >= get(p, "maximumMarkets").as_i64().unwrap_or(0) {
        return Ok(());
    }
    let x = get(get(s, "positions"), &ex);
    let inventory = json!({
        "yes": if get(x, "side").as_str() == Some("YES") { get(x, "quantity").clone() } else { json!(0) },
        "no": if get(x, "side").as_str() == Some("NO") { get(x, "quantity").clone() } else { json!(0) },
    });
    let available = amount(get(s, "cash"))? - reserved(s)?;
    let capacity = amount(get(p, "maximumCommitted"))? - basis(s)? - reserved(s)?;
    let budget = if available < capacity { available } else { capacity };
    if budget < BigInt::from(0) {
        return err("INVENTORY_WALLET_BUDGET");
    }
    let proposal = inventory_aware_quotes_v1(&json!({
        "book": b.clone(),
        "inventory": inventory,
        "cashAvailable": money(&budget),
        "maximumOrderCost": get(p, "maximumOrderCost").clone(),
        "maximumShares": get(p, "quantity").clone(),
        "inventoryScale": get(p, "quantity").clone(),
        "variancePerSecond": variance,
        "riskAversion": get(p, "riskAversion").clone(),
        "horizonSeconds": get(p, "horizonSeconds").clone(),
        "now": get(event, "at").clone(),
    }))?;
    let mut new_quotes = get(&proposal, "sells")
        .as_array()
        .cloned()
        .unwrap_or_default();
    new_quotes.extend(get(&proposal, "buys").as_array().cloned().unwrap_or_default());
    for quote in new_quotes {
        let next_id = get(s, "nextId").as_i64().unwrap_or(1);
        obj_insert(s, "nextId", json!(next_id + 1));
        let mut qq = quote.as_object().cloned().unwrap_or_default();
        qq.insert("id".into(), json!(next_id));
        qq.insert("marketId".into(), get(b, "marketId").clone());
        qq.insert("exchangeId".into(), json!(ex.clone()));
        qq.insert("remaining".into(), get(&quote, "quantity").clone());
        qq.insert("createdAt".into(), get(event, "at").clone());
        qq.insert(
            "activationAt".into(),
            json!(at + get(p, "latencyMs").as_i64().unwrap_or(0)),
        );
        qq.insert(
            "expiresAt".into(),
            json!(at + get(p, "lifetimeMs").as_i64().unwrap_or(0)),
        );
        qq.insert("decisionVersion".into(), get(b, "versionKey").clone());
        qq.insert("decisionHash".into(), get(&proposal, "proposalHash").clone());
        qq.insert("censored".into(), json!(false));
        qq.insert("closedAt".into(), V::Null);
        s.as_object_mut()
            .unwrap()
            .get_mut("quotes")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(V::Object(qq));
        inc(s, "quote_created");
    }
    assert_inventory_quote_wallet_v1(s, p)?;
    Ok(())
}

pub fn inventory_quote_wallet_report_v1(s: &V, p: &V) -> Result<V, String> {
    assert_inventory_quote_wallet_v1(s, p)?;
    Ok(json!({
        "cash": get(s, "cash").clone(),
        "ownedPrincipal": money(&basis(s)?),
        "reservedCash": money(&reserved(s)?),
        "realized": get(s, "realized").clone(),
        "positions": get(s, "positions").clone(),
        "fills": get(s, "fills").as_array().map(|a| a.len() as i64).unwrap_or(0),
        "redemptions": get(s, "redemptions").as_array().map(|a| a.len() as i64).unwrap_or(0),
        "unresolvedQuotes": open(s).len() as i64,
        "censoredQuotes": open(s).iter().filter(|q| get(q, "censored").as_bool().unwrap_or(false)).count() as i64,
        "counts": get(s, "counts").clone(),
        "executionAuthority": false,
        "profitabilityEstablished": false,
    }))
}
