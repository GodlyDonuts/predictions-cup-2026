// execution/paired-owned-exit-retirement-v2.mjs — pure retirement evidence for
// the original one-share .070 paired pilot. It neither changes its historical
// journals nor acts as a lease/activation.
use crate::core::payload_hash;
use baal_service_runtime::fixed::{amount, instant, money};
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use serde_json::{json, Value as V};

fn need(ok: bool, c: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("PAIRED_OWNED_RETIREMENT_{c}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn amount_v(v: &V) -> Result<i128, String> {
    amount(v.as_str().unwrap_or(""))
}

// proof(p,key): strip the hash key and verify payloadHash(body).
fn proof(p: &V, key: &str) -> Result<V, String> {
    let mut body = p.clone();
    let h = body.as_object_mut().and_then(|m| m.remove(key));
    need(
        h.as_ref().is_some_and(|x| hex(x))
            && payload_hash(&body)? == h.as_ref().and_then(|x| x.as_str()).unwrap_or(""),
        "PROOF_HASH",
    )?;
    Ok(body)
}

struct JournalFacts {
    last: V,
    sequence: i64,
    hash: String,
    bytes: usize,
    sha256: String,
    prefix_bytes: i64,
    prefix_sha256: String,
}

// journal(input): verified replay of the actor journal receipt with a sealed
// byte prefix (bodyText covers prefixBytes..end and the prefix digest binds).
fn journal(input: &V) -> Result<JournalFacts, String> {
    need(
        !input.is_null()
            && get(input, "bodyText").as_str().is_some_and(|s| s.len() <= 16 * 1024 * 1024 && s.ends_with('\n'))
            && js_int(get(input, "prefixBytes")).is_some_and(|b| b > 0)
            && hex(get(input, "prefixSha256")),
        "ACTOR_SOURCE",
    )?;
    let body_text = get(input, "bodyText").as_str().unwrap();
    let bytes = body_text.as_bytes();
    let prefix_bytes = js_int(get(input, "prefixBytes")).unwrap() as usize;
    need(
        bytes.len() >= prefix_bytes
            && bytes[prefix_bytes - 1] == 10
            && sha256(&bytes[..prefix_bytes]) == get(input, "prefixSha256").as_str().unwrap_or(""),
        "SEALED_PREFIX",
    )?;
    let mut sequence = 0_i64;
    let mut previous = String::new();
    let mut last = V::Null;
    for line in body_text.trim_end().split('\n') {
        let r: V = serde_json::from_str(line).map_err(|_| "ACTOR_CHAIN".to_string())?;
        let mut body = r.clone();
        let hash = body.as_object_mut().and_then(|m| m.remove("hash"));
        sequence += 1;
        need(
            js_int(get(&body, "sequence")) == Some(sequence)
                && get(&body, "previous") == &json!(previous)
                && hash.as_ref().is_some_and(|h| hex(h))
                && hash.as_ref().and_then(|h| h.as_str())
                    == Some(sha256(js_stringify(&body).as_bytes()).as_str()),
            "ACTOR_CHAIN",
        )?;
        previous = hash.and_then(|h| h.as_str().map(|s| s.to_string())).unwrap_or_default();
        last = r;
    }
    need(
        get(input, "sequence") == &json!(sequence) && get(input, "hash") == &json!(previous),
        "ACTOR_FRONTIER",
    )?;
    Ok(JournalFacts {
        last,
        sequence,
        hash: previous,
        bytes: bytes.len(),
        sha256: sha256(bytes),
        prefix_bytes: prefix_bytes as i64,
        prefix_sha256: get(input, "prefixSha256").as_str().unwrap_or("").to_string(),
    })
}

// registry(row): one verified funding-registry row.
fn registry(row: &V) -> Result<(V, String, i64), String> {
    let mut body = row.clone();
    let hash = body.as_object_mut().and_then(|m| m.remove("hash"));
    need(
        js_int(get(&body, "sequence")).is_some_and(|s| s > 0)
            && hash.as_ref().is_some_and(|h| hex(h))
            && hash.as_ref().and_then(|h| h.as_str())
                == Some(sha256(js_stringify(&body).as_bytes()).as_str())
            && get(&body, "state").get("schemaVersion") == Some(&json!(1)),
        "REGISTRY_RECORD",
    )?;
    Ok((
        get(&body, "state").clone(),
        hash.and_then(|h| h.as_str().map(|s| s.to_string())).unwrap_or_default(),
        js_int(get(&body, "sequence")).unwrap_or(0),
    ))
}

// buildPairedOwnedExitRetirementV2({originalBuyProof,sellProof,sellAfter,actorJournal,actorStatus,fundingRegistryRecord,accountLease,globalRisk,readAt})
pub fn build_paired_owned_exit_retirement_v2(input: &V) -> Result<V, String> {
    let buy = get(input, "originalBuyProof");
    let sell = get(input, "sellProof");
    let sell_after = get(input, "sellAfter");
    let actor_status = get(input, "actorStatus");
    let account_lease = get(input, "accountLease");
    let global_risk = get(input, "globalRisk");
    let read_at = get(input, "readAt");
    let n = instant_v(read_at)?;
    proof(buy, "verificationHash")?;
    proof(sell, "verificationHash")?;
    need(
        matches!(
            get(buy, "mode").as_str(),
            Some("scoped_rest_no_lot_with_rounded_cash_v1" | "scoped_rest_no_lot_with_rounded_cash_neutral_all_v2")
        ) && get(buy, "ownerId") == &json!("Codex")
            && get(buy, "tournamentId") == &json!("bda92870-621e-47b0-bc3c-3602c5c26f55")
            && get(buy, "scopedLotVerified") == &json!(true)
            && get(buy, "filledQuantity") == &json!(1)
            && get(buy, "signedPositionDelta") == &json!(-1)
            && get(buy, "scopedPrincipalCost") == &json!("0.070000")
            && get(get(buy, "lot"), "side").as_str() == Some("no")
            && get(get(buy, "lot"), "quantity") == &json!(1)
            && get(get(buy, "lot"), "ownerId") == get(buy, "ownerId")
            && get(get(buy, "lot"), "runId") == get(buy, "runId")
            && get(get(buy, "lot"), "entryPrice") == &json!("0.070000")
            && get(get(buy, "lot"), "orderId") == get(buy, "orderId"),
        "ORIGINAL_ONE_LOT",
    )?;
    need(
        matches!(
            get(sell, "mode").as_str(),
            Some("scoped_rest_no_owned_exit_with_rounded_cash_v1" | "scoped_rest_no_owned_exit_with_rounded_cash_neutral_all_v2")
        ) && get(sell, "scopedExitVerified") == &json!(true)
            && get(sell, "filledQuantity") == &json!(1)
            && get(sell, "signedPositionDelta") == &json!(1)
            && get(sell, "ownerId") == get(buy, "ownerId")
            && get(sell, "runId") == get(buy, "runId")
            && get(sell, "tournamentId") == get(buy, "tournamentId")
            && get(sell, "marketId") == get(buy, "marketId")
            && get(sell, "exchangeId") == get(buy, "exchangeId")
            && get(sell, "originalBuyProofHash") == get(buy, "verificationHash")
            && get(sell, "originalLotId") == get(get(buy, "lot"), "lotId")
            && get(sell, "originalPrincipalCost") == &json!("0.070000")
            && get(sell, "removedPrincipalCost") == &json!("0.070000")
            && get(sell, "remainingOwnedLot") == &V::Null
            && !get(sell, "requestedLimit").is_null()
            && get(sell, "orderId") != get(buy, "orderId")
            && get(get(sell, "inventory"), "unbackedSell") == &json!(false)
            && get(get(sell, "inventory"), "priorSignedQuantity") == &json!(-1)
            && get(get(sell, "inventory"), "remainingSignedQuantity") == &json!(0),
        "COMPLETE_OWNED_SELL",
    )?;
    need(
        get(sell, "fills").as_array().is_some_and(|f| {
            f.len() == 1
                && get(&f[0], "side").as_str() == Some("no")
                && get(&f[0], "signedQuantity") == &json!(-1)
                && get(&f[0], "quantity") == &json!(1)
                && get(&f[0], "priceBasis").as_str() == Some("side_relative")
                && get(&f[0], "source").as_str() == Some("rest_order_fills")
                && get(&f[0], "price") == get(sell, "scopedProceeds")
        }) && amount_v(get(sell, "scopedProceeds")).is_ok_and(|a| a > 0)
            && money(amount_v(get(sell, "scopedProceeds")).unwrap_or(0) - 70_000) == get(sell, "realizedPnl").as_str().unwrap_or(""),
        "EXACT_SELL_PROCEEDS",
    )?;
    need(
        instant_v(get(sell, "readAt"))? <= n && n - instant_v(get(sell, "readAt"))? <= 20_000,
        "SELL_PROOF_FRESHNESS",
    )?;
    if get(sell, "mode").as_str()
        == Some("scoped_rest_no_owned_exit_with_rounded_cash_neutral_all_v2")
    {
        proof(get(sell, "ackEconomics"), "verificationHash")?;
        let ae = get(sell, "ackEconomics");
        need(
            get(ae, "mode").as_str() == Some("observed_neutral_no_sell_ack_economics_v1")
                && get(ae, "sourceBodyHash")
                    == get(get(get(sell, "sourceReceipts"), "ack"), "sourceBodyHash")
                && get(ae, "executionAuthority") == &json!(false)
                && matches!(
                    get(ae, "allState").as_str(),
                    Some("null" | "observed_neutral_sell")
                ),
            "SELL_ACK_ECONOMICS_SOURCE",
        )?;
        if get(ae, "allState").as_str() == Some("observed_neutral_sell") {
            need(
                get(ae, "ackNotional") == get(sell, "scopedProceeds")
                    && get(ae, "fullNotionalCost") == get(sell, "scopedProceeds")
                    && get(ae, "effectiveEntryCost") == get(sell, "scopedProceeds")
                    && get(ae, "netBuyingPowerImpact")
                        == &json!(money(-amount_v(get(sell, "scopedProceeds")).unwrap_or(0)))
                    && [
                        "collateralSavings",
                        "guaranteedPayoutFloorAfter",
                        "outstandingAdvanceAfter",
                        "collateralRepayment",
                        "redemptionCredit",
                    ]
                    .iter()
                    .all(|k| get(ae, k) == &json!("0.000000"))
                    && get(ae, "relationshipIds").as_array().is_some_and(|a| a.is_empty())
                    && get(ae, "componentId") == &V::Null,
                "SELL_ACK_NEUTRAL_ECONOMICS",
            )?;
        }
    }
    let j = journal(get(input, "actorJournal"))?;
    let j_state = get(&j.last, "state");
    let p = get(&j_state, "plan").clone();
    let orders = get(&j_state, "orders").clone();
    need(
        get(&p, "scope").get("ownerId") == Some(get(buy, "ownerId"))
            && get(&p, "scope").get("runId") == Some(get(buy, "runId"))
            && get(&p, "scope").get("tournamentId") == Some(get(buy, "tournamentId"))
            && hex(get(&p, "planHash"))
            && hex(get(&p, "captureHash"))
            && instant_v(get(&p, "reviewExpiresAt"))? <= n
            && orders.as_array().is_some_and(|o| o.len() == 1),
        "ORIGINAL_EXPIRED_ONE_ORDER",
    )?;
    proof(&p, "planHash")?;
    need(
        get(&p, "legs").as_array().is_some_and(|l| {
            l.len() == 2
                && l.iter().map(|x| get(x, "exchangeId").as_str().unwrap_or("")).collect::<std::collections::BTreeSet<_>>().len() == 2
                && l.iter().any(|x| {
                    get(x, "marketId") == get(buy, "marketId") && get(x, "exchangeId") == get(buy, "exchangeId")
                })
        }),
        "ORIGINAL_PAIR_SCOPE",
    )?;
    let o = &orders[0];
    need(
        get(o, "phase").as_str() == Some("reconciled")
            && get(o, "orderId") == get(buy, "orderId")
            && get(o, "marketId") == get(buy, "marketId")
            && get(get(o, "body"), "exchangeId") == get(buy, "exchangeId")
            && get(get(o, "body"), "action").as_str() == Some("buy")
            && get(get(o, "body"), "side").as_str() == Some("no")
            && get(get(o, "body"), "quantity") == &json!(1)
            && !js_truthy(o.get("cancelPending"))
            && !js_truthy(o.get("semanticMismatch"))
            && get(get(o, "proof"), "verificationHash") == get(buy, "verificationHash")
            && payload_hash(get(o, "proof"))? == payload_hash(buy)?,
        "ORIGINAL_ACTOR_LOT",
    )?;
    need(
        get(actor_status, "planHash") == get(&p, "planHash")
            && js_int(get(actor_status, "sequence")) == Some(j.sequence)
            && get(actor_status, "hash") == &json!(j.hash)
            && get(actor_status, "orders").as_array().is_some_and(|a| {
                a.len() == 1
                    && get(&a[0], "phase").as_str() == Some("reconciled")
                    && get(&a[0], "orderId") == get(buy, "orderId")
                    && get(&a[0], "proofHash") == get(buy, "verificationHash")
            })
            && get(actor_status, "owned").as_array().is_some_and(|a| {
                a.len() == 1
                    && get(&a[0], "lotId") == get(get(buy, "lot"), "lotId")
                    && get(&a[0], "side").as_str() == Some("no")
                    && get(&a[0], "ownerId") == get(buy, "ownerId")
                    && get(&a[0], "runId") == get(buy, "runId")
                    && get(&a[0], "orderId") == get(buy, "orderId")
                    && get(&a[0], "quantity") == &json!(1)
                    && get(&a[0], "exchangeId") == get(buy, "exchangeId")
                    && get(&a[0], "entryPrice") == &json!("0.070000")
            }),
        "ACTUAL_ACTOR_STATUS",
    )?;
    let (r_state, r_hash, r_sequence) = registry(get(input, "fundingRegistryRecord"))?;
    let f = get(&r_state, "funding").clone();
    let mut funding_body = f.clone();
    let reservation_hash = funding_body.as_object_mut().and_then(|m| m.remove("reservationHash"));
    need(
        reservation_hash.as_ref().is_some_and(|h| hex(h))
            && payload_hash(&funding_body)? == reservation_hash.as_ref().and_then(|h| h.as_str()).unwrap_or("")
            && get(&f, "ownerId") == get(buy, "ownerId")
            && get(&f, "runId") == get(buy, "runId")
            && get(&f, "tournamentId") == get(buy, "tournamentId")
            && get(&f, "planHash") == get(&p, "planHash")
            && get(&r_state, "planHash") == get(&p, "planHash")
            && get(&r_state, "captureHash") == get(&p, "captureHash")
            && get(&f, "ownedPrincipal") == &json!("0.070000")
            && get(&f, "cumulativeBuy") == &json!("0.070000")
            && get(&f, "reservedCash") == &json!("0.920000")
            && amount_v(get(&f, "reservedCash"))? + amount_v(get(&f, "cumulativeBuy"))?
                == amount_v(get(&p, "pairCostCeiling"))?
            && get(&r_state, "owned").as_array().is_some_and(|a| {
                a.len() == 1
                    && a[0].get("lotId").is_none()
                    && get(&a[0], "marketId") == get(buy, "marketId")
                    && get(&a[0], "exchangeId") == get(buy, "exchangeId")
                    && get(&a[0], "quantity") == &json!(1)
                    && get(&a[0], "entryPrice") == &json!("0.070000")
                    && get(&a[0], "orderId") == get(buy, "orderId")
            })
            && get(&r_state, "pairedOrders").as_array().is_some_and(|a| {
                a.len() == 1
                    && get(&a[0], "phase").as_str() == Some("reconciled")
                    && get(&a[0], "orderId") == get(buy, "orderId")
                    && !js_truthy(a[0].get("cancelPending"))
                    && !js_truthy(a[0].get("semanticMismatch"))
                    && get(&a[0], "proofHash") == get(buy, "verificationHash")
            })
            && get(&r_state, "ended") == &json!(false)
            && get(&r_state, "exclusiveActive") == &json!(true),
        "ORIGINAL_FUNDING_LIABILITY",
    )?;
    need(
        js_int(get(get(&r_state, "actorAnchor"), "sequence")) == Some(j.sequence)
            && get(get(&r_state, "actorAnchor"), "hash") == &json!(j.hash)
            && js_int(get(get(&r_state, "actorAnchor"), "bytes")) == Some(j.bytes as i64)
            && get(get(&r_state, "actorAnchor"), "sha256") == &json!(j.sha256),
        "REGISTRY_ACTOR_ANCHOR",
    )?;
    let scope = json!({
        "ownerId": get(buy, "ownerId"),
        "runId": get(buy, "runId"),
        "tournamentId": get(buy, "tournamentId"),
        "tournamentSlug": get(get(&p, "scope"), "tournamentSlug"),
        "marketId": get(buy, "marketId"),
        "exchangeId": get(buy, "exchangeId"),
    });
    let account = crate::no_rest_reconciliation_v2::inspect_scoped_rest_no_account(&json!({
        "account": sell_after,
        "binding": scope,
        "readAt": read_at,
    }))?;
    need(
        get(sell, "sourceReceipts").get("after").is_some_and(V::is_array)
            && get(&account, "sourceReceipts").as_array().map(|a| a.len())
                == get(sell, "sourceReceipts").get("after").and_then(V::as_array).map(|a| a.len())
            && get(&account, "sourceReceipts").as_array().unwrap().iter().all(|s| {
                get(sell, "sourceReceipts").get("after").and_then(V::as_array).unwrap().iter().any(|x| {
                    get(x, "endpoint") == get(s, "endpoint")
                        && get(x, "sourceBodyHash") == get(s, "sourceBodyHash")
                        && get(x, "receivedAt") == get(s, "receivedAt")
                })
            }),
        "SELL_AFTER_SOURCE",
    )?;
    let target = get(&account, "positions")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .find(|p| get(p, "exchangeId") == get(buy, "exchangeId"))
        .cloned();
    // !target?.quantity && (!target || target.marketId===buy.marketId)
    need(
        !target.as_ref().is_some_and(|t| js_truthy(t.get("quantity")))
            && (target.is_none() || get(target.as_ref().unwrap(), "marketId") == get(buy, "marketId")),
        "ACTUAL_CURRENT_NO_NOT_FLAT",
    )?;
    let terminal = get(&account, "scopedOrders")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .find(|o| js_int(get(o, "id")).map(|x| x.to_string()) == Some(get(sell, "orderId").as_str().unwrap_or("").to_string()))
        .cloned()
        .unwrap_or(V::Null);
    need(
        get(&terminal, "side").as_str() == Some("no")
            && get(&terminal, "action").as_str() == Some("sell")
            && get(&terminal, "quantity") == &json!(1)
            && get(&terminal, "quantityFilled") == &json!(1)
            && get(&terminal, "open") == &json!(false),
        "TERMINAL_SELL_HISTORY",
    )?;
    need(
        get(account_lease, "ownerId") == get(buy, "ownerId")
            && get(account_lease, "tournamentId") == get(buy, "tournamentId")
            && hex(get(account_lease, "leaseId"))
            && hex(get(account_lease, "sourceHash"))
            && instant_v(get(account_lease, "at"))? >= instant_v(get(&account, "completedAt"))?
            && instant_v(get(account_lease, "at"))? <= n
            && n - instant_v(get(account_lease, "at"))? <= 20_000,
        "FRESH_ACTUAL_ACCOUNT_LEASE",
    )?;
    need(
        get(global_risk, "ownerId") == get(buy, "ownerId")
            && get(global_risk, "runId") == get(buy, "runId")
            && get(global_risk, "tournamentId") == get(buy, "tournamentId")
            && get(global_risk, "complete") == &json!(true)
            && get(global_risk, "soleCollector") == &json!(true)
            && get(global_risk, "stopped") == &json!(false)
            && hex(get(global_risk, "sourceHash"))
            && get(global_risk, "unresolvedIntentIds").as_array().is_some_and(|a| a.is_empty())
            && instant_v(get(global_risk, "at"))? >= instant_v(get(&account, "completedAt"))?
            && instant_v(get(global_risk, "at"))? <= n
            && n - instant_v(get(global_risk, "at"))? <= 20_000,
        "GLOBAL_UNKNOWN",
    )?;
    let mut body = serde_json::Map::from_iter([
        ("schemaVersion".into(), json!(1)),
        ("mode".into(), json!("one_owned_no_paired_exit_retirement_neutral_sell_v2")),
        ("recordedAt".into(), read_at.clone()),
        ("ownerId".into(), get(buy, "ownerId").clone()),
        ("runId".into(), get(buy, "runId").clone()),
        ("tournamentId".into(), get(buy, "tournamentId").clone()),
        ("marketId".into(), get(buy, "marketId").clone()),
        ("exchangeId".into(), get(buy, "exchangeId").clone()),
        ("lotId".into(), get(get(buy, "lot"), "lotId").clone()),
        ("originalBuyProofHash".into(), get(buy, "verificationHash").clone()),
        ("sellProofHash".into(), get(sell, "verificationHash").clone()),
        ("originalActorPlanHash".into(), get(&p, "planHash").clone()),
        ("originalCaptureHash".into(), get(&p, "captureHash").clone()),
        (
            "originalActorSource".into(),
            json!({
                "sequence": j.sequence,
                "hash": j.hash,
                "bytes": j.bytes,
                "sha256": j.sha256,
                "prefixBytes": j.prefix_bytes,
                "prefixSha256": j.prefix_sha256,
            }),
        ),
        ("originalFundingRecordHash".into(), json!(r_hash)),
        ("originalFundingSequence".into(), json!(r_sequence)),
        ("originalFundingReservationHash".into(), get(&f, "reservationHash").clone()),
        ("currentAfterSourceHash".into(), json!(payload_hash(get(&account, "sourceReceipts"))?)),
        ("accountLeaseEvidenceHash".into(), json!(payload_hash(account_lease)?)),
        ("globalRiskHash".into(), json!(payload_hash(global_risk)?)),
        ("releasedUnusedReservation".into(), get(&f, "reservedCash").clone()),
        ("releasedCurrentOwnedQuantity".into(), json!(1)),
        ("currentOwnedQuantity".into(), json!(0)),
        ("currentReservedCash".into(), json!("0.000000")),
        ("originalAcquisitionCost".into(), json!("0.070000")),
        ("preservedCumulativeBuy".into(), json!("0.070000")),
        ("preservedOriginalHistoricalOwnedPrincipal".into(), json!("0.070000")),
        ("scopedSellProceeds".into(), get(sell, "scopedProceeds").clone()),
        ("scopedRealizedPnl".into(), get(sell, "realizedPnl").clone()),
        ("originalRecordsModified".into(), json!(false)),
        ("originalEntryWindowRenewed".into(), json!(false)),
        ("newPairAuthorized".into(), json!(false)),
        ("globalCashReconciledExactly".into(), json!(false)),
        ("executionAuthority".into(), json!(false)),
        ("additionalClients".into(), json!(0)),
    ]);
    let overlay = payload_hash(&V::Object(body.clone()))?;
    body.insert("overlayHash".into(), json!(overlay));
    Ok(V::Object(body))
}
