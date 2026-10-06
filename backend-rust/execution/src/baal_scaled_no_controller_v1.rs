// execution/baal-scaled-no-controller-v1.mjs — journaled scaled-NO order
// controller: durable intent journal + serialized dispatch fences +
// reservation ledger, one active order at a time.
//
// Port notes:
// - JS WeakMap keys (issued) and the in-flight spec become guard-pointer keys
//   (usize), matching backend-rust/execution/src/paired_rest_controller.rs.
// - async dispatch -> synchronous `WireDispatch`; a captured wire receipt on a
//   dispatch failure arrives via WireDispatchError.wire_receipt.
// - `sync(fn,...)` (the SYNC_LOCAL_CALLBACK check) is trivially satisfied:
//   every supplied callback is a synchronous Fn returning Result.
// - `identity()` resolves the repo root the same way the JS module does — the
//   directory containing the module, one level up. The crate lives at
//   <repo>/backend-rust/execution so the root is two parents up.
use crate::baal_scaled_entry_signal_v1::plan_baal_scaled_entry;
use crate::core::{canonical_payload, payload_hash};
use crate::frozen_paper_graph::collect_frozen_paper_files;
use crate::gates::normalize_official_fill_tick;
use crate::model::normalize_book;
use crate::no_rest_multishare_reconciliation_v1::verify_scoped_rest_no_multishare;
use crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::receipts::parse_number_lexemes;
use crate::transport::{Guard, OrderTransport, OrderTransportError, OrderTransportOptions};
use baal_service_runtime::contract::{instant_ns, ms_to_iso};
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use baal_service_runtime::ledger::acquire_owner_lease;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use uuid::Uuid;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("BAAL_SCALED_{code}"))
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
fn iso(n: f64) -> String {
    ms_to_iso(n)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn amount_v(v: &V) -> Result<i128, String> {
    amount(v.as_str().unwrap_or(""))
}
fn same(a: &V, b: &V) -> bool {
    match (payload_hash(a), payload_hash(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}
fn sha_bytes(b: &[u8]) -> String {
    sha256(b)
}
fn sorted(a: &[V]) -> Vec<V> {
    let mut rows = a.to_vec();
    rows.sort_by_key(|o| js_int(get(o, "id")).unwrap_or(0));
    rows
}
fn path_key(path: &[V]) -> String {
    js_stringify(&V::Array(path.to_vec()))
}
// String(v) — JS primitive-to-string coercion.
fn js_string_repr(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(_) | V::Bool(_) => js_stringify(v),
        V::Null => "null".to_string(),
        _ => "undefined".to_string(),
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(Path::new(env!("CARGO_MANIFEST_DIR")))
        .to_path_buf()
}
fn identity() -> Result<V, String> {
    let files = collect_frozen_paper_files(
        &workspace_root(),
        &["execution/baal-scaled-no-controller-v1.mjs".to_string()],
    )?;
    serde_json::to_value(files).map_err(|e| e.to_string())
}
/// baalScaledNoControllerIdentity() — the frozen-source manifest.
pub fn baal_scaled_no_controller_identity() -> Result<V, String> {
    identity()
}

fn active(s: &V) -> Option<V> {
    get(s, "orders")
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|o| get(o, "proof").is_null() && get(o, "phase").as_str() != Some("abandoned"))
        })
        .cloned()
}
// ownAccount(a, ex)
fn own_account(a: &V, ex: &str) -> V {
    let scoped = get(a, "scopedOrdersPagesByExchange")
        .get(ex)
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or_else(|| get(a, "scopedOrdersPages").clone());
    json!({
        "cash": get(a, "cash"),
        "positions": get(a, "positions"),
        "openOrdersPages": get(a, "openOrdersPages"),
        "scopedOrdersPages": scoped,
    })
}
// raw(r, method, endpoint) — the raw-receipt shape kept on orders.
fn raw(r: &V, method: &str, endpoint: &str) -> Result<V, String> {
    need(
        get(r, "method").as_str() == Some(method)
            && get(r, "endpoint").as_str() == Some(endpoint)
            && get(r, "status").is_i64()
            && get(r, "bodyText").is_string()
            && get(r, "bodyText").as_str().unwrap_or("").len() <= 8 * 1024 * 1024
            && get(r, "sourceBodyHash").as_str()
                == Some(sha256(get(r, "bodyText").as_str().unwrap_or("").as_bytes()).as_str())
            && get(r, "fetchInvocationId").is_string(),
        "RAW_RECEIPT",
    )?;
    need(
        instant_v(get(r, "requestStartedAt"))? <= instant_v(get(r, "receivedAt"))?,
        "RAW_CLOCK",
    )?;
    let mut out = json!({
        "method": method,
        "endpoint": endpoint,
        "status": get(r, "status"),
        "bodyText": get(r, "bodyText"),
        "requestStartedAt": get(r, "requestStartedAt"),
        "receivedAt": get(r, "receivedAt"),
        "sourceBodyHash": get(r, "sourceBodyHash"),
        "fetchInvocationId": get(r, "fetchInvocationId"),
    });
    if let Some(h) = r.get("requestBodyHash") {
        if !h.is_null() {
            out.as_object_mut()
                .map(|m| m.insert("requestBodyHash".into(), h.clone()));
        }
    }
    Ok(out)
}

// configuration(scope, allocation, activation, code)
fn configuration(scope: &V, allocation: &V, activation: &V, code: &V) -> Result<(), String> {
    need(
        get(scope, "ownerId").as_str() == Some("Codex")
            && get(scope, "runId").as_str().is_some_and(|s| !s.is_empty())
            && get(scope, "exchanges").as_array().is_some_and(|a| !a.is_empty())
            && get(scope, "exchanges")
                .as_array()
                .map(|a| {
                    let ids: std::collections::HashSet<String> =
                        a.iter().map(|x| js_stringify(get(x, "exchangeId"))).collect();
                    ids.len() == a.len()
                })
                .unwrap_or(false),
        "SCOPE",
    )?;
    need(
        get(allocation, "schemaVersion") == &json!(1)
            && get(allocation, "ownerId") == get(scope, "ownerId")
            && get(allocation, "runId") == get(scope, "runId")
            && get(allocation, "tournamentId") == get(scope, "tournamentId")
            && get(allocation, "capital").as_str() == Some("1000.000000")
            && hex(get(allocation, "humanAllocationSourceHash")),
        "ALLOCATION",
    )?;
    for (k, max) in [
        ("maximumOrderCost", 25_000_000i128),
        ("maximumMarketCost", 100_000_000i128),
        ("maximumEventCost", 200_000_000i128),
    ] {
        let v = allocation
            .get(k)
            .filter(|x| !x.is_null())
            .cloned()
            .unwrap_or(json!(money(max)));
        let x = amount(v.as_str().unwrap_or(""))?;
        need(x > 0 && x <= max, "ALLOCATION_CEILING")?;
    }
    let shares = allocation
        .get("maximumSharesPerOrder")
        .filter(|x| !x.is_null())
        .cloned()
        .unwrap_or(json!(100));
    need(
        js_int(&shares).is_some_and(|n| n > 0 && n <= 100),
        "SHARE_CEILING",
    )?;
    let markets = allocation
        .get("maximumActiveMarkets")
        .filter(|x| !x.is_null())
        .cloned()
        .unwrap_or(json!(20));
    need(
        js_int(&markets).is_some_and(|n| n > 0 && n <= 20),
        "ACTIVE_MARKET_CEILING",
    )?;
    need(
        get(activation, "schemaVersion") == &json!(1)
            && get(activation, "mode").as_str() == Some("baal_scaled_no_production_v1")
            && ["ownerId", "runId", "tournamentId"]
                .iter()
                .all(|k| get(activation, k) == get(scope, k))
            && get(activation, "allocationHash").as_str() == Some(payload_hash(allocation)?.as_str())
            && get(activation, "scopeHash").as_str() == Some(payload_hash(scope)?.as_str())
            && get(activation, "codeIdentityHash").as_str() == Some(payload_hash(code)?.as_str())
            && hex(get(activation, "authorityHash"))
            && hex(get(activation, "budgetInstalledHash")),
        "ACTIVATION_BINDING",
    )?;
    need(
        instant_v(get(activation, "entryExpiresAt"))? > instant_v(get(activation, "startedAt"))?
            && instant_v(get(activation, "entryExpiresAt"))? - instant_v(get(activation, "startedAt"))? <= 3_600_000
            && instant_v(get(allocation, "recordedAt"))? <= instant_v(get(activation, "startedAt"))?,
        "ACTIVATION_WINDOW",
    )?;
    Ok(())
}

// reservation(s) — the full-nominal projection the budget slot installs.
fn reservation(s: &V) -> Result<V, String> {
    let mut owned: Vec<V> = Vec::new();
    let mut reservations: Vec<V> = Vec::new();
    let run_id = get(get(s, "scope"), "runId").as_str().unwrap_or("").to_string();
    let mut add = |kind: &str, leg: &V, event_id: &V, n: BigInt, suffix: &str| {
        if n > BigInt::from(0) {
            let row = json!({
                "exposureId": format!("scaled:{}:{}:{}", run_id, get(leg, "exchangeId").as_str().unwrap_or(""), suffix),
                "marketId": get(leg, "marketId"),
                "eventId": event_id,
                if kind == "owned" { "nominalCost" } else { "maximumNominalCost" }: baal_paper::fixed::money(&n),
            });
            if kind == "owned" {
                owned.push(row);
            } else {
                reservations.push(row);
            }
        }
    };
    if let V::Object(inv) = get(s, "inventory") {
        for (ex, slot) in inv {
            let q = js_int(get(get(slot, "proof"), "remainingOwnedQuantity")).unwrap_or(0);
            if q == 0 {
                continue;
            }
            let legs = get(get(slot, "package"), "legs").as_array().cloned().unwrap_or_default();
            let first = legs
                .iter()
                .find(|l| get(l, "exchangeId").as_str() == Some(ex.as_str()))
                .cloned()
                .unwrap_or(V::Null);
            let other = legs
                .iter()
                .find(|l| get(l, "exchangeId").as_str() != Some(ex.as_str()))
                .cloned()
                .unwrap_or(V::Null);
            let cost = amount_v(get(get(slot, "proof"), "remainingOwnedCost")).map(BigInt::from)?;
            add("owned", &first, get(get(slot, "package"), "eventId"), cost.clone(), "owned");
            add(
                "reserved",
                &other,
                get(get(slot, "package"), "eventId"),
                BigInt::from(990_000i64) * BigInt::from(q) - cost,
                &format!("hedge-for-{ex}"),
            );
        }
    }
    let o = active(s);
    if get(get(&o.clone().unwrap_or(V::Null), "body"), "action").as_str() == Some("buy") {
        let o = o.clone().unwrap();
        let legs = get(get(&o, "package"), "legs")
            .as_array()
            .cloned()
            .unwrap_or_default();
        let first = legs
            .iter()
            .find(|l| get(l, "exchangeId") == get(get(&o, "body"), "exchangeId"))
            .cloned()
            .unwrap_or(V::Null);
        let other = legs
            .iter()
            .find(|l| get(l, "exchangeId") != get(get(&o, "body"), "exchangeId"))
            .cloned()
            .unwrap_or(V::Null);
        let entry = price(&js_string_repr(get(get(&o, "body"), "price")))
            .map(BigInt::from)?
            * js_int(get(get(&o, "body"), "quantity")).map(BigInt::from).unwrap_or_default();
        add("reserved", &first, get(get(&o, "package"), "eventId"), entry.clone(), "entry");
        add(
            "reserved",
            &other,
            get(get(&o, "package"), "eventId"),
            BigInt::from(990_000i64) * js_int(get(get(&o, "body"), "quantity")).map(BigInt::from).unwrap_or_default()
                - entry,
            &format!("hedge-for-{}", get(&first, "exchangeId").as_str().unwrap_or("")),
        );
    }
    let mut body = json!({
        "schemaVersion": 1,
        "kind": "baal_scaled_no_full_nominal_reservation_v1",
        "ownerId": get(get(s, "scope"), "ownerId"),
        "runId": get(get(s, "scope"), "runId"),
        "tournamentId": get(get(s, "scope"), "tournamentId"),
        "allocationHash": payload_hash(get(s, "allocation"))?,
        "owned": owned.clone(),
        "reservations": reservations.clone(),
        "unresolvedIntentIds": match &o {
            Some(o) => vec![get(o, "intentId").clone()],
            None => Vec::new(),
        },
        "cashReserved": baal_paper::fixed::money(&reservations.iter().fold(BigInt::from(0), |n, r| {
            n + get(r, "maximumNominalCost").as_str().and_then(|s| amount(s).ok()).map(BigInt::from).unwrap_or_default()
        })),
        "ownedPrincipal": baal_paper::fixed::money(&owned.iter().fold(BigInt::from(0), |n, r| {
            n + get(r, "nominalCost").as_str().and_then(|s| amount(s).ok()).map(BigInt::from).unwrap_or_default()
        })),
    });
    let h = payload_hash(&body)?;
    body.as_object_mut()
        .map(|m| m.insert("reservationHash".into(), json!(h)));
    Ok(body)
}

