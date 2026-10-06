// live-paper/limit-entry.mjs — prospective price-protected limit-entry paper.
use baal_paper::fixed::{big_amount, money, TICK, SCALE};
use baal_service_runtime::contract::{instant_ns, ms_to_iso, parse_iso_ms};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

pub fn limit_entry_markets() -> Vec<&'static str> { vec!["377", "354", "277", "305"] }
/// LIMIT_ENTRY_EXECUTION (frozen in JS).
pub fn limit_entry_execution() -> V {
    json!({"name":"price_protected_observed_marketable_v1","feeBps":0,
        "slippageTicks":1,"depthFractionBps":100,"orderShares":10,"maxMarketCost":"25.000000","maxExposure":"100.000000",
        "cumulativeBuyBudget":"100.000000","maxOpenMarkets":3,"latencyMs":1000,"entryLifetimeMs":30000,
        "maximumQuoteAgeMs":20000,"accountContextMaxAgeMs":180000,"takeProfit":"0.010000","stopLoss":"0.030000",
        "maximumHoldMs":3600000,"cooldownMs":60000,
        "admission":"unchanged_native_padded_price_gate; hybrid_adds_conditional_one_tick_lower_limit",
        "execution":"later_independent_marketable_asks; adverse_tick_capped_at_frozen_actual_order_limit",
        "evidence":"counterfactual_executable_opportunity_at_later_receipt_not_proof_of_resting_fill_or_queue_priority"})
}
fn names() -> [&'static str; 2] { ["immediate_dip", "price_protected_hybrid"] }
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn clock_str(s: &str) -> bool {
    parse_iso_ms(s).map(|ms| ms_to_iso(ms) == s).unwrap_or(false)
}
fn clock(x: &V) -> bool { x.as_str().map(clock_str).unwrap_or(false) }
fn parse(v: &V) -> f64 { parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN) }
fn need(ok: bool, message: &str) -> Result<(), String> {
    if !ok { return Err(message.to_string()); }
    Ok(())
}
fn js_str_opt(v: &V) -> Option<String> { v.as_str().map(String::from) }
fn amount(v: &V) -> Result<BigInt, String> { big_amount(v.as_str().unwrap_or("")) }
fn count(object: &mut V, key: &str) {
    object[key] = json!(object[key].as_i64().unwrap_or(0) + 1);
}
fn ledger(v: &V) -> &V { &v["wallet"]["strategies"]["mean_reversion"] }
fn spent(st: &V) -> Result<BigInt, String> {
    let mut n = BigInt::from(0);
    for f in st["fills"].as_array().cloned().unwrap_or_default() {
        if f["action"].as_str() == Some("buy") {
            n += amount(&f["price"])? * BigInt::from(f["quantity"].as_i64().unwrap_or(0));
        }
    }
    Ok(n)
}
fn check_bank(bank: &V) -> Result<(), String> {
    need(bank["schemaVersion"].as_i64() == Some(1)
        && bank["name"].as_str() == Some("limit-entry-experiment-v1")
        && baal_service_runtime::json_text::js_int(&bank["sourceRecords"]).is_some_and(|x| x > 0)
        && hex(&bank["sourceHash"]) && clock_str(bank["cutoff"].as_str().unwrap_or("")),
        "Invalid limit-entry bank")?;
    crate::model::validate_config(&bank["config"])?;
    crate::dip_shadow::validate_dip_policy(&bank["policy"])?;
    let c = &bank["config"];
    need(crate::model::digest(&bank["execution"]) == crate::model::digest(&limit_entry_execution())
        && crate::model::digest(&c["marketIds"]) == crate::model::digest(&json!(limit_entry_markets()))
        && c["feeBps"].as_i64() == Some(0) && c["slippageTicks"].as_i64() == Some(1)
        && c["depthFractionBps"].as_i64() == Some(100) && c["orderShares"].as_i64() == Some(10)
        && c["latencyMs"].as_i64() == Some(1000) && c["maxDataAgeMs"].as_i64().unwrap_or(i64::MAX) <= 20000
        && c["maxExposure"].as_str() == Some("100.000000")
        && amount(&c["cashReserve"])? == amount(&c["initialCash"])? - BigInt::from(100000000i64),
        "Changed limit-entry execution contract")?;
    need(bank["universe"]["tournamentId"] == c["tournamentId"]
        && bank["universe"]["exchanges"].is_array(), "Invalid limit-entry universe")?;
    let exchanges = bank["universe"]["exchanges"].as_array().cloned().unwrap_or_default();
    need(exchanges.iter().map(|x| x["exchangeId"].as_str().unwrap_or("").to_string())
        .collect::<BTreeSet<_>>().len() == exchanges.len(), "Duplicate exchange")?;
    for market_id in limit_entry_markets() {
        let matches: Vec<&V> = exchanges.iter()
            .filter(|x| x["marketId"].as_str() == Some(market_id) && x["option"].as_str() == Some("YES"))
            .collect();
        need(matches.len() == 1
            && matches[0]["exchangeId"].as_str()
                .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && !s.starts_with('0')),
            "Missing scoped comparison market")?;
    }
    need(bank["codeIdentity"].as_object().is_some_and(|m| !m.is_empty()
        && m.values().all(hex)), "Missing code identity")?;
    Ok(())
}
/// createLimitEntry({sourceRecords, sourceHash, cutoff, config, universe, identities, policy, codeIdentity})
pub fn create_limit_entry(source_records: i64, source_hash: &str, cutoff: &str, config: &V,
    universe: &V, identities: &V, policy: &V, code_identity: &V) -> Result<V, String> {
    let mut c = config.clone();
    c["marketIds"] = json!(limit_entry_markets());
    c["feeBps"] = json!(0);
    c["slippageTicks"] = json!(1);
    c["depthFractionBps"] = json!(100);
    c["orderShares"] = json!(10);
    c["maxPositionShares"] = json!(50);
    c["latencyMs"] = json!(1000);
    c["maxExposure"] = json!("100.000000");
    c["cashReserve"] = json!(money(&(amount(&config["initialCash"])? - BigInt::from(100000000i64))));
    c["stopLoss"] = json!("0.030000");
    c["exitAfterMs"] = json!(3600000);
    need(amount(&c["initialCash"])? > BigInt::from(100000000i64), "Insufficient comparison cash")?;
    let bank = json!({"schemaVersion":1,"name":"limit-entry-experiment-v1","sourceRecords":source_records,
        "sourceHash":source_hash,"cutoff":cutoff,"config":c,"universe":universe,"identities":identities,
        "policy":policy,"codeIdentity":code_identity,"execution":limit_entry_execution()});
    check_bank(&bank)?;
    Ok(json!({"bank": bank, "state": empty_limit_entry_state(&bank)?}))
}
/// emptyLimitEntryState(bank)
pub fn empty_limit_entry_state(bank: &V) -> Result<V, String> {
    check_bank(bank)?;
    let mut strategies = serde_json::Map::new();
    for name in names() {
        let mut wallet = crate::model::initial_state(&bank["config"],
            &format!("{}:{}", bank["name"].as_str().unwrap_or(""), name),
            bank["cutoff"].as_str().unwrap_or(""))?;
        wallet["strategies"].as_object_mut().unwrap().remove("buy_hold_benchmark");
        strategies.insert(name.to_string(), json!({"wallet":wallet,"uniqueObservations":0,
            "actualNativeAdmissions":0,"conditionalLimitAdmissions":0,"gateCounts":{},
            "executionGateCounts":{},"contaminationCounts":{},"lastSignals":{},"decisions":{},
            "outcomes":{},"exitObligations":{},"cooldowns":{}}));
    }
    Ok(json!({"schemaVersion":1,"bankHash":crate::model::digest(bank),"sourceSequence":0,
        "lastRecordHash":"","lastSourceAt":V::Null,"lastAvailableAt":V::Null,
        "histories":{},"bookHistory":{},"seen":{},"invalidatedAt":{},
        "counters":{"developmentBooks":0,"prospectiveBooks":0,"independentBooks":0,"invalidBooks":0,
            "staleAtDecisionBooks":0,"skippedBooks":0,"collectorBarriers":0,"accountOutages":0},
        "strategies":strategies}))
}
fn finish(v: &mut V, id: &str, at: &str, reason: &str) -> Result<(), String> {
    let pending = v["wallet"]["strategies"]["mean_reversion"]["pending"][id].clone();
    if pending.is_null() { return Ok(()); }
    let pid = pending["id"].as_i64().unwrap_or(0).to_string();
    v["outcomes"][&pid]["status"] = json!(reason);
    v["outcomes"][&pid]["resolvedAt"] = json!(at);
    v["outcomes"][&pid]["unfilledQuantity"] = pending["quantity"].clone();
    if pending["action"].as_str() == Some("buy") && reason == "expired" {
        let indep = v["outcomes"][&pid]["independentInWindowReceipts"].as_i64().unwrap_or(0);
        v["outcomes"][&pid]["censoring"] = json!(if indep == 0 {
            "no_independent_in_window_quote"
        } else { "observed_without_full_execution" });
        count(&mut v["executionGateCounts"],
            if indep == 0 { "entry_expired_unobserved" } else { "entry_expired_observed" });
    }
    if reason != "filled" {
        let e = &mut v["wallet"]["strategies"]["mean_reversion"]["expiredIntents"];
        *e = json!(e.as_i64().unwrap_or(0) + 1);
    }
    v["wallet"]["strategies"]["mean_reversion"]["pending"].as_object_mut().unwrap().remove(id);
    Ok(())
}
fn revoke(s: &mut V, bank: &V, market_id: Option<&str>, at: &str, reason: &str) -> Result<(), String> {
    let _ = bank;
    for id in limit_entry_markets() {
        if market_id.is_some_and(|m| m != id) { continue; }
        s["invalidatedAt"][id] = json!(at);
        let names: Vec<String> = s["strategies"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
        for name in names {
            let v = &mut s["strategies"][&name];
            crate::model::invalidate(&mut v["wallet"], Some(id), at, reason);
            let pending = v["wallet"]["strategies"]["mean_reversion"]["pending"].as_object().cloned().unwrap_or_default();
            for (instrument, p) in pending {
                if p["action"].as_str() == Some("buy") && p["marketId"].as_str() == Some(id) {
                    finish(v, &instrument, at, &format!("invalidated_{}", reason))?;
                }
            }
        }
    }
    Ok(())
}
fn checked_book(s: &mut V, snap: &V, id: &str, bank: &V, available_at: &str) -> Result<(V, bool), String> {
    let expected_endpoint = format!("/markets/{}/orderbook?tournamentId={}&depth=20",
        id, bank["config"]["tournamentId"].as_str().unwrap_or(""));
    need(snap["endpoint"].as_str() == Some(expected_endpoint.as_str())
        && clock_str(snap["receivedAt"].as_str().unwrap_or(""))
        && clock_str(snap["requestStartedAt"].as_str().unwrap_or(""))
        && parse(&snap["receivedAt"]) <= parse(&V::String(available_at.to_string())),
        "Invalid book scope/receipt")?;
    let b = crate::model::normalize_book(&snap["data"], id, bank["config"]["tournamentId"].as_str().unwrap_or(""),
        snap["receivedAt"].as_str().unwrap_or(""), snap["requestStartedAt"].as_str().unwrap_or(""),
        bank["config"]["maxDataAgeMs"].as_i64().unwrap_or(0), &BTreeSet::new(), "authoritative_receipt_v1")?;
    let spec_exchange = bank["universe"]["exchanges"].as_array().cloned().unwrap_or_default().iter()
        .find(|x| x["marketId"].as_str() == Some(id) && x["option"].as_str() == Some("YES"))
        .and_then(|x| x["exchangeId"].as_str().map(String::from));
    need(b["exchangeId"].as_str() == spec_exchange.as_deref(), "Foreign exchange")?;
    let old = s["seen"][id].clone();
    let content = crate::model::digest(&json!({"bids": b["bids"], "asks": b["asks"]}));
    if !old.is_null() {
        need(parse(&b["requestStartedAt"]) >= parse(&old["requestStartedAt"])
            && parse(&b["receivedAt"]) >= parse(&old["receivedAt"])
            && b["sequence"].as_str().unwrap_or("").parse::<i128>().unwrap_or(i128::MIN)
                >= old["sequence"].as_str().unwrap_or("").parse::<i128>().unwrap_or(i128::MIN)
            && instant_ns(b["sourceAt"].as_str().unwrap_or("")).unwrap_or(i128::MIN)
                >= instant_ns(old["sourceAt"].as_str().unwrap_or("")).unwrap_or(i128::MIN),
            "Book rollback")?;
        if b["sequence"] == old["sequence"] {
            need(b["versionKey"] == old["versionKey"] && content == old["content"].as_str().unwrap_or(""),
                "Same sequence changed identity/content")?;
        }
    }
    if !s["invalidatedAt"][id].is_null() {
        need(parse(&b["requestStartedAt"]) > parse(&s["invalidatedAt"][id]), "Pre-invalidation book")?;
    }
    let independent = old.is_null()
        || (b["versionKey"] != old["versionKey"]
            && instant_ns(b["sourceAt"].as_str().unwrap_or("")).unwrap_or(i128::MIN)
                > instant_ns(old["sourceAt"].as_str().unwrap_or("")).unwrap_or(i128::MIN));
    s["seen"][id] = json!({"sequence":b["sequence"],"sourceAt":b["sourceAt"],"versionKey":b["versionKey"],
        "content":content,"requestStartedAt":b["requestStartedAt"],"receivedAt":b["receivedAt"]});
    Ok((b, independent))
}
fn enqueue(v: &mut V, b: &V, record: &V, at: &str, action: &str, quantity: i64, limit: &BigInt,
    reason: &str, reference: Option<&V>, signal: Option<&V>, contamination: &str, bank: &V) {
    let st = &mut v["wallet"]["strategies"]["mean_reversion"];
    let id = st["intents"].as_i64().unwrap_or(0) + 1;
    st["intents"] = json!(id);
    let expires_at = if action == "buy" {
        V::String(ms_to_iso(parse(&V::String(at.to_string())) + bank["execution"]["entryLifetimeMs"].as_f64().unwrap_or(0.0)))
    } else { V::Null };
    let pending = json!({"id":id,"marketId":b["marketId"],"instrument":b["instrument"],"action":action,
        "quantity":quantity,"limit":money(limit),"reason":reason,"createdAt":at,"expiresAt":expires_at,
        "sourceVersion":b["versionKey"]});
    st["pending"][b["instrument"].as_str().unwrap_or("")] = pending.clone();
    let mut decision = pending;
    decision["quantity"] = json!(quantity);
    decision["sourceSequence"] = record["sequence"].clone();
    decision["sourceHash"] = record["hash"].clone();
    decision["quoteReceivedAt"] = b["receivedAt"].clone();
    decision["requestStartedAt"] = b["requestStartedAt"].clone();
    decision["reference"] = reference.cloned().unwrap_or(V::Null);
    decision["admission"] = signal.cloned().unwrap_or(V::Null);
    decision["contamination"] = json!(contamination);
    v["decisions"][id.to_string()] = decision;
    v["outcomes"][id.to_string()] = json!({"status":"open","filledQuantity":0,"unfilledQuantity":quantity,
        "independentInWindowReceipts":0,"marketableReceipts":0,"firstEligibleReceiptAt":V::Null,
        "lastEligibleReceiptAt":V::Null});
}
fn fill(v: &mut V, b: &V, independent: bool, record: &V, bank: &V) -> Result<(), String> {
    let st = v["wallet"]["strategies"]["mean_reversion"].clone();
    let instrument = b["instrument"].as_str().unwrap_or("").to_string();
    let p = st["pending"][&instrument].clone();
    if p.is_null() || !independent || st["consumedVersions"][&instrument] == b["versionKey"]
        || b["versionKey"] == p["sourceVersion"]
        || parse(&b["requestStartedAt"]) < parse(&p["createdAt"]) + bank["execution"]["latencyMs"].as_f64().unwrap_or(0.0) {
        return Ok(());
    }
    if p["action"].as_str() == Some("buy") && parse(&b["receivedAt"]) >= parse(&p["expiresAt"]) {
        return Ok(());
    }
    let pid = p["id"].as_i64().unwrap_or(0).to_string();
    {
        let outcome = &mut v["outcomes"][&pid];
        outcome["independentInWindowReceipts"] = json!(outcome["independentInWindowReceipts"].as_i64().unwrap_or(0) + 1);
        if outcome["firstEligibleReceiptAt"].is_null() { outcome["firstEligibleReceiptAt"] = b["receivedAt"].clone(); }
        outcome["lastEligibleReceiptAt"] = b["receivedAt"].clone();
    }
    let limit = amount(&p["limit"])?;
    drop(st);
    let is_buy = p["action"].as_str() == Some("buy");
    let rows: Vec<V> = if is_buy { b["asks"].clone() } else { b["bids"].clone() }
        .as_array().cloned().unwrap_or_default();
    if rows.is_empty() { return Ok(()); }
    if if is_buy { amount(&rows[0]["price"])? > limit } else { amount(&rows[0]["price"])? < limit } {
        return Ok(());
    }
    v["outcomes"][&pid]["marketableReceipts"] = json!(v["outcomes"][&pid]["marketableReceipts"].as_i64().unwrap_or(0) + 1);
    let mut total = 0i64;
    for level in rows {
        let raw = amount(&level["price"])?;
        if if is_buy { raw > limit } else { raw < limit } { break; }
        let price = if is_buy {
            if &raw + BigInt::from(TICK) > limit { limit.clone() } else { &raw + BigInt::from(TICK) }
        } else if &raw - BigInt::from(TICK) < limit { limit.clone() } else { &raw - BigInt::from(TICK) };
        if price < BigInt::from(TICK) || price > BigInt::from(SCALE - TICK) { break; }
        let st = v["wallet"]["strategies"]["mean_reversion"].clone();
        let mut p = st["pending"][&instrument].clone();
        let mut quantity = p["quantity"].as_i64().unwrap_or(0)
            .min((level["quantity"].as_f64().unwrap_or(0.0) / 100.0).floor() as i64);
        if is_buy {
            let exposure: BigInt = st["positions"].as_object().cloned().unwrap_or_default().values()
                .map(|pos| amount(&pos["cost"]).unwrap_or(BigInt::from(0))).sum();
            let pos_qty = st["positions"][&instrument]["quantity"].as_i64().unwrap_or(0);
            let budget_left = amount(&bank["execution"]["cumulativeBuyBudget"])? - spent(&st)?;
            quantity = quantity
                .min(bank["config"]["maxPositionShares"].as_i64().unwrap_or(0) - pos_qty)
                .min(((amount(&st["cash"])? - amount(&bank["config"]["cashReserve"])?) / &price)
                    .to_string().parse::<i64>().unwrap_or(0))
                .min(((amount(&bank["execution"]["maxExposure"])? - exposure) / &price)
                    .to_string().parse::<i64>().unwrap_or(0))
                .min((budget_left / &price).to_string().parse::<i64>().unwrap_or(0));
        } else {
            quantity = quantity.min(st["positions"][&instrument]["quantity"].as_i64().unwrap_or(0));
        }
        if quantity < 1 { continue; }
        let notional = &price * BigInt::from(quantity);
        let decision_reason = v["decisions"][p["id"].as_i64().unwrap_or(0).to_string()]["reason"].clone();
        let mut close_market: Option<String> = None;
        {
        let st_mut = &mut v["wallet"]["strategies"]["mean_reversion"];
        if is_buy {
            let old = st_mut["positions"][&instrument].clone();
            let mut lots = old["lots"].as_array().cloned().unwrap_or_default();
            lots.push(json!({"quantity": quantity, "cost": money(&notional)}));
            st_mut["positions"][&instrument] = json!({
                "quantity": old["quantity"].as_i64().unwrap_or(0) + quantity,
                "cost": money(&(amount(&old["cost"]).unwrap_or(BigInt::from(0)) + &notional)),
                "enteredAt": if old["enteredAt"].is_null() { b["receivedAt"].clone() } else { old["enteredAt"].clone() },
                "lots": lots});
            st_mut["cash"] = json!(money(&(amount(&st_mut["cash"])? - &notional)));
        } else {
            let mut pos = st_mut["positions"][&instrument].clone();
            let mut left = quantity;
            let mut basis = BigInt::from(0);
            while left > 0 {
                let mut lot = pos["lots"][0].clone();
                let lq = lot["quantity"].as_i64().unwrap_or(0);
                let q = left.min(lq);
                let cost = if q == lq { amount(&lot["cost"])? }
                    else { amount(&lot["cost"])? * BigInt::from(q) / BigInt::from(lq) };
                lot["quantity"] = json!(lq - q);
                lot["cost"] = json!(money(&(amount(&lot["cost"])? - &cost)));
                basis += &cost;
                left -= q;
                let mut lots = pos["lots"].as_array().cloned().unwrap_or_default();
                lots[0] = lot;
                if lots[0]["quantity"].as_i64().unwrap_or(0) == 0 { lots.remove(0); }
                pos["lots"] = json!(lots);
            }
            pos["quantity"] = json!(pos["quantity"].as_i64().unwrap_or(0) - quantity);
            pos["cost"] = json!(money(&(amount(&pos["cost"])? - &basis)));
            st_mut["positions"][&instrument] = pos;
            st_mut["cash"] = json!(money(&(amount(&st_mut["cash"])? + &notional)));
            st_mut["realized"] = json!(money(&(amount(&st_mut["realized"])? + &notional - &basis)));
            if st_mut["positions"][&instrument]["quantity"].as_i64().unwrap_or(0) == 0 {
                st_mut["positions"].as_object_mut().unwrap().remove(&instrument);
                st_mut["closedTrades"] = json!(st_mut["closedTrades"].as_i64().unwrap_or(0) + 1);
                close_market = Some(b["marketId"].as_str().unwrap_or("").to_string());
            }
        }
        st_mut["turnover"] = json!(money(&(amount(&st_mut["turnover"])? + &notional)));
        let mut fills = st_mut["fills"].as_array().cloned().unwrap_or_default();
        fills.push(json!({"intentId":p["id"],"instrument":b["instrument"],"marketId":b["marketId"],
            "action":p["action"],"quantity":quantity,"price":money(&price),"fee":"0.000000",
            "at":b["receivedAt"],"evidenceAvailableAt":record["availableAt"],"requestStartedAt":b["requestStartedAt"],
            "sourceAt":b["sourceAt"],"sourceVersion":b["versionKey"],"sourceSequence":record["sequence"],
            "sourceHash":record["hash"],"decisionType":decision_reason,
            "model":"counterfactual_later_marketable_quote_not_resting_execution",
            "depthFractionBps":100,"slippageTicks":1}));
        st_mut["fills"] = json!(fills);
        }
        if let Some(m) = close_market {
            v["exitObligations"].as_object_mut().unwrap().remove(&instrument);
            v["cooldowns"][&m] = json!(ms_to_iso(parse(&b["receivedAt"]) + bank["execution"]["cooldownMs"].as_f64().unwrap_or(0.0)));
        }
        p["quantity"] = json!(p["quantity"].as_i64().unwrap_or(0) - quantity);
        v["wallet"]["strategies"]["mean_reversion"]["pending"][&instrument] = p.clone();
        total += quantity;
        v["outcomes"][&pid]["filledQuantity"] = json!(v["outcomes"][&pid]["filledQuantity"].as_i64().unwrap_or(0) + quantity);
        v["outcomes"][&pid]["unfilledQuantity"] = p["quantity"].clone();
        if p["quantity"].as_i64().unwrap_or(0) == 0 { break; }
    }
    if total > 0 {
        let st_mut = &mut v["wallet"]["strategies"]["mean_reversion"];
        st_mut["consumedVersions"][&instrument] = b["versionKey"].clone();
        let remaining = st_mut["pending"][&instrument]["quantity"].as_i64().unwrap_or(0);
        v["outcomes"][&pid]["status"] = json!(if remaining > 0 { "partially_filled" } else { "filled" });
        if remaining == 0 { finish(v, &instrument, b["receivedAt"].as_str().unwrap_or(""), "filled")?; }
    }
    Ok(())
}
fn run(v: &mut V, name: &str, b: &V, independent: bool, record: &V, bank: &V, reference: &V,
    contamination: &str, evidence: bool, decide: bool) -> Result<(), String> {
    let at = record["availableAt"].as_str().unwrap_or("").to_string();
    let id = b["instrument"].as_str().unwrap_or("").to_string();
    let bid = amount(&b["bids"][0]["price"])?;
    let ask = amount(&b["asks"][0]["price"])?;
    let tick = BigInt::from(TICK);
    let num = |x: &BigInt| x.to_string().parse::<f64>().unwrap_or(0.0);
    let current = crate::dip_shadow::dip_signal(reference, num(&(&ask + &tick)) / 1e6, num(&(&bid - &tick)) / 1e6, &bank["policy"]);
    let parked_price = &ask - &tick;
    let parked = if parked_price >= tick {
        crate::dip_shadow::dip_signal(reference, num(&(&parked_price + &tick)) / 1e6, num(&(&bid - &tick)) / 1e6, &bank["policy"])
    } else { json!({"admitted": false}) };
    if evidence {
        v["wallet"]["books"][&id] = b.clone();
        v["wallet"]["invalidated"].as_object_mut().map(|m| m.remove(&id));
        v["wallet"]["lastAt"] = json!(at);
        v["wallet"]["observations"] = json!(v["wallet"]["observations"].as_i64().unwrap_or(0) + 1);
    }
    if evidence && independent {
        v["uniqueObservations"] = json!(v["uniqueObservations"].as_i64().unwrap_or(0) + 1);
        if current["admitted"].as_bool() == Some(true) {
            v["actualNativeAdmissions"] = json!(v["actualNativeAdmissions"].as_i64().unwrap_or(0) + 1);
        }
        if current["admitted"].as_bool() != Some(true) && parked["admitted"].as_bool() == Some(true) {
            v["conditionalLimitAdmissions"] = json!(v["conditionalLimitAdmissions"].as_i64().unwrap_or(0) + 1);
        }
        count(&mut v["gateCounts"], current["reason"].as_str().unwrap_or(""));
        count(&mut v["contaminationCounts"], contamination);
    }
    v["lastSignals"][b["marketId"].as_str().unwrap_or("")] = json!({"at":at,"quoteReceivedAt":b["receivedAt"],
        "currentNative":current,"conditionalLimit":parked,"proposedParkedLimit":money(&parked_price),
        "contamination":contamination});
    let st = v["wallet"]["strategies"]["mean_reversion"].clone();
    let held_at_start = !st["positions"][&id].is_null();
    let pending_at_start = !st["pending"][&id].is_null();
    if evidence { fill(v, b, independent, record, bank)?; }
    if !decide { return Ok(()); }
    let pos = v["wallet"]["strategies"]["mean_reversion"]["positions"][&id].clone();
    let padded_bid = &bid - &tick;
    if !pos.is_null() {
        let average = amount(&pos["cost"])? / BigInt::from(pos["quantity"].as_i64().unwrap_or(1).max(1));
        if v["exitObligations"][&id].is_null() {
            let reason = if &average - &padded_bid >= amount(&bank["execution"]["stopLoss"])? {
                "stop_loss"
            } else if parse(&V::String(at.clone())) - parse(&pos["enteredAt"])
                >= bank["execution"]["maximumHoldMs"].as_f64().unwrap_or(0.0) {
                "holding_timeout"
            } else { "" };
            if !reason.is_empty() {
                v["exitObligations"][&id] = json!({"reason":reason,"triggeredAt":at});
            }
        }
        let risk = v["exitObligations"][&id].clone();
        let pending = v["wallet"]["strategies"]["mean_reversion"]["pending"][&id].clone();
        let exit_limit = if padded_bid < tick { tick.clone() } else { padded_bid.clone() };
        if !risk.is_null()
            && (pending.is_null() || pending["action"].as_str() == Some("buy")
                || pending["reason"].as_str() != risk["reason"].as_str()
                || exit_limit < amount(&pending["limit"])?) {
            if !pending.is_null() { finish(v, &id, &at, "risk_replaced")?; }
            enqueue(v, b, record, &at, "sell", pos["quantity"].as_i64().unwrap_or(0), &exit_limit,
                risk["reason"].as_str().unwrap_or(""), None, None, contamination, bank);
        } else if pending.is_null() && &padded_bid - &average >= amount(&bank["execution"]["takeProfit"])? {
            enqueue(v, b, record, &at, "sell", pos["quantity"].as_i64().unwrap_or(0), &exit_limit,
                "take_profit", None, None, contamination, bank);
        }
        return Ok(());
    }
    let st = v["wallet"]["strategies"]["mean_reversion"].clone();
    if held_at_start || pending_at_start || !st["pending"][&id].is_null() || !independent {
        return Ok(());
    }
    let decision_type = if current["admitted"].as_bool() == Some(true) { "immediate" }
        else if name == "price_protected_hybrid" && parked["admitted"].as_bool() == Some(true) { "conditional_park" }
        else { "" };
    if decision_type.is_empty() { return Ok(()); }
    if parse(&v["cooldowns"][b["marketId"].as_str().unwrap_or("")]) > parse(&V::String(at.clone())) {
        count(&mut v["executionGateCounts"], "cooldown");
        return Ok(());
    }
    let mut occupied: BTreeSet<String> = st["positions"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    occupied.extend(st["pending"].as_object().map(|m| m.keys().cloned().collect::<Vec<String>>()).unwrap_or_default());
    if occupied.len() >= bank["execution"]["maxOpenMarkets"].as_i64().unwrap_or(0) as usize {
        count(&mut v["executionGateCounts"], "maximum_open_markets");
        return Ok(());
    }
    let limit = if decision_type == "immediate" { ask.clone() } else { parked_price.clone() };
    let reserved: BigInt = st["pending"].as_object().cloned().unwrap_or_default().values()
        .filter(|p| p["action"].as_str() == Some("buy"))
        .map(|p| amount(&p["limit"]).unwrap_or(BigInt::from(0)) * BigInt::from(p["quantity"].as_i64().unwrap_or(0)))
        .sum();
    let exposure: BigInt = st["positions"].as_object().cloned().unwrap_or_default().values()
        .map(|p| amount(&p["cost"]).unwrap_or(BigInt::from(0))).sum();
    let div = |x: BigInt| -> i64 { (x / &limit).to_string().parse::<i64>().unwrap_or(0) };
    let quantity = 10i64
        .min((b["bids"][0]["quantity"].as_f64().unwrap_or(0.0) / 100.0).floor() as i64)
        .min((b["asks"][0]["quantity"].as_f64().unwrap_or(0.0) / 100.0).floor() as i64)
        .min(div(amount(&bank["execution"]["maxMarketCost"])?))
        .min(div(amount(&bank["execution"]["cumulativeBuyBudget"])? - spent(&st)? - &reserved))
        .min(div(amount(&bank["execution"]["maxExposure"])? - exposure - &reserved))
        .min(div(amount(&st["cash"])? - amount(&bank["config"]["cashReserve"])? - &reserved))
        .max(0);
    if quantity == 0 {
        count(&mut v["executionGateCounts"], "depth_or_budget");
        return Ok(());
    }
    enqueue(v, b, record, &at, "buy", quantity, &limit, decision_type, Some(reference),
        Some(if decision_type == "immediate" { &current } else { &parked }), contamination, bank);
    count(&mut v["executionGateCounts"], &format!("{}_order_created", decision_type));
    Ok(())
}
/// advanceLimitEntry(input, record, bank, {availableAt})
pub fn advance_limit_entry(input: &V, record: &V, bank: &V, available_at: Option<&str>) -> Result<V, String> {
    check_bank(bank)?;
    need(input["bankHash"].as_str() == Some(crate::model::digest(bank).as_str()), "Limit-entry bank mismatch")?;
    let mut body = record.clone();
    let hash = body.as_object_mut().and_then(|m| m.shift_remove("hash")).unwrap_or(V::Null);
    let event_at = record["event"]["clockAt"].as_str().filter(|s| !s.is_empty())
        .or_else(|| record["at"].as_str()).unwrap_or("").to_string();
    need(record["sequence"].as_i64() == Some(input["sourceSequence"].as_i64().unwrap_or(0) + 1)
        && record["previous"].as_str() == Some(input["lastRecordHash"].as_str().unwrap_or(""))
        && hex(&hash)
        && crate::model::digest(&body) == hash.as_str().unwrap_or("").to_string(),
        "Source continuity failure")?;
    need(clock_str(record["at"].as_str().unwrap_or("")) && clock_str(&event_at), "Invalid source clock")?;
    let source_at = ms_to_iso(parse(&record["at"]).max(parse(&V::String(event_at.clone()))));
    let prefix = record["sequence"].as_i64().unwrap_or(0) <= bank["sourceRecords"].as_i64().unwrap_or(0);
    need(input["lastSourceAt"].is_null()
        || parse(&V::String(source_at.clone())) >= parse(&input["lastSourceAt"]), "Source clock rollback")?;
    if prefix {
        need(parse(&V::String(source_at.clone())) <= parse(&bank["cutoff"]), "Prefix beyond cutoff")?;
    }
    need((prefix && available_at.is_none()) || available_at.map(clock_str).unwrap_or(false),
        "Prospective source requires host availability clock")?;
    let at = ms_to_iso(parse(&V::String(source_at.clone()))
        .max(parse(&V::String(available_at.unwrap_or(bank["cutoff"].as_str().unwrap_or("")).to_string()))));
    need(input["lastAvailableAt"].is_null()
        || parse(&V::String(at.clone())) >= parse(&input["lastAvailableAt"]), "Host availability clock rollback")?;
    let mut s = input.clone();
    let e = &record["event"];
    let prospective = !prefix && parse(&V::String(source_at.clone())) > parse(&bank["cutoff"]);
    s["sourceSequence"] = record["sequence"].clone();
    s["lastRecordHash"] = hash.clone();
    s["lastSourceAt"] = json!(source_at);
    s["lastAvailableAt"] = json!(at);
    if e["type"].as_str() == Some("market_history") {
        let h = &e["history"];
        let spec = bank["universe"]["exchanges"].as_array().cloned().unwrap_or_default().iter()
            .find(|x| limit_entry_markets().contains(&x["marketId"].as_str().unwrap_or(""))
                && x["marketId"].as_str() == h["marketId"].as_str())
            .cloned();
        if let Some(spec) = spec {
            need(h["exchangeId"].as_str() == spec["exchangeId"].as_str()
                && h["tournamentId"] == bank["config"]["tournamentId"]
                && clock_str(h["receivedAt"].as_str().unwrap_or(""))
                && clock_str(h["requestStartedAt"].as_str().unwrap_or(""))
                && parse(&h["requestStartedAt"]) <= parse(&h["receivedAt"])
                && parse(&h["receivedAt"]) - parse(&h["requestStartedAt"]) <= 20000.0
                && parse(&h["receivedAt"]) <= parse(&V::String(source_at.clone()))
                && h["candles"].as_array().is_some_and(|a| a.len() <= 500),
                "Invalid historical evidence")?;
            s["histories"][h["exchangeId"].as_str().unwrap_or("")] = h.clone();
        }
    } else if e["type"].as_str() == Some("account_read_outage") {
        if prospective {
            s["counters"]["accountOutages"] = json!(s["counters"]["accountOutages"].as_i64().unwrap_or(0) + 1);
        }
        revoke(&mut s, bank, None, &source_at, "account_outage")?;
    } else if ["started", "stopped", "session_start", "session_stop"].contains(&e["type"].as_str().unwrap_or("")) {
        if prospective {
            s["counters"]["collectorBarriers"] = json!(s["counters"]["collectorBarriers"].as_i64().unwrap_or(0) + 1);
        }
        revoke(&mut s, bank, None, &source_at, "collector_barrier")?;
    } else if e["type"].as_str() == Some("cycle") {
        let mut decisions: indexmap::IndexMap<String, (V, bool, V, String)> = indexmap::IndexMap::new();
        let mut accepted: std::collections::HashMap<String, V> = std::collections::HashMap::new();
        let mut snaps: Vec<V> = e["sourceSnapshots"].as_array().cloned().unwrap_or_default();
        snaps.extend(e["supplementalDepth"].as_array().cloned().unwrap_or_default());
        let mut ordered: Vec<(V, usize, String)> = snaps.iter().enumerate().map(|(index, snap)| {
            let at = if baal_service_runtime::json_text::js_truthy(snap.get("skipped")) {
                snap["at"].as_str().unwrap_or("").to_string()
            } else { snap["receivedAt"].as_str().unwrap_or("").to_string() };
            (snap.clone(), index, at)
        }).collect();
        ordered.sort_by(|a, b| {
            let ka = if clock_str(&a.2) { parse_iso_ms(&a.2).unwrap() } else { f64::INFINITY };
            let kb = if clock_str(&b.2) { parse_iso_ms(&b.2).unwrap() } else { f64::INFINITY };
            ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1))
        });
        for (snap, _, _) in ordered {
            let id = snap["marketId"].as_str().filter(|s| !s.is_empty()).map(String::from)
                .or_else(|| {
                    let endpoint = snap["endpoint"].as_str().unwrap_or("");
                    endpoint.strip_prefix("/markets/")
                        .and_then(|rest| rest.split("/orderbook?").next())
                        .filter(|m| !m.is_empty() && m.bytes().all(|b| b.is_ascii_digit()) && !m.starts_with('0'))
                        .map(String::from)
                });
            let Some(id) = id else { continue; };
            if !limit_entry_markets().contains(&id.as_str()) { continue; }
            if baal_service_runtime::json_text::js_truthy(snap.get("skipped")) {
                let prior = accepted.get(&id).cloned();
                if snap["skipped"].as_str() == Some("Exchange has real account holdings or open orders")
                    && prior.is_some()
                    && e["accountExclusions"].as_array().is_some_and(|a| a.iter()
                        .any(|x| x.as_str() == prior.as_ref().unwrap()["exchangeId"].as_str()))
                    && clock_str(snap["at"].as_str().unwrap_or(""))
                    && parse(&snap["at"]) >= parse(&prior.as_ref().unwrap()["receivedAt"])
                    && parse(&snap["at"]) <= parse(&V::String(source_at.clone())) {
                    continue;
                }
                if prospective {
                    s["counters"]["skippedBooks"] = json!(s["counters"]["skippedBooks"].as_i64().unwrap_or(0) + 1);
                }
                decisions.shift_remove(&id);
                revoke(&mut s, bank, Some(&id), &source_at, "read_skipped")?;
                continue;
            }
            let checked = checked_book(&mut s, &snap, &id, bank, &source_at);
            let Ok((b, independent)) = checked else {
                if prospective {
                    s["counters"]["invalidBooks"] = json!(s["counters"]["invalidBooks"].as_i64().unwrap_or(0) + 1);
                }
                decisions.shift_remove(&id);
                revoke(&mut s, bank, Some(&id), &source_at, "invalid_public_book")?;
                continue;
            };
            accepted.insert(id.clone(), b.clone());
            let decision = prospective && parse(&b["requestStartedAt"]) > parse(&bank["cutoff"]);
            let fresh = parse(&V::String(at.clone())) - parse(&b["receivedAt"])
                <= bank["execution"]["maximumQuoteAgeMs"].as_f64().unwrap_or(0.0);
            if !decision {
                s["counters"]["developmentBooks"] = json!(s["counters"]["developmentBooks"].as_i64().unwrap_or(0) + 1);
            } else if !fresh {
                s["counters"]["staleAtDecisionBooks"] = json!(s["counters"]["staleAtDecisionBooks"].as_i64().unwrap_or(0) + 1);
                let mut rec = record.clone();
                rec["availableAt"] = json!(at);
                rec["sourceAvailableAt"] = json!(source_at);
                let names: Vec<String> = s["strategies"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
                for name in names {
                    let v = &mut s["strategies"][&name];
                    v["wallet"]["books"][b["instrument"].as_str().unwrap_or("")] = b.clone();
                    v["wallet"]["invalidated"].as_object_mut()
                        .map(|m| m.remove(b["instrument"].as_str().unwrap_or("")));
                    v["wallet"]["lastAt"] = json!(at);
                    v["wallet"]["observations"] = json!(v["wallet"]["observations"].as_i64().unwrap_or(0) + 1);
                    fill(v, &b, independent, &rec, bank)?;
                }
            } else {
                s["counters"]["prospectiveBooks"] = json!(s["counters"]["prospectiveBooks"].as_i64().unwrap_or(0) + 1);
                if independent {
                    s["counters"]["independentBooks"] = json!(s["counters"]["independentBooks"].as_i64().unwrap_or(0) + 1);
                }
                let eid = b["exchangeId"].as_str().unwrap_or("");
                let reference = baal_service_runtime::history::historical_reference(
                    if s["histories"][eid].is_null() { None } else { Some(&s["histories"][eid]) },
                    s["bookHistory"][eid].as_array().cloned().unwrap_or_default().as_slice(),
                    b["requestStartedAt"].as_str().unwrap_or(""), &bank["policy"])?;
                let context_age = parse(&b["requestStartedAt"]) - parse(&e["exclusionObservedAt"]);
                let known = e["accountExclusions"].as_array().is_some_and(|a| a.iter().all(|x| x.is_string()))
                    && clock_str(e["exclusionObservedAt"].as_str().unwrap_or(""))
                    && context_age >= 0.0
                    && context_age <= bank["execution"]["accountContextMaxAgeMs"].as_f64().unwrap_or(0.0);
                let contamination = if !known { "unknown_account_context" }
                    else if e["accountExclusions"].as_array().is_some_and(|a| a.iter()
                        .any(|x| x.as_str() == Some(eid))) { "account_exposure_or_orders" }
                    else { "no_reported_account_exposure" };
                let mut rec = record.clone();
                rec["availableAt"] = json!(at);
                rec["sourceAvailableAt"] = json!(source_at);
                for name in names() {
                    let v = &mut s["strategies"][name];
                    run(v, name, &b, independent, &rec, bank, &reference, contamination, true, false)?;
                }
                decisions.insert(id.clone(), (b.clone(), independent, reference.clone(), contamination.to_string()));
            }
            if independent {
                let eid = b["exchangeId"].as_str().unwrap_or("").to_string();
                let mut hist = s["bookHistory"][&eid].as_array().cloned().unwrap_or_default();
                hist.push(json!({"at":b["sourceAt"],"receivedAt":b["receivedAt"],
                    "mid":(amount(&b["bids"][0]["price"])? + amount(&b["asks"][0]["price"])?)
                        .to_string().parse::<f64>().unwrap_or(0.0) / 2e6,
                    "versionKey":b["versionKey"]}));
                let cutoff_ms = parse(&b["receivedAt"]) - bank["policy"]["lookbackMs"].as_f64().unwrap_or(0.0);
                let mut kept: Vec<V> = hist.into_iter().filter(|x| parse(&x["at"]) >= cutoff_ms).collect();
                if kept.len() > 512 { kept = kept[kept.len() - 512..].to_vec(); }
                s["bookHistory"][&eid] = json!(kept);
            }
        }
        for (_, (b, independent, reference, contamination)) in &decisions {
            let mut rec = record.clone();
            rec["availableAt"] = json!(at);
            rec["sourceAvailableAt"] = json!(source_at);
            for name in names() {
                let v = &mut s["strategies"][name];
                run(v, name, b, *independent, &rec, bank, reference, contamination, false, true)?;
            }
        }
    }
    if prospective {
        let names: Vec<String> = s["strategies"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
        for name in names {
            let v = &mut s["strategies"][&name];
            let pending = v["wallet"]["strategies"]["mean_reversion"]["pending"].as_object().cloned().unwrap_or_default();
            for (id, pending) in pending {
                if e["type"].as_str() == Some("cycle") && pending["action"].as_str() == Some("buy")
                    && parse(&V::String(source_at.clone())) >= parse(&pending["expiresAt"]) {
                    finish(v, &id, &at, "expired")?;
                }
            }
            let mut cfg = bank["config"].clone();
            cfg["cycleIntervalMs"] = json!(0);
            crate::model::sample_equity(&mut v["wallet"], &cfg, &at);
            crate::model::assert_state(&v["wallet"], &bank["config"])?;
            need(spent(&v["wallet"]["strategies"]["mean_reversion"])?
                <= amount(&bank["execution"]["cumulativeBuyBudget"])?, "Cumulative buy budget exceeded")?;
        }
    }
    Ok(s)
}
/// limitEntryReport(state, bank, at)
pub fn limit_entry_report(state: &V, bank: &V, at: &str) -> Result<V, String> {
    check_bank(bank)?;
    need(state["bankHash"].as_str() == Some(crate::model::digest(bank).as_str())
        && clock_str(at)
        && (state["lastAvailableAt"].is_null() || parse(&V::String(at.to_string())) >= parse(&state["lastAvailableAt"])),
        "Invalid limit-entry report")?;
    let mut strategies = serde_json::Map::new();
    for name in names() {
        let v = &state["strategies"][name];
        let st = ledger(v);
        let mut cfg = bank["config"].clone();
        cfg["cycleIntervalMs"] = json!(0);
        let r = crate::model::report(&v["wallet"], &cfg, at)["strategies"]["mean_reversion"].clone();
        let label = |instrument: &str| -> String {
            let id = v["wallet"]["books"][instrument]["marketId"].as_str().unwrap_or("");
            bank["identities"][id]["title"].as_str().map(String::from)
                .unwrap_or_else(|| format!("Market {} YES", if id.is_empty() { "undefined" } else { id }))
        };
        let mut out = r.clone();
        out["positions"] = json!(r["positions"].as_array().cloned().unwrap_or_default().iter().map(|p| {
            let mut p = p.clone();
            p["label"] = json!(label(p["instrument"].as_str().unwrap_or("")));
            p
        }).collect::<Vec<V>>());
        out["recentFills"] = json!(r["recentFills"].as_array().cloned().unwrap_or_default().iter().map(|f| {
            let mut f = f.clone();
            f["label"] = json!(label(f["instrument"].as_str().unwrap_or("")));
            f
        }).collect::<Vec<V>>());
        out["pending"] = json!(st["pending"].as_object().cloned().unwrap_or_default().iter().map(|(instrument, p)| {
            let mut p = p.clone();
            p["instrument"] = json!(instrument);
            p["remaining"] = p["quantity"].clone();
            p["label"] = json!(label(instrument));
            p["clockExpiredAwaitingSource"] = json!(p["action"].as_str() == Some("buy")
                && parse(&V::String(at.to_string())) >= parse(&p["expiresAt"]));
            p
        }).collect::<Vec<V>>());
        out["cumulativeBuyCost"] = json!(money(&spent(st)?));
        out["uniqueObservations"] = v["uniqueObservations"].clone();
        out["actualNativeAdmissions"] = v["actualNativeAdmissions"].clone();
        out["conditionalLimitAdmissions"] = v["conditionalLimitAdmissions"].clone();
        out["gateCounts"] = v["gateCounts"].clone();
        out["executionGateCounts"] = v["executionGateCounts"].clone();
        out["contaminationCounts"] = v["contaminationCounts"].clone();
        out["lastSignals"] = v["lastSignals"].clone();
        out["exitObligations"] = v["exitObligations"].clone();
        let outcomes: Vec<V> = v["outcomes"].as_object().cloned().unwrap_or_default().values().cloned().collect();
        let orders: Vec<V> = v["decisions"].as_object().cloned().unwrap_or_default().values()
            .filter(|x| x["action"].as_str() == Some("buy")).cloned().collect();
        out["entryOutcomes"] = json!({
            "orders": orders.len(),
            "immediate": orders.iter().filter(|x| x["reason"].as_str() == Some("immediate")).count(),
            "conditionalPark": orders.iter().filter(|x| x["reason"].as_str() == Some("conditional_park")).count(),
            "expiredUnobserved": outcomes.iter().filter(|x| x["status"].as_str() == Some("expired")
                && x["censoring"].as_str() == Some("no_independent_in_window_quote")).count(),
            "expiredObserved": outcomes.iter().filter(|x| x["status"].as_str() == Some("expired")
                && x["censoring"].as_str() == Some("observed_without_full_execution")).count(),
            "unresolved": st["pending"].as_object().map(|m| m.values()
                .filter(|x| x["action"].as_str() == Some("buy")).count()).unwrap_or(0),
            "partiallyFilledOpen": outcomes.iter().filter(|x| x["status"].as_str() == Some("partially_filled")).count()});
        let all_decisions: Vec<V> = v["decisions"].as_object().cloned().unwrap_or_default().values().cloned().collect();
        let start = all_decisions.len().saturating_sub(20);
        out["recentDecisions"] = json!(all_decisions[start..].iter().map(|x| {
            let mut x = x.clone();
            x["outcome"] = v["outcomes"][x["id"].as_i64().unwrap_or(0).to_string()].clone();
            x
        }).collect::<Vec<V>>());
        strategies.insert(name.to_string(), out);
    }
    Ok(json!({"schemaVersion":1,"mode":"prospective_price_protected_limit_entry_paper",
        "cohortId":format!("{}:{}",bank["name"].as_str().unwrap_or(""),bank["cutoff"].as_str().unwrap_or("")),
        "updatedAt":at,"cutoff":bank["cutoff"],"bankHash":state["bankHash"],
        "sourceSequence":state["sourceSequence"],"sourceHash":state["lastRecordHash"],
        "marketIds":limit_entry_markets(),"execution":bank["execution"],"counters":state["counters"],
        "strategies":strategies,"networkRequests":0,"competitionOrders":0,"profitabilityEstablished":false,
        "assumptions":[
            "Full policy comparison: hybrid adds conditional-price opportunities; it is not an isolated execution-effect estimate.",
            "Both arms submit the same frozen actual ask limit when the current padded native dip gate admits.",
            "Hybrid otherwise tests a one-tick-lower actual limit with the original padded-price gates; committed orders are not filtered by later outcomes.",
            "A later independently requested marketable quote supports a counterfactual fill at that receipt only, not resting execution or queue priority.",
            "Thirty-second entries require request after decision availability plus one second and receipt strictly before expiry.",
            "One percent per displayed eligible level, one adverse tick capped at the actual order limit, zero added fees; wallets reuse public liquidity independently.",
            "Account contamination remains labeled; quote touch, cancellations, missing observations and expired quotes are not fills."]}))
}
