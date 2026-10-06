// live-paper/basket-observer.mjs — measurement only: no I/O, orders, portfolio
// accounting or simulated fills.
use baal_paper::fixed::{big_amount, big_price, money, SCALE, TICK};
use baal_service_runtime::contract::{instant_ns, timestamp_str};
use baal_service_runtime::json_text::canonical_hash;
use num_bigint::BigInt;
use serde_json::{json, Value as V};

pub fn basket_policy() -> V {
    json!({"name": "dual-no-measurement-v1", "depthFractionBps": 100, "totalBudget": "25.000000",
        "maxBasketQuantity": 100, "adverseTicksPerLeg": 1, "maxDataAgeMs": 20000,
        "maxReceiptSkewMs": 20000, "maxSourceClockSkewMs": 2000, "latencyMs": 1000})
}
fn need(ok: bool, msg: &str) -> Result<(), String> { if !ok { return Err(msg.into()); } Ok(()) }
fn hex(s: &V) -> bool { s.as_str().is_some_and(|x| x.len() == 64 && x.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())) }
fn id(s: &V) -> bool { s.as_str().is_some_and(|x| !x.is_empty() && x.as_bytes()[0] != b'0' && x.bytes().all(|b| b.is_ascii_digit()) && x.parse::<i64>().is_ok()) }
fn uuid(s: &str) -> bool {
    s.len() == 36 && s.bytes().enumerate().all(|(i, b)| match i { 8 | 13 | 18 | 23 => b == b'-', _ => b.is_ascii_hexdigit() && !b.is_ascii_uppercase() })
}
fn fields(v: &V, keys: &[&str]) -> bool {
    v.as_object().is_some_and(|m| m.len() == keys.len() && keys.iter().all(|k| m.contains_key(*k)))
}
fn instant(s: &V) -> Result<i64, String> {
    crate::model::instant(s.as_str().unwrap_or(""))
}
fn policy(overrides: &V) -> Result<V, String> {
    let base = basket_policy();
    need(overrides.is_object() && overrides.as_object().unwrap().keys().all(|k| base.get(k).is_some()), "Unknown basket policy")?;
    let mut p = base.as_object().unwrap().clone();
    for (k, v) in overrides.as_object().unwrap() { p.insert(k.clone(), v.clone()); }
    let p = V::Object(p);
    need(p["name"].as_str() == Some("dual-no-measurement-v1")
        && big_amount(p["totalBudget"].as_str().unwrap_or("")).map(|x| x > BigInt::from(0) && x <= BigInt::from(25_000_000)).unwrap_or(false),
        "Unsafe basket budget")?;
    for k in ["depthFractionBps", "maxBasketQuantity", "adverseTicksPerLeg", "maxDataAgeMs", "maxReceiptSkewMs", "latencyMs"] {
        need(p[k].as_i64().is_some_and(|x| x > 0), "Invalid basket policy")?;
    }
    need(p["depthFractionBps"].as_i64().unwrap() <= 100 && p["maxBasketQuantity"].as_i64().unwrap() <= 100
        && p["adverseTicksPerLeg"].as_i64().unwrap() <= 5 && p["maxDataAgeMs"].as_i64().unwrap() <= 20000
        && p["maxReceiptSkewMs"].as_i64().unwrap() <= p["maxDataAgeMs"].as_i64().unwrap()
        && (1000..=60000).contains(&p["latencyMs"].as_i64().unwrap())
        && p["maxSourceClockSkewMs"].as_i64().is_some_and(|x| (0..=2000).contains(&x)), "Unsafe basket policy")?;
    Ok(p)
}
fn proof(input: &V, tournament_id: &str, legs: &V, cutoff: &str) -> Result<V, String> {
    if input.is_null() { return Ok(json!({"status": "unverified", "reason": "metadata_only"})); }
    if input["status"].as_str() == Some("unverified") {
        need(fields(input, &["status", "reason"])
            && ["metadata_only", "missing_authoritative_relationship", "unverified_settlement_rules"].contains(&input["reason"].as_str().unwrap_or("")),
            "Invalid unverified proof")?;
        return Ok(input.clone());
    }
    need(fields(input, &["status","tournamentId","legs","relationship","minimumCombinedNoPayout","normalResolutionOnly","refundFloorVerified","evidenceHash","verifiedAt"])
        && input["status"].as_str() == Some("verified") && input["tournamentId"].as_str() == Some(tournament_id)
        && canonical_hash(&input["legs"]).unwrap_or_default() == canonical_hash(legs).unwrap_or_default()
        && input["minimumCombinedNoPayout"].as_str() == Some("1.000000")
        && input["normalResolutionOnly"] == V::Bool(true) && input["refundFloorVerified"] == V::Bool(false)
        && hex(&input["evidenceHash"])
        && instant(&input["verifiedAt"])? <= instant(&json!(cutoff))?, "Invalid scoped settlement proof")?;
    let r = &input["relationship"];
    need(fields(r, &["id","version","status","type","isExhaustive","members"])
        && r["id"].as_str().is_some_and(uuid)
        && r["version"].as_i64().is_some_and(|x| x > 0)
        && r["status"].as_str() == Some("active") && r["type"].as_str() == Some("mutually_exclusive")
        && r["isExhaustive"].is_boolean()
        && r["members"].as_array().is_some_and(|m| m.len() == 2), "Invalid relationship proof")?;
    for i in 0..2 {
        let m = &r["members"][i];
        need(fields(m, &["marketId","exchangeId","outcome","role"])
            && m["marketId"] == legs[i]["marketId"] && m["exchangeId"] == legs[i]["exchangeId"]
            && m["outcome"].as_str() == Some("YES") && m["role"].as_str() == Some("member"), "Relationship member mismatch")?;
    }
    Ok(input.clone())
}
/// createBasketObserver({tournamentId, legs, sourceHash, cutoff, settlementProof, policy})
pub fn create_basket_observer(tournament_id: &str, legs: &V, source_hash: &str, cutoff: &str,
    settlement_proof: &V, overrides: &V) -> Result<V, String> {
    need(uuid(tournament_id) && hex(&json!(source_hash)), "Invalid frozen basket scope")?;
    instant(&json!(cutoff))?;
    let ls = legs.as_array().cloned().unwrap_or_default();
    need(ls.len() == 2 && ls.iter().all(|x| fields(x, &["marketId","exchangeId","predicateHash"])
        && id(&x["marketId"]) && id(&x["exchangeId"]) && hex(&x["predicateHash"]))
        && ls[0]["marketId"] != ls[1]["marketId"] && ls[0]["exchangeId"] != ls[1]["exchangeId"]
        && ls[0]["predicateHash"] != ls[1]["predicateHash"], "Invalid scoped pair identity")?;
    let p = policy(overrides)?;
    let bank = json!({"schemaVersion": 1, "name": "dual-no-basket-observer-v1", "tournamentId": tournament_id,
        "legs": legs, "sourceHash": source_hash, "cutoff": cutoff, "policy": p,
        "policyHash": canonical_hash(&p)?, "settlementProof": proof(settlement_proof, tournament_id, legs, cutoff)?});
    Ok(json!({"bank": bank, "state": {"schemaVersion": 1, "bankHash": canonical_hash(&bank)?,
        "lastAt": null, "books": {}, "lastSeen": {}, "invalidatedAt": {}, "lastPairHash": null,
        "positiveAnchor": null, "positivePersistenceCount": 0,
        "counts": {"receipts": 0, "cofreshPairs": 0, "apparentPositivePairs": 0,
            "independentPositiveConfirmations": 0, "invalidations": 0}, "lastObservation": null}}))
}
fn check_state(s: &V, bank: &V) -> Result<(), String> {
    need(bank["name"].as_str() == Some("dual-no-basket-observer-v1")
        && bank["policyHash"].as_str() == policy(&bank["policy"]).ok().and_then(|p| canonical_hash(&p).ok()).as_deref()
        && s["schemaVersion"].as_i64() == Some(1)
        && s["bankHash"].as_str() == canonical_hash(bank).ok().as_deref(), "Frozen basket bank changed")?;
    need(s["books"].is_object() && s["invalidatedAt"].is_object()
        && s["counts"].as_object().is_some_and(|m| m.values().all(|x| x.as_i64().is_some_and(|v| v >= 0)))
        && s["positivePersistenceCount"].as_i64().is_some_and(|x| x >= 0), "Invalid basket state")?;
    Ok(())
}
fn checked_book(input: &V, bank: &V, s: &V) -> Result<V, String> {
    let legs = bank["legs"].as_array().cloned().unwrap_or_default();
    let leg = legs.iter().find(|x| x["marketId"] == input["marketId"]).ok_or("Foreign or nonauthoritative basket book")?;
    let p = &bank["policy"];
    need(input["tournamentId"] == bank["tournamentId"] && input["exchangeId"] == leg["exchangeId"]
        && input["instrument"].as_str() == Some(format!("{}:YES", leg["exchangeId"].as_str().unwrap_or("")).as_str())
        && input["freshnessPolicy"].as_str() == Some("authoritative_receipt_v1"), "Foreign or nonauthoritative basket book")?;
    let received = instant(&input["receivedAt"])?;
    let started = instant(&input["requestStartedAt"])?;
    let source = instant_ns(input["sourceAt"].as_str().unwrap_or("")).map_err(|e| e.0)?;
    let seq = input["sequence"].as_str().and_then(|x| x.parse::<i128>().ok()).ok_or("Invalid basket receipt/source clocks")?;
    need(started <= received && received - started <= p["maxDataAgeMs"].as_i64().unwrap()
        && (s["lastAt"].is_null() || received >= instant(&s["lastAt"])?)
        && source <= (received as i128 + p["maxSourceClockSkewMs"].as_i64().unwrap() as i128) * 1_000_000,
        "Invalid basket receipt/source clocks")?;
    let version_key = format!("{}:{}", input["sequence"].as_str().unwrap_or(""), timestamp_str(input["sourceAt"].as_str().unwrap_or("")).map_err(|e| e.0)?);
    need(input["versionKey"].as_str() == Some(version_key.as_str()), "Basket version mismatch")?;
    let eid = leg["exchangeId"].as_str().unwrap_or("");
    if let Some(old) = s["lastSeen"].get(eid).filter(|o| !o.is_null()) {
        need(started >= instant(&old["requestStartedAt"])? && received >= instant(&old["receivedAt"])?
            && seq >= old["sequence"].as_str().and_then(|x| x.parse::<i128>().ok()).unwrap_or(-1)
            && source >= instant_ns(old["sourceAt"].as_str().unwrap_or("")).unwrap_or(i128::MAX), "Basket clock/version rollback")?;
    }
    let mut out = json!({"tournamentId": bank["tournamentId"], "marketId": leg["marketId"], "exchangeId": leg["exchangeId"],
        "instrument": input["instrument"], "receivedAt": input["receivedAt"], "requestStartedAt": input["requestStartedAt"],
        "sourceAt": timestamp_str(input["sourceAt"].as_str().unwrap_or("")).map_err(|e| e.0)?,
        "sequence": input["sequence"], "versionKey": input["versionKey"], "freshnessPolicy": input["freshnessPolicy"]});
    for (side, dir) in [("bids", -1i32), ("asks", 1i32)] {
        let levels = input[side].as_array().filter(|a| !a.is_empty() && a.len() <= 1000).ok_or("Missing basket depth")?;
        let mut previous: Option<BigInt> = None;
        let mut out_levels = Vec::new();
        for l in levels {
            let v = big_price(l["price"].as_str().unwrap_or(""))?;
            need(l["quantity"].as_i64().is_some_and(|q| q > 0), "Invalid basket quantity")?;
            if let Some(prev) = &previous { need(if dir < 0 { v < *prev } else { v > *prev }, "Unsorted basket depth")?; }
            previous = Some(v.clone());
            out_levels.push(json!({"price": money(&v), "quantity": l["quantity"]}));
        }
        out[side] = json!(out_levels);
    }
    need(big_price(out["bids"][0]["price"].as_str().unwrap())? < big_price(out["asks"][0]["price"].as_str().unwrap())?, "Crossed basket book")?;
    out["contentHash"] = json!(canonical_hash(&json!({"bids": out["bids"], "asks": out["asks"]}))?);
    if s["lastSeen"][eid]["versionKey"] == out["versionKey"] && !s["lastSeen"][eid].is_null() {
        need(s["lastSeen"][eid]["contentHash"] == out["contentHash"], "Changed depth under unchanged basket version")?;
    }
    Ok(out)
}
fn walk(books: &[V], p: &V) -> V {
    let ladders: Vec<Vec<V>> = books.iter().map(|book| {
        book["bids"].as_array().cloned().unwrap_or_default().iter().filter_map(|l| {
            let yes_bid = l["price"].as_str().unwrap_or("").to_string();
            let cost = BigInt::from(SCALE) - big_price(&yes_bid).ok()? + BigInt::from(TICK) * BigInt::from(p["adverseTicksPerLeg"].as_i64().unwrap_or(0));
            let quantity = BigInt::from(l["quantity"].as_i64().unwrap_or(0)) * BigInt::from(p["depthFractionBps"].as_i64().unwrap_or(0)) / BigInt::from(10000);
            if quantity > BigInt::from(0) && cost < BigInt::from(SCALE) {
                Some(json!({"yesBid": yes_bid, "cost": cost.to_string(), "quantity": quantity.to_string()}))
            } else { None }
        }).collect()
    }).collect();
    let mut segments: Vec<V> = Vec::new();
    let mut leg_costs = [BigInt::from(0), BigInt::from(0)];
    let mut indices = [0usize, 0];
    let mut available: Vec<BigInt> = ladders.iter().map(|rows| rows.first().map(|r| r["quantity"].as_str().unwrap_or("0").parse().unwrap_or(BigInt::from(0))).unwrap_or(BigInt::from(0))).collect();
    let mut quantity = 0i64;
    let mut total = BigInt::from(0);
    let mut marginal: Option<BigInt> = None;
    let mut stop_reason = "displayed_depth_depleted";
    while indices[0] < ladders[0].len() && indices[1] < ladders[1].len() {
        let levels: Vec<&V> = (0..2).map(|i| &ladders[i][indices[i]]).collect();
        let unit = levels[0]["cost"].as_str().unwrap_or("0").parse::<BigInt>().unwrap_or(BigInt::from(0))
            + levels[1]["cost"].as_str().unwrap_or("0").parse::<BigInt>().unwrap_or(BigInt::from(0));
        if unit >= BigInt::from(SCALE) { stop_reason = "nonpositive_marginal_margin"; break; }
        let affordable: BigInt = (big_amount(p["totalBudget"].as_str().unwrap()).unwrap() - &total) / &unit;
        let qty = [
            available[0].clone(), available[1].clone(),
            BigInt::from(p["maxBasketQuantity"].as_i64().unwrap() - quantity), affordable,
        ].iter().cloned().min().unwrap().to_string().parse::<i64>().unwrap_or(0);
        if qty < 1 {
            stop_reason = if quantity >= p["maxBasketQuantity"].as_i64().unwrap() { "quantity_cap" } else { "budget_cap" };
            break;
        }
        for i in 0..2 {
            let cost = levels[i]["cost"].as_str().unwrap_or("0").parse::<BigInt>().unwrap_or(BigInt::from(0)) * BigInt::from(qty);
            leg_costs[i] += &cost; total += &cost;
            available[i] -= BigInt::from(qty);
            if available[i] == BigInt::from(0) {
                indices[i] += 1;
                available[i] = ladders[i].get(indices[i]).map(|r| r["quantity"].as_str().unwrap_or("0").parse().unwrap_or(BigInt::from(0))).unwrap_or(BigInt::from(0));
            }
        }
        quantity += qty; marginal = Some(unit.clone());
        segments.push(json!({"quantity": qty, "yesBids": levels.iter().map(|l| l["yesBid"].clone()).collect::<Vec<_>>(),
            "noUnitCostsAfterCushion": levels.iter().map(|l| l["cost"].as_str().unwrap().parse::<BigInt>().map(|b| money(&b)).unwrap_or_default()).collect::<Vec<_>>(),
            "marginalUnitCost": money(&unit), "cumulativeQuantity": quantity, "cumulativeCost": money(&total),
            "cumulativeConditionalMargin": money(&(BigInt::from(quantity) * BigInt::from(SCALE) - &total))}));
    }
    let best_unit: BigInt = books.iter().map(|book| {
        BigInt::from(SCALE) - big_price(book["bids"][0]["price"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0))
            + BigInt::from(TICK) * BigInt::from(p["adverseTicksPerLeg"].as_i64().unwrap_or(0))
    }).sum();
    json!({"quantity": quantity, "totalCost": money(&total), "legCosts": leg_costs.iter().map(money).collect::<Vec<_>>(),
        "marginalUnitCost": marginal.map(|m| json!(money(&m))).unwrap_or(V::Null),
        "bestUnitCostAfterCushion": money(&best_unit),
        "conditionalMargin": money(&(BigInt::from(quantity) * BigInt::from(SCALE) - &total)),
        "segments": segments, "stopReason": stop_reason, "depthFractionBps": p["depthFractionBps"],
        "displayedDepthUnits": ladders.iter().map(|rows| rows.iter().map(|l| l["quantity"].as_str().unwrap_or("0").parse::<BigInt>().unwrap_or(BigInt::from(0))).sum::<BigInt>().to_string()).collect::<Vec<_>>(),
        "unmatchedLegRisk": {"modeledFills": false, "worstCaseUnmatchedCostByLeg": leg_costs.iter().map(money).collect::<Vec<_>>(),
            "simultaneousOrEqualFillsGuaranteed": false}})
}
fn execution() -> V {
    json!({"eligibleForExecution": false, "ordersSubmitted": 0, "fills": null, "pnl": null, "profitabilityEstablished": false})
}
/// observeBasket(input, rawBook, bank)
pub fn observe_basket(input: &V, raw_book: &V, bank: &V) -> Result<V, String> {
    check_state(input, bank)?;
    let book = checked_book(raw_book, bank, input)?;
    let mut s = input.clone();
    let p = &bank["policy"];
    let at = instant(&book["receivedAt"])?;
    s["lastAt"] = book["receivedAt"].clone();
    s["counts"]["receipts"] = json!(s["counts"]["receipts"].as_i64().unwrap_or(0) + 1);
    macro_rules! finish {
        ($status:expr, $extra:expr) => {{
            let mut obs = json!({"at": book["receivedAt"], "status": $status,
                "classification": if bank["settlementProof"]["status"].as_str() == Some("verified") { "verified_normal_resolution_floor" } else { "unverified_metadata_lead" },
                "proofStatus": bank["settlementProof"]["status"], "conditionalFloor": "1.000000",
                "verifiedFloor": if bank["settlementProof"]["status"].as_str() == Some("verified") { json!("1.000000") } else { V::Null },
                "unconditionalFloor": null, "refundVoidTreatment": "not_covered_by_floor",
                "independentPair": false, "positivePersistenceCount": s["positivePersistenceCount"]});
            for (k, v) in $extra.as_object().unwrap() { obs[k] = v.clone(); }
            for (k, v) in execution().as_object().unwrap() { obs[k] = v.clone(); }
            s["lastObservation"] = obs.clone();
            return Ok(json!({"state": s, "observation": obs}));
        }};
    }
    let eid = book["exchangeId"].as_str().unwrap_or("").to_string();
    let invalidated = &s["invalidatedAt"][&eid];
    if !invalidated.is_null() && instant(&book["requestStartedAt"])? <= instant(invalidated)? {
        finish!("request_before_invalidation", json!({}));
    }
    s["books"][&eid] = book.clone();
    s["lastSeen"][&eid] = json!({"requestStartedAt": book["requestStartedAt"], "receivedAt": book["receivedAt"],
        "sourceAt": book["sourceAt"], "sequence": book["sequence"], "versionKey": book["versionKey"],
        "contentHash": book["contentHash"]});
    let books: Vec<V> = bank["legs"].as_array().unwrap().iter()
        .map(|leg| s["books"][leg["exchangeId"].as_str().unwrap()].clone()).collect();
    if books.iter().any(|b| b.is_null()) { finish!("waiting_for_pair", json!({})); }
    if books.iter().any(|b| instant(&b["requestStartedAt"]).unwrap_or(0) <= instant(&bank["cutoff"]).unwrap_or(0)) {
        finish!("before_cutoff", json!({}));
    }
    let skew = (instant(&books[0]["receivedAt"])? - instant(&books[1]["receivedAt"])?).abs();
    if books.iter().any(|b| at - instant(&b["receivedAt"]).unwrap_or(0) > p["maxDataAgeMs"].as_i64().unwrap())
        || skew > p["maxReceiptSkewMs"].as_i64().unwrap() {
        s["positiveAnchor"] = V::Null; s["positivePersistenceCount"] = json!(0);
        finish!("stale_pair", json!({}));
    }
    let pair_hash = canonical_hash(&json!(books.iter().map(|b| json!({"versionKey": b["versionKey"], "contentHash": b["contentHash"]})).collect::<Vec<_>>()))?;
    let new_pair_evidence = pair_hash != s["lastPairHash"].as_str().unwrap_or("");
    if new_pair_evidence { s["counts"]["cofreshPairs"] = json!(s["counts"]["cofreshPairs"].as_i64().unwrap_or(0) + 1); }
    s["lastPairHash"] = json!(pair_hash);
    let basket = walk(&books, p);
    let apparent_positive = basket["quantity"].as_i64().unwrap_or(0) > 0
        && big_amount(basket["conditionalMargin"].as_str().unwrap_or(""))? > BigInt::from(0);
    if !apparent_positive {
        s["positiveAnchor"] = V::Null; s["positivePersistenceCount"] = json!(0);
        finish!("no_positive_sized_basket", json!({"basket": basket, "apparentPositive": apparent_positive, "newPairEvidence": new_pair_evidence}));
    }
    if new_pair_evidence { s["counts"]["apparentPositivePairs"] = json!(s["counts"]["apparentPositivePairs"].as_i64().unwrap_or(0) + 1); }
    let anchor = s["positiveAnchor"].clone();
    let latency = p["latencyMs"].as_i64().unwrap();
    let independent = !anchor.is_null() && books.iter().enumerate().all(|(i, b)| {
        (b["versionKey"] != anchor["legs"][i]["versionKey"] || b["contentHash"] != anchor["legs"][i]["contentHash"])
            && instant(&b["requestStartedAt"]).unwrap_or(0) > instant(&anchor["at"]).unwrap_or(0) + latency
            && instant(&b["receivedAt"]).unwrap_or(0) > instant(&anchor["at"]).unwrap_or(0) + latency
    });
    if (anchor.is_null() && new_pair_evidence) || independent {
        s["positivePersistenceCount"] = json!(if independent { s["positivePersistenceCount"].as_i64().unwrap_or(0) + 1 } else { 1 });
        if independent { s["counts"]["independentPositiveConfirmations"] = json!(s["counts"]["independentPositiveConfirmations"].as_i64().unwrap_or(0) + 1); }
        s["positiveAnchor"] = json!({"at": book["receivedAt"],
            "legs": books.iter().map(|b| json!({"versionKey": b["versionKey"], "contentHash": b["contentHash"]})).collect::<Vec<_>>()});
    }
    let status = if independent { "independent_positive_persistence" } else if new_pair_evidence { "apparent_positive_basket" } else { "repeated_pair" };
    finish!(status, json!({"basket": basket, "apparentPositive": apparent_positive, "newPairEvidence": new_pair_evidence,
        "independentPair": independent,
        "pairReceipts": books.iter().map(|b| b["receivedAt"].clone()).collect::<Vec<_>>(),
        "pairVersions": books.iter().map(|b| b["versionKey"].clone()).collect::<Vec<_>>()}));
}
/// invalidateBasket(input, marketId, at, bank)
pub fn invalidate_basket(input: &V, market_id: Option<&str>, at: &str, bank: &V) -> Result<V, String> {
    check_state(input, bank)?;
    let clock = instant(&json!(at))?;
    need(input["lastAt"].is_null() || clock >= instant(&input["lastAt"])?, "Basket invalidation rollback")?;
    need(market_id.is_none() || bank["legs"].as_array().unwrap().iter().any(|l| l["marketId"].as_str() == market_id), "Foreign basket invalidation")?;
    let mut s = input.clone();
    s["lastAt"] = json!(at);
    s["counts"]["invalidations"] = json!(s["counts"]["invalidations"].as_i64().unwrap_or(0) + 1);
    s["positiveAnchor"] = V::Null; s["positivePersistenceCount"] = json!(0);
    for leg in bank["legs"].as_array().unwrap() {
        if market_id.is_none() || market_id == leg["marketId"].as_str() {
            let eid = leg["exchangeId"].as_str().unwrap();
            s["books"].as_object_mut().unwrap().remove(eid);
            s["invalidatedAt"][eid] = json!(at);
        }
    }
    s["lastObservation"] = V::Null;
    Ok(s)
}
/// basketReport(state, bank)
pub fn basket_report(state: &V, bank: &V) -> Result<V, String> {
    check_state(state, bank)?;
    let mut r = json!({"mode": "dual_no_basket_measurement", "bankHash": state["bankHash"], "sourceHash": bank["sourceHash"],
        "cutoff": bank["cutoff"], "asOf": state["lastAt"], "marketIds": bank["legs"].as_array().unwrap().iter().map(|x| x["marketId"].clone()).collect::<Vec<_>>(),
        "proofStatus": bank["settlementProof"]["status"], "counts": state["counts"],
        "positivePersistenceCount": state["positivePersistenceCount"], "lastObservation": state["lastObservation"],
        "limitations": ["Bulk or metadata similarity cannot prove a settlement payoff.",
            "Displayed depth is not a fill or reserved inventory.",
            "Atomic multi-leg placement does not establish equal fills.",
            "Normal binary-resolution floor excludes cancellation, refund and override scenarios."]});
    for (k, v) in execution().as_object().unwrap() { r[k] = v.clone(); }
    Ok(r)
}
