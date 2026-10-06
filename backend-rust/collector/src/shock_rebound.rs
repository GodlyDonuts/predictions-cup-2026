// live-paper/shock-rebound.mjs — research candidate: no I/O, orders or fills.
use baal_paper::fixed::{big_amount, big_price, money, SCALE, TICK};
use baal_service_runtime::contract::{instant_ns, ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::canonical_hash;
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

pub fn shock_rebound_defaults() -> V {
    json!({"lookbackMs": 1800000, "minimumSamples": 8, "minimumSpanMs": 300000,
        "shockWindowMs": 120000, "maxAnchorAgeMs": 240000, "trendWindowMs": 900000,
        "minimumTrendSpanMs": 300000, "maxDataAgeMs": 15000, "maxSourceClockSkewMs": 2000,
        "maxObservationGapMs": 120000, "maxReboundWaitMs": 120000, "minConfirmationDelayMs": 1000,
        "cooldownMs": 60000, "minimumShock": "0.020000", "maximumShock": "0.100000",
        "minimumRoom": "0.010000", "maximumEffectiveSpread": "0.020000",
        "minimumPrice": "0.050000", "maximumPrice": "0.950000", "volatilityFloor": 0.0025,
        "minimumShockZ": 2.5, "minimumTrendPerMinute": -0.0025, "slippageTicks": 1,
        "confirmationTicks": 2, "orderShares": 10, "maxEntryCost": "25.000000",
        "depthFractionBps": 100, "feeBps": 0})
}
fn need(ok: bool, msg: &str) -> Result<(), String> { if !ok { return Err(msg.into()); } Ok(()) }
fn median(a: &[f64]) -> f64 {
    let mut b = a.to_vec(); b.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let i = b.len() / 2;
    if b.len() % 2 == 1 { b[i] } else { (b[i - 1] + b[i]) / 2.0 }
}
fn cfg_i64(c: &V, k: &str) -> i64 { c.get(k).and_then(V::as_i64).unwrap_or(0) }
fn cfg_f64(c: &V, k: &str) -> f64 { c.get(k).and_then(V::as_f64).unwrap_or(f64::NAN) }
fn cfg_str<'a>(c: &'a V, k: &str) -> &'a str { c.get(k).and_then(V::as_str).unwrap_or("") }
fn instant_ms(s: &str) -> Result<i128, String> {
    instant_ns(s).map(|ns| ns / 1_000_000).map_err(|e| e.0)
}
fn shock_config(input: &V) -> Result<V, String> {
    let defaults = shock_rebound_defaults();
    let dkeys: BTreeSet<&str> = defaults.as_object().unwrap().keys().map(String::as_str).collect();
    need(input.is_object() && input.as_object().unwrap().keys().all(|k| dkeys.contains(k.as_str())), "Unknown shock config")?;
    let mut c = defaults.as_object().unwrap().clone();
    for (k, v) in input.as_object().unwrap() { c.insert(k.clone(), v.clone()); }
    let c = V::Object(c);
    let mut int_keys: Vec<String> = c.as_object().unwrap().keys().filter(|k| k.ends_with("Ms")).cloned().collect();
    int_keys.extend(["minimumSamples", "confirmationTicks", "orderShares", "depthFractionBps"].iter().map(|s| s.to_string()));
    for k in &int_keys {
        need(cfg_i64(&c, k) > 0, &format!("Invalid shock config: {}", k))?;
    }
    need((1..=5).contains(&cfg_i64(&c, "slippageTicks")) && cfg_i64(&c, "feeBps") == 0, "Unsupported costs")?;
    need(cfg_i64(&c, "depthFractionBps") <= 100 && cfg_i64(&c, "orderShares") <= 100 && cfg_i64(&c, "minimumSamples") >= 8, "Unsafe sizing or history")?;
    need(cfg_i64(&c, "maxSourceClockSkewMs") <= 2000, "Source clock skew exceeds supported tolerance")?;
    need(cfg_i64(&c, "lookbackMs") >= cfg_i64(&c, "trendWindowMs")
        && cfg_i64(&c, "trendWindowMs") >= cfg_i64(&c, "minimumTrendSpanMs") + cfg_i64(&c, "shockWindowMs")
        && cfg_i64(&c, "maxAnchorAgeMs") >= cfg_i64(&c, "shockWindowMs")
        && cfg_i64(&c, "minimumSpanMs") >= cfg_i64(&c, "shockWindowMs")
        && cfg_i64(&c, "maxReboundWaitMs") >= cfg_i64(&c, "minConfirmationDelayMs"), "Inconsistent time windows")?;
    for k in ["minimumShock", "maximumShock", "minimumRoom", "maximumEffectiveSpread", "minimumPrice", "maximumPrice", "maxEntryCost"] {
        need(big_amount(cfg_str(&c, k)).map(|b| b > BigInt::from(0)).unwrap_or(false), &format!("Invalid amount: {}", k))?;
    }
    need(big_amount(cfg_str(&c, "minimumShock"))? < big_amount(cfg_str(&c, "maximumShock"))?
        && big_amount(cfg_str(&c, "maximumShock"))? < BigInt::from(SCALE)
        && big_price(cfg_str(&c, "minimumPrice"))? < big_price(cfg_str(&c, "maximumPrice"))?
        && big_amount(cfg_str(&c, "maxEntryCost"))? <= big_amount("25.000000")?, "Invalid price bounds")?;
    need(cfg_f64(&c, "volatilityFloor") > 0.0 && cfg_f64(&c, "minimumShockZ") > 0.0 && cfg_f64(&c, "minimumTrendPerMinute") < 0.0, "Invalid statistical limits")?;
    Ok(c)
}
/// createShockRebound({marketId, exchangeId, sourceHash, cutoff, config})
pub fn create_shock_rebound(market_id: &str, exchange_id: &str, source_hash: &str, cutoff: &str, overrides: &V) -> Result<V, String> {
    let id_ok = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && s.as_bytes()[0] != b'0';
    need(id_ok(market_id) && id_ok(exchange_id), "Invalid identity")?;
    need(source_hash.len() == 64 && source_hash.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "A frozen source SHA256 is required")?;
    instant_ms(cutoff)?;
    let c = shock_config(overrides)?;
    let bank = json!({"schemaVersion": 1, "name": "causal-shock-rebound-v1", "mode": "research_paper_only",
        "marketId": market_id, "exchangeId": exchange_id, "sourceHash": source_hash, "cutoff": cutoff,
        "config": c, "configHash": canonical_hash(&c)?});
    let state = json!({"schemaVersion": 1, "bankHash": canonical_hash(&bank)?, "history": [],
        "armed": null, "last": null, "cooldownUntil": null, "invalidatedAt": null});
    Ok(json!({"bank": bank, "state": state}))
}
fn check_state(s: &V, bank: &V) -> Result<(), String> {
    let policy_ok = bank.get("configHash").and_then(V::as_str)
        == shock_config(bank.get("config").unwrap_or(&V::Null)).ok().and_then(|c| canonical_hash(&c).ok()).as_deref();
    need(bank.get("name").and_then(V::as_str) == Some("causal-shock-rebound-v1") && policy_ok
        && s.get("schemaVersion").and_then(V::as_i64) == Some(1)
        && s.get("bankHash").and_then(V::as_str) == canonical_hash(bank).ok().as_deref(), "Frozen shock bank changed")?;
    need(s.get("history").is_some_and(V::is_array), "Invalid shock state")?;
    Ok(())
}
struct CheckedBook { at: i128, request_at: i128, source_ns: i128, bid: BigInt, ask: BigInt, book_hash: String }
fn checked_book(b: &V, bank: &V, s: &V) -> Result<CheckedBook, String> {
    let c = &bank["config"];
    let want_inst = format!("{}:YES", bank["exchangeId"].as_str().unwrap_or(""));
    need(b.get("marketId") == bank.get("marketId") && b.get("exchangeId") == bank.get("exchangeId")
        && b.get("instrument").and_then(V::as_str) == Some(want_inst.as_str()), "Book identity mismatch")?;
    let at = instant_ms(b["receivedAt"].as_str().unwrap_or(""))?;
    let request_at = instant_ms(b["requestStartedAt"].as_str().unwrap_or(""))?;
    let source = instant_ns(b["sourceAt"].as_str().unwrap_or("")).map_err(|e| e.0)?;
    let seq = b["sequence"].as_i64().ok_or("Book version mismatch")?;
    let version_key = format!("{}:{}", seq, ms_to_iso(source as f64 / 1_000_000.0));
    need(b["versionKey"].as_str() == Some(version_key.as_str()), "Book version mismatch")?;
    need(request_at <= at && (at - request_at) <= cfg_i64(c, "maxDataAgeMs") as i128
        && source <= (at + cfg_i64(c, "maxSourceClockSkewMs") as i128) * 1_000_000, "Noncausal or stale response")?;
    if let Some(last) = s.get("last").filter(|l| !l.is_null()) {
        need(at >= last["at"].as_i64().unwrap_or(0) as i128 && request_at >= last["requestAt"].as_i64().unwrap_or(0) as i128, "Receipt rollback")?;
        need(BigInt::from(seq) >= BigInt::from(last["sequence"].as_i64().unwrap_or(0))
            && source >= last["sourceNs"].as_str().unwrap_or("0").parse::<i128>().unwrap_or(0), "Source rollback")?;
    }
    for (side, dir) in [("bids", -1i32), ("asks", 1i32)] {
        let levels = b.get(side).and_then(V::as_array).filter(|a| !a.is_empty()).ok_or("Missing executable depth")?;
        let mut previous: Option<BigInt> = None;
        for l in levels {
            let p = big_price(l["price"].as_str().unwrap_or(""))?;
            need(l["quantity"].as_i64().is_some_and(|q| q > 0), "Invalid depth")?;
            if let Some(prev) = &previous {
                need(if dir < 0 { p < *prev } else { p > *prev }, "Unsorted depth")?;
            }
            previous = Some(p);
        }
    }
    let bid = big_price(b["bids"][0]["price"].as_str().unwrap_or(""))?;
    let ask = big_price(b["asks"][0]["price"].as_str().unwrap_or(""))?;
    need(bid < ask, "Crossed or locked book")?;
    let book_hash = canonical_hash(&json!({"bids": b["bids"], "asks": b["asks"]}))?;
    if s.get("last").filter(|l| !l.is_null()).is_some_and(|l| l["versionKey"] == b["versionKey"]) {
        need(book_hash == s["last"]["bookHash"].as_str().unwrap_or(""), "Changed depth under unchanged version")?;
    }
    Ok(CheckedBook { at, request_at, source_ns: source, bid, ask, book_hash })
}
fn features(history: &[V], q: &CheckedBook, c: &V) -> V {
    let prior: Vec<&V> = history.iter().filter(|h| {
        let at = h["at"].as_i64().unwrap_or(0) as i128;
        at <= q.request_at && at >= q.at - cfg_i64(c, "lookbackMs") as i128
    }).collect();
    let fail = |reason: &str| json!({"ready": false, "reason": reason});
    if prior.len() < cfg_i64(c, "minimumSamples") as usize
        || prior.last().unwrap()["at"].as_i64().unwrap_or(0) - prior[0]["at"].as_i64().unwrap_or(0) < cfg_i64(c, "minimumSpanMs") {
        return fail("insufficient_history");
    }
    if q.at - prior.last().unwrap()["at"].as_i64().unwrap_or(0) as i128 > cfg_i64(c, "maxObservationGapMs") as i128 {
        return fail("observation_gap");
    }
    let Some(anchor) = prior.iter().rev().find(|h| (h["at"].as_i64().unwrap_or(0) as i128) <= q.at - cfg_i64(c, "shockWindowMs") as i128) else {
        return fail("missing_shock_anchor");
    };
    if q.at - anchor["at"].as_i64().unwrap_or(0) as i128 > cfg_i64(c, "maxAnchorAgeMs") as i128 {
        return fail("missing_shock_anchor");
    }
    let background: Vec<&&V> = prior.iter().filter(|h| {
        let at = h["at"].as_i64().unwrap_or(0) as i128;
        at <= q.at - cfg_i64(c, "shockWindowMs") as i128 && at >= q.at - cfg_i64(c, "trendWindowMs") as i128
    }).collect();
    if background.len() < 3
        || background.last().unwrap()["at"].as_i64().unwrap_or(0) - background[0]["at"].as_i64().unwrap_or(0) < cfg_i64(c, "minimumTrendSpanMs") {
        return fail("insufficient_trend_history");
    }
    let t0 = background[0]["at"].as_i64().unwrap_or(0);
    let xs: Vec<f64> = background.iter().map(|h| (h["at"].as_i64().unwrap_or(0) - t0) as f64 / 60000.0).collect();
    let ys: Vec<f64> = background.iter().map(|h| h["mid"].as_f64().unwrap_or(0.0)).collect();
    let xbar = xs.iter().sum::<f64>() / xs.len() as f64;
    let ybar = ys.iter().sum::<f64>() / ys.len() as f64;
    let trend_per_minute = xs.iter().enumerate().map(|(i, x)| (x - xbar) * (ys[i] - ybar)).sum::<f64>()
        / xs.iter().map(|x| (x - xbar).powi(2)).sum::<f64>();
    let returns: Vec<f64> = prior.windows(2).filter(|w| w[1]["at"].as_i64() > w[0]["at"].as_i64()).map(|w| {
        (w[1]["mid"].as_f64().unwrap_or(0.0) - w[0]["mid"].as_f64().unwrap_or(0.0))
            / ((w[1]["at"].as_i64().unwrap_or(0) - w[0]["at"].as_i64().unwrap_or(0)) as f64 / 60000.0).sqrt()
    }).collect();
    let center = median(&returns);
    let volatility = cfg_f64(c, "volatilityFloor")
        .max(1.4826 * median(&returns.iter().map(|x| (x - center).abs()).collect::<Vec<_>>()));
    let anchor_mid = anchor["mid"].as_f64().unwrap_or(0.0);
    let drop = anchor_mid - (&q.bid + &q.ask).to_string().parse::<f64>().unwrap_or(0.0) / 2e6;
    let elapsed_minutes = (q.at - anchor["at"].as_i64().unwrap_or(0) as i128) as f64 / 60000.0;
    let reference_bid = BigInt::from(median(&background.iter()
        .map(|h| big_amount(h["bid"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0)).to_string().parse::<f64>().unwrap_or(0.0))
        .collect::<Vec<_>>()).floor() as i64);
    json!({"ready": true, "drop": drop, "shockZ": drop / (volatility * elapsed_minutes.sqrt()),
        "elapsedMinutes": elapsed_minutes, "volatility": volatility, "trendPerMinute": trend_per_minute,
        "anchorMid": money(&BigInt::from((anchor_mid * 1e6).round() as i64)), "referenceBid": money(&reference_bid)})
}
/// invalidateShockRebound(input, bank, at)
pub fn invalidate_shock_rebound(input: &V, bank: &V, at: &str) -> Result<V, String> {
    check_state(input, bank)?;
    let t = instant_ms(at)?;
    need(input.get("last").filter(|l| !l.is_null()).map_or(true, |l| t >= l["at"].as_i64().unwrap_or(0) as i128), "Invalidation rollback")?;
    let mut s = input.clone();
    s["history"] = json!([]);
    s["armed"] = V::Null;
    s["invalidatedAt"] = json!(at);
    Ok(s)
}
/// advanceShockRebound(input, book, bank) -> {state, signal, diagnostic}
pub fn advance_shock_rebound(input: &V, book: &V, bank: &V) -> Result<V, String> {
    check_state(input, bank)?;
    let q = checked_book(book, bank, input)?;
    let c = bank["config"].clone();
    let mut s = input.clone();
    let finish = |s: &V, reason: &str, extra: &V, signal: &V| -> V {
        let mut d = json!({"reason": reason, "at": book["receivedAt"]});
        if let (Some(dm), Some(em)) = (d.as_object_mut(), extra.as_object()) {
            for (k, v) in em { dm.insert(k.clone(), v.clone()); }
        }
        json!({"state": s, "signal": signal, "diagnostic": d})
    };
    let inv = s.get("invalidatedAt").and_then(V::as_str).and_then(|a| instant_ms(a).ok());
    if inv.is_some_and(|t| q.request_at <= t) { return Ok(finish(&s, "request_before_invalidation", &json!({}), &V::Null)); }
    let repeated = s.get("last").is_some_and(|l| !l.is_null() && l["versionKey"] == book["versionKey"]);
    let independent = s.get("last").map_or(true, |l| l.is_null()
        || q.source_ns > l["sourceNs"].as_str().unwrap_or("0").parse::<i128>().unwrap_or(0));
    s["last"] = json!({"at": q.at.to_string().parse::<i64>().unwrap_or(0), "requestAt": q.request_at.to_string().parse::<i64>().unwrap_or(0),
        "sourceNs": q.source_ns.to_string(), "sequence": book["sequence"], "versionKey": book["versionKey"], "bookHash": q.book_hash});
    if !s["armed"].is_null() {
        let total_drop_twice = BigInt::from(2) * big_amount(s["armed"]["anchorMid"].as_str().unwrap_or(""))? - &q.bid - &q.ask;
        if total_drop_twice > BigInt::from(2) * big_amount(cfg_str(&c, "maximumShock"))? {
            s["armed"] = V::Null; s["history"] = json!([]); s["invalidatedAt"] = book["receivedAt"].clone();
            return Ok(finish(&s, "maximum_shock_breached",
                &json!({"totalShock": money(&(total_drop_twice / BigInt::from(2))), "requiresBackgroundRebuild": true}), &V::Null));
        }
    }
    if !s["armed"].is_null() && q.at - s["armed"]["at"].as_i64().unwrap_or(0) as i128 > cfg_i64(&c, "maxReboundWaitMs") as i128 {
        s["armed"] = V::Null;
    }
    if repeated || !independent { return Ok(finish(&s, "no_independent_quote", &json!({}), &V::Null)); }
    let mut history = s["history"].as_array().cloned().unwrap_or_default();
    if !history.is_empty() && q.at - history.last().unwrap()["at"].as_i64().unwrap_or(0) as i128 > cfg_i64(&c, "maxObservationGapMs") as i128 {
        history.clear(); s["armed"] = V::Null;
    }
    let f = features(&history, &q, &c);
    let mid = (q.bid.clone() + q.ask.clone()).to_string().parse::<f64>().unwrap_or(0.0) / 2e6;
    history.push(json!({"at": q.at.to_string().parse::<i64>().unwrap_or(0), "mid": mid, "bid": money(&q.bid), "versionKey": book["versionKey"]}));
    history.retain(|h| h["at"].as_i64().unwrap_or(0) as i128 >= q.at - cfg_i64(&c, "lookbackMs") as i128);
    if history.len() > 2048 { history = history[history.len() - 2048..].to_vec(); }
    s["history"] = json!(history);
    let cutoff_ms = instant_ms(bank["cutoff"].as_str().unwrap_or("")).unwrap_or(0);
    if q.request_at <= cutoff_ms || q.at <= cutoff_ms { return Ok(finish(&s, "development_prefix", &json!({}), &V::Null)); }
    if let Some(cd) = s.get("cooldownUntil").and_then(V::as_str) {
        if q.at < instant_ms(cd).unwrap_or(0) { return Ok(finish(&s, "cooldown", &json!({}), &V::Null)); }
    }
    if !s["armed"].is_null() {
        if q.bid < big_amount(s["armed"]["troughBid"].as_str().unwrap_or(""))? {
            s["armed"]["troughBid"] = json!(money(&q.bid));
            s["armed"]["troughAt"] = json!(q.at.to_string().parse::<i64>().unwrap_or(0));
            s["armed"]["troughSourceNs"] = json!(q.source_ns.to_string());
            s["armed"]["troughVersion"] = book["versionKey"].clone();
            return Ok(finish(&s, "new_trough", &json!({}), &V::Null));
        }
        if q.request_at < s["armed"]["troughAt"].as_i64().unwrap_or(0) as i128 + cfg_i64(&c, "minConfirmationDelayMs") as i128
            || q.source_ns <= s["armed"]["troughSourceNs"].as_str().unwrap_or("0").parse::<i128>().unwrap_or(0)
            || book["versionKey"] == s["armed"]["troughVersion"] {
            return Ok(finish(&s, "confirmation_not_causal", &json!({}), &V::Null));
        }
        if q.bid < big_amount(s["armed"]["troughBid"].as_str().unwrap_or(""))? + BigInt::from(cfg_i64(&c, "confirmationTicks")) * BigInt::from(TICK) {
            return Ok(finish(&s, "awaiting_bid_recovery", &json!({}), &V::Null));
        }
        if f["ready"] != V::Bool(true) || f["trendPerMinute"].as_f64().unwrap_or(f64::NAN) < cfg_f64(&c, "minimumTrendPerMinute") {
            s["armed"] = V::Null;
            return Ok(finish(&s, "trend_veto", &f, &V::Null));
        }
        let slip = BigInt::from(cfg_i64(&c, "slippageTicks")) * BigInt::from(TICK);
        let entry = &q.ask + &slip;
        let exit = &q.bid - &slip;
        let spread = &entry - &exit;
        let reference_exit = big_amount(s["armed"]["referenceBid"].as_str().unwrap_or(""))? - &slip;
        let room = &reference_exit - &entry;
        if entry > big_price(cfg_str(&c, "maximumPrice"))? || entry < big_price(cfg_str(&c, "minimumPrice"))? || exit < BigInt::from(TICK) {
            return Ok(finish(&s, "price_bounds", &json!({}), &V::Null));
        }
        if spread > big_amount(cfg_str(&c, "maximumEffectiveSpread"))? {
            return Ok(finish(&s, "effective_spread", &json!({"effectiveSpread": money(&spread)}), &V::Null));
        }
        if room < big_amount(cfg_str(&c, "minimumRoom"))? {
            return Ok(finish(&s, "insufficient_recovery_room", &json!({"room": money(&room)}), &V::Null));
        }
        let quantity = [
            BigInt::from(cfg_i64(&c, "orderShares")),
            big_amount(cfg_str(&c, "maxEntryCost"))? / &entry,
            BigInt::from(book["asks"][0]["quantity"].as_i64().unwrap_or(0)) * BigInt::from(cfg_i64(&c, "depthFractionBps")) / BigInt::from(10000),
            BigInt::from(book["bids"][0]["quantity"].as_i64().unwrap_or(0)) * BigInt::from(cfg_i64(&c, "depthFractionBps")) / BigInt::from(10000),
        ].iter().cloned().min().unwrap().to_string().parse::<i64>().unwrap_or(0);
        if quantity == 0 { return Ok(finish(&s, "insufficient_depth", &json!({}), &V::Null)); }
        let mut signal = json!({"kind": "research_entry_proposal", "strategy": bank["name"], "bankHash": s["bankHash"],
            "sourceHash": bank["sourceHash"], "configHash": bank["configHash"], "cutoff": bank["cutoff"],
            "marketId": bank["marketId"], "exchangeId": bank["exchangeId"], "side": "yes", "action": "buy",
            "quantity": quantity, "maximumEntryPrice": money(&entry), "observedExitBidAfterCushion": money(&exit),
            "effectiveSpread": money(&spread), "recoveryReferenceBid": s["armed"]["referenceBid"],
            "recoveryRoom": money(&room), "at": book["receivedAt"], "sourceVersion": book["versionKey"],
            "shockAt": ms_to_iso(s["armed"]["at"].as_f64().unwrap_or(0.0)),
            "troughAt": ms_to_iso(s["armed"]["troughAt"].as_f64().unwrap_or(0.0)),
            "troughBid": s["armed"]["troughBid"], "troughSourceVersion": s["armed"]["troughVersion"],
            "featuresAtShock": s["armed"]["features"], "profitabilityEstablished": false});
        signal["signalId"] = json!(canonical_hash(&signal)?);
        s["armed"] = V::Null;
        s["cooldownUntil"] = json!(ms_to_iso(q.at as f64 + cfg_i64(&c, "cooldownMs") as f64));
        return Ok(finish(&s, "confirmed_rebound", &f, &signal));
    }
    if f["ready"] != V::Bool(true) {
        return Ok(finish(&s, f["reason"].as_str().unwrap_or(""), &json!({}), &V::Null));
    }
    if f["trendPerMinute"].as_f64().unwrap_or(f64::NAN) < cfg_f64(&c, "minimumTrendPerMinute") {
        return Ok(finish(&s, "trend_veto", &f, &V::Null));
    }
    let drop_micros = BigInt::from((f["drop"].as_f64().unwrap_or(0.0) * 1e6).round() as i64);
    if drop_micros < big_amount(cfg_str(&c, "minimumShock"))? || drop_micros > big_amount(cfg_str(&c, "maximumShock"))?
        || f["shockZ"].as_f64().unwrap_or(f64::NAN) < cfg_f64(&c, "minimumShockZ") {
        return Ok(finish(&s, "shock_gate", &f, &V::Null));
    }
    s["armed"] = json!({"at": q.at.to_string().parse::<i64>().unwrap_or(0), "anchorMid": f["anchorMid"],
        "troughAt": q.at.to_string().parse::<i64>().unwrap_or(0), "troughBid": money(&q.bid),
        "troughSourceNs": q.source_ns.to_string(), "troughVersion": book["versionKey"],
        "referenceBid": f["referenceBid"], "features": f});
    let f2 = s["armed"]["features"].clone();
    Ok(finish(&s, "shock_armed", &f2, &V::Null))
}
