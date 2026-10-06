// execution/paired-rest-controller.mjs — staged lifecycle only. The caller
// supplies its EXISTING serialized dispatcher, actual account lease, external
// STOP/activation gate and durable global reserve. No credential/client/
// limiter, activation file, polling loop or implicit retry.
use crate::core::{canonical_payload, payload_hash};
use crate::gates::normalize_official_fill_tick;
use crate::no_rest_reconciliation::verify_scoped_rest_no_buy;
use crate::no_rest_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::paired_dry_run::plan_paired_dry_run;
use crate::receipts::parse_number_lexemes;
use crate::transport::{Guard, OrderTransport, OrderTransportError, OrderTransportOptions};
use baal_service_runtime::contract::instant_ns;
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use baal_service_runtime::ledger::acquire_owner_lease;
use baal_service_runtime::market::normalize_book;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("PAIRED_REST_{c}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sha(v: &V) -> String {
    sha256(v.as_str().unwrap_or("").as_bytes())
}
fn sha_bytes(b: &[u8]) -> String {
    sha256(b)
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n > 0 && n <= 9_007_199_254_740_991)
    })
}
fn iso(n: f64) -> String {
    baal_service_runtime::contract::ms_to_iso(n)
}
fn keys(o: &V, allowed: &[&str]) -> Result<(), String> {
    need(
        o.is_object()
            && o.as_object().unwrap().keys().all(|k| allowed.contains(&k.as_str())),
        "UNSUPPORTED_FIELDS",
    )
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn price_v(v: &V) -> Result<i128, String> {
    price(v.as_str().unwrap_or(""))
}
fn js_string(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        _ => baal_service_runtime::json_text::js_stringify(v),
    }
}

// --- code identity (module .mjs import graph; see paired_dry_run.rs) --------

fn normalize_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}
fn relative_path(base: &Path, file: &Path) -> String {
    let base_c: Vec<_> = base.components().collect();
    let file_c: Vec<_> = file.components().collect();
    let mut i = 0;
    while i < base_c.len() && i < file_c.len() && base_c[i] == file_c[i] {
        i += 1;
    }
    let mut parts: Vec<String> = vec!["..".to_string(); base_c.len() - i];
    parts.extend(file_c[i..].iter().map(|c| c.as_os_str().to_string_lossy().to_string()));
    parts.join("/")
}
fn is_js_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

// identity() — sha256 over every file reachable through relative imports of
// execution/paired-rest-controller.mjs, keyed relative to execution/.
fn code_identity(workspace_dir: &Path) -> Result<V, String> {
    let dir = normalize_path(&workspace_dir.join("execution"));
    let entry = dir.join("paired-rest-controller.mjs");
    let mut files: Vec<(PathBuf, String)> = Vec::new();
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut stack = vec![entry];
    while let Some(file) = stack.pop() {
        let file = normalize_path(&file);
        if seen.contains(&file) {
            continue;
        }
        seen.insert(file.clone());
        let raw = fs::read(&file).map_err(|e| e.to_string())?;
        let source = String::from_utf8_lossy(&raw).to_string();
        files.push((file.clone(), sha_bytes(&raw)));
        let bytes = source.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let kw = if bytes[i..].starts_with(b"from") {
                Some(4)
            } else if bytes[i..].starts_with(b"import") {
                Some(6)
            } else {
                None
            };
            if let Some(n) = kw {
                let mut j = i + n;
                while j < bytes.len() && is_js_space(bytes[j]) {
                    j += 1;
                }
                if j < bytes.len() && (bytes[j] == b'\'' || bytes[j] == b'"') {
                    let quote = bytes[j];
                    let mut k = j + 1;
                    while k < bytes.len() && bytes[k] != quote && bytes[k] != b'\'' && bytes[k] != b'"' {
                        k += 1;
                    }
                    if k < bytes.len() && bytes[k] == quote {
                        let rel = &source[j + 1..k];
                        if rel.starts_with('.') && rel.len() > 1 {
                            stack.push(file.parent().unwrap_or(Path::new("/")).join(rel));
                        }
                        i = k + 1;
                        continue;
                    }
                }
            }
            i += 1;
        }
    }
    let mut pairs: Vec<(String, String)> = files
        .iter()
        .map(|(f, h)| (relative_path(&dir, f).replace('\\', "/"), h.clone()))
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = Map::new();
    for (k, h) in pairs {
        out.insert(k, json!(h));
    }
    Ok(V::Object(out))
}

// --- plan derivation -------------------------------------------------------

// derivePairedRestPlan(input) — a distinct dormant REST execution revision.
// The original capture/dry plan is retained by hash; its observed protective
// cost is not relabeled as the reserve. Sequential hedge flexibility uses only
// the declared ordinary payout margin.
pub fn derive_paired_rest_plan(input: &V) -> Result<V, String> {
    let original = plan_paired_dry_run(input)?;
    let mut body = original.as_object().cloned().unwrap_or_default();
    let original_dry_plan_hash = body.remove("planHash").unwrap_or(V::Null);
    let ceiling = amount(get(&original, "conditionalOrdinaryMinimumPayout").as_str().unwrap_or(""))
        .unwrap_or(0)
        - amount(get(&get(&original, "policy"), "minimumOrdinaryMargin").as_str().unwrap_or("")).unwrap_or(0);
    need(
        ceiling == 990_000 && ceiling % TICK == 0 && amount(get(&original, "pairCostCeiling").as_str().unwrap_or("")).unwrap_or(0) <= ceiling,
        "ORDINARY_MARGIN_CEILING",
    )?;
    let mut derived = body;
    derived.insert("schemaVersion".into(), json!(2));
    derived.insert("mode".into(), json!("paired_rest_no_fixed_margin_v2"));
    derived.insert("originalDryPlanHash".into(), original_dry_plan_hash);
    derived.insert("originalProtectiveCost".into(), get(&original, "pairCostCeiling").clone());
    derived.insert("originalConditionalOrdinarySurplus".into(), get(&original, "conditionalOrdinarySurplus").clone());
    derived.insert(
        "sourceMinimumOrdinaryMargin".into(),
        get(&get(&original, "policy"), "minimumOrdinaryMargin").clone(),
    );
    derived.insert("pairCostCeiling".into(), json!(money(ceiling)));
    derived.insert(
        "conditionalOrdinarySurplus".into(),
        json!(money(amount(get(&original, "conditionalOrdinaryMinimumPayout").as_str().unwrap_or("")).unwrap_or(0) - ceiling)),
    );
    derived.insert("exceptionLoss".into(), json!(money(-ceiling)));
    let plan_hash = payload_hash(&V::Object(derived.clone()))?;
    derived.insert("planHash".into(), json!(plan_hash));
    Ok(V::Object(derived))
}

// --- state helpers ---------------------------------------------------------

