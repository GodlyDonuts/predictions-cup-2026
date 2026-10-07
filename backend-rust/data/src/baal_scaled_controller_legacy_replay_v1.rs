// execution/baal-scaled-controller-legacy-replay-v1.mjs — sealed replay of the
// legacy baal_scaled_no_controller_v1 journal. Exact-byte chain verification;
// the reducer is re-ported here verbatim (errors carry the same code strings).
use baal_collector::model::normalize_book;
use baal_execution::baal_scaled_entry_signal_v1::plan_baal_scaled_entry;
use baal_execution::core::{canonical_payload, payload_hash};
use baal_execution::gates::normalize_official_fill_tick;
use baal_execution::no_rest_multishare_reconciliation_v1::verify_scoped_rest_no_multishare;
use baal_execution::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account;
use baal_execution::receipts::parse_number_lexemes;
use baal_service_runtime::contract::{instant_ns, ms_to_iso};
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use serde_json::{json, Map, Value as V};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;

fn need(x: bool, c: &str) -> Result<(), String> {
    if x {
        Ok(())
    } else {
        Err(format!("BAAL_SCALED_{c}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    static NULL: V = V::Null;
    v.as_object().and_then(|m| m.get(k)).unwrap_or(&NULL)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    get(v, k).as_str()
}
fn hex(x: &V) -> bool {
    x.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn sha(s: &str) -> String {
    sha256(s.as_bytes())
}
fn sha_bytes(b: &[u8]) -> String {
    sha256(b)
}
fn iso(n: i64) -> String {
    ms_to_iso(n as f64)
}
fn instant_v(v: &V) -> Result<i64, String> {
    // JS instant(value) throws 'Timestamp must be canonical UTC ISO milliseconds'
    // on non-strings (the first check fails before parsing).
    instant(
        v.as_str()
            .ok_or_else(|| "Timestamp must be canonical UTC ISO milliseconds".to_string())?,
    )
}
fn js_str(v: &V) -> String {
    v.as_str()
        .map(String::from)
        .or_else(|| v.as_i64().map(|x| x.to_string()))
        .or_else(|| v.as_u64().map(|x| x.to_string()))
        .or_else(|| v.as_f64().map(|x| js_stringify(&json!(x))))
        .unwrap_or_default()
}
fn num_id(v: &V) -> f64 {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
        .unwrap_or(f64::NAN)
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(payload_hash(a)? == payload_hash(b)?)
}
fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}
fn sorted(a: &V) -> V {
    let mut v = a.as_array().cloned().unwrap_or_default();
    v.sort_by(|x, y| num_id(get(x, "id")).partial_cmp(&num_id(get(y, "id"))).unwrap_or(std::cmp::Ordering::Equal));
    json!(v)
}
fn active(s: &V) -> Option<V> {
    get(s, "orders")
        .as_array()
        .and_then(|a| a.iter().find(|o| get(o, "proof").is_null() && sget(o, "phase") != Some("abandoned")).cloned())
}
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
fn raw(r: &V, method: &str, endpoint: &str) -> Result<V, String> {
    need(
        sget(r, "method") == Some(method)
            && sget(r, "endpoint") == Some(endpoint)
            && js_int(get(r, "status")).is_some()
            && get(r, "bodyText").as_str().is_some_and(|s| s.as_bytes().len() <= 8 * 1024 * 1024)
            && sget(r, "sourceBodyHash") == Some(sha(sget(r, "bodyText").unwrap_or("")).as_str())
            && get(r, "fetchInvocationId").is_string(),
        "RAW_RECEIPT",
    )?;
    need(
        instant_v(get(r, "requestStartedAt"))? <= instant_v(get(r, "receivedAt"))?,
        "RAW_CLOCK",
    )?;
    let mut out = Map::new();
    out.insert("method".into(), json!(method));
    out.insert("endpoint".into(), json!(endpoint));
    out.insert("status".into(), get(r, "status").clone());
    out.insert("bodyText".into(), get(r, "bodyText").clone());
    out.insert("requestStartedAt".into(), get(r, "requestStartedAt").clone());
    out.insert("receivedAt".into(), get(r, "receivedAt").clone());
    out.insert("sourceBodyHash".into(), get(r, "sourceBodyHash").clone());
    out.insert("fetchInvocationId".into(), get(r, "fetchInvocationId").clone());
    if let Some(h) = get(r, "requestBodyHash").as_str() {
        out.insert("requestBodyHash".into(), json!(h));
    }
    Ok(V::Object(out))
}

fn configuration(scope: &V, allocation: &V, activation: &V, code: &V) -> Result<(), String> {
    need(
        sget(scope, "ownerId") == Some("Codex")
            && sget(scope, "runId").is_some_and(|s| !s.is_empty())
            && get(scope, "exchanges").as_array().is_some_and(|a| !a.is_empty())
            && {
                let ids: HashSet<&str> = get(scope, "exchanges")
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|x| sget(x, "exchangeId"))
                    .collect();
                ids.len() == get(scope, "exchanges").as_array().unwrap().len()
            },
        "SCOPE",
    )?;
    need(
        js_int(get(allocation, "schemaVersion")) == Some(1)
            && get(allocation, "ownerId") == get(scope, "ownerId")
            && get(allocation, "runId") == get(scope, "runId")
            && get(allocation, "tournamentId") == get(scope, "tournamentId")
            && sget(allocation, "capital") == Some("1000.000000")
            && hex(get(allocation, "humanAllocationSourceHash")),
        "ALLOCATION",
    )?;
    for (k, max) in [("maximumOrderCost", 25_000_000i128), ("maximumMarketCost", 100_000_000i128), ("maximumEventCost", 200_000_000i128)] {
        let v = match get(allocation, k) {
            x if x.is_null() => max,
            x => amount(x.as_str().unwrap_or(""))?,
        };
        need(v > 0 && v <= max, "ALLOCATION_CEILING")?;
    }
    need(
        js_int(&{
            let v = get(allocation, "maximumSharesPerOrder");
            if v.is_null() { json!(100) } else { v.clone() }
        })
        .is_some_and(|v| v > 0 && v <= 100),
        "SHARE_CEILING",
    )?;
    need(
        js_int(&{
            let v = get(allocation, "maximumActiveMarkets");
            if v.is_null() { json!(20) } else { v.clone() }
        })
        .is_some_and(|v| v > 0 && v <= 20),
        "ACTIVE_MARKET_CEILING",
    )?;
    need(
        js_int(get(activation, "schemaVersion")) == Some(1)
            && sget(activation, "mode") == Some("baal_scaled_no_production_v1")
            && ["ownerId", "runId", "tournamentId"].iter().all(|k| get(activation, k) == get(scope, k))
            && sget(activation, "allocationHash") == Some(payload_hash(allocation)?.as_str())
            && sget(activation, "scopeHash") == Some(payload_hash(scope)?.as_str())
            && sget(activation, "codeIdentityHash") == Some(payload_hash(code)?.as_str())
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

fn reservation(s: &V) -> Result<V, String> {
    let mut owned: Vec<V> = Vec::new();
    let mut reservations: Vec<V> = Vec::new();
    // add(kind, leg, eventId, n, suffix)
    let add = |kind: &str, leg: &V, event_id: &V, n: i128, suffix: &str,
                   owned: &mut Vec<V>, reservations: &mut Vec<V>| {
        if n > 0 {
            let field = if kind == "owned" { "nominalCost" } else { "maximumNominalCost" };
            let row = json!({
                "exposureId": format!("scaled:{}:{}:{}", sget(s, "scope").and_then(|_| sget(get(s, "scope"), "runId")).unwrap_or(""), sget(leg, "exchangeId").unwrap_or(""), suffix),
                "marketId": get(leg, "marketId"),
                "eventId": event_id,
            });
            let mut m = row.as_object().cloned().unwrap_or_default();
            m.insert(field.into(), json!(money(n)));
            if kind == "owned" {
                owned.push(V::Object(m));
            } else {
                reservations.push(V::Object(m));
            }
        }
    };
    if let Some(inv) = get(s, "inventory").as_object() {
        for (ex, slot) in inv {
            let q = js_int(get(get(slot, "proof"), "remainingOwnedQuantity")).unwrap_or(0);
            if q == 0 {
                continue;
            }
            let legs = get(get(slot, "package"), "legs").as_array().cloned().unwrap_or_default();
            let first = legs.iter().find(|l| sget(l, "exchangeId") == Some(ex.as_str())).cloned().unwrap_or(V::Null);
            let other = legs.iter().find(|l| sget(l, "exchangeId") != Some(ex.as_str())).cloned().unwrap_or(V::Null);
            let cost = amount(get(get(slot, "proof"), "remainingOwnedCost").as_str().unwrap_or(""))?;
            add("owned", &first, get(get(slot, "package"), "eventId"), cost, "owned", &mut owned, &mut reservations);
            add("reserved", &other, get(get(slot, "package"), "eventId"), 990_000i128 * q as i128 - cost, &format!("hedge-for-{ex}"), &mut owned, &mut reservations);
        }
    }
    let o = active(s);
    if sget(&o.clone().unwrap_or(V::Null), "body").is_some()
        && sget(get(&o.clone().unwrap_or(V::Null), "body"), "action") == Some("buy")
    {
        let oo = o.clone().unwrap();
        let legs = get(get(&oo, "package"), "legs").as_array().cloned().unwrap_or_default();
        let bex = sget(get(&oo, "body"), "exchangeId").unwrap_or("");
        let first = legs.iter().find(|l| sget(l, "exchangeId") == Some(bex)).cloned().unwrap_or(V::Null);
        let other = legs.iter().find(|l| sget(l, "exchangeId") != Some(bex)).cloned().unwrap_or(V::Null);
        let qty = js_int(get(get(&oo, "body"), "quantity")).unwrap_or(0) as i128;
        let entry = price(&js_str(get(get(&oo, "body"), "price")))? * qty;
        add("reserved", &first, get(get(&oo, "package"), "eventId"), entry, "entry", &mut owned, &mut reservations);
        add(
            "reserved",
            &other,
            get(get(&oo, "package"), "eventId"),
            990_000i128 * qty - entry,
            &format!("hedge-for-{}", sget(&first, "exchangeId").unwrap_or("")),
            &mut owned,
            &mut reservations,
        );
    }
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
        json!(o.as_ref().map(|v| vec![get(v, "intentId").clone()]).unwrap_or_default()),
    );
    body.insert(
        "cashReserved".into(),
        json!(money(reservations.iter().map(|r| amount(sget(r, "maximumNominalCost").unwrap_or("")).unwrap_or(0)).sum::<i128>())),
    );
    body.insert(
        "ownedPrincipal".into(),
        json!(money(owned.iter().map(|r| amount(sget(r, "nominalCost").unwrap_or("")).unwrap_or(0)).sum::<i128>())),
    );
    let body = V::Object(body);
    let h = payload_hash(&body)?;
    let mut out = body.as_object().cloned().unwrap_or_default();
    out.insert("reservationHash".into(), json!(h));
    Ok(V::Object(out))
}

// projectBaalScaledReservation(state)
pub fn project_baal_scaled_reservation(state: &V) -> Result<V, String> {
    need(sget(state, "mode") == Some("baal_scaled_no_controller_v1"), "PROJECTION_MODE")?;
    reservation(state)
}

fn risk(s: &V, g: &V, at: i64, cleanup: bool) -> Result<(), String> {
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
            && get(g, "allocationHash").as_str() == Some(payload_hash(get(s, "allocation"))?.as_str())
            && get(g, "budgetInstalledHash") == get(get(s, "activation"), "budgetInstalledHash"),
        "CURRENT_GLOBAL_RISK",
    )?;
    let o = active(s);
    let oi = o.as_ref().map(|v| get(v, "intentId").clone()).unwrap_or(V::Null);
    need(
        get(g, "unresolvedIntentIds").as_array().is_some_and(|x| x.len() <= 1)
            && get(g, "unresolvedIntentIds")
                .as_array()
                .is_some_and(|ids| ids.iter().all(|x| x == &oi))
            && (cleanup
                || get(g, "unknownAcceptance") == &V::Bool(false)
                || (get(g, "unresolvedIntentIds").as_array().is_some_and(|x| x.len() == 1)
                    && ["prepared", "dispatching"].contains(&o.as_ref().and_then(|v| sget(v, "phase")).unwrap_or("")))),
        "GLOBAL_UNKNOWN",
    )?;
    Ok(())
}

fn budget(s: &V, g: &V, next: &V, at: i64, cash: bool) -> Result<(), String> {
    risk(s, g, at, false)?;
    let old = reservation(s)?;
    need(
        get(g, "controllerReservationHash").as_str() == Some(sget(&old, "reservationHash").unwrap_or(""))
            && hex(get(g, "globalLedgerHash"))
            && get(g, "globalLedgerHash") == get(get(s, "reservation"), "globalLedgerHash"),
        "GLOBAL_RESERVATION_BINDING",
    )?;
    let mut rows: Vec<(String, V, i128)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (k, f) in [("owned", "nominalCost"), ("reservations", "maximumNominalCost"), ("loans", "fullNominalPrincipal")] {
        need(get(g, k).is_array(), "GLOBAL_EXPOSURES")?;
        for r in get(g, k).as_array().cloned().unwrap_or_default() {
            let eid = sget(&r, "exposureId").unwrap_or("").to_string();
            need(
                !eid.is_empty()
                    && !seen.contains(&eid)
                    && get(&r, "marketId").is_string()
                    && sget(&r, "eventId").is_some_and(|s| !s.is_empty())
                    && amount(sget(&r, f).unwrap_or(""))? >= 0
                    && get(&r, "unknownAcceptance") != &V::Bool(true),
                "GLOBAL_EXPOSURE",
            )?;
            seen.insert(eid);
            rows.push((k.to_string(), r.clone(), amount(sget(&r, f).unwrap_or(""))?));
        }
    }
    for (k, f) in [("owned", "nominalCost"), ("reservations", "maximumNominalCost")] {
        for r in get(&old, k).as_array().cloned().unwrap_or_default() {
            let pos = rows.iter().position(|(rk, v, _)| {
                rk == k && get(v, "exposureId") == get(&r, "exposureId") && same(v, &r).unwrap_or(false)
            });
            need(pos.is_some(), "OWN_GLOBAL_EXPOSURE_MISSING")?;
            rows.remove(pos.unwrap());
        }
        let _ = f;
    }
    for (k, f) in [("owned", "nominalCost"), ("reservations", "maximumNominalCost")] {
        for r in get(next, k).as_array().cloned().unwrap_or_default() {
            rows.push((k.to_string(), r.clone(), amount(sget(&r, f).unwrap_or(""))?));
        }
    }
    let mut total: i128 = 0;
    let mut reserved: i128 = 0;
    let mut markets: BTreeMap<String, i128> = BTreeMap::new();
    let mut events: BTreeMap<String, i128> = BTreeMap::new();
    let mut market_events: BTreeMap<String, V> = BTreeMap::new();
    for (kind, v, cost) in &rows {
        let mid = sget(v, "marketId").unwrap_or("").to_string();
        need(
            !market_events.contains_key(&mid) || market_events.get(&mid) == Some(get(v, "eventId")),
            "MARKET_EVENT_CHANGED",
        )?;
        market_events.insert(mid.clone(), get(v, "eventId").clone());
        total += cost;
        if kind == "reservations" {
            reserved += cost;
        }
        *markets.entry(mid).or_insert(0) += cost;
        *events.entry(sget(v, "eventId").unwrap_or("").to_string()).or_insert(0) += cost;
    }
    need(
        total <= 1_000_000_000i128
            && markets.values().filter(|v| **v > 0).count()
                <= js_int(&{
                    let v = get(get(s, "allocation"), "maximumActiveMarkets");
                    if v.is_null() { json!(20) } else { v.clone() }
                })
                .unwrap_or(0) as usize
            && markets.values().all(|v| {
                *v <= amount(sget(get(s, "allocation"), "maximumMarketCost").unwrap_or("100.000000")).unwrap_or(i128::MAX)
            })
            && events.values().all(|v| {
                *v <= amount(sget(get(s, "allocation"), "maximumEventCost").unwrap_or("200.000000")).unwrap_or(i128::MAX)
            }),
        "CAPITAL_OR_EVENT_CEILING",
    )?;
    need(
        sget(g, "cashLower").is_some()
            && amount(sget(g, "cashLower").unwrap_or(""))? >= 0
            && (!cash || reserved <= amount(sget(g, "cashLower").unwrap_or(""))?),
        "GLOBAL_CASH_FUNDING",
    )?;
    Ok(())
}

fn continue_owned(s: &V, ex: &str, a: &V, at: i64) -> Result<V, String> {
    let slot = get(get(s, "inventory"), ex);
    let member = get(get(s, "scope"), "exchanges")
        .as_array()
        .and_then(|ms| ms.iter().find(|x| sget(x, "exchangeId") == Some(ex)).cloned());
    need(member.is_some(), "MARKET_SCOPE")?;
    let member = member.unwrap();
    let mut binding = get(s, "scope").as_object().cloned().unwrap_or_default();
    binding.insert("marketId".into(), get(&member, "marketId").clone());
    binding.insert("exchangeId".into(), get(&member, "exchangeId").clone());
    let current = inspect_scoped_rest_no_account(&json!({
        "account": own_account(a, ex),
        "binding": V::Object(binding.clone()),
        "readAt": iso(at),
    }))?;
    let p = get(&current, "positions")
        .as_array()
        .and_then(|rows| rows.iter().find(|x| sget(x, "exchangeId") == Some(ex)).cloned())
        .unwrap_or(V::Null);
    need(
        p.is_null() || (get(&p, "marketId") == get(&member, "marketId") && get(&p, "settled") == &V::Bool(false)),
        "POSITION_SCOPE",
    )?;
    if !slot.is_null() {
        let mut binding2 = get(s, "scope").as_object().cloned().unwrap_or_default();
        binding2.insert("marketId".into(), get(&member, "marketId").clone());
        binding2.insert("exchangeId".into(), get(&member, "exchangeId").clone());
        let old = inspect_scoped_rest_no_account(&json!({
            "account": get(slot, "after"),
            "binding": V::Object(binding2),
            "readAt": get(get(slot, "proof"), "readAt"),
        }))?;
        let pq = js_int(get(&p, "quantity")).unwrap_or(0);
        need(
            instant_v(get(get(slot, "proof"), "readAt"))? <= instant_v(get(&current, "startedAt"))?
                && same(&sorted(get(&current, "scopedOrders")), &sorted(get(&old, "scopedOrders")))?
                && pq == -js_int(get(get(slot, "proof"), "remainingOwnedQuantity")).unwrap_or(0),
            "OWNED_HISTORY_OR_QUANTITY",
        )?;
    } else {
        need(js_int(get(&p, "quantity")).unwrap_or(0) == 0, "FOREIGN_LOT")?;
    }
    Ok(current)
}

fn native_entry(s: &V, e: &V, at: i64) -> Result<V, String> {
    let mut account_risk = get(e, "accountRisk").clone();
    let o = active(s);
    if let Some(o2) = &o {
        if sget(get(o2, "body"), "action") == Some("buy")
            && ["prepared", "dispatching"].contains(&sget(o2, "phase").unwrap_or(""))
            && get(o2, "fetchStarts").as_array().is_some_and(|f| f.is_empty())
        {
            let full = reservation(s)?;
            let mut s2 = s.as_object().cloned().unwrap_or_default();
            let filtered: Vec<V> = get(s, "orders")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|v| get(v, "intentId") != get(o2, "intentId"))
                .collect();
            s2.insert("orders".into(), json!(filtered));
            let without = reservation(&V::Object(s2))?;
            let own: Vec<V> = get(&full, "reservations")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|r| {
                    !get(&without, "reservations").as_array().is_some_and(|ws| {
                        ws.iter().any(|v| get(v, "exposureId") == get(r, "exposureId"))
                    })
                })
                .collect();
            let rows = get(&account_risk, "reservations");
            let ids = get(&account_risk, "unresolvedIntentIds");
            if rows.is_array()
                && ids.is_array()
                && (ids.as_array().unwrap().iter().any(|x| x == get(o2, "intentId"))
                    || own.iter().any(|r| {
                        rows.as_array()
                            .unwrap()
                            .iter()
                            .any(|v| get(v, "exposureId") == get(r, "exposureId"))
                    }))
            {
                need(
                    get(&account_risk, "unknownAcceptance") == &V::Bool(false)
                        && ids.as_array().unwrap().iter().all(|id| id == get(o2, "intentId"))
                        && own.iter().all(|r| {
                            rows.as_array()
                                .unwrap()
                                .iter()
                                .any(|v| same(v, r).unwrap_or(false))
                        }),
                    "PREPARED_RISK_BINDING",
                )?;
                let original_risk_hash = payload_hash(&account_risk)?;
                let mut nr = account_risk.as_object().cloned().unwrap_or_default();
                let nr_rows: Vec<V> = rows
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|r| !own.iter().any(|v| get(v, "exposureId") == get(r, "exposureId")))
                    .collect();
                let nr_ids: Vec<V> = ids
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|id| id != get(o2, "intentId"))
                    .collect();
                nr.insert("reservations".into(), json!(nr_rows));
                nr.insert("unresolvedIntentIds".into(), json!(nr_ids));
                nr.insert(
                    "sourceHash".into(),
                    json!(payload_hash(&json!({
                        "kind": "exact_unfetched_intent_reservation_substitution",
                        "originalRiskHash": original_risk_hash,
                        "intentId": get(o2, "intentId"),
                        "reservationHash": get(&full, "reservationHash"),
                    }))?),
                );
                account_risk = V::Object(nr);
            }
        }
    }
    let mut input = e.as_object().cloned().unwrap_or_default();
    input.insert("accountRisk".into(), account_risk);
    input.insert("allocation".into(), get(s, "allocation").clone());
    input.insert("scope".into(), get(s, "scope").clone());
    input.insert("now".into(), json!(iso(at)));
    let result = plan_baal_scaled_entry(&V::Object(input));
    need(
        get(&result, "proposalReady") == &V::Bool(true),
        &format!("ENTRY_{}", sget(&result, "reason").unwrap_or("")),
    )?;
    for l in get(&result, "legs").as_array().cloned().unwrap_or_default() {
        need(
            get(get(s, "scope"), "exchanges").as_array().is_some_and(|ms| {
                ms.iter().any(|m| sget(m, "exchangeId") == sget(&l, "exchangeId") && get(m, "marketId") == get(&l, "marketId"))
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
            sget(c, "marketId").unwrap_or(""),
            sget(get(s, "scope"), "tournamentId").unwrap_or("")
        );
        let r = raw(get(c, "receipt"), "GET", &endpoint)?;
        let parsed = parse_number_lexemes(sget(&r, "bodyText").unwrap_or(""))?.0;
        let b = normalize_book(
            &parsed,
            sget(c, "marketId").unwrap_or(""),
            sget(get(s, "scope"), "tournamentId").unwrap_or(""),
            sget(&r, "receivedAt").unwrap_or(""),
            sget(&r, "requestStartedAt").unwrap_or(""),
            20000,
            &BTreeSet::new(),
            "authoritative_receipt_v1",
        )?;
        need(get(&b, "exchangeId") == get(c, "exchangeId"), "BOOK_EXCHANGE")?;
        let v = json!({
            "sequence": get(&b, "sequence"),
            "sourceAt": get(&b, "sourceAt"),
            "requestStartedAt": get(&r, "requestStartedAt"),
            "receivedAt": get(&r, "receivedAt"),
            "depthHash": payload_hash(&json!({"bids": get(&b, "bids"), "asks": get(&b, "asks")}))?,
        });
        let old = get(get(o, "books"), sget(c, "exchangeId").unwrap_or(""));
        if !old.is_null() {
            need(
                instant_v(get(&v, "requestStartedAt"))? >= instant_v(get(&old, "requestStartedAt"))?
                    && instant_v(get(&v, "receivedAt"))? >= instant_v(get(&old, "receivedAt"))?
                    && js_str(get(&v, "sequence")).parse::<i128>().unwrap_or(i128::MIN)
                        >= js_str(get(&old, "sequence")).parse::<i128>().unwrap_or(i128::MIN)
                    && instant_ns(sget(&v, "sourceAt").unwrap_or("")).map_err(|e| e.0)?
                        >= instant_ns(sget(&old, "sourceAt").unwrap_or("")).map_err(|e| e.0)?,
                "BOOK_ROLLBACK",
            )?;
            if get(&v, "sequence") == get(&old, "sequence") && get(&v, "sourceAt") == get(&old, "sourceAt") {
                need(get(&v, "depthHash") == get(&old, "depthHash"), "BOOK_VERSION_CONFLICT")?;
            }
        }
        out.insert(sget(c, "exchangeId").unwrap_or("").to_string(), v);
    }
    Ok(V::Object(out))
}

fn sell_source(s: &V, ex: &str, requested: &V, e: &V, at: i64) -> Result<V, String> {
    let slot = get(get(s, "inventory"), ex);
    need(
        js_int(get(get(slot, "proof"), "remainingOwnedQuantity")).is_some_and(|q| q > 0),
        "NO_OWNED_LOT",
    )?;
    let a = continue_owned(s, ex, get(e, "account"), at)?;
    let member = get(get(s, "scope"), "exchanges")
        .as_array()
        .and_then(|ms| ms.iter().find(|m| sget(m, "exchangeId") == Some(ex)).cloned())
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
    let parsed = parse_number_lexemes(sget(&r, "bodyText").unwrap_or(""))?;
    let d = &parsed.0;
    need(
        !d.as_object().map(|o| o.contains_key("marketId")).unwrap_or(false)
            || get(d, "marketId") == get(&member, "marketId"),
        "BOOK_MARKET",
    )?;
    let b = normalize_book(
        d,
        sget(&member, "marketId").unwrap_or(""),
        sget(get(s, "scope"), "tournamentId").unwrap_or(""),
        sget(&r, "receivedAt").unwrap_or(""),
        sget(&r, "requestStartedAt").unwrap_or(""),
        20000,
        &BTreeSet::new(),
        "authoritative_receipt_v1",
    )?;
    need(
        get(&b, "exchangeId") == get(&member, "exchangeId")
            && get(d, "contexts").as_array().is_some_and(|x| x.len() == 1)
            && get(&get(&get(d, "contexts")[0], "orderbook"), "exchanges")
                .as_array()
                .is_some_and(|x| x.len() == 1)
            && get(&b, "asks").as_array().is_some_and(|x| x.len() <= 20),
        "BOOK_SCOPE",
    )?;
    let remaining = js_int(get(get(slot, "proof"), "remainingOwnedQuantity")).unwrap_or(0);
    let max_shares = js_int(&{
        let v = get(get(s, "allocation"), "maximumSharesPerOrder");
        if v.is_null() { json!(100) } else { v.clone() }
    })
    .unwrap_or(0);
    let req = if requested.is_null() { 100 } else { js_int(requested).unwrap_or(i64::MIN) };
    let limit_qty = req.min(remaining).min(max_shares);
    need(limit_qty > 0, "SELL_QUANTITY")?;
    for (i, l) in get(&b, "asks").as_array().cloned().unwrap_or_default().iter().enumerate() {
        let key = js_stringify(&json!(["contexts", 0, "orderbook", "exchanges", 0, "asks", i, "quantity"]));
        let lex = parsed.1.get(&key);
        let ok = lex.is_some()
            && {
                let s = lex.unwrap().as_str();
                let int_part = s.split('.').next().unwrap_or("");
                !int_part.is_empty()
                    && (!int_part.starts_with('0') || int_part == "0")
                    && int_part.bytes().all(|b| b.is_ascii_digit())
                    && s.split('.').nth(1).is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b == b'0'))
            }
            && js_int(get(l, "quantity")).is_some()
            && lex.unwrap().split('.').next().unwrap_or("").parse::<i128>().ok()
                == js_int(get(l, "quantity")).map(|x| x as i128);
        need(ok, "EXACT_DEPTH")?;
        let limit = SCALE - price(&js_str(get(l, "price")))? - TICK;
        if limit < TICK {
            continue;
        }
        let depth_cap = js_int(get(l, "quantity")).unwrap_or(0) / 100;
        let cost_cap = amount(sget(get(s, "allocation"), "maximumOrderCost").unwrap_or("25.000000"))? / limit;
        let q = limit_qty.min(depth_cap).min(cost_cap as i64);
        if q < 1 {
            continue;
        }
        let mut left = q;
        let mut max_cost: i128 = 0;
        for lot in get(get(slot, "proof"), "remainingLots").as_array().cloned().unwrap_or_default() {
            if left <= 0 {
                break;
            }
            let lp = price(sget(&lot, "entryPrice").unwrap_or(""))?;
            if lp > max_cost {
                max_cost = lp;
            }
            left -= left.min(js_int(get(&lot, "quantity")).unwrap_or(0));
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
            "at": iso(at),
        }));
    }
    need(false, "PROTECTED_SELL_UNAVAILABLE")?;
    unreachable!()
}