/// projectBaalScaledReservation(state)
pub fn project_baal_scaled_reservation(state: &V) -> Result<V, String> {
    need(
        get(state, "mode").as_str() == Some("baal_scaled_no_controller_v1"),
        "PROJECTION_MODE",
    )?;
    reservation(state)
}

// risk(s, g, at, {cleanup})
fn risk(s: &V, g: &V, at: i64, cleanup: bool) -> Result<(), String> {
    let o = active(s);
    let o_intent = o.as_ref().map(|o| get(o, "intentId").clone()).unwrap_or(V::Null);
    let unresolved = get(g, "unresolvedIntentIds").as_array().cloned().unwrap_or_default();
    need(
        get(g, "ownerId") == get(get(s, "scope"), "ownerId")
            && get(g, "runId") == get(get(s, "scope"), "runId")
            && get(g, "tournamentId") == get(get(s, "scope"), "tournamentId")
            && get(g, "complete") == &json!(true)
            && get(g, "eventScopeComplete") == &json!(true)
            && get(g, "soleCollector") == &json!(true)
            && get(g, "stopped") == &json!(false)
            && hex(get(g, "sourceHash"))
            && instant_v(get(g, "at"))? <= at
            && at - instant_v(get(g, "at"))? <= 20000
            && get(g, "allocationHash").as_str() == Some(payload_hash(get(s, "allocation"))?.as_str())
            && get(g, "budgetInstalledHash") == get(get(s, "activation"), "budgetInstalledHash"),
        "CURRENT_GLOBAL_RISK",
    )?;
    let phase_ok = o
        .as_ref()
        .map(|o| ["prepared", "dispatching"].contains(&get(o, "phase").as_str().unwrap_or("")))
        .unwrap_or(false);
    need(
        get(g, "unresolvedIntentIds").is_array()
            && unresolved.len() <= 1
            && unresolved.iter().all(|x| *x == o_intent)
            && (cleanup
                || get(g, "unknownAcceptance") == &json!(false)
                || (unresolved.len() == 1 && phase_ok)),
        "GLOBAL_UNKNOWN",
    )?;
    Ok(())
}

// budget(s, g, next, at, {cash=true})
fn budget(s: &V, g: &V, next: &V, at: i64, cash: bool) -> Result<(), String> {
    risk(s, g, at, false)?;
    let old = reservation(s)?;
    need(
        get(g, "controllerReservationHash").as_str() == get(&old, "reservationHash").as_str()
            && hex(get(g, "globalLedgerHash"))
            && get(g, "globalLedgerHash") == get(get(s, "reservation"), "globalLedgerHash"),
        "GLOBAL_RESERVATION_BINDING",
    )?;
    let mut rows: Vec<(String, V, BigInt)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (k, f) in [
        ("owned", "nominalCost"),
        ("reservations", "maximumNominalCost"),
        ("loans", "fullNominalPrincipal"),
    ] {
        need(get(g, k).is_array(), "GLOBAL_EXPOSURES")?;
        for r in get(g, k).as_array().unwrap() {
            let n = amount_v(get(r, f)).map(BigInt::from)?;
            need(
                get(r, "exposureId").is_string()
                    && !seen.contains(get(r, "exposureId").as_str().unwrap_or(""))
                    && get(r, "marketId").is_string()
                    && get(r, "eventId").as_str().is_some_and(|s| !s.is_empty())
                    && n >= BigInt::from(0)
                    && get(r, "unknownAcceptance") != &json!(true),
                "GLOBAL_EXPOSURE",
            )?;
            seen.insert(get(r, "exposureId").as_str().unwrap_or("").to_string());
            rows.push((k.to_string(), r.clone(), n));
        }
    }
    for k in ["owned", "reservations"] {
        for r in get(&old, k).as_array().cloned().unwrap_or_default() {
            let idx = rows
                .iter()
                .position(|(kind, v, _)| {
                    kind == k && get(v, "exposureId") == get(&r, "exposureId") && same(v, &r)
                })
                .ok_or("BAAL_SCALED_OWN_GLOBAL_EXPOSURE_MISSING")?;
            rows.remove(idx);
        }
    }
    for k in ["owned", "reservations"] {
        let f = if k == "owned" { "nominalCost" } else { "maximumNominalCost" };
        for r in get(next, k).as_array().cloned().unwrap_or_default() {
            rows.push((k.to_string(), r.clone(), amount_v(get(&r, f)).map(BigInt::from)?));
        }
    }
    let mut total = BigInt::from(0);
    let mut reserved = BigInt::from(0);
    let mut markets: BTreeMap<String, BigInt> = BTreeMap::new();
    let mut events: BTreeMap<String, BigInt> = BTreeMap::new();
    let mut market_events: HashMap<String, V> = HashMap::new();
    for (kind, v, cost) in &rows {
        let market_id = get(v, "marketId").as_str().unwrap_or("").to_string();
        let event_id = get(v, "eventId").clone();
        if let Some(prev) = market_events.get(&market_id) {
            need(prev == &event_id, "MARKET_EVENT_CHANGED")?;
        }
        market_events.insert(market_id.clone(), event_id.clone());
        total += cost;
        if kind == "reservations" {
            reserved += cost;
        }
        *markets.entry(market_id).or_insert_with(|| BigInt::from(0)) += cost;
        *events
            .entry(event_id.as_str().unwrap_or("").to_string())
            .or_insert_with(|| BigInt::from(0)) += cost;
    }
    let max_markets = get(get(s, "allocation"), "maximumActiveMarkets");
    let max_markets = if max_markets.is_null() { 20 } else { js_int(max_markets).unwrap_or(20) };
    let max_market_cost = amount_v(
        &get(get(s, "allocation"), "maximumMarketCost")
            .clone()
            .pipe(|v| if v.is_null() { json!("100.000000") } else { v }),
    )?;
    let max_event_cost = amount_v(
        &get(get(s, "allocation"), "maximumEventCost")
            .clone()
            .pipe(|v| if v.is_null() { json!("200.000000") } else { v }),
    )?;
    need(
        total <= BigInt::from(1_000_000_000i64)
            && markets.values().filter(|v| **v > BigInt::from(0)).count() as i64 <= max_markets
            && markets.values().all(|v| *v <= BigInt::from(max_market_cost))
            && events.values().all(|v| *v <= BigInt::from(max_event_cost)),
        "CAPITAL_OR_EVENT_CEILING",
    )?;
    need(
        get(g, "cashLower").is_string()
            && amount_v(get(g, "cashLower"))? >= 0
            && (!cash || reserved <= BigInt::from(amount_v(get(g, "cashLower"))?)),
        "GLOBAL_CASH_FUNDING",
    )?;
    Ok(())
}

// continueOwned(s, ex, a, at) — returns the inspected account.
fn continue_owned(s: &V, ex: &str, a: &V, at: i64) -> Result<V, String> {
    let member = get(get(s, "scope"), "exchanges")
        .as_array()
        .and_then(|a| a.iter().find(|x| get(x, "exchangeId").as_str() == Some(ex)))
        .cloned();
    need(member.is_some(), "MARKET_SCOPE")?;
    let member = member.unwrap();
    let mut binding = get(s, "scope").clone();
    if let (V::Object(b), V::Object(m)) = (&mut binding, &member) {
        for (k, v) in m {
            b.insert(k.clone(), v.clone());
        }
    }
    let current = inspect_scoped_rest_no_account(&json!({
        "account": own_account(a, ex),
        "binding": binding,
        "readAt": iso(at as f64),
    }))?;
    let p = get(&current, "positions")
        .as_array()
        .and_then(|a| a.iter().find(|x| get(x, "exchangeId").as_str() == Some(ex)))
        .cloned();
    need(
        p.is_none()
            || (get(p.as_ref().unwrap(), "marketId") == get(&member, "marketId")
                && get(p.as_ref().unwrap(), "settled") != &json!(true)),
        "POSITION_SCOPE",
    )?;
    let slot = get(get(s, "inventory"), ex).clone();
    if !slot.is_null() {
        let mut binding2 = get(s, "scope").clone();
        if let (V::Object(b), V::Object(m)) = (&mut binding2, &member) {
            for (k, v) in m {
                b.insert(k.clone(), v.clone());
            }
        }
        let old = inspect_scoped_rest_no_account(&json!({
            "account": get(&slot, "after"),
            "binding": binding2,
            "readAt": get(get(&slot, "proof"), "readAt"),
        }))?;
        need(
            instant_v(get(get(&slot, "proof"), "readAt"))? <= instant_v(get(&current, "startedAt"))?
                && same(
                    &json!(sorted(get(&current, "scopedOrders").as_array().cloned().unwrap_or_default().as_slice())),
                    &json!(sorted(get(&old, "scopedOrders").as_array().cloned().unwrap_or_default().as_slice())),
                )
                && p.as_ref().and_then(|p| js_int(get(p, "quantity"))).unwrap_or(0)
                    == -js_int(get(get(&slot, "proof"), "remainingOwnedQuantity")).unwrap_or(i64::MIN),
            "OWNED_HISTORY_OR_QUANTITY",
        )?;
    } else {
        need(
            p.as_ref().and_then(|p| js_int(get(p, "quantity"))).unwrap_or(0) == 0,
            "FOREIGN_LOT",
        )?;
    }
    Ok(current)
}

// nativeEntry(s, e, at)
fn native_entry(s: &V, e: &V, at: i64) -> Result<V, String> {
    let mut account_risk = get(e, "accountRisk").clone();
    let o = active(s);
    // A fresh packet for the SAME not-yet-fetched intent can contain its
    // already installed reservation. Substitute only those exact rows in the
    // signal's sizing projection. The full current global budget is checked
    // separately.
    let is_buy = o
        .as_ref()
        .map(|o| get(get(o, "body"), "action").as_str() == Some("buy"))
        .unwrap_or(false);
    if is_buy {
        let o = o.clone().unwrap();
        if ["prepared", "dispatching"].contains(&get(&o, "phase").as_str().unwrap_or(""))
            && get(&o, "fetchStarts").as_array().map(|a| a.is_empty()).unwrap_or(true)
        {
            let mut s_without = s.clone();
            if let V::Object(m) = &mut s_without {
                let filtered: Vec<V> = get(s, "orders")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|v| get(v, "intentId") != get(&o, "intentId"))
                    .collect();
                m.insert("orders".into(), V::Array(filtered));
            }
            let full = reservation(s)?;
            let without = reservation(&s_without)?;
            let own: Vec<V> = get(&full, "reservations")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|r| {
                    !get(&without, "reservations")
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .any(|v| get(v, "exposureId") == get(r, "exposureId"))
                })
                .collect();
            let rows = account_risk.get("reservations").cloned().unwrap_or(V::Null);
            let ids = account_risk.get("unresolvedIntentIds").cloned().unwrap_or(V::Null);
            if rows.is_array() && ids.is_array() {
                let ids_v = ids.as_array().unwrap();
                let rows_v = rows.as_array().unwrap();
                if ids_v.iter().any(|id| id == get(&o, "intentId"))
                    || rows_v.iter().any(|r| {
                        own.iter().any(|v| get(v, "exposureId") == get(r, "exposureId"))
                    })
                {
                    need(
                        get(&account_risk, "unknownAcceptance") == &json!(false)
                            && ids_v.iter().all(|id| id == get(&o, "intentId"))
                            && own.iter().all(|r| {
                                rows_v.iter().any(|v| same(v, r))
                            }),
                        "PREPARED_RISK_BINDING",
                    )?;
                    let original_risk_hash = payload_hash(&account_risk)?;
                    let mut ar = account_risk.clone();
                    if let V::Object(m) = &mut ar {
                        m.insert(
                            "reservations".into(),
                            V::Array(
                                rows_v
                                    .iter()
                                    .filter(|r| {
                                        !own.iter().any(|v| get(v, "exposureId") == get(r, "exposureId"))
                                    })
                                    .cloned()
                                    .collect(),
                            ),
                        );
                        m.insert(
                            "unresolvedIntentIds".into(),
                            V::Array(
                                ids_v
                                    .iter()
                                    .filter(|id| *id != get(&o, "intentId"))
                                    .cloned()
                                    .collect(),
                            ),
                        );
                        m.insert(
                            "sourceHash".into(),
                            json!(payload_hash(&json!({
                                "kind": "exact_unfetched_intent_reservation_substitution",
                                "originalRiskHash": original_risk_hash,
                                "intentId": get(&o, "intentId"),
                                "reservationHash": get(&full, "reservationHash"),
                            }))?),
                        );
                    }
                    account_risk = ar;
                }
            }
        }
    }
    let mut e2 = e.clone();
    if let V::Object(m) = &mut e2 {
        m.insert("accountRisk".into(), account_risk);
        m.insert("allocation".into(), get(s, "allocation").clone());
        m.insert("scope".into(), get(s, "scope").clone());
        m.insert("now".into(), json!(iso(at as f64)));
    }
    let result = plan_baal_scaled_entry(&e2);
    need(
        get(&result, "proposalReady") == &json!(true),
        &format!("ENTRY_{}", get(&result, "reason").as_str().unwrap_or("")),
    )?;
    for l in get(&result, "legs").as_array().cloned().unwrap_or_default() {
        need(
            get(get(s, "scope"), "exchanges")
                .as_array()
                .map(|a| {
                    a.iter().any(|m| {
                        get(m, "exchangeId") == get(&l, "exchangeId")
                            && get(m, "marketId") == get(&l, "marketId")
                    })
                })
                .unwrap_or(false),
            "ENTRY_OUTSIDE_ACTIVATION",
        )?;
    }
    Ok(result)
}