fn active(s: &V) -> Option<V> {
    get(s, "orders")
        .as_array()
        .and_then(|a| a.iter().find(|o| get(o, "phase").as_str() != Some("reconciled")).cloned())
}
fn cost(s: &V) -> i128 {
    get(s, "orders").as_array().cloned().unwrap_or_default().iter().fold(0, |n, o| {
        n + amount(get(&get(o, "proof"), "scopedPrincipalCost").as_str().unwrap_or("0.000000")).unwrap_or(0)
    })
}
fn ended(s: &V) -> bool {
    let orders = get(s, "orders").as_array().cloned().unwrap_or_default();
    (orders.len() == 2 && orders.iter().all(|o| get(o, "phase").as_str() == Some("reconciled")))
        || (orders.first().is_some_and(|o| {
            get(o, "phase").as_str() == Some("reconciled")
                && get(&get(o, "proof"), "filledQuantity") == &json!(0)
        }))
}
fn funding(s: &V) -> Result<V, String> {
    let used = cost(s);
    let mut body = Map::new();
    for (k, v) in [
        ("schemaVersion", json!(1)),
        ("ownerId", get(&get(&get(s, "plan"), "scope"), "ownerId").clone()),
        ("runId", get(&get(&get(s, "plan"), "scope"), "runId").clone()),
        ("tournamentId", get(&get(&get(s, "plan"), "scope"), "tournamentId").clone()),
        ("planHash", get(&get(s, "plan"), "planHash").clone()),
        ("allocatedCapital", json!("2.000000")),
        (
            "reservedCash",
            json!(if ended(s) { "0.000000".to_string() } else { money(amount(get(&get(s, "plan"), "pairCostCeiling").as_str().unwrap_or("")).unwrap_or(0) - used) }),
        ),
        ("ownedPrincipal", json!(money(used))),
        ("cumulativeBuy", json!(money(used))),
        ("maxCumulativeBuy", json!("2.000000")),
    ] {
        body.insert(k.to_string(), v);
    }
    let h = payload_hash(&V::Object(body.clone()))?;
    body.insert("reservationHash".into(), json!(h));
    Ok(V::Object(body))
}
fn binding(s: &V, o: &V, order_id: &V) -> Result<V, String> {
    Ok(json!({
        "ownerId": get(&get(&get(s, "plan"), "scope"), "ownerId"),
        "runId": get(&get(&get(s, "plan"), "scope"), "runId"),
        "intentId": get(o, "intentId"),
        "idempotencyKey": get(&get(o, "body"), "idempotencyKey"),
        "orderId": order_id,
        "marketId": get(o, "marketId"),
        "exchangeId": get(&get(o, "body"), "exchangeId"),
        "tournamentId": get(&get(o, "body"), "tournamentId"),
        "tournamentSlug": get(&get(&get(s, "plan"), "scope"), "tournamentSlug"),
        "side": "no",
        "action": "buy",
        "quantity": 1,
        "limit": money((get(&get(o, "body"), "price").as_f64().unwrap_or(f64::NAN) * 1e6).round() as i128),
        "createdAt": get(o, "createdAt"),
        "expirationDate": get(&get(o, "body"), "expirationDate"),
    }))
}
fn raw(r: &V, method: &str, endpoint: &str) -> Result<V, String> {
    keys(
        r,
        &["method", "endpoint", "status", "bodyText", "requestStartedAt", "receivedAt", "sourceBodyHash", "requestBodyHash", "fetchInvocationId"],
    )?;
    need(
        get(r, "method").as_str() == Some(method)
            && get(r, "endpoint").as_str() == Some(endpoint)
            && js_int(get(r, "status")).is_some()
            && get(r, "bodyText").as_str().is_some_and(|s| s.len() <= 8 * 1024 * 1024),
        "RAW_RECEIPT_REQUIRED",
    )?;
    instant_v(get(r, "requestStartedAt"))?;
    instant_v(get(r, "receivedAt"))?;
    need(
        instant_v(get(r, "receivedAt"))? >= instant_v(get(r, "requestStartedAt"))?,
        "RAW_RECEIPT_CLOCK",
    )?;
    let body_hash = sha(get(r, "bodyText"));
    if let Some(h) = r.get("sourceBodyHash") {
        need(h.as_str() == Some(body_hash.as_str()), "RAW_RECEIPT_HASH")?;
    }
    let mut out = r.clone();
    out.as_object_mut().unwrap().insert("sourceBodyHash".into(), json!(body_hash));
    Ok(out)
}
fn account_shape(a: &V) -> Result<(), String> {
    keys(a, &["cash", "positions", "openOrdersPages", "scopedOrdersPages"])?;
    let mut rows: Vec<V> = vec![get(a, "cash").clone(), get(a, "positions").clone()];
    rows.extend(get(a, "openOrdersPages").as_array().cloned().unwrap_or_default());
    rows.extend(get(a, "scopedOrdersPages").as_array().cloned().unwrap_or_default());
    for r in rows {
        raw(&r, "GET", get(&r, "endpoint").as_str().unwrap_or(""))?;
    }
    Ok(())
}
fn expected_positions(s: &V) -> Vec<V> {
    let mut out: Vec<V> = get(s, "externalPositions").as_array().cloned().unwrap_or_default();
    for o in get(s, "orders").as_array().cloned().unwrap_or_default() {
        if js_truthy(get(&get(&o, "proof"), "filledQuantity").into())
            && js_int(get(&get(&o, "proof"), "filledQuantity")).unwrap_or(0) != 0
        {
            out.push(json!({
                "exchangeId": get(&get(&o, "body"), "exchangeId"),
                "marketId": get(&o, "marketId"),
                "quantity": -1,
                "settled": false,
            }));
        }
    }
    out.retain(|p| js_int(get(p, "quantity")).unwrap_or(0) != 0);
    out.sort_by(|a, b| {
        js_int(get(a, "exchangeId")).unwrap_or(0).cmp(&js_int(get(b, "exchangeId")).unwrap_or(0))
    });
    out
}
fn global_gate(g: &V, s: &V, at: i64, own: Option<&V>) -> Result<(), String> {
    need(
        !g.is_null()
            && get(g, "tournamentId") == get(&get(&get(s, "plan"), "scope"), "tournamentId")
            && get(g, "ownerId") == get(&get(&get(s, "plan"), "scope"), "ownerId")
            && get(g, "runId") == get(&get(&get(s, "plan"), "scope"), "runId")
            && get(g, "complete") == &json!(true)
            && get(g, "soleCollector") == &json!(true)
            && get(g, "stopped") == &json!(false)
            && hex(get(g, "sourceHash"))
            && get(g, "unresolvedIntentIds").as_array().is_some_and(|a| a.len() <= 1)
            && get(g, "unresolvedIntentIds").as_array().unwrap().iter().all(|v| Some(v) == own)
            && instant_v(get(g, "at"))? <= at
            && at - instant_v(get(g, "at"))? <= 20_000,
        "GLOBAL_UNCERTAINTY",
    )
}

fn preflight(s: &V, e: &V, at: i64, o: Option<&V>) -> Result<V, String> {
    keys(e, &["tournamentId", "account", "book", "globalRisk", "funding", "reservedElsewhere"])?;
    account_shape(get(e, "account"))?;
    need(
        !ended(s) && instant_v(get(&get(s, "plan"), "reviewExpiresAt"))? > at,
        "PAIR_FINISHED_OR_RULES_EXPIRED",
    )?;
    let index = o.and_then(|v| js_int(get(v, "index"))).unwrap_or_else(|| {
        get(s, "orders").as_array().map(|a| a.len() as i64).unwrap_or(0)
    }) as usize;
    let leg = get(&get(s, "plan"), "legs").as_array().and_then(|a| a.get(index)).cloned().unwrap_or(V::Null);
    need(!leg.is_null(), "PAIR_LIMIT")?;
    if index == 0 {
        need(
            get(&get(s, "plan"), "legs").as_array().unwrap().iter().all(|l| {
                instant_v(get(&get(l, "book"), "requestStartedAt")).is_ok_and(|t| at - t <= 20_000)
            }),
            "INITIAL_PAIR_BOOKS_STALE",
        )?;
    }
    let cap = if index == 0 {
        price_v(get(&leg, "originalLimit"))?
    } else {
        amount(get(&get(s, "plan"), "pairCostCeiling").as_str().unwrap_or("")).unwrap_or(0) - cost(s)
    };
    need(
        cap >= TICK && cap <= SCALE - TICK && cap <= 1_000_000 && (index != 0 || cap <= 500_000),
        "COST_CAP",
    )?;
    if index == 1 {
        need(
            get(&get(s, "orders").as_array().unwrap()[0], "phase").as_str() == Some("reconciled")
                && get(&get(&get(s, "orders")[0], "proof"), "filledQuantity") == &json!(1),
            "FIRST_LEG_UNRESOLVED",
        )?;
    }
    global_gate(get(e, "globalRisk"), s, at, o.and_then(|v| v.get("intentId")))?;
    need(
        get(e, "tournamentId") == get(&get(&get(s, "plan"), "scope"), "tournamentId"),
        "BOOK_SCOPE",
    )?;
    let reserve = funding(s)?;
    let installed = get(e, "funding");
    need(
        !installed.is_null()
            && get(installed, "reservationHash") == get(&reserve, "reservationHash")
            && get(installed, "amount") == get(&reserve, "reservedCash")
            && hex(get(installed, "globalLedgerHash"))
            && instant_v(get(installed, "confirmedAt"))? <= at
            && at - instant_v(get(installed, "confirmedAt"))? <= 20_000,
        "GLOBAL_FUNDING_NOT_INSTALLED",
    )?;
    let mut b = get(&get(s, "plan"), "scope").as_object().cloned().unwrap_or_default();
    b.insert("exchangeId".into(), get(&leg, "exchangeId").clone());
    let a = inspect_scoped_rest_no_account(&json!({
        "account": get(e, "account"),
        "binding": V::Object(b),
        "readAt": iso(at as f64),
        "requiredFunding": get(&reserve, "reservedCash"),
        "reservedElsewhere": if e.get("reservedElsewhere").is_some() { get(e, "reservedElsewhere").clone() } else { json!("0.000000") },
    }))?;
    need(get(&a, "funded") == &json!(true), "FULL_PAIR_FUNDING")?;
    let mut positions: Vec<V> = get(&a, "positions")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| js_int(get(p, "quantity")).unwrap_or(0) != 0)
        .collect();
    positions.sort_by(|x, y| {
        js_int(get(x, "exchangeId")).unwrap_or(0).cmp(&js_int(get(y, "exchangeId")).unwrap_or(0))
    });
    if get(s, "externalPositions").is_null() {
        need(
            !positions.iter().any(|p| {
                get(&get(s, "plan"), "legs").as_array().unwrap().iter().any(|l| {
                    get(l, "exchangeId") == get(p, "exchangeId")
                })
            }),
            "EXISTING_PAIR_HOLDING",
        )?;
    } else {
        need(
            payload_hash(&V::Array(positions.clone()))? == payload_hash(&V::Array(expected_positions(s)))?,
            "ACCOUNT_POSITION_DRIFT",
        )?;
    }
    let endpoint = format!(
        "/markets/{}/orderbook?tournamentId={}&depth=20",
        get(&leg, "marketId").as_str().unwrap_or(""),
        get(&get(&get(s, "plan"), "scope"), "tournamentId").as_str().unwrap_or("")
    );
    let receipt = raw(get(e, "book"), "GET", &endpoint)?;
    need(js_int(get(&receipt, "status")) == Some(200), "BOOK_HTTP")?;
    let (data, _) = parse_number_lexemes(get(&receipt, "bodyText").as_str().unwrap_or(""))?;
    let book = normalize_book(
        &data,
        get(&leg, "marketId").as_str().unwrap_or(""),
        get(&get(&get(s, "plan"), "scope"), "tournamentId").as_str().unwrap_or(""),
        get(&receipt, "receivedAt").as_str().unwrap_or(""),
        get(&receipt, "requestStartedAt").as_str().unwrap_or(""),
        20_000.0,
        &BTreeSet::new(),
        "authoritative_receipt_v1",
    )?;
    need(
        get(&data, "contexts").as_array().is_some_and(|c| c.len() == 1)
            && get(&get(&data, "contexts")[0], "type") == &json!("tournament")
            && get(&book, "exchangeId") == get(&leg, "exchangeId")
            && get(&book, "bids").as_array().is_some_and(|x| x.len() <= 20)
            && get(&book, "asks").as_array().is_some_and(|x| x.len() <= 20)
            && instant_v(get(&a, "completedAt"))? <= instant_v(get(&book, "requestStartedAt"))?
            && at - instant_v(get(&book, "requestStartedAt"))? <= 20_000
            && instant_v(get(&book, "receivedAt"))? <= at,
        "BOOK_ACCOUNT_CAUSALITY",
    )?;
    let prior = o.and_then(|v| v.get("book")).cloned().unwrap_or_else(|| get(&leg, "book").clone());
    need(
        BigInt::parse_bytes(get(&book, "sequence").as_str().unwrap_or("").as_bytes(), 10)
            .unwrap_or_else(|| BigInt::from(0))
            >= BigInt::parse_bytes(get(&prior, "sequence").as_str().unwrap_or("").as_bytes(), 10)
                .unwrap_or_else(|| BigInt::from(0))
            && instant_ns(get(&book, "sourceAt").as_str().unwrap_or("")).unwrap_or(i128::MIN)
                >= instant_ns(get(&prior, "sourceAt").as_str().unwrap_or("")).unwrap_or(i128::MIN)
            && instant_v(get(&book, "requestStartedAt"))? >= instant_v(get(&prior, "requestStartedAt"))?,
        "BOOK_ROLLBACK",
    )?;
    if get(&book, "versionKey") == get(&prior, "versionKey") {
        need(
            payload_hash(&V::Array(vec![get(&book, "bids").clone(), get(&book, "asks").clone()]))?
                == payload_hash(&V::Array(vec![get(&prior, "bids").clone(), get(&prior, "asks").clone()]))?,
            "BOOK_VERSION_CONFLICT",
        )?;
    }
    need(
        get(&book, "bids").as_array().unwrap().iter().any(|l| {
            (js_int(get(l, "quantity")).unwrap_or(0) as f64 / 100.0).floor() >= 1.0
                && price_v(get(l, "price")).is_ok_and(|p| SCALE - p <= cap)
        }),
        "DEPTH_OR_PRICE",
    )?;
    if let Some(o) = o {
        need(
            at < instant_v(get(&get(o, "body"), "expirationDate"))?
                && price_v(get(&binding(s, o, get(o, "orderId"))?, "limit"))? == cap,
            "EXPIRED_OR_CAP_CHANGED",
        )?;
    }
    Ok(json!({
        "index": index,
        "leg": leg,
        "cap": cap,
        "book": book,
        "positions": positions,
        "account": a,
    }))
}

