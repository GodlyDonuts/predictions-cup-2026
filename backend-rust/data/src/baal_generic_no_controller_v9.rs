// execution/baal-generic-no-controller-v9.mjs — generic full-depth successor
// with exact complete inherited custody and a real financial-owner lock.
// Original journals remain sealed; no abandoned intent or original entry
// window is revived. v9 inherits custody through the expansion RECOVERED
// seed, pins all sealed sources through createPinnedCodeGuardV1 (basic(true)
// re-reads bytes immediately before fetch), and rolls a never-fetched place
// back to prepared on an affirmative clock block (physical_fetch_blocked).
//
// v9 deltas over v8: custody seed -> baal_expansion_recovered_seed_v2;
// abandoned_prepared uses provenUnfetchedFencesV2; new 'physical_fetch_blocked'
// reduce arm (EXACT_PHYSICAL_BLOCK); the per-seal basic() byte loop becomes
// sourceGuard({full}); onBeforeFetch's catch may append the block event.
//
// Port notes:
// - `need` throws BAAL_SCALED_<code>; uncaught helper errors (instant/amount/
//   price, verifier returns) propagate as their own messages, as in JS.
// - async dispatch/rawReceiptOf -> synchronous Fn traits (same model as
//   paired_rest_controller.rs). `issued` WeakMap -> pointer-keyed HashMap.
// - `sync(fn)` checks the return value is truthy (no promise possible).
// - Object.entries/WeakMap ordering follows serde_json insertion order.
// - JS `Number(limit)` on a money string -> f64 (price field is numeric).
// - `'ENTRY_'+result.reason` keeps the doubled BAAL_SCALED_ENTRY_ prefix.
use baal_execution::baal_custody_frontier_v1::read_baal_custody_frontier_v1;
use crate::baal_expansion_recovered_seed_v2::{
    build_baal_expansion_recovered_seed_v2, validate_baal_expansion_recovered_seed_v2,
};
use baal_execution::blocked_network_proof_v1::blocked_network_proof_v1;
use crate::cohort_profit_locked_depth_sweep_v2::select_cohort_profit_locked_depth_v2;
use crate::cohort_profit_locked_no_exit_v2::size_cohort_profit_locked_no_exit_v2;
use baal_execution::funding_failure_classification_v1::stale_funding_failure_can_retry;
use baal_execution::pinned_code_guard_v1::{
    create_pinned_code_guard_v1, PinnedCodeGuardV1,
};
use crate::physical_fetch_block_proof_v1::physical_fetch_block_proof_v1;
use crate::proven_unfetched_fences_v2::proven_unfetched_fences_v2;
use crate::baal_scaled_entry_signal_v3::plan_baal_scaled_entry_v3;
use baal_execution::core::{canonical_payload, payload_hash, validate_execution_state};
use baal_execution::frozen::collect_frozen_paper_files;
use baal_execution::gates::normalize_official_fill_tick;
use baal_execution::ledger::acquire_owner_lease;
use baal_execution::no_rest_multishare_reconciliation_v1::verify_scoped_rest_no_multishare;
use baal_execution::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account;
use baal_execution::receipts::parse_number_lexemes;
use baal_execution::transport::{Guard, OrderTransport, OrderTransportError, OrderTransportOptions};
use baal_service_runtime::contract::{instant_ns, ms_to_iso};
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use baal_service_runtime::ledger::OwnerLease;
use baal_service_runtime::market::normalize_book;
use serde_json::{json, Map, Value as V};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::os::unix::fs::MetadataExt;
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
        &["execution/baal-generic-no-controller-v9.mjs"],
    )
}
pub fn baal_generic_no_controller_v9_identity() -> Result<V, String> {
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
            && get(allocation, "capital").as_str() == Some("10000.000000")
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
        ("maximumOrderCost", 100000000i128),
        ("maximumMarketCost", 1000000000i128),
        ("maximumEventCost", 2000000000i128),
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
        opt_int("maximumActiveMarkets", 20).is_some_and(|v| v > 0 && v <= 100),
        "ACTIVE_MARKET_CEILING",
    )?;
    need(
        instant_v(get(activation, "exitExpiresAt")).is_ok()
            && instant_v(get(activation, "exitExpiresAt"))?
                > instant_v(get(activation, "startedAt"))?
            && instant_v(get(activation, "exitExpiresAt"))?
                - instant_v(get(activation, "startedAt"))?
                <= 86400000,
        "BOUNDED_EXITS",
    )?;
    need(
        js_int(get(activation, "schemaVersion")) == Some(1)
            && get(activation, "mode").as_str() == Some("baal_generic_no_production_v9")
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
            && instant_v(get(activation, "entryExpiresAt"))?
                <= instant_v(get(activation, "exitExpiresAt"))?
            && same(get(activation, "allowedActions"), &json!(["buy", "sell"]))?
            && sget(activation, "minimumCohortProfit") == Some("0.005000")
            && instant_v(get(allocation, "recordedAt"))? <= instant_v(get(activation, "startedAt"))?,
        "SELL_ONLY_ACTIVATION",
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

pub fn project_baal_generic_reservation_v9(state: &V) -> Result<V, String> {
    need(
        get(state, "mode").as_str() == Some("baal_generic_no_controller_v9"),
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
        total <= 10000000000
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
    let result = plan_baal_scaled_entry_v3(&V::Object(plan_input));
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

// const cohortRealized=s=>Object.values(s.inventory).reduce((n,p)=>n+amount(p.proof.cumulativeRealizedPnl),0n);
fn cohort_realized(s: &V) -> Result<i128, String> {
    let mut n: i128 = 0;
    for p in get(s, "inventory")
        .as_object()
        .map(|m| m.values().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        n += amount(
            get(get(p, "proof"), "cumulativeRealizedPnl")
                .as_str()
                .ok_or_else(|| "INVALID".to_string())?,
        )?;
    }
    Ok(n)
}

fn sell_source(s: &V, ex: &str, requested: Option<i64>, e: &V, at: i64) -> Result<V, String> {
    let slot = get(get(s, "inventory"), ex);
    need(
        js_int(get(get(&slot, "proof"), "remainingOwnedQuantity")).is_some_and(|q| q > 0),
        "NO_OWNED_LOT",
    )?;
    // v6: the owned market rule must be re-proven unchanged immediately
    // before the book is consulted (receivedAt <= book.requestStartedAt).
    let root_member = get(get(s, "scope"), "exchanges")
        .as_array()
        .and_then(|xs| xs.iter().find(|x| sget(x, "exchangeId") == Some(ex)))
        .cloned()
        .unwrap_or(V::Null);
    let root_receipt = raw(
        get(e, "rootReceipt"),
        "GET",
        &format!(
            "/markets/{}/nodes?tournamentId={}",
            sget(&root_member, "marketId").unwrap_or(""),
            sget(get(s, "scope"), "tournamentId").unwrap_or("")
        ),
    )?;
    let root_data = parse_number_lexemes(sget(&root_receipt, "bodyText").unwrap_or(""))?.0;
    need(
        js_int(get(&root_receipt, "status")) == Some(200)
            && instant_v(get(&root_receipt, "receivedAt"))?
                <= instant_v(get(get(e, "book"), "requestStartedAt"))?
            && instant_v(get(&root_receipt, "receivedAt"))?
                - instant_v(get(&root_receipt, "requestStartedAt"))?
                <= 20000
            && instant_v(get(&root_receipt, "receivedAt"))? <= at
            && at - instant_v(get(&root_receipt, "requestStartedAt"))? <= 300000
            && get(&root_data, "market_id") == get(&root_member, "marketId")
            && get(&root_data, "contexts")
                .as_array()
                .is_some_and(|x| x.len() == 1)
            && get(
                &get(
                    &get(&root_data, "contexts")
                        .as_array()
                        .cloned()
                        .unwrap_or_default()[0],
                    "tournament",
                ),
                "id",
            ) == get(get(s, "scope"), "tournamentId")
            && get(get(s, "ownedRuleHashes"), ex).as_str()
                == Some(payload_hash(get(&root_data, "root"))?.as_str()),
        "FRESH_UNCHANGED_OWNED_RULE",
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
    }
    let sweep = select_cohort_profit_locked_depth_v2(&json!({
            "realizedProfit": money(cohort_realized(s)?),
            "protectiveTicks": 1,
            "remainingLots": get(get(&slot, "proof"), "remainingLots"),
            "remainingOwnedQuantity": get(get(&slot, "proof"), "remainingOwnedQuantity"),
            "yesAsks": get(&b, "asks"),
            "requestedQuantity": limit_qty,
            "maximumSharesPerOrder": match get(s, "allocation").get("maximumSharesPerOrder") {
                Some(x) if !x.is_null() => x.clone(),
                _ => json!(100),
            },
            "maximumOrderCost": match get(s, "allocation").get("maximumOrderCost") {
                Some(x) if !x.is_null() => x.clone(),
                _ => json!("25.000000"),
            },
        }))?;
    let chosen = get(&sweep, "selected");
    if !chosen.is_null() {
        let mut out = Map::new();
        out.insert("marketId".into(), get(&member, "marketId").clone());
        out.insert("exchangeId".into(), json!(ex));
        out.insert("quantity".into(), get(chosen, "quantity").clone());
        out.insert("limit".into(), get(chosen, "protectedLimit").clone());
        out.insert("bookVersion".into(), get(&b, "versionKey").clone());
        out.insert("bookRequestStartedAt".into(), get(&r, "requestStartedAt").clone());
        out.insert("sourceBodyHash".into(), get(&r, "sourceBodyHash").clone());
        out.insert("minimumCohortProfit".into(), json!("0.005000"));
        out.insert(
            "costProtection".into(),
            json!("each_possible_FIFO_partial_fill_cohort_floor"),
        );
        out.insert("depthThroughLevel".into(), get(chosen, "throughLevel").clone());
        out.insert(
            "protectedTotalGain".into(),
            get(chosen, "protectedTradePnlIfFilled").clone(),
        );
        out.insert(
            "cohortBefore".into(),
            json!(money(cohort_realized(s)?)),
        );
        out.insert(
            "worstPartialCohort".into(),
            get(chosen, "worstPartialFillCohortLowerBound").clone(),
        );
        out.insert("at".into(), json!(iso(at as f64)));
        return Ok(V::Object(out));
    }
    need(false, "PROTECTED_SELL_UNAVAILABLE")?;
    unreachable!()
}

fn fresh_none(s: &V, e: &V, at: i64, market_id: &str) -> Result<(), String> {
    let graph = raw(
        get(e, "graphReceipt"),
        "GET",
        &format!(
            "/relationships/graph?marketId={}&depth=4&tournamentId={}",
            market_id,
            sget(get(s, "scope"), "tournamentId").unwrap_or("")
        ),
    )?;
    let collateral = raw(
        get(e, "collateralReceipt"),
        "GET",
        &format!(
            "/portfolio/collateral?tournamentId={}",
            sget(get(s, "scope"), "tournamentId").unwrap_or("")
        ),
    )?;
    for r in [&graph, &collateral] {
        need(
            js_int(get(r, "status")) == Some(200)
                && instant_v(get(r, "receivedAt"))? <= at
                && at - instant_v(get(r, "requestStartedAt"))? <= 5000,
            "FRESH_NONE_CLOCK",
        )?;
    }
    let g = parse_number_lexemes(sget(&graph, "bodyText").unwrap_or(""))?.0;
    let c = parse_number_lexemes(sget(&collateral, "bodyText").unwrap_or(""))?.0;
    let mut g_keys: Vec<&str> = g
        .as_object()
        .map(|m| m.keys().map(|k| k.as_str()).collect())
        .unwrap_or_default();
    g_keys.sort_unstable();
    let nodes_ok = get(&g, "nodes").as_array().is_some_and(|nodes| {
        nodes.len() <= 2
            && nodes.iter().all(|n| {
                get(get(s, "scope"), "exchanges")
                    .as_array()
                    .is_some_and(|xs| {
                        xs.iter().any(|m| {
                            get(m, "marketId") == get(n, "marketId")
                                && get(m, "exchangeId") == get(n, "exchangeId")
                        })
                    })
            })
    });
    need(
        g_keys == ["nodes", "relationships"]
            && get(&g, "relationships")
                .as_array()
                .is_some_and(|x| x.is_empty())
            && nodes_ok,
        "FRESH_NONE_GRAPH",
    )?;
    need(
        c.as_object()
            .is_some_and(|m| m.keys().map(|k| k.as_str()).collect::<Vec<_>>() == ["data"])
            && get(&c, "data").as_array().is_some_and(|x| x.is_empty()),
        "FRESH_NONE_COLLATERAL",
    )?;
    let refs: Vec<V> = match e.get("bookReceipts") {
        Some(x) if !x.is_null() => x.as_array().cloned().unwrap_or_default(),
        _ => {
            if !get(e, "book").is_null() {
                vec![json!({
                    "marketId": market_id,
                    "exchangeId": get(get(s, "scope"), "exchanges")
                        .as_array()
                        .and_then(|xs| {
                            xs.iter()
                                .find(|x| sget(x, "marketId") == Some(market_id))
                        })
                        .map(|x| get(x, "exchangeId").clone())
                        .unwrap_or(V::Null),
                })]
            } else {
                active(s)
                    .map(|o| get(get(&o, "package"), "legs").clone())
                    .unwrap_or(V::Null)
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            }
        }
    };
    need(
        get(&g, "nodes")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .all(|n| {
                refs.iter().any(|x| {
                    get(x, "marketId") == get(n, "marketId")
                        && get(x, "exchangeId") == get(n, "exchangeId")
                })
            }),
        "SCOPED_NONE_NODES",
    )?;
    let books: Vec<V> = match e.get("bookReceipts") {
        Some(x) if !x.is_null() => x
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|x| get(x, "receipt").clone())
            .collect(),
        _ => {
            if !get(e, "book").is_null() {
                vec![get(e, "book").clone()]
            } else {
                let mut v: Vec<V> = vec![
                    get(get(e, "after"), "cash").clone(),
                    get(get(e, "after"), "positions").clone(),
                ];
                v.extend(
                    get(get(e, "after"), "openOrdersPages")
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                );
                v.extend(
                    get(get(e, "after"), "scopedOrdersPages")
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                );
                if let Some(m) = get(get(e, "after"), "scopedOrdersPagesByExchange").as_object()
                {
                    for (_, pages) in m {
                        v.extend(pages.as_array().cloned().unwrap_or_default());
                    }
                }
                v
            }
        }
    };
    need(
        books.iter().try_fold(true, |acc, b| {
            Ok::<bool, String>(
                acc && instant_v(get(b, "receivedAt"))?
                    <= instant_v(get(&graph, "requestStartedAt"))?,
            )
        })? && instant_v(get(&graph, "receivedAt"))?
            <= instant_v(get(&collateral, "requestStartedAt"))?,
        "NONE_AFTER_BOOK",
    )?;
    Ok(())
}

fn current_owned_census(s: &V, account: &V, at: i64) -> Result<(), String> {
    let r = raw(
        get(account, "positions"),
        "GET",
        &format!(
            "/tournaments/{}/portfolio/positions",
            sget(get(s, "scope"), "tournamentSlug").unwrap_or("")
        ),
    )?;
    need(
        js_int(get(&r, "status")) == Some(200)
            && instant_v(get(&r, "receivedAt"))? <= at
            && at - instant_v(get(&r, "requestStartedAt"))? <= 20000,
        "CURRENT_OWNED_CENSUS_AGE",
    )?;
    let d = parse_number_lexemes(sget(&r, "bodyText").unwrap_or(""))?.0;
    need(get(&d, "positions").is_array(), "CURRENT_OWNED_CENSUS")?;
    let ps = get(&d, "positions").as_array().cloned().unwrap_or_default();
    if let Some(inv) = get(s, "inventory").as_object() {
        for (ex, slot) in inv.iter() {
            let roq = get(get(slot, "proof"), "remainingOwnedQuantity");
            if roq == &json!(0) {
                continue;
            }
            let neg_roq = roq.as_f64().map(|x| -x);
            let matches: Vec<&V> = ps
                .iter()
                .filter(|p| sget(p, "exchangeId") == Some(ex.as_str()))
                .collect();
            need(
                matches.len() == 1
                    && get(matches[0], "marketId")
                        == get(get(slot, "proof"), "marketId")
                    && matches[0].get("quantity").and_then(V::as_f64) == neg_roq
                        && neg_roq.is_some()
                    && get(matches[0], "settled") == &V::Bool(false),
                "CURRENT_OWNED_CENSUS",
            )?;
        }
    }
    Ok(())
}

fn preflight(
    s: &V,
    o: &V,
    e: &V,
    g: &V,
    at: i64,
    planned: &V,
    intent_only: bool,
) -> Result<(), String> {
    current_owned_census(s, get(e, "account"), at)?;
    book_versions(s, o, e)?;
    if !intent_only {
        fresh_none(s, e, at, sget(o, "marketId").unwrap_or(""))?;
    }
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
        need(
            at < instant_v(get(get(s, "activation"), "exitExpiresAt"))?
                && instant_v(get(get(o, "body"), "expirationDate"))?
                    <= instant_v(get(get(s, "activation"), "exitExpiresAt"))?,
            "EXIT_AUTHORITY_WINDOW",
        )?;
        let p = sell_source(
            s,
            sget(get(o, "body"), "exchangeId").unwrap_or(""),
            js_int(get(get(o, "body"), "quantity")),
            e,
            at,
        )?;
        let inv_slot = get(
            get(s, "inventory"),
            sget(get(o, "body"), "exchangeId").unwrap_or(""),
        );
        let fixed = size_cohort_profit_locked_no_exit_v2(&json!({
            "remainingLots": get(get(&inv_slot, "proof"), "remainingLots"),
            "remainingOwnedQuantity": get(get(&inv_slot, "proof"), "remainingOwnedQuantity"),
            "realizedProfit": money(cohort_realized(s)?),
            "yesAskLevel": {
                "price": money(SCALE - price(&num_str(&get(get(o, "body"), "price")))?),
                "quantity": get(get(o, "body"), "quantity"),
            },
            "protectiveTicks": 0,
            "maximumOrderCost": get(get(s, "allocation"), "maximumOrderCost"),
            "maximumSharesPerOrder": get(get(s, "allocation"), "maximumSharesPerOrder"),
        }))?;
        need(
            js_int(get(&p, "quantity")).unwrap_or(0)
                >= js_int(get(get(o, "body"), "quantity")).unwrap_or(0)
                && price(sget(&p, "limit").unwrap_or(""))?
                    >= price(&num_str(&get(get(o, "body"), "price")))?
                && js_int(get(&fixed, "quantity")) == js_int(get(get(o, "body"), "quantity")),
            "FIXED_COHORT_SELL_PROTECTION",
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
        validate_baal_expansion_recovered_seed_v2(
            get(e, "custodySeed"),
            get(e, "scope"),
            get(e, "allocation"),
        )?;
        need(
            get(get(e, "activation"), "inheritanceHash")
                == get(get(e, "custodySeed"), "seedHash"),
            "EXACT_INHERITANCE_ACTIVATION",
        )?;
        let inherited_closed: Vec<V> = get(get(e, "custodySeed"), "inheritedClosedOrders")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|o| {
                json!({
                    "intentId": get(o, "intentId"),
                    "body": {"idempotencyKey": get(get(o, "body"), "idempotencyKey")},
                    "orderId": get(o, "orderId"),
                    "phase": get(o, "phase"),
                    "proofHash": get(get(o, "proof"), "verificationHash"),
                })
            })
            .collect();
        let mut st = Map::new();
        st.insert("schemaVersion".into(), json!(1));
        st.insert("mode".into(), json!("baal_generic_no_controller_v9"));
        st.insert("scope".into(), get(e, "scope").clone());
        st.insert("allocation".into(), get(e, "allocation").clone());
        st.insert("activation".into(), get(e, "activation").clone());
        st.insert("inventory".into(), get(get(e, "custodySeed"), "inventory").clone());
        st.insert(
            "ownedRuleHashes".into(),
            get(get(e, "custodySeed"), "ownedRuleHashes").clone(),
        );
        st.insert("orders".into(), json!([]));
        st.insert("reservation".into(), V::Null);
        st.insert(
            "inheritanceHash".into(),
            get(get(e, "custodySeed"), "seedHash").clone(),
        );
        st.insert("inheritedClosedOrders".into(), json!(inherited_closed));
        st.insert(
            "inheritedHistoryHash".into(),
            json!(payload_hash(get(get(e, "custodySeed"), "inheritedClosedOrders"))?),
        );
        st.insert(
            "inheritedPairAssignments".into(),
            get(get(e, "custodySeed"), "inheritedPairAssignments").clone(),
        );
        st.insert(
            "inheritedPairRetirements".into(),
            get(get(e, "custodySeed"), "inheritedPairRetirements").clone(),
        );
        st.insert(
            "originalActivation".into(),
            get(get(e, "custodySeed"), "originalActivation").clone(),
        );
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
        need(
            sget(e, "action").is_some_and(|a| a == "buy" || a == "sell"),
            "ACTION",
        )?;
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
            need(
                sget(e, "action") == Some("sell")
                    && instant_v(get(e, "at"))?
                        < instant_v(get(get(&s, "activation"), "exitExpiresAt"))?,
                "BOUNDED_EXIT",
            )?;
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
                && !get(&s, "orders")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .chain(
                        get(&s, "inheritedClosedOrders")
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                            .iter(),
                    )
                    .any(|v| {
                        get(v, "intentId") == get(e, "intentId")
                            || get(get(v, "body"), "idempotencyKey") == get(e, "idempotencyKey")
                    }),
            "INTENT_ID",
        )?;
        current_owned_census(&s, get(get(e, "evidence"), "account"), instant_v(get(e, "at"))?)?;
        let expires = iso((instant_v(get(e, "at"))? + 30000).min(
            if sget(e, "action") == Some("buy") {
                instant_v(get(get(&s, "activation"), "entryExpiresAt"))?
            } else {
                instant_v(get(get(&s, "activation"), "exitExpiresAt"))?
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
        if sget(e, "action") == Some("buy") {
            let rule = get(get(&candidate, "proposal"), "sourceReceipts")
                .get("roots")
                .and_then(V::as_array)
                .and_then(|a| {
                    a.iter()
                        .find(|r| sget(r, "exchangeId") == Some(ex.as_str()))
                });
            need(rule.is_some(), "ENTRY_RULE")?;
            n.as_object_mut()
                .unwrap()
                .get_mut("ownedRuleHashes")
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(ex.clone(), get(rule.unwrap(), "rootHash").clone());
        }
        preflight(&s, &candidate, get(e, "evidence"), get(e, "globalRisk"), instant_v(get(e, "at"))?, &n, true)?;
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
                    && proven_unfetched_fences_v2(&o)?
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
                preflight(&s, &o, get(e, "evidence"), get(e, "globalRisk"), instant_v(get(e, "at"))?, &s, false)?;
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
        // v9: an affirmative before-actual-fetch clock block on a still-
        // dispatching place rolls it back to prepared with the proof kept.
        "physical_fetch_blocked" => {
            let o = get(&n, "orders").as_array().unwrap()[oi].clone();
            need(
                sget(&o, "phase") == Some("dispatching")
                    && get(&o, "operation").as_str() == Some("place")
                    && get(&o, "orderId").is_null()
                    && get(&o, "ack").is_null()
                    && get(&o, "fences")
                        .as_array()
                        .is_some_and(|x| x.len() == 1)
                    && get(&o, "fetchStarts")
                        .as_array()
                        .is_some_and(|x| x.is_empty())
                    && get(&o, "receipts")
                        .as_array()
                        .is_some_and(|x| x.is_empty())
                    && get(get(e, "proof"), "kind").as_str()
                        == Some("affirmative_before_actual_fetch_clock_block_v1")
                    && get(get(e, "proof"), "physicalFetchReached") == &V::Bool(false)
                    && get(get(e, "proof"), "requestBodyHash") == get(&o, "bodyHash")
                    && instant_v(get(get(e, "proof"), "at"))? == instant_v(get(e, "at"))?
                    && instant_v(get(e, "at"))?
                        >= instant_v(get(
                            &get(&o, "fences")
                                .as_array()
                                .cloned()
                                .unwrap_or_default()[0],
                            "at",
                        ))?,
                "EXACT_PHYSICAL_BLOCK",
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
                match m
                    .get_mut("physicalFetchBlockProofs")
                    .and_then(V::as_array_mut)
                {
                    Some(a) => a.push(get(e, "proof").clone()),
                    None => {
                        m.insert(
                            "physicalFetchBlockProofs".into(),
                            json!([get(e, "proof").clone()]),
                        );
                    }
                }
                m.insert("phase".into(), json!("prepared"));
                m.shift_remove("operation");
            }
            Ok(n)
        }
        // v5: a proven pre-network block on a still-dispatching, never-fetched
        // place keeps the proof and rolls the order back to prepared.
        "network_blocked" => {
            let o = get(&n, "orders").as_array().unwrap()[oi].clone();
            need(
                sget(&o, "phase") == Some("dispatching")
                    && get(&o, "operation").as_str() == Some("place")
                    && get(&o, "orderId").is_null()
                    && get(&o, "ack").is_null()
                    && !js_truthy(Some(get(&o, "cancelPending")))
                    && get(&o, "fences").as_array().map(|x| x.len()).unwrap_or(2) <= 1
                    && get(&o, "fetchStarts")
                        .as_array()
                        .is_some_and(|x| x.is_empty())
                    && get(&o, "receipts")
                        .as_array()
                        .is_some_and(|x| x.is_empty())
                    && get(get(e, "proof"), "kind").as_str()
                        == Some("serialized_client_before_network_block_v1")
                    && get(get(e, "proof"), "method").as_str() == Some("POST")
                    && get(get(e, "proof"), "endpoint").as_str() == Some("/orders")
                    && get(get(e, "proof"), "physicalFetchReached") == &V::Bool(false)
                    && get(get(e, "proof"), "requestBodyHash") == get(&o, "bodyHash")
                    && instant_v(get(get(e, "proof"), "at"))? == instant_v(get(e, "at"))?,
                "EXACT_UNFETCHED_NETWORK_BLOCK",
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
                match m
                    .get_mut("blockedNetworkProofs")
                    .and_then(V::as_array_mut)
                {
                    Some(a) => a.push(get(e, "proof").clone()),
                    None => {
                        m.insert(
                            "blockedNetworkProofs".into(),
                            json!([get(e, "proof").clone()]),
                        );
                    }
                }
                m.insert("phase".into(), json!("prepared"));
                m.shift_remove("operation");
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
            fresh_none(&s, get(e, "evidence"), instant_v(get(e, "at"))?, sget(&o, "marketId").unwrap_or(""))?;
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
            if sget(get(&o, "body"), "action") == Some("sell")
                && js_int(get(&proof, "filledQuantity")).unwrap_or(0) > 0
            {
                need(
                    cohort_realized(&s)?
                        + amount(sget(&proof, "realizedPnl").ok_or_else(|| "INVALID".to_string())?)?
                        >= TICK,
                    "COHORT_REALIZED_FLOOR",
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

struct InFlight {
    spec: V,
    bd_ptr: usize,
    #[allow(dead_code)]
    bn_ptr: usize,
}

pub struct BaalGenericNoControllerV9Deps {
    pub dispatch: Rc<baal_execution::paired_rest_controller::WireDispatch>,
    pub raw_receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub assert_account_lease: Rc<dyn Fn() -> V>,
    pub assert_financial_owner_lease: Rc<dyn Fn() -> V>,
    pub on_financial_halt: Rc<dyn Fn(&V) -> Result<V, String>>,
    // JS `canCheckMechanical=canOperate`: defaults to the can_operate guard.
    pub can_check_mechanical: Option<Rc<dyn Fn() -> bool>>,
    pub install_funding_reservation: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub get_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_evidence: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct InheritedSource {
    file: PathBuf,
    bytes: Vec<u8>,
    ino: u64,
    birthtime_ms: f64,
}

struct CtrlInner {
    scope: V,
    allocation: V,
    activation: V,
    source_runtime_dir: PathBuf,
    file: PathBuf,
    code: V,
    journal: Journal,
    state: Option<V>,
    closed: bool,
    failed: bool,
    busy: bool,
    size: u64,
    bytes_hash: String,
    ino: u64,
    birthtime_ms: f64,
    in_flight: Option<InFlight>,
    issued: HashMap<usize, V>,
    fetch_deadline: Option<i64>,
    last_guard_failure: V,
    local_timings: Vec<V>,
    inherited_sources: Vec<InheritedSource>,
    source_guard: PinnedCodeGuardV1,
    lease: OwnerLease,
    deps: BaalGenericNoControllerV9Deps,
}

fn stat_birthtime_ms(st: &fs::Metadata) -> f64 {
    st.created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

fn measured<T>(
    inner: &Rc<RefCell<CtrlInner>>,
    name: &str,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    // measured(name,fn): performance.now() delta recorded in localTimings
    // (capped at 40, oldest dropped) even when fn throws — a finally block.
    let start = std::time::Instant::now();
    let r = f();
    let ms = (start.elapsed().as_secs_f64() * 1000.0 * 1000.0).round() / 1000.0;
    let mut g = inner.borrow_mut();
    g.local_timings.push(json!({"name": name, "ms": ms}));
    if g.local_timings.len() > 40 {
        g.local_timings.remove(0);
    }
    drop(g);
    r
}

pub struct BaalGenericNoControllerV9 {
    inner: Rc<RefCell<CtrlInner>>,
    transport: OrderTransport,
}

impl CtrlInner {
    fn mechanical(&self) -> Result<V, String> {
        let check = self
            .deps
            .can_check_mechanical
            .clone()
            .unwrap_or_else(|| self.deps.can_operate.clone());
        need(!self.closed && !self.failed && check(), "STOPPED_OR_FAILED")?;
        self.lease.assert_ownership().map_err(|e| e.message)?;
        let l = truthy_sync((self.deps.assert_account_lease)())?;
        let f = truthy_sync((self.deps.assert_financial_owner_lease)())?;
        let actual: V = serde_json::from_str(
            &fs::read_to_string(
                self.source_runtime_dir
                    .join("baal-financial-owner")
                    .join("execution-owner.lock"),
            )
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        need(
            get(&l, "ownerId") == get(&self.scope, "ownerId")
                && get(&l, "tournamentId") == get(&self.scope, "tournamentId")
                && get(&l, "leaseId").as_str().is_some_and(|s| !s.is_empty()),
            "ACCOUNT_LEASE",
        )?;
        need(
            get(&actual, "ownerId") == get(&self.scope, "ownerId")
                && get(&actual, "runId") == get(&self.scope, "runId")
                && js_int(get(&actual, "pid")) == Some(std::process::id() as i64)
                && get(&actual, "scope").as_str() == Some("baal-global-financial-owner")
                && get(&f, "ownerId") == get(&self.scope, "ownerId")
                && get(&f, "runId") == get(&self.scope, "runId")
                && get(&f, "tournamentId") == get(&self.scope, "tournamentId")
                && get(&f, "leaseId").as_str() == Some(payload_hash(&actual)?.as_str()),
            "ACTUAL_FINANCIAL_OWNER",
        )?;
        Ok(l)
    }
    // v9: the per-seal byte comparison is replaced by the pinned code guard
    // ({full} requests an immediate byte re-read of every sealed source).
    fn basic(&self, full: bool) -> Result<V, String> {
        need((self.deps.can_operate)(), "STOPPED_OR_FAILED")?;
        let l = self.mechanical()?;
        self.source_guard.check(full)?;
        Ok(l)
    }
}

fn current_risk(inner: &Rc<RefCell<CtrlInner>>) -> Result<V, String> {
    measured(inner, "current_risk", || {
        truthy_sync((inner.borrow().deps.get_global_risk)()?)
    })
}

fn guard_error_code(e: &str) -> String {
    // /^[A-Z][A-Z0-9_]{1,150}$/
    let ok = (2..=151).contains(&e.chars().count())
        && e.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && e.chars()
            .skip(1)
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    if ok {
        e.to_string()
    } else {
        "GUARD_FAILED".to_string()
    }
}

fn append_event(inner: &Rc<RefCell<CtrlInner>>, input: &V) -> Result<(), String> {
    inner.borrow().basic(false)?;
    let (before, size, bytes_hash, ino, birthtime, code, dir) = {
        let g = inner.borrow();
        let before = fs::read(&g.file).map_err(|e| e.to_string())?;
        let stat = fs::metadata(&g.file).map_err(|e| e.to_string())?;
        use std::os::unix::fs::MetadataExt;
        let ino = stat.ino();
        let birthtime = stat
            .created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        (
            before,
            g.size,
            g.bytes_hash.clone(),
            g.ino,
            g.birthtime_ms,
            g.code.clone(),
            g.file.clone(),
        )
    };
    need(
        ino == inner.borrow().ino
            && birthtime == inner.borrow().birthtime_ms
            && before.len() as u64 == size
            && sha_bytes(&before) == bytes_hash
            && payload_hash(&identity().unwrap_or(V::Null)).unwrap_or_default()
                == payload_hash(&code).unwrap_or_default(),
        "SOURCE_CHANGED",
    )?;
    let _ = dir;
    let kind = sget(input, "kind").unwrap_or("").to_string();
    let next = measured(inner, &format!("reduce_{kind}"), || {
        reduce(inner.borrow().state.clone(), input)
    })?;
    need(
        (size + js_stringify(&next).len() as u64 + js_stringify(input).len() as u64)
            < 128 * 1024 * 1024,
        "JOURNAL_BOUND",
    )?;
    let event = {
        let g = inner.borrow();
        json!({"codeIdentity": g.code, "input": input})
    };
    let projection = json!({
        "mode": "baal_generic_no_event_projection_v9",
        "economicStateHash": payload_hash(&next)?,
    });
    let commit_result = measured(inner, &format!("commit_{kind}"), || {
        inner.borrow_mut().journal.commit(&projection, &event)
    });
    match commit_result {
        Ok(_) => {
            inner.borrow().mechanical()?;
            let all = fs::read(inner.borrow().file.clone()).map_err(|e| e.to_string())?;
            need(
                all[..size as usize] == before[..] && all.len() as u64 > size,
                "PREFIX_CHANGED",
            )?;
            let mut g = inner.borrow_mut();
            g.state = Some(next);
            g.size = all.len() as u64;
            g.bytes_hash = sha_bytes(&all);
            Ok(())
        }
        Err(e) => {
            inner.borrow_mut().failed = true;
            Err(format!("{e}:fatal:durableRecordingFailed"))
        }
    }
}

fn reserve(inner: &Rc<RefCell<CtrlInner>>) -> Result<V, String> {
    // v6 JS: catch(e){unchanged=bytes.length===size&&sha(bytes)===bytesHash;
    // if(staleFundingFailureCanRetry(e,{journalUnchanged:unchanged,
    // alreadyFailed:failed})){e.recoverable=true;throw e;}
    // failed=true;e.fatal=true;throw e;}
    // `recoverable` has no reader -> plain Err carries it; `fatal` becomes the
    // conventional `:fatal:` marker checked by the live-host catch.
    match reserve_inner(inner) {
        Ok(v) => Ok(v),
        Err(e) => {
            let unchanged = {
                let g = inner.borrow();
                fs::read(&g.file)
                    .map(|bytes| {
                        bytes.len() as u64 == g.size && sha_bytes(&bytes) == g.bytes_hash
                    })
                    .unwrap_or(false)
            };
            let stripped = e.strip_prefix('\u{0}').unwrap_or(&e);
            if stale_funding_failure_can_retry(
                &json!({
                    "code": stripped.split(':').next().unwrap_or(stripped),
                    "message": stripped,
                    "durableRecordingFailed": stripped.contains("durableRecordingFailed"),
                    "mayHaveExecuted": stripped.contains("mayHaveExecuted"),
                }),
                unchanged,
                inner.borrow().failed,
            ) {
                return Err(e);
            }
            inner.borrow_mut().failed = true;
            Err(format!("{e}:fatal:"))
        }
    }
}

fn reserve_inner(inner: &Rc<RefCell<CtrlInner>>) -> Result<V, String> {
    let (current, r, allocation, now) = {
        let g = inner.borrow();
        let state = g.state.clone().unwrap_or(V::Null);
        (
            reservation(&state)?,
            get(&state, "reservation").clone(),
            g.allocation.clone(),
            (g.deps.now)(),
        )
    };
    if get(&r, "reservationHash") == get(&current, "reservationHash")
        && get(&r, "allocationHash").as_str() == Some(payload_hash(&allocation)?.as_str())
        && hex(get(&r, "globalLedgerHash"))
        && instant_v(get(&r, "confirmedAt"))? <= now as i64
        && now as i64 - instant_v(get(&r, "confirmedAt"))? <= 20000
    {
        let g = current_risk(inner)?;
        if get(&g, "controllerReservationHash") == get(&current, "reservationHash")
            && get(&g, "globalLedgerHash") == get(&r, "globalLedgerHash")
            && get(&g, "unknownAcceptance") == &V::Bool(false)
            && get(&g, "complete") == &V::Bool(true)
            && get(&g, "eventScopeComplete") == &V::Bool(true)
            && instant_v(get(&g, "at"))? <= now as i64
            && now as i64 - instant_v(get(&g, "at"))? <= 20000
            && g.get("installedCashLower").is_some()
            && amount(sget(&g, "cashLower").unwrap_or(""))?
                >= amount(sget(&g, "installedCashLower").unwrap_or(""))?
        {
            return Ok(r.clone());
        }
    }
    let res = reservation(&{
        let g = inner.borrow();
        g.state.clone().unwrap_or(V::Null)
    })?;
    let receipt = truthy_sync((inner.borrow().deps.install_funding_reservation)(&res)?)?;
    append_event(
        inner,
        &json!({"kind": "reservation", "receipt": receipt, "at": iso((inner.borrow().deps.now)())}),
    )?;
    Ok(receipt)
}

fn reserved(inner: &Rc<RefCell<CtrlInner>>) -> Result<(), String> {
    let g = inner.borrow();
    let state = g.state.clone().unwrap_or(V::Null);
    let r = get(&state, "reservation");
    let now = (g.deps.now)();
    need(
        get(r, "reservationHash") == get(&reservation(&state)?, "reservationHash")
            && get(r, "allocationHash").as_str()
                == Some(payload_hash(&g.allocation)?.as_str())
            && hex(get(r, "globalLedgerHash"))
            && instant_v(get(r, "confirmedAt"))? <= now as i64
            && now as i64 - instant_v(get(r, "confirmedAt"))? <= 20000,
        "RESERVATION_STALE",
    )?;
    Ok(())
}

fn evidence(inner: &Rc<RefCell<CtrlInner>>, operation: &str) -> Result<V, String> {
    let (o, dep) = {
        let g = inner.borrow();
        let st = g.state.clone().unwrap_or(V::Null);
        (active(&st).unwrap_or(V::Null), ())
    };
    let _ = dep;
    let input = json!({
        "operation": operation,
        "intentId": get(&o, "intentId"),
        "orderId": get(&o, "orderId"),
        "marketId": get(&o, "marketId"),
        "exchangeId": get(get(&o, "body"), "exchangeId"),
    });
    measured(inner, &format!("cached_evidence_{operation}"), || {
        truthy_sync((inner.borrow().deps.get_evidence)(&input)?)
    })
}

// payloadHash({inventory,scope,allocation,activation,record,body}) — the
// financial fingerprint compared across the durable fence write.
fn financial_fingerprint(s: &V) -> Result<String, String> {
    payload_hash(&json!({
        "inventory": get(s, "inventory"),
        "scope": get(s, "scope"),
        "allocation": get(s, "allocation"),
        "activation": get(s, "activation"),
        "record": reservation(s)?,
        "body": get(&active(s).unwrap_or(V::Null), "body"),
    }))
}

fn validate(
    inner: &Rc<RefCell<CtrlInner>>,
    p: &V,
    spec: &V,
    network: bool,
) -> Result<bool, String> {
    inner.borrow().basic(false)?;
    reserved(inner)?;
    let (state, o) = {
        let g = inner.borrow();
        let st = g.state.clone().unwrap_or(V::Null);
        let o = active(&st);
        (st, o)
    };
    let o = o.ok_or_else(|| "BAAL_SCALED_PERMIT".to_string())?;
    need(
        get(p, "intentId") == get(&o, "intentId") && get(p, "bodyHash") == get(&o, "bodyHash"),
        "PERMIT",
    )?;
    let op = if get(p, "operation").as_str() == Some("place") {
        format!("place_{}", sget(get(&o, "body"), "action").unwrap_or(""))
    } else {
        "cancel".to_string()
    };
    let e = evidence(inner, &op)?;
    let g = match e.get("currentGlobalRisk") {
        Some(v) if !v.is_null() => v.clone(),
        _ => current_risk(inner)?,
    };
    let is_place = get(p, "operation").as_str() == Some("place");
    if is_place {
        need(
            get(spec, "method").as_str() == Some("POST")
                && get(spec, "endpoint").as_str() == Some("/orders")
                && get(spec, "bodyJson") == get(&o, "bodyJson"),
            "BODY_CHANGED",
        )?;
        preflight(&state, &o, &e, &g, (inner.borrow().deps.now)() as i64, &state, false)?;
    } else {
        let mut ev = e.as_object().cloned().unwrap_or_default();
        ev.insert("globalRisk".into(), g.clone());
        cancel_check(&state, &V::Object(ev), (inner.borrow().deps.now)() as i64)?;
    }
    let l = inner.borrow().mechanical()?;
    let financial_fp = financial_fingerprint(&state)?;
    let mut ev = e.as_object().cloned().unwrap_or_default();
    if get(p, "operation").as_str() == Some("cancel") {
        ev.insert("globalRisk".into(), g.clone());
    }
    append_event(
        inner,
        &json!({
            "kind": if network { "network_fence" } else { "admitted" },
            "operation": get(p, "operation"),
            "evidence": V::Object(ev),
            "globalRisk": g,
            "leaseIdentity": l,
            "leaseHash": payload_hash(&l)?,
            "at": iso((inner.borrow().deps.now)()),
        }),
    )?;
    if network {
        let state2 = inner.borrow().state.clone().unwrap_or(V::Null);
        need(
            financial_fp == financial_fingerprint(&state2)?,
            "NON_ECONOMIC_FENCE_ONLY",
        )?;
        // v9 reuses the same evidence packet and risk at the network fence
        // (const latest=e, riskNow=g) — no fresh reads; the v6 re-preflight
        // for place is removed (the deadline covers it), cancel still
        // re-checks cancelCheck.
        let latest = e.clone();
        let risk_now = g.clone();
        if !is_place {
            let mut ev = latest.as_object().cloned().unwrap_or_default();
            ev.insert("globalRisk".into(), risk_now.clone());
            cancel_check(&state2, &V::Object(ev), (inner.borrow().deps.now)() as i64)?;
        }
        reserved(inner)?;
        inner.borrow().mechanical()?;
        if is_place {
            let mut deadlines: Vec<i64> = vec![instant_v(get(
                get(&active(&state2).unwrap_or(V::Null), "body"),
                "expirationDate",
            ))?];
            let mut accounts: Vec<V> = vec![
                get(get(&latest, "account"), "cash").clone(),
                get(get(&latest, "account"), "positions").clone(),
            ];
            accounts.extend(
                get(get(&latest, "account"), "openOrdersPages")
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            );
            accounts.extend(
                get(get(&latest, "account"), "scopedOrdersPages")
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            );
            if let Some(m) = get(get(&latest, "account"), "scopedOrdersPagesByExchange").as_object() {
                for (_, pages) in m {
                    accounts.extend(pages.as_array().cloned().unwrap_or_default());
                }
            }
            for r in &accounts {
                deadlines.push(instant_v(get(r, "requestStartedAt"))? + 20000);
            }
            let books: Vec<V> = match latest.get("bookReceipts") {
                Some(x) if !x.is_null() => x
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|x| get(x, "receipt").clone())
                    .collect(),
                _ => vec![get(&latest, "book").clone()],
            };
            for r in &books {
                deadlines.push(instant_v(get(r, "requestStartedAt"))? + 20000);
            }
            deadlines.push(instant_v(get(get(&latest, "graphReceipt"), "requestStartedAt"))? + 5000);
            deadlines
                .push(instant_v(get(get(&latest, "collateralReceipt"), "requestStartedAt"))? + 5000);
            match latest.get("rootReceipts") {
                Some(x) if !x.is_null() => {
                    for r in x.as_array().cloned().unwrap_or_default() {
                        deadlines.push(
                            instant_v(get(get(&r, "receipt"), "requestStartedAt"))? + 300000,
                        );
                    }
                }
                _ => deadlines.push(
                    instant_v(get(get(&latest, "rootReceipt"), "requestStartedAt"))? + 300000,
                ),
            }
            deadlines.push(instant_v(get(&risk_now, "at"))? + 20000);
            deadlines.push(
                instant_v(get(get(&state2, "reservation"), "confirmedAt"))? + 20000,
            );
            deadlines.push(instant_v(get(get(&state2, "activation"), "exitExpiresAt"))?);
            inner.borrow_mut().fetch_deadline = deadlines.iter().copied().min();
        }
    }
    Ok(true)
}

fn wrap_dispatch(
    inner: &Rc<RefCell<CtrlInner>>,
    spec: &V,
    bd: Option<&Guard<'_>>,
    bn: Option<&Guard<'_>>,
) -> Result<V, baal_execution::paired_rest_controller::WireDispatchError> {
    let err_out = |code: String| baal_execution::paired_rest_controller::WireDispatchError {
        message: code,
        wire_receipt: None,
    };
    let run = || -> Result<V, String> {
        let o = {
            let g = inner.borrow();
            active(&g.state.clone().unwrap_or(V::Null)).unwrap_or(V::Null)
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
                    "beforeNetwork": json!(bn_ptr),
                }),
            );
            g.in_flight = Some(InFlight { spec: spec.clone(), bd_ptr, bn_ptr });
        }
        let capture = |inner: &Rc<RefCell<CtrlInner>>, response: &V| -> Result<(), String> {
            let receipt = (inner.borrow().deps.raw_receipt_of)(
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
                &receipt,
                sget(spec, "method").unwrap_or(""),
                sget(spec, "endpoint").unwrap_or(""),
            )?;
            need(
                get(&r, "status") == get(response, "status")
                    && instant_v(get(&r, "requestStartedAt"))?
                        >= instant_v(get(response, "requestStartedAt"))?
                    && instant_v(get(&r, "receivedAt"))? <= instant_v(get(response, "receivedAt"))?
                    && same(
                        &parse_number_lexemes(sget(&r, "bodyText").unwrap_or(""))?.0,
                        get(response, "data"),
                    )?,
                "RAW_RESPONSE_BINDING",
            )?;
            append_event(
                inner,
                &json!({"kind": "wire", "receipt": r, "at": iso((inner.borrow().deps.now)())}),
            )?;
            let st = inner.borrow().state.clone().unwrap_or(V::Null);
            if let Some(v) = active(&st) {
                if ["uncertain", "semantic_mismatch"]
                    .contains(&sget(&v, "phase").unwrap_or(""))
                {
                    truthy_sync((inner.borrow().deps.on_financial_halt)(&json!({
                        "kind": "unknown_or_non_neutral_generic_execution",
                        "sourceHash": get(&r, "sourceBodyHash"),
                        "allWritersStop": true,
                    }))?)?;
                }
            }
            Ok(())
        };
        let result = (inner.borrow().deps.dispatch)(spec, bd, bn);
        let out = match result {
            Ok(r) => {
                capture(inner, &r)?;
                Ok(r)
            }
            Err(e) => {
                if let Some(wr) = e.wire_receipt.clone() {
                    capture(inner, &wr)?;
                }
                Err(e.message)
            }
        };
        {
            let mut g = inner.borrow_mut();
            g.issued.remove(&bd_ptr);
            g.in_flight = None;
        }
        out
    };
    run().map_err(|e| baal_execution::paired_rest_controller::WireDispatchError {
        message: e.clone(),
        wire_receipt: None,
    })
    .map_err(|e| {
        let _ = err_out;
        e
    })
}

fn status_of(inner: &Rc<RefCell<CtrlInner>>) -> V {
    let g = inner.borrow();
    let state = g.state.clone().unwrap_or(V::Null);
    let o = active(&state);
    let mut out = Map::new();
    out.insert("mode".into(), get(&state, "mode").clone());
    out.insert("ownerId".into(), get(&g.scope, "ownerId").clone());
    out.insert("runId".into(), get(&g.scope, "runId").clone());
    out.insert("tournamentId".into(), get(&g.scope, "tournamentId").clone());
    out.insert(
        "allocationHash".into(),
        json!(payload_hash(&g.allocation).unwrap_or_default()),
    );
    out.insert("entryExpiresAt".into(), get(&g.activation, "entryExpiresAt").clone());
    out.insert("sequence".into(), json!(g.journal.sequence));
    out.insert("hash".into(), json!(g.journal.previous));
    out.insert(
        "phase".into(),
        o.as_ref().map(|o| get(o, "phase").clone()).unwrap_or(json!("ready")),
    );
    let orders = get(&state, "orders").as_array().cloned().unwrap_or_default();
    out.insert("orderCount".into(), json!(orders.len()));
    out.insert(
        "actualOrderCount".into(),
        json!(orders.iter().filter(|v| !get(v, "orderId").is_null()).count()),
    );
    out.insert(
        "abandonedIntentCount".into(),
        json!(orders
            .iter()
            .filter(|v| sget(v, "phase") == Some("abandoned"))
            .count()),
    );
    out.insert(
        "intent".into(),
        o.as_ref()
            .map(|o| {
                let fetches = get(o, "fetchStarts").as_array().cloned().unwrap_or_default();
                json!({
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
                    "actualFetchCount": fetches.len(),
                    "lastActualFetchStartAt": fetches.last().map(|f| get(f, "requestStartedAt").clone()).unwrap_or(V::Null),
                })
            })
            .unwrap_or(V::Null),
    );
    let mut inv = Map::new();
    if let Some(inventory) = get(&state, "inventory").as_object() {
        for (ex, x) in inventory {
            inv.insert(
                ex.clone(),
                json!({
                    "marketId": get(get(x, "proof"), "marketId"),
                    "quantity": get(get(x, "proof"), "remainingOwnedQuantity"),
                    "cost": get(get(x, "proof"), "remainingOwnedCost"),
                    "cumulativeRealizedPnl": get(get(x, "proof"), "cumulativeRealizedPnl"),
                    "proofHash": get(get(x, "proof"), "verificationHash"),
                    "eventId": get(get(x, "package"), "eventId"),
                }),
            );
        }
    }
    out.insert("inventory".into(), V::Object(inv));
    let realized: i128 = get(&state, "inventory")
        .as_object()
        .map(|inventory| {
            inventory
                .values()
                .map(|x| {
                    amount(
                        get(get(x, "proof"), "cumulativeRealizedPnl")
                            .as_str()
                            .unwrap_or(""),
                    )
                    .unwrap_or(0)
                })
                .sum()
        })
        .unwrap_or(0);
    out.insert("realizedPnl".into(), json!(money(realized)));
    out.insert(
        "reservation".into(),
        reservation(&state).unwrap_or(V::Null),
    );
    out.insert("failed".into(), json!(g.failed));
    out.insert("closed".into(), json!(g.closed));
    out.insert("busy".into(), json!(g.busy));
    let unknown_acceptance = orders.iter().any(|v| {
        !js_truthy(Some(get(v, "proof")))
            && sget(v, "phase") != Some("abandoned")
            && (["uncertain", "semantic_mismatch"].contains(&sget(v, "phase").unwrap_or(""))
                || (!js_truthy(Some(get(v, "orderId")))
                    && get(v, "fetchStarts").as_array().is_some_and(|fs| {
                        fs.iter().any(|f| sget(f, "method") == Some("POST"))
                    })))
    });
    out.insert("unknownAcceptance".into(), json!(unknown_acceptance));
    out.insert("additionalApiClients".into(), json!(0));
    out.insert("lastGuardFailure".into(), g.last_guard_failure.clone());
    out.insert("localTimings".into(), json!(g.local_timings));
    out.insert(
        "inheritedIntentCount".into(),
        json!(get(&state, "inheritedClosedOrders")
            .as_array()
            .map(|x| x.len())
            .unwrap_or(0)),
    );
    out.insert(
        "inheritanceHash".into(),
        get(&state, "inheritanceHash").clone(),
    );
    out.insert(
        "originalActivation".into(),
        get(&state, "originalActivation").clone(),
    );
    out.insert(
        "pairedInventoryUsesFullNominalFunding".into(),
        json!(true),
    );
    V::Object(out)
}

fn run<T>(inner: &Rc<RefCell<CtrlInner>>, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    need(!inner.borrow().busy, "BUSY")?;
    inner.borrow_mut().busy = true;
    let r = f();
    inner.borrow_mut().busy = false;
    r
}

// openBaalGenericNoControllerV9({dir,sourceRuntimeDir,scope,allocation,
//   activation,dispatch,rawReceiptOf,canOperate,assertAccountLease,
//   assertFinancialOwnerLease,onFinancialHalt,canCheckMechanical=canOperate,
//   installFundingReservation,getGlobalRisk,getEvidence,now})
pub fn open_baal_generic_no_controller_v9(
    dir: &Path,
    source_runtime_dir: &Path,
    scope: &V,
    allocation: &V,
    activation: &V,
    deps: BaalGenericNoControllerV9Deps,
) -> Result<BaalGenericNoControllerV9, String> {
    need(dir.is_absolute(), "DEPENDENCIES")?;
    need(source_runtime_dir.is_absolute(), "SOURCE_RUNTIME")?;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let frontier = read_baal_custody_frontier_v1(source_runtime_dir, &workspace)?;
    let custody_seed =
        build_baal_expansion_recovered_seed_v2(&frontier, scope, allocation, source_runtime_dir)?;
    need(
        get(activation, "inheritanceHash") == get(&custody_seed, "seedHash")
            && get(activation, "exitExpiresAt")
                == get(get(&custody_seed, "originalActivation"), "exitExpiresAt"),
        "SOURCE_BOUND_INHERITANCE",
    )?;
    let root: PathBuf = source_runtime_dir.components().collect();
    let mut inherited_sources: Vec<InheritedSource> = Vec::new();
    for x in get(&frontier, "sourceHashes")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .chain(
            get(&custody_seed, "recoverySources")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter(),
        )
        .chain(
            get(&custody_seed, "currentSources")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter(),
        )
        .chain(
            get(&custody_seed, "cohortRecoverySources")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter(),
        )
        .chain(
            get(&custody_seed, "expansionSources")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter(),
        )
    {
        let file: PathBuf = root.join(sget(&x, "relative").unwrap_or("")).components().collect();
        need(file.starts_with(&root), "SOURCE_PATH")?;
        let bytes = fs::read(&file).map_err(|e| e.to_string())?;
        let st = fs::metadata(&file).map_err(|e| e.to_string())?;
        need(
            js_int(get(&x, "bytes")) == Some(bytes.len() as i64)
                && sget(&x, "sha256") == Some(sha_bytes(&bytes).as_str()),
            "SOURCE_SEAL",
        )?;
        inherited_sources.push(InheritedSource {
            file,
            bytes,
            ino: st.ino(),
            birthtime_ms: stat_birthtime_ms(&st),
        });
    }
    // v9: createPinnedCodeGuardV1({workspace:sourceRuntimeDir,
    //   identity:Object.fromEntries(inheritedSources.map(p=>[
    //   path.relative(sourceRuntimeDir,p.file),sha(p.bytes)]))})
    let source_identity: BTreeMap<String, String> = inherited_sources
        .iter()
        .map(|p| {
            (
                p.file
                    .strip_prefix(&root)
                    .unwrap_or(&p.file)
                    .to_string_lossy()
                    .to_string(),
                sha_bytes(&p.bytes),
            )
        })
        .collect();
    let source_guard = create_pinned_code_guard_v1(source_runtime_dir, &source_identity)?;
    let code = identity()?;
    configuration(scope, allocation, activation, &code)?;
    let mut lease = match acquire_owner_lease(
        dir,
        sget(scope, "ownerId").unwrap_or(""),
        sget(scope, "runId").unwrap_or(""),
        "baal-generic-no-v9",
        (deps.now)(),
    ) {
        Ok(l) => l,
        Err(e) => return Err(e.message),
    };
    let file = dir.join("baal-generic-no-v9-journal.jsonl");
    let open_result: Result<(Journal, Option<V>, u64, String, u64, f64), String> = (|| {
        if !file.exists() {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&file)
                .map_err(|e| e.to_string())?;
        }
        let bytes = fs::read(&file).map_err(|e| e.to_string())?;
        need(bytes.is_empty() || *bytes.last().unwrap() == 10, "PARTIAL_JOURNAL")?;
        let journal = Journal::open(&file)?;
        let mut state: Option<V> = None;
        for line in String::from_utf8_lossy(&bytes).split('\n').filter(|l| !l.is_empty()) {
            let row: V = serde_json::from_str(line).map_err(|_| "BAAL_SCALED_REPLAY".to_string())?;
            need(
                same(get(&get(&row, "event"), "codeIdentity"), &code)?,
                "CODE_CHANGED",
            )?;
            state = Some(reduce(state, get(&get(&row, "event"), "input"))?);
            let proj = get(&row, "state");
            need(
                get(proj, "economicStateHash").as_str()
                    == Some(payload_hash(state.as_ref().unwrap())?.as_str())
                    && sget(proj, "mode") == Some("baal_generic_no_event_projection_v9"),
                "REPLAY",
            )?;
        }
        let size = bytes.len() as u64;
        let bytes_hash = sha_bytes(&bytes);
        let stat = fs::metadata(&file).map_err(|e| e.to_string())?;
        let ino = stat.ino();
        let birthtime = stat_birthtime_ms(&stat);
        Ok((journal, state, size, bytes_hash, ino, birthtime))
    })();
    let (journal, state, size, bytes_hash, ino, birthtime) = match open_result {
        Ok(v) => v,
        Err(e) => {
            let _ = lease.release();
            return Err(e);
        }
    };
    let inner = Rc::new(RefCell::new(CtrlInner {
        scope: scope.clone(),
        allocation: allocation.clone(),
        activation: activation.clone(),
        source_runtime_dir: source_runtime_dir.to_path_buf(),
        file: file.clone(),
        code,
        journal,
        state,
        closed: false,
        failed: false,
        busy: false,
        size,
        bytes_hash,
        ino,
        birthtime_ms: birthtime,
        in_flight: None,
        issued: HashMap::new(),
        fetch_deadline: None,
        last_guard_failure: V::Null,
        local_timings: Vec::new(),
        inherited_sources,
        source_guard,
        lease,
        deps,
    }));
    {
        let has_state = inner.borrow().state.is_some();
        if !has_state {
            let (sc, al, ac, cs, at) = {
                let g = inner.borrow();
                (
                    g.scope.clone(),
                    g.allocation.clone(),
                    g.activation.clone(),
                    custody_seed.clone(),
                    iso((g.deps.now)()),
                )
            };
            append_event(
                &inner,
                &json!({"kind": "initialize", "scope": sc, "allocation": al, "activation": ac, "custodySeed": cs, "at": at}),
            )?;
        } else {
            let g = inner.borrow();
            let st = g.state.clone().unwrap();
            need(
                same(get(&st, "scope"), scope)?
                    && same(get(&st, "allocation"), allocation)?
                    && same(get(&st, "activation"), activation)?,
                "IMMUTABLE_CONFIGURATION",
            )?;
        }
    }
    let transport = {
        let inner_w = inner.clone();
        let wrap: Box<baal_execution::transport::Dispatch> = Box::new(move |spec, bd, bn| {
            wrap_dispatch(&inner_w, spec, bd, bn).map_err(|e| OrderTransportError {
                code: e.message,
                outcome: "unknown".into(),
                may_have_executed: true,
                retry_same_request_only: false,
                retry_after_ms: 0.0,
                status: None,
                api_code: None,
                semantic_mismatch: false,
            })
        });
        OrderTransport::new(OrderTransportOptions {
            dispatch: wrap,
            scope: baal_execution::transport::scope_of(scope).map_err(|e| e.code)?,
            can_write: Box::new({
                let inner = inner.clone();
                move || inner.borrow().mechanical().is_ok()
            }),
            owns_order: Box::new({
                let inner = inner.clone();
                move |oid: &str| {
                    inner
                        .borrow()
                        .state
                        .as_ref()
                        .and_then(|s| active(s))
                        .map(|o| get(&o, "orderId").as_str() == Some(oid))
                        .unwrap_or(false)
                }
            }),
            validate_permit: Box::new({
                let inner = inner.clone();
                move |p: &V, spec: &V| {
                    validate(&inner, p, spec, false).map_err(|e| {
                        let code = guard_error_code(&e);
                        inner.borrow_mut().last_guard_failure = json!({
                            "stage": "admission",
                            "at": iso((inner.borrow().deps.now)()),
                            "code": code,
                        });
                        OrderTransportError {
                            code: e,
                            outcome: "unknown".into(),
                            may_have_executed: true,
                            retry_same_request_only: false,
                            retry_after_ms: 0.0,
                            status: None,
                            api_code: None,
                            semantic_mismatch: false,
                        }
                    })
                }
            }),
            validate_network_permit: Box::new({
                let inner = inner.clone();
                move |p: &V, spec: &V| {
                    validate(&inner, p, spec, true).map_err(|e| {
                        // v5: a pre-network guard failure on a still-
                        // dispatching, never-fetched place is a proven block —
                        // append network_blocked and roll the order back to
                        // prepared before the failure is recorded and rethrown.
                        let eligible = {
                            let g = inner.borrow();
                            let st = g.state.clone().unwrap_or(V::Null);
                            let o = active(&st).unwrap_or(V::Null);
                            sget(&o, "phase") == Some("dispatching")
                                && get(&o, "orderId").is_null()
                                && get(&o, "fences")
                                    .as_array()
                                    .is_some_and(|x| x.len() <= 1)
                                && get(&o, "fetchStarts")
                                    .as_array()
                                    .is_some_and(|x| x.is_empty())
                                && get(&o, "receipts")
                                    .as_array()
                                    .is_some_and(|x| x.is_empty())
                                && !g.failed
                                && (g.deps.can_operate)()
                        };
                        if eligible {
                            // The transport's spec JSON has no beforeNetwork
                            // function value; the proof only needs non-null.
                            let mut spec_v =
                                spec.as_object().cloned().unwrap_or_default();
                            spec_v.insert("beforeNetwork".into(), json!(true));
                            let recovery = (|| -> Result<(), String> {
                                let proof = blocked_network_proof_v1(&json!({
                                    "spec": V::Object(spec_v),
                                    "error": {"message": e},
                                    "at": iso((inner.borrow().deps.now)()),
                                }))?;
                                let at =
                                    sget(&proof, "at").unwrap_or("").to_string();
                                append_event(
                                    &inner,
                                    &json!({"kind": "network_blocked", "proof": proof, "at": at}),
                                )
                            })();
                            if let Err(e2) = recovery {
                                return OrderTransportError {
                                    code: e2,
                                    outcome: "unknown".into(),
                                    may_have_executed: true,
                                    retry_same_request_only: false,
                                    retry_after_ms: 0.0,
                                    status: None,
                                    api_code: None,
                                    semantic_mismatch: false,
                                };
                            }
                        }
                        let code = guard_error_code(&e);
                        inner.borrow_mut().last_guard_failure = json!({
                            "stage": "network",
                            "at": iso((inner.borrow().deps.now)()),
                            "code": code,
                        });
                        OrderTransportError {
                            code: e,
                            outcome: "unknown".into(),
                            may_have_executed: true,
                            retry_same_request_only: false,
                            retry_after_ms: 0.0,
                            status: None,
                            api_code: None,
                            semantic_mismatch: false,
                        }
                    })
                }
            }),
            validate_lease: Box::new({
                let inner = inner.clone();
                move |l: &V, _s: &V| {
                    Ok(inner
                        .borrow()
                        .mechanical()
                        .and_then(|b| same(l, &b))
                        .unwrap_or(false))
                }
            }),
            now: Box::new({
                let inner = inner.clone();
                move || (inner.borrow().deps.now)() as i64
            }),
            max_pages: 10,
        })
        .map_err(|e| e.code)?
    };
    Ok(BaalGenericNoControllerV9 { inner, transport })
}

impl BaalGenericNoControllerV9 {
    pub fn authorizes(
        &self,
        spec: &V,
        before_dispatch: Option<&Guard<'_>>,
        before_network: Option<&Guard<'_>>,
    ) -> bool {
        let r: Result<bool, String> = (|| {
            self.inner.borrow().mechanical()?;
            let bd_ptr = before_dispatch.map(|x| x as *const Guard as *const () as usize).unwrap_or(0);
            let bn_ptr = before_network.map(|x| x as *const Guard as *const () as usize).unwrap_or(0);
            let g = self.inner.borrow();
            let p = g.issued.get(&bd_ptr).cloned();
            let _ = bn_ptr;
            Ok(p.is_some()
                && get(p.as_ref().unwrap(), "method") == get(spec, "method")
                && get(p.as_ref().unwrap(), "endpoint") == get(spec, "endpoint")
                && get(p.as_ref().unwrap(), "bodyJson") == get(spec, "bodyJson")
                && get(p.as_ref().unwrap(), "beforeNetwork") == &json!(bn_ptr)
                && self.transport.authorize_request(spec).map(|x| x == true).unwrap_or(false))
        })();
        r.unwrap_or(false)
    }

    pub fn on_fetch_start(&self, meta: &V) -> Result<bool, String> {
        if get(meta, "method").as_str() == Some("GET") {
            return Ok(false);
        }
        if self.inner.borrow().in_flight.is_none() {
            return Ok(false);
        }
        let r: Result<bool, String> = (|| {
            self.inner.borrow().basic(false)?;
            let (o, f, spec) = {
                let g = self.inner.borrow();
                let st = g.state.clone().unwrap_or(V::Null);
                let o = active(&st).unwrap_or(V::Null);
                let f = get(&o, "fences")
                    .as_array()
                    .and_then(|x| x.last())
                    .cloned()
                    .unwrap_or(V::Null);
                let spec = g.in_flight.as_ref().map(|i| i.spec.clone()).unwrap_or(V::Null);
                (o, f, spec)
            };
            need(
                !f.is_null()
                    && get(meta, "method") == get(&spec, "method")
                    && get(meta, "endpoint") == get(&spec, "endpoint")
                    && get(meta, "requestBodyHash")
                        == &(if get(meta, "method").as_str() == Some("POST") {
                            get(&o, "bodyHash").clone()
                        } else {
                            V::Null
                        }),
                "FETCH_UNBOUND",
            )?;
            let mut start = meta.as_object().cloned().unwrap_or_default();
            start.insert("kind".into(), json!("place"));
            start.insert("ownerId".into(), get(&self.inner.borrow().scope, "ownerId").clone());
            start.insert("runId".into(), get(&self.inner.borrow().scope, "runId").clone());
            start.insert(
                "tournamentId".into(),
                get(&self.inner.borrow().scope, "tournamentId").clone(),
            );
            start.insert("intentId".into(), get(&o, "intentId").clone());
            start.insert(
                "idempotencyKey".into(),
                get(get(&o, "body"), "idempotencyKey").clone(),
            );
            start.insert("requestBodyJson".into(), get(&o, "bodyJson").clone());
            start.insert("requestBodyHash".into(), get(&o, "bodyHash").clone());
            start.insert("leaseIdentity".into(), get(&f, "leaseIdentity").clone());
            start.insert("leaseHash".into(), get(&f, "leaseHash").clone());
            append_event(
                &self.inner,
                &json!({"kind": "fetch_start", "start": V::Object(start), "at": iso((self.inner.borrow().deps.now)())}),
            )?;
            Ok(true)
        })();
        match r {
            Ok(v) => Ok(v),
            Err(e) => {
                self.inner.borrow_mut().failed = true;
                Err(format!("{e}:fatal:durableRecordingFailed:mayHaveExecuted"))
            }
        }
    }

    // onBeforeFetch(meta): physical-journal re-verify + fetchDeadline fence.
    // v5 wraps the whole check: a failure is recorded as lastGuardFailure
    // {stage:'physical',at,deadlineRemainingMs,code} before rethrowing.
    pub fn on_before_fetch(&self, meta: &V) -> Result<V, String> {
        match self.on_before_fetch_inner(meta) {
            Ok(v) => Ok(v),
            Err(e) => {
                let (now, deadline) = {
                    let g = self.inner.borrow();
                    ((g.deps.now)(), g.fetch_deadline)
                };
                let code = guard_error_code(&e);
                let at_iso = iso(now);
                self.inner.borrow_mut().last_guard_failure = json!({
                    "stage": "physical",
                    "at": at_iso,
                    "deadlineRemainingMs": deadline.map(|d| json!(d as f64 - now)).unwrap_or(V::Null),
                    "code": code,
                });
                // v9: on an affirmative before-actual-fetch clock block the
                // still-dispatching place is rolled back to prepared with the
                // proof appended (e.code === e.message for need errors).
                if e == "BAAL_SCALED_PHYSICAL_FETCH_FENCE" {
                    let (o, in_flight, remaining, can_op) = {
                        let g = self.inner.borrow();
                        let st = g.state.clone().unwrap_or(V::Null);
                        (
                            active(&st),
                            g.in_flight.as_ref().map(|i| i.spec.clone()),
                            get(&g.last_guard_failure, "deadlineRemainingMs")
                                .as_f64()
                                .map(|x| x < 0.0)
                                .unwrap_or(false),
                            (g.deps.can_operate)(),
                        )
                    };
                    let o = o.unwrap_or(V::Null);
                    if remaining
                        && sget(&o, "phase") == Some("dispatching")
                        && get(&o, "fences")
                            .as_array()
                            .is_some_and(|x| x.len() == 1)
                        && get(&o, "orderId").is_null()
                        && get(&o, "ack").is_null()
                        && get(&o, "fetchStarts")
                            .as_array()
                            .is_some_and(|x| x.is_empty())
                        && get(&o, "receipts")
                            .as_array()
                            .is_some_and(|x| x.is_empty())
                        && in_flight.is_some()
                        && can_op
                    {
                        let proof = physical_fetch_block_proof_v1(&json!({
                            "spec": in_flight,
                            "code": "BAAL_SCALED_PHYSICAL_FETCH_FENCE",
                            "at": get(&self.inner.borrow().last_guard_failure, "at"),
                        }))?;
                        append_event(
                            &self.inner,
                            &json!({
                                "kind": "physical_fetch_blocked",
                                "proof": proof,
                                "at": get(&proof, "at"),
                            }),
                        )?;
                    }
                }
                Err(e)
            }
        }
    }

    fn on_before_fetch_inner(&self, meta: &V) -> Result<V, String> {
        self.inner.borrow().basic(true)?;
        reserved(&self.inner)?;
        let (file, size, bytes_hash, ino, birthtime_ms) = {
            let g = self.inner.borrow();
            (
                g.file.clone(),
                g.size,
                g.bytes_hash.clone(),
                g.ino,
                g.birthtime_ms,
            )
        };
        let fresh = fs::read(&file).map_err(|e| e.to_string())?;
        let st = fs::metadata(&file).map_err(|e| e.to_string())?;
        need(
            st.ino() == ino
                && stat_birthtime_ms(&st) == birthtime_ms
                && fresh.len() as u64 == size
                && sha_bytes(&fresh) == bytes_hash,
            "PHYSICAL_JOURNAL_CHANGED",
        )?;
        let (o, spec, fetch_deadline) = {
            let g = self.inner.borrow();
            let st = g.state.clone().unwrap_or(V::Null);
            (
                active(&st),
                g.in_flight.as_ref().map(|i| i.spec.clone()).unwrap_or(V::Null),
                g.fetch_deadline,
            )
        };
        let o = o.unwrap_or(V::Null);
        let now = (self.inner.borrow().deps.now)();
        need(
            !spec.is_null()
                && !o.is_null()
                && get(meta, "method").as_str() == Some("POST")
                && get(meta, "endpoint").as_str() == Some("/orders")
                && get(meta, "requestBodyHash") == get(&o, "bodyHash")
                && get(&spec, "bodyJson") == get(&o, "bodyJson")
                && fetch_deadline.is_some()
                && (now as i64) < fetch_deadline.unwrap_or(i64::MIN),
            "PHYSICAL_FETCH_FENCE",
        )?;
        Ok(json!({"notAfter": fetch_deadline}))
    }

    pub fn status(&self) -> V {
        status_of(&self.inner)
    }

    pub fn journal_source(&self) -> Result<V, String> {
        let g = self.inner.borrow();
        let bytes = fs::read(&g.file).map_err(|e| e.to_string())?;
        let nl = bytes.iter().position(|b| *b == 10).unwrap_or(0);
        let first = &bytes[..nl + 1];
        let row: V = serde_json::from_str(&String::from_utf8_lossy(first))
            .map_err(|_| "BAAL_SCALED_JOURNAL".to_string())?;
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
            "record": reservation(&g.state.clone().unwrap_or(V::Null))?,
            "status": status_of(&self.inner),
        }))
    }

    pub fn order_binding(&self, id: &str) -> V {
        let i = self.status().get("intent").cloned().unwrap_or(V::Null);
        if get(&i, "orderId").as_str() == Some(id) {
            i
        } else {
            V::Null
        }
    }

    pub fn prepare_buy(&self) -> Result<V, String> {
        self.prepare("buy", None, None)
    }

    pub fn prepare_sell(&self, exchange_id: &str, quantity: Option<i64>) -> Result<V, String> {
        self.prepare("sell", Some(exchange_id), quantity)
    }

    fn prepare(&self, action: &str, ex: Option<&str>, quantity: Option<i64>) -> Result<V, String> {
        run(&self.inner, || {
            self.inner.borrow().basic(false)?;
            need(
                active(&self.inner.borrow().state.clone().unwrap_or(V::Null)).is_none(),
                "UNKNOWN_OR_OPEN_INTENT",
            )?;
            reserve(&self.inner)?;
            let market_id = ex
                .and_then(|ex| {
                    get(&self.inner.borrow().scope, "exchanges")
                        .as_array()
                        .and_then(|xs| {
                            xs.iter()
                                .find(|m| sget(m, "exchangeId") == Some(ex))
                                .and_then(|m| m.get("marketId"))
                                .cloned()
                        })
                })
                .unwrap_or(V::Null);
            let e = truthy_sync((self.inner.borrow().deps.get_evidence)(&json!({
                "operation": format!("prepare_{}", action),
                "exchangeId": ex.map(|e| json!(e)).unwrap_or(V::Null),
                "marketId": market_id,
            }))?)?;
            let g = current_risk(&self.inner)?;
            append_event(
                &self.inner,
                &json!({
                    "kind": "prepared",
                    "action": action,
                    "exchangeId": ex.map(|e| json!(e)).unwrap_or(V::Null),
                    "quantity": quantity.map(|q| json!(q)).unwrap_or(V::Null),
                    "intentId": format!("baal-scaled-{}", uuid::Uuid::new_v4()),
                    "idempotencyKey": uuid::Uuid::new_v4().to_string(),
                    "evidence": e,
                    "globalRisk": g,
                    "at": iso((self.inner.borrow().deps.now)()),
                }),
            )?;
            reserve(&self.inner)?;
            Ok(self.status())
        })
    }

    fn send(&self, operation: &str) -> Result<V, String> {
        run(&self.inner, || {
            self.inner.borrow().basic(false)?;
            let o = active(&self.inner.borrow().state.clone().unwrap_or(V::Null))
                .ok_or_else(|| "BAAL_SCALED_UNKNOWN_BLOCKS_REPLACEMENT".to_string())?;
            need(
                if operation == "place" {
                    sget(&o, "phase") == Some("prepared")
                } else {
                    sget(&o, "orderId").is_some() && get(&o, "cancelPending") != &V::Bool(true)
                },
                "UNKNOWN_BLOCKS_REPLACEMENT",
            )?;
            if operation == "place" && sget(get(&o, "body"), "action") == Some("sell") {
                reserved(&self.inner)?;
            } else {
                reserve(&self.inner)?;
            }
            let permit = json!({
                "intentId": get(&o, "intentId"),
                "bodyHash": get(&o, "bodyHash"),
                "operation": operation,
            });
            let l = self.inner.borrow().basic(false)?;
            let result = if operation == "place" {
                self.transport.place(get(&o, "body"), &json!({"permit": permit, "lease": l}))
            } else {
                self.transport
                    .cancel(get(&o, "orderId"), &json!({"permit": permit, "lease": l}))
            };
            if let Err(e) = result {
                if self.inner.borrow().failed {
                    return Err(
                        "BAAL_SCALED_DURABLE_FAILURE:fatal:durableRecordingFailed:mayHaveExecuted"
                            .to_string(),
                    );
                }
                if (self.inner.borrow().deps.can_operate)() {
                    let code = if !e.code.is_empty()
                        && e.code.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
                    {
                        e.code.clone()
                    } else {
                        "REQUEST_UNCONFIRMED".to_string()
                    };
                    append_event(
                        &self.inner,
                        &json!({"kind": "error", "code": code, "at": iso((self.inner.borrow().deps.now)())}),
                    )?;
                }
                let uncertain =
                    active(&self.inner.borrow().state.clone().unwrap_or(V::Null));
                if let Some(u) = uncertain {
                    if ["uncertain", "semantic_mismatch"]
                        .contains(&sget(&u, "phase").unwrap_or(""))
                    {
                        truthy_sync((self.inner.borrow().deps.on_financial_halt)(&json!({
                            "kind": "unknown_generic_execution",
                            "sourceHash": get(&u, "bodyHash"),
                            "allWritersStop": true,
                        }))?)?;
                    }
                }
                return Err(e.code);
            }
            Ok(self.status())
        })
    }

    pub fn dispatch_prepared(&self) -> Result<V, String> {
        self.send("place")
    }

    pub fn cancel_expired(&self) -> Result<V, String> {
        self.send("cancel")
    }

    pub fn abandon_prepared(&self) -> Result<V, String> {
        run(&self.inner, || {
            self.inner.borrow().basic(false)?;
            need(self.inner.borrow().in_flight.is_none(), "ABANDON_IN_FLIGHT")?;
            append_event(
                &self.inner,
                &json!({"kind": "abandoned_prepared", "at": iso((self.inner.borrow().deps.now)())}),
            )?;
            reserve(&self.inner)?;
            Ok(self.status())
        })
    }

    pub fn refresh_reservation(&self) -> Result<V, String> {
        run(&self.inner, || {
            self.inner.borrow().basic(false)?;
            reserve(&self.inner)?;
            Ok(self.status())
        })
    }

    pub fn funding_reservation(&self) -> V {
        reservation(&self.inner.borrow().state.clone().unwrap_or(V::Null)).unwrap_or(V::Null)
    }

    pub fn funding_reservation_status(&self) -> V {
        let g = self.inner.borrow();
        let st = g.state.clone().unwrap_or(V::Null);
        json!({
            "record": reservation(&st).unwrap_or(V::Null),
            "receipt": get(&st, "reservation"),
        })
    }

    pub fn inventory(&self, exchange_id: &str) -> V {
        let g = self.inner.borrow();
        get(get(&g.state.clone().unwrap_or(V::Null), "inventory"), exchange_id).clone()
    }

    pub fn reconcile(&self, evidence: &V) -> Result<V, String> {
        run(&self.inner, || {
            self.inner.borrow().basic(false)?;
            let g = current_risk(&self.inner)?;
            {
                let st = self.inner.borrow().state.clone().unwrap_or(V::Null);
                risk(&st, &g, (self.inner.borrow().deps.now)() as i64, true)?;
            }
            let mut ev = evidence.as_object().cloned().unwrap_or_default();
            ev.insert("globalRisk".into(), g);
            append_event(
                &self.inner,
                &json!({"kind": "reconciled", "evidence": V::Object(ev), "at": iso((self.inner.borrow().deps.now)())}),
            )?;
            reserve(&self.inner)?;
            Ok(self.status())
        })
    }

    pub fn close(&self) -> Result<(), String> {
        need(!self.inner.borrow().busy, "BUSY")?;
        {
            let mut g = self.inner.borrow_mut();
            if g.closed {
                return Ok(());
            }
            g.closed = true;
            g.lease.release().map_err(|e| e.message)?;
        }
        Ok(())
    }
}

// replayBaalGenericNoJournalV4({bodyText,expectedCodeIdentity,custodySeed})
pub fn replay_baal_generic_no_journal_v9(
    body_text: &str,
    expected_code_identity: &V,
    custody_seed: &V,
) -> Result<V, String> {
    need(
        body_text.ends_with('\n') && body_text.len() <= 128 * 1024 * 1024,
        "REPLAY_INPUT",
    )?;
    let mut state: Option<V> = None;
    let mut sequence: i64 = 0;
    let mut previous = String::new();
    for line in body_text.trim_end().split('\n') {
        let r: V = serde_json::from_str(line).map_err(|_| "BAAL_SCALED_REPLAY_CHAIN".to_string())?;
        let mut body = r.as_object().cloned().unwrap_or_default();
        let hash = body.shift_remove("hash").unwrap_or(V::Null);
        need(
            js_stringify(&r) == line
                && js_int(get(&V::Object(body.clone()), "sequence")) == Some(sequence + 1)
                && get(&V::Object(body.clone()), "previous").as_str() == Some(previous.as_str())
                && sha_str(&js_stringify(&V::Object(body.clone()))) == hash.as_str().unwrap_or(""),
            "REPLAY_CHAIN",
        )?;
        sequence += 1;
        let body = V::Object(body);
        need(
            same(get(&get(&body, "event"), "codeIdentity"), expected_code_identity)?,
            "REPLAY_CHAIN",
        )?;
        if sequence == 1 {
            need(
                sget(&get(&get(&body, "event"), "input"), "kind") == Some("initialize")
                    && same(
                        get(&get(&get(&body, "event"), "input"), "custodySeed"),
                        custody_seed,
                    )?,
                "REPLAY_EXACT_CUSTODY",
            )?;
            configuration(
                get(&get(&get(&body, "event"), "input"), "scope"),
                get(&get(&get(&body, "event"), "input"), "allocation"),
                get(&get(&get(&body, "event"), "input"), "activation"),
                expected_code_identity,
            )?;
        }
        state = Some(reduce(state, get(&get(&body, "event"), "input"))?);
        let proj = get(&body, "state");
        need(
            sget(proj, "mode") == Some("baal_generic_no_event_projection_v9")
                && get(proj, "economicStateHash").as_str()
                    == Some(payload_hash(state.as_ref().unwrap())?.as_str()),
            "REPLAY_ECONOMICS",
        )?;
        previous = hash.as_str().unwrap_or("").to_string();
    }
    Ok(json!({
        "state": state.unwrap_or(V::Null),
        "anchor": {
            "bytes": body_text.len(),
            "sha256": sha_str(body_text),
            "sequence": sequence,
            "hash": previous,
        },
        "economicReplayPerformed": true,
    }))
}
