// execution/baal-paired-liquidity-sweep-v1.mjs — price/depth mathematics for a
// paired-party-winner depth sweep. This is not an ownership, collateral or fill
// proof: the execution controller must independently verify the contract
// predicate, fresh account, funding, exact request and subsequent fills of
// BOTH legs.
//
// Port notes:
// - JS BigInt arithmetic -> num_bigint::BigInt (cost/maxima accumulations).
// - The JS regexes (decimalRational, id, hex, whole-depth lexeme) are written
//   out as manual parsers; JS String(number) -> js_stringify.
use crate::core::payload_hash;
use crate::gates::normalize_official_fill_tick;
use crate::receipts::parse_number_lexemes;
use baal_service_runtime::fixed::{amount, instant, price, money, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("PAIRED_SWEEP_{code}"))
    }
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
        !s.is_empty()
            && !s.starts_with('0')
            && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<u64>().is_ok_and(|n| n <= 9_007_199_254_740_991)
    })
}

#[derive(Clone, PartialEq, Eq)]
struct Rational {
    numerator: BigInt,
    scale: i64,
}

// decimalRational(text): /^(0|[1-9]\d*)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$/
fn decimal_rational(text: &str) -> Result<Rational, String> {
    need(text.len() <= 128, "EXACT_PRICE_SOURCE")?;
    let b = text.as_bytes();
    let mut i = 0usize;
    let digits = |i: &mut usize| -> bool {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > start
    };
    // Integer part: 0 | [1-9]\d*
    if i >= b.len() {
        return Err(format!("PAIRED_SWEEP_EXACT_PRICE_SOURCE"));
    }
    if b[i] == b'0' {
        i += 1;
        need(i >= b.len() || !b[i].is_ascii_digit(), "EXACT_PRICE_SOURCE")?;
    } else {
        need(b[i].is_ascii_digit() && b[i] != b'0', "EXACT_PRICE_SOURCE")?;
        digits(&mut i);
    }
    let int_part = &text[..i];
    let mut fraction = "";
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let start = i;
        need(digits(&mut i), "EXACT_PRICE_SOURCE")?;
        fraction = &text[start..i];
    }
    let mut exponent: i64 = 0;
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        let neg = if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            let n = b[i] == b'-';
            i += 1;
            n
        } else {
            false
        };
        let start = i;
        need(digits(&mut i), "EXACT_PRICE_SOURCE")?;
        exponent = text[start..i].parse::<i64>().map_err(|_| "PAIRED_SWEEP_EXACT_PRICE_SOURCE".to_string())?;
        if neg {
            exponent = -exponent;
        }
    }
    need(i == b.len(), "EXACT_PRICE_SOURCE")?;
    need(exponent.abs() <= 128, "EXACT_PRICE_SOURCE")?;
    let mut numerator: BigInt = format!("{int_part}{fraction}")
        .parse()
        .map_err(|_| "PAIRED_SWEEP_EXACT_PRICE_SOURCE".to_string())?;
    let mut scale = fraction.len() as i64 - exponent;
    if scale < 0 {
        numerator *= BigInt::from(10u32).pow((-scale) as u32);
        scale = 0;
    }
    let ten = BigInt::from(10u32);
    while scale > 0 && &numerator % &ten == BigInt::from(0) {
        numerator /= &ten;
        scale -= 1;
    }
    Ok(Rational { numerator, scale })
}
fn same_rational(a: &Rational, b: &Rational) -> bool {
    a.scale == b.scale && a.numerator == b.numerator
}

// numbers map key: JSON.stringify(path array)
fn path_key(path: &[V]) -> String {
    js_stringify(&V::Array(path.to_vec()))
}
// String(value) — JS primitive-to-string for string/number values.
fn js_string_repr(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(_) | V::Bool(_) => js_stringify(v),
        _ => "undefined".to_string(),
    }
}

struct Curve {
    levels: Vec<CurveLevel>,
    total: i64,
    start: i64,
    end: i64,
    source_hash: String,
}
struct CurveLevel {
    no_ask: i128,
    quantity: i64,
}

