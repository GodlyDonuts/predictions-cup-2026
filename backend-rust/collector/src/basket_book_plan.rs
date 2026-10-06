// live-paper/basket-book-plan.mjs — read-only measurement overlay.
use baal_paper::fixed::big_amount;
use baal_service_runtime::contract::{instant_ns, parse_iso_ms, timestamp_str};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::{BTreeMap, BTreeSet};

fn need(ok: bool, msg: &str) -> Result<(), String> { if !ok { return Err(msg.into()); } Ok(()) }
fn hex(s: &V) -> bool { s.as_str().is_some_and(|x| x.len() == 64 && x.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())) }
fn canonical_clock(s: &V) -> bool {
    s.as_str().and_then(parse_iso_ms).is_some_and(|ms| baal_service_runtime::contract::ms_to_iso(ms) == s.as_str().unwrap())
}
fn hash_js(v: &V) -> String { baal_service_runtime::json_text::digest(v) }
fn anchor_hash(x: &V) -> String {
    hash_js(&json!({"marketIds": x["marketIds"], "exchangeIds": x["exchangeIds"], "observerBankHash": x["observerBankHash"],
        "pairId": x["pairId"], "sourceSequence": x["sourceSequence"], "sourceHash": x["sourceHash"], "anchorAt": x["anchorAt"],
        "pairVersions": x["pairVersions"], "pairReceipts": x["pairReceipts"], "sourceMeasurementHash": x["sourceMeasurementHash"]}))
}
fn pair_legs(market_ids: &V, exchange_ids: &V, markets: &BTreeMap<String, V>) -> Result<Vec<V>, String> {
    let ids = market_ids.as_array().cloned().unwrap_or_default();
    let exs = exchange_ids.as_array().cloned().unwrap_or_default();
    if ids.len() != 2 || exs.len() != 2 || ids[0] == ids[1] || exs[0] == exs[1]
        || ids.iter().enumerate().any(|(i, id)| {
            markets.get(id.as_str().unwrap_or("")).map(|m| &m["exchangeId"]) != Some(&exs[i])
        }) {
        return Err("Invalid basket followup scope".into());
    }
    let mut legs: Vec<V> = ids.iter().enumerate().map(|(i, id)| {
        json!({"marketId": id, "exchangeId": exs[i], "index": i})
    }).collect();
    legs.sort_by_key(|l| l["marketId"].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0));
    Ok(legs)
}
fn followup_ledger(raw: Option<&V>, markets: &BTreeMap<String, V>, now: i64) -> Result<V, String> {
    let Some(raw) = raw else { return Ok(json!({"schemaVersion": 1, "claims": {}})); };
    need(raw["schemaVersion"].as_i64() == Some(1) && raw["claims"].as_object().is_some_and(|c| c.len() <= 1000), "Invalid basket followup ledger")?;
    for (key, claim) in raw["claims"].as_object().unwrap() {
        let valid = claim.is_object() && hex(&claim["observerBankHash"]) && hex(&claim["pairId"])
            && key == &hash_js(&json!([claim["observerBankHash"], claim["pairId"]]))
            && claim["sourceSequence"].as_i64().is_some_and(|x| x >= 1)
            && hex(&claim["sourceHash"]) && hex(&claim["anchorHash"]) && hex(&claim["sourceMeasurementHash"])
            && canonical_clock(&claim["anchorAt"]) && canonical_clock(&claim["claimedAt"])
            && parse_iso_ms(claim["claimedAt"].as_str().unwrap()).unwrap() > parse_iso_ms(claim["anchorAt"].as_str().unwrap()).unwrap()
            && parse_iso_ms(claim["claimedAt"].as_str().unwrap()).unwrap() <= now as f64
            && claim["pairReceipts"].as_array().is_some_and(|a| a.len() == 2 && a.iter().all(canonical_clock))
            && claim["pairVersions"].as_array().is_some_and(|a| a.len() == 2 && a.iter().all(|v| v.is_string()));
        need(valid, "Invalid basket followup claim")?;
        pair_legs(&claim["marketIds"], &claim["exchangeIds"], markets)?;
        need(anchor_hash(claim) == claim["anchorHash"].as_str().unwrap_or(""), "Basket followup claim identity mismatch")?;
    }
    Ok(raw.clone())
}
fn positive_depth_followup(raw: &V, scope: &V, markets: &BTreeMap<String, V>, ledger: &V,
    excluded: &BTreeSet<String>, now: i64, max_age_ms: i64, latency_ms: i64) -> Result<Option<V>, String> {
    need(raw.is_object() && raw["tournamentId"] == scope["universe"]["tournamentId"] && hex(&raw["observerBankHash"]) && hex(&raw["pairId"])
        && raw["sourceSequence"].as_i64().is_some_and(|x| x >= 1) && hex(&raw["sourceHash"]) && canonical_clock(&raw["cutoff"]),
        "Invalid basket followup provenance")?;
    let legs = pair_legs(&raw["marketIds"], &raw["exchangeIds"], markets)?;
    let o = &raw["observation"];
    need(o.is_object() && o["pairId"] == raw["pairId"] && canonical_clock(&o["at"]), "Invalid basket followup observation")?;
    let origin = &raw["sourceMeasurement"];
    // Only the initiating measurement's causal plan may seed another read.
    if !(origin.is_object() && origin["reason"].as_str() == Some("paired_depth_measurement") && origin.get("followup").is_none()) {
        return Ok(None);
    }
    let origin_legs = pair_legs(&origin["marketIds"], &origin["exchangeIds"], markets)?;
    need(origin_legs.iter().enumerate().all(|(i, l)| l["marketId"] == legs[i]["marketId"] && l["exchangeId"] == legs[i]["exchangeId"])
        && canonical_clock(&origin["receivedAt"])
        && parse_iso_ms(origin["receivedAt"].as_str().unwrap()).unwrap() <= parse_iso_ms(o["at"].as_str().unwrap()).unwrap()
        && origin["cycle"].as_i64().is_some_and(|x| x >= 0)
        && origin["every"].as_i64().is_some_and(|x| x >= 1)
        && origin["cycle"].as_i64().unwrap() % origin["every"].as_i64().unwrap() == 0, "Invalid basket followup origin")?;
    if !(o["status"].as_str() == Some("apparent_positive_basket") && o["apparentPositive"] == V::Bool(true)
        && o["newPairEvidence"] == V::Bool(true) && o["independentPair"] == V::Bool(false)
        && o["positivePersistenceCount"].as_i64() == Some(1)) {
        return Ok(None);
    }
    need(o["eligibleForExecution"] == V::Bool(false) && o["fills"].is_null() && o["pnl"].is_null() && o["basket"].is_object()
        && o["basket"]["quantity"].as_i64().is_some_and(|q| (1..=100).contains(&q))
        && o["basket"]["depthFractionBps"].as_i64().is_some_and(|d| (1..=100).contains(&d))
        && o["pairReceipts"].as_array().is_some_and(|a| a.len() == 2 && a.iter().all(canonical_clock))
        && o["pairVersions"].as_array().is_some_and(|a| a.len() == 2), "Invalid positive basket depth")?;
    let at = parse_iso_ms(o["at"].as_str().unwrap()).unwrap() as i64;
    let receipts: Vec<i64> = o["pairReceipts"].as_array().unwrap().iter().map(|r| parse_iso_ms(r.as_str().unwrap()).unwrap() as i64).collect();
    if at != receipts.iter().max().copied().unwrap() || (receipts[0] - receipts[1]).abs() > 20000
        || receipts.iter().any(|t| *t <= parse_iso_ms(raw["cutoff"].as_str().unwrap()).unwrap() as i64) {
        return Err("Invalid basket followup clocks".into());
    }
    for (index, key) in o["pairVersions"].as_array().unwrap().iter().enumerate() {
        let k = key.as_str().unwrap_or("");
        let (seq, ts) = k.split_once(':').unwrap_or(("", ""));
        let ok = !seq.is_empty() && (seq == "0" || !seq.starts_with('0')) && seq.bytes().all(|b| b.is_ascii_digit())
            && seq.parse::<i64>().is_ok() && timestamp_str(ts).map(|t| t == ts).unwrap_or(false)
            && instant_ns(ts).map(|ns| ns <= (receipts[index] as i128 + 2000) * 1_000_000).unwrap_or(false);
        if !ok { return Err("Invalid basket followup version".into()); }
        if instant_ns(ts).map(|ns| ns > now as i128 * 1_000_000).unwrap_or(false) { return Ok(None); }
    }
    let cost = big_amount(o["basket"]["totalCost"].as_str().unwrap_or(""))?;
    let margin = big_amount(o["basket"]["conditionalMargin"].as_str().unwrap_or(""))?;
    let unit = big_amount(o["basket"]["marginalUnitCost"].as_str().unwrap_or(""))?;
    need(cost > BigInt::from(0) && cost <= BigInt::from(25_000_000) && margin > BigInt::from(0)
        && &cost + &margin == BigInt::from(o["basket"]["quantity"].as_i64().unwrap()) * BigInt::from(1_000_000)
        && unit > BigInt::from(0) && unit < BigInt::from(1_000_000), "Invalid positive basket economics")?;
    let mut selected = json!({"marketIds": legs.iter().map(|l| l["marketId"].clone()).collect::<Vec<_>>(),
        "exchangeIds": legs.iter().map(|l| l["exchangeId"].clone()).collect::<Vec<_>>(), "receivedAt": o["at"],
        "priceGapMicros": (BigInt::from(1_000_000) - &unit).to_string().parse::<i64>().unwrap_or(0), "lastObserved": 0,
        "followup": {"observerBankHash": raw["observerBankHash"], "pairId": raw["pairId"],
            "sourceSequence": raw["sourceSequence"], "sourceHash": raw["sourceHash"], "anchorAt": o["at"],
            "pairVersions": legs.iter().map(|l| o["pairVersions"][l["index"].as_i64().unwrap() as usize].clone()).collect::<Vec<_>>(),
            "pairReceipts": legs.iter().map(|l| o["pairReceipts"][l["index"].as_i64().unwrap() as usize].clone()).collect::<Vec<_>>(),
            "sourceMeasurementHash": hash_js(&json!({"reason": origin["reason"], "cycle": origin["cycle"], "every": origin["every"],
                "receivedAt": origin["receivedAt"],
                "marketIds": origin_legs.iter().map(|l| l["marketId"].clone()).collect::<Vec<_>>(),
                "exchangeIds": origin_legs.iter().map(|l| l["exchangeId"].clone()).collect::<Vec<_>>()}))}});
    let mut f = selected["followup"].clone();
    f["marketIds"] = selected["marketIds"].clone();
    f["exchangeIds"] = selected["exchangeIds"].clone();
    f["anchorHash"] = json!(anchor_hash(&f));
    let key = hash_js(&json!([raw["observerBankHash"], raw["pairId"]]));
    f["claimKey"] = json!(key);
    selected["followup"] = f.clone();
    let prior = &ledger["claims"][&key];
    if !prior.is_null()
        && raw["sourceSequence"] == prior["sourceSequence"]
        && (raw["sourceHash"] != prior["sourceHash"] || f["anchorHash"] != prior["anchorHash"]) {
        return Err("Conflicting basket followup source".into());
    }
    if !prior.is_null()
        && (raw["sourceSequence"].as_i64().unwrap() <= prior["sourceSequence"].as_i64().unwrap_or(0)
            || at <= parse_iso_ms(prior["anchorAt"].as_str().unwrap()).unwrap() as i64
            || f["pairVersions"].as_array().unwrap().iter().enumerate().all(|(i, v)| *v == prior["pairVersions"][i])) {
        return Ok(None);
    }
    if at > now || now - at > max_age_ms || now <= at + latency_ms
        || selected["marketIds"].as_array().unwrap().iter().any(|id| excluded.contains(id.as_str().unwrap_or(""))) {
        return Ok(None);
    }
    Ok(Some(selected))
}
/// planBasketBooks(basePlan, scope, {candidates, followups, excludedMarketIds, cycle, now}, {every, maxAgeMs, followupMaxAgeMs, followupLatencyMs})
pub fn plan_basket_books(base_plan: &V, scope: &V, candidates: Option<&V>, followups: &V,
    excluded_market_ids: Option<&V>, cycle: i64, now: i64,
    every: i64, max_age_ms: i64, followup_max_age_ms: i64, followup_latency_ms: i64) -> Result<V, String> {
    need(cycle >= 0 && now >= 0 && now <= 8_640_000_000_000_000
        && every >= 1 && (1..=120_000).contains(&max_age_ms)
        && (1..=600_000).contains(&followup_max_age_ms) && (1000..=60_000).contains(&followup_latency_ms),
        "Invalid basket measurement cadence/clock")?;
    let state = crate::book_scheduler::validate_scheduler_state(&base_plan["state"], scope)?;
    let markets: BTreeMap<String, V> = scope["universe"]["exchanges"].as_array().cloned().unwrap_or_default()
        .iter().map(|spec| (spec["marketId"].as_str().unwrap_or("").to_string(), spec.clone())).collect();
    let core: BTreeSet<String> = scope["coreMarketIds"].as_array().cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(String::from)).collect();
    let base_reads = base_plan["reads"].as_array().cloned().unwrap_or_default();
    need(base_reads.len() <= 4, "Invalid basket base book budget")?;
    let mut existing: BTreeMap<String, V> = BTreeMap::new();
    for read in &base_reads {
        let mid = read["marketId"].as_str().unwrap_or("");
        let spec = markets.get(mid);
        need(spec.is_some() && spec.unwrap()["exchangeId"] == read["exchangeId"]
            && !existing.contains_key(mid)
            && read["path"].as_str() == Some(if core.contains(mid) { "sourceSnapshots" } else { "supplementalDepth" }),
            "Invalid or foreign basket base book scope")?;
        existing.insert(mid.to_string(), read.clone());
    }
    let excluded_list: Vec<String> = excluded_market_ids.and_then(|e| e.as_array().cloned()).unwrap_or_default().iter().filter_map(|x| x.as_str().map(String::from)).collect();
    need(excluded_list.iter().all(|id| markets.contains_key(id)), "Invalid or foreign basket exclusions")?;
    let excluded: BTreeSet<String> = excluded_list.iter().cloned().collect();
    let mut output = base_plan.clone();
    let mut ledger = followup_ledger(state.get("basketFollowups"), &markets, now)?;
    need(followups.as_array().is_some_and(|f| f.len() <= 1000), "Invalid basket followups")?;
    let mut followup_pairs: Vec<V> = Vec::new();
    for f in followups.as_array().unwrap() {
        if let Some(p) = positive_depth_followup(f, scope, &markets, &ledger, &excluded, now, followup_max_age_ms, followup_latency_ms)? {
            followup_pairs.push(p);
        }
    }
    let mut followup_sources: BTreeMap<String, String> = BTreeMap::new();
    for pair in &followup_pairs {
        let f = &pair["followup"];
        let key = format!("{}:{}", f["claimKey"].as_str().unwrap(), f["sourceSequence"].as_i64().unwrap());
        if let Some(prev) = followup_sources.get(&key) {
            need(prev == f["anchorHash"].as_str().unwrap_or(""), "Conflicting basket followup source")?;
        }
        followup_sources.insert(key, f["anchorHash"].as_str().unwrap_or("").into());
    }
    let finish = |reason: &str, selected: Option<&V>| -> V {
        let mut m = json!({"reason": reason, "cycle": cycle, "every": every});
        if let Some(sel) = selected {
            m["marketIds"] = sel["marketIds"].clone();
            m["exchangeIds"] = sel["exchangeIds"].clone();
            m["receivedAt"] = sel["receivedAt"].clone();
            m["priceGapMicros"] = sel["priceGapMicros"].clone();
            if let Some(f) = sel.get("followup").filter(|f| !f.is_null()) {
                m["followup"] = f.clone();
                m["persistSchedulerStateBeforeReads"] = json!(true);
            }
        }
        let mut out = output.clone();
        out["basketMeasurement"] = m;
        out
    };
    if candidates.is_none() && followup_pairs.is_empty() { return Ok(finish("hints_unavailable", None)); }
    let mut valid = followup_pairs.clone();
    for candidate in candidates.and_then(|c| c.as_array()).cloned().unwrap_or_default() {
        if !candidate.is_object() { continue; }
        need(candidate["tournamentId"] == scope["universe"]["tournamentId"], "Foreign basket tournament")?;
        let ids = candidate["marketIds"].as_array().cloned().unwrap_or_default();
        let exs = candidate["exchangeIds"].as_array().cloned().unwrap_or_default();
        if ids.len() != 2 || exs.len() != 2 { continue; }
        need(ids[0] != ids[1] && exs[0] != exs[1]
            && ids.iter().enumerate().all(|(i, id)| markets.get(id.as_str().unwrap_or("")).map(|m| &m["exchangeId"]) == Some(&exs[i])),
            "Invalid or foreign basket pair scope")?;
        if !canonical_clock(&candidate["receivedAt"])
            || parse_iso_ms(candidate["receivedAt"].as_str().unwrap()).unwrap() > now as f64
            || now as f64 - parse_iso_ms(candidate["receivedAt"].as_str().unwrap()).unwrap() > max_age_ms as f64
            || !candidate["priceGapMicros"].as_i64().is_some_and(|g| g > 0 && g <= 1_000_000)
            || ids.iter().any(|id| excluded.contains(id.as_str().unwrap_or(""))) {
            continue;
        }
        let mut legs: Vec<V> = ids.iter().enumerate().map(|(i, id)| json!({"marketId": id, "exchangeId": exs[i]})).collect();
        legs.sort_by_key(|l| l["marketId"].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0));
        let last_observed = ids.iter().map(|id| {
            state["bookLastAttempt"][id.as_str().unwrap_or("")].as_i64().unwrap_or(0)
        }).max().unwrap_or(0);
        valid.push(json!({"marketIds": legs.iter().map(|x| x["marketId"].clone()).collect::<Vec<_>>(),
            "exchangeIds": legs.iter().map(|x| x["exchangeId"].clone()).collect::<Vec<_>>(),
            "receivedAt": candidate["receivedAt"], "priceGapMicros": candidate["priceGapMicros"], "lastObserved": last_observed}));
    }
    if cycle % every != 0 { return Ok(finish("cadence_reserved", None)); }
    if valid.is_empty() { return Ok(finish("no_fresh_positive_pair", None)); }
    let replaceable = ["comparison", "discovery"];
    let protected_reads: Vec<&V> = base_reads.iter().filter(|r| !replaceable.contains(&r["reason"].as_str().unwrap_or(""))).collect();
    let protected_ids: BTreeSet<String> = protected_reads.iter().filter_map(|r| r["marketId"].as_str().map(String::from)).collect();
    let budget = base_reads.len();
    let mut fitting: Vec<V> = valid.iter().filter(|pair| {
        let mut set = protected_ids.clone();
        for id in pair["marketIds"].as_array().unwrap() { set.insert(id.as_str().unwrap_or("").into()); }
        set.len() <= budget
    }).cloned().collect();
    if fitting.is_empty() { return Ok(finish("protected_capacity", None)); }
    fitting.sort_by(|a, b| {
        let fa = !a["followup"].is_null(); let fb = !b["followup"].is_null();
        fb.cmp(&fa)
            .then(if fa && fb {
                parse_iso_ms(a["receivedAt"].as_str().unwrap()).unwrap().partial_cmp(&parse_iso_ms(b["receivedAt"].as_str().unwrap()).unwrap()).unwrap()
            } else { std::cmp::Ordering::Equal })
            .then(b["priceGapMicros"].as_i64().cmp(&a["priceGapMicros"].as_i64()))
            .then(a["lastObserved"].as_i64().cmp(&b["lastObserved"].as_i64()))
            .then(a["marketIds"][0].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0).cmp(&b["marketIds"][0].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0)))
            .then(a["marketIds"][1].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0).cmp(&b["marketIds"][1].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0)))
            .then(parse_iso_ms(b["receivedAt"].as_str().unwrap()).unwrap().partial_cmp(&parse_iso_ms(a["receivedAt"].as_str().unwrap()).unwrap()).unwrap())
    });
    let selected = fitting[0].clone();
    let mut reads: Vec<V> = protected_reads.iter().map(|r| (*r).clone()).collect();
    let mut chosen: BTreeSet<String> = protected_ids.clone();
    for market_id in selected["marketIds"].as_array().unwrap() {
        let mid = market_id.as_str().unwrap();
        if !chosen.contains(mid) {
            let read = existing.get(mid);
            let spec = &markets[mid];
            reads.push(read.cloned().unwrap_or_else(|| json!({"marketId": mid, "exchangeId": spec["exchangeId"],
                "option": spec["option"], "path": if core.contains(mid) { "sourceSnapshots" } else { "supplementalDepth" },
                "reason": "basket_measurement"})));
            chosen.insert(mid.to_string());
        }
    }
    for read in &base_reads {
        if reads.len() < budget && !chosen.contains(read["marketId"].as_str().unwrap_or("")) {
            reads.push(read.clone());
            chosen.insert(read["marketId"].as_str().unwrap_or("").into());
        }
    }
    let mut out = finish(if selected.get("followup").is_some_and(|f| !f.is_null()) { "paired_depth_followup" } else { "paired_depth_measurement" }, Some(&selected));
    out["reads"] = json!(reads);
    if !selected["followup"].is_null() {
        let mut claim = selected["followup"].clone();
        claim.as_object_mut().unwrap().remove("claimKey");
        claim["marketIds"] = selected["marketIds"].clone();
        claim["exchangeIds"] = selected["exchangeIds"].clone();
        claim["claimedAt"] = json!(baal_service_runtime::contract::ms_to_iso(now as f64));
        ledger["claims"][selected["followup"]["claimKey"].as_str().unwrap()] = claim;
        need(ledger["claims"].as_object().unwrap().len() <= 1000, "Basket followup ledger capacity reached")?;
        out["state"]["basketFollowups"] = ledger;
    }
    Ok(out)
}
