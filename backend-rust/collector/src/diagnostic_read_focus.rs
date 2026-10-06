// live-paper/diagnostic-read-focus.mjs — diagnostic depth-read focus: replays
// market-prices + public-books journals to select markets for supplemental depth.

use baal_paper::fixed::{big_amount, money, TICK};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{canonical_hash, digest, js_int, js_truthy, sha256};
use crate::bid_anchor::bid_anchor_policy;
use crate::book_publication::create_book_publication_state;
use crate::breadth::validate_universe;
use crate::bulk_dip_diagnostic::diagnose_quoted_dip;
use crate::market_price_publication::validate_price_observation_pub;
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::{FileExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("DIAGNOSTIC_FOCUS_{code}")); }
    Ok(())
}
fn hash(x: &V) -> Result<String, String> { canonical_hash(x) }
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(x), V::Number(y)) => x.as_f64() == y.as_f64(),
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
fn to_key(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Null => "null".into(),
        V::Bool(b) => b.to_string(),
        V::Number(n) => {
            let f = n.as_f64().unwrap_or(f64::NAN);
            if f.is_finite() && f.fract() == 0.0 { (f as i64).to_string() } else { n.to_string() }
        }
        V::Array(a) => a.iter().map(|x| match x { V::Null => String::new(), other => to_key(other) })
            .collect::<Vec<_>>().join(","),
        V::Object(_) => "[object Object]".into(),
    }
}
fn js_num(v: &V) -> f64 {
    match v {
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::Bool(b) => if *b { 1.0 } else { 0.0 },
        V::Null => 0.0,
        V::String(s) => {
            let t = s.trim();
            if t.is_empty() { 0.0 } else { t.parse::<f64>().unwrap_or(f64::NAN) }
        }
        _ => f64::NAN,
    }
}
fn instant(v: &V) -> Result<i64, String> {
    baal_paper::fixed::instant(v.as_str().unwrap_or(""))
}
/// decimal(n) = money(BigInt(Math.round(n*1e6))).
fn decimal(n: &V) -> Result<String, String> {
    let x = js_num(n) * 1e6;
    if !x.is_finite() { return Err("Cannot convert NaN to a BigInt".into()); }
    Ok(money(&BigInt::from((x + 0.5).floor() as i128)))
}
/// amount(v) — decimal strings only, like paper/fixed.mjs.
fn amount(v: &V) -> Result<BigInt, String> {
    match v {
        V::String(s) => big_amount(s),
        _ => Err("Money must be a canonical decimal string with at most six places".into()),
    }
}
fn birth_ms(meta: &std::fs::Metadata) -> f64 {
    meta.created().ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0).unwrap_or(f64::NAN)
}

pub fn diagnostic_focus_policy() -> V {
    json!({"minimumGrossDiscount": "0.010000", "lifetimeMs": 60000,
        "cooldownMs": 900000, "maximumAgeMs": 20000, "maximumMarkets": 2})
}

/// diagnosticFocusCodeHash() — sha256 over the concatenated module sources.
pub fn diagnostic_focus_code_hash(live_paper_dir: &Path) -> Result<String, String> {
    let mut bytes = fs::read(live_paper_dir.join("diagnostic-read-focus.mjs")).map_err(|e| e.to_string())?;
    bytes.extend(fs::read(live_paper_dir.join("runner.mjs")).map_err(|e| e.to_string())?);
    Ok(sha256(&bytes))
}

pub fn create_diagnostic_focus_state(universe: &V) -> Result<V, String> {
    let stored_hash = universe.get("hash").cloned();
    let mut raw = universe.clone();
    if let Some(o) = raw.as_object_mut() { o.shift_remove("hash"); }
    let u = validate_universe(&raw, raw["tournamentId"].as_str().unwrap_or(""))?;
    need(stored_hash.is_none() || strict_eq(&stored_hash.unwrap_or(V::Null), &u["hash"]), "UNIVERSE_HASH")?;
    let book_scope = create_book_publication_state(&raw)?;
    Ok(json!({
        "universe": u,
        "bookScopeHash": book_scope["scopeHash"],
        "book": {"sequence": 0, "hash": "", "at": V::Null},
        "price": {"sequence": 0, "hash": "", "at": V::Null},
        "closed": true,
        "history": {},
        "candidates": {},
        "cooldowns": {},
        "barriers": {"global": V::Null, "markets": {}},
    }))
}

