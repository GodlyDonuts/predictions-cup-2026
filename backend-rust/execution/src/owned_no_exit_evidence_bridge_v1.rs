// execution/owned-no-exit-evidence-bridge-v1.mjs — read-only adapter over the
// caller's existing serialized client. Every GET is identity-fenced; no
// credential, independent client, rate limiter or timer.
use crate::core::payload_hash;
use crate::transport::{Guard, OrderTransportError};
use crate::gates::normalize_official_fill_tick;
use crate::no_rest_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::receipts::parse_number_lexemes;
use baal_service_runtime::contract::instant_ns;
use baal_service_runtime::fixed::{amount, instant};
use baal_service_runtime::json_text::{js_int, sha256};
use baal_service_runtime::market::normalize_book;
use serde_json::Value as V;
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok {
        return Err(format!("OWNED_NO_EXIT_EVIDENCE_{code}"));
    }
    Ok(())
}
fn sha(s: &str) -> String {
    sha256(s.as_bytes())
}
fn id(x: &V) -> bool {
    x.as_str()
        .map(|s| {
            !s.is_empty()
                && s.chars().all(|c| c.is_ascii_digit())
                && !s.starts_with('0')
                && s.parse::<i64>().is_ok()
        })
        .unwrap_or(false)
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
// String(x): numbers render without quotes.
fn js_str(v: &V) -> String {
    v.as_str()
        .map(|x| x.to_string())
        .or_else(|| v.as_i64().map(|x| x.to_string()))
        .or_else(|| v.as_u64().map(|x| x.to_string()))
        .or_else(|| v.as_f64().map(|x| x.to_string()))
        .unwrap_or_default()
}
fn integer(v: &V, min: i64) -> bool {
    js_int(v).is_some_and(|x| x >= min)
}
fn fresh(r: &V, n: i64) -> Result<(), String> {
    let a = instant(get(r, "requestStartedAt").as_str().unwrap_or(""))?;
    let z = instant(get(r, "receivedAt").as_str().unwrap_or(""))?;
    need(a <= z && z <= n && n - a <= 20000, "STALE_OR_FUTURE_RECEIPT")
}
fn urlencoding(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')' => {
                vec![c]
            }
            _ => {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf)
                    .bytes()
                    .flat_map(|b| format!("%{b:02X}").chars().collect::<Vec<char>>())
                    .collect()
            }
        })
        .collect()
}
fn exact_leg(scope: &V, v: &V) -> Result<V, String> {
    need(
        !v.is_null()
            && get(scope, "markets").as_array().is_some_and(|ms| {
                ms.iter().any(|m| {
                    get(m, "marketId") == get(v, "marketId") && get(m, "exchangeId") == get(v, "exchangeId")
                })
            }),
        "LEG_SCOPE",
    )?;
    Ok(json!({
        "marketId": get(v, "marketId"),
        "exchangeId": get(v, "exchangeId"),
    }))
}

pub type OwnedBridgeDispatch = dyn Fn(&V) -> Result<V, String>;

#[allow(clippy::type_complexity)]
pub struct OwnedNoExitBridgeDeps {
    pub dispatch: Rc<OwnedBridgeDispatch>,
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub get_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_owned_lot: Rc<dyn Fn() -> Result<V, String>>,
    pub get_exit_reservation: Rc<dyn Fn() -> Result<V, String>>,
    pub owns_order: Rc<dyn Fn(&V, &V) -> Result<bool, String>>,
    pub can_read: Rc<dyn Fn() -> bool>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> Result<bool, String>>,
    pub now: Rc<dyn Fn() -> i64>,
    #[allow(dead_code)]
    pub max_reads_per_operation: Option<BTreeMap<String, i64>>,
}

struct Watermark {
    start: i64,
    end: i64,
}

struct BookRow {
    sequence: V,
    source_at: V,
    request_started_at: V,
    version_key: V,
    bids: V,
    asks: V,
}

struct Inner {
    scope: V,
    original_hash: V,
    deps: OwnedNoExitBridgeDeps,
    cache: Option<V>,
    busy: bool,
    closed: bool,
    operation: Option<String>,
    reads: i64,
    last_failure: Option<String>,
    last_end: Option<i64>,
    issued: HashMap<usize, V>,
    receipt_watermarks: HashMap<String, Watermark>,
    book_watermarks: HashMap<String, BookRow>,
    projection_watermarks: HashMap<String, i64>,
    bounds: BTreeMap<String, i64>,
}

pub struct OwnedNoExitEvidenceBridge {
    inner: Rc<RefCell<Inner>>,
}

