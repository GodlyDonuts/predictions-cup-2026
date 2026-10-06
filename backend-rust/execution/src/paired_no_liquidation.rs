// execution/paired-no-liquidation.mjs — economic signal ONLY: no account
// client, write authorizer, order executor or authoritative SELL reconciler.
// Hashes bind evidence; they do not grant authority.
use crate::core::payload_hash;
use crate::gates::normalize_official_fill_tick;
use crate::receipts::parse_number_lexemes;
use baal_service_runtime::contract::{instant_ns, timestamp_str};
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::sha256;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::BTreeSet;

fn need(ok: bool, c: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("PAIRED_NO_EXIT_{c}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n > 0 && n <= 9_007_199_254_740_991)
    })
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn amount_v(v: &V) -> Result<i128, String> {
    amount(v.as_str().unwrap_or(""))
}

// PAIRED_NO_LIQUIDATION_POLICY (frozen object).
pub fn paired_no_liquidation_policy() -> V {
    json!({
        "schemaVersion": 1,
        "quantity": 1,
        "fee": "0.000000",
        "minimumPairGain": "0.005000",
        "protectedAdverseTicks": 1,
        "depthFractionBps": 100,
        "maxBookAgeMs": 20000,
        "maxBookSkewMs": 10000,
        "lifetimeMs": 30000,
    })
}

// depthUnits(lexeme): /^(\d+)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$/ -> exact
// rational {numerator,denominator} as BigInt.
fn depth_units(lexeme: Option<&str>) -> Result<(BigInt, BigInt), String> {
    need(lexeme.is_some_and(|x| x.len() <= 128), "DEPTH_LEXEME")?;
    let s = lexeme.unwrap();
    let mut it = s.splitn(2, |c| c == 'e' || c == 'E');
    let mant = it.next().unwrap_or("");
    let exp_s = it.next();
    let mut mit = mant.splitn(2, '.');
    let whole = mit.next().unwrap_or("");
    let frac = mit.next();
    let ok = !whole.is_empty()
        && whole.bytes().all(|b| b.is_ascii_digit())
        && frac.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
        && exp_s.is_none_or(|e| {
            let e = e.strip_prefix('+').or_else(|| e.strip_prefix('-')).unwrap_or(e);
            !e.is_empty() && e.bytes().all(|b| b.is_ascii_digit())
        });
    need(ok, "DEPTH_NUMBER")?;
    let exp_value: i64 = match exp_s {
        None => 0,
        Some(e) => {
            let negative = e.starts_with('-');
            let digits = e.trim_start_matches(['+', '-']);
            let v: i64 = digits.parse().unwrap_or(i64::MAX / 2);
            if negative { -v } else { v }
        }
    };
    let exponent: i64 = exp_value - frac.map(|f| f.len()).unwrap_or(0) as i64;
    need(exponent.abs() <= 300, "DEPTH_EXPONENT")?;
    let digits_str: String = mant.chars().filter(|c| *c != '.').collect();
    let digits = BigInt::parse_bytes(digits_str.as_bytes(), 10).unwrap_or_else(|| BigInt::from(0));
    need(digits > BigInt::from(0), "DEPTH_POSITIVE")?;
    if exponent >= 0 {
        Ok((digits * BigInt::from(10).pow(exponent as u32), BigInt::from(1)))
    } else {
        Ok((digits, BigInt::from(10).pow((-exponent) as u32)))
    }
}

