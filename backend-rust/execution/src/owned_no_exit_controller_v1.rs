// execution/owned-no-exit-controller-v1.mjs — all reads are caller-supplied
// cached receipts. One SELL intent maximum; no automatic retry, second order,
// cash-funded complement BUY, or loss exit.
use crate::core::{canonical_payload, payload_hash};
use crate::gates::normalize_official_fill_tick;
use crate::no_rest_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::no_rest_sell_reconciliation_v1::verify_scoped_rest_no_sell;
use crate::receipts::parse_number_lexemes;
use crate::transport::{Guard, OrderTransport, OrderTransportError, OrderTransportOptions};
use baal_service_runtime::fixed::{amount, instant, money, price};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use baal_service_runtime::ledger::acquire_owner_lease;

use num_bigint::BigInt;
use serde_json::Value as V;
use serde_json::json;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const PREFIX: &str = "OWNED_NO_CONTROLLER_";

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok {
        return Err(format!("{PREFIX}{code}"));
    }
    Ok(())
}
fn sha(s: &str) -> String {
    sha256(s.as_bytes())
}
fn hex(x: &V) -> bool {
    x.as_str()
        .map(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()))
        .unwrap_or(false)
}
fn iso(n: i64) -> String {
    baal_service_runtime::contract::ms_to_iso(n as f64)
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

// identity(): collectFrozenPaperFiles(workspace,['execution/owned-no-exit-controller-v1.mjs'])
fn identity() -> Result<V, String> {
    // JS resolves the workspace as <module dir>/.. — here the manifest dir is
    // backend-rust/execution, so ../.. is the repo root holding execution/*.mjs.
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    crate::frozen::collect_frozen_paper_files(&workspace, &["execution/owned-no-exit-controller-v1.mjs"])
}

fn sorted(rows: &V) -> V {
    let mut r = rows.as_array().cloned().unwrap_or_default();
    r.sort_by_key(|a| js_int(get(a, "id")).unwrap_or(0));
    V::Array(r)
}

fn fields(v: &V, keys: &[&str]) -> Result<(), String> {
    need(
        v.is_object()
            && v.as_object().unwrap().keys().all(|k| keys.contains(&k.as_str()))
            && keys.iter().all(|k| v.as_object().unwrap().contains_key(*k)),
        "EVIDENCE_FIELDS",
    )
}

fn raw(r: &V, method: &str, endpoint: &str) -> Result<V, String> {
    need(
        get(r, "method").as_str() == Some(method)
            && get(r, "endpoint").as_str() == Some(endpoint)
            && r.get("status").map(|v| v.is_i64() || v.is_u64()).unwrap_or(false)
            && get(r, "bodyText").is_string()
            && get(r, "bodyText").as_str().unwrap_or("").len() <= 8 * 1024 * 1024,
        "RAW_RECEIPT",
    )?;
    let hash = sha(get(r, "bodyText").as_str().unwrap_or(""));
    need(
        r.get("sourceBodyHash").is_none()
            || get(r, "sourceBodyHash").as_str() == Some(hash.as_str()),
        "RAW_RECEIPT_HASH",
    )?;
    need(
        instant(get(r, "receivedAt").as_str().unwrap_or(""))?
            >= instant(get(r, "requestStartedAt").as_str().unwrap_or(""))?,
        "RAW_RECEIPT_CLOCK",
    )?;
    let mut out = vec![
        ("method", json!(method)),
        ("endpoint", json!(endpoint)),
        ("status", get(r, "status").clone()),
        ("bodyText", get(r, "bodyText").clone()),
        ("requestStartedAt", get(r, "requestStartedAt").clone()),
        ("receivedAt", get(r, "receivedAt").clone()),
        ("sourceBodyHash", json!(hash)),
    ];
    if let Some(v) = r.get("fetchInvocationId") {
        out.push(("fetchInvocationId", v.clone()));
    }
    if let Some(v) = r.get("requestBodyHash") {
        out.push(("requestBodyHash", v.clone()));
    }
    Ok(V::Object(out.into_iter().map(|(k, v)| (k.to_string(), v)).collect()))
}

// original(buyProof,originalAfter,tournamentSlug)
fn original(buy_proof: &V, original_after: &V, tournament_slug: &V) -> Result<V, String> {
    let mut body = buy_proof.clone();
    let verification_hash = body
        .as_object_mut()
        .and_then(|m| m.remove("verificationHash"))
        .unwrap_or(V::Null);
    need(
        hex(&verification_hash)
            && payload_hash(&body)? == verification_hash.as_str().unwrap_or("")
            && ["scoped_rest_no_lot_with_rounded_cash_v1", "scoped_rest_no_lot_with_rounded_cash_neutral_all_v2"]
                .contains(&get(buy_proof, "mode").as_str().unwrap_or(""))
            && get(buy_proof, "scopedLotVerified") == &json!(true)
            && get(buy_proof, "filledQuantity") == &json!(1)
            && get(buy_proof, "signedPositionDelta") == &json!(-1)
            && get(buy_proof, "ownerId").as_str() == Some("Codex")
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
    price(get(lot, "entryPrice").as_str().unwrap_or(""))?;
    let mut binding = buy_proof.clone();
    binding
        .as_object_mut()
        .unwrap()
        .insert("tournamentSlug".into(), tournament_slug.clone());
    let a = inspect_scoped_rest_no_account(&json!({
        "account": original_after,
        "binding": binding,
        "readAt": get(buy_proof, "readAt"),
    }))?;
    need(
        payload_hash(get(&a, "sourceReceipts"))?
            == payload_hash(get(&get(buy_proof, "sourceReceipts"), "after"))?
            && get(&a, "positions").as_array().is_some_and(|ps| {
                ps.iter().any(|p| {
                    get(p, "exchangeId") == get(buy_proof, "exchangeId")
                        && get(p, "marketId") == get(buy_proof, "marketId")
                        && get(p, "quantity") == &json!(-1)
                        && !get(p, "settled").as_bool().unwrap_or(false)
                })
            }),
        "ORIGINAL_AFTER_PROOF",
    )?;
    Ok(a)
}

// claim(s) — the exit reservation record with reservationHash.
fn claim(s: &V) -> Result<V, String> {
    let b = get(s, "buyProof");
    let filled = js_int(get(&get(get(s, "intent"), "proof"), "filledQuantity"));
    let body = vec![
        ("schemaVersion", json!(1)),
        ("kind", json!("single_owned_no_exit_reservation")),
        ("ownerId", get(b, "ownerId").clone()),
        ("runId", get(b, "runId").clone()),
        ("tournamentId", get(b, "tournamentId").clone()),
        ("marketId", get(b, "marketId").clone()),
        ("exchangeId", get(b, "exchangeId").clone()),
        ("lotId", get(&get(b, "lot"), "lotId").clone()),
        ("buyProofHash", get(b, "verificationHash").clone()),
        ("originalPrincipalCost", get(b, "scopedPrincipalCost").clone()),
        ("cashReserved", json!("0.000000")),
        (
            "sellIntentId",
            get(&get(s, "intent"), "intentId").clone(),
        ),
        (
            "remainingQuantity",
            json!(if filled == Some(1) { 0 } else { 1 }),
        ),
        (
            "terminalSell",
            json!(!get(&get(s, "intent"), "proof").is_null()),
        ),
    ];
    let mut obj = serde_json::Map::new();
    for (k, v) in body {
        obj.insert(k.to_string(), v);
    }
    let reservation_hash = payload_hash(&V::Object(obj.clone()))?;
    obj.insert("reservationHash".into(), json!(reservation_hash));
    Ok(V::Object(obj))
}

// risk(g,s,at)
fn risk(g: &V, s: &V, at: i64) -> Result<(), String> {
    let b = get(s, "buyProof");
    let unresolved = get(g, "unresolvedIntentIds").as_array().cloned().unwrap_or_default();
    need(
        get(g, "ownerId") == get(b, "ownerId")
            && get(g, "runId") == get(b, "runId")
            && get(g, "tournamentId") == get(b, "tournamentId")
            && get(g, "complete") == &json!(true)
            && get(g, "soleCollector") == &json!(true)
            && get(g, "stopped") == &json!(false)
            && hex(get(g, "sourceHash"))
            && get(g, "unresolvedIntentIds").is_array()
            && unresolved.len() <= 1
            && unresolved
                .iter()
                .all(|x| Some(x) == get(s, "intent").get("intentId"))
            && instant(get(g, "at").as_str().unwrap_or(""))? <= at
            && at - instant(get(g, "at").as_str().unwrap_or(""))? <= 20000,
        "GLOBAL_UNCERTAINTY",
    )
}

// owned(o,s,at)
fn owned(o: &V, s: &V, at: i64) -> Result<(), String> {
    let b = get(s, "buyProof");
    need(
        !o.is_null()
            && get(o, "ownerId") == get(b, "ownerId")
            && get(o, "runId") == get(b, "runId")
            && get(o, "tournamentId") == get(b, "tournamentId")
            && get(o, "marketId") == get(b, "marketId")
            && get(o, "exchangeId") == get(b, "exchangeId")
            && get(o, "lotId") == get(&get(b, "lot"), "lotId")
            && get(o, "buyProofHash") == get(b, "verificationHash")
            && get(o, "quantity") == &json!(1)
            && hex(get(o, "sourceHash"))
            && instant(get(o, "at").as_str().unwrap_or(""))? <= at
            && at - instant(get(o, "at").as_str().unwrap_or(""))? <= 20000,
        "OWNERSHIP",
    )
}

// preflight(s,e,ownership,at) — the book/freshness/global-risk gate for a sell.
fn preflight(s: &V, e: &V, ownership: &V, at: i64) -> Result<V, String> {
    fields(e, &["account", "book", "globalRisk"])?;
    let b = get(s, "buyProof");
    need(
        !js_truthy(get(&get(s, "intent"), "proof").into()),
        "ALREADY_TERMINAL",
    )?;
    risk(get(e, "globalRisk"), s, at)?;
    owned(ownership, s, at)?;
    let mut binding = b.clone();
    binding
        .as_object_mut()
        .unwrap()
        .insert("tournamentSlug".into(), get(s, "tournamentSlug").clone());
    let a = inspect_scoped_rest_no_account(&json!({
        "account": get(e, "account"),
        "binding": binding,
        "readAt": iso(at),
    }))?;
    let old = original(b, get(s, "originalAfter"), get(s, "tournamentSlug"))?;
    need(
        instant(get(&old, "completedAt").as_str().unwrap_or(""))?
            <= instant(get(&a, "startedAt").as_str().unwrap_or(""))?
            && payload_hash(&sorted(get(&a, "scopedOrders")))?
                == payload_hash(&sorted(get(&old, "scopedOrders")))?,
        "INTERVENING_SCOPED_ACTIVITY",
    )?;
    let book = raw(
        get(e, "book"),
        "GET",
        &format!(
            "/markets/{}/orderbook?tournamentId={}&depth=20",
            get(b, "marketId").as_str().unwrap_or(""),
            get(b, "tournamentId").as_str().unwrap_or("")
        ),
    )?;
    let (d, numbers) = parse_number_lexemes(get(&book, "bodyText").as_str().unwrap_or(""))?;
    need(
        (!d.as_object().map(|o| o.contains_key("marketId")).unwrap_or(false)
            || get(&d, "marketId") == get(b, "marketId"))
            && get(&d, "contexts").as_array().map(|x| x.len() == 1).unwrap_or(false)
            && get(&get(&get(&d, "contexts")[0], "orderbook"), "exchanges")
                .as_array()
                .map(|x| x.len() == 1)
                .unwrap_or(false)
            && get(
                &get(&get(&get(&d, "contexts")[0], "orderbook"), "exchanges")[0],
                "exchangeId",
            ) == get(b, "exchangeId"),
        "BOOK_SCOPE",
    )?;
    need(
        instant(get(&a, "completedAt").as_str().unwrap_or(""))?
            <= instant(get(&book, "requestStartedAt").as_str().unwrap_or(""))?,
        "ACCOUNT_BEFORE_BOOK",
    )?;
    let mut global_risk = get(e, "globalRisk").clone();
    global_risk
        .as_object_mut()
        .unwrap()
        .insert("unresolvedIntentIds".into(), json!([]));
    let signal = crate::single_owned_no_exit_signal_v1::evaluate_single_owned_no_exit(&json!({
        "buyProof": b,
        "positionsReceipt": get(&get(e, "account"), "positions"),
        "bookReceipt": book,
        "globalRisk": global_risk,
        "tournamentSlug": get(s, "tournamentSlug"),
        "previousBookVersion": get(&get(s, "intent"), "lastBookVersion"),
        "now": iso(at),
    }));
    need(get(&signal, "signalReady") == &json!(true), "PROTECTED_PROFIT_UNAVAILABLE")?;
    let asks = get(&get(&get(&get(&d, "contexts")[0], "orderbook"), "exchanges")[0], "asks");
    let rung = asks
        .as_array()
        .map(|levels| {
            levels.iter().position(|l| {
                normalize_official_fill_tick(get(l, "price"))
                    .ok()
                    .and_then(|v| get(&v, "price").as_str().map(|x| x.to_string()))
                    == get(&get(&signal, "yesAskSourceLevel"), "price").as_str().map(|x| x.to_string())
            })
        })
        .unwrap_or(None);
    let rung = match rung {
        Some(i) => i,
        None => i64::MAX as usize,
    };
    let lexeme = numbers.get(&serde_json::to_string(&json!([
        "contexts", 0, "orderbook", "exchanges", 0, "asks", rung, "quantity"
    ])).unwrap_or_default()).cloned();
    let exact_quantity = lexeme
        .as_ref()
        .map(|l| {
            l.chars().take_while(|c| *c != '.').collect::<String>().parse::<BigInt>().unwrap_or(BigInt::from(0))
        })
        .unwrap_or(BigInt::from(0));
    need(
        lexeme.is_some()
            && lexeme
                .as_ref()
                .map(|l| {
                    let re = regex_simple(l);
                    re && exact_quantity >= BigInt::from(100)
                })
                .unwrap_or(false),
        "EXACT_DEPTH",
    )?;
    if !get(s, "intent").is_null() {
        need(
            at < instant(get(&get(&get(s, "intent"), "body"), "expirationDate").as_str().unwrap_or(""))?
                && price(&js_str(&get(&signal, "protectedLimit")))?
                    >= price(&js_str(&get(&get(&get(s, "intent"), "body"), "price")))?
                && instant(get(&book, "requestStartedAt").as_str().unwrap_or(""))?
                    >= instant(get(&get(&get(s, "intent"), "lastBookVersion"), "requestStartedAt").as_str().unwrap_or(""))?,
            "EXPIRED_OR_WORSE_PRICE",
        )?;
    }
    Ok(signal)
}

fn js_str(v: &V) -> String {
    v.as_str()
        .map(|x| x.to_string())
        .or_else(|| v.as_i64().map(|x| x.to_string()))
        .or_else(|| v.as_u64().map(|x| x.to_string()))
        .or_else(|| v.as_f64().map(|x| x.to_string()))
        .unwrap_or_default()
}

// /^(0|[1-9]\d*)(\.0+)?$/ — digits, optional trailing .0*
fn regex_simple(lexeme: &str) -> bool {
    let (int_part, frac) = match lexeme.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (lexeme, None),
    };
    !int_part.is_empty()
        && int_part.chars().all(|c| c.is_ascii_digit())
        && (int_part == "0" || !int_part.starts_with('0'))
        && frac.map(|f| !f.is_empty() && f.chars().all(|c| c == '0')).unwrap_or(true)
}

// binding(s) — order descriptor re-bound at reconciliation.
fn binding(s: &V) -> Result<V, String> {
    let b = get(s, "buyProof");
    let o = get(s, "intent");
    Ok(json!({
        "ownerId": get(b, "ownerId"),
        "runId": get(b, "runId"),
        "tournamentId": get(b, "tournamentId"),
        "tournamentSlug": get(s, "tournamentSlug"),
        "marketId": get(b, "marketId"),
        "exchangeId": get(b, "exchangeId"),
        "intentId": get(o, "intentId"),
        "idempotencyKey": get(&get(o, "body"), "idempotencyKey"),
        "orderId": get(o, "orderId"),
        "side": "no",
        "action": "sell",
        "quantity": 1,
        "limit": money(price(&js_str(&get(&get(o, "body"), "price")))?),
        "createdAt": get(o, "createdAt"),
        "expirationDate": get(&get(o, "body"), "expirationDate"),
    }))
}

fn cancel_preflight(s: &V, e: &V, at: i64) -> Result<(), String> {
    fields(e, &["order", "globalRisk"])?;
    let o = get(s, "intent");
    let b = get(s, "buyProof");
    need(
        !get(o, "orderId").is_null()
            && !js_truthy(o.get("semanticMismatch"))
            && at >= instant(get(&get(o, "body"), "expirationDate").as_str().unwrap_or(""))?,
        "CANCEL_NOT_OWNED_OR_EXPIRED",
    )?;
    risk(get(e, "globalRisk"), s, at)?;
    let r = raw(
        get(e, "order"),
        "GET",
        &format!("/orders/{}", js_str(&get(o, "orderId"))),
    )?;
    let (d, _) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
    need(
        get(&r, "status") == &json!(200)
            && instant(get(&r, "receivedAt").as_str().unwrap_or(""))? <= at
            && at - instant(get(&r, "requestStartedAt").as_str().unwrap_or(""))? <= 20000
            && crate::js_key(get(&d, "id")) == crate::js_key(&get(o, "orderId").clone())
            && get(&d, "exchangeId") == get(b, "exchangeId")
            && get(&d, "tournamentId") == get(b, "tournamentId")
            && get(&d, "side").as_str() == Some("no")
            && get(&d, "action").as_str() == Some("sell")
            && get(&d, "quantity") == &json!(1)
            && get(&d, "open") == &json!(true)
            && get(&normalize_official_fill_tick(get(&d, "priceLimit"))?, "price")
                .as_str()
                == get(&binding(s)?, "limit").as_str()
            && get(&d, "expirationDate") == get(&get(o, "body"), "expirationDate"),
        "CANCEL_ORDER",
    )
}

// canonicalAck(s,r)
fn canonical_ack(s: &V, r: &V) -> bool {
    (|| -> Result<bool, String> {
        let (d, numbers) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
        let o = get(s, "intent");
        let exact = |k: &str| -> bool {
            let lex = numbers.get(&serde_json::to_string(&json!([k])).unwrap_or_default());
            match lex {
                Some(l) if regex_simple(l) => {
                    BigInt::parse_bytes(l.split('.').next().unwrap_or("").as_bytes(), 10)
                        == js_int(&get(&d, k)).map(BigInt::from)
                }
                _ => false,
            }
        };
        let total = amount(numbers
            .get(&serde_json::to_string(&json!(["totalCost"])).unwrap_or_default())
            .map(|x| x.as_str())
            .unwrap_or(""))?;
        let limit = price(&js_str(&get(&binding(s)?, "limit").clone()))?;
        let traded_zero = js_int(get(&d, "quantityTraded")) == Some(0);
        let fill_ok = if traded_zero {
            total == 0 && get(&d, "fillPrice").is_null()
        } else {
            total >= limit
                && total <= 995000
                && total % 5000 == 0
                && price(&js_str(&get(&normalize_official_fill_tick(get(&d, "fillPrice"))?, "price").clone()))? == total
        };
        Ok(get(r, "status") == &json!(200)
            && js_int(get(&d, "orderId")).is_some_and(|x| x > 0)
            && get(&d, "exchangeId") == get(&get(o, "body"), "exchangeId")
            && get(&d, "side").as_str() == Some("no")
            && get(&d, "action").as_str() == Some("sell")
            && get(&d, "quantity") == &json!(1)
            && ["orderId", "quantity", "quantityTraded", "remainingQuantity"]
                .iter()
                .all(|k| exact(k))
            && [0, 1].contains(&js_int(get(&d, "quantityTraded")).unwrap_or(-1))
            && [0, 1].contains(&js_int(get(&d, "remainingQuantity")).unwrap_or(-1))
            && js_int(get(&d, "quantityTraded")).unwrap_or(0) + js_int(get(&d, "remainingQuantity")).unwrap_or(0) == 1
            && get(&normalize_official_fill_tick(get(&d, "price"))?, "price").as_str()
                == get(&binding(s)?, "limit").as_str()
            && d.get("all") == Some(&V::Null)
            && get(&d, "open").is_boolean()
            && fill_ok)
    })()
    .unwrap_or(false)
}

// reduce(s,e) — journal event fold.
fn reduce(s: Option<V>, e: &V) -> Result<V, String> {
    let kind = get(e, "kind").as_str().unwrap_or("");
    if kind == "initialize" {
        need(s.is_none(), "INITIALIZED")?;
        original(get(e, "buyProof"), get(e, "originalAfter"), get(e, "tournamentSlug"))?;
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
    let s = match s {
        Some(v) => v,
        None => return Err(format!("{PREFIX}NOT_INITIALIZED")),
    };
    let mut n = s.clone();
    let o = get(&n, "intent");
    if kind == "reservation" {
        need(
            get(&get(e, "receipt"), "reservationHash") == &get(&claim(&n)?, "reservationHash").clone()
                && hex(get(&get(e, "receipt"), "globalLedgerHash"))
                && instant(get(&get(e, "receipt"), "confirmedAt").as_str().unwrap_or(""))?
                    <= instant(get(e, "at").as_str().unwrap_or(""))?,
            "RESERVATION",
        )?;
        n.as_object_mut()
            .unwrap()
            .insert("reservation".into(), get(e, "receipt").clone());
        return Ok(n);
    }
    if kind == "prepared" {
        need(o.is_null(), "ONE_ORDER_ONLY")?;
        let signal = preflight(
            &n,
            get(e, "evidence"),
            get(e, "ownership"),
            instant(get(e, "at").as_str().unwrap_or(""))?,
        )?;
        need(
            get(&signal, "signalHash") == get(&get(e, "signal"), "signalHash")
                && get(e, "intentId").is_string()
                && get(e, "idempotencyKey").is_string(),
            "PREPARATION",
        )?;
        let body = json!({
            "idempotencyKey": get(e, "idempotencyKey"),
            "exchangeId": get(&get(&n, "buyProof"), "exchangeId"),
            "side": "no",
            "action": "sell",
            "quantity": 1,
            "price": js_str(&get(&signal, "protectedLimit")).parse::<f64>().unwrap_or(f64::NAN),
            "expirationDate": iso(instant(get(e, "at").as_str().unwrap_or(""))? + 30000),
            "tournamentId": get(&get(&n, "buyProof"), "tournamentId"),
        });
        let intent = json!({
            "intentId": get(e, "intentId"),
            "createdAt": get(e, "at"),
            "body": body,
            "bodyJson": canonical_payload(&body),
            "bodyHash": payload_hash(&body)?,
            "signal": signal,
            "before": get(&get(e, "evidence"), "account"),
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
        });
        n.as_object_mut().unwrap().insert("intent".into(), intent);
        return Ok(n);
    }
    need(
        !o.is_null() && get(o, "proof").is_null(),
        "NO_ACTIVE_INTENT",
    )?;
    match kind {
        "admitted" | "network_fence" => {
            let operation = get(e, "operation").as_str().unwrap_or("");
            need(["place", "cancel"].contains(&operation), "OPERATION")?;
            if operation == "place" {
                need(
                    if kind == "admitted" {
                        get(o, "phase").as_str() == Some("prepared")
                    } else {
                        get(o, "phase").as_str() == Some("dispatching")
                    },
                    "PLACEMENT_PHASE",
                )?;
                let v = preflight(
                    &n,
                    get(e, "evidence"),
                    get(e, "ownership"),
                    instant(get(e, "at").as_str().unwrap_or(""))?,
                )?;
                let mut o = o.clone();
                o.as_object_mut()
                    .unwrap()
                    .insert("lastBookVersion".into(), get(&v, "bookVersion").clone());
                let mut arr = n.clone();
                arr.as_object_mut().unwrap().insert("intent".into(), o);
                n = arr;
            } else {
                need(
                    kind != "admitted" || !js_truthy(o.get("cancelPending")),
                    "CANCEL_PHASE",
                )?;
                cancel_preflight(&n, get(e, "evidence"), instant(get(e, "at").as_str().unwrap_or(""))?)?;
            }
            let mut o = get(&n, "intent").clone();
            if kind == "admitted" {
                o.as_object_mut()
                    .unwrap()
                    .insert("operation".into(), get(e, "operation").clone());
                if operation == "place" {
                    o.as_object_mut()
                        .unwrap()
                        .insert("phase".into(), json!("dispatching"));
                } else {
                    o.as_object_mut()
                        .unwrap()
                        .insert("cancelPending".into(), json!(true));
                }
            } else {
                need(
                    get(&o, "operation") == get(e, "operation")
                        && payload_hash(get(e, "leaseIdentity"))? == get(e, "leaseHash").as_str().unwrap_or(""),
                    "LEASE_FENCE",
                )?;
                let mut fences = get(&o, "fences").as_array().cloned().unwrap_or_default();
                fences.push(json!({
                    "operation": get(e, "operation"),
                    "leaseIdentity": get(e, "leaseIdentity"),
                    "leaseHash": get(e, "leaseHash"),
                    "at": get(e, "at"),
                }));
                o.as_object_mut().unwrap().insert("fences".into(), V::Array(fences));
            }
            n.as_object_mut().unwrap().insert("intent".into(), o);
            Ok(n)
        }
        "fetch_start" => {
            let w = get(e, "start");
            let f = get(o, "fences").as_array().and_then(|x| x.last()).cloned().unwrap_or(V::Null);
            need(
                !f.is_null()
                    && get(w, "operation") == get(o, "operation")
                    && get(w, "intentId") == get(o, "intentId")
                    && get(w, "bodyHash") == get(o, "bodyHash")
                    && get(w, "leaseHash") == get(&f, "leaseHash")
                    && get(w, "method").as_str()
                        == Some(if get(o, "operation").as_str() == Some("place") {
                            "POST"
                        } else {
                            "DELETE"
                        })
                    && get(w, "endpoint").as_str()
                        == Some(if get(o, "operation").as_str() == Some("place") {
                            "/orders".to_string()
                        } else {
                            format!("/orders/{}", js_str(&get(o, "orderId")))
                        }.as_str())
                    && get(w, "fetchInvocationId").is_string()
                    && !get(o, "fetchStarts").as_array().cloned().unwrap_or_default().iter().any(|x| {
                        get(x, "fetchInvocationId") == get(w, "fetchInvocationId")
                    })
                    && instant(get(w, "requestStartedAt").as_str().unwrap_or(""))?
                        >= instant(get(&f, "at").as_str().unwrap_or(""))?
                    && instant(get(w, "requestStartedAt").as_str().unwrap_or(""))?
                        <= instant(get(e, "at").as_str().unwrap_or(""))?,
                "FETCH_PROVENANCE",
            )?;
            if get(w, "operation").as_str() == Some("place") {
                need(
                    instant(get(w, "requestStartedAt").as_str().unwrap_or(""))?
                        < instant(get(&get(o, "body"), "expirationDate").as_str().unwrap_or(""))?,
                    "EXPIRED_WIRE",
                )?;
            }
            let mut o = o.clone();
            let mut starts = get(&o, "fetchStarts").as_array().cloned().unwrap_or_default();
            starts.push(w.clone());
            o.as_object_mut().unwrap().insert("fetchStarts".into(), V::Array(starts));
            n.as_object_mut().unwrap().insert("intent".into(), o);
            Ok(n)
        }
        "wire" => {
            let r = get(e, "receipt");
            let w = get(o, "fetchStarts").as_array().and_then(|x| x.last()).cloned().unwrap_or(V::Null);
            need(
                !w.is_null()
                    && get(r, "fetchInvocationId") == get(&w, "fetchInvocationId")
                    && get(r, "requestStartedAt") == get(&w, "requestStartedAt")
                    && get(r, "method") == get(&w, "method")
                    && get(r, "endpoint") == get(&w, "endpoint")
                    && instant(get(r, "receivedAt").as_str().unwrap_or(""))?
                        <= instant(get(e, "at").as_str().unwrap_or(""))?
                    && sha(get(r, "bodyText").as_str().unwrap_or(""))
                        == get(r, "sourceBodyHash").as_str().unwrap_or(""),
                "WIRE_PROVENANCE",
            )?;
            let mut o = o.clone();
            let mut receipts = get(&o, "wireReceipts").as_array().cloned().unwrap_or_default();
            receipts.push(r.clone());
            o.as_object_mut().unwrap().insert("wireReceipts".into(), V::Array(receipts));
            if get(&o, "operation").as_str() == Some("cancel") {
                n.as_object_mut().unwrap().insert("intent".into(), o);
                return Ok(n);
            }
            let status = js_int(get(r, "status")).unwrap_or(0);
            if (200..300).contains(&status) {
                let d = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))
                    .map(|x| x.0)
                    .ok();
                if let Some(order_id) = d.as_ref().and_then(|d| js_int(get(d, "orderId"))).filter(|x| *x > 0) {
                    need(
                        get(&o, "orderId").is_null()
                            || get(&o, "orderId").as_str() == Some(order_id.to_string().as_str()),
                        "ORDER_ID_CHANGED",
                    )?;
                    o.as_object_mut()
                        .unwrap()
                        .insert("orderId".into(), json!(order_id.to_string()));
                }
                let mut ack = r.clone();
                ack.as_object_mut()
                    .unwrap()
                    .insert("requestBodyHash".into(), get(&o, "bodyHash").clone());
                let mismatch = !canonical_ack(&n, &ack);
                o.as_object_mut().unwrap().insert("ack".into(), ack);
                o.as_object_mut()
                    .unwrap()
                    .insert("semanticMismatch".into(), json!(mismatch));
                o.as_object_mut().unwrap().insert(
                    "phase".into(),
                    json!(if mismatch { "semantic_mismatch" } else { "acknowledged" }),
                );
            } else {
                o.as_object_mut().unwrap().insert("phase".into(), json!("uncertain"));
            }
            n.as_object_mut().unwrap().insert("intent".into(), o);
            Ok(n)
        }
        "error" => {
            let mut o = o.clone();
            if get(&o, "phase").as_str() == Some("dispatching") {
                o.as_object_mut().unwrap().insert("phase".into(), json!("uncertain"));
            }
            o.as_object_mut().unwrap().insert("lastError".into(), get(e, "code").clone());
            n.as_object_mut().unwrap().insert("intent".into(), o);
            Ok(n)
        }
        "reconciled" => {
            need(
                !get(o, "orderId").is_null()
                    && !get(o, "ack").is_null()
                    && !js_truthy(o.get("semanticMismatch")),
                "UNRESOLVED_ACK",
            )?;
            fields(get(e, "evidence"), &["order", "fillPages", "after", "globalRisk"])?;
            let mut verify_input = get(e, "evidence").clone();
            let verify_obj = verify_input.as_object_mut().unwrap();
            verify_obj.insert("binding".into(), binding(&n)?);
            verify_obj.insert("originalBuyProof".into(), get(&n, "buyProof").clone());
            verify_obj.insert("originalAfter".into(), get(&n, "originalAfter").clone());
            verify_obj.insert("before".into(), get(o, "before").clone());
            verify_obj.insert("ack".into(), get(o, "ack").clone());
            verify_obj.insert("readAt".into(), get(e, "at").clone());
            let proof = verify_scoped_rest_no_sell(&verify_input)?;
            need(
                get(&proof, "originalBuyProofHash") == get(&get(&n, "buyProof"), "verificationHash")
                    && get(&proof, "originalLotId") == get(&get(&get(&n, "buyProof"), "lot"), "lotId")
                    && (!js_truthy(get(&proof, "filledQuantity").into())
                        || amount(&js_str(&get(&proof, "realizedPnl")))? >= 5000),
                "EXIT_PROOF_OR_MINIMUM",
            )?;
            let mut o = o.clone();
            o.as_object_mut().unwrap().insert("proof".into(), proof);
            o.as_object_mut().unwrap().insert("phase".into(), json!("reconciled"));
            o.as_object_mut()
                .unwrap()
                .insert("cancelPending".into(), json!(false));
            n.as_object_mut().unwrap().insert("intent".into(), o);
            Ok(n)
        }
        _ => Err("OWNED_NO_CONTROLLER_EVENT".into()),
    }
}

