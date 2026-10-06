// live-paper/trade-tape.mjs — anonymous official trade tape collector.
// Reads the public /trades endpoint, dedupes by fingerprint, journals each page.

use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::{instant_ns, ms_to_iso, parse_iso_ms};
use baal_service_runtime::history::{encode_uri_component, TOURNAMENT_ID};
use baal_service_runtime::journal::{journal_lines, Journal, JournalReadOptions};
use baal_service_runtime::json_text::{digest, js_int, js_truthy};
use crate::quote_projection_recovery::validate_quote_recovery_request;
use serde_json::{json, Map, Value as V};
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

fn check(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(code.to_string()); }
    Ok(())
}
fn parse(x: &V) -> f64 {
    parse_iso_ms(x.as_str().unwrap_or("")).unwrap_or(f64::NAN)
}
fn is_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 { return false; }
    for (i, &c) in b.iter().enumerate() {
        if i == 8 || i == 13 || i == 18 || i == 23 {
            if c != b'-' { return false; }
        } else if !c.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty() && s.len() <= 80
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
    })
}
fn is_cursor(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty() && s.len() <= 200
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~".contains(&b))
    })
}
fn clock(x: &V) -> bool {
    x.as_str().is_some_and(|s| s.len() <= 40 && parse_iso_ms(s).is_some())
}
/// JS String(x) on scalar JSON values.
fn js_str(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => {
            let mut b = ryu_js::Buffer::new();
            b.format_finite(n.as_f64().unwrap_or(f64::NAN)).to_string()
        }
        V::Bool(b) => if *b { "true".into() } else { "false".into() },
        V::Null => "null".into(),
        V::Array(a) => a.iter().map(|x| match x { V::Null => String::new(), other => js_str(other) })
            .collect::<Vec<_>>().join(","),
        V::Object(_) => "[object Object]".into(),
    }
}
fn trade_key(exchange_id: &str, t: &V) -> Result<String, String> {
    let ns = instant_ns(t["createdAt"].as_str().unwrap_or("")).map_err(|e| e.0)?;
    let executed = match t.get("executedSpace") {
        Some(v) if !v.is_null() => v.clone(),
        _ => t["side"].clone(),
    };
    Ok(digest(&json!([exchange_id, t["id"], ns.to_string(), executed])))
}
fn trade_fingerprint(t: &V) -> Result<String, String> {
    let ns = instant_ns(t["createdAt"].as_str().unwrap_or("")).map_err(|e| e.0)?;
    // {...t, key: undefined, createdAt: instantNs(...).toString()} — undefined drops the key.
    let mut o: Map<String, V> = t.as_object().cloned().unwrap_or_default();
    o.shift_remove("key");
    o.insert("createdAt".into(), json!(ns.to_string()));
    Ok(digest(&V::Object(o)))
}
pub fn trade_tape_endpoint(spec: &V) -> Result<String, String> {
    check(id(&spec["exchangeId"]) && id(&spec["marketId"])
        && spec["tournamentId"].as_str().is_some_and(is_uuid), "TRADE_TAPE_INVALID")?;
    check(clock(&spec["from"]) && clock(&spec["to"])
        && parse(&spec["from"]) < parse(&spec["to"]), "TRADE_TAPE_INVALID")?;
    check(spec.get("cursor").is_some_and(|c| c.is_null() || is_cursor(c)), "TRADE_TAPE_INVALID")?;
    let suffix = match spec.get("cursor") {
        Some(c) if c.is_null() => String::new(),
        _ => format!("&cursor={}", encode_uri_component(spec["cursor"].as_str().unwrap_or(""))),
    };
    Ok(format!(
        "/exchanges/{}/trades?tournamentId={}&from={}&to={}&limit=200{}",
        spec["exchangeId"].as_str().unwrap_or(""),
        spec["tournamentId"].as_str().unwrap_or(""),
        encode_uri_component(spec["from"].as_str().unwrap_or("")),
        encode_uri_component(spec["to"].as_str().unwrap_or("")),
        suffix
    ))
}
pub fn normalize_trade_tape(response: &V, spec: &V) -> Result<V, String> {
    check(response["endpoint"].as_str() == Some(trade_tape_endpoint(spec)?.as_str()), "TRADE_TAPE_INVALID")?;
    check(response["status"].as_i64() == Some(200), "TRADE_TAPE_INVALID")?;
    let d = &response["data"];
    let started = parse(&response["requestStartedAt"]);
    let received = parse(&response["receivedAt"]);
    let to = parse(&spec["to"]);
    check(clock(&response["requestStartedAt"]) && started >= to
        && clock(&response["receivedAt"]) && received >= started && received - started <= 20_000.0,
        "TRADE_TAPE_INVALID")?;
    check(d["exchangeId"].as_str() == spec["exchangeId"].as_str()
        && d["marketId"].as_str() == spec["marketId"].as_str()
        && d["tournamentId"].as_str() == spec["tournamentId"].as_str(), "TRADE_TAPE_INVALID")?;
    check(d["market"]["externalMarketId"].as_str() == spec["marketId"].as_str(), "TRADE_TAPE_INVALID")?;
    check(d["pagination"]["limit"].as_i64() == Some(200)
        && d["pagination"]["hasMore"].as_bool().is_some()
        && d["pagination"].get("nextCursor").is_some_and(|c| c.is_null() || is_cursor(c)),
        "TRADE_TAPE_INVALID")?;
    check(d["coverage"]["complete"].as_bool().is_some()
        && (d["coverage"].get("projectedThroughSequence").is_some_and(|x| x.is_null())
            || (js_int(&d["coverage"]["projectedThroughSequence"]).is_some()
                && js_int(&d["coverage"]["projectedThroughSequence"]).unwrap_or(0) >= 0)),
        "TRADE_TAPE_INVALID")?;
    let data = d["data"].as_array().cloned().unwrap_or_default();
    check(data.len() <= 200, "TRADE_TAPE_INVALID")?;
    let from_ns = (parse(&spec["from"]) as i128) * 1_000_000;
    let to_ns = (to as i128) * 1_000_000;
    let mut previous: Option<i128> = None;
    let mut keys: Vec<String> = Vec::new();
    let mut trades: Vec<V> = Vec::new();
    for t in &data {
        check(clock(&t["createdAt"]) && js_int(&t["quantity"]).is_some_and(|q| q > 0)
            && (t.get("price").is_some_and(|p| p.is_null())
                || t["price"].as_str().is_some_and(|p| p.parse::<f64>().is_ok()))
            && id(&t["side"]) && id(&t["liquidity"]) && id(&t["id"]), "TRADE_TAPE_INVALID")?;
        check(t.get("executedSpace").is_none() || t["executedSpace"].is_null() || id(&t["executedSpace"]),
            "TRADE_TAPE_INVALID")?;
        let at = instant_ns(t["createdAt"].as_str().unwrap_or("")).map_err(|e| e.0)?;
        check(at >= from_ns && at < to_ns && previous.is_none_or(|p| at <= p), "TRADE_TAPE_INVALID")?;
        previous = Some(at);
        let key = trade_key(d["exchangeId"].as_str().unwrap_or(""), t)?;
        check(!keys.contains(&key), "TRADE_TAPE_INVALID")?;
        keys.push(key.clone());
        let mut t2 = t.clone();
        t2["key"] = json!(key);
        trades.push(t2);
    }
    Ok(json!({
        "schemaVersion": 1,
        "source": "anonymous_trade_tape",
        "exchangeId": d["exchangeId"],
        "marketId": spec["marketId"],
        "tournamentId": spec["tournamentId"],
        "from": spec["from"],
        "to": spec["to"],
        "requestStartedAt": response["requestStartedAt"],
        "receivedAt": response["receivedAt"],
        "requestCursor": spec["cursor"],
        "pagination": d["pagination"],
        "coverage": d["coverage"],
        "trades": trades,
        "responseHash": digest(d),
        "firstAvailableAt": trades.first().map(|t| t["createdAt"].clone()).unwrap_or(V::Null),
        "completeTapeProven": V::Null,
        "counterfactualFillsProven": V::Null,
    }))
}
// selectAuxiliaryReads(historyBudget, cycle) — maker read budget split between tape and history.
pub fn select_auxiliary_reads(history_budget: &V, cycle: &V) -> Result<V, String> {
    check(js_int(history_budget).is_some_and(|b| (0..=2).contains(&b))
        && js_int(cycle).is_some_and(|c| c >= 0), "TRADE_TAPE_INVALID")?;
    let budget = js_int(history_budget).unwrap_or(0);
    let cyc = js_int(cycle).unwrap_or(0);
    let tape = if budget > 0 && cyc % 2 == 0 { 1 } else { 0 };
    Ok(json!({
        "schemaVersion": 1,
        "cycle": cyc,
        "tapeReads": tape,
        "historyReads": budget,
        "totalReads": budget + tape,
        "policy": "one_maker_read_per_cycle_paper_history_two_cycles",
    }))
}