// normalizeExitBook(parsed,lot,scope)
fn normalize_exit_book(parsed_data: &V, numbers: &std::collections::BTreeMap<String, String>, lot: &V, scope: &V) -> Result<V, String> {
    let raw = parsed_data;
    need(
        (!raw.as_object().is_some_and(|m| m.contains_key("marketId"))
            || get(raw, "marketId") == get(lot, "marketId"))
            && get(raw, "contexts").as_array().is_some_and(|c| c.len() == 1)
            && get(&get(raw, "contexts")[0], "type") == &json!("tournament")
            && get(&get(&get(raw, "contexts")[0], "tournament"), "id") == get(scope, "tournamentId")
            && get(&get(&get(raw, "contexts")[0], "orderbook"), "exchanges")
                .as_array()
                .is_some_and(|e| e.len() == 1),
        "BOOK_SCOPE",
    )?;
    let e = get(&get(&get(raw, "contexts")[0], "orderbook"), "exchanges")[0].clone();
    let v = get(&e, "asOf");
    need(
        get(&e, "exchangeId") == get(lot, "exchangeId")
            && get(&e, "option") == &json!("YES")
            && js_int_safe(get(v, "sequence")).is_some_and(|n| n >= 0),
        "BOOK_EXCHANGE_VERSION",
    )?;
    let source_at = timestamp_str(get(v, "at").as_str().unwrap_or("")).map_err(|e| e.0)?;
    let levels = |name: &str| -> Result<Vec<V>, String> {
        let arr = get(&e, name).as_array().cloned().unwrap_or_default();
        need(!arr.is_empty() && arr.len() <= 20, "BOOK_DEPTH")?;
        let mut previous: Option<i128> = None;
        let mut out = Vec::new();
        for (i, l) in arr.iter().enumerate() {
            need(
                get(l, "quantity").as_f64().is_some_and(|q| q.is_finite() && q > 0.0),
                "DEPTH_NUMBER",
            )?;
            let px = get(&normalize_official_fill_tick(get(l, "price"))?, "price")
                .as_str()
                .unwrap_or("")
                .to_string();
            let p = price(&px)?;
            need(
                previous.is_none() || (if name == "asks" { p > previous.unwrap() } else { p < previous.unwrap() }),
                "BOOK_LEVEL_ORDER",
            )?;
            previous = Some(p);
            let key = format!("[\"contexts\",0,\"orderbook\",\"exchanges\",0,\"{name}\",{i},\"quantity\"]");
            let lexeme = numbers.get(&key).cloned();
            let (numerator, denominator) = depth_units(lexeme.as_deref())?;
            out.push(json!({
                "price": px,
                "quantityLexeme": lexeme.unwrap_or_default(),
                "numerator": numerator.to_string(),
                "denominator": denominator.to_string(),
            }));
        }
        Ok(out)
    };
    Ok(json!({
        "exchangeId": get(&e, "exchangeId"),
        "sourceAt": source_at,
        "sequence": js_int_safe(get(v, "sequence")).map(|n| n.to_string()).unwrap_or_default(),
        "bids": levels("bids")?,
        "asks": levels("asks")?,
    }))
}
fn js_int_safe(v: &V) -> Option<i64> {
    baal_service_runtime::json_text::js_int(v)
}
fn level_num(v: &V) -> BigInt {
    BigInt::parse_bytes(get(v, "numerator").as_str().unwrap_or("").as_bytes(), 10).unwrap_or_else(|| BigInt::from(0))
}
fn level_den(v: &V) -> BigInt {
    BigInt::parse_bytes(get(v, "denominator").as_str().unwrap_or("").as_bytes(), 10).unwrap_or_else(|| BigInt::from(1))
}

