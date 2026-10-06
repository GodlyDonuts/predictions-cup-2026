// live-paper/dip-shadow.mjs — diversified historical-dip replay engine.
use baal_paper::fixed::{big_amount, TICK};
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{digest, js_int, js_stringify, js_truthy};
use baal_service_runtime::history::historical_reference;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::BTreeSet;

fn parse(v: &V) -> f64 { v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
fn truthy(v: &V) -> bool { js_truthy(Some(v)) }
/// String(v) for scalar journal values (string/number/bool).
fn js_str(v: &V) -> String {
    match v { V::String(s) => s.clone(), V::Number(n) => n.to_string(), V::Bool(b) => b.to_string(), _ => String::new() }
}
fn num(v: &V) -> f64 { v.as_f64().unwrap_or(f64::NAN) }
fn f64_of_big(v: &BigInt) -> f64 { v.to_string().parse().unwrap_or(f64::NAN) }

/// validateDipPolicy(p)
pub fn validate_dip_policy(p: &V) -> Result<V, String> {
    if p["name"].as_str() != Some("diversified-dip-v1") || p["schemaVersion"].as_i64() != Some(1)
        || js_int(&p["feeBps"]) != Some(0)
    {
        return Err("Invalid dip policy".into());
    }
    for k in ["lookbackMs", "historyCacheAgeMs", "minimumSamples", "minimumSpanMs",
        "lastHistoricalObservationAgeMs", "maximumHoldMs", "cooldownMs", "maxOpenMarkets",
        "maxPerState", "maxPerOffice", "maxUnknownIdentity"] {
        if js_int(&p[k]).is_none_or(|x| x < 1) { return Err("Invalid dip limit".into()); }
    }
    // `!== undefined`: a present-but-null field still fails the check.
    if p.get("profile").is_some() && p["profile"].as_str() != Some("scale") {
        return Err("Unknown dip profile".into());
    }
    let scaled = p["profile"].as_str() == Some("scale");
    if p.get("executionPolicy").is_some() && p["executionPolicy"].as_str() != Some("responsive_v2") {
        return Err("Unknown dip execution policy".into());
    }
    if p["executionPolicy"].as_str() == Some("responsive_v2")
        && (num(&p["maxOpenMarkets"]) > 3.0 || p["requireScheduledEntry"] != V::Bool(true))
    {
        return Err("Responsive cohort exceeds collection capacity".into());
    }
    let max_cost = big_amount(p["maxMarketCost"].as_str().unwrap_or(""))?;
    let cap = big_amount(if scaled { "1000.000000" } else { "50.000000" }).unwrap();
    if num(&p["minimumSamples"]) < 8.0 || num(&p["minimumSpanMs"]) < 1800000.0
        || num(&p["maxOpenMarkets"]) > if scaled { 100.0 } else { 20.0 }
        || num(&p["maxPerState"]) > if scaled { 4.0 } else { 3.0 }
        || num(&p["maxPerOffice"]) > if scaled { 50.0 } else { 6.0 }
        || js_int(&p["maxUnknownIdentity"]) != Some(1)
        || num(&p["lookbackMs"]) > 86400000.0 || num(&p["maximumHoldMs"]) > 21600000.0
        || max_cost > cap
    {
        return Err("Unsafe dip limits".into());
    }
    for k in ["minimumDip", "minimumRelativeDip", "maximumDip", "minimumZ",
        "maximumSpread", "minimumPrice", "maximumPrice"] {
        if !num(&p[k]).is_finite() || num(&p[k]) <= 0.0 { return Err("Invalid dip signal".into()); }
    }
    let take_profit = big_amount(p["takeProfit"].as_str().unwrap_or(""))?;
    let stop_loss = big_amount(p["stopLoss"].as_str().unwrap_or(""))?;
    if num(&p["minimumDip"]) >= num(&p["maximumDip"])
        || num(&p["minimumPrice"]) >= num(&p["maximumPrice"])
        || num(&p["maximumPrice"]) >= 1.0
        || num(&p["maximumSpread"]) > 0.05
        || take_profit <= BigInt::from(0) || stop_loss <= BigInt::from(0) || max_cost <= BigInt::from(0)
    {
        return Err("Invalid dip costs".into());
    }
    Ok(p.clone())
}
fn append_book(s: &mut V, b: &V, p: &V) {
    let key = js_str(&b["exchangeId"]);
    let list = s["bookHistory"][&key].as_array().cloned().unwrap_or_default();
    if list.last().is_some_and(|l| l["versionKey"] == b["versionKey"]) { return; }
    let bid = big_amount(b["bids"][0]["price"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0));
    let ask = big_amount(b["asks"][0]["price"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0));
    let mid = (f64_of_big(&bid) + f64_of_big(&ask)) / 2e6;
    let lookback = num(&p["lookbackMs"]);
    let received = parse(&b["receivedAt"]);
    let mut next: Vec<V> = list;
    next.push(json!({"at": b["sourceAt"], "receivedAt": b["receivedAt"], "mid": mid,
        "versionKey": b["versionKey"]}));
    next.retain(|x| parse(&x["at"]) >= received - lookback);
    if next.len() > 512 { next = next[next.len() - 512..].to_vec(); }
    s["bookHistory"][&key] = json!(next);
}

/// dipSignal(reference, ask, bid, p)
pub fn dip_signal(reference: &V, ask: f64, bid: f64, p: &V) -> V {
    if !truthy(&reference["ready"]) {
        let mut out = json!({"admitted": false, "reason": reference["reason"]});
        let o = out.as_object_mut().unwrap();
        for (k, v) in reference.as_object().cloned().unwrap_or_default() { o.insert(k, v); }
        return out;
    }
    let center = num(&reference["center"]);
    let discount = center - ask;
    let z = discount / num(&reference["robustDeviation"]);
    let relative = discount / center.max(0.001);
    let admitted = ask >= num(&p["minimumPrice"]) && ask <= num(&p["maximumPrice"])
        && ask > bid && ask - bid <= num(&p["maximumSpread"])
        && discount >= num(&p["minimumDip"])
        && relative >= num(&p["minimumRelativeDip"])
        && discount <= num(&p["maximumDip"])
        && z >= num(&p["minimumZ"]);
    let mut out = reference.as_object().cloned().unwrap_or_default();
    out.insert("discount".into(), json!(discount));
    out.insert("relativeDiscount".into(), json!(relative));
    out.insert("z".into(), json!(z));
    out.insert("ask".into(), json!(ask));
    out.insert("bid".into(), json!(bid));
    out.insert("admitted".into(), json!(admitted));
    out.insert("reason".into(), json!(if admitted { "historical_discount" } else { "discount_spread_or_regime_gate" }));
    V::Object(out)
}

/// diversification(s, bank, marketId)
pub fn diversification(s: &V, bank: &V, market_id: &V) -> V {
    let st = &s["wallet"]["strategies"]["mean_reversion"];
    let mid_str = js_str(market_id);
    let mut ids: Vec<String> = Vec::new();
    let mut seen = BTreeSet::new();
    for id in st["positions"].as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() {
        if seen.insert(id.clone()) { ids.push(id); }
    }
    for (id, p) in st["pending"].as_object().cloned().unwrap_or_default() {
        if p["action"].as_str() == Some("buy") && s["wallet"]["books"][&id]["marketId"] != *market_id
            && seen.insert(id.clone())
        {
            ids.push(id);
        }
    }
    let markets: Vec<V> = ids.iter()
        .map(|id| s["wallet"]["books"][id]["marketId"].clone()).collect();
    let current = bank["identities"][mid_str.as_str()].clone();
    let current = if current.is_null() { json!({}) } else { current };
    let meta: Vec<V> = markets.iter()
        .map(|id| { let v = bank["identities"][js_str(id).as_str()].clone();
            if v.is_null() { json!({}) } else { v } }).collect();
    let p = &bank["policy"];
    if markets.len() as i64 >= js_int(&p["maxOpenMarkets"]).unwrap_or(0) {
        return json!({"allowed": false, "reason": "maximum_open_markets"});
    }
    if !truthy(&current["state"]) || !truthy(&current["office"]) {
        if meta.iter().filter(|x| !truthy(&x["state"]) || !truthy(&x["office"])).count() as i64
            >= js_int(&p["maxUnknownIdentity"]).unwrap_or(0)
        {
            return json!({"allowed": false, "reason": "unknown_identity_concentration"});
        }
    }
    if truthy(&current["state"])
        && meta.iter().filter(|x| x["state"] == current["state"]).count() as i64
            >= js_int(&p["maxPerState"]).unwrap_or(0)
    {
        return json!({"allowed": false, "reason": "state_concentration"});
    }
    if truthy(&current["office"])
        && meta.iter().filter(|x| x["office"] == current["office"]).count() as i64
            >= js_int(&p["maxPerOffice"]).unwrap_or(0)
    {
        return json!({"allowed": false, "reason": "office_concentration"});
    }
    json!({"allowed": true, "reason": "within_diversification_limits"})
}
fn invalidate_dip(s: &mut V, market: Option<&str>, at: &str, reason: &str) {
    crate::model::invalidate(&mut s["wallet"], market, at, reason);
    let pending = s["wallet"]["strategies"]["mean_reversion"]["pending"].as_object().cloned().unwrap_or_default();
    for (id, p) in pending {
        if p["action"].as_str() == Some("buy")
            && (market.is_none() || s["wallet"]["books"][&id]["marketId"].as_str() == market)
        {
            s["wallet"]["strategies"]["mean_reversion"]["pending"].as_object_mut().unwrap().remove(&id);
            let e = &mut s["wallet"]["strategies"]["mean_reversion"]["expiredIntents"];
            *e = json!(e.as_i64().unwrap_or(0) + 1);
        }
    }
}
fn receipt_ordered_snapshots(event: &V, cycle_at: &V) -> Result<Vec<(V, V, usize)>, String> {
    if !parse(cycle_at).is_finite() { return Err("Invalid dip cycle clock".into()); }
    // The journal preserves core/supplemental provenance, not network read order.
    // Invalid receipts remain invalid book inputs; use the cycle clock only to
    // order their invalidation. Equal clocks retain the original journal order.
    let mut rows: Vec<(V, V, usize)> = Vec::new();
    for (index, snap) in event["sourceSnapshots"].as_array().cloned().unwrap_or_default().iter()
        .chain(event["supplementalDepth"].as_array().cloned().unwrap_or_default().iter())
        .cloned().enumerate()
    {
        let observed = if truthy(&snap["skipped"]) { snap["at"].clone() } else { snap["receivedAt"].clone() };
        let at = if parse(&observed).is_finite() { observed } else { cycle_at.clone() };
        rows.push((snap, at, index));
    }
    rows.sort_by(|a, b| parse(&a.1).partial_cmp(&parse(&b.1)).unwrap_or(std::cmp::Ordering::Equal)
        .then(a.2.cmp(&b.2)));
    Ok(rows)
}
/// /^\/markets\/(\d+)\/orderbook\?/ — market id capture.
fn endpoint_market(endpoint: &str) -> Option<String> {
    let rest = endpoint.strip_prefix("/markets/")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || !rest[digits.len()..].starts_with("/orderbook?") { return None; }
    Some(digits)
}

/// createDip(raw, c, universe, policy, identities)
pub fn create_dip(raw: &str, c: &V, universe: &V, policy: &V, identities: &V) -> Result<V, String> {
    crate::model::validate_config(c)?;
    validate_dip_policy(policy)?;
    let u = crate::breadth::validate_universe(universe, c["tournamentId"].as_str().unwrap_or(""))?;
    let want = if policy["profile"].as_str() == Some("scale") { "diversified-dip-scale-v1" } else { "diversified-dip-v1" };
    if c["strategyUniverse"].as_str() != Some(want) || js_int(&c["feeBps"]) != Some(0)
        || u["exchanges"].as_array().cloned().unwrap_or_default().iter().any(|x| {
            !c["marketIds"].as_array().is_some_and(|a| a.iter().any(|m| m == &x["marketId"]))
                || x["option"].as_str() != Some("YES")
        })
    {
        return Err("Dip universe scope mismatch".into());
    }
    let records = crate::ml_shadow::verify_source(raw)?;
    let records = match records { Some(v) if !v.records.is_empty() => v, _ => return Err("Complete history source required".into()) };
    let last = records.records.last().unwrap();
    let cutoff = if truthy(&last["event"]["clockAt"]) { last["event"]["clockAt"].clone() } else { last["at"].clone() };
    let bank = json!({"schemaVersion": 1, "name": policy["name"], "config": c, "universe": u,
        "policy": policy, "identities": identities, "cutoff": cutoff,
        "sourceRecords": records.records.len(),
        "sourceHash": records.prefix_hashes.last().map(String::as_str).unwrap_or("")});
    let mut state = json!({"schemaVersion": 1, "bankHash": digest(&bank), "sourceSequence": 0,
        "lastRecordHash": V::Null, "bookHistory": {}, "histories": {}, "signals": {},
        "cooldowns": {}, "wallet": crate::model::initial_state(c, policy["name"].as_str().unwrap_or(""), cutoff.as_str().unwrap_or(""))?,
        "coverage": V::Null});
    state = advance_dip(&state, &records, &bank)?;
    Ok(json!({"bank": bank, "state": state}))
}

/// advanceDip(input, records, bank)
pub fn advance_dip(input: &V, records: &crate::ml_shadow::VerifiedSource, bank: &V) -> Result<V, String> {
    let mut s = input.clone();
    let c = &bank["config"];
    let p = &bank["policy"];
    validate_dip_policy(p)?;
    let source_seq = js_int(&bank["sourceRecords"]).unwrap_or(0);
    let prefix_ok = source_seq >= 1
        && records.prefix_hashes.get((source_seq - 1) as usize).map(String::as_str)
            == bank["sourceHash"].as_str();
    let s_seq = js_int(&s["sourceSequence"]).unwrap_or(0);
    let last_ok = s_seq == 0
        || records.records.get((s_seq - 1) as usize).map(|r| &r["hash"]) == Some(&s["lastRecordHash"]);
    if s["bankHash"].as_str() != Some(digest(bank).as_str())
        || !prefix_ok
        || s_seq > records.records.len() as i64
        || !last_ok
    {
        return Err("Dip source/bank changed".into());
    }
    crate::model::assert_state(&s["wallet"], c)?;
    let mut universe: Map<String, V> = Map::new();
    for x in bank["universe"]["exchanges"].as_array().cloned().unwrap_or_default() {
        universe.insert(js_stringify(&x["marketId"]), x);
    }
    for r in &records.records {
        if js_int(&r["sequence"]).unwrap_or(0) <= js_int(&s["sourceSequence"]).unwrap_or(0) { continue; }
        let event = &r["event"];
        let at = if truthy(&event["clockAt"]) { event["clockAt"].clone() } else { r["at"].clone() };
        s["sourceSequence"] = r["sequence"].clone();
        s["lastRecordHash"] = r["hash"].clone();
        let prospective = parse(&at) > parse(&bank["cutoff"]);
        if event["type"].as_str() == Some("market_history") {
            let h = &event["history"];
            if h["tournamentId"] != c["tournamentId"]
                || universe.get(&js_stringify(&h["marketId"])).map(|x| &x["exchangeId"]) != Some(&h["exchangeId"])
                || parse(&h["receivedAt"]) > parse(&at)
            {
                return Err("Historical capture scope mismatch".into());
            }
            s["histories"][js_str(&h["exchangeId"]).as_str()] = h.clone();
            continue;
        }
        if event["type"].as_str() == Some("account_read_outage") {
            if prospective { invalidate_dip(&mut s, None, at.as_str().unwrap_or(""), "account_outage"); }
            continue;
        }
        if event["type"].as_str() != Some("cycle") { continue; }
        let mut accepted_fresh: BTreeSet<String> = BTreeSet::new();
        for (snap, observed_at, _) in receipt_ordered_snapshots(event, &at)? {
            if truthy(&snap["skipped"]) {
                let fresh = snap["skipped"].as_str() == Some("Missing/stale engine version")
                    && accepted_fresh.contains(&js_stringify(&snap["marketId"]));
                if prospective && !fresh {
                    invalidate_dip(&mut s, Some(&js_str(&snap["marketId"])),
                        observed_at.as_str().unwrap_or(""), snap["skipped"].as_str().unwrap_or(""));
                }
                continue;
            }
            let Some(id) = endpoint_market(snap["endpoint"].as_str().unwrap_or("")) else { continue };
            let spec = universe.get(&js_stringify(&json!(id)));
            let Some(spec) = spec else { continue };
            let exclusions: BTreeSet<String> = match event["accountExclusions"].as_array() {
                Some(a) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
                None => BTreeSet::new(),
            };
            let b = (|| -> Result<V, String> {
                if event["accountExclusions"].as_array().is_none() {
                    return Err("Missing reconciled exclusion list".into());
                }
                let b = crate::model::normalize_book(&snap["data"], &id,
                    c["tournamentId"].as_str().unwrap_or(""), snap["receivedAt"].as_str().unwrap_or(""),
                    snap["requestStartedAt"].as_str().unwrap_or(""),
                    js_int(&c["maxDataAgeMs"]).unwrap_or(0), &exclusions, "authoritative_receipt_v1")?;
                if b["exchangeId"] != spec["exchangeId"] { return Err("Identity changed".into()); }
                Ok(b)
            })();
            let b = match b {
                Ok(b) => b,
                Err(_) => {
                    if prospective {
                        invalidate_dip(&mut s, Some(&id), observed_at.as_str().unwrap_or(""), "invalid_book");
                    }
                    continue;
                }
            };
            accepted_fresh.insert(js_stringify(&json!(id)));
            if prospective && parse(&b["requestStartedAt"]) > parse(&bank["cutoff"]) {
                let exchange_id = js_str(&b["exchangeId"]);
                let history = s["histories"].get(exchange_id.as_str());
                let books = s["bookHistory"][exchange_id.as_str()].as_array().cloned().unwrap_or_default();
                let reference = historical_reference(history, &books,
                    b["requestStartedAt"].as_str().unwrap_or(""), p)?;
                let slip = BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0));
                let ask = f64_of_big(&(big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))? + &slip)) / 1e6;
                let bid = f64_of_big(&(big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))? - &slip)) / 1e6;
                let signal = dip_signal(&reference, ask, bid, p);
                let risk = diversification(&s, bank, &json!(id));
                let st = &s["wallet"]["strategies"]["mean_reversion"];
                let instrument = js_str(&b["instrument"]);
                let was_held = truthy(&st["positions"][instrument.as_str()]);
                let cooldown = parse(&s["cooldowns"][id.as_str()]) > parse(&b["receivedAt"]);
                let mut sig = signal.as_object().cloned().unwrap_or_default();
                sig.insert("marketId".into(), json!(id));
                sig.insert("exchangeId".into(), b["exchangeId"].clone());
                sig.insert("at".into(), b["receivedAt"].clone());
                sig.insert("risk".into(), risk.clone());
                sig.insert("cooldown".into(), json!(cooldown));
                s["signals"][exchange_id.as_str()] = V::Object(sig);
                let scheduled = !truthy(&p["requireScheduledEntry"])
                    || event["entryMarketIds"].as_array().is_some_and(|a| a.iter().any(|x| x.as_str() == Some(id.as_str())));
                let reference_val = if truthy(&reference["ready"]) {
                    Some(num(&reference["center"]) - slip.to_string().parse::<f64>().unwrap_or(0.0) / 1e6)
                } else { None };
                let wallet = crate::model::step_dip(&s["wallet"], &b, c, reference_val,
                    truthy(&signal["admitted"]) && truthy(&risk["allowed"]) && !cooldown && scheduled,
                    num(&p["minimumDip"]), &p["takeProfit"], js_int(&p["maximumHoldMs"]).unwrap_or(0),
                    &p["maxMarketCost"], p["executionPolicy"].as_str().unwrap_or("legacy_v1"))?;
                s["wallet"] = wallet;
                if was_held && !truthy(&s["wallet"]["strategies"]["mean_reversion"]["positions"][instrument.as_str()]) {
                    let t = parse(&b["receivedAt"]) + js_int(&p["cooldownMs"]).unwrap_or(0) as f64;
                    s["cooldowns"][id.as_str()] = json!(ms_to_iso(t));
                }
            }
            append_book(&mut s, &b, p);
        }
        if truthy(&event["bulkPrices"]) {
            let mut cov = event["bulkPrices"].as_object().cloned().unwrap_or_default();
            cov.insert("quotes".into(), event["bulkPrices"]["quotes"].clone());
            s["coverage"] = V::Object(cov);
        }
        if prospective {
            crate::model::sample_equity(&mut s["wallet"], c, at.as_str().unwrap_or(""));
        }
    }
    crate::model::assert_state(&s["wallet"], c)?;
    Ok(s)
}