fn canonical_ack(o: &V, r: &V) -> bool {
    let parsed = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""));
    let d = match parsed {
        Ok((d, _)) => d,
        Err(_) => return false,
    };
    (js_int(get(&d, "orderId")).is_some_and(|x| x > 0))
        && get(&d, "exchangeId") == get(&get(o, "body"), "exchangeId")
        && get(&d, "side").as_str() == Some("no")
        && get(&d, "action").as_str() == Some("buy")
        && get(&d, "quantity") == &json!(1)
        && normalize_official_fill_tick(get(&d, "price"))
            .ok()
            .and_then(|v| get(&v, "price").as_str().map(|s| s.to_string()))
            .and_then(|p| amount(&p).ok())
            .map(|a| a == price(&js_string(get(&get(o, "body"), "price"))).unwrap_or(i128::MIN))
            .unwrap_or(false)
        && d.get("all") == Some(&V::Null)
        && get(&d, "open").is_boolean()
        && js_int(get(&d, "quantityTraded")).is_some()
        && js_int(get(&d, "remainingQuantity")).is_some()
        && js_int(get(&d, "quantityTraded")).unwrap_or(0) >= 0
        && js_int(get(&d, "remainingQuantity")).unwrap_or(0) >= 0
        && js_int(get(&d, "quantityTraded")).unwrap_or(0) + js_int(get(&d, "remainingQuantity")).unwrap_or(0) == 1
}

