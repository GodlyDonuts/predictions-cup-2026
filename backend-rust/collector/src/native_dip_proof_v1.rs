// live-paper/native-dip-proof-v1.mjs — reconcile frozen native dip paper output.
use baal_paper::fixed::big_amount;
use baal_execution::core::payload_hash;
use baal_service_runtime::contract::parse_iso_ms;
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("NATIVE_DIP_{}", code)); } Ok(()) }
fn hash_str(v: &V) -> bool { v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())) }
/// micros(x) — strict canonical signed decimal (unlike big_amount which is unsigned-only in some contexts).
pub fn micros(x: &V) -> Result<BigInt, String> {
    let s = x.as_str().unwrap_or("");
    let body = if s.starts_with('-') { &s[1..] } else { s };
    let Some((a, b)) = body.split_once('.') else { return Err("NATIVE_DIP_MONEY".into()); };
    need(!a.is_empty() && b.len() == 6 && a.bytes().all(|c| c.is_ascii_digit()) && b.bytes().all(|c| c.is_ascii_digit())
        && (a == "0" || !a.starts_with('0')), "MONEY")?;
    let v = a.parse::<BigInt>().unwrap() * 1_000_000u64 + b.parse::<BigInt>().unwrap();
    Ok(if s.starts_with('-') { -v } else { v })
}
fn count(v: &V) -> bool { v.as_i64().is_some_and(|x| x >= 0) }
fn card_key(c: &V) -> String {
    format!("{}/{}/{}", c["strategy"].as_str().unwrap_or(""), c["scenario"]["id"].as_str().unwrap_or(""), c["exitPolicy"].as_str().unwrap_or(""))
}
/// verifyNativeDipResult({result, declaration, binding, universe, previous})
pub fn verify_native_dip_result(result: &V, declaration: &V, binding: &V, universe: &V, previous: Option<&V>) -> Result<V, String> {
    let cap = &result["capture"];
    let e = &result["evaluation"];
    let cfg = &declaration["config"];
    let start = parse_iso_ms(declaration["declaredAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
    let through = parse_iso_ms(e["evaluationThrough"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
    need(binding["schemaVersion"].as_i64() == Some(1)
        && ["binaryHash", "engineHash", "runtimeLibraryHash", "declarationHash"].iter().all(|k| hash_str(&binding[*k])), "BINDING")?;
    need(declaration["schemaVersion"].as_i64() == Some(1) && declaration["mode"].as_str() == Some("causal_dip_subsequent_v2")
        && declaration["cohort"] == binding["cohort"] && cfg["fillClockPolicy"].as_str() == Some("independent_request_latency_v2")
        && cfg["feeBps"].as_i64() == Some(0) && !start.is_nan(), "DECLARATION")?;
    need(result["declarationFileHash"] == binding["declarationHash"] && cap["engine_hash"] == binding["engineHash"]
        && hash_str(&cap["source_hash"]) && count(&cap["source_records"]) && count(&cap["source_bytes"])
        && cap["source_bytes"].as_i64().unwrap() > 0 && cap["through"].as_i64().is_some()
        && cap["through"].as_f64() == Some(through) && through > start, "CAPTURE")?;
    need(e["mode"] == declaration["mode"] && e["cohort"] == binding["cohort"] && e["evaluationStart"] == declaration["declaredAt"]
        && e["networkRequests"].as_i64() == Some(0) && e["competitionOrders"].as_i64() == Some(0)
        && e["profitabilityEstablished"] == V::Bool(false) && count(&e["subsequentRecords"]) && count(&e["subsequentCycles"]), "OUTPUT")?;
    let mut expected: HashMap<String, (V, V)> = HashMap::new();
    for s in declaration["strategies"].as_array().cloned().unwrap_or_default() {
        for scenario in declaration["scenarios"].as_array().cloned().unwrap_or_default() {
            for exit in ["snapshot_limit_v1", "profit_floor_stop_buffer_v1"] {
                expected.insert(format!("{}/{}/{}", s["id"].as_str().unwrap_or(""), scenario["id"].as_str().unwrap_or(""), exit), (s.clone(), scenario.clone()));
            }
        }
    }
    need(e["scorecards"].as_array().is_some_and(|s| s.len() == expected.len()) && e["candidates"].as_i64() == Some(expected.len() as i64), "CARD_COUNT")?;
    let mut scope: HashMap<String, String> = HashMap::new();
    for row in universe["exchanges"].as_array().unwrap() {
        need(row["option"].as_str() == Some("YES") && !scope.contains_key(row["exchangeId"].as_str().unwrap_or("")), "UNIVERSE")?;
        scope.insert(row["exchangeId"].as_str().unwrap().to_string(), row["marketId"].as_str().unwrap().to_string());
    }
    let prior: HashMap<String, V> = previous.map(|p| p["evaluation"]["scorecards"].as_array().cloned().unwrap_or_default()
        .into_iter().map(|c| (card_key(&c), c)).collect()).unwrap_or_default();
    if let Some(prev) = previous {
        need(cap["source_bytes"].as_i64() >= prev["capture"]["source_bytes"].as_i64()
            && cap["source_records"].as_i64() >= prev["capture"]["source_records"].as_i64()
            && cap["through"].as_i64() >= prev["capture"]["through"].as_i64(), "SOURCE_REGRESSION")?;
    }
    let mut seen = BTreeSet::new();
    let mut held = BTreeSet::new();
    let mut events = BTreeSet::new();
    let mut cards = Vec::new();
    for card in e["scorecards"].as_array().cloned().unwrap_or_default() {
        let key = card_key(&card);
        let spec = expected.get(&key).cloned();
        let p = &card["point"];
        let old = prior.get(&key);
        need(spec.as_ref().is_some_and(|(s, sc)| payload_hash(&card["parameters"]).ok() == payload_hash(&s["parameters"]).ok()
            && payload_hash(&card["scenario"]).ok() == payload_hash(sc).ok())
            && !seen.contains(&key), "CARD_IDENTITY")?;
        seen.insert(key.clone());
        need(p["at"] == e["evaluationThrough"] && p["fillClockPolicy"] == cfg["fillClockPolicy"]
            && card["fills"].is_array() && p["fillExecutions"].as_i64() == card["fills"].as_array().map(|f| f.len() as i64)
            && count(&p["openShares"]) && count(&p["completedTrades"])
            && micros(&p["fees"]).unwrap_or(BigInt::from(1)) == BigInt::from(0), "POINT")?;
        if let Some(old) = old {
            let old_fills = old["fills"].as_array().cloned().unwrap_or_default();
            let fills = card["fills"].as_array().cloned().unwrap_or_default();
            need(fills.len() >= old_fills.len() && old_fills.iter().enumerate().all(|(i, f)| serde_json::to_string(f).unwrap() == serde_json::to_string(&fills[i]).unwrap()), "FILL_HISTORY_CHANGED")?;
        }
        let mut lots: HashMap<String, std::collections::VecDeque<(i64, BigInt)>> = HashMap::new();
        let mut cash = micros(&cfg["initialCash"])?;
        let mut realized = BigInt::from(0);
        let mut turnover = BigInt::from(0);
        let (mut sold, mut open, mut last) = (0i64, 0i64, start as i64);
        for f in card["fills"].as_array().unwrap() {
            let q = &f["sourceClock"];
            let a = &f["economics"];
            let instrument = f["instrument"].as_str().unwrap_or("");
            let market = instrument.strip_suffix(":YES").and_then(|e| scope.get(e));
            need(market.is_some() && cfg["marketIds"].as_array().unwrap().contains(&json!(market.unwrap()))
                && ["buy", "sell"].contains(&f["action"].as_str().unwrap_or(""))
                && f["quantity"].as_i64().is_some_and(|x| x > 0)
                && f["at"].as_i64().is_some_and(|t| t as f64 > start && t >= last && t as f64 <= through)
                && micros(&f["fee"]).unwrap_or(BigInt::from(1)) == BigInt::from(0), "FILL")?;
            let fat = f["at"].as_i64().unwrap();
            last = fat;
            need(q["policy"] == cfg["fillClockPolicy"]
                && [q["decisionAt"].clone(), q["minimumLatencyMs"].clone(), q["requestStartedAt"].clone(), q["receivedAt"].clone()].iter().all(|x| x.is_i64())
                && q["minimumLatencyMs"] == card["scenario"]["latencyMs"]
                && q["decisionAt"].as_f64().unwrap() > start
                && q["requestStartedAt"].as_i64().unwrap() >= q["decisionAt"].as_i64().unwrap() + q["minimumLatencyMs"].as_i64().unwrap()
                && q["receivedAt"].as_i64() == f["at"].as_i64()
                && q["receivedAt"].as_i64().unwrap() >= q["requestStartedAt"].as_i64().unwrap()
                && q["receivedAt"].as_i64().unwrap() - q["requestStartedAt"].as_i64().unwrap() <= cfg["maxDataAgeMs"].as_i64().unwrap(), "CLOCK")?;
            for k in ["decisionSequence", "decisionSourceNs", "fillSequence", "fillSourceNs"] {
                need(q[k].as_str().is_some_and(|s| !s.is_empty() && (s == "0" || !s.starts_with('0')) && s.bytes().all(|b| b.is_ascii_digit())), "CLOCK_PRECISION")?;
            }
            need(q["fillSequence"] != q["decisionSequence"] || q["fillSourceNs"] != q["decisionSourceNs"], "CLOCK_VERSION")?;
            need(q["fillSourceNs"].as_str().unwrap().parse::<BigInt>().unwrap() <= BigInt::from(fat + 2000) * BigInt::from(1_000_000)
                && q["fillSourceNs"].as_str().unwrap().parse::<BigInt>().unwrap() >= BigInt::from(fat - cfg["maxDataAgeMs"].as_i64().unwrap()) * BigInt::from(1_000_000), "SOURCE_CLOCK")?;
            need(a["decisionAt"] == q["decisionAt"] && a["reason"].is_string(), "ECONOMICS")?;
            let price = micros(&f["price"])?;
            need(price >= BigInt::from(5000) && price <= BigInt::from(995_000) && &price % BigInt::from(5000) == BigInt::from(0), "PRICE")?;
            let qty = BigInt::from(f["quantity"].as_i64().unwrap());
            turnover += &price * &qty;
            let queue = lots.entry(instrument.to_string()).or_default();
            if f["action"].as_str() == Some("buy") {
                cash -= &price * &qty;
                queue.push_back((f["quantity"].as_i64().unwrap(), price.clone()));
                open += f["quantity"].as_i64().unwrap();
            } else {
                let mut left = f["quantity"].as_i64().unwrap();
                cash += &price * &qty;
                sold += f["quantity"].as_i64().unwrap();
                open -= f["quantity"].as_i64().unwrap();
                while left > 0 {
                    need(!queue.is_empty(), "OVERSELL")?;
                    let lot = queue.front_mut().unwrap();
                    let n = left.min(lot.0);
                    realized += (&price - &lot.1) * BigInt::from(n);
                    left -= n;
                    lot.0 -= n;
                    if lot.0 == 0 { queue.pop_front(); }
                }
            }
            need(cash >= BigInt::from(0) && open >= 0, "WALLET")?;
            events.insert(json!([instrument, f["action"], f["at"], f["price"], f["quantity"]]).to_string());
        }
        need(cash == micros(&p["cash"])? && realized == micros(&p["realizedPnl"])? && turnover == micros(&p["turnover"])?
            && open == p["openShares"].as_i64().unwrap_or(-1) && realized == micros(&card["economics"]["netPnl"])?
            && sold == card["economics"]["matchedShares"].as_i64().unwrap_or(-1)
            && micros(&card["economics"]["fees"])? == BigInt::from(0), "RECONCILIATION")?;
        for (instrument, queue) in &lots {
            if !queue.is_empty() {
                held.insert(scope.get(instrument.split(':').next().unwrap_or("")).cloned().unwrap_or_default());
            }
        }
        cards.push(json!({"id": key, "fills": card["fills"].as_array().unwrap().len(), "completedTrades": p["completedTrades"],
            "openShares": open, "cash": p["cash"], "realizedPnl": p["realizedPnl"],
            "liquidationPnl": p["liquidationPnl"], "valuation": p["valuation"], "pendingIntents": p["pendingIntents"]}));
    }
    let mut groups: BTreeMap<String, V> = BTreeMap::new();
    for card in &cards {
        let mut c = card.clone();
        c.as_object_mut().unwrap().remove("id");
        let key = serde_json::to_string(&c).unwrap();
        let g = groups.entry(key).or_insert_with(|| { let mut g = c.clone(); g["wallets"] = json!(0); g });
        g["wallets"] = json!(g["wallets"].as_i64().unwrap() + 1);
    }
    let mut held_v: Vec<String> = held.into_iter().collect();
    held_v.sort();
    Ok(json!({"schemaVersion": 1, "cohort": binding["cohort"], "declarationHash": binding["declarationHash"],
        "binaryHash": binding["binaryHash"], "engineHash": binding["engineHash"], "sourceHash": cap["source_hash"],
        "sourceBytes": cap["source_bytes"], "sourceRecords": cap["source_records"], "through": e["evaluationThrough"],
        "start": e["evaluationStart"], "candidates": cards.len(), "heldMarketIds": held_v,
        "uniqueFillEvents": events.len(), "cardsWithFills": cards.iter().filter(|c| c["fills"].as_i64().unwrap() > 0).count(),
        "groups": groups.into_values().collect::<Vec<_>>(), "fees": 0, "walletResultsAreCorrelated": true,
        "executionAuthority": false, "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false}))
}