fn preflight(s: &V, o: &V, e: &V, g: &V, at: i64, planned: &V) -> Result<(), String> {
    book_versions(s, o, e)?;
    need(
        js_int(get(get(o, "body"), "quantity")).is_some_and(|q| q <= 100)
            && price(&js_str(get(get(o, "body"), "price")))? * js_int(get(get(o, "body"), "quantity")).unwrap_or(0) as i128
                <= amount(sget(get(s, "allocation"), "maximumOrderCost").unwrap_or("25.000000"))?
            && at < instant_v(get(get(o, "body"), "expirationDate"))?,
        "ORDER_BOUND_OR_EXPIRED",
    )?;
    risk(s, g, at, false)?;
    budget(
        s,
        g,
        &reservation(planned)?,
        at,
        sget(get(o, "body"), "action") == Some("buy"),
    )?;
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
            .and_then(|ls| ls.iter().find(|l| sget(l, "exchangeId") != sget(get(o, "body"), "exchangeId")))
            .cloned()
            .unwrap_or(V::Null);
        need(
            sget(get(&p, "chosenLeg"), "exchangeId") == sget(get(o, "body"), "exchangeId")
                && get(&p, "eventId") == get(get(o, "package"), "eventId")
                && js_int(get(get(&p, "chosenLeg"), "quantity")).unwrap_or(0)
                    >= js_int(get(get(o, "body"), "quantity")).unwrap_or(0)
                && price(sget(get(&p, "chosenLeg"), "limit").unwrap_or(""))?
                    <= price(&js_str(get(get(o, "body"), "price")))?
                && price(sget(&other, "limit").unwrap_or(""))? + price(&js_str(get(get(o, "body"), "price")))?
                    <= 990_000i128
                && get(get(&p, "sourceReceipts"), "roots")
                    .as_array()
                    .is_some_and(|rs| {
                        rs.iter().all(|r| {
                            get(get(get(o, "proposal"), "sourceReceipts"), "roots")
                                .as_array()
                                .is_some_and(|olds| {
                                    olds.iter().any(|old| {
                                        get(old, "marketId") == get(r, "marketId")
                                            && get(old, "exchangeId") == get(r, "exchangeId")
                                            && get(old, "rootHash") == get(r, "rootHash")
                                    })
                                })
                        })
                    }),
            "FIXED_ENTRY_PROPOSAL",
        )?;
        continue_owned(s, sget(get(o, "body"), "exchangeId").unwrap_or(""), get(e, "account"), at)?;
    } else {
        let p = sell_source(
            s,
            sget(get(o, "body"), "exchangeId").unwrap_or(""),
            get(get(o, "body"), "quantity"),
            e,
            at,
        )?;
        need(
            js_int(get(&p, "quantity")).unwrap_or(0) >= js_int(get(get(o, "body"), "quantity")).unwrap_or(0)
                && price(sget(&p, "limit").unwrap_or(""))? >= price(&js_str(get(get(o, "body"), "price")))?,
            "FIXED_SELL_PROTECTION",
        )?;
    }
    Ok(())
}