// bookVersions(s, o, e)
fn book_versions(s: &V, o: &V, e: &V) -> Result<V, String> {
    let list: Vec<V> = if get(get(o, "body"), "action").as_str() == Some("buy") {
        get(e, "bookReceipts").as_array().cloned().unwrap_or_default()
    } else {
        vec![json!({
            "marketId": get(o, "marketId"),
            "exchangeId": get(get(o, "body"), "exchangeId"),
            "receipt": get(e, "book"),
        })]
    };
    let mut out = Map::new();
    for c in &list {
        let endpoint = format!(
            "/markets/{}/orderbook?tournamentId={}&depth=20",
            get(c, "marketId").as_str().unwrap_or(""),
            get(get(s, "scope"), "tournamentId").as_str().unwrap_or("")
        );
        let r = raw(get(c, "receipt"), "GET", &endpoint)?;
        let (d, _) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
        let b = normalize_book(
            &d,
            get(c, "marketId").as_str().unwrap_or(""),
            get(get(s, "scope"), "tournamentId").as_str().unwrap_or(""),
            get(&r, "receivedAt").as_str().unwrap_or(""),
            get(&r, "requestStartedAt").as_str().unwrap_or(""),
            20000,
            &std::collections::BTreeSet::new(),
            "authoritative_receipt_v1",
        )?;
        need(
            get(&b, "exchangeId") == get(c, "exchangeId"),
            "BOOK_EXCHANGE",
        )?;
        let v = json!({
            "sequence": get(&b, "sequence"),
            "sourceAt": get(&b, "sourceAt"),
            "requestStartedAt": get(&r, "requestStartedAt"),
            "receivedAt": get(&r, "receivedAt"),
            "depthHash": payload_hash(&json!({
                "bids": get(&b, "bids"),
                "asks": get(&b, "asks"),
            }))?,
        });
        let ex = get(c, "exchangeId").as_str().unwrap_or("").to_string();
        let old = get(o, "books").get(&ex).cloned();
        if let Some(old) = old {
            if !old.is_null() {
                need(
                    instant_v(get(&v, "requestStartedAt"))? >= instant_v(get(&old, "requestStartedAt"))?
                        && instant_v(get(&v, "receivedAt"))? >= instant_v(get(&old, "receivedAt"))?
                        && js_int(get(&v, "sequence")).unwrap_or(i64::MIN)
                            >= js_int(get(&old, "sequence")).unwrap_or(i64::MAX)
                        && instant_ns(get(&v, "sourceAt").as_str().unwrap_or(""))
                            .map(|x| x)
                            .unwrap_or(i128::MIN)
                            >= instant_ns(get(&old, "sourceAt").as_str().unwrap_or(""))
                                .map(|x| x)
                                .unwrap_or(i128::MAX),
                    "BOOK_ROLLBACK",
                )?;
                if get(&v, "sequence") == get(&old, "sequence")
                    && get(&v, "sourceAt") == get(&old, "sourceAt")
                {
                    need(
                        get(&v, "depthHash") == get(&old, "depthHash"),
                        "BOOK_VERSION_CONFLICT",
                    )?;
                }
            }
        }
        out.insert(ex, v);
    }
    Ok(V::Object(out))
}

// sellSource(s, ex, requested, e, at)
fn sell_source(s: &V, ex: &str, requested: Option<i64>, e: &V, at: i64) -> Result<V, String> {
    let slot = get(get(s, "inventory"), ex).clone();
    need(
        js_int(get(get(&slot, "proof"), "remainingOwnedQuantity")).unwrap_or(0) > 0,
        "NO_OWNED_LOT",
    )?;
    let a = continue_owned(s, ex, get(e, "account"), at)?;
    let member = get(get(s, "scope"), "exchanges")
        .as_array()
        .and_then(|a| a.iter().find(|m| get(m, "exchangeId").as_str() == Some(ex)))
        .cloned()
        .unwrap_or(V::Null);
    let endpoint = format!(
        "/markets/{}/orderbook?tournamentId={}&depth=20",
        get(&member, "marketId").as_str().unwrap_or(""),
        get(get(s, "scope"), "tournamentId").as_str().unwrap_or("")
    );
    let r = raw(get(e, "book"), "GET", &endpoint)?;
    need(
        get(&r, "status") == &json!(200)
            && instant_v(get(&r, "receivedAt"))? <= at
            && instant_v(get(&r, "requestStartedAt"))? >= instant_v(get(&a, "completedAt"))?
            && at - instant_v(get(&r, "requestStartedAt"))? <= 20000,
        "FRESH_ACCOUNT_THEN_BOOK",
    )?;
    let (d, numbers) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
    need(
        d.get("marketId").is_none() || get(&d, "marketId") == get(&member, "marketId"),
        "BOOK_MARKET",
    )?;
    let b = normalize_book(
        &d,
        get(&member, "marketId").as_str().unwrap_or(""),
        get(get(s, "scope"), "tournamentId").as_str().unwrap_or(""),
        get(&r, "receivedAt").as_str().unwrap_or(""),
        get(&r, "requestStartedAt").as_str().unwrap_or(""),
        20000,
        &std::collections::BTreeSet::new(),
        "authoritative_receipt_v1",
    )?;
    need(
        get(&b, "exchangeId").as_str() == Some(ex)
            && get(&d, "contexts").as_array().map(|x| x.len()) == Some(1)
            && get(&get(&get(&d, "contexts")[0], "orderbook"), "exchanges")
                .as_array()
                .map(|x| x.len())
                == Some(1)
            && get(&b, "asks").as_array().map(|x| x.len() <= 20).unwrap_or(false),
        "BOOK_SCOPE",
    )?;
    let limit_qty = std::cmp::min(
        requested.unwrap_or(100),
        std::cmp::min(
            js_int(get(get(&slot, "proof"), "remainingOwnedQuantity")).unwrap_or(0),
            get(get(s, "allocation"), "maximumSharesPerOrder")
                .clone()
                .pipe(|v| if v.is_null() { 100 } else { js_int(&v).unwrap_or(0) }),
        ),
    );
    need(limit_qty > 0, "SELL_QUANTITY")?;
    for (i, l) in get(&b, "asks").as_array().cloned().unwrap_or_default().iter().enumerate() {
        let lex = numbers.get(&path_key(&[
            json!("contexts"), json!(0), json!("orderbook"), json!("exchanges"),
            json!(0), json!("asks"), json!(i), json!("quantity"),
        ]));
        let lex_ok = lex.is_some_and(|s| {
            let mut parts = s.splitn(2, '.');
            let whole = parts.next().unwrap_or("");
            let frac = parts.next();
            (whole == "0" || (!whole.is_empty() && !whole.starts_with('0')))
                && whole.bytes().all(|x| x.is_ascii_digit())
                && match frac {
                    None => true,
                    Some(f) => !f.is_empty() && f.bytes().all(|x| x == b'0'),
                }
        }) && lex
            .and_then(|s| s.split('.').next())
            .and_then(|s| s.parse::<BigInt>().ok())
            == js_int(get(l, "quantity")).map(BigInt::from);
        need(lex_ok, "EXACT_DEPTH")?;
        let limit = SCALE - price(get(l, "price").as_str().unwrap_or(""))? - TICK;
        if limit < TICK {
            continue;
        }
        let order_cap = amount_v(
            &get(get(s, "allocation"), "maximumOrderCost")
                .clone()
                .pipe(|v| if v.is_null() { json!("25.000000") } else { v }),
        )?;
        let affordable = (BigInt::from(order_cap) / BigInt::from(limit))
            .try_into()
            .unwrap_or(0i64);
        let q = std::cmp::min(
            limit_qty,
            std::cmp::min(js_int(get(l, "quantity")).unwrap_or(0) / 100, affordable),
        );
        if q < 1 {
            continue;
        }
        let mut left = q;
        let mut max_cost: i128 = 0;
        for lot in get(get(&slot, "proof"), "remainingLots")
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            if left <= 0 {
                break;
            }
            let p = price(get(&lot, "entryPrice").as_str().unwrap_or(""))?;
            if p > max_cost {
                max_cost = p;
            }
            left -= std::cmp::min(left, js_int(get(&lot, "quantity")).unwrap_or(0));
        }
        if limit < max_cost + TICK {
            continue;
        }
        return Ok(json!({
            "marketId": get(&member, "marketId"),
            "exchangeId": ex,
            "quantity": q,
            "limit": money(limit),
            "bookVersion": get(&b, "versionKey"),
            "bookRequestStartedAt": get(&r, "requestStartedAt"),
            "sourceBodyHash": get(&r, "sourceBodyHash"),
            "minimumNetPerShare": "0.005000",
            "costProtection": "each_consumed_fifo_lot",
            "at": iso(at as f64),
        }));
    }
    need(false, "PROTECTED_SELL_UNAVAILABLE")?;
    unreachable!()
}

