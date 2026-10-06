// execution/paired-rest-controller.mjs — a distinct dormant REST execution
// revision. The original capture/dry plan is retained by hash; its observed
// protective cost is not relabeled as the reserve. Sequential hedge
// flexibility uses only the declared ordinary payout margin.
//
// Staged lifecycle only. The caller supplies its EXISTING serialized
// dispatcher, actual account lease, external STOP/activation gate and durable
// global reserve. No credential/client/limiter, activation file, polling loop
// or implicit retry.
//
// Port notes:
// - The JS `issued` WeakMap keyed on transport's beforeDispatch guard is keyed
//   on the guard's address (usize) here — the spec carries the guard pair.
// - JS `instant(x)`/`price(x)`/`amount(x)`/BigInt raises become Err(String).
// - `verifyScopedRestNoBuy` (v1 semantics) is re-implemented privately below:
//   the previously committed no_rest_reconciliation.rs predates the recovery
//   block + scoped account inspector added to the JS module. Shared snapshot/
//   pages/read machinery is reused via crate::no_rest_reconciliation_v2.
// - `workspace_dir` replaces import.meta.url: the repository root containing
//   execution/ so codeIdentity() hashes the same .mjs sources the JS writes.
use crate::core::{canonical_payload, payload_hash};
use crate::gates::normalize_official_fill_tick;
use crate::no_rest_reconciliation_v2 as nrr;
use crate::paired_dry_run::plan_paired_dry_run;
use crate::receipts::parse_number_lexemes;
use crate::transport::{
    scope_of, Dispatch, Guard, OrderTransport, OrderTransportError, OrderTransportOptions,
};
use baal_service_runtime::contract::{instant_ns, ms_to_iso};
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use baal_service_runtime::ledger::{acquire_owner_lease, OwnerLease};
use baal_service_runtime::market::normalize_book;
use serde_json::{json, Map, Value as V};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

pub(crate) fn need(x: bool, c: &str) -> Result<(), String> {
    if x {
        Ok(())
    } else {
        Err(format!("PAIRED_REST_{c}"))
    }
}
pub(crate) fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
pub(crate) fn sha(s: &str) -> String {
    sha256(s.as_bytes())
}
pub(crate) fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
pub(crate) fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty()
            && !s.starts_with('0')
            && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n > 0 && n <= 9_007_199_254_740_991)
    })
}
pub(crate) fn integer(v: &V, min: i64) -> bool {
    js_int(v).is_some_and(|n| n >= min)
}
pub(crate) fn tick(v: &V) -> Result<i128, String> {
    price(v.as_str().unwrap_or(""))
}
pub(crate) fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
pub(crate) fn instant_ns_v(v: &V) -> Result<i128, String> {
    instant_ns(v.as_str().unwrap_or("")).map_err(|e| e.0)
}
pub(crate) fn amount_v(v: &V) -> Result<i128, String> {
    amount(v.as_str().unwrap_or(""))
}
pub(crate) fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}
// keys(o, allowed)
pub(crate) fn keys(o: &V, allowed: &[&str]) -> Result<(), String> {
    need(
        o.as_object().is_some_and(|m| m.keys().all(|k| allowed.contains(&k.as_str()))),
        "UNSUPPORTED_FIELDS",
    )
}
pub(crate) fn js_bigint(v: &V) -> Result<i128, String> {
    match v {
        V::String(s) => s
            .trim()
            .parse::<i128>()
            .map_err(|_| "Cannot convert to a BigInt".to_string()),
        V::Number(n) => n
            .as_i64()
            .map(|x| x as i128)
            .or_else(|| {
                n.as_f64()
                    .filter(|f| f.is_finite() && f.fract() == 0.0 && f.abs() < 9e18)
                    .map(|f| f as i128)
            })
            .ok_or_else(|| "Cannot convert to a BigInt".to_string()),
        V::Bool(b) => Ok(*b as i128),
        _ => Err("Cannot convert to a BigInt".to_string()),
    }
}
// Math.round — half toward +inf.
pub(crate) fn js_round(x: f64) -> i128 {
    (x + 0.5).floor() as i128
}
pub(crate) fn birthtime(meta: &fs::Metadata) -> (i64, i64) {
    let t = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .unwrap_or_default();
    (t.as_secs() as i64, t.subsec_nanos() as i64)
}
fn normalize_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}
fn relative_path(base: &Path, file: &Path) -> String {
    let base_n = normalize_path(base);
    let file_n = normalize_path(file);
    let base_c: Vec<_> = base_n.components().collect();
    let file_c: Vec<_> = file_n.components().collect();
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

// identity() — sha256 over every .mjs reachable through relative imports from
// `entry`, keyed by path relative to the execution/ directory, sorted.
pub(crate) fn code_identity(workspace_dir: &Path, entry: &str) -> Result<V, String> {
    let dir = normalize_path(&workspace_dir.join("execution"));
    let entry = dir.join(entry);
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
        files.push((file.clone(), sha256(&raw)));
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

/// derivePairedRestPlan(input)
pub fn derive_paired_rest_plan(input: &V) -> Result<V, String> {
    let original = plan_paired_dry_run(input)?;
    let original_dry_plan_hash = get(&original, "planHash").clone();
    // {planHash: originalDryPlanHash, ...body}
    let mut body = original.clone();
    if let V::Object(m) = &mut body {
        m.shift_remove("planHash");
    }
    let ceiling = amount_v(get(&original, "conditionalOrdinaryMinimumPayout"))?
        - amount_v(get(get(&original, "policy"), "minimumOrdinaryMargin"))?;
    need(
        ceiling == 990_000
            && ceiling % TICK == 0
            && amount_v(get(&original, "pairCostCeiling"))? <= ceiling,
        "ORDINARY_MARGIN_CEILING",
    )?;
    let mut derived = body.clone();
    if let V::Object(m) = &mut derived {
        m.insert("schemaVersion".into(), json!(2));
        m.insert("mode".into(), json!("paired_rest_no_fixed_margin_v2"));
        m.insert("originalDryPlanHash".into(), original_dry_plan_hash);
        m.insert(
            "originalProtectiveCost".into(),
            get(&original, "pairCostCeiling").clone(),
        );
        m.insert(
            "originalConditionalOrdinarySurplus".into(),
            get(&original, "conditionalOrdinarySurplus").clone(),
        );
        m.insert(
            "sourceMinimumOrdinaryMargin".into(),
            get(get(&original, "policy"), "minimumOrdinaryMargin").clone(),
        );
        m.insert("pairCostCeiling".into(), json!(money(ceiling)));
        m.insert(
            "conditionalOrdinarySurplus".into(),
            json!(money(
                amount_v(get(&original, "conditionalOrdinaryMinimumPayout"))? - ceiling
            )),
        );
        m.insert("exceptionLoss".into(), json!(money(-ceiling)));
    }
    let h = payload_hash(&derived)?;
    let mut out = derived;
    if let V::Object(m) = &mut out {
        m.insert("planHash".into(), json!(h));
    }
    Ok(out)
}

// active(s)
pub(crate) fn active(s: &V) -> Option<V> {
    get(s, "orders")
        .as_array()
        .and_then(|os| os.iter().find(|o| get(o, "phase") != "reconciled").cloned())
}
// cost(s)
pub(crate) fn cost(s: &V) -> Result<i128, String> {
    let mut n = 0i128;
    for o in get(s, "orders").as_array().cloned().unwrap_or_default() {
        let raw = match get(get(&o, "proof"), "scopedPrincipalCost") {
            v if v.is_null() => "0.000000".to_string(),
            v => v.as_str().unwrap_or("").to_string(),
        };
        n += amount(&raw)?;
    }
    Ok(n)
}
// ended(s)
pub(crate) fn ended(s: &V) -> bool {
    let orders = get(s, "orders").as_array().cloned().unwrap_or_default();
    (orders.len() == 2 && orders.iter().all(|o| get(o, "phase") == "reconciled"))
        || (orders
            .first()
            .is_some_and(|o| get(o, "phase") == "reconciled")
            && js_int(get(get(&orders[0], "proof"), "filledQuantity")) == Some(0))
}
// funding(s)
pub(crate) fn funding(s: &V) -> Result<V, String> {
    let used = cost(s)?;
    let mut body = json!({
        "schemaVersion": 1,
        "ownerId": get(get(get(s, "plan"), "scope"), "ownerId"),
        "runId": get(get(get(s, "plan"), "scope"), "runId"),
        "tournamentId": get(get(get(s, "plan"), "scope"), "tournamentId"),
        "planHash": get(get(s, "plan"), "planHash"),
        "allocatedCapital": "2.000000",
        "reservedCash": if ended(s) { "0.000000".to_string() } else {
            money(amount_v(get(get(s, "plan"), "pairCostCeiling"))? - used)
        },
        "ownedPrincipal": money(used),
        "cumulativeBuy": money(used),
        "maxCumulativeBuy": "2.000000",
    });
    let h = payload_hash(&body)?;
    body["reservationHash"] = json!(h);
    Ok(body)
}
// binding(s, o, orderId = o.orderId)
pub(crate) fn binding(s: &V, o: &V, order_id: Option<&V>) -> Result<V, String> {
    Ok(json!({
        "ownerId": get(get(get(s, "plan"), "scope"), "ownerId"),
        "runId": get(get(get(s, "plan"), "scope"), "runId"),
        "intentId": get(o, "intentId"),
        "idempotencyKey": get(get(o, "body"), "idempotencyKey"),
        "orderId": order_id.unwrap_or_else(|| get(o, "orderId")),
        "marketId": get(o, "marketId"),
        "exchangeId": get(get(o, "body"), "exchangeId"),
        "tournamentId": get(get(o, "body"), "tournamentId"),
        "tournamentSlug": get(get(get(s, "plan"), "scope"), "tournamentSlug"),
        "side": "no",
        "action": "buy",
        "quantity": 1,
        "limit": money(js_round(
            get(get(o, "body"), "price").as_f64().unwrap_or(f64::NAN) * 1e6
        )),
        "createdAt": get(o, "createdAt"),
        "expirationDate": get(get(o, "body"), "expirationDate"),
    }))
}
// raw(r, method, endpoint)
pub(crate) fn raw(r: &V, method: &str, endpoint: &str) -> Result<V, String> {
    keys(
        r,
        &[
            "method",
            "endpoint",
            "status",
            "bodyText",
            "requestStartedAt",
            "receivedAt",
            "sourceBodyHash",
            "requestBodyHash",
            "fetchInvocationId",
        ],
    )?;
    need(
        get(r, "method").as_str() == Some(method)
            && get(r, "endpoint").as_str() == Some(endpoint)
            && get(r, "status").is_i64()
            && get(r, "bodyText").as_str().is_some_and(|t| t.len() <= 8 * 1024 * 1024),
        "RAW_RECEIPT_REQUIRED",
    )?;
    instant_v(get(r, "requestStartedAt"))?;
    instant_v(get(r, "receivedAt"))?;
    need(
        instant_v(get(r, "receivedAt"))? >= instant_v(get(r, "requestStartedAt"))?,
        "RAW_RECEIPT_CLOCK",
    )?;
    let body_hash = sha(get(r, "bodyText").as_str().unwrap_or(""));
    if let Some(h) = r.get("sourceBodyHash") {
        need(h == &json!(body_hash), "RAW_RECEIPT_HASH")?;
    }
    let mut out = r.clone();
    out["sourceBodyHash"] = json!(body_hash);
    Ok(out)
}
// accountShape(a)
pub(crate) fn account_shape(a: &V) -> Result<(), String> {
    keys(a, &["cash", "positions", "openOrdersPages", "scopedOrdersPages"])?;
    let mut all: Vec<V> = vec![get(a, "cash").clone(), get(a, "positions").clone()];
    all.extend(
        get(a, "openOrdersPages")
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    all.extend(
        get(a, "scopedOrdersPages")
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    for r in &all {
        raw(r, "GET", get(r, "endpoint").as_str().unwrap_or(""))?;
    }
    Ok(())
}
// expectedPositions(s)
pub(crate) fn expected_positions(s: &V) -> Result<Vec<V>, String> {
    let mut out: Vec<V> = get(s, "externalPositions")
        .as_array()
        .cloned()
        .unwrap_or_default();
    for o in get(s, "orders").as_array().cloned().unwrap_or_default() {
        if js_truthy(o.get("proof").and_then(|p| p.get("filledQuantity"))) {
            out.push(json!({
                "exchangeId": get(get(&o, "body"), "exchangeId"),
                "marketId": get(&o, "marketId"),
                "quantity": -1,
                "settled": false,
            }));
        }
    }
    out.retain(|p| js_truthy(p.get("quantity")));
    out.sort_by(|a, b| {
        let an = get(a, "exchangeId").as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN);
        let bn = get(b, "exchangeId").as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN);
        an.partial_cmp(&bn).unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}
// globalGate(g, s, at, own)
pub(crate) fn global_gate(g: &V, s: &V, at: f64, own: &V) -> Result<(), String> {
    let scope = get(get(s, "plan"), "scope");
    need(
        get(g, "tournamentId") == get(scope, "tournamentId")
            && get(g, "ownerId") == get(scope, "ownerId")
            && get(g, "runId") == get(scope, "runId")
            && get(g, "complete") == &json!(true)
            && get(g, "soleCollector") == &json!(true)
            && get(g, "stopped") == &json!(false)
            && hex(get(g, "sourceHash"))
            && get(g, "unresolvedIntentIds").as_array().is_some_and(|a| a.len() <= 1)
            && get(g, "unresolvedIntentIds")
                .as_array()
                .map(|a| a.iter().all(|v| v == own))
                .unwrap_or(false)
            && (instant_v(get(g, "at"))? as f64) <= at
            && at - (instant_v(get(g, "at"))? as f64) <= 20_000.0,
        "GLOBAL_UNCERTAINTY",
    )
}

// preflight(s, e, at, o) -> {index, leg, cap, book, positions, account}
pub(crate) fn preflight(
    s: &V,
    e: &V,
    at: f64,
    o: Option<&V>,
) -> Result<(i64, V, i128, V, Vec<V>, V), String> {
    keys(
        e,
        &["tournamentId", "account", "book", "globalRisk", "funding", "reservedElsewhere"],
    )?;
    account_shape(get(e, "account"))?;
    need(
        !ended(s) && (instant_v(get(get(s, "plan"), "reviewExpiresAt"))? as f64) > at,
        "PAIR_FINISHED_OR_RULES_EXPIRED",
    )?;
    let orders = get(s, "orders").as_array().cloned().unwrap_or_default();
    let index = match o {
        Some(o) => js_int(get(o, "index")).unwrap_or(i64::MIN),
        None => orders.len() as i64,
    };
    let legs = get(get(s, "plan"), "legs").as_array().cloned().unwrap_or_default();
    let leg = legs.get(index as usize).cloned().unwrap_or(V::Null);
    need(!leg.is_null(), "PAIR_LIMIT")?;
    if index == 0 {
        need(
            legs.iter().all(|l| {
                instant_v(get(get(l, "book"), "requestStartedAt"))
                    .map(|t| at - t as f64 <= 20_000.0)
                    .unwrap_or(false)
            }),
            "INITIAL_PAIR_BOOKS_STALE",
        )?;
    }
    let cap = if index == 0 {
        price(get(&leg, "originalLimit").as_str().unwrap_or(""))?
    } else {
        amount_v(get(get(s, "plan"), "pairCostCeiling"))? - cost(s)?
    };
    need(
        cap >= TICK && cap <= SCALE - TICK && cap <= 1_000_000 && (index != 0 || cap <= 500_000),
        "COST_CAP",
    )?;
    if index == 1 {
        need(
            orders.first().is_some_and(|x| get(x, "phase") == "reconciled")
                && js_int(get(get(&orders[0], "proof"), "filledQuantity")) == Some(1),
            "FIRST_LEG_UNRESOLVED",
        )?;
    }
    let own = o
        .and_then(|o| o.get("intentId"))
        .cloned()
        .unwrap_or(V::Null);
    global_gate(get(e, "globalRisk"), s, at, &own)?;
    need(
        get(e, "tournamentId") == get(get(get(s, "plan"), "scope"), "tournamentId"),
        "BOOK_SCOPE",
    )?;
    let reserve = funding(s)?;
    let installed = get(e, "funding");
    need(
        get(installed, "reservationHash") == get(&reserve, "reservationHash")
            && get(installed, "amount") == get(&reserve, "reservedCash")
            && hex(get(installed, "globalLedgerHash"))
            && (instant_v(get(installed, "confirmedAt"))? as f64) <= at
            && at - (instant_v(get(installed, "confirmedAt"))? as f64) <= 20_000.0,
        "GLOBAL_FUNDING_NOT_INSTALLED",
    )?;
    let mut b = get(get(s, "plan"), "scope").clone();
    b["exchangeId"] = get(&leg, "exchangeId").clone();
    let default_zero = json!("0.000000");
    let a = nrr::inspect_scoped_rest_no_account(&json!({
        "account": get(e, "account"),
        "binding": b,
        "readAt": ms_to_iso(at as f64),
        "requiredFunding": get(&reserve, "reservedCash"),
        "reservedElsewhere": e.get("reservedElsewhere").filter(|v| !v.is_null()).unwrap_or(&default_zero),
    }))?;
    need(get(&a, "funded") == &json!(true), "FULL_PAIR_FUNDING")?;
    let mut positions: Vec<V> = get(&a, "positions")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| js_truthy(p.get("quantity")))
        .collect();
    positions.sort_by(|x, y| {
        let xn = get(x, "exchangeId").as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN);
        let yn = get(y, "exchangeId").as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN);
        xn.partial_cmp(&yn).unwrap_or(std::cmp::Ordering::Equal)
    });
    if get(s, "externalPositions").is_null() {
        need(
            !positions.iter().any(|p| {
                legs.iter().any(|l| get(l, "exchangeId") == get(p, "exchangeId"))
            }),
            "EXISTING_PAIR_HOLDING",
        )?;
    } else {
        need(
            payload_hash(&json!(positions))? == payload_hash(&json!(expected_positions(s)?))?,
            "ACCOUNT_POSITION_DRIFT",
        )?;
    }
    let endpoint = format!(
        "/markets/{}/orderbook?tournamentId={}&depth=20",
        crate::js_key(get(&leg, "marketId")),
        get(get(get(s, "plan"), "scope"), "tournamentId").as_str().unwrap_or("")
    );
    let receipt = raw(get(e, "book"), "GET", &endpoint)?;
    need(js_int(get(&receipt, "status")) == Some(200), "BOOK_HTTP")?;
    let (data, _numbers) =
        parse_number_lexemes(get(&receipt, "bodyText").as_str().unwrap_or(""))?;
    let book = normalize_book(
        &data,
        crate::js_key(get(&leg, "marketId")).as_str(),
        get(get(get(s, "plan"), "scope"), "tournamentId").as_str().unwrap_or(""),
        get(&receipt, "receivedAt").as_str().unwrap_or(""),
        get(&receipt, "requestStartedAt").as_str().unwrap_or(""),
        20_000.0,
        &BTreeSet::new(),
        "authoritative_receipt_v1",
    )?;
    need(
        get(&data, "contexts").as_array().is_some_and(|c| c.len() == 1)
            && get(&get(&data, "contexts")[0], "type") == "tournament"
            && get(&book, "exchangeId") == get(&leg, "exchangeId")
            && get(&book, "bids").as_array().is_some_and(|x| x.len() <= 20)
            && get(&book, "asks").as_array().is_some_and(|x| x.len() <= 20)
            && instant_v(get(&a, "completedAt"))? <= instant_v(get(&book, "requestStartedAt"))?
            && at - (instant_v(get(&book, "requestStartedAt"))? as f64) <= 20_000.0
            && (instant_v(get(&book, "receivedAt"))? as f64) <= at,
        "BOOK_ACCOUNT_CAUSALITY",
    )?;
    let prior = o
        .and_then(|o| o.get("book"))
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or_else(|| get(&leg, "book").clone());
    need(
        js_bigint(get(&book, "sequence"))? >= js_bigint(get(&prior, "sequence"))?
            && instant_ns_v(get(&book, "sourceAt"))? >= instant_ns_v(get(&prior, "sourceAt"))?
            && instant_v(get(&book, "requestStartedAt"))? >= instant_v(get(&prior, "requestStartedAt"))?,
        "BOOK_ROLLBACK",
    )?;
    if get(&book, "versionKey") == get(&prior, "versionKey") {
        need(
            payload_hash(&json!([get(&book, "bids"), get(&book, "asks")]))?
                == payload_hash(&json!([get(&prior, "bids"), get(&prior, "asks")]))?,
            "BOOK_VERSION_CONFLICT",
        )?;
    }
    need(
        get(&book, "bids").as_array().is_some_and(|bids| {
            bids.iter().any(|l| {
                (get(l, "quantity").as_f64().unwrap_or(f64::NAN) / 100.0).floor() >= 1.0
                    && tick(get(l, "price")).map(|p| SCALE - p <= cap).unwrap_or(false)
            })
        }),
        "DEPTH_OR_PRICE",
    )?;
    if let Some(o) = o {
        need(
            at < instant_v(get(get(o, "body"), "expirationDate"))? as f64
                && price(
                    get(&binding(s, o, None)?, "limit").as_str().unwrap_or(""),
                )? == cap,
            "EXPIRED_OR_CAP_CHANGED",
        )?;
    }
    Ok((index, leg, cap, book, positions, a))
}

// canonicalAck(o, r) — try{ parse and match } catch{ false }
pub(crate) fn canonical_ack_impl(o: &V, r: &V) -> bool {
    let Ok((d, _)) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or("")) else {
        return false;
    };
    js_int(get(&d, "orderId")).is_some_and(|i| i > 0)
        && get(&d, "exchangeId") == get(get(o, "body"), "exchangeId")
        && get(&d, "side") == "no"
        && get(&d, "action") == "buy"
        && js_int(get(&d, "quantity")) == Some(1)
        && tick(get(&d, "price"))
            .ok()
            .zip(price(&crate::js_key(get(get(o, "body"), "price"))).ok())
            .is_some_and(|(a, b)| a == b)
        && d.get("all") == Some(&V::Null)
        && get(&d, "open").is_boolean()
        && js_int(get(&d, "quantityTraded")).is_some()
        && js_int(get(&d, "remainingQuantity")).is_some()
        && js_int(get(&d, "quantityTraded")).unwrap_or(0) >= 0
        && js_int(get(&d, "remainingQuantity")).unwrap_or(0) >= 0
        && js_int(get(&d, "quantityTraded")).unwrap_or(0)
            + js_int(get(&d, "remainingQuantity")).unwrap_or(0)
            == 1
}