fn cancel_check(s: &V, e: &V, at: i64) -> Result<(), String> {
    let o = active(s).unwrap_or(V::Null);
    need(
        sget(&o, "orderId").is_some() && at >= instant_v(get(get(&o, "body"), "expirationDate"))?,
        "CANCEL_NOT_EXPIRED_OR_OWNED",
    )?;
    risk(s, get(e, "globalRisk"), at, true)?;
    let r = raw(get(e, "order"), "GET", &format!("/orders/{}", sget(&o, "orderId").unwrap_or("")))?;
    let d = parse_number_lexemes(sget(&r, "bodyText").unwrap_or(""))?.0;
    let n = num_id(get(get(&o, "body"), "price"));
    need(
        js_int(get(&r, "status")) == Some(200)
            && instant_v(get(&r, "receivedAt"))? <= at
            && at - instant_v(get(&r, "requestStartedAt"))? <= 20000
            && num_id(get(&d, "id")) == sget(&o, "orderId").and_then(|s| s.parse::<f64>().ok()).unwrap_or(f64::NAN)
            && get(&d, "exchangeId") == get(get(&o, "body"), "exchangeId")
            && get(&d, "tournamentId") == get(get(s, "scope"), "tournamentId")
            && sget(&d, "side") == Some("no")
            && get(&d, "action") == get(get(&o, "body"), "action")
            && get(&d, "open") == &V::Bool(true)
            && sget(&normalize_official_fill_tick(get(&d, "priceLimit"))?, "price")
                == Some(money(price(&js_str(&json!(n)))?).as_str())
            && get(&d, "expirationDate") == get(get(&o, "body"), "expirationDate"),
        "CANCEL_SCOPE",
    )?;
    Ok(())
}