// preflight(s, o, e, g, at, planned=s)
fn preflight(s: &V, o: &V, e: &V, g: &V, at: i64, planned: &V) -> Result<(), String> {
    book_versions(s, o, e)?;
    need(
        js_int(get(get(o, "body"), "quantity")).unwrap_or(i64::MAX) <= 100
            && BigInt::from(price(&js_string_repr(get(get(o, "body"), "price")))?)
                * js_int(get(get(o, "body"), "quantity")).map(BigInt::from).unwrap_or_default()
                <= amount_v(
                    &get(get(s, "allocation"), "maximumOrderCost")
                        .clone()
                        .pipe(|v| if v.is_null() { json!("25.000000") } else { v }),
                )
                .map(BigInt::from)?
            && at < instant_v(get(get(o, "body"), "expirationDate"))?,
        "ORDER_BOUND_OR_EXPIRED",
    )?;
    risk(s, g, at, false)?;
    budget(s, g, &reservation(planned)?, at, get(get(o, "body"), "action").as_str() == Some("buy"))?;
    if get(get(o, "body"), "action").as_str() == Some("buy") {
        need(
            at >= instant_v(get(get(s, "activation"), "startedAt"))?
                && at < instant_v(get(get(s, "activation"), "entryExpiresAt"))?
                && at < instant_v(get(get(o, "proposal"), "expiresAt"))?,
            "ENTRY_WINDOW",
        )?;
        let p = native_entry(s, e, at)?;
        let other = get(&p, "legs")
            .as_array()
            .and_then(|a| {
                a.iter()
                    .find(|l| get(l, "exchangeId") != get(get(o, "body"), "exchangeId"))
            })
            .cloned()
            .unwrap_or(V::Null);
        need(
            get(get(&p, "chosenLeg"), "exchangeId") == get(get(&o, "body"), "exchangeId")
                && get(&p, "eventId") == get(get(o, "package"), "eventId")
                && js_int(get(get(&p, "chosenLeg"), "quantity")).unwrap_or(0)
                    >= js_int(get(get(o, "body"), "quantity")).unwrap_or(i64::MAX)
                && price(get(get(&p, "chosenLeg"), "limit").as_str().unwrap_or(""))?
                    <= price(&js_string_repr(get(get(o, "body"), "price")))?
                && BigInt::from(price(get(&other, "limit").as_str().unwrap_or(""))?)
                    + price(&js_string_repr(get(get(o, "body"), "price"))).map(BigInt::from)?
                    <= BigInt::from(990_000i64)
                && get(get(&p, "sourceReceipts"), "roots")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .all(|r| {
                        get(get(get(o, "proposal"), "sourceReceipts"), "roots")
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                            .iter()
                            .any(|old| {
                                get(old, "marketId") == get(r, "marketId")
                                    && get(old, "exchangeId") == get(r, "exchangeId")
                                    && get(old, "rootHash") == get(r, "rootHash")
                            })
                    }),
            "FIXED_ENTRY_PROPOSAL",
        )?;
        continue_owned(
            s,
            get(get(o, "body"), "exchangeId").as_str().unwrap_or(""),
            get(e, "account"),
            at,
        )?;
    } else {
        let p = sell_source(
            s,
            get(get(o, "body"), "exchangeId").as_str().unwrap_or(""),
            js_int(get(get(o, "body"), "quantity")),
            e,
            at,
        )?;
        need(
            js_int(get(&p, "quantity")).unwrap_or(0)
                >= js_int(get(get(o, "body"), "quantity")).unwrap_or(i64::MAX)
                && price(get(&p, "limit").as_str().unwrap_or(""))?
                    >= price(&js_string_repr(get(get(o, "body"), "price")))?,
            "FIXED_SELL_PROTECTION",
        )?;
    }
    Ok(())
}

// cancelCheck(s, e, at)
fn cancel_check(s: &V, e: &V, at: i64) -> Result<(), String> {
    let o = active(s);
    need(
        o.as_ref()
            .and_then(|o| get(o, "orderId").as_str().map(String::from))
            .is_some_and(|x| !x.is_empty())
            && at >= instant_v(get(get(o.as_ref().unwrap(), "body"), "expirationDate"))?,
        "CANCEL_NOT_EXPIRED_OR_OWNED",
    )?;
    risk(s, get(e, "globalRisk"), at, true)?;
    let endpoint = format!(
        "/orders/{}",
        get(o.as_ref().unwrap(), "orderId").as_str().unwrap_or("")
    );
    let r = raw(get(e, "order"), "GET", &endpoint)?;
    let (d, _) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
    let o = o.clone().unwrap();
    need(
        get(&r, "status") == &json!(200)
            && instant_v(get(&r, "receivedAt"))? <= at
            && at - instant_v(get(&r, "requestStartedAt"))? <= 20000
            && js_int(get(&d, "id")).map(|x| x.to_string()) == get(&o, "orderId").as_str().map(String::from)
            && get(&d, "exchangeId") == get(get(&o, "body"), "exchangeId")
            && get(&d, "tournamentId") == get(get(s, "scope"), "tournamentId")
            && get(&d, "side").as_str() == Some("no")
            && get(&d, "action") == get(get(&o, "body"), "action")
            && get(&d, "open") == &json!(true)
            && normalize_official_fill_tick(get(&d, "priceLimit"))
                .ok()
                .and_then(|t| t.get("price").cloned())
                .and_then(|p| p.as_str().map(String::from))
                == Some(money(price(&js_string_repr(get(get(&o, "body"), "price")))?))
            && get(&d, "expirationDate") == get(get(&o, "body"), "expirationDate"),
        "CANCEL_SCOPE",
    )?;
    Ok(())
}

// binding(s, o)
fn binding(s: &V, o: &V) -> V {
    let mut b = get(s, "scope").clone();
    if let V::Object(m) = &mut b {
        m.insert("marketId".into(), get(o, "marketId").clone());
        m.insert("exchangeId".into(), get(get(o, "body"), "exchangeId").clone());
        m.insert("orderId".into(), get(o, "orderId").clone());
        m.insert("intentId".into(), get(o, "intentId").clone());
        m.insert("idempotencyKey".into(), get(get(o, "body"), "idempotencyKey").clone());
        m.insert("side".into(), json!("no"));
        m.insert("action".into(), get(get(o, "body"), "action").clone());
        m.insert("quantity".into(), get(get(o, "body"), "quantity").clone());
        m.insert(
            "limit".into(),
            json!(money(
                price(&js_string_repr(get(get(o, "body"), "price"))).unwrap_or(0)
            )),
        );
        m.insert("createdAt".into(), get(o, "createdAt").clone());
        m.insert(
            "expirationDate".into(),
            get(get(o, "body"), "expirationDate").clone(),
        );
    }
    b
}

