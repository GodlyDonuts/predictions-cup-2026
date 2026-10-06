// execution/owned-no-exit-controller-v2.mjs — explicit repair of one already
// closed, filled SELL ack. The original byte prefix replays under its original
// code identity and null-only semantics. Cannot create, retry or cancel any
// competition order.
use crate::core::{canonical_payload, payload_hash};
use crate::gates::normalize_official_fill_tick;
use crate::no_rest_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::no_neutral_sell_all_v1::inspect_no_sell_ack_all_economics;
use crate::no_rest_sell_reconciliation_v1::verify_scoped_rest_no_sell;
use crate::no_rest_sell_reconciliation_v2::verify_scoped_rest_no_sell_v2;
use crate::receipts::parse_number_lexemes;
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

// identity(): [execution/owned-no-exit-controller-v2.mjs, v1.mjs]
fn identity() -> Result<V, String> {
    // JS resolves the workspace as <module dir>/.. — here the manifest dir is
    // backend-rust/execution, so ../.. is the repo root holding execution/*.mjs.
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    crate::frozen::collect_frozen_paper_files(&workspace, &["execution/owned-no-exit-controller-v2.mjs", "execution/owned-no-exit-controller-v1.mjs"])
}

// legacyIdentity(): [execution/owned-no-exit-controller-v1.mjs]
fn legacy_identity() -> Result<V, String> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    crate::frozen::collect_frozen_paper_files(&workspace, &["execution/owned-no-exit-controller-v1.mjs"])
}

// ownedNoExitControllerV2Identity()
pub fn owned_no_exit_controller_v2_identity() -> Result<V, String> {
    identity()
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

// legacyReduce(s,e) — v1 journal event fold for the byte-sealed prefix.
fn legacy_reduce(s: Option<V>, e: &V) -> Result<V, String> {
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


// canonicalNeutralAck(s,r): same checks as canonicalAck plus the neutral-all
// ack economics inspector (allState must be 'observed_neutral_sell').
fn canonical_neutral_ack(s: &V, r: &V) -> bool {
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
            && js_int(get(&d, "orderId")).map(|x| x.to_string())
                == get(o, "orderId").as_str().map(|x| x.to_string())
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
            && inspect_no_sell_ack_all_economics(r)
                .map(|e| get(&e, "allState").as_str() == Some("observed_neutral_sell"))
                .unwrap_or(false)
            && get(&d, "open").is_boolean()
            && fill_ok)
    })()
    .unwrap_or(false)
}

