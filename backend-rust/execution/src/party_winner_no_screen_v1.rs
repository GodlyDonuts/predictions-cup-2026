// live-paper/party-winner-no-screen-v1.mjs — pure cached-source ranking: no
// clients, timers, publishers, fills or orders. Ported into baal-execution
// because baal-execution does not depend on baal-collector (the live-paper
// crate); the JS importers (owned-no-hedge-plan-v1.mjs) are execution modules.
// ledger.mjs payloadHash === core.mjs canonicalPayload+sha256 (sorted keys).
use crate::core::payload_hash;
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, js_truthy};
use serde_json::{json, Map, Value as V};
use std::collections::BTreeSet;

const FIELDS: [&str; 8] = [
    "raceId", "stageId", "usState", "raceName", "raceStage", "officeLevel", "contractType", "electionDate",
];
const QUALIFIERS_FULL: [&str; 13] = [
    "office", "officeType", "officeId", "officeName", "district", "districtId", "districtNumber",
    "senateClass", "seatId", "numberOfWinners", "seatCount", "raceType", "electionType",
];

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("PARTY_NO_SCREEN_{c}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn id(s: &V) -> bool {
    s.as_str().is_some_and(|s| {
        !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n > 0 && n <= 9_007_199_254_740_991)
    })
}
fn hex(s: &V) -> bool {
    s.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn amount_v(v: &V) -> Result<i128, String> {
    amount(v.as_str().unwrap_or(""))
}

// PARTY_NO_SCREEN_POLICY (frozen object).
pub fn party_no_screen_policy() -> V {
    json!({
        "quoteMaximumAgeMs": 20000,
        "maximumSkewMs": 10000,
        "rootMaximumAgeMs": 300000,
        "accountMaximumAgeMs": 20000,
        "minimumOrdinarySurplus": "0.010000",
        "protectiveTicksPerLeg": 1,
        "fees": "0.000000",
    })
}

// aliases: JS builds Map(i/200 -> i*TICK) for i in 1..200 (the three set() calls
// all produce the same key i/200), so exactPrice accepts the exact f64 grid.
fn exact_price(v: &V) -> Result<i128, String> {
    if let Some(s) = v.as_str() {
        return price(s);
    }
    if let Some(f) = v.as_f64() {
        for i in 1..200_i64 {
            if f == i as f64 / 200.0 {
                return Ok(i as i128 * TICK);
            }
        }
    }
    need(false, "OFF_TICK_QUOTE")?;
    unreachable!()
}

// partyWinnerQuoteHash(q)
pub fn party_winner_quote_hash(q: &V) -> Result<String, String> {
    let mut m = Map::new();
    for k in ["marketId", "exchangeId", "tournamentId", "option", "bestBid", "bestAsk", "requestStartedAt", "receivedAt", "knownAt"] {
        m.insert(k.to_string(), get(q, k).clone());
    }
    payload_hash(&V::Object(m))
}

#[derive(Clone)]
struct Descriptor {
    capture: V,
    key: String,
    party: String,
    known: i64,
    received: i64,
}

fn descriptor(c: &V, universe: &V, now: i64) -> Result<Descriptor, String> {
    let member: Vec<V> = get(universe, "exchanges")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|e| get(e, "marketId") == get(c, "marketId"))
        .collect();
    need(
        member.len() == 1
            && get(&member[0], "option") == &json!("YES")
            && get(&member[0], "exchangeId") == get(c, "exchangeId")
            && id(get(c, "marketId"))
            && id(get(c, "exchangeId"))
            && get(c, "tournamentId") == get(universe, "tournamentId"),
        "ROOT_SCOPE",
    )?;
    need(
        hex(get(c, "rootHash"))
            && hex(get(c, "sourceHash"))
            && payload_hash(get(c, "root"))? == get(c, "rootHash").as_str().unwrap_or(""),
        "ROOT_HASH",
    )?;
    let request = instant_v(get(c, "requestStartedAt"))?;
    let received = instant_v(get(c, "receivedAt"))?;
    let known = instant_v(get(c, "knownAt"))?;
    need(request <= received && received <= known && known <= now, "ROOT_CLOCK")?;
    let r = get(c, "root");
    let p = get(r, "contract_details");
    need(
        get(r, "node_type") == &json!("contract")
            && get(r, "contract_type") == &json!("Election Outcome")
            && get(r, "settled_with") == &V::Null
            && get(r, "node_id") == get(r, "contract_id")
            && id(get(r, "contract_id"))
            && !p.is_null()
            && FIELDS.iter().all(|k| get(p, k).as_str().is_some_and(|s| !s.is_empty())),
        "ROOT_PREDICATE",
    )?;
    need(
        get(p, "raceStage") == &json!("General")
            && get(p, "resolutionType") == &json!("Party Winner")
            && get(p, "contractType") == &json!("Election Outcome")
            && matches!(get(p, "winnerName").as_str(), Some("Democratic Party" | "Republican Party")),
        "SUPPORTED_PARTY_PREDICATE",
    )?;
    need(instant_v(get(r, "settlement_date"))? > now, "RESOLUTION_WINDOW_ENDED")?;
    need(
        ["numberOfWinners", "seatCount"].iter().all(|k| {
            let v = p.get(k);
            v.is_none() || v == Some(&json!(1)) || v == Some(&json!("1"))
        }),
        "MULTIPLE_WINNER_PREDICATE",
    )?;
    let mut key_body = Map::new();
    for k in FIELDS {
        key_body.insert(k.to_string(), get(p, k).clone());
    }
    key_body.insert("settlementDate".into(), get(r, "settlement_date").clone());
    let mut qualifiers = Map::new();
    for k in QUALIFIERS_FULL {
        qualifiers.insert(k.to_string(), p.get(k).cloned().unwrap_or(V::Null));
    }
    key_body.insert("qualifiers".into(), V::Object(qualifiers));
    Ok(Descriptor {
        capture: c.clone(),
        key: payload_hash(&V::Object(key_body))?,
        party: get(p, "winnerName").as_str().unwrap_or("").to_string(),
        known,
        received,
    })
}

