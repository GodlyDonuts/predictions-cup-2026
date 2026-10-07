// live-paper/generic-financial-book-focus-v1.mjs — read allocation only. It
// cannot adopt holdings, authorize trades or make a stale book executable.
// Inventory comes from the current attributed controller.
//
// Port notes:
// - `undefined` is modeled as `V::Null`, so a literal `null` marketId /
//   exchangeId collides with a missing one — a pathological input where JS
//   diverges (`null !== undefined`); both are malformed for this domain.
// - JS own-enumerable order ([[OwnPropertyKeys]]: integer-index keys
//   numerically first, then other string keys in insertion order) is applied
//   to `Object.entries(inventory)` and every spread-built object, because
//   entry order feeds `ids` order and the emitted JSON layout.
// - Property access on null keeps the JS TypeError message text, matching
//   the crate convention.
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy};
use serde_json::{json, Map, Value as V};
use std::cell::RefCell;
use std::cmp::Ordering;

// v[k] — TypeError on null; primitives/arrays yield undefined.
fn prop<'a>(v: &'a V, k: &str) -> Result<Option<&'a V>, String> {
    match v {
        V::Object(m) => Ok(m.get(k)),
        V::Null => Err(format!("Cannot read properties of null (reading '{}')", k)),
        _ => Ok(None),
    }
}
fn prop_get(v: &V, k: &str) -> Result<V, String> {
    Ok(prop(v, k)?.cloned().unwrap_or(V::Null))
}

// JS `===` / `!==`: strict equality — NaN !== NaN, -0 === +0, different
// primitive types are never equal, and arrays/objects only match themselves
// (never equal as JSON values).
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::String(x), V::String(y)) => x == y,
        (V::Number(x), V::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        },
        _ => false,
    }
}
// JS Map key lookup / `new Set` dedup: SameValueZero — NaN equals NaN.
fn same_value_zero(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Number(x), V::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => x == y || (x.is_nan() && y.is_nan()),
            _ => false,
        },
        _ => strict_eq(a, b),
    }
}

// ToPropertyKey/String() for the `lastAttempts[v]` lookups this module does.
fn to_key(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(_) => js_stringify(v),
        V::Bool(b) => b.to_string(),
        V::Null => "null".into(),
        V::Array(a) => a
            .iter()
            .map(|x| match x {
                V::Null => String::new(),
                _ => to_key(x),
            })
            .collect::<Vec<_>>()
            .join(","),
        V::Object(_) => "[object Object]".into(),
    }
}
// ToNumber for the arithmetic coercions (`la[a]-la[b]`, `Number(a)`).
fn js_num_str(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}
fn js_num(v: &V) -> f64 {
    match v {
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::Bool(b) => f64::from(*b as u8),
        V::Null => 0.0, // Number(null); an actual `undefined` diverges — see notes
        // ToNumber(ToString(v)): [] → "" → 0, [x] → String(x), {} → NaN.
        V::Array(_) | V::Object(_) => js_num_str(&to_key(v)),
        V::String(s) => js_num_str(s),
    }
}

// JS Map with SameValueZero keys, preserving insertion order for iteration.
fn map_get<'a>(m: &'a [(V, V)], key: &V) -> Option<&'a V> {
    m.iter().find(|(k, _)| same_value_zero(k, key)).map(|(_, v)| v)
}
fn map_set(m: &mut Vec<(V, V)>, key: V, value: V) {
    if let Some(e) = m.iter_mut().find(|(k, _)| same_value_zero(k, &key)) {
        e.1 = value;
    } else {
        m.push((key, value));
    }
}