pub fn advance_diagnostic_focus(s: &mut V, kind: &str, row: &V) -> Result<(), String> {
    let now = instant(&row["at"])?;
    if kind == "book" {
        let mut body = row.clone();
        if let Some(o) = body.as_object_mut() { o.shift_remove("hash"); }
        let record_hash = row["hash"].clone();
        let book_at_ok = if js_truthy(s["book"].get("at")) {
            now >= instant(&s["book"]["at"])?
        } else { true };
        need(js_num(&body["sequence"]) == js_num(&s["book"]["sequence"]) + 1.0
            && strict_eq(&body["previousHash"], &s["book"]["hash"])
            && strict_eq(&json!(hash(&body)?), &record_hash)
            && strict_eq(&body["tournamentId"], &s["universe"]["tournamentId"])
            && strict_eq(&body["scopeHash"], &s["bookScopeHash"])
            && book_at_ok,
            "BOOK_CHAIN")?;
        need(row["kind"].as_str().is_some_and(|k| {
            ["book", "session_start", "session_stop", "invalidation"].contains(&k)
        }), "BOOK_KIND")?;
        s["book"] = json!({"sequence": row["sequence"], "hash": row["hash"], "at": row["at"]});
        if row["kind"].as_str() == Some("book") { return Ok(()); }
        let (id, id_present) = if row["kind"].as_str() == Some("invalidation") {
            if row.get("payload").is_none() || row["payload"].is_null() {
                return Err("TypeError: Cannot read properties of null or undefined".into());
            }
            (row["payload"]["marketId"].clone(), row["payload"].get("marketId").is_some())
        } else { (V::Null, true) };
        // JS id===null: true for the null branch, and for invalidation only when
        // payload.marketId is present-and-null (missing => undefined => BOUNDARY_SCOPE).
        let id_is_null = id_present && id.is_null();
        let in_scope = !id_is_null && s["universe"]["exchanges"].as_array()
            .is_some_and(|xs| xs.iter().any(|x| strict_eq(&x["marketId"], &id)));
        need(id_is_null || in_scope, "BOUNDARY_SCOPE")?;
        if id_is_null {
            s["history"] = json!({});
            s["candidates"] = json!({});
            s["barriers"]["global"] = row["at"].clone();
        } else {
            let key = to_key(&id);
            if let Some(o) = s["history"].as_object_mut() { o.shift_remove(&key); }
            if let Some(o) = s["candidates"].as_object_mut() { o.shift_remove(&key); }
            s["barriers"]["markets"][key.as_str()] = row["at"].clone();
        }
        if row["kind"].as_str() == Some("session_start") { s["closed"] = json!(false); }
        else if row["kind"].as_str() == Some("session_stop") { s["closed"] = json!(true); }
        return Ok(());
    }
    need(kind == "price", "SOURCE_KIND")?;
    let mut body = row.clone();
    if let Some(o) = body.as_object_mut() { o.shift_remove("hash"); }
    let record_hash = row["hash"].clone();
    let snapshot = body["event"]["snapshot"].clone();
    let price_at_ok = if js_truthy(s["price"].get("at")) {
        now >= instant(&s["price"]["at"])?
    } else { true };
    need(js_num(&body["sequence"]) == js_num(&s["price"]["sequence"]) + 1.0
        && strict_eq(&body["previous"], &s["price"]["hash"])
        && strict_eq(&json!(digest(&body)), &record_hash)
        && body.get("state").is_some_and(|x| x.is_null())
        && body["event"].get("type").is_some_and(|t| strict_eq(t, &json!("market_price_observation")))
        && price_at_ok,
        "PRICE_CHAIN")?;
    validate_price_observation_pub(&snapshot, &s["universe"])?;
    let received = instant(&snapshot["receivedAt"])?;
    need(received <= now, "PRICE_CLOCK")?;
    s["price"] = json!({"sequence": row["sequence"], "hash": row["hash"], "at": row["at"]});
    let mut by_exchange: std::collections::HashMap<String, V> = Default::default();
    for x in s["universe"]["exchanges"].as_array().cloned().unwrap_or_default() {
        by_exchange.insert(to_key(&x["exchangeId"]), x["marketId"].clone());
    }
    for id in snapshot["missingIds"].as_array().cloned().unwrap_or_default() {
        let market = by_exchange.get(&to_key(&id)).map(|m| to_key(m))
            .unwrap_or_else(|| "undefined".into());
        if let Some(o) = s["history"].as_object_mut() { o.shift_remove(&market); }
        if let Some(o) = s["candidates"].as_object_mut() { o.shift_remove(&market); }
        s["barriers"]["markets"][market.as_str()] = row["at"].clone();
    }
    if now - received > 20_000 {
        for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
            let mk = to_key(&q["marketId"]);
            if let Some(o) = s["history"].as_object_mut() { o.shift_remove(&mk); }
            if let Some(o) = s["candidates"].as_object_mut() { o.shift_remove(&mk); }
            s["barriers"]["markets"][mk.as_str()] = row["at"].clone();
        }
        return Ok(());
    }
    for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
        let id = q["marketId"].clone();
        let mk = to_key(&id);
        let clear = |at: &V| -> Result<bool, String> {
            if !js_truthy(Some(at)) { return Ok(true); }
            Ok(instant(&snapshot["requestStartedAt"])? > instant(at)?)
        };
        if js_truthy(s.get("closed"))
            || !(clear(&s["barriers"]["global"])? && clear(&s["barriers"]["markets"][&mk])?) {
            continue;
        }
        if !strict_eq(&q["option"], &json!("YES")) || js_truthy(q.get("crossed"))
            || q.get("bestBid").is_some_and(|b| b.is_null())
            || q.get("bestAsk").is_some_and(|b| b.is_null())
            || js_num(&q["bestBid"]) <= 0.0 || js_num(&q["bestAsk"]) >= 1.0
            || js_num(&q["bestAsk"]) <= js_num(&q["bestBid"]) {
            if let Some(o) = s["history"].as_object_mut() { o.shift_remove(&mk); }
            if let Some(o) = s["candidates"].as_object_mut() { o.shift_remove(&mk); }
            s["barriers"]["markets"][mk.as_str()] = row["at"].clone();
            continue;
        }
        let prior_history = s["history"].get(&mk).cloned().unwrap_or(V::Null);
        if js_truthy(Some(&prior_history)) && !prior_history.is_array() {
            return Err("TypeError: points.some is not a function".into());
        }
        let mut points: Vec<V> = if js_truthy(Some(&prior_history)) {
            prior_history.as_array().cloned().unwrap_or_default()
        } else { Vec::new() };
        let gate = diagnose_quoted_dip(&points, &q, snapshot["receivedAt"].as_str().unwrap_or(""), None)?;
        let prior = s["candidates"].get(&mk).cloned();
        let gross = if js_truthy(gate.get("features")) {
            Some(amount(&gate["features"]["netConcession"])? + BigInt::from(2 * TICK))
        } else { None };
        if let Some(o) = s["candidates"].as_object_mut() { o.shift_remove(&mk); }
        let failures_ok = if gross.is_some() {
            gate["failures"].as_array()
                .ok_or("TypeError: gate.failures.every is not a function")?
                .iter().all(|x| strict_eq(x, &json!("insufficient_concession")))
        } else { false };
        if gross.is_some()
            && gross.clone().unwrap() >= amount(&diagnostic_focus_policy()["minimumGrossDiscount"])?
            && failures_ok && (now as f64) >= js_num(&s["cooldowns"][&mk]) {
            s["candidates"][mk.as_str()] = json!({
                "marketId": id, "exchangeId": q["exchangeId"],
                "availableAt": row["at"], "latestAvailableAt": row["at"],
                "receivedAt": snapshot["receivedAt"],
                "expiresAt": ms_to_iso((now + 60_000) as f64),
                "grossDiscount": money(&gross.clone().unwrap()),
                "anchor": gate["features"]["anchor"],
                "originalAsk": decimal(&q["bestAsk"])?,
                "sourceSequence": row["sequence"], "sourceHash": row["hash"],
                "depthVerified": false,
            });
            s["cooldowns"][mk.as_str()] = json!(now + 900_000);
        } else if let Some(prior) = prior.filter(|p| js_truthy(Some(p))) {
            let policy = bid_anchor_policy();
            if now <= instant(&prior["expiresAt"])?
                && amount(&json!(decimal(&q["bestAsk"])?))? - amount(&json!(decimal(&q["bestBid"])?))?
                    <= amount(&policy["maximumSpread"])?
                && amount(&json!(decimal(&q["bestAsk"])?))? <= amount(&prior["originalAsk"])?
                && amount(&prior["anchor"])? - amount(&json!(decimal(&q["bestBid"])?))?
                    <= amount(&policy["maximumBidDrop"])? {
                let mut c = prior.clone();
                c["latestAvailableAt"] = row["at"].clone();
                c["receivedAt"] = snapshot["receivedAt"].clone();
                s["candidates"][mk.as_str()] = c;
            }
        }
        let minute = received.div_euclid(60_000);
        if !points.iter().any(|p| strict_eq(&p["minute"], &json!(minute))) {
            points.push(json!({"minute": minute, "at": snapshot["receivedAt"],
                "bid": decimal(&q["bestBid"])?}));
        }
        let history_ms = bid_anchor_policy()["historyMs"].as_i64().unwrap_or(0);
        let mut kept: Vec<V> = Vec::new();
        for p in &points {
            if instant(&p["at"])? >= received - history_ms { kept.push(p.clone()); }
        }
        s["history"][mk.as_str()] = json!(kept);
    }
    Ok(())
}

