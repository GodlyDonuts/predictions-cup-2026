// live-paper/bid-anchor.mjs — bid-anchor mean-reversion replay over published
// books with dual latency wallets. Local counterfactual paper accounting.
use crate::edge_depth_audit::one_share_depth;
use crate::model::{digest, initial_state, invalidate, report, step_dip, validate_config};
use baal_paper::fixed::{big_amount, instant, money, TICK};
use baal_service_runtime::json_text::js_truthy;
use num_bigint::BigInt;
use serde_json::{json, Value as V};

/// BID_ANCHOR_POLICY (frozen object).
pub fn bid_anchor_policy() -> V {
    json!({"historyMs":600000,"minimumMinutes":5,"minimumSpanMs":240000,"maximumGapMs":90000,
        "maximumAnchorRange":"0.015000","minimumNetConcession":"0.010000","maximumBidDrop":"0.040000",
        "maximumSpread":"0.020000","horizonMs":300000,"cooldownMs":900000,
        "cumulativeBuyBudget":"100.000000","maxOpenMarkets":3,"latenciesMs":[1000,3000]})
}
fn need(ok: bool, msg: &str) -> Result<(), String> { if !ok { return Err(format!("BID_ANCHOR_{msg}")); } Ok(()) }
fn strict_instant(v: &V) -> Result<i64, String> { instant(v.as_str().unwrap_or("")) }

/// bidAnchorSignal(points, b, policy)
pub fn bid_anchor_signal(points: &[V], b: &V, policy: &V) -> Result<V, String> {
    let now = strict_instant(&b["receivedAt"])?;
    let history_ms = policy["historyMs"].as_i64().unwrap_or(0);
    let mut a: Vec<(&V, i64)> = Vec::new();
    for p in points {
        let t = strict_instant(&p["at"])?;
        if t < now && t >= now - history_ms { a.push((p, t)); }
    }
    let reject = |reason: &str, n: usize| -> V {
        json!({"admitted":false,"reason":reason,"samples":n,"anchor":null})
    };
    if (a.len() as i64) < policy["minimumMinutes"].as_i64().unwrap_or(0) { return Ok(reject("history_minutes", a.len())); }
    if a.last().unwrap().1 - a[0].1 < policy["minimumSpanMs"].as_i64().unwrap_or(0) {
        return Ok(reject("history_span", a.len()));
    }
    if now - a.last().unwrap().1 > policy["maximumGapMs"].as_i64().unwrap_or(0)
        || (1..a.len()).any(|i| a[i].1 - a[i - 1].1 > policy["maximumGapMs"].as_i64().unwrap_or(0)) {
        return Ok(reject("history_gap", a.len()));
    }
    let a: Vec<&V> = a.iter().map(|(p, _)| *p).collect();
    let mut values: Vec<BigInt> = a.iter().map(|p| big_amount(p["bid"].as_str().unwrap_or("")))
        .collect::<Result<_, _>>()?;
    values.sort();
    let anchor = values[((values.len() - 1) / 2) as usize].clone();
    let low = values[0].clone();
    let high = values.last().unwrap().clone();
    let bid = big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?;
    let ask = big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))?;
    let entry = &ask + BigInt::from(TICK);
    if &high - &low > big_amount(policy["maximumAnchorRange"].as_str().unwrap_or(""))? { return Ok(reject("unstable_anchor", a.len())); }
    if &ask - &bid > big_amount(policy["maximumSpread"].as_str().unwrap_or(""))? { return Ok(reject("spread", a.len())); }
    if &anchor - &bid > big_amount(policy["maximumBidDrop"].as_str().unwrap_or(""))? { return Ok(reject("bid_collapse", a.len())); }
    if b["asks"][0]["quantity"].as_f64().map_or(false, |q| q < 100.0) {
        return Ok(reject("entry_depth", a.len()));
    }
    // Prior bid less adverse exit tick must exceed the padded entry by the margin.
    let margin = &anchor - BigInt::from(TICK) - &entry;
    let min_concession = big_amount(policy["minimumNetConcession"].as_str().unwrap_or(""))?;
    Ok(json!({
        "admitted": margin >= min_concession,
        "reason": if margin >= min_concession { "ask_concession" } else { "insufficient_concession" },
        "samples": a.len(),
        "anchor": money(&anchor),
        "modeledReference": money(&(&anchor - BigInt::from(TICK))),
        "concession": money(&margin),
        "historyFirstAt": a[0]["at"],
        "historyLastAt": a.last().unwrap()["at"],
    }))
}

