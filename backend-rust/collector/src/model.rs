// live-paper/model.mjs — mean-reversion/dip paper engine (exact BigInt micros).
use baal_paper::fixed::{big_amount, big_fee, big_price, money, SCALE, TICK};
use baal_service_runtime::contract::{instant_ns, parse_iso_ms, timestamp_str};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

/// digest = sha256(JSON.stringify(v)) — preserves object key order.
pub fn digest(v: &V) -> String { baal_service_runtime::json_text::digest(v) }
/// strict canonical instant: YYYY-MM-DDThh:mm:ss.dddZ exactly.
pub fn instant(s: &str) -> Result<i64, String> {
    if s.len() != 24 || !s.ends_with('Z') || s.as_bytes()[19] != b'.'
        || s[20..23].bytes().any(|b| !b.is_ascii_digit()) {
        return Err("Timestamp must be canonical UTC ISO milliseconds".into());
    }
    let ms = parse_iso_ms(s).filter(|&m| ms_to_iso_check(m, s)).ok_or("Invalid timestamp")?;
    Ok(ms as i64)
}
fn ms_to_iso_check(ms: f64, s: &str) -> bool { baal_service_runtime::contract::ms_to_iso(ms) == s }
fn costs(c: &V) -> V { json!({"feeBps": c["feeBps"].as_i64().unwrap_or(0), "feePerShare": "0.000000"}) }
fn fee_of(notional: &BigInt, quantity: i64, c: &V) -> Result<BigInt, String> {
    big_fee(notional, &BigInt::from(quantity), &costs(c))
}
fn amount_of(v: &V) -> Result<BigInt, String> { big_amount(v.as_str().unwrap_or("")) }
pub fn validate_config(c: &V) -> Result<(), String> {
    if c["schemaVersion"].as_i64() != Some(1) || amount_of(&c["initialCash"]).map(|x| x <= BigInt::from(0)).unwrap_or(true) {
        return Err("Invalid paper configuration".into());
    }
    for k in ["readIntervalMs","cycleIntervalMs","maxDataAgeMs","latencyMs","feeBps","slippageTicks","depthFractionBps","orderShares","maxPositionShares","warmupObservations","lookback","exitAfterMs","port"] {
        if c[k].as_i64().filter(|&x| x >= 0 && x as f64 == c[k].as_f64().unwrap_or(-1.0)).is_none() { return Err(format!("Invalid {}", k)); }
    }
    let gi = |k: &str| c[k].as_i64().unwrap_or(-1);
    if gi("readIntervalMs") < 5000 || gi("cycleIntervalMs") < 10000 || gi("maxDataAgeMs") < 1000
        || gi("orderShares") < 1 || gi("maxPositionShares") < gi("orderShares") || gi("warmupObservations") < 2
        || gi("lookback") < gi("warmupObservations") || !(1..=1000).contains(&gi("depthFractionBps"))
        || gi("feeBps") > 10000 || gi("slippageTicks") > 5 || gi("latencyMs") < 1
        || !(1024..=65535).contains(&gi("port")) || gi("exitAfterMs") < 1000 {
        return Err("Unsafe paper limits".into());
    }
    let su = c.get("strategyUniverse").and_then(V::as_str);
    let diversified = matches!(su, Some("diversified-dip-v1") | Some("diversified-dip-scale-v1"));
    let scaled = su == Some("diversified-dip-scale-v1");
    if su.is_some() && !diversified { return Err("Unknown strategy universe".into()); }
    if diversified {
        let max_pos_ok = gi("maxPositionShares") <= if scaled { 2000 } else { 200 };
        let exp_ok = amount_of(&c["maxExposure"])? <= amount_of(&c["initialCash"])? / BigInt::from(if scaled { 2 } else { 10 });
        let res_ok = amount_of(&c["cashReserve"])? >= amount_of(&c["initialCash"])? * BigInt::from(if scaled { 5 } else { 9 }) / BigInt::from(10);
        if gi("feeBps") != 0 || !max_pos_ok || !exp_ok || !res_ok { return Err("Unsafe diversified paper scope".into()); }
    }
    let ids = c["marketIds"].as_array().cloned().unwrap_or_default();
    if ids.is_empty() || ids.len() > if diversified { 300 } else { 8 }
        || ids.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect::<BTreeSet<_>>().len() != ids.len()
        || ids.iter().any(|x| !x.as_str().is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))) {
        return Err("Invalid market scope".into());
    }
    if c["tournamentId"].as_str() != Some("bda92870-621e-47b0-bc3c-3602c5c26f55") || c["tournamentSlug"].as_str() != Some("midterm-elections") {
        return Err("Invalid tournament scope".into());
    }
    let entry_z = c["entryZ"].as_f64().unwrap_or(-1.0);
    if !(entry_z.is_finite() && entry_z >= 0.0) || amount_of(&c["minEdge"]).map(|x| x <= BigInt::from(0)).unwrap_or(true)
        || amount_of(&c["stopLoss"]).map(|x| x <= BigInt::from(0)).unwrap_or(true)
        || amount_of(&c["maxExposure"]).map(|x| x <= BigInt::from(0)).unwrap_or(true)
        || amount_of(&c["maxExposure"])? > amount_of(&c["initialCash"])?
        || amount_of(&c["cashReserve"]).map(|x| x < BigInt::from(0)).unwrap_or(true)
        || amount_of(&c["cashReserve"])? >= amount_of(&c["initialCash"])? {
        return Err("Invalid strategy risk parameters".into());
    }
    Ok(())
}
pub fn normalize_book(response: &V, market_id: &str, tournament_id: &str, received_at: &str,
    request_started_at: &str, max_age: i64, excluded: &BTreeSet<String>, freshness_policy: &str) -> Result<V, String> {
    if !["event_age_v1", "authoritative_receipt_v1"].contains(&freshness_policy) { return Err("Unknown book freshness policy".into()); }
    let now = parse_iso_ms(received_at).filter(|x| x.is_finite()).ok_or("Slow or invalid snapshot observation")?;
    let started = parse_iso_ms(request_started_at).filter(|x| x.is_finite()).ok_or("Slow or invalid snapshot observation")?;
    if now < started || now - started > max_age as f64 { return Err("Slow or invalid snapshot observation".into()); }
    let contexts: Vec<&V> = response["contexts"].as_array().map(|a| a.iter().filter(|x| x["tournament"]["id"].as_str() == Some(tournament_id)).collect()).unwrap_or_default();
    if contexts.is_empty() || contexts.iter().any(|x| x["type"].as_str() != Some("tournament")) { return Err("Missing exact tournament book context".into()); }
    if contexts.iter().any(|x| digest(&x["orderbook"]) != digest(&contexts[0]["orderbook"])) { return Err("Conflicting tournament contexts".into()); }
    let context = contexts[0];
    let exchanges = context["orderbook"]["exchanges"].as_array().cloned().unwrap_or_default();
    if exchanges.len() != 1 || exchanges[0]["option"].as_str() != Some("YES") { return Err("Only verified single YES exchange books supported".into()); }
    let e = &exchanges[0];
    let id = e["exchangeId"].as_i64().map(|x| x.to_string()).or_else(|| e["exchangeId"].as_str().map(String::from)).unwrap_or_default();
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) { return Err("Missing/stale engine version".into()); }
    let version = &e["asOf"];
    let seq = version["sequence"].as_i64();
    let at_ms = version["at"].as_str().and_then(parse_iso_ms);
    if seq.map(|s| s < 0).unwrap_or(true) || at_ms.is_none() || at_ms.unwrap() > now + 2000.0
        || (freshness_policy == "event_age_v1" && now - at_ms.unwrap() > max_age as f64) {
        return Err("Missing/stale engine version".into());
    }
    if excluded.contains(&id) { return Err("Exchange has real account holdings or open orders".into()); }
    let levels = |rows: &V, descending: bool| -> Result<Vec<V>, String> {
        let rows = rows.as_array().ok_or("Missing depth")?;
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for l in rows {
            let p = big_price(&l["price"].as_f64().map(|x| x.to_string()).or_else(|| l["price"].as_str().map(String::from)).unwrap_or_default())?;
            if l["quantity"].as_i64().filter(|q| *q > 0).is_none() || !seen.insert(p.to_string()) { return Err("Invalid/duplicate depth".into()); }
            out.push(json!({"price": money(&p), "quantity": l["quantity"]}));
        }
        out.sort_by(|a, b| {
            let (pa, pb) = (big_amount(a["price"].as_str().unwrap()).unwrap(), big_amount(b["price"].as_str().unwrap()).unwrap());
            if descending { pb.cmp(&pa) } else { pa.cmp(&pb) }
        });
        Ok(out)
    };
    let bids = levels(&e["bids"], true)?;
    let asks = levels(&e["asks"], false)?;
    if bids.is_empty() || asks.is_empty() || amount_of(&bids[0]["price"])? >= amount_of(&asks[0]["price"])? {
        return Err("Missing, locked or crossed market".into());
    }
    let source_at = timestamp_str(version["at"].as_str().unwrap_or("")).map_err(|e| e.0)?;
    let mut out = json!({"marketId": market_id, "exchangeId": id, "instrument": format!("{}:YES", id),
        "receivedAt": received_at, "requestStartedAt": request_started_at, "sourceAt": source_at,
        "sequence": seq.unwrap().to_string(), "versionKey": format!("{}:{}", seq.unwrap(), source_at),
        "bids": bids, "asks": asks});
    if freshness_policy == "authoritative_receipt_v1" {
        out["freshnessPolicy"] = json!(freshness_policy);
        out["sourceEventAgeMs"] = json!((now - at_ms.unwrap()) as i64);
    }
    Ok(out)
}
pub fn invalidate(s: &mut V, market_id: Option<&str>, at: &str, reason: &str) {
    if s.get("invalidated").is_none() { s["invalidated"] = json!({}); }
    if let Some(books) = s["books"].as_object() {
        let keys: Vec<String> = books.iter().filter(|(_, b)| market_id.is_none() || b["marketId"].as_str() == market_id).map(|(k, _)| k.clone()).collect();
        for id in keys { s["invalidated"][id] = json!({"at": at, "reason": reason}); }
    }
}
pub fn round_trip_stats(fills: &[V]) -> V {
    #[derive(Default)]
    struct Pos { quantity: i64, lots: Vec<(i64, BigInt)>, pnl: BigInt }
    let mut positions: std::collections::HashMap<String, Pos> = Default::default();
    let mut closed = 0i64; let mut winners = 0i64;
    for f in fills {
        let p = positions.entry(f["instrument"].as_str().unwrap_or("").into()).or_default();
        let n = amount_of(&f["price"]).unwrap_or(BigInt::from(0)) * BigInt::from(f["quantity"].as_i64().unwrap_or(0));
        let charge = amount_of(&f["fee"]).unwrap_or(BigInt::from(0));
        let qty = f["quantity"].as_i64().unwrap_or(0);
        if f["action"].as_str() == Some("buy") {
            p.quantity += qty;
            p.lots.push((qty, &n + &charge));
        } else {
            let mut left = qty; let mut basis = BigInt::from(0);
            while left > 0 {
                let Some(l) = p.lots.first_mut() else { return json!({"error": "Round-trip ledger oversell"}) };
                let q = left.min(l.0);
                let used = if q == l.0 { l.1.clone() } else { &l.1 * BigInt::from(q) / BigInt::from(l.0) };
                basis += used.clone(); l.1 -= used; l.0 -= q; left -= q;
                if l.0 == 0 { p.lots.remove(0); }
            }
            p.quantity -= qty; p.pnl += &n - &charge - &basis;
            if p.quantity == 0 { closed += 1; if p.pnl > BigInt::from(0) { winners += 1; } p.pnl = BigInt::from(0); }
        }
    }
    json!({"closedTrades": closed, "winningTrades": winners,
        "winRate": if closed > 0 { json!(winners as f64 / closed as f64) } else { V::Null }})
}
pub fn initial_state(c: &V, run_id: &str, at: &str) -> Result<V, String> {
    validate_config(c)?;
    let st = json!({"cash": c["initialCash"], "realized": "0.000000", "fees": "0.000000",
        "turnover": "0.000000", "positions": {}, "pending": {}, "fills": [], "intents": 0,
        "expiredIntents": 0, "consumedVersions": {}, "equityCurve": [], "peak": c["initialCash"],
        "maxDrawdown": "0.000000", "closedTrades": 0, "winningTrades": 0});
    Ok(json!({"schemaVersion": 1, "engineRevision": 3, "runId": run_id, "startedAt": at,
        "configHash": digest(c), "lastAt": at, "observations": 0, "skips": {}, "books": {},
        "histories": {}, "strategies": {"mean_reversion": st, "buy_hold_benchmark": st}}))
}
pub fn restore_state(input: &V, c: &V) -> Result<V, String> {
    let mut s = input.clone();
    if s.get("engineRevision").is_none() {
        for (_, st) in s["strategies"].as_object_mut().ok_or("state")?.iter_mut() {
            if st["fills"].as_array().is_some_and(|f| f.iter().any(|x| x["action"].as_str() == Some("sell"))) {
                return Err("Legacy sold inventory needs explicit migration".into());
            }
            let fills = st["fills"].as_array().cloned().unwrap_or_default();
            for (id, p) in st["positions"].as_object_mut().ok_or("positions")?.iter_mut() {
                let lots: Vec<V> = fills.iter()
                    .filter(|f| f["instrument"].as_str() == Some(id.as_str()) && f["action"].as_str() == Some("buy"))
                    .map(|f| json!({"quantity": f["quantity"],
                        "cost": money(&(amount_of(&f["price"]).unwrap_or(BigInt::from(0)) * BigInt::from(f["quantity"].as_i64().unwrap_or(0)) + amount_of(&f["fee"]).unwrap_or(BigInt::from(0))))}))
                    .collect();
                p["lots"] = json!(lots);
            }
        }
        s["engineRevision"] = json!(2);
    }
    if s["engineRevision"].as_i64() == Some(2) {
        for (_, st) in s["strategies"].as_object_mut().unwrap().iter_mut() {
            if let Some(cv) = st["consumedVersions"].as_object_mut() {
                for (_, key) in cv.iter_mut() {
                    let k = key.as_str().unwrap_or("").to_string();
                    if let Some(sep) = k.find(':') {
                        *key = json!(format!("{}:{}", &k[..sep], timestamp_str(&k[sep + 1..]).unwrap_or_default()));
                    }
                }
            }
        }
        s["engineRevision"] = json!(3);
        s["evidenceValidation"] = json!("legacy_run_requires_corrected_replay_and_independent_review");
    }
    if s["engineRevision"].as_i64() != Some(3) { return Err("Unknown engine revision".into()); }
    assert_state(&s, c)?;
    Ok(s)
}
pub fn assert_state(s: &V, c: &V) -> Result<(), String> {
    if s["schemaVersion"].as_i64() != Some(1) || s["configHash"].as_str() != Some(digest(c).as_str()) {
        return Err("Resume requires identical config/schema".into());
    }
    for (_, st) in s["strategies"].as_object().ok_or("state")?.iter() {
        let mut signed = BigInt::from(0);
        let mut fees = BigInt::from(0);
        let mut turnover = BigInt::from(0);
        let mut realized = BigInt::from(0);
        let mut quantities: std::collections::HashMap<String, i64> = Default::default();
        let mut lots: std::collections::HashMap<String, Vec<(i64, BigInt)>> = Default::default();
        for f in st["fills"].as_array().cloned().unwrap_or_default() {
            let action = f["action"].as_str().unwrap_or("");
            let qty = f["quantity"].as_i64().filter(|q| *q >= 1);
            if !["buy", "sell"].contains(&action) || qty.is_none() { return Err("Invalid durable fill".into()); }
            let qty = qty.unwrap();
            let notional = amount_of(&f["price"])? * BigInt::from(qty);
            let charge = amount_of(&f["fee"])?;
            signed += if action == "buy" { -&notional - &charge } else { &notional - &charge };
            fees += &charge; turnover += &notional;
            *quantities.entry(f["instrument"].as_str().unwrap_or("").into()).or_insert(0) += if action == "buy" { qty } else { -qty };
            let inst = f["instrument"].as_str().unwrap_or("").to_string();
            let entry = lots.entry(inst).or_default();
            if action == "buy" { entry.push((qty, &notional + &charge)); }
            else {
                let mut left = qty; let mut basis = BigInt::from(0);
                while left > 0 {
                    let Some(l) = entry.first_mut() else { return Err("Fill ledger oversell".into()); };
                    let q = left.min(l.0);
                    let used = if q == l.0 { l.1.clone() } else { &l.1 * BigInt::from(q) / BigInt::from(l.0) };
                    basis += used.clone(); l.1 -= used; l.0 -= q; left -= q;
                    if l.0 == 0 { entry.remove(0); }
                }
                realized += &notional - &charge - &basis;
            }
        }
        if amount_of(&st["cash"])? != amount_of(&c["initialCash"])? + &signed
            || amount_of(&st["fees"])? != fees || amount_of(&st["turnover"])? != turnover
            || amount_of(&st["cash"])? < BigInt::from(0) {
            return Err("Paper cash/fee conservation failure".into());
        }
        for (id, q) in &quantities {
            if *q < 0 || *q != st["positions"][id]["quantity"].as_i64().unwrap_or(0) { return Err("Inventory conservation failure".into()); }
        }
        if realized != amount_of(&st["realized"])? { return Err("Realized basis conservation failure".into()); }
        for (id, p) in st["positions"].as_object().cloned().unwrap_or_default() {
            let pos_lots: Vec<(i64, BigInt)> = p["lots"].as_array().cloned().unwrap_or_default().iter()
                .map(|l| (l["quantity"].as_i64().unwrap_or(0), amount_of(&l["cost"]).unwrap_or(BigInt::from(0)))).collect();
            let expected: Vec<V> = lots.get(&id).cloned().unwrap_or_default().iter().map(|(q, c)| json!({"quantity": q, "cost": money(c)})).collect();
            if p["quantity"].as_i64().filter(|q| *q >= 1).is_none()
                || p["quantity"].as_i64().unwrap_or(0) > c["maxPositionShares"].as_i64().unwrap_or(0)
                || p["quantity"].as_i64().unwrap_or(0) != *quantities.get(&id).unwrap_or(&0)
                || amount_of(&p["cost"])? != lots.get(&id).cloned().unwrap_or_default().iter().map(|l| l.1.clone()).sum::<BigInt>()
                || serde_json::to_string(&p["lots"]).unwrap_or_default() != serde_json::to_string(&expected).unwrap_or_default() {
                return Err("Position invariant failure".into());
            }
            let _ = pos_lots;
        }
    }
    Ok(())
}
fn fill_intent(st: &mut V, intent: &V, b: &V, c: &V, at: &str) {
    if instant(b["receivedAt"].as_str().unwrap_or("")).unwrap_or(0) - instant(intent["createdAt"].as_str().unwrap_or("")).unwrap_or(0) < c["latencyMs"].as_i64().unwrap_or(0)
        || b["receivedAt"].as_str() == intent["createdAt"].as_str() { return; }
    if st["consumedVersions"][b["instrument"].as_str().unwrap_or("")] == b["versionKey"] { return; }
    let rows = if intent["action"].as_str() == Some("buy") { b["asks"].as_array().cloned().unwrap_or_default() } else { b["bids"].as_array().cloned().unwrap_or_default() };
    let mut remaining = intent["quantity"].as_i64().unwrap_or(0);
    let mut cash = amount_of(&st["cash"]).unwrap_or(BigInt::from(0));
    let mut basis = BigInt::from(0);
    let mut net = BigInt::from(0);
    let mut total_qty = 0i64;
    let mut exposure: BigInt = st["positions"].as_object().cloned().unwrap_or_default().values().map(|p| amount_of(&p["cost"]).unwrap_or(BigInt::from(0))).sum();
    let instrument = b["instrument"].as_str().unwrap_or("").to_string();
    for l in &rows {
        if remaining == 0 { break; }
        let dir: i64 = if intent["action"].as_str() == Some("buy") { 1 } else { -1 };
        let p = amount_of(&l["price"]).unwrap_or(BigInt::from(0)) + BigInt::from(dir) * BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0));
        let limit = amount_of(&intent["limit"]).unwrap_or(BigInt::from(0));
        if p < BigInt::from(TICK) || p > BigInt::from(SCALE - TICK)
            || (intent["action"].as_str() == Some("buy") && p > limit) || (intent["action"].as_str() == Some("sell") && p < limit) { break; }
        let mut q = remaining.min((l["quantity"].as_i64().unwrap_or(0) * c["depthFractionBps"].as_i64().unwrap_or(0)) / 10000);
        if intent["action"].as_str() == Some("buy") {
            q = q.min(c["maxPositionShares"].as_i64().unwrap_or(0) - st["positions"][&instrument]["quantity"].as_i64().unwrap_or(0));
            while q > 0 {
                let cost = &p * BigInt::from(q) + fee_of(&(&p * BigInt::from(q)), q, c).unwrap_or(BigInt::from(0));
                if &cash - &cost >= amount_of(&c["cashReserve"]).unwrap_or(BigInt::from(0))
                    && &exposure + &cost <= amount_of(&c["maxExposure"]).unwrap_or(BigInt::from(0)) { break; }
                q -= 1;
            }
        } else {
            q = q.min(st["positions"][&instrument]["quantity"].as_i64().unwrap_or(0));
        }
        if q < 1 { continue; }
        let notional = &p * BigInt::from(q);
        let charge = fee_of(&notional, q, c).unwrap_or(BigInt::from(0));
        if intent["action"].as_str() == Some("buy") {
            cash -= &notional + &charge; exposure += &notional + &charge;
            let old = st["positions"][&instrument].clone();
            let mut lots: Vec<V> = old["lots"].as_array().cloned().unwrap_or_default();
            lots.push(json!({"quantity": q, "cost": money(&(&notional + &charge))}));
            st["positions"][&instrument] = json!({"quantity": old["quantity"].as_i64().unwrap_or(0) + q,
                "cost": money(&(amount_of(&old["cost"]).unwrap_or(BigInt::from(0)) + &notional + &charge)),
                "enteredAt": if old["enteredAt"].is_null() { json!(at) } else { old["enteredAt"].clone() },
                "lots": lots});
        } else {
            let pos = &mut st["positions"][&instrument];
            let mut left = q; let mut used = BigInt::from(0);
            let mut lots: Vec<V> = pos["lots"].as_array().cloned().unwrap_or_default();
            while left > 0 && !lots.is_empty() {
                let l = lots[0].clone();
                let lot_qty = l["quantity"].as_i64().unwrap_or(0);
                let lot_cost = amount_of(&l["cost"]).unwrap_or(BigInt::from(0));
                let n = left.min(lot_qty);
                let part = if n == lot_qty { lot_cost.clone() } else { &lot_cost * BigInt::from(n) / BigInt::from(lot_qty) };
                used += &part;
                let new_cost = &lot_cost - &part;
                let new_qty = lot_qty - n;
                if new_qty == 0 { lots.remove(0); } else { lots[0] = json!({"quantity": new_qty, "cost": money(&new_cost)}); }
                left -= n;
            }
            basis += &used; net += &notional - &charge;
            let new_cost = amount_of(&pos["cost"]).unwrap_or(BigInt::from(0)) - &used;
            let new_qty = pos["quantity"].as_i64().unwrap_or(0) - q;
            st["positions"][&instrument]["cost"] = json!(money(&new_cost));
            st["positions"][&instrument]["quantity"] = json!(new_qty);
            st["positions"][&instrument]["lots"] = json!(lots);
            if new_qty == 0 { st["positions"].as_object_mut().unwrap().remove(&instrument); }
            cash += &notional - &charge;
            let realized = amount_of(&st["realized"]).unwrap_or(BigInt::from(0)) + &notional - &charge - &used;
            st["realized"] = json!(money(&realized));
        }
        st["fees"] = json!(money(&(amount_of(&st["fees"]).unwrap_or(BigInt::from(0)) + &charge)));
        st["turnover"] = json!(money(&(amount_of(&st["turnover"]).unwrap_or(BigInt::from(0)) + &notional)));
        st["cash"] = json!(money(&cash));
        let fill = json!({"intentId": intent["id"], "instrument": instrument, "action": intent["action"],
            "quantity": q, "price": money(&p), "fee": money(&charge), "at": at, "sourceAt": b["sourceAt"],
            "sourceVersion": b["versionKey"], "model": "hypothetical_next_snapshot_taker",
            "depthFractionBps": c["depthFractionBps"], "slippageTicks": c["slippageTicks"]});
        st["fills"].as_array_mut().unwrap().push(fill);
        total_qty += q; remaining -= q;
    }
    if total_qty > 0 {
        st["consumedVersions"][&instrument] = b["versionKey"].clone();
        st["pending"].as_object_mut().unwrap().remove(&instrument);
        if intent["action"].as_str() == Some("sell") {
            st["closedTrades"] = json!(st["closedTrades"].as_i64().unwrap_or(0) + 1);
            if net > basis { st["winningTrades"] = json!(st["winningTrades"].as_i64().unwrap_or(0) + 1); }
        }
    }
}
fn enqueue(st: &mut V, b: &V, action: &str, quantity: i64, limit: BigInt, at: &str, reason: &str) {
    st["intents"] = json!(st["intents"].as_i64().unwrap_or(0) + 1);
    st["pending"][b["instrument"].as_str().unwrap_or("")] = json!({"id": st["intents"].as_i64().unwrap_or(0),
        "createdAt": at, "action": action, "quantity": quantity, "limit": money(&limit), "reason": reason});
}
pub fn observe(input: &V, b: &V, c: &V) -> Result<V, String> {
    let mut s = input.clone();
    let at = b["receivedAt"].as_str().unwrap_or("");
    let id = b["instrument"].as_str().unwrap_or("").to_string();
    if instant(at)? < instant(s["lastAt"].as_str().unwrap_or(""))? { return Err("Observation time moved backward".into()); }
    let prev = s["books"][&id].clone();
    let same_version = !prev.is_null()
        && b["sequence"].as_str().unwrap_or("").parse::<i128>().unwrap_or(-1) == prev["sequence"].as_str().unwrap_or("").parse::<i128>().unwrap_or(-2)
        && instant_ns(b["sourceAt"].as_str().unwrap_or("")).unwrap_or(-1) == instant_ns(prev["sourceAt"].as_str().unwrap_or("")).unwrap_or(-2);
    if !prev.is_null()
        && (b["sequence"].as_str().unwrap_or("").parse::<i128>().unwrap_or(-1) < prev["sequence"].as_str().unwrap_or("").parse::<i128>().unwrap_or(-2)
            || instant_ns(b["sourceAt"].as_str().unwrap_or("")).unwrap_or(-1) < instant_ns(prev["sourceAt"].as_str().unwrap_or("")).unwrap_or(-2)) {
        return Err("Book version rollback".into());
    }
    if same_version && digest(&json!({"bids": b["bids"], "asks": b["asks"]})) != digest(&json!({"bids": prev["bids"], "asks": prev["asks"]})) {
        return Err("Conflicting equal book version".into());
    }
    let mut b = b.clone();
    b["sourceAt"] = json!(timestamp_str(b["sourceAt"].as_str().unwrap_or("")).map_err(|e| e.0)?);
    b["versionKey"] = json!(format!("{}:{}", b["sequence"].as_str().unwrap_or(""), b["sourceAt"].as_str().unwrap_or("")));
    if s["invalidated"][&id].is_object() && instant(b["requestStartedAt"].as_str().unwrap_or(""))? <= instant(s["invalidated"][&id]["at"].as_str().unwrap_or(""))? {
        return Err("Pre-invalidation snapshot cannot recover".into());
    }
    if let Some(inv) = s["invalidated"].as_object_mut() { inv.remove(&id); }
    s["observations"] = json!(s["observations"].as_i64().unwrap_or(0) + 1);
    s["lastAt"] = json!(at);
    s["books"][&id] = b.clone();
    if same_version { assert_state(&s, c)?; return Ok(s); }
    let mid = (amount_of(&b["bids"][0]["price"])?.to_string().parse::<f64>().unwrap_or(0.0)
        + amount_of(&b["asks"][0]["price"])?.to_string().parse::<f64>().unwrap_or(0.0)) / 2.0;
    let hist: Vec<f64> = s["histories"][&id].as_array().cloned().unwrap_or_default().iter().filter_map(|x| x.as_f64()).collect();
    let recent: Vec<f64> = hist.iter().rev().take(c["lookback"].as_i64().unwrap_or(0) as usize).rev().cloned().collect();
    let mean = if recent.is_empty() { mid } else { recent.iter().sum::<f64>() / recent.len() as f64 };
    let sd = if recent.is_empty() { 0.0 } else { (recent.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / recent.len() as f64).sqrt() };
    if s.get("signals").is_none() { s["signals"] = json!({}); }
    s["signals"][&id] = json!({"marketId": b["marketId"], "at": at, "priorObservations": recent.len(),
        "warmupRemaining": (c["warmupObservations"].as_i64().unwrap_or(0) - recent.len() as i64).max(0),
        "midpoint": money(&BigInt::from(mid.round() as i64)), "laggedMean": money(&BigInt::from(mean.round() as i64)),
        "standardDeviation": money(&BigInt::from(sd.round() as i64)),
        "zScore": if sd != 0.0 { json!((mean - mid) / sd) } else { V::Null },
        "estimatedEntryEdge": money(&(BigInt::from(mean.round() as i64) - amount_of(&b["asks"][0]["price"])?
            - BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0))
            - fee_of(&(amount_of(&b["asks"][0]["price"])? + BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0))), 1, c)?))});
    let names: Vec<String> = s["strategies"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    for name in names {
        let mut st = s["strategies"][&name].clone();
        if let Some(pending) = st["pending"].as_object().and_then(|p| p.get(&id)).cloned() {
            if instant(at)? - instant(pending["createdAt"].as_str().unwrap_or(""))? > c["maxDataAgeMs"].as_i64().unwrap_or(0) + c["cycleIntervalMs"].as_i64().unwrap_or(0) * 4 {
                st["pending"].as_object_mut().unwrap().remove(&id);
                st["expiredIntents"] = json!(st["expiredIntents"].as_i64().unwrap_or(0) + 1);
            } else { fill_intent(&mut st, &pending, &b, c, at); }
        }
        s["strategies"][&name] = st.clone();
        let st = &mut s["strategies"][&name];
        if st["pending"].as_object().is_some_and(|p| p.contains_key(&id)) { continue; }
        let pos = st["positions"][&id].clone();
        if name == "buy_hold_benchmark" {
            if pos.is_null() && !st["fills"].as_array().is_some_and(|f| f.iter().any(|x| x["instrument"].as_str() == Some(id.as_str()))) {
                enqueue(st, &b, "buy", c["orderShares"].as_i64().unwrap_or(0),
                    amount_of(&b["asks"][0]["price"])? + BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0)), at, "small_buy_hold_control");
            }
            continue;
        }
        if recent.len() < c["warmupObservations"].as_i64().unwrap_or(0) as usize { continue; }
        if !pos.is_null() {
            let avg = amount_of(&pos["cost"])? / BigInt::from(pos["quantity"].as_i64().unwrap_or(1));
            let bid = amount_of(&b["bids"][0]["price"])?;
            if bid.to_string().parse::<i64>().unwrap_or(0) >= mean as i64
                || &avg - &bid >= amount_of(&c["stopLoss"])?
                || instant(at)? - instant(pos["enteredAt"].as_str().unwrap_or(""))? >= c["exitAfterMs"].as_i64().unwrap_or(0) {
                enqueue(st, &b, "sell", pos["quantity"].as_i64().unwrap_or(0), &bid - BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0)), at, "mean_reversion_exit_stop_or_time");
            }
        } else {
            let ask = amount_of(&b["asks"][0]["price"])? + BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0));
            let edge = mean - ask.to_string().parse::<f64>().unwrap_or(0.0)
                - fee_of(&ask, 1, c)?.to_string().parse::<f64>().unwrap_or(0.0);
            if sd > 0.0 && (mean - mid) / sd >= c["entryZ"].as_f64().unwrap_or(0.0) && edge >= amount_of(&c["minEdge"])?.to_string().parse::<f64>().unwrap_or(0.0) {
                enqueue(st, &b, "buy", c["orderShares"].as_i64().unwrap_or(0), ask, at, "lagged_rolling_mean_zscore");
            }
        }
    }
    let mut hist = hist;
    hist.push(mid);
    let keep = c["lookback"].as_i64().unwrap_or(0) as usize;
    if hist.len() > keep { hist = hist[hist.len() - keep..].to_vec(); }
    s["histories"][&id] = json!(hist);
    assert_state(&s, c)?;
    Ok(s)
}
pub fn report(s: &V, c: &V, now: &str) -> V {
    let now_ms = parse_iso_ms(now).unwrap_or(f64::NAN);
    let age = |id: &str| -> f64 {
        now_ms - parse_iso_ms(s["books"][id]["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
    };
    let mut strategies = serde_json::Map::new();
    for (name, st) in s["strategies"].as_object().cloned().unwrap_or_default() {
        let mut reference = amount_of(&st["cash"]).unwrap_or(BigInt::from(0));
        let mut liquidation = reference.clone();
        let mut reference_complete = true;
        let mut liquidation_complete = true;
        let mut positions = Vec::new();
        for (id, pos) in st["positions"].as_object().cloned().unwrap_or_default() {
            let b = &s["books"][&id];
            let mut proceeds = BigInt::from(0);
            let mut left = pos["quantity"].as_i64().unwrap_or(0);
            let a = age(&id);
            let fresh = !b.is_null() && !s["invalidated"][&id].is_object() && a >= 0.0 && a <= (c["maxDataAgeMs"].as_i64().unwrap_or(0) + c["cycleIntervalMs"].as_i64().unwrap_or(0) * 4) as f64;
            if fresh {
                reference += (amount_of(&b["bids"][0]["price"]).unwrap_or(BigInt::from(0)) + amount_of(&b["asks"][0]["price"]).unwrap_or(BigInt::from(0)))
                    * BigInt::from(pos["quantity"].as_i64().unwrap_or(0)) / BigInt::from(2);
                for l in b["bids"].as_array().cloned().unwrap_or_default() {
                    let q = left.min(l["quantity"].as_i64().unwrap_or(0) * c["depthFractionBps"].as_i64().unwrap_or(0) / 10000);
                    if q > 0 {
                        let p = amount_of(&l["price"]).unwrap_or(BigInt::from(0)) - BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0));
                        if p < BigInt::from(TICK) { break; }
                        let n = &p * BigInt::from(q);
                        proceeds += &n - fee_of(&n, q, c).unwrap_or(BigInt::from(0));
                        left -= q;
                    }
                    if left == 0 { break; }
                }
            }
            if !fresh { reference_complete = false; }
            if !fresh || left > 0 { liquidation_complete = false; }
            liquidation += &proceeds;
            let labels = [("377","Alaska Senate"),("354","Florida Senate"),("277","North Carolina Senate"),("305","Michigan Governor")];
            let label = labels.iter().find(|(k, _)| *k == b["marketId"].as_str().unwrap_or("")).map(|(_, v)| v.to_string())
                .unwrap_or_else(|| format!("Market {}", b["marketId"].as_str().unwrap_or("")));
            let mut pos = pos.clone();
            pos["instrument"] = json!(id);
            pos["label"] = json!(format!("{} Democratic YES", label));
            pos["observationAgeMs"] = if fresh { json!(a as i64) } else { V::Null };
            pos["quoteReceivedAt"] = b["receivedAt"].clone();
            pos["quoteAgeMs"] = if a.is_finite() && a >= 0.0 { json!(a as i64) } else { V::Null };
            pos["quoteInvalidated"] = json!(s["invalidated"][&id].is_object());
            pos["estimatedProceeds"] = json!(money(&proceeds));
            pos["unsoldShares"] = json!(left);
            positions.push(pos);
        }
        let equity = if reference_complete { Some(reference.clone()) } else { None };
        let liq = if liquidation_complete { Some(liquidation.clone()) } else { None };
        let curve = st["equityCurve"].as_array().cloned().unwrap_or_default();
        let mut peak = amount_of(&st["peak"]).unwrap_or(BigInt::from(0));
        let mut dd = amount_of(&st["maxDrawdown"]).unwrap_or(BigInt::from(0));
        if let Some(eq) = &equity {
            if *eq > peak { peak = eq.clone(); }
            if &peak - eq > dd { dd = &peak - eq; }
        }
        let fills = st["fills"].as_array().cloned().unwrap_or_default();
        let mut obj = serde_json::Map::new();
        obj.insert("cash".into(), st["cash"].clone());
        obj.insert("realizedPnl".into(), st["realized"].clone());
        obj.insert("referenceEquity".into(), equity.as_ref().map(|e| json!(money(e))).unwrap_or(V::Null));
        obj.insert("referencePnl".into(), equity.map(|e| json!(money(&(e - amount_of(&c["initialCash"]).unwrap_or(BigInt::from(0)))))).unwrap_or(V::Null));
        obj.insert("estimatedLiquidationEquity".into(), liq.as_ref().map(|e| json!(money(e))).unwrap_or(V::Null));
        obj.insert("liquidationPnl".into(), liq.map(|e| json!(money(&(e - amount_of(&c["initialCash"]).unwrap_or(BigInt::from(0)))))).unwrap_or(V::Null));
        obj.insert("fees".into(), st["fees"].clone());
        obj.insert("turnover".into(), st["turnover"].clone());
        obj.insert("filledShares".into(), json!(fills.iter().map(|f| f["quantity"].as_i64().unwrap_or(0)).sum::<i64>()));
        obj.insert("fills".into(), json!(fills.len()));
        obj.insert("intents".into(), st["intents"].clone());
        obj.insert("pending".into(), json!(st["pending"].as_object().cloned().unwrap_or_default().values().cloned().collect::<Vec<_>>()));
        obj.insert("expiredIntents".into(), st["expiredIntents"].clone());
        let rt = round_trip_stats(&fills);
        for (k, v) in rt.as_object().unwrap() { obj.insert(k.clone(), v.clone()); }
        obj.insert("sellExecutions".into(), st["closedTrades"].clone());
        obj.insert("maxObservedDrawdown".into(), json!(money(&dd)));
        obj.insert("positions".into(), json!(positions));
        obj.insert("equityCurve".into(), json!(curve.iter().rev().take(500).rev().cloned().collect::<Vec<_>>()));
        obj.insert("recentFills".into(), json!(fills.iter().rev().take(25).rev().cloned().collect::<Vec<_>>()));
        strategies.insert(name.clone(), V::Object(obj));
    }
    json!({"schemaVersion": 1, "evidenceValidation": s.get("evidenceValidation").cloned().unwrap_or(json!("pending_independent_review")),
        "engineRevision": s["engineRevision"], "mode": "live_data_paper_only", "runId": s["runId"],
        "startedAt": s["startedAt"], "lastObservationAt": s["lastAt"], "reportedAt": now,
        "initialCash": c["initialCash"], "configHash": s["configHash"], "observations": s["observations"],
        "coveredExchanges": s["books"].as_object().map(|m| m.len()).unwrap_or(0),
        "scope": c["marketIds"], "signals": s.get("signals").cloned().unwrap_or(json!({})), "skips": s["skips"],
        "strategies": strategies, "noTradeBaseline": {"cash": c["initialCash"], "pnl": "0.000000"},
        "ordersSentToCompetition": 0,
        "assumptions": {"fillModel": "Hypothetical taker at subsequent REST snapshot; NOT a verified actual fill",
            "feeBps": c["feeBps"], "feeStatus": "unverified_scenario", "slippageTicks": c["slippageTicks"],
            "depthFractionBps": c["depthFractionBps"], "source": "official API v1, fixed tournament",
            "passiveFillEvidence": false, "completeTradeTape": false, "strategyProfitabilityEstablished": false,
            "mark": "midpoint of fresh bid/ask; not tournament valuation"}, "config": c})
}
pub fn sample_equity(s: &mut V, c: &V, now: &str) {
    let r = report(s, c, now);
    for (name, v) in r["strategies"].as_object().cloned().unwrap_or_default() {
        let st = &mut s["strategies"][&name];
        if !v["referenceEquity"].is_null() {
            let eq = big_amount(v["referenceEquity"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0));
            if eq > amount_of(&st["peak"]).unwrap_or(BigInt::from(0)) { st["peak"] = v["referenceEquity"].clone(); }
            let dd = amount_of(&st["peak"]).unwrap_or(BigInt::from(0)) - &eq;
            if dd > amount_of(&st["maxDrawdown"]).unwrap_or(BigInt::from(0)) { st["maxDrawdown"] = json!(money(&dd)); }
            let curve = st["equityCurve"].as_array().cloned().unwrap_or_default();
            let mut curve = curve;
            curve.push(json!({"at": now, "referenceEquity": v["referenceEquity"], "liquidationEquity": v["estimatedLiquidationEquity"]}));
            if curve.len() > 5000 { curve.remove(0); }
            st["equityCurve"] = json!(curve);
        }
    }
}
/// stepForecast
pub fn step_forecast(input: &V, b: &V, c: &V, exit_value: Option<f64>, net_edge: Option<f64>,
    horizon_ms: i64, minimum_edge: f64) -> Result<V, String> {
    if horizon_ms < 1000 || !minimum_edge.is_finite() || minimum_edge <= 0.0 { return Err("Invalid forecast policy".into()); }
    let previous = input["books"][b["instrument"].as_str().unwrap_or("")].clone();
    let same = !previous.is_null() && previous["sequence"] == b["sequence"]
        && instant_ns(previous["sourceAt"].as_str().unwrap_or("")).unwrap_or(-1) == instant_ns(b["sourceAt"].as_str().unwrap_or("")).unwrap_or(-2);
    let mut s = observe(input, b, c)?;
    let st = input["strategies"]["mean_reversion"].clone();
    let b = s["books"][b["instrument"].as_str().unwrap_or("")].clone();
    s["strategies"]["mean_reversion"] = st;
    if same { assert_state(&s, c)?; return Ok(s); }
    let at = b["receivedAt"].as_str().unwrap_or("").to_string();
    let id = b["instrument"].as_str().unwrap_or("").to_string();
    let mut st = s["strategies"]["mean_reversion"].clone();
    if let Some(pending) = st["pending"].as_object().and_then(|p| p.get(&id)).cloned() {
        if instant(&at)? - instant(pending["createdAt"].as_str().unwrap_or(""))? > c["maxDataAgeMs"].as_i64().unwrap_or(0) + c["cycleIntervalMs"].as_i64().unwrap_or(0) * 4 {
            st["pending"].as_object_mut().unwrap().remove(&id);
            st["expiredIntents"] = json!(st["expiredIntents"].as_i64().unwrap_or(0) + 1);
        } else { fill_intent(&mut st, &pending, &b, c, &at); }
    }
    if st["pending"].as_object().is_none_or(|p| !p.contains_key(&id)) {
        let pos = st["positions"][&id].clone();
        let bid = amount_of(&b["bids"][0]["price"])?;
        if !pos.is_null() {
            if instant(&at)? - instant(pos["enteredAt"].as_str().unwrap_or(""))? >= horizon_ms
                || amount_of(&pos["cost"])? / BigInt::from(pos["quantity"].as_i64().unwrap_or(1)) - &bid >= amount_of(&c["stopLoss"])? {
                enqueue(&mut st, &b, "sell", pos["quantity"].as_i64().unwrap_or(0),
                    &bid - BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0)), &at, "ml_horizon_or_stop");
            }
        } else if net_edge.is_some_and(|e| e.is_finite() && e >= minimum_edge)
            || exit_value.is_some_and(|e| e.is_finite() && (0.0..=1.0).contains(&e)) {
            let ask = amount_of(&b["asks"][0]["price"])? + BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0));
            let entry = (&ask + fee_of(&ask, 1, c)?).to_string().parse::<f64>().unwrap_or(0.0) / 1e6;
            if net_edge.is_some() || exit_value.unwrap_or(f64::NAN) - entry >= minimum_edge {
                enqueue(&mut st, &b, "buy", c["orderShares"].as_i64().unwrap_or(0), ask, &at,
                    if net_edge.is_some() { "ml_net_roundtrip_forecast" } else { "ml_net_exit_forecast" });
            }
        }
    }
    s["strategies"]["mean_reversion"] = st;
    assert_state(&s, c)?;
    Ok(s)
}
fn responsive_dip_exits(s: &mut V, b: &V, c: &V, eligible: bool, ask: &BigInt, take_profit: &V,
    horizon_ms: i64, max_market_cost: &V, same_version: bool) {
    let id = b["instrument"].as_str().unwrap_or("").to_string();
    let at = b["receivedAt"].as_str().unwrap_or("").to_string();
    let st = &mut s["strategies"]["mean_reversion"];
    let held_at_start = !st["positions"][&id].is_null();
    if same_version && st["pending"][&id].is_null() && !held_at_start { return; }
    let pending = st["pending"][&id].clone();
    let mut risk_reason: Option<String> = if pending["exitPolicy"].as_str() == Some("risk_reprice_v2") { pending["reason"].as_str().map(String::from) } else { None };
    if !pending.is_null() {
        if instant(&at).unwrap_or(0) - instant(pending["createdAt"].as_str().unwrap_or("")).unwrap_or(0)
            > c["maxDataAgeMs"].as_i64().unwrap_or(0) + c["cycleIntervalMs"].as_i64().unwrap_or(0) * 4 {
            st["pending"].as_object_mut().unwrap().remove(&id);
            st["expiredIntents"] = json!(st["expiredIntents"].as_i64().unwrap_or(0) + 1);
        } else if instant(b["requestStartedAt"].as_str().unwrap_or("")).unwrap_or(0)
            >= instant(pending["createdAt"].as_str().unwrap_or("")).unwrap_or(0) + c["latencyMs"].as_i64().unwrap_or(0) {
            fill_intent(st, &pending, b, c, &at);
        }
    }
    let pos = st["positions"][&id].clone();
    let bid = amount_of(&b["bids"][0]["price"]).unwrap_or(BigInt::from(0)) - BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0));
    if !pos.is_null() {
        let avg = amount_of(&pos["cost"]).unwrap_or(BigInt::from(0)) / BigInt::from(pos["quantity"].as_i64().unwrap_or(1));
        if risk_reason.is_none() {
            if &avg - &bid >= amount_of(&c["stopLoss"]).unwrap_or(BigInt::from(0)) { risk_reason = Some("dip_stop".into()); }
            else if instant(&at).unwrap_or(0) - instant(pos["enteredAt"].as_str().unwrap_or("")).unwrap_or(0) >= horizon_ms { risk_reason = Some("dip_timeout".into()); }
        }
        let waiting = st["pending"][&id].clone();
        if risk_reason.is_some()
            && (waiting.is_null() || (waiting["action"].as_str() == Some("sell")
                && (waiting["exitPolicy"].as_str() != Some("risk_reprice_v2") || bid < amount_of(&waiting["limit"]).unwrap_or(BigInt::from(0))))) {
            let limit = if bid < BigInt::from(TICK) { BigInt::from(TICK) } else { bid };
            enqueue(st, b, "sell", pos["quantity"].as_i64().unwrap_or(0), limit, &at, risk_reason.as_deref().unwrap_or(""));
            st["pending"][&id]["exitPolicy"] = json!("risk_reprice_v2");
        } else if waiting.is_null() && &bid - &avg >= big_amount(take_profit.as_str().unwrap_or("")).unwrap_or(BigInt::from(0)) {
            enqueue(st, b, "sell", pos["quantity"].as_i64().unwrap_or(0), bid, &at, "dip_take_profit");
        }
    } else if st["pending"][&id].is_null() && eligible && !held_at_start {
        let shares = c["orderShares"].as_i64().unwrap_or(0)
            .min((big_amount(max_market_cost.as_str().unwrap_or("")).unwrap_or(BigInt::from(0)) / ask).to_string().parse::<i64>().unwrap_or(0));
        if shares > 0 { enqueue(st, b, "buy", shares, ask.clone(), &at, "historical_discount"); }
    }
}
/// stepDip
pub fn step_dip(input: &V, b: &V, c: &V, reference: Option<f64>, allow_entry: bool, minimum_edge: f64,
    take_profit: &V, horizon_ms: i64, max_market_cost: &V, execution_policy: &str) -> Result<V, String> {
    if !["legacy_v1", "responsive_v2"].contains(&execution_policy) { return Err("Unknown dip execution policy".into()); }
    if !minimum_edge.is_finite() || minimum_edge <= 0.0 || horizon_ms < 1000
        || big_amount(take_profit.as_str().unwrap_or("")).map(|x| x <= BigInt::from(0)).unwrap_or(true)
        || big_amount(max_market_cost.as_str().unwrap_or("")).map(|x| x <= BigInt::from(0)).unwrap_or(true) {
        return Err("Invalid dip policy".into());
    }
    let id = b["instrument"].as_str().unwrap_or("").to_string();
    let previous = input["books"][&id].clone();
    let mut s = observe(input, b, c)?;
    s["strategies"] = input["strategies"].clone();
    let b = s["books"][&id].clone();
    let at = b["receivedAt"].as_str().unwrap_or("").to_string();
    let ask = amount_of(&b["asks"][0]["price"])? + BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0));
    let entry = ask.to_string().parse::<f64>().unwrap_or(0.0) / 1e6;
    let edge = reference.map(|r| r - entry);
    let eligible = allow_entry && edge.is_some_and(|e| e.is_finite() && e >= minimum_edge);
    let mut st = s["strategies"]["mean_reversion"].clone();
    if st["pending"][&id]["action"].as_str() == Some("buy") && !eligible {
        st["pending"].as_object_mut().unwrap().remove(&id);
        st["expiredIntents"] = json!(st["expiredIntents"].as_i64().unwrap_or(0) + 1);
    }
    s["strategies"]["mean_reversion"] = st.clone();
    if execution_policy == "responsive_v2" {
        let same_version = !previous.is_null() && previous["versionKey"] == b["versionKey"];
        responsive_dip_exits(&mut s, &b, c, eligible, &ask, take_profit, horizon_ms, max_market_cost, same_version);
        assert_state(&s, c)?;
        return Ok(s);
    }
    if !previous.is_null() && previous["versionKey"] == b["versionKey"] && st["pending"][&id].is_null() {
        assert_state(&s, c)?;
        return Ok(s);
    }
    if let Some(pending) = st["pending"].as_object().and_then(|p| p.get(&id)).cloned() {
        if instant(&at)? - instant(pending["createdAt"].as_str().unwrap_or(""))? > c["maxDataAgeMs"].as_i64().unwrap_or(0) + c["cycleIntervalMs"].as_i64().unwrap_or(0) * 4 {
            st["pending"].as_object_mut().unwrap().remove(&id);
            st["expiredIntents"] = json!(st["expiredIntents"].as_i64().unwrap_or(0) + 1);
        } else { fill_intent(&mut st, &pending, &b, c, &at); }
    }
    if st["pending"].as_object().is_none_or(|p| !p.contains_key(&id)) {
        let pos = st["positions"][&id].clone();
        let bid = amount_of(&b["bids"][0]["price"])? - BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0));
        if !pos.is_null() {
            let avg = amount_of(&pos["cost"])? / BigInt::from(pos["quantity"].as_i64().unwrap_or(1));
            if &bid - &avg >= big_amount(take_profit.as_str().unwrap_or(""))? || &avg - &bid >= amount_of(&c["stopLoss"])?
                || instant(&at)? - instant(pos["enteredAt"].as_str().unwrap_or(""))? >= horizon_ms {
                enqueue(&mut st, &b, "sell", pos["quantity"].as_i64().unwrap_or(0), bid.clone(), &at,
                    if &bid - &avg >= big_amount(take_profit.as_str().unwrap_or(""))? { "dip_take_profit" } else { "dip_stop_or_timeout" });
            }
        } else if eligible {
            let shares = c["orderShares"].as_i64().unwrap_or(0)
                .min((big_amount(max_market_cost.as_str().unwrap_or(""))? / &ask).to_string().parse::<i64>().unwrap_or(0));
            if shares > 0 { enqueue(&mut st, &b, "buy", shares, ask, &at, "historical_discount"); }
        }
    }
    s["strategies"]["mean_reversion"] = st;
    assert_state(&s, c)?;
    Ok(s)
}
