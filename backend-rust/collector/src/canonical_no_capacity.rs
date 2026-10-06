// live-paper/canonical-no-capacity.mjs — canonical YES-bid complement NO capacity.
use baal_paper::fixed::{big_amount, big_price, money, SCALE, TICK};
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

pub fn canonical_no_policy() -> V {
    json!({"schemaVersion":1,"maxAgeMs":20000,"maxSkewMs":10000,"maximumAccountAgeMs":20000,
        "maximumReviewAgeMs":21600000,"minimumMarginalEdge":"0.010000","minimumTotalProfit":"0.005000",
        "adverseTicksPerLeg":1,"feePerShare":"0.000000","fixedFeePerLeg":"0.000000","depthFractionBps":100,
        "maxQuantity":100000,"maxCost":"25.000000","maxUnmatchedCost":"5.000000",
        "representation":"documented_same_exchange_yes_bid_complement","feeEvidence":"direct_human_zero_fee_instruction"})
}
fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("CANONICAL_CAPACITY_{}", code)); } Ok(()) }
fn id(v: &V) -> bool { v.as_str().is_some_and(|s| !s.is_empty() && s.as_bytes()[0] != b'0' && s.bytes().all(|b| b.is_ascii_digit()) && s.parse::<i64>().is_ok()) }
fn hex(v: &V) -> bool { v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())) }
fn instant(v: &V) -> i64 { baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).map(|m| m as i64).unwrap_or(i64::MIN) }
fn digest(v: &V) -> String { baal_service_runtime::json_text::digest(v) }
use num_bigint::BigInt;

