// live-paper/probability-receipt.mjs — actual-receipt probability paper adapter.
use baal_paper::fixed::{big_amount, money, TICK};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

pub fn probability_receipt_policy() -> V {
    json!({"minimumLowerEdge": "0.020000", "maximumSpread": "0.020000", "maximumResearchAgeMs": 21600000,
        "maximumAccountAgeMs": 20000, "maxHoldMs": 172800000, "takeProfit": "0.020000",
        "maxMarketCost": "1.000000", "maxOpenMarkets": 3, "cumulativeBuyBudget": "100.000000",
        "latenciesMs": [1000, 3000]})
}
fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("PROBABILITY_RECEIPT_{}", code)); } Ok(()) }
fn id(v: &V) -> bool { v.as_str().is_some_and(|s| !s.is_empty() && s.as_bytes()[0] != b'0' && s.bytes().all(|b| b.is_ascii_digit()) && s.parse::<i64>().is_ok()) }
fn instant(v: &V) -> i64 { crate::model::instant(v.as_str().unwrap_or("")).unwrap_or(i64::MIN) }
fn digest(v: &V) -> String { baal_service_runtime::json_text::digest(v) }
/// validateProbabilityReceiptBank(bank)
pub fn validate_probability_receipt_bank(bank: &V) -> Result<(), String> {
    need(bank["schemaVersion"].as_i64() == Some(2)
        && bank["mode"].as_str() == Some("source_probability_actual_receipt_paper")
        && digest(&bank["policy"]) == digest(&probability_receipt_policy()), "BANK")?;
    crate::model::validate_config(&bank["config"]).map_err(|_| "PROBABILITY_RECEIPT_BANK".to_string())?;
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
        crate::probability_paper::validate_research(r).map_err(|_| "PROBABILITY_RECEIPT_RESEARCH_BINDING".to_string())?;
        let spec = &bank["identities"][r["marketId"].as_str().unwrap_or("")];
        need(!seen.contains(r["marketId"].as_str().unwrap())
            && instant(&r["recordedAt"]) <= instant(&bank["cutoff"])
            && spec["exchangeId"] == r["exchangeId"] && spec["predicateHash"] == r["predicateHash"]
            && bank["universe"]["exchanges"].as_array().is_some_and(|xs| xs.iter().any(|x| x["marketId"] == r["marketId"] && x["exchangeId"] == r["exchangeId"] && x["option"].as_str() == Some("YES")))
            && c["marketIds"].as_array().unwrap().contains(&r["marketId"]), "RESEARCH_BINDING")?;
        seen.insert(r["marketId"].as_str().unwrap().to_string());
    }
    Ok(())
}
/// probabilityAccountEligibility(cache) — null on any failure.
pub fn probability_account_eligibility(cache: &V) -> Option<V> {
    (|| -> Result<V, String> {
        need(cache["errors"]["positions"].is_null() && cache["errors"]["orders"].is_null()
            && cache["positions"]["rows"].is_array() && cache["orders"]["rows"].is_array(), "ACCOUNT_UNAVAILABLE")?;
        instant(&cache["positions"]["at"]); instant(&cache["orders"]["at"]);
        let mut excluded = BTreeSet::new();
        for p in cache["positions"]["rows"].as_array().unwrap() {
            need(id(&p["exchangeId"]) && p["quantity"].as_i64().is_some_and(|q| q > 0), "ACCOUNT_POSITION")?;
            excluded.insert(p["exchangeId"].as_str().unwrap().to_string());
        }
        for o in cache["orders"]["rows"].as_array().unwrap() {
            need(id(&o["exchangeId"]) && o["open"] == V::Bool(true) && o["quantity"].as_f64().is_some_and(|q| q > 0.0), "ACCOUNT_ORDER")?;
            excluded.insert(o["exchangeId"].as_str().unwrap().to_string());
        }
        let mut v: Vec<String> = excluded.into_iter().collect();
        v.sort_by_key(|x| x.parse::<i64>().unwrap_or(0));
        Ok(json!({"schemaVersion": 1, "positionsAt": cache["positions"]["at"], "ordersAt": cache["orders"]["at"],
            "excludedExchangeIds": v}))
    })().ok()
}
/// probabilityAccountGate(account, b, availableAt, policy)
pub fn probability_account_gate(account: &V, b: &V, available_at: &str) -> Option<String> {
    (|| -> Result<Option<String>, String> {
        let keys: String = account.as_object().map(|m| { let mut k: Vec<_> = m.keys().cloned().collect(); k.sort(); k }).unwrap_or_default().join("|");
        if account.is_null() || account["schemaVersion"].as_i64() != Some(1)
            || keys != "excludedExchangeIds|ordersAt|positionsAt|schemaVersion"
            || !account["excludedExchangeIds"].as_array().is_some_and(|a| a.iter().all(id)) {
            return Ok(Some("account_unavailable".into()));
        }
        let now = instant(&json!(available_at));
        let request = instant(&b["requestStartedAt"]);
        if [account["positionsAt"].clone(), account["ordersAt"].clone()].iter().any(|t| instant(t) > request || now - instant(t) > 20000) {
            return Ok(Some("account_not_causal_or_fresh".into()));
        }
        Ok(if account["excludedExchangeIds"].as_array().unwrap().iter().any(|x| x.as_str() == b["exchangeId"].as_str()) {
            Some("own_account_exposure".into())
        } else { None })
    })().unwrap_or(Some("account_unavailable".into()))
}
/// receiptProbabilityGate(r, b, bank, availableAt)
pub fn receipt_probability_gate(r: Option<&V>, b: &V, bank: &V, available_at: &str) -> V {
    let identity = &bank["identities"][b["marketId"].as_str().unwrap_or("")];
    let p = &bank["policy"];
    let now = instant(&json!(available_at));
    let Some(r) = r else { return json!({"accepted": false, "reason": "missing_research"}); };
    if crate::probability_paper::validate_research(r).is_err() {
        return json!({"accepted": false, "reason": "missing_research"});
    }
    let g = |reason: &str| json!({"accepted": false, "reason": reason});
    if r["marketId"] != b["marketId"] || r["exchangeId"] != b["exchangeId"]
        || identity["exchangeId"] != b["exchangeId"] || identity["predicateHash"] != r["predicateHash"] {
        return g("contract_mismatch");
    }
    if !r["settlementReviewed"].as_bool().unwrap_or(false) || !r["flags"].as_array().map(|f| f.is_empty()).unwrap_or(true) {
        return g("unresolved_evidence");
    }
    if instant(&r["recordedAt"]) > instant(&b["requestStartedAt"]) || instant(&r["sourceAt"]) > instant(&b["requestStartedAt"]) {
        return g("research_not_yet_known");
    }
    if now >= instant(&r["expiresAt"]) || now - instant(&r["sourceAt"]) > p["maximumResearchAgeMs"].as_i64().unwrap_or(0) {
        return g("expired_research");
    }
    if b["freshnessPolicy"].as_str() != Some("authoritative_receipt_v1")
        || now - instant(&b["receivedAt"]) > bank["config"]["maxDataAgeMs"].as_i64().unwrap_or(0)
        || instant(&b["receivedAt"]) - instant(&b["requestStartedAt"]) > bank["config"]["maxDataAgeMs"].as_i64().unwrap_or(0) {
        return g("stale_receipt");
    }
    let ask = big_amount(b["asks"][0]["price"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0));
    let bid = big_amount(b["bids"][0]["price"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0));
    if &ask - &bid > big_amount(p["maximumSpread"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0)) {
        return g("spread");
    }
    let entry = ask + BigInt::from(TICK);
    let edge = big_amount(r["lower"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0)) - &entry;
    let min = big_amount(p["minimumLowerEdge"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0));
    json!({"accepted": edge >= min, "reason": if edge >= min { "lower_bound_edge" } else { "insufficient_lower_edge" },
        "entry": money(&entry), "edge": money(&edge), "researchHash": digest(r)})
}
/// createProbabilityReceiptState(bank)
pub fn create_probability_receipt_state(bank: &V) -> Result<V, String> {
    validate_probability_receipt_bank(bank)?;
    let mut wallets = serde_json::Map::new();
    for latency in bank["policy"]["latenciesMs"].as_array().unwrap() {
        let mut c = bank["config"].clone();
        c["latencyMs"] = latency.clone();
        let name = format!("{}:{}", bank["name"].as_str().unwrap_or(""), latency.as_i64().unwrap());
        let st = crate::model::initial_state(&c, &name, bank["cutoff"].as_str().unwrap())?;
        wallets.insert(latency.as_i64().unwrap().to_string(), json!({"state": st, "cooldowns": {}}));
    }
    Ok(json!({"bankHash": digest(bank), "lastAvailableAt": bank["cutoff"],
        "barriers": {"global": bank["cutoff"], "markets": {}}, "counts": {}, "decisions": [], "wallets": wallets}))
}
fn fence(s: &mut V, id: Option<&str>, at: &str, reason: &str) {
    match id {
        None => s["barriers"]["global"] = json!(at),
        Some(m) => s["barriers"]["markets"][m] = json!(at),
    }
    let wallet_keys: Vec<String> = s["wallets"].as_object().map(|w| w.keys().cloned().collect()).unwrap_or_default();
    for k in wallet_keys {
        let mut w = s["wallets"][&k].clone();
        crate::model::invalidate(&mut w["state"], id, at, reason);
        let pending = w["state"]["strategies"]["mean_reversion"]["pending"].as_object().cloned().unwrap_or_default();
        for (key, intent) in pending {
            let matches = id.is_none() || w["state"]["books"][&key]["marketId"].as_str() == id;
            if intent["action"].as_str() == Some("buy") && matches {
                w["state"]["strategies"]["mean_reversion"]["pending"].as_object_mut().unwrap().remove(&key);
                w["state"]["strategies"]["mean_reversion"]["expiredIntents"] =
                    json!(w["state"]["strategies"]["mean_reversion"]["expiredIntents"].as_i64().unwrap_or(0) + 1);
            }
        }
        s["wallets"][&k] = w;
    }
}
/// advanceReceiptProbability(s, row, bank, availableAt, account)
pub fn advance_receipt_probability(s: &V, row: &V, bank: &V, available_at: &str, account: &V) -> Result<V, String> {
    need(s["bankHash"].as_str() == Some(digest(bank).as_str())
        && instant(&row["at"]) <= instant(&json!(available_at))
        && instant(&json!(available_at)) >= instant(&s["lastAvailableAt"]), "CAUSAL_STATE")?;
    let mut s = s.clone();
    s["lastAvailableAt"] = json!(available_at);
    macro_rules! inc {
        ($k:expr) => { s["counts"][$k] = json!(s["counts"][$k].as_i64().unwrap_or(0) + 1); };
    }
    if ["session_start", "session_stop", "invalidation"].contains(&row["kind"].as_str().unwrap_or("")) {
        let mid = if row["kind"].as_str() == Some("invalidation") { row["payload"]["marketId"].as_str() } else { None };
        fence(&mut s, mid, available_at, "probability_source_boundary");
        inc!("source_boundary");
        return Ok(s);
    }
    need(row["kind"].as_str() == Some("book"), "ROW_KIND")?;
    let b = &row["payload"]["book"];
    if bank["identities"][b["marketId"].as_str().unwrap_or("")].is_null() { return Ok(s); }
    if instant(&json!(available_at)) - instant(&b["receivedAt"]) > bank["config"]["maxDataAgeMs"].as_i64().unwrap_or(0) {
        fence(&mut s, b["marketId"].as_str(), available_at, "stale_consumer_book");
        inc!("stale_consumer_book");
        return Ok(s);
    }
    let barriers = [s["barriers"]["global"].clone(), s["barriers"]["markets"][b["marketId"].as_str().unwrap_or("")].clone(), bank["cutoff"].clone()];
    if barriers.iter().any(|at| !at.is_null() && instant(&b["requestStartedAt"]) <= instant(at)) {
        inc!("request_before_consumer_barrier");
        return Ok(s);
    }
    if let Some(reason) = probability_account_gate(account, b, available_at) {
        fence(&mut s, b["marketId"].as_str(), available_at, &reason);
        inc!(&reason);
        return Ok(s);
    }
    let r = bank["research"].as_array().unwrap().iter().find(|x| x["marketId"] == b["marketId"]).cloned();
    let gate = receipt_probability_gate(r.as_ref(), b, bank, available_at);
    inc!(gate["reason"].as_str().unwrap_or(""));
    let mut decision = json!({"at": available_at, "marketId": b["marketId"], "sourceSequence": row["sequence"],
        "sourceHash": row["hash"], "requestStartedAt": b["requestStartedAt"], "receivedAt": b["receivedAt"]});
    for (k, v) in gate.as_object().unwrap() { decision[k] = v.clone(); }
    s["decisions"].as_array_mut().unwrap().push(decision);
    if s["decisions"].as_array().unwrap().len() > 1000 { s["decisions"].as_array_mut().unwrap().remove(0); }
    let latency_keys: Vec<String> = s["wallets"].as_object().map(|w| w.keys().cloned().collect()).unwrap_or_default();
    for latency in latency_keys {
        let latency_i: i64 = latency.parse().unwrap_or(0);
        let mut c = bank["config"].clone();
        c["latencyMs"] = json!(latency_i);
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
        let mut occupied: BTreeSet<String> = st["positions"].as_object().map(|p| p.keys().cloned().collect()).unwrap_or_default();
        occupied.extend(st["pending"].as_object().map(|p| p.keys().cloned().collect::<Vec<String>>()).unwrap_or_default());
        let old = st["pending"][b["instrument"].as_str().unwrap_or("")].clone();
        let instrument = b["instrument"].as_str().unwrap_or("").to_string();
        let ask = big_amount(b["asks"][0]["price"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0)) + BigInt::from(TICK);
        let cooldown_ok = instant(&json!(available_at)) >= w["cooldowns"][b["marketId"].as_str().unwrap_or("")].as_i64().unwrap_or(0);
        let allow = gate["accepted"].as_bool().unwrap_or(false)
            && &spent + &reserved + &(if old["action"].as_str() == Some("buy") { BigInt::from(0) } else { ask.clone() })
                <= big_amount(bank["policy"]["cumulativeBuyBudget"].as_str().unwrap_or(""))?
            && (occupied.contains(&instrument) || occupied.len() < bank["policy"]["maxOpenMarkets"].as_i64().unwrap_or(0) as usize)
            && cooldown_ok;
        let reference = if gate["accepted"].as_bool().unwrap_or(false) {
            Some(big_amount(r.as_ref().unwrap()["lower"].as_str().unwrap_or(""))?.to_string().parse::<f64>().unwrap_or(0.0) / 1e6)
        } else { None };
        w["state"] = crate::model::step_dip(&w["state"], b, &c, reference, allow,
            big_amount(bank["policy"]["minimumLowerEdge"].as_str().unwrap_or(""))?.to_string().parse::<f64>().unwrap_or(0.0) / 1e6 - 4.0 * f64::EPSILON,
            &bank["policy"]["takeProfit"], bank["policy"]["maxHoldMs"].as_i64().unwrap(),
            &bank["policy"]["maxMarketCost"], "responsive_v2")?;
        let next = w["state"]["strategies"]["mean_reversion"].clone();
        let pending = next["pending"][&instrument].clone();
        if !pending.is_null() && (old.is_null() || old["id"] != pending["id"] || old["createdAt"] != pending["createdAt"]) {
            w["state"]["strategies"]["mean_reversion"]["pending"][&instrument]["createdAt"] = json!(available_at);
            w["state"]["strategies"]["mean_reversion"]["pending"][&instrument]["actualAvailableAt"] = json!(available_at);
            w["state"]["strategies"]["mean_reversion"]["pending"][&instrument]["probabilityResearchHash"] = json!(digest(r.as_ref().unwrap()));
            if pending["action"].as_str() == Some("buy") {
                w["state"]["strategies"]["mean_reversion"]["pending"][&instrument]["reason"] = json!("source_probability_lower_bound");
            }
        }
        if !st["positions"][&instrument].is_null() && next["positions"][&instrument].is_null() {
            w["cooldowns"][b["marketId"].as_str().unwrap_or("")] = json!(instant(&json!(available_at)) + 900_000);
        }
        let new_spent: BigInt = next["fills"].as_array().cloned().unwrap_or_default().iter()
            .filter(|x| x["action"].as_str() == Some("buy"))
            .map(|f| big_amount(f["price"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0)) * BigInt::from(f["quantity"].as_i64().unwrap_or(0)))
            .sum();
        need(new_spent <= big_amount(bank["policy"]["cumulativeBuyBudget"].as_str().unwrap_or(""))?, "CUMULATIVE_BUDGET")?;
        s["wallets"][&latency] = w;
    }
    Ok(s)
}
/// receiptProbabilityReport(s, bank, at)
pub fn receipt_probability_report(s: &V, bank: &V, at: &str) -> V {
    let mut wallets = serde_json::Map::new();
    for (latency, w) in s["wallets"].as_object().cloned().unwrap_or_default() {
        let mut c = bank["config"].clone();
        c["latencyMs"] = json!(latency.parse::<i64>().unwrap_or(0));
        let r = crate::model::report(&w["state"], &c, at)["strategies"]["mean_reversion"].clone();
        let mut rr = r.clone();
        let mut positions = Vec::new();
        for x in r["positions"].as_array().cloned().unwrap_or_default() {
            let mut x = x;
            let market = bank["universe"]["exchanges"].as_array().unwrap().iter()
                .find(|y| format!("{}:YES", y["exchangeId"].as_str().unwrap_or("")) == x["instrument"].as_str().unwrap_or(""));
            x["label"] = json!(format!("Market {} · YES", market.map(|m| m["marketId"].as_str().unwrap_or("")).unwrap_or("")));
            positions.push(x);
        }
        rr["positions"] = json!(positions);
        rr["pending"] = json!(r["pending"].as_array().cloned().unwrap_or_default().iter().map(|x| {
            let mut x = x.clone();
            let market = bank["universe"]["exchanges"].as_array().unwrap().iter()
                .find(|y| format!("{}:YES", y["exchangeId"].as_str().unwrap_or("")) == x["instrument"].as_str().unwrap_or(""));
            x["marketId"] = market.map(|m| m["marketId"].clone()).unwrap_or(V::Null);
            x
        }).collect::<Vec<_>>());
        wallets.insert(latency, rr);
    }
    json!({"mode": bank["mode"], "updatedAt": at, "bankHash": s["bankHash"], "cutoff": bank["cutoff"],
        "counts": s["counts"], "decisions": s["decisions"],
        "actualConsumer": {"lastAvailableAt": s["lastAvailableAt"], "barriers": s["barriers"]},
        "wallets": wallets, "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false,
        "limitations": ["Forecast bands and settlement review are supplied research attestations, not authenticated or calibrated probability guarantees.",
            "Settlement probability minus purchase cost does not forecast a profitable sale within the research horizon."]})
}