pub type OwnedDispatch = crate::paired_rest_controller::WireDispatch;

#[allow(clippy::type_complexity)]
pub struct OwnedNoExitControllerDeps {
    pub dispatch: Rc<OwnedDispatch>,
    pub raw_receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub assert_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub get_ownership: Rc<dyn Fn() -> Result<V, String>>,
    pub get_evidence: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub install_exit_reservation: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub now: Rc<dyn Fn() -> i64>,
}

pub struct OwnedNoExitController {
    inner: Rc<RefCell<CtrlInner>>,
    transport: OrderTransport,
}

struct CtrlInner {
    state: Option<V>,
    buy_proof: V,
    original_after: V,
    tournament_slug: V,
    dir: PathBuf,
    file: PathBuf,
    journal: Journal,
    code: V,
    deps: OwnedNoExitControllerDeps,
    lease: Rc<RefCell<baal_service_runtime::ledger::OwnerLease>>,
    rows: Vec<V>,
    closed: bool,
    failed: bool,
    busy: bool,
    in_flight: Option<V>,
    issued: std::collections::HashMap<usize, V>,
    size: usize,
    bytes_hash: String,
    ino: u64,
    birthtime_ms: f64,
}

fn basic(inner: &Rc<RefCell<CtrlInner>>) -> Result<V, String> {
    let g = inner.borrow();
    need(
        !g.closed && !g.failed && (g.deps.can_operate)(),
        "STOPPED_OR_FAILED",
    )?;
    g.lease.borrow().assert_ownership().map_err(|e| format!("{PREFIX}{}", e.message))?;
    let l = (g.deps.assert_account_lease)()?;
    need(
        get(&l, "ownerId") == get(&g.buy_proof, "ownerId")
            && get(&l, "tournamentId") == get(&g.buy_proof, "tournamentId")
            && get(&l, "leaseId").as_str().map(|s| !s.is_empty()).unwrap_or(false),
        "ACCOUNT_LEASE",
    )?;
    Ok(l)
}