impl OwnedNoExitEvidenceBridge {
    fn local_bindings(inner: &Rc<RefCell<Inner>>, reservation_required: bool, remaining_required: bool) -> Result<V, String> {
        let (proof, installed, scope, n) = {
            let g = inner.borrow();
            (
                (g.deps.get_owned_lot)()?,
                (g.deps.get_exit_reservation)()?,
                g.scope.clone(),
                (g.deps.now)(),
            )
        };
        let mut body = proof.clone();
        let verification_hash = body
            .as_object_mut()
            .and_then(|m| m.remove("verificationHash"))
            .unwrap_or(V::Null);
        let leg = get(&scope, "markets").as_array().and_then(|a| a.first()).cloned().unwrap_or(V::Null);
        need(
            hex(&verification_hash)
                && payload_hash(&body)? == verification_hash.as_str().unwrap_or("")
                && verification_hash == inner.borrow().original_hash
                && ["scoped_rest_no_lot_with_rounded_cash_v1", "scoped_rest_no_lot_with_rounded_cash_neutral_all_v2"]
                    .contains(&get(&proof, "mode").as_str().unwrap_or(""))
                && get(&proof, "ownerId") == get(&scope, "ownerId")
                && get(&proof, "runId") == get(&scope, "runId")
                && get(&proof, "tournamentId") == get(&scope, "tournamentId")
                && get(&proof, "marketId") == get(&leg, "marketId")
                && get(&proof, "exchangeId") == get(&leg, "exchangeId")
                && get(&proof, "scopedLotVerified") == &json!(true)
                && get(&proof, "filledQuantity") == &json!(1)
                && get(&proof, "signedPositionDelta") == &json!(-1)
                && get(&get(&proof, "lot"), "side") == &json!("no")
                && get(&get(&proof, "lot"), "quantity") == &json!(1)
                && get(&get(&proof, "lot"), "ownerId") == get(&scope, "ownerId")
                && get(&get(&proof, "lot"), "runId") == get(&scope, "runId")
                && get(&get(&proof, "lot"), "entryPrice") == get(&proof, "scopedPrincipalCost"),
            "ORIGINAL_LOT_BINDING",
        )?;
        if installed.is_null() {
            need(!reservation_required, "EXIT_RESERVATION_REQUIRED")?;
            return Ok(proof);
        }
        let record = get(&installed, "record");
        let mut record_body = record.clone();
        let reservation_hash = record_body
            .as_object_mut()
            .and_then(|m| m.remove("reservationHash"))
            .unwrap_or(V::Null);
        need(
            hex(&reservation_hash)
                && payload_hash(&record_body)? == reservation_hash.as_str().unwrap_or("")
                && get(record, "schemaVersion") == &json!(1)
                && get(record, "kind") == &json!("single_owned_no_exit_reservation")
                && get(record, "ownerId") == get(&scope, "ownerId")
                && get(record, "runId") == get(&scope, "runId")
                && get(record, "tournamentId") == get(&scope, "tournamentId")
                && get(record, "marketId") == get(&leg, "marketId")
                && get(record, "exchangeId") == get(&leg, "exchangeId")
                && get(record, "lotId") == get(&get(&proof, "lot"), "lotId")
                && get(record, "buyProofHash") == &verification_hash
                && get(record, "originalPrincipalCost") == get(&proof, "scopedPrincipalCost")
                && get(record, "cashReserved") == &json!("0.000000")
                && [0, 1].contains(&js_int(get(record, "remainingQuantity")).unwrap_or(-1))
                && get(record, "terminalSell").is_boolean()
                && (get(record, "sellIntentId").is_null() || get(record, "sellIntentId").is_string()),
            "EXIT_RESERVATION_BINDING",
        )?;
        let receipt = get(&installed, "receipt");
        need(
            get(receipt, "reservationHash") == &reservation_hash
                && hex(get(receipt, "globalLedgerHash"))
                && instant(get(receipt, "confirmedAt").as_str().unwrap_or(""))? <= n
                && (!reservation_required
                    || n - instant(get(receipt, "confirmedAt").as_str().unwrap_or(""))? <= 20000),
            "EXIT_RESERVATION_RECEIPT",
        )?;
        if remaining_required {
            need(get(record, "remainingQuantity") == &json!(1), "OWNED_RESERVATION_REQUIRED")?;
        }
        Ok(proof)
    }

    fn allowed(inner: &Rc<RefCell<Inner>>) -> Result<(), String> {
        {
            let g = inner.borrow();
            need(!g.closed && (g.deps.can_read)(), "STOPPED")?;
        }
        Self::local_bindings(inner, false, false)?;
        Ok(())
    }

    fn own(inner: &Rc<RefCell<Inner>>, order_id: &V, leg: &V) -> Result<(), String> {
        need(
            id(order_id) && (inner.borrow().deps.owns_order)(order_id, leg)?,
            "ORDER_NOT_OWNED",
        )
    }

    fn clear_cache(inner: &Rc<RefCell<Inner>>) {
        inner.borrow_mut().cache = None;
    }

    fn local_global(inner: &Rc<RefCell<Inner>>) -> Result<V, String> {
        let (g_risk, scope, n) = {
            let b = inner.borrow();
            ((b.deps.get_global_risk)()?, b.scope.clone(), (b.deps.now)())
        };
        let g = g_risk;
        need(
            !g.is_null()
                && get(&g, "ownerId") == get(&scope, "ownerId")
                && get(&g, "runId") == get(&scope, "runId")
                && get(&g, "tournamentId") == get(&scope, "tournamentId")
                && get(&g, "complete") == &json!(true)
                && get(&g, "soleCollector") == &json!(true)
                && get(&g, "stopped") == &json!(false)
                && get(&g, "unresolvedIntentIds").is_array()
                && get(&g, "unresolvedIntentIds").as_array().unwrap().len() <= 1
                && hex(get(&g, "sourceHash"))
                && instant(get(&g, "at").as_str().unwrap_or(""))? <= n
                && n - instant(get(&g, "at").as_str().unwrap_or(""))? <= 20000,
            "GLOBAL_RISK_CLOCK_OR_SCOPE",
        )?;
        Ok(g)
    }

    fn validate_account(account: &V, leg: &V, scope: &V, n: i64) -> Result<V, String> {
        let mut binding = scope.clone();
        binding
            .as_object_mut()
            .unwrap()
            .insert("exchangeId".into(), get(leg, "exchangeId").clone());
        inspect_scoped_rest_no_account(&json!({
            "account": account,
            "binding": binding,
            "readAt": iso(n),
        }))
    }

