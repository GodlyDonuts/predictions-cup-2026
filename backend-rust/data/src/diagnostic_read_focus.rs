// live-paper/diagnostic-read-focus.mjs — tail-window diagnostic depth-read focus.
use baal_collector::bid_anchor::bid_anchor_policy;
use baal_collector::book_publication::create_book_publication_state;
use baal_collector::breadth::validate_universe;
use baal_collector::bulk_dip_diagnostic::diagnose_quoted_dip;
use baal_collector::market_price_publication::validate_price_observation_pub;
use baal_paper::fixed::{big_amount as big_amount_of, instant, money, TICK};
use baal_service_runtime::json_text::{canonical_json, digest, sha256};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn hash(x: &V) -> Result<String, String> { Ok(sha(canonical_json(x)?.as_bytes())) }
fn decimal(n: f64) -> V { json!(money(&BigInt::from((n * 1e6).round() as i64))) }
fn amount(v: &V) -> BigInt { big_amount_of(v.as_str().unwrap_or("")).unwrap_or_else(|_| BigInt::from(0)) }
fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("DIAGNOSTIC_FOCUS_{}", code)); }
    Ok(())
}
fn inst(v: &V) -> i64 { instant(v.as_str().unwrap_or("")).unwrap_or(0) }
fn null_present(v: &V, key: &str) -> bool {
    v.as_object().is_some_and(|m| m.get(key).is_some_and(|x| x.is_null()))
}
fn truthy(v: &V) -> bool {
    match v {
        V::Null => false,
        V::Bool(b) => *b,
        V::Number(n) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        V::String(s) => !s.is_empty(),
        _ => true,
    }
}
fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64).unwrap_or(0)
}
fn iso(ms: i64) -> String { baal_service_runtime::contract::ms_to_iso(ms as f64) }

pub fn diagnostic_focus_policy() -> V {
    json!({"minimumGrossDiscount": "0.010000", "lifetimeMs": 60000, "cooldownMs": 900000,
        "maximumAgeMs": 20000, "maximumMarkets": 2})
}

/// diagnosticFocusCodeHash() — sha256 over live-paper/diagnostic-read-focus.mjs
/// and live-paper/runner.mjs bytes, taken from `live_paper_dir`.
pub fn diagnostic_focus_code_hash(live_paper_dir: &Path) -> Result<String, String> {
    let mut h = sha2::Sha256::new();
    use sha2::Digest;
    h.update(std::fs::read(live_paper_dir.join("diagnostic-read-focus.mjs")).map_err(|e| e.to_string())?);
    h.update(std::fs::read(live_paper_dir.join("runner.mjs")).map_err(|e| e.to_string())?);
    Ok(format!("{:x}", h.finalize()))
}

pub fn create_diagnostic_focus_state(universe: &V) -> Result<V, String> {
    let mut raw = universe.clone();
    let stored_hash = raw.as_object_mut().and_then(|m| m.shift_remove("hash")).unwrap_or(V::Null);
    let u = validate_universe(&raw, raw["tournamentId"].as_str().unwrap_or(""))?;
    need(stored_hash.is_null() || stored_hash == u["hash"], "UNIVERSE_HASH")?;
    let book_scope = create_book_publication_state(&raw)?["scopeHash"].clone();
    Ok(json!({"universe": u, "bookScopeHash": book_scope,
        "book": {"sequence": 0, "hash": "", "at": V::Null},
        "price": {"sequence": 0, "hash": "", "at": V::Null},
        "closed": true, "history": {}, "candidates": {}, "cooldowns": {},
        "barriers": {"global": V::Null, "markets": {}}}))
}

