// live-paper/party-winner-no-screen-v1.mjs — pure cached-source ranking: no
// clients, timers, publishers, fills or orders.
use baal_execution::core::payload_hash;
use baal_paper::fixed::{big_amount, big_price, instant, money, SCALE, TICK};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::{HashMap, HashSet};

/// PARTY_NO_SCREEN_POLICY (frozen object).
pub fn party_no_screen_policy() -> V {
    json!({"quoteMaximumAgeMs":20000,"maximumSkewMs":10000,"rootMaximumAgeMs":300000,
        "accountMaximumAgeMs":20000,"minimumOrdinarySurplus":"0.010000",
        "protectiveTicksPerLeg":1,"fees":"0.000000"})
}
const FIELDS: [&str; 9] = ["raceId","stageId","usState","raceName","raceStage","officeLevel",
    "contractType","electionDate","resolutionType"];
const QUALIFIERS: [&str; 13] = ["office","officeType","officeId","officeName","district",
    "districtId","districtNumber","senateClass","seatId","numberOfWinners","seatCount",
    "raceType","electionType"];
fn need(v: bool, c: &str) -> Result<(), String> {
    if !v { return Err(format!("PARTY_NO_SCREEN_{}", c)); } Ok(())
}
fn is_id(s: &V) -> bool {
    s.as_str().is_some_and(|x| !x.is_empty()
        && x.bytes().all(|b| b.is_ascii_digit()) && x.as_bytes()[0] != b'0'
        && js_int(&json!(x.parse::<f64>().unwrap_or(f64::NAN))).is_some())
}
fn is_hex(s: &V) -> bool {
    s.as_str().is_some_and(|x| x.len() == 64
        && x.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}
fn instant_s(v: &V) -> Result<i64, String> { instant(v.as_str().unwrap_or("")) }

// aliases: for i in 1..200, three float-key spellings all map to i*TICK.
fn alias_table() -> HashMap<u64, BigInt> {
    let mut m = HashMap::new();
    for i in 1..200i64 {
        for k in [
            i as f64 / 200.0,
            1.0 - ((200 - i) as f64) / 200.0,
            1.0 - (1.0 - (i as f64) / 200.0),
        ] {
            m.insert(k.to_bits(), BigInt::from(i) * BigInt::from(TICK));
        }
    }
    m
}
fn exact_price(v: &V) -> Result<BigInt, String> {
    if let Some(s) = v.as_str() { return big_price(s); }
    if let Some(x) = v.as_f64() {
        if let Some(p) = alias_table().get(&x.to_bits()) { return Ok(p.clone()); }
    }
    need(false, "OFF_TICK_QUOTE")?;
    unreachable!()
}

/// partyWinnerQuoteHash(q)
pub fn party_winner_quote_hash(q: &V) -> Result<String, String> {
    let mut o = serde_json::Map::new();
    for k in ["marketId","exchangeId","tournamentId","option","bestBid","bestAsk",
        "requestStartedAt","receivedAt","knownAt"] {
        o.insert(k.to_string(), q[k].clone());
    }
    payload_hash(&V::Object(o)).map_err(|e| e.to_string())
}

#[derive(Clone)]
struct Descriptor { capture: V, key: String, party: String, known: i64, received: i64 }

fn descriptor(c: &V, universe: &V, now: i64) -> Result<Descriptor, String> {
    let members: Vec<&V> = universe["exchanges"].as_array().map(|a| a.iter()
        .filter(|e| e["marketId"] == c["marketId"]).collect()).unwrap_or_default();
    need(members.len() == 1
        && members[0]["option"].as_str() == Some("YES")
        && members[0]["exchangeId"] == c["exchangeId"]
        && is_id(&c["marketId"])
        && is_id(&c["exchangeId"])
        && c["tournamentId"] == universe["tournamentId"], "ROOT_SCOPE")?;
    need(is_hex(&c["rootHash"]) && is_hex(&c["sourceHash"])
        && payload_hash(&c["root"]).map_err(|e| e.to_string())?
            == c["rootHash"].as_str().unwrap_or(""), "ROOT_HASH")?;
    let request = instant_s(&c["requestStartedAt"])?;
    let received = instant_s(&c["receivedAt"])?;
    let known = instant_s(&c["knownAt"])?;
    need(request <= received && received <= known && known <= now, "ROOT_CLOCK")?;
    let r = &c["root"];
    let p = &r["contract_details"];
    need(r["node_type"].as_str() == Some("contract")
        && r["contract_type"].as_str() == Some("Election Outcome")
        && r.get("settled_with").is_some_and(|v| v.is_null())
        && r["node_id"] == r["contract_id"]
        && is_id(&r["contract_id"])
        && js_truthy(r.get("contract_details"))
        && FIELDS.iter().all(|k| p[*k].as_str().is_some_and(|s| !s.is_empty())),
        "ROOT_PREDICATE")?;
    need(p["raceStage"].as_str() == Some("General")
        && p["resolutionType"].as_str() == Some("Party Winner")
        && p["contractType"].as_str() == Some("Election Outcome")
        && ["Democratic Party", "Republican Party"].contains(&p["winnerName"].as_str().unwrap_or("")),
        "SUPPORTED_PARTY_PREDICATE")?;
    need(instant_s(&r["settlement_date"])? > now, "RESOLUTION_WINDOW_ENDED")?;
    need(["numberOfWinners", "seatCount"].iter().all(|k| p[*k].is_null()
        || p[*k].as_i64() == Some(1) || p[*k].as_str() == Some("1")),
        "MULTIPLE_WINNER_PREDICATE")?;
    let mut key_obj = serde_json::Map::new();
    for k in FIELDS { key_obj.insert(k.to_string(), p[k].clone()); }
    key_obj.insert("settlementDate".to_string(), r["settlement_date"].clone());
    let mut quals = serde_json::Map::new();
    for k in QUALIFIERS {
        // JS `p[k]??null`: undefined or null both become null.
        quals.insert(k.to_string(), if p[k].is_null() { V::Null } else { p[k].clone() });
    }
    key_obj.insert("qualifiers".to_string(), V::Object(quals));
    let key = payload_hash(&V::Object(key_obj)).map_err(|e| e.to_string())?;
    Ok(Descriptor { capture: c.clone(), key,
        party: p["winnerName"].as_str().unwrap_or("").to_string(), known, received })
}

fn semantic_review(a: &Descriptor, b: &Descriptor, universe: &V) -> Result<V, String> {
    need(a.capture["root"]["contract_id"] != b.capture["root"]["contract_id"],
        "CONFLICTING_SHARED_CONTRACT_ID")?;
    let recorded_at = ms_to_iso(a.known.max(b.known) as f64);
    let policy = party_no_screen_policy();
    let expires_at = ms_to_iso((instant(&recorded_at)?
        + policy["rootMaximumAgeMs"].as_i64().unwrap_or(0)) as f64);
    let legs: Vec<V> = [a, b].iter().map(|x| json!({
        "marketId":x.capture["marketId"],"exchangeId":x.capture["exchangeId"],
        "captureMarketId":x.capture["marketId"],"outcome":"NO",
        "rulesHash":x.capture["rootHash"],"root":x.capture["root"]})).collect();
    let r = json!({
        "schemaVersion":1,"kind":"conditional_no_pair_review",
        "id":format!("source-pair-{}-{}", a.capture["marketId"].as_str().unwrap_or(""),
            b.capture["marketId"].as_str().unwrap_or("")),
        "tournamentId":universe["tournamentId"],
        "evidenceHash":payload_hash(&json!([a.capture, b.capture])).map_err(|e| e.to_string())?,
        "recordedAt":recorded_at,"expiresAt":expires_at,
        "normalResolutionOnly":true,"administrativeRefundGapAcknowledged":true,
        "legs":legs,
        "assumptions":[
            "Same structured election race/stage/date/office/district with opposing party winners.",
            "Ordinary mutual exclusion is conditional; separate contracts are not exhaustive complements.",
            "Administrative overrides, voids/refunds, affiliation disputes or inconsistent resolution can lose the full cost."],
        "scenarios":[
            {"id":"democratic","category":"ordinary","payouts":[0,1]},
            {"id":"republican","category":"ordinary","payouts":[1,0]},
            {"id":"other","category":"ordinary","payouts":[1,1]},
            {"id":"exception","category":"exception","payouts":[0,0]}]});
    crate::dual_no_review_v1::validate_dual_no_review(&r)?;
    Ok(r)
}

// account block helper: every failure is swallowed by the caller's try/catch.
fn account_check(account: &V, universe: &V, at: i64, policy: &V,
    exposure_known: &mut bool, excluded: &mut HashSet<String>) -> Result<(), String>
{
    need(account["complete"].as_bool() == Some(true)
        && account["tournamentId"] == universe["tournamentId"]
        && is_hex(&account["sourceHash"])
        && account["excludedExchangeIds"].as_array()
            .is_some_and(|a| a.iter().all(is_id))
        && account["unknownAcceptance"].as_bool() == Some(false), "ACCOUNT")?;
    let known = instant_s(&account["availableAt"])?;
    need(known <= at
        && instant_s(&account["positionsAt"])? <= known
        && instant_s(&account["ordersAt"])? <= known, "ACCOUNT_CLOCK")?;
    *exposure_known = true;
    *excluded = account["excludedExchangeIds"].as_array().cloned().unwrap_or_default()
        .iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect();
    let max_age = policy["accountMaximumAgeMs"].as_i64().unwrap_or(0);
    need(at - instant_s(&account["positionsAt"])? <= max_age
        && at - instant_s(&account["ordersAt"])? <= max_age, "ACCOUNT_STALE")?;
    Ok(())
}

/// screenPartyWinnerNoPairs({universe,rootCaptures,quoteCaptures,account,now,policy})
pub fn screen_party_winner_no_pairs(input: &V) -> Result<V, String> {
    let universe = &input["universe"];
    let root_captures = &input["rootCaptures"];
    let quote_captures = &input["quoteCaptures"];
    let account = &input["account"];
    let now = &input["now"];
    let policy = if input.get("policy").is_some() && !input["policy"].is_null() {
        input["policy"].clone()
    } else { party_no_screen_policy() };
    let at = instant(now.as_str().unwrap_or(""))?;
    need(js_truthy(Some(universe))
        && universe["exchanges"].is_array()
        && root_captures.is_array()
        && quote_captures.is_array(), "INPUT")?;
    need(payload_hash(&policy).map_err(|e| e.to_string())?
        == payload_hash(&party_no_screen_policy()).map_err(|e| e.to_string())?,
        "FIXED_POLICY")?;
    let mut roots: indexmap::IndexMap<String, Descriptor> = indexmap::IndexMap::new();
    let mut groups: indexmap::IndexMap<String, Vec<Descriptor>> = indexmap::IndexMap::new();
    let mut rejections: Vec<V> = Vec::new();
    let mut poisoned_roots: HashSet<String> = HashSet::new();
    for c in root_captures.as_array().cloned().unwrap_or_default() {
        let outcome: Result<(), String> = (|| {
            if poisoned_roots.contains(&js_stringify(&c["marketId"])) { return Ok(()); }
            let d = descriptor(&c, universe, at)?;
            let id_key = c["marketId"].as_str().unwrap_or("");
            match roots.get(id_key) {
                None => { roots.insert(id_key.to_string(), d); }
                Some(old) => {
                    if d.known > old.known { roots.insert(id_key.to_string(), d); }
                    else if d.known == old.known {
                        need(c["rootHash"] == old.capture["rootHash"],
                            "SAME_CLOCK_ROOT_CONFLICT")?;
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = outcome {
            if e == "PARTY_NO_SCREEN_SAME_CLOCK_ROOT_CONFLICT" {
                poisoned_roots.insert(js_stringify(&c["marketId"]));
                roots.shift_remove(c["marketId"].as_str().unwrap_or(""));
            }
            rejections.push(json!({"marketId":if js_truthy(c.get("marketId"))
                { c["marketId"].clone() } else { V::Null }, "reason":e}));
        }
    }
    for (_, d) in roots.iter() {
        groups.entry(d.key.clone()).or_default().push(d.clone());
    }
    let mut quotes: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    let mut poisoned_quotes: HashSet<String> = HashSet::new();
    for q in quote_captures.as_array().cloned().unwrap_or_default() {
        let outcome: Result<(), String> = (|| {
            if poisoned_quotes.contains(&js_stringify(&q["exchangeId"])) { return Ok(()); }
            need(is_id(&q["marketId"]) && is_id(&q["exchangeId"]) && is_hex(&q["quoteHash"])
                && party_winner_quote_hash(&q)? == q["quoteHash"].as_str().unwrap_or(""),
                "QUOTE_BINDING")?;
            instant_s(&q["knownAt"])?;
            let ex_key = q["exchangeId"].as_str().unwrap_or("");
            match quotes.get(ex_key) {
                None => { quotes.insert(ex_key.to_string(), q.clone()); }
                Some(old) => {
                    if instant_s(&q["knownAt"])? > instant_s(&old["knownAt"])? {
                        quotes.insert(ex_key.to_string(), q.clone());
                    } else if q["knownAt"] == old["knownAt"] {
                        need(q["quoteHash"] == old["quoteHash"], "SAME_CLOCK_QUOTE_CONFLICT")?;
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = outcome {
            if e == "PARTY_NO_SCREEN_SAME_CLOCK_QUOTE_CONFLICT" {
                poisoned_quotes.insert(js_stringify(&q["exchangeId"]));
                quotes.shift_remove(q["exchangeId"].as_str().unwrap_or(""));
            }
            rejections.push(json!({"marketId":if js_truthy(q.get("marketId"))
                { q["marketId"].clone() } else { V::Null }, "reason":e}));
        }
    }
    let mut account_valid = false;
    let mut account_exposure_known = false;
    let mut excluded: HashSet<String> = if account["excludedExchangeIds"].is_array() {
        account["excludedExchangeIds"].as_array().unwrap_or(&Vec::new()).iter()
            .filter(|x| is_id(x)).filter_map(|x| x.as_str().map(|s| s.to_string())).collect()
    } else { HashSet::new() };
    let _ = account_check(account, universe, at, &policy,
        &mut account_exposure_known, &mut excluded);

    let mut pairs: Vec<V> = Vec::new();
    let mut groups_without_both = 0i64;
    for (_, group) in groups.iter() {
        let d: Vec<&Descriptor> = group.iter()
            .filter(|x| x.party == "Democratic Party").collect();
        let r: Vec<&Descriptor> = group.iter()
            .filter(|x| x.party == "Republican Party").collect();
        if d.is_empty() || r.is_empty() { groups_without_both += 1; continue; }
        // Duplicate aliases of one party/event are not distinct opportunities.
        if d.len() != 1 || r.len() != 1 {
            rejections.push(json!({"reason":"PARTY_NO_SCREEN_AMBIGUOUS_EVENT_ALIASES",
                "marketIds":group.iter().map(|x| x.capture["marketId"].clone()).collect::<Vec<V>>()}));
            continue;
        }
        let legs = [d[0], r[0]];
        let review = match semantic_review(d[0], r[0], universe) {
            Ok(v) => v,
            Err(e) => {
                rejections.push(json!({"reason":e,
                    "marketIds":legs.iter().map(|x| x.capture["marketId"].clone()).collect::<Vec<V>>()}));
                continue;
            }
        };
        let mut flags: Vec<String> = vec![
            "quantity_and_depth_unknown".to_string(),
            "ordinary_resolution_only_exception_unprotected".to_string()];
        let has_exposure = legs.iter()
            .any(|l| excluded.contains(l.capture["exchangeId"].as_str().unwrap_or("")));
        if has_exposure { flags.push("whole_account_holding_or_open_order".to_string()); }
        if !account_valid {
            flags.push("account_unavailable_stale_or_unknown_acceptance".to_string());
        }
        let roots_current = legs.iter()
            .all(|l| at - l.received <= policy["rootMaximumAgeMs"].as_i64().unwrap_or(0));
        if !roots_current { flags.push("cached_rules_need_refresh".to_string()); }
        let mut prices: Vec<BigInt> = Vec::new();
        let mut starts: Vec<i64> = Vec::new();
        let mut receipts: Vec<i64> = Vec::new();
        let mut quote_hashes: Vec<V> = Vec::new();
        let mut quote_clocks: Vec<V> = Vec::new();
        let mut quotes_valid = true;
        let mut quotes_math_valid = true;
        for l in legs {
            let outcome: Result<(), String> = (|| {
                let q = quotes.get(l.capture["exchangeId"].as_str().unwrap_or(""));
                need(q.is_some()
                    && q.unwrap()["marketId"] == l.capture["marketId"]
                    && q.unwrap()["tournamentId"] == universe["tournamentId"]
                    && q.unwrap()["option"].as_str() == Some("YES")
                    && is_hex(&q.unwrap()["sourceHash"]), "QUOTE_SCOPE")?;
                let q = q.unwrap();
                let start = instant_s(&q["requestStartedAt"])?;
                let received = instant_s(&q["receivedAt"])?;
                let known = instant_s(&q["knownAt"])?;
                need(start <= received && received <= known && known <= at, "QUOTE_CLOCK")?;
                let bid = exact_price(&q["bestBid"])?;
                let ask = exact_price(&q["bestAsk"])?;
                need(bid < ask, "CROSSED_QUOTE")?;
                prices.push(BigInt::from(SCALE) - bid);
                starts.push(start);
                receipts.push(received);
                quote_hashes.push(q["sourceHash"].clone());
                quote_clocks.push(json!({"requestStartedAt":q["requestStartedAt"],
                    "receivedAt":q["receivedAt"],"knownAt":q["knownAt"]}));
                Ok(())
            })();
            match outcome {
                Ok(()) => {
                    let q = quotes.get(l.capture["exchangeId"].as_str().unwrap_or("")).unwrap();
                    let start = instant_s(&q["requestStartedAt"])?;
                    if at - start > policy["quoteMaximumAgeMs"].as_i64().unwrap_or(0) {
                        flags.push("PARTY_NO_SCREEN_QUOTE_AGE".to_string());
                        quotes_valid = false;
                    }
                    if l.known > start {
                        flags.push("PARTY_NO_SCREEN_RULES_NOT_KNOWN_BEFORE_QUOTE".to_string());
                        quotes_valid = false;
                    }
                }
                Err(e) => {
                    flags.push(e);
                    quotes_valid = false;
                    quotes_math_valid = false;
                }
            }
        }
        let skew: Option<i64> = if quotes_math_valid {
            match (receipts.iter().max(), starts.iter().min()) {
                (Some(r), Some(s)) => Some(r - s),
                _ => Some(i64::MIN), // Math.max()=-Infinity, Math.min()=+Infinity
            }
        } else { None };
        if quotes_math_valid && skew.unwrap_or(i64::MIN) > policy["maximumSkewMs"].as_i64().unwrap_or(0) {
            flags.push("quote_packet_skew".to_string());
            quotes_valid = false;
        }
        let account_causal = account_valid && quotes_valid
            && instant_s(&account["availableAt"])?
                <= starts.iter().min().copied().unwrap_or(i64::MAX);
        if !account_causal {
            flags.push("account_not_known_before_both_quote_requests".to_string());
        }
        let raw_cost: Option<BigInt> = if quotes_math_valid {
            Some(prices.iter().fold(BigInt::from(0), |a, b| a + b))
        } else { None };
        let raw_surplus = raw_cost.as_ref().map(|c| BigInt::from(SCALE) - c);
        let cushioned_cost = raw_cost.as_ref().map(|c| c + BigInt::from(2i64)
            * BigInt::from(policy["protectiveTicksPerLeg"].as_i64().unwrap_or(0))
            * BigInt::from(TICK));
        let min_surplus = big_amount(policy["minimumOrdinarySurplus"].as_str().unwrap_or(""))?;
        let raw_qualified = raw_surplus.as_ref().map_or(false, |s| *s >= min_surplus);
        let sufficient = cushioned_cost.as_ref()
            .map_or(false, |c| BigInt::from(SCALE) - c >= min_surplus);
        if !sufficient {
            flags.push("minimum_protective_ordinary_surplus_not_met".to_string());
        }
        let market_ids: Vec<V> = legs.iter().map(|l| l.capture["marketId"].clone()).collect();
        let exchange_ids: Vec<V> = legs.iter().map(|l| l.capture["exchangeId"].clone()).collect();
        let root_hashes: Vec<V> = legs.iter().map(|l| l.capture["rootHash"].clone()).collect();
        let root_source_hashes: Vec<V> =
            legs.iter().map(|l| l.capture["sourceHash"].clone()).collect();
        let root_known_at: Vec<V> = legs.iter().map(|l| l.capture["knownAt"].clone()).collect();
        let raw_no_asks: V = if quotes_math_valid {
            json!(prices.iter().map(|p| money(p)).collect::<Vec<String>>())
        } else { V::Null };
        let unowned_read = roots_current && !has_exposure && account_causal && sufficient;
        let unowned_indicative = account_exposure_known && !has_exposure && sufficient;
        let mut seen = HashSet::new();
        let unique_flags: Vec<String> = flags.iter()
            .filter(|f| seen.insert((*f).clone())).cloned().collect();
        pairs.push(json!({
            "id":review["id"],
            "race":legs[0].capture["root"]["contract_details"]["raceName"],
            "eventKey":legs[0].key,
            "marketIds":market_ids,"exchangeIds":exchange_ids,"rootHashes":root_hashes,
            "rootSourceHashes":root_source_hashes,"rootKnownAt":root_known_at,
            "quoteSourceHashes":quote_hashes,"quoteClocks":quote_clocks,
            "indicativeMathValid":quotes_math_valid,
            "rawNoAsks":raw_no_asks,
            "rawNoCost":raw_cost.as_ref().map(|c| json!(money(c))).unwrap_or(V::Null),
            "conditionalOrdinaryPayoutFloor":"1.000000",
            "conditionalRawSurplus":raw_surplus.as_ref().map(|s| json!(money(s))).unwrap_or(V::Null),
            "protectiveTickCost":cushioned_cost.as_ref().map(|c| json!(money(c))).unwrap_or(V::Null),
            "conditionalProtectiveSurplus":cushioned_cost.as_ref()
                .map(|c| json!(money(&(BigInt::from(SCALE) - c)))).unwrap_or(V::Null),
            "zeroPayoutExceptionLoss":raw_cost.as_ref()
                .map(|c| json!(money(&(-c.clone())))).unwrap_or(V::Null),
            "zeroPayoutExceptionLossAtProtectiveCost":cushioned_cost.as_ref()
                .map(|c| json!(money(&(-c.clone())))).unwrap_or(V::Null),
            "quoteSkewMs":skew.filter(|s| *s != i64::MIN).map(|s| json!(s)).unwrap_or(V::Null),
            "rootsCurrent":roots_current,
            "quotesCurrentAndCofresh":quotes_valid,
            "accountCurrent":account_valid,
            "cachedAccountExposureKnown":account_exposure_known,
            "accountKnownBeforeBothRequests":account_causal,
            "wholeAccountExposure":has_exposure,
            "rawSurplusQualified":raw_qualified,
            "protectiveSurplusQualified":sufficient,
            "unownedReadCandidate":unowned_read,
            "unownedIndicativeCandidate":unowned_indicative,
            "quantityPerLeg":1,"depthKnown":false,"depthQualifiedQuantity":null,
            "nativeNoBookObserved":false,"ordinaryResolutionOnly":true,
            "fullResolutionCompletenessProven":false,"guaranteedProfit":false,
            "flags":unique_flags,"orderEligible":false,"executionAuthority":false}));
    }
    pairs.sort_by(|a, b| {
        let rb = js_truthy(b.get("unownedReadCandidate")) as i64
            - js_truthy(a.get("unownedReadCandidate")) as i64;
        if rb != 0 { return rb.cmp(&0); }
        let rc = js_truthy(b.get("rootsCurrent")) as i64
            - js_truthy(a.get("rootsCurrent")) as i64;
        if rc != 0 { return rc.cmp(&0); }
        let cost_ord = if a["rawNoCost"].is_null() {
            std::cmp::Ordering::Greater
        } else if b["rawNoCost"].is_null() {
            std::cmp::Ordering::Less
        } else {
            let av = big_amount(a["rawNoCost"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0));
            let bv = big_amount(b["rawNoCost"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0));
            av.cmp(&bv)
        };
        if cost_ord != std::cmp::Ordering::Equal { return cost_ord; }
        a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or(""))
    });
    let ranking: Vec<V> = pairs.iter()
        .filter(|p| js_truthy(p.get("unownedReadCandidate")))
        .map(|p| p["id"].clone()).collect();
    let mut indicative: Vec<&V> = pairs.iter()
        .filter(|p| js_truthy(p.get("unownedIndicativeCandidate"))).collect();
    for p in &indicative {
        // JS would throw amount(null) inside the comparator; propagate the same error.
        big_amount(p["protectiveTickCost"].as_str().unwrap_or(""))?;
    }
    indicative.sort_by(|a, b| {
        let av = big_amount(a["protectiveTickCost"].as_str().unwrap_or(""))
            .unwrap_or(BigInt::from(0));
        let bv = big_amount(b["protectiveTickCost"].as_str().unwrap_or(""))
            .unwrap_or(BigInt::from(0));
        av.cmp(&bv).then_with(|| a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or("")))
    });
    let indicative_ranking: Vec<V> = indicative.iter().map(|p| p["id"].clone()).collect();
    Ok(json!({
        "schemaVersion":1,
        "mode":"source_bound_party_winner_conditional_no_read_ranking",
        "evaluatedAt":now,"policy":policy,
        "rootCaptureCount":root_captures.as_array().map(|a| a.len()).unwrap_or(0),
        "validLatestRoots":roots.len(),"semanticPairCount":pairs.len(),
        "groupsWithoutOpposingParty":groups_without_both,
        "rejections":rejections,"pairs":pairs,"ranking":ranking,
        "indicativeRanking":indicative_ranking,
        "networkRequests":0,"competitionOrders":0,"executionAuthority":false,
        "profitabilityEstablished":false,
        "limitations":[
            "Opposing party roots establish only conditional ordinary-election mutual exclusion; they do not protect against inconsistent resolution/void/refund exceptions.",
            "Bulk YES bids are converted to same-exchange NO asks with exact decimals; there is no independently observed native NO book or depth.",
            "Read ranking never establishes executable quantities, atomic two-leg fills, guaranteed profit or authority to submit orders.",
            "ranking requires current rules/cofresh quotes/causal fresh account; indicativeRanking is only cached-cost read-refresh prioritization and can include stale rules or quotes. Neither ranking is order authority.",
            "Stale rules remain source-dated and flagged; fresh original metadata/account/depth receipts are required before any later economic test."]}))
}
