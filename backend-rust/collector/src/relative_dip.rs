// live-paper/relative-dip.mjs — market-relative dip features over bulk quotes.
// The JS module keeps a WeakMap keyed by state object for movement memos; the
// port hands that cache to the caller instead (MovementCache pairs 1:1 with a
// state value, invalidated on every observe_relative_prices call).
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Map, Value as V};
use std::collections::HashMap;

/// RELATIVE_DIP_POLICY (frozen object in JS).
pub fn relative_dip_policy() -> V {
    json!({"horizonMs": 300000, "maxAgeMs": 120000, "maxLatencyMs": 20000, "windowMs": 21600000,
        "minMinutes": 30, "minSpanMs": 1800000, "minPeers": 12, "maximumSpread": 0.02,
        "minimumDip": 0.02, "residualThreshold": -0.05, "episodeSpacingMs": 1800000})
}

/// Per-state memo for `movement` computations, mirroring the module-level
/// WeakMap: keyed on (now, policy); invalidated by observe_relative_prices.
#[derive(Default)]
pub struct MovementCache {
    now: f64,
    policy: Option<V>,
    moves: HashMap<String, Option<V>>,
}
impl MovementCache {
    pub fn new() -> Self { Self::default() }
    pub fn invalidate(&mut self) {
        self.now = f64::NAN;
        self.policy = None;
        self.moves.clear();
    }
    fn get_moves(&mut self, state: &V, now: f64, policy: &V) -> &HashMap<String, Option<V>> {
        if self.now != now || self.policy.as_ref() != Some(policy) {
            self.moves = state["series"].as_object().map(|m| m.iter()
                .map(|(id, s)| (id.clone(), movement(s, now, policy))).collect()).unwrap_or_default();
            self.now = now;
            self.policy = Some(policy.clone());
        }
        &self.moves
    }
}

fn median(a: &[f64]) -> f64 {
    let mut b = a.to_vec();
    b.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let m = b.len() / 2;
    if b.len() % 2 == 1 { b[m] } else { (b[m - 1] + b[m]) / 2.0 }
}
fn logit(p: f64) -> f64 {
    let p = p.clamp(0.02, 0.98);
    (p / (1.0 - p)).ln()
}
fn pf(p: Option<&V>, k: &str) -> f64 {
    p.and_then(|p| p.get(k)).and_then(V::as_f64).unwrap_or(f64::NAN)
}
/// Date.parse on an `at` field (ISO string); NaN when missing/unparseable.
fn pat(p: Option<&V>, k: &str) -> f64 {
    p.and_then(|p| p.get(k)).and_then(V::as_str).and_then(parse_iso_ms).unwrap_or(f64::NAN)
}
fn pol(p: &V, k: &str) -> f64 { p.get(k).and_then(V::as_f64).unwrap_or(f64::NAN) }
/// JS number emission: integral values serialize as integers; NaN/Infinity
/// become null (serde_json maps non-finite f64 to Null, as does JSON.stringify).
fn jnum(x: f64) -> V {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 9.007_199_254_740_992e15 { json!(x as i64) } else { json!(x) }
}

/// relativeIdentity(title) — cached title parsing is a research grouping only,
/// not resolution verification.
pub fn relative_identity(title: &str) -> Option<V> {
    // /^Will the (Democratic|Republican) Party win the (.+)\?$/
    let rest = title.strip_prefix("Will the ")?;
    let (party, after) = rest.split_once(" Party win the ")?;
    if party != "Democratic" && party != "Republican" { return None; }
    let race = after.strip_suffix('?')?;
    if race.is_empty() { return None; }
    Some(json!({"race": race, "sign": if party == "Democratic" { 1 } else { -1 }}))
}

/// createRelativeHistory(universe, identities)
pub fn create_relative_history(universe: &V, identities: &V) -> V {
    let mut ids = Map::new();
    for x in universe["exchanges"].as_array().cloned().unwrap_or_default() {
        let title = identities[x["marketId"].as_str().unwrap_or("")]["title"].as_str().unwrap_or("");
        ids.insert(x["exchangeId"].as_str().unwrap_or("").to_string(),
            relative_identity(title).unwrap_or(V::Null));
    }
    json!({"universeHash": universe["hash"], "lastAt": V::Null, "series": {}, "identities": ids})
}

