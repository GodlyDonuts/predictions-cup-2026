// execution/paired-no-exit-plan-v1.mjs — pure proposal/state core. Caller must
// persist each returned state before its sole account transport can act. This
// module provides no dispatch authority.
use crate::core::{canonical_payload, payload_hash};
use baal_service_runtime::fixed::{amount, instant, money, price, TICK};
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::collections::BTreeMap;

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("PAIRED_NO_EXIT_PLAN_{c}")) }
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
fn slug(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|seg| !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}
fn canon(x: Option<&V>) -> Result<String, String> {
    // JS canonicalPayload(x) — absent values throw INVALID_JSON_VALUE.
    match x {
        Some(v) => canonical_payload(v),
        None => Err("INVALID_JSON_VALUE".into()),
    }
}
// signedRows(xs): truthy-quantity rows keyed by exchangeId.
fn signed_rows(xs: &V) -> Map<String, V> {
    let mut out = Map::new();
    for x in xs.as_array().cloned().unwrap_or_default() {
        if js_truthy(x.get("quantity")) {
            out.insert(
                get(&x, "exchangeId").as_str().unwrap_or("").to_string(),
                x.clone(),
            );
        }
    }
    out
}
// hashCheck(value,key,code): strip `key`, verify hex hash of the body.
fn hash_check(value: &V, key: &str, code: &str) -> Result<V, String> {
    let mut body = value.clone();
    let hash = body.as_object_mut().and_then(|m| m.remove(key));
    need(
        hash.as_ref().is_some_and(|h| hex(h))
            && payload_hash(&body)? == hash.as_ref().and_then(|h| h.as_str()).unwrap_or(""),
        code,
    )?;
    Ok(body)
}
fn census(g: &V, scope: &V, n: i64, after: i64) -> Result<(), String> {
    need(
        get(g, "ownerId") == get(scope, "ownerId")
            && get(g, "runId") == get(scope, "runId")
            && get(g, "tournamentId") == get(scope, "tournamentId")
            && get(g, "complete") == &json!(true)
            && get(g, "soleCollector") == &json!(true)
            && get(g, "stopped") == &json!(false)
            && hex(get(g, "sourceHash"))
            && get(g, "unresolvedIntentIds").as_array().is_some_and(|a| a.is_empty())
            && instant_v(get(g, "at"))? >= after
            && instant_v(get(g, "at"))? <= n
            && n - instant_v(get(g, "at"))? <= 20_000,
        "GLOBAL_UNCERTAINTY",
    )
}
fn account_view(accounts: &V, leg: &V, scope: &V, at: &V) -> Result<V, String> {
    let exchange_id = get(leg, "exchangeId").as_str().unwrap_or("");
    need(
        accounts.as_object().is_some_and(|m| m.contains_key(exchange_id)),
        "SCOPED_ACCOUNT_REQUIRED",
    )?;
    let mut binding = scope.as_object().cloned().unwrap_or_default();
    binding.insert("exchangeId".into(), json!(exchange_id));
    crate::no_rest_reconciliation_v2::inspect_scoped_rest_no_account(&json!({
        "account": accounts.get(exchange_id).unwrap(),
        "binding": binding,
        "readAt": at,
    }))
}

struct Source {
    views: Vec<V>,
    source_hash: String,
    global_risk_hash: String,
    completed_at: String,
}