// canonicalAck(o, r) — bool; errors collapse to false.
fn canonical_ack(o: &V, r: &V) -> bool {
    let result: Result<bool, String> = (|| {
        let (d, numbers) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
        let exact = |k: &str| -> bool {
            let v = numbers.get(&path_key(&[json!(k)]));
            v.is_some_and(|s| {
                s.len() <= 128 && {
                    let mut parts = s.splitn(2, '.');
                    let whole = parts.next().unwrap_or("");
                    let frac = parts.next();
                    (whole == "0" || (!whole.is_empty() && !whole.starts_with('0')))
                        && whole.bytes().all(|x| x.is_ascii_digit())
                        && match frac {
                            None => true,
                            Some(f) => !f.is_empty() && f.bytes().all(|x| x == b'0'),
                        }
                }
            }) && js_int(get(&d, k)).is_some()
                && v.and_then(|s| s.split('.').next())
                    .and_then(|s| s.parse::<BigInt>().ok())
                    == js_int(get(&d, k)).map(BigInt::from)
        };
        let cost = amount(
            numbers
                .get(&path_key(&[json!("totalCost")]))
                .map(|s| s.as_str())
                .unwrap_or(""),
        )
        .map(BigInt::from)?;
        let q = js_int(get(&d, "quantityTraded")).unwrap_or(i64::MIN);
        let limit = price(&js_string_repr(get(get(o, "body"), "price")))?;
        need(
            get(r, "status") == &json!(200)
                && js_int(get(&d, "orderId")).unwrap_or(0) > 0
                && get(&d, "exchangeId") == get(get(o, "body"), "exchangeId")
                && get(&d, "side").as_str() == Some("no")
                && get(&d, "action") == get(get(o, "body"), "action")
                && js_int(get(&d, "quantity")).unwrap_or(0) > 0
                && js_int(get(&d, "quantity")).unwrap_or(i64::MAX)
                    <= js_int(get(get(o, "body"), "quantity")).unwrap_or(i64::MIN)
                && ["orderId", "quantity", "quantityTraded", "remainingQuantity"]
                    .iter()
                    .all(|k| exact(k))
                && q >= 0
                && js_int(get(&d, "remainingQuantity")).unwrap_or(-1) >= 0
                && q + js_int(get(&d, "remainingQuantity")).unwrap_or(i64::MIN)
                    == js_int(get(&d, "quantity")).unwrap_or(i64::MAX)
                && get(&d, "open").is_boolean()
                && (get(&d, "open") == &json!(false)
                    || js_int(get(&d, "remainingQuantity")).unwrap_or(0) > 0)
                && normalize_official_fill_tick(get(&d, "price"))
                    .ok()
                    .and_then(|t| t.get("price").cloned())
                    .and_then(|p| p.as_str().map(String::from))
                    == Some(money(limit)),
            "ACK_CANONICAL",
        )?;
        let filled = js_int(get(&d, "quantityTraded")).unwrap_or(-1);
        need(
            if filled == 0 {
                cost == BigInt::from(0) && get(&d, "fillPrice").is_null()
            } else {
                cost >= BigInt::from(TICK) * BigInt::from(filled)
                    && cost <= BigInt::from(995_000i64) * BigInt::from(filled)
                    && get(&d, "fillPrice").is_f64()
                    && (if get(get(o, "body"), "action").as_str() == Some("buy") {
                        cost <= BigInt::from(limit) * BigInt::from(filled)
                    } else {
                        cost >= BigInt::from(limit) * BigInt::from(filled)
                    })
            },
            "ACK_PRINCIPAL",
        )?;
        if get(&d, "all").is_null() {
            return Ok(true);
        }
        let all = get(&d, "all");
        let mut fields: Vec<String> = all
            .as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        fields.sort();
        let mut want: Vec<String> = [
            "fullNotionalCost", "effectiveEntryCost", "collateralSavings", "relationshipIds",
            "componentId", "guaranteedPayoutFloorAfter", "outstandingAdvanceAfter",
            "collateralRepayment", "netBuyingPowerImpact", "redemptionCredit",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        want.sort();
        need(
            all.is_object()
                && fields == want
                && get(all, "relationshipIds").as_array().map(|a| a.is_empty()).unwrap_or(false)
                && get(all, "componentId").is_null(),
            "ACK_NON_NEUTRAL",
        )?;
        for k in ["fullNotionalCost", "effectiveEntryCost"] {
            need(
                amount(numbers.get(&path_key(&[json!("all"), json!(k)])).map(|s| s.as_str()).unwrap_or(""))
                    .map(BigInt::from)?
                    == cost,
                "ACK_COST",
            )?;
        }
        for k in [
            "collateralSavings", "guaranteedPayoutFloorAfter", "outstandingAdvanceAfter",
            "collateralRepayment", "redemptionCredit",
        ] {
            need(
                amount(numbers.get(&path_key(&[json!("all"), json!(k)])).map(|s| s.as_str()).unwrap_or(""))
                    .map(BigInt::from)?
                    == BigInt::from(0),
                "ACK_NON_NEUTRAL",
            )?;
        }
        let signed = if get(get(o, "body"), "action").as_str() == Some("buy") {
            cost.clone()
        } else {
            -cost.clone()
        };
        need(
            amount(
                numbers
                    .get(&path_key(&[json!("all"), json!("netBuyingPowerImpact")]))
                    .map(|s| s.as_str())
                    .unwrap_or(""),
            )
            .map(BigInt::from)?
                == signed,
            "ACK_SIGN",
        )?;
        Ok(true)
    })();
    result.unwrap_or(false)
}

// reduce(state, event) — the durable intent state machine.
fn reduce(state: Option<V>, e: &V) -> Result<V, String> {
    if get(e, "kind").as_str() == Some("initialize") {
        need(state.is_none(), "INITIALIZED")?;
        return Ok(json!({
            "schemaVersion": 1,
            "mode": "baal_scaled_no_controller_v1",
            "scope": get(e, "scope"),
            "allocation": get(e, "allocation"),
            "activation": get(e, "activation"),
            "inventory": {},
            "orders": [],
            "reservation": V::Null,
        }));
    }
    need(state.is_some(), "NOT_INITIALIZED")?;
    let s = state.unwrap();
    let mut n = s.clone();
    let o = active(&n);
    if get(e, "kind").as_str() == Some("reservation") {
        need(
            get(get(e, "receipt"), "reservationHash") == get(&reservation(&s)?, "reservationHash")
                && get(get(e, "receipt"), "allocationHash").as_str()
                    == Some(payload_hash(get(&s, "allocation"))?.as_str())
                && hex(get(get(e, "receipt"), "globalLedgerHash"))
                && instant_v(get(get(e, "receipt"), "confirmedAt"))? <= instant_v(get(e, "at"))?,
            "RESERVATION",
        )?;
        n.as_object_mut()
            .map(|m| m.insert("reservation".into(), get(e, "receipt").clone()));
        return Ok(n);
    }
    if get(e, "kind").as_str() == Some("prepared") {
        need(o.is_none(), "UNKNOWN_OR_OPEN_INTENT")?;
        let proposal: V;
        let pkg: V;
        let market_id: V;
        let ex: V;
        let q: V;
        let limit: V;
        if get(e, "action").as_str() == Some("buy") {
            need(
                instant_v(get(e, "at"))? >= instant_v(get(get(&s, "activation"), "startedAt"))?
                    && instant_v(get(e, "at"))? < instant_v(get(get(&s, "activation"), "entryExpiresAt"))?,
                "ENTRY_WINDOW",
            )?;
            proposal = native_entry(&s, get(e, "evidence"), instant_v(get(e, "at"))?)?;
            let chosen = get(&proposal, "chosenLeg").clone();
            market_id = get(&chosen, "marketId").clone();
            ex = get(&chosen, "exchangeId").clone();
            q = get(&chosen, "quantity").clone();
            limit = get(&chosen, "limit").clone();
            pkg = json!({
                "eventId": get(&proposal, "eventId"),
                "legs": get(&proposal, "legs").as_array().cloned().unwrap_or_default().iter().map(|l| json!({
                    "marketId": get(l, "marketId"),
                    "exchangeId": get(l, "exchangeId"),
                })).collect::<Vec<V>>(),
                "unitPackageCeiling": "0.990000",
            });
        } else {
            need(get(e, "action").as_str() == Some("sell"), "ACTION")?;
            let requested = if get(e, "quantity").is_null() {
            None
        } else {
            Some(js_int(get(e, "quantity")).unwrap_or(-1))
        };
        proposal = sell_source(
                &s,
                get(e, "exchangeId").as_str().unwrap_or(""),
                requested,
                get(e, "evidence"),
                instant_v(get(e, "at"))?,
            )?;
            market_id = get(&proposal, "marketId").clone();
            ex = get(&proposal, "exchangeId").clone();
            q = get(&proposal, "quantity").clone();
            limit = get(&proposal, "limit").clone();
            pkg = get(get(&s, "inventory"), get(e, "exchangeId").as_str().unwrap_or(""))
                .get("package")
                .cloned()
                .unwrap_or(V::Null);
        }
        need(
            get(e, "intentId").is_string()
                && get(e, "idempotencyKey").is_string()
                && !get(&s, "orders")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .any(|v| {
                        get(v, "intentId") == get(e, "intentId")
                            || get(get(v, "body"), "idempotencyKey") == get(e, "idempotencyKey")
                    }),
            "INTENT_ID",
        )?;
        let expires = iso(std::cmp::min(
            instant_v(get(e, "at"))? + 30000,
            if get(e, "action").as_str() == Some("buy") {
                instant_v(get(get(&s, "activation"), "entryExpiresAt"))?
            } else {
                i64::MAX
            },
        ) as f64);
        let body = json!({
            "idempotencyKey": get(e, "idempotencyKey"),
            "exchangeId": ex,
            "side": "no",
            "action": get(e, "action"),
            "quantity": q,
            "price": limit.as_str().and_then(|s| s.parse::<f64>().ok()).map(V::from).unwrap_or(V::Null),
            "expirationDate": expires,
            "tournamentId": get(get(&s, "scope"), "tournamentId"),
        });
        let mut candidate = json!({
            "intentId": get(e, "intentId"),
            "marketId": market_id,
            "createdAt": get(e, "at"),
            "body": body,
            "bodyJson": canonical_payload(&body)?,
            "bodyHash": payload_hash(&body)?,
            "proposal": proposal,
            "package": pkg,
            "before": own_account(get(get(e, "evidence"), "account"), ex.as_str().unwrap_or("")),
            "priorProof": get(get(&s, "inventory"), ex.as_str().unwrap_or("")).get("proof").cloned().unwrap_or(V::Null),
            "priorAfter": get(get(&s, "inventory"), ex.as_str().unwrap_or("")).get("after").cloned().unwrap_or(V::Null),
            "phase": "prepared",
            "orderId": V::Null,
            "ack": V::Null,
            "proof": V::Null,
            "cancelPending": false,
            "fences": [],
            "fetchStarts": [],
            "receipts": [],
        });
        let books = book_versions(&s, &candidate, get(e, "evidence"))?;
        candidate
            .as_object_mut()
            .map(|m| m.insert("books".into(), books));
        n.as_object_mut()
            .and_then(|m| m.get_mut("orders"))
            .and_then(V::as_array_mut)
            .map(|a| a.push(candidate.clone()));
        preflight(&s, &candidate, get(e, "evidence"), get(e, "globalRisk"), instant_v(get(e, "at"))?, &n)?;
        return Ok(n);
    }
    let mut o = o.ok_or("BAAL_SCALED_NO_ACTIVE_INTENT")?;
    need(get(&o, "proof").is_null(), "NO_ACTIVE_INTENT")?;
    match get(e, "kind").as_str().unwrap_or("") {
        "abandoned_prepared" => {
            need(
                get(&o, "phase").as_str() == Some("prepared")
                    && o.get("operation").is_none()
                    && get(&o, "orderId").is_null()
                    && get(&o, "ack").is_null()
                    && get(&o, "proof").is_null()
                    && get(&o, "cancelPending") == &json!(false)
                    && get(&o, "fences").as_array().map(|a| a.is_empty()).unwrap_or(false)
                    && get(&o, "fetchStarts").as_array().map(|a| a.is_empty()).unwrap_or(false)
                    && get(&o, "receipts").as_array().map(|a| a.is_empty()).unwrap_or(false)
                    && instant_v(get(e, "at"))? >= instant_v(get(get(&o, "body"), "expirationDate"))?,
                "ABANDON_REQUIRES_EXPIRED_NEVER_ADMITTED",
            )?;
            if let V::Object(m) = &mut o {
                m.insert("phase".into(), json!("abandoned"));
                m.insert("abandonedAt".into(), get(e, "at").clone());
                m.insert("abandonedReason".into(), json!("expired_before_any_admission"));
            }
        }
        "admitted" | "network_fence" => {
            if get(e, "operation").as_str() == Some("place") {
                need(
                    if get(e, "kind").as_str() == Some("admitted") {
                        get(&o, "phase").as_str() == Some("prepared")
                    } else {
                        get(&o, "phase").as_str() == Some("dispatching")
                    },
                    "PLACEMENT_PHASE",
                )?;
                preflight(&s, &o, get(e, "evidence"), get(e, "globalRisk"), instant_v(get(e, "at"))?, &s)?;
                let books = book_versions(&s, &o, get(e, "evidence"))?;
                o.as_object_mut()
                    .map(|m| m.insert("books".into(), books));
            } else {
                need(
                    get(e, "operation").as_str() == Some("cancel")
                        && (get(e, "kind").as_str() != Some("admitted")
                            || get(&o, "cancelPending") == &json!(false)),
                    "OPERATION",
                )?;
                cancel_check(&s, get(e, "evidence"), instant_v(get(e, "at"))?)?;
            }
            if get(e, "kind").as_str() == Some("admitted") {
                o.as_object_mut()
                    .map(|m| m.insert("operation".into(), get(e, "operation").clone()));
                if get(e, "operation").as_str() == Some("place") {
                    o.as_object_mut()
                        .map(|m| m.insert("phase".into(), json!("dispatching")));
                } else {
                    o.as_object_mut()
                        .map(|m| m.insert("cancelPending".into(), json!(true)));
                }
            } else {
                need(
                    get(&o, "operation") == get(e, "operation")
                        && Some(payload_hash(get(e, "leaseIdentity"))?)
                            == get(e, "leaseHash").as_str().map(String::from),
                    "LEASE_FENCE",
                )?;
                let mut fences = get(&o, "fences").as_array().cloned().unwrap_or_default();
                fences.push(json!({
                    "at": get(e, "at"),
                    "leaseIdentity": get(e, "leaseIdentity"),
                    "leaseHash": get(e, "leaseHash"),
                    "operation": get(e, "operation"),
                }));
                o.as_object_mut()
                    .map(|m| m.insert("fences".into(), V::Array(fences)));
            }
        }
        "fetch_start" => {
            let f = get(&o, "fences").as_array().and_then(|a| a.last()).cloned().unwrap_or(V::Null);
            let w = get(e, "start").clone();
            need(
                !f.is_null()
                    && get(&w, "intentId") == get(&o, "intentId")
                    && get(&w, "requestBodyHash") == get(&o, "bodyHash")
                    && get(&w, "requestBodyJson") == get(&o, "bodyJson")
                    && get(&w, "leaseHash") == get(&f, "leaseHash")
                    && same(get(&w, "leaseIdentity"), get(&f, "leaseIdentity"))
                    && get(&w, "fetchInvocationId").is_string()
                    && !get(&s, "orders")
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .any(|v| {
                            get(v, "fetchStarts")
                                .as_array()
                                .cloned()
                                .unwrap_or_default()
                                .iter()
                                .any(|x| get(x, "fetchInvocationId") == get(&w, "fetchInvocationId"))
                        })
                    && instant_v(get(&w, "requestStartedAt"))? >= instant_v(get(&f, "at"))?
                    && instant_v(get(&w, "requestStartedAt"))? <= instant_v(get(e, "at"))?
                    && get(&w, "method").as_str()
                        == Some(if get(&o, "operation").as_str() == Some("place") { "POST" } else { "DELETE" })
                    && get(&w, "endpoint").as_str()
                        == Some(if get(&o, "operation").as_str() == Some("place") {
                            "/orders".to_string()
                        } else {
                            format!("/orders/{}", get(&o, "orderId").as_str().unwrap_or(""))
                        }.as_str()),
                "ACTUAL_FETCH",
            )?;
            if get(&o, "operation").as_str() == Some("place") {
                need(
                    instant_v(get(&w, "requestStartedAt"))? < instant_v(get(get(&o, "body"), "expirationDate"))?,
                    "EXPIRED_FETCH",
                )?;
            }
            let mut starts = get(&o, "fetchStarts").as_array().cloned().unwrap_or_default();
            starts.push(w);
            o.as_object_mut()
                .map(|m| m.insert("fetchStarts".into(), V::Array(starts)));
        }
        "wire" => {
            let w = get(&o, "fetchStarts").as_array().and_then(|a| a.last()).cloned().unwrap_or(V::Null);
            let r = get(e, "receipt").clone();
            need(
                !w.is_null()
                    && get(&r, "fetchInvocationId") == get(&w, "fetchInvocationId")
                    && get(&r, "requestStartedAt") == get(&w, "requestStartedAt")
                    && get(&r, "method") == get(&w, "method")
                    && get(&r, "endpoint") == get(&w, "endpoint")
                    && get(&r, "sourceBodyHash").as_str()
                        == Some(sha256(get(&r, "bodyText").as_str().unwrap_or("").as_bytes()).as_str())
                    && instant_v(get(&r, "receivedAt"))? <= instant_v(get(e, "at"))?,
                "WIRE_BINDING",
            )?;
            let mut receipts = get(&o, "receipts").as_array().cloned().unwrap_or_default();
            receipts.push(r.clone());
            o.as_object_mut()
                .map(|m| m.insert("receipts".into(), V::Array(receipts)));
            if get(&o, "operation").as_str() == Some("cancel") {
                // fall through to write o into n
            } else {
                let d = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))
                    .map(|(d, _)| d)
                    .unwrap_or(V::Null);
                if js_int(get(&d, "orderId")).unwrap_or(0) > 0 {
                    let oid = js_int(get(&d, "orderId")).unwrap().to_string();
                    o.as_object_mut()
                        .map(|m| m.insert("orderId".into(), json!(oid)));
                }
                let mut ack = r.clone();
                ack.as_object_mut()
                    .map(|m| m.insert("requestBodyHash".into(), get(&o, "bodyHash").clone()));
                o.as_object_mut()
                    .map(|m| m.insert("ack".into(), ack));
                let phase = if get(&r, "status") == &json!(200) {
                    if canonical_ack(&o, &r) {
                        "acknowledged"
                    } else {
                        "semantic_mismatch"
                    }
                } else {
                    "uncertain"
                };
                o.as_object_mut()
                    .map(|m| m.insert("phase".into(), json!(phase)));
            }
        }
        "error" => {
            if get(&o, "phase").as_str() == Some("dispatching") {
                o.as_object_mut()
                    .map(|m| m.insert("phase".into(), json!("uncertain")));
            }
            o.as_object_mut()
                .map(|m| m.insert("lastError".into(), get(e, "code").clone()));
        }
        "reconciled" => {
            need(
                !get(&o, "orderId").is_null()
                    && !get(&o, "ack").is_null()
                    && get(&o, "phase").as_str() == Some("acknowledged"),
                "UNRESOLVED_ACK",
            )?;
            let start = get(&o, "fetchStarts")
                .as_array()
                .and_then(|a| a.iter().find(|v| get(v, "method").as_str() == Some("POST")))
                .cloned()
                .unwrap_or(V::Null);
            need(!start.is_null(), "ACTUAL_FETCH_MISSING")?;
            let mut evidence = get(e, "evidence").clone();
            if let V::Object(m) = &mut evidence {
                m.insert("binding".into(), binding(&s, &o));
                m.insert("before".into(), get(&o, "before").clone());
                m.insert("ack".into(), get(&o, "ack").clone());
                m.insert("dispatchReceipt".into(), start.clone());
                m.insert("readAt".into(), get(e, "at").clone());
                m.insert(
                    "reservedFunding".into(),
                    if get(get(&o, "body"), "action").as_str() == Some("buy") {
                        json!(baal_paper::fixed::money(
                            &(BigInt::from(990_000i64)
                                * js_int(get(get(&o, "body"), "quantity")).map(BigInt::from).unwrap_or_default())
                        ))
                    } else {
                        json!("0.000000")
                    },
                );
                m.insert("priorProof".into(), get(&o, "priorProof").clone());
                m.insert("priorAfter".into(), get(&o, "priorAfter").clone());
            }
            let proof = verify_scoped_rest_no_multishare(&evidence)?;
            if get(get(&o, "body"), "action").as_str() == Some("sell") {
                need(
                    amount(get(&proof, "realizedPnl").as_str().unwrap_or("")).map(BigInt::from)?
                        >= BigInt::from(TICK)
                            * js_int(get(&proof, "filledQuantity")).map(BigInt::from).unwrap_or_default(),
                    "PROTECTED_REALIZED_PROFIT",
                )?;
            }
            o.as_object_mut()
                .map(|m| m.insert("proof".into(), proof.clone()));
            o.as_object_mut()
                .map(|m| m.insert("phase".into(), json!("reconciled")));
            o.as_object_mut()
                .map(|m| m.insert("cancelPending".into(), json!(false)));
            n.as_object_mut()
                .and_then(|m| m.get_mut("inventory"))
                .and_then(V::as_object_mut)
                .map(|inv| {
                    inv.insert(
                        get(get(&o, "body"), "exchangeId").as_str().unwrap_or("").to_string(),
                        json!({
                            "proof": proof,
                            "after": get(get(e, "evidence"), "after"),
                            "package": get(&o, "package"),
                        }),
                    )
                });
        }
        _ => need(false, "EVENT")?,
    }
    // write the mutated active order back into n.orders (find by intentId)
    if let V::Object(m) = &mut n {
        if let Some(V::Array(orders)) = m.get_mut("orders") {
            for slot in orders.iter_mut() {
                if get(slot, "intentId") == get(&o, "intentId") {
                    *slot = o.clone();
                }
            }
        }
    }
    Ok(n)
}