// curve(leg, binding, at)
fn curve(leg: &V, binding: &V, at: i64) -> Result<Curve, String> {
    need(id(get(leg, "marketId")) && id(get(leg, "exchangeId")), "LEG_SCOPE")?;
    let r = leg.get("receipt").cloned().unwrap_or(V::Null);
    need(
        get(&r, "method").as_str() == Some("GET")
            && get(&r, "status") == &json!(200)
            && get(&r, "endpoint").as_str()
                == Some(format!(
                    "/markets/{}/orderbook?tournamentId={}&depth=20",
                    get(leg, "marketId").as_str().unwrap_or(""),
                    get(binding, "tournamentId").as_str().unwrap_or("")
                ).as_str()),
        "RECEIPT_SCOPE",
    )?;
    need(
        get(&r, "bodyText").is_string()
            && hex(get(&r, "sourceBodyHash"))
            && sha256(get(&r, "bodyText").as_str().unwrap_or("").as_bytes())
                == get(&r, "sourceBodyHash").as_str().unwrap_or(""),
        "BODY_HASH",
    )?;
    let start = instant(get(&r, "requestStartedAt").as_str().unwrap_or(""))?;
    let end = instant(get(&r, "receivedAt").as_str().unwrap_or(""))?;
    need(start <= end && end <= at && at - start <= 20000, "SOURCE_AGE")?;
    let (d, numbers) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap())?;
    need(
        d.get("marketId").is_none() || get(&d, "marketId") == get(leg, "marketId"),
        "MARKET",
    )?;
    let contexts = get(&d, "contexts").as_array().cloned().unwrap_or_default();
    need(
        contexts.len() == 1
            && get(&contexts[0], "type").as_str() == Some("tournament")
            && get(&get(&contexts[0], "tournament"), "id") == get(binding, "tournamentId"),
        "TOURNAMENT",
    )?;
    let rows = get(&get(&contexts[0], "orderbook"), "exchanges")
        .as_array()
        .cloned()
        .unwrap_or_default();
    need(
        rows.len() == 1
            && get(&rows[0], "exchangeId") == get(leg, "exchangeId")
            && get(&rows[0], "option").as_str() == Some("YES")
            && get(&rows[0], "bids").is_array()
            && get(&rows[0], "bids").as_array().unwrap().len() <= 20,
        "BINARY_BOOK",
    )?;
    let mut previous: i128 = SCALE;
    let mut total: i64 = 0;
    let mut levels = Vec::new();
    for (i, l) in get(&rows[0], "bids").as_array().unwrap().iter().enumerate() {
        let normalized = normalize_official_fill_tick(get(l, "price"))?;
        let yes = price(get(&normalized, "price").as_str().unwrap_or(""))?;
        let q = get(l, "quantity");
        let price_lexeme = numbers
            .get(&path_key(&[
                json!("contexts"), json!(0), json!("orderbook"), json!("exchanges"),
                json!(0), json!("bids"), json!(i), json!("price"),
            ]))
            .cloned()
            .unwrap_or_default();
        let rational = decimal_rational(&price_lexeme)?;
        // Permit exact official decimals and the enumerated actual IEEE
        // spelling, never arbitrary raw precision that Number happens to
        // round onto a tick.
        let exact_price = same_rational(
            &rational,
            &decimal_rational(get(&normalized, "price").as_str().unwrap_or(""))?,
        ) || same_rational(&rational, &decimal_rational(&js_string_repr(get(l, "price")))?);
        need(exact_price, "EXACT_PRICE_SOURCE")?;
        let lex = numbers
            .get(&path_key(&[
                json!("contexts"), json!(0), json!("orderbook"), json!("exchanges"),
                json!(0), json!("bids"), json!(i), json!("quantity"),
            ]))
            .cloned();
        need(yes >= TICK && yes < SCALE && yes < previous, "SORTED_TICKS")?;
        previous = yes;
        let qi = js_int(q);
        let lex_ok = lex.as_deref().is_some_and(|s| {
            // /^[1-9]\d*(\.0+)?$/
            let (int_part, frac) = match s.split_once('.') {
                Some((a, f)) => (a, Some(f)),
                None => (s, None),
            };
            !int_part.is_empty()
                && int_part.bytes().all(|b| b.is_ascii_digit())
                && !int_part.starts_with('0')
                && frac.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b == b'0'))
        });
        need(
            qi.is_some_and(|n| n > 0)
                && lex_ok
                && lex.as_deref()
                    .and_then(|s| s.split('.').next())
                    .and_then(|s| s.parse::<BigInt>().ok())
                    == qi.map(BigInt::from),
            "WHOLE_DEPTH",
        )?;
        total += qi.unwrap_or(0);
        need(
            total >= 0 && total <= 9_007_199_254_740_991,
            "DEPTH_BOUND",
        )?;
        levels.push(CurveLevel { no_ask: SCALE - yes, quantity: qi.unwrap_or(0) });
    }
    Ok(Curve {
        levels,
        total,
        start,
        end,
        source_hash: get(&r, "sourceBodyHash").as_str().unwrap_or("").to_string(),
    })
}

