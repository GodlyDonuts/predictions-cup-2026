// live-paper/coupled-maker-paper-v2.mjs — coupled bulk-history trade-tape paper reducer (v2).
use baal_execution::core::payload_hash;
use baal_execution::gates::normalize_official_fill_tick;
use baal_paper::fixed::{big_amount, money, SCALE, TICK};
use baal_service_runtime::contract::{instant_ns, ms_to_iso};
use baal_service_runtime::json_text::{js_int, js_truthy};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("COUPLED_BULK_{}", code)); } Ok(()) }
/// fixed.mjs instant — strict canonical ISO ms (JS throws on malformed).
fn inst(v: &V) -> Result<i64, String> {
    baal_service_runtime::fixed::instant(v.as_str().unwrap_or(""))
}
fn inst_str(s: &str) -> Result<i64, String> {
    baal_service_runtime::fixed::instant(s)
}
fn inst_ns(v: &V) -> Result<i128, String> {
    instant_ns(v.as_str().unwrap_or("")).map_err(|e| e.0)
}
fn amount(v: &V) -> Result<BigInt, String> { big_amount(v.as_str().unwrap_or("")) }
/// JS Number(v) coercion for comparisons the JS source runs through Number().
fn js_num(v: &V) -> f64 {
    match v {
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::String(s) => { let t = s.trim(); if t.is_empty() { 0.0 } else { t.parse::<f64>().unwrap_or(f64::NAN) } }
        V::Bool(b) => if *b { 1.0 } else { 0.0 },
        _ => f64::NAN,
    }
}
/// /^[1-9]\d*$/ .test(v) — regex .test coerces non-strings to String(v).
fn id(v: &V) -> bool {
    let s = match v {
        V::String(s) => s.clone(),
        V::Number(_) | V::Bool(_) => baal_service_runtime::json_text::js_stringify(v),
        _ => return false,
    };
    !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
}
/// v === null on an object member (missing key is NOT null in JS === checks).
fn explicit_null(v: &V, key: &str) -> bool {
    v.as_object().map(|m| m.get(key).map(|x| x.is_null()).unwrap_or(false)).unwrap_or(false)
}
/// JS Set identity key: strings, numbers and bools dedup by JS value identity.
fn js_key(v: &V) -> String {
    match v {
        V::String(s) => format!("s:{}", s),
        V::Number(_) => format!("n:{}", baal_service_runtime::json_text::js_stringify(v)),
        V::Bool(b) => format!("b:{}", b),
        V::Null => "null".into(),
        _ => baal_service_runtime::json_text::js_stringify(v),
    }
}

