// live-paper/book-publication-v3.mjs — native-prefix-restored book publication writer.
use baal_service_runtime::json_text::sha256;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const REASONS: [&str; 6] = ["BOOK_READ_FAILED", "INVALID_BOOK", "ACCOUNT_OUTAGE", "STOP",
    "COLLECTOR_FAILURE", "MANUAL_INVALIDATION"];
const STOP_REASONS: [&str; 3] = ["STOP", "COLLECTOR_FAILURE", "NORMAL_SHUTDOWN"];
fn check(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(if code.is_empty() { "PUBLIC_BOOK_RECORD_INVALID".into() } else { code.to_string() }); }
    Ok(())
}
fn id(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        let b = s.as_bytes();
        !b.is_empty() && b[0] != b'0' && b.iter().all(|c| c.is_ascii_digit())
            && s.parse::<i64>().is_ok()
    })
}
fn plain(v: &V) -> bool {
    v.as_object().is_some_and(|m| {
        !m.contains_key("__proto__") && !m.contains_key("constructor") && !m.contains_key("prototype")
    })
}
fn exact(v: &V, keys: &[&str]) -> bool {
    plain(v) && v.as_object().map(|m| m.len() == keys.len()
        && keys.iter().all(|k| m.contains_key(*k))).unwrap_or(false)
}
fn canonical(v: &V) -> String { crate::book_publication_v2::canonical(v) }
fn hash(v: &V) -> String { sha256(canonical(v).as_bytes()) }
fn session(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        let b = s.as_bytes();
        !b.is_empty() && b.len() <= 128
            && b[0].is_ascii_alphanumeric()
            && b.iter().all(|c| c.is_ascii_alphanumeric() || [b':', b'_', b'.', b'-'].contains(c))
    })
}
fn instant(v: &V) -> Result<i64, String> { baal_paper::fixed::instant(v.as_str().unwrap_or("")) }
fn price(v: &V) -> Result<BigInt, String> { baal_paper::fixed::big_price(v.as_str().unwrap_or("")) }
fn amount(v: &V) -> Result<BigInt, String> { baal_paper::fixed::big_amount(v.as_str().unwrap_or("")) }