// proofLot(p,scope)
fn proof_lot(p: &V, scope: &V) -> Result<V, String> {
    let mut body = p.clone();
    let verification_hash = body.as_object_mut().and_then(|m| m.remove("verificationHash"));
    need(
        verification_hash.as_ref().is_some_and(|h| hex(h))
            && payload_hash(&body)? == verification_hash.as_ref().and_then(|h| h.as_str()).unwrap_or("")
            && get(p, "schemaVersion") == &json!(1)
            && get(p, "mode") == &json!("scoped_rest_no_lot_with_rounded_cash_v1")
            && get(p, "scopedLotVerified") == &json!(true)
            && get(p, "filledQuantity") == &json!(1)
            && get(p, "signedPositionDelta") == &json!(-1)
            && get(p, "executionAuthority") == &json!(false),
        "ORIGINAL_BUY_PROOF",
    )?;
    need(
        get(p, "ownerId") == get(scope, "ownerId")
            && get(p, "runId") == get(scope, "runId")
            && get(p, "tournamentId") == get(scope, "tournamentId")
            && id(get(p, "marketId"))
            && id(get(p, "exchangeId"))
            && id(get(p, "orderId"))
            && get(p, "intentId").is_string()
            && get(p, "fills").as_array().is_some_and(|f| f.len() == 1),
        "PROOF_SCOPE",
    )?;
    let l = get(p, "lot");
    let f = &get(p, "fills")[0];
    need(
        get(l, "side").as_str() == Some("no")
            && get(l, "lotId").as_str().is_some_and(|s| !s.is_empty())
            && get(l, "quantity") == &json!(1)
            && get(l, "ownerId") == get(p, "ownerId")
            && get(l, "runId") == get(p, "runId")
            && get(l, "orderId") == get(p, "orderId")
            && get(l, "intentId") == get(p, "intentId")
            && get(f, "marketId") == get(p, "marketId")
            && get(f, "exchangeId") == get(p, "exchangeId")
            && get(f, "orderId") == get(p, "orderId")
            && get(f, "tournamentId") == get(scope, "tournamentId")
            && get(f, "side").as_str() == Some("no")
            && get(f, "signedQuantity") == &json!(-1)
            && get(f, "quantity") == &json!(1)
            && get(f, "priceBasis").as_str() == Some("side_relative")
            && get(f, "source").as_str() == Some("rest_order_fills")
            && get(f, "price") == get(p, "scopedPrincipalCost")
            && get(l, "entryPrice") == get(p, "scopedPrincipalCost")
            && get(l, "openedAt") == get(f, "filledAt"),
        "LOT_PROVENANCE",
    )?;
    price(get(p, "scopedPrincipalCost").as_str().unwrap_or(""))?;
    instant_v(get(p, "readAt"))?;
    instant_v(get(f, "filledAt"))?;
    need(instant_v(get(f, "filledAt"))? <= instant_v(get(p, "readAt"))?, "PROOF_CLOCK")?;
    Ok(json!({
        "marketId": get(p, "marketId"),
        "exchangeId": get(p, "exchangeId"),
        "lotId": get(l, "lotId"),
        "intentId": get(p, "intentId"),
        "orderId": get(p, "orderId"),
        "quantity": 1,
        "entryPrice": get(p, "scopedPrincipalCost"),
        "proofHash": verification_hash,
    }))
}