// reduce(s,e) — the journal event state machine.
fn reduce(s: Option<V>, e: &V) -> Result<V, String> {
    if get(e, "kind").as_str() == Some("initialize") {
        need(s.is_none(), "ALREADY_INITIALIZED")?;
        return Ok(json!({
            "schemaVersion": 2,
            "plan": derive_paired_rest_plan(&json!({
                "capture": get(e, "capture"),
                "scope": get(e, "scope"),
                "readAt": get(e, "at"),
            }))?,
            "orders": [],
            "externalPositions": V::Null,
            "installedFunding": V::Null,
        }));
    }
    let s = s.ok_or("PAIRED_REST_NOT_INITIALIZED")?;
    let mut n = s.clone();
    let kind = get(e, "kind").as_str().unwrap_or("");
    if kind == "funding" {
        need(
            get(&get(e, "receipt"), "reservationHash") == get(&funding(&s)?, "reservationHash")
                && get(&get(e, "receipt"), "amount") == get(&funding(&s)?, "reservedCash")
                && hex(get(&get(e, "receipt"), "globalLedgerHash"))
                && instant_v(get(&get(e, "receipt"), "confirmedAt"))? <= instant_v(get(e, "at"))?,
            "FUNDING_RECEIPT",
        )?;
        n.as_object_mut().unwrap().insert("installedFunding".into(), get(e, "receipt").clone());
        return Ok(n);
    }
    if kind == "prepared" {
        need(active(&s).is_none(), "ACTIVE_INTENT")?;
        let v = preflight(&s, get(e, "evidence"), instant_v(get(e, "at"))?, None)?;
        need(
            js_int(get(e, "index")) == Some(js_int(get(&v, "index")).unwrap_or(-1))
                && get(e, "intentId").is_string()
                && get(e, "idempotencyKey").is_string()
                && !get(&s, "orders").as_array().unwrap().iter().any(|x| {
                    get(x, "intentId") == get(e, "intentId")
                        || get(&get(x, "body"), "idempotencyKey") == get(e, "idempotencyKey")
                }),
            "INTENT_IDENTITY",
        )?;
        let body = json!({
            "idempotencyKey": get(e, "idempotencyKey"),
            "exchangeId": get(&get(&v, "leg"), "exchangeId"),
            "side": "no",
            "action": "buy",
            "quantity": 1,
            "price": js_int(get(&v, "cap")).unwrap_or(0) as f64 / 1e6,
            "expirationDate": iso((instant_v(get(e, "at"))? + 30_000).min(instant_v(get(&get(&s, "plan"), "reviewExpiresAt"))?) as f64),
            "tournamentId": get(&get(&get(&s, "plan"), "scope"), "tournamentId"),
        });
        if n.get("externalPositions").is_none_or(|v| v.is_null()) {
            n.as_object_mut().unwrap().insert("externalPositions".into(), V::Array(get(&v, "positions").as_array().cloned().unwrap_or_default()));
        }
        let orders = n.get("orders").cloned().unwrap_or(V::Array(vec![]));
        let mut arr = orders.as_array().cloned().unwrap_or_default();
        arr.push(json!({
            "index": get(&v, "index"),
            "intentId": get(e, "intentId"),
            "marketId": get(&get(&v, "leg"), "marketId"),
            "createdAt": get(e, "at"),
            "body": body,
            "bodyJson": canonical_payload(&body)?,
            "bodyHash": payload_hash(&body)?,
            "before": get(&get(e, "evidence"), "account"),
            "reservedElsewhere": if get(&get(e, "evidence"), "reservedElsewhere").is_null() { json!("0.000000") } else { get(&get(e, "evidence"), "reservedElsewhere").clone() },
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
        n.as_object_mut().unwrap().insert("orders".into(), V::Array(arr));
        return Ok(n);
    }
    let order_idx = js_int(get(e, "index")).unwrap_or(-1) as usize;
    let orders = get(&n, "orders").as_array().cloned().unwrap_or_default();
    let o = orders.get(order_idx).cloned().unwrap_or(V::Null);
    need(!o.is_null() && get(&o, "phase").as_str() != Some("reconciled"), "NO_ACTIVE_ORDER")?;
    if kind == "admitted" {
        let operation = get(e, "operation").as_str().unwrap_or("");
        if operation == "place" {
            need(get(&o, "phase").as_str() == Some("prepared"), "PLACEMENT_PHASE")?;
        } else if operation == "recover" {
            need(
                matches!(get(&o, "phase").as_str(), Some("uncertain" | "dispatching"))
                    && get(&o, "orderId").is_null()
                    && !js_truthy(o.get("semanticMismatch"))
                    && instant_v(get(e, "at"))? >= instant_v(get(&get(&o, "body"), "expirationDate"))?
                    && get(&o, "fetchStarts").as_array().is_some_and(|ws| {
                        ws.iter().any(|w| {
                            get(w, "kind").as_str() == Some("place")
                                && instant_v(get(w, "requestStartedAt")).is_ok_and(|t| {
                                    t < instant_v(get(&get(&o, "body"), "expirationDate")).unwrap_or(i64::MAX)
                                })
                        })
                    }),
                "RECOVERY_PHASE",
            )?;
        } else {
            need(
                operation == "cancel"
                    && !get(&o, "orderId").is_null()
                    && instant_v(get(e, "at"))? >= instant_v(get(&get(&o, "body"), "expirationDate"))?
                    && !js_truthy(o.get("cancelPending")),
                "CANCEL_PHASE",
            )?;
        }
        let mut o = o.clone();
        o.as_object_mut().unwrap().insert("operation".into(), json!(operation));
        if operation == "cancel" {
            o.as_object_mut().unwrap().insert("cancelPending".into(), json!(true));
        } else {
            o.as_object_mut().unwrap().insert("phase".into(), json!("dispatching"));
        }
        let mut arr = get(&n, "orders").as_array().cloned().unwrap_or_default();
        arr[order_idx] = o;
        n.as_object_mut().unwrap().insert("orders".into(), V::Array(arr));
        return Ok(n);
    }
    if kind == "network_fence" {
        need(
            get(&o, "operation") == get(e, "operation")
                && hex(get(e, "leaseHash"))
                && payload_hash(get(e, "leaseIdentity"))? == get(e, "leaseHash").as_str().unwrap_or(""),
            "NETWORK_FENCE",
        )?;
        let mut o = o.clone();
        let mut fences = get(&o, "fences").as_array().cloned().unwrap_or_default();
        fences.push(json!({
            "operation": get(e, "operation"),
            "at": get(e, "at"),
            "leaseIdentity": get(e, "leaseIdentity"),
            "leaseHash": get(e, "leaseHash"),
        }));
        o.as_object_mut().unwrap().insert("fences".into(), V::Array(fences));
        let mut arr = get(&n, "orders").as_array().cloned().unwrap_or_default();
        arr[order_idx] = o;
        n.as_object_mut().unwrap().insert("orders".into(), V::Array(arr));
        return Ok(n);
    }
    if kind == "fetch_start" {
        let w = get(e, "start");
        let l = get(&o, "fences").as_array().and_then(|f| f.last().cloned()).unwrap_or(V::Null);
        need(
            !l.is_null()
                && get(w, "kind") == get(&o, "operation")
                && get(w, "ownerId") == get(&get(&get(&s, "plan"), "scope"), "ownerId")
                && get(w, "runId") == get(&get(&get(&s, "plan"), "scope"), "runId")
                && get(w, "intentId") == get(&o, "intentId")
                && get(w, "idempotencyKey") == get(&get(&o, "body"), "idempotencyKey")
                && get(w, "requestBodyJson") == get(&o, "bodyJson")
                && get(w, "requestBodyHash") == get(&o, "bodyHash")
                && get(w, "leaseHash") == get(&l, "leaseHash")
                && payload_hash(get(w, "leaseIdentity"))? == get(&l, "leaseHash").as_str().unwrap_or("")
                && get(w, "fetchInvocationId").as_str().is_some_and(|x| !x.is_empty())
                && !get(&n, "orders").as_array().unwrap().iter().any(|v| {
                    get(v, "fetchStarts").as_array().is_some_and(|fs| {
                        fs.iter().any(|x| get(x, "fetchInvocationId") == get(w, "fetchInvocationId"))
                    })
                })
                && instant_v(get(w, "requestStartedAt"))? >= instant_v(get(&l, "at"))?
                && instant_v(get(w, "requestStartedAt"))? <= instant_v(get(e, "at"))?
                && get(w, "method").as_str()
                    == Some(if get(w, "kind").as_str() == Some("cancel") { "DELETE" } else { "POST" })
                && get(w, "endpoint").as_str()
                    == Some(if get(w, "kind").as_str() == Some("cancel") {
                        Box::leak(format!("/orders/{}", get(&o, "orderId").as_str().unwrap_or("")).into_boxed_str())
                    } else {
                        "/orders"
                    }),
            "FETCH_START_BINDING",
        )?;
        if get(w, "kind").as_str() == Some("place") {
            need(
                instant_v(get(w, "requestStartedAt"))? < instant_v(get(&get(&o, "body"), "expirationDate"))?,
                "ORIGINAL_WIRE_EXPIRED",
            )?;
        }
        let mut o = o.clone();
        let mut fs = get(&o, "fetchStarts").as_array().cloned().unwrap_or_default();
        fs.push(w.clone());
        o.as_object_mut().unwrap().insert("fetchStarts".into(), V::Array(fs));
        let mut arr = get(&n, "orders").as_array().cloned().unwrap_or_default();
        arr[order_idx] = o;
        n.as_object_mut().unwrap().insert("orders".into(), V::Array(arr));
        return Ok(n);
    }
    if kind == "wire" {
        let w = get(e, "wire");
        let started = get(&o, "fetchStarts").as_array().and_then(|f| f.last().cloned()).unwrap_or(V::Null);
        let last_fence = get(&o, "fences").as_array().and_then(|f| f.last().cloned()).unwrap_or(V::Null);
        need(
            !started.is_null()
                && get(w, "fetchInvocationId") == get(&started, "fetchInvocationId")
                && get(w, "requestStartedAt") == get(&started, "requestStartedAt")
                && get(w, "kind") == get(&o, "operation")
                && get(w, "ownerId") == get(&get(&get(&s, "plan"), "scope"), "ownerId")
                && get(w, "runId") == get(&get(&get(&s, "plan"), "scope"), "runId")
                && get(w, "intentId") == get(&o, "intentId")
                && get(w, "idempotencyKey") == get(&get(&o, "body"), "idempotencyKey")
                && get(w, "requestBodyJson") == get(&o, "bodyJson")
                && get(w, "requestBodyHash") == get(&o, "bodyHash")
                && get(w, "leaseHash") == get(&last_fence, "leaseHash")
                && instant_v(get(w, "requestStartedAt"))? >= instant_v(get(&last_fence, "at"))?
                && instant_v(get(w, "receivedAt"))? >= instant_v(get(w, "requestStartedAt"))?
                && instant_v(get(w, "receivedAt"))? <= instant_v(get(e, "at"))?
                && hex(get(w, "sourceBodyHash"))
                && sha(get(w, "bodyText")) == get(w, "sourceBodyHash").as_str().unwrap_or(""),
            "WIRE_PROVENANCE",
        )?;
        if get(w, "kind").as_str() == Some("place") {
            need(
                instant_v(get(w, "requestStartedAt"))? < instant_v(get(&get(&o, "body"), "expirationDate"))?,
                "ORIGINAL_WIRE_EXPIRED",
            )?;
        }
        let mut o = o.clone();
        let mut wa = get(&o, "wireAttempts").as_array().cloned().unwrap_or_default();
        wa.push(w.clone());
        o.as_object_mut().unwrap().insert("wireAttempts".into(), V::Array(wa));
        if get(w, "kind").as_str() == Some("cancel") {
            o.as_object_mut().unwrap().insert("cancelPending".into(), json!(true));
            let mut arr = get(&n, "orders").as_array().cloned().unwrap_or_default();
            arr[order_idx] = o;
            n.as_object_mut().unwrap().insert("orders".into(), V::Array(arr));
            return Ok(n);
        }
        let status = js_int(get(w, "status")).unwrap_or(0);
        if (200..300).contains(&status) {
            let d = parse_number_lexemes(get(w, "bodyText").as_str().unwrap_or("")).map(|x| x.0).unwrap_or(V::Null);
            if js_int(get(&d, "orderId")).is_some_and(|x| x > 0) {
                let new_id = js_int(get(&d, "orderId")).unwrap().to_string();
                need(
                    get(&o, "orderId").is_null() || get(&o, "orderId").as_str() == Some(new_id.as_str()),
                    "ORDER_ID_CHANGED",
                )?;
                o.as_object_mut().unwrap().insert("orderId".into(), json!(new_id));
            }
            let ack = json!({
                "method": "POST",
                "endpoint": "/orders",
                "status": get(w, "status"),
                "requestStartedAt": get(w, "requestStartedAt"),
                "receivedAt": get(w, "receivedAt"),
                "bodyText": get(w, "bodyText"),
                "sourceBodyHash": get(w, "sourceBodyHash"),
                "requestBodyHash": get(&o, "bodyHash"),
            });
            o.as_object_mut().unwrap().insert("ack".into(), ack);
            let mismatch = !canonical_ack(&o, get(&o, "ack"));
            o.as_object_mut().unwrap().insert("semanticMismatch".into(), json!(mismatch));
            o.as_object_mut().unwrap().insert(
                "phase".into(),
                json!(if mismatch { "semantic_mismatch" } else { "acknowledged" }),
            );
        } else {
            o.as_object_mut().unwrap().insert("phase".into(), json!("uncertain"));
        }
        let mut arr = get(&n, "orders").as_array().cloned().unwrap_or_default();
        arr[order_idx] = o;
        n.as_object_mut().unwrap().insert("orders".into(), V::Array(arr));
        return Ok(n);
    }
    if kind == "error" {
        let mut o = o.clone();
        if get(&o, "phase").as_str() == Some("dispatching") {
            o.as_object_mut().unwrap().insert("phase".into(), json!("uncertain"));
        }
        o.as_object_mut().unwrap().insert("lastError".into(), get(e, "code").clone());
        let mut arr = get(&n, "orders").as_array().cloned().unwrap_or_default();
        arr[order_idx] = o;
        n.as_object_mut().unwrap().insert("orders".into(), V::Array(arr));
        return Ok(n);
    }
    if kind == "reconciled" {
        need(
            !get(&o, "orderId").is_null() && !get(&o, "ack").is_null() && !js_truthy(o.get("semanticMismatch")),
            "RECONCILIATION_BINDING",
        )?;
        let evidence = get(e, "evidence");
        keys(evidence, &["order", "fillPages", "after", "globalRisk", "accountActivity"])?;
        account_shape(get(evidence, "after"))?;
        raw(
            get(evidence, "order"),
            "GET",
            &format!("/orders/{}", get(&o, "orderId").as_str().unwrap_or("")),
        )?;
        for r in get(evidence, "fillPages").as_array().cloned().unwrap_or_default() {
            raw(&r, "GET", get(&r, "endpoint").as_str().unwrap_or(""))?;
        }
        let mut verify_input = evidence.clone();
        verify_input.as_object_mut().unwrap().insert("binding".into(), binding(&s, &o, get(&o, "orderId"))?);
        verify_input.as_object_mut().unwrap().insert("before".into(), get(&o, "before").clone());
        verify_input.as_object_mut().unwrap().insert("ack".into(), get(&o, "ack").clone());
        verify_input.as_object_mut().unwrap().insert("recoveryProof".into(), get(e, "recoveryProof").clone());
        verify_input.as_object_mut().unwrap().insert("readAt".into(), get(e, "at").clone());
        verify_input.as_object_mut().unwrap().insert("fullPairReservation".into(), get(&funding(&s)?, "reservedCash").clone());
        verify_input.as_object_mut().unwrap().insert("reservedElsewhere".into(), get(&o, "reservedElsewhere").clone());
        let proof = verify_scoped_rest_no_buy(&verify_input)?;
        need(
            get(&proof, "scopedPrincipalCost") == &json!(money(amount(get(&proof, "scopedPrincipalCost").as_str().unwrap_or("")).unwrap_or(0)))
                && cost(&s) + amount(get(&proof, "scopedPrincipalCost").as_str().unwrap_or("")).unwrap_or(0)
                    <= amount(get(&get(&s, "plan"), "pairCostCeiling").as_str().unwrap_or("")).unwrap_or(0)
                && cost(&s) + amount(get(&proof, "scopedPrincipalCost").as_str().unwrap_or("")).unwrap_or(0) <= 2_000_000,
            "PAIR_PRINCIPAL",
        )?;
        let mut o = o.clone();
        o.as_object_mut().unwrap().insert("proof".into(), proof);
        o.as_object_mut().unwrap().insert("phase".into(), json!("reconciled"));
        o.as_object_mut().unwrap().insert("cancelPending".into(), json!(false));
        let mut arr = get(&n, "orders").as_array().cloned().unwrap_or_default();
        arr[order_idx] = o;
        n.as_object_mut().unwrap().insert("orders".into(), V::Array(arr));
        return Ok(n);
    }
    Err("PAIRED_REST_EVENT".to_string())
}

// --- controller ------------------------------------------------------------

// The caller's dispatch may surface a captured wire receipt on failure
// (JS error.wireReceipt). It is replayed into the journal before the error
// propagates.
pub struct WireDispatchError {
    pub message: String,
    pub wire_receipt: Option<V>,
}
pub type WireDispatch = dyn for<'g> Fn(
    &V,
    Option<&'g Guard<'g>>,
    Option<&'g Guard<'g>>,
) -> Result<V, WireDispatchError>;

pub struct PairedRestControllerDeps {
    pub dispatch: Rc<WireDispatch>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub assert_account_lease: Rc<dyn Fn() -> V>,
    pub install_funding_reservation: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub get_evidence: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub raw_receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub now: Rc<dyn Fn() -> f64>,
    pub workspace_dir: PathBuf,
}

struct InFlight {
    index: i64,
    spec: V,
    bd_ptr: usize,
    bn_ptr: usize,
}

struct CtrlInner {
    scope: V,
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
    rows: Vec<V>,
    in_flight: Option<InFlight>,
    issued: HashMap<usize, V>,
    lease: baal_service_runtime::ledger::OwnerLease,
    deps: PairedRestControllerDeps,
}

pub struct PairedRestController {
    inner: Rc<RefCell<CtrlInner>>,
    transport: OrderTransport,
}

// openPairedRestController({dir,capture,scope,dispatch,canOperate,assertAccountLease,installFundingReservation,getEvidence,rawReceiptOf,now})
pub fn open_paired_rest_controller(
    dir: &Path,
    capture: &V,
    scope: &V,
    deps: PairedRestControllerDeps,
) -> Result<PairedRestController, String> {
    need(dir.is_absolute(), "DEPENDENCIES")?;
    let scope = scope.clone();
    let lease = acquire_owner_lease(
        dir,
        get(&scope, "ownerId").as_str().unwrap_or(""),
        get(&scope, "runId").as_str().unwrap_or(""),
        "paired-rest-lifecycle",
        (deps.now)(),
    )
    .map_err(|e| e.message)?;
    let file = dir.join("paired-rest-journal.jsonl");
    let code = code_identity(&deps.workspace_dir)?;
    let open_result: Result<(Journal, Option<V>, u64, String, u64, f64, Vec<V>), String> = (|| {
        if !file.exists() {
            fs::OpenOptions::new().write(true).create_new(true).open(&file).map_err(|e| e.to_string())?;
        }
        let bytes = fs::read(&file).map_err(|e| e.to_string())?;
        need(bytes.is_empty() || *bytes.last().unwrap() == 10, "PARTIAL_JOURNAL")?;
        let journal = Journal::open(&file)?;
        let mut state: Option<V> = None;
        let mut rows: Vec<V> = Vec::new();
        let recovery_member = |rows: &[V], input: &V| -> Result<(), String> {
            if let Some(rp) = input.get("recoveryProof") {
                if !rp.is_null() {
                    let r = rp.get("journalRecord").cloned().unwrap_or(V::Null);
                    need(
                        !r.is_null()
                            && rows.iter().any(|v| {
                                get(v, "sequence") == get(&r, "sequence")
                                    && get(v, "hash") == get(&r, "hash")
                                    && payload_hash(v).unwrap_or_default() == payload_hash(&r).unwrap_or_default()
                            }),
                        "RECOVERY_JOURNAL_MEMBERSHIP",
                    )?;
                }
            }
            Ok(())
        };
        for line in String::from_utf8_lossy(&bytes).split('\n').filter(|l| !l.is_empty()) {
            let row: V = serde_json::from_str(line).map_err(|_| "PAIRED_REST_REPLAY".to_string())?;
            let row_code = get(&row, "event").get("codeIdentity").cloned().unwrap_or(V::Null);
            need(
                payload_hash(&row_code).unwrap_or_default() == payload_hash(&code).unwrap_or_default(),
                "CODE_CHANGED",
            )?;
            let row_input = get(&row, "event").get("input").cloned().unwrap_or(V::Null);
            recovery_member(&rows, &row_input)?;
            state = Some(reduce(state, &row_input)?);
            need(
                payload_hash(state.as_ref().unwrap())? == payload_hash(get(&row, "state"))?,
                "REPLAY_STATE",
            )?;
            rows.push(row);
        }
        let size = bytes.len() as u64;
        let bytes_hash = sha_bytes(&bytes);
        let stat = fs::metadata(&file).map_err(|e| e.to_string())?;
        use std::os::unix::fs::MetadataExt;
        let ino = stat.ino();
        let birthtime = stat
            .created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        Ok((journal, state, size, bytes_hash, ino, birthtime, rows))
    })();
    let (journal, state, size, bytes_hash, ino, birthtime, rows) = match open_result {
        Ok(v) => v,
        Err(e) => {
            let mut lease = lease;
            let _ = lease.release();
            return Err(e);
        }
    };
    let inner = Rc::new(RefCell::new(CtrlInner {
        scope: scope.clone(),
        file: file.clone(),
        code: code.clone(),
        journal,
        state,
        closed: false,
        failed: false,
        busy: false,
        size,
        bytes_hash,
        ino,
        birthtime_ms: birthtime,
        rows,
        in_flight: None,
        issued: HashMap::new(),
        lease,
        deps,
    }));
    // Initialize on first run; otherwise verify scope/capture unchanged.
    {
        let inner2 = inner.clone();
        let has_state = inner2.borrow().state.is_some();
        if !has_state {
            append_event(&inner2, &json!({"kind": "initialize", "capture": capture, "scope": inner2.borrow().scope, "at": iso((inner2.borrow().deps.now)())}))?;
        } else {
            let g = inner2.borrow();
            let st = g.state.clone().unwrap();
            need(
                payload_hash(get(&get(&st, "plan"), "scope"))? == payload_hash(&g.scope)?
                    && (capture.is_null()
                        || get(&get(&st, "plan"), "captureHash") == &json!(payload_hash(capture)?)),
                "SCOPE_OR_CAPTURE_CHANGED",
            )?;
        }
    }
    let transport = {
        let scope_v = json!({
            "tournamentId": get(&scope, "tournamentId"),
            "tournamentSlug": get(&scope, "tournamentSlug"),
            "exchanges": get(&scope, "markets"),
        });
        let inner_w = inner.clone();
        let wrap: Box<crate::transport::Dispatch> = Box::new(move |spec, bd, bn| {
            wrap_dispatch(&inner_w, spec, bd, bn).map_err(|e| OrderTransportError {
                code: e,
                outcome: "unknown".into(),
                may_have_executed: true,
                retry_same_request_only: false,
                retry_after_ms: 0.0,
                status: None,
                api_code: None,
                semantic_mismatch: false,
            })
        });
        let inner_c = inner.clone();
        OrderTransport::new(OrderTransportOptions {
            dispatch: wrap,
            scope: crate::transport::scope_of(&scope_v).map_err(|e| e.code)?,
            can_write: Box::new({
                let inner = inner.clone();
                move || inner.borrow().basic().is_ok()
            }),
            owns_order: Box::new({
                let inner = inner.clone();
                move |oid: &str| {
                    inner
                        .borrow()
                        .state
                        .as_ref()
                        .map(|s| {
                            get(s, "orders").as_array().is_some_and(|orders| {
                                orders.iter().any(|o| get(o, "orderId").as_str() == Some(oid))
                            })
                        })
                        .unwrap_or(false)
                }
            }),
            validate_permit: Box::new({
                let inner = inner.clone();
                move |p: &V, spec: &V| {
                    validate(&inner, p, spec, false).map_err(|e| OrderTransportError {
                        code: e,
                        outcome: "unknown".into(),
                        may_have_executed: true,
                        retry_same_request_only: false,
                        retry_after_ms: 0.0,
                        status: None,
                        api_code: None,
                        semantic_mismatch: false,
                    })
                }
            }),
            validate_network_permit: Box::new({
                let inner = inner.clone();
                move |p: &V, spec: &V| {
                    validate(&inner, p, spec, true).map_err(|e| OrderTransportError {
                        code: e,
                        outcome: "unknown".into(),
                        may_have_executed: true,
                        retry_same_request_only: false,
                        retry_after_ms: 0.0,
                        status: None,
                        api_code: None,
                        semantic_mismatch: false,
                    })
                }
            }),
            validate_lease: Box::new({
                let inner = inner.clone();
                move |l: &V, _s: &V| {
                    Ok(payload_hash(l).unwrap_or_default()
                        == inner.borrow().basic().and_then(|l| payload_hash(&l)).unwrap_or_default())
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
    Ok(PairedRestController { inner, transport })
}

impl CtrlInner {
    fn basic(&self) -> Result<V, String> {
        need(!self.closed && !self.failed && (self.deps.can_operate)(), "STOPPED_OR_FAILED")?;
        self.lease.assert_ownership().map_err(|e| e.message)?;
        let l = (self.deps.assert_account_lease)();
        need(
            get(&l, "ownerId") == get(&self.scope, "ownerId")
                && get(&l, "tournamentId") == get(&self.scope, "tournamentId")
                && (l.get("runId").is_none() || get(&l, "runId") == get(&self.scope, "runId"))
                && get(&l, "leaseId").as_str().is_some_and(|s| !s.is_empty()),
            "ACCOUNT_LEASE",
        )?;
        keys(&l, &["ownerId", "runId", "tournamentId", "leaseId"])?;
        Ok(l)
    }
}

// append(input)
fn append_event(inner: &Rc<RefCell<CtrlInner>>, input: &V) -> Result<(), String> {
    {
        let g = inner.borrow();
        g.basic()?;
    }
    let (before, size, bytes_hash, ino, birthtime, code) = {
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
        (before, g.size, g.bytes_hash.clone(), g.ino, g.birthtime_ms, g.code.clone())
    };
    {
        let g = inner.borrow();
        need(
            g.ino == ino && g.birthtime_ms == birthtime && before.len() as u64 == size && sha_bytes(&before) == bytes_hash
                && payload_hash(&code_identity(&g.deps.workspace_dir).unwrap_or(V::Null)).unwrap_or_default()
                    == payload_hash(&code).unwrap_or_default(),
            "JOURNAL_CHANGED",
        )?;
    }
    // recoveryMember(input)
    {
        let g = inner.borrow();
        if let Some(rp) = input.get("recoveryProof") {
            if !rp.is_null() {
                let r = rp.get("journalRecord").cloned().unwrap_or(V::Null);
                need(
                    !r.is_null()
                        && g.rows.iter().any(|v| {
                            get(v, "sequence") == get(&r, "sequence")
                                && get(v, "hash") == get(&r, "hash")
                                && payload_hash(v).unwrap_or_default() == payload_hash(&r).unwrap_or_default()
                        }),
                    "RECOVERY_JOURNAL_MEMBERSHIP",
                )?;
            }
        }
    }
    let next = {
        let g = inner.borrow();
        reduce(g.state.clone(), input)?
    };
    need(
        (size + js_str_len(&next) + js_str_len(input)) < 64 * 1024 * 1024,
        "JOURNAL_BOUND",
    )?;
    let event = {
        let g = inner.borrow();
        json!({"codeIdentity": g.code, "input": input})
    };
    let commit_result = inner.borrow_mut().journal.commit(&next, &event);
    match commit_result {
        Ok(_) => {
            {
                let g = inner.borrow();
                g.basic()?;
            }
            let all = fs::read(inner.borrow().file.clone()).map_err(|e| e.to_string())?;
            need(
                all[..size as usize] == before[..] && all.len() as u64 > size,
                "JOURNAL_CHANGED",
            )?;
            let mut g = inner.borrow_mut();
            let tail: V = serde_json::from_str(&String::from_utf8_lossy(&all[size as usize..]))
                .map_err(|_| "JOURNAL_CHANGED".to_string())?;
            g.rows.push(tail);
            g.state = Some(next);
            g.size = all.len() as u64;
            g.bytes_hash = sha_bytes(&all);
            Ok(())
        }
        Err(e) => {
            inner.borrow_mut().failed = true;
            Err(e)
        }
    }
}

fn js_str_len(v: &V) -> u64 {
    baal_service_runtime::json_text::js_stringify(v).len() as u64
}

// evidence(operation,o)
fn evidence(inner: &Rc<RefCell<CtrlInner>>, operation: &str, o: Option<&V>) -> Result<V, String> {
    let (state, o_owned) = {
        let g = inner.borrow();
        (g.state.clone().unwrap_or(V::Null), o.cloned())
    };
    let default_leg = get(&get(&state, "plan"), "legs")
        .as_array()
        .and_then(|a| a.get(get(&state, "orders").as_array().map(|x| x.len()).unwrap_or(0)))
        .cloned()
        .unwrap_or(V::Null);
    let input = json!({
        "operation": operation,
        "marketId": if o_owned.as_ref().is_some_and(|o| o.get("marketId").is_some()) { get(o_owned.as_ref().unwrap(), "marketId").clone() } else { get(&default_leg, "marketId").clone() },
        "exchangeId": if o_owned.as_ref().is_some_and(|o| get(&get(o, "body"), "exchangeId") != &V::Null) { get(&get(o_owned.as_ref().unwrap(), "body"), "exchangeId").clone() } else { get(&default_leg, "exchangeId").clone() },
        "orderId": o_owned.as_ref().map(|o| get(o, "orderId").clone()).unwrap_or(V::Null),
        "reservation": funding(&state)?,
    });
    let e = (inner.borrow().deps.get_evidence)(&input)?;
    need(!e.is_null(), "EVIDENCE_MUST_BE_LOCAL")?;
    inner.borrow().basic()?;
    Ok(e)
}

fn recover_row(inner: &Rc<RefCell<CtrlInner>>, o: &V) -> Result<V, String> {
    let g = inner.borrow();
    let mut row: Option<V> = None;
    for r in &g.rows {
        let input = get(&get(r, "event"), "input");
        if get(input, "kind").as_str() == Some("fetch_start")
            && js_int(get(input, "index")) == js_int(get(o, "index"))
            && get(&get(input, "start"), "kind").as_str() == Some("place")
            && instant_v(get(&get(input, "start"), "requestStartedAt"))?
                < instant_v(get(&get(o, "body"), "expirationDate"))?
        {
            row = Some(r.clone());
            break;
        }
    }
    need(
        row.is_some()
            && get(&get(&get(row.as_ref().unwrap(), "event"), "input"), "start")
                .get("requestBodyHash")
                == Some(&get(o, "bodyHash").clone())
            && get(&get(&get(row.as_ref().unwrap(), "event"), "input"), "start")
                .get("requestBodyJson")
                == Some(&get(o, "bodyJson").clone()),
        "ORIGINAL_WIRE_PROOF_MISSING",
    )?;
    Ok(row.unwrap())
}

fn cancel_gate(inner: &Rc<RefCell<CtrlInner>>, o: &V, e: &V, at: i64) -> Result<(), String> {
    let scope = inner.borrow().scope.clone();
    need(
        !get(o, "orderId").is_null() && at >= instant_v(get(&get(o, "body"), "expirationDate"))?,
        "CANCEL_NOT_EXPIRED_OWN",
    )?;
    let r = raw(get(e, "order"), "GET", &format!("/orders/{}", get(o, "orderId").as_str().unwrap_or("")))?;
    let (d, _) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
    need(
        js_int(get(&r, "status")) == Some(200)
            && instant_v(get(&r, "receivedAt"))? <= at
            && at - instant_v(get(&r, "requestStartedAt"))? <= 20_000
            && js_int(get(&d, "id")).map(|x| x.to_string()) == get(o, "orderId").as_str().map(|s| s.to_string())
            && get(&d, "tournamentId") == get(&scope, "tournamentId")
            && get(&d, "exchangeId") == get(&get(o, "body"), "exchangeId")
            && get(&d, "side").as_str() == Some("no")
            && get(&d, "action").as_str() == Some("buy")
            && get(&d, "quantity") == &json!(1)
            && get(&d, "open") == &json!(true)
            && price(&js_string(get(&d, "priceLimit")))? == price_v(get(&binding(&inner.borrow().state.clone().unwrap_or(V::Null), o, get(o, "orderId"))?, "limit"))?,
        "CANCEL_ORDER_PROOF",
    )?;
    Ok(())
}

// validate(p,spec,network)
fn validate(inner: &Rc<RefCell<CtrlInner>>, p: &V, spec: &V, network: bool) -> Result<bool, String> {
    inner.borrow().basic()?;
    let (o, scope_run_id) = {
        let g = inner.borrow();
        let st = g.state.clone().unwrap_or(V::Null);
        (active(&st), get(&g.scope, "runId").clone())
    };
    let o = o.ok_or("PAIRED_REST_PERMIT_IDENTITY")?;
    need(
        get(p, "intentId") == get(&o, "intentId")
            && get(p, "runId") == &scope_run_id
            && get(p, "bodyHash") == get(&o, "bodyHash"),
        "PERMIT_IDENTITY",
    )?;
    let operation = get(p, "operation").as_str().unwrap_or("");
    if operation == "cancel" {
        need(
            get(spec, "method").as_str() == Some("DELETE")
                && get(spec, "endpoint").as_str() == Some(Box::leak(format!("/orders/{}", get(&o, "orderId").as_str().unwrap_or("")).into_boxed_str())),
            "CANCEL_SCOPE",
        )?;
        let e = evidence(inner, "cancel", Some(&o))?;
        cancel_gate(inner, &o, &e, (inner.borrow().deps.now)() as i64)?;
    } else {
        need(
            get(spec, "method").as_str() == Some("POST")
                && get(spec, "endpoint").as_str() == Some("/orders")
                && get(spec, "bodyJson") == get(&o, "bodyJson"),
            "PLACEMENT_PAYLOAD",
        )?;
        if operation == "recover" {
            need(
                get(&o, "orderId").is_null()
                    && !js_truthy(o.get("semanticMismatch"))
                    && (inner.borrow().deps.now)() as i64 >= instant_v(get(&get(&o, "body"), "expirationDate"))?,
                "RECOVERY_NOT_ALLOWED",
            )?;
            recover_row(inner, &o)?;
        } else {
            let e = evidence(inner, "place", Some(&o))?;
            preflight(&inner.borrow().state.clone().unwrap_or(V::Null), &e, (inner.borrow().deps.now)() as i64, Some(&o))?;
        }
    }
    let l = inner.borrow().basic()?;
    if network {
        append_event(
            inner,
            &json!({
                "kind": "network_fence",
                "index": get(&o, "index"),
                "operation": get(p, "operation"),
                "leaseIdentity": l,
                "leaseHash": payload_hash(&l)?,
                "at": iso((inner.borrow().deps.now)()),
            }),
        )?;
        let cur = {
            let g = inner.borrow();
            active(&g.state.clone().unwrap_or(V::Null))
        };
        if get(p, "operation").as_str() == Some("place") {
            let e = evidence(inner, "place", cur.as_ref())?;
            preflight(&inner.borrow().state.clone().unwrap_or(V::Null), &e, (inner.borrow().deps.now)() as i64, cur.as_ref())?;
        } else if get(p, "operation").as_str() == Some("cancel") {
            let e = evidence(inner, "cancel", cur.as_ref())?;
            cancel_gate(inner, cur.as_ref().unwrap(), &e, (inner.borrow().deps.now)() as i64)?;
        }
        inner.borrow().basic()?;
    } else {
        append_event(
            inner,
            &json!({
                "kind": "admitted",
                "index": get(&o, "index"),
                "operation": get(p, "operation"),
                "at": iso((inner.borrow().deps.now)()),
            }),
        )?;
    }
    Ok(true)
}

// wrap(spec) — the transport dispatch wrapper that captures wire receipts.
fn wrap_dispatch(
    inner: &Rc<RefCell<CtrlInner>>,
    spec: &V,
    bd: Option<&Guard<'_>>,
    bn: Option<&Guard<'_>>,
) -> Result<V, String> {
    let (o, index) = {
        let g = inner.borrow();
        let st = g.state.clone().unwrap_or(V::Null);
        let o = active(&st).ok_or("PAIRED_REST_NO_ACTIVE_ORDER")?;
        let index = js_int(get(&o, "index")).unwrap_or(-1);
        (o, index)
    };
    let bd_ptr = bd.map(|g| g as *const Guard as *const () as usize).unwrap_or(0);
    let bn_ptr = bn.map(|g| g as *const Guard as *const () as usize).unwrap_or(0);
    {
        let mut g = inner.borrow_mut();
        g.issued.insert(
            bd_ptr,
            json!({
                "endpoint": get(spec, "endpoint"),
                "method": get(spec, "method"),
                "bodyJson": get(spec, "bodyJson"),
                "beforeNetworkPtr": bn_ptr,
            }),
        );
        g.in_flight = Some(InFlight { index, spec: spec.clone(), bd_ptr, bn_ptr });
    }
    let capture_wire = |inner: &Rc<RefCell<CtrlInner>>, r: &V| -> Result<(), String> {
        let receipt = (inner.borrow().deps.raw_receipt_of)(
            r,
            &json!({
                "method": get(spec, "method"),
                "requestBodyHash": if get(spec, "method").as_str() == Some("POST") { get(&o, "bodyHash").clone() } else { V::Null },
            }),
        )?;
        let record = raw(
            &receipt,
            get(spec, "method").as_str().unwrap_or(""),
            get(spec, "endpoint").as_str().unwrap_or(""),
        )?;
        need(
            instant_v(get(&record, "requestStartedAt"))? >= instant_v(get(r, "requestStartedAt"))?
                && instant_v(get(&record, "receivedAt"))? <= instant_v(get(r, "receivedAt"))?
                && get(&record, "status") == get(r, "status"),
            "WIRE_CLOCK_RELABELED",
        )?;
        if r.get("data").is_some() {
            need(
                payload_hash(&parse_number_lexemes(get(&record, "bodyText").as_str().unwrap_or(""))?.0)?
                    == payload_hash(get(r, "data"))?,
                "WIRE_BODY_CHANGED",
            )?;
        }
        let (current, l) = {
            let g = inner.borrow();
            let st = g.state.clone().unwrap_or(V::Null);
            let current = get(&st, "orders").as_array().and_then(|a| a.get(index as usize)).cloned().unwrap_or(V::Null);
            let l = get(&current, "fences").as_array().and_then(|f| f.last().cloned()).unwrap_or(V::Null);
            (current, l)
        };
        need(!l.is_null(), "NETWORK_FENCE_MISSING")?;
        let wire = json!({
            "kind": get(&current, "operation"),
            "method": get(spec, "method"),
            "endpoint": get(spec, "endpoint"),
            "ownerId": get(&inner.borrow().scope, "ownerId"),
            "runId": get(&inner.borrow().scope, "runId"),
            "intentId": get(&current, "intentId"),
            "idempotencyKey": get(&get(&current, "body"), "idempotencyKey"),
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
        append_event(
            inner,
            &json!({"kind": "wire", "index": index, "wire": wire, "at": iso((inner.borrow().deps.now)())}),
        )
    };
    let result = (inner.borrow().deps.dispatch)(spec, bd, bn);
    let out = match result {
        Ok(r) => {
            capture_wire(inner, &r)?;
            Ok(r)
        }
        Err(e) => {
            if let Some(wr) = e.wire_receipt {
                capture_wire(inner, &wr)?;
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
}

impl PairedRestController {
    // authorizes(spec, beforeDispatch, beforeNetwork)
    pub fn authorizes(&self, spec: &V, before_dispatch: Option<&Guard<'_>>, before_network: Option<&Guard<'_>>) -> bool {
        let g = self.inner.borrow();
        let r: Result<bool, String> = (|| {
            g.basic()?;
            let bd_ptr = before_dispatch.map(|x| x as *const Guard as *const () as usize).unwrap_or(0);
            let bn_ptr = before_network.map(|x| x as *const Guard as *const () as usize).unwrap_or(0);
            let p = g.issued.get(&bd_ptr).cloned();
            Ok(p.is_some()
                && get(p.as_ref().unwrap(), "method") == get(spec, "method")
                && get(p.as_ref().unwrap(), "endpoint") == get(spec, "endpoint")
                && get(p.as_ref().unwrap(), "bodyJson") == get(spec, "bodyJson")
                && get(p.as_ref().unwrap(), "beforeNetworkPtr") == &json!(bn_ptr as u64)
                && self.transport.authorize_request(spec).map(|x| x == true).unwrap_or(false))
        })();
        r.unwrap_or(false)
    }

    // onFetchStart(meta)
    pub fn on_fetch_start(&self, meta: &V) -> Result<bool, String> {
        if get(meta, "method").as_str() == Some("GET") {
            return Ok(false);
        }
        if self.inner.borrow().in_flight.is_none() {
            return Ok(false);
        }
        let r: Result<bool, String> = (|| {
            self.inner.borrow().basic()?;
            keys(meta, &["fetchInvocationId", "method", "endpoint", "requestBodyHash", "requestStartedAt"])?;
            let (o, l, spec) = {
                let g = self.inner.borrow();
                let inflight = g.in_flight.as_ref().unwrap();
                let st = g.state.clone().unwrap_or(V::Null);
                let o = get(&st, "orders").as_array().and_then(|a| a.get(inflight.index as usize)).cloned().unwrap_or(V::Null);
                let l = get(&o, "fences").as_array().and_then(|f| f.last().cloned()).unwrap_or(V::Null);
                (o, l, inflight.spec.clone())
            };
            need(
                !l.is_null()
                    && get(meta, "method") == get(&spec, "method")
                    && get(meta, "endpoint") == get(&spec, "endpoint")
                    && get(meta, "requestBodyHash")
                        == &(if get(meta, "method").as_str() == Some("POST") { get(&o, "bodyHash").clone() } else { V::Null }),
                "FETCH_START_UNBOUND",
            )?;
            let mut start = meta.clone();
            {
                let g = self.inner.borrow();
                let scope = g.scope.clone();
                start.as_object_mut().unwrap().insert("kind".into(), get(&o, "operation").clone());
                start.as_object_mut().unwrap().insert("ownerId".into(), get(&scope, "ownerId").clone());
                start.as_object_mut().unwrap().insert("runId".into(), get(&scope, "runId").clone());
                start.as_object_mut().unwrap().insert("intentId".into(), get(&o, "intentId").clone());
                start.as_object_mut().unwrap().insert("idempotencyKey".into(), get(&get(&o, "body"), "idempotencyKey").clone());
                start.as_object_mut().unwrap().insert("requestBodyJson".into(), get(&o, "bodyJson").clone());
                start.as_object_mut().unwrap().insert("requestBodyHash".into(), get(&o, "bodyHash").clone());
                start.as_object_mut().unwrap().insert("leaseIdentity".into(), get(&l, "leaseIdentity").clone());
                start.as_object_mut().unwrap().insert("leaseHash".into(), get(&l, "leaseHash").clone());
            }
            append_event(
                &self.inner,
                &json!({
                    "kind": "fetch_start",
                    "index": get(&o, "index"),
                    "start": start,
                    "at": iso((self.inner.borrow().deps.now)()),
                }),
            )?;
            Ok(true)
        })();
        match r {
            Ok(v) => Ok(v),
            Err(e) => {
                self.inner.borrow_mut().failed = true;
                Err(e)
            }
        }
    }

    pub fn status(&self) -> V {
        let g = self.inner.borrow();
        let state = g.state.clone().unwrap_or(V::Null);
        let o = active(&state);
        let owned: Vec<V> = get(&state, "orders")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|x| {
                let lot = get(&get(x, "proof"), "lot");
                if lot.is_null() {
                    None
                } else {
                    let mut m = Map::new();
                    m.insert("marketId".into(), get(x, "marketId").clone());
                    m.insert("exchangeId".into(), get(&get(x, "body"), "exchangeId").clone());
                    for (k, v) in lot.as_object().unwrap() {
                        m.insert(k.clone(), v.clone());
                    }
                    Some(V::Object(m))
                }
            })
            .collect();
        let hedge_blocked = o.is_none()
            && !ended(&state)
            && owned.len() == 1
            && (g.deps.now)() as i64 >= instant_v(get(&get(&state, "plan"), "reviewExpiresAt")).unwrap_or(i64::MAX);
        let phase = if let Some(o) = &o {
            get(o, "phase").clone()
        } else if ended(&state) {
            json!(if owned.len() == 2 { "paired" } else if !owned.is_empty() { "orphaned" } else { "unfilled" })
        } else if hedge_blocked {
            json!("hedge_blocked")
        } else {
            json!("ready")
        };
        json!({
            "mode": "paired_rest_lifecycle_v2",
            "planHash": get(&get(&state, "plan"), "planHash"),
            "originalDryPlanHash": get(&get(&state, "plan"), "originalDryPlanHash"),
            "originalProtectiveCost": get(&get(&state, "plan"), "originalProtectiveCost"),
            "pairCostCeiling": get(&get(&state, "plan"), "pairCostCeiling"),
            "minimumOrdinaryMargin": get(&get(&state, "plan"), "sourceMinimumOrdinaryMargin"),
            "planLegs": get(&get(&state, "plan"), "legs").as_array().cloned().unwrap_or_default().iter().map(|l| {
                json!({"marketId": get(l, "marketId"), "exchangeId": get(l, "exchangeId"), "originalLimit": get(l, "originalLimit")})
            }).collect::<Vec<V>>(),
            "reviewExpiresAt": get(&get(&state, "plan"), "reviewExpiresAt"),
            "sequence": g.journal.sequence,
            "hash": g.journal.previous,
            "closed": g.closed,
            "failed": g.failed,
            "busy": g.busy,
            "phase": phase,
            "blockedReason": if hedge_blocked { json!("rules_expired") } else { V::Null },
            "funding": funding(&state).unwrap_or(V::Null),
            "owned": owned,
            "orders": get(&state, "orders").as_array().cloned().unwrap_or_default().iter().map(|x| {
                json!({
                    "intentId": get(x, "intentId"),
                    "orderId": get(x, "orderId"),
                    "marketId": get(x, "marketId"),
                    "exchangeId": get(&get(x, "body"), "exchangeId"),
                    "phase": get(x, "phase"),
                    "body": get(x, "body"),
                    "bodyHash": get(x, "bodyHash"),
                    "cancelPending": get(x, "cancelPending"),
                    "actualWireStarts": get(x, "wireAttempts").as_array().cloned().unwrap_or_default().iter().map(|w| get(w, "requestStartedAt").clone()).collect::<Vec<V>>(),
                    "lastActualFetchStartAt": get(x, "fetchStarts").as_array().and_then(|f| f.last().map(|v| get(v, "requestStartedAt").clone())).unwrap_or(V::Null),
                    "actualFetchCount": get(x, "fetchStarts").as_array().map(|f| f.len()).unwrap_or(0),
                    "recoveryAttemptCount": get(x, "fetchStarts").as_array().cloned().unwrap_or_default().iter().filter(|w| get(w, "kind").as_str() == Some("recover")).count(),
                    "semanticMismatch": get(x, "semanticMismatch"),
                    "proofHash": if get(&get(x, "proof"), "verificationHash").is_null() { V::Null } else { get(&get(x, "proof"), "verificationHash").clone() },
                })
            }).collect::<Vec<V>>(),
            "globalActivity": get(&state, "orders").as_array().cloned().unwrap_or_default().iter().map(|x| {
                get(&get(&get(x, "proof"), "globalActivity"), "status").clone()
            }).collect::<Vec<V>>(),
            "additionalApiClients": 0,
        })
    }

    // dispatchOrder(operation) — place/recover/cancel.
    fn dispatch_order(&self, operation: &str) -> Result<V, String> {
        let mut g = self.inner.borrow_mut();
        need(!g.busy, "BUSY")?;
        g.busy = true;
        drop(g);
        let result: Result<V, String> = (|| {
            self.inner.borrow().basic()?;
            let o = {
                let g = self.inner.borrow();
                active(&g.state.clone().unwrap_or(V::Null)).ok_or("PAIRED_REST_NO_ACTIVE_ORDER")?
            };
            if operation == "place" {
                need(get(&o, "phase").as_str() == Some("prepared"), "UNKNOWN_BLOCKS_REPLACEMENT")?;
            }
            if operation == "recover" {
                recover_row(&self.inner, &o)?;
            }
            self.reserve()?;
            let lease = self.inner.borrow().basic()?;
            let mut permit = Map::new();
            permit.insert("intentId".into(), get(&o, "intentId").clone());
            permit.insert("runId".into(), get(&self.inner.borrow().scope, "runId").clone());
            permit.insert("bodyHash".into(), get(&o, "bodyHash").clone());
            permit.insert("operation".into(), json!(operation));
            if operation == "recover" {
                permit.insert("retry".into(), json!(true));
            }
            let authority = json!({"permit": V::Object(permit), "lease": lease});
            let result = if operation == "cancel" {
                self.transport.cancel(get(&o, "orderId"), &authority).map_err(|e| e.code)
            } else {
                self.transport.place(get(&o, "body"), &authority).map_err(|e| e.code)
            };
            match result {
                Ok(_) => Ok(self.status()),
                Err(e) => {
                    if self.inner.borrow().failed {
                        return Err("PAIRED_REST_DURABLE_RECORDING_FAILED".to_string());
                    }
                    if (self.inner.borrow().deps.can_operate)() {
                        let code = if e.bytes().all(|b| b.is_ascii_uppercase() || b == b'_') && !e.is_empty() {
                            e.clone()
                        } else {
                            "REQUEST_UNCONFIRMED".to_string()
                        };
                        append_event(
                            &self.inner,
                            &json!({
                                "kind": "error",
                                "index": get(&o, "index"),
                                "code": code,
                                "at": iso((self.inner.borrow().deps.now)()),
                            }),
                        )?;
                    }
                    Err(e)
                }
            }
        })();
        self.inner.borrow_mut().busy = false;
        result
    }

    fn reserve(&self) -> Result<V, String> {
        self.inner.borrow().basic()?;
        let wanted = {
            let g = self.inner.borrow();
            funding(&g.state.clone().unwrap_or(V::Null))?
        };
        let r = (self.inner.borrow().deps.install_funding_reservation)(&wanted)?;
        self.inner.borrow().basic()?;
        append_event(
            &self.inner,
            &json!({"kind": "funding", "receipt": r, "at": iso((self.inner.borrow().deps.now)())}),
        )?;
        Ok(r)
    }

    pub fn funding_reservation(&self) -> V {
        let g = self.inner.borrow();
        funding(&g.state.clone().unwrap_or(V::Null)).unwrap_or(V::Null)
    }

    // prepareNext()
    pub fn prepare_next(&self) -> Result<V, String> {
        let mut g = self.inner.borrow_mut();
        need(!g.busy, "BUSY")?;
        g.busy = true;
        drop(g);
        let result: Result<V, String> = (|| {
            self.inner.borrow().basic()?;
            {
                let g = self.inner.borrow();
                let st = g.state.clone().unwrap_or(V::Null);
                need(active(&st).is_none() && !ended(&st), "ACTIVE_OR_FINISHED")?;
            }
            self.reserve()?;
            let e = evidence(&self.inner, "prepare", None)?;
            let index = {
                let g = self.inner.borrow();
                get(&g.state.clone().unwrap_or(V::Null), "orders").as_array().map(|a| a.len() as i64).unwrap_or(0)
            };
            append_event(
                &self.inner,
                &json!({
                    "kind": "prepared",
                    "index": index,
                    "intentId": format!("paired-{}", uuid::Uuid::new_v4()),
                    "idempotencyKey": uuid::Uuid::new_v4().to_string(),
                    "evidence": e,
                    "at": iso((self.inner.borrow().deps.now)()),
                }),
            )?;
            Ok(self.status())
        })();
        self.inner.borrow_mut().busy = false;
        result
    }

    pub fn dispatch_prepared(&self) -> Result<V, String> {
        self.dispatch_order("place")
    }
    pub fn recover_exact(&self) -> Result<V, String> {
        self.dispatch_order("recover")
    }
    pub fn cancel_expired(&self) -> Result<V, String> {
        self.dispatch_order("cancel")
    }

    // reconcile(input)
    pub fn reconcile(&self, input: &V) -> Result<V, String> {
        let mut g = self.inner.borrow_mut();
        need(!g.busy, "BUSY")?;
        g.busy = true;
        drop(g);
        let result: Result<V, String> = (|| {
            self.inner.borrow().basic()?;
            let o = {
                let g = self.inner.borrow();
                active(&g.state.clone().unwrap_or(V::Null)).ok_or("PAIRED_REST_NO_ACTIVE_ORDER")?
            };
            need(
                !get(&o, "orderId").is_null() && !get(&o, "ack").is_null(),
                "ORDER_ACK_REQUIRED",
            )?;
            let recovery_proof = if instant_v(get(&get(&o, "ack"), "requestStartedAt"))? >= instant_v(get(&get(&o, "body"), "expirationDate"))? {
                json!({"journalRecord": recover_row(&self.inner, &o)?})
            } else {
                V::Null
            };
            append_event(
                &self.inner,
                &json!({
                    "kind": "reconciled",
                    "index": get(&o, "index"),
                    "evidence": input,
                    "recoveryProof": recovery_proof,
                    "at": iso((self.inner.borrow().deps.now)()),
                }),
            )?;
            self.reserve()?;
            Ok(self.status())
        })();
        self.inner.borrow_mut().busy = false;
        result
    }

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

impl crate::paired_rest_cycle::PairedCycleDriver for PairedRestController {
    fn status(&self) -> V {
        self.status()
    }
    fn prepare_next(&self) -> Result<V, String> {
        self.prepare_next()
    }
    fn dispatch_prepared(&self) -> Result<V, String> {
        self.dispatch_prepared()
    }
    fn reconcile(&self, evidence: &V) -> Result<V, String> {
        self.reconcile(evidence)
    }
    fn recover_exact(&self) -> Result<V, String> {
        self.recover_exact()
    }
    fn cancel_expired(&self) -> Result<V, String> {
        self.cancel_expired()
    }
}
