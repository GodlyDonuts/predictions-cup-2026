// live-paper/book-publication-v2.mjs — public book publication chain + writer.
use baal_paper::fixed::{big_amount, big_price};
use baal_service_runtime::contract::{instant_ns, timestamp_str};
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::io::Write;

fn check(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(code.to_string()); } Ok(()) }
fn id(v: &V) -> bool { v.as_str().is_some_and(|s| !s.is_empty() && s.as_bytes()[0] != b'0' && s.bytes().all(|b| b.is_ascii_digit()) && s.parse::<i64>().is_ok()) }
fn valid_session(s: &str) -> bool { !s.is_empty() && s.len() <= 128 && s.bytes().enumerate().all(|(i, b)| b.is_ascii_alphanumeric() || (i > 0 && matches!(b, b':' | b'_' | b'.' | b'-'))) }
/// canonical(v): recursively key-sorted JSON (distinct from js_stringify digest).
pub fn canonical(v: &V) -> String {
    match v {
        V::Array(items) => format!("[{}]", items.iter().map(|i| serde_json::from_str::<V>(&canonical(i)).map(|x| serde_json::to_string(&x).unwrap()).unwrap_or_default()).collect::<Vec<_>>().join(",")),
        V::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            format!("{{{}}}", keys.iter().map(|k| {
                let inner = serde_json::from_str::<V>(&canonical(&map[*k])).unwrap();
                format!("{}:{}", serde_json::to_string(k).unwrap(), serde_json::to_string(&inner).unwrap())
            }).collect::<Vec<_>>().join(","))
        }
        _ => serde_json::to_string(v).unwrap_or_default(),
    }
}
fn hash(v: &V) -> String { sha256(canonical(v).as_bytes()) }
fn exact(v: &V, keys: &[&str]) -> bool { v.as_object().is_some_and(|m| m.len() == keys.len() && keys.iter().all(|k| m.contains_key(*k))) }
fn instant(s: &str) -> i64 { baal_service_runtime::contract::parse_iso_ms(s).map(|m| m as i64).unwrap_or(i64::MIN) }