fn positions_and_history(plan: &V, accounts: &V, global_risk: &V, read_at: &V) -> Result<Source, String> {
    let n = instant_v(read_at)?;
    let legs = get(plan, "legs").as_array().cloned().unwrap_or_default();
    let views: Vec<V> = legs
        .iter()
        .map(|l| account_view(accounts, l, get(plan, "scope"), read_at))
        .collect::<Result<_, _>>()?;
    let first_positions = signed_rows(get(&views[0], "positions"));
    need(
        views.iter().all(|v| {
            payload_hash(&V::Object(signed_rows(get(v, "positions"))))
                .unwrap_or_default()
                == payload_hash(&V::Object(first_positions.clone())).unwrap_or_default()
        }),
        "ACCOUNT_POSITION_CONFLICT",
    )?;
    for (i, leg) in legs.iter().enumerate().take(2) {
        let view = &views[i];
        let p = get(view, "positions")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .find(|p| get(p, "exchangeId") == get(leg, "exchangeId"))
            .cloned()
            .unwrap_or(V::Null);
        need(
            get(&p, "marketId") == get(leg, "marketId")
                && get(&p, "quantity") == &json!(-1)
                && get(&p, "settled") == &json!(false),
            "CURRENT_OWNED_NO_REQUIRED",
        )?;
        need(
            instant_v(get(view, "startedAt"))?
                >= instant_v(get(&get(plan, "openingEvidence")[i], "completedAt"))?,
            "ACCOUNT_ROLLBACK",
        )?;
        need(
            payload_hash(get(view, "scopedOrders"))?
                == get(&get(plan, "openingEvidence")[i], "scopedOrdersHash").as_str().unwrap_or(""),
            "SCOPED_HISTORY_CHANGED",
        )?;
    }
    for (exchange_id, p) in first_positions.iter() {
        if !legs.iter().any(|l| get(l, "exchangeId").as_str() == Some(exchange_id.as_str())) {
            need(
                canonical_payload(p)?
                    == canon(get(plan, "externalPositions").get(exchange_id))?,
                "EXTERNAL_POSITION_CHANGED",
            )?;
        }
    }
    if let Some(ext) = get(plan, "externalPositions").as_object() {
        for id in ext.keys() {
            need(
                canonical_payload(first_positions.get(id).unwrap_or(&V::Null))?
                    == canon(get(plan, "externalPositions").get(id))?,
                "EXTERNAL_POSITION_CHANGED",
            )?;
        }
    }
    let after = views
        .iter()
        .map(|v| instant_v(get(v, "completedAt")))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .max()
        .unwrap_or(0);
    census(global_risk, get(plan, "scope"), n, after)?;
    Ok(Source {
        source_hash: payload_hash(&V::Array(
            views.iter().map(|v| get(v, "sourceReceipts").clone()).collect(),
        ))?,
        global_risk_hash: payload_hash(global_risk)?,
        completed_at: baal_service_runtime::contract::ms_to_iso(after as f64),
        views,
    })
}