/// createBidAnchorState(bank)
pub fn create_bid_anchor_state(bank: &V) -> Result<V, String> {
    need(digest(&bank["policy"]) == digest(&bid_anchor_policy()), "POLICY")?;
    validate_config(&bank["config"])?;
    let mut wallets = serde_json::Map::new();
    for latency in bank["policy"]["latenciesMs"].as_array().map(|v| v.as_slice()).unwrap_or(&[]) {
        let mut c = bank["config"].clone();
        c["latencyMs"] = latency.clone();
        let mut state = initial_state(&c, &format!("bid-anchor:{}", latency.as_i64().unwrap_or(0)), bank["cutoff"].as_str().unwrap_or(""))?;
        state["lastAt"] = json!("1970-01-01T00:00:00.000Z");
        wallets.insert(latency.to_string(), json!({"state": state, "cooldowns": {}}));
    }
    Ok(json!({"bankHash":digest(bank),"histories":{},"seen":{},"counts":{},"signals":[],"wallets":wallets}))
}

/// advanceBidAnchor(s, row, bank) — mutates s in place; returns Result.
pub fn advance_bid_anchor(s: &mut V, row: &V, bank: &V) -> Result<(), String> {
    let bank_hash = digest(bank);
    need(s["bankHash"].as_str() == Some(bank_hash.as_str()), "BANK")?;
    let p = bank["policy"].clone();
    let kind = row["kind"].as_str().unwrap_or("");
    let inc = |s: &mut V, key: &str| {
        s["counts"][key] = json!(s["counts"][key].as_i64().unwrap_or(0) + 1);
    };
    if ["session_start", "session_stop", "invalidation"].contains(&kind) {
        let id = if kind == "invalidation" { row["payload"]["marketId"].as_str().map(|x| x.to_string()) } else { None };
        if id.is_none() {
            s["histories"] = json!({});
            s["seen"] = json!({});
        } else {
            s["histories"].as_object_mut().map(|h| h.remove(id.as_deref().unwrap()));
            s["seen"].as_object_mut().map(|h| h.remove(id.as_deref().unwrap()));
        }
        let wallet_keys: Vec<String> = s["wallets"].as_object().map(|w| w.keys().cloned().collect()).unwrap_or_default();
        for key in wallet_keys {
            invalidate(&mut s["wallets"][key.as_str()]["state"], id.as_deref(), row["at"].as_str().unwrap_or(""), "bid_anchor_source_boundary");
            let pending_keys: Vec<String> = s["wallets"][key.as_str()]["state"]["strategies"]["mean_reversion"]["pending"]
                .as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
            for pk in pending_keys {
                let pend = &s["wallets"][key.as_str()]["state"]["strategies"]["mean_reversion"]["pending"][pk.as_str()];
                if pend["action"].as_str() == Some("buy")
                    && (id.is_none()
                        || s["wallets"][key.as_str()]["state"]["books"][pk.as_str()]["marketId"].as_str() == id.as_deref()) {
                    s["wallets"][key.as_str()]["state"]["strategies"]["mean_reversion"]["pending"]
                        .as_object_mut().map(|o| o.remove(&pk));
                }
            }
        }
        inc(s, "source_boundaries");
        return Ok(());
    }
    if kind != "book" { return Ok(()); }
    let b = row["payload"]["book"].clone();
    let id = b["marketId"].as_str().unwrap_or("").to_string();
    need(bank["universe"]["exchanges"].as_array().map(|xs| xs.iter()
        .any(|x| x["marketId"] == b["marketId"] && x["exchangeId"] == b["exchangeId"])).unwrap_or(false)
        && b["tournamentId"] == bank["config"]["tournamentId"], "SCOPE")?;
    if s["seen"][&id] == b["versionKey"] { inc(s, "duplicate_versions"); return Ok(()); }
    s["seen"][&id] = b["versionKey"].clone();
    let mut points: Vec<V> = s["histories"][&id].as_array().cloned().unwrap_or_default();
    let signal = bid_anchor_signal(&points, &b, &p)?;
    let post = strict_instant(&b["requestStartedAt"])? > strict_instant(&bank["cutoff"])?;
    if post {
        inc(s, signal["reason"].as_str().unwrap_or(""));
        if js_truthy(signal.get("admitted")) {
            let mut sig = json!({"marketId":id,"at":row["at"],"sourceSequence":row["sequence"],
                "sourceHash":row["hash"],"version":b["versionKey"]});
            for (k, v) in signal.as_object().unwrap() { sig[k.clone()] = v.clone(); }
            s["signals"].as_array_mut().map(|a| a.push(sig));
        }
    }
    let wallet_keys: Vec<String> = s["wallets"].as_object().map(|w| w.keys().cloned().collect()).unwrap_or_default();
    for latency in wallet_keys {
        let mut c = bank["config"].clone();
        c["latencyMs"] = latency.parse::<i64>().map(|v| json!(v)).unwrap_or(V::Null);
        if !post { continue; }
        let st = s["wallets"][&latency]["state"]["strategies"]["mean_reversion"].clone();
        let instrument = b["instrument"].as_str().unwrap_or("");
        let was_held = !st["positions"][instrument].is_null();
        let spent: BigInt = st["fills"].as_array().map(|f| f.iter()
            .filter(|f| f["action"].as_str() == Some("buy"))
            .map(|f| big_amount(f["price"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0))
                * BigInt::from(f["quantity"].as_i64().unwrap_or(0)))
            .sum()).unwrap_or_else(|| BigInt::from(0));
        let reserved: BigInt = st["pending"].as_object().map(|o| o.values()
            .filter(|i| i["action"].as_str() == Some("buy"))
            .map(|i| big_amount(i["limit"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0))
                * BigInt::from(i["quantity"].as_i64().unwrap_or(0)))
            .sum()).unwrap_or_else(|| BigInt::from(0));
        let mut occupied: std::collections::BTreeSet<String> = st["positions"].as_object()
            .map(|o| o.keys().cloned().collect()).unwrap_or_default();
        occupied.extend(st["pending"].as_object().map(|o| o.keys().cloned().collect::<Vec<String>>()).unwrap_or_default());
        // Keep a prepared order's original causal anchor during its short lifetime.
        // Its own trough must not retroactively change the reference used to prepare it.
        let pending = st["pending"][instrument].clone();
        let reference: Option<String> = if pending["action"].as_str() == Some("buy") {
            pending["anchorReference"].as_str().map(|x| x.to_string())
        } else {
            signal["modeledReference"].as_str().map(|x| x.to_string())
        };
        let anchored_pending = pending["action"].as_str() == Some("buy") && reference.is_some()
            && big_amount(reference.as_deref().unwrap())? + BigInt::from(TICK)
                - big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?
                <= big_amount(p["maximumBidDrop"].as_str().unwrap_or(""))?
            && big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))?
                - big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?
                <= big_amount(p["maximumSpread"].as_str().unwrap_or(""))?;
        let cooldown_ok = instant(row["at"].as_str().unwrap_or(""))?
            >= s["wallets"][&latency]["cooldowns"][&id].as_i64().unwrap_or(0);
        let entry_cost = if pending["action"].as_str() == Some("buy") { BigInt::from(0) }
            else { big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))? + BigInt::from(TICK) };
        let allow = (js_truthy(signal.get("admitted")) || anchored_pending)
            && cooldown_ok
            && &spent + &reserved + &entry_cost <= big_amount(p["cumulativeBuyBudget"].as_str().unwrap_or(""))?
            && (occupied.contains(instrument) || occupied.len() < p["maxOpenMarkets"].as_i64().unwrap_or(0) as usize);
        let reference_f64 = match reference.as_deref() {
            Some(r) => Some(big_amount(r)?.to_string().parse::<f64>().unwrap_or(0.0) / 1e6),
            None => None,
        };
        let old_intents = st["intents"].as_i64().unwrap_or(0);
        s["wallets"][&latency]["state"] = step_dip(&s["wallets"][&latency]["state"], &b, &c,
            reference_f64, allow, 0.01 - 4.0 * f64::EPSILON, &json!("0.010000"),
            p["horizonMs"].as_i64().unwrap_or(0), &json!("1.000000"), "responsive_v2")?;
        let next_intents = s["wallets"][&latency]["state"]["strategies"]["mean_reversion"]["intents"].as_i64().unwrap_or(0);
        if next_intents > old_intents && !s["wallets"][&latency]["state"]["strategies"]["mean_reversion"]["pending"][instrument].is_null() {
            s["wallets"][&latency]["state"]["strategies"]["mean_reversion"]["pending"][instrument]["createdAt"] = row["at"].clone();
            if s["wallets"][&latency]["state"]["strategies"]["mean_reversion"]["pending"][instrument]["action"].as_str() == Some("buy") {
                match &reference {
                    Some(r) => { s["wallets"][&latency]["state"]["strategies"]["mean_reversion"]["pending"][instrument]["anchorReference"] = json!(r); }
                    None => { s["wallets"][&latency]["state"]["strategies"]["mean_reversion"]["pending"][instrument]
                        .as_object_mut().map(|o| o.remove("anchorReference")); }
                };
            }
        }
        let exit = s["wallets"][&latency]["state"]["strategies"]["mean_reversion"]["pending"][instrument].clone();
        if exit["action"].as_str() == Some("sell") && exit["exitPolicy"].as_str() == Some("risk_reprice_v2") {
            let depth = one_share_depth(&b["bids"], "sell")?;
            if depth["status"].as_str() == Some("available")
                && big_amount(depth["modeledPrice"].as_str().unwrap_or(""))? < big_amount(exit["limit"].as_str().unwrap_or(""))? {
                let pobj = &mut s["wallets"][&latency]["state"]["strategies"]["mean_reversion"]["pending"][instrument];
                pobj["limit"] = depth["modeledPrice"].clone();
                pobj["createdAt"] = row["at"].clone();
                pobj["depthRepricedAt"] = row["at"].clone();
            }
        }
        let now_positions_empty = s["wallets"][&latency]["state"]["strategies"]["mean_reversion"]["positions"][instrument].is_null();
        if was_held && now_positions_empty {
            s["wallets"][&latency]["cooldowns"][&id] =
                json!(instant(row["at"].as_str().unwrap_or(""))? + p["cooldownMs"].as_i64().unwrap_or(0));
        }
        let next_spent: BigInt = s["wallets"][&latency]["state"]["strategies"]["mean_reversion"]["fills"].as_array()
            .map(|f| f.iter()
                .filter(|f| f["action"].as_str() == Some("buy"))
                .map(|f| big_amount(f["price"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0))
                    * BigInt::from(f["quantity"].as_i64().unwrap_or(0)))
                .sum()).unwrap_or_else(|| BigInt::from(0));
        need(next_spent <= big_amount(p["cumulativeBuyBudget"].as_str().unwrap_or(""))?, "BUDGET")?;
    }
    let minute = strict_instant(&b["receivedAt"])?.div_euclid(60000);
    if !points.iter().any(|x| x["minute"].as_i64() == Some(minute)) {
        points.push(json!({"minute":minute,"at":b["receivedAt"],"bid":b["bids"][0]["price"]}));
    }
    let history_ms = p["historyMs"].as_i64().unwrap_or(0);
    let received = strict_instant(&b["receivedAt"])?;
    let mut filtered: Vec<V> = Vec::new();
    for x in points {
        if strict_instant(&x["at"])? >= received - history_ms { filtered.push(x); }
    }
    s["histories"][&id] = json!(filtered);
    Ok(())
}

/// bidAnchorReport(s, bank, at)
pub fn bid_anchor_report(s: &V, bank: &V, at: &str) -> V {
    let mut wallets = serde_json::Map::new();
    if let Some(ws) = s["wallets"].as_object() {
        for (latency, w) in ws {
            let mut c = bank["config"].clone();
            c["latencyMs"] = latency.parse::<i64>().map(|v| json!(v)).unwrap_or(V::Null);
            wallets.insert(latency.clone(),
                report(&w["state"], &c, at)["strategies"]["mean_reversion"].clone());
        }
    }
    json!({
        "mode": "bid_anchor_causal_capture_replay",
        "bankHash": s["bankHash"], "cutoff": bank["cutoff"], "counts": s["counts"],
        "signals": s["signals"], "wallets": wallets,
        "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false,
        "limitations": [
            "Assumes consumer observes publications promptly; later causal request and fixed1/3second latency are required for fills.",
            "Frozen prospective capture replay is local counterfactual paper accounting, not actual orders.",
            "Prior executable bids are a signal hypothesis, not guaranteed fair value or future bid recovery.",
            "Correlated markets and overlapping latency wallets are not independent evidence."
        ],
    })
}