/// projectDiagnosticFocus(s, lock, observerCohort, now) — `live_paper_dir` resolves the
/// module sources for codeHash (import.meta.url in JS).
pub fn project_diagnostic_focus(s: &V, lock: &V, observer_cohort: &V, now: f64,
    live_paper_dir: &Path) -> Result<V, String> {
    let policy = diagnostic_focus_policy();
    let mut candidates: Vec<V> = Vec::new();
    if !js_truthy(s.get("closed")) {
        for c in s["candidates"].as_object().map(|o| o.values()).into_iter().flatten() {
            let latest = instant(&c["latestAvailableAt"])?;
            let expires = instant(&c["expiresAt"])?;
            if now >= latest as f64 && now - latest as f64 <= 20_000.0 && now <= expires as f64 {
                candidates.push(c.clone());
            }
        }
    }
    // The JS sort comparator can throw inside instant()/amount(); pre-evaluate to propagate.
    for c in &candidates {
        instant(&c["availableAt"])?;
        amount(&c["grossDiscount"])?;
    }
    candidates.sort_by(|a, b| {
        let d1 = (amount(&b["grossDiscount"]).unwrap_or_default()
            - amount(&a["grossDiscount"]).unwrap_or_default()).to_string().parse::<f64>().unwrap_or(0.0);
        if d1 != 0.0 && !d1.is_nan() { d1.partial_cmp(&0.0).unwrap() } else {
            (instant(&a["availableAt"]).unwrap_or(0) - instant(&b["availableAt"]).unwrap_or(0)).cmp(&0)
        }
    });
    candidates.truncate(2);
    let mut body = json!({
        "schemaVersion": 1, "mode": "diagnostic_depth_reads_only",
        "policy": policy, "observerCohort": observer_cohort,
        "pid": lock["pid"], "lockId": lock["id"],
        "codeHash": diagnostic_focus_code_hash(live_paper_dir)?,
        "universeHash": s["universe"]["hash"],
        "at": ms_to_iso(now),
        "sourceClosed": s["closed"],
        "price": s["price"], "book": s["book"],
        "candidates": candidates,
        "networkRequests": 0, "competitionOrders": 0, "executionAuthority": false,
    });
    let h = hash(&body)?;
    body["projectionHash"] = json!(h);
    Ok(body)
}