fn scope_of(universe: &V) -> Result<V, String> {
    let tid = universe["tournamentId"].as_str().unwrap_or("");
    let tid_ok = {
        let parts: Vec<&str> = tid.split('-').collect();
        parts.len() == 5 && parts[0].len() == 8 && parts[1..4].iter().all(|p| p.len() == 4)
            && parts[4].len() == 12
            && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_hexdigit()))
    };
    check(plain(universe) && tid_ok
        && universe["exchanges"].as_array().is_some_and(|e| !e.is_empty() && e.len() <= 10000),
        "PUBLIC_BOOK_SCOPE_INVALID")?;
    let mut markets = BTreeSet::new();
    let mut exchanges = BTreeSet::new();
    let mut rows = Vec::new();
    for row in universe["exchanges"].as_array().unwrap() {
        check(plain(row) && id(&row["marketId"]) && id(&row["exchangeId"])
            && row["option"].as_str() == Some("YES")
            && !markets.contains(row["marketId"].as_str().unwrap_or(""))
            && !exchanges.contains(row["exchangeId"].as_str().unwrap_or("")),
            "PUBLIC_BOOK_SCOPE_INVALID")?;
        markets.insert(row["marketId"].as_str().unwrap().to_string());
        exchanges.insert(row["exchangeId"].as_str().unwrap().to_string());
        rows.push(json!({"marketId": row["marketId"], "exchangeId": row["exchangeId"], "option": "YES"}));
    }
    rows.sort_by(|a, b| {
        a["marketId"].as_str().unwrap_or("").parse::<u64>().unwrap_or(0)
            .cmp(&b["marketId"].as_str().unwrap_or("").parse::<u64>().unwrap_or(0))
    });
    Ok(json!({"tournamentId": universe["tournamentId"], "exchanges": rows}))
}
/// createBookPublicationState(universe)
pub fn create_book_publication_state_v3(universe: &V) -> Result<V, String> {
    let scope = scope_of(universe)?;
    Ok(json!({"schemaVersion": 1, "tournamentId": scope["tournamentId"], "scope": scope,
        "scopeHash": hash(&scope), "sessionId": V::Null, "sessions": [], "closed": true,
        "sequence": 0, "hash": "", "lastAt": V::Null, "books": {}, "sourceVersions": {},
        "invalidations": {"global": V::Null, "markets": {}}}))
}
fn validate_book(book: &V, state: &V, published_at: &str) -> Result<V, String> {
    const FIELDS: [&str; 13] = ["tournamentId", "marketId", "exchangeId", "instrument", "receivedAt",
        "requestStartedAt", "sourceAt", "sequence", "versionKey", "bids", "asks", "freshnessPolicy",
        "sourceEventAgeMs"];
    check(exact(book, &FIELDS), "")?;
    let spec = state["scope"]["exchanges"].as_array().cloned().unwrap_or_default().into_iter()
        .find(|row| row["marketId"] == book["marketId"]);
    check(spec.as_ref().is_some_and(|s| s["exchangeId"] == book["exchangeId"])
        && book["tournamentId"] == state["tournamentId"]
        && book["instrument"].as_str()
            == Some(format!("{}:YES", book["exchangeId"].as_str().unwrap_or("")).as_str())
        && book["freshnessPolicy"].as_str() == Some("authoritative_receipt_v1"), "")?;
    let started = instant(&book["requestStartedAt"])?;
    let received = instant(&book["receivedAt"])?;
    let published = instant(&json!(published_at))?;
    let source = baal_service_runtime::contract::instant_ns(book["sourceAt"].as_str().unwrap_or(""))
        .map_err(|_| "PUBLIC_BOOK_RECORD_INVALID".to_string())?;
    check(started <= received && received <= published && published - received <= 20000
        && received - started <= 20000, "")?;
    let canon_source = baal_service_runtime::contract::timestamp_str(book["sourceAt"].as_str().unwrap_or(""))
        .map_err(|_| "PUBLIC_BOOK_RECORD_INVALID".to_string())?;
    check(book["sourceAt"].as_str() == Some(canon_source.as_str())
        && source <= (received as i128 + 2000) * 1_000_000i128
        && book["sequence"].as_str().is_some_and(|s| {
            let b = s.as_bytes();
            !b.is_empty() && (b.len() == 1 || b[0] != b'0') && b.iter().all(|c| c.is_ascii_digit())
                && s.parse::<i64>().is_ok()
        })
        && book["versionKey"].as_str()
            == Some(format!("{}:{}", book["sequence"].as_str().unwrap_or(""), book["sourceAt"].as_str().unwrap_or("")).as_str())
        && book["sourceEventAgeMs"].as_f64()
            == Some(received as f64 - baal_service_runtime::contract::parse_iso_ms(
                book["sourceAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)), "")?;
    for barrier in [&state["invalidations"]["global"], &state["invalidations"]["markets"][book["marketId"].as_str().unwrap_or("")]] {
        if !barrier.is_null() {
            check(started >= instant(&barrier["at"])?, "PUBLIC_BOOK_REQUEST_PRECEDES_INVALIDATION")?;
        }
    }
    let levels = |rows: &V, descending: bool| -> Result<(), String> {
        let rows = rows.as_array().cloned().unwrap_or_default();
        check(!rows.is_empty() && rows.len() <= 20, "")?;
        let mut previous: Option<BigInt> = None;
        for row in &rows {
            check(exact(row, &["price", "quantity"])
                && row["quantity"].as_i64().is_some_and(|q| q > 0), "")?;
            let p = price(&row["price"])?;
            check(previous.is_none()
                || (descending && p < *previous.as_ref().unwrap())
                || (!descending && p > *previous.as_ref().unwrap()), "")?;
            previous = Some(p);
        }
        Ok(())
    };
    levels(&book["bids"], true)?;
    levels(&book["asks"], false)?;
    check(amount(&book["bids"][0]["price"])? < amount(&book["asks"][0]["price"])?, "")?;
    let depth_hash = hash(&json!({"bids": book["bids"], "asks": book["asks"]}));
    let prior = &state["sourceVersions"][book["instrument"].as_str().unwrap_or("")];
    if !prior.is_null() {
        let seq: BigInt = book["sequence"].as_str().unwrap_or("0").parse().unwrap_or_default();
        let pseq: BigInt = prior["sequence"].as_str().unwrap_or("0").parse().unwrap_or_default();
        check(seq >= pseq
            && source >= baal_service_runtime::contract::instant_ns(prior["sourceAt"].as_str().unwrap_or("")).unwrap_or(0)
            && received >= instant(&prior["receivedAt"])?, "PUBLIC_BOOK_SOURCE_ROLLBACK")?;
        check(book["sequence"] != prior["sequence"]
            || (book["versionKey"] == prior["versionKey"] && depth_hash == prior["depthHash"].as_str().unwrap_or("")),
            "PUBLIC_BOOK_VERSION_CONFLICT")?;
    }
    Ok(json!({"sequence": book["sequence"], "sourceAt": book["sourceAt"], "versionKey": book["versionKey"],
        "receivedAt": book["receivedAt"], "depthHash": depth_hash}))
}
/// applyBookPublication(state, record) — v3 chain (previousHash field).
pub fn apply_book_publication_v3(state: &V, record: &V) -> Result<V, String> {
    check(state["schemaVersion"].as_i64() == Some(1)
        && state["scopeHash"].as_str() == Some(hash(&scope_of(&state["scope"])?).as_str()),
        "PUBLIC_BOOK_STATE_INVALID")?;
    check(exact(record, &["schemaVersion", "kind", "sequence", "previousHash", "sessionId",
        "tournamentId", "scopeHash", "at", "payload", "hash"]), "")?;
    let received_hash = record["hash"].as_str().unwrap_or("").to_string();
    let mut body = record.clone();
    body.as_object_mut().and_then(|m| m.shift_remove("hash"));
    check(body["schemaVersion"].as_i64() == Some(1)
        && body["sequence"].as_i64().is_some()
        && body["sequence"].as_i64() == Some(state["sequence"].as_i64().unwrap_or(0) + 1)
        && body["previousHash"] == state["hash"]
        && received_hash == hash(&body)
        && body["tournamentId"] == state["tournamentId"]
        && body["scopeHash"] == state["scopeHash"]
        && session(&body["sessionId"]), "PUBLIC_BOOK_CHAIN_INVALID")?;
    let at = instant(&body["at"])?;
    check(state["lastAt"].is_null() || at >= instant(&state["lastAt"])?, "PUBLIC_BOOK_CLOCK_ROLLBACK")?;
    let mut next = state.clone();
    let kind = body["kind"].as_str().unwrap_or("");
    if kind == "session_start" {
        check(exact(&body["payload"], &["scope"])
            && canonical(&body["payload"]["scope"]) == canonical(&state["scope"])
            && !state["sessions"].as_array().is_some_and(|s| s.contains(&body["sessionId"])),
            "PUBLIC_BOOK_SESSION_INVALID")?;
        next["sessionId"] = body["sessionId"].clone();
        next["sessions"].as_array_mut().map(|s| s.push(body["sessionId"].clone()));
        next["closed"] = json!(false);
        next["books"] = json!({});
        next["invalidations"] = json!({"global": {"at": body["at"], "reason": "SESSION_START",
            "sequence": body["sequence"]}, "markets": {}});
    } else {
        check(state["closed"] != V::Bool(true) && body["sessionId"] == state["sessionId"],
            "PUBLIC_BOOK_SESSION_CLOSED")?;
        if kind == "book" {
            check(exact(&body["payload"], &["book"]), "")?;
            let book = &body["payload"]["book"];
            let version = validate_book(book, state, body["at"].as_str().unwrap_or(""))?;
            next["sourceVersions"][book["instrument"].as_str().unwrap_or("")] = version;
            let mut b = book.clone();
            b["publicationSequence"] = body["sequence"].clone();
            b["publicationHash"] = json!(received_hash);
            b["publicationSessionId"] = body["sessionId"].clone();
            b["publishedAt"] = body["at"].clone();
            next["books"][book["instrument"].as_str().unwrap_or("")] = b;
        } else if kind == "invalidation" {
            check(exact(&body["payload"], &["marketId", "reason"])
                && REASONS.contains(&body["payload"]["reason"].as_str().unwrap_or(""))
                && (body["payload"]["marketId"].is_null()
                    || state["scope"]["exchanges"].as_array().is_some_and(|e| e.iter()
                        .any(|r| r["marketId"] == body["payload"]["marketId"]))), "")?;
            let barrier = json!({"at": body["at"], "reason": body["payload"]["reason"],
                "sequence": body["sequence"]});
            if body["payload"]["marketId"].is_null() {
                next["books"] = json!({});
                next["invalidations"]["global"] = barrier;
            } else {
                next["invalidations"]["markets"][body["payload"]["marketId"].as_str().unwrap_or("")] = barrier;
                let keys: Vec<String> = next["books"].as_object().cloned().unwrap_or_default()
                    .iter().filter(|(_k, b)| b["marketId"] == body["payload"]["marketId"])
                    .map(|(k, _b)| k.clone()).collect();
                for k in keys {
                    next["books"].as_object_mut().and_then(|m| m.shift_remove(&k));
                }
            }
        } else if kind == "session_stop" {
            check(exact(&body["payload"], &["reason"])
                && STOP_REASONS.contains(&body["payload"]["reason"].as_str().unwrap_or("")), "")?;
            next["closed"] = json!(true);
            next["books"] = json!({});
            next["invalidations"]["global"] = json!({"at": body["at"], "reason": body["payload"]["reason"],
                "sequence": body["sequence"]});
        } else {
            check(false, "")?;
        }
    }
    next["sequence"] = body["sequence"].clone();
    next["hash"] = json!(received_hash);
    next["lastAt"] = body["at"].clone();
    Ok(next)
}

fn writers() -> &'static Mutex<std::collections::HashSet<String>> {
    static W: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}
/// createNativeBookPublication({file, universe, sessionId, runtimeDir, nativeBackend, receiptDir, now, canOperate})
pub struct NativeBookPublicationV3 {
    file: PathBuf,
    resolved: String,
    state: V,
    fd: Option<std::fs::File>,
    size: u64,
    failed: bool,
    startup: V,
    session_id: String,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
}
impl NativeBookPublicationV3 {
    pub fn open(file: &Path, universe: &V, session_id: &str, runtime_dir: &Path, native_backend: &V,
        receipt_dir: &Path, now: Arc<dyn Fn() -> i64 + Send + Sync>,
        can_operate: Arc<dyn Fn() -> bool + Send + Sync>) -> Result<Self, String> {
        check(file.is_absolute() && session(&json!(session_id)), "PUBLIC_BOOK_DEPENDENCIES")?;
        let resolved = file.canonicalize().unwrap_or_else(|_| file.to_path_buf())
            .to_string_lossy().to_lowercase();
        check(!writers().lock().unwrap().contains(&resolved), "PUBLIC_BOOK_WRITER_EXISTS")?;
        let state = create_book_publication_state_v3(universe)?;
        let mut host = Self { file: file.to_path_buf(), resolved: resolved.clone(), state, fd: None,
            size: 0, failed: false, startup: V::Null, session_id: session_id.to_string(), now, can_operate };
        writers().lock().unwrap().insert(resolved.clone());
        let r = (|| -> Result<(), String> {
            check(host.allowed(), "PUBLIC_BOOK_STOPPED")?;
            crate::native_publication::native_publication_binding(runtime_dir, native_backend)?;
            if !file.exists() {
                std::fs::OpenOptions::new().write(true).create_new(true).open(file)
                    .map_err(|e| e.to_string())?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600));
                }
            }
            let raw = std::fs::read(file).map_err(|e| e.to_string())?;
            let source_hash = sha256(&raw);
            let started = std::time::Instant::now();
            let restored = crate::native_publication::native_publication_state(runtime_dir,
                native_backend, universe, file, raw.len() as u64, &source_hash, receipt_dir)?;
            check(host.allowed(), "PUBLIC_BOOK_STOPPED")?;
            check(sha256(&std::fs::read(file).map_err(|e| e.to_string())?) == source_hash,
                "PUBLIC_BOOK_PREFIX_CHANGED")?;
            check(restored["state"]["scopeHash"] == host.state["scopeHash"], "PUBLIC_BOOK_SCOPE_INVALID")?;
            host.state = restored["state"].clone();
            host.startup = json!({"mode": "native_public_book_prefix_v1", "sourceBytes": raw.len(),
                "sourceHash": source_hash, "sourceRecords": host.state["sequence"],
                "sourceTailHash": host.state["hash"], "verifyMs": restored["verifyMs"],
                "adapterMs": started.elapsed().as_secs_f64() * 1000.0,
                "binaryHash": native_backend["binaryHash"], "engineHash": restored["engineHash"]});
            host.size = raw.len() as u64;
            host.fd = Some(std::fs::OpenOptions::new().append(true).open(file).map_err(|e| e.to_string())?);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600));
            }
            let prepared = host.prepare("session_start", json!({"scope": host.state["scope"]}))?;
            host.persist(&prepared).map(|_| ())
        })();
        if let Err(e) = r {
            host.release();
            writers().lock().unwrap().remove(&resolved);
            return Err(if e.starts_with("PUBLIC_BOOK_") { e } else { "PUBLIC_BOOK_CHAIN_INVALID".into() });
        }
        Ok(host)
    }
    fn stamp(&self) -> Result<String, String> {
        let n = (self.now)();
        check((0..=86_400_000_000_000_000i64).contains(&n), "PUBLIC_BOOK_CLOCK_INVALID")?;
        Ok(baal_service_runtime::contract::ms_to_iso(n as f64))
    }
    fn allowed(&self) -> bool { (self.can_operate)() }
    fn prepare(&self, kind: &str, payload: V) -> Result<(V, V), String> {
        let mut body = json!({"schemaVersion": 1, "kind": kind,
            "sequence": self.state["sequence"].as_i64().unwrap_or(0) + 1,
            "previousHash": self.state["hash"], "sessionId": self.session_id,
            "tournamentId": self.state["tournamentId"], "scopeHash": self.state["scopeHash"],
            "at": self.stamp()?, "payload": payload});
        let h = hash(&body);
        body.as_object_mut().unwrap().insert("hash".into(), json!(h));
        let next = apply_book_publication_v3(&self.state, &body)?;
        Ok((body, next))
    }
    fn release(&mut self) {
        if let Some(fd) = self.fd.take() {
            drop(fd);
            writers().lock().unwrap().remove(&self.resolved);
        }
    }
    fn persist(&mut self, prepared: &(V, V)) -> Result<V, String> {
        check(self.fd.is_some() && !self.failed, "PUBLIC_BOOK_WRITER_CLOSED")?;
        let r = (|| -> Result<V, String> {
            let meta = self.file.metadata().map_err(|e| e.to_string())?;
            check(meta.len() == self.size, "PUBLIC_BOOK_APPEND_CONFLICT")?;
            let bytes = format!("{}\n", serde_json::to_string(&prepared.0).map_err(|e| e.to_string())?);
            use std::io::Write;
            self.fd.as_ref().unwrap().try_clone().map_err(|e| e.to_string())?
                .write_all(bytes.as_bytes()).map_err(|e| e.to_string())?;
            self.fd.as_ref().unwrap().sync_all().map_err(|e| e.to_string())?;
            self.size += bytes.len() as u64;
            self.state = prepared.1.clone();
            Ok(prepared.0.clone())
        })();
        if r.is_err() {
            self.failed = true;
            self.release();
            return Err("PUBLIC_BOOK_DURABILITY_FAILURE".into());
        }
        r
    }
    /// invalidate({marketId, reason})
    pub fn invalidate(&mut self, market_id: Option<&str>, reason: &str) -> Result<V, String> {
        check(REASONS.contains(&reason)
            && (market_id.is_none() || self.state["scope"]["exchanges"].as_array()
                .is_some_and(|e| e.iter().any(|r| r["marketId"].as_str() == market_id))), "")?;
        let prepared = self.prepare("invalidation",
            json!({"marketId": market_id.map(V::from).unwrap_or(V::Null), "reason": reason}))?;
        self.persist(&prepared)
    }
    /// close(reason = 'STOP')
    pub fn close(&mut self, reason: &str) -> Result<bool, String> {
        if self.fd.is_none() { return Ok(false); }
        check(STOP_REASONS.contains(&reason), "")?;
        let prepared = self.prepare("session_stop", json!({"reason": reason}))?;
        let r = self.persist(&prepared);
        self.release();
        r.map(|_| true)
    }
    /// publishBook({marketId, response})
    pub fn publish_book(&mut self, market_id: &str, response: &V) -> Result<bool, String> {
        check(self.fd.is_some() && !self.failed, "PUBLIC_BOOK_WRITER_CLOSED")?;
        if !self.allowed() {
            self.close("STOP")?;
            return Ok(false);
        }
        let spec = self.state["scope"]["exchanges"].as_array().cloned().unwrap_or_default()
            .into_iter().find(|r| r["marketId"].as_str() == Some(market_id));
        check(spec.is_some(), "PUBLIC_BOOK_SCOPE_INVALID")?;
        let spec = spec.unwrap();
        let prepared = (|| -> Result<(V, V), String> {
            let endpoint = format!("/markets/{}/orderbook?tournamentId={}&depth=20",
                market_id, self.state["tournamentId"].as_str().unwrap_or(""));
            check(response["endpoint"].as_str() == Some(endpoint.as_str())
                && (response["status"].is_null() || response["status"].as_i64() == Some(200))
                && (response["data"]["marketId"].is_null()
                    || response["data"]["marketId"].as_str() == Some(market_id)
                    || response["data"]["marketId"].as_i64().map(|n| n.to_string()).as_deref() == Some(market_id)),
                "INTERNAL")?;
            let normalized = crate::model::normalize_book(&response["data"], market_id,
                self.state["tournamentId"].as_str().unwrap_or(""),
                response["receivedAt"].as_str().unwrap_or(""),
                response["requestStartedAt"].as_str().unwrap_or(""), 20000,
                &BTreeSet::new(), "authoritative_receipt_v1")?;
            check(normalized["exchangeId"] == spec["exchangeId"], "INTERNAL")?;
            let mut book = Map::new();
            book.insert("tournamentId".into(), self.state["tournamentId"].clone());
            for (k, v) in normalized.as_object().cloned().unwrap_or_default() {
                book.insert(k, v);
            }
            self.prepare("book", json!({"book": V::Object(book)}))
        })();
        let prepared = match prepared {
            Ok(p) => p,
            Err(_) => {
                self.invalidate(Some(market_id), "INVALID_BOOK")?;
                return Ok(false);
            }
        };
        if !self.allowed() {
            self.close("STOP")?;
            return Ok(false);
        }
        self.persist(&prepared)?;
        Ok(true)
    }
    pub fn startup(&self) -> V { self.startup.clone() }
    /// status() — failed: {...state, failed: true, closed: true, books: {}}
    pub fn status(&self) -> V {
        if self.failed {
            let mut s = self.state.clone();
            s["failed"] = json!(true);
            s["closed"] = json!(true);
            s["books"] = json!({});
            s
        } else {
            self.state.clone()
        }
    }
}
impl Drop for NativeBookPublicationV3 {
    fn drop(&mut self) { self.release(); }
}