pub fn advance_diagnostic_focus(s: &mut V, kind: &str, row: &V) -> Result<(), String> {
    let policy = diagnostic_focus_policy();
    let anchor = bid_anchor_policy();
    let now = inst(&row["at"]);
    if kind == "book" {
        let mut body = row.clone();
        let record_hash = body.as_object_mut().and_then(|m| m.shift_remove("hash")).unwrap_or(V::Null);
        need(body["sequence"].as_i64() == Some(s["book"]["sequence"].as_i64().unwrap_or(0) + 1)
            && body["previousHash"].as_str() == s["book"]["hash"].as_str()
            && hash(&body)? == record_hash.as_str().unwrap_or("")
            && body["tournamentId"] == s["universe"]["tournamentId"]
            && body["scopeHash"] == s["bookScopeHash"]
            && (s["book"]["at"].is_null() || now >= inst(&s["book"]["at"])), "BOOK_CHAIN")?;
        need(matches!(row["kind"].as_str(), Some("book") | Some("session_start") | Some("session_stop") | Some("invalidation")), "BOOK_KIND")?;
        s["book"] = json!({"sequence": row["sequence"], "hash": row["hash"], "at": row["at"]});
        if row["kind"].as_str() == Some("book") { return Ok(()); }
        let id = if row["kind"].as_str() == Some("invalidation") {
            row["payload"]["marketId"].as_str().map(str::to_string)
        } else { None };
        need(id.is_none() || s["universe"]["exchanges"].as_array().cloned().unwrap_or_default().iter()
            .any(|x| x["marketId"].as_str() == id.as_deref()), "BOUNDARY_SCOPE")?;
        match id {
            None => {
                s["history"] = json!({});
                s["candidates"] = json!({});
                s["barriers"]["global"] = row["at"].clone();
            }
            Some(id) => {
                s["history"].as_object_mut().unwrap().shift_remove(&id);
                s["candidates"].as_object_mut().unwrap().shift_remove(&id);
                s["barriers"]["markets"][&id] = row["at"].clone();
            }
        }
        if row["kind"].as_str() == Some("session_start") { s["closed"] = json!(false); }
        else if row["kind"].as_str() == Some("session_stop") { s["closed"] = json!(true); }
        return Ok(());
    }
    need(kind == "price", "SOURCE_KIND")?;
    let mut body = row.clone();
    let record_hash = body.as_object_mut().and_then(|m| m.shift_remove("hash")).unwrap_or(V::Null);
    let snapshot = body["event"]["snapshot"].clone();
    need(body["sequence"].as_i64() == Some(s["price"]["sequence"].as_i64().unwrap_or(0) + 1)
        && body["previous"].as_str() == s["price"]["hash"].as_str()
        && digest(&body) == record_hash.as_str().unwrap_or("")
        && null_present(&body, "state")
        && body["event"]["type"].as_str() == Some("market_price_observation")
        && (s["price"]["at"].is_null() || now >= inst(&s["price"]["at"])), "PRICE_CHAIN")?;
    validate_price_observation_pub(&snapshot, &s["universe"])?;
    need(inst(&snapshot["receivedAt"]) <= now, "PRICE_CLOCK")?;
    s["price"] = json!({"sequence": row["sequence"], "hash": row["hash"], "at": row["at"]});
    let by_exchange: std::collections::HashMap<String, String> = s["universe"]["exchanges"].as_array().cloned().unwrap_or_default().iter()
        .filter_map(|x| Some((x["exchangeId"].as_str()?.to_string(), x["marketId"].as_str()?.to_string()))).collect();
    for miss in snapshot["missingIds"].as_array().cloned().unwrap_or_default() {
        let market = by_exchange.get(miss.as_str().unwrap_or("")).cloned().unwrap_or_else(|| "undefined".into());
        s["history"].as_object_mut().unwrap().shift_remove(&market);
        s["candidates"].as_object_mut().unwrap().shift_remove(&market);
        s["barriers"]["markets"][&market] = row["at"].clone();
    }
    let maximum_age = policy["maximumAgeMs"].as_i64().unwrap_or(20000);
    if now - inst(&snapshot["receivedAt"]) > maximum_age {
        for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
            let m = q["marketId"].as_str().unwrap_or("").to_string();
            s["history"].as_object_mut().unwrap().shift_remove(&m);
            s["candidates"].as_object_mut().unwrap().shift_remove(&m);
            s["barriers"]["markets"][&m] = row["at"].clone();
        }
        return Ok(());
    }
    let closed = s["closed"].as_bool() == Some(true);
    for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
        let id = q["marketId"].as_str().unwrap_or("").to_string();
        let request_start = inst(&snapshot["requestStartedAt"]);
        let blocked = [s["barriers"]["global"].clone(), s["barriers"]["markets"][&id].clone()].iter()
            .any(|at| !at.is_null() && !(request_start > inst(at)));
        if closed || blocked { continue; }
        // `bestBid===null`/`bestAsk===null` are strict null checks (missing !== null),
        // while <=/>= comparisons treat missing as NaN (false).
        let bid = q["bestBid"].as_f64().unwrap_or(f64::NAN);
        let ask = q["bestAsk"].as_f64().unwrap_or(f64::NAN);
        let invalid = q["option"].as_str() != Some("YES") || truthy(&q["crossed"])
            || null_present(&q, "bestBid") || null_present(&q, "bestAsk")
            || bid <= 0.0 || ask >= 1.0 || ask <= bid;
        if invalid {
            s["history"].as_object_mut().unwrap().shift_remove(&id);
            s["candidates"].as_object_mut().unwrap().shift_remove(&id);
            s["barriers"]["markets"][&id] = row["at"].clone();
            continue;
        }
        let mut points: Vec<V> = s["history"][&id].as_array().cloned().unwrap_or_default();
        let gate = diagnose_quoted_dip(&points, &q, snapshot["receivedAt"].as_str().unwrap_or(""), None)?;
        let prior = s["candidates"][&id].clone();
        let gross: Option<BigInt> = if gate["features"].is_null() { None }
            else { Some(amount(&gate["features"]["netConcession"]) + BigInt::from(2) * BigInt::from(TICK)) };
        s["candidates"].as_object_mut().unwrap().shift_remove(&id);
        let cooldown_at = s["cooldowns"][&id].as_i64().unwrap_or(0);
        if gross.is_some() && gross.as_ref().unwrap() >= &big_amount_of("0.010000").unwrap()
            && gate["failures"].as_array().cloned().unwrap_or_default().iter().all(|x| x.as_str() == Some("insufficient_concession"))
            && now >= cooldown_at {
            let candidate = json!({"marketId": id, "exchangeId": q["exchangeId"], "availableAt": row["at"],
                "latestAvailableAt": row["at"], "receivedAt": snapshot["receivedAt"],
                "expiresAt": iso(now + policy["lifetimeMs"].as_i64().unwrap_or(60000)),
                "grossDiscount": money(&gross.unwrap()), "anchor": gate["features"]["anchor"],
                "originalAsk": decimal(q["bestAsk"].as_f64().unwrap_or(0.0)),
                "sourceSequence": row["sequence"], "sourceHash": row["hash"], "depthVerified": false});
            s["candidates"][&id] = candidate;
            s["cooldowns"][&id] = json!(now + policy["cooldownMs"].as_i64().unwrap_or(900000));
        } else if !prior.is_null() && now <= inst(&prior["expiresAt"])
            && amount(&decimal(q["bestAsk"].as_f64().unwrap_or(0.0))) - amount(&decimal(q["bestBid"].as_f64().unwrap_or(0.0)))
                <= amount(&anchor["maximumSpread"])
            && amount(&decimal(q["bestAsk"].as_f64().unwrap_or(0.0))) <= amount(&prior["originalAsk"])
            && amount(&prior["anchor"]) - amount(&decimal(q["bestBid"].as_f64().unwrap_or(0.0))) <= amount(&anchor["maximumBidDrop"]) {
            let mut c = prior.clone();
            c["latestAvailableAt"] = row["at"].clone();
            c["receivedAt"] = snapshot["receivedAt"].clone();
            s["candidates"][&id] = c;
        }
        let minute = inst(&snapshot["receivedAt"]).div_euclid(60000);
        if !points.iter().any(|p| p["minute"].as_i64() == Some(minute)) {
            points.push(json!({"minute": minute, "at": snapshot["receivedAt"],
                "bid": decimal(q["bestBid"].as_f64().unwrap_or(0.0))}));
        }
        let horizon = inst(&snapshot["receivedAt"]) - anchor["historyMs"].as_i64().unwrap_or(600000);
        s["history"][&id] = json!(points.into_iter().filter(|p| inst(&p["at"]) >= horizon).collect::<Vec<V>>());
    }
    Ok(())
}

