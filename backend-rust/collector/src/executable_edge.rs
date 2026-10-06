// live-paper/executable-edge.mjs — execution-aware online edge experiment.
use baal_paper::fixed::{big_amount, instant, money, TICK, SCALE};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

/// EDGE_POLICY (frozen in JS).
pub fn edge_policy() -> V {
    json!({"horizonMs":300000,"labelLatenessMs":60000,"trialLifetimeMs":30000,"minimumSamples":25,
        "maximumSamples":256,"minimumMarkets":3,"minimumNetMargin":"0.010000","maximumSpread":"0.030000",
        "cumulativeBuyBudget":"100.000000","maxOpenMarkets":3,"cooldownMs":360000})
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(code.to_string()); }
    Ok(())
}
fn count(s: &mut V, k: &str) {
    s["counts"][k] = json!(s["counts"][k].as_i64().unwrap_or(0) + 1);
}
fn amount(v: &V) -> Result<BigInt, String> { big_amount(v.as_str().unwrap_or("")) }
/// createExecutableEdgeBank({config,universe,cutoff,sourcePrefixBytes,sourcePrefixHash,
///  sourcePrefixSequence,sourcePrefixJournalHash,codeIdentity})
pub fn create_executable_edge_bank(config: &V, universe: &V, cutoff: &str, source_prefix_bytes: i64,
    source_prefix_hash: &str, source_prefix_sequence: i64, source_prefix_journal_hash: &str,
    code_identity: &V) -> Result<V, String> {
    let mut c = config.clone();
    c["strategyUniverse"] = json!("diversified-dip-v1");
    c["marketIds"] = json!(universe["exchanges"].as_array().cloned().unwrap_or_default()
        .iter().map(|x| x["marketId"].clone()).collect::<Vec<V>>());
    c["feeBps"] = json!(0);
    c["slippageTicks"] = json!(1);
    c["depthFractionBps"] = json!(100);
    c["orderShares"] = json!(1);
    c["maxPositionShares"] = json!(1);
    c["maxExposure"] = json!("25.000000");
    c["cashReserve"] = json!(money(&(big_amount(config["initialCash"].as_str().unwrap_or(""))? - BigInt::from(25000000i64))));
    c["exitAfterMs"] = json!(300000);
    c["stopLoss"] = json!("0.020000");
    crate::model::validate_config(&c)?;
    instant(cutoff)?;
    Ok(json!({"schemaVersion":1,"name":"executable-edge-v1","config":c,"universe":universe.clone(),
        "cutoff":cutoff,"sourcePrefixBytes":source_prefix_bytes,"sourcePrefixHash":source_prefix_hash,
        "sourcePrefixSequence":source_prefix_sequence,"sourcePrefixJournalHash":source_prefix_journal_hash,
        "codeIdentity":code_identity.clone(),"policy":edge_policy()}))
}
/// emptyExecutableEdge(bank)
pub fn empty_executable_edge(bank: &V) -> Result<V, String> {
    need(crate::model::digest(&bank["policy"]) == crate::model::digest(&edge_policy()), "EDGE_POLICY_CHANGED")?;
    let mut wallet = crate::model::initial_state(&bank["config"],
        &format!("executable-edge:{}", bank["cutoff"].as_str().unwrap_or("")),
        bank["cutoff"].as_str().unwrap_or(""))?;
    wallet["lastAt"] = json!("1970-01-01T00:00:00.000Z");
    Ok(json!({"bankHash":crate::model::digest(bank),"wallet":wallet,"cells":{},"trials":{},"seen":{},
        "nextTrialAt":{},"cooldowns":{},"counts":{},"decisions":[],"labels":[],"lastClock":V::Null}))
}
/// edgeFeatures(b)
pub fn edge_features(b: &V) -> Result<V, String> {
    let bid = amount(&b["bids"][0]["price"])?;
    let ask = amount(&b["asks"][0]["price"])?;
    let spread = &ask - &bid;
    let bq = BigInt::from(b["bids"][0]["quantity"].as_i64().unwrap_or(0));
    let aq = BigInt::from(b["asks"][0]["quantity"].as_i64().unwrap_or(0));
    let pressure = if &bq * 5 >= (&bq + &aq) * 3 { "bid_heavy" }
        else if &aq * 5 >= (&bq + &aq) * 3 { "ask_heavy" } else { "balanced" };
    let cell = format!("{}:{}",
        if spread <= BigInt::from(3 * TICK) { "tight" } else { "wide" }, pressure);
    Ok(json!({"cell":cell,"spread":money(&spread),
        "topBidQuantity":b["bids"][0]["quantity"],"topAskQuantity":b["asks"][0]["quantity"]}))
}
/// edgeForecast(cell, policy)
pub fn edge_forecast(cell: &V, policy: &V) -> Result<V, String> {
    let samples: Vec<V> = cell["samples"].as_array().cloned().unwrap_or_default();
    let mut values: Vec<f64> = samples.iter().map(|x| x["netMicros"].as_f64().unwrap_or(0.0)).collect();
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let markets: BTreeSet<String> = samples.iter()
        .filter_map(|x| x["marketId"].as_str().map(String::from)).collect();
    let lower_quartile: Option<f64> = if values.is_empty() { None }
        else { Some(values[(values.len() - 1) / 4]) };
    let admitted = values.len() >= policy["minimumSamples"].as_i64().unwrap_or(0) as usize
        && markets.len() >= policy["minimumMarkets"].as_i64().unwrap_or(0) as usize
        && lower_quartile.map(|q| q >= amount(&policy["minimumNetMargin"]).unwrap_or(BigInt::from(0))
            .to_string().parse::<f64>().unwrap_or(0.0)).unwrap_or(false);
    Ok(json!({"samples":values.len(),"markets":markets.len(),
        "lowerQuartile":lower_quartile.map(|q| json!(money(&BigInt::from(q as i64)))).unwrap_or(V::Null),
        "admitted":admitted}))
}
fn revoke(s: &mut V, id: Option<&str>, at: &str, reason: &str) -> Result<(), String> {
    let ids: Vec<String> = match id {
        None => s["trials"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default(),
        Some(m) => vec![m.to_string()],
    };
    for m in ids {
        if !s["trials"][&m].is_null() {
            count(s, &format!("trial_{}", reason));
        }
        s["trials"].as_object_mut().unwrap().remove(&m);
        s["seen"].as_object_mut().unwrap().remove(&m);
    }
    crate::model::invalidate(&mut s["wallet"], id, at, reason);
    let pending = s["wallet"]["strategies"]["mean_reversion"]["pending"].as_object().cloned().unwrap_or_default();
    for (key, i) in pending {
        if i["action"].as_str() == Some("buy")
            && (id.is_none() || s["wallet"]["books"][&key]["marketId"].as_str() == id) {
            s["wallet"]["strategies"]["mean_reversion"]["pending"].as_object_mut().unwrap().remove(&key);
        }
    }
    Ok(())
}
/// advanceExecutableEdge(input, row, bank, {availableAt, allowDecisions})
pub fn advance_executable_edge(input: &V, row: &V, bank: &V, available_at: &str,
    allow_decisions: bool) -> Result<V, String> {
    need(input["bankHash"].as_str() == Some(crate::model::digest(bank).as_str())
        && crate::model::digest(&bank["policy"]) == crate::model::digest(&edge_policy()),
        "EDGE_BANK_CHANGED")?;
    let at = instant(available_at)?;
    need(instant(row["at"].as_str().unwrap_or(""))? <= at, "EDGE_FUTURE_PUBLICATION")?;
    need(input["lastClock"].is_null() || at >= instant(input["lastClock"].as_str().unwrap_or(""))?,
        "EDGE_CLOCK_ROLLBACK")?;
    let mut s = input.clone();
    s["lastClock"] = json!(available_at);
    if ["session_start", "session_stop", "invalidation"].contains(&row["kind"].as_str().unwrap_or("")) {
        let mid = if row["kind"].as_str() == Some("invalidation") {
            row["payload"]["marketId"].as_str()
        } else { None };
        revoke(&mut s, mid, row["at"].as_str().unwrap_or(""), row["kind"].as_str().unwrap_or(""))?;
        return Ok(s);
    }
    if row["kind"].as_str() != Some("book") { return Ok(s); }
    let b = &row["payload"]["book"];
    let id = b["marketId"].as_str().unwrap_or("").to_string();
    let c = &bank["config"];
    let p = &bank["policy"];
    need(bank["universe"]["exchanges"].as_array().is_some_and(|xs| xs.iter()
        .any(|x| x["marketId"].as_str() == Some(id.as_str()) && x["exchangeId"].as_str() == b["exchangeId"].as_str()))
        && b["tournamentId"] == c["tournamentId"], "EDGE_FOREIGN_BOOK")?;
    if at - instant(b["receivedAt"].as_str().unwrap_or(""))? > c["maxDataAgeMs"].as_i64().unwrap_or(0)
        || instant(b["receivedAt"].as_str().unwrap_or(""))? > at {
        count(&mut s, "stale_consumer_book");
        revoke(&mut s, Some(&id), row["at"].as_str().unwrap_or(""), "stale")?;
        return Ok(s);
    }
    let barrier = s["wallet"]["invalidated"][b["instrument"].as_str().unwrap_or("")].clone();
    if !barrier.is_null() {
        need(instant(b["requestStartedAt"].as_str().unwrap_or(""))?
            >= instant(barrier["at"].as_str().unwrap_or(""))?, "EDGE_REQUEST_PRECEDES_BARRIER")?;
        if b["requestStartedAt"] == barrier["at"] {
            s["wallet"]["invalidated"].as_object_mut()
                .map(|m| m.remove(b["instrument"].as_str().unwrap_or("")));
        }
    }
    let prior = s["seen"][&id].clone();
    let independent = prior.is_null() || prior != b["versionKey"];
    s["seen"][&id] = b["versionKey"].clone();
    let f = edge_features(b)?;
    let ask = amount(&b["asks"][0]["price"])? + BigInt::from(TICK);
    let bid = amount(&b["bids"][0]["price"])? - BigInt::from(TICK);
    let depth = b["asks"][0]["quantity"].as_i64().unwrap_or(0) >= 100
        && b["bids"][0]["quantity"].as_i64().unwrap_or(0) >= 100;
    let t = s["trials"][&id].clone();
    if !t.is_null() && t["filledAt"].is_null() {
        if at - instant(t["preparedAt"].as_str().unwrap_or(""))? > p["trialLifetimeMs"].as_i64().unwrap_or(0) {
            count(&mut s, "trial_unfilled");
            s["trials"].as_object_mut().unwrap().remove(&id);
        } else if independent && b["versionKey"] != t["version"]
            && instant(b["requestStartedAt"].as_str().unwrap_or(""))?
                >= instant(t["preparedAt"].as_str().unwrap_or(""))? + c["latencyMs"].as_i64().unwrap_or(0)
            && depth && ask <= amount(&t["limit"])? {
            s["trials"][&id]["filledAt"] = b["receivedAt"].clone();
            s["trials"][&id]["entry"] = json!(money(&ask));
            s["trials"][&id]["fillVersion"] = b["versionKey"].clone();
            count(&mut s, "trial_filled");
        }
    } else if !t.is_null() && !t["filledAt"].is_null()
        && instant(b["requestStartedAt"].as_str().unwrap_or(""))?
            >= instant(t["filledAt"].as_str().unwrap_or(""))? + p["horizonMs"].as_i64().unwrap_or(0) {
        let target = instant(t["filledAt"].as_str().unwrap_or(""))? + p["horizonMs"].as_i64().unwrap_or(0);
        if at - target > p["labelLatenessMs"].as_i64().unwrap_or(0)
            || !independent || b["versionKey"] == t["fillVersion"]
            || !depth || bid < BigInt::from(TICK) {
            count(&mut s, "label_missing");
            s["trials"].as_object_mut().unwrap().remove(&id);
        } else {
            let label = json!({"marketId":id.clone(),"cell":t["cell"],"preparedAt":t["preparedAt"],
                "filledAt":t["filledAt"],"entry":t["entry"],"exitBid":money(&bid),
                "at":b["receivedAt"],"netMicros":(&bid - amount(&t["entry"])?).to_string().parse::<f64>().unwrap_or(0.0),
                "sourceSequence":row["sequence"],"sourceHash":row["hash"],
                "entryVersion":t["fillVersion"],"exitVersion":b["versionKey"]});
            let cell_name = t["cell"].as_str().unwrap_or("").to_string();
            if s["cells"][&cell_name].is_null() { s["cells"][&cell_name] = json!({"samples": []}); }
            let mut samples = s["cells"][&cell_name]["samples"].as_array().cloned().unwrap_or_default();
            samples.push(label.clone());
            if samples.len() as i64 > p["maximumSamples"].as_i64().unwrap_or(0) { samples.remove(0); }
            s["cells"][&cell_name]["samples"] = json!(samples);
            let mut labels = s["labels"].as_array().cloned().unwrap_or_default();
            labels.push(label);
            if labels.len() > 1000 { labels.remove(0); }
            s["labels"] = json!(labels);
            count(&mut s, "labels_completed");
            s["trials"].as_object_mut().unwrap().remove(&id);
        }
    }
    let forecast = edge_forecast(&s["cells"][f["cell"].as_str().unwrap_or("")], p)?;
    let post_cutoff = instant(b["requestStartedAt"].as_str().unwrap_or(""))?
        > instant(bank["cutoff"].as_str().unwrap_or(""))?;
    let current = allow_decisions && post_cutoff;
    let st = s["wallet"]["strategies"]["mean_reversion"].clone();
    let used: BigInt = st["fills"].as_array().cloned().unwrap_or_default().iter()
        .filter(|x| x["action"].as_str() == Some("buy"))
        .map(|x| amount(&x["price"]).unwrap_or(BigInt::from(0)) * BigInt::from(x["quantity"].as_i64().unwrap_or(0)))
        .sum();
    let reserved: BigInt = st["pending"].as_object().cloned().unwrap_or_default().values()
        .filter(|x| x["action"].as_str() == Some("buy"))
        .map(|x| amount(&x["limit"]).unwrap_or(BigInt::from(0)) * BigInt::from(x["quantity"].as_i64().unwrap_or(0)))
        .sum();
    let mut occupied: BTreeSet<String> = st["positions"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    occupied.extend(st["pending"].as_object().map(|m| m.keys().cloned().collect::<Vec<String>>()).unwrap_or_default());
    let instrument = b["instrument"].as_str().unwrap_or("");
    let permitted = current && independent
        && forecast["admitted"].as_bool() == Some(true)
        && depth
        && amount(&f["spread"])? <= amount(&p["maximumSpread"])?
        && ask <= BigInt::from(SCALE - TICK)
        && bid >= BigInt::from(TICK)
        && &used + &reserved + &ask <= amount(&p["cumulativeBuyBudget"])?
        && (occupied.contains(instrument) || occupied.len() < p["maxOpenMarkets"].as_i64().unwrap_or(0) as usize)
        && at >= s["cooldowns"][&id].as_i64().unwrap_or(0);
    if current && independent {
        count(&mut s, if permitted { "entry_gate_allowed" }
            else if forecast["admitted"].as_bool() != Some(true) { "unproven_net_edge" }
            else { "execution_gate_rejected" });
    }
    if !current {
        let strategies = s["wallet"]["strategies"].clone();
        s["wallet"] = crate::model::observe(&s["wallet"], b, c)?;
        s["wallet"]["strategies"] = strategies;
    } else {
        let before = st["intents"].as_i64().unwrap_or(0);
        let was_held = !st["positions"][instrument].is_null();
        let reference = if permitted {
            Some((&ask + amount(&forecast["lowerQuartile"])?).to_string().parse::<f64>().unwrap_or(0.0)
                / SCALE as f64)
        } else { None };
        s["wallet"] = crate::model::step_dip(&s["wallet"], b, c, reference, permitted,
            0.01 - 4.0 * f64::EPSILON, &V::String("0.010000".into()),
            p["horizonMs"].as_i64().unwrap_or(0), &V::String("5.000000".into()), "responsive_v2")?;
        let next = s["wallet"]["strategies"]["mean_reversion"].clone();
        let intent = next["pending"][instrument].clone();
        if !intent.is_null() && intent["id"].as_i64().unwrap_or(0) > before {
            let iid = intent["id"].as_i64().unwrap_or(0).to_string();
            s["wallet"]["strategies"]["mean_reversion"]["pending"][instrument]["createdAt"] = json!(available_at);
            if s["wallet"]["strategies"]["mean_reversion"]["pending"][instrument]["action"].as_str() == Some("buy") {
                s["wallet"]["strategies"]["mean_reversion"]["pending"][instrument]["reason"] = json!("matured_executable_edge");
            }
            let decision = json!({"intentId":intent["id"],"action":intent["action"],"marketId":id.clone(),
                "at":available_at,"sourceSequence":row["sequence"],"sourceHash":row["hash"],
                "version":b["versionKey"],"features":f.clone(),"forecast":forecast,
                "modelHash":crate::model::digest(&s["cells"]),"limit":intent["limit"]});
            let mut decisions = s["decisions"].as_array().cloned().unwrap_or_default();
            decisions.push(decision);
            if decisions.len() > 1000 { decisions.remove(0); }
            s["decisions"] = json!(decisions);
            let _ = iid;
        }
        if was_held && s["wallet"]["strategies"]["mean_reversion"]["positions"][instrument].is_null() {
            s["cooldowns"][&id] = json!(at + p["cooldownMs"].as_i64().unwrap_or(0));
        }
        let spent: BigInt = s["wallet"]["strategies"]["mean_reversion"]["fills"].as_array().cloned().unwrap_or_default().iter()
            .filter(|x| x["action"].as_str() == Some("buy"))
            .map(|x| amount(&x["price"]).unwrap_or(BigInt::from(0)) * BigInt::from(x["quantity"].as_i64().unwrap_or(0)))
            .sum();
        need(spent <= amount(&p["cumulativeBuyBudget"])?, "EDGE_BUDGET_EXCEEDED")?;
        crate::model::assert_state(&s["wallet"], c)?;
    }
    if independent && depth && ask <= BigInt::from(SCALE - TICK) && bid >= BigInt::from(TICK)
        && s["trials"][&id].is_null()
        && at >= s["nextTrialAt"][&id].as_i64().unwrap_or(0) {
        s["trials"][&id] = json!({"cell":f["cell"],"preparedAt":available_at,"limit":money(&ask),
            "version":b["versionKey"]});
        s["nextTrialAt"][&id] = json!(at + p["horizonMs"].as_i64().unwrap_or(0)
            + p["labelLatenessMs"].as_i64().unwrap_or(0));
        count(&mut s, "trial_prepared");
    }
    Ok(s)
}
/// executableEdgeReport(s, bank, at)
pub fn executable_edge_report(s: &V, bank: &V, at: &str) -> Result<V, String> {
    let mut wallet = s["wallet"].clone();
    let at_ms = instant(at)?;
    for (k, b) in wallet["books"].as_object().cloned().unwrap_or_default() {
        if at_ms - instant(b["receivedAt"].as_str().unwrap_or(""))? > bank["config"]["maxDataAgeMs"].as_i64().unwrap_or(0) {
            let market = b["marketId"].as_str().map(String::from);
            crate::model::invalidate(&mut wallet, market.as_deref(), at, "stale_report");
            let _ = k;
        }
    }
    let mut cells = serde_json::Map::new();
    for (k, v) in s["cells"].as_object().cloned().unwrap_or_default() {
        cells.insert(k, edge_forecast(&v, &bank["policy"])?);
    }
    let mut prospective = serde_json::Map::new();
    for (k, v) in s["counts"].as_object().cloned().unwrap_or_default() {
        let dev = s["development"]["counts"][&k].as_i64().unwrap_or(0);
        prospective.insert(k, json!(v.as_i64().unwrap_or(0) - dev));
    }
    let decisions = s["decisions"].as_array().cloned().unwrap_or_default();
    let labels = s["labels"].as_array().cloned().unwrap_or_default();
    Ok(json!({"schemaVersion":1,"mode":"execution_aware_online_paper","bankHash":s["bankHash"],
        "cutoff":bank["cutoff"],"updatedAt":at,"counts":s["counts"],
        "development":if s["development"].is_null() { V::Null } else { s["development"].clone() },
        "prospectiveCounts":prospective,"cells":cells,
        "wallet":crate::model::report(&wallet, &bank["config"], at)["strategies"]["mean_reversion"],
        "recentDecisions":decisions[decisions.len().saturating_sub(20)..].to_vec(),
        "recentLabels":labels[labels.len().saturating_sub(20)..].to_vec(),
        "networkRequests":0,"competitionOrders":0,"feesBps":0,"profitabilityEstablished":false,
        "limitations":[
            "Empirical cell quartiles are hypotheses, not independent confidence bounds; market/common-event observations may correlate.",
            "Trial labels condition on hypothetical causal next-book fills; missing/unfilled observations are counted separately.",
            "All wallet fills remain local simulations with one adverse tick per side and one percent public depth."]}))
}