/// COUPLED_BULK_POLICY — frozen entry policy (v2).
pub fn coupled_bulk_policy() -> V {
    json!({"schemaVersion":2,"initialCash":"100.000000","maximumReservedAndOwned":"5.000000","maximumOwnedMarkets":3,"cumulativeBuyBudget":"25.000000","quantity":1,
        "latencyMs":3000,"lifetimeMs":120000,"cooldownMs":60000,"maximumHoldMs":7200000,"stopLoss":"0.030000","profitTarget":"0.005000","exitAdverseTicks":1,"depthFractionBps":100,"feePerShare":"0.000000",
        "historyMs":300000,"minimumHistorySpanMs":240000,"minimumDistinctMinutes":4,"maximumHistoryGapMs":120000,"maximumMidpointRange":"0.005000","minimumSpread":"0.005000","maximumSpread":"0.050000","improvementTicks":1,"maximumHistoryReceiptAgeMs":90000,"minimumPriceRequestSpacingMs":2000,
        "families":["COUPLED"],"fillModel":"hypothetical_strict_trade_through_not_proven_execution"})
}
/// validateCoupledBulkBank(bank)
pub fn validate_coupled_bulk_bank(bank: &V) -> Result<V, String> {
    need(bank["schemaVersion"].as_i64() == Some(1)
        && bank["mode"].as_str() == Some("coupled_bulk_history_trade_tape_paper_v2")
        && payload_hash(&bank["policy"]).ok() == payload_hash(&coupled_bulk_policy()).ok(), "BANK")?;
    inst(&bank["cutoff"])?;
    inst(&bank["entryExpiresAt"])?;
    need(inst(&bank["entryExpiresAt"])? > inst(&bank["cutoff"])?, "CLOCK")?;
    let legs = bank["legs"].as_array().cloned().unwrap_or_default();
    need(bank["legs"].is_array() && !legs.is_empty() && legs.len() <= 300
        && legs.iter().map(|l| js_key(&l["exchangeId"])).collect::<BTreeSet<_>>().len() == legs.len(), "SCOPE")?;
    need(js_truthy(bank.get("universe")) && bank["universe"]["tournamentId"] == bank["tournamentId"], "UNIVERSE")?;
    let hash = bank["universe"]["hash"].clone();
    let mut raw = bank["universe"].clone();
    raw.as_object_mut().unwrap().remove("hash");
    let source = if js_truthy(bank.get("universeSourceJson")) {
        let text = match &bank["universeSourceJson"] {
            V::String(s) => s.clone(),
            V::Number(_) | V::Bool(_) => baal_service_runtime::json_text::js_stringify(&bank["universeSourceJson"]),
            _ => return Err("Unexpected token in JSON".into()),
        };
        serde_json::from_str::<V>(&text).map_err(|e| e.to_string())?
    } else {
        raw
    };
    let validated = crate::breadth::validate_universe(&source, bank["tournamentId"].as_str().unwrap_or(""))?;
    need(validated["hash"] == hash
        && payload_hash(&validated).ok() == payload_hash(&bank["universe"]).ok(), "UNIVERSE_HASH")?;
    for l in &legs {
        need(id(&l["marketId"]) && id(&l["exchangeId"])
            && bank["universe"]["exchanges"].as_array().is_some_and(|xs| xs.iter()
                .any(|x| x["exchangeId"] == l["exchangeId"] && x["marketId"] == l["marketId"]
                    && x["option"].as_str() == Some("YES"))), "IDENTITY")?;
    }
    Ok(bank.clone())
}
/// createCoupledBulkState(bank)
pub fn create_coupled_bulk_state(bank: &V) -> Result<V, String> {
    validate_coupled_bulk_bank(bank)?;
    let mut wallets = serde_json::Map::new();
    for side in bank["policy"]["families"].as_array().cloned().unwrap_or_default() {
        wallets.insert(side.as_str().unwrap_or("").to_string(), json!({
            "cash": bank["policy"]["initialCash"], "realized": "0.000000", "fees": "0.000000",
            "cumulativeBuys": "0.000000", "positions": {}, "quotes": {}, "fills": [],
            "redemptions": [], "cooldowns": {}, "risk": {}}));
    }
    Ok(json!({"schemaVersion":1,"bankHash":payload_hash(bank)?,"lastAt":bank["cutoff"],"nextId":1,
        "seen":{},"books":{},"history":{},"coverage":{},"counts":{},"wallets":wallets}))
}
fn inc(counts: &mut V, k: &str) { counts[k] = json!(counts[k].as_i64().unwrap_or(0) + 1); }
fn open(w: &V) -> Vec<V> {
    w["quotes"].as_object().map(|m| m.values().filter(|q| !js_truthy(q.get("resolvedAt"))).cloned().collect()).unwrap_or_default()
}
fn open_ids(w: &V) -> Vec<String> {
    w["quotes"].as_object().map(|m| m.iter().filter(|(_, q)| !js_truthy(q.get("resolvedAt"))).map(|(k, _)| k.clone()).collect()).unwrap_or_default()
}
fn reserved(w: &V) -> Result<BigInt, String> {
    let mut n = BigInt::from(0);
    for q in open(w) {
        if q["action"].as_str() == Some("buy") { n += amount(&q["limit"])?; }
    }
    Ok(n)
}
fn owned(w: &V) -> Result<BigInt, String> {
    let mut n = BigInt::from(0);
    for p in w["positions"].as_object().map(|m| m.values().cloned().collect::<Vec<_>>()).unwrap_or_default() {
        n += amount(&p["cost"])?;
    }
    Ok(n)
}
fn open_for(w: &V, key: &str) -> bool {
    open(w).iter().any(|q| q["exchangeId"].as_str() == Some(key))
}
struct Px { bid: BigInt, ask: BigInt, buy_depth: f64, sell_depth: f64 }
/// prices(b, side) — YES uses the book directly; NO inverts bid/ask.
fn prices(b: &V, side: &str) -> Result<Px, String> {
    let bid_p = amount(&b["bids"][0]["price"])?;
    let ask_p = amount(&b["asks"][0]["price"])?;
    let bid_q = b["bids"][0]["quantity"].as_f64().unwrap_or(f64::NAN);
    let ask_q = b["asks"][0]["quantity"].as_f64().unwrap_or(f64::NAN);
    Ok(if side == "YES" {
        Px { bid: bid_p, ask: ask_p, buy_depth: bid_q, sell_depth: bid_q }
    } else {
        Px { bid: BigInt::from(SCALE) - ask_p, ask: BigInt::from(SCALE) - bid_p, buy_depth: ask_q, sell_depth: ask_q }
    })
}
/// assertCoupledBulk(s, bank)
pub fn assert_coupled_bulk(s: &V, bank: &V) -> Result<(), String> {
    let wallet_keys: Vec<String> = s["wallets"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    need(s["bankHash"].as_str() == payload_hash(bank).ok().as_deref() && wallet_keys.join("|") == "COUPLED", "BANK_CHANGED")?;
    let w = &s["wallets"]["COUPLED"];
    let mut buys = BigInt::from(0);
    let mut sells = BigInt::from(0);
    for f in w["fills"].as_array().cloned().unwrap_or_default() {
        if f["action"].as_str() == Some("buy") { buys += amount(&f["price"])?; }
        if f["action"].as_str() == Some("sell") { sells += amount(&f["price"])?; }
    }
    let payouts = BigInt::from(w["redemptions"].as_array().map(|a| a.len() as i64).unwrap_or(0)) * BigInt::from(SCALE);
    need(amount(&w["cash"])? == amount(&bank["policy"]["initialCash"])? - &buys + &sells + &payouts
        && amount(&w["cash"])? >= reserved(w)?, "CASH")?;
    need(amount(&w["cumulativeBuys"])? == buys && &buys + reserved(w)? <= amount(&bank["policy"]["cumulativeBuyBudget"])?, "TURNOVER")?;
    need(amount(&w["realized"])? == &sells + &payouts - (&buys - owned(w)?)
        && w["fees"].as_str() == Some("0.000000"), "FIFO")?;
    need(owned(w)? + reserved(w)? <= amount(&bank["policy"]["maximumReservedAndOwned"])?
        && w["positions"].as_object().map(|m| m.values().all(|p| p["quantity"].as_i64() == Some(1)
            && matches!(p["side"].as_str(), Some("YES") | Some("NO")))).unwrap_or(false), "RISK")?;
    let mut markets: BTreeSet<String> = w["positions"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    for q in open(w) { markets.insert(q["exchangeId"].as_str().unwrap_or("").to_string()); }
    need(markets.len() <= bank["policy"]["maximumOwnedMarkets"].as_u64().unwrap_or(0) as usize, "MARKET_LIMIT")?;
    let mut quote_keys: BTreeSet<String> = BTreeSet::new();
    for q in open(w) { quote_keys.insert(q["exchangeId"].as_str().unwrap_or("").to_string()); }
    for key in &quote_keys {
        for side in ["YES", "NO"] {
            need(open(w).iter().filter(|q| q["exchangeId"].as_str() == Some(key.as_str())
                && q["side"].as_str() == Some(side)).count() <= 1, "DUPLICATE_QUOTE")?;
        }
    }
    Ok(())
}
fn fill(s_counts: &mut V, w: &mut V, q: &mut V, at: &str, source: &V, bank: &V) -> Result<(), String> {
    let key = q["exchangeId"].as_str().unwrap_or("").to_string();
    let cost = amount(&q["limit"])?;
    let position = w["positions"][&key].clone();
    let has_position = !position.is_null();
    if q["action"].as_str() == Some("buy") {
        need(amount(&w["cash"])? >= cost
            && (!has_position || position["side"].as_str() != q["side"].as_str()), "BUY")?;
        w["cash"] = json!(money(&(amount(&w["cash"])? - &cost)));
        w["cumulativeBuys"] = json!(money(&(amount(&w["cumulativeBuys"])? + &cost)));
        if has_position {
            let gain = BigInt::from(SCALE) - amount(&position["cost"])? - &cost;
            w["cash"] = json!(money(&(amount(&w["cash"])? + BigInt::from(SCALE))));
            w["realized"] = json!(money(&(amount(&w["realized"])? + &gain)));
            w["redemptions"].as_array_mut().unwrap().push(json!({
                "marketId": q["marketId"], "exchangeId": key, "quantity": 1, "payout": "1.000000",
                "acquisitionCosts": [position["cost"].clone(), q["limit"].clone()],
                "realizedPnl": money(&gain), "at": at,
                "firstLegSide": position["side"], "firstLegSourceAt": position["sourceEntryAt"],
                "secondLegSourceAt": if source["kind"].as_str() == Some("trade") { source["createdAt"].clone() } else { json!(at) },
                "quoteId": q["id"]}));
            w["positions"].as_object_mut().unwrap().remove(&key);
            w["risk"].as_object_mut().unwrap().remove(&key);
            w["cooldowns"][&key] = json!(inst_str(at)? + bank["policy"]["cooldownMs"].as_i64().unwrap_or(0));
            inc(s_counts, "hypothetical_pair_redemption");
        } else {
            w["positions"][&key] = json!({"marketId": q["marketId"], "exchangeId": key, "side": q["side"],
                "quantity": 1, "cost": q["limit"], "enteredAt": at,
                "sourceEntryAt": if source["kind"].as_str() == Some("trade") { source["createdAt"].clone() } else { json!(at) }});
        }
    } else {
        need(position["quantity"].as_i64() == Some(1) && position["side"].as_str() == q["side"].as_str(), "COVERED_EXIT")?;
        w["cash"] = json!(money(&(amount(&w["cash"])? + &cost)));
        w["realized"] = json!(money(&(amount(&w["realized"])? + &cost - amount(&position["cost"])?)));
        w["positions"].as_object_mut().unwrap().remove(&key);
        w["risk"].as_object_mut().unwrap().remove(&key);
        w["cooldowns"][&key] = json!(inst_str(at)? + bank["policy"]["cooldownMs"].as_i64().unwrap_or(0));
    }
    q["resolvedAt"] = json!(at);
    q["resolution"] = json!("hypothetical_fill");
    w["fills"].as_array_mut().unwrap().push(json!({
        "action": q["action"], "marketId": q["marketId"], "exchangeId": key, "side": q["side"],
        "quantity": 1, "price": q["limit"], "at": at, "quoteId": q["id"], "source": source.clone(),
        "model": if source["kind"].as_str() == Some("trade") { bank["policy"]["fillModel"].clone() } else { json!("hypothetical_later_request_taker") }}));
    inc(s_counts, &format!("hypothetical_{}", q["action"].as_str().unwrap_or("")));
    Ok(())
}
fn quote(s_next_id: &mut V, s_counts: &mut V, w: &mut V, b: &V, side: &str, at: &str,
    action: &str, limit: BigInt, bank: &V) -> Result<(), String> {
    let tick = BigInt::from(TICK);
    let scale = BigInt::from(SCALE);
    need(limit >= tick && limit <= &scale - &tick && limit.clone() % &tick == BigInt::from(0), "PRICE")?;
    let activation = ((inst_str(at)? + bank["policy"]["latencyMs"].as_i64().unwrap_or(0)) as f64)
        .max((inst_ns(&b["sourceAt"])? / 1_000_000) as f64 + 2000.0);
    let expiry = inst_str(at)? + bank["policy"]["lifetimeMs"].as_i64().unwrap_or(0);
    let id = js_int(s_next_id).unwrap_or(1);
    *s_next_id = json!(id + 1);
    w["quotes"][id.to_string()] = json!({"id": id, "action": action, "side": side,
        "marketId": b["marketId"], "exchangeId": b["exchangeId"], "quantity": 1,
        "limit": money(&limit),
        "yesEquivalentLimit": money(&(if side == "YES" { limit.clone() } else { &scale - &limit })),
        "createdAt": at, "activationAt": ms_to_iso(activation), "expiresAt": ms_to_iso(expiry as f64),
        "bookVersion": b["versionKey"], "bookSequence": b["sequence"],
        "publicationHash": b["publicationHash"], "publicationSequence": b["publicationSequence"],
        "coverageBarrierSequence": V::Null, "resolvedAt": V::Null, "resolution": V::Null});
    inc(s_counts, &format!("quote_{}", action));
    Ok(())
}
/// observeBulkHistory(s, snapshot, at, bank) — bulk price receipts warm windows.
fn observe_bulk_history(s_counts: &mut V, s_history: &mut V, snapshot: &V, at: &str, bank: &V) -> Result<(), String> {
    crate::market_price_publication::validate_price_observation_pub(snapshot, &bank["universe"])?;
    let p = &bank["policy"];
    let received = inst(&snapshot["receivedAt"])?;
    let request = inst(&snapshot["requestStartedAt"])?;
    let known = inst_str(at)?;
    need(snapshot["tournamentId"] == bank["tournamentId"] && received <= known, "PRICE_CLOCK")?;
    let members: BTreeSet<String> = bank["legs"].as_array().map(|l| l.iter()
        .map(|x| x["exchangeId"].as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
    for id in snapshot["missingIds"].as_array().cloned().unwrap_or_default() {
        if let Some(id) = id.as_str() {
            if members.contains(id) { s_history.as_object_mut().unwrap().remove(id); }
        }
    }
    for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
        let eid = q["exchangeId"].as_str().unwrap_or("").to_string();
        if !members.contains(&eid) { continue; }
        let bid = match normalize_official_fill_tick(&q["bestBid"]) {
            Ok(t) => t["price"].as_str().unwrap_or("").to_string(),
            Err(_) => { s_history.as_object_mut().unwrap().remove(&eid); inc(s_counts, "unusable_bulk_quote"); continue; }
        };
        let ask = match normalize_official_fill_tick(&q["bestAsk"]) {
            Ok(t) => t["price"].as_str().unwrap_or("").to_string(),
            Err(_) => { s_history.as_object_mut().unwrap().remove(&eid); inc(s_counts, "unusable_bulk_quote"); continue; }
        };
        if amount(&json!(bid.clone()))? >= amount(&json!(ask.clone()))?
            || request <= inst(&bank["cutoff"])?
            || known - received > p["maximumHistoryReceiptAgeMs"].as_i64().unwrap_or(0) {
            s_history.as_object_mut().unwrap().remove(&eid);
            continue;
        }
        let mut rows = s_history[&eid].as_array().cloned().unwrap_or_default();
        if !rows.is_empty()
            && received - rows.last().unwrap()["received"].as_f64().unwrap_or(0.0) as i64
                > p["maximumHistoryGapMs"].as_i64().unwrap_or(0) {
            rows.clear();
            inc(s_counts, "bulk_history_gap");
        }
        if rows.is_empty()
            || request > rows.last().unwrap()["request"].as_i64().unwrap_or(0)
                + p["minimumPriceRequestSpacingMs"].as_i64().unwrap_or(0) {
            rows.push(json!({"received": received, "request": request, "known": known,
                "bid": bid, "ask": ask, "marketId": q["marketId"]}));
        }
        let history_ms = p["historyMs"].as_i64().unwrap_or(0);
        rows.retain(|r| received - r["received"].as_f64().unwrap_or(0.0) as i64 <= history_ms);
        s_history[&eid] = json!(rows);
        inc(s_counts, "bulk_feature_observation");
    }
    Ok(())
}
/// stableBulkWindow(s, key, now, p, book)
fn stable_bulk_window(s: &V, key: &str, now: i64, p: &V, book: Option<&V>) -> Result<bool, String> {
    let request = match book { Some(b) => inst(&b["requestStartedAt"])?, None => now };
    let history_ms = p["historyMs"].as_i64().unwrap_or(0);
    let rows: Vec<V> = s["history"][key].as_array().cloned().unwrap_or_default()
        .into_iter()
        .filter(|r| r["received"].as_f64().unwrap_or(0.0) >= (request - history_ms) as f64
            && r["received"].as_f64().unwrap_or(0.0) <= request as f64
            && r["known"].as_f64().unwrap_or(0.0) <= request as f64)
        .collect();
    let last = match rows.last() { Some(l) => l.clone(), None => return Ok(false) };
    if now - last["received"].as_f64().unwrap_or(0.0) as i64 > p["maximumHistoryReceiptAgeMs"].as_i64().unwrap_or(0)
        || rows.len() < 4
        || last["received"].as_f64().unwrap_or(0.0) - rows[0]["received"].as_f64().unwrap_or(0.0)
            < p["minimumHistorySpanMs"].as_f64().unwrap_or(0.0)
        || rows.iter().map(|r| (r["request"].as_f64().unwrap_or(0.0) / 60000.0).floor() as i64)
            .collect::<BTreeSet<_>>().len() < p["minimumDistinctMinutes"].as_i64().unwrap_or(0) as usize {
        return Ok(false);
    }
    let mut mids = Vec::new();
    for r in &rows { mids.push(amount(&r["bid"])? + amount(&r["ask"])?); }
    if let Some(b) = book { mids.push(amount(&b["bids"][0]["price"])? + amount(&b["asks"][0]["price"])?); }
    let range = mids.iter().max().unwrap().clone() - mids.iter().min().unwrap().clone();
    Ok(range <= BigInt::from(2) * amount(&p["maximumMidpointRange"])?)
}
/// coupledBulkCandidates(s, bank, at)
pub fn coupled_bulk_candidates(s: &V, bank: &V, at: &str) -> Result<Vec<V>, String> {
    let now = inst_str(at)?;
    let p = &bank["policy"];
    let mut out = Vec::new();
    for leg in bank["legs"].as_array().cloned().unwrap_or_default() {
        let eid = leg["exchangeId"].as_str().unwrap_or("").to_string();
        if !stable_bulk_window(s, &eid, now, p, None)? { continue; }
        let last = match s["history"][&eid].as_array().and_then(|r| r.last()) {
            Some(l) => l.clone(),
            None => continue,
        };
        let bid = amount(&last["bid"])?;
        let ask = amount(&last["ask"])?;
        let spread = &ask - &bid;
        let total = BigInt::from(SCALE) - &spread
            + BigInt::from(2) * BigInt::from(p["improvementTicks"].as_i64().unwrap_or(0)) * BigInt::from(TICK);
        if spread < amount(&p["minimumSpread"])? || spread > amount(&p["maximumSpread"])?
            || total > BigInt::from(SCALE) - amount(&p["profitTarget"])?
            || bid < BigInt::from(SCALE / 20) || ask > BigInt::from(19 * SCALE / 20) {
            continue;
        }
        let mut row = leg.clone();
        row["spreadTicks"] = json!((&spread / BigInt::from(TICK)).to_string().parse::<i64>().unwrap_or(0));
        row["indicativeOnly"] = json!(true);
        row["depthAvailable"] = json!(false);
        row["executionEligible"] = json!(false);
        out.push(row);
    }
    out.sort_by(|a, b| {
        b["spreadTicks"].as_i64().unwrap_or(0).cmp(&a["spreadTicks"].as_i64().unwrap_or(0))
            .then_with(|| js_num(&a["marketId"]).partial_cmp(&js_num(&b["marketId"])).unwrap_or(std::cmp::Ordering::Equal))
    });
    Ok(out)
}
fn bigint(v: &V) -> Result<BigInt, String> {
    match v {
        V::String(s) => BigInt::parse_bytes(s.as_bytes(), 10).ok_or_else(|| "Cannot convert to BigInt".to_string()),
        V::Number(_) => js_int(v).map(BigInt::from).ok_or_else(|| "Cannot convert to BigInt".to_string()),
        _ => Err("Cannot convert to BigInt".to_string()),
    }
}
fn js_num_string(v: &V) -> String { // String(x)
    match v {
        V::String(s) => s.clone(),
        V::Number(_) => baal_service_runtime::json_text::js_stringify(v),
        V::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}
fn record_coverage(s: &mut V, page: &V) -> Result<(), String> {
    let eid = page["exchangeId"].as_str().unwrap_or("").to_string();
    if s["coverage"][&eid].is_null() {
        s["coverage"][&eid] = json!({"pending": {}, "windows": []});
    }
    let mut c = std::mem::take(&mut s["coverage"][&eid]);
    let wallets = s["wallets"].clone();
    let last_at = s["lastAt"].clone();
    let mut counts = std::mem::take(&mut s["counts"]);
    let result = record_coverage_inner(&mut counts, &mut c, &wallets, &last_at, page);
    s["counts"] = counts;
    s["coverage"][&eid] = c;
    result
}
fn record_coverage_inner(counts: &mut V, c: &mut V, wallets: &V, last_at: &V, page: &V) -> Result<(), String> {
    let request = if page["requestCursor"].is_null() { V::Null } else { page["requestCursor"].clone() };
    let key = payload_hash(&json!([page["from"].clone(), page["to"].clone()]))?;
    let mut chain = if request.is_null() {
        json!({"from": page["from"], "to": page["to"], "expected": V::Null, "complete": true, "checkpoint": V::Null})
    } else {
        c["pending"][&key].clone()
    };
    if chain.is_null() || chain["expected"] != request
        || chain["from"] != page["from"] || chain["to"] != page["to"] {
        c["pending"].as_object_mut().unwrap().remove(&key);
        inc(counts, "coverage_cursor_gap");
        return Ok(());
    }
    let cov = &page["coverage"];
    need(cov.is_object(), "COVERAGE")?;
    let cp = &cov["projectedThroughSequence"];
    let complete = chain["complete"].as_bool().unwrap_or(false) && cov["complete"].as_bool() == Some(true) && !cp.is_null();
    chain["complete"] = json!(complete);
    if !cp.is_null() {
        let cp_string = js_num_string(cp);
        chain["checkpoint"] = if explicit_null(&chain, "checkpoint") || bigint(&chain["checkpoint"])? < bigint(cp)? {
            json!(cp_string)
        } else {
            json!(js_num_string(&chain["checkpoint"]))
        };
    }
    if js_truthy(page["pagination"].get("hasMore")) {
        need(page["pagination"]["nextCursor"].as_str().is_some_and(|s| !s.is_empty())
            && page["pagination"]["nextCursor"] != request, "CURSOR")?;
        chain["expected"] = page["pagination"]["nextCursor"].clone();
        c["pending"][&key] = chain;
        need(c["pending"].as_object().map(|m| m.len()).unwrap_or(0) <= 16, "WINDOW_LIMIT")?;
    } else {
        if chain["complete"].as_bool().unwrap_or(false) {
            c["windows"].as_array_mut().unwrap().push(json!({"from": chain["from"], "to": chain["to"], "checkpoint": chain["checkpoint"]}));
        }
        c["pending"].as_object_mut().unwrap().remove(&key);
    }
    let mut active: Vec<i64> = Vec::new();
    for w in wallets.as_object().map(|m| m.values()).into_iter().flatten() {
        for q in open(w) {
            if q["exchangeId"] == page["exchangeId"] { active.push(inst(&q["activationAt"])?); }
        }
    }
    let oldest = match active.iter().min() {
        Some(m) => *m,
        None => inst(last_at)?,
    };
    let mut kept = Vec::new();
    for x in c["windows"].as_array().cloned().unwrap_or_default() {
        if inst(&x["to"])? > oldest { kept.push(x); }
    }
    c["windows"] = json!(kept);
    Ok(())
}
/// covered(c, q) — window coverage chained from activation through expiry.
fn covered(c: &V, q: &V) -> Result<bool, String> {
    let mut through = inst(&q["activationAt"])?;
    let barrier = bigint(&q["coverageBarrierSequence"])?;
    let mut keyed: Vec<(i64, V)> = Vec::new();
    for x in c["windows"].as_array().cloned().unwrap_or_default() {
        if bigint(&x["checkpoint"])? >= barrier { keyed.push((inst(&x["from"])?, x)); }
    }
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, x) in keyed {
        if inst(&x["from"])? > through { break; }
        through = through.max(inst(&x["to"])?);
        if through >= inst(&q["expiresAt"])? { return Ok(true); }
    }
    Ok(false)
}
/// advanceCoupledBulk(state, event, bank) — JS shallow-copies counts/books
/// and shares or clones history/wallets/coverage/seen by event kind; the Rust
/// port deep-clones the input so the caller's state is never aliased, and the
/// returned value is identical (the JS history delete on ineligible books also
/// lands in the returned object here, just not in the caller's input object).
pub fn advance_coupled_bulk(state: &V, event: &V, bank: &V) -> Result<V, String> {
    need(state["bankHash"].as_str() == payload_hash(bank).ok().as_deref(), "BANK_CHANGED")?;
    let mut s = state.clone();
    let p = &bank["policy"];
    let at = event["availableAt"].as_str().unwrap_or("").to_string();
    let now = inst_str(&at)?;
    need(now >= inst(&s["lastAt"])?, "CLOCK")?;
    s["lastAt"] = event["availableAt"].clone();
    let kind = event["kind"].as_str().unwrap_or("");
    if kind == "boundary" {
        s["books"] = json!({});
        s["history"] = json!({});
        let mut counts = std::mem::take(&mut s["counts"]);
        inc(&mut counts, "source_boundary");
        s["counts"] = counts;
    } else if kind == "prices" {
        let mut counts = std::mem::take(&mut s["counts"]);
        let mut s_history = std::mem::take(&mut s["history"]);
        let result = observe_bulk_history(&mut counts, &mut s_history, &event["snapshot"], &at, bank);
        s["history"] = s_history;
        s["counts"] = counts;
        result?;
    } else if kind == "book" {
        let b = event["book"].clone();
        let key = b["exchangeId"].as_str().unwrap_or("").to_string();
        need(bank["legs"].as_array().is_some_and(|legs| legs.iter()
                .any(|l| l["marketId"] == b["marketId"] && l["exchangeId"] == b["exchangeId"]))
            && b["instrument"].as_str() == Some(format!("{}:YES", key).as_str())
            && b["tournamentId"] == bank["tournamentId"], "BOOK_SCOPE")?;
        need(inst(&b["receivedAt"])? <= now && inst(&b["requestStartedAt"])? <= inst(&b["receivedAt"])?, "RECEIPT")?;
        let fresh = now - inst(&b["requestStartedAt"])? <= 20000
            && crate::probability_receipt::probability_account_gate(&event["accountEligibility"], &b, &at).is_none();
        let mut w = std::mem::take(&mut s["wallets"]["COUPLED"]);
        let mut counts = std::mem::take(&mut s["counts"]);
        let stable = fresh && stable_bulk_window(&s, &key, now, p, Some(&b))?;
        if fresh {
            s["books"][&key] = b.clone();
        } else {
            s["books"].as_object_mut().unwrap().remove(&key);
            inc(&mut counts, "book_ineligible");
        }
        for id in open_ids(&w) {
            let q = w["quotes"][&id].clone();
            if q["exchangeId"].as_str() == Some(key.as_str())
                && explicit_null(&q, "coverageBarrierSequence")
                && inst(&b["requestStartedAt"])? >= inst(&q["expiresAt"])?
                && inst_ns(&b["sourceAt"])? >= inst(&q["expiresAt"])? as i128 * 1_000_000 {
                w["quotes"][&id]["coverageBarrierSequence"] = b["sequence"].clone();
            }
        }
        if fresh {
            let position = w["positions"][&key].clone();
            if !position.is_null() {
                let px = prices(&b, position["side"].as_str().unwrap_or(""))?;
                let mark = &px.bid - &BigInt::from(TICK);
                let timed_out = now as f64
                    - baal_service_runtime::contract::parse_iso_ms(position["sourceEntryAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
                    >= p["maximumHoldMs"].as_f64().unwrap_or(0.0);
                let loss = amount(&position["cost"])? - &mark >= amount(&p["stopLoss"])?;
                if (timed_out || loss)
                    && (w["risk"][&key].is_null() || w["risk"][&key]["reason"].as_str() == Some("profit_available")) {
                    w["risk"][&key] = json!({"createdAt": at,
                        "reason": if timed_out { "holding_limit" } else { "loss_limit" }});
                }
                if w["risk"][&key].is_null() && mark >= amount(&position["cost"])? + amount(&p["profitTarget"])? {
                    w["risk"][&key] = json!({"createdAt": at, "reason": "profit_available"});
                }
                if !w["risk"][&key].is_null() {
                    let allowed = w["risk"][&key]["reason"].as_str() != Some("profit_available")
                        || mark >= amount(&position["cost"])? + amount(&p["profitTarget"])?;
                    if allowed && !open_for(&w, &key)
                        && inst(&b["requestStartedAt"])? >= inst(&w["risk"][&key]["createdAt"])? + p["latencyMs"].as_i64().unwrap_or(0)
                        && mark > BigInt::from(0)
                        && (px.sell_depth * p["depthFractionBps"].as_f64().unwrap_or(0.0) / 10000.0).floor() >= 1.0 {
                        let next_id = js_int(&s["nextId"]).unwrap_or(1);
                        s["nextId"] = json!(next_id + 1);
                        let mut q = json!({"id": next_id, "action": "sell", "side": position["side"],
                            "marketId": b["marketId"], "exchangeId": key, "limit": money(&mark)});
                        fill(&mut counts, &mut w, &mut q, &at,
                            &json!({"kind": "book", "version": b["versionKey"],
                                "publicationHash": b["publicationHash"], "publicationSequence": b["publicationSequence"]}),
                            bank)?;
                    }
                } else if !open_for(&w, &key) {
                    let side = if position["side"].as_str() == Some("YES") { "NO" } else { "YES" };
                    let opposite = prices(&b, side)?;
                    let target = BigInt::from(SCALE) - amount(&position["cost"])? - amount(&p["profitTarget"])?;
                    let improved = &opposite.bid + BigInt::from(p["improvementTicks"].as_i64().unwrap_or(0)) * BigInt::from(TICK);
                    let passive = if improved < opposite.ask { improved } else { &opposite.ask - &BigInt::from(TICK) };
                    let limit = passive.min(target);
                    if limit >= BigInt::from(TICK) && limit <= BigInt::from(SCALE - TICK)
                        && (opposite.buy_depth * p["depthFractionBps"].as_f64().unwrap_or(0.0) / 10000.0).floor() >= 1.0
                        && amount(&w["cash"])? >= limit
                        && owned(&w)? + &limit <= amount(&p["maximumReservedAndOwned"])?
                        && amount(&w["cumulativeBuys"])? + &limit <= amount(&p["cumulativeBuyBudget"])? {
                        quote(&mut s["nextId"], &mut counts, &mut w, &b, side, &at, "buy", limit, bank)?;
                    }
                }
            } else if inst(&b["requestStartedAt"])? > inst(&bank["cutoff"])?
                && now < inst(&bank["entryExpiresAt"])?
                && !open_for(&w, &key)
                && w["cooldowns"][&key].as_i64().unwrap_or(0) <= now {
                if !stable {
                    inc(&mut counts, "unstable_or_warmup");
                } else {
                    let yes = prices(&b, "YES")?;
                    let no = prices(&b, "NO")?;
                    let spread = &yes.ask - &yes.bid;
                    let improvement = BigInt::from(p["improvementTicks"].as_i64().unwrap_or(0)) * BigInt::from(TICK);
                    let limits = vec![&yes.bid + &improvement, &no.bid + &improvement];
                    let total = &limits[0] + &limits[1];
                    let depth_bps = p["depthFractionBps"].as_f64().unwrap_or(0.0);
                    if spread < amount(&p["minimumSpread"])? || spread > amount(&p["maximumSpread"])?
                        || limits[0] >= yes.ask || limits[1] >= no.ask
                        || total > BigInt::from(SCALE) - amount(&p["profitTarget"])?
                        || limits.iter().any(|x| *x < BigInt::from(SCALE / 20) || *x > BigInt::from(19 * SCALE / 20))
                        || (yes.buy_depth * depth_bps / 10000.0).floor() < 1.0
                        || (no.buy_depth * depth_bps / 10000.0).floor() < 1.0 {
                        inc(&mut counts, "spread_or_depth");
                    } else {
                        let mut active_markets: BTreeSet<String> = w["positions"].as_object()
                            .map(|m| m.keys().cloned().collect()).unwrap_or_default();
                        for q in open(&w) {
                            active_markets.insert(q["exchangeId"].as_str().unwrap_or("").to_string());
                        }
                        if active_markets.len() >= p["maximumOwnedMarkets"].as_u64().unwrap_or(0) as usize
                            || owned(&w)? + reserved(&w)? + &total > amount(&p["maximumReservedAndOwned"])?
                            || amount(&w["cumulativeBuys"])? + reserved(&w)? + &total > amount(&p["cumulativeBuyBudget"])?
                            || amount(&w["cash"])? - reserved(&w)? < total {
                            inc(&mut counts, "capital");
                        } else {
                            quote(&mut s["nextId"], &mut counts, &mut w, &b, "YES", &at, "buy", limits[0].clone(), bank)?;
                            quote(&mut s["nextId"], &mut counts, &mut w, &b, "NO", &at, "buy", limits[1].clone(), bank)?;
                        }
                    }
                }
            }
        }
        s["counts"] = counts;
        s["wallets"]["COUPLED"] = w;
    } else if kind == "trades" {
        let page = event["page"].clone();
        need(page["tournamentId"] == bank["tournamentId"]
            && bank["legs"].as_array().is_some_and(|legs| legs.iter()
                .any(|l| l["marketId"] == page["marketId"] && l["exchangeId"] == page["exchangeId"]))
            && inst(&page["receivedAt"])? <= now, "PAGE_SCOPE")?;
        let from_ns = inst(&page["from"])? as i128 * 1_000_000;
        let to_ns = inst(&page["to"])? as i128 * 1_000_000;
        let mut keyed: Vec<(i128, String, V)> = Vec::new();
        for t in page["trades"].as_array().cloned().unwrap_or_default() {
            keyed.push((inst_ns(&t["createdAt"])?, t["key"].as_str().unwrap_or("").to_string(), t));
        }
        keyed.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let trades: Vec<V> = keyed.into_iter().map(|x| x.2).collect();
        let page_eid = page["exchangeId"].as_str().unwrap_or("").to_string();
        let mut w = std::mem::take(&mut s["wallets"]["COUPLED"]);
        let mut counts = std::mem::take(&mut s["counts"]);
        let mut seen = std::mem::take(&mut s["seen"]);
        for t in &trades {
            let tns = inst_ns(&t["createdAt"])?;
            need(tns >= from_ns && tns < to_ns, "TRADE_WINDOW")?;
            let tkey = t["key"].as_str().unwrap_or("").to_string();
            if js_truthy(seen.get(&tkey)) { continue; }
            seen[&tkey] = json!(true);
            let tick = crate::canonical_entry_exit_study::tape_tick(t)?;
            if tick.is_none()
                || tick.as_ref().unwrap()["tick"].as_i64() == Some(0)
                || tick.as_ref().unwrap()["tick"].as_i64() == Some(200) {
                inc(&mut counts, "unusable_trade");
                continue;
            }
            let tick = tick.unwrap();
            let mut remaining = match js_int(&t["size"]) {
                Some(r) if r > 0 => r,
                _ => return Err("COUPLED_BULK_TRADE_SIZE".into()),
            };
            for id in open_ids(&w) {
                if remaining == 0 { break; }
                let q = w["quotes"][&id].clone();
                if q["exchangeId"].as_str() != Some(page_eid.as_str())
                    || tns < inst(&q["activationAt"])? as i128 * 1_000_000
                    || tns >= inst(&q["expiresAt"])? as i128 * 1_000_000 { continue; }
                let yes = amount(&tick["price"])?;
                let limit = amount(&q["yesEquivalentLimit"])?;
                let through = if (q["action"].as_str() == Some("buy")) == (q["side"].as_str() == Some("YES")) {
                    yes <= &limit - &BigInt::from(TICK)
                } else {
                    yes >= &limit + &BigInt::from(TICK)
                };
                if !through { continue; }
                let mut q = q;
                let source = json!({"kind": "trade", "key": t["key"], "id": t["id"],
                    "createdAt": t["createdAt"], "firstAvailableAt": page["receivedAt"],
                    "normalizedYesPrice": tick["price"], "aggressorSide": V::Null,
                    "queuePositionProven": false, "executionProven": false});
                fill(&mut counts, &mut w, &mut q, &at, &source, bank)?;
                w["quotes"][&id] = q;
                remaining -= 1;
            }
        }
        s["seen"] = seen;
        s["counts"] = counts;
        s["wallets"]["COUPLED"] = w;
        record_coverage(&mut s, &page)?;
        let mut counts = std::mem::take(&mut s["counts"]);
        let mut w = std::mem::take(&mut s["wallets"]["COUPLED"]);
        for id in open_ids(&w) {
            let q = w["quotes"][&id].clone();
            if q["exchangeId"].as_str() == Some(page_eid.as_str())
                && !explicit_null(&q, "coverageBarrierSequence")
                && covered(&s["coverage"][&page_eid], &q)? {
                let mut q = q;
                q["resolvedAt"] = json!(at);
                q["resolution"] = json!("projection_observed_without_qualifying_trade");
                w["quotes"][&id] = q;
                inc(&mut counts, "quote_unfilled_resolved");
            }
        }
        s["counts"] = counts;
        s["wallets"]["COUPLED"] = w;
    } else {
        return Err("COUPLED_BULK_EVENT_KIND".into());
    }
    assert_coupled_bulk(&s, bank)?;
    Ok(s)
}
/// coupledBulkReport(s, bank, at)
pub fn coupled_bulk_report(s: &V, bank: &V, at: &str) -> Result<V, String> {
    assert_coupled_bulk(s, bank)?;
    let w = &s["wallets"]["COUPLED"];
    let now = inst_str(at)?;
    let mut proceeds = BigInt::from(0);
    let mut complete = true;
    let mut positions = Vec::new();
    for pos in w["positions"].as_object().map(|m| m.values().cloned().collect::<Vec<_>>()).unwrap_or_default() {
        let b = &s["books"][pos["exchangeId"].as_str().unwrap_or("")];
        let px = if b.is_null() { None } else { Some(prices(b, pos["side"].as_str().unwrap_or(""))?) };
        let fresh = !b.is_null() && now >= inst(&b["receivedAt"])? && now - inst(&b["receivedAt"])? <= 20000;
        let mark = if fresh
            && (px.as_ref().unwrap().sell_depth * bank["policy"]["depthFractionBps"].as_f64().unwrap_or(0.0) / 10000.0).floor() >= 1.0 {
            Some(&px.as_ref().unwrap().bid - &BigInt::from(TICK))
        } else {
            None
        };
        if mark.is_none() || mark.as_ref().unwrap() <= &BigInt::from(0) {
            complete = false;
        } else {
            proceeds += mark.as_ref().unwrap();
        }
        let mut p = pos.clone();
        p["estimatedLiquidationProceeds"] = match &mark {
            Some(m) if *m > BigInt::from(0) => json!(money(m)),
            _ => V::Null,
        };
        positions.push(p);
    }
    let sells = w["fills"].as_array().map(|a| a.iter().filter(|f| f["action"].as_str() == Some("sell")).count()).unwrap_or(0);
    let fills = w["fills"].as_array().map(|a| a.len()).unwrap_or(0);
    let redemptions = w["redemptions"].as_array().map(|a| a.len()).unwrap_or(0);
    let recent_fills: Vec<V> = w["fills"].as_array().cloned().unwrap_or_default().into_iter().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect();
    let recent_redemptions: Vec<V> = w["redemptions"].as_array().cloned().unwrap_or_default().into_iter().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect();
    let liquidation = if complete {
        json!(money(&(amount(&w["cash"])? + proceeds - amount(&bank["policy"]["initialCash"])?)))
    } else {
        V::Null
    };
    Ok(json!({"schemaVersion": 1, "mode": bank["mode"], "cohort": bank["name"],
        "bankHash": s["bankHash"], "updatedAt": at, "cutoff": bank["cutoff"], "counts": s["counts"],
        "bulkStableCandidates": coupled_bulk_candidates(s, bank, at)?.len(),
        "featureScope": bank["legs"].as_array().map(|a| a.len()).unwrap_or(0),
        "wallets": {"COUPLED": {"cash": w["cash"], "realizedPnl": w["realized"], "fees": w["fees"],
            "cumulativeBuys": w["cumulativeBuys"], "fills": fills,
            "closedTrades": sells + redemptions, "pairedRedemptions": redemptions,
            "positions": positions, "unresolvedQuotes": open(w), "reserved": money(&reserved(w)?),
            "recentFills": recent_fills, "recentRedemptions": recent_redemptions,
            "estimatedLiquidationPnl": liquidation}},
        "executionAuthority": false, "competitionOrders": 0, "networkRequests": 0,
        "profitabilityEstablished": false,
        "limitations": [
            "Bulk receipts provide indicative stability only; fresh full-book depth and account eligibility are still mandatory.",
            "One shared cash wallet nets opposite one-share fills at the documented binary payout; unmatched inventory still bears loss.",
            "Strict trade-through is a counterfactual fill assumption; anonymous trades do not prove queue position or actual execution.",
            "Expired unresolved quotes keep reservations until original-window projection is complete; learned fill times never enable retrospective exits."]}))
}