// quote(r,lot,scope,now)
fn quote(r: &V, lot: &V, scope: &V, now: i64) -> Result<V, String> {
    let endpoint = format!(
        "/markets/{}/orderbook?tournamentId={}&depth=20",
        get(lot, "marketId").as_str().unwrap_or(""),
        get(scope, "tournamentId").as_str().unwrap_or("")
    );
    let body_text = get(r, "bodyText").as_str().unwrap_or("").to_string();
    need(
        get(r, "method").as_str() == Some("GET")
            && get(r, "endpoint").as_str() == Some(endpoint.as_str())
            && get(r, "status") == &json!(200)
            && get(r, "bodyText").as_str().is_some_and(|s| s.len() <= 8 * 1024 * 1024)
            && get(r, "sourceBodyHash").as_str() == Some(sha256(body_text.as_bytes()).as_str()),
        "RAW_BOOK",
    )?;
    let start = instant_v(get(r, "requestStartedAt"))?;
    let end = instant_v(get(r, "receivedAt"))?;
    need(
        start <= end && end <= now && now - start <= 20_000,
        "BOOK_CLOCK",
    )?;
    let (data, numbers) = parse_number_lexemes(&body_text)?;
    let b = normalize_exit_book(&data, &numbers, lot, scope)?;
    need(
        price(get(&get(&b, "bids")[0], "price").as_str().unwrap_or(""))?
            < price(get(&get(&b, "asks")[0], "price").as_str().unwrap_or(""))?,
        "BOOK_DEPTH_OR_CROSS",
    )?;
    need(
        instant_ns(get(&b, "sourceAt").as_str().unwrap_or("")).map_err(|e| e.0)?
            <= (end as i128 + 2_000) * 1_000_000,
        "FUTURE_BOOK_VERSION",
    )?;
    // One share needs >=100 shares at ONE level. Never sum shallow levels into
    // fictitious top-of-book liquidity or use YES bids to value a NO sale.
    let level = get(&b, "asks").as_array().unwrap().iter().find(|l| {
        level_num(l) >= BigInt::from(100) * level_den(l)
    }).cloned();
    if level.is_none() {
        return Ok(json!({
            "admissible": false,
            "reason": "one_percent_depth",
            "start": start,
            "end": end,
            "sourceBodyHash": get(r, "sourceBodyHash"),
        }));
    }
    let level = level.unwrap();
    let no_bid = SCALE - price(get(&level, "price").as_str().unwrap_or(""))?;
    let limit = no_bid - TICK;
    if limit < TICK {
        return Ok(json!({
            "admissible": false,
            "reason": "protected_limit_below_tick",
            "start": start,
            "end": end,
            "sourceBodyHash": get(r, "sourceBodyHash"),
        }));
    }
    let depth = level_num(&level) / (BigInt::from(100) * level_den(&level));
    let max_safe = BigInt::from(9_007_199_254_740_991_i64);
    let depth = if depth > max_safe { max_safe } else { depth };
    Ok(json!({
        "admissible": true,
        "marketId": get(lot, "marketId"),
        "exchangeId": get(lot, "exchangeId"),
        "rawNoBid": money(no_bid),
        "protectedSellLimit": money(limit),
        "eligibleDepth": depth.to_string().parse::<u64>().unwrap_or(u64::MAX),
        "sourceQuantityLexeme": get(&level, "quantityLexeme"),
        "requestStartedAt": get(r, "requestStartedAt"),
        "receivedAt": get(r, "receivedAt"),
        "sourceAt": get(&b, "sourceAt"),
        "sequence": get(&b, "sequence"),
        "sourceBodyHash": get(r, "sourceBodyHash"),
        "start": start,
        "end": end,
    }))
}