// createPairedNoExitPlan({signal,scope,buyProofs,originalAfterAccountsByExchange,accountsByExchange,globalRisk,readAt})
pub fn create_paired_no_exit_plan(input: &V) -> Result<V, String> {
    let signal = get(input, "signal");
    let scope = get(input, "scope");
    let buy_proofs = get(input, "buyProofs");
    let original_after = get(input, "originalAfterAccountsByExchange");
    let accounts = get(input, "accountsByExchange");
    let global_risk = get(input, "globalRisk");
    let read_at = get(input, "readAt");
    let n = instant_v(read_at)?;
    need(
        get(signal, "signal") == &json!(true)
            && get(signal, "executionAuthority") == &json!(false)
            && get(get(signal, "plan"), "executionAuthority") == &json!(false),
        "POSITIVE_ECONOMIC_SIGNAL_REQUIRED",
    )?;
    hash_check(get(signal, "plan"), "planHash", "ECONOMIC_PLAN_HASH")?;
    let economic = get(signal, "plan");
    need(
        get(economic, "mode") == &json!("paired_no_liquidation_plan_v1")
            && payload_hash(get(economic, "scope"))? == payload_hash(scope)?
            && get(scope, "ownerId") == &json!("Codex")
            && slug(get(scope, "tournamentSlug").as_str().unwrap_or(""))
            && buy_proofs.as_array().is_some_and(|a| a.len() == 2),
        "SCOPE",
    )?;
    need(
        n >= instant_v(get(economic, "createdAt"))? && n < instant_v(get(economic, "expiresAt"))?,
        "SIGNAL_EXPIRED",
    )?;
    let legs = get(economic, "legs").clone();
    need(
        legs.as_array().is_some_and(|a| a.len() == 2)
            && legs.as_array().unwrap().iter().map(|l| get(l, "exchangeId").as_str().unwrap_or("")).collect::<std::collections::BTreeSet<_>>().len() == 2
            && legs.as_array().unwrap().iter().map(|l| get(l, "lotId").as_str().unwrap_or("")).collect::<std::collections::BTreeSet<_>>().len() == 2,
        "DISTINCT_OWNED_LOTS",
    )?;
    let mut opening_evidence: Vec<V> = Vec::new();
    for leg in legs.as_array().unwrap() {
        let proof = buy_proofs
            .as_array()
            .unwrap()
            .iter()
            .find(|p| get(p, "exchangeId") == get(leg, "exchangeId"))
            .cloned()
            .unwrap_or(V::Null);
        hash_check(&proof, "verificationHash", "ORIGINAL_BUY_PROOF_HASH")?;
        need(
            get(&proof, "mode") == &json!("scoped_rest_no_lot_with_rounded_cash_v1")
                && get(&proof, "ownerId") == get(scope, "ownerId")
                && get(&proof, "runId") == get(scope, "runId")
                && get(&proof, "tournamentId") == get(scope, "tournamentId")
                && get(&proof, "marketId") == get(leg, "marketId")
                && get(&proof, "scopedLotVerified") == &json!(true)
                && get(&proof, "filledQuantity") == &json!(1)
                && get(&proof, "signedPositionDelta") == &json!(-1)
                && get(&proof, "verificationHash") == get(leg, "proofHash")
                && get(economic, "originalProofHashes").as_array().is_some_and(|a| {
                    a.iter().any(|h| h == get(&proof, "verificationHash"))
                })
                && get(get(&proof, "lot"), "side").as_str() == Some("no")
                && get(get(&proof, "lot"), "quantity") == &json!(1)
                && get(get(&proof, "lot"), "lotId") == get(leg, "lotId")
                && get(get(&proof, "lot"), "entryPrice") == get(leg, "entryPrice")
                && get(&proof, "scopedPrincipalCost") == get(leg, "entryPrice")
                && get(&proof, "orderId") == get(leg, "orderId"),
            "LOT_PROVENANCE",
        )?;
        price(get(leg, "entryPrice").as_str().unwrap_or(""))?;
        price(get(leg, "protectedSellLimit").as_str().unwrap_or(""))?;
        let original = account_view(original_after, leg, scope, get(&proof, "readAt"))?;
        need(
            get(&proof, "sourceReceipts").get("after").is_some_and(V::is_array)
                && get(&original, "sourceReceipts").as_array().map(|a| a.len())
                    == get(&proof, "sourceReceipts").get("after").and_then(V::as_array).map(|a| a.len())
                && get(&original, "sourceReceipts").as_array().unwrap().iter().all(|r| {
                    get(&proof, "sourceReceipts").get("after").and_then(V::as_array).unwrap().iter().any(|p| {
                        get(p, "endpoint") == get(r, "endpoint")
                            && get(p, "sourceBodyHash") == get(r, "sourceBodyHash")
                            && get(p, "receivedAt") == get(r, "receivedAt")
                    })
                }),
            "ORIGINAL_AFTER_RECEIPTS",
        )?;
        let p = get(&original, "positions")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .find(|p| get(p, "exchangeId") == get(leg, "exchangeId"))
            .cloned()
            .unwrap_or(V::Null);
        need(
            get(&p, "marketId") == get(leg, "marketId")
                && get(&p, "quantity") == &json!(-1)
                && !js_truthy(p.get("settled")),
            "ORIGINAL_SIGNED_LOT",
        )?;
        let buy = get(&original, "scopedOrders")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .find(|o| {
                js_int(get(o, "id")).map(|x| x.to_string())
                    == Some(get(&proof, "orderId").as_str().unwrap_or("").to_string())
            })
            .cloned()
            .unwrap_or(V::Null);
        need(
            !buy.is_null()
                && get(&buy, "side").as_str() == Some("no")
                && get(&buy, "action").as_str() == Some("buy")
                && get(&buy, "quantity") == &json!(1)
                && get(&buy, "quantityFilled") == &json!(1)
                && get(&buy, "open") == &json!(false),
            "ORIGINAL_BUY_HISTORY",
        )?;
        opening_evidence.push(json!({
            "exchangeId": get(leg, "exchangeId"),
            "completedAt": get(&original, "completedAt"),
            "scopedOrdersHash": payload_hash(get(&original, "scopedOrders"))?,
            "sourceReceiptsHash": payload_hash(get(&original, "sourceReceipts"))?,
        }));
    }
    let views: Vec<V> = legs
        .as_array()
        .unwrap()
        .iter()
        .map(|l| account_view(accounts, l, scope, read_at))
        .collect::<Result<_, _>>()?;
    let mut external_positions = signed_rows(get(&views[0], "positions"));
    for leg in legs.as_array().unwrap() {
        external_positions.shift_remove(get(leg, "exchangeId").as_str().unwrap_or(""));
    }
    for (i, leg) in legs.as_array().unwrap().iter().enumerate().take(2) {
        let proof = buy_proofs
            .as_array()
            .unwrap()
            .iter()
            .find(|p| get(p, "exchangeId") == get(leg, "exchangeId"))
            .cloned()
            .unwrap_or(V::Null);
        let original = account_view(original_after, leg, scope, get(&proof, "readAt"))?;
        let mut old = signed_rows(get(&original, "positions"));
        for leg in legs.as_array().unwrap() {
            old.shift_remove(get(leg, "exchangeId").as_str().unwrap_or(""));
        }
        let _ = i;
        need(
            payload_hash(&V::Object(old))? == payload_hash(&V::Object(external_positions.clone()))?,
            "EXTERNAL_POSITION_CHANGED",
        )?;
    }
    let cost: i128 = legs
        .as_array()
        .unwrap()
        .iter()
        .map(|l| amount_v(get(l, "entryPrice")))
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .sum();
    let proceeds: i128 = legs
        .as_array()
        .unwrap()
        .iter()
        .map(|l| amount_v(get(l, "protectedSellLimit")))
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .sum();
    need(
        money(cost) == get(economic, "ownedPrincipal").as_str().unwrap_or("")
            && money(proceeds) == get(economic, "protectedProceeds").as_str().unwrap_or("")
            && proceeds >= cost + TICK,
        "PAIR_PROCEEDS_FLOOR",
    )?;
    let mut initial = Map::from_iter([
        ("schemaVersion".into(), json!(1)),
        ("mode".into(), json!("owned_no_pair_exit_proposal_v1")),
        ("scope".into(), scope.clone()),
        ("economicPlanHash".into(), get(economic, "planHash").clone()),
        ("originalBuyProofHashes".into(), legs.as_array().unwrap().iter().map(|l| get(l, "proofHash").clone()).collect::<Vec<V>>().into()),
        ("createdAt".into(), read_at.clone()),
        ("expiresAt".into(), get(economic, "expiresAt").clone()),
        ("legs".into(), legs.clone()),
        ("openingEvidence".into(), V::Array(opening_evidence)),
        ("externalPositions".into(), V::Object(external_positions)),
        ("originalOwnedPrincipal".into(), json!(money(cost))),
        ("minimumPairNetGain".into(), json!("0.005000")),
        ("minimumCombinedProceeds".into(), json!(money(cost + TICK))),
        ("protectedCombinedProceeds".into(), json!(money(proceeds))),
        ("fee".into(), json!("0.000000")),
        ("quantityPerLeg".into(), json!(1)),
        ("nonAtomic".into(), json!(true)),
        ("orphanPossible".into(), json!(true)),
        ("executionAuthority".into(), json!(false)),
    ]);
    let current = positions_and_history(&V::Object(initial.clone()), accounts, global_risk, read_at)?;
    let coe = json!({
        "accountSourceHash": current.source_hash,
        "globalRiskHash": current.global_risk_hash,
        "completedAt": current.completed_at,
    });
    let mut hash_body = initial.clone();
    hash_body.insert("currentOwnershipEvidence".into(), coe.clone());
    let plan_hash = payload_hash(&V::Object(hash_body))?;
    initial.insert("currentOwnershipEvidence".into(), coe);
    initial.insert("planHash".into(), json!(plan_hash));
    Ok(V::Object(initial))
}

