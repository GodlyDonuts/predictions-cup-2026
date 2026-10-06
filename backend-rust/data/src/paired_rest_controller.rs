// execution/paired-rest-controller.mjs — staged lifecycle only. The caller
// supplies its EXISTING serialized dispatcher, actual account lease, external
// STOP/activation gate and durable global reserve. No credential/client/
// limiter, activation file, polling loop or implicit retry.
use baal_execution::core::{canonical_payload, payload_hash};
use baal_execution::gates::normalize_official_fill_tick;
use baal_execution::no_rest_reconciliation::verify_scoped_rest_no_buy;
use crate::no_rest_reconciliation::inspect_scoped_rest_no_account;
use baal_execution::paired_dry_run::plan_paired_dry_run;
use baal_execution::receipts::parse_number_lexemes;
use baal_execution::transport::{OrderTransport, OrderTransportError, OrderTransportOptions, scope_of};
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::instant_ns;
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use baal_service_runtime::ledger::acquire_owner_lease;
use serde_json::{json, Map, Value as V};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("PAIRED_REST_{c}")) }
}
fn clone(v: &V) -> V { v.clone() }
fn sha(b: &[u8]) -> String { sha256(b) }
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn id(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        !s.is_empty() && s.as_bytes()[0] != b'0' && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<u128>().map(|n| n <= 9_007_199_254_740_991).unwrap_or(false)
    })
}
fn iso(n: i64) -> String { baal_service_runtime::contract::ms_to_iso(n as f64) }
fn get<'a>(v: &'a V, k: &str) -> &'a V { v.get(k).unwrap_or(&V::Null) }
fn amount_s(v: &V) -> Result<i128, String> { amount(v.as_str().unwrap_or("")) }
fn instant_s(v: &V) -> Result<i64, String> { instant(v.as_str().unwrap_or("")) }
fn price_s(v: &V) -> Result<i128, String> { price(v.as_str().unwrap_or("")) }
fn keys(o: &V, allowed: &[&str]) -> Result<(), String> {
    need(
        o.is_object() && !o.is_array()
            && o.as_object().unwrap().keys().all(|k| allowed.contains(&k.as_str())),
        "UNSUPPORTED_FIELDS",
    )
}

