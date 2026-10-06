// live-paper/canonical-no-capacity.mjs — BUY NO consumes canonical YES bids.
// This is the documented same-exchange conversion, not an independently
// observed native NO book or a second source. Ordinary-election capacity is
// always paired with zero-payout exception stress.
use baal_service_runtime::contract::instant_ns;
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::{digest, js_int};
use serde_json::{json, Map, Value as V};

fn need(ok: bool, c: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("CANONICAL_CAPACITY_{c}")) }
}
fn id(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n > 0 && n <= 9_007_199_254_740_991)
    })
}
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn vinstant(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn vamount(v: &V) -> Result<i128, String> {
    amount(v.as_str().unwrap_or(""))
}
fn vprice(v: &V) -> Result<i128, String> {
    price(v.as_str().unwrap_or(""))
}

// CANONICAL_NO_POLICY (frozen object).
pub fn canonical_no_policy() -> V {
    json!({
        "schemaVersion": 1,
        "maxAgeMs": 20000,
        "maxSkewMs": 10000,
        "maximumAccountAgeMs": 20000,
        "maximumReviewAgeMs": 21600000,
        "minimumMarginalEdge": "0.010000",
        "minimumTotalProfit": "0.005000",
        "adverseTicksPerLeg": 1,
        "feePerShare": "0.000000",
        "fixedFeePerLeg": "0.000000",
        "depthFractionBps": 100,
        "maxQuantity": 100000,
        "maxCost": "25.000000",
        "maxUnmatchedCost": "5.000000",
        "representation": "documented_same_exchange_yes_bid_complement",
        "feeEvidence": "direct_human_zero_fee_instruction",
    })
}

// validateCapacityReview(r)
pub fn validate_capacity_review(r: &V) -> Result<V, String> {
    need(
        get(r, "schemaVersion") == &json!(1)
            && get(r, "kind").as_str() == Some("conditional_no_pair_review")
            && get(r, "id").is_string()
            && get(r, "tournamentId").is_string()
            && hex(get(r, "evidenceHash"))
            && get(r, "legs").is_array()
            && get(r, "legs").as_array().unwrap().len() == 2,
        "REVIEW",
    )?;
    vinstant(get(r, "recordedAt"))?;
    vinstant(get(r, "expiresAt"))?;
    need(
        vinstant(get(r, "expiresAt"))? > vinstant(get(r, "recordedAt"))?,
        "REVIEW_CLOCK",
    )?;
    let legs = get(r, "legs").as_array().unwrap();
    let distinct_exchanges = legs
        .iter()
        .map(|l| get(l, "exchangeId").as_str().unwrap_or(""))
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let distinct_markets = legs
        .iter()
        .map(|l| get(l, "marketId").as_str().unwrap_or(""))
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    need(
        legs.iter().all(|l| {
            id(get(l, "marketId")) && id(get(l, "exchangeId"))
                && get(l, "outcome").as_str() == Some("NO")
                && hex(get(l, "rulesHash"))
        }) && distinct_exchanges == 2 && distinct_markets == 2,
        "REVIEW_SCOPE",
    )?;
    need(
        get(r, "assumptions").as_array().is_some_and(|a| {
            !a.is_empty() && a.iter().all(|v| v.as_str().is_some_and(|s| !s.is_empty()))
        }),
        "ASSUMPTIONS",
    )?;
    let scenarios = get(r, "scenarios").as_array();
    need(
        scenarios.is_some_and(|ss| {
            ss.len() > 1
                && ss.iter().all(|s| {
                    get(s, "id").is_string()
                        && matches!(get(s, "category").as_str(), Some("ordinary" | "exception"))
                        && get(s, "payouts").as_array().is_some_and(|p| {
                            p.len() == 2 && p.iter().all(|v| v == &json!(0) || v == &json!(1))
                        })
                })
                && ss.iter().map(|s| get(s, "id").to_string()).collect::<std::collections::BTreeSet<_>>().len() == ss.len()
                && ss.iter().any(|s| get(s, "category").as_str() == Some("ordinary"))
                && ss.iter().any(|s| {
                    get(s, "category").as_str() == Some("exception")
                        && get(s, "payouts").as_array().is_some_and(|p| p.iter().all(|v| v == &json!(0)))
                })
        }),
        "SCENARIOS",
    )?;
    Ok(r.clone())
}

fn policy_check(p: &V) -> Result<(), String> {
    for k in ["maxAgeMs", "maxSkewMs", "maximumAccountAgeMs", "maximumReviewAgeMs", "adverseTicksPerLeg", "depthFractionBps", "maxQuantity"] {
        need(js_int(get(p, k)).is_some_and(|n| n >= 0), "POLICY")?;
    }
    need(
        js_int(get(p, "depthFractionBps")).unwrap_or(0) > 0
            && js_int(get(p, "depthFractionBps")).unwrap_or(0) <= 10_000
            && js_int(get(p, "adverseTicksPerLeg")).unwrap_or(0) <= 5
            && js_int(get(p, "maxSkewMs")).unwrap_or(0) <= js_int(get(p, "maxAgeMs")).unwrap_or(0),
        "POLICY",
    )?;
    for k in ["minimumMarginalEdge", "minimumTotalProfit", "feePerShare", "fixedFeePerLeg", "maxCost", "maxUnmatchedCost"] {
        need(vamount(get(p, k))? >= 0, "POLICY_COST")?;
    }
    need(
        get(p, "representation").as_str()
            == canonical_no_policy().get("representation").and_then(V::as_str)
            && get(p, "feeEvidence").as_str().is_some_and(|s| !s.is_empty()),
        "REPRESENTATION",
    )?;
    Ok(())
}

// evaluateCanonicalNoCapacity({review,books,account,now,policy})
pub fn evaluate_canonical_no_capacity(input: &V) -> Result<V, String> {
    let review = get(input, "review");
    let books = get(input, "books");
    let account = get(input, "account");
    let now = get(input, "now");
    let default_policy;
    let policy = if get(input, "policy").is_null() {
        default_policy = canonical_no_policy();
        &default_policy
    } else {
        get(input, "policy")
    };
    validate_capacity_review(review)?;
    policy_check(policy)?;
    let at = vinstant(now)?;
    need(books.is_array() && books.as_array().unwrap().len() == 2, "BOOKS")?;
    let mut base = Map::from_iter([
        ("schemaVersion".into(), json!(1)),
        ("mode".into(), json!("canonical_no_conditional_capacity")),
        ("evaluatedAt".into(), now.clone()),
        ("reviewId".into(), get(review, "id").clone()),
        ("reviewHash".into(), json!(digest(review))),
        ("policyHash".into(), json!(digest(policy))),
        ("representation".into(), get(policy, "representation").clone()),
        ("nativeNoObserved".into(), json!(false)),
        ("scenarioCompletenessVerified".into(), json!(false)),
        ("executionAuthority".into(), json!(false)),
        ("ordersSubmitted".into(), json!(0)),
        ("ruleBindingKind".into(), json!("manual_review_attestation_not_live_api_metadata")),
        ("networkRequests".into(), json!(0)),
        ("fills".into(), V::Null),
        ("realizedPnl".into(), V::Null),
        ("profitabilityEstablished".into(), json!(false)),
    ]);
    let reject = |reason: &str| -> V {
        let mut o = base.clone();
        o.insert("reason".into(), json!(reason));
        o.insert("positiveConditionalCapacity".into(), json!(false));
        o.insert("matchedQuantity".into(), json!("0"));
        o.insert("acquisitionCost".into(), json!("0.000000"));
        o.insert("segments".into(), json!([]));
        V::Object(o)
    };
    let adverse_ticks = js_int(get(policy, "adverseTicksPerLeg")).unwrap_or(0) as i128;
    let depth_fraction_bps = js_int(get(policy, "depthFractionBps")).unwrap_or(0) as i128;
    let max_age_ms = js_int(get(policy, "maxAgeMs")).unwrap_or(0);
    let max_skew_ms = js_int(get(policy, "maxSkewMs")).unwrap_or(0);
    let maximum_account_age_ms = js_int(get(policy, "maximumAccountAgeMs")).unwrap_or(0);
    let maximum_review_age_ms = js_int(get(policy, "maximumReviewAgeMs")).unwrap_or(0);
    let max_quantity = js_int(get(policy, "maxQuantity")).unwrap_or(0) as i128;
    if at < vinstant(get(review, "recordedAt"))?
        || at >= vinstant(get(review, "expiresAt"))?
        || at - vinstant(get(review, "recordedAt"))? > maximum_review_age_ms
    {
        return Ok(reject("review_not_current"));
    }
    let mut starts = Vec::new();
    let mut receipts = Vec::new();
    let mut ladders: Vec<Vec<(i128, i128, i128)>> = Vec::new();
    for (i, b) in books.as_array().unwrap().iter().enumerate() {
        let l = &get(review, "legs").as_array().unwrap()[i];
        need(
            get(b, "tournamentId") == get(review, "tournamentId")
                && get(b, "marketId") == get(l, "marketId")
                && get(b, "exchangeId") == get(l, "exchangeId")
                && get(b, "instrument").as_str()
                    == Some(format!("{}:YES", get(l, "exchangeId").as_str().unwrap_or("")).as_str())
                && get(b, "freshnessPolicy").as_str() == Some("authoritative_receipt_v1")
                && hex(get(b, "publicationHash"))
                && js_int(get(b, "publicationSequence")).is_some_and(|n| n > 0),
            "BOOK_SCOPE",
        )?;
        let start = vinstant(get(b, "requestStartedAt"))?;
        let receipt = vinstant(get(b, "receivedAt"))?;
        starts.push(start);
        receipts.push(receipt);
        need(
            start <= receipt && receipt <= at
                && vinstant(get(b, "publishedAt"))? >= receipt
                && vinstant(get(b, "publishedAt"))? <= at,
            "BOOK_CLOCK",
        )?;
        need(
            get(b, "sequence").as_str().is_some_and(|s| {
                (s == "0" || !s.starts_with('0')) && !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
            }) && get(b, "sourceAt") == &V::String(baal_service_runtime::contract::timestamp(get(b, "sourceAt")).map_err(|e| e.0)?)
                && instant_ns(get(b, "sourceAt").as_str().unwrap_or("")).map_err(|e| e.0)?
                    <= (receipt as i128 + 2_000) * 1_000_000
                && get(b, "versionKey").as_str()
                    == Some(format!("{}:{}", get(b, "sequence").as_str().unwrap_or(""), get(b, "sourceAt").as_str().unwrap_or("")).as_str()),
            "BOOK_VERSION",
        )?;
        need(
            get(b, "bids").as_array().is_some_and(|a| !a.is_empty() && a.len() <= 200)
                && get(b, "asks").as_array().is_some_and(|a| !a.is_empty()),
            "DEPTH",
        )?;
        need(
            vprice(get(&get(b, "bids").as_array().unwrap()[0], "price"))?
                < vprice(get(&get(b, "asks").as_array().unwrap()[0], "price"))?,
            "CROSSED_BOOK",
        )?;
        let mut previous = SCALE;
        let mut ladder = Vec::new();
        for v in get(b, "bids").as_array().unwrap() {
            let bid = vprice(get(v, "price"))?;
            need(
                bid < previous && js_int(get(v, "quantity")).is_some_and(|q| q > 0),
                "DEPTH",
            )?;
            previous = bid;
            let raw = SCALE - bid;
            let effective = raw + adverse_ticks * TICK + vamount(get(policy, "feePerShare"))?;
            need(raw + adverse_ticks * TICK <= SCALE, "ADVERSE_PRICE")?;
            let quantity = js_int(get(v, "quantity")).unwrap() as i128 * depth_fraction_bps / 10_000;
            if quantity > 0 {
                ladder.push((raw, effective, quantity));
            }
        }
        ladders.push(ladder);
    }
    if starts.iter().any(|s| *s < vinstant(get(review, "recordedAt")).unwrap_or(0)) {
        return Ok(reject("review_not_known_at_request"));
    }
    if starts.iter().any(|s| at - s > max_age_ms) {
        return Ok(reject("stale_request"));
    }
    if receipts.iter().max().unwrap_or(&0) - starts.iter().min().unwrap_or(&0) > max_skew_ms {
        return Ok(reject("whole_window_skew"));
    }
    if account.is_null()
        || get(account, "schemaVersion") != &json!(1)
        || !get(account, "excludedExchangeIds").is_array()
        || get(account, "excludedExchangeIds").as_array().unwrap().iter().any(|x| !id(x))
    {
        return Ok(reject("account_unavailable"));
    }
    let positions_at = vinstant(get(account, "positionsAt"))?;
    let orders_at = vinstant(get(account, "ordersAt"))?;
    if [positions_at, orders_at].iter().any(|t| {
        *t > *starts.iter().min().unwrap() || at - *t > maximum_account_age_ms
    }) {
        return Ok(reject("account_not_causal_or_fresh"));
    }
    if get(review, "legs").as_array().unwrap().iter().any(|l| {
        get(account, "excludedExchangeIds").as_array().unwrap().contains(get(l, "exchangeId"))
    }) {
        return Ok(reject("own_account_exposure"));
    }
    if ladders.iter().any(|l| l.is_empty()) {
        return Ok(reject("fractional_depth"));
    }
    let ordinary: Vec<&V> = get(review, "scenarios").as_array().unwrap().iter()
        .filter(|s| get(s, "category").as_str() == Some("ordinary"))
        .collect();
    let floor: i128 = ordinary
        .iter()
        .map(|s| {
            let p = get(s, "payouts").as_array().unwrap();
            js_int(&p[0]).unwrap_or(0) as i128 + js_int(&p[1]).unwrap_or(0) as i128
        })
        .min()
        .unwrap_or(0)
        * SCALE;
    let mut costs = [vamount(get(policy, "fixedFeePerLeg"))?, vamount(get(policy, "fixedFeePerLeg"))?];
    let mut indexes = [0usize, 0usize];
    let mut left = [ladders[0][0].2, ladders[1][0].2];
    let budget = vamount(get(policy, "maxCost"))?;
    let unmatched = vamount(get(policy, "maxUnmatchedCost"))?;
    let edge = vamount(get(policy, "minimumMarginalEdge"))?;
    let mut quantity: i128 = 0;
    let mut stop_reason = "depth_exhausted".to_string();
    let mut segments: Vec<V> = Vec::new();
    while indexes[0] < ladders[0].len() && indexes[1] < ladders[1].len() && quantity < max_quantity {
        let l0 = ladders[0][indexes[0]];
        let l1 = ladders[1][indexes[1]];
        let unit_cost = l0.1 + l1.1;
        if floor - unit_cost < edge || floor <= unit_cost {
            stop_reason = "marginal_edge".into();
            break;
        }
        if costs[0] + costs[1] > budget || costs.iter().any(|c| *c > unmatched) {
            stop_reason = "capital_limit".into();
            break;
        }
        let take = [
            left[0],
            left[1],
            max_quantity - quantity,
            (budget - costs[0] - costs[1]) / unit_cost,
            (unmatched - costs[0]) / l0.1,
            (unmatched - costs[1]) / l1.1,
        ]
        .into_iter()
        .min()
        .unwrap();
        if take <= 0 {
            stop_reason = "capital_limit".into();
            break;
        }
        costs[0] += l0.1 * take;
        costs[1] += l1.1 * take;
        left[0] -= take;
        left[1] -= take;
        quantity += take;
        segments.push(json!({
            "quantity": take.to_string(),
            "noPrices": [money(l0.0), money(l1.0)],
            "adverseUnitCost": money(unit_cost),
        }));
        for i in 0..2 {
            if left[i] == 0 {
                indexes[i] += 1;
                left[i] = ladders[i].get(indexes[i]).map(|x| x.2).unwrap_or(0);
            }
        }
    }
    if quantity == max_quantity {
        stop_reason = "quantity_limit".into();
    }
    let cost = if quantity != 0 { costs[0] + costs[1] } else { 0 };
    let ordinary_profit = quantity * floor - cost;
    let min_total_profit = vamount(get(policy, "minimumTotalProfit"))?;
    let stressed_min: i128 = get(review, "scenarios").as_array().unwrap().iter()
        .map(|s| {
            let p = get(s, "payouts").as_array().unwrap();
            js_int(&p[0]).unwrap_or(0) as i128 + js_int(&p[1]).unwrap_or(0) as i128
        })
        .min()
        .unwrap_or(0)
        * SCALE * quantity - cost;
    let max_single_leg = if quantity != 0 {
        std::cmp::min(-costs[0], -costs[1]) * -1
    } else {
        0
    };
    let scenarios: Vec<V> = get(review, "scenarios").as_array().unwrap().iter().map(|s| {
        let p = get(s, "payouts").as_array().unwrap();
        let sum = js_int(&p[0]).unwrap_or(0) as i128 + js_int(&p[1]).unwrap_or(0) as i128;
        let mut o = s.as_object().cloned().unwrap_or_default();
        o.insert("profit".into(), json!(money(sum * SCALE * quantity - cost)));
        V::Object(o)
    }).collect();
    let sources: Vec<V> = books.as_array().unwrap().iter().map(|b| {
        json!({
            "marketId": get(b, "marketId"),
            "exchangeId": get(b, "exchangeId"),
            "requestStartedAt": get(b, "requestStartedAt"),
            "receivedAt": get(b, "receivedAt"),
            "publicationSequence": get(b, "publicationSequence"),
            "publicationHash": get(b, "publicationHash"),
            "versionKey": get(b, "versionKey"),
        })
    }).collect();
    base.insert("reason".into(), json!(if quantity != 0 && ordinary_profit >= min_total_profit { "conditional_positive_capacity" } else { stop_reason.as_str() }));
    base.insert("positiveConditionalCapacity".into(), json!(quantity > 0 && ordinary_profit > 0 && ordinary_profit >= min_total_profit));
    base.insert("stopReason".into(), json!(stop_reason));
    base.insert("matchedQuantity".into(), json!(quantity.to_string()));
    base.insert("acquisitionCost".into(), json!(money(cost)));
    base.insert("conditionalOrdinaryMinimumProfit".into(), json!(money(ordinary_profit)));
    base.insert("stressedMinimumProfit".into(), json!(money(stressed_min)));
    base.insert("maximumSingleLegCostAtRisk".into(), json!(money(max_single_leg)));
    base.insert("segments".into(), json!(segments));
    base.insert("scenarios".into(), json!(scenarios));
    base.insert("sources".into(), json!(sources));
    base.insert("limitations".into(), json!([
        "Canonical YES-bid complement, not independently captured NO depth.",
        "Current snapshot capacity is not atomic liquidity, a fill, a queue model or a profitable near-term exit.",
        "Ordinary election assumptions can fail through fusion, affiliation, rule changes or administrative settlement.",
        "Zero-payout exception stress may lose the full package cost; an unmatched-leg cap does not cover that loss.",
    ]));
    Ok(V::Object(base))
}