fn file_stat(file: &Path) -> Result<(u64, f64), String> {
    let m = fs::metadata(file).map_err(|e| e.to_string())?;
    let created = m.created().map_err(|e| e.to_string())?;
    Ok((
        m.ino(),
        created.duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs_f64() * 1000.0,
    ))
}

fn append_event(inner: &Rc<RefCell<CtrlInner>>, input: &V) -> Result<(), String> {
    let (file, size, bytes_hash, ino, birthtime_ms, state) = {
        let g = inner.borrow();
        (
            g.file.clone(),
            g.size,
            g.bytes_hash.clone(),
            g.ino,
            g.birthtime_ms,
            g.state.clone(),
        )
    };
    basic(inner)?;
    let before = fs::read(&file).map_err(|e| e.to_string())?;
    let (st_ino, st_birth) = file_stat(&file)?;
    let code = {
        let g = inner.borrow();
        g.code.clone()
    };
    need(
        st_ino == ino
            && st_birth == birthtime_ms
            && before.len() == size
            && sha256(&before) == bytes_hash
            && payload_hash(&identity().unwrap_or(V::Null)).unwrap_or_default()
                == payload_hash(&code).unwrap_or_default(),
        "JOURNAL_CHANGED",
    )?;
    let next = reduce(state.clone(), input)?;
    need(
        size + js_stringify(&next).len() + js_stringify(input).len() < 64 * 1024 * 1024,
        "JOURNAL_BOUND",
    )?;
    let result = (|| -> Result<(), String> {
        let row = {
            let code = inner.borrow().code.clone();
            let mut g = inner.borrow_mut();
            g.journal
                .commit(&next, &json!({"codeIdentity": code, "input": input.clone()}))?
        };
        basic(inner)?;
        let after = fs::read(&file).map_err(|e| e.to_string())?;
        need(&after[..size] == before.as_slice(), "JOURNAL_PREFIX_CHANGED")?;
        let mut g = inner.borrow_mut();
        g.state = Some(next);
        g.size = after.len();
        g.bytes_hash = sha256(&after);
        g.rows.push(row);
        Ok(())
    })();
    if result.is_err() {
        inner.borrow_mut().failed = true;
    }
    result
}