fn default_state(scope_hash: &str) -> V {
    json!({"schemaVersion": 1, "seenIdentityVersion": 2, "scopeHash": scope_hash,
        "pages": 0, "tradesCaptured": 0, "duplicates": 0, "markets": {}, "lastAt": V::Null})
}
fn prepare_journal(file: &Path) -> Result<V, String> {
    let exists = file.exists();
    let size = if exists { file.metadata().map_err(|e| e.to_string())?.len() } else { 0 };
    let mut trimmed = false;
    if exists && size > 0 {
        let raw = fs::read(file).map_err(|e| e.to_string())?;
        if !raw.ends_with(b"\n") {
            let last_nl = raw.iter().rposition(|b| *b == b'\n');
            fs::write(file, &raw[..last_nl.map(|i| i + 1).unwrap_or(0)]).map_err(|e| e.to_string())?;
            trimmed = true;
        }
    }
    let partial = if exists && size > 0 {
        let raw = fs::read(file).map_err(|e| e.to_string())?;
        match raw.iter().rposition(|b| *b == b'\n') {
            Some(i) => size - i as u64 - 1,
            None => 0,
        }
    } else { 0 };
    Ok(json!({"offset": size, "partial": partial, "trimmed": trimmed}))
}
fn prepare(runtime_dir: &Path, universe: &V, operable: bool) -> Result<(Journal, V, V), String> {
    check(universe["schemaVersion"].as_i64() == Some(2)
        && universe["tournamentId"].as_str() == Some(TOURNAMENT_ID)
        && universe["exchanges"].as_array().is_some_and(|xs| !xs.is_empty() && xs.iter().all(|x| {
            js_truthy(Some(x)) && id(&x["exchangeId"]) && id(&x["marketId"])
        })), "TRADE_TAPE_INVALID")?;
    let scope_hash = digest(&json!({"tournamentId": universe["tournamentId"], "exchanges": universe["exchanges"]}));
    let file = runtime_dir.join("anonymous-trades.jsonl");
    let attempt_file = runtime_dir.join("trade-tape-attempts.json");
    let mut journal = Journal::open(&file)?;
    let mut state = journal.last.as_ref().and_then(|l| l.get("state")).cloned()
        .filter(|v| js_truthy(Some(v))).unwrap_or_else(|| default_state(&scope_hash));
    check(state["schemaVersion"].as_i64() == Some(1)
        && state["scopeHash"].as_str() == Some(scope_hash.as_str()), "TRADE_TAPE_RESTART_SCOPE_INVALID")?;
    if js_int(&state["seenIdentityVersion"]) != Some(2) {
        check(state.get("seenIdentityVersion").is_none(), "TRADE_TAPE_IDENTITY_VERSION")?;
        let mut next = state.clone();
        if let Some(mk) = next["markets"].as_object_mut() {
            for (_, m) in mk.iter_mut() { m["seen"] = json!({}); }
        }
        let mut resolved: std::collections::BTreeSet<String> = Default::default();
        for line in journal_lines(&file, &JournalReadOptions::default())? {
            let parsed: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            let e = &parsed["event"];
            let eid = js_str(&e["exchangeId"]);
            if next["markets"].get(&eid).is_none() { continue; }
            let old = &state["markets"][&eid];
            for t in e["trades"].as_array().cloned().unwrap_or_default() {
                let key = js_str(&t["key"]);
                let known = old["seen"].get(&key);
                if known.is_none() || !js_truthy(known) { continue; }
                check(known.and_then(|k| k["hash"].as_str()) == Some(digest(&t).as_str()),
                    "TRADE_TAPE_LEGACY_CONFLICT")?;
                resolved.insert(format!("{eid}:{key}"));
                let k2 = trade_key(&eid, &t)?;
                let fp = trade_fingerprint(&t)?;
                next["markets"][&eid]["seen"][&k2] = json!({"at": t["createdAt"], "hash": fp});
            }
        }
        if let Some(mk) = state["markets"].as_object() {
            for (eid, m) in mk {
                if let Some(seen) = m["seen"].as_object() {
                    for key in seen.keys() {
                        check(resolved.contains(&format!("{eid}:{key}")), "TRADE_TAPE_LEGACY_UNRESOLVED")?;
                    }
                }
            }
        }
        next["seenIdentityVersion"] = json!(2);
        if operable && journal.sequence > 0 {
            journal.commit(&next, &json!({"type": "trade_tape_identity_upgrade"}))?;
        }
        state = next;
    }
    let _prepared_attempt = prepare_journal(&attempt_file)?;
    let attempt_exists = attempt_file.exists();
    let mut attempts = if attempt_exists {
        serde_json::from_slice::<V>(&fs::read(&attempt_file).map_err(|e| e.to_string())?)
            .unwrap_or_else(|_| default_attempts(&scope_hash))
    } else { default_attempts(&scope_hash) };
    if !js_truthy(Some(&attempts)) { attempts = default_attempts(&scope_hash); }
    check(attempts["schemaVersion"].as_i64() == Some(1)
        && attempts["scopeHash"].as_str() == Some(scope_hash.as_str())
        && js_truthy(Some(&attempts["markets"]))
        && attempts["markets"].as_object().is_some_and(|mk| mk.iter().all(|(eid, at)| {
            universe["exchanges"].as_array().is_some_and(|xs| {
                xs.iter().any(|x| x["exchangeId"].as_str() == Some(eid.as_str()))
            }) && clock(at)
        }))
        && (attempts.get("attemptsReserved").is_none()
            || js_int(&attempts["attemptsReserved"]).is_some_and(|n| n >= 0)),
        "TRADE_TAPE_ATTEMPT_STATE_INVALID")?;
    Ok((journal, state, attempts))
}
fn default_attempts(scope_hash: &str) -> V {
    json!({"schemaVersion": 1, "scopeHash": scope_hash, "markets": {}})
}