fn semantic_review(a: &Descriptor, b: &Descriptor, universe: &V) -> Result<V, String> {
    need(
        get(get(&a.capture, "root"), "contract_id") != get(get(&b.capture, "root"), "contract_id"),
        "CONFLICTING_SHARED_CONTRACT_ID",
    )?;
    let recorded_at = baal_service_runtime::contract::ms_to_iso(std::cmp::max(a.known, b.known) as f64);
    let legs: Vec<V> = [a, b]
        .iter()
        .map(|x| {
            json!({
                "marketId": get(&x.capture, "marketId"),
                "exchangeId": get(&x.capture, "exchangeId"),
                "captureMarketId": get(&x.capture, "marketId"),
                "outcome": "NO",
                "rulesHash": get(&x.capture, "rootHash"),
                "root": get(&x.capture, "root"),
            })
        })
        .collect();
    let evidence_hash = payload_hash(&V::Array(vec![a.capture.clone(), b.capture.clone()]))?;
    let r = json!({
        "schemaVersion": 1,
        "kind": "conditional_no_pair_review",
        "id": format!("source-pair-{}-{}", get(&a.capture, "marketId").as_str().unwrap_or(""), get(&b.capture, "marketId").as_str().unwrap_or("")),
        "tournamentId": get(universe, "tournamentId"),
        "evidenceHash": evidence_hash,
        "recordedAt": recorded_at,
        "expiresAt": baal_service_runtime::contract::ms_to_iso(
            (instant(&recorded_at)? + 300_000) as f64,
        ),
        "normalResolutionOnly": true,
        "administrativeRefundGapAcknowledged": true,
        "legs": legs,
        "assumptions": [
            "Same structured election race/stage/date/office/district with opposing party winners.",
            "Ordinary mutual exclusion is conditional; separate contracts are not exhaustive complements.",
            "Administrative overrides, voids/refunds, affiliation disputes or inconsistent resolution can lose the full cost.",
        ],
        "scenarios": [
            {"id": "democratic", "category": "ordinary", "payouts": [0, 1]},
            {"id": "republican", "category": "ordinary", "payouts": [1, 0]},
            {"id": "other", "category": "ordinary", "payouts": [1, 1]},
            {"id": "exception", "category": "exception", "payouts": [0, 0]},
        ],
    });
    crate::dual_no_review_v1::validate_dual_no_review(&r)?;
    Ok(r)
}