fn binding(s: &V, o: &V) -> V {
    let mut out = get(s, "scope").as_object().cloned().unwrap_or_default();
    out.insert("marketId".into(), get(o, "marketId").clone());
    out.insert("exchangeId".into(), get(get(o, "body"), "exchangeId").clone());
    out.insert("orderId".into(), get(o, "orderId").clone());
    out.insert("intentId".into(), get(o, "intentId").clone());
    out.insert("idempotencyKey".into(), get(get(o, "body"), "idempotencyKey").clone());
    out.insert("side".into(), json!("no"));
    out.insert("action".into(), get(get(o, "body"), "action").clone());
    out.insert("quantity".into(), get(get(o, "body"), "quantity").clone());
    out.insert("limit".into(), json!(money(price(&js_str(get(get(o, "body"), "price"))).unwrap_or(0))));
    out.insert("createdAt".into(), get(o, "createdAt").clone());
    out.insert("expirationDate".into(), get(get(o, "body"), "expirationDate").clone());
    V::Object(out)
}

fn canonical_ack(o: &V, r: &V) -> bool {
    (|| -> Result<bool, String> {
        let (d, numbers) = parse_number_lexemes(sget(r, "bodyText").unwrap_or(""))?;
        let exact = |k: &str| -> bool {
            let key = js_stringify(&json!([k]));
            let v = numbers.get(&key);
            v.is_some()
                && {
                    let s = v.unwrap().as_str();
                    let int_part = s.split('.').next().unwrap_or("");
                    !int_part.is_empty()
                        && (!int_part.starts_with('0') || int_part == "0")
                        && int_part.bytes().all(|b| b.is_ascii_digit())
                        && s.split('.').nth(1).is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b == b'0'))
                }
                && js_int(get(&d, k)).is_some()
                && v.unwrap().split('.').next().unwrap_or("").parse::<i128>().ok()
                    == js_int(get(&d, k)).map(|x| x as i128)
        };
        let cost = amount(numbers.get(&js_stringify(&json!(["totalCost"]))).map(|s| s.as_str()).unwrap_or(""))?;
        let q = num_id(get(&d, "quantityTraded"));
        let limit = price(&js_str(get(get(o, "body"), "price")))?;
        let qi = q as i128;
        need(
            js_int(get(&r, "status")) == Some(200)
                && num_id(get(&d, "orderId")) > 0.0
                && get(&d, "exchangeId") == get(get(o, "body"), "exchangeId")
                && sget(&d, "side") == Some("no")
                && get(&d, "action") == get(get(o, "body"), "action")
                && num_id(get(&d, "quantity")) > 0.0
                && num_id(get(&d, "quantity")) <= num_id(get(get(o, "body"), "quantity"))
                && ["orderId", "quantity", "quantityTraded", "remainingQuantity"].iter().all(|k| exact(k))
                && q >= 0.0
                && num_id(get(&d, "remainingQuantity")) >= 0.0
                && q + num_id(get(&d, "remainingQuantity")) == num_id(get(&d, "quantity"))
                && get(&d, "open").is_boolean()
                && (get(&d, "open") == &V::Bool(false) || num_id(get(&d, "remainingQuantity")) > 0.0)
                && sget(&normalize_official_fill_tick(get(&d, "price"))?, "price") == Some(money(limit).as_str()),
            "ACK_CANONICAL",
        )?;
        need(
            if q == 0.0 {
                cost == 0 && get(&d, "fillPrice").is_null()
            } else {
                q == qi as f64
                    && cost >= TICK * qi
                    && cost <= 995_000i128 * qi
                    && get(&d, "fillPrice").is_f64()
                    && get(&d, "fillPrice").as_f64().unwrap().is_finite()
                    && (if sget(get(o, "body"), "action") == Some("buy") { cost <= limit * qi } else { cost >= limit * qi })
            },
            "ACK_PRINCIPAL",
        )?;
        if get(&d, "all").is_null() {
            return Ok(true);
        }
        let a = get(&d, "all");
        let fields = [
            "fullNotionalCost", "effectiveEntryCost", "collateralSavings", "relationshipIds", "componentId",
            "guaranteedPayoutFloorAfter", "outstandingAdvanceAfter", "collateralRepayment",
            "netBuyingPowerImpact", "redemptionCredit",
        ];
        let mut keys: Vec<String> = a.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
        keys.sort();
        let mut fields_v: Vec<String> = fields.iter().map(|s| s.to_string()).collect();
        fields_v.sort();
        need(
            a.is_object()
                && keys == fields_v
                && get(a, "relationshipIds").as_array().is_some_and(|x| x.is_empty())
                && get(a, "componentId").is_null(),
            "ACK_NON_NEUTRAL",
        )?;
        for k in ["fullNotionalCost", "effectiveEntryCost"] {
            need(
                amount(numbers.get(&js_stringify(&json!(["all", k]))).map(|s| s.as_str()).unwrap_or(""))? == cost,
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
            need(
                amount(numbers.get(&js_stringify(&json!(["all", k]))).map(|s| s.as_str()).unwrap_or(""))? == 0,
                "ACK_NON_NEUTRAL",
            )?;
        }
        need(
            amount(numbers.get(&js_stringify(&json!(["all", "netBuyingPowerImpact"]))).map(|s| s.as_str()).unwrap_or(""))?
                == (if sget(get(o, "body"), "action") == Some("buy") { cost } else { -cost }),
            "ACK_SIGN",
        )?;
        Ok(true)
    })()
    .unwrap_or(false)
}