    fn check_book(inner: &Rc<RefCell<Inner>>, r: &V, leg: &V, n: i64) -> Result<V, String> {
        fresh(r, n)?;
        let (data, _) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
        let scope = inner.borrow().scope.clone();
        let b = normalize_book(
            &data,
            &js_str(&get(leg, "marketId")),
            &js_str(&get(&scope, "tournamentId")),
            get(r, "receivedAt").as_str().unwrap_or(""),
            get(r, "requestStartedAt").as_str().unwrap_or(""),
            20000.0,
            &BTreeSet::new(),
            "authoritative_receipt_v1",
        )?;
        need(
            (!data.as_object().map(|o| o.contains_key("marketId")).unwrap_or(false)
                || get(&data, "marketId") == get(leg, "marketId"))
                && get(&data, "contexts").as_array().map(|a| a.len() == 1).unwrap_or(false)
                && get(&get(&data, "contexts")[0], "type") == &json!("tournament")
                && get(&get(&get(&data, "contexts")[0], "orderbook"), "exchanges")
                    .as_array()
                    .map(|a| a.len() == 1)
                    .unwrap_or(false)
                && get(&b, "exchangeId") == get(leg, "exchangeId")
                && get(&b, "bids").as_array().map(|a| a.len() <= 20).unwrap_or(false)
                && get(&b, "asks").as_array().map(|a| a.len() <= 20).unwrap_or(false),
            "BOOK_SCOPE",
        )?;
        let key = js_str(&get(leg, "exchangeId"));
        let old_exists = inner.borrow().book_watermarks.contains_key(&key);
        if old_exists {
            let old_seq;
            let old_source;
            let old_started;
            let old_key;
            let old_bids;
            let old_asks;
            {
                let g = inner.borrow();
                let old = g.book_watermarks.get(&key).unwrap();
                old_seq = old.sequence.clone();
                old_source = old.source_at.clone();
                old_started = old.request_started_at.clone();
                old_key = old.version_key.clone();
                old_bids = old.bids.clone();
                old_asks = old.asks.clone();
            }
            need(
                js_int(get(&b, "sequence"))
                    .map(|x| num_bigint::BigInt::from(x))
                    .unwrap_or_else(|| num_bigint::BigInt::from(js_str(&get(&b, "sequence")).parse::<i128>().unwrap_or(i128::MIN)))
                    >= js_int(&old_seq)
                        .map(|x| num_bigint::BigInt::from(x))
                        .unwrap_or_else(|| num_bigint::BigInt::from(js_str(&&old_seq).parse::<i128>().unwrap_or(i128::MIN)))
                    && instant_ns(&js_str(&get(&b, "sourceAt"))).map_err(|e| e.0)?
                        >= instant_ns(&js_str(&old_source)).map_err(|e| e.0)?
                    && instant(&js_str(&get(&b, "requestStartedAt")))?
                        >= instant(&js_str(&&old_started))?,
                "BOOK_ROLLBACK",
            )?;
            if get(&b, "versionKey") == &old_key {
                need(
                    payload_hash(&json!([get(&b, "bids"), get(&b, "asks")]))?
                        == payload_hash(&json!([old_bids, old_asks]))?,
                    "BOOK_VERSION_CONFLICT",
                )?;
            }
        }
        Ok(b)
    }

