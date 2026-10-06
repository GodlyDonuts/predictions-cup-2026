// execution/baal-paired-liquidity-sweep-v1.mjs — displayed book depth sweep for
// a pair of mutually exclusive party-winner YES predicates. Price/depth
// mathematics only; never an ownership, collateral or fill proof.
use baal_execution::core::payload_hash;
use baal_execution::gates::normalize_official_fill_tick;
use baal_execution::receipts::parse_number_lexemes;
use baal_paper::fixed::{big_amount, big_price, instant, money, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::sync::OnceLock;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("PAIRED_SWEEP_{code}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V { v.get(k).unwrap_or(&V::Null) }
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn id(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        s.bytes().all(|b| b.is_ascii_digit())
            && !s.starts_with('0')
            && s.parse::<i64>().map(|n| n >= 1 && n <= 9_007_199_254_740_991).unwrap_or(false)
    })
}
fn sha(s: &str) -> String { sha256(s.as_bytes()) }
// JS Set identity: primitives dedup by value; separately parsed objects/arrays
// never share identity, so two deeply-equal objects still count as distinct.
fn js_set_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Object(_), _) | (V::Array(_), _) | (_, V::Object(_)) | (_, V::Array(_)) => false,
        _ => a == b,
    }
}
// String(x): the value itself for strings; JSON number spelling otherwise.
fn js_string_value(v: &V) -> String {
    v.as_str().map(String::from).unwrap_or_else(|| js_stringify(v))
}

// {numerator, scale} such that text == numerator / 10^scale, fully reduced of
// trailing decimal zeros — exactly decimalRational in the JS source.
fn decimal_rational(text: &str) -> Result<(BigInt, i64), String> {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"^(0|[1-9]\d*)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$").unwrap()
    });
    need(text.len() <= 128, "EXACT_PRICE_SOURCE")?;
    let m = re.captures(text).ok_or_else(|| "PAIRED_SWEEP_EXACT_PRICE_SOURCE".to_string())?;
    let exponent: i64 = m
        .get(3)
        .map(|x| x.as_str())
        .unwrap_or("0")
        .parse()
        .map_err(|_| "PAIRED_SWEEP_EXACT_PRICE_SOURCE".to_string())?;
    need(exponent.abs() <= 128, "EXACT_PRICE_SOURCE")?;
    let fraction = m.get(2).map(|x| x.as_str()).unwrap_or("");
    let mut numerator: BigInt = format!("{}{}", m.get(1).map(|x| x.as_str()).unwrap_or(""), fraction)
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
    Ok((numerator, scale))
}
fn same_rational(a: &(BigInt, i64), b: &(BigInt, i64)) -> bool {
    a.1 == b.1 && a.0 == b.0
}

struct Level {
    no_ask: BigInt,
    quantity: i64,
}
struct Curve {
    levels: Vec<Level>,
    total: i64,
    start: i64,
    end: i64,
    source_hash: V,
}