fn evidence(inner: &Rc<RefCell<CtrlInner>>, operation: &str) -> Result<V, String> {
    let g = inner.borrow();
    let state = g.state.clone().unwrap_or(V::Null);
    let intent = get(&state, "intent");
    (g.deps.get_evidence)(&json!({
        "operation": operation,
        "intentId": get(intent, "intentId"),
        "orderId": get(intent, "orderId"),
    }))
}

fn cancel_gate(inner: &Rc<RefCell<CtrlInner>>, e: &V, at: i64) -> Result<(), String> {
    let g = inner.borrow();
    cancel_preflight(&g.state.clone().unwrap_or(V::Null), e, at)
}

fn validate(inner: &Rc<RefCell<CtrlInner>>, permit: &V, spec: &V, network: bool) -> Result<bool, String> {
    let l = basic(inner)?;
    reserved(inner)?;
    let o = {
        let g = inner.borrow();
        get(&g.state.clone().unwrap_or(V::Null), "intent").clone()
    };
    need(
        !o.is_null()
            && get(&o, "proof").is_null()
            && get(permit, "intentId") == get(&o, "intentId")
            && get(permit, "bodyHash") == get(&o, "bodyHash"),
        "PERMIT",
    )?;
    let operation = get(permit, "operation").as_str().unwrap_or("");
    let e = evidence(inner, operation)?;
    let ownership = if operation == "place" {
        (inner.borrow().deps.get_ownership)()?
    } else {
        V::Null
    };
    let n = (inner.borrow().deps.now)();
    if operation == "place" {
        need(
            get(spec, "method").as_str() == Some("POST")
                && get(spec, "endpoint").as_str() == Some("/orders")
                && get(spec, "bodyJson") == get(&o, "bodyJson"),
            "PAYLOAD",
        )?;
        let st = inner.borrow().state.clone().unwrap_or(V::Null);
        preflight(&st, &e, &ownership, n)?;
    } else {
        need(
            get(spec, "method").as_str() == Some("DELETE")
                && get(spec, "endpoint").as_str()
                    == Some(format!("/orders/{}", js_str(&get(&o, "orderId"))).as_str()),
            "CANCEL_SCOPE",
        )?;
        cancel_gate(inner, &e, n)?;
    }
    let l = basic(inner)?;
    append_event(
        inner,
        &json!({
            "kind": if network { "network_fence" } else { "admitted" },
            "operation": operation,
            "evidence": e,
            "ownership": ownership,
            "leaseIdentity": l,
            "leaseHash": payload_hash(&l)?,
            "at": iso((inner.borrow().deps.now)()),
        }),
    )?;
    if network {
        if operation == "place" {
            let e2 = evidence(inner, "place")?;
            let ownership2 = (inner.borrow().deps.get_ownership)()?;
            let st = inner.borrow().state.clone().unwrap_or(V::Null);
            preflight(&st, &e2, &ownership2, (inner.borrow().deps.now)())?;
        } else {
            let e2 = evidence(inner, "cancel")?;
            cancel_gate(inner, &e2, (inner.borrow().deps.now)())?;
        }
        reserved(inner)?;
        basic(inner)?;
    }
    Ok(true)
}