// verifyScopedRestNoBuy (v1.mjs semantics) — local port using the shared
// snapshot/read/pages machinery of crate::no_rest_reconciliation_v2.
pub(crate) fn verify_scoped_rest_no_buy_v1(input: &V) -> Result<V, String> {
    fn nneed(x: bool, c: &str) -> Result<(), String> {
        if x {
            Ok(())
        } else {
            Err(format!("REST_NO_{c}"))
        }
    }
    let b = get(input, "binding");
    let now = instant_v(get(input, "readAt"))?;
    nrr::scope_check(b)?;
    nneed(
        id(get(b, "orderId"))
            && id(get(b, "marketId"))
            && id(get(b, "exchangeId"))
            && get(b, "ownerId").is_string()
            && get(b, "runId").is_string()
            && get(b, "intentId").is_string()
            && get(b, "idempotencyKey").is_string()
            && get(b, "side") == "no"
            && get(b, "action") == "buy"
            && js_int(get(b, "quantity")) == Some(1),
        "BINDING",
    )?;
    let limit = price(get(b, "limit").as_str().unwrap_or(""))
        .map_err(|e| format!("REST_NO_{e}"))?;
    let created = instant_v(get(b, "createdAt"))?;
    let expires = instant_v(get(b, "expirationDate"))?;
    nneed(
        expires > created && expires - created <= 30_000 && created <= now,
        "BINDING_LIFETIME",
    )?;
    let g = get(input, "globalRisk");
    nneed(
        get(g, "tournamentId") == get(b, "tournamentId")
            && get(g, "ownerId") == get(b, "ownerId")
            && get(g, "runId") == get(b, "runId")
            && get(g, "soleCollector") == &json!(true)
            && get(g, "complete") == &json!(true)
            && get(g, "stopped") == &json!(false)
            && hex(get(g, "sourceHash"))
            && get(g, "unresolvedIntentIds").as_array().is_some_and(|a| a.len() <= 1)
            && get(g, "unresolvedIntentIds")
                .as_array()
                .map(|a| a.iter().all(|x| x == get(b, "intentId")))
                .unwrap_or(false)
            && instant_v(get(g, "at"))? <= now
            && now - instant_v(get(g, "at"))? <= 20_000,
        "GLOBAL_UNKNOWN_OR_OWNER",
    )?;
    let default_zero = json!("0.000000");
    let full_pair_reservation = get(input, "fullPairReservation");
    let reserved_elsewhere = input.get("reservedElsewhere").unwrap_or(&default_zero);
    let reserved = amount_v(full_pair_reservation)?;
    let other = amount_v(reserved_elsewhere)?;
    nneed(
        reserved >= limit && reserved <= 2_000_000 && other >= 0,
        "PAIR_RESERVATION",
    )?;
    let pre = nrr::snapshot(get(input, "before"), b, created)?;
    let post = nrr::snapshot(get(input, "after"), b, now)?;
    nneed(pre.end <= created && post.start >= created, "ACCOUNT_CAUSALITY")?;
    nneed(pre.cash.lower >= reserved + other, "CONSERVATIVE_FUNDING")?;
    nneed(
        !js_truthy(pre.positions.get(get(b, "exchangeId").as_str().unwrap_or("")).and_then(|p| p.get("quantity"))),
        "PRIOR_TARGET_NOT_FLAT",
    )?;
    if let Some(p) = pre.positions.get(get(b, "exchangeId").as_str().unwrap_or("")) {
        nneed(
            get(p, "marketId") == get(b, "marketId"),
            "PRIOR_TARGET_SCOPE",
        )?;
    }
    let original_body = json!({
        "idempotencyKey": get(b, "idempotencyKey"),
        "exchangeId": get(b, "exchangeId"),
        "side": "no",
        "action": "buy",
        "quantity": 1,
        "price": get(b, "limit").as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN),
        "expirationDate": get(b, "expirationDate"),
        "tournamentId": get(b, "tournamentId"),
    });
    let a = nrr::read(get(input, "ack"), "POST", "/orders", now, false)?;
    nneed(
        get(get(input, "ack"), "requestBodyHash") == &json!(payload_hash(&original_body)?)
            && a.start >= created,
        "ACK_REQUEST_BINDING",
    )?;
    let mut recovered = V::Null;
    if a.start >= expires {
        // The lifecycle must additionally source this FULL row from its own
        // replayed journal. A supplied hash/flag cannot replace the actual
        // original wire data.
        let row = get(get(input, "recoveryProof"), "journalRecord").clone();
        let mut body = row.clone();
        let row_hash = match &mut body {
            V::Object(m) => m.shift_remove("hash").unwrap_or(V::Null),
            _ => V::Null,
        };
        let wire = get(get(get(&body, "event"), "input"), "start").clone();
        nneed(
            hex(&row_hash)
                && sha256(js_stringify(&body).as_bytes()) == row_hash.as_str().unwrap_or("")
                && integer(get(&body, "sequence"), 1)
                && get(get(get(&body, "event"), "input"), "kind").as_str() == Some("fetch_start")
                && get(&wire, "kind").as_str() == Some("place")
                && get(&wire, "method").as_str() == Some("POST")
                && get(&wire, "endpoint").as_str() == Some("/orders")
                && get(&wire, "fetchInvocationId").as_str().is_some_and(|s| !s.is_empty())
                && get(&wire, "ownerId") == get(b, "ownerId")
                && get(&wire, "runId") == get(b, "runId")
                && get(&wire, "intentId") == get(b, "intentId")
                && get(&wire, "idempotencyKey") == get(b, "idempotencyKey")
                && get(&wire, "requestBodyJson").as_str()
                    == Some(canonical_payload(&original_body)?.as_str())
                && get(&wire, "requestBodyHash") == get(get(input, "ack"), "requestBodyHash")
                && hex(get(&wire, "leaseHash"))
                && get(get(&wire, "leaseIdentity"), "ownerId") == get(b, "ownerId")
                && get(get(&wire, "leaseIdentity"), "tournamentId") == get(b, "tournamentId")
                && get(&wire, "leaseHash") == &json!(payload_hash(get(&wire, "leaseIdentity"))?)
                && instant_v(get(&wire, "requestStartedAt"))? >= created
                && instant_v(get(&wire, "requestStartedAt"))? < expires
                && instant_v(get(&wire, "requestStartedAt"))? <= a.start,
            "ORIGINAL_DISPATCH_RECEIPT_REQUIRED",
        )?;
        recovered = json!({
            "kind": "idempotent_recovery_receipt",
            "originalRequestStartedAt": get(&wire, "requestStartedAt"),
            "fetchInvocationId": get(&wire, "fetchInvocationId"),
            "dispatchJournalHash": row_hash,
            "dispatchSequence": get(&body, "sequence"),
            "leaseHash": get(&wire, "leaseHash"),
        });
    }
    let d = &a.data;
    nneed(
        integer(get(d, "orderId"), 1)
            && crate::js_key(get(d, "orderId")) == get(b, "orderId").as_str().unwrap_or("")
            && get(d, "exchangeId") == get(b, "exchangeId")
            && get(d, "side") == "no"
            && get(d, "action") == "buy"
            && js_int(get(d, "quantity")) == Some(1)
            && tick(get(d, "price"))? == limit
            && d.get("all") == Some(&V::Null)
            && get(d, "open").is_boolean()
            && integer(get(d, "quantityTraded"), 0)
            && integer(get(d, "remainingQuantity"), 0)
            && js_int(get(d, "quantityTraded")).unwrap_or(0)
                + js_int(get(d, "remainingQuantity")).unwrap_or(0)
                == 1,
        "ACK_CANONICAL",
    )?;
    let ack_cost = tick(get(d, "totalCost"))?;
    nneed(
        if js_int(get(d, "quantityTraded")) == Some(0) {
            ack_cost == 0
        } else {
            ack_cost > 0 && ack_cost <= limit
        },
        "ACK_NOTIONAL",
    )?;
    let terminal = nrr::read(
        get(input, "order"),
        "GET",
        &format!("/orders/{}", get(b, "orderId").as_str().unwrap_or("")),
        now,
        true,
    )?;
    let od = &terminal.data;
    nneed(
        integer(get(od, "id"), 1)
            && crate::js_key(get(od, "id")) == get(b, "orderId").as_str().unwrap_or("")
            && get(od, "exchangeId") == get(b, "exchangeId")
            && get(od, "tournamentId") == get(b, "tournamentId")
            && get(od, "side") == "no"
            && get(od, "action") == "buy"
            && js_int(get(od, "quantity")) == Some(1)
            && integer(get(od, "quantityFilled"), 0)
            && js_int(get(od, "quantityFilled")).unwrap_or(0) <= 1
            && tick(get(od, "priceLimit"))? == limit
            && get(od, "open") == &json!(false)
            && instant_v(get(od, "expirationDate"))? == expires
            && instant_v(get(od, "createdAt"))? >= created - 2_000
            && instant_v(get(od, "createdAt"))? <= std::cmp::min(a.end, expires) + 2_000,
        "TERMINAL_ORDER",
    )?;
    let order_id_for_check = get(b, "orderId").clone();
    let totals = RefCell::new(V::Null);
    let average = RefCell::new(V::Null);
    let b_ex = get(b, "exchangeId").clone();
    let b_tid = get(b, "tournamentId").clone();
    let f = nrr::pages(
        get(input, "fillPages"),
        &format!("/orders/{}/fills?limit=200", get(b, "orderId").as_str().unwrap_or("")),
        now,
        Some(&mut |data: &V| -> Result<(), String> {
            nneed(
                integer(get(data, "orderId"), 1)
                    && crate::js_key(get(data, "orderId"))
                        == order_id_for_check.as_str().unwrap_or("")
                    && get(data, "exchangeId") == &b_ex
                    && get(data, "tournamentId") == &b_tid
                    && js_int(get(data, "totalQuantityFilled")).is_some()
                    && [0i64, -1]
                        .contains(&js_int(get(data, "totalQuantityFilled")).unwrap_or(i64::MIN)),
                "FILLS_SCOPE_OR_TOTAL",
            )?;
            let av = if data.get("avgFillPrice") == Some(&V::Null) {
                V::Null
            } else {
                json!(money(tick(get(data, "avgFillPrice"))?))
            };
            if !totals.borrow().is_null() {
                nneed(
                    *totals.borrow() == *get(data, "totalQuantityFilled")
                        && *average.borrow() == av,
                    "FILL_PROJECTION_CHANGED",
                )?;
            }
            *totals.borrow_mut() = get(data, "totalQuantityFilled").clone();
            *average.borrow_mut() = av;
            Ok(())
        }),
    )?;
    nneed(
        f.sources
            .first()
            .map(|s| s.start >= terminal.end)
            .unwrap_or(false),
        "FILLS_BEFORE_TERMINAL",
    )?;
    let mut seen: BTreeSet<i64> = BTreeSet::new();
    let mut fills: Vec<V> = Vec::new();
    let mut principal = 0i128;
    let mut quantity = 0i64;
    for row in &f.rows {
        nneed(
            integer(get(row, "id"), 1)
                && !seen.contains(&js_int(get(row, "id")).unwrap_or(0))
                && get(row, "side") == "no"
                && js_int(get(row, "quantity")) == Some(-1),
            "FILL_IDENTITY_OR_QUANTITY",
        )?;
        seen.insert(js_int(get(row, "id")).unwrap_or(0));
        let at = instant_v(get(row, "filledAt"))?;
        let p = tick(get(row, "price"))?;
        nneed(
            at >= instant_v(get(od, "createdAt"))?
                && at <= f.sources.last().map(|s| s.end).unwrap_or(i64::MIN)
                && at <= now
                && p > 0
                && p <= limit,
            "FILL_PRICE_OR_TIME",
        )?;
        quantity += 1;
        principal += p;
        fills.push(json!({
            "id": get(row, "id"),
            "orderId": get(b, "orderId"),
            "marketId": get(b, "marketId"),
            "exchangeId": get(b, "exchangeId"),
            "tournamentId": get(b, "tournamentId"),
            "side": "no",
            "signedQuantity": -1,
            "quantity": 1,
            "price": money(p),
            "filledAt": get(row, "filledAt"),
            "source": "rest_order_fills",
            "priceBasis": "side_relative",
        }));
    }
    nneed(
        quantity <= 1
            && *totals.borrow() == json!(-quantity)
            && (quantity == 0 || *average.borrow() == json!(money(principal)))
            && js_int(get(d, "quantityTraded")).unwrap_or(i64::MIN) <= quantity,
        "COMPLETE_FILL_TOTAL",
    )?;
    nneed(
        js_int(get(od, "quantityFilled")) == Some(quantity),
        "ORDER_FILL_TOTAL",
    )?;
    if js_int(get(d, "quantityTraded")) == Some(1) {
        nneed(principal == ack_cost, "ACK_FINAL_NOTIONAL_MISMATCH")?;
    } else {
        nneed(d.get("fillPrice") == Some(&V::Null), "ACK_UNFILLED_PRICE")?;
    }
    if js_int(get(d, "quantityTraded")) == Some(1) {
        nneed(tick(get(d, "fillPrice"))? == principal, "ACK_FILL_PRICE")?;
    }
    let mut post_floor = std::cmp::max(
        terminal.end,
        f.sources.last().map(|s| s.end).unwrap_or(i64::MIN),
    );
    for v in &fills {
        post_floor = std::cmp::max(post_floor, instant_v(get(v, "filledAt"))?);
    }
    nneed(post.start >= post_floor, "POST_ACCOUNT_BEFORE_ORDER_EVIDENCE")?;
    nneed(
        instant_v(get(g, "at"))? >= post.end,
        "GLOBAL_RISK_BEFORE_ACCOUNT",
    )?;
    let oid = get(b, "orderId").as_str().unwrap_or("").parse::<i64>().unwrap_or(0);
    nneed(
        !pre.history.contains_key(&oid.to_string()) && post.history.contains_key(&oid.to_string()),
        "SCOPED_ORDER_MISSING_OR_PREEXISTING",
    )?;
    for key in pre
        .history
        .keys()
        .chain(post.history.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
    {
        if key != oid.to_string() {
            nneed(
                canonical_payload(pre.history.get(&key).unwrap_or(&V::Null))?
                    == canonical_payload(post.history.get(&key).unwrap_or(&V::Null))?,
                "EXTERNAL_SCOPED_ORDER_ACTIVITY",
            )?;
        }
    }
    let final_order = post.history.get(&oid.to_string()).cloned().unwrap_or(V::Null);
    for key in [
        "id",
        "exchangeId",
        "tournamentId",
        "side",
        "action",
        "quantity",
        "priceLimit",
        "open",
        "createdAt",
        "expirationDate",
    ] {
        nneed(
            canonical_payload(get(&final_order, key))? == canonical_payload(get(od, key))?,
            "SCOPED_ORDER_DETAIL_MISMATCH",
        )?;
    }
    nneed(
        integer(get(&final_order, "quantityFilled"), 0)
            && js_int(get(&final_order, "quantityFilled")) == Some(quantity),
        "SCOPED_ORDER_FILL_TOTAL",
    )?;
    let target = post.positions.get(get(b, "exchangeId").as_str().unwrap_or(""));
    nneed(
        if quantity == 0 {
            !js_truthy(target.and_then(|t| t.get("quantity")))
        } else {
            target.is_some_and(|t| {
                get(t, "marketId") == get(b, "marketId")
                    && js_int(get(t, "quantity")) == Some(-1)
                    && get(t, "settled") == &json!(false)
            })
        },
        "SCOPED_POSITION_DELTA",
    )?;
    nrr::same_external(&pre.positions, &post.positions, get(b, "exchangeId").as_str().unwrap_or(""))?;
    let delta_low = post.cash.lower - pre.cash.upper;
    let delta_high = post.cash.upper - pre.cash.lower;
    let expected = -principal;
    nneed(
        expected >= delta_low && expected <= delta_high,
        "CASH_INTERVAL_INCONSISTENT",
    )?;
    let mut activity_status = "unknown";
    let mut activity_hash = V::Null;
    let account_activity = get(input, "accountActivity");
    if !account_activity.is_null() {
        nneed(
            get(account_activity, "tournamentId") == get(b, "tournamentId")
                && hex(get(account_activity, "sourceHash"))
                && get(account_activity, "complete").is_boolean(),
            "ACTIVITY_SCHEMA",
        )?;
        if get(account_activity, "complete") == &json!(true) {
            nneed(
                instant_v(get(account_activity, "from"))? <= pre.start
                    && instant_v(get(account_activity, "through"))? >= post.end
                    && instant_v(get(account_activity, "through"))? <= now
                    && get(account_activity, "events").is_array(),
                "ACTIVITY_WINDOW",
            )?;
            let events = get(account_activity, "events").as_array().cloned().unwrap_or_default();
            nneed(
                events.len() == fills.len()
                    && events.iter().all(|e| {
                        get(e, "kind") == "fill"
                            && get(e, "orderId") == get(b, "orderId")
                            && get(e, "marketId") == get(b, "marketId")
                            && get(e, "exchangeId") == get(b, "exchangeId")
                            && fills.iter().any(|v| {
                                get(v, "id") == get(e, "fillId")
                                    && get(v, "filledAt") == get(e, "at")
                            })
                    })
                    && events
                        .iter()
                        .map(|e| crate::js_key(get(e, "fillId")))
                        .collect::<BTreeSet<String>>()
                        .len()
                        == fills.len(),
                "UNRELATED_OR_MISSING_ACTIVITY",
            )?;
            activity_status = "independently_complete_isolated";
            activity_hash = json!(payload_hash(account_activity)?);
        } else {
            let events = get(account_activity, "events").as_array().cloned().unwrap_or_default();
            nneed(
                get(account_activity, "events").is_array()
                    && events.iter().all(|e| {
                        get(e, "kind") == "fill"
                            && get(e, "orderId") == get(b, "orderId")
                            && get(e, "marketId") == get(b, "marketId")
                            && get(e, "exchangeId") == get(b, "exchangeId")
                            && fills.iter().any(|v| {
                                get(v, "id") == get(e, "fillId")
                                    && get(v, "filledAt") == get(e, "at")
                            })
                    }),
                "KNOWN_UNRELATED_ACTIVITY",
            )?;
            activity_status = "incomplete";
            activity_hash = json!(payload_hash(account_activity)?);
        }
    }
    let sources = json!({
        "before": pre.sources,
        "ack": {
            "endpoint": a.endpoint,
            "sourceBodyHash": a.source_body_hash,
            "receivedAt": a.received_at,
            "requestBodyHash": get(get(input, "ack"), "requestBodyHash"),
        },
        "terminal": {
            "endpoint": terminal.endpoint,
            "sourceBodyHash": terminal.source_body_hash,
            "receivedAt": terminal.received_at,
        },
        "fills": f.sources.iter().map(|r| json!({
            "endpoint": r.endpoint,
            "sourceBodyHash": r.source_body_hash,
            "receivedAt": r.received_at,
        })).collect::<Vec<V>>(),
        "after": post.sources,
    });
    let mut result = json!({
        "schemaVersion": 1,
        "mode": "scoped_rest_no_lot_with_rounded_cash_v1",
        "readAt": get(input, "readAt"),
        "bindingHash": payload_hash(b)?,
        "orderId": get(b, "orderId"),
        "intentId": get(b, "intentId"),
        "ownerId": get(b, "ownerId"),
        "runId": get(b, "runId"),
        "tournamentId": get(b, "tournamentId"),
        "marketId": get(b, "marketId"),
        "exchangeId": get(b, "exchangeId"),
        "scopedLotVerified": true,
        "filledQuantity": quantity,
        "signedPositionDelta": -quantity,
        "scopedPrincipalCost": money(principal),
        "expectedCanonicalTradeCashDelta": money(expected),
        "fills": fills,
        "lot": if quantity != 0 { json!({
            "lotId": format!("order:{}:fill:{}", get(b, "orderId").as_str().unwrap_or(""), get(&fills[0], "id").as_i64().unwrap_or(0)),
            "side": "no",
            "quantity": 1,
            "entryPrice": money(principal),
            "openedAt": get(&fills[0], "filledAt"),
            "ownerId": get(b, "ownerId"),
            "runId": get(b, "runId"),
            "intentId": get(b, "intentId"),
            "orderId": get(b, "orderId"),
        }) } else { V::Null },
        "canonicalAckFilledQuantity": get(d, "quantityTraded"),
        "ackCoversFinalFill": js_int(get(d, "quantityTraded")) == Some(quantity),
        "ackNotional": money(ack_cost),
    });
    if !recovered.is_null() {
        result["ackProvenance"] = recovered;
    }
    let tail = json!({
        "requestedLimit": money(limit),
        "requestedExpirationDate": get(b, "expirationDate"),
        "postExpiryFillObserved": fills.iter().any(|v| instant_v(get(v, "filledAt")).unwrap_or(i64::MIN) > expires),
        "globalIntentCensus": {
            "sourceHash": get(g, "sourceHash"),
            "at": get(g, "at"),
            "unresolvedIntentIds": get(g, "unresolvedIntentIds"),
        },
        "funding": {
            "fullPairReservation": full_pair_reservation,
            "reservedElsewhere": reserved_elsewhere,
            "conservativeBeforeCash": money(pre.cash.lower),
            "funded": true,
        },
        "accountCash": {
            "basis": "rounded_cent_with_conservative_plus_minus_one_cent",
            "before": {
                "displayed": money(pre.cash.displayed),
                "lower": money(pre.cash.lower),
                "upper": money(pre.cash.upper),
            },
            "after": {
                "displayed": money(post.cash.displayed),
                "lower": money(post.cash.lower),
                "upper": money(post.cash.upper),
            },
            "observedDeltaInterval": {"lower": money(delta_low), "upper": money(delta_high)},
            "consistent": true,
            "exactDelta": V::Null,
        },
        "scopedOrderActivity": {
            "complete": true,
            "beforeOrderCount": pre.history.len(),
            "afterOrderCount": post.history.len(),
            "newOrderIds": [get(b, "orderId")],
            "unrelatedChanges": 0,
        },
        "globalActivity": {
            "status": activity_status,
            "evidenceHash": activity_hash,
            "independentlyComplete": activity_status == "independently_complete_isolated",
        },
        "globalCashReconciledExactly": false,
        "globalAccountActivityVerified": activity_status == "independently_complete_isolated",
        "sourceReceipts": sources,
        "authoritativeReconciliation": false,
        "executionAuthority": false,
        "newSubmissionAuthorized": false,
    });
    if let (V::Object(m), V::Object(t)) = (&mut result, tail) {
        for (k, v) in t {
            m.insert(k, v);
        }
    }
    let h = payload_hash(&result)?;
    result["verificationHash"] = json!(h);
    Ok(result)
}

