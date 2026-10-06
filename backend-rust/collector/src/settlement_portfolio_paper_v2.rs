// live-paper/settlement-portfolio-paper-v2.mjs — multi-market portfolio reducer.
use baal_execution::core::payload_hash;
use indexmap::IndexMap;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("SETTLEMENT_PORTFOLIO_{}", code)); }
    Ok(())
}
fn digest(v: &V) -> String { crate::model::digest(v) }
fn instant(v: &V) -> Result<i64, String> { baal_paper::fixed::instant(v.as_str().unwrap_or("")) }
fn amount(v: &V) -> Result<BigInt, String> { baal_paper::fixed::big_amount(v.as_str().unwrap_or("")) }

/// PORTFOLIO_POLICY
pub fn portfolio_policy_const() -> V {
    json!({"minimumEdge": "0.020000", "maximumSpread": "0.020000", "maximumBookAgeMs": 20000,
        "maximumResearchAgeMs": 21600000, "fractionalKellyBps": 2500, "slippageTicks": 1,
        "depthFractionBps": 100, "maxOrderCost": "1.000000", "maxExposure": "3.000000",
        "cashReserve": "90.000000", "cumulativeBuyBudget": "100.000000"})
}
/// eventRiskGroup(root)
pub fn event_risk_group(root: &V) -> Result<String, String> {
    let p = &root["contract_details"];
    let keys = ["raceId", "stageId", "usState", "raceStage", "officeLevel", "electionDate", "resolutionType"];
    need(keys.iter().all(|k| p[k].as_str().is_some_and(|s| !s.is_empty())), "EVENT")?;
    let mut o = Map::new();
    for k in keys { o.insert(k.to_string(), p[k].clone()); }
    Ok(format!("race_{}", &payload_hash(&V::Object(o))?[..24]))
}
/// portfolioPolicy(researchEvidence)
pub fn portfolio_policy(research_evidence: &V) -> Result<V, String> {
    let mut out = portfolio_policy_const();
    let mut group_caps = Map::new();
    group_caps.insert("national_election".to_string(), json!("3.000000"));
    for e in research_evidence.as_array().cloned().unwrap_or_default() {
        group_caps.insert(event_risk_group(&e["root"])?, json!("1.000000"));
    }
    out["groupCaps"] = V::Object(group_caps);
    Ok(out)
}
/// validateSettlementPortfolioBank(bank)
pub fn validate_settlement_portfolio_bank(bank: &V) -> Result<(), String> {
    need(bank["schemaVersion"].as_i64() == Some(2)
        && bank["mode"].as_str() == Some("funded_settlement_portfolio_paper_v2")
        && bank["research"].as_array().is_some_and(|a| !a.is_empty() && a.len() <= 3), "BANK")?;
    need(bank["researchEvidence"].as_array().map(|a| a.len())
        == bank["research"].as_array().map(|a| a.len())
        && bank["identities"].as_object().map(|m| m.len())
            == bank["research"].as_array().map(|a| a.len()), "SCOPE")?;
    need(digest(&bank["policy"]) == digest(&portfolio_policy(&bank["researchEvidence"])?)
        && digest(&bank["fillPolicy"]) == digest(&crate::settlement_paper::settlement_paper_fills()), "POLICY")?;
    instant(&bank["cutoff"])?;
    let mut seen = std::collections::HashSet::new();
    let mut probabilities: IndexMap<String, IndexMap<String, (BigInt, BigInt)>> = IndexMap::new();
    for r in bank["research"].as_array().cloned().unwrap_or_default() {
        crate::probability_paper::validate_research(&r).map_err(|e| e.to_string())?;
        let e = bank["researchEvidence"].as_array().cloned().unwrap_or_default().into_iter()
            .find(|e| e["researchId"] == r["id"] && e["marketId"] == r["marketId"]);
        let root = e.as_ref().map(|e| e["root"].clone()).unwrap_or(V::Null);
        let id = &bank["identities"][r["marketId"].as_str().unwrap_or("")];
        need(!seen.contains(r["marketId"].as_str().unwrap_or(""))
            && r["side"].as_str() == Some("YES")
            && instant(&r["recordedAt"])? <= instant(&bank["cutoff"])?
            && id["exchangeId"] == r["exchangeId"] && id["predicateHash"] == r["predicateHash"]
            && bank["universe"]["exchanges"].as_array().is_some_and(|x| x.iter().any(|x|
                x["marketId"] == r["marketId"] && x["exchangeId"] == r["exchangeId"]
                    && x["option"].as_str() == Some("YES"))), "RESEARCH_BINDING")?;
        seen.insert(r["marketId"].as_str().unwrap_or("").to_string());
        need(root["node_type"].as_str() == Some("contract")
            && root["contract_type"].as_str() == Some("Election Outcome")
            && root["settled_with"].is_null()
            && root["contract_details"]["raceStage"].as_str() == Some("General")
            && root["contract_details"]["resolutionType"].as_str() == Some("Party Winner")
            && payload_hash(&root).ok().as_deref() == r["predicateHash"].as_str()
            && e.as_ref().map(|e| &e["rootHash"]) == Some(&r["predicateHash"]), "EXACT_ROOT")?;
        need(root["contract_id"] == root["node_id"]
            && root["contract_id"].as_str().is_some_and(|s| {
                let b = s.as_bytes();
                !b.is_empty() && b[0] != b'0' && b.iter().all(|c| c.is_ascii_digit())
            })
            && instant(&root["settlement_date"])? == bank["valuationDeadlineMs"].as_i64().unwrap_or(0)
            && bank["valuationDeadlineMs"].as_i64().unwrap_or(0) > instant(&bank["cutoff"])?, "RESOLUTION_HORIZON")?;
        need(digest(&bank["riskGroups"][r["marketId"].as_str().unwrap_or("")])
            == digest(&json!(["national_election", event_risk_group(&root)?])), "EVENT_GROUP_BINDING")?;
        let group = event_risk_group(&root)?;
        let winner = root["contract_details"]["winnerName"].as_str().unwrap_or("").to_string();
        need(!winner.is_empty(), "WINNER")?;
        let winners = probabilities.entry(group).or_default();
        let lower = amount(&r["lower"])?;
        let upper = amount(&r["upper"])?;
        let prior = winners.get(&winner).cloned();
        winners.insert(winner, (
            prior.as_ref().map(|(l, _)| if *l > lower { l.clone() } else { lower.clone() }).unwrap_or_else(|| lower.clone()),
            prior.map(|(_, u)| if u < upper { u } else { upper.clone() }).unwrap_or(upper),
        ));
    }
    for winners in probabilities.values() {
        need(winners.values().all(|(l, u)| l <= u), "CONFLICTING_DUPLICATE_VALUE")?;
        need(winners.values().fold(BigInt::from(0), |n, (l, _)| n + l) <= BigInt::from(1_000_000i64),
            "INCOHERENT_EVENT_PROBABILITY")?;
    }
    Ok(())
}
/// createSettlementPortfolioState(bank)
pub fn create_settlement_portfolio_state(bank: &V) -> Result<V, String> {
    validate_settlement_portfolio_bank(bank)?;
    let mut barriers = Map::new();
    barriers.insert("global".to_string(), bank["cutoff"].clone());
    barriers.insert("markets".to_string(), json!({}));
    let mut wallets = Map::new();
    for ms in bank["fillPolicy"]["latenciesMs"].as_array().cloned().unwrap_or_default() {
        let key = ms.as_i64().map(|i| i.to_string()).unwrap_or_else(|| ms.to_string());
        wallets.insert(key, json!({"cash": bank["fillPolicy"]["initialCash"],
            "realizedPnl": "0.000000", "cumulativeBuys": "0.000000", "positions": {},
            "pending": {}, "fills": [], "consumedVersions": {}, "lastExitedAtMs": {}, "intents": 0}));
    }
    Ok(json!({"bankHash": digest(bank), "lastAvailableAt": bank["cutoff"], "barriers": barriers,
        "counts": {}, "decisions": [], "books": {}, "wallets": wallets}))
}
/// assertSettlementPortfolioState(state, bank)
pub fn assert_settlement_portfolio_state(state: &V, bank: &V) -> Result<(), String> {
    validate_settlement_portfolio_bank(bank)?;
    let mut keys: Vec<String> = state["wallets"].as_object().map(|w| w.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    let mut latencies: Vec<String> = bank["fillPolicy"]["latenciesMs"].as_array().cloned()
        .unwrap_or_default().iter().map(|ms| ms.as_i64().map(|i| i.to_string()).unwrap_or_else(|| ms.to_string())).collect();
    latencies.sort();
    need(digest(&V::Array(keys.into_iter().map(V::from).collect())) == digest(&V::Array(latencies.into_iter().map(V::from).collect())),
        "WALLET_SCOPE")?;
    crate::settlement_paper::assert_settlement_paper_state(state, bank)?;
    for (_l, w) in state["wallets"].as_object().cloned().unwrap_or_default() {
        for f in w["fills"].as_array().cloned().unwrap_or_default() {
            need(bank["identities"][f["marketId"].as_str().unwrap_or("")]["exchangeId"] == f["exchangeId"]
                && f["instrument"].as_str() == Some(format!("{}:YES", f["exchangeId"].as_str().unwrap_or("")).as_str()),
                "FILL_SCOPE")?;
        }
        let mut used: IndexMap<String, BigInt> = IndexMap::new();
        for (id, position) in w["positions"].as_object().cloned().unwrap_or_default() {
            need(!bank["identities"][&id].is_null(), "UNBOUND_POSITION")?;
            for group in bank["riskGroups"][&id].as_array().cloned().unwrap_or_default() {
                let g = group.as_str().unwrap_or("").to_string();
                *used.entry(g).or_insert(BigInt::from(0)) += amount(&position["cost"])?;
            }
        }
        for (id, intent) in w["pending"].as_object().cloned().unwrap_or_default() {
            need(!bank["identities"][&id].is_null() && !intent["limit"].is_null()
                && amount(&intent["limit"])? <= amount(&bank["policy"]["maxOrderCost"])?, "UNBOUND_INTENT")?;
            if intent["action"].as_str() == Some("buy") {
                for group in bank["riskGroups"][&id].as_array().cloned().unwrap_or_default() {
                    let g = group.as_str().unwrap_or("").to_string();
                    *used.entry(g).or_insert(BigInt::from(0)) += amount(&intent["limit"])?;
                }
            }
        }
        for (group, value) in &used {
            need(*value <= amount(&bank["policy"]["groupCaps"][group])?, "CORRELATED_FUNDING")?;
        }
    }
    Ok(())
}
/// advanceSettlementPortfolio(state, row, bank, availableAt, account, evaluate)
/// Re-evaluates each pending intent without double-reserving itself while
/// retaining every other market's pending capital and event exposure.
pub fn advance_settlement_portfolio(state: &V, row: &V, bank: &V, available_at: &str, account: &V,
    evaluate: &mut dyn FnMut(&V) -> Result<V, String>) -> Result<V, String> {
    assert_settlement_portfolio_state(state, bank)?;
    let mut next = state.clone();
    let keys: Vec<String> = next["wallets"].as_object().map(|w| w.keys().cloned().collect()).unwrap_or_default();
    // advance_settlement_paper mutates `next` in place; the per-call wallet's
    // pending is snapshotted before the borrow — only the current market's
    // entry may be deleted mid-loop, and it is filtered out below anyway.
    let pendings: Vec<V> = keys.iter().map(|k| next["wallets"][k]["pending"].clone()).collect();
    let mut evaluated = 0usize;
    let market_id = row["payload"]["book"]["marketId"].as_str().unwrap_or("").to_string();
    let mut evaluate_wrap = |input: &V| -> Result<V, String> {
        need(evaluated < pendings.len(), "LATENCY_SCOPE")?;
        let w_pending = &pendings[evaluated];
        evaluated += 1;
        let mut input = input.clone();
        let reservations: Vec<V> = w_pending.as_object().cloned().unwrap_or_default().iter()
            .filter(|(id, p)| id.as_str() != market_id && p["action"].as_str() == Some("buy"))
            .map(|(id, p)| json!({"candidateId": format!("{}:YES", id), "maximumCost": p["limit"]}))
            .collect();
        input["wallet"]["reservations"] = V::Array(reservations);
        evaluate(&input)
    };
    crate::settlement_paper::advance_settlement_paper(&mut next, row, bank, available_at, account,
        &mut evaluate_wrap)?;
    need(evaluated == 0 || evaluated == keys.len(), "LATENCY_COUNT")?;
    assert_settlement_portfolio_state(&next, bank)?;
    Ok(next)
}
/// settlementPortfolioReport(state, bank, at)
pub fn settlement_portfolio_report(state: &V, bank: &V, at: &str) -> Result<V, String> {
    assert_settlement_portfolio_state(state, bank)?;
    let r = crate::settlement_paper::settlement_paper_report(state, bank, at)?;
    let mut out = r.clone();
    out["stateHash"] = json!(digest(state));
    out["sharedCashPerLatency"] = json!(true);
    out["independentLatencySamples"] = json!(false);
    let mut groups = Map::new();
    for x in bank["research"].as_array().cloned().unwrap_or_default() {
        groups.insert(x["marketId"].as_str().unwrap_or("").to_string(),
            bank["riskGroups"][x["marketId"].as_str().unwrap_or("")].clone());
    }
    out["eventGroups"] = V::Object(groups);
    out["maximumMarkets"] = json!(3);
    let mut limitations = r["limitations"].as_array().cloned().unwrap_or_default();
    limitations.push(json!("One funded cash ledger covers all markets within each latency case; correlated cases are never summed."));
    limitations.push(json!("Exact event groups include both held cost and every pending purchase; source probabilities are not guaranteed returns."));
    out["limitations"] = V::Array(limitations);
    Ok(out)
}