struct Tail { offset: u64, partial: Vec<u8>, ino: u64, birthtime_ms: f64 }

pub struct DiagnosticFocus {
    state: V,
    files: Vec<(String, PathBuf)>,
    tails: std::collections::HashMap<String, Tail>,
    bootstrap: Vec<(String, V)>,
    initialized: bool,
    live_paper_dir: PathBuf,
}

impl DiagnosticFocus {
    fn sort_list(list: &mut Vec<(String, V)>) -> Result<(), String> {
        // Precompute sort keys so instant() errors surface like the JS comparator.
        let keys: Vec<(i64, i64, f64)> = list.iter().map(|(k, row)| {
            let at = instant(&row["at"]).unwrap_or(i64::MIN);
            (at, if k == "book" { 1 } else { 0 }, js_num(&row["sequence"]))
        }).collect();
        let mut idx: Vec<usize> = (0..list.len()).collect();
        idx.sort_by(|&a, &b| keys[a].0.cmp(&keys[b].0)
            .then(keys[a].1.cmp(&keys[b].1))
            .then(keys[a].2.partial_cmp(&keys[b].2).unwrap_or(std::cmp::Ordering::Equal)));
        let sorted: Vec<(String, V)> = idx.into_iter().map(|i| list[i].clone()).collect();
        *list = sorted;
        Ok(())
    }
    fn apply(&mut self, mut list: Vec<(String, V)>) -> Result<(), String> {
        Self::sort_list(&mut list)?;
        for (k, row) in list { advance_diagnostic_focus(&mut self.state, &k, &row)?; }
        Ok(())
    }
    /// async initialize({stopped}) — synchronous; the awaits are scheduling only.
    pub fn initialize(&mut self, stopped: impl Fn() -> bool) -> Result<(), String> {
        if self.initialized { return Ok(()); }
        for item in self.bootstrap.clone() {
            need(!stopped(), "STOPPED")?;
            advance_diagnostic_focus(&mut self.state, &item.0, &item.1)?;
        }
        need(!stopped(), "STOPPED")?;
        self.initialized = true;
        self.bootstrap.clear();
        Ok(())
    }
    pub fn consume(&mut self) -> Result<(), String> {
        need(self.initialized, "BOOTSTRAP_PENDING")?;
        let mut list: Vec<(String, V)> = Vec::new();
        for (k, file) in &self.files {
            let tail = self.tails.get(k).ok_or("DIAGNOSTIC_FOCUS_STATE")?;
            let stat = file.metadata().map_err(|e| e.to_string())?;
            need(stat.len() >= tail.offset && stat.ino() == tail.ino
                && birth_ms(&stat) == tail.birthtime_ms, "SOURCE_REPLACED")?;
            let length = stat.len() - tail.offset;
            if length == 0 { continue; }
            need(length <= 4 * 1024 * 1024, "TAIL_BACKLOG")?;
            let mut bytes = vec![0u8; length as usize];
            let fd = fs::File::open(file).map_err(|e| e.to_string())?;
            fd.read_exact_at(&mut bytes, tail.offset).map_err(|_| "DIAGNOSTIC_FOCUS_TAIL_READ".to_string())?;
            let mut raw = tail.partial.clone();
            raw.extend_from_slice(&bytes);
            let end = raw.iter().rposition(|b| *b == b'\n').map(|i| i + 1).unwrap_or(0);
            // JS advances offset/partial before parsing; a parse failure leaves them advanced.
            let tail = self.tails.get_mut(k).unwrap();
            tail.offset += length;
            tail.partial = raw[end..].to_vec();
            need(tail.partial.len() <= 1024 * 1024, "PARTIAL_SIZE")?;
            let text = String::from_utf8_lossy(&raw[..end]).into_owned();
            for line in text.split('\n').filter(|l| !l.is_empty()) {
                let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
                list.push((k.clone(), row));
            }
        }
        self.apply(list)
    }
    pub fn project(&self, lock: &V, cohort: &V, now: f64) -> Result<V, String> {
        need(self.initialized, "BOOTSTRAP_PENDING")?;
        project_diagnostic_focus(&self.state, lock, cohort, now, &self.live_paper_dir)
    }
}