// screenPairedNoLiquidation({scope,review,buyProofs,books,readAt})
pub fn screen_paired_no_liquidation(input: &V) -> Result<V, String> {
    let scope = get(input, "scope");
    let review = get(input, "review");
    let buy_proofs = get(input, "buyProofs");
    let books = get(input, "books");
    let read_at = get(input, "readAt");
    let n = instant_v(read_at)?;
    need(
        get(scope, "ownerId") == &json!("Codex")
            && get(scope, "runId").as_str().is_some_and(|s| !s.is_empty())
            && get(scope, "tournamentId").is_string(),
        "SCOPE",
    )?;
    crate::dual_no_review_v1::validate_dual_no_review(review)?;
    need(
        get(review, "tournamentId") == get(scope, "tournamentId")
            && buy_proofs.as_array().is_some_and(|a| a.len() == 2)
            && books.as_array().is_some_and(|a| a.len() == 2),
        "PAIR",
    )?;
    let lots: Vec<V> = buy_proofs
        .as_array()
        .unwrap()
        .iter()
        .map(|p| proof_lot(p, scope))
        .collect::<Result<_, _>>()?;
    need(
        lots.iter().map(|l| get(l, "exchangeId").as_str().unwrap_or("")).collect::<BTreeSet<_>>().len() == 2
            && lots.iter().map(|l| get(l, "marketId").as_str().unwrap_or("")).collect::<BTreeSet<_>>().len() == 2
            && lots.iter().map(|l| get(l, "orderId").as_str().unwrap_or("")).collect::<BTreeSet<_>>().len() == 2,
        "DISTINCT_LOTS",
    )?;
    need(
        lots.iter().all(|l| {
            get(review, "legs").as_array().is_some_and(|legs| {
                legs.iter().any(|r| {
                    get(r, "marketId") == get(l, "marketId") && get(r, "exchangeId") == get(l, "exchangeId")
                })
            })
        }),
        "RULES_SCOPE",
    )?;
    need(
        buy_proofs.as_array().unwrap().iter().all(|p| instant_v(get(p, "readAt")).is_ok_and(|x| x <= n)),
        "FUTURE_OWNERSHIP",
    )?;
    let quotes: Vec<V> = lots
        .iter()
        .enumerate()
        .map(|(i, l)| quote(&books.as_array().unwrap()[i], l, scope, n))
        .collect::<Result<_, _>>()?;
    need(
        quotes.iter().map(|q| js_int_safe(get(q, "end")).unwrap()).max().unwrap()
            - quotes.iter().map(|q| js_int_safe(get(q, "start")).unwrap()).min().unwrap()
            <= 10_000,
        "BOOK_SKEW",
    )?;
    need(
        quotes.iter().all(|q| {
            js_int_safe(get(q, "start")).unwrap_or(0)
                >= buy_proofs
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| instant_v(get(p, "readAt")).unwrap_or(0))
                    .max()
                    .unwrap()
        }),
        "BOOK_BEFORE_OWNERSHIP",
    )?;
    need(
        quotes.iter().all(|q| {
            js_int_safe(get(q, "start")).unwrap_or(0) >= instant_v(get(review, "recordedAt")).unwrap_or(0)
        }),
        "RULES_BEFORE_BOOK",
    )?;
    let cost: i128 = lots.iter().map(|l| amount_v(get(l, "entryPrice"))).collect::<Result<Vec<_>, _>>()?.iter().sum();
    let base = json!({
        "schemaVersion": 1,
        "mode": "paired_no_liquidation_screen_v1",
        "readAt": read_at,
        "ownedPrincipal": money(cost),
        "proofHashes": lots.iter().map(|l| get(l, "proofHash").clone()).collect::<Vec<V>>(),
        "bookSourceHashes": quotes.iter().map(|q| get(q, "sourceBodyHash").clone()).collect::<Vec<V>>(),
        "currentSignedHoldingsVerified": false,
        "requiresFreshTwoSignedNoHoldings": true,
        "executionAuthority": false,
        "newSubmissionAuthorized": false,
        "profitabilityEstablished": false,
    });
    let missing = quotes.iter().find(|q| get(q, "admissible") != &json!(true));
    if let Some(m) = missing {
        let mut out = base.as_object().unwrap().clone();
        out.insert("signal".into(), json!(false));
        out.insert("reason".into(), get(m, "reason").clone());
        return Ok(V::Object(out));
    }
    let proceeds: i128 = quotes.iter().map(|q| amount_v(get(q, "protectedSellLimit"))).collect::<Result<Vec<_>, _>>()?.iter().sum();
    if proceeds < cost + TICK {
        let mut out = base.as_object().unwrap().clone();
        out.insert("signal".into(), json!(false));
        out.insert("reason".into(), json!("minimum_pair_net_gain"));
        out.insert("protectedProceeds".into(), json!(money(proceeds)));
        out.insert("conditionalFullFillGain".into(), json!(money(proceeds - cost)));
        return Ok(V::Object(out));
    }
    // legs: {...lot, ...quote minus admissible/start/end}
    let mut legs: Vec<V> = lots
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let mut m = l.as_object().cloned().unwrap_or_default();
            for (k, v) in quotes[i].as_object().unwrap() {
                if !["admissible", "start", "end"].contains(&k.as_str()) {
                    m.insert(k.clone(), v.clone());
                }
            }
            V::Object(m)
        })
        .collect();
    legs.sort_by(|a, b| {
        let pa = amount_v(get(a, "protectedSellLimit")).unwrap_or(0);
        let pb = amount_v(get(b, "protectedSellLimit")).unwrap_or(0);
        if pa == pb {
            let ea = get(a, "exchangeId").as_str().unwrap_or("").parse::<i64>().unwrap_or(0);
            let eb = get(b, "exchangeId").as_str().unwrap_or("").parse::<i64>().unwrap_or(0);
            ea.cmp(&eb)
        } else {
            pb.cmp(&pa)
        }
    });
    let mut plan_body = Map::from_iter([
        ("schemaVersion".into(), json!(1)),
        ("mode".into(), json!("paired_no_liquidation_plan_v1")),
        ("scope".into(), scope.clone()),
        ("policy".into(), paired_no_liquidation_policy()),
        ("createdAt".into(), read_at.clone()),
        ("expiresAt".into(), json!(baal_service_runtime::contract::ms_to_iso((n + 30_000) as f64))),
        ("reviewHash".into(), json!(payload_hash(review)?)),
        ("originalProofHashes".into(), lots.iter().map(|l| get(l, "proofHash").clone()).collect::<Vec<V>>().into()),
        ("ownedPrincipal".into(), json!(money(cost))),
        ("protectedProceeds".into(), json!(money(proceeds))),
        ("conditionalFullFillGain".into(), json!(money(proceeds - cost))),
        ("legs".into(), V::Array(legs)),
        ("nonAtomic".into(), json!(true)),
        ("orphanPossible".into(), json!(true)),
        ("globalCashReconciledExactly".into(), json!(false)),
        ("executionAuthority".into(), json!(false)),
        ("newSubmissionAuthorized".into(), json!(false)),
    ]);
    let plan_hash = payload_hash(&V::Object(plan_body.clone()))?;
    plan_body.insert("planHash".into(), json!(plan_hash));
    let plan = V::Object(plan_body);
    let mut out = base.as_object().unwrap().clone();
    out.insert("signal".into(), json!(true));
    out.insert("reason".into(), json!("protected_pair_gain"));
    out.insert("plan".into(), plan.clone());
    out.insert(
        "scenarios".into(),
        liquidation_scenarios(&json!({"plan": plan, "review": review}))?,
    );
    Ok(V::Object(out))
}

