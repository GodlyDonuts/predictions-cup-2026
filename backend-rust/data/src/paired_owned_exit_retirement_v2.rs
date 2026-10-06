// execution/paired-owned-exit-retirement-v2.mjs — pure retirement evidence for
// the original one-share .070 paired pilot. It neither changes its historical
// journals nor acts as a lease/activation.
use baal_execution::core::payload_hash;
use crate::no_rest_reconciliation::inspect_scoped_rest_no_account;
use baal_service_runtime::fixed::{amount, instant, money};
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use serde_json::{json, Value as V};

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("PAIRED_OWNED_RETIREMENT_{c}")) }
}
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn get<'a>(v: &'a V, k: &str) -> &'a V { v.get(k).unwrap_or(&V::Null) }
fn amount_s(v: &V) -> Result<i128, String> { amount(v.as_str().unwrap_or("")) }
fn instant_s(v: &V) -> Result<i64, String> { instant(v.as_str().unwrap_or("")) }

fn proof(p: &V, key: &str) -> Result<V, String> {
    let mut body = p.clone();
    let h = body.as_object_mut().and_then(|m| m.shift_remove(key)).unwrap_or(V::Null);
    need(hex(&h) && payload_hash(&body).ok().as_deref() == h.as_str(), "PROOF_HASH")?;
    Ok(body)
}
fn journal(input: &V) -> Result<(V, i64, String, usize, String, usize, String), String> {
    let body_text = get(input, "bodyText").as_str().unwrap_or("").to_string();
    need(
        !body_text.is_empty()
            && body_text.len() <= 16 * 1024 * 1024
            && body_text.ends_with('\n')
            && js_int(get(input, "prefixBytes")).is_some_and(|n| n > 0)
            && hex(get(input, "prefixSha256")),
        "ACTOR_SOURCE",
    )?;
    let bytes = body_text.as_bytes();
    let prefix_bytes = js_int(get(input, "prefixBytes")).unwrap() as usize;
    need(
        bytes.len() >= prefix_bytes
            && bytes.get(prefix_bytes.wrapping_sub(1)) == Some(&10u8)
            && sha256(&bytes[..prefix_bytes]) == get(input, "prefixSha256").as_str().unwrap_or(""),
        "SEALED_PREFIX",
    )?;
    let mut sequence: i64 = 0;
    let mut previous = String::new();
    let mut last = V::Null;
    for line in body_text.trim_end().split('\n') {
        let r: V = serde_json::from_str(line).map_err(|_| "PAIRED_OWNED_RETIREMENT_ACTOR_CHAIN".to_string())?;
        let mut body = r.clone();
        let hash = body.as_object_mut().and_then(|m| m.shift_remove("hash")).unwrap_or(V::Null);
        sequence += 1;
        need(
            get(&body, "sequence") == &json!(sequence)
                && get(&body, "previous") == &json!(previous)
                && hex(&hash)
                && sha256(js_stringify(&body).as_bytes()) == hash.as_str().unwrap_or(""),
            "ACTOR_CHAIN",
        )?;
        previous = hash.as_str().unwrap_or("").to_string();
        last = r;
    }
    need(
        get(input, "sequence") == &json!(sequence)
            && get(input, "hash") == &json!(previous),
        "ACTOR_FRONTIER",
    )?;
    Ok((
        last,
        sequence,
        previous,
        bytes.len(),
        sha256(bytes),
        prefix_bytes,
        get(input, "prefixSha256").as_str().unwrap_or("").to_string(),
    ))
}
fn registry(row: &V) -> Result<(V, V, i64), String> {
    let mut body = row.clone();
    let hash = body.as_object_mut().and_then(|m| m.shift_remove("hash")).unwrap_or(V::Null);
    need(
        js_int(get(&body, "sequence")).is_some_and(|n| n > 0)
            && hex(&hash)
            && sha256(js_stringify(&body).as_bytes()) == hash.as_str().unwrap_or("")
            && get(get(&body, "state"), "schemaVersion") == &json!(1),
        "REGISTRY_RECORD",
    )?;
    Ok((
        get(&body, "state").clone(),
        hash,
        js_int(get(&body, "sequence")).unwrap_or(0),
    ))
}