fn reduce(s: Option<&V>, e: &V) -> Result<V, String> {
    if sget(e, "kind") == Some("initialize") {
        need(s.is_none(), "INITIALIZED")?;
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
    let s = match s {
        Some(x) => x.clone(),
        None => return Err("BAAL_SCALED_NOT_INITIALIZED".into()),
    };
    let mut n = s.clone();
    let o_v = active(&n).unwrap_or(V::Null);
    if sget(e, "kind") == Some("reservation") {
        need(
            get(get(e, "receipt"), "reservationHash") == get(&reservation(&s)?, "reservationHash")
                && get(get(e, "receipt"), "allocationHash").as_str()
                    == Some(payload_hash(get(&s, "allocation"))?.as_str())
                && hex(get(get(e, "receipt"), "globalLedgerHash"))
                && instant_v(get(get(e, "receipt"), "confirmedAt"))? <= instant_v(get(e, "at"))?,
            "RESERVATION",
        )?;
        n.as_object_mut().unwrap().insert("reservation".into(), get(e, "receipt").clone());
        return Ok(n);
    }
    if sget(e, "kind") == Some("prepared") {
        need(o_v.is_null(), "UNKNOWN_OR_OPEN_INTENT")?;
        let (proposal, pkg, market_id, ex, q, limit);
        if sget(e, "action") == Some("buy") {
            need(
                instant_v(get(e, "at"))? >= instant_v(get(get(&s, "activation"), "startedAt"))?
                    && instant_v(get(e, "at"))? < instant_v(get(get(&s, "activation"), "entryExpiresAt"))?,
                "ENTRY_WINDOW",
            )?;
            let p = native_entry(&s, get(e, "evidence"), instant_v(get(e, "at"))?)?;
            market_id = get(get(&p, "chosenLeg"), "marketId").clone();
            ex = sget(get(&p, "chosenLeg"), "exchangeId").unwrap_or("").to_string();
            q = get(get(&p, "chosenLeg"), "quantity").clone();
            limit = sget(get(&p, "chosenLeg"), "limit").unwrap_or("").to_string();
            let legs: Vec<V> = get(&p, "legs")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|l| json!({"marketId": get(l, "marketId"), "exchangeId": get(l, "exchangeId")}))
                .collect();
            pkg = json!({"eventId": get(&p, "eventId"), "legs": legs, "unitPackageCeiling": "0.990000"});
            proposal = p;
        } else {
            need(sget(e, "action") == Some("sell"), "ACTION")?;
            let p = sell_source(
                &s,
                sget(e, "exchangeId").unwrap_or(""),
                get(e, "quantity"),
                get(e, "evidence"),
                instant_v(get(e, "at"))?,
            )?;
            market_id = get(&p, "marketId").clone();
            ex = sget(&p, "exchangeId").unwrap_or("").to_string();
            q = get(&p, "quantity").clone();
            limit = sget(&p, "limit").unwrap_or("").to_string();
            pkg = get(get(get(&s, "inventory"), ex.as_str()), "package").clone();
            proposal = p;
        }
        need(
            get(e, "intentId").is_string()
                && get(e, "idempotencyKey").is_string()
                && !get(&s, "orders").as_array().is_some_and(|os| {
                    os.iter().any(|v| {
                        get(v, "intentId") == get(e, "intentId")
                            || get(get(v, "body"), "idempotencyKey") == get(e, "idempotencyKey")
                    })
                }),
            "INTENT_ID",
        )?;
        let cap = if sget(e, "action") == Some("buy") {
            instant_v(get(get(&s, "activation"), "entryExpiresAt"))?
        } else {
            i64::MAX
        };
        let expires_ms = (instant_v(get(e, "at"))? + 30000).min(cap);
        need(expires_ms <= 8_640_000_000_000_000 && expires_ms >= -8_640_000_000_000_000, "INVALID_TIME_VALUE")?;
        let expires = iso(expires_ms);
        let body = json!({
            "idempotencyKey": get(e, "idempotencyKey"),
            "exchangeId": ex,
            "side": "no",
            "action": get(e, "action"),
            "quantity": q,
            "price": limit.parse::<f64>().unwrap_or(f64::NAN),
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
            "before": own_account(get(get(e, "evidence"), "account"), ex.as_str()),
            "priorProof": get(get(get(&s, "inventory"), ex.as_str()), "proof"),
            "priorAfter": get(get(get(&s, "inventory"), ex.as_str()), "after"),
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
        candidate.as_object_mut().unwrap().insert("books".into(), books);
        n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap().push(candidate.clone());
        preflight(&s, &candidate, get(e, "evidence"), get(e, "globalRisk"), instant_v(get(e, "at"))?, &n)?;
        return Ok(n);
    }
    need(!o_v.is_null() && get(&o_v, "proof").is_null(), "NO_ACTIVE_INTENT")?;
    let oi = get(&n, "orders")
        .as_array()
        .and_then(|os| os.iter().position(|v| get(v, "proof").is_null() && sget(v, "phase") != Some("abandoned")))
        .unwrap();
    if sget(e, "kind") == Some("abandoned_prepared") {
        need(
            sget(&o_v, "phase") == Some("prepared")
                && get(&o_v, "operation").is_null()
                && get(&o_v, "orderId").is_null()
                && get(&o_v, "ack").is_null()
                && get(&o_v, "proof").is_null()
                && get(&o_v, "cancelPending") == &V::Bool(false)
                && get(&o_v, "fences").as_array().is_some_and(|x| x.is_empty())
                && get(&o_v, "fetchStarts").as_array().is_some_and(|x| x.is_empty())
                && get(&o_v, "receipts").as_array().is_some_and(|x| x.is_empty())
                && instant_v(get(e, "at"))? >= instant_v(get(get(&o_v, "body"), "expirationDate"))?,
            "ABANDON_REQUIRES_EXPIRED_NEVER_ADMITTED",
        )?;
        let o = &mut n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap()[oi];
        o.as_object_mut().unwrap().insert("phase".into(), json!("abandoned"));
        o.as_object_mut().unwrap().insert("abandonedAt".into(), get(e, "at").clone());
        o.as_object_mut().unwrap().insert("abandonedReason".into(), json!("expired_before_any_admission"));
        return Ok(n);
    }
    if sget(e, "kind") == Some("admitted") || sget(e, "kind") == Some("network_fence") {
        if sget(e, "operation") == Some("place") {
            need(
                if sget(e, "kind") == Some("admitted") {
                    sget(&o_v, "phase") == Some("prepared")
                } else {
                    sget(&o_v, "phase") == Some("dispatching")
                },
                "PLACEMENT_PHASE",
            )?;
            preflight(&s, &o_v, get(e, "evidence"), get(e, "globalRisk"), instant_v(get(e, "at"))?, &s)?;
            let books = book_versions(&s, &o_v, get(e, "evidence"))?;
            n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap()[oi]
                .as_object_mut()
                .unwrap()
                .insert("books".into(), books);
        } else {
            need(
                sget(e, "operation") == Some("cancel")
                    && (sget(e, "kind") != Some("admitted") || get(&o_v, "cancelPending") != &V::Bool(true)),
                "OPERATION",
            )?;
            cancel_check(&s, get(e, "evidence"), instant_v(get(e, "at"))?)?;
        }
        if sget(e, "kind") == Some("admitted") {
            let o = &mut n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap()[oi];
            o.as_object_mut().unwrap().insert("operation".into(), get(e, "operation").clone());
            if sget(e, "operation") == Some("place") {
                o.as_object_mut().unwrap().insert("phase".into(), json!("dispatching"));
            } else {
                o.as_object_mut().unwrap().insert("cancelPending".into(), json!(true));
            }
        } else {
            need(
                sget(&o_v, "operation") == sget(e, "operation")
                    && payload_hash(get(e, "leaseIdentity"))? == sget(e, "leaseHash").unwrap_or(""),
                "LEASE_FENCE",
            )?;
            let o = &mut n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap()[oi];
            o.as_object_mut()
                .unwrap()
                .get_mut("fences")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "at": get(e, "at"),
                    "leaseIdentity": get(e, "leaseIdentity"),
                    "leaseHash": get(e, "leaseHash"),
                    "operation": get(e, "operation"),
                }));
        }
        return Ok(n);
    }
    if sget(e, "kind") == Some("fetch_start") {
        let f = get(&o_v, "fences").as_array().and_then(|x| x.last()).cloned().unwrap_or(V::Null);
        let w = get(e, "start");
        let method_expected = if sget(&o_v, "operation") == Some("place") { "POST" } else { "DELETE" };
        let endpoint_expected = if sget(&o_v, "operation") == Some("place") {
            "/orders".to_string()
        } else {
            format!("/orders/{}", sget(&o_v, "orderId").unwrap_or(""))
        };
        need(
            !f.is_null()
                && get(w, "intentId") == get(&o_v, "intentId")
                && get(w, "requestBodyHash") == get(&o_v, "bodyHash")
                && get(w, "requestBodyJson") == get(&o_v, "bodyJson")
                && get(w, "leaseHash") == get(&f, "leaseHash")
                && same(get(w, "leaseIdentity"), get(&f, "leaseIdentity"))?
                && get(w, "fetchInvocationId").is_string()
                && !get(&s, "orders").as_array().is_some_and(|os| {
                    os.iter().any(|v| {
                        get(v, "fetchStarts").as_array().is_some_and(|fs| {
                            fs.iter().any(|x| get(x, "fetchInvocationId") == get(w, "fetchInvocationId"))
                        })
                    })
                })
                && instant_v(get(w, "requestStartedAt"))? >= instant_v(get(&f, "at"))?
                && instant_v(get(w, "requestStartedAt"))? <= instant_v(get(e, "at"))?
                && sget(w, "method") == Some(method_expected)
                && sget(w, "endpoint") == Some(endpoint_expected.as_str()),
            "ACTUAL_FETCH",
        )?;
        if sget(&o_v, "operation") == Some("place") {
            need(
                instant_v(get(w, "requestStartedAt"))? < instant_v(get(get(&o_v, "body"), "expirationDate"))?,
                "EXPIRED_FETCH",
            )?;
        }
        let o = &mut n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap()[oi];
        o.as_object_mut().unwrap().get_mut("fetchStarts").unwrap().as_array_mut().unwrap().push(w.clone());
        return Ok(n);
    }
    if sget(e, "kind") == Some("wire") {
        let w = get(&o_v, "fetchStarts").as_array().and_then(|x| x.last()).cloned().unwrap_or(V::Null);
        let r = get(e, "receipt");
        need(
            !w.is_null()
                && get(r, "fetchInvocationId") == get(&w, "fetchInvocationId")
                && get(r, "requestStartedAt") == get(&w, "requestStartedAt")
                && get(r, "method") == get(&w, "method")
                && get(r, "endpoint") == get(&w, "endpoint")
                && sget(r, "sourceBodyHash") == Some(sha(sget(r, "bodyText").unwrap_or("")).as_str())
                && instant_v(get(r, "receivedAt"))? <= instant_v(get(e, "at"))?,
            "WIRE_BINDING",
        )?;
        {
            let o = &mut n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap()[oi];
            o.as_object_mut().unwrap().get_mut("receipts").unwrap().as_array_mut().unwrap().push(r.clone());
        }
        if sget(&o_v, "operation") == Some("cancel") {
            return Ok(n);
        }
        let d = parse_number_lexemes(sget(r, "bodyText").unwrap_or("")).map(|x| x.0).unwrap_or(V::Null);
        let oid = js_int(get(&d, "orderId"));
        if oid.is_some_and(|x| x > 0) {
            let o = &mut n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap()[oi];
            o.as_object_mut().unwrap().insert("orderId".into(), json!(oid.unwrap().to_string()));
        }
        {
            let o = &mut n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap()[oi];
            let mut ack = r.as_object().cloned().unwrap_or_default();
            ack.insert("requestBodyHash".into(), get(&o_v, "bodyHash").clone());
            o.as_object_mut().unwrap().insert("ack".into(), V::Object(ack));
            let phase = if js_int(get(r, "status")) == Some(200) {
                if canonical_ack(&o_v, r) { "acknowledged" } else { "semantic_mismatch" }
            } else {
                "uncertain"
            };
            o.as_object_mut().unwrap().insert("phase".into(), json!(phase));
        }
        return Ok(n);
    }
    if sget(e, "kind") == Some("error") {
        let o = &mut n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap()[oi];
        if sget(o, "phase") == Some("dispatching") {
            o.as_object_mut().unwrap().insert("phase".into(), json!("uncertain"));
        }
        o.as_object_mut().unwrap().insert("lastError".into(), get(e, "code").clone());
        return Ok(n);
    }
    if sget(e, "kind") == Some("reconciled") {
        need(
            sget(&o_v, "orderId").is_some() && !get(&o_v, "ack").is_null() && sget(&o_v, "phase") == Some("acknowledged"),
            "UNRESOLVED_ACK",
        )?;
        let start = get(&o_v, "fetchStarts")
            .as_array()
            .and_then(|fs| fs.iter().find(|v| sget(v, "method") == Some("POST")).cloned());
        need(start.is_some(), "ACTUAL_FETCH_MISSING")?;
        let mut ev = get(e, "evidence").as_object().cloned().unwrap_or_default();
        ev.insert("binding".into(), binding(&s, &o_v));
        ev.insert("before".into(), get(&o_v, "before").clone());
        ev.insert("ack".into(), get(&o_v, "ack").clone());
        ev.insert("dispatchReceipt".into(), start.unwrap());
        ev.insert("readAt".into(), get(e, "at").clone());
        ev.insert(
            "reservedFunding".into(),
            json!(if sget(get(&o_v, "body"), "action") == Some("buy") {
                money(990_000i128 * js_int(get(get(&o_v, "body"), "quantity")).unwrap_or(0) as i128)
            } else {
                "0.000000".to_string()
            }),
        );
        ev.insert("priorProof".into(), get(&o_v, "priorProof").clone());
        ev.insert("priorAfter".into(), get(&o_v, "priorAfter").clone());
        let proof = verify_scoped_rest_no_multishare(&V::Object(ev))?;
        if sget(get(&o_v, "body"), "action") == Some("sell") {
            need(
                amount(sget(&proof, "realizedPnl").unwrap_or(""))?
                    >= TICK * js_int(get(&proof, "filledQuantity")).unwrap_or(0) as i128,
                "PROTECTED_REALIZED_PROFIT",
            )?;
        }
        {
            let o = &mut n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap()[oi];
            o.as_object_mut().unwrap().insert("proof".into(), proof.clone());
            o.as_object_mut().unwrap().insert("phase".into(), json!("reconciled"));
            o.as_object_mut().unwrap().insert("cancelPending".into(), json!(false));
        }
        let bex = sget(get(&o_v, "body"), "exchangeId").unwrap_or("").to_string();
        n.as_object_mut().unwrap().get_mut("inventory").unwrap().as_object_mut().unwrap().insert(
            bex,
            json!({
                "proof": proof,
                "after": get(get(e, "evidence"), "after"),
                "package": get(&o_v, "package"),
            }),
        );
        return Ok(n);
    }
    Err("BAAL_SCALED_EVENT".into())
}