/// dipDepthPlan(s, bank, coverage, now)
pub fn dip_depth_plan(s: &V, bank: &V, coverage: &V, now: f64) -> Result<V, String> {
    let mut order: Vec<String> = Vec::new();
    let mut priority: Map<String, V> = Map::new();
    let st = &s["wallet"]["strategies"]["mean_reversion"];
    // JS Set keeps insertion order; sort stability makes tie order observable.
    let mut ids: Vec<String> = Vec::new();
    for id in st["positions"].as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default()
        .into_iter().chain(st["pending"].as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default())
    {
        if !ids.contains(&id) { ids.push(id); }
    }
    for id in &ids {
        let b = &s["wallet"]["books"][id];
        if !b.is_null() {
            let key = js_stringify(&b["marketId"]);
            if !priority.contains_key(&key) {
                order.push(key.clone());
                priority.insert(key, json!({
                    "marketId": b["marketId"],
                    "priority": 1e6 + (now - parse(&b["receivedAt"])) / 1000.0,
                    "reason": "position_or_pending_intent",
                }));
            }
        }
    }
    for (_, q) in coverage["quotes"].as_object().cloned().unwrap_or_default() {
        if q["option"].as_str() != Some("YES")
            || q.get("bestAsk") == Some(&V::Null)
            || q.get("bestBid") == Some(&V::Null)
            || truthy(&q["crossed"])
            || now - parse(&q["receivedAt"]) > 600000.0
        {
            continue;
        }
        let history = s["histories"].get(js_str(&q["exchangeId"]).as_str());
        let books = s["bookHistory"][js_str(&q["exchangeId"]).as_str()].as_array().cloned().unwrap_or_default();
        let ref_v = historical_reference(history, &books, &ms_to_iso(now), &bank["policy"])?;
        let sig = dip_signal(&ref_v, num(&q["bestAsk"]), num(&q["bestBid"]), &bank["policy"]);
        let key = js_stringify(&q["marketId"]);
        if truthy(&sig["admitted"]) && !priority.contains_key(&key)
            && truthy(&diversification(s, bank, &q["marketId"])["allowed"])
        {
            order.push(key.clone());
            priority.insert(key, json!({
                "marketId": q["marketId"], "priority": sig["discount"],
                "reason": "historical_discount_lead", "exchangeId": q["exchangeId"],
            }));
        }
    }
    let mut rows: Vec<V> = order.iter().map(|k| priority[k].clone()).collect();
    rows.sort_by(|a, b| {
        num(&b["priority"]).partial_cmp(&num(&a["priority"])).unwrap_or(std::cmp::Ordering::Equal)
            .then(js_str(&a["marketId"]).cmp(&js_str(&b["marketId"])))
    });
    rows.truncate(2);
    Ok(json!(rows))
}

