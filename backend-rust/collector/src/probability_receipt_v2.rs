// live-paper/probability-receipt-v2.mjs — owned-risk continuation probability paper.
use baal_paper::fixed::{big_amount, money, TICK};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

pub fn probability_receipt_policy_v2() -> V {
    json!({"accountPolicy":"position_entry_block_open_order_fill_block_v2",
        "minimumLowerEdge":"0.020000","maximumSpread":"0.020000","maximumResearchAgeMs":21600000,
        "maximumAccountAgeMs":20000,"maxHoldMs":172800000,"takeProfit":"0.020000",
        "maxMarketCost":"1.000000","maxOpenMarkets":3,"cumulativeBuyBudget":"100.000000",
        "latenciesMs":[1000,3000]})
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("PROBABILITY_RECEIPT_{}", code)); }
    Ok(())
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| !s.is_empty() && s.as_bytes()[0] != b'0'
        && s.bytes().all(|b| b.is_ascii_digit()) && s.parse::<i64>().is_ok())
}
fn instant_of(v: &V) -> i64 { crate::model::instant(v.as_str().unwrap_or("")).unwrap_or(i64::MIN) }
fn digest(v: &V) -> String { baal_service_runtime::json_text::digest(v) }
fn hex64(x: &V) -> bool {
    x.as_str().is_some_and(|s| s.len() == 64
        && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}

/// validateProbabilityReceiptBank(bank) — v3: schemaVersion 3, owned-risk mode.
pub fn validate_probability_receipt_bank(bank: &V) -> Result<(), String> {
    need(bank["schemaVersion"].as_i64() == Some(3)
        && bank["mode"].as_str() == Some("source_probability_owned_risk_continuation_v2")
        && digest(&bank["policy"]) == digest(&probability_receipt_policy_v2()), "BANK")?;
    crate::model::validate_config(&bank["config"])?;
    let c = &bank["config"];
    need(c["feeBps"].as_i64() == Some(0) && c["depthFractionBps"].as_i64() == Some(100)
        && c["slippageTicks"].as_i64() == Some(1) && c["orderShares"].as_i64() == Some(1)
        && c["maxPositionShares"].as_i64() == Some(1) && c["initialCash"].as_str() == Some("100.000000")
        && c["cashReserve"].as_str() == Some("90.000000") && c["maxExposure"].as_str() == Some("10.000000")
        && c["stopLoss"].as_str() == Some("0.050000") && c["strategyUniverse"].as_str() == Some("diversified-dip-v1")
        && c["maxDataAgeMs"].as_i64() == Some(20000)
        && c["marketIds"].as_array().is_some_and(|m| m.len() == bank["research"].as_array().map(|r| r.len()).unwrap_or(0)), "FILL_ASSUMPTIONS")?;
    need(bank["research"].as_array().is_some_and(|r| !r.is_empty() && r.len() <= 3)
        && bank["identities"].as_object().is_some_and(|i| i.len() == bank["research"].as_array().unwrap().len()), "RESEARCH_SET")?;
    let mut seen = BTreeSet::new();
    for r in bank["research"].as_array().unwrap() {
        crate::probability_paper::validate_research(r)?;
        let spec = &bank["identities"][r["marketId"].as_str().unwrap_or("")];
        need(!seen.contains(r["marketId"].as_str().unwrap())
            && instant_of(&r["recordedAt"]) <= instant_of(&bank["cutoff"])
            && spec["exchangeId"] == r["exchangeId"] && spec["predicateHash"] == r["predicateHash"]
            && bank["universe"]["exchanges"].as_array().is_some_and(|xs| xs.iter().any(|x| x["marketId"] == r["marketId"] && x["exchangeId"] == r["exchangeId"] && x["option"].as_str() == Some("YES")))
            && c["marketIds"].as_array().unwrap().contains(&r["marketId"]), "RESEARCH_BINDING")?;
        seen.insert(r["marketId"].as_str().unwrap().to_string());
    }
    let cont = &bank["accountingContinuation"];
    need(cont["accountingCarried"] == V::Bool(true) && cont["executionAuthority"] == V::Bool(false)
        && ["openingFileHash","priorBankHash","priorBankFileHash","priorStateHash","priorJournalHash","priorJournalFileHash"]
            .iter().all(|k| hex64(&cont[k]))
        && cont["priorJournalBytes"].as_i64().is_some_and(|x| x > 0)
        && bank["sourcePrefixBytes"].as_i64().is_some_and(|x| x > 0)
        && bank["sourcePrefixSequence"].as_i64().is_some_and(|x| x > 0)
        && hex64(&bank["sourcePrefixHash"]) && hex64(&bank["sourcePrefixJournalHash"]), "CONTINUATION")?;
    Ok(())
}

/// probabilityAccountEligibility(cache) — schemaVersion 2; null on any failure.
pub fn probability_account_eligibility(cache: &V) -> Option<V> {
    (|| -> Result<V, String> {
        need(cache["errors"]["positions"].is_null() && cache["errors"]["orders"].is_null()
            && cache["positions"]["rows"].is_array() && cache["orders"]["rows"].is_array(), "ACCOUNT_UNAVAILABLE")?;
        let p_at = instant_of(&cache["positions"]["at"]);
        let o_at = instant_of(&cache["orders"]["at"]);
        let mut positions = BTreeSet::new();
        let mut orders = BTreeSet::new();
        for p in cache["positions"]["rows"].as_array().unwrap() {
            need(id(&p["exchangeId"]) && p["quantity"].as_i64().is_some_and(|q| q > 0), "ACCOUNT_POSITION")?;
            positions.insert(p["exchangeId"].as_str().unwrap().to_string());
        }
        for o in cache["orders"]["rows"].as_array().unwrap() {
            need(id(&o["exchangeId"]) && o["open"] == V::Bool(true)
                && o["quantity"].as_f64().is_some_and(|q| q > 0.0), "ACCOUNT_ORDER")?;
            orders.insert(o["exchangeId"].as_str().unwrap().to_string());
        }
        let sorted = |s: BTreeSet<String>| -> Vec<V> {
            let mut v: Vec<String> = s.into_iter().collect();
            v.sort_by_key(|x| x.parse::<i64>().unwrap_or(0));
            v.into_iter().map(V::String).collect()
        };
        Ok(json!({"schemaVersion":2,"positionsAt":cache["positions"]["at"],"ordersAt":cache["orders"]["at"],
            "positionExchangeIds":sorted(positions),"openOrderExchangeIds":sorted(orders)}))
            .map(|v| { let _ = (p_at, o_at); v })
    })().ok()
}

/// probabilityAccountGate(account, b, availableAt, policy, phase) — reason string or null.
pub fn probability_account_gate(account: Option<&V>, b: &V, available_at: &str, policy: Option<&V>, phase: &str) -> V {
    let result = (|| -> Result<V, String> {
        let p = policy.cloned().unwrap_or_else(probability_receipt_policy_v2);
        let account = account.ok_or("x")?;
        let keys: String = account.as_object().map(|m| {
            let mut v: Vec<String> = m.keys().cloned().collect();
            v.sort();
            v
        }).unwrap_or_default().join("|");
        if !(["entry", "reduce_only"].contains(&phase) && account["schemaVersion"].as_i64() == Some(2)
            && keys == "openOrderExchangeIds|ordersAt|positionExchangeIds|positionsAt|schemaVersion"
            && ["positionExchangeIds", "openOrderExchangeIds"].iter().all(|k| {
                account[k].as_array().is_some_and(|a| a.iter().all(id)
                    && a.iter().filter_map(|x| x.as_str()).collect::<BTreeSet<_>>().len() == a.len())
            })) {
            return Ok(V::String("account_unavailable".into()));
        }
        let now = instant_of(&json!(available_at));
        let request = instant_of(&b["requestStartedAt"]);
        if [instant_of(&account["positionsAt"]), instant_of(&account["ordersAt"])].iter()
            .any(|t| *t > request || now - t > p["maximumAccountAgeMs"].as_i64().unwrap_or(0)) {
            return Ok(V::String("account_not_causal_or_fresh".into()));
        }
        if account["openOrderExchangeIds"].as_array().is_some_and(|a| a.contains(&b["exchangeId"])) {
            return Ok(V::String("own_account_open_order".into()));
        }
        if phase == "entry"
            && account["positionExchangeIds"].as_array().is_some_and(|a| a.contains(&b["exchangeId"])) {
            return Ok(V::String("own_account_exposure".into()));
        }
        Ok(V::Null)
    })();
    result.unwrap_or(V::String("account_unavailable".into()))
}

/// validateProbabilityOpening(opening, bank)
pub fn validate_probability_opening(opening: &V, bank: &V) -> Result<V, String> {
    need(opening["schemaVersion"].as_i64() == Some(1)
        && digest(&opening["priorBank"]) == bank["accountingContinuation"]["priorBankHash"].as_str().unwrap_or("")
        && opening["priorBankFileHash"] == bank["accountingContinuation"]["priorBankFileHash"]
        && opening["priorBank"]["name"] == bank["accountingContinuation"]["priorCohort"]
        && opening["state"]["bankHash"] == bank["accountingContinuation"]["priorBankHash"]
        && digest(&opening["state"]) == bank["accountingContinuation"]["priorStateHash"].as_str().unwrap_or(""), "OPENING_IDENTITY")?;
    crate::probability_receipt::validate_probability_receipt_bank(&opening["priorBank"])?;
    need(opening["sourceBytes"] == bank["sourcePrefixBytes"]
        && opening["sourceSequence"] == bank["sourcePrefixSequence"]
        && opening["sourceHash"] == bank["sourcePrefixJournalHash"]
        && opening["sourcePrefixHash"] == bank["sourcePrefixHash"]
        && opening["journalHash"] == bank["accountingContinuation"]["priorJournalHash"]
        && opening["journalFileHash"] == bank["accountingContinuation"]["priorJournalFileHash"]
        && opening["journalBytes"] == bank["accountingContinuation"]["priorJournalBytes"], "OPENING_FRONTIER")?;
    need(digest(&opening["priorBank"]["config"]) == digest(&bank["config"])
        && digest(&opening["priorBank"]["research"]) == digest(&bank["research"])
        && digest(&opening["priorBank"]["identities"]) == digest(&bank["identities"])
        && digest(&opening["priorBank"]["universe"]) == digest(&bank["universe"])
        && instant_of(&opening["state"]["lastAvailableAt"]) <= instant_of(&bank["cutoff"]), "OPENING_SCOPE")?;
    let wallet_keys: String = opening["state"]["wallets"].as_object().map(|m| {
        let mut v: Vec<String> = m.keys().cloned().collect();
        v.sort();
        v.join("|")
    }).unwrap_or_default();
    need(wallet_keys == "1000|3000", "OPENING_WALLETS")?;
    for (latency, w) in opening["state"]["wallets"].as_object().cloned().unwrap_or_default() {
        let mut c = bank["config"].clone();
        c["latencyMs"] = json!(latency.parse::<i64>().unwrap_or(0));
        crate::model::assert_state(&w["state"], &c)?;
    }
    Ok(opening.clone())
}

/// receiptProbabilityGate(r, b, bank, availableAt)
pub fn receipt_probability_gate(r: Option<&V>, b: &V, bank: &V, available_at: &str) -> Result<V, String> {
    let reject = |reason: &str| Ok(json!({"accepted": false, "reason": reason}));
    let identity = &bank["identities"][b["marketId"].as_str().unwrap_or("")];
    let p = &bank["policy"];
    let now = instant_of(&json!(available_at));
    let Some(r) = r else { return reject("missing_research"); };
    crate::probability_paper::validate_research(r)?;
    if r["marketId"] != b["marketId"] || r["exchangeId"] != b["exchangeId"]
        || identity["exchangeId"] != b["exchangeId"] || identity["predicateHash"] != r["predicateHash"] {
        return reject("contract_mismatch");
    }
    if !r["settlementReviewed"].as_bool().unwrap_or(false)
        || r["flags"].as_array().is_some_and(|f| !f.is_empty()) {
        return reject("unresolved_evidence");
    }
    if instant_of(&r["recordedAt"]) > instant_of(&b["requestStartedAt"])
        || instant_of(&r["sourceAt"]) > instant_of(&b["requestStartedAt"]) {
        return reject("research_not_yet_known");
    }
    if now >= instant_of(&r["expiresAt"])
        || now - instant_of(&r["sourceAt"]) > p["maximumResearchAgeMs"].as_i64().unwrap_or(0) {
        return reject("expired_research");
    }
    if b["freshnessPolicy"].as_str() != Some("authoritative_receipt_v1")
        || now - instant_of(&b["receivedAt"]) > bank["config"]["maxDataAgeMs"].as_i64().unwrap_or(0)
        || instant_of(&b["receivedAt"]) - instant_of(&b["requestStartedAt"]) > bank["config"]["maxDataAgeMs"].as_i64().unwrap_or(0) {
        return reject("stale_receipt");
    }
    if big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))? - big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?
        > big_amount(p["maximumSpread"].as_str().unwrap_or(""))? {
        return reject("spread");
    }
    let entry = big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))? + BigInt::from(TICK);
    let edge = big_amount(r["lower"].as_str().unwrap_or(""))? - &entry;
    let admitted = edge >= big_amount(p["minimumLowerEdge"].as_str().unwrap_or(""))?;
    Ok(json!({"accepted": admitted,
        "reason": if admitted { "lower_bound_edge" } else { "insufficient_lower_edge" },
        "entry": money(&entry), "edge": money(&edge), "researchHash": digest(r)}))
}