fn scope_of(universe: &V) -> Result<V, String> {
    check(universe["tournamentId"].as_str().is_some_and(|s| s.len() == 36 && s.bytes().enumerate().all(|(i, b)| match i { 8 | 13 | 18 | 23 => b == b'-', _ => b.is_ascii_hexdigit() }))
        && universe["exchanges"].as_array().is_some_and(|e| !e.is_empty() && e.len() <= 10000), "PUBLIC_BOOK_SCOPE_INVALID")?;
    let mut markets = BTreeSet::new();
    let mut exchanges = BTreeSet::new();
    let mut rows = Vec::new();
    for row in universe["exchanges"].as_array().unwrap() {
        check(id(&row["marketId"]) && id(&row["exchangeId"]) && row["option"].as_str() == Some("YES")
            && !markets.contains(row["marketId"].as_str().unwrap()) && !exchanges.contains(row["exchangeId"].as_str().unwrap()), "PUBLIC_BOOK_SCOPE_INVALID")?;
        markets.insert(row["marketId"].as_str().unwrap().to_string());
        exchanges.insert(row["exchangeId"].as_str().unwrap().to_string());
        rows.push(json!({"marketId": row["marketId"], "exchangeId": row["exchangeId"], "option": "YES"}));
    }
    rows.sort_by_key(|r| r["marketId"].as_str().unwrap().parse::<i64>().unwrap_or(0));
    Ok(json!({"tournamentId": universe["tournamentId"], "exchanges": rows}))
}
/// createBookPublicationState(universe)
pub fn create_book_publication_state(universe: &V) -> Result<V, String> {
    let scope = scope_of(universe)?;
    let scope_hash = hash(&scope);
    Ok(json!({"schemaVersion": 1, "tournamentId": scope["tournamentId"], "scope": scope,
        "scopeHash": scope_hash, "sessionId": V::Null, "sessions": [], "closed": true,
        "sequence": 0, "hash": "", "lastAt": V::Null, "books": {}, "sourceVersions": {},
        "invalidations": {"global": V::Null, "markets": {}}}))
}
fn validate_book(book: &V, state: &V, published_at: &str) -> Result<V, String> {
    check(exact(book, &["tournamentId","marketId","exchangeId","instrument","receivedAt","requestStartedAt","sourceAt","sequence","versionKey","bids","asks","freshnessPolicy","sourceEventAgeMs"]), "PUBLIC_BOOK_RECORD_INVALID")?;
    let spec = state["scope"]["exchanges"].as_array().unwrap().iter().find(|r| r["marketId"] == book["marketId"]).cloned();
    check(spec.as_ref().is_some_and(|s| s["exchangeId"] == book["exchangeId"])
        && book["tournamentId"] == state["tournamentId"]
        && book["instrument"].as_str() == Some(&format!("{}:YES", book["exchangeId"].as_str().unwrap_or("")))
        && book["freshnessPolicy"].as_str() == Some("authoritative_receipt_v1"), "PUBLIC_BOOK_RECORD_INVALID")?;
    let started = instant(book["requestStartedAt"].as_str().unwrap_or(""));
    let received = instant(book["receivedAt"].as_str().unwrap_or(""));
    let published = instant(published_at);
    let source = instant_ns(book["sourceAt"].as_str().unwrap_or("")).unwrap_or(i128::MIN);
    check(started <= received && received <= published && published - received <= 20_000 && received - started <= 20_000, "PUBLIC_BOOK_RECORD_INVALID")?;
    check(timestamp_str(book["sourceAt"].as_str().unwrap_or("")).ok().as_deref() == Some(book["sourceAt"].as_str().unwrap_or(""))
        && source <= (received as i128 + 2000) * 1_000_000
        && book["sequence"].as_str().is_some_and(|s| (s == "0" || (s.as_bytes()[0] != b'0' && s.bytes().all(|b| b.is_ascii_digit()))) && s.parse::<i64>().is_ok())
        && book["versionKey"].as_str() == Some(&format!("{}:{}", book["sequence"].as_str().unwrap(), book["sourceAt"].as_str().unwrap()))
        && book["sourceEventAgeMs"].as_i64() == Some(received - instant(book["sourceAt"].as_str().unwrap_or(""))), "PUBLIC_BOOK_RECORD_INVALID")?;
    for barrier in [state["invalidations"]["global"].clone(), state["invalidations"]["markets"][book["marketId"].as_str().unwrap_or("")].clone()] {
        if !barrier.is_null() {
            check(started >= instant(barrier["at"].as_str().unwrap_or("")), "PUBLIC_BOOK_REQUEST_PRECEDES_INVALIDATION")?;
        }
    }
    let levels = |rows: &V, descending: bool| -> Result<(), String> {
        check(rows.as_array().is_some_and(|r| !r.is_empty() && r.len() <= 20), "PUBLIC_BOOK_RECORD_INVALID")?;
        let mut previous: Option<num_bigint::BigInt> = None;
        for row in rows.as_array().unwrap() {
            check(exact(row, &["price", "quantity"]) && row["quantity"].as_i64().is_some_and(|q| q > 0), "PUBLIC_BOOK_RECORD_INVALID")?;
            let p = big_price(row["price"].as_str().unwrap_or(""))?;
            check(previous.as_ref().is_none_or(|x| if descending { &p < x } else { &p > x }), "PUBLIC_BOOK_RECORD_INVALID")?;
            previous = Some(p);
        }
        Ok(())
    };
    levels(&book["bids"], true)?;
    levels(&book["asks"], false)?;
    check(big_amount(book["bids"][0]["price"].as_str().unwrap_or(""))? < big_amount(book["asks"][0]["price"].as_str().unwrap_or(""))?, "PUBLIC_BOOK_RECORD_INVALID")?;
    let depth_hash = hash(&json!({"bids": book["bids"], "asks": book["asks"]}));
    let prior = &state["sourceVersions"][book["instrument"].as_str().unwrap_or("")];
    if !prior.is_null() {
        check(book["sequence"].as_str().unwrap_or("").parse::<i128>().unwrap_or(0) >= prior["sequence"].as_str().unwrap_or("").parse::<i128>().unwrap_or(0)
            && source >= instant_ns(prior["sourceAt"].as_str().unwrap_or("")).unwrap_or(i128::MIN)
            && received >= instant(prior["receivedAt"].as_str().unwrap_or("")), "PUBLIC_BOOK_SOURCE_ROLLBACK")?;
        check(book["sequence"] != prior["sequence"] || (book["versionKey"] == prior["versionKey"] && depth_hash == prior["depthHash"].as_str().unwrap_or("")), "PUBLIC_BOOK_VERSION_CONFLICT")?;
    }
    Ok(json!({"sequence": book["sequence"], "sourceAt": book["sourceAt"], "versionKey": book["versionKey"],
        "receivedAt": book["receivedAt"], "depthHash": depth_hash}))
}
/// applyBookPublication(state, record)
pub fn apply_book_publication(state: &V, record: &V) -> Result<V, String> {
    check(state["schemaVersion"].as_i64() == Some(1)
        && state["scopeHash"].as_str() == Some(hash(&scope_of(&state["scope"])?).as_str()), "PUBLIC_BOOK_STATE_INVALID")?;
    check(exact(record, &["schemaVersion","kind","sequence","previousHash","sessionId","tournamentId","scopeHash","at","payload","hash"]), "PUBLIC_BOOK_RECORD_INVALID")?;
    let received_hash = record["hash"].as_str().unwrap_or("").to_string();
    let mut body = record.clone();
    body.as_object_mut().unwrap().remove("hash");
    check(body["schemaVersion"].as_i64() == Some(1) && body["sequence"].as_i64() == Some(state["sequence"].as_i64().unwrap_or(0) + 1)
        && body["previousHash"].as_str() == state["hash"].as_str() && received_hash == hash(&body)
        && body["tournamentId"] == state["tournamentId"] && body["scopeHash"] == state["scopeHash"]
        && valid_session(body["sessionId"].as_str().unwrap_or("")), "PUBLIC_BOOK_CHAIN_INVALID")?;
    let at = instant(body["at"].as_str().unwrap_or(""));
    check(state["lastAt"].is_null() || at >= instant(state["lastAt"].as_str().unwrap_or("")), "PUBLIC_BOOK_CLOCK_ROLLBACK")?;
    let mut next = state.clone();
    next["books"] = state["books"].clone();
    next["sourceVersions"] = state["sourceVersions"].clone();
    next["invalidations"] = state["invalidations"].clone();
    let reasons = ["BOOK_READ_FAILED","INVALID_BOOK","ACCOUNT_OUTAGE","STOP","COLLECTOR_FAILURE","MANUAL_INVALIDATION"];
    let stop_reasons = ["STOP","COLLECTOR_FAILURE","NORMAL_SHUTDOWN"];
    let kind = body["kind"].as_str().unwrap_or("");
    if kind == "session_start" {
        check(exact(&body["payload"], &["scope"]) && canonical(&body["payload"]["scope"]) == canonical(&state["scope"])
            && !state["sessions"].as_array().unwrap().contains(&body["sessionId"]), "PUBLIC_BOOK_SESSION_INVALID")?;
        next["sessionId"] = body["sessionId"].clone();
        next["sessions"].as_array_mut().unwrap().push(body["sessionId"].clone());
        next["closed"] = V::Bool(false);
        next["books"] = json!({});
        next["invalidations"] = json!({"global": {"at": body["at"], "reason": "SESSION_START", "sequence": body["sequence"]}, "markets": {}});
    } else {
        check(!state["closed"].as_bool().unwrap_or(true) && body["sessionId"] == state["sessionId"], "PUBLIC_BOOK_SESSION_CLOSED")?;
        if kind == "book" {
            check(exact(&body["payload"], &["book"]), "PUBLIC_BOOK_RECORD_INVALID")?;
            let book = &body["payload"]["book"];
            let version = validate_book(book, state, body["at"].as_str().unwrap_or(""))?;
            next["sourceVersions"][book["instrument"].as_str().unwrap()] = version;
            let mut b = book.clone();
            b["publicationSequence"] = body["sequence"].clone();
            b["publicationHash"] = json!(received_hash);
            b["publicationSessionId"] = body["sessionId"].clone();
            b["publishedAt"] = body["at"].clone();
            next["books"][book["instrument"].as_str().unwrap()] = b;
        } else if kind == "invalidation" {
            check(exact(&body["payload"], &["marketId", "reason"])
                && reasons.contains(&body["payload"]["reason"].as_str().unwrap_or(""))
                && (body["payload"]["marketId"].is_null() || state["scope"]["exchanges"].as_array().unwrap().iter().any(|r| r["marketId"] == body["payload"]["marketId"])), "PUBLIC_BOOK_RECORD_INVALID")?;
            let barrier = json!({"at": body["at"], "reason": body["payload"]["reason"], "sequence": body["sequence"]});
            if body["payload"]["marketId"].is_null() {
                next["books"] = json!({});
                next["invalidations"]["global"] = barrier;
            } else {
                next["invalidations"]["markets"][body["payload"]["marketId"].as_str().unwrap()] = barrier;
                let mid = body["payload"]["marketId"].as_str().unwrap().to_string();
                let keys: Vec<String> = next["books"].as_object().unwrap().iter().filter(|(_, b)| b["marketId"].as_str() == Some(&mid)).map(|(k, _)| k.clone()).collect();
                for k in keys { next["books"].as_object_mut().unwrap().remove(&k); }
            }
        } else if kind == "session_stop" {
            check(exact(&body["payload"], &["reason"]) && stop_reasons.contains(&body["payload"]["reason"].as_str().unwrap_or("")), "PUBLIC_BOOK_RECORD_INVALID")?;
            next["closed"] = V::Bool(true);
            next["books"] = json!({});
            next["invalidations"]["global"] = json!({"at": body["at"], "reason": body["payload"]["reason"], "sequence": body["sequence"]});
        } else {
            return Err("PUBLIC_BOOK_RECORD_INVALID".into());
        }
    }
    next["sequence"] = body["sequence"].clone();
    next["hash"] = json!(received_hash);
    next["lastAt"] = body["at"].clone();
    Ok(next)
}
/// createBookPublication({file, universe, sessionId, now, canOperate}) — synchronous writer.
pub struct BookPublicationWriter<F: Fn() -> i64, G: Fn() -> bool> {
    file: std::path::PathBuf,
    pub state: V,
    size: u64,
    failed: bool,
    open: bool,
    sid: String,
    now: F,
    can_operate: G,
}
impl<F: Fn() -> i64, G: Fn() -> bool> BookPublicationWriter<F, G> {
    pub fn new(file: &std::path::Path, universe: &V, session_id: &str, now: F, can_operate: G) -> Result<Self, String> {
        check(file.is_absolute() && crate::book_publication_v2::valid_session(session_id), "PUBLIC_BOOK_DEPENDENCIES")?;
        let mut w = Self { file: file.to_path_buf(), state: create_book_publication_state(universe)?, size: 0, failed: false, open: true, sid: session_id.to_string(), now, can_operate };
        let raw = if file.exists() { std::fs::read_to_string(file).map_err(|_| "PUBLIC_BOOK_CHAIN_INVALID".to_string())? } else { String::new() };
        check(raw.is_empty() || raw.ends_with('\n'), "PUBLIC_BOOK_CHAIN_INVALID")?;
        if !raw.is_empty() {
            for line in raw[..raw.len() - 1].split('\n') {
                check(!line.is_empty(), "PUBLIC_BOOK_CHAIN_INVALID")?;
                let record: V = serde_json::from_str(line).map_err(|_| "PUBLIC_BOOK_CHAIN_INVALID".to_string())?;
                w.state = apply_book_publication(&w.state, &record)?;
            }
        }
        w.size = raw.len() as u64;
        let rec = w.prepare("session_start", json!({"scope": w.state["scope"]}))?;
        w.persist(&rec)?;
        Ok(w)
    }
    fn stamp(&self) -> Result<String, String> {
        let n = (self.now)();
        check((0..=86_400_000_000_000_000i64).contains(&n), "PUBLIC_BOOK_CLOCK_INVALID")?;
        Ok(baal_service_runtime::contract::ms_to_iso(n as f64))
    }
    fn prepare(&self, kind: &str, payload: V) -> Result<V, String> {
        let mut body = json!({"schemaVersion": 1, "kind": kind, "sequence": self.state["sequence"].as_i64().unwrap_or(0) + 1,
            "previousHash": self.state["hash"], "sessionId": self.sid, "tournamentId": self.state["tournamentId"],
            "scopeHash": self.state["scopeHash"], "at": self.stamp()?, "payload": payload});
        let h = hash(&body);
        body["hash"] = json!(h);
        Ok(body)
    }
    fn persist(&mut self, record: &V) -> Result<V, String> {
        check(self.open && !self.failed, "PUBLIC_BOOK_WRITER_CLOSED")?;
        let r = (|| -> Result<V, String> {
            let meta = std::fs::metadata(&self.file).map_err(|_| "x".to_string())?;
            check(meta.len() == self.size, "PUBLIC_BOOK_APPEND_CONFLICT")?;
            let mut f = std::fs::OpenOptions::new().append(true).open(&self.file).map_err(|_| "x".to_string())?;
            let bytes = format!("{}\n", serde_json::to_string(record).map_err(|_| "x".to_string())?);
            f.write_all(bytes.as_bytes()).map_err(|_| "x".to_string())?;
            f.sync_all().map_err(|_| "x".to_string())?;
            self.size += bytes.len() as u64;
            self.state = apply_book_publication(&self.state, record)?;
            Ok(record.clone())
        })();
        if r.is_err() {
            self.failed = true;
            return Err(if r.as_ref().err().map(|e| e.starts_with("PUBLIC_BOOK_")).unwrap_or(false) { r.err().unwrap() } else { "PUBLIC_BOOK_DURABILITY_FAILURE".into() });
        }
        r
    }
    pub fn invalidate(&mut self, market_id: Option<&str>, reason: &str) -> Result<V, String> {
        check(["BOOK_READ_FAILED","INVALID_BOOK","ACCOUNT_OUTAGE","STOP","COLLECTOR_FAILURE","MANUAL_INVALIDATION"].contains(&reason)
            && (market_id.is_none() || self.state["scope"]["exchanges"].as_array().unwrap().iter().any(|r| r["marketId"].as_str() == market_id)), "PUBLIC_BOOK_RECORD_INVALID")?;
        let rec = self.prepare("invalidation", json!({"marketId": market_id, "reason": reason}))?;
        self.persist(&rec)
    }
    pub fn close(&mut self, reason: &str) -> Result<bool, String> {
        if !self.open { return Ok(false); }
        check(["STOP","COLLECTOR_FAILURE","NORMAL_SHUTDOWN"].contains(&reason), "PUBLIC_BOOK_RECORD_INVALID")?;
        let rec = self.prepare("session_stop", json!({"reason": reason}))?;
        self.persist(&rec)?;
        self.open = false;
        Ok(true)
    }
    pub fn publish_book(&mut self, market_id: &str, response: &V) -> Result<bool, String> {
        check(self.open && !self.failed, "PUBLIC_BOOK_WRITER_CLOSED")?;
        if !(self.can_operate)() { self.close("STOP")?; return Ok(false); }
        check(self.state["scope"]["exchanges"].as_array().unwrap().iter().any(|r| r["marketId"].as_str() == Some(market_id)), "PUBLIC_BOOK_SCOPE_INVALID")?;
        let spec = self.state["scope"]["exchanges"].as_array().unwrap().iter().find(|r| r["marketId"].as_str() == Some(market_id)).unwrap().clone();
        let prepared = (|| -> Result<V, String> {
            let endpoint = format!("/markets/{}/orderbook?tournamentId={}&depth=20", market_id, self.state["tournamentId"].as_str().unwrap());
            check(response["endpoint"].as_str() == Some(&endpoint)
                && (response["status"].is_null() || response["status"].as_i64() == Some(200))
                && (response["data"]["marketId"].is_null() || response["data"]["marketId"].to_string().trim_matches('"') == market_id), "x")?;
            let normalized = crate::model::normalize_book(&response["data"], market_id,
                self.state["tournamentId"].as_str().unwrap(), response["receivedAt"].as_str().unwrap_or(""),
                response["requestStartedAt"].as_str().unwrap_or(""), 20000, &BTreeSet::new(), "authoritative_receipt_v1")?;
            check(normalized["exchangeId"] == spec["exchangeId"], "x")?;
            let mut book = json!({"tournamentId": self.state["tournamentId"]});
            for (k, v) in normalized.as_object().unwrap() { book[k] = v.clone(); }
            self.prepare("book", json!({"book": book}))
        })();
        let prepared = match prepared {
            Ok(p) => p,
            Err(e) if e.starts_with("PUBLIC_BOOK_") => return Err(e),
            Err(_) => { self.invalidate(Some(market_id), "INVALID_BOOK")?; return Ok(false); }
        };
        if !(self.can_operate)() { self.close("STOP")?; return Ok(false); }
        self.persist(&prepared)?;
        Ok(true)
    }
}