// liquidationScenarios({plan,review})
pub fn liquidation_scenarios(input: &V) -> Result<V, String> {
    let plan = get(input, "plan");
    let review = get(input, "review");
    let mut body = plan.clone();
    let plan_hash = body.as_object_mut().and_then(|m| m.remove("planHash"));
    need(
        plan_hash.as_ref().is_some_and(|h| hex(h))
            && payload_hash(&body)? == plan_hash.as_ref().and_then(|h| h.as_str()).unwrap_or("")
            && get(plan, "mode") == &json!("paired_no_liquidation_plan_v1")
            && get(plan, "legs").as_array().is_some_and(|l| l.len() == 2),
        "PLAN_HASH",
    )?;
    crate::dual_no_review_v1::validate_dual_no_review(review)?;
    need(
        payload_hash(review)? == get(plan, "reviewHash").as_str().unwrap_or(""),
        "REVIEW_BINDING",
    )?;
    let c = amount_v(get(plan, "ownedPrincipal"))?;
    let legs = get(plan, "legs").as_array().unwrap();
    let source_index = |l: &V| -> Option<usize> {
        get(review, "legs").as_array()?.iter().position(|r| {
            get(r, "exchangeId") == get(l, "exchangeId") && get(r, "marketId") == get(l, "marketId")
        })
    };
    need(legs.iter().all(|l| source_index(l).is_some()), "SCENARIO_SCOPE")?;
    let mut scenarios = Vec::new();
    for closed in 0..=2usize {
        let cash: i128 = legs[..closed.min(legs.len())]
            .iter()
            .map(|l| amount_v(get(l, "protectedSellLimit")))
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .sum();
        for s in get(review, "scenarios").as_array().cloned().unwrap_or_default().iter().filter(|s| {
            get(s, "category") == &json!("ordinary")
        }) {
            let payout: i128 = legs[closed.min(legs.len())..]
                .iter()
                .map(|l| {
                    let idx = source_index(l).unwrap();
                    js_int_safe(&get(s, "payouts")[idx]).unwrap_or(0) as i128 * SCALE
                })
                .sum();
            let outcome = if s.get("name").is_some_and(|v| !v.is_null()) {
                get(s, "name").clone()
            } else if s.get("id").is_some_and(|v| !v.is_null()) {
                get(s, "id").clone()
            } else {
                json!(get(s, "payouts").as_array().cloned().unwrap_or_default().iter().map(|p| match p { V::String(x) => x.clone(), v => v.to_string() }).collect::<Vec<_>>().join("/"))
            };
            scenarios.push(json!({
                "closedLegs": closed,
                "category": "ordinary",
                "outcome": outcome,
                "cashFromSales": money(cash),
                "remainingConditionalPayout": money(payout),
                "conditionalNetPnl": money(cash + payout - c),
                "cashRealizedGain": if closed == 2 { json!(money(cash - c)) } else { V::Null },
                "actualFillsVerified": false,
            }));
        }
        scenarios.push(json!({
            "closedLegs": closed,
            "category": "administrative_or_void",
            "outcome": "unverified_zero_remaining_payout_stress",
            "cashFromSales": money(cash),
            "remainingConditionalPayout": V::Null,
            "zeroPayoutStressNet": money(cash - c),
            "cashRealizedGain": if closed == 2 { json!(money(cash - c)) } else { V::Null },
            "administrativePayoutVerified": false,
            "actualFillsVerified": false,
        }));
    }
    Ok(json!({
        "semantics": "counterfactual_protected_full_quantity_fill_then_conditional_settlement",
        "nonAtomic": true,
        "orphanPossible": true,
        "noGuaranteedRealizedGain": true,
        "rows": scenarios,
    }))
}