pub fn build_paired_owned_exit_retirement_v2(input: &V) -> Result<V, String> {
    let buy = get(input, "originalBuyProof");
    let sell = get(input, "sellProof");
    let sell_after = get(input, "sellAfter");
    let actor_journal = get(input, "actorJournal");
    let actor_status = get(input, "actorStatus");
    let funding_registry_record = get(input, "fundingRegistryRecord");
    let account_lease = get(input, "accountLease");
    let global_risk = get(input, "globalRisk");
    let read_at = get(input, "readAt");
    let n = instant_s(read_at).map_err(|e| e)?;
    proof(buy, "verificationHash")?;
    proof(sell, "verificationHash")?;
    need(
        ["scoped_rest_no_lot_with_rounded_cash_v1", "scoped_rest_no_lot_with_rounded_cash_neutral_all_v2"]
            .contains(&get(buy, "mode").as_str().unwrap_or(""))
            && get(buy, "ownerId") == &json!("Codex")
            && get(buy, "tournamentId") == &json!("bda92870-621e-47b0-bc3c-3602c5c26f55")
            && get(buy, "scopedLotVerified") == &json!(true)
            && get(buy, "filledQuantity") == &json!(1)
            && get(buy, "signedPositionDelta") == &json!(-1)
            && get(buy, "scopedPrincipalCost") == &json!("0.070000")
            && get(get(buy, "lot"), "side") == &json!("no")
            && get(get(buy, "lot"), "quantity") == &json!(1)
            && get(get(buy, "lot"), "ownerId") == get(buy, "ownerId")
            && get(get(buy, "lot"), "runId") == get(buy, "runId")
            && get(get(buy, "lot"), "entryPrice") == &json!("0.070000")
            && get(get(buy, "lot"), "orderId") == get(buy, "orderId"),
        "ORIGINAL_ONE_LOT",
    )?;
    need(
        ["scoped_rest_no_owned_exit_with_rounded_cash_v1", "scoped_rest_no_owned_exit_with_rounded_cash_neutral_all_v2"]
            .contains(&get(sell, "mode").as_str().unwrap_or(""))
            && get(sell, "scopedExitVerified") == &json!(true)
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
    let sfills = get(sell, "fills");
    need(
        sfills.as_array().is_some_and(|f| f.len() == 1)
            && get(&sfills[0], "side") == &json!("no")
            && get(&sfills[0], "signedQuantity") == &json!(-1)
            && get(&sfills[0], "quantity") == &json!(1)
            && get(&sfills[0], "priceBasis") == &json!("side_relative")
            && get(&sfills[0], "source") == &json!("rest_order_fills")
            && get(&sfills[0], "price") == get(sell, "scopedProceeds")
            && amount_s(get(sell, "scopedProceeds"))? > 0
            && money(amount_s(get(sell, "scopedProceeds"))? - 70_000) == get(sell, "realizedPnl").as_str().unwrap_or(""),
        "EXACT_SELL_PROCEEDS",
    )?;
    need(
        instant_s(get(sell, "readAt"))? <= n && n - instant_s(get(sell, "readAt"))? <= 20000,
        "SELL_PROOF_FRESHNESS",
    )?;
    if get(sell, "mode").as_str() == Some("scoped_rest_no_owned_exit_with_rounded_cash_neutral_all_v2") {
        let ae = get(sell, "ackEconomics");
        proof(ae, "verificationHash")?;
        need(
            get(ae, "mode") == &json!("observed_neutral_no_sell_ack_economics_v1")
                && get(ae, "sourceBodyHash") == get(get(get(sell, "sourceReceipts"), "ack"), "sourceBodyHash")
                && get(ae, "executionAuthority") == &json!(false)
                && ["null", "observed_neutral_sell"].contains(&get(ae, "allState").as_str().unwrap_or("")),
            "SELL_ACK_ECONOMICS_SOURCE",
        )?;
        if get(ae, "allState").as_str() == Some("observed_neutral_sell") {
            need(
                get(ae, "ackNotional") == get(sell, "scopedProceeds")
                    && get(ae, "fullNotionalCost") == get(sell, "scopedProceeds")
                    && get(ae, "effectiveEntryCost") == get(sell, "scopedProceeds")
                    && get(ae, "netBuyingPowerImpact") == &json!(money(-amount_s(get(sell, "scopedProceeds"))?))
                    && ["collateralSavings", "guaranteedPayoutFloorAfter", "outstandingAdvanceAfter", "collateralRepayment", "redemptionCredit"].iter()
                        .all(|k| get(ae, k) == &json!("0.000000"))
                    && get(ae, "relationshipIds").as_array().is_some_and(|a| a.is_empty())
                    && get(ae, "componentId") == &V::Null,
                "SELL_ACK_NEUTRAL_ECONOMICS",
            )?;
        }
    }
    let (jlast, jsequence, jhash, jbytes, jsha, jprefix_bytes, jprefix_sha) = journal(actor_journal)?;
    let p = get(get(&jlast, "state"), "plan").clone();
    let orders = get(get(&jlast, "state"), "orders");
    need(
        get(get(&p, "scope"), "ownerId") == get(buy, "ownerId")
            && get(get(&p, "scope"), "runId") == get(buy, "runId")
            && get(get(&p, "scope"), "tournamentId") == get(buy, "tournamentId")
            && hex(get(&p, "planHash"))
            && hex(get(&p, "captureHash"))
            && instant_s(get(&p, "reviewExpiresAt"))? <= n
            && orders.as_array().is_some_and(|o| o.len() == 1),
        "ORIGINAL_EXPIRED_ONE_ORDER",
    )?;
    proof(&p, "planHash")?;
    need(
        get(&p, "legs").as_array().is_some_and(|l| l.len() == 2)
            && get(&p, "legs").as_array().map(|l| {
                l.iter().map(|x| get(x, "exchangeId").to_string()).collect::<std::collections::BTreeSet<_>>().len() == 2
            }).unwrap_or(false)
            && get(&p, "legs").as_array().map(|l| {
                l.iter().any(|x| get(x, "marketId") == get(buy, "marketId") && get(x, "exchangeId") == get(buy, "exchangeId"))
            }).unwrap_or(false),
        "ORIGINAL_PAIR_SCOPE",
    )?;
    let o = &orders.as_array().unwrap()[0];
    need(
        get(o, "phase") == &json!("reconciled")
            && get(o, "orderId") == get(buy, "orderId")
            && get(o, "marketId") == get(buy, "marketId")
            && get(get(o, "body"), "exchangeId") == get(buy, "exchangeId")
            && get(get(o, "body"), "action") == &json!("buy")
            && get(get(o, "body"), "side") == &json!("no")
            && get(get(o, "body"), "quantity") == &json!(1)
            && !baal_service_runtime::json_text::js_truthy(Some(get(o, "cancelPending")))
            && !baal_service_runtime::json_text::js_truthy(Some(get(o, "semanticMismatch")))
            && get(get(o, "proof"), "verificationHash") == get(buy, "verificationHash")
            && payload_hash(get(o, "proof")).ok().as_deref() == payload_hash(buy).ok().as_deref(),
        "ORIGINAL_ACTOR_LOT",
    )?;
    need(
        get(actor_status, "planHash") == get(&p, "planHash")
            && get(actor_status, "sequence") == &json!(jsequence)
            && get(actor_status, "hash") == &json!(jhash)
            && get(actor_status, "orders").as_array().is_some_and(|o| o.len() == 1)
            && get(&get(actor_status, "orders").as_array().unwrap()[0], "phase") == &json!("reconciled")
            && get(&get(actor_status, "orders").as_array().unwrap()[0], "orderId") == get(buy, "orderId")
            && get(&get(actor_status, "orders").as_array().unwrap()[0], "proofHash") == get(buy, "verificationHash")
            && get(actor_status, "owned").as_array().is_some_and(|x| x.len() == 1)
            && get(&get(actor_status, "owned").as_array().unwrap()[0], "lotId") == get(get(buy, "lot"), "lotId")
            && get(&get(actor_status, "owned").as_array().unwrap()[0], "side") == &json!("no")
            && get(&get(actor_status, "owned").as_array().unwrap()[0], "ownerId") == get(buy, "ownerId")
            && get(&get(actor_status, "owned").as_array().unwrap()[0], "runId") == get(buy, "runId")
            && get(&get(actor_status, "owned").as_array().unwrap()[0], "orderId") == get(buy, "orderId")
            && get(&get(actor_status, "owned").as_array().unwrap()[0], "quantity") == &json!(1)
            && get(&get(actor_status, "owned").as_array().unwrap()[0], "exchangeId") == get(buy, "exchangeId")
            && get(&get(actor_status, "owned").as_array().unwrap()[0], "entryPrice") == &json!("0.070000"),
        "ACTUAL_ACTOR_STATUS",
    )?;
    let (rstate, rhash, rsequence) = registry(funding_registry_record)?;
    let f = get(&rstate, "funding").clone();
    let mut funding_body = f.clone();
    let reservation_hash = funding_body.as_object_mut().and_then(|m| m.shift_remove("reservationHash")).unwrap_or(V::Null);
    need(
        hex(&reservation_hash)
            && payload_hash(&funding_body).ok().as_deref() == reservation_hash.as_str()
            && get(&f, "ownerId") == get(buy, "ownerId")
            && get(&f, "runId") == get(buy, "runId")
            && get(&f, "tournamentId") == get(buy, "tournamentId")
            && get(&f, "planHash") == get(&p, "planHash")
            && get(&rstate, "planHash") == get(&p, "planHash")
            && get(&rstate, "captureHash") == get(&p, "captureHash")
            && get(&f, "ownedPrincipal") == &json!("0.070000")
            && get(&f, "cumulativeBuy") == &json!("0.070000")
            && get(&f, "reservedCash") == &json!("0.920000")
            && amount_s(get(&f, "reservedCash"))? + amount_s(get(&f, "cumulativeBuy"))? == amount_s(get(&p, "pairCostCeiling"))?
            && get(&rstate, "owned").as_array().is_some_and(|x| x.len() == 1)
            && get(&rstate, "owned").as_array().and_then(|x| x[0].get("lotId")).is_none()
            && get(&get(&rstate, "owned").as_array().unwrap()[0], "marketId") == get(buy, "marketId")
            && get(&get(&rstate, "owned").as_array().unwrap()[0], "exchangeId") == get(buy, "exchangeId")
            && get(&get(&rstate, "owned").as_array().unwrap()[0], "quantity") == &json!(1)
            && get(&get(&rstate, "owned").as_array().unwrap()[0], "entryPrice") == &json!("0.070000")
            && get(&get(&rstate, "owned").as_array().unwrap()[0], "orderId") == get(buy, "orderId")
            && get(&rstate, "pairedOrders").as_array().is_some_and(|x| x.len() == 1)
            && get(&get(&rstate, "pairedOrders").as_array().unwrap()[0], "phase") == &json!("reconciled")
            && get(&get(&rstate, "pairedOrders").as_array().unwrap()[0], "orderId") == get(buy, "orderId")
            && !baal_service_runtime::json_text::js_truthy(Some(get(&get(&rstate, "pairedOrders").as_array().unwrap()[0], "cancelPending")))
            && !baal_service_runtime::json_text::js_truthy(Some(get(&get(&rstate, "pairedOrders").as_array().unwrap()[0], "semanticMismatch")))
            && get(&get(&rstate, "pairedOrders").as_array().unwrap()[0], "proofHash") == get(buy, "verificationHash")
            && get(&rstate, "ended") == &json!(false)
            && get(&rstate, "exclusiveActive") == &json!(true),
        "ORIGINAL_FUNDING_LIABILITY",
    )?;
    need(
        get(get(&rstate, "actorAnchor"), "sequence") == &json!(jsequence)
            && get(get(&rstate, "actorAnchor"), "hash") == &json!(jhash)
            && get(get(&rstate, "actorAnchor"), "bytes") == &json!(jbytes as i64)
            && get(get(&rstate, "actorAnchor"), "sha256") == &json!(jsha),
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
    let account = inspect_scoped_rest_no_account(&json!({
        "account": sell_after,
        "binding": scope,
        "readAt": read_at,
    }))?;
    need(
        get(sell, "sourceReceipts").get("after").is_some_and(|x| x.is_array())
            && get(&account, "sourceReceipts").as_array().is_some_and(|a| {
                a.len() == get(get(sell, "sourceReceipts"), "after").as_array().map(|x| x.len()).unwrap_or(0)
                    && a.iter().all(|s| get(get(sell, "sourceReceipts"), "after").as_array().map(|arr| {
                        arr.iter().any(|x| {
                            get(x, "endpoint") == get(s, "endpoint")
                                && get(x, "sourceBodyHash") == get(s, "sourceBodyHash")
                                && get(x, "receivedAt") == get(s, "receivedAt")
                        })
                    }).unwrap_or(false))
            }),
        "SELL_AFTER_SOURCE",
    )?;
    let target = get(&account, "positions").as_array()
        .and_then(|arr| arr.iter().find(|p| get(p, "exchangeId") == get(buy, "exchangeId")));
    need(
        target.map(|t| !baal_service_runtime::json_text::js_truthy(t.get("quantity"))).unwrap_or(true)
            && target.map(|t| get(t, "marketId") == get(buy, "marketId")).unwrap_or(true),
        "ACTUAL_CURRENT_NO_NOT_FLAT",
    )?;
    let terminal = get(&account, "scopedOrders").as_array()
        .and_then(|arr| arr.iter().find(|o| baal_execution::js_key(get(o, "id")) == get(sell, "orderId").as_str().unwrap_or("")));
    need(
        terminal.is_some()
            && get(terminal.unwrap(), "side") == &json!("no")
            && get(terminal.unwrap(), "action") == &json!("sell")
            && get(terminal.unwrap(), "quantity") == &json!(1)
            && get(terminal.unwrap(), "quantityFilled") == &json!(1)
            && get(terminal.unwrap(), "open") == &json!(false),
        "TERMINAL_SELL_HISTORY",
    )?;
    need(
        get(account_lease, "ownerId") == get(buy, "ownerId")
            && get(account_lease, "tournamentId") == get(buy, "tournamentId")
            && hex(get(account_lease, "leaseId"))
            && hex(get(account_lease, "sourceHash"))
            && instant_s(get(account_lease, "at"))? >= instant_s(get(&account, "completedAt"))?
            && instant_s(get(account_lease, "at"))? <= n
            && n - instant_s(get(account_lease, "at"))? <= 20000,
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
            && instant_s(get(global_risk, "at"))? >= instant_s(get(&account, "completedAt"))?
            && instant_s(get(global_risk, "at"))? <= n
            && n - instant_s(get(global_risk, "at"))? <= 20000,
        "GLOBAL_UNKNOWN",
    )?;
    let body = json!({
        "schemaVersion": 1,
        "mode": "one_owned_no_paired_exit_retirement_neutral_sell_v2",
        "recordedAt": read_at,
        "ownerId": get(buy, "ownerId"),
        "runId": get(buy, "runId"),
        "tournamentId": get(buy, "tournamentId"),
        "marketId": get(buy, "marketId"),
        "exchangeId": get(buy, "exchangeId"),
        "lotId": get(get(buy, "lot"), "lotId"),
        "originalBuyProofHash": get(buy, "verificationHash"),
        "sellProofHash": get(sell, "verificationHash"),
        "originalActorPlanHash": get(&p, "planHash"),
        "originalCaptureHash": get(&p, "captureHash"),
        "originalActorSource": {
            "sequence": jsequence,
            "hash": jhash,
            "bytes": jbytes as i64,
            "sha256": jsha,
            "prefixBytes": jprefix_bytes as i64,
            "prefixSha256": jprefix_sha,
        },
        "originalFundingRecordHash": rhash,
        "originalFundingSequence": rsequence,
        "originalFundingReservationHash": get(&f, "reservationHash"),
        "currentAfterSourceHash": payload_hash(get(&account, "sourceReceipts"))?,
        "accountLeaseEvidenceHash": payload_hash(account_lease)?,
        "globalRiskHash": payload_hash(global_risk)?,
        "releasedUnusedReservation": get(&f, "reservedCash"),
        "releasedCurrentOwnedQuantity": 1,
        "currentOwnedQuantity": 0,
        "currentReservedCash": "0.000000",
        "originalAcquisitionCost": "0.070000",
        "preservedCumulativeBuy": "0.070000",
        "preservedOriginalHistoricalOwnedPrincipal": "0.070000",
        "scopedSellProceeds": get(sell, "scopedProceeds"),
        "scopedRealizedPnl": get(sell, "realizedPnl"),
        "originalRecordsModified": false,
        "originalEntryWindowRenewed": false,
        "newPairAuthorized": false,
        "globalCashReconciledExactly": false,
        "executionAuthority": false,
        "additionalClients": 0,
    });
    let mut out = body.clone();
    out.as_object_mut().unwrap().insert("overlayHash".into(), json!(payload_hash(&body)?));
    Ok(out)
}