fn reserved(inner: &Rc<RefCell<CtrlInner>>) -> Result<(), String> {
    let (state, n) = {
        let g = inner.borrow();
        (g.state.clone().unwrap_or(V::Null), (g.deps.now)())
    };
    let r = get(&state, "reservation");
    need(
        get(r, "reservationHash") == get(&claim(&state)?, "reservationHash")
            && hex(get(r, "globalLedgerHash"))
            && instant(get(r, "confirmedAt").as_str().unwrap_or(""))? <= n
            && n - instant(get(r, "confirmedAt").as_str().unwrap_or(""))? <= 20000,
        "RESERVATION_STALE_OR_MISSING",
    )
}

// openOwnedNoExitController({dir,buyProof,originalAfter,tournamentSlug,dispatch,rawReceiptOf,canOperate,assertAccountLease,getOwnership,getEvidence,installExitReservation,now})
pub fn open_owned_no_exit_controller(
    dir: &Path,
    buy_proof: &V,
    original_after: &V,
    tournament_slug: &V,
    deps: OwnedNoExitControllerDeps,
) -> Result<OwnedNoExitController, String> {
    let tournament_slug = tournament_slug.clone();
    need(dir.is_absolute(), "DEPENDENCIES")?;
    original(buy_proof, original_after, &tournament_slug)?;
    let lease = acquire_owner_lease(
        dir,
        get(buy_proof, "ownerId").as_str().unwrap_or(""),
        get(buy_proof, "runId").as_str().unwrap_or(""),
        &format!("single-owned-no-exit:{}", get(&get(buy_proof, "lot"), "lotId").as_str().unwrap_or("")),
        (deps.now)() as f64,
    )
    .map_err(|e| format!("{PREFIX}{}", e.message))?;
    let lease_rc = Rc::new(RefCell::new(lease));
    let open_result = (|| -> Result<OwnedNoExitController, String> {
        let file = dir.join("owned-no-exit-journal.jsonl");
        let code = identity()?;
        basic_guard_dir(dir, &deps, buy_proof, &lease_rc)?;
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
        let mut rows: Vec<V> = Vec::new();
        for line in String::from_utf8_lossy(&bytes).split('\n').filter(|l| !l.is_empty()) {
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            let row_code = get(&row, "event").get("codeIdentity").cloned().unwrap_or(V::Null);
            need(
                payload_hash(&row_code).unwrap_or_default() == payload_hash(&code).unwrap_or_default(),
                "CODE_CHANGED",
            )?;
            let row_input = get(&row, "event").get("input").cloned().unwrap_or(V::Null);
            state = Some(reduce(state, &row_input)?);
            need(
                payload_hash(state.as_ref().unwrap())? == payload_hash(get(&row, "state"))?,
                "REPLAY",
            )?;
            rows.push(row);
        }
        let (ino, birthtime_ms) = file_stat(&file)?;
        let inner = Rc::new(RefCell::new(CtrlInner {
            state,
            buy_proof: buy_proof.clone(),
            original_after: original_after.clone(),
            tournament_slug: tournament_slug.clone(),
            dir: dir.to_path_buf(),
            file: file.clone(),
            journal,
            code,
            deps,
            lease: lease_rc.clone(),
            rows,
            closed: false,
            failed: false,
            busy: false,
            in_flight: None,
            issued: std::collections::HashMap::new(),
            size: bytes.len(),
            bytes_hash: sha256(&bytes),
            ino,
            birthtime_ms,
        }));
        if inner.borrow().state.is_none() {
            append_event(
                &inner,
                &json!({
                    "kind": "initialize",
                    "buyProof": buy_proof,
                    "originalAfter": original_after,
                    "tournamentSlug": tournament_slug,
                    "at": iso((inner.borrow().deps.now)()),
                }),
            )?;
        } else {
            let g = inner.borrow();
            let st = g.state.clone().unwrap();
            need(
                payload_hash(get(&st, "buyProof"))? == payload_hash(buy_proof)?
                    && payload_hash(get(&st, "originalAfter"))? == payload_hash(original_after)?
                    && get(&st, "tournamentSlug") == &tournament_slug,
                "BUY_BINDING_CHANGED",
            )?;
        }
        let inner_w = inner.clone();
        let wrap = move |spec: &V, bd: Option<&Guard<'_>>, bn: Option<&Guard<'_>>| -> Result<V, OrderTransportError> {
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
        };
        let scope_v = json!({
            "tournamentId": get(buy_proof, "tournamentId"),
            "tournamentSlug": tournament_slug,
            "exchanges": [{
                "marketId": get(buy_proof, "marketId"),
                "exchangeId": get(buy_proof, "exchangeId"),
            }],
        });
        let inner_v = inner.clone();
        let inner_v2 = inner.clone();
        let inner_w2 = inner.clone();
        let inner_o = inner.clone();
        let inner_l = inner.clone();
        let transport = OrderTransport::new(OrderTransportOptions {
            scope: crate::transport::scope_of(&scope_v).map_err(|e| e.code)?,
            dispatch: Box::new(wrap),
            can_write: Box::new(move || {
                let g = inner_w2.borrow();
                !g.closed && !g.failed && (g.deps.can_operate)() && g.lease.borrow().assert_ownership().is_ok()
            }),
            owns_order: Box::new(move |id: &str| {
                let g = inner_o.borrow();
                get(&g.state.clone().unwrap_or(V::Null), "intent").get("orderId")
                    == Some(&json!(id))
            }),
            validate_permit: Box::new(move |p: &V, spec: &V| -> Result<bool, OrderTransportError> {
                validate(&inner_v, p, spec, false).map_err(|e| OrderTransportError {
                    code: e,
                    status: None,
                    api_code: None,
                    outcome: "blocked".into(),
                    may_have_executed: false,
                    retry_same_request_only: false,
                    retry_after_ms: 0.0,
                    semantic_mismatch: false,
                })
            }),
            validate_network_permit: Box::new(move |p: &V, spec: &V| -> Result<bool, OrderTransportError> {
                validate(&inner_v2, p, spec, true).map_err(|e| OrderTransportError {
                    code: e,
                    status: None,
                    api_code: None,
                    outcome: "blocked".into(),
                    may_have_executed: false,
                    retry_same_request_only: false,
                    retry_after_ms: 0.0,
                    semantic_mismatch: false,
                })
            }),
            validate_lease: Box::new(move |l: &V, _s: &V| -> Result<bool, OrderTransportError> {
                let actual = basic(&inner_l).map_err(|e| OrderTransportError {
                    code: e,
                    status: None,
                    api_code: None,
                    outcome: "blocked".into(),
                    may_have_executed: false,
                    retry_same_request_only: false,
                    retry_after_ms: 0.0,
                    semantic_mismatch: false,
                })?;
                Ok(payload_hash(l).unwrap_or_default() == payload_hash(&actual).unwrap_or_default())
            }),
            now: Box::new({
                let d = inner.borrow().deps.now.clone();
                move || d()
            }),
            max_pages: 10,
        });
        Ok(OwnedNoExitController { inner, transport: transport.map_err(|e| e.code)? })
    })();
    match open_result {
        Ok(c) => Ok(c),
        Err(e) => {
            let _ = lease_rc.borrow_mut().release();
            Err(e)
        }
    }
}