fn sealed(mut body: V) -> Result<V, String> {
    let h = payload_hash(&body)?;
    body.as_object_mut().unwrap().insert("stateHash".into(), json!(h));
    Ok(body)
}

fn checked(state: &V) -> Result<V, String> {
    let body = hash_check(state, "stateHash", "STATE_HASH")?;
    hash_check(get(&body, "plan"), "planHash", "PLAN_HASH")?;
    need(
        get(&body, "schemaVersion") == &json!(1)
            && get(&body, "mode") == &json!("paired_no_exit_state_v1")
            && js_int(get(&body, "sequence")).is_some_and(|s| s >= 0)
            && get(&body, "intents").as_array().is_some_and(|a| a.len() <= 1)
            && get(&body, "executionAuthority") == &json!(false)
            && get(&body, "cashProceedsVerified") == &json!("0.000000")
            && get(&body, "realizedPnlVerified") == &json!("0.000000"),
        "STATE",
    )?;
    let expected: Vec<V> = get(&body, "plan")
        .get("legs")
        .and_then(V::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|l| {
            json!({
                "lotId": get(l, "lotId"),
                "marketId": get(l, "marketId"),
                "exchangeId": get(l, "exchangeId"),
                "quantity": 1,
                "entryPrice": get(l, "entryPrice"),
                "proofHash": get(l, "proofHash"),
            })
        })
        .collect();
    need(
        payload_hash(&V::Array(expected))? == payload_hash(get(&body, "reservedOwnedLots"))?,
        "OWNED_RESERVATION_RESET",
    )?;
    if let Some(intents) = get(&body, "intents").as_array().filter(|a| !a.is_empty()) {
        let i = &intents[0];
        let legs_v = get(&body, "plan").get("legs").cloned().unwrap_or(V::Array(vec![]));
        let l = &legs_v[0];
        let plan_hash = get(get(&body, "plan"), "planHash").as_str().unwrap_or("");
        let key = format!("paired-no-exit-{}-0", &plan_hash[..plan_hash.len().min(48)]);
        let price_number: f64 = get(l, "protectedSellLimit").as_str().unwrap_or("").parse().unwrap_or(f64::NAN);
        let wire = json!({
            "idempotencyKey": key,
            "exchangeId": get(l, "exchangeId"),
            "side": "no",
            "action": "sell",
            "quantity": 1,
            "price": price_number,
            "expirationDate": get(get(&body, "plan"), "expiresAt"),
            "tournamentId": get(get(get(&body, "plan"), "scope"), "tournamentId"),
        });
        let body_hash = payload_hash(&wire)?;
        need(
            get(i, "intentId") == &json!(format!("paired-no-sell:{plan_hash}:0"))
                && get(i, "idempotencyKey") == &json!(key)
                && get(i, "marketId") == get(l, "marketId")
                && get(i, "exchangeId") == get(l, "exchangeId")
                && get(i, "lotId") == get(l, "lotId")
                && get(i, "originalBuyProofHash") == get(l, "proofHash")
                && get(i, "orderId") == &V::Null
                && matches!(get(i, "phase").as_str(), Some("prepared" | "dispatching" | "uncertain"))
                && get(i, "bodyJson").as_str() == Some(canonical_payload(&wire)?.as_str())
                && payload_hash(get(i, "body"))? == body_hash
                && get(i, "bodyHash").as_str() == Some(body_hash.as_str())
                && instant_v(get(i, "preparedAt"))? >= instant_v(get(get(&body, "plan"), "createdAt"))?
                && instant_v(get(i, "preparedAt"))? < instant_v(get(get(&body, "plan"), "expiresAt"))?,
            "IMMUTABLE_SELL_INTENT",
        )?;
    }
    Ok(body)
}