    pub fn get_evidence(&self, context: &V) -> Result<V, String> {
        let inner = self.inner.clone();
        let result = (|| -> Result<V, String> {
            Self::allowed(&inner)?;
            let (cache, n, scope) = {
                let g = inner.borrow();
                need(!g.busy && g.cache.is_some(), "CACHE_UNAVAILABLE")?;
                (g.cache.clone().unwrap(), (g.deps.now)(), g.scope.clone())
            };
            if context.get("marketId").is_some() {
                need(
                    get(context, "marketId") == get(&get(&cache, "leg"), "marketId")
                        && get(context, "exchangeId") == get(&get(&cache, "leg"), "exchangeId"),
                    "CACHE_LEG_MISMATCH",
                )?;
            }
            if context.get("orderId").is_some() && !get(context, "orderId").is_null() {
                need(get(context, "orderId") == get(&cache, "orderId"), "CACHE_ORDER_MISMATCH")?;
            }
            let kind = get(&cache, "kind").as_str().unwrap_or("").to_string();
            if kind == "preparation" {
                let op = get(context, "operation");
                need(
                    op.is_null() || ["prepare", "place"].contains(&op.as_str().unwrap_or("")),
                    "CACHE_OPERATION",
                )?;
                Self::local_bindings(&inner, op.as_str() != Some("prepare"), true)?;
                let view = Self::validate_account(get(&cache, "account"), get(&cache, "leg"), &scope, n)?;
                let p = get(&view, "positions")
                    .as_array()
                    .and_then(|ps| ps.iter().find(|p| get(p, "exchangeId") == get(&get(&cache, "leg"), "exchangeId")))
                    .cloned()
                    .unwrap_or(V::Null);
                need(
                    get(&p, "marketId") == get(&get(&cache, "leg"), "marketId")
                        && get(&p, "quantity") == &json!(-1)
                        && !get(&p, "settled").as_bool().unwrap_or(false),
                    "CURRENT_NO_LOT_REQUIRED",
                )?;
                let b = Self::check_book(&inner, get(&cache, "book"), get(&cache, "leg"), n)?;
                need(
                    instant(&js_str(&get(&b, "requestStartedAt")))?
                        >= instant(&js_str(&get(&cache, "accountCompletedAt")))?,
                    "BOOK_BEFORE_ACCOUNT",
                )?;
                return Ok(json!({
                    "account": get(&cache, "account"),
                    "book": get(&cache, "book"),
                    "globalRisk": Self::local_global(&inner)?,
                }));
            }
            if kind == "cancel" {
                let op = get(context, "operation");
                need(op.is_null() || op.as_str() == Some("cancel"), "CACHE_OPERATION")?;
                Self::own(&inner, get(&cache, "orderId"), get(&cache, "leg"))?;
                fresh(get(&cache, "order"), n)?;
                return Ok(json!({
                    "order": get(&cache, "order"),
                    "globalRisk": Self::local_global(&inner)?,
                }));
            }
            let op = get(context, "operation");
            need(op.is_null() || op.as_str() == Some("reconcile"), "CACHE_OPERATION")?;
            Self::own(&inner, get(&cache, "orderId"), get(&cache, "leg"))?;
            fresh(get(&cache, "order"), n)?;
            for r in get(&cache, "fillPages").as_array().cloned().unwrap_or_default() {
                fresh(&r, n)?;
            }
            Self::validate_account(get(&cache, "after"), get(&cache, "leg"), &scope, n)?;
            Ok(json!({
                "order": get(&cache, "order"),
                "fillPages": get(&cache, "fillPages"),
                "after": get(&cache, "after"),
                "globalRisk": Self::local_global(&inner)?,
            }))
        })();
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                Self::clear_cache(&inner);
                let failure = if e.starts_with("OWNED_NO_EXIT_EVIDENCE_") {
                    e.clone()
                } else {
                    "OWNED_NO_EXIT_EVIDENCE_INVALID_CACHE".to_string()
                };
                inner.borrow_mut().last_failure = Some(failure.clone());
                Err(failure)
            }
        }
    }

    // authorizes(spec, beforeDispatch, beforeNetwork)
    pub fn authorizes(&self, spec: &V, before_dispatch: &Guard, before_network: &Guard) -> bool {
        let r: Result<bool, String> = (|| {
            Self::allowed(&self.inner)?;
            let key = before_dispatch as *const Guard as *const () as usize;
            let p = self.inner.borrow().issued.get(&key).cloned();
            let p = match p {
                Some(p) => p,
                None => return Ok(false),
            };
            Ok(get(spec, "method").as_str() == Some("GET")
                && get(spec, "endpoint") == get(&p, "endpoint")
                && spec.get("body").is_none()
                && spec.get("bodyJson").is_none()
                && spec.get("beforeNetwork").is_none_or(|v| v.is_null())
                && (before_network as *const Guard as *const () as usize)
                    == get(&p, "beforeNetworkPtr").as_u64().unwrap_or(0) as usize
                && spec
                    .as_object()
                    .map(|o| {
                        o.keys().all(|k| {
                            ["method", "endpoint", "body", "bodyJson", "beforeDispatch", "beforeNetwork"]
                                .contains(&k.as_str())
                        })
                    })
                    .unwrap_or(false))
        })();
        r.unwrap_or(false)
    }

    fn read(inner: &Rc<RefCell<Inner>>, endpoint: &str) -> Result<V, String> {
        Self::allowed(inner)?;
        let (limit, reads) = {
            let g = inner.borrow();
            let op = g.operation.clone().unwrap_or_default();
            (g.bounds.get(&op).copied().unwrap_or(0), g.reads)
        };
        need(reads < limit, "READ_BOUND")?;
        budget_from(inner, false)?;
        let admitted = Rc::new(Cell::new(false));
        let inner2 = inner.clone();
        let admitted2 = admitted.clone();
        let before_dispatch = move |_p: &V, _s: &V| -> Result<bool, OrderTransportError> {
            (|| -> Result<bool, String> {
                Self::allowed(&inner2)?;
                budget_from(&inner2, false)?;
                need(!admitted2.get(), "DUPLICATE_ADMISSION")?;
                admitted2.set(true);
                Ok(true)
            })()
            .map_err(|code| transport_err(&code))
        };
        let inner3 = inner.clone();
        let admitted3 = admitted.clone();
        let before_network = move |_p: &V, _s: &V| -> Result<bool, OrderTransportError> {
            (|| -> Result<bool, String> {
                Self::allowed(&inner3)?;
                need(admitted3.get(), "NOT_ADMITTED")?;
                budget_from(&inner3, true)?;
                Ok(true)
            })()
            .map_err(|code| transport_err(&code))
        };
        let bd_ptr = &before_dispatch as &Guard as *const Guard as *const () as usize;
        let bn_ptr = &before_network as &Guard as *const Guard as *const () as usize;
        let spec = json!({
            "method": "GET",
            "endpoint": endpoint,
        });
        inner
            .borrow_mut()
            .issued
            .insert(bd_ptr, json!({"endpoint": endpoint, "beforeNetworkPtr": bn_ptr as u64}));
        let began = (inner.borrow().deps.now)();
        let result = (|| -> Result<V, String> {
            let response = (inner.borrow().deps.dispatch)(&spec)?;
            Self::allowed(inner)?;
            need(
                admitted.get()
                    && get(&response, "endpoint").as_str() == Some(endpoint)
                    && get(&response, "status").as_i64().unwrap_or(200) == 200,
                "READ_RESPONSE",
            )?;
            let r = (inner.borrow().deps.receipt_of)(&response, &json!({"method": "GET", "requestBodyHash": V::Null}))?;
            need(
                get(&r, "method").as_str() == Some("GET")
                    && get(&r, "endpoint").as_str() == Some(endpoint)
                    && get(&r, "status") == &json!(200)
                    && get(&r, "bodyText").is_string()
                    && get(&r, "bodyText").as_str().unwrap_or("").len() <= 8 * 1024 * 1024
                    && hex(get(&r, "sourceBodyHash"))
                    && get(&r, "sourceBodyHash").as_str() == Some(sha(get(&r, "bodyText").as_str().unwrap_or("")).as_str())
                    && get(&r, "requestBodyHash").is_null()
                    && r.as_object().map(|o| {
                        o.keys().all(|k| {
                            [
                                "method",
                                "endpoint",
                                "status",
                                "bodyText",
                                "sourceBodyHash",
                                "requestBodyHash",
                                "requestStartedAt",
                                "receivedAt",
                                "fetchInvocationId",
                            ]
                            .contains(&k.as_str())
                        })
                    }).unwrap_or(false),
                "RAW_RECEIPT",
            )?;
            fresh(&r, (inner.borrow().deps.now)())?;
            let last_end = inner.borrow().last_end;
            need(
                instant(get(&r, "requestStartedAt").as_str().unwrap_or(""))? >= began
                    && instant(get(&r, "requestStartedAt").as_str().unwrap_or(""))?
                        >= instant(get(&response, "requestStartedAt").as_str().unwrap_or(""))?
                    && instant(get(&r, "receivedAt").as_str().unwrap_or(""))?
                        <= instant(get(&response, "receivedAt").as_str().unwrap_or(""))?
                    && (last_end.is_none()
                        || instant(get(&r, "requestStartedAt").as_str().unwrap_or(""))? >= last_end.unwrap()),
                "RECEIPT_CAUSALITY",
            )?;
            let (parsed, _) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
            need(
                payload_hash(&parsed)? == payload_hash(get(&response, "data"))?,
                "RAW_DATA_MISMATCH",
            )?;
            let prior = inner.borrow().receipt_watermarks.get(endpoint).map(|w| (w.start, w.end));
            if let Some((ps, pe)) = prior {
                need(
                    instant(get(&r, "requestStartedAt").as_str().unwrap_or(""))? >= ps
                        && instant(get(&r, "receivedAt").as_str().unwrap_or(""))? >= pe,
                    "RECEIPT_ROLLBACK",
                )?;
            }
            inner.borrow_mut().receipt_watermarks.insert(
                endpoint.to_string(),
                Watermark {
                    start: instant(get(&r, "requestStartedAt").as_str().unwrap_or(""))?,
                    end: instant(get(&r, "receivedAt").as_str().unwrap_or(""))?,
                },
            );
            inner.borrow_mut().last_end = Some(instant(get(&r, "receivedAt").as_str().unwrap_or(""))?);
            Ok(r)
        })();
        inner.borrow_mut().reads += 1;
        inner.borrow_mut().issued.remove(&bd_ptr);
        result
    }

    fn pages<F>(&self, base: &str, check: F) -> Result<Vec<V>, String>
    where
        F: Fn(&V, &V) -> Result<(), String>,
    {
        let inner = self.inner.clone();
        let mut cursor: Option<String> = None;
        let mut checkpoint: Option<i64> = inner.borrow().projection_watermarks.get(base).copied();
        let mut all: Vec<V> = Vec::new();
        let mut seen_cursors: BTreeSet<String> = BTreeSet::new();
        let mut ids: BTreeSet<i64> = BTreeSet::new();
        for _ in 0..10 {
            let url = if cursor.is_none() {
                base.to_string()
            } else {
                format!("{base}&cursor={}", urlencoding(cursor.as_ref().unwrap()))
            };
            let r = Self::read(&inner, &url)?;
            let (d, _) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
            let p = get(&d, "pagination");
            need(
                get(&d, "data").is_array()
                    && get(&d, "data").as_array().unwrap().len() <= 200
                    && get(p, "limit") == &json!(200)
                    && get(p, "hasMore").is_boolean()
                    && (get(p, "nextCursor").is_null()
                        || (get(p, "nextCursor").is_string()
                            && !get(p, "nextCursor").as_str().unwrap_or("").is_empty()
                            && get(p, "nextCursor").as_str().unwrap_or("").len() <= 1000))
                    && get(&get(&d, "coverage"), "complete") == &json!(true)
                    && (get(&get(&d, "coverage"), "projectedThroughSequence").is_null()
                        || integer(get(&get(&d, "coverage"), "projectedThroughSequence"), 0)),
                "PAGES_INCOMPLETE",
            )?;
            let projected = js_int(get(&get(&d, "coverage"), "projectedThroughSequence"));
            if checkpoint.is_some() && projected.is_some() {
                need(projected.unwrap() >= checkpoint.unwrap(), "PROJECTION_ROLLBACK")?;
            }
            if projected.is_some() {
                checkpoint = projected;
            }
            for row in get(&d, "data").as_array().cloned().unwrap_or_default() {
                need(
                    integer(get(&row, "id"), 1) && !ids.contains(&js_int(get(&row, "id")).unwrap()),
                    "PAGE_DUPLICATE_ID",
                )?;
                ids.insert(js_int(get(&row, "id")).unwrap());
            }
            check(&d, &r)?;
            all.push(r);
            if get(p, "hasMore") == &json!(false) {
                need(get(p, "nextCursor").is_null(), "PAGE_CURSOR")?;
                if let Some(cp) = checkpoint {
                    inner
                        .borrow_mut()
                        .projection_watermarks
                        .insert(base.to_string(), cp);
                }
                return Ok(all);
            }
            let next = get(p, "nextCursor").as_str().unwrap_or("").to_string();
            need(!next.is_empty() && !seen_cursors.contains(&next), "PAGE_CURSOR")?;
            seen_cursors.insert(next.clone());
            cursor = Some(next);
        }
        Err("OWNED_NO_EXIT_EVIDENCE_PAGES_INCOMPLETE".into())
    }

    fn account(&self, leg: &V) -> Result<V, String> {
        let inner = self.inner.clone();
        let scope = inner.borrow().scope.clone();
        let slug = js_str(&get(&scope, "tournamentSlug"));
        let tid = js_str(&get(&scope, "tournamentId"));
        let cash = Self::read(&inner, &format!("/tournaments/{slug}"))?;
        let positions = Self::read(&inner, &format!("/tournaments/{slug}/portfolio/positions"))?;
        let open_orders_pages = self.pages(
            &format!("/orders?status=open&limit=200&tournamentId={tid}"),
            |_d, _r| Ok(()),
        )?;
        let scoped_orders_pages = self.pages(
            &format!(
                "/orders?status=all&exchangeId={}&limit=200&tournamentId={tid}",
                js_str(&get(leg, "exchangeId"))
            ),
            |_d, _r| Ok(()),
        )?;
        let result = json!({
            "cash": cash,
            "positions": positions,
            "openOrdersPages": open_orders_pages,
            "scopedOrdersPages": scoped_orders_pages,
        });
        Self::validate_account(&result, leg, &scope, (inner.borrow().deps.now)())?;
        Ok(result)
    }

    fn order_check(&self, r: &V, order_id: &V, leg: &V, open: bool) -> Result<V, String> {
        let inner = self.inner.clone();
        let scope = inner.borrow().scope.clone();
        let (d, _) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
        let n = (inner.borrow().deps.now)();
        need(
            integer(get(&d, "id"), 1)
                && js_str(&get(&d, "id")) == js_str(&order_id)
                && get(&d, "exchangeId") == get(leg, "exchangeId")
                && get(&d, "tournamentId") == get(&scope, "tournamentId")
                && get(&d, "side").as_str() == Some("no")
                && get(&d, "action").as_str() == Some("sell")
                && get(&d, "quantity") == &json!(1)
                && get(&d, "open") == &json!(open)
                && instant(get(&d, "createdAt").as_str().unwrap_or(""))? <= n + 2000
                && instant(get(&d, "expirationDate").as_str().unwrap_or(""))?
                    > instant(get(&d, "createdAt").as_str().unwrap_or(""))?,
            "ORDER_SCOPE_OR_PHASE",
        )?;
        normalize_official_fill_tick(get(&d, "priceLimit"))?;
        if !open {
            need(
                integer(get(&d, "quantityFilled"), 0) && js_int(get(&d, "quantityFilled")).unwrap_or(2) <= 1,
                "TERMINAL_QUANTITY",
            )?;
        }
        Ok(d)
    }

    fn run<F>(&self, kind: &str, leg_value: &V, order_id: Option<&V>, f: F) -> Result<V, String>
    where
        F: FnOnce(&V) -> Result<V, String>,
    {
        let inner = self.inner.clone();
        need(!inner.borrow().busy, "BUSY")?;
        Self::clear_cache(&inner);
        Self::allowed(&inner)?;
        let leg = exact_leg(&inner.borrow().scope.clone(), leg_value)?;
        if let Some(oid) = order_id {
            Self::own(&inner, oid, &leg)?;
        }
        {
            let mut g = inner.borrow_mut();
            g.busy = true;
            g.operation = Some(kind.to_string());
            g.reads = 0;
            g.last_end = None;
            g.last_failure = None;
        }
        let result = (|| -> Result<V, String> {
            let limit = inner.borrow().bounds.get(kind).copied().unwrap_or(0);
            need((inner.borrow().deps.has_read_headroom)(limit)?, "READ_HEADROOM")?;
            let value = f(&leg)?;
            Self::allowed(&inner)?;
            inner.borrow_mut().cache = Some(value);
            let v = inner.borrow();
            Ok(json!({
                "kind": kind,
                "leg": leg,
                "orderId": order_id.cloned().unwrap_or(V::Null),
                "completedAt": iso((v.deps.now)()),
                "readCount": v.reads,
            }))
        })();
        let result = match result {
            Ok(v) => Ok(v),
            Err(e) => {
                Self::clear_cache(&inner);
                let failure = if e.starts_with("OWNED_NO_EXIT_EVIDENCE_") {
                    e.clone()
                } else {
                    "OWNED_NO_EXIT_EVIDENCE_READ_FAILED".to_string()
                };
                inner.borrow_mut().last_failure = Some(failure.clone());
                Err(failure)
            }
        };
        {
            let mut g = inner.borrow_mut();
            g.busy = false;
            g.operation = None;
        }
        result
    }

    pub fn refresh_preparation(&self, leg: &V) -> Result<V, String> {
        self.run("preparation", leg, None, |l| {
            let a = self.account(l)?;
            let scope = self.inner.borrow().scope.clone();
            let info = Self::validate_account(&a, l, &scope, (self.inner.borrow().deps.now)())?;
            let tid = js_str(&get(&scope, "tournamentId"));
            let book = Self::read(
                &self.inner,
                &format!(
                    "/markets/{}/orderbook?tournamentId={tid}&depth=20",
                    js_str(&get(l, "marketId"))
                ),
            )?;
            let b = Self::check_book(&self.inner, &book, l, (self.inner.borrow().deps.now)())?;
            need(
                instant(&js_str(&get(&b, "requestStartedAt")))?
                    >= instant(get(&info, "completedAt").as_str().unwrap_or(""))?,
                "BOOK_BEFORE_ACCOUNT",
            )?;
            Self::validate_account(&a, l, &scope, (self.inner.borrow().deps.now)())?;
            let row = BookRow {
                sequence: get(&b, "sequence").clone(),
                source_at: get(&b, "sourceAt").clone(),
                request_started_at: get(&b, "requestStartedAt").clone(),
                version_key: get(&b, "versionKey").clone(),
                bids: get(&b, "bids").clone(),
                asks: get(&b, "asks").clone(),
            };
            self.inner
                .borrow_mut()
                .book_watermarks
                .insert(js_str(&get(l, "exchangeId")), row);
            Ok(json!({
                "kind": "preparation",
                "leg": l,
                "orderId": V::Null,
                "account": a,
                "accountCompletedAt": get(&info, "completedAt"),
                "book": book,
            }))
        })
    }

    pub fn refresh_reconciliation(&self, order_id: &V, leg: &V) -> Result<V, String> {
        self.run("reconciliation", leg, Some(order_id), |l| {
            let order = Self::read(&self.inner, &format!("/orders/{}", js_str(&order_id)))?;
            let od = self.order_check(&order, order_id, l, false)?;
            let od_created = get(&od, "createdAt").clone();
            let scope = self.inner.borrow().scope.clone();
            let totals = Rc::new(RefCell::new(Option::<String>::None));
            let totals2 = totals.clone();
            let fill_pages = self.pages(
                &format!("/orders/{}/fills?limit=200", js_str(&order_id)),
                move |d, r| {
                    need(
                        js_int(get(d, "orderId")) == js_int(order_id)
                            && get(d, "exchangeId") == get(l, "exchangeId")
                            && get(d, "tournamentId") == get(&scope, "tournamentId")
                            && [0, -1].contains(&js_int(get(d, "totalQuantityFilled")).unwrap_or(2)),
                        "FILL_SCOPE",
                    )?;
                    let avg = if get(d, "avgFillPrice").is_null() {
                        V::Null
                    } else {
                        get(&normalize_official_fill_tick(get(d, "avgFillPrice"))?, "price").clone()
                    };
                    let identity = payload_hash(&json!([get(d, "totalQuantityFilled"), avg]))?;
                    let mut t = totals2.borrow_mut();
                    need(t.is_none() || t.as_deref() == Some(identity.as_str()), "FILL_TOTAL_CHANGED")?;
                    *t = Some(identity);
                    drop(t);
                    for f in get(d, "data").as_array().cloned().unwrap_or_default() {
                        need(
                            get(&f, "side").as_str() == Some("no")
                                && get(&f, "quantity") == &json!(-1)
                                && instant(get(&f, "filledAt").as_str().unwrap_or(""))?
                                    >= instant(od_created.as_str().unwrap_or(""))?
                                && instant(get(&f, "filledAt").as_str().unwrap_or(""))?
                                    <= instant(get(r, "receivedAt").as_str().unwrap_or(""))?,
                            "FILL_SCHEMA",
                        )?;
                        normalize_official_fill_tick(get(&f, "price"))?;
                    }
                    Ok(())
                },
            )?;
            let decoded: Vec<V> = fill_pages
                .iter()
                .map(|r| parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or("")).map(|x| x.0))
                .collect::<Result<_, _>>()?;
            let fills: Vec<V> = decoded
                .iter()
                .flat_map(|d| get(d, "data").as_array().cloned().unwrap_or_default())
                .collect();
            let last = decoded.last().cloned().unwrap_or(V::Null);
            let fills_ok = if fills.is_empty() {
                get(&last, "avgFillPrice").is_null()
            } else {
                let f0 = get(&normalize_official_fill_tick(get(&fills[0], "price"))?, "price").clone();
                let l0 = get(&normalize_official_fill_tick(get(&last, "avgFillPrice"))?, "price").clone();
                f0 == l0
                    && amount(&js_str(&&f0))? >= amount(&js_str(&&get(&normalize_official_fill_tick(get(&od, "priceLimit"))?, "price").clone()))?
            };
            need(
                fills.len() <= 1
                    && fills.len() as i64 == js_int(get(&od, "quantityFilled")).unwrap_or(-1)
                    && js_int(get(&last, "totalQuantityFilled")).unwrap_or(0) == -(fills.len() as i64)
                    && fills_ok,
                "FILL_TOTAL_MISMATCH",
            )?;
            let after = self.account(l)?;
            need(
                instant(get(&get(&after, "cash"), "requestStartedAt").as_str().unwrap_or(""))?
                    >= instant(get(fill_pages.last().unwrap(), "receivedAt").as_str().unwrap_or(""))?,
                "POST_ACCOUNT_CAUSALITY",
            )?;
            Ok(json!({
                "kind": "reconciliation",
                "leg": l,
                "orderId": order_id,
                "order": order,
                "fillPages": fill_pages,
                "after": after,
            }))
        })?;
        self.get_evidence(&json!({
            "operation": "reconcile",
            "orderId": order_id,
            "marketId": get(&exact_leg(&self.inner.borrow().scope.clone(), leg)?, "marketId"),
            "exchangeId": get(&exact_leg(&self.inner.borrow().scope.clone(), leg)?, "exchangeId"),
        }))
    }

    pub fn refresh_cancel(&self, order_id: &V, leg: &V) -> Result<V, String> {
        self.run("cancel", leg, Some(order_id), |l| {
            let order = Self::read(&self.inner, &format!("/orders/{}", js_str(&order_id)))?;
            self.order_check(&order, order_id, l, true)?;
            Ok(json!({
                "kind": "cancel",
                "leg": l,
                "orderId": order_id,
                "order": order,
            }))
        })
    }

    pub fn status(&self) -> V {
        let g = self.inner.borrow();
        json!({
            "mode": "owned_no_exit_evidence_bridge_v1",
            "busy": g.busy,
            "closed": g.closed,
            "cachedKind": g.cache.as_ref().map(|c| get(c, "kind")).cloned().unwrap_or(V::Null),
            "cachedLeg": g.cache.as_ref().and_then(|c| c.get("leg")).cloned().unwrap_or(V::Null),
            "lastReadCount": g.reads,
            "lastFailure": g.last_failure.clone().map(V::from).unwrap_or(V::Null),
            "additionalApiClients": 0,
            "executionAuthority": false,
        })
    }

    pub fn clear(&self) {
        Self::clear_cache(&self.inner);
    }

    pub fn close(&self) -> Result<(), String> {
        need(!self.inner.borrow().busy, "BUSY")?;
        Self::clear_cache(&self.inner);
        let mut g = self.inner.borrow_mut();
        g.closed = true;
        g.receipt_watermarks.clear();
        g.book_watermarks.clear();
        g.projection_watermarks.clear();
        Ok(())
    }
}