fn index_key(k: &str) -> Option<u32> {
    let v = k.parse::<u32>().ok()?;
    if v < u32::MAX && v.to_string() == k {
        Some(v)
    } else {
        None
    }
}
// Object.entries(v) over every JSON shape: objects enumerate in JS key order
// (integer-index keys numerically first, then insertion order), arrays as
// indices, strings as UTF-16 code-unit keys, primitives have none.
fn object_entries(v: &V) -> Vec<(String, V)> {
    match v {
        V::Object(o) => {
            let mut a: Vec<(String, V)> =
                o.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            a.sort_by(|(a, _), (b, _)| match (index_key(a), index_key(b)) {
                (Some(a), Some(b)) => a.cmp(&b),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                _ => Ordering::Equal,
            });
            a
        }
        V::Array(a) => a
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v.clone()))
            .collect(),
        V::String(s) => s
            .encode_utf16()
            .enumerate()
            .map(|(i, c)| (i.to_string(), json!(String::from_utf16_lossy(&[c]))))
            .collect(),
        _ => vec![],
    }
}
// {...v}: object spread copies own enumerable string-keyed properties.
fn spread_object(v: &V) -> Map<String, V> {
    let mut m = Map::new();
    for (k, v) in object_entries(v) {
        m.insert(k, v);
    }
    m
}
// {...member, path:'supplementalDepth', reason:'attributed_financial_risk'}
fn supplemental(member: &V) -> V {
    let mut m = spread_object(member);
    m.insert("path".into(), json!("supplementalDepth"));
    m.insert("reason".into(), json!("attributed_financial_risk"));
    V::Object(m)
}

/// genericFinancialMarketIds(status, universe): the attributed market ids the
/// controller reports — positive-quantity inventory entries in inventory
/// order, then the in-flight intent market — deduplicated.
pub fn generic_financial_market_ids(status: &V, universe: &V) -> Result<Vec<V>, String> {
    // const c = status?.controller; — `?.` tolerates missing/null.
    let c = status.get("controller");
    // if (c?.ownerId !== 'Codex' || c.tournamentId !== universe.tournamentId
    //     || !c.inventory) return [];
    if c.and_then(|c| c.get("ownerId")).and_then(V::as_str) != Some("Codex") {
        return Ok(vec![]);
    }
    let c = c.unwrap(); // ownerId was read off a live object
    let tournament_id = prop(universe, "tournamentId")?.cloned().unwrap_or(V::Null);
    if !strict_eq(&c.get("tournamentId").cloned().unwrap_or(V::Null), &tournament_id) {
        return Ok(vec![]);
    }
    if !js_truthy(c.get("inventory")) {
        return Ok(vec![]);
    }
    let inventory = c.get("inventory").unwrap();
    // const members = new Map(universe.exchanges.map(x => [x.exchangeId, x.marketId]))
    let exchanges = match prop(universe, "exchanges")? {
        Some(V::Array(a)) => a,
        Some(V::Null) => {
            return Err("Cannot read properties of null (reading 'map')".into())
        }
        None => {
            return Err("Cannot read properties of undefined (reading 'map')".into())
        }
        Some(_) => return Err("universe.exchanges.map is not a function".into()),
    };
    let mut members: Vec<(V, V)> = Vec::new();
    for x in exchanges {
        let k = prop_get(x, "exchangeId")?;
        let v = prop_get(x, "marketId")?;
        map_set(&mut members, k, v);
    }
    let mut ids: Vec<V> = Vec::new();
    for (exchange_id, p) in object_entries(inventory) {
        let quantity = prop_get(&p, "quantity")?;
        // !Number.isSafeInteger(p.quantity) || p.quantity < 0
        let qty = match js_int(&quantity) {
            Some(q) if q >= 0 => q,
            _ => return Err("GENERIC_FINANCIAL_FOCUS_SCOPE".into()),
        };
        // members.get(exchangeId) !== p.marketId — the object-entry key is
        // always a string; a number-keyed member can never equal it, matching
        // SameValueZero on the JS Map.
        let member = map_get(&members, &V::String(exchange_id))
            .cloned()
            .unwrap_or(V::Null);
        let market_id = prop_get(&p, "marketId")?;
        // /^[a-f0-9]{64}$/.test(p.proofHash ?? '') — RegExp.test coerces to
        // string for non-string values.
        let proof_hash = match prop(&p, "proofHash")? {
            None | Some(V::Null) => String::new(),
            Some(v) => to_key(v),
        };
        let proof_ok = proof_hash.len() == 64
            && proof_hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !strict_eq(&member, &market_id) || !proof_ok {
            return Err("GENERIC_FINANCIAL_FOCUS_SCOPE".into());
        }
        if qty > 0 {
            ids.push(market_id);
        }
    }
    if js_truthy(c.get("intent")) {
        let intent = c.get("intent").unwrap();
        let intent_exchange = prop_get(intent, "exchangeId")?;
        let intent_market = prop_get(intent, "marketId")?;
        let member = map_get(&members, &intent_exchange)
            .cloned()
            .unwrap_or(V::Null);
        if !strict_eq(&member, &intent_market) {
            return Err("GENERIC_FINANCIAL_FOCUS_INTENT_SCOPE".into());
        }
        ids.push(intent_market);
    }
    // [...new Set(ids)] — SameValueZero dedup, first occurrence wins.
    let mut out: Vec<V> = Vec::new();
    for id in ids {
        if !out.iter().any(|x| same_value_zero(x, &id)) {
            out.push(id);
        }
    }
    Ok(out)
}

