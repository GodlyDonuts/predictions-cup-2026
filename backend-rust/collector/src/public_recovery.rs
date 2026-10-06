// live-paper/public-recovery.mjs — public rest recovery paper strategy.
use baal_paper::fixed::{big_amount, big_price, big_sequence, instant, money, TICK};
use baal_service_runtime::contract::instant_ns;
use baal_service_runtime::json_text::{digest, js_truthy};
use num_bigint::BigInt;
use serde_json::{json, Value as V};

fn need(ok: bool, m: &str) -> Result<(), String> {
    if !ok {
        return Err(m.to_string());
    }
    Ok(())
}
fn to_iso(x: f64) -> Result<String, String> {
    if !x.is_finite() || x.abs() > 8.64e15 {
        return Err("Invalid time value".into());
    }
    Ok(baal_service_runtime::contract::ms_to_iso(x))
}
fn inc(o: &mut V, k: &str) {
    let cur = o[k].as_f64().unwrap_or(0.0);
    o[k] = json!(cur + 1.0);
}
fn median(x: &[f64]) -> f64 {
    let mut a = x.to_vec();
    a.sort_by(|p, q| p.partial_cmp(q).unwrap_or(std::cmp::Ordering::Equal));
    let n = a.len();
    if n % 2 == 1 {
        a[(n - 1) / 2]
    } else {
        (a[n / 2 - 1] + a[n / 2]) / 2.0
    }
}
fn obj_key(v: &V) -> String {
    // JS property-key coercion for the marketId-keyed state maps.
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => n.to_string(),
        other => serde_json::to_string(other).unwrap_or_default().trim_matches('"').to_string(),
    }
}
fn js_bigint_of_f64(x: f64) -> Result<BigInt, String> {
    if !x.is_finite() || x.fract() != 0.0 {
        return Err(format!("Cannot convert {} to a BigInt", x));
    }
    Ok(BigInt::from(x as i64))
}
fn amount(v: &V) -> Result<BigInt, String> {
    big_amount(v.as_str().unwrap_or(""))
}
fn ins(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn ins_ns(v: &V) -> Result<BigInt, String> {
    instant_ns(v.as_str().unwrap_or(""))
        .map(BigInt::from)
        .map_err(|e| e.to_string())
}
fn dec(v: &V) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

/// PUBLIC_CONFIRMATION_POLICY — {...RECOVERY_CONFIRMATION_POLICY,windowMs:600000,maximumGapMs:180000}
/// (public-recovery-confirmation.mjs already carries those values, so it is identical).
pub fn public_confirmation_policy() -> V {
    crate::public_recovery_confirmation::recovery_confirmation_policy()
}
/// PUBLIC_RECOVERY_VARIANTS
pub fn public_recovery_variants() -> V {
    let mut out = Vec::new();
    for entry in ["baseline", "confirmed", "pricecap"] {
        for (name, take_profit, stop_loss) in [
            ("legacy", "0.010000", "0.030000"),
            ("balanced", "0.020000", "0.020000"),
            ("reward", "0.030000", "0.020000"),
        ] {
            out.push(json!({"id": format!("{}_{}", entry, name), "entry": entry,
                "payoff": {"name": name, "takeProfit": take_profit, "stopLoss": stop_loss}}));
        }
    }
    json!(out)
}
/// publicRecoveryHash — sha256(JSON.stringify(x)) insertion order.
pub fn public_recovery_hash(x: &V) -> String {
    digest(x)
}
/// PUBLIC_RECOVERY_POLICY
pub fn public_recovery_policy() -> V {
    json!({"feeBps":0,"historyMs":21600000,"minimumMinutes":8,"minimumSpanMs":1800000,
        "maximumHistoryAgeMs":2700000,"minimumDip":0.02,"minimumRelativeDip":0.03,"maximumDip":0.1,"minimumZ":1.5,
        "maximumSpread":0.02,"minimumPrice":0.05,"maximumPrice":0.95,"entryLifetimeMs":120000,"maximumQuoteAgeMs":20000,
        "takeProfit":"0.010000","stopLoss":"0.030000","maximumHoldMs":3600000,"cooldownMs":900000,
        "initialCash":"100.000000","maxMarketCost":"25.000000","maxExposure":"100.000000","cumulativeBuyBudget":"100.000000",
        "maxOpenMarkets":3,"orderShares":10,"depthFractionBps":100,"slippageTicks":1,"latenciesMs":[1000,3000]})
}
fn safe_int(v: &V) -> Option<f64> {
    v.as_f64().filter(|f| f.fract() == 0.0 && f.abs() <= 9_007_199_254_740_991.0)
}
fn hex64(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn num_id(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && !s.starts_with('0')
    })
}
fn check_bank(b: &V) -> Result<(), String> {
    need(
        js_truthy(Some(b))
            && b["schemaVersion"].as_i64() == Some(1)
            && b["mode"].as_str() == Some("public_rest_recovery_paper")
            && hex64(&b["sourceScopeHash"]),
        "PUBLIC_RECOVERY_BANK",
    )?;
    ins(&b["cutoff"])?;
    need(
        b["tournamentId"].as_str() == Some("bda92870-621e-47b0-bc3c-3602c5c26f55"),
        "PUBLIC_RECOVERY_TOURNAMENT",
    )?;
    need(
        safe_int(&b["startEventId"]).is_some_and(|x| x >= 0.0),
        "PUBLIC_RECOVERY_START_EVENT",
    )?;
    need(
        public_recovery_hash(&b["policy"]) == public_recovery_hash(&public_recovery_policy()),
        "PUBLIC_RECOVERY_POLICY_CHANGED",
    )?;
    need(
        public_recovery_hash(&b["variants"]) == public_recovery_hash(&public_recovery_variants()),
        "PUBLIC_RECOVERY_VARIANTS_CHANGED",
    )?;
    need(
        public_recovery_hash(&b["confirmationPolicy"]) == public_recovery_hash(&public_confirmation_policy()),
        "PUBLIC_RECOVERY_CONFIRMATION_CHANGED",
    )?;
    let markets = b["markets"].as_array().cloned().unwrap_or_default();
    // JS `new Set(...)` dedups with SameValueZero — the js_stringify text is a
    // faithful identity key (numbers vs strings stay distinct).
    let unique = |vals: Vec<V>| -> usize {
        vals.iter()
            .map(|v| baal_service_runtime::json_text::js_stringify(v))
            .collect::<std::collections::HashSet<_>>()
            .len()
    };
    need(
        b["markets"].is_array()
            && !markets.is_empty()
            && markets.len() <= 256
            && unique(markets.iter().map(|x| x["marketId"].clone()).collect()) == markets.len()
            && unique(markets.iter().map(|x| x["exchangeId"].clone()).collect()) == markets.len(),
        "PUBLIC_RECOVERY_SCOPE",
    )?;
    for m in &markets {
        need(
            num_id(&m["marketId"]) && num_id(&m["exchangeId"]),
            "PUBLIC_RECOVERY_ID",
        )?;
    }
    Ok(())
}

