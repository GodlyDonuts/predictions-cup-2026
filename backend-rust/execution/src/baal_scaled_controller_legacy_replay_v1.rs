// execution/baal-scaled-controller-legacy-replay-v1.mjs — sealed-seed replay
// of the durable single-intent controller journal. Lines 1-103 of the JS are
// byte-identical to baal-scaled-no-controller-v1.mjs, so the shared helper /
// reducer machinery below is a verbatim copy of that port
// (baal_scaled_no_controller_v1.rs) through the end of `reduce`; the
// replay-specific tail is appended at the bottom of this file.
//
// Port notes (shared chunk, from baal_scaled_no_controller_v1.rs):
// - `need` throws BAAL_SCALED_<code>; uncaught helper errors (instant/amount/
//   price, verifier returns) propagate as their own messages, as in JS.
// - async dispatch/rawReceiptOf -> synchronous Fn traits (same model as
//   paired_rest_controller.rs). `issued` WeakMap -> pointer-keyed HashMap.
// - `sync(fn)` checks the return value is truthy (no promise possible).
// - Object.entries/WeakMap ordering follows serde_json insertion order.
// - JS `Number(limit)` on a money string -> f64 (price field is numeric).
// - `'ENTRY_'+result.reason` keeps the doubled BAAL_SCALED_ENTRY_ prefix.
use crate::core::{canonical_payload, payload_hash, validate_execution_state};
use crate::frozen::collect_frozen_paper_files;
use crate::gates::normalize_official_fill_tick;
use crate::ledger::acquire_owner_lease;
use crate::no_rest_multishare_reconciliation_v1::verify_scoped_rest_no_multishare;
use crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::receipts::parse_number_lexemes;
use crate::transport::{Guard, OrderTransport, OrderTransportError, OrderTransportOptions};
use baal_service_runtime::contract::{instant_ns, ms_to_iso};
use baal_service_runtime::fixed::{amount, instant, money, price, TICK};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use baal_service_runtime::ledger::OwnerLease;
use baal_service_runtime::market::normalize_book;
use serde_json::{json, Map, Value as V};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
fn hex(x: &V) -> bool {
    x.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn sha(v: &V) -> String {
    sha256(js_stringify(v).as_bytes())
}
fn sha_str(s: &str) -> String {
    sha256(s.as_bytes())
}
fn sha_bytes(b: &[u8]) -> String {
    sha256(b)
}
fn iso(n: f64) -> String {
    ms_to_iso(n)
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(payload_hash(a)? == payload_hash(b)?)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().ok_or_else(|| "INVALID_INSTANT".to_string())?)
}
fn num_str(v: &V) -> String {
    js_stringify(v)
}
fn sorted_by_id(a: &[V]) -> Vec<V> {
    let mut x: Vec<V> = a.to_vec();
    x.sort_by(|p, q| {
        get(p, "id")
            .as_f64()
            .partial_cmp(&get(q, "id").as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    x
}
fn active_index(s: &V) -> Option<usize> {
    get(s, "orders").as_array().and_then(|orders| {
        orders
            .iter()
            .position(|o| get(o, "proof").is_null() && get(o, "phase").as_str() != Some("abandoned"))
    })
}
fn active(s: &V) -> Option<V> {
    active_index(s).and_then(|i| get(s, "orders").as_array().map(|a| a[i].clone()))
}
fn identity() -> Result<V, String> {
    collect_frozen_paper_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(".."),
        &["execution/baal-scaled-no-controller-v1.mjs"],
    )
}
pub fn baal_scaled_no_controller_identity() -> Result<V, String> {
    identity()
}
fn truthy_sync(v: V) -> Result<V, String> {
    // sync(fn): the callback must return a non-promise truthy value.
    need(js_truthy(Some(&v)), "SYNC_LOCAL_CALLBACK")?;
    Ok(v)
}
fn own_account(a: &V, ex: &str) -> V {
    let scoped = get(a, "scopedOrdersPagesByExchange")
        .get(ex)
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or_else(|| get(a, "scopedOrdersPages").clone());
    let mut out = Map::new();
    out.insert("cash".into(), get(a, "cash").clone());
    out.insert("positions".into(), get(a, "positions").clone());
    out.insert("openOrdersPages".into(), get(a, "openOrdersPages").clone());
    out.insert("scopedOrdersPages".into(), scoped);
    V::Object(out)
}
fn raw(r: &V, method: &str, endpoint: &str) -> Result<V, String> {
    need(
        get(r, "method").as_str() == Some(method)
            && get(r, "endpoint").as_str() == Some(endpoint)
            && js_int(get(r, "status")).is_some()
            && get(r, "bodyText").as_str().is_some_and(|s| s.len() <= 8 * 1024 * 1024)
            && get(r, "sourceBodyHash").as_str() == Some(sha_str(get(r, "bodyText").as_str().unwrap_or("")).as_str())
            && get(r, "fetchInvocationId").as_str().is_some(),
        "RAW_RECEIPT",
    )?;
    need(
        instant_v(get(r, "requestStartedAt"))? <= instant_v(get(r, "receivedAt"))?,
        "RAW_CLOCK",
    )?;
    let mut out = Map::new();
    for k in [
        "method",
        "endpoint",
        "status",
        "bodyText",
        "requestStartedAt",
        "receivedAt",
        "sourceBodyHash",
        "fetchInvocationId",
    ] {
        out.insert(k.into(), get(r, k).clone());
    }
    if let Some(h) = r.get("requestBodyHash").filter(|v| !v.is_null()) {
        out.insert("requestBodyHash".into(), h.clone());
    }
    Ok(V::Object(out))
}
fn configuration(scope: &V, allocation: &V, activation: &V, code: &V) -> Result<(), String> {
    need(
        sget(scope, "ownerId") == Some("Codex")
            && sget(scope, "runId").is_some_and(|s| !s.is_empty())
            && scope
                .get("exchanges")
                .and_then(V::as_array)
                .is_some_and(|x| {
                    !x.is_empty()
                        && x.iter()
                            .map(|e| sget(e, "exchangeId").unwrap_or(""))
                            .collect::<BTreeSet<_>>()
                            .len()
                            == x.len()
                }),
        "SCOPE",
    )?;
    need(
        js_int(get(allocation, "schemaVersion")) == Some(1)
            && get(allocation, "ownerId") == get(scope, "ownerId")
            && get(allocation, "runId") == get(scope, "runId")
            && get(allocation, "tournamentId") == get(scope, "tournamentId")
            && get(allocation, "capital").as_str() == Some("1000.000000")
            && hex(get(allocation, "humanAllocationSourceHash")),
        "ALLOCATION",
    )?;
    let opt_amount = |k: &str, d: &str| -> Result<i128, String> {
        let v = match allocation.get(k) {
            Some(x) if !x.is_null() => x.clone(),
            _ => json!(d),
        };
        amount(v.as_str().ok_or_else(|| "INVALID".to_string())?)
    };
    for (k, max) in [
        ("maximumOrderCost", 25000000i128),
        ("maximumMarketCost", 100000000i128),
        ("maximumEventCost", 200000000i128),
    ] {
        let v = opt_amount(k, &money(max))?;
        need(v > 0 && v <= max, "ALLOCATION_CEILING")?;
    }
    let opt_int = |k: &str, d: i64| -> Option<i64> {
        match allocation.get(k) {
            Some(x) if !x.is_null() => js_int(x),
            _ => Some(d),
        }
    };
    need(
        opt_int("maximumSharesPerOrder", 100).is_some_and(|v| v > 0 && v <= 100),
        "SHARE_CEILING",
    )?;
    need(
        opt_int("maximumActiveMarkets", 20).is_some_and(|v| v > 0 && v <= 20),
        "ACTIVE_MARKET_CEILING",
    )?;
    need(
        js_int(get(activation, "schemaVersion")) == Some(1)
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
            && instant_v(get(activation, "entryExpiresAt"))? - instant_v(get(activation, "startedAt"))?
                <= 3600000
            && instant_v(get(allocation, "recordedAt"))? <= instant_v(get(activation, "startedAt"))?,
        "ACTIVATION_WINDOW",
    )?;
    Ok(())
}

fn reservation(s: &V) -> Result<V, String> {
    let mut owned: Vec<V> = Vec::new();
    let mut reservations: Vec<V> = Vec::new();
    let run_id = sget(get(s, "scope"), "runId").unwrap_or("").to_string();
    let add = |list: &mut Vec<V>, owned_list: bool, leg: &V, event_id: &V, n: i128, suffix: &str| -> Result<(), String> {
        if n > 0 {
            let mut row = Map::new();
            row.insert(
                "exposureId".into(),
                json!(format!(
                    "scaled:{}:{}:{}",
                    run_id,
                    sget(leg, "exchangeId").unwrap_or(""),
                    suffix
                )),
            );
            row.insert("marketId".into(), get(leg, "marketId").clone());
            row.insert("eventId".into(), event_id.clone());
            row.insert(
                if owned_list { "nominalCost" } else { "maximumNominalCost" }.into(),
                json!(money(n)),
            );
            list.push(V::Object(row));
        }
        Ok(())
    };
    if let Some(inv) = get(s, "inventory").as_object() {
        for (ex, slot) in inv {
            let q = js_int(get(get(slot, "proof"), "remainingOwnedQuantity"));
            let q = match q {
                Some(0) | None => continue,
                Some(v) => v,
            };
            let legs = get(get(slot, "package"), "legs").as_array().cloned().unwrap_or_default();
            let first = legs
                .iter()
                .find(|l| sget(l, "exchangeId") == Some(ex.as_str()))
                .cloned()
                .unwrap_or(V::Null);
            let other = legs
                .iter()
                .find(|l| sget(l, "exchangeId") != Some(ex.as_str()))
                .cloned()
                .unwrap_or(V::Null);
            let cost = amount(
                get(get(slot, "proof"), "remainingOwnedCost")
                    .as_str()
                    .ok_or_else(|| "INVALID".to_string())?,
            )?;
            add(&mut owned, true, &first, get(get(slot, "package"), "eventId"), cost, "owned")?;
            add(
                &mut reservations,
                false,
                &other,
                get(get(slot, "package"), "eventId"),
                990000 * q as i128 - cost,
                &format!("hedge-for-{}", ex),
            )?;
        }
    }
    if let Some(o) = active(s) {
        if sget(get(&o, "body"), "action") == Some("buy") {
            let legs = get(get(&o, "package"), "legs").as_array().cloned().unwrap_or_default();
            let ex = sget(get(&o, "body"), "exchangeId").unwrap_or("");
            let first = legs
                .iter()
                .find(|l| sget(l, "exchangeId") == Some(ex))
                .cloned()
                .unwrap_or(V::Null);
            let other = legs
                .iter()
                .find(|l| sget(l, "exchangeId") != Some(ex))
                .cloned()
                .unwrap_or(V::Null);
            let qty = js_int(get(&o, "body").get("quantity").unwrap_or(&V::Null)).unwrap_or(0);
            let entry = price(&num_str(&get(get(&o, "body"), "price").clone()))? * qty as i128;
            add(&mut reservations, false, &first, get(get(&o, "package"), "eventId"), entry, "entry")?;
            add(
                &mut reservations,
                false,
                &other,
                get(get(&o, "package"), "eventId"),
                990000 * qty as i128 - entry,
                &format!("hedge-for-{}", sget(&first, "exchangeId").unwrap_or("")),
            )?;
        }
    }
    let o = active(s);
    let mut body = Map::new();
    body.insert("schemaVersion".into(), json!(1));
    body.insert("kind".into(), json!("baal_scaled_no_full_nominal_reservation_v1"));
    body.insert("ownerId".into(), get(get(s, "scope"), "ownerId").clone());
    body.insert("runId".into(), get(get(s, "scope"), "runId").clone());
    body.insert("tournamentId".into(), get(get(s, "scope"), "tournamentId").clone());
    body.insert("allocationHash".into(), json!(payload_hash(get(s, "allocation"))?));
    body.insert("owned".into(), json!(owned));
    body.insert("reservations".into(), json!(reservations));
    body.insert(
        "unresolvedIntentIds".into(),
        o.as_ref()
            .map(|o| json!([get(&o, "intentId").clone()]))
            .unwrap_or(json!([])),
    );
    body.insert(
        "cashReserved".into(),
        json!(money(
            reservations
                .iter()
                .map(|r| amount(get(r, "maximumNominalCost").as_str().unwrap_or("")))
                .collect::<Result<Vec<i128>, String>>()?
                .iter()
                .sum::<i128>()
        )),
    );
    body.insert(
        "ownedPrincipal".into(),
        json!(money(
            owned
                .iter()
                .map(|r| amount(get(r, "nominalCost").as_str().unwrap_or("")))
                .collect::<Result<Vec<i128>, String>>()?
                .iter()
                .sum::<i128>()
        )),
    );
    let hash = payload_hash(&V::Object(body.clone()))?;
    body.insert("reservationHash".into(), json!(hash));
    Ok(V::Object(body))
}

pub fn project_baal_scaled_reservation(state: &V) -> Result<V, String> {
    need(
        get(state, "mode").as_str() == Some("baal_scaled_no_controller_v1"),
        "PROJECTION_MODE",
    )?;
    reservation(state)
}

fn risk(s: &V, g: &V, at: i64, cleanup: bool) -> Result<(), String> {
    let active_iid = active(s)
        .map(|o| get(&o, "intentId").clone())
        .unwrap_or(V::Null);
    let active_phase = active(s)
        .map(|o| get(&o, "phase").as_str().unwrap_or("").to_string())
        .unwrap_or_default();
    need(
        get(g, "ownerId") == get(get(s, "scope"), "ownerId")
            && get(g, "runId") == get(get(s, "scope"), "runId")
            && get(g, "tournamentId") == get(get(s, "scope"), "tournamentId")
            && get(g, "complete") == &V::Bool(true)
            && get(g, "eventScopeComplete") == &V::Bool(true)
            && get(g, "soleCollector") == &V::Bool(true)
            && get(g, "stopped") == &V::Bool(false)
            && hex(get(g, "sourceHash"))
            && instant_v(get(g, "at"))? <= at
            && at - instant_v(get(g, "at"))? <= 20000
            && get(g, "allocationHash").as_str()
                == Some(payload_hash(get(s, "allocation"))?.as_str())
            && get(g, "budgetInstalledHash")
                == get(get(s, "activation"), "budgetInstalledHash"),
        "CURRENT_GLOBAL_RISK",
    )?;
    let unresolved = get(g, "unresolvedIntentIds").clone();
    let unresolved_ok = unresolved
        .as_array()
        .is_some_and(|ids| ids.len() <= 1 && ids.iter().all(|x| x == &active_iid));
    need(
        unresolved_ok
            && (cleanup
                || get(g, "unknownAcceptance") == &V::Bool(false)
                || (unresolved.as_array().is_some_and(|ids| ids.len() == 1)
                    && ["prepared", "dispatching"].contains(&active_phase.as_str()))),
        "GLOBAL_UNKNOWN",
    )?;
    Ok(())
}

fn budget(s: &V, g: &V, next: &V, at: i64, cash: bool) -> Result<(), String> {
    risk(s, g, at, false)?;
    let old = reservation(s)?;
    need(
        get(g, "controllerReservationHash") == get(&old, "reservationHash")
            && hex(get(g, "globalLedgerHash"))
            && get(g, "globalLedgerHash") == get(get(s, "reservation"), "globalLedgerHash"),
        "GLOBAL_RESERVATION_BINDING",
    )?;
    let mut rows: Vec<V> = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    for (k, f) in [
        ("owned", "nominalCost"),
        ("reservations", "maximumNominalCost"),
        ("loans", "fullNominalPrincipal"),
    ] {
        need(g.get(k).is_some_and(V::is_array), "GLOBAL_EXPOSURES")?;
        for r in get(g, k).as_array().unwrap() {
            let eid = sget(r, "exposureId").unwrap_or("");
            need(
                !eid.is_empty()
                    && !seen.contains_key(eid)
                    && sget(r, "marketId").is_some()
                    && sget(r, "eventId").is_some_and(|s| !s.is_empty())
                    && amount(sget(r, f).ok_or_else(|| "INVALID".to_string())?)? >= 0
                    && get(r, "unknownAcceptance") != &V::Bool(true),
                "GLOBAL_EXPOSURE",
            )?;
            seen.insert(eid.to_string(), ());
            let mut row = Map::new();
            row.insert("kind".into(), json!(k));
            row.insert("value".into(), r.clone());
            row.insert(
                "cost".into(),
                V::Number(serde_json::Number::from(amount(sget(r, f).unwrap()).unwrap_or(0) as i64)),
            );
            rows.push(V::Object(row));
        }
    }
    for (k, f) in [("owned", "nominalCost"), ("reservations", "maximumNominalCost")] {
        for r in get(&old, k).as_array().cloned().unwrap_or_default() {
            let idx = rows.iter().position(|v| {
                get(v, "kind").as_str() == Some(k)
                    && sget(get(v, "value"), "exposureId") == sget(&r, "exposureId")
            });
            let idx = match idx {
                Some(i) if same(get(&rows[i], "value"), &r)? => i,
                _ => return need(false, "OWN_GLOBAL_EXPOSURE_MISSING"),
            };
            let _ = f;
            rows.remove(idx);
        }
    }
    for k in ["owned", "reservations"] {
        for r in get(next, k).as_array().cloned().unwrap_or_default() {
            let mut row = Map::new();
            row.insert("kind".into(), json!(k));
            row.insert("value".into(), r.clone());
            let f = if k == "owned" { "nominalCost" } else { "maximumNominalCost" };
            row.insert(
                "cost".into(),
                V::Number(serde_json::Number::from(
                    amount(sget(&r, f).ok_or_else(|| "INVALID".to_string())?).unwrap_or(0) as i64,
                )),
            );
            rows.push(V::Object(row));
        }
    }
    let mut total: i128 = 0;
    let mut reserved: i128 = 0;
    let mut markets: HashMap<String, i128> = HashMap::new();
    let mut events: HashMap<String, i128> = HashMap::new();
    let mut market_events: HashMap<String, String> = HashMap::new();
    for r in &rows {
        let v = get(r, "value");
        let market = sget(v, "marketId").unwrap_or("").to_string();
        let event = sget(v, "eventId").unwrap_or("").to_string();
        need(
            market_events.get(&market).map(|e| e == &event).unwrap_or(true),
            "MARKET_EVENT_CHANGED",
        )?;
        market_events.insert(market.clone(), event.clone());
        let cost = get(r, "cost").as_i64().unwrap_or(0) as i128;
        total += cost;
        if get(r, "kind").as_str() == Some("reservations") {
            reserved += cost;
        }
        *markets.entry(market).or_insert(0) += cost;
        *events.entry(event).or_insert(0) += cost;
    }
    let max_active = match get(s, "allocation").get("maximumActiveMarkets") {
        Some(x) if !x.is_null() => js_int(x).unwrap_or(0),
        _ => 20,
    };
    let max_market = amount(
        match get(s, "allocation").get("maximumMarketCost") {
            Some(x) if !x.is_null() => x.as_str().unwrap_or(""),
            _ => "100.000000",
        },
    )?;
    let max_event = amount(
        match get(s, "allocation").get("maximumEventCost") {
            Some(x) if !x.is_null() => x.as_str().unwrap_or(""),
            _ => "200.000000",
        },
    )?;
    need(
        total <= 1000000000
            && markets.values().filter(|v| **v > 0).count() as i64 <= max_active
            && markets.values().all(|v| *v <= max_market)
            && events.values().all(|v| *v <= max_event),
        "CAPITAL_OR_EVENT_CEILING",
    )?;
    need(
        sget(g, "cashLower").is_some()
            && amount(sget(g, "cashLower").unwrap())? >= 0
            && (!cash || reserved <= amount(sget(g, "cashLower").unwrap())?),
        "GLOBAL_CASH_FUNDING",
    )?;
    Ok(())
}

fn continue_owned(s: &V, ex: &str, a: &V, at: i64) -> Result<V, String> {
    let slot = get(get(s, "inventory"), ex);
    let member = get(get(s, "scope"), "exchanges")
        .as_array()
        .and_then(|xs| xs.iter().find(|x| sget(x, "exchangeId") == Some(ex)))
        .cloned()
        .unwrap_or(V::Null);
    need(!member.is_null(), "MARKET_SCOPE")?;
    let mut binding = get(s, "scope").as_object().cloned().unwrap_or_default();
    for k in ["marketId", "exchangeId"] {
        if let Some(v) = member.get(k) {
            binding.insert(k.into(), v.clone());
        }
    }
    let current = inspect_scoped_rest_no_account(&json!({
        "account": own_account(a, ex),
        "binding": V::Object(binding.clone()),
        "readAt": iso(at as f64),
    }))?;
    let p = get(&current, "positions")
        .as_array()
        .and_then(|ps| ps.iter().find(|p| sget(p, "exchangeId") == Some(ex)))
        .cloned();
    need(
        p.as_ref().map(|p| {
            sget(p, "marketId") == sget(&member, "marketId") && get(p, "settled") != &V::Bool(true)
        }).unwrap_or(true),
        "POSITION_SCOPE",
    )?;
    if !slot.is_null() {
        let old = inspect_scoped_rest_no_account(&json!({
            "account": get(&slot, "after"),
            "binding": V::Object(binding),
            "readAt": get(get(&slot, "proof"), "readAt"),
        }))?;
        let p_qty = p.as_ref().and_then(|p| js_int(get(p, "quantity"))).unwrap_or(0);
        need(
            instant_v(get(get(&slot, "proof"), "readAt"))? <= instant_v(get(&current, "startedAt"))?
                && same(
                    &json!(sorted_by_id(
                        get(&current, "scopedOrders").as_array().cloned().unwrap_or_default().as_slice()
                    )),
                    &json!(sorted_by_id(
                        get(&old, "scopedOrders").as_array().cloned().unwrap_or_default().as_slice()
                    )),
                )?
                && p_qty == -js_int(get(get(&slot, "proof"), "remainingOwnedQuantity")).unwrap_or(0),
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

fn native_entry(s: &V, e: &V, at: i64) -> Result<V, String> {
    let mut account_risk = get(e, "accountRisk").clone();
    let o = active(s);
    if let Some(o) = o.as_ref() {
        if sget(get(o, "body"), "action") == Some("buy")
            && ["prepared", "dispatching"].contains(&sget(o, "phase").unwrap_or(""))
            && get(o, "fetchStarts").as_array().is_some_and(|f| f.is_empty())
        {
            let full = reservation(s)?;
            let mut without_s = s.clone();
            let iid = get(o, "intentId").clone();
            if let Some(m) = without_s.as_object_mut() {
                let filtered: Vec<V> = get(s, "orders")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|v| get(v, "intentId") != &iid)
                    .collect();
                m.insert("orders".into(), json!(filtered));
            }
            let without = reservation(&without_s)?;
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
            let rows = account_risk.get("reservations").cloned();
            let ids = account_risk.get("unresolvedIntentIds").cloned();
            if rows.as_ref().is_some_and(V::is_array)
                && ids.as_ref().is_some_and(V::is_array)
                && (ids
                    .as_ref()
                    .and_then(V::as_array)
                    .is_some_and(|x| x.iter().any(|id| id == &iid))
                    || rows
                        .as_ref()
                        .and_then(V::as_array)
                        .is_some_and(|rs| {
                            rs.iter().any(|r| {
                                own.iter().any(|v| get(v, "exposureId") == get(r, "exposureId"))
                            })
                        }))
            {
                need(
                    get(&account_risk, "unknownAcceptance") == &V::Bool(false)
                        && ids
                            .as_ref()
                            .and_then(V::as_array)
                            .is_some_and(|x| x.iter().all(|id| id == &iid))
                        && own.iter().all(|r| {
                            rows.as_ref()
                                .and_then(V::as_array)
                                .is_some_and(|rs| {
                                    rs.iter().any(|v| same(v, r).unwrap_or(false))
                                })
                        }),
                    "PREPARED_RISK_BINDING",
                )?;
                let original_risk_hash = payload_hash(&account_risk)?;
                let mut substituted = account_risk.as_object().cloned().unwrap_or_default();
                substituted.insert(
                    "reservations".into(),
                    json!(rows
                        .as_ref()
                        .and_then(V::as_array)
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|r| {
                            !own.iter().any(|v| get(v, "exposureId") == get(r, "exposureId"))
                        })
                        .collect::<Vec<V>>()),
                );
                substituted.insert(
                    "unresolvedIntentIds".into(),
                    json!(ids
                        .as_ref()
                        .and_then(V::as_array)
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|id| id != &iid)
                        .collect::<Vec<V>>()),
                );
                substituted.insert(
                    "sourceHash".into(),
                    json!(payload_hash(&json!({
                        "kind": "exact_unfetched_intent_reservation_substitution",
                        "originalRiskHash": original_risk_hash,
                        "intentId": iid,
                        "reservationHash": get(&full, "reservationHash"),
                    }))?),
                );
                account_risk = V::Object(substituted);
            }
        }
    }
    let mut plan_input = e.as_object().cloned().unwrap_or_default();
    plan_input.insert("accountRisk".into(), account_risk);
    plan_input.insert("allocation".into(), get(s, "allocation").clone());
    plan_input.insert("scope".into(), get(s, "scope").clone());
    plan_input.insert("now".into(), json!(iso(at as f64)));
    let result = crate::baal_scaled_entry_signal_v1::plan_baal_scaled_entry(&V::Object(plan_input));
    need(
        get(&result, "proposalReady") == &V::Bool(true),
        &format!(
            "ENTRY_{}",
            get(&result, "reason").as_str().unwrap_or("undefined")
        ),
    )?;
    for l in get(&result, "legs")
        .as_array()
        .ok_or_else(|| "BAAL_SCALED_ENTRY_LEGS".to_string())?
    {
        need(
            get(get(s, "scope"), "exchanges")
                .as_array()
                .is_some_and(|xs| {
                    xs.iter().any(|m| {
                        sget(m, "exchangeId") == sget(l, "exchangeId")
                            && sget(m, "marketId") == sget(l, "marketId")
                    })
                }),
            "ENTRY_OUTSIDE_ACTIVATION",
        )?;
    }
    Ok(result)
}

fn book_versions(s: &V, o: &V, e: &V) -> Result<V, String> {
    let list: Vec<V> = if sget(get(o, "body"), "action") == Some("buy") {
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
            sget(&c, "marketId").unwrap_or(""),
            sget(get(s, "scope"), "tournamentId").unwrap_or("")
        );
        let r = raw(get(c, "receipt"), "GET", &endpoint)?;
        let (data, _) = parse_number_lexemes(sget(&r, "bodyText").unwrap_or(""))?;
        let b = normalize_book(
            &data,
            sget(&c, "marketId").unwrap_or(""),
            sget(get(s, "scope"), "tournamentId").unwrap_or(""),
            sget(&r, "receivedAt").unwrap_or(""),
            sget(&r, "requestStartedAt").unwrap_or(""),
            20000.0,
            &BTreeSet::new(),
            "authoritative_receipt_v1",
        )?;
        need(get(&b, "exchangeId") == get(c, "exchangeId"), "BOOK_EXCHANGE")?;
        let mut v = Map::new();
        v.insert("sequence".into(), get(&b, "sequence").clone());
        v.insert("sourceAt".into(), get(&b, "sourceAt").clone());
        v.insert("requestStartedAt".into(), get(&r, "requestStartedAt").clone());
        v.insert("receivedAt".into(), get(&r, "receivedAt").clone());
        v.insert(
            "depthHash".into(),
            json!(payload_hash(&json!({"bids": get(&b, "bids"), "asks": get(&b, "asks")}))?),
        );
        let v = V::Object(v);
        let ex = sget(c, "exchangeId").unwrap_or("");
        let old = get(o, "books").get(ex).cloned();
        if let Some(old) = old.filter(|x| !x.is_null()) {
            need(
                instant_v(get(&v, "requestStartedAt"))? >= instant_v(get(&old, "requestStartedAt"))?
                    && instant_v(get(&v, "receivedAt"))? >= instant_v(get(&old, "receivedAt"))?
                    && sget(&v, "sequence")
                        .unwrap_or("")
                        .parse::<i128>()
                        .unwrap_or(i128::MIN)
                        >= sget(&old, "sequence")
                            .unwrap_or("")
                            .parse::<i128>()
                            .unwrap_or(i128::MIN)
                    && instant_ns(sget(&v, "sourceAt").unwrap_or("")).map_err(|e| e.0)?
                        >= instant_ns(sget(&old, "sourceAt").unwrap_or("")).map_err(|e| e.0)?,
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
        out.insert(ex.to_string(), v);
    }
    Ok(V::Object(out))
}

fn sell_source(s: &V, ex: &str, requested: Option<i64>, e: &V, at: i64) -> Result<V, String> {
    let slot = get(get(s, "inventory"), ex);
    need(
        js_int(get(get(&slot, "proof"), "remainingOwnedQuantity")).is_some_and(|q| q > 0),
        "NO_OWNED_LOT",
    )?;
    let a = continue_owned(s, ex, get(e, "account"), at)?;
    let member = get(get(s, "scope"), "exchanges")
        .as_array()
        .and_then(|xs| xs.iter().find(|x| sget(x, "exchangeId") == Some(ex)))
        .cloned()
        .unwrap_or(V::Null);
    let endpoint = format!(
        "/markets/{}/orderbook?tournamentId={}&depth=20",
        sget(&member, "marketId").unwrap_or(""),
        sget(get(s, "scope"), "tournamentId").unwrap_or("")
    );
    let r = raw(get(e, "book"), "GET", &endpoint)?;
    need(
        js_int(get(&r, "status")) == Some(200)
            && instant_v(get(&r, "receivedAt"))? <= at
            && instant_v(get(&r, "requestStartedAt"))? >= instant_v(get(&a, "completedAt"))?
            && at - instant_v(get(&r, "requestStartedAt"))? <= 20000,
        "FRESH_ACCOUNT_THEN_BOOK",
    )?;
    let (d, numbers) = parse_number_lexemes(sget(&r, "bodyText").unwrap_or(""))?;
    need(
        !d.as_object().map(|m| m.contains_key("marketId")).unwrap_or(false)
            || get(&d, "marketId") == get(&member, "marketId"),
        "BOOK_MARKET",
    )?;
    let b = normalize_book(
        &d,
        sget(&member, "marketId").unwrap_or(""),
        sget(get(s, "scope"), "tournamentId").unwrap_or(""),
        sget(&r, "receivedAt").unwrap_or(""),
        sget(&r, "requestStartedAt").unwrap_or(""),
        20000.0,
        &BTreeSet::new(),
        "authoritative_receipt_v1",
    )?;
    let contexts = get(&d, "contexts").as_array().cloned().unwrap_or_default();
    need(
        get(&b, "exchangeId").as_str() == Some(ex)
            && contexts.len() == 1
            && contexts
                .first()
                .and_then(|c| c.get("orderbook"))
                .and_then(|o| o.get("exchanges"))
                .and_then(V::as_array)
                .is_some_and(|x| x.len() == 1)
            && get(&b, "asks").as_array().is_some_and(|x| x.len() <= 20),
        "BOOK_SCOPE",
    )?;
    let remaining = js_int(get(get(&slot, "proof"), "remainingOwnedQuantity")).unwrap_or(0);
    let max_shares = match get(s, "allocation").get("maximumSharesPerOrder") {
        Some(x) if !x.is_null() => js_int(x).unwrap_or(0),
        _ => 100,
    };
    let limit_qty = std::cmp::min(requested.unwrap_or(100), std::cmp::min(remaining, max_shares));
    need(limit_qty > 0, "SELL_QUANTITY")?;
    let max_order_cost = amount(
        match get(s, "allocation").get("maximumOrderCost") {
            Some(x) if !x.is_null() => x.as_str().unwrap_or(""),
            _ => "25.000000",
        },
    )?;
    for (i, l) in get(&b, "asks")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let lex = numbers.get(&serde_json::to_string(&json!([
            "contexts", 0, "orderbook", "exchanges", 0, "asks", i, "quantity"
        ])).unwrap());
        let lq = js_int(get(l, "quantity")).ok_or_else(|| "INVALID".to_string())?;
        need(
            lex.is_some_and(|lex| {
                lex.bytes().filter(|b| *b == b'.').count() <= 1
                    && {
                        let int_part = lex.split('.').next().unwrap_or("");
                        !int_part.is_empty()
                            && (!int_part.starts_with('0') || int_part == "0")
                            && int_part.bytes().all(|b| b.is_ascii_digit())
                            && lex.split('.').nth(1).is_none_or(|f| {
                                !f.is_empty() && f.bytes().all(|b| b == b'0')
                            })
                    }
                    && num_bigint::BigInt::parse_bytes(
                        lex.split('.').next().unwrap_or("").as_bytes(),
                        10,
                    ) == Some(num_bigint::BigInt::from(lq))
            }),
            "EXACT_DEPTH",
        )?;
        let limit = baal_service_runtime::fixed::SCALE
            - price(sget(l, "price").ok_or_else(|| "INVALID".to_string())?)?
            - TICK;
        if limit < TICK {
            continue;
        }
        let q = std::cmp::min(
            limit_qty,
            std::cmp::min(
                (get(l, "quantity").as_f64().unwrap_or(0.0) / 100.0).floor() as i64,
                (max_order_cost / limit) as i64,
            ),
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
            let ep = price(sget(&lot, "entryPrice").ok_or_else(|| "INVALID".to_string())?)?;
            max_cost = std::cmp::max(max_cost, ep);
            left -= std::cmp::min(left, js_int(get(&lot, "quantity")).unwrap_or(0));
        }
        if limit < max_cost + TICK {
            continue;
        }
        let mut out = Map::new();
        out.insert("marketId".into(), get(&member, "marketId").clone());
        out.insert("exchangeId".into(), json!(ex));
        out.insert("quantity".into(), json!(q));
        out.insert("limit".into(), json!(money(limit)));
        out.insert("bookVersion".into(), get(&b, "versionKey").clone());
        out.insert("bookRequestStartedAt".into(), get(&r, "requestStartedAt").clone());
        out.insert("sourceBodyHash".into(), get(&r, "sourceBodyHash").clone());
        out.insert("minimumNetPerShare".into(), json!("0.005000"));
        out.insert("costProtection".into(), json!("each_consumed_fifo_lot"));
        out.insert("at".into(), json!(iso(at as f64)));
        return Ok(V::Object(out));
    }
    need(false, "PROTECTED_SELL_UNAVAILABLE")?;
    unreachable!()
}

fn preflight(s: &V, o: &V, e: &V, g: &V, at: i64, planned: &V) -> Result<(), String> {
    book_versions(s, o, e)?;
    need(
        js_int(get(get(o, "body"), "quantity")).is_some_and(|q| q <= 100)
            && price(&num_str(&get(get(o, "body"), "price")))?
                * js_int(get(get(o, "body"), "quantity")).unwrap_or(0) as i128
                <= amount(
                    match get(s, "allocation").get("maximumOrderCost") {
                        Some(x) if !x.is_null() => x.as_str().unwrap_or(""),
                        _ => "25.000000",
                    },
                )?
            && at < instant_v(get(get(o, "body"), "expirationDate"))?,
        "ORDER_BOUND_OR_EXPIRED",
    )?;
    risk(s, g, at, false)?;
    budget(s, g, &reservation(planned)?, at, sget(get(o, "body"), "action") == Some("buy"))?;
    if sget(get(o, "body"), "action") == Some("buy") {
        need(
            at >= instant_v(get(get(s, "activation"), "startedAt"))?
                && at < instant_v(get(get(s, "activation"), "entryExpiresAt"))?
                && at < instant_v(get(get(o, "proposal"), "expiresAt"))?,
            "ENTRY_WINDOW",
        )?;
        let p = native_entry(s, e, at)?;
        let other = get(&p, "legs")
            .as_array()
            .and_then(|ls| {
                ls.iter().find(|l| {
                    sget(l, "exchangeId") != sget(get(o, "body"), "exchangeId")
                })
            })
            .cloned()
            .unwrap_or(V::Null);
        need(
            sget(get(&p, "chosenLeg"), "exchangeId") == sget(get(o, "body"), "exchangeId")
                && get(&p, "eventId") == get(get(o, "package"), "eventId")
                && js_int(get(get(&p, "chosenLeg"), "quantity")).unwrap_or(0)
                    >= js_int(get(get(o, "body"), "quantity")).unwrap_or(0)
                && price(sget(get(&p, "chosenLeg"), "limit").unwrap_or("9"))?
                    <= price(&num_str(&get(get(o, "body"), "price")))?
                && price(sget(&other, "limit").unwrap_or("9"))?
                    + price(&num_str(&get(get(o, "body"), "price")))?
                    <= 990000
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
        continue_owned(s, sget(get(o, "body"), "exchangeId").unwrap_or(""), get(e, "account"), at)?;
    } else {
        let p = sell_source(
            s,
            sget(get(o, "body"), "exchangeId").unwrap_or(""),
            js_int(get(get(o, "body"), "quantity")),
            e,
            at,
        )?;
        need(
            js_int(get(&p, "quantity")).unwrap_or(0)
                >= js_int(get(get(o, "body"), "quantity")).unwrap_or(0)
                && price(sget(&p, "limit").unwrap_or(""))?
                    >= price(&num_str(&get(get(o, "body"), "price")))?,
            "FIXED_SELL_PROTECTION",
        )?;
    }
    Ok(())
}

fn cancel_check(s: &V, e: &V, at: i64) -> Result<(), String> {
    let o = active(s);
    need(
        o.as_ref().is_some_and(|o| !get(o, "orderId").is_null())
            && at >= instant_v(get(&get(o.as_ref().unwrap(), "body"), "expirationDate"))?,
        "CANCEL_NOT_EXPIRED_OR_OWNED",
    )?;
    let o = o.unwrap();
    risk(s, get(e, "globalRisk"), at, true)?;
    let r = raw(
        get(e, "order"),
        "GET",
        &format!("/orders/{}", sget(&o, "orderId").unwrap_or("")),
    )?;
    let (d, _) = parse_number_lexemes(sget(&r, "bodyText").unwrap_or(""))?;
    need(
        js_int(get(&r, "status")) == Some(200)
            && instant_v(get(&r, "receivedAt"))? <= at
            && at - instant_v(get(&r, "requestStartedAt"))? <= 20000
            && js_int(get(&d, "id")).map(|x| x.to_string()) == sget(&o, "orderId").map(|x| x.to_string())
            && get(&d, "exchangeId") == get(get(&o, "body"), "exchangeId")
            && get(&d, "tournamentId") == get(get(s, "scope"), "tournamentId")
            && get(&d, "side").as_str() == Some("no")
            && get(&d, "action") == get(get(&o, "body"), "action")
            && get(&d, "open") == &V::Bool(true)
            && get(&normalize_official_fill_tick(get(&d, "priceLimit"))?, "price").as_str()
                == Some(money(price(&num_str(&get(get(&o, "body"), "price")))?).as_str())
            && get(&d, "expirationDate") == get(get(&o, "body"), "expirationDate"),
        "CANCEL_SCOPE",
    )?;
    Ok(())
}

fn binding(s: &V, o: &V) -> V {
    let mut b = get(s, "scope").as_object().cloned().unwrap_or_default();
    b.insert("marketId".into(), get(o, "marketId").clone());
    b.insert("exchangeId".into(), get(get(o, "body"), "exchangeId").clone());
    b.insert("orderId".into(), get(o, "orderId").clone());
    b.insert("intentId".into(), get(o, "intentId").clone());
    b.insert(
        "idempotencyKey".into(),
        get(get(o, "body"), "idempotencyKey").clone(),
    );
    b.insert("side".into(), json!("no"));
    b.insert("action".into(), get(get(o, "body"), "action").clone());
    b.insert("quantity".into(), get(get(o, "body"), "quantity").clone());
    b.insert(
        "limit".into(),
        json!(money(price(&num_str(&get(get(o, "body"), "price"))).unwrap_or(0))),
    );
    b.insert("createdAt".into(), get(o, "createdAt").clone());
    b.insert("expirationDate".into(), get(get(o, "body"), "expirationDate").clone());
    V::Object(b)
}

fn canonical_ack(o: &V, r: &V) -> bool {
    (|| -> Result<bool, String> {
        let (d, numbers) = parse_number_lexemes(sget(r, "bodyText").unwrap_or(""))?;
        let exact = |k: &str| -> bool {
            let raw = numbers.get(&serde_json::to_string(&json!([k])).unwrap());
            raw.is_some_and(|v| {
                v.len() <= 128
                    && {
                        let t = v.strip_prefix('-').unwrap_or(v);
                        let int_part = t.split('.').next().unwrap_or("");
                        !int_part.is_empty()
                            && (!int_part.starts_with('0') || int_part == "0")
                            && int_part.bytes().all(|b| b.is_ascii_digit())
                            && v.split('.').nth(1).is_none_or(|f| {
                                !f.is_empty() && f.bytes().all(|b| b == b'0')
                            })
                    }
                    && js_int(get(&d, k)).is_some()
                    && num_bigint::BigInt::parse_bytes(
                        v.split('.').next().unwrap_or("").as_bytes(),
                        10,
                    ) == Some(num_bigint::BigInt::from(js_int(get(&d, k)).unwrap()))
            })
        };
        let cost = amount(
            numbers
                .get(&serde_json::to_string(&json!(["totalCost"])).unwrap())
                .map(|s| s.as_str())
                .ok_or_else(|| "ACK".to_string())?,
        )?;
        let q = js_int(get(&d, "quantityTraded")).unwrap_or(i64::MIN);
        let limit = price(&num_str(&get(get(o, "body"), "price")))?;
        let cneed = |ok: bool, code: &str| -> Result<(), String> {
            if ok {
                Ok(())
            } else {
                Err(code.to_string())
            }
        };
        cneed(
            js_int(get(r, "status")) == Some(200)
                && js_int(get(&d, "orderId")).is_some_and(|x| x > 0)
                && get(&d, "exchangeId") == get(get(o, "body"), "exchangeId")
                && get(&d, "side").as_str() == Some("no")
                && get(&d, "action") == get(get(o, "body"), "action")
                && js_int(get(&d, "quantity")).is_some_and(|x| x > 0)
                && js_int(get(&d, "quantity")).unwrap_or(0)
                    <= js_int(get(get(o, "body"), "quantity")).unwrap_or(0)
                && ["orderId", "quantity", "quantityTraded", "remainingQuantity"]
                    .iter()
                    .all(|k| exact(k))
                && q >= 0
                && js_int(get(&d, "remainingQuantity")).is_some_and(|x| x >= 0)
                && q + js_int(get(&d, "remainingQuantity")).unwrap_or(i64::MIN)
                    == js_int(get(&d, "quantity")).unwrap_or(i64::MIN)
                && get(&d, "open").is_boolean()
                && (get(&d, "open") == &V::Bool(false)
                    || js_int(get(&d, "remainingQuantity")).unwrap_or(0) > 0)
                && get(&normalize_official_fill_tick(get(&d, "price"))?, "price").as_str()
                    == Some(money(limit).as_str()),
            "ACK_CANONICAL",
        )?;
        cneed(
            if q == 0 {
                cost == 0 && get(&d, "fillPrice") == &V::Null && d.get("fillPrice").is_some()
            } else {
                cost >= TICK * q as i128
                    && cost <= 995000 * q as i128
                    && get(&d, "fillPrice").as_f64().is_some_and(|x| x.is_finite())
                    && if sget(get(o, "body"), "action") == Some("buy") {
                        cost <= limit * q as i128
                    } else {
                        cost >= limit * q as i128
                    }
            },
            "ACK_PRINCIPAL",
        )?;
        if get(&d, "all") == &V::Null {
            // `d.all === null` (explicit null) short-circuits; absent falls through.
            if d.get("all").is_some() {
                return Ok(true);
            }
        }
        let a = get(&d, "all");
        let mut fields: Vec<String> = [
            "fullNotionalCost",
            "effectiveEntryCost",
            "collateralSavings",
            "relationshipIds",
            "componentId",
            "guaranteedPayoutFloorAfter",
            "outstandingAdvanceAfter",
            "collateralRepayment",
            "netBuyingPowerImpact",
            "redemptionCredit",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        fields.sort();
        let a_keys: Vec<String> = a
            .as_object()
            .map(|m| {
                let mut k: Vec<String> = m.keys().cloned().collect();
                k.sort();
                k
            })
            .unwrap_or_default();
        cneed(
            a.is_object()
                && same(&json!(a_keys), &json!(fields))?
                && get(a, "relationshipIds").as_array().is_some_and(|x| x.is_empty())
                && a.get("componentId") == Some(&V::Null),
            "ACK_NON_NEUTRAL",
        )?;
        for k in ["fullNotionalCost", "effectiveEntryCost"] {
            cneed(
                amount(
                    numbers
                        .get(&serde_json::to_string(&json!(["all", k])).unwrap())
                        .map(|s| s.as_str())
                        .ok_or_else(|| "ACK".to_string())?,
                )? == cost,
                "ACK_COST",
            )?;
        }
        for k in [
            "collateralSavings",
            "guaranteedPayoutFloorAfter",
            "outstandingAdvanceAfter",
            "collateralRepayment",
            "redemptionCredit",
        ] {
            cneed(
                amount(
                    numbers
                        .get(&serde_json::to_string(&json!(["all", k])).unwrap())
                        .map(|s| s.as_str())
                        .ok_or_else(|| "ACK".to_string())?,
                )? == 0,
                "ACK_NON_NEUTRAL",
            )?;
        }
        cneed(
            amount(
                numbers
                    .get(&serde_json::to_string(&json!(["all", "netBuyingPowerImpact"])).unwrap())
                    .map(|s| s.as_str())
                    .ok_or_else(|| "ACK".to_string())?,
            )? == if sget(get(o, "body"), "action") == Some("buy") { cost } else { -cost },
            "ACK_SIGN",
        )?;
        Ok(true)
    })()
    .unwrap_or(false)
}

fn reduce(s: Option<V>, e: &V) -> Result<V, String> {
    let kind = sget(e, "kind").unwrap_or("");
    if kind == "initialize" {
        need(s.is_none(), "INITIALIZED")?;
        let mut st = Map::new();
        st.insert("schemaVersion".into(), json!(1));
        st.insert("mode".into(), json!("baal_scaled_no_controller_v1"));
        st.insert("scope".into(), get(e, "scope").clone());
        st.insert("allocation".into(), get(e, "allocation").clone());
        st.insert("activation".into(), get(e, "activation").clone());
        st.insert("inventory".into(), json!({}));
        st.insert("orders".into(), json!([]));
        st.insert("reservation".into(), V::Null);
        return Ok(V::Object(st));
    }
    let s = s.ok_or_else(|| "BAAL_SCALED_NOT_INITIALIZED".to_string())?;
    let mut n = s.clone();
    let oi = active_index(&n);
    if kind == "reservation" {
        need(
            get(get(e, "receipt"), "reservationHash") == get(&reservation(&s)?, "reservationHash")
                && get(get(e, "receipt"), "allocationHash").as_str()
                    == Some(payload_hash(get(&s, "allocation"))?.as_str())
                && hex(get(get(e, "receipt"), "globalLedgerHash"))
                && instant_v(get(get(e, "receipt"), "confirmedAt"))? <= instant_v(get(e, "at"))?,
            "RESERVATION",
        )?;
        n.as_object_mut()
            .unwrap()
            .insert("reservation".into(), get(e, "receipt").clone());
        return Ok(n);
    }
    if kind == "prepared" {
        need(oi.is_none(), "UNKNOWN_OR_OPEN_INTENT")?;
        let (proposal, pkg, market_id, ex, q, limit_v);
        if sget(e, "action") == Some("buy") {
            need(
                instant_v(get(e, "at"))? >= instant_v(get(get(&s, "activation"), "startedAt"))?
                    && instant_v(get(e, "at"))? < instant_v(get(get(&s, "activation"), "entryExpiresAt"))?,
                "ENTRY_WINDOW",
            )?;
            proposal = native_entry(&s, get(e, "evidence"), instant_v(get(e, "at"))?)?;
            let cl = get(&proposal, "chosenLeg").clone();
            market_id = sget(&cl, "marketId").unwrap_or("").to_string();
            ex = sget(&cl, "exchangeId").unwrap_or("").to_string();
            q = js_int(get(&cl, "quantity")).unwrap_or(0);
            limit_v = get(&cl, "limit").clone();
            let mut p = Map::new();
            p.insert("eventId".into(), get(&proposal, "eventId").clone());
            p.insert(
                "legs".into(),
                json!(get(&proposal, "legs")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|l| json!({"marketId": get(l, "marketId"), "exchangeId": get(l, "exchangeId")}))
                    .collect::<Vec<V>>()),
            );
            p.insert("unitPackageCeiling".into(), json!("0.990000"));
            pkg = V::Object(p);
        } else {
            need(sget(e, "action") == Some("sell"), "ACTION")?;
            proposal = sell_source(
                &s,
                sget(e, "exchangeId").unwrap_or(""),
                js_int(get(e, "quantity")),
                get(e, "evidence"),
                instant_v(get(e, "at"))?,
            )?;
            market_id = sget(&proposal, "marketId").unwrap_or("").to_string();
            ex = sget(&proposal, "exchangeId").unwrap_or("").to_string();
            q = js_int(get(&proposal, "quantity")).unwrap_or(0);
            limit_v = get(&proposal, "limit").clone();
            pkg = get(get(get(&s, "inventory"), sget(e, "exchangeId").unwrap_or("")), "package").clone();
        }
        need(
            sget(e, "intentId").is_some()
                && sget(e, "idempotencyKey").is_some()
                && !get(&s, "orders").as_array().cloned().unwrap_or_default().iter().any(|v| {
                    get(v, "intentId") == get(e, "intentId")
                        || get(get(v, "body"), "idempotencyKey") == get(e, "idempotencyKey")
                }),
            "INTENT_ID",
        )?;
        let expires = iso((instant_v(get(e, "at"))? + 30000).min(
            if sget(e, "action") == Some("buy") {
                instant_v(get(get(&s, "activation"), "entryExpiresAt"))?
            } else {
                i64::MAX
            },
        ) as f64);
        let limit_num: f64 = limit_v
            .as_str()
            .and_then(|x| x.parse::<f64>().ok())
            .or_else(|| limit_v.as_f64())
            .unwrap_or(f64::NAN);
        let body = json!({
            "idempotencyKey": get(e, "idempotencyKey"),
            "exchangeId": ex,
            "side": "no",
            "action": get(e, "action"),
            "quantity": q,
            "price": limit_num,
            "expirationDate": expires,
            "tournamentId": get(get(&s, "scope"), "tournamentId"),
        });
        let mut candidate = Map::new();
        candidate.insert("intentId".into(), get(e, "intentId").clone());
        candidate.insert("marketId".into(), json!(market_id));
        candidate.insert("createdAt".into(), get(e, "at").clone());
        candidate.insert("body".into(), body.clone());
        candidate.insert("bodyJson".into(), json!(canonical_payload(&body)?));
        candidate.insert("bodyHash".into(), json!(payload_hash(&body)?));
        candidate.insert("proposal".into(), proposal);
        candidate.insert("package".into(), pkg);
        candidate.insert(
            "before".into(),
            own_account(get(get(e, "evidence"), "account"), &ex),
        );
        candidate.insert(
            "priorProof".into(),
            get(get(&s, "inventory"), &ex)
                .get("proof")
                .cloned()
                .unwrap_or(V::Null),
        );
        candidate.insert(
            "priorAfter".into(),
            get(get(&s, "inventory"), &ex)
                .get("after")
                .cloned()
                .unwrap_or(V::Null),
        );
        candidate.insert("phase".into(), json!("prepared"));
        candidate.insert("orderId".into(), V::Null);
        candidate.insert("ack".into(), V::Null);
        candidate.insert("proof".into(), V::Null);
        candidate.insert("cancelPending".into(), json!(false));
        candidate.insert("fences".into(), json!([]));
        candidate.insert("fetchStarts".into(), json!([]));
        candidate.insert("receipts".into(), json!([]));
        let candidate = V::Object(candidate);
        let books = book_versions(&s, &candidate, get(e, "evidence"))?;
        let mut candidate = candidate;
        candidate
            .as_object_mut()
            .unwrap()
            .insert("books".into(), books);
        n.as_object_mut()
            .unwrap()
            .get_mut("orders")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(candidate.clone());
        preflight(&s, &candidate, get(e, "evidence"), get(e, "globalRisk"), instant_v(get(e, "at"))?, &n)?;
        return Ok(n);
    }
    let oi = oi.ok_or_else(|| "BAAL_SCALED_NO_ACTIVE_INTENT".to_string())?;
    {
        let o = &get(&n, "orders").as_array().unwrap()[oi];
        need(get(o, "proof").is_null(), "NO_ACTIVE_INTENT")?;
    }
    match kind {
        "abandoned_prepared" => {
            let o = get(&n, "orders").as_array().unwrap()[oi].clone();
            need(
                sget(&o, "phase") == Some("prepared")
                    && o.get("operation").is_none()
                    && get(&o, "orderId").is_null()
                    && get(&o, "ack").is_null()
                    && get(&o, "proof").is_null()
                    && get(&o, "cancelPending") == &V::Bool(false)
                    && get(&o, "fences").as_array().is_some_and(|x| x.is_empty())
                    && get(&o, "fetchStarts").as_array().is_some_and(|x| x.is_empty())
                    && get(&o, "receipts").as_array().is_some_and(|x| x.is_empty())
                    && instant_v(get(e, "at"))? >= instant_v(get(get(&o, "body"), "expirationDate"))?,
                "ABANDON_REQUIRES_EXPIRED_NEVER_ADMITTED",
            )?;
            let m = n
                .as_object_mut()
                .unwrap()
                .get_mut("orders")
                .unwrap()
                .as_array_mut()
                .unwrap()[oi]
                .as_object_mut()
                .unwrap();
            m.insert("phase".into(), json!("abandoned"));
            m.insert("abandonedAt".into(), get(e, "at").clone());
            m.insert("abandonedReason".into(), json!("expired_before_any_admission"));
            Ok(n)
        }
        "admitted" | "network_fence" => {
            let o = get(&n, "orders").as_array().unwrap()[oi].clone();
            if sget(e, "operation") == Some("place") {
                need(
                    if kind == "admitted" {
                        sget(&o, "phase") == Some("prepared")
                    } else {
                        sget(&o, "phase") == Some("dispatching")
                    },
                    "PLACEMENT_PHASE",
                )?;
                preflight(&s, &o, get(e, "evidence"), get(e, "globalRisk"), instant_v(get(e, "at"))?, &s)?;
                let books = book_versions(&s, &o, get(e, "evidence"))?;
                n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap()[oi]
                    .as_object_mut()
                    .unwrap()
                    .insert("books".into(), books);
            } else {
                need(
                    sget(e, "operation") == Some("cancel")
                        && (kind != "admitted" || get(&o, "cancelPending") != &V::Bool(true)),
                    "OPERATION",
                )?;
                cancel_check(&s, get(e, "evidence"), instant_v(get(e, "at"))?)?;
            }
            let m = n
                .as_object_mut()
                .unwrap()
                .get_mut("orders")
                .unwrap()
                .as_array_mut()
                .unwrap()[oi]
                .as_object_mut()
                .unwrap();
            if kind == "admitted" {
                m.insert("operation".into(), get(e, "operation").clone());
                if sget(e, "operation") == Some("place") {
                    m.insert("phase".into(), json!("dispatching"));
                } else {
                    m.insert("cancelPending".into(), json!(true));
                }
            } else {
                need(
                    get(&o, "operation") == get(e, "operation")
                        && payload_hash(get(e, "leaseIdentity"))?
                            == get(e, "leaseHash").as_str().unwrap_or(""),
                    "LEASE_FENCE",
                )?;
                let mut fence = Map::new();
                fence.insert("at".into(), get(e, "at").clone());
                fence.insert("leaseIdentity".into(), get(e, "leaseIdentity").clone());
                fence.insert("leaseHash".into(), get(e, "leaseHash").clone());
                fence.insert("operation".into(), get(e, "operation").clone());
                m.get_mut("fences")
                    .unwrap()
                    .as_array_mut()
                    .unwrap()
                    .push(V::Object(fence));
            }
            Ok(n)
        }
        "fetch_start" => {
            let o = get(&n, "orders").as_array().unwrap()[oi].clone();
            let f = get(&o, "fences")
                .as_array()
                .and_then(|x| x.last())
                .cloned()
                .unwrap_or(V::Null);
            let w = get(e, "start");
            need(
                !f.is_null()
                    && get(w, "intentId") == get(&o, "intentId")
                    && get(w, "requestBodyHash") == get(&o, "bodyHash")
                    && get(w, "requestBodyJson") == get(&o, "bodyJson")
                    && get(w, "leaseHash") == get(&f, "leaseHash")
                    && same(get(w, "leaseIdentity"), get(&f, "leaseIdentity"))?
                    && sget(w, "fetchInvocationId").is_some()
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
                                .any(|x| get(x, "fetchInvocationId") == get(w, "fetchInvocationId"))
                        })
                    && instant_v(get(w, "requestStartedAt"))? >= instant_v(get(&f, "at"))?
                    && instant_v(get(w, "requestStartedAt"))? <= instant_v(get(e, "at"))?
                    && sget(w, "method")
                        == Some(if get(&o, "operation").as_str() == Some("place") {
                            "POST"
                        } else {
                            "DELETE"
                        })
                    && sget(w, "endpoint")
                        == Some(if get(&o, "operation").as_str() == Some("place") {
                            "/orders".to_string()
                        } else {
                            format!("/orders/{}", sget(&o, "orderId").unwrap_or(""))
                        }
                        .as_str()),
                "ACTUAL_FETCH",
            )?;
            if get(&o, "operation").as_str() == Some("place") {
                need(
                    instant_v(get(w, "requestStartedAt"))?
                        < instant_v(get(get(&o, "body"), "expirationDate"))?,
                    "EXPIRED_FETCH",
                )?;
            }
            n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap()[oi]
                .as_object_mut()
                .unwrap()
                .get_mut("fetchStarts")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(w.clone());
            Ok(n)
        }
        "wire" => {
            let o = get(&n, "orders").as_array().unwrap()[oi].clone();
            let w = get(&o, "fetchStarts")
                .as_array()
                .and_then(|x| x.last())
                .cloned()
                .unwrap_or(V::Null);
            let r = get(e, "receipt");
            need(
                !w.is_null()
                    && get(r, "fetchInvocationId") == get(&w, "fetchInvocationId")
                    && get(r, "requestStartedAt") == get(&w, "requestStartedAt")
                    && get(r, "method") == get(&w, "method")
                    && get(r, "endpoint") == get(&w, "endpoint")
                    && get(r, "sourceBodyHash").as_str()
                        == Some(sha_str(sget(r, "bodyText").unwrap_or("")).as_str())
                    && instant_v(get(r, "receivedAt"))? <= instant_v(get(e, "at"))?,
                "WIRE_BINDING",
            )?;
            {
                let m = n
                    .as_object_mut()
                    .unwrap()
                    .get_mut("orders")
                    .unwrap()
                    .as_array_mut()
                    .unwrap()[oi]
                    .as_object_mut()
                    .unwrap();
                m.get_mut("receipts")
                    .unwrap()
                    .as_array_mut()
                    .unwrap()
                    .push(r.clone());
            }
            if get(&o, "operation").as_str() == Some("cancel") {
                return Ok(n);
            }
            let d = parse_number_lexemes(sget(r, "bodyText").unwrap_or(""))
                .map(|x| x.0)
                .ok();
            if let Some(d) = d.as_ref() {
                if let Some(oid) = js_int(get(d, "orderId")) {
                    if oid > 0 {
                        n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap()[oi]
                            .as_object_mut()
                            .unwrap()
                            .insert("orderId".into(), json!(oid.to_string()));
                    }
                }
            }
            let mut ack = r.as_object().cloned().unwrap_or_default();
            let o_now = get(&n, "orders").as_array().unwrap()[oi].clone();
            ack.insert("requestBodyHash".into(), get(&o_now, "bodyHash").clone());
            let phase = if js_int(get(r, "status")) == Some(200) {
                if canonical_ack(&o_now, r) {
                    "acknowledged"
                } else {
                    "semantic_mismatch"
                }
            } else {
                "uncertain"
            };
            {
                let m = n
                    .as_object_mut()
                    .unwrap()
                    .get_mut("orders")
                    .unwrap()
                    .as_array_mut()
                    .unwrap()[oi]
                    .as_object_mut()
                    .unwrap();
                m.insert("ack".into(), V::Object(ack));
                m.insert("phase".into(), json!(phase));
            }
            Ok(n)
        }
        "error" => {
            let o = get(&n, "orders").as_array().unwrap()[oi].clone();
            let m = n
                .as_object_mut()
                .unwrap()
                .get_mut("orders")
                .unwrap()
                .as_array_mut()
                .unwrap()[oi]
                .as_object_mut()
                .unwrap();
            if sget(&o, "phase") == Some("dispatching") {
                m.insert("phase".into(), json!("uncertain"));
            }
            m.insert("lastError".into(), get(e, "code").clone());
            Ok(n)
        }
        "reconciled" => {
            let o = get(&n, "orders").as_array().unwrap()[oi].clone();
            need(
                sget(&o, "orderId").is_some()
                    && !get(&o, "ack").is_null()
                    && sget(&o, "phase") == Some("acknowledged"),
                "UNRESOLVED_ACK",
            )?;
            let start = get(&o, "fetchStarts")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .find(|v| sget(v, "method") == Some("POST"))
                .cloned()
                .unwrap_or(V::Null);
            need(!start.is_null(), "ACTUAL_FETCH_MISSING")?;
            let mut verify_input = get(e, "evidence").as_object().cloned().unwrap_or_default();
            verify_input.insert("binding".into(), binding(&s, &o));
            verify_input.insert("before".into(), get(&o, "before").clone());
            verify_input.insert("ack".into(), get(&o, "ack").clone());
            verify_input.insert("dispatchReceipt".into(), start);
            verify_input.insert("readAt".into(), get(e, "at").clone());
            verify_input.insert(
                "reservedFunding".into(),
                json!(if sget(get(&o, "body"), "action") == Some("buy") {
                    money(990000i128 * js_int(get(get(&o, "body"), "quantity")).unwrap_or(0) as i128)
                } else {
                    "0.000000".to_string()
                }),
            );
            verify_input.insert("priorProof".into(), get(&o, "priorProof").clone());
            verify_input.insert("priorAfter".into(), get(&o, "priorAfter").clone());
            let proof = verify_scoped_rest_no_multishare(&V::Object(verify_input))?;
            if sget(get(&o, "body"), "action") == Some("sell") {
                need(
                    amount(sget(&proof, "realizedPnl").ok_or_else(|| "INVALID".to_string())?)?
                        >= TICK * js_int(get(&proof, "filledQuantity")).unwrap_or(0) as i128,
                    "PROTECTED_REALIZED_PROFIT",
                )?;
            }
            let ex = sget(get(&o, "body"), "exchangeId").unwrap_or("").to_string();
            {
                let m = n
                    .as_object_mut()
                    .unwrap()
                    .get_mut("orders")
                    .unwrap()
                    .as_array_mut()
                    .unwrap()[oi]
                    .as_object_mut()
                    .unwrap();
                m.insert("proof".into(), proof.clone());
                m.insert("phase".into(), json!("reconciled"));
                m.insert("cancelPending".into(), json!(false));
            }
            let mut inv = Map::new();
            inv.insert("proof".into(), proof);
            inv.insert("after".into(), get(get(e, "evidence"), "after").clone());
            inv.insert("package".into(), get(&o, "package").clone());
            n.as_object_mut()
                .unwrap()
                .get_mut("inventory")
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(ex, V::Object(inv));
            Ok(n)
        }
        _ => need(false, "EVENT").map(|_| n),
    }
}


// ==== baal-scaled-controller-legacy-replay-v1.mjs (replay tail) ====
// The JS module's first 103 lines are byte-identical to
// baal-scaled-no-controller-v1.mjs (shared helpers/reducer above). What
// follows is the replay-only tail: a sealed-seed journal replayer plus the
// neutral-ack validator re-export. `need` still throws BAAL_SCALED_*.
pub const LEGACY_SCALED_CONTROLLER_SOURCE_HASH: &str =
    "20c57aa53ae1cc6b7f1b5aeb42b51cfdc391ded04165032ed102165a087780c0";

/// replayBaalScaledControllerV1({bodyText,expectedCodeIdentity,
/// expectedScope=null,expectedAllocation=null})
pub fn replay_baal_scaled_controller_v1(input: &V) -> Result<V, String> {
    let body_text = get(input, "bodyText").as_str().unwrap_or("");
    let expected_code_identity = get(input, "expectedCodeIdentity");
    let expected_scope = input.get("expectedScope").cloned().unwrap_or(V::Null);
    let expected_allocation = input.get("expectedAllocation").cloned().unwrap_or(V::Null);
    need(
        get(input, "bodyText").is_string()
            && !body_text.is_empty()
            && body_text.len() <= 128 * 1024 * 1024
            && body_text.ends_with('\n'),
        "LEGACY_REPLAY_INPUT",
    )?;
    let legacy_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("execution")
        .join("baal-scaled-no-controller-v1.mjs");
    let legacy_bytes =
        std::fs::read(&legacy_path).map_err(|e| format!("LEGACY_SOURCE_READ: {e}"))?;
    need(
        payload_hash(&identity()?)? == payload_hash(expected_code_identity)?
            && sha256(&legacy_bytes) == LEGACY_SCALED_CONTROLLER_SOURCE_HASH,
        "LEGACY_SOURCE_IDENTITY",
    )?;
    let mut state: Option<V> = None;
    let mut sequence: i64 = 0;
    let mut previous = String::new();
    let mut first = V::Null;
    for line in body_text.trim_end().split('\n') {
        let row: V =
            serde_json::from_str(line).map_err(|e| format!("SyntaxError: {e}"))?;
        // {hash, ...body} — removes the hash key, preserving order.
        let mut body = row.as_object().cloned().unwrap_or_default();
        let hash = body.shift_remove("hash").unwrap_or(V::Null);
        let body = V::Object(body);
        need(
            js_stringify(&row) == line
                && {
                    sequence += 1;
                    js_int(get(&body, "sequence")) == Some(sequence)
                }
                && get(&body, "previous").as_str() == Some(previous.as_str())
                && get(&row, "hash").as_str()
                    == Some(sha_str(&js_stringify(&body)).as_str())
                && same(get(get(&body, "event"), "codeIdentity"), expected_code_identity)?,
            "LEGACY_CHAIN_CODE",
        )?;
        instant_v(get(&body, "at"))?;
        // body.event.input — a non-object event is a JS TypeError, not a
        // chain-code failure.
        if !get(&body, "event").is_object() {
            return Err("TypeError".to_string());
        }
        state = Some(reduce(state, get(get(&body, "event"), "input"))?);
        need(
            same(state.as_ref().unwrap(), get(&body, "state"))?,
            "LEGACY_REDUCER_STATE",
        )?;
        previous = hash.as_str().unwrap_or("").to_string();
        if sequence == 1 {
            first = json!({
                "bytes": line.len() + 1,
                "sha256": sha_str(&format!("{line}\n")),
                "sequence": sequence,
                "hash": hash,
            });
        }
    }
    let state = state.ok_or_else(|| "BAAL_SCALED_LEGACY_EMPTY".to_string())?;
    configuration(
        get(&state, "scope"),
        get(&state, "allocation"),
        get(&state, "activation"),
        expected_code_identity,
    )?;
    if js_truthy(Some(&expected_scope)) {
        need(
            same(get(&state, "scope"), &expected_scope)?,
            "LEGACY_SCOPE",
        )?;
    }
    if js_truthy(Some(&expected_allocation)) {
        need(
            same(get(&state, "allocation"), &expected_allocation)?,
            "LEGACY_ALLOCATION",
        )?;
    }
    need(active(&state).is_none(), "LEGACY_ACTIVE_INTENT")?;
    let bytes = body_text.len();
    Ok(json!({
        "schemaVersion": 1,
        "mode": "exact_scaled_v1_sealed_seed_replay",
        "state": state.clone(),
        "stateHash": payload_hash(&state)?,
        "anchor": {
            "bytes": bytes,
            "sha256": sha_str(body_text),
            "sequence": sequence,
            "hash": previous,
        },
        "rootPrefix": first,
        "legacyCodeIdentityHash": payload_hash(expected_code_identity)?,
        "legacySourceHash": LEGACY_SCALED_CONTROLLER_SOURCE_HASH,
        "legacyReservation": reservation(&state)?,
        "economicReplayPerformed": true,
    }))
}

/// validateLegacyScaledNeutralAck=(order,receipt)=>canonicalAck(order,receipt)
pub fn validate_legacy_scaled_neutral_ack(order: &V, receipt: &V) -> bool {
    canonical_ack(order, receipt)
}