pub struct TradeTape {
    file: PathBuf,
    attempt_file: PathBuf,
    scope: V,
    scope_hash: String,
    journal: Journal,
    state: V,
    attempts: V,
    now: Box<dyn Fn() -> f64>,
    can_operate: Box<dyn Fn() -> bool>,
    on_page: Box<dyn FnMut(&V)>,
    pending: Option<V>,
}

impl TradeTape {
    fn stats(&self) -> V {
        let mut markets: Vec<V> = Vec::new();
        if let Some(mk) = self.state["markets"].as_object() {
            for (eid, m) in mk {
                let mut spent: Vec<V> = Vec::new();
                let mut remaining: Vec<V> = Vec::new();
                for c in m["window"]["cursors"].as_array().cloned().unwrap_or_default() {
                    if c.is_null() { spent.push(c); } else { remaining.push(c); }
                }
                markets.push(json!({
                    "exchangeId": eid, "marketId": m["marketId"],
                    "pages": m["pages"], "tradesCaptured": m["tradesCaptured"],
                    "seen": m["seen"].as_object().map(|s| s.len()).unwrap_or(0),
                    "latestTradeAt": m["latestTradeAt"], "window": m["window"],
                    "windowCursorsSpent": spent, "windowCursorsRemaining": remaining,
                    "lastReceiptAt": m["lastReceiptAt"],
                    "projectedThroughSequence": m["projectedThroughSequence"],
                    "reportedCoverageComplete": m["reportedCoverageComplete"],
                    "recoveryPending": js_truthy(m.get("recovery")),
                    "recoveryCompleted": m["recoveryCompleted"],
                    "lastRequestWasRecovery": m["lastRequestWasRecovery"],
                    "completedTo": m["completedTo"],
                }));
            }
        }
        json!({
            "schemaVersion": 1, "scopeHash": self.scope_hash,
            "pages": self.state["pages"], "tradesCaptured": self.state["tradesCaptured"],
            "duplicates": self.state["duplicates"], "markets": markets,
            "lastAt": self.state["lastAt"],
            "networkRequests": 0, "competitionOrders": 0,
            "journalSequence": self.journal.sequence, "journalHash": self.journal.previous,
            "executionAuthority": false,
        })
    }
    pub fn status(&self) -> V { self.stats() }
    pub fn authorizes(&self, spec: &V) -> bool {
        match &self.pending {
            None => false,
            Some(p) => {
                (self.can_operate)()
                    && spec["method"].as_str() == Some("GET")
                    && spec["endpoint"] == p["endpoint"]
                    && spec.get("body").is_none() && spec.get("bodyJson").is_none()
            }
        }
    }
    pub fn collect<F2>(&mut self, dispatch: &mut F2, market_ids: &[V],
        priority_market_ids: &[V], recovery_windows: &[V]) -> Result<V, String>
        where F2: FnMut(&V) -> Result<V, String> {
        check(self.pending.is_none(), "TRADE_TAPE_CONCURRENT_REQUEST")?;
        if !(self.can_operate)() { return Ok(self.stats()); }
        let scope = self.scope.clone();
        let mut uniq: Vec<V> = Vec::new();
        for m in market_ids { if !uniq.contains(m) { uniq.push(m.clone()); } }
        let mut candidates: Vec<V> = Vec::new();
        if let Some(xs) = scope["exchanges"].as_array() {
            for m in &uniq {
                if let Some(x) = xs.iter().find(|x| x["marketId"] == *m) { candidates.push(x.clone()); }
            }
        }
        check(!candidates.is_empty(), "TRADE_TAPE_NO_SCOPE")?;
        check(recovery_windows.len() <= 4, "TRADE_TAPE_RECOVERY_LIMIT")?;
        for request in recovery_windows {
            validate_quote_recovery_request(request, &scope, (self.now)() as i64)?;
            check(candidates.iter().any(|x| x["exchangeId"] == request["exchangeId"]
                && x["marketId"] == request["marketId"]), "TRADE_TAPE_RECOVERY_SCOPE")?;
        }
        let last_attempt = |x: &V| -> f64 {
            let eid = x["exchangeId"].as_str().unwrap_or("");
            let a = &self.attempts["markets"][eid];
            let v = if js_truthy(Some(a)) { a.clone() } else {
                let l = &self.state["markets"][eid]["lastReceiptAt"];
                if js_truthy(Some(l)) { l.clone() } else { json!("1970-01-01T00:00:00.000Z") }
            };
            parse(&v)
        };
        candidates.sort_by(|a, b| {
            last_attempt(a).partial_cmp(&last_attempt(b)).unwrap_or(Ordering::Equal)
                .then(js_str(&a["marketId"]).parse::<f64>().unwrap_or(f64::NAN)
                    .partial_cmp(&js_str(&b["marketId"]).parse::<f64>().unwrap_or(f64::NAN))
                    .unwrap_or(Ordering::Equal))
        });
        check(priority_market_ids.iter().all(|id| candidates.iter().any(|c| c["marketId"] == *id)),
            "TRADE_TAPE_PRIORITY_SCOPE")?;
        let ordinal = self.attempts.get("attemptsReserved").and_then(|v| v.as_i64()).unwrap_or(0);
        let focused: Vec<V> = candidates.iter()
            .filter(|x| priority_market_ids.iter().any(|p| *p == x["marketId"])).cloned().collect();
        let discovery: Vec<V> = candidates.iter()
            .filter(|x| !priority_market_ids.iter().any(|p| *p == x["marketId"])).cloned().collect();
        let chosen = (if !focused.is_empty() {
            if ordinal % 4 == 3 && !discovery.is_empty() { &discovery } else { &focused }
        } else { &candidates })[0].clone();
        let eid = chosen["exchangeId"].as_str().unwrap_or("").to_string();
        let prior = self.state["markets"][&eid].clone();
        let now_ms = (self.now)();
        let to = ms_to_iso((now_ms / 1000.0).floor() * 1000.0);
        let mut recovery: Option<V> = prior.get("recovery").and_then(|r| r.get("request"))
            .cloned().filter(|v| js_truthy(Some(v)));
        if recovery.is_none() {
            let completed = prior.get("recoveryCompleted").and_then(|r| r.as_array()).cloned().unwrap_or_default();
            recovery = recovery_windows.iter().find(|x| {
                x["exchangeId"] == chosen["exchangeId"] && !completed.contains(&x["key"])
            }).cloned();
        }
        if let Some(r) = &recovery {
            validate_quote_recovery_request(r, &scope, (self.now)() as i64)?;
        }
        let use_recovery = recovery.is_some()
            && prior.get("lastRequestWasRecovery") != Some(&V::Bool(true));
        let window: V = if use_recovery {
            prior.get("recovery").and_then(|r| r.get("window")).cloned()
                .filter(|v| js_truthy(Some(v))).unwrap_or_else(|| json!({
                    "from": recovery.as_ref().map(|r| r["from"].clone()).unwrap_or(V::Null),
                    "to": recovery.as_ref().map(|r| r["to"].clone()).unwrap_or(V::Null),
                    "cursor": V::Null, "cursors": [], "coverageCheckpoint": V::Null,
                }))
        } else {
            prior.get("window").cloned().filter(|v| js_truthy(Some(v))).unwrap_or_else(|| {
                let from_ms = if js_truthy(prior.get("completedTo")) {
                    (parse(&prior["completedTo"]) - 60_000.0).max(0.0)
                } else { parse(&json!(to.clone())) - 1_800_000.0 };
                json!({"from": ms_to_iso(from_ms), "to": to.clone(), "cursor": V::Null, "cursors": []})
            })
        };
        let mut spec = chosen.clone();
        spec["tournamentId"] = scope["tournamentId"].clone();
        spec["from"] = window["from"].clone();
        spec["to"] = window["to"].clone();
        spec["cursor"] = window["cursor"].clone();
        let endpoint = trade_tape_endpoint(&spec)?;
        let mut pending_spec = spec.clone();
        pending_spec["endpoint"] = json!(endpoint.clone());
        self.pending = Some(pending_spec);
        let outcome = self.collect_inner(dispatch, &spec, &window, &chosen,
            recovery, use_recovery, ordinal, &endpoint);
        self.pending = None;
        outcome
    }
    fn collect_inner<F2>(&mut self, dispatch: &mut F2, spec: &V, window: &V, chosen: &V,
        recovery: Option<V>, use_recovery: bool, ordinal: i64, endpoint: &str) -> Result<V, String>
        where F2: FnMut(&V) -> Result<V, String> {
        let eid = chosen["exchangeId"].as_str().unwrap_or("").to_string();
        let now_ms = (self.now)();
        let mut next_attempts = self.attempts.clone();
        next_attempts["attemptsReserved"] = json!(ordinal + 1);
        {
            let mut markets = next_attempts["markets"].as_object().cloned().unwrap_or_default();
            markets.insert(eid.clone(), json!(ms_to_iso(now_ms)));
            next_attempts["markets"] = V::Object(markets);
        }
        atomic_json_canonical(&self.attempt_file, &next_attempts).map_err(|e| e.to_string())?;
        self.attempts = next_attempts;
        let response = dispatch(&json!({"method": "GET", "endpoint": endpoint}))?;
        let page = normalize_trade_tape(&response, spec)?;
        if !(self.can_operate)() { return Ok(self.stats()); }
        let mut next = self.state.clone();
        let mut m = next["markets"].get(&eid).cloned().filter(|v| js_truthy(Some(v)))
            .unwrap_or_else(|| {
                let mut base = chosen.clone();
                base["pages"] = json!(0);
                base["tradesCaptured"] = json!(0);
                base["seen"] = json!({});
                base["latestTradeAt"] = V::Null;
                base
            });
        check(page["pagination"].get("nextCursor").is_some_and(|c| c.is_null())
            || !window["cursors"].as_array().is_some_and(|cs| {
                cs.iter().any(|c| *c == page["pagination"]["nextCursor"])
            }), "TRADE_TAPE_CURSOR_CYCLE")?;
        // Retain all identities needed by an unfinished page window plus one-minute overlap.
        let mw_from = m["window"].get("from").cloned().filter(|v| js_truthy(Some(v)))
            .unwrap_or_else(|| window["from"].clone());
        let mr_from = m["recovery"].get("window").and_then(|w| w.get("from")).cloned()
            .filter(|v| js_truthy(Some(v))).unwrap_or_else(|| window["from"].clone());
        let retained_from = {
            let a = parse(&window["from"]);
            let b = parse(&mw_from);
            let c = parse(&mr_from);
            if a.is_nan() || b.is_nan() || c.is_nan() { f64::NAN } else { a.min(b).min(c) }
        };
        if let Some(seen) = m["seen"].as_object_mut() {
            let drop: Vec<String> = seen.iter()
                .filter(|(_, v)| parse(&v["at"]) < retained_from).map(|(k, _)| k.clone()).collect();
            for k in drop { seen.shift_remove(&k); }
        }
        let mut novel: Vec<V> = Vec::new();
        for t in page["trades"].as_array().cloned().unwrap_or_default() {
            let fingerprint = trade_fingerprint(&t)?;
            let key = js_str(&t["key"]);
            let known = m["seen"].get(&key).cloned().filter(|v| js_truthy(Some(v)));
            if let Some(known) = known {
                check(known["hash"].as_str() == Some(fingerprint.as_str()), "TRADE_TAPE_IDENTITY_CONFLICT")?;
                next["duplicates"] = json!(js_num(&next["duplicates"]) + 1.0);
            } else {
                m["seen"][&key] = json!({"at": t["createdAt"], "hash": fingerprint});
                novel.push(t.clone());
                m["tradesCaptured"] = json!(js_num(&m["tradesCaptured"]) + 1.0);
                next["tradesCaptured"] = json!(js_num(&next["tradesCaptured"]) + 1.0);
                let latest = m["latestTradeAt"].clone();
                if !js_truthy(Some(&latest))
                    || instant_ns(t["createdAt"].as_str().unwrap_or("")).map_err(|e| e.0)?
                        > instant_ns(latest.as_str().unwrap_or("")).map_err(|e| e.0)? {
                    m["latestTradeAt"] = t["createdAt"].clone();
                }
            }
        }
        check(m["seen"].as_object().map(|s| s.len()).unwrap_or(0) <= 100_000, "TRADE_TAPE_WINDOW_CAPACITY")?;
        m["pages"] = json!(js_num(&m["pages"]) + 1.0);
        m["lastReceiptAt"] = page["receivedAt"].clone();
        m["projectedThroughSequence"] = page["coverage"]["projectedThroughSequence"].clone();
        m["reportedCoverageComplete"] = page["coverage"]["complete"].clone();
        // A later page or time window cannot establish a reported window's missing coverage.
        let coverage_base = match window.get("coverageComplete") {
            Some(c) if !c.is_null() => c.clone(),
            _ => json!(window.get("cursor").is_some_and(|c| c.is_null())),
        };
        let all_complete = js_truthy(Some(&coverage_base))
            && page["coverage"]["complete"].as_bool() == Some(true);
        if use_recovery {
            let checkpoint: Option<String> = match page["coverage"].get("projectedThroughSequence") {
                Some(c) if c.is_null() => None,
                _ => Some(js_str(&page["coverage"]["projectedThroughSequence"])),
            };
            let prev_cp = window.get("coverageCheckpoint");
            let prev_is_null = prev_cp.is_some_and(|c| c.is_null());
            let prev_is_undef = prev_cp.is_none();
            let cursor_not_null = !window.get("cursor").is_some_and(|c| c.is_null());
            let through: Option<String> = if checkpoint.is_none() || (cursor_not_null && prev_is_null) {
                None
            } else if prev_is_undef || prev_is_null {
                checkpoint.clone()
            } else {
                let c = checkpoint.clone().unwrap_or_default();
                let p = js_str(prev_cp.unwrap());
                let cb = c.parse::<i128>().unwrap_or(0);
                let pb = p.parse::<i128>().unwrap_or(0);
                Some(if cb < pb { c } else { p })
            };
            let required = js_str(&recovery.as_ref().map(|r| r["requiredCheckpoint"].clone()).unwrap_or(V::Null));
            let complete = all_complete && through.is_some()
                && through.as_ref().and_then(|t| t.parse::<i128>().ok()).unwrap_or(0)
                    >= required.parse::<i128>().unwrap_or(0);
            if page["pagination"]["hasMore"].as_bool() == Some(true) {
                let mut w = window.clone();
                w["coverageComplete"] = json!(all_complete);
                w["coverageCheckpoint"] = through.map(|s| json!(s)).unwrap_or(V::Null);
                w["cursor"] = page["pagination"]["nextCursor"].clone();
                let mut cs = window["cursors"].as_array().cloned().unwrap_or_default();
                cs.push(page["pagination"]["nextCursor"].clone());
                w["cursors"] = json!(cs);
                m["recovery"] = json!({"request": recovery.clone().unwrap_or(V::Null), "window": w});
            } else if complete {
                m["recovery"] = V::Null;
                let mut done = m["recoveryCompleted"].as_array().cloned().unwrap_or_default();
                done.push(recovery.clone().map(|r| r["key"].clone()).unwrap_or(V::Null));
                if done.len() > 256 { done = done.split_off(done.len() - 256); }
                m["recoveryCompleted"] = json!(done);
            } else {
                m["recovery"] = json!({"request": recovery.clone().unwrap_or(V::Null), "window": {
                    "from": window["from"], "to": window["to"], "cursor": V::Null, "cursors": [],
                    "coverageComplete": true, "coverageCheckpoint": V::Null,
                }});
            }
        } else {
            if page["pagination"]["hasMore"].as_bool() == Some(true) {
                let mut w = window.clone();
                w["coverageComplete"] = json!(all_complete);
                w["cursor"] = page["pagination"]["nextCursor"].clone();
                let mut cs = window["cursors"].as_array().cloned().unwrap_or_default();
                cs.push(page["pagination"]["nextCursor"].clone());
                w["cursors"] = json!(cs);
                m["window"] = w;
            } else if all_complete {
                m["window"] = V::Null;
            } else {
                m["window"] = json!({"from": window["from"], "to": window["to"],
                    "cursor": V::Null, "cursors": [], "coverageComplete": true});
            }
            if page["pagination"]["hasMore"].as_bool() != Some(true) && all_complete {
                m["completedTo"] = window["to"].clone();
            }
        }
        m["lastRequestWasRecovery"] = json!(use_recovery);
        {
            let mut markets = next["markets"].as_object().cloned().unwrap_or_default();
            markets.insert(eid.clone(), m);
            next["markets"] = V::Object(markets);
        }
        next["pages"] = json!(js_num(&next["pages"]) + 1.0);
        next["lastAt"] = page["receivedAt"].clone();
        let mut event = Map::new();
        event.insert("type".into(), json!("anonymous_trade_page"));
        if let Some(pm) = page.as_object() {
            for (k, v) in pm { event.insert(k.clone(), v.clone()); }
        }
        let received_count = page["trades"].as_array().map(|t| t.len()).unwrap_or(0);
        event.insert("trades".into(), json!(novel.clone()));
        event.insert("receivedCount".into(), json!(received_count));
        event.insert("duplicateCount".into(), json!(received_count - novel.len()));
        if use_recovery {
            event.insert("queryPurpose".into(), json!("expired_paper_quote_coverage_recovery"));
            event.insert("recoveryRequest".into(), recovery.clone().unwrap_or(V::Null));
        }
        let event = V::Object(event);
        if self.journal.commit(&next, &event).is_err() {
            // JS marks this error with durableRecordingFailed=true.
            return Err("TRADE_TAPE_RECORDING_FAILED".into());
        }
        self.state = next;
        let mut rec = self.journal.last.clone().unwrap_or(V::Null);
        rec["hash"] = json!(self.journal.previous);
        (self.on_page)(&rec);
        Ok(self.stats())
    }
    pub fn file(&self) -> &Path { &self.file }
}

/// JS Number(x) coercion for numeric fields.
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

pub fn create_trade_tape(runtime_dir: &Path, universe: &V,
    now: Box<dyn Fn() -> f64>, can_operate: Box<dyn Fn() -> bool>,
    on_page: Option<Box<dyn FnMut(&V)>>) -> Result<TradeTape, String> {
    let operable = can_operate();
    let file = runtime_dir.join("anonymous-trades.jsonl");
    let attempt_file = runtime_dir.join("trade-tape-attempts.json");
    let (journal, state, attempts) = prepare(runtime_dir, universe, operable)?;
    let scope_hash = state["scopeHash"].as_str().unwrap_or("").to_string();
    Ok(TradeTape {
        file, attempt_file,
        scope: universe.clone(),
        scope_hash,
        journal, state, attempts,
        now, can_operate,
        on_page: on_page.unwrap_or_else(|| Box::new(|_| {})),
        pending: None,
    })
}