pub fn open_diagnostic_focus(runtime_dir: &Path, universe: &V, defer_bootstrap: bool,
    live_paper_dir: &Path) -> Result<DiagnosticFocus, String> {
    let state = create_diagnostic_focus_state(universe)?;
    let files = vec![
        ("price".to_string(), runtime_dir.join("market-prices.jsonl")),
        ("book".to_string(), runtime_dir.join("public-books.jsonl")),
    ];
    let mut tails: std::collections::HashMap<String, Tail> = Default::default();
    let mut bootstrap: Vec<(String, V)> = Vec::new();
    for (k, file) in &files {
        let bytes = fs::read(file).map_err(|e| e.to_string())?;
        let end = bytes.iter().rposition(|b| *b == b'\n').map(|i| i + 1).unwrap_or(0);
        let stat = file.metadata().map_err(|e| e.to_string())?;
        tails.insert(k.clone(), Tail {
            offset: bytes.len() as u64,
            partial: bytes[end..].to_vec(),
            ino: stat.ino(),
            birthtime_ms: birth_ms(&stat),
        });
        let text = String::from_utf8_lossy(&bytes[..end]).into_owned();
        for line in text.split('\n').filter(|l| !l.is_empty()) {
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            bootstrap.push((k.clone(), row));
        }
    }
    let mut focus = DiagnosticFocus {
        state, files, tails, bootstrap,
        initialized: !defer_bootstrap,
        live_paper_dir: live_paper_dir.to_path_buf(),
    };
    if focus.initialized {
        let bootstrap = std::mem::take(&mut focus.bootstrap);
        focus.apply(bootstrap)?;
    } else {
        DiagnosticFocus::sort_list(&mut focus.bootstrap)?;
    }
    Ok(focus)
}