// createPairedNoExitState(plan)
pub fn create_paired_no_exit_state(plan: &V) -> Result<V, String> {
    hash_check(plan, "planHash", "PLAN_HASH")?;
    need(
        get(plan, "mode") == &json!("owned_no_pair_exit_proposal_v1")
            && get(plan, "executionAuthority") == &json!(false),
        "PLAN",
    )?;
    sealed(json!({
        "schemaVersion": 1,
        "mode": "paired_no_exit_state_v1",
        "sequence": 0,
        "plan": plan.clone(),
        "intents": [],
        "reservedOwnedLots": get(plan, "legs").as_array().cloned().unwrap_or_default().iter().map(|l| json!({
            "lotId": get(l, "lotId"),
            "marketId": get(l, "marketId"),
            "exchangeId": get(l, "exchangeId"),
            "quantity": 1,
            "entryPrice": get(l, "entryPrice"),
            "proofHash": get(l, "proofHash"),
        })).collect::<Vec<V>>(),
        "cashProceedsVerified": "0.000000",
        "realizedPnlVerified": "0.000000",
        "executionAuthority": false,
    }))
}

// prepareNextPairedNoSell(state,{accountsByExchange,globalRisk,readAt})
pub fn prepare_next_paired_no_sell(state: &V, input: &V) -> Result<V, String> {
    let mut s = checked(state)?;
    let read_at = get(input, "readAt");
    let n = instant_v(read_at)?;
    need(
        get(&s, "intents").as_array().is_some_and(|a| a.is_empty()),
        "UNKNOWN_OR_UNRECONCILED_SELL_BLOCKS_SECOND",
    )?;
    need(
        n >= instant_v(get(get(&s, "plan"), "createdAt"))?
            && n < instant_v(get(get(&s, "plan"), "expiresAt"))?,
        "PLAN_EXPIRED",
    )?;
    let source = positions_and_history(
        get(&s, "plan"),
        get(input, "accountsByExchange"),
        get(input, "globalRisk"),
        read_at,
    )?;
    let leg = get(&s, "plan").get("legs").cloned().unwrap_or(V::Array(vec![]))[0].clone();
    let plan_hash = get(get(&s, "plan"), "planHash").as_str().unwrap_or("").to_string();
    let intent_id = format!("paired-no-sell:{plan_hash}:0");
    let idempotency_key = format!("paired-no-exit-{}-0", &plan_hash[..plan_hash.len().min(48)]);
    let price_number: f64 = get(&leg, "protectedSellLimit").as_str().unwrap_or("").parse().unwrap_or(f64::NAN);
    let body = json!({
        "idempotencyKey": idempotency_key,
        "exchangeId": get(&leg, "exchangeId"),
        "side": "no",
        "action": "sell",
        "quantity": 1,
        "price": price_number,
        "expirationDate": get(get(&s, "plan"), "expiresAt"),
        "tournamentId": get(get(get(&s, "plan"), "scope"), "tournamentId"),
    });
    let body_json = canonical_payload(&body)?;
    let body_hash = payload_hash(&body)?;
    let intent = json!({
        "intentId": intent_id,
        "idempotencyKey": idempotency_key,
        "marketId": get(&leg, "marketId"),
        "exchangeId": get(&leg, "exchangeId"),
        "lotId": get(&leg, "lotId"),
        "originalBuyProofHash": get(&leg, "proofHash"),
        "body": body,
        "bodyJson": body_json,
        "bodyHash": body_hash,
        "phase": "prepared",
        "preparedAt": read_at,
        "accountSourceHash": source.source_hash,
        "globalRiskHash": source.global_risk_hash,
        "orderId": V::Null,
    });
    s.as_object_mut()
        .unwrap()
        .get_mut("intents")
        .unwrap()
        .as_array_mut()
        .unwrap()
        .push(intent.clone());
    let seq = js_int(get(&s, "sequence")).unwrap_or(0) + 1;
    s.as_object_mut().unwrap().insert("sequence".into(), json!(seq));
    Ok(json!({
        "state": sealed(s)?,
        "intent": intent,
        "request": {
            "method": "POST",
            "endpoint": "/orders",
            "body": body,
            "bodyJson": body_json,
        },
        "executionAuthority": false,
    }))
}