// reduce(s,e) — v2 event fold: the revision event repairs the semantic
// mismatch; every later event requires the revision and only 'reservation'
// (legacy) and 'reconciled' (v2 verifier) are admitted.
fn reduce(s: Option<V>, e: &V) -> Result<V, String> {
    let kind = get(e, "kind").as_str().unwrap_or("");
    let sv = s.clone().unwrap_or(V::Null);
    if kind == "neutral_sell_ack_revision" {
        let o = get(&sv, "intent");
        need(
            !o.is_null()
                && get(o, "phase").as_str() == Some("semantic_mismatch")
                && get(o, "semanticMismatch") == &json!(true)
                && get(o, "proof").is_null()
                && !js_truthy(o.get("cancelPending"))
                && get(o, "orderId") == get(e, "orderId")
                && get(&get(o, "ack"), "sourceBodyHash") == get(e, "sourceBodyHash")
                && instant(get(e, "at").as_str().unwrap_or(""))?
                    >= instant(get(&get(o, "ack"), "receivedAt").as_str().unwrap_or(""))?
                && canonical_neutral_ack(&sv, get(o, "ack")),
            "NEUTRAL_SELL_REVISION_BINDING",
        )?;
        let economics = inspect_no_sell_ack_all_economics(get(o, "ack"))?;
        need(
            get(&economics, "allState").as_str() == Some("observed_neutral_sell")
                && payload_hash(&economics)? == get(e, "economicsProofHash").as_str().unwrap_or("")
                && get(&sv, "reconciliationRevision").is_null(),
            "NEUTRAL_SELL_REVISION_PROOF",
        )?;
        let mut n = sv.clone();
        {
            let m = n.as_object_mut().unwrap();
            m["intent"].as_object_mut().unwrap().insert("semanticMismatch".into(), json!(false));
            m["intent"].as_object_mut().unwrap().insert("phase".into(), json!("acknowledged"));
            m.insert(
                "reconciliationRevision".into(),
                json!({
                    "kind": "neutral_sell_ack_v2",
                    "revisionHash": get(e, "revisionHash"),
                    "originalPrefixBytes": get(e, "originalPrefixBytes"),
                    "originalPrefixHash": get(e, "originalPrefixHash"),
                    "orderId": get(e, "orderId"),
                    "sourceBodyHash": get(e, "sourceBodyHash"),
                    "economicsProofHash": get(e, "economicsProofHash"),
                    "at": get(e, "at"),
                }),
            );
        }
        return Ok(n);
    }
    need(
        s.clone().map(|v| !get(&v, "reconciliationRevision").is_null()).unwrap_or(false),
        "EXPLICIT_REVISION_REQUIRED",
    )?;
    if kind == "reservation" {
        return legacy_reduce(s, e);
    }
    let o = get(&sv, "intent");
    need(
        kind == "reconciled"
            && !get(o, "orderId").is_null()
            && !get(o, "ack").is_null()
            && !js_truthy(o.get("semanticMismatch"))
            && get(o, "proof").is_null(),
        "RECONCILE_ONLY",
    )?;
    fields(get(e, "evidence"), &["order", "fillPages", "after", "globalRisk"])?;
    let mut verify_input = get(e, "evidence").clone();
    {
        let verify_obj = verify_input.as_object_mut().unwrap();
        verify_obj.insert("binding".into(), binding(&sv)?);
        verify_obj.insert("originalBuyProof".into(), get(&sv, "buyProof").clone());
        verify_obj.insert("originalAfter".into(), get(&sv, "originalAfter").clone());
        verify_obj.insert("before".into(), get(o, "before").clone());
        verify_obj.insert("ack".into(), get(o, "ack").clone());
        verify_obj.insert("readAt".into(), get(e, "at").clone());
    }
    let p = verify_scoped_rest_no_sell_v2(&verify_input)?;
    need(
        get(&p, "originalBuyProofHash") == get(&get(&sv, "buyProof"), "verificationHash")
            && get(&p, "originalLotId") == get(&get(&get(&sv, "buyProof"), "lot"), "lotId")
            && get(&p, "filledQuantity") == &json!(1)
            && amount(&js_str(&get(&p, "realizedPnl")))? >= 5000
            && get(&p, "remainingOwnedLot") == &V::Null,
        "EXIT_PROOF_OR_MINIMUM",
    )?;
    let mut n = sv.clone();
    {
        let m = n.as_object_mut().unwrap();
        m["intent"].as_object_mut().unwrap().insert("proof".into(), p);
        m["intent"].as_object_mut().unwrap().insert("phase".into(), json!("reconciled"));
        m["intent"].as_object_mut().unwrap().insert("cancelPending".into(), json!(false));
    }
    Ok(n)
}

#[allow(clippy::type_complexity)]
pub struct OwnedNoExitControllerV2Deps {
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub assert_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub install_exit_reservation: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub neutral_sell_revision: V,
    pub now: Rc<dyn Fn() -> i64>,
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
    legacy_code: V,
    revision: V,
    revision_hash: String,
    deps: OwnedNoExitControllerV2Deps,
    lease: baal_service_runtime::ledger::OwnerLease,
    closed: bool,
    failed: bool,
    busy: bool,
    size: usize,
    bytes_hash: String,
    ino: u64,
    birthtime_ms: f64,
}

pub struct OwnedNoExitControllerV2 {
    inner: Rc<RefCell<CtrlInner>>,
}

