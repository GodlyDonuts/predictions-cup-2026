// live-paper/stable-maker-paper-v2.mjs — strict trade-through stable two-side paper.
use baal_execution::core::payload_hash;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("STABLE_MAKER_{}", code)); }
    Ok(())
}
fn instant(v: &V) -> Result<i64, String> { baal_paper::fixed::instant(v.as_str().unwrap_or("")) }
fn amount(v: &V) -> Result<BigInt, String> { baal_paper::fixed::big_amount(v.as_str().unwrap_or("")) }
fn money(v: &BigInt) -> String { baal_paper::fixed::money(v) }
fn scale() -> BigInt { BigInt::from(baal_paper::fixed::SCALE) }
fn tick() -> BigInt { BigInt::from(baal_paper::fixed::TICK) }
fn instant_ns(v: &V) -> Result<i128, String> {
    baal_service_runtime::contract::instant_ns(v.as_str().unwrap_or("")).map_err(|e| e.to_string())
}

/// STABLE_MAKER_POLICY
pub fn stable_maker_policy() -> V {
    json!({"schemaVersion": 2, "initialCash": "100.000000", "maximumReservedAndOwned": "5.000000",
        "maximumOwnedMarkets": 3, "cumulativeBuyBudget": "25.000000", "quantity": 1,
        "latencyMs": 3000, "lifetimeMs": 120000, "cooldownMs": 60000, "maximumHoldMs": 7200000,
        "stopLoss": "0.030000", "profitTarget": "0.005000", "exitAdverseTicks": 1,
        "depthFractionBps": 100, "feePerShare": "0.000000", "historyMs": 300000,
        "minimumHistorySpanMs": 240000, "minimumDistinctMinutes": 4, "maximumHistoryGapMs": 120000,
        "maximumMidpointRange": "0.005000", "minimumSpread": "0.005000", "maximumSpread": "0.050000",
        "improvementTicks": 0, "families": ["YES", "NO"],
        "fillModel": "hypothetical_strict_trade_through_not_proven_execution"})
}
/// validateStableMakerBank(bank)
pub fn validate_stable_maker_bank(bank: &V) -> Result<(), String> {
    need(bank["schemaVersion"].as_i64() == Some(1)
        && bank["mode"].as_str() == Some("stable_two_side_trade_tape_paper_v2")
        && payload_hash(&bank["policy"])? == payload_hash(&stable_maker_policy())?, "BANK")?;
    instant(&bank["cutoff"])?;
    instant(&bank["entryExpiresAt"])?;
    need(instant(&bank["entryExpiresAt"])? > instant(&bank["cutoff"])?, "CLOCK")?;
    let legs = bank["legs"].as_array().cloned().unwrap_or_default();
    need(!legs.is_empty() && legs.len() <= 16
        && legs.iter().map(|l| l["exchangeId"].as_str().unwrap_or("")).collect::<std::collections::HashSet<_>>().len()
            == legs.len(), "SCOPE")?;
    for l in &legs {
        let num_ok = |v: &V| v.as_str().is_some_and(|s| {
            let b = s.as_bytes();
            !b.is_empty() && b[0] != b'0' && b.iter().all(|c| c.is_ascii_digit())
        });
        need(num_ok(&l["marketId"]) && num_ok(&l["exchangeId"]), "IDENTITY")?;
    }
    Ok(())
}
/// createStableMakerState(bank)
pub fn create_stable_maker_state(bank: &V) -> Result<V, String> {
    validate_stable_maker_bank(bank)?;
    let mut wallets = Map::new();
    for side in bank["policy"]["families"].as_array().cloned().unwrap_or_default() {
        wallets.insert(side.as_str().unwrap_or("").to_string(), json!({"cash": bank["policy"]["initialCash"],
            "realized": "0.000000", "fees": "0.000000", "cumulativeBuys": "0.000000",
            "positions": {}, "quotes": {}, "fills": [], "cooldowns": {}, "risk": {}}));
    }
    Ok(json!({"schemaVersion": 1, "bankHash": payload_hash(bank)?, "lastAt": bank["cutoff"],
        "nextId": 1, "seen": {}, "books": {}, "history": {}, "coverage": {}, "counts": {},
        "wallets": wallets}))
}
fn inc(s: &mut V, k: &str) {
    let n = s["counts"][k].as_i64().unwrap_or(0);
    s["counts"][k] = json!(n + 1);
}
fn open_quotes(w: &V) -> Vec<V> {
    w["quotes"].as_object().cloned().unwrap_or_default().values()
        .filter(|q| q["resolvedAt"].is_null()).cloned().collect()
}
fn reserved(w: &V) -> Result<BigInt, String> {
    let mut n = BigInt::from(0);
    for q in open_quotes(w) {
        if q["action"].as_str() == Some("buy") { n += amount(&q["limit"])?; }
    }
    Ok(n)
}
fn owned(w: &V) -> Result<BigInt, String> {
    let mut n = BigInt::from(0);
    for (_k, p) in w["positions"].as_object().cloned().unwrap_or_default() {
        n += amount(&p["cost"])?;
    }
    Ok(n)
}
fn open_for(w: &V, key: &str) -> bool {
    open_quotes(w).iter().any(|q| q["exchangeId"].as_str() == Some(key))
}
fn prices(b: &V, side: &str) -> Result<(BigInt, BigInt, f64, f64), String> {
    if side == "YES" {
        Ok((amount(&b["bids"][0]["price"])?, amount(&b["asks"][0]["price"])?,
            b["bids"][0]["quantity"].as_f64().unwrap_or(0.0), b["bids"][0]["quantity"].as_f64().unwrap_or(0.0)))
    } else {
        Ok((scale() - amount(&b["asks"][0]["price"])?, scale() - amount(&b["bids"][0]["price"])?,
            b["asks"][0]["quantity"].as_f64().unwrap_or(0.0), b["asks"][0]["quantity"].as_f64().unwrap_or(0.0)))
    }
}
/// assertStableMaker(s, bank)
pub fn assert_stable_maker(s: &V, bank: &V) -> Result<(), String> {
    need(s["bankHash"].as_str() == Some(payload_hash(bank)?.as_str()), "BANK_CHANGED")?;
    for (_side, w) in s["wallets"].as_object().cloned().unwrap_or_default() {
        let fills = w["fills"].as_array().cloned().unwrap_or_default();
        let buys: BigInt = fills.iter().filter(|f| f["action"].as_str() == Some("buy"))
            .map(|f| amount(&f["price"])).collect::<Result<Vec<_>, _>>()?.into_iter().sum();
        let sells: BigInt = fills.iter().filter(|f| f["action"].as_str() == Some("sell"))
            .map(|f| amount(&f["price"])).collect::<Result<Vec<_>, _>>()?.into_iter().sum();
        need(amount(&w["cash"])? == amount(&bank["policy"]["initialCash"])? - &buys + &sells
            && amount(&w["cash"])? >= reserved(&w)?, "CASH")?;
        need(amount(&w["cumulativeBuys"])? == buys
            && buys <= amount(&bank["policy"]["cumulativeBuyBudget"])?, "TURNOVER")?;
        need(amount(&w["realized"])? == &sells - (&buys - owned(&w)?)
            && w["fees"].as_str() == Some("0.000000"), "FIFO")?;
        need(owned(&w)? + reserved(&w)? <= amount(&bank["policy"]["maximumReservedAndOwned"])?
            && w["positions"].as_object().cloned().unwrap_or_default().values()
                .all(|p| p["quantity"].as_f64() == Some(1.0)), "RISK")?;
    }
    Ok(())
}
fn fill(s: &mut V, side: &str, mut q: V, at: &str, source: &V, bank: &V) -> Result<(), String> {
    let w = &mut s["wallets"][side];
    let key = q["exchangeId"].as_str().unwrap_or("").to_string();
    let cost = amount(&q["limit"])?;
    if q["action"].as_str() == Some("buy") {
        need(w["positions"][&key].is_null() && amount(&w["cash"])? >= cost, "BUY")?;
        w["cash"] = json!(money(&(amount(&w["cash"])? - &cost)));
        w["cumulativeBuys"] = json!(money(&(amount(&w["cumulativeBuys"])? + &cost)));
        w["positions"][&key] = json!({"marketId": q["marketId"], "exchangeId": key, "side": q["side"],
            "quantity": 1, "cost": q["limit"], "enteredAt": at,
            "sourceEntryAt": if source["kind"].as_str() == Some("trade") { source["createdAt"].clone() } else { json!(at) }});
    } else {
        need(w["positions"][&key]["quantity"].as_f64() == Some(1.0), "COVERED_EXIT")?;
        w["cash"] = json!(money(&(amount(&w["cash"])? + &cost)));
        w["realized"] = json!(money(&(amount(&w["realized"])? + &cost - amount(&w["positions"][&key]["cost"])?)));
        w["positions"].as_object_mut().and_then(|p| p.shift_remove(&key));
        w["risk"].as_object_mut().and_then(|r| r.shift_remove(&key));
        w["cooldowns"][&key] = json!(instant(&json!(at))? + bank["policy"]["cooldownMs"].as_i64().unwrap_or(0));
    }
    q["resolvedAt"] = json!(at);
    q["resolution"] = json!("hypothetical_fill");
    w["fills"].as_array_mut().map(|f| f.push(json!({"action": q["action"], "marketId": q["marketId"],
        "exchangeId": key, "side": q["side"], "quantity": 1, "price": q["limit"], "at": at,
        "quoteId": q["id"], "source": source.clone(),
        "model": if source["kind"].as_str() == Some("trade") { bank["policy"]["fillModel"].clone() }
            else { json!("hypothetical_later_request_taker") }})));
    inc(s, &format!("hypothetical_{}", q["action"].as_str().unwrap_or("")));
    Ok(())
}
fn quote(s: &mut V, b: &V, side: &str, at: &str, action: &str, limit: &BigInt, bank: &V) -> Result<(), String> {
    need(*limit >= tick() && *limit <= scale() - tick() && limit.clone() % tick() == BigInt::from(0), "PRICE")?;
    let source_ns_ms = (instant_ns(&b["sourceAt"])? / 1_000_000i128) as i64;
    let activation = (instant(&json!(at))? + bank["policy"]["latencyMs"].as_i64().unwrap_or(0))
        .max(source_ns_ms + 2000);
    let expiry = instant(&json!(at))? + bank["policy"]["lifetimeMs"].as_i64().unwrap_or(0);
    let id = s["nextId"].as_i64().unwrap_or(0);
    s["nextId"] = json!(id + 1);
    let w = &mut s["wallets"][side];
    let q = json!({"id": id, "action": action, "side": side, "marketId": b["marketId"],
        "exchangeId": b["exchangeId"], "quantity": 1, "limit": money(limit),
        "yesEquivalentLimit": money(&if side == "YES" { limit.clone() } else { scale() - limit }),
        "createdAt": at, "activationAt": baal_service_runtime::contract::ms_to_iso(activation as f64),
        "expiresAt": baal_service_runtime::contract::ms_to_iso(expiry as f64),
        "bookVersion": b["versionKey"], "bookSequence": b["sequence"],
        "publicationHash": b["publicationHash"], "publicationSequence": b["publicationSequence"],
        "coverageBarrierSequence": V::Null, "resolvedAt": V::Null, "resolution": V::Null});
    w["quotes"][id.to_string()] = q;
    inc(s, &format!("quote_{}", action));
    Ok(())
}
fn observe_history(s: &mut V, b: &V, at: &str, p: &V) -> Result<bool, String> {
    let k = b["exchangeId"].as_str().unwrap_or("").to_string();
    let row = json!({"at": instant(&json!(at))?, "request": instant(&b["requestStartedAt"])?,
        "bid": b["bids"][0]["price"], "ask": b["asks"][0]["price"], "version": b["versionKey"]});
    let mut rows: Vec<V> = s["history"][&k].as_array().cloned().unwrap_or_default();
    if let Some(last) = rows.last() {
        if row["at"].as_i64().unwrap_or(0) - last["at"].as_i64().unwrap_or(0) > p["maximumHistoryGapMs"].as_i64().unwrap_or(0)
            || row["request"].as_i64().unwrap_or(0) - last["request"].as_i64().unwrap_or(0) > p["maximumHistoryGapMs"].as_i64().unwrap_or(0)
        {
            rows = Vec::new();
            inc(s, "history_gap");
        }
    }
    if rows.is_empty() || rows.last().map(|l| &l["version"]) != Some(&row["version"]) {
        rows.push(row.clone());
    }
    let horizon = row["at"].as_i64().unwrap_or(0) - p["historyMs"].as_i64().unwrap_or(0);
    rows.retain(|x| x["at"].as_i64().unwrap_or(0) >= horizon);
    s["history"][&k] = json!(rows);
    if rows.len() < 4
        || row["at"].as_i64().unwrap_or(0) - rows[0]["at"].as_i64().unwrap_or(0) < p["minimumHistorySpanMs"].as_i64().unwrap_or(0)
        || rows.iter().map(|x| x["at"].as_i64().unwrap_or(0) / 60000).collect::<std::collections::HashSet<_>>().len()
            < p["minimumDistinctMinutes"].as_i64().unwrap_or(0) as usize
    {
        return Ok(false);
    }
    let mut mid = Vec::new();
    for x in &rows {
        mid.push(amount(&x["bid"])? + amount(&x["ask"])?);
    }
    let max = mid.iter().cloned().max().unwrap_or_default();
    let min = mid.iter().cloned().min().unwrap_or_default();
    Ok(max - min <= BigInt::from(2) * amount(&p["maximumMidpointRange"])?)
}
fn record_coverage(s: &mut V, page: &V) -> Result<(), String> {
    let exchange_id = page["exchangeId"].as_str().unwrap_or("").to_string();
    if s["coverage"][&exchange_id].is_null() {
        s["coverage"][&exchange_id] = json!({"pending": {}, "windows": []});
    }
    let request = if page["requestCursor"].is_null() { None } else { Some(page["requestCursor"].clone()) };
    let key = payload_hash(&json!([page["from"], page["to"]]))?;
    let mut chain = if request.is_none() {
        json!({"from": page["from"], "to": page["to"], "expected": V::Null, "complete": true, "checkpoint": V::Null})
    } else {
        s["coverage"][&exchange_id]["pending"][&key].clone()
    };
    if chain.is_null()
        || chain["expected"] != request.clone().unwrap_or(V::Null)
        || chain["from"] != page["from"] || chain["to"] != page["to"]
    {
        s["coverage"][&exchange_id]["pending"].as_object_mut().and_then(|p| p.shift_remove(&key));
        inc(s, "coverage_cursor_gap");
        return Ok(());
    }
    let cp = &page["coverage"]["projectedThroughSequence"];
    let new_complete = chain["complete"].as_bool().unwrap_or(false)
        && page["coverage"]["complete"] == V::Bool(true) && !cp.is_null();
    chain["complete"] = json!(new_complete);
    if !cp.is_null() {
        let cp_big: BigInt = cp.as_str().unwrap_or(&cp.to_string()).parse().unwrap_or_default();
        chain["checkpoint"] = if chain["checkpoint"].is_null() {
            json!(cp.as_str().map(String::from).unwrap_or_else(|| cp.to_string()))
        } else {
            let cur: BigInt = chain["checkpoint"].as_str().unwrap_or("0").parse().unwrap_or_default();
            json!(if cur < cp_big { chain["checkpoint"].clone() } else { json!(cp.as_str().map(String::from).unwrap_or_else(|| cp.to_string())) })
        };
    }
    if baal_service_runtime::json_text::js_truthy(page["pagination"].get("hasMore")) {
        need(page["pagination"]["nextCursor"].is_string()
            && !page["pagination"]["nextCursor"].as_str().unwrap_or("").is_empty()
            && page["pagination"]["nextCursor"] != request.clone().unwrap_or(V::Null), "CURSOR")?;
        chain["expected"] = page["pagination"]["nextCursor"].clone();
        s["coverage"][&exchange_id]["pending"][&key] = chain;
        need(s["coverage"][&exchange_id]["pending"].as_object().map(|p| p.len()).unwrap_or(0) <= 16, "WINDOW_LIMIT")?;
    } else {
        if chain["complete"].as_bool().unwrap_or(false) {
            let entry = json!({"from": chain["from"], "to": chain["to"], "checkpoint": chain["checkpoint"]});
            s["coverage"][&exchange_id]["windows"].as_array_mut().map(|w| w.push(entry));
        }
        s["coverage"][&exchange_id]["pending"].as_object_mut().and_then(|p| p.shift_remove(&key));
    }
    let mut oldest = instant(&s["lastAt"])?;
    let mut any = false;
    for (_side, w) in s["wallets"].as_object().cloned().unwrap_or_default() {
        for q in open_quotes(&w) {
            if q["exchangeId"].as_str() == Some(exchange_id.as_str()) {
                any = true;
                let a = instant(&q["activationAt"])?;
                if a < oldest { oldest = a; }
            }
        }
    }
    let _ = any;
    let windows: Vec<V> = s["coverage"][&exchange_id]["windows"].as_array().cloned().unwrap_or_default();
    let kept: Vec<V> = windows.into_iter()
        .filter(|x| instant(&x["to"]).map(|t| t > oldest).unwrap_or(false)).collect();
    s["coverage"][&exchange_id]["windows"] = json!(kept);
    Ok(())
}
fn covered(c: &V, q: &V) -> Result<bool, String> {
    let barrier: BigInt = q["coverageBarrierSequence"].as_str()
        .unwrap_or(&q["coverageBarrierSequence"].to_string()).parse().unwrap_or_default();
    let mut through = instant(&q["activationAt"])?;
    let mut windows: Vec<V> = c["windows"].as_array().cloned().unwrap_or_default().into_iter()
        .filter(|x| {
            let cp: BigInt = x["checkpoint"].as_str().unwrap_or("0").parse().unwrap_or_default();
            cp >= barrier
        }).collect();
    windows.sort_by(|a, b| instant(&a["from"]).unwrap_or(0).cmp(&instant(&b["from"]).unwrap_or(0)));
    for x in &windows {
        let from = instant(&x["from"])?;
        if from > through { break; }
        through = through.max(instant(&x["to"])?);
        if through >= instant(&q["expiresAt"])? { return Ok(true); }
    }
    Ok(false)
}
/// advanceStableMaker(state, event, bank)
pub fn advance_stable_maker(state: &V, event: &V, bank: &V) -> Result<V, String> {
    need(state["bankHash"].as_str() == Some(payload_hash(bank)?.as_str()), "BANK_CHANGED")?;
    let mut s = state.clone();
    let p = &bank["policy"];
    let at = event["availableAt"].as_str().unwrap_or("");
    let now = instant(&json!(at))?;
    need(now >= instant(&s["lastAt"])?, "CLOCK")?;
    s["lastAt"] = json!(at);
    if event["kind"].as_str() == Some("boundary") {
        s["books"] = json!({});
        s["history"] = json!({});
        inc(&mut s, "source_boundary");
    } else if event["kind"].as_str() == Some("book") {
        let b = &event["book"];
        let key = b["exchangeId"].as_str().unwrap_or("").to_string();
        need(bank["legs"].as_array().is_some_and(|l| l.iter()
                .any(|l| l["marketId"] == b["marketId"] && l["exchangeId"].as_str() == Some(key.as_str())))
            && b["instrument"].as_str() == Some(format!("{}:YES", key).as_str())
            && b["tournamentId"] == bank["tournamentId"], "BOOK_SCOPE")?;
        need(instant(&b["receivedAt"])? <= now && instant(&b["requestStartedAt"])? <= instant(&b["receivedAt"])?, "RECEIPT")?;
        let fresh = now - instant(&b["requestStartedAt"])? <= 20000
            && crate::probability_receipt::probability_account_gate(&event["accountEligibility"], b, at).is_none();
        let stable = fresh && observe_history(&mut s, b, at, p)?;
        if fresh {
            s["books"][&key] = b.clone();
        } else {
            s["books"].as_object_mut().and_then(|m| m.shift_remove(&key));
            s["history"].as_object_mut().and_then(|m| m.shift_remove(&key));
            inc(&mut s, "book_ineligible");
        }
        let sides: Vec<String> = s["wallets"].as_object().map(|w| w.keys().cloned().collect()).unwrap_or_default();
        for side in sides {
            {
                let w = &mut s["wallets"][&side];
                for (_qk, q) in w["quotes"].as_object_mut().map(|m| m.iter_mut().collect::<Vec<_>>()).unwrap_or_default() {
                    if q["resolvedAt"].is_null() && q["exchangeId"].as_str() == Some(key.as_str())
                        && q["coverageBarrierSequence"].is_null()
                        && instant(&b["requestStartedAt"])? >= instant(&q["expiresAt"])?
                        && instant_ns(&b["sourceAt"])? >= (instant(&q["expiresAt"])? as i128) * 1_000_000i128
                    {
                        q["coverageBarrierSequence"] = b["sequence"].clone();
                    }
                }
            }
            if !fresh { continue; }
            let px = prices(b, &side)?;
            let (bid, ask, buy_depth, sell_depth) = px;
            let position_exists = !s["wallets"][&side]["positions"][&key].is_null();
            let mark = bid.clone() - tick();
            if position_exists {
                let position = s["wallets"][&side]["positions"][&key].clone();
                let timed_out = now - instant(&position["sourceEntryAt"])? >= p["maximumHoldMs"].as_i64().unwrap_or(0);
                let loss = amount(&position["cost"])? - &mark >= amount(&p["stopLoss"])?;
                let mut sell_fill = false;
                let mut sell_quote: Option<BigInt> = None;
                {
                    let w = &mut s["wallets"][&side];
                    if (timed_out || loss)
                        && (w["risk"][&key].is_null() || w["risk"][&key]["reason"].as_str() == Some("profit_available"))
                    {
                        w["risk"][&key] = json!({"createdAt": at,
                            "reason": if timed_out { "holding_limit" } else { "loss_limit" }});
                    }
                    // Existing resting orders retain their original server expiry, including on
                    // a risk signal or source boundary. A later print must be resolved first.
                    if !w["risk"][&key].is_null() {
                        let allowed = w["risk"][&key]["reason"].as_str() != Some("profit_available")
                            || mark >= amount(&position["cost"])? + amount(&p["profitTarget"])?;
                        if allowed && !open_for(w, &key)
                            && instant(&b["requestStartedAt"])? >= instant(&w["risk"][&key]["createdAt"])? + p["latencyMs"].as_i64().unwrap_or(0)
                            && mark > BigInt::from(0)
                            && (sell_depth * p["depthFractionBps"].as_f64().unwrap_or(0.0) / 10000.0).floor() >= 1.0
                        {
                            sell_fill = true;
                        }
                    } else if !open_for(w, &key) {
                        let limit = amount(&position["cost"])? + amount(&p["profitTarget"])?;
                        if limit > bid && limit <= scale() - tick() {
                            sell_quote = Some(limit);
                        } else if limit <= mark && w["risk"][&key].is_null() {
                            w["risk"][&key] = json!({"createdAt": at, "reason": "profit_available"});
                        }
                    }
                }
                if sell_fill {
                    let id = s["nextId"].as_i64().unwrap_or(0);
                    s["nextId"] = json!(id + 1);
                    let q = json!({"id": id, "action": "sell", "side": side,
                        "marketId": b["marketId"], "exchangeId": key, "limit": money(&mark)});
                    fill(&mut s, &side, q, at, &json!({"kind": "book", "version": b["versionKey"],
                        "publicationHash": b["publicationHash"], "publicationSequence": b["publicationSequence"]}), bank)?;
                } else if let Some(limit) = sell_quote {
                    quote(&mut s, b, &side, at, "sell", &limit, bank)?;
                }
                continue;
            }
            {
                let w = &mut s["wallets"][&side];
                if instant(&b["requestStartedAt"])? <= instant(&bank["cutoff"])?
                    || now >= instant(&bank["entryExpiresAt"])?
                    || open_for(w, &key)
                    || w["cooldowns"][&key].as_i64().unwrap_or(0) > now
                {
                    continue;
                }
            }
            if !stable {
                inc(&mut s, "unstable_or_warmup");
                continue;
            }
            let spread = ask.clone() - bid.clone();
            let limit = bid.clone() + BigInt::from(p["improvementTicks"].as_i64().unwrap_or(0)) * tick();
            if spread < amount(&p["minimumSpread"])? || spread > amount(&p["maximumSpread"])?
                || limit >= ask || limit < scale() / BigInt::from(20) || limit > BigInt::from(19) * scale() / BigInt::from(20)
                || (buy_depth * p["depthFractionBps"].as_f64().unwrap_or(0.0) / 10000.0).floor() < 1.0
            {
                inc(&mut s, "spread_or_depth");
                continue;
            }
            let w = &mut s["wallets"][&side];
            let mut active: std::collections::HashSet<String> = w["positions"].as_object()
                .map(|x| x.keys().cloned().collect()).unwrap_or_default();
            for q in open_quotes(w) {
                if q["action"].as_str() == Some("buy") {
                    active.insert(q["exchangeId"].as_str().unwrap_or("").to_string());
                }
            }
            if active.len() >= p["maximumOwnedMarkets"].as_i64().unwrap_or(0) as usize
                || owned(w)? + reserved(w)? + &limit > amount(&p["maximumReservedAndOwned"])?
                || amount(&w["cumulativeBuys"])? + reserved(w)? + &limit > amount(&p["cumulativeBuyBudget"])?
                || amount(&w["cash"])? - reserved(w)? < limit
            {
                inc(&mut s, "capital");
                continue;
            }
            quote(&mut s, b, &side, at, "buy", &limit, bank)?;
        }
    } else if event["kind"].as_str() == Some("trades") {
        let page = &event["page"];
        need(page["tournamentId"] == bank["tournamentId"]
            && bank["legs"].as_array().is_some_and(|l| l.iter()
                .any(|l| l["marketId"] == page["marketId"] && l["exchangeId"] == page["exchangeId"]))
            && instant(&page["receivedAt"])? <= now, "PAGE_SCOPE")?;
        let mut trades = page["trades"].as_array().cloned().unwrap_or_default();
        trades.sort_by(|a, b| {
            let ta = instant_ns(&a["createdAt"]).unwrap_or(0);
            let tb = instant_ns(&b["createdAt"]).unwrap_or(0);
            ta.cmp(&tb).then_with(|| a["key"].as_str().unwrap_or("").cmp(b["key"].as_str().unwrap_or("")))
        });
        for t in &trades {
            need(instant_ns(&t["createdAt"])? >= (instant(&page["from"])? as i128) * 1_000_000i128
                && instant_ns(&t["createdAt"])? < (instant(&page["to"])? as i128) * 1_000_000i128, "TRADE_WINDOW")?;
            if !s["seen"][t["key"].as_str().unwrap_or("")].is_null() { continue; }
            s["seen"][t["key"].as_str().unwrap_or("")] = json!(true);
            let tick_v = crate::canonical_entry_exit_study::tape_tick(t)?;
            let unusable = tick_v.is_none()
                || tick_v.as_ref().map(|tk| tk["tick"].as_i64()).is_some_and(|t| t == Some(0) || t == Some(200));
            if unusable {
                inc(&mut s, "unusable_trade");
                continue;
            }
            let tick_v = tick_v.unwrap();
            let sides: Vec<String> = s["wallets"].as_object().map(|w| w.keys().cloned().collect()).unwrap_or_default();
            for side in sides {
                let quote_ids: Vec<String> = open_quotes(&s["wallets"][&side]).iter()
                    .filter_map(|q| q["id"].as_i64().map(|i| i.to_string())).collect();
                for qid in quote_ids {
                    let q = s["wallets"][&side]["quotes"][&qid].clone();
                    if q["exchangeId"] != page["exchangeId"]
                        || instant_ns(&t["createdAt"])? < (instant(&q["activationAt"])? as i128) * 1_000_000i128
                        || instant_ns(&t["createdAt"])? >= (instant(&q["expiresAt"])? as i128) * 1_000_000i128
                    {
                        continue;
                    }
                    let yes = amount(&tick_v["price"])?;
                    let limit = amount(&q["yesEquivalentLimit"])?;
                    let buy_side_match = (q["action"].as_str() == Some("buy")) == (side == "YES");
                    let through = if buy_side_match { yes <= limit.clone() - tick() } else { yes >= limit.clone() + tick() };
                    if !through { continue; }
                    need(t["size"].as_f64().unwrap_or(0.0) >= 1.0, "TRADE_SIZE")?;
                    fill(&mut s, &side, q, at, &json!({"kind": "trade", "key": t["key"], "id": t["id"],
                        "createdAt": t["createdAt"], "firstAvailableAt": page["receivedAt"],
                        "normalizedYesPrice": tick_v["price"], "aggressorSide": V::Null,
                        "queuePositionProven": false, "executionProven": false}), bank)?;
                }
            }
        }
        record_coverage(&mut s, page)?;
        let sides: Vec<String> = s["wallets"].as_object().map(|w| w.keys().cloned().collect()).unwrap_or_default();
        for side in sides {
            let quote_ids: Vec<String> = open_quotes(&s["wallets"][&side]).iter()
                .filter_map(|q| q["id"].as_i64().map(|i| i.to_string())).collect();
            for qid in quote_ids {
                let q = s["wallets"][&side]["quotes"][&qid].clone();
                if q["exchangeId"] == page["exchangeId"] && !q["coverageBarrierSequence"].is_null()
                    && covered(&s["coverage"][q["exchangeId"].as_str().unwrap_or("")], &q)?
                {
                    s["wallets"][&side]["quotes"][&qid]["resolvedAt"] = json!(at);
                    s["wallets"][&side]["quotes"][&qid]["resolution"] = json!("projection_observed_without_qualifying_trade");
                    inc(&mut s, "quote_unfilled_resolved");
                }
            }
        }
    } else {
        need(false, "EVENT_KIND")?;
    }
    assert_stable_maker(&s, bank)?;
    Ok(s)
}
/// stableMakerReport(s, bank, at)
pub fn stable_maker_report(s: &V, bank: &V, at: &str) -> Result<V, String> {
    assert_stable_maker(s, bank)?;
    let mut wallets = Map::new();
    for (side, w) in s["wallets"].as_object().cloned().unwrap_or_default() {
        let mut proceeds = BigInt::from(0);
        let mut complete = true;
        let mut positions = Vec::new();
        for (_pk, pos) in w["positions"].as_object().cloned().unwrap_or_default() {
            let b = &s["books"][pos["exchangeId"].as_str().unwrap_or("")];
            let mark: Option<BigInt> = if !b.is_null() {
                let (bid, _ask, _bd, sell_depth) = prices(b, &side)?;
                let fresh = instant(&json!(at))? >= instant(&b["receivedAt"])?
                    && instant(&json!(at))? - instant(&b["receivedAt"])? <= 20000;
                if fresh
                    && (sell_depth * bank["policy"]["depthFractionBps"].as_f64().unwrap_or(0.0) / 10000.0).floor() >= 1.0
                {
                    Some(bid - tick())
                } else {
                    None
                }
            } else {
                None
            };
            match &mark {
                Some(m) if *m > BigInt::from(0) => { proceeds += m; }
                _ => { complete = false; }
            }
            let mut p2 = pos.clone();
            p2["estimatedLiquidationProceeds"] = match &mark {
                Some(m) if *m > BigInt::from(0) => json!(money(m)),
                _ => V::Null,
            };
            positions.push(p2);
        }
        let fills = w["fills"].as_array().cloned().unwrap_or_default();
        let closed = fills.iter().filter(|f| f["action"].as_str() == Some("sell")).count();
        let recent = fills.iter().rev().take(12).rev().cloned().collect::<Vec<_>>();
        wallets.insert(side, json!({"cash": w["cash"], "realizedPnl": w["realized"], "fees": w["fees"],
            "cumulativeBuys": w["cumulativeBuys"], "fills": fills.len(), "closedTrades": closed,
            "positions": positions, "unresolvedQuotes": open_quotes(&w), "reserved": money(&reserved(&w)?),
            "recentFills": recent,
            "estimatedLiquidationPnl": if complete {
                json!(money(&(amount(&w["cash"])? + proceeds - amount(&bank["policy"]["initialCash"])?)))
            } else { V::Null }}));
    }
    Ok(json!({"schemaVersion": 1, "mode": bank["mode"], "cohort": bank["name"],
        "bankHash": s["bankHash"], "updatedAt": at, "cutoff": bank["cutoff"], "counts": s["counts"],
        "wallets": wallets, "executionAuthority": false, "competitionOrders": 0, "networkRequests": 0,
        "profitabilityEstablished": false,
        "limitations": ["Strict trade-through is a counterfactual fill assumption; anonymous trades do not prove our queue position or actual execution.",
            "YES and NO wallets share opportunities, are correlated, and must not be added as independent returns.",
            "Incomplete tape coverage retains cash and inventory reservations; source gaps do not create trades."]}))
}