// beginPairedNoSell(state,intentId)
pub fn begin_paired_no_sell(state: &V, intent_id: &str) -> Result<V, String> {
    let mut s = checked(state)?;
    let intents = s
        .as_object_mut()
        .unwrap()
        .get_mut("intents")
        .and_then(V::as_array_mut)
        .cloned()
        .unwrap_or_default();
    let mut intents = intents;
    let pos = intents.iter().position(|i| get(i, "intentId").as_str() == Some(intent_id));
    need(pos.is_some(), "DISPATCH_PHASE")?;
    let i = intents[pos.unwrap()].clone();
    need(
        get(&i, "phase").as_str() == Some("prepared")
            && get(&i, "bodyJson").as_str() == Some(canonical_payload(get(&i, "body"))?.as_str())
            && get(&i, "bodyHash").as_str() == Some(payload_hash(get(&i, "body"))?.as_str()),
        "DISPATCH_PHASE",
    )?;
    intents[pos.unwrap()].as_object_mut().unwrap().insert("phase".into(), json!("dispatching"));
    s.as_object_mut().unwrap().insert("intents".into(), V::Array(intents));
    let seq = js_int(get(&s, "sequence")).unwrap_or(0) + 1;
    s.as_object_mut().unwrap().insert("sequence".into(), json!(seq));
    sealed(s)
}