/// observeRelativePrices(state, snapshot, universe, policy) — mutates state.
pub fn observe_relative_prices(state: &mut V, snapshot: &V, universe: &V, policy: &V,
    cache: &mut MovementCache) -> Result<(), String> {
    crate::market_price_publication::validate_price_observation_pub(snapshot, universe)?;
    let now = parse_iso_ms(snapshot["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
    if state["universeHash"] != universe["hash"]
        || (!state["lastAt"].is_null()
            && now < parse_iso_ms(state["lastAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)) {
        return Err("RELATIVE_SCOPE_CLOCK".into());
    }
    cache.invalidate();
    for id in snapshot["missingIds"].as_array().cloned().unwrap_or_default() {
        let id = id.as_str().unwrap_or("").to_string();
        // JS if(state.series[id]) — truthy check, not just non-null.
        if js_truthy(state["series"].get(id.as_str())) {
            state["series"][&id]["unavailable"] = json!(true);
        }
    }
    let max_latency = pol(policy, "maxLatencyMs");
    let window_ms = pol(policy, "windowMs");
    let started = parse_iso_ms(snapshot["requestStartedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
    for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
        let eid = q["exchangeId"].as_str().unwrap_or("").to_string();
        if state["series"][&eid].is_null() {
            state["series"][&eid] = json!({"marketId": q["marketId"], "points": [], "unavailable": false});
        }
        let bid = q["bestBid"].as_f64().unwrap_or(f64::NAN);
        let ask = q["bestAsk"].as_f64().unwrap_or(f64::NAN);
        // JS q.bestBid===null / q.bestAsk===null are strict null checks; a missing
        // key is undefined (not null), so the point is built without that key.
        if now - started > max_latency
            || q.get("bestBid").map(|v| v.is_null()).unwrap_or(false)
            || q.get("bestAsk").map(|v| v.is_null()).unwrap_or(false)
            || js_truthy(q.get("crossed")) || bid <= 0.0 || ask >= 1.0 {
            state["series"][&eid]["unavailable"] = json!(true);
            continue;
        }
        let mut point = Map::new();
        point.insert("at".into(), snapshot["receivedAt"].clone());
        point.insert("bucket".into(), json!((now / 60000.0).floor() as i64));
        if let Some(bv) = q.get("bestBid") { point.insert("bid".into(), bv.clone()); }
        if let Some(av) = q.get("bestAsk") { point.insert("ask".into(), av.clone()); }
        point.insert("mid".into(), jnum((bid + ask) / 2.0));
        let point = V::Object(point);
        let mut points = state["series"][&eid]["points"].as_array().cloned().unwrap_or_default();
        if points.last().and_then(|p| p["bucket"].as_f64()) == point["bucket"].as_f64() {
            let n = points.len() - 1;
            points[n] = point;
        } else {
            points.push(point);
        }
        points.retain(|p| parse_iso_ms(p["at"].as_str().unwrap_or("")).unwrap_or(f64::NAN) >= now - window_ms);
        state["series"][&eid]["points"] = json!(points);
        state["series"][&eid]["unavailable"] = json!(false);
    }
    state["lastAt"] = snapshot["receivedAt"].clone();
    Ok(())
}

fn movement(series: &V, now: f64, policy: &V) -> Option<V> {
    if series.is_null() || series["unavailable"].as_bool().unwrap_or(false) { return None; }
    let points = series["points"].as_array()?;
    let le = |p: &V, t: f64| parse_iso_ms(p["at"].as_str().unwrap_or("")).unwrap_or(f64::NAN) <= t;
    let current = points.iter().rev().find(|p| le(p, now))?;
    let target = now - pol(policy, "horizonMs");
    let prior = points.iter().rev().find(|p| le(p, target))?;
    let max_age = pol(policy, "maxAgeMs");
    let max_spread = pol(policy, "maximumSpread");
    if now - pat(Some(current), "at") > max_age
        || target - pat(Some(prior), "at") > max_age
        || pf(Some(current), "ask") - pf(Some(current), "bid") > max_spread + 1e-9
        || pf(Some(prior), "ask") - pf(Some(prior), "bid") > max_spread + 1e-9 {
        return None;
    }
    let change = logit(pf(Some(current), "mid")) - logit(pf(Some(prior), "mid"));
    Some(json!({"current": current, "prior": prior, "change": jnum(change)}))
}

/// relativeDipFeatures(state, exchangeId, clockAt, policy)
pub fn relative_dip_features(state: &V, exchange_id: &str, clock_at: &str, policy: &V,
    cache: &mut MovementCache) -> Result<V, String> {
    let now = parse_iso_ms(clock_at).unwrap_or(f64::NAN);
    if !now.is_finite()
        || (!state["lastAt"].is_null()
            && now < parse_iso_ms(state["lastAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)) {
        return Err("RELATIVE_FEATURE_CLOCK".into());
    }
    let moves = cache.get_moves(state, now, policy);
    let own = moves.get(exchange_id).and_then(|o| o.clone()).unwrap_or(V::Null);
    let identity = &state["identities"][exchange_id];
    if own.is_null() || identity.is_null() {
        return Ok(json!({"ready": false,
            "reason": if identity.is_null() { "unclassified_identity" } else { "unaligned_quotes" }}));
    }
    let mut races: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    for (id, _) in state["series"].as_object().cloned().unwrap_or_default() {
        let meta = &state["identities"][&id];
        if meta.is_null() || meta["race"] == identity["race"] { continue; }
        let Some(mv) = moves.get(&id).and_then(|o| o.as_ref()).cloned() else { continue; };
        // Count one independent race; prefer its Democratic contract consistently.
        let sign = meta["sign"].as_f64().unwrap_or(0.0);
        let value = sign * mv["change"].as_f64().unwrap_or(f64::NAN);
        let old = races.get(meta["race"].as_str().unwrap_or(""));
        if old.is_none() || sign > old.unwrap()["sign"].as_f64().unwrap_or(f64::NEG_INFINITY) {
            races.insert(meta["race"].as_str().unwrap_or("").to_string(), json!({"sign": sign, "value": value}));
        }
    }
    let peer_changes: Vec<f64> = races.values().map(|x| x["value"].as_f64().unwrap_or(f64::NAN)).collect();
    let factor: Option<f64> = if peer_changes.is_empty() { None } else { Some(median(&peer_changes)) };
    let cur_bucket = own["current"]["bucket"].as_f64().unwrap_or(f64::NAN);
    let past: Vec<V> = state["series"][exchange_id]["points"].as_array().cloned().unwrap_or_default().into_iter()
        .filter(|p| p["bucket"].as_f64().unwrap_or(f64::NAN) < cur_bucket - 1.0
            && parse_iso_ms(p["at"].as_str().unwrap_or("")).unwrap_or(f64::NAN) >= now - pol(policy, "windowMs"))
        .collect();
    let mut bids: Vec<f64> = past.iter().map(|p| p["bid"].as_f64().unwrap_or(f64::NAN)).collect();
    bids.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let enough = past.len() as f64 >= pol(policy, "minMinutes")
        && parse_iso_ms(past.last().map(|p| p["at"].as_str().unwrap_or("")).unwrap_or("")).unwrap_or(f64::NAN)
            - parse_iso_ms(past.first().map(|p| p["at"].as_str().unwrap_or("")).unwrap_or("")).unwrap_or(f64::NAN)
            >= pol(policy, "minSpanMs");
    let reference_bid: Option<f64> = if enough {
        Some(bids[((bids.len() - 1) as f64 * 0.25).floor() as usize])
    } else { None };
    let discount: Option<f64> = reference_bid.map(|r| r - pf(own.get("current"), "ask"));
    let ready = enough && peer_changes.len() as f64 >= pol(policy, "minPeers");
    let min_dip = pol(policy, "minimumDip") - 1e-9;
    let own_change = own["change"].as_f64().unwrap_or(f64::NAN);
    let sign = identity["sign"].as_f64().unwrap_or(f64::NAN);
    let residual = factor.map(|f| own_change - sign * f);
    Ok(json!({
        "ready": ready,
        "reason": if ready { "aligned_prior_reference" } else if !enough { "insufficient_history" } else { "insufficient_independent_peers" },
        "marketId": state["series"][exchange_id]["marketId"],
        "exchangeId": exchange_id,
        "clockAt": clock_at,
        "quoteAt": own["current"]["at"],
        "entryAsk": own["current"]["ask"],
        "currentBid": own["current"]["bid"],
        "referenceBid": reference_bid.map(jnum).unwrap_or(V::Null),
        "discount": discount.map(jnum).unwrap_or(V::Null),
        "ownChange": own["change"],
        "peerFactor": factor.map(jnum).unwrap_or(V::Null),
        "peerRaces": peer_changes.len(),
        "residual": residual.map(jnum).unwrap_or(V::Null),
        "localDip": ready && discount.unwrap_or(f64::NAN) >= min_dip,
        "residualDip": ready && discount.unwrap_or(f64::NAN) >= min_dip
            && residual.unwrap_or(f64::NAN) <= pol(policy, "residualThreshold"),
        "executionAuthority": false,
    }))
}