/// createProbabilityReceiptState(bank, opening)
pub fn create_probability_receipt_state(bank: &V, opening: &V) -> Result<V, String> {
    validate_probability_receipt_bank(bank)?;
    validate_probability_opening(opening, bank)?;
    let mut state = opening["state"].clone();
    state["bankHash"] = json!(digest(bank));
    state["lastAvailableAt"] = bank["cutoff"].clone();
    state["barriers"] = json!({"global": bank["cutoff"], "markets": {}});
    let wallets: Vec<String> = state["wallets"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    for k in wallets {
        let mut w = state["wallets"][&k].clone();
        crate::model::invalidate(&mut w["state"], None, bank["cutoff"].as_str().unwrap_or(""), "owned_risk_continuation_boundary");
        if let Some(strategies) = w["state"]["strategies"].as_object_mut() {
            for st in strategies.values_mut() {
                let n = st["pending"].as_object().map(|p| p.len()).unwrap_or(0) as i64;
                st["expiredIntents"] = json!(st["expiredIntents"].as_i64().unwrap_or(0) + n);
                st["pending"] = json!({});
            }
        }
        state["wallets"][&k] = w;
    }
    Ok(state)
}

fn fence(s: &mut V, id: Option<&str>, at_str: &str, reason: &str) {
    match id {
        None => s["barriers"]["global"] = json!(at_str),
        Some(id) => s["barriers"]["markets"][id] = json!(at_str),
    }
    let keys: Vec<String> = s["wallets"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    for k in keys {
        let mut w = s["wallets"][&k].clone();
        crate::model::invalidate(&mut w["state"], id, at_str, reason);
        let pending = w["state"]["strategies"]["mean_reversion"]["pending"].as_object().cloned().unwrap_or_default();
        for (key, intent) in pending {
            if intent["action"].as_str() == Some("buy")
                && (id.is_none() || w["state"]["books"][&key]["marketId"].as_str() == id) {
                w["state"]["strategies"]["mean_reversion"]["pending"].as_object_mut().map(|m| m.remove(&key));
                let st = &mut w["state"]["strategies"]["mean_reversion"];
                st["expiredIntents"] = json!(st["expiredIntents"].as_i64().unwrap_or(0) + 1);
            }
        }
        s["wallets"][&k] = w;
    }
}

/// advanceReceiptProbability(s, row, bank, availableAt, account) — JS mutates s; Rust takes &mut.
pub fn advance_receipt_probability(s: &mut V, row: &V, bank: &V, available_at: &str, account: Option<&V>) -> Result<(), String> {
    need(s["bankHash"].as_str() == Some(digest(bank).as_str())
        && instant_of(&row["at"]) <= instant_of(&json!(available_at))
        && instant_of(&json!(available_at)) >= instant_of(&s["lastAvailableAt"]), "CAUSAL_STATE")?;
    s["lastAvailableAt"] = json!(available_at);
    let mut inc = |s: &mut V, k: &str| {
        s["counts"][k] = json!(s["counts"][k].as_i64().unwrap_or(0) + 1);
    };
    if ["session_start", "session_stop", "invalidation"].contains(&row["kind"].as_str().unwrap_or("")) {
        let id = if row["kind"].as_str() == Some("invalidation") {
            row["payload"]["marketId"].as_str().map(String::from)
        } else { None };
        fence(s, id.as_deref(), available_at, "probability_source_boundary");
        inc(s, "source_boundary");
        return Ok(());
    }
    need(row["kind"].as_str() == Some("book"), "ROW_KIND")?;
    let b = &row["payload"]["book"];
    if bank["identities"][b["marketId"].as_str().unwrap_or("")].is_null() { return Ok(()); }
    if instant_of(&json!(available_at)) - instant_of(&b["receivedAt"]) > bank["config"]["maxDataAgeMs"].as_i64().unwrap_or(0) {
        let id = b["marketId"].as_str().unwrap_or("").to_string();
        fence(s, Some(&id), available_at, "stale_consumer_book");
        inc(s, "stale_consumer_book");
        return Ok(());
    }
    if [s["barriers"]["global"].clone(), s["barriers"]["markets"][b["marketId"].as_str().unwrap_or("")].clone(), bank["cutoff"].clone()]
        .iter().any(|at| !at.is_null() && instant_of(&b["requestStartedAt"]) <= instant_of(at)) {
        inc(s, "request_before_consumer_barrier");
        return Ok(());
    }
    let account_reason = probability_account_gate(account, b, available_at, Some(&bank["policy"]), "reduce_only");
    if !account_reason.is_null() {
        let reason = account_reason.as_str().unwrap_or("").to_string();
        let id = b["marketId"].as_str().unwrap_or("").to_string();
        fence(s, Some(&id), available_at, &reason);
        inc(s, &reason);
        return Ok(());
    }
    let r = bank["research"].as_array().and_then(|rs| rs.iter().find(|x| x["marketId"] == b["marketId"])).cloned();
    let research_gate = receipt_probability_gate(r.as_ref(), b, bank, available_at)?;
    let own_position = account.map(|a| a["positionExchangeIds"].as_array()
        .is_some_and(|v| v.contains(&b["exchangeId"]))).unwrap_or(false);
    let gate = if own_position {
        let mut g = research_gate.clone();
        g["accepted"] = json!(false);
        g["reason"] = json!("own_account_exposure_reduce_only");
        g
    } else { research_gate };
    let reason = gate["reason"].as_str().unwrap_or("").to_string();
    inc(s, &reason);
    if own_position {
        let keys: Vec<String> = s["wallets"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
        for k in keys {
            let mut w = s["wallets"][&k].clone();
            let instrument = b["instrument"].as_str().unwrap_or("").to_string();
            if w["state"]["strategies"]["mean_reversion"]["pending"][&instrument]["action"].as_str() == Some("buy") {
                w["state"]["strategies"]["mean_reversion"]["pending"].as_object_mut().map(|m| m.remove(&instrument));
                let st = &mut w["state"]["strategies"]["mean_reversion"];
                st["expiredIntents"] = json!(st["expiredIntents"].as_i64().unwrap_or(0) + 1);
            }
            s["wallets"][&k] = w;
        }
    }
    let mut decision = json!({"at": available_at, "marketId": b["marketId"], "sourceSequence": row["sequence"],
        "sourceHash": row["hash"], "requestStartedAt": b["requestStartedAt"], "receivedAt": b["receivedAt"]});
    for (k, v) in gate.as_object().unwrap() { decision[k] = v.clone(); }
    let mut decisions = s["decisions"].as_array().cloned().unwrap_or_default();
    decisions.push(decision);
    if decisions.len() > 1000 { decisions.remove(0); }
    s["decisions"] = json!(decisions);
    let latency_keys: Vec<String> = s["wallets"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    let instrument = b["instrument"].as_str().unwrap_or("").to_string();
    let market_id = b["marketId"].as_str().unwrap_or("").to_string();
    for latency in latency_keys {
        let mut c = bank["config"].clone();
        c["latencyMs"] = json!(latency.parse::<i64>().unwrap_or(0));
        let mut w = s["wallets"][&latency].clone();
        let st = w["state"]["strategies"]["mean_reversion"].clone();
        let spent: BigInt = st["fills"].as_array().cloned().unwrap_or_default().iter()
            .filter(|x| x["action"].as_str() == Some("buy"))
            .map(|f| big_amount(f["price"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0)) * BigInt::from(f["quantity"].as_i64().unwrap_or(0)))
            .sum();
        let reserved: BigInt = st["pending"].as_object().cloned().unwrap_or_default().values()
            .filter(|x| x["action"].as_str() == Some("buy"))
            .map(|i| big_amount(i["limit"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0)) * BigInt::from(i["quantity"].as_i64().unwrap_or(0)))
            .sum();
        let mut occupied: BTreeSet<String> = st["positions"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
        occupied.extend(st["pending"].as_object().map(|m| m.keys().cloned().collect::<Vec<String>>()).unwrap_or_default());
        let old = st["pending"][&instrument].clone();
        let allow = gate["accepted"].as_bool() == Some(true)
            && &spent + &reserved + (if old["action"].as_str() == Some("buy") { BigInt::from(0) }
                else { big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))? + BigInt::from(TICK) })
                <= big_amount(bank["policy"]["cumulativeBuyBudget"].as_str().unwrap_or(""))?
            && (occupied.contains(&instrument) || occupied.len() < bank["policy"]["maxOpenMarkets"].as_i64().unwrap_or(0) as usize)
            && instant_of(&json!(available_at)) >= w["cooldowns"][&market_id].as_i64().unwrap_or(0);
        let reference = if gate["accepted"].as_bool() == Some(true) {
            Some(big_amount(r.as_ref().unwrap()["lower"].as_str().unwrap_or(""))?
                .to_string().parse::<f64>().unwrap_or(0.0) / 1e6)
        } else { None };
        w["state"] = crate::model::step_dip(&w["state"], b, &c, reference, allow,
            big_amount(bank["policy"]["minimumLowerEdge"].as_str().unwrap_or(""))?
                .to_string().parse::<f64>().unwrap_or(0.0) / 1e6 - 4.0 * f64::EPSILON,
            &bank["policy"]["takeProfit"], bank["policy"]["maxHoldMs"].as_i64().unwrap_or(0),
            &bank["policy"]["maxMarketCost"], "responsive_v2")?;
        let next = w["state"]["strategies"]["mean_reversion"].clone();
        let pending = next["pending"][&instrument].clone();
        if !pending.is_null()
            && (old.is_null() || old["id"] != pending["id"] || old["createdAt"] != pending["createdAt"]) {
            w["state"]["strategies"]["mean_reversion"]["pending"][&instrument]["createdAt"] = json!(available_at);
            w["state"]["strategies"]["mean_reversion"]["pending"][&instrument]["actualAvailableAt"] = json!(available_at);
            w["state"]["strategies"]["mean_reversion"]["pending"][&instrument]["probabilityResearchHash"] =
                json!(digest(r.as_ref().unwrap()));
            if pending["action"].as_str() == Some("buy") {
                w["state"]["strategies"]["mean_reversion"]["pending"][&instrument]["reason"] =
                    json!("source_probability_lower_bound");
            }
        }
        if !st["positions"][&instrument].is_null() && next["positions"][&instrument].is_null() {
            w["cooldowns"][&market_id] = json!(instant_of(&json!(available_at)) + 900000);
        }
        let new_spent: BigInt = next["fills"].as_array().cloned().unwrap_or_default().iter()
            .filter(|x| x["action"].as_str() == Some("buy"))
            .map(|f| big_amount(f["price"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0)) * BigInt::from(f["quantity"].as_i64().unwrap_or(0)))
            .sum();
        need(new_spent <= big_amount(bank["policy"]["cumulativeBuyBudget"].as_str().unwrap_or(""))?, "CUMULATIVE_BUDGET")?;
        s["wallets"][&latency] = w;
    }
    Ok(())
}

/// receiptProbabilityReport(s, bank, at)
pub fn receipt_probability_report(s: &V, bank: &V, at: &str) -> V {
    let mut wallets = serde_json::Map::new();
    for (latency, w) in s["wallets"].as_object().cloned().unwrap_or_default() {
        let mut c = bank["config"].clone();
        c["latencyMs"] = json!(latency.parse::<i64>().unwrap_or(0));
        let mut r = crate::model::report(&w["state"], &c, at)["strategies"]["mean_reversion"].clone();
        let label_of = |instrument: &str| -> V {
            let spec = bank["universe"]["exchanges"].as_array().map(|a| a.as_slice()).unwrap_or(&[]).iter()
                .find(|y| y["exchangeId"].as_str().map(|e| format!("{}:YES", e)) == Some(instrument.to_string()));
            json!(format!("Market {} · YES", spec.map(|s| s["marketId"].as_str().unwrap_or("undefined")).unwrap_or("undefined")))
        };
        let positions: Vec<V> = r["positions"].as_array().cloned().unwrap_or_default().into_iter()
            .map(|mut x| {
                x["label"] = label_of(x["instrument"].as_str().unwrap_or(""));
                x
            }).collect();
        r["positions"] = json!(positions);
        let pending: Vec<V> = w["state"]["strategies"]["mean_reversion"]["pending"].as_object().cloned()
            .unwrap_or_default().into_iter()
            .map(|(instrument, mut x)| {
                x["instrument"] = json!(instrument);
                let spec = bank["universe"]["exchanges"].as_array().map(|a| a.as_slice()).unwrap_or(&[]).iter()
                    .find(|y| y["exchangeId"].as_str().map(|e| format!("{}:YES", e)) == Some(instrument.clone()));
                x["marketId"] = spec.map(|s| s["marketId"].clone()).unwrap_or(V::Null);
                x
            }).collect();
        r["pending"] = json!(pending);
        wallets.insert(latency, r);
    }
    json!({"mode": bank["mode"], "updatedAt": at, "bankHash": s["bankHash"], "cutoff": bank["cutoff"],
        "accountingContinuation": bank["accountingContinuation"], "counts": s["counts"],
        "decisions": s["decisions"],
        "actualConsumer": {"lastAvailableAt": s["lastAvailableAt"], "barriers": s["barriers"]},
        "wallets": wallets, "networkRequests": 0, "competitionOrders": 0,
        "profitabilityEstablished": false,
        "limitations": [
            "Forecast bands and settlement review are supplied research attestations, not authenticated or calibrated probability guarantees.",
            "Settlement probability minus purchase cost does not forecast a profitable sale within the two-day holding window.",
            "Counterfactual later-request fills use one-percent depth, one adverse tick and zero fees; latencies share correlated opportunities.",
            "Fresh causal positions block new buys; fresh complete open orders block all simulated fills on our own liquidity.",
            "Official account positions may already influence public prices. Reduce-only paper exits are counterfactual and omit market impact.",
            "The original wallet cash, basis and fills are carried forward; the continuation cutoff does not reset performance."]})
}
