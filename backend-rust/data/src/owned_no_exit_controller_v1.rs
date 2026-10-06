// execution/owned-no-exit-controller-v1.mjs — all reads are caller-supplied
// cached receipts. One SELL intent maximum; no automatic retry, second order,
// cash-funded complement BUY, or loss exit.
use baal_execution::core::{canonical_payload, payload_hash};
use baal_execution::gates::normalize_official_fill_tick;
use baal_execution::js_key;
use crate::no_rest_sell_reconciliation_v1::inspect_scoped_rest_no_account;
use baal_execution::receipts::parse_number_lexemes;
use baal_execution::transport::{OrderTransport, OrderTransportError, OrderTransportOptions, Guard};
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::fixed::{amount, instant, money, price};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use baal_service_runtime::ledger::{acquire_owner_lease, OwnerLease};
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

fn need(v: bool, c: &str) -> Result<(), String> {
    if v { Ok(()) } else { Err(format!("OWNED_NO_CONTROLLER_{c}")) }
}
fn copy(v: &V) -> V { v.clone() }
fn sha(b: &[u8]) -> String { sha256(b) }
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn iso(n: i64) -> String { ms_to_iso(n as f64) }
fn get<'a>(v: &'a V, k: &str) -> &'a V { v.get(k).unwrap_or(&V::Null) }
fn amount_s(v: &V) -> Result<i128, String> { amount(v.as_str().unwrap_or("")) }
fn instant_v(v: &V) -> Result<i64, String> { instant(v.as_str().unwrap_or("")) }
fn price_v(v: &V) -> Result<i128, String> { price(v.as_str().unwrap_or("")) }
fn sorted(rows: &V) -> V {
    let mut r = rows.as_array().cloned().unwrap_or_default();
    r.sort_by_key(|x| js_int(get(x, "id")).unwrap_or(0));
    V::Array(r)
}
fn fields(v: &V, keys: &[&str]) -> Result<(), String> {
    need(
        v.is_object() && !v.is_array()
            && v.as_object().unwrap().keys().all(|k| keys.contains(&k.as_str()))
            && keys.iter().all(|k| v.as_object().unwrap().contains_key(*k)),
        "EVIDENCE_FIELDS",
    )
}
fn raw(r: &V, method: &str, endpoint: &str) -> Result<V, String> {
    need(
        get(r, "method") == &json!(method)
            && get(r, "endpoint") == &json!(endpoint)
            && js_int(get(r, "status")).is_some()
            && get(r, "bodyText").as_str().is_some_and(|b| b.len() <= 8 * 1024 * 1024),
        "RAW_RECEIPT",
    )?;
    let hash = sha(get(r, "bodyText").as_str().unwrap_or("").as_bytes());
    need(
        r.get("sourceBodyHash").map(|x| x == &json!(hash)).unwrap_or(true),
        "RAW_RECEIPT_HASH",
    )?;
    need(
        instant_v(get(r, "receivedAt"))? >= instant_v(get(r, "requestStartedAt"))?,
        "RAW_RECEIPT_CLOCK",
    )?;
    let mut out = json!({
        "method": method, "endpoint": endpoint, "status": get(r, "status"),
        "bodyText": get(r, "bodyText"), "requestStartedAt": get(r, "requestStartedAt"),
        "receivedAt": get(r, "receivedAt"), "sourceBodyHash": hash,
    });
    if let Some(f) = r.get("fetchInvocationId") { out.as_object_mut().unwrap().insert("fetchInvocationId".into(), f.clone()); }
    if let Some(h) = r.get("requestBodyHash") { out.as_object_mut().unwrap().insert("requestBodyHash".into(), h.clone()); }
    Ok(out)
}
pub(crate) fn original(buy_proof: &V, original_after: &V, tournament_slug: &str) -> Result<V, String> {
    let mut body = buy_proof.clone();
    let verification_hash = body.as_object_mut().and_then(|m| m.shift_remove("verificationHash")).unwrap_or(V::Null);
    need(
        hex(&verification_hash)
            && payload_hash(&body).ok().as_deref() == verification_hash.as_str()
            && ["scoped_rest_no_lot_with_rounded_cash_v1", "scoped_rest_no_lot_with_rounded_cash_neutral_all_v2"]
                .contains(&get(buy_proof, "mode").as_str().unwrap_or(""))
            && js_truthy(buy_proof.get("scopedLotVerified"))
            && get(buy_proof, "filledQuantity") == &json!(1)
            && get(buy_proof, "signedPositionDelta") == &json!(-1)
            && get(buy_proof, "ownerId") == &json!("Codex")
            && get(buy_proof, "executionAuthority") == &json!(false),
        "BUY_PROOF",
    )?;
    let lot = get(buy_proof, "lot");
    need(
        get(lot, "quantity") == &json!(1)
            && get(lot, "side") == &json!("no")
            && get(lot, "entryPrice") == get(buy_proof, "scopedPrincipalCost")
            && get(lot, "orderId") == get(buy_proof, "orderId")
            && get(lot, "intentId") == get(buy_proof, "intentId")
            && get(lot, "ownerId") == get(buy_proof, "ownerId")
            && get(lot, "runId") == get(buy_proof, "runId"),
        "BUY_LOT",
    )?;
    price_v(get(lot, "entryPrice"))?;
    let mut binding = buy_proof.clone();
    binding.as_object_mut().unwrap().insert("tournamentSlug".into(), json!(tournament_slug));
    let a = inspect_scoped_rest_no_account(&json!({
        "account": original_after, "binding": binding, "readAt": get(buy_proof, "readAt"),
    }))?;
    need(
        payload_hash(get(&a, "sourceReceipts")).ok() == payload_hash(get(get(buy_proof, "sourceReceipts"), "after")).ok()
            && get(&a, "positions").as_array().map(|ps| ps.iter().any(|p| {
                get(p, "exchangeId") == get(buy_proof, "exchangeId")
                    && get(p, "marketId") == get(buy_proof, "marketId")
                    && get(p, "quantity") == &json!(-1)
                    && !js_truthy(p.get("settled"))
            })).unwrap_or(false),
        "ORIGINAL_AFTER_PROOF",
    )?;
    Ok(a)
}
fn claim(s: &V) -> Result<V, String> {
    let b = get(s, "buyProof");
    let intent = get(s, "intent");
    let body = json!({
        "schemaVersion": 1,
        "kind": "single_owned_no_exit_reservation",
        "ownerId": get(b, "ownerId"),
        "runId": get(b, "runId"),
        "tournamentId": get(b, "tournamentId"),
        "marketId": get(b, "marketId"),
        "exchangeId": get(b, "exchangeId"),
        "lotId": get(get(b, "lot"), "lotId"),
        "buyProofHash": get(b, "verificationHash"),
        "originalPrincipalCost": get(b, "scopedPrincipalCost"),
        "cashReserved": "0.000000",
        "sellIntentId": get(intent, "intentId"),
        "remainingQuantity": if get(get(intent, "proof"), "filledQuantity") == &json!(1) { 0 } else { 1 },
        "terminalSell": !get(intent, "proof").is_null(),
    });
    let mut out = body.clone();
    out.as_object_mut().unwrap().insert("reservationHash".into(), json!(payload_hash(&body)?));
    Ok(out)
}
fn risk(g: &V, s: &V, at: i64) -> Result<(), String> {
    let b = get(s, "buyProof");
    need(
        get(g, "ownerId") == get(b, "ownerId")
            && get(g, "runId") == get(b, "runId")
            && get(g, "tournamentId") == get(b, "tournamentId")
            && get(g, "complete") == &json!(true)
            && get(g, "soleCollector") == &json!(true)
            && get(g, "stopped") == &json!(false)
            && hex(get(g, "sourceHash"))
            && get(g, "unresolvedIntentIds").as_array().is_some_and(|a| a.len() <= 1)
            && get(g, "unresolvedIntentIds").as_array().map(|a| a.iter().all(|x| x == get(get(s, "intent"), "intentId"))).unwrap_or(true)
            && instant_v(get(g, "at"))? <= at
            && at - instant_v(get(g, "at"))? <= 20000,
        "GLOBAL_UNCERTAINTY",
    )
}
fn owned(o: &V, s: &V, at: i64) -> Result<(), String> {
    let b = get(s, "buyProof");
    need(
        !o.is_null()
            && get(o, "ownerId") == get(b, "ownerId")
            && get(o, "runId") == get(b, "runId")
            && get(o, "tournamentId") == get(b, "tournamentId")
            && get(o, "marketId") == get(b, "marketId")
            && get(o, "exchangeId") == get(b, "exchangeId")
            && get(o, "lotId") == get(get(b, "lot"), "lotId")
            && get(o, "buyProofHash") == get(b, "verificationHash")
            && get(o, "quantity") == &json!(1)
            && hex(get(o, "sourceHash"))
            && instant_v(get(o, "at"))? <= at
            && at - instant_v(get(o, "at"))? <= 20000,
        "OWNERSHIP",
    )
}
fn preflight(s: &V, e: &V, ownership: &V, at: i64) -> Result<V, String> {
    fields(e, &["account", "book", "globalRisk"])?;
    let b = get(s, "buyProof");
    need(!js_truthy(Some(get(get(s, "intent"), "proof"))), "ALREADY_TERMINAL")?;
    risk(get(e, "globalRisk"), s, at)?;
    owned(ownership, s, at)?;
    let mut binding = b.clone();
    binding.as_object_mut().unwrap().insert("tournamentSlug".into(), get(s, "tournamentSlug").clone());
    let a = inspect_scoped_rest_no_account(&json!({
        "account": get(e, "account"), "binding": binding, "readAt": iso(at),
    }))?;
    let old = original(b, get(s, "originalAfter"), get(s, "tournamentSlug").as_str().unwrap_or(""))?;
    need(
        instant_v(get(&old, "completedAt"))? <= instant_v(get(&a, "startedAt"))?
            && payload_hash(&sorted(get(&a, "scopedOrders"))).ok() == payload_hash(&sorted(get(&old, "scopedOrders"))).ok(),
        "INTERVENING_SCOPED_ACTIVITY",
    )?;
    let endpoint = format!("/markets/{}/orderbook?tournamentId={}&depth=20",
        get(b, "marketId").as_str().unwrap_or(""), get(b, "tournamentId").as_str().unwrap_or(""));
    let book = raw(get(e, "book"), "GET", &endpoint)?;
    let (d, numbers) = parse_number_lexemes(get(&book, "bodyText").as_str().unwrap_or(""))?;
    need(
        (!d.as_object().map(|m| m.contains_key("marketId")).unwrap_or(false) || get(&d, "marketId") == get(b, "marketId"))
            && get(&d, "contexts").as_array().is_some_and(|c| c.len() == 1)
            && get(&d, "contexts").as_array().unwrap()[0].get("orderbook")
                .and_then(|x| x.get("exchanges")).and_then(|x| x.as_array()).is_some_and(|x| x.len() == 1)
            && get(&d, "contexts").as_array().unwrap()[0].get("orderbook")
                .and_then(|x| x.get("exchanges")).and_then(|x| x.as_array()).map(|x| {
                    get(&x[0], "exchangeId") == get(b, "exchangeId")
                }).unwrap_or(false),
        "BOOK_SCOPE",
    )?;
    need(instant_v(get(&a, "completedAt"))? <= instant_v(get(&book, "requestStartedAt"))?, "ACCOUNT_BEFORE_BOOK")?;
    let mut gr = get(e, "globalRisk").clone();
    gr.as_object_mut().unwrap().insert("unresolvedIntentIds".into(), json!([]));
    let signal = crate::single_owned_no_exit_signal_v1::evaluate_single_owned_no_exit(&json!({
        "buyProof": b,
        "positionsReceipt": get(get(e, "account"), "positions"),
        "bookReceipt": book,
        "globalRisk": gr,
        "tournamentSlug": get(s, "tournamentSlug"),
        "previousBookVersion": get(get(s, "intent"), "lastBookVersion"),
        "now": iso(at),
    }));
    need(get(&signal, "signalReady") == &json!(true), "PROTECTED_PROFIT_UNAVAILABLE")?;
    let rung = get(&d, "contexts").as_array().unwrap()[0].get("orderbook")
        .and_then(|x| x.get("exchanges")).and_then(|x| x.as_array()).unwrap()[0]
        .get("asks").and_then(|x| x.as_array()).cloned().unwrap_or_default()
        .iter().position(|l| {
            normalize_official_fill_tick(get(l, "price")).ok()
                .map(|v| get(&v, "price") == get(get(&signal, "yesAskSourceLevel"), "price")).unwrap_or(false)
        });
    let rung = match rung { Some(i) => i as i64, None => -1 };
    let lexeme = numbers.get(&js_stringify(&json!(["contexts", 0, "orderbook", "exchanges", 0, "asks", rung, "quantity"]))).cloned().unwrap_or_default();
    need(
        lexeme.chars().count() > 0
            && {
                let int_part = lexeme.split('.').next().unwrap_or("");
                !int_part.is_empty() && int_part.bytes().all(|b| b.is_ascii_digit())
                    && (int_part == "0" || !int_part.starts_with('0'))
                    && lexeme.split('.').nth(1).map(|f| f.bytes().all(|b| b == b'0')).unwrap_or(true)
                    && int_part.parse::<u128>().map(|n| n >= 100).unwrap_or(false)
            },
        "EXACT_DEPTH",
    )?;
    if !get(s, "intent").is_null() {
        need(
            at < instant_v(get(get(get(s, "intent"), "body"), "expirationDate"))?
                && price_v(get(&signal, "protectedLimit"))? >= price_v(&json!(js_key(get(get(get(s, "intent"), "body"), "price"))))?
                && instant_v(get(&book, "requestStartedAt"))? >= instant_v(get(get(get(s, "intent"), "lastBookVersion"), "requestStartedAt"))?,
            "EXPIRED_OR_WORSE_PRICE",
        )?;
    }
    Ok(signal)
}
fn binding(s: &V) -> V {
    let b = get(s, "buyProof");
    let o = get(s, "intent");
    json!({
        "ownerId": get(b, "ownerId"),
        "runId": get(b, "runId"),
        "tournamentId": get(b, "tournamentId"),
        "tournamentSlug": get(s, "tournamentSlug"),
        "marketId": get(b, "marketId"),
        "exchangeId": get(b, "exchangeId"),
        "intentId": get(o, "intentId"),
        "idempotencyKey": get(get(o, "body"), "idempotencyKey"),
        "orderId": get(o, "orderId"),
        "side": "no",
        "action": "sell",
        "quantity": 1,
        "limit": get(o, "body").get("price").map(|p| json!(money(price_v(&json!(js_key(p))).unwrap_or(0)))).unwrap_or(V::Null),
        "createdAt": get(o, "createdAt"),
        "expirationDate": get(get(o, "body"), "expirationDate"),
    })
}
fn cancel_preflight(s: &V, e: &V, at: i64) -> Result<(), String> {
    fields(e, &["order", "globalRisk"])?;
    let o = get(s, "intent");
    let b = get(s, "buyProof");
    need(
        !get(o, "orderId").is_null()
            && !js_truthy(o.get("semanticMismatch"))
            && at >= instant_v(get(get(o, "body"), "expirationDate"))?,
        "CANCEL_NOT_OWNED_OR_EXPIRED",
    )?;
    risk(get(e, "globalRisk"), s, at)?;
    let r = raw(get(e, "order"), "GET", &format!("/orders/{}", get(o, "orderId").as_str().unwrap_or("")))?;
    let (d, _) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
    need(
        get(&r, "status") == &json!(200)
            && instant_v(get(&r, "receivedAt"))? <= at
            && at - instant_v(get(&r, "requestStartedAt"))? <= 20000
            && js_int(get(&d, "id")).map(|x| x.to_string()) == get(o, "orderId").as_str().map(String::from)
            && get(&d, "exchangeId") == get(b, "exchangeId")
            && get(&d, "tournamentId") == get(b, "tournamentId")
            && get(&d, "side") == &json!("no")
            && get(&d, "action") == &json!("sell")
            && get(&d, "quantity") == &json!(1)
            && get(&d, "open") == &json!(true)
            && get(&normalize_official_fill_tick(get(&d, "priceLimit"))?, "price") == get(&binding(s), "limit")
            && get(&d, "expirationDate") == get(get(o, "body"), "expirationDate"),
        "CANCEL_ORDER",
    )?;
    Ok(())
}
fn canonical_ack(s: &V, r: &V) -> bool {
    (|| -> Result<bool, String> {
        let (d, numbers) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
        let o = get(s, "intent");
        let exact = |k: &str| -> bool {
            let lex = numbers.get(&js_stringify(&json!([k]))).cloned().unwrap_or_default();
            let int_part = lex.split('.').next().unwrap_or("");
            !int_part.is_empty() && int_part.bytes().all(|b| b.is_ascii_digit())
                && (int_part == "0" || !int_part.starts_with('0'))
                && lex.split('.').nth(1).map(|f| f.bytes().all(|b| b == b'0')).unwrap_or(true)
                && int_part.parse::<i128>().ok() == js_int(get(&d, k)).map(|x| x as i128)
        };
        let total = amount_s(&json!(numbers.get(&js_stringify(&json!(["totalCost"]))).cloned().unwrap_or_default()))?;
        let limit = price_v(get(&binding(s), "limit"))?;
        Ok(
            get(r, "status") == &json!(200)
                && js_int(get(&d, "orderId")).is_some_and(|x| x > 0)
                && get(&d, "exchangeId") == get(get(o, "body"), "exchangeId")
                && get(&d, "side") == &json!("no")
                && get(&d, "action") == &json!("sell")
                && get(&d, "quantity") == &json!(1)
                && ["orderId", "quantity", "quantityTraded", "remainingQuantity"].iter().all(|k| exact(k))
                && [0, 1].contains(&js_int(get(&d, "quantityTraded")).unwrap_or(-1))
                && [0, 1].contains(&js_int(get(&d, "remainingQuantity")).unwrap_or(-1))
                && js_int(get(&d, "quantityTraded")).unwrap() + js_int(get(&d, "remainingQuantity")).unwrap() == 1
                && get(&normalize_official_fill_tick(get(&d, "price"))?, "price") == get(&binding(s), "limit")
                && get(&d, "all") == &V::Null
                && get(&d, "open").is_boolean()
                && (if js_int(get(&d, "quantityTraded")).unwrap() == 0 {
                    total == 0 && get(&d, "fillPrice").is_null()
                } else {
                    total >= limit && total <= 995_000 && total % 5_000 == 0
                        && price_v(get(&normalize_official_fill_tick(get(&d, "fillPrice"))?, "price"))? == total
                }),
        )
    })().unwrap_or(false)
}
pub(crate) fn legacy_reduce(s: Option<&V>, e: &V) -> Result<V, String> {
    reduce(s, e)
}
fn reduce(s: Option<&V>, e: &V) -> Result<V, String> {
    if get(e, "kind") == &json!("initialize") {
        need(s.is_none(), "INITIALIZED")?;
        original(get(e, "buyProof"), get(e, "originalAfter"), get(e, "tournamentSlug").as_str().unwrap_or(""))?;
        return Ok(json!({
            "schemaVersion": 1,
            "mode": "single_owned_no_exit_controller_v1",
            "buyProof": get(e, "buyProof"),
            "originalAfter": get(e, "originalAfter"),
            "tournamentSlug": get(e, "tournamentSlug"),
            "intent": V::Null,
            "reservation": V::Null,
        }));
    }
    let s = s.ok_or("OWNED_NO_CONTROLLER_NOT_INITIALIZED")?;
    let mut n = copy(s);
    if get(e, "kind") == &json!("reservation") {
        need(
            get(get(e, "receipt"), "reservationHash") == get(&claim(s)?, "reservationHash")
                && hex(get(get(e, "receipt"), "globalLedgerHash"))
                && instant_v(get(get(e, "receipt"), "confirmedAt"))? <= instant_v(get(e, "at"))?,
            "RESERVATION",
        )?;
        n.as_object_mut().unwrap().insert("reservation".into(), get(e, "receipt").clone());
        return Ok(n);
    }
    if get(e, "kind") == &json!("prepared") {
        need(get(&n, "intent").is_null(), "ONE_ORDER_ONLY")?;
        let signal = preflight(s, get(e, "evidence"), get(e, "ownership"), instant_v(get(e, "at"))?)?;
        need(
            get(&signal, "signalHash") == get(get(e, "signal"), "signalHash")
                && get(e, "intentId").is_string()
                && get(e, "idempotencyKey").is_string(),
            "PREPARATION",
        )?;
        let body = json!({
            "idempotencyKey": get(e, "idempotencyKey"),
            "exchangeId": get(get(s, "buyProof"), "exchangeId"),
            "side": "no",
            "action": "sell",
            "quantity": 1,
            "price": amount_s(get(&signal, "protectedLimit")).map(|x| x as f64 / 1e6).unwrap_or(f64::NAN),
            "expirationDate": iso(instant_v(get(e, "at"))? + 30000),
            "tournamentId": get(get(s, "buyProof"), "tournamentId"),
        });
        n.as_object_mut().unwrap().insert("intent".into(), json!({
            "intentId": get(e, "intentId"),
            "createdAt": get(e, "at"),
            "body": body,
            "bodyJson": canonical_payload(&body),
            "bodyHash": payload_hash(&body)?,
            "signal": signal,
            "before": get(get(e, "evidence"), "account"),
            "lastBookVersion": get(&signal, "bookVersion"),
            "phase": "prepared",
            "orderId": V::Null,
            "ack": V::Null,
            "semanticMismatch": false,
            "cancelPending": false,
            "fences": [],
            "fetchStarts": [],
            "wireReceipts": [],
            "proof": V::Null,
        }));
        return Ok(n);
    }
    need(!get(s, "intent").is_null() && get(get(s, "intent"), "proof").is_null(), "NO_ACTIVE_INTENT")?;
    if get(e, "kind") == &json!("admitted") || get(e, "kind") == &json!("network_fence") {
        let op = get(e, "operation").as_str().unwrap_or("");
        need(["place", "cancel"].contains(&op), "OPERATION")?;
        let o = get(s, "intent").clone();
        if op == "place" {
            need(
                if get(e, "kind") == &json!("admitted") { get(&o, "phase") == &json!("prepared") } else { get(&o, "phase") == &json!("dispatching") },
                "PLACEMENT_PHASE",
            )?;
            let v = preflight(s, get(e, "evidence"), get(e, "ownership"), instant_v(get(e, "at"))?)?;
            n.as_object_mut().unwrap().get_mut("intent").unwrap().as_object_mut().unwrap()
                .insert("lastBookVersion".into(), get(&v, "bookVersion").clone());
        } else {
            need(
                get(e, "kind") != &json!("admitted") || !js_truthy(o.get("cancelPending")),
                "CANCEL_PHASE",
            )?;
            cancel_preflight(s, get(e, "evidence"), instant_v(get(e, "at"))?)?;
        }
        let om = n.as_object_mut().unwrap().get_mut("intent").unwrap().as_object_mut().unwrap();
        if get(e, "kind") == &json!("admitted") {
            om.insert("operation".into(), get(e, "operation").clone());
            if op == "place" { om.insert("phase".into(), json!("dispatching")); }
            else { om.insert("cancelPending".into(), json!(true)); }
        } else {
            need(
                get(&o, "operation") == get(e, "operation")
                    && payload_hash(get(e, "leaseIdentity")).ok() == get(e, "leaseHash").as_str().map(String::from),
                "LEASE_FENCE",
            )?;
            om.get_mut("fences").unwrap().as_array_mut().unwrap().push(json!({
                "operation": get(e, "operation"),
                "leaseIdentity": get(e, "leaseIdentity"),
                "leaseHash": get(e, "leaseHash"),
                "at": get(e, "at"),
            }));
        }
        return Ok(n);
    }
    if get(e, "kind") == &json!("fetch_start") {
        let w = get(e, "start").clone();
        let o = get(s, "intent").clone();
        let f = get(&o, "fences").as_array().and_then(|x| x.last()).cloned().unwrap_or(V::Null);
        need(
            !f.is_null()
                && get(&w, "operation") == get(&o, "operation")
                && get(&w, "intentId") == get(&o, "intentId")
                && get(&w, "bodyHash") == get(&o, "bodyHash")
                && get(&w, "leaseHash") == get(&f, "leaseHash")
                && get(&w, "method") == &json!(if get(&o, "operation") == &json!("place") { "POST" } else { "DELETE" })
                && get(&w, "endpoint") == &json!(if get(&o, "operation") == &json!("place") {
                    "/orders".to_string()
                } else { format!("/orders/{}", get(&o, "orderId").as_str().unwrap_or("")) })
                && get(&w, "fetchInvocationId").is_string()
                && !get(&o, "fetchStarts").as_array().map(|fs| fs.iter().any(|x| get(x, "fetchInvocationId") == get(&w, "fetchInvocationId"))).unwrap_or(false)
                && instant_v(get(&w, "requestStartedAt"))? >= instant_v(get(&f, "at"))?
                && instant_v(get(&w, "requestStartedAt"))? <= instant_v(get(e, "at"))?,
            "FETCH_PROVENANCE",
        )?;
        if get(&w, "operation") == &json!("place") {
            need(
                instant_v(get(&w, "requestStartedAt"))? < instant_v(get(get(&o, "body"), "expirationDate"))?,
                "EXPIRED_WIRE",
            )?;
        }
        n.as_object_mut().unwrap().get_mut("intent").unwrap().as_object_mut().unwrap()
            .get_mut("fetchStarts").unwrap().as_array_mut().unwrap().push(w);
        return Ok(n);
    }
    if get(e, "kind") == &json!("wire") {
        let r = get(e, "receipt").clone();
        let o = get(s, "intent").clone();
        let w = get(&o, "fetchStarts").as_array().and_then(|x| x.last()).cloned().unwrap_or(V::Null);
        need(
            !w.is_null()
                && get(&r, "fetchInvocationId") == get(&w, "fetchInvocationId")
                && get(&r, "requestStartedAt") == get(&w, "requestStartedAt")
                && get(&r, "method") == get(&w, "method")
                && get(&r, "endpoint") == get(&w, "endpoint")
                && instant_v(get(&r, "receivedAt"))? <= instant_v(get(e, "at"))?
                && sha(get(&r, "bodyText").as_str().unwrap_or("").as_bytes()) == get(&r, "sourceBodyHash").as_str().unwrap_or(""),
            "WIRE_PROVENANCE",
        )?;
        let om = n.as_object_mut().unwrap().get_mut("intent").unwrap().as_object_mut().unwrap();
        om.get_mut("wireReceipts").unwrap().as_array_mut().unwrap().push(r.clone());
        if get(&o, "operation") == &json!("cancel") { return Ok(n); }
        let status = js_int(get(&r, "status")).unwrap_or(0);
        if (200..300).contains(&status) {
            let d = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or("")).map(|(x, _)| x).unwrap_or(V::Null);
            if js_int(get(&d, "orderId")).is_some_and(|x| x > 0) {
                let sid = js_int(get(&d, "orderId")).unwrap().to_string();
                need(om.get("orderId").unwrap_or(&V::Null).is_null() || om.get("orderId").unwrap_or(&V::Null) == &json!(sid), "ORDER_ID_CHANGED")?;
                om.insert("orderId".into(), json!(sid));
            }
            let mut ack = r.clone();
            ack.as_object_mut().unwrap().insert("requestBodyHash".into(), get(&o, "bodyHash").clone());
            drop(om);
            let mismatch = !canonical_ack(&n, &ack);
            let om = n.as_object_mut().unwrap().get_mut("intent").unwrap().as_object_mut().unwrap();
            om.insert("ack".into(), ack);
            om.insert("semanticMismatch".into(), json!(mismatch));
            om.insert("phase".into(), json!(if mismatch { "semantic_mismatch" } else { "acknowledged" }));
        } else {
            om.insert("phase".into(), json!("uncertain"));
        }
        return Ok(n);
    }
    if get(e, "kind") == &json!("error") {
        let om = n.as_object_mut().unwrap().get_mut("intent").unwrap().as_object_mut().unwrap();
        if om.get("phase").unwrap_or(&V::Null) == &json!("dispatching") {
            om.insert("phase".into(), json!("uncertain"));
        }
        om.insert("lastError".into(), get(e, "code").clone());
        return Ok(n);
    }
    if get(e, "kind") == &json!("reconciled") {
        let o = get(s, "intent").clone();
        need(
            !get(&o, "orderId").is_null() && !get(&o, "ack").is_null() && !js_truthy(o.get("semanticMismatch")),
            "UNRESOLVED_ACK",
        )?;
        fields(get(e, "evidence"), &["order", "fillPages", "after", "globalRisk"])?;
        let mut verify_in = get(e, "evidence").clone();
        let vm = verify_in.as_object_mut().unwrap();
        vm.insert("binding".into(), binding(s));
        vm.insert("originalBuyProof".into(), get(s, "buyProof").clone());
        vm.insert("originalAfter".into(), get(s, "originalAfter").clone());
        vm.insert("before".into(), get(&o, "before").clone());
        vm.insert("ack".into(), get(&o, "ack").clone());
        vm.insert("readAt".into(), get(e, "at").clone());
        let proof = crate::no_rest_sell_reconciliation_v1::verify_scoped_rest_no_sell(&verify_in)?;
        need(
            get(&proof, "originalBuyProofHash") == get(get(s, "buyProof"), "verificationHash")
                && get(&proof, "originalLotId") == get(get(get(s, "buyProof"), "lot"), "lotId")
                && (!js_truthy(proof.get("filledQuantity")) || amount_s(get(&proof, "realizedPnl"))? >= 5_000),
            "EXIT_PROOF_OR_MINIMUM",
        )?;
        let om = n.as_object_mut().unwrap().get_mut("intent").unwrap().as_object_mut().unwrap();
        om.insert("proof".into(), proof);
        om.insert("phase".into(), json!("reconciled"));
        om.insert("cancelPending".into(), json!(false));
        return Ok(n);
    }
    Err("OWNED_NO_CONTROLLER_EVENT".into())
}

