// live-paper/receipt-fill-price.mjs — counterfactual execution pricing.
use baal_paper::fixed::{big_price, instant, money, TICK};
use num_bigint::BigInt;
use serde_json::{json, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok {
        return Err(format!("RECEIPT_FILL_{code}"));
    }
    Ok(())
}
fn hex64(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn safe_int(v: &V) -> Option<i64> {
    if let Some(i) = v.as_i64() {
        return Some(i).filter(|x| x.abs() <= 9_007_199_254_740_991);
    }
    v.as_f64()
        .filter(|f| f.is_finite() && f.fract() == 0.0 && f.abs() <= 9_007_199_254_740_991.0)
        .map(|f| f as i64)
}

// Counterfactual execution pricing; independent of order-limit construction.
// No wallet mutation or contest dispatch. A later receipt is not proof that
// an actual order obtained the displayed liquidity or queue priority.
pub fn receipt_fill_price(
    intent: &V,
    book: &V,
    source_sequence: &V,
    source_hash: &V,
    available_at: &str,
    latency_ms: i64,
    depth_fraction_bps: i64,
    residual_adverse_ticks: i64,
    maximum_book_age_ms: i64,
) -> Result<V, String> {
    need(
        matches!(intent["action"].as_str(), Some("buy") | Some("sell"))
            && safe_int(&intent["quantity"]).is_some_and(|q| q > 0)
            && safe_int(&intent["sourceSequence"]).is_some_and(|s| s > 0)
            && hex64(&intent["sourceHash"])
            && intent["decisionVersion"].is_string()
            && latency_ms >= 0,
        "INTENT",
    )?;
    need(
        depth_fraction_bps > 0
            && depth_fraction_bps <= 10000
            && [0, 1, 2].contains(&residual_adverse_ticks),
        "MODEL",
    )?;
    let created = instant(intent["createdAt"].as_str().unwrap_or(""))?;
    let requested = instant(book["requestStartedAt"].as_str().unwrap_or(""))?;
    let received = instant(book["receivedAt"].as_str().unwrap_or(""))?;
    let available = instant(available_at)?;
    need(
        book["marketId"] == intent["marketId"]
            && book["exchangeId"] == intent["exchangeId"]
            && book["instrument"].as_str().is_some_and(|i| {
                i == format!("{}:YES", intent["exchangeId"].as_str().unwrap_or("")).as_str()
            }),
        "SCOPE",
    )?;
    need(safe_int(source_sequence).is_some() && hex64(source_hash), "SOURCE")?;
    let src_seq = safe_int(source_sequence).unwrap();
    let reject = |reason: &str| -> V {
        json!({"eligible":false,"reason":reason,"executionAuthority":false,"competitionOrders":0,"walletChanged":false})
    };
    if src_seq <= intent["sourceSequence"].as_i64().unwrap_or(0)
        || source_hash == &intent["sourceHash"]
        || book["versionKey"] == intent["decisionVersion"]
    {
        return Ok(reject("same_source"));
    }
    if requested <= created || requested < created + latency_ms {
        return Ok(reject("not_independently_later"));
    }
    if available > instant(intent["expiresAt"].as_str().unwrap_or(""))? {
        return Ok(reject("intent_expired"));
    }
    if received < requested || received > available || available - requested > maximum_book_age_ms {
        return Ok(reject("stale_or_future_receipt"));
    }
    let limit = big_price(intent["limit"].as_str().unwrap_or(""))?;
    let levels = &book[if intent["action"].as_str() == Some("buy") { "asks" } else { "bids" }];
    need(levels.is_array(), "DEPTH")?;
    let mut remaining = safe_int(&intent["quantity"]).unwrap();
    let mut total = BigInt::from(0);
    let mut raw_total = BigInt::from(0);
    let mut fills: Vec<V> = Vec::new();
    for (index, level) in levels.as_array().unwrap().iter().enumerate() {
        let quantity = safe_int(&level["quantity"]).filter(|q| *q >= 0);
        need(quantity.is_some(), "DEPTH_QUANTITY")?;
        let raw = big_price(level["price"].as_str().unwrap_or(""))?;
        let adjustment = BigInt::from(residual_adverse_ticks) * BigInt::from(TICK);
        let unit = if intent["action"].as_str() == Some("buy") {
            &raw + &adjustment
        } else {
            &raw - &adjustment
        };
        if unit < BigInt::from(TICK)
            || unit > BigInt::from(1_000_000 - TICK)
            || (intent["action"].as_str() == Some("buy") && unit > limit)
            || (intent["action"].as_str() == Some("sell") && unit < limit)
        {
            break;
        }
        let eligible = BigInt::from(quantity.unwrap()) * BigInt::from(depth_fraction_bps)
            / BigInt::from(10_000);
        let take = std::cmp::min(BigInt::from(remaining), eligible.clone());
        if take == BigInt::from(0) {
            continue;
        }
        raw_total += &raw * &take;
        total += &unit * &take;
        remaining -= take.to_string().parse::<i64>().unwrap_or(0);
        fills.push(json!({"levelIndex":index,"quantity":take.to_string().parse::<i64>().unwrap_or(0),
            "rawPrice":money(&raw),"modeledPrice":money(&unit)}));
        if remaining == 0 {
            break;
        }
    }
    if remaining != 0 {
        return Ok(reject("insufficient_limit_compliant_depth"));
    }
    Ok(json!({
        "eligible":true,
        "quantity":safe_int(&intent["quantity"]).unwrap(),
        "total":money(&total),
        "rawTotal":money(&raw_total),
        "residualStressAmount":money(&(&total - &raw_total)),
        "protectiveLimit":money(&limit),
        "residualAdverseTicks":residual_adverse_ticks,
        "fees":"0.000000",
        "fills":fills,
        "source":{"sequence":src_seq,"hash":source_hash,"version":book["versionKey"],
            "requestStartedAt":book["requestStartedAt"],"receivedAt":book["receivedAt"]},
        "model":if residual_adverse_ticks == 0 { "later_receipt_raw_baseline" } else { "later_receipt_plus_declared_residual_stress" },
        "counterfactualOnly":true,
        "executionAuthority":false,
        "competitionOrders":0,
        "walletChanged":false
    }))
}
