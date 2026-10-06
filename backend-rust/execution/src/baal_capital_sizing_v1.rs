// execution/baal-capital-sizing-v1.mjs — pure conservative proposal sizing.
// Allocation is not an activation/window.
use crate::core::payload_hash;
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Map, Value as V};

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("BAAL_CAPITAL_{c}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn hex(v: &V) -> bool {
    v.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn id(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        !s.is_empty()
            && !s.starts_with('0')
            && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n <= 9_007_199_254_740_991)
    })
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn amount_v(v: &V) -> Result<i128, String> {
    amount(v.as_str().unwrap_or(""))
}
fn price_v(v: &V) -> Result<i128, String> {
    price(v.as_str().unwrap_or(""))
}

pub fn baal_capital_ceilings() -> V {
    json!({
        "capital": "1000.000000",
        "maximumOrderCost": "25.000000",
        "maximumMarketCost": "100.000000",
        "maximumEventCost": "200.000000",
        "maximumSharesPerOrder": 100,
        "depthFractionBps": 100,
        "feesPerShare": "0.000000",
        "maximumReceiptAgeMs": 20000,
        "protectiveTicks": 1,
    })
}

pub fn size_baal_capital(input: &V) -> V {
    let allocation = get(input, "allocation");
    let scope = get(input, "scope");
    let account_risk = get(input, "accountRisk");
    let opportunity = get(input, "opportunity");
    let now_v = get(input, "now");
    let base = json!({
        "schemaVersion": 1,
        "mode": "baal_conservative_nominal_capital_sizing_v1",
        "at": now_v,
        "proposedQuantity": 0,
        "proposedCost": "0.000000",
        "orderEligible": false,
        "executionAuthority": false,
        "allocationCreatesWindow": false,
        "profitabilityEstablished": false,
        "priorSmallTradeProvesScale": false,
        "networkRequests": 0,
        "competitionOrders": 0,
    });
    let run = || -> Result<V, String> {
        let at = instant_v(now_v)?;
        let a = allocation;
        let g = account_risk;
        let o = opportunity;
        need(
            get(scope, "ownerId").as_str() == Some("Codex")
                && get(scope, "runId").as_str().is_some_and(|s| !s.is_empty())
                && get(scope, "tournamentId").as_str().is_some_and(|s| !s.is_empty()),
            "SCOPE",
        )?;
        need(
            js_int(get(a, "schemaVersion")) == Some(1)
                && get(a, "ownerId") == get(scope, "ownerId")
                && get(a, "runId") == get(scope, "runId")
                && get(a, "tournamentId") == get(scope, "tournamentId")
                && hex(get(a, "humanAllocationSourceHash"))
                && instant_v(get(a, "recordedAt"))? <= at
                && get(a, "capital").as_str() == Some("1000.000000"),
            "EXPLICIT_ALLOCATION",
        )?;
        let defaults = baal_capital_ceilings();
        let order_cap = amount_v(match a.get("maximumOrderCost") {
            Some(v) if !v.is_null() => v,
            _ => &get(&defaults, "maximumOrderCost"),
        })?;
        let market_cap = amount_v(match a.get("maximumMarketCost") {
            Some(v) if !v.is_null() => v,
            _ => &get(&defaults, "maximumMarketCost"),
        })?;
        let event_cap = amount_v(match a.get("maximumEventCost") {
            Some(v) if !v.is_null() => v,
            _ => &get(&defaults, "maximumEventCost"),
        })?;
        let share_cap = match a.get("maximumSharesPerOrder") {
            Some(v) if !v.is_null() => js_int(v).unwrap_or(i64::MIN),
            _ => 100,
        };
        need(
            order_cap > 0
                && order_cap <= 25_000_000
                && market_cap > 0
                && market_cap <= 100_000_000
                && event_cap > 0
                && event_cap <= 200_000_000
                && share_cap >= 1
                && share_cap <= 100,
            "CONFIGURED_CEILINGS",
        )?;
        need(
            get(g, "ownerId") == get(scope, "ownerId")
                && get(g, "runId") == get(scope, "runId")
                && get(g, "tournamentId") == get(scope, "tournamentId")
                && get(g, "complete") == &V::Bool(true)
                && get(g, "eventScopeComplete") == &V::Bool(true)
                && hex(get(g, "sourceHash"))
                && instant_v(get(g, "at"))? <= at
                && at - instant_v(get(g, "at"))? <= 20_000
                && get(g, "owned").is_array()
                && get(g, "reservations").is_array()
                && get(g, "loans").is_array(),
            "FRESH_COMPLETE_RISK",
        )?;
        let side = get(o, "side").as_str().unwrap_or("");
        need(
            id(get(o, "marketId"))
                && id(get(o, "exchangeId"))
                && get(o, "eventId").as_str().is_some_and(|s| !s.is_empty())
                && get(o, "action").as_str() == Some("buy")
                && ["yes", "no"].contains(&side)
                && get(o, "receiptKind").as_str() == Some("official_rest_depth20")
                && js_int(get(o, "depth")) == Some(20)
                && get(o, "priceBasis").as_str()
                    == Some(if side == "yes" {
                        "official_yes_ask"
                    } else {
                        "same_exchange_yes_bid_complement_for_no_buy"
                    })
                && hex(get(o, "sourceHash"))
                && hex(get(o, "valuationHash"))
                && ["fresh_protected_exit", "source_bound_settlement_estimate"]
                    .contains(&get(o, "valuationKind").as_str().unwrap_or(""))
                && instant_v(get(o, "valuationExpiresAt"))? > at,
            "OPPORTUNITY_SOURCE",
        )?;
        need(
            instant_v(get(o, "requestStartedAt"))? <= instant_v(get(o, "receivedAt"))?
                && instant_v(get(o, "receivedAt"))? <= instant_v(get(o, "availableAt"))?
                && instant_v(get(o, "availableAt"))? <= at
                && at - instant_v(get(o, "requestStartedAt"))? <= 20_000
                && instant_v(get(g, "at"))? <= instant_v(get(o, "requestStartedAt"))?,
            "CAUSAL_FRESH_RECEIPT",
        )?;
        let levels_in = get(o, "levels").as_array();
        need(
            get(o, "feesPerShare").as_str() == Some("0.000000")
                && js_int(get(o, "protectiveTicks")) == Some(1)
                && levels_in.is_some_and(|l| !l.is_empty() && l.len() <= 20),
            "EXACT_FEES_AND_DEPTH",
        )?;
        let levels_in = levels_in.unwrap();
        let value = amount_v(get(o, "protectedNetValuePerShare"))?;
        let edge = amount_v(get(o, "minimumNetEdgePerShare"))?;
        need(value >= 0 && value <= SCALE && edge >= TICK, "DECLARED_NET_EDGE")?;
        let mut nominal: i128 = 0;
        let mut market: i128 = 0;
        let mut event: i128 = 0;
        let mut unknown = false;
        let mut counts = Map::new();
        counts.insert("owned".into(), json!(0));
        counts.insert("reservations".into(), json!(0));
        counts.insert("loans".into(), json!(0));
        for (k, field) in [
            ("owned", "nominalCost"),
            ("reservations", "maximumNominalCost"),
            ("loans", "fullNominalPrincipal"),
        ] {
            for r in get(g, k).as_array().cloned().unwrap_or_default() {
                need(
                    id(get(&r, "marketId"))
                        && get(&r, "eventId").as_str().is_some_and(|s| !s.is_empty())
                        && get(&r, "exposureId").as_str().is_some_and(|s| !s.is_empty())
                        && (get(&r, "marketId") != get(o, "marketId")
                            || get(&r, "eventId") == get(o, "eventId")),
                    "EXPOSURE_SCOPE",
                )?;
                let n = amount_v(get(&r, field))?;
                need(n >= 0, "FULL_NOMINAL_EXPOSURE")?;
                nominal += n;
                if get(&r, "marketId") == get(o, "marketId") {
                    market += n;
                }
                if get(&r, "eventId") == get(o, "eventId") {
                    event += n;
                }
                let c = counts[k].as_i64().unwrap_or(0) + 1;
                counts.insert(k.into(), json!(c));
                unknown |= get(&r, "unknownAcceptance") == &V::Bool(true);
            }
        }
        let cash = amount_v(get(g, "cashLower"))?;
        need(cash >= 0, "CONSERVATIVE_CASH")?;
        let capital_remaining = 1_000_000_000i128 - nominal;
        let market_remaining = market_cap - market;
        let event_remaining = event_cap - event;
        let risk = json!({
            "allocationHash": payload_hash(a)?,
            "accountRiskHash": payload_hash(g)?,
            "nominalExposure": money(nominal),
            "marketNominalExposure": money(market),
            "eventNominalExposure": money(event),
            "capitalRemaining": money(capital_remaining),
            "marketRemaining": money(market_remaining),
            "eventRemaining": money(event_remaining),
            "cashLower": money(cash),
            "exposureCounts": V::Object(counts),
            "loansDiscounted": false,
            "collateralAdvanceIncreasesBudget": false,
            "unknownReservationsCounted": true,
        });
        let mut with_risk = |reason: &str, levels: Option<V>| -> V {
            let mut m = base.clone();
            m["risk"] = risk.clone();
            if let Some(l) = levels {
                m["levelDiagnostics"] = l;
            }
            m["reason"] = json!(reason);
            m
        };
        if get(g, "unknownAcceptance") != &V::Bool(false)
            || !get(g, "unresolvedIntentIds").is_array()
            || !get(g, "unresolvedIntentIds").as_array().unwrap().is_empty()
            || unknown
        {
            return Ok(with_risk("UNKNOWN_ACCEPTANCE_OR_UNRESOLVED_INTENT", None));
        }
        let funding = capital_remaining
            .min(market_remaining)
            .min(event_remaining)
            .min(order_cap)
            .min(cash);
        if funding <= 0 {
            return Ok(with_risk("NO_NOMINAL_OR_CASH_CAPACITY", None));
        }
        let mut prior: Option<i128> = None;
        let mut best: Option<V> = None;
        let mut levels: Vec<V> = Vec::new();
        for l in levels_in {
            let p = price_v(get(l, "price"))?;
            let q = js_int(get(l, "quantity"));
            need(
                q.is_some_and(|x| x > 0) && (prior.is_none() || p > prior.unwrap()),
                "INDIVIDUAL_EXECUTABLE_LEVELS",
            )?;
            prior = Some(p);
            let limit = p + TICK;
            if limit > SCALE - TICK {
                levels.push(json!({"price": get(l, "price"), "reason": "PROTECTION_EXCEEDS_PRICE_RANGE"}));
                continue;
            }
            let net = value - limit;
            let depth = q.unwrap() / 100;
            let funded = (share_cap as i128)
                .min(depth as i128)
                .min(funding / limit);
            if depth < 1 || net < edge || funded < 1 {
                levels.push(json!({
                    "price": get(l, "price"),
                    "depthQualifiedQuantity": depth,
                    "netEdgePerShare": money(net),
                    "reason": if depth < 1 {
                        "LESS_THAN_ONE_PERCENT_INTEGER_SHARE"
                    } else if net < edge {
                        "PROTECTED_NET_EDGE_INSUFFICIENT"
                    } else {
                        "FUNDED_QUANTITY_ZERO"
                    },
                }));
                continue;
            }
            let candidate = json!({
                "price": get(l, "price"),
                "protectedLimit": money(limit),
                "quantity": funded as i64,
                "cost": money(limit * funded),
                "netEdgePerShare": money(net),
                "declaredTotalNetEdge": money(net * funded),
                "levelQuantity": get(l, "quantity"),
                "depthQualifiedQuantity": depth,
            });
            let better = best
                .as_ref()
                .map(|b| js_int(get(&candidate, "quantity")).unwrap_or(0) > js_int(get(b, "quantity")).unwrap_or(0))
                .unwrap_or(true);
            levels.push(candidate.clone());
            if better {
                best = Some(candidate);
            }
        }
        if best.is_none() {
            return Ok(with_risk(
                "NO_FUNDED_DEPTH_AND_PROTECTED_NET_EDGE",
                Some(V::Array(levels)),
            ));
        }
        let best = best.unwrap();
        let mut body = base.clone();
        body["proposedQuantity"] = get(&best, "quantity").clone();
        body["proposedCost"] = get(&best, "cost").clone();
        body["reason"] = json!("CONSERVATIVE_FUNDED_PROPOSAL_ONLY");
        body["ownerId"] = get(scope, "ownerId").clone();
        body["runId"] = get(scope, "runId").clone();
        body["tournamentId"] = get(scope, "tournamentId").clone();
        body["marketId"] = get(o, "marketId").clone();
        body["exchangeId"] = get(o, "exchangeId").clone();
        body["eventId"] = get(o, "eventId").clone();
        body["side"] = get(o, "side").clone();
        body["action"] = get(o, "action").clone();
        body["protectedLimit"] = get(&best, "protectedLimit").clone();
        body["netEdgePerShare"] = get(&best, "netEdgePerShare").clone();
        body["declaredTotalNetEdge"] = get(&best, "declaredTotalNetEdge").clone();
        body["valuationKind"] = get(o, "valuationKind").clone();
        body["valuationHash"] = get(o, "valuationHash").clone();
        body["valuationExpiresAt"] = get(o, "valuationExpiresAt").clone();
        body["opportunityHash"] = json!(payload_hash(o)?);
        body["risk"] = risk;
        body["levelDiagnostics"] = V::Array(levels);
        body["chosenIndividualLevel"] = json!({
            "price": get(&best, "price"),
            "quantity": get(&best, "levelQuantity"),
            "onePercentQuantity": get(&best, "depthQualifiedQuantity"),
        });
        body["wholeIntegerShares"] = json!(true);
        body["depthCombinedAcrossLevels"] = json!(false);
        body["fullNominalDebit"] = get(&best, "cost").clone();
        body["fees"] = json!("0.000000");
        let mut out = body.clone();
        out["sizingHash"] = json!(payload_hash(&body)?);
        Ok(out)
    };
    match run() {
        Ok(v) => v,
        Err(e) => {
            let mut out = base;
            let code = e.as_str();
            out["reason"] = json!(if code.starts_with("BAAL_CAPITAL_")
                && code[13..]
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b == b'_')
                && code.len() > 13
            {
                code.to_string()
            } else {
                "INVALID_OR_INCOMPLETE_CAPITAL_EVIDENCE".to_string()
            });
            out
        }
    }
}