// screenPartyWinnerNoPairs({universe,rootCaptures,quoteCaptures,account,now,policy})
pub fn screen_party_winner_no_pairs(input: &V) -> Result<V, String> {
    let universe = get(input, "universe");
    let root_captures = get(input, "rootCaptures");
    let quote_captures = get(input, "quoteCaptures");
    let account = get(input, "account");
    let now_v = get(input, "now");
    let default_policy = party_no_screen_policy();
    let policy = input.get("policy").unwrap_or(&default_policy);
    let at = instant_v(now_v)?;
    need(
        !universe.is_null()
            && get(universe, "exchanges").is_array()
            && root_captures.is_array()
            && quote_captures.is_array(),
        "INPUT",
    )?;
    need(payload_hash(policy)? == payload_hash(&party_no_screen_policy())?, "FIXED_POLICY")?;
    let quote_max_age = js_int(get(policy, "quoteMaximumAgeMs")).unwrap_or(20_000);
    let max_skew = js_int(get(policy, "maximumSkewMs")).unwrap_or(10_000);
    let root_max_age = js_int(get(policy, "rootMaximumAgeMs")).unwrap_or(300_000);
    let account_max_age = js_int(get(policy, "accountMaximumAgeMs")).unwrap_or(20_000);
    let minimum = amount_v(get(policy, "minimumOrdinarySurplus"))?;
    let protective_ticks_per_leg = js_int(get(policy, "protectiveTicksPerLeg")).unwrap_or(1) as i128;

    let mut roots: Vec<(String, Descriptor)> = Vec::new();
    let mut groups: Vec<(String, Vec<Descriptor>)> = Vec::new();
    let mut rejections: Vec<V> = Vec::new();
    let mut poisoned_roots: BTreeSet<String> = BTreeSet::new();
    for c in root_captures.as_array().unwrap() {
        let r: Result<(), String> = (|| {
            let cid = get(c, "marketId").as_str().unwrap_or("").to_string();
            if poisoned_roots.contains(&cid) {
                return Ok(());
            }
            let d = descriptor(c, universe, at)?;
            let old = roots.iter().find(|(k, _)| *k == cid).map(|(_, v)| v.clone());
            match old {
                None => roots.push((cid, d)),
                Some(o) => {
                    if d.known > o.known {
                        let pos = roots.iter().position(|(k, _)| *k == cid).unwrap();
                        roots[pos] = (cid, d);
                    } else if d.known == o.known {
                        need(
                            get(c, "rootHash") == get(&o.capture, "rootHash"),
                            "SAME_CLOCK_ROOT_CONFLICT",
                        )?;
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = r {
            let cid = get(c, "marketId").as_str().unwrap_or("").to_string();
            if e == "PARTY_NO_SCREEN_SAME_CLOCK_ROOT_CONFLICT" {
                poisoned_roots.insert(cid.clone());
                roots.retain(|(k, _)| *k != cid);
            }
            rejections.push(json!({
                "marketId": if js_truthy(c.get("marketId")) { get(c, "marketId").clone() } else { V::Null },
                "reason": e,
            }));
        }
    }
    for (_, d) in roots.iter() {
        let pos = groups.iter().position(|(k, _)| *k == d.key);
        match pos {
            Some(i) => groups[i].1.push(d.clone()),
            None => groups.push((d.key.clone(), vec![d.clone()])),
        }
    }

    let mut quotes: Vec<(String, V)> = Vec::new();
    let mut poisoned_quotes: BTreeSet<String> = BTreeSet::new();
    for q in quote_captures.as_array().unwrap() {
        let r: Result<(), String> = (|| {
            let qid = get(q, "exchangeId").as_str().unwrap_or("").to_string();
            if poisoned_quotes.contains(&qid) {
                return Ok(());
            }
            need(
                id(get(q, "marketId"))
                    && id(get(q, "exchangeId"))
                    && hex(get(q, "quoteHash"))
                    && party_winner_quote_hash(q)? == get(q, "quoteHash").as_str().unwrap_or(""),
                "QUOTE_BINDING",
            )?;
            instant_v(get(q, "knownAt"))?;
            let old = quotes.iter().find(|(k, _)| *k == qid).map(|(_, v)| v.clone());
            match old {
                None => quotes.push((qid, q.clone())),
                Some(o) => {
                    if instant_v(get(q, "knownAt"))? > instant_v(get(&o, "knownAt"))? {
                        let pos = quotes.iter().position(|(k, _)| *k == qid).unwrap();
                        quotes[pos] = (qid, q.clone());
                    } else if get(q, "knownAt") == get(&o, "knownAt") {
                        need(
                            get(q, "quoteHash") == get(&o, "quoteHash"),
                            "SAME_CLOCK_QUOTE_CONFLICT",
                        )?;
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = r {
            let qid = get(q, "exchangeId").as_str().unwrap_or("").to_string();
            if e == "PARTY_NO_SCREEN_SAME_CLOCK_QUOTE_CONFLICT" {
                poisoned_quotes.insert(qid.clone());
                quotes.retain(|(k, _)| *k != qid);
            }
            rejections.push(json!({
                "marketId": if js_truthy(q.get("marketId")) { get(q, "marketId").clone() } else { V::Null },
                "reason": e,
            }));
        }
    }

    let mut account_valid = false;
    let mut account_exposure_known = false;
    let mut excluded: BTreeSet<String> = BTreeSet::new();
    if let Some(a) = account.get("excludedExchangeIds").and_then(V::as_array) {
        excluded = a.iter().filter(|e| id(e)).map(|e| e.as_str().unwrap().to_string()).collect();
    }
    let account_gate: Result<(), String> = (|| {
        need(
            get(account, "complete") == &json!(true)
                && get(account, "tournamentId") == get(universe, "tournamentId")
                && hex(get(account, "sourceHash"))
                && get(account, "excludedExchangeIds").as_array().is_some_and(|a| a.iter().all(|e| id(e)))
                && get(account, "unknownAcceptance") == &json!(false),
            "ACCOUNT",
        )?;
        let known = instant_v(get(account, "availableAt"))?;
        need(
            known <= at
                && [get(account, "positionsAt"), get(account, "ordersAt")]
                    .iter()
                    .all(|t| instant_v(t).is_ok_and(|x| x <= known)),
            "ACCOUNT_CLOCK",
        )?;
        account_exposure_known = true;
        excluded = get(account, "excludedExchangeIds")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|e| e.as_str().unwrap_or("").to_string())
            .collect();
        need(
            [get(account, "positionsAt"), get(account, "ordersAt")]
                .iter()
                .all(|t| instant_v(t).is_ok_and(|x| at - x <= account_max_age)),
            "ACCOUNT_STALE",
        )?;
        account_valid = true;
        Ok(())
    })();
    let _ = account_gate;

    let mut pairs: Vec<V> = Vec::new();
    let mut groups_without_both = 0_i64;
    for (_, group) in groups.iter() {
        let d: Vec<&Descriptor> = group.iter().filter(|x| x.party == "Democratic Party").collect();
        let r: Vec<&Descriptor> = group.iter().filter(|x| x.party == "Republican Party").collect();
        if d.is_empty() || r.is_empty() {
            groups_without_both += 1;
            continue;
        }
        // Duplicate aliases of one party/event are not distinct opportunities.
        if d.len() != 1 || r.len() != 1 {
            rejections.push(json!({
                "reason": "PARTY_NO_SCREEN_AMBIGUOUS_EVENT_ALIASES",
                "marketIds": group.iter().map(|x| get(&x.capture, "marketId").clone()).collect::<Vec<V>>(),
            }));
            continue;
        }
        let legs: Vec<&Descriptor> = vec![d[0], r[0]];
        let review = match semantic_review(d[0], r[0], universe) {
            Ok(v) => v,
            Err(e) => {
                rejections.push(json!({
                    "reason": e,
                    "marketIds": legs.iter().map(|x| get(&x.capture, "marketId").clone()).collect::<Vec<V>>(),
                }));
                continue;
            }
        };
        let mut flags: Vec<V> = vec![
            json!("quantity_and_depth_unknown"),
            json!("ordinary_resolution_only_exception_unprotected"),
        ];
        let has_exposure = legs.iter().any(|l| {
            excluded.contains(get(&l.capture, "exchangeId").as_str().unwrap_or(""))
        });
        if has_exposure {
            flags.push(json!("whole_account_holding_or_open_order"));
        }
        if !account_valid {
            flags.push(json!("account_unavailable_stale_or_unknown_acceptance"));
        }
        let roots_current = legs.iter().all(|l| at - l.received <= root_max_age);
        if !roots_current {
            flags.push(json!("cached_rules_need_refresh"));
        }
        let mut prices: Vec<i128> = Vec::new();
        let mut starts: Vec<i64> = Vec::new();
        let mut receipts: Vec<i64> = Vec::new();
        let mut quote_hashes: Vec<V> = Vec::new();
        let mut quote_clocks: Vec<V> = Vec::new();
        let mut quotes_valid = true;
        let mut quotes_math_valid = true;
        for l in &legs {
            let r: Result<(), String> = (|| {
                let q = quotes
                    .iter()
                    .find(|(k, _)| *k == get(&l.capture, "exchangeId").as_str().unwrap_or(""))
                    .map(|(_, v)| v.clone())
                    .unwrap_or(V::Null);
                need(
                    !q.is_null()
                        && get(&q, "marketId") == get(&l.capture, "marketId")
                        && get(&q, "tournamentId") == get(universe, "tournamentId")
                        && get(&q, "option") == &json!("YES")
                        && hex(get(&q, "sourceHash")),
                    "QUOTE_SCOPE",
                )?;
                let start = instant_v(get(&q, "requestStartedAt"))?;
                let received = instant_v(get(&q, "receivedAt"))?;
                let known = instant_v(get(&q, "knownAt"))?;
                need(start <= received && received <= known && known <= at, "QUOTE_CLOCK")?;
                let bid = exact_price(get(&q, "bestBid"))?;
                let ask = exact_price(get(&q, "bestAsk"))?;
                need(bid < ask, "CROSSED_QUOTE")?;
                prices.push(SCALE - bid);
                starts.push(start);
                receipts.push(received);
                quote_hashes.push(get(&q, "sourceHash").clone());
                quote_clocks.push(json!({
                    "requestStartedAt": get(&q, "requestStartedAt"),
                    "receivedAt": get(&q, "receivedAt"),
                    "knownAt": get(&q, "knownAt"),
                }));
                if at - start > quote_max_age {
                    flags.push(json!("PARTY_NO_SCREEN_QUOTE_AGE"));
                    quotes_valid = false;
                }
                if l.known > start {
                    flags.push(json!("PARTY_NO_SCREEN_RULES_NOT_KNOWN_BEFORE_QUOTE"));
                    quotes_valid = false;
                }
                Ok(())
            })();
            if let Err(e) = r {
                flags.push(json!(e));
                quotes_valid = false;
                quotes_math_valid = false;
            }
        }
        let skew: Option<i64> = if quotes_math_valid {
            Some(receipts.iter().max().copied().unwrap_or(i64::MIN) - starts.iter().min().copied().unwrap_or(i64::MAX))
        } else {
            None
        };
        if quotes_math_valid && skew.unwrap_or(0) > max_skew {
            flags.push(json!("quote_packet_skew"));
            quotes_valid = false;
        }
        let account_causal = account_valid
            && quotes_valid
            && instant_v(get(account, "availableAt")).is_ok_and(|x| x <= starts.iter().min().copied().unwrap_or(i64::MAX));
        if !account_causal {
            flags.push(json!("account_not_known_before_both_quote_requests"));
        }
        let raw_cost: Option<i128> = if quotes_math_valid {
            Some(prices.iter().copied().fold(0, |a, b| a + b))
        } else {
            None
        };
        let raw_surplus: Option<i128> = raw_cost.map(|c| SCALE - c);
        let cushioned_cost: Option<i128> = raw_cost.map(|c| c + 2 * protective_ticks_per_leg * TICK);
        let raw_qualified = raw_surplus.is_some_and(|s| s >= minimum);
        let sufficient = cushioned_cost.is_some_and(|c| SCALE - c >= minimum);
        if !sufficient {
            flags.push(json!("minimum_protective_ordinary_surplus_not_met"));
        }
        let mut seen_flags: BTreeSet<String> = BTreeSet::new();
        let deduped: Vec<V> = flags
            .iter()
            .filter(|f| {
                let k = baal_service_runtime::json_text::js_stringify(f);
                if seen_flags.contains(&k) {
                    false
                } else {
                    seen_flags.insert(k);
                    true
                }
            })
            .cloned()
            .collect();
        pairs.push(json!({
            "id": get(&review, "id"),
            "race": get(get(&legs[0].capture, "root"), "contract_details").get("raceName").cloned().unwrap_or(V::Null),
            "eventKey": legs[0].key,
            "marketIds": legs.iter().map(|l| get(&l.capture, "marketId").clone()).collect::<Vec<V>>(),
            "exchangeIds": legs.iter().map(|l| get(&l.capture, "exchangeId").clone()).collect::<Vec<V>>(),
            "rootHashes": legs.iter().map(|l| get(&l.capture, "rootHash").clone()).collect::<Vec<V>>(),
            "rootSourceHashes": legs.iter().map(|l| get(&l.capture, "sourceHash").clone()).collect::<Vec<V>>(),
            "rootKnownAt": legs.iter().map(|l| get(&l.capture, "knownAt").clone()).collect::<Vec<V>>(),
            "quoteSourceHashes": quote_hashes,
            "quoteClocks": quote_clocks,
            "indicativeMathValid": quotes_math_valid,
            "rawNoAsks": if quotes_math_valid { V::Array(prices.iter().map(|p| json!(money(*p))).collect()) } else { V::Null },
            "rawNoCost": raw_cost.map(|c| json!(money(c))).unwrap_or(V::Null),
            "conditionalOrdinaryPayoutFloor": "1.000000",
            "conditionalRawSurplus": raw_surplus.map(|s| json!(money(s))).unwrap_or(V::Null),
            "protectiveTickCost": cushioned_cost.map(|c| json!(money(c))).unwrap_or(V::Null),
            "conditionalProtectiveSurplus": cushioned_cost.map(|c| json!(money(SCALE - c))).unwrap_or(V::Null),
            "zeroPayoutExceptionLoss": raw_cost.map(|c| json!(money(-c))).unwrap_or(V::Null),
            "zeroPayoutExceptionLossAtProtectiveCost": cushioned_cost.map(|c| json!(money(-c))).unwrap_or(V::Null),
            "quoteSkewMs": skew.map(|s| json!(s)).unwrap_or(V::Null),
            "rootsCurrent": roots_current,
            "quotesCurrentAndCofresh": quotes_valid,
            "accountCurrent": account_valid,
            "cachedAccountExposureKnown": account_exposure_known,
            "accountKnownBeforeBothRequests": account_causal,
            "wholeAccountExposure": has_exposure,
            "rawSurplusQualified": raw_qualified,
            "protectiveSurplusQualified": sufficient,
            "unownedReadCandidate": roots_current && !has_exposure && account_causal && sufficient,
            "unownedIndicativeCandidate": account_exposure_known && !has_exposure && sufficient,
            "quantityPerLeg": 1,
            "depthKnown": false,
            "depthQualifiedQuantity": V::Null,
            "nativeNoBookObserved": false,
            "ordinaryResolutionOnly": true,
            "fullResolutionCompletenessProven": false,
            "guaranteedProfit": false,
            "flags": deduped,
            "orderEligible": false,
            "executionAuthority": false,
        }));
    }
    // JS sort: unownedReadCandidate desc, rootsCurrent desc, rawNoCost asc
    // (nulls last), id asc (localeCompare).
    pairs.sort_by(|a, b| {
        let av = |x: &V, k: &str| js_truthy(x.get(k)) as i32;
        let cost = |x: &V| x.get("rawNoCost").and_then(|c| if c.is_null() { None } else { amount_v(c).ok() });
        av(b, "unownedReadCandidate")
            .cmp(&av(a, "unownedReadCandidate"))
            .then(av(b, "rootsCurrent").cmp(&av(a, "rootsCurrent")))
            .then(match (cost(a), cost(b)) {
                (None, _) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (Some(x), Some(y)) => x.cmp(&y),
            })
            .then(get(a, "id").as_str().unwrap_or("").cmp(get(b, "id").as_str().unwrap_or("")))
    });
    let mut indicative: Vec<&V> = pairs
        .iter()
        .filter(|p| get(p, "unownedIndicativeCandidate") == &json!(true))
        .collect();
    indicative.sort_by(|a, b| {
        let cost = |x: &V| amount_v(get(x, "protectiveTickCost")).unwrap_or(i128::MAX);
        cost(a).cmp(&cost(b)).then(get(a, "id").as_str().unwrap_or("").cmp(get(b, "id").as_str().unwrap_or("")))
    });
    Ok(json!({
        "schemaVersion": 1,
        "mode": "source_bound_party_winner_conditional_no_read_ranking",
        "evaluatedAt": now_v,
        "policy": policy,
        "rootCaptureCount": root_captures.as_array().unwrap().len(),
        "validLatestRoots": roots.len(),
        "semanticPairCount": pairs.len(),
        "groupsWithoutOpposingParty": groups_without_both,
        "rejections": rejections,
        "pairs": pairs,
        "ranking": pairs.iter().filter(|p| get(p, "unownedReadCandidate") == &json!(true)).map(|p| get(p, "id").clone()).collect::<Vec<V>>(),
        "indicativeRanking": indicative.iter().map(|p| get(p, "id").clone()).collect::<Vec<V>>(),
        "networkRequests": 0,
        "competitionOrders": 0,
        "executionAuthority": false,
        "profitabilityEstablished": false,
        "limitations": [
            "Opposing party roots establish only conditional ordinary-election mutual exclusion; they do not protect against inconsistent resolution/void/refund exceptions.",
            "Bulk YES bids are converted to same-exchange NO asks with exact decimals; there is no independently observed native NO book or depth.",
            "Read ranking never establishes executable quantities, atomic two-leg fills, guaranteed profit or authority to submit orders.",
            "ranking requires current rules/cofresh quotes/causal fresh account; indicativeRanking is only cached-cost read-refresh prioritization and can include stale rules or quotes. Neither ranking is order authority.",
            "Stale rules remain source-dated and flagged; fresh original metadata/account/depth receipts are required before any later economic test.",
        ],
    }))
}