/// focusGenericFinancialBooks(plan, status, universe, lastAttempts={}):
/// reprioritize the four-book depth-read budget toward the attributed
/// generic-financial markets; the caller reads
/// `out.genericFinancialFocus` back off the returned plan
/// (`runtime.genericFinancialReadFocus = planned.genericFinancialFocus ?? null`).
pub fn focus_generic_financial_books(
    plan: &V,
    status: &V,
    universe: &V,
    last_attempts: Option<&V>,
) -> Result<V, String> {
    let ids = generic_financial_market_ids(status, universe)?;
    if ids.is_empty() {
        return Ok(plan.clone());
    }
    // const budget = plan.reads.length
    let reads = prop(plan, "reads")?;
    let budget = match reads {
        None => {
            return Err("Cannot read properties of undefined (reading 'length')".into())
        }
        Some(V::Null) => {
            return Err("Cannot read properties of null (reading 'length')".into())
        }
        Some(V::Array(a)) => a.len() as f64,
        Some(V::String(s)) => s.encode_utf16().count() as f64,
        // (value).length → undefined → NaN budget; NaN comparisons are false,
        // so JS reaches `plan.reads.map` and throws the TypeError below.
        Some(_) => f64::NAN,
    };
    if budget < 1.0 || budget > 4.0 {
        return Err("GENERIC_FINANCIAL_FOCUS_BUDGET".into());
    }
    // const priority = [...ids].sort((a,b) =>
    //   (lastAttempts[a]??0)-(lastAttempts[b]??0) || Number(a)-Number(b))
    let sort_err = RefCell::new(None::<String>);
    let mut priority = ids.clone();
    priority.sort_by(|a, b| {
        if sort_err.borrow().is_some() {
            return Ordering::Equal;
        }
        match compare_market_ids(a, b, last_attempts) {
            Ok(o) => o,
            Err(e) => {
                *sort_err.borrow_mut() = Some(e);
                Ordering::Equal
            }
        }
    });
    if let Some(e) = sort_err.into_inner() {
        return Err(e);
    }
    // const byMarket = new Map(plan.reads.map(r => [r.marketId, r])) and the
    // for-head `plan.reads.map(r => r.marketId)` — a non-array reads throws
    // `plan.reads.map is not a function` here (after the sort).
    let reads_arr: &Vec<V> = match reads {
        Some(V::Array(a)) => a,
        Some(V::Null) | None => unreachable!("reads.length guard above"),
        Some(_) => return Err("plan.reads.map is not a function".into()),
    };
    let mut by_market: Vec<(V, V)> = Vec::new();
    let mut read_ids: Vec<V> = Vec::new();
    for r in reads_arr {
        let market_id = prop_get(r, "marketId")?;
        map_set(&mut by_market, market_id.clone(), r.clone());
        read_ids.push(market_id);
    }
    let mut selected: Vec<V> = Vec::new();
    for market_id in priority.iter().chain(read_ids.iter()) {
        // selected.some(r => r.marketId === marketId) — evaluated first, so
        // its TypeError precedes the `selected.length === budget` check.
        let mut sel_err = None::<String>;
        let dup = selected.iter().any(|r| match prop(r, "marketId") {
            Ok(v) => strict_eq(&v.cloned().unwrap_or(V::Null), market_id),
            Err(e) => {
                sel_err = Some(e);
                true
            }
        });
        if let Some(e) = sel_err {
            return Err(e);
        }
        if dup || selected.len() as f64 == budget {
            continue;
        }
        // universe.exchanges.find(x => x.marketId === marketId) — evaluated
        // per iteration, after the dup/budget guards.
        let member = {
            let list = match prop(universe, "exchanges")? {
                Some(V::Array(a)) => a,
                Some(V::Null) => {
                    return Err("Cannot read properties of null (reading 'find')".into())
                }
                None => {
                    return Err("Cannot read properties of undefined (reading 'find')".into())
                }
                Some(_) => return Err("universe.exchanges.find is not a function".into()),
            };
            let mut found = None;
            let mut find_err = None::<String>;
            for x in list {
                match prop(x, "marketId") {
                    Ok(v) => {
                        if strict_eq(&v.cloned().unwrap_or(V::Null), market_id) {
                            found = Some(x);
                            break;
                        }
                    }
                    Err(e) => {
                        find_err = Some(e);
                        break;
                    }
                }
            }
            if let Some(e) = find_err {
                return Err(e);
            }
            found
        };
        let member = member.ok_or("GENERIC_FINANCIAL_FOCUS_MARKET")?;
        // selected.push(byMarket.get(marketId) ?? {...member, path,
        // reason}) — `??` skips the supplemental for any present non-null
        // member.
        let entry = match map_get(&by_market, market_id) {
            Some(V::Null) | None => supplemental(member),
            Some(r) => r.clone(),
        };
        selected.push(entry);
    }
    // return {...plan, reads: selected, genericFinancialFocus: {...}}
    let mut out = spread_object(plan);
    out.insert("reads".into(), V::Array(selected));
    let mut focus = Map::new();
    focus.insert("marketIds".into(), V::Array(ids));
    focus.insert("budget".into(), json!(budget as i64));
    focus.insert("executionAuthority".into(), V::Bool(false));
    out.insert("genericFinancialFocus".into(), V::Object(focus));
    Ok(V::Object(out))
}

