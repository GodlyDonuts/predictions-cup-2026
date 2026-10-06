//! Port of execution/paired-rest-controller-v3.mjs — the dormant third REST
//! execution revision. Shares the journal (paired-rest-journal.jsonl), the
//! 'paired-rest-lifecycle' lease scope and every runtime closure with the v2
//! controller; differs in: (1) codeIdentity is the v3 module's import closure,
//! (2) `neutralAckRevision` replays a v2-written journal prefix under legacy
//! semantics, and (3) reduce() gains the `neutral_ack_revision` event plus a
//! `legacy` flag switching canonicalAck/verifyScopedRestNoBuy to the v1 forms.
use crate::core::{canonical_payload, payload_hash};
use crate::gates::normalize_official_fill_tick;
use crate::no_rest_reconciliation_v2 as nrr;
use crate::paired_rest_controller::{
    account_shape, active, binding, cost, derive_paired_rest_plan, ended, endpoint_of_order,
    expected_positions, funding, get, global_gate, hex, id, instant_ns_v, instant_v, integer,
    amount_v, js_bigint, js_round, keys, need, open_controller, orders_of, preflight, raw, sha,
    tick,
    uuid_v4, verify_scoped_rest_no_buy_v1, PairedRestController, PairedRestControllerOptions,
    PipeNull,
};
use crate::receipts::parse_number_lexemes;
use baal_service_runtime::contract::{instant_ns, ms_to_iso};
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use baal_service_runtime::market::normalize_book;
use serde_json::{json, Map, Value as V};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

// canonicalAck(o, r) — the v3 ack check: official-tick price + neutral-all
// economics instead of the raw `all===null` flag. `try{...}catch{false}`.
fn canonical_ack(o: &V, r: &V) -> bool {
    let Ok((d, _)) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))
    else {
        return false;
    };
    js_int(get(&d, "orderId")).is_some_and(|i| i > 0)
        && get(&d, "exchangeId") == get(get(o, "body"), "exchangeId")
        && get(&d, "side") == "no"
        && get(&d, "action") == "buy"
        && js_int(get(&d, "quantity")) == Some(1)
        && normalize_official_fill_tick(get(&d, "price"))
            .ok()
            .and_then(|n| amount(get(&n, "price").as_str().unwrap_or("")).ok())
            .zip(price(&crate::js_key(get(get(o, "body"), "price"))).ok())
            .is_some_and(|(a, b)| a == b)
        && crate::no_neutral_all::inspect_no_ack_all_economics(r).is_ok()
        && get(&d, "open").is_boolean()
        && js_int(get(&d, "quantityTraded")).is_some()
        && js_int(get(&d, "remainingQuantity")).is_some()
        && js_int(get(&d, "quantityTraded")).unwrap_or(0) >= 0
        && js_int(get(&d, "remainingQuantity")).unwrap_or(0) >= 0
        && js_int(get(&d, "quantityTraded")).unwrap_or(0)
            + js_int(get(&d, "remainingQuantity")).unwrap_or(0)
            == 1
}

// legacyCanonicalAck(o, r) — the v2-form ack check retained for journal rows
// replayed below the neutralAckRevision prefix.
fn legacy_canonical_ack(o: &V, r: &V) -> bool {
    let Ok((d, _)) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))
    else {
        return false;
    };
    js_int(get(&d, "orderId")).is_some_and(|i| i > 0)
        && get(&d, "exchangeId") == get(get(o, "body"), "exchangeId")
        && get(&d, "side") == "no"
        && get(&d, "action") == "buy"
        && js_int(get(&d, "quantity")) == Some(1)
        && normalize_official_fill_tick(get(&d, "price"))
            .ok()
            .and_then(|n| amount(get(&n, "price").as_str().unwrap_or("")).ok())
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

fn reduce_v3(s: &V, e: &V, legacy: bool) -> Result<V, String> {
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
    if get(e, "kind") == "neutral_ack_revision" {
        need(
            !legacy
                && get(&o, "phase") == "semantic_mismatch"
                && js_truthy(o.get("semanticMismatch"))
                && get(&o, "orderId") == get(e, "orderId")
                && get(get(&o, "ack"), "sourceBodyHash") == get(e, "sourceBodyHash")
                && canonical_ack(&o, get(&o, "ack")),
            "NEUTRAL_ACK_REVISION_BINDING",
        )?;
        let proof =
            crate::no_neutral_all::inspect_no_ack_all_economics(get(&o, "ack"))?;
        need(
            get(&proof, "allState") == "neutral"
                && get(e, "economicsProofHash") == &json!(payload_hash(&proof)?),
            "NEUTRAL_ACK_REVISION_PROOF",
        )?;
        o["semanticMismatch"] = json!(false);
        o["phase"] = json!("acknowledged");
        n["orders"].as_array_mut().unwrap()[idx] = o;
        return Ok(n);
    }
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
                let mismatch = !(if legacy {
                    legacy_canonical_ack(&o, &ack)
                } else {
                    canonical_ack(&o, &ack)
                });
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
            let proof = if legacy {
                verify_scoped_rest_no_buy_v1(&verify_input)?
            } else {
                nrr::verify_scoped_rest_no_buy_v2(&verify_input)?
            };
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


fn reduce_v3_default(s: &V, e: &V) -> Result<V, String> {
    reduce_v3(s, e, false)
}

/// openPairedRestControllerV3({dir, capture, scope, dispatch, canOperate,
/// assertAccountLease, installFundingReservation, getEvidence, rawReceiptOf,
/// now, neutralAckRevision})
pub fn open_paired_rest_controller_v3(
    opts: PairedRestControllerOptions,
    neutral_ack_revision: Option<&V>,
) -> Result<PairedRestController, String> {
    open_controller(
        opts,
        "paired-rest-controller-v3.mjs",
        reduce_v3_default,
        reduce_v3,
        neutral_ack_revision,
    )
}