pub fn project_diagnostic_focus(s: &V, lock: &V, observer_cohort: &V, now: Option<i64>, live_paper_dir: &Path) -> Result<V, String> {
    let policy = diagnostic_focus_policy();
    let now = now.unwrap_or_else(now_ms);
    let mut candidates: Vec<V> = if s["closed"].as_bool() == Some(true) { Vec::new() } else {
        s["candidates"].as_object().cloned().unwrap_or_default().values()
            .filter(|c| now >= inst(&c["latestAvailableAt"])
                && now - inst(&c["latestAvailableAt"]) <= policy["maximumAgeMs"].as_i64().unwrap_or(20000)
                && now <= inst(&c["expiresAt"]))
            .cloned().collect()
    };
    candidates.sort_by(|a, b| {
        let g = amount(&b["grossDiscount"]).cmp(&amount(&a["grossDiscount"]));
        if g != std::cmp::Ordering::Equal { return g; }
        // JS compares Number(...ms) — asc.
        inst(&a["availableAt"]).cmp(&inst(&b["availableAt"]))
    });
    let max_markets = policy["maximumMarkets"].as_u64().unwrap_or(2) as usize;
    candidates.truncate(max_markets);
    let body = json!({"schemaVersion": 1, "mode": "diagnostic_depth_reads_only", "policy": policy,
        "observerCohort": observer_cohort, "pid": lock["pid"], "lockId": lock["id"],
        "codeHash": diagnostic_focus_code_hash(live_paper_dir)?, "universeHash": s["universe"]["hash"],
        "at": iso(now), "sourceClosed": s["closed"], "price": s["price"], "book": s["book"],
        "candidates": candidates, "networkRequests": 0, "competitionOrders": 0, "executionAuthority": false});
    let mut out = body.clone();
    out["projectionHash"] = json!(hash(&body)?);
    Ok(out)
}