pub fn paired_rest_code_identity(execution_dir: &Path) -> serde_json::Map<String, V> {
    // sha256 each reachable .mjs source under the controller's own import graph.
    let mut seen: serde_json::Map<String, V> = serde_json::Map::new();
    fn visit(file: &Path, entry_dir: &Path, seen: &mut serde_json::Map<String, V>) {
        let key = file.strip_prefix(entry_dir).unwrap_or(file).to_string_lossy().replace('\\', "/");
        if seen.contains_key(&key) { return; }
        let raw = match std::fs::read(file) { Ok(b) => b, Err(_) => return };
        seen.insert(key, json!(sha(&raw)));
        let text = String::from_utf8_lossy(&raw);
        for caps in import_specifier_iter(&text) {
            let target = file.parent().unwrap_or(Path::new("")).join(&caps);
            let target = if target.extension().is_none() { target.with_extension("mjs") } else { target };
            visit(&target, entry_dir, seen);
        }
    }
    visit(&execution_dir.join("paired-rest-controller.mjs"), execution_dir, &mut seen);
    seen
}
fn import_specifier_iter(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for m in text.match_indices("from") {
        let rest = &text[m.0..];
        if let Some(qi) = rest.find(['\'', '"']) {
            let q = rest.as_bytes()[qi] as char;
            if let Some(end) = rest[qi + 1..].find(q) {
                let spec = &rest[qi + 1..qi + 1 + end];
                if spec.starts_with('.') { out.push(spec.to_string()); }
            }
        }
    }
    for m in text.match_indices("import") {
        let rest = &text[m.0..];
        if let Some(qi) = rest.find(['\'', '"']) {
            let q = rest.as_bytes()[qi] as char;
            if let Some(end) = rest[qi + 1..].find(q) {
                let spec = &rest[qi + 1..qi + 1 + end];
                if spec.starts_with('.') { out.push(spec.to_string()); }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

// A distinct dormant REST execution revision. The original capture/dry plan is
// retained by hash; its observed protective cost is not relabeled as the
// reserve. Sequential hedge flexibility uses only the declared ordinary payout
// margin.
pub fn derive_paired_rest_plan(input: &V) -> Result<V, String> {
    let original = plan_paired_dry_run(input)?;
    let original_dry_plan_hash = get(&original, "planHash").clone();
    let ceiling = amount_s(get(&original, "conditionalOrdinaryMinimumPayout"))?
        - amount_s(get(get(&original, "policy"), "minimumOrdinaryMargin"))?;
    need(
        ceiling == 990_000 && ceiling % TICK == 0 && amount_s(get(&original, "pairCostCeiling"))? <= ceiling,
        "ORDINARY_MARGIN_CEILING",
    )?;
    let mut derived = original.clone();
    let dm = derived.as_object_mut().unwrap();
    dm.shift_remove("planHash");
    dm.insert("schemaVersion".into(), json!(2));
    dm.insert("mode".into(), json!("paired_rest_no_fixed_margin_v2"));
    dm.insert("originalDryPlanHash".into(), original_dry_plan_hash);
    dm.insert("originalProtectiveCost".into(), get(&original, "pairCostCeiling").clone());
    dm.insert("originalConditionalOrdinarySurplus".into(), get(&original, "conditionalOrdinarySurplus").clone());
    dm.insert("sourceMinimumOrdinaryMargin".into(), get(get(&original, "policy"), "minimumOrdinaryMargin").clone());
    dm.insert("pairCostCeiling".into(), json!(money(ceiling)));
    dm.insert("conditionalOrdinarySurplus".into(), json!(money(amount_s(get(&original, "conditionalOrdinaryMinimumPayout"))? - ceiling)));
    dm.insert("exceptionLoss".into(), json!(money(-ceiling)));
    let h = payload_hash(&derived)?;
    derived.as_object_mut().unwrap().insert("planHash".into(), json!(h));
    Ok(derived)
}

fn active(s: &V) -> Option<&V> {
    get(s, "orders").as_array().and_then(|o| o.iter().find(|x| get(x, "phase").as_str() != Some("reconciled")))
}
fn cost(s: &V) -> i128 {
    get(s, "orders").as_array().map(|o| o.iter().map(|x| {
        amount(get(x, "proof").get("scopedPrincipalCost").and_then(|v| v.as_str()).unwrap_or("0.000000")).unwrap_or(0)
    }).sum()).unwrap_or(0)
}
fn ended(s: &V) -> bool {
    let orders = get(s, "orders").as_array().cloned().unwrap_or_default();
    (orders.len() == 2 && orders.iter().all(|o| get(o, "phase") == &json!("reconciled")))
        || orders.first().is_some_and(|o| {
            get(o, "phase") == &json!("reconciled") && get(get(o, "proof"), "filledQuantity") == &json!(0)
        })
}
pub fn paired_rest_funding(s: &V) -> Result<V, String> {
    let used = cost(s);
    let body = json!({
        "schemaVersion": 1,
        "ownerId": get(get(get(s, "plan"), "scope"), "ownerId"),
        "runId": get(get(get(s, "plan"), "scope"), "runId"),
        "tournamentId": get(get(get(s, "plan"), "scope"), "tournamentId"),
        "planHash": get(get(s, "plan"), "planHash"),
        "allocatedCapital": "2.000000",
        "reservedCash": if ended(s) { "0.000000".to_string() } else { money(amount_s(get(get(s, "plan"), "pairCostCeiling"))? - used) },
        "ownedPrincipal": money(used),
        "cumulativeBuy": money(used),
        "maxCumulativeBuy": "2.000000",
    });
    let mut out = body.clone();
    out.as_object_mut().unwrap().insert("reservationHash".into(), json!(payload_hash(&body)?));
    Ok(out)
}
fn binding(s: &V, o: &V, order_id: Option<&V>) -> Result<V, String> {
    let oid = order_id.cloned().unwrap_or_else(|| get(o, "orderId").clone());
    Ok(json!({
        "ownerId": get(get(get(s, "plan"), "scope"), "ownerId"),
        "runId": get(get(get(s, "plan"), "scope"), "runId"),
        "intentId": get(o, "intentId"),
        "idempotencyKey": get(get(o, "body"), "idempotencyKey"),
        "orderId": oid,
        "marketId": get(o, "marketId"),
        "exchangeId": get(get(o, "body"), "exchangeId"),
        "tournamentId": get(get(o, "body"), "tournamentId"),
        "tournamentSlug": get(get(get(s, "plan"), "scope"), "tournamentSlug"),
        "side": "no",
        "action": "buy",
        "quantity": 1,
        "limit": money((get(get(o, "body"), "price").as_f64().unwrap_or(f64::NAN) * 1e6).round() as i128),
        "createdAt": get(o, "createdAt"),
        "expirationDate": get(get(o, "body"), "expirationDate"),
    }))
}
fn raw(r: &V, method: &str, endpoint: &str) -> Result<V, String> {
    keys(r, &["method", "endpoint", "status", "bodyText", "requestStartedAt", "receivedAt", "sourceBodyHash", "requestBodyHash", "fetchInvocationId"])?;
    need(
        get(r, "method") == &json!(method)
            && get(r, "endpoint") == &json!(endpoint)
            && js_int(get(r, "status")).is_some()
            && get(r, "bodyText").as_str().is_some_and(|b| b.len() <= 8 * 1024 * 1024),
        "RAW_RECEIPT_REQUIRED",
    )?;
    instant_s(get(r, "requestStartedAt")).map_err(|e| e)?;
    instant_s(get(r, "receivedAt")).map_err(|e| e)?;
    need(instant_s(get(r, "receivedAt"))? >= instant_s(get(r, "requestStartedAt"))?, "RAW_RECEIPT_CLOCK")?;
    let body_hash = sha(get(r, "bodyText").as_str().unwrap_or("").as_bytes());
    if let Some(h) = r.get("sourceBodyHash") { need(h == &json!(body_hash), "RAW_RECEIPT_HASH")?; }
    let mut out = r.clone();
    out.as_object_mut().unwrap().insert("sourceBodyHash".into(), json!(body_hash));
    Ok(out)
}
fn account_shape(a: &V) -> Result<(), String> {
    keys(a, &["cash", "positions", "openOrdersPages", "scopedOrdersPages"])?;
    for r in [get(a, "cash").clone(), get(a, "positions").clone()]
        .into_iter()
        .chain(get(a, "openOrdersPages").as_array().cloned().unwrap_or_default())
        .chain(get(a, "scopedOrdersPages").as_array().cloned().unwrap_or_default())
    {
        raw(&r, "GET", get(&r, "endpoint").as_str().unwrap_or(""))?;
    }
    Ok(())
}
fn expected_positions(s: &V) -> Vec<V> {
    let mut out: Vec<V> = get(s, "externalPositions").as_array().cloned().unwrap_or_default();
    for o in get(s, "orders").as_array().cloned().unwrap_or_default() {
        if js_int(get(get(&o, "proof"), "filledQuantity")).is_some_and(|x| x != 0) {
            out.push(json!({
                "exchangeId": get(get(&o, "body"), "exchangeId"),
                "marketId": get(&o, "marketId"),
                "quantity": -1,
                "settled": false,
            }));
        }
    }
    out.retain(|p| js_int(get(p, "quantity")).is_some_and(|x| x != 0));
    out.sort_by_key(|p| {
        get(p, "exchangeId").as_str().and_then(|s| s.parse::<i64>().ok())
            .or_else(|| get(p, "exchangeId").as_i64()).unwrap_or(0)
    });
    out
}
fn global_gate(g: &V, s: &V, at: i64, own: Option<&V>) -> Result<(), String> {
    need(
        get(g, "tournamentId") == get(get(get(s, "plan"), "scope"), "tournamentId")
            && get(g, "ownerId") == get(get(get(s, "plan"), "scope"), "ownerId")
            && get(g, "runId") == get(get(get(s, "plan"), "scope"), "runId")
            && get(g, "complete") == &json!(true)
            && get(g, "soleCollector") == &json!(true)
            && get(g, "stopped") == &json!(false)
            && hex(get(g, "sourceHash"))
            && get(g, "unresolvedIntentIds").as_array().is_some_and(|a| a.len() <= 1)
            && get(g, "unresolvedIntentIds").as_array().map(|a| a.iter().all(|v| Some(v) == own)).unwrap_or(false)
            && instant_s(get(g, "at"))? <= at
            && at - instant_s(get(g, "at"))? <= 20000,
        "GLOBAL_UNCERTAINTY",
    )
}
fn preflight(s: &V, e: &V, at: i64, o: Option<&V>) -> Result<V, String> {
    keys(e, &["tournamentId", "account", "book", "globalRisk", "funding", "reservedElsewhere"])?;
    account_shape(get(e, "account"))?;
    need(!ended(s) && instant_s(get(get(s, "plan"), "reviewExpiresAt"))? > at, "PAIR_FINISHED_OR_RULES_EXPIRED")?;
    let index = o.map(|x| js_int(get(x, "index")).unwrap_or(0) as usize).unwrap_or_else(|| get(s, "orders").as_array().map(|a| a.len()).unwrap_or(0));
    let legs = get(get(s, "plan"), "legs").as_array().cloned().unwrap_or_default();
    need(index < legs.len(), "PAIR_LIMIT")?;
    let leg = legs[index].clone();
    if index == 0 {
        need(
            legs.iter().all(|l| at - instant_s(get(get(l, "book"), "requestStartedAt")).unwrap_or(i64::MIN) <= 20000),
            "INITIAL_PAIR_BOOKS_STALE",
        )?;
    }
    let cap = if index == 0 { price_s(get(&leg, "originalLimit"))? } else { amount_s(get(get(s, "plan"), "pairCostCeiling"))? - cost(s) };
    need(cap >= TICK && cap <= SCALE - TICK && cap <= 1_000_000 && (index != 0 || cap <= 500_000), "COST_CAP")?;
    if index == 1 {
        need(
            get(s, "orders").as_array().and_then(|a| a.first()).is_some_and(|o0| {
                get(o0, "phase") == &json!("reconciled") && get(get(o0, "proof"), "filledQuantity") == &json!(1)
            }),
            "FIRST_LEG_UNRESOLVED",
        )?;
    }
    global_gate(get(e, "globalRisk"), s, at, o.map(|x| get(x, "intentId")))?;
    need(get(e, "tournamentId") == get(get(get(s, "plan"), "scope"), "tournamentId"), "BOOK_SCOPE")?;
    let reserve = paired_rest_funding(s)?;
    let installed = get(e, "funding");
    need(
        get(installed, "reservationHash") == get(&reserve, "reservationHash")
            && get(installed, "amount") == get(&reserve, "reservedCash")
            && hex(get(installed, "globalLedgerHash"))
            && instant_s(get(installed, "confirmedAt"))? <= at
            && at - instant_s(get(installed, "confirmedAt"))? <= 20000,
        "GLOBAL_FUNDING_NOT_INSTALLED",
    )?;
    let mut b = get(get(s, "plan"), "scope").clone();
    b.as_object_mut().unwrap().insert("exchangeId".into(), get(&leg, "exchangeId").clone());
    let a = inspect_scoped_rest_no_account(&json!({
        "account": get(e, "account"),
        "binding": b,
        "readAt": iso(at),
        "requiredFunding": get(&reserve, "reservedCash"),
        "reservedElsewhere": e.get("reservedElsewhere").cloned().unwrap_or_else(|| json!("0.000000")),
    }))?;
    need(get(&a, "funded") == &json!(true), "FULL_PAIR_FUNDING")?;
    let mut positions: Vec<V> = get(&a, "positions").as_array().cloned().unwrap_or_default()
        .into_iter().filter(|p| baal_service_runtime::json_text::js_truthy(p.get("quantity"))).collect();
    positions.sort_by_key(|x| {
        get(x, "exchangeId").as_str().and_then(|s| s.parse::<i64>().ok())
            .or_else(|| get(x, "exchangeId").as_i64()).unwrap_or(0)
    });
    if get(s, "externalPositions").is_null() {
        need(
            !positions.iter().any(|p| legs.iter().any(|l| get(l, "exchangeId") == get(p, "exchangeId"))),
            "EXISTING_PAIR_HOLDING",
        )?;
    } else {
        need(
            payload_hash(&json!(positions)).ok() == payload_hash(&json!(expected_positions(s))).ok(),
            "ACCOUNT_POSITION_DRIFT",
        )?;
    }
    let endpoint = format!("/markets/{}/orderbook?tournamentId={}&depth=20",
        get(&leg, "marketId").as_str().unwrap_or(""), get(get(get(s, "plan"), "scope"), "tournamentId").as_str().unwrap_or(""));
    let receipt = raw(get(e, "book"), "GET", &endpoint)?;
    need(get(&receipt, "status") == &json!(200), "BOOK_HTTP")?;
    let (data, _) = parse_number_lexemes(get(&receipt, "bodyText").as_str().unwrap_or("")).map_err(|_| "PAIRED_REST_RAW_RECEIPT_REQUIRED".to_string())?;
    let book = baal_collector::model::normalize_book(
        &data,
        get(&leg, "marketId").as_str().unwrap_or(""),
        get(get(get(s, "plan"), "scope"), "tournamentId").as_str().unwrap_or(""),
        get(&receipt, "receivedAt").as_str().unwrap_or(""),
        get(&receipt, "requestStartedAt").as_str().unwrap_or(""),
        20000,
        &BTreeSet::new(),
        "authoritative_receipt_v1",
    ).map_err(|e| e)?;
    need(
        get(&data, "contexts").as_array().is_some_and(|c| c.len() == 1)
            && get(&get(&data, "contexts").as_array().unwrap()[0], "type") == &json!("tournament")
            && get(&book, "exchangeId") == get(&leg, "exchangeId")
            && get(&book, "bids").as_array().is_some_and(|x| x.len() <= 20)
            && get(&book, "asks").as_array().is_some_and(|x| x.len() <= 20)
            && instant_s(get(&a, "completedAt"))? <= instant_s(get(&book, "requestStartedAt"))?
            && at - instant_s(get(&book, "requestStartedAt"))? <= 20000
            && instant_s(get(&book, "receivedAt"))? <= at,
        "BOOK_ACCOUNT_CAUSALITY",
    )?;
    let prior = o.map(|x| get(x, "book")).unwrap_or_else(|| get(&leg, "book")).clone();
    need(
        get(&book, "sequence").as_str().unwrap_or("").parse::<i128>().ok() >= get(&prior, "sequence").as_str().unwrap_or("").parse::<i128>().ok()
            && instant_ns(get(&book, "sourceAt").as_str().unwrap_or("")).unwrap_or(i128::MIN) >= instant_ns(get(&prior, "sourceAt").as_str().unwrap_or("")).unwrap_or(i128::MIN)
            && instant_s(get(&book, "requestStartedAt")).unwrap_or(i64::MIN) >= instant_s(get(&prior, "requestStartedAt")).unwrap_or(i64::MIN),
        "BOOK_ROLLBACK",
    )?;
    if get(&book, "versionKey") == get(&prior, "versionKey") {
        need(
            payload_hash(&json!([get(&book, "bids"), get(&book, "asks")])).ok()
                == payload_hash(&json!([get(&prior, "bids"), get(&prior, "asks")])).ok(),
            "BOOK_VERSION_CONFLICT",
        )?;
    }
    need(
        get(&book, "bids").as_array().map(|bids| bids.iter().any(|l| {
            get(l, "quantity").as_i64().map(|q| q / 100 >= 1).unwrap_or(false)
                && price_s(get(l, "price")).map(|p| SCALE - p <= cap).unwrap_or(false)
        })).unwrap_or(false),
        "DEPTH_OR_PRICE",
    )?;
    if let Some(order) = o {
        need(
            at < instant_s(get(get(order, "body"), "expirationDate"))?
                && price_s(get(&binding(s, order, None)?, "limit"))? == cap,
            "EXPIRED_OR_CAP_CHANGED",
        )?;
    }
    Ok(json!({"index": index, "leg": leg, "cap": money(cap), "book": book, "positions": positions, "account": a}))
}
fn canonical_ack(o: &V, r: &V) -> bool {
    (|| -> Result<bool, String> {
        let (d, _) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
        Ok(
            js_int(get(&d, "orderId")).is_some_and(|x| x > 0)
                && get(&d, "exchangeId") == get(get(o, "body"), "exchangeId")
                && get(&d, "side") == &json!("no")
                && get(&d, "action") == &json!("buy")
                && get(&d, "quantity") == &json!(1)
                && amount_s(get(&normalize_official_fill_tick(get(&d, "price"))?, "price"))? == price_s(&json!(baal_execution::js_key(get(get(o, "body"), "price"))))?
                && get(&d, "all") == &V::Null
                && get(&d, "open").is_boolean()
                && js_int(get(&d, "quantityTraded")).is_some()
                && js_int(get(&d, "remainingQuantity")).is_some()
                && js_int(get(&d, "quantityTraded")).unwrap() >= 0
                && js_int(get(&d, "remainingQuantity")).unwrap() >= 0
                && js_int(get(&d, "quantityTraded")).unwrap() + js_int(get(&d, "remainingQuantity")).unwrap() == 1,
        )
    })().unwrap_or(false)
}
fn reduce(s: Option<&V>, e: &V) -> Result<V, String> {
    if get(e, "kind") == &json!("initialize") {
        need(s.is_none(), "ALREADY_INITIALIZED")?;
        return Ok(json!({
            "schemaVersion": 2,
            "plan": derive_paired_rest_plan(&json!({"capture": get(e, "capture"), "scope": get(e, "scope"), "readAt": get(e, "at")}))?,
            "orders": [],
            "externalPositions": V::Null,
            "installedFunding": V::Null,
        }));
    }
    let s = s.ok_or("PAIRED_REST_NOT_INITIALIZED")?;
    let mut n = clone(s);
    {
        let orders = n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap();
        let _ = orders;
    }
    let index = js_int(get(e, "index")).unwrap_or(0) as usize;
    if get(e, "kind") == &json!("funding") {
        let funding_now = paired_rest_funding(s)?;
        need(
            get(get(e, "receipt"), "reservationHash") == get(&funding_now, "reservationHash")
                && get(get(e, "receipt"), "amount") == get(&funding_now, "reservedCash")
                && hex(get(get(e, "receipt"), "globalLedgerHash"))
                && instant_s(get(get(e, "receipt"), "confirmedAt"))? <= instant_s(get(e, "at"))?,
            "FUNDING_RECEIPT",
        )?;
        n.as_object_mut().unwrap().insert("installedFunding".into(), clone(get(e, "receipt")));
        return Ok(n);
    }
    if get(e, "kind") == &json!("prepared") {
        need(active(s).is_none(), "ACTIVE_INTENT")?;
        let v = preflight(s, get(e, "evidence"), instant_s(get(e, "at"))?, None)?;
        need(
            get(e, "index") == get(&v, "index")
                && get(e, "intentId").is_string()
                && get(e, "idempotencyKey").is_string()
                && !get(s, "orders").as_array().map(|a| a.iter().any(|x| {
                    get(x, "intentId") == get(e, "intentId") || get(get(x, "body"), "idempotencyKey") == get(e, "idempotencyKey")
                })).unwrap_or(false),
            "INTENT_IDENTITY",
        )?;
        let cap = amount_s(get(&v, "cap"))?;
        let body = json!({
            "idempotencyKey": get(e, "idempotencyKey"),
            "exchangeId": get(get(&v, "leg"), "exchangeId"),
            "side": "no",
            "action": "buy",
            "quantity": 1,
            "price": cap as f64 / 1e6,
            "expirationDate": iso(std::cmp::min(instant_s(get(e, "at"))? + 30000, instant_s(get(get(s, "plan"), "reviewExpiresAt"))?)),
            "tournamentId": get(get(get(s, "plan"), "scope"), "tournamentId"),
        });
        if get(s, "externalPositions").is_null() {
            n.as_object_mut().unwrap().insert("externalPositions".into(), json!(get(&v, "positions")));
        }
        n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap().push(json!({
            "index": get(&v, "index"),
            "intentId": get(e, "intentId"),
            "marketId": get(get(&v, "leg"), "marketId"),
            "createdAt": get(e, "at"),
            "body": body,
            "bodyJson": canonical_payload(&body),
            "bodyHash": payload_hash(&body)?,
            "before": clone(get(get(e, "evidence"), "account")),
            "reservedElsewhere": get(get(e, "evidence"), "reservedElsewhere"),
            "book": get(&v, "book"),
            "phase": "prepared",
            "orderId": V::Null,
            "ack": V::Null,
            "wireAttempts": [],
            "fetchStarts": [],
            "fences": [],
            "semanticMismatch": false,
            "cancelPending": false,
            "proof": V::Null,
        }));
        return Ok(n);
    }
    // All remaining kinds require a live, unreconciled order at e.index.
    let o_val = get(s, "orders").as_array().and_then(|a| a.get(index)).cloned().unwrap_or(V::Null);
    need(!o_val.is_null() && get(&o_val, "phase") != &json!("reconciled"), "NO_ACTIVE_ORDER")?;
    if get(e, "kind") == &json!("admitted") {
        let op = get(e, "operation").as_str().unwrap_or("");
        match op {
            "place" => need(get(&o_val, "phase") == &json!("prepared"), "PLACEMENT_PHASE")?,
            "recover" => need(
                ["uncertain", "dispatching"].contains(&get(&o_val, "phase").as_str().unwrap_or(""))
                    && get(&o_val, "orderId").is_null()
                    && !baal_service_runtime::json_text::js_truthy(o_val.get("semanticMismatch"))
                    && instant_s(get(e, "at"))? >= instant_s(get(get(&o_val, "body"), "expirationDate"))?
                    && get(&o_val, "fetchStarts").as_array().map(|fs| fs.iter().any(|w| {
                        get(w, "kind") == &json!("place")
                            && instant_s(get(w, "requestStartedAt")).map(|x| x < instant_s(get(get(&o_val, "body"), "expirationDate")).unwrap_or(i64::MIN)).unwrap_or(false)
                    })).unwrap_or(false),
                "RECOVERY_PHASE",
            )?,
            _ => need(
                op == "cancel"
                    && !get(&o_val, "orderId").is_null()
                    && instant_s(get(e, "at"))? >= instant_s(get(get(&o_val, "body"), "expirationDate"))?
                    && !baal_service_runtime::json_text::js_truthy(o_val.get("cancelPending")),
                "CANCEL_PHASE",
            )?,
        }
        let orders = n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap();
        let o = &mut orders[index];
        o.as_object_mut().unwrap().insert("operation".into(), json!(op));
        if op == "cancel" { o.as_object_mut().unwrap().insert("cancelPending".into(), json!(true)); }
        else { o.as_object_mut().unwrap().insert("phase".into(), json!("dispatching")); }
        return Ok(n);
    }
    if get(e, "kind") == &json!("network_fence") {
        need(
            get(&o_val, "operation") == get(e, "operation")
                && hex(get(e, "leaseHash"))
                && payload_hash(get(e, "leaseIdentity")).ok() == get(e, "leaseHash").as_str().map(String::from),
            "NETWORK_FENCE",
        )?;
        let orders = n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap();
        orders[index].as_object_mut().unwrap().get_mut("fences").unwrap().as_array_mut().unwrap().push(json!({
            "operation": get(e, "operation"),
            "at": get(e, "at"),
            "leaseIdentity": get(e, "leaseIdentity"),
            "leaseHash": get(e, "leaseHash"),
        }));
        return Ok(n);
    }
    if get(e, "kind") == &json!("fetch_start") {
        let w = get(e, "start");
        let l = get(&o_val, "fences").as_array().and_then(|f| f.last()).cloned().unwrap_or(V::Null);
        need(
            !l.is_null()
                && get(w, "kind") == get(&o_val, "operation")
                && get(w, "ownerId") == get(get(get(s, "plan"), "scope"), "ownerId")
                && get(w, "runId") == get(get(get(s, "plan"), "scope"), "runId")
                && get(w, "intentId") == get(&o_val, "intentId")
                && get(w, "idempotencyKey") == get(get(&o_val, "body"), "idempotencyKey")
                && get(w, "requestBodyJson") == get(&o_val, "bodyJson")
                && get(w, "requestBodyHash") == get(&o_val, "bodyHash")
                && get(w, "leaseHash") == get(&l, "leaseHash")
                && payload_hash(get(w, "leaseIdentity")).ok() == get(&l, "leaseHash").as_str().map(String::from)
                && get(w, "fetchInvocationId").as_str().is_some_and(|x| !x.is_empty())
                && !get(&n, "orders").as_array().map(|a| a.iter().any(|v| {
                    get(v, "fetchStarts").as_array().map(|fs| fs.iter().any(|x| get(x, "fetchInvocationId") == get(w, "fetchInvocationId"))).unwrap_or(false)
                })).unwrap_or(false)
                && instant_s(get(w, "requestStartedAt"))? >= instant_s(get(&l, "at"))?
                && instant_s(get(w, "requestStartedAt"))? <= instant_s(get(e, "at"))?
                && get(w, "method") == &json!(if get(w, "kind") == &json!("cancel") { "DELETE" } else { "POST" })
                && get(w, "endpoint") == &json!(if get(w, "kind") == &json!("cancel") {
                    format!("/orders/{}", get(&o_val, "orderId").as_str().unwrap_or(""))
                } else { "/orders".to_string() }),
            "FETCH_START_BINDING",
        )?;
        if get(w, "kind") == &json!("place") {
            need(instant_s(get(w, "requestStartedAt"))? < instant_s(get(get(&o_val, "body"), "expirationDate"))?, "ORIGINAL_WIRE_EXPIRED")?;
        }
        let orders = n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap();
        orders[index].as_object_mut().unwrap().get_mut("fetchStarts").unwrap().as_array_mut().unwrap().push(clone(w));
        return Ok(n);
    }
    if get(e, "kind") == &json!("wire") {
        let w = get(e, "wire");
        let started = get(&o_val, "fetchStarts").as_array().and_then(|f| f.last()).cloned().unwrap_or(V::Null);
        need(
            !started.is_null()
                && get(w, "fetchInvocationId") == get(&started, "fetchInvocationId")
                && get(w, "requestStartedAt") == get(&started, "requestStartedAt")
                && get(w, "kind") == get(&o_val, "operation")
                && get(w, "ownerId") == get(get(get(s, "plan"), "scope"), "ownerId")
                && get(w, "runId") == get(get(get(s, "plan"), "scope"), "runId")
                && get(w, "intentId") == get(&o_val, "intentId")
                && get(w, "idempotencyKey") == get(get(&o_val, "body"), "idempotencyKey")
                && get(w, "requestBodyJson") == get(&o_val, "bodyJson")
                && get(w, "requestBodyHash") == get(&o_val, "bodyHash")
                && get(w, "leaseHash") == get(&o_val, "fences").as_array().and_then(|f| f.last()).map(|l| get(l, "leaseHash")).unwrap_or(&V::Null)
                && instant_s(get(w, "requestStartedAt"))? >= instant_s(get(&get(&o_val, "fences").as_array().and_then(|f| f.last()).cloned().unwrap_or(V::Null), "at"))?
                && instant_s(get(w, "receivedAt"))? >= instant_s(get(w, "requestStartedAt"))?
                && instant_s(get(w, "receivedAt"))? <= instant_s(get(e, "at"))?
                && hex(get(w, "sourceBodyHash"))
                && sha(get(w, "bodyText").as_str().unwrap_or("").as_bytes()) == get(w, "sourceBodyHash").as_str().unwrap_or(""),
            "WIRE_PROVENANCE",
        )?;
        if get(w, "kind") == &json!("place") {
            need(instant_s(get(w, "requestStartedAt"))? < instant_s(get(get(&o_val, "body"), "expirationDate"))?, "ORIGINAL_WIRE_EXPIRED")?;
        }
        let orders = n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap();
        let o = &mut orders[index];
        o.as_object_mut().unwrap().get_mut("wireAttempts").unwrap().as_array_mut().unwrap().push(clone(w));
        if get(w, "kind") == &json!("cancel") {
            o.as_object_mut().unwrap().insert("cancelPending".into(), json!(true));
            return Ok(n);
        }
        let status = js_int(get(w, "status")).unwrap_or(0);
        if (200..300).contains(&status) {
            let d = parse_number_lexemes(get(w, "bodyText").as_str().unwrap_or("")).map(|(x, _)| x).unwrap_or(V::Null);
            if js_int(get(&d, "orderId")).is_some_and(|x| x > 0) {
                let sid = js_int(get(&d, "orderId")).unwrap().to_string();
                need(get(o, "orderId").is_null() || get(o, "orderId") == &json!(sid), "ORDER_ID_CHANGED")?;
                o.as_object_mut().unwrap().insert("orderId".into(), json!(sid));
            }
            let ack = json!({
                "method": "POST",
                "endpoint": "/orders",
                "status": get(w, "status"),
                "requestStartedAt": get(w, "requestStartedAt"),
                "receivedAt": get(w, "receivedAt"),
                "bodyText": get(w, "bodyText"),
                "sourceBodyHash": get(w, "sourceBodyHash"),
                "requestBodyHash": get(o, "bodyHash"),
            });
            let mismatch = !canonical_ack(o, &ack);
            o.as_object_mut().unwrap().insert("ack".into(), ack);
            o.as_object_mut().unwrap().insert("semanticMismatch".into(), json!(mismatch));
            o.as_object_mut().unwrap().insert("phase".into(), json!(if mismatch { "semantic_mismatch" } else { "acknowledged" }));
        } else {
            o.as_object_mut().unwrap().insert("phase".into(), json!("uncertain"));
        }
        return Ok(n);
    }
    if get(e, "kind") == &json!("error") {
        let orders = n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap();
        let o = &mut orders[index];
        if get(o, "phase") == &json!("dispatching") {
            o.as_object_mut().unwrap().insert("phase".into(), json!("uncertain"));
        }
        o.as_object_mut().unwrap().insert("lastError".into(), get(e, "code").clone());
        return Ok(n);
    }
    if get(e, "kind") == &json!("reconciled") {
        need(!get(&o_val, "orderId").is_null() && !get(&o_val, "ack").is_null() && !baal_service_runtime::json_text::js_truthy(o_val.get("semanticMismatch")), "RECONCILIATION_BINDING")?;
        keys(get(e, "evidence"), &["order", "fillPages", "after", "globalRisk", "accountActivity"])?;
        account_shape(get(get(e, "evidence"), "after"))?;
        raw(get(get(e, "evidence"), "order"), "GET", &format!("/orders/{}", get(&o_val, "orderId").as_str().unwrap_or("")))?;
        for r in get(get(e, "evidence"), "fillPages").as_array().cloned().unwrap_or_default() {
            raw(&r, "GET", get(&r, "endpoint").as_str().unwrap_or(""))?;
        }
        let mut verify_in = get(e, "evidence").clone();
        let vm = verify_in.as_object_mut().unwrap();
        vm.insert("binding".into(), binding(s, &o_val, None)?);
        vm.insert("before".into(), get(&o_val, "before").clone());
        vm.insert("ack".into(), get(&o_val, "ack").clone());
        if let Some(rp) = e.get("recoveryProof") { vm.insert("recoveryProof".into(), rp.clone()); }
        vm.insert("readAt".into(), get(e, "at").clone());
        vm.insert("fullPairReservation".into(), get(&paired_rest_funding(s)?, "reservedCash").clone());
        vm.insert("reservedElsewhere".into(), get(&o_val, "reservedElsewhere").clone());
        let proof = verify_scoped_rest_no_buy(&verify_in)?;
        need(
            get(&proof, "scopedPrincipalCost") == &json!(money(amount_s(get(&proof, "scopedPrincipalCost"))?))
                && cost(s) + amount_s(get(&proof, "scopedPrincipalCost"))? <= amount_s(get(get(s, "plan"), "pairCostCeiling"))?
                && cost(s) + amount_s(get(&proof, "scopedPrincipalCost"))? <= 2_000_000,
            "PAIR_PRINCIPAL",
        )?;
        let orders = n.as_object_mut().unwrap().get_mut("orders").unwrap().as_array_mut().unwrap();
        let o = &mut orders[index];
        o.as_object_mut().unwrap().insert("proof".into(), proof);
        o.as_object_mut().unwrap().insert("phase".into(), json!("reconciled"));
        o.as_object_mut().unwrap().insert("cancelPending".into(), json!(false));
        return Ok(n);
    }
    Err("PAIRED_REST_EVENT".into())
}

type Now = Rc<dyn Fn() -> i64>;

pub struct PairedRestController {
    dir: PathBuf,
    scope: V,
    dispatch: Rc<dyn for<'g> Fn(&V, Option<&'g dyn Fn(&V,&V)->Result<bool,OrderTransportError>>, Option<&'g dyn Fn(&V,&V)->Result<bool,OrderTransportError>>) -> Result<V, OrderTransportError>>,
    can_operate: Rc<dyn Fn() -> bool>,
    assert_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    install_funding_reservation: Rc<dyn Fn(V) -> Result<V, String>>,
    get_evidence: Rc<dyn Fn(&V) -> Result<V, String>>,
    raw_receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    now: Now,
    file: PathBuf,
    code: V,
    state: RefCell<Option<V>>,
    closed: RefCell<bool>,
    failed: RefCell<bool>,
    busy: RefCell<bool>,
    size: RefCell<usize>,
    bytes_hash: RefCell<String>,
    stat: RefCell<(u64, f64)>,
    journal: RefCell<Journal>,
    rows: RefCell<Vec<V>>,
    issued: RefCell<Vec<V>>,
    in_flight: RefCell<V>,
    transport: RefCell<Option<OrderTransport>>,
    me: RefCell<std::rc::Weak<Self>>,
}

impl PairedRestController {
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        dir: &Path,
        capture: Option<&V>,
        scope: &V,
        dispatch: Rc<dyn for<'g> Fn(&V, Option<&'g dyn Fn(&V,&V)->Result<bool,OrderTransportError>>, Option<&'g dyn Fn(&V,&V)->Result<bool,OrderTransportError>>) -> Result<V, OrderTransportError>>,
        can_operate: Rc<dyn Fn() -> bool>,
        assert_account_lease: Rc<dyn Fn() -> Result<V, String>>,
        install_funding_reservation: Rc<dyn Fn(V) -> Result<V, String>>,
        get_evidence: Rc<dyn Fn(&V) -> Result<V, String>>,
        raw_receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
        now: Now,
    ) -> Result<Rc<Self>, String> {
        need(dir.is_absolute(), "DEPENDENCIES")?;
        let file = dir.join("paired-rest-journal.jsonl");
        let code = json!(paired_rest_code_identity(&execution_dir()));
        let this = Rc::new(Self {
            dir: dir.to_path_buf(),
            scope: clone(scope),
            dispatch,
            can_operate,
            assert_account_lease,
            install_funding_reservation,
            get_evidence,
            raw_receipt_of,
            now,
            file: file.clone(),
            code,
            state: RefCell::new(None),
            closed: RefCell::new(false),
            failed: RefCell::new(false),
            busy: RefCell::new(false),
            size: RefCell::new(0),
            bytes_hash: RefCell::new(String::new()),
            stat: RefCell::new((0, 0.0)),
            journal: RefCell::new(Journal::open(&file).map_err(|e| e)?),
            rows: RefCell::new(Vec::new()),
            issued: RefCell::new(Vec::new()),
            in_flight: RefCell::new(V::Null),
            transport: RefCell::new(None),
            me: RefCell::new(std::rc::Weak::new()),
        });
        *this.me.borrow_mut() = Rc::downgrade(&this);
        this.init(capture)?;
        Ok(this)
    }
    fn basic(&self) -> Result<V, String> {
        need(!*self.closed.borrow() && !*self.failed.borrow() && (self.can_operate)(), "STOPPED_OR_FAILED")?;
        let l = (self.assert_account_lease)()?;
        need(
            get(&l, "ownerId") == get(&self.scope, "ownerId")
                && get(&l, "tournamentId") == get(&self.scope, "tournamentId")
                && (l.get("runId").is_none() || get(&l, "runId") == get(&self.scope, "runId"))
                && get(&l, "leaseId").as_str().is_some_and(|s| !s.is_empty()),
            "ACCOUNT_LEASE",
        )?;
        keys(&l, &["ownerId", "runId", "tournamentId", "leaseId"])?;
        Ok(clone(&l))
    }
    fn recovery_member(&self, input: &V) -> Result<(), String> {
        if let Some(rp) = input.get("recoveryProof") {
            let r = rp.get("journalRecord").cloned().unwrap_or(V::Null);
            need(
                !r.is_null()
                    && self.rows.borrow().iter().any(|v| {
                        get(v, "sequence") == get(&r, "sequence") && get(v, "hash") == get(&r, "hash")
                            && payload_hash(v).ok() == payload_hash(&r).ok()
                    }),
                "RECOVERY_JOURNAL_MEMBERSHIP",
            )?;
        }
        Ok(())
    }
    fn init(&self, capture: Option<&V>) -> Result<(), String> {
        self.basic()?;
        if !self.file.exists() {
            let f = std::fs::OpenOptions::new().write(true).create_new(true).open(&self.file).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&self.file, std::fs::Permissions::from_mode(0o600)).ok(); }
            drop(f);
        }
        let bytes = std::fs::read(&self.file).map_err(|e| e.to_string())?;
        need(bytes.is_empty() || bytes.last() == Some(&10), "PARTIAL_JOURNAL")?;
        for line in String::from_utf8_lossy(&bytes).split('\n').filter(|l| !l.is_empty()) {
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            need(
                payload_hash(get(get(&row, "event"), "codeIdentity")).ok() == payload_hash(&self.code).ok(),
                "CODE_CHANGED",
            )?;
            self.recovery_member(get(&row, "event").get("input").unwrap_or(&V::Null))?;
            let next = reduce(self.state.borrow().as_ref(), get(get(&row, "event"), "input"))?;
            need(payload_hash(&next).ok() == payload_hash(get(&row, "state")).ok(), "REPLAY_STATE")?;
            *self.state.borrow_mut() = Some(next);
            self.rows.borrow_mut().push(row);
        }
        *self.size.borrow_mut() = bytes.len();
        *self.bytes_hash.borrow_mut() = sha(&bytes);
        let meta = std::fs::metadata(&self.file).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            *self.stat.borrow_mut() = (meta.ino(), meta.mtime() as f64);
        }
        if self.state.borrow().is_none() {
            self.append(&json!({"kind": "initialize", "capture": capture, "scope": self.scope, "at": iso((self.now)())}))?;
        } else {
            let s = self.state.borrow();
            let st = s.clone().unwrap();
            need(
                payload_hash(get(get(&st, "plan"), "scope")).ok() == payload_hash(&self.scope).ok()
                    && (capture.is_none() || get(get(&st, "plan"), "captureHash").as_str() == payload_hash(capture.unwrap()).ok().as_deref()),
                "SCOPE_OR_CAPTURE_CHANGED",
            )?;
        }
        Ok(())
    }
    pub fn append(&self, input: &V) -> Result<(), String> {
        self.basic()?;
        let before = std::fs::read(&self.file).map_err(|e| e.to_string())?;
        let meta = std::fs::metadata(&self.file).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        { use std::os::unix::fs::MetadataExt; need(meta.ino() == self.stat.borrow().0, "JOURNAL_CHANGED")?; }
        need(
            before.len() == *self.size.borrow() && sha(&before) == *self.bytes_hash.borrow()
                && payload_hash(&self.code).ok() == payload_hash(&self.code).ok(),
            "JOURNAL_CHANGED",
        )?;
        self.recovery_member(input)?;
        let next = reduce(self.state.borrow().as_ref(), input)?;
        need(
            *self.size.borrow() + js_stringify(&next).len() + js_stringify(input).len() < 64 * 1024 * 1024,
            "JOURNAL_BOUND",
        )?;
        let result = (|| -> Result<(), String> {
            self.journal.borrow_mut().commit(&next, &json!({"codeIdentity": self.code, "input": input})).map_err(|e| e)?;
            self.basic()?;
            let all = std::fs::read(&self.file).map_err(|e| e.to_string())?;
            need(all[..*self.size.borrow()] == before[..] && all.len() > *self.size.borrow(), "JOURNAL_CHANGED")?;
            let row: V = serde_json::from_str(std::str::from_utf8(&all[*self.size.borrow()..]).map_err(|e| e.to_string())?.trim_end()).map_err(|e| e.to_string())?;
            self.rows.borrow_mut().push(row);
            *self.state.borrow_mut() = Some(next);
            *self.size.borrow_mut() = all.len();
            *self.bytes_hash.borrow_mut() = sha(&all);
            Ok(())
        })();
        if result.is_err() { *self.failed.borrow_mut() = true; }
        result
    }
    fn evidence(&self, operation: &str, o: Option<&V>) -> Result<V, String> {
        let ctx = json!({
            "operation": operation,
            "marketId": o.map(|x| get(x, "marketId").clone()).unwrap_or_else(|| get(&self.plan_legs(), "marketId").clone()),
            "exchangeId": o.map(|x| get(get(x, "body"), "exchangeId").clone()).unwrap_or_else(|| get(&self.plan_legs(), "exchangeId").clone()),
            "orderId": o.map(|x| get(x, "orderId")).cloned().unwrap_or(V::Null),
            "reservation": paired_rest_funding(self.state.borrow().as_ref().unwrap())?,
        });
        (self.get_evidence)(&ctx)
    }
    fn plan_legs(&self) -> V {
        let s = self.state.borrow().clone().unwrap_or(V::Null);
        let legs = get(get(&s, "plan"), "legs").as_array().cloned().unwrap_or_default();
        let idx = get(&s, "orders").as_array().map(|a| a.len()).unwrap_or(0);
        legs.get(idx).cloned().unwrap_or(V::Null)
    }
    pub fn authorizes(&self, spec: &V) -> bool {
        let _ = self.basic();
        let p = self.issued.borrow().iter().find(|i| get(i, "beforeDispatch") == get(spec, "beforeDispatch")).cloned();
        p.as_ref().map(|pp| {
            get(pp, "method") == get(spec, "method")
                && get(pp, "endpoint") == get(spec, "endpoint")
                && get(pp, "bodyJson") == get(spec, "bodyJson")
                && get(pp, "beforeNetwork") == get(spec, "beforeNetwork")
        }).unwrap_or(false)
            && self.transport.borrow().as_ref().map(|t| t.authorize_request(spec).unwrap_or(false)).unwrap_or(false)
    }
    pub fn on_fetch_start(&self, meta: &V) -> Result<bool, String> {
        if get(meta, "method") == &json!("GET") { return Ok(false); }
        if self.in_flight.borrow().is_null() { return Ok(false); }
        self.basic()?;
        keys(meta, &["fetchInvocationId", "method", "endpoint", "requestBodyHash", "requestStartedAt"])?;
        let in_flight = self.in_flight.borrow().clone();
        let s = self.state.borrow().clone().unwrap_or(V::Null);
        let idx = js_int(get(&in_flight, "index")).unwrap_or(0) as usize;
        let o = get(&s, "orders").as_array().and_then(|a| a.get(idx)).cloned().unwrap_or(V::Null);
        let l = get(&o, "fences").as_array().and_then(|f| f.last()).cloned().unwrap_or(V::Null);
        need(
            !l.is_null()
                && get(meta, "method") == get(get(&in_flight, "spec"), "method")
                && get(meta, "endpoint") == get(get(&in_flight, "spec"), "endpoint")
                && get(meta, "requestBodyHash") == &json!(if get(meta, "method") == &json!("POST") { get(&o, "bodyHash").clone() } else { V::Null }),
            "FETCH_START_UNBOUND",
        )?;
        let mut start = meta.clone();
        start.as_object_mut().unwrap().insert("kind".into(), get(&o, "operation").clone());
        start.as_object_mut().unwrap().insert("ownerId".into(), get(&self.scope, "ownerId").clone());
        start.as_object_mut().unwrap().insert("runId".into(), get(&self.scope, "runId").clone());
        start.as_object_mut().unwrap().insert("intentId".into(), get(&o, "intentId").clone());
        start.as_object_mut().unwrap().insert("idempotencyKey".into(), get(get(&o, "body"), "idempotencyKey").clone());
        start.as_object_mut().unwrap().insert("requestBodyJson".into(), get(&o, "bodyJson").clone());
        start.as_object_mut().unwrap().insert("requestBodyHash".into(), get(&o, "bodyHash").clone());
        start.as_object_mut().unwrap().insert("leaseIdentity".into(), get(&l, "leaseIdentity").clone());
        start.as_object_mut().unwrap().insert("leaseHash".into(), get(&l, "leaseHash").clone());
        self.append(&json!({"kind": "fetch_start", "index": idx, "start": start, "at": iso((self.now)())}))?;
        Ok(true)
    }
    pub fn status(&self) -> V {
        let s = self.state.borrow().clone().unwrap_or(V::Null);
        let o = active(&s);
        let owned: Vec<V> = get(&s, "orders").as_array().cloned().unwrap_or_default().iter()
            .flat_map(|x| {
                if !get(get(x, "proof"), "lot").is_null() {
                    let mut m = json!({"marketId": get(x, "marketId"), "exchangeId": get(get(x, "body"), "exchangeId")});
                    for (k, v) in get(get(x, "proof"), "lot").as_object().unwrap() { m.as_object_mut().unwrap().insert(k.clone(), v.clone()); }
                    vec![m]
                } else { vec![] }
            }).collect();
        let hedge_blocked = o.is_none() && !ended(&s) && owned.len() == 1
            && (self.now)() >= instant_s(get(get(&s, "plan"), "reviewExpiresAt")).unwrap_or(i64::MAX);
        let journal = self.journal.borrow();
        json!({
            "mode": "paired_rest_lifecycle_v2",
            "planHash": get(get(&s, "plan"), "planHash"),
            "originalDryPlanHash": get(get(&s, "plan"), "originalDryPlanHash"),
            "originalProtectiveCost": get(get(&s, "plan"), "originalProtectiveCost"),
            "pairCostCeiling": get(get(&s, "plan"), "pairCostCeiling"),
            "minimumOrdinaryMargin": get(get(&s, "plan"), "sourceMinimumOrdinaryMargin"),
            "planLegs": get(get(&s, "plan"), "legs").as_array().map(|l| l.iter().map(|x| json!({
                "marketId": get(x, "marketId"), "exchangeId": get(x, "exchangeId"), "originalLimit": get(x, "originalLimit"),
            })).collect::<Vec<V>>()).unwrap_or_default(),
            "reviewExpiresAt": get(get(&s, "plan"), "reviewExpiresAt"),
            "sequence": journal.sequence,
            "hash": journal.previous,
            "closed": *self.closed.borrow(),
            "failed": *self.failed.borrow(),
            "busy": *self.busy.borrow(),
            "phase": o.map(|x| get(x, "phase").clone()).unwrap_or_else(|| json!(if ended(&s) {
                if owned.len() == 2 { "paired" } else if owned.is_empty() { "unfilled" } else { "orphaned" }
            } else if hedge_blocked { "hedge_blocked" } else { "ready" })),
            "blockedReason": if hedge_blocked { json!("rules_expired") } else { V::Null },
            "funding": paired_rest_funding(&s).unwrap_or(V::Null),
            "owned": owned,
            "orders": get(&s, "orders").as_array().map(|a| a.iter().map(|x| json!({
                "intentId": get(x, "intentId"),
                "orderId": get(x, "orderId"),
                "marketId": get(x, "marketId"),
                "exchangeId": get(get(x, "body"), "exchangeId"),
                "phase": get(x, "phase"),
                "body": clone(get(x, "body")),
                "bodyHash": get(x, "bodyHash"),
                "cancelPending": get(x, "cancelPending"),
                "actualWireStarts": get(x, "wireAttempts").as_array().map(|w| w.iter().map(|v| get(v, "requestStartedAt").clone()).collect::<Vec<V>>()).unwrap_or_default(),
                "lastActualFetchStartAt": get(x, "fetchStarts").as_array().and_then(|f| f.last()).map(|v| get(v, "requestStartedAt").clone()).unwrap_or(V::Null),
                "actualFetchCount": get(x, "fetchStarts").as_array().map(|f| f.len()).unwrap_or(0),
                "recoveryAttemptCount": get(x, "fetchStarts").as_array().map(|f| f.iter().filter(|w| get(w, "kind") == &json!("recover")).count()).unwrap_or(0),
                "semanticMismatch": get(x, "semanticMismatch"),
                "proofHash": get(get(x, "proof"), "verificationHash"),
            })).collect::<Vec<V>>()).unwrap_or_default(),
            "globalActivity": get(&s, "orders").as_array().map(|a| a.iter().map(|x| {
                get(get(get(x, "proof"), "globalActivity"), "status").as_str().map(|v| json!(v)).unwrap_or(json!("unknown"))
            }).collect::<Vec<V>>()).unwrap_or_default(),
            "additionalApiClients": 0,
        })
    }
    pub fn funding_reservation(&self) -> Result<V, String> {
        paired_rest_funding(self.state.borrow().as_ref().unwrap())
    }
    fn reserve(&self) -> Result<V, String> {
        self.basic()?;
        let wanted = paired_rest_funding(self.state.borrow().as_ref().unwrap())?;
        let r = (self.install_funding_reservation)(clone(&wanted))?;
        self.basic()?;
        self.append(&json!({"kind": "funding", "receipt": r, "at": iso((self.now)())}))?;
        Ok(r)
    }
    pub fn prepare_next(&self) -> Result<V, String> {
        need(!*self.busy.borrow(), "BUSY")?;
        *self.busy.borrow_mut() = true;
        let result = (|| {
            self.basic()?;
            let s = self.state.borrow().clone().unwrap();
            need(active(&s).is_none() && !ended(&s), "ACTIVE_OR_FINISHED")?;
            drop(s);
            self.reserve()?;
            let e = self.evidence("prepare", None)?;
            let s = self.state.borrow().clone().unwrap();
            let index = get(&s, "orders").as_array().map(|a| a.len()).unwrap_or(0);
            drop(s);
            self.append(&json!({
                "kind": "prepared",
                "index": index,
                "intentId": format!("paired-{}", uuid::Uuid::new_v4()),
                "idempotencyKey": uuid::Uuid::new_v4().to_string(),
                "evidence": e,
                "at": iso((self.now)()),
            }))?;
            Ok(self.status())
        })();
        *self.busy.borrow_mut() = false;
        result
    }
    pub fn reconcile(&self, input: &V) -> Result<V, String> {
        need(!*self.busy.borrow(), "BUSY")?;
        *self.busy.borrow_mut() = true;
        let result = (|| {
            self.basic()?;
            let s = self.state.borrow().clone().unwrap();
            let o = active(&s).cloned().unwrap_or(V::Null);
            need(!get(&o, "orderId").is_null() && !get(&o, "ack").is_null(), "ORDER_ACK_REQUIRED")?;
            let recovery_proof = if instant_s(get(get(&o, "ack"), "requestStartedAt")).unwrap_or(0) >= instant_s(get(get(&o, "body"), "expirationDate")).unwrap_or(i64::MIN) {
                json!({"journalRecord": self.recover_row(&o)?})
            } else { V::Null };
            self.append(&json!({
                "kind": "reconciled",
                "index": get(&o, "index"),
                "evidence": input,
                "recoveryProof": recovery_proof,
                "at": iso((self.now)()),
            }))?;
            self.reserve()?;
            Ok(self.status())
        })();
        *self.busy.borrow_mut() = false;
        result
    }
    fn dispatch_order(&self, operation: &str) -> Result<V, String> {
        need(!*self.busy.borrow(), "BUSY")?;
        *self.busy.borrow_mut() = true;
        let result = (|| {
            self.basic()?;
            let s = self.state.borrow().clone().unwrap();
            let o = active(&s).cloned().unwrap_or(V::Null);
            need(!o.is_null(), "NO_ACTIVE_ORDER")?;
            if operation == "place" { need(get(&o, "phase") == &json!("prepared"), "UNKNOWN_BLOCKS_REPLACEMENT")?; }
            if operation == "recover" { self.recover_row(&o)?; }
            drop(s);
            self.reserve()?;
            let s = self.state.borrow().clone().unwrap();
            let o = active(&s).cloned().unwrap_or(V::Null);
            let l = self.basic()?;
            let mut permit = json!({
                "intentId": get(&o, "intentId"),
                "runId": get(&self.scope, "runId"),
                "bodyHash": get(&o, "bodyHash"),
                "operation": operation,
            });
            if operation == "recover" {
                permit.as_object_mut().unwrap().insert("retry".into(), json!(true));
            }
            self.ensure_transport().map_err(|e| e.code)?;
            let transport_ref = self.transport.borrow();
            let transport = transport_ref.as_ref().unwrap();
            let result = if operation == "cancel" {
                transport.cancel(get(&o, "orderId"), &json!({"permit": permit, "lease": l}))
            } else {
                transport.place(get(&o, "body"), &json!({"permit": permit, "lease": l}))
            };
            if let Err(e) = result {
                if *self.failed.borrow() {
                    return Err("PAIRED_REST_DURABLE_RECORDING_FAILED".into());
                }
                if (self.can_operate)() {
                    let code = if !e.code.is_empty() && e.code.bytes().all(|b| b.is_ascii_uppercase() || b == b'_') {
                        e.code.clone()
                    } else { "REQUEST_UNCONFIRMED".into() };
                    self.append(&json!({"kind": "error", "index": get(&o, "index"), "code": code, "at": iso((self.now)())}))?;
                }
                return Err(e.code);
            }
            Ok(self.status())
        })();
        *self.busy.borrow_mut() = false;
        result
    }
    pub fn dispatch_prepared(&self) -> Result<V, String> { self.dispatch_order("place") }
    pub fn recover_exact(&self) -> Result<V, String> { self.dispatch_order("recover") }
    pub fn cancel_expired(&self) -> Result<V, String> { self.dispatch_order("cancel") }
}
impl crate::paired_rest_cycle::PairedCycleHost for PairedRestController {
    fn status(&self) -> V { PairedRestController::status(self) }
    fn prepare_next(&self) -> Result<V, String> { PairedRestController::prepare_next(self) }
    fn dispatch_prepared(&self) -> Result<V, String> { PairedRestController::dispatch_prepared(self) }
    fn recover_exact(&self) -> Result<V, String> { PairedRestController::recover_exact(self) }
    fn reconcile(&self, evidence: &V) -> Result<V, String> { PairedRestController::reconcile(self, evidence) }
    fn cancel_expired(&self) -> Result<V, String> { PairedRestController::cancel_expired(self) }
}
impl PairedRestController {
    fn ensure_transport(&self) -> Result<(), OrderTransportError> {
        if self.transport.borrow().is_some() { return Ok(()); }
        let me = self.me.borrow().upgrade().ok_or_else(|| OrderTransportError {
            code: "PAIRED_REST_STOPPED_OR_FAILED".into(), status: None, api_code: None,
            outcome: "rejected".into(), may_have_executed: false,
            retry_same_request_only: false, retry_after_ms: 0.0, semantic_mismatch: false,
        })?;
        let t = self.build_transport(&me)?;
        *self.transport.borrow_mut() = Some(t);
        Ok(())
    }
    fn build_transport(&self, this: &Rc<Self>) -> Result<OrderTransport, OrderTransportError> {
        let scope = scope_of(&json!({
            "tournamentId": get(&self.scope, "tournamentId"),
            "tournamentSlug": get(&self.scope, "tournamentSlug"),
            "exchanges": get(&self.scope, "markets"),
        }))?;
        fn terr(e: String) -> OrderTransportError {
            OrderTransportError {
                code: e, status: None, api_code: None, outcome: "rejected".into(),
                may_have_executed: false, retry_same_request_only: false,
                retry_after_ms: 0.0, semantic_mismatch: false,
            }
        }
        let w1 = Rc::downgrade(this);
        let w2 = Rc::downgrade(this);
        let w3 = Rc::downgrade(this);
        let w4 = Rc::downgrade(this);
        let w5 = Rc::downgrade(this);
        let w6 = Rc::downgrade(this);
        let now = self.now.clone();
        OrderTransport::new(OrderTransportOptions {
            dispatch: Box::new(move |spec: &V, _bd: Option<&baal_execution::transport::Guard>, _bn: Option<&baal_execution::transport::Guard>| {
                let this = w1.upgrade().ok_or_else(|| terr("PAIRED_REST_STOPPED_OR_FAILED".into()))?;
                this.wrap(spec)
            }),
            scope,
            can_write: Box::new(move || w2.upgrade().map(|t| t.basic().is_ok()).unwrap_or(false)),
            owns_order: Box::new(move |id: &str| {
                w3.upgrade().map(|t| {
                    let s = t.state.borrow().clone().unwrap_or(V::Null);
                    get(&s, "orders").as_array().map(|a| a.iter().any(|o| get(o, "orderId").as_str() == Some(id))).unwrap_or(false)
                }).unwrap_or(false)
            }),
            validate_permit: Box::new(move |p: &V, spec: &V| {
                w4.upgrade().map(|t| t.validate(p, spec, false).map_err(terr)).unwrap_or(Ok(false))
            }),
            validate_network_permit: Box::new(move |p: &V, spec: &V| {
                w5.upgrade().map(|t| t.validate(p, spec, true).map_err(terr)).unwrap_or(Ok(false))
            }),
            validate_lease: Box::new(move |l: &V, _u: &V| {
                w6.upgrade().map(|t| {
                    t.basic().ok().and_then(|b| payload_hash(&b).ok())
                        .map(|h| payload_hash(l).ok() == Some(h)).unwrap_or(false)
                }).map(Ok).unwrap_or(Ok(false))
            }),
            now: Box::new(move || now()),
            max_pages: 10,
        })
    }
    fn wrap(&self, spec: &V) -> Result<V, OrderTransportError> {
        let s = self.state.borrow().clone().unwrap_or(V::Null);
        let o = active(&s).cloned().unwrap_or(V::Null);
        let index = js_int(get(&o, "index")).unwrap_or(0);
        self.issued.borrow_mut().push(json!({
            "method": get(spec, "method"),
            "endpoint": get(spec, "endpoint"),
            "bodyJson": get(spec, "bodyJson"),
            "beforeDispatch": get(spec, "beforeDispatch"),
            "beforeNetwork": get(spec, "beforeNetwork"),
        }));
        *self.in_flight.borrow_mut() = json!({"index": index, "spec": spec});
        let capture = |response: &V| -> Result<(), String> {
            let body_hash = get(get(&o, "body"), "bodyHash").clone();
            let record = raw(
                &(self.raw_receipt_of)(response, &json!({
                    "method": get(spec, "method"),
                    "requestBodyHash": if get(spec, "method") == &json!("POST") { body_hash } else { V::Null },
                }))?,
                get(spec, "method").as_str().unwrap_or(""),
                get(spec, "endpoint").as_str().unwrap_or(""),
            )?;
            need(
                instant_s(get(&record, "requestStartedAt"))? >= instant_s(get(response, "requestStartedAt"))?
                    && instant_s(get(&record, "receivedAt"))? <= instant_s(get(response, "receivedAt"))?
                    && get(&record, "status") == get(response, "status"),
                "WIRE_CLOCK_RELABELED",
            )?;
            if !get(response, "data").is_null() {
                need(
                    payload_hash(&parse_number_lexemes(get(&record, "bodyText").as_str().unwrap_or(""))?.0).ok()
                        == payload_hash(get(response, "data")).ok(),
                    "WIRE_BODY_CHANGED",
                )?;
            }
            let s = self.state.borrow().clone().unwrap_or(V::Null);
            let current = get(&s, "orders").as_array().and_then(|a| a.get(index as usize)).cloned().unwrap_or(V::Null);
            let l = get(&current, "fences").as_array().and_then(|f| f.last()).cloned().unwrap_or(V::Null);
            need(!l.is_null(), "NETWORK_FENCE_MISSING")?;
            let wire = json!({
                "kind": get(&current, "operation"),
                "method": get(spec, "method"),
                "endpoint": get(spec, "endpoint"),
                "ownerId": get(&self.scope, "ownerId"),
                "runId": get(&self.scope, "runId"),
                "intentId": get(&current, "intentId"),
                "idempotencyKey": get(get(&current, "body"), "idempotencyKey"),
                "requestBodyJson": get(&current, "bodyJson"),
                "requestBodyHash": get(&current, "bodyHash"),
                "leaseIdentity": get(&l, "leaseIdentity"),
                "leaseHash": get(&l, "leaseHash"),
                "fetchInvocationId": get(&record, "fetchInvocationId"),
                "requestStartedAt": get(&record, "requestStartedAt"),
                "receivedAt": get(&record, "receivedAt"),
                "status": get(&record, "status"),
                "sourceBodyHash": get(&record, "sourceBodyHash"),
                "bodyText": get(&record, "bodyText"),
            });
            self.append(&json!({"kind": "wire", "index": index, "wire": wire, "at": iso((self.now)())}))?;
            Ok(())
        };
        let result = (self.dispatch)(spec, None, None);
        let out = match result {
            Ok(response) => {
                if let Err(e) = capture(&response) {
                    *self.failed.borrow_mut() = true;
                    return Err(OrderTransportError {
                        code: e, status: None, api_code: None, outcome: "rejected".into(),
                        may_have_executed: true, retry_same_request_only: false,
                        retry_after_ms: 0.0, semantic_mismatch: false,
                    });
                }
                Ok(response)
            }
            Err(e) => Err(e),
        };
        self.issued.borrow_mut().retain(|i| get(i, "beforeDispatch") != get(spec, "beforeDispatch"));
        *self.in_flight.borrow_mut() = V::Null;
        out
    }
    fn validate(&self, p: &V, spec: &V, network: bool) -> Result<bool, String> {
        self.basic()?;
        let s = self.state.borrow().clone().unwrap_or(V::Null);
        let o = active(&s).cloned().unwrap_or(V::Null);
        need(
            !o.is_null()
                && get(p, "intentId") == get(&o, "intentId")
                && get(p, "runId") == get(&self.scope, "runId")
                && get(p, "bodyHash") == get(&o, "bodyHash"),
            "PERMIT_IDENTITY",
        )?;
        let op = get(p, "operation").as_str().unwrap_or("");
        if op == "cancel" {
            need(
                get(spec, "method") == &json!("DELETE")
                    && get(spec, "endpoint") == &json!(format!("/orders/{}", get(&o, "orderId").as_str().unwrap_or(""))),
                "CANCEL_SCOPE",
            )?;
            let e = self.evidence("cancel", Some(&o))?;
            self.cancel_gate(&o, &e, (self.now)())?;
        } else {
            need(
                get(spec, "method") == &json!("POST")
                    && get(spec, "endpoint") == &json!("/orders")
                    && get(spec, "bodyJson") == get(&o, "bodyJson"),
                "PLACEMENT_PAYLOAD",
            )?;
            if op == "recover" {
                need(
                    get(&o, "orderId").is_null()
                        && !baal_service_runtime::json_text::js_truthy(o.get("semanticMismatch"))
                        && (self.now)() >= instant_s(get(get(&o, "body"), "expirationDate"))?,
                    "RECOVERY_NOT_ALLOWED",
                )?;
                self.recover_row(&o)?;
            } else {
                let e = self.evidence("place", Some(&o))?;
                preflight(&s, &e, (self.now)(), Some(&o))?;
            }
        }
        let l = self.basic()?;
        if network {
            self.append(&json!({
                "kind": "network_fence", "index": get(&o, "index"), "operation": op,
                "leaseIdentity": l, "leaseHash": payload_hash(&l)?, "at": iso((self.now)()),
            }))?;
            let s2 = self.state.borrow().clone().unwrap();
            let o2 = active(&s2).cloned().unwrap_or(V::Null);
            drop(s2);
            if op == "place" {
                let e = self.evidence("place", Some(&o2))?;
                let s3 = self.state.borrow().clone().unwrap();
                preflight(&s3, &e, (self.now)(), Some(&o2))?;
            } else if op == "cancel" {
                let e = self.evidence("cancel", Some(&o2))?;
                self.cancel_gate(&o2, &e, (self.now)())?;
            }
            self.basic()?;
        } else {
            self.append(&json!({"kind": "admitted", "index": get(&o, "index"), "operation": op, "at": iso((self.now)())}))?;
        }
        Ok(true)
    }
    fn cancel_gate(&self, o: &V, e: &V, at: i64) -> Result<(), String> {
        need(
            !get(o, "orderId").is_null() && at >= instant_s(get(get(o, "body"), "expirationDate"))?,
            "CANCEL_NOT_EXPIRED_OWN",
        )?;
        let r = raw(get(e, "order"), "GET", &format!("/orders/{}", get(o, "orderId").as_str().unwrap_or("")))?;
        let (d, _) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
        need(
            get(&r, "status") == &json!(200)
                && instant_s(get(&r, "receivedAt"))? <= at
                && at - instant_s(get(&r, "requestStartedAt"))? <= 20000
                && baal_execution::js_key(get(&d, "id")) == get(o, "orderId").as_str().unwrap_or("")
                && get(&d, "tournamentId") == get(&self.scope, "tournamentId")
                && get(&d, "exchangeId") == get(get(o, "body"), "exchangeId")
                && get(&d, "side") == &json!("no")
                && get(&d, "action") == &json!("buy")
                && get(&d, "quantity") == &json!(1)
                && get(&d, "open") == &json!(true)
                && price_s(&json!(baal_execution::js_key(get(&d, "priceLimit"))))? == price_s(get(&binding(self.state.borrow().as_ref().unwrap(), o, None)?, "limit"))?
                && get(&d, "expirationDate") == get(get(o, "body"), "expirationDate"),
            "CANCEL_ORDER_PROOF",
        )
    }
    fn recover_row(&self, o: &V) -> Result<V, String> {
        let row = self.rows.borrow().iter().find(|r| {
            let i = get(get(r, "event"), "input");
            get(i, "kind") == &json!("fetch_start")
                && get(i, "index") == get(o, "index")
                && get(get(i, "start"), "kind") == &json!("place")
                && instant_s(get(get(i, "start"), "requestStartedAt")).map(|x| x < instant_s(get(get(o, "body"), "expirationDate")).unwrap_or(i64::MIN)).unwrap_or(false)
        }).cloned();
        let row = row.ok_or("PAIRED_REST_ORIGINAL_WIRE_PROOF_MISSING")?;
        need(
            get(get(get(&row, "event"), "input"), "start").get("requestBodyHash") == Some(get(o, "bodyHash"))
                && get(get(get(&row, "event"), "input"), "start").get("requestBodyJson") == Some(get(o, "bodyJson")),
            "ORIGINAL_WIRE_PROOF_MISSING",
        )?;
        Ok(clone(&row))
    }
    pub fn close(&self) -> Result<(), String> {
        need(!*self.busy.borrow(), "BUSY")?;
        if *self.closed.borrow() { return Ok(()); }
        *self.closed.borrow_mut() = true;
        Ok(())
    }
}

pub fn execution_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../execution")
}

pub fn paired_rest_code_identity_file(execution_dir: &Path, entry_name: &str) -> serde_json::Map<String, V> {
    let mut seen: serde_json::Map<String, V> = serde_json::Map::new();
    fn visit(file: &Path, entry_dir: &Path, seen: &mut serde_json::Map<String, V>) {
        let key = file.strip_prefix(entry_dir).unwrap_or(file).to_string_lossy().replace('\\', "/");
        if seen.contains_key(&key) { return; }
        let raw = match std::fs::read(file) { Ok(b) => b, Err(_) => return };
        seen.insert(key, json!(sha(&raw)));
        let text = String::from_utf8_lossy(&raw);
        for caps in import_specifier_iter(&text) {
            let target = file.parent().unwrap_or(Path::new("")).join(&caps);
            let target = if target.extension().is_none() { target.with_extension("mjs") } else { target };
            visit(&target, entry_dir, seen);
        }
    }
    visit(&execution_dir.join(entry_name), execution_dir, &mut seen);
    seen
}