/// The caller's dispatch may surface a captured wire receipt on failure
/// (JS error.wireReceipt). It is replayed into the journal before the error
/// propagates.
pub struct WireDispatchError {
    pub message: String,
    pub wire_receipt: Option<V>,
}
pub type WireDispatch = dyn for<'g> Fn(
    &V,
    Option<&'g Guard<'g>>,
    Option<&'g Guard<'g>>,
) -> Result<V, WireDispatchError>;

pub struct BaalScaledNoControllerDeps {
    pub dispatch: Rc<WireDispatch>,
    pub raw_receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub assert_account_lease: Rc<dyn Fn() -> V>,
    pub install_funding_reservation: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub get_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_evidence: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub now: Rc<dyn Fn() -> f64>,
}

#[derive(Clone)]
struct InFlight {
    method: String,
    endpoint: String,
}

struct CtrlInner {
    scope: V,
    allocation: V,
    activation: V,
    code: V,
    file: PathBuf,
    journal: Journal,
    state: Option<V>,
    closed: bool,
    failed: bool,
    busy: bool,
    size: u64,
    bytes_hash: String,
    ino: u64,
    birthtime_ms: f64,
    issued: HashMap<usize, V>,
    in_flight: Option<InFlight>,
    lease: baal_service_runtime::ledger::OwnerLease,
    deps: BaalScaledNoControllerDeps,
    transport: Option<Rc<OrderTransport>>,
}

pub struct BaalScaledNoController {
    inner: Rc<RefCell<CtrlInner>>,
}

fn basic(inner: &Rc<RefCell<CtrlInner>>) -> Result<V, String> {
    let g = inner.borrow();
    need(!g.closed && !g.failed && (g.deps.can_operate)(), "STOPPED_OR_FAILED")?;
    g.lease.assert_ownership().map_err(|e| e.message)?;
    let l = (g.deps.assert_account_lease)();
    need(
        get(&l, "ownerId") == get(&g.scope, "ownerId")
            && get(&l, "tournamentId") == get(&g.scope, "tournamentId")
            && get(&l, "leaseId").as_str().is_some_and(|s| !s.is_empty()),
        "ACCOUNT_LEASE",
    )?;
    Ok(l)
}

fn append_event(inner: &Rc<RefCell<CtrlInner>>, input: &V) -> Result<(), String> {
    basic(inner)?;
    let file = inner.borrow().file.clone();
    let (old, ino, birthtime, size, bytes_hash, code) = {
        let g = inner.borrow();
        let old = std::fs::read(&g.file).map_err(|e| e.to_string())?;
        let stat = std::fs::metadata(&g.file).map_err(|e| e.to_string())?;
        use std::os::unix::fs::MetadataExt;
        (
            old,
            stat.ino(),
            stat.created()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0),
            g.size,
            g.bytes_hash.clone(),
            g.code.clone(),
        )
    };
    {
        let g = inner.borrow();
        need(
            g.ino == ino
                && g.birthtime_ms == birthtime
                && old.len() as u64 == size
                && sha_bytes(&old) == bytes_hash
                && same(&identity().unwrap_or(V::Null), &code),
            "SOURCE_CHANGED",
        )?;
    }
    let next = reduce(inner.borrow().state.clone(), input)?;
    let bound_ok =
        size + js_stringify(&next).len() as u64 + js_stringify(input).len() as u64;
    need(bound_ok < 128 * 1024 * 1024, "JOURNAL_BOUND")?;
    let event = json!({"codeIdentity": code, "input": input});
    let commit_result = inner.borrow_mut().journal.commit(&next, &event);
    match commit_result {
        Ok(_) => {
            basic(inner)?;
            let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
            need(
                bytes.len() >= old.len() && bytes[..old.len()] == old[..],
                "PREFIX_CHANGED",
            )?;
            let mut g = inner.borrow_mut();
            g.state = Some(next);
            g.size = bytes.len() as u64;
            g.bytes_hash = sha_bytes(&bytes);
            Ok(())
        }
        Err(e) => {
            inner.borrow_mut().failed = true;
            Err(if e.is_empty() { "COMMIT".into() } else { e })
        }
    }
}

/// openBaalScaledNoController({dir, scope, allocation, activation, dispatch,
/// rawReceiptOf, canOperate, assertAccountLease, installFundingReservation,
/// getGlobalRisk, getEvidence, now})
pub fn open_baal_scaled_no_controller(
    dir: &Path,
    scope: &V,
    allocation: &V,
    activation: &V,
    deps: BaalScaledNoControllerDeps,
) -> Result<Rc<BaalScaledNoController>, String> {
    need(dir.is_absolute(), "DEPENDENCIES")?;
    let code = identity()?;
    configuration(scope, allocation, activation, &code)?;
    let lease = acquire_owner_lease(
        dir,
        get(scope, "ownerId").as_str().unwrap_or(""),
        get(scope, "runId").as_str().unwrap_or(""),
        "baal-scaled-no",
        (deps.now)(),
    )
    .map_err(|e| e.message)?;
    let file = dir.join("baal-scaled-no-journal.jsonl");
    let open_result: Result<(Journal, Option<V>, u64, String, u64, f64), String> = (|| {
        basic_file(&file)?;
        let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
        need(bytes.is_empty() || *bytes.last().unwrap() == 10, "PARTIAL_JOURNAL")?;
        let journal = Journal::open(&file)?;
        let mut state: Option<V> = None;
        for line in String::from_utf8_lossy(&bytes).split('\n').filter(|l| !l.is_empty()) {
            let r: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            need(
                same(get(get(&r, "event"), "codeIdentity"), &code),
                "CODE_CHANGED",
            )?;
            state = Some(reduce(state, get(get(&r, "event"), "input"))?);
            need(same(state.as_ref().unwrap(), get(&r, "state")), "REPLAY")?;
        }
        let size = bytes.len() as u64;
        let bytes_hash = sha_bytes(&bytes);
        let stat = std::fs::metadata(&file).map_err(|e| e.to_string())?;
        use std::os::unix::fs::MetadataExt;
        Ok((
            journal,
            state,
            size,
            bytes_hash,
            stat.ino(),
            stat.created()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0),
        ))
    })();
    let (journal, state, size, bytes_hash, ino, birthtime) = match open_result {
        Ok(v) => v,
        Err(e) => {
            let mut lease = lease;
            let _ = lease.release();
            return Err(e);
        }
    };
    let inner = Rc::new(RefCell::new(CtrlInner {
        scope: scope.clone(),
        allocation: allocation.clone(),
        activation: activation.clone(),
        code,
        file,
        journal,
        state,
        closed: false,
        failed: false,
        busy: false,
        size,
        bytes_hash,
        ino,
        birthtime_ms: birthtime,
        issued: HashMap::new(),
        in_flight: None,
        lease,
        deps,
        transport: None,
    }));
    // First run: initialize; else verify immutable configuration.
    {
        if inner.borrow().state.is_none() {
            append_event(
                &inner,
                &json!({
                    "kind": "initialize",
                    "scope": scope,
                    "allocation": allocation,
                    "activation": activation,
                    "at": iso((inner.borrow().deps.now)()),
                }),
            )?;
        } else {
            let g = inner.borrow();
            let st = g.state.clone().unwrap();
            need(
                same(get(&st, "scope"), scope)
                    && same(get(&st, "allocation"), allocation)
                    && same(get(&st, "activation"), activation),
                "IMMUTABLE_CONFIGURATION",
            )?;
        }
    }
    // OrderTransport — the serialized dispatch wrapper.
    let transport = {
        let wrap_inner = inner.clone();
        let wrap: Box<crate::transport::Dispatch> =
            Box::new(move |spec, bd, bn| wrap_dispatch(&wrap_inner, spec, bd, bn));
        let inner_c = inner.clone();
        let can_write = Box::new(move || basic(&inner_c).is_ok());
        let inner_o = inner.clone();
        let owns_order = Box::new(move |id: &str| {
            inner_o
                .borrow()
                .state
                .as_ref()
                .and_then(|s| active(s))
                .and_then(|o| get(&o, "orderId").as_str().map(String::from))
                .as_deref()
                == Some(id)
        });
        let inner_vp = inner.clone();
        let validate_permit = Box::new(
            move |p: &V, spec: &V| -> Result<bool, OrderTransportError> {
                validate(&inner_vp, p, spec, false).map_err(|e| guard_err(&e))
            },
        );
        let inner_vn = inner.clone();
        let validate_network_permit = Box::new(
            move |p: &V, spec: &V| -> Result<bool, OrderTransportError> {
                validate(&inner_vn, p, spec, true).map_err(|e| guard_err(&e))
            },
        );
        let inner_vl = inner.clone();
        let validate_lease = Box::new(
            move |l: &V, _s: &V| -> Result<bool, OrderTransportError> {
                match basic(&inner_vl) {
                    Ok(l0) => Ok(same(l, &l0)),
                    Err(c) => Err(guard_err(&c)),
                }
            },
        );
        let inner_t = inner.clone();
        OrderTransport::new(OrderTransportOptions {
            dispatch: wrap,
            scope: crate::transport::scope_of(scope).map_err(|e| e.code)?,
            can_write,
            owns_order,
            validate_permit,
            validate_lease,
            validate_network_permit,
            now: Box::new(move || (inner_t.borrow().deps.now)() as i64),
            max_pages: 10,
        })
        .map_err(|e| e.code)?
    };
    inner.borrow_mut().transport = Some(Rc::new(transport));
    Ok(Rc::new(BaalScaledNoController { inner }))
}