/// validateCapacityReview(r)
pub fn validate_capacity_review(r: &V) -> Result<V, String> {
    need(r["schemaVersion"].as_i64() == Some(1) && r["kind"].as_str() == Some("conditional_no_pair_review")
        && r["id"].is_string() && r["tournamentId"].is_string() && hex(&r["evidenceHash"])
        && r["legs"].as_array().is_some_and(|l| l.len() == 2), "REVIEW")?;
    instant(&r["recordedAt"]); instant(&r["expiresAt"]);
    need(instant(&r["expiresAt"]) > instant(&r["recordedAt"]), "REVIEW_CLOCK")?;
    need(r["legs"].as_array().unwrap().iter().all(|l| id(&l["marketId"]) && id(&l["exchangeId"])
            && l["outcome"].as_str() == Some("NO") && hex(&l["rulesHash"]))
        && r["legs"].as_array().unwrap().iter().map(|l| l["exchangeId"].as_str()).collect::<BTreeSet<_>>().len() == 2
        && r["legs"].as_array().unwrap().iter().map(|l| l["marketId"].as_str()).collect::<BTreeSet<_>>().len() == 2, "REVIEW_SCOPE")?;
    need(r["assumptions"].as_array().is_some_and(|a| !a.is_empty() && a.iter().all(|v| v.as_str().is_some_and(|s| !s.is_empty()))), "ASSUMPTIONS")?;
    need(r["scenarios"].as_array().is_some_and(|s| s.len() > 1
            && s.iter().all(|x| x["id"].is_string() && ["ordinary","exception"].contains(&x["category"].as_str().unwrap_or(""))
                && x["payouts"].as_array().is_some_and(|p| p.len() == 2 && p.iter().all(|v| v.as_i64() == Some(0) || v.as_i64() == Some(1))))
            && s.iter().map(|x| x["id"].as_str()).collect::<BTreeSet<_>>().len() == s.len()
            && s.iter().any(|x| x["category"].as_str() == Some("ordinary"))
            && s.iter().any(|x| x["category"].as_str() == Some("exception") && x["payouts"].as_array().unwrap().iter().all(|v| v.as_i64() == Some(0)))), "SCENARIOS")?;
    Ok(r.clone())
}
fn policy_check(p: &V) -> Result<(), String> {
    for k in ["maxAgeMs","maxSkewMs","maximumAccountAgeMs","maximumReviewAgeMs","adverseTicksPerLeg","depthFractionBps","maxQuantity"] {
        need(p[k].as_i64().is_some_and(|x| x >= 0), "POLICY")?;
    }
    need(p["depthFractionBps"].as_i64().unwrap() > 0 && p["depthFractionBps"].as_i64().unwrap() <= 10000
        && p["adverseTicksPerLeg"].as_i64().unwrap() <= 5 && p["maxSkewMs"].as_i64().unwrap() <= p["maxAgeMs"].as_i64().unwrap(), "POLICY")?;
    for k in ["minimumMarginalEdge","minimumTotalProfit","feePerShare","fixedFeePerLeg","maxCost","maxUnmatchedCost"] {
        need(big_amount(p[k].as_str().unwrap_or("")).map(|x| x >= BigInt::from(0)).unwrap_or(false), "POLICY_COST")?;
    }
    need(p["representation"] == canonical_no_policy()["representation"] && p["feeEvidence"].as_str().is_some_and(|s| !s.is_empty()), "REPRESENTATION")?;
    Ok(())
}
/// evaluateCanonicalNoCapacity({review, books, account, now, policy})
pub fn evaluate_canonical_no_capacity(review: &V, books: &[V], account: Option<&V>, now: &str, policy: &V) -> Result<V, String> {
    validate_capacity_review(review)?;
    policy_check(policy)?;
    let at = instant(&json!(now));
    need(books.len() == 2, "BOOKS")?;
    let mut base = json!({"schemaVersion":1,"mode":"canonical_no_conditional_capacity","evaluatedAt":now,
        "reviewId":review["id"],"reviewHash":digest(review),"policyHash":digest(policy),
        "representation":policy["representation"],"nativeNoObserved":false,"scenarioCompletenessVerified":false,
        "executionAuthority":false,"ordersSubmitted":0,
        "ruleBindingKind":"manual_review_attestation_not_live_api_metadata",
        "networkRequests":0,"fills":V::Null,"realizedPnl":V::Null,"profitabilityEstablished":false});
    let reject = |reason: &str| -> V {
        let mut b = base.clone();
        b["reason"] = json!(reason); b["positiveConditionalCapacity"] = V::Bool(false);
        b["matchedQuantity"] = json!("0"); b["acquisitionCost"] = json!("0.000000"); b["segments"] = json!([]);
        b
    };
    if at < instant(&review["recordedAt"]) || at >= instant(&review["expiresAt"])
        || at - instant(&review["recordedAt"]) > policy["maximumReviewAgeMs"].as_i64().unwrap() {
        return Ok(reject("review_not_current"));
    }
    let mut starts = Vec::new();
    let mut receipts = Vec::new();
    let mut ladders = Vec::new();
    for (i, b) in books.iter().enumerate() {
        let l = &review["legs"][i];
        need(b["tournamentId"] == review["tournamentId"] && b["marketId"] == l["marketId"] && b["exchangeId"] == l["exchangeId"]
            && b["instrument"].as_str() == Some(&format!("{}:YES", l["exchangeId"].as_str().unwrap()))
            && b["freshnessPolicy"].as_str() == Some("authoritative_receipt_v1")
            && hex(&b["publicationHash"]) && b["publicationSequence"].as_i64().is_some_and(|x| x > 0), "BOOK_SCOPE")?;
        let start = instant(&b["requestStartedAt"]);
        let receipt = instant(&b["receivedAt"]);
        starts.push(start); receipts.push(receipt);
        need(start <= receipt && receipt <= at && instant(&b["publishedAt"]) >= receipt && instant(&b["publishedAt"]) <= at, "BOOK_CLOCK")?;
        need(b["sequence"].as_str().is_some_and(|s| s == "0" || (!s.starts_with('0') && s.bytes().all(|c| c.is_ascii_digit())))
            && b["sourceAt"].as_str().and_then(|s| baal_service_runtime::contract::timestamp_str(s).ok()).as_deref() == b["sourceAt"].as_str()
            && baal_service_runtime::contract::instant_ns(b["sourceAt"].as_str().unwrap_or("")).map_err(|e| e.0)? <= (receipt as i128 + 2000) * 1_000_000
            && b["versionKey"].as_str() == Some(&format!("{}:{}", b["sequence"].as_str().unwrap(), b["sourceAt"].as_str().unwrap())), "BOOK_VERSION")?;
        need(b["bids"].as_array().is_some_and(|x| !x.is_empty() && x.len() <= 200) && b["asks"].as_array().is_some_and(|x| !x.is_empty()), "DEPTH")?;
        need(big_price(b["bids"][0]["price"].as_str().unwrap_or(""))? < big_price(b["asks"][0]["price"].as_str().unwrap_or(""))?, "CROSSED_BOOK")?;
        let mut previous = BigInt::from(SCALE);
        let mut ladder = Vec::new();
        for v in b["bids"].as_array().unwrap() {
            let bid = big_price(v["price"].as_str().unwrap_or(""))?;
            need(bid < previous && v["quantity"].as_i64().is_some_and(|q| q > 0), "DEPTH")?;
            previous = bid.clone();
            let raw = BigInt::from(SCALE) - &bid;
            let effective = &raw + BigInt::from(policy["adverseTicksPerLeg"].as_i64().unwrap()) * BigInt::from(TICK)
                + big_amount(policy["feePerShare"].as_str().unwrap())?;
            need(&raw + BigInt::from(policy["adverseTicksPerLeg"].as_i64().unwrap()) * BigInt::from(TICK) <= BigInt::from(SCALE), "ADVERSE_PRICE")?;
            let quantity = BigInt::from(v["quantity"].as_i64().unwrap()) * BigInt::from(policy["depthFractionBps"].as_i64().unwrap()) / BigInt::from(10000);
            if quantity > BigInt::from(0) { ladder.push((raw, effective, quantity)); }
        }
        ladders.push(ladder);
    }
    if starts.iter().any(|s| *s < instant(&review["recordedAt"])) { return Ok(reject("review_not_known_at_request")); }
    if starts.iter().any(|s| at - s > policy["maxAgeMs"].as_i64().unwrap()) { return Ok(reject("stale_request")); }
    if *receipts.iter().max().unwrap() - *starts.iter().min().unwrap() > policy["maxSkewMs"].as_i64().unwrap() {
        return Ok(reject("whole_window_skew"));
    }
    let account = match account {
        Some(a) if a["schemaVersion"].as_i64() == Some(1)
            && a["excludedExchangeIds"].as_array().is_some_and(|x| x.iter().all(id)) => a,
        _ => return Ok(reject("account_unavailable")),
    };
    let min_start = *starts.iter().min().unwrap();
    if [instant(&account["positionsAt"]), instant(&account["ordersAt"])].iter()
        .any(|t| *t > min_start || at - t > policy["maximumAccountAgeMs"].as_i64().unwrap()) {
        return Ok(reject("account_not_causal_or_fresh"));
    }
    if review["legs"].as_array().unwrap().iter().any(|l| account["excludedExchangeIds"].as_array().unwrap().contains(&l["exchangeId"])) {
        return Ok(reject("own_account_exposure"));
    }
    if ladders.iter().any(|l| l.is_empty()) { return Ok(reject("fractional_depth")); }
    let ordinary: Vec<&V> = review["scenarios"].as_array().unwrap().iter().filter(|s| s["category"].as_str() == Some("ordinary")).collect();
    let floor = ordinary.iter().map(|s| s["payouts"][0].as_i64().unwrap() + s["payouts"][1].as_i64().unwrap()).min().unwrap() * SCALE as i64;
    let floor = BigInt::from(floor);
    let mut costs = vec![big_amount(policy["fixedFeePerLeg"].as_str().unwrap())?, big_amount(policy["fixedFeePerLeg"].as_str().unwrap())?];
    let mut indexes = [0usize, 0];
    let mut left: Vec<BigInt> = ladders.iter().map(|l| l[0].2.clone()).collect();
    let budget = big_amount(policy["maxCost"].as_str().unwrap())?;
    let unmatched = big_amount(policy["maxUnmatchedCost"].as_str().unwrap())?;
    let edge = big_amount(policy["minimumMarginalEdge"].as_str().unwrap())?;
    let mut quantity = BigInt::from(0);
    let mut stop_reason = "depth_exhausted";
    let mut segments = Vec::new();
    let max_q = BigInt::from(policy["maxQuantity"].as_i64().unwrap());
    while indexes.iter().enumerate().all(|(j, idx)| *idx < ladders[j].len()) && quantity < max_q {
        let levels: Vec<BigInt> = indexes.iter().enumerate().map(|(j, idx)| ladders[j][*idx].1.clone()).collect();
        let unit_cost: BigInt = &levels[0] + &levels[1];
        if &floor - &unit_cost < edge || floor <= unit_cost { stop_reason = "marginal_edge"; break; }
        if &costs[0] + &costs[1] > budget || costs.iter().any(|c| *c > unmatched) { stop_reason = "capital_limit"; break; }
        let mut take = left[0].clone().min(left[1].clone()).min(&max_q - &quantity).min((&budget - &costs[0] - &costs[1]) / &unit_cost);
        for i in 0..2 { take = take.min((&unmatched - &costs[i]) / &levels[i]); }
        if take <= BigInt::from(0) { stop_reason = "capital_limit"; break; }
        for i in 0..2 { costs[i] += &levels[i] * &take; left[i] -= &take; }
        quantity += &take;
        segments.push(json!({"quantity": take.to_string(),
            "noPrices": [money(&ladders[0][indexes[0]].0), money(&ladders[1][indexes[1]].0)],
            "adverseUnitCost": money(&unit_cost)}));
        for i in 0..2 {
            if left[i] == BigInt::from(0) {
                indexes[i] += 1;
                left[i] = if indexes[i] < ladders[i].len() { ladders[i][indexes[i]].2.clone() } else { BigInt::from(0) };
            }
        }
    }
    if quantity == max_q { stop_reason = "quantity_limit"; }
    let cost = if quantity > BigInt::from(0) { &costs[0] + &costs[1] } else { BigInt::from(0) };
    let ordinary_profit = &quantity * &floor - &cost;
    let minimum_profit = big_amount(policy["minimumTotalProfit"].as_str().unwrap())?;
    base["reason"] = json!(if quantity > BigInt::from(0) && ordinary_profit >= minimum_profit { "conditional_positive_capacity" } else { stop_reason });
    base["positiveConditionalCapacity"] = json!(quantity > BigInt::from(0) && ordinary_profit > BigInt::from(0) && ordinary_profit >= minimum_profit);
    base["stopReason"] = json!(stop_reason);
    base["matchedQuantity"] = json!(quantity.to_string());
    base["acquisitionCost"] = json!(money(&cost));
    base["conditionalOrdinaryMinimumProfit"] = json!(money(&ordinary_profit));
    let min_all = review["scenarios"].as_array().unwrap().iter().map(|s| s["payouts"][0].as_i64().unwrap() + s["payouts"][1].as_i64().unwrap()).min().unwrap();
    base["stressedMinimumProfit"] = json!(money(&(BigInt::from(min_all) * BigInt::from(SCALE) * &quantity - &cost)));
    base["maximumSingleLegCostAtRisk"] = json!(money(&(&quantity * costs[0].clone().max(costs[1].clone()))));
    base["scenarios"] = json!(review["scenarios"].as_array().unwrap().iter().map(|s| {
        let mut s = (*s).clone();
        s["profit"] = json!(money(&(BigInt::from(s["payouts"][0].as_i64().unwrap() + s["payouts"][1].as_i64().unwrap()) * BigInt::from(SCALE) * &quantity - &cost)));
        s
    }).collect::<Vec<_>>());
    base["sources"] = json!(books.iter().map(|b| json!({"marketId": b["marketId"], "exchangeId": b["exchangeId"],
        "requestStartedAt": b["requestStartedAt"], "receivedAt": b["receivedAt"],
        "publicationSequence": b["publicationSequence"], "publicationHash": b["publicationHash"],
        "versionKey": b["versionKey"]})).collect::<Vec<_>>());
    base["limitations"] = json!(["Canonical YES-bid complement, not independently captured NO depth.",
        "Current snapshot capacity is not atomic liquidity, a fill, a queue model or a profitable near-term exit.",
        "Ordinary election assumptions can fail through fusion, affiliation, rule changes or administrative settlement.",
        "Zero-payout exception stress may lose the full package cost; an unmatched-leg cap does not cover that loss."]);
    Ok(base)
}