/// createPublicRecovery(bank)
pub fn create_public_recovery(bank: &V) -> Result<V, String> {
    check_bank(bank)?;
    let mut wallets = serde_json::Map::new();
    for variant in bank["variants"].as_array().cloned().unwrap_or_default() {
        for latency in bank["policy"]["latenciesMs"].as_array().cloned().unwrap_or_default() {
            let key = format!(
                "{}_{}",
                variant["id"].as_str().unwrap_or(""),
                latency.as_f64().map(|x| crate::probability_receipt_host_v2::fmt_js_num(x)).unwrap_or_default()
            );
            wallets.insert(
                key,
                json!({"variant": variant, "latencyMs": latency, "cash": bank["policy"]["initialCash"],
                    "realizedPnl": "0.000000", "cumulativeBuys": "0.000000", "positions": {}, "pending": {},
                    "outcomes": [], "fills": [], "roundTrips": [], "gateCounts": {}, "signals": {},
                    "cooldowns": {}, "closedTrades": 0, "nextOrder": 1}),
            );
        }
    }
    Ok(json!({
        "schemaVersion": 1,
        "bankHash": public_recovery_hash(bank),
        "lastEventId": bank["startEventId"],
        "lastAvailableAt": V::Null,
        "histories": {},
        "recoveryPoints": {},
        "seen": {},
        "latest": {},
        "invalid": {},
        "warmBooks": 0,
        "observations": 0,
        "wallets": V::Object(wallets),
    }))
}
fn find_market<'a>(bank: &'a V, market_id: &V) -> Option<&'a V> {
    bank["markets"]
        .as_array()?
        .iter()
        .find(|x| &x["marketId"] == market_id)
}
fn checked_book<'a>(e: &'a V, bank: &'a V) -> Result<&'a V, String> {
    let b = &e["book"];
    let m = find_market(bank, &e["marketId"]);
    need(
        m.is_some()
            && &e["tournamentId"] == &bank["tournamentId"]
            && &e["exchangeId"] == &m.unwrap()["exchangeId"]
            && &b["marketId"] == &m.unwrap()["marketId"]
            && &b["exchangeId"] == &m.unwrap()["exchangeId"]
            && b["instrument"].as_str()
                == Some(format!("{}:YES", m.unwrap()["exchangeId"].as_str().unwrap_or("")).as_str()),
        "PUBLIC_RECOVERY_BOOK_SCOPE",
    )?;
    big_sequence(b["sequence"].as_str().unwrap_or(""))?;
    ins_ns(&b["sourceAt"])?;
    let seq_str = match &b["sequence"] {
        V::String(s) => s.clone(),
        V::Number(n) => n.to_string(),
        _ => String::new(),
    };
    let src_str = match &b["sourceAt"] {
        V::String(s) => s.clone(),
        V::Number(n) => n.to_string(),
        _ => String::new(),
    };
    need(
        b["versionKey"].as_str() == Some(format!("{}:{}", seq_str, src_str).as_str()),
        "PUBLIC_RECOVERY_VERSION",
    )?;
    for side in ["bids", "asks"] {
        let levels = b[side].as_array();
        need(
            levels.is_some() && !levels.unwrap().is_empty(),
            "PUBLIC_RECOVERY_DEPTH",
        )?;
        let mut previous: Option<BigInt> = None;
        for l in levels.unwrap() {
            let p = big_price(l["price"].as_str().unwrap_or(""))?;
            need(
                safe_int(&l["quantity"]).is_some_and(|q| q > 0.0),
                "PUBLIC_RECOVERY_QUANTITY",
            )?;
            if let Some(prev) = &previous {
                need(
                    if side == "bids" { &p < prev } else { &p > prev },
                    "PUBLIC_RECOVERY_DEPTH_ORDER",
                )?;
            }
            previous = Some(p);
        }
    }
    need(
        amount(&b["bids"][0]["price"])? < amount(&b["asks"][0]["price"])?,
        "PUBLIC_RECOVERY_CROSSED",
    )?;
    need(
        e["origin"].as_str() == Some("rest_observation"),
        "PUBLIC_RECOVERY_ORIGIN",
    )?;
    need(
        e["requestStartedAt"].is_string(),
        "PUBLIC_RECOVERY_REQUEST_CLOCK",
    )?;
    let received = ins(&e["quoteReceivedAt"])?;
    let confirmed = ins(&e["confirmedAt"])?;
    let available = ins(&e["availableAt"])?;
    need(
        received <= confirmed + 1000 && confirmed <= available,
        "PUBLIC_RECOVERY_CLOCK",
    )?;
    if js_truthy(Some(&e["requestStartedAt"])) {
        need(
            ins_ns(&e["requestStartedAt"])? <= ins_ns(&e["quoteReceivedAt"])?,
            "PUBLIC_RECOVERY_REQUEST_CLOCK",
        )?;
    }
    if js_truthy(Some(&e["nextExpiryAt"])) {
        ins_ns(&e["nextExpiryAt"])?;
    }
    Ok(b)
}
fn append_history(s: &mut V, e: &V, p: &V) -> Result<(), String> {
    let b = &e["book"];
    let id = obj_key(&e["marketId"]);
    let minute = (ins(&e["quoteReceivedAt"])? as f64 / 60000.0).floor() as i64;
    let mut a = s["histories"][&id].as_array().cloned().unwrap_or_default();
    if !a.iter().any(|x| x["minute"].as_i64() == Some(minute)) {
        a.push(json!({"minute": minute, "receivedAt": e["quoteReceivedAt"], "confirmedAt": e["confirmedAt"],
            "sourceAt": b["sourceAt"],
            "mid": (bigint_to_f64(&amount(&b["bids"][0]["price"])?) + bigint_to_f64(&amount(&b["asks"][0]["price"])?)) / 2e6,
            "versionKey": b["versionKey"]}));
    }
    a.sort_by(|x, y| x["minute"].as_i64().cmp(&y["minute"].as_i64()));
    let newest = a.last().map(|x| x["minute"].as_i64().unwrap_or(0)).unwrap_or(0) * 60000;
    let cutoff = newest as f64 - dec(&p["historyMs"]);
    a.retain(|x| x["minute"].as_i64().unwrap_or(0) as f64 * 60000.0 >= cutoff);
    let a: Vec<V> = a.into_iter().rev().take(361).collect::<Vec<_>>().into_iter().rev().collect();
    s["histories"][&id] = json!(a);
    Ok(())
}
fn bigint_to_f64(x: &BigInt) -> f64 {
    // JS Number(bigint) rounds to nearest double — decimal parse does the same.
    x.to_string().parse::<f64>().unwrap_or(f64::NAN)
}
/// publicRecoveryReference(points, at, p)
pub fn public_recovery_reference(points: Option<&[V]>, at: &str, p: &V) -> Result<V, String> {
    let now = instant(at)?;
    let now_ns = BigInt::from(now) * BigInt::from(1_000_000u64);
    let mut a = Vec::new();
    for x in points.unwrap_or(&[]) {
        if ins(&x["receivedAt"])? < now
            && ins(&x["confirmedAt"])? <= now
            && ins_ns(&x["sourceAt"])? < now_ns
            && x["minute"].as_i64().unwrap_or(0) as f64 * 60000.0 >= now as f64 - dec(&p["historyMs"])
        {
            a.push(x.clone());
        }
    }
    if (a.len() as f64) < dec(&p["minimumMinutes"]) {
        return Ok(json!({"ready": false, "reason": "insufficient_minutes", "samples": a.len()}));
    }
    let first = ins(&a[0]["receivedAt"])?;
    let last = ins(&a[a.len() - 1]["receivedAt"])?;
    if ((last - first) as f64) < dec(&p["minimumSpanMs"]) {
        return Ok(json!({"ready": false, "reason": "insufficient_history_span", "samples": a.len()}));
    }
    if (now - last) as f64 > dec(&p["maximumHistoryAgeMs"]) {
        return Ok(json!({"ready": false, "reason": "historical_reference_stale", "samples": a.len()}));
    }
    let center = median(&a.iter().map(|x| dec(&x["mid"])).collect::<Vec<_>>());
    let mad = median(
        &a.iter()
            .map(|x| (dec(&x["mid"]) - center).abs())
            .collect::<Vec<_>>(),
    );
    Ok(json!({"ready": true, "source": "minute_balanced_prior_verified_book_midpoints",
        "samples": a.len(), "center": center,
        "robustDeviation": (mad * 1.4826).max(0.0025)}))
}
/// warmPublicRecovery(input, books, bank)
pub fn warm_public_recovery(input: &V, books: &[V], bank: &V) -> Result<V, String> {
    check_bank(bank)?;
    need(
        input["bankHash"].as_str() == Some(public_recovery_hash(bank).as_str())
            && input["lastEventId"] == bank["startEventId"]
            && !js_truthy(input.get("lastAvailableAt")),
        "PUBLIC_RECOVERY_WARM_AFTER_START",
    )?;
    let mut s = input.clone();
    for e in books {
        checked_book(e, bank)?;
        need(
            ins(&e["confirmedAt"])? <= ins(&bank["cutoff"])?
                && ins(&e["quoteReceivedAt"])? <= ins(&bank["cutoff"])?,
            "PUBLIC_RECOVERY_FUTURE_WARM",
        )?;
        append_history(&mut s, e, &bank["policy"])?;
        let id = obj_key(&e["marketId"]);
        let points = s["recoveryPoints"][&id].as_array().cloned().unwrap_or_default();
        s["recoveryPoints"][&id] = json!(crate::public_recovery_confirmation::observe_recovery(
            &points,
            e,
            &bank["confirmationPolicy"],
        )?);
        let warm = s["warmBooks"].as_f64().unwrap_or(0.0);
        s["warmBooks"] = json!(warm + 1.0);
    }
    Ok(s)
}
fn resolve(w: &mut V, id: &str, at: &V, status: &str) {
    let order = w["pending"][id].clone();
    if order.is_null() {
        return;
    }
    if let Some(o) = w["outcomes"]
        .as_array_mut()
        .and_then(|out| out.iter_mut().find(|x| x["id"] == order["id"]))
    {
        o["status"] = json!(status);
        o["resolvedAt"] = at.clone();
        o["unfilledQuantity"] = order["remaining"].clone();
        if status == "expired" {
            o["censoring"] = json!(if o["independentReceipts"].as_f64().unwrap_or(0.0) == 0.0 {
                "no_independent_in_window_quote"
            } else {
                "observed_without_full_execution"
            });
        }
    }
    if let Some(p) = w["pending"].as_object_mut() {
        p.shift_remove(id);
    }
}
fn assert_wallet(w: &V, p: &V) -> Result<(), String> {
    let mut cost = BigInt::from(0);
    for pos in w["positions"].as_object().map(|o| o.values()).into_iter().flatten() {
        need(
            safe_int(&pos["quantity"]).is_some_and(|q| q > 0.0) && amount(&pos["cost"])? > BigInt::from(0),
            "PUBLIC_RECOVERY_POSITION",
        )?;
        cost += amount(&pos["cost"])?;
    }
    need(
        amount(&w["cash"])? >= BigInt::from(0)
            && amount(&w["cash"])? + &cost - amount(&p["initialCash"])? == amount(&w["realizedPnl"])?,
        "PUBLIC_RECOVERY_ACCOUNTING",
    )?;
    need(
        cost <= amount(&p["maxExposure"])?
            && amount(&w["cumulativeBuys"])? <= amount(&p["cumulativeBuyBudget"])?,
        "PUBLIC_RECOVERY_BUDGET",
    )?;
    Ok(())
}
fn fill_pending(w: &mut V, e: &V, p: &V) -> Result<(), String> {
    let id = obj_key(&e["marketId"]);
    if w["pending"][&id].is_null() {
        return Ok(());
    }
    let b = e["book"].clone();
    let receipt = ins(&e["quoteReceivedAt"])?;
    let confirmed = ins(&e["confirmedAt"])?;
    let o_seq = big_sequence(w["pending"][&id]["sequence"].as_str().unwrap_or(""))?;
    let o_src = ins_ns(&w["pending"][&id]["sourceAt"])?;
    let independent =
        big_sequence(b["sequence"].as_str().unwrap_or(""))? > o_seq && ins_ns(&b["sourceAt"])? > o_src;
    let o_at = ins(&w["pending"][&id]["at"])?;
    let o_exp = ins(&w["pending"][&id]["expiresAt"])?;
    let latency = dec(&w["latencyMs"]) as i64;
    let expiry_gate = js_truthy(Some(&e["nextExpiryAt"]))
        && ins_ns(&e["nextExpiryAt"])? <= ins_ns(&e["confirmedAt"])?;
    if !independent
        || receipt < o_at + latency
        || receipt >= o_exp
        || confirmed >= o_exp
        || !e["continuity"]["ready"].as_bool().unwrap_or(false)
        || !e["continuity"]["continuous"].as_bool().unwrap_or(false)
        || expiry_gate
    {
        return Ok(());
    }
    {
        let order_id = w["pending"][&id]["id"].clone();
        let outcomes = w["outcomes"].as_array_mut().unwrap();
        if let Some(outcome) = outcomes.iter_mut().find(|x| x["id"] == order_id) {
            let n = outcome["independentReceipts"].as_f64().unwrap_or(0.0);
            outcome["independentReceipts"] = json!(n + 1.0);
        }
    }
    let o_side = w["pending"][&id]["side"].as_str().unwrap_or("").to_string();
    let levels = if o_side == "buy" {
        b["asks"].as_array().cloned().unwrap_or_default()
    } else {
        b["bids"].as_array().cloned().unwrap_or_default()
    };
    let limit = amount(&w["pending"][&id]["limit"])?;
    let mut remaining = dec(&w["pending"][&id]["remaining"]);
    let mut resolve_at: Option<V> = None;
    for l in &levels {
        let raw = amount(&l["price"])?;
        if (o_side == "buy" && raw > limit) || (o_side != "buy" && raw < limit) {
            break;
        }
        let depth = (dec(&l["quantity"]) * dec(&p["depthFractionBps"]) / 10000.0).floor();
        let qty = remaining.min(depth);
        if qty == 0.0 {
            continue;
        }
        // This is a counterfactual executable opportunity, not queue priority or a real fill.
        let tick = BigInt::from(TICK);
        let execution = if o_side == "buy" {
            if &raw + &tick < limit { &raw + &tick } else { limit.clone() }
        } else {
            if &raw - &tick > limit { &raw - &tick } else { limit.clone() }
        };
        let qty_big = js_bigint_of_f64(qty)?;
        let notional = &execution * &qty_big;
        let mut gain = BigInt::from(0);
        if o_side == "buy" {
            need(amount(&w["cash"])? >= notional, "PUBLIC_RECOVERY_CASH")?;
            let mut pos = w["positions"][&id].clone();
            if pos.is_null() {
                pos = json!({"quantity": 0, "cost": "0.000000", "openedAt": e["confirmedAt"],
                    "realizedPnl": "0.000000", "enteredQuantity": 0, "entryCost": "0.000000"});
            }
            let en = dec(&pos["enteredQuantity"]) + qty;
            pos["enteredQuantity"] = json!(en);
            pos["entryCost"] = json!(money(&(amount(&pos["entryCost"])? + &notional)));
            let qn = dec(&pos["quantity"]) + qty;
            pos["quantity"] = json!(qn);
            pos["cost"] = json!(money(&(amount(&pos["cost"])? + &notional)));
            w["positions"][&id] = pos;
            w["cash"] = json!(money(&(amount(&w["cash"])? - &notional)));
            w["cumulativeBuys"] = json!(money(&(amount(&w["cumulativeBuys"])? + &notional)));
        } else {
            let pos_exists = !w["positions"][&id].is_null();
            need(
                pos_exists && dec(&w["positions"][&id]["quantity"]) >= qty,
                "PUBLIC_RECOVERY_OVERSELL",
            )?;
            let pos_qty = dec(&w["positions"][&id]["quantity"]);
            let released = if qty == pos_qty {
                amount(&w["positions"][&id]["cost"])?
            } else {
                amount(&w["positions"][&id]["cost"])? * &qty_big / js_bigint_of_f64(pos_qty)?
            };
            gain = &notional - &released;
            let new_qty = pos_qty - qty;
            w["positions"][&id]["quantity"] = json!(new_qty);
            w["positions"][&id]["cost"] = json!(money(&(amount(&w["positions"][&id]["cost"])? - &released)));
            w["positions"][&id]["realizedPnl"] =
                json!(money(&(amount(&w["positions"][&id]["realizedPnl"])? + &gain)));
            w["cash"] = json!(money(&(amount(&w["cash"])? + &notional)));
            w["realizedPnl"] = json!(money(&(amount(&w["realizedPnl"])? + &gain)));
            if new_qty == 0.0 {
                let pos = w["positions"][&id].clone();
                w["roundTrips"].as_array_mut().unwrap().push(json!({
                    "marketId": e["marketId"], "openedAt": pos["openedAt"], "closedAt": e["confirmedAt"],
                    "quantity": pos["enteredQuantity"], "entryCost": pos["entryCost"],
                    "realizedPnl": pos["realizedPnl"]}));
                if let Some(m) = w["positions"].as_object_mut() {
                    m.shift_remove(&id);
                }
                let ct = dec(&w["closedTrades"]);
                w["closedTrades"] = json!(ct + 1.0);
                w["cooldowns"][&id] = json!(to_iso(confirmed as f64 + dec(&p["cooldownMs"]))?);
            }
        }
        let o = w["pending"][&id].clone();
        w["fills"].as_array_mut().unwrap().push(json!({
            "orderId": o["id"], "marketId": e["marketId"], "side": o["side"], "quantity": qty,
            "price": money(&execution), "realizedPnl": money(&gain), "fee": "0.000000",
            "sourceVersion": b["versionKey"], "quoteReceivedAt": e["quoteReceivedAt"],
            "confirmedAt": e["confirmedAt"], "gatewayAvailableAt": e["availableAt"],
            "evidence": "counterfactual_executable_opportunity"}));
        remaining -= qty;
        w["pending"][&id]["remaining"] = json!(remaining);
        {
            let order_id = o["id"].clone();
            if let Some(outcome) = w["outcomes"]
                .as_array_mut()
                .and_then(|out| out.iter_mut().find(|x| x["id"] == order_id))
            {
                let fq = outcome["filledQuantity"].as_f64().unwrap_or(0.0);
                outcome["filledQuantity"] = json!(fq + qty);
            }
        }
        if remaining == 0.0 {
            break;
        }
    }
    // One accepted version consumes depth once, including partial fills.
    w["pending"][&id]["sequence"] = b["sequence"].clone();
    w["pending"][&id]["sourceAt"] = b["sourceAt"].clone();
    if remaining == 0.0 {
        resolve_at = Some(e["confirmedAt"].clone());
    }
    if let Some(at) = resolve_at {
        resolve(w, &id, &at, "filled");
    }
    assert_wallet(w, p)
}
fn fresh(s: &V, e: &V, now: i64, p: &V) -> bool {
    if !js_truthy(Some(&e["decisionEligible"]))
        || js_truthy(s["invalid"].get(obj_key(&e["marketId"]).as_str()))
        || !e["continuity"]["ready"].as_bool().unwrap_or(false)
        || !e["continuity"]["continuous"].as_bool().unwrap_or(false)
    {
        return false;
    }
    let q = match ins(&e["quoteReceivedAt"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if now - q < 0 || (now - q) as f64 >= dec(&p["maximumQuoteAgeMs"]) {
        return false;
    }
    if js_truthy(Some(&e["nextExpiryAt"])) {
        match ins_ns(&e["nextExpiryAt"]) {
            Ok(v) if v > BigInt::from(now) * BigInt::from(1_000_000u64) => {}
            _ => return false,
        }
    }
    true
}
fn submit(w: &mut V, e: &V, at: &str, side: &str, quantity: f64, limit: &V, p: &V, reason: &str) -> Result<(), String> {
    let id = obj_key(&e["marketId"]);
    let next = dec(&w["nextOrder"]);
    w["nextOrder"] = json!(next + 1.0);
    let order_id = format!(
        "recovery-{}-{}-{}",
        w["variant"]["id"].as_str().unwrap_or(""),
        crate::probability_receipt_host_v2::fmt_js_num(dec(&w["latencyMs"])),
        crate::probability_receipt_host_v2::fmt_js_num(next)
    );
    let o = json!({"id": order_id, "side": side, "quantity": quantity, "remaining": quantity,
        "limit": limit, "at": at, "expiresAt": to_iso(instant(at)? as f64 + dec(&p["entryLifetimeMs"]))?,
        "sequence": e["book"]["sequence"], "sourceAt": e["book"]["sourceAt"], "reason": reason});
    w["pending"][&id] = o.clone();
    // JS {...o,marketId,status,...}: o's own keys first, then appended keys.
    let mut ordered = serde_json::Map::new();
    for (k, v) in o.as_object().unwrap() {
        ordered.insert(k.clone(), v.clone());
    }
    ordered.insert("marketId".to_string(), e["marketId"].clone());
    ordered.insert("status".to_string(), json!("pending"));
    ordered.insert("filledQuantity".to_string(), json!(0));
    ordered.insert("independentReceipts".to_string(), json!(0));
    w["outcomes"].as_array_mut().unwrap().push(V::Object(ordered));
    Ok(())
}
fn decide(s: &V, w: &mut V, e: &V, at: &str, bank: &V) -> Result<(), String> {
    let id = obj_key(&e["marketId"]);
    let mut p = bank["policy"].as_object().cloned().unwrap_or_default();
    for (k, v) in w["variant"]["payoff"].as_object().cloned().unwrap_or_default() {
        p.insert(k, v);
    }
    let p = V::Object(p);
    let b = e["book"].clone();
    let now = instant(at)?;
    if !fresh(s, e, now, &p) {
        inc(&mut w["gateCounts"], "stale_or_invalid");
        return Ok(());
    }
    if js_truthy(w["pending"].get(id.as_str())) {
        return Ok(());
    }
    let bid = amount(&b["bids"][0]["price"])?;
    let ask = amount(&b["asks"][0]["price"])?;
    let pos_present = !w["positions"][&id].is_null();
    if pos_present {
        let pos = w["positions"][&id].clone();
        let per = amount(&pos["cost"])? / js_bigint_of_f64(dec(&pos["quantity"]))?;
        let exit = &bid - BigInt::from(TICK);
        if &exit - &per >= amount(&p["takeProfit"])?
            || &per - &exit >= amount(&p["stopLoss"])?
            || (now - ins(&pos["openedAt"])?) as f64 >= dec(&p["maximumHoldMs"])
        {
            let lim = if exit > BigInt::from(0) { exit } else { BigInt::from(TICK) };
            submit(w, e, at, "sell", dec(&pos["quantity"]), &json!(money(&lim)), &p, "profit_loss_or_hold_exit")?;
            return Ok(());
        }
        return Ok(());
    }
    let points = s["histories"][&id].as_array().cloned().unwrap_or_default();
    let reference = public_recovery_reference(Some(&points), at, &p)?;
    {
        let mut sig = reference.as_object().cloned().unwrap_or_default();
        sig.insert("at".to_string(), json!(at));
        sig.insert("rawAsk".to_string(), json!(money(&ask)));
        sig.insert("rawBid".to_string(), json!(money(&bid)));
        sig.insert("paddedAsk".to_string(), json!(money(&(&ask + BigInt::from(TICK)))));
        sig.insert("paddedBid".to_string(), json!(money(&(&bid - BigInt::from(TICK)))));
        sig.insert("rawSpread".to_string(), json!(bigint_to_f64(&(&ask - &bid)) / 1e6));
        sig.insert(
            "paddedSpread".to_string(),
            json!(bigint_to_f64(&(&ask - &bid + BigInt::from(2 * TICK))) / 1e6),
        );
        if reference["ready"].as_bool().unwrap_or(false) {
            sig.insert(
                "paddedDiscount".to_string(),
                json!(dec(&reference["center"]) - bigint_to_f64(&(&ask + BigInt::from(TICK))) / 1e6),
            );
            sig.insert(
                "conditionalDiscount".to_string(),
                json!(dec(&reference["center"]) - bigint_to_f64(&ask) / 1e6),
            );
        }
        w["signals"][&id] = V::Object(sig);
    }
    if !reference["ready"].as_bool().unwrap_or(false) {
        let reason = reference["reason"].as_str().unwrap_or("").to_string();
        inc(&mut w["gateCounts"], &reason);
        return Ok(());
    }
    let cooldown = w["cooldowns"][&id].as_str().unwrap_or("");
    if baal_service_runtime::contract::parse_iso_ms(cooldown).unwrap_or(f64::NAN) > now as f64 {
        inc(&mut w["gateCounts"], "cooldown");
        return Ok(());
    }
    let mut occupied: Vec<String> = w["positions"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    for k in w["pending"].as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() {
        if !occupied.contains(&k) {
            occupied.push(k);
        }
    }
    if occupied.len() as f64 >= dec(&p["maxOpenMarkets"]) {
        inc(&mut w["gateCounts"], "market_count");
        return Ok(());
    }
    let identity = find_market(bank, &e["marketId"]).cloned();
    let others: Vec<Option<V>> = occupied
        .iter()
        .map(|x| find_market(bank, &json!(x)).cloned())
        .collect();
    let identity = match identity {
        Some(v) => v,
        // scope check guarantees membership; mirror the JS TypeError.
        None => return Err("Cannot read properties of undefined (reading 'state')".into()),
    };
    if !js_truthy(identity.get("state")) || !js_truthy(identity.get("office")) {
        // JS others.some(x=>!x.state||!x.office) evaluates in order and throws
        // TypeError the moment it reads .state on an absent market.
        let mut any_unknown = false;
        for x in &others {
            match x {
                Some(v) => {
                    if !js_truthy(v.get("state")) || !js_truthy(v.get("office")) {
                        any_unknown = true;
                        break;
                    }
                }
                None => {
                    return Err("Cannot read properties of undefined (reading 'state')".into());
                }
            }
        }
        if any_unknown {
            inc(&mut w["gateCounts"], "unknown_identity_concentration");
            return Ok(());
        }
    }
    if js_truthy(identity.get("state")) {
        // JS others.filter(x=>x.state===identity.state) — same ordered TypeError.
        let mut same = 0usize;
        for x in &others {
            match x {
                Some(v) => {
                    if crate::probability_receipt_host_v2::js_eq(&v["state"], &identity["state"]) {
                        same += 1;
                    }
                }
                None => {
                    return Err("Cannot read properties of undefined (reading 'state')".into());
                }
            }
        }
        if same >= 2 {
            inc(&mut w["gateCounts"], "state_concentration");
            return Ok(());
        }
    }
    if ins(&e["quoteReceivedAt"])? <= ins(&bank["cutoff"])? {
        inc(&mut w["gateCounts"], "pre_cutoff_quote");
        return Ok(());
    }
    let bid_value = bigint_to_f64(&(&bid - BigInt::from(TICK))) / 1e6;
    let center = dec(&reference["center"]);
    let admitted = |candidate: &BigInt| -> bool {
        let a = bigint_to_f64(candidate) / 1e6;
        let d = center - a;
        a >= dec(&p["minimumPrice"])
            && a <= dec(&p["maximumPrice"])
            && a > bid_value
            && a - bid_value <= dec(&p["maximumSpread"]) + 1e-12
            && d >= dec(&p["minimumDip"]) - 1e-12
            && d <= dec(&p["maximumDip"])
            && d / center.max(0.001) >= dec(&p["minimumRelativeDip"])
            && d / dec(&reference["robustDeviation"]) >= dec(&p["minimumZ"])
    };
    let points = s["recoveryPoints"][&id].as_array().cloned().unwrap_or_default();
    let confirmation = crate::public_recovery_confirmation::recovery_confirmation(
        &points,
        e,
        at,
        &bank["confirmationPolicy"],
    )?;
    w["signals"][&id]["recovery"] = confirmation.clone();
    let entry = w["variant"]["entry"].as_str().unwrap_or("");
    if entry != "baseline" && !confirmation["admitted"].as_bool().unwrap_or(false) {
        // JS key-coerces reason through String(); absent reason -> "undefined".
        let reason = confirmation["reason"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "undefined".to_string());
        inc(&mut w["gateCounts"], &reason);
        return Ok(());
    }
    let tick = BigInt::from(TICK);
    let mut limit: Option<BigInt> = None;
    if entry == "pricecap" {
        if ask > tick && admitted(&ask) {
            limit = Some(&ask - &tick);
        }
    } else if admitted(&(&ask + &tick)) {
        limit = Some(ask.clone());
    } else if entry == "baseline" && ask > tick && admitted(&ask) {
        limit = Some(&ask - &tick);
    }
    if let Some(l) = &limit {
        if entry != "baseline" && l - &bid + &tick >= amount(&p["stopLoss"])? {
            inc(&mut w["gateCounts"], "insufficient_exit_headroom");
            return Ok(());
        }
    }
    let Some(limit) = limit else {
        inc(&mut w["gateCounts"], "discount_spread_or_regime_gate");
        let a = bigint_to_f64(&ask) / 1e6;
        let discount = center - a;
        inc(
            &mut w["gateCounts"],
            if a < dec(&p["minimumPrice"]) || a > dec(&p["maximumPrice"]) {
                "price_band"
            } else if bigint_to_f64(&(&ask - &bid + BigInt::from(TICK))) / 1e6
                > dec(&p["maximumSpread"]) + 1e-12
            {
                "spread"
            } else if discount < dec(&p["minimumDip"]) - 1e-12 {
                "minimum_discount"
            } else if discount > dec(&p["maximumDip"]) {
                "regime_change"
            } else if discount / center.max(0.001) < dec(&p["minimumRelativeDip"]) {
                "relative_discount"
            } else {
                "robust_z"
            },
        );
        return Ok(());
    };
    let reserved: BigInt = w["pending"]
        .as_object()
        .map(|o| o.values().filter(|x| x["side"].as_str() == Some("buy")).cloned().collect::<Vec<_>>())
        .unwrap_or_default()
        .iter()
        .try_fold(BigInt::from(0), |n, x| {
            Ok::<BigInt, String>(n + amount(&x["limit"])? * js_bigint_of_f64(dec(&x["remaining"]))?)
        })?;
    let budget = amount(&p["cumulativeBuyBudget"])? - amount(&w["cumulativeBuys"])? - &reserved;
    let qty = [
        dec(&p["orderShares"]),
        bigint_to_f64(&(amount(&p["maxMarketCost"])? / &limit)),
        bigint_to_f64(&(&budget / &limit)),
        bigint_to_f64(&((amount(&w["cash"])? - &reserved) / &limit)),
    ]
    .iter()
    .cloned()
    .fold(f64::INFINITY, f64::min);
    if qty < 1.0 {
        inc(&mut w["gateCounts"], "budget");
        return Ok(());
    }
    let reason = if limit == ask { "native_dip" } else { "conditional_one_tick_lower" };
    submit(w, e, at, "buy", qty, &json!(money(&limit)), &p, reason)?;
    inc(&mut w["gateCounts"], "admitted");
    Ok(())
}
/// advancePublicRecovery(input, events, bank, {availableAt,evidenceThrough=availableAt,
/// allowDecisions=true}) — allow_decisions is the raw JSON value so that a
/// non-boolean input fails EVIDENCE_CLOCK exactly like `typeof` in JS.
pub fn advance_public_recovery(
    input: &V,
    events: &[V],
    bank: &V,
    available_at: &str,
    evidence_through: Option<&str>,
    allow_decisions: &V,
) -> Result<V, String> {
    check_bank(bank)?;
    need(
        input["bankHash"].as_str() == Some(public_recovery_hash(bank).as_str()),
        "PUBLIC_RECOVERY_BANK_CHANGED",
    )?;
    let evidence_through = evidence_through.unwrap_or(available_at);
    let now = instant(available_at)?;
    let watermark = instant(evidence_through)?;
    need(
        watermark <= now
            && (!js_truthy(input.get("lastEvidenceThrough"))
                || watermark >= ins(input.get("lastEvidenceThrough").unwrap())?)
            && allow_decisions.is_boolean(),
        "PUBLIC_RECOVERY_EVIDENCE_CLOCK",
    )?;
    let allow_decisions = allow_decisions.as_bool().unwrap_or(false);
    need(
        now >= ins(&bank["cutoff"])?
            && (!js_truthy(input.get("lastAvailableAt"))
                || now >= ins(input.get("lastAvailableAt").unwrap())?),
        "PUBLIC_RECOVERY_HOST_CLOCK",
    )?;
    let mut s = input.clone();
    let mut last = indexmap::IndexMap::<String, V>::new();
    for e in events {
        // e.eventId === s.lastEventId+1
        let expected_id = s["lastEventId"].as_f64().unwrap_or(f64::NAN) + 1.0;
        need(
            e["eventId"].as_f64() == Some(expected_id)
                && ins(&e["availableAt"])? <= now
                && ins(&e["confirmedAt"])? <= now,
            "PUBLIC_RECOVERY_EVENT_ORDER",
        )?;
        need(
            &e["tournamentId"] == &bank["tournamentId"]
                && bank["markets"].as_array().is_some_and(|ms| {
                    ms.iter().any(|x| {
                        crate::probability_receipt_host_v2::js_eq(&x["marketId"], &e["marketId"])
                            && crate::probability_receipt_host_v2::js_eq(&x["exchangeId"], &e["exchangeId"])
                    })
                }),
            "PUBLIC_RECOVERY_SCOPE",
        )?;
        s["lastEventId"] = e["eventId"].clone();
        let id = obj_key(&e["marketId"]);
        if e["kind"].as_str() == Some("invalidate") {
            s["invalid"][&id] = json!(true);
            if let Some(m) = s["latest"].as_object_mut() {
                m.shift_remove(&id);
            }
            last.shift_remove(&id);
            let wallets: Vec<String> = s["wallets"]
                .as_object()
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default();
            for key in wallets {
                resolve(&mut s["wallets"][&key], &id, &e["confirmedAt"], "invalidated");
            }
            continue;
        }
        need(e["kind"].as_str() == Some("book"), "PUBLIC_RECOVERY_EVENT_KIND")?;
        let b = checked_book(e, bank)?.clone();
        let old = s["seen"][&id].clone();
        let content_hash = public_recovery_hash(&json!({"bids": b["bids"], "asks": b["asks"]}));
        if !old.is_null() {
            need(
                big_sequence(b["sequence"].as_str().unwrap_or(""))?
                    >= big_sequence(old["sequence"].as_str().unwrap_or(""))?
                    && ins_ns(&b["sourceAt"])? >= ins_ns(&old["sourceAt"])?,
                "PUBLIC_RECOVERY_SOURCE_ROLLBACK",
            )?;
            if &b["sequence"] == &old["sequence"] {
                need(
                    &b["sourceAt"] == &old["sourceAt"] && content_hash == old["contentHash"].as_str().unwrap_or(""),
                    "PUBLIC_RECOVERY_SAME_VERSION_CHANGED",
                )?;
            }
        }
        s["seen"][&id] = json!({"sequence": b["sequence"], "sourceAt": b["sourceAt"], "contentHash": content_hash});
        if let Some(m) = s["invalid"].as_object_mut() {
            m.shift_remove(&id);
        }
        s["latest"][&id] = e.clone();
        if old.is_null() || &b["sequence"] != &old["sequence"] {
            let obs = s["observations"].as_f64().unwrap_or(0.0);
            s["observations"] = json!(obs + 1.0);
        }
        let wallets: Vec<String> = s["wallets"]
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        for key in wallets {
            fill_pending(&mut s["wallets"][&key], e, &bank["policy"])?;
        }
        let points = s["recoveryPoints"][&id].as_array().cloned().unwrap_or_default();
        s["recoveryPoints"][&id] = json!(crate::public_recovery_confirmation::observe_recovery(
            &points,
            e,
            &bank["confirmationPolicy"],
        )?);
        // JS Map.set on an existing key keeps its original position — IndexMap
        // insert does the same.
        last.insert(id, e.clone());
    }
    // Resolve the entire ordered evidence batch before expiring old orders or making
    // new decisions. A delayed consumer cannot erase an adverse pre-expiry fill.
    let wallet_keys: Vec<String> = s["wallets"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    for key in &wallet_keys {
        let w = &mut s["wallets"][key.as_str()];
        let pending_ids: Vec<String> = w["pending"]
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        for pid in pending_ids {
            if watermark >= ins(&w["pending"][pid.as_str()]["expiresAt"])? {
                resolve(w, &pid, &json!(available_at), "expired");
            }
        }
        if allow_decisions {
            for e in last.values() {
                // JS mutates the wallet object inside s; clone-out/write-back is
                // equivalent because decide never reads s.wallets.
                let mut w = s["wallets"][key.as_str()].clone();
                decide(&s, &mut w, e, available_at, bank)?;
                s["wallets"][key.as_str()] = w;
            }
        }
        assert_wallet(&s["wallets"][key.as_str()], &bank["policy"])?;
    }
    for e in last.values() {
        append_history(&mut s, &e.clone(), &bank["policy"])?;
    }
    s["lastAvailableAt"] = json!(available_at);
    s["lastEvidenceThrough"] = json!(evidence_through);
    Ok(s)
}
/// publicRecoveryReport(s, bank, at)
pub fn public_recovery_report(s: &V, bank: &V, at: &str) -> Result<V, String> {
    need(
        s["bankHash"].as_str() == Some(public_recovery_hash(bank).as_str()),
        "PUBLIC_RECOVERY_REPORT_BANK",
    )?;
    let now = instant(at)?;
    let mut wallets = serde_json::Map::new();
    for (name, w) in s["wallets"].as_object().cloned().unwrap_or_default() {
        let mut liquidation = BigInt::from(0);
        let mut complete = true;
        let mut positions = Vec::new();
        for (pid, pos) in w["positions"].as_object().cloned().unwrap_or_default() {
            let e = s["latest"][&pid].clone();
            let ready = !e.is_null() && fresh(s, &e, now, &bank["policy"]);
            let mut mark: Option<V> = None;
            if ready {
                let mut q = dec(&pos["quantity"]);
                let mut value = BigInt::from(0);
                for l in e["book"]["bids"].as_array().cloned().unwrap_or_default() {
                    let n = q.min((dec(&l["quantity"]) * dec(&bank["policy"]["depthFractionBps"]) / 10000.0).floor());
                    let px = amount(&l["price"])?;
                    let unit = if px > BigInt::from(TICK) { px - BigInt::from(TICK) } else { BigInt::from(0) };
                    value += unit * js_bigint_of_f64(n)?;
                    q -= n;
                    if q == 0.0 {
                        break;
                    }
                }
                if q == 0.0 {
                    mark = Some(json!(money(&value)));
                }
            }
            if mark.is_none() {
                complete = false;
            } else {
                liquidation += amount(mark.as_ref().unwrap())?;
            }
            // {marketId:id, ...pos, liquidationValue, markReady, quoteReceivedAt,
            //  quoteInvalidated, observationAgeMs}
            let mut entry = serde_json::Map::new();
            entry.insert("marketId".to_string(), json!(pid));
            for (k, v) in pos.as_object().cloned().unwrap_or_default() {
                entry.insert(k, v);
            }
            entry.insert("liquidationValue".to_string(), mark.clone().unwrap_or(V::Null));
            entry.insert("markReady".to_string(), json!(mark.is_some()));
            entry.insert(
                "quoteReceivedAt".to_string(),
                if e.is_null() { V::Null } else { e["quoteReceivedAt"].clone() },
            );
            entry.insert("quoteInvalidated".to_string(), json!(!ready));
            entry.insert(
                "observationAgeMs".to_string(),
                if e.is_null() { V::Null } else { json!(now - ins(&e["quoteReceivedAt"])?) },
            );
            positions.push(V::Object(entry));
        }
        let round_trips = w["roundTrips"].as_array().cloned().unwrap_or_default();
        let wins: Vec<&V> = round_trips
            .iter()
            .filter(|x| amount(&x["realizedPnl"]).unwrap_or_default() > BigInt::from(0))
            .collect();
        let losses: Vec<&V> = round_trips
            .iter()
            .filter(|x| amount(&x["realizedPnl"]).unwrap_or_default() < BigInt::from(0))
            .collect();
        let gross_wins: BigInt = wins.iter().try_fold(BigInt::from(0), |n, x| {
            Ok::<BigInt, String>(n + amount(&x["realizedPnl"])?)
        })?;
        let gross_losses: BigInt = losses.iter().try_fold(BigInt::from(0), |n, x| {
            Ok::<BigInt, String>(n - amount(&x["realizedPnl"])?)
        })?;
        let pending_arr: Vec<V> = w["pending"]
            .as_object()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|(market_id, o)| {
                let mut m = serde_json::Map::new();
                m.insert("marketId".to_string(), json!(market_id));
                for (k, v) in o.as_object().cloned().unwrap_or_default() {
                    m.insert(k, v);
                }
                V::Object(m)
            })
            .collect();
        let closed_round_trips = json!({
            "count": round_trips.len(),
            "wins": wins.len(),
            "losses": losses.len(),
            "flat": round_trips.len() - wins.len() - losses.len(),
            "winRate": if round_trips.is_empty() { V::Null } else { json!(wins.len() as f64 / round_trips.len() as f64) },
            "grossWins": money(&gross_wins),
            "grossLosses": money(&gross_losses),
            "profitFactor": if gross_losses == BigInt::from(0) { V::Null } else { json!(bigint_to_f64(&gross_wins) / bigint_to_f64(&gross_losses)) },
            "recent": round_trips.iter().rev().take(30).rev().cloned().collect::<Vec<_>>(),
        });
        let stop_loss = amount(&w["variant"]["payoff"]["stopLoss"])?;
        let take_profit = amount(&w["variant"]["payoff"]["takeProfit"])?;
        let total_pnl = if complete {
            json!(money(&(amount(&w["cash"])? + &liquidation - amount(&bank["policy"]["initialCash"])?)))
        } else {
            V::Null
        };
        // JS now>=instant(o.expiresAt) — malformed input propagates, not swallows.
        let mut clock_expired = 0usize;
        for x in w["pending"].as_object().map(|o| o.values().collect::<Vec<_>>()).unwrap_or_default() {
            if now >= ins(&x["expiresAt"])? {
                clock_expired += 1;
            }
        }
        let fills = w["fills"].as_array().cloned().unwrap_or_default();
        let outcomes = w["outcomes"].as_array().cloned().unwrap_or_default();
        wallets.insert(
            name,
            json!({
                "variant": w["variant"], "latencyMs": w["latencyMs"], "cash": w["cash"],
                "realizedPnl": w["realizedPnl"], "cumulativeBuys": w["cumulativeBuys"],
                "closedTrades": w["closedTrades"], "positions": positions, "pending": pending_arr,
                "closedRoundTrips": closed_round_trips,
                "idealizedBreakEvenWinRate": bigint_to_f64(&stop_loss) / bigint_to_f64(&(&stop_loss + &take_profit)),
                "totalPnl": total_pnl, "marksComplete": complete,
                "clockExpiredAwaitingSource": clock_expired,
                "gateCounts": w["gateCounts"], "signals": w["signals"],
                "orders": outcomes.len(), "fills": fills.len(),
                "recentFills": fills.iter().rev().take(30).rev().cloned().collect::<Vec<_>>(),
                "outcomes": outcomes.iter().rev().take(50).rev().cloned().collect::<Vec<_>>(),
            }),
        );
    }
    let usable = bank["markets"]
        .as_array()
        .map(|ms| {
            ms.iter()
                .filter(|m| {
                    public_recovery_reference(
                        s["histories"][obj_key(&m["marketId"]).as_str()].as_array().map(|a| a.as_slice()),
                        at,
                        &bank["policy"],
                    )
                    .map(|r| r["ready"].as_bool().unwrap_or(false))
                    .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(0);
    Ok(json!({
        "mode": bank["mode"],
        "bankHash": s["bankHash"],
        "sourceScopeHash": bank["sourceScopeHash"],
        "updatedAt": at,
        "cutoff": bank["cutoff"],
        "lastEventId": s["lastEventId"],
        "variants": bank["variants"],
        "confirmationPolicy": bank["confirmationPolicy"],
        "configuredMarkets": bank["markets"].as_array().map(|a| a.len()).unwrap_or(0),
        "historyCoverage": {
            "marketsWithBookHistory": s["histories"].as_object().map(|o| o.len()).unwrap_or(0),
            "marketsWithRecoveryContext": s["recoveryPoints"].as_object().map(|o| o.len()).unwrap_or(0),
            "usableHistoricalReferences": usable,
        },
        "warmBooks": s["warmBooks"],
        "observations": s["observations"],
        "networkRequests": 0,
        "competitionOrders": 0,
        "feeBps": 0,
        "profitabilityEstablished": false,
        "wallets": V::Object(wallets),
    }))
}
/// streamDipReferenceForReport(points, at, p)
pub fn stream_dip_reference_for_report(points: Option<&[V]>, at: &str, p: &V) -> bool {
    public_recovery_reference(points, at, p)
        .map(|r| r["ready"].as_bool().unwrap_or(false))
        .unwrap_or(false)
}