fn basic_file(file: &Path) -> Result<(), String> {
    if !file.exists() {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(file)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn guard_err(code: &str) -> OrderTransportError {
    OrderTransportError {
        code: code.into(),
        status: None,
        api_code: None,
        outcome: "unknown".into(),
        may_have_executed: true,
        retry_same_request_only: false,
        retry_after_ms: 0.0,
        semantic_mismatch: false,
    }
}

// validate(p, spec, network) — admission-time and network-time permit gates.
fn validate(inner: &Rc<RefCell<CtrlInner>>, p: &V, spec: &V, network: bool) -> Result<bool, String> {
    basic(inner)?;
    reserved(inner)?;
    let (o, scope, file_at) = {
        let g = inner.borrow();
        let o = g.state.as_ref().and_then(|s| active(s));
        (o, g.scope.clone(), (g.deps.now)())
    };
    let o = o.ok_or("BAAL_SCALED_PERMIT")?;
    need(
        get(p, "intentId") == get(&o, "intentId") && get(p, "bodyHash") == get(&o, "bodyHash"),
        "PERMIT",
    )?;
    let operation = get(p, "operation").as_str().unwrap_or("").to_string();
    let evidence_op = if operation == "place" {
        format!("place_{}", get(get(&o, "body"), "action").as_str().unwrap_or(""))
    } else {
        "cancel".to_string()
    };
    let e = (inner.borrow().deps.get_evidence)(&json!({
        "operation": evidence_op,
        "intentId": get(&o, "intentId"),
        "orderId": get(&o, "orderId"),
        "marketId": get(&o, "marketId"),
        "exchangeId": get(get(&o, "body"), "exchangeId"),
    }))?;
    let g_risk = (inner.borrow().deps.get_global_risk)()?;
    let state = inner.borrow().state.clone().unwrap_or(V::Null);
    if operation == "place" {
        need(
            get(spec, "method").as_str() == Some("POST")
                && get(spec, "endpoint").as_str() == Some("/orders")
                && get(spec, "bodyJson") == get(&o, "bodyJson"),
            "BODY_CHANGED",
        )?;
        preflight(&state, &o, &e, &g_risk, file_at as i64, &state)?;
    } else {
        let mut e2 = e.clone();
        e2.as_object_mut()
            .map(|m| m.insert("globalRisk".into(), g_risk.clone()));
        cancel_check(&state, &e2, file_at as i64)?;
    }
    let l = basic(inner)?;
    let mut evidence = e.clone();
    if operation != "place" {
        evidence
            .as_object_mut()
            .map(|m| m.insert("globalRisk".into(), g_risk.clone()));
    }
    append_event(
        inner,
        &json!({
            "kind": if network { "network_fence" } else { "admitted" },
            "operation": get(p, "operation"),
            "evidence": evidence,
            "globalRisk": g_risk,
            "leaseIdentity": l.clone(),
            "leaseHash": payload_hash(&l)?,
            "at": iso((inner.borrow().deps.now)()),
        }),
    )?;
    if network {
        let latest = (inner.borrow().deps.get_evidence)(&json!({
            "operation": evidence_op,
            "intentId": get(&o, "intentId"),
            "orderId": get(&o, "orderId"),
            "marketId": get(&o, "marketId"),
            "exchangeId": get(get(&o, "body"), "exchangeId"),
        }))?;
        let risk_now = (inner.borrow().deps.get_global_risk)()?;
        let state = inner.borrow().state.clone().unwrap_or(V::Null);
        let o2 = active(&state).unwrap_or(V::Null);
        if operation == "place" {
            preflight(&state, &o2, &latest, &risk_now, (inner.borrow().deps.now)() as i64, &state)?;
        } else {
            let mut e2 = latest.clone();
            e2.as_object_mut()
                .map(|m| m.insert("globalRisk".into(), risk_now.clone()));
            cancel_check(&state, &e2, (inner.borrow().deps.now)() as i64)?;
        }
        reserved(inner)?;
        basic(inner)?;
    }
    Ok(true)
}

// reserved() — the installed slot must still match the current projection.
fn reserved(inner: &Rc<RefCell<CtrlInner>>) -> Result<(), String> {
    let g = inner.borrow();
    let s = g.state.clone().unwrap_or(V::Null);
    let r = get(&s, "reservation").clone();
    let alloc = g.allocation.clone();
    let now = (g.deps.now)();
    need(
        get(&r, "reservationHash") == get(&reservation(&s)?, "reservationHash")
            && get(&r, "allocationHash").as_str() == Some(payload_hash(&alloc)?.as_str())
            && hex(get(&r, "globalLedgerHash"))
            && instant_v(get(&r, "confirmedAt"))? <= now as i64
            && now as i64 - instant_v(get(&r, "confirmedAt"))? <= 20000,
        "RESERVATION_STALE",
    )?;
    Ok(())
}

// wrap(spec, beforeDispatch, beforeNetwork) — the dispatch the transport sees.
fn wrap_dispatch(
    inner: &Rc<RefCell<CtrlInner>>,
    spec: &V,
    bd: Option<&Guard<'_>>,
    bn: Option<&Guard<'_>>,
) -> Result<V, OrderTransportError> {
    let o = {
        let g = inner.borrow();
        g.state.as_ref().and_then(|s| active(s)).unwrap_or(V::Null)
    };
    let bd_ptr = bd.map(|g| g as *const Guard as *const () as usize).unwrap_or(0);
    let bn_ptr = bn.map(|g| g as *const Guard as *const () as usize).unwrap_or(0);
    {
        let mut g = inner.borrow_mut();
        g.issued.insert(
            bd_ptr,
            json!({
                "method": get(spec, "method"),
                "endpoint": get(spec, "endpoint"),
                "bodyJson": get(spec, "bodyJson"),
                "beforeNetworkPtr": bn_ptr,
            }),
        );
        g.in_flight = Some(InFlight {
            method: get(spec, "method").as_str().unwrap_or("").to_string(),
            endpoint: get(spec, "endpoint").as_str().unwrap_or("").to_string(),
        });
    }
    let capture = |inner: &Rc<RefCell<CtrlInner>>, response: &V| -> Result<(), String> {
        let raw_r = (inner.borrow().deps.raw_receipt_of)(
            response,
            &json!({
                "method": get(spec, "method"),
                "requestBodyHash": if get(spec, "method").as_str() == Some("POST") {
                    get(&o, "bodyHash").clone()
                } else {
                    V::Null
                },
            }),
        )?;
        let r = raw(
            &raw_r,
            get(spec, "method").as_str().unwrap_or(""),
            get(spec, "endpoint").as_str().unwrap_or(""),
        )?;
        let (d, _) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
        need(
            get(&r, "status") == get(response, "status")
                && instant_v(get(&r, "requestStartedAt"))? >= instant_v(get(response, "requestStartedAt"))?
                && instant_v(get(&r, "receivedAt"))? <= instant_v(get(response, "receivedAt"))?
                && same(&d, get(response, "data")),
            "RAW_RESPONSE_BINDING",
        )?;
        append_event(
            inner,
            &json!({"kind": "wire", "receipt": r, "at": iso((inner.borrow().deps.now)())}),
        )
    };
    let result = {
        let dispatch = inner.borrow().deps.dispatch.clone();
        dispatch(spec, bd, bn)
    };
    let out = match result {
        Ok(r) => match capture(inner, &r) {
            Ok(()) => Ok(r),
            Err(c) => Err(guard_err(&c)),
        },
        Err(e) => {
            if let Some(wr) = &e.wire_receipt {
                capture(inner, wr).map_err(|c| guard_err(&c))?;
            }
            Err(OrderTransportError {
                code: e.message,
                status: None,
                api_code: None,
                outcome: "unknown".into(),
                may_have_executed: true,
                retry_same_request_only: false,
                retry_after_ms: 0.0,
                semantic_mismatch: false,
            })
        }
    };
    {
        let mut g = inner.borrow_mut();
        g.issued.remove(&bd_ptr);
        g.in_flight = None;
    }
    out
}



impl BaalScaledNoController {
    fn run<T>(&self, f: impl FnOnce(&Self) -> Result<T, String>) -> Result<T, String> {
        {
            let mut g = self.inner.borrow_mut();
            need(!g.busy, "BUSY")?;
            g.busy = true;
        }
        let out = f(self);
        self.inner.borrow_mut().busy = false;
        out
    }

    fn reserve(&self) -> Result<V, String> {
        let state = self.inner.borrow().state.clone().unwrap_or(V::Null);
        let proj = reservation(&state)?;
        let receipt = (self.inner.borrow().deps.install_funding_reservation)(&proj);
        match receipt {
            Ok(receipt) => {
                append_event(
                    &self.inner,
                    &json!({"kind": "reservation", "receipt": receipt, "at": iso((self.inner.borrow().deps.now)())}),
                )?;
                Ok(receipt)
            }
            Err(e) => {
                self.inner.borrow_mut().failed = true;
                Err(e)
            }
        }
    }

    /// status()
    pub fn status(&self) -> V {
        let g = self.inner.borrow();
        let state = g.state.clone().unwrap_or(V::Null);
        let o = active(&state);
        let inventory: Map<String, V> = get(&state, "inventory")
            .as_object()
            .map(|inv| {
                inv.iter()
                    .map(|(ex, x)| {
                        (
                            ex.clone(),
                            json!({
                                "marketId": get(get(x, "proof"), "marketId"),
                                "quantity": get(get(x, "proof"), "remainingOwnedQuantity"),
                                "cost": get(get(x, "proof"), "remainingOwnedCost"),
                                "cumulativeRealizedPnl": get(get(x, "proof"), "cumulativeRealizedPnl"),
                                "proofHash": get(get(x, "proof"), "verificationHash"),
                                "eventId": get(get(x, "package"), "eventId"),
                            }),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let realized: BigInt = get(&state, "inventory")
            .as_object()
            .map(|inv| {
                inv.values().fold(BigInt::from(0), |n, x| {
                    n + get(get(x, "proof"), "cumulativeRealizedPnl")
                        .as_str()
                        .and_then(|s| amount(s).ok())
                        .map(BigInt::from)
                        .unwrap_or_default()
                })
            })
            .unwrap_or_default();
        json!({
            "mode": get(&state, "mode"),
            "ownerId": get(&g.scope, "ownerId"),
            "runId": get(&g.scope, "runId"),
            "tournamentId": get(&g.scope, "tournamentId"),
            "allocationHash": payload_hash(&g.allocation).unwrap_or_default(),
            "entryExpiresAt": get(&g.activation, "entryExpiresAt"),
            "sequence": g.journal.sequence,
            "hash": g.journal.previous,
            "phase": o.as_ref().map(|o| get(o, "phase").clone()).unwrap_or(json!("ready")),
            "orderCount": get(&state, "orders").as_array().map(|a| a.len()).unwrap_or(0),
            "actualOrderCount": get(&state, "orders").as_array().map(|a| {
                a.iter().filter(|v| !get(v, "orderId").is_null()).count()
            }).unwrap_or(0),
            "abandonedIntentCount": get(&state, "orders").as_array().map(|a| {
                a.iter().filter(|v| get(v, "phase").as_str() == Some("abandoned")).count()
            }).unwrap_or(0),
            "intent": match &o {
                Some(o) => json!({
                    "intentId": get(o, "intentId"),
                    "orderId": get(o, "orderId"),
                    "action": get(get(o, "body"), "action"),
                    "marketId": get(o, "marketId"),
                    "exchangeId": get(get(o, "body"), "exchangeId"),
                    "body": get(o, "body"),
                    "bodyHash": get(o, "bodyHash"),
                    "createdAt": get(o, "createdAt"),
                    "packageLegs": get(get(o, "package"), "legs"),
                    "cancelPending": get(o, "cancelPending"),
                    "actualFetchCount": get(o, "fetchStarts").as_array().map(|a| a.len()).unwrap_or(0),
                    "lastActualFetchStartAt": get(o, "fetchStarts").as_array().and_then(|a| a.last()).map(|f| get(f, "requestStartedAt").clone()).unwrap_or(V::Null),
                }),
                None => V::Null,
            },
            "inventory": V::Object(inventory),
            "realizedPnl": baal_paper::fixed::money(&realized),
            "reservation": reservation(&state).unwrap_or(V::Null),
            "failed": g.failed,
            "closed": g.closed,
            "busy": g.busy,
            "additionalApiClients": 0,
        })
    }

    /// authorizes(spec) — was this exact spec admitted through the wrapper?
    pub fn authorizes(
        &self,
        spec: &V,
        before_dispatch: Option<&Guard<'_>>,
        before_network: Option<&Guard<'_>>,
    ) -> bool {
        let g = self.inner.borrow();
        let bd_ptr = before_dispatch
            .map(|x| x as *const Guard as *const () as usize)
            .unwrap_or(0);
        let bn_ptr = before_network
            .map(|x| x as *const Guard as *const () as usize)
            .unwrap_or(0);
        let p = g.issued.get(&bd_ptr).cloned();
        let t = match &g.transport {
            Some(t) => t,
            None => return false,
        };
        basic(&self.inner).is_ok()
            && p.is_some()
            && get(p.as_ref().unwrap(), "method") == get(spec, "method")
            && get(p.as_ref().unwrap(), "endpoint") == get(spec, "endpoint")
            && get(p.as_ref().unwrap(), "bodyJson") == get(spec, "bodyJson")
            && get(p.as_ref().unwrap(), "beforeNetworkPtr") == &json!(bn_ptr as u64)
            && t.authorize_request(spec).unwrap_or(false)
    }

    /// onFetchStart(meta) — record an actual scoped fetch start.
    pub fn on_fetch_start(&self, meta: &V) -> Result<bool, String> {
        if get(meta, "method").as_str() == Some("GET") || self.inner.borrow().in_flight.is_none() {
            return Ok(false);
        }
        let result: Result<(), String> = (|| {
            basic(&self.inner)?;
            let (o, fences_last, scope, file) = {
                let g = self.inner.borrow();
                let s = g.state.clone().unwrap_or(V::Null);
                let o = active(&s).unwrap_or(V::Null);
                let f = get(&o, "fences").as_array().and_then(|a| a.last()).cloned().unwrap_or(V::Null);
                (o, f, g.scope.clone(), g.file.clone())
            };
            let _ = file;
            let in_flight = self.inner.borrow().in_flight.clone().unwrap();
            need(
                !fences_last.is_null()
                    && get(meta, "method").as_str() == Some(in_flight.method.as_str())
                    && get(meta, "endpoint").as_str() == Some(in_flight.endpoint.as_str())
                    && get(meta, "requestBodyHash")
                        == &(if in_flight.method == "POST" {
                            get(&o, "bodyHash").clone()
                        } else {
                            V::Null
                        }),
                "FETCH_UNBOUND",
            )?;
            let mut start = meta.clone();
            if let V::Object(m) = &mut start {
                m.insert("kind".into(), json!("place"));
                m.insert("ownerId".into(), get(&scope, "ownerId").clone());
                m.insert("runId".into(), get(&scope, "runId").clone());
                m.insert("tournamentId".into(), get(&scope, "tournamentId").clone());
                m.insert("intentId".into(), get(&o, "intentId").clone());
                m.insert("idempotencyKey".into(), get(get(&o, "body"), "idempotencyKey").clone());
                m.insert("requestBodyJson".into(), get(&o, "bodyJson").clone());
                m.insert("requestBodyHash".into(), get(&o, "bodyHash").clone());
                m.insert("leaseIdentity".into(), get(&fences_last, "leaseIdentity").clone());
                m.insert("leaseHash".into(), get(&fences_last, "leaseHash").clone());
            }
            append_event(
                &self.inner,
                &json!({"kind": "fetch_start", "start": start, "at": iso((self.inner.borrow().deps.now)())}),
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => Ok(true),
            Err(e) => {
                self.inner.borrow_mut().failed = true;
                Err(e)
            }
        }
    }

    /// journalSource()
    pub fn journal_source(&self) -> Result<V, String> {
        let g = self.inner.borrow();
        let bytes = std::fs::read(&g.file).map_err(|e| e.to_string())?;
        let first_len = bytes.iter().position(|b| *b == 10).map(|i| i + 1).unwrap_or(0);
        let first = &bytes[..first_len.min(bytes.len())];
        let row: V = serde_json::from_slice(first).map_err(|e| e.to_string())?;
        Ok(json!({
            "journalFile": g.file.to_string_lossy(),
            "sequence": g.journal.sequence,
            "hash": g.journal.previous,
            "rootPrefix": {
                "bytes": first.len(),
                "sha256": sha_bytes(first),
                "sequence": get(&row, "sequence"),
                "hash": get(&row, "hash"),
            },
            "headPrefix": {
                "bytes": g.size,
                "sha256": g.bytes_hash,
                "sequence": g.journal.sequence,
                "hash": g.journal.previous,
            },
            "record": reservation(g.state.as_ref().unwrap_or(&V::Null))?,
            "status": self.status(),
        }))
    }

    /// orderBinding(id)
    pub fn order_binding(&self, id: &str) -> V {
        let status = self.status();
        if get(get(&status, "intent"), "orderId").as_str() == Some(id) {
            get(&status, "intent").clone()
        } else {
            V::Null
        }
    }

    /// prepareBuy() / prepareSell(ex, {quantity})
    pub fn prepare_buy(&self) -> Result<V, String> {
        self.prepare("buy", None, None)
    }
    pub fn prepare_sell(&self, ex: &str, quantity: Option<i64>) -> Result<V, String> {
        self.prepare("sell", Some(ex), quantity)
    }
    fn prepare(&self, action: &str, ex: Option<&str>, quantity: Option<i64>) -> Result<V, String> {
        self.run(|this| {
            basic(&this.inner)?;
            need(
                this.inner.borrow().state.as_ref().and_then(|s| active(s)).is_none(),
                "UNKNOWN_OR_OPEN_INTENT",
            )?;
            this.reserve()?;
            let scope_v = this.inner.borrow().scope.clone();
            let market_id = ex
                .and_then(|ex| {
                    get(&scope_v, "exchanges")
                        .as_array()
                        .and_then(|a| {
                            a.iter()
                                .find(|m| get(m, "exchangeId").as_str() == Some(ex))
                                .cloned()
                        })
                        .map(|m| get(&m, "marketId").clone())
                })
                .unwrap_or(V::Null);
            let e = (this.inner.borrow().deps.get_evidence)(&json!({
                "operation": format!("prepare_{action}"),
                "exchangeId": ex.map(|x| json!(x)).unwrap_or(V::Null),
                "marketId": market_id,
            }))?;
            let g_risk = (this.inner.borrow().deps.get_global_risk)()?;
            append_event(
                &this.inner,
                &json!({
                    "kind": "prepared",
                    "action": action,
                    "exchangeId": ex,
                    "quantity": quantity,
                    "intentId": format!("baal-scaled-{}", Uuid::new_v4()),
                    "idempotencyKey": Uuid::new_v4().to_string(),
                    "evidence": e,
                    "globalRisk": g_risk,
                    "at": iso((this.inner.borrow().deps.now)()),
                }),
            )?;
            this.reserve()?;
            Ok(this.status())
        })
    }

    /// dispatchPrepared() / cancelExpired()
    pub fn dispatch_prepared(&self) -> Result<V, String> {
        self.send("place")
    }
    pub fn cancel_expired(&self) -> Result<V, String> {
        self.send("cancel")
    }
    fn send(&self, operation: &str) -> Result<V, String> {
        self.run(|this| {
            basic(&this.inner)?;
            let o = this
                .inner
                .borrow()
                .state
                .as_ref()
                .and_then(|s| active(s))
                .ok_or("BAAL_SCALED_UNKNOWN_BLOCKS_REPLACEMENT")?;
            need(
                if operation == "place" {
                    get(&o, "phase").as_str() == Some("prepared")
                } else {
                    !get(&o, "orderId").is_null() && get(&o, "cancelPending") == &json!(false)
                },
                "UNKNOWN_BLOCKS_REPLACEMENT",
            )?;
            this.reserve()?;
            let result = (|| -> Result<V, OrderTransportError> {
                let permit = json!({
                    "intentId": get(&o, "intentId"),
                    "bodyHash": get(&o, "bodyHash"),
                    "operation": operation,
                });
                let l = basic(&this.inner).map_err(|c| OrderTransportError {
                    code: c,
                    status: None,
                    api_code: None,
                    outcome: "unknown".into(),
                    may_have_executed: true,
                    retry_same_request_only: false,
                    retry_after_ms: 0.0,
                    semantic_mismatch: false,
                })?;
                let authority = json!({"permit": permit, "lease": l});
                let transport = this.inner.borrow().transport.clone();
                if operation == "place" {
                    transport.unwrap().place(get(&o, "body"), &authority)
                } else {
                    transport.unwrap().cancel(get(&o, "orderId"), &authority)
                }
            })();
            if let Err(e) = result {
                if this.inner.borrow().failed {
                    return Err("BAAL_SCALED_DURABLE_FAILURE".into());
                }
                if (this.inner.borrow().deps.can_operate)() {
                    let code = if !e.code.is_empty()
                        && e.code.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
                    {
                        e.code.clone()
                    } else {
                        "REQUEST_UNCONFIRMED".to_string()
                    };
                    append_event(
                        &this.inner,
                        &json!({"kind": "error", "code": code, "at": iso((this.inner.borrow().deps.now)())}),
                    )?;
                }
                return Err(e.code);
            }
            Ok(this.status())
        })
    }

    /// abandonPrepared()
    pub fn abandon_prepared(&self) -> Result<V, String> {
        self.run(|this| {
            basic(&this.inner)?;
            need(this.inner.borrow().in_flight.is_none(), "ABANDON_IN_FLIGHT")?;
            append_event(
                &this.inner,
                &json!({"kind": "abandoned_prepared", "at": iso((this.inner.borrow().deps.now)())}),
            )?;
            this.reserve()?;
            Ok(this.status())
        })
    }

    /// refreshReservation()
    pub fn refresh_reservation(&self) -> Result<V, String> {
        self.run(|this| {
            basic(&this.inner)?;
            this.reserve()?;
            Ok(this.status())
        })
    }

    /// fundingReservation() / fundingReservationStatus()
    pub fn funding_reservation(&self) -> Result<V, String> {
        reservation(&self.inner.borrow().state.clone().unwrap_or(V::Null))
    }
    pub fn funding_reservation_status(&self) -> Result<V, String> {
        let g = self.inner.borrow();
        let state = g.state.clone().unwrap_or(V::Null);
        Ok(json!({
            "record": reservation(&state)?,
            "receipt": get(&state, "reservation"),
        }))
    }

    /// inventory(ex)
    pub fn inventory(&self, ex: &str) -> V {
        self.inner
            .borrow()
            .state
            .as_ref()
            .map(|s| get(get(s, "inventory"), ex).clone())
            .unwrap_or(V::Null)
    }

    /// reconcile(e)
    pub fn reconcile(&self, e: &V) -> Result<V, String> {
        self.run(|this| {
            basic(&this.inner)?;
            let g = (this.inner.borrow().deps.get_global_risk)()?;
            let state = this.inner.borrow().state.clone().unwrap_or(V::Null);
            risk(&state, &g, (this.inner.borrow().deps.now)() as i64, true)?;
            let mut evidence = e.clone();
            evidence
                .as_object_mut()
                .map(|m| m.insert("globalRisk".into(), g));
            append_event(
                &this.inner,
                &json!({"kind": "reconciled", "evidence": evidence, "at": iso((this.inner.borrow().deps.now)())}),
            )?;
            this.reserve()?;
            Ok(this.status())
        })
    }

    /// close()
    pub fn close(&self) -> Result<(), String> {
        let mut g = self.inner.borrow_mut();
        need(!g.busy, "BUSY")?;
        if g.closed {
            return Ok(());
        }
        g.closed = true;
        let _ = g.lease.release();
        Ok(())
    }
}

trait Pipe: Sized {
    fn pipe<R>(self, f: impl FnOnce(Self) -> R) -> R {
        f(self)
    }
}
impl Pipe for V {}