/// diagnosticDepthFocus(p, universe, {pid, lockId, now}) — try/catch → null.
pub fn diagnostic_depth_focus(p: &V, universe: &V, pid: &V, lock_id: &V, now: f64,
    live_paper_dir: &Path) -> Option<V> {
    (|| -> Result<V, String> {
        let mut body = p.clone();
        if let Some(o) = body.as_object_mut() { o.shift_remove("projectionHash"); }
        let policy = diagnostic_focus_policy();
        need(js_int(&body["schemaVersion"]) == Some(1)
            && body["mode"].as_str() == Some("diagnostic_depth_reads_only")
            && strict_eq(&body["executionAuthority"], &V::Bool(false))
            && strict_eq(&body["networkRequests"], &json!(0))
            && strict_eq(&body["competitionOrders"], &json!(0))
            && strict_eq(&body["sourceClosed"], &V::Bool(false))
            && strict_eq(&body["pid"], pid) && strict_eq(&body["lockId"], lock_id)
            && strict_eq(&json!(hash(&body)?), &p["projectionHash"])
            && strict_eq(&body["codeHash"], &json!(diagnostic_focus_code_hash(live_paper_dir)?))
            && strict_eq(&body["universeHash"], &universe["hash"])
            && digest(&body["policy"]) == digest(&policy), "IDENTITY")?;
        let at = instant(&body["at"])?;
        need(now >= at as f64 && now - at as f64 <= 20_000.0
            && body["candidates"].as_array().is_some_and(|a| a.len() <= 2), "AGE_OR_SIZE")?;
        let mut members: std::collections::HashMap<String, V> = Default::default();
        for x in universe["exchanges"].as_array().cloned().unwrap_or_default() {
            members.insert(to_key(&x["marketId"]), x["exchangeId"].clone());
        }
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for c in body["candidates"].as_array().cloned().unwrap_or_default() {
            let mid_key = to_key(&c["marketId"]);
            need(members.get(&mid_key).is_some_and(|e| strict_eq(e, &c["exchangeId"]))
                && !seen.contains(&mid_key)
                && strict_eq(&c["depthVerified"], &V::Bool(false))
                && instant(&c["expiresAt"])? == instant(&c["availableAt"])? + 60_000
                && instant(&c["availableAt"])? <= instant(&c["latestAvailableAt"])?
                && instant(&c["latestAvailableAt"])? <= at
                && now - instant(&c["latestAvailableAt"])? as f64 <= 20_000.0
                && now <= instant(&c["expiresAt"])? as f64
                && amount(&c["grossDiscount"])? >= amount(&policy["minimumGrossDiscount"])?
                && js_int(&c["sourceSequence"]).is_some_and(|s| s > 0)
                && js_num(&c["sourceSequence"]) <= js_num(&body["price"]["sequence"])
                && c["sourceHash"].as_str().is_some_and(|h| {
                    h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                }), "CANDIDATE")?;
            seen.insert(mid_key);
        }
        Ok(json!({
            "markets": body["candidates"].as_array().cloned().unwrap_or_default().iter()
                .map(|c| json!({"marketId": c["marketId"], "phase": "depth_required"}))
                .collect::<Vec<V>>(),
            "sourceSequence": body["price"]["sequence"],
            "executionAuthority": false,
        }))
    })().ok()
}