struct Consumed {
    cost: BigInt,
    limit: i128,
}
fn consume(c: &Curve, q: i64) -> Option<Consumed> {
    let mut remaining = q;
    let mut cost = BigInt::from(0);
    let mut limit: Option<i128> = None;
    for l in &c.levels {
        let n = std::cmp::min(l.quantity, remaining);
        cost += BigInt::from(l.no_ask) * BigInt::from(n);
        remaining -= n;
        if n != 0 {
            limit = Some(l.no_ask);
        }
        if remaining == 0 {
            break;
        }
    }
    if remaining != 0 {
        None
    } else {
        Some(Consumed { cost, limit: limit.unwrap_or(0) })
    }
}

// JS amount(x)/price(x) on a non-string throws; here a missing/non-string
// value parses "" which fails the same decimal check with an Err.
fn bm(s: &str) -> Result<BigInt, String> {
    amount(s).map(BigInt::from)
}
fn big_money(x: &BigInt) -> V {
    json!(baal_paper::fixed::money(x))
}
fn amount_of(v: &V) -> BigInt {
    v.as_str()
        .and_then(|s| amount(s).ok())
        .map(BigInt::from)
        .unwrap_or_else(|| BigInt::from(0))
}

/// planBaalPairedLiquiditySweep({binding, legs, availableNominalCapital,
/// maximumLegCost='25.000000', maximumEventCost='200.000000',
/// maximumQuantity=100, minimumMarginPerMatchedShare='0.010000', now})
pub fn plan_baal_paired_liquidity_sweep(args: &V) -> Result<V, String> {
    let binding = get(args, "binding");
    let legs = get(args, "legs");
    let available_nominal_capital = get(args, "availableNominalCapital");
    let maximum_leg_cost = args.get("maximumLegCost").cloned().unwrap_or(json!("25.000000"));
    let maximum_event_cost = args.get("maximumEventCost").cloned().unwrap_or(json!("200.000000"));
    let maximum_quantity = args.get("maximumQuantity").cloned().unwrap_or(json!(100));
    let minimum_margin = args
        .get("minimumMarginPerMatchedShare")
        .cloned()
        .unwrap_or(json!("0.010000"));
    let now = get(args, "now");
    need(
        get(binding, "ownerId").as_str() == Some("Codex")
            && get(binding, "runId").is_string()
            && get(binding, "tournamentId").is_string()
            && hex(get(binding, "reviewHash"))
            && hex(get(binding, "eventId")),
        "BINDING",
    )?;
    let legs_list = legs.as_array().cloned().unwrap_or_default();
    need(
        legs.is_array()
            && legs_list.len() == 2
            // Set(...).size===2 over two elements == the two values differ.
            && get(&legs_list[0], "marketId") != get(&legs_list[1], "marketId")
            && get(&legs_list[0], "exchangeId") != get(&legs_list[1], "exchangeId"),
        "DISTINCT_PAIR",
    )?;
    let maximum_quantity = js_int(&maximum_quantity);
    need(
        maximum_quantity.is_some_and(|q| q > 0 && q <= 100),
        "QUANTITY_CAP",
    )?;
    let maximum_quantity = maximum_quantity.unwrap_or(0);
    let at = instant(now.as_str().unwrap_or(""))?;
    let cash = bm(available_nominal_capital.as_str().unwrap_or("\u{0}"))?;
    let leg_cap = bm(maximum_leg_cost.as_str().unwrap_or("\u{0}"))?;
    let event_cap = bm(maximum_event_cost.as_str().unwrap_or("\u{0}"))?;
    let margin = BigInt::from(price(minimum_margin.as_str().unwrap_or("\u{0}"))?);
    need(
        cash >= BigInt::from(0)
            && cash <= BigInt::from(1000i64) * SCALE
            && leg_cap > BigInt::from(0)
            && leg_cap <= BigInt::from(25i64) * SCALE
            && event_cap > BigInt::from(0)
            && event_cap <= BigInt::from(200i64) * SCALE
            && margin >= BigInt::from(TICK),
        "NOMINAL_CAPS",
    )?;
    let curves: Vec<Curve> = legs_list
        .iter()
        .map(|l| curve(l, binding, at))
        .collect::<Result<_, _>>()?;
    need(
        curves.iter().map(|c| c.end).max().unwrap_or(0)
            - curves.iter().map(|c| c.start).min().unwrap_or(0)
            <= 10000,
        "SOURCE_SKEW",
    )?;
    let maximum = std::cmp::min(
        maximum_quantity,
        curves.iter().map(|c| c.total).min().unwrap_or(0),
    );
    let mut candidates: Vec<V> = Vec::new();
    for q in 1..=maximum {
        let consumed: Vec<Consumed> = match curves.iter().map(|c| consume(c, q)).collect::<Option<Vec<_>>>() {
            Some(v) => v,
            None => continue, // JS would deref null.limit -> TypeError; a missing level is not a candidate.
        };
        let maxima: Vec<BigInt> = consumed
            .iter()
            .map(|x| BigInt::from(x.limit) * BigInt::from(q))
            .collect();
        let total = &maxima[0] + &maxima[1];
        if maxima.iter().any(|x| *x > leg_cap) || total > cash || total > event_cap {
            continue;
        }
        let unit_margin = BigInt::from(SCALE) - consumed[0].limit - consumed[1].limit;
        if unit_margin < margin {
            continue;
        }
        candidates.push(json!({
            "quantity": q,
            "limits": consumed.iter().map(|x| json!(money(x.limit))).collect::<Vec<V>>(),
            "maximumLegCosts": maxima.iter().map(big_money).collect::<Vec<V>>(),
            "maximumNominalCost": big_money(&total),
            "displayedDepthNotional": big_money(&consumed.iter().fold(BigInt::from(0), |n, x| n + &x.cost)),
            "conditionalOrdinaryMarginLowerBound": big_money(&(&unit_margin * BigInt::from(q))),
            "conditionalOrdinaryPayout": big_money(&(BigInt::from(SCALE) * BigInt::from(q))),
            "unitMarginLowerBound": big_money(&unit_margin),
        }));
    }
    candidates.sort_by(|a, b| {
        let x = amount_of(get(a, "conditionalOrdinaryMarginLowerBound"));
        let y = amount_of(get(b, "conditionalOrdinaryMarginLowerBound"));
        if x != y {
            return y.cmp(&x);
        }
        let ac = amount_of(get(a, "maximumNominalCost"));
        let bc = amount_of(get(b, "maximumNominalCost"));
        if ac == bc {
            js_int(get(b, "quantity")).unwrap_or(0).cmp(&js_int(get(a, "quantity")).unwrap_or(0))
        } else {
            ac.cmp(&bc)
        }
    });
    // Exact historical control: one percent of each individual BEST rung only.
    let control_cap = std::cmp::min(
        maximum_quantity,
        curves
            .iter()
            .map(|c| c.levels.first().map(|l| l.quantity / 100).unwrap_or(0))
            .min()
            .unwrap_or(0),
    );
    let control = candidates
        .iter()
        .find(|c| {
            js_int(get(c, "quantity")).unwrap_or(0) <= control_cap
                && curves.iter().enumerate().all(|(i, x)| {
                    x.levels.first().is_some_and(|l0| {
                        get(c, "limits")
                            .as_array()
                            .and_then(|ls| ls.get(i))
                            .and_then(|l| l.as_str())
                            .and_then(|s| price(s).ok())
                            == Some(l0.no_ask)
                    })
                })
        })
        .cloned()
        .unwrap_or(V::Null);
    let mut result = json!({
        "schemaVersion": 1,
        "mode": "paired_price_bounded_depth_sweep_v1",
        "evaluatedAt": now,
        "binding": binding,
        "selected": candidates.first().cloned().unwrap_or(V::Null),
        "onePercentBestRungControl": control,
        "candidates": candidates,
        "marketIds": legs_list.iter().map(|l| get(l, "marketId").clone()).collect::<Vec<V>>(),
        "exchangeIds": legs_list.iter().map(|l| get(l, "exchangeId").clone()).collect::<Vec<V>>(),
        "sourceHashes": curves.iter().map(|c| json!(c.source_hash)).collect::<Vec<V>>(),
        "sourceStartedAt": legs_list.iter().map(|l| get(get(l, "receipt"), "requestStartedAt").clone()).collect::<Vec<V>>(),
        "conditionalFloorAssumption": "Two distinct mutually exclusive party-winner YES predicates for the same race; ordinary paired NO payout at least one per matched share.",
        "unvalidatedSemanticReview": true,
        "partialOrUnfilledLegsInvalidatePackageMargin": true,
        "administrativeOutcomesNotGuaranteed": true,
        "collateralSavingsAssumed": "0.000000",
        "fees": "0.000000",
        "executionAuthority": false,
        "orderEligible": false,
        "profitabilityEstablished": false,
        "actualFills": 0,
    });
    let hash = payload_hash(&result)?;
    result.as_object_mut().map(|m| m.insert("proposalHash".into(), json!(hash)));
    Ok(result)
}