// reduce(s, e) — pure journal transition.
fn reduce(s: &V, e: &V) -> Result<V, String> {
    if get(e, "kind") == "initialize" {
        need(s.is_null(), "ALREADY_INITIALIZED")?;
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
    need(!s.is_null(), "NOT_INITIALIZED")?;
    let mut n = s.clone();
    let idx = js_int(get(e, "index")).unwrap_or(i64::MIN) as usize;
    let o_exists = get(&n, "orders").as_array().is_some_and(|a| a.get(idx).is_some());
    let mut o = get(&n, "orders").as_array().and_then(|a| a.get(idx).cloned()).unwrap_or(V::Null);
    if get(e, "kind") == "funding" {
        let f = funding(s)?;
        need(
            get(get(e, "receipt"), "reservationHash") == get(&f, "reservationHash")
                && get(get(e, "receipt"), "amount") == get(&f, "reservedCash")
                && hex(get(get(e, "receipt"), "globalLedgerHash"))
                && instant_v(get(get(e, "receipt"), "confirmedAt"))? <= instant_v(get(e, "at"))?,
            "FUNDING_RECEIPT",
        )?;
        n["installedFunding"] = get(e, "receipt").clone();
        return Ok(n);
    }
    if get(e, "kind") == "prepared" {
        need(active(s).is_none(), "ACTIVE_INTENT")?;
        let v = preflight(s, get(e, "evidence"), instant_v(get(e, "at"))? as f64, None)?;
        need(
            js_int(get(e, "index")) == Some(v.0)
                && get(e, "intentId").is_string()
                && get(e, "idempotencyKey").is_string()
                && !orders_of(s).iter().any(|x| {
                    get(x, "intentId") == get(e, "intentId")
                        || get(get(x, "body"), "idempotencyKey") == get(e, "idempotencyKey")
                }),
            "INTENT_IDENTITY",
        )?;
        let body = json!({
            "idempotencyKey": get(e, "idempotencyKey"),
            "exchangeId": get(&v.1, "exchangeId"),
            "side": "no",
            "action": "buy",
            "quantity": 1,
            "price": money(v.2).parse::<f64>().unwrap_or(f64::NAN),
            "expirationDate": ms_to_iso(
                std::cmp::min(instant_v(get(e, "at"))? + 30_000, instant_v(get(get(s, "plan"), "reviewExpiresAt"))?) as f64
            ),
            "tournamentId": get(get(get(s, "plan"), "scope"), "tournamentId"),
        });
        if get(&n, "externalPositions").is_null() {
            n["externalPositions"] = json!(v.4);
        }
        let order = json!({
            "index": v.0,
            "intentId": get(e, "intentId"),
            "marketId": get(&v.1, "marketId"),
            "createdAt": get(e, "at"),
            "body": body,
            "bodyJson": canonical_payload(&body)?,
            "bodyHash": payload_hash(&body)?,
            "before": get(get(e, "evidence"), "account"),
            "reservedElsewhere": get(get(e, "evidence"), "reservedElsewhere")
                .clone()
                .pipe_null_default(),
            "book": v.3,
            "phase": "prepared",
            "orderId": V::Null,
            "ack": V::Null,
            "wireAttempts": [],
            "fetchStarts": [],
            "fences": [],
            "semanticMismatch": false,
            "cancelPending": false,
            "proof": V::Null,
        });
        n["orders"].as_array_mut().unwrap().push(order);
        return Ok(n);
    }
    need(
        o_exists && get(&o, "phase") != "reconciled",
        "NO_ACTIVE_ORDER",
    )?;
    match get(e, "kind").as_str() {
        Some("admitted") => {
            let op = get(e, "operation").as_str().unwrap_or("");
            if op == "place" {
                need(get(&o, "phase") == "prepared", "PLACEMENT_PHASE")?;
            } else if op == "recover" {
                need(
                    ["uncertain", "dispatching"].contains(&get(&o, "phase").as_str().unwrap_or(""))
                        && !js_truthy(o.get("orderId"))
                        && !js_truthy(o.get("semanticMismatch"))
                        && instant_v(get(e, "at"))? >= instant_v(get(get(&o, "body"), "expirationDate"))?
                        && get(&o, "fetchStarts").as_array().is_some_and(|ws| {
                            ws.iter().any(|w| {
                                get(w, "kind") == "place"
                                    && instant_v(get(w, "requestStartedAt"))
                                        .map(|t| t < instant_v(get(get(&o, "body"), "expirationDate")).unwrap_or(i64::MAX))
                                        .unwrap_or(false)
                            })
                        }),
                    "RECOVERY_PHASE",
                )?;
            } else {
                need(
                    op == "cancel"
                        && js_truthy(o.get("orderId"))
                        && instant_v(get(e, "at"))? >= instant_v(get(get(&o, "body"), "expirationDate"))?
                        && !js_truthy(o.get("cancelPending")),
                    "CANCEL_PHASE",
                )?;
            }
            o["operation"] = json!(op);
            if op == "cancel" {
                o["cancelPending"] = json!(true);
            } else {
                o["phase"] = json!("dispatching");
            }
            n["orders"].as_array_mut().unwrap()[idx] = o;
            Ok(n)
        }
        Some("network_fence") => {
            need(
                get(&o, "operation") == get(e, "operation")
                    && hex(get(e, "leaseHash"))
                    && get(e, "leaseHash") == &json!(payload_hash(get(e, "leaseIdentity"))?),
                "NETWORK_FENCE",
            )?;
            o["fences"].as_array_mut().unwrap().push(json!({
                "operation": get(e, "operation"),
                "at": get(e, "at"),
                "leaseIdentity": get(e, "leaseIdentity"),
                "leaseHash": get(e, "leaseHash"),
            }));
            n["orders"].as_array_mut().unwrap()[idx] = o;
            Ok(n)
        }
        Some("fetch_start") => {
            let w = get(e, "start");
            let l = get(&o, "fences").as_array().and_then(|f| f.last().cloned());
            let expected_endpoint = if get(w, "kind") == "cancel" {
                endpoint_of_order(&o)
            } else {
                "/orders".to_string()
            };
            need(
                l.is_some()
                    && get(w, "kind") == get(&o, "operation")
                    && get(w, "ownerId") == get(get(get(s, "plan"), "scope"), "ownerId")
                    && get(w, "runId") == get(get(get(s, "plan"), "scope"), "runId")
                    && get(w, "intentId") == get(&o, "intentId")
                    && get(w, "idempotencyKey") == get(get(&o, "body"), "idempotencyKey")
                    && get(w, "requestBodyJson") == get(&o, "bodyJson")
                    && get(w, "requestBodyHash") == get(&o, "bodyHash")
                    && get(w, "leaseHash") == get(&l.clone().unwrap_or(V::Null), "leaseHash")
                    && get(&l.clone().unwrap_or(V::Null), "leaseHash")
                        == &json!(payload_hash(get(w, "leaseIdentity"))?)
                    && get(w, "fetchInvocationId").as_str().is_some_and(|s| !s.is_empty())
                    && !orders_of(&n).iter().any(|v| {
                        get(v, "fetchStarts").as_array().is_some_and(|fs| {
                            fs.iter().any(|x| {
                                get(x, "fetchInvocationId") == get(w, "fetchInvocationId")
                            })
                        })
                    })
                    && instant_v(get(w, "requestStartedAt"))? >= instant_v(get(&l.clone().unwrap_or(V::Null), "at"))?
                    && instant_v(get(w, "requestStartedAt"))? <= instant_v(get(e, "at"))?
                    && get(w, "method").as_str()
                        == Some(if get(w, "kind") == "cancel" { "DELETE" } else { "POST" })
                    && get(w, "endpoint").as_str() == Some(expected_endpoint.as_str()),
                "FETCH_START_BINDING",
            )?;
            if get(w, "kind") == "place" {
                need(
                    instant_v(get(w, "requestStartedAt"))?
                        < instant_v(get(get(&o, "body"), "expirationDate"))?,
                    "ORIGINAL_WIRE_EXPIRED",
                )?;
            }
            o["fetchStarts"].as_array_mut().unwrap().push(w.clone());
            n["orders"].as_array_mut().unwrap()[idx] = o;
            Ok(n)
        }
        Some("wire") => {
            let w = get(e, "wire");
            let started = get(&o, "fetchStarts").as_array().and_then(|f| f.last().cloned());
            let last_fence = get(&o, "fences").as_array().and_then(|f| f.last().cloned());
            need(
                started.is_some()
                    && get(w, "fetchInvocationId") == get(&started.clone().unwrap_or(V::Null), "fetchInvocationId")
                    && get(w, "requestStartedAt") == get(&started.clone().unwrap_or(V::Null), "requestStartedAt")
                    && get(w, "kind") == get(&o, "operation")
                    && get(w, "ownerId") == get(get(get(s, "plan"), "scope"), "ownerId")
                    && get(w, "runId") == get(get(get(s, "plan"), "scope"), "runId")
                    && get(w, "intentId") == get(&o, "intentId")
                    && get(w, "idempotencyKey") == get(get(&o, "body"), "idempotencyKey")
                    && get(w, "requestBodyJson") == get(&o, "bodyJson")
                    && get(w, "requestBodyHash") == get(&o, "bodyHash")
                    && get(w, "leaseHash") == get(&last_fence.clone().unwrap_or(V::Null), "leaseHash")
                    && instant_v(get(w, "requestStartedAt"))?
                        >= instant_v(get(&last_fence.clone().unwrap_or(V::Null), "at"))?
                    && instant_v(get(w, "receivedAt"))? >= instant_v(get(w, "requestStartedAt"))?
                    && instant_v(get(w, "receivedAt"))? <= instant_v(get(e, "at"))?
                    && hex(get(w, "sourceBodyHash"))
                    && sha(get(w, "bodyText").as_str().unwrap_or("")) == get(w, "sourceBodyHash").as_str().unwrap_or(""),
                "WIRE_PROVENANCE",
            )?;
            if get(w, "kind") == "place" {
                need(
                    instant_v(get(w, "requestStartedAt"))?
                        < instant_v(get(get(&o, "body"), "expirationDate"))?,
                    "ORIGINAL_WIRE_EXPIRED",
                )?;
            }
            o["wireAttempts"].as_array_mut().unwrap().push(w.clone());
            if get(w, "kind") == "cancel" {
                o["cancelPending"] = json!(true);
                n["orders"].as_array_mut().unwrap()[idx] = o;
                return Ok(n);
            }
            if js_int(get(w, "status")).is_some_and(|s| (200..300).contains(&s)) {
                let d = parse_number_lexemes(get(w, "bodyText").as_str().unwrap_or(""))
                    .map(|(d, _)| d)
                    .unwrap_or(V::Null);
                if js_int(get(&d, "orderId")).is_some_and(|i| i > 0) {
                    need(
                        !js_truthy(o.get("orderId"))
                            || get(&o, "orderId").as_str()
                                == Some(crate::js_key(get(&d, "orderId")).as_str()),
                        "ORDER_ID_CHANGED",
                    )?;
                    o["orderId"] = json!(crate::js_key(get(&d, "orderId")));
                }
                o["ack"] = json!({
                    "method": "POST",
                    "endpoint": "/orders",
                    "status": get(w, "status"),
                    "requestStartedAt": get(w, "requestStartedAt"),
                    "receivedAt": get(w, "receivedAt"),
                    "bodyText": get(w, "bodyText"),
                    "sourceBodyHash": get(w, "sourceBodyHash"),
                    "requestBodyHash": get(&o, "bodyHash"),
                });
                let ack = o["ack"].clone();
                let mismatch = !canonical_ack_impl(&o, &ack);
                o["semanticMismatch"] = json!(mismatch);
                o["phase"] = json!(if mismatch { "semantic_mismatch" } else { "acknowledged" });
            } else {
                o["phase"] = json!("uncertain");
            }
            n["orders"].as_array_mut().unwrap()[idx] = o;
            Ok(n)
        }
        Some("error") => {
            if get(&o, "phase") == "dispatching" {
                o["phase"] = json!("uncertain");
            }
            o["lastError"] = get(e, "code").clone();
            n["orders"].as_array_mut().unwrap()[idx] = o;
            Ok(n)
        }
        Some("reconciled") => {
            need(
                js_truthy(o.get("orderId"))
                    && !get(&o, "ack").is_null()
                    && !js_truthy(o.get("semanticMismatch")),
                "RECONCILIATION_BINDING",
            )?;
            keys(
                get(e, "evidence"),
                &["order", "fillPages", "after", "globalRisk", "accountActivity"],
            )?;
            account_shape(get(get(e, "evidence"), "after"))?;
            raw(
                get(get(e, "evidence"), "order"),
                "GET",
                &format!("/orders/{}", get(&o, "orderId").as_str().unwrap_or("")),
            )?;
            for r in get(get(e, "evidence"), "fillPages").as_array().cloned().unwrap_or_default() {
                raw(&r, "GET", get(&r, "endpoint").as_str().unwrap_or(""))?;
            }
            // {...e.evidence, binding, before, ack, recoveryProof, readAt,
            //  fullPairReservation, reservedElsewhere} — preserve spread order.
            let mut verify_input = get(e, "evidence").clone();
            if let V::Object(m) = &mut verify_input {
                m.insert("binding".into(), binding(s, &o, None)?);
                m.insert("before".into(), get(&o, "before").clone());
                m.insert("ack".into(), get(&o, "ack").clone());
                if let Some(rp) = e.get("recoveryProof") {
                    m.insert("recoveryProof".into(), rp.clone());
                }
                m.insert("readAt".into(), get(e, "at").clone());
                m.insert(
                    "fullPairReservation".into(),
                    get(&funding(s)?, "reservedCash").clone(),
                );
                m.insert("reservedElsewhere".into(), get(&o, "reservedElsewhere").clone());
            }
            let proof = verify_scoped_rest_no_buy_v1(&verify_input)?;
            need(
                get(&proof, "scopedPrincipalCost").as_str()
                    == Some(money(amount_v(get(&proof, "scopedPrincipalCost"))?).as_str())
                    && cost(s)? + amount_v(get(&proof, "scopedPrincipalCost"))?
                        <= amount_v(get(get(s, "plan"), "pairCostCeiling"))?
                    && cost(s)? + amount_v(get(&proof, "scopedPrincipalCost"))? <= 2_000_000,
                "PAIR_PRINCIPAL",
            )?;
            o["proof"] = proof;
            o["phase"] = json!("reconciled");
            o["cancelPending"] = json!(false);
            n["orders"].as_array_mut().unwrap()[idx] = o;
            Ok(n)
        }
        _ => Err("PAIRED_REST_EVENT".into()),
    }
}

pub(crate) fn orders_of(s: &V) -> Vec<V> {
    get(s, "orders").as_array().cloned().unwrap_or_default()
}

pub(crate) trait PipeNull {
    fn pipe_null_default(self) -> V;
}
impl PipeNull for V {
    // reservedElsewhere ?? '0.000000' — absent or null defaults.
    fn pipe_null_default(self) -> V {
        if self.is_null() {
            json!("0.000000")
        } else {
            self
        }
    }
}

pub(crate) fn endpoint_of_order(o: &V) -> String {
    format!("/orders/{}", crate::js_key(get(o, "orderId")))
}
pub(crate) fn ote(code: impl Into<String>) -> OrderTransportError {
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
pub(crate) fn guard_key(g: Option<&Guard<'_>>) -> usize {
    g.map(|g| g as *const Guard<'_> as *const () as usize).unwrap_or(0)
}

/// The serialized dispatcher the caller supplies: spec -> response; failures may
/// carry the wire receipt the API layer observed (JS error.wireReceipt).
pub struct PairedRestDispatchFailure {
    pub code: String,
    pub wire_receipt: Option<V>,
}
pub type PairedRestDispatch = dyn Fn(&V) -> Result<V, PairedRestDispatchFailure>;

pub struct PairedRestControllerOptions {
    /// Absolute data dir (JS `dir`).
    pub dir: PathBuf,
    /// The original capture object (may be absent on reopen).
    pub capture: Option<V>,
    pub scope: V,
    /// Repository root containing execution/ — replaces import.meta.url for
    /// codeIdentity().
    pub workspace_dir: PathBuf,
    pub dispatch: Rc<PairedRestDispatch>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub assert_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub install_funding_reservation: Rc<dyn Fn(&V) -> Result<V, String>>,
    /// Synchronous LOCAL cache read — issuing another request from the queue's
    /// own admission callback would deadlock the shared client.
    pub get_evidence: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub raw_receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub now: Rc<dyn Fn() -> f64>,
}

pub(crate) struct IssuedSpec {
    pub(crate) endpoint: String,
    pub(crate) method: String,
    pub(crate) body_json: V,
    pub(crate) before_network: usize,
}

pub(crate) struct Inner {
    pub(crate) state: V,
    pub(crate) closed: bool,
    pub(crate) failed: bool,
    pub(crate) busy: bool,
    pub(crate) size: usize,
    pub(crate) bytes_hash: String,
    pub(crate) file_ino: u64,
    pub(crate) file_birth: (i64, i64),
    pub(crate) rows: Vec<V>,
    pub(crate) journal: Journal,
    pub(crate) lease: OwnerLease,
    pub(crate) in_flight: Option<(usize, V)>,
    pub(crate) issued: HashMap<usize, IssuedSpec>,
}

pub struct PairedRestCore {
    pub(crate) inner: RefCell<Inner>,
    pub(crate) file: PathBuf,
    pub(crate) workspace_dir: PathBuf,
    pub(crate) entry: &'static str,
    pub(crate) code: V,
    pub(crate) scope: V,
    pub(crate) reducer: fn(&V, &V) -> Result<V, String>,
    pub(crate) dispatch: Rc<PairedRestDispatch>,
    pub(crate) can_operate: Rc<dyn Fn() -> bool>,
    pub(crate) assert_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub(crate) install_funding_reservation: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub(crate) get_evidence: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub(crate) raw_receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub(crate) now: Rc<dyn Fn() -> f64>,
}

impl PairedRestCore {
    pub(crate) fn state(&self) -> V {
        self.inner.borrow().state.clone()
    }
    pub(crate) fn basic(&self) -> Result<V, String> {
        {
            let i = self.inner.borrow();
            need(
                !i.closed && !i.failed && (self.can_operate)(),
                "STOPPED_OR_FAILED",
            )?;
        }
        self.inner
            .borrow()
            .lease
            .assert_ownership()
            .map_err(|e: baal_service_runtime::ledger::LedgerError| e.code.to_string())?;
        let l = (self.assert_account_lease)()?;
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
    pub(crate) fn recovery_member(&self, input: &V) -> Result<(), String> {
        let i = self.inner.borrow();
        if js_truthy(input.get("recoveryProof")) {
            let r = get(get(input, "recoveryProof"), "journalRecord");
            let rh = payload_hash(r).unwrap_or_default();
            need(
                js_truthy(Some(r))
                    && i.rows.iter().any(|v| {
                        js_int(get(v, "sequence")) == js_int(get(r, "sequence"))
                            && get(v, "hash") == get(r, "hash")
                            && payload_hash(v).unwrap_or_default() == rh
                    }),
                "RECOVERY_JOURNAL_MEMBERSHIP",
            )?;
        }
        Ok(())
    }
    pub(crate) fn append(&self, input: &V) -> Result<(), String> {
        self.basic()?;
        let (ino, birth, size, bytes_hash) = {
            let i = self.inner.borrow();
            (i.file_ino, i.file_birth, i.size, i.bytes_hash.clone())
        };
        let before = fs::read(&self.file).map_err(|e| e.to_string())?;
        let current = fs::metadata(&self.file).map_err(|e| e.to_string())?;
        need(
            current.ino() == ino
                && birthtime(&current) == birth
                && before.len() == size
                && sha256(&before) == bytes_hash
                && payload_hash(&code_identity(&self.workspace_dir, self.entry)?)?
                    == payload_hash(&self.code)?,
            "JOURNAL_CHANGED",
        )?;
        self.recovery_member(input)?;
        let next = (self.reducer)(&self.state(), input)?;
        need(
            size + js_stringify(&next).len() + js_stringify(input).len() < 64 * 1024 * 1024,
            "JOURNAL_BOUND",
        )?;
        let result = (|| -> Result<(), String> {
            {
                let mut i = self.inner.borrow_mut();
                i.journal
                    .commit(&next, &json!({"codeIdentity": self.code.clone(), "input": input.clone()}))?;
            }
            self.basic()?;
            let all = fs::read(&self.file).map_err(|e| e.to_string())?;
            need(
                all[..size] == before[..] && all.len() > size,
                "JOURNAL_CHANGED",
            )?;
            let tail: V = serde_json::from_str(&String::from_utf8_lossy(&all[size..]))
                .map_err(|e| e.to_string())?;
            let mut i = self.inner.borrow_mut();
            i.rows.push(tail);
            i.state = next;
            i.size = all.len();
            i.bytes_hash = sha256(&all);
            Ok(())
        })();
        if result.is_err() {
            self.inner.borrow_mut().failed = true;
        }
        result
    }
    // reserve()
    pub(crate) fn reserve(&self) -> Result<V, String> {
        self.basic()?;
        let wanted = funding(&self.state())?;
        let r = (self.install_funding_reservation)(&wanted)?;
        self.basic()?;
        self.append(&json!({
            "kind": "funding",
            "receipt": r,
            "at": ms_to_iso((self.now)()),
        }))?;
        Ok(r)
    }
    // evidence(operation, o)
    pub(crate) fn evidence(&self, operation: &str, o: Option<&V>) -> Result<V, String> {
        let state = self.state();
        let legs_len = orders_of(&state).len();
        let next_leg = get(get(&state, "plan"), "legs")
            .as_array()
            .and_then(|l| l.get(legs_len).cloned())
            .unwrap_or(V::Null);
        let market = o
            .and_then(|o| o.get("marketId"))
            .filter(|v| !v.is_null())
            .cloned()
            .or_else(|| next_leg.get("marketId").cloned());
        let exchange = o
            .and_then(|o| o.get("body"))
            .and_then(|b| b.get("exchangeId"))
            .filter(|v| !v.is_null())
            .cloned()
            .or_else(|| next_leg.get("exchangeId").cloned());
        let order_id = o
            .and_then(|o| o.get("orderId"))
            .filter(|v| !v.is_null())
            .cloned()
            .unwrap_or(V::Null);
        let mut ctx = Map::new();
        ctx.insert("operation".into(), json!(operation));
        if let Some(m) = market {
            ctx.insert("marketId".into(), m);
        }
        if let Some(x) = exchange {
            ctx.insert("exchangeId".into(), x);
        }
        ctx.insert("orderId".into(), order_id);
        ctx.insert("reservation".into(), funding(&state)?);
        let e = (self.get_evidence)(&V::Object(ctx))?;
        need(js_truthy(Some(&e)), "EVIDENCE_MUST_BE_LOCAL")?;
        self.basic()?;
        Ok(e)
    }
    // own(id)
    pub(crate) fn own(&self, id: &str) -> bool {
        orders_of(&self.state())
            .iter()
            .any(|o| get(o, "orderId").as_str() == Some(id))
    }
    // recoverRow(o)
    pub(crate) fn recover_row(&self, o: &V) -> Result<V, String> {
        let inner = self.inner.borrow();
        let expiration = instant_v(get(get(o, "body"), "expirationDate"))?;
        let mut found: Option<V> = None;
        for r in &inner.rows {
            let input = get(get(r, "event"), "input");
            if get(input, "kind").as_str() == Some("fetch_start")
                && js_int(get(input, "index")) == js_int(get(o, "index"))
                && get(get(input, "start"), "kind").as_str() == Some("place")
                && instant_v(get(get(input, "start"), "requestStartedAt"))? < expiration
            {
                found = Some(r.clone());
                break;
            }
        }
        need(
            found.is_some()
                && get(get(get(get(found.as_ref().unwrap(), "event"), "input"), "start"), "requestBodyHash")
                    == get(o, "bodyHash")
                && get(get(get(get(found.as_ref().unwrap(), "event"), "input"), "start"), "requestBodyJson")
                    == get(o, "bodyJson"),
            "ORIGINAL_WIRE_PROOF_MISSING",
        )?;
        Ok(found.unwrap())
    }
    // cancelGate(o, e, at)
    pub(crate) fn cancel_gate(&self, o: &V, e: &V, at: f64) -> Result<(), String> {
        need(
            js_truthy(o.get("orderId"))
                && at >= instant_v(get(get(o, "body"), "expirationDate"))? as f64,
            "CANCEL_NOT_EXPIRED_OWN",
        )?;
        let r = raw(get(e, "order"), "GET", &endpoint_of_order(o))?;
        let (d, _n) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
        need(
            js_int(get(&r, "status")) == Some(200)
                && instant_v(get(&r, "receivedAt"))? as f64 <= at
                && at - instant_v(get(&r, "requestStartedAt"))? as f64 <= 20_000.0
                && crate::js_key(get(&d, "id")) == get(o, "orderId").as_str().unwrap_or("")
                && get(&d, "tournamentId") == get(&self.scope, "tournamentId")
                && get(&d, "exchangeId") == get(get(o, "body"), "exchangeId")
                && get(&d, "side").as_str() == Some("no")
                && get(&d, "action").as_str() == Some("buy")
                && js_int(get(&d, "quantity")) == Some(1)
                && get(&d, "open") == &json!(true)
                && price(&crate::js_key(get(&d, "priceLimit")))?
                    == price(get(&binding(&self.state(), o, None)?, "limit").as_str().unwrap_or(""))?
                && get(&d, "expirationDate") == get(get(o, "body"), "expirationDate"),
            "CANCEL_ORDER_PROOF",
        )?;
        Ok(())
    }
    // validate(p, spec, network) — the transport Guard surface.
    pub(crate) fn validate(&self, p: &V, spec: &V, network: bool) -> Result<bool, OrderTransportError> {
        (|| -> Result<(), String> {
            self.basic()?;
            let state = self.state();
            let o = active(&state);
            need(
                o.is_some()
                    && get(p, "intentId") == get(o.as_ref().unwrap(), "intentId")
                    && get(p, "runId") == get(&self.scope, "runId")
                    && get(p, "bodyHash") == get(o.as_ref().unwrap(), "bodyHash"),
                "PERMIT_IDENTITY",
            )?;
            let o = o.unwrap();
            let now = (self.now)();
            if get(p, "operation").as_str() == Some("cancel") {
                need(
                    get(spec, "method").as_str() == Some("DELETE")
                        && get(spec, "endpoint").as_str() == Some(endpoint_of_order(&o).as_str()),
                    "CANCEL_SCOPE",
                )?;
                let e = self.evidence("cancel", Some(&o))?;
                self.cancel_gate(&o, &e, now)?;
            } else {
                need(
                    get(spec, "method").as_str() == Some("POST")
                        && get(spec, "endpoint").as_str() == Some("/orders")
                        && get(spec, "bodyJson") == get(&o, "bodyJson"),
                    "PLACEMENT_PAYLOAD",
                )?;
                if get(p, "operation").as_str() == Some("recover") {
                    need(
                        !js_truthy(o.get("orderId"))
                            && !js_truthy(o.get("semanticMismatch"))
                            && now >= instant_v(get(get(&o, "body"), "expirationDate"))? as f64,
                        "RECOVERY_NOT_ALLOWED",
                    )?;
                    self.recover_row(&o)?;
                } else {
                    let e = self.evidence("place", Some(&o))?;
                    preflight(&state, &e, now, Some(&o))?;
                }
            }
            let l = self.basic()?;
            if network {
                self.append(&json!({
                    "kind": "network_fence",
                    "index": get(&o, "index"),
                    "operation": get(p, "operation"),
                    "leaseIdentity": l,
                    "leaseHash": payload_hash(&l)?,
                    "at": ms_to_iso((self.now)()),
                }))?;
                if get(p, "operation").as_str() == Some("place") {
                    let state2 = self.state();
                    let cur = active(&state2).unwrap_or(V::Null);
                    let e = self.evidence("place", Some(&cur))?;
                    preflight(&state2, &e, (self.now)(), Some(&cur))?;
                } else if get(p, "operation").as_str() == Some("cancel") {
                    let state2 = self.state();
                    let cur = active(&state2).unwrap_or(V::Null);
                    let e = self.evidence("cancel", Some(&cur))?;
                    self.cancel_gate(&cur, &e, (self.now)())?;
                }
                self.basic()?;
            } else {
                self.append(&json!({
                    "kind": "admitted",
                    "index": get(&o, "index"),
                    "operation": get(p, "operation"),
                    "at": ms_to_iso((self.now)()),
                }))?;
            }
            Ok(())
        })()
        .map(|_| true)
        .map_err(ote)
    }
    // captureWire(index, spec, r) — the wire journal event for a returned
    // response or an error's wire receipt.
    pub(crate) fn capture_wire(&self, index: usize, spec: &V, r: &V) -> Result<(), String> {
        let body_hash = if get(spec, "method").as_str() == Some("POST") {
            let state = self.state();
            let o = orders_of(&state).get(index).cloned().unwrap_or(V::Null);
            get(&o, "bodyHash").clone()
        } else {
            V::Null
        };
        let record = raw(
            &(self.raw_receipt_of)(
                r,
                &json!({"method": get(spec, "method"), "requestBodyHash": body_hash}),
            )?,
            get(spec, "method").as_str().unwrap_or(""),
            get(spec, "endpoint").as_str().unwrap_or(""),
        )?;
        need(
            instant_v(get(&record, "requestStartedAt"))? >= instant_v(get(r, "requestStartedAt"))?
                && instant_v(get(&record, "receivedAt"))? <= instant_v(get(r, "receivedAt"))?
                && js_int(get(&record, "status")) == js_int(get(r, "status")),
            "WIRE_CLOCK_RELABELED",
        )?;
        if r.get("data").is_some() {
            need(
                payload_hash(&parse_number_lexemes(get(&record, "bodyText").as_str().unwrap_or(""))?.0)?
                    == payload_hash(get(r, "data"))?,
                "WIRE_BODY_CHANGED",
            )?;
        }
        let state = self.state();
        let current = orders_of(&state).get(index).cloned().unwrap_or(V::Null);
        let l = get(&current, "fences").as_array().and_then(|f| f.last().cloned());
        need(l.is_some(), "NETWORK_FENCE_MISSING")?;
        let l = l.unwrap();
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
        self.append(&json!({
            "kind": "wire",
            "index": index,
            "wire": wire,
            "at": ms_to_iso((self.now)()),
        }))?;
        Ok(())
    }
    // wrap(spec) — the OrderTransport dispatch surface: registers the issued
    // guard pair, fires the supplied fences (the serialized client invokes them
    // at queue admission and immediately before the socket write), then calls
    // the caller's dispatch.
    pub(crate) fn wrap(
        &self,
        spec: &V,
        before_dispatch: Option<&Guard<'_>>,
        before_network: Option<&Guard<'_>>,
    ) -> Result<V, OrderTransportError> {
        let state = self.state();
        let o = active(&state).ok_or_else(|| {
            ote("TypeError: Cannot read properties of null (reading 'index')")
        })?;
        let index = js_int(get(&o, "index")).unwrap_or(0) as usize;
        let bd_key = guard_key(before_dispatch);
        let bn_key = guard_key(before_network);
        {
            let mut i = self.inner.borrow_mut();
            i.issued.insert(
                bd_key,
                IssuedSpec {
                    endpoint: get(spec, "endpoint").as_str().unwrap_or("").to_string(),
                    method: get(spec, "method").as_str().unwrap_or("").to_string(),
                    body_json: spec.get("bodyJson").cloned().unwrap_or(V::Null),
                    before_network: bn_key,
                },
            );
            i.in_flight = Some((index, spec.clone()));
        }
        let result = (|| -> Result<V, OrderTransportError> {
            if let Some(g) = before_dispatch {
                g(&V::Null, spec)?;
            }
            if let Some(g) = before_network {
                g(&V::Null, spec)?;
            }
            match (self.dispatch)(spec) {
                Ok(r) => {
                    self.capture_wire(index, spec, &r).map_err(ote)?;
                    Ok(r)
                }
                Err(f) => {
                    if let Some(wr) = &f.wire_receipt {
                        self.capture_wire(index, spec, wr).map_err(ote)?;
                    }
                    Err(OrderTransportError {
                        code: f.code,
                        outcome: "unknown".into(),
                        ..ote("")
                    })
                }
            }
        })();
        {
            let mut i = self.inner.borrow_mut();
            i.issued.remove(&bd_key);
            i.in_flight = None;
        }
        result
    }
    // onFetchStart(meta) — the scoped client's write-admission hook.
    pub fn on_fetch_start(&self, meta: &V) -> Result<bool, String> {
        if get(meta, "method").as_str() == Some("GET") {
            return Ok(false);
        }
        let flight = self.inner.borrow().in_flight.clone();
        let Some((index, ispec)) = flight else {
            return Ok(false);
        };
        let r = (|| -> Result<bool, String> {
            self.basic()?;
            keys(
                meta,
                &[
                    "fetchInvocationId",
                    "method",
                    "endpoint",
                    "requestBodyHash",
                    "requestStartedAt",
                ],
            )?;
            let state = self.state();
            let o = orders_of(&state).get(index).cloned().unwrap_or(V::Null);
            let l = get(&o, "fences").as_array().and_then(|f| f.last().cloned());
            let expected_hash = if get(meta, "method").as_str() == Some("POST") {
                get(&o, "bodyHash").clone()
            } else {
                V::Null
            };
            need(
                l.is_some()
                    && get(meta, "method") == get(&ispec, "method")
                    && get(meta, "endpoint") == get(&ispec, "endpoint")
                    && meta.get("requestBodyHash") == Some(&expected_hash),
                "FETCH_START_UNBOUND",
            )?;
            let l = l.unwrap();
            let mut start = meta.clone();
            if let V::Object(m) = &mut start {
                m.insert("kind".into(), get(&o, "operation").clone());
                m.insert("ownerId".into(), get(&self.scope, "ownerId").clone());
                m.insert("runId".into(), get(&self.scope, "runId").clone());
                m.insert("intentId".into(), get(&o, "intentId").clone());
                m.insert(
                    "idempotencyKey".into(),
                    get(get(&o, "body"), "idempotencyKey").clone(),
                );
                m.insert("requestBodyJson".into(), get(&o, "bodyJson").clone());
                m.insert("requestBodyHash".into(), get(&o, "bodyHash").clone());
                m.insert("leaseIdentity".into(), get(&l, "leaseIdentity").clone());
                m.insert("leaseHash".into(), get(&l, "leaseHash").clone());
            }
            self.append(&json!({
                "kind": "fetch_start",
                "index": get(&o, "index"),
                "start": start,
                "at": ms_to_iso((self.now)()),
            }))?;
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
    // run(fn) — busy gate.
    pub(crate) fn run<T>(&self, f: impl FnOnce(&Self) -> Result<T, String>) -> Result<T, String> {
        need(!self.inner.borrow().busy, "BUSY")?;
        self.inner.borrow_mut().busy = true;
        let r = f(self);
        self.inner.borrow_mut().busy = false;
        r
    }
}

pub struct PairedRestController {
    core: Rc<PairedRestCore>,
    transport: OrderTransport,
}

fn reduce_default(s: &V, e: &V, legacy: bool) -> Result<V, String> {
    debug_assert!(!legacy);
    reduce(s, e)
}

/// Shared openPairedRestController/openPairedRestControllerV3 body. `entry` is
/// the module whose import closure becomes this revision's codeIdentity;
/// `replay_reducer` receives (state, input, legacy); `neutral_ack_revision`
/// enables the v3 revision-prefix replay + descriptor injection.
pub(crate) fn open_controller(
    opts: PairedRestControllerOptions,
    entry: &'static str,
    reducer: fn(&V, &V) -> Result<V, String>,
    replay_reducer: fn(&V, &V, bool) -> Result<V, String>,
    neutral_ack_revision: Option<&V>,
) -> Result<PairedRestController, String> {
    need(opts.dir.is_absolute(), "DEPENDENCIES")?;
    let scope = opts.scope.clone();
    let mut local = acquire_owner_lease(
        &opts.dir,
        get(&scope, "ownerId").as_str().unwrap_or(""),
        get(&scope, "runId").as_str().unwrap_or(""),
        "paired-rest-lifecycle",
        (opts.now)(),
    )
    .map_err(|e: baal_service_runtime::ledger::LedgerError| e.code.to_string())?;
    let file = opts.dir.join("paired-rest-journal.jsonl");
    let code = code_identity(&opts.workspace_dir, entry)?;
    let legacy_code = code_identity(&opts.workspace_dir, "paired-rest-controller.mjs")?;
    // try { ... } catch (e) { local.release(); throw e; }
    let opened = (|| -> Result<(V, usize, String, u64, (i64, i64), Vec<V>, Journal), String> {
        if !file.exists() {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&file)
                .map_err(|e| e.to_string())?;
        }
        let bytes = fs::read(&file).map_err(|e| e.to_string())?;
        need(bytes.is_empty() || bytes.last() == Some(&10), "PARTIAL_JOURNAL")?;
        let journal = Journal::open(&file)?;
        if let Some(rev) = neutral_ack_revision {
            let original_prefix_bytes = js_int(get(rev, "originalPrefixBytes")).unwrap_or(-1);
            need(
                get(rev, "kind") == "same_run_neutral_all_ack_reconciliation"
                    && get(rev, "ownerId") == get(&scope, "ownerId")
                    && get(rev, "runId") == get(&scope, "runId")
                    && get(rev, "scopeHash") == &json!(payload_hash(&scope)?)
                    && original_prefix_bytes > 0
                    && original_prefix_bytes <= bytes.len() as i64
                    && sha256(&bytes[..original_prefix_bytes as usize])
                        == get(rev, "originalPrefixHash").as_str().unwrap_or(""),
                "REVISION_PREFIX_BINDING",
            )?;
        }
        let mut state = V::Null;
        let mut rows: Vec<V> = Vec::new();
        let mut prefix_seen: usize = 0;
        for line in String::from_utf8_lossy(&bytes).split('\n').filter(|l| !l.is_empty()) {
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            let row_bytes = line.len() + 1;
            let original_prefix_bytes = neutral_ack_revision
                .map(|rev| js_int(get(rev, "originalPrefixBytes")).unwrap_or(-1) as usize)
                .unwrap_or(0);
            let is_legacy =
                neutral_ack_revision.is_some() && prefix_seen < original_prefix_bytes;
            need(
                !is_legacy || prefix_seen + row_bytes <= original_prefix_bytes,
                "REVISION_PARTIAL_ROW",
            )?;
            need(
                payload_hash(get(get(&row, "event"), "codeIdentity"))?
                    == payload_hash(if is_legacy { &legacy_code } else { &code })?,
                "CODE_CHANGED",
            )?;
            let input = get(get(&row, "event"), "input");
            if get(input, "kind") == "neutral_ack_revision" {
                let rev = neutral_ack_revision;
                need(
                    !is_legacy
                        && rev.is_some()
                        && get(input, "revisionHash")
                            == &json!(payload_hash(rev.unwrap())?)
                        && get(input, "orderId") == get(rev.unwrap(), "orderId")
                        && get(input, "sourceBodyHash")
                            == get(rev.unwrap(), "ackSourceBodyHash")
                        && get(input, "originalPrefixHash")
                            == get(rev.unwrap(), "originalPrefixHash")
                        && get(input, "originalPrefixBytes")
                            == get(rev.unwrap(), "originalPrefixBytes"),
                    "REVISION_DESCRIPTOR_CHANGED",
                )?;
            }
            if js_truthy(input.get("recoveryProof")) {
                let r = get(get(input, "recoveryProof"), "journalRecord");
                let rh = payload_hash(r).unwrap_or_default();
                need(
                    js_truthy(Some(r))
                        && rows.iter().any(|v| {
                            js_int(get(v, "sequence")) == js_int(get(r, "sequence"))
                                && get(v, "hash") == get(r, "hash")
                                && payload_hash(v).unwrap_or_default() == rh
                        }),
                    "RECOVERY_JOURNAL_MEMBERSHIP",
                )?;
            }
            state = replay_reducer(&state, input, is_legacy)?;
            need(
                payload_hash(&state)? == payload_hash(get(&row, "state"))?,
                "REPLAY_STATE",
            )?;
            rows.push(row);
            prefix_seen += row_bytes;
        }
        let size = bytes.len();
        let bytes_hash = sha256(&bytes);
        let meta = fs::metadata(&file).map_err(|e| e.to_string())?;
        Ok((
            state,
            size,
            bytes_hash,
            meta.ino(),
            birthtime(&meta),
            rows,
            journal,
        ))
    })();
    let (state, size, bytes_hash, file_ino, file_birth, rows, journal) = match opened {
        Ok(x) => x,
        Err(e) => {
            let _ = local.release();
            return Err(e);
        }
    };
    let core = Rc::new(PairedRestCore {
        inner: RefCell::new(Inner {
            state: state.clone(),
            closed: false,
            failed: false,
            busy: false,
            size,
            bytes_hash,
            file_ino,
            file_birth,
            rows,
            journal,
            lease: local,
            in_flight: None,
            issued: HashMap::new(),
        }),
        file: file.clone(),
        workspace_dir: opts.workspace_dir.clone(),
        entry,
        code: code.clone(),
        scope: scope.clone(),
        reducer,
        dispatch: opts.dispatch.clone(),
        can_operate: opts.can_operate.clone(),
        assert_account_lease: opts.assert_account_lease.clone(),
        install_funding_reservation: opts.install_funding_reservation.clone(),
        get_evidence: opts.get_evidence.clone(),
        raw_receipt_of: opts.raw_receipt_of.clone(),
        now: opts.now.clone(),
    });
    // if (!state) append({kind:'initialize',capture,scope,at:iso(now())}) ...
    let post_open = (|| -> Result<OrderTransport, String> {
        if state.is_null() {
            let mut ev = Map::new();
            ev.insert("kind".into(), json!("initialize"));
            if let Some(c) = &opts.capture {
                ev.insert("capture".into(), c.clone());
            }
            ev.insert("scope".into(), scope.clone());
            ev.insert("at".into(), json!(ms_to_iso((opts.now)())));
            core.append(&V::Object(ev))?;
        } else {
            need(
                payload_hash(get(get(&state, "plan"), "scope"))? == payload_hash(&scope)?
                    && (opts.capture.is_none()
                        || get(get(&state, "plan"), "captureHash")
                            == &json!(payload_hash(opts.capture.as_ref().unwrap())?)),
                "SCOPE_OR_CAPTURE_CHANGED",
            )?;
        }
        // v3: append the neutral_ack_revision descriptor when missing.
        if let Some(rev) = neutral_ack_revision {
            let has_revision = core.inner.borrow().rows.iter().any(|r| {
                get(get(get(r, "event"), "input"), "kind") == "neutral_ack_revision"
            });
            if !has_revision {
                let o = active(&core.state());
                need(
                    o.is_some()
                        && get(o.as_ref().unwrap(), "orderId") == get(rev, "orderId")
                        && get(get(o.as_ref().unwrap(), "ack"), "sourceBodyHash")
                            == get(rev, "ackSourceBodyHash"),
                    "REVISION_ORDER_BINDING",
                )?;
                let proof = crate::no_neutral_all::inspect_no_ack_all_economics(get(
                    o.as_ref().unwrap(),
                    "ack",
                ))?;
                core.append(&json!({
                    "kind": "neutral_ack_revision",
                    "index": get(o.as_ref().unwrap(), "index"),
                    "orderId": get(o.as_ref().unwrap(), "orderId"),
                    "sourceBodyHash": get(get(o.as_ref().unwrap(), "ack"), "sourceBodyHash"),
                    "economicsProofHash": payload_hash(&proof)?,
                    "revisionHash": payload_hash(rev)?,
                    "originalPrefixHash": get(rev, "originalPrefixHash"),
                    "originalPrefixBytes": get(rev, "originalPrefixBytes"),
                    "at": ms_to_iso((opts.now)()),
                }))?;
            }
        }
        OrderTransport::new(OrderTransportOptions {
            dispatch: Box::new({
                let core = core.clone();
                move |spec: &V, bd: Option<&Guard<'_>>, bn: Option<&Guard<'_>>| {
                    core.wrap(spec, bd, bn)
                }
            }),
            scope: scope_of(&json!({
                "tournamentId": get(&scope, "tournamentId"),
                "tournamentSlug": get(&scope, "tournamentSlug"),
                "exchanges": get(&scope, "markets"),
            }))
            .map_err(|e| e.code)?,
            can_write: Box::new({
                let core = core.clone();
                move || core.basic().is_ok()
            }),
            owns_order: Box::new({
                let core = core.clone();
                move |id: &str| core.own(id)
            }),
            validate_permit: Box::new({
                let core = core.clone();
                move |p: &V, spec: &V| core.validate(p, spec, false)
            }),
            validate_lease: Box::new({
                let core = core.clone();
                move |lease: &V, _spec: &V| {
                    (|| -> Result<bool, String> {
                        Ok(payload_hash(lease)? == payload_hash(&core.basic()?)?)
                    })()
                    .map_err(ote)
                }
            }),
            validate_network_permit: Box::new({
                let core = core.clone();
                move |p: &V, spec: &V| core.validate(p, spec, true)
            }),
            now: Box::new({
                let now = opts.now.clone();
                move || now() as i64
            }),
            max_pages: 100,
        })
        .map_err(|e| e.code.to_string())
    })();
    match post_open {
        Ok(transport) => Ok(PairedRestController { core, transport }),
        Err(e) => {
            let _ = core.inner.borrow_mut().lease.release();
            Err(e)
        }
    }
}

/// openPairedRestController({dir, capture, scope, dispatch, canOperate,
/// assertAccountLease, installFundingReservation, getEvidence, rawReceiptOf,
/// now, workspaceDir})
pub fn open_paired_rest_controller(
    opts: PairedRestControllerOptions,
) -> Result<PairedRestController, String> {
    open_controller(opts, "paired-rest-controller.mjs", reduce, reduce_default, None)
}

impl PairedRestController {
    /// authorizes(spec) — fence-key version of the WeakMap membership check.
    pub fn authorizes(&self, spec: &V) -> bool {
        (|| -> Result<bool, String> {
            self.core.basic()?;
            let i = self.core.inner.borrow();
            let p = spec
                .get("beforeDispatch")
                .and_then(V::as_u64)
                .and_then(|id| i.issued.get(&(id as usize)));
            let Some(p) = p else {
                return Ok(false);
            };
            Ok(get(spec, "method").as_str() == Some(p.method.as_str())
                && get(spec, "endpoint").as_str() == Some(p.endpoint.as_str())
                && &p.body_json == spec.get("bodyJson").unwrap_or(&V::Null)
                && spec.get("beforeNetwork").and_then(V::as_u64).map(|x| x as usize)
                    == Some(p.before_network)
                && self.transport.authorize_request(spec).unwrap_or(false))
        })()
        .unwrap_or(false)
    }
    /// onFetchStart(meta)
    pub fn on_fetch_start(&self, meta: &V) -> Result<bool, String> {
        self.core.on_fetch_start(meta)
    }
    /// status()
    pub fn status(&self) -> Result<V, String> {
        self.core.status()
    }
}

impl PairedRestCore {
    pub(crate) fn status(&self) -> Result<V, String> {
        let state = self.state();
        let i = self.inner.borrow();
        let o = active(&state);
        let mut owned: Vec<V> = Vec::new();
        for x in orders_of(&state) {
            let lot = get(get(&x, "proof"), "lot");
            if js_truthy(Some(lot)) {
                let mut row = Map::new();
                row.insert("marketId".into(), get(&x, "marketId").clone());
                row.insert("exchangeId".into(), get(get(&x, "body"), "exchangeId").clone());
                if let V::Object(m) = lot {
                    for (k, v) in m {
                        row.insert(k.clone(), v.clone());
                    }
                }
                owned.push(V::Object(row));
            }
        }
        let hedge_blocked = o.is_none()
            && !ended(&state)
            && owned.len() == 1
            && (self.now)() >= instant_v(get(get(&state, "plan"), "reviewExpiresAt"))? as f64;
        let orders: Vec<V> = orders_of(&state)
            .iter()
            .map(|x| {
                let fetch_starts = get(x, "fetchStarts").as_array().cloned().unwrap_or_default();
                json!({
                    "intentId": get(x, "intentId"),
                    "orderId": get(x, "orderId"),
                    "marketId": get(x, "marketId"),
                    "exchangeId": get(get(&x, "body"), "exchangeId"),
                    "phase": get(x, "phase"),
                    "body": get(x, "body"),
                    "bodyHash": get(x, "bodyHash"),
                    "cancelPending": get(x, "cancelPending"),
                    "actualWireStarts": get(x, "wireAttempts")
                        .as_array()
                        .map(|ws| ws.iter().map(|w| get(w, "requestStartedAt").clone()).collect::<Vec<V>>())
                        .unwrap_or_default(),
                    "lastActualFetchStartAt": fetch_starts
                        .last()
                        .map(|w| get(w, "requestStartedAt").clone())
                        .unwrap_or(V::Null),
                    "actualFetchCount": fetch_starts.len(),
                    "recoveryAttemptCount": fetch_starts
                        .iter()
                        .filter(|w| get(w, "kind") == "recover")
                        .count(),
                    "semanticMismatch": get(x, "semanticMismatch"),
                    "proofHash": get(get(x, "proof"), "verificationHash"),
                })
            })
            .collect();
        let global_activity: Vec<V> = orders_of(&state)
            .iter()
            .map(|x| {
                let s = get(get(get(x, "proof"), "globalActivity"), "status");
                if s.is_null() {
                    json!("unknown")
                } else {
                    s.clone()
                }
            })
            .collect();
        let phase = match o.as_ref().map(|o| get(o, "phase")).filter(|p| !p.is_null()) {
            Some(p) => p.clone(),
            None => {
                if ended(&state) {
                    json!(if owned.len() == 2 {
                        "paired"
                    } else if !owned.is_empty() {
                        "orphaned"
                    } else {
                        "unfilled"
                    })
                } else if hedge_blocked {
                    json!("hedge_blocked")
                } else {
                    json!("ready")
                }
            }
        };
        Ok(json!({
            "mode": "paired_rest_lifecycle_v2",
            "planHash": get(get(&state, "plan"), "planHash"),
            "originalDryPlanHash": get(get(&state, "plan"), "originalDryPlanHash"),
            "originalProtectiveCost": get(get(&state, "plan"), "originalProtectiveCost"),
            "pairCostCeiling": get(get(&state, "plan"), "pairCostCeiling"),
            "minimumOrdinaryMargin": get(get(&state, "plan"), "sourceMinimumOrdinaryMargin"),
            "planLegs": get(get(&state, "plan"), "legs")
                .as_array()
                .map(|ls| ls.iter().map(|l| json!({
                    "marketId": get(l, "marketId"),
                    "exchangeId": get(l, "exchangeId"),
                    "originalLimit": get(l, "originalLimit"),
                })).collect::<Vec<V>>())
                .unwrap_or_default(),
            "reviewExpiresAt": get(get(&state, "plan"), "reviewExpiresAt"),
            "sequence": i.journal.sequence,
            "hash": i.journal.previous,
            "closed": i.closed,
            "failed": i.failed,
            "busy": i.busy,
            "phase": phase,
            "blockedReason": if hedge_blocked { json!("rules_expired") } else { V::Null },
            "funding": funding(&state)?,
            "owned": owned,
            "orders": orders,
            "globalActivity": global_activity,
            "additionalApiClients": 0,
        }))
    }
}

impl PairedRestController {
    /// fundingReservation()
    pub fn funding_reservation(&self) -> Result<V, String> {
        funding(&self.core.state())
    }
    /// prepareNext()
    pub fn prepare_next(&self) -> Result<V, String> {
        self.core.run(|s| {
            s.basic()?;
            need(
                active(&s.state()).is_none() && !ended(&s.state()),
                "ACTIVE_OR_FINISHED",
            )?;
            s.reserve()?;
            let e = s.evidence("prepare", None)?;
            let index = orders_of(&s.state()).len();
            s.append(&json!({
                "kind": "prepared",
                "index": index,
                "intentId": format!("paired-{}", uuid_v4()),
                "idempotencyKey": uuid_v4(),
                "evidence": e,
                "at": ms_to_iso((s.now)()),
            }))?;
            s.status()
        })
    }
    /// dispatchPrepared()
    pub fn dispatch_prepared(&self) -> Result<V, String> {
        self.dispatch_order("place")
    }
    /// recoverExact()
    pub fn recover_exact(&self) -> Result<V, String> {
        self.dispatch_order("recover")
    }
    /// cancelExpired()
    pub fn cancel_expired(&self) -> Result<V, String> {
        self.dispatch_order("cancel")
    }
    /// reconcile(input)
    pub fn reconcile(&self, input: &V) -> Result<V, String> {
        self.core.run(|s| {
            s.basic()?;
            let state = s.state();
            let o = active(&state).ok_or("PAIRED_REST_ORDER_ACK_REQUIRED".to_string())?;
            need(
                js_truthy(o.get("orderId")) && !get(&o, "ack").is_null(),
                "ORDER_ACK_REQUIRED",
            )?;
            let recovery_proof = if instant_v(get(get(&o, "ack"), "requestStartedAt"))?
                >= instant_v(get(get(&o, "body"), "expirationDate"))?
            {
                json!({"journalRecord": s.recover_row(&o)?})
            } else {
                V::Null
            };
            s.append(&json!({
                "kind": "reconciled",
                "index": get(&o, "index"),
                "evidence": input,
                "recoveryProof": recovery_proof,
                "at": ms_to_iso((s.now)()),
            }))?;
            s.reserve()?;
            s.status()
        })
    }
    /// dispatchOrder(operation)
    fn dispatch_order(&self, operation: &str) -> Result<V, String> {
        self.core.run(|s| {
            s.basic()?;
            let state = s.state();
            let o = active(&state).ok_or("PAIRED_REST_NO_ACTIVE_ORDER".to_string())?;
            if operation == "place" {
                need(get(&o, "phase") == "prepared", "UNKNOWN_BLOCKS_REPLACEMENT")?;
            }
            if operation == "recover" {
                s.recover_row(&o)?;
            }
            s.reserve()?;
            let lease = s.basic()?;
            let mut permit = json!({
                "intentId": get(&o, "intentId"),
                "runId": get(&s.scope, "runId"),
                "bodyHash": get(&o, "bodyHash"),
                "operation": operation,
            });
            if operation == "recover" {
                permit["retry"] = json!(true);
            }
            let authority = json!({"permit": permit, "lease": lease});
            let result = if operation == "cancel" {
                self.transport.cancel(get(&o, "orderId"), &authority)
            } else {
                self.transport.place(get(&o, "body"), &authority)
            };
            match result {
                Ok(_) => s.status(),
                Err(e) => {
                    if s.inner.borrow().failed {
                        return Err("PAIRED_REST_DURABLE_RECORDING_FAILED".into());
                    }
                    if (s.can_operate)() {
                        let code = if !e.code.is_empty()
                            && e.code.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
                        {
                            e.code.clone()
                        } else {
                            "REQUEST_UNCONFIRMED".to_string()
                        };
                        s.append(&json!({
                            "kind": "error",
                            "index": get(&o, "index"),
                            "code": code,
                            "at": ms_to_iso((s.now)()),
                        }))?;
                    }
                    Err(e.code)
                }
            }
        })
    }
    /// close()
    pub fn close(&self) -> Result<(), String> {
        need(!self.core.inner.borrow().busy, "BUSY")?;
        {
            let mut i = self.core.inner.borrow_mut();
            if i.closed {
                return Ok(());
            }
            i.closed = true;
        }
        self.core
            .inner
            .borrow_mut()
            .lease
            .release()
            .map_err(|e: baal_service_runtime::ledger::LedgerError| e.code.to_string())
    }
}