/// dipReport(s, bank, at, coverage)
pub fn dip_report(s: &V, bank: &V, at: &str, coverage: &V) -> Result<V, String> {
    let mut r = crate::model::report(&s["wallet"], &bank["config"], at);
    let label = |id: &V| -> String {
        let mid = &s["wallet"]["books"][js_str(id).as_str()]["marketId"];
        let title = bank["identities"][js_str(mid).as_str()]["title"].clone();
        if truthy(&title) { title.as_str().unwrap_or("").to_string() }
        else {
            let shown = if mid.is_null() { "undefined".to_string() } else { js_str(mid) };
            format!("Market {shown}")
        }
    };
    {
        let v = &mut r["strategies"]["mean_reversion"];
        if let Some(positions) = v["positions"].as_array_mut() {
            for pos in positions.iter_mut() {
                pos["label"] = json!(label(&pos["instrument"]));
            }
        }
        let pending = s["wallet"]["strategies"]["mean_reversion"]["pending"]
            .as_object().cloned().unwrap_or_default();
        let pending_rows: Vec<V> = pending.iter().map(|(instrument, p)| {
            let mut o = p.as_object().cloned().unwrap_or_default();
            o.insert("instrument".into(), json!(instrument));
            o.insert("label".into(), json!(label(&json!(instrument))));
            V::Object(o)
        }).collect();
        v["pending"] = json!(pending_rows);
        if let Some(fills) = v["recentFills"].as_array_mut() {
            for f in fills.iter_mut() {
                f["label"] = json!(label(&f["instrument"]));
            }
        }
    }
    let v = r["strategies"]["mean_reversion"].clone();
    let now_ms = parse(&json!(at));
    Ok(json!({
        "mode": "diversified_historical_dip_paper", "updatedAt": at, "feeBps": 0,
        "bankHash": s["bankHash"], "cutoff": bank["cutoff"],
        "sourceSequence": s["sourceSequence"], "sourceHash": s["lastRecordHash"],
        "observations": r["observations"],
        "universeSize": bank["universe"]["exchanges"].as_array().map(|a| a.len()).unwrap_or(0),
        "historiesAvailable": s["histories"].as_object().map(|o| o.len()).unwrap_or(0),
        "bookHistoriesAvailable": s["bookHistory"].as_object().map(|o| o.len()).unwrap_or(0),
        "signals": s["signals"],
        "depthPlan": dip_depth_plan(s, bank, coverage, now_ms)?,
        "strategy": v,
        "performance": crate::dip_performance::dip_performance(
            s["wallet"]["strategies"]["mean_reversion"]["fills"].as_array().map(|a| a.as_slice()).unwrap_or(&[]),
            v["positions"].as_array().map(|a| a.as_slice()).unwrap_or(&[]), at)?,
        "policy": bank["policy"], "config": bank["config"],
        "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false,
    }))
}