// This is price/depth mathematics, not an ownership, collateral or fill proof.
// The execution controller must independently verify the contract predicate,
// fresh account, funding, exact request and subsequent fills of BOTH legs.
fn curve(leg: &V, binding: &V, at: i64) -> Result<Curve, String> {
    need(id(get(leg, "marketId")) && id(get(leg, "exchangeId")), "LEG_SCOPE")?;
    let r = get(leg, "receipt");
    need(
        get(r, "method") == &json!("GET")
            && js_int(get(r, "status")) == Some(200)
            && get(r, "endpoint")
                == &json!(format!(
                    "/markets/{}/orderbook?tournamentId={}&depth=20",
                    get(leg, "marketId").as_str().unwrap_or(""),
                    get(binding, "tournamentId").as_str().unwrap_or("")
                )),
        "RECEIPT_SCOPE",
    )?;
    let body_text = get(r, "bodyText").as_str();
    need(
        body_text.is_some()
            && hex(get(r, "sourceBodyHash"))
            && sha(body_text.unwrap_or("")) == get(r, "sourceBodyHash").as_str().unwrap_or(""),
        "BODY_HASH",
    )?;
    let body_text = body_text.unwrap_or("");
    let start = instant(get(r, "requestStartedAt").as_str().unwrap_or(""))?;
    let end = instant(get(r, "receivedAt").as_str().unwrap_or(""))?;
    need(start <= end && end <= at && at - start <= 20000, "SOURCE_AGE")?;
    let (d, numbers) = parse_number_lexemes(body_text)?;
    need(
        !(d.is_object() && d.get("marketId").is_some()) || get(&d, "marketId") == get(leg, "marketId"),
        "MARKET",
    )?;
    let contexts = get(&d, "contexts");
    need(
        contexts.as_array().is_some_and(|a| a.len() == 1)
            && get(&contexts[0], "type") == &json!("tournament")
            && get(get(&contexts[0], "tournament"), "id") == get(binding, "tournamentId"),
        "TOURNAMENT",
    )?;
    let rows = get(get(&contexts[0], "orderbook"), "exchanges");
    need(
        rows.as_array().is_some_and(|a| a.len() == 1)
            && get(&rows[0], "exchangeId") == get(leg, "exchangeId")
            && get(&rows[0], "option") == &json!("YES")
            && get(&rows[0], "bids").as_array().is_some_and(|b| b.len() <= 20),
        "BINARY_BOOK",
    )?;
    let mut previous = BigInt::from(SCALE);
    let mut total: i64 = 0;
    let mut levels = Vec::new();
    for (i, l) in get(&rows[0], "bids").as_array().cloned().unwrap_or_default().into_iter().enumerate() {
        let normalized = normalize_official_fill_tick(get(&l, "price"))?;
        let yes = big_price(get(&normalized, "price").as_str().unwrap_or(""))?;
        let q = js_int(get(&l, "quantity"));
        let price_key = js_stringify(&json!(["contexts", 0, "orderbook", "exchanges", 0, "bids", i, "price"]));
        let rational = decimal_rational(numbers.get(&price_key).map(String::as_str).unwrap_or(""))?;
        let exact_price =
            same_rational(&rational, &decimal_rational(&js_string_value(get(&normalized, "price")))?);
        // Permit exact official decimals and the enumerated actual IEEE spelling,
        // never arbitrary raw precision that Number happens to round onto a tick.
        need(
            exact_price || same_rational(&rational, &decimal_rational(&js_string_value(get(&l, "price")))?),
            "EXACT_PRICE_SOURCE",
        )?;
        let lex_key = js_stringify(&json!(["contexts", 0, "orderbook", "exchanges", 0, "bids", i, "quantity"]));
        let lex = numbers.get(&lex_key).map(String::as_str);
        need(
            yes >= BigInt::from(TICK) && yes < BigInt::from(SCALE) && yes < previous,
            "SORTED_TICKS",
        )?;
        previous = yes.clone();
        need(
            q.is_some_and(|n| n > 0)
                && lex.is_some_and(|s| {
                    let (w, f) = match s.split_once('.') {
                        Some((w, f)) => (w, Some(f)),
                        None => (s, None),
                    };
                    // /^[1-9]\d*(\.0+)?$/
                    !w.is_empty()
                        && !w.starts_with('0')
                        && w.bytes().all(|b| b.is_ascii_digit())
                        && f.map(|f| !f.is_empty() && f.bytes().all(|b| b == b'0')).unwrap_or(true)
                })
                && lex
                    .map(|s| s.split('.').next().unwrap_or("").parse::<BigInt>().ok())
                    == Some(q.map(BigInt::from)),
            "WHOLE_DEPTH",
        )?;
        let q = q.unwrap_or(0);
        total = total
            .checked_add(q)
            .filter(|t| *t <= 9_007_199_254_740_991)
            .ok_or_else(|| "PAIRED_SWEEP_DEPTH_BOUND".to_string())?;
        levels.push(Level { no_ask: BigInt::from(SCALE) - &yes, quantity: q });
    }
    Ok(Curve { levels, total, start, end, source_hash: get(r, "sourceBodyHash").clone() })
}