fn basic(inner: &Rc<RefCell<CtrlInner>>) -> Result<V, String> {
    let g = inner.borrow();
    need(
        !g.closed && !g.failed && (g.deps.can_operate)(),
        "STOPPED_OR_FAILED",
    )?;
    let mut g = inner.borrow_mut();
    g.lease.assert_ownership().map_err(|e| format!("{PREFIX}{}", e.message))?;
    drop(g);
    let g = inner.borrow();
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

// eventRevision(e): a journal 'neutral_sell_ack_revision' row must carry the
// request descriptor exactly.
fn event_revision(inner: &Rc<RefCell<CtrlInner>>, e: &V) -> Result<(), String> {
    let g = inner.borrow();
    need(
        get(e, "revisionHash").as_str() == Some(g.revision_hash.as_str())
            && get(e, "orderId") == get(&g.revision, "orderId")
            && get(e, "sourceBodyHash") == get(&g.revision, "ackSourceBodyHash")
            && get(e, "originalPrefixHash") == get(&g.revision, "originalPrefixHash")
            && get(e, "originalPrefixBytes") == get(&g.revision, "originalPrefixBytes"),
        "REVISION_EVENT_CHANGED",
    )
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
    let (code, legacy_code) = {
        let g = inner.borrow();
        (g.code.clone(), g.legacy_code.clone())
    };
    need(
        st_ino == ino
            && st_birth == birthtime_ms
            && before.len() == size
            && sha256(&before) == bytes_hash
            && payload_hash(&identity().unwrap_or(V::Null)).unwrap_or_default()
                == payload_hash(&code).unwrap_or_default()
            && payload_hash(&legacy_identity().unwrap_or(V::Null)).unwrap_or_default()
                == payload_hash(&legacy_code).unwrap_or_default(),
        "JOURNAL_CHANGED",
    )?;
    if get(input, "kind").as_str() == Some("neutral_sell_ack_revision") {
        event_revision(inner, input)?;
    }
    let next = reduce(state.clone(), input)?;
    need(
        size + js_stringify(&next).len() + js_stringify(input).len() < 64 * 1024 * 1024,
        "JOURNAL_BOUND",
    )?;
    let result = (|| -> Result<(), String> {
        {
            let code = inner.borrow().code.clone();
            let mut g = inner.borrow_mut();
            g.journal
                .commit(&next, &json!({"codeIdentity": code, "input": input.clone()}))?;
        }
        basic(inner)?;
        let after = fs::read(&file).map_err(|e| e.to_string())?;
        need(&after[..size] == before.as_slice(), "JOURNAL_PREFIX_CHANGED")?;
        let mut g = inner.borrow_mut();
        g.state = Some(next);
        g.size = after.len();
        g.bytes_hash = sha256(&after);
        Ok(())
    })();
    if result.is_err() {
        inner.borrow_mut().failed = true;
    }
    result
}

// openOwnedNoExitControllerV2({dir,buyProof,originalAfter,tournamentSlug,canOperate,assertAccountLease,installExitReservation,neutralSellRevision,now})
pub fn open_owned_no_exit_controller_v2(
    dir: &Path,
    buy_proof: &V,
    original_after: &V,
    tournament_slug: &str,
    deps: OwnedNoExitControllerV2Deps,
) -> Result<OwnedNoExitControllerV2, String> {
    need(dir.is_absolute(), "DEPENDENCIES")?;
    let buy_proof = buy_proof.clone();
    let original_after = original_after.clone();
    original(&buy_proof, &original_after, &json!(tournament_slug))?;
    let revision = deps.neutral_sell_revision.clone();
    fields(
        &revision,
        &[
            "kind", "ownerId", "runId", "tournamentId", "lotId", "buyProofHash",
            "orderId", "originalPrefixBytes", "originalPrefixHash", "ackSourceBodyHash",
        ],
    )?;
    need(
        get(&revision, "kind").as_str() == Some("same_run_neutral_all_sell_ack_reconciliation")
            && get(&revision, "ownerId") == get(&buy_proof, "ownerId")
            && get(&revision, "runId") == get(&buy_proof, "runId")
            && get(&revision, "tournamentId") == get(&buy_proof, "tournamentId")
            && get(&revision, "lotId") == get(&get(&buy_proof, "lot"), "lotId")
            && get(&revision, "buyProofHash") == get(&buy_proof, "verificationHash")
            && get(&revision, "orderId").as_str().is_some_and(|s| {
                !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
            })
            && js_int(get(&revision, "originalPrefixBytes")).is_some_and(|b| b > 0)
            && ["originalPrefixHash", "ackSourceBodyHash"]
                .iter()
                .all(|k| hex(get(&revision, k))),
        "REVISION_DESCRIPTOR",
    )?;
    let revision_hash = payload_hash(&revision)?;
    let file = dir.join("owned-no-exit-journal.jsonl");
    need(file.exists(), "EXISTING_JOURNAL_REQUIRED")?;
    let code = identity()?;
    let legacy_code = legacy_identity()?;
    let lease = acquire_owner_lease(
        dir,
        get(&buy_proof, "ownerId").as_str().unwrap_or(""),
        get(&buy_proof, "runId").as_str().unwrap_or(""),
        &format!(
            "single-owned-no-exit:{}",
            get(&get(&buy_proof, "lot"), "lotId").as_str().unwrap_or("")
        ),
        (deps.now)() as f64,
    )
    .map_err(|e| format!("{PREFIX}{}", e.message))?;
    let inner = Rc::new(RefCell::new(CtrlInner {
        state: None,
        buy_proof,
        original_after,
        tournament_slug: json!(tournament_slug),
        dir: dir.to_path_buf(),
        file,
        journal: Journal::open(&dir.join("owned-no-exit-journal.jsonl"))?,
        code,
        legacy_code,
        revision,
        revision_hash,
        deps,
        lease,
        closed: false,
        failed: false,
        busy: false,
        size: 0,
        bytes_hash: String::new(),
        ino: 0,
        birthtime_ms: 0.0,
    }));
    let mut opened_lease = false;
    let result = (|| -> Result<(), String> {
        {
            let g = inner.borrow();
            let _ = &g;
        }
        // run basic() before touching the journal, exactly like JS.
        basic(&inner)?;
        opened_lease = true;
        let file = inner.borrow().file.clone();
        let bytes = fs::read(&file).map_err(|e| e.to_string())?;
        let prefix_bytes =
            js_int(get(&inner.borrow().revision, "originalPrefixBytes")).unwrap_or(0) as usize;
        need(
            bytes.len() <= 64 * 1024 * 1024
                && bytes.last() == Some(&b'\n')
                && prefix_bytes <= bytes.len()
                && bytes[prefix_bytes - 1] == b'\n'
                && sha256(&bytes[..prefix_bytes])
                    == get(&inner.borrow().revision, "originalPrefixHash")
                        .as_str()
                        .unwrap_or(""),
            "REVISION_PREFIX_BINDING",
        )?;
        let mut seen = 0usize;
        for line in String::from_utf8_lossy(&bytes).split('\n').filter(|l| !l.is_empty()) {
            let row: V = serde_json::from_str(line).map_err(|_| format!("{PREFIX}REPLAY"))?;
            let length = line.len() + 1;
            let legacy = seen < prefix_bytes;
            need(!legacy || seen + length <= prefix_bytes, "REVISION_PARTIAL_ROW")?;
            let (code, legacy_code) = {
                let g = inner.borrow();
                (g.code.clone(), g.legacy_code.clone())
            };
            need(
                payload_hash(get(&get(&row, "event"), "codeIdentity"))?
                    == payload_hash(if legacy { &legacy_code } else { &code })?,
                "CODE_CHANGED",
            )?;
            if !legacy && get(&get(&row, "event"), "input").get("kind") == Some(&json!("neutral_sell_ack_revision")) {
                event_revision(&inner, get(&get(&row, "event"), "input"))?;
            }
            let next = {
                let g = inner.borrow();
                if legacy {
                    legacy_reduce(g.state.clone(), get(&get(&row, "event"), "input"))?
                } else {
                    reduce(g.state.clone(), get(&get(&row, "event"), "input"))?
                }
            };
            need(
                payload_hash(&next)? == payload_hash(get(&row, "state"))?,
                "REPLAY",
            )?;
            inner.borrow_mut().state = Some(next);
            seen += length;
        }
        {
            let g = inner.borrow();
            need(
                seen == bytes.len()
                    && payload_hash(get(&g.state.clone().unwrap_or(V::Null), "buyProof"))?
                        == payload_hash(&g.buy_proof)?
                    && payload_hash(get(&g.state.clone().unwrap_or(V::Null), "originalAfter"))?
                        == payload_hash(&g.original_after)?
                    && get(&g.state.clone().unwrap_or(V::Null), "tournamentSlug")
                        == &g.tournament_slug,
                "ORIGINAL_BINDING",
            )?;
        }
        let (ino, birth) = file_stat(&file)?;
        {
            let mut g = inner.borrow_mut();
            g.size = bytes.len();
            g.bytes_hash = sha256(&bytes);
            g.ino = ino;
            g.birthtime_ms = birth;
        }
        // Append the revision event if the journal does not carry it yet.
        if get(&inner.borrow().state.clone().unwrap_or(V::Null), "reconciliationRevision").is_null() {
            let o = {
                let g = inner.borrow();
                get(&g.state.clone().unwrap_or(V::Null), "intent").clone()
            };
            {
                let g = inner.borrow();
                need(
                    get(&o, "orderId") == get(&g.revision, "orderId")
                        && get(&get(&o, "ack"), "sourceBodyHash")
                            == get(&g.revision, "ackSourceBodyHash"),
                    "REVISION_ORDER_BINDING",
                )?;
            }
            let economics = inspect_no_sell_ack_all_economics(get(&o, "ack"))?;
            append_event(
                &inner,
                &json!({
                    "kind": "neutral_sell_ack_revision",
                    "revisionHash": inner.borrow().revision_hash,
                    "orderId": get(&o, "orderId"),
                    "sourceBodyHash": get(&get(&o, "ack"), "sourceBodyHash"),
                    "economicsProofHash": payload_hash(&economics)?,
                    "originalPrefixBytes": get(&inner.borrow().revision, "originalPrefixBytes"),
                    "originalPrefixHash": get(&inner.borrow().revision, "originalPrefixHash"),
                    "at": iso((inner.borrow().deps.now)()),
                }),
            )?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        if opened_lease {
            let _ = inner.borrow_mut().lease.release();
        } else {
            // lease was acquired before basic(); JS releases unconditionally on
            // catch — mirror that even if basic failed.
            let _ = inner.borrow_mut().lease.release();
        }
        return Err(e);
    }
    Ok(OwnedNoExitControllerV2 { inner })
}

fn status(inner: &Rc<RefCell<CtrlInner>>) -> V {
    let g = inner.borrow();
    let s = g.state.clone().unwrap_or(V::Null);
    let o = get(&s, "intent");
    let p = get(o, "proof");
    json!({
        "mode": "single_owned_no_exit_controller_v2_reconciliation_only",
        "ownerId": get(&g.buy_proof, "ownerId"),
        "runId": get(&g.buy_proof, "runId"),
        "tournamentId": get(&g.buy_proof, "tournamentId"),
        "marketId": get(&g.buy_proof, "marketId"),
        "exchangeId": get(&g.buy_proof, "exchangeId"),
        "lotId": get(&get(&g.buy_proof, "lot"), "lotId"),
        "buyProofHash": get(&g.buy_proof, "verificationHash"),
        "originalPrincipalCost": get(&g.buy_proof, "scopedPrincipalCost"),
        "sequence": g.journal.sequence as i64,
        "hash": g.journal.previous,
        "phase": get(o, "phase"),
        "remainingOwnedLot": if p.is_null() { get(&get(&g.buy_proof, "lot"), "lotId").clone().pipe(|_| get(&g.buy_proof, "lot").clone()) } else { get(p, "remainingOwnedLot").clone() },
        "realizedPnl": if p.is_null() { json!("0.000000") } else { get(p, "realizedPnl").clone() },
        "scopedProceeds": if p.is_null() { json!("0.000000") } else { get(p, "scopedProceeds").clone() },
        "intent": {
            "intentId": get(o, "intentId"),
            "orderId": get(o, "orderId"),
            "body": get(o, "body"),
            "bodyHash": get(o, "bodyHash"),
            "createdAt": get(o, "createdAt"),
            "semanticMismatch": get(o, "semanticMismatch"),
            "cancelPending": get(o, "cancelPending"),
            "actualFetchCount": get(o, "fetchStarts").as_array().map(|x| x.len() as i64).unwrap_or(0),
            "lastActualFetchStartAt": get(o, "fetchStarts").as_array().and_then(|x| x.last()).map(|v| get(v, "requestStartedAt").clone()).unwrap_or(V::Null),
            "proofHash": get(p, "verificationHash"),
        },
        "reservation": claim(&s).unwrap_or(V::Null),
        "revision": get(&s, "reconciliationRevision"),
        "failed": g.failed,
        "closed": g.closed,
        "busy": g.busy,
        "writeCapability": false,
        "newSubmissionAuthorized": false,
        "additionalApiClients": 0,
    })
}

trait Pipe: Sized {
    fn pipe<R>(self, f: impl FnOnce(Self) -> R) -> R {
        f(self)
    }
}
impl Pipe for V {}

impl OwnedNoExitControllerV2 {
    pub fn authorizes(&self, _spec: &V) -> bool {
        false
    }
    pub fn on_fetch_start(&self, _meta: &V) -> bool {
        false
    }
    pub fn status(&self) -> V {
        status(&self.inner)
    }
    pub fn exit_reservation(&self) -> V {
        let g = self.inner.borrow();
        claim(&g.state.clone().unwrap_or(V::Null)).unwrap_or(V::Null)
    }
    pub fn exit_reservation_status(&self) -> V {
        let g = self.inner.borrow();
        let s = g.state.clone().unwrap_or(V::Null);
        json!({
            "record": claim(&s).unwrap_or(V::Null),
            "receipt": get(&s, "reservation"),
        })
    }
    pub fn verified_exit_proof(&self) -> V {
        let g = self.inner.borrow();
        get(&get(&g.state.clone().unwrap_or(V::Null), "intent"), "proof").clone()
    }
    pub fn prepare(&self) -> Result<V, String> {
        Err("OWNED_NO_CONTROLLER_RECONCILIATION_ONLY".into())
    }
    pub fn dispatch_prepared(&self) -> Result<V, String> {
        Err("OWNED_NO_CONTROLLER_RECONCILIATION_ONLY".into())
    }
    pub fn cancel_expired(&self) -> Result<V, String> {
        Err("OWNED_NO_CONTROLLER_RECONCILIATION_ONLY".into())
    }
    pub fn refresh_reservation(&self) -> Result<V, String> {
        {
            let mut g = self.inner.borrow_mut();
            need(!g.busy, "BUSY")?;
            g.busy = true;
        }
        let r = (|| -> Result<V, String> {
            basic(&self.inner)?;
            let s = self.inner.borrow().state.clone().unwrap_or(V::Null);
            need(
                !get(&get(&s, "intent"), "proof").is_null()
                    && get(&get(&s, "intent"), "phase").as_str() == Some("reconciled"),
                "VERIFIED_EXIT_REQUIRED",
            )?;
            let receipt = (self.inner.borrow().deps.install_exit_reservation)(&claim(&s)?)?;
            append_event(
                &self.inner,
                &json!({
                    "kind": "reservation",
                    "receipt": receipt,
                    "at": iso((self.inner.borrow().deps.now)()),
                }),
            )?;
            Ok(status(&self.inner))
        })();
        self.inner.borrow_mut().busy = false;
        r
    }
    pub fn reconcile(&self, evidence: &V) -> Result<V, String> {
        {
            let mut g = self.inner.borrow_mut();
            need(!g.busy, "BUSY")?;
            g.busy = true;
        }
        let r = (|| -> Result<V, String> {
            basic(&self.inner)?;
            append_event(
                &self.inner,
                &json!({
                    "kind": "reconciled",
                    "evidence": evidence,
                    "at": iso((self.inner.borrow().deps.now)()),
                }),
            )?;
            let s = self.inner.borrow().state.clone().unwrap_or(V::Null);
            let receipt = (self.inner.borrow().deps.install_exit_reservation)(&claim(&s)?)?;
            append_event(
                &self.inner,
                &json!({
                    "kind": "reservation",
                    "receipt": receipt,
                    "at": iso((self.inner.borrow().deps.now)()),
                }),
            )?;
            Ok(status(&self.inner))
        })();
        self.inner.borrow_mut().busy = false;
        r
    }
    pub fn close(&self) -> Result<(), String> {
        {
            let mut g = self.inner.borrow_mut();
            need(!g.busy, "BUSY")?;
            if g.closed {
                return Ok(());
            }
            g.closed = true;
            let _ = g.lease.release();
        }
        Ok(())
    }
}