// recordPairedNoSellUnknown(state,intentId)
pub fn record_paired_no_sell_unknown(state: &V, intent_id: &str) -> Result<V, String> {
    let mut s = checked(state)?;
    let mut intents = s
        .as_object_mut()
        .unwrap()
        .get_mut("intents")
        .and_then(V::as_array_mut)
        .cloned()
        .unwrap_or_default();
    let pos = intents.iter().position(|i| get(i, "intentId").as_str() == Some(intent_id));
    let i = pos.map(|p| intents[p].clone());
    need(
        i.as_ref().is_some_and(|i| matches!(get(i, "phase").as_str(), Some("dispatching" | "uncertain"))),
        "UNKNOWN_PHASE",
    )?;
    intents[pos.unwrap()].as_object_mut().unwrap().insert("phase".into(), json!("uncertain"));
    s.as_object_mut().unwrap().insert("intents".into(), V::Array(intents));
    let seq = js_int(get(&s, "sequence")).unwrap_or(0) + 1;
    s.as_object_mut().unwrap().insert("sequence".into(), json!(seq));
    sealed(s)
}

// pairedNoExitStatus(state)
pub fn paired_no_exit_status(state: &V) -> Result<V, String> {
    let s = checked(state)?;
    let intents = get(&s, "intents").as_array().cloned().unwrap_or_default();
    Ok(json!({
        "planHash": get(get(&s, "plan"), "planHash"),
        "sequence": get(&s, "sequence"),
        "phase": if intents.is_empty() { json!("ready") } else { get(&intents[0], "phase").clone() },
        "reservedOwnedLots": get(&s, "reservedOwnedLots"),
        "cashProceedsVerified": get(&s, "cashProceedsVerified"),
        "realizedPnlVerified": get(&s, "realizedPnlVerified"),
        "secondLegBlocked": !intents.is_empty(),
        "requiresExactScopedRestNoSellVerifier": true,
        "nonAtomic": true,
        "orphanPossible": true,
        "executionAuthority": false,
    }))
}