pub const LEGACY_SCALED_CONTROLLER_SOURCE_HASH: &str =
    "20c57aa53ae1cc6b7f1b5aeb42b51cfdc391ded04165032ed102165a087780c0";

// baalScaledNoControllerIdentity — identical workspace-relative graph to the
// execution crate's port.
pub fn baal_scaled_no_controller_identity() -> Result<V, String> {
    baal_scaled_no_controller_identity_impl()
}
fn baal_scaled_no_controller_identity_impl() -> Result<V, String> {
    Ok(json!(baal_collector::frozen_paper_graph::collect_frozen_paper_files(
        &workspace(),
        &["execution/baal-scaled-no-controller-v1.mjs".to_string()],
    )?))
}

// replayBaalScaledControllerV1({bodyText,expectedCodeIdentity,expectedScope,expectedAllocation})
pub fn replay_baal_scaled_controller_v1(input: &V) -> Result<V, String> {
    let body_text = get(input, "bodyText").as_str().unwrap_or("").to_string();
    let expected_code_identity = get(input, "expectedCodeIdentity");
    let expected_scope = get(input, "expectedScope");
    let expected_allocation = get(input, "expectedAllocation");
    need(
        !body_text.is_empty()
            && body_text.as_bytes().len() <= 128 * 1024 * 1024
            && body_text.ends_with('\n'),
        "LEGACY_REPLAY_INPUT",
    )?;
    need(
        payload_hash(&baal_scaled_no_controller_identity_impl()?)?
            == payload_hash(expected_code_identity)?,
        "LEGACY_SOURCE_IDENTITY",
    )?;
    let legacy_file = workspace().join("execution").join("baal-scaled-no-controller-v1.mjs");
    need(
        sha_bytes(&std::fs::read(&legacy_file).unwrap_or_default()) == LEGACY_SCALED_CONTROLLER_SOURCE_HASH,
        "LEGACY_SOURCE_IDENTITY",
    )?;
    let mut state: Option<V> = None;
    let mut sequence: i64 = 0;
    let mut previous = String::new();
    let mut first: Option<V> = None;
    for line in body_text.trim_end_matches('\n').split('\n') {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let hash = get(&row, "hash").as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        body.as_object_mut().map(|m| m.remove("hash"));
        need(
            js_stringify(&row) == line
                && js_int(get(&body, "sequence")) == Some(sequence + 1)
                && sget(&body, "previous") == Some(previous.as_str())
                && sha(&js_stringify(&body)) == hash
                && same(get(get(&body, "event"), "codeIdentity"), expected_code_identity)?,
            "LEGACY_CHAIN_CODE",
        )?;
        instant_v(get(&body, "at"))?;
        state = Some(reduce(state.as_ref(), get(&get(&body, "event"), "input"))?);
        need(
            same(state.as_ref().unwrap(), get(&body, "state"))?,
            "LEGACY_REDUCER_STATE",
        )?;
        sequence += 1;
        previous = hash.clone();
        if sequence == 1 {
            first = Some(json!({
                "bytes": line.as_bytes().len() + 1,
                "sha256": sha(&format!("{line}\n")),
                "sequence": sequence,
                "hash": hash,
            }));
        }
    }
    let state = state.unwrap_or(V::Null);
    configuration(
        get(&state, "scope"),
        get(&state, "allocation"),
        get(&state, "activation"),
        expected_code_identity,
    )?;
    if !expected_scope.is_null() {
        need(same(get(&state, "scope"), expected_scope)?, "LEGACY_SCOPE")?;
    }
    if !expected_allocation.is_null() {
        need(same(get(&state, "allocation"), expected_allocation)?, "LEGACY_ALLOCATION")?;
    }
    need(active(&state).is_none(), "LEGACY_ACTIVE_INTENT")?;
    let bytes = body_text.as_bytes().len();
    Ok(json!({
        "schemaVersion": 1,
        "mode": "exact_scaled_v1_sealed_seed_replay",
        "state": state,
        "stateHash": payload_hash(&state)?,
        "anchor": {
            "bytes": bytes,
            "sha256": sha(&body_text),
            "sequence": sequence,
            "hash": previous,
        },
        "rootPrefix": first.unwrap_or(V::Null),
        "legacyCodeIdentityHash": payload_hash(expected_code_identity)?,
        "legacySourceHash": LEGACY_SCALED_CONTROLLER_SOURCE_HASH,
        "legacyReservation": reservation(&state)?,
        "economicReplayPerformed": true,
    }))
}

// validateLegacyScaledNeutralAck=(order,receipt)=>canonicalAck(order,receipt)
pub fn validate_legacy_scaled_neutral_ack(order: &V, receipt: &V) -> bool {
    canonical_ack(order, receipt)
}