fn transport_err(code: &str) -> OrderTransportError {
    OrderTransportError {
        code: code.into(),
        status: None,
        api_code: None,
        outcome: "blocked".into(),
        may_have_executed: false,
        retry_same_request_only: false,
        retry_after_ms: 0.0,
        semantic_mismatch: false,
    }
}

fn budget_from(inner: &Rc<RefCell<Inner>>, admitted_already: bool) -> Result<(), String> {
    let (reads, limit) = {
        let g = inner.borrow();
        let op = g.operation.clone().unwrap_or_default();
        (g.reads, g.bounds.get(&op).copied().unwrap_or(0))
    };
    need(
        (inner.borrow().deps.has_read_headroom)(limit - reads - if admitted_already { 1 } else { 0 })?,
        "READ_HEADROOM",
    )
}

// createOwnedNoExitEvidenceBridge({api,receiptOf,scope,getGlobalRisk,getOwnedLot,getExitReservation,ownsOrder,canRead,hasReadHeadroom,now,maxReadsPerOperation})
pub fn create_owned_no_exit_evidence_bridge(
    api_dispatch: Rc<OwnedBridgeDispatch>,
    scope: &V,
    receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    get_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    get_owned_lot: Rc<dyn Fn() -> Result<V, String>>,
    get_exit_reservation: Rc<dyn Fn() -> Result<V, String>>,
    owns_order: Rc<dyn Fn(&V, &V) -> Result<bool, String>>,
    can_read: Rc<dyn Fn() -> bool>,
    has_read_headroom: Rc<dyn Fn(i64) -> Result<bool, String>>,
    now: Rc<dyn Fn() -> i64>,
    max_reads_per_operation: Option<BTreeMap<String, i64>>,
) -> Result<OwnedNoExitEvidenceBridge, String> {
    let mut scope = scope.clone();
    if scope.get("markets").is_none() {
        let markets = json!([{ "marketId": get(&scope, "marketId"), "exchangeId": get(&scope, "exchangeId") }]);
        scope.as_object_mut().unwrap().insert("markets".into(), markets);
    }
    need(
        get(&scope, "ownerId").as_str() == Some("Codex")
            && get(&scope, "runId").as_str().map(|s| !s.is_empty()).unwrap_or(false)
            && get(&scope, "tournamentId").as_str() == Some("bda92870-621e-47b0-bc3c-3602c5c26f55")
            && get(&scope, "tournamentSlug")
                .as_str()
                .map(|s| {
                    !s.is_empty()
                        && s.split('-').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
                        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                })
                .unwrap_or(false)
            && get(&scope, "markets").as_array().map(|a| a.len() == 1).unwrap_or(false)
            && get(&scope, "markets")
                .as_array()
                .unwrap()
                .iter()
                .all(|m| id(get(m, "marketId")) && id(get(m, "exchangeId"))),
        "SCOPE",
    )?;
    let bounds_def: BTreeMap<String, i64> = [
        ("preparation".to_string(), 5),
        ("reconciliation".to_string(), 6),
        ("cancel".to_string(), 1),
    ]
    .into_iter()
    .collect();
    let provided = max_reads_per_operation.clone().unwrap_or_else(|| bounds_def.clone());
    for (k, bound) in &bounds_def {
        need(
            provided.get(k).is_some_and(|v| *v >= 1 && *v <= *bound),
            "READ_BOUND",
        )?;
    }
    let bridge = OwnedNoExitEvidenceBridge {
        inner: Rc::new(RefCell::new(Inner {
            scope: scope.clone(),
            original_hash: V::Null,
            deps: OwnedNoExitBridgeDeps {
                dispatch: api_dispatch,
                receipt_of,
                get_global_risk,
                get_owned_lot,
                get_exit_reservation,
                owns_order,
                can_read,
                has_read_headroom,
                now,
                max_reads_per_operation: max_reads_per_operation.clone(),
            },
            cache: None,
            busy: false,
            closed: false,
            operation: None,
            reads: 0,
            last_failure: None,
            last_end: None,
            issued: HashMap::new(),
            receipt_watermarks: HashMap::new(),
            book_watermarks: HashMap::new(),
            projection_watermarks: HashMap::new(),
            bounds: provided,
        })),
    };
    let original_proof = (bridge.inner.borrow().deps.get_owned_lot)()?;
    bridge.inner.borrow_mut().original_hash = get(&original_proof, "verificationHash").clone();
    OwnedNoExitEvidenceBridge::local_bindings(&bridge.inner, false, false)?;
    Ok(bridge)
}