fn consume(c: &Curve, q: i64) -> Option<(BigInt, BigInt)> {
    let mut remaining = q;
    let mut cost = BigInt::from(0);
    let mut limit = BigInt::from(0);
    for l in &c.levels {
        let n = l.quantity.min(remaining);
        cost += &l.no_ask * BigInt::from(n);
        remaining -= n;
        if n != 0 {
            limit = l.no_ask.clone();
        }
        if remaining == 0 {
            break;
        }
    }
    if remaining != 0 { None } else { Some((cost, limit)) }
}

fn amount_of(v: &V) -> Result<BigInt, String> {
    big_amount(v.as_str().unwrap_or(""))
}

pub fn plan_baal_paired_liquidity_sweep(input: &V) -> Result<V, String> {
    let binding = get(input, "binding");
    let legs = get(input, "legs");
    let maximum_leg_cost = input.get("maximumLegCost").cloned().unwrap_or(json!("25.000000"));
    let maximum_event_cost = input.get("maximumEventCost").cloned().unwrap_or(json!("200.000000"));
    let maximum_quantity = input.get("maximumQuantity").cloned().unwrap_or(json!(100));
    let minimum_margin = input.get("minimumMarginPerMatchedShare").cloned().unwrap_or(json!("0.010000"));
    let now = get(input, "now");
    need(
        get(binding, "ownerId") == &json!("Codex")
            && get(binding, "runId").as_str().is_some()
            && get(binding, "tournamentId").as_str().is_some()
            && hex(get(binding, "reviewHash"))
            && hex(get(binding, "eventId")),
        "BINDING",
    )?;
    need(
        legs.as_array().is_some_and(|a| {
            a.len() == 2
                && !js_set_eq(get(&a[0], "marketId"), get(&a[1], "marketId"))
                && !js_set_eq(get(&a[0], "exchangeId"), get(&a[1], "exchangeId"))
        }),
        "DISTINCT_PAIR",
    )?;
    need(
        js_int(&maximum_quantity).is_some_and(|q| q > 0 && q <= 100),
        "QUANTITY_CAP",
    )?;
    let at = instant(now.as_str().unwrap_or(""))?;
    let cash = amount_of(get(input, "availableNominalCapital"))?;
    let leg_cap = amount_of(&maximum_leg_cost)?;
    let event_cap = amount_of(&maximum_event_cost)?;
    let margin = big_price(minimum_margin.as_str().unwrap_or(""))?;
    need(
        cash >= BigInt::from(0)
            && cash <= BigInt::from(1000) * BigInt::from(SCALE)
            && leg_cap > BigInt::from(0)
            && leg_cap <= BigInt::from(25) * BigInt::from(SCALE)
            && event_cap > BigInt::from(0)
            && event_cap <= BigInt::from(200) * BigInt::from(SCALE)
            && margin >= BigInt::from(TICK),
        "NOMINAL_CAPS",
    )?;
    let leg_list = legs.as_array().cloned().unwrap_or_default();
    let mut curves = Vec::new();
    for l in &leg_list {
        curves.push(curve(l, binding, at)?);
    }
    need(
        curves.iter().map(|c| c.end).max().unwrap_or(0) - curves.iter().map(|c| c.start).min().unwrap_or(0)
            <= 10000,
        "SOURCE_SKEW",
    )?;
    let maximum = js_int(&maximum_quantity)
        .unwrap_or(0)
        .min(curves.iter().map(|c| c.total).min().unwrap_or(0));
    let mut candidates: Vec<V> = Vec::new();
    for q in 1..=maximum {
        let consumed: Vec<(BigInt, BigInt)> = curves
            .iter()
            .map(|c| consume(c, q).ok_or_else(|| "PAIRED_SWEEP_DEPTH_BOUND".to_string()))
            .collect::<Result<_, _>>()?;
        let maxima: Vec<BigInt> = consumed.iter().map(|x| &x.1 * BigInt::from(q)).collect();
        let total = &maxima[0] + &maxima[1];
        if maxima.iter().any(|x| x > &leg_cap) || total > cash || total > event_cap {
            continue;
        }
        let unit_margin = BigInt::from(SCALE) - &consumed[0].1 - &consumed[1].1;
        if unit_margin < margin {
            continue;
        }
        candidates.push(json!({
            "quantity": q,
            "limits": consumed.iter().map(|x| money(&x.1)).collect::<Vec<String>>(),
            "maximumLegCosts": maxima.iter().map(|m| money(m)).collect::<Vec<String>>(),
            "maximumNominalCost": money(&total),
            "displayedDepthNotional": money(&consumed.iter().fold(BigInt::from(0), |n, x| n + &x.0)),
            "conditionalOrdinaryMarginLowerBound": money(&(&unit_margin * BigInt::from(q))),
            "conditionalOrdinaryPayout": money(&(BigInt::from(SCALE) * BigInt::from(q))),
            "unitMarginLowerBound": money(&unit_margin),
        }));
    }
    candidates.sort_by(|a, b| {
        let x = amount_of(get(a, "conditionalOrdinaryMarginLowerBound")).unwrap_or_default();
        let y = amount_of(get(b, "conditionalOrdinaryMarginLowerBound")).unwrap_or_default();
        if x != y {
            return y.cmp(&x);
        }
        let ac = amount_of(get(a, "maximumNominalCost")).unwrap_or_default();
        let bc = amount_of(get(b, "maximumNominalCost")).unwrap_or_default();
        if ac == bc {
            js_int(get(b, "quantity")).unwrap_or(0).cmp(&js_int(get(a, "quantity")).unwrap_or(0))
        } else {
            ac.cmp(&bc)
        }
    });
    // Exact historical control: one percent of each individual BEST rung only.
    let control_cap = curves
        .iter()
        .map(|c| c.levels.first().map(|l| l.quantity / 100).unwrap_or(0))
        .fold(js_int(&maximum_quantity).unwrap_or(0), i64::min);
    let control = candidates
        .iter()
        .find(|c| {
            js_int(get(c, "quantity")).is_some_and(|q| q <= control_cap)
                && curves.iter().enumerate().all(|(i, x)| {
                    let limit = get(c, "limits").as_array().and_then(|a| a.get(i));
                    big_price(limit.and_then(V::as_str).unwrap_or("")).ok().as_ref()
                        == x.levels.first().map(|l| &l.no_ask)
                })
        })
        .cloned()
        .unwrap_or(V::Null);
    let mut result = json!({
        "schemaVersion": 1,
        "mode": "paired_price_bounded_depth_sweep_v1",
        "evaluatedAt": now.clone(),
        "binding": binding.clone(),
        "selected": candidates.first().cloned().unwrap_or(V::Null),
        "onePercentBestRungControl": control,
        "candidates": candidates,
        "marketIds": leg_list.iter().map(|l| get(l, "marketId").clone()).collect::<Vec<V>>(),
        "exchangeIds": leg_list.iter().map(|l| get(l, "exchangeId").clone()).collect::<Vec<V>>(),
        "sourceHashes": curves.iter().map(|c| c.source_hash.clone()).collect::<Vec<V>>(),
        "sourceStartedAt": leg_list.iter().map(|l| get(get(l, "receipt"), "requestStartedAt").clone()).collect::<Vec<V>>(),
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
    let proposal_hash = payload_hash(&result)?;
    result.as_object_mut().map(|o| o.insert("proposalHash".into(), json!(proposal_hash)));
    Ok(result)
}

// A read/experiment ranking only. Actual matching and funding remain serialized.
pub fn rank_disjoint_baal_sweeps(input: &V) -> Result<V, String> {
    let plans = get(input, "plans");
    let maximum_new_markets = input.get("maximumNewMarkets").cloned().unwrap_or(json!(20));
    need(
        plans.is_array() && js_int(&maximum_new_markets).is_some_and(|m| m >= 0 && m <= 20),
        "RANK_INPUT",
    )?;
    let mut remaining = amount_of(get(input, "availableNominalCapital"))?;
    need(
        remaining >= BigInt::from(0) && remaining <= BigInt::from(1000) * BigInt::from(SCALE),
        "RANK_CAPITAL",
    )?;
    let mut ordered: Vec<V> = Vec::new();
    for p in plans.as_array().cloned().unwrap_or_default() {
        if !baal_service_runtime::json_text::js_truthy(p.get("selected")) {
            continue;
        }
        let mut body = p.clone();
        let proposal_hash = body.as_object_mut().and_then(|o| o.shift_remove("proposalHash")).and_then(|h| h.as_str().map(String::from));
        let body_hash = payload_hash(&body)?;
        need(
            Some(body_hash.as_str()) == proposal_hash.as_deref()
                && get(&p, "executionAuthority") == &json!(false)
                && get(&p, "orderEligible") == &json!(false),
            "RANK_SOURCE",
        )?;
        ordered.push(p);
    }
    ordered.sort_by(|a, b| {
        let x = amount_of(get(get(a, "selected"), "conditionalOrdinaryMarginLowerBound")).unwrap_or_default();
        let y = amount_of(get(get(b, "selected"), "conditionalOrdinaryMarginLowerBound")).unwrap_or_default();
        if x == y {
            get(a, "proposalHash").as_str().unwrap_or("").cmp(get(b, "proposalHash").as_str().unwrap_or(""))
        } else {
            y.cmp(&x)
        }
    });
    let max_new = js_int(&maximum_new_markets).unwrap_or(0);
    let mut used_markets: Vec<V> = Vec::new();
    let mut used_exchanges: Vec<V> = Vec::new();
    let mut used_events: Vec<V> = Vec::new();
    let mut selected: Vec<V> = Vec::new();
    for p in &ordered {
        let market_ids = get(p, "marketIds").as_array().cloned().unwrap_or_default();
        let exchange_ids = get(p, "exchangeIds").as_array().cloned().unwrap_or_default();
        if market_ids.iter().any(|m| used_markets.iter().any(|u| js_set_eq(u, m)))
            || exchange_ids.iter().any(|x| used_exchanges.iter().any(|u| js_set_eq(u, x)))
            || used_events.iter().any(|u| js_set_eq(u, get(get(p, "binding"), "eventId")))
            || used_markets.len() as i64 + 2 > max_new
        {
            continue;
        }
        let mut choice: Option<V> = None;
        for c in get(p, "candidates").as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            if amount_of(get(c, "maximumNominalCost"))? <= remaining {
                choice = Some(c.clone());
                break;
            }
        }
        let Some(choice) = choice else { continue };
        remaining -= amount_of(get(&choice, "maximumNominalCost"))?;
        for m in &market_ids {
            if !used_markets.iter().any(|u| js_set_eq(u, m)) {
                used_markets.push(m.clone());
            }
        }
        for x in &exchange_ids {
            if !used_exchanges.iter().any(|u| js_set_eq(u, x)) {
                used_exchanges.push(x.clone());
            }
        }
        let event_id = get(get(p, "binding"), "eventId").clone();
        if !used_events.iter().any(|u| js_set_eq(u, &event_id)) {
            used_events.push(event_id);
        }
        selected.push(json!({
            "proposalHash": get(p, "proposalHash").clone(),
            "eventId": get(get(p, "binding"), "eventId").clone(),
            "marketIds": get(p, "marketIds").clone(),
            "exchangeIds": get(p, "exchangeIds").clone(),
            "choice": choice,
        }));
    }
    Ok(json!({
        "selected": selected,
        "remainingNominalCapital": money(&remaining),
        "mode": "greedy_disjoint_dollar_margin_read_ranking",
        "globallyOptimal": false,
        "executionAuthority": false,
        "orderEligible": false,
    }))
}