/// rankDisjointBaalSweeps({plans, availableNominalCapital, maximumNewMarkets=20})
/// — a read/experiment ranking only. Actual matching and funding remain
/// serialized.
pub fn rank_disjoint_baal_sweeps(args: &V) -> Result<V, String> {
    let plans = get(args, "plans");
    let available_nominal_capital = get(args, "availableNominalCapital");
    let maximum_new_markets = args.get("maximumNewMarkets").cloned().unwrap_or(json!(20));
    need(
        plans.is_array()
            && js_int(&maximum_new_markets).is_some_and(|n| n >= 0 && n <= 20),
        "RANK_INPUT",
    )?;
    let maximum_new_markets = js_int(&maximum_new_markets).unwrap_or(0);
    let mut remaining = bm(available_nominal_capital.as_str().unwrap_or("\u{0}"))?;
    need(
        remaining >= BigInt::from(0) && remaining <= BigInt::from(1000i64) * SCALE,
        "RANK_CAPITAL",
    )?;
    let mut used_markets: BTreeSet<String> = BTreeSet::new();
    let mut used_exchanges: BTreeSet<String> = BTreeSet::new();
    let mut used_events: BTreeSet<String> = BTreeSet::new();
    let mut selected: Vec<V> = Vec::new();
    let mut ordered: Vec<V> = Vec::new();
    for p in plans.as_array().unwrap() {
        if get(p, "selected").is_null() {
            continue;
        }
        let mut body = p.clone();
        let proposal_hash = body
            .as_object_mut()
            .and_then(|m| m.shift_remove("proposalHash"))
            .unwrap_or(V::Null);
        need(
            payload_hash(&body)? == proposal_hash.as_str().unwrap_or("")
                && get(p, "executionAuthority") == &json!(false)
                && get(p, "orderEligible") == &json!(false),
            "RANK_SOURCE",
        )?;
        ordered.push(p.clone());
    }
    ordered.sort_by(|a, b| {
        let x = amount_of(get(get(a, "selected"), "conditionalOrdinaryMarginLowerBound"));
        let y = amount_of(get(get(b, "selected"), "conditionalOrdinaryMarginLowerBound"));
        if x == y {
            get(a, "proposalHash")
                .as_str()
                .unwrap_or("")
                .cmp(get(b, "proposalHash").as_str().unwrap_or(""))
        } else {
            y.cmp(&x)
        }
    });
    for p in &ordered {
        let market_ids: Vec<String> = get(p, "marketIds")
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let exchange_ids: Vec<String> = get(p, "exchangeIds")
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        if market_ids.iter().any(|m| used_markets.contains(m))
            || exchange_ids.iter().any(|e| used_exchanges.contains(e))
            || used_events.contains(get(get(p, "binding"), "eventId").as_str().unwrap_or(""))
            || used_markets.len() as i64 + 2 > maximum_new_markets
        {
            continue;
        }
        let choice = get(p, "candidates")
            .as_array()
            .and_then(|cs| {
                cs.iter()
                    .find(|c| amount_of(get(c, "maximumNominalCost")) <= remaining)
            })
            .cloned();
        let Some(choice) = choice else { continue };
        remaining -= amount_of(get(&choice, "maximumNominalCost"));
        for m in &market_ids {
            used_markets.insert(m.clone());
        }
        for e in &exchange_ids {
            used_exchanges.insert(e.clone());
        }
        used_events.insert(get(get(p, "binding"), "eventId").as_str().unwrap_or("").to_string());
        selected.push(json!({
            "proposalHash": get(p, "proposalHash"),
            "eventId": get(get(p, "binding"), "eventId"),
            "marketIds": get(p, "marketIds"),
            "exchangeIds": get(p, "exchangeIds"),
            "choice": choice,
        }));
    }
    Ok(json!({
        "selected": selected,
        "remainingNominalCapital": big_money(&remaining),
        "mode": "greedy_disjoint_dollar_margin_read_ranking",
        "globallyOptimal": false,
        "executionAuthority": false,
        "orderEligible": false,
    }))
}