type GetV = Rc<dyn Fn() -> Result<V, String>>;

pub struct OwnedNoExitController {
    dir: PathBuf,
    buy_proof: V,
    original_after: V,
    tournament_slug: String,
    dispatch: Rc<dyn Fn(&V) -> Result<V, String>>,
    raw_receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    can_operate: Rc<dyn Fn() -> bool>,
    assert_account_lease: GetV,
    get_ownership: GetV,
    get_evidence: Rc<dyn Fn(&V) -> Result<V, String>>,
    install_exit_reservation: Rc<dyn Fn(&V) -> Result<V, String>>,
    now: Rc<dyn Fn() -> i64>,
    lease: RefCell<OwnerLease>,
    file: PathBuf,
    code: V,
    state: RefCell<Option<V>>,
    closed: RefCell<bool>,
    failed: RefCell<bool>,
    busy: RefCell<bool>,
    size: RefCell<usize>,
    bytes_hash: RefCell<String>,
    stat: RefCell<u64>,
    journal: RefCell<Journal>,
    issued: RefCell<BTreeMap<usize, V>>,
    in_flight: RefCell<V>,
}

pub fn owned_no_exit_controller_identity(execution_dir: &Path) -> V {
    json!(crate::paired_rest_controller::paired_rest_code_identity_file(execution_dir, "owned-no-exit-controller-v1.mjs"))
}