fn basic_guard_dir(
    _dir: &Path,
    deps: &OwnedNoExitControllerDeps,
    buy_proof: &V,
    lease: &Rc<RefCell<baal_service_runtime::ledger::OwnerLease>>,
) -> Result<(), String> {
    need((deps.can_operate)(), "STOPPED_OR_FAILED")?;
    lease.borrow().assert_ownership().map_err(|e| format!("{PREFIX}{}", e.message))?;
    let l = (deps.assert_account_lease)()?;
    need(
        get(&l, "ownerId") == get(buy_proof, "ownerId")
            && get(&l, "tournamentId") == get(buy_proof, "tournamentId")
            && get(&l, "leaseId").as_str().map(|s| !s.is_empty()).unwrap_or(false),
        "ACCOUNT_LEASE",
    )?;
    Ok(())
}

fn wrap_dispatch(
    inner: &Rc<RefCell<CtrlInner>>,
    spec: &V,
    bd: Option<&Guard<'_>>,
    bn: Option<&Guard<'_>>,
) -> Result<V, String> {
    let o = {
        let g = inner.borrow();
        get(&g.state.clone().unwrap_or(V::Null), "intent").clone()
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
                "beforeNetworkPtr": bn_ptr as u64,
            }),
        );
        g.in_flight = Some(spec.clone());
    }
    let capture = |response: &V| -> Result<(), String> {
        let meta = json!({
            "method": get(spec, "method"),
            "requestBodyHash": if get(spec, "method").as_str() == Some("POST") {
                get(&o, "bodyHash").clone()
            } else {
                V::Null
            },
        });
        let receipt = raw(
            &(inner.borrow().deps.raw_receipt_of)(response, &meta)?,
            get(spec, "method").as_str().unwrap_or(""),
            get(spec, "endpoint").as_str().unwrap_or(""),
        )?;
        let (parsed, _) = parse_number_lexemes(get(&receipt, "bodyText").as_str().unwrap_or(""))?;
        need(
            instant(get(&receipt, "requestStartedAt").as_str().unwrap_or(""))?
                >= instant(get(&response, "requestStartedAt").as_str().unwrap_or(""))?
                && instant(get(&receipt, "receivedAt").as_str().unwrap_or(""))?
                    <= instant(get(&response, "receivedAt").as_str().unwrap_or(""))?
                && get(&receipt, "status") == get(&response, "status")
                && payload_hash(&parsed)? == payload_hash(get(&response, "data"))?,
            "RAW_WIRE_BINDING",
        )?;
        append_event(
            inner,
            &json!({
                "kind": "wire",
                "receipt": receipt,
                "at": iso((inner.borrow().deps.now)()),
            }),
        )?;
        Ok(())
    };
    let dispatch = inner.borrow().deps.dispatch.clone();
    let result = dispatch(spec, bd, bn);
    let out = match result {
        Ok(response) => {
            capture(&response)?;
            Ok(response)
        }
        Err(e) => {
            if let Some(wr) = e.wire_receipt {
                capture(&wr)?;
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

impl OwnedNoExitController {
    pub fn authorizes(&self, spec: &V, before_dispatch: Option<&Guard<'_>>, before_network: Option<&Guard<'_>>) -> bool {
        let r: Result<bool, String> = (|| {
            basic(&self.inner)?;
            let bd_ptr = before_dispatch.map(|x| x as *const Guard as *const () as usize).unwrap_or(0);
            let bn_ptr = before_network.map(|x| x as *const Guard as *const () as usize).unwrap_or(0);
            let p = self.inner.borrow().issued.get(&bd_ptr).cloned();
            Ok(p.is_some()
                && get(p.as_ref().unwrap(), "method") == get(spec, "method")
                && get(p.as_ref().unwrap(), "endpoint") == get(spec, "endpoint")
                && get(p.as_ref().unwrap(), "bodyJson") == get(spec, "bodyJson")
                && get(p.as_ref().unwrap(), "beforeNetworkPtr") == &json!(bn_ptr as u64)
                && self.transport.authorize_request(spec).map(|x| x).unwrap_or(false))
        })();
        r.unwrap_or(false)
    }

    pub fn on_fetch_start(&self, meta: &V) -> Result<bool, String> {
        if get(meta, "method").as_str() == Some("GET") || self.inner.borrow().in_flight.is_none() {
            return Ok(false);
        }
        let result = (|| -> Result<bool, String> {
            basic(&self.inner)?;
            let (o, f, in_flight) = {
                let g = self.inner.borrow();
                let st = g.state.clone().unwrap_or(V::Null);
                let o = get(&st, "intent").clone();
                let f = get(&o, "fences").as_array().and_then(|x| x.last()).cloned().unwrap_or(V::Null);
                (o, f, g.in_flight.clone().unwrap())
            };
            need(
                get(meta, "method") == get(&in_flight, "method")
                    && get(meta, "endpoint") == get(&in_flight, "endpoint")
                    && get(meta, "requestBodyHash")
                        == &(if get(meta, "method").as_str() == Some("POST") {
                            get(&o, "bodyHash").clone()
                        } else {
                            V::Null
                        }),
                "FETCH_BINDING",
            )?;
            let mut start = meta.clone();
            let start_obj = start.as_object_mut().unwrap();
            start_obj.insert("operation".into(), get(&o, "operation").clone());
            start_obj.insert("intentId".into(), get(&o, "intentId").clone());
            start_obj.insert("bodyHash".into(), get(&o, "bodyHash").clone());
            start_obj.insert("leaseHash".into(), get(&f, "leaseHash").clone());
            append_event(
                &self.inner,
                &json!({
                    "kind": "fetch_start",
                    "start": start,
                    "at": iso((self.inner.borrow().deps.now)()),
                }),
            )?;
            Ok(true)
        })();
        match result {
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
        let o = get(&state, "intent");
        let p = get(o, "proof");
        let fetch_starts = get(o, "fetchStarts").as_array().cloned().unwrap_or_default();
        let mut intent = V::Null;
        if !o.is_null() {
            intent = json!({
                "intentId": get(o, "intentId"),
                "orderId": get(o, "orderId"),
                "body": get(o, "body"),
                "bodyHash": get(o, "bodyHash"),
                "createdAt": get(o, "createdAt"),
                "semanticMismatch": get(o, "semanticMismatch"),
                "cancelPending": get(o, "cancelPending"),
                "actualFetchCount": fetch_starts.len(),
                "lastActualFetchStartAt": fetch_starts.last().and_then(|f| f.get("requestStartedAt")).cloned().unwrap_or(V::Null),
                "proofHash": get(p, "verificationHash"),
            });
        }
        json!({
            "mode": get(&state, "mode"),
            "ownerId": get(&g.buy_proof, "ownerId"),
            "runId": get(&g.buy_proof, "runId"),
            "tournamentId": get(&g.buy_proof, "tournamentId"),
            "marketId": get(&g.buy_proof, "marketId"),
            "exchangeId": get(&g.buy_proof, "exchangeId"),
            "lotId": get(&get(&g.buy_proof, "lot"), "lotId"),
            "buyProofHash": get(&g.buy_proof, "verificationHash"),
            "originalPrincipalCost": get(&g.buy_proof, "scopedPrincipalCost"),
            "sequence": g.journal.sequence,
            "hash": g.journal.previous.clone(),
            "phase": if o.is_null() { json!("owned") } else { get(o, "phase").clone() },
            "remainingOwnedLot": if !p.is_null() { get(p, "remainingOwnedLot").clone() } else { get(&g.buy_proof, "lot").clone() },
            "realizedPnl": if !p.is_null() { get(p, "realizedPnl").clone() } else { json!("0.000000") },
            "scopedProceeds": if !p.is_null() { get(p, "scopedProceeds").clone() } else { json!("0.000000") },
            "intent": intent,
            "reservation": claim(&state).unwrap_or(V::Null),
            "failed": g.failed,
            "closed": g.closed,
            "busy": g.busy,
            "additionalApiClients": 0,
        })
    }

    fn run<F>(&self, f: F) -> Result<V, String>
    where
        F: FnOnce(&Self) -> Result<V, String>,
    {
        need(!self.inner.borrow().busy, "BUSY")?;
        self.inner.borrow_mut().busy = true;
        let result = f(self);
        self.inner.borrow_mut().busy = false;
        result
    }

    fn dispatch_order(&self, operation: &str) -> Result<V, String> {
        self.run(|_| {
            let l = basic(&self.inner)?;
            let o = get(&self.inner.borrow().state.clone().unwrap_or(V::Null), "intent").clone();
            need(
                !o.is_null()
                    && get(&o, "proof").is_null()
                    && (if operation == "place" {
                        get(&o, "phase").as_str() == Some("prepared")
                    } else {
                        !get(&o, "orderId").is_null()
                            && !js_truthy(o.get("cancelPending"))
                            && !js_truthy(o.get("semanticMismatch"))
                    }),
                "UNKNOWN_OR_TERMINAL_BLOCKS_ORDER",
            )?;
            self.reserve()?;
            let permit = json!({
                "intentId": get(&o, "intentId"),
                "bodyHash": get(&o, "bodyHash"),
                "operation": operation,
            });
            let authority = json!({"permit": permit, "lease": l});
            let result = if operation == "place" {
                self.transport.place(get(&o, "body"), &authority)
            } else {
                self.transport.cancel(get(&o, "orderId"), &authority)
            };
            match result {
                Ok(_) => Ok(self.status()),
                Err(e) => {
                    if self.inner.borrow().failed {
                        return Err(format!("{PREFIX}DURABLE_FAILURE"));
                    }
                    if (self.inner.borrow().deps.can_operate)() {
                        let code = e.code.clone();
                        let code = if code.chars().all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()) && !code.is_empty() {
                            code
                        } else {
                            "REQUEST_UNCONFIRMED".into()
                        };
                        append_event(
                            &self.inner,
                            &json!({
                                "kind": "error",
                                "code": code,
                                "at": iso((self.inner.borrow().deps.now)()),
                            }),
                        )?;
                    }
                    Err(e.code)
                }
            }
        })
    }

    fn reserve(&self) -> Result<V, String> {
        let state = self.inner.borrow().state.clone().unwrap_or(V::Null);
        let receipt = (self.inner.borrow().deps.install_exit_reservation)(&claim(&state)?)?;
        append_event(
            &self.inner,
            &json!({
                "kind": "reservation",
                "receipt": receipt,
                "at": iso((self.inner.borrow().deps.now)()),
            }),
        )?;
        Ok(receipt)
    }

    pub fn exit_reservation(&self) -> V {
        let state = self.inner.borrow().state.clone().unwrap_or(V::Null);
        claim(&state).unwrap_or(V::Null)
    }

    pub fn exit_reservation_status(&self) -> V {
        let state = self.inner.borrow().state.clone().unwrap_or(V::Null);
        json!({
            "record": claim(&state).unwrap_or(V::Null),
            "receipt": get(&state, "reservation"),
        })
    }

    pub fn verified_exit_proof(&self) -> V {
        let state = self.inner.borrow().state.clone().unwrap_or(V::Null);
        get(&get(&state, "intent"), "proof").clone()
    }

    pub fn prepare(&self) -> Result<V, String> {
        self.run(|_| {
            basic(&self.inner)?;
            need(
                get(&self.inner.borrow().state.clone().unwrap_or(V::Null), "intent").is_null(),
                "ONE_ORDER_ONLY",
            )?;
            let e = evidence(&self.inner, "prepare")?;
            let ownership = (self.inner.borrow().deps.get_ownership)()?;
            let n = (self.inner.borrow().deps.now)();
            let st = self.inner.borrow().state.clone().unwrap_or(V::Null);
            let signal = preflight(&st, &e, &ownership, n)?;
            append_event(
                &self.inner,
                &json!({
                    "kind": "prepared",
                    "intentId": format!("owned-no-exit-{}", uuid::Uuid::new_v4()),
                    "idempotencyKey": uuid::Uuid::new_v4().to_string(),
                    "evidence": e,
                    "ownership": ownership,
                    "signal": signal,
                    "at": iso(n),
                }),
            )?;
            self.reserve()?;
            Ok(self.status())
        })
    }

    pub fn dispatch_prepared(&self) -> Result<V, String> {
        self.dispatch_order("place")
    }

    pub fn cancel_expired(&self) -> Result<V, String> {
        self.dispatch_order("cancel")
    }

    pub fn reconcile(&self, evidence: &V) -> Result<V, String> {
        self.run(|_| {
            basic(&self.inner)?;
            append_event(
                &self.inner,
                &json!({
                    "kind": "reconciled",
                    "evidence": evidence,
                    "at": iso((self.inner.borrow().deps.now)()),
                }),
            )?;
            self.reserve()?;
            Ok(self.status())
        })
    }

    pub fn close(&self) -> Result<(), String> {
        let mut g = self.inner.borrow_mut();
        need(!g.busy, "BUSY")?;
        if g.closed {
            return Ok(());
        }
        g.closed = true;
        let _ = g.lease.borrow_mut().release();
        Ok(())
    }
}