// `(lastAttempts[a] ?? 0) - (lastAttempts[b] ?? 0) || Number(a) - Number(b)`
fn attempt_number(last_attempts: Option<&V>, key: &str) -> Result<f64, String> {
    match last_attempts {
        Some(V::Null) => {
            Err(format!("Cannot read properties of null (reading '{}')", key))
        }
        Some(V::Object(o)) => Ok(match o.get(key) {
            None | Some(V::Null) => 0.0, // `?? 0` on nullish
            Some(v) => js_num(v),
        }),
        Some(_) | None => Ok(0.0), // (primitive)[key] → undefined ?? 0
    }
}
fn compare_market_ids(a: &V, b: &V, last_attempts: Option<&V>) -> Result<Ordering, String> {
    let d1 =
        attempt_number(last_attempts, &to_key(a))? - attempt_number(last_attempts, &to_key(b))?;
    // `||` picks the first truthy operand: d2 when d1 is 0 or NaN.
    let v = if d1 != 0.0 && !d1.is_nan() {
        d1
    } else {
        js_num(a) - js_num(b)
    };
    // JS treats a NaN comparator result as +0.
    Ok(if v.is_nan() || v == 0.0 {
        Ordering::Equal
    } else if v < 0.0 {
        Ordering::Less
    } else {
        Ordering::Greater
    })
}