impl OwnedNoExitController {
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        dir: &Path,
        buy_proof: &V,
        original_after: &V,
        tournament_slug: &str,
        dispatch: Rc<dyn Fn(&V) -> Result<V, String>>,
        raw_receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
        can_operate: Rc<dyn Fn() -> bool>,
        assert_account_lease: GetV,
        get_ownership: GetV,
        get_evidence: Rc<dyn Fn(&V) -> Result<V, String>>,
        install_exit_reservation: Rc<dyn Fn(&V) -> Result<V, String>>,
        now: Rc<dyn Fn() -> i64>,
        execution_dir: &Path,
    ) -> Result<Self, String> {
        need(dir.is_absolute(), "DEPENDENCIES")?;
        let buy_proof = copy(buy_proof);
        let original_after = copy(original_after);
        original(&buy_proof, &original_after, tournament_slug)?;
        let lease = acquire_owner_lease(
            dir,
            get(&buy_proof, "ownerId").as_str().unwrap_or(""),
            get(&buy_proof, "runId").as_str().unwrap_or(""),
            &format!("single-owned-no-exit:{}", get(get(&buy_proof, "lot"), "lotId").as_str().unwrap_or("")),
            (now)() as f64,
        ).map_err(|e| format!("OWNED_NO_CONTROLLER_{}", e.code))?;
        let file = dir.join("owned-no-exit-journal.jsonl");
        let code = owned_no_exit_controller_identity(execution_dir);
        let this = Self {
            dir: dir.to_path_buf(),
            buy_proof,
            original_after,
            tournament_slug: tournament_slug.to_string(),
            dispatch,
            raw_receipt_of,
            can_operate,
            assert_account_lease,
            get_ownership,
            get_evidence,
            install_exit_reservation,
            now,
            lease: RefCell::new(lease),
            file: file.clone(),
            code,
            state: RefCell::new(None),
            closed: RefCell::new(false),
            failed: RefCell::new(false),
            busy: RefCell::new(false),
            size: RefCell::new(0),
            bytes_hash: RefCell::new(String::new()),
            stat: RefCell::new(0),
            journal: RefCell::new(Journal::open(&file)?),
            issued: RefCell::new(BTreeMap::new()),
            in_flight: RefCell::new(V::Null),
        };
        match this.init() {
            Ok(()) => Ok(this),
            Err(e) => {
                this.lease.borrow_mut().release().ok();
                Err(e)
            }
        }
    }
    fn basic(&self) -> Result<V, String> {
        need(!*self.closed.borrow() && !*self.failed.borrow() && (self.can_operate)(), "STOPPED_OR_FAILED")?;
        self.lease.borrow().assert_ownership().map_err(|e| format!("OWNED_NO_CONTROLLER_{}", e.code))?;
        let l = (self.assert_account_lease)()?;
        need(
            get(&l, "ownerId") == get(&self.buy_proof, "ownerId")
                && get(&l, "tournamentId") == get(&self.buy_proof, "tournamentId")
                && get(&l, "leaseId").as_str().is_some_and(|s| !s.is_empty()),
            "ACCOUNT_LEASE",
        )?;
        Ok(l)
    }
    fn init(&self) -> Result<(), String> {
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
            let next = reduce(self.state.borrow().as_ref(), get(get(&row, "event"), "input"))?;
            need(payload_hash(&next).ok() == payload_hash(get(&row, "state")).ok(), "REPLAY")?;
            *self.state.borrow_mut() = Some(next);
        }
        *self.size.borrow_mut() = bytes.len();
        *self.bytes_hash.borrow_mut() = sha(&bytes);
        let meta = std::fs::metadata(&self.file).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        { use std::os::unix::fs::MetadataExt; *self.stat.borrow_mut() = meta.ino(); }
        if self.state.borrow().is_none() {
            self.append(&json!({
                "kind": "initialize",
                "buyProof": self.buy_proof,
                "originalAfter": self.original_after,
                "tournamentSlug": self.tournament_slug,
                "at": iso((self.now)()),
            }))?;
        } else {
            let s = self.state.borrow().clone().unwrap();
            need(
                payload_hash(get(&s, "buyProof")).ok() == payload_hash(&self.buy_proof).ok()
                    && payload_hash(get(&s, "originalAfter")).ok() == payload_hash(&self.original_after).ok()
                    && get(&s, "tournamentSlug").as_str() == Some(self.tournament_slug.as_str()),
                "BUY_BINDING_CHANGED",
            )?;
        }
        Ok(())
    }
    fn append(&self, input: &V) -> Result<(), String> {
        self.basic()?;
        let before = std::fs::read(&self.file).map_err(|e| e.to_string())?;
        let meta = std::fs::metadata(&self.file).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        { use std::os::unix::fs::MetadataExt; need(meta.ino() == *self.stat.borrow(), "JOURNAL_CHANGED")?; }
        need(
            before.len() == *self.size.borrow() && sha(&before) == *self.bytes_hash.borrow(),
            "JOURNAL_CHANGED",
        )?;
        let next = reduce(self.state.borrow().as_ref(), input)?;
        need(
            *self.size.borrow() + js_stringify(&next).len() + js_stringify(input).len() < 64 * 1024 * 1024,
            "JOURNAL_BOUND",
        )?;
        let result = (|| -> Result<(), String> {
            self.journal.borrow_mut().commit(&next, &json!({"codeIdentity": self.code, "input": input}))?;
            self.basic()?;
            let after = std::fs::read(&self.file).map_err(|e| e.to_string())?;
            need(after[..*self.size.borrow()] == before[..], "JOURNAL_PREFIX_CHANGED")?;
            *self.state.borrow_mut() = Some(next);
            *self.size.borrow_mut() = after.len();
            *self.bytes_hash.borrow_mut() = sha(&after);
            Ok(())
        })();
        if result.is_err() { *self.failed.borrow_mut() = true; }
        result
    }
    fn reserve(&self) -> Result<V, String> {
        let receipt = (self.install_exit_reservation)(&claim(self.state.borrow().as_ref().unwrap())?)?;
        self.append(&json!({"kind": "reservation", "receipt": receipt, "at": iso((self.now)())}))?;
        Ok(receipt)
    }
    fn reserved(&self) -> Result<(), String> {
        let s = self.state.borrow().clone().unwrap();
        let r = get(&s, "reservation").clone();
        need(
            get(&r, "reservationHash") == get(&claim(&s)?, "reservationHash")
                && hex(get(&r, "globalLedgerHash"))
                && instant_v(get(&r, "confirmedAt"))? <= (self.now)()
                && (self.now)() - instant_v(get(&r, "confirmedAt"))? <= 20000,
            "RESERVATION_STALE_OR_MISSING",
        )
    }
    fn evidence(&self, operation: &str) -> Result<V, String> {
        let s = self.state.borrow().clone().unwrap_or(V::Null);
        (self.get_evidence)(&json!({
            "operation": operation,
            "intentId": get(get(&s, "intent"), "intentId"),
            "orderId": get(get(&s, "intent"), "orderId"),
        }))
    }
    fn cancel_gate(&self, e: &V, at: i64) -> Result<(), String> {
        cancel_preflight(self.state.borrow().as_ref().unwrap(), e, at)
    }
    fn validate(&self, permit: &V, spec: &V, network: bool) -> Result<bool, String> {
        self.basic()?;
        self.reserved()?;
        let s = self.state.borrow().clone().unwrap();
        let o = get(&s, "intent").clone();
        need(
            !o.is_null() && get(&o, "proof").is_null()
                && get(permit, "intentId") == get(&o, "intentId")
                && get(permit, "bodyHash") == get(&o, "bodyHash"),
            "PERMIT",
        )?;
        let e = self.evidence(get(permit, "operation").as_str().unwrap_or(""))?;
        let ownership = if get(permit, "operation") == &json!("place") {
            (self.get_ownership)()?
        } else { V::Null };
        if get(permit, "operation") == &json!("place") {
            need(
                get(spec, "method") == &json!("POST")
                    && get(spec, "endpoint") == &json!("/orders")
                    && get(spec, "bodyJson") == get(&o, "bodyJson"),
                "PAYLOAD",
            )?;
            preflight(&s, &e, &ownership, (self.now)())?;
        } else {
            need(
                get(spec, "method") == &json!("DELETE")
                    && get(spec, "endpoint") == &json!(format!("/orders/{}", get(&o, "orderId").as_str().unwrap_or(""))),
                "CANCEL_SCOPE",
            )?;
            self.cancel_gate(&e, (self.now)())?;
        }
        let l = self.basic()?;
        self.append(&json!({
            "kind": if network { "network_fence" } else { "admitted" },
            "operation": get(permit, "operation"),
            "evidence": e,
            "ownership": ownership,
            "leaseIdentity": l,
            "leaseHash": payload_hash(&l)?,
            "at": iso((self.now)()),
        }))?;
        if network {
            if get(permit, "operation") == &json!("place") {
                let e2 = self.evidence("place")?;
                let s2 = self.state.borrow().clone().unwrap();
                preflight(&s2, &e2, &(self.get_ownership)()?, (self.now)())?;
            } else {
                let e2 = self.evidence("cancel")?;
                self.cancel_gate(&e2, (self.now)())?;
            }
            self.reserved()?;
            self.basic()?;
        }
        Ok(true)
    }
    pub fn on_fetch_start(&self, meta: &V) -> Result<bool, String> {
        if get(meta, "method") == &json!("GET") || self.in_flight.borrow().is_null() {
            return Ok(false);
        }
        let result = (|| -> Result<bool, String> {
            self.basic()?;
            let s = self.state.borrow().clone().unwrap();
            let o = get(&s, "intent").clone();
            let f = get(&o, "fences").as_array().and_then(|x| x.last()).cloned().unwrap_or(V::Null);
            let in_flight = self.in_flight.borrow().clone();
            need(
                get(meta, "method") == get(&in_flight, "method")
                    && get(meta, "endpoint") == get(&in_flight, "endpoint")
                    && get(meta, "requestBodyHash") == &json!(if get(meta, "method") == &json!("POST") { get(&o, "bodyHash").clone() } else { V::Null }),
                "FETCH_BINDING",
            )?;
            let mut start = meta.clone();
            start.as_object_mut().unwrap().insert("operation".into(), get(&o, "operation").clone());
            start.as_object_mut().unwrap().insert("intentId".into(), get(&o, "intentId").clone());
            start.as_object_mut().unwrap().insert("bodyHash".into(), get(&o, "bodyHash").clone());
            start.as_object_mut().unwrap().insert("leaseHash".into(), get(&f, "leaseHash").clone());
            self.append(&json!({"kind": "fetch_start", "start": start, "at": iso((self.now)())}))?;
            Ok(true)
        })();
        if result.is_err() { *self.failed.borrow_mut() = true; }
        result
    }
    fn wrap(&self, spec: &V) -> Result<V, OrderTransportError> {
        let s = self.state.borrow().clone().unwrap_or(V::Null);
        let o = get(&s, "intent").clone();
        let key = self.issued.borrow().len();
        self.issued.borrow_mut().insert(key, json!({
            "method": get(spec, "method"),
            "endpoint": get(spec, "endpoint"),
            "bodyJson": get(spec, "bodyJson"),
            "beforeNetwork": !get(spec, "beforeNetwork").is_null(),
        }));
        *self.in_flight.borrow_mut() = spec.clone();
        let capture = |response: &V| -> Result<(), String> {
            let receipt = raw(
                &(self.raw_receipt_of)(response, &json!({
                    "method": get(spec, "method"),
                    "requestBodyHash": if get(spec, "method") == &json!("POST") { get(&o, "bodyHash").clone() } else { V::Null },
                }))?,
                get(spec, "method").as_str().unwrap_or(""),
                get(spec, "endpoint").as_str().unwrap_or(""),
            )?;
            need(
                instant_v(get(&receipt, "requestStartedAt"))? >= instant_v(get(response, "requestStartedAt"))?
                    && instant_v(get(&receipt, "receivedAt"))? <= instant_v(get(response, "receivedAt"))?
                    && get(&receipt, "status") == get(response, "status")
                    && payload_hash(&parse_number_lexemes(get(&receipt, "bodyText").as_str().unwrap_or(""))?.0).ok()
                        == payload_hash(get(response, "data")).ok(),
                "RAW_WIRE_BINDING",
            )?;
            self.append(&json!({"kind": "wire", "receipt": receipt, "at": iso((self.now)())}))?;
            Ok(())
        };
        let result = (self.dispatch)(spec);
        let out = match result {
            Ok(response) => {
                if let Err(e) = capture(&response) {
                    return Err(OrderTransportError {
                        code: e.clone(), status: None, api_code: None, outcome: "rejected".into(),
                        may_have_executed: true, retry_same_request_only: false, retry_after_ms: 0.0,
                        semantic_mismatch: false,
                    });
                }
                Ok(response)
            }
            Err(e) => {
                let _ = e;
                Err(OrderTransportError {
                    code: "REQUEST_UNCONFIRMED".into(), status: None, api_code: None, outcome: "rejected".into(),
                    may_have_executed: true, retry_same_request_only: false, retry_after_ms: 0.0,
                    semantic_mismatch: false,
                })
            }
        };
        self.issued.borrow_mut().remove(&key);
        *self.in_flight.borrow_mut() = V::Null;
        out
    }
    fn transport(self: &Rc<Self>) -> Result<OrderTransport, OrderTransportError> {
        let scope = baal_execution::transport::scope_of(&json!({
            "tournamentId": get(&self.buy_proof, "tournamentId"),
            "tournamentSlug": self.tournament_slug,
            "exchanges": [{"marketId": get(&self.buy_proof, "marketId"), "exchangeId": get(&self.buy_proof, "exchangeId")}],
        }))?;
        fn terr(e: String) -> OrderTransportError {
            OrderTransportError {
                code: e, status: None, api_code: None, outcome: "rejected".into(),
                may_have_executed: false, retry_same_request_only: false,
                retry_after_ms: 0.0, semantic_mismatch: false,
            }
        }
        let w1 = Rc::downgrade(self);
        let w2 = Rc::downgrade(self);
        let w3 = Rc::downgrade(self);
        let w4 = Rc::downgrade(self);
        let w5 = Rc::downgrade(self);
        let w6 = Rc::downgrade(self);
        let now = self.now.clone();
        OrderTransport::new(OrderTransportOptions {
            dispatch: Box::new(move |spec: &V, _bd: Option<&Guard>, _bn: Option<&Guard>| {
                let this = w1.upgrade().ok_or_else(|| terr("OWNED_NO_CONTROLLER_STOPPED_OR_FAILED".into()))?;
                this.wrap(spec)
            }),
            scope,
            can_write: Box::new(move || w2.upgrade().map(|t| t.basic().is_ok()).unwrap_or(false)),
            owns_order: Box::new(move |id: &str| {
                w3.upgrade().map(|t| {
                    let s = t.state.borrow().clone().unwrap_or(V::Null);
                    get(get(&s, "intent"), "orderId").as_str() == Some(id)
                }).unwrap_or(false)
            }),
            validate_permit: Box::new(move |p: &V, spec: &V| {
                w4.upgrade().map(|t| t.validate(p, spec, false).map_err(terr)).unwrap_or(Ok(false))
            }),
            validate_network_permit: Box::new(move |p: &V, spec: &V| {
                w5.upgrade().map(|t| t.validate(p, spec, true).map_err(terr)).unwrap_or(Ok(false))
            }),
            validate_lease: Box::new(move |l: &V, _unused: &V| {
                w6.upgrade().map(|t| {
                    t.basic().ok().and_then(|b| payload_hash(&b).ok())
                        .map(|h| payload_hash(l).ok() == Some(h)).unwrap_or(false)
                }).map(Ok).unwrap_or(Ok(false))
            }),
            now: Box::new(move || now()),
            max_pages: 10,
        })
    }
    pub fn authorizes(&self, spec: &V) -> bool {
        self.basic().is_ok()
            && self.issued.borrow().values().any(|p| {
                get(p, "method") == get(spec, "method")
                    && get(p, "endpoint") == get(spec, "endpoint")
                    && get(p, "bodyJson") == get(spec, "bodyJson")
            })
    }
    pub fn status(&self) -> V {
        let s = self.state.borrow().clone().unwrap_or(V::Null);
        let o = get(&s, "intent");
        let p = get(o, "proof");
        let journal = self.journal.borrow();
        json!({
            "mode": get(&s, "mode"),
            "ownerId": get(&self.buy_proof, "ownerId"),
            "runId": get(&self.buy_proof, "runId"),
            "tournamentId": get(&self.buy_proof, "tournamentId"),
            "marketId": get(&self.buy_proof, "marketId"),
            "exchangeId": get(&self.buy_proof, "exchangeId"),
            "lotId": get(get(&self.buy_proof, "lot"), "lotId"),
            "buyProofHash": get(&self.buy_proof, "verificationHash"),
            "originalPrincipalCost": get(&self.buy_proof, "scopedPrincipalCost"),
            "sequence": journal.sequence,
            "hash": journal.previous,
            "phase": get(o, "phase").as_str().map(String::from).unwrap_or_else(|| "owned".to_string()),
            "remainingOwnedLot": if !p.is_null() { get(p, "remainingOwnedLot").clone() } else { copy(get(&self.buy_proof, "lot")) },
            "realizedPnl": if !p.is_null() { get(p, "realizedPnl").clone() } else { json!("0.000000") },
            "scopedProceeds": if !p.is_null() { get(p, "scopedProceeds").clone() } else { json!("0.000000") },
            "intent": if !o.is_null() { json!({
                "intentId": get(o, "intentId"),
                "orderId": get(o, "orderId"),
                "body": get(o, "body"),
                "bodyHash": get(o, "bodyHash"),
                "createdAt": get(o, "createdAt"),
                "semanticMismatch": get(o, "semanticMismatch"),
                "cancelPending": get(o, "cancelPending"),
                "actualFetchCount": get(o, "fetchStarts").as_array().map(|a| a.len()).unwrap_or(0),
                "lastActualFetchStartAt": get(o, "fetchStarts").as_array().and_then(|f| f.last()).map(|v| get(v, "requestStartedAt").clone()).unwrap_or(V::Null),
                "proofHash": get(p, "verificationHash"),
            }) } else { V::Null },
            "reservation": claim(&s).unwrap_or(V::Null),
            "failed": *self.failed.borrow(),
            "closed": *self.closed.borrow(),
            "busy": *self.busy.borrow(),
            "additionalApiClients": 0,
        })
    }
    pub fn exit_reservation(&self) -> Result<V, String> {
        claim(self.state.borrow().as_ref().unwrap())
    }
    pub fn exit_reservation_status(&self) -> Result<V, String> {
        let s = self.state.borrow().clone().unwrap();
        Ok(json!({"record": claim(&s)?, "receipt": get(&s, "reservation")}))
    }
    pub fn verified_exit_proof(&self) -> V {
        let s = self.state.borrow().clone().unwrap_or(V::Null);
        get(get(&s, "intent"), "proof").clone()
    }
    fn run<F, T>(&self, f: F) -> Result<T, String> where F: FnOnce() -> Result<T, String> {
        need(!*self.busy.borrow(), "BUSY")?;
        *self.busy.borrow_mut() = true;
        let r = f();
        *self.busy.borrow_mut() = false;
        r
    }
    pub fn prepare(&self) -> Result<V, String> {
        self.run(|| {
            self.basic()?;
            need(self.state.borrow().as_ref().unwrap().get("intent").unwrap_or(&V::Null).is_null(), "ONE_ORDER_ONLY")?;
            let e = self.evidence("prepare")?;
            let ownership = (self.get_ownership)()?;
            let at = iso((self.now)());
            let s = self.state.borrow().clone().unwrap();
            let signal = preflight(&s, &e, &ownership, instant_v(&json!(at.clone())).unwrap_or_else(|_| parse_iso_ms(&at).unwrap_or(0.0) as i64))?;
            self.append(&json!({
                "kind": "prepared",
                "intentId": format!("owned-no-exit-{}", uuid::Uuid::new_v4()),
                "idempotencyKey": uuid::Uuid::new_v4().to_string(),
                "evidence": e,
                "ownership": ownership,
                "signal": signal,
                "at": at,
            }))?;
            self.reserve()?;
            Ok(self.status())
        })
    }
    fn dispatch_order(self: &Rc<Self>, operation: &str) -> Result<V, String> {
        self.run(|| {
            self.basic()?;
            let s = self.state.borrow().clone().unwrap();
            let o = get(&s, "intent").clone();
            need(
                !o.is_null() && get(&o, "proof").is_null()
                    && if operation == "place" {
                        get(&o, "phase") == &json!("prepared")
                    } else {
                        !get(&o, "orderId").is_null()
                            && !js_truthy(o.get("cancelPending"))
                            && !js_truthy(o.get("semanticMismatch"))
                    },
                "UNKNOWN_OR_TERMINAL_BLOCKS_ORDER",
            )?;
            drop(s);
            self.reserve()?;
            let o = get(&self.state.borrow().clone().unwrap(), "intent").clone();
            let permit = json!({"intentId": get(&o, "intentId"), "bodyHash": get(&o, "bodyHash"), "operation": operation});
            let l = self.basic()?;
            let transport = self.transport().map_err(|e| e.code)?;
            let result = if operation == "place" {
                transport.place(&get(&o, "body").clone(), &json!({"permit": permit, "lease": l}))
            } else {
                transport.cancel(get(&o, "orderId"), &json!({"permit": permit, "lease": l}))
            };
            if let Err(e) = result {
                if *self.failed.borrow() {
                    return Err("OWNED_NO_CONTROLLER_DURABLE_FAILURE".into());
                }
                if (self.can_operate)() {
                    let code = if !e.code.is_empty() && e.code.bytes().all(|b| b.is_ascii_uppercase() || b == b'_') {
                        e.code.clone()
                    } else { "REQUEST_UNCONFIRMED".into() };
                    self.append(&json!({"kind": "error", "code": code, "at": iso((self.now)())}))?;
                }
                return Err(e.code);
            }
            Ok(self.status())
        })
    }
    pub fn dispatch_prepared(self: &Rc<Self>) -> Result<V, String> { self.dispatch_order("place") }
    pub fn cancel_expired(self: &Rc<Self>) -> Result<V, String> { self.dispatch_order("cancel") }
    pub fn reconcile(&self, evidence: &V) -> Result<V, String> {
        self.run(|| {
            self.basic()?;
            self.append(&json!({"kind": "reconciled", "evidence": evidence, "at": iso((self.now)())}))?;
            self.reserve()?;
            Ok(self.status())
        })
    }
    pub fn close(&self) -> Result<(), String> {
        need(!*self.busy.borrow(), "BUSY")?;
        if *self.closed.borrow() { return Ok(()); }
        *self.closed.borrow_mut() = true;
        self.lease.borrow_mut().release().map_err(|e| e.message)
    }
}