struct Tail {
    offset: u64,
    partial: Vec<u8>,
    ino: u64,
    birthtime_ms: f64,
}

fn stat_tail(file: &Path, bytes: &[u8]) -> Result<Tail, String> {
    let stat = std::fs::metadata(file).map_err(|e| e.to_string())?;
    use std::os::unix::fs::MetadataExt;
    let birth = stat.created().ok()
        .map(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0))
        .unwrap_or_else(|| stat.ctime() as f64 * 1000.0 + stat.ctime_nsec() as f64 / 1e6);
    let end = bytes.iter().rposition(|&b| b == 10).map(|i| i + 1).unwrap_or(0);
    Ok(Tail { offset: bytes.len() as u64, partial: bytes[end..].to_vec(), ino: stat.ino(), birthtime_ms: birth })
}

/// openDiagnosticFocus(runtimeDir, universe, {deferBootstrap}).
pub struct DiagnosticFocus {
    state: V,
    files: [(&'static str, PathBuf); 2],
    tails: [Tail; 2],
    bootstrap: Vec<(&'static str, V)>,
    initialized: bool,
}

impl DiagnosticFocus {
    /// Async `initialize({stopped})` — synchronous here; calls stopped() per row.
    pub fn initialize(&mut self, stopped: impl Fn() -> bool) -> Result<(), String> {
        if self.initialized { return Ok(()); }
        for i in 0..self.bootstrap.len() {
            need(!stopped(), "STOPPED")?;
            let (kind, row) = self.bootstrap[i].clone();
            advance_diagnostic_focus(&mut self.state, kind, &row)?;
            let _ = i; // `i%64===63 -> await setImmediate` is a no-op when synchronous.
        }
        need(!stopped(), "STOPPED")?;
        self.initialized = true;
        self.bootstrap.clear();
        Ok(())
    }

    fn sorted_apply(state: &mut V, mut list: Vec<(&'static str, V)>) -> Result<(), String> {
        list.sort_by(|a, b| {
            inst(&a.1["at"]).cmp(&inst(&b.1["at"]))
                .then((a.0 == "book").cmp(&(b.0 == "book")))
                .then(a.1["sequence"].as_i64().unwrap_or(0).cmp(&b.1["sequence"].as_i64().unwrap_or(0)))
        });
        for (k, row) in list { advance_diagnostic_focus(state, k, &row)?; }
        Ok(())
    }

    pub fn consume(&mut self) -> Result<(), String> {
        need(self.initialized, "BOOTSTRAP_PENDING")?;
        let mut list: Vec<(&'static str, V)> = Vec::new();
        for (idx, (kind, file)) in self.files.iter().enumerate() {
            use std::os::unix::fs::MetadataExt;
            let stat = std::fs::metadata(file).map_err(|e| e.to_string())?;
            let birth = stat.created().ok()
                .map(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0))
                .unwrap_or_else(|| stat.ctime() as f64 * 1000.0 + stat.ctime_nsec() as f64 / 1e6);
            let tail = &mut self.tails[idx];
            need(stat.len() >= tail.offset && stat.ino() == tail.ino && birth == tail.birthtime_ms, "SOURCE_REPLACED")?;
            let length = stat.len() - tail.offset;
            if length == 0 { continue; }
            need(length <= 4 * 1024 * 1024, "TAIL_BACKLOG")?;
            let mut bytes = vec![0u8; length as usize];
            let mut fd = std::fs::File::open(file).map_err(|e| e.to_string())?;
            fd.seek(std::io::SeekFrom::Start(tail.offset)).map_err(|e| e.to_string())?;
            let got = fd.read(&mut bytes).map_err(|e| e.to_string())?;
            need(got == length as usize, "TAIL_READ")?;
            tail.offset += length;
            let mut raw = tail.partial.clone();
            raw.extend_from_slice(&bytes);
            let end = raw.iter().rposition(|&b| b == 10).map(|i| i + 1).unwrap_or(0);
            tail.partial = raw[end..].to_vec();
            need(tail.partial.len() <= 1024 * 1024, "PARTIAL_SIZE")?;
            for line in String::from_utf8_lossy(&raw[..end]).split('\n').filter(|l| !l.is_empty()) {
                list.push((*kind, serde_json::from_str(line).map_err(|e| e.to_string())?));
            }
        }
        Self::sorted_apply(&mut self.state, list)
    }

    pub fn project(&self, lock: &V, cohort: &V, now: i64, live_paper_dir: &Path) -> Result<V, String> {
        need(self.initialized, "BOOTSTRAP_PENDING")?;
        project_diagnostic_focus(&self.state, lock, cohort, Some(now), live_paper_dir)
    }

    pub fn state(&self) -> &V { &self.state }
}

pub fn open_diagnostic_focus(runtime_dir: &Path, universe: &V, defer_bootstrap: bool) -> Result<DiagnosticFocus, String> {
    let state = create_diagnostic_focus_state(universe)?;
    let price = runtime_dir.join("market-prices.jsonl");
    let book = runtime_dir.join("public-books.jsonl");
    let mut bootstrap: Vec<(&'static str, V)> = Vec::new();
    let tails = {
        let mut v = Vec::new();
        for file in [&price, &book] {
            let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
            let end = bytes.iter().rposition(|&b| b == 10).map(|i| i + 1).unwrap_or(0);
            let kind = if file == &price { "price" } else { "book" };
            for line in String::from_utf8_lossy(&bytes[..end]).split('\n').filter(|l| !l.is_empty()) {
                bootstrap.push((kind, serde_json::from_str(line).map_err(|e| e.to_string())?));
            }
            v.push(stat_tail(file, &bytes)?);
        }
        <[Tail; 2]>::try_from(v).map_err(|_| "TAILS".to_string())?
    };
    // JS `sort` the bootstrap even when deferred.
    bootstrap.sort_by(|a, b| {
        inst(&a.1["at"]).cmp(&inst(&b.1["at"]))
            .then((a.0 == "book").cmp(&(b.0 == "book")))
            .then(a.1["sequence"].as_i64().unwrap_or(0).cmp(&b.1["sequence"].as_i64().unwrap_or(0)))
    });
    let mut focus = DiagnosticFocus { state, files: [("price", price), ("book", book)], tails, bootstrap, initialized: !defer_bootstrap };
    if focus.initialized {
        let boot = std::mem::take(&mut focus.bootstrap);
        DiagnosticFocus::sorted_apply(&mut focus.state, boot)?;
    }
    Ok(focus)
}

/// diagnosticDepthFocus(p, universe, {pid,lockId,now}) — Option (JS null on error).
pub fn diagnostic_depth_focus(p: &V, universe: &V, pid: i64, lock_id: &V, now: i64, live_paper_dir: &Path) -> Option<V> {
    (|| -> Result<V, String> {
        let mut body = p.clone();
        let projection_hash = body.as_object_mut().and_then(|m| m.shift_remove("projectionHash")).unwrap_or(V::Null);
        need(body["schemaVersion"] == json!(1) && body["mode"].as_str() == Some("diagnostic_depth_reads_only")
            && body["executionAuthority"] == json!(false) && body["networkRequests"] == json!(0)
            && body["competitionOrders"] == json!(0) && body["sourceClosed"] == json!(false)
            && body["pid"] == json!(pid) && body["lockId"] == *lock_id
            && hash(&body)? == projection_hash.as_str().unwrap_or("")
            && body["codeHash"].as_str().unwrap_or("") == diagnostic_focus_code_hash(live_paper_dir)?
            && body["universeHash"] == universe["hash"]
            && digest(&body["policy"]) == digest(&diagnostic_focus_policy()), "IDENTITY")?;
        let policy = diagnostic_focus_policy();
        let body_at = inst(&body["at"]);
        need(now >= body_at && now - body_at <= policy["maximumAgeMs"].as_i64().unwrap_or(20000)
            && body["candidates"].is_array() && body["candidates"].as_array().unwrap().len() <= 2, "AGE_OR_SIZE")?;
        let members: std::collections::HashMap<String, String> = universe["exchanges"].as_array().cloned().unwrap_or_default().iter()
            .filter_map(|x| Some((x["marketId"].as_str()?.to_string(), x["exchangeId"].as_str()?.to_string()))).collect();
        let mut seen = std::collections::HashSet::new();
        for c in body["candidates"].as_array().cloned().unwrap_or_default() {
            let market = c["marketId"].as_str().unwrap_or("");
            need(members.get(market).map(|s| s.as_str()) == c["exchangeId"].as_str()
                && !seen.contains(market) && c["depthVerified"] == json!(false)
                && inst(&c["expiresAt"]) == inst(&c["availableAt"]) + policy["lifetimeMs"].as_i64().unwrap_or(60000)
                && inst(&c["availableAt"]) <= inst(&c["latestAvailableAt"])
                && inst(&c["latestAvailableAt"]) <= body_at
                && now - inst(&c["latestAvailableAt"]) <= policy["maximumAgeMs"].as_i64().unwrap_or(20000)
                && now <= inst(&c["expiresAt"])
                && amount(&c["grossDiscount"]) >= amount(&policy["minimumGrossDiscount"])
                && c["sourceSequence"].as_f64().is_some_and(|x| x.fract() == 0.0 && x.abs() <= 9007199254740991.0)
                && c["sourceSequence"].as_i64().unwrap_or(0) > 0
                && c["sourceSequence"].as_i64().unwrap_or(0) <= body["price"]["sequence"].as_i64().unwrap_or(0)
                && c["sourceHash"].as_str().is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())),
                "CANDIDATE")?;
            seen.insert(market.to_string());
        }
        Ok(json!({"markets": body["candidates"].as_array().unwrap().iter()
            .map(|c| json!({"marketId": c["marketId"], "phase": "depth_required"})).collect::<Vec<V>>(),
            "sourceSequence": body["price"]["sequence"], "executionAuthority": false}))
    })().ok()
}