// pairedNoExitBudget(plan)
pub fn paired_no_exit_budget(plan: &V) -> Result<V, String> {
    let mut body = plan.clone();
    let plan_hash = body.as_object_mut().and_then(|m| m.remove("planHash"));
    need(
        plan_hash.as_ref().is_some_and(|h| hex(h))
            && payload_hash(&body)? == plan_hash.as_ref().and_then(|h| h.as_str()).unwrap_or("")
            && get(plan, "mode") == &json!("paired_no_liquidation_plan_v1"),
        "PLAN_HASH",
    )?;
    let exit_id = format!(
        "paired-no-exit:{}",
        plan_hash.as_ref().and_then(|h| h.as_str()).unwrap_or("")
    );
    Ok(json!({
        "schemaVersion": 1,
        "planHash": plan_hash,
        "exitId": exit_id,
        "reservedOwnedLots": get(plan, "legs").as_array().unwrap_or(&vec![]).iter().map(|l| json!({
            "lotId": get(l, "lotId"),
            "marketId": get(l, "marketId"),
            "exchangeId": get(l, "exchangeId"),
            "quantity": 1,
            "proofHash": get(l, "proofHash"),
        })).collect::<Vec<V>>(),
        "maximumSellQuantity": 2,
        "minimumCombinedProceeds": money(amount_v(get(plan, "ownedPrincipal"))? + TICK),
        "protectiveLimitProceeds": get(plan, "protectedProceeds"),
        "expiresAt": get(plan, "expiresAt"),
        "unknownFirstAcceptanceBlocksSecond": true,
        "replaceUnknownForbidden": true,
        "requiresFreshTwoSignedNoHoldings": true,
        "requiresFreshExactSignedNoSellReconciliation": true,
        "unbackedSellForbidden": true,
        "executionAuthority": false,
    }))
}
